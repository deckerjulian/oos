// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the readers of files (ADR 0047): waveform files of OOS and of other instruments, fast-frame
//! files, mask files, setup files and the power-on picture. Each test states the contract it holds the reader to;
//! the generator and the list of findings that still stand are in `crate::hostile`.

use super::header_import_tests::{ag_f32, isf_i16, raw_f64, tr0_f32, wavedesc_i16, wfmx_f32};
use super::tests::{custom, wfm_f32};
use super::*;
use crate::hostile::{is_known, run};

/// A record of two channels, as a capture stores it.
fn record() -> (Record, Model) {
    let mut m = Model::default();
    m.ch[2].on = true;
    let mut rec = Record {
        t0: -1e-3,
        dt: 1e-6,
        ch: Default::default(),
        triggered: true,
        partial: false,
        raw: None,
        la: None,
        pairs: false,
    };
    rec.ch[0] = vec![0.5, 1.25, -0.75, 0.0, 2.5, f32::NAN];
    rec.ch[2] = vec![-1.0, 0.0, 3.3, 3.3, 0.1, -0.1];
    (rec, m)
}

/// What every trace taken from a file promises its users: a time step that is a positive number, a start time that
/// is a number, and at least two samples.
fn trace_ok(t: &WaveTrace) -> Result<(), String> {
    if !(t.dt.is_finite() && t.dt > 0.0) {
        return Err(format!("accepted a trace with dt {}", t.dt));
    }
    if !t.t0.is_finite() {
        return Err(format!("accepted a trace with t0 {}", t.t0));
    }
    if t.v.len() < 2 || t.v.len() > MAX_WAVEFORM_SAMPLES {
        return Err(format!("accepted a trace of {} samples", t.v.len()));
    }
    Ok(())
}

/// A valid file of every format `parse_wave` knows.
fn wave_files() -> Vec<Vec<u8>> {
    let (rec, m) = record();
    let v = [0.0_f32, 1.0, -0.5, 0.25, 0.75, -1.0];
    let codes = [0_i16, 1000, -500, 250, 750, -1000];
    let mut tr0_text = String::from("00020000000000009601*x ");
    for name in ["TIME", "v(1)"] {
        tr0_text.push_str(&format!("{name:<16}"));
    }
    tr0_text.push_str("$&%#");
    for (k, x) in v.iter().enumerate() {
        tr0_text.push_str(&format!("{:11.4e}{:11.4e}", k as f64 * 1e-9, x));
    }
    tr0_text.push_str(&format!("{:11.4e}", 1.0e30));
    let raw_text = "Title: t\nPlotname: Transient Analysis\nFlags: real\nNo. Variables: 2\nNo. Points: 3\n\
        Variables:\n\t0\ttime\ttime\n\t1\tv(1)\tvoltage\nValues:\n0\t0.0\n\t1.0\n1\t1e-9\n\t2.0\n2\t2e-9\n\t3.0\n";
    let isf_text = b":WFMPRE:BYT_NR 1;ENCDG ASC;NR_PT 4;PT_FMT ENV;XINCR 1e-9;XZERO 0;PT_OFF 1;YMULT 0.5;YOFF 1;\
        YZERO 0.25;:CURVE 1,2,3,4,5,6,7,8";
    vec![
        wave_bin(&rec, &m),
        wave_csv(&rec, &m).into_bytes(),
        b"0,1,2\n1e-6,3\n2e-6,4,5,6\n".to_vec(),
        wfm_f32(b":WFM#001", -2e-6, 1e-9, &v, "lane"),
        wfm_f32(b":WFM#003", -2e-6, 1e-9, &v, "lane"),
        isf_i16(-1e-6, 1e-9, 1e-3, &codes),
        isf_text.to_vec(),
        wavedesc_i16(-1e-6, 1e-9, 1e-3, 0.5, &codes),
        ag_f32(-1e-6, 2e-9, &v, "lane"),
        tr0_f32(0.0, 1e-9, &v),
        tr0_text.into_bytes(),
        raw_f64(0.0, 1e-9, &v),
        raw_text.as_bytes().to_vec(),
        wfmx_f32(-1e-6, 1e-9, &v),
    ]
}

