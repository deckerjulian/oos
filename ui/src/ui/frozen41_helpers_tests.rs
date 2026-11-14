use super::*;
use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};

fn reference(v: Vec<f32>, dt: f64, t0: f64) -> RefWave {
    RefWave { src: 0, color: 0, label: "frozen41".into(), v: v.into(), dt, t0,
        vpd: 0.2, offset: 0.0, on: true }
}

#[test]
fn force_queues_exactly_one_live_mock_request_even_with_a_modal_open() {
    let shared = Arc::new(Mutex::new(crate::board::Shared::default()));
    let mut ui = Ui::new(Source::Hw { shared: Arc::clone(&shared), seen: 0 });
    for modal in 0..3 {
        ui.m.run = RunState::Stop;
        ui.dialog = None; ui.power_dlg = false; ui.close_menu();
        match modal { 0 => ui.dialog = Some(vec!["modal".into()]), 1 => ui.open_menu(MenuId::Trigger), _ => ui.power_dlg = true }
        shared.lock().unwrap().force = false;
        ui.key(key::FORCE);
        let state = shared.lock().unwrap();
        assert!(state.force);
        assert_eq!(state.force_requests, modal + 1, "one key press must not request the pulse twice");
        assert_eq!(ui.m.run, RunState::Run);
        assert_eq!((ui.records, ui.wfm_total, ui.replay.captures), (0, 0, 0));
    }
}

#[test]
fn force_in_stored_and_sim_sessions_never_touches_the_mock_hardware() {
    for sim in [false, true] {
        let shared = Arc::new(Mutex::new(crate::board::Shared::default()));
        let mut ui = Ui::new(Source::Hw { shared: Arc::clone(&shared), seen: 0 });
        ui.refs[0] = Some(reference(vec![0.0; 100], 1e-9, 0.0));
        if sim { ui.session_is_sim = true; } else { ui.set_offline(true); }
        ui.m.sim_slot = 0; ui.ref_sel = 0;
        ui.m.trig_mode = TrigMode::Single; ui.m.run = RunState::Stop;
        ui.dialog = Some(vec!["modal must not swallow Force".into()]);
        ui.key(key::FORCE);
        assert!(ui.replay.force_requested());
        assert_eq!(ui.m.run, RunState::Run);
        assert!(ui.sim_play);
        let state = shared.lock().unwrap();
        assert!(!state.force);
        assert_eq!(state.force_requests, 0);
        assert_eq!(ui.replay.captures, 0);
    }
}

