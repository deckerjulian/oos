// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Spectrum analyser, eye/jitter, power and identify against synthetic signals with known answers.

use oos_dsp::math::Window;
use oos_dsp::sim::sample;
use oos_dsp::spectrum::{self, Detector, Settings, Trace};
use std::f64::consts::PI;

/// Deterministic Gaussian noise (xorshift + Box-Muller), standard deviation `sigma`.
struct Gauss(u64);

impl Gauss {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn next(&mut self, sigma: f64) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        sigma * (-2.0 * u.ln()).sqrt() * (2.0 * PI * v).cos()
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ------------------------------------------------------------------------------------------------ spectrum

#[test]
fn spectrum_enbw_table_matches_the_windows() {
    let m = 4096;
    for w in [Window::Rect, Window::Hann, Window::Hamming, Window::Blackman, Window::Flattop] {
        let (s1, s2) = (0..m).fold((0.0, 0.0), |(a, b), i| (a + w.w(i, m), b + w.w(i, m).powi(2)));
        let e = m as f64 * s2 / (s1 * s1);
        assert!(close(e, spectrum::enbw(w), 0.005), "{:?} {}", w, e);
    }
}

#[test]
fn spectrum_tone_marker_within_0p1_percent_and_0p1_db() {
    let dt = 1e-5; // 100 kSa/s
    let v = sample(|t| 1.0 * (2.0 * PI * 1000.0 * t + 0.3).sin(), 0.0, dt, 100_000);
    let s = Settings { center: 1000.0, span: 2000.0, window: Window::Flattop, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    assert_eq!(sp.dbv.len(), 801);
    assert!(close(sp.rbw, 20.0, 1.0), "rbw {}", sp.rbw);
    let m = spectrum::max_peak(&sp).unwrap();
    assert!(close(m.f, 1000.0, 1.0), "{:?}", m);
    assert!(close(m.dbv, -3.0103, 0.1), "{:?}", m);
    // off-grid tone with every detector
    let v = sample(|t| 0.5 * (2.0 * PI * 1234.5 * t).sin(), 0.0, dt, 50_000);
    for d in [Detector::Peak, Detector::Sample, Detector::Avg] {
        let sp = spectrum::analyze(&v, dt, &Settings { detector: d, ..s }, &mut None);
        let m = spectrum::max_peak(&sp).unwrap();
        assert!(close(m.f, 1234.5, 1.2) && close(m.dbv, -9.0309, 0.1), "{:?} {:?}", d, m);
    }
}

#[test]
fn spectrum_peak_table_next_peak_and_delta() {
    let dt = 1e-5;
    let v = sample(|t| (2.0 * PI * 1000.0 * t).sin() + 0.1 * (2.0 * PI * 3000.0 * t).sin() + 0.01 * (2.0 * PI * 7000.0 * t).sin(), 0.0, dt, 100_000);
    let s = Settings { center: 5000.0, span: 10_000.0, window: Window::Flattop, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    let p = spectrum::peak_table(&sp, 5, -80.0, 500.0);
    assert_eq!(p.len(), 3, "{:?}", p);
    assert!(close(p[0].f, 1000.0, 5.0) && close(p[1].f, 3000.0, 5.0) && close(p[2].f, 7000.0, 5.0), "{:?}", p);
    let d = spectrum::delta(p[0], p[1]);
    assert!(close(d.f, 2000.0, 5.0) && close(d.dbv, -20.0, 0.15), "{:?}", d);
    let r = spectrum::next_peak(&sp, p[0].f, true, -80.0).unwrap();
    assert!(close(r.f, 3000.0, 5.0), "{:?} {:?}", r, &sp.dbv[70..90]);
    let l = spectrum::next_peak(&sp, r.f, false, -80.0).unwrap();
    assert!(close(l.f, 1000.0, 5.0));
    assert!(spectrum::next_peak(&sp, p[0].f, false, -80.0).is_none());
    let at = spectrum::marker_at(&sp, 3000.0).unwrap();
    assert!(close(at.dbv, -23.01, 0.2), "{:?}", at);
}

#[test]
fn spectrum_rbw_and_noise_floor() {
    // white noise sigma: the Avg-detector trace reads sigma^2 * RBW / (fs/2) per bin; the channel power over the
    // full span is sigma^2; the 99 % occupied bandwidth of flat noise is 99 % of the span
    let dt = 1e-6;
    let mut g = Gauss(7);
    let v: Vec<f32> = (0..200_000).map(|_| g.next(0.1) as f32).collect();
    for (rbw, w) in [(1000.0, Window::Hann), (5000.0, Window::Flattop), (300.0, Window::Blackman)] {
        let s = Settings { rbw, window: w, detector: Detector::Avg, ..Default::default() };
        let sp = spectrum::analyze(&v, dt, &s, &mut None);
        assert!(close(sp.rbw, rbw, rbw * 0.01), "{} {}", rbw, sp.rbw);
        let mean_db: f64 = sp.dbv[10..790].iter().map(|&x| x as f64).sum::<f64>() / 780.0;
        let want = 10.0 * (0.01 * rbw / 500e3f64).log10();
        assert!(close(mean_db, want, 0.6), "rbw {} {} {}", rbw, mean_db, want);
        let cp = spectrum::channel_power(&sp, 0.0, 500e3);
        assert!(close(cp, 0.01, 0.0006), "{}", cp);
        let (a, b) = spectrum::occupied_bw(&sp, 99.0).unwrap();
        assert!(close(b - a, 0.99 * 500e3, 0.02 * 500e3), "{} {}", a, b);
    }
    // a tone's channel power is its mean square
    let v = sample(|t| 2.0 * (2.0 * PI * 10e3 * t).sin(), 0.0, dt, 100_000);
    let s = Settings { center: 10e3, span: 10e3, rbw: 200.0, window: Window::Hann, detector: Detector::Avg, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    assert!(close(spectrum::channel_power(&sp, 8e3, 12e3), 2.0, 0.02));
    let (a, b) = spectrum::occupied_bw(&sp, 99.0).unwrap();
    assert!(a > 9e3 && b < 11e3 && b - a < 1e3, "{} {}", a, b);
}

#[test]
fn spectrum_trace_average_max_and_min_hold() {
    let dt = 1e-6;
    let s = Settings { center: 50e3, span: 100e3, rbw: 2000.0, window: Window::Hann, detector: Detector::Sample, ..Default::default() };
    let mut g = Gauss(3);
    let (mut avg, mut mx, mut mn, mut single) = (None, None, None, Vec::new());
    for k in 0..32 {
        let v: Vec<f32> = (0..20_000).map(|_| g.next(0.1) as f32).collect();
        let a = spectrum::analyze(&v, dt, &Settings { trace: Trace::Average, averaging: 32, ..s }, &mut avg);
        let x = spectrum::analyze(&v, dt, &Settings { trace: Trace::MaxHold, ..s }, &mut mx);
        let n = spectrum::analyze(&v, dt, &Settings { trace: Trace::MinHold, ..s }, &mut mn);
        if k == 0 {
            single = a.dbv.clone();
        }
        if k == 31 {
            let sd = |d: &[f32]| {
                let m = d.iter().map(|&x| x as f64).sum::<f64>() / d.len() as f64;
                (d.iter().map(|&x| (x as f64 - m).powi(2)).sum::<f64>() / d.len() as f64).sqrt()
            };
            // averaging 32 traces narrows the noise trace by about sqrt(32)
            assert!(sd(&a.dbv[5..795]) < sd(&single[5..795]) / 3.0, "{} {}", sd(&a.dbv), sd(&single));
            assert!(a.dbv.iter().zip(&x.dbv).zip(&n.dbv).all(|((a, x), n)| x >= a && a >= n));
            assert_eq!(avg.as_ref().unwrap().count(), 32);
        }
    }
}

#[test]
fn spectrum_short_and_bad_input() {
    let s = Settings::default();
    assert!(spectrum::analyze(&[], 1e-6, &s, &mut None).dbv.is_empty());
    assert!(spectrum::analyze(&[f32::NAN; 100], 1e-6, &s, &mut None).dbv.is_empty());
    let mut v = sample(|t| (2.0 * PI * 1e3 * t).sin(), 0.0, 1e-5, 1000);
    v[10] = f32::NAN;
    assert!(spectrum::analyze(&v, 1e-5, &s, &mut None).dbv.iter().all(|x| x.is_finite()));
    assert!(spectrum::distortion(&[0.0; 10], 1e-6, None, 5).is_none());
    assert!(spectrum::max_peak(&spectrum::Spectrum::default()).is_none());
    assert!(spectrum::occupied_bw(&spectrum::Spectrum::default(), 99.0).is_none());
}

#[test]
fn distortion_thd_of_known_harmonics() {
    let dt = 1e-5;
    let v = sample(|t| (2.0 * PI * 1000.0 * t).sin() + 0.01 * (2.0 * PI * 2000.0 * t).sin() + 0.005 * (2.0 * PI * 3000.0 * t + 1.0).sin(), 0.0, dt, 65_000);
    let d = spectrum::distortion(&v, dt, None, 5).unwrap();
    let thd = 20.0 * ((0.01f64.powi(2) + 0.005f64.powi(2)).sqrt()).log10(); // -39.03 dB
    assert!(close(d.f1, 1000.0, 0.01), "{}", d.f1);
    assert!(close(d.a1_dbv, -3.0103, 0.01), "{}", d.a1_dbv);
    assert!(close(d.thd_db, thd, 0.2), "{} {}", d.thd_db, thd);
    assert!(close(d.thd_pct, 1.118, 0.03), "{}", d.thd_pct);
    assert!(close(d.harmonics[0].0, 2000.0, 0.1) && close(d.harmonics[0].1, -40.0, 0.1), "{:?}", d.harmonics);
    assert!(close(d.harmonics[1].1, -46.02, 0.1), "{:?}", d.harmonics);
    assert!(close(d.sfdr_dbc, 40.0, 0.1), "{}", d.sfdr_dbc);
    assert!(d.snr_db > 100.0, "{}", d.snr_db);
    // with the fundamental given
    let d2 = spectrum::distortion(&v, dt, Some(1000.0), 3).unwrap();
    assert!(close(d2.thd_db, thd, 0.2));
}

#[test]
fn distortion_snr_of_white_noise() {
    let dt = 1e-6;
    let mut g = Gauss(11);
    let v: Vec<f32> = (0..100_000).map(|k| ((2.0 * PI * 12_345.0 * k as f64 * dt).sin() + g.next(0.01)) as f32).collect();
    let d = spectrum::distortion(&v, dt, None, 5).unwrap();
    let snr = 10.0 * (0.5f64 / 1e-4).log10(); // 36.99 dB
    assert!(close(d.snr_db, snr, 0.5), "{} {}", d.snr_db, snr);
    assert!(close(d.sinad_db, snr, 0.5), "{}", d.sinad_db);
    assert!(close(d.thdn_db, -snr, 0.5), "{}", d.thdn_db);
}

#[test]
fn distortion_enob_of_an_ideal_quantiser() {
    for bits in [8u32, 12] {
        let dt = 1e-6;
        let q = 2.0 / (1u64 << bits) as f64; // full scale +-1 V
        let v: Vec<f32> = (0..65_536)
            .map(|k| {
                let x = (1.0 - q / 2.0) * (2.0 * PI * 10_007.3 * k as f64 * dt).sin();
                ((x / q).floor() * q + q / 2.0) as f32
            })
            .collect();
        let d = spectrum::distortion(&v, dt, None, 9).unwrap();
        assert!(close(d.enob, bits as f64, 0.3), "{} bits: {}", bits, d.enob);
    }
}

#[test]
fn waterfall_rows_and_colours() {
    let mut w = spectrum::Waterfall::new(3);
    for k in 0..5 {
        w.push(&spectrum::Spectrum { dbv: vec![k as f32; 4], ..Default::default() });
    }
    assert_eq!(w.rows.len(), 3);
    assert_eq!(w.rows[0][0], 4.0);
    assert_eq!(w.rows[2][0], 2.0);
    w.push(&spectrum::Spectrum { dbv: vec![0.0; 8], ..Default::default() });
    assert_eq!(w.rows.len(), 1);
    assert_eq!(spectrum::color(-200.0, 0.0, 100.0), (68, 1, 84));
    assert_eq!(spectrum::color(10.0, 0.0, 100.0), (253, 231, 37));
    assert_eq!(spectrum::color(f32::NAN, 0.0, 100.0), (68, 1, 84));
    // monotonic lightness (green channel rises along the map)
    let g: Vec<u8> = (0..=10).map(|i| spectrum::color(-100.0 + 10.0 * i as f32, 0.0, 100.0).1).collect();
    assert!(g.windows(2).all(|p| p[1] >= p[0]), "{:?}", g);
}

// ------------------------------------------------------------------------------------------------ eye / jitter

use oos_dsp::eye::{self, Eye};
use oos_dsp::sim::Logic;

/// PRBS7 (x^7 + x^6 + 1) bit sequence.
fn prbs7(n: usize) -> Vec<bool> {
    let mut s = 0x7fu8;
    (0..n)
        .map(|_| {
            let b = ((s >> 6) ^ (s >> 5)) & 1;
            s = ((s << 1) | b) & 0x7f;
            b == 1
        })
        .collect()
}

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
            let y = if x < 0.5 * ui { up(x) } else if x < 1.5 * ui { 1.0 - up(x - ui) } else { up(x - 2.0 * ui) };
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

// ------------------------------------------------------------------------------------------------ power

use oos_dsp::power;

#[test]
fn power_resistive_load_has_unity_power_factor() {
    let dt = 10e-6;
    let w = 2.0 * PI * 50.0;
    // 10.65 cycles: only the 10 whole ones count
    let v = sample(|t| 325.0 * (w * t).sin(), 0.0, dt, 21_300);
    let i = sample(|t| 325.0 / 23.0 * (w * t).sin(), 0.0, dt, 21_300);
    let q = power::quality(&v, &i, dt, None);
    assert_eq!(q.cycles, 10);
    assert!(close(q.freq, 50.0, 0.05), "{:?}", q);
    assert!(close(q.vrms, 325.0 / 2f64.sqrt(), 0.1), "{:?}", q);
    assert!(close(q.p_real, 325.0f64.powi(2) / 2.0 / 23.0, 1.0), "{:?}", q);
    assert!(close(q.pf, 1.0, 1e-3) && close(q.phase_deg, 0.0, 0.1) && q.q_reactive.abs() < 0.03 * q.s_apparent, "{:?}", q);
    assert!(close(q.crest_v, 2f64.sqrt(), 0.01) && close(q.crest_i, 2f64.sqrt(), 0.01));
}

#[test]
fn power_rl_load_phase_and_reactive_power() {
    // R = 10 ohm, X_L = 10 ohm at 50 Hz: current lags by 45 degrees
    let dt = 20e-6;
    let w = 2.0 * PI * 50.0;
    let ip = 325.0 / 200f64.sqrt();
    let v = sample(|t| 325.0 * (w * t).sin(), 0.0, dt, 10_000);
    let i = sample(|t| ip * (w * t - PI / 4.0).sin(), 0.0, dt, 10_000);
    let q = power::quality(&v, &i, dt, Some(50.0));
    let s = 325.0 / 2f64.sqrt() * ip / 2f64.sqrt();
    assert!(close(q.phase_deg, 45.0, 0.2), "{:?}", q);
    assert!(close(q.pf, 0.70711, 0.003), "{:?}", q);
    assert!(close(q.s_apparent, s, 0.5) && close(q.p_real, s * 0.70711, 2.0) && close(q.q_reactive, s * 0.70711, 2.0), "{:?}", q);
    // capacitive: negative phase and reactive power
    let i = sample(|t| ip * (w * t + PI / 6.0).sin(), 0.0, dt, 10_000);
    let q = power::quality(&v, &i, dt, None);
    assert!(close(q.phase_deg, -30.0, 0.2) && q.q_reactive < 0.0, "{:?}", q);
}

#[test]
fn power_current_harmonics_against_class_a() {
    let dt = 20e-6;
    let w = 2.0 * PI * 50.0;
    let i = sample(|t| 5.0 * (w * t).sin() + 3.0 * (3.0 * w * t + 0.4).sin() + 0.5 * (5.0 * w * t).sin() + 1.5 * (7.0 * w * t).sin(), 0.0, dt, 20_000);
    let h = power::harmonics(&i, dt, Some(50.0), 15);
    assert_eq!(h.len(), 15);
    let r = std::f64::consts::FRAC_1_SQRT_2;
    assert!(close(h[0].1, 5.0 * r, 0.01) && h[0].3);
    assert!(close(h[2].1, 3.0 * r, 0.01) && close(h[2].2, 2.30, 1e-9) && h[2].3, "{:?}", h[2]);
    assert!(close(h[4].1, 0.5 * r, 0.01) && h[4].3);
    assert!(close(h[6].1, 1.5 * r, 0.01) && close(h[6].2, 0.77, 1e-9) && !h[6].3, "{:?}", h[6]);
    assert!(h[1].1 < 1e-3 && h[1].3);
    assert!(close(power::class_a_limit(10), 0.184, 1e-9) && close(power::class_a_limit(21), 0.107_142_857, 1e-6));
    // frequency measured from the current
    assert!(close(power::harmonics(&i, dt, None, 3)[2].1, 3.0 * r, 0.02));
}

/// Piecewise linear function through (t, value) points.
fn pwl(p: &[(f64, f64)], t: f64) -> f64 {
    if t <= p[0].0 {
        return p[0].1;
    }
    for w in p.windows(2) {
        if t <= w[1].0 {
            return w[0].1 + (w[1].1 - w[0].1) * (t - w[0].0) / (w[1].0 - w[0].0);
        }
    }
    p[p.len() - 1].1
}

#[test]
fn power_switching_loss_of_hard_switched_edges() {
    // 400 V / 10 A, R_on = 50 mOhm: turn-on = current rise 50 ns, then voltage fall 100 ns; turn-off mirrored
    let (t_i, t_v) = (50e-9, 100e-9);
    let on = 1e-6;
    let von = 0.5;
    let vp = [(on, 400.0), (on + t_i, 400.0), (on + t_i + t_v, von), (on + 5e-6, von), (on + 5e-6 + t_v, 400.0)];
    let ip = [(on, 0.0), (on + t_i, 10.0), (on + 5e-6 + t_v, 10.0), (on + 5e-6 + t_v + t_i, 0.0)];
    let dt = 1e-9;
    let period = 10e-6;
    let n = 30_000;
    let vds = sample(|t| pwl(&vp, t % period), 0.0, dt, n);
    let id = sample(|t| pwl(&ip, t % period), 0.0, dt, n);
    let s = power::switching(&vds, &id, dt, 10.0);
    // analytic energies in the 10 % windows
    let va = 400.0 - von;
    let e_v = 10.0 * t_v * (0.9 * 400.0 - va * 0.405); // V falls from 100 % to 10 % at 10 A
    let e_i = 4000.0 * t_i * 0.495; // I rises from 10 % to 100 % at 400 V
    assert_eq!(s.transitions.len(), 6, "{:?}", s.transitions);
    assert!(close(s.e_on, e_i + e_v, 0.01 * (e_i + e_v)), "{} {}", s.e_on, e_i + e_v);
    assert!(close(s.e_off, e_i + e_v, 0.01 * (e_i + e_v)), "{} {}", s.e_off, e_i + e_v);
    // conduction: the 0.5 V flat part plus the bottom 10 % of both voltage ramps
    let tail = 10.0 * t_v * (0.1 * von + va * 0.005);
    let e_c = von * 10.0 * (5e-6 - t_i - t_v) + 2.0 * tail;
    assert!(close(s.e_cond, e_c, 0.02 * e_c), "{} {}", s.e_cond, e_c);
    assert!(s.transitions[0].on && !s.transitions[1].on);
    assert!(close(s.transitions[0].t0, on + 0.1 * t_i, 2e-9), "{:?}", s.transitions[0]);
    // average power over 3 whole periods: everything above plus the current ramps below 10 % at 400 V
    let e_total = 2.0 * (e_i + e_v) + e_c + 2.0 * 4000.0 * t_i * 0.005;
    assert!(close(s.p_avg, e_total / period, 0.01 * e_total / period), "{} {}", s.p_avg, e_total / period);
}

#[test]
fn power_ripple_inrush_efficiency_soa_and_duty() {
    let dt = 1e-6;
    let v = sample(|t| 5.0 + 0.05 * (2.0 * PI * 1e3 * t).sin(), 0.0, dt, 10_000);
    let (pp, ac) = power::ripple(&v);
    assert!(close(pp, 0.1, 1e-4) && close(ac, 0.05 / 2f64.sqrt(), 1e-4));
    // inrush i = 50 exp(-t / 1 ms): I^2t = 2500 * tau / 2
    let i = sample(|t| 50.0 * (-t / 1e-3).exp(), 0.0, dt, 20_000);
    let (pk, tp, i2t) = power::inrush(&i, dt);
    assert!(close(pk, 50.0, 1e-4) && tp == 0.0 && close(i2t, 1.25, 0.01), "{} {}", pk, i2t);
    let e = power::efficiency(&[12.0; 100], &[1.0; 100], &[5.0; 100], &[2.0; 100], dt);
    assert!(close(e.eff, 10.0 / 12.0, 1e-6) && close(e.p_in, 12.0, 1e-6) && close(e.e_out, 10.0 * 100.0 * dt, 1e-9));
    // SOA: 20 A up to 10 V, then constant power (log-log line) to 2 A at 100 V, nothing above 100 V
    let lim = [(10.0, 20.0), (100.0, 2.0)];
    assert!(close(power::soa_limit(&lim, 31.62), 6.325, 0.01));
    let vds = [5.0, 50.0, 50.0, 120.0];
    let id = [19.0, 3.9, 4.1, 0.1];
    assert_eq!(power::soa(&vds, &id, &lim), vec![2, 3]);
    // PWM, duty ramping 20 % .. 80 % over 50 periods
    let f = 10e3;
    let v = sample(|t| { let k = (t * f).floor(); let d = 0.2 + 0.6 * k / 49.0; if t * f - k < d { 3.3 } else { 0.0 } }, 0.0, 1e-7, 50 * 1000);
    let d = power::duty_trend(&v, 1e-7);
    assert!(d.len() >= 47, "{}", d.len());
    for &(t, x) in &d {
        let k = (t * f).round();
        assert!(close(x, 0.2 + 0.6 * k / 49.0, 0.003), "{} {}", t, x);
    }
    // bad input
    assert_eq!(power::quality(&[], &[], dt, None), power::Quality::default());
    assert!(power::harmonics(&[1.0; 100], dt, None, 5).is_empty());
    assert!(power::switching(&[f32::NAN; 10], &[0.0; 10], dt, 10.0).transitions.is_empty());
    assert!(power::ripple(&[]).0.is_nan());
}

// ------------------------------------------------------------------------------------------------ magnetic, motor, double pulse

fn inductor(l: f64, r: f64, dt: f64, n: usize) -> (Vec<f32>, Vec<f32>) {
    let f = 1000.0;
    let v = sample(
        |t| {
            let x = (t * f).fract();
            let i = if x < 0.5 { -1.0 + 4.0 * x } else { 3.0 - 4.0 * x };
            let di = if x < 0.5 { 4.0 * f } else { -4.0 * f };
            l * di + r * i
        },
        0.0,
        dt,
        n,
    );
    let i = sample(|t| { let x = (t * f).fract(); if x < 0.5 { -1.0 + 4.0 * x } else { 3.0 - 4.0 * x } }, 0.0, dt, n);
    (v, i)
}

#[test]
fn magnetic_inductance_of_a_linear_core_is_the_slope() {
    let l = 1e-3;
    let dt = 2e-6;
    let (v, i) = inductor(l, 0.0, dt, 6000);
    let m = oos_dsp::magnetic::measure(&v, &i, dt, oos_dsp::magnetic::Geom::default());
    assert!(m.kept >= 8, "{:?}", (m.kept, m.dropped, m.note, m.cycles));
    assert!(close(m.l.unwrap(), l, 0.01 * l), "{} {}", m.l.unwrap(), m.note);
    // a sampled pure inductor leaves a fraction of a watt of trapezoid error
    assert!(m.loss.unwrap().abs() < 0.02, "{}", m.loss.unwrap());
    let loop0 = &m.loops[0];
    let (i0, p0) = (loop0[0].1, loop0[0].0);
    for &(psi, cur) in loop0 {
        let est = p0 + l * (cur - i0);
        assert!((psi - est).abs() < 2e-4, "psi {} est {} i {}", psi, est, cur);
    }
}

#[test]
fn magnetic_series_resistance_is_the_loss_and_rcu_removes_it() {
    let dt = 2e-6;
    let (v, i) = inductor(1e-3, 1.0, dt, 6000);
    let m = oos_dsp::magnetic::measure(&v, &i, dt, oos_dsp::magnetic::Geom::default());
    assert!(close(m.loss.unwrap(), 1.0 / 3.0, 0.02), "{} {}", m.loss.unwrap(), m.note);
    let g = oos_dsp::magnetic::Geom { rcu: 1.0, ..oos_dsp::magnetic::Geom::default() };
    let m = oos_dsp::magnetic::measure(&v, &i, dt, g);
    assert!(m.loss.unwrap().abs() < 0.02, "left {} copper {}", m.loss.unwrap(), m.copper);
    assert!(close(m.copper, 1.0 / 3.0, 0.02), "{}", m.copper);
}

#[test]
fn magnetic_short_record_nan_and_reversed_current() {
    let dt = 2e-6;
    let (v, i) = inductor(1e-3, 1.0, dt, 200);
    let m = oos_dsp::magnetic::measure(&v, &i, dt, Default::default());
    assert!(m.l.is_none() && m.loss.is_none() && m.partial, "{}", m.note);
    let nan = vec![f32::NAN; 64];
    let m = oos_dsp::magnetic::measure(&nan, &nan, dt, Default::default());
    assert!(m.l.is_none() && m.loss.is_none(), "{}", m.note);
    let (v, i) = inductor(1e-3, 1.0, dt, 6000);
    let i: Vec<f32> = i.iter().map(|x| -x).collect();
    let m = oos_dsp::magnetic::measure(&v, &i, dt, Default::default());
    assert!(m.loss.unwrap() < 0.0, "{}", m.loss.unwrap());
    assert!(close(m.loss.unwrap(), -1.0 / 3.0, 0.02), "{}", m.loss.unwrap());
}

#[test]
fn motor_balanced_sines_put_the_peak_on_d() {
    let dt = 1e-4;
    let (f, a, n) = (50.0, 10.0, 8000);
    let w = 2.0 * PI * f;
    let va = sample(|t| a * (w * t).cos(), 0.0, dt, n);
    let vb = sample(|t| a * (w * t - 2.0 * PI / 3.0).cos(), 0.0, dt, n);
    let vc = sample(|t| a * (w * t + 2.0 * PI / 3.0).cos(), 0.0, dt, n);
    let d = oos_dsp::motor::dq0(&va, &vb, &vc, dt, oos_dsp::motor::Theta::Vector).unwrap();
    assert!(close(d.d, a, 0.05), "{:?}", (d.d, d.q, d.zero));
    assert!(close(d.q, 0.0, 0.05) && close(d.zero, 0.0, 0.05), "{:?}", (d.d, d.q, d.zero));
    assert!(oos_dsp::motor::dq0(&va, &vb, &[], dt, oos_dsp::motor::Theta::Vector).is_none());
}

#[test]
fn motor_quadrature_speed_and_swapped_direction() {
    let (hold, cycles, dt, cpr) = (10usize, 50usize, 1e-4, 100.0);
    let n = hold * 4 * cycles;
    let mut a = vec![false; n];
    let mut b = vec![false; n];
    for k in 0..n {
        match (k / hold) % 4 {
            0 => {}
            1 => a[k] = true,
            2 => {
                a[k] = true;
                b[k] = true;
            }
            _ => b[k] = true,
        }
    }
    let s = oos_dsp::motor::shaft_quad(&a, &b, None, dt, 0.0, cpr, false);
    // one count per state change: the first sample establishes the state and does not count
    let counts = (n / hold - 1) as f64;
    let cps = counts / ((n - 1) as f64 * dt);
    let rpm = cps / cpr * 60.0;
    assert!(close(s.rpm.unwrap(), rpm, 1e-6), "{} vs {} ({})", s.rpm.unwrap(), rpm, cps * 360.0 / cpr);
    assert_eq!(s.direction, oos_dsp::motor::Direction::Forward);
    let swapped = oos_dsp::motor::shaft_quad(&a, &b, None, dt, 0.0, cpr, true);
    assert_eq!(swapped.direction, oos_dsp::motor::Direction::Reverse);
    assert!(close(swapped.rpm.unwrap(), -rpm, 1e-6), "{}", swapped.rpm.unwrap());
}

#[test]
fn motor_torque_and_bus_give_mechanical_power_and_efficiency() {
    let (rpm, dt, n) = (600.0, 1e-4, 4000);
    let deg_s = rpm * 6.0;
    let v = sample(|t| deg_s * t, 0.0, dt, n);
    let shaft = oos_dsp::motor::shaft_analog(&v, dt, 0.0, 1.0, 0.0, false, 1);
    assert!(close(shaft.rpm.unwrap(), rpm, 0.05), "{}", shaft.rpm.unwrap());
    let torque = vec![2.0f32; n];
    let bus_v = vec![48.0f32; n];
    let bus_i = vec![4.0f32; n];
    let m = oos_dsp::motor::mechanical(shaft, Some(&torque), 1.0, Some(&bus_v), Some(&bus_i));
    let p = 2.0 * rpm * 2.0 * PI / 60.0;
    assert!(close(m.p_mech.unwrap(), p, 0.01), "{}", m.p_mech.unwrap());
    assert!(close(m.p_elec.unwrap(), 192.0, 1e-6));
    assert!(close(m.eff.unwrap(), p / 192.0, 1e-4), "{}", m.eff.unwrap());
    assert!(!m.generating);
}

#[test]
fn double_pulse_timings_peak_and_recovery_charge() {
    let dt = 1e-9;
    let tend = 13e-6;
    let n = (tend / dt) as usize;
    // gate: two pulses, 0..10 V in 20 ns. Current plateau 10 A. Recovery triangle 4 A, 200 ns base.
    let gate = [(0.0, 0.0), (1e-6, 0.0), (1.02e-6, 10.0), (6e-6, 10.0), (6.02e-6, 0.0), (10e-6, 0.0), (10.02e-6, 10.0), (12e-6, 10.0), (12.02e-6, 0.0)];
    let id = [
        (0.0, 0.0),
        (1.1e-6, 0.0),
        (1.2e-6, 10.0),
        (6.05e-6, 10.0),
        (6.15e-6, 0.0),
        (10.05e-6, 0.0),
        (10.15e-6, 10.0),
        (10.25e-6, 14.0),
        (10.35e-6, 10.0),
        (12e-6, 10.0),
    ];
    let vds = [
        (0.0, 400.0),
        (1.05e-6, 400.0),
        (1.15e-6, 1.0),
        (6.1e-6, 1.0),
        (6.2e-6, 400.0),
        (6.25e-6, 480.0),
        (6.4e-6, 400.0),
        (10.05e-6, 400.0),
        (10.15e-6, 1.0),
        (12e-6, 1.0),
    ];
    let g = sample(|t| pwl(&gate, t), 0.0, dt, n);
    let i = sample(|t| pwl(&id, t), 0.0, dt, n);
    let v = sample(|t| pwl(&vds, t), 0.0, dt, n);
    let p = oos_dsp::dpt::measure(&v, &i, Some(&g), oos_dsp::dpt::Fourth::Off, dt, 10.0);
    assert!(close(p.td_on.unwrap(), 58e-9, 2e-9), "tdon {}", p.td_on.unwrap());
    assert!(close(p.tr.unwrap(), 80e-9, 2e-9), "tr {}", p.tr.unwrap());
    assert!(close(p.ton.unwrap(), 138e-9, 3e-9), "ton {}", p.ton.unwrap());
    assert!(close(p.vpeak.unwrap(), 480.0, 1e-6), "vpeak {}", p.vpeak.unwrap());
    assert!(close(p.ipeak.unwrap(), 14.0, 1e-3), "ipeak {}", p.ipeak.unwrap());
    assert!(close(p.irrm.unwrap(), 4.0, 0.05), "irrm {}", p.irrm.unwrap());
    // symmetric triangle, Trr ends at 10 % of Irrm: 190 ns of the 200 ns base
    assert!(close(p.trr.unwrap(), 190e-9, 3e-9), "trr {}", p.trr.unwrap());
    assert!(close(p.qrr.unwrap(), 0.5 * 200e-9 * 4.0, 0.01 * 0.5 * 200e-9 * 4.0), "qrr {}", p.qrr.unwrap());
}

#[test]
fn double_pulse_one_pulse_no_dead_time_and_inverted_current() {
    let dt = 1e-9;
    let n = (8e-6 / dt) as usize;
    let gate = [(0.0, 0.0), (1e-6, 0.0), (1.02e-6, 10.0), (6e-6, 10.0), (6.02e-6, 0.0)];
    let id = [(0.0, 0.0), (1.1e-6, 0.0), (1.2e-6, 10.0), (6.05e-6, 10.0), (6.15e-6, 0.0)];
    let vds = [(0.0, 400.0), (1.05e-6, 400.0), (1.15e-6, 1.0), (6.1e-6, 1.0), (6.2e-6, 400.0), (6.25e-6, 480.0), (6.4e-6, 400.0)];
    let g = sample(|t| pwl(&gate, t), 0.0, dt, n);
    let i = sample(|t| pwl(&id, t), 0.0, dt, n);
    let v = sample(|t| pwl(&vds, t), 0.0, dt, n);
    let pos = oos_dsp::dpt::measure(&v, &i, Some(&g), oos_dsp::dpt::Fourth::Off, dt, 10.0);
    assert!(pos.trr.is_none() && pos.tdt.is_none() && pos.td_on.is_none(), "{}", pos.note);
    assert!(pos.td_off.is_some() && pos.tf.is_some() && pos.toff.is_some(), "{:?}", (pos.td_off, pos.tf, pos.toff));
    let inv: Vec<f32> = i.iter().map(|x| -x).collect();
    let neg = oos_dsp::dpt::measure(&v, &inv, Some(&g), oos_dsp::dpt::Fourth::Off, dt, 10.0);
    assert!(neg.inverted && neg.trr.is_none() && neg.tdt.is_none());
    assert!(close(neg.td_off.unwrap(), pos.td_off.unwrap(), 2e-9), "{} {}", neg.td_off.unwrap(), pos.td_off.unwrap());
    assert!(close(neg.tf.unwrap(), pos.tf.unwrap(), 2e-9));
    assert!(close(neg.toff.unwrap(), pos.toff.unwrap(), 2e-9));
    assert!(neg.td_on.is_none());
}

// ------------------------------------------------------------------------------------------------ identify

use oos_dsp::decode::Parity;
use oos_dsp::identify::{self, DecoderHint, Finding, SignalKind, TriggerHint};
use oos_dsp::sim::{self, add_noise, PulseTrain};

fn top(ch: &[&[f32]], dt: f64) -> Finding {
    let f = identify::identify(ch, dt);
    assert!(!f.is_empty());
    f.into_iter().next().unwrap()
}

#[test]
fn identify_dc_noise_and_sine() {
    let dt = 1e-6;
    let mut dc = vec![1.5f32; 10_000];
    add_noise(&mut dc, 0.001, 1);
    let f = top(&[&dc], dt);
    assert!(matches!(f.kind, SignalKind::Dc { level } if close(level, 1.5, 0.01)), "{:?}", f);
    assert_eq!(f.trigger, TriggerHint::None);
    let mut g = Gauss(2);
    let n: Vec<f32> = (0..50_000).map(|_| g.next(0.2) as f32).collect();
    let f = top(&[&n], dt);
    assert!(matches!(f.kind, SignalKind::Noise { rms } if close(rms, 0.2, 0.01)), "{:?}", f);
    let mut s = sample(|t| 0.8 * (2.0 * PI * 12_500.0 * t).sin() + 0.2, 0.0, dt, 20_000);
    add_noise(&mut s, 0.01, 3);
    let f = top(&[&s], dt);
    assert!(matches!(f.kind, SignalKind::Sine { freq, amp } if close(freq, 12_500.0, 10.0) && close(amp, 0.8, 0.02)), "{:?}", f);
    assert!(matches!(f.trigger, TriggerHint::Edge { source: 0, level, rising: true, .. } if close(level as f64, 0.2, 0.05)), "{:?}", f);
    assert!(f.summary.starts_with("Sine 12.50 kHz"), "{}", f.summary);
}

#[test]
fn identify_mains_triangle_sawtooth() {
    let dt = 20e-6;
    let m = sample(|t| 325.0 * (2.0 * PI * 50.0 * t).sin(), 0.0, dt, 10_000);
    let f = top(&[&m], dt);
    assert!(matches!(f.kind, SignalKind::Mains { freq, vrms } if close(freq, 50.0, 0.1) && close(vrms, 229.8, 1.0)), "{:?}", f);
    let dt = 1e-6;
    let tri = sample(|t| { let x = (t * 1000.0).fract(); if x < 0.5 { 4.0 * x - 1.0 } else { 3.0 - 4.0 * x } }, 0.0, dt, 20_000);
    let f = top(&[&tri], dt);
    assert!(matches!(f.kind, SignalKind::Triangle { freq, amp } if close(freq, 1000.0, 1.0) && close(amp, 1.0, 0.02)), "{:?}", f);
    let saw = sample(|t| 2.0 * (t * 1000.0).fract() - 1.0, 0.0, dt, 20_000);
    let f = top(&[&saw], dt);
    assert!(matches!(f.kind, SignalKind::Sawtooth { freq, rising: true, .. } if close(freq, 1000.0, 1.0)), "{:?}", f);
    let saw: Vec<f32> = saw.iter().map(|x| -x).collect();
    assert!(matches!(top(&[&saw], dt).kind, SignalKind::Sawtooth { rising: false, .. }));
}

#[test]
fn identify_clock_pwm_and_glitchy_pulse_train() {
    let dt = 1e-8;
    let clk = PulseTrain::square(1e6, 0.0, 3.3, 10e-9);
    let mut v = sample(|t| clk.v(t), 0.0, dt, 5000);
    add_noise(&mut v, 0.05, 4);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Clock { freq, duty, lo, hi } if close(freq, 1e6, 1e3) && close(duty, 0.5, 0.02) && close(lo, 0.0, 0.05) && close(hi, 3.3, 0.05)), "{:?}", f);
    assert!(matches!(f.trigger, TriggerHint::Edge { level, rising: true, .. } if close(level as f64, 1.65, 0.05)));
    // PWM 10 kHz, duty 20 .. 80 % (the first period starts before the record, the last one is incomplete)
    let fr = 10e3;
    let v = sample(|t| { let k = (t * fr).floor(); let d = 0.2 + 0.6 * (k / 19.0).min(1.0); if t * fr - k < d { 5.0 } else { 0.0 } }, 0.0, 1e-7, 20_000);
    let f = top(&[&v], 1e-7);
    assert!(matches!(f.kind, SignalKind::Pwm { freq, duty_min, duty_max } if close(freq, fr, 10.0) && close(duty_min, 0.2 + 0.6 / 19.0, 0.01) && close(duty_max, 0.2 + 0.6 * 18.0 / 19.0, 0.01)), "{:?}", f);
    // narrow pulses: 5 % duty
    let p = PulseTrain { width: 0.05e-3, ..PulseTrain::square(1e3, 0.0, 5.0, 1e-7) };
    let v = sample(|t| p.v(t), 0.0, 1e-6, 20_000);
    let f = top(&[&v], 1e-6);
    assert!(matches!(f.kind, SignalKind::PulseTrain { freq, width, positive: true, anomalies: 0 } if close(freq, 1e3, 1.0) && close(width, 50e-6, 2e-6)), "{:?}", f);
    // every 5th pulse is a glitch of 5 % of the width: pulse width trigger below half the normal width
    let p = PulseTrain { width: 0.3e-3, every: 5, at: 2, special: sim::Pulse { width: 0.015e-3, height: 1.0 }, ..PulseTrain::square(1e3, 0.0, 5.0, 1e-7) };
    let v = sample(|t| p.v(t), 0.0, 1e-6, 20_000);
    let f = top(&[&v], 1e-6);
    assert!(matches!(f.kind, SignalKind::PulseTrain { anomalies: 4, positive: true, .. }), "{:?}", f);
    assert!(matches!(f.trigger, TriggerHint::PulseWidth { positive: true, less_than: Some(w), .. } if close(w, 0.15e-3, 0.01e-3)), "{:?}", f);
}

#[test]
fn identify_am_fm_and_bursts() {
    let dt = 1e-6;
    let v = sample(|t| (1.0 + 0.5 * (2.0 * PI * 500.0 * t).sin()) * (2.0 * PI * 10e3 * t).sin(), 0.0, dt, 100_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Am { carrier, modulation, depth } if close(carrier, 10e3, 20.0) && close(modulation, 500.0, 10.0) && close(depth, 0.5, 0.05)), "{:?}", f);
    let v = sample(|t| (2.0 * PI * 10e3 * t + 5.0 * (2.0 * PI * 200.0 * t).sin()).sin(), 0.0, dt, 100_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Fm { carrier, modulation, deviation } if close(carrier, 10e3, 300.0) && close(modulation, 200.0, 5.0) && close(deviation, 1000.0, 100.0)), "{:?}", f);
    // 20 cycles of 100 kHz every 1 ms
    let dt = 1e-7;
    let v = sample(|t| if t % 1e-3 < 0.2e-3 { (2.0 * PI * 100e3 * t).sin() } else { 0.0 }, 0.0, dt, 50_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Burst { freq, length, period } if close(freq, 100e3, 5e3) && close(length, 0.2e-3, 0.01e-3) && close(period, 1e-3, 0.01e-3)), "{:?}", f);
    assert!(matches!(f.trigger, TriggerHint::Edge { holdoff, .. } if holdoff > 0.2e-3 && holdoff < 0.8e-3), "{:?}", f);
}

