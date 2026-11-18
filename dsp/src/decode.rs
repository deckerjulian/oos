// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Protocol decoders on analog channels, one file per protocol family in `decode/`. Every decoder runs its channels
//! through a comparator (threshold `thr` + hysteresis `hyst`) and returns `Packet`s: packet times are fractional
//! sample indices like everywhere in oos-dsp, `ok` = the check of the field (CRC, parity, framing) passed, and fields
//! without a `Kind` of their own are `Kind::Field(label)`. Signal generators for the tests and the demo source are in
//! `gen`.
//!
//! This file keeps the packet types and the bit helpers the families share and re-exports every family, so
//! `oos_dsp::decode` is the one path. A new decoder goes into the file of its family; a new family gets a file of its
//! own and a `mod` and `pub use` line here. The ui reaches a decoder through its protocol module, registered in
//! `ui/src/protocols/mod.rs` (ADR 0002).

use crate::edges::edges;

mod audio;
mod automotive;
mod avionics;
mod can;
pub(crate) mod crc;
mod dcc;
mod debug;
mod dmx;
pub mod gen;
mod i3c;
mod modbus;
mod onewire;
mod parallel;
mod qspi;
mod serial;
mod usb;
mod usb_bus;
mod usbpd;

pub use audio::*;
pub use automotive::*;
pub use avionics::*;
pub use can::*;
pub use crc::{
    can_crc15, can_fd_crc, flexray_frame_crc, flexray_header_crc, modbus_crc, ow_crc8, sent_crc, usb_crc16, usb_crc5,
    usbpd_crc32,
};
pub use dcc::*;
pub use debug::*;
pub use dmx::*;
pub use i3c::*;
pub use modbus::*;
pub use onewire::*;
pub use parallel::*;
pub use qspi::*;
pub use serial::*;
pub use usb::*;
pub use usbpd::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// UART character / SPI word / I2C data byte
    Data,
    /// I2C address byte (value = 7-bit address, `read` flag)
    Addr {
        read: bool,
    },
    Start,
    RepStart,
    Stop,
    /// CAN identifier (value = 11- or 29-bit ID) / LIN protected identifier (value = 6-bit ID, `ok` = parity)
    Id {
        ext: bool,
        rtr: bool,
    },
    /// CAN CRC-15 / LIN checksum (`ok` = it matches)
    Crc,
    /// CAN ACK slot (`ok` = acknowledged, i.e. dominant)
    Ack,
    /// LIN break field
    Break,
    /// I2S audio sample (value = the word, two's complement in `bits`; `right` = right channel)
    Sample {
        right: bool,
    },
    /// MIL-STD-1553 word (value = 16 bits; `cmd` = command/status sync, else data sync; `ok` = odd parity)
    Word {
        cmd: bool,
    },
    /// any other labelled field of the protocol decoders: the label is shown before the value ("CMD", "ADDR", "ROM",
    /// "SOF", "HDR", ...); `ok` = check passed. A label starting with '!' marks an event without a value (shown as a
    /// tag like Start/Stop).
    Field(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub t0: f64,
    pub t1: f64,
    pub kind: Kind,
    pub value: u32,
    /// second value (SPI MISO)
    pub value2: Option<u32>,
    /// UART: stop bit and parity ok; I2C: acknowledged
    pub ok: bool,
}

impl Packet {
    /// A packet of `kind` without a second value.
    pub fn new(kind: Kind, t0: f64, t1: f64, value: u32, ok: bool) -> Packet {
        Packet { t0, t1, kind, value, value2: None, ok }
    }

    /// A labelled protocol field (`Kind::Field`).
    pub fn field(label: &'static str, t0: f64, t1: f64, value: u32, ok: bool) -> Packet {
        Packet::new(Kind::Field(label), t0, t1, value, ok)
    }

    /// A value-less event tag (label starting with '!').
    pub fn tag(label: &'static str, t0: f64, t1: f64, ok: bool) -> Packet {
        Packet::field(label, t0, t1, 0, ok)
    }

    /// The packet with a second value.
    pub fn with_value2(self, value2: u32) -> Packet {
        Packet { value2: Some(value2), ..self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
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
    let value2 = (n > 4).then_some((v >> 32) as u32);
    out.push(Packet { value2, ..Packet::field(label, s[a].1, s[a + n - 1].2, v as u32, ok && fok) });
    true
}

fn rising(clk: &[f32], thr: f32, hyst: f32) -> Vec<f64> {
    edges(clk, thr, hyst).into_iter().filter(|e| e.rising).map(|e| e.t).collect()
}

/// Comparator state at time `t` (nearest sample).
fn at(st: &[Option<bool>], t: f64) -> Option<bool> {
    st.get((t + 0.5) as usize).copied().flatten()
}

/// Bits LSB first -> value.
fn lsb(bits: &[(f64, bool)]) -> u32 {
    bits.iter().take(32).enumerate().fold(0, |v, (k, b)| v | (b.1 as u32) << k)
}

/// Bits MSB first -> value.
fn msb(bits: &[(f64, bool)]) -> u64 {
    bits.iter().fold(0, |v, b| v << 1 | b.1 as u64)
}

fn edge_times(v: &[f32], thr: f32, hyst: f32) -> Vec<f64> {
    edges(v, thr, hyst).into_iter().map(|e| e.t).collect()
}
