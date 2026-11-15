// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio "Add": one side panel for everything that can be put next to the waveform. The first page lists what can be
//! added (Cursors, Measure, Search, Results table, Plot, Math, Ref); each kind has a page with a small picture, one
//! line about it, its types and sources, and Add. The panel is the Settings Sheet, so the waveform stays visible and
//! the panel stays open: several things can be added in a row.
//!
//! Also here: the cursor and Edge search settings pages (rows are `Item`s like every other sheet page), the cursor
//! readouts (a box at each cursor and a delta box) and the search itself (its own source, level and slope).
use super::meascatalog::wrap_lines;
use super::sheet::{SHEET_FOOT, SHEET_HEAD, SHEET_ROW};
use super::sheet::PanelKind;
use super::studio::chip;
use super::*;
use std::sync::Arc;

const PAD: i32 = 12;
/// Height of one row of the first page (the whole row is the target).
const RAIL_ROW: i32 = 56;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum AddKind {
    Cursors,
    Measure,
    Search,
    Table,
    Plot,
    Math,
    Ref,
}

/// What the first page lists: the kind, its name and the line under it.
pub(super) const ADD_KINDS: [(AddKind, &str, &str); 7] = [
    (AddKind::Cursors, "Cursors", "Two cursors read time and level"),
    (AddKind::Measure, "Measure", "Amplitude, period, rise time and more"),
    (AddKind::Search, "Search", "Mark every edge on the waveform"),
    (AddKind::Table, "Results table", "Measurements, search events, decode"),
    (AddKind::Plot, "Plot", "XY, eye, histogram, spectrum, trend"),
    (AddKind::Math, "Math", "A waveform calculated from others"),
    (AddKind::Ref, "Ref", "Keep a waveform to compare with"),
];

/// Table types of the Results table page: name and what it shows.
pub(super) const TABLES: [(&str, &str); 3] = [
    ("Measurements", "Every measurement with its statistics, docked at the right."),
    ("Search events", "The time of every search event; tap a row to go there."),
    ("Bus decode", "The decoded packets of the bus, under the waveform."),
];

/// Plot types of the Plot page.
pub(super) const PLOTS: [(&str, &str, PanelKind); 5] = [
    ("XY", "One channel against another: pick X and Y.", PanelKind::Cursor),
    ("Eye", "An eye diagram beside the waveform.", PanelKind::Eye),
    ("Histogram", "How often each level occurs, drawn in the waveform.", PanelKind::Histogram),
    ("Spectrum", "The frequency content, under the waveform.", PanelKind::Spectrum),
    ("Trend", "A measurement over time.", PanelKind::Trend),
];

/// What the Add panel remembers: the page, the chosen types and the chosen source.
#[derive(Default)]
pub(super) struct AddUi {
    pub kind: Option<AddKind>,
    pub table: usize,
    pub plot: usize,
    pub src: Option<WaveId>,
    /// the second source of a page: the Y of XY
    pub src_b: Option<WaveId>,
    /// the measurement chip of the Trend page
    pub meas: usize,
}

/// The buttons at the foot of a page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RailAct {
    Add,
    Remove,
    Settings,
    /// Measure: open the list of measurements
    Choose,
}

/// The Edge search of Studio: its own source, level and slope (0 rising, 1 falling, 2 either).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct SearchSpec {
    pub src: WaveId,
    pub level: f32,
    pub slope: u8,
}

/// The rectangles of the Add panel.
pub(super) struct RailGeo {
    pub rows: Vec<(AddKind, Rect)>,
    pub preview: Rect,
    pub desc: Rect,
    pub types: Vec<(usize, Rect)>,
    pub srcs: Vec<(WaveId, Rect)>,
    /// the second row of sources (the Y of XY)
    pub srcs_b: Vec<(WaveId, Rect)>,
    /// the measurement chips of the Trend page
    pub meas: Vec<(usize, Rect)>,
    /// the labels of the source rows ("X", "Y"), with the room they take at the left
    pub src_labels: Vec<(&'static str, Rect)>,
    pub note: Rect,
    pub foot: Vec<(RailAct, Rect)>,
}

const SLOPES: [&str; 3] = ["Rising", "Falling", "Either"];

impl Ui {
    // ------------------------------------------------------------------------------------------------ the pages

    /// What a page has types for: the names, and the one chosen.
    fn rail_types(&self, kind: AddKind) -> (Vec<&'static str>, usize) {
        match kind {
            AddKind::Table => (TABLES.iter().map(|t| t.0).collect(), self.sheet.add.table.min(TABLES.len() - 1)),
            AddKind::Plot => (PLOTS.iter().map(|t| t.0).collect(), self.sheet.add.plot.min(PLOTS.len() - 1)),
            _ => (Vec::new(), 0),
        }
    }

    /// The source chosen on the Cursors and Search pages: the one tapped, else the selected waveform.
    pub(super) fn rail_source(&self) -> Option<WaveId> {
        let shown = self.sheet_shown_sources();
        self.sheet.add.src.filter(|s| shown.contains(s))
            .or_else(|| Some(self.cursor_source()).filter(|s| shown.contains(s)))
            .or_else(|| shown.first().copied())
    }

    /// The source rows of a page: a label for the row ("" for none) and the waves it offers. Cursors and Search
    /// pick one source; the plots pick what they draw: XY has X and Y, Trend a source and a measurement.
    fn rail_src_rows(&self, kind: AddKind) -> Vec<(&'static str, Vec<WaveId>)> {
        let shown = self.sheet_shown_sources();
        let chans: Vec<WaveId> = shown.iter().copied().filter(|id| matches!(id, WaveId::Ch(_))).collect();
        match kind {
            AddKind::Cursors | AddKind::Search => vec![("", shown)],
            AddKind::Plot => match self.sheet.add.plot.min(PLOTS.len() - 1) {
                0 => vec![("X", chans.clone()), ("Y", chans)],
                2 | 4 => vec![("", chans)],
                _ => vec![("", shown)],
            },
            _ => Vec::new(),
        }
    }

    /// What the chips of a Plot page say is chosen: the tapped one, else what the plot uses now, else the first on.
    pub(super) fn rail_plot_choice(&self, row: usize) -> Option<WaveId> {
        let rows = self.rail_src_rows(AddKind::Plot);
        let cands = &rows.get(row)?.1;
        let plot = self.sheet.add.plot.min(PLOTS.len() - 1);
        let tapped = if row == 0 { self.sheet.add.src } else { self.sheet.add.src_b };
        let now = match (plot, row) {
            (0, 0) => Some(WaveId::Ch(self.xy_src[0])),
            (0, _) => Some(WaveId::Ch(self.xy_src[1])),
            (1, _) => Some(self.m.eye_src),
            (2, _) => self.hist_src().map(WaveId::Ch),
            (3, _) => Some(self.m.spec_src),
            _ => Some(self.cursor_source()),
        };
        tapped.filter(|s| cands.contains(s))
            .or(now.filter(|s| cands.contains(s)))
            .or_else(|| {
                // Y defaults to a channel other than X
                let x = if row == 1 { self.rail_plot_choice(0) } else { None };
                cands.iter().copied().find(|c| Some(*c) != x).or_else(|| cands.first().copied())
            })
    }

