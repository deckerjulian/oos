// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the acquisition thread reads: arming ahead and disarming, the batch plan, frames left out of a batch,
//! the zone batch, the screen window of a deep record, and the reads again at STOP and for the deep
//! measurements. A new kind of read is planned here.

use super::decode::LIVE_READ;
use super::settings::raw_newest;
#[cfg(any(target_os = "linux", test))]
use crate::board::Frame;
use oos_hal::dho::hz;
use oos_hal::dho::seq::Settings;

/// Arm the next batch before the decode of the current one (`seq::prearm`), so the decode is no longer dead time.
/// `acq.prearm = on` in system.conf, or OOS_PREARM set (any value). Off by default: the stop of an acquisition armed
/// ahead that is then not read (a settings change, the generator, self-cal, roll) is not verified on the unit yet
///.
pub(super) fn prearm_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OOS_PREARM").is_some() || PREARM.get())
}

pub(super) const PREARM: crate::sysconf::Flag = crate::sysconf::Flag { key: "acq.prearm", env: None, on: "on" };

/// Stops the acquisition armed ahead, if one is, before anything else writes the FPGA.
#[cfg(target_os = "linux")]
pub(super) fn disarm(
    armed: &mut Option<(Settings, u64)>,
    state: &mut oos_hal::dho::seq::State,
    bar: &mut oos_hal::dho::bus::Bar1,
    c2h: &mut std::fs::File,
) {
    if armed.take().is_some() {
        if let Err(e) = oos_hal::dho::seq::disarm(state, bar, c2h, |_| {}) {
            eprintln!("oos-ui hw: disarm: {}", e);
        }
    }
}

/// Batch settings for a configured acquisition: frames per batch (at most ~4 MB per read, the frame ring, 1000 /
/// ratio) and, for records above 1 M points, what of the record is read for the screen:
/// - the screen holds most of the record: the whole record through the FPGA compression to ~`LIVE_READ` points;
/// - the screen is a small part of it (a fixed depth at a short time base, at the full rate): the screen window with a
///   margin, every sample. Compressed, 10 M at 100 ns/div left about 80 min/max points for the screen.
pub fn batch_plan(s: &Settings, p: &hz::Params) -> Settings {
    let mut b = s.clone();
    if p.memsel > 1_000_000 {
        match screen_window(s, p) {
            Some(w) => b.window = Some(w),
            None => b.compress = p.memsel.div_ceil(LIVE_READ),
        }
    }
    let fb = oos_hal::dho::seq::frame_bytes(&b, p).max(1);
    // hardware average: one frame is read whatever the batch holds, so the read size does not limit it
    let by_size =
        if matches!(s.mode, oos_hal::dho::acq::Mode::Average { .. }) { u32::MAX } else { (4_000_000 / fb) as u32 };
    b.frames = by_size.min(p.max_frames).min(1000 / p.ratio.max(1)).max(1);
    // SINGLE shows the first frame after the press: one frame per batch. A full batch recorded up to 248 more triggers
    // at 1 us/div (`acquired`, unit 2026-11-13) and waited up to 25 ms longer for the ring before the read
    if s.single {
        b.frames = 1;
    }
    // the zone trigger: the ring gets one qualified frame per arm on the unit (round 3: 29 frames/s, the 25 ms wait
    // for more frames per batch), so a batch is `zone_batch` frames (docs/fork/hwzone.md "Rate")
    if s.hwzone.is_some() {
        b.frames = b.frames.min(zone_batch());
    }
    b
}

/// Longest record whose first batch after a change of settings is left out (`read_with` in `spawn`), in seconds.
pub(super) const FIRST_LEFT_MAX_S: f64 = 0.05;

/// Whether frame 0 of a batch of several frames is left out (issue #5): on unless `acq.frame0 = keep` in
/// system.conf. In a small batch it is kept when its edge is at the trigger point (`frame0_left_out`).
pub(super) fn frame0_drop() -> bool {
    static K: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *K.get_or_init(|| !FRAME0_KEEP.get())
}

