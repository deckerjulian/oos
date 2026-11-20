// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The host run of the acquisition loop (ADR 0053, docs/board-on-a-host.md 6, stage 2): `run` against the simulated
//! unit for a matrix of settings, as the screen would set them. Each case checks what reached the screen's side against
//! its scene. With `OOS_LIVE_OUT` set, it writes one file per case: the unit's trace (every write, read, stream read
//! and frame, on the virtual clock) and a line per frame delivered (its fields and the extremes and mean of each
//! channel). `tools/compare-base.sh` runs it on both sides and compares the files, so a change of the loop that changes
//! what it does to the unit, or what it delivers, shows in the comparison. Tests only: the scope's programs do not have
//! it.

use super::port::{Hw, Port};
use crate::board::{
    lock, CounterRead, CounterWant, DvmRead, DvmWant, Frame, HwShared, MaskRead, MaskWant, Shared, ZoneRead, ZoneWant,
};
use crate::model::*;
use oos_hal::dho::afg::{Mod, ModShape, Wave};
use oos_hal::dho::sim::{unit, GenState, Scene, Unit, View};
use std::cell::RefCell;
use std::rc::Rc;

/// The simulated unit as the loop's port: its register window, stream and clock, a front-end stub, lines that record,
/// and the generator's lines of the unit it holds (None: they cannot be opened).
pub(super) struct SimPort(pub(super) Option<Rc<RefCell<Unit>>>);

impl Port for SimPort {
    type Bus = oos_hal::dho::sim::Bar;
    type Stream = oos_hal::dho::sim::Stream;
    type Afe = oos_hal::dho::boot::sim::AfeStub;
    type Lines = oos_hal::dho::lines::Recorded;
    type Clock = oos_hal::dho::sim::Time;
    type Gen = oos_hal::dho::sim::Gen;
    fn open_gen(&mut self) -> std::io::Result<Self::Gen> {
        let u =
            self.0.clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no generator lines"))?;
        Ok(oos_hal::dho::sim::Gen(u))
    }
}

/// Femtoseconds of a microsecond.
const US: i128 = 1_000_000_000;

/// A box of the zone trigger: from (s, V) to (s, V), and whether the trace must touch it.
type ZoneBox = ((f64, f64), (f64, f64), bool);

/// What the screen does at a pass of the loop.
enum Act {
    /// new settings: the model changed as the function says
    Set(fn(&mut Model)),
    /// new settings that change what the front ends make of the scene: the model, and the scene's view of it
    SetView(fn(&mut Model)),
    /// CLEAR of the average (`Shared::avg_clear`)
    AvgClear,
    /// a request of the generator (`Shared::afg`): on, Hz, Vpp, V offset, wave, modulation
    Gen(crate::board::Gen),
    /// a request of the frequency counter (`Shared::counter_want`), None to stop it
    Counter(Option<CounterWant>),
    /// a request of the DVM block (`Shared::dvm_want`), None to stop it
    Dvm(Option<DvmWant>),
    /// a request of the mask unit on CH1 (`Shared::mask_want`): a band of `MASK_TOL` around the scene's signal shifted
    /// by this many seconds, the clear number; None to stop it
    Mask(Option<(f64, u64)>),
    /// a request of the zone trigger on CH1 (`Shared::zone_want`): a box from (s, V) to (s, V) the trace must touch
    /// (true) or must not; None to stop it
    Zone(Option<ZoneBox>),
    /// the auto set-up's levels from the measure unit (`Shared::levels_want`): request 1, this many runs
    Levels(u32),
    /// one raw run of the measure unit for `:DEBug:SLOTs?` (`Shared::slots_want`): request 1
    Slots,
    /// STOP
    Hold,
    /// the next SINGLE press, once the scan sweep of the last has halted (at this pass or the first after it)
    Rearm,
    /// hardware segments: RUN starts a recording (`HwSeg::run`), STOP ends it early, a segment asked to be shown
    SegRun,
    SegStop,
    SegShow(u32),
    /// STOP that wants the newest frame read raw (`Shared::final_req`)
    FinalRaw,
    /// in STOP: the view the screen shows (time per division, delay), read again from the stored record
    StopView(f64, f64),
    /// the force-trigger key
    Force,
    /// the measurements want the screen window of a deep record raw, for every batch (`Shared::deep_raw`)
    DeepRaw,
    /// the loop busy this many microseconds before the pass (its work on the batch it read, such as decoding): the
    /// unit's clock runs on, and a batch armed ahead records
    Busy(u64),
}

/// A switch of system.conf that the board reads itself and a case turns on for the test's thread.
#[derive(Clone, Copy, PartialEq)]
enum Switch {
    /// arming ahead, `acq.prearm = on` (`plan::prearm_on`)
    Prearm,
    /// the core trim, `cal.core_offset = tight` (`trim::core_offset_tight`)
    CoreTrim,
}

/// One case of the matrix: the model as the screen sets it, the frequency of the scene's sine, the trigger period of
/// the scene (None: it never triggers; roll and scan do not wait for it), the passes of the loop, and what the screen
/// does at which pass.
#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    model: fn(&mut Model),
    freq_hz: f64,
    /// a square of the sine's sign instead of the sine (an edge a slow time base sees as steep)
    square: bool,
    period_us: Option<i128>,
    /// the logic analyser on, its lines a count of `LA_TICK_S` steps
    la: bool,
    /// the loop starts with the made-up calibration set in its folder (else with none)
    cal: bool,
    /// the live compression on (`policy::compress::enabled`, a switch of the unit that is off by default)
    compress: bool,
    /// the switches of system.conf on for this case (all others off, as by default)
    switches: &'static [Switch],
    passes: u32,
    script: &'static [(u32, Act)],
}

/// What a case has unless it says otherwise.
const BASE: Case = Case {
    name: "",
    model: |_| {},
    freq_hz: 1e6,
    square: false,
    period_us: Some(50),
    la: false,
    cal: false,
    compress: false,
    switches: &[],
    passes: 30,
    script: &[],
};

/// The memory depth of the menu named `name`.
fn depth(m: &mut Model, name: &str) {
    m.mdepth = mdepths().iter().position(|d| d.0 == name).expect("a depth of the menu");
}

fn tdiv(m: &mut Model, t: f32) {
    m.tscale = tscales().iter().position(|&x| (x - t).abs() <= t * 1e-3).expect("a time base of the list");
}

/// The channels `on` (0 = CH1), the trigger on CH1's edge at 0 V, where the scene's signal crosses it.
fn ch(m: &mut Model, on: &[usize]) {
    for (c, x) in m.ch.iter_mut().enumerate() {
        x.on = on.contains(&c);
    }
    m.trig_level = 0.0;
}

