// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Nominal serial records from `oos_dsp::serial` (the SIM menu).

use oos_dsp::eye::{self, Clock};
use oos_dsp::serial::{self, LANES};

fn pcie4() -> &'static serial::Lane {
    LANES.iter().find(|l| l.name == "PCIe 4.0").expect("PCIe 4.0")
}

#[test]
fn pcie4_record_covers_one_full_ssc_period_at_eight_samples_per_ui() {
    let lane = pcie4();
    let v = serial::generate(lane);
    assert_eq!(v.len(), serial::sample_count(lane).unwrap());
    let dt = serial::dt_of(lane);
    assert!((dt * lane.bits_per_s - 0.125).abs() < 1e-12);
    let period = 1.0 / lane.ssc_hz;
    assert!(v.len() as f64 * dt >= period);
    assert!((v.len() - 1) as f64 * dt < period);
    let (lo, hi) = v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |s, &x| (s.0.min(x), s.1.max(x)));
    assert!(hi > 0.2 && lo < -0.2, "rails {lo} {hi}");
    assert!(hi < 0.8 && lo > -0.8, "overshoot {lo} {hi}");
}

#[test]
fn pcie4_inner_rails_open_an_eye() {
    // This check isolates pulse shaping and the analog limit. Full-period SSC
    // length/modulation are checked independently, without an expensive full-eye fold.
    let mut lane = *pcie4();
    lane.ssc = false;
    let v = serial::generate(&lane);
    let dt = serial::dt_of(&lane);
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

#[test]
fn every_preset_has_a_bounded_grid_and_plain_records_keep_their_length() {
    for lane in LANES {
        let n = serial::sample_count(lane).unwrap();
        assert!(n <= serial::MAX_SAMPLES, "{}: {n}", lane.name);
        if lane.ssc {
            let dt = serial::dt_of(lane);
            assert!(n as f64 * dt >= 1.0 / lane.ssc_hz);
            assert!((n - 1) as f64 * dt < 1.0 / lane.ssc_hz);
        } else {
            assert_eq!(n, 65_536);
        }
    }
    let mut lane = LANES[2];
    lane.ssc = false;
    assert_eq!(serial::try_generate(&lane).unwrap().len(), 65_536);
}

#[test]
fn invalid_and_excessive_inputs_fail_explicitly() {
    let mut lane = LANES[2];
    lane.ssc_hz = 1.0;
    assert!(serial::sample_count(&lane).unwrap_err().contains("sample limit"));
    assert!(serial::try_generate(&lane).unwrap_err().contains("sample limit"));
    lane = LANES[2];
    lane.noise_rms = f64::NAN;
    assert!(serial::try_generate(&lane).unwrap_err().contains("finite"));
    lane = LANES[2];
    lane.ssc = false;
    lane.ssc_deviation = f64::INFINITY;
    assert!(serial::sample_count(&lane).unwrap_err().contains("finite"));
    lane = LANES[2];
    lane.v_low = lane.v_high;
    assert!(serial::try_generate(&lane).unwrap_err().contains("high level"));
    lane = LANES[2];
    lane.ssc = false;
    lane.rj_rms = 1e-9;
    assert!(serial::try_generate(&lane).unwrap_err().contains("reorders"));
    lane = LANES[2];
    lane.bits_per_s = 64e9;
    lane.rise = 2e-9;
    assert!(serial::sample_count(&lane).unwrap_err().contains("work limit"));
    assert!(serial::try_generate(&lane).unwrap_err().contains("work limit"));
    lane = LANES[2];
    lane.bw = 4.0 * lane.bits_per_s;
    assert!(serial::sample_count(&lane).unwrap_err().contains("Nyquist"));
    assert!(serial::try_generate(&lane).unwrap_err().contains("Nyquist"));
}

#[test]
fn serial_controls_reach_the_shared_engine() {
    let mut lane = LANES[2];
    lane.bits_per_s = 100e6;
    lane.ssc_hz = 1e6;
    lane.ssc_deviation = 0.02;
    lane.noise_rms = 0.0;
    lane.rj_rms = 0.0;
    lane.pj_peak = 0.0;
    lane.bw = 0.0;
    lane.rise = 50e-12;
    let down = serial::try_generate(&lane).unwrap();
    lane.ssc_center = true;
    let center = serial::try_generate(&lane).unwrap();
    assert_eq!(down.len(), center.len());
    assert_ne!(down, center);
    lane.ssc = false;
    let clean = serial::try_generate(&lane).unwrap();
    lane.dcd_pp = 500e-12;
    let dcd = serial::try_generate(&lane).unwrap();
    assert_ne!(clean, dcd);
    lane.dcd_pp = 0.0;
    lane.corr_rj_rms = 100e-12;
    lane.corr_rj_hz = 1e6;
    let correlated = serial::try_generate(&lane).unwrap();
    assert_ne!(clean, correlated);
    assert_eq!(correlated, serial::try_generate(&lane).unwrap());
    lane.corr_rj_hz = lane.bits_per_s;
    assert!(serial::sample_count(&lane).unwrap_err().contains("half the symbol rate"));
    assert!(serial::try_generate(&lane).unwrap_err().contains("half the symbol rate"));
}

#[test]
#[ignore = "local debug responsiveness benchmark"]
fn generation_timing() {
    for index in [2, 3, 4] {
        for ssc in [false, true] {
            let mut lane = LANES[index];
            lane.ssc = ssc;
            let start = std::time::Instant::now();
            let wave = serial::try_generate(&lane).unwrap();
            eprintln!("{} SSC={ssc}: {} samples in {:?}", lane.name, wave.len(), start.elapsed());
        }
    }
}
