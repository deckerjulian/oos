// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Nominal high-speed serial records for the SIM menu. A preset is a public rate, a PRBS, rails, a tanh edge,
//! a Butterworth limit, one-tap de-emphasis, full-period SSC, and seeded independent/correlated jitter.
//! The bits are the PRBS. There is no line coding.

use crate::hss::{self, FilterKind, Jitter, Settings, Ssc};

/// Maximum samples in one generated record; larger settings fail before allocation.
pub const MAX_SAMPLES: usize = hss::MAX_SAMPLES;

/// One menu row. Numbers are SI: bits per second, volts, seconds, hertz.
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub name: &'static str,
    pub bits_per_s: f64,
    pub prbs: u8,
    pub v_high: f64,
    pub v_low: f64,
    /// 20–80 % isolated edge, seconds
    pub rise: f64,
    pub noise_rms: f64,
    /// Butterworth cutoff, hertz
    pub bw: f64,
    /// de-emphasis of a repeated symbol, dB. Zero leaves the rail.
    pub deemph_db: f64,
    pub ssc: bool,
    /// False preserves the HSS down-spread convention with its DC term removed
    /// (a total span of `ssc_deviation`); true uses +/- `ssc_deviation`.
    pub ssc_center: bool,
    /// Fraction, e.g. 0.005 means 0.5%.
    pub ssc_deviation: f64,
    pub ssc_hz: f64,
    /// Duty-cycle distortion, seconds peak-to-peak; rising edges move late.
    pub dcd_pp: f64,
    /// Correlated random jitter, seconds RMS, and physical low-pass corner in hertz.
    pub corr_rj_rms: f64,
    pub corr_rj_hz: f64,
    /// random jitter, seconds RMS
    pub rj_rms: f64,
    /// periodic jitter peak, seconds, and its frequency, hertz. Zero peak skips it.
    pub pj_peak: f64,
    pub pj_hz: f64,
    pub seed: u64,
}

pub const LANES: &[Lane] = &[
    Lane { name: "PCIe 1.0", bits_per_s: 2.5e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 120e-12, noise_rms: 4.15625e-3, bw: 2e9, deemph_db: 0.0, ssc: true, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 1 },
    Lane { name: "PCIe 2.0", bits_per_s: 5.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 60e-12, noise_rms: 4.3125e-3, bw: 4e9, deemph_db: 3.5, ssc: true, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 2 },
    Lane { name: "PCIe 3.0", bits_per_s: 8.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 35e-12, noise_rms: 4.5e-3, bw: 6e9, deemph_db: 3.5, ssc: true, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: hss::PCIE3_SEED },
    Lane { name: "PCIe 4.0", bits_per_s: 16.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 20e-12, noise_rms: 5e-3, bw: 12e9, deemph_db: 4.5, ssc: true, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 4 },
    Lane { name: "PCIe 5.0", bits_per_s: 32.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 12e-12, noise_rms: 6e-3, bw: 22e9, deemph_db: 6.0, ssc: true, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 5 },
    Lane { name: "USB 2.0 HS", bits_per_s: 480e6, prbs: 7, v_high: 0.4, v_low: -0.4, rise: 500e-12, noise_rms: 2e-3, bw: 250e6, deemph_db: 0.0, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 8e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 6 },
    Lane { name: "USB 3.2 Gen 1", bits_per_s: 5.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 60e-12, noise_rms: 4e-3, bw: 3.5e9, deemph_db: 0.0, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 2e-12, pj_peak: 4e-12, pj_hz: 2.5e6, seed: 7 },
    Lane { name: "USB 3.2 Gen 2", bits_per_s: 10.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 30e-12, noise_rms: 5e-3, bw: 7e9, deemph_db: 0.0, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.5e-12, pj_peak: 4e-12, pj_hz: 5e6, seed: 8 },
    Lane { name: "SATA I", bits_per_s: 1.5e9, prbs: 7, v_high: 0.4, v_low: -0.4, rise: 150e-12, noise_rms: 3e-3, bw: 1e9, deemph_db: 0.0, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 4e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 9 },
    Lane { name: "SATA II", bits_per_s: 3.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 90e-12, noise_rms: 3e-3, bw: 2e9, deemph_db: 0.0, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 3e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 10 },
    Lane { name: "SATA III", bits_per_s: 6.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 45e-12, noise_rms: 4e-3, bw: 4e9, deemph_db: 0.0, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 2.5e-12, pj_peak: 6e-12, pj_hz: 3e6, seed: 11 },
    Lane { name: "10GBASE-KR", bits_per_s: 10.3125e9, prbs: 31, v_high: 0.425, v_low: -0.425, rise: 30e-12, noise_rms: 5e-3, bw: 7.5e9, deemph_db: 3.5, ssc: false, ssc_center: false, ssc_deviation: 0.005, ssc_hz: 33e3, dcd_pp: 0.0, corr_rj_rms: 0.0, corr_rj_hz: 1e6, rj_rms: 1.2e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 12 },
];

