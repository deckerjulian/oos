// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Tests of `:REFerence`, `:SIM` and `:WAVeform` (scpi/reference.rs, scpi/sim.rs, scpi/waveform.rs): slots, loading
//! and importing files, the simulator, and the pages the waveform answers read. A test of a new command of these goes
//! here; its query also goes into ui/tests/scpi.txt, its line into docs/scpi.md (AGENTS.md).

use super::*;

#[test]
fn reference_run_stop_are_idempotent_and_hold_acquisitions() {
    let (mut s, mut ui, cv) = reference_session();
    assert_eq!(line(&mut s, &mut ui, &cv, ":RUN;:SIM:PLAY?"), "1", "RUN updates replay immediately");
    assert_eq!(line(&mut s, &mut ui, &cv, ":RUN;:SIM:PLAY?"), "1", "repeated RUN must keep running");
    ui.step(0.2);
    assert!(captures(&ui) > 0.0, "RUN must capture a reference window");
    let acquired = captures(&ui);
    assert_eq!(line(&mut s, &mut ui, &cv, ":STOP;:SIM:PLAY?;:TRIG:STAT?"), "0;STOP");
    assert_eq!(line(&mut s, &mut ui, &cv, ":STOP;:SIM:PLAY?"), "0", "repeated STOP must keep stopped");
    ui.step(0.5);
    assert_eq!(captures(&ui), acquired, "STOP must hold the acquired record");
    assert_eq!(ui.m.run, RunState::Stop);
}

#[test]
fn reference_single_waits_for_a_trigger_and_rearms_with_a_menu_open() {
    let (mut s, mut ui, cv) = reference_session();
    ui.workflow_command("settings");
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 10;:SING");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PLAY?;:TRIG:STAT?"), "1;WAIT");
    ui.step(0.2);
    assert_eq!(captures(&ui), 0.0, "an impossible trigger must wait");
    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 0");
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0);
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PLAY?;:TRIG:STAT?"), "0;STOP");
    line(&mut s, &mut ui, &cv, ":SING");
    ui.step(0.2);
    assert_eq!(captures(&ui), 2.0, "a second SINGLE must re-arm and capture once");
    assert_eq!(ui.m.run, RunState::Stop);
    ui.step(0.5);
    assert_eq!(captures(&ui), 2.0);
}

#[test]
fn sim_advanced_parameters_validate_without_regenerating_the_record() {
    let (mut s, mut ui, cv) = session();
    line(&mut s, &mut ui, &cv, ":SIM:SSC:MODE CENTER;:SIM:SSC:DEV 0.007;:SIM:SSC:FREQ 40000;:SIM:JITT:DCD 4e-12;:SIM:JITT:CORR 2e-12;:SIM:JITT:CORN 2e6;:SIM:PATT PRBS-13");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:SSC:MODE?;:SIM:PATT?;:SIM:BUSY?"), "CENTER;PRBS13;0");
    for (command, expected) in [
        (":SIM:SSC:DEV?", 0.007),
        (":SIM:SSC:FREQ?", 40000.0),
        (":SIM:JITT:DCD?", 4e-12),
        (":SIM:JITT:CORR?", 2e-12),
        (":SIM:JITT:CORN?", 2e6),
    ] {
        let actual: f64 = line(&mut s, &mut ui, &cv, command).parse().unwrap();
        assert!((actual - expected).abs() <= expected.abs() * 1e-6, "{command}: {actual}");
    }
    assert!((0..NREF).all(|slot| ui.ref_volts(slot).is_empty()), "configuration must not replace sources until Play");
    for command in [
        ":SIM:SSC:MODE BAD",
        ":SIM:SSC:DEV NaN",
        ":SIM:SSC:FREQ 0",
        ":SIM:JITT:DCD -1",
        ":SIM:JITT:CORR inf",
        ":SIM:JITT:CORN 1e30",
        ":SIM:PATT PRBS8",
    ] {
        line(&mut s, &mut ui, &cv, command);
        assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "{command}");
    }
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:SSC:MODE?;:SIM:PATT?"), "CENTER;PRBS13");
    assert_eq!(ui.m.sim_ssc_deviation, 0.007);
    assert_eq!(ui.m.sim_dcd_pp, 4e-12);
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
}

