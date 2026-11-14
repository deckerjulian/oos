// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Frequency response (Bode plot) helpers: sine fit at a known frequency, gain and phase of a DUT output against its
//! input, and the logarithmic sweep plan. `sweep_response` reads a whole sweep of a generator that sweeps on its own
//! (free sweep) from one record.

use std::f64::consts::PI;

/// Least-squares fit of `a sin(wt) + b cos(wt) + c` at frequency `f` over whole periods of `v` (sample period `dt`):
/// returns (amplitude, phase in radians, offset). None if fewer than one period or no samples.
pub fn sine_fit(v: &[f32], dt: f64, f: f64) -> Option<(f64, f64, f64)> {
    if f <= 0.0 || dt <= 0.0 || v.len() < 8 {
        return None;
    }
    let per = 1.0 / (f * dt); // samples per period
    let periods = (v.len() as f64 / per).floor();
    if periods < 1.0 {
        return None;
    }
    let n = (periods * per).floor() as usize;
    let w = 2.0 * std::f64::consts::PI * f * dt;
    // normal equations of [sin, cos, 1]
    let (mut ss, mut sc, mut s1, mut cc, mut c1, mut nn) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut ys, mut yc, mut y1) = (0.0, 0.0, 0.0);
    for (k, &y) in v[..n].iter().enumerate() {
        if !y.is_finite() {
            continue;
        }
        let (s, c) = (w * k as f64).sin_cos();
        let y = y as f64;
        ss += s * s;
        sc += s * c;
        s1 += s;
        cc += c * c;
        c1 += c;
        nn += 1.0;
        ys += y * s;
        yc += y * c;
        y1 += y;
    }
    let [a, b, c] = solve3([[ss, sc, s1], [sc, cc, c1], [s1, c1, nn]], [ys, yc, y1])?;
    Some(((a * a + b * b).sqrt(), b.atan2(a), c))
}

/// Solves the 3x3 system `m x = r` by Cramer's rule; None if it is singular.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let col = |i: usize| {
        let mut x = m;
        for (row, v) in r.iter().enumerate() {
            x[row][i] = *v;
        }
        det(x) / d
    };
    Some([col(0), col(1), col(2)])
}

fn wrap_deg(mut ph: f64) -> f64 {
    while ph > 180.0 {
        ph -= 360.0;
    }
    while ph <= -180.0 {
        ph += 360.0;
    }
    ph
}

/// Gain (dB) and phase (degrees, -180..180, output relative to input) of `out` against `inp` at `f`.
pub fn gain_phase(inp: &[f32], out: &[f32], dt: f64, f: f64) -> Option<(f64, f64)> {
    let (ai, pi, _) = sine_fit(inp, dt, f)?;
    let (ao, po, _) = sine_fit(out, dt, f)?;
    if ai <= 0.0 {
        return None;
    }
    Some((20.0 * (ao / ai).log10(), wrap_deg((po - pi).to_degrees())))
}

/// What a Bode sweep stores in the middle field of each point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Control loop: gain in dB, `20 log10(Vout / Vin)`.
    Loop,
    /// Power-supply rejection: `20 log10(Vin / Vout)`, the negation of the control-loop gain. Phase is kept so the
    /// point is still (Hz, dB, degrees).
    Psrr,
    /// Impedance: the linear ratio `Vout / Vin`, not ohms. Phase is computed the same way as the control loop.
    /// `:BODE:DATA?` documents the middle field as dB; in this mode that field is the linear ratio.
    Z,
}

/// One frequency of a sweep in `mode`: (middle field, phase degrees). The middle field is dB for the loop and for
/// PSRR, and the linear Vout/Vin ratio for impedance.
pub fn response(mode: Mode, inp: &[f32], out: &[f32], dt: f64, f: f64) -> Option<(f64, f64)> {
    match mode {
        Mode::Loop => gain_phase(inp, out, dt, f),
        Mode::Psrr => gain_phase(inp, out, dt, f).map(|(g, p)| (-g, p)),
        Mode::Z => {
            let (ai, pi, _) = sine_fit(inp, dt, f)?;
            let (ao, po, _) = sine_fit(out, dt, f)?;
            if ai <= 0.0 {
                return None;
            }
            Some((ao / ai, wrap_deg((po - pi).to_degrees())))
        }
    }
}

