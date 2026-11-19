// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Drawing the screen: `Ui::render` and its layers (under and over the traces, the GPU jobs), the views, the
//! graticule, the axis labels, the traces with their brightness grading and the persistence maps. The parts drawn over
//! the traces are in `render_derived.rs` (mask, histogram, math, decode) and `render_marks.rs` (references, markers,
//! cursors); a new layer of the screen goes into `Ui::render_layers`.

use super::*;

impl Ui {
    pub fn render(&mut self, cv: &mut Canvas) {
        self.render_layers(&mut Layers { under: cv, over: None, jobs: Vec::new() });
    }

    /// The screen into `l`: with one canvas everything is drawn on it; with two (System > Graphics = GPU) the
    /// background, graticule and masks go under the traces, everything else over them, and the traces become jobs
    /// for the GPU (`gpu.rs`). The order of the drawing is the same either way.
    pub fn render_layers(&mut self, l: &mut Layers) {
        self.relayout();
        if let Some(o) = l.over.as_deref_mut() {
            o.no_clip();
            o.clear(0);
        }
        if self.start_up_active() {
            self.draw_start_up(l.under);
            // the power dialog over it (the power key works during the start-up)
            self.draw_overlay(l.over());
            return;
        }
        l.under.no_clip();
        l.under.clear(BG);
        let g = self.grid;
        if self.m.zoom && !self.xy {
            let (o, _) = Self::split_zoom(g);
            self.draw_grid(l.under, o);
            self.traces(l, o, false, true);
            self.draw_refs_at(l.over(), o, (self.m.time_per_div(), self.m.delay), true);
            // zoom window in the overview: dim the rest, frame the window
            let (tz, dz) = self.view();
            let ppx = o.w as f32 / HDIV;
            let tdiv = self.m.time_per_div();
            // a window far off the overview (an extreme delay) is held a screen width beyond it: drawn the same (it is
            // clipped to the overview), and the pixel arithmetic below stays within i32
            let far = 2.0 * o.w as f32;
            let at = |t: f32| o.x + (o.w as f32 / 2.0 + (t - self.m.delay) / tdiv * ppx).clamp(-far, far) as i32;
            let (xa, xb) = (at(dz - tz * HDIV / 2.0), at(dz + tz * HDIV / 2.0));
            let cv = l.over();
            cv.set_clip(o);
            cv.blend(Rect::new(o.x, o.y, (xa - o.x).max(0), o.h), BG, 170);
            cv.blend(Rect::new(xb, o.y, (o.x + o.w - xb).max(0), o.h), BG, 170);
            cv.rect(Rect::new(xa, o.y, (xb - xa).max(2), o.h), ACCENT);
            for edge in [xa, xb] {
                cv.round(Rect::new(edge - 3, o.y + o.h / 2 - 12, 7, 24), 3, ACCENT, 255);
            }
            cv.no_clip();
            for &lane in &self.wins {
                self.draw_grid(l.under, lane);
            }
        } else {
            for &w in &self.wins {
                self.draw_grid(l.under, w);
            }
        }
        if self.view_on() {
            // the analysers and XY draw their own picture: under what goes over it
            if self.xy {
                self.draw_xy(l.under);
            } else {
                self.draw_analyzer(l.under);
            }
            self.draw_view_btn(l.over());
        } else {
            // Every detail lane draws its own sources. The shared zoom overview
            // was drawn above; detail maps use the same lane dimensions.
            for i in 0..self.wins.len() {
                self.cur_win = i;
                self.area = self.wins[i];
                if !self.m.zoom {
                    self.draw_mask(l.under);
                    self.traces(l, self.area, false, false);
                } else {
                    self.traces(l, self.area, true, false);
                }
                let cv = l.over();
                self.draw_hist(cv);
                self.draw_zones(cv);
                if self.wins.len() > 1 || i == 0 || self.stacked() {
                    if self.waves_on() {
                        self.draw_refs_at(cv, self.waves_plot(self.area), self.view(), false);
                    } else {
                        self.draw_refs(cv);
                    }
                }
                self.draw_math(cv);
                if i == self.la_win_of() {
                    self.draw_la(cv);
                }
                self.draw_decode(cv);
                if i == 0 {
                    self.draw_bus(cv);
                }
                self.draw_meas_labels(cv);
                self.draw_cursors(cv);
                self.draw_search(cv);
                self.draw_bus_event(cv);
                self.draw_markers(cv);
                self.draw_axes(cv);
                self.draw_mask_pill(cv);
                if i == 0 {
                    self.draw_waves_board(cv);
                }
            }
            self.cur_win = 0;
            self.area = self.wave_area();
        }
        let cv = l.over();
        if self.m.zoom && !self.xy && !self.view_on() {
            let (tz, dz) = self.view();
            self.draw_zoom_readout(cv, tz, dz);
        }
        if self.eye_open() {
            // eye_pane_rect maps Full to the plot while retaining floating panes.
            self.draw_eye_pane(cv);
        }
        self.draw_cal_banner(cv);
        self.draw_info(cv);
        self.draw_zoom_box(cv);
        self.draw_full_btn(cv);
        // the zoom magnifier belongs to the scope picture: an analyser page has its own controls under it
        if !(self.eye_open() && self.m.eye_place == EyePlace::Full && self.workspace.eye_float.is_none())
            && !self.view_on()
        {
            self.draw_zoom_btn(cv);
        }
        self.draw_auto_bar(cv);
        self.draw_strip(cv);
        // Full on the external display is the plot and the strip. The statistics card and a lower pane stay on the panel.
        if self.hdmi_snap_want && self.m.density == Density::Full {
            self.capture_hdmi(l);
        }
        let cv = l.over();
        if self.pane_h() > 0 {
            self.draw_pane(cv);
            self.draw_pane_close(cv);
        }
        self.draw_results(cv);
        self.draw_table(cv);
        // waveform on the external display: the band between the bars, before the bars and the menus go over it
        // (the GPU puts it together from the layers itself)
        if self.hdmi_snap_want && self.m.density != Density::Full {
            self.capture_hdmi(l);
        }
        let cv = l.over();
        if !self.full_on() {
            self.draw_top(cv);
            self.draw_bottom(cv);
        }
        self.draw_menu(cv);
        self.draw_meas_cfg(cv);
        self.draw_toast(cv);
        self.draw_overlay(cv);
        self.workspace_draw(cv);
        self.draw_pointer(cv);
        self.frames += 1;
    }