    /// The measurement chips of the Trend page.
    pub(super) const TREND_MEAS: [Meas; 4] = [Meas::Vpp, Meas::Freq, Meas::Vrms, Meas::Vavg];

    fn rail_has_meas_chips(&self, kind: AddKind) -> bool {
        kind == AddKind::Plot && self.sheet.add.plot.min(PLOTS.len() - 1) == 4
    }

    fn rail_has_sources(&self, kind: AddKind) -> bool { !self.rail_src_rows(kind).is_empty() }

    pub(super) fn rail_geo(&self) -> RailGeo {
        let (r, _, _) = self.sheet_chrome();
        let w = r.w - 2 * PAD;
        let x = r.x + PAD;
        let mut g = RailGeo { rows: Vec::new(), preview: Rect::new(0, 0, 0, 0), desc: Rect::new(0, 0, 0, 0), types: Vec::new(), srcs: Vec::new(), srcs_b: Vec::new(), meas: Vec::new(), src_labels: Vec::new(), note: Rect::new(0, 0, 0, 0), foot: Vec::new() };
        let Some(kind) = self.sheet.add.kind else {
            g.rows = ADD_KINDS.iter().enumerate().map(|(i, (k, _, _))| (*k, Rect::new(x, r.y + SHEET_HEAD + 6 + i as i32 * (RAIL_ROW + 2), w, RAIL_ROW))).collect();
            return g;
        };
        let (names, _) = self.rail_types(kind);
        // the picture gives way a little when the page has two rows of sources
        let rows = self.rail_src_rows(kind);
        let below = (names.len() as i32 + 2) / 3 * 44 + rows.len() as i32 * 44 + if self.rail_has_meas_chips(kind) { 44 } else { 0 } + 46;
        let avail = r.h - SHEET_HEAD - 6 - 6 - 40 - SHEET_FOOT - below;
        let ph = avail.clamp(60, 88);
        let mut y = r.y + SHEET_HEAD + 6;
        g.preview = Rect::new(x, y, w, ph);
        y += ph + 6;
        g.desc = Rect::new(x, y, w, 36);
        y += 40;
        if !names.is_empty() {
            let cols = 3;
            let cw = (w - (cols - 1) * 4) / cols;
            for i in 0..names.len() as i32 {
                g.types.push((i as usize, Rect::new(x + (i % cols) * (cw + 4), y + (i / cols) * 44, cw, 40)));
            }
            y += (names.len() as i32 + cols - 1) / cols * 44 + 4;
        }
        let src_rows = self.rail_src_rows(kind);
        if !src_rows.is_empty() {
            let labelled = src_rows.len() > 1 && !src_rows[0].0.is_empty();
            let lw = if labelled { 22 } else { 0 };
            for (k, (label, cands)) in src_rows.iter().enumerate() {
                let n = cands.len().clamp(1, 8) as i32;
                let cw = (w - lw - (n - 1) * 4) / n;
                let row: Vec<(WaveId, Rect)> = cands.iter().take(8).enumerate().map(|(i, &id)| (id, Rect::new(x + lw + i as i32 * (cw + 4), y, cw, 40))).collect();
                if labelled { g.src_labels.push((*label, Rect::new(x, y, lw, 40))); }
                if k == 0 { g.srcs = row; } else { g.srcs_b = row; }
                y += 44;
            }
        }
        if self.rail_has_meas_chips(kind) {
            let n = Self::TREND_MEAS.len() as i32;
            let cw = (w - (n - 1) * 4) / n;
            g.meas = (0..Self::TREND_MEAS.len()).map(|i| (i, Rect::new(x + i as i32 * (cw + 4), y, cw, 40))).collect();
            y += 44;
        }
        g.note = Rect::new(x, y + 2, w, 44);
        let acts = self.rail_acts(kind);
        let fy = r.bottom() - SHEET_FOOT + 6;
        g.foot = match acts.len() {
            0 => Vec::new(),
            1 => vec![(acts[0], Rect::new(x, fy, w, 44))],
            _ => {
                let half = (w - 8) / 2;
                vec![(acts[0], Rect::new(x, fy, half, 44)), (acts[1], Rect::new(x + half + 8, fy, half, 44))]
            }
        };
        g
    }

    /// The buttons of a page: Add, or Settings and Remove once it is on the screen.
    fn rail_acts(&self, kind: AddKind) -> Vec<RailAct> {
        match kind {
            AddKind::Cursors if self.m.cursor != CursorMode::Off => vec![RailAct::Settings, RailAct::Remove],
            AddKind::Search if self.studio.search.is_some() => vec![RailAct::Settings, RailAct::Remove],
            AddKind::Table if self.rail_table_shown(self.sheet.add.table) => vec![RailAct::Remove],
            AddKind::Plot if self.rail_plot_shown(self.sheet.add.plot) => {
                if self.rail_plot_menu(self.sheet.add.plot).is_some() { vec![RailAct::Settings, RailAct::Remove] } else { vec![RailAct::Remove] }
            }
            AddKind::Cursors | AddKind::Search | AddKind::Table | AddKind::Plot => vec![RailAct::Add],
            AddKind::Measure => vec![RailAct::Choose],
            _ => Vec::new(),
        }
    }

    fn rail_table_shown(&self, i: usize) -> bool {
        match i {
            0 => self.panel_shown(PanelKind::Measurements),
            1 => self.table == Some(tables::TableKind::Search),
            _ => self.table == Some(tables::TableKind::Decode),
        }
    }

    fn rail_plot_shown(&self, i: usize) -> bool {
        let (_, _, k) = PLOTS[i.min(PLOTS.len() - 1)];
        if i == 0 { self.xy } else { self.panel_shown(k) }
    }

    fn rail_plot_menu(&self, i: usize) -> Option<MenuId> {
        match PLOTS[i.min(PLOTS.len() - 1)].0 {
            "Eye" => Some(MenuId::Eye),
            "Spectrum" => Some(MenuId::Spectrum),
            _ => None,
        }
    }

    /// What the line under the buttons says about the state of the page.
    fn rail_note(&self, kind: AddKind) -> String {
        match kind {
            AddKind::Cursors => match self.m.cursor {
                CursorMode::Off => "Cursors are off".to_string(),
                _ => format!("Cursors on {}", self.wave_name(self.cursor_source())),
            },
            AddKind::Search => match self.studio.search {
                Some(sp) => { let n = self.studio_search_events().len(); format!("Search 1 on {}: {} event{}", self.wave_name(sp.src), n, if n == 1 { "" } else { "s" }) }
                None => "No search yet".to_string(),
            },
            AddKind::Table => {
                let i = self.sheet.add.table.min(TABLES.len() - 1);
                if self.rail_table_shown(i) { "On the screen".to_string() } else if i == 2 && self.value(Item::DecOn) != "On" { "Needs a decoder: turn one on in Menu".to_string() } else { String::new() }
            }
            AddKind::Plot => if self.rail_plot_shown(self.sheet.add.plot) { "On the screen".to_string() } else { String::new() },
            AddKind::Measure => match self.measurement_count() { 0 => "No measurements yet".to_string(), 1 => "1 measurement on the screen".to_string(), n => format!("{n} measurements on the screen") },
            _ => String::new(),
        }
    }

