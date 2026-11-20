// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! I2S and S/PDIF (AES3).
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{edge_times, Kind, Packet};
use crate::edges::{edges, states};
use crate::protocols::{BusWhat, ModuleInfo};
use crate::trigger::Slope;

/// I2S: data, bit clock and word select.
pub static I2S: ModuleInfo = ModuleInfo::new("I2s", "I2S", "audio")
    .roles([Some("SD"), Some("SCK"), Some("WS"), None])
    .starts_on(BusWhat::Data, 0x7fff)
    // a frame starts at a change of WS, either way
    .starts_with(2, Slope::Either)
    .bit(|_, _| 0.6e-6)
    .decoder(|l| Ok(i2s(l.need(1, "source channel off")?, l.lines[2], l.main(), l.sh.bits, l.thr, l.hy)));

/// I2S (Philips): SD sampled on the rising SCK edge, MSB first, the word starts one SCK period after the WS edge
/// (WS low = left, high = right). `bits` = word length to keep (MSB-aligned; longer slots are cut, shorter ones end at
/// the next WS edge). Without `ws` the words are cut every `bits` clocks (left only).
pub fn i2s(sck: &[f32], ws: Option<&[f32]>, sd: &[f32], bits: u32, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let ce: Vec<f64> = edges(sck, thr, hyst).into_iter().filter(|e| e.rising).map(|e| e.t).collect();
    let sds = states(sd, thr, hyst);
    let wss = ws.map(|w| states(w, thr, hyst));
    let level = |st: &Vec<Option<bool>>, t: f64| st.get(t as usize).copied().flatten();
    // per rising clock edge: WS level and SD bit
    let mut cur: Option<(bool, f64, u32, u32)> = None; // (right, t0, value, bits so far)
    let mut prev_ws: Option<bool> = None;
    let flush = |cur: &mut Option<(bool, f64, u32, u32)>, t1: f64, out: &mut Vec<Packet>| {
        if let Some((right, t0, v, n)) = cur.take() {
            if n > 0 {
                // MSB-aligned word of `bits`; shorter slots are padded with zeros
                let v = if n < bits { v << (bits - n) } else { v };
                out.push(Packet::new(Kind::Sample { right }, t0, t1, v, n >= bits));
            }
        }
    };
    for (k, &t) in ce.iter().enumerate() {
        let w = match &wss {
            Some(st) => level(st, t),
            None => Some(false),
        };
        let Some(w) = w else { continue };
        let Some(b) = level(&sds, t) else { continue };
        let period = if k > 0 { t - ce[k - 1] } else { 0.0 };
        if wss.is_some() {
            if prev_ws.is_some() && prev_ws != Some(w) {
                // WS changed: the previous word ends with this bit (the one-clock delay), the new word starts next
                if let Some(c) = cur.as_mut() {
                    if c.3 < bits {
                        c.2 = c.2 << 1 | b as u32;
                    }
                    c.3 += 1;
                }
                flush(&mut cur, t + period / 2.0, &mut out);
                cur = Some((w, t + period / 2.0, 0, 0));
                prev_ws = Some(w);
                continue;
            }
            prev_ws = Some(w);
            if let Some(c) = cur.as_mut() {
                if c.3 < bits {
                    c.2 = c.2 << 1 | b as u32;
                }
                c.3 += 1;
            }
        } else {
            let c = cur.get_or_insert((false, t - period / 2.0, 0, 0));
            c.2 = c.2 << 1 | b as u32;
            c.3 += 1;
            if c.3 == bits {
                flush(&mut cur, t + period / 2.0, &mut out);
            }
        }
    }
    // the last word has no WS edge after it: keep it when complete
    if cur.is_some_and(|c| c.3 >= bits) {
        let t1 = ce.last().copied().unwrap_or(0.0);
        flush(&mut cur, t1, &mut out);
    }
    out
}

/// S/PDIF on one line.
pub static SPDIF: ModuleInfo =
    ModuleInfo::new("Spdif", "S/PDIF", "audio").bit(|_, _| 0.15e-6).decoder(|l| Ok(spdif(l.main(), l.thr, l.hy)));