pub(super) const FRAME0_KEEP: crate::sysconf::Flag = crate::sysconf::Flag { key: "acq.frame0", env: None, on: "keep" };

#[cfg(any(target_os = "linux", test))]
/// Largest batch whose frame 0 is checked at the trigger point instead of left out unseen (issue #220). Frame 0 is the
/// frame the FPGA held across the read-out and the arm (issue #5): usually a good trigger from just before the arm,
/// now and then joined with older memory. At slow time bases a batch holds few frames (1 ms/div, 100 kpts: the held
/// one and one more in the 25 ms wait), so leaving frame 0 out halved the records in the picture (unit 2026-11-18:
/// 18 of 37 a second with 1 channel). Up to this size frame 0 is a quarter of the batch or more; a longer batch keeps
/// the rule of #5 as it was verified on the unit (batches of 26 to 32 frames at 10 us/div).
pub(super) const FRAME0_CHECK_MAX_FRAMES: usize = 4;

/// Whether frame 0 of a batch of `avail` frames is worth the edge check (one more decode): the rule of #5 is on
/// (`drop`), the batch holds several frames and at most `FRAME0_CHECK_MAX_FRAMES`, and the record is short enough for
/// the decode (`FRAME1_CHECK_MAX` points).
#[cfg(any(target_os = "linux", test))]
pub(super) fn frame0_check_wanted(avail: usize, drop: bool, memsel: u32) -> bool {
    drop && (2..=FRAME0_CHECK_MAX_FRAMES).contains(&avail) && memsel <= FRAME1_CHECK_MAX
}

/// Whether frame 0 of a batch of `avail` frames is left out: never with the rule of #5 off (`drop` false) or in a
/// batch of one (its only frame; the first batch after a change of settings has a rule of its own); in a batch of up
/// to `FRAME0_CHECK_MAX_FRAMES` frames only when the edge check (`edge` of frame 0, None when not checked) does not put
/// its edge at the trigger point; always in a longer batch. A trigger the check cannot judge leaves frame 0 out, as
/// before.
#[cfg(any(target_os = "linux", test))]
pub(super) fn frame0_left_out(avail: usize, drop: bool, edge: Option<Edge>) -> bool {
    drop && avail > 1 && !(avail <= FRAME0_CHECK_MAX_FRAMES && edge == Some(Edge::At))
}

/// What the edge check of a frame found (`edge_check`).
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edge {
    /// a trigger type, source or coupling the check cannot judge, or a frame too short for it
    Unchecked,
    /// a crossing of the level in the trigger's direction near the trigger point
    At,
    /// none: the frame is not the trigger it claims to be
    Off,
}

#[cfg(any(target_os = "linux", test))]
/// Samples around the trigger point in which frame 1's edge must lie (`frame1_off_trigger`); the hardware puts it
/// within a few samples (unit 2026-11-16: +6.5 at 6.25 MSa/s).
const FRAME1_SPAN: usize = 32;

#[cfg(any(target_os = "linux", test))]
/// Largest record whose frame 1 the batch path decodes for `frame1_off_trigger` (and frame 0 of a small batch for
/// `frame0_left_out`): one more decode per batch, cheap for the short records batches are made of; a longer record is
/// searched for its edge by the fast path anyway.
pub(super) const FRAME1_CHECK_MAX: u32 = 100_000;

/// Counts a frame 1 left out (`frame1_off_trigger`) and logs the count at 1, 2, 4, 8, ...
#[cfg(target_os = "linux")]
pub(super) fn frame1_left_out() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if n.is_power_of_two() {
        eprintln!(
            "oos-ui hw: frame 1 of a batch without its edge at the trigger point, left out (issue #5): {} so far",
            n
        );
    }
}

