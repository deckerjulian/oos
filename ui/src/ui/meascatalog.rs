// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The measurement page: categories, groups of which one is open, a one-line description and a drawn mark. It holds
//! every measurement OOS has: the 60 standard ones, the 17 pair variants and the analysis rows. A tap on a tile adds
//! the measurement on the current sources, a second tap removes it. A button with no function is not listed.

use super::*;
#[path = "meascatalog_backend.rs"]
pub(super) mod backend;

use crate::measures::{self, Def, Kind, Mark, CATEGORIES};
pub(super) use crate::measures::{meas_name as meas_long_name, pair_name as pair_long_name};

/// The state of the page (ADR 0062): the category shown, the tile picked, which groups are open, how far it is
/// scrolled, and the extra sources C and D of a measurement with four.
pub(super) struct Catalog {
    pub tab: u8,
    pub sel: u16,
    /// one bit per group of each category
    pub open: [u16; CATEGORIES.len()],
    /// pixels
    pub scroll: i32,
    pub c: WaveId,
    pub d: WaveId,
}

impl Default for Catalog {
    fn default() -> Self {
        Catalog { tab: 0, sel: 1, open: [1; CATEGORIES.len()], scroll: 0, c: WaveId::Ch(2), d: WaveId::Ch(3) }
    }
}

/// A tile of the page: one measurement of a module (`crate::measures`).
type Cat = &'static Def;

pub(super) fn cat_from(n: u16) -> Option<Cat> {
    measures::def(n)
}

/// Every measurement, in the order of the modules.
fn listed() -> Vec<Cat> {
    measures::all().map(|x| x.1).collect()
}

/// The groups of category `tab` with their tiles.
fn tab_groups(tab: u8) -> &'static [(&'static str, Vec<Cat>)] {
    measures::groups(tab)
}

/// The tile `id` (tests).
#[cfg(test)]
pub(super) fn tile(id: u16) -> Cat {
    cat_from(id).unwrap_or_else(|| panic!("no tile {id}"))
}

/// The tile shown when nothing is picked yet.
fn first_tile() -> Cat {
    cat_from(measures::std_id(Meas::Vamp)).expect("Amplitude tile")
}

/// Source slots used by this result. Unused picker slots must not block analysis.
pub(super) fn catalog_roles(c: Cat) -> &'static [usize] {
    c.roles
}

pub(super) fn primary_source(row: (u16, WaveId, WaveId, WaveId, WaveId)) -> WaveId {
    let sources = [row.1, row.2, row.3, row.4];
    sources[cat_from(row.0).map_or(0, |c| catalog_roles(c)[0])]
}

struct Hit {
    n: u16,
    r: Rect,
}

struct Lay {
    r: Rect,
    close: Rect,
    back: Option<Rect>,
    settings: Rect,
    selected: Rect,
    gate: Rect,
    tabs: Vec<Rect>,
    preview: Rect,
    mark: Rect,
    sources: Vec<Hit>,
    view: Rect,
    groups: Vec<Hit>,
    items: Vec<Hit>,
    /// the scroll bar's track beside the list
    bar: Rect,
    content: i32,
}

impl Ui {
    pub(super) fn catalog_source_label(&self, id: u16, sources: [WaveId; 4]) -> String {
        let Some(c) = cat_from(id) else { return String::new() };
        let roles = catalog_roles(c).iter().map(|&slot| self.wave_name(sources[slot])).collect::<Vec<_>>().join("-");
        format!("{roles} {}", c.name)
    }
    fn cat_sel(&self) -> Cat {
        cat_from(self.cat.sel).unwrap_or_else(first_tile)
    }

    fn src_label(&self, id: WaveId) -> String {
        match id {
            WaveId::Ch(c) => format!("Ch {}", c + 1),
            WaveId::Ref(i) => format!("Ref {}", i + 1),
        }
    }

    /// The source rows of the page: as many as the category always shows, more when the measurement picked reads
    /// more; labelled as the category names them.
    fn source_slots(&self) -> Vec<(u8, &'static str, WaveId)> {
        let cat = &CATEGORIES[(self.cat.tab as usize).min(CATEGORIES.len() - 1)];
        let need = self.cat_sel().roles.iter().max().map_or(1, |r| r + 1);
        let n = cat.min.max(need).min(4);
        let ids = [self.meas_src, self.meas_src_b, self.cat.c, self.cat.d];
        (0..n).map(|k| (k as u8, if n == 1 { cat.one } else { cat.slots[k] }, ids[k])).collect()
    }

