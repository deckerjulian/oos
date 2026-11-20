// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! One batch of the acquisition thread after its read (docs/protocols/acquisition-start.md): the LA read, the raw reads
//! of the newest frame (live compression, the deep measurements' window), the units' readings, the hardware average,
//! arming ahead, the frames left out (issue #5, #220) and the queue. The state between batches is here too. A change
//! of what a batch delivers goes here.

use super::decode::{codes_into, decode, decode_into, decode_with, la_record, POOL};
use super::plan::{
    deep_raw_settings, edge_check, frame0_check_wanted, frame0_drop, frame0_left_out, frame1_left_out,
    frame1_left_out_unseen, frame1_off_trigger, prearm_keeps_replay, prearm_on, FIRST_LEFT_MAX_S, FRAME1_CHECK_MAX,
};
use super::settings::raw_newest;
use super::slow::Io;
use super::units::{hw_status_on, mask_map, unit_key, UnitGuard};
use super::{execute, port, report, since};
use crate::board::{Frame, QUEUE_MAX, QUEUE_SAMPLES};
use oos_hal::dho::bus::{Bus, DmaBuf};
use oos_hal::dho::hz;
use oos_hal::dho::seq::{self, Settings};
use std::sync::Arc;

/// The batches between passes of the loop: the read buffer, the raw second read, when the last raw reads were, the
/// settings of the last batch read and whether its record was left out as the first with new ones, the read-out
/// buffers made so far, the LA reads failed in a row, the SINGLE stamp whose batch brought its frame, the batch read by
/// the previous pass, and this thread's share of the deposit work while it waits.
pub(super) struct BatchRun {
    /// the read buffer of the batches, kept (bus::DmaBuf)
    pub(super) dma: DmaBuf,
    /// the raw second read of the newest frame of a compressed batch (compress.rs)
    pub(super) raw_dma: DmaBuf,
    /// the last raw read of a deep record's screen window for the measurements (deepmeas.rs)
    deep_last: Option<u64>,
    read_with: Option<Settings>,
    first_left: bool,
    /// the last raw read of the newest frame of a live-compressed batch (compress.rs, at most one per RAW_PERIOD)
    comp_last: Option<u64>,
    /// read-out buffers made so far: this one, and up to `POOL` in all lent to the deposit workers in batch mode
    pool_made: usize,
    pub(super) la_fails: u32,
    /// the SINGLE stamp whose batch brought its frame (`single_wants_more`)
    pub(super) single_done: Option<u64>,
    /// the batch read by the previous pass and nothing since (settings, parameters, frames, newest frame read raw)
    pub(super) fresh: Option<(Settings, hz::Params, u32, bool)>,
    /// this thread's share of the deposit work while it waits (drawpipe.rs)
    pub(super) helper: crate::drawpipe::Helper,
}

impl Default for BatchRun {
    fn default() -> BatchRun {
        BatchRun {
            dma: DmaBuf::new(),
            raw_dma: DmaBuf::new(),
            deep_last: None,
            read_with: None,
            first_left: false,
            comp_last: None,
            pool_made: 1,
            la_fails: 0,
            single_done: None,
            fresh: None,
            helper: Default::default(),
        }
    }
}

/// What a batch changes of the loop's other state: the batch armed ahead, the hardware average, the learnt offsets of
/// the ADC cores, the last batch a STOP reads again (`stop.rs`) and the mask/zone unit's stall guard.
pub(super) struct Held<'a> {
    pub(super) armed: &'a mut Option<(Settings, u64)>,
    pub(super) avg: &'a mut Option<(oos_hal::dho::avg::Tracker, u64, u32)>,
    pub(super) cores: &'a mut crate::board::dho::cores::Cores,
    pub(super) last_batch: &'a mut Option<(Settings, hz::Params, u32)>,
    pub(super) guard: &'a mut UnitGuard,
}

