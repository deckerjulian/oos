// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Wide-bandgap double pulse. One turn-off (end of the first gate pulse) and one turn-on (the second rise).
//! Levels come from the pulse itself: the gate swing, Vbus and Von, Ioff and the load plateau. E on and E off
//! stay in `power::switching`; this module does not compute them again.

use crate::edges::edges;
use crate::measure::analyze;
use crate::power::{crossing, integrate};

/// The optional fourth probe.
pub enum Fourth<'a> {
    Off,
    /// the other gate, for dead time
    Gate(&'a [f32]),
    /// diode voltage, for Err
    DiodeV(&'a [f32]),
    /// diode current, positive when the diode has taken the load
    DiodeI(&'a [f32]),
}

/// Cursor times, seconds from the start of the record.
#[derive(Clone, Copy, Debug, Default)]
pub struct Marks {
    pub gate10: Option<f64>,
    pub i10: Option<f64>,
    pub i90: Option<f64>,
    pub gate90_off: Option<f64>,
    pub i90_off: Option<f64>,
    pub i10_off: Option<f64>,
    pub vpeak: Option<f64>,
    pub ipeak: Option<f64>,
    pub trr_end: Option<f64>,
}

/// One double-pulse record. Times are seconds. Empty crossings stay `None`.
#[derive(Clone, Debug)]
pub struct Pulse {
    pub vpeak: Option<f64>,
    pub vbus: Option<f64>,
    /// Vpeak − Vbus
    pub overshoot: Option<f64>,
    pub ipeak: Option<f64>,
    pub il: Option<f64>,
    /// Ipeak − I_L
    pub i_over: Option<f64>,
    pub td_on: Option<f64>,
    pub tr: Option<f64>,
    pub ton: Option<f64>,
    pub td_off: Option<f64>,
    pub tf: Option<f64>,
    pub toff: Option<f64>,
    /// turn-off voltage rise, V/s
    pub dvdt: Option<f64>,
    /// turn-on current rise, A/s
    pub didt: Option<f64>,
    /// a 10–90 interval was only one sample wide
    pub sample_limit: bool,
    /// the smaller of the two dead times
    pub tdt: Option<f64>,
    /// after the low side falls, and after the high side falls
    pub tdt_a: Option<f64>,
    pub tdt_b: Option<f64>,
    pub trr: Option<f64>,
    pub qrr: Option<f64>,
    pub err: Option<f64>,
    pub irrm: Option<f64>,
    /// Irrm / (t_peak − t0), A/s
    pub diode_didt: Option<f64>,
    pub qoss: Option<f64>,
    /// Id at the end of the voltage rise was still far from I_L
    pub qoss_approx: bool,
    pub inverted: bool,
    /// on intervals found on the gate
    pub pulses: usize,
    pub no_second: bool,
    /// recovery current was I_L − Id rather than a diode-current probe
    pub diode_from_id: bool,
    /// Err used max(Vbus − Vds, 0) rather than a diode-voltage probe
    pub err_from_vds: bool,
    pub note: &'static str,
    pub marks: Marks,
    /// zoom windows (both pulses, turn-on, turn-off), seconds. (0, 0) means the whole record.
    pub zoom_both: (f64, f64),
    pub zoom_on: (f64, f64),
    pub zoom_off: (f64, f64),
}

impl Default for Pulse {
    fn default() -> Self {
        Pulse {
            vpeak: None,
            vbus: None,
            overshoot: None,
            ipeak: None,
            il: None,
            i_over: None,
            td_on: None,
            tr: None,
            ton: None,
            td_off: None,
            tf: None,
            toff: None,
            dvdt: None,
            didt: None,
            sample_limit: false,
            tdt: None,
            tdt_a: None,
            tdt_b: None,
            trr: None,
            qrr: None,
            err: None,
            irrm: None,
            diode_didt: None,
            qoss: None,
            qoss_approx: false,
            inverted: false,
            pulses: 0,
            no_second: false,
            diode_from_id: true,
            err_from_vds: true,
            note: "",
            marks: Marks::default(),
            zoom_both: (0.0, 0.0),
            zoom_on: (0.0, 0.0),
            zoom_off: (0.0, 0.0),
        }
    }
}

/// Double pulse of `vds`, `id` and an optional gate. `gate_pct` is the gate threshold in percent (clamped to 1..49).
pub fn measure(vds: &[f32], id: &[f32], vgs: Option<&[f32]>, fourth: Fourth<'_>, dt: f64, gate_pct: f64) -> Pulse {
    let mut n = vds.len().min(id.len());
    if let Some(g) = vgs {
        n = n.min(g.len());
    }
    n = match fourth {
        Fourth::Gate(x) | Fourth::DiodeV(x) | Fourth::DiodeI(x) => n.min(x.len()),
        Fourth::Off => n,
    };
    let mut out = Pulse::default();
    if n < 4 || !(dt > 0.0) {
        out.note = "record too short";
        return out;
    }
    let vds = &vds[..n];
    let id_raw = &id[..n];
    let pct = (gate_pct / 100.0).clamp(0.01, 0.49);
    let Some(g) = vgs.map(|g| &g[..n]).filter(|g| swing(g).is_some()) else {
        out.note = "no gate";
        ungated(&mut out, vds, id_raw, dt);
        return out;
    };
    let (gmin, gmax) = swing(g).unwrap();
    let ev = edges(g, ((gmin + gmax) / 2.0) as f32, (0.1 * (gmax - gmin)) as f32);
    let rises: Vec<f64> = ev.iter().filter(|e| e.rising).map(|e| e.t).collect();
    let falls: Vec<f64> = ev.iter().filter(|e| !e.rising).map(|e| e.t).collect();
    let pairs = on_pairs(&rises, &falls);
    out.pulses = pairs.len();
    if pairs.is_empty() {
        out.note = "no gate";
        ungated(&mut out, vds, id_raw, dt);
        return out;
    }
    let (tr0, tf0) = pairs[0];
    out.no_second = rises.len() < 2;
    if out.pulses > 2 {
        out.note = "further pulses ignored";
    }
    let (mut ioff, mut il) = (median_before(id_raw, tr0), median_span(id_raw, tr0, tf0));
    let mut id = id_raw.to_vec();
    if il.is_finite() && ioff.is_finite() && il < ioff {
        for x in &mut id {
            if x.is_finite() {
                *x = -*x;
            }
        }
        out.inverted = true;
        ioff = median_before(&id, tr0);
        il = median_span(&id, tr0, tf0);
    }
    let von = median_span(vds, tr0, tf0);
    let vbus = median_before(vds, tr0);
    out.il = finite(il);
    out.vbus = finite(vbus);
    if !(il.is_finite() && ioff.is_finite() && (il - ioff).abs() > 0.0) {
        out.note = if out.note.is_empty() { "no current step" } else { out.note };
    }
    let i10 = ioff + pct_of(0.1, ioff, il);
    let i90 = ioff + pct_of(0.9, ioff, il);
    let g10 = gmin + pct * (gmax - gmin);
    let g90 = gmin + (1.0 - pct) * (gmax - gmin);
    let end = n as f64;
    // turn-off: the falling edge of the first pulse, searched until the next rise
    let off_b = rises.get(1).copied().unwrap_or(end);
    let (off_a, off_b) = (bound(tr0), bound(off_b) + 1);
    if let Some(t) = crossing(g, off_a, off_b, g90, false) {
        out.marks.gate90_off = Some(t * dt);
        if let Some(t90) = crossing(&id, off_a, off_b, i90, false) {
            out.td_off = Some((t90 - t) * dt);
            out.marks.i90_off = Some(t90 * dt);
            if let Some(t10) = crossing(&id, off_a, off_b, i10, false) {
                out.tf = Some((t10 - t90) * dt);
                out.toff = Some((t10 - t) * dt);
                out.marks.i10_off = Some(t10 * dt);
            }
        }
    }
    // turn-on: the second rising edge
    if let Some(&tr1) = rises.get(1) {
        let tf1 = falls.iter().copied().find(|&t| t > tr1).unwrap_or(end);
        let (on_a, on_b) = (bound(tf0), bound(tf1) + 1);
        if let Some(tg) = crossing(g, on_a, on_b, g10, true) {
            out.marks.gate10 = Some(tg * dt);
            if let Some(t10) = crossing(&id, on_a, on_b, i10, true) {
                out.td_on = Some((t10 - tg) * dt);
                out.marks.i10 = Some(t10 * dt);
                if let Some(t90) = crossing(&id, on_a, on_b, i90, true) {
                    out.tr = Some((t90 - t10) * dt);
                    out.ton = Some((t90 - tg) * dt);
                    out.marks.i90 = Some(t90 * dt);
                    if (t90 - t10) <= 1.0 {
                        out.sample_limit = true;
                    }
                    let (i_at_10, i_at_90) = (at(&id, t10), at(&id, t90));
                    if i_at_10.is_finite() && i_at_90.is_finite() && t90 > t10 {
                        out.didt = Some((i_at_90 - i_at_10) / ((t90 - t10) * dt));
                    }
                }
            }
        }
        ipeak_window(&mut out, &id, tr1, tf1, dt, il);
        diode(&mut out, &id, vds, fourth_i(&fourth, n), fourth_v(&fourth, n), il, vbus, tr1, tf1, dt);
    } else {
        out.no_second = true;
        if out.note.is_empty() {
            out.note = "no second pulse";
        }
    }
    let t_off = falls[0];
    let t_next = rises.get(1).copied().unwrap_or(end);
    vpeak_window(&mut out, vds, t_off, t_next, dt, vbus);
    if von.is_finite() && vbus.is_finite() && vbus > von {
        let v10 = von + 0.1 * (vbus - von);
        let v90 = von + 0.9 * (vbus - von);
        if let (Some(a), Some(b)) = (crossing(vds, off_a, off_b, v10, true), crossing(vds, off_a, off_b, v90, true)) {
            if b > a {
                out.dvdt = Some((v90 - v10) / ((b - a) * dt));
                if (b - a) <= 1.0 {
                    out.sample_limit = true;
                }
                qoss(&mut out, &id, il, a, b, dt);
            }
        }
    }
    if let Fourth::Gate(h) = fourth {
        dead_time(&mut out, g, &h[..n], pct, dt);
    }
    zoom(&mut out, &rises, &falls, n, dt);
    out
}

fn pct_of(p: f64, lo: f64, hi: f64) -> f64 {
    p * (hi - lo)
}

fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

fn bound(t: f64) -> usize {
    t.floor().max(0.0) as usize
}

fn swing(x: &[f32]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in x {
        if v.is_finite() {
            lo = lo.min(v as f64);
            hi = hi.max(v as f64);
        }
    }
    (hi > lo).then_some((lo, hi))
}

fn on_pairs(rises: &[f64], falls: &[f64]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &r in rises {
        if let Some(&f) = falls.iter().find(|&&f| f > r) {
            out.push((r, f));
        }
    }
    out
}

fn median_before(x: &[f32], t: f64) -> f64 {
    let k = bound(t).min(x.len());
    median(&x[..k])
}

fn median_span(x: &[f32], t0: f64, t1: f64) -> f64 {
    let a = t0 + 0.25 * (t1 - t0);
    let b = t0 + 0.75 * (t1 - t0);
    let i0 = bound(a).min(x.len());
    let i1 = bound(b).min(x.len());
    if i1 <= i0 {
        return f64::NAN;
    }
    median(&x[i0..i1])
}

fn median(x: &[f32]) -> f64 {
    let mut v: Vec<f64> = x.iter().copied().filter(|z| z.is_finite()).map(|z| z as f64).collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) }
}