#[test]
fn identify_uart_and_inverted_uart() {
    let baud = 115_200.0;
    let l = sim::uart_logic(b"Hello, OOS! 0123456789", baud, 8, None, 1.0, 0.0, 3.3);
    let dt = 1.0 / baud / 20.0;
    let n = (25.0 * 12.0 * 20.0) as usize + 400;
    let mut v = sample(|t| l.v(t), 0.0, dt, n);
    add_noise(&mut v, 0.05, 6);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Uart { baud: b, idle_high: true } if b == baud), "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::Uart { ch: 0, baud: b, bits: 8, parity: Parity::None, invert: false, .. }) if b == baud), "{:?}", f);
    assert!(matches!(f.trigger, TriggerHint::Edge { rising: false, .. }));
    assert!(f.confidence > 0.8);
    assert_eq!(identify::estimate_baud(&v, dt, 1.65, 0.3), Some(baud));
    // RS-232 line side: idle low
    let l = sim::uart_logic(b"\x00\xffAbc", 9600.0, 8, None, 2.0, 5.0, -5.0);
    let dt = 1.0 / 9600.0 / 16.0;
    let v = sample(|t| l.v(t), 0.0, dt, 16 * 80);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Uart { baud: b, idle_high: false } if b == 9600.0), "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::Uart { invert: true, .. })));
    // 0x55 sync bytes with half-bit idle gaps: nearly periodic, but a decoder-confirmed UART at exactly 1 MBd
    let l = sim::uart_logic(&[0x55; 40], 1e6, 8, None, 0.5, 0.0, 3.3);
    let dt = 1e-8;
    let v = sample(|t| l.v(t), 0.0, dt, 50_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Uart { baud: b, idle_high: true } if b == 1e6), "{:?}", f);
}

