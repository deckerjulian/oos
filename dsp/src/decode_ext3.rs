// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Extended decoders, part 3: ARINC 429, SENT (SAE J2716) and USB 1.x / 2.0 low / full speed.
//!
//! Same conventions as `decode`: analog inputs through a comparator (threshold + hysteresis), packet times are
//! fractional sample indices, `ok` = the check of the field passed, protocol-specific fields are `Kind::Field(label)`.
//! Signal generators for the tests and the demo source are in `gen`.

use crate::decode::{Kind, Packet};
use crate::edges::{edges, states};

fn pk(kind: Kind, t0: f64, t1: f64, value: u32, value2: Option<u32>, ok: bool) -> Packet {
    Packet { t0, t1, kind, value, value2, ok }
}

/// A value-less event tag (label starting with '!').
fn tag(label: &'static str, t0: f64, t1: f64, ok: bool) -> Packet {
    pk(Kind::Field(label), t0, t1, 0, None, ok)
}

// ---------------------------------------------------------------------------------------------------------------
// ARINC 429

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
    if !(thr > 0.0) || !(dt > 0.0) {
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
            median(pulses.windows(2).map(|q| mid(&q[1]) - mid(&q[0])).filter(|&d| d < 3.0 * w).collect()).unwrap_or(2.0 * w)
        }
    };
    if !(bit >= 4.0) || !bit.is_finite() {
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
    let slots = p.len() == 32 && (b / bit - 1.0).abs() < 0.2 && p.iter().enumerate().all(|(k, q)| ((mid(q) - m0) / b - k as f64).abs() < 0.3);
    if !slots {
        if inside {
            out.push(tag("!ERR", first.0, last.1, false));
        }
        return;
    }
    let bits: Vec<bool> = p.iter().map(|q| q.2).collect();
    let t0 = m0 - 0.25 * b; // start of bit 1
    let at = |k: usize| t0 + k as f64 * b;
    let lsb = |a: usize, len: usize| (0..len).fold(0u32, |acc, k| acc | (bits[a + k] as u32) << k);
    let label = bits[..8].iter().fold(0u32, |acc, &x| acc << 1 | x as u32);
    let odd = bits.iter().filter(|&&x| x).count() % 2 == 1;
    out.push(pk(Kind::Id { ext: false, rtr: false }, at(0), at(8), label, None, true));
    out.push(pk(Kind::Field("SDI"), at(8), at(10), lsb(8, 2), None, true));
    out.push(pk(Kind::Data, at(10), at(29), lsb(10, 19), None, true));
    out.push(pk(Kind::Field("SSM"), at(29), at(31), lsb(29, 2), None, true));
    out.push(pk(Kind::Field("PAR"), at(31), at(32), bits[31] as u32, None, odd));
}

// ---------------------------------------------------------------------------------------------------------------
// SENT

