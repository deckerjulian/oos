// SPDX-License-Identifier: GPL-2.0-or-later

//! Mean-type measurements of deep records from raw samples (fork, docs/fork/deep-meas.md). A record above 1 M points
//! is read for the screen through the FPGA compression (min/max pairs), and Mean, RMS, Std dev, Area and their cycle
//! variants cannot come from pairs (`framestats::needs_samples`). With `meas.deep_raw = on` the acquisition thread
//! reads the screen window of the newest frame once more, raw, at most once per `PERIOD` while running and for every
//! batch in Single; that frame becomes the record of its batch. While such raw records come, the UI measures only
//! them (`hold`): every value of a measurement then belongs to one raw record, and the statistics count each once.
//! The live picture stays on pairs.

use crate::board::policy::compress::MAX_DEPTH;
use crate::board::policy::deep::{Want, MAX_BYTES};
use crate::board::Frame;
use crate::model::*;

thread_local! {
    /// tests: `enabled` for this thread
    #[cfg(test)]
    static FORCE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Sets `enabled` for the calling thread (tests).
#[cfg(test)]
pub fn force(on: Option<bool>) {
    FORCE.with(|f| f.set(on));
}

/// On unless `meas.deep_raw = off` in system.conf, or OOS_DEEP_RAW set to empty or 0 (verified on the DHO924S
/// 2026-11-15, docs/fork/deep-meas.md).
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(on) = FORCE.with(|f| f.get()) {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !OFF.get())
}

/// Off: the switch of `enabled` reads `off`.
const OFF: crate::sysconf::Flag = crate::sysconf::Flag { key: "meas.deep_raw", env: Some("OOS_DEEP_RAW"), on: "off" };

/// The request for the model: on, and a shown measurement (or the software DVM) needs samples. Normal acquisition
/// only: in Peak the memory holds peak pairs, not samples; Average and High res have their own records.
pub fn want(m: &Model) -> Option<Want> {
    let needs = m.meas.iter().any(|&(k, s)| s.ch().is_some_and(|c| m.ch[c].on) && crate::framestats::needs_samples(k))
        || (m.dvm_on && m.ch[m.dvm_src].on);
    (enabled() && needs && m.acq == AcqMode::Normal).then_some(Want { every: m.trig_mode == TrigMode::Single })
}

/// A frame of the deep-memory read through the compression (min/max pairs of a record above 1 M points).
pub fn deep_pairs(f: &Frame) -> bool {
    f.envelope && !f.roll && !f.scan && f.acq.1 > MAX_DEPTH as usize
}

/// A raw read of a deep record's screen window (the record of its batch while raw deep reads come).
pub fn deep_raw_frame(f: &Frame) -> bool {
    !f.envelope && !f.roll && !f.scan && f.acq.1 > MAX_DEPTH as usize
}

/// Points of the raw screen window `hw` reads for a deep record (screen and a quarter screen each side, at least 4096
/// points, within the record), and whether its `channels` fit `MAX_BYTES`. The thread's budget counts the interleave
/// ratio (`hz::ratio`): three channels are read as four.
pub fn fits(rate: f64, tdiv: f64, memsel: usize, channels: usize) -> bool {
    let screen = (tdiv * HDIV as f64 * rate).ceil() as usize;
    let n = (screen + 2 * (screen / 4).max(4096)).min(memsel);
    2 * n * channels.max(1).next_power_of_two() <= MAX_BYTES
}

