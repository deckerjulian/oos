// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Automatic measurements (conventions of IEEE 181: top and base from the histogram, 10/50/90 % reference
//! levels, edges at the 50 % level with 10 % hysteresis, time values averaged over all complete cycles in the record).

use crate::edges::edges;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Meas {
    Vmax,
    Vmin,
    Vpp,
    Vtop,
    Vbase,
    Vamp,
    Vupper,
    Vmid,
    Vlower,
    Vavg,
    Vrms,
    VavgCyc,
    VrmsCyc,
    VrmsAc,
    StdDev,
    Overshoot,
    Preshoot,
    Area,
    AreaCyc,
    Period,
    Freq,
    Rise,
    Fall,
    PWidth,
    NWidth,
    PDuty,
    NDuty,
    PPulses,
    NPulses,
    PEdges,
    NEdges,
    TVmax,
    TVmin,
    PSlew,
    NSlew,
    /// Time a cycle stays above the high reference (90 %).
    HighTime,
    /// Time a cycle stays below the low reference (10 %).
    LowTime,
    /// Mean width of bursts separated by an idle gap. The idle is a menu number; `get` uses the period.
    Burst,
    /// Mean time above the high reference, below the low reference, or either. `get` uses both.
    Outside,
    /// Mean time across N mid crossings. `get` uses N = 2 on rising edges.
    NPeriods,
    /// Mean of the per-cycle histogram amplitude.
    CycAmp,
    CycTop,
    CycBase,
    CycPp,
    CycMax,
    CycMin,
    /// GCD of mid-crossing spacings (same estimator as the eye diagram).
    EstUi,
    /// Sample median.
    Median,
    /// Histogram mode of the voltage range.
    Mode,
    /// Least-squares slope of voltage vs time.
    LinSlope,
    /// Least-squares intercept at the first sample.
    LinInt,
    Pam0,
    Pam1,
    Pam2,
    Pam3,
    /// Smallest gap between adjacent 4-level modes.
    PamSep,
    /// Mid-sliced ones in the record.
    Bits,
    /// Unit interval from a 2nd-order PLL clock.
    PllUi,
    /// Weighted 1/2/3-UI mean of mid spacings.
    UiWtd,
    /// RMS after folding onto the estimated UI.
    FoldRms,
}

pub const ALL: [Meas; 60] = [
    Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vtop, Meas::Vbase, Meas::Vamp, Meas::Vupper, Meas::Vmid, Meas::Vlower, Meas::Vavg,
    Meas::Vrms, Meas::VavgCyc, Meas::VrmsCyc, Meas::VrmsAc, Meas::StdDev, Meas::Overshoot, Meas::Preshoot, Meas::Area,
    Meas::AreaCyc, Meas::Period, Meas::Freq, Meas::Rise, Meas::Fall, Meas::PWidth, Meas::NWidth, Meas::PDuty, Meas::NDuty,
    Meas::PPulses, Meas::NPulses, Meas::PEdges, Meas::NEdges, Meas::TVmax, Meas::TVmin, Meas::PSlew, Meas::NSlew,
    Meas::HighTime, Meas::LowTime, Meas::Burst, Meas::Outside, Meas::NPeriods, Meas::CycAmp, Meas::CycTop, Meas::CycBase,
    Meas::CycPp, Meas::CycMax, Meas::CycMin, Meas::EstUi, Meas::Median, Meas::Mode, Meas::LinSlope, Meas::LinInt,
    Meas::Pam0, Meas::Pam1, Meas::Pam2, Meas::Pam3, Meas::PamSep, Meas::Bits, Meas::PllUi, Meas::UiWtd, Meas::FoldRms,
];