const CASES: &[Case] = &[
    Case {
        name: "auto-ch1-1us",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 30,
        script: &[],
    },
    Case {
        name: "auto-ch12-1us",
        model: |m| (ch(m, &[0, 1]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 30,
        script: &[],
    },
    Case {
        name: "auto-ch1234-1us",
        model: |m| (ch(m, &[0, 1, 2, 3]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 30,
        script: &[],
    },
    Case {
        name: "auto-ch1-1ms",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-3)).0,
        freq_hz: 1e3,
        square: true,
        period_us: Some(1_000),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 20,
        script: &[],
    },
    Case {
        name: "normal-no-trigger",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Normal).0,
        freq_hz: 1e6,
        square: false,
        period_us: None,
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 6,
        script: &[],
    },
    Case {
        name: "normal-new-settings-during-the-wait",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Normal).0,
        freq_hz: 1e6,
        square: false,
        period_us: None,
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 8,
        script: &[(3, Act::Set(|m| tdiv(m, 2e-6)))],
    },
    Case {
        name: "single-ch1",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Single).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 20,
        script: &[],
    },
    Case {
        name: "auto-then-stop",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 30,
        script: &[(12, Act::Hold)],
    },
    Case {
        name: "roll-ch1-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = true).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 40,
        script: &[],
    },
    Case {
        name: "roll-ch1234-200ms",
        model: |m| (ch(m, &[0, 1, 2, 3]), tdiv(m, 0.2), m.auto_roll = true).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 40,
        script: &[],
    },
    Case {
        name: "scan-run-ch1-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = false).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: SCAN_PASSES,
        script: &[],
    },
    Case {
        name: "scan-single-ch1-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = false, m.trig_mode = TrigMode::Single).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: SCAN_PASSES,
        script: &[(SCAN_PASSES / 2, Act::Rearm)],
    },
    Case {
        name: "scan-run-ch1-200ms-delayed",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = false, m.delay = 1.5).0,
        freq_hz: 1e3,
        period_us: None,
        passes: 60,
        ..BASE
    },
    Case {
        name: "roll-ch1-la-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = true, m.la.on = true).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        la: true,
        cal: false,
        compress: false,
        switches: &[],
        passes: 40,
        script: &[],
    },
    Case {
        name: "scan-run-ch1-la-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = false, m.la.on = true).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        la: true,
        cal: false,
        compress: false,
        switches: &[],
        passes: SCAN_PASSES,
        script: &[],
    },
    Case {
        name: "segments-20-then-show-one",
        model: |m| segments(m, 20),
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        la: false,
        cal: false,
        compress: false,
        switches: &[],
        passes: 60,
        script: &[(1, Act::SegRun), (40, Act::SegShow(7))],
    },
    Case {
        name: "segments-10-1ms-apart",
        model: |m| (segments(m, 10), m.seg_ivl = 1e-3).0,
        passes: 60,
        script: &[(1, Act::SegRun)],
        ..BASE
    },
    Case {
        name: "segments-stopped-early",
        model: |m| segments(m, 1000),
        period_us: Some(1_000),
        script: &[(1, Act::SegRun), (5, Act::SegStop)],
        ..BASE
    },
    Case {
        name: "segments-aborted-by-new-settings",
        model: |m| segments(m, 1000),
        period_us: Some(1_000),
        passes: 12,
        script: &[(1, Act::SegRun), (5, Act::Set(|m| tdiv(m, 2e-6)))],
        ..BASE
    },
    Case { name: "auto-ch1-1us-calibrated", model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0, cal: true, ..BASE },
    Case {
        name: "auto-ch1-200us-compressed-then-stop-raw",
        compress: true,
        model: |m| (ch(m, &[0]), tdiv(m, 2e-4)).0,
        freq_hz: 1e3,
        square: true,
        period_us: Some(1_000),
        passes: 20,
        script: &[(14, Act::Hold), (14, Act::FinalRaw)],
        ..BASE
    },
    Case {
        name: "auto-then-stop-two-views",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        script: &[(12, Act::Hold), (14, Act::StopView(1e-6, 0.0)), (17, Act::StopView(5e-7, 1e-6))],
        ..BASE
    },
    Case {
        name: "prearm-ch1-1us-new-settings-then-stop",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        switches: &[Switch::Prearm],
        passes: 22,
        // the loop busy before the new settings and the STOP, so the batch armed ahead is complete when the disarm
        // polls: its poll waits on the wall clock (`seq::disarm`), and then its count of reads depends on the machine
        script: &[
            (8, Act::Busy(10_000)),
            (8, Act::SetView(|m| tdiv(m, 2e-6))),
            (16, Act::Busy(10_000)),
            (16, Act::Hold),
            (18, Act::StopView(1e-6, 0.0)),
        ],
        ..BASE
    },
    Case {
        name: "core-trim-ch1-quiet",
        // a quiet input at mid-scale
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Normal, m.ch[0].offset = 0.0).0,
        freq_hz: 0.0,
        period_us: None,
        cal: true,
        switches: &[Switch::CoreTrim],
        passes: 4,
        ..BASE
    },
    Case {
        name: "auto-ch1-deep-10m-screen-window",
        compress: true,
        model: |m| (ch(m, &[0]), tdiv(m, 5e-3), depth(m, "10M")).0,
        freq_hz: 1e3,
        square: true,
        period_us: Some(1_000),
        passes: 6,
        script: &[(1, Act::DeepRaw)],
        ..BASE
    },
    Case {
        name: "normal-no-trigger-force",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Normal).0,
        period_us: None,
        passes: 8,
        script: &[(3, Act::Force)],
        ..BASE
    },
    Case { name: "average-hw-16-then-clear", model: |m| average(m, true, 16), script: &[(15, Act::AvgClear)], ..BASE },
    Case {
        name: "average-hw-64-cleared-by-new-settings",
        model: |m| average(m, true, 64),
        period_us: Some(1_000),
        passes: 20,
        script: &[(10, Act::SetView(|m| m.ch[0].vscale -= 1))],
        ..BASE
    },
    Case { name: "average-sw-16", model: |m| average(m, false, 16), ..BASE },
    Case {
        name: "generator-on-retuned-off",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        passes: 20,
        script: &[
            (2, Act::Gen((true, 1e3, 1.0, 0.0, Wave::Sine, Mod::None))),
            (8, Act::Gen((true, 2e3, 1.0, 0.0, Wave::Sine, Mod::None))),
            (14, Act::Gen((false, 2e3, 1.0, 0.0, Wave::Sine, Mod::None))),
        ],
        ..BASE
    },
    Case {
        name: "generator-square-then-am",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        passes: 12,
        script: &[
            (2, Act::Gen((true, 1e4, 2.0, 0.5, Wave::Square { duty: 0.5 }, Mod::None))),
            (
                6,
                Act::Gen((true, 1e4, 2.0, 0.5, Wave::Sine, Mod::Am { rate: 100.0, depth: 0.5, shape: ModShape::Sine })),
            ),
        ],
        ..BASE
    },
    Case {
        name: "counter-ch1-frequency-then-totalize",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-3), depth(m, "1k")).0,
        freq_hz: 1e3,
        square: true,
        period_us: Some(1_000),
        passes: 70,
        script: &[
            (2, Act::Counter(Some(CounterWant { ch: 0, mode: 0, digits: 3, clear: 0 }))),
            (25, Act::Counter(Some(CounterWant { ch: 0, mode: 2, digits: 3, clear: 0 }))),
            (55, Act::Counter(None)),
        ],
        ..BASE
    },
    Case {
        name: "mask-ch1-pass-then-fail-then-off",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-3), depth(m, "1k")).0,
        freq_hz: 1e3,
        period_us: Some(1_000),
        passes: UNIT_PASSES,
        script: &[(2, Act::Mask(Some((0.0, 0)))), (12, Act::Mask(Some((2.5e-4, 1)))), (22, Act::Mask(None))],
        ..BASE
    },
    Case {
        name: "zone-ch1-touched-untouched-then-stalled",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-3), depth(m, "1k")).0,
        freq_hz: 1e3,
        period_us: Some(1_000),
        passes: UNIT_PASSES,
        script: &[
            // the sine's crest at 0.25 ms; nothing above 0.5 V
            (2, Act::Zone(Some(((2e-4, 0.4), (3e-4, 0.6), true)))),
            (12, Act::Zone(Some(((2e-4, 0.6), (3e-4, 0.7), false)))),
            (22, Act::Zone(Some(((2e-4, 0.6), (3e-4, 0.7), true)))),
        ],
        ..BASE
    },
    Case {
        name: "la-record-ch1-1ms",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-3), depth(m, "1k"), m.la.on = true).0,
        freq_hz: 1e3,
        period_us: Some(1_000),
        la: true,
        passes: 20,
        ..BASE
    },
    Case {
        name: "levels-and-slots-ch12-1us",
        model: |m| (ch(m, &[0, 1]), tdiv(m, 1e-6)).0,
        passes: 10,
        script: &[(2, Act::Levels(2)), (6, Act::Slots)],
        ..BASE
    },
    Case {
        name: "dvm-ch2-then-ch1-then-off",
        model: |m| (ch(m, &[0, 1]), tdiv(m, 1e-3), depth(m, "1k"), m.ch[1].offset = DVM_OFFSET).0,
        freq_hz: 1e3,
        period_us: Some(1_000),
        passes: DVM_PASSES,
        script: &[
            (2, Act::Dvm(Some(DvmWant { ch: 1, lane: None }))),
            (DVM_PASSES / 2, Act::Dvm(Some(DvmWant { ch: 0, lane: None }))),
            (DVM_PASSES - 10, Act::Dvm(None)),
        ],
        ..BASE
    },
];

/// Passes of the mask and zone cases: three requests of ten passes each, the last one stalled twice and dropped.
const UNIT_PASSES: u32 = 40;

/// Passes of the DVM case: two requests of at least three readings each (a window of 2^28 samples is 1.7 s on the
/// virtual clock, read every 0.5 s), then the stop.
const DVM_PASSES: u32 = 240;

/// Readings of the DVM block after a start before its first whole window: 2^28 samples at 156.25 MSa/s take 1.7 s, it
/// is read every 0.5 s (and a pass of the case's loop).
const DVM_EARLY: usize = 3;

/// The vertical offset of CH2 in the DVM case: its codes are not those of CH1, and the volts take it out again.
const DVM_OFFSET: f32 = 0.3;

