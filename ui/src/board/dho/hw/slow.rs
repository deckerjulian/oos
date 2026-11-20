// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Roll and scan in the acquisition thread (roll-mode.md): the stream's state between passes and one pass of it. The
//! FPGA streams chunks; roll keeps 10 divisions with the newest point at the right edge, scan writes the screen from
//! left to right, sweep by sweep. A change of how the slow time bases stream or are shown goes here.

use super::settings::roll_la_t0;
use crate::board::{Frame, HwShared, Slow};
use crate::model::*;
use oos_hal::dho::boot::Clock;
use oos_hal::dho::bus::Bus;
use oos_hal::dho::seq::{self, Settings};
use oos_hal::dho::{hz, roll};
use std::sync::Arc;

/// The roll or scan stream between passes of the loop: its parameters while it runs, the ring of volts per channel
/// (roll), the sweep picture per channel (scan), both for the LA words, the write position, the last publish.
pub(super) struct SlowRun {
    rolling: Option<roll::Params>,
    ring: [std::collections::VecDeque<f32>; NCH],
    // the same for the LA words: ring, sweep picture (as f32 for `scan_write`; a word is exact in it)
    ring_la: std::collections::VecDeque<u16>,
    sweep_la: Vec<f32>,
    published: u64,
    // scan: the sweep picture per channel (one screen), write position, last 0x1204, SINGLE sweep complete
    sweep: [Vec<f32>; NCH],
    cursor: usize,
    last_info: roll::ScanInfo,
    halted: bool,
    // 0x40d0 reported done; the last chunks of the sweep are read once more before the stop
    done_seen: Option<u64>,
}

/// What a pass of the stream works with on the unit: the shadow state, the bus, the stream of chunks, the loop's clock,
/// what is shared with the screen, the count of frames published and the stamp of this pass.
pub(super) struct Io<'a, B, R, C> {
    pub(super) state: &'a mut seq::State,
    pub(super) bar: &'a mut B,
    pub(super) c2h: &'a mut R,
    pub(super) clock: &'a mut C,
    pub(super) out: &'a HwShared,
    pub(super) n: &'a mut u64,
    pub(super) stamp: u64,
}

impl SlowRun {
    /// Nothing streams; `now`: the clock's time, the last publish.
    pub(super) fn new(now: u64) -> SlowRun {
        SlowRun {
            rolling: None,
            ring: Default::default(),
            ring_la: Default::default(),
            sweep_la: Vec::new(),
            published: now,
            sweep: Default::default(),
            cursor: 0,
            last_info: roll::ScanInfo::default(),
            halted: false,
            done_seen: None,
        }
    }

    /// Leaves roll or scan: the stream is stopped when it ran (`roll::stop`).
    pub(super) fn stop<B: Bus>(&mut self, state: &mut seq::State, bar: &mut B) {
        if self.rolling.take().is_some() {
            bar.apply(&roll::stop(&mut state.sh));
        }
    }