impl BatchRun {
    /// The batch just read: `count` frames of the settings `b` configured as `p`, read in `batch_ms`.
    pub(super) fn take<P: port::Port>(
        &mut self,
        io: Io<'_, P::Bus, P::Stream, P::Clock>,
        lines: &mut P::Lines,
        batch: (&Settings, &hz::Params, u32, f32),
        held: Held<'_>,
    ) {
        let Io { state, bar, c2h, clock, out, n: seq_n, stamp } = io;
        let Held { armed, avg, cores, last_batch, guard } = held;
        let (b, p, count, batch_ms) = (batch.0, *batch.1, batch.2, batch.3);
        guard.batch(false, false, unit_key(b, mask_map()));
        crate::compress::note_batch_k(if raw_newest(b, &p) { b.compress } else { 1 });
        let (la_words, raw_frame, deep_want) = self.reads((state, bar, c2h), clock, out, (b, &p, count), cores);
        unit_readings(state, bar, out, (b, &p), count, batch_ms);
        // hardware average: one frame, shown or not, the average cleared after a complete hold average
        let after = average(avg, (b, &p, count), self.dma.data(), bar);
        // the next batch records while this one is decoded (after the LA read, which needs this record);
        // not in average mode, where a clear may have to come first, and not where the deep measurements
        // read the newest frame raw (its ring must stay for the replay at STOP)
        if count > 0 && b.single {
            self.single_done = Some(stamp);
        }
        if prearm_on() && avg.is_none() && !b.single && prearm_keeps_replay(b, &p, deep_want) {
            match seq::prearm(state, b, &p, bar, c2h, |_| {}) {
                Ok(()) => *armed = Some((b.clone(), stamp)),
                Err(e) => report(out, format!("prearm: {}", e)),
            }
        }
        let fb = seq::frame_bytes(b, &p);
        // decode before taking the lock: the UI must not wait for 100 k-point conversions;
        // every recorded frame was read; in average mode only the newest goes to the screen
        let shown = match after {
            Some(a) if !a.show => 0,
            _ => count,
        };
        let avail = self.dma.data().len().checked_div(fb).map_or(0, |n| (shown as usize).min(n));
        crate::drawpipe::Counters::add(&crate::drawpipe::counters().acquired, avail as u64);
        // batch mode (drawpipe.rs): the newest frame to the queue, the others with the buffer itself to the
        // deposit workers, the next batch read into a buffer they gave back
        let pipe = crate::drawpipe::pipe().filter(|x| after.is_none() && avail > 1 && x.accepting());
        // the raw newest frame stands for the newest compressed one (same record)
        let raw_frame = raw_frame.filter(|_| avail as u32 == count);
        let mut skip = 0usize;
        // frame 0 of a small batch: kept when its edge is at the trigger point (issue #220), so a batch
        // of two at 1 ms/div does not lose every other record
        let f0_drop = after.is_none() && frame0_drop();
        let f0_edge = frame0_check_wanted(avail, f0_drop, p.memsel)
            .then(|| edge_check(&decode(&self.dma.data()[..fb], b, &p, 0), b));
        let f0_left = frame0_left_out(avail, f0_drop, f0_edge);
        let mut frames: Vec<Frame> = if let Some(pipe) = pipe {
            *seq_n += avail as u64;
            // the record of measurements: with the ADC-core correction, as on the path without batches
            let mc = out.lock().unwrap().cores;
            let newest = match raw_frame {
                Some(f) => Frame { stamp, seq: *seq_n, ..f },
                None => {
                    let last = &self.dma.data()[(avail - 1) * fb..avail * fb];
                    Frame { stamp, ..decode_with(last, b, &p, *seq_n, mc.then_some(&mut *cores)) }
                }
            };
            // frame 1: left out when its edge is not at the trigger point. In a batch of two it is the
            // newest, checked before the workers are fed, so a frame 0 its edge check kept goes with it
            // (the workers take a run of frames; issue #220)
            let f1_off = frame0_drop()
                && match avail {
                    2 => frame1_off_trigger(&newest, b),
                    a => {
                        frame1_left_out_unseen(a, true)
                            || (a > 2
                                && p.memsel <= FRAME1_CHECK_MAX
                                && frame1_off_trigger(&decode(&self.dma.data()[fb..2 * fb], b, &p, 0), b))
                    }
                };
            let next = pipe.take_buffer().and_then(|x| x.downcast::<oos_hal::dho::bus::DmaBuf>().ok()).map(|x| *x);
            let next = next.or_else(|| {
                (self.pool_made < POOL).then(|| {
                    self.pool_made += 1;
                    oos_hal::dho::bus::DmaBuf::new()
                })
            });
            match next {
                Some(next) => {
                    let full = std::mem::replace(&mut self.dma, next);
                    let (bs, ps) = (b.clone(), p);
                    let (cs, cp) = (b.clone(), p);
                    let codes: crate::drawpipe::CodesInto =
                        Arc::new(move |c: &[u8], f: &mut Frame| codes_into(c, &cs, &cp, 0, f));
                    // frame 0 left out as on the path without batches (issue #5; unit 2026-11-16: the
                    // one stray frame of 7,075 in batch mode was frame 0 of its batch), unless its edge
                    // check kept it; a frame 1 off the trigger takes frame 0 with it (the workers take a
                    // run of frames); frame 1 itself is counted here only when the workers hold it
                    let f1_fed = f1_off && avail > 2;
                    skip = (f0_left || f1_off) as usize + f1_fed as usize;
                    if f1_fed {
                        frame1_left_out(frame1_left_out_unseen(avail, true));
                    }
                    pipe.feed_codes(
                        Box::new(full),
                        fb,
                        skip,
                        avail - 1,
                        stamp,
                        Arc::new(move |c: &[u8], f: &mut Frame| decode_into(c, &bs, &ps, 0, f)),
                        Some(codes),
                    );
                }
                None => crate::drawpipe::Counters::add(&crate::drawpipe::counters().dropped, avail as u64 - 1),
            }
            // two frames: the newest is frame 1
            if avail == 2 && f1_off {
                frame1_left_out(false);
                skip += 1;
                Vec::new()
            } else {
                vec![newest]
            }
        } else {
            let (all, left) = self.decoded(
                (b, &p, stamp),
                (avail, fb, f0_left, f0_drop),
                after.is_some(),
                raw_frame,
                seq_n,
                (out, cores),
            );
            skip = left;
            all
        };
        skip += self.first_of_settings(&mut frames, (b, &p), avail, after.is_some());
        if after.is_some_and(|a| a.clear) {
            if let Err(e) = execute::<P>(&state.avg_clear_steps(), bar, None, c2h, lines, clock) {
                report(out, format!("average clear: {}", e));
            }
        }
        if let (Some(w), Some(f)) = (la_words, frames.last_mut()) {
            f.la = Some(la_record(w, b, &p));
        }
        // the newest frame becomes the record: its six full-frame readings (PR #8, `BasicStats`) here, off
        // the frame loop; the fast path folds the other frames itself
        if let Some(f) = frames.last_mut() {
            f.basic = std::array::from_fn(|c| crate::board::BasicStats::of(&f.ch[c]));
        }
        if count > 0 {
            *last_batch = Some((b.clone(), p, count - 1));
        }
        queue(out, frames, count, skip);
    }