#[test]
fn identify_can_lin_onewire_manchester() {
    let frames = [sim::CanFrame { id: 0x123, ext: false, rtr: false, data: vec![1, 2, 3, 4] }, sim::CanFrame { id: 0x7e0, ext: false, rtr: false, data: vec![0x55; 8] }];
    let l = sim::can_logic(&frames, 500e3, 0.0, 3.3);
    let dt = 1.0 / 25e6;
    let v = sample(|t| l.v(t), 0.0, dt, 15_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Can { bitrate, dominant_low: true } if bitrate == 500e3), "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::Can { bitrate, invert: false, .. }) if bitrate == 500e3));
    let l = sim::lin_logic(&[(0x10, vec![1, 2, 3], false), (0x3c, vec![0xaa, 0x55], true)], 19200.0, 0.0, 12.0);
    let dt = 1e-6;
    let v = sample(|t| l.v(t), 0.0, dt, 9_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Lin { baud } if baud == 19200.0), "{:?}", f);
    // 1-Wire: reset 480 us, presence, then 16 bit slots (1: 6 us low, 0: 60 us low; slots of 70 us)
    let mut ow = Logic::new(0.0, 3.3, 0.5e-6);
    ow.push(0.0, true);
    let mut t = 50e-6;
    ow.push(t, false);
    t += 480e-6;
    ow.push(t, true);
    t += 70e-6;
    ow.push(t, false);
    t += 120e-6;
    ow.push(t, true);
    t += 300e-6;
    for k in 0..16 {
        let low = if (0xa5c3u16 >> k) & 1 == 1 { 6e-6 } else { 60e-6 };
        ow.push(t, false);
        ow.push(t + low, true);
        t += 70e-6;
    }
    let v = sample(|x| ow.v(x), 0.0, 0.5e-6, ((t + 100e-6) / 0.5e-6) as usize);
    let f = top(&[&v], 0.5e-6);
    assert_eq!(f.kind, SignalKind::OneWire, "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::OneWire { ch: 0, .. })));
    // Manchester (IEEE 802.3: 1 = low -> high), 10 kbit/s
    let br = 10e3;
    let mut half = Vec::new();
    for b in prbs7(300) {
        half.push(!b);
        half.push(b);
    }
    let l = sim::bits_logic(&half, 0.5 / br, 4, 0.0, 3.3);
    let dt = 1e-6;
    let v = sample(|t| l.v(t), 0.0, dt, (310.0 / br / dt) as usize);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Manchester { bitrate } if bitrate == br), "{:?}", f);
}

