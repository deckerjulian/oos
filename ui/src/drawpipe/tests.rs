// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the deposit pipeline (`drawpipe`): the workers, the claims, the mailbox and the merge, the
//! counters, the codes path and `keep_out`, run on the host. A test of a new part of the pipeline goes here.

use super::*;
use oos_hal::dho::hz;
use oos_hal::dho::seq::Settings;

/// A read-out buffer for the tests.
struct Bytes(Vec<u8>);

impl RawBatch for Bytes {
    fn bytes(&self) -> &[u8] {
        &self.0
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

fn fresh() -> &'static Counters {
    Box::leak(Box::default())
}

fn dec(s: &Settings, p: &hz::Params) -> DecodeInto {
    let (s, p) = (s.clone(), *p);
    Arc::new(move |c: &[u8], f: &mut Frame| crate::board::dho::hw::decode_into(c, &s, &p, 0, f))
}

/// A raw batch of `frames` frames, CH1 (and CH2 with `two`): a sine with a rising zero crossing near the hardware
/// trigger point, a per-frame jitter, every fifth frame shifted up, every seventh without the crossing.
fn batch(frames: usize, memsel: u32, two: bool) -> (Vec<u8>, usize, Settings, hz::Params) {
    let r = if two { 2 } else { 1 };
    let p = hz::Params {
        ratio: r,
        rate: 1_250_000_000 / r as u64,
        memsel,
        dot_fs: 800_000,
        seg: 0,
        max_frames: 1000,
        pre: memsel / 2,
        post: memsel / 2,
    };
    let s = Settings { mask: if two { 3 } else { 1 }, window: None, compress: 1, header: false, ..Settings::default() };
    let fb = oos_hal::dho::seq::frame_bytes(&s, &p);
    let mut bytes = Vec::new();
    for k in 0..frames {
        let jit = (k % 7) as f64 * 0.13 - 0.4;
        for i in 0..memsel as usize {
            let ph = (i as f64 - memsel as f64 / 2.0 + jit) / 400.0 * std::f64::consts::TAU;
            for c in 0..r as usize {
                let v = if k % 7 == 6 {
                    0.7
                } else {
                    0.9 * (ph * (1.0 + c as f64 * 0.5)).sin() + if k % 5 == 0 { 0.05 } else { 0.0 }
                };
                bytes.extend(((32768.0 + v * 7500.0) as u16).to_le_bytes());
            }
        }
    }
    assert_eq!(bytes.len(), frames * fb, "frame layout");
    (bytes, fb, s, p)
}

fn model(tdiv: f64, two: bool, dots: bool) -> Model {
    let mut m = Model {
        tscale: tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-3).unwrap(),
        ..Default::default()
    };
    (m.trig_level, m.dots) = (0.0, dots);
    for c in 0..NCH {
        (m.ch[c].on, m.ch[c].offset) = (c == 0 || c == 1 && two, 0.0);
    }
    m
}

/// Frames of channels `mask` (ratio from the mask), `n` points each: per channel a signal from `sig(frame, channel,
/// point)` in volts at 1 V/div (7500 codes per volt).
fn frames_of(
    mask: u32,
    n: u32,
    frames: usize,
    sig: &dyn Fn(usize, usize, usize) -> f64,
) -> (Vec<u8>, usize, Settings, hz::Params) {
    let r = hz::ratio(mask);
    let p = hz::Params {
        ratio: r,
        rate: 1_250_000_000 / r as u64,
        memsel: n,
        dot_fs: 800_000 * r as u64,
        seg: 0,
        max_frames: 1000,
        pre: n / 2,
        post: n / 2,
    };
    let s = Settings { mask, window: None, compress: 1, header: false, ..Settings::default() };
    let fb = oos_hal::dho::seq::frame_bytes(&s, &p);
    let map = oos_hal::dho::seq::code_map(mask, s.ch.map(|k| (k.scale, k.offset)));
    let mut bytes = vec![0u8; frames * fb];
    for k in 0..frames {
        for i in 0..n as usize {
            for c in 0..NCH {
                if let Some(wd) = map.word[c] {
                    let x = (32768.0 + sig(k, c, i) * 7500.0).round().clamp(0.0, 65535.0) as u16;
                    let j = k * fb + 2 * (i * r as usize + wd);
                    bytes[j..j + 2].copy_from_slice(&x.to_le_bytes());
                }
            }
        }
    }
    (bytes, fb, s, p)
}

// Break it catches: the workers' codes path (large frames, no per-frame measurements) drawing another picture or
// other trigger statistics than `draw_frame` on every frame, or frames lost on it
#[test]
fn batch_picture_from_the_codes_equals_each_frame_drawn() {
    let (w, h) = (970, 448);
    let sig = |k: usize, c: usize, i: usize| {
        0.9 * (((i as f64 + (k % 7) as f64 * 0.13 - 35_000.0) / 400.0) * (1.0 + c as f64 * 0.5) * std::f64::consts::TAU)
            .sin()
            + if k.is_multiple_of(5) { 0.05 } else { 0.0 }
    };
    for (mask, n) in [(1u32, 100_000u32), (3, 70_312)] {
        let mut m = model(1e-5, mask == 3, false);
        m.meas = Vec::new();
        let (bytes, fb, s, p) = frames_of(mask, n, 9, &sig);
        assert!(fb >= CODES_MIN_BYTES);
        let pipe = Pipe::start(2, false, fresh());
        pipe.set_view(Some(&m), w, h);
        let (cs, cp) = (s.clone(), p);
        let codes: CodesInto =
            Arc::new(move |c: &[u8], f: &mut Frame| crate::board::dho::hw::codes_into(c, &cs, &cp, 0, f));
        pipe.feed_codes(Box::new(Bytes(bytes.clone())), fb, 0, 9, 0, dec(&s, &p), Some(codes));
        settle(&pipe);
        let mut got = Phosphor::new(w, h);
        let (mut clear, mut edges, mut off) = (false, [0u64; 4], (0.0, 0.0));
        let n_got = pipe.merge(&mut got, &mut clear, &mut edges, &mut off);
        let (want, trig, wedges, woff, _) = reference(&bytes, fb, &s, &p, &m, w, h);
        assert_eq!((n_got.frames, n_got.triggered, edges), (9, trig, wedges), "mask {}", mask);
        assert!((off.0 - woff.0).abs() <= 1e-9 * woff.0.abs().max(1.0) && off.1 == woff.1);
        for c in 0..NCH {
            assert!(got.map[c] == want.map[c], "mask {}: CH{} differs", mask, c + 1);
        }
    }
}