/// What the screen's side sees of the unit after each pass: what the generator was told and the counter's reading.
/// `seen[k]` is taken when the loop asks before pass k + 1, so it is the state after pass k, the pass that applied a
/// request of the script at k.
#[derive(Clone, Copy)]
struct Seen {
    gen: GenState,
    counter: Option<CounterRead>,
    dvm: Option<DvmRead>,
    mask: Option<MaskRead>,
    zone: Option<ZoneRead>,
    /// frames decoded so far, and the mask or zone unit dropped for the settings (`Shared::unit_blocked`)
    decoded: u64,
    blocked: bool,
    /// events in the unit's trace so far: pass k's are those from `seen[k - 1]` to `seen[k]`
    events: usize,
    /// each ADC core's offset register against the core's own offset, in register steps, where the case gives them
    /// (`Unit::cores`)
    cores: Option<[i32; 8]>,
}

/// The volts a mask band keeps around the scene's signal (`Act::Mask`).
const MASK_TOL: f32 = 0.1;

/// The mask test of `Act::Mask` for the case's model `m`: CH1, each of the `MASK_COLUMNS` columns over the screen
/// the scene's signal at its time less `shift` s, +-`MASK_TOL`.
fn mask_want(case: &Case, m: &Model, (shift, clear): (f64, u64)) -> MaskWant {
    let volts = signal(case.freq_hz, case.square);
    let w = 10.0 * m.hw_time_per_div() as f64;
    let n = crate::board::MASK_COLUMNS;
    let at = |j: usize| volts(0, m.delay as f64 - w / 2.0 + (j as f64 + 0.5) / n as f64 * w - shift) as f32;
    let lo = (0..n).map(|j| at(j) - MASK_TOL).collect();
    let hi = (0..n).map(|j| at(j) + MASK_TOL).collect();
    MaskWant { ch: 0, lo, hi, stop: false, clear }
}

/// Where a 0x4020 bit was last written: set or not, None when 0x4020 was not written.
fn last_4020_bit(trace: &str, bit: u32) -> Option<bool> {
    let v = trace.lines().rfind(|l| l.starts_with("Write") && l.contains("off: 16416,"))?;
    v.rsplit("v: ").next()?.trim_end_matches(" }").parse::<u32>().ok().map(|v| v & bit != 0)
}

/// The passes from each request of `pick` in the script to the next one: (from, until, the request).
fn spans<T: Copy>(case: &Case, seen: &[Seen], pick: fn(&Act) -> Option<T>) -> Vec<(usize, usize, T)> {
    let asked: Vec<(usize, T)> = case.script.iter().filter_map(|(at, a)| Some((*at as usize, pick(a)?))).collect();
    asked.iter().enumerate().map(|(i, &(at, w))| (at, asked.get(i + 1).map_or(seen.len(), |n| n.0), w)).collect()
}

/// A line of the trace as an access: (true for a write, offset, value); None for other events.
fn access(line: &str) -> Option<(bool, u32, u32)> {
    let num = |key: &str| line.split(key).nth(1)?.split([',', ' ']).next()?.parse::<u32>().ok();
    let write = line.starts_with("Write");
    (write || line.starts_with("Read")).then_some(())?;
    Some((write, num("off: ")?, num("v: ")?))
}

/// An arm in the trace: the write of 0x400c that sets record-seq mode (bits 29 and 25) without play (bit 30).
fn is_arm(line: &str) -> bool {
    access(line).is_some_and(|(w, off, v)| w && off == 0x400c && v & 0x6200_0000 == 0x2200_0000)
}

/// The checks of arming ahead (`Switch::Prearm`), on the unit's trace pass by pass: every pass that reads a batch
/// while running arms the next before it ends and waits for nothing after that arm; the pass after it waits without
/// arming again. New settings and the STOP disarm first: the pass that takes them starts with the disarm (its poll of
/// 0x4034, then the stop sequence in 0x4004), and nothing is recorded after its stop (bit 31) until the next arm;
/// after the STOP nothing is recorded or played at all. The case lets the batch armed ahead complete first
/// (`Act::Busy`), so the poll finds the FSM stopped at its first read. Break it catches: no arming ahead, a batch
/// armed ahead and armed again, a configuration or a STOP without the disarm, a view in STOP read from the ring the
/// arm wrote.
fn prearm_checks(case: &Case, trace: &str, seen: &[Seen]) {
    if !case.switches.contains(&Switch::Prearm) {
        return;
    }
    let lines: Vec<&str> = trace.lines().collect();
    let pass = |k: usize| &lines[seen[k - 1].events..seen[k].events];
    let at = |f: fn(&Act) -> bool| case.script.iter().find(|(_, a)| f(a)).map(|(k, _)| *k as usize);
    let (set, hold) = (at(|a| matches!(a, Act::SetView(_))), at(|a| matches!(a, Act::Hold)));
    let hold = hold.expect("a STOP in the case");
    let poll = |l: &&str| access(l).is_some_and(|(w, off, _)| !w && off == 0x4080);
    let mut ahead = 0;
    for k in 1..hold {
        let p = pass(k);
        let Some(read) = p.iter().rposition(|l| l.starts_with("Played")) else { continue };
        let arm = p.iter().rposition(|l| is_arm(l)).filter(|&a| a > read && !p[a..].iter().any(poll));
        assert!(arm.is_some(), "{}: pass {} read a batch and did not arm the next ahead", case.name, k);
        ahead += 1;
        let next = pass(k + 1);
        if Some(k + 1) != set && k + 1 < hold {
            let wait = next.iter().position(poll).unwrap_or(next.len());
            assert!(!next[..wait].iter().any(|l| is_arm(l)), "{}: pass {} armed again", case.name, k + 1);
        }
    }
    assert!(ahead >= 10, "{}: {} batches armed ahead", case.name, ahead);
    for k in set.into_iter().chain([hold]) {
        let p = pass(k);
        let first = p.iter().position(|l| l.starts_with("Write"));
        let first = first.unwrap_or_else(|| panic!("{}: pass {} wrote nothing: no disarm", case.name, k));
        let stop = access(p[first]).is_some_and(|(_, off, _)| off == 0x4004);
        let status: Vec<&str> =
            p[..first].iter().copied().filter(|l| access(l).is_some_and(|(w, off, _)| !w && off == 0x4034)).collect();
        assert!(stop && !status.is_empty(), "{}: pass {} did not start with the disarm: {}", case.name, k, p[first]);
        // the case's `Act::Busy`: the batch armed ahead is complete, the poll ends at its first read on any machine
        let at_once = matches!(status[..], [l] if l.ends_with("times: 1 }")
            && access(l).is_some_and(|(_, _, v)| v & oos_hal::dho::scu::status::STOPPED != 0));
        assert!(at_once, "{}: the disarm of pass {} polled a running FSM: {:?}", case.name, k, status);
        let halt = |l: &&str| access(l).is_some_and(|(w, off, v)| w && off == 0x4004 && v & 1 << 31 != 0);
        let halt = p.iter().position(halt).unwrap_or_else(|| panic!("{}: pass {} did not stop the FSM", case.name, k));
        let rest = &lines[seen[k - 1].events + halt..];
        let until = if k == hold { rest.len() } else { rest.iter().position(|l| is_arm(l)).unwrap_or(rest.len()) };
        let late = rest[..until].iter().find(|l| l.starts_with("Recorded") || (k == hold && l.starts_with("Played")));
        assert!(late.is_none(), "{}: after the disarm of pass {}: {:?}", case.name, k, late);
    }
}

/// How far the stored offsets of the made-up set leave each ADC core from its own offset in the core trim's case, in
/// register steps: core 5 three high, core 7 two low. Within `coretrim::SAVE_MIN_STEPS`, so the trim keeps its result
/// for the session and writes no file.
const CORE_ERROR: [i32; 8] = [0, 0, 0, 0, 0, 3, 0, -2];

/// The cores' own offsets of the core trim's case (`Unit::cores`): the stored ones of the made-up set in `dir` for the
/// ADC mode of the model's settings, less `CORE_ERROR`; None without the set.
fn own_offsets(m: &Model, dir: &str) -> Option<[i32; 8]> {
    let want = super::settings(m, false);
    let mut st = oos_hal::dho::seq::State::handover(want.mask, None);
    st.configure(&want, None, None).ok()?;
    let stored = oos_hal::dho::adc::CalAdc::load(dir).ok()?.offsets(st.mode);
    Some(std::array::from_fn(|n| stored[n] - CORE_ERROR[n]))
}

