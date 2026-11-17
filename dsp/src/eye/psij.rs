// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Power-supply-induced jitter: the TIE lines that a supply rail record shares, and a notch frequency, are fitted as
//! sines and taken out of the TIE trend, and the eye is measured again without them. Another correction of the TIE by
//! a second record would sit next to `psij`.

use super::jitter::tie_spectrum;
use super::periodic::{pj_lines, tone_fit};
use super::{is_positive, Clock, Eye};

/// Before and after a PSIJ notch: TIE peak-to-peak, periodic-jitter RMS, eye height, eye width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsijSide {
    pub tie_pp: f64,
    pub pj: f64,
    pub height: f64,
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Psij {
    pub before: PsijSide,
    pub after: PsijSide,
}

fn tie_pp(t: &[f64]) -> f64 {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &x in t.iter().filter(|x| x.is_finite()) {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if lo.is_finite() {
        hi - lo
    } else {
        f64::NAN
    }
}

fn pj_rss(trend: &[f32], period: f64) -> f64 {
    if trend.len() < 8 || !is_positive(period) {
        return f64::NAN;
    }
    let (df, spec) = tie_spectrum(trend, period);
    let lines = pj_lines(df, &spec, 8);
    if lines.is_empty() {
        0.0
    } else {
        lines.iter().map(|l| l.1 * l.1).sum::<f64>().sqrt()
    }
}

/// Correlated rail lines and the notch frequency are subtracted as fitted sines from the TIE trend.
/// `rail` is (samples, dt) with its mean removed here. `wave` is the victim record, used to remeasure the eye
/// when a line was removed. `height` and `width` are the eye figures before the correction.
pub fn psij(
    trend: &[f32],
    c: &Clock,
    tie: &[f64],
    rail: Option<(&[f32], f64)>,
    notch: f64,
    height: f64,
    width: f64,
    wave: Option<(&[f32], f64)>,
) -> Psij {
    let before = PsijSide { tie_pp: tie_pp(tie), pj: pj_rss(trend, c.period), height, width };
    let mut after = before;
    if trend.len() < 8 || !is_positive(c.period) {
        return Psij { before, after };
    }
    let (df, spec) = tie_spectrum(trend, c.period);
    let victim = pj_lines(df, &spec, 32);
    let mut kill = Vec::new();
    if notch > 0.0 {
        kill.push(notch);
    }
    if let Some((rail, rdt)) = rail {
        if rail.len() >= 16 && rdt > 0.0 {
            let fin: Vec<f64> = rail.iter().copied().filter(|x| x.is_finite()).map(|x| x as f64).collect();
            if !fin.is_empty() {
                let m = fin.iter().sum::<f64>() / fin.len() as f64;
                let ac: Vec<f32> = rail.iter().map(|&x| if x.is_finite() { x - m as f32 } else { 0.0 }).collect();
                let (rdf, rspec) = crate::math::spectrum(&ac, rdt, crate::math::Window::Hann);
                let rlines = pj_lines(rdf, &rspec, 32);
                for (f, _) in &victim {
                    if rlines.iter().any(|(rf, _)| (rf - f).abs() <= df.max(1e-15)) {
                        kill.push(*f);
                    }
                }
            }
        }
    }
    kill.sort_by(f64::total_cmp);
    kill.dedup_by(|a, b| (*a - *b).abs() <= df.max(1e-15));
    if kill.is_empty() {
        return Psij { before, after };
    }
    let mut removed = vec![0.0; trend.len()];
    for f in &kill {
        for (k, s) in tone_fit(trend, c.period, *f).2.iter().enumerate() {
            if k < removed.len() {
                removed[k] += s;
            }
        }
    }
    let corrected: Vec<f32> = trend.iter().zip(&removed).map(|(t, r)| *t - *r as f32).collect();
    let u0 = c.edges.first().zip(tie.first()).map(|(&e, &ti)| c.ui_at(e - ti).round()).unwrap_or(0.0);
    let mut tie2 = tie.to_vec();
    let mut edges = c.edges.clone();
    for i in 0..tie2.len().min(edges.len()) {
        let u = c.ui_at(edges[i] - tie[i]).round() - u0;
        if u >= 0.0 {
            if let Some(&s) = removed.get(u as usize) {
                tie2[i] -= s;
                edges[i] -= s;
            }
        }
    }
    after.tie_pp = tie_pp(&tie2);
    after.pj = pj_rss(&corrected, c.period);
    if let Some((v, dt)) = wave {
        let fin: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).map(|x| x as f64).collect();
        if fin.len() >= 2 && dt > 0.0 {
            let (lo, hi) = fin.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
            let measure = |edges: &[f64]| {
                let clk = Clock { edges: edges.to_vec(), ..c.clone() };
                let mut e = Eye::new(96, 96, 2, lo as f32, hi as f32);
                e.accumulate(v, dt, &clk);
                let m = e.measure(c.period);
                (m.height, m.width)
            };
            let (hb, wb) = measure(&c.edges);
            let (ha, wa) = measure(&edges);
            // keep the page's eye as the before figure; move it by what this record's eye did
            if hb.is_finite() && ha.is_finite() {
                after.height = height + (ha - hb);
            }
            if wb.is_finite() && wa.is_finite() {
                after.width = width + (wa - wb);
            }
        }
    }
    Psij { before, after }
}
