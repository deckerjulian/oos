// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The measurement strip and the results card: where they sit (under the plot, at its side or docked in the bottom
//! bar), how much room they take from the plot, and drawing their rows. A new place for them goes into
//! `Ui::strip_rect` and `Ui::results_rects`; what a row says comes from the measurement modules.

use super::*;

/// The statistics panel of the measurements, as the screen keeps it (ADR 0062).
#[derive(Clone, Copy, Default)]
pub(super) struct Results {
    /// the panel is shown (Measure > Statistics)
    pub on: bool,
    /// the panel is folded to its header
    pub folded: bool,
}

impl Ui {
    /// Height the strip takes when it sits under the plot or above the trace, with the measurement card if that is
    /// docked there too.
    pub(super) fn strip_rows_h(&self) -> i32 {
        let rows = self.strip_rows();
        (if rows == 0 { 0 } else { rows * self.lay().strip_row + 4 }) + self.results_band_h()
    }

    /// The measurement card in a dock (compact bottom bar), when it was docked rather than floated.
    pub(super) fn results_docked(&self) -> Option<StripDock> {
        (self.m.bar_compact
            && self.workspace.measure_docked
            && self.workspace.measure_float.is_none()
            && self.results_panel())
        .then_some(self.m.strip_dock)
    }

    /// Width a side-docked measurement card takes from the plot.
    fn results_side_w(&self) -> i32 {
        if matches!(self.results_docked(), Some(StripDock::Left | StripDock::Right)) {
            self.lay().results_w + 6
        } else {
            0
        }
    }

    /// Height a card docked under or above the plot takes: the header and two rows, paged.
    pub(super) fn results_band_h(&self) -> i32 {
        if !matches!(self.results_docked(), Some(StripDock::Under | StripDock::Above)) {
            return 0;
        }
        let total = self.measurement_count();
        let page = self.results_page_size();
        let lines = total.min(page).div_ceil(self.results_cols()) as i32;
        let nav = if total > page { 38 } else { 0 };
        30 + if self.results.folded { 0 } else { lines * self.results_row(total as i32) + nav + 4 } + 4
    }

    /// Left edge and width of the graticule. A side dock keeps a column for the strip and for a docked card.
    pub(super) fn plot_span(&self) -> (i32, i32) {
        let body = self.frame().body;
        let side = if self.strip_rows() > 0 { STRIP_SIDE } else { 0 } + self.results_side_w();
        match self.m.strip_dock {
            StripDock::Left => (body.x + side, body.w - side),
            StripDock::Right => (body.x, body.w - side),
            _ => (body.x, body.w),
        }
    }

    /// Top of the rows when the strip runs across the screen.
    pub(super) fn strip_band_y(&self) -> i32 {
        if self.m.strip_dock == StripDock::Above {
            let top = if self.full_on() { 0 } else { self.frame().body.y };
            return top + 2;
        }
        let g = self.plot.grid;
        // the rows sit on the cards: what the plot's whole divisions leave over goes between plot and rows
        (self.plot_bottom() + 2 - self.strip_rows() * self.lay().strip_row)
            .max(g.y + g.h + 4 + self.eye_below() + self.la_strip_h() + self.pane_h())
    }

    /// The column a side dock reserves, from the graticule down to the cards.
    pub(super) fn strip_column(&self) -> Rect {
        let y = self.plot.grid.y;
        let h = (self.plot_bottom() - y).max(0);
        let body = self.frame().body;
        let x = if self.m.strip_dock == StripDock::Left { body.x } else { body.right() - STRIP_SIDE };
        Rect::new(x, y, STRIP_SIDE, h)
    }

    /// Where the strip is drawn. None when it has nothing to show.
    #[cfg(test)]
    pub(super) fn strip_rect(&self) -> Option<Rect> {
        let rows = self.strip_rows();
        if rows == 0 {
            return None;
        }
        match self.m.strip_dock {
            StripDock::Left | StripDock::Right => Some(self.strip_column()),
            _ => Some(Rect::new(0, self.strip_band_y(), self.scr.w, rows * self.lay().strip_row)),
        }
    }

