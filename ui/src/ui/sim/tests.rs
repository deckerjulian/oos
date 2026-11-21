// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of SIM: the menu and its units, generation on the worker and its errors, cancelling and the newest request,
//! play, show and replay, and SIM captures in the main loop. A new test of SIM goes here.

use super::*;

fn ui() -> Ui {
    crate::ui::testkit::demo_ui()
}

/// A wanted scale of NaN: the nearest scale was chosen with `partial_cmp(..).unwrap()`, which panicked; every distance
/// is NaN now and the first scale is taken.
#[test]
fn nearest_scale_of_nan_takes_the_first() {
    assert_eq!((nearest_vdiv(f32::NAN), nearest_tscale(f64::NAN)), (VSCALES[0], 0));
    assert_eq!(nearest_vdiv(0.21), 0.2);
}

#[test]
fn advanced_sim_controls_are_reachable_and_use_explicit_units() {
    let mut u = ui();
    u.open_menu(MenuId::Sim);
    assert!(u.items(MenuId::Sim).contains(&Item::Open(MenuId::SimAdvanced)));
    u.activate(Item::Open(MenuId::SimAdvanced));
    assert_eq!(u.menu, MenuId::SimAdvanced);
    assert_eq!(u.parent, Some(MenuId::Sim));
    assert_eq!(u.menu_root(), MenuId::Sim);
    let items = u.items(MenuId::SimAdvanced);
    for expected in
        [Item::SimSscCenter, Item::SimSscDeviation, Item::SimSscHz, Item::SimDcd, Item::SimCorrRj, Item::SimCorrHz]
    {
        assert!(items.contains(&expected));
    }
    assert!(u.value(Item::SimSscDeviation).contains("p-p"));
    u.adjust(Item::SimSscCenter, 1);
    assert!(u.value(Item::SimSscDeviation).contains("pk"));
    assert!(u.options(Item::SimPat).contains(&"PRBS-13".into()));
    u.m.sim_prbs = 9;
    u.adjust(Item::SimPat, 1);
    assert_eq!(u.sim_lane().prbs, 13);
    let dialog = u.dlg_layout().unwrap();
    for row in &dialog.rows {
        assert!(row.ctrl.x >= dialog.r.x && row.ctrl.x + row.ctrl.w <= dialog.r.x + dialog.r.w);
        assert!(row.ctrl.y >= dialog.r.y && row.ctrl.y + row.ctrl.h <= dialog.r.y + dialog.r.h);
    }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let _ = std::fs::write(format!("{dir}/sim-advanced.png"), crate::store::png_of(&cv));
    }
    let back = dialog.back.unwrap();
    u.tap_menu(back.x + back.w / 2, back.y + back.h / 2);
    assert_eq!(u.menu, MenuId::Sim);
}

#[test]
fn defaults_regenerate_a_stopped_sim_and_reset_advanced_knobs() {
    let mut u = ui();
    u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 3.2 Gen 2").unwrap();
    u.m.apply_sim_preset();
    u.m.sim_dcd_pp = 20e-12;
    u.m.sim_corr_rj = 4e-12;
    u.m.sim_ssc_center = true;
    u.m.sim_ssc_deviation = 0.01;
    u.sim_play_set(true).unwrap();
    let altered = u.refs[0].as_ref().unwrap().v.clone();
    u.sim_play_set(false).unwrap();
    u.activate(Item::SimDefaults);
    assert_eq!(u.m.sim_dcd_pp, 0.0);
    assert_eq!(u.m.sim_corr_rj, 0.0);
    assert!(!u.m.sim_ssc_center);
    assert_eq!(u.m.sim_ssc_deviation, 0.005);
    assert_ne!(u.refs[0].as_ref().unwrap().v, altered);
    assert_eq!(u.m.run, RunState::Stop);
    assert!(!u.sim_playing());
}

