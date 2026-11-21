// Copyright (C) 2026 dekajoule
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

/// The kernel of `table` divided by the sum of its phase's weights: the inner part of `lanczos8` needs no
/// renormalisation per point.
fn table_norm() -> &'static [f32] {
    static T: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        table()
            .as_chunks::<{ 2 * A }>()
            .0
            .iter()
            .flat_map(|w| {
                let s: f64 = w.iter().map(|&x| x as f64).sum();
                w.iter().map(move |&x| (x as f64 / s) as f32)
            })
            .collect()
    })
}

/// The `2 * A` samples of `v` from `first` on, None when one of them is missing.
fn window(v: &[f32], first: i64) -> Option<&[f32; 2 * A]> {
    v.get(usize::try_from(first).ok()?..)?.first_chunk()
}

/// The `2 * A` weights of phase `p` in a kernel table (`table` or `table_norm`).
fn weights(t: &[f32], p: usize) -> &[f32; 2 * A] {
    &t.as_chunks().0[p]
}

/// The 16-term dot product of `lanczos8`, in four partial sums (vector registers; one long chain otherwise).
#[inline(always)]
fn dot16(s: &[f32; 2 * A], w: &[f32; 2 * A]) -> f32 {
    let mut acc = [0.0f32; 4];
    for j in 0..2 * A / 4 {
        for l in 0..4 {
            acc[l] += s[4 * j + l] * w[4 * j + l];
        }
    }
    (acc[0] + acc[1]) + (acc[2] + acc[3])
}

/// `lanczos(v, x, 8)` with a precomputed kernel (1024 phases): no trigonometry per point, for drawing thousands of
/// screen points per record. Same edge handling (missing neighbours left out, weights renormalised).
pub fn lanczos8(v: &[f32], x: f64) -> Option<f32> {
    if x < 0.0 || x > (v.len() as f64 - 1.0) || v.len() < 2 {
        return None;
    }
    let i0 = x.floor() as i64;
    let p = (((x - i0 as f64) * PHASES as f64).round() as usize).min(PHASES);
    let first = i0 - A as i64 + 1;
    if let Some(s) = window(v, first) {
        // inner part: all neighbours present, the weights of the phase sum to 1
        let y = dot16(s, weights(table_norm(), p));
        return if y.is_finite() { Some(y) } else { linear(v, x) };
    }
    let w = weights(table(), p);
    let (mut sum, mut wsum) = (0.0f32, 0.0f32);
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
    (wsum.abs() > 1e-6).then(|| sum / wsum)
}

/// `lanczos8(v, x0 + k * step)` for k in 0..n onto `out` (NaN where it gives None): a screen record's points in one
/// pass, without the per-point checks where all neighbours are there.
pub fn lanczos8_grid(v: &[f32], x0: f64, step: f64, n: usize, out: &mut Vec<f32>) {
    out.reserve(n);
    let tn = table_norm();
    for k in 0..n {
        let x = x0 + k as f64 * step;
        let i0 = x.floor();
        let first = i0 as i64 - A as i64 + 1;
        match window(v, first) {
            Some(s) if x >= 0.0 => {
                let p = (((x - i0) * PHASES as f64).round() as usize).min(PHASES);
                let y = dot16(s, weights(tn, p));
                out.push(if y.is_finite() { y } else { linear(v, x).unwrap_or(f32::NAN) });
            }
            _ => out.push(lanczos8(v, x).unwrap_or(f32::NAN)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grid gives exactly what `lanczos8` gives point by point, inside, at both ends, past them and over a gap.
    #[test]
    fn grid_equals_point_by_point() {
        let mut v: Vec<f32> = (0..300).map(|k| (k as f32 * 0.37).sin() + 0.1 * (k as f32 * 2.9).cos()).collect();
        v[150] = f32::NAN;
        let (x0, step, n) = (-5.3, 0.0917, 3600);
        let mut g = Vec::new();
        lanczos8_grid(&v, x0, step, n, &mut g);
        assert_eq!(g.len(), n);
        for (k, &y) in g.iter().enumerate() {
            let want = lanczos8(&v, x0 + k as f64 * step).unwrap_or(f32::NAN);
            assert!(y == want || (y.is_nan() && want.is_nan()), "k {}: {} vs {}", k, y, want);
        }
    }

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

    // Issue #335 step 1. Break it catches: a kernel (fewer lobes, a coarser phase table) that rings on the signals Auto
    // bandwidth lets through. Auto puts the nominal filter corner near a quarter of the channel's rate
    // (`hal::dho::afe::auto_c`), so a sine up to 0.25 of the rate must come back almost exactly, and one up to 0.4 (the
    // filter's slope) still close; the error is the largest over many phases between the samples.
    #[test]
    fn the_screen_kernel_stays_clean_up_to_the_auto_corner() {
        let worst = |f: f64| {
            let v: Vec<f32> =
                (0..400).map(|k| (2.0 * std::f64::consts::PI * f * k as f64 + 0.3).sin() as f32).collect();
            let mut out = Vec::new();
            lanczos8_grid(&v, 100.0, 0.0371, 5000, &mut out);
            out.iter()
                .enumerate()
                .map(|(k, &y)| {
                    (y as f64 - (2.0 * std::f64::consts::PI * f * (100.0 + k as f64 * 0.0371) + 0.3).sin()).abs()
                })
                .fold(0.0, f64::max)
        };
        // host, 2026-11-21: at most 0.0023 up to 0.25, 0.0076 at 0.35, 0.023 at 0.4. The bound up to the corner
        // (-48 dB of the amplitude) stays under the scope's own noise (SINAD 43.4 dB, docs/models.md 4.1).
        for (f, bound) in [(0.01, 0.004), (0.05, 0.004), (0.1, 0.004), (0.15, 0.004), (0.2, 0.004), (0.25, 0.004)] {
            assert!(worst(f) < bound, "{} of the rate: error {}", f, worst(f));
        }
        for (f, bound) in [(0.3, 0.01), (0.35, 0.015), (0.4, 0.04)] {
            assert!(worst(f) < bound, "{} of the rate: error {}", f, worst(f));
        }
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
