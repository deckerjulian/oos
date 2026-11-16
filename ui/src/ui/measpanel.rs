// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the header, the cards view and the footer of the docked measurements panel. The header has the source and
//! gate dropdowns, the search, the count and "Pinned only"; the views are List, Cards and Detailed (measview.rs); the
//! footer has "+ Add measurement" and the statistics button that cycles Off, Mean + sigma, Min Max.
use super::studio::{chip, icon_pin, PanelGeo, StudioView};
use super::*;

/// Height of the header: the dropdown row and the count row.
pub(super) const HEAD_H: i32 = 68;
/// A card of the cards view and the space between cards.
pub(super) const CARD_H: i32 = 98;
const CARD_GAP: i32 = 4;

/// The dropdown that is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PanelDrop {
    Source,
    Gate,
}

pub(super) struct PanelHead {
    pub src: Rect,
    pub gate: Rect,
    pub search: Rect,
    pub count: Rect,
    pub only: Rect,
}

/// How the panel shows its rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PanelView {
    List,
    Cards,
    Detailed,
}

impl Ui {
    pub(super) fn panel_is_cards(&self) -> bool {
        self.m.layout == Layout::Studio && self.studio.panel.cards && !self.m.studio_detail
            && matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline)
    }

    pub(super) fn panel_view(&self) -> PanelView {
        if self.m.studio_detail { PanelView::Detailed } else if self.studio.panel.cards { PanelView::Cards } else { PanelView::List }
    }

    pub(super) fn panel_set_view(&mut self, v: PanelView) {
        self.studio.panel.cards = v == PanelView::Cards;
        self.studio.panel.drop = None;
        self.studio_set_detailed(v == PanelView::Detailed);
    }

    /// The statistics button of the footer says what a tap shows next: Off, Mean + sigma, Min Max.
    pub(super) fn panel_stats_label(&self) -> &'static str {
        match self.studio.panel.stats_mode { 1 => "Mean + \u{3c3}", 2 => "Min Max", _ => "Stats off" }
    }

    pub(super) fn panel_stats_cycle(&mut self) {
        let mode = (self.studio.panel.stats_mode + 1) % 3;
        self.studio.panel.stats_mode = mode;
        self.studio.panel.stats = mode != 0;
        self.relayout();
    }

    /// The header: two dropdowns and the search, then the count and the Pinned only switch.
    pub(super) fn panel_head(&self, p: Rect) -> PanelHead {
        let w = p.w - 8;
        let search = Rect::new(p.right() - 4 - 36, p.y + 4, 36, 36);
        let dw = (w - 36 - 8) / 2;
        let src = Rect::new(p.x + 4, p.y + 4, dw, 36);
        let gate = Rect::new(src.right() + 4, p.y + 4, dw, 36);
        let only = Rect::new(p.right() - 4 - 112, p.y + 42, 112, 24);
        let count = Rect::new(p.x + 10, p.y + 42, (only.x - p.x - 12).max(1), 24);
        PanelHead { src, gate, search, count, only }
    }

    /// The sources the rows come from, in the order they first appear.
    pub(super) fn panel_sources(&self) -> Vec<WaveId> {
        let mut v: Vec<WaveId> = Vec::new();
        for r in self.meas_rows().iter().filter(|r| !self.meas_row_hidden(r)) {
            if !v.contains(&r.src) { v.push(r.src); }
        }
        v
    }

    fn panel_drop_options(&self, which: PanelDrop) -> Vec<(String, bool)> {
        match which {
            PanelDrop::Source => {
                let mut v = vec![("All sources".to_string(), self.studio.filter.is_none())];
                for s in self.panel_sources() { v.push((self.wave_name(s), self.studio.filter == Some(s))); }
                v
            }
            PanelDrop::Gate => vec![("Full record".to_string(), self.m.meas_full_record), ("Screen".to_string(), !self.m.meas_full_record)],
        }
    }

    pub(super) fn panel_source_label(&self) -> String {
        self.studio.filter.map_or("All sources".to_string(), |id| self.wave_name(id))
    }

    pub(super) fn panel_gate_label(&self) -> &'static str {
        if self.m.meas_full_record { "Full record" } else { "Screen" }
    }

    /// The popup list of the open dropdown: under its button, one row of 40 px per option.
    pub(super) fn panel_drop_rect(&self) -> Option<Rect> {
        let which = self.studio.panel.drop?;
        let g = self.studio_panel_geo()?;
        let h = self.panel_head(g.p);
        let b = if which == PanelDrop::Source { h.src } else { h.gate };
        let n = self.panel_drop_options(which).len() as i32;
        let w = b.w.max(130).min(g.p.w - 8);
        let x = b.x.min(g.p.right() - 4 - w);
        Some(Rect::new(x, b.bottom() + 2, w, n * 40 + 8))
    }

    pub(super) fn panel_drop_row(p: Rect, i: usize) -> Rect {
        Rect::new(p.x + 4, p.y + 4 + i as i32 * 40, p.w - 8, 40)
    }

    /// A tap while a dropdown is open: it picks an option or closes. Always taken.
    pub(super) fn panel_drop_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(which) = self.studio.panel.drop else { return false };
        let Some(p) = self.panel_drop_rect() else { self.studio.panel.drop = None; return false };
        self.studio.panel.drop = None;
        if !p.contains(x, y) { return true; }
        let opts = self.panel_drop_options(which);
        let Some(i) = (0..opts.len()).find(|&i| Self::panel_drop_row(p, i).contains(x, y)) else { return true };
        match which {
            PanelDrop::Source => {
                let sources = self.panel_sources();
                self.studio.filter = if i == 0 { None } else { sources.get(i - 1).copied() };
                self.studio.panel.first = 0;
            }
            PanelDrop::Gate => self.select_option(Item::MeasGate, if i == 0 { "Full record" } else { "Screen" }),
        }
        true
    }

    /// A tap on the header or the footer of the panel. False: not on them.
    pub(super) fn panel_head_tap(&mut self, g: &PanelGeo, x: i32, y: i32) -> bool {
        let h = self.panel_head(g.p);
        if Self::hit40(h.src).contains(x, y) {
            self.studio.panel.drop = Some(PanelDrop::Source);
        } else if Self::hit40(h.gate).contains(x, y) {
            self.studio.panel.drop = Some(PanelDrop::Gate);
        } else if h.search.contains(x, y) {
            if self.studio.panel.query.is_empty() {
                self.studio.panel.query_osk = true;
                self.wlan.osk = Some(wlan::Osk::for_search(""));
            } else {
                self.studio.panel.query.clear();
                self.studio.panel.first = 0;
            }
        } else if Self::hit40(h.only).contains(x, y) {
            self.studio.panel.pinned_only = !self.studio.panel.pinned_only;
            self.studio.panel.first = 0;
        } else if g.stats.contains(x, y) {
            self.panel_stats_cycle();
        } else {
            return false;
        }
        true
    }

    /// The text of the search keyboard went to the panel.
    pub(super) fn panel_set_query(&mut self, text: &str) {
        self.studio.panel.query = text.to_string();
        self.studio.panel.query_osk = false;
        self.studio.panel.first = 0;
    }

    pub(super) fn draw_panel_head(&self, cv: &mut Canvas, g: &PanelGeo, shown: usize) {
        let f = self.fonts();
        let h = self.panel_head(g.p);
        let st = &self.studio.panel;
        for (r, label, open) in [(h.src, self.panel_source_label(), st.drop == Some(PanelDrop::Source)), (h.gate, self.panel_gate_label().to_string(), st.drop == Some(PanelDrop::Gate))] {
            chip(cv, r, BTN, if open { ACCENT } else { BTN_BORDER });
            let font = if f.m.width(&label) <= r.w - 34 { &f.m } else { &f.s };
            cv.set_clip(Rect::new(r.x + 4, r.y, r.w - 26, r.h));
            cv.text_in(font, Rect::new(r.x + 10, r.y, r.w - 30, r.h), 0, &label, TEXT);
            cv.no_clip();
            chevron_down(cv, r.right() - 18, r.y + r.h / 2 - 3, DIM);
        }
        let on = !st.query.is_empty();
        chip(cv, h.search, if on { mix(BTN, ACCENT, 40) } else { BTN }, if on { ACCENT } else { BTN_BORDER });
        // a magnifier
        let (cx, cy) = (h.search.x + 16, h.search.y + 16);
        cv.circle(cx, cy, 6, if on { ACCENT } else { TEXT });
        cv.circle(cx, cy, 4, if on { mix(BTN, ACCENT, 40) } else { BTN });
        cv.line(cx + 4, cy + 4, cx + 9, cy + 9, if on { ACCENT } else { TEXT });
        cv.line(cx + 5, cy + 4, cx + 10, cy + 9, if on { ACCENT } else { TEXT });
        // the count, and the switch
        let pinned = self.studio_pinned().len();
        let text = if on { format!("{shown} results for \"{}\" \u{b7} {pinned} pinned", st.query) } else { format!("{shown} results \u{b7} {pinned} pinned") };
        cv.set_clip(h.count);
        cv.text_in(&f.s, h.count, 0, &text, DIM);
        cv.no_clip();
        let sw = Rect::new(h.only.right() - 32, h.only.y + 3, 30, 18);
        cv.round(sw, 9, if st.pinned_only { ACCENT } else { BTN_BORDER }, 255);
        cv.circle(if st.pinned_only { sw.right() - 9 } else { sw.x + 9 }, sw.y + 9, 6, if st.pinned_only { DARK_TEXT } else { DIM });
        cv.text_in(&f.s, Rect::new(h.only.x, h.only.y, h.only.w - 36, h.only.h), 2, "Pinned only", if st.pinned_only { TEXT } else { DIM });
    }

    pub(super) fn draw_panel_drop(&self, cv: &mut Canvas) {
        let (Some(which), Some(p)) = (self.studio.panel.drop, self.panel_drop_rect()) else { return };
        let f = self.fonts();
        cv.round(p, 8, ACCENT, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        for (i, (label, cur)) in self.panel_drop_options(which).into_iter().enumerate() {
            let r = Self::panel_drop_row(p, i);
            if cur { cv.round(r, 5, mix(PANEL_BG, ACCENT, 40), 255); }
            cv.set_clip(r);
            cv.text_in(&f.m, Rect::new(r.x + 10, r.y, r.w - 12, r.h), 0, &label, if cur { ACCENT } else { TEXT });
            cv.no_clip();
        }
    }

    /// The footer: the statistics button, in the place the + Panel button had.
    pub(super) fn draw_panel_stats_button(&self, cv: &mut Canvas, r: Rect) {
        let f = self.fonts();
        let on = self.studio.panel.stats_mode != 0;
        chip(cv, r, if on { mix(BTN, ACCENT, 30) } else { BTN }, if on { ACCENT } else { BTN_BORDER });
        let label = self.panel_stats_label();
        let font = if f.m.width(label) <= r.w - 8 { &f.m } else { &f.s };
        cv.set_clip(r.inset(2));
        cv.text_in(font, r, 1, label, TEXT);
        cv.no_clip();
    }

    // ------------------------------------------------------------------------------------------------ cards

    /// The cards that are on the page: two columns of cards. `pos` indexes `listed`.
    pub(super) fn panel_cards_layout(&self, first: usize, total: usize, list: Rect) -> Vec<(usize, Rect)> {
        let cw = (list.w - 10 - CARD_GAP) / 2;
        let per_col = ((list.h + CARD_GAP) / (CARD_H + CARD_GAP)).max(1) as usize;
        (first..total.min(first + per_col * 2)).map(|pos| {
            let k = pos - first;
            (pos, Rect::new(list.x + 2 + (k % 2) as i32 * (cw + CARD_GAP), list.y + (k / 2) as i32 * (CARD_H + CARD_GAP), cw, CARD_H))
        }).collect()
    }

    /// How many cards a page of the list holds.
    pub(super) fn panel_cards_page(list_h: i32) -> usize {
        (((list_h + CARD_GAP) / (CARD_H + CARD_GAP)).max(1) * 2) as usize
    }

    /// The pin of a card: top right.
    pub(super) fn panel_card_pin(r: Rect) -> Rect {
        Rect::new(r.right() - 34, r.y, 34, 34)
    }

    pub(super) fn draw_panel_cards(&self, cv: &mut Canvas, g: &PanelGeo, rows: &[tools::MeasRow], keys: &[studio::MeasKey], listed: &[usize], first: usize) {
        let f = self.fonts();
        let pinned = self.studio_pinned();
        let gate = if self.m.meas_full_record { "Full" } else { "Screen" };
        cv.set_clip(g.list);
        for (pos, r) in self.panel_cards_layout(first, listed.len(), g.list) {
            let i = listed[pos];
            let row = &rows[i];
            let is_pinned = keys.get(i).is_some_and(|k| pinned.contains(k));
            chip(cv, r, BTN, if is_pinned { mix(BTN_BORDER, AMBER, 90) } else { BTN_BORDER });
            // the name and the pin
            cv.set_clip(Rect::new(r.x + 4, r.y, r.w - 34, 24));
            cv.text(&f.s, r.x + 9, r.y + 17, &self.panel_row_name(keys.get(i).copied(), &row.name), LABEL);
            cv.set_clip(g.list);
            icon_pin(cv, r.right() - 22, r.y + 14, is_pinned, if is_pinned { AMBER } else { DIM });
            // the value
            cv.set_clip(Rect::new(r.x + 4, r.y + 20, r.w - 8, 28));
            cv.text(&f.lb, r.x + 9, r.y + 43, &row.text(), TEXT);
            cv.set_clip(g.list);
            // the statistics
            let st = self.studio_stat_values(i);
            let cell = |v: Option<f64>| v.map_or("----".to_string(), |v| row.fmt(v));
            let (a, b) = if self.studio.panel.stats_mode == 2 {
                (format!("Min {}", cell(st.map(|s| s.1))), format!("Max {}", cell(st.map(|s| s.2))))
            } else {
                (format!("Mean {}", cell(st.map(|s| s.0))), format!("Std dev {}", cell(st.map(|s| s.3))))
            };
            cv.set_clip(Rect::new(r.x + 4, r.y + 48, r.w - 8, 32));
            if self.studio.panel.stats_mode != 0 || st.is_some() {
                cv.text(&f.s, r.x + 9, r.y + 59, &a, DIM);
                cv.text(&f.s, r.x + 9, r.y + 73, &b, DIM);
            }
            cv.set_clip(g.list);
            // the foot: the source in its colour, the gate and the count
            let n = self.mstat.get(i).map_or(0, |s| s.n);
            let foot = if n > 0 { format!("{gate} \u{b7} n {n}") } else { gate.to_string() };
            let fw = f.s.width(&foot);
            let col = self.wave_color(row.src);
            cv.set_clip(Rect::new(r.x + 4, r.y + r.h - 20, r.w - 8, 18));
            cv.circle(r.x + 12, r.y + r.h - 11, 3, col);
            cv.text(&f.sb, r.x + 19, r.y + r.h - 7, &self.wave_name(row.src), col);
            cv.text(&f.s, (r.right() - 8 - fw).max(r.x + 19 + f.sb.width(&self.wave_name(row.src)) + 4), r.y + r.h - 7, &foot, DIM);
            cv.set_clip(g.list);
        }
        cv.no_clip();
    }

    /// The name a row shows: for a measurement of one source, its name (pair and catalogue names are whole already).
    pub(super) fn panel_row_name(&self, _key: Option<studio::MeasKey>, long: &str) -> String {
        long.to_string()
    }
}
