// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Fast display path for the hardware source: a frame's raw samples go straight into the phosphor map, one vertical
//! span per screen column (min..max of the column's samples, joined to the previous column's last sample), instead
//! of the screen record of `board::to_record` (software trigger over a tenth of a screen, then 3600 interpolated or
//! peak-detected points) for every frame. Used for dense views (at least one sample per column) with a plain edge
//! trigger; the hardware already put the edge at `pre` to within a sample or two, so the crossing is only refined
//! (sub-sample, by linear interpolation) near it.

/// The level crossing of the edge nearest to `pre` within +-`span` samples, as a fractional sample index (rising:
/// below -> at or above `level`; falling the other way). `either`: both directions. `hyst` > 0: like the hardware
/// comparators, only the first crossing after the signal was `hyst` on the other side of the level counts, so noise on
/// an edge of the other direction is no edge.
pub fn edge_near(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let lo = pre.saturating_sub(span).max(1);
    let hi = (pre + span).min(v.len() - 1);
    // back from the crossing at k: armed when the signal went past the band before it came back over the level
    let look = span.max(4096);
    let armed = |k: usize, up: bool| {
        if hyst <= 0.0 {
            return true;
        }
        for j in (k.saturating_sub(look)..k).rev() {
            let x = v[j];
            if up && x < level - hyst || !up && x > level + hyst {
                return true;
            }
            if up && x >= level || !up && x <= level {
                return false;
            }
        }
        false
    };
    let mut best: Option<f64> = None;
    for k in lo..=hi {
        let (a, b) = (v[k - 1], v[k]);
        let up = a < level && b >= level && (rising || either) && armed(k, true);
        let down = a > level && b <= level && (!rising || either) && armed(k, false);
        if up || down {
            let t = (k - 1) as f64 + ((level - a) / (b - a)) as f64;
            if best.is_none_or(|x| (t - pre as f64).abs() < (x - pre as f64).abs()) {
                best = Some(t);
            }
        }
    }
    best
}

/// Where the edge of the set direction is, seen at the resolution of the screen: `v` around `pre` (+-`span` samples)
/// smoothed by a centred moving average of `l` samples (one screen column), then a comparator with a hysteresis band of
/// `hyst` (total width, as the hardware's), the edge of the set direction nearest to `pre`. Noise larger than the band
/// (a slow sine at a high rate) cannot make an edge of the other direction look right, which a test on single samples
/// cannot rule out. Only edges at times `fits` accepts count. Returns the edge and the nearest edge of either direction
/// (for the statistics).
#[allow(clippy::too_many_arguments)]
pub fn edge_smooth(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize, l: usize, fits: impl Fn(f64) -> bool) -> (Option<f64>, Option<oos_dsp::edges::Edge>) {
    let l = l.max(1);
    let a = pre.saturating_sub(span + l);
    let b = (pre + span + l).min(v.len());
    if b <= a + 2 * l {
        return (None, None);
    }
    // running sum over the window; NaN samples count as the level (none in hardware frames)
    let x = |i: usize| if v[i].is_finite() { v[i] as f64 } else { level as f64 };
    let mut s = Vec::with_capacity(b - a);
    let mut sum = 0.0;
    let half = l / 2;
    for i in a..(a + l).min(b) {
        sum += x(i);
    }
    // s[j] = mean of v[a + j - half .. a + j - half + l], defined from j = half
    for j in 0..(b - a) {
        let lo = a + j;
        if j >= half && lo + l - half <= b {
            s.push((sum / l as f64) as f32);
            if lo + l - half < b {
                sum += x(lo + l - half) - x(lo - half);
            }
        } else {
            s.push(f32::NAN);
        }
    }
    let edges = oos_dsp::edges::edges(&s, level, hyst);
    let d = |e: &oos_dsp::edges::Edge| (a as f64 + e.t - pre as f64).abs();
    let near = |ok: &dyn Fn(&oos_dsp::edges::Edge) -> bool| edges.iter().filter(|e| ok(e) && fits(a as f64 + e.t)).min_by(|p, q| d(p).total_cmp(&d(q))).copied();
    let want = near(&|e| either || e.rising == rising).map(|e| a as f64 + e.t);
    (want, near(&|_| true).map(|e| oos_dsp::edges::Edge { t: a as f64 + e.t, ..e }))
}

