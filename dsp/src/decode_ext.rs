// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Extended decoders, part 1: parallel bus, UART auto baud, 9-bit multidrop UART / RS-485, DMX512 + RDM, Modbus
//! RTU/ASCII, 1-Wire (direct and through a DS2482 I2C bridge), CAN FD, FlexRay.
//!
//! Same conventions as `decode`: analog inputs with threshold `thr` + hysteresis `hyst`, packet times are fractional
//! sample indices, `ok` = the check of the field (CRC, parity, framing) passed. Fields without an own `Kind` use
//! `Kind::Field(label)`. Signal generators for tests and the demo source are in `gen`.

use crate::decode::{can_crc15, uart, Kind, Packet, Parity};
use crate::edges::{edges, states, Edge};
use crate::trigger::Slope;

fn pk(kind: Kind, t0: f64, t1: f64, value: u32, value2: Option<u32>, ok: bool) -> Packet {
    Packet { t0, t1, kind, value, value2, ok }
}

/// A value-less event tag (label starting with '!').
fn tag(label: &'static str, t0: f64, t1: f64, ok: bool) -> Packet {
    pk(Kind::Field(label), t0, t1, 0, None, ok)
}

/// Bitwise CRC, MSB first, without reflection or final XOR (`poly` may include the x^len term).
fn crc_msb(bits: impl IntoIterator<Item = bool>, len: u32, poly: u32, init: u32) -> u32 {
    let mask = if len >= 32 { u32::MAX } else { (1u32 << len) - 1 };
    let mut crc = init & mask;
    for b in bits {
        let top = (crc >> (len - 1)) & 1 == 1;
        crc = (crc << 1) & mask;
        if top != b {
            crc ^= poly & mask;
        }
    }
    crc
}

// ---------------------------------------------------------------------------------------------------------------
// Parallel bus

