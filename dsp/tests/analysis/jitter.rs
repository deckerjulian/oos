// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The eye and jitter analysis against PRBS and clock signals with known jitter: eye opening, RJ / DJ / TJ, pooled
//! records, the bathtub, DCD and DDJ, PJ lines, BER, SSC and phase noise. A new eye or jitter test goes here.

use super::*;

// ------------------------------------------------------------------------------------------------ eye / jitter

/// NRZ 0/1 V waveform of `bits` at `rate`, `spu` samples per UI, linear transitions of `tr` UI, each edge displaced by
/// `jit(k, t)` seconds (k = UI number). Returns (samples, dt).
fn nrz(bits: &[bool], rate: f64, spu: usize, tr: f64, jit: &mut dyn FnMut(usize, f64) -> f64) -> (Vec<f32>, f64) {
    let ui = 1.0 / rate;
    let mut l = Logic::new(0.0, 1.0, tr * ui);
    l.push(0.0, bits[0]);
    for k in 1..bits.len() {
        if bits[k] != bits[k - 1] {
            let t = k as f64 * ui;
            // the logic level changes at the start of the ramp: centre the ramp on the edge time
            l.push(t + jit(k, t) - tr * ui / 2.0, bits[k]);
        }
    }
    let dt = ui / spu as f64;
    (sample(|t| l.v(t), 0.0, dt, bits.len() * spu), dt)
}

#[test]
fn eye_ideal_prbs7_is_open_without_jitter() {
    let rate = 1e6;
    let bits = prbs7(4000);
    let (v, dt) = nrz(&bits, rate, 32, 0.25, &mut |_, _| 0.0);
    let c = eye::recover_clock(&v, dt, 0.5, 0.1, None).unwrap();
    assert!(close(c.period, 1e-6, 1e-12), "{}", c.period);
    let t = eye::tie(&c.edges, &c);
    assert!(t.iter().all(|x| x.abs() < 1e-9), "{:?}", &t[..5]);
    let j = eye::jitter(&t);
    assert!(j.rj_rms < 1e-9 && j.dj_pp < 1e-9 && j.pp < 2e-9, "{:?}", j);
    let mut e = Eye::new(256, 256, 2, -0.25, 1.25);
    e.accumulate(&v, dt, &c);
    let m = e.measure(c.period);
    assert!(close(m.amplitude, 1.0, 0.02) && close(m.height, 1.0, 0.03), "{:?}", m);
    assert!(m.width > 0.95e-6 && m.width <= 1e-6, "{:?}", m);
    assert!(close(m.crossing_pct, 50.0, 3.0), "{:?}", m);
    assert!(close(m.rise, 0.15e-6, 0.02e-6) && close(m.fall, 0.15e-6, 0.02e-6), "{:?}", m);
    assert!(m.q_factor > 20.0, "{:?}", m);
    // the eye centre is empty, the crossing is not
    assert_eq!(e.at(128, 128), 0);
    assert!(e.at(64, 128) > 0);
    e.clear();
    assert!(e.hits.iter().all(|&h| h == 0));
}

#[test]
fn eye_sparse_record_still_draws_lines() {
    // 4 samples per UI: the interpolation fills the transitions
    let bits = prbs7(2000);
    let (v, dt) = nrz(&bits, 1e6, 4, 0.5, &mut |_, _| 0.0);
    let c = eye::recover_clock(&v, dt, 0.5, 0.1, Some(1e6)).unwrap();
    let mut e = Eye::new(128, 64, 2, -0.25, 1.25);
    e.accumulate(&v, dt, &c);
    let filled = (0..128).filter(|&x| (0..64).any(|y| e.at(x, y) > 0)).count();
    assert_eq!(filled, 128);
    assert!(close(e.eye_amplitude(), 1.0, 0.05));
}

