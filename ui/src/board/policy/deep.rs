// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The raw reads of a deep record's screen window for the mean-type measurements (deepmeas.rs): what the UI asks for
//! and how often the thread may read.

/// Least time between two raw deep reads while running, for a read of up to `PERIOD_BYTES`; a larger read waits
/// longer in proportion (`period`). On the DHO924S a raw read cost about 3 % of the records per MiB read once a second
/// (10 M at 1 ms/div, 20 MiB: a third of the records at once a second, unit 2026-11-15): so a read costs at most about
/// 6 % of them and of the screen rate.
pub const PERIOD: std::time::Duration = std::time::Duration::from_secs(1);
/// Raw bytes a read may have to be due every `PERIOD`.
pub const PERIOD_BYTES: usize = 2 << 20;
/// Largest raw read of one frame (all channels): 24 MiB, 12 M points of one channel, six C2H reads of 4 MiB. A
/// screen window above it keeps the pair measurement (mean-type values NaN).
pub const MAX_BYTES: usize = 24 << 20;

/// What the UI asks of the acquisition thread (`Shared::deep_raw`): raw deep reads, for every batch (`every`, Single:
/// the one record is the answer) or at most once per `PERIOD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub every: bool,
}

/// Least time between two raw deep reads of `bytes`: `PERIOD` per `PERIOD_BYTES`, at least `PERIOD`.
pub fn period(bytes: usize) -> std::time::Duration {
    PERIOD.mul_f64((bytes as f64 / PERIOD_BYTES as f64).max(1.0))
}

/// The acquisition thread's rate limit: a raw deep read of `bytes` is due for every batch with `every`, else when its
/// `period` has passed since the last (or none was made). `last` and `now` are microseconds of the loop's clock.
pub fn due(w: Want, last: Option<u64>, now: u64, bytes: usize) -> bool {
    w.every || last.is_none_or(|t| std::time::Duration::from_micros(now.saturating_sub(t)) >= period(bytes))
}
