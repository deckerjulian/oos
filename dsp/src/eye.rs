// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Eye diagram and jitter: clock recovery from data or from a strobe (DQS), constant-rate least-squares fit or a 1st/2nd-order PLL,
//! eye accumulation with linear interpolation between samples, eye measurements, time interval error (TIE), RJ/DJ
//! separation with a dual-Dirac tail fit, DCD and DDJ from conditional means, periodic jitter from the TIE spectrum
//! and the bathtub curve.
//!
//! Times here are seconds from the first sample (not sample indices), the clock is "ticks" at the ideal edge times.

use crate::edges::edges;

/// Q of the Gaussian tail at BER 1e-12 (TJ = DJ + 2 Q RJ).
pub const Q_BER_1E12: f64 = 7.034;

/// Edges needed for any jitter model.
pub const MIN_EDGES: usize = 8;
/// Edges needed for the tail fit: below this RJ scatters by about 40 %, above it by about 20 % (Gaussian test data).
pub const TAIL_EDGES: usize = 1024;
/// Distinct TIE values needed in each tail: fewer is a quantised TIE, not a tail.
const TAIL_DISTINCT: usize = 5;
/// Points kept per tail for the fit, and outermost points left out when thinning.
const TAIL_POINTS: usize = 48;
const TAIL_SKIP: usize = 3;
/// A TIE RMS below this (s) is the rounding of the edge times, not jitter.
const TIE_FLOOR: f64 = 1e-15;

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
    let (s, n) = d.iter().filter(|&&x| x / u < 12.5).fold((0.0, 0usize), |(s, n), &x| (s + (x / u - (x / u).round()).abs(), n + 1));
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
fn line_fit(n: &[f64], t: &[f64]) -> Option<(f64, f64)> {
    let k = n.len() as f64;
    if n.len() < 2 {
        return None;
    }
    let (mn, mt) = (n.iter().sum::<f64>() / k, t.iter().sum::<f64>() / k);
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (a, b) in n.iter().zip(t) {
        sxy += (a - mn) * (b - mt);
        sxx += (a - mn) * (a - mn);
    }
    if !(sxx > 0.0) {
        return None;
    }
    let p = sxy / sxx;
    Some((mt - p * mn, p))
}

/// Constant-rate clock recovered from the data edges of `v` (comparator `thr` / `hyst`): the unit interval from the
/// edge spacings (`unit_interval`, or refined from `nominal_rate` bit/s when given), then a least-squares fit of the
/// edge times against their UI numbers (two passes). `None` with fewer than 3 edges.
pub fn recover_clock(v: &[f32], dt: f64, thr: f32, hyst: f32, nominal_rate: Option<f64>) -> Option<Clock> {
    let found = edges(v, thr, hyst);
    let first_rising = found.first().is_some_and(|x| x.rising);
    let e: Vec<f64> = found.iter().map(|x| x.t * dt).collect();
    clock_from_edges(e, nominal_rate).map(|c| Clock { first_rising, ..c })
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
    if !(u0 > 0.0) {
        return None;
    }
    let mut n = vec![0.0f64; e.len()];
    for i in 1..e.len() {
        n[i] = n[i - 1] + ((e[i] - e[i - 1]) / u0).round();
    }
    let (mut phase, mut period) = line_fit(&n, &e)?;
    for _ in 0..2 {
        if !(period > 0.0) {
            return None;
        }
        n.iter_mut().zip(&e).for_each(|(k, &t)| *k = ((t - phase) / period).round());
        (phase, period) = line_fit(&n, &e)?;
    }
    if !(period > 0.0) {
        return None;
    }
    Some(Clock { period, phase, edges: e, ticks: Vec::new(), first_rising: false })
}

/// Samples more than `uis` unit intervals from every clock edge become NaN, so idle time between strobe
/// bursts is not folded into the eye.
pub fn gate_around_clock(v: &[f32], dt: f64, c: &Clock, uis: f64) -> Vec<f32> {
    if !(dt > 0.0) || !(c.period > 0.0) || c.edges.is_empty() {
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
/// UI). The ticks cover the whole record.
pub fn recover_clock_pll(v: &[f32], dt: f64, thr: f32, hyst: f32, nominal_rate: Option<f64>, loop_bw: f64, second_order: bool) -> Option<Clock> {
    let c = recover_clock(v, dt, thr, hyst, nominal_rate)?;
    let end = v.len() as f64 * dt;
    let (first, last) = (c.edges[0], c.edges[c.edges.len() - 1]);
    let uis = ((last - first) / c.period).round().max(1.0);
    let density = (c.edges.len() as f64 / uis).clamp(0.01, 1.0);
    let bw = if loop_bw > 0.0 { loop_bw } else { c.rate() / 1667.0 };
    let wn_te = 2.0 * std::f64::consts::PI * bw * c.period / density; // natural frequency x time between edges
    let (kp, ki) = if second_order { ((2.0 * 0.707 * wn_te).min(1.0), wn_te * wn_te * density) } else { (wn_te.min(1.0), 0.0) };
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
        period += ki * err;
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

/// Time interval error of every edge: edge time minus the nearest ideal clock edge (s).
pub fn tie(edges: &[f64], c: &Clock) -> Vec<f64> {
    if !(c.period > 0.0) {
        return Vec::new();
    }
    edges.iter().map(|&t| t - c.nearest_tick(t)).collect()
}

/// TIE trend: the TIE linearly interpolated onto every UI between the first and the last edge (one value per UI, s);
/// the input of `tie_spectrum`.
pub fn tie_trend(edges: &[f64], tie: &[f64], c: &Clock) -> Vec<f32> {
    let pts: Vec<(i64, f64)> = edges.iter().zip(tie).map(|(&t, &e)| (c.ui_at(t - e).round() as i64, e)).collect();
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let ((n0, e0), (n1, e1)) = (w[0], w[1]);
        if n1 <= n0 {
            continue;
        }
        for k in n0..n1 {
            out.push((e0 + (e1 - e0) * (k - n0) as f64 / (n1 - n0) as f64) as f32);
        }
    }
    if let Some(&(_, e)) = pts.last() {
        out.push(e as f32);
    }
    out
}

/// Jitter spectrum of a TIE trend: (bin spacing Hz, RMS seconds per bin), Hann window; periodic jitter shows as lines.
pub fn tie_spectrum(trend: &[f32], period: f64) -> (f64, Vec<f64>) {
    crate::math::spectrum(trend, period, crate::math::Window::Hann)
}

/// How the RJ / DJ / TJ of a `Jitter` were found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JitterFit {
    /// fewer than `MIN_EDGES` edges: RJ, DJ and TJ are NaN
    #[default]
    TooFew,
    /// the TIE is below `TIE_FLOOR`: RJ, DJ and TJ are 0
    Quiet,
    /// no usable tails (few edges or a quantised TIE): `rj_rms` and `tj_pp` are upper limits, DJ is NaN
    Limit,
    /// dual-Dirac fit of the two tails
    Tails,
}

/// Jitter decomposition of a TIE record (s).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Jitter {
    pub fit: JitterFit,
    /// total jitter at BER 1e-12: dj_pp + 2 x 7.034 x rj_rms
    pub tj_pp: f64,
    /// random jitter (Gaussian sigma of the tails)
    pub rj_rms: f64,
    /// deterministic jitter, dual-Dirac (distance between the two tail means)
    pub dj_pp: f64,
    /// the two Gaussian tails on the TIE axis: (mean, sigma)
    pub left: (f64, f64),
    pub right: (f64, f64),
    /// measured peak-to-peak and RMS of the TIE
    pub pp: f64,
    pub rms: f64,
    /// RMS of the change of TIE between consecutive edges (period jitter for a clock)
    pub period_jitter_rms: f64,
    /// RMS of the change between consecutive periods (cycle-to-cycle)
    pub c2c_rms: f64,
    /// TIE histogram, 64 bins: (bin centre s, count)
    pub histogram: Vec<(f64, u32)>,
}

