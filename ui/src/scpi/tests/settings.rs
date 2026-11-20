// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of the settings of `:CHANnel`, `:TRIGger`, `:ACQuire` (scpi/dispatch.rs), `:MASK`, `:HISTogram`, `:MATH`
//! and `:DISPlay`. A test of a new command of these goes here, or into the file of its subsystem in this folder
//! when it gets one; its query also goes into ui/tests/scpi.txt, its line into docs/scpi.md (AGENTS.md).

use super::*;
use crate::settings::testkit::{changed, instances, GROUPS};

#[test]
fn digital_trigger_source_queries_preserve_identity_and_secondary_channel() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":LA:STAT ON;:TRIG:EDGE:SOUR D7");
    assert_eq!(ui.m.trig_line(), Some(7), "D7 must be the active digital source");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR?;:TRIG:SOUR?;:TRIG:SOUR2?"), "D7;D7;CHAN4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");

    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR CHAN2");
    assert_eq!(ui.m.trig_line(), None);
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR?;:TRIG:SOUR2?"), "CHAN2;CHAN4");
}

#[test]
fn reference_trigger_source_query_takes_precedence_over_a_stale_la_selection() {
    let (mut s, mut ui, cv) = reference_session();
    ui.m.la.on = true;
    ui.m.trig_la = Some(7);
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:EDGE:SOUR?;:TRIG:SOUR?"), "R1;R1");
}

#[test]
fn channel_zero_bias_shifts_the_reading() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..8 {
        ui.step(0.05);
    }
    let before: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VAVG,CHAN1").parse().unwrap();
    assert!(before.is_finite() && before < 1e37, "{before}");
    line(&mut s, &mut ui, &cv, ":CHAN1:BIAS 0.25");
    for _ in 0..4 {
        ui.step(0.05);
    }
    let after: f64 = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VAVG,CHAN1").parse().unwrap();
    assert!((before - after - 0.25).abs() < 0.05, "before {before} after {after}");
}

