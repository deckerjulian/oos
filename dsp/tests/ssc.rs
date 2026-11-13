// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Analytic FM edges, independent of the SIM generator's triangle integration.

use oos_dsp::eye::{self, Clock};
use std::f64::consts::PI;

const UI: f64 = 1.0 / 20e6;
const MODULATION: f64 = 33e3;
const DEPTH: f64 = 0.005;

fn edge_time(index: usize, modulated: bool) -> f64 {
    let nominal = index as f64 * UI;
    // d(time)/d(nominal) = 1 + DEPTH*cos(...), so local frequency
    // spans nominal/(1+DEPTH) to nominal/(1-DEPTH): about 10,000 ppm.
    nominal + if modulated { DEPTH / (2.0 * PI * MODULATION) * (2.0 * PI * MODULATION * nominal).sin() } else { 0.0 }
}

fn clock(edges: Vec<f64>) -> Clock {
    Clock { period: UI, phase: 0.0, edges, ticks: Vec::new(), first_rising: true }
}

#[test]
fn hinted_ui_rate_twice_the_clock_cycle_rate_keeps_ssc_depth_and_hertz() {
    let c = clock((0..20_002).map(|i| edge_time(i, true)).collect());
    let (ppm, series) = eye::ssc_deviation(&c);
    assert!((ppm - 10_000.0).abs() < 50.0, "{ppm} ppm, expected about 10,000");
    assert!((series.iter().sum::<f64>() / series.len() as f64).abs() < 1e-4);
    let frequency = eye::ssc_rate(&c);
    assert!((frequency / MODULATION - 1.0).abs() < 0.02, "{frequency} Hz");
    // A full-cycle hint changes the inferred UI count, not the relative depth.
    let cycle_hint = Clock { period: 2.0 * UI, ..c };
    assert!((eye::ssc_deviation(&cycle_hint).0 - ppm).abs() < 1e-4);
    assert!((eye::ssc_rate(&cycle_hint) / MODULATION - 1.0).abs() < 0.02);
}

fn data_clock(modulated: bool) -> Clock {
    let bits = oos_dsp::hss::prbs_bits(7, 60_002).unwrap();
    let edges = bits.windows(2).enumerate().filter_map(|(i, bits)| {
        (bits[0] != bits[1]).then(|| edge_time(i + 1, modulated))
    }).collect();
    clock(edges)
}

#[test]
fn prbs_run_lengths_do_not_create_false_ssc() {
    let c = data_clock(false);
    let (ppm, _) = eye::ssc_deviation(&c);
    assert!(ppm.abs() < 1e-3, "unmodulated PRBS has {ppm} ppm");
    assert!(eye::ssc_rate(&c).is_nan());
}

#[test]
fn nonuniform_prbs_transitions_keep_the_physical_modulation_frequency() {
    let c = data_clock(true);
    let (ppm, _) = eye::ssc_deviation(&c);
    assert!((ppm - 10_000.0).abs() < 150.0, "modulated PRBS has {ppm} ppm");
    let frequency = eye::ssc_rate(&c);
    assert!((frequency / MODULATION - 1.0).abs() < 0.02, "{frequency} Hz");
}