    /// The measurement page. Header: the title, then what applies to every measurement (gate, settings). Left: the
    /// categories, and under them the measurements that are on (Selected). Right of them: the sources, the groups of the
    /// category (one open at a time), and at the bottom the one last tapped with its description.
    fn catalog_lay(&self) -> Lay {
        let r = Rect::new(8, TOP.h + 4, W - 16, self.frame().bottom.y - TOP.h - 8);
        let close = Rect::new(r.x + r.w - 56, r.y + 6, 44, DLG_CTRL);
        let back = self.parent.map(|_| Rect::new(close.x - 86, close.y, 78, DLG_CTRL));
        let settings = Rect::new(back.unwrap_or(close).x - 108, r.y + 6, 100, 40);
        let gate = Rect::new(settings.x - 16 - 220, r.y + 6, 220, 40);
        let side_w = 168;
        let top = r.y + 54;
        let mut tabs = Vec::new();
        for i in 0..CATEGORIES.len() {
            tabs.push(Rect::new(r.x + 12, top + i as i32 * 44, side_w, 40));
        }
        let selected = Rect::new(r.x + 12, r.y + r.h - 48, side_w, 40);
        let cx = r.x + 12 + side_w + 12;
        let cw = r.x + r.w - 12 - cx;
        let preview = Rect::new(cx, r.y + r.h - 72, cw, 64);
        let mark = Rect::new(preview.x + 8, preview.y + 10, 72, preview.h - 20);
        let slots = self.source_slots();
        let rows = if slots.len() > 2 { 2 } else { 1 };
        let src_h = 40;
        let per = cw / 2;
        let mut sources = Vec::new();
        for (i, (id, _, _)) in slots.iter().enumerate() {
            let col = (i % 2) as i32;
            let row = (i / 2) as i32;
            let x = cx + col * per;
            let y = top + row * (src_h + 6);
            sources.push(Hit { n: *id as u16, r: Rect::new(x + per / 3, y, per - per / 3 - 8, src_h) });
        }
        let view_y = top + rows * (src_h + 6) + 2;
        let view = Rect::new(cx, view_y, cw - 46, preview.y - 8 - view_y);
        let mut y = 0;
        let mut groups = Vec::new();
        let mut items = Vec::new();
        let open = self.cat.open[self.cat.tab as usize];
        let cols = 3;
        let gap = 6;
        let bw = (view.w - 8 - (cols - 1) * gap) / cols;
        let bh = 42;
        for (g, (_, list)) in tab_groups(self.cat.tab).iter().enumerate() {
            groups.push(Hit { n: g as u16, r: Rect::new(0, y, view.w - 4, 40) });
            y += 44;
            if open & (1 << g) == 0 {
                continue;
            }
            for (i, &c) in list.iter().enumerate() {
                let col = (i % cols as usize) as i32;
                let row = (i / cols as usize) as i32;
                items.push(Hit { n: c.id, r: Rect::new(col * (bw + gap), y + row * (bh + gap), bw, bh) });
            }
            let nrows = list.len().div_ceil(cols as usize) as i32;
            y += nrows * (bh + gap) + 4;
        }
        let bar = Rect::new(view.x + view.w + 6, view.y, scroll::BAR_W, view.h);
        Lay {
            r,
            close,
            back,
            settings,
            selected,
            gate,
            tabs,
            preview,
            mark,
            sources,
            view,
            groups,
            items,
            bar,
            content: y,
        }
    }

    #[cfg(test)]
    pub(super) fn catalog_settings_rect(&self) -> Rect {
        self.catalog_lay().settings
    }

    /// Set control on an added standard measurement, in screen coordinates.
    #[cfg(test)]
    pub(super) fn catalog_set_rect(&self, k: Meas) -> Option<Rect> {
        let lay = self.catalog_lay();
        lay.items.iter().find_map(|it| {
            let c = cat_from(it.n)?;
            let Kind::Standard(mk) = c.kind else { return None };
            (mk == k).then(|| self.catalog_set_on(screen_rect(lay.view, it.r, self.cat_top()), c)).flatten()
        })
    }

    /// Header of one group on the current tab, in screen coordinates.
    pub(super) fn catalog_group_rect(&self, group: usize) -> Option<Rect> {
        let lay = self.catalog_lay();
        lay.groups.iter().find(|g| g.n as usize == group).map(|g| screen_rect(lay.view, g.r, self.cat_top()))
    }

    pub(super) fn catalog_view(&self) -> Rect {
        self.catalog_lay().view
    }

    /// The tile `id` on the screen (tests).
    #[cfg(test)]
    pub(super) fn catalog_tile_rect(&self, id: u16) -> Option<Rect> {
        let lay = self.catalog_lay();
        lay.items.iter().find(|it| it.n == id).map(|it| screen_rect(lay.view, it.r, self.cat_top()))
    }

    /// The scroll bar of the page and its list, while the page is open.
    pub(super) fn catalog_bar(&self) -> Option<(scroll::Bar, Rect)> {
        if !(self.menu_open && self.menu == MenuId::MeasAdd) {
            return None;
        }
        let lay = self.catalog_lay();
        Some((scroll::Bar { track: lay.bar, total: lay.content, shown: lay.view.h, pos: self.cat.scroll }, lay.view))
    }

    /// Where the list stands, kept in range (a folded group may have made it shorter).
    pub(super) fn cat_top(&self) -> i32 {
        let lay = self.catalog_lay();
        self.cat.scroll.clamp(0, (lay.content - lay.view.h).max(0))
    }

    fn catalog_set_on(&self, rect: Rect, c: Cat) -> Option<Rect> {
        let Kind::Standard(_) = c.kind else { return None };
        self.cat_added(c).then(|| Rect::new(rect.x + rect.w - 52, rect.y + (rect.h - 40) / 2, 48, 40))
    }

    pub(super) fn catalog_dialog(&self) -> Dlg {
        let lay = self.catalog_lay();
        let mut rows = lay
            .sources
            .iter()
            .map(|hit| Row {
                it: match hit.n {
                    0 => Item::MeasSrc,
                    1 => Item::MeasSrcB,
                    2 => Item::CatSourceC,
                    _ => Item::CatSourceD,
                },
                label: Rect::new(0, 0, 0, 0),
                ctrl: hit.r,
                ctl: Ctl::Pick,
            })
            .collect::<Vec<_>>();
        rows.push(Row {
            it: Item::MeasGate,
            label: Rect::new(0, 0, 0, 0),
            ctrl: lay.gate,
            ctl: Ctl::Seg(vec!["Screen".into(), "Full record".into()], Some(usize::from(self.m.meas_full_record))),
        });
        Dlg { r: lay.r, close: lay.close, back: lay.back, rows, heads: Vec::new(), bar: None }
    }

    fn content_h(&self) -> i32 {
        self.catalog_lay().content
    }

    fn clamp_scroll(&mut self) {
        let h = self.content_h();
        let view = self.catalog_lay().view.h;
        let max = (h - view).max(0);
        self.cat.scroll = self.cat.scroll.clamp(0, max);
    }

