// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Where things sit on the screen: the plot rectangle and what the strip, the results card and the eye pane take from
//! it, the windows of the views (which channel, reference and math trace is in which), the full-screen and zoom
//! buttons, the zoom window with its box, readout and wheel steps, and `Ui::relayout`, which places it all after a
//! change. A new area of the plot goes into `Ui::relayout` and the `*_area` methods here.

use super::*;

/// The zoom box (ADR 0062): the tool armed to draw boxes (persistent, independent of the corner zoom-display toggle),
/// the screen corners of the box being drawn (`path`) and the waveform rectangle and time base it started in (`map`:
/// rect, s/div, delay).
#[derive(Default)]
pub(super) struct ZoomBox {
    pub arm: bool,
    pub path: Vec<(i32, i32)>,
    pub map: Option<(Rect, f32, f32)>,
}

impl ZoomBox {
    /// Disarmed, and no box drawn.
    pub fn cancel(&mut self) {
        (self.arm, self.map) = (false, None);
        self.path.clear();
    }
}

impl Ui {
    /// Measurement results in a panel on the right instead of the strip under the grid.
    pub(super) fn results_panel(&self) -> bool {
        self.stats_on && !(self.m.meas.is_empty() && self.m.meas2.is_empty() && self.m.cat.is_empty())
    }

    /// Channels with measurements, in the order of their first measurement (one strip row each).
    pub(super) fn meas_groups(&self) -> Vec<WaveId> {
        let mut v = Vec::new();
        for id in self
            .m
            .meas
            .iter()
            .map(|e| e.1)
            .chain(self.m.meas2.iter().map(|e| e.1))
            .chain(self.m.cat.iter().map(|e| meascatalog::primary_source(*e)))
        {
            if !v.contains(&id) {
                v.push(id);
            }
        }
        v
    }

    /// The measurements of a default set-up: one full strip row.
    pub(super) fn default_meas() -> Vec<(Meas, WaveId)> {
        [Meas::Vpp, Meas::Freq, Meas::Vavg, Meas::Vrms, Meas::Rise, Meas::PDuty].map(|k| (k, WaveId::Ch(0))).to_vec()
    }

    /// Strip rows of the measurements of channel `c`: six to a row.
    pub(super) fn meas_strip_rows(&self, id: WaveId) -> i32 {
        let n = self.m.meas.iter().filter(|e| e.1 == id).count()
            + self.m.meas2.iter().filter(|e| e.1 == id).count()
            + self.m.cat.iter().filter(|e| meascatalog::primary_source(**e) == id).count();
        n.div_ceil(tools::STRIP_PER_ROW) as i32
    }

    pub(super) fn strip_rows(&self) -> i32 {
        let dvm = (self.m.dvm_on || self.m.cnt_on) as i32;
        if self.results_pane_on() {
            return dvm;
        }
        let meas =
            if self.results_panel() { 0 } else { self.meas_groups().iter().map(|&c| self.meas_strip_rows(c)).sum() };
        self.visible_measurement_rows(meas) + (self.m.cursor != CursorMode::Off) as i32 + dvm
    }

    /// The plot: 10 x 8 divisions between the top bar and the cards; it gets shorter for strip rows (the results
    /// panel is an overlay and takes no room). With the eye open the trace keeps the left part, or the top.
    fn plot_rect(&self) -> Rect {
        // the logic analyser lanes have their own strip under the graticule: the analog picture shrinks with it
        let avail = self.plot_base() - self.la_strip_h() - self.pane_h();
        let top = self.plot_top();
        let (plot_x, plot_w) = self.plot_span();
        if self.eye_open() && self.eye_side() && self.workspace.eye_float.is_none() {
            let wave_w = (plot_w * 58 / 100).clamp(360, plot_w - 300);
            let h = (avail / 8 * 8).max(160);
            return Rect::new(if self.workspace.eye_left { plot_x + plot_w - wave_w } else { plot_x }, top, wave_w, h);
        }
        let h = ((avail - self.eye_below()) / 8 * 8).max(160);
        Rect::new(plot_x, top, plot_w, h)
    }

