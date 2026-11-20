// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the acquisition per frame: live raw and pair records with their background measurements, the trace
//! brightness per picture, hardware trigger status, hardware segments, the fast path for queued frames and hardware
//! averaging. STOP and SINGLE are in `stop_tests.rs`, other tests of the screen in `tests.rs`.

// Break it catches (unit 2026-11-13, 1 ms/div 10 M, meas.deep_raw = on): the background measurement measuring the
// newest record when the worker gets free. Raw deep records come once a second between pair records (held), so it
// measured a pair record: Mean stayed NaN live. The raw record waiting for the worker is the one measured.
#[test]
fn live_raw_deep_records_are_measured_while_the_worker_is_busy() {
    for tdiv in [1e-3, 2e-3, 5e-3] {
        live_raw_at(tdiv);
    }
}

fn live_raw_at(tdiv: f64) {
    use super::*;
    crate::deepmeas::force(Some(true));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.measure_in_background();
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-6).unwrap();
    ui.m.mdepth = mdepths().iter().position(|d| d.1 == 10_000_000).unwrap();
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
        seq,
        dt: window * 1.2 / n as f64,
        pre: n / 2,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        envelope,
        acq: (1e7 / (tdiv * 16.0), 10_000_000),
        ..Default::default()
    };
    let raw = |high: f32| -> Vec<f32> { (0..n).map(|i| if (i / 50) % 4 == 0 { high } else { 0.0 }).collect() };
    let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 1.0 }).collect();
    let mut seq = 0;
    for round in 0..3 {
        // a new value each round: a measurement that stays at the last one is caught too
        let high = 2.0 + round as f32;
        // the worker is still on the previous record when the raw one comes, and pair records follow
        ui.meas_bg.as_mut().unwrap().busy = true;
        seq += 1;
        shared.lock().unwrap().queue.push_back(frame(seq, false, raw(high)));
        // one batch of one frame each at 2 and 5 ms/div: the next pair record is queued before the UI looks
        if tdiv > 1.5e-3 {
            seq += 1;
            shared.lock().unwrap().queue.push_back(frame(seq, true, pairs.clone()));
        }
        ui.step(0.02);
        for _ in 0..3 {
            seq += 1;
            shared.lock().unwrap().queue.push_back(frame(seq, true, pairs.clone()));
            ui.step(0.02);
        }
        ui.meas_bg.as_mut().unwrap().busy = false;
        for _ in 0..50 {
            ui.step(0.02);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let avg = ui.meas(0, Meas::Vavg);
        assert!((avg - high as f64 / 4.0).abs() < 0.03, "Mean live at {} s/div, round {}: {}", tdiv, round, avg);
    }
    crate::deepmeas::force(None);
}

// Break it catches: Clear keeping the raw deep record that waits for the busy worker. The next record asked for a
// measurement and the worker took the record cleared away: its values were kept and shown for the pair records
// after the Clear (held, deepmeas.rs).
#[test]
fn clear_drops_the_record_waiting_for_the_worker() {
    use super::*;
    crate::deepmeas::force(Some(true));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.measure_in_background();
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    let frame = |seq: u64, envelope: bool, depth: usize, high: f32| {
        let v: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { high } else { 0.0 }).collect();
        let dt = window * 1.2 / n as f64;
        let ch = [v, Vec::new(), Vec::new(), Vec::new()];
        crate::board::Frame { seq, dt, pre: n / 2, ch, envelope, acq: (1e9, depth), ..Default::default() }
    };
    // steps until the worker has answered what the record asked for (however slow the computer)
    let wait = |ui: &mut Ui| {
        let t = std::time::Instant::now();
        loop {
            ui.step(0.02);
            if (!ui.meas_bg.as_ref().unwrap().busy && !ui.meas_want) || t.elapsed().as_secs() > 20 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    };
    // a raw deep record waits for the busy worker
    ui.meas_bg.as_mut().unwrap().busy = true;
    shared.lock().unwrap().queue.push_back(frame(1, false, 10_000_000, 2.0));
    ui.step(0.02);
    assert!(ui.meas_pending.is_some());
    ui.clear_display();
    // the worker is free and a shallow record comes: it is the one measured
    ui.meas_bg.as_mut().unwrap().busy = false;
    shared.lock().unwrap().queue.push_back(frame(2, false, 100_000, 4.0));
    wait(&mut ui);
    // a deep pair record: its measurement stays that of the last record measured
    shared.lock().unwrap().queue.push_back(frame(3, true, 10_000_000, 1.0));
    wait(&mut ui);
    let avg = ui.meas(0, Meas::Vavg);
    assert!((avg - 1.0).abs() < 0.03, "Mean held after the Clear: {}", avg);
    crate::deepmeas::force(None);
}

// Break it catches: Mean, RMS and Std dev shown from min/max pairs (a deep record read through the FPGA
// compression), or Max/Min/Pk-pk lost with them
#[test]
fn measurements_on_min_max_pairs_leave_out_the_mean_type() {
    use super::*;
    // without the raw reads of deepmeas.rs (meas.deep_raw = off)
    crate::deepmeas::force(Some(false));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    let v: Vec<f32> = (0..n)
        .map(|i| {
            if (i / 2) % 50 < 10 {
                1.0
            } else if i % 2 == 0 {
                -1.0
            } else {
                -0.98
            }
        })
        .collect();
    for (seq, envelope) in [(1u64, true), (2, false), (3, true)] {
        // a deep record (above 1 M points): pairs of the live compression are held instead (compress::hold)
        shared.lock().unwrap().queue.push_back(crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()],
            envelope,
            acq: (1e9, 10_000_000),
            ..Default::default()
        });
        for _ in 0..3 {
            ui.step(0.02);
        }
        let (avg, rms, max, pp) =
            (ui.meas(0, Meas::Vavg), ui.meas(0, Meas::Vrms), ui.meas(0, Meas::Vmax), ui.meas(0, Meas::Vpp));
        assert_eq!(avg.is_nan() && rms.is_nan(), envelope, "envelope {}: mean {} rms {}", envelope, avg, rms);
        assert!((max - 1.0).abs() < 0.05 && (pp - 2.0).abs() < 0.05, "envelope {}: max {} pk-pk {}", envelope, max, pp);
        let row = ui.meas_rows().into_iter().find(|r| r.name == Meas::Vavg.name()).map(|r| r.v);
        assert!(row.is_none_or(|x| x.is_nan() == envelope));
    }
    crate::deepmeas::force(None);
}