fn at(x: &[f32], t: f64) -> f64 {
    if x.len() < 2 {
        return x.first().map(|v| *v as f64).unwrap_or(f64::NAN);
    }
    let k = (t.floor() as usize).min(x.len() - 2);
    let (p, q) = (x[k] as f64, x[k + 1] as f64);
    if p.is_finite() && q.is_finite() { p + (q - p) * (t - k as f64) } else { f64::NAN }
}

fn max_in(x: &[f32], t0: f64, t1: f64) -> Option<(f64, f64)> {
    let i0 = bound(t0).min(x.len());
    let i1 = (t1.ceil() as usize).clamp(i0, x.len());
    let mut best: Option<(usize, f64)> = None;
    for (k, &v) in x[i0..i1].iter().enumerate() {
        if v.is_finite() {
            let y = v as f64;
            if best.is_none_or(|(_, b)| y > b) {
                best = Some((i0 + k, y));
            }
        }
    }
    best.map(|(k, y)| (y, k as f64))
}

fn vpeak_window(out: &mut Pulse, vds: &[f32], t0: f64, t1: f64, dt: f64, vbus: f64) {
    if let Some((v, t)) = max_in(vds, t0, t1) {
        out.vpeak = Some(v);
        out.marks.vpeak = Some(t * dt);
        if vbus.is_finite() {
            out.overshoot = Some(v - vbus);
        }
    }
}