/// S/PDIF / AES3 (IEC 60958-1/-3/-4): biphase-mark coded subframes of 32 time slots. The unit interval (half a time
/// slot) is recovered from the shortest pulses, the preambles B (Z, block start, channel A), M (X, channel A) and W
/// (Y, channel B) are found by their 3-UI pulses. Emits `Field("BLK")` at every B preamble (value = block count) and
/// one `Kind::Sample { right }` per subframe (`right` = W preamble): value = the 24 bits of slots 4..27 (aux + audio,
/// LSB first; 20-bit audio = value >> 4), value2 = V | U << 1 | C << 2 | P << 3, ok = even parity and V = 0.
pub fn spdif(v: &[f32], thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let e = edge_times(v, thr, hyst);
    if e.len() < 64 {
        return out;
    }
    let d: Vec<f64> = e.windows(2).map(|w| w[1] - w[0]).collect();
    let mut s = d.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    let cand = s[s.len() / 200];
    let near: Vec<f64> = s.iter().copied().filter(|&x| x >= 0.7 * cand && x <= 1.35 * cand).collect();
    let ui = near.iter().sum::<f64>() / near.len().max(1) as f64;
    if ui.is_nan() || ui <= 0.0 {
        return out;
    }
    let u: Vec<u8> = d
        .iter()
        .map(|&x| match x / ui {
            r if r < 0.5 => 0,
            r if r < 1.5 => 1,
            r if r < 2.5 => 2,
            r if r < 3.5 => 3,
            _ => 0,
        })
        .collect();
    let mut blk = 0;
    let mut i = 0;
    while i + 4 <= u.len() {
        let right = match u[i..i + 4] {
            [3, 1, 1, 3] => None,
            [3, 3, 1, 1] => Some(false),
            [3, 2, 1, 2] => Some(true),
            _ => {
                i += 1;
                continue;
            }
        };
        // 28 biphase-mark bits: 2 UI = 0, 1 + 1 UI = 1
        let (mut j, mut bits, mut n) = (i + 4, 0u32, 0);
        while n < 28 && j < u.len() {
            match u[j] {
                2 => j += 1,
                1 if u.get(j + 1) == Some(&1) => {
                    bits |= 1 << n;
                    j += 2;
                }
                _ => break,
            }
            n += 1;
        }
        if n < 28 {
            i += 1;
            continue;
        }
        if right.is_none() {
            out.push(Packet::field("BLK", e[i], e[i + 4], blk, true));
            blk += 1;
        }
        let vflag = bits >> 24 & 1 == 1;
        let kind = Kind::Sample { right: right == Some(true) };
        let ok = bits.count_ones() % 2 == 0 && !vflag;
        out.push(Packet::new(kind, e[i], e[j], bits & 0xff_ffff, ok).with_value2(bits >> 24));
        i = j;
    }
    out
}

/// The 192 channel-status bits (channel A) of the first complete block in `spdif` packets: bit n in byte n / 8, bit
/// n % 8 (byte 0 bit 0 = professional flag, the first transmitted bit).
pub fn spdif_channel_status(p: &[Packet]) -> Option<[u8; 24]> {
    let b = p.iter().position(|x| matches!(x.kind, Kind::Field("BLK")))?;
    let mut cs = [0u8; 24];
    let mut n = 0;
    for x in &p[b + 1..] {
        if x.kind == (Kind::Sample { right: false }) {
            let c = x.value2.unwrap_or(0) >> 2 & 1;
            cs[n / 8] |= (c as u8) << (n % 8);
            n += 1;
            if n == 192 {
                return Some(cs);
            }
        }
    }
    None
}

/// Sample rate from the channel status (IEC 60958-3 consumer: byte 3 bits 0..3; IEC 60958-4 professional: byte 0
/// bits 6..7). None when not indicated.
pub fn spdif_sample_rate(cs: &[u8; 24]) -> Option<u32> {
    if cs[0] & 1 == 1 {
        return match cs[0] >> 6 {
            1 => Some(48000),
            2 => Some(44100),
            3 => Some(32000),
            _ => None,
        };
    }
    match cs[3] & 0x0f {
        0 => Some(44100),
        2 => Some(48000),
        3 => Some(32000),
        4 => Some(22050),
        6 => Some(24000),
        8 => Some(88200),
        9 => Some(768000),
        10 => Some(96000),
        12 => Some(176400),
        14 => Some(192000),
        _ => None,
    }
}