#[test]
fn identify_i2c_pair_among_other_channels() {
    let x = [
        sim::I2cXfer { addr: 0x50, read: false, data: vec![0x00, 0x10], repeated: false, stop: false },
        sim::I2cXfer { addr: 0x50, read: true, data: vec![0xde, 0xad, 0xbe], repeated: true, stop: true },
    ];
    let (sda, scl) = sim::i2c_logic(&x, 100e3, 0.0, 3.3);
    let dt = 1e-7;
    let n = 70_000;
    let (a, b) = (sample(|t| sda.v(t), 0.0, dt, n), sample(|t| scl.v(t), 0.0, dt, n));
    let s = sample(|t| (2.0 * PI * 3e3 * t).sin(), 0.0, dt, n);
    let f = identify::identify(&[&s, &b, &a], dt);
    let i2c = f.iter().find(|x| matches!(x.kind, SignalKind::I2c { .. })).expect("I2C");
    assert!(matches!(i2c.kind, SignalKind::I2c { rate } if rate == 100e3), "{:?}", i2c);
    assert_eq!(i2c.decoder, Some(DecoderHint::I2c { sda: 2, scl: 1, thr: 1.65 }));
    assert!(matches!(i2c.trigger, TriggerHint::Edge { source: 2, rising: false, .. }));
    assert_eq!(i2c.channels, vec![2, 1]);
    assert_eq!(f.len(), 2);
    assert!(f.iter().any(|x| matches!(x.kind, SignalKind::Sine { .. }) && x.channels == vec![0]));
}