    // ------------------------------------------------------------------------------------------------ drawing

    pub(super) fn draw_sheet_add_panel(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let kind = self.sheet.add.kind;
        let title = match kind { Some(k) => ADD_KINDS.iter().find(|e| e.0 == k).map_or("Add", |e| e.1), None => "Add" };
        self.sheet_draw_chrome(cv, title);
        let g = self.rail_geo();
        let Some(kind) = kind else {
            for &(k, r) in &g.rows {
                let (_, name, sub) = ADD_KINDS.iter().find(|e| e.0 == k).copied().unwrap();
                let on = self.rail_kind_on(k);
                btn(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
                cv.set_clip(Rect::new(r.x, r.y, r.w - 30, r.h));
                cv.text(&f.mb, r.x + 12, r.y + 24, name, TEXT);
                cv.text(&f.s, r.x + 12, r.y + 44, sub, DIM);
                cv.no_clip();
                if on { cv.text_in(&f.sb, Rect::new(r.right() - 64, r.y, 40, 30), 2, "On", ACCENT); }
                let (cx, cy) = (r.right() - 14, r.y + r.h / 2);
                cv.line(cx - 3, cy - 6, cx + 3, cy, DIM);
                cv.line(cx + 3, cy, cx - 3, cy + 6, DIM);
            }
            return;
        };
        let id = self.rail_preview_id(kind);
        self.draw_rail_preview(cv, g.preview, id);
        let desc = self.rail_desc(kind);
        for (i, line) in wrap_lines(&f.s, &desc, g.desc.w).into_iter().take(2).enumerate() {
            cv.text(&f.s, g.desc.x, g.desc.y + 14 + i as i32 * 16, &line, DIM);
        }
        let (names, cur) = self.rail_types(kind);
        for &(i, r) in &g.types {
            let on = i == cur;
            if on {
                cv.round(r, 5, rgb(0x2e, 0x2e, 0x32), 255);
                cv.round_outline(r, 5, ACCENT);
            } else {
                btn(cv, r, BTN, BTN_BORDER);
            }
            cv.text_in(&f.sb, r, 1, names[i], if on { TEXT } else { DIM });
        }
        let is_plot = kind == AddKind::Plot;
        let chosen = if is_plot { self.rail_plot_choice(0) } else { self.rail_source() };
        let chosen_b = if is_plot { self.rail_plot_choice(1) } else { None };
        for (row, pick) in [(&g.srcs, chosen), (&g.srcs_b, chosen_b)] {
            for &(sid, r) in row {
                let on = Some(sid) == pick;
                if on { cv.round(r, 5, self.wave_color(sid), 255); } else { btn(cv, r, BTN, self.wave_color(sid)); }
                cv.text_in(&f.mb, r, 1, &self.wave_name(sid), if on { DARK_TEXT } else { TEXT });
            }
        }
        for &(label, r) in &g.src_labels {
            cv.text_in(&f.sb, r, 1, label, DIM);
        }
        let mchosen = self.sheet.add.meas.min(Self::TREND_MEAS.len() - 1);
        for &(i, r) in &g.meas {
            let on = i == mchosen;
            if on { cv.round(r, 5, rgb(0x2e, 0x2e, 0x32), 255); cv.round_outline(r, 5, ACCENT); } else { btn(cv, r, BTN, BTN_BORDER); }
            cv.text_in(&f.sb, r, 1, Self::TREND_MEAS[i].name(), if on { TEXT } else { DIM });
        }
        if self.rail_has_sources(kind) && g.srcs.is_empty() {
            cv.text_in(&f.m, Rect::new(g.note.x, g.note.y - 44, g.note.w, 40), 0, "No source is on: turn on a channel", DIM);
        }
        let note = self.rail_note(kind);
        if !note.is_empty() {
            cv.text_in(&f.m, g.note, 0, &note, if note.starts_with("Needs") { AMBER } else { TEXT });
        }
        if !g.foot.is_empty() {
            let (r, _, _) = self.sheet_chrome();
            cv.hline(r.x + 6, r.right() - 7, r.bottom() - SHEET_FOOT, LINE);
        }
        for &(act, b) in &g.foot {
            let red = act == RailAct::Remove;
            btn(cv, b, if red { mix(DLG_BG, RED, 30) } else if matches!(act, RailAct::Add | RailAct::Choose) { mix(BTN, ACCENT, 60) } else { BTN }, if red { RED } else if matches!(act, RailAct::Add | RailAct::Choose) { ACCENT } else { BTN_BORDER });
            let label = match act { RailAct::Add => "Add", RailAct::Remove => "Remove", RailAct::Settings => "Settings", RailAct::Choose => "Choose a measurement" };
            cv.text_in(&f.mb, b, 1, label, if red { RED } else { TEXT });
        }
    }

    /// A kind that is on the screen now.
    fn rail_kind_on(&self, k: AddKind) -> bool {
        match k {
            AddKind::Cursors => self.m.cursor != CursorMode::Off,
            AddKind::Measure => self.measurement_count() > 0,
            AddKind::Search => self.studio.search.is_some(),
            AddKind::Table => (0..TABLES.len()).any(|i| self.rail_table_shown(i)),
            AddKind::Plot => (0..PLOTS.len()).any(|i| self.rail_plot_shown(i)),
            AddKind::Math => self.m.math != MathOp::Off,
            AddKind::Ref => self.refs.iter().any(Option::is_some),
        }
    }

    fn rail_preview_id(&self, kind: AddKind) -> &'static str {
        match kind {
            AddKind::Cursors => "cursors",
            AddKind::Search => "search",
            AddKind::Table => ["table_meas", "table_search", "table_decode"][self.sheet.add.table.min(2)],
            AddKind::Plot => ["xy", "eye", "hist", "spectrum", "trend"][self.sheet.add.plot.min(4)],
            AddKind::Measure => "measure",
            AddKind::Math => "math",
            AddKind::Ref => "ref",
        }
    }

    fn rail_desc(&self, kind: AddKind) -> String {
        match kind {
            AddKind::Cursors => "Two waveform cursors on the chosen trace, with a reading at each and the difference between them.".into(),
            AddKind::Search => "Marks every edge of the source on the waveform and counts them. Level and slope are set to the signal.".into(),
            AddKind::Table => TABLES[self.sheet.add.table.min(TABLES.len() - 1)].1.into(),
            AddKind::Plot => PLOTS[self.sheet.add.plot.min(PLOTS.len() - 1)].1.into(),
            AddKind::Measure => "Amplitude, frequency, rise time and more, read from a waveform. Pick one and it joins the readout.".into(),
            AddKind::Math => "A waveform calculated from others.".into(),
            AddKind::Ref => "A waveform kept to compare with.".into(),
        }
    }

