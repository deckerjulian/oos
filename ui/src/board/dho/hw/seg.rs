// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The hardware segmented recording of the acquisition thread: its state, its abort and one step of it per
//! pass of the loop. A change of how segments are recorded or read back goes here.

#[cfg(any(target_os = "linux", test))]
use super::decode::decode;
#[cfg(any(target_os = "linux", test))]
use super::settings::seg_read_plan;
#[cfg(any(target_os = "linux", test))]
use super::slow::Io;
#[cfg(any(target_os = "linux", test))]
use crate::board::{lock, Frame, HwShared};
#[cfg(any(target_os = "linux", test))]
use oos_hal::dho::hz;
#[cfg(any(target_os = "linux", test))]
use oos_hal::dho::seq::Settings;

thread_local! {
    #[cfg(test)]
    static LOW_FORCE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Sets the frames `low_frames` allows for the calling thread (tests: the live run's recording into the upper frame
/// memory with small segments, `liverun.rs`).
#[cfg(test)]
pub(super) fn force_low_frames(n: Option<u32>) {
    LOW_FORCE.with(|f| f.set(n));
}

/// Frames a recording of segments of `seg` bursts keeps below 256 MiB: `hz::max_record_frames`.
pub(super) fn low_frames(seg: u32) -> u32 {
    #[cfg(test)]
    if let Some(n) = LOW_FORCE.with(|f| f.get()) {
        return n.min(oos_hal::dho::hz::max_record_frames(seg));
    }
    oos_hal::dho::hz::max_record_frames(seg)
}

/// The frame memory above 256 MiB per segment size, as `seg::probe_upper` found it: kept for the run of the program
/// (horizontal-config.md 1.8).
static UPPER: std::sync::Mutex<Vec<(u32, bool)>> = std::sync::Mutex::new(Vec::new());

/// The optional memory for System info: in use once a probe found it for a segment size, missing when every probe
/// said no.
pub fn upper_mem() -> crate::board::UpperMem {
    use crate::board::UpperMem;
    let v = crate::board::lock(&UPPER);
    if v.iter().any(|e| e.1) {
        UpperMem::InUse
    } else if v.is_empty() {
        UpperMem::NotProbed
    } else {
        UpperMem::Missing
    }
}

/// Whether segments of `seg` bursts fit above 256 MiB, once probed.
pub(super) fn upper_known(seg: u32) -> Option<bool> {
    crate::board::lock(&UPPER).iter().find(|e| e.0 == seg).map(|e| e.1)
}

/// The segments of a recording of `want` that `low` frames below 256 MiB allow (`up`: and as many above it): all of
/// them and those of the lower part. None, (0, 0), where not one segment fits below the top 1 M bursts that a record
/// must not reach (segments of 100 M and 125 M points, horizontal-config.md 1.8).
fn parts(want: u32, low: u32, up: bool) -> (u32, u32) {
    if low == 0 {
        return (0, 0);
    }
    let total = want.max(1).min(if up { 2 * low } else { low });
    (total, total.min(low))
}

/// The probe of the upper frame memory for the segments of `p`, once per segment size.
#[cfg(any(target_os = "linux", test))]
fn upper_probed<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
    (s, p): (&Settings, &hz::Params),
    state: &mut oos_hal::dho::seq::State,
    bar: &mut B,
    c2h: &mut R,
    clock: &mut C,
) -> Result<bool, String> {
    if let Some(v) = upper_known(p.seg) {
        return Ok(v);
    }
    let v = oos_hal::dho::seg::probe_upper(state, &seg_read_plan(s, p), p, bar, c2h, clock)?;
    eprintln!("oos-ui hw: segments of {} bursts above 256 MiB: {}", p.seg, if v { "yes" } else { "no" });
    crate::board::lock(&UPPER).push((p.seg, v));
    Ok(v)
}

/// A hardware segmented recording.
#[cfg(any(target_os = "linux", test))]
pub(super) struct SegRun {
    /// `HwSeg::run` it belongs to
    run: u64,
    n: u32,
    pub(super) recording: bool,
    /// segments recorded; the next one to read back
    count: u32,
    next: u32,
    /// read the frames into the queue too (the recording ended by itself, not by STOP)
    deliver: bool,
    tl: oos_hal::dho::seg::Timeline,
    /// segment size and the segments of the lower part; the rest goes above 256 MiB once the lower part is full
    seg: u32,
    low: u32,
    upper: bool,
    /// the first-trigger stamp of the lower part: an arm starts the stamp again (horizontal-config.md 1.8)
    tag: Option<u64>,
}

#[cfg(any(target_os = "linux", test))]
impl SegRun {
    /// Ring index of segment `i` (`hz::record_frame`).
    fn frame(&self, i: u32) -> u32 {
        hz::record_frame(self.seg, self.low, i)
    }
}

/// Ends a recording that is still in the FPGA before anything else writes it, and forgets the segments (their settings
/// are no longer the configured ones).
#[cfg(any(target_os = "linux", test))]
pub(super) fn seg_abort<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
    run: &mut Option<SegRun>,
    state: &mut oos_hal::dho::seq::State,
    bar: &mut B,
    c2h: &mut R,
    clock: &mut C,
    out: &HwShared,
) {
    let Some(r) = run.take() else { return };
    if r.recording {
        if let Err(e) = oos_hal::dho::seg::stop(state, bar, c2h, clock) {
            eprintln!("oos-ui hw: segments stop: {}", e);
        }
    }
    let mut g = lock(out);
    (g.seg.phase, g.seg.shown) = (crate::board::SegPhase::Idle, None);
    g.seg.times.clear();
}

