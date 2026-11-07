// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Bandwidth measurement of the scope's own front end (docs/bandwidth-test.md):
//!
//! - **Equivalent-time folding**: a periodic signal sampled for a long record is sorted by its phase `t mod T` into
//!   many bins. The picture then has a much finer time step than the sample period, even for signals above Nyquist
//!   (the samples alias, the folding undoes it). It needs the true period, which `tone` and `unalias` give from the
//!   strongest tone of the record plus a rough frequency hint.
//! - **Step response**: from a fast edge (a square wave from a pulse generator), the 10-90 % rise time, the overshoot,
//!   and the magnitude response |H(f)| as the FFT of the edge's derivative, normalised to DC, with its -3 dB point.

use crate::bode::sine_fit;
use crate::math::fft;

/// Frequency (Hz, 0..fs/2) of the strongest tone of `v`: FFT peak over the first 2^17 samples (Hann window,
/// zero-padded 4x, parabolic interpolation), then refined by the phase drift between sine fits on the two halves of
/// ever longer parts of the record (doubling up to the whole record, so the phase difference never wraps). For a
/// periodic signal above Nyquist this is its alias. The cost stays O(n) for long records.
pub fn tone(v: &[f32], dt: f64) -> Option<f64> {
    if v.len() < 64 || dt <= 0.0 {
        return None;
    }
    let n0 = v.len().min(1 << 17);
    let head = &v[..n0];
    let mean = head.iter().map(|&x| x as f64).sum::<f64>() / n0 as f64;
    let m = (n0 * 4).next_power_of_two();
    let (mut re, mut im) = (vec![0.0; m], vec![0.0; m]);
    for (k, &x) in head.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / (n0 - 1) as f64).cos();
        re[k] = (x as f64 - mean) * w;
    }
    fft(&mut re, &mut im);
    let mag = |i: usize| (re[i] * re[i] + im[i] * im[i]).sqrt();
    let k = (2..m / 2 - 1).max_by(|&a, &b| mag(a).total_cmp(&mag(b)))?;
    let (a, b, c) = (mag(k - 1).ln(), mag(k).ln(), mag(k + 1).ln());
    let d = if (a - 2.0 * b + c).abs() > 1e-30 { 0.5 * (a - c) / (a - 2.0 * b + c) } else { 0.0 };
    let mut f = (k as f64 + d) / (m as f64 * dt);
    let mut len = n0;
    loop {
        // A sin(w t + phi) fitted at a slightly wrong w shows the phase of the segment's middle. The second half
        // starts h samples later, so its phase is ahead by w_true h dt: dphi = (w_true - w) h dt.
        let h = len / 2;
        for _ in 0..2 {
            let (Some(p1), Some(p2)) = (sine_fit(&v[..h], dt, f), sine_fit(&v[h..2 * h], dt, f)) else { break };
            let expect = 2.0 * std::f64::consts::PI * f * h as f64 * dt;
            let dphi = ((p2.1 - p1.1) - expect + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI;
            f += dphi / (2.0 * std::f64::consts::PI * h as f64 * dt);
        }
        if len == v.len() {
            break;
        }
        len = (len * 2).min(v.len());
    }
    Some(f)
}

/// True frequency of a tone seen at the alias `f_alias` when sampled at `fs`: the candidate `n fs +- f_alias` nearest
/// to `hint` (the hint must be within fs/4 of the truth).
pub fn unalias(f_alias: f64, fs: f64, hint: f64) -> f64 {
    let n = (hint / fs).round();
    [n * fs + f_alias, n * fs - f_alias, (n + 1.0) * fs - f_alias, (n - 1.0) * fs + f_alias]
        .into_iter()
        .filter(|f| *f > 0.0)
        .min_by(|a, b| (a - hint).abs().total_cmp(&(b - hint).abs()))
        .unwrap_or(f_alias)
}

/// Equivalent-time picture of a periodic signal: every sample goes to bin `floor(bins * frac(t / period))`, the bin
/// value is the mean; empty bins are interpolated from their neighbours (cyclically). Returns the bins (one period,
/// starting at phase 0 of the record) and their time step `period / bins`. None if fewer samples than bins.
pub fn fold(v: &[f32], dt: f64, period: f64, bins: usize) -> Option<(Vec<f32>, f64)> {
    if bins < 4 || period <= 0.0 || v.len() < bins {
        return None;
    }
    let (mut sum, mut cnt) = (vec![0.0f64; bins], vec![0u32; bins]);
    for (k, &x) in v.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        let ph = (k as f64 * dt / period).fract();
        let b = ((ph * bins as f64) as usize).min(bins - 1);
        sum[b] += x as f64;
        cnt[b] += 1;
    }
    let filled: Vec<usize> = (0..bins).filter(|&b| cnt[b] > 0).collect();
    if filled.len() < bins / 4 {
        return None;
    }
    let mut out = vec![0.0f32; bins];
    for &b in &filled {
        out[b] = (sum[b] / cnt[b] as f64) as f32;
    }
    // empty bins: linear between the nearest filled bins on both sides (cyclic)
    for i in 0..filled.len() {
        let (a, b) = (filled[i], filled[(i + 1) % filled.len()]);
        let gap = (b + bins - a) % bins;
        for j in 1..gap {
            let t = j as f32 / gap as f32;
            out[(a + j) % bins] = out[a] * (1.0 - t) + out[b] * t;
        }
    }
    Some((out, period / bins as f64))
}

