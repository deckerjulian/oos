// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal generators for the decoder tests and the demo source: the waveform builder and the byte-oriented buses
//! (DMX512, Modbus, 1-Wire, ARINC 429, S/PDIF, DCC) here, the larger families in `gen/` (`debug`: SWD, JTAG, Quad-SPI,
//! I3C; `usb`: USB, USB PD; `vehicle`: CAN FD, FlexRay, SENT).
//!
//! A generator for a new decoder goes next to the generators of its family, in `gen.rs` or a file in `gen/`; the
//! decoder itself goes into the file of its family in `decode/`, and its protocol module is registered in
//! `ui/src/protocols/mod.rs` (ADR 0002).

use crate::decode::{dcc_xor, modbus_crc, modbus_lrc, Parity, DCC_ONE_HALF, DCC_ZERO_HALF, DMX_BAUD, RDM_SC};
use crate::sim::{Levels, Logic};

mod debug;
mod usb;
mod vehicle;

pub use debug::*;
pub use usb::*;
pub use vehicle::*;

/// Builder of a logic waveform from levels and durations; idle high at t = 0.
#[derive(Debug)]
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
            Parity::Odd => self.level(ones.is_multiple_of(2), tb),
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
    Reset {
        presence: bool,
    },
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
        let (a, rd0, c, slot, h, pw, pl, j) = if od {
            (1.0, 5.0, 7.5, 10.0, 70.0, 3.0, 12.0, 40.0)
        } else {
            (6.0, 30.0, 60.0, 70.0, 480.0, 30.0, 120.0, 410.0)
        };
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

/// Biphase-mark half-cells -> logic waveform from `t`: `ui` seconds per half cell; a final transition
/// ends the last cell, then the line holds.
fn halves_logic(halves: &[bool], ui: f64, l: &mut Logic, t: &mut f64) {
    for &x in halves {
        l.push(*t, x);
        *t += ui;
    }
    if let Some(&last) = halves.last() {
        l.push(*t, !last);
    }
}

/// Biphase-mark coding of `bits` starting from `level`: a transition at every cell start, one more in the middle
/// of a 1.
fn bmc(bits: &[bool], level: &mut bool, out: &mut Vec<bool>) {
    for &b in bits {
        *level = !*level;
        out.push(*level);
        if b {
            *level = !*level;
        }
        out.push(*level);
    }
}

/// S/PDIF stream of stereo frames (24-bit samples), channel status `cs` (both subframes), V = 0, U = 0, frame 0
/// starts a block; `bad_parity` = subframe indices (2 frame + channel) with a wrong parity bit; `fs` = sample rate.
pub fn spdif(frames: &[(u32, u32)], cs: &[u8; 24], fs: f64, bad_parity: &[usize], lo: f64, hi: f64) -> Logic {
    let ui = 1.0 / (128.0 * fs);
    let mut h: Vec<bool> = Vec::new();
    let mut lv = false;
    for (k, &(l, r)) in frames.iter().enumerate() {
        for (ch, s) in [(0usize, l), (1, r)] {
            let pre: u8 = if ch == 1 {
                0b1110_0100
            } else if k % 192 == 0 {
                0b1110_1000
            } else {
                0b1110_0010
            };
            let before = lv;
            for q in (0..8).rev() {
                lv = (pre >> q & 1 == 1) != before;
                h.push(lv);
            }
            let c = cs[(k % 192) / 8] >> (k % 8) & 1 == 1;
            let mut bits = bits_lsb(s as u64 & 0xff_ffff, 24);
            bits.extend([false, false, c]);
            let ones = bits.iter().filter(|&&b| b).count();
            bits.push((ones % 2 == 1) != bad_parity.contains(&(2 * k + ch)));
            bmc(&bits, &mut lv, &mut h);
        }
    }
    let mut l = Logic::new(lo, hi, ui / 10.0);
    l.push(0.0, false);
    let mut t = 8.0 * ui;
    halves_logic(&h, ui, &mut l, &mut t);
    l
}

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

/// One NMRA DCC packet: address, data bytes, optional wrong XOR (`None` = correct), preamble length in ones.
#[derive(Clone, Debug)]
pub struct DccFrame {
    pub address: u8,
    pub data: Vec<u8>,
    pub xor: Option<u8>,
    pub preamble: u32,
}

/// Bit stream of DCC frames: preamble ones, then for each byte a start `0` and 8 MSB-first bits, then end `1`.
pub fn dcc_bits(frames: &[DccFrame]) -> Vec<bool> {
    let mut bits = Vec::new();
    for f in frames {
        bits.extend(std::iter::repeat_n(true, f.preamble.max(10) as usize));
        let mut bytes = vec![f.address];
        bytes.extend_from_slice(&f.data);
        let x = f.xor.unwrap_or_else(|| dcc_xor(&bytes));
        bytes.push(x);
        for &b in &bytes {
            bits.push(false);
            for k in (0..8).rev() {
                bits.push((b >> k) & 1 == 1);
            }
        }
        bits.push(true);
    }
    bits
}

/// DCC frames as a logic waveform (idle low, then alternating half-bits at NMRA timings).
pub fn dcc(frames: &[DccFrame], lo: f64, hi: f64) -> Logic {
    let bits = dcc_bits(frames);
    let mut l = Logic::new(lo, hi, 1e-6);
    let mut t = 20e-6;
    let mut level = true;
    l.push(0.0, false);
    l.push(t, true);
    for &one in &bits {
        let half = if one { DCC_ONE_HALF } else { DCC_ZERO_HALF };
        t += half;
        level = !level;
        l.push(t, level);
        t += half;
        level = !level;
        l.push(t, level);
    }
    // trailing idle so the last half-bit has a closing edge and a following packet is not cut by the record end
    l.push(t + 500e-6, level);
    l
}