/// Inverse of the standard normal CDF (Acklam's rational approximation, relative error < 1.2e-9).
pub fn norm_inv(p: f64) -> f64 {
    const A: [f64; 6] = [-3.969683028665376e1, 2.209460984245205e2, -2.759285104469687e2, 1.383577518672690e2, -3.066479806614716e1, 2.506628277459239];
    const B: [f64; 5] = [-5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2, 6.680131188771972e1, -1.328068155288572e1];
    const C: [f64; 6] = [-7.784894002430293e-3, -3.223964580411365e-1, -2.400758277161838, -2.549732539343734, 4.374664141464968, 2.938163982698783];
    const D: [f64; 4] = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996, 3.754408661907416];
    let p = p.clamp(1e-300, 1.0 - 1e-16);
    let tail = |q: f64| (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0);
    if p < 0.02425 {
        tail((-2.0 * p.ln()).sqrt())
    } else if p > 1.0 - 0.02425 {
        -tail((-2.0 * (1.0 - p).ln()).sqrt())
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    }
}

/// Gaussian tail fit on the Q scale: the tail points (x, p) with p = ρ Φ((x - μ) / σ); for each weight ρ on a grid
/// the line x = μ + σ Φ⁻¹(p / ρ) is fitted, the ρ with the smallest normalised residual wins. Returns (μ, σ).
fn tail_fit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let p_max = pts.iter().fold(0.0f64, |a, x| a.max(x.1));
    let mut best: Option<(f64, f64, f64)> = None;
    let mut rho = 1.0;
    while rho >= 2.5 * p_max && rho > 0.04 {
        let z: Vec<f64> = pts.iter().map(|&(_, p)| norm_inv(p / rho)).collect();
        let x: Vec<f64> = pts.iter().map(|&(x, _)| x).collect();
        if let Some((mu, sigma)) = line_fit(&z, &x) {
            if sigma > 0.0 {
                let rss: f64 = z.iter().zip(&x).map(|(z, x)| (x - mu - sigma * z).powi(2)).sum::<f64>() / (sigma * sigma * z.len() as f64);
                if best.map_or(true, |b| rss < b.2) {
                    best = Some((mu, sigma, rss));
                }
            }
        }
        rho -= 0.05;
    }
    best.map(|b| (b.0, b.1))
}