    /// The small picture of a page: a dark graticule with a drawing of what the page adds.
    pub(super) fn draw_rail_preview(&self, cv: &mut Canvas, r: Rect, id: &str) {
        let f = self.fonts();
        cv.round(r, 6, LINE, 255);
        let i = r.inset(1);
        cv.fill(i, rgb(0x08, 0x0a, 0x10));
        for k in 1..8 { cv.vline(i.x + i.w * k / 8, i.y + 1, i.bottom() - 2, GRID_LINE); }
        for k in 1..4 { cv.hline(i.x + 1, i.right() - 2, i.y + i.h * k / 4, GRID_LINE); }
        let mid = i.y + i.h / 2;
        let amp = (i.h / 2 - 12) as f32;
        let at = |t: f32| i.x + 4 + (t * (i.w - 8) as f32) as i32;
        let curve = |cv: &mut Canvas, col: Rgb, f: &dyn Fn(f32) -> f32| {
            let mut prev: Option<(i32, i32)> = None;
            for px in 0..=(i.w - 8) {
                let t = px as f32 / (i.w - 8) as f32;
                let p = (i.x + 4 + px, mid - (f(t) * amp) as i32);
                if let Some(q) = prev { cv.line(q.0, q.1, p.0, p.1, col); }
                prev = Some(p);
            }
        };
        let sine = |t: f32| (t * std::f32::consts::TAU * 2.0).sin();
        let square = |t: f32| if ((t * 4.0 + 0.125) % 1.0) < 0.5 { 0.8 } else { -0.8 };
        let yellow = CH_COLORS[0];
        match id {
            "cursors" => {
                curve(cv, yellow, &sine);
                for (t, name) in [(0.3f32, "A"), (0.7f32, "B")] {
                    let x = at(t);
                    cv.dashed_vline(x, i.y + 2, i.bottom() - 3, 4, 3, CURSOR);
                    let b = Rect::new(x + 3, i.y + 4, 40, 14);
                    cv.round(b, 2, CURSOR, 255);
                    cv.text_in(&f.s, b, 1, name, DARK_TEXT);
                }
                let b = Rect::new(i.x + i.w / 2 - 42, i.bottom() - 22, 84, 16);
                cv.round(b, 2, BAR, 255);
                cv.text_in(&f.s, b, 1, "\u{394}t  \u{394}v", TEXT);
            }
            "search" => {
                curve(cv, yellow, &square);
                for k in 0..4 {
                    let x = at(0.125 + k as f32 * 0.25);
                    cv.triangle([(x - 4, i.y + 3), (x + 4, i.y + 3), (x, i.y + 10)], AMBER);
                    cv.dashed_vline(x, i.y + 11, i.bottom() - 3, 2, 6, mix(BG, AMBER, 90));
                }
            }
            "table_meas" | "table_search" | "table_decode" => {
                for row in 0..5 {
                    let y = i.y + 8 + row * 15;
                    let head = row == 0;
                    cv.fill(Rect::new(i.x + 8, y, i.w - 16, 12), if head { LINE } else { mix(BG, LINE, 90) });
                    let cols: [i32; 4] = if id == "table_decode" { [30, 70, 100, 60] } else { [60, 60, 60, 60] };
                    let mut x = i.x + 14;
                    for (c, wcol) in cols.iter().enumerate() {
                        let col = if head { TEXT } else if id == "table_search" && c == 0 { AMBER } else { DIM };
                        cv.fill(Rect::new(x, y + 5, (*wcol * 2 / 3).max(8), 3), col);
                        x += *wcol + 10;
                    }
                }
            }
            "xy" => {
                let mut prev: Option<(i32, i32)> = None;
                let side = (i.h - 12) / 2;
                for k in 0..=120 {
                    let t = k as f32 / 120.0 * std::f32::consts::TAU;
                    let p = (i.x + i.w / 2 + (t.sin() * side as f32 * 1.4) as i32, mid - ((2.0 * t + 0.8).sin() * side as f32) as i32);
                    if let Some(q) = prev { cv.line(q.0, q.1, p.0, p.1, rgb(0x9c, 0xf0, 0x9c)); }
                    prev = Some(p);
                }
            }
            "eye" => {
                let s = |t: f32| t * t * (3.0 - 2.0 * t);
                for k in 0..6 {
                    let d = (k as f32 - 2.5) * 0.012;
                    curve(cv, yellow, &|t: f32| s((t + d).clamp(0.0, 1.0)) * 1.6 - 0.8);
                    curve(cv, yellow, &|t: f32| 0.8 - s((t + d).clamp(0.0, 1.0)) * 1.6);
                }
                curve(cv, yellow, &|_| 0.8);
                curve(cv, yellow, &|_| -0.8);
            }
            "hist" => {
                for row in 0..9 {
                    let y = i.y + 6 + row * (i.h - 12) / 9;
                    let d = (row as f32 - 4.0) / 2.2;
                    let wbar = ((-d * d).exp() * (i.w as f32 * 0.55)) as i32;
                    cv.fill(Rect::new(i.x + 6, y, wbar.max(3), (i.h - 12) / 9 - 2), mix(BG, yellow, 200));
                }
            }
            "spectrum" => {
                curve(cv, yellow, &|t: f32| -0.75 + 0.1 * (t * 53.0).sin().abs() + if (t - 0.2).abs() < 0.012 { 1.2 } else if (t - 0.4).abs() < 0.012 { 0.7 } else if (t - 0.6).abs() < 0.012 { 0.4 } else { 0.0 });
            }
            "measure" => {
                curve(cv, yellow, &square);
                // the levels a rise time is read between, and the readout that carries the value
                let (lo, hi) = (mid + (amp * 0.8) as i32, mid - (amp * 0.8) as i32);
                for y in [hi, lo] { cv.dashed_hline(i.x + 3, i.right() - 4, y, 3, 3, mix(BG, TEXT, 120)); }
                let b = Rect::new(i.right() - 98, i.y + 6, 92, 16);
                cv.round(b, 3, BAR, 255);
                cv.text_in(&f.s, b, 1, "Pk-Pk 1.6 V", TEXT);
                let b = Rect::new(i.x + 6, i.bottom() - 22, 80, 16);
                cv.round(b, 3, BAR, 255);
                cv.text_in(&f.s, b, 1, "Freq 1 kHz", TEXT);
            }
            "trend" => {
                curve(cv, yellow, &|t: f32| 0.5 * (t * 7.0).sin() + 0.2 * (t * 23.0).sin() - 0.1);
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------------------------------------ taps

    pub(super) fn sheet_add_panel_tap(&mut self, x: i32, y: i32) -> bool {
        let g = self.rail_geo();
        let Some(kind) = self.sheet.add.kind else {
            if let Some(&(k, _)) = g.rows.iter().find(|(_, r)| r.contains(x, y)) { self.rail_open(k); }
            return true;
        };
        if let Some(&(i, _)) = g.types.iter().find(|(_, r)| r.contains(x, y)) {
            if kind == AddKind::Table { self.sheet.add.table = i; } else {
                if self.sheet.add.plot != i { (self.sheet.add.src, self.sheet.add.src_b) = (None, None); }
                self.sheet.add.plot = i;
            }
            return true;
        }
        if let Some(&(id, _)) = g.srcs.iter().find(|(_, r)| r.contains(x, y)) {
            self.sheet.add.src = Some(id);
            self.rail_plot_live(kind);
            return true;
        }
        if let Some(&(id, _)) = g.srcs_b.iter().find(|(_, r)| r.contains(x, y)) {
            self.sheet.add.src_b = Some(id);
            self.rail_plot_live(kind);
            return true;
        }
        if let Some(&(i, _)) = g.meas.iter().find(|(_, r)| r.contains(x, y)) {
            self.sheet.add.meas = i;
            self.rail_plot_live(kind);
            return true;
        }
        if let Some(&(act, _)) = g.foot.iter().find(|(_, r)| r.contains(x, y)) { self.rail_do(kind, act); }
        true
    }

    /// A row of the first page: Math and Ref open the page that already exists, the others get their own.
    fn rail_open(&mut self, k: AddKind) {
        match k {
            AddKind::Math => self.open_submenu(MenuId::Math),
            AddKind::Ref => self.open_submenu(MenuId::Ref),
            _ => {
                self.sheet.add.kind = Some(k);
                (self.sheet.add.src, self.sheet.add.src_b) = (None, None);
            }
        }
    }

    /// The Choose button of the Measure page: the list of measurements, in the same sheet.
    fn rail_measure_choose(&mut self) {
        self.ensure_meas_own();
        self.open_submenu(MenuId::MeasAdd);
        self.sheet_add_open();
    }

    /// A source tapped while its plot is on the screen changes the plot at once.
    fn rail_plot_live(&mut self, kind: AddKind) {
        if kind == AddKind::Plot && self.rail_plot_shown(self.sheet.add.plot) {
            self.rail_plot_apply_sources(self.sheet.add.plot);
            self.relayout();
        }
    }

    /// Gives the plot the sources the chips show: XY its X and Y, the histogram its channel, the eye and the
    /// spectrum their source, Trend a measurement of the source in its first chart.
    pub(super) fn rail_plot_apply_sources(&mut self, i: usize) {
        let (a, b) = (self.rail_plot_choice(0), self.rail_plot_choice(1));
        match i {
            0 => {
                if let (Some(WaveId::Ch(x)), Some(WaveId::Ch(y))) = (a, b) { self.xy_src = [x, y]; }
            }
            1 => if let Some(id) = a { if id != self.m.eye_src { let n = self.wave_name(id); self.select_option(Item::EyeSrc, &n); } },
            2 => if let Some(WaveId::Ch(c)) = a { self.hist_ch = Some(c); },
            3 => if let Some(id) = a { if id != self.m.spec_src { let n = self.wave_name(id); self.select_option(Item::SpecSrc, &n); } },
            _ => if let Some(src) = a {
                let k = Self::TREND_MEAS[self.sheet.add.meas.min(Self::TREND_MEAS.len() - 1)];
                if self.meas_add(k, src).is_ok() {
                    if let Some(p) = self.m.meas.iter().position(|e| *e == (k, src)) { self.m.trend_sel[0] = 2 + p; }
                }
            },
        }
    }

    fn rail_do(&mut self, kind: AddKind, act: RailAct) {
        match (kind, act) {
            (AddKind::Cursors, RailAct::Add) => { let s = self.rail_source(); self.studio_cursors_add(s); }
            (AddKind::Cursors, RailAct::Settings) => self.open_submenu(MenuId::Cursor),
            (AddKind::Cursors, RailAct::Remove) => self.studio_cursors_remove(),
            (AddKind::Search, RailAct::Add) => { let s = self.rail_source(); self.studio_search_add(s); }
            (AddKind::Search, RailAct::Settings) => self.open_submenu(MenuId::Search),
            (AddKind::Search, RailAct::Remove) => self.studio_search_remove(),
            (AddKind::Table, RailAct::Add) => self.rail_table_add(self.sheet.add.table),
            (AddKind::Table, RailAct::Remove) => self.rail_table_remove(self.sheet.add.table),
            (AddKind::Plot, RailAct::Add) => { self.rail_plot_apply_sources(self.sheet.add.plot); self.rail_plot_add(self.sheet.add.plot) }
            (AddKind::Measure, RailAct::Choose) => self.rail_measure_choose(),
            (AddKind::Plot, RailAct::Remove) => self.rail_plot_remove(self.sheet.add.plot),
            (AddKind::Plot, RailAct::Settings) => if let Some(m) = self.rail_plot_menu(self.sheet.add.plot) { self.open_submenu(m); },
            _ => {}
        }
        self.relayout();
    }

    fn rail_table_add(&mut self, i: usize) {
        match i {
            0 => self.studio_set_view(super::studio::StudioView::Docked),
            1 => {
                if self.studio.search.is_none() {
                    let s = self.rail_source();
                    self.studio_search_add(s);
                }
                self.table = Some(tables::TableKind::Search);
                self.table_top = 0;
            }
            _ => {
                if self.value(Item::DecOn) != "On" { return self.toast("Turn a decoder on first (Menu)"); }
                self.set_decode_table(true);
            }
        }
    }

    fn rail_table_remove(&mut self, i: usize) {
        if i == 0 { self.studio_set_view(super::studio::StudioView::Waveform); } else { self.table = None; }
    }

    fn rail_plot_add(&mut self, i: usize) {
        if i == 0 { return self.select_option(Item::XyMode, "XY"); }
        self.studio_apply_panel(PLOTS[i.min(PLOTS.len() - 1)].2);
    }

    fn rail_plot_remove(&mut self, i: usize) {
        match PLOTS[i.min(PLOTS.len() - 1)].0 {
            "XY" => self.select_option(Item::XyMode, "YT"),
            "Eye" => if self.m.view == AView::Eye { self.adjust(Item::EyeOn, 1) },
            "Histogram" => self.select_option(Item::Histogram, "Off"),
            _ => self.select_option(Item::PaneSel, "Traces"),
        }
    }

    // ------------------------------------------------------------------------------------------------ cursors

    /// 1/dt as a frequency; "----" when the cursors sit on the same time.
    pub(super) fn inv_dt(dt: f64) -> String {
        if dt.is_finite() && dt.abs() >= 1e-15 { si(1.0 / dt.abs(), "Hz") } else { "----".into() }
    }

    /// One tap: two waveform cursors on the chosen trace (the selected one when none is given).
    pub(super) fn studio_cursors_add(&mut self, src: Option<WaveId>) {
        if let Some(s) = src { self.set_cursor_source(s); }
        self.m.cur_b_source = None;
        self.m.cursor = CursorMode::Track;
        // apart: a third and two thirds of the visible span
        (self.m.cur_a, self.m.cur_b, self.m.cur_c, self.m.cur_d) = (1.0 / 3.0, 2.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0);
        self.cursor_view = None;
        self.settings_changed();
        self.relayout();
    }

    pub(super) fn studio_cursors_remove(&mut self) {
        self.m.cursor = CursorMode::Off;
        self.m.cur_b_source = None;
        if self.menu_open && self.menu == MenuId::Cursor { self.close_menu(); }
        self.settings_changed();
        self.relayout();
    }

    /// The Cursors button: the first tap puts the cursors on, the next ones open their settings.
    pub(super) fn studio_cursors_tap(&mut self) {
        if self.m.cursor == CursorMode::Off {
            if self.menu_open { self.close_menu(); }
            self.studio_cursors_add(None);
        } else {
            self.toggle_menu(MenuId::Cursor);
        }
    }

    fn cursor_type_names() -> [&'static str; 4] { ["Waveform", "V bars", "H bars", "V and H"] }
    const CURSOR_MODES: [CursorMode; 4] = [CursorMode::Track, CursorMode::Time, CursorMode::Volt, CursorMode::Xy];

    fn set_cursor_type(&mut self, mode: CursorMode) {
        let was = self.m.cursor;
        self.m.cursor = mode;
        let off_view = |f: f32| !(0.0..=1.0).contains(&f);
        if was == CursorMode::Off || (was == CursorMode::Volt) != (mode == CursorMode::Volt) || off_view(self.m.cur_a) || off_view(self.m.cur_b) {
            (self.m.cur_a, self.m.cur_b) = (0.3, 0.7);
        }
        if mode == CursorMode::Xy && was != CursorMode::Xy { (self.m.cur_c, self.m.cur_d) = (0.3, 0.7); }
        self.settings_changed();
        self.relayout();
    }

    pub(super) fn addrail_cursor_items(&self) -> Vec<Item> {
        if self.m.cursor == CursorMode::Off { return vec![Item::CurType, Item::CurSrc]; }
        let mut v = vec![Item::CurType, Item::CurSrc, Item::CurSrcB, Item::CurReadout, Item::CurLink, Item::CurA, Item::CurB];
        if self.m.cursor == CursorMode::Xy { v.extend([Item::CurC, Item::CurD]); }
        v.push(Item::CurRemove);
        v
    }

    /// The cursor readouts at each cursor and the delta box, drawn into the lane `a`.
    pub(super) fn draw_studio_cursor_readouts(&self, cv: &mut Canvas, a: Rect) {
        if self.m.layout != Layout::Studio || self.studio.cur_badge { return; }
        let f = self.fonts();
        let source = self.cursor_source();
        let second = self.cursor_b_source();
        let (ta, tb) = self.cursor_times();
        let (unit, unit_b) = (self.cursor_source_unit(source), self.cursor_source_unit(second));
        let boxed = |cv: &mut Canvas, x: i32, y: i32, text: &str| {
            let w = f.s.width(text) + 10;
            let r = Rect::new(x.clamp(a.x + 2, (a.right() - w - 2).max(a.x + 2)), y, w, 18);
            cv.round(r, 3, BAR, 255);
            cv.round_outline(r, 3, CURSOR);
            cv.text_in(&f.s, r, 1, text, TEXT);
        };
        let mode = self.m.cursor;
        let vertical = matches!(mode, CursorMode::Time | CursorMode::Track | CursorMode::Xy);
        let (mut va, mut vb) = (None, None);
        if vertical {
            if mode == CursorMode::Track {
                va = self.wave_at_source(source, ta);
                vb = self.wave_at_source(second, tb);
            }
            for (k, (frac, t, v, u)) in [(self.m.cur_a, ta, va, unit), (self.m.cur_b, tb, vb, unit_b)].into_iter().enumerate() {
                if !(0.0..=1.0).contains(&frac) { continue; }
                let x = a.x + (frac * a.w as f32) as i32;
                let text = match (mode, v) {
                    (CursorMode::Track, Some(v)) => format!("{}  {}", si(t, "s"), si(v as f64, u)),
                    (CursorMode::Track, None) => format!("{}  ----", si(t, "s")),
                    _ => si(t, "s"),
                };
                boxed(cv, x + 3, a.y + 4 + k as i32 * 20, &text);
            }
        }
        // the levels of the horizontal bars
        let area = self.source_area(source).unwrap_or(self.area);
        let (scale, offset) = self.cursor_source_context().map_or((1.0, 0.0), |c| (c.1, c.2));
        let mut dv: Option<f64> = None;
        if matches!(mode, CursorMode::Volt | CursorMode::Xy) {
            let (fa, fb) = if mode == CursorMode::Volt { (self.m.cur_a, self.m.cur_b) } else { (self.m.cur_c, self.m.cur_d) };
            let level = |fr: f32| (self.zoom_divs_at(area, fr) * scale - offset) as f64;
            let (la, lb) = (level(fa), level(fb));
            for (fr, l) in [(fa, la), (fb, lb)] {
                let y = a.y + (fr * a.h as f32) as i32;
                boxed(cv, a.x + 4, (y - 20).max(a.y + 2), &si(l, unit));
            }
            dv = Some(lb - la);
        } else if let (Some(x), Some(y)) = (va, vb) {
            if unit == unit_b { dv = Some((y - x) as f64); }
        }
        // the delta box
        let mut lines: Vec<String> = Vec::new();
        if vertical {
            let dt = tb - ta;
            lines.push(format!("\u{394}t {}   1/\u{394}t {}", si(dt, "s"), Self::inv_dt(dt)));
        }
        if let Some(dv) = dv {
            let mut line = format!("\u{394}v {}", si(dv, unit));
            if vertical && (tb - ta).abs() > 0.0 { line.push_str(&format!("   \u{394}v/\u{394}t {}", si(dv / (tb - ta), &format!("{unit}/s")))); }
            lines.push(line);
        }
        if lines.is_empty() { return; }
        let w = lines.iter().map(|l| f.s.width(l)).max().unwrap_or(0) + 16;
        let h = 8 + lines.len() as i32 * 16;
        let cx = if vertical { a.x + ((self.m.cur_a + self.m.cur_b) * 0.5 * a.w as f32) as i32 } else { a.x + a.w / 2 };
        let r = Rect::new((cx - w / 2).clamp(a.x + 2, (a.right() - w - 2).max(a.x + 2)), a.bottom() - h - 8, w, h);
        cv.round(r, 4, CURSOR, 255);
        cv.round(r.inset(1), 3, BAR, 255);
        for (i, l) in lines.iter().enumerate() { cv.text(&f.s, r.x + 8, r.y + 16 + i as i32 * 16, l, TEXT); }
    }

    /// The cursors in the overview of a zoomed view: a line at each cursor's time.
    pub(super) fn draw_studio_overview_cursors(&self, cv: &mut Canvas, o: Rect) {
        if self.m.layout != Layout::Studio || !matches!(self.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) { return; }
        let f = self.fonts();
        let (ta, tb) = self.cursor_times();
        let tdiv = self.m.time_per_div();
        if !(tdiv > 0.0) { return; }
        let ppx = o.w as f32 / HDIV;
        for (t, name) in [(ta, "A"), (tb, "B")] {
            let x = o.x + (o.w as f32 / 2.0 + (t as f32 - self.m.delay) / tdiv * ppx) as i32;
            if x < o.x || x >= o.right() { continue; }
            cv.dashed_vline(x, o.y, o.bottom() - 1, 4, 3, CURSOR);
            let b = Rect::new(x + 2, o.y + 2, 16, 14);
            cv.round(b, 2, CURSOR, 255);
            cv.text_in(&f.s, b, 1, name, DARK_TEXT);
        }
    }

    // ------------------------------------------------------------------------------------------------ search

    pub(super) fn addrail_search_items(&self) -> Vec<Item> {
        vec![Item::SrchSrc, Item::SrchSlope, Item::SrchLevel, Item::Srch50, Item::SrchFromTrig, Item::SrchToTrig, Item::SrchEvent, Item::SrchRemove]
    }

    /// Search 1 is added with the defaults the signal asks for: Edge on `src`, the level at 50 %, rising.
    pub(super) fn studio_search_add(&mut self, src: Option<WaveId>) {
        let Some(src) = src.or_else(|| self.rail_source()) else { return self.toast("No source is on") };
        self.studio.search = Some(SearchSpec { src, level: 0.0, slope: 2 });
        self.studio_search_set_50();
        self.studio.search_at = 0;
    }

    pub(super) fn studio_search_remove(&mut self) {
        self.studio.search = None;
        self.studio.search_at = 0;
        if self.table == Some(tables::TableKind::Search) && !self.search_on { self.table = None; }
        if self.menu_open && self.menu == MenuId::Search { self.close_menu(); }
    }

    /// The middle of what the source's samples reach.
    fn studio_search_mid(&self, src: WaveId) -> Option<f32> {
        let (w, _, _) = self.deskew_samples(src)?;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for &v in w.iter().filter(|v| v.is_finite()) { lo = lo.min(v); hi = hi.max(v); }
        (lo <= hi).then(|| (lo + hi) / 2.0)
    }

    pub(super) fn studio_search_set_50(&mut self) {
        let Some(sp) = self.studio.search else { return };
        if let Some(mid) = self.studio_search_mid(sp.src) { self.studio.search = Some(SearchSpec { level: mid, ..sp }); }
    }

    fn studio_search_vpd(&self, src: WaveId) -> f32 {
        match src {
            WaveId::Ch(c) if c < n_ch() => self.m.ch[c].volts_per_div(),
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).map_or(1.0, |r| r.vpd),
            _ => 1.0,
        }
    }

    /// The events of Search 1 (times relative to the trigger, as the trigger search has them). Cached for the record.
    pub(super) fn studio_search_events(&self) -> Arc<Vec<f64>> {
        let none = || Arc::new(Vec::new());
        let Some(sp) = self.studio.search else { return none() };
        let Some((w, dt, t0)) = self.deskew_samples(sp.src) else { return none() };
        if w.len() < 2 || !(dt > 0.0) { return none(); }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (w.as_ptr() as usize, w.len(), dt.to_bits(), t0.to_bits(), sp.level.to_bits(), sp.slope).hash(&mut h);
        format!("{:?}", sp.src).hash(&mut h);
        for k in 0..16 { w[k * (w.len() - 1) / 15].to_bits().hash(&mut h); }
        let key = h.finish();
        if let Some((k, ev)) = &*self.studio.search_cache.borrow() { if *k == key { return ev.clone(); } }
        let hyst = (0.1 * self.studio_search_vpd(sp.src)).max(1e-9);
        let level = sp.level;
        let (mut armed_up, mut armed_down) = (w[0] < level - hyst, w[0] > level + hyst);
        let mut out = Vec::new();
        for i in 1..w.len() {
            let (p, v) = (w[i - 1], w[i]);
            if !v.is_finite() || !p.is_finite() { continue; }
            if v < level - hyst { armed_up = true; }
            if v > level + hyst { armed_down = true; }
            let cross = |out: &mut Vec<f64>| {
                let f = if (v - p).abs() > 0.0 { ((level - p) / (v - p)).clamp(0.0, 1.0) } else { 0.0 };
                out.push(t0 + (i - 1) as f64 * dt + f as f64 * dt);
            };
            if armed_up && v >= level && p < level && sp.slope != 1 { cross(&mut out); armed_up = false; }
            if armed_down && v <= level && p > level && sp.slope != 0 { cross(&mut out); armed_down = false; }
            if out.len() >= 100_000 { break; }
        }
        let ev = Arc::new(out);
        *self.studio.search_cache.borrow_mut() = Some((key, ev.clone()));
        ev
    }

    /// The events to draw and list while Studio has a search of its own.
    pub(super) fn studio_search_marks(&self) -> Option<Arc<Vec<f64>>> {
        (self.m.layout == Layout::Studio && self.studio.search.is_some()).then(|| self.studio_search_events())
    }

    /// Next (1) or previous (-1) event: the view goes there.
    pub(super) fn studio_search_go(&mut self, d: i32) {
        let ev = self.studio_search_events();
        if ev.is_empty() { return self.toast("No events"); }
        let n = ev.len() as i32;
        let at = (self.studio.search_at as i32 + d).rem_euclid(n) as usize;
        self.studio.search_at = at;
        self.go_to_time(ev[at]);
    }

    fn studio_search_copy_from_trigger(&mut self) {
        let Some(sp) = self.studio.search else { return };
        let src = match self.m.trigger_wave() { w @ WaveId::Ch(_) => w, _ => WaveId::Ch(self.m.trig_src.min(n_ch() - 1)) };
        let slope = match self.m.slope { Slope::Rise => 0, Slope::Fall => 1, _ => 2 };
        self.studio.search = Some(SearchSpec { src, level: self.m.trig_level, slope, ..sp });
    }

    fn studio_search_copy_to_trigger(&mut self) {
        let Some(sp) = self.studio.search else { return };
        let WaveId::Ch(c) = sp.src else { return self.toast("The trigger takes a channel") };
        self.m.trig_type = TrigType::Edge;
        self.m.trig_ref = None;
        self.m.trig_src = c;
        self.m.trig_level = sp.level;
        self.m.slope = match sp.slope { 0 => Slope::Rise, 1 => Slope::Fall, _ => Slope::Either };
        self.settings_changed();
    }

    /// The search chip of the badge row: previous, the label, next.
    pub(super) fn studio_search_chip(&self) -> Option<(Rect, Rect, Rect, String)> {
        self.studio.search?;
        let f = self.fonts();
        let row = self.studio_row();
        let n = self.studio_search_events().len();
        let text = format!("Search 1 \u{b7} {} event{}", n, if n == 1 { "" } else { "s" });
        let w = f.sb.width(&text) + 24;
        // a target of 40 px, a little taller than the row
        let h = 40;
        let y = row.y + (row.h - h) / 2;
        let prev = Rect::new(row.x + 4, y, 40, h);
        let label = Rect::new(prev.right() + 2, y, w, h);
        let next = Rect::new(label.right() + 2, y, 40, h);
        Some((prev, label, next, text))
    }

    pub(super) fn draw_studio_search_chip(&self, cv: &mut Canvas) {
        let Some((prev, label, next, text)) = self.studio_search_chip() else { return };
        let f = self.fonts();
        let h = self.studio_chip_h();
        let shrink = |r: Rect| Rect::new(r.x, r.y + (r.h - h) / 2, r.w, h);
        let (prev_hit, next_hit, label_hit) = (prev, next, label);
        let (prev, next, label) = (shrink(prev_hit), shrink(next_hit), shrink(label_hit));
        chip(cv, prev, BTN, BTN_BORDER);
        chip(cv, next, BTN, BTN_BORDER);
        chip(cv, label, BAR, AMBER);
        let (cx, cy) = (prev.x + prev.w / 2, prev.y + prev.h / 2);
        cv.triangle([(cx + 4, cy - 6), (cx + 4, cy + 6), (cx - 4, cy)], TEXT);
        let (cx, cy) = (next.x + next.w / 2, next.y + next.h / 2);
        cv.triangle([(cx - 4, cy - 6), (cx - 4, cy + 6), (cx + 4, cy)], TEXT);
        cv.text_in(&f.sb, label, 1, &text, AMBER);
    }

    /// A tap in the badge row on the search chip. True when it was taken.
    pub(super) fn studio_search_chip_tap(&mut self, x: i32, y: i32) -> bool {
        let Some((prev, label, next, _)) = self.studio_search_chip() else { return false };
        if prev.contains(x, y) { self.studio_search_go(-1); }
        else if next.contains(x, y) { self.studio_search_go(1); }
        else if label.contains(x, y) { self.toggle_menu(MenuId::Search); }
        else { return false; }
        true
    }

    /// The width the search chip takes at the left of the badge row.
    pub(super) fn studio_search_w(&self) -> i32 {
        self.studio_search_chip().map_or(0, |(p, _, n, _)| n.right() - p.x + 6)
    }

    // ------------------------------------------------------------------------------------------------ items

    pub(super) fn addrail_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::CurType | Item::CurSrc | Item::CurSrcB | Item::CurReadout | Item::CurLink | Item::SrchSrc | Item::SrchSlope => Kind::Choice,
            Item::SrchLevel | Item::SrchEvent => Kind::Value,
            Item::CurRemove | Item::Srch50 | Item::SrchFromTrig | Item::SrchToTrig | Item::SrchRemove => Kind::Action,
            _ => return None,
        })
    }