    /// The traces of a plot window: drawn now on one canvas, a job for the GPU with layers. `zoom`: the zoom
    /// window's maps.
    /// `all_sources`: every channel that is on (the zoom overview), else only those of the window being drawn.
    fn traces(&self, l: &mut Layers, area: Rect, zoom: bool, all_sources: bool) {
        let (ph, vmax, vmax_cg) =
            if zoom { (&self.phz, &self.vmaxz, self.vmaxz_cg) } else { (&self.ph, &self.vmax, self.vmax_cg) };
        if l.over.is_none() {
            return self.draw_traces(l.under, area, ph, vmax, vmax_cg, all_sources);
        }
        let show: [bool; NCH] = std::array::from_fn(|c| self.m.ch[c].on && (all_sources || self.shows(c)));
        if !show.iter().any(|&s| s) {
            return;
        }
        let graded = self.m.color_grade;
        let (mut alpha, mut colours, mut shift) = (Vec::new(), Vec::new(), [0u32; NCH]);
        if graded {
            let (lut, sh) = self.grade_lut(vmax_cg);
            colours = lut.to_vec();
            shift[0] = sh;
        } else {
            alpha = vec![0u8; NCH * 1024];
            for c in 0..NCH {
                let (lut, sh, least) = self.trace_lut(vmax[c]);
                for (i, a) in alpha[c * 1024..(c + 1) * 1024].iter_mut().enumerate() {
                    *a = lut[i].max(least) as u8;
                }
                shift[c] = sh;
            }
        }
        l.jobs.push(TraceJob { area, zoom, graded, show, alpha, colours, shift, col: CH_COLORS });
    }

    /// The screen once more, laid out for the size of `cv` (Display > HDMI = Layout): the same settings, menus and
    /// records, sharp at the monitor's resolution. The traces come from the panel's phosphor maps, stretched to the
    /// larger plot (`draw_traces`). The panel's layout is as it was afterwards.
    pub fn render_view(&mut self, cv: &mut Canvas) {
        self.render_view_layers(&mut Layers { under: cv, over: None, jobs: Vec::new() });
    }

    /// `render_view` into layers (System > Graphics = GPU): the traces become jobs at the monitor's size.
    pub fn render_view_layers(&mut self, l: &mut Layers) {
        // Display > Monitor view: another view than the panel's
        if self.monitor_view().is_some() {
            self.with_monitor_view(|u| u.render_view_layers_as_is(l));
            return;
        }
        self.render_view_layers_as_is(l);
    }