// Break it catches (compress.rs, raw newest frame every RAW_PERIOD): live-compression pair records measured between
// two raw ones (mean-type NaN flicker, statistics counting pairs), a raw record not measured, a STOP that keeps a
// pair record instead of asking for the final raw frame
#[test]
fn live_pair_records_keep_the_raw_measurement_and_stop_takes_a_raw_frame() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0)), (Meas::Vmax, WaveId::Ch(0))];
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
        seq,
        dt: window * 1.2 / n as f64,
        pre: n / 2,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        envelope,
        acq: (625e6, 100_000),
        ..Default::default()
    };
    let raw: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { 1.0 } else { 0.0 }).collect();
    let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 1.5 }).collect();
    let steps = |ui: &mut Ui, k: usize| {
        for _ in 0..k {
            ui.step(0.02);
        }
    };
    shared.lock().unwrap().queue.push_back(frame(1, false, raw.clone()));
    steps(&mut ui, 2);
    let avg = ui.meas(0, Meas::Vavg);
    assert!((avg - 0.25).abs() < 0.02, "raw mean {}", avg);
    let counted = ui.mstat.first().map_or(0, |s| s.n);
    for seq in 2..5 {
        shared.lock().unwrap().queue.push_back(frame(seq, true, pairs.clone()));
        steps(&mut ui, 2);
        assert_eq!(ui.meas(0, Meas::Vavg), avg);
        assert!((ui.meas(0, Meas::Vmax) - 1.0).abs() < 0.02);
    }
    assert_eq!(ui.mstat.first().map_or(0, |s| s.n), counted);
    // STOP on a pair record: the final raw frame is asked for and shown
    ui.m.run = RunState::Stop;
    steps(&mut ui, 1);
    assert!(ui.stop.final_until.is_some() && shared.lock().unwrap().final_req == 1);
    {
        let mut g = shared.lock().unwrap();
        g.queue.push_back(frame(5, false, raw.iter().map(|x| 2.0 * x).collect()));
        g.final_ack = 1;
    }
    steps(&mut ui, 2);
    assert!(ui.stop.final_until.is_none());
    assert_eq!(
        ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope)),
        Some((5, false))
    );
    assert!((ui.meas(0, Meas::Vavg) - 0.5).abs() < 0.03);
}

// Break it catches: the protocol watch or XY reading pair records (the hardware thread is not told)
#[test]
fn protocol_watch_and_xy_ask_for_raw_frames() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.step(0.02);
    assert!(!shared.lock().unwrap().needs_raw);
    ui.azr.watch = true;
    ui.step(0.02);
    assert!(shared.lock().unwrap().needs_raw);
    (ui.azr.watch, ui.xy) = (false, true);
    ui.step(0.02);
    assert!(shared.lock().unwrap().needs_raw);
    ui.xy = false;
    ui.step(0.02);
    assert!(!shared.lock().unwrap().needs_raw);
}

// Break it catches (deepmeas.rs): with raw deep reads on, a pair record measured between two raw ones (Mean back
// to NaN, statistics counting pair records), a raw deep record not measured, the hold active in Stop or Single
#[test]
fn deep_raw_records_carry_the_measurements_between_pair_records() {
    use super::*;
    crate::deepmeas::force(Some(true));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0)), (Meas::Vmax, WaveId::Ch(0))];
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    // raw: mean 0.25; pairs: the min/max of the same signal, other top so the record shown is visible
    let raw: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { 1.0 } else { 0.0 }).collect();
    let pairs = |top: f32| -> Vec<f32> { (0..n).map(|i| if i % 2 == 0 { 0.0 } else { top }).collect() };
    let push = |seq: u64, envelope: bool, v: Vec<f32>| {
        shared.lock().unwrap().queue.push_back(crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            envelope,
            acq: (1e9, 10_000_000),
            ..Default::default()
        });
    };
    let step = |ui: &mut Ui| {
        for _ in 0..3 {
            ui.step(0.02);
        }
    };
    // before the first raw record: no mean-type value on pairs, the maximum from them
    push(1, true, pairs(1.0));
    step(&mut ui);
    assert!(ui.meas(0, Meas::Vavg).is_nan() && (ui.meas(0, Meas::Vmax) - 1.0).abs() < 0.02);
    push(2, false, raw.clone());
    step(&mut ui);
    let avg = ui.meas(0, Meas::Vavg);
    assert!((avg - 0.25).abs() < 0.02, "raw mean {}", avg);
    let counted = ui.mstat.first().map_or(0, |s| s.n);
    // pair records keep the raw record's values, the statistics do not count them
    for seq in 3..6 {
        push(seq, true, pairs(1.5));
        step(&mut ui);
        assert_eq!(ui.meas(0, Meas::Vavg), avg);
        assert!((ui.meas(0, Meas::Vmax) - 1.0).abs() < 0.02);
    }
    assert_eq!(ui.mstat.first().map_or(0, |s| s.n), counted);
    // off: pair records are measured (mean-type NaN, max of the pairs)
    crate::deepmeas::force(Some(false));
    push(6, true, pairs(1.5));
    step(&mut ui);
    assert!(ui.meas(0, Meas::Vavg).is_nan() && (ui.meas(0, Meas::Vmax) - 1.5).abs() < 0.02);
    crate::deepmeas::force(None);
}

