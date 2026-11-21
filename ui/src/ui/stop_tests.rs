// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of STOP and SINGLE: the final raw frame of a deep record and its measurements, the record read again in
//! STOP, Clear and RUN after STOP, `*OPC?` after `:STOP`, and the stamps of SINGLE after Default. The other tests
//! of the acquisition are in `acquisition_tests.rs`.

// Break it catches (docs/fork/stop-exact.md): a STOP that keeps the pair record on screen while the measurements
// show an older raw record, a final frame not taken or taken after the stop, frames after it taken, the hardware
// left arming in STOP or held in RUN, a STOP waiting forever, a plain STOP of a shallow record waiting at all
#[test]
fn stop_shows_the_final_raw_frame_of_a_deep_record() {
    use super::*;
    crate::deepmeas::force(Some(true));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    let frame = |seq: u64, envelope: bool, acq: usize, v: Vec<f32>| crate::board::Frame {
        seq,
        dt: window * 1.2 / n as f64,
        pre: n / 2,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        envelope,
        acq: (1e9, acq),
        ..Default::default()
    };
    let raw = |top: f32| -> Vec<f32> { (0..n).map(|i| if (i / 50) % 4 == 0 { top } else { 0.0 }).collect() };
    let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 1.0 }).collect();
    let steps = |ui: &mut Ui, k: usize| {
        for _ in 0..k {
            ui.step(0.02);
        }
    };
    shared.lock().unwrap().queue.push_back(frame(1, true, 10_000_000, pairs.clone()));
    steps(&mut ui, 3);
    assert!(!shared.lock().unwrap().hold);
    // STOP on a pair record: the thread is held and asked for the final frame, the UI waits for it
    ui.m.run = RunState::Stop;
    steps(&mut ui, 2);
    {
        let g = shared.lock().unwrap();
        assert!(g.hold && g.final_req == 1);
    }
    assert!(
        ui.stop.final_until.is_some()
            && ui.last[0].as_ref().is_some_and(|r| r.raw.as_ref().is_some_and(|x| x.frame.envelope))
    );
    // the thread: the last frame of the batch in progress, then the newest frame raw, then the answer
    {
        let mut g = shared.lock().unwrap();
        g.queue.push_back(frame(2, true, 10_000_000, pairs.clone()));
        g.queue.push_back(frame(3, false, 10_000_000, raw(2.0)));
        g.final_ack = 1;
    }
    steps(&mut ui, 2);
    assert!(ui.stop.final_until.is_none());
    let shown = ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope));
    assert_eq!(shown, Some((3, false)), "the final raw frame is the record shown");
    let avg = ui.meas(0, Meas::Vavg);
    assert!((avg - 0.5).abs() < 0.03, "mean of the record shown: {}", avg);
    assert_eq!(ui.m.status, TrigStatus::Stop);
    // frames after it stay unseen in STOP
    shared.lock().unwrap().queue.push_back(frame(4, false, 10_000_000, raw(4.0)));
    steps(&mut ui, 2);
    assert_eq!(ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq), Some(3));
    assert!(shared.lock().unwrap().hold);
    // RUN: armed again
    ui.m.run = RunState::Run;
    steps(&mut ui, 1);
    assert!(!shared.lock().unwrap().hold);
    // no answer: the STOP ends after STOP_FINAL_WAIT anyway
    shared.lock().unwrap().queue.clear();
    shared.lock().unwrap().queue.push_back(frame(5, true, 10_000_000, pairs.clone()));
    steps(&mut ui, 2);
    ui.m.run = RunState::Stop;
    steps(&mut ui, 2);
    assert!(ui.stop.final_until.is_some());
    steps(&mut ui, (STOP_FINAL_WAIT / 0.02) as usize + 2);
    assert!(ui.stop.final_until.is_none() && ui.m.status == TrigStatus::Stop);
    // a shallow record: STOP holds the thread at once and waits for its last record as well (#24), which the thread
    // answers without a raw read
    ui.m.run = RunState::Run;
    steps(&mut ui, 1);
    shared.lock().unwrap().queue.push_back(frame(6, false, 100_000, raw(1.0)));
    steps(&mut ui, 2);
    ui.m.run = RunState::Stop;
    steps(&mut ui, 1);
    assert!(ui.stop.final_until.is_some() && shared.lock().unwrap().hold && shared.lock().unwrap().final_req == 3);
    {
        let mut g = shared.lock().unwrap();
        g.final_ack = g.final_req;
    }
    steps(&mut ui, 1);
    assert!(ui.stop.final_until.is_none() && ui.m.status == TrigStatus::Stop);
    crate::deepmeas::force(None);
}