    /// Top bar, waveform area and bottom bar of the screen.
    pub fn frame(&self) -> Frame {
        let mut f = Frame::of(self.scr);
        if !self.m.bar_compact {
            return f;
        }
        // the compact bottom bar (PR #8, Display > Bottom bar = Compact): the body takes what the cards would have had
        let grow = f.bottom.h - BOTTOM_H;
        f.body.h += grow;
        f.bottom = Rect::new(f.bottom.x, f.bottom.y + grow, f.bottom.w, BOTTOM_H);
        f.menu_btn = Rect::new(f.bottom.x + 4, f.bottom.y + 3, 46, 30);
        let x = f.menu_btn.right() + 6;
        f.cards = Rect::new(x, f.bottom.y + 3, (f.bottom.right() - METRICS.edge - x).max(0), 30);
        f
    }

    /// The eye is showing next to the trace (it does not replace the waveform).
    pub(super) fn eye_open(&self) -> bool {
        self.m.view == AView::Eye && !self.xy
    }

    pub(super) fn eye_side(&self) -> bool {
        self.m.eye_place == EyePlace::Side || (self.m.eye_place == EyePlace::Under && self.eye_below() == 0)
    }

    /// Height taken under the trace when the eye is stacked there.
    pub(super) fn eye_below(&self) -> i32 {
        if self.workspace.eye_float.is_some() || !(self.eye_open() && self.m.eye_place == EyePlace::Under) {
            return 0;
        }
        let avail = self.plot_base() - self.la_strip_h() - self.pane_h();
        let room = avail - 160;
        if room < 140 {
            return 0;
        }
        (avail * 42 / 100).clamp(180, 260).min(room)
    }

    /// The eye's rectangle: to the right of the trace, or under it.
    pub(super) fn eye_pane_rect(&self) -> Rect {
        if let Some(rect) = self.workspace.eye_float {
            return rect;
        }
        let wave = self.plot_rect();
        if self.m.eye_place == EyePlace::Full {
            return wave;
        }
        if self.eye_side() {
            if self.workspace.eye_left {
                return Rect::new(self.plot_span().0, wave.y, (wave.x - self.plot_span().0 - 4).max(1), wave.h);
            }
            let x = wave.x + wave.w + 4;
            Rect::new(x, wave.y, (self.plot_span().0 + self.plot_span().1 - x).max(1), wave.h)
        } else {
            Rect::new(wave.x, wave.y + wave.h + 2, wave.w, (self.eye_below() - 4).max(1))
        }
    }

    /// Where the eye page itself is drawn: under the placement bar, on the whole plot or in the pane.
    pub(super) fn eye_page_rect(&self) -> Rect {
        if !self.eye_open() {
            return self.area;
        }
        let a = self.eye_pane_rect();
        const BAR: i32 = workspace::HEADER;
        Rect::new(a.x, a.y + BAR, a.w, (a.h - BAR).max(1))
    }

    /// Height for the graticule and the LA strip together (above a strip that sits under the plot).
    pub(super) fn plot_base(&self) -> i32 {
        let under = if self.m.strip_dock == StripDock::Under { self.strip_rows_h() } else { 0 };
        self.plot_bottom() - 2 - self.plot_top() - under
    }

    /// Full density drawn on the panel. Not while the external display shows the waveform (the panel stays the
    /// controls) unless the expand button asked for it, and not with the touch lock, whose way out is the Menu button.
    pub(super) fn full_on(&self) -> bool {
        self.full && !self.m.touch_lock
    }

    pub(super) fn plot_top(&self) -> i32 {
        let y = if self.full_on() { 0 } else { self.frame().body.y };
        if self.m.strip_dock == StripDock::Above {
            y + self.strip_rows_h()
        } else {
            y
        }
    }

    /// Where the measurement rows end: the cards, or the bottom of the screen.
    pub(super) fn plot_bottom(&self) -> i32 {
        if self.full_on() {
            self.scr.h
        } else {
            self.frame().bottom.y
        }
    }

    /// Waveform windows in use: one while zoom, XY or an analysis view has the plot.
    pub(super) fn n_wins(&self) -> usize {
        if self.stacked() {
            return self.trace_sources().len().max(1);
        }
        if self.m.zoom || self.view_on() || self.eye_open() {
            1
        } else {
            self.m.win_layout.count()
        }
    }

