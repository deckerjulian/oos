// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The S/PDIF decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

fn spdif_frames(n: usize) -> Vec<(u32, u32)> {
    (0..n as u32).map(|k| ((k * 0x01_2345) & 0xff_ffff, (0xff_ffff - k * 0x777) & 0xff_ffff)).collect()
}

#[test]
fn spdif_samples_blocks_and_channel_status() {
    let frames = spdif_frames(200);
    let mut cs = [0u8; 24];
    cs[0] = 0x04; // consumer, copying permitted
    cs[1] = 0x01; // category
    cs[3] = 0x02; // 48 kHz
    let fs = 48e3;
    let l = gen::spdif(&frames, &cs, fs, &[], 0.0, 0.5);
    let dt = 1.0 / (128.0 * fs) / 6.0; // 6 samples per UI
    let mut w = waves(&[&l], dt);
    add_noise(&mut w[0], 0.03, 1);
    for inv in [false, true] {
        let v: Vec<f32> = if inv { w[0].iter().map(|x| 0.5 - x).collect() } else { w[0].clone() };
        let p = spdif(&v, 0.25, 0.1);
        let s: Vec<(bool, u32)> = p
            .iter()
            .filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None })
            .collect();
        let want: Vec<(bool, u32)> = frames.iter().flat_map(|&(a, b)| [(false, a), (true, b)]).collect();
        assert_eq!(s, want, "inverted {}", inv);
        assert!(p.iter().filter(|x| matches!(x.kind, Kind::Sample { .. })).all(|x| x.ok));
        let blk: Vec<u32> = p.iter().filter(|x| x.kind == F("BLK")).map(|x| x.value).collect();
        assert_eq!(blk, vec![0, 1]);
        let got = spdif_channel_status(&p).expect("one complete block");
        assert_eq!(got, cs);
        assert_eq!(spdif_sample_rate(&got), Some(48000));
    }
    let mut pro = [0u8; 24];
    pro[0] = 0x01 | 2 << 6;
    assert_eq!(spdif_sample_rate(&pro), Some(44100));
}

#[test]
fn spdif_parity_error_and_short_capture() {
    let frames = spdif_frames(20);
    let cs = [0u8; 24];
    let l = gen::spdif(&frames, &cs, 44.1e3, &[5, 12], 0.0, 1.0);
    let dt = 1.0 / (128.0 * 44.1e3) / 5.0;
    let w = waves(&[&l], dt);
    let p = spdif(&w[0], 0.5, 0.2);
    let ok: Vec<bool> = p.iter().filter(|x| matches!(x.kind, Kind::Sample { .. })).map(|x| x.ok).collect();
    assert_eq!(ok.len(), 40);
    let bad: Vec<usize> = ok.iter().enumerate().filter(|x| !x.1).map(|x| x.0).collect();
    assert_eq!(bad, vec![5, 12]);
    // fewer than 192 frames: no channel status
    assert_eq!(spdif_channel_status(&p), None);
}