#[test]
fn identify_spi_roles_and_mode() {
    for mode in [0u8, 3] {
        let words = [(0xa5u32, 0x3c), (0x0f, 0xf0), (0x81, 0x7e), (0x5a, 0x00)];
        let [cs, clk, mosi, miso] = sim::spi_logic(&words, 8, mode, 1e6, 0.0, 3.3);
        let dt = 2e-8;
        let n = 2500;
        let s = |l: &Logic| sample(|t| l.v(t), 0.0, dt, n);
        let (cs, clk, mosi, miso) = (s(&cs), s(&clk), s(&mosi), s(&miso));
        let f = identify::identify(&[&mosi, &cs, &clk, &miso], dt);
        let spi = f.iter().find(|x| matches!(x.kind, SignalKind::Spi { .. })).unwrap_or_else(|| panic!("mode {}: {:?}", mode, f));
        assert!(matches!(spi.kind, SignalKind::Spi { rate, mode: m, bits: 8 } if rate == 1e6 && m == mode), "{:?}", spi);
        match &spi.decoder {
            Some(DecoderHint::Spi { clk: 2, data, cs: Some(1), mode: m, bits: 8, .. }) => {
                assert_eq!(*m, mode);
                assert!(data.contains(&0) && data.contains(&3), "{:?}", data);
            }
            d => panic!("{:?}", d),
        }
        assert_eq!(f.len(), 1, "{:?}", f);
    }
}

