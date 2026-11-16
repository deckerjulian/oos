// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the controls, the cards view and the dropdowns of the docked measurements panel. The controls (source, gate,
//! search, Pinned only, Add, statistics) are one bar: in the title strip when the panel is wide, in one slim toolbar
//! under it when it is narrow, with what does not fit behind a ... button. The views are Table, List and Cards
//! (measview.rs); there is no footer and no paging: the list scrolls by swipe (measscroll.rs).
use super::dock::{VKind, STRIP_H};
use super::studio::{chip, icon_pin, PanelGeo, StudioView};
use super::*;

/// Height of the slim toolbar of a narrow panel.
pub(super) const TOOL_H: i32 = 32;
/// A panel at least this wide has its controls in the title strip.
pub(super) const WIDE_BAR_FROM: i32 = 760;
/// A card of the cards view and the space between cards.
pub(super) const CARD_H: i32 = 98;
const CARD_GAP: i32 = 4;

/// The dropdown that is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PanelDrop {
    Source,
    Gate,
    /// the ... button: the controls that did not fit
    More,
}

/// The controls of the bar, in the order of the bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum BarItem {
    Source,
    Gate,
    Search,
    Pinned,
    Add,
    Stats,
}
const BAR_ORDER: [BarItem; 6] = [BarItem::Source, BarItem::Gate, BarItem::Search, BarItem::Pinned, BarItem::Stats, BarItem::Add];
/// The order in which controls go behind the ... button when the room is short. Add (+) stays: it sits next to the view switch.
const BAR_DROP: [BarItem; 4] = [BarItem::Gate, BarItem::Stats, BarItem::Source, BarItem::Pinned];
const MORE_W: i32 = 34;

/// One control: what is drawn, and the (larger) rectangle that takes the touch.
#[derive(Clone, Copy, Debug)]
pub(super) struct BarSlot {
    pub item: BarItem,
    pub r: Rect,
    pub hit: Rect,
}

pub(super) struct PanelBar {
    pub slots: Vec<BarSlot>,
    pub more: Option<BarSlot>,
    pub hidden: Vec<BarItem>,
    /// in the title strip (else in the toolbar under it)
    pub wide: bool,
    /// the labelled chips (else compact ones)
    pub full: bool,
    /// the search field and its close button, when the search is open: it replaces the title (or the toolbar)
    pub field: Option<(Rect, Rect)>,
    /// the left edge of what the bar uses
    pub left: i32,
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

