// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! A frame of the hardware made into a screen record: software trigger near the hardware trigger point, then the
//! screen window. Board-neutral: it works on frames in volts.

use super::Frame;
use crate::model::*;
use std::sync::Arc;

/// Turns the latest frame into a screen record: software trigger (oos-dsp) closest to the hardware trigger point,
/// then the screen window (`n` points over `window` s from `t_first` relative to the trigger) by linear interpolation.
pub fn to_record(f: &Arc<Frame>, m: &Model, n: usize, window: f64, t_first: f64) -> crate::wave::Record {
    // the software trigger looks only around the hardware trigger point: searching whole 100 k-point frames for
    // every record cost most of the frame time; the video trigger counts fields and keeps the whole frame
    let len = f.ch.iter().map(|v| v.len()).max().unwrap_or(0);
    let screen = (window / f.dt).ceil() as usize;
    // the hardware trigger already put the event near `pre`: look a tenth of a screen around it, plus the time the
    // trigger condition needs before its decision (pulse widths, timeouts, Nth-edge idle time)
    let near = (screen / 10).max(2000);
    let span = (m.trig_span() / f.dt).ceil().max(0.0) as usize;
    let (lo, hi) = if m.trig_type == TrigType::Video {
        (0, len)
    } else {
        (f.pre.saturating_sub(near + span), (f.pre + near).min(len))
    };
    let e: [f32; 0] = [];
    let refs: [&[f32]; NCH] = std::array::from_fn(|c| if f.ch[c].len() < hi || lo >= hi { &e[..] } else { &f.ch[c][lo..hi] });
    // the hysteresis of the hardware comparators (0.3 div, trigger.md 2.3): with less, noise on an edge of the other
    // direction counts as an edge, and a slow noisy sine is drawn now on one, now on the other edge
    let hyst: [f32; NCH] = std::array::from_fn(|c| m.trig_hyst_div() * m.ch[c].volts_per_div());
    let inp = oos_dsp::trigger::Input { ch: refs, dt: f.dt, hyst };
    let usable = m.trig_channels().iter().enumerate().all(|(c, &u)| !u || !refs[c].is_empty());
    // plain edge trigger on a channel: the edge of the set direction nearest the hardware point, on the signal smoothed to
    // one screen point (fastdraw::edge_smooth; the hardware point is not always on the right edge, trigger.md 8)
    let smooth = (m.trig_type == TrigType::Edge && m.trig_line().is_none() && !f.roll && !f.scan && f.ch[m.trig_src].len() > 1)
        .then(|| {
            let col = (window / n as f64 / f.dt).floor().clamp(1.0, 256.0) as usize;
            let (rising, either) = (m.slope != Slope::Fall, m.slope == Slope::Either);
            // the screen window stays inside the frame where it can
            let (first, last) = (-t_first / f.dt, len as f64 - 1.0 - (t_first + window) / f.dt);
            let fits = |t: f64| first > last || (first..=last).contains(&t);
            crate::fastdraw::edge_smooth(&f.ch[m.trig_src], f.pre, m.trig_level, hyst[m.trig_src], rising, either, screen / 2, col, fits).0
        })
        .flatten();
    let found = if f.roll || f.scan {
        None
    } else if smooth.is_some() {
        smooth
    } else if let Some(n) = m.trig_line() {
        // the hardware trigger on an LA line: the analog samples cannot refine it. Without any edge on the line the
        // frame came from the auto trigger.
        f.la.as_ref().filter(|l| crate::ui::la::line_moves(l, n)).map(|_| f.pre as f64)
    } else if m.bus_on_la() {
        // the matching field nearest the hardware trigger: the record moves to it, as with the channels
        f.la.as_ref().and_then(|l| {
            let p = crate::trig::decode_la(m, &l.words, l.dt).ok()?;
            let t = p.iter().filter(|x| crate::trig::matches(m, x)).map(|x| l.t0 + x.t1 * l.dt).min_by(|a, b| a.abs().total_cmp(&b.abs()))?;
            Some(f.pre as f64 + t / f.dt)
        })
    } else if m.pat_on_la() {
        // a pattern over D lines: as above; the frame counts as triggered when the pattern occurs in it
        f.la.as_ref().filter(|l| crate::ui::la::pattern_at(&l.words, &m.pat_la, 0).is_some()).map(|_| f.pre as f64)
    } else if usable {
        // nearest the hardware trigger among the events that leave the screen window inside the frame: the frame holds
        // little more than the window, and a noisy slow edge can cross the level far from `pre`
        let (first, last) = (-t_first / f.dt, len as f64 - 1.0 - (t_first + window) / f.dt);
        let fits = |t: &f64| first > last || (first..=last).contains(t);
        let nearest = |a: &f64, b: &f64| (a - f.pre as f64).abs().total_cmp(&(b - f.pre as f64).abs());
        let all: Vec<f64> = crate::trig::find_all(m, &inp).into_iter().map(|t| t + lo as f64).collect();
        all.iter().copied().filter(fits).min_by(nearest).or_else(|| {
            // nothing of the set kind near `pre`: the hardware can fire on the other edge of a clean signal, so look
            // up to half a screen around it (edge trigger only, the others have their own time conditions)
            if m.trig_type != TrigType::Edge || near >= screen / 2 {
                return all.first().map(|_| f.pre as f64);
            }
            let (lo2, hi2) = (f.pre.saturating_sub(screen / 2), (f.pre + screen / 2).min(len));
            let refs2: [&[f32]; NCH] = std::array::from_fn(|c| if f.ch[c].len() < hi2 || lo2 >= hi2 { &e[..] } else { &f.ch[c][lo2..hi2] });
            let inp2 = oos_dsp::trigger::Input { ch: refs2, dt: f.dt, hyst };
            let wide: Vec<f64> = crate::trig::find_all(m, &inp2).into_iter().map(|t| t + lo2 as f64).collect();
            wide.iter().copied().filter(fits).min_by(nearest).or_else(|| (!all.is_empty() || !wide.is_empty()).then_some(f.pre as f64))
        })
    } else {
        None
    };
    // roll: right-aligned like a chart recorder, the start of the stream may be left of the buffer (blank)
    let ti = if f.roll {
        let len = f.ch.iter().map(|v| v.len()).max().unwrap_or(0) as f64;
        len - 1.0 - (window / 2.0) / f.dt
    } else if f.scan {
        // scan: point 0 at the left edge of the screen
        -t_first / f.dt
    } else {
        found.unwrap_or(f.pre as f64)
    };
    let dt = window / n as f64;
    let mut ch: [Vec<f32>; NCH] = Default::default();
    for c in 0..NCH {
        let v = &f.ch[c];
        if v.len() < 2 {
            continue;
        }
        let per = dt / f.dt; // samples per output point
        let sparse = !f.envelope && f.dt > 2.0 * dt;
        if per >= 2.0 && m.acq == AcqMode::HiRes {
            // high res: the mean of the samples behind each point. A software mode: the hardware has no such filter,
            // so it only gains where the record holds more samples than the screen has points.
            ch[c] = (0..n)
                .map(|k| {
                    let x = ti + (t_first + (k as f64 + 0.5) * dt) / f.dt;
                    let a = (x - per / 2.0).max(0.0) as usize;
                    let b = ((x + per / 2.0).ceil().max(0.0) as usize).min(v.len());
                    let s = v[a.min(b)..b].iter().filter(|y| y.is_finite());
                    let (sum, cnt) = s.fold((0.0f64, 0u32), |(t, n), &y| (t + y as f64, n + 1));
                    if cnt > 0 { (sum / cnt as f64) as f32 } else { f32::NAN }
                })
                .collect();
            continue;
        }
        if per >= 2.0 {
            // dense views (several samples per screen point): peak detect. Points 2j and 2j+1 share one span (two
            // output points wide); 2j gets its minimum, 2j+1 its maximum, so noise and one-sample spikes stay
            // visible like on the stock display. One pass per pair.
            let mut out = vec![f32::NAN; n];
            for j in 0..n.div_ceil(2) {
                let xm = ti + (t_first + (2 * j) as f64 * dt + 0.5 * dt) / f.dt;
                if xm < -per || xm >= v.len() as f64 + per {
                    continue;
                }
                let a = (xm - per).max(0.0) as usize;
                let b = ((xm + per).ceil() as usize).min(v.len());
                let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                for &y in v[a.min(b)..b].iter().filter(|y| y.is_finite()) {
                    lo = lo.min(y);
                    hi = hi.max(y);
                }
                if lo <= hi {
                    out[2 * j] = lo;
                    if 2 * j + 1 < n {
                        out[2 * j + 1] = hi;
                    }
                }
            }
            ch[c] = out;
            continue;
        }
        // sparse views (less than one sample per two screen points, fast time bases): windowed sinc like the stock
        // "Auto" interpolation; otherwise linear
        ch[c] = (0..n)
            .map(|k| {
                let x = ti + (t_first + k as f64 * dt) / f.dt;
                let y = if sparse { oos_dsp::interp::lanczos8(v, x) } else { oos_dsp::interp::linear(v, x) };
                y.unwrap_or(f32::NAN)
            })
            .collect();
    }
    // the frame's samples inside the screen window
    let i0 = (ti + t_first / f.dt).floor().max(0.0) as usize;
    let i1 = ((ti + (t_first + window) / f.dt).ceil().max(0.0) as usize).min(len);
    let raw = (i1 > i0 + 1).then(|| crate::wave::Raw { frame: f.clone(), i0, i1, t0: (i0 as f64 - ti) * f.dt });
    // LA: the same shift as the analog record (software trigger at `ti` instead of the hardware point `pre`)
    let la = f.la.as_ref().map(|l| crate::ui::la::LaRec { t0: l.t0 - (ti - f.pre as f64) * f.dt, ..l.clone() });
    crate::wave::Record { t0: t_first, dt, ch, triggered: found.is_some(), partial: f.partial, raw, la }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_res_averages_where_the_record_has_more_samples_than_the_screen() {
        // 100 k samples of noise around 1 V, 3600 points: about 28 samples behind each point
        let mut x = 12345u32;
        let v: Vec<f32> = (0..100_000)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                1.0 + ((x >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.4
            })
            .collect();
        let window = 100_000e-9;
        let frame = Arc::new(Frame { seq: 1, dt: 1e-9, pre: 50_000, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (0.0, 0), stamp: 0, la: None, hw_avg: false });
        let rms = |v: &[f32]| (v.iter().map(|y| (y - 1.0) * (y - 1.0)).sum::<f32>() / v.len() as f32).sqrt();
        let mut m = Model::default();
        m.trig_level = 5.0; // nothing to trigger on: the window stays at the hardware point
        let normal = to_record(&frame, &m, 3600, window, -window / 2.0);
        m.acq = AcqMode::HiRes;
        let hires = to_record(&frame, &m, 3600, window, -window / 2.0);
        assert!(hires.ch[0].iter().all(|y| y.is_finite()));
        // uniform noise of 0.115 V rms over 28 samples gives 0.022 V
        assert!(rms(&hires.ch[0]) < 0.03 && rms(&hires.ch[0]) < 0.25 * rms(&normal.ch[0]), "{} {}", rms(&hires.ch[0]), rms(&normal.ch[0]));
        let mean = hires.ch[0].iter().sum::<f32>() / 3600.0;
        assert!((mean - 1.0).abs() < 0.01, "{}", mean);
    }

    #[test]
    fn the_trigger_keeps_the_screen_window_inside_the_frame() {
        // rising edges at 4600 and 5600; 5600 is nearer the hardware point, but the window would run off the frame
        let v: Vec<f32> = (0..10_000).map(|i| if (4600..5000).contains(&i) || i >= 5600 { 2.0 } else { -2.0 }).collect();
        let mut m = Model::default();
        m.trig_level = 0.0;
        let window = 9000e-9;
        let f = Frame { seq: 1, dt: 1e-9, pre: 5400, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (0.0, 0), stamp: 0, la: None, hw_avg: false };
        let r = to_record(&Arc::new(f), &m, 3600, window, -window / 2.0);
        assert!(r.triggered);
        assert!(r.ch[0].iter().all(|y| y.is_finite()), "{} gaps", r.ch[0].iter().filter(|y| !y.is_finite()).count());
    }
}

