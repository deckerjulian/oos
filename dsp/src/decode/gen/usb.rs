// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal generators for the USB and USB Power Delivery decoders.
//!
//! A generator for a new decoder goes next to the generators of its family, in `gen.rs` or a file in `gen/`; the
//! decoder itself goes into the file of its family in `decode/`, and its protocol module is registered in
//! `ui/src/protocols/mod.rs` (ADR 0002).

use super::{bits_lsb, bmc};
use crate::decode::{usb_crc16, usb_crc5, usbpd_crc32, PD_4B5B, PD_EOP, PD_SETS};
use crate::sim::Logic;

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
        let last = l.seg.last().is_some_and(|s| s.1);
        l.seg.push((t, last));
    }
    (dp, dm)
}

/// One USB PD transmission for `usbpd`.
#[derive(Clone, Debug)]
pub enum PdFrame {
    /// `sop` = index of the ordered set (0 SOP, 1 SOP', 2 SOP'', 3 SOP'_Debug, 4 SOP''_Debug)
    Msg {
        sop: usize,
        header: u16,
        data: Vec<u32>,
        bad_crc: bool,
    },
    HardReset,
    CableReset,
}

/// The bits of a transmission: preamble (64 alternating bits from 0), ordered set, 4b5b payload with CRC-32, EOP.
pub fn usbpd_bits(f: &PdFrame) -> Vec<bool> {
    let mut b: Vec<bool> = (0..64).map(|k| k % 2 == 1).collect();
    let sym = |b: &mut Vec<bool>, s: u8| b.extend(bits_lsb(s as u64, 5));
    let set = match f {
        PdFrame::Msg { sop, .. } => (*sop).min(4),
        PdFrame::HardReset => 5,
        PdFrame::CableReset => 6,
    };
    PD_SETS[set].1.iter().for_each(|&k| sym(&mut b, k));
    if let PdFrame::Msg { header, data, bad_crc, .. } = f {
        let mut bytes = header.to_le_bytes().to_vec();
        data.iter().for_each(|d| bytes.extend(d.to_le_bytes()));
        let crc = usbpd_crc32(&bytes) ^ *bad_crc as u32;
        bytes.extend(crc.to_le_bytes());
        for x in bytes {
            sym(&mut b, PD_4B5B[(x & 15) as usize]);
            sym(&mut b, PD_4B5B[(x >> 4) as usize]);
        }
        sym(&mut b, PD_EOP);
    }
    b
}

/// USB PD transmissions on CC as BMC at `bitrate`, idle low, 40 bit times between them.
pub fn usbpd(frames: &[PdFrame], bitrate: f64, lo: f64, hi: f64) -> Logic {
    let ui = 0.5 / bitrate;
    let mut l = Logic::new(lo, hi, ui / 15.0);
    l.push(0.0, false);
    let mut t = 20.0 * ui;
    for f in frames {
        let mut h = Vec::new();
        let mut lv = false;
        bmc(&usbpd_bits(f), &mut lv, &mut h);
        for &x in &h {
            l.push(t, x);
            t += ui;
        }
        // hold low after the last bit (final transition if the line is high)
        l.push(t, false);
        t += 80.0 * ui;
    }
    l
}
