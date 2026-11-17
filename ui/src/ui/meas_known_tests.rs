// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later
//! The module measurements read from signals whose figures are known. Each signal is built into reference slots, as a
//! SIM waveform or a loaded file would be, and the reading is compared with the value worked out from how the signal
//! was made.

use super::*;
use std::f64::consts::PI;

/// The catalog entry `id` on the signals `refs` (R1, R2 ... with their sample step): (value, text).
fn read(refs: &[(Vec<f32>, f64)], id: u16, prep: &dyn Fn(&mut Ui)) -> (f64, String) {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.run = RunState::Stop;
    u.m.ch.iter_mut().for_each(|c| c.on = false);
    u.m.meas.clear();
    u.m.meas2.clear();
    u.m.cat.clear();
    u.m.meas_full_record = true;
    for (r, (v, dt)) in refs.iter().enumerate() {
        let t0 = -0.5 * v.len() as f64 * dt;
        u.refs[r] = Some(RefWave { src: 0, color: r, label: String::new(), t0, dt: *dt, v: v.clone().into(), vpd: 1.0, offset: 0.0, on: true });
    }
    (u.meas_src, u.meas_src_b, u.cat_c, u.cat_d) = (WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3));
    prep(&mut u);
    for _ in 0..3 { u.step(0.04); }
    let def = crate::measures::all().map(|(_, d)| d).find(|d| d.id == id).unwrap();
    u.catalog_add(def);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        u.step(0.04);
        let rows = u.meas_rows();
        if rows.iter().all(|r| !r.computing) || std::time::Instant::now() > end {
            return rows.first().map_or((f64::NAN, "NO ROW".into()), |r| (r.v, r.text()));
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn wave(n: usize, dt: f64, f: impl Fn(f64) -> f64) -> (Vec<f32>, f64) {
    ((0..n).map(|i| f(i as f64 * dt) as f32).collect(), dt)
}

/// PRBS7 bits.
fn prbs7(n: usize) -> Vec<bool> {
    let mut s = 0x7fu8;
    (0..n).map(|_| { let b = ((s >> 6) ^ (s >> 5)) & 1; s = ((s << 1) | b) & 0x7f; b == 1 }).collect()
}

/// An NRZ signal of `bits`, 0 to 1 V, `ui` seconds a bit, linear edges of `edge` seconds centred on the bit boundary
/// shifted by `shift(boundary time)`.
fn nrz(bits: &[bool], ui: f64, edge: f64, dt: f64, shift: impl Fn(f64) -> f64) -> (Vec<f32>, f64) {
    let n = (bits.len() as f64 * ui / dt) as usize;
    let bound: Vec<f64> = (0..=bits.len()).map(|k| k as f64 * ui + shift(k as f64 * ui)).collect();
    wave(n, dt, |t| {
        let k = ((t / ui) as usize).min(bits.len() - 1);
        // the nearer boundary of this bit
        let (b, prev, next) = if t - bound[k] < bound[k + 1] - t { (bound[k], if k > 0 { bits[k - 1] } else { bits[k] }, bits[k]) }
            else { (bound[k + 1], bits[k], if k + 1 < bits.len() { bits[k + 1] } else { bits[k] }) };
        let x = ((t - b) / edge + 0.5).clamp(0.0, 1.0);
        let (p, q) = (prev as u8 as f64, next as u8 as f64);
        p + (q - p) * x
    })
}

/// Each row within 6 % of its value, or within `floor` of it (for a value that is zero).
fn show(group: &str, floor: f64, rows: &[(u16, &str, f64)], refs: &[(Vec<f32>, f64)], prep: &dyn Fn(&mut Ui)) -> Vec<String> {
    let mut bad = Vec::new();
    for &(id, name, want) in rows {
        let (v, text) = read(refs, id, prep);
        let ok = want.is_nan() || (v - want).abs() <= want.abs() * 0.06 + floor;
        println!("MK|{group}|{id}|{name}|{text}|{v:e}|{}|{}", if want.is_nan() { "-".into() } else { format!("{want:e}") }, if want.is_nan() { "?" } else if ok { "ok" } else { "BAD" });
        if !ok { bad.push(format!("{group} {name}: {text} (want {want:e})")); }
    }
    bad
}

const Q: f64 = f64::NAN;

#[test]
fn the_module_measurements_read_known_signals() {
    let mut bad = Vec::new();
    // power: 50 Hz, 10 V peak; the current 2 A peak 60 degrees behind, with 10 % third harmonic; ten cycles
    let w = 2.0 * PI * 50.0;
    let v = wave(20_000, 10e-6, |t| 10.0 * (w * t).sin());
    let i = wave(20_000, 10e-6, |t| 2.0 * (w * t - PI / 3.0).sin() + 0.2 * (3.0 * w * t).sin());
    let ipk = i.0.iter().fold(0f32, |a, x| a.max(x.abs())) as f64;
    let irms = (2.0f64 + 0.02).sqrt();
    bad.extend(show("power", 1e-3, &[(90, "Power Quality", 5.0), (118, "Power Factor", 5.0 / (10.0 / 2f64.sqrt() * irms)), (91, "Harmonics", 10.0),
        (92, "Inrush Current", ipk), (1000, "Crest Factor", 2f64.sqrt()), (1001, "Form Factor", PI / (2.0 * 2f64.sqrt())), (113, "Ripple", 20.0),
        (153, "DC Rail", 0.0), (116, "PSRR", 20.0 * 5f64.log10()), (117, "Impedance", 0.2)], &[v, i], &|_| {}));
    // a rail: 5 V with 50 mV of 100 Hz
    let rail = wave(20_000, 10e-6, |t| 5.0 + 0.05 * (2.0 * PI * 100.0 * t).sin());
    bad.extend(show("rail", 1e-3, &[(153, "DC Rail", 5.0), (113, "Ripple", 0.1), (150, "Overshoot", 0.05), (151, "Undershoot", 0.05)], &[rail], &|u| u.m.pw_ref = Some(5.0)));
    // NRZ: PRBS7 at 1 Mb/s, 0 to 1 V, 100 ns edges, 100 samples a bit; the second source is its complement
    let bits = prbs7(2000);
    let d = nrz(&bits, 1e-6, 100e-9, 10e-9, |_| 0.0);
    let dn = (d.0.iter().map(|x| 1.0 - x).collect::<Vec<f32>>(), d.1);
    bad.extend(show("nrz", 0.01, &[(15, "Unit Interval", 1e-6), (16, "NRZ Data Rate", 1e6), (80, "NRZ Clock Rate", 1e6), (70, "Bit High", 1.0), (71, "Bit Low", 0.0),
        (72, "Bit Amplitude", 1.0), (62, "Eye High", 1.0), (65, "Eye Low", 0.0), (60, "Eye Height", 1.0), (61, "Eye Width", Q), (63, "Eye Height@BER", Q),
        (64, "Eye Width@BER", Q), (66, "Q-Factor", Q), (73, "DC Common Mode", 0.5), (74, "AC Common Mode (Pk-Pk)", 0.0), (75, "Differential Crossover", 0.5),
        (76, "T/nT Ratio", Q), (41, "TIE", Q), (40, "Jitter Summary", Q), (43, "TJ@BER", Q), (44, "RJ-dd", Q), (45, "DJ-dd", Q), (48, "DDJ", Q), (49, "DCD", Q),
        (81, "SSC Freq Dev", Q), (82, "SSC Modulation Rate", Q)], &[d, dn], &|_| {}));
    // jitter: a 1 Mb/s clock pattern whose edges move by 20 ns * sin(2 pi 10 kHz t)
    let clock: Vec<bool> = (0..4000).map(|k| k % 2 == 0).collect();
    let j = nrz(&clock, 1e-6, 100e-9, 10e-9, |t| 20e-9 * (2.0 * PI * 10e3 * t).sin());
    bad.extend(show("jitter", 1e-12, &[(41, "TIE", 40e-9), (46, "SJ@Freq", 20e-9 / 2f64.sqrt()), (47, "PJ", 20e-9 / 2f64.sqrt()), (40, "Jitter Summary", Q), (43, "TJ@BER", Q), (44, "RJ-dd", Q), (45, "DJ-dd", Q),
        (42, "Phase Noise", Q), (49, "DCD", Q), (48, "DDJ", Q), (50, "SRJ", Q), (51, "J2", Q), (15, "Unit Interval", 1e-6)], &[j], &|_| {}));
    // duty-cycle distortion: every high bit of the clock pattern is 40 ns longer
    let dcd = nrz(&clock, 1e-6, 100e-9, 10e-9, |t| if ((t / 1e-6).round() as i64) % 2 == 0 { -20e-9 } else { 20e-9 });
    bad.extend(show("dcd", 1e-12, &[(49, "DCD", -40e-9), (41, "TIE", 40e-9)], &[dcd], &|_| {}));
    // magnetic: +-1 V square at 1 kHz across an inductor of 1 mH: the current is a triangle of 1000 A/s
    let mv = wave(20_000, 1e-6, |t| if (t * 1e3).rem_euclid(1.0) < 0.5 { 1.0 } else { -1.0 });
    let mi = wave(20_000, 1e-6, |t| { let p = (t * 1e3).rem_euclid(1.0); if p < 0.5 { -0.25 + p * 1.0 } else { 0.25 - (p - 0.5) * 1.0 } });
    bad.extend(show("magnetic", 1e-9, &[(110, "Inductance", 1e-3), (112, "Magnetic Loss", Q), (111, "Magnetic Property", Q)], &[mv, mi], &|_| {}));
    // motor: three phases 120 degrees apart at 50 Hz, a torque signal of 2 V
    let ph = |k: f64| wave(20_000, 10e-6, move |t| (w * t - k * 2.0 * PI / 3.0).sin());
    let tq = wave(20_000, 10e-6, |_| 2.0);
    bad.extend(show("motor", 1e-3, &[(131, "Speed", 3000.0), (133, "Direction", 1.0), (132, "Acceleration", 0.0), (134, "Angle", Q), (130, "DQ0", 1.0), (135, "Torque", 2.0),
        (136, "Mechanical Power", 2.0 * w), (137, "System Efficiency", Q)], &[ph(0.0), ph(1.0), ph(2.0), tq], &|_| {}));
    let rev = show("motor-rev", 1e-3, &[(133, "Direction", -1.0)], &[ph(0.0), ph(2.0), ph(1.0)], &|_| {});
    bad.extend(rev);
    // a switch: 10 V off, 0.1 V on while 2 A flows; edges of 10 us; 1 kHz, on for half the time
    let sw = |hi: f64, lo: f64, inv: bool| wave(20_000, 1e-6, move |t| {
        let p = (t * 1e3 + 0.25).rem_euclid(1.0) * 1e-3;
        let x = if p < 10e-6 { p / 10e-6 } else if p < 500e-6 { 1.0 } else if p < 510e-6 { 1.0 - (p - 500e-6) / 10e-6 } else { 0.0 };
        let x = if inv { 1.0 - x } else { x };
        lo + (hi - lo) * x
    });
    let (vds, id) = (sw(10.0, 0.1, true), sw(2.0, 0.0, false));
    bad.extend(show("switch", 1e-9, &[(109, "RDS(on)", 0.05), (105, "dv/dt rise", 9.9e5), (106, "dv/dt fall", -9.9e5), (107, "di/dt rise", 2e5), (108, "di/dt fall", -2e5),
        (102, "E on", Q), (103, "E off", Q), (104, "E cond", 0.1 * 2.0 * 490e-6), (100, "Turn On Time", Q), (101, "Turn Off Time", Q)], &[vds, id], &|_| {}));
    // input capacitance: the voltage ramps from 0 to 10 V in 1 ms into 1 uF, so 10 mA flows while it does
    let cv = wave(4000, 1e-6, |t| ((t - 1e-3) / 1e-3).clamp(0.0, 1.0) * 10.0);
    let ci = wave(4000, 1e-6, |t| if (1e-3..2e-3).contains(&t) { 0.01 } else { 0.0 });
    bad.extend(show("cap", 1e-12, &[(93, "Input Capacitance", 1e-6)], &[cv, ci], &|_| {}));
    // sequencing: the enable (second source) rises at 1 ms, the rail (first) ramps from 1.1 ms and is at its top at 1.5 ms;
    // the enable falls at 3 ms and the rail is at its base at 3.3 ms
    let en = wave(5000, 1e-6, |t| if (1e-3..3e-3).contains(&t) { 3.3 } else { 0.0 });
    let rl = wave(5000, 1e-6, |t| if t < 3e-3 { ((t - 1.1e-3) / 0.4e-3).clamp(0.0, 1.0) * 5.0 } else { (1.0 - (t - 3e-3) / 0.3e-3).clamp(0.0, 1.0) * 5.0 });
    bad.extend(show("sequence", 1e-9, &[(100, "Turn On Time", 0.5e-3), (101, "Turn Off Time", 0.3e-3), (152, "Turn-on Overshoot", Q)], &[rl, en], &|_| {}));
    // double pulse: bus 100 V; the gate is on from 100 to 300 us and from 400 to 500 us. The current ramps to 10 A in the
    // first pulse and falls in 1 us, 3 us after the gate; the voltage rises in 2 us from there and peaks at 120 V. The
    // second turn-on starts 2 us after the gate, rises in 1 us, overshoots to 12 A (the diode's recovery, 1 us wide) and
    // then the inductor ramps on to 14.8 A.
    let dp = 50e-9;
    let gate = wave(12_000, dp, |t| if (100e-6..300e-6).contains(&t) || (400e-6..500e-6).contains(&t) { 10.0 } else { 0.0 });
    let id = wave(12_000, dp, |t| {
        let fall = |t0: f64, from: f64| from * (1.0 - ((t - t0 - 3e-6) / 1e-6).clamp(0.0, 1.0));
        if t < 100e-6 { 0.0 }
        else if t < 300e-6 { (t - 100e-6) * 50e3 }
        else if t < 402e-6 { fall(300e-6, 10.0) }
        else if t < 403e-6 { (t - 402e-6) / 1e-6 * 10.0 }
        else if t < 404e-6 { 10.0 + 2.0 * (1.0 - ((t - 403.5e-6) / 0.5e-6).abs()) }
        else if t < 500e-6 { 10.0 + (t - 404e-6) * 50e3 }
        else { fall(500e-6, 14.8) }
    });
    let vds = wave(12_000, dp, |t| {
        let off = |t0: f64| if t < t0 + 3e-6 { 0.0 } else if t < t0 + 5e-6 { (t - t0 - 3e-6) / 2e-6 * 100.0 } else if t < t0 + 7e-6 { 100.0 + 20.0 * (1.0 - ((t - t0 - 6e-6) / 1e-6).abs()) } else { 100.0 };
        if t < 100e-6 { 100.0 } else if t < 300e-6 { 0.0 } else if t < 403e-6 { if t < 402e-6 { off(300e-6) } else { 100.0 * (1.0 - (t - 402e-6) / 1e-6) } } else if t < 500e-6 { 0.0 } else { off(500e-6) }
    });
    bad.extend(show("dpt", 1e-12, &[(170, "Vpeak", 120.0), (171, "Ipeak", 12.0), (172, "Td(on)", 2.1e-6), (173, "Tr", 0.8e-6), (174, "Ton", 2.9e-6), (175, "Td(off)", 3.1e-6),
        (176, "Tf", 0.8e-6), (177, "Toff", 3.9e-6), (178, "dv/dt", 50e6), (179, "di/dt", 10e6), (184, "Irrm", 2.0), (181, "Trr", 0.95e-6), (182, "Qrr", 0.995e-6),
        (185, "Diode d/dt", 4e6), (183, "Err", 100.0 * 0.995e-6), (186, "Qoss", Q), (180, "Tdt", Q)], &[vds, id, gate], &|_| {}));
    // skew, setup and hold work in nanoseconds: a 10 MHz clock, a second one 3 ns later, and data that rises 4 ns before
    // each rising clock edge and falls 6 ns after it
    let ns = 0.1e-9;
    let pulse = move |start: f64, len: f64| wave(20_000, ns, move |t| {
        let p = (t - start).rem_euclid(100e-9);
        if p < 0.5e-9 { p / 0.5e-9 } else if p < len { 1.0 } else if p < len + 0.5e-9 { 1.0 - (p - len) / 0.5e-9 } else { 0.0 }
    });
    let (clk, late, data) = (pulse(20e-9, 50e-9), pulse(23e-9, 50e-9), pulse(16e-9, 10e-9));
    bad.extend(show("skew", 1e-12, &[(19, "Skew rise-rise", 3e-9), (236, "Skew fall-fall", 3e-9)], &[clk.clone(), late], &|_| {}));
    // source A is the data, source B the clock; the third row counts violations
    bad.extend(show("setup", 1e-12, &[(30, "Setup Time", 4e-9), (31, "Hold Time", 6e-9), (239, "Setup + Hold", 0.0)], &[data, clk], &|_| {}));
    assert!(bad.is_empty(), "{bad:#?}");
}
