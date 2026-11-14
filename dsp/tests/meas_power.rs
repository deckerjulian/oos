// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Switching slew and Rds(on), rail ripple and turn times, PSRR / impedance Bode modes and ACPR.

use oos_dsp::power;
use oos_dsp::sim::sample;
use oos_dsp::spectrum;
use std::f64::consts::PI;


fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
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
    let dt_r = 1e-7;
    let ripple: Vec<f32> = (0..20_000)
        .map(|k| {
            let t = k as f64 * dt_r;
            ((2.0 * std::f64::consts::PI * 1e5 * t).sin() + 0.25 * (2.0 * std::f64::consts::PI * 1e6 * t).sin()) as f32
        })
        .collect();
    let line = power::switching_ripple(&ripple, dt_r, 1e5);
    assert!(close(line.pp, 2.0, 0.02) && close(line.rms, 1.0 / 2f64.sqrt(), 0.02), "{line:?}");
    let clamped = power::switching_ripple(&ripple, dt_r, 5e6);
    assert!(close(clamped.pp, 0.5, 0.05), "1 MHz clamp {clamped:?}");
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
