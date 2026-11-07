// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Spectrum analyser mode: RBW-controlled FFT (decimation, Welch segments and zero padding on top of `math::fft`), trace
//! averaging / max hold / min hold, detectors, the search for spectral lines, markers with interpolated peaks, distortion
//! after IEEE 1241 (THD, THD+N, SNR, SINAD, SFDR, ENOB), channel power, occupied bandwidth and a waterfall.
//!
//! Power values are mean-square volts (V², the power into 1 Ω); a sine of amplitude A on its own reads A²/2, i.e. its
//! RMS value in dBV, independent of the window (coherent-gain normalisation like `math::spectrum`).

use crate::math::{fft, Window};
use std::collections::VecDeque;
use std::f64::consts::PI;

/// Equivalent noise bandwidth of a window in FFT bins (N Σw² / (Σw)²): Rect 1.000, Hann 1.500, Hamming 1.363,
/// Blackman 1.727, Flattop 3.770. RBW (Hz) = ENBW × fs / window length.
pub fn enbw(w: Window) -> f64 {
    match w {
        Window::Rect => 1.0,
        Window::Hann => 1.5,
        Window::Hamming => 1.363,
        Window::Blackman => 1.727,
        Window::Flattop => 3.770,
    }
}

/// Half width of a window's main lobe in FFT bins of its length.
pub fn lobe_bins(w: Window) -> f64 {
    match w {
        Window::Rect => 1.0,
        Window::Hann | Window::Hamming => 2.0,
        Window::Blackman => 3.0,
        Window::Flattop => 5.0,
    }
}

/// Side lobe envelope of a window in dB, `bins` from the centre of a line: first lobe, its place, fall per octave.
fn sidelobe_db(w: Window, bins: f64) -> f64 {
    let (first, at, fall) = match w {
        Window::Rect => (-13.0, 1.5, 6.0),
        Window::Hann => (-31.0, 2.5, 18.0),
        Window::Hamming => (-40.0, 2.5, 4.0),
        Window::Blackman => (-58.0, 3.5, 12.0),
        Window::Flattop => (-90.0, 5.5, 6.0),
    };
    first - fall * (bins / at).max(1.0).log2()
}

/// Samples read of a very long record.
const MAX_SAMPLES: usize = 1 << 23;
/// Longest window, and longest FFT with zero padding.
const MAX_WINDOW: usize = 1 << 17;
const MAX_FFT: usize = 1 << 18;
/// FFT points of all Welch segments of one call.
const MAX_WORK: usize = 1 << 19;
/// Decimation leaves the sample rate at least this many times the top of the band: aliases stay 55 dB down.
const OVERSAMPLE: f64 = 8.0;

/// One halving step in place: binomial low-pass 1 4 6 4 1, every second sample kept. Its gain is cos^4(pi f / fs).
fn halve(x: &mut Vec<f32>) {
    let m = (x.len() - 3) / 2;
    for i in 0..m {
        let c = 2 * i + 2;
        x[i] = (x[c - 2] + x[c + 2] + 4.0 * (x[c - 1] + x[c + 1]) + 6.0 * x[c]) * 0.0625;
    }
    x.truncate(m);
}

/// How several FFT bins falling into one display bin are combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detector {
    /// largest bin (never misses a narrow tone)
    Peak,
    /// value at the display frequency (interpolated)
    Sample,
    /// mean power (correct noise and channel power readings)
    Avg,
}

/// Trace processing across calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trace {
    /// every call shows the current spectrum
    Normal,
    /// linear power average over `Settings::averaging` calls (exponential once the count is reached)
    Average,
    MaxHold,
    MinHold,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Hz
    pub center: f64,
    /// Hz; 0 = full span (0 .. fs/2)
    pub span: f64,
    /// resolution bandwidth (noise bandwidth) in Hz; 0 = auto (span / 100)
    pub rbw: f64,
    pub window: Window,
    /// number of traces for `Trace::Average`
    pub averaging: u32,
    pub trace: Trace,
    pub detector: Detector,
    /// display bins; 0 = 801
    pub points: usize,
    /// top of the display (dBV); not applied to the data
    pub ref_level_dbv: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { center: 0.0, span: 0.0, rbw: 0.0, window: Window::Flattop, averaging: 16, trace: Trace::Normal, detector: Detector::Peak, points: 0, ref_level_dbv: 0.0 }
    }
}

/// A display trace: bin i is at f0 + i df.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spectrum {
    pub f0: f64,
    pub df: f64,
    pub dbv: Vec<f32>,
    /// actual resolution (noise) bandwidth, Hz
    pub rbw: f64,
    pub ref_level_dbv: f64,
    /// spectral lines of the band, strongest first (`find_lines`)
    pub lines: Vec<Marker>,
    /// half width of a line's main lobe, Hz
    pub lobe: f64,
    /// noise floor from the lowest tenth of the band, dBV per RBW
    pub floor_dbv: f64,
}