#[test]
fn sim_rejects_an_oversized_full_ssc_record_without_changing_the_live_session() {
    let (mut s, mut ui, cv) = session();
    let running = ui.m.run;
    line(&mut s, &mut ui, &cv, ":SIM:PRES \"PCIe 4.0\";:SIM:SSC:FREQ 1000;:SIM:PLAY ON");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"));
    assert_eq!(ui.m.run, running);
    assert!(!ui.record_mode());
    assert!((0..NREF).all(|slot| ui.ref_volts(slot).is_empty()));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:BUSY?"), "0");
}

#[test]
fn sim_play_fills_the_slot_and_waveform_reads_it() {
    let (mut s, mut ui, cv) = session();
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PRES?"), "PCIe 4.0");
    line(&mut s, &mut ui, &cv, ":SIM:PRES \"USB 2.0 HS\"");
    line(&mut s, &mut ui, &cv, ":SIM:SLOT R3");
    line(&mut s, &mut ui, &cv, ":SIM:PLAY");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PRES?;:SIM:SLOT?;:SIM:PLAY?"), "USB 2.0 HS;R3;1");
    let vpp = line(&mut s, &mut ui, &cv, ":MEAS:ITEM? VPP,R3");
    assert_ne!(vpp, "9.9E37");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR R3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:SOUR?"), "R3");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "65536");
    line(&mut s, &mut ui, &cv, ":SIM:SHOW OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:SHOW?"), "0");
    line(&mut s, &mut ui, &cv, ":SIM:PLAY OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SIM:PLAY?"), "0");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR CHAN1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:SOUR?"), "CHAN1");
}

#[test]
fn load_and_import_csv_bin_and_wfm() {
    let (mut s, mut ui, cv) = session();
    let dir = crate::ui::testkit::Scratch::new("ref");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    ui.files.dir = dir.clone();

    let csv = dir.join("lane.csv");
    std::fs::write(&csv, "Time (s),Volts\n0,0.1\n1e-9,0.4\n2e-9,-0.2\n").unwrap();
    line(&mut s, &mut ui, &cv, &format!(":REF:SLOT R2;:REF:LOAD \"{}\"", csv.display()));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    line(&mut s, &mut ui, &cv, ":WAV:FORM ASC;:WAV:SOUR R2");
    let data = line(&mut s, &mut ui, &cv, ":WAV:DATA?");
    assert!(data.starts_with("#9"), "{data}");
    assert!(data.contains("1.000000E-01"), "{data}");

    let bin_src = {
        let mut m = crate::model::Model::default();
        m.ch[0].on = true;
        let rec = crate::wave::Record {
            t0: 0.0,
            dt: 1e-9,
            ch: {
                let mut c: [Vec<f32>; crate::model::NCH] = Default::default();
                c[0] = vec![0.2, 0.3, 0.4];
                c
            },
            triggered: true,
            partial: false,
            raw: None,
            la: None,
            pairs: false,
        };
        crate::store::wave_bin(&rec, &m)
    };
    let bin = dir.join("ref4.bin");
    std::fs::write(&bin, &bin_src).unwrap();
    line(&mut s, &mut ui, &cv, ":REF:SLOT 4");
    line(&mut s, &mut ui, &cv, ":REF:IMP");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("0,"), "slot file should be the bin");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR REF4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "3");

    let wfm = wfm_bytes();
    std::fs::write(dir.join("ref4.wfm"), &wfm).unwrap();
    line(&mut s, &mut ui, &cv, ":REF:IMP");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    line(&mut s, &mut ui, &cv, ":WAV:SOUR R4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "4");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW?"), "1");
    line(&mut s, &mut ui, &cv, ":REF:SHOW OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW?"), "0");

    let bad = dir.join("nope.wfm");
    let mut raw = wfm.clone();
    raw[2..10].copy_from_slice(b":WFM#999");
    std::fs::write(&bad, &raw).unwrap();
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", bad.display()));
    let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(err.contains("version 001, 002 or 003"), "{err}");

    line(&mut s, &mut ui, &cv, ":REF:CLE");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "0");
    let _ = std::fs::remove_dir_all(&dir);
}

