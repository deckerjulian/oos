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
//! and the time base are recorded, not interpreted (3.2). Every access leaves a line in the trace. The roll stream and
//! scan (roll-mode.md) come as chunks on the same clock (`Roll`), with the logic analyser's chunks when it is on. The
//! hardware average (0x1004 bit 3) keeps a running average of the triggers since its clear (`Avg`). The generator's
//! DDS block and its lines (afg.md) keep what they were told (`GenState`, the side `Gen`). The frequency counter
//! (counter.md) counts the scene's trigger instants over its gates (`Counter`), the DVM block (docs/fork/dvm.md) sums
//! the codes of a lane over its windows (`Dvm`). The stamps of a segmented record are those of the head of `seg`: the
//! latch holds the trigger of the last frame played, the first-trigger stamp that of the first frame recorded since its
//! clear, both in ticks of 0.8 ns.

pub use crate::dho::seg::sim::{engine, Engine, SimBus, SimC2h};

use crate::dho::boot::Clock;
use crate::dho::bus::{Bus, FPGA_VERSION};
use crate::dho::roll::{CHUNK_BYTES, CHUNK_WORDS};
use crate::dho::seg::{HEADER_TICK_FS, LATCH_TICK_FS};
use crate::dho::{acq, encode, pairs, readout, scu};
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

/// Chunks of a scan sweep: 10 divisions of 100 columns, a chunk each (roll-mode.md 9.1, at 1-2-5 time bases).
pub const SWEEP_CHUNKS: u64 = 1000;
/// Chunks of a sweep still to come when 0x40d0 reports it done (roll-mode.md 9.1: 3 to 4 on the unit).
pub const DONE_EARLY: u64 = 4;
/// One tick of the interval of a segmented record, 0x4068 (acquisition-start.md, `record_seq`: interval / 6.4e6).
const INTERVAL_TICK_FS: i128 = 6_400_000;
/// What 0x11e8 counts for a whole chunk waiting (roll-mode.md 2.2 and 8).
const FILL_PER_CHUNK: u32 = 27;

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
/// recorded that long after the arm or the frame before it when no trigger came (AUTO). `la`: the 16 lines of the logic
/// analyser at a time (bit n = Dn), where a test has them.
pub struct Scene {
    pub volts: Box<dyn Fn(usize, f64) -> f64>,
    pub la: Option<Box<dyn Fn(f64) -> u16>>,
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
    Write {
        us: i128,
        off: u32,
        v: u32,
    },
    Read {
        us: i128,
        off: u32,
        v: u32,
        times: u32,
    },
    Stream {
        us: i128,
        bytes: usize,
    },
    Recorded {
        us: i128,
        frame: u32,
        trigger_us: i128,
    },
    Played {
        us: i128,
        first: u32,
        last: u32,
        bytes: usize,
    },
    /// the generator's lines (`Gen`): the relay code and the supply after a change of either
    GenLines {
        us: i128,
        relays: u8,
        power: bool,
    },
    /// roll and scan: chunks made, the first with this counter
    Chunks {
        us: i128,
        counter: u16,
        n: u64,
    },
}

/// An armed record: the frames `first..=last`, and when it was armed.
struct Rec {
    first: u32,
    last: u32,
    armed_fs: i128,
    /// frames recorded so far
    n: u32,
    /// the zone trigger did not take a trigger: the next is looked for from here (`MaskUnit`)
    rearm_fs: Option<i128>,
}

/// The roll or scan stream (roll-mode.md 2, 5 and 9): started by the FSM run (0x4004 bit 0) while 0x4008 bit 17
/// (roll) or bit 22 (scan) is set, ended by a stop (0x4004 bit 31). The point spacing is read from the display chain
/// written: 0x1170 bits 27:0 (10 x ratio x k) sample periods of the view, times the interpolation fraction
/// (0x115c + 1) / 2^32 while 0x1154 bit 0 is clear; a chunk holds 0x11fc points of each channel. Each point is the
/// scene's sample at its time (that the unit sends min/max pairs is assumed, roll-mode.md 3.3; the decode does not tell
/// them apart). A scan sweep has `SWEEP_CHUNKS` chunks and then waits for the next start; its counter starts at 1 in
/// every sweep. It does not wait for a trigger: 0x1204 reports one at the column of the screen's centre when 0x4058 is
/// 0 (a scan trigger position is not modelled). With the logic analyser on at the start (0x4008 bit 24, la.md 4.4) a
/// chunk of 100 LA words (header word 1 bit 1) comes before each analog chunk, over the same time, and the two share
/// the counter (LA 1, analog 2, LA 3: roll-mode.md, "Scan with the LA on"); words of a scene without lines are 0.
struct Roll {
    scan: bool,
    la: bool,
    start_fs: i128,
    point_fs: i128,
    per: u32,
    /// chunks made since the start
    made: u64,
}

/// The hardware average (horizontal-config.md 3, the head of `avg`): on while 0x1004 bit 3 is set, over the last
/// 0x10b0 + 1 triggers since the last rising edge of 0x10b4 bit 0 (the clear). Each frame recorded in average mode is
/// stored as the average of the record at its trigger and the records before it, each memory point the mean of the
/// codes the frames that have it hold there, truncated: `avg::mean_words` for records of one length. Assumed, and kept
/// simple: that the average runs over the triggers recorded (the running reading of `avg::Kind`;
/// `seq::State::read_steps` assumes the frames reach it when they are played, which gives the same frames while every
/// recorded frame is played, as the loop does), that a frame holds the average as it stood after its own trigger, and
/// that a record of another length or scale without a clear is averaged with the ones before it. 0x10b0 reads back the
/// triggers in the average less one, up to the count written (`avg::progress`: that it counts is assumed; once the
/// average is full it is the value written either way).
#[derive(Default)]
struct Avg {
    /// the codes of each channel over the whole record, of the triggers in the average, the newest last
    raw: VecDeque<[Vec<u16>; 4]>,
    /// per channel and memory point: the sum of the codes in `raw` and the number of frames that have the point
    sum: [Vec<(u64, u32)>; 4],
    /// triggers since the clear
    triggers: u64,
    /// the averaged record of each frame of the ring recorded in average mode
    frames: BTreeMap<u32, [Vec<u16>; 4]>,
}

impl Avg {
    fn clear(&mut self) {
        self.raw.clear();
        self.sum = Default::default();
        self.triggers = 0;
    }

    /// Frame `k` recorded with the codes `raw` in an average of `n`: its averaged record.
    fn push(&mut self, k: u32, raw: [Vec<u16>; 4], n: usize) {
        for (sum, codes) in self.sum.iter_mut().zip(&raw) {
            if sum.len() < codes.len() {
                sum.resize(codes.len(), (0, 0));
            }
            for (s, &c) in sum.iter_mut().zip(codes) {
                *s = (s.0 + c as u64, s.1 + 1);
            }
        }
        self.raw.push_back(raw);
        self.triggers += 1;
        while self.raw.len() > n.max(1) {
            let Some(old) = self.raw.pop_front() else { break };
            for (sum, codes) in self.sum.iter_mut().zip(&old) {
                for (s, &c) in sum.iter_mut().zip(codes) {
                    *s = (s.0 - c as u64, s.1 - 1);
                }
            }
        }
        let mean = std::array::from_fn(|c| {
            let len = self.raw.back().map_or(0, |f| f[c].len());
            self.sum[c][..len].iter().map(|&(s, n)| (s / n.max(1) as u64) as u16).collect()
        });
        self.frames.insert(k, mean);
    }
}

/// The bank selects of the generator's block (afg.md 2.1): B (DDS), M (modulation), A (amplitude).
const GEN_B: u32 = 0x3ff0;
const GEN_M: u32 = 0x3ff4;
const GEN_A: u32 = 0x3ff8;
/// The DDS clock (afg.md 2.3: 156.25 MHz, verified on the unit, section 14) and one table cycle in its phase.
const DDS_HZ: f64 = 156.25e6;
const DDS_CYCLE: f64 = (1u64 << 52) as f64;

/// What the generator was told, as afg.md 2.2 reads its registers: the bank selected by the last select written 1
/// takes the writes of 0x3000..0x307c (that a select written 0 ends its bank, and that the last one written 1 holds
/// while another is still 1, is assumed: afg.md 2.1 calls the meaning of the selects assumed), and the lines of `Gen`.
/// Nothing is read back from the block (afg.md 1); it makes no signal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GenState {
    /// bank B 0x3000 bit 0: the output on
    pub output: bool,
    /// the frequency of the phase increment, bank B 0x3048..0x3054 (56 bits; 2^52 a cycle at `DDS_HZ`), Hz
    pub hz: f64,
    /// the amplitude and offset DAC codes, bank A 0x3070 / 0x3078 (written as 0xffff - code)
    pub ampl: u32,
    pub offset: u32,
    /// the amplitude (Vpp) and the offset (V) the codes give on the path the relays select, without the calibration
    /// file (afg.md 5.2): A = ((code / 65535 x 5.29) - 2.75) x 1.158 x G, O = ((code / 65535 x 5.29) - 2.75) / (0.11
    /// with the x10 stage, else 1.1). G from the relay bits as afg.md 4.2 decodes them (assumed there, consistent with
    /// every row): bit 0 x0.316, bit 1 x0.1, bit 2 x10, bit 3 x0.1
    pub vpp: f64,
    pub offset_v: f64,
    /// bank M 0x3000: 0 none, 0x82 AM, 0x84 FM, 0x88 PM
    pub modulation: u32,
    /// samples written to the RAM port, bank B 0x3038, since the start
    pub samples: u64,
    /// the lines: relay code (bit n = afg_in(n+1)) and the supply
    pub relays: u8,
    pub power: bool,
}

/// The generator's block between writes: the bank selected and its registers.
#[derive(Default)]
struct GenRegs {
    bank: Option<u32>,
    regs: BTreeMap<(u32, u32), u32>,
    samples: u64,
    relays: u8,
    power: bool,
}

impl GenRegs {
    fn write(&mut self, off: u32, v: u32) {
        match (off, self.bank) {
            (GEN_B | GEN_M | GEN_A, _) if v & 1 != 0 => self.bank = Some(off),
            (GEN_B | GEN_M | GEN_A, Some(b)) if b == off => self.bank = None,
            (0x3000..=0x307c, Some(b)) => {
                self.regs.insert((b, off), v);
                self.samples += (b == GEN_B && off == 0x3038) as u64;
            }
            _ => {}
        }
    }

