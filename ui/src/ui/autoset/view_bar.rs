// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The bar after an auto set-up: the views it offers (few or many periods, a rising or a falling edge), where its
//! buttons stand, what a tap on one does, and its drawing. A new view of the bar goes here.

use super::*;

/// What the bar after an auto set-up offers (drawn in the waveform window, `AUTO_BAR_S` s or until a tap elsewhere).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) enum AutoView {
    /// about three periods
    Few,
    /// about ten periods
    Many,
    /// the rising edge across about two divisions
    Rise,
    Fall,
}

const AUTO_VIEWS: [AutoView; 4] = [AutoView::Few, AutoView::Many, AutoView::Rise, AutoView::Fall];
/// The bar goes after this long without a tap on it.
const AUTO_BAR_S: f64 = 30.0;
const AB_W: i32 = 88;
/// Low enough for the free band above a signal the auto set-up placed (80 % of the height: 0.8 div at the top).
const AB_H: i32 = 34;
/// Wide enough that the trigger position mark at the top centre stays clear.
const AB_GAP: i32 = 16;
const AB_TOP: i32 = 6;

impl Ui {
    /// The buttons of the bar at the very top of the waveform window, the views centred, the close button (None) right of
    /// them.
    pub(in crate::ui) fn auto_bar_rects(&self) -> Vec<(Rect, Option<AutoView>)> {
        let g = self.grid;
        let n = AUTO_VIEWS.len() as i32;
        let w = n * AB_W + (n - 1) * AB_GAP;
        let x0 = g.x + (g.w - w) / 2;
        let y = g.y + AB_TOP;
        let mut v: Vec<(Rect, Option<AutoView>)> = AUTO_VIEWS
            .iter()
            .enumerate()
            .map(|(i, &v)| (Rect::new(x0 + i as i32 * (AB_W + AB_GAP), y, AB_W, AB_H), Some(v)))
            .collect();
        v.push((Rect::new(x0 + w + AB_GAP, y, AB_H, AB_H), None));
        v
    }

    pub(in crate::ui) fn auto_bar_on(&self) -> bool {
        self.auto_bar.is_some_and(|t| self.now - t < AUTO_BAR_S) && !self.view_on()
    }