/// `parse_wave` (a file from the stick, the save folder or the web page): returns for any bytes; what it accepts is
/// one trace or more, each with a positive finite `dt`, a finite `t0` and 2..=32 million samples; what it refuses
/// has a message.
#[test]
fn hostile_waveform_files() {
    let files = wave_files();
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    run("store::parse_wave", &seeds, 1, |b| match parse_wave(b, "hostile.bin") {
        Ok(traces) if traces.is_empty() => Err("accepted a file without a trace".into()),
        Ok(traces) => traces.iter().try_for_each(trace_ok),
        Err(e) if e.is_empty() => Err("refused without a message".into()),
        Err(_) => Ok(()),
    });
}

// Break it catches: a CSV time base taken without the check of the binary readers (#181)
#[test]
fn hostile_csv_time_not_finite() {
    assert!(parse_wave(b"0,1\ninf,2\n", "a.csv").is_err(), "the second time is inf: dt = inf");
    assert!(parse_wave(b"-inf,1\n0,2\n", "a.csv").is_err(), "the first time is -inf: t0 = -inf");
}

/// What a record of a capture file promises: a positive finite `dt`, a finite `t0`, and channels of one length.
fn record_ok(r: &Record) -> Result<(), String> {
    if !(r.dt.is_finite() && r.dt > 0.0) {
        return Err(format!("accepted a record with dt {}", r.dt));
    }
    if !r.t0.is_finite() {
        return Err(format!("accepted a record with t0 {}", r.t0));
    }
    let n = r.ch.iter().map(Vec::len).max().unwrap_or(0);
    if n > MAX_WAVEFORM_SAMPLES || r.ch.iter().any(|c| !c.is_empty() && c.len() != n) {
        return Err(format!("accepted a record with channels of {:?} samples", r.ch.each_ref().map(Vec::len)));
    }
    Ok(())
}

/// `parse_wave_bin` and `parse_ff` (OOSW and OOSF, the frame files of the save folder): return for any bytes; a
/// record they accept has a positive finite `dt`, a finite `t0` and channels of one length; an OOSW record ends
/// inside the bytes; an OOSF file gives as many records as its header says.
#[test]
fn hostile_frame_files() {
    let (rec, m) = record();
    let mut other = rec.clone();
    (other.t0, other.dt) = (0.0, 2e-9);
    let files = [wave_bin(&rec, &m), ff_bin(&[rec.clone(), other], &m), ff_bin(&[], &m)];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    run("store::parse_ff", &seeds, 1, |b| {
        if let Ok((r, used)) = parse_wave_bin(b) {
            record_ok(&r)?;
            if used > b.len() || used < 36 {
                return Err(format!("a record of {} bytes in a file of {}", used, b.len()));
            }
        }
        match parse_ff(b) {
            Ok(recs) => {
                let said =
                    if b.starts_with(b"OOSF") { u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize } else { 1 };
                if recs.len() != said {
                    return Err(format!("{} records from a file that says {}", recs.len(), said));
                }
                recs.iter().try_for_each(record_ok)
            }
            Err(e) if e.is_empty() => Err("refused without a message".into()),
            Err(_) => Ok(()),
        }
    });
}

/// `parse_mask` (a mask file of the save folder): returns for any text; a mask it accepts has at least one point,
/// a source that is a channel, and tolerances that are numbers.
#[test]
fn hostile_mask_files() {
    let files = [mask_text(2, 0.2, 0.4, true, &[1.0, 2.5, -0.5, 0.0]), "1.0\n2.0\n# a comment\n\n3.5e-1\n".to_string()];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_bytes()).collect();
    run("store::parse_mask", &seeds, 1, |b| match parse_mask(&String::from_utf8_lossy(b)) {
        Ok(mf) => {
            if mf.v.is_empty() || mf.src.is_some_and(|s| s >= NCH) {
                return Err(format!("accepted a mask of {} points for source {:?}", mf.v.len(), mf.src));
            }
            if [mf.x, mf.y].iter().flatten().any(|t| !t.is_finite()) {
                return Err(format!("accepted the tolerances {:?} and {:?}", mf.x, mf.y));
            }
            Ok(())
        }
        Err(e) if e.is_empty() => Err("refused without a message".into()),
        Err(_) => Ok(()),
    });
}

