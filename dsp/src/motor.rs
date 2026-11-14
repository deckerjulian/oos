// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Inverter and motor measurements. DQ0 is the amplitude-invariant Clarke transform followed by Park.
//! Speed, acceleration, direction and angle come from a shaft source; torque and the electrical pair are optional.
//! `oos-dsp` has no logic-analyser type: quadrature and Hall arrive as boolean samples.

use crate::power::{freq_of, whole_cycles};
use std::f64::consts::PI;

/// Sign of the mean shaft speed. Stopped is below 1 rpm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
    Stopped,
}

impl Default for Direction {
    fn default() -> Self {
        Direction::Stopped
    }
}

/// Shaft motion over one record. `rpm` is mechanical. `synchronous` is the stator field, not the shaft.
#[derive(Clone, Debug)]
pub struct Shaft {
    pub rpm: Option<f64>,
    pub accel: Option<f64>,
    /// rpm/s, slope of the per-revolution speed
    pub direction: Direction,
    /// wrapped shaft angle at the trigger, degrees
    pub angle_deg: Option<f64>,
    /// end minus start, degrees
    pub travel_deg: Option<f64>,
    pub synchronous: bool,
    /// an index input is connected and has not been seen
    pub no_index: bool,
    pub hall_faults: u32,
    /// (time s, rpm) once per revolution
    pub trend: Vec<(f64, f64)>,
    /// mechanical radians at each sample. NaN is a gap. Empty when the shaft was not decoded.
    pub rad: Vec<f64>,
    pub note: &'static str,
}

impl Default for Shaft {
    fn default() -> Self {
        Shaft {
            rpm: None,
            accel: None,
            direction: Direction::Stopped,
            angle_deg: None,
            travel_deg: None,
            synchronous: false,
            no_index: false,
            hall_faults: 0,
            trend: Vec::new(),
            rad: Vec::new(),
            note: "",
        }
    }
}

/// Means of D, Q and zero over the whole cycles of phase A, plus the decimated waveforms.
#[derive(Clone, Debug)]
pub struct Dq0 {
    pub d: f64,
    pub q: f64,
    pub zero: f64,
    pub freq: f64,
    /// fundamental above 0.1 / dt: the cycle window is short
    pub short: bool,
    /// θ was unwrap(atan2(β, α))
    pub from_vector: bool,
    /// decimated (d, q, zero)
    pub wave: Vec<(f64, f64, f64)>,
    /// decimated (α, β)
    pub ab: Vec<(f64, f64)>,
}

/// Torque, mechanical power and the ratio to one electrical pair.
#[derive(Clone, Debug)]
pub struct Mech {
    pub shaft: Shaft,
    pub torque: Option<f64>,
    pub p_mech: Option<f64>,
    pub p_elec: Option<f64>,
    /// mechanical / electrical. Signed, and not clamped to 0..1.
    pub eff: Option<f64>,
    /// negative mechanical power with positive electrical power
    pub generating: bool,
}

/// Where θ comes from. `Elec` is electrical radians, one per sample; NaN is a gap.
pub enum Theta<'a> {
    /// θ = atan2(β, α), so D sits on the vector
    Vector,
    Elec(&'a [f64]),
}

/// DQ0 of three phases. `None` when a phase is missing or nothing finite is left.
/// The reported values are the means over the whole cycles of phase A.
pub fn dq0(a: &[f32], b: &[f32], c: &[f32], dt: f64, theta: Theta<'_>) -> Option<Dq0> {
    let n = a.len().min(b.len()).min(c.len());
    let n = match theta {
        Theta::Elec(th) => n.min(th.len()),
        Theta::Vector => n,
    };
    if n < 4 || !(dt > 0.0) {
        return None;
    }
    let freq = freq_of(&a[..n], dt).unwrap_or(0.0);
    let (w0, w1, _) = if freq > 0.0 { whole_cycles(n, dt, freq) } else { (0, n, 0) };
    let stride = (n / 1500).max(1);
    let (mut sd, mut sq, mut sz, mut cnt) = (0.0, 0.0, 0.0, 0usize);
    let mut wave = Vec::new();
    let mut ab = Vec::new();
    for k in 0..n {
        let (va, vb, vc) = (a[k] as f64, b[k] as f64, c[k] as f64);
        if !(va.is_finite() && vb.is_finite() && vc.is_finite()) {
            continue;
        }
        let alpha = (2.0 / 3.0) * (va - 0.5 * vb - 0.5 * vc);
        let beta = (2.0 / 3.0) * (3.0_f64.sqrt() / 2.0) * (vb - vc);
        let zero = (va + vb + vc) / 3.0;
        let th = match theta {
            Theta::Vector => beta.atan2(alpha),
            Theta::Elec(th) => th[k],
        };
        if !th.is_finite() {
            continue;
        }
        let (ct, st) = (th.cos(), th.sin());
        let d = alpha * ct + beta * st;
        let q = -alpha * st + beta * ct;
        if k % stride == 0 {
            wave.push((d, q, zero));
            ab.push((alpha, beta));
        }
        if (w0..w1).contains(&k) {
            sd += d;
            sq += q;
            sz += zero;
            cnt += 1;
        }
    }
    if cnt == 0 {
        return None;
    }
    let c = cnt as f64;
    Some(Dq0 {
        d: sd / c,
        q: sq / c,
        zero: sz / c,
        freq,
        short: freq > 0.1 / dt,
        from_vector: matches!(theta, Theta::Vector),
        wave,
        ab,
    })
}