impl Spectrum {
    pub fn freq(&self, i: usize) -> f64 {
        self.f0 + i as f64 * self.df
    }
    /// mean-square volts of bin i
    pub fn power(&self, i: usize) -> f64 {
        10f64.powf(self.dbv[i] as f64 / 10.0)
    }
}

/// Trace averaging state kept by the caller between `analyze` calls (reset automatically when the display grid or
/// RBW changes; set it to `None` to restart).
#[derive(Clone, Debug, Default)]
pub struct Avg {
    key: (u64, u64, usize, u64),
    n: u32,
    avg: Vec<f64>,
    max: Vec<f64>,
    min: Vec<f64>,
    /// the window of the last call and the power of its main lobe per FFT bin: they cost more than a short FFT
    win_of: Option<(Window, usize, usize)>,
    win: Vec<f64>,
    lobe_pow: Vec<f64>,
}

impl Avg {
    /// traces accumulated so far
    pub fn count(&self) -> u32 {
        self.n
    }
}

/// Spectrum of `v` for the display described by `s`. A band far below Nyquist is first decimated by halving steps (the
/// record time and so the resolution stay, the FFT gets short; the steps' droop is taken out again). The window length is
/// chosen so that the window's ENBW gives the requested RBW (limited by the record length and `MAX_WINDOW`;
/// `Spectrum::rbw` holds the result); a longer record is split into 50 %-overlapping segments whose power spectra are
/// averaged (Welch, at most `MAX_WORK` FFT points in total); each segment is zero-padded until the FFT bins are a quarter
/// of the display bins or narrower (at most 8x, `MAX_FFT` points). The detector then maps the FFT bins onto the display
/// bins, and the trace mode combines the result with `state`. The lines are searched at FFT resolution in Normal, on
/// the processed trace in the other trace modes.
pub fn analyze(v: &[f32], dt: f64, s: &Settings, state: &mut Option<Avg>) -> Spectrum {
    let mut x: Vec<f32> = v.iter().filter(|x| x.is_finite()).take(MAX_SAMPLES).copied().collect();
    if x.len() < 16 || !(dt > 0.0) || !dt.is_finite() {
        return Spectrum { ref_level_dbv: s.ref_level_dbv, ..Default::default() };
    }
    let fs0 = 1.0 / dt;
    let nyq = fs0 / 2.0;
    let (mut start, mut stop) = if s.span > 0.0 && s.center.is_finite() { ((s.center - s.span / 2.0).max(0.0), (s.center + s.span / 2.0).min(nyq)) } else { (0.0, nyq) };
    if !(stop > start) {
        (start, stop) = (0.0, nyq);
    }
    let mut fs = fs0;
    let mut stages = 0;
    while fs / 2.0 >= OVERSAMPLE * stop && x.len() >= 128 {
        halve(&mut x);
        fs /= 2.0;
        stages += 1;
    }
    let m = x.len();
    let points = if s.points >= 2 { s.points } else { 801 };
    let ddf = (stop - start) / (points - 1) as f64;
    let e = enbw(s.window);
    let rbw_req = if s.rbw > 0.0 && s.rbw.is_finite() { s.rbw } else { (stop - start) / 100.0 };
    let l = ((e * fs / rbw_req).round().min(1e9) as usize).clamp(16, m.min(MAX_WINDOW));
    // where the record sets the window, a record that varies by a sample must not restart the averaging
    let l = if l == m && l >= 256 { l / 64 * 64 } else { l };
    let rbw = e * fs / l as f64;
    let n0 = l.next_power_of_two();
    let mut n = n0;
    while fs / n as f64 > ddf / 4.0 && n < 8 * n0 && n < MAX_FFT {
        n *= 2;
    }
    let df = fs / n as f64;
    let hop = (l / 2).max(1);
    let nseg_all = (m - l) / hop + 1;
    let nseg = nseg_all.min((MAX_WORK / n).max(1));
    let st = state.get_or_insert_with(Avg::default);
    if st.win_of != Some((s.window, l, n)) {
        st.win = (0..l).map(|i| s.window.w(i, l)).collect();
        st.win_of = Some((s.window, l, n));
        let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
        re[..l].copy_from_slice(&st.win);
        fft(&mut re, &mut im);
        let bins = ((lobe_bins(s.window) * n as f64 / l as f64).ceil() as usize + 1).min(n / 2);
        st.lobe_pow = (0..=bins).map(|j| re[j] * re[j] + im[j] * im[j]).collect();
    }
    let w = &st.win;
    let wsum: f64 = w.iter().sum();
    // the mean goes before the window: no DC skirt over the low lines. The trace gets DC back further down.
    let mean = x.iter().map(|&v| v as f64).sum::<f64>() / m as f64;
    let mut pow = vec![0.0f64; n / 2 + 1];
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    for k in 0..nseg {
        let off = if nseg > 1 { k * (nseg_all - 1) / (nseg - 1) * hop } else { 0 };
        re.iter_mut().for_each(|r| *r = 0.0);
        im.iter_mut().for_each(|r| *r = 0.0);
        for i in 0..l {
            re[i] = (x[off + i] as f64 - mean) * w[i];
        }
        fft(&mut re, &mut im);
        for (j, p) in pow.iter_mut().enumerate() {
            let a = (re[j] * re[j] + im[j] * im[j]) / (wsum * wsum);
            *p += if j == 0 { a } else { 2.0 * a };
        }
    }
    pow.iter_mut().for_each(|p| *p /= nseg as f64);
    // the droop of the halving steps: their gains multiply to (sin(g t) / (g sin t))^4
    if stages > 0 {
        let g = (1u64 << stages) as f64;
        for (j, p) in pow.iter_mut().enumerate().skip(1) {
            let t = PI * j as f64 * df / fs0;
            *p /= ((g * t).sin() / (g * t.sin())).powi(8);
        }
    }
    let lobe = lobe_bins(s.window) * fs / l as f64;
    let found = (s.trace == Trace::Normal).then(|| {
        let (k0, k1) = (((start / df).ceil() as usize).min(n / 2), ((stop / df).floor() as usize).min(n / 2));
        find_lines(&pow[k0..=k1.max(k0)], k0 as f64 * df, df, 0.0, s.window, fs / l as f64)
    });
    for (j, a) in st.lobe_pow.iter().enumerate() {
        pow[j] += mean * mean * a / (wsum * wsum) * if j == 0 { 1.0 } else { 2.0 };
    }
    // detector: FFT bins -> display bins
    let interp = |f: f64| {
        let k = f / df;
        let k0 = (k.floor() as usize).min(n / 2);
        let k1 = (k0 + 1).min(n / 2);
        let a = k - k0 as f64;
        pow[k0] * (1.0 - a) + pow[k1] * a
    };
    let cur: Vec<f64> = (0..points)
        .map(|i| {
            let f = start + i as f64 * ddf;
            let lo = (((f - ddf / 2.0) / df).ceil().max(0.0) as usize).min(n / 2 + 1);
            let hi = ((((f + ddf / 2.0) / df).ceil() - 1.0).max(-1.0) as isize).min(n as isize / 2);
            if s.detector == Detector::Sample || hi < lo as isize {
                return interp(f);
            }
            let bins = &pow[lo..=hi as usize];
            match s.detector {
                Detector::Peak => bins.iter().fold(0.0f64, |a, &b| a.max(b)),
                _ => bins.iter().sum::<f64>() / bins.len() as f64,
            }
        })
        .collect();
    // trace averaging
    let key = (start.to_bits(), ddf.to_bits(), points, rbw.to_bits());
    if st.key != key || st.avg.len() != points {
        (st.key, st.n, st.avg, st.max, st.min) = (key, 0, cur.clone(), cur.clone(), cur.clone());
    }
    st.n = st.n.saturating_add(1);
    let wgt = 1.0 / st.n.min(s.averaging.max(1)) as f64;
    for i in 0..points {
        st.avg[i] += (cur[i] - st.avg[i]) * wgt;
        st.max[i] = st.max[i].max(cur[i]);
        st.min[i] = st.min[i].min(cur[i]);
    }
    let out = match s.trace {
        Trace::Normal => &cur,
        Trace::Average => &st.avg,
        Trace::MaxHold => &st.max,
        Trace::MinHold => &st.min,
    };
    let (lines, floor) = found.unwrap_or_else(|| find_lines(out, start, ddf, mean * mean, s.window, fs / l as f64));
    let dbv = out.iter().map(|&p| (10.0 * p.max(1e-30).log10()) as f32).collect();
    Spectrum { f0: start, df: ddf, dbv, rbw, ref_level_dbv: s.ref_level_dbv, lines, lobe, floor_dbv: 10.0 * floor.max(1e-30).log10() }
}

