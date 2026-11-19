// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The simulated unit, for host runs of the board code (ADR 0053, docs/board-on-a-host.md 3): what the unit answers
//! on the register window and sends in the C2H stream, as docs/protocols/ describes it (acquisition-start.md,
//! record-play.md, the head of `seg`). Behind the feature `sim`, which only tests switch on: the programs for the scope
//! are built without it (ADR 0041).
//!
//! `Engine` is the model of the record engine and the play path for host runs of the whole sequence (the steps of
//! `State::record_steps`, `read_steps_at`, `segment_stamp_steps` and `disarm_steps`, and the average mode). It is
//! still written in `seg` and only named here: moving its code out of `seg` changed the programs for the scope,
//! although they never call it (the compiler lays out the rest of hal anew). It moves with the stage that runs on a
//! unit anyway (docs/board-on-a-host.md 6).
//!
//! `Unit` is the simulated unit itself (docs/board-on-a-host.md 3), first for triggered acquisitions. It reads from the
//! registers written what decides which bytes come (arm, stop, the frames of a play, the window, the channels, the
//! header, the compression) and answers the reads the loop polls, all on one virtual clock. What the signal is, when it
//! triggers and how the front ends scale it is the test's `Scene`; the writes of the front ends, the ADC, the trigger
//! and the time base are recorded, not interpreted (3.2). Every access leaves a line in the trace. Not modelled yet:
//! the roll stream, scan and the segmented record (stage 3), the hardware average (`Engine` has it), the front-end bus
//! and the single lines (stage 1).

pub use crate::dho::seg::sim::{engine, Engine, SimBus, SimC2h};

use crate::dho::boot::Clock;
use crate::dho::bus::{Bus, FPGA_VERSION};
use crate::dho::seg::HEADER_TICK_FS;
use crate::dho::{encode, pairs, scu};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

/// Femtoseconds of a microsecond.
const US: i128 = 1_000_000_000;
/// What an access costs on the virtual clock (docs/board-on-a-host.md 3.5; invented, the same on every computer): a
/// register write 10 us (the gap of `Bar1`), a read 1 us, a byte of the stream 10 ns (100 MB/s).
pub const WRITE_FS: i128 = 10 * US;
pub const READ_FS: i128 = US;
pub const STREAM_BYTE_FS: i128 = 10_000_000;

/// How the front ends and the time base turn the scene into codes and samples: what the settings the test asks for
/// would make of it, told by the test and not read from the registers (3.2). A new view takes effect at the next arm.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// (volts per division, offset in volts) of CH1..CH4, as `seq::decode_volts` takes them
    pub cal: [(f64, f64); 4],
    /// the sample period, fs
    pub dot_fs: i128,
    /// points before and after the trigger of a record (0x4050 / 0x4054, `hz::Params::pre` / `post`)
    pub pre: u32,
    pub post: u32,
}

/// The signal and its triggers: the volts of a channel at a time (seconds on the virtual clock), and the instants at
/// which it triggers: `first_fs` and every `period_fs` after it, none without a period. With `auto_fs`, a frame is
/// recorded that long after the arm or the frame before it when no trigger came (AUTO).
pub struct Scene {
    pub volts: Box<dyn Fn(usize, f64) -> f64>,
    pub first_fs: i128,
    pub period_fs: Option<i128>,
    pub auto_fs: Option<i128>,
    pub view: View,
}

impl Scene {
    /// The first trigger instant at or after `t`, or the AUTO frame `auto_fs` after `t` without one.
    pub fn next_trigger(&self, t: i128) -> Option<i128> {
        let periodic = self.period_fs.filter(|&p| p > 0).map(|p| {
            if t <= self.first_fs {
                self.first_fs
            } else {
                self.first_fs + (t - self.first_fs + p - 1) / p * p
            }
        });
        periodic.or(self.auto_fs.map(|a| t + a))
    }
}