/// The core offset written by a line of the trace, (core, offset): an ADC word (0x1034, bit 24) to the offset
/// register 0x14 + 8n (adc-mode.md 2.2, 11 bits signed).
fn core_offset_write(line: &str) -> Option<(usize, i32)> {
    let (w, off, v) = access(line)?;
    let reg = v >> 16 & 0x7f;
    (w && off == 0x1034 && v & 1 << 24 != 0 && (0x14..0x54).contains(&reg) && reg % 8 == 4)
        .then(|| ((reg as usize - 0x14) / 8, ((v & 0x7ff) << 21) as i32 >> 21))
}

/// The checks of the core trim (`Switch::CoreTrim`) on a quiet input: the trim runs the measure unit no earlier than
/// 2 s (`TRIM_HOLD`) after the start (the configuration), starts from the stored offsets (each core `CORE_ERROR` off its
/// own), and at the end every core sits at its own offset. Break it catches: a trim that does not wait for the
/// configuration to stand, offsets not written or written to other cores, a trim that does not converge or keeps
/// nothing.
fn trim_checks(case: &Case, trace: &str, seen: &[Seen]) {
    if !case.switches.contains(&Switch::CoreTrim) {
        return;
    }
    let run = |l: &&str| access(l).is_some_and(|(w, off, v)| w && off == 0x1040 && v & 1 << 31 != 0);
    let first = trace.lines().find(run).unwrap_or_else(|| panic!("{}: no measure run", case.name));
    let us: u64 = first.split("us: ").nth(1).and_then(|x| x.split(',').next()?.parse().ok()).unwrap_or(0);
    // the 2 s of docs/fork/core-offset.md (round 15), not the code's constant
    let hold = 2_000_000;
    assert!(us >= hold, "{}: the trim ran {} us after the start, before the configuration stood", case.name, us);
    let last = seen[seen.len() - 1].cores.unwrap_or_else(|| panic!("{}: the cores' own offsets", case.name));
    assert!(last.iter().all(|&e| e == last[0]), "{}: the cores after the trim: {:?}", case.name, last);
    // the first and the last offset of each core: the first was CORE_ERROR off its own, the last is `last[0]` off
    let (mut start, mut end) = ([None; 8], [0; 8]);
    for (n, o) in trace.lines().filter_map(core_offset_write) {
        start[n].get_or_insert(o);
        end[n] = o;
    }
    let from: [Option<i32>; 8] = std::array::from_fn(|n| start[n].map(|s| s - end[n] + last[0]));
    assert_eq!(from, CORE_ERROR.map(Some), "{}: the cores before the trim", case.name);
}

/// The checks of the mask unit's requests against its readings after each pass until the next request: the band
/// around the signal passes every frame, the shifted one (counters cleared) fails every one, the counts grow; after
/// the stop no reading and the test off in 0x4020. Break it catches: a table not loaded or on another slot, column or
/// scale, counters not latched or not cleared, a test left on.
fn mask_checks(case: &Case, trace: &str, seen: &[Seen]) {
    for (from, until, want) in spans(case, seen, |a| if let Act::Mask(w) = a { Some(*w) } else { None }) {
        let mut reads: Vec<MaskRead> = seen[from..until].iter().filter_map(|s| s.mask).collect();
        reads.dedup_by_key(|r| r.frames);
        let Some((shift, _)) = want else {
            assert!(
                seen[from + 1..until].iter().all(|s| s.mask.is_none()),
                "{}: mask readings after its stop",
                case.name
            );
            assert_eq!(last_4020_bit(trace, 1 << 15), Some(false), "{}: the mask test left on", case.name);
            continue;
        };
        assert!(reads.len() >= 3, "{}: {} mask readings from pass {}", case.name, reads.len(), from);
        for r in &reads {
            let fails = if shift == 0.0 { 0 } else { r.frames };
            assert_eq!((r.ch, r.slot, r.fails), (0, 0, fails), "{}: {:?}", case.name, r);
        }
    }
}

/// The checks of the zone trigger's requests: frames while it qualifies them (the crest touched, the box above it
/// not), with its readings counting them; a box above the signal it must touch stalls the arms, and after two the
/// guard drops the unit for these settings (`Shared::unit_blocked`), switches it off and the frames come again,
/// filtered in software. Break it catches: a zone not loaded, of the wrong mode or column, a stall not counted or not
/// dropped, frames that stop for good.
fn zone_checks(case: &Case, trace: &str, seen: &[Seen]) {
    let all = spans(case, seen, |a| if let Act::Zone(w) = a { Some(*w) } else { None });
    for &(from, until, want) in &all {
        let reached = seen[from..until].iter().filter_map(|s| s.zone).map(|z| z.frames).max();
        let decoded = seen[until - 1].decoded - seen[from].decoded;
        let stalls = want.is_some_and(|((_, v), _, inside)| inside && v > 0.5);
        let blocked = seen[from..until].iter().position(|s| s.blocked);
        if !stalls {
            assert!(blocked.is_none() && decoded > 0, "{}: from pass {}: {} frames", case.name, from, decoded);
            assert!(reached.is_some_and(|n| n > 1), "{}: the zone's frames from pass {}", case.name, from);
            continue;
        }
        let Some(b) = blocked else { panic!("{}: the stalled zone not dropped", case.name) };
        let after = seen[until - 1].decoded - seen[from + b].decoded;
        assert!(after > 0, "{}: no frames after the zone was dropped", case.name);
        assert_eq!(last_4020_bit(trace, 1 << 1), Some(false), "{}: the zone left armed", case.name);
    }
}

/// The checks of the LA words read with the records of a triggered case with the LA on: every frame carries them (a
/// batch is one frame while the LA is on, `seq::frames`, la.md 10: only the newest frame of a batch has its LA), as many over as long a time as its analog record (n_la = memsel x m / 2 at m / 2 LA samples
/// per analog sample, la.md 3.3), from the same start before the trigger, and they are the scene's count of
/// `LA_TICK_S` steps: two words next to each other differ by 0 or 1, and the count advances by the record's time.
/// Break it catches: an LA read not made or not delivered, a window of another length or start, a time base of the
/// LA record other than its words', words out of order.
fn la_record_checks(case: &Case, frames: &[Frame]) {
    assert!(frames.len() >= 3, "{}: {} frames", case.name, frames.len());
    for f in frames {
        let Some(la) = &f.la else { panic!("{}: frame {} without its LA words", case.name, f.seq) };
        let (n, span) = (la.words.len(), f.ch[0].len() as f64 * f.dt);
        assert!(
            (n as f64 * la.dt - span).abs() <= la.dt,
            "{}: {} LA words of {} s for {} s",
            case.name,
            n,
            la.dt,
            span
        );
        assert!((la.t0 + f.pre as f64 * f.dt).abs() <= la.dt, "{}: the LA from {} s", case.name, la.t0);
        let steps = la.words.windows(2).all(|p| p[1].wrapping_sub(p[0]) <= 1);
        let advance = la.words[n - 1].wrapping_sub(la.words[0]) as f64;
        let want = (n - 1) as f64 * la.dt / LA_TICK_S;
        assert!(steps && (advance - want).abs() <= 1.0, "{}: the LA count {} for {}", case.name, advance, want);
    }
}

/// The checks of the measure unit's answers a case asked for, in the shared state the loop left: the levels of each
/// channel of the settings the scene's sine (-0.5 V to 0.5 V, mean 0 V; within a 12-bit step and 5 mV), none of a
/// channel not acquired; the slot dump of the raw run, each slot's extremes 1 V apart (on the 12-bit scale, 7500 codes
/// a division at 1 V/div) and the slots of one channel alike. Break it catches: levels not measured or not answered,
/// a channel read from another's slots, volts of another scale or offset, a dump not answered.
fn measure_checks(case: &Case, g: &Shared) {
    let asked = |f: fn(&Act) -> bool| case.script.iter().any(|(_, a)| f(a));
    if asked(|a| matches!(a, Act::Levels(_))) {
        let mut m = Model::default();
        (case.model)(&mut m);
        let Some((1, lv)) = g.levels else { panic!("{}: no levels answered", case.name) };
        for (c, l) in lv.iter().enumerate() {
            assert_eq!(l.is_some(), m.ch[c].on, "{}: levels of CH{}: {:?}", case.name, c + 1, l);
            let Some(l) = l else { continue };
            let ok = (l.lo + 0.5).abs() < 5e-3 && (l.hi - 0.5).abs() < 5e-3 && l.mean.abs() < 5e-3 && !l.clipped;
            assert!(ok, "{}: CH{} levels {:?}", case.name, c + 1, l);
        }
    }
    if asked(|a| matches!(a, Act::Slots)) {
        let Some((1, Some(slots))) = g.slots else { panic!("{}: no slot dump answered: {:?}", case.name, g.slots) };
        for (k, x) in slots.iter().enumerate() {
            let swing = (x[1] - x[0]) * 16.0;
            assert!((swing - 7500.0).abs() < 7500.0 * 2e-2, "{}: slot {}: {:?}", case.name, k, x);
            assert_eq!(*x, slots[k / 4 * 4], "{}: slot {} not like the other slots of its channel", case.name, k);
        }
    }
}