/// Screen mapping of one channel: x = (k - ti) * x_per_sample + x0 (columns), y = val * y_per_volt + y0 (rows).
#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub ti: f64,
    pub x_per_sample: f64,
    pub x0: f64,
    pub y_per_volt: f32,
    pub y0: f32,
}

/// Adds `w` to every pixel of `map` (w x h, row-major) the trace covers: per column the span of its samples, joined
/// to the last sample of the column before (so steep edges stay connected). Needs at least one sample per column
/// (`x_per_sample <= 1`).
pub fn deposit(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16) {
    if v.is_empty() || m.x_per_sample <= 0.0 {
        return;
    }
    // first sample of column c: ceil(k_of(c)); per column one min/max over a slice (no per-sample screen math)
    let k_of = |x: f64| m.ti + (x - m.x0) / m.x_per_sample;
    let first = |c: i64| (k_of(c as f64).ceil().max(0.0) as usize).min(v.len());
    let c0 = (((-m.ti) * m.x_per_sample + m.x0).floor() as i64).max(0);
    let c1 = ((((v.len() as f64 - m.ti) * m.x_per_sample + m.x0).ceil()) as i64).min(w as i64);
    let hh = h as i32;
    let mut ka = first(c0);
    for c in c0..c1 {
        let kb = first(c + 1);
        if kb <= ka {
            continue;
        }
        let (mut mn, mut mx) = (f32::INFINITY, f32::NEG_INFINITY);
        for &x in &v[ka..kb] {
            // f32::min/max skip NaN (gaps)
            mn = mn.min(x);
            mx = mx.max(x);
        }
        // the join to the previous column's last sample
        if ka > 0 && v[ka - 1].is_finite() {
            mn = mn.min(v[ka - 1]);
            mx = mx.max(v[ka - 1]);
        }
        ka = kb;
        if !(mn <= mx) {
            continue;
        }
        let (ya, yb) = (mn * m.y_per_volt + m.y0, mx * m.y_per_volt + m.y0);
        let r0 = (ya.min(yb).round() as i32).max(0);
        let r1 = (ya.max(yb).round() as i32).min(hh - 1);
        let mut i = r0.max(0) as usize * w + c as usize;
        for _ in r0..=r1 {
            map[i] = map[i].saturating_add(wgt);
            i += w;
        }
    }
}