    /// `render_view_layers` of the view in place.
    fn render_view_layers_as_is(&mut self, l: &mut Layers) {
        let keep = (
            self.scr,
            self.grid,
            std::mem::take(&mut self.wins),
            self.area,
            self.cur_win,
            self.frames,
            self.hdmi_snap_want,
        );
        self.scr = Screen { w: l.under.w, h: l.under.h };
        (self.view_pass, self.hdmi_snap_want) = (true, false);
        self.render_layers(l);
        self.view_pass = false;
        (self.scr, self.grid, self.wins, self.area, self.cur_win, self.frames, self.hdmi_snap_want) = keep;
    }

    /// Graticule in the classic scope style: dotted division lines (Full), a solid frame (Full, Light) and solid
    /// centre axes, with ticks every fifth of a division on the axes (both sides) and on the frame (inwards).
    fn draw_grid(&self, cv: &mut Canvas, g: Rect) {
        let dx = g.w as f32 / HDIV;
        let dy = g.h as f32 / VDIV;
        let (x1, y1) = (g.x + g.w - 1, g.y + g.h - 1);
        let xs = |k: i32, per: f32| (g.x + (k as f32 * dx / per).round() as i32).min(x1);
        let ys = |k: i32, per: f32| (g.y + (k as f32 * dy / per).round() as i32).min(y1);
        let cx = g.x + (g.w as f32 / 2.0).round() as i32;
        let cy = g.y + (g.h as f32 / 2.0).round() as i32;
        // Display > Grid brightness: towards the background
        let k = self.m.grid_bright.clamp(10, 100) * 255 / 100;
        let (dot, line) = (mix(BG, GRID_DOT, k), mix(BG, GRID_FRAME, k));
        if self.m.grid == Grid::Full {
            // dots about every 5 px along each division line (a whole number of dots per division, so they line
            // up with the ticks)
            let (nx, ny) = ((dx / 5.0).round().max(2.0), (dy / 5.0).round().max(2.0));
            for i in 1..HDIV as i32 {
                let x = xs(i, 1.0);
                if x != cx {
                    for k in 0..=(VDIV * ny) as i32 {
                        cv.pixel(x, ys(k, ny), dot, 255);
                    }
                }
            }
            for j in 1..VDIV as i32 {
                let y = ys(j, 1.0);
                if y != cy {
                    for k in 0..=(HDIV * nx) as i32 {
                        cv.pixel(xs(k, nx), y, dot, 255);
                    }
                }
            }
        }
        if self.m.grid != Grid::None && self.m.grid_frame {
            // frame with inward ticks every fifth of a division
            cv.rect(Rect::new(g.x, g.y, g.w, g.h), line);
            for k in 1..(HDIV as i32 * 5) {
                let (x, len) = (xs(k, 5.0), if k % 5 == 0 { 8 } else { 4 });
                cv.vline(x, g.y, g.y + len, line);
                cv.vline(x, y1 - len, y1, line);
            }
            for k in 1..(VDIV as i32 * 5) {
                let (y, len) = (ys(k, 5.0), if k % 5 == 0 { 8 } else { 4 });
                cv.hline(g.x, g.x + len, y, line);
                cv.hline(x1 - len, x1, y, line);
            }
        }
        // centre axes with ticks on both sides every fifth of a division, longer at the divisions
        cv.vline(cx, g.y, y1, line);
        cv.hline(g.x, x1, cy, line);
        for k in 0..=(HDIV as i32 * 5) {
            let len = if k % 5 == 0 { 6 } else { 3 };
            cv.vline(xs(k, 5.0), cy - len, cy + len, line);
        }
        for k in 0..=(VDIV as i32 * 5) {
            let len = if k % 5 == 0 { 6 } else { 3 };
            cv.hline(cx - len, cx + len, ys(k, 5.0), line);
        }
    }

    /// The refs that are shown over the live channels, in slot order.
    pub(super) fn shown_refs(&self) -> Vec<usize> {
        (0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).collect()
    }

    /// (V/div, offset, unit, colour) of the ref that labels the volt axis when no channel is shown here: the selected
    /// ref, else the first one this window holds.
    pub(super) fn ref_axis_source(&self) -> Option<(f32, f32, &'static str, Rgb)> {
        let here: Vec<usize> =
            self.shown_refs().into_iter().filter(|&i| self.workspace_source_visible_here(WaveId::Ref(i))).collect();
        let i = here
            .iter()
            .copied()
            .find(|&i| self.selected_workspace_source() == Some(WaveId::Ref(i)))
            .or(here.first().copied())?;
        let r = self.refs[i].as_ref()?;
        Some((r.vpd, r.offset, "V", self.wave_color(WaveId::Ref(i))))
    }