/// A source-synchronous strobe (DQS) clocks the data (DQ). The eye is DQ folded on DQS edges, not CDR from DQ.
#[test]
fn eye_folds_data_on_a_strobe_clock() {
    let (rate, spu, nui) = (1e9, 32usize, 2048usize);
    let ui = 1.0 / rate;
    let dt = ui / spu as f64;
    let bits = prbs7(nui);
    let mut dq = Vec::with_capacity(nui * spu);
    let mut dqs = Vec::with_capacity(nui * spu);
    for i in 0..nui * spu {
        let t = i as f64 * dt;
        let k = ((t + 0.5 * ui) / ui).floor() as usize;
        dq.push(if bits[k.min(bits.len() - 1)] { 1.0 } else { 0.0 });
        dqs.push(if ((t / ui).floor() as usize).is_multiple_of(2) { 1.0 } else { 0.0 });
    }
    let clk = eye::recover_clock(&dqs, dt, 0.5, 0.1, None).unwrap();
    assert!(close(clk.period, ui, ui * 1e-6), "{}", clk.period);
    let mut e = Eye::new(128, 64, 2, -0.25, 1.25);
    e.accumulate(&dq, dt, &eye::center_strobe(&clk));
    let m = e.measure(clk.period);
    assert!(m.height > 0.7, "{:?}", m);
    assert!(m.width > 0.5 * ui, "{:?}", m);
    let idle = eye::gate_around_clock(&dq, dt, &clk, 1.5);
    assert_eq!(idle.iter().filter(|x| x.is_finite()).count(), dq.len());
    // an empty record against a clock is an empty result, not a panic
    assert!(eye::gate_around_clock(&[], dt, &clk, 1.5).is_empty());
}

#[test]
fn jitter_gaussian_edge_jitter_gives_rj() {
    let rate = 1e6;
    let sigma = 0.02e-6;
    let mut g = Gauss(5);
    let bits = prbs7(30_000);
    let (v, dt) = nrz(&bits, rate, 16, 0.2, &mut |_, _| g.next(sigma));
    let c = eye::recover_clock(&v, dt, 0.5, 0.1, None).unwrap();
    assert!(close(c.period, 1e-6, 1e-10));
    let j = eye::jitter(&eye::tie(&c.edges, &c));
    assert!(close(j.rj_rms, sigma, 0.15 * sigma), "{:?}", j);
    assert!(close(j.rms, sigma, 0.05 * sigma), "{:?}", j);
    assert!(j.dj_pp < 1.0 * sigma, "{:?}", j);
    assert!(close(j.period_jitter_rms, 2f64.sqrt() * sigma, 0.15 * sigma), "{:?}", j);
    assert!(close(j.c2c_rms, 6f64.sqrt() * sigma, 0.3 * sigma), "{:?}", j);
    assert!(close(j.tj_pp, j.dj_pp + 14.068 * j.rj_rms, 1e-12));
    assert_eq!(j.histogram.iter().map(|h| h.1).sum::<u32>() as usize, c.edges.len());
}

#[test]
fn jitter_sinusoidal_jitter_gives_dj_and_a_spectral_line() {
    let rate = 1e6;
    let (a, f_sj) = (0.1e-6, rate / 97.3);
    let mut g = Gauss(9);
    let bits = prbs7(30_000);
    let (v, dt) = nrz(&bits, rate, 16, 0.2, &mut |_, t| a * (2.0 * PI * f_sj * t).sin() + g.next(0.005e-6));
    let c = eye::recover_clock(&v, dt, 0.5, 0.1, None).unwrap();
    let t = eye::tie(&c.edges, &c);
    let j = eye::jitter(&t);
    assert!(close(j.dj_pp, 2.0 * a, 0.15 * 2.0 * a), "{:?}", j);
    assert!(j.rj_rms < 0.02e-6, "{:?}", j);
    let tr = eye::tie_trend(&c.edges, &t, &c);
    assert!(tr.len() > 29_000);
    let (df, s) = eye::tie_spectrum(&tr, c.period);
    let k = (1..s.len()).max_by(|&x, &y| s[x].total_cmp(&s[y])).unwrap();
    assert!(close(k as f64 * df, f_sj, 2.0 * df), "{} {}", k as f64 * df, f_sj);
    assert!(close(s[k] * 2f64.sqrt(), a, 0.2 * a), "{}", s[k]);
}