    /// The statistics button says what a tap shows next: Off, Mean + sigma, Min Max.
    pub(super) fn panel_stats_label(&self) -> &'static str {
        match self.studio.panel.stats_mode { 1 => "Mean + \u{3c3}", 2 => "Min Max", _ => "Stats off" }
    }

    pub(super) fn panel_stats_cycle(&mut self) {
        let mode = (self.studio.panel.stats_mode + 1) % 3;
        self.studio.panel.stats_mode = mode;
        self.studio.panel.stats = mode != 0;
        self.relayout();
    }

    /// The panel `p` (under its title strip) has its controls in the strip.
    pub(super) fn panel_bar_wide(p: Rect) -> bool { p.w >= WIDE_BAR_FROM }

    /// Height the controls take from the content: the slim toolbar of a narrow panel, nothing for a wide one.
    pub(super) fn panel_tool_h(p: Rect) -> i32 { if Self::panel_bar_wide(p) { 0 } else { TOOL_H } }

    /// The controls of the panel `p`: as many as fit, whole; the others are behind the ... button.
    pub(super) fn panel_bar(&self, p: Rect) -> PanelBar {
        let wide = Self::panel_bar_wide(p);
        // the row of the bar: the title strip, or the toolbar
        let (y, h) = if wide { (p.y - STRIP_H + 2, STRIP_H - 4) } else { (p.y + 2, TOOL_H - 4) };
        let (hit_y, hit_h) = if wide { (p.y - STRIP_H, 40) } else { (p.y, TOOL_H) };
        let right = if wide {
            let r = Rect::new(p.x, p.y - STRIP_H, p.w, p.h + STRIP_H);
            self.dock_view_rect(VKind::Measure, r).map_or(p.right() - 100, |v| v.x) - 6
        } else { p.right() - 3 };
        let left = if wide { p.x + 140 } else { p.x + 3 };
        let slot = |item, x: i32, w: i32| BarSlot { item, r: Rect::new(x, y, w, h), hit: Rect::new(x - 1, hit_y, w + 2, hit_h) };
        if self.studio.panel.searching {
            let xw = 30;
            let (fx, fw) = if wide { (p.x + 28, right - p.x - 28) } else { (p.x + 3, p.w - 6) };
            let field = Rect::new(fx, y, fw.max(60), h);
            let close = Rect::new(field.right() - xw, y, xw, h);
            return PanelBar { slots: Vec::new(), more: None, hidden: Vec::new(), wide, full: false, field: Some((field, close)), left: fx };
        }
        let width = |full: bool, it: BarItem| match (it, full) {
            (BarItem::Source, true) => 104, (BarItem::Source, false) => 56,
            (BarItem::Gate, true) => 84, (BarItem::Gate, false) => 48,
            (BarItem::Stats, true) => 74,
            _ => 34,
        };
        let gap = 3;
        let avail = right - left;
        let sum = |full: bool, keep: &[BarItem], more: bool| -> i32 {
            keep.iter().map(|&i| width(full, i) + gap).sum::<i32>() + if more { MORE_W + gap } else { 0 } - gap
        };
        let all: Vec<BarItem> = BAR_ORDER.to_vec();
        let (full, keep, more) = if sum(true, &all, false) <= avail { (true, all, false) }
            else if sum(false, &all, false) <= avail { (false, all, false) }
            else {
                let mut keep = all;
                for d in BAR_DROP {
                    keep.retain(|&i| i != d);
                    if sum(false, &keep, true) <= avail { break; }
                }
                (false, keep, true)
            };
        let hidden: Vec<BarItem> = BAR_ORDER.into_iter().filter(|i| !keep.contains(i)).collect();
        let total = sum(full, &keep, more);
        let mut x = if wide { right - total } else { left };
        let start = x;
        let mut slots = Vec::new();
        for it in keep {
            let w = width(full, it);
            slots.push(slot(it, x, w));
            x += w + gap;
        }
        let more = more.then(|| slot(BarItem::Add, x, MORE_W));
        PanelBar { slots, more, hidden, wide, full, field: None, left: start }
    }

    /// The control under a point; `Some(None)` is the ... button (or the search field).
    pub(super) fn panel_bar_hit(&self, p: Rect, x: i32, y: i32) -> Option<Option<BarItem>> {
        let bar = self.panel_bar(p);
        let (hy, hh) = if bar.wide { (p.y - STRIP_H, 40) } else { (p.y, TOOL_H) };
        if let Some((field, close)) = bar.field {
            if Rect::new(close.x, hy, close.w, hh).contains(x, y) { return Some(Some(BarItem::Search)); }
            return Rect::new(field.x, hy, field.w, hh).contains(x, y).then_some(None);
        }
        if let Some(s) = bar.slots.iter().find(|s| s.hit.contains(x, y)) { return Some(Some(s.item)); }
        bar.more.filter(|m| m.hit.contains(x, y)).map(|_| None)
    }

    /// The rectangle a dropdown opens under: its control, or the ... button when the control is behind it.
    fn panel_anchor(&self, p: Rect, which: PanelDrop) -> Option<Rect> {
        let bar = self.panel_bar(p);
        let item = match which { PanelDrop::Source => BarItem::Source, PanelDrop::Gate => BarItem::Gate, PanelDrop::More => return bar.more.map(|m| m.r) };
        bar.slots.iter().find(|s| s.item == item).map(|s| s.r).or(bar.more.map(|m| m.r))
    }

    /// What a control does when it is tapped.
    pub(super) fn panel_bar_do(&mut self, it: BarItem) {
        match it {
            BarItem::Source => self.studio.panel.drop = Some(PanelDrop::Source),
            BarItem::Gate => self.studio.panel.drop = Some(PanelDrop::Gate),
            BarItem::Search => self.panel_search_open(),
            BarItem::Pinned => {
                self.studio.panel.pinned_only = !self.studio.panel.pinned_only;
                self.studio.panel.scroll_top();
            }
            BarItem::Add => self.open_meas_list(),
            BarItem::Stats => self.panel_stats_cycle(),
        }
    }

    /// The search icon: the field opens in place of the title, with the keyboard.
    pub(super) fn panel_search_open(&mut self) {
        self.studio.panel.searching = true;
        self.studio.panel.query_osk = true;
        let q = self.studio.panel.query.clone();
        self.wlan.osk = Some(wlan::Osk::for_search(&q));
    }

    /// The close button of the field: the search is cleared and the title is back.
    pub(super) fn panel_search_close(&mut self) {
        self.studio.panel.searching = false;
        self.studio.panel.query.clear();
        self.studio.panel.scroll_top();
    }

    fn panel_bar_tap_item(&mut self, hit: Option<BarItem>) {
        match hit {
            Some(BarItem::Search) if self.studio.panel.searching => self.panel_search_close(),
            Some(it) => self.panel_bar_do(it),
            None if self.studio.panel.searching => self.panel_search_open(),
            None => self.studio.panel.drop = Some(PanelDrop::More),
        }
    }

    /// The sources the rows come from, in the order they first appear.
    pub(super) fn panel_sources(&self) -> Vec<WaveId> {
        let mut v: Vec<WaveId> = Vec::new();
        for r in self.meas_rows().iter().filter(|r| !self.meas_row_hidden(r)) {
            if !v.contains(&r.src) { v.push(r.src); }
        }
        v
    }

    /// A control's name in the ... list.
    fn panel_item_label(&self, it: BarItem) -> String {
        match it {
            BarItem::Source => format!("Source: {}", self.panel_source_label()),
            BarItem::Gate => format!("Gate: {}", self.panel_gate_label()),
            BarItem::Search => "Search".to_string(),
            BarItem::Pinned => "Pinned only".to_string(),
            BarItem::Add => "Add measurement".to_string(),
            BarItem::Stats => self.panel_stats_label().to_string(),
        }
    }

    fn panel_item_on(&self, it: BarItem) -> bool {
        let st = &self.studio.panel;
        match it {
            BarItem::Source => self.studio.filter.is_some(),
            BarItem::Gate | BarItem::Add => false,
            BarItem::Search => !st.query.is_empty(),
            BarItem::Pinned => st.pinned_only,
            BarItem::Stats => st.stats_mode != 0,
        }
    }

    fn panel_drop_options(&self, which: PanelDrop) -> Vec<(String, bool)> {
        match which {
            PanelDrop::Source => {
                let mut v = vec![("All sources".to_string(), self.studio.filter.is_none())];
                for s in self.panel_sources() { v.push((self.wave_name(s), self.studio.filter == Some(s))); }
                v
            }
            PanelDrop::Gate => vec![("Full record".to_string(), self.m.meas_full_record), ("Screen".to_string(), !self.m.meas_full_record)],
            PanelDrop::More => self.studio_panel_geo().map(|g| self.panel_bar(g.p).hidden).unwrap_or_default().into_iter().map(|it| (self.panel_item_label(it), self.panel_item_on(it))).collect(),
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
        let b = self.panel_anchor(g.p, which)?;
        let n = self.panel_drop_options(which).len() as i32;
        let w = b.w.max(if which == PanelDrop::More { 190 } else { 130 }).min(g.p.w - 8);
        let x = b.x.min(g.p.right() - 4 - w).max(g.p.x + 4);
        let h = n * 40 + 8;
        Some(Rect::new(x, (b.bottom() + 2).min(self.scr.h - h - 2), w, h))
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
                self.studio.panel.scroll_top();
            }
            PanelDrop::Gate => self.select_option(Item::MeasGate, if i == 0 { "Full record" } else { "Screen" }),
            PanelDrop::More => {
                let hidden = self.studio_panel_geo().map(|g| self.panel_bar(g.p).hidden).unwrap_or_default();
                if let Some(&it) = hidden.get(i) { self.panel_bar_do(it); }
            }
        }
        true
    }

    /// A tap on the toolbar of a narrow panel. False: not on it. (The bar in the title strip is taken by the dock.)
    pub(super) fn panel_head_tap(&mut self, g: &PanelGeo, x: i32, y: i32) -> bool {
        if Self::panel_bar_wide(g.p) { return false; }
        let Some(hit) = self.panel_bar_hit(g.p, x, y) else { return false };
        self.panel_bar_tap_item(hit);
        true
    }

    /// A tap on the bar in the title strip, from the dock. False: not on a control.
    pub(super) fn panel_strip_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(g) = self.studio_panel_geo() else { return false };
        let Some(hit) = self.panel_bar_hit(g.p, x, y) else { return false };
        self.panel_bar_tap_item(hit);
        true
    }

    /// The text of the search keyboard went to the panel.
    pub(super) fn panel_set_query(&mut self, text: &str) {
        self.studio.panel.query = text.to_string();
        self.studio.panel.query_osk = false;
        self.studio.panel.searching = !text.is_empty();
        self.studio.panel.scroll_top();
    }

    /// "6 results . 3 pinned": only while something filters the list.
    pub(super) fn panel_caption(&self) -> Option<String> {
        let st = &self.studio.panel;
        if st.query.is_empty() && self.studio.filter.is_none() && !st.pinned_only { return None; }
        let rows = self.meas_rows();
        Some(format!("{} results \u{b7} {} pinned", self.studio_listed(&rows).len(), self.studio_pinned().len()))
    }

    /// The controls of the bar (and the search field), drawn where `bar` says.
    pub(super) fn draw_panel_bar(&self, cv: &mut Canvas, bar: &PanelBar) {
        let f = self.fonts();
        let st = &self.studio.panel;
        if let Some((field, close)) = bar.field {
            chip(cv, field, BTN, ACCENT);
            let (cx, cy) = (field.x + 12, field.y + field.h / 2 - 1);
            cv.circle(cx, cy, 4, TEXT);
            cv.line(cx + 3, cy + 3, cx + 7, cy + 7, TEXT);
            let text = Rect::new(field.x + 24, field.y, (close.x - field.x - 26).max(1), field.h);
            cv.set_clip(text);
            if st.query.is_empty() { cv.text_in(&f.s, text, 0, "Search measurements", DIM); } else { cv.text_in(&f.m, text, 0, &st.query, TEXT); }
            cv.no_clip();
            let (xc, yc) = (close.x + close.w / 2, close.y + close.h / 2);
            cv.line(xc - 4, yc - 4, xc + 4, yc + 4, TEXT);
            cv.line(xc - 4, yc + 4, xc + 4, yc - 4, TEXT);
            return;
        }
        for s in bar.slots.iter().chain(bar.more.iter()) {
            let r = s.r;
            let more = bar.more.is_some_and(|m| m.r == r);
            let on = !more && self.panel_item_on(s.item);
            let open = match s.item { BarItem::Source => st.drop == Some(PanelDrop::Source), BarItem::Gate => st.drop == Some(PanelDrop::Gate), _ => false } || (more && st.drop == Some(PanelDrop::More));
            let lit = on && matches!(s.item, BarItem::Pinned | BarItem::Search | BarItem::Stats);
            let fill = if lit { mix(BTN, ACCENT, if s.item == BarItem::Pinned { 70 } else { 35 }) } else { BTN };
            chip(cv, r, fill, if open || on { ACCENT } else { BTN_BORDER });
            let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
            if more {
                for dx in [-6, 0, 6] { cv.circle(cx + dx, cy, 2, TEXT); }
                continue;
            }
            match s.item {
                BarItem::Source | BarItem::Gate => {
                    let label = if s.item == BarItem::Source {
                        if bar.full { self.panel_source_label() } else { self.studio.filter.map_or("All".to_string(), |id| self.wave_name(id)) }
                    } else if bar.full { self.panel_gate_label().to_string() } else if self.m.meas_full_record { "Full".to_string() } else { "Screen".to_string() };
                    let room = r.w - if bar.full { 26 } else { 8 };
                    let font = if f.m.width(&label) <= room { &f.m } else { &f.s };
                    cv.set_clip(Rect::new(r.x + 3, r.y, r.w - if bar.full { 20 } else { 6 }, r.h));
                    cv.text_in(font, Rect::new(r.x + 8, r.y, r.w - 10, r.h), if bar.full { 0 } else { 1 }, &label, if on { ACCENT } else { TEXT });
                    cv.no_clip();
                    if bar.full { chevron_down(cv, r.right() - 14, cy - 3, DIM); }
                }
                BarItem::Search => {
                    let c = if on { ACCENT } else { TEXT };
                    cv.circle(cx - 2, cy - 2, 5, c);
                    cv.circle(cx - 2, cy - 2, 3, fill);
                    cv.line(cx + 2, cy + 2, cx + 6, cy + 6, c);
                    cv.line(cx + 3, cy + 2, cx + 7, cy + 6, c);
                }
                BarItem::Pinned => icon_pin(cv, cx - 6, cy, on, if on { ACCENT } else { TEXT }),
                BarItem::Add => {
                    cv.hline(cx - 5, cx + 5, cy, TEXT);
                    cv.vline(cx, cy - 5, cy + 5, TEXT);
                }
                BarItem::Stats => {
                    let label = if bar.full { self.panel_stats_label() } else { "\u{3c3}" };
                    let font = if f.m.width(label) <= r.w - 6 { &f.m } else { &f.s };
                    cv.set_clip(r.inset(2));
                    cv.text_in(font, r, 1, label, if on { ACCENT } else { TEXT });
                    cv.no_clip();
                }
            }
        }
    }

    /// The toolbar of a narrow panel: its row under the title strip.
    pub(super) fn draw_panel_toolbar(&self, cv: &mut Canvas, g: &PanelGeo) {
        if Self::panel_bar_wide(g.p) { return; }
        let bar = self.panel_bar(g.p);
        self.draw_panel_bar(cv, &bar);
        cv.hline(g.p.x + 4, g.p.right() - 5, g.p.y + TOOL_H - 1, LINE);
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
