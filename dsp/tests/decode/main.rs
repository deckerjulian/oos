// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The protocol decoders of `oos_dsp::decode` against generated signals with known content (round trips, corrupted
//! bits, noise), one module per protocol family as in `dsp/src/decode/`, and the helpers they share.
//!
//! The tests of a new decoder go into the module of its family, a new family gets a module of its own and a `mod` line
//! here; the decoder's protocol module is registered in `ui/src/protocols/mod.rs` (ADR 0002).

use oos_dsp::decode::gen::{
    self, bits_lsb, DccFrame, FdFrame, FrFrame, I3cBus, JtagGen, Ow, PdFrame, QspiFrame, SentFrame, SwdOp, Wave,
};
use oos_dsp::decode::*;
use oos_dsp::sim::{add_noise, bits_logic, i2c_logic, sample, I2cXfer, Levels, Logic};
use oos_dsp::trigger::Slope;

mod audio;
mod automotive;
mod avionics;
mod can;
mod dcc;
mod debug;
mod dmx;
mod garbage;
mod i3c;
mod modbus;
mod onewire;
mod parallel;
mod qspi;
mod serial;
mod usb;
mod usbpd;

/// xorshift random numbers
fn rng(seed: u64) -> impl FnMut() -> u32 {
    let mut s = seed | 1;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 16) as u32
    }
}

fn label(k: Kind) -> String {
    match k {
        Kind::Field(l) => l.to_string(),
        Kind::Data => "D".into(),
        Kind::Addr { .. } => "A".into(),
        Kind::Crc => "CRC".into(),
        Kind::Start => "S".into(),
        Kind::Stop => "P".into(),
        Kind::Break => "BRK".into(),
        Kind::Ack => "ACK".into(),
        Kind::Id { .. } => "ID".into(),
        k => format!("{:?}", k),
    }
}

/// (label, value, ok) of every packet
fn lvo(p: &[Packet]) -> Vec<(String, u32, bool)> {
    p.iter().map(|x| (label(x.kind), x.value, x.ok)).collect()
}

fn vals(p: &[Packet], l: &str) -> Vec<u32> {
    p.iter().filter(|x| label(x.kind) == l).map(|x| x.value).collect()
}

fn oks(p: &[Packet], l: &str) -> Vec<bool> {
    p.iter().filter(|x| label(x.kind) == l).map(|x| x.ok).collect()
}

fn e(l: &str, v: u32, ok: bool) -> (String, u32, bool) {
    (l.to_string(), v, ok)
}

/// Samples logic waveforms to a common length (until shortly after the last change of any of them).
fn waves(ls: &[&Logic], dt: f64) -> Vec<Vec<f32>> {
    let end = ls.iter().map(|l| l.end()).fold(0.0, f64::max);
    let n = (end / dt) as usize + 200;
    ls.iter().map(|l| sample(|t| l.v(t), 0.0, dt, n)).collect()
}

fn kv(p: &[Packet]) -> Vec<(Kind, u32, bool)> {
    p.iter().map(|x| (x.kind, x.value, x.ok)).collect()
}

const F: fn(&'static str) -> Kind = Kind::Field;

const ID: Kind = Kind::Id { ext: false, rtr: false };

fn levels(l: &Levels, dt: f64) -> Vec<f32> {
    let end = l.seg.last().map_or(0.0, |s| s.0);
    sample(|t| l.v(t), 0.0, dt, (end / dt) as usize)
}