#[test]
fn jitter_pll_tracks_slow_wander() {
    let rate = 1e6;
    let (a, f_w) = (0.3e-6, rate / 10_000.0);
    let bits = prbs7(30_000);
    let (v, dt) = nrz(&bits, rate, 8, 0.2, &mut |_, t| a * (2.0 * PI * f_w * t).sin());
    let c = eye::recover_clock(&v, dt, 0.5, 0.1, Some(rate)).unwrap();
    let p = eye::recover_clock_pll(&v, dt, 0.5, 0.1, Some(rate), 0.0, false).unwrap();
    let p2 = eye::recover_clock_pll(&v, dt, 0.5, 0.1, Some(rate), 0.0, true).unwrap();
    let pp = |c: &eye::Clock| eye::jitter(&eye::tie(&c.edges, c)).pp;
    assert!(pp(&p) < 0.35 * pp(&c), "{} {}", pp(&p), pp(&c));
    assert!(pp(&p2) < 0.35 * pp(&c), "{} {}", pp(&p2), pp(&c));
    assert!(p.ticks.len() >= 30_000);
    // the PLL eye stays open
    let mut e = Eye::new(128, 64, 1, -0.25, 1.25);
    e.accumulate(&v, dt, &p);
    assert!(e.eye_width(p.period) > 0.5e-6, "{}", e.eye_width(p.period));
}

/// Square of 0/1 V edges at `rate` UI/s with ramps of `tr` s, quantised to `lsb`, `n` samples of `dt`.
fn clean_square(rate: f64, tr: f64, dt: f64, n: usize, lsb: f64, noise: f64, g: &mut Gauss) -> Vec<f32> {
    let ui = 1.0 / rate;
    (0..n)
        .map(|k| {
            let t = k as f64 * dt + 0.13 * ui;
            let x = (t / ui).rem_euclid(2.0) * ui; // time in the period, edges at 0 and ui
            let up = |d: f64| (d / tr + 0.5).clamp(0.0, 1.0);
            let y = if x < 0.5 * ui {
                up(x)
            } else if x < 1.5 * ui {
                1.0 - up(x - ui)
            } else {
                up(x - 2.0 * ui)
            };
            (((y + g.next(noise)) / lsb).round() * lsb) as f32
        })
        .collect()
}

/// A clock starts each record on the same polarity when triggered: the eye still has both levels and its crossings.
#[test]
fn eye_of_a_clock_has_both_levels_and_crossings() {
    let mut g = Gauss(17);
    let dt = 8e-10;
    let v = clean_square(10e6, 25e-9, dt, 1250, 1e-4, 1e-3, &mut g);
    let c = eye::recover_clock(&v, dt, 0.5, 0.1, None).unwrap();
    let mut e = Eye::new(256, 128, 2, -0.5, 1.5);
    for _ in 0..4 {
        e.accumulate(&v, dt, &c);
    }
    let m = e.measure(c.period);
    assert!(close(m.amplitude, 1.0, 0.03), "{:?}", m);
    assert!(close(m.crossing_pct, 50.0, 5.0), "{:?}", m);
    assert!(m.width > 0.9e-7 && m.rise.is_finite() && m.fall.is_finite(), "{:?}", m);
    // both UI of the span look alike, the eye is open in each
    assert_eq!(e.at(128, 64), 0);
    assert_eq!(e.at(0, 32), e.at(128, 32));
}

