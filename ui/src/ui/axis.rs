// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the axes are handles. A pinch, the wheel or a drag along the row of time labels under the waveform changes
//! the main time/div in 1-2-5 steps; the same on the volts labels at the left of a lane changes the V/div of the
//! channel that lane labels. Each strip is at least 40 px thick and lights up while it is touched. Elsewhere on the
//! waveform nothing changes: a pinch there does nothing, and Ctrl-wheel is what it was.
use super::*;
use super::studio::StudioDrag;

/// Thickness of the time strip and width of the volts strip: finger-sized.
const STRIP: i32 = 40;
const VOLTS_W: i32 = 44;
/// A drag changes the scale one step for this much travel.
const STEP_PX: i32 = 36;
/// How long the strip stays lit after a wheel turn.
const FLASH_S: f64 = 0.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Axis {
    Time,
    Volts(usize),
}

/// A finger (or two) on an axis strip.
#[derive(Clone, Copy, Debug)]
pub(super) struct AxisTouch {
    pub axis: Axis,
    /// where the first finger went down, and now
    pub x0: i32,
    pub y0: i32,
    pub x: i32,
    pub y: i32,
    /// the second finger of a pinch
    pub p1: Option<(i32, i32)>,
    /// the spread the pinch last stepped at
    pub ref_spread: i32,
}

#[derive(Default)]
pub(super) struct AxisState {
    pub touch: Option<AxisTouch>,
    /// a wheel turn lights the strip for a moment
    pub flash: Option<(Axis, f64)>,
}

impl Ui {
    /// The channel a lane labels on its volts axis.
    fn axis_lane_channel(&self, lane: usize) -> Option<usize> {
        if self.stacked() {
            return match self.trace_sources().get(lane)? { workspace::Trace::Channel(c) => Some(*c), _ => None };
        }
        let c = self.m.active;
        if c < n_ch() && self.m.ch[c].on { Some(c) } else { (0..n_ch()).find(|&i| self.m.ch[i].on) }
    }

    /// The time strip: the bottom of the lowest lane, where the time labels are.
    pub(super) fn axis_time_strip(&self) -> Option<Rect> {
        let l = *self.wins.last()?;
        Some(Rect::new(l.x, l.bottom() - STRIP, l.w, STRIP))
    }

    /// The volts strip of a lane, without the bottom strip and clear of the ground marker of its channel.
    pub(super) fn axis_volts_strip(&self, lane: usize) -> Option<Rect> {
        let l = *self.wins.get(lane)?;
        let last = lane + 1 == self.wins.len();
        Some(Rect::new(l.x, l.y, VOLTS_W, l.h - if last { STRIP } else { 0 }))
    }

    /// The strip under a point, when the gesture may start there.
    pub(super) fn axis_at(&self, x: i32, y: i32) -> Option<Axis> {
        if self.m.layout != Layout::Studio || self.full_on() || self.menu_open || self.dialog.is_some() || self.m.touch_lock || self.view_on() || self.wins.is_empty() { return None; }
        if let Some(t) = self.axis_time_strip().filter(|t| t.contains(x, y)) {
            // a time cursor within reach of the finger is the cursor's
            let near_cursor = matches!(self.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy)
                && [self.m.cur_a, self.m.cur_b].iter().any(|f| (t.x + (*f * t.w as f32) as i32 - x).abs() < 24);
            return (!near_cursor).then_some(Axis::Time);
        }
        let lane = self.wins.iter().position(|l| l.contains(x, y))?;
        let v = self.axis_volts_strip(lane)?;
        if !v.contains(x, y) { return None; }
        let c = self.axis_lane_channel(lane)?;
        // the ground marker keeps its own drag
        let my = self.y_of_volts(c, 0.0).clamp(v.y + (v.h / 2).min(9), v.y + v.h - (v.h / 2).min(9));
        if (y - my).abs() < 16 && x < v.x + MARK_W + 6 { return None; }
        Some(Axis::Volts(c))
    }

    fn axis_rect(&self, a: Axis) -> Option<Rect> {
        match a {
            Axis::Time => self.axis_time_strip(),
            Axis::Volts(c) => (0..self.wins.len()).find(|&l| self.axis_lane_channel(l) == Some(c)).and_then(|l| self.axis_volts_strip(l)),
        }
    }

    /// One step along an axis: negative zooms in (shorter time/div, smaller V/div).
    pub(super) fn axis_step(&mut self, a: Axis, dir: i32) {
        match a {
            Axis::Time => self.main_horizontal_step(dir, 0.5),
            Axis::Volts(c) => {
                self.select_workspace_source(WaveId::Ch(c));
                self.adjust(Item::ChScale(c), dir);
            }
        }
        self.dirty_map = true;
    }

