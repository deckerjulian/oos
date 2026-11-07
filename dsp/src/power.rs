// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Power analysis: power quality from a voltage and a current channel, current harmonics against IEC 61000-3-2
//! class A, switching loss, ripple, inrush, efficiency, safe operating area and the PWM duty trend.

use crate::edges::edges;
use crate::measure::analyze;
use std::f64::consts::PI;

/// Power quality over whole line cycles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quality {
    pub vrms: f64,
    pub irms: f64,
    /// mean of v·i, W
    pub p_real: f64,
    /// vrms · irms, VA
    pub s_apparent: f64,
    /// sqrt(S² - P²) with the sign of the phase (positive = inductive), var
    pub q_reactive: f64,
    /// P / S
    pub pf: f64,
    /// phase of the current fundamental behind the voltage fundamental, degrees (positive = current lags)
    pub phase_deg: f64,
    /// line frequency, Hz
    pub freq: f64,
    pub crest_v: f64,
    pub crest_i: f64,
    /// whole cycles evaluated
    pub cycles: usize,
}

/// Frequency of a periodic signal from its rising crossings of the mean (hysteresis 10 % of peak-peak).
fn freq_of(x: &[f32], dt: f64) -> Option<f64> {
    let f: Vec<f32> = x.iter().copied().filter(|v| v.is_finite()).collect();
    if f.len() < 4 {
        return None;
    }
    let mean = f.iter().map(|&v| v as f64).sum::<f64>() / f.len() as f64;
    let (lo, hi) = f.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    if !(hi > lo) {
        return None;
    }
    let r: Vec<f64> = edges(x, mean as f32, 0.1 * (hi - lo)).iter().filter(|e| e.rising).map(|e| e.t).collect();
    if r.len() < 2 {
        return None;
    }
    Some((r.len() - 1) as f64 / ((r[r.len() - 1] - r[0]) * dt))
}

/// Sample range [a, b) covering the largest whole number of cycles of `f` (at least the whole record when shorter
/// than one cycle) and the number of cycles.
fn whole_cycles(n: usize, dt: f64, f: f64) -> (usize, usize, usize) {
    let k = (n as f64 * dt * f).floor();
    if !(k >= 1.0) {
        return (0, n, 0);
    }
    let b = ((k / f / dt).round() as usize).clamp(1, n);
    (0, b, k as usize)
}

/// Complex DFT of x[a..b] at frequency f (Hz): (re, im) scaled to the peak amplitude, NaN samples skipped.
fn dft(x: &[f32], dt: f64, a: usize, b: usize, f: f64) -> (f64, f64) {
    let w = 2.0 * PI * f * dt;
    let (mut re, mut im, mut n) = (0.0, 0.0, 0usize);
    // rotating phasor (recurrence) with a periodic exact reset against drift
    let (cw, sw) = (w.cos(), w.sin());
    let (mut c, mut s) = (1.0f64, 0.0f64);
    for (k, &v) in x[a..b].iter().enumerate() {
        if k % 1024 == 0 {
            let p = w * k as f64;
            (c, s) = (p.cos(), p.sin());
        }
        if v.is_finite() {
            re += v as f64 * c;
            im -= v as f64 * s;
            n += 1;
        }
        (c, s) = (c * cw - s * sw, s * cw + c * sw);
    }
    if n == 0 {
        return (0.0, 0.0);
    }
    (2.0 * re / n as f64, 2.0 * im / n as f64)
}