#[test]
fn identify_video_pal_and_ntsc() {
    for (std, ntsc) in [(sim::VideoStd::Pal, false), (sim::VideoStd::Ntsc, true)] {
        let dt = 0.1e-6;
        let t0 = sim::video_line_time(std, 1, 20);
        let v = sample(|t| sim::video(std, t), t0, dt, 20_000);
        let f = top(&[&v], dt);
        assert!(matches!(f.kind, SignalKind::Video { ntsc: n, line_rate } if n == ntsc && close(line_rate, 1.0 / std.line(), 5.0)), "{:?}", f);
        assert_eq!(f.trigger, TriggerHint::Video { source: 0, ntsc });
    }
}

#[test]
fn identify_bad_input() {
    assert!(identify::identify(&[], 1e-6).is_empty());
    assert!(identify::identify(&[&[]], 1e-6).is_empty());
    assert!(identify::identify(&[&[1.0; 8]], 1e-6).is_empty());
    assert!(identify::identify(&[&[0.0; 100]], 0.0).is_empty());
    let f = identify::identify(&[&[f32::NAN; 1000]], 1e-6);
    assert!(f.len() <= 1);
    let step: Vec<f32> = (0..1000).map(|k| if k < 500 { 0.0 } else { 1.0 }).collect();
    assert!(matches!(top(&[&step], 1e-6).kind, SignalKind::Step { rising: true, .. }));
    assert_eq!(identify::snap_rate(9500.0), 9600.0);
    assert_eq!(identify::snap_rate(12345.0), 12345.0);
}

