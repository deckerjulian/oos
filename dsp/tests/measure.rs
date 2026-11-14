// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurements against signals with known values.

use oos_dsp::measure::*;
use oos_dsp::sim::{add_noise, sample, Pulse, PulseTrain};

fn close(x: f64, want: f64, rel: f64) -> bool {
    (x - want).abs() <= rel * want.abs().max(1e-12)
}

#[test]
fn square_wave() {
    let dt = 10e-9;
    // 100 kHz, 0..2 V, linear 100 ns transitions (10-90 % = 80 ns), 30 % duty
    let p = PulseTrain { width: 3e-6, ..PulseTrain::square(100e3, 0.0, 2.0, 100e-9) };
    let mut v = sample(|t| p.v(t), 0.0, dt, 100_000);
    add_noise(&mut v, 0.005, 7);
    let a = analyze(&v, dt);
    assert!(close(a.get(Meas::Vtop), 2.0, 0.01));
    assert!(a.get(Meas::Vbase).abs() < 0.01);
    assert!(close(a.get(Meas::Freq), 100e3, 1e-4));
    assert!(close(a.get(Meas::PWidth), 3e-6, 0.002));
    assert!(close(a.get(Meas::NWidth), 7e-6, 0.002));
    assert!(close(a.get(Meas::PDuty), 30.0, 0.003));
    assert!(close(a.get(Meas::Rise), 80e-9, 0.05), "{}", a.get(Meas::Rise));
    assert!(close(a.get(Meas::Fall), 80e-9, 0.05));
    assert!(close(a.get(Meas::PSlew), 1.6 / 80e-9, 0.06));
    assert_eq!(a.get(Meas::PEdges), 100.0);
    assert!(close(a.get(Meas::Vavg), 0.6, 0.01));
    assert!(close(a.get(Meas::VavgCyc), 0.6, 0.01));
    assert!(close(a.get(Meas::Area), 0.6 * 1e-3, 0.01));
}

#[test]
fn sine_wave() {
    let dt = 1e-6;
    let v = sample(|t| 1.5 * (2.0 * std::f64::consts::PI * 1e3 * t).sin() + 0.5, 0.0, dt, 20_000);
    let a = analyze(&v, dt);
    assert!(close(a.get(Meas::Vpp), 3.0, 1e-3));
    assert!(close(a.get(Meas::VrmsAc), 1.5 / 2f64.sqrt(), 1e-3));
    assert!(close(a.get(Meas::Vrms), (0.25 + 1.125f64).sqrt(), 1e-3));
    assert!(close(a.get(Meas::Freq), 1e3, 1e-4));
    assert!(close(a.get(Meas::Period), 1e-3, 1e-4));
    // no flat levels: top/base fall back to max/min
    assert!(close(a.get(Meas::Vamp), 3.0, 1e-3));
    assert!(close(a.get(Meas::TVmax), 0.25e-3, 1e-2));
}

#[test]
fn overshoot_preshoot() {
    let dt = 1e-9;
    // 0..1 V square with a 20 % overshoot ringing after the rising edge and 5 % undershoot after the falling edge
    let f = |t: f64| {
        let u = t.rem_euclid(1e-6);
        if u < 0.5e-6 {
            1.0 + 0.2 * (-u / 20e-9).exp() * (2.0 * std::f64::consts::PI * u / 40e-9).cos()
        } else {
            let w = u - 0.5e-6;
            -0.05 * (-w / 20e-9).exp() * (2.0 * std::f64::consts::PI * w / 40e-9).cos()
        }
    };
    let v = sample(f, 0.0, dt, 10_000);
    let a = analyze(&v, dt);
    assert!(close(a.get(Meas::Overshoot), 20.0, 0.05), "{}", a.get(Meas::Overshoot));
    assert!(close(a.get(Meas::Preshoot), 5.0, 0.1), "{}", a.get(Meas::Preshoot));
}

#[test]
fn pulse_counts_with_missing_pulse() {
    let dt = 1e-6;
    let p = PulseTrain { every: 5, at: 2, special: Pulse { width: 0.5e-3, height: 0.0 }, ..PulseTrain::square(1e3, 0.0, 1.0, 1e-6) };
    let v = sample(|t| p.v(t), 0.0, dt, 10_500);
    let a = analyze(&v, dt);
    assert_eq!(a.get(Meas::PEdges), 9.0);
    // the 9th pulse starts at 10 ms and ends after the record: 8 complete pulses
    assert_eq!(a.get(Meas::PPulses), 8.0);
}

#[test]
fn high_low_burst_outside_and_n_periods() {
    let dt = 10e-9;
    let p = PulseTrain { width: 3e-6, ..PulseTrain::square(100e3, 0.0, 2.0, 100e-9) };
    let v = sample(|t| p.v(t), 0.0, dt, 100_000);
    let a = analyze(&v, dt);
    // 0-100 % edges are 100 ns; the 90 % and 10 % points sit 40 ns off the 50 % width
    assert!(close(a.get(Meas::HighTime), 2.92e-6, 0.02), "{}", a.get(Meas::HighTime));
    assert!(close(a.get(Meas::LowTime), 6.92e-6, 0.02), "{}", a.get(Meas::LowTime));
    assert!(close(time_outside(&a, Outside::Above), a.high_time, 1e-9));
    assert!(close(time_outside(&a, Outside::Below), a.low_time, 1e-9));
    let both = time_outside(&a, Outside::Both);
    assert!(both > a.high_time && both < a.low_time, "{}", both);
    // gaps are 3 us and 7 us: an idle of one period is a single burst, 4 us splits on the low gap
    assert!(burst_width(&a, a.period()) > 0.5e-3, "{}", burst_width(&a, a.period()));
    assert!(close(burst_width(&a, 4e-6), 3e-6, 0.02), "{}", burst_width(&a, 4e-6));
    assert!(close(a.get(Meas::NPeriods), 20e-6, 1e-4), "{}", a.get(Meas::NPeriods));
    assert!(close(duration_n(&a, 2, true), 10e-6, 0.02), "{}", duration_n(&a, 2, true));
}

