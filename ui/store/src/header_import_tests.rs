// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests of the waveform files of other instruments (`import.rs`): their headers, the samples read from them and
//! the damaged files refused. A test of a new importer goes here; the files it builds into `testkit.rs`.

use super::testkit::*;
use super::*;
#[test]
fn other_waveform_headers_load_and_unknown_bytes_are_refused() {
    let samples = [0.0_f32, 1.0, -0.5, 0.25];
    let isf = isf_i16(0.0, 1e-9, 1e-3, &[0, 1000, -500, 250]);
    let got = parse_wave(&isf, "a.isf").unwrap();
    assert_close(&got[0], 0.0, 1e-9, &samples);

    let trc = wavedesc_i16(0.0, 1e-9, 1e-3, 0.0, &[0, 1000, -500, 250]);
    assert_close(&parse_wave(&trc, "a.trc").unwrap()[0], 0.0, 1e-9, &samples);

    let ag = ag_f32(-1e-6, 2e-9, &samples, "lane");
    let got = parse_wave(&ag, "a.bin").unwrap();
    assert_eq!(got[0].label, "lane");
    assert_close(&got[0], -1e-6, 2e-9, &samples);

    let tr0 = tr0_f32(0.0, 1e-9, &samples);
    assert_close(&parse_wave(&tr0, "a.tr0").unwrap()[0], 0.0, 1e-9, &samples);
    let mut ascii = String::from("00020000000000009601*x ");
    for name in ["TIME", "v(1)"] {
        ascii.push_str(&format!("{name:<16}"));
    }
    ascii.push_str("$&%#");
    for (k, x) in samples.iter().enumerate() {
        ascii.push_str(&format!("{:11.4e}{:11.4e}", k as f64 * 1e-9, x));
    }
    ascii.push_str(&format!("{:11.4e}", 1.0e30));
    assert_close(&parse_wave(ascii.as_bytes(), "c.tr0").unwrap()[0], 0.0, 1e-9, &samples);
    let raw = raw_f64(0.0, 1e-9, &samples);
    assert_close(&parse_wave(&raw, "b.tr0").unwrap()[0], 0.0, 1e-9, &samples);

    let wfmx = wfmx_f32(-1e-6, 1e-9, &samples);
    assert_close(&parse_wave(&wfmx, "a.wfmx").unwrap()[0], -1e-6, 1e-9, &samples);

    assert!(parse_wave(b"\x00\x01\x02\x03\xff", "no.bin").is_err());
    assert!(parse_wave(b"not a waveform\n", "no.csv").unwrap_err().contains("no waveform"));
    assert!(parse_wave(b"AGxx,1,2\n3,4,5\n", "no.bin").is_err());
}

#[test]
fn header_import_isf_block_digits_and_point_counts_are_checked() {
    for byte in 0..=u8::MAX {
        let mut b = b":WFMPRE:BYT_NR 1;NR_PT 2;XINCR 1e-9;:CURVE #".to_vec();
        b.push(byte);
        b.extend_from_slice(b"000000000");
        assert!(parse_wave(&b, "bad.isf").is_err(), "block byte {byte}");
    }
    for field in ["NR_PT -1", "NR_PT 2.5", "NR_PT 18446744073709551615", "BYT_NR 1.5", "YMULT wrong"] {
        let b = format!(":WFMPRE:BYT_NR 1;XINCR 1e-9;{field};:CURVE #12\0\x01");
        assert!(parse_wave(b.as_bytes(), "bad.isf").is_err(), "{field}");
    }
    assert!(parse_wave(b":WFMPRE:BYT_NR 1;NR_PT 3;XINCR 1;:CURVE #12\0\x01", "short.isf")
        .unwrap_err()
        .contains("truncated"));
}

#[test]
fn header_import_isf_envelope_preserves_pairs_and_requires_complete_data() {
    let header = b":WFMPRE:BYT_NR 1;NR_PT 2;PT_FMT ENV;XINCR 1;:CURVE #14";
    let mut b = header.to_vec();
    b.extend([-2i8 as u8, 2, -1i8 as u8, 1]);
    let traces = parse_wave(&b, "envelope.isf").unwrap();
    assert_eq!(traces.len(), 2);
    assert_eq!(traces[0].v, [-2.0, -1.0]);
    assert_eq!(traces[1].v, [2.0, 1.0]);
    let short = b":WFMPRE:BYT_NR 1;NR_PT 2;PT_FMT ENV;XINCR 1;:CURVE #13\0\x01\x02";
    assert!(parse_wave(short, "short.isf").unwrap_err().contains("truncated"));
}

