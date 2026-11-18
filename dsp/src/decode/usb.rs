// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! USB 1.x / 2.0 low speed and full speed: the packets. The line states of D+ and D- are in `usb_bus`.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::usb_bus::{usb_bus, Ls, UsbBus};
use super::{usb_crc16, usb_crc5, Kind, Packet};
use crate::protocols::{BusWhat, ModuleInfo};

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

/// USB at low and full speed: D+, and D- when there is one.
pub static USB: ModuleInfo = ModuleInfo::new("Usb", "USB", "usb")
    .roles([Some("D+"), None, Some("D-"), None])
    .trigger(usb_trigger)
    .starts_on(BusWhat::Id, 5)
    // auto: room for a low-speed packet before the trigger point
    .bit(|_, sh| 1.0 / if sh.usb_speed == UsbSpeed::Low { USB_LOW } else { USB_FULL })
    .span(|_, sh| (sh.usb_speed == UsbSpeed::Auto).then_some(200.0 / USB_LOW))
    .decoder(|l| Ok(usb(l.main(), l.lines[2], l.dt, l.sh.usb_speed, l.thr, l.hy)));

/// USB's rule of the protocol trigger: an ID is the device address or a PID.
fn usb_trigger(what: BusWhat, value: u32, p: &Packet) -> Option<bool> {
    (what == BusWhat::Id).then_some(matches!(p.kind, Kind::Field("ADDR" | "PID")) && p.value == value)
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
    if !dt.is_finite() || dt <= 0.0 {
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
                    out.push(Packet::tag("!RESET", t, e, true));
                } else if i > 0 && i + 1 < segs.len() {
                    out.push(Packet::tag("!EOP", t, e, segs[i + 1].1 == Ls::J));
                }
                i += 1;
            }
            Ls::K if i > 0 && segs[i - 1].1 == Ls::J => match bus.sync(i) {
                Some((bit, kbit)) => {
                    // a SYNC from idle: after an EOP, a long J or where the last packet ended (PRE, D+ alone); else
                    // (after a packet cut by the record start) only a packet without errors
                    let from_idle = resume.is_some_and(|r| i <= r + 1)
                        || (i >= 2 && segs[i - 2].1 == Ls::Se0)
                        || bus.dur(i - 1) > idle;
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
                    let eop_end =
                        if bus.fs_pol { segs.get(next).map_or(te + 2.0 * bit, |s| s.0) } else { te + 2.0 * bit };
                    out.push(Packet::tag("!EOP", te, eop_end, true));
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
                if n1.is_multiple_of(8) {
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
                out.push(Packet::tag("!EOP", te, if bus.fs_pol { segs[j].0 } else { te + 2.0 * bit }, true));
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
            out.push(Packet::tag("!STUFF", ts, ts + bit, false));
            j + 1
        }
        None if j < segs.len() => {
            // two lines: SE0 = EOP, SE1 = an invalid line state
            let (t, e) = (segs[j].0, bus.end(j));
            tm.push(t);
            if segs[j].1 == Ls::Se1 {
                usb_fields(&b, &tm, kbit, false, out);
                out.push(Packet::tag("!ERR", t, e, false));
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
            out.push(Packet::tag("!EOP", t, e, w > 0.6 && w < 3.5 && segs.get(j + 1).is_some_and(|s| s.1 == Ls::J)));
            j + 1
        }
        None => segs.len(),
    }
}

/// Whether a packet ends after the bits `b` (ending at `te`): a PRE after its PID (true: no EOP), or with D+ alone
/// the length of its PID / a matching CRC16 when an EOP follows; the segment to go on from.
fn usb_end(bus: &UsbBus, b: &[bool], te: f64, bit: f64) -> Option<(usize, bool)> {
    let l = b.len();
    if l < 16 || !l.is_multiple_of(8) || b[..7].iter().any(|&x| x) || !b[7] {
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
        0xC3 | 0x4B | 0x87 | 0x0F => {
            m >= 3 && byte(m - 1) as u16 | (byte(m) as u16) << 8 == usb_crc16(&(2..m - 1).map(byte).collect::<Vec<_>>())
        }
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
    out.push(Packet::field("SYNC", tm[0], tm[8], kbit, sync_ok));
    if !sync_ok {
        return;
    }
    if l < 16 {
        if whole {
            out.push(Packet::tag("!ERR", tm[8], tm[l], false));
        }
        return;
    }
    let pid = val(8, 8) as u8;
    let pid_ok = pid >> 4 == !pid & 15;
    out.push(Packet::field("PID", tm[8], tm[16], pid as u32, pid_ok));
    if !whole {
        return;
    }
    let nb = (l - 8) / 8; // bytes after the SYNC, the PID included
    let byte = |k: usize| Packet::new(Kind::Data, tm[8 + 8 * k], tm[16 + 8 * k], val(8 + 8 * k, 8), true);
    // bytes shown, and whether the length fits the PID
    let (shown, len_ok) = match if pid_ok { pid & 15 } else { 0 } {
        // tokens, SOF: 11 bits and CRC5
        1 | 9 | 13 | 4 | 5 if nb >= 3 => {
            if pid & 15 == 5 {
                out.push(Packet::field("FRAME", tm[16], tm[27], val(16, 11), true));
            } else {
                out.push(Packet::field("ADDR", tm[16], tm[23], val(16, 7), true));
                out.push(Packet::field("ENDP", tm[23], tm[27], val(23, 4), true));
            }
            let c = val(27, 5);
            out.push(Packet::new(Kind::Crc, tm[27], tm[32], c, c == usb_crc5(&b[16..27]) as u32));
            (3, nb == 3)
        }
        1 | 9 | 13 | 4 | 5 => (1, false),
        // data: payload and CRC16
        3 | 11 | 7 | 15 if nb >= 3 => {
            let payload: Vec<u8> = (1..nb - 2).map(|k| val(8 + 8 * k, 8) as u8).collect();
            out.extend((1..nb - 2).map(byte));
            let a = 8 + 8 * (nb - 2);
            let c = val(a, 16);
            out.push(Packet::new(Kind::Crc, tm[a], tm[a + 16], c, c == usb_crc16(&payload) as u32));
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
        out.push(Packet::tag("!ERR", tm[rest], tm[l], false));
    }
}