    pub(super) fn draw_catalog(&self, cv: &mut Canvas) {
        let lay = self.catalog_lay();
        let f = &self.fonts();
        cv.framed(lay.r, 8, 2, ACCENT, 7, DLG_BG);
        cv.text(&f.mb, lay.r.x + 16, lay.r.y + 31, "Measure", TEXT);
        btn(cv, lay.close, BTN, BTN_BORDER);
        cv.text_in(&f.mb, lay.close, 1, "\u{d7}", TEXT);
        if let Some(back) = lay.back {
            btn(cv, back, BTN, BTN_BORDER);
            cv.text_in(&f.sb, back, 1, "Back", TEXT);
        }
        btn(cv, lay.settings, BTN, BTN_BORDER);
        cv.text_in(&f.s, lay.settings, 1, "Settings", TEXT);
        cv.text_in(&f.s, Rect::new(lay.gate.x - 44, lay.gate.y, 38, lay.gate.h), 2, "Gate", DIM);
        for (index, label) in ["Screen", "Full record"].into_iter().enumerate() {
            let half = (lay.gate.w - 4) / 2;
            let rect = Rect::new(lay.gate.x + index as i32 * (half + 4), lay.gate.y, half, lay.gate.h);
            btn(cv, rect, if self.m.meas_full_record == (index == 1) { ACCENT } else { BTN }, BTN_BORDER);
            cv.text_in(&f.s, rect, 1, label, if self.m.meas_full_record == (index == 1) { DARK_TEXT } else { TEXT });
        }
        // the categories; the one shown is filled
        for (i, rect) in lay.tabs.iter().enumerate() {
            let on = i as u8 == self.cat.tab;
            btn(cv, *rect, if on { ACCENT } else { BTN }, BTN_BORDER);
            let name = CATEGORIES[i].name;
            let (font, tr, _) = self.btn_text(*rect, false, name);
            cv.text_in(font, tr, 1, name, if on { DARK_TEXT } else { TEXT });
        }
        // under them: the measurements that are on, to look through and remove
        btn(cv, lay.selected, BTN, BTN_BORDER);
        let sel_r = Rect::new(lay.selected.x, lay.selected.y, lay.selected.w - 18, lay.selected.h);
        cv.text_in(&f.s, sel_r, 1, &format!("Selected ({})", self.measurement_count()), TEXT);
        chevron(cv, lay.selected.x + lay.selected.w - 14, lay.selected.y + lay.selected.h / 2, 1, DIM);
        // the measurement that is picked: sign, name, what it is, and Add / Remove
        cv.round(lay.preview, 6, PREVIEW_BG, 255);
        let sel = self.cat_sel();
        let info = sel;
        self.draw_mark(cv, lay.mark, info.mark, AMBER);
        let tx = lay.mark.x + lay.mark.w + 14;
        let hint = if self.cat_added(sel) { "On \u{b7} tap again to remove" } else { "Tap to add" };
        let hw = f.s.width(hint);
        cv.text_in(&f.s, Rect::new(lay.preview.x + lay.preview.w - hw - 14, lay.preview.y + 6, hw, 24), 2, hint, DIM);
        let tw = lay.preview.x + lay.preview.w - tx - 14 - hw - 12;
        cv.set_clip(Rect::new(tx, lay.preview.y, tw, lay.preview.h));
        cv.text(&f.mb, tx, lay.preview.y + 24, info.name, TEXT);
        if info.kind == Kind::Module && self.cat_added(sel) {
            let reading = self.catalog_reading(sel, [self.meas_src, self.meas_src_b, self.cat.c, self.cat.d]);
            let status = if reading.computing {
                "Computing".into()
            } else if let Some(reason) = reading.reason {
                format!("Unavailable: {reason}")
            } else if reading.value.is_finite() {
                format!("{} · {}", si(reading.value, info.unit), reading.quality)
            } else {
                format!("---- · {}", reading.quality)
            };
            cv.text(&f.s, tx + f.mb.width(info.name) + 14, lay.preview.y + 24, &status, DIM);
        }
        let mut y = lay.preview.y + 43;
        for line in wrap_lines(&f.s, info.blurb, tw) {
            if y > lay.preview.y + lay.preview.h - 4 {
                break;
            }
            cv.text(&f.s, tx, y, &line, LABEL);
            y += f.s.size + 2;
        }
        cv.no_clip();
        let slots = self.source_slots();
        for (hit, (_, label, id)) in lay.sources.iter().zip(&slots) {
            let lab = Rect::new(hit.r.x - hit.r.w / 2, hit.r.y, hit.r.w / 2, hit.r.h);
            cv.text_in(&f.s, lab, 0, label, DIM);
            btn(cv, hit.r, BTN, BTN_BORDER);
            cv.text_in(&f.s, Rect::new(hit.r.x + 8, hit.r.y, hit.r.w - 22, hit.r.h), 0, &self.src_label(*id), TEXT);
            chevron_down(cv, hit.r.x + hit.r.w - 16, hit.r.y + hit.r.h / 2 - 3, DIM);
        }
        cv.set_clip(lay.view);
        let scroll = self.cat_top();
        let open = self.cat.open[self.cat.tab as usize];
        for g in &lay.groups {
            let rect = screen_rect(lay.view, g.r, scroll);
            cv.rect(rect, GROUP_HEAD);
            let name = tab_groups(self.cat.tab).get(g.n as usize).map(|g| g.0).unwrap_or("");
            cv.text_in(&f.s, Rect::new(rect.x + 10, rect.y, rect.w - 36, rect.h), 0, name, LABEL);
            if open & (1 << g.n) != 0 {
                chevron_down(cv, rect.x + rect.w - 18, rect.y + rect.h / 2 - 3, DIM);
            } else {
                chevron(cv, rect.x + rect.w - 16, rect.y + rect.h / 2, 1, DIM);
            }
        }
        for it in &lay.items {
            let Some(c) = cat_from(it.n) else { continue };
            let rect = screen_rect(lay.view, it.r, scroll);
            let on = c.id == sel.id;
            let added = self.cat_added(c);
            let set = self.catalog_set_on(rect, c);
            btn(cv, rect, if added { ACCENT } else { BTN }, if on { ACCENT } else { BTN_BORDER });
            if on {
                // the picked one: a frame, inside the fill where it is added
                cv.round_outline(if added { rect.inset(3) } else { rect }, 5, if added { DARK_TEXT } else { ACCENT });
            }
            let ink = if added { DARK_TEXT } else { TEXT };
            let icon = Rect::new(rect.x + 4, rect.y + 6, 32, rect.h - 12);
            self.draw_mark(cv, icon, c.mark, if added { DARK_TEXT } else { BLUE });
            let (a, b) = split_name(c.name);
            let text_w = rect.w - 46 - if set.is_some() { 52 } else { 0 };
            let text = Rect::new(rect.x + 40, rect.y, text_w, rect.h);
            if let Some(set) = set {
                btn(cv, set, BTN, ACCENT);
                cv.text_in(&f.s, set, 1, "Set", TEXT);
            }
            if let Some(b) = b {
                cv.text_in(&f.s, Rect::new(text.x, text.y, text.w, text.h / 2), 0, a, ink);
                cv.text_in(&f.s, Rect::new(text.x, text.y + text.h / 2, text.w, text.h / 2), 0, b, ink);
            } else {
                cv.text_in(&f.s, text, 0, a, ink);
            }
        }
        cv.no_clip();
        scroll::Bar { track: lay.bar, total: lay.content, shown: lay.view.h, pos: self.cat.scroll }.draw(cv);
    }