// Break it catches (docs/fork/stop-exact.md): after the final raw frame of a deep record the view read again in
// STOP (`stop_replay`, min/max pairs of the whole record) replaced the record measured, so Mean, RMS, Std dev and
// AC RMS went to NaN one step after the STOP; a view counted in the statistics; a raw window read for a view far
// zoomed in not measured; the raw record of the STOP not measured again once the view is pairs again
#[test]
fn stop_measures_the_final_raw_frame_while_the_views_are_drawn_from_pairs() {
    use super::*;
    crate::deepmeas::force(Some(true));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Run;
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    let ch1 = WaveId::Ch(0);
    let kinds = [Meas::Vavg, Meas::Vrms, Meas::StdDev, Meas::VrmsAc, Meas::Vmax];
    ui.m.meas = kinds.iter().map(|&k| (k, ch1)).collect();
    ui.m.meas2.clear();
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
    let window = ui.m.time_per_div() as f64 * HDIV as f64;
    let n = 40_000usize;
    let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
        seq,
        dt: window * 1.2 / n as f64,
        pre: n / 2,
        ch: [v, Vec::new(), Vec::new(), Vec::new()],
        envelope,
        acq: (1e9, 10_000_000),
        ..Default::default()
    };
    // what the thread answers for a view in STOP: the stored record read again, trigger at the point given
    let view = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
        fixed_ti: Some((n / 2) as f64),
        ..frame(seq, envelope, v)
    };
    // a square of a quarter duty, 2 V high: mean 0.5, RMS 1, std dev and AC RMS sqrt(0.75)
    let raw: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { 2.0 } else { 0.0 }).collect();
    let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 2.0 }).collect();
    let steps = |ui: &mut Ui, k: usize| {
        for _ in 0..k {
            ui.step(0.02);
        }
    };
    let check = |ui: &Ui, what: &str| {
        let sd = 0.75f64.sqrt();
        for (k, want) in kinds.into_iter().zip([0.5, 1.0, sd, sd, 2.0]) {
            let v = ui.meas(0, k);
            assert!((v - want).abs() < 0.03, "{}: {:?} {} instead of {}", what, k, v, want);
        }
    };
    shared.lock().unwrap().queue.push_back(frame(1, true, pairs.clone()));
    steps(&mut ui, 3);
    ui.m.run = RunState::Stop;
    steps(&mut ui, 2);
    {
        let mut g = shared.lock().unwrap();
        g.queue.push_back(frame(2, false, raw.clone()));
        g.final_ack = 1;
    }
    steps(&mut ui, 2);
    assert!(ui.stop.final_until.is_none());
    check(&ui, "the final raw frame");
    let counted = ui.results.rows[0].n;
    assert!(counted > 0);
    // the view of the STOP read again as pairs: drawn, while the raw frame stays the record measured
    steps(&mut ui, 1);
    assert!(shared.lock().unwrap().stop_view.is_some(), "the UI asks for the view read again");
    shared.lock().unwrap().replay = Some(view(3, true, pairs.clone()));
    steps(&mut ui, 2);
    check(&ui, "the view read as pairs");
    assert_eq!(ui.results.rows[0].n, counted, "the view counted in the statistics");
    let measured = |ui: &Ui| ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq);
    let drawn = |ui: &Ui| ui.stop_picture().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope));
    assert_eq!(measured(&ui), Some(2));
    assert_eq!(drawn(&ui), Some((3, true)), "the pairs read for the view are drawn");
    // zoomed in and out (a new time base clears the statistics, as any setting): the screen part of the raw frame
    for d in [-1, 1] {
        ui.adjust(Item::HScale, d);
        steps(&mut ui, 1);
        shared.lock().unwrap().replay = Some(view(4, true, pairs.clone()));
        steps(&mut ui, 2);
        check(&ui, "a new view read as pairs");
        assert!(ui.results.rows.iter().all(|s| s.n == 0), "the record of the STOP counted for the view");
        assert_eq!(measured(&ui), Some(2));
        assert_eq!(drawn(&ui), Some((4, true)));
    }
    // far zoomed in the thread reads every sample of the view: that window is the record measured, as before
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
    steps(&mut ui, 1);
    shared.lock().unwrap().replay = Some(view(5, false, vec![3.0; n]));
    steps(&mut ui, 2);
    assert_eq!(measured(&ui), Some(5));
    assert_eq!(drawn(&ui), None, "the raw window is drawn as the record shown");
    assert!((ui.meas(0, Meas::Vavg) - 3.0).abs() < 1e-3, "mean of the raw window {}", ui.meas(0, Meas::Vavg));
    // zoomed out again: pairs, measured from the raw frame of the STOP again
    ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
    steps(&mut ui, 1);
    shared.lock().unwrap().replay = Some(view(6, true, pairs.clone()));
    steps(&mut ui, 2);
    assert_eq!((measured(&ui), drawn(&ui)), (Some(2), Some((6, true))));
    check(&ui, "pairs after the raw window");
    assert!(ui.results.rows.iter().all(|s| s.n == 0));
    // RUN: the record of the STOP is let go
    ui.m.run = RunState::Run;
    shared.lock().unwrap().queue.push_back(frame(7, true, pairs.clone()));
    steps(&mut ui, 2);
    assert!(ui.stop.raw.is_none() && ui.stop_picture().is_none());
    crate::deepmeas::force(None);
}

