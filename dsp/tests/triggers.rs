// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Every trigger type against a synthetic signal with a known answer.

use oos_dsp::sim::{sample, video, video_line_time, Pulse, PulseTrain, VideoStd};
use oos_dsp::trigger::*;

const E: [f32; 0] = [];

fn inp1<'a>(v: &'a [f32], dt: f64, hyst: f32) -> Input<'a> {
    Input { ch: [v, &E, &E, &E], dt, hyst: [hyst; 4] }
}

fn near(t: f64, want_s: f64, dt: f64, tol_samples: f64) -> bool {
    (t * dt - want_s).abs() <= tol_samples * dt
}

#[test]
fn edge() {
    let dt = 1e-6;
    let v = sample(|t| PulseTrain::square(1e3, 0.0, 2.0, 1e-6).v(t), 0.0, dt, 10_000);
    let i = inp1(&v, dt, 0.1);
    let r = find_all(&Trigger::Edge { src: 0, slope: Slope::Rising, level: 1.0 }, &i);
    assert_eq!(r.len(), 10);
    assert!(near(r[3], 3e-3 + 0.5e-6, dt, 0.1));
    assert_eq!(find_all(&Trigger::Edge { src: 0, slope: Slope::Either, level: 1.0 }, &i).len(), 20);
}

#[test]
fn pulse_width_glitch() {
    let dt = 10e-9;
    let p = PulseTrain { period: 100e-6, width: 50e-6, every: 5, at: 2, special: Pulse { width: 0.2e-6, height: 1.0 }, ..PulseTrain::square(1e4, 0.0, 1.0, 10e-9) };
    let v = sample(|t| p.v(t), 0.0, dt, 100_000);
    let i = inp1(&v, dt, 0.05);
    let g = find_all(&Trigger::Pulse { src: 0, pol: Pol::Pos, level: 0.5, when: When::Lt(1e-6) }, &i);
    assert_eq!(g.len(), 2);
    assert!(near(g[0], 200e-6 + 0.2e-6 + 5e-9, dt, 1.0));
    assert_eq!(find_all(&Trigger::Pulse { src: 0, pol: Pol::Pos, level: 0.5, when: When::Gt(10e-6) }, &i).len(), 8);
    // negative pulses (the gaps) are 50 us or ~100 us around the glitch: none shorter than 1 us
    assert_eq!(find_all(&Trigger::Pulse { src: 0, pol: Pol::Neg, level: 0.5, when: When::Lt(1e-6) }, &i).len(), 0);
}

#[test]
fn slope_transition_time() {
    // rising edges alternate 100 ns / 1 us (0..1 V), falling 100 ns; period 20 us
    let dt = 10e-9;
    let f = |t: f64| {
        let k = (t / 20e-6).floor() as i64;
        let u = t - k as f64 * 20e-6;
        let tr = if k % 2 == 0 { 100e-9 } else { 1e-6 };
        if u < 10e-6 {
            (u / tr).min(1.0)
        } else {
            (1.0 - (u - 10e-6) / 100e-9).max(0.0)
        }
    };
    let v = sample(f, 0.0, dt, 20_000);
    let i = inp1(&v, dt, 0.02);
    // 20 % .. 80 % takes 60 ns or 600 ns
    assert_eq!(find_all(&Trigger::Slope { src: 0, slope: Slope::Rising, lo: 0.2, hi: 0.8, when: When::Gt(300e-9) }, &i).len(), 5);
    assert_eq!(find_all(&Trigger::Slope { src: 0, slope: Slope::Rising, lo: 0.2, hi: 0.8, when: When::Lt(300e-9) }, &i).len(), 5);
    assert_eq!(find_all(&Trigger::Slope { src: 0, slope: Slope::Falling, lo: 0.2, hi: 0.8, when: When::Lt(300e-9) }, &i).len(), 10);
}

#[test]
fn runt() {
    let dt = 100e-9;
    let p = PulseTrain { every: 4, at: 2, special: Pulse { width: 50e-6, height: 0.5 }, ..PulseTrain::square(1e4, 0.0, 2.0, 100e-9) };
    let v = sample(|t| p.v(t), 0.0, dt, 10_000);
    let i = inp1(&v, dt, 0.05);
    let r = find_all(&Trigger::Runt { src: 0, pol: Pol::Pos, lo: 0.5, hi: 1.5, when: None }, &i);
    assert_eq!(r.len(), 2);
    assert!(near(r[0], 200e-6 + 50e-6 + 25e-9, dt, 1.0));
    assert_eq!(find_all(&Trigger::Runt { src: 0, pol: Pol::Pos, lo: 0.5, hi: 1.5, when: Some(When::Gt(60e-6)) }, &i).len(), 0);
}