// Break it catches: the newest frame of a picture drawn another way than the others (record points instead of the
// frame's samples), so the grade follows the mix of the two in each picture: the flicker at 500 us/div on the
// probe-comp output (docs/fork/flicker.md). Equal frames, 1 to 5 of them per picture, must look the same.
#[test]
fn hardware_pictures_look_the_same_whatever_the_frames_per_picture() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    ui.m.persist = 0;
    ui.m.trig_level = 0.15;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    assert!(ui.fast_ok());
    // 100 k samples over 1.2 screens: a 0.3 V square with a little noise
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 100_000usize;
    let mut x = 9u32;
    let v: Vec<f32> = (0..n)
        .map(|k| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (if (k / 4000) % 2 == 1 { 0.3 } else { 0.0 }) + ((x >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.004
        })
        .collect();
    let frame = |seq: u64| crate::board::Frame {
        seq,
        dt: window * 1.2 / n as f64,
        pre: n / 2,
        ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()],
        ..Default::default()
    };
    let mut seq = 0;
    let mut picture = |ui: &mut Ui, k: usize| -> Vec<u32> {
        {
            let mut g = shared.lock().unwrap();
            for _ in 0..k {
                seq += 1;
                g.queue.push_back(frame(seq));
            }
        }
        ui.step(0.033);
        let (lut, sh, _) = ui.trace_lut(ui.grade.vmax[0]);
        ui.ph.map[0].iter().map(|&c| if c == 0 { 0 } else { lut[((c as u32) >> sh).min(1023) as usize] }).collect()
    };
    let first = picture(&mut ui, 3);
    for k in [1, 2, 3, 1, 5, 1, 2] {
        let p = picture(&mut ui, k);
        let worst = first.iter().zip(&p).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(worst <= 3, "{} frames: brightness off by {}", k, worst);
    }
}

// Break it catches: Trig'd shown for a batch the auto timeout forced (the software found an edge in it), or the
// hardware status ignored when the thread reports it
#[test]
fn hardware_trigger_status_overrides_the_software_one() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    ui.m.trig_level = 0.5;
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 4000usize;
    let frame = |seq: u64| crate::board::Frame {
        seq,
        dt: window / n as f64 * 1.2,
        pre: n / 2,
        ch: [(0..n).map(|k| if (k / 200) % 2 == 1 { 1.0 } else { 0.0 }).collect(), Vec::new(), Vec::new(), Vec::new()],
        ..Default::default()
    };
    let mut seq = 0;
    let mut run = |ui: &mut Ui, fired: Option<(u64, Option<bool>)>| {
        {
            let mut g = shared.lock().unwrap();
            for _ in 0..3 {
                seq += 1;
                g.queue.push_back(frame(seq));
            }
            g.trig_fired = fired;
        }
        ui.step(0.02);
        ui.m.status
    };
    // off (no reading): the software finds the edge at the hardware point
    assert_eq!(run(&mut ui, None), TrigStatus::Triggered);
    assert_eq!(run(&mut ui, Some((1, Some(false)))), TrigStatus::Auto);
    assert_eq!(run(&mut ui, Some((2, Some(true)))), TrigStatus::Triggered);
}

