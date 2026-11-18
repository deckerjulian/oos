// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! MIL-STD-1553 and ARINC 429.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{Kind, Packet};
use crate::edges::{edges, states};

/// MIL-STD-1553: Manchester II at 1 Mbit/s on a bipolar bus signal (threshold normally 0 V). A word = 3 us sync
/// (command/status: 1.5 us positive then 1.5 us negative; data: the inverse), 16 data bits MSB first (1 = positive
/// then negative half), odd parity. `ok` = halves complementary and parity odd.
pub fn mil1553(v: &[f32], dt: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let us = 1e-6 / dt; // samples per microsecond
    let mut out = Vec::new();
    if us < 4.0 {
        return out;
    }
    let st = states(v, thr, hyst);
    let level = |t: f64| st.get(t as usize).copied().flatten();
    let e = edges(v, thr, hyst);
    let mut busy_until = 0.0;
    for b in &e {
        // sync: the edge in the middle of the 3 us sync. Data bits never hold a level longer than 1 us, so a level
        // held for >= 1.3 us on both sides of the edge only occurs there (the bus idles at 0 V before a word, which
        // gives no usable edge at the start of the sync)
        let mid = b.t;
        if mid < busy_until {
            continue;
        }
        let first_pos = !b.rising; // positive before the middle edge
                                   // the bus must really be driven (beyond the hysteresis), not idle at 0 V where the comparator keeps its state
        let driven =
            |t: f64, lv: bool| v.get(t as usize).is_some_and(|&x| if lv { x > thr + hyst } else { x < thr - hyst });
        let held = |from: f64, to: f64, lv: bool| (0..=6).all(|i| driven(from + (to - from) * i as f64 / 6.0, lv));
        if !held(mid - 1.3 * us, mid - 0.15 * us, first_pos) || !held(mid + 0.15 * us, mid + 1.3 * us, !first_pos) {
            continue;
        }
        let start = mid + 1.5 * us; // first data bit
        let mut val = 0u32;
        let mut ones = 0;
        let mut ok = true;
        let mut complete = true;
        for i in 0..17 {
            let t = start + i as f64 * us;
            match (level(t + 0.25 * us), level(t + 0.75 * us)) {
                (Some(h1), Some(h2)) => {
                    ok &= h1 != h2;
                    if i < 16 {
                        val = val << 1 | h1 as u32;
                    }
                    ones += h1 as u32;
                }
                _ => complete = false,
            }
        }
        if !complete {
            continue;
        }
        ok &= ones % 2 == 1;
        out.push(Packet::new(Kind::Word { cmd: first_pos }, mid - 1.5 * us, start + 17.0 * us, val, ok));
        busy_until = start + 17.0 * us - 0.5 * us;
    }
    out
}

/// ARINC 429 low speed (12 to 14.5 kbit/s; the usual rate) and high speed bit rates.
pub const ARINC429_LOW: f64 = 12.5e3;

pub const ARINC429_HIGH: f64 = 100e3;

/// One RZ pulse: start, end (fractional samples), HI (true) or LO.
type Pulse = (f64, f64, bool);

fn mid(p: &Pulse) -> f64 {
    0.5 * (p.0 + p.1)
}

