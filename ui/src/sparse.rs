// SPDX-License-Identifier: GPL-2.0-or-later

//! Sparse frames on the fast display path (fork, docs/fork/perf-plan.md item 4): fewer than two samples per screen
//! column, the short time bases where the waveform rate peaks. `Ui::fast_draw` made a full screen record of each such
//! frame (`board::to_record`: a copy of the frame, every channel, the raw window and the LA record) only to draw its
//! points. `draw` computes the same points of the channels that are on, with the same expressions, into reused
//! buffers and draws them. It answers only in the plain case (edge trigger found on a channel, interpolated points);
//! everything else returns None and takes `to_record` as before.
//!
//! `acq.interp = fast` (or OOS_INTERP=fast) draws the sinc curve from a grid of `GRID` points per sample where that is
//! cheaper (short time bases), within half a pixel of the exact points and one pixel on the screen (guarded); `exact`
//! stays the default until a screenshot comparison on the unit.

use crate::board::{sw_trig_level, Frame};
use crate::model::*;
use crate::wave::Phosphor;
use std::cell::RefCell;

/// Points of the screen record, as in `Source::acquire` and `Ui::fast_draw`.
pub const POINTS: usize = 3600;

thread_local! {
    static BUF: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

/// Draws a sparse frame into `ph` like `to_record` + `draw_ex` with weight `wgt`: Some(triggered), or None when the
/// frame needs `to_record`.
pub fn draw(ph: &mut Phosphor, m: &Model, f: &Frame, wgt: u16) -> Option<bool> {
    draw_with(ph, m, f, wgt, interp())
}

/// `draw` with interpolation `mode`.
pub fn draw_with(ph: &mut Phosphor, m: &Model, f: &Frame, wgt: u16, mode: Interp) -> Option<bool> {
    let tdiv = m.time_per_div() as f64;
    let window = tdiv * HDIV as f64;
    let t_first = m.delay as f64 - window / 2.0;
    let n = POINTS;
    let dt = window / n as f64;
    let ti = trigger(f, m, n, window, t_first)?;
    // the interpolated branch of `to_record` only: peak detect, high res and min/max pairs go there
    let per = dt / f.dt;
    if f.envelope || per >= 2.0 || m.acq == AcqMode::HiRes {
        return None;
    }
    let sparse = f.dt > 2.0 * dt;
    BUF.with(|b| {
        let mut out = b.borrow_mut();
        for c in 0..NCH {
            let v = &f.ch[c];
            if !m.ch[c].on || v.len() < 2 {
                continue;
            }
            out.clear();
            let xs = (0..n).map(|k| ti + (t_first + k as f64 * dt) / f.dt);
            if sparse && mode == Interp::Fast {
                fast_many(v, ti + t_first / f.dt, dt / f.dt, n, &mut out);
            } else if sparse {
                lanczos8_many(v, xs, &mut out);
            } else {
                out.extend(xs.map(|x| oos_dsp::interp::linear(v, x).unwrap_or(f32::NAN)));
            }
            ph.draw_ex(c, m, (m.time_per_div(), m.delay), t_first, dt, &out, wgt, None, false);
        }
    });
    Some(true)
}

/// Phases of the kernel tables (as `oos_dsp::interp::lanczos8`).
const PHASES: usize = 1024;

/// Kernel table of a Lanczos window with `a` lobes, computed as `oos_dsp::interp::lanczos8` computes its own (a = 8:
/// the same values), and per phase the sum of its weights in the order `lanczos8` adds them (the same bits).
fn make_table(a: usize) -> (Vec<f32>, Vec<f32>) {
    let sinc = |x: f64| if x.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
    let mut t = Vec::with_capacity((PHASES + 1) * 2 * a);
    let mut ws = Vec::with_capacity(PHASES + 1);
    for p in 0..=PHASES {
        let fr = p as f64 / PHASES as f64;
        let mut wsum = 0.0f32;
        for j in 0..2 * a {
            let d = fr - (j as f64 - (a as f64 - 1.0));
            let w = (sinc(d) * sinc(d / a as f64)) as f32;
            t.push(w);
            wsum += w;
        }
        ws.push(wsum);
    }
    (t, ws)
}

fn table8() -> &'static (Vec<f32>, Vec<f32>) {
    static T: std::sync::OnceLock<(Vec<f32>, Vec<f32>)> = std::sync::OnceLock::new();
    T.get_or_init(|| make_table(8))
}