#[cfg(any(target_os = "linux", test))]
/// Issue #5, the rest: frame 1 of a batch (the first trigger after the arm) is now and then joined with older memory.
/// On the unit (2026-11-16, generator 1 kHz square on CH1, 1 kpts, 10 us/div, edge trigger) 27 of about 12,700
/// batches had no edge of the trigger slope near the trigger point in frame 1 (61 of 13,500 with the trigger point
/// moved right), and no other frame ever. True when `f` is such a frame: an edge trigger on a channel with DC or HF
/// reject coupling and no rising (falling, either) crossing of the level within `FRAME1_SPAN` samples of `pre`. Other
/// trigger types cannot be checked this way and keep their frame 1.
pub(super) fn frame1_off_trigger(f: &Frame, s: &Settings) -> bool {
    edge_check(f, s) == Edge::Off
}

#[cfg(any(target_os = "linux", test))]
/// The check of `frame1_off_trigger`: whether `f` has a crossing of the level in the trigger's direction within
/// `FRAME1_SPAN` samples of `pre`, for an edge trigger on a channel with DC or HF reject coupling.
pub(super) fn edge_check(f: &Frame, s: &Settings) -> Edge {
    use oos_hal::dho::tpu::{Coupling, Slope};
    if !matches!(s.trigger, oos_hal::dho::seq::Trig::Edge)
        || !(1..=4).contains(&s.trig_src)
        || !matches!(s.trig_couple, Coupling::Dc | Coupling::HfReject)
        || f.envelope
    {
        return Edge::Unchecked;
    }
    let tc = (s.trig_src - 1) as usize;
    let v = &f.ch[tc];
    if v.len() < 2 * FRAME1_SPAN || f.pre >= v.len() {
        return Edge::Unchecked;
    }
    // a fifth of a division: an edge the trigger took crosses it, noise on a flat level near the level does not
    let hyst = (0.2 * s.ch[tc].scale) as f32;
    crate::fastdraw::edge_near(
        v,
        f.pre,
        s.level as f32,
        hyst,
        s.slope != Slope::Falling,
        s.slope == Slope::Either,
        FRAME1_SPAN,
    )
    .map_or(Edge::Off, |_| Edge::At)
}

/// Frames per batch while the zone trigger runs: `zone.batch = <n>` in system.conf, default 1.
pub(super) fn zone_batch() -> u32 {
    static N: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *N.get_or_init(|| zone_batch_from(&crate::sysconf::fork_text()))
}

pub(super) fn zone_batch_from(conf: &str) -> u32 {
    crate::sysconf::value(conf, "zone.batch").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).clamp(1, 1000)
}

/// The part of a deep record the screen shows, as a read-out window (`Settings::window`: points, first point from the
/// trigger point), when it fits `LIVE_READ` points: the screen and a quarter of a screen on each side (at least 4096
/// points), widened to the trigger point and 4096 points around it for the software trigger (a delay moves the screen
/// away from it).
/// The record is centred on the screen (`hz::prep`: it starts half a record before the delay). None with the LA,
/// which reads its own window of the same record.
fn screen_window(s: &Settings, p: &hz::Params) -> Option<(u32, i64)> {
    window_below(s, p, LIVE_READ as i64)
}

/// `screen_window` for windows of fewer than `cap` points.
fn window_below(s: &Settings, p: &hz::Params, cap: i64) -> Option<(u32, i64)> {
    if s.la.is_some() || p.rate == 0 {
        return None;
    }
    let screen = (s.tdiv * 10.0 * p.rate as f64).ceil() as i64;
    let margin = (screen / 4).max(4096);
    let (mem, pre, mid) = (p.memsel as i64, p.pre as i64, p.memsel as i64 / 2);
    // a multiple of 8 points like the stock TX length, inside the record
    let lo = ((mid - screen / 2 - margin).min(pre - 4096).max(0)) / 8 * 8;
    let hi = (mid + screen / 2 + margin).max(pre + 4096).min(mem);
    let n = (hi - lo + 7) / 8 * 8;
    if n >= cap || lo + n > mem {
        return None;
    }
    Some((n as u32, lo - pre))
}