/// ARINC 429 (ARINC Specification 429 Part 1): bipolar return-to-zero, a 1 = HI and a 0 = LO for the first half of
/// the bit, NULL for the second; `v` = A − B (or one line against ground at half the amplitude). HI is above `thr`,
/// LO below −`thr` (`thr` > 0, each with the hysteresis `hyst`), NULL in between. `rate` = bit rate (`ARINC429_LOW`,
/// `ARINC429_HIGH`), None = from the pulse spacing. Words are 32 bits, separated by at least 4 bit times of NULL.
///
/// Emits per word: Id (label = bits 1-8, sent MSB first, conventionally shown in octal), Field("SDI") (bits 9-10),
/// Data (bits 11-29, bit 11 = LSB), Field("SSM") (bits 30-31) and Field("PAR") (bit 32, `ok` = odd parity over the
/// word). A word with a wrong number of bits or a bit out of its slot gives Field("!ERR"); words cut by the record
/// start or end are skipped.
pub fn arinc429(v: &[f32], dt: f64, rate: Option<f64>, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let thr = thr.abs();
    if thr.is_nan() || thr <= 0.0 || dt.is_nan() || dt <= 0.0 {
        return out;
    }
    // HI pulses through +thr, LO pulses through -thr
    let mut pulses: Vec<Pulse> = Vec::new();
    for hi in [true, false] {
        let mut start = None;
        for e in edges(v, if hi { thr } else { -thr }, hyst) {
            if e.rising == hi {
                start = Some(e.t);
            } else if let Some(s) = start.take() {
                pulses.push((s, e.t, hi));
            }
        }
    }
    pulses.sort_by(|a, b| a.0.total_cmp(&b.0));
    if pulses.is_empty() {
        return out;
    }
    let median = |mut x: Vec<f64>| {
        x.sort_by(f64::total_cmp);
        x.get(x.len() / 2).copied()
    };
    let bit = match rate {
        Some(r) => 1.0 / r / dt,
        None => {
            // pulses are half a bit wide, inside a word they follow one bit apart
            let w = median(pulses.iter().map(|p| p.1 - p.0).collect()).unwrap_or(0.0);
            median(pulses.windows(2).map(|q| mid(&q[1]) - mid(&q[0])).filter(|&d| d < 3.0 * w).collect())
                .unwrap_or(2.0 * w)
        }
    };
    if !bit.is_finite() || bit < 4.0 {
        return out;
    }
    let n = v.len() as f64;
    let mut i = 0;
    while i < pulses.len() {
        // a word: pulses less than 2.5 bits apart (the gap between words is at least 5 bits pulse to pulse)
        let mut j = i + 1;
        while j < pulses.len() && mid(&pulses[j]) - mid(&pulses[j - 1]) < 2.5 * bit {
            j += 1;
        }
        arinc429_word(&pulses[i..j], bit, n, &mut out);
        i = j;
    }
    out
}

fn arinc429_word(p: &[Pulse], bit: f64, n: f64, out: &mut Vec<Packet>) {
    let (first, last) = (p[0], p[p.len() - 1]);
    let inside = first.0 >= 2.0 * bit && n - last.1 >= 2.0 * bit;
    let m0 = mid(&first);
    // the bit time of this word, and every pulse in its slot
    let b = (mid(&last) - m0) / 31.0;
    let slots = p.len() == 32
        && (b / bit - 1.0).abs() < 0.2
        && p.iter().enumerate().all(|(k, q)| ((mid(q) - m0) / b - k as f64).abs() < 0.3);
    if !slots {
        if inside {
            out.push(Packet::tag("!ERR", first.0, last.1, false));
        }
        return;
    }
    let bits: Vec<bool> = p.iter().map(|q| q.2).collect();
    let t0 = m0 - 0.25 * b; // start of bit 1
    let at = |k: usize| t0 + k as f64 * b;
    let lsb = |a: usize, len: usize| (0..len).fold(0u32, |acc, k| acc | (bits[a + k] as u32) << k);
    let label = bits[..8].iter().fold(0u32, |acc, &x| acc << 1 | x as u32);
    let odd = bits.iter().filter(|&&x| x).count() % 2 == 1;
    out.push(Packet::new(Kind::Id { ext: false, rtr: false }, at(0), at(8), label, true));
    out.push(Packet::field("SDI", at(8), at(10), lsb(8, 2), true));
    out.push(Packet::new(Kind::Data, at(10), at(29), lsb(10, 19), true));
    out.push(Packet::field("SSM", at(29), at(31), lsb(29, 2), true));
    out.push(Packet::field("PAR", at(31), at(32), bits[31] as u32, odd));
}
