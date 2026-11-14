// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Automatic measurements (conventions of IEEE 181: top and base from the histogram, 10/50/90 % reference
//! levels, edges at the 50 % level with 10 % hysteresis, time values averaged over all complete cycles in the record).

use crate::edges::level_edges;

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
    /// Unit interval estimated from the mid edges (`estimated_ui`).
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
    /// Complete reference-level crossing pairs used in the rise/fall means.
    pub rise_count: usize,
    pub fall_count: usize,
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
    /// Mean of the per-edge slew rates between the low and high reference levels, V/s (falling is negative).
    pub rise_slew: f64,
    pub fall_slew: f64,
    /// Time of the last sample, seconds from the first.
    pub t_end: f64,
    /// Time outside level, one value per high span (rising mid edge to the next falling one): first to last time at
    /// or above the upper reference level, 0 when it never gets there.
    pub out_hi: Vec<f64>,
    /// As `out_hi` for low spans, at or below the lower reference level.
    pub out_lo: Vec<f64>,
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

/// Top/base, automatic method: histogram modes, falling back to max/min.
///
/// Histogram over [min, max] with 1000 bins (fewer samples: half the sample count, at least 64). From the middle
/// bin, the lower half gives base and the upper half gives top: the fullest bin, ties to the outer bin. Max/min
/// is used instead when the data is flat, a half is empty, a peak sits in the outermost bin (clipping), or a peak
/// holds fewer samples than `peak_threshold` (a poor peak). A bin's value is its grid point.
fn top_base(f: &[f32], vmin: f32, vmax: f32) -> (f64, f64) {
    let minmax = (vmax as f64, vmin as f64);
    let n = f.len();
    if !(vmax > vmin) || n == 0 {
        return minmax;
    }
    let nbins = if n > 1000 { 1000 } else { (n / 2).max(64) };
    // bins are centred on a grid from min to max (first and last bin centred on min and max)
    let (lo_v, width) = (vmin as f64, (vmax as f64 - vmin as f64) / (nbins - 1) as f64);
    let mut hist = vec![0u64; nbins];
    for &x in f {
        let b = ((x as f64 - lo_v) / width).round().max(0.0) as usize;
        hist[b.min(nbins - 1)] += 1;
    }
    let thr = peak_threshold(&hist, n);
    let mid = (0.5 * nbins as f64).round() as usize;
    // fullest bin scanning from the middle outward; `>=` makes ties go to the outer bin
    let pick = |bins: &mut dyn Iterator<Item = usize>| {
        let mut best = None::<usize>;
        for i in bins {
            if best.map_or(true, |b| hist[i] >= hist[b]) {
                best = Some(i);
            }
        }
        best
    };
    let lo = pick(&mut (0..=mid.min(nbins - 1)).rev());
    let hi = pick(&mut (mid.min(nbins - 1)..nbins));
    let (Some(lo), Some(hi)) = (lo, hi) else { return minmax };
    let bad = |i: usize, outer: usize| hist[i] == 0 || i == outer || (hist[i] as f64) < thr;
    if bad(lo, 0) || bad(hi, nbins - 1) {
        return minmax;
    }
    (lo_v + hi as f64 * width, lo_v + lo as f64 * width)
}