// Break it catches: the trace brightness at minimum persistence following the number of frames per picture (one
// frame after a batch of several came out dim: the flicker after autoset, docs/fork/trigger-lockin.md)
#[test]
fn equal_frames_give_equal_brightness_whatever_their_number() {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.persist = 0;
    for c in 1..super::NCH {
        ui.m.ch[c].on = false;
    }
    // one frame: a 1 kHz square with a little noise, 3600 points over the screen
    let mut x = 3u32;
    let v: Vec<f32> = (0..3600)
        .map(|k| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (if (k / 360) % 2 == 0 { 0.0 } else { 0.3 }) + ((x >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.004
        })
        .collect();
    let (tdiv, window) = (ui.m.time_per_div(), ui.m.time_per_div() as f64 * super::HDIV as f64);
    let picture = |ui: &mut super::Ui, n: u32| -> Vec<u32> {
        ui.ph.clear();
        for _ in 0..n {
            ui.ph.draw_ex(0, &ui.m.clone(), (tdiv, 0.0), -window / 2.0, window / 3600.0, &v, 24, None, false);
        }
        ui.grade.frames = n;
        ui.grade_refs();
        let (lut, sh, _) = ui.trace_lut(ui.grade.vmax[0]);
        // as `draw_traces`: an empty pixel stays dark
        ui.ph.map[0].iter().map(|&k| if k == 0 { 0 } else { lut[((k as u32) >> sh).min(1023) as usize] }).collect()
    };
    let first = picture(&mut ui, 4);
    for n in [1, 7, 1, 2, 16, 1, 4] {
        let p = picture(&mut ui, n);
        let worst = first.iter().zip(&p).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(worst <= 3, "{} frames: brightness off by {}", n, worst);
    }
    // the plain grade (longer persistence) is what pumped: one frame after sixteen is graded against 16 times its
    // counts
    ui.grade.frames = 0;
    ui.m.persist = 1;
    let _ = picture(&mut ui, 16);
    ui.grade.frames = 0;
    ui.grade_refs();
    let bright = |ui: &super::Ui| ui.grade.vmax[0];
    let before = bright(&ui);
    ui.ph.clear();
    ui.ph.draw_ex(0, &ui.m.clone(), (tdiv, 0.0), -window / 2.0, window / 3600.0, &v, 24, None, false);
    ui.grade_refs();
    assert!(bright(&ui) > ui.ph.map[0].iter().copied().max().unwrap() as u32 * 8, "{}", before);
}

/// Measurements in the background (the frame loop on the unit) while a new record comes every frame: their results
/// still arrive (0.1.7 to 0.1.10 dropped each one as older than the record shown, and the values froze in Run).
#[test]
fn background_measurements_follow_a_running_record() {
    let mut u = crate::ui::testkit::demo_ui();
    u.measure_in_background();
    let n = |u: &super::Ui| u.mstat.iter().map(|s| s.n).max().unwrap_or(0);
    for _ in 0..120 {
        u.step(1.0 / 30.0);
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    assert!(n(&u) >= 20, "{} results taken in 120 frames", n(&u));
}
// Break it catches: a hardware recording that starts without a new number for the thread, stops on the count of
// records that reached the screen instead of the thread's read-back, browses the RAM history instead of playing
// the segment from the FPGA, or loses the stamps in the status line and the table. The thread is played by hand.
#[test]
fn hardware_segments_record_in_the_thread_and_browse_on_demand() {
    use super::*;
    use crate::board::SegPhase;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap();
    (ui.m.segments, ui.m.seg_hw, ui.m.seg_ivl) = (100, true, 1e-3);
    ui.m.run = RunState::Run;
    ui.step(0.02);
    {
        let g = shared.lock().unwrap();
        assert_eq!((g.seg.run, g.seg.running), (1, true));
        let w = g.want.as_ref().unwrap();
        assert_eq!(w.segments, Some(oos_hal::dho::seq::Segments { n: 100, interval: 1e-3f32 as f64 }));
    }
    // the thread records: no frame, the scope keeps running and shows the progress
    {
        let mut g = shared.lock().unwrap();
        (g.seg.done_run, g.seg.phase, g.seg.total, g.seg.recorded) = (1, SegPhase::Recording, 100, 40);
    }
    for _ in 0..30 {
        ui.step(0.02);
    }
    assert_eq!(ui.m.run, RunState::Run);
    assert_eq!(ui.seg.view.unwrap().recorded, 40);
    // read back: 100 stamps 10 us apart, the frames went through the queue; done once the queue is empty
    {
        let mut g = shared.lock().unwrap();
        (g.seg.phase, g.seg.recorded, g.seg.read) = (SegPhase::Done, 100, 100);
        g.seg.times = (0..100).map(|k| Some(k as f64 * 10e-6)).collect();
    }
    ui.step(0.02);
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.toast.as_ref().unwrap().0, "100 segments recorded");
    // browse: back from the newest asks the thread for segment 99, then 98
    ui.history_step(-1);
    assert_eq!(shared.lock().unwrap().seg.want, Some(98));
    assert_eq!(ui.seg_label(98), "Segment 99/100 \u{b7} 980 \u{b5}s \u{b7} \u{394}t 10.0 \u{b5}s");
    let v: Vec<f32> = (0..1000).map(|k| ((k as f32) * 0.1).sin()).collect();
    let f = crate::board::Frame {
        seq: 1,
        dt: 1e-9,
        pre: 500,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        acq: (1e9, 1000),
        ..Default::default()
    };
    shared.lock().unwrap().seg.shown = Some((98, f));
    ui.step(0.02);
    assert_eq!(ui.seg.pos, Some(98));
    assert!(ui.last[0].is_some());
    assert!(shared.lock().unwrap().seg.shown.is_none());
    // the table and SCPI have every stamp
    let rows = ui.table_rows(tables::TableKind::Segments);
    assert_eq!(rows.len(), 100);
    assert_eq!((rows[0].text.as_str(), rows[3].ok), ("", true));
    assert!(rows[3].text.starts_with("\u{394}t 10"));
    assert!(ui.seg_times_text().starts_with("0e0,1e-5,2e-5"));
    // RUN again: a new recording number
    ui.m.run = RunState::Run;
    ui.step(0.02);
    assert_eq!(shared.lock().unwrap().seg.run, 2);
    assert!(ui.seg.times.is_empty());
}

// Break it catches: the software limit applied to hardware segments, or the hardware limit to the software history.
#[test]
fn segment_counts_follow_the_limit_of_the_set_up() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared, seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap();
    ui.m.segments = 10;
    assert_eq!(*ui.seg_options().last().unwrap(), SEG_MAX);
    ui.adjust(Item::AcqSegHw, 1);
    // 100 ns/div, 1 channel: 1408 points -> 256 bursts -> 61,440 segments
    let lim = board().seg_max(&ui.m).unwrap();
    assert_eq!(lim, 61_440);
    let o = ui.seg_options();
    assert_eq!(&o[o.len() - 2..], &[50_000, 61_440]);
    for want in ["61440", "Off", "20000", "10"] {
        ui.select_option(Item::AcqSeg, want);
        assert_eq!(ui.value(Item::AcqSeg), want);
    }
    ui.select_option(Item::AcqSeg, "61440");
    // back to software: the history limit
    ui.adjust(Item::AcqSegHw, 1);
    assert_eq!((ui.m.segments, ui.value(Item::AcqSeg)), (SEG_MAX, "1000".to_string()));
    let o = ui.options(Item::AcqSegIvl);
    assert_eq!(o.len(), SEG_IVLS.len());
    for want in o.iter().rev().chain(o.iter()) {
        ui.select_option(Item::AcqSegIvl, want);
        assert_eq!(&ui.value(Item::AcqSegIvl), want);
    }
    assert_eq!(ui.m.seg_ivl, 1.0);
    // roll: no hardware segments
    ui.m.seg_hw = true;
    ui.m.tscale = tscales().iter().position(|&x| x >= 0.2).unwrap();
    assert!(board().seg_max(&ui.m).is_none() && !ui.seg_hw_on());
}