/// The reason RJ / DJ / TJ stayed empty on a 5 MHz square: too few edges in the window, and tails without spread.
#[test]
fn jitter_of_a_clean_square_is_never_left_empty() {
    use oos_dsp::eye::JitterFit as F;
    let mut g = Gauss(7);
    // 10 edges: a limit from the RMS, no tail fit
    let v = clean_square(10e6, 3e-9, 8e-10, 1300, 1e-4, 1e-3, &mut g);
    let c = eye::recover_clock(&v, 8e-10, 0.5, 0.1, None).unwrap();
    assert!(c.edges.len() >= 8 && c.edges.len() < 20, "{}", c.edges.len());
    let j = eye::jitter(&eye::tie(&c.edges, &c));
    assert_eq!(j.fit, F::Limit);
    assert!(close(j.rj_rms, j.rms, 1e-18) && j.dj_pp.is_nan(), "{:?}", j);
    assert!(close(j.tj_pp, 2.0 * (1.0 + eye::Q_BER_1E12.powi(2)).sqrt() * j.rms, 1e-18), "{:?}", j);
    // every edge on the same sample phase with a coarse ADC: the same TIE on each, a few values at most
    let v = clean_square(10e6, 3e-9, 8e-10, 25_000, 0.03, 1e-3, &mut g);
    let c = eye::recover_clock(&v, 8e-10, 0.5, 0.1, None).unwrap();
    let j = eye::jitter(&eye::tie(&c.edges, &c));
    assert!(j.rj_rms.is_finite() && j.tj_pp.is_finite(), "{:?}", j);
    assert_ne!(j.fit, F::TooFew);
    // without noise the TIE is rounding of the times: no jitter at all
    let v = clean_square(10e6, 3e-9, 8e-10, 25_000, 1e-6, 0.0, &mut g);
    let c = eye::recover_clock(&v, 8e-10, 0.5, 0.1, None).unwrap();
    let j = eye::jitter(&eye::tie(&c.edges, &c));
    assert_eq!((j.fit, j.rj_rms, j.dj_pp, j.tj_pp), (F::Quiet, 0.0, 0.0, 0.0), "{:?}", j);
    // too few edges for a model: says so
    let j = eye::jitter(&[1e-12, -1e-12, 2e-12]);
    assert_eq!(j.fit, F::TooFew);
    assert!(j.tj_pp.is_nan() && j.pp > 0.0);
}

/// Few edges per record add up: TIE pooled over records reaches the tail fit and finds the sigma.
#[test]
fn jitter_pooled_over_records_recovers_rj() {
    use oos_dsp::eye::JitterFit as F;
    let mut g = Gauss(11);
    let sigma = 5e-12;
    let pool: Vec<f64> = (0..16384).map(|_| g.next(sigma)).collect();
    let j = eye::jitter(&pool);
    assert_eq!(j.fit, F::Tails);
    assert!(close(j.rj_rms, sigma, 0.15 * sigma), "{:?}", j.rj_rms);
    assert!(j.dj_pp < 1.5 * sigma, "{:?}", j.dj_pp);
    // dual Dirac: +-50 ps with the same RJ
    let pool: Vec<f64> = (0..16384).map(|k| if k % 2 == 0 { 50e-12 } else { -50e-12 } + g.next(sigma)).collect();
    let j = eye::jitter(&pool);
    assert_eq!(j.fit, F::Tails);
    assert!(close(j.dj_pp, 100e-12, 10e-12), "{:?}", j.dj_pp);
    assert!(close(j.rj_rms, sigma, 0.25 * sigma), "{:?}", j.rj_rms);
}

#[test]
fn q_tail_matches_known_points() {
    assert!(close(eye::q_tail(0.0), 0.5, 1e-6));
    assert!(close(eye::q_tail(1.0), 0.158655, 1e-5));
    assert!(close(eye::q_tail(-1.0), 0.841345, 1e-5));
    assert!(close(eye::q_tail(eye::Q_BER_1E12) / 1e-12, 1.0, 0.01), "{}", eye::q_tail(eye::Q_BER_1E12));
    assert!(eye::q_tail(50.0) >= 0.0);
}