/// One line of the trace: the virtual time in us, then what happened. A poll read again with the same answer is one
/// line with its count.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Write { us: i128, off: u32, v: u32 },
    Read { us: i128, off: u32, v: u32, times: u32 },
    Stream { us: i128, bytes: usize },
    Recorded { us: i128, frame: u32, trigger_us: i128 },
    Played { us: i128, first: u32, last: u32, bytes: usize },
}

/// An armed record: the frames `first..=last`, and when it was armed.
struct Rec {
    first: u32,
    last: u32,
    armed_fs: i128,
    /// frames recorded so far
    n: u32,
}

/// The simulated unit. Its sides are `Bar`, `Stream` and `Time`, which share it (`unit`).
pub struct Unit {
    pub scene: Scene,
    view: View,
    regs: BTreeMap<u32, u32>,
    t_fs: i128,
    rec: Option<Rec>,
    /// the ring: the trigger time of every frame recorded, by its number
    ring: BTreeMap<u32, i128>,
    /// frames recorded by the last record that was stopped
    recorded: u32,
    stream: VecDeque<u8>,
    prev_4004: u32,
    pub trace: Vec<Event>,
}

impl Unit {
    pub fn new(scene: Scene) -> Unit {
        Unit {
            view: scene.view,
            scene,
            regs: BTreeMap::new(),
            t_fs: 0,
            rec: None,
            ring: BTreeMap::new(),
            recorded: 0,
            stream: VecDeque::new(),
            prev_4004: 0,
            trace: Vec::new(),
        }
    }

    /// The virtual time, us.
    pub fn now_us(&self) -> i128 {
        self.t_fs / US
    }

    /// The trigger time of frame `k` of the ring, fs, when it was recorded.
    pub fn trigger_fs(&self, k: u32) -> Option<i128> {
        self.ring.get(&k).copied()
    }

    /// The trace as text, one event a line (for a comparison of two runs).
    pub fn trace_text(&self) -> String {
        self.trace.iter().map(|e| format!("{:?}\n", e)).collect()
    }

    fn reg(&self, r: u32) -> u32 {
        self.regs.get(&r).copied().unwrap_or(0)
    }

    /// Records the frames whose trigger has come by now. The first takes the first trigger once its pre-trigger points
    /// are filled after the arm (record-play.md: a record takes the pre-trigger fill, then the wait for the next edge);
    /// a trigger inside the fill is not taken. Each later frame takes the first trigger after the pre- and post-trigger
    /// points of the frame before it. The record stops by itself after its last frame.
    fn advance(&mut self) {
        let Some(r) = self.rec.as_mut() else { return };
        let span = (self.view.pre + self.view.post) as i128 * self.view.dot_fs;
        loop {
            let from = match r.n {
                0 => r.armed_fs + self.view.pre as i128 * self.view.dot_fs,
                _ => self.ring[&(r.first + r.n - 1)] + span,
            };
            let Some(at) = self.scene.next_trigger(from).filter(|&at| at <= self.t_fs) else { break };
            let k = r.first + r.n;
            self.ring.insert(k, at);
            r.n += 1;
            self.trace.push(Event::Recorded { us: self.t_fs / US, frame: k, trigger_us: at / US });
            if r.first + r.n > r.last {
                self.recorded = r.n;
                self.rec = None;
                break;
            }
        }
    }

    /// The code words of channel `c` for `n` points from memory point `start` of the frame triggered at `trig`.
    fn words(&self, c: usize, trig: i128, start: u32, n: usize) -> Vec<u16> {
        let v = self.view;
        let (scale, offset) = v.cal[c];
        (0..n)
            .map(|i| {
                let t = trig + (start as i128 + i as i128 - v.pre as i128) * v.dot_fs;
                encode::code((self.scene.volts)(c, t as f64 * 1e-15), scale, offset)
            })
            .collect()
    }