fn rms_dev(x: &[f64]) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    let m = x.iter().sum::<f64>() / x.len() as f64;
    (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

/// Tail points of the sorted `s` (outer end first) up to rank `k_max`: (value, CDF at the mid rank). Values within
/// `tol` are one point, so a quantised TIE gives its few steps and not a flat tail. Long tails are thinned evenly in
/// log p, without the `TAIL_SKIP` outermost points, which scatter most.
fn tail_points(s: &[f64], k_max: usize, tol: f64) -> Vec<(f64, f64)> {
    let n = s.len() as f64;
    let mut pts = Vec::new();
    let mut i = 0;
    while i < k_max.min(s.len()) {
        let mut j = i + 1;
        while j < s.len() && s[j] - s[i] <= tol {
            j += 1;
        }
        pts.push((s[i..j].iter().sum::<f64>() / (j - i) as f64, (i + j) as f64 / 2.0 / n));
        i = j;
    }
    if pts.len() <= TAIL_POINTS {
        return pts;
    }
    let (first, last) = (TAIL_SKIP as f64 + 1.0, pts.len() as f64);
    let mut idx: Vec<usize> = (0..TAIL_POINTS).map(|k| (first * (last / first).powf(k as f64 / (TAIL_POINTS - 1) as f64) - 1.0).round() as usize).collect();
    idx.dedup();
    idx.into_iter().map(|k| pts[k]).collect()
}

/// RJ / DJ separation with the dual-Dirac model: the TIE distribution is taken as two Diracs (DJ) convolved with a
/// Gaussian (RJ). Each tail of the empirical CDF (the outer 5 % of the edges, at least 10 points) is fitted with a
/// Gaussian on the Q scale (`tail_fit`, free tail weight); RJ = mean of the two sigmas, DJ = right mean - left mean
/// (>= 0).
///
/// The fit needs `TAIL_EDGES` edges and `TAIL_DISTINCT` different values in each tail. A clean periodic signal often
/// has neither: few edges in the window, or edges that all fall on the same sample phase so the TIE takes a few values
/// only. Then the model still bounds the result: with dual-Dirac DJ and Gaussian RJ, rms^2 = (DJ/2)^2 + RJ^2, so
/// RJ <= rms and TJ <= 2 sqrt(1 + Q^2) rms (`JitterFit::Limit`). Below `MIN_EDGES` only pp / rms / period values are
/// filled. The caller pools records to reach the tail fit.
pub fn jitter(tie: &[f64]) -> Jitter {
    let t: Vec<f64> = tie.iter().copied().filter(|x| x.is_finite()).collect();
    let mut j = Jitter { fit: JitterFit::TooFew, tj_pp: f64::NAN, rj_rms: f64::NAN, dj_pp: f64::NAN, left: (f64::NAN, f64::NAN), right: (f64::NAN, f64::NAN), pp: f64::NAN, rms: rms_dev(&t), period_jitter_rms: f64::NAN, c2c_rms: f64::NAN, histogram: Vec::new() };
    if t.is_empty() {
        return j;
    }
    let d: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    let d2: Vec<f64> = d.windows(2).map(|w| w[1] - w[0]).collect();
    j.period_jitter_rms = rms_dev(&d);
    j.c2c_rms = rms_dev(&d2);
    let mut s = t;
    s.sort_by(f64::total_cmp);
    let (lo, hi) = (s[0], s[s.len() - 1]);
    j.pp = hi - lo;
    let bw = (hi - lo) / 64.0;
    let mut h = vec![0u32; 64];
    for &x in &s {
        h[if bw > 0.0 { (((x - lo) / bw) as usize).min(63) } else { 0 }] += 1;
    }
    j.histogram = h.iter().enumerate().map(|(i, &c)| (lo + (i as f64 + 0.5) * bw, c)).collect();
    let n = s.len();
    if n < MIN_EDGES {
        return j;
    }
    let mean = s.iter().sum::<f64>() / n as f64;
    if j.rms < TIE_FLOOR {
        (j.rj_rms, j.dj_pp, j.tj_pp, j.fit) = (0.0, 0.0, 0.0, JitterFit::Quiet);
        (j.left, j.right) = ((mean, 0.0), (mean, 0.0));
        return j;
    }
    if !fit_tails(&s, &mut j) {
        let tj = 2.0 * (1.0 + Q_BER_1E12 * Q_BER_1E12).sqrt() * j.rms;
        let half = 0.5 * tj - Q_BER_1E12 * j.rms;
        (j.rj_rms, j.tj_pp, j.fit) = (j.rms, tj, JitterFit::Limit);
        (j.left, j.right) = ((mean - half, j.rms), (mean + half, j.rms));
    }
    j
}

/// The dual-Dirac fit on the sorted TIE; false when the tails do not carry it. A sigma well above the RMS cannot be
/// (rms^2 = (DJ/2)^2 + RJ^2), it comes from a few stray points.
fn fit_tails(s: &[f64], j: &mut Jitter) -> bool {
    let n = s.len();
    if n < TAIL_EDGES {
        return false;
    }
    let p_max = (0.05f64).max(10.0 / n as f64).min(0.2);
    let k_max = ((p_max * n as f64) as usize).max(3);
    let tol = 1e-3 * j.rms;
    let rev: Vec<f64> = s.iter().rev().map(|x| -x).collect();
    let (left, right) = (tail_points(s, k_max, tol), tail_points(&rev, k_max, tol));
    if left.len() < TAIL_DISTINCT || right.len() < TAIL_DISTINCT {
        return false;
    }
    let (Some((ml, sl)), Some((mr, sr))) = (tail_fit(&left), tail_fit(&right)) else { return false };
    let rj = 0.5 * (sl + sr);
    if rj > 1.25 * j.rms {
        return false;
    }
    // right mean below left mean is no DJ: one Gaussian in the middle
    let (a, b) = if ml <= -mr { (ml, -mr) } else { (0.5 * (ml - mr), 0.5 * (ml - mr)) };
    j.rj_rms = rj;
    j.dj_pp = b - a;
    j.tj_pp = j.dj_pp + 2.0 * Q_BER_1E12 * rj;
    (j.left, j.right) = ((a, sl), (b, sr));
    j.fit = JitterFit::Tails;
    true
}

/// Run length classes of `EdgeMeans`: 1 .. RUNS UI, longer runs in the last.
const RUNS: usize = 8;
/// Edges a class needs to count: its mean then carries rms / 8 of noise.
pub const RUN_MIN: u64 = 64;

/// Mean TIE per edge polarity and preceding run length, summed over records. Averaging takes the random part out, so
/// what stays is measured and needs no model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeMeans {
    sum: [[f64; RUNS]; 2],
    n: [[u64; RUNS]; 2],
}

impl EdgeMeans {
    /// Adds the TIE of a record (`tie` and `c.edges` in step).
    pub fn add(&mut self, tie: &[f64], c: &Clock) {
        if !(c.period > 0.0) {
            return;
        }
        for (i, (w, e)) in c.edges.windows(2).zip(tie.iter().skip(1)).enumerate() {
            if e.is_finite() {
                let rising = (i % 2 == 0) != c.first_rising;
                let run = (((w[1] - w[0]) / c.period).round() as usize).clamp(1, RUNS) - 1;
                self.sum[rising as usize][run] += e;
                self.n[rising as usize][run] += 1;
            }
        }
    }
    fn polarity_mean(&self, p: usize) -> Option<f64> {
        let n: u64 = self.n[p].iter().sum();
        (n >= RUN_MIN).then(|| self.sum[p].iter().sum::<f64>() / n as f64)
    }
    /// Duty cycle distortion: mean TIE of the rising edges minus that of the falling ones (s, NaN until both have
    /// enough edges).
    pub fn dcd(&self) -> f64 {
        match (self.polarity_mean(1), self.polarity_mean(0)) {
            (Some(r), Some(f)) => r - f,
            _ => f64::NAN,
        }
    }
    /// Deterministic TIE of one edge: the run-length mean when that class has `RUN_MIN` edges, else the polarity mean.
    /// `run_ui` is the spacing in unit intervals before the edge.
    pub fn edge_offset(&self, rising: bool, run_ui: usize) -> Option<f64> {
        let p = rising as usize;
        let run = run_ui.clamp(1, RUNS) - 1;
        if self.n[p][run] >= RUN_MIN {
            Some(self.sum[p][run] / self.n[p][run] as f64)
        } else {
            self.polarity_mean(p)
        }
    }
    /// Data-dependent jitter: peak-to-peak of the mean TIE over the run lengths before an edge, polarity taken out
    /// (s). NaN with fewer than two run lengths, as on a clock.
    pub fn ddj(&self) -> f64 {
        let (mut lo, mut hi, mut seen) = (f64::INFINITY, f64::NEG_INFINITY, [false; RUNS]);
        for p in 0..2 {
            let Some(m) = self.polarity_mean(p) else { continue };
            for r in (0..RUNS).filter(|&r| self.n[p][r] >= RUN_MIN) {
                let d = self.sum[p][r] / self.n[p][r] as f64 - m;
                (lo, hi, seen[r]) = (lo.min(d), hi.max(d), true);
            }
        }
        if seen.iter().filter(|&&x| x).count() < 2 {
            f64::NAN
        } else {
            hi - lo
        }
    }
}

/// Periodic jitter in a TIE spectrum (`tie_spectrum`): the bins standing 6 x above the median floor, strongest first,
/// as (Hz, RMS s); white jitter stays under that even at its highest bin.
pub fn pj_lines(df: f64, spec: &[f64], count: usize) -> Vec<(f64, f64)> {
    if spec.len() < 8 {
        return Vec::new();
    }
    let mut s: Vec<f64> = spec[2..].to_vec();
    s.sort_by(f64::total_cmp);
    let floor = s[s.len() / 2];
    crate::math::peaks(spec, count).into_iter().filter(|&(_, v)| v > 6.0 * floor).map(|(k, v)| (k as f64 * df, v)).collect()
}