// Break it catches: the stored record read again in STOP (`stop_replay`) counted as a new record in the statistics,
// the trend and the catalog, once at the STOP and once for every view after it
#[test]
fn stop_reads_the_record_again_without_counting_it() {
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
    ui.m.meas = vec![(Meas::Vmax, WaveId::Ch(0))];
    ui.m.meas2.clear();
    let (n, dt) = (12_500usize, 0.8e-9);
    let frame = |seq: u64, top: f32| {
        let v: Vec<f32> = (0..n).map(|k| if (k + n / 2) % 1000 < 200 { top } else { -1.0 }).collect();
        let ch = [v, Vec::new(), Vec::new(), Vec::new()];
        crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
    };
    shared.lock().unwrap().queue.push_back(frame(1, 1.0));
    ui.step(0.05);
    assert_eq!(ui.results.rows[0].n, 1);
    let seq = |ui: &Ui| ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq);
    ui.m.run = RunState::Stop;
    ui.step(0.05);
    // the STOP waits for the thread's last record (#24); the thread holds and answers
    {
        let mut g = shared.lock().unwrap();
        g.final_ack = g.final_req;
    }
    ui.step(0.05);
    ui.step(0.05);
    // the record of the STOP holds the whole acquisition: nothing is read again, it stays as it is (#24: read again,
    // the view was a record of another length and origin, which replaced it between two queries of a client)
    assert!(shared.lock().unwrap().stop_view.is_none(), "the whole record of the STOP read again");
    assert_eq!(seq(&ui), Some(1));
    assert_eq!(ui.results.rows[0].n, 1, "the record of the STOP counted again");
    // a new view: the statistics start over, the record of the STOP is drawn and measured for it from its own
    // samples, not counted
    for k in [1, -1] {
        ui.adjust(Item::HScale, k);
        ui.step(0.05);
        ui.step(0.05);
        assert!(shared.lock().unwrap().stop_view.is_none(), "the whole record of the STOP read again for a view");
        assert_eq!(seq(&ui), Some(1));
        assert!((ui.meas(0, Meas::Vmax) - 1.0).abs() < 1e-3, "Vmax {}", ui.meas(0, Meas::Vmax));
        assert!(ui.results.rows.iter().all(|s| s.n == 0), "the record of the STOP counted for the view");
    }
    crate::deepmeas::force(None);
}