    /// Plays the frames `first..=last` of the ring into the stream: per frame its header while 0x1010 bit 31 is clear,
    /// then 0x11f0 points of every channel of 0x4008[3:0] from point 0x1108 of the window, through the compression
    /// when 0x1170 bit 31 is set (k in bits 13:0, `pairs::model` of the scene's samples). The read-out rounds 0x11f0 up
    /// so that the words of a frame fill its 8-byte stride (`scu::readout_window`): the points beyond the window are
    /// what `seq::unpad` drops as the padding. That the unit sends further samples there, and not zeros, is assumed.
    fn play(&mut self, first: u32, last: u32) {
        let mask = self.reg(0x4008) & 0xf;
        let (n, start) = (self.reg(0x11f0) as usize, self.reg(0x1108));
        let c1170 = self.reg(0x1170);
        let k = if c1170 & 1 << 31 != 0 { (c1170 & 0x3fff).max(1) as usize } else { 1 };
        let at = self.stream.len();
        for f in first..=last {
            // a frame that was not recorded plays the trigger time 0 (not known what the unit sends)
            let trig = self.ring.get(&f).copied().unwrap_or(0);
            if self.reg(0x1010) & 1 << 31 == 0 {
                let before = f.checked_sub(1).and_then(|b| self.ring.get(&b)).copied().unwrap_or(0);
                let stamp = (before / HEADER_TICK_FS) as u64 & ((1 << 48) - 1);
                self.stream.extend(encode::header(f as u16, stamp));
            }
            let ch: Vec<Vec<u16>> = (0..4)
                .map(|c| {
                    if mask & (1 << c) == 0 {
                        return Vec::new();
                    }
                    let raw = self.words(c, trig, start, n * k);
                    if k > 1 {
                        pairs::model(&raw, k, pairs::LAYOUT)
                    } else {
                        raw
                    }
                })
                .collect();
            self.stream.extend(encode::interleave([&ch[0], &ch[1], &ch[2], &ch[3]], mask));
        }
        let bytes = self.stream.len() - at;
        self.trace.push(Event::Played { us: self.t_fs / US, first, last, bytes });
    }

    fn write(&mut self, off: u32, v: u32) {
        self.t_fs += WRITE_FS;
        self.advance();
        self.trace.push(Event::Write { us: self.t_fs / US, off, v });
        self.regs.insert(off, v);
        if off != 0x4004 {
            return;
        }
        let rise = v & !self.prev_4004;
        self.prev_4004 = v;
        if v & 1 << 31 != 0 {
            // stop: the record ends where it is, the ring keeps it
            if let Some(r) = self.rec.take() {
                self.recorded = r.n;
            }
        }
        let c = self.reg(0x400c);
        let (first, last) = (self.reg(0x4010), self.reg(0x4018));
        if rise & 1 != 0 && c & 0x4000_0000 != 0 {
            self.play(first, last);
        } else if rise & 1 != 0 && c & 0x2200_0000 == 0x2200_0000 {
            // a new view takes effect at the arm
            self.view = self.scene.view;
            self.rec = Some(Rec { first, last, armed_fs: self.t_fs, n: 0 });
            self.recorded = 0;
        } else if rise & 4 != 0 {
            // play-last: the newest frame of the ring
            if let Some((&f, _)) = self.ring.iter().next_back() {
                self.play(f, f);
            }
        }
    }

    fn read(&mut self, off: u32) -> u32 {
        self.t_fs += READ_FS;
        self.advance();
        let v = match off {
            0x4000 => FPGA_VERSION,
            0x4080 => {
                let n = self.rec.as_ref().map_or(self.recorded, |r| r.n);
                if n > 0 {
                    scu::PLAYINFO_DONE
                } else {
                    0
                }
            }
            0x4010 => match &self.rec {
                Some(r) => r.first + r.n.max(1) - 1,
                None => self.reg(0x4014) + self.recorded.max(1) - 1,
            },
            0x4034 => match &self.rec {
                Some(r) => scu::status::WAIT_TRIG | if r.n > 0 { 0x20 } else { 0 },
                None => scu::status::STOPPED,
            },
            _ => self.reg(off),
        };
        let us = self.t_fs / US;
        match self.trace.last_mut() {
            Some(Event::Read { off: o, v: w, times, .. }) if *o == off && *w == v => *times += 1,
            _ => self.trace.push(Event::Read { us, off, v, times: 1 }),
        }
        v
    }