/// The checks of the DVM block's requests in a case's script against the readings after each pass until the next
/// request (as `counter_checks`): readings of the channel asked for on its lane (`dvm::lane`), the first `DVM_EARLY`
/// (before the first whole window) none valid and all after them valid, at least two of them, each the scene's sine:
/// DC 0 V, AC and AC+DC rms its amplitude over the root of 2; after the stop none and 0x2244 written 0. Break it
/// catches: a block not started, not read or not restarted for another channel, a result before a whole window, a lane
/// or channel mixed up, the offset or scale of the volts wrong, a block left running.
fn dvm_checks(case: &Case, trace: &str, seen: &[Seen]) {
    let asked: Vec<(u32, Option<DvmWant>)> =
        case.script.iter().filter_map(|(at, a)| if let Act::Dvm(w) = a { Some((*at, *w)) } else { None }).collect();
    let mut m = Model::default();
    (case.model)(&mut m);
    let mask = super::settings(&m, false).mask;
    let rms = 0.5 / 2f64.sqrt();
    for (i, &(at, want)) in asked.iter().enumerate() {
        let until = asked.get(i + 1).map_or(seen.len(), |n| n.0 as usize);
        let mut reads: Vec<DvmRead> = seen[at as usize..until].iter().filter_map(|s| s.dvm).collect();
        reads.dedup_by_key(|r| r.n);
        let Some(w) = want else {
            assert!(reads.is_empty(), "{}: readings after the stop: {:?}", case.name, reads);
            let last = trace.lines().rfind(|l| l.starts_with("Write") && l.contains("off: 8772,"));
            assert!(last.is_some_and(|l| l.ends_with("v: 0 }")), "{}: the DVM block not stopped", case.name);
            continue;
        };
        let lane = oos_hal::dho::dvm::lane(mask, w.ch);
        assert!(reads.iter().all(|r| (r.ch, Some(r.lane)) == (w.ch, lane)), "{}: {:?}", case.name, reads);
        // a window of 2^28 samples is 1.7 s, the readings 0.5 s apart: the first three see no whole window
        let early = reads.iter().take(DVM_EARLY).filter(|r| r.volts.is_some()).count();
        assert!(reads.len() > DVM_EARLY && early == 0, "{}: valid before the first window: {:?}", case.name, reads);
        let valid: Vec<[f64; 3]> = reads[DVM_EARLY..].iter().map(|r| r.volts.unwrap_or_default()).collect();
        assert!(valid.len() >= 2, "{}: {} valid readings of the request at pass {}", case.name, valid.len(), at);
        for v in valid {
            let ok = v[0].abs() < 5e-3 && v[1..].iter().all(|x| (x - rms).abs() < rms * 2e-2);
            assert!(ok, "{}: CH{}: {:?} V, not 0 V and {} V rms", case.name, w.ch + 1, v, rms);
        }
    }
}

/// The checks of the counter's requests in a case's script against the readings after each pass (`seen[k]`: after k
/// passes) until the next request: in frequency mode readings of the scene's trigger period (its signal's period), in
/// totalize mode a total that grows, after the stop none and 0x2210 written 0. Break it catches: a counter not started
/// or not read, read before its gate, a period other than the signal's, a total that does not count, a counter left
/// running.
fn counter_checks(case: &Case, trace: &str, seen: &[Seen]) {
    let asked: Vec<(u32, Option<CounterWant>)> =
        case.script.iter().filter_map(|(at, a)| if let Act::Counter(w) = a { Some((*at, *w)) } else { None }).collect();
    for (i, &(at, want)) in asked.iter().enumerate() {
        // after the pass that applied it, up to the pass before the one that applies the next
        let until = asked.get(i + 1).map_or(seen.len(), |n| n.0 as usize);
        let mut reads: Vec<CounterRead> = seen[at as usize..until].iter().filter_map(|s| s.counter).collect();
        reads.dedup_by_key(|r| r.n);
        let Some(w) = want else {
            assert!(reads.is_empty(), "{}: readings after the stop: {:?}", case.name, reads);
            assert!(trace.contains("off: 8720, v: 0 }"), "{}: the counter not stopped (0x2210 = 0)", case.name);
            continue;
        };
        assert!(reads.len() >= 2, "{}: {} readings of the request at pass {}", case.name, reads.len(), at);
        let period = case.period_us.map(|p| p as f64 * 1e-6);
        for r in &reads {
            if w.mode == 2 {
                assert!(r.total > 0, "{}: no edges totalized: {:?}", case.name, r);
            } else {
                let ok = r.period.zip(period).is_some_and(|(x, p)| (x - p).abs() < p * 1e-9);
                assert!(ok, "{}: period {:?}, not {:?}", case.name, r.period, period);
            }
        }
        assert!(reads.windows(2).all(|p| p[1].total > p[0].total), "{}: the total does not grow", case.name);
    }
}

/// The checks of the generator's requests in a case's script against what the unit's generator was told by the pass
/// of each (`seen[k]`: after k passes): the output and its supply on as asked, the frequency of the phase increment,
/// the amplitude and offset of the DAC codes and the modulation; a new frequency of the same wave, level and no
/// modulation retuned without a new table (`afg::retune`), any other change of a running output with one. Break it
/// catches: a request not applied, applied to another bank, at another frequency or level, an output left on, a
/// retune that starts over or a start that keeps the old table.
fn generator_checks(case: &Case, seen: &[Seen]) {
    let mut before: Option<(crate::board::Gen, GenState)> = None;
    for (at, want) in case.script.iter().filter_map(|(at, a)| if let Act::Gen(w) = a { Some((*at, *w)) } else { None })
    {
        let g = seen[at as usize].gen;
        let (on, f, a, o, wave, md) = want;
        assert_eq!((g.output, g.power), (on, on), "{}: the generator after pass {}: {:?}", case.name, at, g);
        let modulation = match md {
            Mod::None => 0,
            Mod::Am { .. } => 0x82,
            Mod::Fm { .. } => 0x84,
            Mod::Pm { .. } => 0x88,
        };
        if on {
            assert!((g.hz - f).abs() < 1e-3, "{}: {} Hz, not {} Hz", case.name, g.hz, f);
            // the levels of the DAC codes on the path of the relays: within a step of 0x3070 / 0x3078 at gains <= 1
            let (da, doff) = (g.vpp - a, g.offset_v - o);
            assert!(
                da.abs() < 2e-4 && doff.abs() < 2e-4,
                "{}: {} Vpp {} V, not {} {}",
                case.name,
                g.vpp,
                g.offset_v,
                a,
                o
            );
            assert_eq!(g.modulation, modulation, "{}: modulation after pass {}", case.name, at);
        }
        if let Some((w, b)) = before.filter(|(w, _)| w.0 && on) {
            let retune = (w.2, w.3, w.4, w.5) == (a, o, wave, Mod::None) && md == Mod::None;
            assert_eq!(g.samples == b.samples, retune, "{}: a new table after pass {}", case.name, at);
        }
        before = Some((want, g));
    }
}

/// The model in Average mode of `count`, in the FPGA (`hw`) or in software: CH1 at 1 us/div, records of 1k points.
fn average(m: &mut Model, hw: bool, count: u32) {
    ch(m, &[0]);
    tdiv(m, 1e-6);
    depth(m, "1k");
    (m.acq, m.avg_hw, m.avg_count) = (AcqMode::Average, hw, count);
}

/// The offset the scene adds in Average mode: +`DITHER` V at every other trigger, -`DITHER` V at the others (a frame
/// is much shorter than the trigger period), so an average of an even count takes it out and a frame not averaged
/// keeps it.
const DITHER: f64 = 0.02;

/// `DITHER` with its sign at `t` s, of the trigger nearest to it, the triggers `period_us` apart.
fn dither(t: f64, period_us: i128) -> f64 {
    if (t * 1e6 / period_us as f64).round() as i64 % 2 == 0 {
        DITHER
    } else {
        -DITHER
    }
}

/// The model of a case in Average mode, else None.
fn averaged(case: &Case) -> Option<Model> {
    let mut m = Model::default();
    (case.model)(&mut m);
    (m.acq == AcqMode::Average).then_some(m)
}