/// Folds a record of a periodic signal: its tone (alias) and the hint give the period, then `fold`.
/// Returns (period picture, its time step, the true frequency).
pub fn fold_auto(v: &[f32], dt: f64, hint_hz: f64, bins: usize) -> Option<(Vec<f32>, f64, f64)> {
    let f = unalias(tone(v, dt)?, 1.0 / dt, hint_hz);
    let (w, step) = fold(v, dt, 1.0 / f, bins)?;
    Some((w, step, f))
}

/// Result of `step_response`.
#[derive(Clone, Debug, PartialEq)]
pub struct StepResponse {
    /// 10-90 % rise time (s)
    pub rise: f64,
    /// overshoot above the final level (% of the step)
    pub overshoot: f64,
    /// low and high level (V)
    pub levels: (f64, f64),
    /// first frequency where |H| falls below -3 dB, interpolated
    pub f3db: Option<f64>,
    /// 0.35 / rise: the bandwidth of a Gaussian-like response with this rise time
    pub bw_from_rise: f64,
    /// |H(f)| in dB relative to DC, up to the Nyquist frequency of the picture
    pub mag: Vec<(f64, f64)>,
    /// the edge as used (the segment around the rising edge), for display
    pub edge: Vec<f32>,
    /// index of the 50 % crossing in `edge`
    pub edge_at: usize,
}