    /// One line of the strip when it is a column: channel pill on the first line of a group.
    pub(super) fn strip_side_lines(&self) -> Vec<StripLine> {
        let mut lines = Vec::new();
        let in_pane = self.results_pane_on();
        if !self.results_panel() && !in_pane {
            let rows = self.meas_rows();
            for id in self.meas_groups() {
                let col = self.wave_color(id);
                let label = self.wave_name(id);
                let mine: Vec<&tools::MeasRow> = rows.iter().filter(|r| r.src == id).collect();
                for (i, row) in mine.iter().enumerate() {
                    lines.push(StripLine {
                        pill: (i == 0).then(|| (label.clone(), col)),
                        name: row.name.clone(),
                        value: row.text(),
                        col,
                        align: i > 0,
                    });
                }
            }
        }
        if self.m.dvm_on || self.m.cnt_on {
            for (name, value) in self.dvm_row() {
                lines.push(StripLine { pill: None, name, value, col: TEXT, align: false });
            }
        }
        if self.m.cursor != CursorMode::Off && !in_pane {
            let mode = match self.m.cursor {
                CursorMode::Volt => "Volt",
                CursorMode::Track => "Track",
                CursorMode::Xy => "XY",
                _ => "Time",
            };
            let vals = self.cursor_values();
            if let Some((label, value)) = vals.first() {
                lines.push(StripLine {
                    pill: self.cursor_badge(),
                    name: format!("{mode} {label}"),
                    value: value.clone(),
                    col: TEXT,
                    align: false,
                });
            }
            for (name, value) in vals.iter().skip(1) {
                lines.push(StripLine { pill: None, name: name.clone(), value: value.clone(), col: TEXT, align: true });
            }
        }
        lines
    }

    /// Measurement rows and the cursor row. Measure > Dock puts them under the plot, above the trace, or on a side.
    pub(super) fn draw_strip(&self, cv: &mut Canvas) {
        if self.strip_rows() == 0 {
            return;
        }
        if self.draw_paged_measurement_strip(cv) {
            return;
        }
        match self.m.strip_dock {
            StripDock::Left | StripDock::Right => self.draw_strip_side(cv),
            _ => self.draw_strip_band(cv, self.strip_band_y()),
        }
    }

    /// The strip as rows across the screen, starting at `y`.
    pub(super) fn draw_strip_band(&self, cv: &mut Canvas, mut y: i32) {
        let f = self.fonts();
        // bar height and text baseline of a row; the source pill and the values right of it
        let bh = self.lay().strip_row - 2;
        let base = (bh + f.s.size * 72 / 100) / 2;
        let pw = 42.max(f.sb.width("CH4").max(f.sb.width("R10")) + 18);
        let vx = 8 + pw + 8;
        // a row: the width of the screen less the edges
        let row = |y: i32| Rect::new(METRICS.edge, y, self.scr.w - 2 * METRICS.edge, bh);
        let pill = |cv: &mut Canvas, y: i32, label: &str, col: Rgb| {
            let r = Rect::new(8, y + 1, pw, bh - 2);
            cv.round(r, 3, col, 255);
            cv.text_in(&f.sb, r, 1, label, DARK_TEXT);
        };
        let in_pane = self.results_pane_on();
        if !self.results_panel() && !in_pane {
            let rows = self.meas_rows();
            for id in self.meas_groups() {
                let mine: Vec<&tools::MeasRow> = rows.iter().filter(|r| r.src == id).collect();
                let col = self.wave_color(id);
                let label = self.wave_name(id);
                for part in mine.chunks(tools::STRIP_PER_ROW) {
                    cv.fill(row(y), BAR);
                    pill(cv, y, &label, col);
                    // the row's width shared out, whatever the number of values
                    let pitch = (row(y).right() - 2 - vx) / part.len() as i32;
                    for (i, r) in part.iter().enumerate() {
                        let x = vx + i as i32 * pitch;
                        let x2 = cv.text(&f.s, x, y + base, &r.name, DIM) + 8;
                        cv.text(&f.sb, x2, y + base, &r.text(), col);
                    }
                    y += self.lay().strip_row;
                }
            }
        }
        if self.m.dvm_on || self.m.cnt_on {
            cv.fill(row(y), BAR);
            let mut x = 12;
            for (l, v) in self.dvm_row() {
                x = cv.text(&f.s, x, y + base, &l, DIM) + 8;
                x = cv.text(&f.sb, x, y + base, &v, TEXT) + 24;
            }
            y += self.lay().strip_row;
        }
        if self.m.cursor != CursorMode::Off && !in_pane {
            cv.fill(row(y), BAR);
            if let Some((label, color)) = self.cursor_badge() {
                pill(cv, y, &label, color);
            }
            let mode = match self.m.cursor {
                CursorMode::Volt => "Volt",
                CursorMode::Track => "Track",
                CursorMode::Xy => "XY",
                _ => "Time",
            };
            let r = Rect::new(vx - 2, y + 2, 38.max(f.s.width("Track") + 8), bh - 4);
            cv.round_outline(r, 3, DIM);
            cv.text_in(&f.s, r, 1, mode, LABEL);
            let vals = self.cursor_values();
            let x0 = r.x + r.w + 10;
            // the pitch of the values, closer where a large text size would push the last one off the row
            let pitch = if matches!(self.m.cursor, CursorMode::Track | CursorMode::Xy) { 150 } else { 200 }
                .min((row(y).right() - 6 - x0) / vals.len().max(1) as i32);
            for (i, (l, v)) in vals.iter().enumerate() {
                let x = x0 + i as i32 * pitch;
                let x2 = cv.text(&f.s, x, y + base, l, DIM) + 8;
                cv.text(&f.sb, x2, y + base, v, TEXT);
            }
        }
    }

