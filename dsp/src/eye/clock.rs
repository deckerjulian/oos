// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Clock recovery for the eye and jitter analysis: the unit interval from edge spacings, a constant-rate
//! least-squares clock or a 1st/2nd-order PLL from data edges, a strobe (DQS) clock, and gating around it. A new way
//! to recover a clock goes here and returns a `Clock`, so the eye, TIE and jitter code takes it unchanged.

use super::{is_positive, line_fit};
use crate::edges::edges;

/// A recovered clock: ideal edges at phase + k period (constant clock), or at `ticks` (PLL clock, one tick per UI
/// covering the whole record; empty for the constant clock).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clock {
    /// unit interval, s
    pub period: f64,
    /// time of tick 0, s
    pub phase: f64,
    /// data edge times, s
    pub edges: Vec<f64>,
    pub ticks: Vec<f64>,
    /// the first edge rises; edges alternate from there
    pub first_rising: bool,
}

impl Clock {
    pub fn rate(&self) -> f64 {
        1.0 / self.period
    }
    /// Position of time `t` in unit intervals from tick 0 (fractional; ticks are at integers).
    pub fn ui_at(&self, t: f64) -> f64 {
        if self.ticks.len() < 2 {
            return (t - self.phase) / self.period;
        }
        let j = self.ticks.partition_point(|&x| x <= t).clamp(1, self.ticks.len() - 1) - 1;
        let p = self.ticks[j + 1] - self.ticks[j];
        j as f64 + (t - self.ticks[j]) / if p > 0.0 { p } else { self.period }
    }
    /// The ideal edge time nearest to `t`.
    pub fn nearest_tick(&self, t: f64) -> f64 {
        if self.ticks.is_empty() {
            return self.phase + ((t - self.phase) / self.period).round() * self.period;
        }
        let j = self.ticks.partition_point(|&x| x <= t);
        let a = self.ticks[j.saturating_sub(1)];
        let b = self.ticks[j.min(self.ticks.len() - 1)];
        if (t - a).abs() <= (b - t).abs() {
            a
        } else {
            b
        }
    }
}

/// Mean |x/u - round(x/u)| over the spacings of at most 12 units.
fn fit_err(d: &[f64], u: f64) -> f64 {
    let (s, n) = d
        .iter()
        .filter(|&&x| x / u < 12.5)
        .fold((0.0, 0usize), |(s, n), &x| (s + (x / u - (x / u).round()).abs(), n + 1));
    if n == 0 {
        1.0
    } else {
        s / n as f64
    }
}

/// Refines a unit estimate: sum of the spacings / sum of their unit counts (spacings of at most 12 units within
/// ±0.25 unit of an integer, so idle gaps of arbitrary length between UART characters do not bias it).
fn refine(d: &[f64], mut u: f64) -> f64 {
    for _ in 0..3 {
        let (s, k) = d.iter().fold((0.0, 0.0), |(s, k), &x| {
            let r = (x / u).round();
            if (1.0..=12.0).contains(&r) && (x / u - r).abs() < 0.25 {
                (s + x, k + r)
            } else {
                (s, k)
            }
        });
        if k > 0.0 {
            u = s / k;
        }
    }
    u
}

/// Unit interval of a set of edge spacings (s): every spacing of an NRZ / UART / CAN signal is an integer number of
/// units. The shortest robust spacing (5th percentile, averaged with its neighbours within ±50 %) is the first guess;
/// if the spacings are not integer multiples of it, it is divided by 2, 3 or 4 (no single-unit runs in the record);
/// the result is refined over all spacings of up to 12 units. `None` for fewer than 2 spacings.
pub fn unit_interval(spacings: &[f64]) -> Option<f64> {
    let mut s: Vec<f64> = spacings.iter().copied().filter(|x| x.is_finite() && *x > 0.0).collect();
    if s.len() < 2 {
        return None;
    }
    s.sort_by(f64::total_cmp);
    let p = s[s.len() / 20];
    let near: Vec<f64> = s.iter().copied().filter(|&x| x > 0.5 * p && x < 1.5 * p).collect();
    let u0 = near.iter().sum::<f64>() / near.len().max(1) as f64;
    for k in 1..=4 {
        let u = refine(&s, u0 / k as f64);
        if fit_err(&s, u) < 0.1 {
            return Some(u);
        }
    }
    Some(refine(&s, u0))
}

/// Least-squares line t = phase + n period through (n, t).
/// Constant-rate clock recovered from the data edges of `v` (comparator `thr` / `hyst`): the unit interval from the
/// edge spacings (`unit_interval`, or refined from `nominal_rate` bit/s when given), then a least-squares fit of the
/// edge times against their UI numbers (two passes). `None` with fewer than 3 edges.
pub fn recover_clock(v: &[f32], dt: f64, thr: f32, hyst: f32, nominal_rate: Option<f64>) -> Option<Clock> {
    let found = edges(v, thr, hyst);
    let first_rising = found.first().is_some_and(|x| x.rising);
    let e: Vec<f64> = found.iter().map(|x| x.t * dt).collect();
    // a unit interval shorter than half a sample is no clock of this record (a signal that changes at every sample
    // has one of a sample): a nominal rate from a fast time base, kept at a slow one, gave a clock with hundreds of
    // millions of unit intervals between two edges, and what holds a value per unit interval (`recover_clock_pll`,
    // `tie_trend`, the eye itself) took all the memory there is (seen on a unit: 3.7 GB, and the program was killed)
    clock_from_edges(e, nominal_rate).filter(|c| c.period >= 0.5 * dt).map(|c| Clock { first_rising, ..c })
}

