// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! I3C SDR.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{at, msb, Kind, Packet};
use crate::edges::{edges, states};
use crate::protocols::ModuleInfo;

/// Common Command Code names (MIPI I3C Basic v1.1.1, table 16).
pub fn i3c_ccc_name(code: u8) -> &'static str {
    match code {
        0x00 => "ENEC",
        0x01 => "DISEC",
        0x02 => "ENTAS0",
        0x03 => "ENTAS1",
        0x04 => "ENTAS2",
        0x05 => "ENTAS3",
        0x06 => "RSTDAA",
        0x07 => "ENTDAA",
        0x08 => "DEFTGTS",
        0x09 => "SETMWL",
        0x0a => "SETMRL",
        0x0b => "ENTTM",
        0x12 => "ENDXFER",
        0x20..=0x27 => "ENTHDR",
        0x28 => "SETXTIME",
        0x29 => "SETAASA",
        0x2a => "RSTACT",
        0x2b => "DEFGRPA",
        0x2c => "RSTGRPA",
        0x80 => "ENEC (direct)",
        0x81 => "DISEC (direct)",
        0x82..=0x85 => "ENTAS (direct)",
        0x86 => "RSTDAA (direct)",
        0x87 => "SETDASA",
        0x88 => "SETNEWDA",
        0x89 => "SETMWL (direct)",
        0x8a => "SETMRL (direct)",
        0x8b => "GETMWL",
        0x8c => "GETMRL",
        0x8d => "GETPID",
        0x8e => "GETBCR",
        0x8f => "GETDCR",
        0x90 => "GETSTATUS",
        0x91 => "GETACCCR",
        0x92 => "ENDXFER (direct)",
        0x93 => "SETBRGTGT",
        0x94 => "GETMXDS",
        0x95 => "GETCAPS",
        0x9a => "RSTACT (direct)",
        _ => "?",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum I3cPh {
    Idle,
    Addr,
    Ccc,
    Bytes { read: bool, i3c: bool },
    Pid,
    Da,
}

struct I3c {
    out: Vec<Packet>,
    ph: I3cPh,
    buf: Vec<(f64, bool)>,
    in_frame: bool,
    after_start: bool,
    daa: bool,
    ccc: Option<u8>,
    known: Vec<u8>,
    user: Vec<u8>,
    hdr: bool,
}

impl I3c {
    fn bit(&mut self, t: f64, b: bool) {
        let need = match self.ph {
            I3cPh::Idle => return,
            I3cPh::Pid => 64,
            _ => 9,
        };
        self.buf.push((t, b));
        if self.buf.len() < need {
            return;
        }
        let u = std::mem::take(&mut self.buf);
        let (t0, t1) = (u[0].0, u[u.len() - 1].0);
        let v8 = msb(&u[..8]) as u32;
        let ninth = u.get(8).is_some_and(|x| x.1);
        let t_odd = (v8.count_ones() + ninth as u32) % 2 == 1;
        match self.ph {
            I3cPh::Idle => {}
            I3cPh::Addr => {
                let (addr, rnw, ack) = (v8 >> 1, v8 & 1 == 1, !ninth);
                let known = self.known.contains(&(addr as u8));
                if addr == 0x7e && rnw && self.daa {
                    self.out.push(Packet::new(Kind::Addr { read: true }, t0, t1, addr, ack));
                    self.ph = if ack { I3cPh::Pid } else { I3cPh::Idle };
                } else if addr == 0x7e && !rnw {
                    self.out.push(Packet::new(Kind::Addr { read: false }, t0, t1, addr, ack));
                    self.ccc = None;
                    self.daa = false;
                    self.ph = if ack { I3cPh::Ccc } else { I3cPh::Idle };
                } else if self.after_start && rnw && known {
                    // a target's in-band interrupt request won the address arbitration after START
                    self.out.push(Packet::field("IBI", t0, t1, addr, ack));
                    self.ph = if ack { I3cPh::Bytes { read: true, i3c: true } } else { I3cPh::Idle };
                } else {
                    let i3c = known || self.ccc.is_some_and(|c| c >= 0x80);
                    self.out.push(Packet::new(Kind::Addr { read: rnw }, t0, t1, addr, ack));
                    self.ph = if ack || i3c { I3cPh::Bytes { read: rnw, i3c } } else { I3cPh::Idle };
                    if !ack && i3c {
                        self.ph = I3cPh::Idle;
                    }
                }
            }
            I3cPh::Ccc => {
                self.out.push(Packet::field("CCC", t0, t1, v8, t_odd));
                self.ccc = Some(v8 as u8);
                self.ph = I3cPh::Bytes { read: false, i3c: true };
                match v8 {
                    0x07 => self.daa = true,
                    0x06 => self.known = self.user.clone(),
                    0x20..=0x27 => {
                        self.out.push(Packet::field("!HDR", t1, t1, v8 & 7, true));
                        self.hdr = true;
                        self.ph = I3cPh::Idle;
                    }
                    _ => {}
                }
            }
            I3cPh::Bytes { read, i3c } => {
                if !i3c {
                    self.out.push(Packet::new(Kind::Data, t0, t1, v8, !ninth));
                } else if read {
                    self.out.push(Packet::new(Kind::Data, t0, t1, v8, true));
                    if !ninth {
                        // T-bit low: the target ends the read
                        self.out.push(Packet::tag("!EOD", t1, t1, true));
                        self.ph = I3cPh::Idle;
                    }
                } else if matches!(self.ccc, Some(0x87 | 0x88)) {
                    // SETDASA / SETNEWDA: the new dynamic address
                    let da = v8 >> 1;
                    self.out.push(Packet::field("DA", t0, t1, da, t_odd));
                    if t_odd && !self.known.contains(&(da as u8)) {
                        self.known.push(da as u8);
                    }
                } else {
                    self.out.push(Packet::new(Kind::Data, t0, t1, v8, t_odd));
                }
            }
            I3cPh::Pid => {
                let pid = msb(&u[..48]);
                self.out.push(Packet::field("PID", t0, u[47].0, pid as u32, true).with_value2((pid >> 32) as u32));
                self.out.push(Packet::field("BCR", u[48].0, u[55].0, msb(&u[48..56]) as u32, true));
                self.out.push(Packet::field("DCR", u[56].0, t1, msb(&u[56..64]) as u32, true));
                self.ph = I3cPh::Da;
            }
            I3cPh::Da => {
                // 7-bit dynamic address, odd parity bit, target ACK
                let da = v8 >> 1;
                let ok = (v8.count_ones() % 2 == 1) && !ninth;
                self.out.push(Packet::field("DA", t0, t1, da, ok));
                if ok && !self.known.contains(&(da as u8)) {
                    self.known.push(da as u8);
                }
                self.ph = I3cPh::Idle;
            }
        }
    }
}

/// MIPI I3C on SDA and SCL.
pub static I3C: ModuleInfo = ModuleInfo::new("I3c", "I3C", "i3c")
    .roles([Some("SDA"), Some("SCL"), None, None])
    // a bit at 10 MHz (SDR)
    .bit(|_, _| 0.1e-6);

/// I3C SDR (MIPI I3C Basic v1.1.1): START/Sr/STOP and bits like I2C (SDA sampled on the rising SCL edge, MSB first).
/// Emits Start/RepStart/Stop, `Kind::Addr { read }` (ok = ACK) incl. the broadcast address 0x7E, `Field("CCC")`
/// (value = code, see `i3c_ccc_name`; ok = T-bit odd parity), `Kind::Data` (I3C write: ok = T-bit odd parity; I3C
/// read: ok, followed by `Field("!EOD")` when the target ends the read with T = 0; I2C: ok = ACK). ENTDAA: per
/// target `Field("PID")` (48-bit provisioned ID: low 32 bits in value, high 16 in value2), `Field("BCR")`,
/// `Field("DCR")` and `Field("DA")` (assigned address, ok = parity and ACK); SETDASA/SETNEWDA give `Field("DA")` too.
/// ENTHDRx gives `Field("!HDR")` (value = x) and the decoder skips the HDR traffic until the HDR exit pattern (4 SDA
/// falling edges while SCL stays low), marked `Field("!HDREXIT")`.
///
/// Addresses use T-bits (I3C) when they were assigned in the capture or are listed in `i3c_addrs`, and inside direct
/// CCCs; all others are decoded as I2C with ACK bits. A read of a known I3C address directly after START (not Sr) is
/// shown as `Field("IBI")` (the target won the arbitration with an in-band interrupt request).
pub fn i3c(sda: &[f32], scl: &[f32], i3c_addrs: &[u8], thr: f32, hyst: f32) -> Vec<Packet> {
    let scl_s = states(scl, thr, hyst);
    let sda_s = states(sda, thr, hyst);
    // 0 = SCL rising, 1 = SDA falling, 2 = SDA rising, 3 = SCL falling
    let mut ev: Vec<(f64, u8)> = edges(scl, thr, hyst).iter().map(|e| (e.t, if e.rising { 0 } else { 3 })).collect();
    ev.extend(edges(sda, thr, hyst).iter().map(|e| (e.t, if e.rising { 2 } else { 1 })));
    ev.sort_by(|a, b| a.0.total_cmp(&b.0));
    let high = |s: &[Option<bool>], t: f64| at(s, t).unwrap_or(true);
    let mut d = I3c {
        out: Vec::new(),
        ph: I3cPh::Idle,
        buf: Vec::new(),
        in_frame: false,
        after_start: false,
        daa: false,
        ccc: None,
        known: i3c_addrs.to_vec(),
        user: i3c_addrs.to_vec(),
        hdr: false,
    };
    let mut exit_cnt = 0;
    for (t, k) in ev {
        if d.hdr {
            match k {
                0 | 3 => exit_cnt = 0,
                1 if !high(&scl_s, t) => {
                    exit_cnt += 1;
                    if exit_cnt == 4 {
                        d.hdr = false;
                        d.ph = I3cPh::Idle;
                        d.out.push(Packet::tag("!HDREXIT", t, t, true));
                    }
                }
                _ => {}
            }
            continue;
        }
        match k {
            1 if high(&scl_s, t) => {
                d.out.push(Packet::new(if d.in_frame { Kind::RepStart } else { Kind::Start }, t, t, 0, true));
                d.after_start = !d.in_frame;
                d.in_frame = true;
                d.ph = I3cPh::Addr;
                d.buf.clear();
            }
            2 if high(&scl_s, t) => {
                d.out.push(Packet::new(Kind::Stop, t, t, 0, true));
                d.in_frame = false;
                d.daa = false;
                d.ccc = None;
                d.ph = I3cPh::Idle;
                d.buf.clear();
            }
            0 => {
                let b = high(&sda_s, t);
                d.bit(t, b);
                if d.hdr {
                    exit_cnt = 0;
                }
            }
            _ => {}
        }
    }
    d.out
}