/// `oos_dsp::interp::lanczos8(v, x).unwrap_or(NAN)` for every x, appended to `out`, bit for bit.
fn lanczos8_many(v: &[f32], xs: impl Iterator<Item = f64>, out: &mut Vec<f32>) {
    let (t, ws) = table8();
    lanczos_many::<16>(v, xs, out, t, ws);
}

/// Points of the fast interpolation's grid per sample: linear interpolation between grid points spaced 1/U sample
/// is within 3.8 V / U^2 of the sinc curve for a full-scale (+-3.8 div) sine at 0.45 of the sample rate.
const GRID: usize = 24;

/// Kernel rows of the grid phases u / GRID (u = 0..GRID), 16 taps each as `lanczos8`'s, and their sums.
fn grid_table() -> &'static (Vec<f32>, Vec<f32>) {
    static T: std::sync::OnceLock<(Vec<f32>, Vec<f32>)> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let sinc = |x: f64| if x.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
        let (mut t, mut ws) = (Vec::new(), Vec::new());
        for u in 0..GRID {
            let fr = u as f64 / GRID as f64;
            let mut wsum = 0.0f32;
            for j in 0..16 {
                let d = fr - (j as f64 - 7.0);
                let w = (sinc(d) * sinc(d / 8.0)) as f32;
                t.push(w);
                wsum += w;
            }
            ws.push(wsum);
        }
        (t, ws)
    })
}