/// Electrical radians for Park from an analog angle. A mechanical source is multiplied by the pole pairs.
pub fn analog_elec(v: &[f32], deg_per_volt: f64, offset_deg: f64, electrical: bool, poles: u32) -> Vec<f64> {
    let poles = poles.max(1) as f64;
    let scale = if electrical { 1.0 } else { poles };
    v.iter()
        .map(|&x| if x.is_finite() { (deg_per_volt * x as f64 + offset_deg).to_radians() * scale } else { f64::NAN })
        .collect()
}

/// Synchronous mechanical speed of a three-phase set: θ = unwrap(atan2(β, α)), then divided by the pole pairs.
pub fn shaft_phase(a: &[f32], b: &[f32], c: &[f32], dt: f64, t0: f64, poles: u32) -> Shaft {
    let n = a.len().min(b.len()).min(c.len());
    if n < 4 || !(dt > 0.0) {
        return Shaft { note: "no phase set", ..Shaft::default() };
    }
    let poles = poles.max(1) as f64;
    let mut elec = Vec::with_capacity(n);
    let mut prev: Option<f64> = None;
    let mut acc = 0.0;
    for k in 0..n {
        let (va, vb, vc) = (a[k] as f64, b[k] as f64, c[k] as f64);
        if !(va.is_finite() && vb.is_finite() && vc.is_finite()) {
            elec.push(f64::NAN);
            prev = None;
            continue;
        }
        let alpha = (2.0 / 3.0) * (va - 0.5 * vb - 0.5 * vc);
        let beta = (2.0 / 3.0) * (3.0_f64.sqrt() / 2.0) * (vb - vc);
        let raw = beta.atan2(alpha);
        if let Some(p) = prev {
            let mut d = raw - p;
            if d > PI {
                d -= 2.0 * PI;
            }
            if d < -PI {
                d += 2.0 * PI;
            }
            acc += d;
        } else {
            acc = raw;
        }
        prev = Some(raw);
        elec.push(acc / poles);
    }
    let mut s = from_unwrapped(&elec, dt, t0, None);
    s.synchronous = true;
    s.rad = elec;
    s
}

/// Analog angle. `electrical` means the volts are an electrical angle and pole pairs convert it to the shaft.
pub fn shaft_analog(v: &[f32], dt: f64, t0: f64, deg_per_volt: f64, offset_deg: f64, electrical: bool, poles: u32) -> Shaft {
    if v.len() < 2 || !(dt > 0.0) || !deg_per_volt.is_finite() {
        return Shaft { note: "no angle", ..Shaft::default() };
    }
    let poles = poles.max(1) as f64;
    let to_shaft = if electrical { 1.0 / poles } else { 1.0 };
    let mut prev_deg: Option<f64> = None;
    let mut acc = 0.0;
    let rad: Vec<f64> = v
        .iter()
        .map(|&x| {
            if !x.is_finite() {
                prev_deg = None;
                return f64::NAN;
            }
            let deg = deg_per_volt * x as f64 + offset_deg;
            if let Some(p) = prev_deg {
                let mut d = deg - p;
                // a wrapped sensor jumps by about ±360; a ramp does not
                if d > 180.0 {
                    d -= 360.0;
                }
                if d < -180.0 {
                    d += 360.0;
                }
                acc += d;
            } else {
                acc = deg;
            }
            prev_deg = Some(deg);
            acc.to_radians() * to_shaft
        })
        .collect();
    let mut s = from_unwrapped(&rad, dt, t0, None);
    s.rad = rad;
    s
}