    /// The reads of a batch before anything arms the FPGA again: the LA words of the same record, and the newest frame
    /// raw where the live compression or the deep measurements want it. Also what the deep measurements want, and the
    /// batch as read (`fresh`).
    fn reads<B: Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
        &mut self,
        (state, bar, c2h): (&mut seq::State, &mut B, &mut R),
        clock: &mut C,
        out: &crate::board::HwShared,
        (b, p, count): (&Settings, &hz::Params, u32),
        cores: &mut crate::board::dho::cores::Cores,
    ) -> (Option<Vec<u16>>, Option<Frame>, crate::board::DeepWant) {
        let p = *p;
        // LA on: the second, raw read of the same record before anything else touches the FPGA
        // after a few failures in a row the LA read stops until the next set-up: every failed read waits for
        // its time-out, and the analog frames must not wait with it
        let la_words = match b.la {
            Some(_) if self.la_fails < 3 => match seq::acquire_la(state, b, &p, bar, c2h, |_| {}) {
                Ok(w) => {
                    self.la_fails = 0;
                    Some(w)
                }
                Err(e) => {
                    self.la_fails += 1;
                    report(out, format!("LA read ({} of 3): {}", self.la_fails, e));
                    None
                }
            },
            _ => None,
        };
        // live compression: the newest frame once more, raw, before anything arms the FPGA again; at most
        // once per compress::RAW_PERIOD (each read cost 7-9 % of the frames at 1 channel on the unit)
        let raw_frame = if raw_newest(b, &p)
            && count > 0
            && self.comp_last.is_none_or(|t| since(clock.now_us(), t) >= crate::compress::RAW_PERIOD)
        {
            self.comp_last = Some(clock.now_us());
            // the flag first: a guard taken in the argument would be held for the whole decode
            let mc = out.lock().unwrap().cores;
            match seq::read_raw_again(state, b, &p, bar, c2h, &mut self.raw_dma, count - 1, |_| {}) {
                Ok(()) => {
                    Some(decode_with(self.raw_dma.data(), &seq::raw_settings(b), &p, 0, mc.then_some(&mut *cores)))
                }
                Err(e) => {
                    report(out, format!("raw read of the newest frame: {}", e));
                    None
                }
            }
        } else {
            None
        };
        // deep record and mean-type measurements wanted: the screen window of the newest frame raw, at most
        // once per deepmeas::PERIOD while running (deepmeas.rs)
        let deep_want = out.lock().unwrap().deep_raw;
        let now_deep = clock.now_us();
        let deep = deep_want.filter(|_| count > 0).and_then(|w| {
            deep_raw_settings(b, &p)
                .filter(|ds| crate::deepmeas::due(w, self.deep_last, now_deep, seq::frame_bytes(ds, &p)))
        });
        let raw_frame = match deep {
            Some(ds) if raw_frame.is_none() => {
                self.deep_last = Some(clock.now_us());
                let mc = out.lock().unwrap().cores;
                match seq::read_raw_again(state, &ds, &p, bar, c2h, &mut self.raw_dma, count - 1, |_| {}) {
                    Ok(()) => Some(decode_with(self.raw_dma.data(), &ds, &p, 0, mc.then_some(&mut *cores))),
                    Err(e) => {
                        report(out, format!("raw read of the deep window: {}", e));
                        None
                    }
                }
            }
            _ => raw_frame,
        };
        self.fresh = Some((b.clone(), p, count, raw_frame.is_some()));
        (la_words, raw_frame, deep_want)
    }

    /// The frames of a batch decoded on this thread (no deposit workers): only the newest ones the screen takes, frame 0
    /// and frame 1 left out where they hold older memory (issue #5, #220), the raw newest frame in place of the newest
    /// compressed one. Also the number of frames read and not decoded or left out.
    fn decoded(
        &mut self,
        (b, p, stamp): (&Settings, &hz::Params, u64),
        (avail, fb, f0_left, f0_drop): (usize, usize, bool, bool),
        averaged: bool,
        raw_frame: Option<Frame>,
        seq_n: &mut u64,
        (out, cores): (&crate::board::HwShared, &mut crate::board::dho::cores::Cores),
    ) -> (Vec<Frame>, usize) {
        let p = *p;
        // only the newest frames the screen takes are decoded (`Shared::take`; on the fast cores,
        // which the frame loop needs), in average mode the newest one
        let (take, match_cores) = {
            let g = out.lock().unwrap();
            (if g.take == 0 { avail } else { g.take.min(avail) }, g.cores)
        };
        let take = if averaged { take.min(1) } else { take };
        // frame 0 of a batch is the frame the FPGA held across the stop, read-out and arm: recorded
        // before this arm, sometimes joined with older memory (a stray trace; issue #5, mj1911). With
        // more frames in the batch it is left out (1 of 26 at 1 kpts, 10 us/div); in a small batch only
        // when its edge is not at the trigger point (`f0_left` above, issue #220)
        let stale = f0_left as usize;
        let mut skip = (avail - take).max(stale);
        *seq_n += skip as u64;
        let mut all: Vec<Frame> = self
            .dma
            .data()
            .chunks_exact(fb.max(1))
            .take(avail)
            .skip(skip)
            .map(|chunk| {
                *seq_n += 1;
                Frame {
                    stamp,
                    hw_avg: averaged,
                    ..decode_with(chunk, b, &p, *seq_n, match_cores.then_some(&mut *cores))
                }
            })
            .collect();
        // frame 1 without its edge at the trigger point: left out too, whether frame 0 before it was
        // left out or kept by its edge check (`all` starts at frame `skip`)
        let at1 = (f0_drop && avail > 1 && skip <= 1).then(|| 1 - skip);
        let off = |f: &Frame| frame1_left_out_unseen(avail, f0_drop) || frame1_off_trigger(f, b);
        if let Some(i) = at1.filter(|&i| all.get(i).is_some_and(off)) {
            all.remove(i);
            frame1_left_out(frame1_left_out_unseen(avail, f0_drop));
            skip += 1;
        }
        if let (Some(r), Some(last)) = (raw_frame.filter(|_| !all.is_empty()), all.last_mut()) {
            *last = Frame { stamp, seq: last.seq, ..r };
        }
        (all, skip)
    }

    /// The first batch with new settings, of one frame (a deep record): that frame is frame 0, which the FPGA
    /// records before the arm (issue #5), and the first after a change can still be the older memory of the old settings
    /// (unit 2026-11-15: the whole screen window of a 10 M record was the 4 Vpp of the depth-Auto records before it). It
    /// is left out, never two in a row; not in SINGLE (its one record is the answer), not in hardware average (`averaged`),
    /// and only for a record shorter than `FIRST_LEFT_MAX_S`: at slow time bases the next record would keep the screen
    /// waiting as long again (1.25 s at 50 ms/div). The number of frames it left out.
    fn first_of_settings(
        &mut self,
        frames: &mut Vec<Frame>,
        (b, p): (&Settings, &hz::Params),
        avail: usize,
        averaged: bool,
    ) -> usize {
        let first = self.read_with.as_ref() != Some(b);
        self.read_with = Some(b.clone());
        // not in roll and scan: their pieces come in batches of one with settings of their own
        let short =
            (p.memsel as f64) < FIRST_LEFT_MAX_S * p.rate as f64 && frames.first().is_some_and(|f| !f.roll && !f.scan);
        if first
            && !self.first_left
            && short
            && frames.len() == 1
            && avail == 1
            && !b.single
            && !averaged
            && frame0_drop()
        {
            let left = frames.len();
            frames.clear();
            self.first_left = true;
            // the deep window read of this batch went with it: the next batch reads one at once
            self.deep_last = None;
            left
        } else {
            self.first_left = false;
            0
        }
    }
}

