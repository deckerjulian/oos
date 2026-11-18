// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! DMX512-A with RDM.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{field_be, uart, Byte, Kind, Packet, Parity};
use crate::edges::{edges, states};
use crate::protocols::ModuleInfo;

/// DMX512 bit rate.
pub const DMX_BAUD: f64 = 250e3;

/// RDM start code (ANSI E1.20).
pub const RDM_SC: u8 = 0xcc;

/// DMX512 on an RS-485 pair.
pub static DMX512: ModuleInfo = ModuleInfo::new("Dmx512", "DMX512", "dmx").differential();

/// DMX512-A (ANSI E1.11) with RDM (ANSI E1.20): 250 kbit/s 8N2, idle high (`invert` = idle low). Per packet: Break
/// (low >= 88 us), Field("MAB") (value = mark after break in us, `ok` >= 8 us), Field("SC") start code. Start code
/// 0x00 (dimmer data) and other alternate start codes: Data per slot (value = level, value2 = slot number 1..). RDM
/// (0xCC): Field "SUB" (sub start code, ok = 0x01), "LEN" (message length), "DST"/"SRC" (UID: value = low 32 bits,
/// value2 = manufacturer ID), "TN", "PORT" (port ID / response type), "MC" (message count), "SUBDEV", "CC" (command
/// class), "PID", "PDL" (ok = consistent with LEN), Data per parameter byte, Crc (16-bit additive checksum).
/// `ok` of a slot = both stop bits high. RDM discovery responses (no break) are not decoded.
pub fn dmx512(v: &[f32], dt: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let spb = 1.0 / DMX_BAUD / dt;
    let us = 1e-6 / dt;
    let mut out = Vec::new();
    if spb.is_nan() || spb < 2.5 {
        return out;
    }
    let e = edges(v, thr, hyst);
    let st = states(v, thr, hyst);
    let mark = |x: f64| st.get(x as usize).copied().flatten().map(|b| b != invert);
    // (break start, break end = MAB start, MAB end)
    let mut brk = Vec::new();
    for (k, w) in e.windows(2).enumerate() {
        if w[0].rising == invert && w[1].rising != invert && w[1].t - w[0].t >= 88.0 * us - 1.0 {
            brk.push((w[0].t, w[1].t, e.get(k + 2).map(|x| x.t)));
        }
    }
    let chars = uart(v, dt, DMX_BAUD, 8, Parity::None, thr, hyst, invert);
    for (k, &(b0, b1, m1)) in brk.iter().enumerate() {
        let end = brk.get(k + 1).map_or(f64::INFINITY, |b| b.0);
        out.push(Packet::new(Kind::Break, b0, b1, 0, true));
        let Some(m1) = m1 else { continue };
        out.push(Packet::field("MAB", b1, m1, ((m1 - b1) / us).round() as u32, m1 - b1 >= 8.0 * us - 1.0));
        let slots: Vec<Byte> = chars
            .iter()
            .filter(|c| c.t0 >= b1 - 0.5 && c.t0 < end)
            .map(|c| (c.value as u8, c.t0, c.t0 + 11.0 * spb, c.ok && mark(c.t0 + 10.5 * spb) == Some(true)))
            .collect();
        let Some(&(sc, s0, s1, sok)) = slots.first() else { continue };
        out.push(Packet::field("SC", s0, s1, sc as u32, sok));
        if sc == RDM_SC {
            rdm(&slots, &mut out);
        } else {
            for (i, s) in slots.iter().enumerate().skip(1) {
                out.push(Packet::new(Kind::Data, s.1, s.2, s.0 as u32, s.3).with_value2(i as u32));
            }
        }
    }
    out
}

/// RDM message (ANSI E1.20 section 6) from its slots (slot 0 = start code).
fn rdm(s: &[Byte], out: &mut Vec<Packet>) {
    let b = |i: usize| s.get(i).map_or(0, |x| x.0) as usize;
    let (len, pdl) = (b(2), b(23));
    let layout: [(&'static str, usize, usize, bool); 11] = [
        ("SUB", 1, 1, b(1) == 1),
        ("LEN", 2, 1, len >= 24),
        ("DST", 3, 6, true),
        ("SRC", 9, 6, true),
        ("TN", 15, 1, true),
        ("PORT", 16, 1, true),
        ("MC", 17, 1, true),
        ("SUBDEV", 18, 2, true),
        ("CC", 20, 1, true),
        ("PID", 21, 2, true),
        ("PDL", 23, 1, 24 + pdl == len),
    ];
    let trunc = |out: &mut Vec<Packet>| {
        let l = s.last().unwrap_or(&(0, 0.0, 0.0, false));
        out.push(Packet::tag("!ERR", l.1, l.2, false));
    };
    for &(label, a, n, ok) in &layout {
        if !field_be(out, s, label, a, n, ok) {
            return trunc(out);
        }
    }
    for x in s.iter().skip(24).take(pdl) {
        out.push(Packet::new(Kind::Data, x.1, x.2, x.0 as u32, x.3));
    }
    let cs_at = if len >= 24 { len } else { 24 + pdl };
    if cs_at + 2 > s.len() {
        return trunc(out);
    }
    let rx = (b(cs_at) << 8 | b(cs_at + 1)) as u32;
    let sum = s[..cs_at].iter().map(|x| x.0 as u32).sum::<u32>() & 0xffff;
    out.push(Packet::new(Kind::Crc, s[cs_at].1, s[cs_at + 1].2, rx, rx == sum && s[cs_at].3 && s[cs_at + 1].3));
}
