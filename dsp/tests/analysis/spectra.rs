// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The spectrum analyser against tones, noise and squares: markers, peaks, RBW and noise floor, trace modes,
//! distortion, the waterfall, followed spans, decimation and ACPR. A new spectrum test goes here.

use super::*;

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
    let v = sample(
        |t| (2.0 * PI * 1000.0 * t).sin() + 0.1 * (2.0 * PI * 3000.0 * t).sin() + 0.01 * (2.0 * PI * 7000.0 * t).sin(),
        0.0,
        dt,
        100_000,
    );
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
    let s = Settings {
        center: 10e3,
        span: 10e3,
        rbw: 200.0,
        window: Window::Hann,
        detector: Detector::Avg,
        ..Default::default()
    };
    let sp = spectrum::analyze(&v, dt, &s, &mut None);
    assert!(close(spectrum::channel_power(&sp, 8e3, 12e3), 2.0, 0.02));
    let (a, b) = spectrum::occupied_bw(&sp, 99.0).unwrap();
    assert!(a > 9e3 && b < 11e3 && b - a < 1e3, "{} {}", a, b);
}

#[test]
fn spectrum_trace_average_max_and_min_hold() {
    let dt = 1e-6;
    let s = Settings {
        center: 50e3,
        span: 100e3,
        rbw: 2000.0,
        window: Window::Hann,
        detector: Detector::Sample,
        ..Default::default()
    };
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
    let v = sample(
        |t| {
            (2.0 * PI * 1000.0 * t).sin()
                + 0.01 * (2.0 * PI * 2000.0 * t).sin()
                + 0.005 * (2.0 * PI * 3000.0 * t + 1.0).sin()
        },
        0.0,
        dt,
        65_000,
    );
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

// Break it catches: with five periods in the record the lobe of the fundamental held the third harmonic, and the
// fundamental of a 10 kHz square was read as 16.2 kHz, of a ramp as 12.3 kHz (seen on a DHO924S).
#[test]
fn distortion_finds_the_fundamental_of_a_square_with_few_periods() {
    let (n, f) = (3600usize, 10_000.0f64);
    for periods in [4.6, 5.0, 5.3, 8.0, 12.5, 19.0, 21.0, 40.0] {
        let dt = periods / f / n as f64;
        let phase = |i: usize| (i as f64 * dt * f + 0.137).fract();
        let square: Vec<f32> = (0..n).map(|i| if phase(i) < 0.5 { 0.5 } else { -0.5 }).collect();
        let ramp: Vec<f32> = (0..n).map(|i| phase(i) as f32 - 0.5).collect();
        let sine: Vec<f32> = (0..n).map(|i| (2.0 * PI * phase(i)).sin() as f32).collect();
        for (name, v, thd) in [("square", &square, 20.0..50.0), ("ramp", &ramp, 20.0..85.0), ("sine", &sine, 0.0..1.0)]
        {
            let d = spectrum::distortion(v, dt, None, 9).unwrap();
            assert!((d.f1 - f).abs() < 0.02 * f, "{name}, {periods} periods: fundamental {} Hz", d.f1);
            assert!(thd.contains(&d.thd_pct), "{name}, {periods} periods: THD {} %", d.thd_pct);
        }
    }
}

#[test]
fn distortion_snr_of_white_noise() {
    let dt = 1e-6;
    let mut g = Gauss(11);
    let v: Vec<f32> =
        (0..100_000).map(|k| ((2.0 * PI * 12_345.0 * k as f64 * dt).sin() + g.next(0.01)) as f32).collect();
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
    let s = Settings {
        center: stop / 2.0,
        span: stop,
        rbw: 1.0,
        window: Window::Blackman,
        points: 780,
        ..Default::default()
    };
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
    let s =
        Settings { center: 7.5e3, span: 15e3, rbw: 1.0, window: Window::Blackman, points: 780, ..Default::default() };
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
    let v: Vec<f32> = (0..200_000)
        .map(|i| (2.0_f64.sqrt() * (2.0 * std::f64::consts::PI * 1.7e6 * i as f64 * dt).sin()) as f32)
        .collect();
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
    for (n, span, name) in [
        (70_000usize, 2.5e6, "70k pane"),
        (70_000, 0.0, "70k full"),
        (1_000_000, 2.5e6, "1M pane"),
        (1_000_000, 0.0, "1M full"),
    ] {
        let v: Vec<f32> = (0..n).map(|i| ((i as f64 * 0.8e-9 * 200e3).rem_euclid(1.0) < 0.5) as u8 as f32).collect();
        let s = Settings {
            center: span / 2.0,
            span,
            rbw: 1.0,
            window: Window::Blackman,
            points: 780,
            ..Default::default()
        };
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