/// Dots display (`Model::dots`): every sample as one pixel, no spans and no joins; the brightness of a column follows
/// where its samples dwell. Same mapping and column walk as `deposit`.
pub fn deposit_dots(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16) {
    if v.is_empty() || m.x_per_sample <= 0.0 {
        return;
    }
    let k_of = |x: f64| m.ti + (x - m.x0) / m.x_per_sample;
    let first = |c: i64| (k_of(c as f64).ceil().max(0.0) as usize).min(v.len());
    let c0 = (((-m.ti) * m.x_per_sample + m.x0).floor() as i64).max(0);
    let c1 = ((((v.len() as f64 - m.ti) * m.x_per_sample + m.x0).ceil()) as i64).min(w as i64);
    let mut ka = first(c0);
    for c in c0..c1 {
        let kb = first(c + 1);
        for &x in &v[ka..kb.max(ka)] {
            let r = (x * m.y_per_volt + m.y0).round();
            // NaN fails both comparisons
            if r >= 0.0 && r < h as f32 {
                let i = r as usize * w + c as usize;
                map[i] = map[i].saturating_add(wgt);
            }
        }
        ka = kb.max(ka);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_is_refined_near_pre() {
        // a ramp crossing 0.5 between samples 11 and 12 (at 11.25), another rising crossing far away
        let mut v: Vec<f32> = (0..40).map(|k| (k as f32 - 9.0) * 0.4 - 0.4).collect();
        v[30] = -1.0;
        v[31] = 1.0;
        let t = edge_near(&v, 11, 0.5, 0.0, true, false, 4).unwrap();
        assert!((t - 11.25).abs() < 1e-4, "{}", t);
        assert!(edge_near(&v, 11, 0.5, 0.0, false, false, 4).is_none());
        // the nearest of two crossings wins
        assert!((edge_near(&v, 29, 0.0, 0.0, true, false, 40).unwrap() - 30.5).abs() < 1e-4);
    }

    /// A slow sine with noise larger than the band, the hardware point on the falling edge: the smoothed search finds
    /// the rising edge half a period away, never the noise on the falling one.
    #[test]
    fn smoothed_search_takes_the_trend_not_the_noise() {
        // 12500 samples a period, amplitude 0.55, noise +-0.075 (pseudo random), level 0, band 0.06
        let mut r = 12345u32;
        let v: Vec<f32> = (0..50_000)
            .map(|k| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                0.55 * (k as f32 / 12500.0 * std::f32::consts::TAU).sin() + ((r >> 16) as f32 / 65536.0 - 0.5) * 0.15
            })
            .collect();
        for pre in [6250 + 130, 6250 - 40, 6250, 18750 + 200] {
            let (e, any) = edge_smooth(&v, pre, 0.0, 0.06, true, false, 31_250, 62, |_| true);
            let t = e.unwrap();
            assert!((t - 12500.0).abs() < 60.0 || (t - 25000.0).abs() < 60.0, "pre {}: {}", pre, t);
            assert!(!any.unwrap().rising, "pre {}: the nearest edge is the falling one", pre);
        }
        // at the rising edge itself
        assert!((edge_smooth(&v, 12_480, 0.0, 0.06, true, false, 31_250, 62, |_| true).0.unwrap() - 12500.0).abs() < 60.0);
        // falling
        assert!((edge_smooth(&v, 12_480, 0.0, 0.06, false, false, 31_250, 62, |_| true).0.unwrap() - 6250.0).abs() < 60.0);
    }

    /// A falling edge with noise crosses the level upwards too: with hysteresis that is no rising edge, and the rising
    /// edge half a period away is found.
    #[test]
    fn hysteresis_keeps_noise_on_the_other_edge_out() {
        // a sine, 1000 samples a period, with a +-0.02 wiggle; it falls through 0 at 500
        let v: Vec<f32> = (0..3000).map(|k| (k as f32 / 1000.0 * std::f32::consts::TAU).sin() + if k % 2 == 0 { 0.02 } else { -0.02 }).collect();
        // without hysteresis the wiggle at the falling crossing looks like a rising edge
        assert!((edge_near(&v, 500, 0.0, 0.0, true, false, 16).unwrap() - 500.0).abs() < 3.0);
        // with it, nothing rising near 500 ...
        assert!(edge_near(&v, 500, 0.0, 0.1, true, false, 16).is_none());
        // ... and the rising crossing at 1000 within half a period
        assert!((edge_near(&v, 500, 0.0, 0.1, true, false, 600).unwrap() - 1000.0).abs() < 3.0);
        // the falling one is where the hardware said
        assert!((edge_near(&v, 500, 0.0, 0.1, false, false, 16).unwrap() - 500.0).abs() < 6.0);
    }

    #[test]
    fn spans_per_column_and_joined_edges() {
        let (w, h) = (10, 20);
        let mut map = vec![0u16; w * h];
        // a square: 4 samples per column, low (row 15) for 5 columns, then high (row 5)
        let v: Vec<f32> = (0..40).map(|k| if k < 20 { 0.0 } else { 1.0 }).collect();
        let m = Map { ti: 0.0, x_per_sample: 0.25, x0: 0.0, y_per_volt: -10.0, y0: 15.0 };
        deposit(&mut map, w, h, &v, &m, 1);
        let col = |c: usize| (0..h).filter(|&r| map[r * w + c] > 0).collect::<Vec<_>>();
        assert_eq!(col(2), [15]);
        // column 5 holds the step: joined from row 15 (last low sample) up to row 5
        assert_eq!(col(5), (5..=15).collect::<Vec<_>>());
        assert_eq!(col(7), [5]);
        // samples off screen are skipped; a shift by ti moves the step
        let mut map2 = vec![0u16; w * h];
        deposit(&mut map2, w, h, &v, &Map { ti: 8.0, ..m }, 1);
        assert_eq!((0..h).filter(|&r| map2[r * w + 3] > 0).count(), 11);
        // dots: only the sample rows, no join across the step, 4 samples per column add up
        let mut map3 = vec![0u16; w * h];
        deposit_dots(&mut map3, w, h, &v, &m, 1);
        let col3 = |c: usize| (0..h).filter(|&r| map3[r * w + c] > 0).collect::<Vec<_>>();
        assert_eq!((col3(5), map3[5 * w + 5], col3(4)), (vec![5], 4, vec![15]));
    }
}