#[test]
fn bathtub_opening_is_one_ui_less_tj() {
    let mut g = Gauss(3);
    let period = 1e-9;
    let mut s: Vec<f64> = (0..16384).map(|k| if k % 2 == 0 { 20e-12 } else { -20e-12 } + g.next(4e-12)).collect();
    let j = eye::jitter(&s);
    s.sort_by(f64::total_cmp);
    let b = eye::bathtub(&s, &j, period, 201);
    assert!(close(b.opening, 1.0 - j.tj_pp / period, 1e-12));
    assert!(close(b.seen, 1.0 - j.pp / period, 1e-12));
    assert_eq!((b.ui.len(), b.measured.len(), b.model.len()), (201, 201, 201));
    // the model curve crosses 1e-12 at the edges of the opening
    let first = b.model.iter().position(|&x| x < -12.0).unwrap();
    let last = b.model.iter().rposition(|&x| x < -12.0).unwrap();
    let width = (b.ui[last] - b.ui[first]) as f64;
    assert!(close(width, b.opening, 0.02), "{} {}", width, b.opening);
    // the walls are at about one half, the floor of the count is one edge
    assert!(b.measured[0] > -0.5 && b.measured[200] > -0.5, "{:?}", (b.measured[0], b.measured[200]));
    assert!(b.measured[100].is_nan());
    assert!(b.measured.iter().filter(|x| x.is_finite()).all(|&x| x >= -(16384f32.log10()) - 0.01));
    // the curve falls towards the middle
    assert!(b.model[100] <= b.model[50] && b.model[50] <= b.model[10], "{:?}", &b.model[..3]);
}

#[test]
fn bathtub_without_a_model_has_no_curve_and_no_opening() {
    let s = [1e-12, 2e-12, 3e-12];
    let j = eye::jitter(&s);
    let b = eye::bathtub(&s, &j, 1e-9, 21);
    assert!(b.model.is_empty() && b.opening.is_nan());
    assert!(close(b.seen, 0.998, 1e-9));
    assert!(eye::bathtub(&[], &j, 1e-9, 21).ui.is_empty());
    assert!(eye::bathtub(&s, &j, 0.0, 21).ui.is_empty());
    // no jitter at all: the tub is square and open
    let q = [0.0; 32];
    let b = eye::bathtub(&q, &eye::jitter(&q), 1e-9, 21);
    assert!(close(b.opening, 1.0, 1e-12));
    assert!(b.model[10] <= eye::BATHTUB_FLOOR);
}

#[test]
fn dcd_and_ddj_come_from_the_edge_means() {
    let mut g = Gauss(21);
    // a clock with late rising edges: DCD, and only one run length, so no DDJ
    let period = 1e-8;
    let e: Vec<f64> = (0..2000).map(|k| k as f64 * period).collect();
    let t: Vec<f64> = (0..2000).map(|k| if k % 2 == 0 { 30e-12 } else { -30e-12 } + g.next(5e-12)).collect();
    let c = eye::Clock { period, edges: e, first_rising: true, ..Default::default() };
    let mut m = eye::EdgeMeans::default();
    m.add(&t, &c);
    assert!(close(m.dcd(), 60e-12, 2e-12), "{}", m.dcd());
    assert!(m.ddj().is_nan());
    // the other polarity turns the sign
    let c2 = eye::Clock { first_rising: false, ..c.clone() };
    let mut m = eye::EdgeMeans::default();
    m.add(&t, &c2);
    assert!(close(m.dcd(), -60e-12, 2e-12), "{}", m.dcd());
    // edges after a long run come 40 ps early
    let (mut e, mut t, mut at) = (Vec::new(), Vec::new(), 0.0);
    for k in 0..4000 {
        let long = k % 3 == 0;
        at += if long { 3.0 } else { 1.0 } * period;
        e.push(at);
        t.push(if long { -40e-12 } else { 0.0 } + g.next(5e-12));
    }
    let c = eye::Clock { period, edges: e, first_rising: true, ..Default::default() };
    let mut m = eye::EdgeMeans::default();
    m.add(&t, &c);
    assert!(close(m.ddj(), 40e-12, 3e-12), "{}", m.ddj());
    // too few edges say nothing
    let mut m = eye::EdgeMeans::default();
    m.add(&t[..20], &c);
    assert!(m.dcd().is_nan() && m.ddj().is_nan());
}