/// After a batch: the trigger status from the SCU's sticky bits, the mask unit's counters and the zone unit's status, for
/// the UI (`Shared::trig_fired`, `mask`, `zone`).
fn unit_readings<B: Bus>(
    state: &mut seq::State,
    bar: &mut B,
    out: &crate::board::HwShared,
    (b, p): (&Settings, &hz::Params),
    count: u32,
    batch_ms: f32,
) {
    // trigger status of this batch from the SCU's sticky bits, then the stock status clear, before the next
    // arm (prearm below) can record anything
    if hw_status_on() {
        let v = bar.rd(0x4034);
        bar.apply(&oos_hal::dho::scu::clear_status(v));
        let f = match oos_hal::dho::scu::fired(v) {
            oos_hal::dho::scu::Fired::Trigger => Some(true),
            oos_hal::dho::scu::Fired::Forced => Some(false),
            oos_hal::dho::scu::Fired::None => None,
        };
        let mut g = out.lock().unwrap();
        g.trig_fired = Some((g.trig_fired.map_or(1, |x| x.0 + 1), f));
    }
    // the mask unit's counters after every batch (docs/fork/hwmask.md)
    let mr = state.mask_ctrl().zip(b.hwmask.as_ref()).zip(state.mask_run).map(|((ctrl, m), run)| {
        let raw = oos_hal::dho::mask::read(bar, ctrl);
        (m.ch, run.0, ctrl, raw)
    });
    {
        let mut g = out.lock().unwrap();
        let n = g.mask.map_or(0, |r| r.n) + 1;
        g.mask = mr.map(|(ch, slot, ctrl, raw)| crate::board::MaskRead {
            ch,
            slot,
            ctrl,
            raw: [raw.frames_lo, raw.frames_hi, raw.fails_lo, raw.fails_hi],
            frames: raw.frames(),
            fails: raw.fails(),
            n,
            scale: b.ch[ch].scale,
            offset: b.ch[ch].offset,
            gain: state.gain[ch],
            points: p.memsel,
            chans: b.mask,
        });
        // the table as loaded, for the column debug (`:SYSTem:DEBug:MASK:COLumns?`), when it changed
        let table: Option<Vec<[u8; 4]>> = state
            .mask_cols
            .as_ref()
            .filter(|_| mr.is_some())
            .map(|t| t.iter().map(|k| [k.lo, k.hi, k.lo_en as u8, k.hi_en as u8]).collect());
        if table.as_deref() != g.mask_table.as_deref().map(|t| &t[..]) {
            g.mask_table = table.map(std::sync::Arc::new);
        }
    }
    // the zone unit's status and qualified-frame counter (docs/fork/hwzone.md)
    let zr = state
        .zone_ctrl()
        .zip(b.hwzone.as_ref())
        .zip(state.zone_run)
        .map(|((ctrl, z), run)| (z.ch, run.0, ctrl, oos_hal::dho::zone::read(bar, ctrl)));
    {
        let mut g = out.lock().unwrap();
        let n = g.zone.map_or(0, |r| r.n) + 1;
        g.zone = zr.map(|(ch, slot, ctrl, r)| {
            let c = r.counters;
            crate::board::ZoneRead {
                ch,
                slot,
                ctrl,
                raw: [r.s4030, r.s4038, c.frames_lo, c.frames_hi, c.fails_lo, c.fails_hi],
                frames: c.frames(),
                armed: r.armed(),
                pass: r.pass(),
                in_cycle: r.in_cycle(),
                state: r.state(),
                n,
                batch: (count, b.frames, batch_ms),
            }
        });
    }
}