/// Lines kept by the search.
const MAX_LINES: usize = 32;
/// The lowest tenth of a band of noise lies this far (power) under its median: a band full of lines has no median of noise.
const LOW_TENTH: f64 = 6.6;
/// A line stands this far (power) over the noise: 15 dB, more than noise reaches in a band.
const LINE_OVER_FLOOR: f64 = 31.6;
/// A line this far (power) under the strongest bin is a window side lobe or rounding: 90 dB, past what 12 bits resolve.
const MAX_RANGE: f64 = 1e-9;
/// A leftover of a window side lobe is this much (dB) under the envelope of the stronger line, a real line above it.
const LOBE_MARGIN_DB: f64 = 6.0;
/// Lines that are no harmonic count only this close (dB) to the strongest one: the rest are spurs.
const SPUR_DB: f64 = 40.0;
/// Higher orders than this are no harmonics to rank by: everything lies near one of them.
const MAX_HARMONIC: f64 = 64.0;

/// Spectral lines in the power values `p` (bin i at f0 + i df), strongest first, and the noise floor. A line is a local
/// maximum over the floor, outside the main lobe and the side lobes of every stronger line. On a trace with DC in it, `dc`
/// is its power: it counts as such a line, so its skirt is no line. `bin` is the FFT bin width of the window's length.
fn find_lines(p: &[f64], f0: f64, df: f64, dc: f64, w: Window, bin: f64) -> (Vec<Marker>, f64) {
    if p.len() < 8 {
        return (Vec::new(), 0.0);
    }
    let mut q: Vec<f64> = p.iter().step_by((p.len() / 2048).max(1)).copied().collect();
    let tenth = q.len() / 10;
    let floor = *q.select_nth_unstable_by(tenth, f64::total_cmp).1 * LOW_TENTH;
    let thr = (floor * LINE_OVER_FLOOR).max(p.iter().copied().fold(0.0, f64::max) * MAX_RANGE);
    let mut c: Vec<usize> = (1..p.len() - 1).filter(|&i| p[i] > thr && p[i] > p[i - 1] && p[i] >= p[i + 1]).collect();
    c.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
    let lobe = lobe_bins(w);
    let hidden = |f: f64, level: f64, by: (f64, f64)| {
        let d = (f - by.0).abs() / bin;
        d <= 1.2 * lobe || 10.0 * (level / by.1).log10() < sidelobe_db(w, d) + LOBE_MARGIN_DB
    };
    let mut kept: Vec<(f64, f64)> = if dc > 0.0 { vec![(0.0, dc)] } else { Vec::new() };
    let mut at: Vec<usize> = Vec::new();
    for i in c {
        let f = f0 + i as f64 * df;
        if at.len() >= MAX_LINES {
            break;
        }
        // nearer to 0 Hz than this it is the rest of the mean, or drift
        if f >= 0.3 * lobe * bin && !kept.iter().any(|&by| hidden(f, p[i], by)) {
            kept.push((f, p[i]));
            at.push(i);
        }
    }
    let half = ((lobe * bin / df).ceil() as usize).clamp(2, 300);
    let lines = at
        .iter()
        .filter_map(|&i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(p.len()));
            let part = Spectrum { f0: f0 + a as f64 * df, df, dbv: p[a..b].iter().map(|&x| (10.0 * x.max(1e-30).log10()) as f32).collect(), ..Default::default() };
            interpolate_peak(&part, i - a)
        })
        .collect();
    (lines, floor)
}