    /// One pass of roll or scan (`mode` not `Slow::Off`) with the settings `s` configured as `p`: the stream started
    /// when it is not running, the chunks waiting read, a picture published about 30 times a second. A failed read
    /// ends the stream and forgets the configuration (`current`), so the next pass configures again.
    pub(super) fn step<B: Bus, R: std::io::Read, C: Clock>(
        &mut self,
        mode: Slow,
        s: &Settings,
        p: &hz::Params,
        io: Io<'_, B, R, C>,
        current: &mut Option<(Settings, hz::Params)>,
        status: &impl Fn(String),
    ) {
        let Io { state, bar, c2h, clock, out, n, stamp } = io;
        // roll and scan (roll-mode.md): stream chunks; roll keeps 10 divisions and puts the newest point at the
        // right edge, scan writes the screen from left to right; publish ~30 pictures per second
        let tdiv_fs = seq::tdiv_fs(s.tdiv);
        let scan = match mode {
            Slow::Scan { delay, single } => Some(((delay * 1e15) as i128, single)),
            _ => None,
        };
        // LA on: its words come in chunks of their own in the same stream (la.md 4.4)
        let la_t = s.la.is_some().then(|| oos_hal::dho::la::timing(p, s.mask));
        let sweep_start = |state: &mut seq::State, bar: &mut B, rp: &roll::Params| {
            match scan {
                Some((delay_fs, _)) => {
                    let pos = roll::scan_trig_pos(delay_fs, tdiv_fs, p.dot_fs);
                    bar.apply(&roll::config_scan(&mut state.sh, rp, p.memsel, pos));
                }
                None => bar.apply(&roll::config(&mut state.sh, rp, p.memsel)),
            }
            if let Some(t) = &la_t {
                bar.apply(&roll::la_config(rp, t));
            }
            bar.apply(&roll::start(&mut state.sh, s.mask));
        };
        let rp = match self.rolling {
            Some(rp) => rp,
            None => {
                let rp = roll::params(s.mask, tdiv_fs, s.depth, p);
                sweep_start(state, bar, &rp);
                self.ring.iter_mut().for_each(|r| r.clear());
                self.sweep.iter_mut().for_each(|v| v.clear());
                self.ring_la.clear();
                self.sweep_la.clear();
                (self.cursor, self.halted, self.done_seen, self.last_info) = (0, false, None, Default::default());
                let name = if scan.is_some() { "scan" } else { "roll" };
                status(format!(
                    "{}: {:.1} us per point, chunk {:.2} ms",
                    name,
                    rp.spacing_fs as f64 / 1e9,
                    rp.spacing_fs as f64 * 100.0 / rp.ratio as f64 / 1e12
                ));
                self.rolling = Some(rp);
                // roll and scan stream raw points: no batch, so the state line's factor is set here
                crate::board::policy::compress::note_batch_k(1);
                rp
            }
        };
        if self.halted {
            // SINGLE scan complete: wait for the next SINGLE press
            let go = std::mem::take(&mut out.lock().unwrap().rearm);
            if !go {
                clock.sleep_us(20000);
                return;
            }
            sweep_start(state, bar, &rp);
            (self.cursor, self.halted) = (0, false);
        }
        let dt = rp.spacing_fs as f64 / 1e15;
        let keep = ((10.0 * s.tdiv / dt).round() as usize).max(2);
        let la_dt = if la_t.is_some() { roll::la_spacing_fs(&rp) as f64 / 1e15 } else { dt };
        let keep_la = ((10.0 * s.tdiv / la_dt).round() as usize).max(2);
        bar.apply(&roll::fill_pulse(&state.sh));
        let c = roll::chunks(bar.rd(0x11e8));
        let c = if la_t.is_some() { roll::la_chunks(c) } else { c };
        if c == 0 {
            clock.sleep_us(9000);
        } else {
            let buf = match oos_hal::dho::bus::read_dma(c2h, c * roll::CHUNK_BYTES, |_, _| {}) {
                Ok(b) => b,
                Err(e) => {
                    status(format!("{} read of {} chunks: {}", if scan.is_some() { "scan" } else { "roll" }, c, e));
                    self.rolling = None;
                    *current = None;
                    return;
                }
            };
            self.put(s, &buf, scan.is_some(), la_t.is_some(), keep, keep_la);
        }
        // scan: after every read, also an empty one
        let mut done = false;
        if let Some((_, single)) = scan {
            if self.done_seen.is_none() {
                let r = roll::SCAN_INFO_REGS.map(|a| bar.rd(a));
                let info = roll::scan_info(r[0], r[1]);
                if info.triggered != self.last_info.triggered || info.done {
                    status(format!(
                        "scan: 0x1204 = {:#010x}, 0x40d0 = {:#x}, cursor {} of {}",
                        r[0], r[1], self.cursor, keep
                    ));
                }
                self.last_info = info;
                // done arrives a few chunks before the end of the sweep (unit: 4 chunks): read on until the
                // last column is in (cursor back at 0) or 100 ms passed
                if info.done {
                    self.done_seen = Some(clock.now_us());
                }
            }
            let now = clock.now_us();
            if self.done_seen.is_some_and(|t| self.cursor == 0 || super::since(now, t).as_millis() >= 100) {
                status(format!(
                    "scan: sweep complete {} ms after done, cursor {} of {}",
                    super::since(now, self.done_seen.unwrap()).as_millis(),
                    self.cursor,
                    keep
                ));
                self.done_seen = None;
                done = true;
                // stock state 8 (RUN): stop, clear the average, configure again, next sweep after 500 ms; state 9
                // (SINGLE): stop
                bar.apply(&roll::scan_done(&mut state.sh));
                self.halted = single;
            }
        }
        let now = clock.now_us();
        if done || super::since(now, self.published).as_millis() >= 33 {
            self.published = now;
            *n += 1;
            let f = self.frame(s, scan.map(|_| done), la_t.is_some(), (dt, la_dt, keep), *n, stamp);
            let mut g = out.lock().unwrap();
            // a complete sweep must not be dropped by the next partial picture
            if g.queue.back().is_none_or(|b| b.partial) {
                g.queue.clear();
            }
            g.queue.push_back(f.clone());
            g.frame = f;
            g.halted = self.halted;
        }
        if done && !self.halted {
            clock.sleep_us(500000);
            sweep_start(state, bar, &rp);
            self.cursor = 0;
        }
    }

