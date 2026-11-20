// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The levels of a record and what is built on them: `Stats` (extremes, top and base, period), the periodicity
//! and noise checks, and the auto set-up of live channels and of shown references (`autoset`, `place`). A new figure
//! of a record that auto set-up needs goes here.

use super::*;

// the levels are computed in oos-dsp (ADR 0001); auto set-up and the checks below build on them here
pub use oos_dsp::levels::{stats, Stats};

/// Auto set-up: per channel find amplitude and frequency, choose V/div, offset, timebase and trigger.
/// Live channels only. A shown reference uses `autoset_shown`.
pub fn autoset(m: &mut Model, src: &Source) {
    // pass 1: which channels carry a signal, their levels and periods
    let mut found: Vec<(usize, Stats, f64)> = Vec::new();
    for c in (0..NCH).filter(|&c| m.ch[c].on) {
        let v = src.probe(c, 0.0137, 1e-6, 40000);
        let s = stats(&v, 1e-6);
        if s.vmax - s.vmin < 0.05 {
            continue;
        }
        let mut period = s.period;
        if period.is_finite() && period < 50e-6 {
            let v2 = src.probe(c, 0.0137, period / 200.0, 4000);
            let s2 = stats(&v2, period / 200.0);
            if s2.period.is_finite() {
                period = s2.period;
            }
        }
        found.push((c, s, period));
    }
    place(m, &found);
}

/// Auto set-up of stored records. Each entry is one shown reference: its samples and its own sample interval.
/// Vertical scale and position come from that record's levels, the same band rule as a live channel (the peak-to-peak
/// fills 80 % of its band, the midpoint sits on the band centre). The time base shows about three periods of the
/// slowest record. Analog channels, the trigger and the run state stay as they are.
/// `None` where the record is flatter than the live set-up's floor (50 mV).
#[cfg(test)]
pub fn autoset_shown(m: &mut Model, waves: &[(&[f32], f64)]) -> Vec<Option<(f32, f32)>> {
    let measured: Vec<Stats> = waves.iter().map(|(v, dt)| autoset_summary(v, *dt)).collect();
    let hot: Vec<bool> = measured.iter().map(|s| s.vmax - s.vmin >= 0.05).collect();
    let n = hot.iter().filter(|&&h| h).count();
    if n == 0 {
        return vec![None; waves.len()];
    }
    let band = VDIV / n as f32;
    let mut k = 0usize;
    let mut slowest = 0.0f64;
    let mut out = Vec::with_capacity(waves.len());
    for (s, &h) in measured.iter().zip(&hot) {
        if !h {
            out.push(None);
            continue;
        }
        let y_div = VDIV / 2.0 - band * (k as f32 + 0.5);
        let vpp = s.vmax - s.vmin;
        let want = vpp / (band * 0.8);
        let vpd = VSCALES.iter().copied().find(|&x| x >= want).unwrap_or(VSCALES[VSCALES.len() - 1]);
        let mid = (s.vmax + s.vmin) / 2.0;
        out.push(Some((vpd, y_div * vpd - mid)));
        if s.period.is_finite() && s.period > slowest {
            slowest = s.period;
        }
        k += 1;
    }
    if slowest > 0.0 {
        let ts = tscales();
        let want = (slowest * 3.0 / HDIV as f64) as f32;
        m.tscale = ts.iter().position(|&t| t >= want).unwrap_or(ts.len() - 1);
        m.tfine = 1.0;
        m.delay = 0.0;
    }
    out
}

/// Display setup is bounded independently of file size. Native contiguous windows
/// retain fast edges; a separate record-wide sample supplies slow-signal context.
/// This is an autoset estimate, never a replacement for measurement analysis.
pub(crate) fn autoset_summary(v: &[f32], dt: f64) -> Stats {
    const WINDOW: usize = 16_384;
    if v.len() <= WINDOW * 4 {
        return stats(v, dt);
    }
    if !dt.is_finite() || dt <= 0.0 {
        return Stats::default();
    }
    let mut summary = Stats { vmin: f32::INFINITY, vmax: f32::NEG_INFINITY, period: f64::NAN, ..Stats::default() };
    let mut candidates = [(0.0f32, f64::NAN); 3];
    for (index, start) in [0, (v.len() - WINDOW) / 2, v.len() - WINDOW].into_iter().enumerate() {
        let window = stats(&v[start..start + WINDOW], dt);
        if !window.valid {
            continue;
        }
        summary.valid = true;
        summary.vmin = summary.vmin.min(window.vmin);
        summary.vmax = summary.vmax.max(window.vmax);
        candidates[index] = (window.vmax - window.vmin, window.period);
    }
    let stride = v.len().div_ceil(WINDOW);
    let coarse: Vec<f32> = v.iter().step_by(stride).copied().collect();
    let wide = stats(&coarse, dt * stride as f64);
    if wide.valid {
        summary.valid = true;
        summary.vmin = summary.vmin.min(wide.vmin);
        summary.vmax = summary.vmax.max(wide.vmax);
    }
    // Small fast noise on a slow waveform is not its period. A candidate must
    // carry a substantial part of the observed swing, as in hardware autoset.
    let swing = summary.vmax - summary.vmin;
    summary.period = candidates
        .into_iter()
        .filter(|&(height, period)| height >= 0.4 * swing && period.is_finite() && period > 0.0)
        .map(|(_, period)| period)
        .min_by(f64::total_cmp)
        .unwrap_or(wide.period);
    summary
}