/// Step response of one period of a square wave (or any picture with one clean rising edge between two flat
/// levels), time step `dt`. The picture is treated as periodic: the segment from a quarter period before to a
/// quarter period after the rising edge is used (for a 50 % square wave that stays on the two plateaus).
pub fn step_response(v: &[f32], dt: f64) -> Option<StepResponse> {
    let n = v.len();
    if n < 16 || dt <= 0.0 {
        return None;
    }
    let (min, max) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    // levels: means of the halves around the middle, without the edges (25 / 75 % of the range)
    let lo_v: Vec<f64> = v.iter().filter(|&&x| x < min + (max - min) * 0.25).map(|&x| x as f64).collect();
    let hi_v: Vec<f64> = v.iter().filter(|&&x| x > min + (max - min) * 0.75).map(|&x| x as f64).collect();
    if lo_v.is_empty() || hi_v.is_empty() {
        return None;
    }
    let (lo, hi) = (lo_v.iter().sum::<f64>() / lo_v.len() as f64, hi_v.iter().sum::<f64>() / hi_v.len() as f64);
    let amp = hi - lo;
    if amp <= 0.0 {
        return None;
    }
    let at = |i: isize| v[i.rem_euclid(n as isize) as usize] as f64;
    // the rising 50 % crossing (first one, cyclic)
    let mid = (lo + hi) / 2.0;
    let e = (0..n as isize).find(|&i| at(i - 1) < mid && at(i) >= mid)?;
    let q = (n / 4) as isize;
    let seg: Vec<f64> = (e - q..e + q).map(at).collect();
    let c = q as usize; // the crossing in seg
    // 10 / 90 % crossings around the edge, interpolated
    let cross = |level: f64, from: usize, back: bool| -> Option<f64> {
        let lv = lo + level * amp;
        if back {
            (1..=from).rev().find(|&i| seg[i - 1] < lv && seg[i] >= lv).map(|i| i as f64 - 1.0 + (lv - seg[i - 1]) / (seg[i] - seg[i - 1]))
        } else {
            (from..seg.len()).find(|&i| i > 0 && seg[i - 1] < lv && seg[i] >= lv).map(|i| i as f64 - 1.0 + (lv - seg[i - 1]) / (seg[i] - seg[i - 1]))
        }
    };
    let t10 = cross(0.1, c, true)?;
    let t90 = cross(0.9, c.saturating_sub(1), false)?;
    let rise = (t90 - t10) * dt;
    let peak = seg[c..].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let overshoot = ((peak - hi) / amp * 100.0).max(0.0);
    // impulse response = derivative of the step, tapered at the segment ends (cosine over the outer 10 %)
    let len = seg.len() - 1;
    let taper = (len / 10).max(1);
    let h: Vec<f64> = (0..len)
        .map(|i| {
            let d = seg[i + 1] - seg[i];
            let edge = i.min(len - 1 - i);
            if edge < taper {
                d * 0.5 * (1.0 - (std::f64::consts::PI * edge as f64 / taper as f64).cos())
            } else {
                d
            }
        })
        .collect();
    let m = (len * 8).next_power_of_two();
    let (mut re, mut im) = (vec![0.0; m], vec![0.0; m]);
    re[..len].copy_from_slice(&h);
    fft(&mut re, &mut im);
    let dc = (re[0] * re[0] + im[0] * im[0]).sqrt();
    if dc <= 0.0 {
        return None;
    }
    let mag: Vec<(f64, f64)> = (0..=m / 2).map(|k| (k as f64 / (m as f64 * dt), 20.0 * ((re[k] * re[k] + im[k] * im[k]).sqrt() / dc).max(1e-12).log10())).collect();
    let f3db = mag.windows(2).find(|w| w[0].1 >= -3.0 && w[1].1 < -3.0).map(|w| w[0].0 + (w[1].0 - w[0].0) * (w[0].1 + 3.0) / (w[0].1 - w[1].1));
    Some(StepResponse { rise, overshoot, levels: (lo, hi), f3db, bw_from_rise: 0.35 / rise, mag, edge: seg.iter().map(|&x| x as f32).collect(), edge_at: c })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Square wave (period `t`, levels 0/1) through a single-pole low-pass of corner `fc`, sampled every `dt` from
    /// `t0`, with a small deterministic noise.
    fn rc_square(n: usize, dt: f64, t: f64, fc: f64, t0: f64) -> Vec<f32> {
        let tau = 1.0 / (2.0 * std::f64::consts::PI * fc);
        (0..n)
            .map(|k| {
                let tt = t0 + k as f64 * dt;
                let ph = (tt / t).fract() * t;
                let half = t / 2.0;
                let y = if ph < half { 1.0 - (-ph / tau).exp() } else { (-(ph - half) / tau).exp() };
                let noise = ((k as f64 * 12.9898).sin() * 43758.5453).fract() - 0.5;
                (y + noise * 0.004) as f32
            })
            .collect()
    }

    #[test]
    fn tone_and_unalias() {
        // 400 MHz sampled at 312.5 MSa/s: alias at 87.5 MHz
        let dt = 1.0 / 312.5e6;
        let v: Vec<f32> = (0..200_000).map(|k| (2.0 * std::f64::consts::PI * 400.0123e6 * k as f64 * dt).sin() as f32).collect();
        let fa = tone(&v, dt).unwrap();
        assert!((fa - 87.5123e6).abs() < 0.05, "alias {}", fa);
        let f = unalias(fa, 312.5e6, 410e6);
        assert!((f - 400.0123e6).abs() < 1.0, "{}", f);
        assert_eq!(unalias(10e6, 1.25e9, 9e6), 10e6);
    }

    #[test]
    fn folding_recovers_a_sine_above_nyquist() {
        let (dt, f) = (1.0 / 312.5e6, 400.0123e6);
        let v: Vec<f32> = (0..400_000).map(|k| (2.0 * std::f64::consts::PI * f * k as f64 * dt).sin() as f32).collect();
        let (w, step, ft) = fold_auto(&v, dt, 410e6, 200).unwrap();
        assert!((ft - f).abs() < 1.0);
        assert!((step - 1.0 / f / 200.0).abs() < 1e-15);
        // the folded period is the sine (phase 0 at the record start), within 2 %
        for (i, &x) in w.iter().enumerate() {
            let want = (2.0 * std::f64::consts::PI * (i as f64 + 0.5) / 200.0).sin();
            assert!((x as f64 - want).abs() < 0.02, "bin {}: {} vs {}", i, x, want);
        }
    }

    #[test]
    fn step_response_of_a_single_pole() {
        // 300 MHz single pole: rise 0.35/300 MHz = 1.167 ns (exactly 2.2 tau), f3db 300 MHz. A 10.0123 MHz square
        // sampled at 1.25 GSa/s (0.8 ns, slower than the rise) and folded into 2000 bins (50 ps).
        let (dt, t, fc) = (0.8e-9, 1.0 / 10.0123e6, 300e6);
        let v = rc_square(1_000_000, dt, t, fc, 0.37e-9);
        let (w, step, f) = fold_auto(&v, dt, 10e6, 2000).unwrap();
        assert!((f - 10.0123e6).abs() < 10.0, "{}", f);
        let r = step_response(&w, step).unwrap();
        let tau = 1.0 / (2.0 * std::f64::consts::PI * fc);
        assert!((r.rise / (tau * 9f64.ln()) - 1.0).abs() < 0.05, "rise {} vs {}", r.rise, tau * 9f64.ln());
        let f3 = r.f3db.unwrap();
        assert!((f3 / fc - 1.0).abs() < 0.08, "f3db {}", f3);
        assert!(r.overshoot < 1.0);
        assert!((r.levels.0).abs() < 0.01 && (r.levels.1 - 1.0).abs() < 0.01);
        // the response falls with frequency
        let at = |hz: f64| r.mag.iter().find(|p| p.0 >= hz).unwrap().1;
        assert!(at(100e6) > -1.0 && at(1e9) < -8.0);
    }

    #[test]
    fn rejects_garbage() {
        assert!(tone(&[0.0; 10], 1e-9).is_none());
        assert!(fold(&[1.0; 10], 1e-9, 1e-6, 100).is_none());
        assert!(step_response(&[1.0; 100], 1e-9).is_none());
    }
}