/// `n` logarithmically spaced frequencies from `start` to `stop` (both included).
pub fn log_sweep(start: f64, stop: f64, n: usize) -> Vec<f64> {
    if n < 2 || start <= 0.0 || stop <= start {
        return vec![start.max(stop)];
    }
    let (l0, l1) = (start.ln(), stop.ln());
    let mut v: Vec<f64> = (0..n).map(|k| (l0 + (l1 - l0) * k as f64 / (n - 1) as f64).exp()).collect();
    (v[0], v[n - 1]) = (start, stop);
    v
}

/// A frequency with 6 significant digits, no trailing zeros (for generator commands).
pub fn fmt_hz(f: f64) -> String {
    let dec = (5 - f.abs().max(1e-9).log10().floor() as i32).max(0) as usize;
    let s = format!("{:.*}", dec, f);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Time per division that shows about `periods` periods of `f` on 10 divisions.
pub fn tdiv_for(f: f64, periods: f64) -> f64 {
    periods / f / 10.0
}

/// Stability margins of a sweep, each as (value, frequency).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margins {
    /// dB below 0 where the phase passes -180 degrees
    pub gain: Option<(f64, f64)>,
    /// degrees above -180 where the gain falls through 0 dB
    pub phase: Option<(f64, f64)>,
}

/// Gain and phase margin of an open loop measured as (Hz, dB, degrees), rising in frequency. The phase comes wrapped
/// to +-180 and is unwrapped first; points without a phase are left out. Crossings are interpolated on log frequency.
pub fn margins(points: &[(f64, f64, f64)]) -> Margins {
    let mut u: Vec<(f64, f64, f64)> = Vec::with_capacity(points.len());
    for &(f, g, p) in points.iter().filter(|p| p.0 > 0.0 && p.1.is_finite() && p.2.is_finite()) {
        let p = match u.last() {
            Some(l) => p + 360.0 * ((l.2 - p) / 360.0).round(),
            None => p,
        };
        u.push((f, g, p));
    }
    let mut m = Margins::default();
    for w in u.windows(2) {
        let (a, b) = (w[0], w[1]);
        let at = |x: f64, y0: f64, y1: f64| {
            let r = (x - y0) / (y1 - y0);
            (r, (a.0.ln() + r * (b.0.ln() - a.0.ln())).exp())
        };
        if m.phase.is_none() && a.1 >= 0.0 && b.1 < 0.0 {
            let (r, f) = at(0.0, a.1, b.1);
            m.phase = Some((180.0 + a.2 + r * (b.2 - a.2), f));
        }
        if m.gain.is_none() && a.2 > -180.0 && b.2 <= -180.0 {
            let (r, f) = at(-180.0, a.2, b.2);
            m.gain = Some((-(a.1 + r * (b.1 - a.1)), f));
        }
    }
    m
}

// ---------------------------------------------------------------------------------------------- free sweep

/// A sweep that an external generator runs on its own (the scope does not set it): start and stop frequency (Hz),
/// sweep time (s), logarithmic or linear. Time 0 is the sync edge at the start of the sweep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    pub start: f64,
    pub stop: f64,
    pub time: f64,
    pub log: bool,
}

impl Sweep {
    /// Start above 0 and below stop, a sweep time above 0.
    pub fn valid(&self) -> bool {
        self.start > 0.0 && self.stop > self.start && self.stop.is_finite() && self.time > 0.0 && self.time.is_finite()
    }

    /// Frequency `t` seconds after the start: f0 + (f1 - f0) t / T, or f0 (f1 / f0)^(t / T).
    pub fn freq_at(&self, t: f64) -> f64 {
        let x = t / self.time;
        if self.log {
            self.start * (self.stop / self.start).powf(x)
        } else {
            self.start + (self.stop - self.start) * x
        }
    }

    /// Time after the start at which the sweep passes `f`.
    pub fn time_of(&self, f: f64) -> f64 {
        if self.log {
            self.time * (f / self.start).ln() / (self.stop / self.start).ln()
        } else {
            self.time * (f - self.start) / (self.stop - self.start)
        }
    }

