// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the periodic-jitter module: `srj_pp` and `phase_noise` give bit for bit what the indexed loops they
//! replaced gave. Further tests of `periodic` go here.

use super::*;

// Indexed reference calculations preserve the operation order before the iterator rewrite.
fn indexed_srj_pp(trend: &[f32], period: f64) -> f64 {
    if !is_positive(period) || trend.len() < 8 {
        return f64::NAN;
    }
    let rate = 1.0 / period;
    let tones = [2.0, 4.0, 8.0].map(|n| tone_fit(trend, period, rate / n).2);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut k = 0;
    while k < trend.len() {
        let s = tones[0][k] + tones[1][k] + tones[2][k];
        lo = lo.min(s);
        hi = hi.max(s);
        k += 1;
    }
    if lo.is_finite() {
        hi - lo
    } else {
        f64::NAN
    }
}

fn indexed_phase_noise(trend: &[f32], period: f64, f_lo: f64, f_hi: f64) -> f64 {
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
    let mut k = 0;
    while k + 1 < spec.len() {
        k += 1;
        let f = k as f64 * df;
        if f < lo || f > hi || marked[k] {
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

fn trend(len: usize) -> Vec<f32> {
    let mut seed = 7u64;
    (0..len)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
            let phase = 2.0 * std::f64::consts::PI * i as f64;
            (1e-10 * ((phase / 2.0 + 0.3).cos() + (phase / 4.0).sin() + 0.3 * (phase / 8.0).sin() + noise)) as f32
        })
        .collect()
}

#[test]
fn subrate_sum_matches_indexed_calculation_bit_for_bit() {
    for len in [0, 7, 8, 9, 63, 64, 65, 127, 1024] {
        for data in [vec![0.0; len], vec![1e-10; len], trend(len)] {
            for period in [0.0, -0.0, -1.0, f64::NAN, 1e-9, 0.25] {
                assert_eq!(
                    srj_pp(&data, period).to_bits(),
                    indexed_srj_pp(&data, period).to_bits(),
                    "len={len}, period={period}"
                );
            }
        }
    }
}

#[test]
fn phase_noise_matches_indexed_calculation_bit_for_bit() {
    for len in [0, 63, 64, 65, 127, 512, 4096] {
        for data in [vec![0.0; len], vec![1e-10; len], trend(len)] {
            for period in [0.0, -0.0, -1.0, f64::NAN, 1e-9, 0.25, f64::INFINITY] {
                for (lo, hi) in [
                    (0.0, 0.5 / period),
                    (0.01 / period, 0.2 / period),
                    (0.499 / period, 0.5 / period),
                    (-1.0, f64::INFINITY),
                    (1.0, 0.0),
                    (f64::NAN, 1.0),
                    (0.0, f64::NAN),
                ] {
                    assert_eq!(
                        phase_noise(&data, period, lo, hi).to_bits(),
                        indexed_phase_noise(&data, period, lo, hi).to_bits(),
                        "len={len}, period={period}, band=({lo}, {hi})"
                    );
                }
            }
        }
    }
}

#[test]
fn subrate_sum_keeps_the_infinite_period_bounds_check() {
    let data = trend(8);
    assert!(std::panic::catch_unwind(|| indexed_srj_pp(&data, f64::INFINITY)).is_err());
    assert!(std::panic::catch_unwind(|| srj_pp(&data, f64::INFINITY)).is_err());
}