    /// The same read-outs in a column beside the graticule: one measurement a line, the value at the right.
    fn draw_strip_side(&self, cv: &mut Canvas) {
        let lines = self.strip_side_lines();
        if lines.is_empty() {
            return;
        }
        let col = self.strip_column();
        let n = lines.len() as i32;
        let strip_row = self.lay().strip_row;
        let room = (col.h - 4).max(strip_row);
        let pitch = if n * strip_row <= room { strip_row } else { (room / n).max(12) };
        let f = self.fonts();
        cv.set_clip(col);
        let mut y = col.y + 2;
        for line in &lines {
            let bar = Rect::new(col.x + 4, y, col.w - 8, (pitch - 2).max(12));
            cv.fill(bar, BAR);
            let mut text_x = bar.x + 6;
            if let Some((label, pill_col)) = &line.pill {
                let ph = (bar.h - 2).clamp(10, 14);
                let r = Rect::new(bar.x + 2, bar.y + (bar.h - ph) / 2, 42, ph);
                cv.round(r, 3, *pill_col, 255);
                cv.text_in(&f.sb, r, 1, label, DARK_TEXT);
                text_x = r.x + r.w + 6;
            } else if line.align {
                text_x = bar.x + 50;
            }
            let room_r = bar.x + bar.w - 4;
            let vw = if line.value.is_empty() { 0 } else { f.sb.width(&line.value) };
            // the value keeps the right edge; a long one is clipped there and the name gives way
            let vx = if vw == 0 { room_r } else { (room_r - vw).max(text_x) };
            let base = bar.y + bar.h - 4;
            if vw > 0 {
                cv.set_clip(Rect::new(text_x, bar.y, (room_r - text_x).max(0), bar.h));
                cv.text(&f.sb, vx, base, &line.value, line.col);
                cv.set_clip(col);
            }
            let name_w = vx - 6 - text_x;
            if name_w > 8 && !line.name.is_empty() {
                cv.set_clip(Rect::new(text_x, bar.y, name_w, bar.h));
                cv.text(&f.s, text_x, base, &line.name, DIM);
                cv.set_clip(col);
            }
            y += pitch;
            if y + 12 > col.y + col.h {
                break;
            }
        }
        cv.no_clip();
    }

    /// Header and body of the measurement results overlay (statistics on): top right of the plot, under the status
    /// line and the math label, left of the strip where a drag moves the trigger level. Folded: header only.
    pub(super) fn results_rects(&self) -> Option<(Rect, Rect)> {
        if !self.results_panel() {
            return None;
        }
        let a = self.over_area();
        let total = self.measurement_count();
        let page = self.results_page_size();
        let n = total.min(page).div_ceil(self.results_cols()) as i32;
        let nav = if total > page { 38 } else { 0 };
        let body_h = if self.results.folded { 0 } else { n * self.results_row(total as i32) + nav + 4 };
        let panel_h = 30 + body_h;
        let rw = self.lay().results_w * self.results_cols() as i32;
        let fr = self.frame();
        let (top, bottom, sw) = (fr.top.bottom(), fr.bottom.y, self.scr.w);
        let docked = self.results_docked();
        let strip_side = if self.strip_rows() > 0 { STRIP_SIDE } else { 0 };
        let strip_h = {
            let rows = self.strip_rows();
            if rows == 0 {
                0
            } else {
                rows * self.lay().strip_row + 4
            }
        };
        let (body_top, plot_x, plot_w) =
            (if self.full_on() { 0 } else { fr.body.y }, self.plot_span().0, self.plot_span().1);
        let default = match docked {
            Some(StripDock::Left) => (fr.body.x + strip_side + 2, body_top),
            Some(StripDock::Right) => (fr.body.right() - strip_side - rw - 2, body_top),
            Some(StripDock::Above) => (plot_x + (plot_w - rw) / 2, body_top + strip_h + 2),
            Some(StripDock::Under) => (plot_x + (plot_w - rw) / 2, bottom - strip_h - panel_h - 2),
            None => (
                a.x + a.w - 56 - rw,
                a.y + 22 + 26 * (0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).count() as i32,
            ),
        };
        let (mut x, mut y) = self.workspace.measure_float.unwrap_or(default);
        // A floating card never covers the corner buttons or the zoom bar.
        let floating = docked.is_none();
        for r in if floating { self.corner_btns().to_vec() } else { Vec::new() } {
            let r = Rect::new(r.x - 4, r.y - 4, r.w + 8, r.h + 8);
            if x + rw > r.x && x < r.right() && y < r.bottom() && y + panel_h > r.y {
                x = (r.x - rw).max(0);
            }
        }
        if floating && self.m.zoom && !self.view_on() {
            y = y.max(self.zoom_readout().bottom() + 2);
        }
        // Expanding a collapsed panel or changing its page must never put the
        // body and navigation beyond the touchable workspace.
        let x = x.clamp(0, sw - rw);
        let y = y.clamp(top, (bottom - panel_h).max(top));
        let head = Rect::new(x, y, rw, 30);
        Some((head, Rect::new(x, y + 30, rw, body_h)))
    }

