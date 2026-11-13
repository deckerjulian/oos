//! Explicit real-file acceptance. These tests open no native window or service.
use super::*;
use std::{path::Path, sync::Arc, time::{Duration, Instant}};

fn output(ui: &mut Ui, name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../local/ref-integration/actual-backend");
    std::fs::create_dir_all(&dir).unwrap();
    ui.toast = None;
    let mut canvas = Canvas::new(W, H);
    ui.render(&mut canvas);
    crate::store::write(&dir.join(name), &crate::store::png_of(&canvas)).unwrap();
}

#[test]
#[ignore = "Reads the user's actual D-drive Gen7 files; explicit local acceptance only"]
fn actual_gen7_reference_math_zoom_and_automatic_eye_backend() {
    let started = Instant::now();
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    for name in ["Gen7_65GB_CP_Q6_n_Ch2.wfm", "Gen7_65GB_CP_Q6_p_Ch1.wfm"] {
        let bytes = std::fs::read(Path::new("D:/").join(name)).unwrap();
        ui.load_waveform_auto(&bytes, name).unwrap();
    }
    ui.set_offline(true);
    ui.m.run = RunState::Stop;
    ui.m.math = MathOp::Sub;
    ui.m.math_a = MATH_REF_BASE + 1;
    ui.m.math_b = MATH_REF_BASE;
    let dispatched = Instant::now();
    assert!(ui.math_record(0).unwrap().is_none());
    let dispatch_ms = dispatched.elapsed().as_secs_f64() * 1000.0;
    let deadline = Instant::now() + Duration::from_secs(30);
    let math = loop {
        if let Some(w) = ui.math_record(0).unwrap() { break w; }
        assert!(Instant::now() < deadline, "20M math did not complete");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(math.v.len(), 20_000_000);
    assert_eq!(math.dt, 5e-12);
    for i in [0, 1, 127, 1024, 1_000_000, 10_000_000, 19_999_999] {
        assert_eq!(math.v[i].to_bits(), (ui.refs[1].as_ref().unwrap().v[i] - ui.refs[0].as_ref().unwrap().v[i]).to_bits());
    }
    ui.m.math_scale = math_scales().iter().position(|&v| v >= 0.1).unwrap();
    ui.m.tscale = tscales().iter().position(|&t| (t - 2e-9).abs() < 1e-16).unwrap();
    ui.m.delay = 0.0;
    ui.m.zoom = false;
    ui.relayout();
    output(&mut ui, "01-gen7-reference-math-overlay.png");
    ui.workspace.stacked = true;
    ui.set_zoom(true);
    ui.m.zscale = tscales().iter().position(|&t| (t - 100e-12).abs() < 1e-18).unwrap();
    ui.m.zdelay = 0.0;
    ui.zoom_vertical_step(1, ui.area, 0.5);
    ui.relayout();
    output(&mut ui, "02-gen7-reference-math-stacked-zoom.png");
    ui.m.zdelay = 1e-9;
    assert!(Arc::ptr_eq(&math, &ui.math_record(0).unwrap().unwrap()), "pan must not recompute full-record math");
    ui.set_zoom(false);
    ui.workspace.stacked = false;
    ui.select_workspace_source(WaveId::Ref(1));
    ui.eye_make();
    let eye_deadline = Instant::now() + Duration::from_secs(30);
    while ui.azr.eye_auto.applied_results == 0 && ui.azr.eye_auto.summary.is_none() {
        ui.step(0.02);
        if ui.azr.page.title.contains("No eye") { break; }
        assert!(Instant::now() < eye_deadline, "Auto Eye did not settle");
        std::thread::sleep(Duration::from_millis(3));
    }
    output(&mut ui, "03-gen7-auto-eye.png");
    assert_eq!(ui.m.eye_src, WaveId::Ref(1));
    assert_eq!(ui.m.run, RunState::Stop);
    assert_eq!(ui.replay.captures, 0);
    let report = format!("# Actual Gen7 backend acceptance\n\nBoth 20M n/p files loaded at native5ps. R2-R1 math has20M points; seven values match independently selected source samples bit for bit. Math dispatch: {dispatch_ms:.3}ms. Pan reused the same cached Arc. Stacked zoom includes both refs and Math, with independent display vertical zoom. Auto Eye source isR2; status: {}. Total local runtime: {:.3}s. No window/service/OS input/live-app API used.\n", ui.azr.page.title, started.elapsed().as_secs_f64());
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join("../local/ref-integration/actual-backend/gen7.md"), report).unwrap();
}