/// Quadrature decoder. A leading B is forward; `swap` reverses that. Index, once seen, zeros the angle.
pub fn shaft_quad(a: &[bool], b: &[bool], index: Option<&[bool]>, dt: f64, t0: f64, counts_per_rev: f64, swap: bool) -> Shaft {
    let n = a.len().min(b.len());
    if n < 2 || !(dt > 0.0) || !(counts_per_rev > 0.0) {
        return Shaft { note: "no encoder", ..Shaft::default() };
    }
    let mut count = 0.0;
    let mut edges = 0u32;
    let mut zero: Option<f64> = None;
    let mut prev_i = false;
    let mut rad = Vec::with_capacity(n);
    let mut trend = Vec::new();
    let mut acc = 0.0;
    let mut t_rev = 0.0;
    for k in 0..n {
        if k > 0 {
            let step = quad_step(a[k - 1], b[k - 1], a[k], b[k], swap);
            if step != 0 {
                edges += 1;
                count += step as f64;
                acc += step as f64;
                if acc.abs() + 1e-9 >= counts_per_rev {
                    let t = k as f64 * dt;
                    let rpm = acc.signum() * 60.0 / (t - t_rev).max(dt);
                    trend.push((t, rpm));
                    t_rev = t;
                    acc -= acc.signum() * counts_per_rev;
                }
            }
        }
        if let Some(ix) = index {
            let bit = ix.get(k).copied().unwrap_or(false);
            if bit && !prev_i && zero.is_none() {
                zero = Some(count);
            }
            prev_i = bit;
        }
        rad.push(count * 2.0 * PI / counts_per_rev);
    }
    let rpm = count / ((n - 1) as f64 * dt) / counts_per_rev * 60.0;
    let mut s = from_unwrapped(&rad, dt, t0, Some(edges));
    // the reported speed is the count rate, not the derivative of the stepped angle
    s.rpm = Some(rpm);
    s.direction = direction_of(rpm);
    s.trend = trend;
    s.accel = if edges < 2 || s.trend.len() < 2 { None } else { slope_of(&s.trend) };
    s.no_index = index.is_some() && zero.is_none();
    if let Some(z) = zero {
        let k = ((-t0 / dt).round()).clamp(0.0, (n - 1) as f64) as usize;
        let count_k = rad[k] / (2.0 * PI) * counts_per_rev;
        s.angle_deg = Some((count_k - z) * 360.0 / counts_per_rev).map(|d| d.rem_euclid(360.0));
    }
    if edges == 0 {
        s.note = "no encoder edges";
    }
    s.rad = rad;
    s
}

/// Three Hall lines. The order 100, 110, 010, 011, 001, 101 (A is the high bit) is forward.
/// Codes 000 and 111 hold the last direction and count a fault.
pub fn shaft_hall(a: &[bool], b: &[bool], c: &[bool], dt: f64, t0: f64) -> Shaft {
    let n = a.len().min(b.len()).min(c.len());
    if n < 2 || !(dt > 0.0) {
        return Shaft { note: "no hall", ..Shaft::default() };
    }
    let code = |k: usize| (a[k] as u8) << 2 | (b[k] as u8) << 1 | c[k] as u8;
    let mut faults = 0u32;
    let mut steps = 0i32;
    let mut edges = 0u32;
    let mut rad = vec![f64::NAN; n];
    let mut acc = 0.0;
    let mut have = false;
    let mut prev = code(0);
    if let Some(deg) = hall_deg(prev) {
        acc = deg.to_radians();
        have = true;
        rad[0] = acc;
    }
    let mut in_fault = matches!(prev, 0 | 7);
    let mut trend = Vec::new();
    let mut step_acc = 0i32;
    let mut t_rev = 0.0;
    for k in 1..n {
        let cur = code(k);
        if cur == 0 || cur == 7 {
            if !in_fault {
                faults += 1;
                in_fault = true;
            }
            rad[k] = if have { acc } else { f64::NAN };
            continue;
        }
        in_fault = false;
        if let Some(dir) = hall_step(prev, cur) {
            if dir != 0 {
                edges += 1;
                steps += dir;
                acc += dir as f64 * 60.0_f64.to_radians();
                have = true;
                step_acc += dir;
                if step_acc.abs() >= 6 {
                    let t = k as f64 * dt;
                    let rpm = (step_acc.signum() as f64) * 60.0 / (t - t_rev).max(dt);
                    trend.push((t, rpm));
                    t_rev = t;
                    step_acc -= step_acc.signum() * 6;
                }
            }
            prev = cur;
        }
        rad[k] = if have { acc } else { f64::NAN };
    }
    let rpm = steps as f64 * 10.0 / ((n - 1) as f64 * dt);
    let mut s = from_unwrapped(&rad, dt, t0, Some(edges));
    s.rpm = if edges == 0 { None } else { Some(rpm) };
    s.direction = direction_of(s.rpm.unwrap_or(0.0));
    s.hall_faults = faults;
    s.trend = trend;
    s.accel = if edges < 2 || s.trend.len() < 2 { None } else { slope_of(&s.trend) };
    if edges == 0 {
        s.note = "no hall edges";
    }
    s.rad = rad;
    s
}