#[test]
fn acquisition_deskew_reference_color_and_thresholds_round_trip() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":ACQ:TYPE AVER;:ACQ:AVER 100;:ACQ:ULTR:MAXF 100");
    assert_eq!(line(&mut s, &mut ui, &cv, ":ACQ:TYPE?;:ACQ:AVER?;:ACQ:ULTR:MAXF?"), "AVER;64;100");
    line(&mut s, &mut ui, &cv, ":ACQ:TYPE HRES;:ACQ:ULTR:MAXF 0");
    assert_eq!(line(&mut s, &mut ui, &cv, ":ACQ:TYPE?;:ACQ:ULTR:MAXF?"), "HRES;0");

    line(&mut s, &mut ui, &cv, ":CHAN1:BIAS 0.25");
    let bias: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:BIAS?").parse().unwrap();
    assert!((bias - 0.25).abs() < 1e-6, "{bias}");
    line(&mut s, &mut ui, &cv, "*CLS;:CHAN1:BIAS nan");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "a non-finite zero bias is refused");
    let held_bias: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:BIAS?").parse().unwrap();
    assert!((held_bias - 0.25).abs() < 1e-6, "{held_bias}");
    line(&mut s, &mut ui, &cv, ":CHAN1:BIAS 0");

    line(&mut s, &mut ui, &cv, "*CLS;:CHAN1:OFFS 0.5;:CHAN1:SCAL 1;:CHAN1:EXP GND");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:EXP?;:CHAN2:EXP?"), "GND;GND");
    line(&mut s, &mut ui, &cv, ":CHAN1:SCAL 2");
    let grown: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:OFFS?").parse().unwrap();
    assert!((grown - 1.0).abs() < 1e-4, "ground expand keeps the zero in place, offset {grown}");
    line(&mut s, &mut ui, &cv, ":CHAN1:EXP CENT;:CHAN1:SCAL 1");
    let held: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:OFFS?").parse().unwrap();
    assert!((held - 1.0).abs() < 1e-4, "center expand leaves the offset, {held}");
    line(&mut s, &mut ui, &cv, ":CHAN1:EXP FOO");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(line(&mut s, &mut ui, &cv, ":CHAN1:EXP?"), "CENT");

    line(&mut s, &mut ui, &cv, ":CHAN1:TCAL 2e-8");
    let skew: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:TCAL?").parse().unwrap();
    assert!((skew - 2e-8).abs() < 1e-12, "{skew}");
    line(&mut s, &mut ui, &cv, ":TIM:SCAL 2e-5;:CHAN1:TCAL 1e-9");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "a slow time base refuses a deskew change");
    let held: f64 = line(&mut s, &mut ui, &cv, ":CHAN1:TCAL?").parse().unwrap();
    assert!((held - 2e-8).abs() < 1e-12, "{held}");

    line(&mut s, &mut ui, &cv, ":REF:VSC 1,0.5;:REF:VOFF 1,0.1;:REF:RES 1;:REF:COL 1,RED");
    let vsc: f64 = line(&mut s, &mut ui, &cv, ":REF:VSC? 1").parse().unwrap();
    let voff: f64 = line(&mut s, &mut ui, &cv, ":REF:VOFF? 1").parse().unwrap();
    assert!((vsc - 1.0).abs() < 1e-6 && voff.abs() < 1e-9, "reset scale {vsc} offset {voff}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:COL? 1"), "RED");

    line(&mut s, &mut ui, &cv, "*CLS;:MEAS:SET:PSA CHAN2;:MEAS:SET:PSB R1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:DSA?;:MEAS:SET:DSB?;:MEAS:SOUR?"), "CHAN2;R1;CHAN2");
    line(&mut s, &mut ui, &cv, ":MEAS:SET:PSA MATH1");
    assert!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"),
        "a measurement source is a channel or a reference"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SOUR?"), "CHAN2");

    line(&mut s, &mut ui, &cv, ":MEAS:SET:MIN 20;:MEAS:SET:MAX 80");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:MIN?;:MEAS:SET:MAX?"), "20;80");
    line(&mut s, &mut ui, &cv, ":MEAS:SET:MAX 10");
    assert!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"),
        "the upper threshold must stay above the lower"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:SET:MAX?"), "80");

    // the runt thresholds the same way (review U6)
    line(&mut s, &mut ui, &cv, "*CLS;:TRIG:TYPE RUNT;:TRIG:RUNT:UPP 1.5;:TRIG:RUNT:LOW 0.25");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("0,"), "thresholds in order are taken");
    line(&mut s, &mut ui, &cv, ":TRIG:RUNT:LOW 2");
    let error = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(error.starts_with("-224,"), "the lower threshold must stay below the upper: {error}");
    line(&mut s, &mut ui, &cv, ":TRIG:RUNT:UPP 0.1");
    let error = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(error.starts_with("-224,"), "the upper threshold must stay above the lower: {error}");
    let upper: f64 = line(&mut s, &mut ui, &cv, ":TRIG:RUNT:UPP?").parse().unwrap();
    let lower: f64 = line(&mut s, &mut ui, &cv, ":TRIG:RUNT:LOW?").parse().unwrap();
    assert!((upper - 1.5).abs() < 1e-6 && (lower - 0.25).abs() < 1e-6, "{upper} {lower}");
}

#[test]
fn a_mask_counts_passes_and_failures() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..20 {
        ui.step(0.05);
    }
    line(&mut s, &mut ui, &cv, "*CLS;:MASK:SOUR CHAN1;:MASK:X 0.5;:MASK:Y 0.5;:MASK:ENAB ON;:MASK:CRE;:MASK:OPER RUN");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?"),
        "0,\"No error\"",
        "the demo record must be there to build a mask"
    );
    assert_eq!(line(&mut s, &mut ui, &cv, ":MASK:ENAB?;:MASK:SOUR?;:MASK:OPER?"), "1;CHAN1;RUN");
    let x: f64 = line(&mut s, &mut ui, &cv, ":MASK:X?").parse().unwrap();
    let y: f64 = line(&mut s, &mut ui, &cv, ":MASK:Y?").parse().unwrap();
    assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6, "{x} {y}");
    for _ in 0..10 {
        ui.step(0.05);
    }
    let passed: u64 = line(&mut s, &mut ui, &cv, ":MASK:PASS?").parse().unwrap();
    let failed: u64 = line(&mut s, &mut ui, &cv, ":MASK:FAIL?").parse().unwrap();
    let total: u64 = line(&mut s, &mut ui, &cv, ":MASK:TOT?").parse().unwrap();
    assert_eq!(total, passed + failed);
    assert!(total > 0, "a running mask must count frames, pass {passed} fail {failed}");
    line(&mut s, &mut ui, &cv, ":MASK:RES");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MASK:PASS?;:MASK:FAIL?;:MASK:TOT?"), "0;0;0");
    line(&mut s, &mut ui, &cv, ":MASK:OPER STOP");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MASK:OPER?"), "STOP");
}