fn ipeak_window(out: &mut Pulse, id: &[f32], t0: f64, t1: f64, dt: f64, il: f64) {
    if let Some((v, t)) = max_in(id, t0, t1) {
        out.ipeak = Some(v);
        out.marks.ipeak = Some(t * dt);
        if il.is_finite() {
            out.i_over = Some(v - il);
        }
    }
}

fn qoss(out: &mut Pulse, id: &[f32], il: f64, t10: f64, t90: f64, dt: f64) {
    if !il.is_finite() {
        return;
    }
    let p: Vec<f64> = id.iter().map(|&i| if i.is_finite() { il - i as f64 } else { 0.0 }).collect();
    let q = integrate(&p, t10, t90, dt);
    if q > 0.0 {
        out.qoss = Some(q);
        let i_end = at(id, t90);
        if i_end.is_finite() && (i_end - il).abs() > 0.1 * il.abs() {
            out.qoss_approx = true;
        }
    }
}

fn fourth_i<'a>(f: &'a Fourth<'a>, n: usize) -> Option<&'a [f32]> {
    match f {
        Fourth::DiodeI(x) => Some(&x[..n.min(x.len())]),
        _ => None,
    }
}

fn fourth_v<'a>(f: &'a Fourth<'a>, n: usize) -> Option<&'a [f32]> {
    match f {
        Fourth::DiodeV(x) => Some(&x[..n.min(x.len())]),
        _ => None,
    }
}