#[test]
fn force_after_completed_single_arms_a_fresh_receipt_in_live_and_stored_modes() {
    for stored in [false, true] {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.key(key::SINGLE);
        let old = ui.scpi_operations();
        let deadline = Instant::now() + Duration::from_secs(5);
        while ui.m.run != RunState::Stop {
            ui.step(0.02); assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(old.state(&ui), OperationState::Complete);
        let shared = Arc::new(Mutex::new(crate::board::Shared::default()));
        ui.src = Source::Hw { shared: Arc::clone(&shared), seen: 0 };
        if stored {
            ui.refs[0] = Some(reference(vec![0.0; 100], 1e-9, 0.0));
            ui.set_offline(true);
        }
        let accepted = ui.wfm_total;
        ui.force_trigger();
        assert_eq!(ui.scpi_operations().state(&ui), OperationState::Pending,
            "the earlier accepted Single receipt must not complete a newly forced one-shot");
        assert_eq!(ui.wfm_total, accepted);
        assert_eq!(shared.lock().unwrap().force_requests, u64::from(!stored));
    }
}

#[test]
fn independent_track_sources_use_refs_native_channels_deskew_and_real_units() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..4 { ui.step(0.05); }
    let statistics = ui.measurement_statistics_snapshot_for_test();
    ui.refs[0] = Some(reference(vec![0.0, 1.0, 2.0, 3.0], 1e-9, 10e-9));
    ui.refs[1] = Some(reference(vec![10.0, 11.0, 12.0, 13.0], 2e-9, 12e-9));
    ui.m.deskew_ref[0] = 2e-9; ui.m.deskew_ref[1] = 4e-9;
    ui.m.cursor = CursorMode::Track;
    assert!(ui.set_cursor_source(WaveId::Ref(0)));
    assert_eq!(ui.cursor_b_source(), WaveId::Ref(0), "unset B follows selected A, including refs");
    ui.key(key::SINGLE); let fence = ui.scpi_operations();
    let trigger = ui.m.trigger_wave();
    assert!(ui.set_cursor_b_source(WaveId::Ref(1)));
    assert_eq!(ui.cursor_source(), WaveId::Ref(0));
    assert_eq!(ui.wave_at_source(ui.cursor_source(), 9e-9), Some(1.0));
    assert_eq!(ui.wave_at_source(ui.cursor_b_source(), 12e-9), Some(12.0));
    let frame = Arc::new(crate::board::Frame { dt: 0.5e-9,
        ch: [Vec::new(), (0..100).map(|i| 50.0 + i as f32).collect(), Vec::new(), Vec::new()], ..Default::default() });
    let mut record = Ui::blank_record(); record.ch[1] = vec![999.0; 4]; record.dt = 1.0; record.t0 = 100.0;
    record.raw = Some(crate::wave::Raw { frame: Arc::clone(&frame), i0: 10, i1: 14, t0: 14e-9, conditioned: true });
    ui.last[0] = Some(record); ui.m.ch[1].on = true; ui.m.deskew_ch[1] = 2e-9;
    assert!(ui.set_cursor_b_source(WaveId::Ch(1)));
    assert!((ui.wave_at_source(ui.cursor_b_source(), 12e-9).unwrap() - 60.0).abs() < 1e-4,
        "B uses the full native origin, not the wrong processed screen record");
    ui.m.ch[1].unit = AmpUnit::A;
    assert_eq!(ui.cursor_source_unit(WaveId::Ref(0)), "V");
    assert_eq!(ui.cursor_source_unit(WaveId::Ch(1)), "A");
    assert_eq!(ui.cursor_values().last().unwrap().1, "----", "a V-A subtraction is unavailable");
    assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics);
    assert_eq!(ui.m.trigger_wave(), trigger);
    assert_eq!(fence.state(&ui), OperationState::Pending);
}

#[test]
fn track_b_source_setup_roundtrip_accepts_legacy_channel_and_rejects_invalid_ids() {
    let mut model = Model::default();
    model.cur_b_source = Some(WaveId::Ref(3));
    let restored = crate::store::parse_setup(&crate::store::setup_text(&model)).unwrap();
    assert_eq!(restored.cur_b_source, model.cur_b_source);
    crate::store::set(&mut model, "cur_b_ch", "1").unwrap();
    assert_eq!(model.cur_b_source, Some(WaveId::Ch(1)));
    for invalid in ["R99", "CH99", "-1", "NaN"] {
        let old = model.cur_b_source;
        assert!(crate::store::set(&mut model, "cur_b_source", invalid).is_err());
        assert_eq!(model.cur_b_source, old);
    }
    crate::store::set(&mut model, "cur_b_source", "none").unwrap();
    assert_eq!(model.cur_b_source, None);
}

#[test]
fn remote_current_statistics_use_the_selected_rows_own_thresholds() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 }); ui.m.run = RunState::Stop; ui.m.meas_full_record = true;
    let values = (0..1600).map(|i| { let k=i%400; match k {
        0..=99 => -0.5, 100..=199 => -0.5+(k-100) as f32/99.0,
        200..=299 => 0.5, _ => 0.5-(k-300) as f32/99.0,
    }}).collect();
    ui.refs[0] = Some(reference(values, 1e-9, 0.001));
    ui.m.meas = vec![(Meas::Rise, WaveId::Ref(0))];
    ui.meas_own.insert((Meas::Rise, WaveId::Ref(0)), reflook::MeasOwn {
        refs_set: true, low: 0.4, high: 0.6, ..reflook::MeasOwn::shared(&ui.m) });
    let shared = ui.meas_at(WaveId::Ref(0), Meas::Rise);
    let own = ui.meas_number(WaveId::Ref(0), Meas::Rise);
    assert!(shared.is_finite() && own.is_finite() && own < shared*0.5);
    assert_eq!(ui.meas_remote_stat(Meas::Rise, WaveId::Ref(0)).1, own);
    assert_eq!(ui.meas_rows()[0].v, own);
}

