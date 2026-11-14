//! Integration of SCPI helper access with the native record and visible workspace.
use super::*;
use std::{sync::Arc, time::{Duration, Instant}};

fn reference(values: Vec<f32>, dt: f64, t0: f64) -> RefWave {
    RefWave { src: 0, color: 1, label: "helper fixture".into(),
        v: values.into(), dt, t0, vpd: 0.2, offset: -0.3, on: true }
}

#[test]
fn native_channel_pages_walk_full_memory_with_original_time_and_screen_fallback() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    let mut native = (0..100).map(|i| i as f32).collect::<Vec<_>>();
    native[98] = f32::NAN;
    let frame = Arc::new(crate::board::Frame { dt: 2e-9, ch: [native, Vec::new(), Vec::new(), Vec::new()], ..Default::default() });
    let mut record = Ui::blank_record();
    record.ch[0] = vec![500.0, 501.0];
    record.dt = 10e-9;
    record.t0 = -20e-9;
    record.raw = Some(crate::wave::Raw { frame: Arc::clone(&frame), i0: 40, i1: 60,
        t0: -20e-9, conditioned: true });
    ui.last[0] = Some(record);
    let origin = -20e-9 - 40.0 * frame.dt;
    assert_eq!(ui.channel_points(0, true), 100, "RAW must include points outside the display gate");
    assert_eq!(ui.channel_origin(0, true), Some((frame.dt, origin)));
    let (page, dt, t0) = ui.channel_page(0, true, 65, 150).unwrap();
    assert_eq!(page.len(), 36);
    assert_eq!(page[0], 64.0);
    assert_eq!((page[34], page[35]), (0.0, 99.0));
    assert_eq!((dt, t0), (frame.dt, origin + 64.0 * frame.dt));
    assert!(ui.channel_page(0, true, 101, 0).unwrap().0.is_empty());
    assert_eq!(ui.channel_page(0, false, 1, 0), Some((vec![500.0, 501.0], 10e-9, -20e-9)));
    assert!(ui.channel_origin(NCH + 1, true).is_none());
    assert!(frame.ch[0][98].is_nan(), "wire-format sanitizing must not mutate native samples");
    assert_eq!(Arc::strong_count(&frame), 2, "metadata/page access must not retain another native frame");
    let envelope = Arc::new(crate::board::Frame { envelope: true, dt: frame.dt,
        ch: [vec![1.0, -1.0], Vec::new(), Vec::new(), Vec::new()], ..Default::default() });
    ui.last[0].as_mut().unwrap().raw.as_mut().unwrap().frame = envelope;
    assert_eq!(ui.channel_points(0, true), 2, "min/max envelope pairs are not native scalar samples");
    assert_eq!(ui.channel_origin(0, true), Some((10e-9, -20e-9)));
}

#[test]
fn reference_pages_keep_native_time_and_scale_independent_of_deskew_and_view() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    ui.refs[2] = Some(reference((0..20).map(|i| i as f32).collect(), 5e-12, -50e-6));
    ui.m.deskew_ref[2] = 1e-6;
    ui.m.delay = 0.123;
    let owner = Arc::clone(&ui.refs[2].as_ref().unwrap().v);
    let (page, dt, t0, scale, offset) = ui.ref_page(2, 10, 12).unwrap();
    assert_eq!(page, vec![9.0, 10.0, 11.0]);
    assert_eq!((dt, t0), (5e-12, -50e-6 + 9.0 * 5e-12));
    assert_eq!((scale, offset), (0.2f32 as f64, -0.3f32 as f64));
    assert!(Arc::ptr_eq(&owner, &ui.refs[2].as_ref().unwrap().v));
    assert!(ui.ref_page(NREF, 1, 0).is_none());
}