/// Power quality of voltage `v` and current `i` (same sampling). The line frequency is `f_line` or measured on `v`;
/// all values are taken over the largest whole number of cycles from the record start.
pub fn quality(v: &[f32], i: &[f32], dt: f64, f_line: Option<f64>) -> Quality {
    let n = v.len().min(i.len());
    let mut q = Quality::default();
    if n < 4 || !(dt > 0.0) {
        return q;
    }
    let Some(f) = f_line.filter(|f| *f > 0.0).or_else(|| freq_of(&v[..n], dt)) else { return q };
    let (a, b, k) = whole_cycles(n, dt, f);
    let (mut sv, mut si, mut sp, mut cnt, mut pv, mut pi) = (0.0, 0.0, 0.0, 0usize, 0.0f64, 0.0f64);
    for j in a..b {
        let (x, y) = (v[j] as f64, i[j] as f64);
        if x.is_finite() && y.is_finite() {
            sv += x * x;
            si += y * y;
            sp += x * y;
            cnt += 1;
            pv = pv.max(x.abs());
            pi = pi.max(y.abs());
        }
    }
    if cnt == 0 {
        return q;
    }
    q.freq = f;
    q.cycles = k;
    q.vrms = (sv / cnt as f64).sqrt();
    q.irms = (si / cnt as f64).sqrt();
    q.p_real = sp / cnt as f64;
    q.s_apparent = q.vrms * q.irms;
    q.pf = if q.s_apparent > 0.0 { q.p_real / q.s_apparent } else { f64::NAN };
    let (vr, vi) = dft(v, dt, a, b, f);
    let (ir, ii) = dft(i, dt, a, b, f);
    let ph = (vi.atan2(vr) - ii.atan2(ir)).to_degrees();
    q.phase_deg = (ph + 180.0).rem_euclid(360.0) - 180.0;
    q.q_reactive = (q.s_apparent * q.s_apparent - q.p_real * q.p_real).max(0.0).sqrt() * if q.phase_deg < 0.0 { -1.0 } else { 1.0 };
    q.crest_v = if q.vrms > 0.0 { pv / q.vrms } else { f64::NAN };
    q.crest_i = if q.irms > 0.0 { pi / q.irms } else { f64::NAN };
    q
}

/// IEC 61000-3-2 class A limit (A rms) for harmonic `order`; infinity where there is none (fundamental, > 40).
pub fn class_a_limit(order: u32) -> f64 {
    match order {
        2 => 1.08,
        3 => 2.30,
        4 => 0.43,
        5 => 1.14,
        6 => 0.30,
        7 => 0.77,
        9 => 0.40,
        11 => 0.33,
        13 => 0.21,
        8..=40 if order % 2 == 0 => 0.23 * 8.0 / order as f64,
        15..=39 => 0.15 * 15.0 / order as f64,
        _ => f64::INFINITY,
    }
}

/// Current harmonics 1..=n: (order, A rms, class A limit A rms, pass), DFT at the exact harmonic frequencies over whole
/// cycles of `f_line` (measured on `i` when `None`). Empty when no frequency can be found.
pub fn harmonics(i: &[f32], dt: f64, f_line: Option<f64>, n: u32) -> Vec<(u32, f64, f64, bool)> {
    if i.len() < 4 || !(dt > 0.0) {
        return Vec::new();
    }
    let Some(f) = f_line.filter(|f| *f > 0.0).or_else(|| freq_of(i, dt)) else { return Vec::new() };
    let (a, b, _) = whole_cycles(i.len(), dt, f);
    (1..=n)
        .filter(|&h| h as f64 * f < 0.5 / dt)
        .map(|h| {
            let (re, im) = dft(i, dt, a, b, h as f64 * f);
            let amps = (re * re + im * im).sqrt() / std::f64::consts::SQRT_2;
            let lim = class_a_limit(h);
            (h, amps, lim, amps <= lim)
        })
        .collect()
}

/// One switching transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    /// true = turn-on
    pub on: bool,
    /// window, s from the record start
    pub t0: f64,
    pub t1: f64,
    /// ∫ v·i over the window, J
    pub energy: f64,
}

/// Switching loss summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Switching {
    /// mean turn-on / turn-off energy per transition, J
    pub e_on: f64,
    pub e_off: f64,
    /// mean conduction energy per on interval (end of turn-on .. start of turn-off), J
    pub e_cond: f64,
    /// mean of v·i over the record, W
    pub p_avg: f64,
    pub transitions: Vec<Transition>,
}

/// First crossing of `level` in direction `up` within samples [a, b) (fractional index).
fn crossing(x: &[f32], a: usize, b: usize, level: f64, up: bool) -> Option<f64> {
    (a.max(1)..b.min(x.len())).find_map(|k| {
        let (p, q) = (x[k - 1] as f64, x[k] as f64);
        let hit = if up { p < level && q >= level } else { p > level && q <= level };
        (hit && p.is_finite() && q.is_finite()).then(|| (k - 1) as f64 + (level - p) / (q - p))
    })
}

/// ∫ p dt between fractional sample indices a and b (trapezoid, linear interpolation at the ends).
fn integrate(p: &[f64], a: f64, b: f64, dt: f64) -> f64 {
    if !(b > a) || p.len() < 2 {
        return 0.0;
    }
    let at = |t: f64| {
        let k = (t.floor() as usize).min(p.len() - 2);
        p[k] + (p[k + 1] - p[k]) * (t - k as f64)
    };
    let (ka, kb) = (a.ceil() as usize, (b.floor() as usize).min(p.len() - 1));
    if ka > kb {
        return 0.5 * (at(a) + at(b)) * (b - a) * dt;
    }
    let mut s = 0.5 * (at(a) + p[ka]) * (ka as f64 - a) + 0.5 * (p[kb] + at(b)) * (b - kb as f64);
    for k in ka..kb {
        s += 0.5 * (p[k] + p[k + 1]);
    }
    s * dt
}

