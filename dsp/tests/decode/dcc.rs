// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The NMRA DCC decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

fn dcc_frames() -> Vec<DccFrame> {
    vec![
        DccFrame { address: 0x03, data: vec![0x3F], xor: None, preamble: 14 },
        DccFrame { address: 0x28, data: vec![0x00, 0x28], xor: None, preamble: 12 },
    ]
}

fn dcc_want(fr: &[DccFrame]) -> Vec<(Kind, u32, bool)> {
    let mut want = Vec::new();
    for f in fr {
        let mut payload = vec![f.address];
        payload.extend_from_slice(&f.data);
        let x = f.xor.unwrap_or_else(|| dcc_xor(&payload));
        want.push((F("PRE"), f.preamble.max(10), true));
        want.push((ID, f.address as u32, true));
        for &d in &f.data {
            want.push((Kind::Data, d as u32, true));
        }
        want.push((Kind::Crc, x as u32, f.xor.is_none_or(|g| g == dcc_xor(&payload))));
    }
    want
}

#[test]
fn dcc_xor_and_baseline_packets() {
    assert_eq!(dcc_xor(&[0x03, 0x3F]), 0x3C);
    assert_eq!(dcc_xor(&[0x28, 0x00, 0x28]), 0x00);
    let fr = dcc_frames();
    let dt = DCC_ONE_HALF / 8.0;
    let mut v = gen::dcc(&fr, 0.0, 3.3).sample(dt, 200e-6);
    add_noise(&mut v, 0.05, 9);
    let p = dcc(&v, dt, 1.65, 0.3);
    assert_eq!(kv(&p), dcc_want(&fr));
    // nothing on a flat line or with too few samples per half-bit
    assert!(dcc(&vec![1.65; 5000], dt, 1.65, 0.3).is_empty());
    assert!(dcc(&v, 50e-6, 1.65, 0.3).is_empty());
}

#[test]
fn dcc_bad_xor_and_cut_packet() {
    let mut fr = dcc_frames();
    fr[0].xor = Some(0x3C ^ 0xFF);
    let dt = DCC_ONE_HALF / 8.0;
    let v = gen::dcc(&fr, 0.0, 3.3).sample(dt, 200e-6);
    let p = dcc(&v, dt, 1.65, 0.3);
    let crcs: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Crc).map(|x| (x.value, x.ok)).collect();
    assert_eq!(crcs, [(0x3C ^ 0xFF, false), (0x00, true)]);
    // a record that ends inside the first packet: no partial decode
    let full = gen::dcc(&dcc_frames()[..1], 0.0, 3.3).sample(dt, 200e-6);
    let cut = &full[..full.len() / 2];
    assert!(dcc(cut, dt, 1.65, 0.3).is_empty());
}

/// An orphan long half before a valid packet (capture starts mid-bit / odd phase) must not leave a hole that turns
/// the following preamble into PRE + !ERR — resync by dropping that half.
#[test]
fn dcc_resyncs_phase_slip_before_preamble() {
    let fr = dcc_frames();
    let dt = DCC_ONE_HALF / 8.0;
    let body = gen::dcc(&fr, 0.0, 3.3).sample(dt, 200e-6);
    let orphan_n = (DCC_ZERO_HALF / dt).round() as usize;
    let first = body[0];
    let orphan_level = if first >= 1.65 { 0.0 } else { 3.3 };
    let mut v = vec![orphan_level as f32; orphan_n];
    v.extend_from_slice(&body);
    let p = dcc(&v, dt, 1.65, 0.3);
    assert!(
        !p.iter().any(|x| x.kind == Kind::Field("!ERR")),
        "phase slip must not emit !ERR, got {:?}",
        kv(&p)
    );
    assert_eq!(kv(&p), dcc_want(&fr));
}

/// A 0 inside a long run of ones (false start) must not emit PRE+!ERR when the "packet" ends after one byte.
#[test]
fn dcc_false_start_in_preamble_is_not_broken() {
    let idle = DccFrame { address: 0xFF, data: vec![0x00], xor: None, preamble: 14 };
    let mut bits: Vec<bool> = vec![true; 12];
    bits.push(false);
    bits.extend(std::iter::repeat_n(true, 8));
    bits.push(true);
    bits.extend(gen::dcc_bits(&[idle.clone()]));
    let mut l = Logic::new(0.0, 3.3, 1e-6);
    let mut t = 20e-6;
    let mut level = true;
    l.push(0.0, false);
    l.push(t, true);
    for &one in &bits {
        let half = if one { DCC_ONE_HALF } else { DCC_ZERO_HALF };
        for _ in 0..2 {
            t += half;
            level = !level;
            l.push(t, level);
        }
    }
    t += 500e-6;
    l.push(t, false);
    let dt = DCC_ONE_HALF / 8.0;
    let v = l.sample(dt, 200e-6);
    let p = dcc(&v, dt, 1.65, 0.3);
    assert!(
        !p.iter().any(|x| x.kind == Kind::Field("!ERR")),
        "false start must not emit !ERR, got {:?}",
        kv(&p)
    );
    assert_eq!(kv(&p), dcc_want(&[idle]));
}