// Break it catches (`draw_frame_codes`): a picture or trigger statistics from the read-out words that differ from
// decoding the frame first (column extremes, join, interleaved channels, the near window, the wide fallback),
// or the codes path taken where it must not be.
#[test]
fn drawing_from_the_codes_equals_drawing_the_decoded_frame() {
    let (w, h) = (970, 448);
    let sine = |k: usize, c: usize, i: usize| {
        0.9 * (((i as f64 - 50_000.0 + (k % 7) as f64 * 0.13) / 400.0) * (1.0 + c as f64 * 0.5) * std::f64::consts::TAU)
            .sin()
    };
    let square = |k: usize, c: usize, i: usize| {
        let x = i as f64 + 37.0 * k as f64 + 101.0 * c as f64;
        (if (x / 9_000.0).fract() < 0.5 { 1.2 } else { -1.2 }) + 0.03 * ((i * 7919 + k * 31) % 101) as f64 / 101.0
    };
    let flat = |_k: usize, c: usize, i: usize| 0.2 + 0.001 * (i % 3) as f64 + c as f64 * 0.1;
    let mut taken = 0;
    let mut cases = 0;
    for (mask, n, tdiv) in [
        (1u32, 100_000u32, 1e-5),
        (1, 12_512, 1e-6),
        (3, 70_312, 1e-5),
        (3, 12_512, 1e-6),
        (15, 30_000, 1e-5),
        (5, 25_000, 1e-5),
        (1, 1408, 1e-7),
    ] {
        for (name, sig) in
            [("sine", &sine as &dyn Fn(usize, usize, usize) -> f64), ("square", &square), ("flat", &flat)]
        {
            for (src, dots, coup) in [
                (0usize, false, TrigCoup::Dc),
                (1, false, TrigCoup::Dc),
                (0, true, TrigCoup::Dc),
                (0, false, TrigCoup::Ac),
            ] {
                let mut m = model(tdiv, false, dots);
                for c in 0..NCH {
                    m.ch[c].on = mask & (1 << c) != 0;
                }
                m.trig_src = src;
                m.trig_coup = coup;
                let (bytes, fb, s, p) = frames_of(mask, n, 6, sig);
                let (mut a, mut b) = (Phosphor::new(w, h), Phosphor::new(w, h));
                let (mut sa, mut sb) = (([0u64; 4], (0.0, 0.0)), ([0u64; 4], (0.0, 0.0)));
                let (mut ta, mut tb) = (0, 0);
                for chunk in bytes.chunks_exact(fb) {
                    let f = crate::board::dho::hw::decode(chunk, &s, &p, 0);
                    ta += draw_frame(&mut a, &m, &f, Some(Stats { edges: &mut sa.0, off: &mut sa.1 })) as u32;
                    let mut g = Frame::default();
                    let lay = crate::board::dho::hw::codes_into(chunk, &s, &p, 0, &mut g).expect("raw frame");
                    assert!(g.ch.iter().all(|v| v.is_empty()) && (g.dt, g.pre, g.acq) == (f.dt, f.pre, f.acq));
                    match draw_frame_codes(
                        &mut b,
                        &m,
                        &g,
                        chunk,
                        &lay,
                        Some(Stats { edges: &mut sb.0, off: &mut sb.1 }),
                    ) {
                        Some(t) => {
                            tb += t as u32;
                            taken += 1;
                        }
                        None => {
                            assert!(
                                dots || coup != TrigCoup::Dc
                                    || f.dt / m.time_per_div() as f64 * w as f64 / HDIV as f64 > 0.5,
                                "{} {} codes path refused",
                                name,
                                mask
                            );
                            tb += draw_frame(&mut b, &m, &f, Some(Stats { edges: &mut sb.0, off: &mut sb.1 })) as u32;
                        }
                    }
                }
                let what = format!("{} mask {} n {} src {} dots {} {:?}", name, mask, n, src, dots, coup);
                assert_eq!((ta, sa.0), (tb, sb.0), "{}", what);
                assert!(sa.1 == sb.1, "{}: {:?} vs {:?}", what, sa.1, sb.1);
                for c in 0..NCH {
                    assert!(a.map[c] == b.map[c], "{}: CH{} differs", what, c + 1);
                }
                cases += 1;
            }
        }
    }
    assert!(taken > 150, "codes path taken {} times in {} cases", taken, cases);
    // the wide fallback was compared too
    assert!(WIDE.with(|n| n.get()) > 5, "{} wide searches", WIDE.with(|n| n.get()));
}

