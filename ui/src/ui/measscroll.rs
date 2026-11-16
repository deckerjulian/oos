// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: how the measurements panel scrolls. There is no pager: a finger dragged up or down moves the list, and a flick
//! keeps it moving (it slows down by itself); the mouse wheel moves it too. The table moves by pixels, the list and
//! the cards by whole rows (the position is kept in pixels all the same, so a slow flick still moves them).
//!
//! The gesture on a row: a press that moves is a scroll when it goes mostly up or down, and a swipe (the row slides
//! left over a red Remove) when it goes mostly left past `SWIPE_START` first; once it has gone vertical it stays a scroll.
//! A press held without moving picks a list row up (reorder). A badge or a pill dragged down off the screen is removed
//! as before: that is the one swipe down.
use super::studio::{PanelGeo, StudioView};
use super::*;

/// Flicks slower than this (px/s) do not coast; the coasting stops below `STOP_V`.
const FLICK_MIN: f32 = 120.0;
const STOP_V: f32 = 25.0;
/// How fast the coasting slows: the speed is multiplied by exp(-DECAY * dt).
const DECAY: f64 = 4.0;
/// The finger must have moved within this long before it lifted for the list to coast.
const FLICK_WINDOW: f64 = 0.12;

impl Ui {
    /// Pixels one scroll step is high: a row of the table, of the list, or a row of two cards.
    fn panel_unit(&self) -> i32 { self.studio_panel_row().max(1) }

    fn panel_cols(&self) -> usize { if self.panel_is_cards() { 2 } else { 1 } }

    /// Where the list is scrolled to, in pixels from the top.
    pub(super) fn panel_scroll_px(&self) -> i32 {
        (self.studio.panel.first / self.panel_cols()) as i32 * self.panel_unit() + self.studio.panel.sub
    }

    /// The farthest the list scrolls, in pixels, for `total` rows in `g`.
    pub(super) fn panel_scroll_max(&self, total: usize, g: &PanelGeo) -> i32 {
        let unit = self.panel_unit();
        if self.studio_detailed() { return (total as i32 * unit - g.list.h).max(0); }
        let cols = self.panel_cols();
        let first_max = total.saturating_sub(g.page);
        (first_max.div_ceil(cols)) as i32 * unit
    }

    /// Scrolls to `px`, kept in range. True when that is an end of the list.
    pub(super) fn panel_scroll_set(&mut self, px: i32, total: usize, g: &PanelGeo) -> bool {
        let unit = self.panel_unit();
        let max = self.panel_scroll_max(total, g);
        let px = px.clamp(0, max);
        self.studio.panel.first = (px / unit) as usize * self.panel_cols();
        self.studio.panel.sub = px % unit;
        px == 0 || px == max
    }

    /// The list and its geometry when the docked panel (not the SIM list or the table of the whole body) is up.
    fn panel_scroll_ctx(&self) -> Option<(PanelGeo, usize)> {
        if self.m.layout != Layout::Studio || self.studio.view == StudioView::Sim { return None; }
        let g = self.studio_panel_geo()?;
        let rows = self.meas_rows();
        let total = self.studio_listed(&rows).len();
        Some((g, total))
    }

    /// The finger went down on the list: remember where the scroll was; a coasting list stops.
    pub(super) fn panel_scroll_begin(&mut self) {
        let st = &mut self.studio.panel;
        st.sub0 = st.sub;
        st.vel = 0.0;
        st.last = None;
    }

    /// The finger moved from `y0` (where it went down, with the list at row `first0`) to `y`. False: not the panel's list.
    pub(super) fn panel_scroll_drag(&mut self, y0: i32, first0: usize, y: i32) -> bool {
        let Some((g, total)) = self.panel_scroll_ctx() else { return false };
        let pos0 = (first0 / self.panel_cols()) as i32 * self.panel_unit() + self.studio.panel.sub0;
        self.panel_scroll_set(pos0 + (y0 - y), total, &g);
        // the speed of the finger, smoothed
        let now = self.now;
        if let Some((t, ly)) = self.studio.panel.last {
            let dt = now - t;
            if dt > 0.0005 {
                let v = (ly - y) as f32 / dt as f32;
                self.studio.panel.vel = self.studio.panel.vel * 0.5 + v * 0.5;
            }
        }
        self.studio.panel.last = Some((now, y));
        true
    }

    /// The finger lifted after a scroll: a quick flick coasts, a finger that stopped before it lifted does not.
    pub(super) fn panel_scroll_end(&mut self) {
        let st = &mut self.studio.panel;
        let fresh = st.last.is_some_and(|(t, _)| self.now - t <= FLICK_WINDOW);
        if !fresh || st.vel.abs() < FLICK_MIN { st.vel = 0.0; }
        st.last = None;
    }

    /// Every frame: the coasting of the list after a flick.
    pub(super) fn panel_fling_step(&mut self, dt: f64) {
        if self.studio.panel.vel == 0.0 { return; }
        // the finger is still on the list: it measures the speed, nothing coasts yet
        if self.studio.drag.is_some() { return; }
        if self.touch.down { self.studio.panel.vel = 0.0; return; }
        let Some((g, total)) = self.panel_scroll_ctx() else { self.studio.panel.vel = 0.0; return };
        let v = self.studio.panel.vel;
        let at = self.panel_scroll_px();
        // at least a pixel a frame while it is moving, so a slow list still creeps
        let mut step = (v as f64 * dt).round() as i32;
        if step == 0 { step = v.signum() as i32; }
        let end = self.panel_scroll_set(at + step, total, &g);
        let v = v * (-DECAY * dt).exp() as f32;
        self.studio.panel.vel = if end || v.abs() < STOP_V { 0.0 } else { v };
    }

    /// The mouse wheel over the panel's list. True when it was taken.
    pub(super) fn panel_wheel(&mut self, x: i32, y: i32, steps: i32) -> bool {
        let Some((g, total)) = self.panel_scroll_ctx() else { return false };
        if !g.list.contains(x, y) { return false; }
        let at = self.panel_scroll_px();
        self.studio.panel.vel = 0.0;
        // a step is a row and a half
        self.panel_scroll_set(at - steps * self.panel_unit() * 3 / 2, total, &g);
        true
    }

    /// The thin mark at the right edge of the list that shows where it is, in pixels.
    pub(super) fn draw_scroll_mark(cv: &mut Canvas, list: Rect, pos: i32, content: i32) {
        if content <= list.h { return; }
        let x = list.right() - 3;
        let h = ((list.h as i64 * list.h as i64 / content as i64) as i32).max(16);
        let max = (content - list.h).max(1);
        let y = list.y + ((list.h - h) as i64 * pos.clamp(0, max) as i64 / max as i64) as i32;
        cv.fill(Rect::new(x, y, 2, h), DIM);
    }
}
