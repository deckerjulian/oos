// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//! Measure > Labels on trace: what a measurement measures, on the trace: level lines for levels, arrows for spans.

use super::{si, Canvas, Rect, Ui, CH_COLORS, REF_COLORS};
use crate::model::WaveId;
use crate::gfx::Rgb;
use oos_dsp::measure::{Analysis, Meas};

/// Labels at most, the rest stays in the measurement strip: more only hides the trace.
const MAX_LABELS: usize = 6;
const LABEL_H: i32 = 17;

/// Where a label goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Mark {
    /// a level in V
    Level(f64),
    /// between two levels
    Vertical(f64, f64),
    /// between two times from the start of the measured record
    Span(f64, f64),
}

/// First index of `v` above `after`.
fn next_after(v: &[f64], after: f64) -> Option<f64> {
    v.iter().copied().find(|&t| t > after)
}

/// The mark of measurement `k`, starting from the first time on the screen `t0`; `None` = nothing to draw for it.
pub(super) fn mark(k: Meas, a: &Analysis, t0: f64) -> Option<Mark> {
    let first_rise = next_after(&a.rises, t0);
    let first_fall = next_after(&a.falls, t0);
    let m = match k {
        Meas::Vtop => Mark::Level(a.top),
        Meas::Vbase => Mark::Level(a.base),
        Meas::Vmax => Mark::Level(a.vmax),
        Meas::Vmin => Mark::Level(a.vmin),
        Meas::Vavg => Mark::Level(a.avg),
        Meas::Vmid => Mark::Level((a.top + a.base) / 2.0),
        Meas::Vpp => Mark::Vertical(a.vmin, a.vmax),
        Meas::Vamp => Mark::Vertical(a.base, a.top),
        Meas::Period | Meas::Freq => {
            let r = first_rise?;
            Mark::Span(r, next_after(&a.rises, r)?)
        }
        Meas::PWidth => {
            let r = first_rise?;
            Mark::Span(r, next_after(&a.falls, r)?)
        }
        Meas::NWidth => {
            let f = first_fall?;
            Mark::Span(f, next_after(&a.rises, f)?)
        }
        Meas::Median => Mark::Level(a.median),
        Meas::Mode => Mark::Level(a.mode),
        Meas::EstUi if a.est_ui > 0.0 => {
            let r = first_rise?;
            Mark::Span(r, r + a.est_ui)
        }
        Meas::Rise if a.rise_time > 0.0 => {
            let r = first_rise?;
            Mark::Span(r - a.rise_time / 2.0, r + a.rise_time / 2.0)
        }
        Meas::Fall if a.fall_time > 0.0 => {
            let f = first_fall?;
            Mark::Span(f - a.fall_time / 2.0, f + a.fall_time / 2.0)
        }
        _ => return None,
    };
    Some(m)
}

impl Ui {
    fn meas_label(&self, cv: &mut Canvas, cx: i32, y: i32, text: &str, col: Rgb) {
        let f = &self.fonts;
        let a = self.area;
        let w = f.s.width(text) + 10;
        let x = (cx - w / 2).clamp(a.x + 2, a.x + a.w - w - 2);
        let r = Rect::new(x, (y - LABEL_H / 2).clamp(a.y + 2, a.y + a.h - LABEL_H - 2), w, LABEL_H);
        cv.round(r, 3, super::PANEL_BG, 225);
        cv.text_in(&f.s, r, 1, text, col);
    }