/// Up to `n` lines of the trace, strongest first: harmonics of the fundamental, and what else stands within `SPUR_DB` of
/// the strongest line. The fundamental is the line at the measured frequency `f1`, else the strongest line.
pub fn rank_lines(s: &Spectrum, f1: Option<f64>, n: usize) -> Vec<Marker> {
    let Some(top) = s.lines.first().copied() else { return Vec::new() };
    let near = f1.filter(|f| f.is_finite() && *f > 0.0).and_then(|f| s.lines.iter().find(|m| (m.f - f).abs() <= s.lobe));
    let base = near.map_or(top.f, |m| m.f);
    let harmonic = |m: &Marker| {
        let k = (m.f / base).round().max(1.0);
        k <= MAX_HARMONIC && (m.f - k * base).abs() <= 0.5 * s.lobe + k * base * 1e-4
    };
    s.lines.iter().filter(|m| harmonic(m) || m.dbv >= top.dbv - SPUR_DB).take(n).copied().collect()
}

/// Top of the band that shows `harmonics` harmonics of `f1`, from 1, 1.5, 2, 2.5, 3, 4, 5, 6, 8 times a power of ten: the
/// frequency axis stays readable. `cur` is the top in use, kept while it fits, so a noisy `f1` does not move the axis.
pub fn auto_stop(f1: f64, harmonics: usize, cur: f64) -> f64 {
    if !(f1 > 0.0) || !f1.is_finite() || harmonics == 0 {
        return cur;
    }
    let want = (harmonics as f64 + 0.5) * f1;
    if cur > 0.0 && want <= cur && want >= 0.6 * cur {
        return cur;
    }
    let d = 10f64.powf(want.log10().floor());
    [1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0].iter().map(|k| k * d).find(|&x| x >= want * (1.0 - 1e-9)).unwrap_or(10.0 * d)
}