// Timing guard (tools/perf-guard.sh): a worker frame drawn from the read-out words against decode + draw, lines,
// no per-frame measurements. The A53 gains more than the host (the f32 copy is 2x the read-out per channel).
#[test]
#[ignore]
fn perf_guard_codes_per_frame() {
    let (w, h) = (970, 448);
    let sig = |k: usize, c: usize, i: usize| {
        0.9 * (((i as f64 + (k % 7) as f64 * 0.13) / 400.0) * (1.0 + c as f64 * 0.5) * std::f64::consts::TAU).sin()
    };
    let mut worst: f64 = 0.0;
    for (mask, n, tdiv) in [(1u32, 100_000u32, 1e-5), (3, 70_312, 1e-5), (5, 50_000, 1e-5), (1, 40_000, 2e-6)] {
        let mut m = model(tdiv, false, false);
        for c in 0..NCH {
            m.ch[c].on = mask & (1 << c) != 0;
        }
        let frames = (2_000_000 / (n as usize * 2 * hz::ratio(mask) as usize)).clamp(4, 100);
        let (bytes, fb, s, p) = frames_of(mask, n, frames, &sig);
        let (mut pa, mut pb) = (Phosphor::new(w, h), Phosphor::new(w, h));
        let (mut fa, mut fbm) = (Frame::default(), Frame::default());
        // one round: every frame once (each round's time divided by the frames below)
        let (old, new) = crate::perfguard::best_pair(
            crate::perfguard::ROUNDS,
            1,
            || {
                for chunk in bytes.chunks_exact(fb) {
                    crate::board::dho::hw::decode_into(chunk, &s, &p, 0, &mut fa);
                    std::hint::black_box(draw_frame(&mut pa, &m, &fa, None));
                }
            },
            || {
                for chunk in bytes.chunks_exact(fb) {
                    let lay = crate::board::dho::hw::codes_into(chunk, &s, &p, 0, &mut fbm).unwrap();
                    std::hint::black_box(draw_frame_codes(&mut pb, &m, &fbm, chunk, &lay, None).unwrap());
                }
            },
        );
        let (old, new) = (old / frames as u32, new / frames as u32);
        let r = new.as_secs_f64() / old.as_secs_f64();
        worst = worst.max(r);
        assert!(fb >= CODES_MIN_BYTES && hz::ratio(mask) as usize <= CODES_MAX_STRIDE);
        println!(
            "codes mask {:#x} {} pt: decode + draw {:?}/frame, from the codes {:?}/frame (ratio {:.2})",
            mask, n, old, new, r
        );
    }
    assert!(worst <= 0.9, "drawing from the codes must be cheaper: worst ratio {:.2}", worst);
}

/// Until every batch fed is done (drawn and handed on, or dropped); a batch that is never done hangs the test
/// instead of a deadline a busy computer could miss (ADR 0020).
fn settle(p: &Pipe) {
    while p.live.load(Ordering::Acquire) > 0 {
        std::thread::yield_now();
    }
}

/// Sets wait in the mailbox: the full one or parked ones.
fn pending(p: &Pipe) -> bool {
    let mb = p.mail.lock().unwrap();
    mb.full.is_some() || mb.parked.iter().any(Option::is_some)
}

/// Merges until the mailbox is empty, as the screens after a run do: the full set, then the parked ones, one a
/// merge. Returns the sums and the pixels each merge added.
fn merge_all(
    p: &Pipe,
    ph: &mut Phosphor,
    clear: &mut bool,
    edges: &mut [u64; 4],
    off: &mut (f64, f64),
) -> (Merged, Vec<u64>) {
    let (mut sum, mut px) = (Merged::default(), Vec::new());
    while pending(p) {
        let d = p.merge(ph, clear, edges, off);
        (sum.frames, sum.triggered) = (sum.frames + d.frames, sum.triggered + d.triggered);
        crate::framestats::add_folds(&mut sum.folds, &d.folds);
        px.push(d.px);
    }
    sum.px = px.iter().sum();
    (sum, px)
}

/// The same frames one by one: `draw_frame` and the per-frame statistics on every frame.
fn reference(
    bytes: &[u8],
    fb: usize,
    s: &Settings,
    p: &hz::Params,
    m: &Model,
    w: usize,
    h: usize,
) -> (Phosphor, u64, [u64; 4], (f64, f64), crate::framestats::Folds) {
    let mut want = Phosphor::new(w, h);
    let (mut trig, mut edges, mut off) = (0, [0u64; 4], (0.0, 0.0));
    let mut folds = crate::framestats::folds_for(m);
    for chunk in bytes.chunks_exact(fb) {
        let f = crate::board::dho::hw::decode(chunk, s, p, 0);
        trig += draw_frame(&mut want, m, &f, Some(Stats { edges: &mut edges, off: &mut off })) as u64;
        crate::framestats::fold_frame(&f, m, &mut folds);
    }
    (want, trig, edges, off, folds)
}