/// Parallel bus: the data lines (`data[0]` = LSB, at most 32 are used) sampled on each `edge` of `clk`, or without a
/// clock after every change of any line (changes less than 2 samples apart count as one, for line skew; the first
/// packet is the bus word at the record start, when all lines are defined). One Data packet per sample: value = the bus word, t0 = the sampling
/// time, t1 = the next one (or the record end); `ok` = every line had a defined level.
pub fn parallel(data: &[&[f32]], clk: Option<&[f32]>, edge: Slope, thr: f32, hyst: f32) -> Vec<Packet> {
    let data = &data[..data.len().min(32)];
    let mut out = Vec::new();
    let n = data.iter().map(|d| d.len()).min().unwrap_or(0);
    if n == 0 {
        return out;
    }
    let st: Vec<Vec<Option<bool>>> = data.iter().map(|d| states(d, thr, hyst)).collect();
    let word = |i: usize| {
        let i = i.min(n - 1);
        let mut v = 0u32;
        let mut ok = true;
        for (k, s) in st.iter().enumerate() {
            match s[i] {
                Some(true) => v |= 1 << k,
                Some(false) => {}
                None => ok = false,
            }
        }
        (v, ok)
    };
    // (time, sample index to read)
    let mut at: Vec<(f64, usize)> = Vec::new();
    if let Some(c) = clk {
        for e in edges(c, thr, hyst) {
            let m = match edge {
                Slope::Rising => e.rising,
                Slope::Falling => !e.rising,
                Slope::Either => true,
            };
            if m {
                at.push((e.t, (e.t + 0.5) as usize));
            }
        }
    } else {
        const SKEW: f64 = 2.0;
        let mut ts: Vec<f64> = data.iter().flat_map(|d| edges(&d[..n], thr, hyst)).map(|e| e.t).collect();
        ts.sort_by(|a, b| a.total_cmp(b));
        let mut i = 0;
        while i < ts.len() {
            let t0 = ts[i];
            let mut t1 = t0;
            while i + 1 < ts.len() && ts[i + 1] - t1 <= SKEW {
                i += 1;
                t1 = ts[i];
            }
            at.push((t0, t1.ceil() as usize + 1));
            i += 1;
        }
        let before = at.first().map_or(n - 1, |a| (a.0.floor() as usize).saturating_sub(1));
        if word(before).1 {
            at.insert(0, (0.0, before));
        }
    }
    for (k, &(t, i)) in at.iter().enumerate() {
        let (v, ok) = word(i);
        let t1 = at.get(k + 1).map_or((n - 1) as f64, |a| a.0);
        out.push(pk(Kind::Data, t, t1.max(t), v, None, ok));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// UART auto baud, 9-bit multidrop, RS-485

/// Standard UART bit rates `uart_baud` snaps to (incl. MIDI 31250 and DMX512 250000).
pub const STD_BAUD: [f64; 25] = [
    300.0, 600.0, 1200.0, 2400.0, 4800.0, 9600.0, 14400.0, 19200.0, 28800.0, 31250.0, 38400.0, 57600.0, 76800.0, 115200.0, 230400.0,
    250000.0, 460800.0, 500000.0, 921600.0, 1e6, 1.5e6, 2e6, 2.5e6, 3e6, 4e6,
];

/// Bit rate of an asynchronous serial signal: the shortest pulse width that occurs consistently (at least 2 pulses
/// and 5 % of all, so single glitches do not count) is the first estimate of a bit time; it is refined by a least
/// squares fit over all pulses that are near integer multiples of it (1..12 bits). The result is snapped to the
/// nearest `STD_BAUD` within ±3 %, else returned as estimated. None without enough pulses or below 2 samples/bit.
pub fn uart_baud(v: &[f32], dt: f64, thr: f32, hyst: f32) -> Option<f64> {
    let e = edges(v, thr, hyst);
    let w: Vec<f64> = e.windows(2).map(|p| p[1].t - p[0].t).filter(|&x| x > 0.0).collect();
    if w.len() < 3 || !(dt > 0.0) {
        return None;
    }
    let mut s = w.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    let need = (s.len() / 20).max(2);
    let mut tb = None;
    for &c in &s {
        if c < 1.5 {
            continue;
        }
        let support = s.partition_point(|&x| x <= 1.25 * c) - s.partition_point(|&x| x < 0.8 * c);
        if support >= need {
            tb = Some(c);
            break;
        }
    }
    let mut tb = tb?;
    for _ in 0..3 {
        let (mut sw, mut sn) = (0.0, 0.0);
        for &x in &w {
            let n = (x / tb).round();
            if (1.0..=12.0).contains(&n) && (x / tb - n).abs() < 0.3 {
                sw += x;
                sn += n;
            }
        }
        if sn > 0.0 {
            tb = sw / sn;
        }
    }
    let baud = 1.0 / (tb * dt);
    let best = STD_BAUD.iter().copied().min_by(|a, b| ((a - baud).abs() / a).total_cmp(&((b - baud).abs() / b)))?;
    Some(if (best - baud).abs() / best <= 0.03 { best } else { baud })
}

/// Differential pair (RS-485 / RS-422 A/B, FlexRay BP/BM, CAN H/L): A − B sample by sample (the shorter length).
/// Decode the result with a threshold of 0 V and a hysteresis of the receiver (RS-485: ±200 mV → 0.4).
pub fn diff(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x - y).collect()
}

/// 9-bit multidrop UART (start, 9 data bits LSB first, stop; the 9th bit marks an address character, as in RS-485
/// multidrop / MDB): `Addr { read: false }` with value = the low 8 bits when the 9th bit is set, else Data;
/// value2 = the 9th bit; `ok` = stop bit.
pub fn uart9(v: &[f32], dt: f64, baud: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    uart(v, dt, baud, 9, Parity::None, thr, hyst, invert)
        .into_iter()
        .map(|p| {
            let a = (p.value >> 8) & 1;
            Packet { kind: if a == 1 { Kind::Addr { read: false } } else { Kind::Data }, value: p.value & 0xff, value2: Some(a), ..p }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// DMX512 / RDM

/// DMX512 bit rate.
pub const DMX_BAUD: f64 = 250e3;
/// RDM start code (ANSI E1.20).
pub const RDM_SC: u8 = 0xcc;

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
    if !(spb >= 2.5) {
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
        out.push(pk(Kind::Break, b0, b1, 0, None, true));
        let Some(m1) = m1 else { continue };
        out.push(pk(Kind::Field("MAB"), b1, m1, ((m1 - b1) / us).round() as u32, None, m1 - b1 >= 8.0 * us - 1.0));
        let slots: Vec<Byte> = chars
            .iter()
            .filter(|c| c.t0 >= b1 - 0.5 && c.t0 < end)
            .map(|c| (c.value as u8, c.t0, c.t0 + 11.0 * spb, c.ok && mark(c.t0 + 10.5 * spb) == Some(true)))
            .collect();
        let Some(&(sc, s0, s1, sok)) = slots.first() else { continue };
        out.push(pk(Kind::Field("SC"), s0, s1, sc as u32, None, sok));
        if sc == RDM_SC {
            rdm(&slots, &mut out);
        } else {
            for (i, s) in slots.iter().enumerate().skip(1) {
                out.push(pk(Kind::Data, s.1, s.2, s.0 as u32, Some(i as u32), s.3));
            }
        }
    }
    out
}

/// A decoded byte: (value, t0, t1, framing ok).
type Byte = (u8, f64, f64, bool);

/// Field over bytes a..a+n, big endian (> 4 bytes: value = low 32 bits, value2 = the rest). False when truncated.
fn field_be(out: &mut Vec<Packet>, s: &[Byte], label: &'static str, a: usize, n: usize, ok: bool) -> bool {
    if n == 0 || a + n > s.len() {
        return false;
    }
    let v = s[a..a + n].iter().fold(0u64, |acc, x| acc << 8 | x.0 as u64);
    let fok = s[a..a + n].iter().all(|x| x.3);
    out.push(pk(Kind::Field(label), s[a].1, s[a + n - 1].2, v as u32, if n > 4 { Some((v >> 32) as u32) } else { None }, ok && fok));
    true
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
        out.push(tag("!ERR", l.1, l.2, false));
    };
    for &(label, a, n, ok) in &layout {
        if !field_be(out, s, label, a, n, ok) {
            return trunc(out);
        }
    }
    for x in s.iter().skip(24).take(pdl) {
        out.push(pk(Kind::Data, x.1, x.2, x.0 as u32, None, x.3));
    }
    let cs_at = if len >= 24 { len } else { 24 + pdl };
    if cs_at + 2 > s.len() {
        return trunc(out);
    }
    let rx = (b(cs_at) << 8 | b(cs_at + 1)) as u32;
    let sum = s[..cs_at].iter().map(|x| x.0 as u32).sum::<u32>() & 0xffff;
    out.push(pk(Kind::Crc, s[cs_at].1, s[cs_at + 1].2, rx, None, rx == sum && s[cs_at].3 && s[cs_at + 1].3));
}

// ---------------------------------------------------------------------------------------------------------------
// Modbus

/// CRC-16/MODBUS (Modbus over serial line V1.02, 6.2.2): init 0xFFFF, reflected polynomial 0xA001; sent low byte first.
pub fn modbus_crc(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &b in bytes {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 == 1 { crc >> 1 ^ 0xa001 } else { crc >> 1 };
        }
    }
    crc
}

/// Modbus ASCII LRC: two's complement of the 8-bit sum.
pub fn modbus_lrc(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b)).wrapping_neg()
}

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
            out.push(tag("!ERR", frame[0].1, frame[n - 1].2, false));
        } else {
            let bytes: Vec<u8> = frame.iter().map(|x| x.0).collect();
            let rx = bytes[n - 2] as u32 | (bytes[n - 1] as u32) << 8;
            let ok = rx == modbus_crc(&bytes[..n - 2]) as u32 && frame[n - 2].3 && frame[n - 1].3;
            modbus_adu(&frame[..n - 2], out);
            out.push(pk(Kind::Crc, frame[n - 2].1, frame[n - 1].2, rx, None, ok));
        }
        frame.clear();
    };
    for c in &ch {
        if frame.last().map_or(false, |l| c.t0 - l.2 >= 0.8 * t35) {
            flush(&mut frame, &mut out);
        }
        frame.push((c.value as u8, c.t0, c.t1, c.ok));
    }
    flush(&mut frame, &mut out);
    out
}