    pub(super) fn addrail_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::CurType => "Type",
            Item::CurSrc => "Source",
            Item::CurSrcB => "Second source",
            Item::CurReadout => "Readout",
            Item::CurLink => "Move",
            Item::CurRemove => "Remove cursors",
            Item::SrchSrc => "Source",
            Item::SrchSlope => "Slope",
            Item::SrchLevel => "Level",
            Item::Srch50 => "Set to 50 %",
            Item::SrchFromTrig => "Copy from trigger",
            Item::SrchToTrig => "Copy to trigger",
            Item::SrchEvent => "Event",
            Item::SrchRemove => "Remove search",
            _ => return None,
        })
    }

    pub(super) fn addrail_value(&self, it: Item) -> Option<String> {
        Some(match it {
            Item::CurType => match self.m.cursor {
                CursorMode::Off => "Off".into(),
                m => Self::cursor_type_names()[Self::CURSOR_MODES.iter().position(|&x| x == m).unwrap_or(0)].into(),
            },
            Item::CurSrc => self.wave_name(self.cursor_source()),
            Item::CurSrcB => self.m.cur_b_source.map_or("Same".into(), |s| self.wave_name(s)),
            Item::CurReadout => if self.studio.cur_badge { "Badge" } else { "Graticule" }.into(),
            Item::CurLink => if self.studio.cur_linked { "Linked" } else { "Independent" }.into(),
            Item::SrchSrc => self.studio.search.map_or(String::new(), |s| self.wave_name(s.src)),
            Item::SrchSlope => self.studio.search.map_or(String::new(), |s| SLOPES[s.slope.min(2) as usize].into()),
            Item::SrchLevel => self.studio.search.map_or(String::new(), |s| si_short(s.level as f64, match s.src { WaveId::Ch(c) if c < n_ch() => self.m.unit(c), _ => "V" })),
            Item::SrchEvent => {
                let n = self.studio_search_events().len();
                if n == 0 { "0 / 0".into() } else { format!("{} / {}", self.studio.search_at.min(n - 1) + 1, n) }
            }
            _ => return None,
        })
    }

    pub(super) fn addrail_options(&self, it: Item) -> Option<Vec<String>> {
        let names = || self.sheet_shown_sources().into_iter().map(|s| self.wave_name(s)).collect::<Vec<_>>();
        Some(match it {
            Item::CurType => Self::cursor_type_names().iter().map(|s| s.to_string()).collect(),
            Item::CurSrc | Item::SrchSrc => names(),
            Item::CurSrcB => std::iter::once("Same".to_string()).chain(names()).collect(),
            Item::CurReadout => vec!["Graticule".into(), "Badge".into()],
            Item::CurLink => vec!["Independent".into(), "Linked".into()],
            Item::SrchSlope => SLOPES.iter().map(|s| s.to_string()).collect(),
            Item::SrchLevel | Item::SrchEvent => Vec::new(),
            _ => return None,
        })
    }

    pub(super) fn addrail_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::CurType => {
                let all = Self::CURSOR_MODES;
                let now = all.iter().position(|&m| m == self.m.cursor);
                let next = match now { Some(i) => (i as i32 + d).rem_euclid(all.len() as i32) as usize, None => if d < 0 { all.len() - 1 } else { 0 } };
                self.set_cursor_type(all[next]);
            }
            Item::CurSrc | Item::SrchSrc => {
                let shown = self.sheet_shown_sources();
                if shown.is_empty() { return true; }
                let cur = if it == Item::CurSrc { self.cursor_source() } else { self.studio.search.map_or(shown[0], |s| s.src) };
                let i = shown.iter().position(|&s| s == cur).unwrap_or(0) as i32;
                let to = shown[(i + d).rem_euclid(shown.len() as i32) as usize];
                if it == Item::CurSrc {
                    self.set_cursor_source(to);
                } else if let Some(sp) = self.studio.search {
                    self.studio.search = Some(SearchSpec { src: to, ..sp });
                    self.studio_search_set_50();
                }
            }
            Item::CurSrcB => {
                let shown = self.sheet_shown_sources();
                let mut opts: Vec<Option<WaveId>> = vec![None];
                opts.extend(shown.into_iter().map(Some));
                let i = opts.iter().position(|&s| s == self.m.cur_b_source).unwrap_or(0) as i32;
                self.m.cur_b_source = opts[(i + d).rem_euclid(opts.len() as i32) as usize];
            }
            Item::CurReadout => if d.rem_euclid(2) != 0 { self.studio.cur_badge = !self.studio.cur_badge; },
            Item::CurLink => if d.rem_euclid(2) != 0 { self.studio.cur_linked = !self.studio.cur_linked; },
            Item::SrchSlope => if let Some(sp) = self.studio.search { self.studio.search = Some(SearchSpec { slope: (sp.slope as i32 + d).rem_euclid(3) as u8, ..sp }); },
            Item::SrchLevel => if let Some(sp) = self.studio.search {
                let step = 0.1 * self.studio_search_vpd(sp.src);
                self.studio.search = Some(SearchSpec { level: sp.level + d as f32 * step, ..sp });
            },
            Item::SrchEvent => self.studio_search_go(d.signum()),
            _ => return false,
        }
        true
    }

    pub(super) fn addrail_activate(&mut self, it: Item) -> bool {
        match it {
            Item::CurRemove => self.studio_cursors_remove(),
            Item::Srch50 => self.studio_search_set_50(),
            Item::SrchFromTrig => self.studio_search_copy_from_trigger(),
            Item::SrchToTrig => self.studio_search_copy_to_trigger(),
            Item::SrchRemove => self.studio_search_remove(),
            _ => return false,
        }
        true
    }
}

/// The two rows of the badge area are 44 px; keep the constant referenced so the sheet geometry stays in one place.
#[allow(dead_code)]
const _ROW: i32 = SHEET_ROW;