// Break it catches: a batch picture that differs from `draw_frame` on every frame (decode into kept
// vectors, frames claimed by several workers and the helper, dense and sparse frames, dots, two channels),
// trigger or measurement statistics that leave frames out, frames lost or counted twice, a merge that does not
// clear for minimum persistence, a buffer that does not come back.
#[test]
fn batch_picture_equals_each_frame_drawn() {
    let (w, h) = (970, 448);
    for (tdiv, memsel, two, dots, workers, helper) in [
        (1e-6, 12_512u32, false, false, 2, false),
        (1e-6, 12_512, true, true, 4, true),
        (1e-7, 1408, false, false, 1, true),
        (2e-8, 400, true, false, 1, true),
    ] {
        let mut m = model(tdiv, two, dots);
        m.meas = vec![
            (Meas::Vpp, crate::model::WaveId::Ch(0)),
            (Meas::Vavg, crate::model::WaveId::Ch(0)),
            (Meas::Vrms, crate::model::WaveId::Ch(1)),
        ];
        let (bytes, fb, s, p) = batch(41, memsel, two);
        let c = fresh();
        let pipe = Pipe::start(workers, false, c);
        pipe.set_view(Some(&m), w, h);
        assert!(pipe.accepting());
        pipe.feed(Box::new(Bytes(bytes[..17 * fb].to_vec())), fb, 17, 0, dec(&s, &p));
        pipe.feed(Box::new(Bytes(bytes[17 * fb..].to_vec())), fb, 24, 0, dec(&s, &p));
        // the acquisition thread's share, as while it waits for the next batch
        let mut hp = Helper::default();
        if helper {
            while pipe.help(&mut hp) {}
        }
        settle(&pipe);
        pipe.help_done(&mut hp);
        let mut got = Phosphor::new(w, h);
        got.map[0][0] = 7;
        let (mut clear, mut edges, mut off) = (true, [0u64; 4], (0.0, 0.0));
        let (n, px) = merge_all(&pipe, &mut got, &mut clear, &mut edges, &mut off);
        let (want, trig, wedges, woff, folds) = reference(&bytes, fb, &s, &p, &m, w, h);
        let what = format!("{} s/div, {} workers, helper {}", tdiv, workers, helper);
        assert_eq!((n.frames, n.triggered), (41, trig), "{}", what);
        assert!(!clear && trig > 30 && trig < 41, "{}: {} triggered", what, trig);
        // minimum persistence: the first set swapped in, nothing added on the UI's thread; a parked set after it
        // is added (the blocks with counts of one map a channel on)
        assert_eq!(px[0], 0, "{}", what);
        assert!(px[1..].iter().all(|&x| x > 0 && x <= (w * h * (1 + two as usize)) as u64), "{}: {:?}", what, px);
        assert_eq!(edges, wedges, "{}", what);
        assert!((off.0 - woff.0).abs() <= 1e-9 * woff.0.abs().max(1.0) && off.1 == woff.1);
        for c in 0..NCH {
            assert!(got.map[c] == want.map[c], "{}: CH{} differs", what, c + 1);
        }
        // per-frame statistics: the same frames counted, sums in another order
        assert_eq!(n.folds.len(), folds.len());
        for ((k, a), (k2, b)) in n.folds.iter().zip(&folds) {
            assert_eq!((k, a.n, a.min, a.max), (k2, b.n, b.min, b.max), "{}", what);
            assert!(
                (a.sum - b.sum).abs() <= 1e-12 * b.sum.abs().max(1.0)
                    && (a.sumsq - b.sumsq).abs() <= 1e-12 * b.sumsq.abs().max(1.0)
            );
        }
        assert!(tdiv < 1e-6 || folds.iter().all(|x| x.1.n > 30), "{}: {:?}", what, folds);
        assert_eq!(c.read(), (0, 0, 0), "{}: nothing dropped", what);
        assert_eq!(c.measured.load(Ordering::Relaxed), folds.first().map_or(0, |x| x.1.n));
        // merged once; both buffers back in the pool
        assert_eq!(pipe.merge(&mut got, &mut clear, &mut edges, &mut off), Merged::default());
        let back: Vec<_> = std::iter::from_fn(|| pipe.take_buffer()).collect();
        assert_eq!(back.len(), 2);
        assert!(back.into_iter().all(|b| b.downcast::<Bytes>().is_ok()));
    }
}

// Break it catches (issue #186): frames lost or merged twice while the workers and the helper fold their maps into
// one set and the UI takes it meanwhile, a merge that adds more than one set of maps on the UI's thread, or maps of
// channels that are off.
#[test]
fn every_frame_is_merged_once_while_the_workers_fold() {
    let (w, h) = (970, 448);
    let m = model(1e-6, false, false);
    let (bytes, fb, s, p) = batch(40, 12_512, false);
    let (want, ..) = reference(&bytes, fb, &s, &p, &m, w, h);
    let c = fresh();
    let pipe = Pipe::start(4, false, c);
    pipe.set_view(Some(&m), w, h);
    let rounds = 12u64;
    let mut ph = Phosphor::new(w, h);
    // a channel that is off keeps what it has (persistence)
    ph.map[1][5] = 3;
    let (mut frames, mut merges) = (0, 0);
    let done = std::sync::atomic::AtomicBool::new(false);
    let mut take = |ph: &mut Phosphor| {
        let got = pipe.merge(ph, &mut false, &mut [0; 4], &mut (0.0, 0.0));
        assert!(got.px <= (w * h) as u64, "{} pixels added", got.px);
        frames += got.frames;
        merges += (got.frames > 0) as u64;
    };
    std::thread::scope(|sc| {
        sc.spawn(|| {
            let mut hp = Helper::default();
            for _ in 0..rounds {
                pipe.feed(Box::new(Bytes(bytes.clone())), fb, 40, 0, dec(&s, &p));
                while pipe.help(&mut hp) {}
                pipe.help_done(&mut hp);
            }
            done.store(true, Ordering::Release);
        });
        while !done.load(Ordering::Acquire) {
            take(&mut ph);
        }
    });
    settle(&pipe);
    while pending(&pipe) {
        take(&mut ph);
    }
    assert_eq!((frames, c.read()), (40 * rounds, (0, 0, 0)), "{} merges", merges);
    assert!(ph.map[0].iter().zip(&want.map[0]).all(|(&x, &y)| x as u64 == y as u64 * rounds));
    assert!(ph.map[1].iter().enumerate().all(|(i, &x)| x == if i == 5 { 3 } else { 0 }));
    assert!(ph.map[2..].iter().all(|m| m.iter().all(|&x| x == 0)));
}