#[test]
fn histogram_type_source_and_result() {
    let (mut s, mut ui, cv) = session();
    for _ in 0..8 {
        ui.step(0.05);
    }
    line(&mut s, &mut ui, &cv, "*CLS;:HIST:ENAB ON;:HIST:TYPE VERT;:HIST:SOUR CHAN1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":HIST:ENAB?;:HIST:TYPE?;:HIST:SOUR?"), "1;VERT;CHAN1");
    line(&mut s, &mut ui, &cv, ":HIST:TYPE HOR");
    assert_eq!(line(&mut s, &mut ui, &cv, ":HIST:TYPE?"), "HOR");
    let result = line(&mut s, &mut ui, &cv, ":HIST:STAT:RES?");
    let sum: u64 = result.split([',', ':']).nth(1).unwrap_or("0").parse().unwrap_or(0);
    assert!(sum > 0 && result.contains("Mean:") && result.contains("Sigma:"), "{result}");
    line(&mut s, &mut ui, &cv, ":HIST:ENAB OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":HIST:ENAB?"), "0");
    let off = line(&mut s, &mut ui, &cv, ":HIST:STAT:RES?");
    assert!(off.starts_with("Sum:0,"), "{off}");
}

#[test]
fn a_math_trace_sets_operator_scale_fft_and_filter() {
    let (mut s, mut ui, cv) = session();
    line(
        &mut s,
        &mut ui,
        &cv,
        "*CLS;:MATH1:DISP ON;:MATH1:OPER ADD;:MATH1:SOUR1 CHAN1;:MATH1:SOUR2 CHAN2;:MATH1:SCAL 0.2;:MATH1:OFFS 1",
    );
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":MATH1:DISP?;:MATH1:OPER?;:MATH1:SOUR1?;:MATH1:SOUR2?"),
        "1;ADD;CHAN1;CHAN2"
    );
    let scale: f64 = line(&mut s, &mut ui, &cv, ":MATH1:SCAL?").parse().unwrap();
    let offset: f64 = line(&mut s, &mut ui, &cv, ":MATH1:OFFS?").parse().unwrap();
    assert!((scale - 0.2).abs() < 1e-9 && (offset - 1.0).abs() < 1e-6, "{scale} {offset}");
    line(&mut s, &mut ui, &cv, ":MATH2:OPER INTG;:MATH2:SOUR1 MATH1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH2:OPER?;:MATH2:SOUR1?"), "INTG;MATH1");
    line(&mut s, &mut ui, &cv, ":MATH1:SOUR1 MATH1");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224"), "a trace cannot use itself");
    line(&mut s, &mut ui, &cv, ":MATH1:FFT:SOUR CHAN1;:MATH1:FFT:WIND BLAC;:MATH1:FFT:SCAL 10");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:OPER?;:MATH1:FFT:SOUR?;:MATH1:FFT:WIND?"), "FFT;CHAN1;BLAC");
    let db: f64 = line(&mut s, &mut ui, &cv, ":MATH1:FFT:SCAL?").parse().unwrap();
    assert!((db - 10.0).abs() < 1e-9, "{db}");
    line(&mut s, &mut ui, &cv, ":MATH1:FILT:TYPE BPAS;:MATH1:FILT:W1 1000;:MATH1:FILT:W2 3000");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:FILT:TYPE?;:MATH1:OPER?"), "BPAS;BPAS");
    let w1: f64 = line(&mut s, &mut ui, &cv, ":MATH1:FILT:W1?").parse().unwrap();
    let w2: f64 = line(&mut s, &mut ui, &cv, ":MATH1:FILT:W2?").parse().unwrap();
    assert!((w1 - 1000.0).abs() < 1e-3 && (w2 - 3000.0).abs() < 1e-3, "{w1} {w2}");
    line(&mut s, &mut ui, &cv, ":MATH1:EXPR \"2*CH1-CH2\"");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:EXPR?;:MATH1:OPER?"), "2*CH1-CH2;EXPR");
    line(&mut s, &mut ui, &cv, ":MATH1:DISP OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MATH1:DISP?;:MATH1:OPER?"), "0;OFF");
}

