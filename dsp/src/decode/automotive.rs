// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! LIN, FlexRay and SENT (SAE J2716).
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{flexray_frame_crc, flexray_header_crc, sent_crc, uart, Kind, Packet, Parity};
use crate::edges::{edges, states};
use crate::protocols::{BusWhat, ModuleInfo, OPTS};

/// SENT clock ticks (s) of the Decode menu; 0 = any tick (from the sync pulses).
pub const SENT_TICKS: [f64; 8] = [3e-6, 5e-6, 10e-6, 20e-6, 30e-6, 50e-6, 90e-6, 0.0];

/// The SENT clock tick of the options; `None` = from the sync pulses.
pub fn sent_tick(opt: &[usize; OPTS]) -> Option<f64> {
    let t = SENT_TICKS[opt[0]];
    (t > 0.0).then_some(t)
}

/// LIN protected identifier: ID with the two parity bits P0 = ID0^ID1^ID2^ID4, P1 = !(ID1^ID3^ID4^ID5).
pub fn lin_pid(id: u8) -> u8 {
    let b = |i: u8| (id >> i) & 1;
    let p0 = b(0) ^ b(1) ^ b(2) ^ b(4);
    let p1 = 1 ^ (b(1) ^ b(3) ^ b(4) ^ b(5));
    (id & 0x3f) | p0 << 6 | p1 << 7
}

/// LIN checksum: inverted 8-bit sum with carry wrap-around over `bytes` (classic: data; enhanced: PID + data).
pub fn lin_checksum(bytes: &[u8]) -> u8 {
    let mut s: u16 = 0;
    for &b in bytes {
        s += b as u16;
        if s > 0xff {
            s -= 0xff;
        }
    }
    !(s as u8)
}

/// LIN on one line or a pair.
pub static LIN: ModuleInfo =
    ModuleInfo::new("Lin", "LIN", "automotive").differential().starts_on(BusWhat::Id, 0x21).bit(|_, sh| 1.0 / sh.baud);

/// LIN frames: break (dominant >= 11 bit times), sync 0x55, protected ID, data bytes and the checksum (the last byte
/// before the next break or the end; classic or enhanced checksum accepted). Bytes are UART 8N1, idle high.
pub fn lin(v: &[f32], dt: f64, baud: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let spb = 1.0 / baud / dt;
    let mut out = Vec::new();
    if spb < 2.5 {
        return out;
    }
    let e = edges(v, thr, hyst);
    // breaks: falling edge followed by the next rising edge >= 11 bit times later
    let mut breaks = Vec::new();
    for w in e.windows(2) {
        if !w[0].rising && w[1].rising && w[1].t - w[0].t >= 11.0 * spb {
            breaks.push((w[0].t, w[1].t));
        }
    }
    let bytes = uart(v, dt, baud, 8, Parity::None, thr, hyst, false);
    for (k, &(b0, b1)) in breaks.iter().enumerate() {
        let end = breaks.get(k + 1).map_or(f64::INFINITY, |b| b.0);
        let fr: Vec<&Packet> = bytes.iter().filter(|p| p.t0 > b1 && p.t0 < end).collect();
        out.push(Packet::new(Kind::Break, b0, b1, 0, true));
        if fr.len() < 2 || fr[0].value != 0x55 {
            continue;
        }
        out.push(Packet { kind: Kind::Data, ..fr[0].clone() }); // sync
        let pid = fr[1].value as u8;
        out.push(Packet {
            kind: Kind::Id { ext: false, rtr: false },
            value: (pid & 0x3f) as u32,
            ok: lin_pid(pid & 0x3f) == pid,
            ..fr[1].clone()
        });
        if fr.len() >= 3 {
            let data: Vec<u8> = fr[2..fr.len() - 1].iter().map(|p| p.value as u8).collect();
            for p in &fr[2..fr.len() - 1] {
                out.push(Packet { kind: Kind::Data, ..(*p).clone() });
            }
            let cs = fr[fr.len() - 1];
            let mut enh = vec![pid];
            enh.extend(&data);
            let ok = cs.value as u8 == lin_checksum(&data) || cs.value as u8 == lin_checksum(&enh);
            out.push(Packet { kind: Kind::Crc, ok, ..cs.clone() });
        }
    }
    out
}

