// SPDX-License-Identifier: GPL-2.0-or-later

//! Narrow-first trigger refinement (fork, docs/fork/perf-plan.md item 3). `fastdraw::edge_smooth` smooths and scans up
//! to half a screen on each side of the hardware trigger point for every frame, although the hardware usually put the
//! edge within a sample or two of it. `edge_smooth_near` first searches `NEAR_COLS` screen columns around the point and
//! keeps that answer only when it provably equals the wide one; otherwise it runs the wide search.
//!
//! Why the near answer is the wide one when it is accepted: the comparator of `oos_dsp::edges` is decided by the last
//! smoothed sample outside the hysteresis band, so from the first such sample in the near window on, both windows see
//! the same state and the same edges. Let d be the distance of the near result from `pre`. Every wide edge within d has
//! its time and its comparator transition between `pre` - d and the next out-of-band sample after `pre` + d (between an
//! edge's level crossing and its transition the signal stays inside the band). So when the near window has an
//! out-of-band sample before `pre` - d and one after `pre` + d, it holds every wide edge within d, in the same order:
//! the nearest one (and the nearest of either direction) is the same. The times can differ in the last bits (the
//! running sum starts at another sample, the window offset is added in another order), far below 1e-6 sample.

use crate::fastdraw::edge_smooth;
use oos_dsp::edges::Edge;

/// Half width of the near search in screen columns (plus `NEAR_PAD` samples).
pub const NEAR_COLS: usize = 16;
pub const NEAR_PAD: usize = 64;

/// `fastdraw::edge_smooth` with the same arguments and the same result, searched near `pre` first.
#[allow(clippy::too_many_arguments)]
pub fn edge_smooth_near(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize, l: usize, fits: impl Fn(f64) -> bool) -> (Option<f64>, Option<Edge>) {
    let l = l.max(1);
    let near = NEAR_COLS * l + NEAR_PAD;
    if near < span {
        if let Some(r) = near_only(v, pre, level, hyst, rising, either, near, l, &fits) {
            return r;
        }
    }
    edge_smooth(v, pre, level, hyst, rising, either, span, l, fits)
}

/// The near search: Some only when its answer is the wide search's (module comment).
#[allow(clippy::too_many_arguments)]
fn near_only(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize, l: usize, fits: &dyn Fn(f64) -> bool) -> Option<(Option<f64>, Option<Edge>)> {
    let (a, s) = smoothed(v, pre, span, l, level)?;
    let edges = oos_dsp::edges::edges(&s, level, hyst);
    let d = |e: &Edge| (a as f64 + e.t - pre as f64).abs();
    let near = |ok: &dyn Fn(&Edge) -> bool| edges.iter().filter(|e| ok(e) && fits(a as f64 + e.t)).min_by(|p, q| d(p).total_cmp(&d(q))).copied();
    let want = near(&|e| either || e.rising == rising)?;
    let dist = d(&want);
    // out-of-band samples on both sides beyond the result (absolute indices)
    let (lo, hi) = (level - hyst / 2.0, level + hyst / 2.0);
    let out = |x: &f32| x.is_finite() && (*x > hi || *x < lo);
    let first = s.iter().position(out)? as f64 + a as f64;
    let last = s.iter().rposition(out)? as f64 + a as f64;
    if !(first < pre as f64 - dist && last > pre as f64 + dist) {
        return None;
    }
    let any = near(&|_| true).map(|e| Edge { t: a as f64 + e.t, ..e });
    Some((Some(a as f64 + want.t), any))
}

