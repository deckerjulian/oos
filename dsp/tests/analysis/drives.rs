// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The magnetic, motor and double-pulse analysers against an ideal inductor, phase sines, quadrature and Hall lines,
//! and hard-switched pulses. A new test of these drive analysers goes here.

use super::*;

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
    let i = sample(
        |t| {
            let x = (t * f).fract();
            if x < 0.5 {
                -1.0 + 4.0 * x
            } else {
                3.0 - 4.0 * x
            }
        },
        0.0,
        dt,
        n,
    );
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

/// Forward Hall sequence: 100 samples per state, so 600 per electrical revolution; `first` replaces sample 0.
fn hall_lines(first: Option<u8>) -> (Vec<bool>, Vec<bool>, Vec<bool>) {
    let seq = [0b100u8, 0b110, 0b010, 0b011, 0b001, 0b101];
    let mut codes: Vec<u8> = (0..60).flat_map(|_| seq.iter().flat_map(|&s| std::iter::repeat_n(s, 100))).collect();
    if let Some(c) = first {
        codes[0] = c;
    }
    let line = |bit: u8| codes.iter().map(|x| x & bit != 0).collect::<Vec<_>>();
    (line(4), line(2), line(1))
}

#[test]
fn motor_hall_counts_steps_after_a_fault_code_at_the_start() {
    let dt = 1e-5;
    let (a, b, c) = hall_lines(None);
    let clean = oos_dsp::motor::shaft_hall(&a, &b, &c, dt, 0.0, 1);
    // 359 steps of 60 electrical degrees over 35 999 sample intervals
    let rpm = 359.0 * 10.0 / (35_999.0 * dt);
    assert!(close(clean.rpm.unwrap(), rpm, 1e-6), "{:?}", clean.rpm);
    for fault in [0b000, 0b111] {
        let (a, b, c) = hall_lines(Some(fault));
        let s = oos_dsp::motor::shaft_hall(&a, &b, &c, dt, 0.0, 1);
        assert!(close(s.rpm.unwrap_or(0.0), rpm, 1e-6), "{fault:03b}: {:?} {}", s.rpm, s.note);
        assert_eq!(s.direction, oos_dsp::motor::Direction::Forward);
    }
}

#[test]
fn motor_hall_speed_and_angle_are_mechanical() {
    // the Hall sensors see the rotor poles: with 4 pole pairs the shaft turns a quarter of the electrical angle
    let dt = 1e-5;
    let (a, b, c) = hall_lines(None);
    let elec = oos_dsp::motor::shaft_hall(&a, &b, &c, dt, 0.0, 1);
    let s = oos_dsp::motor::shaft_hall(&a, &b, &c, dt, 0.0, 4);
    assert!(close(s.rpm.unwrap(), elec.rpm.unwrap() / 4.0, 1e-9), "{:?} {:?}", s.rpm, elec.rpm);
    assert!(close(s.trend[0].1, elec.trend[0].1 / 4.0, 1e-9), "{:?}", s.trend[0]);
    let span = |r: &[f64]| r[r.len() - 1] - r[0];
    assert!(close(span(&elec.rad), (359.0 * 60.0f64).to_radians(), 1e-9), "{}", span(&elec.rad));
    assert!(close(span(&s.rad), span(&elec.rad) / 4.0, 1e-9), "{}", span(&s.rad));
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
    let gate = [
        (0.0, 0.0),
        (1e-6, 0.0),
        (1.02e-6, 10.0),
        (6e-6, 10.0),
        (6.02e-6, 0.0),
        (10e-6, 0.0),
        (10.02e-6, 10.0),
        (12e-6, 10.0),
        (12.02e-6, 0.0),
    ];
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
    let vds = [
        (0.0, 400.0),
        (1.05e-6, 400.0),
        (1.15e-6, 1.0),
        (6.1e-6, 1.0),
        (6.2e-6, 400.0),
        (6.25e-6, 480.0),
        (6.4e-6, 400.0),
    ];
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

#[test]
fn double_pulse_load_current_is_the_current_at_turn_off_while_it_ramps() {
    // an inductive load: Id ramps 0 -> 20 A over the first pulse, freewheels, and the second pulse switches 20 A
    let dt = 1e-9;
    let n = 13_000;
    let gate = [
        (0.0, 0.0),
        (1e-6, 0.0),
        (1.02e-6, 10.0),
        (6e-6, 10.0),
        (6.02e-6, 0.0),
        (10e-6, 0.0),
        (10.02e-6, 10.0),
        (12e-6, 10.0),
        (12.02e-6, 0.0),
    ];
    let id = [
        (0.0, 0.0),
        (1.1e-6, 0.0),
        (6.05e-6, 20.0),
        (6.15e-6, 0.0),
        (10.05e-6, 0.0),
        (10.15e-6, 20.0),
        (12e-6, 22.0),
        (12.1e-6, 0.0),
    ];
    let vds = [
        (0.0, 400.0),
        (1.05e-6, 400.0),
        (1.15e-6, 1.0),
        (6.1e-6, 1.0),
        (6.2e-6, 400.0),
        (10.05e-6, 400.0),
        (10.15e-6, 1.0),
        (12e-6, 1.0),
    ];
    let g = sample(|t| pwl(&gate, t), 0.0, dt, n);
    let i = sample(|t| pwl(&id, t), 0.0, dt, n);
    let v = sample(|t| pwl(&vds, t), 0.0, dt, n);
    let p = oos_dsp::dpt::measure(&v, &i, Some(&g), oos_dsp::dpt::Fourth::Off, dt, 10.0);
    // the gate leaves 90 % at 6.002 µs, where the ramp is at 19.8 A
    assert!(close(p.il.unwrap(), 19.8, 0.1), "il {:?}", p.il);
    // both current edges run 0 <-> 20 A in 100 ns: 10-90 % of the switched current is 80 ns
    assert!(close(p.tr.unwrap(), 80e-9, 2e-9), "tr {:?}", p.tr);
    assert!(close(p.tf.unwrap(), 80e-9, 2e-9), "tf {:?}", p.tf);
    // the current falls from 20 A at 6.05 µs and passes 90 % of I_L at 6.061 µs, 59 ns after the gate's 90 %
    assert!(close(p.td_off.unwrap(), 59e-9, 2e-9), "td off {:?}", p.td_off);
    assert!(close(p.i_over.unwrap(), 22.0 - 19.8, 0.1), "Ipeak - I_L {:?}", p.i_over);
}