/// Recovery on the turn-on edge. Without a diode current, i_rev = Id − I_L (positive above the plateau).
fn diode(out: &mut Pulse, id: &[f32], vds: &[f32], idi: Option<&[f32]>, vdi: Option<&[f32]>, il: f64, vbus: f64, t0w: f64, t1w: f64, dt: f64) {
    let (a, b) = (bound(t0w), bound(t1w) + 1);
    let from_id = idi.is_none();
    out.diode_from_id = from_id;
    // reverse current, positive in recovery, sampled on the integer grid
    let rev: Vec<f64> = if let Some(d) = idi {
        out.diode_from_id = false;
        d.iter().map(|&x| if x.is_finite() { -(x as f64) } else { f64::NAN }).collect()
    } else {
        if !il.is_finite() {
            return;
        }
        id.iter().map(|&x| if x.is_finite() { x as f64 - il } else { f64::NAN }).collect()
    };
    let t0 = if from_id {
        crossing(id, a, b, il, true)
    } else {
        // diode current crossing zero downward, into reverse: -i crosses 0 upward
        crossing_f(&rev, a, b, 0.0, true)
    };
    let Some(t0) = t0 else { return };
    let i1 = bound(t1w).min(rev.len());
    let i0 = bound(t0).min(i1);
    let mut peak = 0.0;
    let mut t_peak = t0;
    for (k, &y) in rev[i0..i1].iter().enumerate() {
        if y.is_finite() && y > peak {
            peak = y;
            t_peak = (i0 + k) as f64;
        }
    }
    if !(peak > 0.0) {
        return;
    }
    out.irrm = Some(peak);
    let tend = crossing_f(&rev, bound(t_peak), i1, 0.1 * peak, false);
    let Some(tend) = tend else { return };
    if !(tend > t0) {
        return;
    }
    out.trr = Some((tend - t0) * dt);
    out.marks.trr_end = Some(tend * dt);
    let q = integrate(&rev.iter().map(|y| if y.is_finite() { *y } else { 0.0 }).collect::<Vec<_>>(), t0, tend, dt);
    if q.is_finite() {
        out.qrr = Some(q);
    }
    if t_peak > t0 {
        out.diode_didt = Some(peak / ((t_peak - t0) * dt));
    }
    let vd: Vec<f64> = if let Some(v) = vdi {
        out.err_from_vds = false;
        v.iter().map(|&x| if x.is_finite() { x as f64 } else { 0.0 }).collect()
    } else if vbus.is_finite() {
        out.err_from_vds = true;
        vds.iter().map(|&x| if x.is_finite() { (vbus - x as f64).max(0.0) } else { 0.0 }).collect()
    } else {
        return;
    };
    let n = vd.len().min(rev.len());
    let prod: Vec<f64> = (0..n)
        .map(|k| {
            let i = rev[k];
            if i.is_finite() { vd[k] * i } else { 0.0 }
        })
        .collect();
    out.err = Some(integrate(&prod, t0, tend, dt));
}

