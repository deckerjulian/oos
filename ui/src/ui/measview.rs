// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the two ways the measurements panel shows its rows. List: compact rows, with a small line of statistics
//! under the value when they are on. Detailed: a table (source, name, value, mean, min, max, sigma, count) that drops
//! sigma and count when the panel is narrow and scrolls sideways when what is left does not fit.
use super::*;
use super::studio::{chip, icon_pin, MeasKey, StudioView};

/// The columns of the detailed table: name and width. Sigma and Count go first when the room is short.
/// Widths when the panel is narrow: the source is a colour bar, so Name, Value and Mean fit.
pub(super) const DETAIL_NARROW_W: [i32; 8] = [44, 54, 70, 66, 64, 64, 56, 52];
/// Below this width the source is a colour bar only (no name).
pub(super) const DETAIL_BAR_ONLY_BELOW: i32 = 240;
/// The room the pin keeps at the right end of the Name cell.
pub(super) const DETAIL_PIN_W: i32 = 18;
/// Below this width the table uses them.
pub(super) const DETAIL_NARROW_BELOW: i32 = 330;
pub(super) const DETAIL_COLS: [(&str, i32); 8] = [("Source", 44), ("Name", 104), ("Value", 90), ("Mean", 84), ("Min", 84), ("Max", 84), ("\u{3c3}", 76), ("Count", 64)];
/// Row height of the detailed table.
pub(super) const DETAIL_ROW: i32 = 28;
/// Height of the column heads of the table.
pub(super) const COL_HEAD: i32 = 24;
/// Extra height of a list row for the line of statistics.
pub(super) const STATS_LINE: i32 = 16;