    fn stream_read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let k = b.len().min(self.stream.len());
        if k == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "nothing played"));
        }
        for (d, s) in b.iter_mut().zip(self.stream.drain(..k)) {
            *d = s;
        }
        self.t_fs += k as i128 * STREAM_BYTE_FS;
        self.advance();
        self.trace.push(Event::Stream { us: self.t_fs / US, bytes: k });
        Ok(k)
    }
}

/// The register window of a shared `Unit`.
pub struct Bar(pub Rc<RefCell<Unit>>);
/// The C2H stream of a shared `Unit`.
pub struct Stream(pub Rc<RefCell<Unit>>);
/// The virtual clock of a shared `Unit`, as the cold boot takes a clock.
pub struct Time(pub Rc<RefCell<Unit>>);

impl Bus for Bar {
    fn wr(&mut self, off: u32, v: u32) {
        self.0.borrow_mut().write(off, v)
    }
    fn rd(&mut self, off: u32) -> u32 {
        self.0.borrow_mut().read(off)
    }
}

impl std::io::Read for Stream {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().stream_read(b)
    }
}

impl Clock for Time {
    fn sleep_us(&mut self, us: u64) {
        let mut u = self.0.borrow_mut();
        u.t_fs += us as i128 * US;
        u.advance();
    }
    fn now_us(&mut self) -> u64 {
        self.0.borrow().now_us() as u64
    }
}