impl Meas {
    pub fn name(self) -> &'static str {
        match self {
            Meas::Vmax => "Vmax",
            Meas::Vmin => "Vmin",
            Meas::Vpp => "Vpp",
            Meas::Vtop => "Vtop",
            Meas::Vbase => "Vbase",
            Meas::Vamp => "Vamp",
            Meas::Vupper => "Vupper",
            Meas::Vmid => "Vmid",
            Meas::Vlower => "Vlower",
            Meas::Vavg => "Vavg",
            Meas::Vrms => "Vrms",
            Meas::VavgCyc => "Per. Vavg",
            Meas::VrmsCyc => "Per. Vrms",
            Meas::VrmsAc => "AC RMS",
            Meas::StdDev => "Std dev",
            Meas::Overshoot => "Overshoot",
            Meas::Preshoot => "Preshoot",
            Meas::Area => "Area",
            Meas::AreaCyc => "Cyc area",
            Meas::Period => "Period",
            Meas::Freq => "Freq",
            Meas::Rise => "Rise time",
            Meas::Fall => "Fall time",
            Meas::PWidth => "+Width",
            Meas::NWidth => "-Width",
            Meas::PDuty => "+Duty",
            Meas::NDuty => "-Duty",
            Meas::PPulses => "+Pulses",
            Meas::NPulses => "-Pulses",
            Meas::PEdges => "+Edges",
            Meas::NEdges => "-Edges",
            Meas::TVmax => "tVmax",
            Meas::TVmin => "tVmin",
            Meas::PSlew => "+Rate",
            Meas::NSlew => "-Rate",
            Meas::HighTime => "High time",
            Meas::LowTime => "Low time",
            Meas::Burst => "Burst",
            Meas::Outside => "Outside",
            Meas::NPeriods => "N periods",
            Meas::CycAmp => "Cyc amp",
            Meas::CycTop => "Cyc top",
            Meas::CycBase => "Cyc base",
            Meas::CycPp => "Cyc pk-pk",
            Meas::CycMax => "Cyc max",
            Meas::CycMin => "Cyc min",
            Meas::EstUi => "Est. UI",
            Meas::Median => "Median",
            Meas::Mode => "Mode",
            Meas::LinSlope => "Lin slope",
            Meas::LinInt => "Lin int.",
            Meas::Pam0 => "L0",
            Meas::Pam1 => "L1",
            Meas::Pam2 => "L2",
            Meas::Pam3 => "L3",
            Meas::PamSep => "L sep.",
            Meas::Bits => "Ones",
            Meas::PllUi => "PLL UI",
            Meas::UiWtd => "UI wtd",
            Meas::FoldRms => "Fold RMS",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            Meas::Overshoot | Meas::Preshoot | Meas::PDuty | Meas::NDuty => "%",
            Meas::Area | Meas::AreaCyc => "Vs",
            Meas::Period | Meas::Rise | Meas::Fall | Meas::PWidth | Meas::NWidth | Meas::TVmax | Meas::TVmin | Meas::HighTime
            | Meas::LowTime | Meas::Burst | Meas::Outside | Meas::NPeriods | Meas::EstUi | Meas::PllUi | Meas::UiWtd => "s",
            Meas::Freq => "Hz",
            Meas::PPulses | Meas::NPulses | Meas::PEdges | Meas::NEdges | Meas::Bits => "",
            Meas::PSlew | Meas::NSlew | Meas::LinSlope => "V/s",
            _ => "V",
        }
    }
}

/// Which side of the reference levels `Outside` counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outside {
    Above,
    Below,
    Both,
}

/// Everything measured on one waveform (times in seconds from the first sample).
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub valid: bool,
    pub vmax: f64,
    pub vmin: f64,
    pub top: f64,
    pub base: f64,
    pub avg: f64,
    pub rms: f64,
    pub std: f64,
    pub area: f64,
    pub t_vmax: f64,
    pub t_vmin: f64,
    /// 50 % crossings
    pub rises: Vec<f64>,
    pub falls: Vec<f64>,
    pub rise_time: f64,
    pub fall_time: f64,
    pub cyc_avg: f64,
    pub cyc_rms: f64,
    pub cyc_area: f64,
    /// Mean time above the high reference, and below the low reference.
    pub high_time: f64,
    pub low_time: f64,
    /// High-reference (90 %) and low-reference (10 %) crossings, seconds from the first sample.
    pub hi_rises: Vec<f64>,
    pub hi_falls: Vec<f64>,
    pub lo_rises: Vec<f64>,
    pub lo_falls: Vec<f64>,
    /// Mean, over rising-mid cycles, of that cycle's histogram top, base, amplitude, peak-to-peak, maximum, minimum.
    pub cyc_top: f64,
    pub cyc_base: f64,
    pub cyc_amp: f64,
    pub cyc_pp: f64,
    pub cyc_max: f64,
    pub cyc_min: f64,
    /// Rise/fall thresholds as fractions of amplitude (IEEE-181 default 0.1 / 0.9).
    pub lpct: f64,
    pub hpct: f64,
    pub est_ui: f64,
    pub median: f64,
    pub mode: f64,
    pub lin_slope: f64,
    pub lin_int: f64,
    pub pam: [f64; 4],
    pub bits: f64,
    pub pll_ui: f64,
    pub ui_wtd: f64,
    pub fold_rms: f64,
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn refs(lpct: f64, hpct: f64) -> (f64, f64) {
    let lo = if lpct.is_finite() && lpct > 0.0 && lpct < 0.5 { lpct } else { 0.1 };
    let hi = if hpct.is_finite() && hpct > 0.5 && hpct < 1.0 { hpct } else { 0.9 };
    if hi > lo { (lo, hi) } else { (0.1, 0.9) }
}

fn median_samples(f: &[f32]) -> f64 {
    let mut s = f.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        s[n / 2] as f64
    } else {
        (s[n / 2 - 1] as f64 + s[n / 2] as f64) / 2.0
    }
}

fn hist_mode(f: &[f32], vmin: f32, vmax: f32) -> f64 {
    let span = (vmax - vmin).max(1e-12);
    let mut hist = [0u32; 256];
    for &x in f {
        let i = (((x - vmin) / span) * 255.0) as usize;
        hist[i.min(255)] += 1;
    }
    let b = (0..256).max_by_key(|&i| hist[i]).unwrap_or(0);
    // The last bin contains vmax; its nominal centre must not exceed the
    // observed range (also keeps a constant record exactly constant).
    (vmin as f64 + span as f64 * (b as f64 + 0.5) / 255.0).clamp(vmin as f64, vmax as f64)
}

fn linreg(v: &[f32], dt: f64) -> (f64, f64) {
    let (mut n, mut st, mut sy, mut stt, mut sty) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &x) in v.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        let t = i as f64 * dt;
        let y = x as f64;
        n += 1.0;
        st += t;
        sy += y;
        stt += t * t;
        sty += t * y;
    }
    let den = n * stt - st * st;
    if n < 2.0 || !(den.abs() > 0.0) {
        return (f64::NAN, f64::NAN);
    }
    let slope = (n * sty - st * sy) / den;
    let intercept = (sy * stt - st * sty) / den;
    (slope, intercept)
}