#[test]
fn asymmetric_sim_rails_are_centered_without_changing_signal_volts() {
    let mut u = ui();
    u.m.sim_ssc = false;
    u.m.sim_vlow = 1.0;
    u.m.sim_vhigh = 2.0;
    u.m.sim_noise = 0.0;
    u.m.sim_deemph = 0.0;
    u.sim_play_set(true).unwrap();
    let reference = u.refs[0].as_ref().unwrap();
    assert_eq!(reference.offset, -1.5);
    let (low, high) = reference
        .v
        .iter()
        .copied()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), value| (low.min(value), high.max(value)));
    assert!(low > 0.9 && high > 1.9 && high < 2.1, "samples retain their actual voltage: {low}..{high}");
    assert!((low + reference.offset).abs() < 4.0 * reference.vpd);
    assert!((high + reference.offset).abs() < 4.0 * reference.vpd);
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let _ = std::fs::write(format!("{dir}/sim-asymmetric-rails.png"), crate::store::png_of(&cv));
    }
}

#[test]
fn generation_error_preserves_the_working_waveform_and_session() {
    let mut u = ui();
    u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
    let original = u.refs[0].as_ref().unwrap().v.clone();
    let state = u.workflow_state();
    u.m.sim_rate = 32e9;
    u.m.sim_ssc = true;
    u.m.sim_ssc_hz = 1e3;
    let error = u.sim_play_set(true).unwrap_err();
    assert!(error.contains("sample") || error.contains("record"), "{error}");
    assert_eq!(u.refs[0].as_ref().unwrap().v, original);
    assert_eq!(u.workflow_state(), state);
    assert!(!u.session_is_sim && !u.sim_generating());
    u.activate(Item::SimPlay);
    assert!(u.toast.as_ref().unwrap().0.contains(&error));
    assert_eq!(u.refs[0].as_ref().unwrap().v, original);
}

#[test]
fn asynchronous_generation_error_preserves_sources_and_is_reported_once() {
    let mut u = ui();
    u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
    let original = u.refs[0].as_ref().unwrap().v.clone();
    u.m.sim_ssc_hz = 1e6;
    u.m.sim_rj = 50e-12; // seeded random edges cross at this amplitude
    let state = u.workflow_state();
    u.sim_play_set(true).unwrap();
    assert!(u.sim_generating());
    u.wait_sim_generation();
    assert_eq!(u.refs[0].as_ref().unwrap().v, original);
    assert!(!u.session_is_sim);
    assert_eq!(u.workflow_state(), state);
    let error = u.take_sim_error().expect("the asynchronous error reaches SCPI");
    assert!(error.contains("jitter"), "{error}");
    assert!(u.take_sim_error().is_none());
    assert!(u.toast.as_ref().unwrap().0.contains(&error));
}

#[test]
fn live_skips_unrequested_refs_but_keeps_hidden_measurements_and_remote_facts() {
    let mut u = ui();
    u.m.sim_ssc = false;
    u.sim_play_set(true).unwrap();
    u.return_to_live();
    assert!(!u.refs[0].as_ref().unwrap().on);
    u.measure_refs();
    assert!(u.measured.refs[0].get(Meas::Vpp).is_nan(), "an unused hidden ref has no cached analysis");
    assert!(!u.measured.refs_valid[0]);
    assert!(u.meas_at(WaveId::Ref(0), Meas::Vpp) > 0.5, "direct UI measurement also analyzes on demand");
    assert!(!u.measured.refs_valid[0], "an on-demand read must not enable repeated idle rescans");
    assert!(u.fact_meas(Meas::Vpp, WaveId::Ref(0)) > 0.5, "explicit SCPI facts still analyze a hidden ref");
    u.m.meas.push((Meas::Vpp, WaveId::Ref(0)));
    u.measure_refs();
    assert!(u.measured.refs[0].get(Meas::Vpp) > 0.5, "a hidden measured source remains current");
    u.m.meas.clear();
    u.m.meas2.push((0, WaveId::Ref(0), WaveId::Ch(0)));
    u.measure_refs();
    assert!(u.measured.refs[0].get(Meas::Vpp) > 0.5, "pairs retain their hidden reference source");
    u.m.meas2.clear();
    u.set_offline(true);
    u.m.dvm_on = true;
    u.measure_refs();
    assert!(u.measured.refs[0].get(Meas::Vpp) > 0.5, "the offline meter retains its source");
}