    /// The windows in the plot `g`, all of the same size (they share the phosphor size), in reading order.
    fn split_wins(&self, g: Rect) -> Vec<Rect> {
        const GAP: i32 = 6;
        if self.stacked() {
            let n = self.n_wins() as i32;
            let gap = if n > 8 { 2 } else { 4 };
            let h = ((g.h - (n - 1) * gap) / n).max(1);
            return (0..n).map(|row| Rect::new(g.x, g.y + row * (h + gap), g.w, h)).collect();
        }
        let (cols, rows) = match (self.n_wins(), self.m.win_layout) {
            (1, _) => return vec![g],
            (_, WinLayout::Cols) => (2, 1),
            (_, WinLayout::Rows) => (1, 2),
            _ => (2, 2),
        };
        let w = (g.w - (cols - 1) * GAP) / cols;
        // a whole number of pixels per division keeps the graticule even
        let h = ((g.h - (rows - 1) * GAP) / rows / 8 * 8).max(64);
        let mut v = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                v.push(Rect::new(g.x + c * (w + GAP), g.y + r * (h + GAP), w, h));
            }
        }
        v
    }

    /// Window of channel `c` (a window number beyond the layout goes to the last).
    fn win_of(&self, c: usize) -> usize {
        if self.stacked() {
            return self
                .trace_sources()
                .iter()
                .position(|source| *source == workspace::Trace::Channel(c))
                .unwrap_or(usize::MAX);
        }
        self.m.ch_win[c].min(self.wins.len().max(1) - 1)
    }

    /// Window of reference `i` in a split: its Windows row; in Stacked its own row.
    pub(super) fn ref_win_of(&self, i: usize) -> usize {
        if self.stacked() {
            return self
                .trace_sources()
                .iter()
                .position(|source| *source == workspace::Trace::Reference(i))
                .unwrap_or(usize::MAX);
        }
        self.m.ref_win[i].min(self.wins.len().max(1) - 1)
    }

    pub(super) fn math_win_of(&self, k: usize) -> usize {
        if self.stacked() {
            return self
                .trace_sources()
                .iter()
                .position(|source| *source == workspace::Trace::Math(k))
                .unwrap_or(usize::MAX);
        }
        self.m.math_win[k].min(self.wins.len().max(1) - 1)
    }

    /// Whether channel `c` belongs to the window being drawn.
    pub(super) fn shows(&self, c: usize) -> bool {
        self.win_of(c) == self.cur_win
    }

    /// The rectangle channel `c` is drawn in: its window, or the main waveform area (zoom).
    pub(super) fn ch_area(&self, c: usize) -> Rect {
        if self.wins.len() > 1 {
            self.wins.get(self.win_of(c)).copied().unwrap_or(self.area)
        } else {
            self.area
        }
    }

    /// Window of the LA lanes.
    pub(super) fn la_win_of(&self) -> usize {
        self.m.la_win.min(self.wins.len().max(1) - 1)
    }

    /// Area whose time axis the LA lanes and the decoders on them use: the window of the LA, or the main area.
    pub(super) fn lane_area(&self) -> Rect {
        if self.wins.len() > 1 {
            self.wins[self.la_win_of()]
        } else {
            self.area
        }
    }

    /// Whether window `w` has an analog channel or a math channel in it.
    pub(super) fn win_has_analog(&self, w: usize) -> bool {
        (0..NCH).any(|c| self.m.ch[c].on && self.win_of(c) == w)
            || (0..NMATH).any(|k| self.m.math_ch(k).op != MathOp::Off && self.math_win_of(k) == w)
    }

    /// Window of decoder `k`'s band in the plot, or None when it has a row under the LA lanes.
    pub(super) fn dec_band_win(&self, k: usize) -> Option<usize> {
        let m = self.m.dec_view(k);
        if m.dec_on_la() {
            return if self.la_dec_count() > 0 { None } else { Some(0) };
        }
        if let Some(WaveId::Ref(i)) = decode_wave(m.dec_src) {
            return self
                .source_area(WaveId::Ref(i))
                .and_then(|area| self.wins.iter().position(|r| *r == area))
                .or(Some(0));
        }
        Some(if self.wins.len() > 1 { self.win_of(m.dec_src) } else { 0 })
    }

    /// Area for cards over the plot (tables, results): the whole plot when it is split.
    pub(super) fn over_area(&self) -> Rect {
        if self.wins.len() > 1 {
            self.grid
        } else {
            self.area
        }
    }

    /// Spreads the channels that are on over the windows of a new layout, in channel order (math channels after
    /// them), so that a split shows something in every window at once.
    pub(super) fn spread_wins(&mut self) {
        let n = self.m.win_layout.count();
        let mut i = 0;
        for c in 0..NCH {
            self.m.ch_win[c] = if self.m.ch[c].on {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        for k in 0..NMATH {
            self.m.math_win[k] = if self.m.math_ch(k).op != MathOp::Off {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        // shown references after them, so a loaded R1 gets a window of its own as well
        for r in 0..NREF {
            self.m.ref_win[r] = if self.refs[r].as_ref().is_some_and(|x| x.on) {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        // the LA after them: in a window of its own while there is one
        self.m.la_win = if self.m.la_on() { i % n } else { 0 };
    }

    /// The expand button at the top right of the plot: (drawn, touch area). It is a tap target only; it does not move.
    pub(super) fn full_btn(&self) -> (Rect, Rect) {
        let g = self.grid;
        // left of the view button while an analysis view has the plot
        let right = if self.view_on() { self.view_btn().x - 6 } else { g.x + g.w - 4 };
        let r = Rect::new(right - 36, g.y + 3, 36, 28);
        (r, Rect::new(r.x - 8, r.y - 3, r.w + 16, 44))
    }

    pub(super) fn draw_full_btn(&self, cv: &mut Canvas) {
        let (r, _) = self.full_btn();
        let on = self.full_on();
        cv.round(r, 5, if on { mix(PANEL_BG, AMBER, 40) } else { PANEL_BG }, 230);
        cv.round_outline(r, 5, if on { AMBER } else { mix(BTN_BORDER, AMBER, 90) });
        // four corner brackets: at the corners pointing in to enlarge, near the middle pointing out to go back
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let l = 7;
        let col = if on { AMBER } else { TEXT };
        for (sx, sy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            // corner of the bracket, and the way its arms go (x, y)
            // pointing out while Full is selected, including when the panel keeps its bars for the external display
            let back = self.m.density == Density::Full && !self.m.touch_lock;
            let (px, py, ax, ay) =
                if back { (cx + sx * 4, cy + sy * 3, sx, sy) } else { (cx + sx * 12, cy + sy * 9, -sx, -sy) };
            let (hx0, hx1) = (px.min(px + ax * (l - 1)), px.max(px + ax * (l - 1)));
            let (vy0, vy1) = (py.min(py + ay * (l - 1)), py.max(py + ay * (l - 1)));
            // 2 px thick, the second pixel on the side of the arms
            cv.fill(Rect::new(hx0, py.min(py + ay), hx1 - hx0 + 1, 2), col);
            cv.fill(Rect::new(px.min(px + ax), vy0, 2, vy1 - vy0 + 1), col);
        }
    }

    /// Corner magnifier toggles the zoom display. Draw-a-box is a separate tool.
    pub(super) fn zoom_btn(&self) -> (Rect, Rect) {
        Self::below_btn(self.full_btn())
    }

    /// The Windows button under the magnifier opens the Windows menu: (drawn, touch area). While zoom is on, the
    /// overview is too low for it, and it moves to the top of the zoomed view, under the zoom bar.
    pub(super) fn win_btn(&self) -> (Rect, Rect) {
        let (r, hit) = Self::below_btn(self.zoom_btn());
        let top = if self.m.zoom && !self.view_on() { Self::split_zoom(self.grid).1.y + 3 } else { r.y };
        let dy = top.max(r.y) - r.y;
        (Rect::new(r.x, r.y + dy, r.w, r.h), Rect::new(hit.x, hit.y + dy, hit.w, hit.h))
    }

    /// A corner button under `above`, so the two buttons do not share a tap.
    fn below_btn((above, hit): (Rect, Rect)) -> (Rect, Rect) {
        let r = Rect::new(above.x, hit.y + hit.h + 2, above.w, above.h);
        (r, Rect::new(r.x - 5, r.y, r.w + 10, 40))
    }

    /// The magnifier and the Windows button belong to the scope picture: an analyser page has its own controls under
    /// the expand button.
    pub(super) fn plot_btns_shown(&self) -> bool {
        !(self.eye_open() && self.m.eye_place == EyePlace::Full && self.workspace.eye_float.is_none())
            && !self.view_on()
    }

    /// The corner buttons as drawn, which floating cards and tags keep clear of.
    pub(super) fn corner_btns(&self) -> [Rect; 3] {
        [self.full_btn().0, self.zoom_btn().0, self.win_btn().0]
    }

    pub(super) fn draw_zoom_btn(&self, cv: &mut Canvas) {
        let (r, _) = self.zoom_btn();
        let armed = self.m.zoom;
        cv.round(r, 5, PANEL_BG, 220);
        cv.round_outline(r, 5, if armed { AMBER } else { BTN_BORDER });
        let col = if armed { AMBER } else { TEXT };
        cv.line(r.x + 5, r.y + 4, r.x + r.w - 5, r.y + 4, col);
        cv.line(r.x + r.w - 5, r.y + 4, r.x + r.w - 5, r.y + r.h - 4, col);
        cv.line(r.x + 5, r.y + 4, r.x + r.w - 5, r.y + r.h - 4, col);
        cv.round_outline(Rect::new(r.x + r.w - 16, r.y + 6, 8, 8), 4, col);
        cv.line(r.x + r.w - 15, r.y + 13, r.x + r.w - 21, r.y + 19, col);
    }

    pub(super) fn draw_win_btn(&self, cv: &mut Canvas) {
        let (r, _) = self.win_btn();
        let open = self.menu_open && self.menu_root() == MenuId::Windows;
        cv.round(r, 5, PANEL_BG, 220);
        cv.round_outline(r, 5, if open { AMBER } else { BTN_BORDER });
        let col = if open { AMBER } else { TEXT };
        // four small windows, two by two
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        for (sx, sy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
            cv.round_outline(Rect::new(cx + sx * 11 + 1, cy + sy * 9 + 1, 9, 7), 2, col);
        }
    }

    /// The box while a zoom-area drag is in progress.
    pub(super) fn draw_zoom_box(&self, cv: &mut Canvas) {
        if self.touch.drag != Drag::ZoomBox || self.zoom_box.path.len() != 2 {
            return;
        }
        let ((x0, y0), (x1, y1)) = (self.zoom_box.path[0], self.zoom_box.path[1]);
        let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
        if w < 2 && h < 2 {
            return;
        }
        cv.set_clip(self.zoom_box.map.map_or(self.grid, |map| map.0));
        cv.rect(Rect::new(x0.min(x1), y0.min(y1), w.max(1), h.max(1)), ACCENT);
        cv.no_clip();
    }

    /// Time base under a point of the waveform. While zoom is on, the overview keeps the main time base.
    pub(super) fn zoom_drag_map(&self, x: i32, y: i32) -> Option<(Rect, f32, f32)> {
        if self.view_on() {
            return None;
        }
        if self.m.zoom {
            let (o, z) = Self::split_zoom(self.grid);
            if o.contains(x, y) {
                return Some((o, self.m.time_per_div(), self.m.delay));
            }
            if z.contains(x, y) {
                let lane = self.wins.iter().copied().find(|lane| lane.contains(x, y)).unwrap_or(z);
                return Some((lane, tscales()[self.m.zscale], self.m.zdelay));
            }
            return None;
        }
        if self.area.w > 0 && self.area.contains(x, y) {
            Some((self.area, self.m.time_per_div(), self.m.delay))
        } else {
            None
        }
    }

    /// Draw boxes repeatedly until a plain tap or menu cancels this tool.
    fn arm_zoom_area(&mut self) {
        if self.view_on() {
            self.toast("Not on this view");
            return;
        }
        self.zoom_box.arm = !self.zoom_box.arm;
        if self.zoom_box.arm {
            self.zone_arm = None;
            self.toast("Drag a box to zoom");
        }
    }

    pub(super) fn zoom_vertical_transform(&self, area: Rect) -> (f32, f32) {
        let detail = Self::split_zoom(self.grid).1;
        if self.m.zoom && !self.view_on() && area.y >= detail.y && area.y < detail.y + detail.h {
            (self.zoom_vfactor, self.zoom_vcenter)
        } else {
            (1.0, 0.0)
        }
    }

    pub(super) fn zoom_y_of_divs(&self, area: Rect, divs: f32) -> f32 {
        let (factor, center) = self.zoom_vertical_transform(area);
        area.y as f32 + area.h as f32 * 0.5 - (divs - center) * factor * area.h as f32 / VDIV
    }

    pub(super) fn zoom_divs_at(&self, area: Rect, fraction: f32) -> f32 {
        let (factor, center) = self.zoom_vertical_transform(area);
        center + (0.5 - fraction) * VDIV / factor
    }

    pub(super) fn zoom_display_model(&self) -> Model {
        let mut display = self.m.clone();
        for channel in &mut display.ch {
            let vpd = channel.volts_per_div();
            channel.vfine /= self.zoom_vfactor;
            channel.offset -= self.zoom_vcenter * vpd;
        }
        display
    }

    pub(super) fn zoom_vertical_step(&mut self, direction: i32, area: Rect, fraction: f32) {
        let anchor = self.zoom_divs_at(area, fraction);
        self.zoom_vfactor = (self.zoom_vfactor * 2.0f32.powi(direction)).clamp(1.0, 1024.0);
        self.zoom_vcenter = anchor - (0.5 - fraction) * VDIV / self.zoom_vfactor;
        self.dirty_map = true;
    }

    pub(super) fn main_horizontal_step(&mut self, direction: i32, fraction: f32) {
        let divisions = (fraction as f64 - 0.5) * HDIV as f64;
        let anchor = self.m.delay as f64 + divisions * self.m.time_per_div() as f64;
        self.adjust(Item::HScale, direction);
        self.m.delay = (anchor - divisions * self.m.time_per_div() as f64) as f32;
        self.clamp_zoom();
    }

    pub(super) fn zoom_horizontal_step(&mut self, direction: i32, _area: Rect, fraction: f32) {
        let divisions = (fraction as f64 - 0.5) * HDIV as f64;
        let anchor = self.m.zdelay as f64 + divisions * tscales()[self.m.zscale] as f64;
        self.m.zscale = (self.m.zscale as i64 + direction as i64).clamp(0, (tscales().len() - 1) as i64) as usize;
        self.m.zscale = self.m.zscale.min(self.m.tscale);
        self.m.zdelay = (anchor - divisions * tscales()[self.m.zscale] as f64) as f32;
        self.clamp_zoom();
        self.settings_changed();
    }

    /// Plain wheel pans time; Ctrl-wheel zooms time around the pointer.
    pub fn pointer_wheel(&mut self, x: i32, y: i32, steps: i32, zoom_time: bool) -> bool {
        self.pointer_wheel_motion(x, y, steps, zoom_time, false)
    }

    pub(super) fn pointer_wheel_motion(&mut self, x: i32, y: i32, steps: i32, zoom_time: bool, vertical: bool) -> bool {
        if self.eye_open() && self.eye_pane_rect().contains(x, y) {
            return false;
        }
        if self.load_slot_ask
            || self.menu_open
            || self.dialog.is_some()
            || self.power_dlg
            || self.m.touch_lock
            || self.view_on()
            || !self.grid.contains(x, y)
        {
            return false;
        }
        let lane = self.wins.iter().position(|area| area.contains(x, y));
        // the lane of the zoomed window the wheel is over
        let detail_lane = lane.filter(|_| self.m.zoom);
        let detail = detail_lane.is_some();
        if self.m.zoom && lane.is_none() && !Self::split_zoom(self.grid).0.contains(x, y) {
            return false;
        }
        let source = if self.stacked() { lane.and_then(|lane| self.trace_sources().get(lane).copied()) } else { None };
        match source {
            Some(workspace::Trace::Channel(c)) => self.select_workspace_source(WaveId::Ch(c)),
            Some(workspace::Trace::Reference(i)) => self.select_workspace_source(WaveId::Ref(i)),
            Some(workspace::Trace::Math(i)) => self.m.math_select(i),
            None => {}
        }
        let steps = steps.clamp(-32, 32);
        if zoom_time {
            let area = if let Some(l) = detail_lane {
                self.wins[l]
            } else if self.m.zoom {
                Self::split_zoom(self.grid).0
            } else {
                lane.map(|index| self.wins[index]).unwrap_or(self.area)
            };
            let fraction = ((x - area.x) as f32 / area.w.max(1) as f32).clamp(0.0, 1.0);
            for _ in 0..steps.unsigned_abs() {
                if detail {
                    self.zoom_horizontal_step(-steps.signum(), area, fraction);
                } else {
                    self.main_horizontal_step(-steps.signum(), fraction);
                }
            }
        } else if vertical {
            if let Some(l) = detail_lane {
                let area = self.wins[l];
                let fraction = (y - area.y) as f32 / area.h.max(1) as f32;
                for _ in 0..steps.unsigned_abs() {
                    self.zoom_vertical_step(steps.signum(), area, fraction);
                }
            } else {
                match source {
                    Some(workspace::Trace::Math(_)) => self.adjust(Item::MathScale, -steps),
                    _ => match self.selected_workspace_source() {
                        Some(WaveId::Ref(i)) => {
                            self.ref_sel = i;
                            self.adjust(Item::RefVpd, -steps);
                        }
                        Some(WaveId::Ch(c)) => self.adjust(Item::ChScale(c), -steps),
                        None => return false,
                    },
                }
            }
        } else if detail {
            self.m.zdelay -= steps as f32 * tscales()[self.m.zscale] * 0.5;
            self.clamp_zoom();
        } else {
            let shift = steps as f32 * self.m.time_per_div() * 0.5;
            self.m.delay -= shift;
            if self.m.zoom {
                self.m.zdelay -= shift;
                self.clamp_zoom();
            }
        }
        self.dirty_map = true;
        true
    }

    /// Compact controls in the existing zoom row; no additional panel space.
    pub(super) fn zoom_controls(&self) -> [(Rect, &'static str); 5] {
        let row = self.zoom_readout();
        std::array::from_fn(|i| {
            (
                Rect::new(row.x + row.w - 480 + i as i32 * 62, row.y + 2, 58, row.h - 4),
                ["H-", "H+", "V-", "V+", "Box"][i],
            )
        })
    }

    /// Zoom: overview on top, a readout row (with Close), zoomed view below.
    pub(super) fn split_zoom(g: Rect) -> (Rect, Rect) {
        let top = 96;
        let gap = 32;
        (Rect::new(g.x, g.y, g.w, top), Rect::new(g.x, g.y + top + gap, g.w, (g.h - top - gap) / 8 * 8))
    }

    /// The strip between the overview and the zoomed view.
    pub(super) fn zoom_readout(&self) -> Rect {
        let (o, z) = Self::split_zoom(self.grid);
        Rect::new(o.x, o.y + o.h, o.w, (z.y - o.y - o.h).max(1))
    }

    /// Close removes the detail display while retaining explicit overview/timebase changes.
    pub(super) fn zoom_close(&self) -> Rect {
        let r = self.zoom_readout();
        Rect::new(r.x + r.w - 78, r.y + 4, 70, (r.h - 8).max(22))
    }

    /// The main waveform area (the zoomed view when zoom is on, the first window when the plot is split).
    pub(super) fn wave_area(&self) -> Rect {
        self.wins.first().copied().unwrap_or(self.grid)
    }

    /// Recomputes the layout and reallocates the phosphor maps when the plot size changed.
    pub(super) fn relayout(&mut self) {
        let g = self.plot_rect();
        // Stacked is a display format for the detail view. The shared overview
        // remains intact when horizontal zoom is enabled.
        let detail = if self.m.zoom { Self::split_zoom(g).1 } else { g };
        let wins = self.split_wins(detail);
        let w0 = wins[0];
        if self.view_pass {
            // the second picture (HDMI layout): rectangles of its own, the maps of the panel
            self.grid = g;
        } else if g != self.grid || self.layout_zoom != self.m.zoom || self.wins.first() != Some(&w0) {
            self.grid = g;
            self.layout_zoom = self.m.zoom;
            if self.m.zoom {
                let (o, _) = Self::split_zoom(g);
                self.ph = Phosphor::new(o.w as usize, o.h as usize);
                self.phz = Phosphor::new(w0.w as usize, w0.h as usize);
            } else {
                // one map per channel at the size of a window: each channel is drawn into its own window
                self.ph = Phosphor::new(w0.w as usize, w0.h as usize);
                self.phz = Phosphor::new(1, 1);
            }
            self.dirty_map = true;
        }
        self.wins = wins;
        self.cur_win = 0;
        self.area = self.wave_area();
    }

    /// Time base of the main waveform area: (s/div, delay).
    pub(super) fn view(&self) -> (f32, f32) {
        if self.m.zoom {
            (tscales()[self.m.zscale], self.m.zdelay)
        } else {
            (self.m.time_per_div(), self.m.delay)
        }
    }

    /// Keeps the zoom window inside the main window and not slower than the main time base.
    pub(super) fn clamp_zoom(&mut self) {
        if self.m.zscale > self.m.tscale {
            self.m.zscale = self.m.tscale;
        }
        let half_main = self.m.time_per_div() * HDIV / 2.0;
        let half_zoom = tscales()[self.m.zscale] * HDIV / 2.0;
        let lim = (half_main - half_zoom).max(0.0);
        self.m.zdelay = self.m.zdelay.clamp(self.m.delay - lim, self.m.delay + lim);
    }

    pub(super) fn set_zoom(&mut self, on: bool) {
        self.zoom_user_closed = !on;
        if on == self.m.zoom {
            self.relayout();
            return;
        }
        if on {
            self.zoom_saved = Some((self.m.tscale, self.m.tfine, self.m.delay));
            self.m.zscale = self.m.tscale.saturating_sub(3);
            self.m.zdelay = self.m.delay;
            self.zoom_vfactor = 1.0;
            self.zoom_vcenter = 0.0;
        } else {
            self.zoom_saved = None;
        }
        self.m.zoom = on;
        self.zoom_box.cancel();
        self.relayout();
    }

    pub(super) fn set_record_zoom(&mut self, on: bool) {
        if !self.zoom_user_closed {
            self.set_zoom(on);
        }
    }

    /// Detail scale, delay and display magnification controls in the existing zoom row.
    pub(super) fn draw_zoom_readout(&self, cv: &mut Canvas, tz: f32, dz: f32) {
        let f = self.fonts();
        let row = self.zoom_readout();
        let close = self.zoom_close();
        let lbl = format!(
            "Zoom \u{b7} {}/div \u{b7} {} \u{b7} V {:.2}x",
            si_short(tz as f64, "s"),
            si(dz as f64, "s"),
            self.zoom_vfactor
        );
        let reset = Rect::new(close.x - 76, close.y, 70, close.h);
        let controls = self.zoom_controls();
        let lab = Rect::new(row.x + 4, row.y, (controls[0].0.x - row.x - 10).max(1), row.h);
        cv.set_clip(lab);
        cv.text_in(&f.sb, lab, 0, &lbl, ACCENT);
        cv.no_clip();
        btn(cv, close, BTN, ACCENT);
        cv.text_in(&f.sb, close, 1, "Close", ACCENT);
        btn(cv, reset, BTN, BTN_BORDER);
        cv.text_in(&f.sb, reset, 1, "Reset", TEXT);
        for (rect, text) in controls {
            btn(cv, rect, if text == "Box" && self.zoom_box.arm { ACCENT } else { BTN }, BTN_BORDER);
            cv.text_in(&f.sb, rect, 1, text, if text == "Box" && self.zoom_box.arm { DARK_TEXT } else { TEXT });
        }
    }

    pub(super) fn zoom_close_tap(&mut self, x: i32, y: i32) -> bool {
        if self.m.zoom && !self.view_on() {
            for (rect, text) in self.zoom_controls() {
                if !rect.contains(x, y) {
                    continue;
                }
                let area = self.wave_area();
                match text {
                    "H-" => self.zoom_horizontal_step(1, area, 0.5),
                    "H+" => self.zoom_horizontal_step(-1, area, 0.5),
                    "V-" => self.zoom_vertical_step(-1, area, 0.5),
                    "V+" => self.zoom_vertical_step(1, area, 0.5),
                    _ => self.arm_zoom_area(),
                }
                return true;
            }
        }
        let close = self.zoom_close();
        if self.m.zoom && Rect::new(close.x - 76, close.y, 70, close.h).contains(x, y) {
            self.reset_zoom();
            return true;
        }
        if self.m.zoom && !self.xy && !self.view_on() && self.zoom_close().contains(x, y) {
            self.set_zoom(false);
            return true;
        }
        false
    }

    pub(super) fn reset_zoom(&mut self) {
        if !self.m.zoom {
            return;
        }
        self.m.zscale = self.m.tscale;
        self.m.zdelay = self.m.delay;
        self.zoom_vfactor = 1.0;
        self.zoom_vcenter = 0.0;
        self.zoom_box.arm = false;
        self.clamp_zoom();
        self.settings_changed();
    }

    pub(super) fn eye_tap(&mut self, x: i32, y: i32) -> bool {
        if self.eye_open() && self.eye_pane_close().contains(x, y) {
            self.view_exit();
            self.relayout();
            return true;
        }
        false
    }
}