/// Smallest count a histogram peak needs to be a real level: about twice the mean count per bin. `stride` is the
/// spacing of populated bins (above 1 when the samples are quantised more coarsely than the bins).
fn peak_threshold(hist: &[u64], n: usize) -> f64 {
    let k = hist.len() as f64;
    let m = k + 3.0;
    let or = hist.iter().enumerate().filter(|(_, &c)| c > 0).fold(0usize, |a, (i, _)| a | i);
    let stride = if or == 0 { 1.0 } else { (1usize << or.trailing_zeros()) as f64 };
    let n = n as f64;
    let t1 = if k < 2.0 { 2.0 * n } else { 2.0 * (stride * n / k).floor() };
    let t2 = 2.0 * n / (m / (4.0 * stride) + k) + 8.0;
    t1.max(t2)
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
    // the finite samples: the record itself when it has no gap (a copy of 100 kpts per channel otherwise)
    let own: Vec<f32>;
    let f: &[f32] = if v.iter().all(|x| x.is_finite()) {
        v
    } else {
        own = v.iter().copied().filter(|x| x.is_finite()).collect();
        &own
    };
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
    let sum = f.iter().map(|&x| x as f64).sum::<f64>();
    let smean = sum / n;
    let rms = (f.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / n).sqrt();
    let std = (f.iter().map(|&x| (x as f64 - smean).powi(2)).sum::<f64>() / n).sqrt();
    // trapezoid rule: the area over the record, and the mean is that area over the record's span
    let trap = sum - (f[0] as f64 + f[f.len() - 1] as f64) / 2.0;
    let avg = trap / (n - 1.0);
    let (top, base) = top_base(f, vmin, vmax);
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
        area: trap * dt,
        rise_slew: f64::NAN,
        fall_slew: f64::NAN,
        t_end: (v.len() - 1) as f64 * dt,
        out_hi: Vec::new(),
        out_lo: Vec::new(),
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
    let e = level_edges(v, mid, (amp * 0.05) as f32);
    a.rises = e.iter().filter(|x| x.rising).map(|x| x.t * dt).collect();
    a.falls = e.iter().filter(|x| !x.rising).map(|x| x.t * dt).collect();
    let spacings: Vec<f64> = {
        let t = mids(&a);
        t.windows(2).map(|w| w[1] - w[0]).collect()
    };
    a.est_ui = estimated_ui(&mids(&a));
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
    // the low and high reference crossings use the same edge finder as the mid edges: a band of 5 % of the amplitude
    // either side of the level, so noise or ringing at 10 % / 90 % does not add or move crossings
    let band = (amp * 0.05) as f32;
    let at = |level: f32| {
        let e = level_edges(v, level, band);
        let r: Vec<f64> = e.iter().filter(|x| x.rising).map(|x| x.t * dt).collect();
        let f: Vec<f64> = e.iter().filter(|x| !x.rising).map(|x| x.t * dt).collect();
        (r, f)
    };
    let (hr, hf) = at(hi_l);
    let (lr, lf) = at(lo);
    // one result per transition: a low crossing pairs with the next high crossing when no other low crossing is between
    let rt = pair_widths(&lr, &hr);
    let ft = pair_widths(&hf, &lf);
    let dv = (hi_l - lo) as f64;
    a.rise_time = mean(&rt);
    a.fall_time = mean(&ft);
    a.rise_slew = mean(&rt.iter().map(|w| dv / w).collect::<Vec<_>>());
    a.fall_slew = mean(&ft.iter().map(|w| -dv / w).collect::<Vec<_>>());
    a.rise_count = rt.len();
    a.fall_count = ft.len();
    // high time, low time and time outside are measured at the same reference crossings
    let hi = pair_widths(&hr, &hf);
    let low = pair_widths(&lf, &lr);
    a.high_time = mean(&hi);
    a.low_time = mean(&low);
    (a.hi_rises, a.hi_falls, a.lo_rises, a.lo_falls) = (hr, hf, lr, lf);
    (a.out_hi, a.out_lo) = outside_spans(v, dt, &a);
    if a.rises.len() >= 2 {
        let (top, base, amp, pp, mx, mn) = cycle_levels(v, dt, &a.rises);
        (a.cyc_top, a.cyc_base, a.cyc_amp, a.cyc_pp, a.cyc_max, a.cyc_min) = (top, base, amp, pp, mx, mn);
    }
    // cycle mean, RMS and area: one value per cycle, then the mean of those
    let (mut avg, mut rms, mut area) = (Vec::new(), Vec::new(), Vec::new());
    for w in a.cycle_edges().windows(2) {
        let (t0, t1) = (w[0], w[1]);
        if !(t1 > t0) {
            continue;
        }
        let ar = integral(v, dt, t0, t1);
        let (s, e) = ((t0 / dt).ceil() as usize, ((t1 / dt).floor() as usize).min(v.len() - 1));
        let sq: Vec<f64> = v[s..=e.max(s)].iter().filter(|x| x.is_finite()).map(|&x| (x as f64).powi(2)).collect();
        area.push(ar);
        avg.push(ar / (t1 - t0));
        rms.push(mean(&sq).sqrt());
    }
    (a.cyc_avg, a.cyc_rms, a.cyc_area) = (mean(&avg), mean(&rms), mean(&area));
    a
}