#[test]
fn a_live_force_trigger_asks_the_hardware_thread_to_pulse() {
    let shared: crate::board::HwShared = Default::default();
    let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
    ui.m.run = RunState::Stop;
    let mut s = Scpi::open(0);
    let cv = Canvas::new(crate::ui::W, crate::ui::H);
    line(&mut s, &mut ui, &cv, ":TFOR");
    assert!(shared.lock().unwrap().force, "the acquisition thread pulses the FPGA force bit");
    assert_eq!(ui.m.run, RunState::Run);
}

#[test]
fn display_type_persistence_brightness_and_grid() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, "*CLS;:DISP:TYPE VECT;:DISP:GRAD:TIME 0.1;:DISP:WBR 50;:DISP:GRID HALF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:TYPE?;:DISP:GRAD:TIME?;:DISP:WBR?;:DISP:GRID?"), "VECT;0.1;50;HALF");
    line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME 0.2");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME?"), "0.1", "0.2 s snaps to the screen's 100 ms step");
    line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME INF;:DISP:TYPE DOTS;:DISP:GRID NONE;:DISP:WBR 7");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GRAD:TIME?;:DISP:TYPE?;:DISP:GRID?;:DISP:WBR?"), "INF;DOTS;NONE;10");

    line(&mut s, &mut ui, &cv, "*CLS;:DISP:GBR 40;:DISP:RUL OFF;:DISP:COL ON");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GBR?;:DISP:RUL?;:DISP:COL?"), "40;0;1");
    line(&mut s, &mut ui, &cv, ":DISP:GBR 3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GBR?"), "10", "grid brightness stays at the screen floor");
    line(&mut s, &mut ui, &cv, ":DISP:GBR 101");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(line(&mut s, &mut ui, &cv, ":DISP:GBR?"), "10");

    line(&mut s, &mut ui, &cv, "*CLS;:TIM:ROLL 0;:TIM:MODE XY;:TIM:XY:Z CHAN3");
    assert_eq!(
        line(&mut s, &mut ui, &cv, ":TIM:ROLL?;:TIM:MODE?;:TIM:XY:ENAB?;:TIM:XY:X?;:TIM:XY:Y?;:TIM:XY:Z?"),
        "0;XY;1;CHAN1;CHAN2;CHAN3"
    );
    line(&mut s, &mut ui, &cv, ":TIM:MODE ROLL");
    assert!(
        line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"),
        "roll is the auto-roll switch, not a timebase mode"
    );
    line(&mut s, &mut ui, &cv, ":TIM:XY:X CHAN2");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "the XY plot is CH1 against CH2");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:MODE?;:TIM:XY:X?;:TIM:XY:Z?"), "XY;CHAN1;CHAN3");
    line(&mut s, &mut ui, &cv, ":TIM:MODE MAIN;:TIM:XY:Z NONE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TIM:MODE?;:TIM:XY:ENAB?;:TIM:XY:Z?"), "MAIN;0;NONE");

    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:IND?"), "0");
    line(&mut s, &mut ui, &cv, ":MEAS:IND ON");
    assert_eq!(line(&mut s, &mut ui, &cv, ":CURS:MEAS:IND?"), "1");
    line(&mut s, &mut ui, &cv, ":CURS:MEAS:IND OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":MEAS:IND?"), "0");
}