/// A shared unit of `scene` and its three sides.
pub fn unit(scene: Scene) -> (Rc<RefCell<Unit>>, Bar, Stream, Time) {
    let u = Rc::new(RefCell::new(Unit::new(scene)));
    (u.clone(), Bar(u.clone()), Stream(u.clone()), Time(u))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::bus::DmaBuf;
    use crate::dho::hz;
    use crate::dho::seq::{self, Settings, State, Step};
    use std::io::Read;

    /// (V/div, offset) of CH1..CH4: each channel's sine stays within four divisions of its offset.
    const CAL: [(f64, f64); 4] = [(0.1, 0.0), (0.2, 0.05), (0.5, -0.1), (1.0, 0.0)];

    /// A sine of 0.3 V x (c + 1) at (c + 1) MHz on channel c.
    fn sine(c: usize, t: f64) -> f64 {
        0.3 * (c as f64 + 1.0) * (2.0 * std::f64::consts::PI * 1e6 * (c as f64 + 1.0) * t).sin()
    }

    /// The scene of `sine` in the view `p` gives, triggering every `period_fs` from 1 us.
    fn scene(p: &hz::Params, period_fs: Option<i128>, auto_fs: Option<i128>) -> Scene {
        let view = View { cal: CAL, dot_fs: p.dot_fs as i128, pre: p.pre, post: p.post };
        Scene { volts: Box::new(sine), first_fs: US, period_fs, auto_fs, view }
    }

    fn configured(s: &Settings) -> (State, hz::Params) {
        // the ADC already in the mode of the channels: no calibration set needed
        let mut st = State::handover(s.mask, None);
        let p = st.configure(s, None, None).unwrap().1;
        (st, p)
    }

    /// One batch of `s` read with `seq::acquire_batch` against the unit: the frames read, the bytes, the unit.
    fn batch(s: &Settings) -> (u32, Vec<u8>, Rc<RefCell<Unit>>, hz::Params) {
        let (mut st, p) = configured(s);
        // triggers every 200 ns: a batch of short records is complete before the first look at the frame count
        let (u, mut bar, mut c2h, _) = unit(scene(&p, Some(200_000_000), None));
        let mut dma = DmaBuf::new();
        let n = seq::acquire_batch(&mut st, s, &p, &mut bar, &mut c2h, &mut dma, false, 0, |_| {}).unwrap();
        (n, dma.data().to_vec(), u, p)
    }

    // Break it catches: a model or a codec that sends another sample, channel or frame than the decode reads, or a run
    // that is not the same twice
    #[test]
    fn a_batch_read_against_the_unit_is_the_scene_in_every_frame() {
        for mask in [1u32, 3, 0b0111, 0xf] {
            let s = Settings { mask, tdiv: 10e-9, frames: 4, window: None, ..Settings::default() };
            let (n, bytes, u, p) = batch(&s);
            assert_eq!(n, 4, "mask {:#x}: frames read", mask);
            let fb = seq::frame_bytes(&s, &p);
            assert_eq!(bytes.len(), 4 * fb, "mask {:#x}", mask);
            if mask == 1 {
                assert_ne!(fb % 8, 0, "one case has padding between the frames: four frames still decode in place");
            }
            let u = u.borrow();
            for (k, f) in bytes.chunks_exact(fb).enumerate() {
                let trig = u.trigger_fs(k as u32).expect("recorded");
                let v = seq::decode_volts(f, mask, CAL);
                for c in (0..4).filter(|c| mask & (1 << c) != 0) {
                    assert_eq!(v[c].len(), p.memsel as usize, "mask {:#x} CH{}", mask, c + 1);
                    let half = CAL[c].0 / crate::dho::readout::CODES_PER_DIV / 2.0;
                    for (i, &x) in v[c].iter().enumerate() {
                        let t = (trig + (i as i128 - p.pre as i128) * p.dot_fs as i128) as f64 * 1e-15;
                        let want = sine(c, t);
                        assert!(
                            (x as f64 - want).abs() <= half * 1.001 + 1e-6,
                            "mask {:#x} frame {} CH{} point {}",
                            mask,
                            k,
                            c + 1,
                            i
                        );
                    }
                }
            }
            let again = batch(&s).2.borrow().trace_text();
            assert_eq!(u.trace_text(), again, "mask {:#x}: the same run gives the same trace", mask);
        }
    }

    // Break it catches: an executor that sets a gear past the lines it is given, or waits for the ADC word on the wall
    // clock
    #[test]
    fn the_executor_drives_the_front_end_the_lines_and_the_clock_it_is_given() {
        use crate::dho::afe::Op;
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (_, p) = configured(&s);
        let (u, mut bar, mut c2h, mut clock) = unit(scene(&p, None, None));
        let mut afe = crate::dho::boot::sim::AfeStub { writes: 0, reads: 0 };
        let mut lines = crate::dho::lines::Recorded::default();
        let steps = [
            Step::Afe(Op::Afe { ch: 0, reg: 0x21, val: 0x0a91 }),
            Step::Afe(Op::Gear { ch: 1, gear: 1 }),
            Step::Afe(Op::Adc(0x0154_5251)),
            Step::Sleep { us: 300 },
            Step::Wr((0x10b0, 5)),
        ];
        let t0 = u.borrow().now_us();
        let mut dma = DmaBuf::new();
        seq::execute_on(&steps, &mut bar, Some(&mut afe), &mut c2h, &mut lines, &mut dma, &mut clock, |_| {}, || false)
            .unwrap();
        assert_eq!((afe.writes, lines.gears.clone()), (1, vec![(1, 1)]));
        // 100 us before and after the ADC word, its write, the sleep, the last write
        assert_eq!(u.borrow().now_us() - t0, 100 + 10 + 100 + 300 + 10);
        let writes: Vec<(u32, u32)> = u
            .borrow()
            .trace
            .iter()
            .filter_map(|e| if let Event::Write { off, v, .. } = e { Some((*off, *v)) } else { None })
            .collect();
        assert_eq!(writes, [(0x1034, 0x0154_5251), (0x10b0, 5)]);
        // without a front-end bus an AFE step is refused, not skipped
        let none: Option<&mut crate::dho::boot::sim::AfeStub> = None;
        let r =
            seq::execute_on(&steps[..1], &mut bar, none, &mut c2h, &mut lines, &mut dma, &mut clock, |_| {}, || false);
        assert!(r.is_err());
    }

    // Break it catches: a self-calibration that sets gears past its lines, waits on the wall clock, or configures a
    // channel through another executor than the one it is given
    #[test]
    fn the_self_calibration_runs_on_the_sides_it_is_given() {
        use crate::dho::afe::Op;
        use crate::dho::selfcal::{CalHw, Unit};
        // the made-up calibration set (madeup.rs), in its files as calfile writes them
        let pay = crate::dho::madeup::payloads([0, 1, 2, 3]);
        let f =
            |n: &str| crate::dho::calfile::build(&pay.iter().find(|p| p.0 == n).unwrap().1, crate::dho::madeup::STAMP);
        let cal = crate::dho::afe::Cal::from_bytes(
            &f("cal_lsb.hex"),
            &f("cal_afe_zero.hex"),
            &f("cal_afe_bandwidth.hex"),
            f("cal_vertical.hex"),
        );
        let cal_adc = crate::dho::adc::CalAdc::from_file_bytes(&f("cal_adc.hex")).unwrap();
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (mut state, p) = configured(&s);
        let (u, mut bar, mut c2h, mut clock) = unit(scene(&p, None, None));
        let mut afe = crate::dho::boot::sim::AfeStub { writes: 0, reads: 0 };
        let mut lines = crate::dho::lines::Recorded::default();
        let cancel = || false;
        let mut unit = Unit {
            bar: &mut bar,
            afe: &mut afe,
            c2h: &mut c2h,
            lines: &mut lines,
            clock: &mut clock,
            state: &mut state,
            cal: &cal,
            cal_adc: Some(&cal_adc),
            cancel: &cancel,
            adc_new: None,
        };
        let t0 = u.borrow().now_us();
        unit.apply(&[Op::Gear { ch: 2, gear: 1 }, Op::Adc(0x0155_000c), Op::Afe { ch: 2, reg: 0x21, val: 1 }]);
        unit.sleep_ms(5);
        assert_eq!(u.borrow().now_us() - t0, 100 + 10 + 100 + 5000, "the ADC word and the sleep on the clock");
        // the normal set-up of one channel: its steps through `execute_on`, its gear on the lines
        unit.configure(1, 0).expect("configure on the simulated unit");
        assert!(afe.writes > 1, "the front end was written: {}", afe.writes);
        assert_eq!(lines.gears.first(), Some(&(2, 1)));
        assert!(lines.gears.iter().skip(1).any(|&(ch, _)| ch == 1), "configure set the gear of CH2: {:?}", lines.gears);
    }

    // Break it catches: an executor that waits on the wall clock, or a ring wait that counts frames by anything but
    // the time that passed
    #[test]
    fn the_ring_fills_on_the_virtual_clock() {
        let s = Settings { tdiv: 10e-9, frames: 10, window: None, ..Settings::default() };
        let (mut st, p) = configured(&s);
        // a trigger every 5 ms: the poll waits for the first, then the ring gets one more every 5 ms of the 12 ms wait
        let (u, mut bar, mut c2h, mut clock) = unit(scene(&p, Some(5_000 * US), None));
        let mut dma = DmaBuf::new();
        let n = seq::acquire_batch_on(
            &mut st,
            &s,
            &p,
            &mut bar,
            &mut c2h,
            &mut dma,
            &mut clock,
            false,
            12,
            |_| {},
            || false,
            || false,
        )
        .unwrap();
        assert_eq!(n, 3, "frames at 5, 10 and 15 ms; the wait ends at about 17 ms");
        assert!(u.borrow().now_us() > 17_000 && u.borrow().now_us() < 18_000, "{} us", u.borrow().now_us());
        assert_eq!(dma.data().len(), 3 * seq::frame_bytes(&s, &p));
    }

    // Break it catches: a done flag or a status without a trigger, a clock that does not run on the accesses, polls that
    // fill the trace line by line
    #[test]
    fn without_a_trigger_nothing_is_recorded_and_auto_records_after_its_time() {
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        for auto in [None, Some(50 * US)] {
            let (mut st, p) = configured(&s);
            let (u, mut bar, _, _) = unit(scene(&p, None, auto));
            for step in st.arm_only_steps(&s, &p) {
                if let Step::Wr((off, v)) = step {
                    bar.wr(off, v);
                }
            }
            let t0 = u.borrow().now_us();
            let done: Vec<u32> = (0..100).map(|_| bar.rd(0x4080) & scu::PLAYINFO_DONE).collect();
            assert_eq!(u.borrow().now_us() - t0, 100, "a read costs 1 us");
            match auto {
                None => {
                    assert!(done.iter().all(|&d| d == 0), "no trigger, no frame");
                    assert_eq!(bar.rd(0x4034) & scu::status::WAIT_TRIG, scu::status::WAIT_TRIG);
                    let polls =
                        u.borrow().trace.iter().filter(|e| matches!(e, Event::Read { off: 0x4080, .. })).count();
                    assert_eq!(polls, 1, "100 equal polls are one line of the trace");
                }
                Some(_) => {
                    let at = done.iter().position(|&d| d != 0).expect("AUTO records a frame");
                    assert!(at > 30 && at < 60, "the AUTO frame comes 50 us after the arm: at read {}", at);
                    assert_eq!(bar.rd(0x4034), scu::status::STOPPED, "the batch of one frame is complete");
                }
            }
        }
    }

    // Break it catches: a compressed play that is not the extremes of the scene's blocks in the order they occur
    // (pairs.rs, verified on the unit)
    #[test]
    fn a_compressed_frame_holds_the_extremes_of_each_block_in_the_order_they_occur() {
        let s = Settings { tdiv: 1e-6, frames: 1, compress: 4, window: None, ..Settings::default() };
        let (n, bytes, u, p) = batch(&s);
        assert_eq!(n, 1);
        let k = seq::compress_k(&s, &p) as usize;
        let got = &seq::deinterleave(&bytes, 1)[0];
        assert_eq!(got.len(), seq::compressed_len(&s, &p) as usize);
        let trig = u.borrow().trigger_fs(0).unwrap();
        let raw = |i: usize| {
            let t = (trig + (i as i128 - p.pre as i128) * p.dot_fs as i128) as f64 * 1e-15;
            encode::code(sine(0, t), CAL[0].0, CAL[0].1)
        };
        for (j, pair) in got.as_chunks::<2>().0.iter().enumerate() {
            let block: Vec<u16> = (2 * k * j..2 * k * (j + 1)).map(raw).collect();
            let lo = (0..block.len()).min_by_key(|&i| (block[i], i)).unwrap();
            let hi = (0..block.len()).max_by_key(|&i| (block[i], usize::MAX - i)).unwrap();
            let want = if lo <= hi { [block[lo], block[hi]] } else { [block[hi], block[lo]] };
            assert_eq!(*pair, want, "pair {}", j);
        }
    }

    // Break it catches: a play whose frames lose their header, their number or the stamp of the trigger before them
    #[test]
    fn a_played_frame_carries_its_number_and_the_trigger_before_it() {
        let (t0, period) = (1_000_000i128, 50_000_000i128);
        let (_, mut bar, mut c2h) = engine(Engine::new(t0, period, 1, 1));
        // play frames 2..=3 of 8 words each, with the header (0x1010 bit 31 clear)
        bar.wr(0x11f0, 8);
        bar.wr(0x4010, 2);
        bar.wr(0x4018, 3);
        bar.wr(0x400c, 0x4000_0000);
        bar.wr(0x4004, 1);
        let mut b = vec![0u8; 2 * (16 + 2 * 8)];
        c2h.read_exact(&mut b).unwrap();
        for (f, k) in b.as_chunks::<32>().0.iter().zip([2u32, 3]) {
            assert_eq!(u16::from_le_bytes([f[8], f[9]]), k as u16, "header word 4");
            let before = ((t0 + (k as i128 - 1) * period) / HEADER_TICK_FS) as u64;
            assert_eq!(crate::dho::seg::header_stamp(&f[..16]), Some(before), "frame {}", k);
            assert_eq!(u16::from_le_bytes([f[16], f[17]]), Engine::word(k, 0), "frame {}: its words", k);
        }
        assert!(c2h.read(&mut [0u8; 1]).is_err(), "nothing more was played");
    }
}