#[test]
fn cancelled_ssc_generation_cannot_replace_live_or_an_imported_record() {
    for import in [false, true] {
        let mut u = ui();
        let live = (u.m.n_on(), u.m.run, u.record_mode());
        u.m.sim_ssc_hz = 1e6; // complete SSC period, small enough for a fast worker test
        u.sim_play_set(true).unwrap();
        assert!(u.sim_generating());
        assert!(!u.session_is_sim, "starting generation must not replace the live session");
        if import {
            u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
            assert_eq!(
                (u.m.n_on(), u.m.run, u.record_mode()),
                live,
                "cancelling pending SIM must preserve additive Live import"
            );
        } else {
            u.return_to_live();
        }
        assert!(!u.sim_generating());
        let state = u.workflow_state();
        u.wait_sim_worker();
        assert_eq!(u.workflow_state(), state, "a stale background reply must not start SIM");
        assert!(!u.session_is_sim);
        if import {
            assert_eq!(u.refs[0].as_ref().unwrap().label, "keep.csv");
        }
    }
}

#[test]
fn newest_ssc_request_wins_and_stop_cancels_pending_work() {
    let mut u = ui();
    u.m.sim_ssc_hz = 1e6;
    u.sim_play_set(true).unwrap();
    u.m.sim_slot = 1;
    u.m.sim_prbs = 13;
    u.sim_load(false).unwrap();
    u.wait_sim_generation();
    assert!(u.session_is_sim);
    assert!(u.refs[0].is_none(), "superseded request must not populate its old slot");
    assert!(u.refs[1].is_some());
    assert_eq!(u.m.run, RunState::Stop);
    u.sim_load(true).unwrap();
    assert!(u.sim_generating());
    u.key(key::SINGLE); // arm the existing reference and cancel a pending replacement
    assert!(!u.sim_generating());
    u.key(key::RUNSTOP);
    let state = u.workflow_state();
    u.wait_sim_worker();
    assert_eq!(u.workflow_state(), state);
    assert_eq!(u.m.run, RunState::Stop);
}

#[test]
fn entering_sim_preserves_configured_measurements_pairs_and_settings() {
    let mut u = ui();
    u.m.sim_ssc = false;
    u.m.sim_slot = 4;
    u.m.meas = ALL_MEAS.iter().map(|&kind| (kind, WaveId::Ref(1))).collect();
    u.m.meas2 = (0..super::tools::PAIRS.len()).map(|kind| (kind, WaveId::Ref(1), WaveId::Ref(2))).collect();
    u.m.meas_full_record = true;
    u.m.meas_labels = true;
    let singles = u.m.meas.clone();
    let pairs = u.m.meas2.clone();
    u.sim_load(false).unwrap();
    u.wait_sim_generation();
    assert!(u.m.meas.starts_with(&singles));
    assert_eq!(u.m.meas.len(), singles.len() + SIM_MEAS.len());
    assert_eq!(u.m.meas2, pairs);
    assert!(u.m.meas_full_record && u.m.meas_labels);
    assert_eq!(u.m.run, RunState::Stop);
    let configured = u.m.meas.clone();
    u.sim_load(false).unwrap();
    u.wait_sim_generation();
    assert_eq!(u.m.meas, configured, "regenerating does not duplicate SIM readouts");
}

#[test]
fn live_is_on_the_top_bar_and_in_the_sim_menu() {
    let mut u = ui();
    assert!(u.m.ch[0].on);
    assert!(!u.top_buttons().iter().any(|b| b.0 == TopBtn::Live));
    u.session_is_sim = true;
    u.m.ch[0].on = false;
    assert!(u.sim_items().first() == Some(&Item::SimLive));
    let live = u.top_buttons().into_iter().find(|b| b.0 == TopBtn::Live).expect("Live");
    u.tap(live.2.x + 4, 10);
    assert!(!u.session_is_sim);
    assert!(u.m.ch[0].on);
    u.session_is_sim = true;
    u.activate(Item::SimLive);
    assert!(!u.session_is_sim && u.m.ch[0].on);
}