/// SENT CRC-4 (SAE J2716): polynomial x^4 + x^3 + x^2 + 1, seed 0101, over the data nibbles (not the status
/// nibble). The recommended method (J2716 APR2010) appends a zero nibble; `legacy` = without it (earlier issues).
pub fn sent_crc(data: &[u8], legacy: bool) -> u8 {
    // c * x^4 + nibble, mod the polynomial
    let step = |c: u8, nib: u8| {
        let mut r = c;
        for _ in 0..4 {
            r = ((r << 1) ^ if r & 8 != 0 { 0x1d } else { 0 }) & 0xf;
        }
        r ^ (nib & 0xf)
    };
    let c = data.iter().fold(5u8, |c, &d| step(c, d));
    if legacy {
        c
    } else {
        step(c, 0)
    }
}

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
    if !(dt > 0.0) {
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
        let tick_ok = nom.map_or(true, |t| (tk / t - 1.0).abs() <= 0.2);
        out.push(pk(Kind::Field("SYNC"), f[i], f[i + 1], (tk * dt * 1e9).round() as u32, None, tick_ok));
        if let Some(k) = bad {
            out.push(tag("!ERR", f[i + 1 + k], f[i + 2 + k], false));
            i += 1 + k;
            continue;
        }
        let e = |k: usize| f[i + 1 + k]; // start of nibble k (0 = status)
        out.push(pk(Kind::Field("STAT"), e(0), e(1), vals[0], None, true));
        let data: Vec<u8> = vals[1..=nn].iter().map(|&x| x as u8).collect();
        let value = data.iter().fold(0u32, |a, &x| a << 4 | x as u32);
        out.push(pk(Kind::Data, e(1), e(nn + 1), value, Some(nn as u32), true));
        out.push(pk(Kind::Crc, e(nn + 1), e(nn + 2), vals[nn + 1], None, vals[nn + 1] == sent_crc(&data, false) as u32));
        i += nn + 3; // the edge that ends the CRC nibble
        // a pause pulse: followed by a sync of this tick
        if let (Some(&a), Some(&b)) = (f.get(i + 1), f.get(i + 2)) {
            let p = (a - f[i]) / tk;
            if (11.5..=768.5).contains(&p) && ((b - a) / (56.0 * tk) - 1.0).abs() < 0.1 {
                out.push(pk(Kind::Field("PAUSE"), f[i], a, p.round() as u32, None, true));
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// USB 1.x / 2.0 low speed and full speed

/// USB bit rates: low speed and full speed.
pub const USB_LOW: f64 = 1.5e6;
pub const USB_FULL: f64 = 12e6;

/// Bus speed setting of the USB decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbSpeed {
    /// polarity from the idle state (D+ high: full speed, D- high: low speed), bit rate from every SYNC
    Auto,
    /// J = D- high, 1.5 Mbit/s
    Low,
    /// J = D+ high, 12 Mbit/s; low-speed packets (after PRE, and the low-speed device's answers) at 1.5 Mbit/s
    Full,
}

/// The PID byte as sent (check nibble = the complement in the high half) of a 4-bit PID type.
pub fn usb_pid(code: u8) -> u8 {
    code & 15 | (!code & 15) << 4
}

/// Name of a PID byte; "?" for a wrong check nibble or the reserved PID. 0x3C is PRE on a low / full-speed bus
/// (ERR only in high-speed split transactions).
pub fn usb_pid_name(pid: u8) -> &'static str {
    match pid {
        0xE1 => "OUT",
        0x69 => "IN",
        0xA5 => "SOF",
        0x2D => "SETUP",
        0xB4 => "PING",
        0xC3 => "DATA0",
        0x4B => "DATA1",
        0x87 => "DATA2",
        0x0F => "MDATA",
        0xD2 => "ACK",
        0x5A => "NAK",
        0x1E => "STALL",
        0x96 => "NYET",
        0x3C => "PRE",
        0x78 => "SPLIT",
        _ => "?",
    }
}

/// USB CRC5 of tokens and SOF: x^5 + x^2 + 1 over the bits in bus order (every field LSB first), preset to ones,
/// the remainder inverted. The result is the CRC field as read from the bus LSB first (bit 0 = the first CRC bit
/// sent, the x^4 coefficient), comparable to the received field.
pub fn usb_crc5(bits: &[bool]) -> u8 {
    let r = bits.iter().fold(0x1fu8, |r, &b| if (r & 1 != 0) != b { r >> 1 ^ 0x14 } else { r >> 1 });
    r ^ 0x1f
}

/// USB CRC16 of data packets: x^16 + x^15 + x^2 + 1 over the payload bytes (LSB first), preset to ones, inverted;
/// sent low byte first.
pub fn usb_crc16(data: &[u8]) -> u16 {
    let r = data.iter().fold(0xffffu16, |mut r, &d| {
        r ^= d as u16;
        for _ in 0..8 {
            r = if r & 1 != 0 { r >> 1 ^ 0xa001 } else { r >> 1 };
        }
        r
    });
    r ^ 0xffff
}

/// Line state of the USB pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ls {
    J,
    K,
    Se0,
    Se1,
}

/// The line as state segments (start in fractional samples, state) and what the decoder knows about the bus.
struct UsbBus {
    segs: Vec<(f64, Ls)>,
    n: f64,
    /// D+ and D- given; with D+ alone there is no SE0 (it looks like D+ low)
    two: bool,
    /// J = D+ high (full-speed signalling)
    fs_pol: bool,
    /// bit times (samples) the SYNC may have, with the rate in kbit/s
    rates: Vec<(f64, u32)>,
}