// Break it catches: Clear in STOP releasing the hold (the hold asked for a record shown): the thread armed while
// stopped, overwrote the stored record and filled the queue, which the next RUN took; a source started in STOP
// without its first record
#[test]
fn clear_in_stop_keeps_the_hardware_held() {
    use super::*;
    crate::deepmeas::force(Some(false));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    let (n, dt) = (12_500usize, 0.8e-9);
    let frame = |seq: u64| {
        let ch = [vec![0.5; n], Vec::new(), Vec::new(), Vec::new()];
        crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
    };
    // started in STOP: not held until the first record is here
    ui.m.run = RunState::Stop;
    ui.step(0.05);
    assert!(!shared.lock().unwrap().hold, "a source started in STOP gets its first record");
    shared.lock().unwrap().queue.push_back(frame(1));
    ui.step(0.05);
    ui.step(0.05);
    assert!(ui.last[0].is_some() && shared.lock().unwrap().hold);
    ui.clear_display();
    for _ in 0..3 {
        ui.step(0.05);
    }
    assert!(ui.last[0].is_none() && shared.lock().unwrap().hold, "Clear in STOP released the hardware");
    ui.m.run = RunState::Run;
    ui.step(0.05);
    assert!(!shared.lock().unwrap().hold);
    crate::deepmeas::force(None);
}

// Break it catches: RUN after STOP taking a frame queued before the RUN (the batch in progress at the STOP) as the
// first record of the run, before the thread dropped the queue on its next pass
#[test]
fn run_after_stop_takes_no_frame_from_before() {
    use super::*;
    crate::deepmeas::force(Some(false));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    let (n, dt) = (12_500usize, 0.8e-9);
    let frame = |seq: u64| {
        let ch = [vec![0.5; n], Vec::new(), Vec::new(), Vec::new()];
        crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
    };
    let seq = |ui: &Ui| ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq);
    shared.lock().unwrap().queue.push_back(frame(1));
    ui.step(0.05);
    ui.m.run = RunState::Stop;
    ui.step(0.05);
    assert!(shared.lock().unwrap().hold);
    // the batch in progress at the STOP is read and queued: until the thread holds it is taken, the record of the
    // STOP, the one the thread reads again for the views (#24)
    shared.lock().unwrap().queue.push_back(frame(2));
    ui.step(0.05);
    assert_eq!(seq(&ui), Some(2));
    {
        let mut g = shared.lock().unwrap();
        g.final_ack = g.final_req;
    }
    ui.step(0.05);
    // held: a frame queued after that is no record of the STOP
    shared.lock().unwrap().queue.push_back(frame(3));
    ui.step(0.05);
    assert_eq!(seq(&ui), Some(2));
    ui.m.run = RunState::Run;
    ui.step(0.05);
    assert_eq!(seq(&ui), Some(2), "a frame acquired before the RUN became the first record of the run");
    // what the thread acquires after the RUN is taken
    shared.lock().unwrap().queue.push_back(frame(4));
    ui.step(0.05);
    assert_eq!(seq(&ui), Some(4));
    crate::deepmeas::force(None);
}