/// Top/base by histogram modes of the upper/lower 40 % (fallback: max/min when there are no flat levels).
fn top_base(f: &[f32], vmin: f32, vmax: f32) -> (f64, f64) {
    let span = (vmax - vmin).max(1e-12);
    let mut hist = [0u32; 256];
    for &x in f {
        hist[(((x - vmin) / span) * 255.0) as usize] += 1;
    }
    let mode = |a: usize, b: usize| (a..b).max_by_key(|&i| hist[i]).unwrap_or(a);
    let (lo, hi) = (mode(0, 102), mode(154, 256));
    let need = (f.len() / 20) as u32;
    if hist[lo] < need || hist[hi] < need {
        return (vmax as f64, vmin as f64);
    }
    // centre of the mode bin, refined by the mean of the samples in it
    let bin_mean = |b: usize| {
        let (l, h) = (vmin + span * b as f32 / 255.0, vmin + span * (b + 1) as f32 / 255.0);
        let s: Vec<f64> = f.iter().filter(|&&x| x >= l && x < h).map(|&x| x as f64).collect();
        if s.is_empty() {
            l as f64
        } else {
            mean(&s)
        }
    };
    (bin_mean(hi), bin_mean(lo))
}

/// Crossing of `level` searched from index `from` backwards (`back`) down to `lim`, or forwards up to `lim`, in the
/// given direction.
fn cross(v: &[f32], from: usize, level: f32, up: bool, back: bool, lim: usize) -> Option<f64> {
    let hit = |i: usize| {
        let (a, b) = (v[i - 1], v[i]);
        a.is_finite() && b.is_finite() && if up { a < level && b >= level } else { a > level && b <= level }
    };
    let at = |i: usize| {
        let (a, b) = (v[i - 1], v[i]);
        (i - 1) as f64 + ((level - a) / (b - a)) as f64
    };
    if back {
        (lim.max(1)..=from.min(v.len() - 1)).rev().find(|&i| hit(i)).map(at)
    } else {
        (from.max(1)..lim.min(v.len())).find(|&i| hit(i)).map(at)
    }
}

pub fn analyze(v: &[f32], dt: f64) -> Analysis {
    analyze_min(v, dt, 0.0)
}

/// Like `analyze`, but edges and time values only when the amplitude (top - base) is at least `min_amp` volts, so
/// noise on a flat trace does not produce frequencies (the scope passes a fraction of a division).
pub fn analyze_min(v: &[f32], dt: f64, min_amp: f64) -> Analysis {
    analyze_refs(v, dt, min_amp, 0.1, 0.9)
}

