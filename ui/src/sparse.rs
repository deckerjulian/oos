// SPDX-License-Identifier: GPL-2.0-or-later

//! Sparse frames on the fast display path (fork, docs/fork/perf-plan.md item 4): fewer than two samples per screen
//! column, the short time bases where the waveform rate peaks. `Ui::fast_draw` made a full screen record of each such
//! frame (`board::to_record`: a copy of the frame, every channel, the raw window and the LA record) only to draw its
//! points. `draw` computes the same points of the channels that are on, with the same expressions, into reused
//! buffers and draws them. It answers only in the plain case (edge trigger found on a channel, interpolated points);
//! everything else returns None and takes `to_record` as before.

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
            if sparse {
                lanczos8_many(v, xs, &mut out);
            } else {
                out.extend(xs.map(|x| oos_dsp::interp::linear(v, x).unwrap_or(f32::NAN)));
            }
            ph.draw_ex(c, m, (m.time_per_div(), m.delay), t_first, dt, &out, wgt, None);
        }
    });
    Some(true)
}

/// Phases and lobes of `oos_dsp::interp::lanczos8`.
const PHASES: usize = 1024;
const A: usize = 8;

/// The kernel table of `oos_dsp::interp::lanczos8`, computed the same way, and per phase the sum of its weights in the
/// order `lanczos8` adds them (the same bits).
fn table() -> &'static (Vec<f32>, Vec<f32>) {
    static T: std::sync::OnceLock<(Vec<f32>, Vec<f32>)> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let sinc = |x: f64| if x.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
        let mut t = Vec::with_capacity((PHASES + 1) * 2 * A);
        let mut ws = Vec::with_capacity(PHASES + 1);
        for p in 0..=PHASES {
            let fr = p as f64 / PHASES as f64;
            let mut wsum = 0.0f32;
            for j in 0..2 * A {
                let d = fr - (j as f64 - (A as f64 - 1.0));
                let w = (sinc(d) * sinc(d / A as f64)) as f32;
                t.push(w);
                wsum += w;
            }
            ws.push(wsum);
        }
        (t, ws)
    })
}

/// `oos_dsp::interp::lanczos8(v, x).unwrap_or(NAN)` for every x, appended to `out`, bit for bit. Eight points at a time
/// in the inner part: eight independent sums instead of one dependent chain, each summed in `lanczos8`'s order.
fn lanczos8_many(v: &[f32], xs: impl Iterator<Item = f64>, out: &mut Vec<f32>) {
    const L: usize = 8;
    let (t, ws) = table();
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
            let f0 = i0 - A as i64 + 1;
            if f0 < 0 || f0 + 2 * A as i64 > v.len() as i64 {
                inner = false;
                break;
            }
            first[q] = f0 as usize;
            ph[q] = (((xq - i0 as f64) * PHASES as f64).round() as usize).min(PHASES);
        }
        if !inner {
            out.extend(x[..cnt].iter().map(|&y| scalar(y)));
            continue;
        }
        let row = |q: usize| -> (&[f32; 2 * A], &[f32; 2 * A]) {
            let s = v[first[q]..first[q] + 2 * A].try_into().expect("16 samples");
            let w = t[ph[q] * 2 * A..(ph[q] + 1) * 2 * A].try_into().expect("16 weights");
            (s, w)
        };
        let rows: [(&[f32; 2 * A], &[f32; 2 * A]); L] = std::array::from_fn(row);
        let mut sum = [0.0f32; L];
        for j in 0..2 * A {
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
                ph.draw_ex(c, m, (m.time_per_div(), m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None);
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
                            let got = draw(&mut b, &m, &f, 24);
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
            assert_eq!(draw(&mut b, &m, &f, 24), Some(true), "{} s/div: the sparse path must answer", tdiv);
            let reps = 3000;
            let t0 = std::time::Instant::now();
            for _ in 0..reps {
                std::hint::black_box(reference(&mut a, &m, std::hint::black_box(&f)));
            }
            let old = t0.elapsed();
            let t0 = std::time::Instant::now();
            for _ in 0..reps {
                std::hint::black_box(draw(&mut b, &m, std::hint::black_box(&f), 24));
            }
            let new = t0.elapsed();
            let r = new.as_secs_f64() / old.as_secs_f64();
            println!("{} s/div, {} pt, {} ch: record path {:?}, sparse path {:?} per frame, ratio {:.2}", tdiv, len, chans, old / reps, new / reps, r);
            assert!(r <= 0.8, "sparse/record {:.2}", r);
        }
    }
}