impl Analysis {
    /// Mean of the per-cycle periods.
    pub fn period(&self) -> f64 {
        mean(&self.periods())
    }
    /// Mean of the per-cycle frequencies (1 / each cycle's period), not 1 / mean period.
    pub fn frequency(&self) -> f64 {
        mean(&self.periods().iter().map(|p| 1.0 / p).collect::<Vec<_>>())
    }
    /// Back-to-back cycle periods between edges of the same polarity as the first mid edge.
    fn periods(&self) -> Vec<f64> {
        self.cycle_edges().windows(2).map(|w| w[1] - w[0]).collect()
    }
    /// Cycle boundaries: the mid edges of the same polarity as the first edge in the record.
    fn cycle_edges(&self) -> &[f64] {
        let first_rising = match (self.rises.first(), self.falls.first()) {
            (Some(r), Some(f)) => r <= f,
            (Some(_), None) => true,
            _ => false,
        };
        if first_rising {
            &self.rises
        } else {
            &self.falls
        }
    }
    /// Mean positive duty cycle, %. Each cycle starts on a rising edge: (next fall - rise) / (next rise - rise).
    fn duty(&self) -> f64 {
        let e = edge_seq(self);
        let mut d = Vec::new();
        let mut k = e.iter().position(|x| x.1).unwrap_or(e.len());
        while k + 2 < e.len() {
            let (t0, t1, t2) = (e[k].0, e[k + 1].0, e[k + 2].0);
            if t2 > t0 {
                d.push((t1 - t0) / (t2 - t0) * 100.0);
            }
            k += 2;
        }
        mean(&d)
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
            Meas::Freq => self.frequency(),
            Meas::Rise => self.rise_time,
            Meas::Fall => self.fall_time,
            Meas::PWidth => self.width(true),
            Meas::NWidth => self.width(false),
            Meas::PDuty => self.duty(),
            Meas::NDuty => 100.0 - self.duty(),
            Meas::PPulses => self.pulses(true),
            Meas::NPulses => self.pulses(false),
            Meas::PEdges => self.rises.len() as f64,
            Meas::NEdges => self.falls.len() as f64,
            Meas::TVmax => self.t_vmax,
            Meas::TVmin => self.t_vmin,
            Meas::PSlew => self.rise_slew,
            Meas::NSlew => self.fall_slew,
            Meas::HighTime => self.high_time,
            Meas::LowTime => self.low_time,
            Meas::Burst => burst_width(self, 0.0),
            Meas::Outside => time_outside(self, Outside::Both),
            Meas::NPeriods => duration_n(self, 6, false),
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

/// Mid crossings with their direction (true = rising), in time order.
fn edge_seq(a: &Analysis) -> Vec<(f64, bool)> {
    let mut e: Vec<(f64, bool)> = a.rises.iter().map(|&t| (t, true)).chain(a.falls.iter().map(|&t| (t, false))).collect();
    e.sort_by(|x, y| x.0.total_cmp(&y.0));
    e
}

/// Default idle time between bursts, seconds.
pub const BURST_IDLE: f64 = 50e-6;

/// Mean burst width. A burst starts at a rising mid edge that follows at least `idle` seconds without edges (counted
/// from the start of the record for the first edge) and ends at the first falling edge followed by more than `idle`
/// seconds without edges (or by the end of the record). `idle` <= 0 uses `BURST_IDLE`. A clock that never pauses for
/// `idle` has no burst.
pub fn burst_width(a: &Analysis, idle: f64) -> f64 {
    let idle = if idle > 0.0 && idle.is_finite() { idle } else { BURST_IDLE };
    let e = edge_seq(a);
    if e.len() < 2 {
        return f64::NAN;
    }
    let (mut widths, mut prev, mut i) = (Vec::new(), 0.0, 0usize);
    'bursts: while i < e.len() {
        while i < e.len() && !(e[i].1 && e[i].0 - prev >= idle) {
            prev = e[i].0;
            i += 1;
        }
        if i == e.len() {
            break;
        }
        let start = e[i].0;
        for j in i..e.len() {
            if !e[j].1 {
                let next = e.get(j + 1).map_or(a.t_end, |x| x.0);
                if next - e[j].0 > idle {
                    widths.push(e[j].0 - start);
                    i = j;
                    continue 'bursts;
                }
            }
        }
        break;
    }
    mean(&widths)
}

/// Mean duration of the intervals above the high reference, below the low reference, or either.
pub fn time_outside(a: &Analysis, mode: Outside) -> f64 {
    match mode {
        Outside::Above => mean(&a.out_hi),
        Outside::Below => mean(&a.out_lo),
        Outside::Both => {
            let mut both = a.out_hi.clone();
            both.extend_from_slice(&a.out_lo);
            mean(&both)
        }
    }
}

/// Unit interval from edge times alone. P = the shortest same-polarity period (edge to the edge two later); the mean
/// of the edge intervals shorter than 0.75 P is the first guess u0. Each interval then counts as 1, 2 or 3 bits (below
/// 1.5, 2.5, 3.5 u0; longer ones are left out), and the UI is the counted time over the counted bits.
pub fn estimated_ui(t: &[f64]) -> f64 {
    if t.len() < 3 {
        return f64::NAN;
    }
    let p = t.windows(3).step_by(2).map(|w| w[2] - w[0]).fold(f64::INFINITY, f64::min);
    let d: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    let u0 = mean(&d.iter().copied().filter(|&x| x < 0.75 * p).collect::<Vec<_>>());
    let (mut time, mut bits) = (0.0, 0.0);
    for &x in &d {
        let n = if x < 1.5 * u0 {
            1.0
        } else if x < 2.5 * u0 {
            2.0
        } else if x < 3.5 * u0 {
            3.0
        } else {
            continue;
        };
        time += x;
        bits += n;
    }
    if bits > 0.0 { time / bits } else { f64::NAN }
}

/// Integral of the straight-line interpolated waveform from `t0` to `t1` (seconds from the first sample).
fn integral(v: &[f32], dt: f64, t0: f64, t1: f64) -> f64 {
    let at = |x: f64| {
        let i = (x.floor().max(0.0) as usize).min(v.len() - 1);
        let j = (i + 1).min(v.len() - 1);
        v[i] as f64 + (v[j] as f64 - v[i] as f64) * (x - i as f64)
    };
    let (x0, x1) = (t0 / dt, t1 / dt);
    let (a, b) = (x0.ceil() as usize, (x1.floor() as usize).min(v.len() - 1));
    if b < a {
        return (at(x0) + at(x1)) / 2.0 * (x1 - x0) * dt;
    }
    let mut s = (at(x0) + v[a] as f64) / 2.0 * (a as f64 - x0);
    for k in a..b {
        s += (v[k] as f64 + v[k + 1] as f64) / 2.0;
    }
    s += (v[b] as f64 + at(x1)) / 2.0 * (x1 - b as f64);
    s * dt
}

/// Per span between consecutive mid edges: the time from the first to the last sample beyond the reference level
/// (upper level in a high span, lower level in a low span), with straight-line interpolation at both crossings.
fn outside_spans(v: &[f32], dt: f64, a: &Analysis) -> (Vec<f64>, Vec<f64>) {
    let amp = a.top - a.base;
    let (up, lo) = (a.base + a.hpct * amp, a.base + a.lpct * amp);
    let (mut hi_w, mut lo_w) = (Vec::new(), Vec::new());
    let last = v.len().saturating_sub(1);
    for w in edge_seq(a).windows(2) {
        let ((t0, rising), (t1, _)) = (w[0], w[1]);
        let (s, e) = (((t0 / dt).floor().max(0.0) as usize).min(last), ((t1 / dt).ceil() as usize).min(last));
        let lvl = if rising { up } else { lo };
        let beyond = |x: f32| x.is_finite() && if rising { x as f64 >= lvl } else { x as f64 <= lvl };
        let width = match ((s..=e).find(|&i| beyond(v[i])), (s..=e).rev().find(|&i| beyond(v[i]))) {
            (Some(i), Some(j)) => {
                let cross = |k: usize, m: usize| {
                    let (yk, ym) = (v[k] as f64, v[m] as f64);
                    if ym == yk {
                        m as f64 * dt
                    } else {
                        (k as f64 + (lvl - yk) / (ym - yk) * (m as f64 - k as f64)) * dt
                    }
                };
                let ta = if i > s && v[i - 1].is_finite() { cross(i - 1, i) } else { i as f64 * dt };
                let tb = if j < e && v[j + 1].is_finite() { cross(j, j + 1) } else { j as f64 * dt };
                tb - ta
            }
            _ => 0.0,
        };
        if rising { hi_w.push(width) } else { lo_w.push(width) }
    }
    (hi_w, lo_w)
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

/// Delay from the first selected edge of A to the first selected edge of B in the record, seconds (negative when B's
/// edge comes first).
pub fn delay(a: &Analysis, b: &Analysis, p: Pair) -> f64 {
    match (edge_list(a, p.0).first(), edge_list(b, p.1).first()) {
        (Some(&ta), Some(&tb)) => tb - ta,
        _ => f64::NAN,
    }
}

/// Phase of B relative to A in degrees, 0..360: for every selected edge of B inside a cycle of A (from one selected
/// edge of A to the next), (B - cycle start) / cycle length x 360. The mean over all such edges.
pub fn phase(a: &Analysis, b: &Analysis, p: Pair) -> f64 {
    let (ea, eb) = (edge_list(a, p.0), edge_list(b, p.1));
    let (mut d, mut j) = (Vec::new(), 0usize);
    for w in ea.windows(2) {
        while j < eb.len() && eb[j] < w[0] {
            j += 1;
        }
        while j < eb.len() && eb[j] < w[1] {
            d.push((eb[j] - w[0]) / (w[1] - w[0]) * 360.0);
            j += 1;
        }
    }
    mean(&d)
}

fn edge_list<'a>(a: &'a Analysis, rising: bool) -> &'a [f64] {
    if rising { &a.rises } else { &a.falls }
}