/// Torque (mean times the N·m scale), mechanical power and system efficiency.
/// Electrical power is the mean of v·i. The ratio is empty when that mean is 0.
pub fn mechanical(shaft: Shaft, torque: Option<&[f32]>, scale: f64, vin: Option<&[f32]>, iin: Option<&[f32]>) -> Mech {
    let torque = torque.and_then(|t| mean_finite(t)).map(|m| m * scale);
    let p_elec = match (vin, iin) {
        (Some(v), Some(i)) => mean_product(v, i),
        _ => None,
    };
    let p_mech = match (torque, shaft.rpm) {
        (Some(t), Some(w)) => Some(t * w * 2.0 * PI / 60.0),
        _ => None,
    };
    let eff = match (p_mech, p_elec) {
        (Some(pm), Some(pe)) if pe != 0.0 => Some(pm / pe),
        _ => None,
    };
    let generating = p_mech.is_some_and(|p| p < 0.0) && p_elec.is_some_and(|p| p > 0.0);
    Mech { shaft, torque, p_mech, p_elec, eff, generating }
}

fn direction_of(rpm: f64) -> Direction {
    if !rpm.is_finite() || rpm.abs() < 1.0 { Direction::Stopped } else if rpm > 0.0 { Direction::Forward } else { Direction::Reverse }
}

/// Mechanical radians, NaN splits the unwrap. `edges` set means acceleration also needs two edges (encoder, Hall).
fn from_unwrapped(rad: &[f64], dt: f64, t0: f64, edges: Option<u32>) -> Shaft {
    let n = rad.len();
    if n < 2 || !(dt > 0.0) {
        return Shaft::default();
    }
    let mut sum = 0.0;
    let mut cnt = 0usize;
    for k in 0..n {
        let l = k.saturating_sub(1);
        let r = (k + 1).min(n - 1);
        if r == l || !rad[l].is_finite() || !rad[r].is_finite() {
            continue;
        }
        if !(l..=r).all(|i| rad[i].is_finite()) {
            continue;
        }
        sum += (rad[r] - rad[l]) / ((r - l) as f64 * dt);
        cnt += 1;
    }
    let rpm = if cnt == 0 { None } else { Some(sum / cnt as f64 * 60.0 / (2.0 * PI)) };
    let k_trig = ((-t0 / dt).round()).clamp(0.0, (n - 1) as f64) as usize;
    let angle_deg = nearest_finite(rad, k_trig).map(|x| x.to_degrees().rem_euclid(360.0));
    let travel_deg = travel(rad).map(|r| r.to_degrees());
    let trend = revolution_trend(rad, dt);
    let accel = if edges.is_some_and(|e| e < 2) || trend.len() < 2 { None } else { slope_of(&trend) };
    Shaft {
        rpm,
        accel,
        direction: direction_of(rpm.unwrap_or(0.0)),
        angle_deg,
        travel_deg,
        trend,
        ..Shaft::default()
    }
}

fn nearest_finite(x: &[f64], k: usize) -> Option<f64> {
    if x.get(k).is_some_and(|v| v.is_finite()) {
        return Some(x[k]);
    }
    for d in 1..x.len() {
        if k >= d && x[k - d].is_finite() {
            return Some(x[k - d]);
        }
        if k + d < x.len() && x[k + d].is_finite() {
            return Some(x[k + d]);
        }
    }
    None
}

