// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Power analysis against loads with known answers: power factor, harmonics against class A, switching loss, ripple,
//! inrush, efficiency, slew, turn times, capacitance and rail, and Bode PSRR and impedance. A new power test goes
//! here.

use super::*;

// ------------------------------------------------------------------------------------------------ power

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
    assert!(
        close(q.pf, 1.0, 1e-3) && close(q.phase_deg, 0.0, 0.1) && q.q_reactive.abs() < 0.03 * q.s_apparent,
        "{:?}",
        q
    );
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
    assert!(close(q.pf, FRAC_1_SQRT_2, 0.003), "{:?}", q);
    assert!(
        close(q.s_apparent, s, 0.5)
            && close(q.p_real, s * FRAC_1_SQRT_2, 2.0)
            && close(q.q_reactive, s * FRAC_1_SQRT_2, 2.0),
        "{:?}",
        q
    );
    // capacitive: negative phase and reactive power
    let i = sample(|t| ip * (w * t + PI / 6.0).sin(), 0.0, dt, 10_000);
    let q = power::quality(&v, &i, dt, None);
    assert!(close(q.phase_deg, -30.0, 0.2) && q.q_reactive < 0.0, "{:?}", q);
}

#[test]
fn power_current_harmonics_against_class_a() {
    let dt = 20e-6;
    let w = 2.0 * PI * 50.0;
    let i = sample(
        |t| {
            5.0 * (w * t).sin()
                + 3.0 * (3.0 * w * t + 0.4).sin()
                + 0.5 * (5.0 * w * t).sin()
                + 1.5 * (7.0 * w * t).sin()
        },
        0.0,
        dt,
        20_000,
    );
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
    let v = sample(
        |t| {
            let k = (t * f).floor();
            let d = 0.2 + 0.6 * k / 49.0;
            if t * f - k < d {
                3.3
            } else {
                0.0
            }
        },
        0.0,
        1e-7,
        50 * 1000,
    );
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
    assert!(
        close(r.overshoot, 1.0, 1e-9)
            && close(r.undershoot, 1.0, 1e-9)
            && close(r.dc, 5.0, 1e-9)
            && close(r.turn_on, 6.0, 1e-9),
        "{:?}",
        r
    );
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
