// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware segmented memory with trigger timestamps (docs/fork/specs/03-segmented-timestamps.md). One arm records N
//! frames into the frame ring, the FPGA re-arms itself between them; the host transfers nothing until the end. Then
//! every segment is played on its own for its stamp (latch 0x40b0/0x40b4 and header words 5..7), and read in batches
//! like the live frames. Unit 2026-11-13 (private RE notes docs/logs/oos_unitlab_20261113.md, B3): the latch counts
//! 0.8 ns, the header of segment k carries the latch value of segment k - 1, and 0x4034 shows bits of 0x2320 while
//! the FPGA still records.
//!
//! Generic over `Bus` and the C2H reader, so the whole sequence runs against `sim::Engine` on the host.

use crate::dho::bus::{Bus, DmaBuf};
use crate::dho::{hz, scu, seq};

/// One LSB of the latched and first-trigger stamps (0x40b0, 0x40c0) in femtoseconds: 0.8 ns, one sample at 1.25 GSa/s.
/// Unit 2026-11-13: a 1.000 kHz trigger gave latch steps of 1,250,000. (The stock API's 250,000 fs per tick, spec 03
/// 1.1, made every interval 3.2 times too short.)
pub const LATCH_TICK_FS: i128 = 800_000;
/// One LSB of the 48-bit stamp in header words 5..7: the latch's tick (unit 2026-11-13: the header value equals a
/// latch value, no scaling).
pub const HEADER_TICK_FS: i128 = LATCH_TICK_FS;

/// The stamps of one segment, raw ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    /// 0x40b0/0x40b4 latched after the segment's own play: the segment's trigger time
    pub latch: Option<u64>,
    /// header words 5..7 of the segment's frame: the latch value of the segment before it (unit 2026-11-13; segment
    /// 0's header is an older trigger), so it is logged only, not used for the time
    pub header: Option<u64>,
}

/// Low and high register words to one 64-bit stamp.
pub fn join(lo: u32, hi: u32) -> u64 {
    (hi as u64) << 32 | lo as u64
}

/// The 48-bit value in header words 5 (most significant), 6, 7 of a 16-byte frame header (spec 03, 1.6).
pub fn header_stamp(hdr: &[u8]) -> Option<u64> {
    let w = |i: usize| hdr.get(2 * i..2 * i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u64);
    Some(w(5)? << 32 | w(6)? << 16 | w(7)?)
}

/// Seconds of `ticks` ticks of `tick_fs`.
pub fn ticks_to_s(ticks: i128, tick_fs: i128) -> f64 {
    (ticks * tick_fs) as f64 * 1e-15
}

/// Trigger times of the segments in seconds since the first trigger, from their latched stamps in segment order.
/// They count from the first-trigger stamp (0x40c0) when it is valid, else from the first latched stamp; 64 bits do
/// not wrap. The header stamp lags one segment behind and is not used.
#[derive(Clone, Debug, Default)]
pub struct Timeline {
    start: Option<u64>,
}

impl Timeline {
    /// `first_trigger`: 0x40c0/0x40c4 read at the end of the recording, if 0x40b8 bit1 said valid.
    pub fn new(first_trigger: Option<u64>) -> Timeline {
        Timeline { start: first_trigger }
    }

    /// Time of the next segment (call in segment order); None without a latched stamp.
    pub fn push(&mut self, st: Stamp) -> Option<f64> {
        let l = st.latch?;
        let s0 = *self.start.get_or_insert(l);
        Some(ticks_to_s(l as i128 - s0 as i128, LATCH_TICK_FS))
    }
}

/// State of a running recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// frames recorded so far
    pub recorded: u32,
    /// 0x4034 as read, for the log
    pub fsm: u32,
}

impl Status {
    /// The recording is over: all `n` frames are in. Only the count decides, as in the stock record state (0xb), which
    /// keeps polling the count and never reads 0x4034 for it. Bits of 0x2320 in 0x4034 do not mean "stopped": on the
    /// unit they were set after the first frames of every recording, and taking them as the end cut every recording at
    /// the frames of the first 10 ms poll (B3, 2026-11-13). A recording that never reaches `n` ends with STOP.
    pub fn complete(&self, n: u32) -> bool {
        self.recorded >= n
    }
}

