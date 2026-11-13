//! Independent bounded search oracles and an opt-in host CPU timing fixture.
use super::*;

fn closest(candidates: impl IntoIterator<Item = f64>, pre: usize) -> Option<f64> {
    candidates.into_iter().fold(None, |best, t| match best {
        Some(old) if (old - pre as f64).abs() <= (t - pre as f64).abs() => Some(old),
        _ => Some(t),
    })
}

/// Track rail visits forward, independently of edge_near's backward candidate scan.
fn raw_oracle(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize) -> Option<f64> {
    let mut low = None;
    let mut high = None;
    let mut not_low = None;
    let mut not_high = None;
    let mut candidates = Vec::new();
    let look = span.max(4096);
    for k in 1..v.len() {
        let previous = v[k - 1];
        if previous < level - hyst { low = Some(k - 1); }
        if previous > level + hyst { high = Some(k - 1); }
        if previous >= level { not_low = Some(k - 1); }
        if previous <= level { not_high = Some(k - 1); }
        if k < pre.saturating_sub(span).max(1) || k > pre.saturating_add(span).min(v.len() - 1) { continue; }
        let up = previous < level && v[k] >= level;
        let down = previous > level && v[k] <= level;
        let armed = |rail: Option<usize>, reset: Option<usize>| hyst <= 0.0 || rail.is_some_and(|at|
            at >= k.saturating_sub(look) && reset.is_none_or(|after| at > after));
        if (up && (rising || either) && armed(low, not_low)) || (down && (!rising || either) && armed(high, not_high)) {
            candidates.push((k - 1) as f64 + (level as f64 - previous as f64) / (v[k] as f64 - previous as f64));
        }
    }
    closest(candidates, pre)
}

/// Compute each mean independently. Then pair opposite stable rails and locate
/// their last level crossing backwards; this does not call the production edge FSM.
fn smooth_oracle(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool,
    span: usize, length: usize, fits: impl Fn(f64) -> bool) -> (Option<f64>, Option<(f64, bool)>) {
    let length = length.max(1);
    let a = pre.saturating_sub(span + length);
    let b = pre.saturating_add(span + length).min(v.len());
    if b <= a + 2 * length { return (None, None); }
    let half = length / 2;
    let means: Vec<(usize, f32)> = (a + half..b - (length - half) + 1).map(|i| {
        let sum = v[i - half..i - half + length].iter().map(|&x| if x.is_finite() { x as f64 } else { level as f64 }).sum::<f64>();
        (i, (sum / length as f64) as f32)
    }).collect();
    let rails: Vec<_> = means.iter().enumerate().filter_map(|(i, &(_, value))| {
        if value > level + hyst / 2.0 { Some((i, true)) }
        else if value < level - hyst / 2.0 { Some((i, false)) } else { None }
    }).collect();
    let mut candidates = Vec::new();
    for pair in rails.windows(2) {
        let ((from, old), (to, new)) = (pair[0], pair[1]);
        if old == new { continue; }
        let crossing = (from + 1..=to).rev().find_map(|i| {
            let ((j, p), (k, value)) = (means[i - 1], means[i]);
            ((p < level) != (value < level)).then(|| j as f64 +
                (level as f64 - p as f64) / (value as f64 - p as f64) * (k - j) as f64)
        }).unwrap_or(means[to].0 as f64);
        if fits(crossing) { candidates.push((crossing, new)); }
    }
    let want = closest(candidates.iter().filter(|(_, up)| either || *up == rising).map(|(t, _)| *t), pre);
    let any = closest(candidates.iter().map(|(t, _)| *t), pre).and_then(|at| candidates.into_iter().find(|(t, _)| *t == at));
    (want, any)
}

fn same(a: Option<f64>, b: Option<f64>, label: &str) {
    match (a, b) {
        (Some(a), Some(b)) => assert!((a - b).abs() < 2e-5, "{label}: {a} versus {b}"),
        (None, None) => {},
        _ => panic!("{label}: {a:?} versus {b:?}"),
    }
}

fn signal(seed: usize, count: usize) -> Vec<f32> {
    let mut noise = seed as u32 + 17;
    (0..count).map(|i| {
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let phase = (i as f64 + seed as f64 * 7.3) / (113.0 + seed as f64 * 13.0);
        let wave = if seed % 2 == 0 { phase.fract() as f32 * 2.0 - 1.0 }
            else { (phase * std::f64::consts::TAU).sin() as f32 };
        let value = 0.1 + wave + ((noise >> 16) as f32 / 65536.0 - 0.5) * (seed as f32 * 0.03);
        if seed == 3 && i % 101 == 0 { f32::NAN } else { value }
    }).collect()
}