/// FlexRay at 10 Mbit/s on RxD or a pair.
pub static FLEXRAY: ModuleInfo =
    ModuleInfo::new("FlexRay", "FlexRay", "automotive").differential().starts_on(BusWhat::Id, 0x2a).bit(|_, _| 0.1e-6);

/// FlexRay frames (FlexRay Protocol Specification V3.0.1) on one channel at `rate` (normally 10 Mbit/s), as on the
/// transceiver RxD pin (idle high; `invert` = low) or a BP − BM difference from `diff` (idle 0 V reads as high).
/// TSS (low 3..15 bits), FSS (one high bit), every byte preceded by a byte start sequence (high, low; the falling
/// edge resynchronises), header, payload, CRC, FES (low, high).
///
/// Emits Field("TSS") (value = length in bits, `ok` = 3..15), Field("IND") (reserved, payload preamble, null frame,
/// sync, startup indicator; MSB = reserved bit, `ok` = reserved bit 0), Id (frame ID, `ok` = not 0), Field("LEN")
/// (payload words, value2 = bytes), Field("HCRC") (`ok` = header CRC matches), Field("CYC") (cycle count), Data per
/// payload byte, Crc (frame CRC-24, value2 = channel 0 = A / 1 = B whose init vector matched, `ok` = one matched);
/// a missing byte start sequence or a wrong FES: Field("!ERR"). A TSS counts after at least 10 bits of idle (the
/// channel idle delimiter is 11); a frame cut by the record end gives its fields so far, without an error.
pub fn flexray(v: &[f32], dt: f64, rate: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let spb = 1.0 / rate / dt;
    let mut out = Vec::new();
    if spb.is_nan() || spb < 4.0 {
        return out;
    }
    let st = states(v, thr, hyst);
    let high = |x: f64| st.get(x as usize).copied().flatten() != Some(invert);
    let e = edges(v, thr, hyst);
    let fall: Vec<f64> = e.iter().filter(|x| x.rising == invert).map(|x| x.t).collect();
    let rise: Vec<f64> = e.iter().filter(|x| x.rising != invert).map(|x| x.t).collect();
    // TSS candidates: falling edges after the channel idle (at least 10 bits high, longer than any high run inside a
    // frame, so the tail of a frame cut by the record start is not taken for one), and a low first decision after as
    // long an undefined start (a differential bus idles at 0 V)
    let idle = 10.0 * spb;
    let mut starts: Vec<f64> = fall
        .iter()
        .copied()
        .filter(|&f| f - rise.partition_point(|&x| x < f).checked_sub(1).map_or(0.0, |k| rise[k]) >= idle)
        .collect();
    if let Some(i) = st.iter().position(|s| s.is_some()) {
        if st[i].map(|b| b != invert) == Some(false) && i as f64 >= idle {
            starts.insert(0, i as f64);
        }
    }
    let mut busy = f64::NEG_INFINITY;
    for &f in &starts {
        if f < busy {
            continue;
        }
        let Some(&r) = rise.get(rise.partition_point(|&x| x <= f)) else { break };
        let tss = (r - f) / spb;
        if !(2.5..=16.5).contains(&tss) {
            continue;
        }
        // bits of the frame: (level, start)
        let mut bits: Vec<(bool, f64)> = Vec::new();
        let mut exp = r + 2.0 * spb; // first BSS falling edge: after FSS and the high BSS bit
        let mut nbytes = 5usize;
        let mut lost = false;
        while bits.len() < 8 * nbytes {
            let j = fall.partition_point(|&x| x < exp - 0.75 * spb);
            let sync = match fall.get(j) {
                Some(&x) if x <= exp + 0.75 * spb => x,
                _ => {
                    lost = true;
                    break;
                }
            };
            let s0 = sync + spb;
            for k in 0..8 {
                let t = s0 + k as f64 * spb;
                bits.push((high(t + 0.5 * spb), t));
            }
            exp = sync + 10.0 * spb;
            if bits.len() == 40 {
                let len = bits[16..23].iter().fold(0usize, |a, b| a << 1 | b.0 as usize);
                nbytes = 5 + 2 * len + 3;
            }
        }
        if bits.len() < 40 {
            continue; // not a frame
        }
        busy = exp + spb;
        let fv = |a: usize, n: usize| bits[a..a + n].iter().fold(0u32, |acc, b| acc << 1 | b.0 as u32);
        let fld = |kind, a: usize, n: usize, ok| Packet::new(kind, bits[a].1, bits[a + n - 1].1 + spb, fv(a, n), ok);
        out.push(Packet::field("TSS", f, r, tss.round() as u32, (2.5..=15.5).contains(&tss)));
        out.push(fld(Kind::Field("IND"), 0, 5, !bits[0].0));
        let id = fv(5, 11);
        out.push(fld(Kind::Id { ext: false, rtr: false }, 5, 11, id != 0));
        let words = fv(16, 7);
        out.push(fld(Kind::Field("LEN"), 16, 7, true).with_value2(2 * words));
        out.push(fld(Kind::Field("HCRC"), 23, 11, fv(23, 11) == flexray_header_crc(fv(3, 20))));
        out.push(fld(Kind::Field("CYC"), 34, 6, true));
        let nfull = bits.len() / 8;
        for k in 5..nfull.min(nbytes - 3) {
            out.push(fld(Kind::Data, 8 * k, 8, true));
        }
        // a frame cut by the record end is not an error
        let end = v.len() as f64;
        if lost {
            if exp + 0.75 * spb < end {
                out.push(Packet::tag("!ERR", exp - 0.75 * spb, exp + 0.75 * spb, false));
            }
            continue;
        }
        let bytes: Vec<u8> = (0..nbytes).map(|k| fv(8 * k, 8) as u8).collect();
        let rx = fv(8 * (nbytes - 3), 24);
        let body = &bytes[..nbytes - 3];
        let ch = if flexray_frame_crc(body, false) == rx {
            Some(0)
        } else if flexray_frame_crc(body, true) == rx {
            Some(1)
        } else {
            None
        };
        out.push(Packet { value2: ch, ..fld(Kind::Crc, 8 * (nbytes - 3), 24, ch.is_some()) });
        // FES: low bit from the end of the last byte, then high
        if exp + spb < end && (high(exp - 0.5 * spb) || !high(exp + 0.5 * spb)) {
            out.push(Packet::tag("!ERR", exp - spb, exp + spb, false));
        }
    }
    out
}