#[test]
fn header_import_fixed_width_unicode_is_safe_and_invalid_data_is_refused() {
    let text = format!("0002{}x$&%#", "é".repeat(16));
    let (nvars, names) = post_vars(&text).unwrap();
    assert_eq!((nvars, names.len()), (2, 2));
    let text = format!("00020000000000009601*x $&%#{}", "é".repeat(16));
    assert!(parse_wave(text.as_bytes(), "bad.tr0").is_err());
}

#[test]
fn header_import_transient_requires_every_time_sample_to_be_uniform() {
    for time in [3.0, 1.0, f64::NAN, f64::INFINITY] {
        let mut b = raw_f64(0.0, 1.0, &[10.0, 20.0, 30.0, 40.0]);
        let start = b.len() - 4 * 16;
        b[start + 2 * 16..start + 2 * 16 + 8].copy_from_slice(&time.to_le_bytes());
        assert!(parse_wave(&b, "adaptive.tr0").unwrap_err().contains("nonuniform"));
    }
    let values = [10.0, 20.0, 30.0, 40.0];
    let b = raw_f64(-2e-6, 2e-9, &values);
    assert_close(&parse_wave(&b, "regular.tr0").unwrap()[0], -2e-6, 2e-9, &values);
}

#[test]
fn header_import_wfmx_preserves_whitespace_bytes_at_binary_boundaries() {
    for first in [0x3f800020, 0x3f80000a, 0x3f80000d] {
        for last in [0x0a800000, 0x0d800000] {
            let values = [f32::from_bits(first), f32::from_bits(last)];
            let b = wfmx_f32(0.0, 1.0, &values);
            let trace = parse_wave(&b, "bytes.wfmx").unwrap().remove(0);
            assert_eq!(trace.v.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), [first, last]);
            let mut crlf = b.clone();
            let separator = crlf.len() - 8 - 1;
            crlf.insert(separator, b'\r');
            assert_eq!(parse_wave(&crlf, "crlf.wfmx").unwrap()[0].v, values);
        }
    }
}

#[test]
fn header_import_wfmx_embedded_payload_uses_exact_length_not_binary_tag_bytes() {
    let values = [f32::from_bits(0x3f800020), 2.0];
    let mut b =
        b"<?xml?><DataFile><FileType>WaveformInternal</FileType><Points>2</Points><XOrg>0</XOrg><XInc>1</XInc><Data>"
            .to_vec();
    for value in values {
        b.extend(value.to_le_bytes());
    }
    b.extend_from_slice(b"</Data></DataFile>");
    assert_eq!(parse_wave(&b, "embedded.wfmx").unwrap()[0].v, values);
    let mut b = b"<Data>".to_vec();
    let payload = b"</Data>123456789";
    assert_eq!(payload.len(), 16);
    b.extend_from_slice(payload);
    b.extend_from_slice(b"</Data>");
    assert_eq!(wfmx_payload(&b, 4).unwrap(), payload);
    assert!(wfmx_payload(&b, usize::MAX).is_err());
}

#[test]
fn header_import_wfmx_separator_cannot_replace_a_missing_payload_byte() {
    let mut b = wfmx_f32(0.0, 1.0, &[1.0, 2.0]);
    b.pop();
    assert!(parse_wave(&b, "short.wfmx").unwrap_err().contains("truncated"));
}

#[test]
fn header_import_wfmx_checks_the_whole_explicit_time_axis() {
    let mut payload = Vec::new();
    for value in [0.0f64, 1.0, 3.0, 10.0, 20.0, 30.0] {
        payload.extend(value.to_le_bytes());
    }
    assert!(wfmx_volts(&payload, 3, 0.0, 1.0).is_err());
    payload[16..24].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(wfmx_volts(&payload, 3, 0.0, 1.0).unwrap(), [10.0, 20.0, 30.0]);
}