    /// Axis labels inside the plot: volts of the active channel along the left edge (not on the top and bottom
    /// lines, where the status line and the time labels are), times along the bottom edge. A volt label is left out
    /// rather than drawn over another label, the status line, the time labels, the mask pill or the decode band; it
    /// keeps clear of the channel markers at the left edge.
    fn draw_axes(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let a = self.area;
        let c = if self.m.ch[self.m.active].on && self.shows(self.m.active) {
            Some(self.m.active)
        } else {
            (0..NCH).find(|&i| self.m.ch[i].on && self.shows(i))
        };
        // with no channel to label, the shown ref gives the axis (R1 in the lane or window that holds it)
        let axis = match c {
            Some(c) => {
                let ch = &self.m.ch[c];
                Some((ch.volts_per_div(), ch.offset, ch.unit.name(), CH_COLORS[c]))
            }
            None => self.ref_axis_source(),
        };
        if let Some((base_vpd, base_offset, unit, color)) = axis {
            let (factor, center) = self.zoom_vertical_transform(a);
            let vpd = base_vpd / factor;
            let offset = base_offset - center * base_vpd;
            let span = (VDIV / 2.0 * vpd).abs() + offset.abs();
            let mut taken: Vec<Rect> = vec![Rect::new(a.x, a.y, a.w, 20), Rect::new(a.x, a.y + a.h - 22, a.w, 22)];
            if self.m.mask_on && self.mask.is_some() && self.shows(self.m.mask_src) {
                taken.push(Rect::new(a.x, a.y + 22, a.w, 28));
            }
            let bands = (0..DECODERS)
                .filter(|&k| self.m.dec_is_on(k) && self.dec_band_win(k) == Some(self.cur_win))
                .count() as i32;
            if bands > 0 {
                taken.push(Rect::new(a.x, a.y + a.h - 50 - (bands - 1) * DEC_BAND, a.w, bands * DEC_BAND));
            }
            // the channel ground markers at the left edge: a label on their line is left out (as on the stock
            // screen, where the marker takes the place of the "0" label)
            for k in 0..NCH {
                if self.m.ch[k].on && self.shows(k) {
                    let y = self.y_of_volts(k, 0.0).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
                    taken.push(Rect::new(a.x, y - 9, MARK_W + 2, 18));
                }
            }
            // labels right at the left edge of the graticule, centred on their division line
            let x = a.x + 6;
            for j in 1..VDIV as i32 {
                let y = a.y + (j as f32 * a.h as f32 / VDIV).round() as i32;
                let v = (VDIV / 2.0 - j as f32) * vpd - offset;
                let s = axis_label(v, vpd, offset, span, unit);
                let r = Rect::new(x - 3, y + 4 - f.s.size, f.s.width(&s) + 6, f.s.size + 4);
                if taken.iter().any(|t| overlaps(*t, r)) {
                    continue;
                }
                taken.push(r);
                backed_text(cv, &f.s, x, y + 4, &s, scale(color, 210));
            }
        }
        if self.stacked() && self.cur_win + 1 < self.wins.len() {
            return;
        }
        let (tdiv, delay) = self.view();
        let span = (HDIV / 2.0 * tdiv).abs() + delay.abs();
        // a narrow window (side by side) labels every second division, the centre always
        let every = if a.w < 700 { 2 } else { 1 };
        let mut free = i32::MIN;
        for i in 0..=HDIV as i32 {
            if (i - HDIV as i32 / 2).rem_euclid(every) != 0 {
                continue;
            }
            let x = a.x + (i as f32 * a.w as f32 / HDIV).round() as i32;
            let t = delay + (i as f32 - HDIV / 2.0) * tdiv;
            let zero = t.abs() < tdiv * 1e-3;
            let s = if zero { "0 s".to_string() } else { axis_label(t, tdiv, delay, span, "s") };
            let w = f.s.width(&s);
            let tx = (x - w / 2).clamp(a.x + 6, a.x + a.w - 6 - w);
            if tx < free + 8 {
                continue;
            }
            free = tx + w;
            backed_text(cv, &f.s, tx, a.y + a.h - 6, &s, if zero { ACCENT } else { AXIS_TEXT });
        }
    }

