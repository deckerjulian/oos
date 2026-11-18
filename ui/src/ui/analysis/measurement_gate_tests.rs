// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the measurement gate and of the background analysis of long channels and references: the half-open
//! screen gate in physical time, the cache of a reference's analysis, and the one worker with its stale results.
//! New tests of `measure.rs` go here.

use super::*;

/// Actual user file only, never part of unattended tests or CI.
#[test]
#[ignore = "requires OOS_GATE_WAVEFORM pointing at a supplied large waveform"]
fn actual_large_reference_full_record_cache_latency_and_memory() {
    let path = std::env::var("OOS_GATE_WAVEFORM").expect("supply the actual read-only waveform path");
    let load = std::time::Instant::now();
    let bytes = std::fs::read(&path).unwrap();
    let traces = crate::store::parse_wave(&bytes, &path).unwrap();
    drop(bytes);
    let mut ui = crate::ui::testkit::demo_ui();
    ui.commit_waveform_traces(traces, &path, true).unwrap();
    ui.set_offline(true);
    ui.m.run = RunState::Stop;
    let points = ui.refs[0].as_ref().unwrap().v.len();
    assert!(points <= crate::store::MAX_WAVEFORM_SAMPLES);
    let loaded = load.elapsed();
    let scan = std::time::Instant::now();
    ui.set_measurement_full_record(true);
    let dispatch = scan.elapsed();
    let mut worst_tick = std::time::Duration::ZERO;
    while ui.measurement_busy() {
        let tick = std::time::Instant::now();
        ui.step(0.0);
        worst_tick = worst_tick.max(tick.elapsed());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let full_scan = scan.elapsed();
    let (first, _) = ui.reference_measurement(0).unwrap();
    let cached = std::time::Instant::now();
    for k in 0..100 {
        ui.m.zdelay += 1e-9;
        ui.m.delay += 1e-9;
        ui.measure_refs();
        let (analysis, _) = ui.reference_measurement(0).unwrap();
        assert!(Arc::ptr_eq(&first, &analysis), "pan {k} rescanned the immutable full record");
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), first.get(Meas::Vpp));
    }
    println!("file={path} points={points} load+initial_screen={loaded:?} full_dispatch={dispatch:?} full_result_ready={full_scan:?} worst_event_tick={worst_tick:?} cached_100_pan_readouts={:?}", cached.elapsed());
}

fn sources() -> Ui {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.run = RunState::Stop;
    ui.m.zoom = false;
    ui.m.tscale = tscales().iter().position(|&v| (v - 100e-9).abs() < 1e-12).unwrap();
    ui.m.tfine = 1.0;
    ui.m.delay = 500e-9;
    let mut rec = Ui::blank_record();
    rec.dt = 1e-9;
    rec.ch[0] = (0..4000)
        .map(|i| {
            let amplitude = if i < 2000 { 1.0 } else { 3.0 };
            if i % 100 < 50 {
                amplitude
            } else {
                -amplitude
            }
        })
        .collect();
    ui.last[0] = Some(rec);
    ui.refs[0] = Some(RefWave {
        label: "2ns reference".into(),
        ..RefWave::test(
            (0..2000)
                .map(|i| {
                    let amplitude = if i < 1000 { 2.0 } else { 4.0 };
                    if i % 50 < 25 {
                        amplitude
                    } else {
                        -amplitude
                    }
                })
                .collect::<Vec<_>>(),
            2e-9,
            0.0,
        )
    });
    ui
}

#[test]
fn screen_gate_is_half_open_at_f32_200ps_boundaries_with_nonzero_origins() {
    let dt = 20e-12;
    for center in [0.0, 2f64.powi(-20)] {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.run = RunState::Stop;
        ui.m.zoom = false;
        ui.m.tscale = 0;
        ui.m.tfine = 200e-12f32 / tscales()[0];
        ui.m.delay = center as f32;
        assert_eq!(ui.view().0, 200e-12f32);
        let t0 = center - 1e-6;
        let (lo, hi) = (49_950, 50_050);
        assert_eq!(ui.measurement_bounds(100_000, t0, dt), (lo, hi));
        let mut values = vec![0.0; 100_000];
        for (i, value) in values[lo..hi].iter_mut().enumerate() {
            *value = if i % 2 == 0 { -0.02 } else { 0.02 };
        }
        values[lo] = -0.08; // exact left border belongs to the gate
        values[hi - 1] = 0.04;
        values[lo - 1] = -0.6; // the old floor bound leaked this in
        values[hi] = 0.8; // the right border must never be included
        let expected_rms = (values[lo..hi].iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / 100.0).sqrt();
        let mut rec = Ui::blank_record();
        rec.t0 = t0;
        rec.dt = dt;
        rec.ch[0] = values.clone();
        ui.last[0] = Some(rec);
        ui.refs[0] = Some(RefWave { label: "border outliers".into(), vpd: 0.02, ..RefWave::test(values, dt, t0) });
        ui.measure_last();
        for source in [WaveId::Ch(0), WaveId::Ref(0)] {
            assert!((ui.fact_meas(Meas::Vpp, source) - 0.12).abs() < 1e-8);
            assert!((ui.fact_meas(Meas::Vrms, source) - expected_rms).abs() < 1e-12);
        }
    }
}