/// 200 kHz square of `vpp` around `dc`, 70 k points at 1.25 GSa/s, a little noise (the case the second pane is made for).
fn square_record(dc: f32, vpp: f32) -> (Vec<f32>, f64) {
    let dt = 0.8e-9;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let v = (0..70_000)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5;
            let hi = (i as f64 * dt * 200e3).rem_euclid(1.0) < 0.5;
            dc + if hi { vpp / 2.0 } else { -vpp / 2.0 } + 2e-3 * noise
        })
        .collect();
    (v, dt)
}

fn assert_odd_harmonics(m: &[spectrum::Marker], top: usize) {
    assert!(m.len() >= top, "{:?}", m);
    for p in &m[..top] {
        let k = p.f / 200e3;
        assert!((k - k.round()).abs() < 0.02 && k.round() as u64 % 2 == 1, "{:?}", m);
    }
    assert!((m[0].f - 200e3).abs() < 500.0, "{:?}", m);
}

#[test]
fn a_square_wave_shows_its_odd_harmonics_in_a_followed_span() {
    let (v, dt) = square_record(0.5, 2.0);
    let stop = spectrum::auto_stop(200e3, 10, 0.0);
    assert_eq!(stop, 2.5e6);
    let s = Settings { center: stop / 2.0, span: stop, rbw: 1.0, window: Window::Blackman, points: 780, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    // the record sets the resolution: ENBW / 56 us
    assert!((sp.rbw / (1.727 / 56e-6) - 1.0).abs() < 0.02, "RBW {}", sp.rbw);
    let m = spectrum::rank_lines(&sp, Some(200e3), 8);
    assert_odd_harmonics(&m, 6);
    // RMS of the fundamental of a square of amplitude 1: 4 / pi / sqrt(2) = -0.92 dBV; the third is 9.54 dB down
    assert!((m[0].dbv + 0.92).abs() < 0.3, "{:?}", m[0]);
    assert!((m[0].dbv - m[1].dbv - 9.54).abs() < 0.3, "{:?}", m);
}

#[test]
fn the_whole_band_still_finds_the_fundamental_beside_the_dc_skirt() {
    let (v, dt) = square_record(0.5, 2.0);
    for w in [Window::Hann, Window::Blackman, Window::Flattop] {
        let s = Settings { rbw: 1.0, window: w, points: 1024, ..Default::default() };
        let sp = spectrum::analyze(&v, dt, &s, &mut None);
        assert_eq!(sp.f0, 0.0);
        let m = spectrum::rank_lines(&sp, Some(200e3), 8);
        assert_odd_harmonics(&m, 6);
    }
}

#[test]
fn two_periods_on_the_screen_still_give_the_fundamental() {
    // 1 kHz square around +1.25 V, 2 ms of it: the main lobe is wider than the distance to DC
    let dt = 2e-3 / 3600.0;
    let v: Vec<f32> = (0..3600).map(|i| if (i as f64 * dt * 1e3).rem_euclid(1.0) < 0.5 { 2.5 } else { 0.0 }).collect();
    let s = Settings { center: 7.5e3, span: 15e3, rbw: 1.0, window: Window::Blackman, points: 780, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    let m = spectrum::rank_lines(&sp, Some(1e3), 8);
    assert!(!m.is_empty() && (m[0].f - 1e3).abs() < 200.0, "{:?}", sp.lines);
    // the trace still shows DC at its level: 1.25 V = 1.94 dBV
    assert!((sp.dbv[0] - 1.94).abs() < 0.3, "{}", sp.dbv[0]);
    // from three periods on the lobes part and the harmonics come
    let dt = 3e-3 / 3600.0;
    let v: Vec<f32> = (0..3600).map(|i| if (i as f64 * dt * 1e3).rem_euclid(1.0) < 0.5 { 2.5 } else { 0.0 }).collect();
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    assert!(spectrum::rank_lines(&sp, Some(1e3), 8).len() >= 4, "{:?}", sp.lines);
}

#[test]
fn spurs_far_under_a_sine_are_no_lines_to_mark() {
    let dt = 0.8e-9;
    let v: Vec<f32> = (0..70_000)
        .map(|i| {
            let t = i as f64 * dt;
            let w = |f: f64, a: f64| a * (2.0 * std::f64::consts::PI * f * t).sin();
            (w(1e6, 1.0) + w(156.25e6, 1e-3) + w(223.4e6, 7e-4)) as f32
        })
        .collect();
    let s = Settings { rbw: 1.0, window: Window::Blackman, points: 1024, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    assert!(sp.lines.iter().any(|m| (m.f - 156.25e6).abs() < 1e5), "{:?}", sp.lines);
    let m = spectrum::rank_lines(&sp, Some(1e6), 8);
    assert_eq!(m.len(), 1, "{:?}", m);
    assert!((m[0].f - 1e6).abs() < 500.0);
}

#[test]
fn window_side_lobes_are_no_lines() {
    let dt = 1e-6;
    let v: Vec<f32> = (0..4000).map(|i| (2.0 * std::f64::consts::PI * 51_234.5 * i as f64 * dt).sin() as f32).collect();
    for w in [Window::Rect, Window::Hann, Window::Hamming, Window::Blackman, Window::Flattop] {
        let s = Settings { rbw: 1.0, window: w, points: 800, ..Default::default() };
        let sp = spectrum::analyze(&v, dt, &s, &mut None);
        assert_eq!(sp.lines.len(), 1, "{:?} {:?}", w, sp.lines);
    }
}

#[test]
fn decimation_keeps_level_and_noise_density() {
    let dt = 0.8e-9;
    let v: Vec<f32> = (0..200_000).map(|i| (2.0_f64.sqrt() * (2.0 * std::f64::consts::PI * 1.7e6 * i as f64 * dt).sin()) as f32).collect();
    // 1.7 MHz near the top of a 2 MHz band, where the droop is largest
    let s = Settings { center: 1e6, span: 2e6, rbw: 1e3, window: Window::Hann, points: 800, ..Default::default() };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    let m = spectrum::rank_lines(&sp, None, 1);
    assert!(m[0].dbv.abs() < 0.1 && (m[0].f - 1.7e6).abs() < 300.0, "{:?}", m);
}

#[test]
fn a_followed_span_holds_still_on_a_noisy_frequency() {
    let stop = spectrum::auto_stop(200e3, 10, 0.0);
    for f in [190e3, 199e3, 205e3, 210e3, 180e3, 150e3] {
        assert_eq!(spectrum::auto_stop(f, 10, stop), stop, "{}", f);
    }
    assert_eq!(spectrum::auto_stop(300e3, 10, stop), 4e6);
    assert_eq!(spectrum::auto_stop(50e3, 10, stop), 6e5);
    assert_eq!(spectrum::auto_stop(f64::NAN, 10, stop), stop);
    assert_eq!(spectrum::auto_stop(1e3, 0, 0.0), 0.0);
}

#[test]
#[ignore]
fn spectrum_cost_on_the_host() {
    for (n, span, name) in [(70_000usize, 2.5e6, "70k pane"), (70_000, 0.0, "70k full"), (1_000_000, 2.5e6, "1M pane"), (1_000_000, 0.0, "1M full")] {
        let v: Vec<f32> = (0..n).map(|i| ((i as f64 * 0.8e-9 * 200e3).rem_euclid(1.0) < 0.5) as u8 as f32).collect();
        let s = Settings { center: span / 2.0, span, rbw: 1.0, window: Window::Blackman, points: 780, ..Default::default() };
        let mut st = None;
        let t = std::time::Instant::now();
        for _ in 0..10 {
            std::hint::black_box(spectrum::analyze(&v, 0.8e-9, &s, &mut st));
        }
        println!("{}: {:.2} ms", name, t.elapsed().as_secs_f64() * 100.0);
    }
}

#[test]
fn acpr_ratio_and_a_band_that_runs_off_the_trace() {
    use oos_dsp::spectrum::Spectrum;
    let mut dbv = vec![-120.0f32; 100];
    dbv[20] = 0.0;
    dbv[60] = -10.0;
    let s = Spectrum { f0: 0.0, df: 1e3, rbw: 1e3, dbv, ..Default::default() };
    let a = spectrum::acpr(&s, 4e3, 40e3);
    assert!(close(a.ratio, 0.1, 1e-6), "{:?}", a);
    assert!(close(a.upper_db, -10.0, 1e-4) && a.lower_db.is_nan(), "{:?}", a);
    assert!(spectrum::acpr(&s, 0.0, 1.0).ratio.is_nan());
}

#[test]
fn switching_slew_rds_ripple_turn_times_capacitance_and_rail() {
    let (t_i, t_v) = (50e-9, 100e-9);
    let on = 1e-6;
    let von = 0.5;
    let vp = [(on, 400.0), (on + t_i, 400.0), (on + t_i + t_v, von), (on + 5e-6, von), (on + 5e-6 + t_v, 400.0)];
    let ip = [(on, 0.0), (on + t_i, 10.0), (on + 5e-6 + t_v, 10.0), (on + 5e-6 + t_v + t_i, 0.0)];
    let dt = 1e-9;
    let vds = sample(|t| pwl(&vp, t % 10e-6), 0.0, dt, 30_000);
    let id = sample(|t| pwl(&ip, t % 10e-6), 0.0, dt, 30_000);
    let s = power::switching(&vds, &id, dt, 10.0);
    let slope_v = (400.0 - von) / t_v;
    assert!(close(s.dvdt_rise, slope_v, 0.02 * slope_v), "{}", s.dvdt_rise);
    assert!(close(s.dvdt_fall, -slope_v, 0.02 * slope_v), "{}", s.dvdt_fall);
    assert!(close(s.didt_rise, 10.0 / t_i, 0.02 * 10.0 / t_i), "{}", s.didt_rise);
    assert!(close(s.didt_fall, -10.0 / t_i, 0.02 * 10.0 / t_i), "{}", s.didt_fall);
    assert!(s.rds_on > 0.04 && s.rds_on < 0.08, "{}", s.rds_on);

    let dt = 1e-5;
    let v = sample(|t| (2.0 * PI * 1e3 * t).sin(), 0.0, dt, 10_000);
    let tone = power::tone_ripple(&v, dt, 1e3);
    assert!(close(tone.pp, 2.0, 1e-3) && close(tone.rms, 1.0 / 2f64.sqrt(), 1e-3), "{:?}", tone);
    assert!(power::switching_ripple(&v, dt, 2e6).pp.is_nan());
    assert!(close(power::line_hz(&v, dt), 1e3, 5.0));

    let vin = sample(|t| pwl(&[(0.0, 0.0), (100e-6, 0.0), (110e-6, 10.0), (1e-3, 10.0)], t), 0.0, 1e-6, 1000);
    let vout = sample(|t| pwl(&[(0.0, 0.0), (200e-6, 0.0), (300e-6, 5.0), (1e-3, 5.0)], t), 0.0, 1e-6, 1000);
    let ton = power::turn_on_time(&vin, &vout, 1e-6);
    assert!(close(ton, 195e-6, 2e-6), "{}", ton);
    let vin = sample(|t| pwl(&[(0.0, 10.0), (100e-6, 10.0), (110e-6, 0.0), (1e-3, 0.0)], t), 0.0, 1e-6, 1000);
    let vout = sample(|t| pwl(&[(0.0, 5.0), (200e-6, 5.0), (300e-6, 0.0), (1e-3, 0.0)], t), 0.0, 1e-6, 1000);
    let toff = power::turn_off_time(&vin, &vout, 1e-6);
    assert!(close(toff, 195e-6, 2e-6), "{}", toff);

    let v: Vec<f32> = (0..1000).map(|k| k as f32 / 999.0 * 10.0).collect();
    let i = vec![0.01f32; 1000];
    let c = power::input_capacitance(&v, &i, 1e-6);
    assert!(close(c, 0.01 * 999e-6 / 10.0, 1e-12), "{}", c);
    assert!(power::input_capacitance(&[1.0; 10], &i, 1e-6).is_nan());

    let mut rail = vec![5.0f32; 1000];
    rail[100] = 6.0;
    rail[200] = 4.0;
    let r = power::rail(&rail, 1e-6, Some(5.0), None, None);
    assert!(close(r.overshoot, 1.0, 1e-9) && close(r.undershoot, 1.0, 1e-9) && close(r.dc, 5.0, 1e-9) && close(r.turn_on, 6.0, 1e-9), "{:?}", r);
}

#[test]
fn bode_psrr_negates_the_gain_and_impedance_is_the_ratio() {
    use oos_dsp::bode::{self, Mode};
    let dt = 1e-6;
    let sine = |amp: f64, ph: f64| -> Vec<f32> {
        (0..50_000).map(|k| (amp * (2.0 * PI * 1000.0 * k as f64 * dt + ph).sin()) as f32).collect()
    };
    let (inp, out) = (sine(1.0, 0.0), sine(0.1, -PI / 4.0));
    let (g, p) = bode::response(Mode::Loop, &inp, &out, dt, 1000.0).unwrap();
    assert!(close(g, -20.0, 0.01) && close(p, -45.0, 0.05), "{} {}", g, p);
    let (g, p) = bode::response(Mode::Psrr, &inp, &out, dt, 1000.0).unwrap();
    assert!(close(g, 20.0, 0.01) && close(p, -45.0, 0.05), "{} {}", g, p);
    let (z, p) = bode::response(Mode::Z, &inp, &out, dt, 1000.0).unwrap();
    assert!(close(z, 0.1, 1e-4) && close(p, -45.0, 0.05), "{} {}", z, p);
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
    assert!(close(eye::srj_pp(&trend, period), 2e-12, 1e-15), "{}", eye::srj_pp(&trend, period));
    assert_eq!(eye::phase_noise(&vec![0.0; 128], period, 1.0, 1.0 / period), 0.0);
    assert!(eye::npj(&[0.0; 10], &Clock::default(), &trend, &eye::EdgeMeans::default()).is_nan());

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
    let clock = Clock { period: 1e-6, phase: 0.0, edges: (0..32).map(|k| k as f64 * 1e-6).collect(), ticks: vec![], first_rising: true };
    assert!(eye::is_clock(&clock));
    let data = Clock { edges: (0..32).map(|k| k as f64 * 3e-6).collect(), ..clock };
    assert!(!eye::is_clock(&data));
}