thread_local! {
    static GRIDBUF: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static ROW: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

/// The `fast` interpolation (`acq.interp = fast`) at the evenly spaced points `x0 + k * step`, k < n: the sinc curve
/// on a grid of `GRID` points per sample (per phase a fixed 16-tap filter over consecutive samples, which vectorises),
/// then linear between grid points. Cheaper than `lanczos8` per point only when the points are dense (short time
/// bases); elsewhere, and near the record ends, it is `lanczos8_many`. Within a bound of the exact points, not equal
/// (the guard measures it).
fn fast_many(v: &[f32], x0: f64, step: f64, n: usize, out: &mut Vec<f32>) {
    let xs = (0..n).map(|k| x0 + k as f64 * step);
    let len = v.len() as i64;
    // samples whose grid the points need, inside the part where all 16 taps exist
    let lo = (x0.floor() as i64).max(7);
    let hi = ((x0 + (n as f64 - 1.0) * step).floor() as i64 + 1).min(len - 9);
    if hi <= lo || (hi - lo) as usize * GRID > 4 * n {
        return lanczos8_many(v, xs, out);
    }
    let (lo, hi) = (lo as usize, hi as usize);
    let (t, ws) = grid_table();
    GRIDBUF.with(|g| {
        let mut g = g.borrow_mut();
        let cells = hi - lo + 1;
        g.clear();
        g.resize(cells * GRID, 0.0);
        ROW.with(|r| {
            let mut acc = r.borrow_mut();
            for u in 0..GRID {
                let w = &t[u * 16..(u + 1) * 16];
                // one tap at a time over all samples: the sums run side by side (vectorised), each in tap order
                acc.clear();
                acc.resize(cells, 0.0);
                for (j, &wj) in w.iter().enumerate() {
                    let src = &v[lo - 7 + j..lo - 7 + j + cells];
                    for (a, &x) in acc.iter_mut().zip(src) {
                        *a += x * wj;
                    }
                }
                for (c, &a) in acc.iter().enumerate() {
                    g[c * GRID + u] = a / ws[u];
                }
            }
        });
        for x in xs {
            let pos = (x - lo as f64) * GRID as f64;
            let k = pos.floor();
            let ki = k as i64;
            if x < lo as f64 || ki + 1 >= (cells * GRID) as i64 || !pos.is_finite() {
                out.push(oos_dsp::interp::lanczos8(v, x).unwrap_or(f32::NAN));
                continue;
            }
            let (a, b) = (g[ki as usize], g[ki as usize + 1]);
            let fr = (pos - k) as f32;
            out.push(if a.is_finite() && b.is_finite() { a + (b - a) * fr } else { oos_dsp::interp::lanczos8(v, x).unwrap_or(f32::NAN) });
        }
    });
}

/// Windowed sinc of `TAPS` taps from table `t` (weight sums `ws`) at every x, eight points at a time in the inner part:
/// eight independent sums instead of one dependent chain, each summed in `lanczos8`'s order. Points near the record
/// ends or next to a gap: `lanczos8` itself.
fn lanczos_many<const TAPS: usize>(v: &[f32], xs: impl Iterator<Item = f64>, out: &mut Vec<f32>, t: &[f32], ws: &[f32]) {
    const L: usize = 8;
    let a = TAPS / 2;
    let scalar = |x: f64| oos_dsp::interp::lanczos8(v, x).unwrap_or(f32::NAN);
    let mut xs = xs.peekable();
    while xs.peek().is_some() {
        let mut x = [0.0f64; L];
        let mut cnt = 0;
        while cnt < L {
            match xs.next() {
                Some(y) => x[cnt] = y,
                None => break,
            }
            cnt += 1;
        }
        // first sample and phase of each point; any point outside the inner part: these points one by one
        let mut first = [0usize; L];
        let mut ph = [0usize; L];
        let mut inner = cnt == L;
        for q in 0..cnt {
            let xq = x[q];
            if !(xq >= 0.0 && xq <= v.len() as f64 - 1.0 && v.len() >= 2) {
                inner = false;
                break;
            }
            let i0 = xq.floor() as i64;
            // the inner part of `lanczos8` (8 lobes), whatever the taps: the same points fall back
            let f8 = i0 - 7;
            if f8 < 0 || f8 + 16 > v.len() as i64 {
                inner = false;
                break;
            }
            first[q] = (i0 - a as i64 + 1) as usize;
            ph[q] = (((xq - i0 as f64) * PHASES as f64).round() as usize).min(PHASES);
        }
        if !inner {
            out.extend(x[..cnt].iter().map(|&y| scalar(y)));
            continue;
        }
        let row = |q: usize| -> (&[f32; TAPS], &[f32; TAPS]) {
            let s = v[first[q]..first[q] + TAPS].try_into().expect("TAPS samples");
            let w = t[ph[q] * TAPS..(ph[q] + 1) * TAPS].try_into().expect("TAPS weights");
            (s, w)
        };
        let rows: [(&[f32; TAPS], &[f32; TAPS]); L] = std::array::from_fn(row);
        let mut sum = [0.0f32; L];
        for j in 0..TAPS {
            for q in 0..L {
                sum[q] += rows[q].0[j] * rows[q].1[j];
            }
        }
        for q in 0..L {
            out.push(if !sum[q].is_finite() {
                scalar(x[q])
            } else if ws[ph[q]].abs() > 1e-6 {
                sum[q] / ws[ph[q]]
            } else {
                f32::NAN
            });
        }
    }
}

/// Interpolation of sparse frames: `exact` (`lanczos8`, the record path's) or `fast` (`fast_many`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Exact,
    Fast,
}

/// OOS_INTERP, else the `acq.interp` line of system.conf, else `exact`.
pub fn interp_from(env: Option<&str>, conf: &str) -> Interp {
    match crate::drawpipe::conf_value(env, conf, "acq.interp").as_deref() {
        Some("fast") => Interp::Fast,
        _ => Interp::Exact,
    }
}

