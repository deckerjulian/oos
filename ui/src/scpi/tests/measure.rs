// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of `:MEASure`, `:CURSor` and the eye page's figures (scpi/measure.rs, scpi/cursor.rs, scpi/analysis.rs). A
//! test of a new command of these goes here; its query also goes into ui/tests/scpi.txt, its line into docs/scpi.md
//! (AGENTS.md).

use super::*;

#[test]
fn selected_catalog_rows_are_exported_and_setup_clear_accepts_an_empty_catalog() {
    let (mut s, mut ui, cv) = session();
    for amplitude in [2.0, 0.5] {
        let mut csv = String::from("Time (s),Volts\n");
        for i in 0..1000 {
            csv.push_str(&format!(
                "{:.12e},{}\n",
                i as f64 * 1e-6,
                amplitude * (i as f64 * std::f64::consts::TAU / 20.0).sin()
            ));
        }
        ui.load_waveform_auto(csv.as_bytes(), "power.csv").unwrap();
    }
    ui.set_offline(true);
    ui.m.meas_full_record = true;
    ui.m.meas.clear();
    ui.m.meas2.clear();
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:SET cat = 90:R1:R2:3:4;:SYST:ERR?"), "0,\"No error\"");
    let end = std::time::Instant::now() + Duration::from_secs(10);
    let answer = loop {
        let answer = line(&mut s, &mut ui, &cv, ":MEAS:STRIP?");
        let value = answer.rsplit(',').next().unwrap().parse::<f64>().unwrap();
        if value.abs() < 1e30 {
            assert!((value - 0.5).abs() < 0.01, "{answer}");
            break answer;
        }
        assert!(std::time::Instant::now() < end, "{answer}");
        ui.step(0.0);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(answer.starts_with("CAT90,R1,R2,CHAN3,CHAN4,"), "{answer}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:READOUT?"), answer);
    assert_eq!(captures(&ui), 0.0, "SCPI row exports are not acquisitions");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:SET cat = ;:SYST:ERR?"), "0,\"No error\"");
    assert!(ui.m.cat.is_empty());
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STRIP?"), "");
}

#[test]
fn strip_and_readout_export_the_configured_row_levels_and_physical_time() {
    let (mut s, mut ui, cv) = session();
    let mut csv = String::from("Time (s),Volts\n");
    for i in 0..800 {
        let k = i % 400;
        let v = match k {
            0..=99 => -0.5,
            100..=199 => -0.5 + (k - 100) as f64 / 99.0,
            200..=299 => 0.5,
            _ => 0.5 - (k - 300) as f64 / 99.0,
        };
        csv.push_str(&format!("{:.12e},{v}\n", 0.001 + i as f64 * 1e-9));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "row-levels.csv").unwrap();
    ui.m.run = RunState::Stop;
    ui.m.meas_full_record = true;
    let id = WaveId::Ref(0);
    ui.m.meas = vec![(Meas::Rise, id), (Meas::TVmax, id)];
    ui.m.meas2.clear();
    ui.set_meas_refs(0, 0.2, 0.8);
    ui.set_meas_refs(1, 0.2, 0.8);
    let shared = ui.meas_at(id, Meas::Rise);
    let own = ui.fact_meas(Meas::Rise, id);
    assert!(own < shared * 0.85, "row {own}, shared {shared}");
    let time = ui.fact_meas(Meas::TVmax, id);
    assert!((time - (0.001 + 199e-9)).abs() < 1e-13, "{time}");
    let expected = format!("{},R1,{};{},R1,{}", meas_token(Meas::Rise), e(own), meas_token(Meas::TVmax), e(time));
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STRIP?"), expected);
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:READOUT?"), expected);
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    assert_eq!(captures(&ui), 0.0);
}