/// Modbus ASCII (Modbus over serial line V1.02, 2.5.2): characters from `decode::uart` (`bits` = 7 normally, 8 also
/// accepted); Start at ':', hex pairs (address, PDU, LRC), Stop at CR LF. Same fields as `modbus_rtu`, the LRC as
/// Crc (`ok` = it matches). Bad hex characters or an odd count: Field("!ERR").
pub fn modbus_ascii(v: &[f32], dt: f64, baud: f64, bits: u32, parity: Parity, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
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
        out.push(pk(Kind::Start, ch[i].t0, ch[i].t1, 0, None, ch[i].ok));
        let mut j = i + 1;
        while j < ch.len() && ch[j].value != 0x0d && ch[j].value != b':' as u32 {
            j += 1;
        }
        let body = &ch[i + 1..j];
        let mut bad = body.len() % 2 != 0;
        let mut bytes: Vec<Byte> = Vec::new();
        for p in body.chunks_exact(2) {
            match (hex(p[0].value), hex(p[1].value)) {
                (Some(h), Some(l)) => bytes.push(((h << 4 | l) as u8, p[0].t0, p[1].t1, p[0].ok && p[1].ok)),
                _ => bad = true,
            }
        }
        let n = bytes.len();
        if bad || n < 3 {
            out.push(tag("!ERR", ch[i].t1, body.last().map_or(ch[i].t1, |c| c.t1), false));
        } else {
            let raw: Vec<u8> = bytes.iter().map(|x| x.0).collect();
            modbus_adu(&bytes[..n - 1], &mut out);
            let ok = modbus_lrc(&raw[..n - 1]) == raw[n - 1] && bytes[n - 1].3;
            out.push(pk(Kind::Crc, bytes[n - 1].1, bytes[n - 1].2, raw[n - 1] as u32, None, ok));
        }
        if j + 1 < ch.len() && ch[j].value == 0x0d && ch[j + 1].value == 0x0a {
            out.push(pk(Kind::Stop, ch[j].t0, ch[j + 1].t1, 0, None, ch[j].ok && ch[j + 1].ok));
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
    out.push(pk(Kind::Addr { read: false }, a.1, a.2, a.0 as u32, None, a.3));
    let Some(f) = b.get(1) else { return };
    let func = f.0;
    out.push(pk(Kind::Field("FUNC"), f.1, f.2, func as u32, None, f.3));
    let d = &b[2..];
    let words = |out: &mut Vec<Packet>, from: usize| {
        let mut i = from;
        while i + 1 < d.len() {
            out.push(pk(Kind::Data, d[i].1, d[i + 1].2, (d[i].0 as u32) << 8 | d[i + 1].0 as u32, None, d[i].3 && d[i + 1].3));
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
        out.push(pk(Kind::Data, x.1, x.2, x.0 as u32, None, x.3));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// 1-Wire

/// 1-Wire CRC-8 (x^8 + x^5 + x^4 + 1, reflected 0x8C, init 0), as in the ROM code and scratchpads.
pub fn ow_crc8(bytes: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in bytes {
        let mut x = b;
        for _ in 0..8 {
            let mix = (c ^ x) & 1;
            c >>= 1;
            if mix != 0 {
                c ^= 0x8c;
            }
            x >>= 1;
        }
    }
    c
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OwPhase {
    /// before the first reset: bytes are shown as Data
    Idle,
    /// the ROM command follows
    Rom,
    /// 8 ROM code bytes (READ / MATCH ROM)
    Code,
    /// 64 search triplets (SEARCH ROM / ALARM SEARCH)
    Search,
    /// function command and data until the next reset
    Func,
}

/// 1-Wire network layer above the bit level (shared by `onewire` and `ds2482`).
struct OwNet {
    out: Vec<Packet>,
    phase: OwPhase,
    /// bits of the byte or search triplet being assembled
    bits: Vec<(bool, f64, f64)>,
    /// search: chosen directions = ROM code bits
    dirs: Vec<(bool, f64, f64)>,
    code: Vec<(u8, f64, f64)>,
    /// set by OVERDRIVE SKIP / MATCH ROM: the following time slots run at overdrive speed
    overdrive: bool,
}

impl OwNet {
    fn new(overdrive: bool) -> OwNet {
        OwNet { out: Vec::new(), phase: OwPhase::Idle, bits: Vec::new(), dirs: Vec::new(), code: Vec::new(), overdrive }
    }

    fn reset(&mut self, t0: f64, t1: f64) {
        self.bits.clear();
        self.dirs.clear();
        self.code.clear();
        self.phase = OwPhase::Rom;
        self.out.push(tag("!RESET", t0, t1, true));
    }

    fn presence(&mut self, t0: f64, t1: f64, ok: bool) {
        self.out.push(tag("!PRESENCE", t0, t1, ok));
    }

    fn bit(&mut self, b: bool, t0: f64, t1: f64) {
        self.bits.push((b, t0, t1));
        if self.phase == OwPhase::Search {
            if self.bits.len() < 3 {
                return;
            }
            let (id, cmp, dir) = (self.bits[0].0, self.bits[1].0, self.bits[2].0);
            let ts = self.bits[0].1;
            self.bits.clear();
            if id && cmp {
                // no device answered: the search ends
                self.out.push(tag("!ERR", ts, t1, false));
                self.phase = OwPhase::Func;
                return;
            }
            self.dirs.push((dir, ts, t1));
            if self.dirs.len() == 8 {
                let v = self.dirs.iter().enumerate().fold(0u8, |a, (i, d)| a | (d.0 as u8) << i);
                let (s, e) = (self.dirs[0].1, self.dirs[7].2);
                self.dirs.clear();
                self.code.push((v, s, e));
                if self.code.len() == 8 {
                    self.rom_code();
                    self.phase = OwPhase::Func;
                }
            }
            return;
        }
        if self.bits.len() == 8 {
            let v = self.bits.iter().enumerate().fold(0u8, |a, (i, d)| a | (d.0 as u8) << i);
            let s = self.bits[0].1;
            self.byte(v, s, t1);
        }
    }

    fn byte(&mut self, v: u8, t0: f64, t1: f64) {
        self.bits.clear();
        match self.phase {
            OwPhase::Rom => {
                self.out.push(pk(Kind::Field("ROM"), t0, t1, v as u32, None, true));
                self.phase = match v {
                    0x33 | 0x0f | 0x55 | 0x69 => OwPhase::Code,
                    0xf0 | 0xec => OwPhase::Search,
                    _ => OwPhase::Func,
                };
                if v == 0x3c || v == 0x69 {
                    self.overdrive = true;
                }
            }
            OwPhase::Code => {
                self.code.push((v, t0, t1));
                if self.code.len() == 8 {
                    self.rom_code();
                    self.phase = OwPhase::Func;
                }
            }
            _ => self.out.push(pk(Kind::Data, t0, t1, v as u32, None, true)),
        }
    }

    /// The 64-bit ROM code: family code, 48-bit serial number, CRC-8.
    fn rom_code(&mut self) {
        let c = std::mem::take(&mut self.code);
        let b = |i: usize| c[i].0 as u32;
        self.out.push(pk(Kind::Field("FAM"), c[0].1, c[0].2, b(0), None, true));
        self.out.push(pk(Kind::Field("SER"), c[1].1, c[6].2, b(1) | b(2) << 8 | b(3) << 16 | b(4) << 24, Some(b(5) | b(6) << 8), true));
        let bytes: Vec<u8> = c.iter().map(|x| x.0).collect();
        self.out.push(pk(Kind::Crc, c[7].1, c[7].2, b(7), None, ow_crc8(&bytes[..7]) == bytes[7]));
    }
}

/// 1-Wire (timings of application note AN126, standard and overdrive speed) on the bus line, idle high. Low
/// pulses are classified by length: standard: < 15 us = 1, < 150 us = 0, >= 300 us = reset; overdrive: < 2 us = 1,
/// < 40 us = 0, >= 40 us = reset; a reset >= 400 us returns to standard speed, OVERDRIVE SKIP/MATCH ROM (0x3C/0x69)
/// switch to overdrive. `overdrive` = speed at the record start.
///
/// Emits Field("!RESET"), Field("!PRESENCE") (`ok` = a presence pulse followed), Field("ROM") = the ROM command;
/// READ/MATCH ROM and SEARCH ROM / ALARM SEARCH (the direction bits of the 64 triplets) give the ROM code as
/// Field("FAM") (family), Field("SER") (48-bit serial: value = low 32 bits, value2 = high 16) and Crc (`ok` = CRC-8
/// matches); a search where no device answers gives Field("!ERR"). All other bytes (function commands, data) are
/// Data, LSB first. Pulses of no valid length: Field("!ERR").
pub fn onewire(v: &[f32], dt: f64, thr: f32, hyst: f32, overdrive: bool) -> Vec<Packet> {
    let us = 1e-6 / dt;
    let mut net = OwNet::new(overdrive);
    if !(us >= 0.5) {
        return net.out;
    }
    let e = edges(v, thr, hyst);
    let p: Vec<(f64, f64)> = e.windows(2).filter(|w| !w[0].rising && w[1].rising).map(|w| (w[0].t, w[1].t)).collect();
    let mut i = 0;
    while i < p.len() {
        let (f, r) = p[i];
        i += 1;
        let w = (r - f) / us;
        let (one, zero, rst, slot) = if net.overdrive { (2.0, 40.0, 40.0, 8.0) } else { (15.0, 150.0, 300.0, 60.0) };
        if w >= rst {
            if w >= 400.0 {
                net.overdrive = false;
            }
            net.reset(f, r);
            // presence: the slaves pull low 15..60 us after the reset (overdrive 2..6) for 60..240 us (8..24)
            let (wait, plo, phi, win) = if net.overdrive { (10.0, 4.0, 40.0, 10.0) } else { (80.0, 30.0, 300.0, 70.0) };
            match p.get(i) {
                Some(&(pf, pr)) if pf - r <= wait * us && (pr - pf) / us >= plo && (pr - pf) / us < phi => {
                    net.presence(pf, pr, true);
                    i += 1;
                }
                _ => net.presence(r, r + win * us, false),
            }
        } else if w < zero {
            net.bit(w < one, f, r.max(f + slot * us));
        } else {
            net.out.push(tag("!ERR", f, r, false));
        }
    }
    net.out
}

/// DS2482 register pointer codes.
const DS_STATUS: u8 = 0xf0;
const DS_DATA: u8 = 0xe1;

#[derive(Clone, Copy, Debug)]
enum DsPend {
    Reset(f64),
    Bit(bool, f64),
    Read(f64),
    Triplet(bool, f64),
}

/// 1-Wire through a DS2482-100/-800 I2C bridge: interprets the command stream (packets from `decode::i2c`, device
/// addresses 0x18..0x1F) as 1-Wire activity and decodes it like `onewire` (Field "!RESET", "!PRESENCE", "ROM", "FAM",
/// "SER", Crc, Data). Device reset gives Field("!DRST"), Write Configuration Field("CFG") (value = config nibble,
/// `ok` = the complement nibble is right), Channel Select Field("CH") (value = channel 0..7). Results of 1-Wire
/// reset / single bit / triplet come from the next status read with 1WB = 0 (presence = PPD and not SD); a read
/// byte from the following read of the read data register (pointer 0xE1). Commands without a status poll are
/// completed at the next command.
pub fn ds2482(i2c: &[Packet]) -> Vec<Packet> {
    let mut net = OwNet::new(false);
    let mut ptr = DS_STATUS;
    let mut pend: Option<DsPend> = None;
    let mut read_ready: Option<f64> = None;
    // a transaction: (read, t0, bytes)
    let mut xfers: Vec<(bool, f64, Vec<(u8, f64, f64)>)> = Vec::new();
    let mut cur: Option<(bool, f64, Vec<(u8, f64, f64)>)> = None;
    for p in i2c {
        match p.kind {
            Kind::Addr { read } => {
                xfers.extend(cur.take());
                if (0x18..=0x1f).contains(&p.value) {
                    cur = Some((read, p.t0, Vec::new()));
                }
            }
            Kind::Data => {
                if let Some(c) = cur.as_mut() {
                    c.2.push((p.value as u8, p.t0, p.t1));
                }
            }
            Kind::Start | Kind::RepStart | Kind::Stop => xfers.extend(cur.take()),
            _ => {}
        }
    }
    xfers.extend(cur.take());
    // completes a pending 1-Wire command (status = the status byte that showed 1WB = 0, if polled)
    let finish = |net: &mut OwNet, pend: DsPend, status: Option<u8>, t1: f64, read_ready: &mut Option<f64>| match pend {
        DsPend::Reset(t0) => {
            net.reset(t0, t1);
            if let Some(s) = status {
                net.presence(t0, t1, s & 0x02 != 0 && s & 0x04 == 0);
            }
        }
        DsPend::Bit(b, t0) => net.bit(status.map_or(b, |s| s & 0x20 != 0), t0, t1),
        DsPend::Triplet(dir, t0) => {
            let (id, cmp, d) = status.map_or((dir, !dir, dir), |s| (s & 0x20 != 0, s & 0x40 != 0, s & 0x80 != 0));
            net.bit(id, t0, t1);
            net.bit(cmp, t0, t1);
            net.bit(d, t0, t1);
        }
        DsPend::Read(t0) => *read_ready = Some(t0),
    };
    for (read, t0, b) in xfers {
        let t1 = b.last().map_or(t0, |x| x.2);
        if read {
            for &(v, _, bt1) in &b {
                match ptr {
                    DS_STATUS if v & 0x01 == 0 => {
                        if let Some(pd) = pend.take() {
                            finish(&mut net, pd, Some(v), bt1, &mut read_ready);
                        }
                    }
                    DS_DATA => {
                        let s = match pend {
                            Some(DsPend::Read(s)) => {
                                pend = None;
                                Some(s)
                            }
                            _ => read_ready.take(),
                        };
                        if let Some(s) = s {
                            net.byte(v, s, bt1);
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        let Some(&(cmd, _, _)) = b.first() else { continue };
        let par = b.get(1).map(|x| x.0);
        // a new 1-Wire or device command completes the previous one (not Set Read Pointer, used for polling)
        if cmd != 0xe1 {
            if let Some(pd) = pend.take() {
                finish(&mut net, pd, None, t0, &mut read_ready);
            }
        }
        match (cmd, par) {
            (0xf0, _) => {
                ptr = DS_STATUS;
                net.out.push(tag("!DRST", t0, t1, true));
            }
            (0xe1, Some(p)) => ptr = p,
            (0xd2, Some(c)) => {
                ptr = 0xc3;
                net.out.push(pk(Kind::Field("CFG"), t0, t1, (c & 0x0f) as u32, None, c >> 4 == !c & 0x0f));
            }
            (0xc3, Some(c)) => {
                ptr = 0xd2;
                let ch = [0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0xa5, 0x96, 0x87].iter().position(|&x| x == c);
                net.out.push(pk(Kind::Field("CH"), t0, t1, ch.unwrap_or(0) as u32, None, ch.is_some()));
            }
            (0xb4, _) => {
                ptr = DS_STATUS;
                pend = Some(DsPend::Reset(t0));
            }
            (0x87, Some(x)) => {
                ptr = DS_STATUS;
                pend = Some(DsPend::Bit(x & 0x80 != 0, t0));
            }
            (0xa5, Some(x)) => {
                ptr = DS_STATUS;
                net.byte(x, t0, t1);
            }
            (0x96, _) => {
                ptr = DS_STATUS;
                pend = Some(DsPend::Read(t0));
            }
            (0x78, Some(x)) => {
                ptr = DS_STATUS;
                pend = Some(DsPend::Triplet(x & 0x80 != 0, t0));
            }
            _ => {}
        }
    }
    if let Some(pd) = pend.take() {
        let t = net.out.last().map_or(0.0, |p| p.t1);
        finish(&mut net, pd, None, t, &mut read_ready);
    }
    net.out
}

// ---------------------------------------------------------------------------------------------------------------
// CAN FD

/// Payload length per DLC in CAN FD frames.
pub const FD_LEN: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];
/// Sample point (fraction of the bit) where the bit rate switches (BRS bit, CRC delimiter); assumed by `can_fd`
/// and used by `gen::can_fd_bits`.
pub const FD_SP: f64 = 0.75;

/// CAN FD CRC (ISO 11898-1:2015): CRC-17 (0x3685B) for up to 16 data bytes, CRC-21 (0x302899) above, register
/// initialised with a 1 in the MSB; over SOF .. data field including the dynamic stuff bits, then the stuff count.
pub fn can_fd_crc(bits: &[bool], len: u32) -> u32 {
    let poly = if len == 21 { 0x30_2899 } else { 0x3_685b };
    crc_msb(bits.iter().copied(), len, poly, 1 << (len - 1))
}

/// Bit timing reader: one bit at a time from the current position, resynchronised on edges near the bit end.
struct BitClock<'a> {
    t: f64,
    rec: &'a dyn Fn(f64) -> Option<bool>,
    e: &'a [Edge],
}

impl BitClock<'_> {
    /// One bit sampled `smp` samples after its start; `f(level)` = (bit length, resync tolerance). The next bit starts
    /// at an edge within the tolerance of the nominal end, else at the nominal end. (level, start, end)
    fn bit(&mut self, smp: f64, f: impl Fn(bool) -> (f64, f64)) -> Option<(bool, f64, f64)> {
        let s = self.t;
        let b = (self.rec)(s + smp)?;
        let (len, tol) = f(b);
        let nominal = s + len;
        let i = self.e.partition_point(|x| x.t < nominal - tol);
        self.t = match self.e.get(i) {
            Some(x) if x.t < nominal + tol => x.t,
            _ => nominal,
        };
        Some((b, s, self.t))
    }
}

/// CAN FD (ISO 11898-1:2015) and classic CAN frames (base and extended format) on one line, `invert` as in
/// `decode::can`. Nominal bit rate `nominal`, data phase `data_rate` (from the sample point of a recessive BRS bit to
/// the sample point of the CRC delimiter, `FD_SP`). Dynamic stuffing up to the end of the data field, then the stuff
/// count and CRC with fixed stuff bits (complement of the preceding bit, before the stuff count and after every 4
/// bits; it replaces a dynamic stuff bit due at the data field end).
///
/// Emits Start (SOF), Id { ext, rtr } (rtr only for classic remote frames), FD only: Field("BRS"), Field("ESI");
/// Field("DLC") (value = DLC, value2 = data bytes), Data per byte, FD only: Field("SBC") (stuff count mod 8 from the
/// Gray code; `ok` = parity and count right), Crc (value = received CRC-15/17/21, `ok` = it matches and the fixed
/// stuff bits are right), Ack (`ok` = dominant). A stuff error ends the frame with Field("!ERR").
pub fn can_fd(v: &[f32], dt: f64, nominal: f64, data_rate: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let tn = 1.0 / nominal / dt;
    let td = 1.0 / data_rate / dt;
    let mut out = Vec::new();
    if !(tn >= 3.0 && td >= 3.0) {
        return out;
    }
    let st = states(v, thr, hyst);
    let rec = |x: f64| -> Option<bool> { st.get(x as usize).copied().flatten().map(|b| b != invert) };
    let e = edges(v, thr, hyst);
    let mut busy_until = 0.0;
    let mut last_rec_start: Option<f64> = None;
    for (k, x) in e.iter().enumerate() {
        if x.rising != invert {
            last_rec_start = Some(x.t);
            continue;
        }
        if x.t < busy_until {
            continue;
        }
        let idle_from = last_rec_start.unwrap_or(if k == 0 && rec(x.t - 1.0) == Some(true) { 0.0 } else { f64::INFINITY });
        if x.t - idle_from < 7.0 * tn {
            continue;
        }
        let mut clk = BitClock { t: x.t, rec: &rec, e: &e };
        if let Some((pk, end)) = can_fd_frame(&mut clk, tn, td) {
            busy_until = end;
            out.extend(pk);
        }
    }
    out
}

/// One CAN / CAN FD frame from its SOF: the packets and the time after the ACK delimiter.
fn can_fd_frame(clk: &mut BitClock, tn: f64, td: f64) -> Option<(Vec<Packet>, f64)> {
    let sof = clk.t;
    let mut raw: Vec<bool> = Vec::new(); // SOF .. data end with the dynamic stuff bits (FD CRC input)
    let mut bits: Vec<(bool, f64, f64)> = Vec::new(); // de-stuffed
    let (mut run, mut last) = (0u32, None::<bool>);
    let mut stuff_cnt = 0u32;
    let mut need = 14usize; // up to IDE first
    let (mut ext, mut fd, mut fast) = (false, false, false);
    let (mut fdf_at, mut dlc_at, mut data_end, mut brs_at) = (usize::MAX, usize::MAX, usize::MAX, usize::MAX);
    let nominal = |_: bool| (tn, 0.4 * tn);
    let data = |_: bool| (td, 0.4 * td);
    // the dynamically stuffed part; classic frames include the CRC-15
    while bits.len() < need {
        let idx = bits.len();
        let stuff = run == 5;
        let b = if fast {
            clk.bit(0.65 * td, data)?
        } else if !stuff && idx == brs_at {
            // BRS: a recessive BRS switches to the data bit rate at its sample point
            clk.bit(0.65 * tn, |l| if l { (FD_SP * tn + (1.0 - FD_SP) * td, 0.4 * td) } else { (tn, 0.4 * tn) })?
        } else {
            clk.bit(0.65 * tn, nominal)?
        };
        raw.push(b.0);
        if stuff {
            if Some(b.0) == last {
                return Some((vec![pk(Kind::Start, sof, bits[0].2, 0, None, true), tag("!ERR", b.1, b.2, false)], b.2));
            }
            run = 1;
            last = Some(b.0);
            stuff_cnt += 1;
            continue;
        }
        if Some(b.0) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(b.0);
        }
        bits.push(b);
        if idx == brs_at && b.0 {
            fast = true;
        }
        let n = bits.len();
        if n == 14 && fdf_at == usize::MAX {
            ext = bits[13].0;
            fdf_at = if ext { 33 } else { 14 };
            need = fdf_at + 1;
        } else if dlc_at == usize::MAX && fdf_at != usize::MAX && n == fdf_at + 1 {
            fd = bits[fdf_at].0;
            dlc_at = if fd { fdf_at + 4 } else if ext { fdf_at + 2 } else { fdf_at + 1 };
            if fd {
                brs_at = fdf_at + 2;
            }
            need = dlc_at + 4;
        } else if data_end == usize::MAX && dlc_at != usize::MAX && n == dlc_at + 4 {
            let dlc = bits[dlc_at..dlc_at + 4].iter().fold(0usize, |a, b| a << 1 | b.0 as usize);
            let rtr = !fd && bits[if ext { 32 } else { 12 }].0;
            let nb = if fd { FD_LEN[dlc] } else if rtr { 0 } else { dlc.min(8) };
            data_end = n + 8 * nb;
            need = if fd { data_end } else { data_end + 15 };
        }
    }
    let val = |r: std::ops::Range<usize>| bits[r].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
    let span = |kind, a: usize, b: usize, value: u32, value2, ok| pk(kind, bits[a].1, bits[b].2, value, value2, ok);
    let rtr = !fd && bits[if ext { 32 } else { 12 }].0;
    let id = if ext { val(1..12) << 18 | val(14..32) } else { val(1..12) };
    let dlc = val(dlc_at..dlc_at + 4);
    let nb = (data_end - dlc_at - 4) / 8;
    let mut out = vec![span(Kind::Start, 0, 0, 0, None, true), span(Kind::Id { ext, rtr }, 1, if ext { 31 } else { 11 }, id, None, true)];
    if fd {
        out.push(span(Kind::Field("BRS"), brs_at, brs_at, bits[brs_at].0 as u32, None, true));
        out.push(span(Kind::Field("ESI"), brs_at + 1, brs_at + 1, bits[brs_at + 1].0 as u32, None, true));
    }
    out.push(span(Kind::Field("DLC"), dlc_at, dlc_at + 3, dlc, Some(nb as u32), true));
    for i in 0..nb {
        let a = dlc_at + 4 + 8 * i;
        out.push(span(Kind::Data, a, a + 7, val(a..a + 8), None, true));
    }
    let dd = |_: bool| if fast { (td, 0.4 * td) } else { (tn, 0.4 * tn) };
    let ds = if fast { 0.65 * td } else { 0.65 * tn };
    if fd {
        let crc_len = if nb > 16 { 21 } else { 17 };
        let mut prev = raw.last().copied().unwrap_or(false);
        let mut fixed_ok = true;
        let mut seq: Vec<(bool, f64, f64)> = Vec::new();
        for i in 0..4 + crc_len {
            if i % 4 == 0 {
                let f = clk.bit(ds, dd)?;
                fixed_ok &= f.0 != prev;
            }
            let b = clk.bit(ds, dd)?;
            prev = b.0;
            seq.push(b);
        }
        let sv = |r: std::ops::Range<usize>| seq[r].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
        let gray = sv(0..3);
        let cnt = (gray ^ gray >> 1 ^ gray >> 2) & 7;
        let par_ok = (gray.count_ones() + seq[3].0 as u32) % 2 == 0;
        out.push(pk(Kind::Field("SBC"), seq[0].1, seq[3].2, cnt, None, par_ok && cnt == stuff_cnt % 8));
        let rx = sv(4..4 + crc_len);
        let mut input = raw.clone();
        input.extend(seq[..4].iter().map(|b| b.0));
        let calc = can_fd_crc(&input, crc_len as u32);
        out.push(pk(Kind::Crc, seq[4].1, seq[3 + crc_len].2, rx, None, rx == calc && fixed_ok));
    } else {
        // a stuff bit after the last CRC bit
        if run == 5 {
            clk.bit(0.65 * tn, nominal)?;
        }
        let crc_at = data_end;
        let calc = can_crc15(&bits[..crc_at].iter().map(|b| b.0).collect::<Vec<_>>());
        let rx = val(crc_at..crc_at + 15);
        out.push(span(Kind::Crc, crc_at, crc_at + 14, rx, None, rx == calc as u32));
    }
    // CRC delimiter (back to the nominal rate at its sample point), ACK slot, ACK delimiter
    if fast {
        clk.bit(0.65 * td, |_| (FD_SP * td + (1.0 - FD_SP) * tn, 0.4 * td))?;
    } else {
        clk.bit(0.65 * tn, nominal)?;
    }
    let ack = clk.bit(0.65 * tn, nominal)?;
    let del = clk.bit(0.65 * tn, nominal)?;
    out.push(pk(Kind::Ack, ack.1, ack.2, 0, None, !ack.0));
    Some((out, del.2))
}

// ---------------------------------------------------------------------------------------------------------------
// FlexRay

/// FlexRay header CRC-11 (FlexRay Protocol Specification V3.0.1, 4.5.2): polynomial 0x385, init 0x1A, over the
/// 20 bits sync frame indicator, startup frame indicator, frame ID, payload length (`bits20`, MSB first).
pub fn flexray_header_crc(bits20: u32) -> u32 {
    crc_msb((0..20).rev().map(|i| (bits20 >> i) & 1 == 1), 11, 0x385, 0x1a)
}

/// FlexRay frame CRC-24 (V3.0.1, 4.5.3): polynomial 0x5D6DCB, init 0xFEDCBA (channel A) / 0xABCDEF (channel B),
/// over header and payload bytes, MSB first.
pub fn flexray_frame_crc(bytes: &[u8], channel_b: bool) -> u32 {
    let init = if channel_b { 0xab_cdef } else { 0xfe_dcba };
    crc_msb(bytes.iter().flat_map(|&b| (0..8).rev().map(move |i| (b >> i) & 1 == 1)), 24, 0x5d_6dcb, init)
}

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
    if !(spb >= 4.0) {
        return out;
    }
    let st = states(v, thr, hyst);
    let high = |x: f64| st.get(x as usize).copied().flatten().map_or(true, |b| b != invert);
    let e = edges(v, thr, hyst);
    let fall: Vec<f64> = e.iter().filter(|x| x.rising == invert).map(|x| x.t).collect();
    let rise: Vec<f64> = e.iter().filter(|x| x.rising != invert).map(|x| x.t).collect();
    // TSS candidates: falling edges after the channel idle (at least 10 bits high, longer than any high run inside a
    // frame, so the tail of a frame cut by the record start is not taken for one), and a low first decision after as
    // long an undefined start (a differential bus idles at 0 V)
    let idle = 10.0 * spb;
    let mut starts: Vec<f64> = fall.iter().copied().filter(|&f| f - rise.partition_point(|&x| x < f).checked_sub(1).map_or(0.0, |k| rise[k]) >= idle).collect();
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
        let fld = |kind, a: usize, n: usize, value2, ok| pk(kind, bits[a].1, bits[a + n - 1].1 + spb, fv(a, n), value2, ok);
        out.push(pk(Kind::Field("TSS"), f, r, tss.round() as u32, None, (2.5..=15.5).contains(&tss)));
        out.push(fld(Kind::Field("IND"), 0, 5, None, !bits[0].0));
        let id = fv(5, 11);
        out.push(fld(Kind::Id { ext: false, rtr: false }, 5, 11, None, id != 0));
        let words = fv(16, 7);
        out.push(fld(Kind::Field("LEN"), 16, 7, Some(2 * words), true));
        out.push(fld(Kind::Field("HCRC"), 23, 11, None, fv(23, 11) == flexray_header_crc(fv(3, 20))));
        out.push(fld(Kind::Field("CYC"), 34, 6, None, true));
        let nfull = bits.len() / 8;
        for k in 5..nfull.min(nbytes - 3) {
            out.push(fld(Kind::Data, 8 * k, 8, None, true));
        }
        // a frame cut by the record end is not an error
        let end = v.len() as f64;
        if lost {
            if exp + 0.75 * spb < end {
                out.push(tag("!ERR", exp - 0.75 * spb, exp + 0.75 * spb, false));
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
        out.push(fld(Kind::Crc, 8 * (nbytes - 3), 24, ch, ch.is_some()));
        // FES: low bit from the end of the last byte, then high
        if exp + spb < end && (high(exp - 0.5 * spb) || !high(exp + 0.5 * spb)) {
            out.push(tag("!ERR", exp - spb, exp + spb, false));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------

/// Signal generators for the decoders above (tests, demo source).
pub mod gen {
    use super::*;
    use crate::sim::Logic;

    /// Builder of a logic waveform from levels and durations; idle high at t = 0.
    pub struct Wave {
        pub l: Logic,
        /// current time (s)
        pub t: f64,
    }

    impl Wave {
        pub fn new(lo: f64, hi: f64, tr: f64) -> Wave {
            let mut l = Logic::new(lo, hi, tr);
            l.push(0.0, true);
            Wave { l, t: 0.0 }
        }
        /// `level` for `dur` seconds.
        pub fn level(&mut self, level: bool, dur: f64) {
            self.l.push(self.t, level);
            self.t += dur;
        }
        /// A UART character: start bit, `bits` data bits LSB first, parity, `stop` stop bits.
        pub fn uart(&mut self, v: u32, bits: u32, parity: Parity, stop: u32, baud: f64) {
            let tb = 1.0 / baud;
            self.level(false, tb);
            for i in 0..bits {
                self.level((v >> i) & 1 == 1, tb);
            }
            let ones = (v & ((1u64 << bits) - 1) as u32).count_ones();
            match parity {
                Parity::None => {}
                Parity::Even => self.level(ones % 2 == 1, tb),
                Parity::Odd => self.level(ones % 2 == 0, tb),
            }
            self.level(true, stop as f64 * tb);
        }
        /// Ends with the idle (high) level.
        pub fn done(mut self) -> Logic {
            self.l.push(self.t, true);
            self.l
        }
    }

    /// DMX512 packet: break 176 us, MAB 12 us, start code and slots (8N2), 2 us between slots.
    pub fn dmx(w: &mut Wave, start_code: u8, slots: &[u8]) {
        w.level(false, 176e-6);
        w.level(true, 12e-6);
        for &s in std::iter::once(&start_code).chain(slots) {
            w.uart(s as u32, 8, Parity::None, 2, DMX_BAUD);
            w.level(true, 2e-6);
        }
        w.level(true, 50e-6);
    }

    /// RDM request/response bytes (ANSI E1.20): start code .. parameter data, then the 16-bit checksum.
    pub fn rdm_packet(dst: u64, src: u64, tn: u8, port: u8, mc: u8, subdev: u16, cc: u8, pid: u16, pd: &[u8]) -> Vec<u8> {
        let mut b = vec![RDM_SC, 0x01, (24 + pd.len()) as u8];
        b.extend_from_slice(&dst.to_be_bytes()[2..]);
        b.extend_from_slice(&src.to_be_bytes()[2..]);
        b.extend([tn, port, mc]);
        b.extend(subdev.to_be_bytes());
        b.push(cc);
        b.extend(pid.to_be_bytes());
        b.push(pd.len() as u8);
        b.extend_from_slice(pd);
        let sum = b.iter().map(|&x| x as u32).sum::<u32>() as u16;
        b.extend(sum.to_be_bytes());
        b
    }

    /// Modbus RTU frame: address, PDU, CRC (low byte first).
    pub fn modbus_rtu_frame(addr: u8, pdu: &[u8]) -> Vec<u8> {
        let mut b = vec![addr];
        b.extend_from_slice(pdu);
        b.extend(modbus_crc(&b).to_le_bytes());
        b
    }

    /// Modbus ASCII frame characters: ':', hex of address, PDU and LRC, CR LF.
    pub fn modbus_ascii_frame(addr: u8, pdu: &[u8]) -> Vec<u8> {
        let mut b = vec![addr];
        b.extend_from_slice(pdu);
        b.push(modbus_lrc(&b));
        let mut s = vec![b':'];
        for x in b {
            s.extend(format!("{:02X}", x).bytes());
        }
        s.extend(b"\r\n");
        s
    }

    /// 1-Wire master/slave activity for `onewire_logic`.
    #[derive(Clone, Copy, Debug)]
    pub enum Ow {
        Reset { presence: bool },
        /// master writes a byte (LSB first)
        Write(u8),
        /// a slave answers a read byte
        Read(u8),
        WriteBit(bool),
        ReadBit(bool),
        /// switch the timing to overdrive (true) / standard speed
        Speed(bool),
    }

    /// 1-Wire waveform with the recommended AN126 timings (standard: write 1 = 6 us low, write 0 = 60 us, slot 70 us,
    /// reset 480 us, presence 30 us after it for 120 us; overdrive: 1, 7.5, 10, 70, 3 / 12 us).
    pub fn onewire_logic(ops: &[Ow], lo: f64, hi: f64) -> Logic {
        let us = 1e-6;
        let mut w = Wave::new(lo, hi, 0.05 * us);
        w.level(true, 20.0 * us);
        let mut od = false;
        for &op in ops {
            // (write-1 low, read-0 low, write-0 low, slot, reset, presence wait, presence low, after reset)
            let (a, rd0, c, slot, h, pw, pl, j) =
                if od { (1.0, 5.0, 7.5, 10.0, 70.0, 3.0, 12.0, 40.0) } else { (6.0, 30.0, 60.0, 70.0, 480.0, 30.0, 120.0, 410.0) };
            let bit = |w: &mut Wave, low: f64| {
                w.level(false, low * us);
                w.level(true, (slot - low) * us);
            };
            match op {
                Ow::Reset { presence } => {
                    w.level(false, h * us);
                    if presence {
                        w.level(true, pw * us);
                        w.level(false, pl * us);
                        w.level(true, (j - pw - pl) * us);
                    } else {
                        w.level(true, j * us);
                    }
                }
                Ow::Write(x) => (0..8).for_each(|i| bit(&mut w, if (x >> i) & 1 == 1 { a } else { c })),
                Ow::Read(x) => (0..8).for_each(|i| bit(&mut w, if (x >> i) & 1 == 1 { a } else { rd0 })),
                Ow::WriteBit(b) => bit(&mut w, if b { a } else { c }),
                Ow::ReadBit(b) => bit(&mut w, if b { a } else { rd0 }),
                Ow::Speed(o) => od = o,
            }
        }
        w.level(true, 20.0 * us);
        w.done()
    }

    /// One CAN / CAN FD frame for `can_fd_bits`.
    #[derive(Clone, Debug)]
    pub struct FdFrame {
        pub id: u32,
        pub ext: bool,
        /// FD frame (FDF recessive); false = classic data frame
        pub fd: bool,
        pub brs: bool,
        pub esi: bool,
        /// payload; FD frames are padded with 0xCC to the next valid length
        pub data: Vec<u8>,
    }

    /// The bus bits of a frame (true = recessive) with their durations in seconds: SOF .. EOF and 3 bits
    /// intermission, data phase at `data_rate` when BRS is set (switch at `FD_SP` of the BRS bit / CRC delimiter).
    pub fn can_fd_bits(f: &FdFrame, nominal: f64, data_rate: f64) -> Vec<(bool, f64)> {
        let (tn, td) = (1.0 / nominal, 1.0 / data_rate);
        let push = |h: &mut Vec<bool>, v: u32, n: u32| (0..n).rev().for_each(|i| h.push((v >> i) & 1 == 1));
        let dlc = if f.fd { FD_LEN.iter().position(|&l| l >= f.data.len()).unwrap_or(15) as u32 } else { f.data.len().min(8) as u32 };
        let nb = if f.fd { FD_LEN[dlc as usize] } else { dlc as usize };
        let mut h = vec![false];
        if f.ext {
            push(&mut h, f.id >> 18 & 0x7ff, 11);
            h.push(true); // SRR
            h.push(true); // IDE
            push(&mut h, f.id & 0x3ffff, 18);
            h.push(false); // RTR / RRS
        } else {
            push(&mut h, f.id & 0x7ff, 11);
            h.push(false); // RTR / RRS
            h.push(false); // IDE
        }
        let mut brs_at = usize::MAX;
        if f.fd {
            h.push(true); // FDF
            h.push(false); // res
            brs_at = h.len();
            h.push(f.brs);
            h.push(f.esi);
        } else {
            if f.ext {
                h.push(false); // r1
            }
            h.push(false); // r0
        }
        push(&mut h, dlc, 4);
        for i in 0..nb {
            push(&mut h, *f.data.get(i).unwrap_or(&0xcc) as u32, 8);
        }
        if !f.fd {
            let c = can_crc15(&h) as u32;
            push(&mut h, c, 15);
        }
        let fast = f.fd && f.brs;
        let brs_len = FD_SP * tn + (1.0 - FD_SP) * td;
        let del_len = FD_SP * td + (1.0 - FD_SP) * tn;
        let mut out: Vec<(bool, f64)> = Vec::new();
        let mut raw = Vec::new();
        let (mut run, mut last, mut cnt) = (0, None, 0u32);
        for (k, &x) in h.iter().enumerate() {
            let len = if fast && k == brs_at { brs_len } else if fast && k > brs_at { td } else { tn };
            out.push((x, len));
            raw.push(x);
            if Some(x) == last {
                run += 1;
            } else {
                run = 1;
                last = Some(x);
            }
            // a stuff bit after 5 equal bits; at the end of an FD data field the fixed stuff bit replaces it
            if run == 5 && !(f.fd && k + 1 == h.len()) {
                out.push((!x, if fast && k >= brs_at { td } else { tn }));
                raw.push(!x);
                last = Some(!x);
                run = 1;
                cnt += 1;
            }
        }
        let dd = if fast { td } else { tn };
        if f.fd {
            let g = (cnt % 8) ^ (cnt % 8) >> 1;
            let mut seq = Vec::new();
            push(&mut seq, g, 3);
            seq.push(g.count_ones() % 2 == 1);
            let crc_len = if nb > 16 { 21 } else { 17 };
            let mut input = raw.clone();
            input.extend(&seq);
            push(&mut seq, can_fd_crc(&input, crc_len), crc_len);
            let mut prev = *raw.last().unwrap();
            for (i, &b) in seq.iter().enumerate() {
                if i % 4 == 0 {
                    out.push((!prev, dd));
                }
                out.push((b, dd));
                prev = b;
            }
        }
        out.push((true, if fast { del_len } else { tn })); // CRC delimiter
        out.push((false, tn)); // ACK slot
        out.push((true, tn)); // ACK delimiter
        out.push((true, 10.0 * tn)); // EOF + intermission
        out
    }

    /// CAN / CAN FD frames as a logic waveform (dominant = `lo`), 12 idle bits before the first.
    pub fn can_fd_logic(frames: &[FdFrame], nominal: f64, data_rate: f64, lo: f64, hi: f64) -> Logic {
        let mut w = Wave::new(lo, hi, 0.02 / data_rate);
        w.level(true, 12.0 / nominal);
        for f in frames {
            for (b, d) in can_fd_bits(f, nominal, data_rate) {
                w.level(b, d);
            }
        }
        w.done()
    }

    /// One FlexRay frame for `flexray_bits`.
    #[derive(Clone, Debug)]
    pub struct FrFrame {
        pub ppi: bool,
        /// null frame indicator (false = null frame)
        pub nf: bool,
        pub sync: bool,
        pub startup: bool,
        pub id: u16,
        pub cycle: u8,
        /// payload, padded with 0 to an even length
        pub payload: Vec<u8>,
        pub channel_b: bool,
    }

    /// Header, payload and CRC bytes of a FlexRay frame.
    pub fn flexray_bytes(f: &FrFrame) -> Vec<u8> {
        let mut pl = f.payload.clone();
        pl.truncate(254);
        if pl.len() % 2 == 1 {
            pl.push(0);
        }
        let words = (pl.len() / 2) as u64;
        let id = (f.id & 0x7ff) as u64;
        let hcrc = flexray_header_crc(((f.sync as u32) << 19) | ((f.startup as u32) << 18) | (id as u32) << 7 | words as u32) as u64;
        let h: u64 = (f.ppi as u64) << 38
            | (f.nf as u64) << 37
            | (f.sync as u64) << 36
            | (f.startup as u64) << 35
            | id << 24
            | words << 17
            | hcrc << 6
            | (f.cycle & 0x3f) as u64;
        let mut b: Vec<u8> = h.to_be_bytes()[3..].to_vec();
        b.extend(pl);
        let crc = flexray_frame_crc(&b, f.channel_b);
        b.extend(&crc.to_be_bytes()[1..]);
        b
    }

    /// The bits of a FlexRay frame on the bus (true = high / Data_1): TSS of `tss` bits, FSS, BSS + byte for every
    /// byte, FES.
    pub fn flexray_bits(f: &FrFrame, tss: usize) -> Vec<bool> {
        let mut s = vec![false; tss];
        s.push(true);
        for b in flexray_bytes(f) {
            s.push(true);
            s.push(false);
            (0..8).rev().for_each(|i| s.push((b >> i) & 1 == 1));
        }
        s.push(false);
        s.push(true);
        s
    }
}