/// :REF:LOAD names any path on the scope. A file of the kernel's file systems is refused by where it resolves to
/// (before, /proc/kmsg passed the is_file check and its read never ended: the frame loop stood still), and a file
/// over the import limit by its size, before it is read.
#[test]
fn load_refuses_kernel_files_and_oversize_files_before_reading() {
    let (mut s, mut ui, cv) = session();
    let dir = crate::ui::testkit::Scratch::new("ref-limits");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("lane.csv");
    std::fs::write(&csv, "Time (s),Volts\n0,0.1\n1e-9,0.4\n").unwrap();
    // a regular file under /proc or /dev: Linux has /proc; macOS shows an open regular file as /dev/fd/N
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let open = std::fs::File::open(&csv).unwrap();
        let kernel = if cfg!(target_os = "linux") {
            std::path::PathBuf::from("/proc/self/status")
        } else {
            use std::os::fd::AsRawFd;
            std::path::PathBuf::from(format!("/dev/fd/{}", open.as_raw_fd()))
        };
        assert!(kernel.is_file());
        line(&mut s, &mut ui, &cv, &format!(":REF:SLOT R1;:REF:LOAD \"{}\"", kernel.display()));
        let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
        assert!(err.starts_with("-224,") && err.contains("/proc, /sys and /dev"), "{err}");
        assert!(ui.ref_shown_slot(0).is_none());
    }
    let big = dir.join("big.csv");
    std::fs::File::create(&big).unwrap().set_len(crate::store::MAX_WAVEFORM_BYTES as u64 + 1).unwrap();
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", big.display()));
    let err = line(&mut s, &mut ui, &cv, ":SYST:ERR?");
    assert!(err.starts_with("-224,") && err.contains("big.csv exceeds the 128 MiB"), "{err}");
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", dir.display()));
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-256,"), "a directory is no file");
    line(&mut s, &mut ui, &cv, &format!(":REF:LOAD \"{}\"", csv.display()));
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    assert!(ui.ref_shown_slot(0).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bare_value_sets_the_selected_slot() {
    let (mut s, mut ui, cv) = reference_session();
    // A value without a slot (`:VSCale 2`, docs/scpi.md `[<slot>,]<V/div>`) was read as slot 2 and refused.
    line(&mut s, &mut ui, &cv, ":REF:CURR 1;:REF:VSC 2;:REF:VOFF -0.1;:REF:COL RED;:REF:LAB:CONT Ramp;:REF:SHOW 0");
    assert_eq!(line(&mut s, &mut ui, &cv, ":SYST:ERR?"), "0,\"No error\"");
    let vsc: f64 = line(&mut s, &mut ui, &cv, ":REF:VSC?").parse().unwrap();
    let voff: f64 = line(&mut s, &mut ui, &cv, ":REF:VOFF? 1").parse().unwrap();
    assert!((vsc - 2.0).abs() < 1e-6 && (voff + 0.1).abs() < 1e-6, "{vsc} {voff}");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:COL?;:REF:LAB:CONT? R1;:REF:SHOW? REF1"), "RED;Ramp;0");
    // with a slot as before; a query and :RESet still take a bare slot
    line(&mut s, &mut ui, &cv, ":REF:SHOW ON;:REF:VSC R1,0.5;:REF:VSC REF1,0.25");
    let vsc: f64 = line(&mut s, &mut ui, &cv, ":REF:VSC? R1").parse().unwrap();
    assert!((vsc - 0.25).abs() < 1e-6, "{vsc}");
    line(&mut s, &mut ui, &cv, ":REF:RES 1");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW?;:SYST:ERR?"), "1;0,\"No error\"");
    let v = ui.ref_vscale(0);
    assert!(v.is_some_and(|v| (v - 0.25).abs() > 1e-6), "RES 1 resets slot 1 and keeps it: {v:?}");
    line(&mut s, &mut ui, &cv, ":REF:VSC 0,2");
    assert!(line(&mut s, &mut ui, &cv, ":SYST:ERR?").starts_with("-224,"), "a bad slot stays an error");
}