/// Switching loss from drain-source voltage and drain current without a gate signal. The off level of `vds` and the
/// on level of `id` are the histogram top values (`measure::analyze`), the other levels the bases. Each `vds` edge
/// through 50 % is a turn-on (falling) or turn-off (rising); its window runs from the first of (I through `thr_pct`,
/// V through 100 - `thr_pct` %) to the last of (I through 100 - `thr_pct`, V through `thr_pct` %) — e.g. turn-on
/// from 10 % Id to 10 % Vds — searched between the midpoints to the neighbouring edges.
pub fn switching(vds: &[f32], id: &[f32], dt: f64, thr_pct: f64) -> Switching {
    let n = vds.len().min(id.len());
    let mut out = Switching::default();
    if n < 4 || !(dt > 0.0) {
        return out;
    }
    let (vds, id) = (&vds[..n], &id[..n]);
    let p: Vec<f64> = vds.iter().zip(id).map(|(&v, &i)| if v.is_finite() && i.is_finite() { v as f64 * i as f64 } else { 0.0 }).collect();
    out.p_avg = p.iter().sum::<f64>() / n as f64;
    let (av, ai) = (analyze(vds, dt), analyze(id, dt));
    if !av.valid || !ai.valid {
        return out;
    }
    let thr = (thr_pct / 100.0).clamp(0.001, 0.49);
    let (vb, va) = (av.base, av.top - av.base);
    let (ib, ia) = (ai.base, ai.top - ai.base);
    if !(va > 0.0 && ia > 0.0) {
        return out;
    }
    let ev = edges(vds, (vb + 0.5 * va) as f32, (0.1 * va) as f32);
    let mut on_end: Option<f64> = None;
    let (mut e_on, mut e_off, mut e_cond) = (Vec::new(), Vec::new(), Vec::new());
    for (k, e) in ev.iter().enumerate() {
        let ra = if k == 0 { 0 } else { ((ev[k - 1].t + e.t) / 2.0) as usize };
        let rb = if k + 1 < ev.len() { ((e.t + ev[k + 1].t) / 2.0) as usize + 1 } else { n };
        let turn_on = !e.rising;
        let (i_first, i_last) = if turn_on { (thr, 1.0 - thr) } else { (1.0 - thr, thr) };
        let (v_first, v_last) = if turn_on { (1.0 - thr, thr) } else { (thr, 1.0 - thr) };
        let ti0 = crossing(id, ra, rb, ib + i_first * ia, turn_on);
        let tv0 = crossing(vds, ra, rb, vb + v_first * va, !turn_on);
        let ti1 = crossing(id, ra, rb, ib + i_last * ia, turn_on);
        let tv1 = crossing(vds, ra, rb, vb + v_last * va, !turn_on);
        let start = [ti0, tv0].into_iter().flatten().fold(f64::INFINITY, f64::min);
        let end = [ti1, tv1].into_iter().flatten().fold(f64::NEG_INFINITY, f64::max);
        if !(start.is_finite() && end.is_finite() && end > start) {
            continue;
        }
        let en = integrate(&p, start, end, dt);
        out.transitions.push(Transition { on: turn_on, t0: start * dt, t1: end * dt, energy: en });
        if turn_on {
            e_on.push(en);
            on_end = Some(end);
        } else {
            e_off.push(en);
            if let Some(s) = on_end.take() {
                e_cond.push(integrate(&p, s, start, dt));
            }
        }
    }
    let mean = |x: &[f64]| if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 };
    out.e_on = mean(&e_on);
    out.e_off = mean(&e_off);
    out.e_cond = mean(&e_cond);
    out
}

/// Ripple of a DC rail: (peak-peak, AC RMS = standard deviation), V. NaN samples are skipped.
pub fn ripple(v: &[f32]) -> (f64, f64) {
    let f: Vec<f64> = v.iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
    if f.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let (lo, hi) = f.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    let m = f.iter().sum::<f64>() / f.len() as f64;
    (hi - lo, (f.iter().map(|x| (x - m).powi(2)).sum::<f64>() / f.len() as f64).sqrt())
}