#[test]
fn measurement_gating_screen_tracks_pan_while_none_measures_the_full_record() {
    let (mut s, mut ui, cv) = session();
    ui.set_offline(true);
    let mut csv = String::from("Time (s),Volts\n");
    for index in 0..4000 {
        let amplitude = if index < 2000 { 0.25 } else { 0.75 };
        let value = if index % 40 < 20 { -amplitude } else { amplitude };
        csv.push_str(&format!("{},{}\n", index as f64 * 1e-9, value));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "gate-regions.csv").unwrap();
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 1e-8;:TIM:OFFS 1e-6;:MEAS:GAT SCREEN");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:GATING?"), "SCREEN");
    let vpp = |s: &mut Scpi, ui: &mut Ui| line(s, ui, &cv, ":MEAS:ITEM? VPP,R1").parse::<f64>().unwrap();
    let first = vpp(&mut s, &mut ui);
    assert!(
        (first - 0.5).abs() < 1e-6,
        "first screen Vpp {first}, timebase {:?}, delay {}, zoom {}, state {}",
        ui.m.time_per_div(),
        ui.m.delay,
        ui.m.zoom,
        ui.workflow_state()
    );
    line(&mut s, &mut ui, &cv, ":TIM:OFFS 3e-6");
    assert!((vpp(&mut s, &mut ui) - 1.5).abs() < 1e-6, "SCREEN follows the stopped viewport");

    line(&mut s, &mut ui, &cv, ":MEAS:GATING NONE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:GAT?"), "NONE");
    assert!((vpp(&mut s, &mut ui) - 1.5).abs() < 1e-6);
    line(&mut s, &mut ui, &cv, ":TIM:OFFS 1e-6");
    assert!((vpp(&mut s, &mut ui) - 1.5).abs() < 1e-6, "NONE keeps the full record while panning");

    line(&mut s, &mut ui, &cv, ":MEAS:GAT SCREEN");
    assert!((vpp(&mut s, &mut ui) - 0.5).abs() < 1e-6, "restoring SCREEN recomputes the stopped gate");
    assert_eq!((ui.m.run, captures(&ui)), (RunState::Stop, 0.0), "gate changes and pan must not acquire");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");

    line(&mut s, &mut ui, &cv, ":MEAS:GAT unsupported");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:GAT?"), "SCREEN");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
}

#[test]
fn a_remote_eye_view_analyzes_a_stopped_reference_without_another_capture() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":SIM:PRES \"USB 2.0 HS\";:SIM:PLAY ON;:STOP");
    let acquired = captures(&ui);
    line(&mut s, &mut ui, &cv, ":VIEW EYE");
    let unit_interval: f64 = line(&mut s, &mut ui, &cv, ":EYE:ITEM? UI").parse().unwrap();
    assert!((unit_interval * 480e6 - 1.0).abs() < 0.02, "UI {unit_interval}");
    let edges: f64 = line(&mut s, &mut ui, &cv, ":EYE:ITEM? EDGES").parse().unwrap();
    assert!(edges > 1000.0, "Eye needs the full reference, got {edges} edges");
    assert_eq!(captures(&ui), acquired, "opening Eye must not acquire another window");
    assert_eq!(ui.m.run, RunState::Stop);
}

