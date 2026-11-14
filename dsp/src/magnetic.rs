// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Magnetic measurements on a winding: inductance from the ψ–i slope, B / H / μr when the core geometry is set,
//! cycle loss, and the I vs ∫V loop. ψ is the flux linkage ∫v dt (V·s), restarted at each voltage cycle.

use crate::edges::edges;
use crate::power::{integrate, quality};
use std::f64::consts::{PI, SQRT_2};

/// Permeability of free space, H/m.
pub const MU0: f64 = 4.0 * PI * 1e-7;

/// Core geometry. Area or path length absent (or not positive) leaves B, H and μr empty.
#[derive(Clone, Copy, Debug)]
pub struct Geom {
    /// turns, at least 1
    pub turns: f64,
    /// core area Ae, m²
    pub area: Option<f64>,
    /// magnetic path length le, m
    pub length: Option<f64>,
    /// copper resistance, Ω. 0 leaves the copper in the loss.
    pub rcu: f64,
}

impl Default for Geom {
    fn default() -> Self {
        Geom { turns: 1.0, area: None, length: None, rcu: 0.0 }
    }
}

/// One cycle of the I vs ∫V loop, (ψ V·s, i A). The last cycle is the newest.
pub type Loop = Vec<(f64, f64)>;

/// Result of one record. Empty numbers stay `None`; `note` says why when the record cannot be measured.
#[derive(Clone, Debug)]
pub struct Magnetic {
    pub freq: f64,
    pub cycles: usize,
    /// cycles kept in the average (a cycle whose ψ does not close is dropped)
    pub kept: usize,
    pub dropped: usize,
    /// least-squares dψ/di on the rising-current branch, H
    pub l: Option<f64>,
    /// current window of that fit, A
    pub i_lo: f64,
    pub i_hi: f64,
    /// the two halves of the rising branch disagree by more than 2×
    pub saturated: bool,
    /// Vrms / (2 π f Irms), only when both crest factors are near √2. Not the inductance result.
    pub l_sine: Option<f64>,
    pub b_peak: Option<f64>,
    pub h_peak: Option<f64>,
    pub mu_r: Option<f64>,
    /// magnetic loss, W. Signed: negative means the current opposes the voltage.
    pub loss: Option<f64>,
    /// Irms² · Rcu over the same window, W
    pub copper: f64,
    /// largest |ψ| left at a cycle end, V·s
    pub psi_residual: f64,
    /// no complete cycle: `loops` is the partial integral and the numbers are empty
    pub partial: bool,
    pub loops: Vec<Loop>,
    pub note: &'static str,
}

impl Default for Magnetic {
    fn default() -> Self {
        Magnetic {
            freq: 0.0,
            cycles: 0,
            kept: 0,
            dropped: 0,
            l: None,
            i_lo: 0.0,
            i_hi: 0.0,
            saturated: false,
            l_sine: None,
            b_peak: None,
            h_peak: None,
            mu_r: None,
            loss: None,
            copper: 0.0,
            psi_residual: 0.0,
            partial: false,
            loops: Vec::new(),
            note: "",
        }
    }
}

