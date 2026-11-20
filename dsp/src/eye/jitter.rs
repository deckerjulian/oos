// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Time interval error and the jitter model: TIE against a `Clock`, its trend and spectrum, RJ/DJ separation with
//! the dual-Dirac tail fit, DCD and DDJ from conditional edge means, TJ at a BER and the bathtub curve. A new jitter
//! figure that needs only the TIE goes here; one built on the TIE spectrum goes into `periodic`.

use super::tails::fit_tails;
use super::{is_positive, is_probability, norm_inv, Clock};

/// Q of the Gaussian tail at BER 1e-12 (TJ = DJ + 2 Q RJ).
pub const Q_BER_1E12: f64 = 7.034;

/// Edges needed for any jitter model.
pub const MIN_EDGES: usize = 8;
/// Edges needed for the tail fit: below this RJ scatters by about 40 %, above it by about 20 % (Gaussian test data).
pub const TAIL_EDGES: usize = 1024;
/// A TIE RMS below this (s) is the rounding of the edge times, not jitter.
const TIE_FLOOR: f64 = 1e-15;

/// Time interval error of every edge: edge time minus the nearest ideal clock edge (s).
pub fn tie(edges: &[f64], c: &Clock) -> Vec<f64> {
    if !is_positive(c.period) {
        return Vec::new();
    }
    edges.iter().map(|&t| t - c.nearest_tick(t)).collect()
}

/// The most unit intervals a TIE trend holds: more than the deepest record has samples.
const TREND_MAX: i64 = 1 << 26;

/// TIE trend: the TIE linearly interpolated onto every UI between the first and the last edge (one value per UI, s);
/// the input of `tie_spectrum`. Empty for a clock with more than `TREND_MAX` unit intervals between those edges.
pub fn tie_trend(edges: &[f64], tie: &[f64], c: &Clock) -> Vec<f32> {
    let pts: Vec<(i64, f64)> = edges.iter().zip(tie).map(|(&t, &e)| (c.ui_at(t - e).round() as i64, e)).collect();
    // one value per unit interval: a clock from edge times alone (`clock_from_edges`) can have more unit intervals
    // between its edges than any record has samples. Such a clock has no trend, not one of gigabytes
    if pts.first().zip(pts.last()).is_some_and(|(a, b)| b.0.saturating_sub(a.0) > TREND_MAX) {
        return Vec::new();
    }
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
fn rms_dev(x: &[f64]) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    let m = x.iter().sum::<f64>() / x.len() as f64;
    (x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
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
    let mut j = Jitter {
        fit: JitterFit::TooFew,
        tj_pp: f64::NAN,
        rj_rms: f64::NAN,
        dj_pp: f64::NAN,
        left: (f64::NAN, f64::NAN),
        right: (f64::NAN, f64::NAN),
        pp: f64::NAN,
        rms: rms_dev(&t),
        period_jitter_rms: f64::NAN,
        c2c_rms: f64::NAN,
        histogram: Vec::new(),
    };
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
        if !is_positive(c.period) {
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
    if !is_positive(period) || s.is_empty() || points < 2 {
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
            // NaN counts as 1, as before the clamp
            let p = if p.is_nan() { 1.0 } else { p.clamp(1e-300, 1.0) };
            b.model.push((p.log10() as f32).max(BATHTUB_FLOOR));
        }
    }
    b.seen = (1.0 - j.pp / period).max(0.0);
    if modelled {
        b.opening = (1.0 - j.tj_pp / period).max(0.0);
    }
    b
}
/// Total jitter at a bit-error rate: DJ + 2 Q RJ, with Q = `norm_inv(1 − ber)`.
pub fn tj_at(j: &Jitter, ber: f64) -> f64 {
    if !is_probability(ber) || !j.dj_pp.is_finite() || !j.rj_rms.is_finite() {
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
    if !tj.is_finite() || !is_positive(period) {
        return f64::NAN;
    }
    (1.0 - tj / period).max(0.0)
}