    /// Phase (radians) at `t`: 2 pi times the integral of the frequency from the start.
    pub fn phase_at(&self, t: f64) -> f64 {
        if self.log {
            let k = (self.stop / self.start).ln() / self.time;
            2.0 * PI * self.start * (k * t).exp_m1() / k
        } else {
            2.0 * PI * (self.start * t + 0.5 * (self.stop - self.start) / self.time * t * t)
        }
    }

    /// Half the length of a window centred at `t` whose last frequency is `ratio` times its first.
    fn half_span(&self, t: f64, ratio: f64) -> f64 {
        if self.log {
            self.time * ratio.ln() / (2.0 * (self.stop / self.start).ln())
        } else {
            self.freq_at(t) * (ratio - 1.0) / (ratio + 1.0) * self.time / (self.stop - self.start)
        }
    }
}

/// One record of a sweep: the output channel, the input channel (None: no input, the caller compares the output
/// with a level), the sample period, the time of sample 0 after the sweep start, and whether the samples are min/max
/// pairs (records of slow time bases) rather than plain samples.
pub struct SweepRecord<'a> {
    pub out: &'a [f32],
    pub inp: Option<&'a [f32]>,
    pub dt: f64,
    pub t0: f64,
    pub envelope: bool,
}

/// One frequency of a free sweep: frequency (Hz), amplitude (V peak) of the output and of the input, phase of the
/// output against the input (degrees, -180..180).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepPoint {
    pub f: f64,
    pub out: f64,
    pub inp: Option<f64>,
    pub phase: Option<f64>,
}

/// The points of a free sweep, and how many frequencies were left out for which reason.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SweepResult {
    pub points: Vec<SweepPoint>,
    /// the sweep passes the frequency so fast that a window holds less than one period
    pub too_fast: usize,
    /// fewer than 3 samples per period, or a window of fewer than 8 samples
    pub too_high: usize,
    /// the window is not inside the record, or holds no signal
    pub outside: usize,
}

/// Window length in periods of the local frequency.
pub const SWEEP_PERIODS: f64 = 4.0;
/// Highest frequency of a window over its lowest: limits the window where the sweep is fast for the frequency.
pub const SWEEP_RATIO: f64 = 1.25;
/// Least periods in a window.
pub const SWEEP_MIN_PERIODS: f64 = 1.0;
/// Least samples per period of plain samples.
const SWEEP_MIN_SPP: f64 = 3.0;
/// Samples per period the fit takes at most: a long window uses every k-th sample.
const SWEEP_FIT_SPP: f64 = 64.0;
/// Least share of the signal power that the fitted sine explains, in both channels, for a phase.
const SWEEP_PHASE_R2: f64 = 0.8;

/// The start of the sweep in a record: the edge of the sync signal `v` (sample 0 at `t0` s from the trigger point,
/// period `dt`) through `level` (hysteresis `hyst`, total width) in the set direction. Of the edges after which the
/// whole sweep (`time`) is inside the record, the one nearest the trigger point; without one, the edge nearest the
/// trigger point. Returns its time from the trigger point, None without an edge.
pub fn sync_start(v: &[f32], dt: f64, t0: f64, level: f32, hyst: f32, rising: bool, time: f64) -> Option<f64> {
    let end = t0 + v.len().saturating_sub(1) as f64 * dt;
    let ts: Vec<f64> = crate::edges::edges(v, level, hyst).into_iter().filter(|e| e.rising == rising).map(|e| t0 + e.t * dt).collect();
    let near = |a: &&f64, b: &&f64| a.abs().total_cmp(&b.abs());
    ts.iter().filter(|&&t| t + time <= end + dt).min_by(near).or_else(|| ts.iter().min_by(near)).copied()
}