    fn draw_traces(
        &self,
        cv: &mut Canvas,
        area: Rect,
        ph: &Phosphor,
        vmaxs: &[u32; NCH],
        vmax_cg: u32,
        all_sources: bool,
    ) {
        if self.m.color_grade {
            return self.draw_graded(cv, area, ph, vmax_cg, all_sources);
        }
        let (w, h) = (ph.w, ph.h);
        for c in (0..NCH).rev() {
            if !self.m.ch[c].on || (!all_sources && !self.shows(c)) {
                continue;
            }
            let col = CH_COLORS[c];
            let (cr, cg, cb) = ((col >> 16) & 255, (col >> 8) & 255, col & 255);
            let (lut, shift, least) = self.trace_lut(vmaxs[c]);
            let map = &ph.map[c];
            let put = |p: &mut u32, v: u16| {
                if v == 0 {
                    return;
                }
                let a = lut[((v as usize) >> shift).min(1023)].max(least);
                // additive with saturation, but channels on top of each other mix towards the newer one
                let d = *p;
                let r = (((d >> 16) & 255) * (255 - a) / 255 + cr * a / 255).min(255);
                let g = (((d >> 8) & 255) * (255 - a) / 255 + cg * a / 255).min(255);
                let b = ((d & 255) * (255 - a) / 255 + cb * a / 255).min(255);
                *p = rgb(r, g, b);
            };
            match Self::stretch(area, w, h) {
                None => {
                    for y in 0..h {
                        let row = &map[y * w..(y + 1) * w];
                        let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                        for (x, &v) in row.iter().enumerate() {
                            put(&mut cv.px[o + x], v);
                        }
                    }
                }
                Some((xs, ys)) => {
                    for (y, &sy) in ys.iter().enumerate() {
                        let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                        for (x, &sx) in xs.iter().enumerate() {
                            put(&mut cv.px[o + x], map[sy * w + sx]);
                        }
                    }
                }
            }
        }
    }

    /// One part of `step` into `times.parts` (`:SYSTem:STATe?`).
    pub(super) fn prof_add(&mut self, i: usize, d: std::time::Duration) {
        self.times.parts[i] += 0.05 * (d.as_secs_f32() * 1e3 - self.times.parts[i]);
    }

    /// Brightness of a trace pixel by its count (`draw_traces`): floor + sqrt(hits / brightest), the intensity
    /// setting raises floor and gain. The count shifted right by the second value indexes the table; a pixel that
    /// was hit at all gets at least the third.
    /// The counts the picture is graded against (`picture_ref`), per channel and for colour grade; whether the main and
    /// the zoom map are near saturation.
    pub(super) fn grade_refs(&mut self) -> (bool, bool) {
        let n = self.pic_frames;
        let (mut full, mut fullz) = (false, false);
        for c in 0..NCH {
            if self.m.ch[c].on {
                let mx = self.ph.map[c].iter().copied().max().unwrap_or(0) as u32;
                self.vmax[c] = picture_ref(self.vmax[c], &mut self.per_frame[c], mx, n);
                full |= mx > 0xc000;
                if self.m.zoom {
                    let mz = self.phz.map[c].iter().copied().max().unwrap_or(0) as u32;
                    self.vmaxz[c] = picture_ref(self.vmaxz[c], &mut self.per_framez[c], mz, n);
                    fullz |= mz > 0xc000;
                }
            }
        }
        if self.m.color_grade {
            let on: [bool; NCH] = std::array::from_fn(|c| self.m.ch[c].on);
            self.vmax_cg = picture_ref(self.vmax_cg, &mut self.per_frame_cg[0], self.ph.merged_max(on), n);
            if self.m.zoom {
                self.vmaxz_cg = picture_ref(self.vmaxz_cg, &mut self.per_frame_cg[1], self.phz.merged_max(on), n);
            }
        }
        (full, fullz)
    }

    pub(super) fn trace_lut(&self, vmax: u32) -> ([u32; 1024], u32, u32) {
        let inten = self.m.intensity as f32 / 100.0;
        let floor = 30.0 + 140.0 * inten;
        let gain = 0.35 + 1.3 * inten;
        let vmax = vmax.max(1);
        let mut shift = 0;
        while (vmax >> shift) > 1023 {
            shift += 1;
        }
        let top = ((vmax >> shift).max(1)) as f32;
        let mut lut = [0u32; 1024];
        for (i, l) in lut.iter_mut().enumerate() {
            let v = (i as f32 + 0.5) / top;
            *l = if i == 0 && shift == 0 {
                0
            } else {
                (floor + (255.0 - floor) * (v * gain).min(1.0).sqrt()).min(255.0) as u32
            };
        }
        (lut, shift, floor as u32 / 2)
    }