/// Frames recorded so far (stock `RequestNormTrace` in state 0xb): 0 until 0x4080 reports the first frame done, then
/// 0x4010 (last recorded frame) - 0x4014 (first) + 1, at most `n`.
pub fn status<B: Bus>(bar: &mut B, n: u32) -> Status {
    let recorded = if bar.rd(0x4080) & scu::PLAYINFO_DONE == 0 { 0 } else { bar.rd(0x4010).wrapping_sub(bar.rd(0x4014)).wrapping_add(1).min(n) };
    Status { recorded, fsm: bar.rd(0x4034) }
}

/// The first-trigger stamp if 0x40b8 bit1 says it is valid.
pub fn first_trigger<B: Bus>(bar: &mut B) -> Option<u64> {
    (bar.rd(scu::stamp::CTRL) & scu::stamp::FIRST_VALID != 0).then(|| read_pair(bar, scu::stamp::FIRST))
}

/// A 64-bit stamp at `lo` (low word) and `lo + 4`, low word read first.
pub fn read_pair<B: Bus>(bar: &mut B, lo: u32) -> u64 {
    let l = bar.rd(lo);
    join(l, bar.rd(lo + 4))
}

/// Arms the recording (`State::record_steps`); returns the frame count.
pub fn start<B: Bus, R: std::io::Read>(st: &mut seq::State, s: &seq::Settings, p: &hz::Params, bar: &mut B, c2h: &mut R) -> Result<u32, String> {
    let mut dma = DmaBuf::new();
    seq::execute_bus(&st.record_steps(s, p), bar, c2h, &mut dma, |_| {})?;
    Ok(seq::record_frames(s, p))
}

/// Ends a recording early or after the last frame: the stock StopScope (`State::disarm_steps`). Read `status` and
/// `first_trigger` before it.
pub fn stop<B: Bus, R: std::io::Read>(st: &mut seq::State, bar: &mut B, c2h: &mut R) -> Result<(), String> {
    let mut dma = DmaBuf::new();
    seq::execute_bus(&st.disarm_steps(), bar, c2h, &mut dma, |_| {})
}

/// Stamps of segments `first..first + count`: one stamp play each (`State::segment_stamp_steps`), its header, then the
/// latched pair.
pub fn stamps<B: Bus, R: std::io::Read>(st: &mut seq::State, s: &seq::Settings, p: &hz::Params, bar: &mut B, c2h: &mut R, first: u32, count: u32) -> Result<Vec<Stamp>, String> {
    let mut dma = DmaBuf::new();
    let mut out = Vec::with_capacity(count as usize);
    for k in first..first + count {
        seq::execute_bus(&st.segment_stamp_steps(s, p, k), bar, c2h, &mut dma, |_| {})?;
        let header = header_stamp(&dma.data()[..dma.data().len().min(16)]);
        out.push(Stamp { latch: Some(read_pair(bar, scu::stamp::LATCHED)), header });
    }
    Ok(out)
}

/// Settings for reading segments back: `s` with the frame header on (its stamp words) and `frames` per read.
pub fn read_settings(s: &seq::Settings, frames: u32) -> seq::Settings {
    seq::Settings { header: true, frames, ..s.clone() }
}

/// Reads segments `first..first + count` into `dma` with one type-4 play (`State::read_steps_at`). `s` from
/// `read_settings`. The frames are `seq::frame_bytes(s, p)` each, header first.
pub fn read<B: Bus, R: std::io::Read>(st: &mut seq::State, s: &seq::Settings, p: &hz::Params, bar: &mut B, c2h: &mut R, dma: &mut DmaBuf, first: u32, count: u32) -> Result<(), String> {
    seq::execute_bus(&st.read_steps_at(s, p, first, count), bar, c2h, dma, |_| {})
}

