// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Nominal serial records from `oos_dsp::serial` (the SIM menu).

use oos_dsp::eye::{self, Clock};
use oos_dsp::serial::{self, LANES};

fn pcie4() -> &'static serial::Lane {
    LANES.iter().find(|l| l.name == "PCIe 4.0").expect("PCIe 4.0")
}

#[test]
fn pcie4_record_is_65536_samples_at_eight_per_ui() {
    let lane = pcie4();
    let v = serial::generate(lane);
    assert_eq!(v.len(), 65_536);
    let dt = serial::dt_of(lane);
    assert!((dt * lane.bits_per_s - 0.125).abs() < 1e-12);
    let (lo, hi) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |s, &x| (s.0.min(x), s.1.max(x)));
    assert!(hi > 0.2 && lo < -0.2, "rails {lo} {hi}");
    assert!(hi < 0.8 && lo > -0.8, "overshoot {lo} {hi}");
}

#[test]
fn pcie4_inner_rails_open_an_eye() {
    let lane = pcie4();
    let v = serial::generate(lane);
    let dt = serial::dt_of(lane);
    let c = eye::recover_clock(&v, dt, 0.0, 0.05, Some(lane.bits_per_s)).expect("clock");
    assert!((c.period - 1.0 / 16e9).abs() / (1.0 / 16e9) < 0.02, "UI {}", c.period);
    let (lo, hi) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |s, &x| (s.0.min(x), s.1.max(x)));
    let mut eye = eye::Eye::new(160, 160, 2, lo, hi);
    eye.accumulate(&v, dt, &c);
    let m = eye.measure(c.period);
    assert!(m.height > 0.15, "height {}", m.height);
    assert!(m.width / c.period > 0.2, "width {}", m.width / c.period);
    let _c: Clock = c;
}

#[test]
fn names_are_public_rates() {
    let joined = serial::names().join("|").to_ascii_lowercase();
    for bad in ["hss", "waveformengine", "pyside", "visa"] {
        assert!(!joined.contains(bad), "{bad} in {joined}");
    }
}
