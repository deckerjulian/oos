// Copyright (C) 2026 dekajoule
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
pub fn edges(v: &[f32], level: f32, hyst: f32) -> Vec<Edge> {
    let (lo, hi) = (level - hyst / 2.0, level + hyst / 2.0);
    let mut out = Vec::new();
    let mut state: Option<bool> = None;
    let mut last_cross: Option<f64> = None;
    let mut prev: Option<(usize, f32)> = None;
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
                out.push(Edge { t: last_cross.unwrap_or(i as f64), rising: true });
            }
            Some(true) if x < lo => {
                state = Some(false);
                out.push(Edge { t: last_cross.unwrap_or(i as f64), rising: false });
            }
            _ => {}
        }
        prev = Some((i, x));
    }
    out
}

/// Edges of `v` as the trigger comparators of the FPGA see them (docs/protocols/trigger.md 2.3): a rising edge is armed
/// by a sample below `level - h` and fires at the first sample at or above `level` after it; a falling edge is armed
/// above `level + h` and fires at or below `level`. The two directions arm independently. The time is the crossing of
/// `level` between the firing sample and the one before (linear interpolation). NaN samples are skipped.
///
/// Unlike `edges`, the band is not centred: noise that stays within `h` below the level never arms a rising edge,
/// as on the hardware, and an edge is decided at its crossing.
pub fn trigger_edges(v: &[f32], level: f32, h: f32) -> Vec<Edge> {
    let (arm_r, arm_f) = (level - h, level + h);
    let (mut up, mut down) = (false, false);
    let mut out = Vec::new();
    let mut prev: Option<(usize, f32)> = None;
    for (i, &x) in v.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        let cross = |(j, p): (usize, f32)| j as f64 + (level - p) as f64 / (x - p) as f64 * (i - j) as f64;
        if up && x >= level {
            up = false;
            out.push(Edge { t: prev.map_or(i as f64, cross), rising: true });
        }
        if down && x <= level {
            down = false;
            out.push(Edge { t: prev.map_or(i as f64, cross), rising: false });
        }
        if x < arm_r {
            up = true;
        }
        if x > arm_f {
            down = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_noise() {
        // 0/1 V square, period 100 samples, small noise that would chatter without hysteresis
        let v: Vec<f32> = (0..1000)
            .map(|i| if (i / 50) % 2 == 1 { 1.0 } else { 0.0 } + if i % 2 == 0 { 0.02 } else { -0.02 })
            .collect();
        let e = edges(&v, 0.5, 0.1);
        assert_eq!(e.len(), 19);
        assert!(e[0].rising && (e[0].t - 49.5).abs() < 0.6);
        assert!(!e[1].rising && (e[1].t - 99.5).abs() < 0.6);
    }

    /// The comparator pair written plainly, per direction: the reference of `trigger_edges`.
    fn comparators(v: &[f32], level: f32, h: f32) -> Vec<(usize, bool)> {
        let mut out = Vec::new();
        for rising in [true, false] {
            let mut armed = false;
            for (i, &x) in v.iter().enumerate() {
                let (arms, fires) = if rising { (x < level - h, x >= level) } else { (x > level + h, x <= level) };
                if armed && fires {
                    out.push((i, rising));
                    armed = false;
                }
                armed |= arms;
            }
        }
        out.sort();
        out
    }

    // Break it catches: a software trigger that arms on noise inside the hardware band (it then shows Trig'd and moves
    // frames the hardware forced), or one that waits for the far side of a centred band
    #[test]
    fn trigger_edges_follow_the_hardware_comparators() {
        let mut x = 99u32;
        let mut noise = || {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        for (amp, level, h) in
            [(0.1f32, 0.5f32, 0.15f32), (0.5, 0.5, 0.15), (0.05, 0.02, 0.15), (0.3, 0.0, 0.0), (0.4, 1.0, 0.3)]
        {
            let v: Vec<f32> = (0..5000).map(|i| if (i / 250) % 2 == 1 { 1.0 } else { 0.0 } + amp * noise()).collect();
            let got: Vec<(usize, bool)> =
                trigger_edges(&v, level, h).iter().map(|e| (e.t.ceil() as usize, e.rising)).collect();
            let mut got = got;
            got.sort();
            assert_eq!(got, comparators(&v, level, h), "amp {} level {} h {}", amp, level, h);
        }
        // near the low rail: noise of 0.2 pp around 0 V never reaches 0.02 - 0.15, so no rising edge arms;
        // the centred comparator of `edges` arms at 0.02 - 0.075 and finds edges in the noise
        let v: Vec<f32> = (0..4000).map(|_| 0.2 * noise()).collect();
        assert!(trigger_edges(&v, 0.02, 0.15).is_empty());
        assert!(!edges(&v, 0.02, 0.15).is_empty());
        // decided at the crossing: the edge is there although the signal stops just above the level
        let v = [0.0f32, 0.0, 0.55, 0.56];
        let e = trigger_edges(&v, 0.5, 0.3);
        assert_eq!(e.len(), 1);
        assert!(e[0].rising && (e[0].t - (1.0 + 0.5 / 0.55)).abs() < 1e-6);
        assert!(edges(&v, 0.5, 0.3).is_empty());
    }

    #[test]
    fn interpolates_a_ramp() {
        let v: Vec<f32> = (0..100).map(|i| i as f32 * 0.01).collect();
        let e = edges(&v, 0.255, 0.02);
        assert_eq!(e.len(), 1);
        assert!((e[0].t - 25.5).abs() < 1e-3);
    }

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

    /// A falling edge with noise crosses the level upwards too: with hysteresis that is no rising edge, and the rising
    /// edge half a period away is found.
    #[test]
    fn hysteresis_keeps_noise_on_the_other_edge_out() {
        // a sine, 1000 samples a period, with a +-0.02 wiggle; it falls through 0 at 500
        let v: Vec<f32> = (0..3000)
            .map(|k| (k as f32 / 1000.0 * std::f32::consts::TAU).sin() + if k % 2 == 0 { 0.02 } else { -0.02 })
            .collect();
        // without hysteresis the wiggle at the falling crossing looks like a rising edge
        assert!((edge_near(&v, 500, 0.0, 0.0, true, false, 16).unwrap() - 500.0).abs() < 3.0);
        // with it, nothing rising near 500 ...
        assert!(edge_near(&v, 500, 0.0, 0.1, true, false, 16).is_none());
        // ... and the rising crossing at 1000 within half a period
        assert!((edge_near(&v, 500, 0.0, 0.1, true, false, 600).unwrap() - 1000.0).abs() < 3.0);
        // the falling one is where the hardware said
        assert!((edge_near(&v, 500, 0.0, 0.1, false, false, 16).unwrap() - 500.0).abs() < 6.0);
    }
}