// Break it catches: the acquisition thread's share drawn differently from the workers', or not handed to the UI.
#[test]
fn the_helper_alone_draws_the_same_picture() {
    let (w, h) = (970, 448);
    for (tdiv, memsel) in [(1e-6, 12_512u32), (1e-7, 1408)] {
        let mut m = model(tdiv, false, false);
        m.meas = vec![(Meas::Vpp, crate::model::WaveId::Ch(0))];
        let (bytes, fb, s, p) = batch(20, memsel, false);
        let pipe = Pipe::start(0, false, fresh());
        pipe.set_view(Some(&m), w, h);
        let b = pipe.feed_codes(Box::new(Bytes(bytes.clone())), fb, 0, 20, 0, dec(&s, &p), None);
        let mut hp = Helper::default();
        let mut k = 0;
        while pipe.help(&mut hp) {
            k += 1;
            assert!(k < 100);
        }
        drop(b);
        let mut got = Phosphor::new(w, h);
        let n = pipe.merge(&mut got, &mut false, &mut [0; 4], &mut (0.0, 0.0));
        let (want, trig, ..) = reference(&bytes, fb, &s, &p, &m, w, h);
        assert_eq!((k, n.frames, n.triggered), (20, 20, trig), "{} s/div", tdiv);
        assert!((0..NCH).all(|c| got.map[c] == want.map[c]), "{} s/div", tdiv);
    }
}

// Break it catches: frame 0 of a batch (the frame the FPGA held across the arm, issue #5) drawn in batch mode,
// where the path without batches leaves it out.
#[test]
fn frames_before_first_are_left_out() {
    let (w, h) = (970, 448);
    let m = model(1e-6, false, false);
    let (bytes, fb, s, p) = batch(20, 12_512, false);
    let pipe = Pipe::start(0, false, fresh());
    pipe.set_view(Some(&m), w, h);
    let b = pipe.feed_codes(Box::new(Bytes(bytes.clone())), fb, 1, 20, 0, dec(&s, &p), None);
    let mut hp = Helper::default();
    let mut k = 0;
    while pipe.help(&mut hp) {
        k += 1;
        assert!(k < 100);
    }
    drop(b);
    let mut got = Phosphor::new(w, h);
    let n = pipe.merge(&mut got, &mut false, &mut [0; 4], &mut (0.0, 0.0));
    let (want, trig, ..) = reference(&bytes[fb..], fb, &s, &p, &m, w, h);
    assert_eq!((k, n.frames, n.triggered), (19, 19, trig));
    assert!((0..NCH).all(|c| got.map[c] == want.map[c]));
}

// Break it catches: frames of older settings drawn, maps of an older mapping merged, frames nobody drew not
// counted, batches taken while the fast path does not apply.
#[test]
fn stale_work_is_dropped_and_counted() {
    let (w, h) = (970, 448);
    let mut m = model(1e-6, false, false);
    let (bytes, fb, s, p) = batch(10, 12_512, false);
    let c = fresh();
    let pipe = Pipe::start(2, false, c);
    let mut ph = Phosphor::new(w, h);
    let merge = |ph: &mut Phosphor| pipe.merge(ph, &mut false, &mut [0; 4], &mut (0.0, 0.0));
    assert!(!pipe.accepting());
    // settings stamp older than the view's: dropped
    m.hw_stamp = 3;
    pipe.set_view(Some(&m), w, h);
    pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 2, dec(&s, &p));
    settle(&pipe);
    assert_eq!(merge(&mut ph), Merged::default());
    assert_eq!(c.read(), (0, 0, 10));
    // drawn, then the mapping changes before the merge: dropped at the merge
    pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
    settle(&pipe);
    m.ch[0].offset = 0.5;
    pipe.set_view(Some(&m), w, h);
    assert_eq!(merge(&mut ph), Merged::default());
    assert!(ph.map.iter().all(|x| x.iter().all(|&y| y == 0)));
    assert_eq!(c.read(), (0, 0, 20));
    // a change the drawing does not depend on keeps the maps
    pipe.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
    settle(&pipe);
    m.status = TrigStatus::Wait;
    pipe.set_view(Some(&m), w, h);
    assert_eq!(merge(&mut ph).frames, 10);
    // many batches at once: all drawn, nothing more dropped
    let (big, fb2, s2, p2) = batch(200, 12_512, false);
    for _ in 0..6 {
        pipe.feed(Box::new(Bytes(big.clone())), fb2, 200, 3, dec(&s2, &p2));
    }
    settle(&pipe);
    assert_eq!(merge_all(&pipe, &mut ph, &mut false, &mut [0; 4], &mut (0.0, 0.0)).0.frames, 1200);
    assert_eq!(c.read().2, 20);
    // a batch dropped before anybody claimed a frame counts all of them
    let pipe0 = Pipe::start(0, false, c);
    pipe0.set_view(Some(&m), w, h);
    pipe0.feed(Box::new(Bytes(bytes.clone())), fb, 10, 3, dec(&s, &p));
    assert_eq!(c.read().2, 30);
    // no view: the acquisition thread queues the frames itself
    pipe.set_view(None, 0, 0);
    assert!(!pipe.accepting());
}