/// Default skew window, seconds: only B edges within this of an A edge are paired.
pub const SKEW_WINDOW: (f64, f64) = (-10e-9, 10e-9);

/// Mean skew (B edge - A edge) with the default window of +/- 10 ns.
pub fn skew(a: &Analysis, b: &Analysis, p: Pair) -> f64 {
    skew_within(a, b, p, SKEW_WINDOW.0, SKEW_WINDOW.1)
}

/// Mean skew over edge pairs whose B - A lies in `min..=max`. Pairs are found greedily in time order: a B edge too
/// early for the window moves to the next B, too late moves to the next A, and a later A nearer to the same B wins.
/// Edges outside the window are never paired.
pub fn skew_within(a: &Analysis, b: &Analysis, p: Pair, min: f64, max: f64) -> f64 {
    let (ea, eb) = (edge_list(a, p.0), edge_list(b, p.1));
    let (mut d, mut i, mut j) = (Vec::new(), 0usize, 0usize);
    while i < ea.len() && j < eb.len() {
        let mut x = eb[j] - ea[i];
        if x < min {
            j += 1;
            continue;
        }
        if x > max {
            i += 1;
            continue;
        }
        while i + 1 < ea.len() {
            let y = eb[j] - ea[i + 1];
            if y >= min && y <= max && y.abs() < x.abs() {
                x = y;
                i += 1;
            } else {
                break;
            }
        }
        d.push(x);
        i += 1;
        j += 1;
    }
    mean(&d)
}

