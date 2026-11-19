// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The levels of a record: the extremes, top and base (histogram modes), and from the crossings of the middle level
//! with hysteresis the period, the first pulse width and the 10-90 % rise and fall times. oos-ui's auto set-up, its
//! measurements and the eye's clock recovery start from them.

#[derive(Clone, Copy, Default)]
/// The levels of a record (`stats`): volts, and seconds for the times; NaN where a figure was not found.
pub struct Stats {
    pub vmax: f32,
    pub vmin: f32,
    pub top: f32,
    pub base: f32,
    pub period: f64,
    pub pwidth: f64,
    pub rise: f64,
    pub fall: f64,
    pub valid: bool,
}

/// The levels of `v` (volts, one sample every `dt` seconds); `valid` false with fewer than four finite samples.
pub fn stats(v: &[f32], dt: f64) -> Stats {
    let f: &[f32] = &crate::measure::finite_samples(v);
    if f.len() < 4 {
        return Stats::default();
    }
    let vmax = f.iter().copied().fold(f32::MIN, f32::max);
    let vmin = f.iter().copied().fold(f32::MAX, f32::min);
    // top/base: histogram modes of the upper and lower 40 %
    let span = (vmax - vmin).max(1e-9);
    let mut hist = [0u32; 256];
    for &x in f {
        hist[(((x - vmin) / span) * 255.0) as usize] += 1;
    }
    let mode = |a: usize, b: usize| -> usize { (a..b).max_by_key(|&i| hist[i]).unwrap_or(a) };
    let lo = mode(0, 102);
    let hi = mode(154, 256);
    let mut base = vmin + span * lo as f32 / 255.0;
    let mut top = vmin + span * hi as f32 / 255.0;
    if hist[lo] < (f.len() / 20) as u32 || hist[hi] < (f.len() / 20) as u32 {
        base = vmin; // no flat levels (sine, triangle): use the extremes
        top = vmax;
    }
    let amp = top - base;
    let mut s = Stats {
        vmax,
        vmin,
        top,
        base,
        period: f64::NAN,
        pwidth: f64::NAN,
        rise: f64::NAN,
        fall: f64::NAN,
        valid: true,
    };
    if amp <= 0.0 {
        return s;
    }
    let mid = base + amp * 0.5;
    let hy = amp * 0.1;
    // crossings with hysteresis at the mid level
    let mut rises = Vec::new();
    let mut falls = Vec::new();
    let mut state = v.iter().find(|x| x.is_finite()).map(|&x| x > mid).unwrap_or(false);
    for i in 1..v.len() {
        let (a, b) = (v[i - 1], v[i]);
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        if !state && b > mid + hy {
            state = true;
            // interpolate the mid crossing backwards
            let mut j = i;
            while j > 0 && v[j - 1].is_finite() && v[j - 1] > mid {
                j -= 1;
            }
            if j > 0 && v[j - 1].is_finite() {
                let (p, q) = (v[j - 1], v[j]);
                rises.push((j - 1) as f64 + ((mid - p) / (q - p)) as f64);
            }
        } else if state && b < mid - hy {
            state = false;
            let mut j = i;
            while j > 0 && v[j - 1].is_finite() && v[j - 1] < mid {
                j -= 1;
            }
            if j > 0 && v[j - 1].is_finite() {
                let (p, q) = (v[j - 1], v[j]);
                falls.push((j - 1) as f64 + ((p - mid) / (p - q)) as f64);
            }
        }
    }
    if rises.len() >= 2 {
        s.period = (rises[rises.len() - 1] - rises[0]) / (rises.len() - 1) as f64 * dt;
    } else if falls.len() >= 2 {
        s.period = (falls[falls.len() - 1] - falls[0]) / (falls.len() - 1) as f64 * dt;
    }
    if let Some(&r) = rises.first() {
        if let Some(&fl) = falls.iter().find(|&&x| x > r) {
            s.pwidth = (fl - r) * dt;
        }
    }
    // 10-90 % rise/fall around the first edges
    let l10 = base + amp * 0.1;
    let l90 = base + amp * 0.9;
    let cross = |from: usize, level: f32, up: bool, back: bool| -> Option<f64> {
        if back {
            let mut i = from;
            while i > 0 {
                let (a, b) = (v[i - 1], v[i]);
                if a.is_finite()
                    && b.is_finite()
                    && ((up && a < level && b >= level) || (!up && a > level && b <= level))
                {
                    return Some((i - 1) as f64 + ((level - a) / (b - a)) as f64);
                }
                i -= 1;
            }
            None
        } else {
            for i in from + 1..v.len() {
                let (a, b) = (v[i - 1], v[i]);
                if a.is_finite()
                    && b.is_finite()
                    && ((up && a < level && b >= level) || (!up && a > level && b <= level))
                {
                    return Some((i - 1) as f64 + ((level - a) / (b - a)) as f64);
                }
            }
            None
        }
    };
    if let Some(&r) = rises.first() {
        let ri = r as usize;
        if let (Some(a), Some(b)) = (cross(ri + 1, l10, true, true), cross(ri, l90, true, false)) {
            s.rise = (b - a) * dt;
        }
    }
    if let Some(&fl) = falls.first() {
        let fi = fl as usize;
        if let (Some(a), Some(b)) = (cross(fi + 1, l90, false, true), cross(fi, l10, false, false)) {
            s.fall = (b - a) * dt;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_wave_has_its_levels_period_and_width() {
        // 1 kHz, 0 V and 3 V, 30 % high, 1 us a sample, three periods
        let v: Vec<f32> = (0..3000).map(|i| if i % 1000 < 300 { 3.0 } else { 0.0 }).collect();
        let s = stats(&v, 1e-6);
        assert!(s.valid);
        assert_eq!((s.vmin, s.vmax), (0.0, 3.0));
        assert!((s.top - 3.0).abs() < 0.02 && s.base.abs() < 0.02, "{} {}", s.top, s.base);
        assert!((s.period - 1e-3).abs() < 2e-6, "{}", s.period);
        assert!((s.pwidth - 0.3e-3).abs() < 2e-6, "{}", s.pwidth);
    }

    #[test]
    fn too_few_samples_are_not_valid() {
        assert!(!stats(&[1.0, f32::NAN, 2.0], 1e-6).valid);
    }
}