/// IEEE-181 uses 10/90. The menu also offers 20/80 (`lpct` / `hpct` as fractions of amplitude).
pub fn analyze_refs(v: &[f32], dt: f64, min_amp: f64, lpct: f64, hpct: f64) -> Analysis {
    let f: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if f.len() < 4 {
        return Analysis::default();
    }
    let (mut imax, mut imin) = (0, 0);
    for (i, &x) in v.iter().enumerate() {
        if x.is_finite() {
            if !v[imax].is_finite() || x > v[imax] {
                imax = i;
            }
            if !v[imin].is_finite() || x < v[imin] {
                imin = i;
            }
        }
    }
    let (vmax, vmin) = (v[imax], v[imin]);
    let n = f.len() as f64;
    let avg = f.iter().map(|&x| x as f64).sum::<f64>() / n;
    let rms = (f.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / n).sqrt();
    let std = (f.iter().map(|&x| (x as f64 - avg).powi(2)).sum::<f64>() / n).sqrt();
    let (top, base) = top_base(&f, vmin, vmax);
    let (lpct, hpct) = refs(lpct, hpct);
    let (lin_slope, lin_int) = linreg(v, dt);
    let mut a = Analysis {
        valid: true,
        vmax: vmax as f64,
        vmin: vmin as f64,
        top,
        base,
        avg,
        rms,
        std,
        area: avg * n * dt,
        t_vmax: imax as f64 * dt,
        t_vmin: imin as f64 * dt,
        rise_time: f64::NAN,
        fall_time: f64::NAN,
        cyc_avg: f64::NAN,
        cyc_rms: f64::NAN,
        cyc_area: f64::NAN,
        high_time: f64::NAN,
        low_time: f64::NAN,
        cyc_top: f64::NAN,
        cyc_base: f64::NAN,
        cyc_amp: f64::NAN,
        cyc_pp: f64::NAN,
        cyc_max: f64::NAN,
        cyc_min: f64::NAN,
        lpct,
        hpct,
        est_ui: f64::NAN,
        median: median_samples(&f),
        mode: hist_mode(&f, vmin, vmax),
        lin_slope,
        lin_int,
        pam: crate::linecode::pam4_levels(&f, vmin, vmax),
        bits: f.iter().filter(|&&x| x >= 0.5 * (vmin + vmax)).count() as f64,
        pll_ui: f64::NAN,
        ui_wtd: f64::NAN,
        fold_rms: f64::NAN,
        ..Default::default()
    };
    let amp = top - base;
    if amp <= 0.0 || amp < min_amp {
        return a;
    }
    let mid = (base + amp * 0.5) as f32;
    let e = edges(v, mid, (amp * 0.1) as f32);
    a.rises = e.iter().filter(|x| x.rising).map(|x| x.t * dt).collect();
    a.falls = e.iter().filter(|x| !x.rising).map(|x| x.t * dt).collect();
    let spacings: Vec<f64> = {
        let t = mids(&a);
        t.windows(2).map(|w| w[1] - w[0]).collect()
    };
    a.est_ui = crate::eye::unit_interval(&spacings).unwrap_or(f64::NAN);
    a.ui_wtd = crate::linecode::unit_interval_runs(&spacings).unwrap_or(f64::NAN);
    if a.rises.len() + a.falls.len() <= 8000 {
        let mid_v = (base + amp * 0.5) as f32;
        if let Some(c) = crate::eye::recover_clock_pll(v, dt, mid_v, (amp * 0.1) as f32, None, 0.0, true) {
            a.pll_ui = c.period;
        }
        if a.est_ui.is_finite() {
            a.fold_rms = crate::linecode::fold_rms(v, dt, a.est_ui);
        }
    }
    let lo = (base + lpct * amp) as f32;
    let hi_l = (base + hpct * amp) as f32;
    let mut rt = Vec::new();
    let mut ft = Vec::new();
    // an edge's levels lie between its neighbours. Searching further costs a pass over the record per edge: a million
    // noisy samples that seldom reach the 10 % and 90 % levels held the main thread for half a minute
    for (k, x) in e.iter().enumerate() {
        let i = x.t as usize + 1;
        let (before, after) = (if k > 0 { e[k - 1].t as usize } else { 0 }, e.get(k + 1).map_or(v.len(), |n| n.t as usize + 1));
        if x.rising {
            if let (Some(p), Some(q)) = (cross(v, i, lo, true, true, before), cross(v, i, hi_l, true, false, after)) {
                rt.push((q - p) * dt);
            }
        } else if let (Some(p), Some(q)) = (cross(v, i, hi_l, false, true, before), cross(v, i, lo, false, false, after)) {
            ft.push((q - p) * dt);
        }
    }
    a.rise_time = mean(&rt);
    a.fall_time = mean(&ft);
    // keep the high and low reference crossings: high time, low time and time outside are measured there
    let (hr, hf) = level_times(v, hi_l, dt);
    let (lr, lf) = level_times(v, lo, dt);
    let hi = pair_widths(&hr, &hf);
    let low = pair_widths(&lf, &lr);
    a.high_time = mean(&hi);
    a.low_time = mean(&low);
    (a.hi_rises, a.hi_falls, a.lo_rises, a.lo_falls) = (hr, hf, lr, lf);
    if a.rises.len() >= 2 {
        let (top, base, amp, pp, mx, mn) = cycle_levels(v, dt, &a.rises);
        (a.cyc_top, a.cyc_base, a.cyc_amp, a.cyc_pp, a.cyc_max, a.cyc_min) = (top, base, amp, pp, mx, mn);
    }
    // cycle values between the first and last rising edge
    if a.rises.len() >= 2 {
        let (s, t) = ((a.rises[0] / dt).ceil() as usize, (a.rises[a.rises.len() - 1] / dt).floor() as usize);
        let c: Vec<f64> = v[s..=t.min(v.len() - 1)].iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
        if !c.is_empty() {
            a.cyc_avg = mean(&c);
            a.cyc_rms = (c.iter().map(|x| x * x).sum::<f64>() / c.len() as f64).sqrt();
            a.cyc_area = a.cyc_avg * (a.rises[a.rises.len() - 1] - a.rises[0]) / (a.rises.len() - 1) as f64;
        }
    }
    a
}

impl Analysis {
    pub fn period(&self) -> f64 {
        let p = |e: &[f64]| if e.len() >= 2 { (e[e.len() - 1] - e[0]) / (e.len() - 1) as f64 } else { f64::NAN };
        let r = p(&self.rises);
        if r.is_finite() {
            r
        } else {
            p(&self.falls)
        }
    }
    /// Mean width of complete positive (rise -> fall) or negative (fall -> rise) pulses. One pass over the sorted edges:
    /// a deep record has a hundred thousand of them, and this runs for every frame.
    fn width(&self, pos: bool) -> f64 {
        let (a, b) = if pos { (&self.rises, &self.falls) } else { (&self.falls, &self.rises) };
        let (mut w, mut j) = (Vec::new(), 0);
        for (i, &s) in a.iter().enumerate() {
            while j < b.len() && b[j] <= s {
                j += 1;
            }
            let Some(&e) = b.get(j) else { break };
            // a pulse ends at the next edge of the other kind, but must not span another opening edge
            if !a[i + 1..].iter().find(|&&x| x > s).is_some_and(|&x| x < e) {
                w.push(e - s);
            }
        }
        mean(&w)
    }
    fn pulses(&self, pos: bool) -> f64 {
        let (a, b) = if pos { (&self.rises, &self.falls) } else { (&self.falls, &self.rises) };
        let Some(&last) = b.last() else { return 0.0 };
        a.partition_point(|&s| s < last) as f64
    }