#[test]
fn segment_scpi() {
    use super::*;
    let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
    let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
    q(&mut ui, ":ACQ:SEGM 5000");
    q(&mut ui, ":ACQ:SEGM:HARD ON");
    q(&mut ui, ":ACQuire:SEGMents:INTerval 1e-4");
    assert_eq!((ui.m.segments, ui.m.seg_hw, ui.m.seg_ivl), (5000, true, 1e-4));
    assert_eq!((q(&mut ui, ":ACQ:SEGM?"), q(&mut ui, ":ACQ:SEGM:HARD?")), ("5000".into(), "1".into()));
    assert!(q(&mut ui, ":ACQ:SEGM:INT?").starts_with("1.000000E-04"));
    q(&mut ui, ":ACQ:SEGM:INT -3");
    q(&mut ui, ":ACQ:SEGM OFF");
    assert_eq!((ui.m.segments, ui.m.seg_ivl), (0, 0.0));
    ui.seg.times = vec![Some(0.0), None, Some(2e-3)];
    assert_eq!(q(&mut ui, ":ACQ:SEGM:TIM?"), "0e0,9.9E37,2e-3");
}
/// Fast display path: every queued hardware frame is drawn, the newest also becomes the record.
#[test]
fn fast_path_draws_every_queued_frame() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    // 1 us/div, 12.5 k points at 0.8 ns, a 200 kHz sine through 0 V rising at the trigger point
    let (n, dt, pre) = (12_500usize, 0.8e-9, 6250usize);
    let frame = |seq: u64| {
        let v: Vec<f32> = (0..n)
            .map(|k| (2.0 * std::f64::consts::PI * 200e3 * (k as f64 - pre as f64 + 0.3) * dt).sin() as f32)
            .collect();
        crate::board::Frame {
            seq,
            dt,
            pre,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            roll: false,
            envelope: false,
            scan: false,
            partial: false,
            acq: (1.25e9, n),
            stamp: 0,
            la: None,
            fixed_ti: None,
            hw_avg: false,
            basic: Default::default(),
        }
    };
    shared.lock().unwrap().queue.extend((1..=10).map(frame));
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    ui.m.trig_level = 0.0;
    ui.m.ch[0].offset = 0.0;
    assert!(ui.fast_ok());
    ui.step(0.05);
    assert_eq!(ui.wfm_total, 10);
    assert!(shared.lock().unwrap().queue.is_empty());
    assert_eq!(ui.m.status, TrigStatus::Triggered);
    // the zero crossing sits at the centre of the plot in every frame: the centre column is lit at the middle row
    let (w, h) = (ui.ph.w, ui.ph.h);
    let lit: Vec<usize> = (0..h).filter(|&r| ui.ph.map[0][r * w + w / 2] > 0).collect();
    assert!(lit.iter().any(|&r| (r as i32 - h as i32 / 2).abs() <= 2), "{:?}", lit);
    // averaging needs every record: back to the record path
    ui.m.acq = AcqMode::Average;
    assert!(!ui.fast_ok());
}

/// The fast path on several cores draws the same maps as one frame after the other, dense and sparse, four
/// channels; with it on, the record loop takes one frame a screen and leaves the rest of the queue to it.
#[test]
fn fast_path_on_several_cores_draws_as_one_by_one() {
    use super::*;
    for &(tdiv, n, dt) in &[(1e-6, 12_500usize, 0.8e-9), (1e-7, 1_000, 3.2e-9)] {
        let pre = n / 2;
        let frame = |seq: u64| {
            let ch: [Vec<f32>; NCH] = std::array::from_fn(|c| {
                (0..n)
                    .map(|k| {
                        (2.0 * std::f64::consts::PI
                            * 2e6
                            * (c + 1) as f64
                            * (k as f64 - pre as f64 + 0.1 * seq as f64)
                            * dt)
                            .sin() as f32
                    })
                    .collect()
            });
            crate::board::Frame {
                seq,
                dt,
                pre,
                ch,
                roll: false,
                envelope: false,
                scan: false,
                partial: false,
                acq: (1.0 / dt, n),
                stamp: 0,
                la: None,
                fixed_ti: None,
                hw_avg: false,
                basic: Default::default(),
            }
        };
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
        ui.m.trig_level = 0.0;
        for c in 0..NCH {
            ui.m.ch[c].on = true;
            ui.m.ch[c].offset = 0.0;
        }
        assert!(ui.fast_ok());
        let frames: Vec<_> = (1..=40).map(frame).collect();
        let (w, h) = (ui.ph.w, ui.ph.h);
        let mut want = crate::wave::Phosphor::new(w, h);
        for f in &frames {
            let f = std::sync::Arc::new(f.clone());
            let p = fast_plan(&ui.m, w, h, &f, [false; NCH]);
            for c in 0..NCH {
                fast_deposit(&ui.m, w, h, c, &mut want.map[c], &f, &p.plan);
            }
        }
        // with the helpers, and on the frame loop's thread alone (batch mode while the deposit workers have batches)
        for helpers in [crate::fastdraw::helpers(), 0] {
            ui.ph.clear();
            assert_eq!(ui.fast_draw_all(frames.clone(), helpers), 40);
            for c in 0..NCH {
                assert!(want.map[c].iter().any(|&x| x > 0), "{} s/div CH{}: nothing drawn", tdiv, c + 1);
                assert!(ui.ph.map[c] == want.map[c], "{} s/div CH{} helpers {}: maps differ", tdiv, c + 1, helpers);
            }
        }
        // through `step`: 41 queued frames, 40 by the fast path, the newest as the record; nothing left
        shared.lock().unwrap().queue.extend((1..=41).map(frame));
        // the take handed to the thread is the one before the step: the step then adapts it to the time it
        // measured, which under load (tests in parallel) is less
        let take = ui.fast_max() + 1;
        ui.step(0.05);
        assert_eq!(ui.wfm_total, 41);
        assert_eq!(ui.records, 1);
        assert!(shared.lock().unwrap().queue.is_empty());
        // the thread is told how many of a batch to decode; the record path wants them all
        assert_eq!(shared.lock().unwrap().take, take);
        ui.m.acq = AcqMode::Average;
        ui.step(0.05);
        assert_eq!(shared.lock().unwrap().take, 0);
        ui.m.acq = AcqMode::Normal;
    }
}

