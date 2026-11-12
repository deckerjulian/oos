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
pub(crate) fn freq_of(x: &[f32], dt: f64) -> Option<f64> {
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
pub(crate) fn whole_cycles(n: usize, dt: f64, f: f64) -> (usize, usize, usize) {
    let k = (n as f64 * dt * f).floor();
    if !(k >= 1.0) {
        return (0, n, 0);
    }
    let b = ((k / f / dt).round() as usize).clamp(1, n);
    (0, b, k as usize)
}

/// Complex DFT of x[a..b] at frequency f (Hz): cosine and sine peak amplitudes of `re·cos(wt) − im·sin(wt)`,
/// NaN samples skipped.
pub(crate) fn dft(x: &[f32], dt: f64, a: usize, b: usize, f: f64) -> (f64, f64) {
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
    /// least-squares slope of Vds between the 10 % and 90 % levels, rising and falling edges, V/s
    pub dvdt_rise: f64,
    pub dvdt_fall: f64,
    /// the same fit on the drain current, A/s
    pub didt_rise: f64,
    pub didt_fall: f64,
    /// mean of (mean Vds / mean Id) over conduction intervals, ohms
    pub rds_on: f64,
    pub transitions: Vec<Transition>,
}

/// First crossing of `level` in direction `up` within samples [a, b) (fractional index).
pub(crate) fn crossing(x: &[f32], a: usize, b: usize, level: f64, up: bool) -> Option<f64> {
    (a.max(1)..b.min(x.len())).find_map(|k| {
        let (p, q) = (x[k - 1] as f64, x[k] as f64);
        let hit = if up { p < level && q >= level } else { p > level && q <= level };
        (hit && p.is_finite() && q.is_finite()).then(|| (k - 1) as f64 + (level - p) / (q - p))
    })
}

/// ∫ p dt between fractional sample indices a and b (trapezoid, linear interpolation at the ends).
pub(crate) fn integrate(p: &[f64], a: f64, b: f64, dt: f64) -> f64 {
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
    let mut out = Switching {
        e_on: f64::NAN,
        e_off: f64::NAN,
        e_cond: f64::NAN,
        dvdt_rise: f64::NAN,
        dvdt_fall: f64::NAN,
        didt_rise: f64::NAN,
        didt_fall: f64::NAN,
        rds_on: f64::NAN,
        ..Switching::default()
    };
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
    let (mut e_on, mut e_off, mut e_cond, mut rds) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
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
                if let Some(r) = conduction_r(&vds[..n], &id[..n], s, start, ia) {
                    rds.push(r);
                }
            }
        }
    }
    let mean = |x: &[f64]| if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 };
    out.e_on = mean(&e_on);
    out.e_off = mean(&e_off);
    out.e_cond = mean(&e_cond);
    out.rds_on = mean(&rds);
    out.dvdt_rise = edge_slope(vds, dt, &av, true);
    out.dvdt_fall = edge_slope(vds, dt, &av, false);
    out.didt_rise = edge_slope(id, dt, &ai, true);
    out.didt_fall = edge_slope(id, dt, &ai, false);
    out
}

/// Least-squares slope of the samples between the 10 % and 90 % crossings of one edge direction.
/// The search stays between the neighbouring mid crossings.
fn edge_slope(x: &[f32], dt: f64, a: &crate::measure::Analysis, rising: bool) -> f64 {
    let amp = a.top - a.base;
    if !a.valid || !(amp > 0.0) || !(dt > 0.0) {
        return f64::NAN;
    }
    let mut all = a.rises.clone();
    all.extend_from_slice(&a.falls);
    all.sort_by(f64::total_cmp);
    let edges = if rising { &a.rises } else { &a.falls };
    let (lo, hi) = (a.base + 0.1 * amp, a.base + 0.9 * amp);
    let mut slopes = Vec::new();
    for &te in edges {
        let i = (te / dt) as usize + 1;
        let prev = all.iter().rev().find(|&&t| t < te).map(|t| (t / dt) as usize).unwrap_or(0);
        let next = all.iter().find(|&&t| t > te).map(|t| (t / dt) as usize + 1).unwrap_or(x.len());
        let (t0, t1) = if rising {
            (crossing(x, prev.max(1), i, lo, true), crossing(x, i.saturating_sub(1), next, hi, true))
        } else {
            (crossing(x, prev.max(1), i, hi, false), crossing(x, i.saturating_sub(1), next, lo, false))
        };
        let (Some(t0), Some(t1)) = (t0, t1) else { continue };
        if let Some(s) = ls_slope(x, dt, t0, t1) {
            slopes.push(s);
        }
    }
    if slopes.is_empty() { f64::NAN } else { slopes.iter().sum::<f64>() / slopes.len() as f64 }
}

