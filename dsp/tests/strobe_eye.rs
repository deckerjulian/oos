//! DDR strobe eye: DQ folded on DQS that runs in bursts, each burst at its own phase, idling at mid-level between
//! bursts (a differential strobe with both legs parked low), as an LPDDR write capture looks.

use oos_dsp::eye::{self, Eye};

const DT: f64 = 10e-12;
const UI: f64 = 375e-12;
const EDGES: usize = 16;
const BURSTS: usize = 24;

/// (DQS, DQ) samples. DQS toggles every UI inside a burst and sits at 0 V between bursts; DQ changes half a UI
/// before each strobe edge, so the strobe is centred in the data eye.
fn capture() -> (Vec<f32>, Vec<f32>) {
    let n = (BURSTS as f64 * 60e-9 / DT) as usize;
    let mut dqs = vec![0.0f32; n];
    let mut lfsr = 0x5au8;
    let mut bit = || { let b = ((lfsr >> 7) ^ (lfsr >> 5) ^ (lfsr >> 4) ^ (lfsr >> 3)) & 1; lfsr = (lfsr << 1) | b; b };
    // DQ transitions (time, level after): one chance per strobe edge, half a UI before it
    let mut changes: Vec<(f64, f32)> = Vec::new();
    for b in 0..BURSTS {
        // bursts 60 ns apart, each at an unrelated phase (no constant-rate clock fits them all)
        let t0 = 5e-9 + b as f64 * 60e-9 + ((b * b * 31 + 7 * b + 13) % 97) as f64 / 97.0 * UI;
        let (i0, i1) = (((t0 - UI) / DT) as usize, ((t0 + (EDGES as f64 + 1.0) * UI) / DT) as usize);
        for i in i0..i1.min(n) {
            let k = ((i as f64 * DT - t0) / UI).floor();
            // strobe: +0.5 / -0.5 alternating each UI inside the burst
            dqs[i] = if k < 0.0 || k >= EDGES as f64 { 0.0 } else if (k as i64) % 2 == 0 { 0.5 } else { -0.5 };
        }
        for e in 0..EDGES {
            changes.push((t0 + e as f64 * UI - 0.5 * UI, if bit() == 1 { 0.5 } else { 0.0 }));
        }
    }
    // DQ everywhere: the level of the last change, with a 60 ps linear transition centred on each change
    let mut dq = vec![0.0f32; n];
    let (mut c, mut level) = (0usize, 0.0f32);
    for (i, out) in dq.iter_mut().enumerate() {
        let t = i as f64 * DT;
        while c < changes.len() && changes[c].0 + 30e-12 <= t { level = changes[c].1; c += 1; }
        *out = match changes.get(c) {
            Some(&(edge, to)) if t > edge - 30e-12 => level + (to - level) * ((t - edge) / 60e-12 + 0.5) as f32,
            _ => level,
        };
    }
    (dqs, dq)
}

fn fold(dq: &[f32], clock: &eye::Clock) -> eye::EyeMeas {
    let mut e = Eye::new(256, 192, 2, -0.1, 0.6);
    let gated = eye::gate_around_clock(dq, DT, clock, 1.5);
    let centred = eye::center_strobe(clock);
    e.accumulate(&gated, DT, &centred);
    e.measure(clock.period)
}

#[test]
fn a_bursting_strobe_folds_each_burst_on_its_own_edges() {
    let (dqs, dq) = capture();
    // the page's strobe comparator: mid-swing with 20 % hysteresis (DQS idles right on that mid level)
    let clock = eye::recover_clock(&dqs, DT, 0.0, 0.2, None).expect("strobe edges");
    // every toggle plus the step out of idle into each burst (the first burst has no level before it)
    assert_eq!(clock.edges.len(), BURSTS * EDGES - 1, "only real strobe edges: no chatter at the idle level");
    assert!((clock.period / UI - 1.0).abs() < 0.01, "UI {}", clock.period);

    let strobe = fold(&dq, &eye::strobe_clock(&clock));
    assert!(strobe.width > 0.6 * UI, "strobe fold opens the eye: width {:e}", strobe.width);
    assert!(strobe.height > 0.4, "height {}", strobe.height);

    // one constant-rate clock across bursts of arbitrary phase smears the crossings over the UI
    let constant = fold(&dq, &clock);
    assert!(!(constant.width > 0.3 * UI), "a single clock cannot line up the bursts: width {:e}", constant.width);

    // the strobe's own jitter inside its bursts is zero here; against one clock it spans a whole UI
    let ideal = eye::strobe_ideal(&clock);
    let tie = eye::tie(&clock.edges, &ideal);
    assert!(tie.iter().all(|t| t.abs() < 0.05 * UI), "max TIE {:e}", tie.iter().fold(0.0f64, |m, t| m.max(t.abs())));
}

#[test]
fn strobe_ticks_stay_ordered_and_one_ui_apart_in_and_around_bursts() {
    let (dqs, _) = capture();
    let clock = eye::recover_clock(&dqs, DT, 0.0, 0.2, None).unwrap();
    for c in [eye::strobe_clock(&clock), eye::strobe_ideal(&clock)] {
        assert!(c.ticks.windows(2).all(|w| w[1] > w[0]), "ticks ascend");
        let spacing: Vec<f64> = c.ticks.windows(2).map(|w| w[1] - w[0]).collect();
        // inside bursts and next to them one UI; only the middle of a long idle gap may be longer
        let near_ui = spacing.iter().filter(|s| (*s / UI - 1.0).abs() < 0.05).count();
        assert!(near_ui as f64 > 0.95 * spacing.len() as f64, "{} of {}", near_ui, spacing.len());
    }
}