#[test]
fn sim_play_loads_the_preset_into_the_slot() {
    let mut u = ui();
    u.m.sim_preset = LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
    u.m.apply_sim_preset();
    u.m.sim_slot = 2;
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    let r = u.refs[2].as_ref().expect("R3");
    assert_eq!(r.label, "PCIe 4.0");
    assert!(r.on);
    assert_eq!(r.v.len(), serial::sample_count(&u.sim_lane()).unwrap());
    assert!((r.dt - 1.0 / (8.0 * 16e9)).abs() < 1e-18);
    assert_eq!(u.m.eye_src, WaveId::Ref(2));
    assert!((u.m.eye_rate - 16e9).abs() < 1.0);
    assert!(u.m.ch[0].on, "SIM is a reference and does not switch the analog channels off");
    assert!(u.last[0].is_none());
    assert_eq!(u.sources.a, WaveId::Ref(2));
    assert!(u.m.zoom);
    // The overview fits the complete SSC period; zoom retains the bit scale.
    let record_span = r.v.len() as f64 * r.dt;
    let expected_scale = tscales().iter().copied().find(|&scale| scale as f64 * HDIV as f64 >= record_span).unwrap();
    assert_eq!(u.m.time_per_div(), expected_scale);
    assert!((tscales()[u.m.zscale] as f64 - 50e-12).abs() < 1e-12, "zoom {}", tscales()[u.m.zscale]);
    assert!((u.m.delay as f64 - record_span * 0.5).abs() < record_span * 1e-6);
    assert!(u.m.meas.iter().all(|(_, id)| *id == WaveId::Ref(2)));
    assert_eq!(u.m.meas.len(), 12);
    u.render(&mut Canvas::new(W, H));
    let names = serial::names().join(" ");
    assert!(!names.to_ascii_lowercase().contains("hss"));
    assert!(!names.contains("Waveform"));
    let labels: String = u.sim_items().iter().filter_map(|&it| Ui::sim_label(it)).collect::<Vec<_>>().join(" ");
    assert!(!labels.to_ascii_lowercase().contains("hss"));
}

#[test]
fn sim_usb32_gen2_uses_the_record_timebase() {
    let mut u = ui();
    u.m.sim_preset = LANES.iter().position(|l| l.name == "USB 3.2 Gen 2").unwrap();
    u.m.apply_sim_preset();
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    let r = u.refs[0].as_ref().expect("R1");
    assert_eq!(r.label, "USB 3.2 Gen 2");
    assert!((r.dt - 1.0 / (8.0 * 10e9)).abs() < 1e-18);
    assert!((u.m.time_per_div() as f64 - 100e-9).abs() < 1e-12, "overview tdiv {}", u.m.time_per_div());
    assert!((tscales()[u.m.zscale] as f64 - 100e-12).abs() < 1e-12, "zoom {}", tscales()[u.m.zscale]);
    let (sr, n) = u.sim_rate_depth().expect("rate");
    assert!((sr - 80e9).abs() < 1.0, "sr {}", sr);
    assert_eq!(n, 65_536);
    let rows = u.meas_rows();
    assert_eq!(rows.len(), 12);
    for want in
        ["Pk-pk", "Freq", "Period", "Rise", "Fall", "+Duty", "Amp", "Overshoot", "+Width", "-Width", "+Slew", "+Edges"]
    {
        let row = rows.iter().find(|r| r.name == want).unwrap_or_else(|| panic!("missing {want}"));
        assert!(!row.v.is_infinite(), "{want} {}", row.v);
    }
    let vpp = rows.iter().find(|r| r.name == Meas::Vpp.name()).unwrap().v;
    assert!(vpp > 0.5 && vpp < 1.2, "vpp {}", vpp);
    assert!(!u.m.meas_labels, "labels are opt-in so initial SIM does not hide the waveform");
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let png = crate::store::png_of(&cv);
        let _ = std::fs::write(format!("{}/sim-usb32-gen2-loaded.png", dir), &png);
        let _ = std::fs::write(format!("{}/sim-usb32-measurements.png", dir), &png);
    }
}