#[test]
fn cycle_levels_of_a_sine() {
    let dt = 1e-6;
    let v = sample(|t| 1.5 * (2.0 * std::f64::consts::PI * 1e3 * t).sin() + 0.5, 0.0, dt, 20_000);
    let a = analyze(&v, dt);
    assert!(close(a.get(Meas::CycMax), 2.0, 0.01), "{}", a.get(Meas::CycMax));
    assert!(close(a.get(Meas::CycMin), -1.0, 0.01), "{}", a.get(Meas::CycMin));
    assert!(close(a.get(Meas::CycPp), 3.0, 0.01));
    // no flat levels: top and base fall back to the cycle max and min
    assert!(close(a.get(Meas::CycAmp), 3.0, 0.02), "{}", a.get(Meas::CycAmp));
    assert!(close(a.get(Meas::CycTop), 2.0, 0.02) && close(a.get(Meas::CycBase), -1.0, 0.02));
}

#[test]
fn skew_setup_and_hold() {
    let dt = 1e-6;
    let fa = PulseTrain::square(1e3, 0.0, 1.0, 1e-6);
    let fb = PulseTrain { offset: 125e-6, ..PulseTrain::square(1e3, 0.0, 1.0, 1e-6) };
    let a = analyze(&sample(|t| fa.v(t), 0.0, dt, 5000), dt);
    let b = analyze(&sample(|t| fb.v(t), 0.0, dt, 5000), dt);
    assert!(close(skew(&a, &b, Pair(true, true)), 125e-6, 1e-3), "{}", skew(&a, &b, Pair(true, true)));
    // data lags the clock by 125 us: setup is the rest of the period to the previous data edge, hold is the lag
    assert!(close(setup_time(&a, &b), 375e-6, 0.02), "{}", setup_time(&a, &b));
    assert!(close(hold_time(&a, &b), 125e-6, 0.02), "{}", hold_time(&a, &b));
    assert!(close(skew(&b, &a, Pair(true, true)), -125e-6, 1e-3));
    assert_eq!(setup_hold_violations(&a, &b, 0.0, 0.0), 0.0);
    assert!(setup_hold_violations(&a, &b, 400e-6, 0.0) > 0.0);
}

#[test]
fn est_ui_median_mode_and_linreg() {
    let dt = 1e-6;
    let fa = PulseTrain::square(1e3, 0.0, 1.0, 1e-6);
    let v = sample(|t| fa.v(t), 0.0, dt, 5000);
    let a = analyze(&v, dt);
    assert!(close(a.get(Meas::EstUi), 500e-6, 0.05), "{}", a.get(Meas::EstUi));
    assert!(a.get(Meas::Median).is_finite());
    assert!(a.get(Meas::Mode).is_finite());
    let ramp: Vec<f32> = (0..1000).map(|i| i as f32 * 0.01).collect();
    let r = analyze(&ramp, 1.0);
    assert!(close(r.get(Meas::LinSlope), 0.01, 0.02), "{}", r.get(Meas::LinSlope));
    assert!(r.get(Meas::LinInt).abs() < 0.05, "{}", r.get(Meas::LinInt));
    assert!(close(r.get(Meas::Median), 4.995, 0.01), "{}", r.get(Meas::Median));
    assert!(a.get(Meas::PllUi).is_finite());
}

#[test]
fn delay_and_phase() {
    let dt = 1e-6;
    let fa = PulseTrain::square(1e3, 0.0, 1.0, 1e-6);
    let fb = PulseTrain { offset: 125e-6, ..PulseTrain::square(1e3, 0.0, 1.0, 1e-6) };
    let a = analyze(&sample(|t| fa.v(t), 0.0, dt, 5000), dt);
    let b = analyze(&sample(|t| fb.v(t), 0.0, dt, 5000), dt);
    assert!(close(delay(&a, &b, Pair(true, true)), 125e-6, 1e-3));
    assert!(close(phase(&a, &b, Pair(true, true)), 45.0, 1e-3));
    assert!(close(delay(&a, &b, Pair(true, false)), 625e-6, 1e-3).then_some(()).or_else(|| close(delay(&a, &b, Pair(true, false)), -375e-6, 1e-3).then_some(())).is_some());
}


#[test]
fn no_frequency_on_noise() {
    let dt = 1e-6;
    let mut v = vec![0.0f32; 10_000];
    add_noise(&mut v, 0.03, 3);
    // without a limit the noise crossings count as edges; with 0.4 div at 1 V/div the time values are invalid
    assert!(analyze(&v, dt).get(Meas::Freq).is_finite());
    let a = analyze_min(&v, dt, 0.4);
    assert!(a.get(Meas::Freq).is_nan() && a.get(Meas::Rise).is_nan());
    assert!(a.get(Meas::Vpp) > 0.0);
}