    fn state(&self) -> GenState {
        let r = |b: u32, off: u32| self.regs.get(&(b, off)).copied().unwrap_or(0) as u64;
        let inc = r(GEN_B, 0x3048) | r(GEN_B, 0x304c) << 16 | r(GEN_B, 0x3050) << 32 | (r(GEN_B, 0x3054) & 0xff) << 48;
        let code = |off: u32| self.regs.get(&(GEN_A, off)).map_or(0, |&v| 0xffff - (v & 0xffff));
        let bits = [0.316_227_766, 0.1, 10.0, 0.1];
        let gain: f64 = (0..4).filter(|b| self.relays & 1 << b != 0).map(|b| bits[b]).product();
        let dac = |c: u32| c as f64 / 65535.0 * 5.29 - 2.75;
        let (ampl, offset) = (code(0x3070), code(0x3078));
        GenState {
            output: r(GEN_B, 0x3000) & 1 != 0,
            hz: inc as f64 * DDS_HZ / DDS_CYCLE,
            ampl,
            offset,
            vpp: dac(ampl) * 1.158 * gain,
            offset_v: dac(offset) / if self.relays & 4 != 0 { 0.11 } else { 1.1 },
            modulation: r(GEN_M, 0x3000) as u32,
            samples: self.samples,
            relays: self.relays,
            power: self.power,
        }
    }
}

/// One tick of the frequency counter's results, and of its gate register (counter.md 1).
const COUNT_TICK_FS: i128 = 800_000;
const GATE_TICK_FS: i128 = 6_400_000;

/// The frequency counter (counter.md): started by the rising edge of the run bit of 0x2210 (gates of 0x2214 ticks back
/// to back from then), cleared by its clear bit, stopped by 0x2210 = 0. A write of 1 to 0x2218 latches the results of
/// the last whole gate into 0x221c..0x2240: the ticks from the gate's start to its first edge and from its last edge to
/// its end, the edges in it with the frequency flag when there is one, the last whole period before the gate's end
/// with its flag, and the edges since the start or the clear. Its edges are the scene's trigger instants: what the
/// comparator fires on is not known (0x2200 / 0x2204, counter.md 4), so it is assumed to fire where the trigger does,
/// and the scene's triggers are its signal's edges. Without a whole gate the results latched are 0 (assumed).
#[derive(Default)]
struct Counter {
    /// the start of the first gate while it runs
    run_fs: Option<i128>,
    /// the start of the total: the run or the last clear
    total_fs: i128,
    /// the results latched, as the ten words read
    words: [u32; 10],
}

/// One sample of an ADC lane at 156.25 MSa/s, the rate the DVM block and the measure unit count in (docs/fork/dvm.md,
/// on the DHO924S: a window of 2^28 samples every 1.71 s, whatever the time base; self-cal.md 2.4).
const CORE_SAMPLE_FS: i128 = 6_400_000;
/// Samples of the scene the model takes per window at most, evenly spread; the sums are scaled to the window's.
const DVM_POINTS_LOG2: u32 = 12;

/// The DVM block (docs/fork/dvm.md): a write of 0x2244 that clears the restart bit (bit 0) after it was set starts
/// windows of 2^exp samples (bits 5:1) of one lane (bits 9:8) back to back from then; exp 0 (the stop) runs none. The
/// rising edge of 0x2248 bit 0 latches the sum and the sum of squares of the last whole window. The samples are the
/// stored 12-bit codes of the channel on that lane (`dvm::lane`; with one channel every lane carries it): the scene's
/// volts as the read-out codes, divided by 16 (a read-path gain of 1, which the scene's view has). Assumed: that the
/// windows run back to back from the restart, that the latch takes the last whole one, the zeros without one, that a
/// lane without a channel sums nothing, the low half of 0x2250 and bits 7:6 of 0x2244 as 0; and the sums are those of
/// 2^12 samples of the window, scaled.
#[derive(Default)]
struct Dvm {
    /// the start of the first window while it runs, its exp and lane
    run: Option<(i128, u32, u32)>,
    /// the sum and the sum of squares latched
    sums: (u64, u64),
}

/// The mask unit's comparator and the zone logic on its sample path (docs/fork/hwmask.md, hwzone.md). The table
/// (0x1194) and the two zone tables (0x11d8, 0x11dc) take a column's word while its write bit is set. A frame recorded
/// while the mask test is on (0x4020 bit 15) counts as tested and, when a sample of the slot's channel (0x1190 bits
/// 23:20, `mask::slot`) lies outside the limits of its column, as failed; with the zone trigger armed (0x4020 bit 1)
/// a trigger is recorded only when the frame qualifies (each zone on in 0x11d0 touched, or not, as its mode says) and
/// then counts. The samples are the read-out codes >> 8, column `i / n` for sample `i` and the strobe count `n` of
/// 0x1184 bits 9:0. 0x1190 bit 29 and 30 clear the counters, its rising bit 28 latches them. Verified on the unit: the
/// scale, the slots and the columns at 10k points (hwmask.md, "On the DHO924S"), the qualification (hwzone.md).
/// Assumed: that a rejected trigger costs a whole record before the next is taken, the latch, the counters' clear.
/// Not modelled: stop on fail, the stall of a strobe count the record does not hold, the zone's flags in 0x4030 and
/// 0x4038 (they read what was written).
#[derive(Default)]
struct MaskUnit {
    /// per column: the lower and upper limit and whether each is on
    table: BTreeMap<u32, (u8, u8, bool, bool)>,
    /// per zone and column in it: the lower and upper limit
    zones: [BTreeMap<u32, (u8, u8)>; 2],
    /// frames tested (with the zone trigger: qualified) and failed since their clear, and the two latched
    frames: u64,
    fails: u64,
    latched: (u64, u64),
}

/// Samples of the scene the measure unit's model takes per run, evenly spread over it.
const MEASURE_POINTS: i128 = 1024;

/// Measure codes (16-bit scale) one step of a core's offset register moves the slot of that core by: about half an ADC
/// step of 16 codes (docs/fork/core-offset.md, rounds 11 and 12 on the unit: 8 to 10.6 codes).
const CORE_STEP_CODES: i32 = 9;
/// The slot of the measure unit that shows ADC core n (core-offset.md, round 11: mode 2 on the unit; the other modes
/// alike, assumed).
const CORE_SLOT: [usize; 8] = [0, 2, 1, 3, 4, 6, 5, 7];

/// The ADC measure unit (self-cal.md 2.4, the head of `levels`): the rising edge of 0x1040 bit 31 starts a run of 2^e
/// samples per core (e = bits 4:0) at 156.25 MHz, bit 30 reads set once it is over. Bits 10:8 select the slot 0x1044
/// (its mean on the 16-bit scale) and 0x1048 (its 12-bit maximum in bits 27:16, minimum in 11:0) show. A slot shows
/// the channel `levels::slots` gives it (assumed there) over the run: the scene's read-out codes, which are those of
/// the measure unit with a read-path gain of 1, as the scene has. 0xeeeeeeee in both before a run is over (what
/// `levels::run` refuses). Assumed: the extremes and the mean of 1024 samples of the run. Where the test gives the
/// cores' own offsets (`Unit::cores`), each slot is moved by its core's offset register against them,
/// `CORE_STEP_CODES` a register step; else every core is alike.
#[derive(Default)]
struct Measure {
    /// the start and the end of the last run
    run: Option<(i128, i128)>,
    /// the offset register of each ADC core as written through 0x1034 (adc-mode.md 2.2: `0x01000000 | (0x14 + 8n) <<
    /// 16 | o & 0x7ff`, 11 bits signed)
    offsets: [i32; 8],
}