#[test]
#[ignore = "Reads the user's actual D-drive C-PHY trio; does not operate TekScope"]
fn actual_cphy_reference_trio_decode_backend() {
    let mut ui = Ui::new(Source::Demo { t: 0.0 });
    for name in ["VA_SE.wfm", "VB_SE.wfm", "VC_SE.wfm"] {
        let bytes = std::fs::read(Path::new("D:/WFMs/WFMs").join(name)).unwrap();
        ui.load_waveform_auto(&bytes, name).unwrap();
    }
    ui.set_offline(true);
    ui.m.run = RunState::Stop;
    ui.m.dec_proto = Proto::Cphy;
    ui.m.dec_on = true;
    ui.m.dec_src = DEC_REF_BASE;
    ui.m.dec_src2 = DEC_REF_BASE + 1;
    ui.m.dec_cs = Some(DEC_REF_BASE + 2);
    ui.m.dec_thr = 0.02;
    ui.m.dec_cphy_rate = 2.25e9;
    let packets = ui.decode_packets_of(&ui.m).unwrap();
    assert!(!packets.is_empty());
    assert!(packets.iter().any(|p| p.ok && matches!(p.kind, oos_dsp::decode::Kind::Field("SYM" | "SYNC") | oos_dsp::decode::Kind::Word { .. })), "actual trio must yield supported electrical information");
    let words = packets.iter().filter(|p| p.kind == oos_dsp::decode::Kind::Word { cmd: false }).count();
    let syncs = packets.iter().filter(|p| p.kind == oos_dsp::decode::Kind::Field("SYNC")).count();
    let errors = packets.iter().filter(|p| !p.ok).count();
    ui.m.trig_type = TrigType::Bus;
    ui.m.trig_bus = BusWhat::Frame;
    ui.set_trigger_source(WaveId::Ref(0), false);
    let expected_hit = packets.iter().any(|p| crate::trig::matches(&ui.m, p));
    ui.key(key::SINGLE);
    for _ in 0..3 { ui.step(0.02); }
    assert_eq!(ui.replay.captures, u64::from(expected_hit));
    if !expected_hit {
        assert_eq!(ui.m.status, TrigStatus::Wait, "unaligned symbols must not invent a packet trigger");
        ui.key(key::FORCE);
        ui.step(0.02);
        assert_eq!(ui.replay.captures, 1, "Force explicitly accepts one stored capture");
    }
    assert_eq!(ui.m.run, RunState::Stop);
    ui.m.tscale = tscales().iter().position(|&t| (t - 2e-9).abs() < 1e-16).unwrap();
    ui.m.delay = 0.0;
    ui.m.zoom = false;
    ui.relayout();
    output(&mut ui, "04-cphy-reference-trio-decode.png");
    let report = format!("# Actual C-PHY trio backend\n\nVA/VB/VC original native dt/t0 retained; known symbol-rate override2.25Gsym/s. {} electrical fields, {words} aligned16-bit words, {syncs} qualified SYNCs, {errors} error fields. Frame-trigger predicate hit={expected_hit}; Single matched that actual predicate, otherwise stayedWAIT until explicitForce accepted once and stopped. A record without qualified alignment exposes symbols without inventing words. This is six-wirestate, single-trio decoding; no CSI/DSI packet interpretation, multi-trio merging or18-wirestate claim. Final STOP/captures: {:?}/{}. No native window or TekScope commands were used.\n", packets.len(), ui.m.run, ui.replay.captures);
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join("../local/ref-integration/actual-backend/cphy.md"), report).unwrap();
}