    /// Colour of a colour-graded pixel by the summed count (`draw_graded`), and the shift of the index.
    fn grade_lut(&self, vmax: u32) -> ([u32; 1024], u32) {
        let gain = 0.35 + 1.3 * self.m.intensity as f32 / 100.0;
        let vmax = vmax.max(1);
        let mut shift = 0;
        while (vmax >> shift) > 1023 {
            shift += 1;
        }
        let top = (vmax >> shift).max(1) as f32;
        let pal = heat_palette();
        let mut lut = [0u32; 1024];
        for (i, l) in lut.iter_mut().enumerate() {
            let v = ((i as f32 + 0.5) / top * gain).min(1.0).sqrt();
            *l = pal[((v * 255.0) as usize).min(255)];
        }
        (lut, shift)
    }

    /// A plot of another size than its phosphor map (the HDMI layout): for each of its columns and rows the cell of
    /// the map it shows (the nearest). None: the same size, one pixel per cell.
    fn stretch(area: Rect, w: usize, h: usize) -> Option<(Vec<usize>, Vec<usize>)> {
        let (aw, ah) = (area.w.max(0) as usize, area.h.max(0) as usize);
        if (aw, ah) == (w, h) || w == 0 || h == 0 {
            return None;
        }
        Some(((0..aw).map(|x| x * w / aw).collect(), (0..ah).map(|y| y * h / ah).collect()))
    }

    /// Colour-graded persistence: the hit maps of all enabled channels are added up and every pixel is coloured by its
    /// density relative to the densest pixel (blue = rare .. red = frequent). Per frame only a 1024-entry LUT is
    /// built; per pixel it is an add per channel and one table look-up.
    pub(super) fn draw_graded(&self, cv: &mut Canvas, area: Rect, ph: &Phosphor, vmax: u32, all_sources: bool) {
        let maps: Vec<&[u16]> = (0..NCH)
            .filter(|&c| self.m.ch[c].on && (all_sources || self.shows(c)))
            .map(|c| ph.map[c].as_slice())
            .collect();
        if maps.is_empty() {
            return;
        }
        let (lut, shift) = self.grade_lut(vmax);
        let w = ph.w;
        if let Some((xs, ys)) = Self::stretch(area, w, ph.h) {
            for (y, &sy) in ys.iter().enumerate() {
                let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                for (x, &sx) in xs.iter().enumerate() {
                    let s: u32 = maps.iter().map(|m| m[sy * w + sx] as u32).sum();
                    if s != 0 {
                        cv.px[o + x] = lut[((s >> shift) as usize).min(1023)];
                    }
                }
            }
            return;
        }
        for y in 0..ph.h {
            let o = ((area.y + y as i32) * cv.w + area.x) as usize;
            let row = y * w;
            for (x, d) in cv.px[o..o + w].iter_mut().enumerate() {
                let s: u32 = maps.iter().map(|m| m[row + x] as u32).sum();
                if s != 0 {
                    *d = lut[((s >> shift) as usize).min(1023)];
                }
            }
        }
    }
}

/// The count a trace pixel is graded against (full brightness), from the brightest pixel `mx` of the picture. It
/// follows a higher `mx` at once and a lower one slowly (`prev`, no flicker). Minimum persistence (`frames` > 0, the
/// frames in the picture): the picture holds as many frames as arrived since the last screen refresh, in batches, so
/// its counts rise and fall with that number. Graded against the per-frame maximum (`per_frame`, followed the same
/// way) times `frames`, equal frames give equal brightness whatever their number. Without this a refresh with one
/// frame after one with several came out dim: the trace pumped at the batch rate (docs/fork/trigger-lockin.md).
fn picture_ref(prev: u32, per_frame: &mut u32, mx: u32, frames: u32) -> u32 {
    let follow = |v: u32, mx: u32| if mx >= v { mx } else { (v * 7 + mx) / 8 }.max(1);
    if frames == 0 {
        return follow(prev, mx);
    }
    *per_frame = follow(*per_frame, mx.div_ceil(frames));
    per_frame.saturating_mul(frames).max(1)
}