/// The checks of a case in Average mode (`m`), whose scene adds `DITHER`. With the hardware average: 0x10b4 pulsed by
/// the configuration and by every clear the screen asked for (CLEAR, new settings), one frame a batch, each the FPGA's
/// (`hw_avg`) and the scene's signal with the offset averaged out. In software: no average in the FPGA, every frame a
/// raw one with the offset of its trigger, both signs. Break it catches: an average not switched on or not read, a
/// clear left out (the frames after new settings would mix records of the old scale), more than one frame a batch, a
/// software average done by the FPGA.
fn average_checks(case: &Case, m: &Model, trace: &str, frames: &[Frame]) {
    let writes = |off: &str| trace.lines().filter(|l| l.starts_with("Write") && l.contains(off)).collect::<Vec<_>>();
    let pulses = writes("off: 4276, v: 1 }").len();
    let batches = trace.lines().filter(|l| l.starts_with("Played")).count();
    assert!(!frames.is_empty(), "{}: no frame", case.name);
    if m.avg_hw {
        let asked = case.script.iter().filter(|(_, a)| matches!(a, Act::AvgClear | Act::SetView(_))).count();
        assert_eq!(pulses, 1 + asked, "{}: the average clears (0x10b4)", case.name);
        assert_eq!(frames.len(), batches, "{}: one averaged frame a batch", case.name);
    } else {
        let mode = writes("off: 4100,");
        let on = mode.iter().filter_map(|l| l.rsplit("v: ").next()?.trim_end_matches(" }").parse::<u32>().ok());
        assert!(pulses == 0 && on.into_iter().all(|v| v & 1 << 3 == 0), "{}: averaged in the FPGA", case.name);
    }
    let volts = signal(case.freq_hz, case.square);
    let mut signs = [false; 2];
    for f in frames {
        assert_eq!(f.hw_avg, m.avg_hw, "{}: frame {} hw_avg", case.name, f.seq);
        let res: Vec<f64> =
            f.ch[0].iter().enumerate().map(|(i, &x)| x as f64 - volts(0, (i as f64 - f.pre as f64) * f.dt)).collect();
        let off = res.iter().sum::<f64>() / res.len().max(1) as f64;
        let spread = res.iter().fold(0f64, |a, r| a.max((r - off).abs()));
        assert!(spread < 2e-3, "{}: frame {} is not the scene's signal: {} V off", case.name, f.seq, spread);
        if m.avg_hw {
            assert!(off.abs() < DITHER / 4.0, "{}: frame {}: {} V left of the offset", case.name, f.seq, off);
        } else {
            assert!((off.abs() - DITHER).abs() < 2e-3, "{}: frame {}: offset {} V", case.name, f.seq, off);
            signs[(off > 0.0) as usize] = true;
        }
    }
    assert!(m.avg_hw || signs == [true, true], "{}: the raw frames' offsets of both signs", case.name);
}

/// What each segment case must end with: the recordings armed (frame 0 recorded) and the segments of the last one.
/// The stopped one ends after 30 (STOP at pass 5, a poll of 10 ms each and 1 ms a segment); the aborted one starts
/// again with the new settings (`hw::seg_abort`, then a new recording).
const SEG_EXPECT: &[(&str, usize, u32)] = &[
    ("segments-20-then-show-one", 1, 20),
    ("segments-10-1ms-apart", 1, 10),
    ("segments-stopped-early", 1, 30),
    ("segments-aborted-by-new-settings", 2, SEG_ABORTED),
];

/// Segments of the second recording of the aborted case when the run ends (it still records).
const SEG_ABORTED: u32 = 60;

/// The model with hardware segments: `n` of them, CH1 at 1 us/div.
fn segments(m: &mut Model, n: usize) {
    ch(m, &[0]);
    tdiv(m, 1e-6);
    (m.seg_hw, m.segments) = (true, n);
}

/// The step of the count on the logic analyser's lines: longer than the spacing of its words in every case, so two
/// words next to each other differ by 0 or 1, and a chunk lost or out of order (100 words) shows as a larger step.
const LA_TICK_S: f64 = 1e-4;

/// The lines of the logic analyser at `t` s: the count of `LA_TICK_S` steps.
fn la_lines(t: f64) -> u16 {
    (t / LA_TICK_S) as u64 as u16
}

/// Passes of a scan case: a sweep of 2 s at 200 ms/div (a read and an empty poll of 9 ms per 4 to 5 chunks of 2 ms),
/// the pause or the halt after it, and the start of the next sweep.
const SCAN_PASSES: u32 = 600;

/// The scene's signal on every channel: a 0.5 V sine of `f` Hz, or the square of its sign, rising through 0 at every
/// trigger instant (they are whole periods apart).
fn signal(f: f64, square: bool) -> Box<dyn Fn(usize, f64) -> f64> {
    Box::new(move |_c, t| {
        let x = (2.0 * std::f64::consts::PI * f * t).sin();
        0.5 * if square { x.signum() } else { x }
    })
}

/// One line of a frame: its fields and, per channel with points, the count, the extremes, the mean (to 0.1 mV) and a
/// checksum of every sample (FNV-1a over their bits), so a change of any sample shows in the comparison.
fn frame_line(f: &Frame) -> String {
    let mut s =
        format!("frame {} dt {:e} pre {} env {} roll {} scan {}", f.seq, f.dt, f.pre, f.envelope, f.roll, f.scan);
    for (c, v) in f.ch.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        let mean = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
        let sum = v
            .iter()
            .flat_map(|x| x.to_bits().to_le_bytes())
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        s += &format!(" | CH{} n {} lo {:.4} hi {:.4} mean {:.4} sum {:016x}", c + 1, v.len(), lo, hi, mean, sum);
    }
    if let Some(la) = &f.la {
        let sum = la
            .words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        s += &format!(" | LA n {} t0 {:e} dt {:e} sum {:016x}", la.words.len(), la.t0, la.dt, sum);
    }
    s
}

/// The switches a case sets for the test's thread (`policy::compress::force`, `deepmeas::force`): back to the build's when
/// dropped.
struct Switches;

impl Drop for Switches {
    fn drop(&mut self) {
        crate::board::policy::compress::force(None);
        crate::deepmeas::force(None);
        super::plan::force_prearm(None);
        super::trim::force_core_offset(None);
    }
}

/// What the front ends and the time base of the settings the model `m` gives make of the scene: the volts per division
/// and offset of each channel, the sample period and the points before and after the trigger.
fn view_of(m: &Model) -> View {
    let want = super::settings(m, false);
    let mut st = oos_hal::dho::seq::State::handover(want.mask, None);
    let p = st.configure(&want, None, None).expect("settings the unit takes").1;
    let cal = std::array::from_fn(|c| (want.ch[c].scale, want.ch[c].offset));
    View { cal, dot_fs: p.dot_fs as i128, pre: p.pre, post: p.post }
}