#[test]
fn full_eye_has_optional_panes_compact_placement_picker_and_live_return() {
    let mut ui=Ui::new(Source::Demo{t:0.0});
    assert_eq!(ui.m.eye_place,EyePlace::Full);
    ui.m.mdepth=0;ui.step(0.02);ui.m.run=RunState::Stop;
    ui.activate(Item::EyeAct);ui.render(&mut Canvas::new(W,H));
    assert!(ui.eye_open());let full=ui.eye_page_rect();
    assert!(full.w>=700 && full.h>=300,"{full:?}");
    let layout=eyepage::layout(full,false);assert!(layout.fig_rows>0 && layout.jit_rows>0);
    let place=ui.eye_place_control();ui.eye_header_tap(place.x+2,place.y+2);
    assert!(ui.pick.as_ref().is_some_and(|picker|picker.it==Item::EyePlace));
    ui.close_menu();
    for where_ in [EyePlace::Side,EyePlace::Under,EyePlace::Full] {
        ui.eye_set_place(where_);ui.render(&mut Canvas::new(W,H));
        assert_eq!(ui.m.eye_place,where_);assert!(ui.eye_page_rect().w>200);
    }
    let close=ui.eye_pane_close();assert!(ui.eye_tap(close.x+2,close.y+2));assert!(!ui.eye_open());
    ui.refs[0]=Some(RefWave{src:0,color:0,label:"test".into(),t0:0.0,dt:1e-9,v:vec![0.0,1.0,0.0],vpd:0.2,offset:0.0,on:true});
    ui.set_offline(true);
    let live=ui.top_buttons().into_iter().find(|b|b.0==TopBtn::Live).expect("Live return");
    ui.tap(live.2.x+4,live.2.y+4);assert!(!ui.record_mode());
    assert!(!ui.top_buttons().iter().any(|b|b.0==TopBtn::Live));
    ui.set_offline(true);assert_eq!(ui.sim_items()[0],Item::SimLive);
    ui.activate(Item::SimLive);assert!(!ui.record_mode());
    for value in ["Full","Side","Under"] {
        crate::store::set(&mut ui.m,"eye_place",value).unwrap();
        let encoded=crate::store::setup_text(&ui.m);let m=crate::store::parse_setup(&encoded).unwrap();assert_eq!(m.eye_place,ui.m.eye_place);
    }
}

#[test]
fn full_eye_owns_plot_input_and_keeps_close_clear_of_the_corner_button() {
    let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.mdepth=0;ui.step(0.02);
    ui.set_zoom(true);ui.m.view=AView::Eye;ui.m.eye_place=EyePlace::Full;ui.relayout();
    let before=(ui.m.tscale,ui.m.delay,ui.m.zscale,ui.m.zdelay,ui.zoom_vfactor,ui.m.zoom);
    let a=ui.eye_page_rect();assert!(!ui.pointer_wheel(a.x+a.w/2,a.y+a.h/2,1,false));
    assert!(!ui.pointer_wheel(a.x+a.w/2,a.y+a.h/2,1,true));
    let hidden=ui.zoom_controls().iter().find(|(_,name)|*name=="H+").unwrap().0;
    ui.tap(hidden.x+2,hidden.y+2);
    assert_eq!((ui.m.tscale,ui.m.delay,ui.m.zscale,ui.m.zdelay,ui.zoom_vfactor,ui.m.zoom),before);
    let close=ui.eye_pane_close();assert!(!overlaps(close,ui.full_btn().1));
    ui.tap(close.x+4,close.y+4);assert!(!ui.eye_open());assert!(ui.m.zoom);
}