#[test]
fn oversized_native_ascii_page_is_rejected_before_copy_but_small_pages_keep_metadata() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    let frame = Arc::new(crate::board::Frame { dt: 2e-9,
        ch: [vec![0.25; 5_000_000], Vec::new(), Vec::new(), Vec::new()], ..Default::default() });
    let mut record = Ui::blank_record(); record.ch[0] = vec![0.25; 64]; record.dt = 2e-9; record.t0 = -20e-9;
    record.raw = Some(crate::wave::Raw { frame: Arc::clone(&frame), i0: 40, i1: 104, t0: -20e-9, conditioned: true });
    ui.last[0] = Some(record);
    let mut scpi = crate::scpi::Scpi::open(0);
    let canvas = Canvas::new(W, H);
    scpi.exec_line(&mut ui, &canvas, ":WAV:SOUR CHAN1;:WAV:MODE RAW;:WAV:FORM ASC");
    assert_eq!(scpi.exec_line(&mut ui, &canvas, ":WAV:POIN?"), b"5000000");
    let preamble = String::from_utf8(scpi.exec_line(&mut ui, &canvas, ":WAV:PRE?")).unwrap();
    assert_eq!(preamble.split(',').nth(2), Some("5000000"));
    let rejected = scpi.exec_line(&mut ui, &canvas, ":WAV:DATA?");
    assert!(std::str::from_utf8(&rejected).unwrap().starts_with("-223,"));
    assert!(String::from_utf8(scpi.exec_line(&mut ui, &canvas, ":SYST:ERR?")).unwrap().starts_with("-223,"));
    scpi.exec_line(&mut ui, &canvas, ":WAV:STAR 1;:WAV:STOP 3");
    let page = scpi.exec_line(&mut ui, &canvas, ":WAV:DATA?");
    assert_eq!(page[0], b'#');
    let start = 2 + (page[1] - b'0') as usize;
    let values = std::str::from_utf8(&page[start..]).unwrap().split(',').map(|v| v.parse::<f32>().unwrap()).collect::<Vec<_>>();
    assert_eq!(values, vec![0.25; 3]);
    assert_eq!(Arc::strong_count(&frame), 2);
}

#[test]
fn cursor_time_roundtrip_retains_small_intervals_at_large_record_offsets() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    ui.m.tscale = tscales().iter().position(|&t| (t - 100e-12).abs() < 1e-18).unwrap();
    ui.m.delay = 1.0;
    ui.m.zoom = false;
    let a = ui.cursor_time_at(0.25);
    let b = ui.cursor_time_at(0.75);
    assert!(a < 1.0 && b > 1.0, "f32 addition would collapse both cursors to the delay");
    let expected = 0.5 * HDIV as f64 * ui.view().0 as f64;
    assert!(((b - a) - expected).abs() < 3e-16);
    assert!((ui.cursor_frac_time(a) - 0.25).abs() < 1e-5);
    assert!((ui.cursor_frac_time(b) - 0.75).abs() < 1e-5);
    assert_eq!(ui.cursor_frac_time(f64::NAN), 0.5);
}

#[test]
fn selected_reference_cursors_match_stacked_zoom_readouts_and_touch_keeps_xy_axes_independent() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..4 { ui.step(0.05); }
    let statistics = ui.measurement_statistics_snapshot_for_test();
    assert!(statistics.iter().any(|s| s.0 >= 3));
    let accepted = ui.last[0].as_ref().unwrap().ch[0].as_ptr();
    ui.m.run = RunState::Stop;
    ui.refs[0] = Some(reference(vec![0.0, 1.0, 0.0, -1.0], 1e-9, 10e-9));
    ui.m.deskew_ref[0] = 2e-9;
    let channel = (ui.m.ch[0].vscale, ui.m.ch[0].offset);
    assert!(ui.set_cursor_source(WaveId::Ref(0)));
    assert_eq!(ui.cursor_source(), WaveId::Ref(0));
    assert_eq!(ui.wave_at(9e-9), Some(1.0), "track uses the selected reference's deskewed physical origin");
    assert_eq!(ui.wave_at(7e-9), None);
    ui.workspace.stacked = true;
    ui.m.zoom = true;
    ui.zoom_vfactor = 4.0;
    ui.zoom_vcenter = 0.5;
    ui.m.cursor = CursorMode::Xy;
    (ui.m.cur_a, ui.m.cur_b, ui.m.cur_c, ui.m.cur_d) = (0.2, 0.8, 0.3, 0.7);
    ui.key(key::SINGLE);
    let fence = ui.scpi_operations();
    let acquisition = (ui.m.run, ui.m.trigger_wave(), ui.replay.captures, ui.wfm_total);
    assert!(ui.set_cursor_source(WaveId::Ch(0)));
    assert!(ui.set_cursor_source(WaveId::Ref(0)));
    ui.relayout();
    let area = ui.source_area(WaveId::Ref(0)).unwrap();
    assert_eq!(ui.zoom_vertical_transform(area), (4.0, 0.5));
    let expected = (0.5 + (0.5 - ui.m.cur_c) * VDIV / 4.0) * 0.2 + 0.3;
    assert!((ui.cursor_volt_at(ui.m.cur_c) - expected as f64).abs() < 1e-7);
    for fraction in [0.15, 0.3, 0.7, 0.85] {
        assert!((ui.cursor_frac_volt(ui.cursor_volt_at(fraction)) - fraction).abs() < 1e-6);
    }
    let rows = ui.cursor_values();
    assert_eq!(rows[2], ("Y1".into(), si(ui.cursor_volt_at(ui.m.cur_c), "V")));
    let (x, y) = (area.x + area.w / 2, area.y + (ui.m.cur_c * area.h as f32) as i32);
    ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
    ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x, y: y + 14 });
    ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y: y + 14 });
    assert!(ui.m.cur_c > 0.3, "the visible Y cursor must be draggable on the reference lane");
    assert_eq!((ui.m.cur_a, ui.m.cur_b, ui.m.cur_d), (0.2, 0.8, 0.7));
    assert_eq!((ui.m.ch[0].vscale, ui.m.ch[0].offset), channel);
    assert_eq!((ui.m.run, ui.m.trigger_wave(), ui.replay.captures, ui.wfm_total), acquisition);
    assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics);
    assert_eq!(ui.last[0].as_ref().unwrap().ch[0].as_ptr(), accepted);
    assert_eq!(fence.state(&ui), OperationState::Pending, "cursor-only interaction must not rearm or cancel the waiting capture");
}