    fn draw_mark(&self, cv: &mut Canvas, r: Rect, mark: Mark, col: Rgb) {
        let p = |x: f32, y: f32| (r.x + (x * r.w as f32) as i32, r.y + ((1.0 - y) * r.h as f32) as i32);
        let stroke = |cv: &mut Canvas, pts: &[(f32, f32)]| {
            for w in pts.windows(2) {
                let (a, b) = (p(w[0].0, w[0].1), p(w[1].0, w[1].1));
                cv.line(a.0, a.1, b.0, b.1, col);
            }
        };
        let base = [(0.08, 0.22), (0.92, 0.22)];
        match mark {
            Mark::Pulse => stroke(
                cv,
                &[
                    (0.08, 0.35),
                    (0.22, 0.35),
                    (0.28, 0.78),
                    (0.42, 0.78),
                    (0.48, 0.48),
                    (0.62, 0.48),
                    (0.68, 0.78),
                    (0.82, 0.78),
                    (0.88, 0.35),
                    (0.94, 0.35),
                ],
            ),
            Mark::Square => stroke(
                cv,
                &[
                    (0.08, 0.3),
                    (0.22, 0.3),
                    (0.22, 0.75),
                    (0.48, 0.75),
                    (0.48, 0.3),
                    (0.74, 0.3),
                    (0.74, 0.75),
                    (0.92, 0.75),
                ],
            ),
            Mark::Edge => {
                stroke(cv, &[(0.1, 0.28), (0.38, 0.28), (0.62, 0.74), (0.9, 0.74)]);
                stroke(cv, &[(0.18, 0.55), (0.82, 0.55)]);
            }
            Mark::Eye => {
                stroke(cv, &[(0.12, 0.25), (0.5, 0.72), (0.88, 0.25)]);
                stroke(cv, &[(0.12, 0.75), (0.5, 0.28), (0.88, 0.75)]);
            }
            Mark::Bars => {
                for (i, h) in [0.45, 0.78, 0.55].into_iter().enumerate() {
                    let x = 0.22 + i as f32 * 0.24;
                    stroke(cv, &[(x, 0.22), (x, h)]);
                }
            }
            Mark::Loop => {
                let pts: Vec<(f32, f32)> = (0..=16)
                    .map(|k| {
                        let a = k as f32 / 16.0 * std::f32::consts::TAU;
                        (0.5 + 0.32 * a.cos(), 0.5 + 0.28 * a.sin())
                    })
                    .collect();
                stroke(cv, &pts);
            }
            Mark::Slope => stroke(cv, &[(0.12, 0.28), (0.42, 0.28), (0.78, 0.78), (0.9, 0.78)]),
        }
        if !matches!(mark, Mark::Eye | Mark::Bars | Mark::Loop) {
            stroke(cv, &base);
        }
    }

    pub(super) fn catalog_tap(&mut self, x: i32, y: i32) -> bool {
        let lay = self.catalog_lay();
        if !lay.r.contains(x, y) {
            return false;
        }
        if lay.close.contains(x, y) {
            self.close_menu();
            return true;
        }
        if lay.back.is_some_and(|b| b.contains(x, y)) {
            self.menu_back();
            return true;
        }
        if lay.settings.contains(x, y) {
            self.open_submenu(MenuId::Measure);
            return true;
        }
        if lay.selected.contains(x, y) {
            self.meas_pages.selected = 0;
            self.open_submenu(MenuId::MeasSelected);
            return true;
        }
        if lay.gate.contains(x, y) {
            let full = x >= lay.gate.x + lay.gate.w / 2;
            if self.m.meas_full_record != full {
                self.set_measurement_full_record(full);
            }
            return true;
        }
        for (i, rect) in lay.tabs.iter().enumerate() {
            if rect.contains(x, y) {
                self.cat.tab = i as u8;
                self.cat.scroll = 0;
                let first =
                    tab_groups(self.cat.tab).first().and_then(|g| g.1.first()).copied().unwrap_or_else(first_tile);
                if tab_groups(self.cat.tab).iter().all(|g| !g.1.iter().any(|d| d.id == self.cat.sel)) {
                    self.cat.sel = first.id;
                }
                return true;
            }
        }
        for hit in &lay.sources {
            if hit.r.contains(x, y) {
                match hit.n {
                    0 => self.open_picker(Item::MeasSrc),
                    1 => self.open_picker(Item::MeasSrcB),
                    2 => self.open_picker(Item::CatSourceC),
                    _ => self.open_picker(Item::CatSourceD),
                }
                return true;
            }
        }
        if lay.bar.contains(x, y) {
            return true;
        }
        if lay.view.contains(x, y) {
            let top = self.cat_top();
            let cy = y - lay.view.y + top;
            let cx = x - lay.view.x;
            for g in &lay.groups {
                if g.r.contains(cx, cy) {
                    // one group open at a time: a tap opens it and closes the others, a tap on the open one closes it
                    let bit = 1u16 << g.n;
                    let open = &mut self.cat.open[self.cat.tab as usize];
                    *open = if *open & bit != 0 { 0 } else { bit };
                    // an opened group comes to the top of the list; a folded one leaves the list where it can stand
                    if self.cat.open[self.cat.tab as usize] & bit != 0 {
                        self.cat.scroll = g.r.y;
                    }
                    self.clamp_scroll();
                    return true;
                }
            }
            for it in &lay.items {
                if it.r.contains(cx, cy) {
                    let rect = screen_rect(lay.view, it.r, top);
                    if let Some(c) = cat_from(it.n) {
                        if let Some(set) = self.catalog_set_on(rect, c) {
                            if set.contains(x, y) {
                                if let Kind::Standard(k) = c.kind {
                                    if let Some(i) =
                                        self.m.meas.iter().position(|&(mk, id)| mk == k && id == self.meas_src)
                                    {
                                        self.open_meas_cfg(i);
                                    }
                                }
                                return true;
                            }
                        }
                    }
                    // one tap: picked (the description shows) and added on the current sources, or removed
                    self.cat.sel = it.n;
                    if let Some(c) = cat_from(it.n) {
                        self.catalog_add(c);
                    }
                    return true;
                }
            }
        }
        true
    }