#[test]
fn statistics_reset_keeps_rows_and_pending_single_but_rejects_old_deferred_counts() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 }); ui.set_offline(true);
    ui.refs[0] = Some(reference((0..3_000_000).map(|i| if i%20<10 {1.0}else{-1.0}).collect(), 1e-9, 0.0));
    ui.m.meas = vec![(Meas::Period, WaveId::Ref(0))]; ui.m.meas2.clear(); ui.m.cat.clear(); ui.m.meas_full_record=true;
    ui.m.set_trigger_wave(WaveId::Ref(0), false); ui.m.trig_level=0.0;
    ui.m.trig_mode=TrigMode::Single; ui.m.run=RunState::Run; ui.sim_play=true;
    let end=Instant::now()+Duration::from_secs(10);
    while ui.replay.captures==0 {ui.step(0.02);assert!(Instant::now()<end);std::thread::sleep(Duration::from_millis(1));}
    assert!(ui.measurement_busy()); ui.key(key::SINGLE); let fence=ui.scpi_operations();
    let selection=ui.m.meas.clone(); ui.stats_reset();
    while ui.measurement_busy() {ui.poll_reference_measurements();assert!(Instant::now()<end);std::thread::sleep(Duration::from_millis(2));}
    ui.finish_deferred_capture_statistics();
    assert_eq!(ui.m.meas,selection); assert!(ui.mstat.iter().all(|s|s.n==0));
    assert_eq!(ui.replay.captures,1);assert_eq!(fence.state(&ui),OperationState::Pending);
}

#[test]
fn histogram_statistics_use_the_detail_lane_transform_and_horizontal_time_axis() {
    let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.run=RunState::Stop;
    ui.refs[0]=Some(reference(vec![0.0;100],1e-9,0.0));ui.workspace.stacked=true;ui.set_zoom(true);ui.relayout();
    ui.zoom_vfactor=4.0;ui.zoom_vcenter=0.5;ui.m.ch[0].offset=0.1;
    ui.m.ch[0].vscale=VSCALES.iter().position(|&v|v==0.2).unwrap();ui.m.ch[0].unit=AmpUnit::A;
    ui.meas_src=WaveId::Ch(0);ui.m.hist=Hist::Vertical;
    let(w,h)=(ui.phz.w,ui.phz.h);let(top,bottom)=(h/4,3*h/4);
    ui.ph.clear();ui.phz.clear();ui.ph.map[0][0]=200;
    ui.phz.map[0][top*w+w/2]=3;ui.phz.map[0][bottom*w+w/2]=1;
    let stat=ui.hist_stat().unwrap();let scale=ui.m.ch[0].volts_per_div() as f64;
    let volts=|row:usize|(0.5+(0.5-row as f64/h as f64)*VDIV as f64/4.0)*scale-0.1f32 as f64;
    assert_eq!(stat.sum,4);assert!((stat.max-volts(top)).abs()<1e-12);assert!((stat.min-volts(bottom)).abs()<1e-12);
    assert!((stat.mean-(3.0*volts(top)+volts(bottom))/4.0).abs()<1e-12);
    ui.phz.clear();ui.m.hist=Hist::Horizontal;ui.m.trig_level=0.0;
    ui.phz.map[0][h/2*w+w/4]=2;ui.phz.map[0][h/2*w+3*w/4]=2;
    let stat=ui.hist_stat().unwrap();let(tdiv,delay)=ui.view();
    assert_eq!(stat.sum,4);assert!((stat.mean-delay as f64).abs()<1e-10);
    assert!((stat.bin-tdiv as f64*HDIV as f64/w as f64).abs()<1e-15);
}

#[test]
fn invalid_reference_color_and_mask_pair_do_not_partially_mutate_configuration() {
    let mut ui=Ui::new(Source::Demo{t:0.0});ui.refs[0]=Some(reference(vec![0.0;10],1e-9,0.0));ui.ref_sel=0;
    assert!(!ui.ref_color_set(1,1));assert!(!ui.ref_color_set(0,REF_COLORS.len()));
    assert_eq!(ui.ref_sel,0);assert_eq!(ui.refs[0].as_ref().unwrap().color,0);
    let before=(ui.m.mask_x,ui.m.mask_y);assert!(!ui.mask_tolerance(Some(1.0),Some(f32::NAN)));
    assert_eq!((ui.m.mask_x,ui.m.mask_y),before);
}