    /// Keep source/value and mean/deviation readable even for large selections (two lines at the normal text size).
    pub(super) fn results_pitch(_n: i32) -> i32 {
        42
    }

    /// Height of a row of the results card at the text size: always two lines.
    pub(super) fn results_row(&self, n: i32) -> i32 {
        self.lay().res_pitch.1.max(Self::results_pitch(n))
    }

    pub(super) fn results_name(&self, index: usize, row: &tools::MeasRow) -> String {
        if let Some(&(kind, a, b)) = index.checked_sub(self.m.meas.len()).and_then(|i| self.m.meas2.get(i)) {
            format!("{}-{} {}", self.wave_name(a), self.wave_name(b), tools::PAIRS[kind.min(tools::PAIRS.len() - 1)].0)
        } else if index >= self.m.meas.len() + self.m.meas2.len() {
            self.selected_measurement_label(index)
        } else {
            format!("{} {}", self.wave_name(row.src), row.name)
        }
    }

    /// Measurement results with statistics as a translucent card over the plot: value, mean, ±σ, acquisitions;
    /// tapping the header folds it.
    pub(super) fn draw_results(&self, cv: &mut Canvas) {
        let Some((head, body)) = self.results_rects() else { return };
        let f = self.fonts();
        let all = Rect::new(head.x, head.y, head.w, head.h + body.h);
        cv.round(all, 6, CARD_BORDER, 255);
        cv.round(all.inset(1), 5, BAR, 215);
        if self.results.folded {
            chevron(cv, head.x + 13, head.y + 15, 1, DIM);
        } else {
            chevron_down(cv, head.x + 9, head.y + 12, DIM);
        }
        cv.text(&f.s, head.x + 24, head.y + 19, &format!("Meas · {}", self.measurement_gate_label()), TEXT);
        let n = self.mstat.first().map(|s| s.n).unwrap_or(0);
        let ns = format!("n {}", fmt_count(n as f64));
        cv.text(&f.s, head.x + head.w - 10 - f.s.width(&ns), head.y + 19, &ns, DIM);
        if self.results.folded {
            return;
        }
        let rows = self.meas_rows();
        let pitch = self.results_row(rows.len() as i32);
        let (one, two) = self.lay().res_pitch;
        // baselines: 16 and 35 px into a row at the normal text size
        let (y1, y2) = (one - 6, two - 7);
        let (first, end) = self.results_page_range(rows.len());
        let cols = self.results_cols();
        let cw = body.w / cols as i32;
        for (page_row, (i, r)) in rows.iter().enumerate().skip(first).take(end - first).enumerate() {
            // the rows of a card docked under or above the plot run in columns
            let body = Rect::new(body.x + (page_row % cols) as i32 * cw, body.y, cw, body.h);
            let by = body.y + (page_row / cols) as i32 * pitch;
            cv.hline(body.x + 8, body.x + body.w - 9, by, LINE);
            let st = self.mstat.get(i).copied().unwrap_or_default();
            let mean = st.sum / st.n.max(1) as f64;
            let name = self.results_name(i, r);
            let value = r.text();
            let value_x = body.x + body.w - 10 - f.sb.width(&value);
            let sd = if st.n > 1 {
                let sd = (st.sumsq / st.n as f64 - mean * mean).max(0.0).sqrt();
                format!("\u{3c3} {}", si(sd, r.unit))
            } else {
                String::new()
            };
            let sd_x = body.x + body.w - 10 - f.s.width(&sd);
            cv.set_clip(Rect::new(body.x + 10, by, (value_x - body.x - 15).max(1), y1 + 4));
            cv.text(&f.s, body.x + 10, by + y1, &name, LABEL);
            cv.no_clip();
            cv.text(&f.sb, value_x, by + y1, &value, self.wave_color(r.src));
            if st.n > 1 {
                cv.set_clip(Rect::new(body.x + 10, by + y1 + 4, (sd_x - body.x - 15).max(1), pitch - y1 - 4));
                cv.text(&f.s, body.x + 10, by + y2, &format!("\u{b5} {}", r.fmt(mean)), DIM);
                cv.no_clip();
                cv.text(&f.s, sd_x, by + y2, &sd, DIM);
            }
        }
        self.draw_results_page_buttons(cv, body, rows.len());
    }
}