    /// The tile named `name` (case does not matter): its category and group shown, then a tap on it.
    pub(super) fn catalog_tap_name(&mut self, name: &str) -> Result<(), String> {
        let c = listed()
            .into_iter()
            .find(|&c| c.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| format!("no measurement {:?}", name.trim()))?;
        let (tab, g) = (0..CATEGORIES.len() as u8)
            .find_map(|t| tab_groups(t).iter().position(|g| g.1.iter().any(|d| d.id == c.id)).map(|g| (t, g)))
            .ok_or_else(|| format!("{:?} is in no group", name.trim()))?;
        self.cat.tab = tab;
        self.cat.open[tab as usize] = 1 << g;
        self.cat.scroll = self.catalog_group_rect(g).map_or(0, |r| r.y - self.catalog_view().y + self.cat_top());
        self.clamp_scroll();
        self.cat.sel = c.id;
        self.catalog_add(c);
        Ok(())
    }

    fn cat_added(&self, c: Cat) -> bool {
        match c.kind {
            Kind::Standard(k) => self.m.meas.iter().any(|e| e.0 == k && e.1 == self.meas_src),
            Kind::Pair(k) => self.m.meas2.iter().any(|e| e.0 == k && e.1 == self.meas_src && e.2 == self.meas_src_b),
            Kind::Module => self.m.cat.iter().any(|e| {
                e.0 == c.id && e.1 == self.meas_src && e.2 == self.meas_src_b && e.3 == self.cat.c && e.4 == self.cat.d
            }),
        }
    }

    pub(super) fn catalog_add(&mut self, c: Cat) {
        match c.kind {
            Kind::Standard(k) => self.activate(Item::MeasAdd(k)),
            Kind::Pair(k) => self.meas_pair_toggle(k),
            Kind::Module => {
                let row = (c.id, self.meas_src, self.meas_src_b, self.cat.c, self.cat.d);
                if let Some(i) = self.m.cat.iter().position(|e| *e == row) {
                    self.m.cat.remove(i);
                    self.toast(&format!("{} removed", c.name));
                } else if self.meas_make_room() {
                    self.m.cat.push(row);
                    self.toast(&format!("{} added", c.name));
                }
                self.clear_measurement_statistics();
            }
        }
    }

    /// Copies every measurement that reads `from` (single, pair and catalog rows) to `to`; rows already present are
    /// skipped. Returns how many were added.
    pub(super) fn copy_measurements(&mut self, from: WaveId, to: WaveId) -> usize {
        if from == to {
            return 0;
        }
        let swap = |w: WaveId| if w == from { to } else { w };
        let mut added = 0;
        let room = |m: &crate::model::Model| m.meas.len() + m.meas2.len() + m.cat.len() < MEAS_SELECTION_MAX;
        for (k, w) in self.m.meas.clone() {
            if w == from && !self.m.meas.contains(&(k, to)) && room(&self.m) {
                self.m.meas.push((k, to));
                added += 1;
            }
        }
        for (k, a, b) in self.m.meas2.clone() {
            let row = (k, swap(a), swap(b));
            if (a == from || b == from) && !self.m.meas2.contains(&row) && room(&self.m) {
                self.m.meas2.push(row);
                added += 1;
            }
        }
        for (id, a, b, c, d) in self.m.cat.clone() {
            let row = (id, swap(a), swap(b), swap(c), swap(d));
            if [a, b, c, d].contains(&from) && !self.m.cat.contains(&row) && room(&self.m) {
                self.m.cat.push(row);
                added += 1;
            }
        }
        if added > 0 {
            self.clear_measurement_statistics();
            self.settings_changed();
        }
        added
    }

    pub(super) fn cat_row(&self, id: u16, a: WaveId, b: WaveId, c: WaveId, d: WaveId) -> Option<tools::MeasRow> {
        let cat = cat_from(id)?;
        let info = cat;
        let reading = self.catalog_reading(cat, [a, b, c, d]);
        Some(tools::MeasRow {
            name: info.name.into(),
            src: [a, b, c, d][catalog_roles(cat)[0]],
            v: reading.value,
            unit: info.unit,
            computing: reading.computing,
        })
    }
}

fn screen_rect(view: Rect, r: Rect, scroll: i32) -> Rect {
    Rect::new(view.x + r.x, view.y + r.y - scroll, r.w, r.h)
}

fn split_name(name: &str) -> (&str, Option<&str>) {
    let Some(mid) = name.find(' ') else { return (name, None) };
    let i = name
        .match_indices(' ')
        .min_by_key(|(i, _)| (*i as i32 - name.len() as i32 / 2).abs())
        .map(|(i, _)| i)
        .unwrap_or(mid);
    (&name[..i], Some(&name[i + 1..]))
}