/// Gaussian tail probability Q(z), relative error below 1 % out to where it underflows.
pub fn q_tail(z: f64) -> f64 {
    if z < 0.0 {
        return 1.0 - q_tail(-z);
    }
    let pdf = (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt();
    if z > 4.0 {
        let u = 1.0 / (z * z);
        return pdf / z * (1.0 - u + 3.0 * u * u - 15.0 * u * u * u);
    }
    let t = 1.0 / (1.0 + 0.2316419 * z);
    pdf * t * (0.319381530 + t * (-0.356563782 + t * (1.781477937 + t * (-1.821255978 + t * 1.330274429))))
}

/// Lowest log10 BER a bathtub reports.
pub const BATHTUB_FLOOR: f32 = -16.0;

/// Bathtub: error probability per edge against the sampling position between two crossings. An edge from the left
/// crossing lands right of the sampling point, or one from the right crossing left of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bathtub {
    /// sampling position, UI from the left crossing (0 .. 1)
    pub ui: Vec<f32>,
    /// log10 BER counted from the edges; NaN where none got that far
    pub measured: Vec<f32>,
    /// log10 BER of the tails of `Jitter`, down to `BATHTUB_FLOOR`; empty without a model
    pub model: Vec<f32>,
    /// opening at BER 1e-12 in UI: 1 - TJ / UI (at least 0); NaN without a model
    pub opening: f64,
    /// opening no edge entered in UI: 1 - pp / UI (at least 0)
    pub seen: f64,
}

/// Bathtub of the sorted TIE `s` with its `Jitter`, `points` positions across the UI. The model carries a tail weight
/// of 1, the convention of TJ = DJ + 2 Q RJ, so the 1e-12 opening of the curve is 1 - TJ / UI.
pub fn bathtub(s: &[f64], j: &Jitter, period: f64, points: usize) -> Bathtub {
    let mut b = Bathtub { opening: f64::NAN, seen: f64::NAN, ..Bathtub::default() };
    if !(period > 0.0) || s.is_empty() || points < 2 {
        return b;
    }
    let n = s.len() as f64;
    let modelled = j.tj_pp.is_finite();
    // probability that an edge lies more than `d` beyond its tail mean
    let beyond = |d: f64, sigma: f64| if sigma > 0.0 { q_tail(d / sigma) } else { (d < 0.0) as u8 as f64 };
    for k in 0..points {
        let u = k as f64 / (points - 1) as f64;
        let t = u * period;
        let late = s.len() - s.partition_point(|&x| x <= t);
        let early = s.partition_point(|&x| x < t - period);
        b.ui.push(u as f32);
        b.measured.push(if late + early == 0 { f32::NAN } else { ((late + early) as f64 / n).min(1.0).log10() as f32 });
        if modelled {
            // the right tail of the left crossing, the left tail of the right crossing
            let p = beyond(t - j.right.0, j.right.1) + beyond(j.left.0 + period - t, j.left.1);
            b.model.push((p.min(1.0).max(1e-300).log10() as f32).max(BATHTUB_FLOOR));
        }
    }
    b.seen = (1.0 - j.pp / period).max(0.0);
    if modelled {
        b.opening = (1.0 - j.tj_pp / period).max(0.0);
    }
    b
}

// ---------------------------------------------------------------------------------------------------------------
// eye diagram

/// Eye diagram: a hit counter per pixel; `ui_span` unit intervals across `w` columns with the eye centred (crossings
/// at span/2 ± 0.5 UI), `v_max` at row 0 and `v_min` at row h-1.
#[derive(Clone, Debug, PartialEq)]
pub struct Eye {
    pub w: usize,
    pub h: usize,
    pub ui_span: u32,
    pub v_min: f32,
    pub v_max: f32,
    pub hits: Vec<u32>,
    /// records accumulated
    pub records: u32,
}

/// All eye measurements (NaN when not measurable).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeMeas {
    /// one / zero levels (mean in the central 20 % of the eye) and their sigmas, V
    pub one: f64,
    pub zero: f64,
    pub sigma_one: f64,
    pub sigma_zero: f64,
    /// one - zero
    pub amplitude: f64,
    /// (one - 3 σ1) - (zero + 3 σ0)
    pub height: f64,
    /// Q-scale eye height at a bit-error rate: (one − Q σ1) − (zero + Q σ0), clamped at 0.
    /// Q is `norm_inv(1 − BER)`. Empty until `height_at` fills it. The 3σ `height` stays as it is.
    pub height_ber: f64,
    /// 1 UI - 6 σ of the crossing time, s
    pub width: f64,
    /// crossing level relative to zero..one, %
    pub crossing_pct: f64,
    /// (one - zero) / (σ1 + σ0)
    pub q_factor: f64,
    /// 20-80 % transition times on the eye, s
    pub rise: f64,
    pub fall: f64,
}

impl Eye {
    pub fn new(w: usize, h: usize, ui_span: u32, v_min: f32, v_max: f32) -> Eye {
        let (w, h) = (w.max(1), h.max(1));
        Eye { w, h, ui_span: ui_span.max(1), v_min, v_max, hits: vec![0; w * h], records: 0 }
    }
    pub fn clear(&mut self) {
        self.hits.iter_mut().for_each(|x| *x = 0);
        self.records = 0;
    }
    pub fn at(&self, x: usize, y: usize) -> u32 {
        self.hits[y * self.w + x]
    }
    fn span(&self) -> f64 {
        self.ui_span as f64
    }
    /// voltage at the centre of row y
    fn row_v(&self, y: usize) -> f64 {
        self.v_max as f64 - (y as f64 + 0.5) / self.h as f64 * (self.v_max - self.v_min) as f64
    }
    fn row_of(&self, v: f64) -> Option<usize> {
        let y = ((self.v_max as f64 - v) / (self.v_max - self.v_min) as f64 * self.h as f64).floor();
        (y >= 0.0 && y < self.h as f64).then_some(y as usize)
    }
    /// UI position of the centre of column x
    fn col_u(&self, x: usize) -> f64 {
        (x as f64 + 0.5) / self.w as f64 * self.span()
    }
    /// Hits the point in every UI of the span. Folding by the span instead would keep a clock, which repeats every
    /// 2 UI, in one phase: one level per slot and no crossing.
    fn plot(&mut self, u: f64, v: f64) {
        if let Some(y) = self.row_of(v) {
            let (f, cols) = (u.rem_euclid(1.0), self.w as f64 / self.span());
            for k in 0..self.ui_span {
                let i = y * self.w + (((f + k as f64) * cols) as usize).min(self.w - 1);
                self.hits[i] = self.hits[i].saturating_add(1);
            }
        }
    }