#[test]
fn cursors_and_a_readout_round_trip() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-6;:TIM:MAIN:OFFS 0;:REF:VSC 1,1;:REF:VOFF 1,0");
    line(
        &mut s,
        &mut ui,
        &cv,
        ":CURS:MODE MAN;:CURS:MAN:TYPE TIME;:CURS:MAN:SOUR R1;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6",
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?;:CURS:MAN:TYPE?;:CURS:MAN:SOUR?"), "MAN;TIME;R1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MAN:CAX?"), "0.000000E+00");
    let dx: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:XDEL?").parse().unwrap();
    let inv: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:IXD?").parse().unwrap();
    assert!((dx - 1e-6).abs() < 1e-9, "{dx}");
    assert!((inv - 1e6).abs() < 1.0, "{inv}");

    line(&mut s, &mut ui, &cv, ":CURS:MAN:TYPE AMPL;:CURS:MAN:CAY 0.5;:CURS:MAN:CBY -0.5");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?;:CURS:MAN:TYPE?"), "MAN;AMPL");
    let ay: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:AYV?").parse().unwrap();
    let dy: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:YDEL?").parse().unwrap();
    assert!((ay - 0.5).abs() < 1e-4, "{ay}");
    assert!((dy + 1.0).abs() < 1e-3, "{dy}");

    line(&mut s, &mut ui, &cv, ":CURS:MODE XY;:CURS:XY:AX 1e-6;:CURS:XY:AY 0.25");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?"), "XY");
    let ax: f64 = line(&mut s, &mut ui, &cv, ":CURS:XY:AX?").parse().unwrap();
    let y: f64 = line(&mut s, &mut ui, &cv, ":CURS:XY:AY?").parse().unwrap();
    assert!((ax - 1e-6).abs() < 1e-9 && (y - 0.25).abs() < 1e-4, "{ax} {y}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE OFF;:CURS:MAN:XDEL?"), "9.9E37");

    line(&mut s, &mut ui, &cv, ":MEAS:CLE;:MEAS:SOUR R1;:MEAS:ITEM VPP;:MEAS:ITEM RRDELAY,CHAN1,CHAN2");
    let strip = line(&mut s, &mut ui, &cv, ":MEAS:STR?");
    assert!(strip.contains("VPP,R1,"), "{strip}");
    assert!(strip.contains("RRDELAY,CHAN1,CHAN2,"), "{strip}");
    let vpp: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VPP").parse().unwrap();
    assert!((vpp - 1.0).abs() < 1e-3, "the saved square is 1 Vpp, got {vpp}");
    line(&mut s, &mut ui, &cv, ":MEAS:CLE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STR?"), "");

    line(
        &mut s,
        &mut ui,
        &cv,
        ":DVM:ENAB ON;:DVM:SOUR CHAN2;:DVM:MODE ACRMS;:COUN:ENAB ON;:COUN:MODE PER;:COUN:NDIG 6",
    );
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":DVM:ENAB?;:DVM:SOUR?;:DVM:MODE?;:COUN:ENAB?;:COUN:MODE?;:COUN:NDIG?"),
        "1;CHAN2;ACRMS;1;PER;6"
    );
    assert!(!line(&mut s, &mut ui, &cv, ":DVM:CURR?;:COUN:CURR?").is_empty());
    line(&mut s, &mut ui, &cv, ":COUN:TOT:CLE");
}