/// One channel in bands of rows on several cores, lines and dots: the same map as the whole map at once.
#[test]
fn fast_path_bands_draw_as_the_whole_map() {
    use super::*;
    for &(tdiv, n, dt) in &[(1e-6, 12_500usize, 0.8e-9), (1e-7, 1_000, 3.2e-9)] {
        for dots in [false, true] {
            let pre = n / 2;
            let frames: Vec<crate::board::Frame> = (1..=24u64)
                .map(|seq| {
                    // a sine with a frame-dependent phase and a spike, off the screen at the top in some frames
                    let mut v: Vec<f32> = (0..n)
                        .map(|k| {
                            3.0 * (2.0
                                * std::f64::consts::PI
                                * 1.3e6
                                * (k as f64 - pre as f64 + 0.37 * seq as f64)
                                * dt)
                                .sin() as f32
                        })
                        .collect();
                    v[pre + 7] = if seq % 3 == 0 { 9.0 } else { -1.5 };
                    crate::board::Frame {
                        seq,
                        dt,
                        pre,
                        ch: [v, Vec::new(), Vec::new(), Vec::new()],
                        roll: false,
                        envelope: false,
                        scan: false,
                        partial: false,
                        acq: (1.0 / dt, n),
                        stamp: 0,
                        la: None,
                        fixed_ti: None,
                        hw_avg: false,
                        basic: Default::default(),
                    }
                })
                .collect();
            let mut ui = Ui::new(Source::Hw { shared: Default::default(), seen: 0 });
            ui.m.tscale = tscales().iter().position(|&x| (x as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
            ui.m.trig_level = 0.0;
            ui.m.ch[0].offset = 0.0;
            ui.m.dots = dots;
            let (w, h) = (ui.ph.w, ui.ph.h);
            let mut want = vec![0u16; w * h];
            for f in &frames {
                let f = std::sync::Arc::new(f.clone());
                let p = fast_plan(&ui.m, w, h, &f, [false; NCH]);
                fast_deposit(&ui.m, w, h, 0, &mut want, &f, &p.plan);
            }
            // with the helpers, and on the frame loop's thread alone (batch mode while the deposit workers have
            // batches)
            for helpers in [crate::fastdraw::helpers(), 0] {
                ui.ph.clear();
                ui.fast_draw_all(frames.clone(), helpers);
                let what = format!("{} s/div dots {} helpers {}", tdiv, dots, helpers);
                assert!(want.iter().filter(|&&x| x > 0).count() > w / 2, "{}: little drawn", what);
                assert!(ui.ph.map[0] == want, "{}: maps differ", what);
            }
        }
    }
}

/// Statistics of the voltage measurements over every frame of the fast path, not only over the records; the
/// values of one frame equal what the measurements give on it.
#[test]
fn statistics_take_every_frame_of_the_fast_path() {
    use super::*;
    let (n, dt, pre) = (12_500usize, 0.8e-9, 6250usize);
    // frame k: a sine of amplitude 1 + k/10 around 0.25 V
    let frame = |seq: u64| {
        let a = 1.0 + seq as f64 / 10.0;
        let v: Vec<f32> = (0..n)
            .map(|k| (0.25 + a * (2.0 * std::f64::consts::PI * 2e6 * (k as f64 - pre as f64) * dt).sin()) as f32)
            .collect();
        crate::board::Frame {
            seq,
            dt,
            pre,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            roll: false,
            envelope: false,
            scan: false,
            partial: false,
            acq: (1.0 / dt, n),
            stamp: 0,
            la: None,
            fixed_ti: None,
            hw_avg: false,
            basic: Default::default(),
        }
    };
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    ui.m.trig_level = 0.0;
    ui.m.ch[0].offset = 0.0;
    ui.m.meas = vec![
        (Meas::Vpp, WaveId::Ch(0)),
        (Meas::Vavg, WaveId::Ch(0)),
        (Meas::Vrms, WaveId::Ch(0)),
        (Meas::StdDev, WaveId::Ch(0)),
        (Meas::Freq, WaveId::Ch(0)),
    ];
    ui.m.meas2.clear();
    // the first record sets up the rows
    shared.lock().unwrap().queue.push_back(frame(0));
    ui.step(0.05);
    assert_eq!(ui.mstat.len(), 5);
    assert_eq!(ui.mstat[0].n, 1);
    // 20 more: 19 by the fast path, the newest as the record
    shared.lock().unwrap().queue.extend((1..=20).map(frame));
    ui.step(0.05);
    assert_eq!(ui.mstat[0].n, 21, "Pk-pk over every frame");
    assert_eq!(ui.mstat[4].n, 2, "Freq only over the records");
    // the largest Pk-pk is that of frame 20 (amplitude 3), the smallest that of frame 0
    assert!(
        (ui.mstat[0].max - 6.0).abs() < 0.01 && (ui.mstat[0].min - 2.0).abs() < 0.01,
        "{} {}",
        ui.mstat[0].min,
        ui.mstat[0].max
    );
    // a frame's fold gives the values of the measurements on the same samples
    let f = frame(7);
    let fo = crate::fastdraw::Fold::of(&f.ch[0][1000..9000]);
    let an = oos_dsp::measure::analyze(&f.ch[0][1000..9000], dt);
    for k in [Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vavg, Meas::Vrms, Meas::StdDev, Meas::VrmsAc] {
        let (a, b) = (fo.get(k).unwrap(), an.get(k));
        assert!((a - b).abs() < 1e-5, "{:?}: fold {} measure {}", k, a, b);
    }
    assert!(fo.get(Meas::Freq).is_none());
}

// Break it catches: the fast path folding Mean, RMS, Std dev and AC RMS of min/max pairs into the statistics
// (deep pairs: the value shown is NaN while the statistics count pair values), or anything of live-compression
// pairs (only their raw newest frame counts), where the deposit workers (`framestats::fold_frame`) leave them out
#[test]
fn statistics_of_the_fast_path_leave_out_what_pairs_cannot_give() {
    use super::*;
    crate::deepmeas::force(Some(false));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    ui.m.trig_level = 0.0;
    ui.m.ch[0].offset = 0.0;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0)), (Meas::Vmax, WaveId::Ch(0)), (Meas::StdDev, WaveId::Ch(0))];
    ui.m.meas2.clear();
    let n = 20_000usize;
    let dt = 1e-5 * 1.2 / n as f64;
    // a fifth of the time high: the mean (-0.6) is far from that of the pairs (0)
    let frame = |seq: u64, envelope: bool, depth: usize| {
        let v: Vec<f32> = (0..n).map(|k| if (k + n / 2) % 1000 < 200 { 1.0 } else { -1.0 }).collect();
        let ch = [v, Vec::new(), Vec::new(), Vec::new()];
        crate::board::Frame { seq, dt, pre: n / 2, ch, envelope, acq: (1.0 / dt, depth), ..Default::default() }
    };
    let counts = |ui: &Ui| ui.mstat.iter().map(|s| s.n).collect::<Vec<_>>();
    // deep pairs: the newest is the record (Max measured), the fast path draws the other nine
    shared.lock().unwrap().queue.push_back(frame(0, true, 10_000_000));
    ui.step(0.05);
    assert_eq!(counts(&ui), [0, 1, 0]);
    shared.lock().unwrap().queue.extend((1..=10).map(|s| frame(s, true, 10_000_000)));
    ui.step(0.05);
    assert_eq!(counts(&ui), [0, 11, 0], "Max of every frame, no Mean or Std dev of pairs");
    // live-compression pairs: a raw record, then pair frames; only the raw one counts
    ui.clear_display();
    ui.m.run = RunState::Run;
    shared.lock().unwrap().queue.push_back(frame(11, false, n));
    ui.step(0.05);
    assert_eq!(counts(&ui), [1, 1, 1]);
    shared.lock().unwrap().queue.extend((12..=21).map(|s| frame(s, true, n)));
    ui.step(0.05);
    assert_eq!(counts(&ui), [1, 1, 1], "values of live-compression pairs counted");
    crate::deepmeas::force(None);
}