/// Runs `case` and returns the unit's trace, the frames delivered, the channels of the settings, what the loop left
/// in the shared state and what the screen's side saw after each pass.
fn run_case(case: &Case) -> (String, Vec<Frame>, u32, HwShared, Vec<Seen>) {
    let mut m = Model::default();
    (case.model)(&mut m);
    // the switches of this thread, put back when the case ends (a failing one too)
    let _switches = Switches;
    crate::board::policy::compress::force(Some(case.compress));
    super::plan::force_prearm(Some(case.switches.contains(&Switch::Prearm)));
    super::trim::force_core_offset(Some(case.switches.contains(&Switch::CoreTrim)));
    let want = super::settings(&m, false);
    let volts = signal(case.freq_hz, case.square);
    // in Average mode an offset that differs from trigger to trigger, which the average takes out
    let volts: Box<dyn Fn(usize, f64) -> f64> = match (m.acq, case.period_us) {
        (AcqMode::Average, Some(p)) => Box::new(move |c, t| volts(c, t) + dither(t, p)),
        _ => volts,
    };
    let scene = Scene {
        volts,
        la: case.la.then(|| Box::new(la_lines) as Box<dyn Fn(f64) -> u16>),
        first_fs: case.period_us.unwrap_or(1) * US,
        period_fs: case.period_us.map(|p| p * US),
        auto_fs: None,
        view: view_of(&m),
    };
    let (u, bar, c2h, clock) = unit(scene);
    let sim = u.clone();
    // what the screen's side saw after each pass
    let mut seen = Vec::new();
    // with the calibration the front ends are programmed too, through a stub that answers them
    let afe = case.cal.then_some(oos_hal::dho::boot::sim::AfeStub { writes: 0, reads: 0 });
    let port = SimPort(Some(u.clone()));
    let hw = Hw { bar, c2h, afe, lines: oos_hal::dho::lines::Recorded::default(), clock, port };
    let out: HwShared = std::sync::Arc::new(std::sync::Mutex::new(Shared::default()));
    let mask = want.mask;
    {
        let mut g = lock(&out);
        g.want = Some(want);
        g.slow = super::slow_mode(&m);
    }
    let (mut pass, mut rearm_due) = (0, false);
    // roll and scan replace the picture in the queue: every one published is taken here, as the screen takes it
    let mut slow_frames: Vec<Frame> = Vec::new();
    let mut replays: Vec<Frame> = Vec::new();
    let shared = out.clone();
    // the made-up calibration set in a folder of this case (its files are always the same bytes)
    let cal_dir = if case.cal {
        let d = std::env::temp_dir().join(format!("oos-liverun-cal-{}", std::process::id()));
        let d = d.to_str().expect("a folder name in UTF-8").to_string();
        oos_hal::dho::madeup::write(&d).expect("the made-up set written");
        d
    } else {
        "/nonexistent".to_string()
    };
    // the core trim's case: the cores' own offsets against the stored ones of the made-up set
    if case.switches.contains(&Switch::CoreTrim) {
        u.borrow_mut().cores = own_offsets(&m, &cal_dir);
    }
    // the deep measurements' raw window is a switch of the build; on for this thread, as on the scope
    crate::deepmeas::force(Some(true));
    // the drawing workers are another test's when its screen switched them on: this loop keeps out of them (their
    // hand-over is run on a unit, docs/board-on-a-host.md 7)
    let _out = crate::drawpipe::keep_out();
    super::run(hw, super::settings(&m, false).mask, case.cal, cal_dir.clone(), out.clone(), || {
        pass += 1;
        let mut g = lock(&shared);
        // the state after the pass before this one (`Seen`)
        seen.push(Seen {
            gen: sim.borrow().generator(),
            counter: g.counter,
            dvm: g.dvm,
            mask: g.mask,
            zone: g.zone,
            decoded: g.decoded,
            blocked: g.unit_blocked.is_some(),
            events: sim.borrow().trace.len(),
            cores: sim.borrow().cores.map(|own| {
                let o = sim.borrow().core_offsets();
                std::array::from_fn(|n| o[n] - own[n])
            }),
        });
        for (_, act) in case.script.iter().filter(|(at, _)| *at == pass) {
            match act {
                Act::Set(f) => {
                    f(&mut m);
                    g.want = Some(super::settings(&m, false));
                    g.slow = super::slow_mode(&m);
                }
                Act::SetView(f) => {
                    f(&mut m);
                    g.want = Some(super::settings(&m, false));
                    sim.borrow_mut().scene.view = view_of(&m);
                }
                Act::AvgClear => g.avg_clear += 1,
                Act::Gen(w) => g.afg[0] = Some(*w),
                Act::Counter(w) => g.counter_want = *w,
                Act::Dvm(w) => g.dvm_want = *w,
                Act::Levels(runs) => g.levels_want = Some((1, 0, *runs)),
                Act::Slots => g.slots_want = Some(1),
                Act::Mask(w) => g.mask_want = w.map(|w| mask_want(case, &m, w)),
                Act::Zone(w) => {
                    g.zone_want =
                        w.map(|(a, b, inside)| ZoneWant { ch: 0, zones: vec![(vec![a, b], inside)], clear: 0 })
                }
                Act::Hold => g.hold = true,
                Act::Rearm => rearm_due = true,
                Act::SegRun => (g.seg.run, g.seg.running) = (g.seg.run + 1, true),
                Act::SegStop => g.seg.running = false,
                Act::SegShow(k) => g.seg.want = Some(*k),
                Act::FinalRaw => g.final_req += 1,
                Act::StopView(t, d) => g.stop_view = Some((*t, *d)),
                Act::Force => g.force = true,
                Act::DeepRaw => g.deep_raw = Some(crate::board::policy::deep::Want { every: true }),
                Act::Busy(us) => sim.borrow_mut().pass_us(*us),
            }
        }
        if rearm_due && g.halted {
            (g.rearm, rearm_due) = (true, false);
        }
        if (g.frame.roll || g.frame.scan) && slow_frames.last().is_none_or(|f| f.seq != g.frame.seq) {
            slow_frames.push(g.frame.clone());
        }
        // STOP: every view read again
        if let Some(f) = g.replay.take() {
            replays.push(f);
        }
        pass <= case.passes
    });
    let queued = lock(&out).queue.iter().filter(|f| !f.roll && !f.scan).cloned().collect::<Vec<_>>();
    if case.cal {
        let _ = std::fs::remove_dir_all(&cal_dir);
    }
    let frames: Vec<Frame> = slow_frames.into_iter().chain(queued).chain(replays).collect();
    let trace = u.borrow().trace_text();
    (trace, frames, mask, out, seen)
}

/// The line of a segmented recording: segments asked for, recorded, read back, the time of each and the one shown.
fn seg_line(g: &Shared) -> String {
    let times: Vec<String> =
        g.seg.times.iter().map(|t| t.map_or("-".to_string(), |t| format!("{:.3}", t * 1e6))).collect();
    let shown = g.seg.shown.as_ref().map_or("none".to_string(), |(k, f)| format!("{}: {}", k, frame_line(f)));
    format!(
        "segments total {} recorded {} read {} times_us [{}] shown {}\n",
        g.seg.total,
        g.seg.recorded,
        g.seg.read,
        times.join(" "),
        shown
    )
}