    /// The chunks just read (`buf`) into the sweep pictures (scan) or the rings (roll), analog and LA: `keep` and
    /// `keep_la` points a screen.
    fn put(&mut self, s: &Settings, buf: &[u8], scan: bool, la: bool, keep: usize, keep_la: usize) {
        let (w, lw) = if la { roll::split_la(buf, s.mask) } else { (roll::split(buf, s.mask), Vec::new()) };
        if scan {
            // the chunk counter restarts with every sweep: its column is the write position of the first
            // analog chunk
            let per = roll::points_per_chunk(s.mask);
            let chunks = buf.as_chunks::<{ roll::CHUNK_BYTES }>().0;
            let first = chunks.iter().map(|c| &c[..]).find(|c| !roll::is_la_chunk(c));
            let first = first.unwrap_or(&buf[..roll::CHUNK_BYTES]);
            let col = roll::scan_column(roll::counter(first), la);
            self.cursor = (col * per) % keep;
            if !lw.is_empty() {
                let v: Vec<f32> = lw.iter().map(|&x| x as f32).collect();
                scan_write(&mut self.sweep_la, keep_la, scan_la_pos(self.cursor, keep, keep_la), &v);
            }
            let fresh = w.iter().map(|v| v.len()).max().unwrap_or(0);
            for ch in 0..NCH {
                if w[ch].is_empty() {
                    self.sweep[ch].clear();
                    continue;
                }
                scan_write(
                    &mut self.sweep[ch],
                    keep,
                    self.cursor,
                    &seq::volts(&w[ch], s.ch[ch].scale, s.ch[ch].offset),
                );
            }
            self.cursor = (self.cursor + fresh) % keep;
        } else {
            self.ring_la.extend(lw.iter().copied());
            while self.ring_la.len() > keep_la {
                self.ring_la.pop_front();
            }
            for ch in 0..NCH {
                if w[ch].is_empty() {
                    self.ring[ch].clear();
                    continue;
                }
                self.ring[ch].extend(seq::volts(&w[ch], s.ch[ch].scale, s.ch[ch].offset));
                while self.ring[ch].len() > keep {
                    self.ring[ch].pop_front();
                }
            }
        }
    }

    /// The picture to publish: the sweep (`scan`: whether it is complete; when not, with a gap at the write position)
    /// or the ring (roll), with the LA words when `la`; `dt`, `la_dt` the spacings and `keep` the points of a screen.
    fn frame(
        &self,
        s: &Settings,
        scan: Option<bool>,
        la: bool,
        (dt, la_dt, keep): (f64, f64, usize),
        n: u64,
        stamp: u64,
    ) -> Frame {
        if let Some(done) = scan {
            let mut ch = self.sweep.clone();
            if !done {
                // a short gap ahead of the write position shows where the sweep is
                scan_gap(&mut ch, self.cursor, keep / 50);
            }
            // word 0 at the left edge like analog point 0; only what the sweeps have written so far
            let la = la.then(|| crate::logic::LaRec {
                words: Arc::new(self.sweep_la.iter().take_while(|x| !x.is_nan()).map(|&x| x as u16).collect()),
                t0: 0.0,
                dt: la_dt,
            });
            Frame {
                seq: n,
                dt,
                pre: 0,
                ch,
                roll: false,
                envelope: true,
                scan: true,
                partial: !done,
                acq: (0.0, 0),
                stamp,
                la,
                fixed_ti: None,
                hw_avg: false,
                basic: Default::default(),
            }
        } else {
            let len = self.ring.iter().map(|r| r.len()).max().unwrap_or(0);
            // the newest point at the right edge: index len - 1 sits half a screen right of `pre`
            let pre = (len as f64 - 1.0 - 5.0 * s.tdiv / dt).max(0.0) as usize;
            // the newest LA word next to the newest analog point, at the right edge
            let la = la.then(|| crate::logic::LaRec {
                t0: roll_la_t0(len, pre, dt, self.ring_la.len(), la_dt),
                words: Arc::new(self.ring_la.iter().copied().collect()),
                dt: la_dt,
            });
            Frame {
                seq: n,
                dt,
                pre,
                ch: self.ring.clone().map(|r| r.into_iter().collect()),
                roll: true,
                envelope: true,
                scan: false,
                partial: len < keep,
                acq: (0.0, 0),
                stamp,
                la,
                fixed_ti: None,
                hw_avg: false,
                basic: Default::default(),
            }
        }
    }
}

/// Writes the scan points `v` into the sweep picture (`width` points) from `at`, wrapping at the right edge.
pub(super) fn scan_write(pic: &mut Vec<f32>, width: usize, at: usize, v: &[f32]) {
    if pic.len() != width {
        *pic = vec![f32::NAN; width];
    }
    for (i, &y) in v.iter().enumerate() {
        pic[(at + i) % width] = y;
    }
}

/// Where the LA words of a chunk go in the sweep picture: the same place on the screen as the analog point `cursor`.
pub(super) fn scan_la_pos(cursor: usize, keep: usize, keep_la: usize) -> usize {
    ((cursor as f64 * keep_la as f64 / keep.max(1) as f64).round() as usize) % keep_la.max(1)
}

/// Blanks `gap` points from the write position (the old sweep right of it stays visible).
pub(super) fn scan_gap(pic: &mut [Vec<f32>; NCH], at: usize, gap: usize) {
    for v in pic.iter_mut().filter(|v| !v.is_empty()) {
        let w = v.len();
        for i in 0..gap.min(w) {
            v[(at + i) % w] = f32::NAN;
        }
    }
}