// ---------------------------------------------------------------------------------------------------------------
// markers

/// A marker (or a delta between two markers): frequency Hz, level dBV (dB for a delta).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub f: f64,
    pub dbv: f64,
}

/// Level at frequency `f` (linear interpolation of the dB trace).
pub fn marker_at(s: &Spectrum, f: f64) -> Option<Marker> {
    if s.dbv.is_empty() || !(s.df > 0.0) || !f.is_finite() {
        return None;
    }
    let k = ((f - s.f0) / s.df).clamp(0.0, (s.dbv.len() - 1) as f64);
    let i = (k.floor() as usize).min(s.dbv.len() - 1);
    let j = (i + 1).min(s.dbv.len() - 1);
    let a = k - i as f64;
    Some(Marker { f: s.freq(0) + k * s.df, dbv: s.dbv[i] as f64 * (1.0 - a) + s.dbv[j] as f64 * a })
}

/// Interpolated peak around bin `i`. Level: a parabola through the dB values of bins i-1, i, i+1. Frequency: the
/// centre of the lobe's -3 dB width when that spans at least 4 display bins (robust on the flat top of a Flattop lobe
/// and with the Peak detector), else the parabola's vertex. A tone reads within 0.1 % / 0.1 dB with Flattop.
pub fn interpolate_peak(s: &Spectrum, i: usize) -> Option<Marker> {
    let d = &s.dbv;
    if i >= d.len() {
        return None;
    }
    if i == 0 || i + 1 >= d.len() {
        return Some(Marker { f: s.freq(i), dbv: d[i] as f64 });
    }
    let (a, b, c) = (d[i - 1] as f64, d[i] as f64, d[i + 1] as f64);
    let den = a - 2.0 * b + c;
    let (p, level) = if den < 0.0 {
        let p = (0.5 * (a - c) / den).clamp(-0.5, 0.5);
        (p, b - 0.25 * (a - c) * p)
    } else {
        (0.0, b)
    };
    // -3 dB crossings walking down the lobe on both sides (stop at a valley)
    let cut = level - 3.0;
    let walk = |dir: isize| -> Option<f64> {
        let mut j = i as isize;
        loop {
            let k = j + dir;
            if k < 0 || k as usize >= d.len() || (i as isize - k).abs() > 256 {
                return None;
            }
            let (y0, y1) = (d[j as usize] as f64, d[k as usize] as f64);
            if y1 > y0 + 0.5 {
                return None;
            }
            if y1 < cut {
                return Some(j as f64 + dir as f64 * (y0 - cut) / (y0 - y1));
            }
            j = k;
        }
    };
    let f = match (walk(-1), walk(1)) {
        (Some(l), Some(r)) if r - l >= 4.0 => s.f0 + 0.5 * (l + r) * s.df,
        _ => s.freq(i) + p * s.df,
    };
    Some(Marker { f, dbv: level })
}

/// Peak excursion: a local maximum is a peak only when it stands this many dB above the higher of the valleys on
/// either side (the valley ends at the next higher point), so the ripple on a lobe or the noise floor is no peak.
pub const PEAK_EXCURSION_DB: f32 = 6.0;

/// Peaks of the trace (bin indices) at or above `threshold_db`.
fn local_maxima(s: &Spectrum, threshold_db: f32) -> Vec<usize> {
    let d = &s.dbv;
    let valley = |i: usize, dir: isize| {
        let (mut j, mut lo) = (i as isize + dir, d[i]);
        while j >= 0 && (j as usize) < d.len() && d[j as usize] <= d[i] {
            lo = lo.min(d[j as usize]);
            j += dir;
        }
        // running into the end of the trace counts as a valley at the lowest point seen
        lo
    };
    (1..d.len().saturating_sub(1))
        .filter(|&i| d[i] >= threshold_db && d[i] > d[i - 1] && d[i] >= d[i + 1])
        .filter(|&i| d[i] - valley(i, -1).max(valley(i, 1)) >= PEAK_EXCURSION_DB)
        .collect()
}

/// Up to `n` highest peaks at or above `threshold_db`, at least `min_separation_hz` apart (a lower peak closer than
/// that to a higher one is dropped), interpolated, highest first.
pub fn peak_table(s: &Spectrum, n: usize, threshold_db: f32, min_separation_hz: f64) -> Vec<Marker> {
    let mut p = local_maxima(s, threshold_db);
    p.sort_by(|&a, &b| s.dbv[b].total_cmp(&s.dbv[a]));
    let mut out: Vec<Marker> = Vec::new();
    for i in p {
        if out.len() >= n {
            break;
        }
        let Some(m) = interpolate_peak(s, i) else { continue };
        if out.iter().all(|o| (o.f - m.f).abs() >= min_separation_hz) {
            out.push(m);
        }
    }
    out
}