#[cfg(test)]
mod peak_detect {
    use super::*;
    #[test]
    fn peak_detect_keeps_a_spike_and_is_cheap() {
        // the hardware trigger point is the falling edge at 50 k
        let m = Model { slope: Slope::Fall, ..Model::default() };
        let mut v: Vec<f32> = (0..100_000).map(|k| if (k / 25_000) % 2 == 0 { 0.0 } else { 3.0 }).collect();
        v[12_345] = 2.0; // one-sample glitch on the low level
        let f = Arc::new(Frame { seq: 1, dt: 20e-9, pre: 50_000, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (0.0, 0), stamp: 0, la: None, hw_avg: false });
        let window = m.time_per_div() as f64 * 10.0; // 2 ms > record: whole record on screen
        let t = std::time::Instant::now();
        let mut r = to_record(&f, &m, 3600, window, -window / 2.0);
        for _ in 0..47 {
            r = to_record(&f, &m, 3600, window, -window / 2.0);
        }
        eprintln!("PERF 48 x to_record (100 k samples): {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        // measurements see every captured sample of the window, not the 3600 screen points
        let raw = r.raw.as_ref().unwrap();
        assert_eq!((raw.i0, raw.i1), (0, 100_000));
        let mx = r.ch[0].iter().copied().filter(|x| x.is_finite() && *x < 2.5).fold(0.0f32, f32::max);
        assert!((mx - 2.0).abs() < 1e-6, "glitch lost: max of the low part {}", mx);
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// Per-record cost of the display path (`cargo test --release bench_record -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn bench_record() {
        for &(tdiv, pts) in &[(1e-7, 1408usize), (1e-6, 12_500), (1e-5, 100_000)] {
            let mut m = Model::default();
            m.tscale = tscales().iter().position(|&x| (x as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
            m.trig_level = 0.0;
            m.ch[0].offset = 0.0;
            let dt = 0.8e-9 * (tdiv / 1e-7).max(1.0) * 1408.0 / pts as f64 * if pts == 1408 { 1.0 } else { 1.0 };
            let f = 3.0 / (tdiv * 10.0);
            let v: Vec<f32> = (0..pts).map(|k| (2.0 * std::f64::consts::PI * f * k as f64 * dt).sin() as f32).collect();
            let fr = Arc::new(Frame { seq: 1, dt, pre: pts / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (1.0 / dt, pts), stamp: 0, la: None, hw_avg: false });
            let window = tdiv * 10.0;
            let mut ph = crate::wave::Phosphor::new(970, 448);
            let n = 400;
            let (mut t_rec, mut t_draw, mut t_clone) = (0.0, 0.0, 0.0);
            for _ in 0..n {
                let t0 = std::time::Instant::now();
                let rec = to_record(&fr, &m, 3600, window, -window / 2.0);
                let t1 = std::time::Instant::now();
                ph.draw_ex(0, &m, (tdiv as f32, 0.0), rec.t0, rec.dt, &rec.ch[0], 24, None);
                let t2 = std::time::Instant::now();
                let h = crate::wave::Record { raw: None, ..rec.clone() };
                std::hint::black_box(&h);
                t_rec += (t1 - t0).as_secs_f64();
                t_draw += (t2 - t1).as_secs_f64();
                t_clone += t2.elapsed().as_secs_f64();
            }
            // the fast path: edge near pre, raw samples straight into the map
            let mut t_fast = 0.0;
            let mut map = vec![0u16; 970 * 448];
            for _ in 0..n {
                let t0 = std::time::Instant::now();
                let ti = crate::fastdraw::edge_near(&fr.ch[0], fr.pre, 0.0, 0.0, true, false, 16).unwrap_or(fr.pre as f64);
                let xm = crate::fastdraw::Map { ti, x_per_sample: dt / window * 970.0, x0: 485.0, y_per_volt: -56.0, y0: 224.0 };
                crate::fastdraw::deposit(&mut map, 970, 448, &fr.ch[0], &xm, 24);
                t_fast += t0.elapsed().as_secs_f64();
            }
            let us = |t: f64| t / n as f64 * 1e6;
            eprintln!("bench {:>6} pts {:>8.0e} s/div: to_record {:7.1} us, draw {:6.1} us, clone {:5.1} us | fast {:6.1} us", pts, tdiv, us(t_rec), us(t_draw), us(t_clone), us(t_fast));
        }
    }
}