    pub fn get(&self, m: Meas) -> f64 {
        if !self.valid {
            return f64::NAN;
        }
        let amp = self.top - self.base;
        match m {
            Meas::Vmax => self.vmax,
            Meas::Vmin => self.vmin,
            Meas::Vpp => self.vmax - self.vmin,
            Meas::Vtop => self.top,
            Meas::Vbase => self.base,
            Meas::Vamp => amp,
            Meas::Vupper => self.base + self.hpct_or_default() * amp,
            Meas::Vmid => self.base + 0.5 * amp,
            Meas::Vlower => self.base + self.lpct_or_default() * amp,
            Meas::Vavg => self.avg,
            Meas::Vrms => self.rms,
            Meas::VavgCyc => self.cyc_avg,
            Meas::VrmsCyc => self.cyc_rms,
            Meas::VrmsAc | Meas::StdDev => self.std,
            Meas::Overshoot => {
                if amp > 0.0 {
                    (self.vmax - self.top) / amp * 100.0
                } else {
                    f64::NAN
                }
            }
            Meas::Preshoot => {
                if amp > 0.0 {
                    (self.base - self.vmin) / amp * 100.0
                } else {
                    f64::NAN
                }
            }
            Meas::Area => self.area,
            Meas::AreaCyc => self.cyc_area,
            Meas::Period => self.period(),
            Meas::Freq => 1.0 / self.period(),
            Meas::Rise => self.rise_time,
            Meas::Fall => self.fall_time,
            Meas::PWidth => self.width(true),
            Meas::NWidth => self.width(false),
            Meas::PDuty => self.width(true) / self.period() * 100.0,
            Meas::NDuty => self.width(false) / self.period() * 100.0,
            Meas::PPulses => self.pulses(true),
            Meas::NPulses => self.pulses(false),
            Meas::PEdges => self.rises.len() as f64,
            Meas::NEdges => self.falls.len() as f64,
            Meas::TVmax => self.t_vmax,
            Meas::TVmin => self.t_vmin,
            Meas::PSlew => (self.hpct_or_default() - self.lpct_or_default()) * amp / self.rise_time,
            Meas::NSlew => -(self.hpct_or_default() - self.lpct_or_default()) * amp / self.fall_time,
            Meas::HighTime => self.high_time,
            Meas::LowTime => self.low_time,
            Meas::Burst => burst_width(self, 0.0),
            Meas::Outside => time_outside(self, Outside::Both),
            Meas::NPeriods => duration_n(self, 2, false),
            Meas::CycAmp => self.cyc_amp,
            Meas::CycTop => self.cyc_top,
            Meas::CycBase => self.cyc_base,
            Meas::CycPp => self.cyc_pp,
            Meas::CycMax => self.cyc_max,
            Meas::CycMin => self.cyc_min,
            Meas::EstUi => self.est_ui,
            Meas::Median => self.median,
            Meas::Mode => self.mode,
            Meas::LinSlope => self.lin_slope,
            Meas::LinInt => self.lin_int,
            Meas::Pam0 => self.pam[0],
            Meas::Pam1 => self.pam[1],
            Meas::Pam2 => self.pam[2],
            Meas::Pam3 => self.pam[3],
            Meas::PamSep => {
                if self.pam.iter().any(|level| !level.is_finite()) { return f64::NAN; }
                let d = [self.pam[1] - self.pam[0], self.pam[2] - self.pam[1], self.pam[3] - self.pam[2]];
                d.into_iter().filter(|x| x.is_finite() && *x > 0.0).fold(f64::NAN, |a, x| if a.is_nan() { x } else { a.min(x) })
            }
            Meas::Bits => self.bits,
            Meas::PllUi => self.pll_ui,
            Meas::UiWtd => self.ui_wtd,
            Meas::FoldRms => self.fold_rms,
        }
    }

    fn lpct_or_default(&self) -> f64 {
        if self.lpct > 0.0 { self.lpct } else { 0.1 }
    }
    fn hpct_or_default(&self) -> f64 {
        if self.hpct > 0.0 { self.hpct } else { 0.9 }
    }
}

/// Rising and falling crossings of `level`, seconds from the first sample.
fn level_times(v: &[f32], level: f32, dt: f64) -> (Vec<f64>, Vec<f64>) {
    let (mut rises, mut falls) = (Vec::new(), Vec::new());
    for i in 1..v.len() {
        let (a, b) = (v[i - 1], v[i]);
        if !(a.is_finite() && b.is_finite()) || a == b {
            continue;
        }
        let hit_up = a < level && b >= level;
        let hit_dn = a > level && b <= level;
        if hit_up || hit_dn {
            let t = ((i - 1) as f64 + ((level - a) / (b - a)) as f64) * dt;
            if hit_up {
                rises.push(t);
            } else {
                falls.push(t);
            }
        }
    }
    (rises, falls)
}

/// Durations of complete pulses, with ascending edge times. Each opening goes to
/// the next closing edge only when no strictly later opening lies between them.
fn pair_widths(open: &[f64], close: &[f64]) -> Vec<f64> {
    let (mut w, mut j, mut next) = (Vec::new(), 0, 0);
    for (i, &s) in open.iter().enumerate() {
        while j < close.len() && close[j] <= s {
            j += 1;
        }
        let Some(&e) = close.get(j) else { break };
        next = next.max(i + 1);
        while next < open.len() && (open[next].is_nan() || open[next] <= s) { next += 1; }
        if s.is_nan() || !open.get(next).is_some_and(|&x| x < e) {
            w.push(e - s);
        }
    }
    w
}