/// Whether the UI leaves the newest record unmeasured: raw deep reads are asked for and running, and the record is
/// of min/max pairs whose screen window a raw read can hold. The measurement shown then stays that of the last raw
/// record (at most about `PERIOD` old).
pub fn hold(m: &Model, f: Option<&Frame>) -> bool {
    let Some(f) = f.filter(|f| deep_pairs(f)) else { return false };
    // the thread reads no raw window while the LA records (`deep_raw_settings` in board/dho/hw/plan.rs): holding for
    // one would freeze the measurements
    if m.la_on() {
        return false;
    }
    let channels = f.ch.iter().filter(|v| !v.is_empty()).count();
    m.run == RunState::Run
        && want(m).is_some_and(|w| !w.every)
        && fits(f.acq.0, m.hw_time_per_div() as f64, f.acq.1, channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::policy::deep::*;

    fn deep(envelope: bool) -> Frame {
        Frame {
            envelope,
            acq: (1.25e9, 10_000_000),
            ch: [vec![0.0; 16], Vec::new(), Vec::new(), Vec::new()],
            ..Default::default()
        }
    }

    #[test]
    fn asked_for_only_when_on_and_a_measurement_needs_samples() {
        OFF.check_both_files();
        assert!(!OFF.of(None, "meas.deep_raw = on\n"));
        force(Some(true));
        let mut m = Model::default();
        m.ch[0].on = true;
        m.meas = vec![(Meas::Vmax, crate::model::WaveId::Ch(0))];
        assert_eq!(want(&m), None, "max needs no samples");
        m.meas.push((Meas::Vavg, crate::model::WaveId::Ch(0)));
        assert_eq!(want(&m), Some(Want { every: false }));
        m.trig_mode = TrigMode::Single;
        assert_eq!(want(&m), Some(Want { every: true }));
        m.acq = AcqMode::Peak;
        assert_eq!(want(&m), None);
        m.acq = AcqMode::Normal;
        m.ch[0].on = false;
        assert_eq!(want(&m), None);
        (m.ch[0].on, m.meas, m.dvm_on, m.dvm_src) = (true, Vec::new(), true, 0);
        assert!(want(&m).is_some(), "the software DVM reads the mean");
        force(Some(false));
        assert_eq!(want(&m), None);
        force(None);
    }

    #[test]
    fn hold_while_running_on_pair_records_that_a_raw_read_can_replace() {
        force(Some(true));
        let mut m = Model::default();
        m.ch[0].on = true;
        m.meas = vec![(Meas::Vrms, crate::model::WaveId::Ch(0))];
        m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        // 10 M at 1 ms/div: the 10 M record is the window, 20 MB
        assert!(hold(&m, Some(&deep(true))));
        assert!(!hold(&m, Some(&deep(false))), "a raw record is measured");
        assert!(
            !hold(&m, Some(&Frame { acq: (1.25e9, 1_000_000), ..deep(true) })),
            "live compression: the newest frame is raw anyway"
        );
        assert!(!hold(&m, None));
        m.run = RunState::Stop;
        assert!(!hold(&m, Some(&deep(true))));
        m.run = RunState::Run;
        m.trig_mode = TrigMode::Single;
        assert!(!hold(&m, Some(&deep(true))), "Single: every batch is read raw");
        m.trig_mode = TrigMode::Auto;
        // 125 M: 250 MB, beyond the budget: the pair measurement stays
        assert!(!hold(&m, Some(&Frame { acq: (1.25e9, 125_000_000), ..deep(true) })));
        // the LA on: the thread reads no raw window, so nothing is held for one
        if crate::ui::la::available() {
            m.la.on = true;
            assert!(!hold(&m, Some(&deep(true))), "LA on");
            m.la.on = false;
        }
        force(None);
    }

    #[test]
    fn budget_and_rate() {
        assert!(fits(1.25e9, 1e-3, 10_000_000, 1));
        assert!(!fits(1.25e9, 1e-3, 10_000_000, 2));
        assert!(fits(625e6, 1e-3, 5_000_000, 2));
        assert!(!fits(1.25e9, 10e-3, 125_000_000, 1));
        // three channels are read as four (the interleave ratio): 3.5 M points are 28 MB, not 21 MB
        assert!(!fits(1.25e9, 1e-3, 3_500_000, 3));
        assert!(fits(1.25e9, 1e-3, 3_000_000, 3));
        let t = 5_000_000u64;
        let us = |d: std::time::Duration| d.as_micros() as u64;
        let w = Want { every: false };
        let small = PERIOD_BYTES / 2;
        assert!(due(w, None, t, small));
        assert!(!due(w, Some(t), t + 999_000, small));
        assert!(due(w, Some(t), t + us(PERIOD), small));
        assert!(due(Want { every: true }, Some(t), t, 10 * PERIOD_BYTES));
        // a read of ten times the bytes waits ten times as long
        assert!(!due(w, Some(t), t + us(PERIOD * 9), 10 * PERIOD_BYTES));
        assert!(due(w, Some(t), t + us(PERIOD * 10), 10 * PERIOD_BYTES));
    }
}