    /// Folds a record into the eye: each sample's position in UI comes from the clock (modulo 1 UI, drawn in every UI
    /// of the span); consecutive samples are joined by a line rasterised in steps of about one pixel, so a sparse
    /// record still draws lines.
    pub fn accumulate(&mut self, v: &[f32], dt: f64, c: &Clock) {
        if !(c.period > 0.0) || !(self.v_max > self.v_min) {
            return;
        }
        let off = self.span() / 2.0 - 0.5;
        let (cols_per_ui, rows_per_v) = (self.w as f64 / self.span(), self.h as f64 / (self.v_max - self.v_min) as f64);
        let max_steps = 4 * (self.w + self.h);
        // UI position of every sample: constant clock directly, PLL clock with a cursor over the ticks
        let mut j = 0usize;
        let mut ui = |t: f64| -> f64 {
            let tk = &c.ticks;
            if tk.len() < 2 {
                return (t - c.phase) / c.period;
            }
            while j + 2 < tk.len() && tk[j + 1] <= t {
                j += 1;
            }
            j as f64 + (t - tk[j]) / (tk[j + 1] - tk[j])
        };
        let mut prev: Option<(f64, f64)> = None;
        for (k, &x) in v.iter().enumerate() {
            if !x.is_finite() {
                prev = None;
                continue;
            }
            let u = ui(k as f64 * dt) + off;
            let x = x as f64;
            if let Some((pu, pv)) = prev {
                let steps = (((u - pu) * cols_per_ui).abs().max((x - pv).abs() * rows_per_v).ceil() as usize).clamp(1, max_steps);
                for s in 0..steps {
                    let a = s as f64 / steps as f64;
                    self.plot(pu + a * (u - pu), pv + a * (x - pv));
                }
            }
            prev = Some((u, x));
        }
        if let Some((u, x)) = prev {
            self.plot(u, x);
        }
        self.records = self.records.saturating_add(1);
    }

    /// Weighted (mean, sigma) of row voltages over the given rows and columns.
    fn stats(&self, rows: impl Iterator<Item = usize> + Clone, cols: &[usize]) -> (f64, f64, f64) {
        let (mut n, mut s, mut s2) = (0.0, 0.0, 0.0);
        for y in rows {
            let v = self.row_v(y);
            for &x in cols {
                let c = self.at(x, y) as f64;
                n += c;
                s += c * v;
                s2 += c * v * v;
            }
        }
        if n == 0.0 {
            return (f64::NAN, f64::NAN, 0.0);
        }
        let m = s / n;
        (m, (s2 / n - m * m).max(0.0).sqrt(), n)
    }

    fn cols_within(&self, centre: f64, half: f64) -> Vec<usize> {
        let span = self.span();
        let mut c: Vec<usize> = (0..self.w)
            .filter(|&x| {
                let d = (self.col_u(x) - centre + span / 2.0).rem_euclid(span) - span / 2.0;
                d.abs() <= half
            })
            .collect();
        if c.is_empty() {
            c.push((((centre.rem_euclid(span)) / span * self.w as f64) as usize).min(self.w - 1));
        }
        c
    }