// Stage 2 of ADR 0053: the loop against the simulated unit for every case of the matrix. Break it catches: a case
// that arms or delivers nothing although its scene triggers, delivers anything although it never triggers, or
// delivers something else than its scene; and, through tools/compare-base.sh, any change of what the loop does
#[test]
fn the_acquisition_loop_runs_the_matrix_on_the_simulated_unit() {
    let dir = std::env::var_os("OOS_LIVE_OUT").map(std::path::PathBuf::from);
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).expect("OOS_LIVE_OUT is a folder");
    }
    for case in CASES {
        let (trace, frames, mask, out, seen) = run_case(case);
        let g = lock(&out);
        if let Some(d) = &dir {
            let mut text = format!("# {}\n", case.name);
            text += &trace;
            for f in &frames {
                text += &frame_line(f);
                text.push('\n');
            }
            if g.seg.total > 0 {
                text += &seg_line(&g);
            }
            std::fs::write(d.join(format!("{}.txt", case.name)), text).expect("write the case's file");
        }
        generator_checks(case, &seen);
        counter_checks(case, &trace, &seen);
        dvm_checks(case, &trace, &seen);
        measure_checks(case, &g);
        mask_checks(case, &trace, &seen);
        zone_checks(case, &trace, &seen);
        prearm_checks(case, &trace, &seen);
        trim_checks(case, &trace, &seen);
        let count = |what: &str| trace.lines().filter(|l| l.starts_with(what)).count();
        let (played, streamed) = (count("Played"), count("Stream"));
        // what the screen asked for in the script was done
        let asked = |f: fn(&Act) -> bool| case.script.iter().filter(|(_, a)| f(a)).count();
        if asked(|a| matches!(a, Act::Force)) > 0 {
            assert!(trace.contains("off: 8284, v: 32 }") && !g.force, "{}: no force pulse (0x205c bit 5)", case.name);
        }
        // every view in STOP read again from the stored record; none after a batch armed ahead, which wrote the ring
        let views = asked(|a| matches!(a, Act::StopView(..)));
        let replayed = frames.iter().filter(|f| f.fixed_ti.is_some()).count();
        let ahead = case.switches.contains(&Switch::Prearm);
        assert_eq!(replayed, if ahead { 0 } else { views }, "{}: views in STOP read again", case.name);
        if asked(|a| matches!(a, Act::FinalRaw)) > 0 {
            assert_eq!(g.final_ack, g.final_req, "{}: the final frame request answered", case.name);
            let last = frames.last().expect("frames before the STOP");
            assert!(!last.envelope, "{}: the newest frame read raw at STOP", case.name);
        }
        // live compression: the batches before the STOP came through it (frames of min/max pairs)
        if case.compress && !case.script.iter().any(|(_, a)| matches!(a, Act::DeepRaw)) {
            let before = &frames[..frames.len().saturating_sub(1)];
            assert!(before.iter().any(|f| f.envelope), "{}: no frame through the live compression", case.name);
        }
        // the deep measurements: the record read compressed, then its screen window raw, every sample of more than
        // the deepest compressed record
        if asked(|a| matches!(a, Act::DeepRaw)) > 0 {
            let raw = frames
                .iter()
                .filter(|f| !f.envelope && f.ch[0].len() > crate::board::policy::compress::MAX_DEPTH as usize);
            assert!(raw.count() > 0, "{}: no raw screen window of the deep record", case.name);
            let mut plays: Vec<&str> = trace
                .lines()
                .filter(|l| l.starts_with("Played"))
                .filter_map(|l| l.find("bytes").map(|i| &l[i..]))
                .collect();
            plays.sort_unstable();
            plays.dedup();
            assert!(plays.len() >= 2, "{}: the compressed read and the raw window: {:?}", case.name, plays);
        }
        // the calibration reaches the configuration: the same case without it writes other values
        if case.cal {
            let (plain, ..) = run_case(&Case { cal: false, ..*case });
            let writes = |t: &str| -> Vec<String> {
                t.lines().filter(|l| l.starts_with("Write")).map(|l| l[l.find("off").unwrap()..].to_string()).collect()
            };
            assert_ne!(writes(&trace), writes(&plain), "{}: the same writes as without the calibration", case.name);
        }
        // roll or scan from the case's settings, not from what came out: a slow case that delivers nothing fails
        let slow = {
            let mut m = Model::default();
            (case.model)(&mut m);
            super::slow_mode(&m) != crate::board::Slow::Off
        };
        if case.la && !slow {
            la_record_checks(case, &frames);
        }
        if let Some(m) = averaged(case) {
            average_checks(case, &m, &trace, &frames);
        } else if slow {
            assert!(!frames.is_empty(), "{}: no picture published", case.name);
            // roll and scan: pictures of the scene's signal on the channels of the settings, in roll the newest point
            // at the right edge; in scan a whole sweep and the start of the next (RUN: after the pause; SINGLE:
            // after the next press)
            assert!(count("Chunks") > 0, "{}: no chunk was streamed", case.name);
            for f in &frames {
                assert!(f.roll || f.scan, "{}: a triggered frame {} among the slow ones", case.name, f.seq);
                let on = (0..4).filter(|&c| !f.ch[c].is_empty()).fold(0, |m, c| m | 1 << c);
                assert_eq!(on, mask, "{}: picture {} has the channels {:#x}", case.name, f.seq, on);
                for v in f.ch.iter().filter(|v| !v.is_empty()) {
                    let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                    assert!((hi - 0.5).abs() < 0.01 && (lo + 0.5).abs() < 0.01, "{}: {} to {} V", case.name, lo, hi);
                }
            }
            if case.la {
                let mut swept = false;
                for f in &frames {
                    let w = &f.la.as_ref().expect("the LA words with every picture").words;
                    assert!(!w.is_empty(), "{}: picture {} without LA words", case.name, f.seq);
                    // a partial scan picture after a whole sweep holds the sweep being written up to its write
                    // position, then what is left of the sweep before: one jump at that seam, none anywhere else
                    let jumps = w.windows(2).filter(|p| p[1].wrapping_sub(p[0]) > 1).count();
                    let seams = usize::from(f.scan && f.partial && swept);
                    assert!(jumps <= seams, "{}: picture {}: the LA count jumps {} times", case.name, f.seq, jumps);
                    swept |= f.scan && !f.partial;
                }
            }
            // scan with the trigger point left of the screen: the scan trigger position, enabled (0x4058 bit 31)
            let mut m = Model::default();
            (case.model)(&mut m);
            if frames[0].scan && m.delay as f64 > 5.1 * m.hw_time_per_div() as f64 {
                let pos = trace.lines().filter(|l| l.starts_with("Write") && l.contains("off: 16472,"));
                let on = pos.filter_map(|l| l.rsplit("v: ").next()?.trim_end_matches(" }").parse::<u32>().ok());
                assert!(on.into_iter().any(|v| v & 1 << 31 != 0), "{}: no scan trigger position", case.name);
            }
            // a whole sweep and the next where the case runs long enough for one
            if frames[0].scan && case.passes >= SCAN_PASSES {
                assert!(frames.iter().any(|f| !f.partial), "{}: no sweep completed", case.name);
                let sweeps = trace.lines().filter(|l| l.contains("counter: 1,")).count();
                assert!(sweeps >= 2, "{}: {} sweeps started", case.name, sweeps);
            }
        } else if case.period_us.is_some() {
            // hardware segments: every segment recorded is read back with its time, a trigger period after the one
            // before it; delivered to the screen when the recording ended by itself, not after STOP
            let stopped = g.seg.total > 0 && g.seg.recorded < g.seg.total;
            let recording = g.seg.phase == crate::board::SegPhase::Recording;
            if let Some(&(_, armed, recorded)) = SEG_EXPECT.iter().find(|e| e.0 == case.name) {
                let zero = trace.lines().filter(|l| l.starts_with("Recorded") && l.contains(" frame: 0,")).count();
                assert_eq!((zero, g.seg.recorded), (armed, recorded), "{}: recordings, segments", case.name);
            }
            if g.seg.total > 0 && !recording {
                assert!(g.seg.recorded > 0 && g.seg.read == g.seg.recorded, "{}: {}", case.name, seg_line(&g));
                // a segment is a trigger of the scene, and the next is the first trigger after the record before it
                // and the interval: back to back without one, a whole number of trigger periods apart, never less
                // than the interval
                let period = case.period_us.unwrap() as f64 * 1e-6;
                let mut m = Model::default();
                (case.model)(&mut m);
                let ivl = m.seg_ivl as f64;
                let times: Vec<f64> = g
                    .seg
                    .times
                    .iter()
                    .enumerate()
                    .map(|(k, t)| t.unwrap_or_else(|| panic!("{}: segment {} without a stamp", case.name, k)))
                    .collect();
                for (k, w) in times.windows(2).enumerate() {
                    let periods = (w[1] - w[0]) / period;
                    assert!((periods - periods.round()).abs() < 1e-6, "{}: segment {} at {} s", case.name, k + 1, w[1]);
                    let want = if ivl == 0.0 { periods.round() == 1.0 } else { w[1] - w[0] >= ivl };
                    assert!(want, "{}: segment {} {} s after the one before", case.name, k + 1, w[1] - w[0]);
                }
                let delivered = if stopped { 0 } else { g.seg.recorded as usize };
                assert_eq!(frames.len(), delivered, "{}: frames delivered", case.name);
                if let Some(k) =
                    case.script.iter().find_map(|(_, a)| if let Act::SegShow(k) = a { Some(*k) } else { None })
                {
                    assert_eq!(g.seg.shown.as_ref().map(|s| s.0), Some(k), "{}: the segment shown", case.name);
                    let v = &g.seg.shown.as_ref().unwrap().1.ch[0];
                    let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                    assert!(
                        (hi - 0.5).abs() < 0.01 && (lo + 0.5).abs() < 0.01,
                        "{}: shown {} to {} V",
                        case.name,
                        lo,
                        hi
                    );
                }
            }
            assert!(
                (played > 0 || recording) && (stopped || !frames.is_empty()),
                "{}: played {}, {} frames",
                case.name,
                played,
                frames.len()
            );
            let volts = signal(case.freq_hz, case.square);
            for f in &frames {
                let on = (0..4).filter(|&c| !f.ch[c].is_empty()).fold(0, |m, c| m | 1 << c);
                assert_eq!(on, mask, "{}: frame {} has the channels {:#x}", case.name, f.seq, on);
                for (c, v) in f.ch.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
                    let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                    assert!((hi - 0.5).abs() < 0.01 && (lo + 0.5).abs() < 0.01, "{}: {} to {} V", case.name, lo, hi);
                    // every sample, where a point is one sample: the scene's triggers are whole periods of its
                    // signal apart, so point i is the signal (i - pre) points after a trigger at phase 0 (a square
                    // not at its edges, where the sign of the sine is the rounding's)
                    if !f.envelope {
                        for (i, &x) in v.iter().enumerate() {
                            let t = (i as f64 - f.pre as f64) * f.dt;
                            if case.square && (2.0 * std::f64::consts::PI * case.freq_hz * t).sin().abs() < 1e-3 {
                                continue;
                            }
                            let want = volts(c, t);
                            assert!(
                                (x as f64 - want).abs() < 2e-3,
                                "{}: CH{} point {}: {} V, not {} V",
                                case.name,
                                c + 1,
                                i,
                                x,
                                want
                            );
                        }
                    }
                }
            }
        } else {
            assert!(
                frames.is_empty() && played == 0 && streamed == 0,
                "{}: {} frames, {} plays, {} stream reads without a trigger",
                case.name,
                frames.len(),
                played,
                streamed
            );
        }
    }
}
