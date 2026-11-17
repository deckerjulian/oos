// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal-level measurements of a serial link that need no eye: common mode (DC and AC), the differential crossover,
//! transition / non-transition ratio and spread-spectrum clocking. A new per-record measurement of a lane goes here.

use super::{is_positive, Clock};

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
    if c == 0 {
        f64::NAN
    } else {
        s / c as f64
    }
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
    if lo.is_finite() {
        hi - lo
    } else {
        f64::NAN
    }
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
    if s.is_empty() {
        f64::NAN
    } else {
        s.iter().sum::<f64>() / s.len() as f64
    }
}

/// Mean 20 log10 of |midpoint of a non-transition bit| / |midpoint of the nearest preceding transition bit|.
pub fn tnt_ratio(v: &[f32], dt: f64, c: &Clock) -> f64 {
    if !is_positive(c.period) || !is_positive(dt) || v.len() < 2 || c.edges.is_empty() {
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
        if a.is_finite() && b.is_finite() {
            a + f * (b - a)
        } else {
            f64::NAN
        }
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
    if ratios.is_empty() {
        f64::NAN
    } else {
        ratios.iter().sum::<f64>() / ratios.len() as f64
    }
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
    let (low, high) =
        series.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), (_, f)| (low.min(*f), high.max(*f)));
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
    spectrum
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, magnitude)| **magnitude > 0.0)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(bin, _)| bin as f64 * df)
        .filter(|f| *f > 0.0)
        .unwrap_or(f64::NAN)
}