/// The frames of a batch to the queue of the UI: the pipeline's counters take the `skip` frames left out and the oldest
/// of a full queue; the newest frame stays whatever its size.
fn queue(out: &crate::board::HwShared, frames: Vec<Frame>, count: u32, skip: usize) {
    let mut g = out.lock().unwrap();
    // the pipeline's counters take every frame read that is neither queued nor lent to the workers:
    // the ones left out above, the ones the screen does not take, the oldest of a full queue
    let dropped_before = g.dropped;
    g.decoded += count as u64;
    g.dropped += skip as u64;
    g.queue.extend(frames);
    let size = |f: &Frame| f.ch.iter().map(Vec::len).sum::<usize>();
    let mut held: usize = g.queue.iter().map(size).sum();
    // the newest frame stays whatever its size
    while g.queue.len() > 1 && (g.queue.len() > QUEUE_MAX || held > QUEUE_SAMPLES) {
        if let Some(f) = g.queue.pop_front() {
            held -= size(&f);
        }
        g.dropped += 1;
    }
    crate::drawpipe::Counters::add(&crate::drawpipe::counters().dropped, g.dropped - dropped_before);
    if let Some(f) = g.queue.back() {
        g.frame = f.clone();
    }
    drop(g);
}

/// The hardware average after a batch of `count` triggers: one frame, shown or not, the average cleared after a
/// complete hold average; the first batches logged with the FPGA's progress and the newest frame's first words (`dma`).
fn average<B: Bus>(
    avg: &mut Option<(oos_hal::dho::avg::Tracker, u64, u32)>,
    (b, p, count): (&Settings, &hz::Params, u32),
    dma: &[u8],
    bar: &mut B,
) -> Option<oos_hal::dho::avg::After> {
    avg.as_mut().map(|(tr, _, logged)| {
        let a = tr.batch(count);
        if *logged < 32 {
            // hardware check: 0x10b0 progress and the first words next to the triggers counted here
            *logged += 1;
            // the newest frame's first words (every recorded frame was played)
            let at = (count.max(1) as usize - 1) * seq::frame_bytes(b, p);
            let w: Vec<String> = dma
                .get(at..)
                .unwrap_or_default()
                .as_chunks::<2>()
                .0
                .iter()
                .take(4)
                .map(|x| format!("{:04x}", u16::from_le_bytes([x[0], x[1]])))
                .collect();
            eprintln!(
                "oos-ui hw: average {:?} batch {} of {} triggers, {} in, show {}, 0x10b0 = {}, words {}",
                tr.kind,
                *logged,
                count,
                a.triggers,
                a.show,
                oos_hal::dho::avg::progress(bar),
                w.join(" ")
            );
        }
        a
    })
}