#[test]
fn pj_lines_finds_the_sine_and_not_the_noise() {
    let period = 1e-8;
    let mut g = Gauss(31);
    let n = 8192;
    let f = 1.0 / (period * 64.0);
    let a = 20e-12;
    let tr: Vec<f32> = (0..n).map(|k| (a * (2.0 * PI * f * k as f64 * period).sin() + g.next(5e-12)) as f32).collect();
    let (df, s) = eye::tie_spectrum(&tr, period);
    let l = eye::pj_lines(df, &s, 8);
    assert_eq!(l.len(), 1, "{:?}", l);
    assert!(close(l[0].0, f, 2.0 * df) && close(l[0].1, a / 2f64.sqrt(), 0.1 * a), "{:?}", l);
    let tr: Vec<f32> = (0..n).map(|_| g.next(5e-12) as f32).collect();
    let (df, s) = eye::tie_spectrum(&tr, period);
    assert!(eye::pj_lines(df, &s, 8).is_empty());
    assert!(eye::pj_lines(1.0, &[1.0, 2.0], 8).is_empty());
}

#[test]
fn eye_and_jitter_bad_input() {
    assert!(eye::recover_clock(&[], 1e-9, 0.5, 0.1, None).is_none());
    assert!(eye::recover_clock(&[f32::NAN; 50], 1e-9, 0.5, 0.1, None).is_none());
    let j = eye::jitter(&[]);
    assert!(j.rj_rms.is_nan());
    let j = eye::jitter(&[1e-12, 2e-12]);
    assert!(j.pp > 0.0 && j.rj_rms.is_nan());
    let mut e = Eye::new(0, 0, 0, 0.0, 1.0);
    e.accumulate(&[0.0, 1.0], 1e-9, &eye::Clock::default());
    assert!(e.measure(1e-9).one.is_nan());
    assert_eq!(eye::unit_interval(&[3.0, 6.0, 9.0, 6.0, 3.0]), Some(3.0));
    // no single-unit spacing: 2 and 3 units of 1.0
    assert!(close(eye::unit_interval(&[2.0, 3.0, 2.0, 3.0, 3.0, 2.0]).unwrap(), 1.0, 1e-9));
}

