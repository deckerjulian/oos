// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Waveform files of other instruments, built for the tests of their importers here and for the hostile-input tests
//! of oos-ui (the feature `testkit`, which oos-ui turns on for its tests only): each takes the time of the first
//! sample, the sample step and the samples.

pub fn isf_i16(t0: f64, dt: f64, ymult: f64, codes: &[i16]) -> Vec<u8> {
    let mut curve = Vec::new();
    for c in codes {
        curve.extend_from_slice(&c.to_le_bytes());
    }
    let len = curve.len().to_string();
    let mut b = format!(":WFMPRE:BYT_NR 2;ENCDG BIN;BN_FMT RI;BYT_OR LSB;NR_PT {};PT_FMT Y;XINCR {dt:.9e};XZERO {t0:.9e};PT_OFF 0;YMULT {ymult:.9e};YOFF 0;YZERO 0;WFID \"lane\";:CURVE #{}{len}", codes.len(), len.len()).into_bytes();
    b.extend(curve);
    b
}

pub fn wavedesc_i16(t0: f64, dt: f64, gain: f32, off: f32, codes: &[i16]) -> Vec<u8> {
    let desc = 346usize;
    let mut b = vec![0u8; desc + codes.len() * 2];
    b[..8].copy_from_slice(b"WAVEDESC");
    b[32] = 1;
    b[34] = 1;
    b[36..40].copy_from_slice(&(desc as u32).to_le_bytes());
    b[60..64].copy_from_slice(&((codes.len() * 2) as u32).to_le_bytes());
    b[116..120].copy_from_slice(&(codes.len() as u32).to_le_bytes());
    b[96..98].copy_from_slice(b"C1");
    b[156..160].copy_from_slice(&gain.to_le_bytes());
    b[160..164].copy_from_slice(&off.to_le_bytes());
    b[176..180].copy_from_slice(&(dt as f32).to_le_bytes());
    b[180..188].copy_from_slice(&t0.to_le_bytes());
    for (k, c) in codes.iter().enumerate() {
        b[desc + k * 2..desc + k * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
    b
}

pub fn ag_f32(t0: f64, dt: f64, v: &[f32], label: &str) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"AG10");
    let size_at = b.len();
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    let mut h = vec![0u8; 140];
    h[..4].copy_from_slice(&140u32.to_le_bytes());
    h[4..8].copy_from_slice(&1u32.to_le_bytes());
    h[8..12].copy_from_slice(&1u32.to_le_bytes());
    h[12..16].copy_from_slice(&(v.len() as u32).to_le_bytes());
    h[32..40].copy_from_slice(&dt.to_le_bytes());
    h[40..48].copy_from_slice(&t0.to_le_bytes());
    h[112..112 + label.len()].copy_from_slice(label.as_bytes());
    b.extend(h);
    b.extend_from_slice(&12u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&((v.len() * 4) as u32).to_le_bytes());
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    let n = b.len() as u32;
    b[size_at..size_at + 4].copy_from_slice(&n.to_le_bytes());
    b
}

fn tr0_block(payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x04\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x00");
    b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    b.extend_from_slice(payload);
    b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    b
}

pub fn tr0_f32(t0: f64, dt: f64, v: &[f32]) -> Vec<u8> {
    let mut header = String::from("00020000000000009601*x ");
    for name in ["TIME", "v(1)"] {
        header.push_str(&format!("{name:<16}"));
    }
    header.push_str("$&%#");
    let mut data = Vec::new();
    for (k, x) in v.iter().enumerate() {
        let t = (t0 + k as f64 * dt) as f32;
        data.extend_from_slice(&t.to_le_bytes());
        data.extend_from_slice(&x.to_le_bytes());
    }
    data.extend_from_slice(&1.0e30_f32.to_le_bytes());
    let mut b = tr0_block(header.as_bytes());
    b.extend(tr0_block(&data));
    b
}

pub fn raw_f64(t0: f64, dt: f64, v: &[f32]) -> Vec<u8> {
    let mut b = format!("Title: t\nDate: today\nPlotname: Transient Analysis\nFlags: real\nNo. Variables: 2\nNo. Points: {}\nVariables:\n\t0\ttime\ttime\n\t1\tv(1)\tvoltage\nBinary:\n", v.len()).into_bytes();
    for (k, x) in v.iter().enumerate() {
        b.extend_from_slice(&(t0 + k as f64 * dt).to_le_bytes());
        b.extend_from_slice(&(*x as f64).to_le_bytes());
    }
    b
}

pub fn wfmx_f32(t0: f64, dt: f64, v: &[f32]) -> Vec<u8> {
    let mut b = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<DcaFile><Header><FileType>WaveformInternal</FileType></Header><Points>{}</Points><XOrg>{t0}</XOrg><XInc>{dt}</XInc><Data Lengths=\"{}, 1\"></Data></DcaFile>\n", v.len(), v.len()).into_bytes();
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}