/// Host cost per frame of a worker (decode into kept vectors, trigger refinement, deposit) against the frames
/// mode's per-frame work on the UI thread (`decode` + `draw_frame`), and the rate of 1, 2 and 4 workers (the
/// helper runs the same code per frame). Run by tools/perf-guard.sh.
#[test]
#[ignore]
fn perf_guard_deposit_worker_per_frame() {
    let (w, h) = (970, 448);
    for (tdiv, memsel, meas) in [
        (1e-7, 1408u32, false),
        (1e-6, 12_512, false),
        (1e-6, 12_512, true),
        (1e-5, 125_000, false),
        (1e-5, 125_000, true),
    ] {
        let mut m = model(tdiv, false, false);
        // the measurements made on every frame cost on top (reported, not guarded): the worst case, all of them on
        m.meas = if meas {
            crate::framestats::CHEAP.iter().map(|&k| (k, crate::model::WaveId::Ch(0))).collect()
        } else {
            Vec::new()
        };
        let frames = (2_000_000 / (memsel as usize * 2)).clamp(8, 500);
        let (bytes, fb, s, p) = batch(frames, memsel, false);
        // decode + draw_frame on one thread (the frame's work without the pipeline; its measurements: a few
        // records a second), against one worker of the pipeline, alternating (perfguard.rs); 2 and 4 workers are
        // reported only
        let mut ph = Phosphor::new(w, h);
        let one = Pipe::start(1, false, fresh());
        one.set_view(Some(&m), w, h);
        // the buffers fed are copied before, outside the timing (the copy is not the pipeline's work)
        let mut copies: Vec<Bytes> = (0..crate::perfguard::ROUNDS).map(|_| Bytes(bytes.clone())).collect();
        let (old, one_t) = crate::perfguard::best_pair(
            crate::perfguard::ROUNDS,
            1,
            || {
                for chunk in bytes.chunks_exact(fb) {
                    std::hint::black_box(draw_frame(
                        &mut ph,
                        &m,
                        &crate::board::dho::hw::decode(chunk, &s, &p, 0),
                        None,
                    ));
                }
            },
            || {
                one.feed(Box::new(copies.pop().expect("a copy per round")), fb, frames, 0, dec(&s, &p));
                settle(&one);
            },
        );
        let (old, mut per) = (old / frames as u32, vec![one_t / frames as u32]);
        for n in [2usize, 4] {
            let pipe = Pipe::start(n, false, fresh());
            pipe.set_view(Some(&m), w, h);
            let t0 = std::time::Instant::now();
            for _ in 0..3 {
                pipe.feed(Box::new(Bytes(bytes.clone())), fb, frames, 0, dec(&s, &p));
                settle(&pipe);
            }
            per.push(t0.elapsed() / (3 * frames) as u32);
        }
        let r = per[0].as_secs_f64() / old.as_secs_f64();
        println!("{} s/div {} pt, frame statistics {}: one by one {:?}/frame; one core in the pipeline {:?}/frame (ratio {:.2}); 1/2/4 workers {:.0}/{:.0}/{:.0} frames/s", tdiv, memsel, meas, old, per[0], r, 1.0 / per[0].as_secs_f64(), 1.0 / per[1].as_secs_f64(), 1.0 / per[2].as_secs_f64());
        assert!(meas || r <= 1.5, "a frame in the pipeline must not cost more than one by one: {:.2}", r);
    }
}

/// One batch of 16 frames of `bytes` as the acquisition thread hands it on in batch mode: frame 0 left out, the
/// others to the workers and its own share (`help`), until all are drawn and in the mailbox.
fn feed_batch(pipe: &Pipe, bytes: &[u8], fb: usize, s: &Settings, p: &hz::Params, stamp: u64, hp: &mut Helper) {
    let (cs, cp) = (s.clone(), *p);
    let codes: CodesInto =
        Arc::new(move |c: &[u8], f: &mut Frame| crate::board::dho::hw::codes_into(c, &cs, &cp, 0, f));
    let frames = bytes.len() / fb;
    let b = pipe.feed_codes(Box::new(Bytes(bytes.to_vec())), fb, 1, frames - 1, stamp, dec(s, p), Some(codes));
    while pipe.help(hp) {}
    drop(b);
    settle(pipe);
    pipe.help_done(hp);
    while pipe.take_buffer().is_some() {}
}

/// The frame loop's time per step at 10 us/div, 1, 2 and 4 channels (250 kB frames, 16 a batch, as the unit reads
/// them), the workers against the queue for the same frames, one batch a step (issue #186). With the workers (4,
/// as on the unit) they and the acquisition thread's share have drawn the batch before the step, so what is timed
/// is the frame loop's own work; through the queue the frame loop draws the frames itself (with its helpers), as
/// it does with the frames of a batch the workers do not take. Also the merge alone, with minimum persistence (the
/// maps swapped in) and with persistence (added): its time and the pixels it adds. Guarded: the workers must not
/// cost the frame loop more than the queue. Run by tools/perf-guard.sh.
#[test]
#[ignore]
fn perf_guard_batch_step_at_10_us_per_div() {
    use crate::wave::Source;
    let sig = |k: usize, c: usize, i: usize| {
        0.9 * (((i as f64 + (k % 7) as f64 * 0.13) / 400.0) * (1.0 + c as f64 * 0.5) * std::f64::consts::TAU).sin()
    };
    let pipe = start_for_tests(4);
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mut worst: f64 = 0.0;
    for (mask, n) in [(1u32, 125_000u32), (3, 62_500), (15, 31_250)] {
        let frames = 16;
        let (bytes, fb, s, p) = frames_of(mask, n, frames, &sig);
        let mut per = [0.0f64; 2];
        let mut hp = Helper::default();
        for (k, batch) in [false, true].into_iter().enumerate() {
            let shared: crate::board::HwShared = Default::default();
            let mut ui = crate::ui::Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
            ui.measure_in_background();
            ui.m.run = RunState::Run;
            ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-5).abs() < 1e-8).unwrap();
            ui.m.trig_level = 0.0;
            for c in 0..NCH {
                (ui.m.ch[c].on, ui.m.ch[c].offset) = (mask & (1 << c) != 0, 0.0);
            }
            // the first step publishes the mapping to the workers
            ui.step(1.0 / 30.0);
            let mut times = Vec::new();
            for step in 0..40 {
                let stamp = ui.m.hw_stamp;
                let mut newest =
                    Frame { stamp, ..crate::board::dho::hw::decode(&bytes[(frames - 1) * fb..], &s, &p, 0) };
                // the record's readings, made by the acquisition thread
                newest.basic = std::array::from_fn(|c| crate::board::BasicStats::of(&newest.ch[c]));
                if batch {
                    feed_batch(pipe, &bytes, fb, &s, &p, stamp, &mut hp);
                } else {
                    let mut q = shared.lock().unwrap();
                    for chunk in bytes.chunks_exact(fb).skip(1).take(frames - 2) {
                        q.queue.push_back(Frame { stamp, ..crate::board::dho::hw::decode(chunk, &s, &p, 0) });
                    }
                }
                shared.lock().unwrap().queue.push_back(newest);
                let t0 = std::time::Instant::now();
                ui.step(1.0 / 30.0);
                if step >= 8 {
                    times.push(t0.elapsed().as_secs_f64() * 1e3);
                }
            }
            // every frame drawn (fast path or pipeline), 15 a step
            assert_eq!(ui.counts.wfm_total, 15 * 40, "{} frames drawn", ui.counts.wfm_total);
            per[k] = median(times);
            let mode = if batch { "workers" } else { "queue" };
            println!("10 us/div mask {:#x} {} pt, {}: frame loop {:.2} ms a step (median)", mask, n, mode, per[k]);
        }
        // the merge alone, with the mapping of the last step's view
        let mut m = model(1e-5, false, false);
        for c in 0..NCH {
            m.ch[c].on = mask & (1 << c) != 0;
        }
        let (w, h) = (970, 448);
        pipe.set_view(Some(&m), w, h);
        let mut ph = Phosphor::new(w, h);
        for clear in [true, false] {
            let (mut times, mut px) = (Vec::new(), 0);
            for _ in 0..20 {
                feed_batch(pipe, &bytes, fb, &s, &p, m.hw_stamp, &mut hp);
                let t0 = std::time::Instant::now();
                let got = pipe.merge(&mut ph, &mut clear.clone(), &mut [0; 4], &mut (0.0, 0.0));
                times.push(t0.elapsed().as_secs_f64() * 1e3);
                assert_eq!(got.frames, frames as u64 - 2);
                px = got.px;
            }
            println!("  merge, clear {}: {:.3} ms (median), {} pixels added", clear, median(times), px);
        }
        worst = worst.max(per[1] / per[0]);
    }
    assert!(worst <= 1.0, "the workers must not cost the frame loop more than the queue: ratio {:.2}", worst);
}