/// A model of the record engine and the play path for host runs of the whole sequence: what the steps of
/// `State::record_steps`, `read_steps_at`, `segment_stamp_steps` and `disarm_steps` do, as spec 03 describes it, with
/// what the unit showed on 2026-11-13: the latch holds the trigger time of the last frame played in 0.8 ns ticks, the
/// header of frame k holds the trigger time of frame k - 1 in the same ticks (frame 0 an older trigger), and 0x4034
/// shows bit 5 (in the stock 0x2320 mask) while frames are still to come.
///
/// Average mode (0x1004 bit3, docs/fork/specs/01-hw-averaging.md): the played words are the average of `Engine::raw`
/// over the triggers since the last 0x10b4 pulse, of the last 0x10b0 + 1 of them (`avg_hold` false: running) or of the
/// first 0x10b0 + 1 (`avg_hold` true: cumulative, then held), as `avg::mean_words` computes it. Trigger numbers count on
/// across arms; a clear pulse starts from the next one.
pub mod sim {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The engine. Triggers come every `period_fs` from `t0_fs`; `per_poll` frames are recorded per read of 0x4080.
    pub struct Engine {
        pub regs: std::collections::HashMap<u32, u32>,
        pub t0_fs: i128,
        pub period_fs: i128,
        pub per_poll: u32,
        /// interleave factor of the record (the play length of a frame is 0x11f0 x ratio words)
        pub ratio: u32,
        /// recording: first, last frame and frames recorded so far
        rec: Option<(u32, u32, u32)>,
        /// frames recorded by the last recording
        pub recorded: u32,
        first_valid: bool,
        latched: u64,
        last_played: Option<u32>,
        stream: Vec<u8>,
        prev_40b8: u32,
        prev_4004: u32,
        /// average mode: hold after the count instead of a running average
        pub avg_hold: bool,
        /// triggers recorded by all stopped recordings; the number of frame 0 of the current or last one
        pub triggers: u64,
        trig_base: u64,
        /// first trigger of the average (the trigger count at the last 0x10b4 pulse)
        avg_from: u64,
        prev_10b4: u32,
    }

    impl Engine {
        pub fn new(t0_fs: i128, period_fs: i128, per_poll: u32, ratio: u32) -> Engine {
            Engine {
                regs: Default::default(),
                t0_fs,
                period_fs,
                per_poll,
                ratio,
                rec: None,
                recorded: 0,
                first_valid: false,
                latched: 0,
                last_played: None,
                stream: Vec::new(),
                prev_40b8: 0,
                prev_4004: 0,
                avg_hold: false,
                triggers: 0,
                trig_base: 0,
                avg_from: 0,
                prev_10b4: 0,
            }
        }

        /// Raw sample word `i` of trigger `g` (average mode): a ramp over 64 points plus a trigger-dependent step of 0..60
        /// codes, so averages of 16 consecutive triggers are exact (+30).
        pub fn raw(g: u64, i: usize) -> u16 {
            (32768 + (i % 64) * 16 + (g % 16) as usize * 4) as u16
        }

        /// The triggers the average of trigger `g` covers (`raw` numbers, inclusive).
        pub fn avg_span(&self, g: u64) -> (u64, u64) {
            let n = self.reg(crate::dho::acq::AVG_COUNT) as u64 + 1;
            if self.avg_hold {
                (self.avg_from, g.min(self.avg_from + n - 1))
            } else {
                (self.avg_from.max((g + 1).saturating_sub(n)), g)
            }
        }

        /// The averaged words of frame `k` of the last recording.
        pub fn avg_words(&self, k: u32, words: usize) -> Vec<u16> {
            let (lo, hi) = self.avg_span(self.trig_base + k as u64);
            let frames: Vec<Vec<u16>> = (lo..=hi).map(|g| (0..words).map(|i| Engine::raw(g, i)).collect()).collect();
            crate::dho::avg::mean_words(&frames.iter().map(|f| &f[..]).collect::<Vec<_>>())
        }

        /// Trigger time of frame `k`, fs.
        pub fn trig_fs(&self, k: u32) -> i128 {
            self.t0_fs + k as i128 * self.period_fs
        }

        /// The time in the header of frame `k`: the trigger before it (for frame 0 one from before the recording).
        pub fn header_fs(&self, k: u32) -> i128 {
            self.t0_fs + (k as i128 - 1) * self.period_fs
        }

        fn reg(&self, r: u32) -> u32 {
            self.regs.get(&r).copied().unwrap_or(0)
        }

        /// Sample word `i` of frame `k`: the frame number in the high byte, so a read shows which frame it got.
        pub fn word(k: u32, i: usize) -> u16 {
            ((k & 0xff) << 8) as u16 | (i & 0xff) as u16
        }

