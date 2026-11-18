// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Modbus RTU and ASCII.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{field_be, modbus_crc, uart, Byte, Kind, Packet, Parity};
use crate::protocols::ModuleInfo;

/// Modbus ASCII LRC: two's complement of the 8-bit sum.
pub fn modbus_lrc(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b)).wrapping_neg()
}

/// Modbus RTU on one line or an RS-485 pair.
pub static MODBUS_RTU: ModuleInfo = ModuleInfo::new("ModbusRtu", "Modbus RTU", "modbus").differential();

/// Modbus RTU (Modbus over serial line V1.02): characters from `decode::uart` (8 data bits, `parity`), frames
/// separated by a silence of 3.5 character times (fixed 1.75 ms above 19200 baud; 20 % tolerance). Per frame: `Addr`
/// (slave address, 0 = broadcast), the PDU (see below) and Crc (value = the CRC as sent, low byte first on the wire;
/// `ok` = it matches). Frames shorter than 4 bytes: Field("!ERR").
///
/// PDU fields: Field("FUNC") function code; exception responses (function | 0x80): Field("EXC") with the exception
/// code. Function codes 1..6, 15, 16 are split into Field "REG" (start address), "QTY" (quantity), "CNT" (byte
/// count) and Data (16-bit register values for 3, 4, 6, 16, bytes otherwise); request or response is told apart by
/// the length. Other functions: Data per byte.
pub fn modbus_rtu(v: &[f32], dt: f64, baud: f64, parity: Parity, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let mut out = Vec::new();
    if !(baud > 0.0 && dt > 0.0) {
        return out;
    }
    let ch = uart(v, dt, baud, 8, parity, thr, hyst, invert);
    let bit = 1.0 / baud / dt;
    let t35 = if baud > 19200.0 { 1.75e-3 / dt } else { 3.5 * 11.0 * bit };
    let mut frame: Vec<Byte> = Vec::new();
    let flush = |frame: &mut Vec<Byte>, out: &mut Vec<Packet>| {
        let n = frame.len();
        if n == 0 {
            return;
        }
        if n < 4 {
            out.push(Packet::tag("!ERR", frame[0].1, frame[n - 1].2, false));
        } else {
            let bytes: Vec<u8> = frame.iter().map(|x| x.0).collect();
            let rx = bytes[n - 2] as u32 | (bytes[n - 1] as u32) << 8;
            let ok = rx == modbus_crc(&bytes[..n - 2]) as u32 && frame[n - 2].3 && frame[n - 1].3;
            modbus_adu(&frame[..n - 2], out);
            out.push(Packet::new(Kind::Crc, frame[n - 2].1, frame[n - 1].2, rx, ok));
        }
        frame.clear();
    };
    for c in &ch {
        if frame.last().is_some_and(|l| c.t0 - l.2 >= 0.8 * t35) {
            flush(&mut frame, &mut out);
        }
        frame.push((c.value as u8, c.t0, c.t1, c.ok));
    }
    flush(&mut frame, &mut out);
    out
}

/// Modbus ASCII on one line or an RS-485 pair.
pub static MODBUS_ASCII: ModuleInfo = ModuleInfo::new("ModbusAscii", "Modbus ASCII", "modbus").differential();