#[test]
fn reference_commands_and_a_raw_word_page() {
    let (mut s, mut ui, cv) = reference_session();
    line(&mut s, &mut ui, &cv, ":REF:CURR 1;:REF:SOUR 1,CHAN2;:REF:VSC 1,0.2;:REF:VOFF 1,-0.05;:REF:LAB:CONT 1,lane");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:CURR?;:REF:SOUR? 1;:REF:LAB:CONT? 1"), "1;CHAN2;lane");
    let vsc: f64 = line(&mut s, &mut ui, &cv, ":REF:VSC? 1").parse().unwrap();
    let voff: f64 = line(&mut s, &mut ui, &cv, ":REF:VOFF? 1").parse().unwrap();
    assert!((vsc - 0.2).abs() < 1e-6 && (voff + 0.05).abs() < 1e-6, "{vsc} {voff}");
    line(&mut s, &mut ui, &cv, ":REF:SHOW 1,OFF");
    assert_eq!(line(&mut s, &mut ui, &cv, ":REF:SHOW? 1"), "0");
    line(&mut s, &mut ui, &cv, ":REF:SHOW 1,ON");

    line(&mut s, &mut ui, &cv, ":WAV:SOUR R1;:WAV:MODE RAW;:WAV:FORM WORD;:WAV:STAR 10;:WAV:STOP 12");
    assert_eq!(line(&mut s, &mut ui, &cv, ":WAV:POIN?"), "3");
    let data = s.exec_line(&mut ui, &cv, ":WAV:DATA?");
    assert!(data.starts_with(b"#9"), "{:?}", &data[..data.len().min(16)]);
    let n: usize = std::str::from_utf8(&data[2..11]).unwrap().parse().unwrap();
    assert_eq!(n, 6, "three little-endian WORD points");
    assert_eq!(data.len(), 11 + 6);

    line(&mut s, &mut ui, &cv, ":TRIG:EDGE:LEV 100;:SING");
    ui.step(0.2);
    assert_eq!(captures(&ui), 0.0);
    line(&mut s, &mut ui, &cv, ":TFOR");
    ui.step(0.2);
    assert_eq!(captures(&ui), 1.0, "TFORce must release a waiting Single");
    assert_eq!(line(&mut s, &mut ui, &cv, ":TRIG:STAT?"), "STOP");
}

#[test]
fn waveform_word_is_little_endian_and_page_metadata_has_the_page_origin() {
    let (mut scpi, mut ui, canvas) = session();
    let volts = |code: u16| (code as f64 - 32768.0) * 8.0 / 60000.0;
    ui.commit_waveform_traces(
        vec![crate::store::WaveTrace {
            v: vec![0.0, volts(0x8123) as f32, volts(0x2345) as f32, 0.0],
            dt: 5e-12,
            t0: -2e-9,
            label: "Known WORD codes".into(),
        }],
        "wire-policy.csv",
        true,
    )
    .unwrap();
    line(
        &mut scpi,
        &mut ui,
        &canvas,
        ":REF:VSC 1,1;:REF:VOFF 1,0;:WAV:SOUR R1;:WAV:MODE RAW;:WAV:FORM WORD;:WAV:STAR 2;:WAV:STOP 3",
    );
    assert_eq!(scpi.exec_line(&mut ui, &canvas, ":WAV:DATA?"), b"#9000000004\x23\x81\x45\x23");
    let preamble = line(&mut scpi, &mut ui, &canvas, ":WAV:PRE?");
    let values: Vec<_> = preamble.split(',').collect();
    assert_eq!(&values[..4], &["1", "2", "2", "1"]);
    assert!((values[4].parse::<f64>().unwrap() - 5e-12).abs() < 1e-18);
    assert!((values[5].parse::<f64>().unwrap() - (-2e-9 + 5e-12)).abs() < 1e-15);
    line(&mut scpi, &mut ui, &canvas, ":WAV:MODE MAX;:STOP");
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":WAV:PRE?").split(',').nth(1), Some("1"));
    line(&mut scpi, &mut ui, &canvas, ":RUN");
    assert_eq!(line(&mut scpi, &mut ui, &canvas, ":WAV:PRE?").split(',').nth(1), Some("1"));
}

fn wfm_bytes() -> Vec<u8> {
    const CURVE: usize = 838;
    let v = [0.0_f32, 0.5, 1.0, -0.25];
    let mut b = vec![0u8; CURVE + v.len() * 4 + 8];
    b[0..2].copy_from_slice(&0x0F0Fu16.to_le_bytes());
    b[2..10].copy_from_slice(b":WFM#002");
    b[15] = 4;
    b[16..20].copy_from_slice(&(CURVE as i32).to_le_bytes());
    b[40..44].copy_from_slice(b"lane");
    let put_f64 = |b: &mut [u8], o: usize, x: f64| b[o..o + 8].copy_from_slice(&x.to_le_bytes());
    let put_u32 = |b: &mut [u8], o: usize, x: u32| b[o..o + 4].copy_from_slice(&x.to_le_bytes());
    put_f64(&mut b, 168, 1.0);
    put_f64(&mut b, 176, 0.0);
    put_u32(&mut b, 240, 4);
    put_f64(&mut b, 488, 1e-9);
    put_f64(&mut b, 496, 0.0);
    put_u32(&mut b, 822, 0);
    put_u32(&mut b, 826, (v.len() * 4) as u32);
    for (k, &x) in v.iter().enumerate() {
        b[CURVE + k * 4..CURVE + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}