/// Inductance, magnetic property, loss and the I vs ∫V loops of voltage `v` and current `i`.
pub fn measure(v: &[f32], i: &[f32], dt: f64, geom: Geom) -> Magnetic {
    let n = v.len().min(i.len());
    let mut out = Magnetic::default();
    if n < 4 || !(dt > 0.0) {
        out.note = "record too short";
        return out;
    }
    let (v, i) = (&v[..n], &i[..n]);
    if !v.iter().zip(i).any(|(&x, &y)| x.is_finite() && y.is_finite()) {
        out.note = "no finite samples";
        return out;
    }
    let g = clean_geom(geom);
    let Some((freq, rises)) = voltage_cycles(v, dt) else {
        out.partial = true;
        out.note = "under one cycle";
        out.loops = vec![partial_loop(v, i, dt)];
        out.loops.retain(|l| !l.is_empty());
        return out;
    };
    out.freq = freq;
    out.cycles = rises.len() - 1;
    let mut p = vec![0.0; n];
    let mut slopes = Vec::new();
    let mut lo_s = Vec::new();
    let mut hi_s = Vec::new();
    let mut energies = Vec::new();
    let (mut i2, mut i_n, mut psi_peak, mut i_peak) = (0.0f64, 0usize, 0.0f64, 0.0f64);
    for w in rises.windows(2) {
        let (t0, t1) = (w[0], w[1]);
        if !(t1 > t0) {
            continue;
        }
        let (k0, k1) = (t0.ceil() as usize, (t1.floor() as usize).min(n - 1));
        if k1 <= k0 {
            continue;
        }
        let mean = mean_finite(&v[k0..=k1]);
        if !mean.is_finite() {
            continue;
        }
        let k_lo = (t0.floor() as usize).min(n - 1);
        let k_hi = (t1.ceil() as usize).min(n - 1);
        for k in k_lo..=k_hi {
            let (x, y) = (v[k] as f64 - mean, i[k] as f64);
            p[k] = if x.is_finite() && y.is_finite() { x * y } else { 0.0 };
        }
        let (pts, psi_end, peak) = flux_loop(v, i, mean, t0, t1, dt);
        out.psi_residual = out.psi_residual.max(psi_end.abs());
        // a cycle that does not return is not periodic: keep the drawing, drop it from the average
        let closed = peak > 0.0 && psi_end.abs() <= (0.05 * peak).max(1e-9) || peak == 0.0 && psi_end.abs() <= 1e-9;
        let loop_pts = stride(&pts, 2000);
        if !closed {
            out.dropped += 1;
            if !loop_pts.is_empty() {
                out.loops.push(loop_pts);
            }
            continue;
        }
        if !loop_pts.is_empty() {
            out.loops.push(loop_pts);
        }
        out.kept += 1;
        energies.push(integrate(&p, t0, t1, dt));
        psi_peak = psi_peak.max(peak);
        for k in k0..=k1 {
            let y = i[k] as f64;
            if y.is_finite() {
                i2 += y * y;
                i_n += 1;
                i_peak = i_peak.max(y.abs());
            }
        }
        if let Some((slope, lo, hi, sat)) = branch_fit(&pts, i_peak) {
            slopes.push(slope);
            lo_s.push(lo);
            hi_s.push(hi);
            out.saturated |= sat;
        }
    }
    if out.dropped > 0 && out.note.is_empty() {
        out.note = "cycle did not close";
    }
    if slopes.len() >= 1 {
        out.l = Some(slopes.iter().sum::<f64>() / slopes.len() as f64);
        out.i_lo = lo_s.iter().sum::<f64>() / lo_s.len() as f64;
        out.i_hi = hi_s.iter().sum::<f64>() / hi_s.len() as f64;
    }
    if out.kept > 0 {
        let irms = if i_n > 0 { (i2 / i_n as f64).sqrt() } else { 0.0 };
        let p_elec = energies.iter().sum::<f64>() / energies.len() as f64 * freq;
        out.copper = if g.rcu > 0.0 { irms * irms * g.rcu } else { 0.0 };
        out.loss = Some(p_elec - out.copper);
        if out.loss.is_some_and(|p| p < 0.0) && out.note.is_empty() {
            out.note = "current reversed";
        }
        if let (Some(ae), Some(le)) = (g.area, g.length) {
            out.b_peak = Some(psi_peak / (g.turns * ae));
            out.h_peak = Some(g.turns * i_peak / le);
            // μr from a vanishing H is not a permeability
            if let Some(l) = out.l {
                if out.h_peak.is_some_and(|h| h >= 1e-3) {
                    out.mu_r = Some(l * le / (MU0 * g.turns * g.turns * ae));
                }
            }
        }
        let a = (rises[0].floor() as usize).min(n);
        let b = (rises[rises.len() - 1].ceil() as usize).clamp(a, n);
        let q = quality(&v[a..b], &i[a..b], dt, Some(freq));
        if q.freq > 0.0 && q.irms > 0.0 && (q.crest_v - SQRT_2).abs() < 0.15 && (q.crest_i - SQRT_2).abs() < 0.15 {
            out.l_sine = Some(q.vrms / (2.0 * PI * q.freq * q.irms));
        }
    } else if out.note.is_empty() {
        out.note = "no closed cycle";
    }
    out
}