impl UsbBus {
    fn end(&self, k: usize) -> f64 {
        self.segs.get(k + 1).map_or(self.n, |s| s.0)
    }
    fn dur(&self, k: usize) -> f64 {
        self.end(k) - self.segs[k].0
    }
    /// The segment at time `t`.
    fn at(&self, t: f64) -> usize {
        self.segs.partition_point(|s| s.0 <= t).saturating_sub(1)
    }
    /// A SYNC from segment `k`: K J K J K J one bit each, then K; its bit time and rate.
    fn sync(&self, k: usize) -> Option<(f64, u32)> {
        let s = self.segs.get(k..k + 7)?;
        if s.iter().enumerate().any(|(q, x)| x.1 != if q % 2 == 0 { Ls::K } else { Ls::J }) {
            return None;
        }
        let bit = (s[6].0 - s[0].0) / 6.0;
        if s.windows(2).any(|w| !((0.6..1.4).contains(&((w[1].0 - w[0].0) / bit)))) || !(bit >= 3.0) {
            return None;
        }
        self.rates.iter().find(|r| (bit / r.0 - 1.0).abs() <= 0.25).map(|r| (bit, r.1))
    }
    /// D+ alone: whether the line after `te` (the end of a candidate packet) looks like the EOP (D+ low for two bit
    /// times) and then idle or the next SYNC; the segment to go on from.
    fn eop_after(&self, te: f64, bit: f64) -> Option<usize> {
        let q = self.at(te + 0.5 * bit);
        let idle_or_sync = |k: usize| self.end(k) >= self.n || self.dur(k) > 7.0 * bit || self.sync(k + 1).is_some();
        if self.fs_pol {
            // the SE0 looks like K, then J
            let ok = self.segs[q].1 == Ls::K && (self.end(q) - te - 2.0 * bit).abs() <= 0.6 * bit && q + 1 < self.segs.len() && (self.dur(q + 1) >= 1.5 * bit || self.end(q + 1) >= self.n) && idle_or_sync(q + 1);
            ok.then_some(q + 1)
        } else {
            // the SE0 looks like J: at least two bit times of it and two of idle
            let ok = self.segs[q].1 == Ls::J && (self.end(q) - te >= 3.5 * bit || self.end(q) >= self.n) && idle_or_sync(q);
            ok.then_some(q + 1)
        }
    }
}

fn usb_bus(dp: &[f32], dm: Option<&[f32]>, dt: f64, speed: UsbSpeed, thr: f32, hyst: f32) -> UsbBus {
    let len = dm.map_or(dp.len(), |m| m.len().min(dp.len()));
    let n = len as f64;
    let (fs, ls) = (1.0 / USB_FULL / dt, 1.0 / USB_LOW / dt);
    // line code: bit 0 = D+, bit 1 = D-; with D+ alone D- is taken as its inverse
    let lines: Vec<&[f32]> = std::iter::once(&dp[..len]).chain(dm.map(|m| &m[..len])).collect();
    let single = |c: u8| if dm.is_some() { c } else if c & 1 != 0 { 1 } else { 2 };
    let mut code = 0u8;
    let mut ev = Vec::new();
    for (k, v) in lines.iter().enumerate() {
        let e = edges(v, thr, hyst);
        let first = match e.first() {
            Some(x) => !x.rising,
            None => states(v, thr, hyst).into_iter().flatten().last().unwrap_or(false),
        };
        code |= (first as u8) << k;
        ev.extend(e.into_iter().map(|x| (x.t, k, x.rising)));
    }
    ev.sort_by(|a, b| a.0.total_cmp(&b.0));
    code = single(code);
    let mut raw = vec![(0.0, code)];
    for (t, k, rising) in ev {
        code = single(if rising { code | 1 << k } else { code & !(1 << k) });
        if raw.last().is_some_and(|x| x.1 != code) {
            raw.push((t, code));
        }
    }
    let rdur = |k: usize| raw.get(k + 1).map_or(n, |s| s.0) - raw[k].0;
    // J: the idle state = the longest D+ / D- high segment
    let jc = match speed {
        UsbSpeed::Low => 2,
        UsbSpeed::Full => 1,
        UsbSpeed::Auto => (0..raw.len()).filter(|&k| raw[k].1 == 1 || raw[k].1 == 2).max_by(|&a, &b| rdur(a).total_cmp(&rdur(b))).map_or(1, |k| raw[k].1),
    };
    let fs_pol = jc == 1;
    // SE0 / SE1 shorter than 0.3 bit: the crossover of D+ and D- in a J <-> K transition; the transition is in its
    // middle
    let g = 0.3 * if fs_pol { fs } else { ls };
    let mut segs: Vec<(f64, Ls)> = Vec::new();
    let mut mid = None;
    for k in 0..raw.len() {
        let (t, c) = raw[k];
        if (c == 0 || c == 3) && k > 0 && k + 1 < raw.len() && rdur(k) < g {
            mid = Some(t + 0.5 * rdur(k));
            continue;
        }
        let st = match c {
            0 => Ls::Se0,
            3 => Ls::Se1,
            c if c == jc => Ls::J,
            _ => Ls::K,
        };
        let t = mid.take().unwrap_or(t);
        if segs.last().map_or(true, |s| s.1 != st) {
            segs.push((t, st));
        }
    }
    let rates = if fs_pol { vec![(fs, 12000), (ls, 1500)] } else { vec![(ls, 1500)] };
    UsbBus { segs, n, two: dm.is_some(), fs_pol, rates }
}