/// Mean of the per-cycle histogram top and base, their difference, and the sample max, min and peak-to-peak.
/// Cycles run from one rising mid crossing to the next. A record with fewer than two rising edges has no result.
fn cycle_levels(v: &[f32], dt: f64, rises: &[f64]) -> (f64, f64, f64, f64, f64, f64) {
    let (mut top, mut base, mut amp, mut pp, mut mx, mut mn) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for w in rises.windows(2) {
        let s = (w[0] / dt).ceil() as usize;
        let e = ((w[1] / dt).floor() as usize).min(v.len().saturating_sub(1));
        if e <= s {
            continue;
        }
        let samp: Vec<f32> = v[s..=e].iter().copied().filter(|x| x.is_finite()).collect();
        if samp.len() < 2 {
            continue;
        }
        let (hi, lo) = samp.iter().fold((f32::NEG_INFINITY, f32::INFINITY), |(h, l), &x| (h.max(x), l.min(x)));
        if !(hi > lo) {
            continue;
        }
        let (t, b) = top_base(&samp, lo, hi);
        top.push(t);
        base.push(b);
        amp.push(t - b);
        pp.push(hi as f64 - lo as f64);
        mx.push(hi as f64);
        mn.push(lo as f64);
    }
    (mean(&top), mean(&base), mean(&amp), mean(&pp), mean(&mx), mean(&mn))
}

/// Mid crossings, either direction, in time order.
fn mids(a: &Analysis) -> Vec<f64> {
    let mut t = a.rises.clone();
    t.extend_from_slice(&a.falls);
    t.sort_by(f64::total_cmp);
    t
}

/// Mean burst width. A burst is a run of adjacent mid crossings whose gaps are all at most `idle` seconds.
/// `idle` <= 0 uses the period of `a`. One continuous clock is one burst.
pub fn burst_width(a: &Analysis, idle: f64) -> f64 {
    let idle = if idle > 0.0 { idle } else { a.period() };
    if !(idle > 0.0) || !idle.is_finite() {
        return f64::NAN;
    }
    let t = mids(a);
    if t.len() < 2 {
        return f64::NAN;
    }
    let (mut widths, mut start) = (Vec::new(), 0usize);
    for i in 1..t.len() {
        if t[i] - t[i - 1] > idle {
            if i - start >= 2 {
                widths.push(t[i - 1] - t[start]);
            }
            start = i;
        }
    }
    if t.len() - start >= 2 {
        widths.push(t[t.len() - 1] - t[start]);
    }
    mean(&widths)
}

/// Mean duration of the intervals above the high reference, below the low reference, or either.
pub fn time_outside(a: &Analysis, mode: Outside) -> f64 {
    let hi = pair_widths(&a.hi_rises, &a.hi_falls);
    let lo = pair_widths(&a.lo_falls, &a.lo_rises);
    match mode {
        Outside::Above => mean(&hi),
        Outside::Below => mean(&lo),
        Outside::Both => {
            let mut both = hi;
            both.extend(lo);
            mean(&both)
        }
    }
}

/// Mean time from a mid crossing to the one N crossings later. `data` uses either direction; a clock uses rising edges.
/// N is a count of edge intervals.
pub fn duration_n(a: &Analysis, n: usize, data: bool) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    let edges = if data { mids(a) } else { a.rises.clone() };
    if edges.len() <= n {
        return f64::NAN;
    }
    mean(&edges.windows(n + 1).map(|w| w[n] - w[0]).collect::<Vec<_>>())
}

/// Edge pair for delay / phase: (edge of A, edge of B), true = rising.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair(pub bool, pub bool);

/// Delay from the first selected edge of A to the nearest selected edge of B (may be negative), seconds.
pub fn delay(a: &Analysis, b: &Analysis, p: Pair) -> f64 {
    let ea = if p.0 { &a.rises } else { &a.falls };
    let eb = if p.1 { &b.rises } else { &b.falls };
    let Some(&ta) = ea.first() else { return f64::NAN };
    eb.iter().map(|&tb| tb - ta).min_by(|x, y| x.abs().partial_cmp(&y.abs()).unwrap()).unwrap_or(f64::NAN)
}

/// Phase of B relative to A in degrees (delay / period of A x 360).
pub fn phase(a: &Analysis, b: &Analysis, p: Pair) -> f64 {
    delay(a, b, p) / a.period() * 360.0
}

fn edge_list<'a>(a: &'a Analysis, rising: bool) -> &'a [f64] {
    if rising { &a.rises } else { &a.falls }
}

/// Mean, over every selected edge of A, of the closest selected edge of B minus that edge. May be negative.
pub fn skew(a: &Analysis, b: &Analysis, p: Pair) -> f64 {
    let (ea, eb) = (edge_list(a, p.0), edge_list(b, p.1));
    if ea.is_empty() || eb.is_empty() {
        return f64::NAN;
    }
    let mut j = 0usize;
    let mut d = Vec::with_capacity(ea.len());
    for &ta in ea {
        while j + 1 < eb.len() && (eb[j + 1] - ta).abs() <= (eb[j] - ta).abs() {
            j += 1;
        }
        d.push(eb[j] - ta);
    }
    mean(&d)
}

