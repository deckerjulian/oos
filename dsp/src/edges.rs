// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Comparator with hysteresis.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// fractional sample index where the signal crosses the level
    pub t: f64,
    pub rising: bool,
}

/// Edges of `v` through `level` with a hysteresis band of `hyst` (total width, centred on the level): a rising edge is
/// counted when the signal goes from below level - hyst/2 to above level + hyst/2; its time is the last crossing of
/// `level` before that (linear interpolation). NaN samples are skipped.
///
/// A signal can rest inside the band: a DDR strobe (DQS) idles at its mid level between bursts. The last crossing of
/// `level` then lies where the rest began, often a whole burst gap before the edge. When the signal stayed in the band
/// after that crossing much longer than it took to get there from the far side, the crossing is stale and the edge is
/// timed where the signal leaves the band instead (its crossing of level ± hyst/2).
pub fn edges(v: &[f32], level: f32, hyst: f32) -> Vec<Edge> {
    let (lo, hi) = (level - hyst / 2.0, level + hyst / 2.0);
    let mut out = Vec::new();
    let mut state: Option<bool> = None;
    let mut last_cross: Option<f64> = None;
    // last sample outside the band on the side the signal is leaving (above hi while high, below lo while low)
    let mut outside: Option<f64> = None;
    let mut prev: Option<(usize, f32)> = None;
    // the edge time: the level crossing, or the band exit when the crossing went stale in the band
    let time = |cross: Option<f64>, outside: Option<f64>, prev: Option<(usize, f32)>, i: usize, x: f32, bound: f32| -> f64 {
        let exit = prev.map_or(i as f64, |(j, p)| if x != p { j as f64 + (bound - p) as f64 / (x - p) as f64 * (i - j) as f64 } else { i as f64 });
        match (cross, outside) {
            (Some(c), Some(o)) if exit - c > 4.0 * (c - o).max(1.0) => exit,
            (Some(c), _) => c,
            (None, _) => exit,
        }
    };
    for (i, &x) in v.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        if let Some((j, p)) = prev {
            if (p < level) != (x < level) {
                last_cross = Some(j as f64 + (level - p) as f64 / (x - p) as f64 * (i - j) as f64);
            }
        }
        match state {
            None => {
                if x > hi {
                    state = Some(true);
                } else if x < lo {
                    state = Some(false);
                }
            }
            Some(false) if x > hi => {
                state = Some(true);
                out.push(Edge { t: time(last_cross, outside, prev, i, x, hi), rising: true });
            }
            Some(true) if x < lo => {
                state = Some(false);
                out.push(Edge { t: time(last_cross, outside, prev, i, x, lo), rising: false });
            }
            _ => {}
        }
        match state {
            Some(true) if x > hi => outside = Some(i as f64),
            Some(false) if x < lo => outside = Some(i as f64),
            _ => {}
        }
        prev = Some((i, x));
    }
    out
}

/// Logic state of `v` at every sample with hysteresis (None until the first decision).
pub fn states(v: &[f32], level: f32, hyst: f32) -> Vec<Option<bool>> {
    let (lo, hi) = (level - hyst / 2.0, level + hyst / 2.0);
    let mut s = None;
    v.iter()
        .map(|&x| {
            if x > hi {
                s = Some(true);
            } else if x < lo {
                s = Some(false);
            }
            s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_noise() {
        // 0/1 V square, period 100 samples, small noise that would chatter without hysteresis
        let v: Vec<f32> = (0..1000).map(|i| if (i / 50) % 2 == 1 { 1.0 } else { 0.0 } + if i % 2 == 0 { 0.02 } else { -0.02 }).collect();
        let e = edges(&v, 0.5, 0.1);
        assert_eq!(e.len(), 19);
        assert!(e[0].rising && (e[0].t - 49.5).abs() < 0.6);
        assert!(!e[1].rising && (e[1].t - 99.5).abs() < 0.6);
    }

    #[test]
    fn interpolates_a_ramp() {
        let v: Vec<f32> = (0..100).map(|i| i as f32 * 0.01).collect();
        let e = edges(&v, 0.255, 0.02);
        assert_eq!(e.len(), 1);
        assert!((e[0].t - 25.5).abs() < 1e-3);
    }
}