/// The nearest peak (at or above `threshold_db`) right (higher frequency) or left of `from_hz`, more than half a
/// display bin away from it (so the marker moves off its current peak).
pub fn next_peak(s: &Spectrum, from_hz: f64, right: bool, threshold_db: f32) -> Option<Marker> {
    let guard = s.df * 0.5;
    let mut p: Vec<Marker> = local_maxima(s, threshold_db).into_iter().filter_map(|i| interpolate_peak(s, i)).collect();
    if !right {
        p.reverse();
    }
    p.into_iter().find(|m| if right { m.f > from_hz + guard } else { m.f < from_hz - guard })
}

/// The highest peak of the trace.
pub fn max_peak(s: &Spectrum) -> Option<Marker> {
    let i = (0..s.dbv.len()).max_by(|&a, &b| s.dbv[a].total_cmp(&s.dbv[b]))?;
    interpolate_peak(s, i)
}

/// Delta marker: `m` relative to `reference` (Hz, dB).
pub fn delta(reference: Marker, m: Marker) -> Marker {
    Marker { f: m.f - reference.f, dbv: m.dbv - reference.dbv }
}

// ---------------------------------------------------------------------------------------------------------------
// channel power, occupied bandwidth

/// Power (V²) between `f_lo` and `f_hi`: sum of the display bins scaled by display bin width / RBW (exact with the
/// Avg detector, a good approximation with Sample).
pub fn channel_power(s: &Spectrum, f_lo: f64, f_hi: f64) -> f64 {
    if s.dbv.is_empty() || !(s.rbw > 0.0) {
        return 0.0;
    }
    let k = s.df / s.rbw;
    (0..s.dbv.len()).filter(|&i| (f_lo..=f_hi).contains(&s.freq(i))).map(|i| s.power(i) * k).sum()
}

/// Occupied bandwidth: the band (f_lo, f_hi) holding `pct` % of the trace power, with (100 - pct) / 2 % below and
/// above it (99 % OBW: pct = 99).
pub fn occupied_bw(s: &Spectrum, pct: f64) -> Option<(f64, f64)> {
    let p: Vec<f64> = (0..s.dbv.len()).map(|i| s.power(i)).collect();
    let total: f64 = p.iter().sum();
    if !(total > 0.0) {
        return None;
    }
    let tail = total * (1.0 - pct.clamp(0.0, 100.0) / 100.0) / 2.0;
    // fractional bin where the cumulative power reaches `target` (from the given side)
    let find = |target: f64, rev: bool| {
        let mut acc = 0.0;
        for j in 0..p.len() {
            let i = if rev { p.len() - 1 - j } else { j };
            if acc + p[i] >= target {
                let a = if p[i] > 0.0 { (target - acc) / p[i] } else { 0.0 };
                return if rev { i as f64 + 0.5 - a } else { i as f64 - 0.5 + a };
            }
            acc += p[i];
        }
        if rev { 0.0 } else { (p.len() - 1) as f64 }
    };
    let (a, b) = (find(tail, false), find(tail, true));
    Some((s.f0 + a.max(0.0) * s.df, s.f0 + b.min((p.len() - 1) as f64) * s.df))
}

// ---------------------------------------------------------------------------------------------------------------
// distortion

/// Distortion and noise of a sine record (IEEE 1241 definitions; all ratios relative to the fundamental).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Distortion {
    /// fundamental frequency, Hz
    pub f1: f64,
    /// fundamental RMS, dBV
    pub a1_dbv: f64,
    /// total harmonic distortion (harmonics 2..=n), dB (negative) and %
    pub thd_db: f64,
    pub thd_pct: f64,
    /// THD + noise, dB (negative)
    pub thdn_db: f64,
    /// signal to noise (harmonics excluded), dB
    pub snr_db: f64,
    /// signal to noise and distortion, dB
    pub sinad_db: f64,
    /// spurious-free dynamic range: fundamental over the largest other component, dBc
    pub sfdr_dbc: f64,
    /// effective number of bits (SINAD - 1.76) / 6.02
    pub enob: f64,
    /// (frequency after aliasing Hz, level dBc) of harmonics 2..=n
    pub harmonics: Vec<(f64, f64)>,
}

/// 7-term Blackman-Harris window (side lobes < -160 dB, main lobe ±8 bins), so that leakage of the fundamental does
/// not limit SNR measurements of 16-bit-class signals.
fn bh7(i: usize, m: usize) -> f64 {
    const A: [f64; 7] = [0.271_051_400_693_42, 0.433_297_939_234_48, 0.218_122_999_543_11, 0.065_925_446_388_03, 0.010_811_742_098_37, 0.000_776_584_825_22, 0.000_013_887_217_35];
    let x = 2.0 * PI * i as f64 / m as f64;
    A.iter().enumerate().map(|(k, a)| if k % 2 == 0 { a * (k as f64 * x).cos() } else { -a * (k as f64 * x).cos() }).sum()
}