fn clean_geom(g: Geom) -> Geom {
    Geom {
        turns: if g.turns >= 1.0 && g.turns.is_finite() { g.turns } else { 1.0 },
        area: g.area.filter(|a| a.is_finite() && *a > 0.0),
        length: g.length.filter(|l| l.is_finite() && *l > 0.0),
        rcu: if g.rcu.is_finite() && g.rcu > 0.0 { g.rcu } else { 0.0 },
    }
}

/// Rising crossings of the voltage mean, and the frequency, the same rule as `power::freq_of`.
fn voltage_cycles(v: &[f32], dt: f64) -> Option<(f64, Vec<f64>)> {
    let f: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if f.len() < 4 {
        return None;
    }
    let mean = f.iter().map(|&x| x as f64).sum::<f64>() / f.len() as f64;
    let (lo, hi) = f.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    if !(hi > lo) {
        return None;
    }
    let r: Vec<f64> = edges(v, mean as f32, 0.1 * (hi - lo)).into_iter().filter(|e| e.rising).map(|e| e.t).collect();
    if r.len() < 2 {
        return None;
    }
    let freq = (r.len() - 1) as f64 / ((r[r.len() - 1] - r[0]) * dt);
    (freq > 0.0).then_some((freq, r))
}

fn mean_finite(x: &[f32]) -> f64 {
    let (mut s, mut n) = (0.0, 0usize);
    for &v in x {
        if v.is_finite() {
            s += v as f64;
            n += 1;
        }
    }
    if n == 0 { f64::NAN } else { s / n as f64 }
}

fn at(x: &[f32], t: f64) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    let k = (t.floor() as usize).min(x.len() - 1);
    let v = x[k] as f64;
    if t.fract() == 0.0 || k + 1 >= x.len() {
        return v;
    }
    let w = x[k + 1] as f64;
    if v.is_finite() && w.is_finite() { v + (w - v) * (t - k as f64) } else { f64::NAN }
}

/// (ψ, i) from t0 to t1, ψ at t1, and the peak |ψ|. NaN voltage holds ψ; NaN current is left out of the trace.
fn flux_loop(v: &[f32], i: &[f32], mean: f64, t0: f64, t1: f64, dt: f64) -> (Vec<(f64, f64)>, f64, f64) {
    let n = v.len().min(i.len());
    let mut psi = 0.0f64;
    let mut peak = 0.0f64;
    let mut prev: Option<(f64, f64)> = Some((t0, at(v, t0) - mean));
    let mut pts = Vec::new();
    let k0 = (t0.ceil() as usize).min(n);
    let k1 = (t1.floor() as usize).min(n.saturating_sub(1));
    if k0 <= k1 && k1 < n {
    for k in k0..=k1 {
        let tk = k as f64;
        if tk + 1e-9 < t0 {
            continue;
        }
        let vk = v[k] as f64 - mean;
        if let Some((pt, pv)) = prev {
            if pv.is_finite() && vk.is_finite() && tk > pt {
                psi += 0.5 * (pv + vk) * (tk - pt) * dt;
            }
        }
        prev = if vk.is_finite() { Some((tk, vk)) } else { None };
        peak = peak.max(psi.abs());
        let y = i[k] as f64;
        if y.is_finite() && tk <= t1 + 1e-9 {
            pts.push((psi, y));
        }
    }
    }
    if let Some((pt, pv)) = prev {
        let vend = at(v, t1) - mean;
        if pv.is_finite() && vend.is_finite() && t1 > pt {
            psi += 0.5 * (pv + vend) * (t1 - pt) * dt;
        }
    }
    peak = peak.max(psi.abs());
    (pts, psi, peak)
}