    /// (one, zero, σ1, σ0): the hits in the central 20 % of the eye, split at their mean voltage.
    /// One rail on a side uses every hit there. Two rails (a transition bit and a de-emphasised bit)
    /// use the rail nearer the decision level: that is the opening. Averaging the two puts the level
    /// in the empty gap and treats the split as noise, which closes the eye at a low BER.
    pub fn levels(&self) -> (f64, f64, f64, f64) {
        let cols = self.cols_within(self.span() / 2.0, 0.1);
        let (mid, _, n) = self.stats(0..self.h, &cols);
        if n == 0.0 {
            return (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        }
        let (one, s1) = self.side_level(&cols, mid, true);
        let (zero, s0) = self.side_level(&cols, mid, false);
        (one, zero, s1, s0)
    }

    /// Mean and sigma on one side of `mid`. A second rail beyond a valley is left out.
    fn side_level(&self, cols: &[usize], mid: f64, upper: bool) -> (f64, f64) {
        let mut rows: Vec<usize> = (0..self.h).filter(|&y| (self.row_v(y) >= mid) == upper).collect();
        rows.sort_by(|&a, &b| (self.row_v(a) - mid).abs().total_cmp(&(self.row_v(b) - mid).abs()));
        let counts: Vec<f64> = rows.iter().map(|&y| cols.iter().map(|&x| self.at(x, y) as f64).sum()).collect();
        let (mean, sigma, n) = self.stats(rows.iter().copied(), cols);
        if n == 0.0 || counts.is_empty() {
            return (mean, sigma);
        }
        let sm: Vec<f64> = (0..counts.len())
            .map(|i| {
                let a = if i > 0 { counts[i - 1] } else { counts[i] };
                let c = if i + 1 < counts.len() { counts[i + 1] } else { counts[i] };
                (a + counts[i] + c) / 3.0
            })
            .collect();
        let sm_max = sm.iter().copied().fold(0.0, f64::max);
        if !(sm_max > 0.0) {
            return (mean, sigma);
        }
        let mut peak = None;
        for i in 1..sm.len().saturating_sub(1) {
            if sm[i] >= sm[i - 1] && sm[i] >= sm[i + 1] && sm[i] >= 0.2 * sm_max {
                peak = Some(i);
                break;
            }
        }
        let Some(p) = peak else { return (mean, sigma) };
        let mut valley: Option<usize> = None;
        let mut dropped = false;
        for i in (p + 1)..counts.len() {
            let local = sm[i] >= sm[i - 1] && (i + 1 >= sm.len() || sm[i] >= sm[i + 1]);
            if dropped && local && sm[i] >= 0.15 * sm_max && valley.is_some_and(|v| counts[i] > 3.0 * counts[v]) {
                let end = valley.unwrap_or(i);
                let floor = 0.15 * counts[p];
                let mut lo = p;
                while lo > 0 && counts[lo - 1] >= floor {
                    lo -= 1;
                }
                let (m, s, bn) = self.stats(rows[lo..end].iter().copied(), cols);
                if bn > 0.0 {
                    return (m, s);
                }
                break;
            }
            if counts[i] < 0.4 * counts[p] {
                dropped = true;
                if valley.map_or(true, |v| counts[i] < counts[v]) {
                    valley = Some(i);
                }
            }
        }
        (mean, sigma)
    }
    pub fn eye_amplitude(&self) -> f64 {
        let (one, zero, _, _) = self.levels();
        one - zero
    }
    pub fn eye_height(&self) -> f64 {
        let (one, zero, s1, s0) = self.levels();
        (one - 3.0 * s1) - (zero + 3.0 * s0)
    }
    pub fn q_factor(&self) -> f64 {
        let (one, zero, s1, s0) = self.levels();
        if s1 + s0 > 0.0 {
            (one - zero) / (s1 + s0)
        } else {
            f64::INFINITY
        }
    }
    /// Crossing level (V): mean of the hits between the levels at the left crossing (± 2 % UI).
    pub fn crossing_level(&self) -> f64 {
        let (one, zero, _, _) = self.levels();
        let a = one - zero;
        let cols = self.cols_within(self.span() / 2.0 - 0.5, 0.02);
        self.stats((0..self.h).filter(|&y| self.row_v(y) > zero + 0.1 * a && self.row_v(y) < one - 0.1 * a), &cols).0
    }
    pub fn crossing_pct(&self) -> f64 {
        let (one, zero, _, _) = self.levels();
        (self.crossing_level() - zero) / (one - zero) * 100.0
    }
    /// Offsets (UI) from the crossing of the hits in row y within ±0.5 UI: (offset, count).
    fn row_offsets(&self, y: usize) -> Vec<(f64, f64)> {
        let c = self.span() / 2.0 - 0.5;
        (0..self.w).filter(|&x| self.at(x, y) > 0).map(|x| (((self.col_u(x) - c + 0.5).rem_euclid(1.0)) - 0.5, self.at(x, y) as f64)).collect()
    }
    /// (mean, sigma) of the crossing time (UI, relative to the nominal crossing) in the rows at the crossing level.
    fn crossing_time(&self) -> (f64, f64) {
        let Some(y) = self.row_of(self.crossing_level()) else { return (f64::NAN, f64::NAN) };
        let mut o = Vec::new();
        for yy in y.saturating_sub(1)..=(y + 1).min(self.h - 1) {
            o.extend(self.row_offsets(yy));
        }
        let n: f64 = o.iter().map(|x| x.1).sum();
        if n == 0.0 {
            return (f64::NAN, f64::NAN);
        }
        let m = o.iter().map(|x| x.0 * x.1).sum::<f64>() / n;
        (m, (o.iter().map(|x| (x.0 - m).powi(2) * x.1).sum::<f64>() / n).sqrt())
    }
    /// Eye width (s): 1 UI - 6 σ of the crossing time.
    pub fn eye_width(&self, period: f64) -> f64 {
        (1.0 - 6.0 * self.crossing_time().1) * period
    }
    /// 20-80 % rise and fall times on the eye (s): at the 20 % and 80 % rows the hits left of the crossing belong to
    /// the rising (at 20 %) / falling (at 80 %) edges, those right of it to the other one.
    pub fn rise_fall(&self, period: f64) -> (f64, f64) {
        let (one, zero, _, _) = self.levels();
        let a = one - zero;
        let (mc, _) = self.crossing_time();
        let side = |level: f64, left: bool| -> f64 {
            let Some(y) = self.row_of(zero + level * a) else { return f64::NAN };
            let o: Vec<(f64, f64)> = self.row_offsets(y).into_iter().filter(|x| (x.0 < mc) == left).collect();
            let n: f64 = o.iter().map(|x| x.1).sum();
            if n == 0.0 {
                f64::NAN
            } else {
                o.iter().map(|x| x.0 * x.1).sum::<f64>() / n
            }
        };
        ((side(0.8, false) - side(0.2, true)) * period, (side(0.2, false) - side(0.8, true)) * period)
    }
    /// All measurements at once (`period` = unit interval, s).
    pub fn measure(&self, period: f64) -> EyeMeas {
        let (one, zero, s1, s0) = self.levels();
        let (rise, fall) = self.rise_fall(period);
        EyeMeas {
            one,
            zero,
            sigma_one: s1,
            sigma_zero: s0,
            amplitude: one - zero,
            height: self.eye_height(),
            height_ber: f64::NAN,
            width: self.eye_width(period),
            crossing_pct: self.crossing_pct(),
            q_factor: self.q_factor(),
            rise,
            fall,
        }
    }
}

impl EyeMeas {
    /// Vertical opening extrapolated to `ber`: replace the 3 of the 3σ height with `norm_inv(1 − ber)`, and clamp at 0.
    pub fn height_at(&self, ber: f64) -> f64 {
        if !(ber > 0.0 && ber < 1.0) {
            return f64::NAN;
        }
        let q = norm_inv(1.0 - ber);
        let h = (self.one - q * self.sigma_one) - (self.zero + q * self.sigma_zero);
        if h.is_finite() { h.max(0.0) } else { f64::NAN }
    }
}

/// Total jitter at a bit-error rate: DJ + 2 Q RJ, with Q = `norm_inv(1 − ber)`.
pub fn tj_at(j: &Jitter, ber: f64) -> f64 {
    if !(ber > 0.0 && ber < 1.0) || !j.dj_pp.is_finite() || !j.rj_rms.is_finite() {
        return f64::NAN;
    }
    j.dj_pp + 2.0 * norm_inv(1.0 - ber) * j.rj_rms
}

/// J2: total jitter at BER 2.5e-3. J9: total jitter at BER 2.5e-10.
pub const BER_J2: f64 = 2.5e-3;
pub const BER_J9: f64 = 2.5e-10;

/// Bathtub opening at `ber`, in unit intervals: 1 − TJ(ber) / UI, clamped at 0.
pub fn eye_width_at(j: &Jitter, period: f64, ber: f64) -> f64 {
    let tj = tj_at(j, ber);
    if !tj.is_finite() || !(period > 0.0) {
        return f64::NAN;
    }
    (1.0 - tj / period).max(0.0)
}

/// Cosine and sine amplitudes of one tone on a TIE trend, and the tone itself (seconds).
fn tone_fit(trend: &[f32], period: f64, f: f64) -> (f64, f64, Vec<f64>) {
    if trend.is_empty() || !(period > 0.0) || !(f > 0.0) {
        return (0.0, 0.0, Vec::new());
    }
    let (mut re, mut im) = crate::power::dft(trend, period, 0, trend.len(), f);
    // the 2/N scale counts DC and Nyquist twice; F/2 is the Nyquist line of a one-sample-per-UI trend
    if (f * period - 0.5).abs() < 1e-6 {
        re *= 0.5;
        im *= 0.5;
    }
    let s = (0..trend.len())
        .map(|k| {
            let th = 2.0 * std::f64::consts::PI * f * k as f64 * period;
            re * th.cos() - im * th.sin()
        })
        .collect();
    (re, im, s)
}

/// Peak-to-peak of one tone at `rate / n` fitted to the TIE trend. `2√2` times the RMS of that phasor.
pub fn subrate_pp(trend: &[f32], period: f64, n: u32) -> f64 {
    if n == 0 || !(period > 0.0) || trend.len() < 8 {
        return f64::NAN;
    }
    let (re, im, _) = tone_fit(trend, period, 1.0 / (period * n as f64));
    let rms = (re * re + im * im).sqrt() / std::f64::consts::SQRT_2;
    2.0 * std::f64::consts::SQRT_2 * rms
}

/// Peak-to-peak of the sum of the F/2, F/4 and F/8 tones, phases kept.
pub fn srj_pp(trend: &[f32], period: f64) -> f64 {
    if !(period > 0.0) || trend.len() < 8 {
        return f64::NAN;
    }
    let rate = 1.0 / period;
    let tones = [2.0, 4.0, 8.0].map(|n| tone_fit(trend, period, rate / n).2);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for k in 0..trend.len() {
        let s = tones[0][k] + tones[1][k] + tones[2][k];
        lo = lo.min(s);
        hi = hi.max(s);
    }
    if lo.is_finite() { hi - lo } else { f64::NAN }
}

/// RMS of the TIE-spectrum bins between two offsets, after a moving average one tenth of a decade wide,
/// with the periodic lines `pj_lines` marks left out. Seconds. A data record still returns the number.
pub fn phase_noise(trend: &[f32], period: f64, f_lo: f64, f_hi: f64) -> f64 {
    if trend.len() < 64 || !(period > 0.0) || !(f_hi > f_lo) {
        return f64::NAN;
    }
    let (df, spec) = tie_spectrum(trend, period);
    if !(df > 0.0) || spec.len() < 8 {
        return f64::NAN;
    }
    let marked: Vec<usize> = pj_lines(df, &spec, spec.len()).into_iter().map(|(f, _)| (f / df).round() as usize).collect();
    let (lo, hi) = (f_lo.min(f_hi), f_lo.max(f_hi));
    let mut acc = 0.0;
    let mut any = false;
    for k in 1..spec.len() {
        let f = k as f64 * df;
        if f < lo || f > hi || marked.contains(&k) {
            continue;
        }
        let lf = f.log10();
        let (mut s, mut c) = (0.0, 0usize);
        let (k0, k1) = (((k as f64) / 10f64.powf(0.05)).floor() as usize, ((k as f64) * 10f64.powf(0.05)).ceil() as usize);
        for j in k0.max(1)..=k1.min(spec.len() - 1) {
            if ((j as f64 * df).log10() - lf).abs() <= 0.05 {
                s += spec[j];
                c += 1;
            }
        }
        let v = if c == 0 { spec[k] } else { s / c as f64 };
        acc += v * v;
        any = true;
    }
    if any { acc.sqrt() } else { f64::NAN }
}

/// A record whose edges are one unit apart is a clock. Phase noise of anything else is still a number, and the page says so.
pub fn is_clock(c: &Clock) -> bool {
    if c.edges.len() < 8 || !(c.period > 0.0) {
        return false;
    }
    let n = c.edges.windows(2).filter(|w| ((w[1] - w[0]) / c.period - 1.0).abs() < 0.25).count();
    n * 5 >= 4 * (c.edges.len() - 1)
}

/// Dual-Dirac DJ left after the periodic lines and the DCD / DDJ means are subtracted. Empty below `TAIL_EDGES`.
pub fn npj(tie: &[f64], c: &Clock, trend: &[f32], means: &EdgeMeans) -> f64 {
    if tie.len() < TAIL_EDGES || !(c.period > 0.0) || c.edges.len() < 2 {
        return f64::NAN;
    }
    let (df, spec) = tie_spectrum(trend, c.period);
    let lines = pj_lines(df, &spec, 16);
    let mut rest = tie.to_vec();
    let u0 = c.ui_at(c.edges[0] - tie.first().copied().unwrap_or(0.0)).round();
    for (f, _) in &lines {
        let series = tone_fit(trend, c.period, *f).2;
        for i in 0..rest.len().min(c.edges.len()) {
            let u = c.ui_at(c.edges[i] - tie[i]).round() - u0;
            if u >= 0.0 {
                if let Some(&s) = series.get(u as usize) {
                    rest[i] -= s;
                }
            }
        }
    }
    for i in 1..rest.len().min(c.edges.len()) {
        let rising = ((i - 1) % 2 == 0) != c.first_rising;
        let run = ((c.edges[i] - c.edges[i - 1]) / c.period).round().max(1.0) as usize;
        if let Some(d) = means.edge_offset(rising, run) {
            rest[i] -= d;
        }
    }
    jitter(&rest).dj_pp
}

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
    if lo.is_finite() { hi - lo } else { f64::NAN }
}