    /// A tap while the bar shows: a button sets its view and keeps the bar; anywhere else closes it (and the tap goes on
    /// to what is under it).
    pub(in crate::ui) fn auto_bar_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.auto_bar_on() {
            return false;
        }
        if let Some((_, v)) = self.auto_bar_rects().into_iter().find(|(r, _)| r.contains(x, y)) {
            match v {
                Some(v) => {
                    self.auto_view(v);
                    self.auto_bar = Some(self.now);
                }
                None => self.auto_bar = None,
            }
            return true;
        }
        self.auto_bar = None;
        false
    }

    /// The time base (and slope) for view `v` of the trigger channel, from its measurements now.
    pub(in crate::ui) fn auto_view(&mut self, v: AutoView) {
        let c = self.m.trig_src;
        let ok = |x: f64| x.is_finite() && x > 0.0;
        // The bar navigates the fitted ADC capture. A previous edge view may
        // make the Screen measurement gate too short to contain even one full
        // period; that must not disable the next edge/period navigation action.
        let native = self.last[0].as_ref().map(|record| {
            if let Some(raw) = record.raw.as_ref().filter(|raw| !raw.frame.envelope && !raw.frame.ch[c].is_empty()) {
                crate::wave::autoset_summary(&raw.frame.ch[c], raw.frame.dt)
            } else {
                crate::wave::autoset_summary(&record.ch[c], record.dt)
            }
        });
        let period = native
            .as_ref()
            .map(|summary| summary.period)
            .filter(|&p| ok(p))
            .or_else(|| Some(self.meas(c, Meas::Period)).filter(|&p| ok(p)));
        let Some(p) = period else { return self.toast("No period on the trigger channel") };
        let (t, slope) = match v {
            AutoView::Few => (p * 3.0 / HDIV as f64, None),
            AutoView::Many => (p * 10.0 / HDIV as f64, None),
            // the 10-90 % time over about two divisions; a period / 20 when there is no edge time
            AutoView::Rise => (
                native
                    .as_ref()
                    .map(|summary| summary.rise)
                    .filter(|&x| ok(x))
                    .or_else(|| Some(self.meas(c, Meas::Rise)).filter(|&x| ok(x)))
                    .unwrap_or(p / 10.0)
                    / 2.0,
                Some(Slope::Rise),
            ),
            AutoView::Fall => (
                native
                    .as_ref()
                    .map(|summary| summary.fall)
                    .filter(|&x| ok(x))
                    .or_else(|| Some(self.meas(c, Meas::Fall)).filter(|&x| ok(x)))
                    .unwrap_or(p / 10.0)
                    / 2.0,
                Some(Slope::Fall),
            ),
        };
        self.m.tscale = tscale_for(t as f32);
        self.m.tfine = 1.0;
        self.m.delay = 0.0;
        if let Some(s) = slope {
            self.m.slope = s;
        }
        self.dirty_map = true;
        self.settings_changed();
        self.mark_changed();
    }

    pub(in crate::ui) fn draw_auto_bar(&self, cv: &mut Canvas) {
        if !self.auto_bar_on() {
            return;
        }
        for (r, v) in self.auto_bar_rects() {
            cv.round(r, 6, PANEL_BG, 225);
            cv.round_outline(r, 6, BTN_BORDER);
            match v {
                Some(v) => auto_icon(cv, r, v),
                None => {
                    // a cross, 2 px
                    let (cx, cy, d) = (r.x + r.w / 2, r.y + r.h / 2, 6);
                    for k in 0..2 {
                        cv.line(cx - d + k, cy - d, cx + d + k, cy + d, TEXT);
                        cv.line(cx + d + k, cy - d, cx - d + k, cy + d, TEXT);
                    }
                }
            }
        }
    }
}

/// The symbol of a view, drawn in lines 2 px wide: square waves for the periods, a ramp between two levels for an edge.
fn auto_icon(cv: &mut Canvas, r: Rect, v: AutoView) {
    let (x0, x1) = (r.x + 16, r.x + r.w - 16);
    let (hi, lo) = (r.y + 9, r.y + r.h - 9);
    let seg = |cv: &mut Canvas, a: (i32, i32), b: (i32, i32)| {
        cv.line(a.0, a.1, b.0, b.1, TEXT);
        if a.0 == b.0 {
            cv.line(a.0 + 1, a.1, b.0 + 1, b.1, TEXT);
        } else {
            cv.line(a.0, a.1 + 1, b.0, b.1 + 1, TEXT);
        }
    };
    match v {
        AutoView::Few | AutoView::Many => {
            // half periods across the width, starting low
            let halves = if v == AutoView::Few { 5 } else { 10 };
            let w = (x1 - x0) as f32 / halves as f32;
            let mut y = lo;
            let mut x = x0;
            for k in 1..=halves {
                let nx = x0 + (k as f32 * w).round() as i32;
                seg(cv, (x, y), (nx, y));
                if k < halves {
                    let ny = if y == lo { hi } else { lo };
                    seg(cv, (nx, y), (nx, ny));
                    y = ny;
                }
                x = nx;
            }
        }
        AutoView::Rise | AutoView::Fall => {
            let (a, b) = if v == AutoView::Rise { (lo, hi) } else { (hi, lo) };
            let (m0, m1) = (x0 + (x1 - x0) * 3 / 10, x0 + (x1 - x0) * 7 / 10);
            seg(cv, (x0, a), (m0, a));
            seg(cv, (m0, a), (m1, b));
            seg(cv, (m1, b), (x1, b));
            // the trigger level through the middle of the edge, dashed
            let my = (hi + lo) / 2;
            let mut x = x0;
            while x < x1 {
                cv.hline(x, (x + 2).min(x1), my, DIM);
                x += 6;
            }
        }
    }
}