#[test]
fn screen_gate_keeps_real_near_boundary_samples_without_rounding_the_view() {
    let mut ui = crate::ui::testkit::demo_ui();
    ui.m.zoom = false;
    ui.m.tscale = 0;
    ui.m.tfine = 200e-12f32 / tscales()[0];
    ui.m.delay = 0.0;
    let dt = 20e-12;
    // Two hundred-thousandths of a sample is a real interior shift, beyond
    // the bounded numerical fringe. Do not round either physical endpoint.
    let shift = 2e-5 * dt;
    assert_eq!(ui.measurement_bounds(100_000, -1e-6 - shift, dt), (49_951, 50_051));
    assert_eq!(ui.measurement_bounds(100_000, -1e-6 + shift, dt), (49_950, 50_050));
    assert_eq!(ui.m.delay, 0.0, "index snapping must not modify the selected view");
}

#[test]
fn screen_and_full_record_gate_mixed_sources_in_physical_time() {
    let mut ui = sources();
    assert!(!ui.m.meas_full_record);
    assert!(!ui.record_mode(), "this checks mixed live and reference overlays");
    ui.measure_last();
    assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 2.0);
    assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 4.0);
    ui.m.delay = 3e-6;
    ui.refresh_stopped_measurements();
    assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 6.0);
    assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 8.0);
    ui.set_measurement_full_record(true);
    ui.m.delay = 500e-9;
    ui.refresh_stopped_measurements();
    assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 6.0);
    assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 8.0);
    ui.set_measurement_full_record(false);
    assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 2.0);
    assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 4.0);
}

#[test]
fn long_reference_cache_reuses_analysis_and_invalidates_accepted_sim_settings() {
    let mut ui = sources();
    ui.m.meas_full_record = true;
    ui.session_is_sim = true;
    ui.m.sim_slot = 0;
    ui.replay.sim_revision = 1;
    let (first, _) = ui.reference_measurement(0).unwrap();
    ui.m.delay = 3e-6;
    ui.measure_refs();
    let (pan, _) = ui.reference_measurement(0).unwrap();
    assert!(Arc::ptr_eq(&first, &pan), "full-record panning and capture ticks must reuse the exact analysis");
    assert!(Arc::ptr_eq(&pan, &ui.ref_ana[0]), "every readout shares the cached result");
    ui.refs[0].as_mut().unwrap().vpd = 2.0;
    let (scale, _) = ui.reference_measurement(0).unwrap();
    assert!(!Arc::ptr_eq(&pan, &scale));
    ui.m.meas_lpct = 0.2;
    ui.m.meas_hpct = 0.8;
    let (thresholds, _) = ui.reference_measurement(0).unwrap();
    assert!(!Arc::ptr_eq(&scale, &thresholds));
    // A worker may fill the existing Vec: pointer/length alone are insufficient.
    for value in std::sync::Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v) {
        *value *= 2.0;
    }
    ui.replay.sim_revision += 1;
    let (fresh, _) = ui.reference_measurement(0).unwrap();
    assert!(!Arc::ptr_eq(&thresholds, &fresh));
    assert_eq!(fresh.get(Meas::Vpp), 16.0);
}