/// STOP: what of the stored record (parameters `p`, read live with the settings `b`) to read again for a view of
/// `tdiv` per division at `delay`, and where the trigger lies in the frame that read gives (in its points, may be
/// outside it). The view with a quarter screen on each side at every sample when that fits `LIVE_READ` points; else
/// the whole record through the FPGA compression (as the live read of a long record). The record is laid out from
/// the trigger point `p.pre` at `p.rate`, so any part of it can be read, the trigger need not be in the window.
pub fn stop_plan(b: &Settings, p: &hz::Params, tdiv: f64, delay: f64) -> (Settings, f64) {
    let mut r = b.clone();
    (r.frames, r.window, r.compress) = (1, None, 1);
    let rate = p.rate as f64;
    let screen = (tdiv * 10.0 * rate).ceil();
    let margin = (screen / 4.0).max(4096.0);
    let (mem, pre) = (p.memsel as f64, p.pre as f64);
    let centre = pre + delay * rate;
    let lo = ((centre - screen / 2.0 - margin).max(0.0) / 8.0).floor() * 8.0;
    let hi = (centre + screen / 2.0 + margin).min(mem);
    if hi > lo {
        let n = ((hi - lo) / 8.0).ceil() * 8.0;
        if n < LIVE_READ as f64 && lo + n <= mem {
            r.window = Some((n as u32, lo as i64 - p.pre as i64));
            return (r, pre - lo);
        }
    }
    if p.memsel > LIVE_READ && r.play4 {
        r.compress = p.memsel.div_ceil(LIVE_READ);
    }
    let k = r.compress.max(1) as f64;
    (r, pre / k)
}

/// Whether arming ahead (`acq.prearm`) leaves the replay at STOP possible: not for a batch of the live compression or
/// one whose newest frame the deep measurements may read raw (`deep_raw_settings`), since the next batch would write
/// the ring under it. Prearm gains at
/// short time bases; deep compressed records take most of a batch in the read anyway.
pub(super) fn prearm_keeps_replay(b: &Settings, p: &hz::Params, deep: Option<crate::deepmeas::Want>) -> bool {
    !raw_newest(b, p) && (deep.is_none() || deep_raw_settings(b, p).is_none())
}

/// The raw read at STOP (docs/fork/stop-exact.md) of the newest frame of the batch `b` with `count` frames read just
/// before, when that frame was read as pairs and not already raw and nothing was armed ahead (`ahead`: the ring is
/// being written again): the whole frame for the live compression, the screen window for a deep record when the deep
/// measurements want samples.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn final_replay(
    b: &Settings,
    p: &hz::Params,
    count: u32,
    newest_raw: bool,
    ahead: bool,
    deep: Option<crate::deepmeas::Want>,
) -> Option<Settings> {
    if count == 0 || newest_raw || ahead {
        return None;
    }
    if raw_newest(b, p) {
        return Some(oos_hal::dho::seq::raw_settings(b));
    }
    deep.and_then(|_| deep_raw_settings(b, p))
}

/// The raw read of a deep record's screen window for the measurements (deepmeas.rs): the batch `b` read the whole
/// record through the compression (`batch_plan`, above 1 M points), Normal acquisition, no LA; the window of
/// `screen_window` with the record's edge allowed, at most `deepmeas::MAX_BYTES`. Settings for `seq::read_raw_again`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn deep_raw_settings(b: &Settings, p: &hz::Params) -> Option<Settings> {
    if b.compress < 2
        || b.window.is_some()
        || p.memsel <= crate::compress::MAX_DEPTH
        || !matches!(b.mode, oos_hal::dho::acq::Mode::Normal)
    {
        return None;
    }
    let cap = (crate::deepmeas::MAX_BYTES / (2 * p.ratio.max(1) as usize)) as i64 + 1;
    // the whole record when the window would reach past it (`window_below` refuses a window beyond the end)
    let w = window_below(b, p, cap)
        .or_else(|| ((p.memsel as i64) < cap && b.la.is_none()).then(|| (p.memsel, -(p.pre as i64))))?;
    Some(oos_hal::dho::seq::raw_settings(&Settings { window: Some(w), ..b.clone() }))
}