#[test]
fn cursor_setters_preserve_genuine_statistics_and_pending_single() {
    let (mut scpi, mut ui, canvas) = session();
    for _ in 0..4 {
        ui.step(0.05);
    }
    let statistics = ui.measurement_statistics_snapshot_for_test();
    assert!(statistics.iter().any(|&(count, _)| count >= 3));
    let record = ui.record_volts(0).to_vec();
    ui.m.trig_level = 100.0;
    ui.key(crate::ui::key::SINGLE);
    let stamp = ui.m.hw_stamp;
    let source = ui.m.trigger_wave();
    let mut waiting = Program::new("*OPC?;:RUN");
    assert!(scpi.poll_program(&mut ui, &canvas, &mut waiting).is_none());
    for command in [
        ":CURS:MODE MAN",
        ":CURS:MAN:TYPE TIME",
        ":CURS:MAN:SOUR CHAN1",
        ":CURS:MAN:CAX 0",
        ":CURS:MAN:CBX 1e-8",
        ":CURS:MAN:TYPE AMPL",
        ":CURS:MAN:CAY 0.2",
        ":CURS:MAN:CBY -0.2",
        ":CURS:MODE XY",
        ":CURS:XY:AX 0",
        ":CURS:XY:AY 0.1",
        ":CURS:XY:BX 1e-8",
        ":CURS:XY:BY -0.1",
        ":CURS:MODE TRAC",
        ":CURS:TRAC:SOUR2 CHAN1",
        ":CURS:MODE OFF",
    ] {
        line(&mut scpi, &mut ui, &canvas, command);
        assert_eq!(ui.measurement_statistics_snapshot_for_test(), statistics, "{command}");
        assert_eq!(ui.record_volts(0), record, "{command}");
        assert_eq!(ui.m.hw_stamp, stamp, "{command}");
        assert_eq!(ui.m.trigger_wave(), source);
        assert_eq!(ui.m.trig_mode, TrigMode::Single);
        assert!(scpi.poll_program(&mut ui, &canvas, &mut waiting).is_none(), "{command}");
    }
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn manual_cursors_keep_time_and_voltage_together() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-6;:TIM:MAIN:OFFS 0;:CHAN1:SCAL 1;:CHAN1:OFFS 0");
    line(
        &mut s,
        &mut ui,
        &cv,
        ":CURS:MODE MAN;:CURS:MAN:CAX 0;:CURS:MAN:CBX 1e-6;:CURS:MAN:CAY 0.5;:CURS:MAN:CBY -0.5",
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?"), "XY", "both axes are the screen's XY cursors");
    let ax: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAX?").parse().unwrap();
    let bx: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CBX?").parse().unwrap();
    let ay: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAY?").parse().unwrap();
    let by: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CBY?").parse().unwrap();
    assert!((ax).abs() < 1e-9 && (bx - 1e-6).abs() < 1e-9, "times {ax} {bx}");
    assert!((ay - 0.5).abs() < 1e-4 && (by + 0.5).abs() < 1e-4, "volts {ay} {by}");
    line(&mut s, &mut ui, &cv, ":CURS:MODE OFF;:CURS:MAN:TYPE AMPL;:CURS:MAN:CAY 0.25;:CURS:MAN:CAX 0");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MODE?"), "XY");
    let kept: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAY?").parse().unwrap();
    let moved: f64 = line(&mut s, &mut ui, &cv, ":CURS:MAN:CAX?").parse().unwrap();
    assert!((kept - 0.25).abs() < 1e-4 && moved.abs() < 1e-9, "voltage survived the time cursor: {kept} {moved}");
}

#[test]
fn track_cursor_b_follows_its_own_channel() {
    let (mut s, mut ui, cv) = session();
    line(
        &mut s,
        &mut ui,
        &cv,
        "*CLS;:CURS:MODE TRAC;:CURS:TRAC:SOUR1 CHAN1;:CURS:TRAC:SOUR2 CHAN2;:CURS:TRAC:CAX 0;:CURS:TRAC:CBX 1e-6",
    );
    for _ in 0..4 {
        ui.step(0.05);
    }
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:TRAC:SOUR1?;:CURS:TRAC:SOUR2?"), "CHAN1;CHAN2");
    let ay = line(&mut s, &mut ui, &cv, ":CURS:TRAC:AYV?");
    let by = line(&mut s, &mut ui, &cv, ":CURS:TRAC:BYV?");
    assert!(ay.parse::<f64>().unwrap().is_finite() && by.parse::<f64>().unwrap().is_finite(), "{ay} {by}");
    line(&mut s, &mut ui, &cv, "*CLS;:CURS:MODE MAN;:CURS:MAN:SOUR2 CHAN2");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224"), "manual mode keeps one source");
}

#[test]
fn statistics_channel_labels_and_the_zoom_window_round_trip() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":CHAN1:LAB:CONT Vbus;:CHAN1:UNIT AMP;:CHAN1:VERN ON;:CHAN1:POS 0.2");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":CHAN1:LAB:CONT?;:CHAN1:LAB:SHOW?;:CHAN1:UNIT?;:CHAN1:VERN?"),
        "Vbus;1;AMP;1"
    );
    let pos: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:OFFS?").parse().unwrap();
    assert!((pos - 0.2).abs() < 1e-4, "{pos}");
    line(&mut s, &mut ui, &cv, ":CHAN1:LAB:SHOW OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:LAB:SHOW?;:CHAN1:LAB:CONT?"), "0;");

    line(&mut s, &mut ui, &cv, ":TIM:MAIN:SCAL 1e-3;:TIM:HREF:MODE TRIG;:TIM:VERN ON;:TIM:DEL:ENAB ON;:TIM:DEL:SCAL 1e-4;:TIM:DEL:OFFS 0;:TIM:XY:ENAB ON");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:HREF:MODE?;:TIM:VERN?;:TIM:DEL:ENAB?;:TIM:XY:ENAB?"), "TRIG;1;1;1");
    let z: f64 = line(&mut s, &mut ui, &cv, ":TIM:DEL:SCAL?").parse().unwrap();
    assert!((z - 1e-4).abs() < 1e-8, "{z}");

    line(&mut s, &mut ui, &cv, ":MEAS:CLE;:MEAS:STAT:DISP ON;:MEAS:STAT:ITEM VPP,R1;:RUN");
    ui.step(0.25);
    ui.step(0.25);
    let current: f64 = line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? CURR,VPP,R1").parse().unwrap();
    let mean: f64 = line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? AVER,VPP,R1").parse().unwrap();
    let n: f64 = line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? CNT,VPP,R1").parse().unwrap();
    assert!((current - 1.0).abs() < 1e-3, "{current}");
    assert!((mean - 1.0).abs() < 1e-3 && n >= 2.0, "mean {mean} count {n}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STAT:DISP?;:MEAS:STR?"), "1;VPP,R1,1.000000E+00");
    line(&mut s, &mut ui, &cv, ":MEAS:STAT:RES");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:STAT:ITEM? CNT,VPP,R1"), "0.000000E+00");
}

