// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The parallel bus decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

#[test]
fn parallel_clocked_and_unclocked() {
    let words = [0x3u32, 0xa, 0xf, 0x0, 0x5, 0x9];
    let t = 1e-6;
    let dt = t / 50.0;
    let mut lines = Vec::new();
    for k in 0..4 {
        let mut l = Logic::new(0.0, 3.3, t / 100.0);
        // line 3 lags by half a sample (skew)
        let skew = if k == 3 { dt / 2.0 } else { 0.0 };
        for (i, w) in words.iter().enumerate() {
            l.push(i as f64 * t + if i > 0 { skew } else { 0.0 }, (w >> k) & 1 == 1);
        }
        lines.push(l);
    }
    let mut clk = Logic::new(0.0, 3.3, t / 100.0);
    clk.push(0.0, false);
    for i in 0..words.len() {
        clk.push(i as f64 * t + t / 2.0, true);
        clk.push((i + 1) as f64 * t, false);
    }
    let n = (words.len() as f64 * t / dt) as usize + 10;
    let v: Vec<Vec<f32>> = lines.iter().map(|l| sample(|x| l.v(x), 0.0, dt, n)).collect();
    let c = sample(|x| clk.v(x), 0.0, dt, n);
    let refs: Vec<&[f32]> = v.iter().map(|x| x.as_slice()).collect();
    let p = parallel(&refs, Some(&c), Slope::Rising, 1.65, 0.3);
    assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), words);
    assert!(p.iter().all(|x| x.ok));
    let p = parallel(&refs, None, Slope::Rising, 1.65, 0.3);
    assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), words, "{:?}", p);
    // a 2-line bus on the falling clock edge sees the next word (data changes with the falling edge)
    let p = parallel(&refs[..2], Some(&c), Slope::Falling, 1.65, 0.3);
    let mut want: Vec<u32> = words[1..].iter().map(|w| w & 3).collect();
    want.push(words[5] & 3);
    assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), want);
}