fn wrap_lines(f: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if cur.is_empty() || f.width(&trial) <= width {
            cur = trial;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::collections::HashSet;

    fn tap(ui: &mut Ui, r: Rect) {
        ui.tap(r.x + r.w / 2, r.y + r.h / 2);
    }

    #[test]
    fn the_page_holds_all_sixty_measurements_all_pairs_and_one_tap_adds() {
        // every standard measurement and every pair variant has exactly one tile, with a name of its own
        for &k in ALL_MEAS.iter() {
            let n = listed().into_iter().filter(|&c| matches!(c.kind, Kind::Standard(x) if x == k)).count();
            assert_eq!(n, 1, "{k:?}");
            assert_ne!(meas_long_name(k), k.name().to_string() + "?");
        }
        for p in 0..tools::PAIRS.len() {
            let n = listed().into_iter().filter(|&c| matches!(c.kind, Kind::Pair(x) if x == p)).count();
            assert_eq!(n, 1, "pair {p}");
        }
        let names: HashSet<_> = listed().into_iter().map(|c| c.name).collect();
        assert_eq!(meas_long_name(Meas::Vpp), "Peak-to-Peak");
        assert!(names.len() >= listed().len() - 3, "names tell the tiles apart");
        // one tap on a tile adds it on the source, a second tap removes it
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.meas.clear();
        ui.key(key::MEASURE);
        let rect_of = |ui: &Ui, c: Cat| {
            let lay = ui.catalog_lay();
            lay.items.iter().find(|it| it.n == c.id).map(|it| screen_rect(lay.view, it.r, ui.cat_top())).unwrap()
        };
        let r = rect_of(&ui, tile(measures::std_id(Meas::Vpp)));
        tap(&mut ui, Rect::new(r.x, r.y, r.w / 2, r.h));
        assert_eq!(ui.m.meas, vec![(Meas::Vpp, WaveId::Ch(0))]);
        assert_eq!(ui.cat.sel, measures::std_id(Meas::Vpp), "the description follows the tap");
        let r = rect_of(&ui, tile(measures::std_id(Meas::Vpp)));
        tap(&mut ui, Rect::new(r.x, r.y, r.w / 2, r.h));
        assert!(ui.m.meas.is_empty());
        // a pair on the Two sources page, between source A and B
        let t = ui.catalog_lay().tabs[1];
        tap(&mut ui, t);
        assert_eq!(ui.catalog_lay().sources.len(), 2, "source A and B");
        let r = rect_of(&ui, tile(measures::pair_id(0)));
        tap(&mut ui, r);
        assert_eq!(ui.m.meas2, vec![(0, WaveId::Ch(0), WaveId::Ch(1))]);
        // one group open at a time
        let g = ui.catalog_group_rect(2).unwrap();
        tap(&mut ui, g);
        assert_eq!(ui.cat.open[1], 1 << 2);
        let g = ui.catalog_group_rect(2).unwrap();
        tap(&mut ui, g);
        assert_eq!(ui.cat.open[1], 0);
        // Selected opens the list of what is on, Back returns
        let sel = ui.catalog_lay().selected;
        tap(&mut ui, sel);
        assert_eq!((ui.menu, ui.parent), (MenuId::MeasSelected, Some(MenuId::MeasAdd)));
        assert!(ui.dlg_layout().unwrap().rows.iter().any(|r| r.it == Item::MeasRemove(0)));
        let back = ui.dlg_layout().unwrap().back.unwrap();
        tap(&mut ui, back);
        assert_eq!(ui.menu, MenuId::MeasAdd);
        // the 83 analysis measurements of the scope build (the test build adds the example module's)
        let ids: HashSet<_> = listed().into_iter().filter(|c| c.kind == Kind::Module).map(|c| c.id).collect();
        assert_eq!(ids.iter().filter(|&&id| id < 1000).count(), 83);
        for id in 0..=255 {
            assert_eq!(crate::measures::catalog_id_valid(id), ids.contains(&id), "persisted ID{id}");
        }
    }

    #[test]
    fn selected_pages_include_and_remove_all_row_types_and_clear_everything() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.meas = ALL_MEAS.iter().map(|&k| (k, WaveId::Ch(0))).collect();
        ui.m.meas2 = (0..13).map(|k| (k, WaveId::Ch(0), WaveId::Ch(1))).collect();
        ui.m.cat = vec![
            (90, WaveId::Ref(0), WaveId::Ref(1), WaveId::Ch(2), WaveId::Ch(3)),
            (91, WaveId::Ref(0), WaveId::Ref(1), WaveId::Ch(2), WaveId::Ch(3)),
            (113, WaveId::Ref(0), WaveId::Ref(1), WaveId::Ch(2), WaveId::Ch(3)),
        ];
        assert_eq!(ui.measurement_count(), 76);
        ui.key(key::MEASURE);
        let selected = ui.catalog_lay().selected;
        tap(&mut ui, selected);
        assert_eq!(ui.menu, MenuId::MeasSelected);
        let mut found = HashSet::new();
        for page in 0..76usize.div_ceil(16) {
            ui.meas_pages.selected = page;
            let dialog = ui.dlg_layout().unwrap();
            for row in dialog.rows {
                assert!(row.ctrl.x >= dialog.r.x && row.ctrl.x + row.ctrl.w <= dialog.r.x + dialog.r.w);
                assert!(row.ctrl.y >= dialog.r.y && row.ctrl.y + row.ctrl.h <= dialog.r.y + dialog.r.h);
                if let Item::MeasRemove(index) = row.it {
                    found.insert(index as usize);
                }
            }
        }
        assert_eq!(found, (0..76).collect());
        assert!(ui.selected_measurement_label(73).contains("R1-R2 Power Quality"));
        ui.meas_pages.selected = 73 / 16;
        let remove = ui.dlg_layout().unwrap().rows.into_iter().find(|row| row.it == Item::MeasRemove(73)).unwrap().ctrl;
        tap(&mut ui, remove);
        assert_eq!(ui.m.cat.len(), 2);
        assert_eq!(ui.m.meas.len(), 60);
        assert_eq!(ui.m.meas2.len(), 13);
        ui.activate(Item::MeasClear);
        assert_eq!(ui.measurement_count(), 0);
        assert!(ui.mstat.is_empty());
        assert_eq!((ui.m.run, ui.counts.wfm_total), (RunState::Stop, 0));
    }

    #[test]
    fn close_hides_the_catalog_and_source_c_d_are_real_pickers() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.key(key::MEASURE);
        let close = ui.catalog_lay().close;
        tap(&mut ui, close);
        assert!(!ui.menu_open);
        let mut canvas = Canvas::new(W, H);
        canvas.clear(BG);
        ui.draw_menu(&mut canvas);
        assert!(canvas.px.iter().all(|&p| p == BG), "a closed catalog must not cover the waveform");
        ui.key(key::MEASURE);
        let dpt = ui.catalog_lay().tabs[measures::DPT as usize];
        tap(&mut ui, dpt);
        let c = ui.catalog_lay().sources.into_iter().find(|hit| hit.n == 2).unwrap().r;
        tap(&mut ui, c);
        assert_eq!(ui.pick.unwrap().it, Item::CatSourceC);
        ui.close_picker();
        ui.cat.tab = measures::MOTOR;
        ui.cat.sel = measures::motor::TORQUE;
        let d = ui.catalog_lay().sources.into_iter().find(|hit| hit.n == 3).unwrap().r;
        tap(&mut ui, d);
        assert_eq!(ui.pick.unwrap().it, Item::CatSourceD);
        ui.close_picker();
        for tab in 0..CATEGORIES.len() {
            ui.cat.tab = tab as u8;
            let lay = ui.catalog_lay();
            assert!(lay.bar.w >= 40 && lay.bar.h >= 80, "a scroll bar that a finger can hold");
            assert!(!overlaps(lay.gate, lay.settings) && lay.tabs.iter().all(|t| !overlaps(*t, lay.selected)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(ui: &mut Ui, slot: usize, v: Vec<f32>, dt: f64) {
        ui.refs[slot] = Some(RefWave::test(v, dt, 0.0));
    }

    fn ready_rows(ui: &mut Ui) -> Vec<tools::MeasRow> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            ui.poll_catalog_measurements();
            let rows = ui.meas_rows();
            if rows.iter().all(|row| !row.computing) {
                return rows;
            }
            assert!(std::time::Instant::now() < end, "catalog rows did not complete");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn square(amp: f32, n: usize, half: usize) -> Vec<f32> {
        (0..n).map(|i| if (i / half).is_multiple_of(2) { amp } else { -amp }).collect()
    }

    #[test]
    fn every_button_has_a_name_and_the_unwired_ones_are_absent() {
        let names: Vec<&str> = listed().into_iter().map(|c| c.name).collect();
        for want in [
            "Amplitude",
            "Low Time",
            "Jitter Summary",
            "Eye Height@BER",
            "T/nT Ratio",
            "Power Quality",
            "Inductance",
            "DQ0",
            "Vpeak",
            "Qoss",
            "Td(on)",
            "RDS(on)",
        ] {
            assert!(names.contains(&want), "missing {want}");
        }
        for absent in ["Pattern Length", "SSC Slew Rate", "ISI", "RJ", "DJ", "HSS", "WaveformEngine"] {
            assert!(!names.iter().any(|n| n.eq_ignore_ascii_case(absent)), "{absent} shipped");
        }
        assert!(names.contains(&"RJ-dd") && names.contains(&"DJ-dd"), "dual-Dirac names");
        assert_eq!(CATEGORIES.len(), 8);
    }

    #[test]
    fn copy_to_repeats_source_a_measurements_on_the_target_once() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        put(&mut ui, 0, square(1.0, 2000, 40), 1e-6);
        put(&mut ui, 1, square(1.0, 2000, 40), 1e-6);
        let (r1, r2, ch1) = (WaveId::Ref(0), WaveId::Ref(1), WaveId::Ch(0));
        ui.m.meas = vec![(Meas::Vpp, r1), (Meas::Freq, r1), (Meas::Vpp, ch1), (Meas::Freq, r2)];
        ui.m.meas2 = vec![(0, r1, ch1)];
        ui.m.cat = vec![(measures::serial::DATA_RATE, r1, r1, r1, r1)];
        ui.meas_src = r1;
        assert!(ui.options(Item::MeasCopyTo).iter().all(|n| n != "R1"));
        ui.select_option(Item::MeasCopyTo, "R2");
        assert_eq!(
            ui.m.meas,
            vec![(Meas::Vpp, r1), (Meas::Freq, r1), (Meas::Vpp, ch1), (Meas::Freq, r2), (Meas::Vpp, r2)]
        );
        assert_eq!(ui.m.meas2, vec![(0, r1, ch1), (0, r2, ch1)]);
        assert_eq!(ui.m.cat.last().copied(), Some((measures::serial::DATA_RATE, r2, r2, r2, r2)));
        assert_eq!(ui.copy_measurements(r1, r2), 0, "a second copy adds nothing");
    }

    #[test]
    fn add_puts_a_reference_measurement_on_the_strip() {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.meas_full_record = true;
        put(&mut ui, 1, square(1.0, 2000, 40), 1e-6);
        ui.meas_src = WaveId::Ref(1);
        ui.m.meas.clear();
        ui.catalog_add(tile(measures::std_id(Meas::Vamp)));
        ui.catalog_add(tile(measures::std_id(Meas::LowTime)));
        let rows = ready_rows(&mut ui);
        let amp = rows.iter().find(|r| r.name == Meas::Vamp.name()).expect("amplitude");
        assert!((amp.v - 2.0).abs() < 0.15, "amplitude {}", amp.v);
        assert!(rows.iter().any(|r| r.name == "Low time" && r.v.is_finite()), "low time");
        ui.catalog_add(tile(measures::serial::DATA_RATE));
        let rate = ready_rows(&mut ui).into_iter().find(|r| r.name == "NRZ Data Rate").unwrap();
        assert!(rate.v.is_finite() && rate.v > 0.0, "{}", rate.v);
    }

    #[test]
    fn power_jitter_and_double_pulse_return_numbers() {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.meas_full_record = true;
        let n = 2000;
        let v: Vec<f32> = (0..n).map(|i| (i as f32 * 0.02).sin()).collect();
        let i: Vec<f32> = (0..n).map(|i| 0.4 * (i as f32 * 0.02).sin()).collect();
        put(&mut ui, 0, v, 1e-4);
        put(&mut ui, 1, i, 1e-4);
        ui.meas_src = WaveId::Ref(0);
        ui.meas_src_b = WaveId::Ref(1);
        ui.m.cat.clear();
        ui.catalog_add(tile(measures::power::POWER_QUALITY));
        let p = ready_rows(&mut ui).into_iter().find(|r| r.name == "Power Quality").unwrap();
        assert!(p.v.is_finite() && p.v > 0.0, "power {}", p.v);
        put(&mut ui, 2, square(0.5, 4000, 20), 1e-9);
        ui.meas_src = WaveId::Ref(2);
        ui.catalog_add(tile(measures::jitter::EYE_HEIGHT));
        ui.catalog_add(tile(measures::jitter::TIE));
        let rows = ready_rows(&mut ui);
        assert!(rows.iter().any(|r| r.name == "Eye Height" && r.v.is_finite()), "eye");
        assert!(rows.iter().any(|r| r.name == "TIE" && r.v.is_finite()), "tie");

        let dt = 1e-9;
        let tend = 13e-6;
        let n = (tend / dt) as usize;
        let pwl = |pts: &[(f64, f64)], t: f64| -> f32 {
            if t <= pts[0].0 {
                return pts[0].1 as f32;
            }
            for w in pts.windows(2) {
                if t <= w[1].0 {
                    let u = (t - w[0].0) / (w[1].0 - w[0].0);
                    return (w[0].1 + u * (w[1].1 - w[0].1)) as f32;
                }
            }
            pts[pts.len() - 1].1 as f32
        };
        let gate = [
            (0.0, 0.0),
            (1e-6, 0.0),
            (1.02e-6, 10.0),
            (6e-6, 10.0),
            (6.02e-6, 0.0),
            (10e-6, 0.0),
            (10.02e-6, 10.0),
            (12e-6, 10.0),
            (12.02e-6, 0.0),
        ];
        let id = [
            (0.0, 0.0),
            (1.1e-6, 0.0),
            (1.2e-6, 10.0),
            (6.05e-6, 10.0),
            (6.15e-6, 0.0),
            (10.05e-6, 0.0),
            (10.15e-6, 10.0),
            (10.25e-6, 14.0),
            (10.35e-6, 10.0),
            (12e-6, 10.0),
        ];
        let vds = [
            (0.0, 400.0),
            (1.05e-6, 400.0),
            (1.15e-6, 1.0),
            (6.1e-6, 1.0),
            (6.2e-6, 400.0),
            (6.25e-6, 480.0),
            (6.4e-6, 400.0),
            (10.05e-6, 400.0),
            (10.15e-6, 1.0),
            (12e-6, 1.0),
        ];
        put(&mut ui, 3, (0..n).map(|k| pwl(&vds, k as f64 * dt)).collect(), dt);
        put(&mut ui, 4, (0..n).map(|k| pwl(&id, k as f64 * dt)).collect(), dt);
        put(&mut ui, 5, (0..n).map(|k| pwl(&gate, k as f64 * dt)).collect(), dt);
        ui.meas_src = WaveId::Ref(3);
        ui.meas_src_b = WaveId::Ref(4);
        ui.cat.c = WaveId::Ref(5);
        ui.catalog_add(tile(measures::double_pulse::VPEAK));
        let peak = ready_rows(&mut ui).into_iter().find(|r| r.name == "Vpeak").unwrap();
        assert!((peak.v - 480.0).abs() < 1.0, "vpeak {}", peak.v);
    }

    #[test]
    fn catalog_screenshots() {
        let Ok(dir) = std::env::var("OOS_CATALOG_DIR") else { return };
        let mut ui = crate::ui::testkit::demo_ui();
        for _ in 0..4 {
            ui.step(0.05);
        }
        ui.open_menu(MenuId::MeasAdd);
        ui.meas_src = WaveId::Ref(1);
        ui.cat.tab = 0;
        ui.cat.sel = measures::std_id(Meas::Vamp);
        ui.cat.open[0] = 1;
        ui.cat.scroll = 0;
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/oos-standard.png"), crate::store::png_of(&cv)).unwrap();
        ui.cat.tab = 3;
        ui.cat.sel = measures::jitter::JITTER_SUMMARY;
        ui.cat.open[3] = 1;
        ui.cat.scroll = 0;
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/oos-jitter.png"), crate::store::png_of(&cv)).unwrap();
        ui.cat.tab = measures::POWER;
        ui.cat.sel = measures::power::POWER_QUALITY;
        ui.cat.open[measures::POWER as usize] = 1;
        ui.cat.scroll = 0;
        ui.meas_src = WaveId::Ch(0);
        ui.meas_src_b = WaveId::Ch(1);
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/oos-power.png"), crate::store::png_of(&cv)).unwrap();
        for tab in 0..CATEGORIES.len() {
            ui.cat.tab = tab as u8;
            ui.cat.scroll = 0;
            ui.cat.open[tab] = 1;
            ui.cat.sel = tab_groups(tab as u8)[0].1[0].id;
            ui.render(&mut cv);
            std::fs::write(format!("{dir}/catalog-tab-{tab}.png"), crate::store::png_of(&cv)).unwrap();
        }
        ui.m.meas = ALL_MEAS.iter().map(|&kind| (kind, WaveId::Ref(0))).collect();
        ui.m.meas2 = (0..17).map(|kind| (kind, WaveId::Ref(0), WaveId::Ref(1))).collect();
        ui.meas_pages.selected = 0;
        ui.open_submenu(MenuId::MeasSelected);
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/catalog-selected.png"), crate::store::png_of(&cv)).unwrap();
    }
}