/// Setup: clock edge minus the closest earlier data mid. Positive when the data edge leads the clock.
/// `rise` selects rising or falling clock edges; data uses either-direction mids.
pub fn setup_time(clock: &Analysis, data: &Analysis) -> f64 {
    setup_time_at(clock, data, true)
}

pub fn setup_time_at(clock: &Analysis, data: &Analysis, rise: bool) -> f64 {
    mean(&setup_times(clock, data, rise))
}

/// Hold: the next data mid minus the clock edge.
pub fn hold_time(clock: &Analysis, data: &Analysis) -> f64 {
    hold_time_at(clock, data, true)
}

pub fn hold_time_at(clock: &Analysis, data: &Analysis, rise: bool) -> f64 {
    mean(&hold_times(clock, data, rise))
}

fn setup_times(clock: &Analysis, data: &Analysis, rise: bool) -> Vec<f64> {
    let (c, d) = (if rise { &clock.rises } else { &clock.falls }, mids(data));
    let mut j = 0usize;
    let mut s = Vec::new();
    for &tc in c {
        while j < d.len() && d[j] < tc {
            j += 1;
        }
        if j > 0 {
            s.push(tc - d[j - 1]);
        }
    }
    s
}

fn hold_times(clock: &Analysis, data: &Analysis, rise: bool) -> Vec<f64> {
    let (c, d) = (if rise { &clock.rises } else { &clock.falls }, mids(data));
    let mut j = 0usize;
    let mut s = Vec::new();
    for &tc in c {
        while j < d.len() && d[j] <= tc {
            j += 1;
        }
        if j < d.len() {
            s.push(d[j] - tc);
        }
    }
    s
}

/// Clock edges whose setup is below `tsetup` or hold below `thold`.
pub fn setup_hold_violations(clock: &Analysis, data: &Analysis, tsetup: f64, thold: f64) -> f64 {
    setup_hold_violations_at(clock, data, tsetup, thold, true)
}