pub fn names() -> Vec<&'static str> {
    LANES.iter().map(|l| l.name).collect()
}

/// Sample interval that keeps eight samples per unit interval.
pub fn dt_of(lane: &Lane) -> f64 {
    1.0 / (hss::SAMPLES_PER_UI as f64 * lane.bits_per_s)
}

fn settings(lane: &Lane) -> Settings {
    Settings {
        name: lane.name,
        data_rate_gbps: lane.bits_per_s / 1e9,
        prbs_order: lane.prbs as u32,
        v_high_mv: lane.v_high * 1e3,
        v_low_mv: lane.v_low * 1e3,
        rise_time_ps: lane.rise * 1e12,
        noise_rms_mv: lane.noise_rms * 1e3,
        bandwidth_ghz: lane.bw / 1e9,
        filter: FilterKind::Butterworth,
        filter_order: 3,
        deemph_db: lane.deemph_db,
        ssc: lane.ssc.then_some(Ssc {
            deviation_percent: lane.ssc_deviation * 100.0,
            freq_khz: lane.ssc_hz / 1e3,
            profile: if lane.ssc_center { "Center-spread" } else { "Down-spread only" },
        }),
        jitter: vec![
            Jitter::Rj { ps_rms: lane.rj_rms * 1e12 },
            Jitter::Pj { freq_khz: lane.pj_hz / 1e3, ps_peak: lane.pj_peak * 1e12 },
            Jitter::Dcd { ps: lane.dcd_pp * 1e12 },
            Jitter::CorrRj { ps_rms: lane.corr_rj_rms * 1e12, corner_khz: lane.corr_rj_hz / 1e3 },
        ],
        seed: lane.seed,
    }
}

/// Checked record length. SSC covers a complete modulation period; without SSC
/// the record remains 65,536 points. Invalid or excessive settings return an error.
pub fn sample_count(lane: &Lane) -> Result<usize, String> {
    validate_lane(lane)?;
    hss::try_sample_grid(&settings(lane)).map(|(_, n)| n)
}

/// Generate the SIM record through the common HSS engine. Samples are volts.
/// User-facing callers must keep the error so invalid settings cannot replace a
/// good reference with an empty or silently altered record.
pub fn try_generate(lane: &Lane) -> Result<Vec<f32>, String> {
    validate_lane(lane)?;
    let settings = settings(lane);
    let (dt, n) = hss::try_sample_grid(&settings)?;
    hss::generate(&settings, dt, n)
}

fn validate_lane(lane: &Lane) -> Result<(), String> {
    // Check disabled controls too, so NaN/Inf never survives a saved configuration
    // and unexpectedly becomes active when another control is toggled.
    for (name, value) in [
        ("data rate", lane.bits_per_s), ("high level", lane.v_high),
        ("low level", lane.v_low), ("rise time", lane.rise),
        ("noise RMS", lane.noise_rms), ("bandwidth", lane.bw),
        ("de-emphasis", lane.deemph_db), ("SSC deviation", lane.ssc_deviation),
        ("SSC frequency", lane.ssc_hz), ("random jitter", lane.rj_rms),
        ("periodic jitter", lane.pj_peak), ("periodic jitter frequency", lane.pj_hz),
        ("duty-cycle distortion", lane.dcd_pp), ("correlated jitter", lane.corr_rj_rms),
        ("correlated jitter corner", lane.corr_rj_hz),
    ] {
        if !value.is_finite() {
            return Err(format!("{name} must be finite"));
        }
    }
    if lane.ssc_deviation < 0.0 || lane.ssc_deviation >= 1.0 || lane.ssc_hz < 0.0 {
        return Err("SSC deviation must be in [0, 1), and frequency must be nonnegative".into());
    }
    Ok(())
}

/// Compatibility for fixed, valid presets and tests. User-facing inputs should
/// call [`try_generate`] and display its error instead of panicking.
pub fn generate(lane: &Lane) -> Vec<f32> {
    try_generate(lane).expect("invalid serial SIM settings; use try_generate for user inputs")
}