#[test]
fn measurement_gate_is_visible_selectable_and_keeps_old_setups_on_screen() {
    let mut ui = sources();
    ui.open_menu(MenuId::MeasAdd);
    let row = ui.dlg_layout().unwrap().rows.into_iter().find(|row| row.it == Item::MeasGate).unwrap();
    ui.tap_menu(row.ctrl.x + row.ctrl.w * 3 / 4, row.ctrl.y + row.ctrl.h / 2);
    assert!(ui.m.meas_full_record);
    assert_eq!(ui.value(Item::MeasGate), "Full record");
    let text = crate::store::setup_text(&ui.m);
    assert!(crate::store::parse_setup(&text).unwrap().meas_full_record);
    let old = text.lines().filter(|line| !line.starts_with("meas_full_record =")).collect::<Vec<_>>().join("\n");
    assert!(!crate::store::parse_setup(&old).unwrap().meas_full_record);
}

fn large() -> Ui {
    let mut ui = sources();
    ui.set_offline(true);
    ui.refs[0].as_mut().unwrap().v =
        (0..400_000).map(|i| if i % 100 < 50 { 1.0 } else { -1.0 }).collect::<Vec<_>>().into();
    ui.m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
    ui
}

/// Until the worker is done; one that never is hangs the test (no deadline a busy computer could miss).
fn pump(ui: &mut Ui) {
    loop {
        ui.step(0.0);
        if !ui.azr.ref_measure_worker.borrow().as_ref().is_some_and(RefMeasureWorker::busy) && !ui.measurement_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn large_full_measurements_are_nonblocking_and_use_one_worker_without_fake_captures() {
    let mut ui = large();
    ui.set_measurement_full_record(true);
    // dispatch does not scan on the event thread: the job is still running and no value is there yet
    assert!(ui.measurement_busy());
    for _ in 0..100 {
        assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
    }
    assert_eq!(
        ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted,
        1,
        "readouts must not queue duplicate jobs"
    );
    assert_eq!(ui.meas_rows()[0].text(), "Computing");
    assert!(ui.workflow_state().contains("\"measurementBusy\":true"));
    let captures = ui.replay.captures;
    ui.mstat = vec![MStat { n: 7, ..Default::default() }];
    pump(&mut ui);
    assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 2.0);
    assert_eq!(ui.replay.captures, captures);
    assert_eq!(ui.mstat[0].n, 7, "analysis completion must preserve acquisition statistics without adding a sample");
    assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted, 1);
}

#[test]
fn stale_large_results_cannot_replace_a_new_source_or_gate() {
    let mut ui = large();
    ui.set_measurement_full_record(true);
    // Replace the source while its immutable worker snapshot is being measured.
    ui.workflow_epoch += 1;
    for value in std::sync::Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v) {
        *value *= 3.0;
    }
    assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
    pump(&mut ui);
    assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 6.0);
    assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted, 2);
    ui.session_is_sim = true;
    ui.m.sim_slot = 0;
    ui.replay.sim_revision += 1;
    assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
    ui.set_measurement_full_record(false);
    assert!(!ui.measurement_busy());
    assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 6.0);
    pump(&mut ui);
    assert!(!ui.m.meas_full_record);
    assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 6.0);
    assert!(ui.workflow_state().contains("\"gate\":\"screen\""));
}

#[test]
fn a_large_screen_gate_runs_in_background_and_discards_stale_pan_results() {
    let mut ui = large();
    ui.refs[0].as_mut().unwrap().v = (0..400_000)
        .map(|i| {
            let amplitude = if i < 200_000 { 3.0 } else { 1.0 };
            if i % 100 < 50 {
                amplitude
            } else {
                -amplitude
            }
        })
        .collect::<Vec<_>>()
        .into();
    ui.m.tscale = tscales().iter().position(|&v| (v - 100e-6).abs() < 1e-10).unwrap();
    ui.m.delay = 400e-6;
    ui.m.meas_full_record = false;
    ui.measure_refs();
    // a zoomed-out Screen gate does not scan on the event thread: busy, no value yet
    assert!(ui.measurement_busy());
    assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
    assert_eq!(ui.meas_rows()[0].text(), "Computing");
    // Pan to a smaller, low-amplitude region before the old job completes.
    ui.m.tscale = tscales().iter().position(|&v| (v - 50e-6).abs() < 1e-10).unwrap();
    ui.m.delay = 700e-6;
    assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 2.0);
    let wanted = ui.reference_measurement_key(0).unwrap().0;
    pump(&mut ui);
    assert!(
        ui.azr.ref_measure[0].borrow().as_ref().is_some_and(|cached| cached.key == wanted),
        "the old broad Screen result cannot replace the panned gate"
    );
    assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 2.0);
    assert_eq!(ui.replay.captures, 0);
}