        fn run(&mut self) {
            let c = self.reg(0x400c);
            let (first, last) = (self.reg(0x4010), self.reg(0x4018));
            if c & 0x4000_0000 != 0 {
                // play: the frames first..=last into the stream, header on when 0x1010 bit31 is clear
                let words = (self.reg(0x11f0) * self.ratio) as usize;
                for k in first..=last {
                    if self.reg(0x1010) & 1 << 31 == 0 {
                        let h = (self.header_fs(k) / HEADER_TICK_FS) as u64 & ((1 << 48) - 1);
                        for w in [0xfa05, 0, 0x7b, 0, k as u16, (h >> 32) as u16, (h >> 16) as u16, h as u16] {
                            self.stream.extend(u16::to_le_bytes(w));
                        }
                    }
                    if self.reg(0x1004) & 8 != 0 {
                        for w in self.avg_words(k, words) {
                            self.stream.extend(w.to_le_bytes());
                        }
                    } else if self.reg(0x1170) & 1 << 31 != 0 {
                        // FPGA compression (0x1170 bit 31, k in bits 27:0): `words` min/max points of the frame's
                        // `words` x k samples in the layout `pairs::LAYOUT` (ratio 1)
                        let c = (self.reg(0x1170) & 0x0fff_ffff).max(1) as usize;
                        let raw: Vec<u16> = (0..words * c).map(|i| Engine::word(k, i)).collect();
                        for i in 0..words {
                            let w = crate::dho::pairs::point(&raw, c, crate::dho::pairs::LAYOUT, i).unwrap_or(0);
                            self.stream.extend(w.to_le_bytes());
                        }
                    } else {
                        for i in 0..words {
                            self.stream.extend(Engine::word(k, i).to_le_bytes());
                        }
                    }
                }
                self.last_played = Some(last);
            } else if c & 0x2200_0000 == 0x2200_0000 {
                self.rec = Some((first, last, 0));
                self.trig_base = self.triggers;
            }
        }
    }

    impl Bus for Engine {
        fn wr(&mut self, off: u32, v: u32) {
            self.regs.insert(off, v);
            match off {
                0x4004 => {
                    if v & 1 << 31 != 0 {
                        // stop: the recording ends where it is
                        if let Some((_, _, n)) = self.rec.take() {
                            self.recorded = n;
                            self.triggers += n as u64;
                        }
                    }
                    if v & 1 != 0 && self.prev_4004 & 1 == 0 {
                        self.run();
                    }
                    self.prev_4004 = v;
                }
                crate::dho::acq::AVG_CLEAR => {
                    if v & 1 != 0 && self.prev_10b4 & 1 == 0 {
                        self.avg_from = self.triggers;
                    }
                    self.prev_10b4 = v;
                }
                0x40b8 => {
                    if v & scu::stamp::FIRST_CLEAR != 0 {
                        self.first_valid = false;
                    }
                    if v & scu::stamp::LATCH != 0 && self.prev_40b8 & scu::stamp::LATCH == 0 {
                        self.latched = self.last_played.map_or(0, |k| (self.trig_fs(k) / LATCH_TICK_FS) as u64);
                    }
                    self.prev_40b8 = v;
                }
                _ => {}
            }
        }

        fn rd(&mut self, off: u32) -> u32 {
            match off {
                // the done-flag poll is where time passes: `per_poll` more frames each
                0x4080 => {
                    let n = match self.rec.as_mut() {
                        Some((first, last, n)) => {
                            *n = (*n + self.per_poll).min(*last - *first + 1);
                            *n
                        }
                        None => self.recorded,
                    };
                    self.first_valid |= n > 0;
                    if n > 0 {
                        scu::PLAYINFO_DONE
                    } else {
                        0
                    }
                }
                0x4010 => match self.rec {
                    Some((first, _, n)) => first + n.max(1) - 1,
                    None => self.reg(0x4014) + self.recorded.max(1) - 1,
                },
                // still recording: waiting for a trigger (bit4), with bit5 once a frame is in (as the unit shows); else
                // stopped (bit13)
                0x4034 => match self.rec {
                    Some((first, last, n)) if n < last - first + 1 => scu::status::WAIT_TRIG | if n > 0 { 0x20 } else { 0 },
                    _ => scu::status::STOPPED,
                },
                0x40b8 => self.prev_40b8 | if self.first_valid { scu::stamp::FIRST_VALID } else { 0 },
                0x40b0 => self.latched as u32,
                0x40b4 => (self.latched >> 32) as u32,
                0x40c0 => (self.t0_fs / LATCH_TICK_FS) as u32,
                0x40c4 => ((self.t0_fs / LATCH_TICK_FS) >> 32) as u32,
                _ => self.reg(off),
            }
        }
    }

    /// The register side and the C2H side of one shared engine.
    pub struct SimBus(pub Rc<RefCell<Engine>>);
    pub struct SimC2h(pub Rc<RefCell<Engine>>);

