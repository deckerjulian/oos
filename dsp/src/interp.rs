// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Interpolation between samples for the screen: linear, and windowed sinc (Lanczos) for sparse views where only a
//! few samples fall on the screen (fast time bases). A band-limited signal sampled above twice its bandwidth is
//! reconstructed by the sinc sum; the Lanczos window (a lobes) keeps it local and fast.

/// Linear interpolation at fractional index `x` (None outside the samples or on a non-finite neighbour).
pub fn linear(v: &[f32], x: f64) -> Option<f32> {
    if x < 0.0 || x > (v.len() as f64 - 1.0) || v.len() < 2 {
        return None;
    }
    let i = (x as usize).min(v.len() - 2);
    let fr = (x - i as f64) as f32;
    let (a, b) = (v[i], v[i + 1]);
    (a.is_finite() && b.is_finite()).then_some(a * (1.0 - fr) + b * fr)
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let p = std::f64::consts::PI * x;
        p.sin() / p
    }
}

/// Lanczos-windowed sinc interpolation with `a` lobes at fractional index `x`. Near the ends the missing neighbours
/// are left out and the kernel weights are renormalised, so the edges stay finite.
pub fn lanczos(v: &[f32], x: f64, a: usize) -> Option<f32> {
    if x < 0.0 || x > (v.len() as f64 - 1.0) || v.len() < 2 {
        return None;
    }
    let i0 = x.floor() as i64;
    let a = a.max(1) as i64;
    let (mut sum, mut wsum) = (0.0f64, 0.0f64);
    for i in (i0 - a + 1)..=(i0 + a) {
        if i < 0 || i >= v.len() as i64 {
            continue;
        }
        let s = v[i as usize];
        if !s.is_finite() {
            return linear(v, x);
        }
        let d = x - i as f64;
        let w = sinc(d) * sinc(d / a as f64);
        sum += w * s as f64;
        wsum += w;
    }
    (wsum.abs() > 1e-12).then(|| (sum / wsum) as f32)
}

/// Phases of the precomputed Lanczos kernel (`lanczos8`).
const PHASES: usize = 1024;
/// Lobes of `lanczos8`.
const A: usize = 8;

/// Kernel table of `lanczos8`: for each phase p (fraction p / PHASES) the 2A weights of the samples i0 - A + 1 ..= i0 + A.
fn table() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Vec::with_capacity((PHASES + 1) * 2 * A);
        for p in 0..=PHASES {
            let fr = p as f64 / PHASES as f64;
            for j in 0..2 * A {
                let d = fr - (j as f64 - (A as f64 - 1.0));
                t.push((sinc(d) * sinc(d / A as f64)) as f32);
            }
        }
        t
    })
}

/// `lanczos(v, x, 8)` with a precomputed kernel (1024 phases): no trigonometry per point, for drawing thousands of
/// screen points per record. Same edge handling (missing neighbours left out, weights renormalised).
pub fn lanczos8(v: &[f32], x: f64) -> Option<f32> {
    if x < 0.0 || x > (v.len() as f64 - 1.0) || v.len() < 2 {
        return None;
    }
    let i0 = x.floor() as i64;
    let p = (((x - i0 as f64) * PHASES as f64).round() as usize).min(PHASES);
    let w = &table()[p * 2 * A..(p + 1) * 2 * A];
    let first = i0 - A as i64 + 1;
    let (mut sum, mut wsum) = (0.0f32, 0.0f32);
    if first >= 0 && first + 2 * A as i64 <= v.len() as i64 {
        // inner part: all neighbours present
        let s = &v[first as usize..first as usize + 2 * A];
        for (a, b) in s.iter().zip(w) {
            sum += a * b;
            wsum += b;
        }
        if !sum.is_finite() {
            return linear(v, x);
        }
    } else {
        for (j, &wj) in w.iter().enumerate() {
            let i = first + j as i64;
            if i < 0 || i >= v.len() as i64 {
                continue;
            }
            let s = v[i as usize];
            if !s.is_finite() {
                return linear(v, x);
            }
            sum += s * wj;
            wsum += wj;
        }
    }
    (wsum.abs() > 1e-6).then(|| sum / wsum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sinc_reconstructs_a_sparse_sine_much_better_than_linear() {
        // 2.5 samples per period (0.4 of the sampling rate), like a 500 MHz sine at 1.25 GSa/s
        let f = 0.4;
        let v: Vec<f32> = (0..200).map(|k| (2.0 * std::f64::consts::PI * f * k as f64).sin() as f32).collect();
        let (mut el, mut es) = (0.0f64, 0.0f64);
        let mut n = 0;
        let mut x = 50.0;
        while x < 150.0 {
            let truth = (2.0 * std::f64::consts::PI * f * x).sin();
            el = el.max((linear(&v, x).unwrap() as f64 - truth).abs());
            es = es.max((lanczos(&v, x, 8).unwrap() as f64 - truth).abs());
            x += 0.07;
            n += 1;
        }
        assert!(n > 1000);
        assert!(el > 0.5, "linear error {}", el);
        assert!(es < 0.08, "sinc error {}", es);
    }

    #[test]
    fn table_kernel_matches_the_exact_one() {
        let v: Vec<f32> = (0..300).map(|k| ((k as f64 * 0.37).sin() + 0.3 * (k as f64 * 1.9).cos()) as f32).collect();
        let mut x = 0.0;
        while x < 299.0 {
            let (a, b) = (lanczos(&v, x, 8).unwrap(), lanczos8(&v, x).unwrap());
            assert!((a - b).abs() < 2e-3, "x {} exact {} table {}", x, a, b);
            x += 0.013;
        }
        assert_eq!(lanczos8(&v, -0.5), None);
    }

    #[test]
    fn exact_at_samples_and_safe_at_the_edges() {
        let v = [0.0f32, 1.0, 0.5, -0.25, 2.0];
        for (i, &s) in v.iter().enumerate() {
            assert!((lanczos(&v, i as f64, 4).unwrap() - s).abs() < 1e-6);
        }
        assert!(lanczos(&v, 0.3, 4).unwrap().is_finite() && lanczos(&v, 3.9, 4).unwrap().is_finite());
        assert_eq!(lanczos(&v, -0.1, 4), None);
        assert_eq!(lanczos(&v, 4.1, 4), None);
        let w = [0.0f32, f32::NAN, 1.0];
        assert_eq!(lanczos(&w, 0.5, 4), None); // falls back to linear, which refuses the NaN neighbour
    }
}
