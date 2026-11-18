// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Drawing the eye page: the pane and its bar, the eye with its heat scale, the column tab, the four charts and the
//! boxes of figures. Drawing only copies and plots what `Page` holds; a new plot goes here.

use super::*;

impl Ui {
    /// The eye in its pane, with Side / Under and Close on the bar. The trace stays in the plot.
    pub(in crate::ui) fn draw_eye_pane(&self, cv: &mut Canvas) {
        if !self.eye_open() {
            return;
        }
        let a = self.eye_pane_rect();
        cv.fill(a, BG);
        cv.rect(a, CARD_BORDER);
        self.draw_eye_bar(cv, a);
        self.draw_eye(cv);
    }

    pub(in crate::ui) fn eye_header_source(&self) -> Rect {
        let a = self.eye_pane_rect();
        Rect::new(a.x + 4, a.y + 1, 88.min((a.w - 58).max(1)), workspace::HEADER - 2)
    }

    pub(in crate::ui) fn eye_source_mode_label(&self) -> &'static str {
        match self.m.eye_src {
            WaveId::Ref(i) if self.session_is_sim && i == self.m.sim_slot => "SIM captures",
            WaveId::Ref(_) => "Full record",
            WaveId::Ch(_) if self.m.run == RunState::Stop => "Stopped",
            WaveId::Ch(_) => "Live",
        }
    }

    pub(in crate::ui) fn eye_place_control(&self) -> Rect {
        let a = self.eye_pane_rect();
        Rect::new(a.x + 96, a.y + 1, 48, workspace::HEADER - 2)
    }

    pub(in crate::ui) fn eye_header_tap(&mut self, x: i32, y: i32) {
        if self.eye_place_control().contains(x, y) {
            self.open_menu(MenuId::Eye);
            self.open_picker(Item::EyePlace);
            return;
        }
        let source = self.eye_header_source().contains(x, y);
        self.open_menu(MenuId::Eye);
        if source {
            self.open_picker(Item::EyeSrc);
        }
    }

    pub(in crate::ui) fn eye_rate_caption(&self) -> String {
        if self.m.eye_rate > 0.0 {
            format!("{}{}", if self.azr.eye_auto.enabled { "Est." } else { "" }, si_short(self.m.eye_rate, "b/s"))
        } else {
            "Auto rate".into()
        }
    }

    fn draw_eye_bar(&self, cv: &mut Canvas, a: Rect) {
        let close = self.eye_pane_close();
        cv.fill(Rect::new(a.x + 1, a.y + 1, a.w - 2, workspace::HEADER - 1), BAR);
        let source = match self.m.eye_src {
            WaveId::Ch(c) => format!("CH{}", c + 1),
            WaveId::Ref(i) => format!("R{}", i + 1),
        };
        let rate = self.eye_rate_caption();
        let pick = self.eye_header_source();
        btn(cv, pick, BTN, BTN_BORDER);
        cv.text(&self.fonts().sb, pick.x + 5, a.y + 18, &format!("Eye {}", source), self.wave_color(self.m.eye_src));
        chevron_down(cv, pick.x + pick.w - 14, pick.y + pick.h / 2 - 2, DIM);
        let place = self.eye_place_control();
        btn(cv, place, BTN, BTN_BORDER);
        cv.text_in(&self.fonts().s, place, 1, &self.value(Item::EyePlace), TEXT);
        let settings = Rect::new(place.x + place.w + 4, pick.y, (close.x - place.x - place.w - 8).max(1), pick.h);
        btn(cv, settings, BTN, BTN_BORDER);
        cv.set_clip(Rect::new(settings.x + 5, settings.y, (settings.w - 23).max(1), settings.h));
        let scope = self.azr.eye_auto.snapshot_caption().unwrap_or_else(|| self.eye_source_mode_label().into());
        cv.text(&self.fonts().s, settings.x + 5, a.y + 18, &format!("{} · {}", scope, rate), TEXT);
        cv.no_clip();
        chevron_down(cv, settings.x + settings.w - 14, settings.y + settings.h / 2 - 2, DIM);
        btn(cv, close, BTN, BTN_BORDER);
        cv.text_in(&self.fonts().s, close, 1, "Close", TEXT);
    }

    pub(in crate::ui) fn draw_eye(&self, cv: &mut Canvas) {
        let a = self.eye_page_rect();
        let p = &self.azr.page;
        // full screen drops the column: the eye is the whole plot, and the plot button still goes back
        if self.full_on() {
            let l = layout(a, true);
            cv.fill(a, BG);
            self.eye_plot(cv, &l);
            return;
        }
        let l = placed(a, self.eye_column_open());
        cv.fill(a, BG);
        self.eye_plot(cv, &l);
        if l.fig_rows > 0 {
            self.eye_box(
                cv,
                l.fig,
                "Eye figures",
                l.fig_rows,
                &[("Measured", &p.eye_measured), ("Model", &p.eye_model), ("Records", &p.stats)],
            );
        }
        if l.charts[0].x < a.x + a.w {
            cv.set_clip(a);
            // One workspace header owns source/rate and docking; charts need no duplicate title.
            let (charts, plots) = charts_shown(&l, self.eye_big);
            let col = self.wave_color(self.m.eye_src);
            for k in 0..4 {
                if self.eye_big.is_some_and(|b| b as usize != k) {
                    continue;
                }
                if charts[k].w < 40 || charts[k].h < 36 {
                    continue;
                }
                self.chart_frame(cv, charts[k], plots[k], &p.charts[k]);
                if p.charts[k].empty.is_empty() {
                    match k {
                        0 => Self::plot_trend(cv, plots[0], &p.trend, col),
                        1 => Self::plot_hist(cv, plots[1], &p.hist, col),
                        2 => Self::plot_spec(cv, plots[2], &p.spec, col),
                        _ => self.plot_tub(cv, plots[3], &p.tub),
                    }
                }
            }
            if l.jit_rows > 0 {
                self.eye_box(
                    cv,
                    l.jit,
                    "Jitter breakdown",
                    l.jit_rows,
                    &[("Measured", &p.measured), ("Model", &p.model)],
                );
            }
            cv.no_clip();
        }
        self.draw_eye_tab(cv);
    }

    /// Left edge of the heat scale on plot `r`. On the usual split it is the top right of the plot; with the column
    /// away that corner belongs to the full-screen button, so the scale stops short of it.
    #[cfg(test)]
    pub(super) fn heat_x(&self, r: Rect) -> i32 {
        let src = match self.src.name() {
            "DEMO" => "Demo signals",
            "REPLAY" => "Replay",
            _ => "",
        };
        let info = if src.is_empty() { 0 } else { self.fonts().s.width(src) + 16 };
        let end = 68 + self.fonts().s.width("dense") + 4;
        let button = self.full_btn().0;
        let candidate = r.x + r.w - 128;
        if button.x >= r.x && button.x < r.x + r.w {
            let limit = button.x - 8 - info;
            candidate.min(limit - end).max(r.x)
        } else {
            candidate.max(r.x)
        }
    }

    fn draw_eye_tab(&self, cv: &mut Canvas) {
        if layout(self.eye_page_rect(), false).charts[0].w == 0 {
            return;
        }
        let r = self.eye_tab();
        cv.round(r, 6, PANEL_BG, 235);
        cv.round_outline(r, 6, BTN_BORDER);
        chevron(cv, r.x + r.w / 2, r.y + r.h / 2, if self.eye_to > 0.5 { 1 } else { -1 }, TEXT);
    }

    /// The eye: frame, axes in volts and UI, the coloured hits and the two levels.
    fn eye_plot(&self, cv: &mut Canvas, l: &Layout) {
        let f = self.fonts();
        let p = &self.azr.page;
        let r = l.plot;
        cv.fill(r, BAR);
        // half a UI per time line, the crossings stand out
        for k in 0..=4 {
            let x = r.x + k * (r.w - 1) / 4;
            cv.vline(x, r.y, r.y + r.h - 1, if k % 2 == 1 { GRID_AXIS } else { GRID_LINE });
            let s = ["-0.5", "0", "0.5", "1", "1.5 UI"][k as usize];
            let w = f.s.width(s);
            let tx = if k == 4 {
                x - w
            } else if k == 0 {
                x
            } else {
                x - w / 2
            };
            cv.text(&f.s, tx, r.y + r.h + 13, s, AXIS_TEXT);
        }
        if let Some(e) = &self.azr.eye {
            let (top, span) = (e.v_max as f64, (e.v_max - e.v_min) as f64);
            let lines = if p.vstep > 0.0 { (span / p.vstep).round() as i32 } else { 0 };
            for k in 0..=lines {
                let v = top - k as f64 * p.vstep;
                let y = r.y + k * (r.h - 1) / lines.max(1);
                cv.hline(r.x, r.x + r.w - 1, y, GRID_LINE);
                // a sum of steps misses zero by a rounding error
                let s = si_short(if v.abs() < 1e-3 * p.vstep { 0.0 } else { v }, "V");
                let ty = if k == 0 {
                    y + 10
                } else if k == lines {
                    y
                } else {
                    y + 4
                };
                cv.text(&f.s, r.x - 4 - f.s.width(&s), ty, &s, AXIS_TEXT);
            }
            blit_hits(cv, r, &p.img, p.img_w);
            if let Some((one, zero)) = p.levels {
                let span = (e.v_max - e.v_min) as f64;
                for v in [one, zero] {
                    let y = r.y + ((e.v_max as f64 - v) / span * (r.h - 1) as f64) as i32;
                    if y > r.y && y < r.y + r.h - 1 {
                        cv.dashed_hline(r.x, r.x + r.w - 1, y, 6, 6, LABEL);
                    }
                }
            }
        }
        if p.img.is_empty() {
            cv.text_in(&f.sb, r, 1, &p.status[0], DIM);
        }
        cv.rect(r, GRID_FRAME);
    }

    /// Box, title, axis ends of a chart, or the reason it is empty.
    fn chart_frame(&self, cv: &mut Canvas, c: Rect, r: Rect, ch: &Chart) {
        let f = self.fonts();
        cv.round(c, 4, BAR, 255);
        cv.round_outline(c, 4, CARD_BORDER);
        // a title wider than the card takes the smaller font, then loses its last words (never a cut word)
        let room = c.w - 12;
        let mut title = ch.title.clone();
        let font = if f.sb.width(&title) <= room { &f.sb } else { &f.s };
        while font.width(&title) > room {
            let Some((head, _)) = title.trim_end().rsplit_once(' ') else { break };
            title = head.trim_end().trim_end_matches(" at").to_string();
        }
        cv.text(font, c.x + 6, c.y + 13, &title, TEXT);
        cv.fill(r, BG);
        cv.rect(r, GRID_AXIS);
        if !ch.empty.is_empty() {
            cv.text_in(&f.s, r, 1, &ch.empty, DIM);
            return;
        }
        // a wide label stays inside the frame instead of running over its border
        let lx = |w: i32| (r.x - 3 - w).max(c.x + 3);
        cv.text(&f.s, lx(f.s.width(&ch.y.0)), r.y + 10, &ch.y.0, AXIS_TEXT);
        cv.text(&f.s, lx(f.s.width(&ch.y.1)), r.y + r.h - 1, &ch.y.1, AXIS_TEXT);
        cv.text(&f.s, r.x, r.y + r.h + 12, &ch.x.0, AXIS_TEXT);
        cv.text(&f.s, r.x + r.w - f.s.width(&ch.x.1), r.y + r.h + 12, &ch.x.1, AXIS_TEXT);
    }

    fn plot_trend(cv: &mut Canvas, r: Rect, t: &[(f32, f32)], col: Rgb) {
        let mid = r.y + r.h / 2;
        cv.hline(r.x + 1, r.x + r.w - 2, mid, GRID_AXIS);
        let y = |v: f32| mid - (v * (r.h / 2 - 2) as f32) as i32;
        let n = t.len().max(2) as i32 - 1;
        let x = |k: i32| r.x + 1 + k * (r.w - 3) / n;
        let mut prev: Option<(i32, i32)> = None;
        for (k, &(lo, hi)) in t.iter().enumerate() {
            let (px, c) = (x(k as i32), y(0.5 * (lo + hi)));
            cv.vline(px, y(hi), y(lo), col);
            if let Some(q) = prev {
                cv.line(q.0, q.1, px, c, col);
            }
            prev = Some((px, c));
        }
    }

    fn plot_hist(cv: &mut Canvas, r: Rect, h: &[f32], col: Rgb) {
        let n = h.len().max(1) as i32;
        for (k, &v) in h.iter().enumerate() {
            let k = k as i32;
            let (x0, x1) = (r.x + 1 + k * (r.w - 2) / n, r.x + 1 + (k + 1) * (r.w - 2) / n);
            let bh = ((v * (r.h - 3) as f32) as i32).max((v > 0.0) as i32);
            cv.fill(Rect::new(x0, r.y + r.h - 1 - bh, (x1 - x0).max(1), bh), col);
        }
    }

    fn plot_spec(cv: &mut Canvas, r: Rect, s: &[f32], col: Rgb) {
        let y = |db: f32| r.y + 2 + (-db / SPECTRUM_DB * (r.h - 4) as f32) as i32;
        let n = (s.len() as i32 - 1).max(1);
        let mut prev: Option<(i32, i32)> = None;
        for (k, &db) in s.iter().enumerate() {
            let p = (r.x + 1 + k as i32 * (r.w - 3) / n, y(db));
            if let Some(q) = prev {
                cv.line(q.0, q.1, p.0, p.1, col);
            }
            prev = Some(p);
        }
    }

    fn plot_tub(&self, cv: &mut Canvas, r: Rect, b: &Bathtub) {
        let y = |l: f32| r.y + 1 + ((l / TUB_FOOT).clamp(0.0, 1.0) * (r.h - 3) as f32) as i32;
        cv.dashed_hline(r.x + 1, r.x + r.w - 2, y(-12.0), 4, 4, GRID_FRAME);
        let n = (b.ui.len() as i32 - 1).max(1);
        let x = |k: usize| r.x + 1 + k as i32 * (r.w - 3) / n;
        let f = self.fonts();
        // the legend sits between the axis ends ("0", "1 UI"); a plot too narrow for all three leaves it out
        let (lo, hi) = (r.x + f.s.width("0") + 8, r.x + r.w - f.s.width("1 UI") - 8);
        let legend = f.s.width("counted") + 8 + f.s.width("model");
        if legend <= hi - lo {
            let x0 = (r.x + r.w / 2 - legend / 2).clamp(lo, hi - legend);
            let x1 = cv.text(&f.s, x0, r.y + r.h + 12, "counted", MEASURED);
            cv.text(&f.s, x1 + 8, r.y + r.h + 12, "model", MODELLED);
        }
        for (curve, c) in [(&b.model, MODELLED), (&b.measured, MEASURED)] {
            let mut prev: Option<(i32, i32)> = None;
            for (k, &l) in curve.iter().enumerate() {
                if !l.is_finite() || l < TUB_FOOT {
                    prev = None;
                    continue;
                }
                let p = (x(k), y(l));
                if let Some(q) = prev {
                    cv.line(q.0, q.1, p.0, p.1, c);
                }
                prev = Some(p);
            }
        }
    }

    /// A text box: a title, then columns of figures under their heads. Rows that do not fit stay out, the first are
    /// the ones that matter most.
    fn eye_box(&self, cv: &mut Canvas, r: Rect, title: &str, rows: i32, cols: &[(&str, &[Cell])]) {
        // dense tables of figures: the normal text size at every setting, their rows and columns are made for it
        let f = crate::font::Fonts::get(0);
        cv.round(r, 4, BAR, 255);
        cv.round_outline(r, 4, CARD_BORDER);
        let cw = (r.w - 12) / cols.len() as i32;
        for (i, (head, cells)) in cols.iter().enumerate() {
            let x = r.x + 8 + i as i32 * cw;
            let tone = if *head == "Model" { MODELLED } else { LABEL };
            let name = if i == 0 { format!("{}: {}", title, head) } else { head.to_string() };
            cv.text(&f.sb, x, r.y + 15, &name, tone);
            if cells.is_empty() {
                cv.text(&f.s, x, r.y + BOX_HEAD + 10, "no data yet", DIM);
            }
            for (k, c) in cells.iter().take(rows as usize).enumerate() {
                let y = r.y + BOX_HEAD + 10 + k as i32 * ROW;
                let tone = match c.kind {
                    Kind::Measured => MEASURED,
                    Kind::Model => MODELLED,
                    Kind::Missing => DIM,
                };
                cv.text(&f.s, x, y, c.label, DIM);
                // a long value starts earlier, after its label, and stops at the end of its column
                let (end, w) = (x + cw - 6, f.s.width(&c.value));
                let vx = if x + LABEL_W + w <= end { x + LABEL_W } else { (end - w).max(x + f.s.width(c.label) + 8) };
                cv.set_clip(Rect::new(x, y - ROW, end - x, ROW + 4));
                cv.text(&f.s, vx, y, &c.value, tone);
                cv.no_clip();
            }
        }
    }
}