#[test]
fn eye_ber_subrate_common_mode_and_ssc() {
    use oos_dsp::eye::{self, Clock, EyeMeas, Jitter};
    let m = EyeMeas { one: 1.0, zero: 0.0, sigma_one: 0.01, sigma_zero: 0.01, ..Default::default() };
    let q = eye::norm_inv(1.0 - 1e-12);
    let want = (1.0 - q * 0.01) - q * 0.01;
    assert!(close(m.height_at(1e-12), want, 1e-9), "{}", m.height_at(1e-12));
    let closed = EyeMeas { sigma_one: 1.0, sigma_zero: 1.0, ..m };
    assert_eq!(closed.height_at(1e-12), 0.0);
    let j = Jitter { dj_pp: 1e-12, rj_rms: 1e-12, ..Default::default() };
    let j2 = eye::tj_at(&j, eye::BER_J2);
    let j9 = eye::tj_at(&j, eye::BER_J9);
    assert!(j2 < j9 && close(eye::tj_at(&j, 1e-12), 1e-12 + 2.0 * q * 1e-12, 1e-18), "{} {}", j2, j9);
    assert!(close(eye::eye_width_at(&j, 1e-9, 1e-12), (1.0 - eye::tj_at(&j, 1e-12) / 1e-9).max(0.0), 1e-12));

    let period = 1e-9;
    let trend: Vec<f32> = (0..256).map(|k| (1e-12 * (PI * k as f64).cos()) as f32).collect();
    assert!(close(eye::subrate_pp(&trend, period, 2), 2e-12, 1e-16), "{}", eye::subrate_pp(&trend, period, 2));
    let tone =
        |div: f64| -> Vec<f32> { (0..512).map(|k| (1e-12 * (PI * k as f64 / (div / 2.0)).cos()) as f32).collect() };
    assert!(close(eye::subrate_pp(&tone(4.0), period, 4), 2e-12, 1e-14), "F/4");
    assert!(close(eye::subrate_pp(&tone(8.0), period, 8), 2e-12, 1e-14), "F/8");
    assert!(close(eye::srj_pp(&trend, period), 2e-12, 1e-15), "{}", eye::srj_pp(&trend, period));
    let wide = tone(2.0);
    let mixed: Vec<f32> = (0..512).map(|k| wide[k] + tone(4.0)[k] + tone(8.0)[k]).collect();
    assert!(close(eye::srj_pp(&mixed, period), 6e-12, 0.05), "{}", eye::srj_pp(&mixed, period));
    assert_eq!(eye::phase_noise(&vec![0.0; 128], period, 1.0, 1.0 / period), 0.0);
    let pn: Vec<f32> = (0..4096).map(|k| (5e-12 * (2.0 * PI * k as f64 / 64.0).sin()) as f32).collect();
    let floor = eye::phase_noise(&pn, period, 1.0 / (period * 256.0), 1.0 / (period * 16.0));
    // the sine's own bin is left out, so this is the window skirt, well under the 5 ps tone
    assert!(floor.is_finite() && floor < 2.5e-12, "phase noise off the line {floor}");
    assert!(eye::npj(&[0.0; 10], &Clock::default(), &trend, &eye::EdgeMeans::default()).is_nan());
    let n_edge = 2048usize;
    let sine: Vec<f64> = (0..n_edge).map(|k| 10e-12 * (2.0 * PI * k as f64 / 32.0).sin()).collect();
    let trend_j: Vec<f32> = sine.iter().map(|x| *x as f32).collect();
    let edges: Vec<f64> = (0..n_edge).map(|k| k as f64 * period).collect();
    let clk = Clock { period, phase: 0.0, edges, ticks: vec![], first_rising: true };
    let left = eye::npj(&sine, &clk, &trend_j, &eye::EdgeMeans::default());
    assert!(left.is_finite() && left < 2e-12, "npj after the sine {left}");
    let ps = eye::psij(&trend_j, &clk, &sine, None, 1.0 / (period * 32.0), 0.4, 0.6 * period, None);
    assert!(close(ps.before.tie_pp, 20e-12, 0.02), "psij before {}", ps.before.tie_pp);
    assert!(ps.after.tie_pp < 2e-12 && ps.after.pj < ps.before.pj, "psij after {} pj {}", ps.after.tie_pp, ps.after.pj);

    assert!(close(eye::dc_common(&[1.0, 1.0], &[3.0, 3.0]), 2.0, 1e-9));
    assert!(close(eye::ac_common_pp(&[0.0, 1.0, 0.0, 1.0], None, 1e-6, false), 1.0, 1e-9));
    let a: Vec<f32> = (0..11).map(|k| k as f32 / 10.0).collect();
    let b: Vec<f32> = (0..11).map(|k| 1.0 - k as f32 / 10.0).collect();
    assert!(close(eye::diff_crossover(&a, &b), 0.5, 1e-6), "{}", eye::diff_crossover(&a, &b));

    let dt = 1e-8;
    let mut v = vec![0.5f32; 500];
    for k in 40..60 {
        v[k] = 1.0;
    }
    let c = Clock { period: 1e-6, phase: 0.0, edges: vec![0.0, 3e-6], ticks: vec![], first_rising: true };
    assert!(close(eye::tnt_ratio(&v, dt, &c), 20.0 * (0.5f64).log10(), 0.05), "{}", eye::tnt_ratio(&v, dt, &c));

    let mut edges = Vec::new();
    let mut t = 0.0;
    for _ in 0..64 {
        let f = 1e6 * (1.0 + 0.05 * (2.0 * PI * 125e3 * t).sin());
        let p = 1.0 / f;
        edges.push(t);
        edges.push(t + 0.5 * p);
        t += p;
    }
    let c = Clock { period: 1e-6, phase: 0.0, edges, ticks: vec![], first_rising: true };
    let (ppm, _) = eye::ssc_deviation(&c);
    assert!(ppm > 50_000.0 && ppm < 150_000.0, "{}", ppm);
    let rate = eye::ssc_rate(&c);
    assert!(close(rate, 125e3, 30e3), "{}", rate);
    let clock = Clock {
        period: 1e-6,
        phase: 0.0,
        edges: (0..32).map(|k| k as f64 * 1e-6).collect(),
        ticks: vec![],
        first_rising: true,
    };
    assert!(eye::is_clock(&clock));
    let data = Clock { edges: (0..32).map(|k| k as f64 * 3e-6).collect(), ..clock };
    assert!(!eye::is_clock(&data));
}

