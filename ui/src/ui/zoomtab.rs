// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the pull-down zoom tab. With "Zoom controls: Pull-down tab" (the default) the zoom view has no toolbar row: a small
//! tab at its top edge ("Zoom 10x") opens the whole toolbar over the zoom view when it is tapped or pulled down, and it shuts on
//! a tap outside, on a push up of its label, or after a few seconds of no use. The other setting puts the same controls in
//! the title strip of the waveform (dock.rs).
use super::studio::chip;
use super::*;

const TAB_W: i32 = 132;
const TAB_H: i32 = 20;
const BAR_H: i32 = 30;
/// The toolbar shuts itself after this long without a touch on it.
const IDLE: f64 = 4.0;

impl Ui {
    /// The controls are in the pull-down tab (else in the title strip).
    pub(super) fn zoom_tab_mode(&self) -> bool { self.m.layout == Layout::Studio && self.m.studio_zoom_tab }

    /// Zoom is on, with the tab setting.
    fn zoom_tab_on(&self) -> bool { self.zoom_tab_mode() && self.studio_zoom_active() }

    pub(super) fn zoom_tab_open(&self) -> bool { self.studio.zoom_open.is_some() }

    /// The top edge of the zoom view.
    fn zoom_view_top(&self) -> Rect { self.split_zoom(self.grid).1 }

    /// The closed tab: a handle centred at the top of the zoom view.
    pub(super) fn zoom_tab_rect(&self) -> Rect {
        let z = self.zoom_view_top();
        Rect::new(z.x + z.w / 2 - TAB_W / 2, z.y, TAB_W, TAB_H)
    }

    /// The open toolbar: over the top of the zoom view, no room taken from it.
    pub(super) fn zoom_tab_bar_rect(&self) -> Rect {
        let z = self.zoom_view_top();
        Rect::new(z.x, z.y, z.w, BAR_H)
    }

    /// The rectangle the zoom controls use: the open toolbar, or the tab.
    pub(super) fn zoom_tab_region(&self) -> Rect {
        if self.zoom_tab_open() { self.zoom_tab_bar_rect() } else { self.zoom_tab_rect() }
    }

    pub(super) fn zoom_tab_set(&mut self, open: bool) {
        self.studio.zoom_open = if open { Some(self.now) } else { None };
        self.studio.zoom_pull = None;
        self.studio.zoom_menu = false;
    }

    /// The tab, or the open toolbar, is drawn over the zoom view.
    pub(super) fn draw_zoom_tab(&self, cv: &mut Canvas) {
        if !self.zoom_tab_on() { return; }
        if self.zoom_tab_open() {
            let r = self.zoom_tab_bar_rect();
            cv.fill(r, BAR);
            cv.hline(r.x, r.right() - 1, r.bottom() - 1, LINE);
            self.draw_studio_zoom_bar(cv);
            return;
        }
        let r = self.zoom_tab_rect();
        chip(cv, r, BAR, BTN_BORDER);
        let f = self.fonts();
        chevron_down(cv, r.x + 14, r.y + r.h / 2 - 2, DIM);
        let text = format!("Zoom {}", super::studio::zoom_text_pub(self.studio_time_zoom()));
        cv.text_in(&f.sb, Rect::new(r.x + 26, r.y, r.w - 30, r.h), 0, &text, TEXT);
    }

    /// The touch area of the closed tab (the usual 40 px target grows it down and sideways).
    fn zoom_tab_hit(&self) -> Rect {
        let r = self.zoom_tab_rect();
        Rect::new(r.x - 10, r.y, r.w + 20, 40)
    }

    /// A finger went down on the tab (it may pull it open) or on the label of the open toolbar (it may push it shut).
    pub(super) fn zoom_tab_down(&mut self, x: i32, y: i32) -> bool {
        if !self.zoom_tab_on() { return false; }
        if !self.zoom_tab_open() {
            if self.zoom_tab_hit().contains(x, y) { self.studio.zoom_pull = Some((y, true)); return true; }
        } else if self.studio_zoom_bar().is_some_and(|b| b.label.contains(x, y)) {
            self.studio.zoom_open = Some(self.now);
            self.studio.zoom_pull = Some((y, false));
            return true;
        }
        false
    }

    /// The finger moved: far enough down opens the tab, far enough up shuts the toolbar.
    pub(super) fn zoom_tab_move(&mut self, y: i32) {
        let Some((y0, opening)) = self.studio.zoom_pull else { return };
        if opening && y - y0 > 10 { self.zoom_tab_set(true); }
        else if !opening && y0 - y > 10 { self.zoom_tab_set(false); }
    }

    /// A tap: the tab opens, a tap outside the open toolbar shuts it (and goes on to what is there), one on it keeps it open.
    pub(super) fn zoom_tab_tap(&mut self, x: i32, y: i32) -> bool {
        self.studio.zoom_pull = None;
        if !self.zoom_tab_on() { return false; }
        if !self.zoom_tab_open() {
            if self.zoom_tab_hit().contains(x, y) { self.zoom_tab_set(true); return true; }
            return false;
        }
        if self.studio.zoom_menu { self.studio.zoom_open = Some(self.now); return false; }
        if self.zoom_tab_bar_rect().contains(x, y) {
            // the label is the grip: a tap on it folds the toolbar away
            if self.studio_zoom_bar().is_some_and(|b| b.label.contains(x, y)) { self.zoom_tab_set(false); return true; }
            self.studio.zoom_open = Some(self.now);
            return false;
        }
        self.zoom_tab_set(false);
        false
    }

    /// Every frame: the toolbar shuts after a few seconds of no use, and when zoom goes off.
    pub(super) fn zoom_tab_tick(&mut self) {
        let Some(t) = self.studio.zoom_open else { return };
        if !self.zoom_tab_on() || (!self.studio.zoom_menu && self.studio.zoom_pull.is_none() && self.now - t > IDLE) { self.zoom_tab_set(false); }
        else if self.studio.zoom_menu { self.studio.zoom_open = Some(self.now); }
    }
}