/// SENT (SAE J2716) on one line.
pub static SENT: ModuleInfo = ModuleInfo::new("Sent", "SENT", "automotive")
    .starts_on(BusWhat::Data, 0x7f480b)
    // about 4 ticks per data bit; auto: the shortest tick
    .bit(|opt, _| 4.0 * sent_tick(opt).unwrap_or(SENT_TICKS[0]));

/// SENT (SAE J2716, fast channel): every nibble is the time between two falling edges in clock ticks, 12 + value.
/// A frame: synchronisation / calibration pulse of 56 ticks, status nibble, `nibbles` data nibbles (1..6), CRC
/// nibble, optional pause pulse (12..768 ticks). The tick is measured from each sync pulse; `tick` = the nominal
/// tick (s; a sync within +-25 % of 56 ticks counts), None = any tick (a frame counts only when all its nibbles are
/// valid).
///
/// Emits Field("SYNC") (value = measured tick in ns, `ok` = within +-20 % of the nominal), Field("STAT") (status
/// nibble), Data (the data nibbles, the first one most significant; value2 = their count), Crc (received CRC
/// nibble, `ok` = matches `sent_crc`, recommended method) and Field("PAUSE") (value = ticks). With a nominal tick a
/// nibble out of 0..15 after a sync gives Field("!ERR"); frames cut by the record end are skipped.
pub fn sent(v: &[f32], dt: f64, tick: Option<f64>, nibbles: usize, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    if dt.is_nan() || dt <= 0.0 {
        return out;
    }
    let nn = nibbles.clamp(1, 6);
    let f: Vec<f64> = edges(v, thr, hyst).into_iter().filter(|e| !e.rising).map(|e| e.t).collect();
    let nom = tick.map(|t| t / dt).filter(|t| t.is_finite() && *t > 0.0);
    let is_sync = |d: f64| match nom {
        Some(t) => (d / (56.0 * t) - 1.0).abs() <= 0.25,
        None => d >= 56.0 * 2.0,
    };
    let nib = |d: f64, tk: f64| {
        let x = d / tk - 12.0;
        if x > -0.5 && x < 15.5 {
            Some(x.round() as u32)
        } else {
            None
        }
    };
    let mut i = 0;
    while i + 1 < f.len() {
        let d = f[i + 1] - f[i];
        let tk = d / 56.0;
        if !is_sync(d) || tk < 2.0 {
            i += 1;
            continue;
        }
        // a pause pulse of about the sync length: the sync follows
        if let Some(&f2) = f.get(i + 2) {
            let d2 = f2 - f[i + 1];
            if if nom.is_some() { is_sync(d2) } else { (d2 / d - 1.0).abs() < 0.1 } {
                i += 1;
                continue;
            }
        }
        // status, data, CRC
        let mut vals = Vec::new();
        let mut bad = None;
        for k in 0..nn + 2 {
            let Some(&e) = f.get(i + 2 + k) else { break };
            match nib(e - f[i + 1 + k], tk) {
                Some(x) => vals.push(x),
                None => {
                    bad = Some(k);
                    break;
                }
            }
        }
        if vals.len() < nn + 2 && (nom.is_none() || bad.is_none()) {
            i += 1;
            continue;
        }
        let tick_ok = nom.is_none_or(|t| (tk / t - 1.0).abs() <= 0.2);
        out.push(Packet::field("SYNC", f[i], f[i + 1], (tk * dt * 1e9).round() as u32, tick_ok));
        if let Some(k) = bad {
            out.push(Packet::tag("!ERR", f[i + 1 + k], f[i + 2 + k], false));
            i += 1 + k;
            continue;
        }
        let e = |k: usize| f[i + 1 + k]; // start of nibble k (0 = status)
        out.push(Packet::field("STAT", e(0), e(1), vals[0], true));
        let data: Vec<u8> = vals[1..=nn].iter().map(|&x| x as u8).collect();
        let value = data.iter().fold(0u32, |a, &x| a << 4 | x as u32);
        out.push(Packet::new(Kind::Data, e(1), e(nn + 1), value, true).with_value2(nn as u32));
        out.push(Packet::new(
            Kind::Crc,
            e(nn + 1),
            e(nn + 2),
            vals[nn + 1],
            vals[nn + 1] == sent_crc(&data, false) as u32,
        ));
        i += nn + 3; // the edge that ends the CRC nibble
                     // a pause pulse: followed by a sync of this tick
        if let (Some(&a), Some(&b)) = (f.get(i + 1), f.get(i + 2)) {
            let p = (a - f[i]) / tk;
            if (11.5..=768.5).contains(&p) && ((b - a) / (56.0 * tk) - 1.0).abs() < 0.1 {
                out.push(Packet::field("PAUSE", f[i], a, p.round() as u32, true));
                i += 1;
            }
        }
    }
    out
}