/// The samples around `pre` (+-`span`, plus `l` for the average) smoothed by a centred moving average of `l` samples,
/// as `edge_smooth` computes them: the first index and the smoothed values (NaN where the window is incomplete).
fn smoothed(v: &[f32], pre: usize, span: usize, l: usize, level: f32) -> Option<(usize, Vec<f32>)> {
    let a = pre.saturating_sub(span + l);
    let b = (pre + span + l).min(v.len());
    if b <= a + 2 * l {
        return None;
    }
    let x = |i: usize| if v[i].is_finite() { v[i] as f64 } else { level as f64 };
    let half = l / 2;
    let mut s = Vec::with_capacity(b - a);
    let mut sum: f64 = (a..(a + l).min(b)).map(x).sum();
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
    Some((a, s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oos_dsp::sim;

    /// Frames of the guard corpus: name, samples.
    fn corpus() -> Vec<(String, Vec<f32>)> {
        let mut out = Vec::new();
        let n = 40_000;
        // fast square, slow square (edges over hundreds of samples), sine, each clean and noisy
        for (name, f, tr) in [("square", 1e5, 2e-8), ("slow-edge", 2e4, 4e-6), ("square-hf", 2e6, 1e-9)] {
            let p = sim::PulseTrain::square(f, -1.0, 1.0, tr);
            for noise in [0.0f32, 0.05, 0.4] {
                let mut v = sim::sample(|t| p.v(t), 3.3e-7, 1e-9, n);
                sim::add_noise(&mut v, noise, 7);
                out.push((format!("{} noise {}", name, noise), v));
            }
        }
        for noise in [0.0f32, 0.02, 0.3] {
            let mut v = sim::sample(|t| (t * 3.1e4 * std::f64::consts::TAU).sin(), 0.0, 1e-9, n);
            sim::add_noise(&mut v, noise, 11);
            out.push((format!("sine noise {}", noise), v));
        }
        // runts: pulses to half height between full pulses
        let runt: Vec<f32> = (0..n).map(|k| match (k / 500) % 4 { 0 => 1.0, 2 => 0.1, _ => -1.0 }).collect();
        out.push(("runts".into(), runt));
        let mut rp = sim::PulseTrain::square(1e5, -1.0, 1.0, 5e-8);
        (rp.every, rp.special) = (3, sim::Pulse { width: 5e-6, height: 0.45 });
        let mut v = sim::sample(|t| rp.v(t), 1.1e-6, 1e-9, n);
        sim::add_noise(&mut v, 0.03, 5);
        out.push(("runt-train".into(), v));
        // quantised like decoded ADC words: many samples exactly on the level
        let q: Vec<f32> = (0..n).map(|k| ((k as f32 / 900.0).sin() * 40.0).round() / 40.0).collect();
        out.push(("quantised".into(), q));
        // flat with a single glitch, and constant: no edge at all
        let mut g = vec![0.0f32; n];
        g[n / 2 + 37] = 1.0;
        out.push(("glitch".into(), g));
        out.push(("flat".into(), vec![0.25f32; n]));
        out
    }

    fn same(a: (Option<f64>, Option<Edge>), b: (Option<f64>, Option<Edge>)) -> bool {
        let t = |x: Option<f64>, y: Option<f64>| match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => (x - y).abs() < 1e-6,
            _ => false,
        };
        t(a.0, b.0) && t(a.1.map(|e| e.t), b.1.map(|e| e.t)) && a.1.map(|e| e.rising) == b.1.map(|e| e.rising)
    }

    // Break it catches: a near search that returns an edge the wide search would not (a closer edge it could not see,
    // an edge the comparator only decides outside the near window, a different edge of either direction).
    #[test]
    fn near_first_equals_the_wide_search() {
        let (mut cases, mut hits) = (0, 0);
        for (name, v) in corpus() {
            // anywhere, and where the hardware puts it: on or next to an edge
            let at_edges = oos_dsp::edges::edges(&v, 0.0, 0.1).into_iter().map(|e| e.t as usize).filter(|&t| t > 300 && t < v.len() - 300).step_by(7).take(12);
            let pres: Vec<usize> = (300..v.len() - 300).step_by(4999).chain(at_edges.flat_map(|t| [t, t + 3, t - 40])).collect();
            for pre in pres {
                for (l, span) in [(1, 600), (4, 2000), (16, 9000), (62, 19_000)] {
                    for hyst in [0.0f32, 0.06, 0.3] {
                        for level in [0.0f32, 0.3, -0.5] {
                            for (rising, either) in [(true, false), (false, false), (true, true)] {
                                let fits_all = |_: f64| true;
                                let lo = pre as f64 - 50.0;
                                let fits_some = |t: f64| t >= lo;
                                let w = edge_smooth(&v, pre, level, hyst, rising, either, span, l, fits_all);
                                let n = edge_smooth_near(&v, pre, level, hyst, rising, either, span, l, fits_all);
                                assert!(same(w, n), "{} pre {} l {} hyst {} level {} {:?}/{}: wide {:?} near {:?}", name, pre, l, hyst, level, rising, either, w, n);
                                let w = edge_smooth(&v, pre, level, hyst, rising, either, span, l, fits_some);
                                let n = edge_smooth_near(&v, pre, level, hyst, rising, either, span, l, fits_some);
                                assert!(same(w, n), "{} (fits) pre {} l {}: wide {:?} near {:?}", name, pre, l, w, n);
                                cases += 2;
                                hits += near_only(&v, pre, level, hyst, rising, either, NEAR_COLS * l + NEAR_PAD, l, &fits_all).is_some() as usize;
                            }
                        }
                    }
                }
            }
        }
        // the near search must answer a fair share by itself, or it only costs time
        println!("{} cases, the near search answered {}", cases / 2, hits);
        assert!(cases > 10_000 && hits * 5 > cases / 2, "{} cases, near answered {}", cases / 2, hits);
    }

    // Break it catches: a near result accepted although a closer wide edge is decided outside the near window, either
    // after it (the signal lingers inside the band) or before it (the near window starts inside the band).
    #[test]
    fn near_first_falls_back_when_the_window_cannot_decide() {
        let pre = 1000;
        // falling at pre - 30, then a rising crossing at pre - 10 that stays inside the band for 3000 samples
        let late: Vec<f32> = (0..6000).map(|k| if k < pre - 30 { 1.0 } else if k < pre - 10 { -1.0 } else if k < pre + 3000 { 0.05 } else { 1.0 }).collect();
        // -1, then just below the level from pre - 200, a rising crossing at pre - 75 inside the band, +1 from pre - 60,
        // falling at pre + 79
        let early: Vec<f32> = (0..6000).map(|k| if k < pre - 200 { -1.0 } else if k < pre - 75 { -0.05 } else if k < pre - 60 { 0.05 } else if k < pre + 79 { 1.0 } else { -1.0 }).collect();
        for v in [late, early] {
            let wide = edge_smooth(&v, pre, 0.0, 0.3, true, true, 4000, 1, |_| true);
            assert!(wide.1.is_some_and(|e| e.rising), "{:?}", wide);
            assert!(near_only(&v, pre, 0.0, 0.3, true, true, NEAR_COLS + NEAR_PAD, 1, &|_| true).is_none());
            assert!(same(wide, edge_smooth_near(&v, pre, 0.0, 0.3, true, true, 4000, 1, |_| true)));
        }
    }

    /// Reference timing (run with --ignored --nocapture): near-first vs the wide search at 12.5 k and 100 k points with
    /// the edge at the hardware point.
    #[test]
    #[ignore]
    fn near_first_is_much_cheaper_at_the_hardware_point() {
        for (n, l) in [(12_500usize, 12usize), (100_000, 100)] {
            let p = sim::PulseTrain::square(1e6 / (n as f64 / 1000.0), -1.0, 1.0, 1e-9);
            let mut v = sim::sample(|t| p.v(t), -(n as f64) / 2.0 * 1e-9, 1e-9, n);
            sim::add_noise(&mut v, 0.05, 3);
            let pre = n / 2;
            let (span, reps) = (n / 2, 2000);
            let t0 = std::time::Instant::now();
            let mut acc = 0.0;
            for _ in 0..reps {
                acc += edge_smooth(std::hint::black_box(&v), pre, 0.0, 0.06, true, false, span, l, |_| true).0.unwrap_or(0.0);
            }
            let wide = t0.elapsed();
            let t0 = std::time::Instant::now();
            for _ in 0..reps {
                acc -= edge_smooth_near(std::hint::black_box(&v), pre, 0.0, 0.06, true, false, span, l, |_| true).0.unwrap_or(0.0);
            }
            let near = t0.elapsed();
            let r = near.as_secs_f64() / wide.as_secs_f64();
            println!("{} points: wide {:?}, near-first {:?} per frame, ratio {:.3} ({})", n, wide / reps, near / reps, r, acc);
            assert!(r <= 0.2, "near/wide {:.3}", r);
        }
    }
}
