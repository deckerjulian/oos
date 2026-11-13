// SPDX-License-Identifier: GPL-2.0-or-later

//! Mean-type measurements of deep records from raw samples (fork, docs/fork/deep-meas.md). A record above 1 M points
//! is read for the screen through the FPGA compression (min/max pairs), and Mean, RMS, Std dev, Area and their cycle
//! variants cannot come from pairs (`framestats::needs_samples`). With `meas.deep_raw = on` the acquisition thread
//! reads the screen window of the newest frame once more, raw, at most once per `PERIOD` while running and for every
//! batch in Single; that frame becomes the record of its batch. While such raw records come, the UI measures only
//! them (`hold`): every value of a measurement then belongs to one raw record, and the statistics count each once.
//! The live picture stays on pairs.

use crate::board::Frame;
use crate::model::*;

/// Least time between two raw deep reads while running: one raw read of up to `MAX_BYTES` a second keeps the batch
/// rate of the pair reads.
pub const PERIOD: std::time::Duration = std::time::Duration::from_secs(1);
/// Largest raw read of one frame (all channels): 24 MiB, 12 M points of one channel, six C2H reads of 4 MiB. A
/// screen window above it keeps the pair measurement (mean-type values NaN).
pub const MAX_BYTES: usize = 24 << 20;

/// What the UI asks of the acquisition thread (`Shared::deep_raw`): raw deep reads, for every batch (`every`, Single:
/// the one record is the answer) or at most once per `PERIOD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub every: bool,
}

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

/// `meas.deep_raw = on` in system.conf, or OOS_DEEP_RAW set (any value but empty or 0). Off until verified on the unit.
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(on) = FORCE.with(|f| f.get()) {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let env = std::env::var("OOS_DEEP_RAW").ok().map(|v| if v.is_empty() || v == "0" { "off" } else { "on" });
        let conf = crate::sysconf::fork_text();
        enabled_from(env, &conf)
    })
}

/// `env` if set, else the `meas.deep_raw` line of `conf`; off unless `on`.
pub fn enabled_from(env: Option<&str>, conf: &str) -> bool {
    crate::drawpipe::conf_value(env, conf, "meas.deep_raw").as_deref() == Some("on")
}

/// The request for the model: on, and a shown measurement (or the software DVM) needs samples. Normal acquisition
/// only: in Peak the memory holds peak pairs, not samples; Average and High res have their own records.
pub fn want(m: &Model) -> Option<Want> {
    let needs = m.meas.iter().any(|&(k, c)| m.ch[c].on && crate::framestats::needs_samples(k)) || (m.dvm_on && m.ch[m.dvm_src].on);
    (enabled() && needs && m.acq == AcqMode::Normal).then_some(Want { every: m.trig_mode == TrigMode::Single })
}

/// A frame of the deep-memory read through the compression (min/max pairs of a record above 1 M points).
pub fn deep_pairs(f: &Frame) -> bool {
    f.envelope && !f.roll && !f.scan && f.acq.1 > crate::compress::MAX_DEPTH as usize
}

/// Points of the raw screen window `hw` reads for a deep record (screen and a quarter screen each side, at least 4096
/// points, within the record), and whether its `channels` fit `MAX_BYTES`.
pub fn fits(rate: f64, tdiv: f64, memsel: usize, channels: usize) -> bool {
    let screen = (tdiv * HDIV as f64 * rate).ceil() as usize;
    let n = (screen + 2 * (screen / 4).max(4096)).min(memsel);
    2 * n * channels.max(1) <= MAX_BYTES
}

/// Whether the UI leaves the newest record unmeasured: raw deep reads are asked for and running, and the record is
/// of min/max pairs whose screen window a raw read can hold. The measurement shown then stays that of the last raw
/// record (at most about `PERIOD` old).
pub fn hold(m: &Model, f: Option<&Frame>) -> bool {
    let Some(f) = f.filter(|f| deep_pairs(f)) else { return false };
    let channels = f.ch.iter().filter(|v| !v.is_empty()).count();
    m.run == RunState::Run && want(m).is_some_and(|w| !w.every) && fits(f.acq.0, m.hw_time_per_div() as f64, f.acq.1, channels)
}

/// The acquisition thread's rate limit: a raw deep read is due for every batch with `every`, else when `PERIOD` has
/// passed since the last (or none was made).
pub fn due(w: Want, last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    w.every || last.is_none_or(|t| now.duration_since(t) >= PERIOD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deep(envelope: bool) -> Frame {
        Frame { envelope, acq: (1.25e9, 10_000_000), ch: [vec![0.0; 16], Vec::new(), Vec::new(), Vec::new()], ..Default::default() }
    }

    #[test]
    fn asked_for_only_when_on_and_a_measurement_needs_samples() {
        assert!(!enabled_from(None, ""));
        assert!(enabled_from(None, "meas.deep_raw = on\n"));
        assert!(!enabled_from(Some("off"), "meas.deep_raw = on\n"));
        force(Some(true));
        let mut m = Model::default();
        m.ch[0].on = true;
        m.meas = vec![(Meas::Vmax, 0)];
        assert_eq!(want(&m), None, "max needs no samples");
        m.meas.push((Meas::Vavg, 0));
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
        m.meas = vec![(Meas::Vrms, 0)];
        m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        // 10 M at 1 ms/div: the 10 M record is the window, 20 MB
        assert!(hold(&m, Some(&deep(true))));
        assert!(!hold(&m, Some(&deep(false))), "a raw record is measured");
        assert!(!hold(&m, Some(&Frame { acq: (1.25e9, 1_000_000), ..deep(true) })), "live compression: the newest frame is raw anyway");
        assert!(!hold(&m, None));
        m.run = RunState::Stop;
        assert!(!hold(&m, Some(&deep(true))));
        m.run = RunState::Run;
        m.trig_mode = TrigMode::Single;
        assert!(!hold(&m, Some(&deep(true))), "Single: every batch is read raw");
        m.trig_mode = TrigMode::Auto;
        // 125 M: 250 MB, beyond the budget: the pair measurement stays
        assert!(!hold(&m, Some(&Frame { acq: (1.25e9, 125_000_000), ..deep(true) })));
        force(None);
    }

    #[test]
    fn budget_and_rate() {
        assert!(fits(1.25e9, 1e-3, 10_000_000, 1));
        assert!(!fits(1.25e9, 1e-3, 10_000_000, 2));
        assert!(fits(625e6, 1e-3, 5_000_000, 2));
        assert!(!fits(1.25e9, 10e-3, 125_000_000, 1));
        let t = std::time::Instant::now();
        let w = Want { every: false };
        assert!(due(w, None, t));
        assert!(!due(w, Some(t), t + std::time::Duration::from_millis(999)));
        assert!(due(w, Some(t), t + PERIOD));
        assert!(due(Want { every: true }, Some(t), t));
    }
}
