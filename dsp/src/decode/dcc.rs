// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! NMRA DCC (S-9.2), the digital command signal of model railways.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{Kind, Packet};
use crate::edges::edges;

/// Nominal half-bit time of a DCC `1` (58 us high + 58 us low).
pub const DCC_ONE_HALF: f64 = 58e-6;

/// Nominal half-bit time of a DCC `0` (100 us high + 100 us low; zeros may stretch).
pub const DCC_ZERO_HALF: f64 = 100e-6;

/// XOR of all address and data bytes (NMRA error-detection byte).
pub fn dcc_xor(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |a, &b| a ^ b)
}

/// NMRA DCC (S-9.2) on one channel (rail vs ground, or a differential A−B): edges mark half-bit boundaries.
/// A short half (~58 us) is a `1`, a long half (>= ~80 us, up to several ms for a stretched zero) is a `0`. Bits are
/// pairs of halves; a short/long mismatch or a gap resets the search. Packet: preamble (>=10 ones) → start `0` →
/// address byte → (`0` + data byte)* → (`0` + XOR) → end `1`. Emits Field("PRE") (value = preamble ones), Id
/// (address), Data (each data byte), Crc (received XOR, `ok` = matches). Cut packets and half-bit disagreements
/// give Field("!ERR"); idle gaps and non-DCC bursts are skipped.
pub fn dcc(v: &[f32], dt: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    if dt.is_nan() || dt <= 0.0 {
        return out;
    }
    let e: Vec<f64> = edges(v, thr, hyst).into_iter().map(|x| x.t).collect();
    if e.len() < 4 {
        return out;
    }
    // half-bit classification in samples: < 80 us → 1, 80 us .. 12 ms → 0, longer → gap
    let short_max = 80e-6 / dt;
    let long_max = 12e-3 / dt;
    if short_max.is_nan() || short_max < 2.0 {
        return out;
    }
    #[derive(Clone, Copy)]
    enum Half {
        One,
        Zero,
        Gap,
    }
    let half = |d: f64| {
        if d < short_max {
            Half::One
        } else if d <= long_max {
            Half::Zero
        } else {
            Half::Gap
        }
    };
    // bits: (t0, t1, is_one); gaps and mismatches leave holes that reset the packet search
    let mut bits: Vec<Option<(f64, f64, bool)>> = Vec::new();
    let mut i = 0;
    while i + 1 < e.len() {
        let (t0, t1, t2) = (e[i], e[i + 1], e.get(i + 2).copied());
        let a = half(t1 - t0);
        let Some(t2) = t2 else {
            break;
        };
        let b = half(t2 - t1);
        match (a, b) {
            (Half::One, Half::One) => {
                bits.push(Some((t0, t2, true)));
                i += 2;
            }
            (Half::Zero, Half::Zero) => {
                bits.push(Some((t0, t2, false)));
                i += 2;
            }
            // long half followed by idle (>12 ms), or idle first: reset the bit stream
            (Half::Zero, Half::Gap) | (Half::Gap, _) => {
                bits.push(None);
                i += 1;
            }
            // phase slip or noise: drop one half and try again
            _ => {
                bits.push(None);
                i += 1;
            }
        }
    }
    let mut k = 0;
    while k < bits.len() {
        // find a preamble of >= 10 ones
        while k < bits.len() && !matches!(bits[k], Some((_, _, true))) {
            k += 1;
        }
        let pre0 = k;
        while k < bits.len() && matches!(bits[k], Some((_, _, true))) {
            k += 1;
        }
        let n_pre = k - pre0;
        if n_pre < 10 {
            continue;
        }
        // start bit 0
        let Some((s0, s1, false)) = bits.get(k).copied().flatten() else {
            continue;
        };
        let (Some(first), Some(last)) = (bits[pre0], bits[k - 1]) else { continue };
        let (pre_t0, pre_t1) = (first.0, last.1);
        out.push(Packet::field("PRE", pre_t0, pre_t1, n_pre as u32, true));
        k += 1;
        // bytes: each is 8 bits MSB first, preceded by a 0 (already consumed for the first); after each byte a
        // separator: 0 = more data, 1 = end (last byte was the XOR)
        let mut bytes: Vec<(f64, f64, u8)> = Vec::new();
        let mut cut = false;
        let mut err_at: Option<(f64, f64)> = None;
        let mut t_floor = s1;
        loop {
            if k + 8 > bits.len() {
                cut = true;
                break;
            }
            let mut val = 0u8;
            let mut ok_bits = true;
            let t_byte0 = bits[k].map(|b| b.0);
            let mut t_byte1 = t_byte0.unwrap_or(t_floor);
            for b in 0..8 {
                match bits[k + b] {
                    Some((_t0, t1, one)) => {
                        t_byte1 = t1;
                        if one {
                            val |= 1 << (7 - b);
                        }
                    }
                    None => {
                        ok_bits = false;
                        err_at = Some((t_byte1.max(t_floor), t_byte1.max(t_floor) + 1.0));
                        break;
                    }
                }
            }
            if !ok_bits {
                break;
            }
            let t0 = t_byte0.unwrap_or(t_floor);
            bytes.push((t0, t_byte1, val));
            t_floor = t_byte1;
            k += 8;
            // separator
            match bits.get(k).copied().flatten() {
                Some((_, _, true)) => {
                    // end bit: last byte is XOR
                    k += 1;
                    break;
                }
                Some((_, _, false)) => {
                    k += 1;
                    if bytes.len() > 8 {
                        // defensive: NMRA packets are short
                        err_at = Some((t0, t_byte1));
                        break;
                    }
                }
                None => {
                    cut = true;
                    break;
                }
            }
        }
        if cut {
            // packet truncated by the record: drop what we emitted for this attempt's PRE only if nothing else
            if let Some(last) = out.last() {
                if last.kind == Kind::Field("PRE") {
                    out.pop();
                }
            }
            continue;
        }
        if let Some((t0, t1)) = err_at {
            out.push(Packet::tag("!ERR", t0, t1.max(t0 + 1.0), false));
            continue;
        }
        if bytes.len() < 2 {
            out.push(Packet::tag("!ERR", s0, s0 + 1.0, false));
            continue;
        }
        let (xor_t0, xor_t1, xor) = bytes[bytes.len() - 1];
        let payload: Vec<u8> = bytes[..bytes.len() - 1].iter().map(|b| b.2).collect();
        let (a0, a1, addr) = bytes[0];
        out.push(Packet::new(Kind::Id { ext: false, rtr: false }, a0, a1, addr as u32, true));
        for &(t0, t1, d) in &bytes[1..bytes.len() - 1] {
            out.push(Packet::new(Kind::Data, t0, t1, d as u32, true));
        }
        out.push(Packet::new(Kind::Crc, xor_t0, xor_t1, xor as u32, xor == dcc_xor(&payload)));
    }
    out
}