#[test]
fn frozen_math_scpi_preserves_reference_ids_expression_parser_and_cycles() {
    let (mut scpi, mut ui, canvas) = reference_session();
    ui.commit_waveform_traces_at(
        vec![crate::store::WaveTrace { v: vec![0.0, 1.0, 0.0, 1.0], dt: 1e-9, t0: 2e-6, label: "math R2".into() }],
        "R2",
        1,
    )
    .unwrap();
    line(
        &mut scpi,
        &mut ui,
        &canvas,
        ":MATH1:OPER SUBT;:MATH1:SOUR1 R1;:MATH1:SOUR2 R2;:MATH2:OPER ADD;:MATH2:SOUR1 MATH1;:MATH2:SOUR2 R1",
    );
    assert_eq!(
        line(&mut scpi, &mut ui, &canvas, ":MATH1:SOUR1?;:MATH1:SOUR2?;:MATH2:SOUR1?;:MATH2:SOUR2?"),
        "R1;R2;MATH1;R1"
    );
    assert_eq!((ui.m.math_ch(0).a, ui.m.math_ch(0).b), (MATH_REF_BASE, MATH_REF_BASE + 1));
    assert_eq!(ui.m.math_ch(1).a, NCH);
    line(&mut scpi, &mut ui, &canvas, ":MATH1:EXPR R1 - R2");
    assert_eq!(ui.m.math_ch(0).op, MathOp::Expr);
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":MATH1:EXPR?"), "R1 - R2");
    let old = ui.m.math_ch(0);
    line(&mut scpi, &mut ui, &canvas, ":MATH1:SOUR1 MATH1");
    assert_eq!(ui.m.math_ch(0), old);
    assert!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?").starts_with("-224,"));
    line(&mut scpi, &mut ui, &canvas, ":MATH1:FFT:SOUR R2");
    assert_eq!(ui.m.math_ch(0).a, MATH_REF_BASE + 1);
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":SYST:ERR?"), "0,\"No error\"");
}

// Break it catches: Run after a Single went to Auto whatever the sweep before the Single was, so a Normal sweep was
// lost (seen on a DHO924S, over SCPI and with the Run/Stop key).
#[test]
fn run_after_single_goes_back_to_the_sweep_before_it() {
    let (mut s, mut ui, cv) = session();
    for sweep in ["NORM", "AUTO"] {
        line(&mut s, &mut ui, &cv, &format!(":TRIG:SWE {sweep};:SING"));
        assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:SWE?"), "SING");
        line(&mut s, &mut ui, &cv, ":RUN");
        assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:SWE?"), sweep, "after :SINGle and :RUN");
        // the sweep set to Single, then Run
        line(&mut s, &mut ui, &cv, ":TRIG:SWE SING;:RUN");
        assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:SWE?"), sweep, "after :TRIGger:SWEep SINGle and :RUN");
        // the keys: Single, then Run/Stop twice (stop, run)
        ui.key(crate::ui::key::SINGLE);
        ui.key(crate::ui::key::RUNSTOP);
        ui.key(crate::ui::key::RUNSTOP);
        assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:SWE?"), sweep, "after the Single key and Run/Stop");
    }
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

/// A value of every row with a command, set over SCPI, reads back and is in the model (the rows: oos-model's
/// `settings::testkit`).
#[test]
fn a_changed_value_set_over_scpi_reads_back() {
    for (slices, root, _, _) in GROUPS {
        for s in slices.iter().flat_map(|g| g.iter()) {
            let crate::settings::Scpi::Cmd(c) = s.scpi else { continue };
            for n in instances(s) {
                let m = changed(s, n);
                let want = s.query(&m, n);
                let root = root.replace("<n>", &(n + 1).to_string());
                let (mut sc, mut ui, cv) = session();
                assert_eq!(line(&mut sc, &mut ui, &cv, &format!("{}:{} {}", root, c, want)), "");
                // the query in the short form of the root (`:DISP`)
                let short: String = root.chars().filter(|c| !c.is_ascii_lowercase()).collect();
                assert_eq!(line(&mut sc, &mut ui, &cv, &format!("{}:{}?", short, c)), want, "{} {}", s.key, n);
                for k in instances(s) {
                    assert_eq!((s.get)(&ui.m, k), (s.get)(&m, k), "{} {} {}", s.key, n, k);
                }
                assert!(line(&mut sc, &mut ui, &cv, ":SYSTem:ERRor?").starts_with("0,"), "{}", s.key);
            }
        }
    }
}