pub fn setup_hold_violations_at(clock: &Analysis, data: &Analysis, tsetup: f64, thold: f64, rise: bool) -> f64 {
    let (su, ho) = (setup_times(clock, data, rise), hold_times(clock, data, rise));
    if su.len() != ho.len() {
        let n = su.len().min(ho.len());
        return su[..n].iter().zip(&ho[..n]).filter(|(&s, &h)| s < tsetup || h < thold).count() as f64;
    }
    su.iter().zip(&ho).filter(|(&s, &h)| s < tsetup || h < thold).count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_pulse_join_matches_suffix_reference_with_duplicate_and_nan_edges() {
        fn old(open:&[f64],close:&[f64])->Vec<f64>{
            let (mut widths,mut j)=(Vec::new(),0);
            for (i,&s) in open.iter().enumerate(){
                while j<close.len()&&close[j]<=s{j+=1;}
                let Some(&e)=close.get(j) else{break};
                if !open[i+1..].iter().any(|&x|x>s&&x<e){widths.push(e-s);}
            }
            widths
        }
        let cases:Vec<(Vec<f64>,Vec<f64>)>=vec![
            (vec![],vec![]),(vec![1.0],vec![]),(vec![],vec![1.0]),
            (vec![1.0,1.0,2.0,3.0],vec![1.0,1.5,2.0,2.5,3.5]),
            (vec![3.0,4.0],vec![0.0,1.0]),(vec![0.0,1.0,2.0],vec![3.0]),
            (vec![f64::NAN;3],vec![f64::NAN;3]),
        ];
        for (open,close) in cases {
            let (a,b)=(pair_widths(&open,&close),old(&open,&close));
            assert_eq!(a.len(),b.len());assert!(a.iter().zip(b).all(|(a,b)|*a==b||(a.is_nan()&&b.is_nan())));
        }
        let mut state=7u32;
        for count in [2,17,100,1_000] {
            let mut values=||{state=state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);(state%100) as f64};
            let mut open:Vec<_>=(0..count).map(|_|values()).collect();let mut close:Vec<_>=(0..count+3).map(|_|values()).collect();
            open.sort_by(f64::total_cmp);close.sort_by(f64::total_cmp);
            assert_eq!(pair_widths(&open,&close),old(&open,&close));
        }
    }

    #[test]
    fn complete_pulse_join_is_linear_on_a_large_sorted_record() {
        let open:Vec<f64>=(0..400_000).map(|i|i as f64).collect();let close:Vec<f64>=open.iter().map(|t|t+0.5).collect();
        let start=std::time::Instant::now();let widths=pair_widths(&open,&close);let elapsed=start.elapsed();
        assert_eq!(widths.len(),400_000);assert!(widths.iter().all(|&width|width==0.5));
        assert!(elapsed<std::time::Duration::from_millis(500),"sorted pulse join took {elapsed:?}");
        eprintln!("sorted complete-pulse join400k: {elapsed:?}");
    }

    #[test]
    fn histogram_mode_representative_stays_inside_sample_extrema() {
        for samples in [vec![-1.0, 1.0, 1.0, 1.0], vec![-0.02, 0.404, 0.404, 0.404]] {
            let lo = samples.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mode = hist_mode(&samples, lo, hi);
            assert!((lo as f64..=hi as f64).contains(&mode));
            assert_eq!(mode, hi as f64, "the occupied last bin represents the observed maximum");
        }
    }

    #[test]
    fn histogram_mode_constant_signal_is_exact() {
        for value in [0.0, 1.0, -0.25, 1e-9] {
            assert_eq!(hist_mode(&[value; 16], value, value), value as f64);
        }
    }

    #[test]
    fn pam_separation_requires_all_four_occupied_bands() {
        for samples in [vec![-1.0, 1.0, -1.0, 1.0], vec![0.25; 16]] {
            let analysis = analyze(&samples, 1e-9);
            assert!(analysis.get(Meas::PamSep).is_nan());
        }
        let analysis = analyze(&[-1.0, -0.33, 0.33, 1.0], 1e-9);
        assert!(analysis.get(Meas::PamSep).is_finite());
    }

    #[test]
    fn width_and_pulses_match_the_definition() {
        // what they mean, edge by edge
        fn width(a: &[f64], b: &[f64]) -> f64 {
            let w: Vec<f64> = a.iter().filter_map(|&s| b.iter().find(|&&e| e > s).map(|&e| e - s)).collect();
            mean(&a.iter().zip(w.iter()).filter(|(&s, &wd)| !a.iter().any(|&x| x > s && x < s + wd)).map(|(_, &wd)| wd).collect::<Vec<f64>>())
        }
        let mut x = 7u32;
        let mut r = || {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 16) as f64
        };
        for n in [0usize, 1, 2, 3, 10, 200] {
            let (mut rises, mut falls): (Vec<f64>, Vec<f64>) = ((0..n).map(|_| r()).collect(), (0..n + 1).map(|_| r()).collect());
            rises.sort_by(f64::total_cmp);
            falls.sort_by(f64::total_cmp);
            let an = Analysis { rises: rises.clone(), falls: falls.clone(), ..Default::default() };
            for (pos, a, b) in [(true, &rises, &falls), (false, &falls, &rises)] {
                let (got, want) = (an.width(pos), width(a, b));
                assert!(got.is_nan() && want.is_nan() || (got - want).abs() < 1e-9, "n {} {} {}", n, got, want);
                assert_eq!(an.pulses(pos), a.iter().filter(|&&s| b.iter().any(|&e| e > s)).count() as f64);
            }
        }
    }

    /// Seen on the unit: a deep record with many edges that seldom reach the 10 % and 90 % levels.
    #[test]
    fn edges_that_miss_the_levels_are_quick() {
        let n = 400_000;
        let mut v = vec![0.0f32; n / 10];
        v.extend(vec![1.0f32; n / 10]);
        v.extend((0..n * 8 / 10).map(|i| if i % 2 == 0 { 0.4f32 } else { 0.6 }));
        let t = std::time::Instant::now();
        let a = analyze_min(&v, 1e-9, 0.1);
        assert!(a.rises.len() > 1000, "{}", a.rises.len());
        assert!(t.elapsed().as_secs() < 2, "{:?}", t.elapsed());
    }

    #[test]
    fn width_of_a_deep_record_is_quick() {
        let rises: Vec<f64> = (0..400_000).map(|i| i as f64).collect();
        let falls: Vec<f64> = rises.iter().map(|t| t + 0.5).collect();
        let an = Analysis { rises, falls, ..Default::default() };
        let t = std::time::Instant::now();
        assert!((an.width(true) - 0.5).abs() < 1e-9 && an.pulses(true) == 400_000.0);
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    }

    #[test]
    fn twenty_eighty_rise_is_shorter_than_ten_ninety() {
        let mut v = vec![0.0f32; 20];
        v.extend((0..100).map(|i| i as f32 / 99.0));
        v.extend(vec![1.0; 80]);
        v.extend((0..100).map(|i| 1.0 - i as f32 / 99.0));
        v.extend(vec![0.0; 20]);
        let a10 = analyze_refs(&v, 1e-9, 0.0, 0.1, 0.9);
        let a20 = analyze_refs(&v, 1e-9, 0.0, 0.2, 0.8);
        assert!(close_abs(a10.rise_time, 80e-9, 5e-9), "{}", a10.rise_time);
        assert!(close_abs(a20.rise_time, 60e-9, 5e-9), "{}", a20.rise_time);
        assert!(a20.rise_time < a10.rise_time);
    }

    fn close_abs(x: f64, want: f64, abs: f64) -> bool {
        (x - want).abs() <= abs
    }

    #[test]
    fn setup_hold_counts_a_late_data_edge() {
        let clk = Analysis { valid: true, rises: vec![10e-9, 20e-9], falls: vec![15e-9, 25e-9], ..Analysis::default() };
        let data = Analysis { valid: true, rises: vec![8e-9, 19e-9], falls: vec![12e-9, 22e-9], ..Analysis::default() };
        // clocks at 10 ns (data at 8 ns, next 12 ns) and 20 ns (data at 19 ns, next 22 ns)
        assert!((setup_time(&clk, &data) - 1.5e-9).abs() < 1e-12);
        assert_eq!(setup_hold_violations(&clk, &data, 1.2e-9, 1.2e-9), 1.0);
        assert_eq!(setup_hold_violations(&clk, &data, 2e-9, 3e-9), 2.0);
    }
}
