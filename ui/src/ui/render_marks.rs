// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The marks on the plot: the reference waveforms with their badges, the XY picture, the markers at the edges (trigger,
//! levels, offsets), the cursors with their readout, the info box and the search marks. A new mark gets its drawing
//! here and its call in `Ui::render_layers` (`render.rs`).

use super::*;

impl Ui {
    /// Vertical scale and the sample rate of this record. Time per division stays the one horizontal scale.
    pub(super) fn ref_badge_readout(r: &RefWave) -> (String, String) {
        let scale = format!("{}/div", si_short(r.vpd as f64, "V"));
        let rate = if r.dt.is_finite() && r.dt > 0.0 { si_short(1.0 / r.dt, "Sa/s") } else { "\u{2014}".into() };
        (scale, rate)
    }

    pub(super) fn ref_badge_lines(&self, slot: usize, r: &RefWave, maximum: i32) -> (String, String) {
        let tag = format!("R{}", slot + 1);
        let mut name = if r.label.trim().is_empty() || r.label.trim() == tag {
            tag.clone()
        } else {
            format!("{} {}", tag, r.label.trim())
        };
        let width = (maximum - 12).max(16);
        if self.fonts().sb.width(&name) > width {
            while name.len() > tag.len() && self.fonts().sb.width(&format!("{}...", name)) > width {
                name.pop();
            }
            if self.fonts().sb.width(&format!("{}...", name)) <= width {
                name.push_str("...");
            } else {
                name = tag;
            }
        }
        let (scale, rate) = Self::ref_badge_readout(r);
        (name, format!("{} · {}", scale, rate))
    }

    /// Name, volts per division, and the record's sample rate, beside the trace.
    pub(super) fn draw_ref_badge(&self, cv: &mut Canvas, area: Rect, y: i32, col: Rgb, slot: usize, r: &RefWave) {
        let f = self.fonts();
        let maximum = (area.w - 16).clamp(36, 300);
        let (name, readout) = self.ref_badge_lines(slot, r, maximum);
        let text_w = f.sb.width(&name).max(f.s.width(&readout));
        let bw = (text_w + 12).clamp(36, maximum);
        let bh = 34;
        let y0 = y.clamp(area.y + 2, (area.y + area.h - bh - self.ref_badge_pad()).max(area.y + 2));
        let x0 = (area.x + 80).min((area.x + area.w - bw - 4).max(area.x + 2));
        let b = Rect::new(x0, y0, bw, bh);
        cv.round(b, 3, col, 255);
        cv.set_clip(b);
        let ink = rgb(0, 0, 0);
        let row = b.h / 2;
        cv.text_in(&f.sb, Rect::new(b.x, b.y + 1, b.w, row), 1, &name, ink);
        cv.text_in(&f.s, Rect::new(b.x, b.y + row, b.w, row), 1, &readout, ink);
        cv.set_clip(area);
    }

    /// Room a ref badge keeps clear under it: the time labels along the bottom of the lowest lane.
    fn ref_badge_pad(&self) -> i32 {
        if self.stacked() && self.plot.cur_win + 1 < self.plot.wins.len() {
            2
        } else {
            self.fonts().s.size + 10
        }
    }

    pub(super) fn draw_refs(&self, cv: &mut Canvas) {
        self.draw_refs_at(cv, self.area, self.view(), false);
    }

