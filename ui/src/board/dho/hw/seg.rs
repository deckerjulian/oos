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
        let total = seg::start(state, s, p, bar, c2h, clock)?;
        *run = Some(SegRun {
            run: cmd,
            n: total,
            recording: true,
            count: 0,
            next: 0,
            deliver: true,
            tl: seg::Timeline::new(None),
        });
        let mut g = lock(out);
        g.seg = crate::board::HwSeg {
            run: g.seg.run,
            running: g.seg.running,
            done_run: cmd,
            phase: SegPhase::Recording,
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
        let x = seg::status(bar, r.n);
        lock(out).seg.recorded = x.recorded;
        if !x.complete(r.n) && running {
            clock.sleep_us(10000);
            return Ok(());
        }
        let tag = seg::first_trigger(bar);
        seg::stop(state, bar, c2h, clock)?;
        (r.recording, r.count, r.next, r.deliver, r.tl) = (false, x.recorded, 0, running, seg::Timeline::new(tag));
        eprintln!(
            "oos-ui hw: segments: {} of {} recorded, 0x4034 {:#x}, first-trigger stamp {:?}",
            x.recorded, r.n, x.fsm, tag
        );
        lock(out).seg.phase = if r.count == 0 { SegPhase::Done } else { SegPhase::Reading };
        return Ok(());
    }
    let rs = seg_read_plan(s, p);
    let fb = oos_hal::dho::seq::frame_bytes(&rs, p);
    if r.next < r.count {
        r.deliver &= running;
        let cnt = rs.frames.min(r.count - r.next);
        if r.deliver && lock(out).queue.len() + cnt as usize > crate::board::QUEUE_MAX {
            // the screen has not taken the last batch yet
            clock.sleep_us(5000);
            return Ok(());
        }
        let st = seg::stamps(state, &rs, p, bar, c2h, clock, r.next, cnt)?;
        if r.next == 0 {
            // hardware check 5.3: header words 5..7 against the latch, frame by frame
            for (k, x) in st.iter().take(8).enumerate() {
                eprintln!("oos-ui hw: segment {} latch {:?} header {:?}", k, x.latch, x.header);
            }
        }
        let times: Vec<Option<f64>> = st.iter().map(|x| r.tl.push(*x)).collect();
        let mut frames = Vec::new();
        if r.deliver {
            seg::read(state, &rs, p, bar, c2h, dma, clock, r.next, cnt)?;
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
            seg::read(state, &one, p, bar, c2h, dma, clock, k, 1)?;
            *n += 1;
            let f = Frame { stamp, ..decode(&dma.data()[..fb.min(dma.data().len())], &one, p, *n) };
            lock(out).seg.shown = Some((k, f));
        }
        Some(_) => lock(out).seg.want = None,
        None => clock.sleep_us(10000),
    }
    Ok(())
}
