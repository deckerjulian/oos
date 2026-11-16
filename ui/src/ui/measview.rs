// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the two ways the measurements panel shows its rows. List: compact rows, with a small line of statistics
//! under the value when they are on. Detailed: a table (source, name, value, mean, min, max, sigma, count) that drops
//! sigma and count when the panel is narrow and scrolls sideways when what is left does not fit.
use super::*;
use super::studio::{chip, icon_pin, MeasKey, StudioView};

/// The columns of the detailed table: name and width. Sigma and Count go first when the room is short.
/// Widths when the panel is narrow: the source is a colour bar, so Name, Value and Mean fit.
pub(super) const DETAIL_NARROW_W: [i32; 8] = [30, 86, 76, 70, 76, 76, 70, 60];
/// Below this width the table uses them.
pub(super) const DETAIL_NARROW_BELOW: i32 = 330;
pub(super) const DETAIL_COLS: [(&str, i32); 8] = [("Source", 34), ("Name", 104), ("Value", 90), ("Mean", 84), ("Min", 84), ("Max", 84), ("\u{3c3}", 76), ("Count", 64)];
/// Row height of the detailed table.
pub(super) const DETAIL_ROW: i32 = 38;
/// Extra height of a list row for the line of statistics.
pub(super) const STATS_LINE: i32 = 16;

impl Ui {
    /// The panel shows the table: the mode is set, and a panel (not the SIM list or the full-body table) is there.
    pub(super) fn studio_detailed(&self) -> bool {
        self.m.layout == Layout::Studio && self.m.studio_detail && matches!(self.studio.view, StudioView::Docked | StudioView::Floating | StudioView::Offline)
    }

    pub(super) fn studio_set_detailed(&mut self, on: bool) {
        self.m.studio_detail = on;
        self.studio.panel.first = 0;
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
        let width = |i: usize| if w < DETAIL_NARROW_BELOW { DETAIL_NARROW_W[i] } else { DETAIL_COLS[i].1 };
        let all: i32 = (0..DETAIL_COLS.len()).map(width).sum();
        let keep = if all <= w { DETAIL_COLS.len() } else { DETAIL_COLS.len() - 2 };
        let mut x = 0;
        let cols = (0..keep).map(|i| { let c = (i, x, width(i)); x += width(i); c }).collect();
        (cols, x)
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

    /// The detailed table in the panel: the column heads in its head row and one row per measurement.
    pub(super) fn draw_panel_detail(&self, cv: &mut Canvas, g: &studio::PanelGeo, rows: &[tools::MeasRow], keys: &[MeasKey], listed: &[usize], first: usize) {
        let f = self.fonts();
        let (cols, total) = Self::detail_cols(g.list.w);
        let multi = self.studio_multi_source();
        let hx = self.studio.panel.hx.clamp(0, (total - g.list.w).max(0));
        // the head: the names of the columns, scrolled with the rows
        let head = Rect::new(g.list.x, g.list.y - 26, g.list.w, 26);
        cv.fill(head, BAR);
        cv.set_clip(head);
        for &(i, x, w) in &cols {
            cv.text_in(&f.sb, Rect::new(head.x + x - hx + 6, head.y, w - 6, head.h), 0, DETAIL_COLS[i].0, LABEL);
        }
        cv.no_clip();
        cv.set_clip(g.list);
        for (k, &i) in listed.iter().skip(first).take(g.page).enumerate() {
            let r = Rect::new(g.list.x, g.list.y + k as i32 * DETAIL_ROW, g.list.w, DETAIL_ROW);
            let row = &rows[i];
            let stats = self.studio_stat_values(i);
            let pinned = keys.get(i).is_some_and(|key| self.studio_is_pinned(*key));
            for &(c, x, w) in &cols {
                let cell = Rect::new(r.x + x - hx + 6, r.y, w - 8, r.h);
                let dash = || "----".to_string();
                match c {
                    // the source: a thin bar of its colour; its name only with several sources, in the first row of a group
                    0 => {
                        cv.round(Rect::new(r.x + x - hx + 3, r.y + 6, 4, r.h - 12), 2, self.wave_color(row.src), 255);
                        if multi && (first + k == 0 || rows[listed[first + k - 1]].src != row.src) {
                            cv.text(&f.sb, r.x + x - hx + 9, r.y + 14, &self.wave_name(row.src), self.wave_color(row.src));
                        }
                    }
                    // the pin comes after the name, in a place of its own
                    1 => {
                        let text = Rect::new(cell.x, cell.y, (cell.w - 16).max(1), cell.h);
                        cv.set_clip(Rect::new(text.x, text.y, text.w, text.h));
                        cv.text_in(&f.m, text, 0, &row.name, TEXT);
                        cv.no_clip();
                        if pinned { icon_pin(cv, cell.right() - 8, r.y + r.h / 2 + 1, true, AMBER); }
                    }
                    2 => cv.text_in(&f.mb, cell, 0, &row.text(), TEXT),
                    3 => cv.text_in(&f.m, cell, 0, &stats.map_or_else(dash, |s| row.fmt4(s.0)), TEXT),
                    4 => cv.text_in(&f.m, cell, 0, &stats.map_or_else(dash, |s| row.fmt4(s.1)), TEXT),
                    5 => cv.text_in(&f.m, cell, 0, &stats.map_or_else(dash, |s| row.fmt4(s.2)), TEXT),
                    6 => cv.text_in(&f.m, cell, 0, &stats.map_or_else(dash, |s| row.fmt4(s.3)), TEXT),
                    _ => cv.text_in(&f.m, cell, 0, &stats.map_or_else(dash, |s| s.4.to_string()), DIM),
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

    /// The little switch in the strip of the measurements panel: its two modes, the one that is on lit.
    pub(super) fn draw_view_switch(&self, cv: &mut Canvas, e: Rect) {
        let b = Rect::new(e.x + 4, e.y + 2, 68, 22);
        chip(cv, b, BTN, BTN_BORDER);
        let (l, k, t) = (Rect::new(b.x + 2, b.y + 2, 21, 18), Rect::new(b.x + 23, b.y + 2, 21, 18), Rect::new(b.x + 44, b.y + 2, 21, 18));
        let view = self.panel_view();
        let at = match view { super::measpanel::PanelView::List => l, super::measpanel::PanelView::Cards => k, super::measpanel::PanelView::Detailed => t };
        cv.round(at, 4, ACCENT, 255);
        let ink = |on: bool| if on { DARK_TEXT } else { TEXT };
        let (cl, ck, ct) = (ink(at == l), ink(at == k), ink(at == t));
        // list: three lines; cards: four squares; table: a grid
        for i in 0..3 { cv.hline(l.x + 4, l.right() - 5, l.y + 4 + i * 5, cl); }
        for (dx, dy) in [(0, 0), (7, 0), (0, 7), (7, 7)] { cv.rect(Rect::new(k.x + 4 + dx, k.y + 3 + dy, 6, 6), ck); }
        cv.rect(Rect::new(t.x + 4, t.y + 3, 12, 12), ct);
        cv.hline(t.x + 4, t.x + 15, t.y + 9, ct);
        cv.vline(t.x + 10, t.y + 3, t.y + 14, ct);
    }
}