/// Slope of y against time for the samples with fractional index in [t0, t1].
fn ls_slope(x: &[f32], dt: f64, t0: f64, t1: f64) -> Option<f64> {
    let (a, b) = (t0.min(t1), t0.max(t1));
    let i0 = a.ceil() as usize;
    let i1 = (b.floor() as usize).min(x.len().saturating_sub(1));
    if i1 <= i0 {
        return None;
    }
    let mut n = 0.0;
    let (mut st, mut sy) = (0.0, 0.0);
    for i in i0..=i1 {
        if x[i].is_finite() {
            st += i as f64 * dt;
            sy += x[i] as f64;
            n += 1.0;
        }
    }
    if n < 2.0 {
        return None;
    }
    let (mt, my) = (st / n, sy / n);
    let (mut num, mut den) = (0.0, 0.0);
    for i in i0..=i1 {
        if x[i].is_finite() {
            let t = i as f64 * dt - mt;
            num += t * (x[i] as f64 - my);
            den += t * t;
        }
    }
    (den > 0.0).then_some(num / den)
}

/// Mean voltage over mean current on a conduction interval. Skips a current under 10 % of the amplitude.
fn conduction_r(v: &[f32], i: &[f32], a: f64, b: f64, i_amp: f64) -> Option<f64> {
    let mean = |x: &[f32]| {
        let i0 = a.ceil() as usize;
        let i1 = (b.floor() as usize).min(x.len().saturating_sub(1));
        if i1 < i0 {
            return f64::NAN;
        }
        let s: Vec<f64> = x[i0..=i1].iter().copied().filter(|v| v.is_finite()).map(|v| v as f64).collect();
        if s.is_empty() { f64::NAN } else { s.iter().sum::<f64>() / s.len() as f64 }
    };
    let (mv, mi) = (mean(v), mean(i));
    if mi.is_finite() && mv.is_finite() && mi.abs() >= 0.1 * i_amp && mi != 0.0 {
        Some(mv / mi)
    } else {
        None
    }
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

/// Peak-to-peak and RMS of one DFT line: RMS is hypot(re, im) / √2, peak-to-peak is 2 × hypot(re, im).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub pp: f64,
    pub rms: f64,
}

/// One DFT line of `v` at `f` hertz. Empty when `f` is not a positive frequency the record can hold.
pub fn tone_ripple(v: &[f32], dt: f64, f: f64) -> Tone {
    let empty = Tone { pp: f64::NAN, rms: f64::NAN };
    if v.len() < 4 || !(dt > 0.0) || !(f > 0.0) || f >= 0.5 / dt {
        return empty;
    }
    let (re, im) = dft(v, dt, 0, v.len(), f);
    let amp = (re * re + im * im).sqrt();
    Tone { pp: 2.0 * amp, rms: amp / std::f64::consts::SQRT_2 }
}

/// Switching ripple. `f` is clamped at 1 MHz, the manual's ceiling.
pub fn switching_ripple(v: &[f32], dt: f64, f: f64) -> Tone {
    tone_ripple(v, dt, f.min(1e6))
}

/// Line frequency `quality` would measure on `v`, or 60 Hz.
pub fn line_hz(v: &[f32], dt: f64) -> f64 {
    freq_of(v, dt).unwrap_or(60.0)
}

/// Switching frequency from the period of `v`, clamped at 1 MHz. Empty when the period is not measurable.
pub fn switch_hz(v: &[f32], dt: f64) -> f64 {
    let p = analyze(v, dt).period();
    if p.is_finite() && p > 0.0 { (1.0 / p).min(1e6) } else { f64::NAN }
}

/// Time from the input crossing 50 % rising to the output first reaching its histogram top.
pub fn turn_on_time(vin: &[f32], vout: &[f32], dt: f64) -> f64 {
    rail_delay(vin, vout, dt, true)
}

/// Time from the input crossing 50 % falling to the output first reaching its histogram base.
pub fn turn_off_time(vin: &[f32], vout: &[f32], dt: f64) -> f64 {
    rail_delay(vin, vout, dt, false)
}

fn rail_delay(vin: &[f32], vout: &[f32], dt: f64, on: bool) -> f64 {
    let n = vin.len().min(vout.len());
    if n < 4 || !(dt > 0.0) {
        return f64::NAN;
    }
    let (ain, aout) = (analyze(&vin[..n], dt), analyze(&vout[..n], dt));
    let (ia, oa) = (ain.top - ain.base, aout.top - aout.base);
    if !(ain.valid && aout.valid && ia > 0.0 && oa > 0.0) {
        return f64::NAN;
    }
    let mid = ain.base + 0.5 * ia;
    let Some(t_in) = crossing(&vin[..n], 1, n, mid, on) else { return f64::NAN };
    let level = if on { aout.top } else { aout.base };
    let Some(t_out) = crossing(&vout[..n], t_in.floor() as usize, n, level, on) else { return f64::NAN };
    if t_out > t_in { (t_out - t_in) * dt } else { f64::NAN }
}