fn travel(rad: &[f64]) -> Option<f64> {
    let mut total = 0.0;
    let mut start: Option<f64> = None;
    let mut prev: Option<f64> = None;
    let mut any = false;
    for &x in rad {
        if !x.is_finite() {
            if let (Some(s), Some(p)) = (start, prev) {
                total += p - s;
                any = true;
            }
            start = None;
            prev = None;
        } else {
            if start.is_none() {
                start = Some(x);
            }
            prev = Some(x);
        }
    }
    if let (Some(s), Some(p)) = (start, prev) {
        total += p - s;
        any = true;
    }
    any.then_some(total)
}

/// One rpm each time the unwrapped angle advances a revolution.
fn revolution_trend(rad: &[f64], dt: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut run: Option<(f64, f64)> = None;
    let mut sign = 0.0;
    for (k, &x) in rad.iter().enumerate() {
        if !x.is_finite() {
            run = None;
            sign = 0.0;
            continue;
        }
        let t = k as f64 * dt;
        match run {
            None => run = Some((t, x)),
            Some((t0, x0)) => {
                if sign == 0.0 {
                    let d = x - x0;
                    if d.abs() > 1e-6 {
                        sign = d.signum();
                    }
                }
                if sign != 0.0 {
                    let mut guard = 0;
                    let mut base_t = t0;
                    let mut base_x = x0;
                    while sign * (x - base_x) >= 2.0 * PI && guard < 8 {
                        let need = base_x + sign * 2.0 * PI;
                        let frac = ((need - base_x) / (x - base_x)).clamp(0.0, 1.0);
                        let tr = base_t + (t - base_t) * frac;
                        let dt_rev = (tr - base_t).max(dt);
                        out.push((tr, sign * 60.0 / dt_rev));
                        base_t = tr;
                        base_x = need;
                        guard += 1;
                    }
                    run = Some((base_t, base_x));
                }
            }
        }
    }
    out
}

fn slope_of(trend: &[(f64, f64)]) -> Option<f64> {
    if trend.len() < 2 {
        return None;
    }
    let n = trend.len() as f64;
    let (mt, my) = (trend.iter().map(|p| p.0).sum::<f64>() / n, trend.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut var, mut cov) = (0.0, 0.0);
    for &(t, y) in trend {
        let d = t - mt;
        var += d * d;
        cov += d * (y - my);
    }
    (var > 1e-18).then_some(cov / var)
}

fn quad_step(a0: bool, b0: bool, a1: bool, b1: bool, swap: bool) -> i32 {
    let (a0, b0, a1, b1) = if swap { (b0, a0, b1, a1) } else { (a0, b0, a1, b1) };
    match (a0, b0, a1, b1) {
        (false, false, true, false) | (true, false, true, true) | (true, true, false, true) | (false, true, false, false) => 1,
        (false, false, false, true) | (false, true, true, true) | (true, true, true, false) | (true, false, false, false) => -1,
        _ => 0,
    }
}

const HALL_FWD: [u8; 6] = [0b100, 0b110, 0b010, 0b011, 0b001, 0b101];

fn hall_deg(code: u8) -> Option<f64> {
    HALL_FWD.iter().position(|&c| c == code).map(|i| i as f64 * 60.0)
}

fn hall_step(prev: u8, cur: u8) -> Option<i32> {
    if prev == cur {
        return Some(0);
    }
    let i = HALL_FWD.iter().position(|&c| c == prev)?;
    let j = HALL_FWD.iter().position(|&c| c == cur)?;
    let d = (j as i32 - i as i32).rem_euclid(6);
    if d == 1 {
        Some(1)
    } else if d == 5 {
        Some(-1)
    } else {
        Some(0)
    }
}

fn mean_finite(x: &[f32]) -> Option<f64> {
    let (mut s, mut n) = (0.0, 0usize);
    for &v in x {
        if v.is_finite() {
            s += v as f64;
            n += 1;
        }
    }
    (n > 0).then_some(s / n as f64)
}

fn mean_product(v: &[f32], i: &[f32]) -> Option<f64> {
    let n = v.len().min(i.len());
    let (mut s, mut c) = (0.0, 0usize);
    for k in 0..n {
        if v[k].is_finite() && i[k].is_finite() {
            s += v[k] as f64 * i[k] as f64;
            c += 1;
        }
    }
    (c > 0).then_some(s / c as f64)
}
