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
}

/// One case of the matrix: the model as the screen sets it, the frequency of the scene's sine, the trigger period of
/// the scene (None: it never triggers), the passes of the loop, and what the screen does at which pass.
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
];

/// The scene's signal on every channel: a 0.5 V sine of `f` Hz, or the square of its sign, rising through 0 at every
/// trigger instant (they are whole periods apart).
fn signal(f: f64, square: bool) -> Box<dyn Fn(usize, f64) -> f64> {
    Box::new(move |_c, t| {
        let x = (2.0 * std::f64::consts::PI * f * t).sin();
        0.5 * if square { x.signum() } else { x }
    })
}

/// One line of a frame: its fields and, per channel with points, the count, the extremes and the mean (to 0.1 mV).
fn frame_line(f: &Frame) -> String {
    let mut s =
        format!("frame {} dt {:e} pre {} env {} roll {} scan {}", f.seq, f.dt, f.pre, f.envelope, f.roll, f.scan);
    for (c, v) in f.ch.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        let mean = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
        s += &format!(" | CH{} n {} lo {:.4} hi {:.4} mean {:.4}", c + 1, v.len(), lo, hi, mean);
    }
    s
}

/// Runs `case` and returns the unit's trace and the frames delivered.
fn run_case(case: &Case) -> (String, Vec<Frame>) {
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
    out.lock().unwrap().want = Some(want);
    let mut pass = 0;
    let shared = out.clone();
    super::run(hw, super::settings(&m, false).mask, false, "/nonexistent".into(), out.clone(), || {
        pass += 1;
        for (_, act) in case.script.iter().filter(|(at, _)| *at == pass) {
            let mut g = shared.lock().unwrap();
            match act {
                Act::Set(f) => {
                    f(&mut m);
                    g.want = Some(super::settings(&m, false));
                }
                Act::Hold => g.hold = true,
            }
        }
        pass <= case.passes
    });
    let frames: Vec<Frame> = out.lock().unwrap().queue.iter().cloned().collect();
    let trace = u.borrow().trace_text();
    (trace, frames)
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
        let (trace, frames) = run_case(case);
        if let Some(d) = &dir {
            let mut text = format!("# {}\n", case.name);
            text += &trace;
            for f in &frames {
                text += &frame_line(f);
                text.push('\n');
            }
            std::fs::write(d.join(format!("{}.txt", case.name)), text).expect("write the case's file");
        }
        let played = trace.lines().filter(|l| l.starts_with("Played")).count();
        if case.period_us.is_some() {
            assert!(played > 0 && !frames.is_empty(), "{}: played {}, {} frames", case.name, played, frames.len());
            for f in &frames {
                for v in f.ch.iter().filter(|v| !v.is_empty()) {
                    let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                    assert!((hi - 0.5).abs() < 0.01 && (lo + 0.5).abs() < 0.01, "{}: {} to {} V", case.name, lo, hi);
                }
            }
        } else {
            assert!(frames.is_empty() && played == 0, "{}: {} frames without a trigger", case.name, frames.len());
        }
    }
}