#[test]
fn sim_knobs_change_the_record() {
    let mut u = ui();
    u.m.sim_preset = LANES.iter().position(|l| l.name == "USB 3.2 Gen 2").unwrap();
    u.m.apply_sim_preset();
    u.m.sim_rate = 5e9;
    u.m.sim_prbs = 7;
    u.m.sim_deemph = 3.5;
    u.m.sim_ssc = true;
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    let r = u.refs[0].as_ref().unwrap();
    assert!((r.dt - 1.0 / (8.0 * 5e9)).abs() < 1e-18);
    u.activate(Item::SimDefaults);
    assert!((u.m.sim_rate - 10e9).abs() < 1.0);
    assert_eq!(u.m.sim_prbs, 15);
    assert!(!u.m.sim_ssc);
    u.open_menu(MenuId::Sim);
    let labels: Vec<_> = u.sim_items().iter().filter_map(|&it| Ui::sim_label(it)).collect();
    for want in [
        "Rate",
        "Pattern",
        "High",
        "Low",
        "Rise 20-80%",
        "Bandwidth",
        "De-emph",
        "Noise",
        "Spread",
        "RJ RMS",
        "PJ peak",
    ] {
        assert!(labels.contains(&want), "missing {want} in {labels:?}");
    }
    u.render(&mut Canvas::new(W, H));
    if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        let _ = std::fs::write(format!("{}/sim-menu-knobs.png", dir), crate::store::png_of(&cv));
    }
}

#[test]
fn sim_play_does_not_arm_the_adc() {
    let mut u = ui();
    u.m.sim_preset = LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
    u.m.apply_sim_preset();
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    assert!(u.last[0].is_none());
    u.m.run = RunState::Run;
    u.step(1.0 / 60.0);
    assert!(u.last[0].is_none(), "Run with analog off must not acquire");
    assert!(u.m.ch[0].on, "the inputs keep their state; Run in a record session still does not acquire");
    u.return_to_live();
    u.m.tscale = 0;
    u.m.ch[0].on = true;
    u.m.floor_live_tdiv();
    assert!((u.m.time_per_div() as f64 - 1e-9).abs() < 1e-12);
    u.step(1.0 / 60.0);
    assert!(u.last[0].is_some(), "Run with analog on acquires");
}

#[test]
fn sim_show_toggles_the_slot() {
    let mut u = ui();
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    assert!(u.refs[0].as_ref().unwrap().on);
    u.adjust(Item::SimShow, 1);
    assert!(!u.refs[0].as_ref().unwrap().on);
}

fn set_tdiv(u: &mut Ui, tdiv: f32) {
    let ts = tscales();
    u.m.tscale = ts.iter().position(|&t| (t - tdiv).abs() <= tdiv * 1e-4).expect("timebase");
    u.m.tfine = 1.0;
}

/// A step in the record, low then high, so the edge on screen has a place to move.
fn put_step(u: &mut Ui, n: usize, dt: f64, tdiv: f32) {
    let v: Vec<f32> = (0..n).map(|i| if i < n / 5 { -0.5 } else { 0.5 }).collect();
    u.refs[0] = Some(RefWave { label: "rec".into(), vpd: 0.2, ..RefWave::test(v, dt, 0.0) });
    u.m.sim_slot = 0;
    u.m.ch[0].on = false;
    u.m.run = RunState::Run;
    u.m.trig_mode = TrigMode::Normal;
    u.m.trig_level = 0.0;
    u.m.delay = 0.0;
    set_tdiv(u, tdiv);
    u.sim_play = true;
}

fn window_of(u: &Ui) -> f64 {
    u.view().0 as f64 * HDIV as f64
}

#[test]
fn replay_locks_the_view_to_the_trigger_condition() {
    let mut u = ui();
    put_step(&mut u, 5_000, 1e-9, 100e-9);
    u.step(0.25);
    assert_eq!(u.m.status, TrigStatus::Triggered);
    assert_eq!(u.replay.captures, 1);
    assert!((u.m.delay as f64 - 999.5e-9).abs() < 1e-9);
    let position = u.m.delay;
    u.step(0.25);
    assert_eq!(u.m.delay, position, "a repeated trigger holds the edge in place");
    assert_eq!(u.replay.captures, 2);
    assert!(u.ph.map.iter().all(|m| m.iter().all(|&p| p == 0)));
}