#[test]
fn nearest_raw_and_smoothed_search_match_independent_oracles() {
    let mut cases = 0;
    for seed in 0..4 {
        let values = signal(seed, 4096);
        for pre in [0, 2, 63, 511, 2048, 4093] {
            for span in [32, 257] {
                for hysteresis in [0.0, 0.1, 0.6] {
                    for (rising, either) in [(true, false), (false, false), (false, true)] {
                        same(edge_near(&values, pre, 0.1, hysteresis, rising, either, span),
                            raw_oracle(&values, pre, 0.1, hysteresis, rising, either, span), "raw oracle");
                        for length in [1, 8, 31] {
                            // Admit/reject different boundary edges as well as testing direction.
                            let fits = |t: f64| t >= pre.saturating_sub(span / 2) as f64 && t <= (pre + span / 2) as f64;
                            let got = edge_smooth(&values, pre, 0.1, hysteresis, rising, either, span, length, fits);
                            let expected = smooth_oracle(&values, pre, 0.1, hysteresis, rising, either, span, length, fits);
                            same(got.0, expected.0, "smoothed oracle");
                            same(got.1.map(|e| e.t), expected.1.map(|e| e.0), "either diagnostic");
                            assert_eq!(got.1.map(|e| e.rising), expected.1.map(|e| e.1));
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1296);
}

#[test]
fn record_trigger_origin_matches_full_oracle_for_admissible_near_edges() {
    use crate::{board::Frame, model::*, wave::Raw};
    for (pre, direction) in [(60, Slope::Rise), (50_000, Slope::Fall), (99_940, Slope::Either)] {
        let values: Vec<f32> = (0..100_000).map(|i| ((i as f64 - pre as f64) * std::f64::consts::TAU / 239.0).sin() as f32).collect();
        let mut model = Model::default();
        model.slope = direction;
        model.trig_level = 0.0;
        let frame = std::sync::Arc::new(Frame { seq: 1, dt: 1e-9, pre,
            ch: [values, Vec::new(), Vec::new(), Vec::new()], ..Frame::default() });
        let window = 4096e-9;
        let first = -window / 2.0;
        let fits = |t: f64| t >= -first / frame.dt && t <= frame.ch[0].len() as f64 - 1.0 - (first + window) / frame.dt;
        let expected = smooth_oracle(&frame.ch[0], pre, 0.0, model.trig_hyst_div() * model.ch[0].volts_per_div(),
            direction != Slope::Fall, direction == Slope::Either, 2048, 1, fits).0;
        let record = crate::board::to_record(&frame, &model, 3600, window, first);
        assert!(record.triggered);
        let Raw { t0, i0, .. } = record.raw.unwrap();
        // With edges present but none leaving a complete screen in the near
        // window, the existing record path deliberately keeps hardware pre.
        same(Some(i0 as f64 - t0 / frame.dt), Some(expected.unwrap_or(pre as f64)), "retained trigger origin");
    }
}

#[test]
#[ignore = "explicit optimized host search timing; no hardware or FPS assertion"]
fn benchmark_near_and_full_edge_search() {
    use std::{hint::black_box, time::Instant};
    let count = 100_000usize;
    let pre = count / 2;
    let values: Vec<f32> = (0..count).map(|i| ((i as f64 - pre as f64) * std::f64::consts::TAU / 503.0).sin() as f32
        + if i % 2 == 0 { 0.01 } else { -0.01 }).collect();
    let fits = |t: f64| (t - pre as f64).abs() <= 2048.0;
    let near = || edge_smooth(black_box(&values), pre, 0.0, 0.1, true, false, 2048, 16, fits).0;
    let full = || edge_smooth(black_box(&values), pre, 0.0, 0.1, true, false, count, 16, fits).0;
    same(near(), full(), "near/full benchmark equivalence");
    let median = |f: &dyn Fn() -> Option<f64>| {
        let mut times = Vec::new();
        for _ in 0..9 {
            let started = Instant::now();
            for _ in 0..50 { black_box(f()); }
            times.push(started.elapsed().as_secs_f64() * 1e6 / 50.0);
        }
        times.sort_by(f64::total_cmp); times[4]
    };
    let near_us = median(&near);
    let full_us = median(&full);
    let report = format!("{{\n  \"scope\":\"optimized host CPU search only; no hardware, transport, rendering or FPS\",\n  \"samples\":{count},\n  \"pre\":{pre},\n  \"near_span\":2048,\n  \"smoothing_samples\":16,\n  \"repetitions_per_round\":50,\n  \"rounds\":9,\n  \"near_median_us\":{near_us},\n  \"full_median_us\":{full_us},\n  \"host_search_speedup\":{}\n}}\n", full_us / near_us);
    if let Ok(path) = std::env::var("OOS_EDGE_BENCH_PATH") { std::fs::write(path, &report).unwrap(); }
    println!("{report}");
}