/// A steady period in `v`, sample distance `dt`: at least three rising edges with hysteresis, spaced within 30 %.
/// Neighbouring points are averaged first, the min/max pairs of a dense record become its middle. Noise and spikes
/// have no steady spacing.
pub fn periodic(v: &[f32], dt: f64) -> Option<f64> {
    let p: Vec<f32> = v.as_chunks::<2>().0.iter().map(|x| (x[0] + x[1]) / 2.0).collect();
    let (lo, hi) = (p.iter().copied().fold(f32::MAX, f32::min), p.iter().copied().fold(f32::MIN, f32::max));
    if !(hi > lo) {
        return None;
    }
    let (mid, h) = ((hi + lo) / 2.0, (hi - lo) * 0.2);
    let mut rises = Vec::new();
    let mut low = false;
    for (i, &x) in p.iter().enumerate() {
        if x < mid - h {
            low = true;
        } else if low && x > mid + h {
            low = false;
            rises.push(i);
        }
    }
    if rises.len() < 3 {
        return None;
    }
    let gaps: Vec<usize> = rises.windows(2).map(|w| w[1] - w[0]).collect();
    let (min, max) = (*gaps.iter().min()?, *gaps.iter().max()?);
    if (max as f64) > min.max(1) as f64 * 1.3 + 1.0 {
        return None;
    }
    Some((rises[rises.len() - 1] - rises[0]) as f64 / gaps.len() as f64 * 2.0 * dt)
}

/// The trigger level halfway through a signal: between top and base where it has flat levels (a spike or overshoot
/// moves the extremes, not these), else between the extremes. Used by level 50 % and the auto set-up.
pub fn mid_level(s: &Stats) -> f32 {
    if s.top > s.base {
        (s.top + s.base) / 2.0
    } else {
        (s.vmax + s.vmin) / 2.0
    }
}

/// Noise of a two-level signal (square, logic) as a standard deviation: from the median absolute deviation of each
/// half about its median (split halfway between the extremes), so the samples on the edges do not count. None when the
/// halves are not clearly apart (6 sigma), as for a sine or a triangle, whose halves spread by the signal itself.
/// `stats`' histogram top and base are no help here: noise of a fifth of the swing flattens their modes.
pub fn flat_noise(v: &[f32]) -> Option<f32> {
    let f: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
    let (lo, hi) = f.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    if f.len() < 32 || hi <= lo {
        return None;
    }
    let mid = (lo + hi) / 2.0;
    let median = |v: &mut Vec<f32>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let half = |up: bool| -> Option<(f32, f32)> {
        let mut h: Vec<f32> = f.iter().copied().filter(|&x| (x >= mid) == up).collect();
        if h.len() < 8 {
            return None;
        }
        let m = median(&mut h);
        let mut dev: Vec<f32> = h.iter().map(|x| (x - m).abs()).collect();
        Some((m, 1.4826 * median(&mut dev)))
    };
    let ((m1, s1), (m0, s0)) = (half(true)?, half(false)?);
    let sigma = s1.max(s0);
    (m1 - m0 > 6.0 * sigma).then_some(sigma)
}

/// The end of an auto set-up: `found` = channel, levels, period; the rest of the set-up stays.
pub fn place(m: &mut Model, found: &[(usize, Stats, f64)]) {
    // one horizontal band per channel (8 div / n), signal fills 80 % of its band; in a split plot per window: n and the
    // band count the channels of the window
    let nw = if m.zoom || m.view != AView::Scope { 1 } else { m.win_layout.count() };
    let win_of = |c: usize| m.ch_win[c].min(nw - 1);
    let wins: Vec<usize> = found.iter().map(|f| win_of(f.0)).collect();
    for (i, (c, s, _)) in found.iter().enumerate() {
        let n = wins.iter().filter(|&&w| w == wins[i]).count().max(1) as f32;
        let k = wins[..i].iter().filter(|&&w| w == wins[i]).count();
        let band = VDIV / n;
        let ch = &mut m.ch[*c];
        ch.on = true;
        ch.coupling = Coupling::Dc;
        let vpp = s.vmax - s.vmin;
        let want = vpp / (band * 0.8);
        ch.vscale = VSCALES.iter().position(|&x| x * ch.probe >= want).unwrap_or(VSCALES.len() - 1);
        ch.vfine = 1.0;
        let vpd = ch.volts_per_div();
        let y_div = VDIV / 2.0 - band * (k as f32 + 0.5);
        let mid = (s.vmax + s.vmin) / 2.0;
        ch.offset = y_div * vpd - mid;
    }
    if let Some((c, s, _)) = found.first() {
        m.trig_src = *c;
        m.trig_level = mid_level(s);
        m.slope = Slope::Rise;
        m.trig_mode = TrigMode::Auto;
        m.active = *c;
    }
    let slowest = found.iter().map(|f| f.2).filter(|p| p.is_finite()).fold(0.0, f64::max);
    if slowest > 0.0 {
        let ts = tscales();
        let want = (slowest * 3.0 / HDIV as f64) as f32;
        m.tscale = ts.iter().position(|&t| t >= want).unwrap_or(ts.len() - 1);
        m.tfine = 1.0;
    }
    m.delay = 0.0;
    m.run = RunState::Run;
}