// Break it catches (unit 2026-11-18, more batches dropped for want of a buffer): a batch's buffer kept until every
// holder of the batch lets it go (a worker folding its maps into the mailbox, a worker with the batch in its
// queue) instead of coming back with its last frame drawn.
#[test]
fn the_buffer_comes_back_with_the_last_frame_drawn() {
    let m = model(1e-6, false, false);
    let (bytes, fb, s, p) = batch(10, 12_512, false);
    let pipe = Pipe::start(0, false, fresh());
    pipe.set_view(Some(&m), 970, 448);
    // held as by a worker that still hands its maps on
    let b = pipe.feed_codes(Box::new(Bytes(bytes)), fb, 0, 10, 0, dec(&s, &p), None);
    let mut hp = Helper::default();
    while pipe.help(&mut hp) {}
    assert!(pipe.take_buffer().is_some(), "every frame drawn: the buffer is back while the batch is held");
    drop(b);
}

// Break it catches (unit 2026-11-18, more frames dropped at 4 channels): the drawers folding their maps into the
// mailbox after every batch (folds a screen that grow with the batches: worker time and memory traffic on the
// small cores taken from drawing), or frames lost while they park their maps between screens.
#[test]
fn the_drawers_fold_once_a_screen() {
    let (w, h) = (970, 448);
    let m = model(1e-6, false, false);
    let (bytes, fb, s, p) = batch(40, 12_512, false);
    let c = fresh();
    let pipe = Pipe::start(4, false, c);
    pipe.set_view(Some(&m), w, h);
    let (screens, per) = (5u64, 8u64);
    let mut hp = Helper::default();
    let mut ph = Phosphor::new(w, h);
    let mut frames = 0;
    for _ in 0..screens {
        for _ in 0..per {
            let b = pipe.feed_codes(Box::new(Bytes(bytes.clone())), fb, 0, 40, 0, dec(&s, &p), None);
            while pipe.help(&mut hp) {}
            drop(b);
            settle(&pipe);
            pipe.help_done(&mut hp);
        }
        frames += pipe.merge(&mut ph, &mut true, &mut [0; 4], &mut (0.0, 0.0)).frames;
    }
    let folds = pipe.mail.lock().unwrap().folds;
    frames += merge_all(&pipe, &mut ph, &mut true, &mut [0; 4], &mut (0.0, 0.0)).0.frames;
    assert_eq!((frames, c.read()), (40 * per * screens, (0, 0, 0)));
    // each of the 5 drawers hands its maps on once a screen at most, the first into the empty mailbox
    assert!(folds <= 4 * screens, "{} folds in {} screens of {} batches", folds, screens, per);
}

// Break it catches (unit 2026-11-18, 9-14 % fewer frames at 4 channels): a fold or a clear that goes over the whole
// map where the traces hold counts in a small part of it (where the batches are fewer than the screens every
// drawer of a batch folds its maps, at 4 channels four 970 x 448 maps each on a small core), counts left out, or
// pixels outside the blocks with counts touched wrongly (saturation, the last partial block).
#[test]
fn folding_and_clearing_touch_the_blocks_with_counts() {
    let (w, h) = (970, 448);
    let n = w * h;
    // a batch's maps, as a drawer folds them: the traces of 40 frames, a sine with jitter and shifted frames
    let m = model(1e-6, false, false);
    let (bytes, fb, s, p) = batch(40, 12_512, false);
    let (drawn, ..) = reference(&bytes, fb, &s, &p, &m, w, h);
    let mut set = Phosphor::new(w, h);
    let px = add_map(&mut set.map[0], &drawn.map[0]);
    assert!(set.map[0] == drawn.map[0]);
    assert!(px > 0 && px * 2 <= n as u64, "{} of {} pixels touched", px, n);
    // the sums, saturated, and a last block of 59 pixels
    let n = n - 5;
    let mut src = vec![0u16; n];
    for &(i, v) in &[(0usize, 3u16), (63, 1), (64, 7), (5000, u16::MAX), (n - 1, 2)] {
        src[i] = v;
    }
    let mut dst = vec![1u16; n];
    dst[5000] = 9;
    assert_eq!(add_map(&mut dst, &src), 3 * BLOCK as u64 + 59);
    let want: Vec<u16> = (0..n).map(|i| if i == 5000 { u16::MAX } else { 1 + src[i] }).collect();
    assert!(dst == want);
    clear_map(&mut src);
    assert!(src.iter().all(|&x| x == 0));
}