    /// The wheel over a strip: its steps. True when a strip took it.
    pub(super) fn axis_wheel(&mut self, x: i32, y: i32, steps: i32) -> bool {
        let Some(a) = self.axis_at(x, y) else { return false };
        for _ in 0..steps.clamp(-32, 32).unsigned_abs() { self.axis_step(a, -steps.signum()); }
        self.studio.axis.flash = Some((a, self.now));
        true
    }

    // ------------------------------------------------------------------------------------------------ touch

    /// A finger went down. True when it is on a strip: the touch ends in a tap unless it moves.
    pub(super) fn axis_down(&mut self, x: i32, y: i32) -> bool {
        self.studio.axis.touch = None;
        let Some(axis) = self.axis_at(x, y) else { return false };
        self.studio.axis.touch = Some(AxisTouch { axis, x0: x, y0: y, x, y, p1: None, ref_spread: 0 });
        self.studio_eat(x, y)
    }

    /// The second finger: a pinch on the strip. True when it joined one.
    pub(super) fn axis_pinch_down(&mut self, x: i32, y: i32) -> bool {
        let Some(mut t) = self.studio.axis.touch else { return false };
        t.p1 = Some((x, y));
        t.ref_spread = Self::axis_spread(t.axis, (t.x, t.y), (x, y));
        self.studio.axis.touch = Some(t);
        // two fingers are not a tap, and not a drag of the one
        self.studio.drag = Some(StudioDrag::Eat { x0: -10_000, y0: -10_000 });
        true
    }

    fn axis_spread(a: Axis, p: (i32, i32), q: (i32, i32)) -> i32 {
        match a { Axis::Time => (q.0 - p.0).abs(), Axis::Volts(_) => (q.1 - p.1).abs() }
    }

    /// The second finger moved or lifted.
    pub(super) fn axis_pinch_event(&mut self, ph: Phase, x: i32, y: i32) {
        let Some(mut t) = self.studio.axis.touch else { return };
        match ph {
            Phase::Move => t.p1 = Some((x, y)),
            _ => { t.p1 = None; self.studio.axis.touch = Some(t); return; }
        }
        self.studio.axis.touch = Some(t);
        self.axis_pinch_steps();
    }

    /// Compares the spread with the last step: a quarter more is one step in, a quarter less one step out.
    fn axis_pinch_steps(&mut self) {
        let Some(mut t) = self.studio.axis.touch else { return };
        let Some(p1) = t.p1 else { return };
        let spread = Self::axis_spread(t.axis, (t.x, t.y), p1);
        if t.ref_spread > 30 {
            if spread * 100 > t.ref_spread * 125 { self.axis_step(t.axis, -1); t.ref_spread = spread; }
            else if spread * 100 < t.ref_spread * 80 { self.axis_step(t.axis, 1); t.ref_spread = spread; }
        } else if spread > 30 { t.ref_spread = spread; }
        self.studio.axis.touch = Some(t);
    }

    /// The first finger moved. True while a strip has it (a drag along the axis, or the first finger of a pinch).
    pub(super) fn axis_move(&mut self, x: i32, y: i32) -> bool {
        let Some(mut t) = self.studio.axis.touch else { return false };
        t.x = x;
        t.y = y;
        self.studio.axis.touch = Some(t);
        if t.p1.is_some() { self.axis_pinch_steps(); return true; }
        // a drag: right and up zoom in, left and down out; one step for every STEP_PX
        let (travel, along) = match t.axis { Axis::Time => (x - t.x0, true), Axis::Volts(_) => (t.y0 - y, false) };
        let _ = along;
        if travel.abs() >= STEP_PX {
            self.axis_step(t.axis, if travel > 0 { -1 } else { 1 });
            let mut t = t;
            t.x0 = x;
            t.y0 = y;
            self.studio.axis.touch = Some(t);
        }
        // only a clear move along the axis is the gesture; a small one is still a tap
        let moved = (x - t.x0).abs().max((y - t.y0).abs()) > 8;
        moved || travel.abs() >= STEP_PX || false
    }

    pub(super) fn axis_up(&mut self) { self.studio.axis.touch = None; }

    // ------------------------------------------------------------------------------------------------ drawing

    /// A touched strip is lit: subtly, so the gesture can be found.
    pub(super) fn draw_axis_highlight(&self, cv: &mut Canvas) {
        let lit = self.studio.axis.touch.map(|t| t.axis).or_else(|| self.studio.axis.flash.filter(|(_, at)| self.now - at < FLASH_S).map(|(a, _)| a));
        let Some(r) = lit.and_then(|a| self.axis_rect(a)) else { return };
        cv.blend(r, ACCENT, 28);
        cv.rect(r, mix(BG, ACCENT, 90));
    }
}