    impl Bus for SimBus {
        fn wr(&mut self, off: u32, v: u32) {
            self.0.borrow_mut().wr(off, v)
        }
        fn rd(&mut self, off: u32) -> u32 {
            self.0.borrow_mut().rd(off)
        }
    }

    impl std::io::Read for SimC2h {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let mut e = self.0.borrow_mut();
            let k = b.len().min(e.stream.len());
            if k == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no frame played"));
            }
            b[..k].copy_from_slice(&e.stream[..k]);
            e.stream.drain(..k);
            Ok(k)
        }
    }

    /// A shared engine and its two sides.
    pub fn engine(e: Engine) -> (Rc<RefCell<Engine>>, SimBus, SimC2h) {
        let r = Rc::new(RefCell::new(e));
        (r.clone(), SimBus(r.clone()), SimC2h(r))
    }
}

#[cfg(test)]
mod tests {
    use super::sim::*;
    use super::*;
    use crate::dho::seq::{Segments, Settings, State};

    const MS: i128 = 1_000_000_000_000;

    #[test]
    fn stamps_join_and_header_words() {
        assert_eq!(join(0xffff_fff0, 1), 0x1_ffff_fff0);
        // word 5 most significant
        let h: Vec<u8> = [0xfa05u16, 0, 0x7b, 0, 3, 0x0012, 0x3456, 0x789a].iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(header_stamp(&h), Some(0x0012_3456_789a));
        assert_eq!(header_stamp(&h[..14]), None);
    }

    // Break it catches: differences across a 32-bit carry taken from one word only, the stock API's 250 ps tick
    // (every interval 3.2 times too short on the unit), the header's lagging stamp used as a time.
    #[test]
    fn timeline_scales_latches_only() {
        // across a carry of the low word: 32 ticks of 0.8 ns
        let mut t = Timeline::new(Some(0xffff_fff0));
        assert_eq!(t.push(Stamp { latch: Some(0xffff_fff0), header: None }), Some(0.0));
        let d = t.push(Stamp { latch: Some(0x1_0000_0010), header: None }).unwrap();
        assert!((d - 32.0 * 0.8e-9).abs() < 1e-18, "{}", d);
        // unit 2026-11-13: a 1.000 kHz trigger, latch steps of 1,250,000 = 1 ms
        let mut t = Timeline::new(None);
        t.push(Stamp { latch: Some(1000), header: None });
        assert!((t.push(Stamp { latch: Some(1_251_000), header: None }).unwrap() - 1e-3).abs() < 1e-15);
        // the header is never the time: without a latch nothing, with one the latch alone
        assert_eq!(t.push(Stamp { latch: None, header: Some(5) }), None);
        let mut t = Timeline::new(Some(0));
        assert_eq!(t.push(Stamp { latch: Some(5), header: Some(1_000_000) }), Some(4e-9));
    }

    // Break it catches: 0x4034 bits read as the end of the recording (B3: every recording cut at the frames of the
    // first poll), a count that runs past N.
    #[test]
    fn status_counts_from_the_done_flag() {
        let mut r = crate::dho::bus::Recorder::default();
        r.regs.insert(0x4010, 7);
        r.regs.insert(0x4014, 0);
        r.regs.insert(0x4034, scu::status::WAIT_TRIG);
        // (done flag, 0x4034, n) -> (recorded, complete)
        let cases: [(bool, u32, u32, u32, bool); 6] = [
            // not done yet: nothing recorded, whatever 0x4010 says
            (false, scu::status::WAIT_TRIG, 100, 0, false),
            (true, scu::status::WAIT_TRIG, 100, 8, false),
            // the unit's status during a recording (bit 5 of 0x2320): not the end
            (true, scu::status::WAIT_TRIG | 0x20, 100, 8, false),
            (true, 0x2320, 100, 8, false),
            // all frames in
            (true, scu::status::WAIT_TRIG, 8, 8, true),
            (true, scu::status::STOPPED, 5, 5, true),
        ];
        for (done, fsm, n, rec, complete) in cases {
            r.regs.insert(0x4080, if done { scu::PLAYINFO_DONE } else { 0 });
            r.regs.insert(0x4034, fsm);
            let x = status(&mut r, n);
            assert_eq!((x.recorded, x.complete(n), x.fsm), (rec, complete, fsm), "done {} 0x4034 {:#x} n {}", done, fsm, n);
        }
    }