fn crossing_f(x: &[f64], a: usize, b: usize, level: f64, up: bool) -> Option<f64> {
    let b = b.min(x.len());
    (a.max(1)..b).find_map(|k| {
        let (p, q) = (x[k - 1], x[k]);
        let hit = if up { p < level && q >= level } else { p > level && q <= level };
        (hit && p.is_finite() && q.is_finite() && q != p).then(|| (k - 1) as f64 + (level - p) / (q - p))
    })
}

fn dead_time(out: &mut Pulse, low: &[f32], high: &[f32], pct: f64, dt: f64) {
    let Some((l0, l1)) = swing(low) else { return };
    let Some((h0, h1)) = swing(high) else { return };
    let lt = l0 + pct * (l1 - l0);
    let ht = h0 + pct * (h1 - h0);
    let n = low.len().min(high.len());
    // after the low side falls, until the high side rises; then after that high side falls, until the low side rises
    if let Some(low_fall) = crossing(low, 1, n, lt, false) {
        if let Some(high_rise) = crossing(high, bound(low_fall), n, ht, true) {
            out.tdt_a = Some((high_rise - low_fall) * dt);
            if let Some(high_fall) = crossing(high, bound(high_rise), n, ht, false) {
                if let Some(low_rise) = crossing(low, bound(high_fall), n, lt, true) {
                    out.tdt_b = Some((low_rise - high_fall) * dt);
                }
            }
        }
    }
    out.tdt = match (out.tdt_a, out.tdt_b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => None,
    };
}

fn zoom(out: &mut Pulse, rises: &[f64], falls: &[f64], n: usize, dt: f64) {
    let end = (n.saturating_sub(1)) as f64 * dt;
    let first = rises.first().copied().unwrap_or(0.0) * dt;
    let last = falls.iter().chain(rises.iter()).copied().fold(0.0, f64::max) * dt;
    let pad = ((last - first) * 0.08).max(20.0 * dt);
    out.zoom_both = ((first - pad).max(0.0), (last + pad).min(end));
    if let Some(&r) = rises.get(1) {
        let f = falls.iter().copied().find(|&t| t > r).unwrap_or(n as f64);
        out.zoom_on = (((r * dt) - pad).max(0.0), (f * dt + pad).min(end));
    }
    if let Some(&f) = falls.first() {
        let r = rises.get(1).copied().unwrap_or(n as f64);
        out.zoom_off = (((f * dt) - pad).max(0.0), (r * dt + pad).min(end));
    }
}

/// No gate: Vpeak on a rising Vds and Ipeak on a falling Vds, the same edges `switching` uses. Delays stay empty.
fn ungated(out: &mut Pulse, vds: &[f32], id: &[f32], dt: f64) {
    let a = analyze(vds, dt);
    if !a.valid || !(a.top > a.base) {
        return;
    }
    let amp = a.top - a.base;
    let ev = edges(vds, (a.base + 0.5 * amp) as f32, (0.1 * amp) as f32);
    let n = vds.len() as f64;
    for (k, e) in ev.iter().enumerate() {
        let next = ev.get(k + 1).map(|x| x.t).unwrap_or(n);
        if e.rising && out.vpeak.is_none() {
            vpeak_window(out, vds, e.t, next, dt, a.top);
        }
        if !e.rising && out.ipeak.is_none() {
            ipeak_window(out, id, e.t, next, dt, f64::NAN);
        }
    }
}