/// Modbus ASCII (Modbus over serial line V1.02, 2.5.2): characters from `decode::uart` (`bits` = 7 normally, 8 also
/// accepted); Start at ':', hex pairs (address, PDU, LRC), Stop at CR LF. Same fields as `modbus_rtu`, the LRC as
/// Crc (`ok` = it matches). Bad hex characters or an odd count: Field("!ERR").
pub fn modbus_ascii(
    v: &[f32],
    dt: f64,
    baud: f64,
    bits: u32,
    parity: Parity,
    thr: f32,
    hyst: f32,
    invert: bool,
) -> Vec<Packet> {
    let mut out = Vec::new();
    if !(baud > 0.0 && dt > 0.0) || !(7..=8).contains(&bits) {
        return out;
    }
    let ch = uart(v, dt, baud, bits, parity, thr, hyst, invert);
    let hex = |c: u32| char::from_u32(c).and_then(|c| c.to_digit(16));
    let mut i = 0;
    while i < ch.len() {
        if ch[i].value != b':' as u32 {
            i += 1;
            continue;
        }
        out.push(Packet::new(Kind::Start, ch[i].t0, ch[i].t1, 0, ch[i].ok));
        let mut j = i + 1;
        while j < ch.len() && ch[j].value != 0x0d && ch[j].value != b':' as u32 {
            j += 1;
        }
        let body = &ch[i + 1..j];
        let mut bad = !body.len().is_multiple_of(2);
        let mut bytes: Vec<Byte> = Vec::new();
        for p in body.as_chunks::<2>().0 {
            match (hex(p[0].value), hex(p[1].value)) {
                (Some(h), Some(l)) => bytes.push(((h << 4 | l) as u8, p[0].t0, p[1].t1, p[0].ok && p[1].ok)),
                _ => bad = true,
            }
        }
        let n = bytes.len();
        if bad || n < 3 {
            out.push(Packet::tag("!ERR", ch[i].t1, body.last().map_or(ch[i].t1, |c| c.t1), false));
        } else {
            let raw: Vec<u8> = bytes.iter().map(|x| x.0).collect();
            modbus_adu(&bytes[..n - 1], &mut out);
            let ok = modbus_lrc(&raw[..n - 1]) == raw[n - 1] && bytes[n - 1].3;
            out.push(Packet::new(Kind::Crc, bytes[n - 1].1, bytes[n - 1].2, raw[n - 1] as u32, ok));
        }
        if j + 1 < ch.len() && ch[j].value == 0x0d && ch[j + 1].value == 0x0a {
            out.push(Packet::new(Kind::Stop, ch[j].t0, ch[j + 1].t1, 0, ch[j].ok && ch[j + 1].ok));
            i = j + 2;
        } else {
            i = j.max(i + 1);
        }
    }
    out
}

/// Address + PDU of a Modbus frame (without the check).
fn modbus_adu(b: &[Byte], out: &mut Vec<Packet>) {
    let Some(a) = b.first() else { return };
    out.push(Packet::new(Kind::Addr { read: false }, a.1, a.2, a.0 as u32, a.3));
    let Some(f) = b.get(1) else { return };
    let func = f.0;
    out.push(Packet::field("FUNC", f.1, f.2, func as u32, f.3));
    let d = &b[2..];
    let words = |out: &mut Vec<Packet>, from: usize| {
        let mut i = from;
        while i + 1 < d.len() {
            out.push(Packet::new(
                Kind::Data,
                d[i].1,
                d[i + 1].2,
                (d[i].0 as u32) << 8 | d[i + 1].0 as u32,
                d[i].3 && d[i + 1].3,
            ));
            i += 2;
        }
        bytes_from(out, d, i);
    };
    if func & 0x80 != 0 {
        if field_be(out, d, "EXC", 0, 1, true) {
            bytes_from(out, d, 1);
        }
        return;
    }
    let n = d.len();
    let cnt = |i: usize| d.get(i).map_or(usize::MAX, |x| x.0 as usize);
    match func {
        1..=4 if n == 4 => {
            field_be(out, d, "REG", 0, 2, true);
            field_be(out, d, "QTY", 2, 2, true);
        }
        1..=4 if n >= 1 && n == 1 + cnt(0) => {
            field_be(out, d, "CNT", 0, 1, true);
            if func >= 3 {
                words(out, 1)
            } else {
                bytes_from(out, d, 1)
            }
        }
        5 | 6 if n == 4 => {
            field_be(out, d, "REG", 0, 2, true);
            words(out, 2);
        }
        15 | 16 if n == 4 => {
            field_be(out, d, "REG", 0, 2, true);
            field_be(out, d, "QTY", 2, 2, true);
        }
        15 | 16 if n >= 5 && n == 5 + cnt(4) => {
            field_be(out, d, "REG", 0, 2, true);
            field_be(out, d, "QTY", 2, 2, true);
            field_be(out, d, "CNT", 4, 1, true);
            if func == 16 {
                words(out, 5)
            } else {
                bytes_from(out, d, 5)
            }
        }
        _ => bytes_from(out, d, 0),
    }
}

fn bytes_from(out: &mut Vec<Packet>, d: &[Byte], from: usize) {
    for x in d.iter().skip(from) {
        out.push(Packet::new(Kind::Data, x.1, x.2, x.0 as u32, x.3));
    }
}