fn partial_loop(v: &[f32], i: &[f32], dt: f64) -> Loop {
    let _ = dt;
    let mean = mean_finite(v);
    if !mean.is_finite() {
        return Vec::new();
    }
    let (pts, _, _) = flux_loop(v, i, mean, 0.0, (v.len().saturating_sub(1)) as f64, dt);
    stride(&pts, 2000)
}

/// Slope of ψ against i on the rising branch, between the 10th and 90th percentile of that branch.
/// `None` when the branch is shorter than 8 samples or the current barely moves.
fn branch_fit(pts: &[(f64, f64)], i_peak_hint: f64) -> Option<(f64, f64, f64, bool)> {
    if pts.len() < 8 {
        return None;
    }
    let (mut imin, mut imax) = (0usize, 0usize);
    for (k, &(_, y)) in pts.iter().enumerate() {
        if y < pts[imin].1 {
            imin = k;
        }
        if y > pts[imax].1 {
            imax = k;
        }
    }
    let rising: Vec<usize> = if imax >= imin { (imin..=imax).collect() } else { (imin..pts.len()).chain(0..=imax).collect() };
    if rising.len() < 8 {
        return None;
    }
    let mut cur: Vec<f64> = rising.iter().map(|&k| pts[k].1).collect();
    cur.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (lo, hi) = (percentile(&cur, 0.1), percentile(&cur, 0.9));
    let span = hi - lo;
    let peak = cur.iter().fold(0.0f64, |a, &y| a.max(y.abs())).max(i_peak_hint);
    // a flat current makes the slope blow up; the loop is still drawn
    if !(span > 0.0) || span < 1e-6 || span < 0.01 * peak {
        return None;
    }
    let win: Vec<(f64, f64)> = rising.iter().map(|&k| pts[k]).filter(|&(_, y)| y >= lo && y <= hi).collect();
    if win.len() < 8 {
        return None;
    }
    let slope = lin_slope(&win.iter().map(|p| p.1).collect::<Vec<_>>(), &win.iter().map(|p| p.0).collect::<Vec<_>>())?;
    let mid = 0.5 * (lo + hi);
    let (mut xl, mut yl, mut xu, mut yu) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for &(psi, y) in &win {
        if y <= mid {
            xl.push(y);
            yl.push(psi);
        } else {
            xu.push(y);
            yu.push(psi);
        }
    }
    let sat = match (lin_slope(&xl, &yl), lin_slope(&xu, &yu)) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 && (b > 2.0 * a || a > 2.0 * b) => true,
        _ => false,
    };
    Some((slope, lo, hi, sat))
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let x = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let i = x.floor() as usize;
    let f = x - i as f64;
    if i + 1 >= sorted.len() { sorted[i] } else { sorted[i] * (1.0 - f) + sorted[i + 1] * f }
}

/// Least-squares slope of y against x.
fn lin_slope(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len().min(y.len());
    if n < 2 {
        return None;
    }
    let (mx, my) = (x.iter().take(n).sum::<f64>() / n as f64, y.iter().take(n).sum::<f64>() / n as f64);
    let (mut var, mut cov) = (0.0, 0.0);
    for k in 0..n {
        let d = x[k] - mx;
        var += d * d;
        cov += d * (y[k] - my);
    }
    (var > 1e-30).then_some(cov / var)
}

fn stride(pts: &[(f64, f64)], max: usize) -> Vec<(f64, f64)> {
    if pts.len() <= max || max < 2 {
        return pts.to_vec();
    }
    let step = (pts.len() - 1) as f64 / (max - 1) as f64;
    (0..max).map(|k| pts[((k as f64 * step).round() as usize).min(pts.len() - 1)]).collect()
}
