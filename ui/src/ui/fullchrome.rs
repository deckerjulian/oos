// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio full screen: the bars are gone, but the status pill (Run / Stop) and an "Exit full screen" chip stay, in
//! the bottom left corner over the waveform.
use super::*;

impl Ui {
    /// The pill and the exit chip of full screen; None when the layout is not in full screen.
    pub(super) fn full_chrome_rects(&self) -> Option<(Rect, Rect)> {
        if self.m.layout != Layout::Studio || !self.full_on() { return None; }
        let y = self.scr.h - 40;
        let pill = Rect::new(8, y, 112, 32);
        Some((pill, Rect::new(pill.right() + 8, y, 156, 32)))
    }

    pub(super) fn draw_full_chrome(&self, cv: &mut Canvas) {
        let Some((pill, exit)) = self.full_chrome_rects() else { return };
        let f = self.fonts();
        let (txt, col) = self.run_caption();
        cv.round(pill, 6, mix(BAR, col, 80), 255);
        cv.round(pill.inset(1), 5, mix(BAR, col, 36), 255);
        let (gx, gy) = (pill.x + 16, pill.y + 16);
        if self.m.run == RunState::Run && !self.record_mode() {
            cv.fill(Rect::new(gx - 5, gy - 5, 3, 10), col);
            cv.fill(Rect::new(gx + 2, gy - 5, 3, 10), col);
        } else {
            cv.triangle([(gx - 4, gy - 6), (gx - 4, gy + 6), (gx + 6, gy)], col);
        }
        cv.text_in(&f.mb, Rect::new(pill.x + 32, pill.y, pill.w - 32, pill.h), 0, txt, col);
        btn(cv, exit, BTN, AMBER);
        cv.text_in(&f.mb, exit, 1, "Exit full screen", AMBER);
    }

    /// A tap on the pill or the exit chip. True when it was on one.
    pub(super) fn full_chrome_tap(&mut self, x: i32, y: i32) -> bool {
        let Some((pill, exit)) = self.full_chrome_rects() else { return false };
        if Self::hit40(exit).contains(x, y) { self.toggle_expand(); return true; }
        if Self::hit40(pill).contains(x, y) { self.studio_run_stop(); return true; }
        false
    }
}
