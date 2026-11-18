// Copyright (C) 2026 dekajoule
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
    let edges = bits
        .windows(2)
        .enumerate()
        .filter(|&(_i, bits)| bits[0] != bits[1])
        .map(|(i, _bits)| edge_time(i + 1, modulated))
        .collect();
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

/// `n` edges `ui` apart, frequency-modulated by a sine of `hz` that spans `ppm` (peak to peak), each edge moved by
/// Gaussian timing noise of `jitter` UI RMS (xorshift + Box-Muller, fixed seed).
fn fm_clock(n: usize, ui: f64, hz: f64, ppm: f64, jitter: f64) -> Clock {
    let depth = ppm * 1e-6 / 2.0;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut uniform = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    let edges = (0..n)
        .map(|i| {
            let t = i as f64 * ui;
            let noise = (-2.0 * uniform().ln()).sqrt() * (2.0 * PI * uniform()).cos();
            t + depth / (2.0 * PI * hz) * (2.0 * PI * hz * t).sin() + jitter * ui * noise
        })
        .collect();
    Clock { period: ui, phase: 0.0, edges, ticks: Vec::new(), first_rising: true }
}

/// The deviation and the rate are read on the modulation, not on single edges or the nearest spectral bin (the span
/// of single cycles read the edge jitter: 30,940 ppm for this 5,027 ppm clock with 0.5 % UI jitter; the strongest bin
/// read 4.58 kHz for 5 kHz, 33.57 kHz for 33.3 kHz between two bins, and 29.3 kHz for 33 kHz with three periods in
/// the record).
#[test]
fn deviation_and_rate_are_read_on_the_modulation() {
    let clean = fm_clock(40_000, 1e-8, 5e3, 5_027.0, 0.0);
    let (ppm, _) = eye::ssc_deviation(&clean);
    assert!((ppm - 5_019.0).abs() < 2.0, "{ppm} ppm for 5,027 (a sine reads 0.2 % under over 1/32 of its period)");
    let rate = eye::ssc_rate(&clean);
    assert!((rate / 5e3 - 1.0).abs() < 0.001, "{rate} Hz for 5 kHz");
    let noisy = fm_clock(40_000, 1e-8, 5e3, 5_027.0, 0.005);
    let (ppm, _) = eye::ssc_deviation(&noisy);
    assert!((ppm / 5_027.0 - 1.0).abs() < 0.03, "{ppm} ppm for 5,027 with edge jitter");
    assert!((eye::ssc_rate(&noisy) / 5e3 - 1.0).abs() < 0.01, "{} Hz with edge jitter", eye::ssc_rate(&noisy));
    let between_bins = eye::ssc_rate(&fm_clock(20_002, UI, 33.3e3, 10_000.0, 0.0));
    assert!((between_bins / 33.3e3 - 1.0).abs() < 0.001, "{between_bins} Hz for 33.3 kHz over 33 periods");
    for (n, periods) in [(1_820, 3.0), (1_516, 2.5), (728, 1.2)] {
        let short = fm_clock(n, UI, MODULATION, 10_000.0, 0.0);
        let rate = eye::ssc_rate(&short);
        assert!((rate / MODULATION - 1.0).abs() < 0.002, "{rate} Hz for 33 kHz with {periods} periods in the record");
    }
}