/// USB 1.x / 2.0 low speed (1.5 Mbit/s) and full speed (12 Mbit/s) on D+ and D- (`dm`; None = D+ alone). `speed`
/// fixes the polarity (Low: J = D- high, Full: J = D+ high) or takes it from the idle state (Auto); the bit time
/// is measured on every SYNC (KJKJKJKK), and a full-speed bus also carries low-speed packets (PRE). Decodes NRZI
/// with bit unstuffing (a zero after six ones; seven ones are a stuff error), the clock re-synchronised on every
/// transition. Crossover SE0 / SE1 shorter than 0.3 bit are ignored.
///
/// Per packet: Field("SYNC") (value = the rate in kbit/s, 12000 or 1500; `ok` = the pattern), Field("PID")
/// (value = the PID byte as sent, e.g. 0xC3 = DATA0; `ok` = check nibble). Then by PID: tokens (OUT, IN, SETUP,
/// PING) Field("ADDR") (7 bits), Field("ENDP") (4 bits), Crc (CRC5); SOF Field("FRAME") (11 bits), Crc (CRC5);
/// data packets (DATA0/1/2, MDATA) one Data per payload byte, Crc (CRC16, value = as received, low byte first);
/// handshakes (ACK, NAK, STALL, NYET) and PRE nothing more (PRE ends without EOP, the low-speed packet follows);
/// other PIDs and PIDs with a wrong check nibble: the bytes as Data. A packet of the wrong length for its PID or
/// with bits left over before the EOP (one dribble bit is ignored) gives Field("!ERR"), seven ones Field("!STUFF")
/// (the packet is abandoned after its PID), Field("!EOP") marks the end of packet (`ok` = SE0 of 0.6..3.5 bit times,
/// then J). Outside packets: Field("!RESET") (SE0 of 2.5 us or more) and Field("!EOP") (an EOP alone, the low-speed
/// keep-alive).
///
/// With D+ alone the SE0 cannot be seen: a packet ends after the length of its PID or at the first CRC16 that
/// matches when the line then looks like an EOP followed by idle or a SYNC; else at the next idle (more than 7 bit
/// times without a transition). Packets cut by the record start or end are skipped; a SYNC that does not follow
/// idle or an EOP counts only for a packet without errors.
pub fn usb(dp: &[f32], dm: Option<&[f32]>, dt: f64, speed: UsbSpeed, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    if !(dt > 0.0) || !dt.is_finite() {
        return out;
    }
    let bus = usb_bus(dp, dm, dt, speed, thr, hyst);
    let segs = &bus.segs;
    let reset = 2.5e-6 / dt;
    // a J this long is idle
    let idle = 7.0 * if bus.fs_pol { 1.0 / USB_FULL } else { 1.0 / USB_LOW } / dt;
    let mut resume = None; // where the last packet ended
    let mut i = 0;
    while i < segs.len() {
        let (t, s) = segs[i];
        let e = bus.end(i);
        match s {
            Ls::Se0 => {
                if e - t >= reset {
                    out.push(tag("!RESET", t, e, true));
                } else if i > 0 && i + 1 < segs.len() {
                    out.push(tag("!EOP", t, e, segs[i + 1].1 == Ls::J));
                }
                i += 1;
            }
            Ls::K if i > 0 && segs[i - 1].1 == Ls::J => match bus.sync(i) {
                Some((bit, kbit)) => {
                    // a SYNC from idle: after an EOP, a long J or where the last packet ended (PRE, D+ alone); else
                    // (after a packet cut by the record start) only a packet without errors
                    let from_idle = resume.is_some_and(|r| i <= r + 1) || (i >= 2 && segs[i - 2].1 == Ls::Se0) || bus.dur(i - 1) > idle;
                    let mut p = Vec::new();
                    let r = usb_packet(&bus, i, bit, kbit, reset, &mut p).max(i + 1);
                    if from_idle || (p.iter().any(|x| x.kind == Kind::Field("PID")) && p.iter().all(|x| x.ok)) {
                        out.extend(p);
                        resume = Some(r);
                        i = r;
                    } else {
                        i += 1;
                    }
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    out
}

/// One packet from its SYNC at segment `i` with the bit time `bit`: NRZI by run lengths, unstuffed, up to the EOP
/// (D+ alone: see `usb`), PRE or a stuff error. Returns the segment to go on from.
fn usb_packet(bus: &UsbBus, i: usize, bit: f64, kbit: u32, reset: f64, out: &mut Vec<Packet>) -> usize {
    let segs = &bus.segs;
    // the bits (SYNC first) and their start times
    let (mut b, mut tm) = (Vec::new(), Vec::new());
    let mut ones = 0;
    let mut j = i;
    // bits before the run of segment j, and whether its transition is a data bit (not a stuff bit)
    let (mut edge_bits, mut edge_data) = (0, false);
    let mut stuff = None;
    while j < segs.len() && matches!(segs[j].1, Ls::J | Ls::K) {
        let (t, e) = (segs[j].0, bus.end(j));
        let nb = ((e - t) / bit).round().max(1.0);
        // more than 7 bit times without a transition: idle or a stuff error, met within 8 bits
        let long = nb > 7.0;
        if e >= bus.n && (bus.two || !long) {
            return segs.len();
        }
        let step = if long { bit } else { (e - t) / nb };
        (edge_bits, edge_data) = (b.len(), false);
        for k in 0..(nb as usize).min(8) {
            // a transition is a 0, no transition a 1
            let (x, tb) = (k > 0, t + k as f64 * step);
            if ones == 6 {
                if x {
                    stuff = Some(tb);
                    break;
                }
                ones = 0;
                continue;
            }
            edge_data |= k == 0;
            ones = if x { ones + 1 } else { 0 };
            b.push(x);
            tm.push(tb);
            if let Some((next, pre)) = usb_end(bus, &b, tb + step, bit) {
                let te = tb + step;
                tm.push(te);
                usb_fields(&b, &tm, kbit, true, out);
                if !pre {
                    let eop_end = if bus.fs_pol { segs.get(next).map_or(te + 2.0 * bit, |s| s.0) } else { te + 2.0 * bit };
                    out.push(tag("!EOP", te, eop_end, true));
                }
                return next;
            }
        }
        if stuff.is_some() {
            break;
        }
        j += 1;
    }
    match stuff {
        Some(ts) if !bus.two && segs[j].1 == Ls::J => {
            // D+ alone, no length / CRC match: the idle ends the packet. Full-speed polarity: the EOP (looks like K)
            // ends where the idle starts, the bits before it count to the last whole byte. Low speed: the EOP looks
            // like J, the transition before the idle is the last data bit (then ones up to the byte boundary), a
            // stuff bit, or the start of the EOP itself (one bit past the boundary)
            let l = if bus.fs_pol {
                tm.iter().take_while(|&&x| x < segs[j].0 - 2.0 * bit).count() / 8 * 8
            } else {
                let n1 = edge_bits + edge_data as usize;
                if n1 % 8 == 0 {
                    n1
                } else if n1 % 8 >= 2 {
                    n1 + 8 - n1 % 8
                } else {
                    n1 - 1
                }
            };
            let te = tm.get(l).copied().unwrap_or(ts);
            b.truncate(l);
            tm.truncate(l);
            tm.push(te);
            if l >= 8 {
                usb_fields(&b, &tm, kbit, true, out);
                out.push(tag("!EOP", te, if bus.fs_pol { segs[j].0 } else { te + 2.0 * bit }, true));
            }
            j + 1
        }
        Some(ts) => {
            // seven ones: the packet is lost after its PID
            let l = b.len().min(16);
            let te = tm.get(l).copied().unwrap_or(ts);
            b.truncate(l);
            tm.truncate(l);
            tm.push(te);
            usb_fields(&b, &tm, kbit, false, out);
            out.push(tag("!STUFF", ts, ts + bit, false));
            j + 1
        }
        None if j < segs.len() => {
            // two lines: SE0 = EOP, SE1 = an invalid line state
            let (t, e) = (segs[j].0, bus.end(j));
            tm.push(t);
            if segs[j].1 == Ls::Se1 {
                usb_fields(&b, &tm, kbit, false, out);
                out.push(tag("!ERR", t, e, false));
                return j + 1;
            }
            if e >= bus.n && e - t < reset {
                return segs.len();
            }
            usb_fields(&b, &tm, kbit, true, out);
            if e - t >= reset {
                return j; // a reset right after the packet
            }
            let w = (e - t) / bit;
            out.push(tag("!EOP", t, e, w > 0.6 && w < 3.5 && segs.get(j + 1).is_some_and(|s| s.1 == Ls::J)));
            j + 1
        }
        None => segs.len(),
    }
}

/// Whether a packet ends after the bits `b` (ending at `te`): a PRE after its PID (true: no EOP), or with D+ alone
/// the length of its PID / a matching CRC16 when an EOP follows; the segment to go on from.
fn usb_end(bus: &UsbBus, b: &[bool], te: f64, bit: f64) -> Option<(usize, bool)> {
    let l = b.len();
    if l < 16 || l % 8 != 0 || b[..7].iter().any(|&x| x) || !b[7] {
        return None;
    }
    let byte = |k: usize| (0..8).fold(0u8, |a, q| a | (b[8 * k + q] as u8) << q);
    let pid = byte(1);
    let m = l / 8 - 1; // bytes after the SYNC
    if pid == 0x3C && m == 1 {
        // the hub enables its low-speed ports, the host drives J, then the low-speed SYNC
        return Some((bus.at(te - 0.5 * bit) + 1, true));
    }
    if bus.two {
        return None;
    }
    let len_ok = match pid {
        0xE1 | 0x69 | 0x2D | 0xB4 | 0xA5 => m == 3,
        0xD2 | 0x5A | 0x1E | 0x96 => m == 1,
        0xC3 | 0x4B | 0x87 | 0x0F => m >= 3 && byte(m - 1) as u16 | (byte(m) as u16) << 8 == usb_crc16(&(2..m - 1).map(byte).collect::<Vec<_>>()),
        _ => false,
    };
    if len_ok {
        bus.eop_after(te, bit).map(|k| (k, false))
    } else {
        None
    }
}

/// The fields of a packet from its unstuffed bits `b` (SYNC first) and their start times `tm` (one more: the end).
/// `whole` = the packet is complete (else only SYNC and PID).
fn usb_fields(b: &[bool], tm: &[f64], kbit: u32, whole: bool, out: &mut Vec<Packet>) {
    let l = b.len();
    if l < 8 {
        return;
    }
    let val = |a: usize, len: usize| (0..len).fold(0u32, |acc, k| acc | (b[a + k] as u32) << k);
    let sync_ok = b[..7].iter().all(|&x| !x) && b[7];
    out.push(pk(Kind::Field("SYNC"), tm[0], tm[8], kbit, None, sync_ok));
    if !sync_ok {
        return;
    }
    if l < 16 {
        if whole {
            out.push(tag("!ERR", tm[8], tm[l], false));
        }
        return;
    }
    let pid = val(8, 8) as u8;
    let pid_ok = pid >> 4 == !pid & 15;
    out.push(pk(Kind::Field("PID"), tm[8], tm[16], pid as u32, None, pid_ok));
    if !whole {
        return;
    }
    let nb = (l - 8) / 8; // bytes after the SYNC, the PID included
    let byte = |k: usize| pk(Kind::Data, tm[8 + 8 * k], tm[16 + 8 * k], val(8 + 8 * k, 8), None, true);
    // bytes shown, and whether the length fits the PID
    let (shown, len_ok) = match if pid_ok { pid & 15 } else { 0 } {
        // tokens, SOF: 11 bits and CRC5
        1 | 9 | 13 | 4 | 5 if nb >= 3 => {
            if pid & 15 == 5 {
                out.push(pk(Kind::Field("FRAME"), tm[16], tm[27], val(16, 11), None, true));
            } else {
                out.push(pk(Kind::Field("ADDR"), tm[16], tm[23], val(16, 7), None, true));
                out.push(pk(Kind::Field("ENDP"), tm[23], tm[27], val(23, 4), None, true));
            }
            let c = val(27, 5);
            out.push(pk(Kind::Crc, tm[27], tm[32], c, None, c == usb_crc5(&b[16..27]) as u32));
            (3, nb == 3)
        }
        1 | 9 | 13 | 4 | 5 => (1, false),
        // data: payload and CRC16
        3 | 11 | 7 | 15 if nb >= 3 => {
            let payload: Vec<u8> = (1..nb - 2).map(|k| val(8 + 8 * k, 8) as u8).collect();
            out.extend((1..nb - 2).map(byte));
            let a = 8 + 8 * (nb - 2);
            let c = val(a, 16);
            out.push(pk(Kind::Crc, tm[a], tm[a + 16], c, None, c == usb_crc16(&payload) as u32));
            (nb, true)
        }
        3 | 11 | 7 | 15 => (1, false),
        // handshakes, PRE
        2 | 10 | 14 | 6 | 12 => (1, nb == 1),
        // SPLIT, reserved, a wrong check nibble: the bytes
        _ => {
            out.extend((1..nb).map(byte));
            (nb, true)
        }
    };
    let rest = 8 + 8 * shown;
    if !len_ok || l >= rest + 2 {
        out.push(tag("!ERR", tm[rest], tm[l], false));
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Signal generators for the decoders above (tests, demo source).
pub mod gen {
    use super::{sent_crc, usb_crc16, usb_crc5};
    use crate::sim::{Levels, Logic};

    /// The 32 bits of an ARINC 429 word in bus order (bit 1 first): label MSB first, then SDI, data (19 bits), SSM,
    /// LSB first, odd parity (`bad_parity` flips it).
    pub fn arinc429_word(label: u8, sdi: u8, data: u32, ssm: u8, bad_parity: bool) -> Vec<bool> {
        let mut b: Vec<bool> = (0..8).rev().map(|k| label >> k & 1 == 1).collect();
        b.extend((0..2).map(|k| sdi >> k & 1 == 1));
        b.extend((0..19).map(|k| data >> k & 1 == 1));
        b.extend((0..2).map(|k| ssm >> k & 1 == 1));
        let ones = b.iter().filter(|&&x| x).count();
        b.push((ones % 2 == 0) != bad_parity);
        b
    }

    /// ARINC 429 words (bus bit order) as bipolar RZ of amplitude `a` at `rate`: +a for a 1, −a for a 0 during the
    /// first half of the bit, NULL (0 V) otherwise; `gap` bit times of NULL before every word.
    pub fn arinc429(words: &[Vec<bool>], rate: f64, a: f64, gap: usize) -> Levels {
        let bit = 1.0 / rate;
        let mut l = Levels { seg: vec![(0.0, 0.0)], tr: bit / 10.0 };
        let mut t = 0.0;
        for w in words {
            t += gap as f64 * bit;
            for &b in w {
                l.seg.push((t, if b { a } else { -a }));
                l.seg.push((t + 0.5 * bit, 0.0));
                t += bit;
            }
        }
        l.seg.push((t + gap as f64 * bit, 0.0));
        l
    }

    /// One SENT frame for `sent_ticks`: status and data nibbles, `crc` = None for the right CRC (recommended method),
    /// `pause` = pause pulse ticks (0 = none).
    #[derive(Clone, Debug)]
    pub struct SentFrame {
        pub status: u8,
        pub data: Vec<u8>,
        pub crc: Option<u8>,
        pub pause: u32,
    }

    /// The falling-edge intervals of SENT frames in ticks: sync 56, nibbles 12 + value, the pause.
    pub fn sent_ticks(frames: &[SentFrame]) -> Vec<u32> {
        let mut t = Vec::new();
        for f in frames {
            t.push(56);
            t.push(12 + (f.status & 15) as u32);
            t.extend(f.data.iter().map(|&x| 12 + (x & 15) as u32));
            t.push(12 + (f.crc.unwrap_or_else(|| sent_crc(&f.data, false)) & 15) as u32);
            if f.pause > 0 {
                t.push(f.pause);
            }
        }
        t
    }

    /// SENT intervals (ticks) as a logic waveform, idle high: every interval starts with a falling edge and 5 ticks
    /// low; a last falling edge (and low pulse) ends the last interval.
    pub fn sent_logic(ticks: &[u32], tick: f64, lo: f64, hi: f64) -> Logic {
        let mut l = Logic::new(lo, hi, tick / 4.0);
        l.push(0.0, true);
        let mut t = 20.0 * tick;
        for &n in ticks.iter().chain([&5]) {
            l.push(t, false);
            l.push(t + 5.0 * tick, true);
            t += n as f64 * tick;
        }
        l
    }

    /// SENT frames as a logic waveform at `tick` (s).
    pub fn sent(frames: &[SentFrame], tick: f64, lo: f64, hi: f64) -> Logic {
        sent_logic(&sent_ticks(frames), tick, lo, hi)
    }

    /// A token packet (PID byte as sent: OUT 0xE1, IN 0x69, SETUP 0x2D, PING 0xB4): PID, ADDR, ENDP, CRC5.
    pub fn usb_token(pid: u8, addr: u8, endp: u8) -> Vec<u8> {
        usb_token11(pid, addr as u16 & 0x7f | (endp as u16 & 15) << 7)
    }

    /// A start-of-frame packet: PID, frame number (11 bits), CRC5.
    pub fn usb_sof(frame: u16) -> Vec<u8> {
        usb_token11(0xA5, frame & 0x7ff)
    }

    fn usb_token11(pid: u8, v: u16) -> Vec<u8> {
        let bits: Vec<bool> = (0..11).map(|k| v >> k & 1 == 1).collect();
        let w = v | (usb_crc5(&bits) as u16) << 11;
        vec![pid, w as u8, (w >> 8) as u8]
    }

    /// A data packet (DATA0 0xC3, DATA1 0x4B, DATA2 0x87, MDATA 0x0F): PID, payload, CRC16 low byte first.
    pub fn usb_data(pid: u8, payload: &[u8]) -> Vec<u8> {
        let c = usb_crc16(payload);
        let mut v = vec![pid];
        v.extend_from_slice(payload);
        v.extend([c as u8, (c >> 8) as u8]);
        v
    }

    /// The bits of a packet before stuffing: SYNC (0000 0001, KJKJKJKK on the bus), then the bytes LSB first.
    pub fn usb_bits(bytes: &[u8]) -> Vec<bool> {
        let mut b: Vec<bool> = (0..8).map(|k| k == 7).collect();
        b.extend(bytes.iter().flat_map(|&x| (0..8).map(move |k| x >> k & 1 == 1)));
        b
    }

    /// Bit stuffing: a zero after every six ones in a row (also at the end of the packet).
    pub fn usb_stuff(bits: &[bool]) -> Vec<bool> {
        let mut out = Vec::new();
        let mut ones = 0;
        for &x in bits {
            out.push(x);
            ones = if x { ones + 1 } else { 0 };
            if ones == 6 {
                out.push(false);
                ones = 0;
            }
        }
        out
    }

    /// One item of USB bus traffic.
    #[derive(Clone, Debug)]
    pub enum UsbItem {
        /// a packet: its bits on the bus (SYNC first, stuffed) at `rate` (bit/s); `eop` = SE0 for two bit times and
        /// one bit of J after it, else J right away (PRE)
        Packet { bits: Vec<bool>, rate: f64, eop: bool },
        /// idle (J) for the time (s)
        Idle(f64),
        /// SE0 for the time (s), then J: a reset, a low-speed keep-alive
        Se0(f64),
    }

    impl UsbItem {
        /// The packet of `bytes` (PID first) with SYNC, stuffing and EOP.
        pub fn packet(bytes: &[u8], rate: f64) -> UsbItem {
            UsbItem::Packet { bits: usb_stuff(&usb_bits(bytes)), rate, eop: true }
        }
    }

    /// USB traffic as D+ and D-: NRZI from J (a 0 toggles J / K, a 1 keeps the state). `low_speed` = low-speed
    /// signalling (J = D- high), else full speed (J = D+ high); D- is delayed by `skew` (s; the receiver sees short
    /// SE0 / SE1 at the transitions).
    pub fn usb(items: &[UsbItem], low_speed: bool, skew: f64, lo: f64, hi: f64) -> (Logic, Logic) {
        let tr = 4e-9;
        let (mut dp, mut dm) = (Logic::new(lo, hi, tr), Logic::new(lo, hi, tr));
        // (D+, D-) of J, K, SE0
        let j = (!low_speed, low_speed);
        let k = (low_speed, !low_speed);
        let mut set = |t: f64, s: (bool, bool)| {
            dp.push(t, s.0);
            dm.push(t + skew, s.1);
        };
        set(0.0, j);
        let mut t = 0.0;
        for it in items {
            match it {
                UsbItem::Packet { bits, rate, eop } => {
                    let bt = 1.0 / rate;
                    let mut st = true; // J
                    for &x in bits {
                        st ^= !x;
                        set(t, if st { j } else { k });
                        t += bt;
                    }
                    if *eop {
                        set(t, (false, false));
                        t += 2.0 * bt;
                        set(t, j);
                        t += bt;
                    } else {
                        set(t, j);
                    }
                }
                UsbItem::Idle(d) => t += d,
                UsbItem::Se0(d) => {
                    set(t, (false, false));
                    t += d;
                    set(t, j);
                }
            }
        }
        // the end of the traffic
        for l in [&mut dp, &mut dm] {
            let last = l.seg.last().map_or(false, |s| s.1);
            l.seg.push((t, last));
        }
        (dp, dm)
    }
}