fn pj_rss(trend: &[f32], period: f64) -> f64 {
    if trend.len() < 8 || !(period > 0.0) {
        return f64::NAN;
    }
    let (df, spec) = tie_spectrum(trend, period);
    let lines = pj_lines(df, &spec, 8);
    if lines.is_empty() { 0.0 } else { lines.iter().map(|l| l.1 * l.1).sum::<f64>().sqrt() }
}

/// Correlated rail lines and the notch frequency are subtracted as fitted sines from the TIE trend.
/// `rail` is (samples, dt) with its mean removed here. `wave` is the victim record, used to remeasure the eye
/// when a line was removed. `height` and `width` are the eye figures before the correction.
pub fn psij(trend: &[f32], c: &Clock, tie: &[f64], rail: Option<(&[f32], f64)>, notch: f64, height: f64, width: f64, wave: Option<(&[f32], f64)>) -> Psij {
    let before = PsijSide { tie_pp: tie_pp(tie), pj: pj_rss(trend, c.period), height, width };
    let mut after = before;
    if trend.len() < 8 || !(c.period > 0.0) {
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

/// Mean of (a + b) / 2 over the finite pairs. The two records already share a sample grid.
pub fn dc_common(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let mut s = 0.0;
    let mut c = 0usize;
    for k in 0..n {
        if a[k].is_finite() && b[k].is_finite() {
            s += 0.5 * (a[k] as f64 + b[k] as f64);
            c += 1;
        }
    }
    if c == 0 { f64::NAN } else { s / c as f64 }
}

/// Peak-to-peak of the common mode. `b` none uses `a` alone. `hipass` applies the 1-pole 30 kHz high-pass.
pub fn ac_common_pp(a: &[f32], b: Option<&[f32]>, dt: f64, hipass: bool) -> f64 {
    let cm: Vec<f32> = match b {
        Some(b) => {
            let n = a.len().min(b.len());
            (0..n).map(|k| if a[k].is_finite() && b[k].is_finite() { 0.5 * (a[k] + b[k]) } else { f32::NAN }).collect()
        }
        None => a.to_vec(),
    };
    let w = if hipass && dt > 0.0 { crate::math::unary(crate::math::Op1::Highpass(30e3), &cm, dt) } else { cm };
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &x in &w {
        if x.is_finite() {
            lo = lo.min(x as f64);
            hi = hi.max(x as f64);
        }
    }
    if lo.is_finite() { hi - lo } else { f64::NAN }
}

/// Mean voltage where the two traces cross.
pub fn diff_crossover(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let mut s = Vec::new();
    for k in 1..n {
        let (p, q) = (a[k - 1] as f64, a[k] as f64);
        let (bp, bq) = (b[k - 1] as f64, b[k] as f64);
        let (d0, d1) = (p - bp, q - bq);
        if !(d0.is_finite() && d1.is_finite()) {
            continue;
        }
        if d0 == 0.0 {
            s.push(p);
        } else if d0 * d1 < 0.0 && d1 != d0 {
            let u = d0 / (d0 - d1);
            s.push(p + u * (q - p));
        }
    }
    if s.is_empty() { f64::NAN } else { s.iter().sum::<f64>() / s.len() as f64 }
}

/// Mean 20 log10 of |midpoint of a non-transition bit| / |midpoint of the nearest preceding transition bit|.
pub fn tnt_ratio(v: &[f32], dt: f64, c: &Clock) -> f64 {
    if !(c.period > 0.0) || !(dt > 0.0) || v.len() < 2 || c.edges.is_empty() {
        return f64::NAN;
    }
    let at = |t: f64| -> f64 {
        let x = t / dt;
        if x < 0.0 || x > (v.len() - 1) as f64 {
            return f64::NAN;
        }
        let i = x.floor() as usize;
        let f = x - i as f64;
        let (a, b) = (v[i] as f64, v[(i + 1).min(v.len() - 1)] as f64);
        if a.is_finite() && b.is_finite() { a + f * (b - a) } else { f64::NAN }
    };
    let t_of = |u: i64| -> f64 {
        if c.ticks.len() >= 2 {
            let i = u.clamp(0, c.ticks.len() as i64 - 2) as usize;
            0.5 * (c.ticks[i] + c.ticks[(i + 1).min(c.ticks.len() - 1)])
        } else {
            c.phase + (u as f64 + 0.5) * c.period
        }
    };
    let mut occupied = Vec::new();
    for &e in &c.edges {
        occupied.push(c.ui_at(e).floor() as i64);
    }
    occupied.sort_unstable();
    occupied.dedup();
    let (u0, u1) = (*occupied.first().unwrap(), *occupied.last().unwrap());
    let mut last = f64::NAN;
    let mut ratios = Vec::new();
    for u in u0..=u1 {
        let volt = at(t_of(u));
        let trans = occupied.binary_search(&u).is_ok();
        if trans {
            if volt.is_finite() {
                last = volt;
            }
        } else if last.is_finite() && last != 0.0 && volt.is_finite() {
            ratios.push(20.0 * (volt.abs() / last.abs()).log10());
        }
    }
    if ratios.is_empty() { f64::NAN } else { ratios.iter().sum::<f64>() / ratios.len() as f64 }
}

/// Local unit rates between equal-polarity transitions, with their midpoint times.
/// NRZ run lengths are integer UIs: counting them prevents a 3-bit run from being
/// misread as a third of the clock rate. As with clock recovery, this assumes the
/// local timing error is small enough to identify the nearest integer UI count.
fn same_edge_rates(c: &Clock) -> Vec<(f64, f64)> {
    if !c.period.is_finite() || c.period <= 0.0 {
        return Vec::new();
    }
    let mut series = Vec::new();
    for i in (0..c.edges.len().saturating_sub(2)).step_by(2) {
        let (a, b) = (c.edges[i], c.edges[i + 2]);
        let elapsed = b - a;
        if a.is_finite() && b.is_finite() && elapsed > 0.0 {
            let units = (elapsed / c.period).round().max(1.0);
            let rate = units / elapsed;
            if rate.is_finite() {
                series.push((a + 0.5 * elapsed, rate));
            }
        }
    }
    series
}

/// Peak-to-peak local frequency deviation, parts per million of its measured
/// mean, and the per-cycle series in ppm. The recovered UI is used to count NRZ
/// run lengths; a hinted UI rate twice a clock's cycle rate does not halve depth.
/// Other per-cycle timing jitter is included in this frequency-deviation span.
pub fn ssc_deviation(c: &Clock) -> (f64, Vec<f64>) {
    let series = same_edge_rates(c);
    if series.len() < 2 {
        return (f64::NAN, Vec::new());
    }
    let mean = series.iter().map(|(_, f)| f).sum::<f64>() / series.len() as f64;
    if !mean.is_finite() || mean <= 0.0 {
        return (f64::NAN, Vec::new());
    }
    let ppm: Vec<f64> = series.iter().map(|(_, f)| (f - mean) / mean * 1e6).collect();
    let (lo, hi) = ppm.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    (hi - lo, ppm)
}

/// Strongest non-DC line of the local frequency series, hertz. Equal-polarity
/// NRZ intervals are nonuniform in time, so their rates are linearly interpolated
/// at uniform midpoint times before the FFT. This is an estimate when transitions
/// are sparse; the UI period is never substituted for the actual elapsed time.
pub fn ssc_rate(c: &Clock) -> f64 {
    let series = same_edge_rates(c);
    if series.len() < 16 {
        return f64::NAN;
    }
    let mean = series.iter().map(|(_, f)| f).sum::<f64>() / series.len() as f64;
    let (low, high) = series.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), (_, f)| (low.min(*f), high.max(*f)));
    // Constant-rate edges still have floating-point subtraction noise. They do
    // not contain an observable SSC modulation line.
    if !mean.is_finite() || mean <= 0.0 || high - low <= mean * 1e-10 {
        return f64::NAN;
    }
    let first = series[0].0;
    let last = series[series.len() - 1].0;
    let dt = (last - first) / (series.len() - 1) as f64;
    if !dt.is_finite() || dt <= 0.0 {
        return f64::NAN;
    }
    let mut v = Vec::with_capacity(series.len());
    let mut j = 0;
    for i in 0..series.len() {
        let t = first + i as f64 * dt;
        while j + 1 < series.len() - 1 && series[j + 1].0 < t {
            j += 1;
        }
        let (ta, fa) = series[j];
        let (tb, fb) = series[j + 1];
        let fraction = ((t - ta) / (tb - ta)).clamp(0.0, 1.0);
        // Remove the large carrier before f32 conversion so a small FM signal
        // is not quantized at the much larger nominal unit rate.
        v.push((fa + fraction * (fb - fa) - mean) as f32);
    }
    let dc = v.iter().map(|f| *f as f64).sum::<f64>() / v.len() as f64;
    for f in &mut v {
        *f = (*f as f64 - dc) as f32;
    }
    let (df, spectrum) = crate::math::spectrum(&v, dt, crate::math::Window::Hann);
    spectrum.iter().enumerate().skip(1).filter(|(_, magnitude)| **magnitude > 0.0)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(bin, _)| bin as f64 * df).filter(|f| *f > 0.0).unwrap_or(f64::NAN)
}