#[test]
fn header_import_wfmx_f32_axis_accepts_quantization_without_guessing_other_layouts() {
    let n = 100_000;
    let t0 = -37e-6;
    let dt = 1e-9;
    let xs: Vec<f32> = (0..n).map(|i| (t0 + i as f64 * dt) as f32).collect();
    let ys: Vec<f32> = (0..n).map(|i| ((i % 37) as f32 - 18.0) / 19.0).collect();
    for interleaved in [false, true] {
        let mut payload = Vec::with_capacity(n * 8);
        if interleaved {
            for i in 0..n {
                payload.extend(xs[i].to_le_bytes());
                payload.extend(ys[i].to_le_bytes());
            }
        } else {
            for x in &xs {
                payload.extend(x.to_le_bytes());
            }
            for y in &ys {
                payload.extend(y.to_le_bytes());
            }
        }
        assert_eq!(wfmx_volts(&payload, n, t0, dt).unwrap(), ys);
        let at = if interleaved { (n - 1) * 8 } else { (n - 1) * 4 };
        payload[at..at + 4].copy_from_slice(&(xs[n - 1] + 10.0 * dt as f32).to_le_bytes());
        assert!(wfmx_volts(&payload, n, t0, dt).unwrap_err().contains("ambiguous"));
    }
    let implicit_f64: Vec<u8> = [1.0f64, 2.0, 3.0].into_iter().flat_map(f64::to_le_bytes).collect();
    assert!(wfmx_volts(&implicit_f64, 3, 0.0, 1.0).is_err());
}

#[test]
fn header_import_long_tr0_f32_times_keep_source_precision_and_reject_adaptive_axis() {
    let values: Vec<f32> = (0..100_000).map(|i| (i % 7) as f32).collect();
    let b = tr0_f32(-37e-6, 1e-9, &values);
    let got = parse_wave(&b, "long.tr0").unwrap();
    assert_eq!(got[0].v, values);
    assert!((got[0].dt - 1e-9).abs() < 1e-16);
    let mut cols = Vec::new();
    for i in 0..1000 {
        cols.extend([(i as f64 * 1e-9) as f32 as f64, i as f64]);
    }
    cols[998 * 2] += 5e-9;
    assert!(traces_from_columns(&cols, 2, &[], true).unwrap_err().contains("nonuniform"));
}

#[test]
fn header_import_known_formats_tolerate_all_truncation_boundaries_without_panicking() {
    let values = [0.0, 1.0, -0.5, 0.25];
    let fixtures = [
        isf_i16(0.0, 1e-9, 1e-3, &[0, 1000, -500, 250]),
        wavedesc_i16(0.0, 1e-9, 1e-3, 0.0, &[0, 1000, -500, 250]),
        ag_f32(0.0, 1e-9, &values, "lane"),
        tr0_f32(0.0, 1e-9, &values),
        raw_f64(0.0, 1e-9, &values),
        wfmx_f32(0.0, 1e-9, &values),
    ];
    for b in fixtures {
        for end in 0..b.len() {
            let _ = parse_wave(&b[..end], "header.bin");
        }
        assert_eq!(parse_wave(&b, "header.bin").unwrap()[0].v.len(), 4);
    }
}

fn assert_close(t: &WaveTrace, t0: f64, dt: f64, v: &[f32]) {
    let tol = dt.abs() * 1e-5 + 1e-15;
    assert!((t.t0 - t0).abs() <= tol, "t0 {} {}", t.t0, t0);
    assert!((t.dt - dt).abs() <= tol, "dt {} {}", t.dt, dt);
    assert_eq!(t.v.len(), v.len());
    for (a, b) in t.v.iter().zip(v) {
        assert!((a - b).abs() < 1e-5, "{a} {b}");
    }
}

// Break it catches: a reader of the fields of a header that reads past the end of a damaged file
#[test]
fn the_fields_of_a_header_are_read_in_both_byte_orders_and_not_past_the_end() {
    let f = Fields::new(b"\x01\x02\x03", true);
    assert_eq!((f.u16(1), Fields::new(b"\x01\x02\x03", false).u16(1)), (Ok(0x0302), Ok(0x0203)));
    assert_eq!(f.u32(0), Err("truncated waveform".to_string()));
    assert_eq!(f.u16(usize::MAX), Err("truncated waveform".to_string()));
}