#[test]
fn measurement_gate_changes_burst_outside_and_duration() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 1e-3");
    for _ in 0..8 {
        ui.step(0.05);
    }
    line(&mut s, &mut ui, &cv, "*CLS;:MEAS:BURS:IDLE 0;:MEAS:OUTS BOTH;:MEAS:NPER 1;:MEAS:EDG CLOC");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:OUTS?;:MEAS:NPER?;:MEAS:EDG?"), "BOTH;1;CLOC");
    let idle: f64 = line(&mut s, &mut ui, &cv, ":MEAS:BURS:IDLE?").parse().unwrap();
    assert!(idle.abs() < 1e-15, "{idle}");
    let one: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? NPER,CHAN1").parse().unwrap();
    line(&mut s, &mut ui, &cv, ":MEAS:NPER 2");
    let two: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? NPER,CHAN1").parse().unwrap();
    assert!(one.is_finite() && one < 1e37 && (two / one - 2.0).abs() < 0.15, "one {one} two {two}");
    line(&mut s, &mut ui, &cv, ":MEAS:NPER 1;:MEAS:EDG DATA");
    let data: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? NPER,CHAN1").parse().unwrap();
    assert!((data / one - 0.5).abs() < 0.15, "clock {one} data {data}");
    let burst: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? BWID,CHAN1").parse().unwrap();
    assert!(burst.is_finite() && burst < 1e37, "{burst}");
    line(&mut s, &mut ui, &cv, ":MEAS:BURS:IDLE 1e-6");
    let split: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? BWID,CHAN1").parse().unwrap();
    assert!(split > 1e37, "a gap shorter than the half period splits the burst, {split}");
    let mut csv = String::from("Time (s),Volts\n");
    for index in 0..2000 {
        let value = if index % 100 < 10 { 1.0 } else { 0.0 };
        csv.push_str(&format!("{},{}\n", index as f64 * 1e-6, value));
    }
    ui.load_waveform_bytes(csv.as_bytes(), "pulse.csv").unwrap();
    line(&mut s, &mut ui, &cv, ":MEAS:OUTS ABOV");
    let above: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? TOUT,R1").parse().unwrap();
    line(&mut s, &mut ui, &cv, ":MEAS:OUTS BEL");
    let below: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? TOUT,R1").parse().unwrap();
    assert!(above > 0.0 && below > above * 5.0, "a 10 % pulse is high briefly, above {above} below {below}");
    line(&mut s, &mut ui, &cv, ":MEAS:NPER 0;:MEAS:BURS:IDLE -1;:MEAS:EDG BOTH");
    let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(err.starts_with("-224,"), "{err}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:NPER?;:MEAS:EDG?"), "1;DATA");
}