/// The sliding window of `phase_noise` gives what a window summed afresh for every bin gave (O(N²): 1.7 s for 131k
/// unit intervals), on a trend with tones (lines left out), noise and a slow wander.
#[test]
fn phase_noise_window_matches_the_direct_sum() {
    fn direct(trend: &[f32], period: f64, f_lo: f64, f_hi: f64) -> f64 {
        let (df, spec) = eye::tie_spectrum(trend, period);
        let lines = eye::pj_lines(df, &spec, spec.len());
        let marked: Vec<usize> = lines.into_iter().map(|(f, _)| (f / df).round() as usize).collect();
        let (lo, hi) = (f_lo.min(f_hi), f_lo.max(f_hi));
        let (mut acc, mut any) = (0.0, false);
        for k in 1..spec.len() {
            let f = k as f64 * df;
            if f < lo || f > hi || marked.contains(&k) {
                continue;
            }
            let lf = f.log10();
            let (mut s, mut c) = (0.0, 0usize);
            let k0 = ((k as f64) / 10f64.powf(0.05)).floor() as usize;
            let k1 = ((k as f64) * 10f64.powf(0.05)).ceil() as usize;
            for j in k0.max(1)..=k1.min(spec.len() - 1) {
                if ((j as f64 * df).log10() - lf).abs() <= 0.05 {
                    s += spec[j];
                    c += 1;
                }
            }
            let v = if c == 0 { spec[k] } else { s / c as f64 };
            acc += v * v;
            any = true;
        }
        if any {
            acc.sqrt()
        } else {
            f64::NAN
        }
    }
    let period = 1e-9;
    let mut x = 3u32;
    for n in [64usize, 1000, 4096, 10_000] {
        let trend: Vec<f32> = (0..n)
            .map(|k| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((x >> 8) as f64 / (1u32 << 24) as f64 - 0.5) * 1e-12;
                let t = k as f64;
                let tones = 3e-12 * (2.0 * PI * t / 37.0).sin() + 1e-12 * (2.0 * PI * t / 5.3).sin();
                (noise + tones + 2e-12 * (t / n as f64)) as f32
            })
            .collect();
        for (lo, hi) in [(1.0, 1.0 / period), (1.0 / (period * n as f64 / 4.0), 1.0 / (period * 8.0)), (2e6, 3e6)] {
            let (got, want) = (eye::phase_noise(&trend, period, lo, hi), direct(&trend, period, lo, hi));
            let same = got.is_nan() && want.is_nan() || (got - want).abs() <= 1e-9 * want.abs();
            assert!(same, "n {n} {lo}..{hi}: {got} {want}");
        }
    }
}