/// The interpolation in force (read once).
pub fn interp() -> Interp {
    static I: std::sync::OnceLock<Interp> = std::sync::OnceLock::new();
    *I.get_or_init(|| {
        let env = std::env::var("OOS_INTERP").ok();
        interp_from(env.as_deref(), &std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default())
    })
}

/// The trigger point `to_record` takes for an edge trigger on a channel, or None where it would look further (no edge
/// near the hardware point) or trigger otherwise.
fn trigger(f: &Frame, m: &Model, n: usize, window: f64, t_first: f64) -> Option<f64> {
    if m.trig_type != TrigType::Edge || m.trig_line().is_some() || f.roll || f.scan || f.ch[m.trig_src].len() <= 1 {
        return None;
    }
    let len = f.ch.iter().map(|v| v.len()).max().unwrap_or(0);
    let screen = (window / f.dt).ceil() as usize;
    let hyst = m.trig_hyst_div() * m.ch[m.trig_src].volts_per_div();
    let col = (window / n as f64 / f.dt).floor().clamp(1.0, 256.0) as usize;
    let (rising, either) = (m.slope != Slope::Fall, m.slope == Slope::Either);
    let (first, last) = (-t_first / f.dt, len as f64 - 1.0 - (t_first + window) / f.dt);
    let fits = |t: f64| first > last || (first..=last).contains(&t);
    let found = crate::trignear::edge_smooth_near(&f.ch[m.trig_src], f.pre, sw_trig_level(m, f), hyst, rising, either, screen / 2, col, fits).0;
    found.or((m.trig_coupling() != TrigCoup::Dc).then_some(f.pre as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The path this replaces: `Ui::fast_draw` for sparse frames before item 4.
    fn reference(ph: &mut Phosphor, m: &Model, f: &Frame) -> bool {
        let fr = Arc::new(f.clone());
        let window = m.time_per_div() as f64 * HDIV as f64;
        let rec = crate::board::to_record(&fr, m, POINTS, window, m.delay as f64 - window / 2.0);
        for c in 0..NCH {
            if m.ch[c].on && !rec.ch[c].is_empty() {
                ph.draw_ex(c, m, (m.time_per_div(), m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None, rec.pairs);
            }
        }
        rec.triggered
    }

    /// A frame of `len` points at 1.25 GSa/s / `chans`, hardware point in the middle: a square wave with noise on each
    /// channel on, of a different frequency per channel.
    fn frame(len: usize, chans: usize, f_sig: f64, seed: u64) -> Frame {
        let dt = chans as f64 / 1.25e9;
        let mut ch: [Vec<f32>; NCH] = Default::default();
        for (c, v) in ch.iter_mut().enumerate().take(chans) {
            let p = oos_dsp::sim::PulseTrain::square(f_sig * (1.0 + c as f64 * 0.37), -0.8, 0.9, 0.6e-9);
            *v = oos_dsp::sim::sample(|t| p.v(t), -(len as f64 / 2.0) * dt + 0.31e-9, dt, len);
            oos_dsp::sim::add_noise(v, 0.03, seed + c as u64);
        }
        Frame { seq: 1, dt, pre: len / 2, ch, ..Default::default() }
    }

    fn model(tdiv: f64, chans: usize) -> Model {
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
        for c in 0..NCH {
            m.ch[c].on = c < chans;
        }
        m
    }

    // Break it catches: a sparse fast path whose pixels differ from the record path in any setting it answers
    // (time base, channels, slope, delay, level, dots, trigger coupling), or one that answers where `to_record` takes
    // another branch.
    #[test]
    fn sparse_path_draws_the_same_pixels_as_the_record_path() {
        let (w, h) = (970, 448);
        let (mut answered, mut cases) = (0, 0);
        for tdiv in [5e-9, 1e-8, 2e-8, 5e-8, 1e-7, 2e-7] {
            for chans in [1, 2, 4] {
                for (len, f_sig) in [(80usize, 3e7), (1408, 1e7), (5000, 2.2e7)] {
                    for (k, slope) in [Slope::Rise, Slope::Fall, Slope::Either].into_iter().enumerate() {
                        for (delay, level, dots, coup) in [(0.0f32, 0.0f32, false, TrigCoup::Dc), (3e-9, 0.2, true, TrigCoup::Dc), (-2e-8, 2.0, false, TrigCoup::Dc), (0.0, 2.0, false, TrigCoup::Ac)] {
                            let mut m = model(tdiv, chans);
                            (m.slope, m.delay, m.trig_level, m.dots) = (slope, delay, level, dots);
                            m.trig_coup = coup;
                            let f = frame(len, chans, f_sig, k as u64 * 7 + len as u64);
                            let x_per_sample = f.dt / tdiv * w as f64 / HDIV as f64;
                            if x_per_sample <= 0.5 {
                                continue;
                            }
                            let (mut a, mut b) = (Phosphor::new(w, h), Phosphor::new(w, h));
                            let want = reference(&mut a, &m, &f);
                            let got = draw_with(&mut b, &m, &f, 24, Interp::Exact);
                            cases += 1;
                            if let Some(t) = got {
                                answered += 1;
                                assert_eq!(t, want, "{} s/div {} ch {} pt {:?}", tdiv, chans, len, slope);
                                for c in 0..NCH {
                                    assert!(a.map[c] == b.map[c], "{} s/div {} ch {} pt {:?} delay {} level {} dots {}: CH{} pixels differ", tdiv, chans, len, slope, delay, level, dots, c + 1);
                                }
                            }
                        }
                    }
                }
            }
        }
        println!("{} of {} answered", answered, cases);
        assert!(cases > 200 && answered * 2 > cases, "{} of {} answered", answered, cases);
    }

    // Break it catches: a batched kernel whose points differ from `lanczos8` in any bit (table, phase rounding, order
    // of the sums, the edges of the record, NaN samples).
    #[test]
    fn batched_lanczos_is_lanczos8_bit_for_bit() {
        let mut v: Vec<f32> = (0..300).map(|k| (k as f32 * 0.71).sin() * 0.8 + (k as f32 * 0.13).cos() * 0.3).collect();
        oos_dsp::sim::add_noise(&mut v, 0.05, 9);
        let mut holed = v.clone();
        holed[150] = f32::NAN;
        holed[151] = f32::INFINITY;
        for v in [v, holed, vec![0.5f32; 3], vec![1.0f32]] {
            for step in [0.013f64, 0.2777, 0.5, 1.0 / 3.0] {
                let xs: Vec<f64> = (0..2000).map(|k| -3.3 + k as f64 * step).collect();
                let mut got = Vec::new();
                lanczos8_many(&v, xs.iter().copied(), &mut got);
                assert_eq!(got.len(), xs.len());
                for (&x, g) in xs.iter().zip(&got) {
                    let want = oos_dsp::interp::lanczos8(&v, x).unwrap_or(f32::NAN);
                    assert!(g.to_bits() == want.to_bits() || g.is_nan() && want.is_nan(), "x {}: {} vs {}", x, g, want);
                }
            }
        }
    }

    /// Largest distance between the lit pixels of two maps: each lit pixel of one to the nearest lit pixel of the
    /// other (Chebyshev, both ways), at most `cap`. A vertical edge moved by a fraction of a column lights a whole
    /// column in one map and the next one in the other: one pixel, not the height of the edge.
    fn pixel_distance(a: &[u16], b: &[u16], w: usize, h: usize) -> usize {
        let cap = 64;
        let far = |p: &[u16], q: &[u16]| {
            let mut worst = 0;
            for y in 0..h {
                for x in 0..w {
                    if p[y * w + x] == 0 {
                        continue;
                    }
                    let hit = |r: usize| {
                        let (y0, y1, x0, x1) = (y.saturating_sub(r), (y + r).min(h - 1), x.saturating_sub(r), (x + r).min(w - 1));
                        (y0..=y1).any(|yy| (x0..=x1).any(|xx| q[yy * w + xx] > 0))
                    };
                    // the screen edge counts as lit: a trace that leaves the screen in one map only
                    let edge = (y + 1).min(h - y);
                    let r = (worst..=cap).find(|&r| hit(r) || r >= edge).unwrap_or(cap);
                    worst = worst.max(r);
                }
            }
            worst
        };
        far(a, b).max(far(b, a))
    }

    // Break it catches: a fast interpolation more than a pixel away from the exact one anywhere on the screen (full
    // scale signals up to 0.45 of the sample rate, edges, noise; every sparse time base; record ends).
    #[test]
    fn fast_interpolation_stays_within_a_pixel() {
        let (w, h) = (970, 448);
        let (mut worst_v, mut worst_px, mut cases) = (0.0f32, 0usize, 0);
        let mut at = String::new();
        for tdiv in [5e-9, 1e-8, 2e-8, 5e-8, 1e-7, 2e-7] {
            for chans in [1usize, 4] {
                let dt = chans as f64 / 1.25e9;
                let len = ((tdiv * HDIV as f64 / dt) * 1.15) as usize + 40;
                let sigs: Vec<(String, Box<dyn Fn(f64) -> f64>)> = vec![
                    ("sine 0.05".into(), Box::new(move |t: f64| 3.8 * (t / dt * 0.05 * std::f64::consts::TAU).sin())),
                    ("sine 0.2".into(), Box::new(move |t: f64| 3.8 * (t / dt * 0.2 * std::f64::consts::TAU + 0.3).sin())),
                    ("sine 0.4".into(), Box::new(move |t: f64| 3.8 * (t / dt * 0.4 * std::f64::consts::TAU + 0.7).sin())),
                    ("sine 0.45".into(), Box::new(move |t: f64| 3.8 * (t / dt * 0.45 * std::f64::consts::TAU + 1.1).sin())),
                    ("square".into(), Box::new(move |t: f64| if (t / dt / 37.0).rem_euclid(1.0) < 0.5 { -3.5 } else { 3.5 })),
                ];
                for (name, sig) in &sigs {
                    let mut m = Model::default();
                    m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
                    m.trig_level = 0.0;
                    m.ch[0].offset = 0.0;
                    let mut v = oos_dsp::sim::sample(|t| sig(t), -(len as f64 / 2.0) * dt + 0.37 * dt, dt, len);
                    if name == "square" {
                        oos_dsp::sim::add_noise(&mut v, 0.05, 3);
                    }
                    let f = Frame { seq: 1, dt, pre: len / 2, ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()], ..Default::default() };
                    let x_per_sample = dt / tdiv * w as f64 / HDIV as f64;
                    if x_per_sample <= 0.5 {
                        continue;
                    }
                    let (mut a, mut b) = (Phosphor::new(w, h), Phosphor::new(w, h));
                    let ta = draw_with(&mut a, &m, &f, 24, Interp::Exact);
                    let tb = draw_with(&mut b, &m, &f, 24, Interp::Fast);
                    assert_eq!(ta, tb);
                    if ta.is_none() {
                        continue;
                    }
                    cases += 1;
                    // the points themselves, in volts, over the whole frame
                    let step = (len - 1) as f64 / 3999.0 / 8.0;
                    let x0 = len as f64 / 2.0 - 2000.0 * step;
                    let (mut pe, mut pf) = (Vec::new(), Vec::new());
                    lanczos8_many(&v, (0..4000).map(|k| x0 + k as f64 * step), &mut pe);
                    fast_many(&v, x0, step, 4000, &mut pf);
                    let dv = pe.iter().zip(&pf).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
                    let dp = pixel_distance(&a.map[0], &b.map[0], w, h);
                    if dp > worst_px || dv > worst_v {
                        at = format!("{} s/div {} ch {}", tdiv, chans, name);
                    }
                    (worst_v, worst_px) = (worst_v.max(dv), worst_px.max(dp));
                }
            }
        }
        // 56 pixels a volt at 1 V/div
        println!("fast vs exact over {} cases: max {:.4} V ({:.2} px) in the points, {} px on the screen (worst at {})", cases, worst_v, worst_v * h as f32 / VDIV, worst_px, at);
        assert!(cases > 30 && worst_px <= 1, "{} px at {}", worst_px, at);
    }

    /// Host timing (run with --ignored --nocapture): the sparse path against `to_record` + `draw_ex`.
    #[test]
    #[ignore]
    fn sparse_path_is_cheaper_than_the_record_path() {
        let (w, h) = (970, 448);
        for (tdiv, len, chans) in [(5e-9, 80usize, 1usize), (1e-7, 1408, 1), (2e-7, 5000, 4)] {
            let mut m = model(tdiv, chans);
            m.trig_level = 0.0;
            let f = frame(len, chans, 1e7, 1);
            let (mut a, mut b) = (Phosphor::new(w, h), Phosphor::new(w, h));
            assert_eq!(draw_with(&mut b, &m, &f, 24, Interp::Exact), Some(true), "{} s/div: the sparse path must answer", tdiv);
            let reps = 3000;
            let t0 = std::time::Instant::now();
            for _ in 0..reps {
                std::hint::black_box(reference(&mut a, &m, std::hint::black_box(&f)));
            }
            let old = t0.elapsed();
            let t0 = std::time::Instant::now();
            for _ in 0..reps {
                std::hint::black_box(draw_with(&mut b, &m, std::hint::black_box(&f), 24, Interp::Exact));
            }
            let new = t0.elapsed();
            let r = new.as_secs_f64() / old.as_secs_f64();
            println!("{} s/div, {} pt, {} ch: record path {:?}, sparse path {:?} per frame, ratio {:.2}", tdiv, len, chans, old / reps, new / reps, r);
            assert!(r <= 0.8, "sparse/record {:.2}", r);
        }
    }

    /// Host cost of the fast interpolation against the exact one per sparse frame (run by tools/perf-guard.sh).
    #[test]
    #[ignore]
    fn perf_guard_fast_interpolation() {
        let (w, h) = (970, 448);
        for (tdiv, chans) in [(5e-9, 1usize), (2e-8, 1), (5e-8, 1), (1e-7, 1), (2e-8, 4), (1e-7, 4), (2e-7, 4)] {
            let dt = chans as f64 / 1.25e9;
            let len = ((tdiv * HDIV as f64 / dt) * 1.15) as usize + 40;
            let mut m = model(tdiv, 1);
            m.trig_level = 0.0;
            let p = oos_dsp::sim::PulseTrain::square(1.0 / (60.0 * dt), -1.0, 1.0, 2.0 * dt);
            let v = oos_dsp::sim::sample(|t| p.v(t), -(len as f64 / 2.0) * dt, dt, len);
            let f = Frame { seq: 1, dt, pre: len / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], ..Default::default() };
            let mut ph = Phosphor::new(w, h);
            assert_eq!(draw_with(&mut ph, &m, &f, 24, Interp::Fast), Some(true), "{} s/div", tdiv);
            let mut time = |mode: Interp| {
                let reps = 3000;
                let t0 = std::time::Instant::now();
                for _ in 0..reps {
                    std::hint::black_box(draw_with(&mut ph, &m, std::hint::black_box(&f), 24, mode));
                }
                t0.elapsed() / reps
            };
            let (e, fa) = (time(Interp::Exact), time(Interp::Fast));
            let r = fa.as_secs_f64() / e.as_secs_f64();
            println!("{} s/div {} ch ({} samples on screen): exact {:?}, fast {:?} per frame, ratio {:.2}", tdiv, chans, (tdiv * HDIV as f64 / dt) as usize, e, fa, r);
            assert!(r <= 1.1, "fast must never cost more: {:.2}", r);
        }
    }
}