#[test]
fn normal_replay_waits_and_force_captures_without_a_match() {
    let mut u = ui();
    put_step(&mut u, 5_000, 1e-9, 100e-9);
    u.m.trig_level = 10.0;
    u.step(0.25);
    assert_eq!(u.m.status, TrigStatus::Wait);
    assert_eq!(u.replay.captures, 0);
    u.key(key::FORCE);
    u.step(0.25);
    assert_eq!(u.replay.captures, 1);
    assert_eq!(u.m.status, TrigStatus::Auto);
}

#[test]
fn play_holds_a_record_that_fits() {
    let mut u = ui();
    put_step(&mut u, 200, 1e-9, 100e-9);
    let r = u.refs[0].as_ref().unwrap();
    let end = (r.v.len() - 1) as f64 * r.dt;
    let window = window_of(&u);
    assert!(end <= window);
    u.step(0.0);
    let delay = u.m.delay;
    let left = delay as f64 - window * 0.5;
    assert!(left <= 0.0 && left + window >= end);
    u.key(key::RUNSTOP);
    let first = picture(&mut u);
    u.step(1.0);
    assert_eq!(u.m.delay, delay);
    let second = picture(&mut u);
    assert_eq!(first.px, second.px);
}

#[test]
fn main_loop_sim_changes_samples_measurements_and_eye_only_on_accepted_captures() {
    let mut u = ui();
    u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 2.0 HS").unwrap();
    u.m.apply_sim_preset();
    u.m.sim_ssc = false;
    u.m.meas_full_record = true;
    u.sim_play_set(true).unwrap();
    u.wait_sim_generation();
    u.eye_make();
    let records = |u: &Ui| u.eye_readings().iter().find(|(name, _)| *name == "RECords").unwrap().1;
    let before = u.refs[0].as_ref().unwrap().v.clone();
    let value = u.meas_at(WaveId::Ref(0), Meas::Vpp);
    let captures = u.replay.captures;
    let eye_records = records(&u);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while u.replay.captures < captures + 2 && std::time::Instant::now() < deadline {
        u.step(0.01);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(u.replay.captures >= captures + 2);
    assert_ne!(u.refs[0].as_ref().unwrap().v, before, "capture counters alone do not prove fresh acquisition");
    assert_ne!(
        u.meas_at(WaveId::Ref(0), Meas::Vpp),
        value,
        "full-record SIM measurements must use the new noisy samples"
    );
    assert!(records(&u) >= eye_records + 2.0, "Eye accumulates each accepted revision");
    u.key(key::RUNSTOP);
    let held = u.refs[0].as_ref().unwrap().v.clone();
    let held_count = u.replay.captures;
    let held_eye = records(&u);
    u.step(1.0);
    u.wait_sim_worker();
    u.step(0.1);
    assert_eq!(u.refs[0].as_ref().unwrap().v, held);
    assert_eq!(u.replay.captures, held_count);
    assert_eq!(records(&u), held_eye);
    u.key(key::SINGLE);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while u.m.run != RunState::Stop && std::time::Instant::now() < deadline {
        u.step(0.01);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(u.m.run, RunState::Stop);
    assert_eq!(u.replay.captures, held_count + 1);
    assert_ne!(u.refs[0].as_ref().unwrap().v, held, "Single also needs a fresh generated record");
    assert_eq!(records(&u), held_eye + 1.0);
}

fn drive_until(u: &mut Ui, done: impl Fn(&Ui) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !done(u) && std::time::Instant::now() < deadline {
        u.step(0.01);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(done(u), "SIM main loop did not reach the expected capture/analysis state");
}

#[test]
fn actual_default_ssc_accepts_fresh_large_captures_and_honors_stop_single_wait_force() {
    let mut u = ui();
    assert_eq!(u.sim_lane().name, "PCIe 4.0");
    assert!(u.sim_lane().ssc);
    let count = serial::sample_count(&u.sim_lane()).unwrap();
    assert!(count > 3_000_000, "the actual default SSC record must exercise asynchronous triggering");
    u.sim_play_set(true).unwrap();
    drive_until(&mut u, |u| u.replay.captures >= 1);
    assert_eq!(u.refs[0].as_ref().unwrap().v.len(), count);
    assert!(!u.reference_search_busy(), "initial acceptance uses worker-computed hits");
    let first = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
    let first_value = u.meas_at(WaveId::Ref(0), Meas::Vpp);
    drive_until(&mut u, |u| u.replay.captures >= 2);
    assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], first);
    assert_ne!(u.meas_at(WaveId::Ref(0), Meas::Vpp), first_value);
    u.key(key::RUNSTOP);
    let held = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
    let held_count = u.replay.captures;
    u.wait_sim_worker();
    u.step(0.2);
    assert_eq!(u.replay.captures, held_count);
    assert_eq!(u.refs[0].as_ref().unwrap().v[..8192], held);
    u.key(key::SINGLE);
    drive_until(&mut u, |u| u.m.run == RunState::Stop);
    assert_eq!(u.replay.captures, held_count + 1);
    assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], held);
    let waiting = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
    let before = u.replay.captures;
    u.m.trig_level = 10.0;
    u.key(key::SINGLE);
    let attempt = u.replay.sim_attempt;
    drive_until(&mut u, |u| u.replay.sim_attempt >= attempt + 2);
    assert_eq!(u.m.status, TrigStatus::Wait);
    assert_eq!(u.replay.captures, before);
    assert_eq!(
        u.refs[0].as_ref().unwrap().v[..8192],
        waiting,
        "no-hit candidates never become the visible accepted record"
    );
    u.key(key::FORCE);
    drive_until(&mut u, |u| u.m.run == RunState::Stop);
    assert_eq!(u.replay.captures, before + 1);
    assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], waiting);
    println!("default SSC: {count} samples; fresh captures, Stop, Single, WAIT and Force passed");
}