    fn settings(n: u32) -> Settings {
        Settings { tdiv: 100e-9, depth: 1000, window: None, segments: Some(Segments { n, interval: 0.0 }), ..Settings::default() }
    }

    // End to end against the model: arm, poll until all frames are in, stop, stamps, read back in two batches. Break
    // it catches: a read-out that plays the wrong frames (the model puts the frame number in every word), stamps of
    // the wrong frame, a progress that stops short or runs past N.
    #[test]
    fn record_poll_stamp_and_read_back_against_the_model() {
        let s = settings(100);
        let mut st = State::handover(1, None);
        let p = st.configure(&s, None, None).unwrap().1;
        let (eng, mut bar, mut c2h) = engine(Engine::new(5 * MS, MS, 7, p.ratio));
        let n = start(&mut st, &s, &p, &mut bar, &mut c2h).unwrap();
        assert_eq!(n, 100);
        let mut polls = 0;
        // the model shows the unit's 0x4034 bit 5 from the first frame on; the recording must still run to N
        let end = loop {
            let x = status(&mut bar, n);
            polls += 1;
            if x.complete(n) || polls > 1000 {
                break x;
            }
        };
        assert_eq!(end.recorded, 100);
        assert!(polls > 10, "progress in steps of 7: {} polls", polls);
        let tag = first_trigger(&mut bar);
        assert_eq!(tag, Some((5 * MS / LATCH_TICK_FS) as u64));
        stop(&mut st, &mut bar, &mut c2h).unwrap();
        let sts = stamps(&mut st, &s, &p, &mut bar, &mut c2h, 0, n).unwrap();
        let mut tl = Timeline::new(tag);
        let times: Vec<f64> = sts.iter().map(|x| tl.push(*x).unwrap()).collect();
        for (k, t) in times.iter().enumerate() {
            assert!((t - k as f64 * 1e-3).abs() < 1e-12, "segment {}: {}", k, t);
        }
        // the header of segment k holds the latch of segment k - 1 (unit 2026-11-13)
        for k in 1..sts.len() {
            assert_eq!(sts[k].header, sts[k - 1].latch, "segment {}", k);
        }
        // read back: 60 + 40 frames, header first, every word from its own frame
        let rs = read_settings(&s, 60);
        let fb = seq::frame_bytes(&rs, &p);
        assert_eq!(fb, 16 + 2 * 1000);
        let mut dma = DmaBuf::new();
        for (first, count) in [(0u32, 60u32), (60, 40)] {
            read(&mut st, &rs, &p, &mut bar, &mut c2h, &mut dma, first, count).unwrap();
            assert_eq!(dma.data().len(), count as usize * fb);
            for (j, f) in dma.data().chunks_exact(fb).enumerate() {
                let k = first + j as u32;
                assert_eq!(header_stamp(&f[..16]), Some(((4 + k as i128) * MS / HEADER_TICK_FS) as u64));
                assert_eq!(u16::from_le_bytes([f[16 + 2 * 999], f[17 + 2 * 999]]), Engine::word(k, 999));
            }
        }
        assert!(eng.borrow().regs.get(&0x4068) == Some(&0));
    }

    // A user stop at k < N: the progress, the stop and the playable range agree (hardware check 5.6).
    #[test]
    fn stop_before_n_keeps_what_was_recorded() {
        let s = settings(1000);
        let mut st = State::handover(1, None);
        let p = st.configure(&s, None, None).unwrap().1;
        let (eng, mut bar, mut c2h) = engine(Engine::new(0, 10 * MS, 3, p.ratio));
        let n = start(&mut st, &s, &p, &mut bar, &mut c2h).unwrap();
        let mut x = status(&mut bar, n);
        for _ in 0..4 {
            x = status(&mut bar, n);
        }
        assert!(!x.complete(n) && x.recorded > 0 && x.recorded < n);
        stop(&mut st, &mut bar, &mut c2h).unwrap();
        assert_eq!(eng.borrow().recorded, x.recorded);
        let sts = stamps(&mut st, &s, &p, &mut bar, &mut c2h, 0, x.recorded).unwrap();
        assert_eq!(sts.len(), x.recorded as usize);
        assert_eq!(sts.last().unwrap().latch, Some(((x.recorded as i128 - 1) * 10 * MS / LATCH_TICK_FS) as u64));
    }
}