/// Inrush current: (peak |i| A, its time s, I²t A²s over the record).
pub fn inrush(i: &[f32], dt: f64) -> (f64, f64, f64) {
    let (mut peak, mut tp, mut i2t) = (f64::NAN, f64::NAN, 0.0);
    for (k, &x) in i.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        let a = (x as f64).abs();
        if !(a <= peak) {
            peak = a;
            tp = k as f64 * dt;
        }
        i2t += a * a * dt;
    }
    (peak, tp, i2t)
}

/// Input/output power and efficiency.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Efficiency {
    /// mean input / output power, W
    pub p_in: f64,
    pub p_out: f64,
    /// p_out / p_in
    pub eff: f64,
    /// energies over the record, J
    pub e_in: f64,
    pub e_out: f64,
}

/// Efficiency from input and output voltage/current (mean of v·i over the common finite samples; use whole cycles
/// of any line or switching frequency for an exact result).
pub fn efficiency(vin: &[f32], iin: &[f32], vout: &[f32], iout: &[f32], dt: f64) -> Efficiency {
    let n = vin.len().min(iin.len()).min(vout.len()).min(iout.len());
    let (mut pi, mut po, mut c) = (0.0, 0.0, 0usize);
    for k in 0..n {
        let x = [vin[k], iin[k], vout[k], iout[k]];
        if x.iter().all(|v| v.is_finite()) {
            pi += x[0] as f64 * x[1] as f64;
            po += x[2] as f64 * x[3] as f64;
            c += 1;
        }
    }
    if c == 0 {
        return Efficiency { eff: f64::NAN, ..Default::default() };
    }
    let (p_in, p_out) = (pi / c as f64, po / c as f64);
    Efficiency { p_in, p_out, eff: if p_in != 0.0 { p_out / p_in } else { f64::NAN }, e_in: pi * dt, e_out: po * dt }
}

/// Maximum current of the SOA boundary at voltage `v`: `limits` = (V, I max) points sorted by V, interpolated
/// log-log (straight lines on the usual SOA chart; linear when a value is not positive); below the first point its
/// current applies, above the last point nothing is allowed.
pub fn soa_limit(limits: &[(f64, f64)], v: f64) -> f64 {
    let Some(&(v0, i0)) = limits.first() else { return f64::INFINITY };
    let v = v.abs();
    if v <= v0 {
        return i0;
    }
    for w in limits.windows(2) {
        let ((va, ia), (vb, ib)) = (w[0], w[1]);
        if v <= vb {
            if va > 0.0 && ia > 0.0 && ib > 0.0 && vb > va {
                let a = (v / va).ln() / (vb / va).ln();
                return (ia.ln() + a * (ib.ln() - ia.ln())).exp();
            }
            return if vb > va { ia + (ib - ia) * (v - va) / (vb - va) } else { ib };
        }
    }
    if v <= limits[limits.len() - 1].0 {
        limits[limits.len() - 1].1
    } else {
        0.0
    }
}

/// Safe operating area check: indices of the samples where |id| exceeds the boundary at |vds| (`soa_limit`).
pub fn soa(vds: &[f32], id: &[f32], limits: &[(f64, f64)]) -> Vec<usize> {
    vds.iter()
        .zip(id)
        .enumerate()
        .filter(|(_, (&v, &i))| v.is_finite() && i.is_finite() && (i as f64).abs() > soa_limit(limits, v as f64))
        .map(|(k, _)| k)
        .collect()
}

/// PWM duty per period: (start time of the period s, high time / period 0..1) for every complete period (rising edge
/// to rising edge at the 50 % level of the histogram top/base, hysteresis 10 %).
pub fn duty_trend(v: &[f32], dt: f64) -> Vec<(f64, f64)> {
    let a = analyze(v, dt);
    if !a.valid || !(a.top > a.base) {
        return Vec::new();
    }
    let amp = a.top - a.base;
    let e = edges(v, (a.base + 0.5 * amp) as f32, (0.1 * amp) as f32);
    let mut out = Vec::new();
    for (k, x) in e.iter().enumerate() {
        if !x.rising {
            continue;
        }
        let Some(f) = e[k + 1..].iter().position(|y| !y.rising).map(|p| e[k + 1 + p].t) else { break };
        let Some(r) = e[k + 1..].iter().find(|y| y.rising).map(|y| y.t) else { break };
        if f < r {
            out.push((x.t * dt, (f - x.t) / (r - x.t)));
        }
    }
    out
}