#[test]
fn large_full_record_sim_backpressure_finishes_values_and_counts_statistics_once_per_capture() {
    let mut u = ui();
    assert!(serial::sample_count(&u.sim_lane()).unwrap() > 3_000_000);
    u.m.meas_full_record = true;
    u.sim_play_set(true).unwrap();
    drive_until(&mut u, |u| u.replay.captures >= 1);
    drive_until(&mut u, |u| u.results.rows.first().is_some_and(|stat| stat.n >= 1));
    assert!(u.meas_at(WaveId::Ref(0), Meas::Vpp).is_finite());
    assert_eq!(u.m.run, RunState::Run, "Full record must become ready without stopping");
    let first = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
    let value = u.meas_at(WaveId::Ref(0), Meas::Vpp);
    drive_until(&mut u, |u| u.results.rows.first().is_some_and(|stat| stat.n >= 2));
    assert_eq!(u.results.rows[0].n, u.replay.captures);
    assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], first);
    assert_ne!(u.meas_at(WaveId::Ref(0), Meas::Vpp), value);
    u.key(key::RUNSTOP);
    let n = u.results.rows[0].n;
    for _ in 0..10 {
        u.poll_reference_measurements();
        u.finish_deferred_capture_statistics();
        let _ = u.meas_rows();
    }
    assert_eq!(u.results.rows[0].n, n, "worker polling and reads cannot count an accepted revision twice");
    u.wait_sim_worker();
}