#[test]
fn duplicate_measurement_add_preserves_genuine_statistics_and_pending_single() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..4 { ui.step(0.05); }
    let before = ui.measurement_statistics_snapshot_for_test();
    assert!(before.iter().any(|s| s.0 >= 3));
    let (kind, source) = ui.m.meas[0];
    ui.key(key::SINGLE);
    let fence = ui.scpi_operations();
    assert_eq!(ui.meas_add(kind, source), Ok(false));
    assert_eq!(ui.measurement_statistics_snapshot_for_test(), before);
    assert_eq!(fence.state(&ui), OperationState::Pending);
    let pair = (0, WaveId::Ch(0), WaveId::Ref(0));
    ui.m.meas2.push(pair);
    assert_eq!(ui.meas_add_pair(pair.0, pair.1, pair.2), Ok(false));
    assert_eq!(ui.measurement_statistics_snapshot_for_test(), before);
    assert_eq!(fence.state(&ui), OperationState::Pending);
}

#[test]
fn remote_clear_cancels_deferred_statistics_before_same_row_is_readded() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    ui.set_offline(true);
    ui.refs[0] = Some(reference((0..3_000_000).map(|i| if i % 20 < 10 { 1.0 } else { -1.0 }).collect(), 1e-9, 0.0));
    ui.m.meas = vec![(Meas::Period, WaveId::Ref(0))];
    ui.m.meas2.clear(); ui.m.cat.clear();
    ui.m.meas_full_record = true;
    ui.m.set_trigger_wave(WaveId::Ref(0), false);
    ui.m.trig_level = 0.0;
    ui.m.trig_mode = TrigMode::Single; ui.m.run = RunState::Run; ui.sim_play = true;
    let capture_deadline = Instant::now() + Duration::from_secs(10);
    while ui.replay.captures == 0 {
        ui.step(0.02);
        assert!(Instant::now() < capture_deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(ui.replay.captures, 1);
    assert!(ui.measurement_busy());
    assert_eq!(ui.mstat[0].n, 0);
    assert!(ui.meas_clear_remote());
    assert!(!ui.meas_clear_remote(), "clearing an empty selection is idempotent");
    assert!(ui.mstat.is_empty());
    assert_eq!(ui.meas_add(Meas::Period, WaveId::Ref(0)), Ok(true));
    let deadline = Instant::now() + Duration::from_secs(10);
    while ui.measurement_busy() {
        ui.poll_reference_measurements();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    ui.finish_deferred_capture_statistics();
    assert!((ui.meas_number(WaveId::Ref(0), Meas::Period) - 20e-9).abs() < 1e-14);
    assert!(ui.mstat.iter().all(|s| s.n == 0), "old completion must not count as a new acquisition for the readded row");
    assert_eq!(ui.replay.captures, 1);
}

#[test]
fn rejected_reference_edits_do_not_change_the_selected_slot_or_loaded_data() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    ui.refs[0] = Some(reference(vec![0.0, 1.0], 1e-9, 0.0));
    ui.ref_sel = 0;
    let owner = Arc::clone(&ui.refs[0].as_ref().unwrap().v);
    assert!(!ui.ref_vscale_set(1, 0.5));
    assert!(!ui.ref_voffset_set(1, 0.1));
    assert!(!ui.ref_label_set(1, "empty".into()));
    assert!(!ui.ref_show_slot(1, true));
    assert!(!ui.ref_vscale_set(0, f32::INFINITY));
    assert_eq!(ui.ref_sel, 0);
    assert!(Arc::ptr_eq(&owner, &ui.refs[0].as_ref().unwrap().v));
}