    pub(super) fn draw_refs_at(&self, cv: &mut Canvas, area: Rect, view: (f32, f32), all_sources: bool) {
        cv.set_clip(area);
        let (tdiv, delay) = view;
        let (tdiv, delay) = (tdiv as f64, delay as f64);
        let ppx = area.w as f64 / HDIV as f64;
        if !(tdiv > 0.0) {
            cv.no_clip();
            return;
        }
        let half = tdiv * HDIV as f64 / 2.0;
        let (t_left, t_right) = (delay - half, delay + half);
        let mut badges: Vec<i32> = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            let Some(r) = r else { continue };
            let t0 = self.source_time_origin(WaveId::Ref(i), r.t0);
            if !all_sources
                && self.stacked()
                && self.trace_sources().get(self.plot.cur_win) != Some(&workspace::Trace::Reference(i))
            {
                continue;
            }
            // a split (not Stacked): each reference in the window its Windows row names
            if !all_sources && !self.stacked() && self.plot.wins.len() > 1 && self.ref_win_of(i) != self.plot.cur_win {
                continue;
            }
            if !r.on || r.v.is_empty() || !(r.dt > 0.0) {
                continue;
            }
            let col = REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1;
            // samples inside this window, not a stride of the whole record (that aliases a long record)
            let n = r.v.len() as isize;
            let at = |t: f64| -> isize {
                let x = (t - t0) / r.dt;
                if !x.is_finite() {
                    return 0;
                }
                x.floor().clamp(-1.0, n as f64) as isize
            };
            let k0 = (at(t_left) - 1).clamp(0, n) as usize;
            let k1 = (at(t_right) + 2).clamp(0, n) as usize;
            if k1 <= k0 {
                continue;
            }
            let cols = (area.w as usize).saturating_mul(2).max(1);
            let step = ((k1 - k0) / cols).max(1);
            let mut prev: Option<(i32, i32)> = None;
            let mut first = None;
            for k in (k0..k1).step_by(step).chain(std::iter::once(k1 - 1)) {
                let v = r.v[k];
                if !v.is_finite() {
                    prev = None;
                    continue;
                }
                let t = t0 + k as f64 * r.dt - delay;
                let x = (area.x as f64 + area.w as f64 / 2.0 + t / tdiv * ppx).round() as i32;
                let y = self.zoom_y_of_divs(area, (v + r.offset) / r.vpd).round() as i32;
                if let Some((x0, y0)) = prev {
                    if (x - x0).abs() < area.w * 2 && (y - y0).abs() < area.h * 4 {
                        cv.line(x0, y0, x, y, col);
                    }
                }
                if first.is_none() && x >= area.x && x < area.x + area.w {
                    first = Some(y);
                }
                prev = Some((x, y));
            }
            if let Some(y) = first {
                // badges of references that start at the same height are fanned out instead of printed over each other
                // none in the overview of a zoom (it shows the whole record, the detail has the badges)
                if !all_sources {
                    let (lo, hi) = (area.y + 2, (area.y + area.h - 34 - self.ref_badge_pad()).max(area.y + 2));
                    let spot = badge_spot(&badges, y, lo, hi);
                    badges.push(spot);
                    self.draw_ref_badge(cv, area, spot, col, i, r);
                }
            }
        }
        cv.no_clip();
    }

    pub(super) fn draw_xy(&self, cv: &mut Canvas) {
        // CH1 = X, CH2 = Y in a square of 8 x 8 div in the middle of the grid
        let Some(rec) = &self.last[0] else { return };
        let (a, b) = (&rec.ch[0], &rec.ch[1]);
        let f = self.fonts();
        let side = self.area.h;
        let sq = Rect::new(self.area.x + (self.area.w - side) / 2, self.area.y, side, side);
        cv.fill(sq, rgb(0x04, 0x04, 0x08));
        cv.rect(sq, GRID_AXIS);
        cv.vline(sq.x + side / 2, sq.y, sq.y + side - 1, GRID_LINE);
        cv.hline(sq.x, sq.x + side - 1, sq.y + side / 2, GRID_LINE);
        if a.is_empty() || b.is_empty() {
            cv.text_in(&f.m, sq, 1, "XY needs CH1 and CH2", DIM);
            return;
        }
        cv.set_clip(sq);
        let ppd = side as f32 / 8.0;
        let (ca, cb) = (&self.m.ch[0], &self.m.ch[1]);
        let col = rgb(0x9c, 0xf0, 0x9c);
        for k in 0..a.len().min(b.len()) {
            if !a[k].is_finite() || !b[k].is_finite() {
                continue;
            }
            let x = (sq.x as f32 + side as f32 / 2.0 + (a[k] + ca.offset) / ca.volts_per_div() * ppd) as i32;
            let y = (sq.y as f32 + side as f32 / 2.0 - (b[k] + cb.offset) / cb.volts_per_div() * ppd) as i32;
            cv.pixel(x, y, col, 160);
            cv.pixel(x + 1, y, col, 70);
            cv.pixel(x, y + 1, col, 70);
        }
        cv.no_clip();
        cv.text(&f.s, sq.x + 6, sq.y + 16, "XY  X = CH1  Y = CH2", col);
    }

    pub(super) fn draw_markers(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let a = self.area;
        // channel ground markers at the left edge of the grid
        for c in 0..NCH {
            if !self.m.ch[c].on || !self.shows(c) {
                continue;
            }
            let y = self.y_of_volts(c, 0.0).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
            let col = CH_COLORS[c];
            let r = Rect::new(a.x + 2, y - 8, 16, 16);
            cv.round(r, 3, col, 255);
            cv.triangle([(a.x + 17, y - 6), (a.x + 24, y), (a.x + 17, y + 6)], col);
            cv.text_in(&f.sb, r, 1, &format!("{}", c + 1), DARK_TEXT);
        }
        // trigger level marker at the right edge of the grid, its value next to it. Not with AC or LF reject trigger
        // coupling: the level then counts from the signal's mean, a mark at that level in volts would point nowhere
        // (the trigger card shows the level)
        let ts = self.m.trig_src;
        let lvl = self.trig_level_shown();
        let filtered = matches!(self.m.trig_coupling(), TrigCoup::Ac | TrigCoup::LfReject);
        if self.m.trig_ref.is_none()
            && self.m.ch[ts].on
            && self.shows(ts)
            && self.m.trig_line().is_none()
            && !filtered
            && self.trigger_absolute_level_visible(false)
        {
            let y = self.y_of_volts(ts, lvl).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
            let x1 = a.x + a.w - MARK_W;
            let r = Rect::new(x1 + 8, y - 8, 16, 16);
            cv.round(r, 3, ACCENT, 255);
            cv.triangle([(x1 + 9, y - 6), (x1 + 2, y), (x1 + 9, y + 6)], ACCENT);
            cv.text_in(&f.sb, r, 1, "T", DARK_TEXT);
            let s = si_short(lvl as f64, self.m.unit(ts));
            backed_text(cv, &f.sb, x1 - 4 - f.sb.width(&s), y + 5, &s, ACCENT);
            if self.touch.drag == Drag::Level || self.level_line_on() {
                cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, ACCENT);
            }
        }
        // second level: lower threshold, or level of source B / the clock
        let t = self.m.trig_type;
        let c2 = match t {
            TrigType::Slope | TrigType::Runt | TrigType::Window => Some((ts, "t")),
            TrigType::Delay => Some((self.m.trig_src2, "B")),
            TrigType::SetupHold => Some((self.m.trig_src2, "C")),
            _ => None,
        };
        if let Some((c, tag)) = c2 {
            let secondary = matches!(t, TrigType::Delay | TrigType::SetupHold);
            let source = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
            if source == WaveId::Ch(c)
                && self.m.ch[c].on
                && self.shows(c)
                && self.trigger_absolute_level_visible(secondary)
            {
                let y =
                    self.y_of_volts(c, self.m.trig_level2).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
                let x1 = a.x + a.w - MARK_W;
                let r = Rect::new(x1 + 8, y - 8, 16, 16);
                cv.round(r, 3, mix(BG, ACCENT, 170), 255);
                cv.triangle([(x1 + 9, y - 6), (x1 + 2, y), (x1 + 9, y + 6)], mix(BG, ACCENT, 170));
                cv.text_in(&f.sb, r, 1, tag, DARK_TEXT);
                let s = si_short(self.m.trig_level2 as f64, self.m.unit(ts));
                backed_text(cv, &f.sb, x1 - 4 - f.sb.width(&s), y + 5, &s, mix(BG, ACCENT, 200));
                if self.touch.drag == Drag::Level2 {
                    cv.hline(a.x, a.x + a.w - 1, y, ACCENT);
                }
            }
        }
        self.draw_reference_trigger_markers(cv);
        // Trigger time is absolute in an imported record.
        let tx = self.x_of_time(self.replay.trigger_time.unwrap_or(0.0) as f32);
        let ty = a.y + 9;
        if tx < a.x {
            cv.triangle([(a.x + 1, ty - 4), (a.x + 9, ty - 9), (a.x + 9, ty + 1)], ACCENT);
        } else if tx > a.x + a.w {
            let x = a.x + a.w - 1;
            cv.triangle([(x, ty - 4), (x - 8, ty - 9), (x - 8, ty + 1)], ACCENT);
        } else {
            cv.triangle([(tx - 6, ty - 8), (tx + 6, ty - 8), (tx, ty)], ACCENT);
        }
        // drag feedback
        let tag = |cv: &mut Canvas, x: i32, y: i32, s: &str, col: Rgb| {
            let r = Rect::new(x, y, f.sb.width(s) + 20, 22);
            cv.framed(r, 5, 1, LINE, 4, BAR);
            cv.text_in(&f.sb, r, 1, s, col);
        };
        match self.touch.drag {
            Drag::Offset(c) if self.shows(c) => {
                let y = self.y_of_volts(c, 0.0);
                cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, CH_COLORS[c]);
                tag(
                    cv,
                    a.x + 8,
                    y - 30,
                    &format!("Offset {}", si(self.m.ch[c].offset as f64, self.m.unit(c))),
                    CH_COLORS[c],
                );
            }
            Drag::Delay if self.plot.cur_win == self.touch.win.min(self.plot.wins.len().max(1) - 1) => {
                tag(cv, a.x + a.w / 2 - 60, a.y + 12, &format!("Delay {}", si(self.view().1 as f64, "s")), ACCENT)
            }
            Drag::Level
                if self.shows(ts)
                    && !filtered
                    && self.m.trig_ref.is_none()
                    && self.trigger_absolute_level_visible(false) =>
            {
                let y = self.y_of_volts(ts, self.m.trig_level);
                tag(
                    cv,
                    a.x + a.w - 130,
                    y - 30,
                    &format!("Level {}", si(self.m.trig_level as f64, self.m.unit(ts))),
                    ACCENT,
                );
            }
            _ => {}
        }
    }

    pub(super) fn draw_cursors(&self, cv: &mut Canvas) {
        let source = self.cursor_source();
        if !self.workspace_source_visible_here(source)
            && !(self.m.cursor == CursorMode::Track && self.workspace_source_visible_here(self.cursor_b_source()))
        {
            return;
        }
        let f = self.fonts();
        let a = self.area;
        cv.set_clip(a);
        let tagbox = |cv: &mut Canvas, x: i32, y: i32, s: &str| {
            let r = Rect::new(x, y, f.s.width(s) + 8, 14);
            cv.round(r, 2, CURSOR, 255);
            cv.text_in(&f.s, r, 1, s, DARK_TEXT);
        };
        match self.m.cursor {
            CursorMode::Off => {}
            CursorMode::Time | CursorMode::Track | CursorMode::Xy => {
                if self.m.cursor == CursorMode::Xy {
                    for (frac, name) in [(self.m.cur_c, "Y1"), (self.m.cur_d, "Y2")] {
                        let y = a.y + (frac * a.h as f32) as i32;
                        cv.dashed_hline(a.x, a.x + a.w - 1, y, 4, 3, CURSOR);
                        tagbox(cv, self.right_tag_x(a, y - 16, f.s.width(name) + 8), y - 16, name);
                    }
                }
                let (ta, tb) = self.cursor_times();
                for (frac, name, t) in [(self.m.cur_a, "X1", ta), (self.m.cur_b, "X2", tb)] {
                    // a cursor that the zoomed view does not show keeps its time and is not drawn
                    if !(0.0..=1.0).contains(&frac) {
                        continue;
                    }
                    let x = a.x + (frac * a.w as f32) as i32;
                    cv.dashed_vline(x, a.y, a.y + a.h - 1, 4, 3, CURSOR);
                    tagbox(cv, x + 2, a.y + a.h - 36, name);
                    // Each tracking mark follows its own native source and lane.
                    if self.m.cursor == CursorMode::Track {
                        let id = if name == "X1" { source } else { self.cursor_b_source() };
                        if self.workspace_source_visible_here(id) {
                            if let Some(v) = self.wave_at_source(id, t) {
                                let y = self.y_of_wave(id, v);
                                cv.dashed_hline(x - 10, x + 10, y, 2, 2, CURSOR);
                            }
                        }
                    }
                }
            }
            CursorMode::Volt => {
                for (frac, name) in [(self.m.cur_a, "Y1"), (self.m.cur_b, "Y2")] {
                    let y = a.y + (frac * a.h as f32) as i32;
                    cv.dashed_hline(a.x, a.x + a.w - 1, y, 4, 3, CURSOR);
                    tagbox(cv, self.right_tag_x(a, y - 16, f.s.width(name) + 8), y - 16, name);
                }
            }
        }
        cv.no_clip();
    }

    /// Left edge of a cursor tag at the right end of the plot: where it clears the corner buttons.
    pub(super) fn right_tag_x(&self, a: Rect, y: i32, w: i32) -> i32 {
        let mut x = a.x + a.w - 26;
        for r in self.corner_btns() {
            if x + w > r.x - 2 && x < r.right() + 2 && y < r.bottom() + 2 && y + 14 > r.y - 2 {
                x = r.x - 6 - w;
            }
        }
        x
    }

    /// Cursor read-outs: (label, value) pairs.
    pub(super) fn cursor_values(&self) -> Vec<(String, String)> {
        let (source, scale, offset, unit) = match self.cursor_source_context() {
            Some(context) => context,
            None if self.m.cursor == CursorMode::Track => {
                let source = self.cursor_source();
                (source, 1.0, 0.0, self.cursor_source_unit(source))
            }
            None => return Vec::new(),
        };
        let area = self.source_area(source).unwrap_or(self.area);
        match self.m.cursor {
            CursorMode::Off => Vec::new(),
            CursorMode::Xy => {
                let (ta, tb) = self.cursor_times();
                let v = |c: f32| self.zoom_divs_at(area, c) * scale - offset;
                let (va, vb) = (v(self.m.cur_c), v(self.m.cur_d));
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("X2".into(), si(tb, "s")),
                    ("Y1".into(), si(va as f64, unit)),
                    ("Y2".into(), si(vb as f64, unit)),
                    ("\u{394}X".into(), si(tb - ta, "s")),
                    ("\u{394}Y".into(), si((vb - va) as f64, unit)),
                ]
            }
            CursorMode::Track => {
                let (ta, tb) = self.cursor_times();
                let second = self.cursor_b_source();
                let (ya, yb) = (self.wave_at_source(source, ta), self.wave_at_source(second, tb));
                let (ua, ub) = (unit, self.cursor_source_unit(second));
                let txt = |v: Option<f32>, u: &str| v.map_or("----".to_string(), |v| si(v as f64, u));
                let dy = ya.zip(yb).filter(|_| ua == ub).map(|(a, b)| b - a);
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("Y1".into(), txt(ya, ua)),
                    ("X2".into(), si(tb, "s")),
                    ("Y2".into(), txt(yb, ub)),
                    ("\u{394}X".into(), si(tb - ta, "s")),
                    ("\u{394}Y".into(), txt(dy, if ua == ub { ua } else { "" })),
                ]
            }
            CursorMode::Time => {
                let (ta, tb) = self.cursor_times();
                let d = tb - ta;
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("X2".into(), si(tb, "s")),
                    ("\u{394}X".into(), si(d, "s")),
                    ("1/\u{394}X".into(), si(1.0 / d.abs(), "Hz")),
                ]
            }
            CursorMode::Volt => {
                let va = self.zoom_divs_at(area, self.m.cur_a) * scale - offset;
                let vb = self.zoom_divs_at(area, self.m.cur_b) * scale - offset;
                vec![
                    ("Y1".into(), si(va as f64, unit)),
                    ("Y2".into(), si(vb as f64, unit)),
                    ("\u{394}Y".into(), si((vb - va) as f64, unit)),
                ]
            }
        }
    }

    /// Status line inside the top of the plot: touch lock, history position and search count on the left; a source that
    /// is not the live hardware on the right.
    pub(super) fn draw_info(&self, cv: &mut Canvas) {
        if self.eye_open() && self.m.eye_place == EyePlace::Full && self.workspace.eye_float.is_none() {
            return;
        }
        let f = self.fonts();
        let g = self.grid;
        let y = g.y + 14;
        // only what makes the picture something else than a live measurement; the rates are on System info
        let record_caption = self.record_caption();
        let src = if self.record_mode() {
            record_caption.as_str()
        } else {
            match self.src.name() {
                "DEMO" => "Demo signals",
                "REPLAY" => "Replay",
                _ => "",
            }
        };
        if !src.is_empty() {
            backed_text(cv, &f.s, self.full_btn().0.x - 16 - f.s.width(src), y, src, DIM);
        }
        let mut x = g.x + 8;
        if self.m.touch_lock {
            x = backed_text(cv, &f.sb, x, y, "Touch locked \u{b7} tap Menu to unlock", AMBER) + 16;
        }
        let seg = self.seg.view.filter(|_| self.seg_hw_on()).and_then(|v| match (v.phase, self.seg.pos) {
            (crate::board::SegPhase::Recording, _) => Some(format!("Recording {}/{}", v.recorded, v.total)),
            (crate::board::SegPhase::Reading, _) => Some(format!("Reading {}/{}", v.read, v.recorded)),
            (crate::board::SegPhase::Done, Some(i)) => Some(self.seg_label(i)),
            _ => None,
        });
        if let Some(t) = seg {
            x = backed_text(cv, &f.sb, x, y, &t, BLUE) + 16;
        } else if let Some(i) = self.history.pos {
            x = backed_text(cv, &f.sb, x, y, &format!("History {}/{}", i + 1, self.history.recs.len()), BLUE) + 16;
        }
        if self.search_on {
            let events = self.search_events();
            let label = if self.m.trig_ref.is_some() && !self.reference_search_supported() {
                "Search: condition needs hardware channels".into()
            } else if self.m.trig_ref.is_some() && self.reference_search_busy() {
                format!("Search {} · full record · computing…", self.trigger_source_label())
            } else {
                format!(
                    "Search {}{}: {} events",
                    self.trigger_source_label(),
                    if self.m.trig_ref.is_some() { " · full record" } else { "" },
                    events.len()
                )
            };
            backed_text(cv, &f.sb, x, y, &label, AMBER);
        }
        let note = self.live_ref_note();
        if !note.is_empty() && !self.view_on() && !self.eye_open() {
            let y = g.y + g.h - 36;
            let fit = Rect::new(g.x + 8, y - 12, 52, 22);
            btn(cv, fit, BTN, ACCENT);
            cv.text_in(&f.s, fit, 1, "Fit", TEXT);
            let lab = Rect::new(fit.x + fit.w + 8, y - 12, (g.x + g.w - fit.x - fit.w - 16).max(1), 22);
            cv.set_clip(lab);
            let text = self.fit_ref_note(&note, lab.w);
            // over a trace of the last lane: backed, like the time labels
            cv.blend(Rect::new(lab.x - 3, lab.y + 2, f.s.width(&text) + 6, lab.h - 4), BG, 190);
            cv.text_in(&f.s, lab, 0, &text, LABEL);
            cv.no_clip();
        }
    }

    /// The "With live" note cut to `width`, naming how many references no longer fit ("+2").
    pub(super) fn fit_ref_note(&self, note: &str, width: i32) -> String {
        let f = self.fonts();
        if f.s.width(note) <= width {
            return note.to_string();
        }
        let Some((head, rest)) = note.split_once(" · ") else { return note.to_string() };
        let parts: Vec<&str> = rest.split("   ").collect();
        for keep in (0..parts.len()).rev() {
            let text = if keep == 0 {
                format!("{head} · +{}", parts.len())
            } else {
                format!("{head} · {}   +{}", parts[..keep].join("   "), parts.len() - keep)
            };
            if f.s.width(&text) <= width || keep == 0 {
                return text;
            }
        }
        note.to_string()
    }

    /// Search markers: an amber tick at the top of the plot for every event of the trigger condition.
    pub(super) fn draw_search(&self, cv: &mut Canvas) {
        if !self.search_on {
            return;
        }
        let a = self.area;
        let events = self.search_events();
        let (scale, delay) = self.view();
        let left = delay as f64 - scale as f64 * HDIV as f64 * 0.5;
        let per_pixel = scale as f64 * HDIV as f64 / a.w.max(1) as f64;
        let mut first = events.partition_point(|&time| time < left);
        // At most one marker per occupied pixel, without walking millions of
        // off-screen events or hiding their actual count in Search results.
        for column in 0..a.w {
            let next = events.partition_point(|&time| time < left + (column + 1) as f64 * per_pixel);
            if next > first {
                let x = a.x + column;
                cv.triangle([(x - 4, a.y + 1), (x + 4, a.y + 1), (x, a.y + 8)], AMBER);
                cv.dashed_vline(x, a.y + 9, a.y + a.h - 1, 2, 6, mix(BG, AMBER, 90));
            }
            first = next;
        }
    }
}
