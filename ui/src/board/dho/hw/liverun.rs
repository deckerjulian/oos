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
use crate::board::{Frame, HwShared, Shared};
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
    /// STOP
    Hold,
    /// the next SINGLE press, once the scan sweep of the last has halted (at this pass or the first after it)
    Rearm,
}

/// One case of the matrix: the model as the screen sets it, the frequency of the scene's sine, the trigger period of
/// the scene (None: it never triggers; roll and scan do not wait for it), the passes of the loop, and what the screen
/// does at which pass.
struct Case {
    name: &'static str,
    model: fn(&mut Model),
    freq_hz: f64,
    /// a square of the sine's sign instead of the sine (an edge a slow time base sees as steep)
    square: bool,
    period_us: Option<i128>,
    passes: u32,
    script: &'static [(u32, Act)],
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
        passes: 30,
        script: &[],
    },
    Case {
        name: "auto-ch12-1us",
        model: |m| (ch(m, &[0, 1]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        passes: 30,
        script: &[],
    },
    Case {
        name: "auto-ch1234-1us",
        model: |m| (ch(m, &[0, 1, 2, 3]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        passes: 30,
        script: &[],
    },
    Case {
        name: "auto-ch1-1ms",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-3)).0,
        freq_hz: 1e3,
        square: true,
        period_us: Some(1_000),
        passes: 20,
        script: &[],
    },
    Case {
        name: "normal-no-trigger",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Normal).0,
        freq_hz: 1e6,
        square: false,
        period_us: None,
        passes: 6,
        script: &[],
    },
    Case {
        name: "normal-new-settings-during-the-wait",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Normal).0,
        freq_hz: 1e6,
        square: false,
        period_us: None,
        passes: 8,
        script: &[(3, Act::Set(|m| tdiv(m, 2e-6)))],
    },
    Case {
        name: "single-ch1",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6), m.trig_mode = TrigMode::Single).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        passes: 20,
        script: &[],
    },
    Case {
        name: "auto-then-stop",
        model: |m| (ch(m, &[0]), tdiv(m, 1e-6)).0,
        freq_hz: 1e6,
        square: false,
        period_us: Some(50),
        passes: 30,
        script: &[(12, Act::Hold)],
    },
    Case {
        name: "roll-ch1-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = true).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        passes: 40,
        script: &[],
    },
    Case {
        name: "roll-ch1234-200ms",
        model: |m| (ch(m, &[0, 1, 2, 3]), tdiv(m, 0.2), m.auto_roll = true).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        passes: 40,
        script: &[],
    },
    Case {
        name: "scan-run-ch1-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = false).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        passes: SCAN_PASSES,
        script: &[],
    },
    Case {
        name: "scan-single-ch1-200ms",
        model: |m| (ch(m, &[0]), tdiv(m, 0.2), m.auto_roll = false, m.trig_mode = TrigMode::Single).0,
        freq_hz: 1e3,
        square: false,
        period_us: None,
        passes: SCAN_PASSES,
        script: &[(SCAN_PASSES / 2, Act::Rearm)],
    },
];

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
    s
}

/// Runs `case` and returns the unit's trace, the frames delivered and the channels of the settings.
fn run_case(case: &Case) -> (String, Vec<Frame>, u32) {
    let mut m = Model::default();
    (case.model)(&mut m);
    let want = super::settings(&m, false);
    let mut st = oos_hal::dho::seq::State::handover(want.mask, None);
    let p = st.configure(&want, None, None).expect("settings the unit takes").1;
    let cal = std::array::from_fn(|c| (want.ch[c].scale, want.ch[c].offset));
    let view = View { cal, dot_fs: p.dot_fs as i128, pre: p.pre, post: p.post };
    let scene = Scene {
        volts: signal(case.freq_hz, case.square),
        first_fs: case.period_us.unwrap_or(1) * US,
        period_fs: case.period_us.map(|p| p * US),
        auto_fs: None,
        view,
    };
    let (u, bar, c2h, clock) = unit(scene);
    let hw = Hw { bar, c2h, afe: None, lines: oos_hal::dho::lines::Recorded::default(), clock, port: SimPort };
    let out: HwShared = std::sync::Arc::new(std::sync::Mutex::new(Shared::default()));
    let mask = want.mask;
    {
        let mut g = out.lock().unwrap();
        g.want = Some(want);
        g.slow = super::slow_mode(&m);
    }
    let (mut pass, mut rearm_due) = (0, false);
    // roll and scan replace the picture in the queue: every one published is taken here, as the screen takes it
    let mut slow_frames: Vec<Frame> = Vec::new();
    let shared = out.clone();
    // the drawing workers are another test's when its screen switched them on: this loop keeps out of them (their
    // hand-over is run on a unit, docs/board-on-a-host.md 7)
    crate::drawpipe::keep_out(true);
    super::run(hw, super::settings(&m, false).mask, false, "/nonexistent".into(), out.clone(), || {
        pass += 1;
        let mut g = shared.lock().unwrap();
        for (_, act) in case.script.iter().filter(|(at, _)| *at == pass) {
            match act {
                Act::Set(f) => {
                    f(&mut m);
                    g.want = Some(super::settings(&m, false));
                    g.slow = super::slow_mode(&m);
                }
                Act::Hold => g.hold = true,
                Act::Rearm => rearm_due = true,
            }
        }
        if rearm_due && g.halted {
            (g.rearm, rearm_due) = (true, false);
        }
        if (g.frame.roll || g.frame.scan) && slow_frames.last().is_none_or(|f| f.seq != g.frame.seq) {
            slow_frames.push(g.frame.clone());
        }
        pass <= case.passes
    });
    let queued = out.lock().unwrap().queue.iter().filter(|f| !f.roll && !f.scan).cloned().collect::<Vec<_>>();
    let frames: Vec<Frame> = slow_frames.into_iter().chain(queued).collect();
    let trace = u.borrow().trace_text();
    (trace, frames, mask)
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
        let (trace, frames, mask) = run_case(case);
        if let Some(d) = &dir {
            let mut text = format!("# {}\n", case.name);
            text += &trace;
            for f in &frames {
                text += &frame_line(f);
                text.push('\n');
            }
            std::fs::write(d.join(format!("{}.txt", case.name)), text).expect("write the case's file");
        }
        let count = |what: &str| trace.lines().filter(|l| l.starts_with(what)).count();
        let (played, streamed) = (count("Played"), count("Stream"));
        // roll or scan from the case's settings, not from what came out: a slow case that delivers nothing fails
        let slow = {
            let mut m = Model::default();
            (case.model)(&mut m);
            super::slow_mode(&m) != crate::board::Slow::Off
        };
        if slow {
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
            if frames[0].scan {
                assert!(frames.iter().any(|f| !f.partial), "{}: no sweep completed", case.name);
                let sweeps = trace.lines().filter(|l| l.contains("counter: 1,")).count();
                assert!(sweeps >= 2, "{}: {} sweeps started", case.name, sweeps);
            }
        } else if case.period_us.is_some() {
            assert!(played > 0 && !frames.is_empty(), "{}: played {}, {} frames", case.name, played, frames.len());
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