/// Amplitude and phase over frequency from one record of a free sweep, at `n` frequencies spaced logarithmically from
/// start to stop.
///
/// Each frequency f has a window around the time the sweep passes it: `SWEEP_PERIODS` periods long, shorter where the
/// frequency changes by more than `SWEEP_RATIO` within it, moved inwards at the ends of the sweep. Plain samples:
/// least-squares fit of a sin(p(t)) + b cos(p(t)) + c, where p(t) is the phase of the sweep itself, so the frequency
/// change inside the window costs nothing; amplitude sqrt(a^2 + b^2), phase atan2(b, a). The phase of the output
/// against the input is given where both fits explain at least 80 % of the signal power. Min/max pairs: half the
/// peak-to-peak value in the window, no phase. Each point carries the frequency at the middle of its window.
pub fn sweep_response(sw: &Sweep, rec: &SweepRecord, n: usize) -> SweepResult {
    let mut r = SweepResult::default();
    let len = rec.out.len().min(rec.inp.map_or(usize::MAX, |v| v.len()));
    if !sw.valid() || rec.dt <= 0.0 || len < 8 || n == 0 {
        return r;
    }
    let (rec_a, rec_b) = (rec.t0, rec.t0 + (len - 1) as f64 * rec.dt);
    for fc in log_sweep(sw.start, sw.stop, n) {
        let tc = sw.time_of(fc);
        let mut h = SWEEP_PERIODS / (2.0 * fc);
        if rec.envelope {
            // min/max pairs: a few of them at least, the phase does not matter
            h = h.max(8.0 * rec.dt);
        }
        let h = h.min(sw.half_span(tc, SWEEP_RATIO));
        // inside the sweep: a window at one of its ends moves inwards
        let (mut a, mut b) = (tc - h, tc + h);
        if a < 0.0 {
            (a, b) = (0.0, (2.0 * h).min(sw.time));
        }
        if b > sw.time {
            (a, b) = ((sw.time - 2.0 * h).max(0.0), sw.time);
        }
        if (b - a) * sw.freq_at(0.5 * (a + b)) < SWEEP_MIN_PERIODS {
            r.too_fast += 1;
            continue;
        }
        let (a, b) = (a.max(rec_a), b.min(rec_b));
        let fm = sw.freq_at(0.5 * (a + b));
        if b <= a || (b - a) * fm < SWEEP_MIN_PERIODS {
            r.outside += 1;
            continue;
        }
        if !rec.envelope && 1.0 / (sw.freq_at(b) * rec.dt) < SWEEP_MIN_SPP {
            r.too_high += 1;
            continue;
        }
        let i0 = ((a - rec.t0) / rec.dt).ceil().max(0.0) as usize;
        let i1 = (((b - rec.t0) / rec.dt).floor().max(0.0) as usize).min(len - 1);
        if i1 < i0 + 8 {
            r.too_high += 1;
            continue;
        }
        // the windows at the ends of the sweep can come out the same
        if r.points.last().is_some_and(|p| fm <= p.f * 1.0005) {
            continue;
        }
        let pt = if rec.envelope { envelope_point(rec, i0, i1, fm) } else { fit_point(sw, rec, i0, i1, fm, 0.5 * (a + b)) };
        match pt {
            Some(p) => r.points.push(p),
            None => r.outside += 1,
        }
    }
    r
}

/// Half the peak-to-peak value of the output and the input in samples i0..=i1.
fn envelope_point(rec: &SweepRecord, i0: usize, i1: usize, f: f64) -> Option<SweepPoint> {
    let half = |v: &[f32]| {
        let (lo, hi) = v[i0..=i1].iter().filter(|x| x.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), &x| (l.min(x), h.max(x)));
        (hi > lo).then_some(0.5 * (hi - lo) as f64)
    };
    let out = half(rec.out)?;
    let inp = match rec.inp {
        Some(v) => Some(half(v)?),
        None => None,
    };
    Some(SweepPoint { f, out, inp, phase: None })
}