/// Constant-rate clock from edge times (s), see `recover_clock`.
pub fn clock_from_edges(e: Vec<f64>, nominal_rate: Option<f64>) -> Option<Clock> {
    if e.len() < 3 {
        return None;
    }
    let d: Vec<f64> = e.windows(2).map(|w| w[1] - w[0]).collect();
    let u0 = match nominal_rate {
        Some(r) if r > 0.0 && r.is_finite() => refine(&d, 1.0 / r),
        _ => unit_interval(&d)?,
    };
    if !is_positive(u0) {
        return None;
    }
    let mut n = vec![0.0f64; e.len()];
    for i in 1..e.len() {
        n[i] = n[i - 1] + ((e[i] - e[i - 1]) / u0).round();
    }
    let (mut phase, mut period) = line_fit(&n, &e)?;
    for _ in 0..2 {
        if !is_positive(period) {
            return None;
        }
        n.iter_mut().zip(&e).for_each(|(k, &t)| *k = ((t - phase) / period).round());
        (phase, period) = line_fit(&n, &e)?;
    }
    if !is_positive(period) {
        return None;
    }
    Some(Clock { period, phase, edges: e, ticks: Vec::new(), first_rising: false })
}

/// Samples more than `uis` unit intervals from every clock edge become NaN, so idle time between strobe
/// bursts is not folded into the eye.
pub fn gate_around_clock(v: &[f32], dt: f64, c: &Clock, uis: f64) -> Vec<f32> {
    if v.is_empty() || !is_positive(dt) || !is_positive(c.period) || c.edges.is_empty() {
        return v.to_vec();
    }
    let n = v.len();
    let w = ((uis * c.period / dt).ceil() as usize).max(1);
    let mut keep = vec![false; n];
    for &te in &c.edges {
        let i = (te / dt).round().clamp(0.0, (n.saturating_sub(1)) as f64) as usize;
        let a = i.saturating_sub(w);
        let b = (i + w).min(n.saturating_sub(1));
        keep[a..=b].fill(true);
    }
    v.iter().zip(keep).map(|(&x, k)| if k { x } else { f32::NAN }).collect()
}

/// Half a UI earlier: a source-synchronous strobe samples the middle of the bit, which this page draws at 0.5 UI.
pub fn center_strobe(c: &Clock) -> Clock {
    let mut o = c.clone();
    o.phase -= 0.5 * o.period;
    for t in &mut o.ticks {
        *t -= 0.5 * c.period;
    }
    o
}

/// PLL clock: starts locked on the constant-rate fit, then a digital PLL corrects the tick nearest to every data edge
/// by the phase error. `loop_bw` = closed-loop bandwidth in Hz (0 = rate / 1667, the common "golden PLL"); 1st order
/// (phase only) or 2nd order (phase and frequency, damping 0.707). The gains account for the edge density (edges per
/// UI). The ticks cover the whole record. A bandwidth near the edge rate would make the discrete loop unstable and
/// drive the period to zero or below, so the loop is held where it is stable (natural frequency x time between edges
/// at most 0.6) and the period within 10 % of the constant-rate clock: every tick step stays positive.
pub fn recover_clock_pll(
    v: &[f32],
    dt: f64,
    thr: f32,
    hyst: f32,
    nominal_rate: Option<f64>,
    loop_bw: f64,
    second_order: bool,
) -> Option<Clock> {
    let c = recover_clock(v, dt, thr, hyst, nominal_rate)?;
    let end = v.len() as f64 * dt;
    let (first, last) = (c.edges[0], c.edges[c.edges.len() - 1]);
    let uis = ((last - first) / c.period).round().max(1.0);
    let density = (c.edges.len() as f64 / uis).clamp(0.01, 1.0);
    let bw = if loop_bw > 0.0 { loop_bw } else { c.rate() / 1667.0 };
    // natural frequency x time between edges, at most where the discrete 2nd-order loop is still stable
    let wn_te = (2.0 * std::f64::consts::PI * bw * c.period / density).min(0.6);
    let (kp, ki) =
        if second_order { ((2.0 * 0.707 * wn_te).min(1.0), wn_te * wn_te * density) } else { (wn_te.min(1.0), 0.0) };
    let mut period = c.period;
    let mut next = c.phase - (c.phase / period).ceil() * period; // first tick at or before t = 0
    let mut ticks = Vec::with_capacity((end / period) as usize + 2);
    for &te in &c.edges {
        while next + period / 2.0 < te {
            ticks.push(next);
            next += period;
        }
        let err = te - next;
        next += kp * err;
        period = (period + ki * err).clamp(0.9 * c.period, 1.1 * c.period);
    }
    while next <= end + period {
        ticks.push(next);
        next += period;
    }
    let k0 = ticks.partition_point(|&x| x < c.phase - period / 2.0);
    // keep tick 0 = the tick nearest the constant clock's phase, so UI numbers stay comparable
    let phase = ticks.get(k0).copied().unwrap_or(c.phase);
    Some(Clock { period: c.period, phase, edges: c.edges, ticks, first_rising: c.first_rising })
}