/// The clock edges that latch data: rising, falling, or both (DDR strobes such as DQS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Latch {
    Rise,
    Fall,
    Both,
}

impl Latch {
    pub fn of(rise: bool) -> Latch {
        if rise { Latch::Rise } else { Latch::Fall }
    }

    fn edges(self, clock: &Analysis) -> Vec<f64> {
        match self {
            Latch::Rise => clock.rises.clone(),
            Latch::Fall => clock.falls.clone(),
            Latch::Both => mids(clock),
        }
    }
}

/// Setup: clock edge minus the last data mid before it. Positive when the data edge leads the clock.
/// `rise` selects rising or falling clock edges; data uses either-direction mids.
///
/// Only a data change inside the clock edge's own cycle counts: after the previous latching edge and at most one
/// typical edge spacing before this one. A clock edge whose data did not change in its cycle has no setup of its
/// own. Without this, data that holds steady (an idle or repeating DQ between strobe bursts) pairs every clock edge
/// with a transition from long ago and the mean reads hundreds of UIs.
pub fn setup_time(clock: &Analysis, data: &Analysis) -> f64 {
    setup_time_at(clock, data, true)
}

pub fn setup_time_at(clock: &Analysis, data: &Analysis, rise: bool) -> f64 {
    setup_time_on(clock, data, Latch::of(rise))
}

pub fn setup_time_on(clock: &Analysis, data: &Analysis, latch: Latch) -> f64 {
    mean(&setup_hold_times(clock, data, latch).0)
}

/// Hold: the first data mid after the clock edge minus the edge, when it comes before the next latching edge and
/// within one typical edge spacing (as for setup).
pub fn hold_time(clock: &Analysis, data: &Analysis) -> f64 {
    hold_time_at(clock, data, true)
}