/// Input capacitance C = q / v. `q` is the trapezoid integral of the current over the whole record, `v` the
/// peak-to-peak voltage. Empty when the voltage does not move. The record is the turn-on event.
pub fn input_capacitance(v: &[f32], i: &[f32], dt: f64) -> f64 {
    let n = v.len().min(i.len());
    if n < 2 || !(dt > 0.0) {
        return f64::NAN;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &x in &v[..n] {
        if x.is_finite() {
            lo = lo.min(x as f64);
            hi = hi.max(x as f64);
        }
    }
    let pp = hi - lo;
    if !(pp > 0.0) || !pp.is_finite() {
        return f64::NAN;
    }
    let cur: Vec<f64> = i[..n].iter().map(|&x| if x.is_finite() { x as f64 } else { 0.0 }).collect();
    integrate(&cur, 0.0, (n - 1) as f64, dt) / pp
}

/// Power-integrity rail: overshoot and undershoot against a reference voltage, the DC mean, and the maximum during turn-on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rail {
    /// maximum − reference, V
    pub overshoot: f64,
    /// reference − minimum, V
    pub undershoot: f64,
    /// mean of the finite samples, or the mean of the per-cycle means when a clock is given
    pub dc: f64,
    /// maximum of the rail after the input crosses 50 % rising, or of the whole record
    pub turn_on: f64,
}

/// `reference` None uses the histogram top. `clock` splits overshoot, undershoot and the DC mean into cycles
/// (rising mid crossings) and reports the mean of those. `vin` None takes the turn-on maximum over the whole record.
pub fn rail(v: &[f32], dt: f64, reference: Option<f64>, vin: Option<&[f32]>, clock: Option<&[f32]>) -> Rail {
    let mut r = Rail { overshoot: f64::NAN, undershoot: f64::NAN, dc: f64::NAN, turn_on: f64::NAN };
    if v.len() < 2 || !(dt > 0.0) {
        return r;
    }
    let a = analyze(v, dt);
    let reference = reference.filter(|x| x.is_finite()).unwrap_or(a.top);
    let finite: Vec<(usize, f64)> = v.iter().enumerate().filter_map(|(k, &x)| x.is_finite().then_some((k, x as f64))).collect();
    if finite.is_empty() || !reference.is_finite() {
        return r;
    }
    let cycles: Vec<(usize, usize)> = match clock {
        Some(c) if c.len() >= 4 => {
            let ck = analyze(c, dt);
            if ck.rises.len() >= 2 {
                ck.rises.windows(2).filter_map(|w| {
                    let s = (w[0] / dt).ceil() as usize;
                    let e = (w[1] / dt).floor() as usize;
                    (e > s).then_some((s, e.min(v.len() - 1)))
                }).collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    if cycles.is_empty() {
        let (mx, mn) = finite.iter().fold((f64::NEG_INFINITY, f64::INFINITY), |(h, l), &(_, x)| (h.max(x), l.min(x)));
        r.overshoot = mx - reference;
        r.undershoot = reference - mn;
        r.dc = finite.iter().map(|&(_, x)| x).sum::<f64>() / finite.len() as f64;
    } else {
        let (mut os, mut us, mut dc) = (Vec::new(), Vec::new(), Vec::new());
        for (s, e) in cycles {
            let sl: Vec<f64> = v[s..=e].iter().copied().filter(|x| x.is_finite()).map(|x| x as f64).collect();
            if sl.is_empty() {
                continue;
            }
            let (mx, mn) = sl.iter().fold((f64::NEG_INFINITY, f64::INFINITY), |(h, l), &x| (h.max(x), l.min(x)));
            os.push(mx - reference);
            us.push(reference - mn);
            dc.push(sl.iter().sum::<f64>() / sl.len() as f64);
        }
        let mean = |x: &[f64]| if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 };
        (r.overshoot, r.undershoot, r.dc) = (mean(&os), mean(&us), mean(&dc));
    }
    r.turn_on = match vin {
        Some(vin) if vin.len() >= 4 => {
            let ai = analyze(vin, dt);
            let amp = ai.top - ai.base;
            if ai.valid && amp > 0.0 {
                if let Some(t) = crossing(vin, 1, vin.len().min(v.len()), ai.base + 0.5 * amp, true) {
                    let s = t.ceil() as usize;
                    v[s.min(v.len() - 1)..].iter().copied().filter(|x| x.is_finite()).map(|x| x as f64).fold(f64::NEG_INFINITY, f64::max)
                } else {
                    f64::NAN
                }
            } else {
                f64::NAN
            }
        }
        _ => finite.iter().fold(f64::NEG_INFINITY, |m, &(_, x)| m.max(x)),
    };
    if !r.turn_on.is_finite() {
        r.turn_on = f64::NAN;
    }
    r
}