/// THD, THD+N, SNR, SINAD, SFDR and ENOB of a sine record. The fundamental is the largest component (or the largest
/// near `fundamental` Hz); its frequency is the power centroid of its main lobe. Each component's power is the sum of
/// its main-lobe bins / ENBW (7-term Blackman-Harris window, mean removed). DC and the main lobes of the fundamental
/// and the harmonics are excluded from the noise; the noise of the excluded bins is replaced by the mean noise per bin.
/// Harmonics are searched at their aliased frequency. Returns `None` for records shorter than 64 samples or without a
/// fundamental.
pub fn distortion(v: &[f32], dt: f64, fundamental: Option<f64>, harmonics: usize) -> Option<Distortion> {
    let x: Vec<f64> = v.iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
    let m = x.len();
    if m < 64 || !(dt > 0.0) {
        return None;
    }
    let mean = x.iter().sum::<f64>() / m as f64;
    let n = m.next_power_of_two();
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    let (mut ws, mut ws2) = (0.0, 0.0);
    for i in 0..m {
        let w = bh7(i, m);
        re[i] = (x[i] - mean) * w;
        ws += w;
        ws2 += w * w;
    }
    fft(&mut re, &mut im);
    let nb = n / 2 + 1;
    let p: Vec<f64> = (0..nb).map(|k| (re[k] * re[k] + im[k] * im[k]) / (ws * ws) * if k == 0 { 1.0 } else { 2.0 }).collect();
    let e = n as f64 * ws2 / (ws * ws);
    let fs = 1.0 / dt;
    let df = fs / n as f64;
    let r = n as f64 / m as f64;
    let hw = (9.0 * r).ceil() as usize;
    if nb <= 2 * hw + 2 {
        return None;
    }
    let k1 = match fundamental {
        Some(f) if f > 0.0 && f < fs / 2.0 => {
            let c = (f / df).round() as usize;
            (c.saturating_sub(hw).max(hw + 1)..=(c + hw).min(nb - 1)).max_by(|&a, &b| p[a].total_cmp(&p[b]))?
        }
        _ => (hw + 1..nb).max_by(|&a, &b| p[a].total_cmp(&p[b]))?,
    };
    let lobe = |c: usize| c.saturating_sub(hw)..=(c + hw).min(nb - 1);
    let (sp, swp) = lobe(k1).fold((0.0, 0.0), |(a, b), k| (a + p[k], b + p[k] * k as f64));
    if !(sp > 0.0) {
        return None;
    }
    let kf = swp / sp;
    let f1 = kf * df;
    let p1 = sp / e;
    let mut used = vec![false; nb];
    used[..=hw].iter_mut().for_each(|u| *u = true);
    lobe(k1).for_each(|k| used[k] = true);
    let mut harm = Vec::new();
    let mut ph_sum = 0.0;
    let mut spur = 0.0f64;
    for h in 2..=harmonics.max(1) {
        let mut fa = (h as f64 * f1).rem_euclid(fs);
        if fa > fs / 2.0 {
            fa = fs - fa;
        }
        let c = (fa / df).round() as usize;
        let sr = (2.0 * r).ceil() as usize + 1;
        let Some(kh) = (c.saturating_sub(sr)..=(c + sr).min(nb - 1)).max_by(|&a, &b| p[a].total_cmp(&p[b])) else { continue };
        let mut ph = 0.0;
        for k in lobe(kh) {
            if !used[k] {
                ph += p[k];
                used[k] = true;
            }
        }
        let ph = ph / e;
        ph_sum += ph;
        spur = spur.max(ph);
        harm.push((fa, 10.0 * (ph.max(1e-30) / p1).log10()));
    }
    let (mut rest, mut cnt) = (0.0, 0usize);
    for k in 0..nb {
        if !used[k] {
            rest += p[k];
            cnt += 1;
            spur = spur.max(p[k]);
        }
    }
    let noise = if cnt > 0 { rest / e * nb as f64 / cnt as f64 } else { 0.0 };
    let db = |x: f64| 10.0 * x.max(1e-30).log10();
    let sinad = db(p1 / (noise + ph_sum));
    Some(Distortion {
        f1,
        a1_dbv: db(p1),
        thd_db: db(ph_sum / p1),
        thd_pct: 100.0 * (ph_sum / p1).sqrt(),
        thdn_db: db((ph_sum + noise) / p1),
        snr_db: db(p1 / noise),
        sinad_db: sinad,
        sfdr_dbc: db(p1 / spur),
        enob: (sinad - 1.76) / 6.02,
        harmonics: harm,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// waterfall

/// Spectrogram history: newest trace first.
#[derive(Clone, Debug, Default)]
pub struct Waterfall {
    pub rows: VecDeque<Vec<f32>>,
    pub max_rows: usize,
}

impl Waterfall {
    pub fn new(max_rows: usize) -> Waterfall {
        Waterfall { rows: VecDeque::with_capacity(max_rows), max_rows }
    }
    /// Adds a trace on top; the oldest row falls off when `max_rows` is reached. A trace of a different length clears
    /// the history (the display grid changed).
    pub fn push(&mut self, s: &Spectrum) {
        if self.rows.front().is_some_and(|r| r.len() != s.dbv.len()) {
            self.rows.clear();
        }
        self.rows.push_front(s.dbv.clone());
        self.rows.truncate(self.max_rows.max(1));
    }
    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Colour of a level for the waterfall: a viridis-like perceptual map from `reference - range` (dark violet) to
/// `reference` (yellow); NaN maps to the bottom colour.
pub fn color(db: f32, reference: f32, range: f32) -> (u8, u8, u8) {
    const MAP: [(f32, f32, f32); 9] = [(68.0, 1.0, 84.0), (71.0, 44.0, 122.0), (59.0, 81.0, 139.0), (44.0, 113.0, 142.0), (33.0, 144.0, 141.0), (39.0, 173.0, 129.0), (92.0, 200.0, 99.0), (170.0, 220.0, 50.0), (253.0, 231.0, 37.0)];
    let x = ((db - (reference - range)) / range.max(1e-6)).clamp(0.0, 1.0);
    let x = if x.is_finite() { x } else { 0.0 };
    let k = x * (MAP.len() - 1) as f32;
    let i = (k.floor() as usize).min(MAP.len() - 2);
    let a = k - i as f32;
    let (c0, c1) = (MAP[i], MAP[i + 1]);
    let mix = |p: f32, q: f32| (p + (q - p) * a).round() as u8;
    (mix(c0.0, c1.0), mix(c0.1, c1.1), mix(c0.2, c1.2))
}

/// A limit line: dBV over frequency, rising.
pub type Limit = Vec<(f64, f32)>;

/// A limit from a trace: the highest value within `widen` bins either side, plus `margin` dB, so a peak that drifts a
/// little or noise on it does not count as a violation.
pub fn limit_from(sp: &Spectrum, margin: f32, widen: usize) -> Limit {
    let n = sp.dbv.len();
    (0..n)
        .map(|i| {
            let top = sp.dbv[i.saturating_sub(widen)..(i + widen + 1).min(n)].iter().copied().fold(f32::MIN, f32::max);
            (sp.freq(i), top + margin)
        })
        .collect()
}

/// The limit at `f`, interpolated; None outside its range.
pub fn limit_at(l: &[(f64, f32)], f: f64) -> Option<f32> {
    let i = l.partition_point(|p| p.0 < f);
    match (i.checked_sub(1).and_then(|k| l.get(k)), l.get(i)) {
        (_, Some(b)) if b.0 == f => Some(b.1),
        (Some(a), Some(b)) => Some(a.1 + (b.1 - a.1) * ((f - a.0) / (b.0 - a.0)) as f32),
        _ => None,
    }
}

/// The worst bin above the limit as (frequency, dB over it), None when the trace stays below. Bins outside the limit's
/// range are not judged.
pub fn limit_check(sp: &Spectrum, l: &[(f64, f32)]) -> Option<(f64, f32)> {
    let mut worst: Option<(f64, f32)> = None;
    for (i, &db) in sp.dbv.iter().enumerate() {
        let f = sp.freq(i);
        if let Some(x) = limit_at(l, f) {
            if db > x && worst.is_none_or(|w| db - x > w.1) {
                worst = Some((f, db - x));
            }
        }
    }
    worst
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    #[test]
    fn a_limit_catches_what_rises_above_it() {
        let mut sp = Spectrum { f0: 0.0, df: 10.0, dbv: vec![-60.0; 100], rbw: 10.0, ..Default::default() };
        sp.dbv[50] = -20.0;
        let l = limit_from(&sp, 6.0, 2);
        assert_eq!((l.len(), l[48].1, l[47].1), (100, -14.0, -54.0));
        assert_eq!(limit_check(&sp, &l), None);
        assert_eq!(limit_at(&l, 5.0), Some(-54.0));
        assert_eq!(limit_at(&l, 1e6), None);
        sp.dbv[10] = -50.0;
        sp.dbv[80] = -40.0;
        let (f, over) = limit_check(&sp, &l).unwrap();
        assert_eq!((f, over), (800.0, 14.0));
    }
}