    pub(super) fn draw_meas_labels(&self, cv: &mut Canvas) {
        if !self.m.meas_labels {
            return;
        }
        let a = self.area;
        // spans in rows between the levels, vertical arrows from the left, level labels spread over the width
        let (mut rows, mut verts, mut levels) = (0, 0, 0);
        let (tdiv, delay) = self.view();
        for &(k, id) in self.m.meas.iter().take(MAX_LABELS) {
            let (ana, rec_t0, col) = match id {
                WaveId::Ch(c) => {
                    let rec = match self.last[0].as_ref() {
                        Some(r) if self.m.ch[c].on && self.shows(c) => r,
                        _ => continue,
                    };
                    let ana = &self.ana[c];
                    if !ana.valid {
                        continue;
                    }
                    (ana.clone(), rec.t0, CH_COLORS[c])
                }
                WaveId::Ref(i) => {
                    let Some(r) = self.refs.get(i).and_then(|x| x.as_ref()).filter(|r| r.on) else { continue };
                    let ana = &self.ref_ana[i];
                    if !ana.valid {
                        continue;
                    }
                    (ana.clone(), r.t0, REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1)
                }
            };
            let v = self.meas_of(&ana, k);
            if !v.is_finite() {
                continue;
            }
            let text = format!("{} {}", k.name(), si(v, k.unit()));
            // an arrow from the very edge of the screen is hard to read
            let left = delay as f64 - 4.7 * tdiv as f64 - rec_t0;
            match mark(k, &ana, left) {
                Some(Mark::Level(l)) => {
                    let y = self.y_of_wave(id, l as f32);
                    cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, col);
                    self.meas_label(cv, a.x + a.w * (3 + 2 * (levels % 3)) / 10, y - 11, &text, col);
                    levels += 1;
                }
                Some(Mark::Vertical(lo, hi)) => {
                    let x = a.x + 46 + verts * 30;
                    let (y0, y1) = (self.y_of_wave(id, hi as f32), self.y_of_wave(id, lo as f32));
                    cv.vline(x, y0, y1, col);
                    for (y, d) in [(y0, 1), (y1, -1)] {
                        cv.line(x, y, x - 4, y + 7 * d, col);
                        cv.line(x, y, x + 4, y + 7 * d, col);
                    }
                    self.meas_label(cv, x + 8 + label_half(self, &text), (y0 + y1) / 2 + verts * (LABEL_H + 2), &text, col);
                    verts += 1;
                }
                Some(Mark::Span(t0, t1)) => {
                    let (x0, x1) = (self.x_of_time((rec_t0 + t0) as f32), self.x_of_time((rec_t0 + t1) as f32));
                    if x1 <= x0 + 1 || x1 < a.x || x0 > a.x + a.w {
                        continue;
                    }
                    let mid = (ana.top + ana.base) / 2.0 + (ana.top - ana.base) * (0.22 - 0.16 * rows as f64);
                    let y = self.y_of_wave(id, mid as f32);
                    cv.hline(x0, x1, y, col);
                    let head = 7.min((x1 - x0) / 2);
                    for (x, d) in [(x0, 1), (x1, -1)] {
                        cv.line(x, y, x + head * d, y - 4, col);
                        cv.line(x, y, x + head * d, y + 4, col);
                    }
                    self.meas_label(cv, (x0 + x1) / 2, y - 11, &text, col);
                    rows += 1;
                }
                None => {}
            }
        }
    }
}

/// Half the width of a label: places it beside a vertical arrow.
fn label_half(ui: &Ui, text: &str) -> i32 {
    (ui.fonts.s.width(text) + 10) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Analysis {
        // rises at 1, 3, 5 ms, falls at 2, 4, 6 ms, 0..1 V
        Analysis { valid: true, vmin: -0.1, vmax: 1.1, top: 1.0, base: 0.0, rises: vec![1e-3, 3e-3, 5e-3], falls: vec![2e-3, 4e-3, 6e-3], rise_time: 1e-5, fall_time: 2e-5, ..Default::default() }
    }

    #[test]
    fn marks_of_levels_and_spans() {
        let a = square();
        assert_eq!(mark(Meas::Vtop, &a, 0.0), Some(Mark::Level(1.0)));
        assert_eq!(mark(Meas::Vpp, &a, 0.0), Some(Mark::Vertical(-0.1, 1.1)));
        assert_eq!(mark(Meas::Freq, &a, 0.0), Some(Mark::Span(1e-3, 3e-3)));
        assert_eq!(mark(Meas::PWidth, &a, 0.0), Some(Mark::Span(1e-3, 2e-3)));
        assert_eq!(mark(Meas::NWidth, &a, 0.0), Some(Mark::Span(2e-3, 3e-3)));
        assert_eq!(mark(Meas::Vrms, &a, 0.0), None);
    }

    #[test]
    fn spans_start_on_the_screen_and_need_two_edges() {
        let a = square();
        assert_eq!(mark(Meas::Period, &a, 2e-3), Some(Mark::Span(3e-3, 5e-3)));
        assert_eq!(mark(Meas::Period, &a, 4e-3), None);
    }
}