/// Fit of a sin(p) + b cos(p) + c on the sweep's own phase p over samples i0..=i1 of the output and the input.
fn fit_point(sw: &Sweep, rec: &SweepRecord, i0: usize, i1: usize, f: f64, tm: f64) -> Option<SweepPoint> {
    let step = ((1.0 / (f * rec.dt) / SWEEP_FIT_SPP).floor() as usize).max(1);
    // the phase from the middle of the window: small numbers for sin and cos
    let pm = sw.phase_at(tm);
    let mut m = [[0.0f64; 3]; 3];
    // per channel: y sin, y cos, y, y^2
    let mut acc = [[0.0f64; 4]; 2];
    for k in (i0..=i1).step_by(step) {
        let yo = rec.out[k];
        let yi = rec.inp.map_or(0.0, |v| v[k]);
        if !yo.is_finite() || !yi.is_finite() {
            continue;
        }
        let (s, c) = (sw.phase_at(rec.t0 + k as f64 * rec.dt) - pm).sin_cos();
        let row = [s, c, 1.0];
        for (i, ri) in row.iter().enumerate() {
            for (j, rj) in row.iter().enumerate() {
                m[i][j] += ri * rj;
            }
        }
        for (a, y) in acc.iter_mut().zip([yo as f64, yi as f64]) {
            a[0] += y * s;
            a[1] += y * c;
            a[2] += y;
            a[3] += y * y;
        }
    }
    let nn = m[2][2];
    if nn < 8.0 {
        return None;
    }
    // amplitude, phase and the share of the AC power the sine explains
    let fit = |a: &[f64; 4]| {
        let x = solve3(m, [a[0], a[1], a[2]])?;
        let ss_res = a[3] - (x[0] * a[0] + x[1] * a[1] + x[2] * a[2]);
        let ss_ac = a[3] - a[2] * a[2] / nn;
        let r2 = if ss_ac > 0.0 { 1.0 - ss_res / ss_ac } else { 0.0 };
        Some(((x[0] * x[0] + x[1] * x[1]).sqrt(), x[1].atan2(x[0]), r2))
    };
    let (ao, po, ro) = fit(&acc[0])?;
    if ao.is_nan() || ao <= 0.0 {
        return None;
    }
    let (inp, phase) = match rec.inp {
        Some(_) => {
            let (ai, pi, ri) = fit(&acc[1])?;
            if ai.is_nan() || ai <= 0.0 {
                return None;
            }
            let ph = (ro >= SWEEP_PHASE_R2 && ri >= SWEEP_PHASE_R2).then(|| ((po - pi).to_degrees() + 180.0).rem_euclid(360.0) - 180.0);
            (Some(ai), ph)
        }
        None => (None, None),
    };
    Some(SweepPoint { f, out: ao, inp, phase })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_of_a_third_order_loop() {
        // three real poles at 1 kHz, gain 20: phase -180 at sqrt(3) kHz where the gain is 20/8 (+8 dB, unstable by 8 dB)
        let pts: Vec<(f64, f64, f64)> = log_sweep(10.0, 100e3, 400)
            .into_iter()
            .map(|f| {
                let x = f / 1e3;
                let g = 20.0 * (20.0 / (1.0 + x * x).powf(1.5)).log10();
                let p = -3.0 * x.atan().to_degrees();
                // wrapped as the sweep reports it
                (f, g, (p + 180.0).rem_euclid(360.0) - 180.0)
            })
            .collect();
        let m = margins(&pts);
        let (gm, fg) = m.gain.unwrap();
        assert!((gm + 7.96).abs() < 0.1, "{}", gm);
        assert!((fg / 1732.0 - 1.0).abs() < 0.02, "{}", fg);
        let (pm, fp) = m.phase.unwrap();
        assert!((fp / 2522.0 - 1.0).abs() < 0.03, "{}", fp);
        assert!((pm - (180.0 - 3.0 * (fp / 1e3).atan().to_degrees())).abs() < 2.0, "{}", pm);
        assert_eq!(margins(&[(1.0, -3.0, 0.0), (10.0, -6.0, -10.0)]), Margins::default());
        assert_eq!(margins(&[]), Margins::default());
    }

    fn sine(f: f64, amp: f64, ph: f64, off: f64, dt: f64, n: usize) -> Vec<f32> {
        (0..n).map(|k| (amp * (2.0 * PI * f * k as f64 * dt + ph).sin() + off) as f32).collect()
    }

    #[test]
    fn fit_and_gain_phase() {
        let dt = 1e-6;
        let v = sine(1234.0, 0.7, 0.3, 0.1, dt, 20_000);
        let (a, p, c) = sine_fit(&v, dt, 1234.0).unwrap();
        assert!((a - 0.7).abs() < 1e-4 && (p - 0.3).abs() < 1e-4 && (c - 0.1).abs() < 1e-4, "{} {} {}", a, p, c);
        // an RC low-pass at its corner: -3.01 dB, -45 degrees
        let inp = sine(1000.0, 1.0, 0.0, 0.0, dt, 50_000);
        let out = sine(1000.0, 1.0 / 2f64.sqrt(), -PI / 4.0, 0.0, dt, 50_000);
        let (g, ph) = gain_phase(&inp, &out, dt, 1000.0).unwrap();
        assert!((g + 3.0103).abs() < 1e-3 && (ph + 45.0).abs() < 1e-3, "{} {}", g, ph);
        // with noise and a large offset the fit still finds the line
        let mut noisy = sine(5000.0, 0.01, 1.0, 2.0, dt, 40_000);
        for (k, x) in noisy.iter_mut().enumerate() {
            *x += 0.005 * (((k * 7919) % 101) as f32 / 50.0 - 1.0);
        }
        let (a, _, _) = sine_fit(&noisy, dt, 5000.0).unwrap();
        assert!((a - 0.01).abs() < 5e-4, "{}", a);
        assert!(sine_fit(&v[..100], dt, 1234.0).is_none());
    }

    #[test]
    fn sweep_plan() {
        let f = log_sweep(10.0, 1e6, 6);
        assert_eq!(f.len(), 6);
        for (k, want) in [10.0, 100.0, 1e3, 1e4, 1e5, 1e6].iter().enumerate() {
            assert!((f[k] / want - 1.0).abs() < 1e-9);
        }
        assert!((tdiv_for(1e3, 5.0) - 5e-4).abs() < 1e-12);
        assert_eq!((fmt_hz(999.9999999999998), fmt_hz(3162.2776601683804), fmt_hz(1e5), fmt_hz(12.5)), ("1000".into(), "3162.28".into(), "100000".into(), "12.5".into()));
    }

    /// A generator running `sw` (amplitude 1 V peak, start phase 0.7 rad; the start frequency before the sweep, the
    /// stop frequency after it), a first-order low-pass at `fc` with gain `g` behind it, and the sync pulse (3.3 V,
    /// 1 ms) at the start. The record runs from -10 % to 110 % of the sweep at `fs`, with 1 mV of noise.
    /// Returns (input, output, sync, time of sample 0, the low-pass's true response at f as (dB, degrees)).
    #[allow(clippy::type_complexity)]
    fn free_sweep_bench(sw: &Sweep, fs: f64, fc: f64, g: f64) -> (Vec<f32>, Vec<f32>, Vec<f32>, f64, Box<dyn Fn(f64) -> (f64, f64)>) {
        let dt = 1.0 / fs;
        let t0 = -0.1 * sw.time;
        let n = (1.2 * sw.time * fs) as usize;
        let s = *sw;
        let phase = move |t: f64| {
            if t < 0.0 {
                2.0 * PI * s.start * t
            } else if t > s.time {
                s.phase_at(s.time) + 2.0 * PI * s.stop * (t - s.time)
            } else {
                s.phase_at(t)
            }
        };
        let mut x = crate::sim::sample(|t| (phase(t) + 0.7).sin(), t0, dt, n);
        // the low-pass of the step response sampled at dt: y += alpha (x - y)
        let alpha = 1.0 - (-2.0 * PI * fc * dt).exp();
        let mut y = 0.0f64;
        let mut out: Vec<f32> = x
            .iter()
            .map(|&v| {
                y += alpha * (v as f64 - y);
                (g * y) as f32
            })
            .collect();
        let mut sync = crate::sim::sample(|t| if (0.0..1e-3).contains(&t) { 3.3 } else { 0.0 }, t0, dt, n);
        crate::sim::add_noise(&mut x, 1e-3, 1);
        crate::sim::add_noise(&mut out, 1e-3, 2);
        crate::sim::add_noise(&mut sync, 1e-3, 3);
        // the exact response of that discrete filter: g alpha / (1 - (1 - alpha) e^-jw)
        let truth = move |f: f64| {
            let w = 2.0 * PI * f * dt;
            let (re, im) = (1.0 - (1.0 - alpha) * w.cos(), (1.0 - alpha) * w.sin());
            let mag = g * alpha / (re * re + im * im).sqrt();
            (20.0 * mag.log10(), -im.atan2(re).to_degrees())
        };
        (x, out, sync, t0, Box::new(truth))
    }

    /// Largest gain and phase error of a result against the truth (phase over the points that have one).
    fn free_sweep_errors(r: &SweepResult, truth: &dyn Fn(f64) -> (f64, f64), reference: Option<f64>) -> (f64, f64) {
        let (mut eg, mut ep) = (0.0f64, 0.0f64);
        for p in &r.points {
            let (tg, tp) = truth(p.f);
            let g = match (p.inp, reference) {
                (Some(i), _) => 20.0 * (p.out / i).log10(),
                (None, Some(a)) => 20.0 * (p.out / a).log10(),
                (None, None) => unreachable!(),
            };
            eg = eg.max((g - tg).abs());
            if let Some(ph) = p.phase {
                ep = ep.max((ph - tp).abs());
            }
        }
        (eg, ep)
    }

    #[test]
    fn free_log_sweep_through_a_low_pass() {
        let sw = Sweep { start: 20.0, stop: 20e3, time: 4.0, log: true };
        let fs = 250e3;
        let (x, y, sync, t0, truth) = free_sweep_bench(&sw, fs, 1e3, 2.0);
        let ts = sync_start(&sync, 1.0 / fs, t0, 1.5, 0.3, true, sw.time).unwrap();
        assert!(ts.abs() < 2.0 / fs, "{}", ts);
        let rec = SweepRecord { out: &y, inp: Some(&x), dt: 1.0 / fs, t0: t0 - ts, envelope: false };
        let r = sweep_response(&sw, &rec, 60);
        let (eg, ep) = free_sweep_errors(&r, &truth, None);
        println!("log sweep: {} points, gain error {:.3} dB, phase error {:.2} deg, left out {} {} {}", r.points.len(), eg, ep, r.too_fast, r.too_high, r.outside);
        assert!(r.points.len() >= 58 && r.too_fast + r.too_high + r.outside == 0, "{:?}", r);
        assert!(eg < 0.5 && ep < 3.0, "{} {}", eg, ep);
        assert!(r.points.iter().all(|p| p.phase.is_some()));
        assert!(r.points.first().unwrap().f < 25.0 && r.points.last().unwrap().f > 18e3);
        // no input: the output against the generator's 1 V peak
        let rec = SweepRecord { inp: None, ..rec };
        let r = sweep_response(&sw, &rec, 60);
        let (eg, _) = free_sweep_errors(&r, &truth, Some(1.0));
        println!("log sweep without input: {} points, gain error {:.3} dB", r.points.len(), eg);
        assert!(r.points.len() >= 58 && eg < 0.5 && r.points.iter().all(|p| p.phase.is_none()), "{}", eg);
        // a generator 0.2 % slower than set: the ratio of both channels does not care
        let slow = Sweep { time: sw.time * 1.002, ..sw };
        let (x, y, _, t0, truth) = free_sweep_bench(&slow, fs, 1e3, 2.0);
        let r = sweep_response(&sw, &SweepRecord { out: &y, inp: Some(&x), dt: 1.0 / fs, t0, envelope: false }, 60);
        let (eg, ep) = free_sweep_errors(&r, &truth, None);
        println!("log sweep, generator 0.2 % slow: gain error {:.3} dB, phase error {:.2} deg", eg, ep);
        assert!(r.points.len() >= 58 && eg < 0.5 && ep < 3.0, "{} {}", eg, ep);
    }

    #[test]
    fn free_lin_sweep_through_a_low_pass() {
        let sw = Sweep { start: 20.0, stop: 20e3, time: 10.0, log: false };
        let fs = 100e3;
        let (x, y, sync, t0, truth) = free_sweep_bench(&sw, fs, 1e3, 1.0);
        let ts = sync_start(&sync, 1.0 / fs, t0, 1.5, 0.3, true, sw.time).unwrap();
        let rec = SweepRecord { out: &y, inp: Some(&x), dt: 1.0 / fs, t0: t0 - ts, envelope: false };
        let r = sweep_response(&sw, &rec, 60);
        let (eg, ep) = free_sweep_errors(&r, &truth, None);
        println!("lin sweep: {} points from {:.0} Hz, gain error {:.3} dB, phase error {:.2} deg, too fast {}", r.points.len(), r.points[0].f, eg, ep, r.too_fast);
        // a linear sweep passes the low frequencies in a few periods: they are left out, the rest is right
        assert!(r.too_fast > 0 && r.points[0].f > 50.0 && r.points[0].f < 200.0, "{:?}", r.points[0]);
        assert!(r.points.len() + r.too_fast >= 58);
        assert!(eg < 0.5 && ep < 3.0, "{} {}", eg, ep);
    }

    #[test]
    fn free_sweep_on_min_max_pairs() {
        // a slow time base: min/max of every 200 samples at 2 MSa/s, as the record of a scan sweep
        let sw = Sweep { start: 20.0, stop: 20e3, time: 2.0, log: true };
        let fs = 2e6;
        let (x, y, sync, t0, truth) = free_sweep_bench(&sw, fs, 1e3, 2.0);
        let pairs = |v: &[f32]| v.chunks(200).flat_map(|c| [c.iter().copied().fold(f32::INFINITY, f32::min), c.iter().copied().fold(f32::NEG_INFINITY, f32::max)]).collect::<Vec<f32>>();
        let (x, y, sync) = (pairs(&x), pairs(&y), pairs(&sync));
        let dt = 100.0 / fs;
        let ts = sync_start(&sync, dt, t0, 1.5, 0.3, true, sw.time).unwrap();
        assert!(ts.abs() < 2.0 * dt, "{}", ts);
        let r = sweep_response(&sw, &SweepRecord { out: &y, inp: Some(&x), dt, t0: t0 - ts, envelope: true }, 60);
        let (eg, _) = free_sweep_errors(&r, &truth, None);
        println!("min/max sweep: {} points, gain error {:.3} dB", r.points.len(), eg);
        assert!(r.points.len() >= 58 && eg < 0.5 && r.points.iter().all(|p| p.phase.is_none()), "{} {:?}", eg, r);
    }

    #[test]
    fn free_sweep_limits() {
        let sw = Sweep { start: 20.0, stop: 20e3, time: 0.1, log: true };
        assert!((sw.freq_at(0.05) - 632.46).abs() < 0.01 && (sw.time_of(632.456) - 0.05).abs() < 1e-6);
        let lin = Sweep { log: false, ..sw };
        assert!((lin.freq_at(0.05) - 10010.0).abs() < 1e-6 && (lin.time_of(10010.0) - 0.05).abs() < 1e-12);
        // the phase is the integral of the frequency
        for s in [sw, lin] {
            let t = 0.03;
            let d = (s.phase_at(t + 1e-7) - s.phase_at(t - 1e-7)) / 2e-7 / (2.0 * PI);
            assert!((d / s.freq_at(t) - 1.0).abs() < 1e-6, "{}", d);
        }
        // a 100 ms sweep over 3 decades is too fast for its low end, and 50 kSa/s too slow for its top
        let fs = 50e3;
        let v = crate::sim::sample(|t| (sw.phase_at(t.max(0.0))).sin(), 0.0, 1.0 / fs, 5000);
        let r = sweep_response(&sw, &SweepRecord { out: &v, inp: None, dt: 1.0 / fs, t0: 0.0, envelope: false }, 30);
        assert!(r.too_fast > 0 && r.too_high > 0 && !r.points.is_empty(), "{:?}", r);
        // half the sweep in the record: the rest is outside
        let r = sweep_response(&Sweep { time: 0.2, ..sw }, &SweepRecord { out: &v, inp: None, dt: 1.0 / fs, t0: 0.0, envelope: false }, 30);
        assert!(r.outside > 0, "{:?}", r);
        assert!(!Sweep { start: 0.0, ..sw }.valid() && !Sweep { stop: 10.0, ..sw }.valid() && !Sweep { time: 0.0, ..sw }.valid());
        // the sync edge: the one with the whole sweep after it, nearest the trigger point
        let s: Vec<f32> = (0..1000).map(|k| if (100..110).contains(&(k % 300)) { 1.0 } else { 0.0 }).collect();
        // rising edges at -400.5, -100.5 and 199.5, the record ends at 499
        let at = |rising: bool, time: f64| sync_start(&s, 1.0, -500.0, 0.5, 0.1, rising, time).unwrap();
        assert_eq!((at(true, 450.0), at(true, 850.0), at(true, 1200.0), at(false, 450.0)), (-100.5, -400.5, -100.5, -90.5));
        assert_eq!(sync_start(&[0.0; 100], 1.0, 0.0, 0.5, 0.1, true, 10.0), None);
    }
}
