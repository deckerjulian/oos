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
use crate::board::{lock, Frame, HwShared, Shared};
use crate::model::*;
use oos_hal::dho::sim::{unit, Scene, View};

/// The simulated unit as the loop's port: its register window, stream and clock, a front-end stub, lines that record,
/// and a generator that is not there.
pub(super) struct SimPort;
pub(super) struct NoGen;

impl oos_hal::dho::afg::GenLines for NoGen {
    fn relays(&mut self, _code: u8) {}
    fn power(&mut self, _on: bool) {}
    fn relay_code(&self) -> u8 {
        0
    }
}

impl Port for SimPort {
    type Bus = oos_hal::dho::sim::Bar;
    type Stream = oos_hal::dho::sim::Stream;
    type Afe = oos_hal::dho::boot::sim::AfeStub;
    type Lines = oos_hal::dho::lines::Recorded;
    type Clock = oos_hal::dho::sim::Time;
    type Gen = NoGen;
    fn open_gen(&mut self) -> std::io::Result<NoGen> {
        Ok(NoGen)
    }
}

/// Femtoseconds of a microsecond.
const US: i128 = 1_000_000_000;

/// What the screen does at a pass of the loop.
enum Act {
    /// new settings: the model changed as the function says
    Set(fn(&mut Model)),
    /// new settings that change what the front ends make of the scene: the model, and the scene's view of it
    SetView(fn(&mut Model)),
    /// CLEAR of the average (`Shared::avg_clear`)
    AvgClear,
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
];

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

/// Runs `case` and returns the unit's trace, the frames delivered, the channels of the settings and what the loop left
/// in the shared state.
fn run_case(case: &Case) -> (String, Vec<Frame>, u32, HwShared) {
    let mut m = Model::default();
    (case.model)(&mut m);
    // the switches of this thread, put back when the case ends (a failing one too)
    let _switches = Switches;
    crate::board::policy::compress::force(Some(case.compress));
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
    // with the calibration the front ends are programmed too, through a stub that answers them
    let afe = case.cal.then_some(oos_hal::dho::boot::sim::AfeStub { writes: 0, reads: 0 });
    let hw = Hw { bar, c2h, afe, lines: oos_hal::dho::lines::Recorded::default(), clock, port: SimPort };
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
    // the deep measurements' raw window is a switch of the build; on for this thread, as on the scope
    crate::deepmeas::force(Some(true));
    // the drawing workers are another test's when its screen switched them on: this loop keeps out of them (their
    // hand-over is run on a unit, docs/board-on-a-host.md 7)
    let _out = crate::drawpipe::keep_out();
    super::run(hw, super::settings(&m, false).mask, case.cal, cal_dir.clone(), out.clone(), || {
        pass += 1;
        let mut g = lock(&shared);
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
                Act::Hold => g.hold = true,
                Act::Rearm => rearm_due = true,
                Act::SegRun => (g.seg.run, g.seg.running) = (g.seg.run + 1, true),
                Act::SegStop => g.seg.running = false,
                Act::SegShow(k) => g.seg.want = Some(*k),
                Act::FinalRaw => g.final_req += 1,
                Act::StopView(t, d) => g.stop_view = Some((*t, *d)),
                Act::Force => g.force = true,
                Act::DeepRaw => g.deep_raw = Some(crate::board::policy::deep::Want { every: true }),
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
    (trace, frames, mask, out)
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
        let (trace, frames, mask, out) = run_case(case);
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
        let count = |what: &str| trace.lines().filter(|l| l.starts_with(what)).count();
        let (played, streamed) = (count("Played"), count("Stream"));
        // what the screen asked for in the script was done
        let asked = |f: fn(&Act) -> bool| case.script.iter().filter(|(_, a)| f(a)).count();
        if asked(|a| matches!(a, Act::Force)) > 0 {
            assert!(trace.contains("off: 8284, v: 32 }") && !g.force, "{}: no force pulse (0x205c bit 5)", case.name);
        }
        let views = asked(|a| matches!(a, Act::StopView(..)));
        let replayed = frames.iter().filter(|f| f.fixed_ti.is_some()).count();
        assert_eq!(replayed, views, "{}: every view in STOP read again from the stored record", case.name);
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
            let (plain, _, _, _) = run_case(&Case { cal: false, ..*case });
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