/// The simulated unit. Its sides are `Bar`, `Stream`, `Time` and the generator's lines `Gen`, which share it (`unit`).
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
    roll: Option<Roll>,
    /// scan: 0x40d0 (the sweep is complete), until the next start
    sweep_done: bool,
    /// the stamps: the last frame played, the latched trigger time (0x40b0/0x40b4, ticks), the trigger of the first
    /// frame recorded since the first-trigger stamp was cleared (0x40c0/0x40c4, fs)
    last_played: Option<u32>,
    latched: u64,
    first_trigger_fs: Option<i128>,
    avg: Avg,
    gen: GenRegs,
    counter: Counter,
    dvm: Dvm,
    mask: MaskUnit,
    measure: Measure,
    /// the value of each ADC core's offset register at which that core reads the scene as it is (its own offset, which
    /// the stored calibration and the core trim aim at); None: every core alike, whatever is written
    pub cores: Option<[i32; 8]>,
    pub trace: Vec<Event>,
    /// microseconds slept through `Time` (the waits of the code under test, not the cost of its accesses)
    pub slept_us: u64,
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
            roll: None,
            sweep_done: false,
            last_played: None,
            latched: 0,
            first_trigger_fs: None,
            avg: Avg::default(),
            gen: GenRegs::default(),
            counter: Counter::default(),
            dvm: Dvm::default(),
            mask: MaskUnit::default(),
            measure: Measure::default(),
            cores: None,
            trace: Vec::new(),
            slept_us: 0,
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

    /// The ADC cores' offset registers as last written (`Measure`).
    pub fn core_offsets(&self) -> [i32; 8] {
        self.measure.offsets
    }

    /// What the generator was told so far.
    pub fn generator(&self) -> GenState {
        self.gen.state()
    }

    /// The scene's trigger instants in `[a, b)`: how many, the first and the last (the AUTO frames are none).
    fn edges(&self, a: i128, b: i128) -> (u64, Option<i128>, Option<i128>) {
        let (f, Some(p)) = (self.scene.first_fs, self.scene.period_fs.filter(|&p| p > 0)) else {
            return (0, None, None);
        };
        let k0 = if a <= f { 0 } else { (a - f + p - 1) / p };
        let k1 = if b <= f { 0 } else { (b - f + p - 1) / p };
        let n = (k1 - k0).max(0);
        let at = |k: i128| (n > 0).then_some(f + k * p);
        (n as u64, at(k0), at(k1 - 1))
    }

    /// The counter's results of the last whole gate, latched (`Counter`).
    fn count_latch(&mut self) {
        let gate = self.reg(crate::dho::counter::GATE) as i128 * GATE_TICK_FS;
        let Some(run) = self.counter.run_fs.filter(|_| gate > 0) else {
            self.counter.words = [0; 10];
            return;
        };
        let whole = (self.t_fs - run) / gate;
        if whole == 0 {
            self.counter.words = [0; 10];
            return;
        }
        let (g0, g1) = (run + (whole - 1) * gate, run + whole * gate);
        let (n, first, last) = self.edges(g0, g1);
        let ticks = |fs: i128| (fs / COUNT_TICK_FS) as u64;
        let start = first.map_or(0, |e| ticks(e - g0));
        let end = last.map_or(0, |e| ticks(g1 - e));
        // the last whole period before the gate's end: the two edges before it
        let (m, _, before) = self.edges(run, g1);
        let period = before.filter(|_| m >= 2).zip(self.scene.period_fs).map(|(_, p)| ticks(p));
        let total = self.edges(self.counter.total_fs, self.t_fs).0;
        let flag = |on: bool| if on { 1u64 << 63 } else { 0 };
        let w = [start | flag(n > 0), end, n, period.unwrap_or(0) | flag(period.is_some()), total];
        self.counter.words = std::array::from_fn(|i| (w[i / 2] >> (32 * (i % 2))) as u32);
    }

    /// The DVM block's sums of its last whole window, latched (`Dvm`).
    fn dvm_latch(&mut self) {
        self.dvm.sums = (0, 0);
        let Some((run, exp, lane)) = self.dvm.run else { return };
        let window = (1i128 << exp) * CORE_SAMPLE_FS;
        let whole = (self.t_fs - run) / window;
        let mask = self.reg(0x4008) & 0xf;
        let one = (mask.count_ones() == 1).then(|| mask.trailing_zeros() as usize);
        let ch = one.or_else(|| (0..4).find(|&c| crate::dho::dvm::lane(mask, c) == Some(lane)));
        let Some(c) = ch.filter(|_| whole > 0) else { return };
        let g0 = run + (whole - 1) * window;
        let points = exp.min(DVM_POINTS_LOG2);
        let (scale, offset) = self.scene.view.cal[c];
        let (mut sum, mut sq) = (0u64, 0u64);
        for i in 0..1i128 << points {
            let t = g0 + (2 * i + 1) * window / (2 << points);
            let code = (encode::code((self.scene.volts)(c, t as f64 * 1e-15), scale, offset) as u64 + 8) >> 4;
            let code = code.min(4095);
            (sum, sq) = (sum + code, sq + code * code);
        }
        self.dvm.sums = (sum << (exp - points), sq << (exp - points));
    }

    /// The channel the mask unit compares (its slot, `mask::slot`) and the strobe count; None without a slot or with
    /// its channel not acquired (`MaskUnit`).
    fn unit_source(&self) -> Option<(usize, usize)> {
        let mask = self.reg(0x4008) & 0xf;
        let slot = (self.reg(crate::dho::mask::CTRL) & crate::dho::mask::ctrl::SLOT_MASK) >> 20;
        let slot = (slot != 0).then(|| slot.trailing_zeros())?;
        let c = (0..4).find(|&c| crate::dho::mask::slot(mask, c) == Some(slot))?;
        Some((c, (self.reg(crate::dho::mask::COMPRESS) & 0x3ff).max(1) as usize))
    }

    /// The 8-bit codes the mask unit compares of channel `c` in the frame triggered at `at`, from sample `from` on.
    fn unit_codes(&self, c: usize, at: i128, from: usize, n: usize) -> Vec<u8> {
        self.words(c, at, from as u32, n).iter().map(|&w| (w >> 8) as u8).collect()
    }

    /// Whether the zone trigger takes the trigger at `at`: each zone on touched (a sample of one of its columns within
    /// its limits, or the trace crossing them from the sample before) when its mode asks for it, not touched else
    /// (`MaskUnit`). True while it is not armed.
    fn zone_takes(&self, at: i128) -> bool {
        use crate::dho::zone::ctrl;
        let w = self.reg(crate::dho::zone::CTRL);
        if self.reg(0x4020) & crate::dho::zone::SCU_ENABLE == 0 || w >> ctrl::ON_SHIFT & 3 == 0 {
            return true;
        }
        let Some((c, n)) = self.unit_source() else { return true };
        let len = (self.view.pre + self.view.post) as usize;
        (0..2).filter(|k| w >> (ctrl::ON_SHIFT + k) & 1 != 0).all(|k| {
            let touched = self.mask.zones[k as usize].iter().any(|(&col, &(lo, hi))| {
                let first = (col as usize * n).min(len);
                let from = first.saturating_sub(1);
                let codes = self.unit_codes(c, at, from, ((col as usize + 1) * n).min(len) - from);
                let inside = codes[first - from..].iter().any(|x| (lo..=hi).contains(x));
                inside || codes.windows(2).any(|p| (p[0] < lo && p[1] > hi) || (p[0] > hi && p[1] < lo))
            });
            touched == (w >> (ctrl::MODE_SHIFT + k) & 1 != 0)
        })
    }

    /// The frame triggered at `at` through the mask test while it is on: tested, and failed when a sample lies beyond
    /// its column's limits; else counted while the zone trigger, which took it, is armed (`MaskUnit`).
    fn mask_test(&mut self, at: i128) {
        if self.reg(0x4020) & crate::dho::mask::scu::ENABLE == 0 {
            self.mask.frames += (self.reg(0x4020) & crate::dho::zone::SCU_ENABLE != 0) as u64;
            return;
        }
        let Some((c, n)) = self.unit_source() else { return };
        let codes = self.unit_codes(c, at, 0, (self.view.pre + self.view.post) as usize);
        let fail = codes.iter().enumerate().any(|(i, &x)| {
            self.mask.table.get(&((i / n) as u32)).is_some_and(|&(lo, hi, le, he)| (le && x < lo) || (he && x > hi))
        });
        self.mask.frames += 1;
        self.mask.fails += fail as u64;
    }

    /// A write to one of the mask unit's registers (`MaskUnit`).
    fn mask_write(&mut self, off: u32, v: u32, before: u32) {
        use crate::dho::mask::{ctrl, table, CTRL, TABLE};
        use crate::dho::zone::{table as ztable, TABLES};
        if off == TABLE && v & table::WRITE != 0 {
            let lim = (v as u8, (v >> table::HI_SHIFT) as u8, v & table::LO_EN != 0, v & table::HI_EN != 0);
            self.mask.table.insert(v >> table::COL_SHIFT & 0x3ff, lim);
        }
        if let Some(k) = TABLES.iter().position(|&r| r == off).filter(|_| v & ztable::WRITE != 0) {
            let col = v >> ztable::COL_SHIFT & 0x3ff;
            if v & ztable::IN_ZONE != 0 {
                self.mask.zones[k].insert(col, (v as u8, (v >> ztable::HI_SHIFT) as u8));
            } else {
                self.mask.zones[k].remove(&col);
            }
        }
        if off == CTRL {
            if v & ctrl::CLEAR_FAILS != 0 {
                self.mask.fails = 0;
            }
            if v & ctrl::CLEAR_FRAMES != 0 {
                self.mask.frames = 0;
            }
            if v & ctrl::LATCH != 0 && before & ctrl::LATCH == 0 {
                self.mask.latched = (self.mask.frames, self.mask.fails);
            }
        }
    }

    /// What the measure unit's slot selected in 0x1040 shows of its last run: (0x1044, 0x1048) (`Measure`).
    fn measure_slot(&self) -> (u32, u32) {
        let none = (0xeeee_eeee, 0xeeee_eeee);
        let Some((a, b)) = self.measure.run.filter(|r| r.1 <= self.t_fs) else { return none };
        let (mask, slot) = (self.reg(0x4008) & 0xf, (self.reg(0x1040) >> 8 & 7) as usize);
        let Some(c) = (0..4).find(|&c| crate::dho::levels::slots(mask, c).contains(&slot)) else { return none };
        let (scale, offset) = self.scene.view.cal[c];
        let codes: Vec<u16> = (0..MEASURE_POINTS)
            .map(|i| a + (2 * i + 1) * (b - a) / (2 * MEASURE_POINTS))
            .map(|t| encode::code((self.scene.volts)(c, t as f64 * 1e-15), scale, offset))
            .map(|x| (x as i32 + self.core_shift(slot)).clamp(0, u16::MAX as i32) as u16)
            .collect();
        let (lo, hi) = codes.iter().fold((u16::MAX, 0), |(l, h), &x| (l.min(x), h.max(x)));
        let mean = codes.iter().map(|&x| x as f64).sum::<f64>() / codes.len() as f64;
        (mean.round() as u32, ((hi >> 4) as u32) << 16 | (lo >> 4) as u32)
    }

    /// Measure codes the core shown in `slot` reads off the scene: its offset register against its own offset
    /// (`Unit::cores`), `CORE_STEP_CODES` a step; 0 without them.
    fn core_shift(&self, slot: usize) -> i32 {
        let Some(own) = self.cores else { return 0 };
        let n = CORE_SLOT.iter().position(|&k| k == slot).unwrap_or(slot);
        (self.measure.offsets[n] - own[n]) * CORE_STEP_CODES
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
    /// points of the frame before it and the wait of 0x4068 (the interval of a segmented record, `scu::record_arm`; that
    /// the FPGA waits it after each frame is assumed). The record stops by itself after its last frame.
    fn advance(&mut self) {
        self.stream_chunks();
        let wait = self.reg(0x4068) as i128 * INTERVAL_TICK_FS;
        let Some(mut r) = self.rec.take() else { return };
        let span = (self.view.pre + self.view.post) as i128 * self.view.dot_fs;
        let mut new = Vec::new();
        loop {
            let from = match (r.rearm_fs, r.n) {
                (Some(t), _) => t,
                (None, 0) => r.armed_fs + self.view.pre as i128 * self.view.dot_fs,
                _ => self.ring[&(r.first + r.n - 1)] + span + wait,
            };
            let Some(at) = self.scene.next_trigger(from).filter(|&at| at <= self.t_fs) else { break };
            // a trigger the zone trigger does not take: the FSM arms again and fills a whole record (assumed)
            r.rearm_fs = (!self.zone_takes(at)).then_some(at + span);
            if r.rearm_fs.is_some() {
                continue;
            }
            let k = r.first + r.n;
            self.ring.insert(k, at);
            self.first_trigger_fs.get_or_insert(at);
            r.n += 1;
            self.trace.push(Event::Recorded { us: self.t_fs / US, frame: k, trigger_us: at / US });
            new.push((k, at));
            if r.first + r.n > r.last {
                self.recorded = r.n;
                break;
            }
        }
        if r.first + r.n <= r.last {
            self.rec = Some(r);
        }
        for (k, at) in new {
            self.average(k, at);
            self.mask_test(at);
        }
    }

    /// Frame `k`, triggered at `at`, into the hardware average while it is on (`Avg`); else it holds its own samples.
    fn average(&mut self, k: u32, at: i128) {
        if self.reg(0x1004) & 1 << 3 == 0 {
            self.avg.frames.remove(&k);
            return;
        }
        let mask = self.reg(0x4008) & 0xf;
        let len = (self.view.pre + self.view.post) as usize;
        let raw = std::array::from_fn(|c| if mask & 1 << c != 0 { self.words(c, at, 0, len) } else { Vec::new() });
        self.avg.push(k, raw, self.reg(acq::AVG_COUNT) as usize + 1);
    }

    /// Makes the chunks of the roll or scan stream whose last point has come by now, into the stream.
    fn stream_chunks(&mut self) {
        let Some(r) = self.roll.as_ref() else { return };
        let (scan, la, start_fs, point_fs, per, made) = (r.scan, r.la, r.start_fs, r.point_fs, r.per as usize, r.made);
        // the counter of the analog chunk k
        let counter = |k: u64| if la { 2 * k + 2 } else { k + 1 } as u16;
        let mask = self.reg(0x4008) & 0xf;
        let chunk_fs = per as i128 * point_fs;
        let mut n = 0;
        while !(scan && made + n >= SWEEP_CHUNKS) && start_fs + (made + n + 1) as i128 * chunk_fs <= self.t_fs {
            let k = made + n;
            if la {
                let mut words = [0u16; CHUNK_WORDS];
                for (j, w) in words.iter_mut().enumerate() {
                    let t = start_fs + k as i128 * per as i128 * point_fs + j as i128 * per as i128 * point_fs / 100;
                    *w = self.scene.la.as_ref().map_or(0, |f| f(t as f64 * 1e-15));
                }
                self.stream.extend(encode::chunk(counter(k) - 1, true, &words));
            }
            let ch: Vec<Vec<u16>> = (0..4)
                .map(|c| {
                    if mask & (1 << c) == 0 {
                        return Vec::new();
                    }
                    let (scale, offset) = self.view.cal[c];
                    (0..per)
                        .map(|j| {
                            let t = start_fs + (k as i128 * per as i128 + j as i128) * point_fs;
                            encode::code((self.scene.volts)(c, t as f64 * 1e-15), scale, offset)
                        })
                        .collect()
                })
                .collect();
            let bytes = encode::interleave([&ch[0], &ch[1], &ch[2], &ch[3]], mask);
            let mut words = [readout::MID as u16; CHUNK_WORDS];
            for (w, b) in words.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *w = u16::from_le_bytes(*b);
            }
            self.stream.extend(encode::chunk(counter(k), false, &words));
            n += 1;
        }
        if n == 0 {
            return;
        }
        if let Some(r) = self.roll.as_mut() {
            r.made += n;
            if r.scan && r.made + DONE_EARLY >= SWEEP_CHUNKS {
                self.sweep_done = true;
            }
        }
        let first = counter(made) - la as u16;
        self.trace.push(Event::Chunks { us: self.t_fs / US, counter: first, n: n << la as u32 });
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
                    let mut raw = self.words(c, trig, start, n * k);
                    // the averaged record where the frame has one; points past its end are the scene's (assumed)
                    if let Some(a) = self.avg.frames.get(&f) {
                        for (w, &x) in raw.iter_mut().zip(a[c].iter().skip(start as usize)) {
                            *w = x;
                        }
                    }
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
        self.last_played = Some(last);
        self.trace.push(Event::Played { us: self.t_fs / US, first, last, bytes });
    }

    /// The raw LA window of frame `f`, triggered at `at` (la.md 4.2): N = 0x1100 / 10 words from LA sample 0x1108 of
    /// its record, the read rounded up to 4 words, no header. The record has n_la = memsel x m / 2 LA samples at m / 2
    /// per analog sample, the trigger at LA sample pre x m / 2 (la.md 3.3, m = `la::sample_mode`); word k is the
    /// scene's lines at its time, 0 without lines and past the record (stale memory on the unit). The trigger index and
    /// the unit of 0x1108 are assumed there (test T1), and so are they here.
    fn play_la(&mut self, f: u32, at: i128) {
        let m = crate::dho::la::sample_mode(self.reg(0x4008)) as i128;
        let (n, start) = (self.reg(0x1100) / 10, self.reg(0x1108) as i128);
        let dot = self.view.dot_fs * 2 / m;
        let (pre, len) = (self.view.pre as i128 * m / 2, (self.view.pre + self.view.post) as i128 * m / 2);
        let bytes = crate::dho::la::read_bytes(n);
        let at_word = |k: i128| match &self.scene.la {
            Some(la) if start + k < len => la((at + (start + k - pre) * dot) as f64 * 1e-15),
            _ => 0,
        };
        let words: Vec<u16> = (0..bytes as i128 / 2).map(at_word).collect();
        self.stream.extend(words.iter().flat_map(|w| w.to_le_bytes()));
        self.trace.push(Event::Played { us: self.t_fs / US, first: f, last: f, bytes });
    }

    fn write(&mut self, off: u32, v: u32) {
        self.t_fs += WRITE_FS;
        self.advance();
        self.trace.push(Event::Write { us: self.t_fs / US, off, v });
        let before = self.regs.insert(off, v).unwrap_or(0);
        if off == scu::stamp::CTRL {
            if v & scu::stamp::FIRST_CLEAR != 0 {
                self.first_trigger_fs = None;
            }
            if v & scu::stamp::LATCH != 0 && before & scu::stamp::LATCH == 0 {
                let at = self.last_played.and_then(|k| self.ring.get(&k)).copied().unwrap_or(0);
                self.latched = (at / LATCH_TICK_FS) as u64;
            }
        }
        if (0x3000..=0x3ffc).contains(&off) {
            self.gen.write(off, v);
        }
        if off == crate::dho::counter::CTRL {
            if v & 4 != 0 {
                self.counter.total_fs = self.t_fs;
            }
            match (v & 1 != 0, before & 1 != 0) {
                (true, false) => self.counter.run_fs = Some(self.t_fs),
                (false, _) => self.counter.run_fs = None,
                _ => {}
            }
        }
        if off == crate::dho::counter::LATCH && v & 1 != 0 {
            self.count_latch();
        }
        if off == crate::dho::dvm::CFG && v & 1 == 0 && before & 1 != 0 {
            let exp = v >> 1 & 0x1f;
            self.dvm.run = (exp > 0).then_some((self.t_fs, exp, v >> 8 & 3));
        }
        if off == crate::dho::dvm::LATCH && v & 1 != 0 && before & 1 == 0 {
            self.dvm_latch();
        }
        self.mask_write(off, v, before);
        // an ADC word (bit 24) to a core's offset register 0x14 + 8n
        let adc_reg = v >> 16 & 0x7f;
        if off == 0x1034 && v & 1 << 24 != 0 && (0x14..0x54).contains(&adc_reg) && adc_reg % 8 == 4 {
            self.measure.offsets[(adc_reg as usize - 0x14) / 8] = ((v & 0x7ff) << 21) as i32 >> 21;
        }
        if off == 0x1040 && v & 1 << 31 != 0 && before & 1 << 31 == 0 {
            self.measure.run = Some((self.t_fs, self.t_fs + (1i128 << (v & 0x1f)) * CORE_SAMPLE_FS));
        }
        if off == acq::AVG_CLEAR && v & 1 != 0 && before & 1 == 0 {
            self.avg.clear();
        }
        if off != 0x4004 {
            return;
        }
        let rise = v & !self.prev_4004;
        self.prev_4004 = v;
        if v & 1 << 31 != 0 {
            // stop: the record ends where it is, the ring keeps it; a roll or scan stream ends, its chunks not read
            // are dropped
            if let Some(r) = self.rec.take() {
                self.recorded = r.n;
            }
            if self.roll.take().is_some() {
                self.stream.clear();
            }
        }
        let c = self.reg(0x400c);
        let (first, last) = (self.reg(0x4010), self.reg(0x4018));
        let r4008 = self.reg(0x4008);
        // the stream is the LA's alone (la.md 4.2): its words come with the play-last pulse, the FSM run after it plays
        // no analog frame (assumed: the unit sent the LA words alone)
        let la_only = r4008 & (1 << 24 | 1 << 23) == 1 << 24;
        if rise & 1 != 0 && r4008 & (1 << 17 | 1 << 22) != 0 {
            let r1170 = self.reg(0x1170) & 0x0fff_ffff;
            let frac = if self.reg(0x1154) & 1 == 0 { self.reg(0x115c) as i128 + 1 } else { 1 << 32 };
            let point_fs = (r1170.max(1) as i128 * self.scene.view.dot_fs * frac) >> 32;
            self.view = self.scene.view;
            self.roll = Some(Roll {
                scan: r4008 & 1 << 22 != 0,
                la: r4008 & 1 << 24 != 0,
                start_fs: self.t_fs,
                point_fs: point_fs.max(1),
                per: self.reg(0x11fc).clamp(1, CHUNK_WORDS as u32),
                made: 0,
            });
            self.sweep_done = false;
        } else if rise & 1 != 0 && c & 0x4000_0000 != 0 && !la_only {
            self.play(first, last);
        } else if rise & 1 != 0 && c & 0x2200_0000 == 0x2200_0000 {
            // a new view takes effect at the arm. The arm overwrites the ring from frame `first` on
            // (acquisition-start.md, "Reading a stored frame again"): its pre-trigger fill takes that frame at once, so
            // the record stored there is gone even when a stop comes before the trigger (assumed, as the loop's batch
            // path has it); the later frames are overwritten as they are recorded
            self.view = self.scene.view;
            self.ring.remove(&first);
            self.rec = Some(Rec { first, last, armed_fs: self.t_fs, n: 0, rearm_fs: None });
            self.recorded = 0;
        } else if rise & 4 != 0 {
            // play-last: the newest frame of the ring, its LA words while the stream is the LA's (0x4008 bit 24)
            if let Some((&f, &at)) = self.ring.iter().next_back() {
                if la_only {
                    self.play_la(f, at);
                } else {
                    self.play(f, f);
                }
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
            0x11e8 => (self.stream.len() / CHUNK_BYTES) as u32 * FILL_PER_CHUNK,
            scu::stamp::CTRL => {
                self.reg(off) & !scu::stamp::FIRST_VALID
                    | if self.first_trigger_fs.is_some() { scu::stamp::FIRST_VALID } else { 0 }
            }
            0x40b0 => self.latched as u32,
            0x40b4 => (self.latched >> 32) as u32,
            0x40c0 | 0x40c4 => {
                let t = (self.first_trigger_fs.unwrap_or(0) / LATCH_TICK_FS) as u64;
                if off == 0x40c0 {
                    t as u32
                } else {
                    (t >> 32) as u32
                }
            }
            acq::AVG_COUNT if self.reg(0x1004) & 1 << 3 != 0 => {
                (self.avg.triggers.min(self.reg(off) as u64 + 1).saturating_sub(1)) as u32
            }
            0x221c..=0x2240 => self.counter.words[(off - crate::dho::counter::RESULTS) as usize / 4],
            crate::dho::dvm::CFG => (self.dvm.sums.1 >> 32) as u32 & !0x3ff | self.reg(off) & 0x33f,
            crate::dho::dvm::SUM_LO => self.dvm.sums.0 as u32,
            crate::dho::dvm::SUM_HI => ((self.dvm.sums.0 >> 32) as u32) << 16,
            crate::dho::dvm::SQ_LO => self.dvm.sums.1 as u32,
            crate::dho::dvm::SQ_HI => (self.dvm.sums.1 >> 32) as u32,
            0x1040 => self.reg(off) & !(1 << 30) | (self.measure.run.is_some_and(|r| r.1 <= self.t_fs) as u32) << 30,
            0x1044 => self.measure_slot().0,
            0x1048 => self.measure_slot().1,
            crate::dho::mask::FRAMES_LO => self.mask.latched.0 as u32,
            crate::dho::mask::FRAMES_HI => (self.mask.latched.0 >> 32) as u32,
            crate::dho::mask::FAILS_LO => self.mask.latched.1 as u32,
            crate::dho::mask::FAILS_HI => (self.mask.latched.1 >> 32) as u32,
            0x40d0 => self.sweep_done as u32,
            0x1204 => match &self.roll {
                Some(r) if r.scan && !self.sweep_done && r.made >= SWEEP_CHUNKS / 2 && self.reg(0x4058) == 0 => {
                    1 << 31 | (SWEEP_CHUNKS / 2) as u32
                }
                _ => 0,
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
        u.slept_us += us;
        u.advance();
    }
    fn now_us(&mut self) -> u64 {
        self.0.borrow().now_us() as u64
    }
}

/// The generator's lines of a shared `Unit` (afg.md 4): the relays and the supply, kept in its `GenState` and traced.
pub struct Gen(pub Rc<RefCell<Unit>>);

impl Gen {
    fn set(&mut self, f: impl FnOnce(&mut GenRegs)) {
        let mut u = self.0.borrow_mut();
        f(&mut u.gen);
        let (us, relays, power) = (u.t_fs / US, u.gen.relays, u.gen.power);
        u.trace.push(Event::GenLines { us, relays, power });
    }
}

impl crate::dho::afg::GenLines for Gen {
    fn relays(&mut self, code: u8) {
        self.set(|g| g.relays = code)
    }
    fn power(&mut self, on: bool) {
        self.set(|g| g.power = on)
    }
    fn relay_code(&self) -> u8 {
        self.0.borrow().gen.relays
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
        Scene { volts: Box::new(sine), la: None, first_fs: US, period_fs, auto_fs, view }
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

    /// The arms in the trace of `u`: the writes of 0x400c that set record-seq mode (bits 29 and 25) without play.
    fn arms(u: &Unit) -> usize {
        let arm = |e: &&Event| matches!(e, Event::Write { off: 0x400c, v, .. } if v & 0x6200_0000 == 0x2200_0000);
        u.trace.iter().filter(arm).count()
    }

    // Break it catches: a batch armed ahead whose wait arms again or that records nothing while the batch before is
    // decoded, an arm that leaves the record stored in its first frame, a disarm that does not stop the FSM
    #[test]
    fn a_batch_armed_ahead_records_while_the_one_before_is_decoded_and_a_disarm_stops_it() {
        let s = Settings { tdiv: 10e-9, frames: 3, window: None, ..Settings::default() };
        let (mut st, p) = configured(&s);
        // a trigger every 100 us from 1 us
        let (u, mut bar, mut c2h, mut clock) = unit(scene(&p, Some(100 * US), None));
        let mut dma = DmaBuf::new();
        let mut batch = |st: &mut State, bar: &mut Bar, c2h: &mut Stream, clock: &mut Time, ahead: bool| {
            seq::acquire_batch_on(st, &s, &p, bar, c2h, &mut dma, clock, ahead, 1, |_| {}, || false, || false)
        };
        assert_eq!(batch(&mut st, &mut bar, &mut c2h, &mut clock, false), Ok(3));
        // armed ahead, then 1 ms of decoding: the ring records meanwhile, and the wait finds the batch complete
        seq::prearm(&mut st, &s, &p, &mut bar, &mut c2h, |_| {}).unwrap();
        let armed_us = u.borrow().now_us();
        clock.sleep_us(1000);
        assert_eq!(batch(&mut st, &mut bar, &mut c2h, &mut clock, true), Ok(3));
        let read_us = u.borrow().now_us();
        assert_eq!(arms(&u.borrow()), 2, "the batch armed ahead was armed again");
        for k in 0..3 {
            let at = u.borrow().trigger_fs(k).expect("recorded") / US;
            assert!(at > armed_us && at < armed_us + 1000, "frame {} at {} us: not during the decode", k, at);
        }
        let v = seq::decode_volts(&dma.data()[..seq::frame_bytes(&s, &p)], 1, CAL);
        let t0 = (u.borrow().trigger_fs(0).unwrap() - p.pre as i128 * p.dot_fs as i128) as f64 * 1e-15;
        assert!((v[0][0] as f64 - sine(0, t0)).abs() < 1e-3, "frame 0 is the record armed ahead");
        // armed ahead and stopped before its first trigger: the record of frame 0 is gone, frames 1 and 2 keep theirs
        let kept = u.borrow().trigger_fs(1);
        u.borrow_mut().scene.period_fs = None;
        seq::prearm(&mut st, &s, &p, &mut bar, &mut c2h, |_| {}).unwrap();
        seq::disarm(&mut st, &mut bar, &mut c2h, |_| {}).unwrap();
        assert_eq!(bar.rd(0x4034), scu::status::STOPPED, "the FSM stopped");
        u.borrow_mut().scene.period_fs = Some(100 * US);
        clock.sleep_us(1000);
        let u = u.borrow();
        assert_eq!((u.trigger_fs(0), u.trigger_fs(1)), (None, kept), "the ring after the disarm");
        let late = u.trace.iter().any(|e| matches!(e, Event::Recorded { us, .. } if *us > read_us));
        assert!(!late, "recorded after the stop");
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

    /// The roll (or scan) stream of `s` started on a unit as the loop starts it (roll-mode.md 6, A and B), on a scene of
    /// a 1 kHz sine of 0.3 V on every channel: the state, the roll parameters, the unit and its sides.
    fn rolling(s: &Settings, scan: bool) -> (State, crate::dho::roll::Params, Rc<RefCell<Unit>>, Bar, Stream, Time) {
        rolling_la(s, scan, None)
    }

    /// `rolling` with the logic analyser's lines `la` (and the LA on in the acquisition block, 0x4008 bit 24).
    fn rolling_la(
        s: &Settings,
        scan: bool,
        la: Option<Box<dyn Fn(f64) -> u16>>,
    ) -> (State, crate::dho::roll::Params, Rc<RefCell<Unit>>, Bar, Stream, Time) {
        use crate::dho::roll;
        let (mut st, p) = configured(s);
        if la.is_some() {
            st.sh.s4008 |= 1 << 24;
        }
        let rp = roll::params(s.mask, seq::tdiv_fs(s.tdiv), s.depth, &p);
        let view = View { cal: CAL, dot_fs: p.dot_fs as i128, pre: p.pre, post: p.post };
        let volts = Box::new(|_c: usize, t: f64| 0.3 * (2.0 * std::f64::consts::PI * 1e3 * t).sin());
        let (u, mut bar, c2h, clock) = unit(Scene { volts, la, first_fs: US, period_fs: None, auto_fs: None, view });
        let a = if scan {
            roll::config_scan(&mut st.sh, &rp, p.memsel, 0)
        } else {
            roll::config(&mut st.sh, &rp, p.memsel)
        };
        bar.apply(&a);
        bar.apply(&roll::start(&mut st.sh, s.mask));
        (st, rp, u, bar, c2h, clock)
    }

    /// The counters of the chunks in `b`.
    fn counters(b: &[u8]) -> Vec<u16> {
        b.as_chunks::<CHUNK_BYTES>().0.iter().map(|c| crate::dho::roll::counter(c)).collect()
    }

    // Break it catches: a roll stream at another spacing than `roll::params`, a fill level that does not count whole
    // chunks, chunks out of order, or points of each channel other than the scene's at their times
    #[test]
    fn the_roll_stream_is_the_scene_at_the_spacing_of_the_roll_parameters() {
        use crate::dho::roll;
        for mask in [0x1, 0x3, 0x7, 0xf] {
            let s = Settings { mask, tdiv: 0.2, window: None, ..Settings::default() };
            let (_, rp, u, mut bar, mut c2h, mut clock) = rolling(&s, false);
            let start = u.borrow().t_fs;
            let per = roll::points_per_chunk(mask);
            let chunk_us = (per as i128 * rp.spacing_fs as i128 / US) as u64;
            clock.sleep_us(10 * chunk_us + 1);
            let c = roll::chunks(bar.rd(0x11e8));
            assert_eq!(c, 10, "mask {:#x}: ten chunks of {} us", mask, chunk_us);
            let mut b = vec![0u8; c * CHUNK_BYTES];
            c2h.read_exact(&mut b).unwrap();
            assert_eq!(counters(&b), (1..=10).collect::<Vec<u16>>());
            let w = roll::split(&b, mask);
            for (c, w) in w.iter().enumerate() {
                if mask & 1 << c == 0 {
                    assert!(w.is_empty(), "CH{} is off", c + 1);
                    continue;
                }
                assert_eq!(w.len(), 10 * per, "mask {:#x}, CH{}", mask, c + 1);
                for (i, &x) in seq::volts(w, CAL[c].0, CAL[c].1).iter().enumerate() {
                    let t = (start + i as i128 * rp.spacing_fs as i128) as f64 * 1e-15;
                    let want = 0.3 * (2.0 * std::f64::consts::PI * 1e3 * t).sin();
                    assert!(
                        (x as f64 - want).abs() <= CAL[c].0 / 7500.0,
                        "CH{} point {}: {} V, not {} V",
                        c + 1,
                        i,
                        x,
                        want
                    );
                }
            }
        }
    }

    // Break it catches: a stop that leaves the chunks not read in the stream or in the fill level, so that the next
    // read after a change of settings gets old chunks
    #[test]
    fn a_stop_drops_the_chunks_not_read() {
        use crate::dho::roll;
        let s = Settings { mask: 1, tdiv: 0.2, window: None, ..Settings::default() };
        let (mut st, _, _u, mut bar, mut c2h, mut clock) = rolling(&s, false);
        clock.sleep_us(20_000);
        assert!(roll::chunks(bar.rd(0x11e8)) > 0, "chunks wait");
        bar.apply(&roll::stop(&mut st.sh));
        assert_eq!(bar.rd(0x11e8), 0, "none after the stop");
        clock.sleep_us(20_000);
        assert_eq!(bar.rd(0x11e8), 0, "and none made after it");
        assert!(c2h.read(&mut [0u8; 1]).is_err(), "nothing left to read");
    }

    // Break it catches: a sweep that does not end after its 1000 chunks, a done flag that is not early, a trigger
    // reported away from the centre, a counter that does not start again with the next sweep
    #[test]
    fn a_scan_sweep_ends_after_its_chunks_and_the_next_counts_from_1() {
        use crate::dho::roll;
        let s = Settings { mask: 1, tdiv: 0.2, window: None, ..Settings::default() };
        let (mut st, rp, _u, mut bar, mut c2h, mut clock) = rolling(&s, true);
        let chunk_us = (roll::points_per_chunk(1) as i128 * rp.spacing_fs as i128 / US) as u64;
        assert_eq!(chunk_us, 2000, "roll-mode.md 9.1: 2 ms a chunk at 200 ms/div");
        clock.sleep_us(500 * chunk_us + 1);
        assert_eq!((bar.rd(0x1204), bar.rd(0x40d0)), (1 << 31 | 500, 0), "the centre column, not done");
        clock.sleep_us(496 * chunk_us);
        assert_eq!((bar.rd(0x1204), bar.rd(0x40d0)), (0, 1), "done 4 chunks early");
        clock.sleep_us(1_000_000);
        let c = roll::chunks(bar.rd(0x11e8));
        assert_eq!(c, 1000, "the sweep stops after its last column");
        let mut b = vec![0u8; c * CHUNK_BYTES];
        c2h.read_exact(&mut b).unwrap();
        assert_eq!(counters(&b), (1..=1000).collect::<Vec<u16>>());
        // the next sweep, as the loop starts it in RUN
        bar.apply(&roll::scan_done(&mut st.sh));
        let p = configured(&s).1;
        bar.apply(&roll::config_scan(&mut st.sh, &rp, p.memsel, 0));
        bar.apply(&roll::start(&mut st.sh, s.mask));
        assert_eq!(bar.rd(0x40d0), 0, "done clears with the start");
        clock.sleep_us(2 * chunk_us + 1);
        let mut b = vec![0u8; 2 * CHUNK_BYTES];
        c2h.read_exact(&mut b).unwrap();
        assert_eq!(counters(&b), vec![1, 2]);
    }

    // Break it catches: LA chunks that do not alternate with the analog ones, a counter they do not share, LA words
    // at another spacing than `roll::la_spacing_fs` or other than the scene's lines
    #[test]
    fn with_the_la_on_its_chunks_come_before_each_analog_chunk() {
        use crate::dho::roll;
        let s = Settings { mask: 1, tdiv: 0.2, window: None, ..Settings::default() };
        // the lines: a count of 7 us steps (every word another)
        let lines = |t: f64| (t * 1e6 / 7.0) as u64 as u16;
        let (_, rp, u, mut bar, mut c2h, mut clock) = rolling_la(&s, false, Some(Box::new(lines)));
        let start = u.borrow().t_fs;
        let chunk_us = (roll::points_per_chunk(1) as i128 * rp.spacing_fs as i128 / US) as u64;
        clock.sleep_us(10 * chunk_us + 1);
        let c = roll::la_chunks(roll::chunks(bar.rd(0x11e8)));
        assert_eq!(c, 20, "ten LA and ten analog chunks");
        let mut b = vec![0u8; c * CHUNK_BYTES];
        c2h.read_exact(&mut b).unwrap();
        assert_eq!(counters(&b), (1..=20).collect::<Vec<u16>>());
        let la: Vec<bool> = b.as_chunks::<CHUNK_BYTES>().0.iter().map(|c| roll::is_la_chunk(c)).collect();
        assert!(la.iter().enumerate().all(|(i, &x)| x == (i % 2 == 0)), "LA first, then analog: {:?}", la);
        let (w, lw) = roll::split_la(&b, 1);
        assert_eq!((w[0].len(), lw.len()), (1000, 1000));
        for (i, &x) in lw.iter().enumerate() {
            let want = lines((start + i as i128 * roll::la_spacing_fs(&rp) as i128) as f64 * 1e-15);
            assert!(x.abs_diff(want) <= 1, "LA word {}: {:#x}, not {:#x}", i, x, want);
        }
    }

    // Break it catches: a hardware average that is not the mean of the triggers since the clear, over the last 0x10b0 +
    // 1 of them, a frame that holds another average than the one after its own trigger, a clear that does not start it
    // over, a progress in 0x10b0 that does not count, or a frame of the normal mode that is averaged
    #[test]
    fn the_hardware_average_is_the_mean_of_the_triggers_since_its_clear() {
        let avg = Settings {
            tdiv: 100e-9,
            depth: 1000,
            frames: 10,
            window: None,
            mode: acq::Mode::Average { count: 16 },
            ..Settings::default()
        };
        let mut st = State::handover(avg.mask, None);
        let (cfg, p) = st.configure(&avg, None, None).unwrap();
        // a level of 0.05 V x (g % 7) for the whole record of trigger g, every 20 us from 1 us
        let period = 20 * US;
        let level = move |t: f64| 0.05 * (((t * 1e15 - US as f64) / period as f64).round() as i64).rem_euclid(7) as f64;
        let view = View { cal: CAL, dot_fs: p.dot_fs as i128, pre: p.pre, post: p.post };
        let scene = Scene {
            volts: Box::new(move |_, t| level(t)),
            la: None,
            first_fs: US,
            period_fs: Some(period),
            auto_fs: None,
            view,
        };
        let (u, bar, c2h, clock) = unit(scene);
        let mut sides = (bar, c2h, clock);
        let run = |steps: &[Step], (bar, c2h, clock): &mut (Bar, Stream, Time)| {
            seq::execute_bus_on(steps, bar, c2h, &mut DmaBuf::new(), clock, &mut |_| {}, &|| false).unwrap()
        };
        run(&cfg, &mut sides);
        assert_eq!(sides.0.rd(acq::AVG_COUNT), 0, "nothing in the average after the clear");
        // one batch of 10 frames, each against the mean of the last `n` levels since the clear (`since`)
        let mut dma = DmaBuf::new();
        let mut check = |st: &mut State,
                         (s, p): (&Settings, &hz::Params),
                         since: &mut Vec<f64>,
                         n: usize,
                         sides: &mut (Bar, Stream, Time)| {
            let (bar, c2h, clock) = sides;
            let got = seq::acquire_batch_on(st, s, p, bar, c2h, &mut dma, clock, false, 12, |_| {}, || false, || false);
            assert_eq!(got, Ok(10));
            for (k, f) in dma.data().chunks_exact(seq::frame_bytes(s, p)).enumerate() {
                let at = u.borrow().trigger_fs(k as u32).unwrap();
                since.push(level(at as f64 * 1e-15));
                let last = &since[since.len().saturating_sub(n)..];
                let want = last.iter().sum::<f64>() / last.len() as f64;
                let v = &seq::decode_volts(f, 1, CAL)[0];
                let code = CAL[0].0 / crate::dho::readout::CODES_PER_DIV;
                let ok = v.iter().all(|&x| (x as f64 - want).abs() <= code * 1.01);
                assert!(ok, "frame {}: {} V, not {} V", k, v[0], want);
            }
            bar.rd(acq::AVG_COUNT)
        };
        // two batches: 10 triggers in, then the last 16 of 20
        let mut since = Vec::new();
        assert_eq!(check(&mut st, (&avg, &p), &mut since, 16, &mut sides), 9, "0x10b0: 10 triggers in");
        assert_eq!(check(&mut st, (&avg, &p), &mut since, 16, &mut sides), 15, "0x10b0: full, the value written");
        // the clear: the next frames average only the triggers after it
        let clear = st.avg_clear_steps();
        let mut fresh = Vec::new();
        run(&clear, &mut sides);
        assert_eq!(check(&mut st, (&avg, &p), &mut fresh, 16, &mut sides), 9, "0x10b0: 10 triggers since the clear");
        // the normal mode: each frame its own trigger, 0x10b0 as written
        let normal = Settings { mode: acq::Mode::Normal, ..avg.clone() };
        let (cfg, pn) = st.configure(&normal, None, None).unwrap();
        run(&cfg, &mut sides);
        assert_eq!(check(&mut st, (&normal, &pn), &mut Vec::new(), 1, &mut sides), 1, "0x10b0 outside the average");
    }

    // Break it catches: a generator block that takes the writes of another bank, a frequency that is not the phase
    // increment at 156.25 MHz, DAC codes not read as 0xffff - code, lines that are not kept, or a retune that uploads
    // the table again
    #[test]
    fn the_generator_keeps_what_its_block_and_lines_were_told() {
        use crate::dho::afg::{self, GenLines, Mod, ModShape, Wave};
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (_, p) = configured(&s);
        let (u, mut bar, _, mut clock) = unit(scene(&p, None, None));
        let mut lines = Gen(u.clone());
        let g0 = u.borrow().generator();
        assert!(!g0.output && !g0.power && g0.samples == 0 && g0.relays == 0, "nothing told yet: {:?}", g0);
        // 1 kHz, 1 Vpp, 0 V without the calibration file
        afg::run(&mut bar, &mut lines, &mut clock, &afg::start(None, Wave::Sine, 1e9, 1e9, 0.0, 0));
        let g = u.borrow().generator();
        assert!(g.output && g.power && g.modulation == 0, "{:?}", g);
        assert!((g.hz - 1e3).abs() < 1e-6, "{} Hz", g.hz);
        // 1 Vpp at 0 V: gear 7 (0x10, afg.md 4.2), the codes within one step of their levels (afg.md 5.2)
        assert_eq!((g.samples, g.relays), (8192, 0x10));
        let step = 5.29 / 65535.0;
        assert!((g.vpp - 1.0).abs() <= step * 1.158 && (g.offset_v).abs() <= step / 1.1, "{:?}", g);
        // 2 kHz without a new table (`afg::retune`), the output on all along
        let quick = afg::retune(None, Wave::Sine, 1e9, 2e9, 1e9, 0.0, lines.relay_code()).expect("a retune");
        afg::run(&mut bar, &mut lines, &mut clock, &quick);
        let r = u.borrow().generator();
        assert!(r.output && (r.hz - 2e3).abs() < 1e-6 && r.samples == 8192, "{:?}", r);
        // 4 Vpp at +0.5 V: gear 0 (0x15, x10 stage), its offset path
        let high = afg::retune(None, Wave::Sine, 2e9, 2e9, 4e9, 0.5e9, lines.relay_code());
        assert!(high.is_none(), "another gear: no retune");
        afg::run(&mut bar, &mut lines, &mut clock, &afg::start(None, Wave::Sine, 2e9, 4e9, 0.5e9, 0x10));
        let h = u.borrow().generator();
        assert_eq!(h.relays, 0x15);
        assert!((h.vpp - 4.0).abs() <= step * 1.158 * 3.17 && (h.offset_v - 0.5).abs() <= step / 0.11, "{:?}", h);
        // AM: the modulation's bank, not the DDS's 0x3000
        let am = Mod::Am { rate: 100.0, depth: 0.5, shape: ModShape::Sine };
        afg::run(&mut bar, &mut lines, &mut clock, &afg::start_mod(None, Wave::Sine, 1e9, 1e9, 0.0, 0, am));
        let m = u.borrow().generator();
        assert!(m.output && m.modulation == 0x82, "{:?}", m);
        afg::run(&mut bar, &mut lines, &mut clock, &afg::stop());
        let o = u.borrow().generator();
        assert!(!o.output && !o.power, "{:?}", o);
        let traced = u.borrow().trace.iter().filter(|e| matches!(e, Event::GenLines { .. })).count();
        assert!(traced >= 3, "the lines in the trace: {}", traced);
    }

    // Break it catches: a counter that counts other edges than the scene's triggers, results of a gate not yet whole or
    // of another gate than the last, a total not counted from the clear, a counter that runs on after its stop
    #[test]
    fn the_counter_counts_the_triggers_over_its_gates() {
        use crate::dho::counter::{self, Config, Mode};
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (_, p) = configured(&s);
        // a trigger every 1 ms from 1 us; gates of 0.25 s (3 digits)
        let (_, mut bar, _, mut clock) = unit(scene(&p, Some(1000 * US), None));
        bar.apply(&counter::start(&Config { ch: 0, mode: Mode::Frequency, digits: 3 }));
        clock.sleep_us(200_000);
        assert_eq!(counter::period(&counter::read(&mut bar), 0.25), None, "no whole gate yet");
        clock.sleep_us(160_000);
        let r = counter::read(&mut bar);
        assert_eq!((r.edges, r.freq_valid, r.period_valid), (250, true, true), "{:?}", r);
        let t = counter::period(&r, 0.25).unwrap();
        assert!((t - 1e-3).abs() < 1e-12, "{} s", t);
        assert_eq!(r.period, 1_250_000, "the single period in 0.8 ns ticks");
        assert!((359..=361).contains(&r.total), "edges since the start: {}", r.total);
        // totalize from a clear; then stopped
        bar.apply(&counter::start(&Config { ch: 0, mode: Mode::Totalize, digits: 3 }));
        clock.sleep_us(300_000);
        assert!((299..=301).contains(&counter::read(&mut bar).total));
        bar.apply(&counter::stop());
        clock.sleep_us(300_000);
        assert_eq!(counter::read(&mut bar), counter::Raw::default(), "stopped: nothing latched");
    }

    // Break it catches: a DVM block that sums another lane or channel than the one written, results of a window not yet
    // whole, sums not on the stored codes' scale (the volts of `dvm::volts` would be off), a block that runs on after
    // its stop
    #[test]
    fn the_dvm_block_sums_the_codes_of_its_lane_over_its_windows() {
        use crate::dho::dvm::{self, Mode};
        use crate::dho::levels::Vert;
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (_, p) = configured(&s);
        let (_, mut bar, _, mut clock) = unit(scene(&p, None, None));
        let vert = |c: usize| Vert { scale: CAL[c].0, offset: CAL[c].1, gain: 1.0 };
        let rms = |c: usize| 0.3 * (c as f64 + 1.0) / 2f64.sqrt();
        // CH2 of CH1 + CH2 is lane 2; windows of 2^20 samples, 6.7 ms
        bar.wr(0x4008, 0b0011);
        bar.apply(&dvm::start(2, 20));
        clock.sleep_us(5_000);
        assert_eq!(dvm::decode(&dvm::read(&mut bar), 20), None, "no whole window yet");
        clock.sleep_us(5_000);
        let r = dvm::decode(&dvm::read(&mut bar), 20).expect("a whole window");
        let (dc, ac) = (dvm::volts(&r, Mode::Dc, &vert(1)), dvm::volts(&r, Mode::AcRms, &vert(1)));
        assert!(dc.abs() < 2e-3 && (ac - rms(1)).abs() < rms(1) * 1e-2, "CH2: DC {} V, AC {} V", dc, ac);
        // CH1 alone on lane 3: with one channel every lane carries it
        bar.wr(0x4008, 0b0001);
        bar.apply(&dvm::start(3, 20));
        clock.sleep_us(7_000);
        let r = dvm::decode(&dvm::read(&mut bar), 20).expect("a whole window");
        let ac = dvm::volts(&r, Mode::AcRms, &vert(0));
        assert!((ac - rms(0)).abs() < rms(0) * 1e-2, "CH1: AC {} V", ac);
        bar.apply(&dvm::stop());
        clock.sleep_us(20_000);
        let r = dvm::read(&mut bar);
        assert_eq!((r.sum(), r.sq()), (0, 0), "stopped: nothing summed");
    }

    // Break it catches: a mask test that compares another channel, column or scale than the table written, counters not
    // latched or not cleared, a zone trigger that takes a trigger whose frame does not qualify (or of the other mode),
    // or none that does
    #[test]
    fn the_mask_unit_tests_and_the_zone_trigger_qualifies_the_frames_recorded() {
        use crate::dho::seq::{HwMask, HwZone};
        use crate::dho::{mask, zone};
        let mut base = Settings { tdiv: 100e-9, depth: 1000, frames: 10, window: None, ..Settings::default() };
        // the tables at the scale and offset the scene's view has
        (base.ch[0].scale, base.ch[0].offset) = CAL[0];
        // CH1's sine (0.3 V, 1 MHz) at each column's time over the screen, `shift` s later, +-0.05 V
        let band = |shift: f64, clear: u64| {
            let at = |j: usize| sine(0, base.delay + (j as f64 + 0.5 - 500.0) * base.tdiv / 100.0 - shift) as f32;
            let lo = (0..mask::COLUMNS).map(|j| at(j) - 0.05).collect();
            let hi = (0..mask::COLUMNS).map(|j| at(j) + 0.05).collect();
            HwMask { ch: 0, lo, hi, stop_on_fail: false, clear, map: mask::Map::default() }
        };
        // a box over the sine's crest (0.25 us, 0.3 V) or above it, the trace to touch it or not
        let zone = |v: (f64, f64), inside: bool| HwZone {
            ch: 0,
            zones: vec![zone::Zone { poly: vec![(0.2e-6, v.0), (0.3e-6, v.1)], inside }],
            clear: 0,
            map: mask::Map::default(),
        };
        let mut st = State::handover(base.mask, None);
        let p = st.configure(&base, None, None).unwrap().1;
        // a trigger every 20 us from 1 us: at the sine's rising zero crossing
        let (_, bar, c2h, clock) = unit(scene(&p, Some(20 * US), None));
        let mut sides = (bar, c2h, clock);
        let batch = |st: &mut State, s: &Settings, sides: &mut (Bar, Stream, Time)| {
            let (bar, c2h, clock) = sides;
            let (cfg, p) = st.configure(s, None, None).unwrap();
            seq::execute_bus_on(&cfg, bar, c2h, &mut DmaBuf::new(), clock, &mut |_| {}, &|| false).unwrap();
            let mut dma = DmaBuf::new();
            let got =
                seq::acquire_batch_on(st, s, &p, bar, c2h, &mut dma, clock, false, 12, |_| {}, || false, || false);
            let ctrl = st.mask_ctrl().or(st.zone_ctrl()).unwrap();
            (got, mask::read(bar, ctrl))
        };
        let (got, r) = batch(&mut st, &Settings { hwmask: Some(band(0.0, 0)), ..base.clone() }, &mut sides);
        assert_eq!((got, r.frames(), r.fails()), (Ok(10), 10, 0), "the band around the signal");
        let (got, r) = batch(&mut st, &Settings { hwmask: Some(band(0.25e-6, 1)), ..base.clone() }, &mut sides);
        assert_eq!((got, r.frames(), r.fails()), (Ok(10), 10, 10), "the band a quarter period late, cleared");
        let (got, r) = batch(&mut st, &Settings { hwzone: Some(zone((0.25, 0.35), true)), ..base.clone() }, &mut sides);
        assert_eq!((got, r.frames(), r.fails()), (Ok(10), 10, 0), "the crest touched");
        let (got, r) = batch(&mut st, &Settings { hwzone: Some(zone((0.35, 0.5), false)), ..base.clone() }, &mut sides);
        assert_eq!((got, r.frames()), (Ok(10), 20), "the box above the crest not touched");
        let (got, _) = batch(&mut st, &Settings { hwzone: Some(zone((0.35, 0.5), true)), ..base.clone() }, &mut sides);
        assert!(got.is_err_and(|e| e.contains("no acquisition")), "the box above the crest is never touched");
    }

    // Break it catches: LA words of another frame, start, spacing or length than the record's (la.md 3.3, 4.2), an
    // analog frame played into the LA read, or bytes left in the stream for the next batch
    #[test]
    fn the_la_read_of_a_record_is_the_scenes_lines_from_its_start() {
        // one, two and four channels: m = 1, 2 and 4 LA samples per two analog samples (la.md 3.1)
        for mask in [0b0001u32, 0b0011, 0b1111] {
            let m = crate::dho::la::sample_mode(mask) as i128;
            let s = Settings { tdiv: 100e-9, depth: 1000, frames: 3, window: None, mask, ..Settings::default() };
            let (mut st, p) = configured(&s);
            let view = View { cal: CAL, dot_fs: p.dot_fs as i128, pre: p.pre, post: p.post };
            // the lines count steps of 10 ns
            let la = |t: f64| (t / 1e-8) as u64 as u16;
            let period_fs = Some(20 * US);
            let scene =
                Scene { volts: Box::new(sine), la: Some(Box::new(la)), first_fs: US, period_fs, auto_fs: None, view };
            let (u, mut bar, mut c2h, mut clock) = unit(scene);
            let mut dma = DmaBuf::new();
            let mut batch = |st: &mut State, bar: &mut Bar, c2h: &mut Stream| {
                seq::acquire_batch_on(st, &s, &p, bar, c2h, &mut dma, &mut clock, false, 12, |_| {}, || false, || false)
            };
            assert_eq!(batch(&mut st, &mut bar, &mut c2h), Ok(3), "mask {:#x}", mask);
            let w = seq::acquire_la(&mut st, &s, &p, &mut bar, &mut c2h, |_| {}).expect("the LA read");
            let at = u.borrow().trigger_fs(2).expect("the newest frame");
            let (pre, dot) = (p.pre as i128 * m / 2, 2 * p.dot_fs as i128 / m);
            assert_eq!(w.len() as i128, p.memsel as i128 * m / 2, "mask {:#x}: n_la = memsel x m / 2 words", mask);
            for (k, &x) in w.iter().enumerate() {
                let want = la((at + (k as i128 - pre) * dot) as f64 * 1e-15);
                assert_eq!(x, want, "mask {:#x}: word {}", mask, k);
            }
            assert!(u.borrow().stream.is_empty(), "mask {:#x}: bytes left after the LA read", mask);
            assert_eq!(batch(&mut st, &mut bar, &mut c2h), Ok(3), "mask {:#x}: the next batch", mask);
        }
    }

    // Break it catches: a measure unit that shows a slot another channel than `levels::slots` gives it, values of
    // another scale (12-bit extremes, 16-bit mean) or offset, a run done before its time, values before a run
    #[test]
    fn the_measure_unit_shows_each_slots_channel_over_its_run() {
        use crate::dho::levels::{self, Vert};
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (_, p) = configured(&s);
        let (_, mut bar, _, mut clock) = unit(scene(&p, None, None));
        bar.wr(0x4008, 0b0011);
        assert_eq!(bar.rd(0x1044), 0xeeee_eeee, "no run yet");
        bar.wr(0x1040, 0x8000_0000 | levels::RUN_EXP);
        assert_eq!(bar.rd(0x1040) & 1 << 30, 0, "a run of 1.68 ms is not over at once");
        let runs = [levels::run(&mut bar, &mut |us| clock.sleep_us(us as u64)).expect("a run")];
        let vert = std::array::from_fn(|c| Vert { scale: CAL[c].0, offset: CAL[c].1, gain: 1.0 });
        let lv = levels::channel_levels(&runs, 0b0011, &vert);
        // CH1 0.3 V, CH2 0.6 V around 0 V; CH3 and CH4 not acquired
        for (c, a) in [(0, 0.3), (1, 0.6)] {
            let l = lv[c].unwrap_or_else(|| panic!("CH{} levels", c + 1));
            let step = 16.0 * CAL[c].0 / 7500.0;
            assert!((l.hi - a).abs() < a * 1e-2 + step && (l.lo + a).abs() < a * 1e-2 + step, "CH{}: {:?}", c + 1, l);
            assert!(l.mean.abs() < a * 1e-2 && !l.clipped, "CH{}: {:?}", c + 1, l);
        }
        assert!(lv[2].is_none() && lv[3].is_none());
    }

    /// The core trim's side of the simulated unit (as `hw::trim::UnitTrim` of oos-ui): measure runs and offset words.
    struct Trim<'a>(&'a mut Bar, &'a mut Time, crate::dho::adc::Adc);

    impl crate::dho::coretrim::TrimHw for Trim<'_> {
        fn slots(&mut self) -> Option<[crate::dho::levels::Slot; 8]> {
            let clock = &mut *self.1;
            crate::dho::levels::run(self.0, &mut |us| clock.sleep_us(us as u64))
        }
        fn wait_ms(&mut self, ms: u32) {
            self.1.sleep_us(ms as u64 * 1000);
        }
        fn set_offset(&mut self, core: usize, o: i32) {
            for w in self.2.core_offset(core, o) {
                self.0.wr(w.reg, w.val);
            }
        }
    }

    // Break it catches: an offset word not taken for its core (another register, the sign of 11 bits lost), a core
    // shown in another slot than core-offset.md's or moved by another step, cores that differ without `cores`
    #[test]
    fn the_measure_unit_shows_each_cores_offset_and_the_core_trim_matches_them() {
        use crate::dho::coretrim::{self, TrimHw};
        let s = Settings { tdiv: 10e-9, frames: 1, window: None, ..Settings::default() };
        let (_, p) = configured(&s);
        let quiet = Scene { volts: Box::new(|_, _| 0.0), ..scene(&p, None, None) };
        let (u, mut bar, _, mut clock) = unit(quiet);
        bar.wr(0x4008, 1);
        let own = [-30, 31, 32, 33, 34, 35, 36, 37];
        // stored offsets: core 5 three steps high, core 7 two low, core 0 below zero (the sign of the 11 bits)
        let stored = [-30, 31, 32, 33, 34, 38, 36, 35];
        let mut hw = Trim(&mut bar, &mut clock, crate::dho::adc::Adc::default());
        for (n, &o) in stored.iter().enumerate() {
            hw.set_offset(n, o);
        }
        assert_eq!(u.borrow().core_offsets(), stored);
        let alike = hw.slots().expect("a run");
        assert!(alike.iter().all(|k| *k == alike[0]), "without the cores' own offsets every slot alike: {:?}", alike);
        u.borrow_mut().cores = Some(own);
        let m = hw.slots().expect("a run");
        let mid = m[0].mean;
        let want = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 27.0, -18.0];
        for (k, w) in want.iter().enumerate() {
            assert_eq!(m[k].mean - mid, *w, "slot {}: {:?}", k, m[k]);
        }
        let (groups, lsb) = coretrim::plan(1, &[1.0; 4]);
        let g = coretrim::gate(&mut hw, &groups, &lsb).expect("the gate");
        assert_eq!((g.matched, g.skipped), (1, 0), "a quiet channel at mid-scale");
        let order = coretrim::order(&mut hw, &stored).expect("the order");
        assert_eq!((order.slot, order.codes), (CORE_SLOT, [CORE_STEP_CODES as f64; 8]));
        let t = coretrim::trim_gated(&mut hw, &groups, &stored, &order, &lsb, &g).expect("the trim");
        assert_eq!(t.offsets, own, "every core at its own offset: {:?}", t);
        assert_eq!(u.borrow().core_offsets(), own);
    }

    // Break it catches: stamps other than the head of `seg` states (the latch holds the trigger of the frame played
    // last, a frame's header that of the frame before it, the first-trigger stamp that of the first frame since its
    // clear), or a segmented record that does not record on the clock
    #[test]
    fn a_segmented_record_runs_on_the_unit_with_its_stamps() {
        use crate::dho::seg;
        let segments = Some(seq::Segments { n: 20, interval: 0.0 });
        let s = Settings { tdiv: 100e-9, depth: 1000, window: None, segments, ..Settings::default() };
        let (mut st, p) = configured(&s);
        let ms = 1000 * US;
        let (u, mut bar, mut c2h, mut clock) = unit(scene(&p, Some(ms), None));
        let n = seg::start(&mut st, &s, &p, &mut bar, &mut c2h, &mut clock).unwrap();
        assert_eq!(n, 20);
        clock.sleep_us(5_500);
        let x = seg::status(&mut bar, n);
        assert!(x.recorded > 0 && !x.complete(n), "a part after 5.5 ms: {}", x.recorded);
        clock.sleep_us(20_000);
        assert!(seg::status(&mut bar, n).complete(n), "all 20 after 25.5 ms");
        let first = *u.borrow().ring.values().next().unwrap();
        let tag = seg::first_trigger(&mut bar);
        assert_eq!(tag, Some((first / LATCH_TICK_FS) as u64), "the first trigger since the clear");
        seg::stop(&mut st, &mut bar, &mut c2h, &mut clock).unwrap();
        let sts = seg::stamps(&mut st, &s, &p, &mut bar, &mut c2h, &mut clock, 0, n).unwrap();
        let mut tl = seg::Timeline::new(tag);
        for (k, x) in sts.iter().enumerate() {
            let t = tl.push(*x).unwrap();
            assert!((t - k as f64 * 1e-3).abs() < 1e-12, "segment {}: {} s", k, t);
            if k > 0 {
                assert_eq!(x.header, sts[k - 1].latch, "segment {}: the header holds the trigger before it", k);
            }
        }
        let rs = seg::read_settings(&s, n);
        let mut dma = DmaBuf::new();
        seg::read(&mut st, &rs, &p, &mut bar, &mut c2h, &mut dma, &mut clock, 0, n).unwrap();
        assert_eq!(dma.data().len(), n as usize * seq::frame_bytes(&rs, &p));
    }
}
