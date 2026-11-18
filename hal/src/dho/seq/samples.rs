// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The sample codec: read-out bytes (16-bit words, the enabled channels interleaved) -> channels and volts, in the
//! two-pass form (`deinterleave`, `volts`) and the fused f32 form (`decode_volts_into`, `CodeMap`). Nothing here
//! writes a register; a new way of turning read-out words into samples goes here. The word layout and the code scale
//! are described in docs/protocols/.

use crate::dho::{hz, readout};

/// Splits a read-out (16-bit words, `ratio` channels interleaved) into channels: ratio 1 = the one enabled
/// channel; ratio 2 = lower channel on even words, higher on odd; ratio 4 = word 4i+k is CH k+1 (horizontal-config.md
/// 2.2). Channels not enabled stay empty.
pub fn deinterleave(bytes: &[u8], mask: u32) -> [Vec<u16>; 4] {
    let w: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
    let mut out: [Vec<u16>; 4] = Default::default();
    match hz::ratio(mask) {
        1 => out[on[0]] = w,
        2 => {
            out[on[0]] = w.iter().step_by(2).copied().collect();
            out[on[1]] = w.iter().skip(1).step_by(2).copied().collect();
        }
        _ => {
            for c in on {
                out[c] = w.iter().skip(c).step_by(4).copied().collect();
            }
        }
    }
    out
}

/// `deinterleave` and `volts` in one pass, straight from the read-out bytes: `cal[c]` = (scale, offset) of CH c+1.
/// The same mapping and lengths as the two passes, in f32 (`volt32`), without the per-channel `u16` vectors. One
/// exact-size vector per enabled channel.
pub fn decode_volts(bytes: &[u8], mask: u32, cal: [(f64, f64); 4]) -> [Vec<f32>; 4] {
    let mut out: [Vec<f32>; 4] = Default::default();
    decode_volts_into(bytes, mask, cal, &mut out);
    out
}

/// `decode_volts` into kept vectors: the enabled channels are refilled in place, the others left empty. A frame that
/// fits their capacity allocates nothing (musl's malloc maps every block of 128 kB or more anew, with page faults).
pub fn decode_volts_into(bytes: &[u8], mask: u32, cal: [(f64, f64); 4], out: &mut [Vec<f32>; 4]) {
    let ratio = hz::ratio(mask) as usize;
    let slot = slots(mask, ratio);
    let group = 2 * ratio;
    let whole = bytes.len() / group;
    let tail = &bytes[whole * group..];
    for o in out.iter_mut() {
        o.clear();
    }
    for (k, c) in slot.iter().enumerate().take(ratio) {
        let Some(c) = *c else { continue };
        // the word of an incomplete last group, as deinterleave hands it out
        let last = tail.get(2 * k..2 * k + 2);
        let (a, off) = affine(cal[c]);
        let o = &mut out[c];
        o.reserve_exact(whole + last.is_some() as usize);
        // one pass per channel over its word of every group; the group size as a constant keeps it a vectorised loop
        match ratio {
            1 => lane::<2>(bytes, 0, a, off, o),
            2 => lane::<4>(bytes, k, a, off, o),
            _ => lane::<8>(bytes, k, a, off, o),
        }
        if let Some(b) = last {
            o.push(volt32(u16::from_le_bytes([b[0], b[1]]), a, off));
        }
    }
}

/// Where a read-out holds each channel without decoding it (fork, docs/fork/perf-plan.md "codes"): words per group,
/// the word of each channel in its group (`decode_volts_into`'s mapping) and `volt_code`'s (volts per code, offset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CodeMap {
    pub ratio: usize,
    pub word: [Option<usize>; 4],
    pub affine: [(f32, f32); 4],
}

/// `CodeMap` of a read-out of channels `mask` with `cal[c]` = (scale, offset) of CH c+1.
pub fn code_map(mask: u32, cal: [(f64, f64); 4]) -> CodeMap {
    let ratio = hz::ratio(mask) as usize;
    let slot = slots(mask, ratio);
    let mut word = [None; 4];
    for (k, c) in slot.iter().enumerate().take(ratio) {
        if let Some(c) = *c {
            word[c] = Some(k);
        }
    }
    CodeMap { ratio, word, affine: cal.map(affine) }
}

/// The volts `decode_volts_into` gives for read-out word `x` of a channel with `affine` (the same f32 operations).
#[inline]
pub fn volt_code(x: u16, (a, off): (f32, f32)) -> f32 {
    volt32(x, a, off)
}

/// Volts of samples `a..b` of the channel at `word` of every group of `ratio` words in `bytes`, into `out` (cleared
/// first): `volt_code` of each, in one loop.
pub fn lane_volts_into(
    bytes: &[u8],
    ratio: usize,
    word: usize,
    a: usize,
    b: usize,
    (sa, off): (f32, f32),
    out: &mut Vec<f32>,
) {
    out.clear();
    out.extend((a..b).map(|i| {
        let j = 2 * (i * ratio + word);
        volt32(u16::from_le_bytes([bytes[j], bytes[j + 1]]), sa, off)
    }));
}

/// Slot k of a group of `ratio` words goes to channel `slot[k]` (deinterleave's mapping): ratio 1 and 2 = the enabled
/// channels in order, ratio 4 = word k is CH k+1 when it is enabled. A fixed table: no allocation per frame.
fn slots(mask: u32, ratio: usize) -> [Option<usize>; 4] {
    let mut slot = [None; 4];
    let mut k = 0;
    for c in (0..4).filter(|c| mask & (1 << c) != 0) {
        if ratio == 4 {
            slot[c] = Some(c);
        } else if k < ratio {
            slot[k] = Some(c);
            k += 1;
        }
    }
    slot
}

/// Volts of word `k` of every whole group of `G` bytes, appended to `o` (`decode_volts_into`).
fn lane<const G: usize>(bytes: &[u8], k: usize, a: f32, off: f32, o: &mut Vec<f32>) {
    let at = o.len();
    o.resize(at + bytes.len() / G, 0.0);
    for (d, g) in o[at..].iter_mut().zip(bytes.as_chunks::<G>().0) {
        *d = volt32(u16::from_le_bytes([g[2 * k], g[2 * k + 1]]), a, off);
    }
}

/// (volts per code, offset) of a channel in f32 for `volt32`.
fn affine((scale, offset): (f64, f64)) -> (f32, f32) {
    ((scale / readout::CODES_PER_DIV) as f32, offset as f32)
}

/// `volt` in f32: the code minus 32768 is exact, then one multiply and one subtract. The error against the f64 `volt`
/// is a few f32 ulps of the full-scale value, under 1e-6 of a division: below 1 % of one code (1 / 7500 div).
#[inline]
fn volt32(x: u16, a: f32, off: f32) -> f32 {
    (x as i32 - readout::MID as i32) as f32 * a - off
}

/// Volts of read-out words: 7500 codes per division around 32768 (preamble yinc = V/div / 7500), minus the offset.
pub fn volts(w: &[u16], scale: f64, offset: f64) -> Vec<f32> {
    w.iter().map(|&x| volt(x, scale, offset)).collect()
}

/// Volts of one read-out word (`volts`).
#[inline]
fn volt(x: u16, scale: f64, offset: f64) -> f32 {
    ((x as f64 - readout::MID) / readout::CODES_PER_DIV * scale - offset) as f32
}
