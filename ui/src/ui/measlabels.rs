// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//! Measure > Labels on trace: what a measurement measures, on the trace: level lines for levels, arrows for spans.

use super::{si, Canvas, Rect, Ui, CH_COLORS, REF_COLORS};
use crate::model::{WaveId, VDIV};
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

/// Time of the strongest on-screen peak of a vertical (pk-pk / amp) arrow (farthest from the mid-level).
pub(super) fn vertical_t(v: &[f32], t0: f64, dt: f64, t_lo: f64, t_hi: f64, _lo: f64, hi: f64) -> Option<f64> {
    if !(dt > 0.0) {
        return None;
    }
    let i0 = ((t_lo - t0) / dt).floor().max(0.0) as usize;
    let mut best: Option<(f64, f64)> = None;
    for (k, &x) in v.iter().enumerate().skip(i0) {
        let t = t0 + k as f64 * dt;
        if t > t_hi {
            break;
        }
        if t < t_lo || !x.is_finite() {
            continue;
        }
        // sit on the high end of the span (pk-pk max / amplitude top)
        let d = (x as f64 - hi).abs();
        if best.map_or(true, |(s, _)| d < s) {
            best = Some((d, t));
        }
    }
    best.map(|(_, t)| t)
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

    /// Pixel y of voltage `v` on the same scale the trace is drawn with.
    fn y_of_id(&self, id: WaveId, v: f32) -> i32 {
        match id {
            WaveId::Ch(c) => self.y_of_volts(c, v),
            WaveId::Ref(i) => {
                let a = self.area;
                let Some(r) = self.refs.get(i).and_then(|r| r.as_ref()) else {
                    return a.y + a.h / 2;
                };
                let ppy = a.h as f32 / VDIV;
                let cy = a.y as f32 + a.h as f32 / 2.0;
                (cy - (v + r.offset) / r.vpd.max(1e-12) * ppy).round() as i32
            }
        }
    }

    fn label_col(&self, id: WaveId) -> Rgb {
        match id {
            WaveId::Ch(c) => CH_COLORS[c.min(CH_COLORS.len() - 1)],
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).map(|r| REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1).unwrap_or(CH_COLORS[0]),
        }
    }

    fn label_wave(&self, id: WaveId) -> Option<(&[f32], f64, f64)> {
        match id {
            WaveId::Ch(c) => {
                let rec = self.last[0].as_ref()?;
                let v = rec.ch.get(c)?;
                (!v.is_empty()).then_some((v.as_slice(), rec.t0, rec.dt))
            }
            WaveId::Ref(i) => {
                let (v, t0, dt) = self.reference_slice(i)?;
                (v.len() >= 2 && dt > 0.0).then_some((v, t0, dt))
            }
        }
    }

    fn label_shows(&self, id: WaveId) -> bool {
        match id {
            WaveId::Ch(c) => self.m.ch.get(c).is_some_and(|ch| ch.on) && self.shows(c),
            WaveId::Ref(i) => self.cur_win == 0 && self.refs.get(i).and_then(|r| r.as_ref()).is_some_and(|r| r.on),
        }
    }

    pub(super) fn draw_meas_labels(&self, cv: &mut Canvas) {
        if !self.m.meas_labels {
            return;
        }
        let a = self.area;
        // voltage spans sit on the first on-screen peak/trough; time spans on the edges; levels across the width
        let (mut rows, mut verts, mut levels) = (0, 0, 0);
        let (tdiv, delay) = self.view();
        let t_lo = delay as f64 - 5.0 * tdiv as f64;
        let t_hi = delay as f64 + 5.0 * tdiv as f64;
        for &(k, id) in self.m.meas.iter().take(MAX_LABELS) {
            if !self.label_shows(id) {
                continue;
            }
            let Some((samp, wave_t0, dt)) = self.label_wave(id) else { continue };
            let ana = match id {
                WaveId::Ch(c) => &self.ana[c],
                WaveId::Ref(i) => &self.ref_ana[i],
            };
            if !ana.valid {
                continue;
            }
            let v = self.meas_number(id, k);
            if !v.is_finite() {
                continue;
            }
            let col = self.label_col(id);
            let text = format!("{} {}", k.name(), si(v, k.unit()));
            let left = t_lo - wave_t0;
            match mark(k, &ana, left) {
                Some(Mark::Level(l)) => {
                    let y = self.y_of_id(id, l as f32);
                    cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, col);
                    self.meas_label(cv, a.x + a.w * (3 + 2 * (levels % 3)) / 10, y - 11, &text, col);
                    levels += 1;
                }
                Some(Mark::Vertical(lo, hi)) => {
                    let x = vertical_t(samp, wave_t0, dt, t_lo, t_hi, lo, hi)
                        .map(|t| self.x_of_time(t as f32))
                        .unwrap_or(a.x + 46 + verts * 30);
                    let x = (x + verts * 16).clamp(a.x + 24, a.x + a.w - 24);
                    let (y0, y1) = (self.y_of_id(id, hi as f32), self.y_of_id(id, lo as f32));
                    cv.vline(x, y0, y1, col);
                    for (y, d) in [(y0, 1), (y1, -1)] {
                        cv.line(x, y, x - 4, y + 7 * d, col);
                        cv.line(x, y, x + 4, y + 7 * d, col);
                    }
                    self.meas_label(cv, x + 8 + label_half(self, &text), (y0 + y1) / 2 + verts * (LABEL_H + 2), &text, col);
                    verts += 1;
                }
                Some(Mark::Span(t0, t1)) => {
                    let (x0, x1) = (self.x_of_time((wave_t0 + t0) as f32), self.x_of_time((wave_t0 + t1) as f32));
                    if x1 <= x0 + 1 || x1 < a.x || x0 > a.x + a.w {
                        continue;
                    }
                    let mid = (ana.top + ana.base) / 2.0 + (ana.top - ana.base) * (0.22 - 0.16 * rows as f64);
                    let y = self.y_of_id(id, mid as f32);
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

    #[test]
    fn vertical_arrow_sits_on_the_on_screen_peak() {
        // 2 ms of zeros, a 1 V peak at 2 ms, then zeros; the screen starts at 1.5 ms
        let mut v = vec![0.0f32; 400];
        v[300] = 1.0;
        let t0 = -1e-3;
        let dt = 1e-5;
        assert_eq!(vertical_t(&v, t0, dt, 1.5e-3, 3e-3, 0.0, 1.0), Some(t0 + 300.0 * dt));
        assert_eq!(vertical_t(&v, t0, dt, -1e-3, 0.0, 0.0, 1.0), Some(t0));
    }
}