/// The fast path takes what fits into its time budget: the newest frames, the older ones are counted as dropped.
#[test]
fn fast_path_keeps_to_its_budget() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let frame = |seq: u64| crate::board::Frame {
        seq,
        dt: 0.8e-9,
        pre: 50,
        ch: [vec![0.0; 100], Vec::new(), Vec::new(), Vec::new()],
        ..Default::default()
    };
    shared.lock().unwrap().queue.extend((1..=100).map(frame));
    let mut src = Source::Hw { shared: shared.clone(), seen: 0 };
    let got = src.drain_newest(30);
    assert_eq!(got.iter().map(|f| f.seq).collect::<Vec<_>>(), (70..=99).collect::<Vec<_>>());
    let g = shared.lock().unwrap();
    assert_eq!((g.queue.len(), g.queue[0].seq, g.dropped), (1, 100, 69));
    drop(g);
    let mut ui = crate::ui::testkit::demo_ui();
    assert_eq!(ui.fast_max(), crate::board::QUEUE_MAX);
    // nothing else in the screen: 28 ms; a slow screen leaves at least 5
    assert_eq!(ui.fast_budget(), 28.0);
    ui.times.render = 40.0;
    assert_eq!(ui.fast_budget(), 5.0);
    // the take settles where the time meets the budget, with a fixed 3 ms a screen and 0.3 ms a frame
    ui.times.render = 0.0;
    // a queue always fuller than the take, 3 ms a screen and 0.3 ms a frame: settles where the time meets 28 ms
    let mut take = crate::board::QUEUE_MAX as f32;
    for _ in 0..40 {
        let n = (take as usize).clamp(4, crate::board::QUEUE_MAX);
        take = fast_take_next(take, n, 3.0 + 0.3 * n as f32, 28.0);
    }
    assert!((take - 83.3).abs() < 1.0, "{}", take);
    // from below, too
    let mut low = 4.0;
    for _ in 0..40 {
        let n = (low as usize).clamp(4, crate::board::QUEUE_MAX);
        low = fast_take_next(low, n, 3.0 + 0.3 * n as f32, 28.0);
    }
    assert!((low - 83.3).abs() < 1.0, "{}", low);
    // a screen with a few frames (a burst still to come) leaves the take alone; one over the budget lowers it
    assert_eq!(fast_take_next(take, 6, 4.8, 28.0), take);
    assert!(fast_take_next(take, 6, 40.0, 28.0) < take);
}
// Break it catches: an FPGA-averaged frame blended once more in software, the software average lost for frames
// the FPGA did not average, no clear request on CLEAR / RUN / SINGLE, hardware averaging where the board falls back
// (roll, a count the FPGA does not take, the switch off).
#[test]
fn hardware_average_bypasses_the_blend_and_restarts() {
    use super::*;
    use oos_hal::dho::acq::Mode;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    // 100 us/div, Auto depth: 62.5 MSa/s
    ui.m.tscale = tscales().iter().position(|&x| (x - 100e-6).abs() < 1e-9).unwrap();
    (ui.m.acq, ui.m.avg_count, ui.m.avg_hw) = (AcqMode::Average, 16, true);
    ui.m.run = RunState::Run;
    let flat = |seq: u64, v: f32, hw_avg: bool| crate::board::Frame {
        seq,
        dt: 0.8e-9,
        pre: 6250,
        ch: [vec![v; 12_500], Vec::new(), Vec::new(), Vec::new()],
        acq: (1.25e9, 12_500),
        hw_avg,
        ..Default::default()
    };
    let mid = |ui: &Ui| {
        let c = &ui.last[0].as_ref().unwrap().ch[0];
        c[c.len() / 2]
    };
    ui.step(0.02);
    let c0 = {
        let g = shared.lock().unwrap();
        assert_eq!(g.want.as_ref().unwrap().mode, Mode::Average { count: 16 });
        g.avg_clear
    };
    assert!(board().avg_hw(&ui.m));
    assert!(ui.items(MenuId::Acquire).contains(&Item::AcqAvgHw));
    for (k, v) in [1.0, 0.0].into_iter().enumerate() {
        shared.lock().unwrap().queue.push_back(flat(k as u64 + 1, v, true));
        ui.step(0.02);
    }
    assert_eq!(mid(&ui), 0.0);
    // a frame the FPGA did not average is blended: 0 + (1 - 0) / 16
    shared.lock().unwrap().queue.push_back(flat(3, 1.0, false));
    ui.step(0.02);
    assert!((mid(&ui) - 1.0 / 16.0).abs() < 1e-6, "{}", mid(&ui));
    // CLEAR, then STOP and RUN, then SINGLE: each a new clear request
    ui.clear_display();
    ui.step(0.02);
    let c1 = shared.lock().unwrap().avg_clear;
    assert!(c1 > c0);
    ui.m.run = RunState::Stop;
    ui.step(0.02);
    ui.m.run = RunState::Run;
    ui.step(0.02);
    let c2 = shared.lock().unwrap().avg_clear;
    assert!(c2 > c1);
    ui.m.trig_mode = TrigMode::Single;
    ui.step(0.02);
    assert!(shared.lock().unwrap().avg_clear > c2);
    ui.m.trig_mode = TrigMode::Auto;
    // fall-backs to the software average
    ui.m.avg_count = 24;
    assert!(!board().avg_hw(&ui.m));
    ui.m.avg_count = 16;
    ui.m.avg_hw = false;
    assert!(!board().avg_hw(&ui.m));
    ui.m.avg_hw = true;
    ui.m.tscale = tscales().iter().position(|&x| x >= 0.2).unwrap();
    assert!(!board().avg_hw(&ui.m));
    // 1 us/div at 1.25 GSa/s: the FPGA averages at every rate (0x1004 bit 2, B1)
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    assert!(board().avg_hw(&ui.m));
}