pub fn hold_time_at(clock: &Analysis, data: &Analysis, rise: bool) -> f64 {
    hold_time_on(clock, data, Latch::of(rise))
}

pub fn hold_time_on(clock: &Analysis, data: &Analysis, latch: Latch) -> f64 {
    mean(&setup_hold_times(clock, data, latch).1)
}

/// Default range of accepted setup and hold times, seconds (min, max), as on the reference scope.
pub const SETUP_HOLD_WINDOW: (f64, f64) = (0.0, 10e-9);

/// Every setup and hold time with the default range `SETUP_HOLD_WINDOW`.
pub fn setup_hold_times(clock: &Analysis, data: &Analysis, latch: Latch) -> (Vec<f64>, Vec<f64>) {
    setup_hold_times_within(clock, data, latch, SETUP_HOLD_WINDOW.0, SETUP_HOLD_WINDOW.1)
}

/// Every setup and hold time between `min` and `max` (inclusive, seconds; negative values allowed). Clock edges are
/// those `latch` selects, data edges are every mid crossing of the data. Setup pairs each clock edge with the nearest
/// data edge before it (clock - data); hold pairs each data edge with the nearest clock edge before it (data - clock).
/// An edge is used in one pair at most, and a pair counts only when its value is inside the range.
pub fn setup_hold_times_within(clock: &Analysis, data: &Analysis, latch: Latch, min: f64, max: f64) -> (Vec<f64>, Vec<f64>) {
    let (c, d) = (latch.edges(clock), mids(data));
    let (mut setup, mut hold) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0usize, 0usize);
    while i < c.len() && j < d.len() {
        let x = d[j] - c[i];
        if x < -max {
            j += 1;
        } else if x > -min {
            i += 1;
        } else if j + 1 < d.len() && d[j + 1] - c[i] < -min {
            j += 1; // a later data edge is also far enough before this clock edge: take the nearest
        } else {
            setup.push(-x);
            (i, j) = (i + 1, j + 1);
        }
    }
    let (mut i, mut j) = (0usize, 0usize);
    while i < c.len() && j < d.len() {
        let x = d[j] - c[i];
        if x < min {
            j += 1;
        } else if x > max {
            i += 1;
        } else if i + 2 < c.len() && d[j] - c[i + 1] > min {
            i += 1; // a later clock edge also comes far enough before this data edge: take the nearest
        } else {
            hold.push(x);
            (i, j) = (i + 1, j + 1);
        }
    }
    (setup, hold)
}

/// Median spacing of sorted edges; infinite when there are fewer than two.
fn typical_spacing(c: &[f64]) -> f64 {
    let mut gaps: Vec<f64> = c.windows(2).map(|w| w[1] - w[0]).filter(|g| g.is_finite() && *g > 0.0).collect();
    if gaps.is_empty() {
        return f64::INFINITY;
    }
    let k = gaps.len() / 2;
    gaps.select_nth_unstable_by(k, f64::total_cmp);
    gaps[k]
}

/// Per latching clock edge: (setup, hold), each present only when the data changed inside that edge's cycle.
fn cycles(clock: &Analysis, data: &Analysis, latch: Latch) -> Vec<(Option<f64>, Option<f64>)> {
    let (c, d) = (latch.edges(clock), mids(data));
    let span = typical_spacing(&c);
    let mut out = Vec::with_capacity(c.len());
    let mut j = 0usize;
    for (i, &tc) in c.iter().enumerate() {
        if !tc.is_finite() {
            continue;
        }
        let prev = if i > 0 { c[i - 1].max(tc - span) } else { tc - span };
        let next = if i + 1 < c.len() { c[i + 1].min(tc + span) } else { tc + span };
        while j < d.len() && d[j] < tc {
            j += 1;
        }
        let setup = (j > 0 && d[j - 1] > prev).then(|| tc - d[j - 1]);
        let mut k = j;
        while k < d.len() && d[k] <= tc {
            k += 1;
        }
        let hold = (k < d.len() && d[k] < next).then(|| d[k] - tc);
        out.push((setup, hold));
    }
    out
}

/// Clock edges whose setup is below `tsetup` or hold below `thold`.
pub fn setup_hold_violations(clock: &Analysis, data: &Analysis, tsetup: f64, thold: f64) -> f64 {
    setup_hold_violations_at(clock, data, tsetup, thold, true)
}