/// `parse_setup_lenient` and `parse_ref_looks` (a setup file of the save folder or the stick, and the setup of the
/// last session at the start): return for any text; every line the reader leaves out is named with its number; the
/// model it gives is one the writer and the reader agree on: written out and read again it has no refused line and
/// is the same text. The look of a reference slot has a positive finite scale and a finite offset.
#[test]
fn hostile_setup_files() {
    let looks = [Some(RefLook { on: Some(true), vdiv: Some(0.5), offset: Some(-1.0), color: Some(3), label: None })];
    let files = [setup_text(&Model::default()), setup_text(&custom()) + &ref_setup_text(&looks)];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_bytes()).collect();
    run("store::parse_setup", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let (m, bad) = parse_setup_lenient(&text);
        if let Some(e) = bad.iter().find(|e| !e.starts_with("line ")) {
            return Err(format!("a refused line without its number: {e}"));
        }
        if bad.len() > text.lines().count() {
            return Err(format!("{} refused lines in a file of {}", bad.len(), text.lines().count()));
        }
        let written = setup_text(&m);
        let (again, bad_again) = parse_setup_lenient(&written);
        if let Some(e) = bad_again.first() {
            // the finding `setup-refused-line-half-applied`: the key refused now is one refused in the input
            let key = |e: &str| e.split(": ").nth(1).unwrap_or("").to_string();
            if bad.iter().any(|b| key(b) == key(e)) && is_known("setup-refused-line-half-applied") {
                return Ok(());
            }
            return Err(format!("accepted a setup the reader refuses when it is written out: {e}"));
        }
        let rewritten = setup_text(&again);
        if rewritten != written {
            let (a, b) = written.lines().zip(rewritten.lines()).find(|(a, b)| a != b).unwrap_or(("", ""));
            return Err(format!("accepted a setup that changes when written and read again: {a:?} became {b:?}"));
        }
        for l in parse_ref_looks(&text, NREF).iter().flatten() {
            if l.vdiv.is_some_and(|v| !(v.is_finite() && v > 0.0)) || l.offset.is_some_and(|o| !o.is_finite()) {
                return Err(format!("accepted the reference look {l:?}"));
            }
        }
        Ok(())
    });
}

#[test]
#[ignore = "a setup line that is refused for a later value leaves its first values in the model, #181"]
pub(crate) fn hostile_known_setup_refused_line_half_applied() {
    let (m, bad) = parse_setup_lenient("bode_sweep = inf true 1 Rise 0 1 0\n");
    assert_eq!(bad.len(), 1, "an infinite sweep time is refused: {bad:?}");
    assert_eq!(m.bode_sw_time, Model::default().bode_sw_time, "and the refused line changes nothing");
}

/// `ppm_into` (the power-on picture, `/etc/oos/splash.ppm` or one copied from removable media): returns for any
/// bytes; a picture it takes has a size above zero whose pixels are all in the file.
#[test]
fn hostile_power_on_picture() {
    let mut small = b"P6\n# OOS\n2 1\n255\n".to_vec();
    small.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
    let mut wide = b"P6 40 3 255\n".to_vec();
    wide.extend((0..40 * 3 * 3).map(|k| k as u8));
    let seeds: [&[u8]; 2] = [&small, &wide];
    let mut cv = Canvas::new(16, 8);
    run("store::ppm_into", &seeds, 1, |b| match ppm_into(b, &mut cv) {
        Some((w, h)) if w <= 0 || h <= 0 || (w as usize * h as usize * 3) > b.len() => {
            Err(format!("took a picture of {w} x {h} from {} bytes", b.len()))
        }
        _ => Ok(()),
    });
}

// Break it catches: a header that ends inside a comment (#181)
#[test]
fn hostile_ppm_header_ends_in_a_comment() {
    assert_eq!(ppm_into(b"P6\n#", &mut Canvas::new(16, 8)), None);
}

// Break it catches: a size whose pixels are counted in i32 (#181)
#[test]
fn hostile_ppm_size_overflows() {
    assert_eq!(ppm_into(b"P6 65536 65536 255\n", &mut Canvas::new(16, 8)), None);
}

// Break it catches: a tolerance of the mask file taken without asking for a number (#181)
#[test]
fn hostile_mask_tolerance_not_finite() {
    assert!(parse_mask("# OOS mask\nx=inf\n---\n1\n2\n").is_err(), "x = inf");
    assert!(parse_mask("# OOS mask\ny=NaN\n---\n1\n2\n").is_err(), "y = NaN");
}
