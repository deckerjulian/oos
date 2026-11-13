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