/// The unit's loop at 10 us/div at a fixed rate (issue #186, unit 2026-11-18): a batch of 16 frames of 250 kB
/// every 600, 400 and 250 us, read into one of 3 buffers (`hw::POOL`) and dropped when none is back, 4 workers,
/// the acquisition thread's share while the next one "fills", the frame loop merging 29 times a second with
/// minimum persistence. Prints the frames dropped for want of a buffer and the frames each screen got. Guarded:
/// every frame merged or counted, and the median screen gets at least half of its share (a set out of the mailbox
/// while the drawers fold it leaves screens empty). The drops depend on the computer; host (M3), 4 channels, a
/// batch every 400 us: 31-37 % with a mailbox per drawer, 40 % with the shared one folded after every batch,
/// 1-13 % with the maps parked between screens. Run by tools/perf-guard.sh.
#[test]
#[ignore]
fn perf_guard_batch_at_a_fixed_rate() {
    use std::time::{Duration, Instant};
    let sig = |k: usize, c: usize, i: usize| {
        0.9 * (((i as f64 + (k % 7) as f64 * 0.13) / 400.0) * (1.0 + c as f64 * 0.5) * std::f64::consts::TAU).sin()
    };
    let frames = 16;
    for mask in [1u32, 15] {
        let (bytes, fb, s, p) = frames_of(mask, 125_000 / hz::ratio(mask), frames, &sig);
        let (cs, cp) = (s.clone(), p);
        let codes: CodesInto =
            Arc::new(move |x: &[u8], f: &mut Frame| crate::board::dho::hw::codes_into(x, &cs, &cp, 0, f));
        let mut m = model(1e-5, false, false);
        for k in 0..NCH {
            m.ch[k].on = mask & (1 << k) != 0;
        }
        for us in [600u64, 400, 250] {
            let c = fresh();
            let pipe = Pipe::start(4, false, c);
            let (w, h) = (970, 448);
            pipe.set_view(Some(&m), w, h);
            let done = std::sync::atomic::AtomicBool::new(false);
            let (mut fed, mut nobuf, mut merged, mut per) = (0u64, 0u64, 0u64, Vec::new());
            std::thread::scope(|sc| {
                // the acquisition thread
                sc.spawn(|| {
                    let mut hp = Helper::default();
                    let (mut made, mut cur) = (1, Box::new(Bytes(bytes.clone())));
                    let t0 = Instant::now();
                    let mut due = t0;
                    while t0.elapsed() < Duration::from_secs(2) {
                        due += Duration::from_micros(us);
                        while Instant::now() < due {
                            if !pipe.help(&mut hp) {
                                std::thread::yield_now();
                            }
                        }
                        let back = pipe.take_buffer().map(|x| x.downcast::<Bytes>().expect("test buffer"));
                        let next = back.or_else(|| {
                            (made < 3).then(|| {
                                made += 1;
                                Box::new(Bytes(bytes.clone()))
                            })
                        });
                        match next {
                            Some(nx) => {
                                let full = std::mem::replace(&mut cur, nx);
                                pipe.feed_codes(full, fb, 0, frames - 1, 0, dec(&s, &p), Some(codes.clone()));
                                fed += frames as u64 - 1;
                            }
                            None => nobuf += frames as u64 - 1,
                        }
                    }
                    while pipe.help(&mut hp) {}
                    pipe.help_done(&mut hp);
                    done.store(true, Ordering::Release);
                });
                let mut ph = Phosphor::new(w, h);
                while !done.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_micros(34_000));
                    let got = pipe.merge(&mut ph, &mut true, &mut [0; 4], &mut (0.0, 0.0)).frames;
                    merged += got;
                    per.push(got);
                }
            });
            settle(&pipe);
            let screens = per.len() as u64;
            merged += merge_all(&pipe, &mut Phosphor::new(w, h), &mut true, &mut [0; 4], &mut (0.0, 0.0)).0.frames;
            per.sort();
            let p50 = per[per.len() / 2];
            println!(
                "mask {:#x}, a batch every {} us: {} frames, {:.0} % dropped for want of a buffer; {} screens, \
                 frames a screen p10 {} p50 {} p90 {}",
                mask,
                us,
                fed + nobuf,
                100.0 * nobuf as f64 / (fed + nobuf) as f64,
                screens,
                per[per.len() / 10],
                p50,
                per[per.len() * 9 / 10],
            );
            // every frame fed merged or counted: dropped only when every worker is 16 batches behind (a busy computer)
            let (_, _, dropped) = c.read();
            assert_eq!(merged + dropped, fed, "mask {:#x} {} us: frames lost", mask, us);
            assert!(p50 * 2 >= fed / screens, "mask {:#x} {} us: median screen {} of {}", mask, us, p50, fed / screens);
        }
    }
}

// Break it catches: a thread kept out of the workers for good after the live run, so a later test on the same
// thread never reaches them, or a guard that a panic leaves set
#[test]
fn keep_out_holds_while_its_guard_lives() {
    let out = || OUT.with(|o| o.get());
    assert!(!out());
    {
        let _g = keep_out();
        assert!(out() && pipe().is_none());
        let _inner = keep_out();
        assert!(out());
    }
    assert!(!out(), "back after the guards");
    let r = std::panic::catch_unwind(|| {
        let _g = keep_out();
        panic!("a failing case");
    });
    assert!(r.is_err() && !out(), "back after a panic");
}
