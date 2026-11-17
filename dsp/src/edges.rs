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
        for (amp, level, h) in [(0.1f32, 0.5f32, 0.15f32), (0.5, 0.5, 0.15), (0.05, 0.02, 0.15), (0.3, 0.0, 0.0), (0.4, 1.0, 0.3)] {
            let v: Vec<f32> = (0..5000).map(|i| if (i / 250) % 2 == 1 { 1.0 } else { 0.0 } + amp * noise()).collect();
            let got: Vec<(usize, bool)> = trigger_edges(&v, level, h).iter().map(|e| (e.t.ceil() as usize, e.rising)).collect();
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
}

/// sin(x)/x interpolation filter: row r gives the waveform at r/8 of a sample before sample i from samples i-4 .. i+3.
const SINC8: [[f64; 8]; 8] = [
    [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    [-0.002431213002523691, 0.013571640261943685, -0.0449928600273779, 0.13696806975349318, 0.9559562663298493, -0.07431540814104665, 0.018075864528327656, -0.0029436086272198736],
    [-0.004559319391205463, 0.025784400964766974, -0.0877010984440858, 0.2910057852332102, 0.8713054198315149, -0.12204652839318442, 0.03118660907193771, -0.005177759860202968],
    [-0.0060867394809065445, 0.03490663330831012, -0.12218482731532276, 0.45221821499035286, 0.752957912297461, -0.14396752281390945, 0.038379988420729205, -0.006485852327273278],
    [-0.006777513830684256, 0.039457774230413634, -0.14265809342759894, 0.6098363606597559, 0.6098363606597559, -0.14265809342759894, 0.039457774230413634, -0.006777513830684256],
    [-0.006485852327273278, 0.038379988420729205, -0.14396752281390945, 0.752957912297461, 0.45221821499035286, -0.12218482731532276, 0.03490663330831012, -0.0060867394809065445],
    [-0.005177759860202968, 0.03118660907193771, -0.12204652839318442, 0.8713054198315149, 0.2910057852332102, -0.0877010984440858, 0.025784400964766974, -0.004559319391205463],
    [-0.0029436086272198736, 0.018075864528327656, -0.07431540814104665, 0.9559562663298493, 0.13696806975349318, -0.0449928600273779, 0.013571640261943685, -0.002431213002523691],
];

/// Crossing of `level` between samples i-1 and i, as a fractional sample index: the waveform is sin(x)/x interpolated
/// at eighths of a sample, and the crossing lies on a straight line between the two eighths around it. Near the ends
/// of the record, or with a NaN among the 8 neighbours, plain straight-line interpolation between the two samples.
fn crossing(v: &[f32], i: usize, level: f64, rising: bool) -> f64 {
    let (a, b) = (v[i - 1] as f64, v[i] as f64);
    let linear = || if b != a { (i - 1) as f64 + (level - a) / (b - a) } else { i as f64 };
    if i < 4 || i + 4 > v.len() || v[i - 4..i + 4].iter().any(|x| !x.is_finite()) {
        return linear();
    }
    let taps = &v[i - 4..i + 4];
    let mut prev = a;
    for k in 0..8 {
        let val: f64 = taps.iter().zip(SINC8[7 - k].iter()).map(|(&x, c)| x as f64 * c).sum();
        if (rising && val >= level) || (!rising && val <= level) {
            let lo = (i - 1) as f64 + k as f64 * 0.125;
            return if val != prev { lo + 0.125 * (level - prev) / (val - prev) } else { lo };
        }
        prev = val;
    }
    linear()
}

/// Edges through `level` for measurements, with a hysteresis band of `hyst` on each side of the level. An edge counts
/// once the signal goes from beyond one side of the band to beyond the other. Its time is the crossing of `level`
/// (`crossing`); when the signal crosses the level, falls back and crosses again inside the band, the time is the
/// mean of the first and the last of those crossings. NaN samples are skipped.
pub fn level_edges(v: &[f32], level: f32, hyst: f32) -> Vec<Edge> {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Unknown,
        Low,
        UpInBand,   // crossed the level going up, still inside the band
        UpBack,     // and fell back below the level, still inside the band
        High,
        DownInBand,
        DownBack,
    }
    let t = level as f64;
    let (hi, lo) = (t + hyst as f64, t - hyst as f64);
    let mut out = Vec::new();
    let mut s = S::Unknown;
    let (mut t1, mut t2) = (0.0, 0.0);
    let mut prev_ok = false;
    for i in 0..v.len() {
        let x = v[i] as f64;
        if !x.is_finite() {
            prev_ok = false;
            continue;
        }
        let can = prev_ok && i > 0;
        prev_ok = true;
        let cross = |rising| if can { crossing(v, i, t, rising) } else { i as f64 };
        s = match s {
            S::Unknown => {
                if x > hi {
                    S::High
                } else if x < lo {
                    S::Low
                } else {
                    S::Unknown
                }
            }
            S::Low if x > hi => {
                out.push(Edge { t: cross(true), rising: true });
                S::High
            }
            S::Low if x > t => {
                (t1, t2) = (cross(true), cross(true));
                S::UpInBand
            }
            S::UpInBand if x > hi => {
                out.push(Edge { t: (t1 + t2) / 2.0, rising: true });
                S::High
            }
            S::UpInBand if x < lo => S::Low,
            S::UpInBand if x < t => S::UpBack,
            S::UpBack if x < lo => S::Low,
            S::UpBack if x > hi => {
                t2 = cross(true);
                out.push(Edge { t: (t1 + t2) / 2.0, rising: true });
                S::High
            }
            S::UpBack if x > t => {
                t2 = cross(true);
                S::UpInBand
            }
            S::High if x < lo => {
                out.push(Edge { t: cross(false), rising: false });
                S::Low
            }
            S::High if x < t => {
                (t1, t2) = (cross(false), cross(false));
                S::DownInBand
            }
            S::DownInBand if x < lo => {
                out.push(Edge { t: (t1 + t2) / 2.0, rising: false });
                S::Low
            }
            S::DownInBand if x > hi => S::High,
            S::DownInBand if x > t => S::DownBack,
            S::DownBack if x > hi => S::High,
            S::DownBack if x < lo => {
                t2 = cross(false);
                out.push(Edge { t: (t1 + t2) / 2.0, rising: false });
                S::Low
            }
            S::DownBack if x < t => {
                t2 = cross(false);
                S::DownInBand
            }
            other => other,
        };
    }
    out
}
