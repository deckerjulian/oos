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
use oos_dsp::protocols::BusWhat;
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

/// The files of `dsp/src/decode/` that have no module of tests here, each with the reason. One in this list that has
/// a module here fails the test, so the list only gets shorter.
const NO_TESTS_HERE: [(&str, &str); 3] = [
    ("crc", "the CRC engine of the decoders: its tests are in its own file, and every family's tests run it"),
    ("gen", "the generators: they make the signals the tests of every family decode"),
    ("usb_bus", "the line states of USB that the packet decoder reads: the tests of `usb` run it"),
];

/// Every protocol family of `dsp/src/decode.rs` has its module of tests here, with a test in it, or stands in
/// `NO_TESTS_HERE` with the reason. A new family fails here until its tests are written.
#[test]
fn every_decoder_family_has_its_tests() {
    let (families, here) = (include_str!("../../src/decode.rs"), include_str!("main.rs"));
    let mut seen = Vec::new();
    for line in families.lines() {
        let line = line.strip_prefix("pub(crate) ").or(line.strip_prefix("pub ")).unwrap_or(line);
        let Some(family) = line.strip_prefix("mod ").and_then(|l| l.strip_suffix(';')) else { continue };
        seen.push(family);
        let tested = here.lines().any(|l| l == format!("mod {};", family));
        match NO_TESTS_HERE.iter().find(|x| x.0 == family) {
            Some(x) => {
                assert!(!tested && !x.1.is_empty(), "{}: it has its tests, take it out of NO_TESTS_HERE", family)
            }
            None => {
                assert!(
                    tested,
                    "{}: no `mod {};` in dsp/tests/decode/main.rs, and no reason in NO_TESTS_HERE",
                    family, family
                );
                let file = format!("{}/tests/decode/{}.rs", env!("CARGO_MANIFEST_DIR"), family);
                let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {}", file, e));
                assert!(text.contains("#[test]"), "{}: no test in dsp/tests/decode/{}.rs", family, family);
            }
        }
    }
    assert!(seen.len() > NO_TESTS_HERE.len(), "dsp/src/decode.rs names its families as `mod <family>;`");
    // an entry left behind by a removed or renamed family fails, and so does one that stands twice
    for (i, (key, _)) in NO_TESTS_HERE.iter().enumerate() {
        assert!(seen.contains(key), "NO_TESTS_HERE: {} is no family of dsp/src/decode.rs, take it out", key);
        assert!(NO_TESTS_HERE[..i].iter().all(|x| x.0 != *key), "NO_TESTS_HERE: {} stands twice", key);
    }
}