/// Batch mode at 1 ms/div (unit 2026-11-18: half the records reached the picture): batches of one frame, 38 a second
/// against 29 screens, through the queue to the frame loop: every record is used, as many as were queued, at 1 and 4
/// channels. The frame loop's part; whether a batch holds one frame or two (frame 0 left out, issue #5) is the
/// acquisition thread's.
#[test]
fn batch_mode_uses_every_record_at_1_ms_per_div() {
    use super::*;
    for mask in [1u32, 15] {
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.measure_in_background();
        ui.m.run = RunState::Run;
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-6).unwrap();
        ui.m.trig_level = 0.0;
        for c in 0..NCH {
            (ui.m.ch[c].on, ui.m.ch[c].offset) = (mask & (1 << c) != 0, 0.0);
        }
        // a frame as the board decodes it at 1 ms/div (100 k points a channel), a sine on each channel on
        let s = crate::board::dho::hw::settings(&ui.m, false);
        let p = oos_hal::dho::hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
        let mut f = crate::board::dho::hw::decode(&vec![0u8; oos_hal::dho::seq::frame_bytes(&s, &p)], &s, &p, 0);
        for ch in f.ch.iter_mut() {
            let n = ch.len();
            for (k, v) in ch.iter_mut().enumerate() {
                *v = 0.9 * ((k as f64 - n as f64 / 2.0) / n as f64 * 40.0 * std::f64::consts::TAU).sin() as f32;
            }
        }
        f.basic = std::array::from_fn(|c| crate::board::BasicStats::of(&f.ch[c]));
        ui.step(1.0 / 30.0);
        let (w0, mut fed) = (ui.wfm_total, 0u64);
        for step in 0..87u64 {
            // 38 frames in 29 screens: one or two a screen
            for _ in 0..(38 * (step + 1) / 29 - 38 * step / 29) {
                fed += 1;
                shared.lock().unwrap().queue.push_back(crate::board::Frame { seq: fed, ..f.clone() });
            }
            ui.step(1.0 / 30.0);
        }
        let left = shared.lock().unwrap().queue.len() as u64;
        assert_eq!(ui.wfm_total - w0 + left, fed, "mask {:#x}: records used", mask);
    }
}

// Break it catches: a changed setting while running emptied the picture at the next step, with or without a record in
// it. At a slow time base or with a Normal sweep waiting for its trigger the screen stayed empty until the next record,
// so every step of the trigger level looked like a trigger that had stopped working (#280, a DHO804 on 0.1.13).
#[test]
fn a_setting_changed_while_running_keeps_the_picture_until_the_next_record() {
    use super::*;
    for persist in [0, PERSIST.len() - 1] {
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        ui.m.persist = persist;
        ui.m.trig_level = 0.15;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas.clear();
        ui.m.meas2.clear();
        let push = |ui: &Ui, seq: u64, high: usize| {
            let window = ui.m.time_per_div() as f64 * HDIV as f64;
            let n = 100_000usize;
            let v: Vec<f32> = (0..n).map(|k| if (k / high) % 2 == 1 { 0.3 } else { 0.0 }).collect();
            shared.lock().unwrap().queue.push_back(crate::board::Frame {
                seq,
                stamp: ui.m.hw_stamp,
                dt: window * 1.2 / n as f64,
                pre: n / 2,
                ch: [v, Vec::new(), Vec::new(), Vec::new()],
                ..Default::default()
            });
        };
        // the first step publishes the settings; then one record, a 0.3 V square
        ui.step(0.033);
        push(&ui, 1, 4000);
        ui.step(0.033);
        let picture = ui.ph.map[0].clone();
        assert!(picture.iter().any(|&c| c != 0), "the record is in the picture");
        // the trigger level moves: steps without a record keep the picture
        ui.adjust(Item::TrigLevel, 1);
        for _ in 0..5 {
            ui.step(0.033);
            assert!(ui.ph.map[0] == picture, "persistence {persist}: the picture left with the setting");
        }
        // the next record replaces it: the old trace is gone, also with infinite persistence
        push(&ui, 2, 1000);
        ui.step(0.033);
        let next = ui.ph.map[0].clone();
        assert!(next.iter().any(|&c| c != 0), "persistence {persist}: the new record is drawn");
        assert!(next != picture, "persistence {persist}: the new record is the picture");
    }
}