#[test]
fn window() {
    let dt = 1e-6;
    let v = sample(|t| (2.0 * std::f64::consts::PI * 1e3 * t).sin(), 0.0, dt, 5000);
    let i = inp1(&v, dt, 0.02);
    let w = |when| find_all(&Trigger::Window { src: 0, lo: -0.5, hi: 0.5, when }, &i).len();
    assert_eq!(w(WinWhen::Enter), 10);
    assert_eq!(w(WinWhen::Exit), 10);
    // inside for 166.7 us each time
    assert_eq!(w(WinWhen::Inside(When::Lt(200e-6))), 9);
    assert_eq!(w(WinWhen::Inside(When::Gt(200e-6))), 0);
}

#[test]
fn timeout_missing_pulse() {
    let dt = 1e-6;
    let p = PulseTrain { every: 6, at: 3, special: Pulse { width: 0.5e-3, height: 0.0 }, ..PulseTrain::square(1e3, 0.0, 1.0, 1e-6) };
    let v = sample(|t| p.v(t), 0.0, dt, 12_000);
    let i = inp1(&v, dt, 0.05);
    let r = find_all(&Trigger::Timeout { src: 0, slope: Slope::Rising, level: 0.5, time: 1.5e-3 }, &i);
    assert_eq!(r.len(), 2);
    assert!(near(r[0], 2e-3 + 0.5e-6 + 1.5e-3, dt, 0.1));
    assert!(near(r[1], 8e-3 + 0.5e-6 + 1.5e-3, dt, 0.1));
}

#[test]
fn delay_between_channels() {
    let dt = 1e-6;
    let a = sample(|t| PulseTrain::square(1e3, 0.0, 1.0, 1e-6).v(t), 0.0, dt, 10_000);
    let b = sample(|t| PulseTrain { offset: 100e-6, ..PulseTrain::square(1e3, 0.0, 1.0, 1e-6) }.v(t), 0.0, dt, 10_000);
    let i = Input { ch: [&a, &b, &E, &E], dt, hyst: [0.05; 4] };
    let d = |when| find_all(&Trigger::Delay { a: 0, slope_a: Slope::Rising, level_a: 0.5, b: 1, slope_b: Slope::Rising, level_b: 0.5, when }, &i);
    let r = d(When::In(90e-6, 110e-6));
    assert_eq!(r.len(), 10);
    assert!(near(r[0], 100e-6 + 0.5e-6, dt, 0.1));
    assert_eq!(d(When::Lt(50e-6)).len(), 0);
}

#[test]
fn setup_hold_violation() {
    let dt = 1e-9;
    let clk = sample(|t| PulseTrain::square(1e6, 0.0, 1.0, 1e-9).v(t), 0.0, dt, 20_000);
    // data toggles at 500 ns after each clock edge, except: 5 ns before the edge at 6 us, 3 ns after the edge at 13 us
    let mut tr: Vec<f64> = (0..20).map(|k| k as f64 * 1e-6 + 500e-9).collect();
    tr[5] = 6e-6 - 5e-9;
    tr[12] = 13e-6 + 3e-9;
    let data = sample(
        |t| {
            let mut x = 0.0;
            for &s in &tr {
                let r = ((t - s) / 1e-9).clamp(0.0, 1.0);
                x = if (x as i32) % 2 == 0 { x + r } else { x - r };
                if r < 1.0 {
                    break;
                }
            }
            x.rem_euclid(2.0).min(1.0)
        },
        0.0,
        dt,
        20_000,
    );
    let i = Input { ch: [&data, &clk, &E, &E], dt, hyst: [0.05; 4] };
    let r = find_all(&Trigger::SetupHold { data: 0, clk: 1, clk_slope: Slope::Rising, data_level: 0.5, clk_level: 0.5, setup: 10e-9, hold: 10e-9 }, &i);
    assert_eq!(r.len(), 2, "{:?}", r.iter().map(|t| t * dt).collect::<Vec<_>>());
    assert!(near(r[0], 6e-6 + 0.5e-9, dt, 1.0)); // setup violation: at the clock edge
    assert!(near(r[1], 13e-6 + 3.5e-9, dt, 1.0)); // hold violation: at the late data edge
}