/// One step of the hardware segmented recording: start on a new `HwSeg::run`, poll the progress every 10 ms (no
/// transfer), then stop and read back one batch per step (stamps by single-frame plays, frames into the queue), then
/// play single segments on request; with the settings `s` configured as `p`, the batch part's buffer `dma` and the
/// loop's handles `io`.
#[cfg(any(target_os = "linux", test))]
pub(super) fn seg_step<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
    run: &mut Option<SegRun>,
    (s, p): (&Settings, &hz::Params),
    dma: &mut oos_hal::dho::bus::DmaBuf,
    io: Io<'_, B, R, C>,
) -> Result<(), String> {
    use crate::board::SegPhase;
    use oos_hal::dho::seg;
    let Io { state, bar, c2h, clock, out, n, stamp } = io;
    let (cmd, running, want) = {
        let g = lock(out);
        (g.seg.run, g.seg.running, g.seg.want)
    };
    if running && run.as_ref().map_or(0, |r| r.run) != cmd {
        if run.as_ref().is_some_and(|r| r.recording) {
            seg::stop(state, bar, c2h, clock)?;
        }
        // below 256 MiB first; more segments than fit there go above it where the unit has that memory
        let want = s.segments.map_or(0, |g| g.n);
        let low = low_frames(p.seg);
        let up = want > low && low >= 2 && upper_probed((s, p), state, bar, c2h, clock)?;
        let (total, low) = parts(want, low, up);
        if total == 0 {
            eprintln!("oos-ui hw: segments of {} bursts do not fit below the reserved top: none recorded", p.seg);
        } else {
            seg::start_at(state, s, bar, c2h, clock, 0, low)?;
        }
        *run = Some(SegRun {
            run: cmd,
            n: total,
            recording: total > 0,
            count: 0,
            next: 0,
            deliver: true,
            tl: seg::Timeline::new(None),
            seg: p.seg,
            low,
            upper: false,
            tag: None,
        });
        let mut g = lock(out);
        g.seg = crate::board::HwSeg {
            run: g.seg.run,
            running: g.seg.running,
            done_run: cmd,
            phase: if total > 0 { SegPhase::Recording } else { SegPhase::Done },
            total,
            ..Default::default()
        };
        return Ok(());
    }
    let Some(r) = run.as_mut() else {
        clock.sleep_us(10000);
        return Ok(());
    };
    if r.recording {
        let part = if r.upper { r.n - r.low } else { r.low };
        let x = seg::status(bar, part);
        let recorded = x.recorded + if r.upper { r.low } else { 0 };
        lock(out).seg.recorded = recorded;
        if !x.complete(part) && running {
            clock.sleep_us(10000);
            return Ok(());
        }
        if !r.upper && r.n > r.low && running {
            // the lower part is full: the rest above 256 MiB; the times count from the lower part's first trigger
            r.tag = seg::first_trigger(bar);
            seg::start_at(state, s, bar, c2h, clock, hz::upper_first_frame(r.seg), r.n - r.low)?;
            r.upper = true;
            return Ok(());
        }
        let tag = if r.upper { r.tag } else { seg::first_trigger(bar) };
        seg::stop(state, bar, c2h, clock)?;
        (r.recording, r.count, r.next, r.deliver, r.tl) = (false, recorded, 0, running, seg::Timeline::new(tag));
        eprintln!(
            "oos-ui hw: segments: {} of {} recorded ({} below 256 MiB), 0x4034 {:#x}, first-trigger stamp {:?}",
            recorded, r.n, r.low, x.fsm, tag
        );
        lock(out).seg.phase = if r.count == 0 { SegPhase::Done } else { SegPhase::Reading };
        return Ok(());
    }
    let rs = seg_read_plan(s, p);
    let fb = oos_hal::dho::seq::frame_bytes(&rs, p);
    if r.next < r.count {
        r.deliver &= running;
        // a batch stays on one side of 256 MiB: its frames are consecutive in the ring
        let cnt = rs.frames.min(r.count - r.next);
        let cnt = if r.next < r.low { cnt.min(r.low - r.next) } else { cnt };
        let k0 = r.frame(r.next);
        if r.deliver && lock(out).queue.len() + cnt as usize > crate::board::QUEUE_MAX {
            // the screen has not taken the last batch yet
            clock.sleep_us(5000);
            return Ok(());
        }
        let st = seg::stamps(state, &rs, p, bar, c2h, clock, k0, cnt)?;
        if r.next == 0 {
            // hardware check 5.3: header words 5..7 against the latch, frame by frame
            for (k, x) in st.iter().take(8).enumerate() {
                eprintln!("oos-ui hw: segment {} latch {:?} header {:?}", k, x.latch, x.header);
            }
        }
        let times: Vec<Option<f64>> = st.iter().map(|x| r.tl.push(*x)).collect();
        let mut frames = Vec::new();
        if r.deliver {
            seg::read(state, &rs, p, bar, c2h, dma, clock, k0, cnt)?;
            frames = dma
                .data()
                .chunks_exact(fb)
                .take(cnt as usize)
                .map(|chunk| {
                    *n += 1;
                    Frame { stamp, ..decode(chunk, &rs, p, *n) }
                })
                .collect();
        }
        r.next += cnt;
        let mut g = lock(out);
        g.seg.times.extend(times);
        g.seg.read = r.next;
        if let Some(f) = frames.last() {
            g.frame = f.clone();
        }
        g.queue.extend(frames);
        if r.next >= r.count {
            g.seg.phase = SegPhase::Done;
        }
        return Ok(());
    }
    // done: single segments on request
    match want {
        Some(k) if k < r.count => {
            lock(out).seg.want = None;
            let one = oos_hal::dho::seg::read_settings(&rs, 1);
            seg::read(state, &one, p, bar, c2h, dma, clock, r.frame(k), 1)?;
            *n += 1;
            let f = Frame { stamp, ..decode(&dma.data()[..fb.min(dma.data().len())], &one, p, *n) };
            lock(out).seg.shown = Some((k, f));
        }
        Some(_) => lock(out).seg.want = None,
        None => clock.sleep_us(10000),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parts;

    // Break it catches: a segment that does not fit below the reserved top recorded anyway (it would reach the top
    // 1 M bursts), the upper part used without the probe's yes, more segments than both parts hold.
    #[test]
    fn parts_of_a_recording() {
        assert_eq!(parts(1000, 0, true), (0, 0), "100 M / 125 M points: nothing");
        assert_eq!(parts(6, 3, false), (3, 3));
        assert_eq!(parts(6, 3, true), (6, 3));
        assert_eq!(parts(10, 3, true), (6, 3));
        assert_eq!(parts(2, 3, true), (2, 2));
        assert_eq!(parts(0, 3, false), (1, 1), "at least one");
    }
}