#[test]
fn generated_hits_reject_changed_trigger_source_threshold_hysteresis_and_secondary_record() {
    for change in 0..7 {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 2.0 HS").unwrap();
        u.m.apply_sim_preset();
        u.sim_play_set(true).unwrap();
        u.refs[1] = Some(RefWave {
            color: 1,
            label: "External".into(),
            vpd: 0.2,
            on: false,
            ..RefWave::test((0..1024).map(|i| if i % 16 < 8 { -0.5 } else { 0.5 }).collect::<Vec<_>>(), 2e-9, 0.0)
        });
        if change == 5 {
            u.m.trig_type = TrigType::Delay;
            u.m.set_trigger_wave(WaveId::Ref(1), true);
        } else if change == 6 {
            u.m.trig_type = TrigType::Bus;
            u.m.dec_proto = Proto::UART;
            u.m.trig_bus = BusWhat::Data;
            u.m.trig_bus_val = 0x55;
        }
        let mut lane = u.sim_lane();
        lane.seed ^= 19;
        let request = u.sim_capture_request(lane, 0, true, true, true);
        let generated = Generated::evaluate(&request, serial::try_generate(&lane).unwrap());
        let pointer = u.refs[0].as_ref().unwrap().v.as_ptr();
        let captures = u.replay.captures;
        match change {
            0 => u.m.trig_level = 10.0,
            1 => u.m.trig_type = TrigType::Pulse,
            2 => u.m.set_trigger_wave(WaveId::Ref(1), false),
            3 => u.refs[0].as_mut().unwrap().vpd *= 2.0,
            4 => u.m.trig_nrej = !u.m.trig_nrej,
            5 => u.refs[1].as_mut().unwrap().v = vec![0.0; 1024].into(),
            _ => u.m.trig_bus_val = 0xaa,
        }
        u.sim_finish(request, generated);
        assert_eq!(u.refs[0].as_ref().unwrap().v.as_ptr(), pointer);
        assert_eq!(u.replay.captures, captures, "change {change}: stale worker hits cannot validate a new condition");
    }
}

#[test]
fn external_reference_scan_holds_one_candidate_without_exposing_it_and_keeps_its_own_timebase() {
    for source in [1, 2] {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 2.0 HS").unwrap();
        u.m.apply_sim_preset();
        u.sim_play_set(true).unwrap();
        u.refs[source] = Some(RefWave {
            color: source,
            label: "Long external trigger".into(),
            vpd: 0.2,
            on: false,
            ..RefWave::test(
                (0..263_168).map(|i| if i % 16 < 8 { -0.5 } else { 0.5 }).collect::<Vec<_>>(),
                10e-12,
                20e-9,
            )
        });
        assert!(u.set_trigger_source(WaveId::Ref(source), false));
        u.m.trig_level = 0.0;
        u.m.trig_mode = TrigMode::Single;
        let captures = u.replay.captures;
        let pointer = u.refs[0].as_ref().unwrap().v.as_ptr();
        let mut lane = u.sim_lane();
        lane.seed ^= 23;
        let request = u.sim_capture_request(lane, 0, true, true, true);
        assert!(request.trigger.is_none(), "independent external triggers use their existing shared cache");
        let generated = Generated::evaluate(&request, serial::try_generate(&lane).unwrap());
        u.sim_finish(request, generated);
        assert_eq!(u.refs[0].as_ref().unwrap().v.as_ptr(), pointer);
        assert_eq!(u.replay.captures, captures);
        assert!(u.sim_gen.worker.as_ref().unwrap().pending.is_some());
        u.wait_sim_generation();
        assert_eq!(u.replay.captures, captures + 1);
        assert_eq!(u.m.run, RunState::Stop);
        assert!((u.replay.trigger_time.unwrap() - (20e-9 + 7.5 * 10e-12)).abs() < 1e-15);
        let hits = u.reference_trigger_hits(source).unwrap();
        u.replay.sim_attempt += 1;
        assert!(
            Arc::ptr_eq(&hits, &u.reference_trigger_hits(source).unwrap()),
            "unrelated SIM attempts must not invalidate the external hit cache"
        );
    }
}

#[test]
fn loaded_sim_uses_run_stop_and_returns_to_live_explicitly() {
    let mut u = ui();
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    let before = u.replay.captures;
    u.step(0.5);
    // A fresh capture is generated off-thread. Advancing simulated time is
    // not evidence that the worker has run, particularly in the full suite.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while u.replay.captures == before && std::time::Instant::now() < deadline {
        u.step(0.01);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(u.replay.captures > before && u.sim_play);
    assert!(u.last[0].is_none());
    u.key(key::RUNSTOP);
    let held = u.replay.captures;
    u.step(0.5);
    assert_eq!(u.replay.captures, held);
    u.return_to_live();
    u.step(0.02);
    assert!(!u.sim_play && u.last[0].is_some());
}

fn picture(u: &mut Ui) -> crate::gfx::Canvas {
    let mut cv = crate::gfx::Canvas::new(W, H);
    u.render(&mut cv);
    cv
}