impl Ui {
    /// The panel shows the table: the mode is set, and a panel (not the SIM list or the full-body table) is there.
    pub(super) fn studio_detailed(&self) -> bool {
        self.m.layout == Layout::Studio && self.m.studio_detail && matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline)
    }

    pub(super) fn studio_set_detailed(&mut self, on: bool) {
        self.m.studio_detail = on;
        self.studio.panel.scroll_top();
        self.studio.panel.hx = 0;
        self.studio.md.menu = None;
        self.relayout();
    }

    /// The rows of the list that are on the page, each with its rectangle. A row that starts the group of a source (or the
    /// page) has a band for the header; the others are as high as their body. `pos` indexes `listed`.
    pub(super) fn panel_layout(&self, rows: &[tools::MeasRow], listed: &[usize], first: usize, list: Rect) -> Vec<(usize, Rect)> {
        let body = self.studio_row_body();
        let multi = self.studio_multi_source();
        let mut y = list.y;
        let mut out = Vec::new();
        for pos in first..listed.len() {
            let group_first = multi && (pos == first || rows[listed[pos - 1]].src != rows[listed[pos]].src);
            let h = if self.studio_detailed() { DETAIL_ROW } else { body + if group_first { super::studio::GROUP_BAND } else { 0 } };
            if y + h > list.bottom() { break; }
            out.push((pos, Rect::new(list.x, y, list.w, h)));
            y += h;
        }
        out
    }

    /// Statistics are on for all rows (the Statistics switch) or for some.
    pub(super) fn studio_stats_any(&self) -> bool {
        self.studio.panel.stats || !self.studio.md.stats_rows.is_empty()
    }

    pub(super) fn studio_stats_on(&self, key: MeasKey) -> bool {
        self.studio.panel.stats || self.studio.md.stats_rows.contains(&key)
    }

    /// Mean, minimum, maximum, sigma and count of row `i`; None until it has a value.
    pub(super) fn studio_stat_values(&self, i: usize) -> Option<(f64, f64, f64, f64, u64)> {
        let st = self.mstat.get(i).copied().filter(|s| s.n > 0)?;
        let mean = st.sum / st.n as f64;
        let sd = (st.sumsq / st.n as f64 - mean * mean).max(0.0).sqrt();
        Some((mean, st.min, st.max, sd, st.n))
    }

    /// The columns that are shown for a table `w` wide: (index in `DETAIL_COLS`, x, width), and the total width.
    pub(super) fn detail_cols(w: i32) -> (Vec<(usize, i32, i32)>, i32) {
        let narrow = w < DETAIL_NARROW_BELOW;
        let mut ws: Vec<i32> = (0..DETAIL_COLS.len()).map(|i| if narrow { DETAIL_NARROW_W[i] } else { DETAIL_COLS[i].1 }).collect();
        if w < DETAIL_BAR_ONLY_BELOW { ws[0] = 12; ws[1] = 54; ws[2] = 70; ws[3] = 70; }
        // the columns that fit, whole: source, name and value always; then by priority mean, min, max, sigma, count
        // (no half-drawn column, and nothing to scroll to)
        let mut keep: Vec<usize> = Vec::new();
        let mut sum = 0;
        for c in DETAIL_PRIORITY {
            if sum + ws[c] > w && c > 2 { break; }
            sum += ws[c];
            keep.push(c);
        }
        keep.sort_unstable();
        // the name takes what the table has over
        if sum < w { ws[1] += w - sum; sum = w; }
        let mut x = 0;
        let cols = keep.iter().map(|&i| { let c = (i, x, ws[i]); x += ws[i]; c }).collect();
        (cols, sum.max(x))
    }

    /// The cell of a column in a row `r` of the table, scrolled by `hx`.
    pub(super) fn detail_cell(r: Rect, x: i32, w: i32, hx: i32) -> Rect {
        Rect::new(r.x + x - hx + 6, r.y, (w - 8).max(1), r.h)
    }

    /// The name a row of the table shows in `room` pixels, and whether it is in the small font: the long name if it fits, else
    /// the short one (Pk-pk, Ampl), else the long one in the small font, else cut in the middle so that its end stays
    /// (Eye Height and Eye Height@BER stay apart).
    pub(super) fn detail_name(&self, key: Option<MeasKey>, long: &str, room: i32) -> (String, bool) {
        let f = self.fonts();
        if f.m.width(long) <= room { return (long.to_string(), false); }
        let short = key.map_or_else(|| long.to_string(), |k| self.studio_badge_name(k, long));
        if f.m.width(&short) <= room { return (short, false); }
        if f.s.width(&short) <= room { return (short, true); }
        (fit_middle(&f.s, &short, room), true)
    }

    /// The label of a column head in a cell `w` wide: the full word, the short one, or none.
    pub(super) fn detail_head(&self, c: usize, w: i32) -> &'static str {
        let f = self.fonts();
        let full = DETAIL_COLS[c].0;
        if f.sb.width(full) <= w { full } else if c == 0 && f.sb.width("Src") <= w { "Src" } else { "" }
    }

    /// How far the table can scroll sideways.
    pub(super) fn detail_hmax(&self, list_w: i32) -> i32 {
        (Self::detail_cols(list_w).1 - list_w).max(0)
    }

    /// The small line under a row's value: mean, minimum, maximum and sigma, as many as fit.
    pub(super) fn draw_panel_stats_line(&self, cv: &mut Canvas, r: Rect, i: usize, row: &tools::MeasRow) {
        let f = self.fonts();
        let base = r.h - STATS_LINE;
        let y = r.y + base + 4;
        let avail = r.w - 16;
        let Some((mean, min, max, sd, _)) = self.studio_stat_values(i) else {
            cv.text(&f.s, r.x + 8, y + 9, "statistics: waiting for values", DIM);
            return;
        };
        let mut parts = match self.studio.panel.stats_mode {
            2 => vec![format!("min {}", row.fmt(min)), format!("max {}", row.fmt(max))],
            _ => vec![format!("\u{b5} {}", row.fmt(mean)), format!("\u{3c3} {}", row.fmt(sd)), format!("min {}", row.fmt(min)), format!("max {}", row.fmt(max))],
        };
        while parts.len() > 1 && f.s.width(&parts.join("  ")) > avail { parts.pop(); }
        cv.set_clip(Rect::new(r.x + 4, r.y, r.w - 8, r.h));
        cv.text(&f.s, r.x + 8, y + 9, &parts.join("  "), DIM);
        cv.no_clip();
    }

    /// The rows of the table that are on the page, partly visible ones too: (index in `listed`, rectangle). The rows move by
    /// pixels: `sub` is how far the first one is scrolled up.
    pub(super) fn detail_layout(&self, g: &studio::PanelGeo, total: usize, first: usize) -> Vec<(usize, Rect)> {
        let sub = self.studio.panel.sub.clamp(0, DETAIL_ROW - 1);
        (first..total).map(|pos| (pos, Rect::new(g.list.x, g.list.y + (pos - first) as i32 * DETAIL_ROW - sub, g.list.w, DETAIL_ROW)))
            .take_while(|(_, r)| r.y < g.list.bottom()).collect()
    }

    /// The pin of a table row: the end of the name cell.
    pub(super) fn detail_pin_rect(&self, g: &studio::PanelGeo, r: Rect) -> Rect {
        let hx = self.studio.panel.hx.clamp(0, self.detail_hmax(g.list.w));
        let (cols, _) = Self::detail_cols(g.list.w);
        let name = cols.iter().find(|c| c.0 == 1).map_or(0, |c| Self::detail_cell(r, c.1, c.2, hx).right());
        Rect::new(name - 24, r.y, 28, r.h)
    }

    /// The detailed table in the panel: the column heads in its head row and one row per measurement.
    pub(super) fn draw_panel_detail(&self, cv: &mut Canvas, g: &studio::PanelGeo, rows: &[tools::MeasRow], keys: &[MeasKey], listed: &[usize], first: usize) {
        let f = self.fonts();
        let (cols, total) = Self::detail_cols(g.list.w);
        let hx = self.studio.panel.hx.clamp(0, (total - g.list.w).max(0));
        // the head: the names of the columns, scrolled with the rows
        let head = Rect::new(g.list.x, g.list.y - COL_HEAD, g.list.w, COL_HEAD);
        cv.fill(head, BAR);
        cv.set_clip(head);
        for &(i, x, w) in &cols {
            let cell = Self::detail_cell(head, x, w, hx);
            cv.text_in(&f.sb, cell, if i >= 2 { 2 } else { 0 }, self.detail_head(i, cell.w), LABEL);
        }
        cv.no_clip();
        cv.set_clip(g.list);
        let md = &self.studio.md;
        for (pos, r0) in self.detail_layout(g, listed.len(), first) {
            let i = listed[pos];
            // a row that is swiped or springs back slides left over a red Remove
            let off = match (keys.get(i), md.swipe, md.spring) {
                (Some(a), Some((b, dx)), _) if *a == b => dx,
                (Some(a), _, Some((b, dx))) if *a == b => dx as i32,
                _ => 0,
            };
            if off < 0 {
                let rev = Rect::new((r0.right() + off).max(r0.x), r0.y, (-off).min(r0.w), r0.h);
                cv.fill(rev, mix(PANEL_BG, RED, 150));
                cv.text_in(&f.mb, rev, 1, "Remove", TEXT);
            }
            let r = Rect::new(r0.x + off, r0.y, r0.w, r0.h);
            // alternating shading: no gaps, a thin line under each row
            cv.fill(r, if pos % 2 == 1 { mix(PANEL_BG, TEXT, 10) } else { PANEL_BG });
            let row = &rows[i];
            let stats = self.studio_stat_values(i);
            let pinned = keys.get(i).is_some_and(|key| self.studio_is_pinned(*key));
            for &(c, x, w) in &cols {
                let cell = Self::detail_cell(r, x, w, hx);
                let dash = || "----".to_string();
                match c {
                    // the source: a bar of its colour and, when there is room, its short name, centred in the row
                    0 => {
                        let col = self.wave_color(row.src);
                        cv.round(Rect::new(r.x + x - hx + 3, r.y + 4, 4, r.h - 8), 2, col, 255);
                        if w >= 30 {
                            let name = Rect::new(r.x + x - hx + 11, r.y, w - 12, r.h);
                            cv.clip_within(name);
                            cv.text_in(&f.sb, name, 0, &self.wave_name(row.src), col);
                            cv.set_clip(g.list);
                        }
                    }
                    // the thumbtack comes after the name, in a place of its own
                    1 => {
                        let room = cell.w - DETAIL_PIN_W;
                        let text = Rect::new(cell.x, cell.y, room.max(1), cell.h);
                        cv.clip_within(text);
                        let (shown, small) = self.detail_name(keys.get(i).copied(), &row.name, room);
                        cv.text_in(if small { &f.s } else { &f.m }, text, 0, &shown, TEXT);
                        cv.set_clip(g.list);
                        icon_pin(cv, cell.right() - 13, r.y + r.h / 2 + 1, pinned, if pinned { AMBER } else { DIM });
                    }
                    2 => cv.text_in(&f.mb, cell, 2, &row.text(), TEXT),
                    3 => cv.text_in(&f.m, cell, 2, &stats.map_or_else(dash, |s| row.fmt4(s.0)), TEXT),
                    4 => cv.text_in(&f.m, cell, 2, &stats.map_or_else(dash, |s| row.fmt4(s.1)), TEXT),
                    5 => cv.text_in(&f.m, cell, 2, &stats.map_or_else(dash, |s| row.fmt4(s.2)), TEXT),
                    6 => cv.text_in(&f.m, cell, 2, &stats.map_or_else(dash, |s| row.fmt4(s.3)), TEXT),
                    _ => cv.text_in(&f.m, cell, 2, &stats.map_or_else(dash, |s| s.4.to_string()), DIM),
                }
            }
            cv.hline(r.x, r.right() - 1, r.bottom() - 1, LINE);
        }
        cv.no_clip();
        // a mark at the edge that scrolls
        if total > g.list.w {
            let (bar_w, track) = ((g.list.w * g.list.w / total).max(24), g.list.w);
            let x = g.list.x + (track - bar_w) * hx / (total - g.list.w).max(1);
            cv.fill(Rect::new(x, g.list.bottom() - 3, bar_w, 2), DIM);
        }
    }

    /// The little switch in the strip of the measurements panel: Table, List, Cards, the one that is on lit.
    pub(super) fn draw_view_switch(&self, cv: &mut Canvas, e: Rect) {
        let b = Rect::new(e.x + 4, e.y + 2, 68, 22);
        chip(cv, b, BTN, BTN_BORDER);
        let (t, l, k) = (Rect::new(b.x + 2, b.y + 2, 21, 18), Rect::new(b.x + 23, b.y + 2, 21, 18), Rect::new(b.x + 44, b.y + 2, 21, 18));
        let view = self.panel_view();
        let at = match view { super::measpanel::PanelView::Detailed => t, super::measpanel::PanelView::List => l, super::measpanel::PanelView::Cards => k };
        cv.round(at, 4, ACCENT, 255);
        let ink = |on: bool| if on { DARK_TEXT } else { TEXT };
        let (cl, ck, ct) = (ink(at == l), ink(at == k), ink(at == t));
        // table: a grid; list: three lines; cards: four squares
        cv.rect(Rect::new(t.x + 4, t.y + 3, 12, 12), ct);
        cv.hline(t.x + 4, t.x + 15, t.y + 9, ct);
        cv.vline(t.x + 10, t.y + 3, t.y + 14, ct);
        for i in 0..3 { cv.hline(l.x + 4, l.right() - 5, l.y + 4 + i * 5, cl); }
        for (dx, dy) in [(0, 0), (7, 0), (0, 7), (7, 7)] { cv.rect(Rect::new(k.x + 4 + dx, k.y + 3 + dy, 6, 6), ck); }
    }
}

/// The columns in the order they earn their place when the table is narrow: source, name, value, mean, sigma, min, max, count.
const DETAIL_PRIORITY: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// `s` cut to `room` pixels with "..." in the middle, so that the start and the end of the name show.
pub(super) fn fit_middle(f: &crate::font::Font, s: &str, room: i32) -> String {
    if f.width(s) <= room { return s.to_string(); }
    let c: Vec<char> = s.chars().collect();
    for keep in (2..c.len()).rev() {
        let tail = (keep * 2 / 5).max(1);
        let head = keep - tail;
        let t: String = c[..head].iter().chain("...".chars().collect::<Vec<_>>().iter()).chain(c[c.len() - tail..].iter()).collect();
        if f.width(&t) <= room { return t; }
    }
    "...".to_string()
}
