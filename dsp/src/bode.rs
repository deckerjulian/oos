// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Frequency response (Bode plot) helpers: sine fit at a known frequency, gain and phase of a DUT output against its
//! input, and the logarithmic sweep plan.

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
    // solve the 3x3 system by Cramer's rule
    let m = [[ss, sc, s1], [sc, cc, c1], [s1, c1, nn]];
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let col = |i: usize| {
        let mut x = m;
        for (r, v) in [ys, yc, y1].iter().enumerate() {
            x[r][i] = *v;
        }
        det(x) / d
    };
    let (a, b, c) = (col(0), col(1), col(2));
    Some(((a * a + b * b).sqrt(), b.atan2(a), c))
}

/// Gain (dB) and phase (degrees, -180..180, output relative to input) of `out` against `inp` at `f`.
pub fn gain_phase(inp: &[f32], out: &[f32], dt: f64, f: f64) -> Option<(f64, f64)> {
    let (ai, pi, _) = sine_fit(inp, dt, f)?;
    let (ao, po, _) = sine_fit(out, dt, f)?;
    if ai <= 0.0 {
        return None;
    }
    let mut ph = (po - pi).to_degrees();
    while ph > 180.0 {
        ph -= 360.0;
    }
    while ph <= -180.0 {
        ph += 360.0;
    }
    Some((20.0 * (ao / ai).log10(), ph))
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
    use std::f64::consts::PI;

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
}