#[test]
fn nth_edge_in_bursts() {
    // bursts of 5 pulses (1 us period) every 15 us, record starts in the idle gap
    let dt = 10e-9;
    let f = |t: f64| {
        let u = (t - 7e-6).rem_euclid(15e-6);
        if u < 5e-6 && u.rem_euclid(1e-6) < 0.5e-6 {
            1.0
        } else {
            0.0
        }
    };
    let v = sample(f, 0.0, dt, 6000);
    let i = inp1(&v, dt, 0.05);
    let r = find_all(&Trigger::NthEdge { src: 0, slope: Slope::Rising, level: 0.5, idle: 5e-6, n: 3 }, &i);
    assert_eq!(r.len(), 4);
    assert!(near(r[0], 7e-6 + 2e-6, dt, 1.0));
}

#[test]
fn pattern_and_duration() {
    let dt = 1e-6;
    let a = sample(|t| PulseTrain::square(1e3, 0.0, 1.0, 1e-6).v(t), 0.0, dt, 10_000);
    let b = sample(|t| PulseTrain { offset: 0.1e-3, ..PulseTrain::square(2e3, 0.0, 1.0, 1e-6) }.v(t), 0.0, dt, 10_000);
    let i = Input { ch: [&a, &b, &E, &E], dt, hyst: [0.05; 4] };
    let lv = [0.5; 4];
    let hh = find_all(&Trigger::Pattern { bits: [Bit::H, Bit::H, Bit::X, Bit::X], levels: lv }, &i);
    assert_eq!(hh.len(), 10);
    assert!(near(hh[0], 0.1e-3 + 0.5e-6, dt, 0.2));
    let hr = find_all(&Trigger::Pattern { bits: [Bit::H, Bit::R, Bit::X, Bit::X], levels: lv }, &i);
    assert_eq!(hr.len(), 10);
    // duration: CH1 high for 300 us or 700 us alternately
    let p = PulseTrain { width: 300e-6, every: 2, at: 1, special: Pulse { width: 700e-6, height: 1.0 }, ..PulseTrain::square(1e3, 0.0, 1.0, 1e-6) };
    let c = sample(|t| p.v(t), 0.0, dt, 10_000);
    let i = Input { ch: [&c, &E, &E, &E], dt, hyst: [0.05; 4] };
    let d = find_all(&Trigger::Duration { bits: [Bit::H, Bit::X, Bit::X, Bit::X], levels: lv, when: When::Gt(500e-6) }, &i);
    assert_eq!(d.len(), 5);
    assert!(near(d[0], 1.7e-3 + 0.5e-6, dt, 0.2));
}

#[test]
fn video_pal() {
    let dt = 0.25e-6;
    let std = VideoStd::Pal;
    let t0 = -0.5e-3;
    let v = sample(|t| video(std, t), t0, dt, 162_000); // -0.5 ms .. 40 ms: one frame
    let i = inp1(&v, dt, 0.05);
    let run = |sync| find_all(&Trigger::Video { src: 0, std, sync, level: 0.15 }, &i);
    let at = |t: f64| (t - t0) / dt;
    let l = std.line();
    let fields = run(VideoSync::AllFields);
    assert_eq!(fields.len(), 2, "{:?}", fields);
    assert!((fields[0] - at(2.5 * l)).abs() < 1.0);
    assert!((fields[1] - at(20e-3 + 2.5 * l)).abs() < 1.0);
    let odd = run(VideoSync::OddField);
    assert_eq!(odd.len(), 1);
    assert!((odd[0] - fields[0]).abs() < 0.01);
    let even = run(VideoSync::EvenField);
    assert_eq!(even.len(), 1);
    assert!((even[0] - fields[1]).abs() < 0.01);
    let l10 = run(VideoSync::Line(10));
    assert_eq!(l10.len(), 2);
    assert!((l10[0] - at(video_line_time(std, 1, 10))).abs() < 1.0);
    assert!((l10[1] - at(video_line_time(std, 2, 10))).abs() < 1.0);
    let all = run(VideoSync::AllLines).len();
    eprintln!("all lines {}", all);
    assert!((612..=620).contains(&all), "{}", all);
}