// Break it catches (#24): `*OPC?` after `:STOP` answered before the record of the STOP was in place, so a client's
// next queries read the last running record and then the record of the STOP; a RUN while it waits completing it
#[test]
fn opc_after_stop_waits_for_the_record_of_the_stop() {
    use super::scpi_operations::OperationState;
    use super::*;
    crate::deepmeas::force(Some(false));
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
    for c in 1..NCH {
        ui.m.ch[c].on = false;
    }
    let (n, dt) = (12_500usize, 0.8e-9);
    let frame = |seq: u64| {
        let ch = [vec![0.5; n], Vec::new(), Vec::new(), Vec::new()];
        crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
    };
    let ack = || {
        let mut g = shared.lock().unwrap();
        g.final_ack = g.final_req;
    };
    shared.lock().unwrap().queue.push_back(frame(1));
    ui.step(0.05);
    assert_eq!(ui.scpi_operations().state(&ui), OperationState::Complete, "nothing to wait for while running");
    // `:STOP;*OPC?` in one line: the fence is taken before the step has seen the STOP
    ui.m.run = RunState::Stop;
    let fence = ui.scpi_operations();
    assert_eq!(fence.state(&ui), OperationState::Pending);
    ui.step(0.05);
    shared.lock().unwrap().queue.push_back(frame(2));
    ui.step(0.05);
    assert_eq!(fence.state(&ui), OperationState::Pending, "answered before the thread held");
    ack();
    ui.step(0.05);
    assert_eq!(fence.state(&ui), OperationState::Complete);
    assert_eq!(ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq), Some(2));
    // a STOP already settled: nothing to wait for
    assert_eq!(ui.scpi_operations().state(&ui), OperationState::Complete);
    // RUN while a STOP waits: the fence is cancelled, not completed
    ui.m.run = RunState::Run;
    ui.step(0.05);
    shared.lock().unwrap().queue.push_back(frame(3));
    ui.step(0.05);
    ui.m.run = RunState::Stop;
    let fence = ui.scpi_operations();
    ui.step(0.05);
    ui.m.run = RunState::Run;
    ui.step(0.05);
    assert_eq!(fence.state(&ui), OperationState::Cancelled);
    // the thread never answers: after the time limit the STOP stands, but a waiting `*OPC?` is cancelled, not told 1
    shared.lock().unwrap().queue.push_back(frame(4));
    ui.step(0.05);
    ui.m.run = RunState::Stop;
    let fence = ui.scpi_operations();
    for _ in 0..(STOP_FINAL_WAIT / 0.05) as usize + 2 {
        ui.step(0.05);
    }
    assert!(ui.stop.final_until.is_none());
    assert_eq!(fence.state(&ui), OperationState::Cancelled);
    crate::deepmeas::force(None);
}

// Break it catches: the Default key (`*RST`) set the hardware session's stamp back to 0. The next SINGLE counted it up
// to a stamp the acquisition thread had already finished a SINGLE with, so it armed nothing and the SINGLE never ended
// (unit 2026-11-20, the second `*RST` in one oos-ui process)
#[test]
fn default_keeps_the_stamps_of_the_hardware_session() {
    use super::*;
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    let (n, dt) = (12_500usize, 0.8e-9);
    // the stamps of the SINGLEs the thread finished: it arms nothing for a SINGLE whose stamp is one of them
    // (`single_wants_more` in board/dho/hw/settings.rs), so such a SINGLE gets no frame and never ends
    let mut done: Vec<u64> = Vec::new();
    let mut single = |ui: &mut Ui| {
        ui.m.trig_mode = TrigMode::Single;
        ui.m.run = RunState::Run;
        ui.step(0.05);
        let stamp = ui.m.hw_stamp;
        if !done.contains(&stamp) {
            let ch = [vec![0.5; n], Vec::new(), Vec::new(), Vec::new()];
            let f =
                crate::board::Frame { seq: stamp, stamp, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() };
            shared.lock().unwrap().queue.push_back(f);
            done.push(stamp);
        }
        ui.step(0.05);
        ui.step(0.05);
        let seq = ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq);
        let ended = ui.m.run == RunState::Stop && seq == Some(stamp);
        ui.m.trig_mode = TrigMode::Auto;
        ui.m.run = RunState::Stop;
        ui.step(0.05);
        ended
    };
    assert!(single(&mut ui) && single(&mut ui), "SINGLE with its own frame");
    let clear = ui.m.hw_avg_clear;
    ui.key(crate::ui::key::DEFAULT);
    assert!(ui.m.hw_avg_clear >= clear, "the average count went back");
    assert!(single(&mut ui), "the SINGLE after Default never ended");
}
