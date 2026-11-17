// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Periodic jitter and phase noise from the TIE trend and its spectrum: PJ lines, tones at sub-rates, spread of the
//! sub-rate tones, phase noise in a band and the DJ left after the periodic lines. A new figure taken from the TIE
//! spectrum goes here.

use super::{is_above, is_positive, jitter, tie_spectrum, Clock, EdgeMeans, TAIL_EDGES};

/// Periodic jitter in a TIE spectrum (`tie_spectrum`): the bins standing 6 x above the median floor, strongest first,
/// as (Hz, RMS s); white jitter stays under that even at its highest bin.
pub fn pj_lines(df: f64, spec: &[f64], count: usize) -> Vec<(f64, f64)> {
    if spec.len() < 8 {
        return Vec::new();
    }
    let mut s: Vec<f64> = spec[2..].to_vec();
    s.sort_by(f64::total_cmp);
    let floor = s[s.len() / 2];
    crate::math::peaks(spec, count)
        .into_iter()
        .filter(|&(_, v)| v > 6.0 * floor)
        .map(|(k, v)| (k as f64 * df, v))
        .collect()
}
/// Cosine and sine amplitudes of one tone on a TIE trend, and the tone itself (seconds).
pub(super) fn tone_fit(trend: &[f32], period: f64, f: f64) -> (f64, f64, Vec<f64>) {
    if trend.is_empty() || !is_positive(period) || !is_positive(f) {
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
    if n == 0 || !is_positive(period) || trend.len() < 8 {
        return f64::NAN;
    }
    let (re, im, _) = tone_fit(trend, period, 1.0 / (period * n as f64));
    let rms = (re * re + im * im).sqrt() / std::f64::consts::SQRT_2;
    2.0 * std::f64::consts::SQRT_2 * rms
}

/// Peak-to-peak of the sum of the F/2, F/4 and F/8 tones, phases kept.
pub fn srj_pp(trend: &[f32], period: f64) -> f64 {
    if !is_positive(period) || trend.len() < 8 {
        return f64::NAN;
    }
    let rate = 1.0 / period;
    let tones = [2.0, 4.0, 8.0].map(|n| tone_fit(trend, period, rate / n).2);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let [a, b, c] = tones.each_ref().map(|tone| &tone[..trend.len()]);
    for ((a, b), c) in a.iter().zip(b).zip(c) {
        let s = a + b + c;
        lo = lo.min(s);
        hi = hi.max(s);
    }
    if lo.is_finite() {
        hi - lo
    } else {
        f64::NAN
    }
}

/// RMS of the TIE-spectrum bins between two offsets, after a moving average one tenth of a decade wide,
/// with the periodic lines `pj_lines` marks left out. Seconds. A data record still returns the number.
pub fn phase_noise(trend: &[f32], period: f64, f_lo: f64, f_hi: f64) -> f64 {
    if trend.len() < 64 || !is_positive(period) || !is_above(f_hi, f_lo) {
        return f64::NAN;
    }
    let (df, spec) = tie_spectrum(trend, period);
    if !is_positive(df) || spec.len() < 8 {
        return f64::NAN;
    }
    let mut marked = vec![false; spec.len()];
    for (f, _) in pj_lines(df, &spec, spec.len()) {
        if let Some(m) = marked.get_mut((f / df).round() as usize) {
            *m = true;
        }
    }
    let (lo, hi) = (f_lo.min(f_hi), f_lo.max(f_hi));
    // the window of bin k is the run of bins j within ±0.05 decade of it: a sliding range over prefix sums, whose
    // ends only move up with k (a window summed afresh per bin took 1.7 s for 131k unit intervals)
    let mut prefix = Vec::with_capacity(spec.len() + 1);
    prefix.push(0.0);
    for &x in &spec {
        prefix.push(prefix[prefix.len() - 1] + x);
    }
    let last = spec.len() - 1;
    let step = 10f64.powf(0.05);
    let (mut a, mut b) = (1usize, 1usize);
    let mut acc = 0.0;
    let mut any = false;
    for (k, &is_marked) in marked.iter().enumerate().skip(1) {
        let f = k as f64 * df;
        if f < lo || f > hi || is_marked {
            continue;
        }
        let lf = f.log10();
        let inside = |j: usize| ((j as f64 * df).log10() - lf).abs() <= 0.05;
        // as the direct sum: bins from k / 10^0.05 (rounded down) to k x 10^0.05 (rounded up) that are inside
        let k0 = ((k as f64 / step).floor() as usize).max(1);
        let k1 = ((k as f64 * step).ceil() as usize).min(last);
        a = a.max(k0);
        while a < k && !inside(a) {
            a += 1;
        }
        b = b.max(k).max(a).min(k1);
        while b < k1 && inside(b + 1) {
            b += 1;
        }
        // bin k itself is always inside, so the window is never empty
        let v = (prefix[b + 1] - prefix[a]) / (b + 1 - a) as f64;
        acc += v * v;
        any = true;
    }
    if any {
        acc.sqrt()
    } else {
        f64::NAN
    }
}

/// A record whose edges are one unit apart is a clock. Phase noise of anything else is still a number, and the page says so.
pub fn is_clock(c: &Clock) -> bool {
    if c.edges.len() < 8 || !is_positive(c.period) {
        return false;
    }
    let n = c.edges.windows(2).filter(|w| ((w[1] - w[0]) / c.period - 1.0).abs() < 0.25).count();
    n * 5 >= 4 * (c.edges.len() - 1)
}

/// Dual-Dirac DJ left after the periodic lines and the DCD / DDJ means are subtracted. Empty below `TAIL_EDGES`.
pub fn npj(tie: &[f64], c: &Clock, trend: &[f32], means: &EdgeMeans) -> f64 {
    if tie.len() < TAIL_EDGES || !is_positive(c.period) || c.edges.len() < 2 {
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

#[cfg(test)]
mod tests;