pub fn setup_hold_violations_at(clock: &Analysis, data: &Analysis, tsetup: f64, thold: f64, rise: bool) -> f64 {
    setup_hold_violations_on(clock, data, tsetup, thold, Latch::of(rise))
}

pub fn setup_hold_violations_on(clock: &Analysis, data: &Analysis, tsetup: f64, thold: f64, latch: Latch) -> f64 {
    cycles(clock, data, latch)
        .iter()
        .filter(|(s, h)| s.is_some_and(|s| s < tsetup) || h.is_some_and(|h| h < thold))
        .count() as f64
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

    /// DDR write: a strobe toggling every UI in bursts with long idle gaps between them, and data centred
    /// between strobe edges that changes only now and then.
    fn ddr_write() -> (Analysis, Analysis, f64) {
        let ui = 375e-12;
        let (mut rises, mut falls, mut data) = (Vec::new(), Vec::new(), Vec::new());
        for burst in 0..20 {
            let t0 = burst as f64 * 300e-9;
            for k in 0..16 {
                let t = t0 + k as f64 * ui;
                if k % 2 == 0 { rises.push(t) } else { falls.push(t) }
            }
            // two data changes per burst, each half a UI before a strobe edge (rising in one, falling in the other)
            data.push(t0 + 3.5 * ui);
            data.push(t0 + 10.5 * ui);
        }
        let clock = Analysis { valid: true, rises, falls, ..Analysis::default() };
        let (rises, falls): (Vec<f64>, Vec<f64>) = (data.iter().step_by(2).copied().collect(), data.iter().skip(1).step_by(2).copied().collect());
        (clock, Analysis { valid: true, rises, falls, ..Analysis::default() }, ui)
    }

    #[test]
    fn steady_data_between_strobe_bursts_does_not_pair_with_stale_transitions() {
        let (dqs, dq, ui) = ddr_write();
        // both strobe edges latch: every data change is half a UI from its edges on either side
        let (setups, holds) = setup_hold_times(&dqs, &dq, Latch::Both);
        assert_eq!((setups.len(), holds.len()), (40, 40), "one setup and one hold per data change");
        for t in setups.iter().chain(&holds) {
            assert!((t - 0.5 * ui).abs() < 1e-15, "{}", t);
        }
        assert!((setup_time_on(&dqs, &dq, Latch::Both) - 0.5 * ui).abs() < 1e-15);
        assert!((hold_time_on(&dqs, &dq, Latch::Both) - 0.5 * ui).abs() < 1e-15);
        // one edge direction: the stable time inside that edge's own cycle (two UIs), never the idle gap
        for latch in [Latch::Rise, Latch::Fall] {
            let (s, h) = setup_hold_times(&dqs, &dq, latch);
            assert!(!s.is_empty() && !h.is_empty());
            assert!(s.iter().chain(&h).all(|t| *t > 0.0 && *t <= 2.0 * ui + 1e-15), "{:?} {:?}", s, h);
        }
        assert_eq!(setup_hold_violations_on(&dqs, &dq, 0.4 * ui, 0.4 * ui, Latch::Both), 0.0);
        assert_eq!(setup_hold_violations_on(&dqs, &dq, 0.6 * ui, 0.0, Latch::Both), 40.0);
    }

    #[test]
    fn dense_single_edge_clock_keeps_its_setup_and_hold() {
        // data changes once every clock period, 3 ns before each rising edge of a 10 ns clock
        let rises: Vec<f64> = (1..50).map(|k| k as f64 * 10e-9).collect();
        let falls: Vec<f64> = rises.iter().map(|t| t + 5e-9).collect();
        let d: Vec<f64> = rises.iter().map(|t| t - 3e-9).collect();
        let clock = Analysis { valid: true, rises, falls, ..Analysis::default() };
        let data = Analysis { valid: true, rises: d.iter().step_by(2).copied().collect(), falls: d.iter().skip(1).step_by(2).copied().collect(), ..Analysis::default() };
        assert!((setup_time(&clock, &data) - 3e-9).abs() < 1e-15);
        // the next change is 7 ns after each edge, one before the last edge has none
        assert!((hold_time(&clock, &data) - 7e-9).abs() < 1e-15);
        assert_eq!(setup_hold_times(&clock, &data, Latch::Rise).0.len(), 49);
    }
}
