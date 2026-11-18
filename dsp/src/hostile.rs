// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for oos-dsp (ADR 0047): the formula a person types or a setup file holds, and the samples of a
//! record, which can be none, one, or no numbers at all. Each test states the contract it holds its part to; the
//! generator is hal's `hostile/gen.rs`, which ui takes too. `cargo test hostile` runs them, `OOS_HOSTILE_ITERS` raises
//! the number of inputs. A new parser of outside input comes with its test here. A finding is not fixed in the test:
//! its smallest input becomes an ignored test, and `KNOWN` names it so the run steps over that one case until the
//! fix is merged.

// the generator of all three crates, one file (tests only: dsp itself depends on nothing)
#[path = "../../hal/src/hostile/gen.rs"]
mod gen;

use crate::decode::{self, Packet, Parity, QspiCfg, UsbSpeed};
use crate::trigger::{find_all, Bit, Input, Pol, Slope, Trigger, VideoSync, When, WinWhen};
use gen::{known, run, still_stand, Known};

/// The findings that still stand, each with its smallest input as an ignored test. The list only shrinks: a fix
/// removes its line and the `#[ignore]` of its test (`hostile_known_findings_still_stand` fails until it does).
const KNOWN: &[Known] = &[Known {
    name: "qspi-frame-ends-before-it-starts",
    what: "decode::qspi panics on a CS# frame that ends where it starts with a clock edge at that instant",
    repro: hostile_known_qspi_frame_ends_before_it_starts,
}];

// Break it catches: a finding that was fixed and is still stepped over
#[test]
fn hostile_known_findings_still_stand() {
    still_stand(KNOWN);
}

/// `Expr::parse` and `Expr::parse_sources` (the formula of a math trace: typed, sent by `:MATH<n>:EXPRession`, or
/// read from a setup file): return for any text; a formula they take reads only sources that were named, and gives
/// as many samples as its shortest source, or as asked when it reads none; a text over 64 bytes is refused.
#[test]
fn hostile_formulas() {
    let seeds: [&[u8]; 8] = [
        b"CH1*CH2/50",
        b"sqrt(CH1^2+CH2^2)",
        b"sin(2*pi*1000*t)",
        b"max(1,min(ch3,-ch4))+1.5e3",
        b"R1-M1+t",
        b"ln(exp(abs(-2)))^-1",
        b"1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1+(1)))))))))))))))",
        b" log( 10 ) / tan ( 0.5 ) ",
    ];
    let names = ["CH1", "CH2", "CH3", "CH4", "M1", "R1"];
    let (a, b) = ([1.0f32, 2.0, f32::NAN], [4.0f32, f32::INFINITY]);
    let sources: [&[f32]; 6] = [&a, &b, &a, &b, &a, &b];
    run("expr::parse", &seeds, 1, |text| {
        let text = String::from_utf8_lossy(text);
        if let Ok(e) = crate::expr::Expr::parse(&text) {
            if e.used_sources.iter().any(|&s| s >= crate::CHANNELS || !e.used[s]) {
                return Err(format!("reads the sources {:?}, the channels {:?}", e.used_sources, e.used));
            }
        }
        let Ok(e) = crate::expr::Expr::parse_sources(&text, &names) else { return Ok(()) };
        if text.len() > crate::expr::EXPR_MAX || e.used_sources.iter().any(|&s| s >= names.len()) {
            return Err(format!("took {} bytes that read the sources {:?}", text.len(), e.used_sources));
        }
        let want = e.used_sources.iter().map(|&s| sources[s].len()).min().unwrap_or(5);
        let got = e.eval_sources(&sources, -1e-3, 1e-6, 5).len();
        if got != want || !e.eval_sources(&[], 0.0, f64::NAN, 0).is_empty() {
            return Err(format!("{got} samples, its sources have {want}"));
        }
        Ok(())
    });
}

/// Four lines of a record side by side, one little-endian f32 per line and sample, eight samples a bit: bytes as a
/// UART sends them, a clock, a frame line that is low for most of every eighty samples, and data that changes.
fn traffic() -> Vec<u8> {
    let mut bits = Vec::new();
    for byte in [0x55u8, 0x00, 0xff, 0xa7, 0x13] {
        bits.push(false);
        bits.extend((0..8).map(|k| byte >> k & 1 == 1));
        bits.extend([true, true]);
    }
    let mut out = Vec::new();
    let mut lfsr = 0x5au8;
    for k in 0..bits.len() * 8 {
        if k % 8 == 0 {
            lfsr = lfsr << 1 | ((lfsr >> 6 ^ lfsr >> 5) & 1);
        }
        let lines = [bits[k / 8], k % 8 >= 4, k % 80 >= 72, lfsr & 1 == 1];
        for high in lines {
            out.extend_from_slice(&(if high { 3.3f32 } else { 0.0 }).to_le_bytes());
        }
    }
    out
}

/// The lines of an input of `traffic`'s layout.
fn lines(b: &[u8]) -> [Vec<f32>; 4] {
    let mut l: [Vec<f32>; 4] = Default::default();
    for step in b.as_chunks::<16>().0 {
        for (k, v) in step.as_chunks::<4>().0.iter().enumerate() {
            l[k].push(f32::from_le_bytes(*v));
        }
    }
    l
}

/// Records that are short or hold no numbers, and the traffic.
fn records() -> Vec<Vec<u8>> {
    let fill = |v: f32, n: usize| v.to_le_bytes().repeat(4 * n);
    vec![
        fill(3.3, 1),
        fill(f32::NAN, 40),
        fill(f32::INFINITY, 40),
        fill(f32::NEG_INFINITY, 40),
        fill(0.0, 40),
        traffic(),
    ]
}

const DT: f64 = 1e-6;
const BAUD: f64 = 125e3;
const THR: f32 = 1.65;
const HY: f32 = 0.2;

/// A decoder on the four lines of a record.
type Decoder = (&'static str, fn(&[Vec<f32>; 4]) -> Vec<Packet>);

/// Every decoder of `decode`, `linecode` and `cphy`, with settings that fit eight samples a bit.
const DECODERS: [Decoder; 30] = [
    ("uart", |l| decode::uart(&l[0], DT, BAUD, 8, Parity::None, THR, HY, false)),
    ("uart-odd-inverted", |l| decode::uart(&l[0], DT, BAUD, 7, Parity::Odd, THR, HY, true)),
    ("uart9", |l| decode::uart9(&l[0], DT, BAUD, THR, HY, false)),
    ("i2c", |l| decode::i2c(&l[0], &l[1], THR, HY)),
    ("spi", |l| decode::spi(&l[1], &l[0], Some(&l[3]), Some(&l[2]), 0, 8, true, THR, HY, 0.0)),
    ("spi-no-cs", |l| decode::spi(&l[1], &l[0], None, None, 3, 16, false, THR, HY, 20.0)),
    ("can", |l| decode::can(&l[0], DT, BAUD, THR, HY, false)),
    ("can-fd", |l| decode::can_fd(&l[0], DT, BAUD, 4.0 * BAUD, THR, HY, true)),
    ("lin", |l| decode::lin(&l[0], DT, BAUD, THR, HY)),
    ("flexray", |l| decode::flexray(&l[0], DT, BAUD, THR, HY, false)),
    ("sent", |l| decode::sent(&l[0], DT, None, 6, THR, HY)),
    ("sent-tick", |l| decode::sent(&l[0], DT, Some(3e-6), 6, THR, HY)),
    ("parallel", |l| decode::parallel(&[&l[0], &l[3]], Some(&l[1]), Slope::Rising, THR, HY)),
    ("parallel-no-clock", |l| decode::parallel(&[&l[0], &l[1], &l[2], &l[3]], None, Slope::Either, THR, HY)),
    ("qspi", |l| decode::qspi(&l[2], &l[1], &[&l[0], &l[3]], &QspiCfg::new(1, 1, 1, 3, 0, false), true, THR, HY)),
    ("qspi-qpi", |l| decode::qspi(&l[2], &l[1], &[&l[0], &l[3]], &QspiCfg::new(4, 4, 4, 4, 8, true), false, THR, HY)),
    ("i3c", |l| decode::i3c(&l[0], &l[1], &[0x30], THR, HY)),
    ("swd", |l| decode::swd(&l[1], &l[0], THR, HY)),
    ("jtag", |l| decode::jtag(&l[1], &l[2], &l[0], Some(&l[3]), None, THR, HY)),
    ("i2s", |l| decode::i2s(&l[1], Some(&l[2]), &l[0], 16, THR, HY)),
    ("spdif", |l| decode::spdif(&l[0], THR, HY)),
    ("mil1553", |l| decode::mil1553(&l[0], DT, THR, HY)),
    ("arinc429", |l| decode::arinc429(&l[0], DT, None, THR, HY)),
    ("dcc", |l| decode::dcc(&l[0], DT, THR, HY)),
    ("dmx512", |l| decode::dmx512(&l[0], DT, THR, HY, false)),
    ("modbus-rtu", |l| decode::modbus_rtu(&l[0], DT, BAUD, Parity::Even, THR, HY, false)),
    ("modbus-ascii", |l| decode::modbus_ascii(&l[0], DT, BAUD, 7, Parity::Even, THR, HY, false)),
    ("onewire", |l| decode::onewire(&l[0], DT, THR, HY, false)),
    ("usbpd", |l| decode::usbpd(&l[0], DT, THR, HY)),
    ("usb", |l| decode::usb(&l[0], Some(&l[3]), DT, UsbSpeed::Auto, THR, HY)),
];

/// The decoders outside `decode`: line codes and C-PHY.
const MORE_DECODERS: [Decoder; 4] = [
    ("manchester", |l| crate::linecode::manchester(&l[0], DT, 0.0, THR, HY)),
    ("manchester-rate", |l| crate::linecode::manchester(&l[0], DT, BAUD, THR, HY)),
    ("mlt3", |l| crate::linecode::mlt3(&l[0], DT, BAUD, THR, HY)),
    ("cphy", |l| crate::cphy::decode(&l[0], &l[1], &l[3], DT, None, 0.1)),
];

/// Every decoder (`decode::*`, `linecode::manchester`, `linecode::mlt3`, `cphy::decode`) on the samples of a record:
/// returns for any samples, also none, one, and ones that are no numbers; the times of its packets are numbers, and
/// there are not more packets than a record of that length can hold. Each decoder's own traffic, mutated, is tried
/// through the registry of oos-ui (`hostile::signals`).
#[test]
fn hostile_decoder_records() {
    let records = records();
    let seeds: Vec<&[u8]> = records.iter().map(|r| r.as_slice()).collect();
    for (name, decoder) in DECODERS.iter().chain(&MORE_DECODERS) {
        run(&format!("decode::{name}"), &seeds, 1, |b| {
            let l = lines(b);
            if qspi_trap(name, &l) && known(KNOWN, "qspi-frame-ends-before-it-starts") {
                return Ok(());
            }
            let p = decoder(&l);
            if let Some(x) = p.iter().find(|x| !(x.t0.is_finite() && x.t1.is_finite())) {
                return Err(format!("a packet from {} to {}: {:?}", x.t0, x.t1, x.kind));
            }
            if p.len() > 8 * l[0].len() + 8 {
                return Err(format!("{} packets from {} samples", p.len(), l[0].len()));
            }
            Ok(())
        });
    }
}

/// The finding `qspi-frame-ends-before-it-starts`, if this record runs into it: a CS# frame that ends at or before
/// its start (samples so far apart that both crossings land on one instant) with a clock edge at that instant.
fn qspi_trap(name: &str, l: &[Vec<f32>; 4]) -> bool {
    if !name.starts_with("qspi") {
        return false;
    }
    let (cs, sck) = (&l[2], &l[1]);
    let clk: Vec<f64> = crate::edges::edges(sck, THR, HY).iter().filter(|e| e.rising).map(|e| e.t).collect();
    let low = crate::edges::states(cs, THR, HY).iter().flatten().next() == Some(&false);
    let mut open = low.then_some(0.0);
    for e in crate::edges::edges(cs, THR, HY) {
        if !e.rising {
            open = Some(e.t);
        } else if let Some(start) = open.take() {
            if clk.iter().any(|&t| t <= start && t >= e.t) {
                return true;
            }
        }
    }
    false
}

#[test]
#[ignore = "decode::qspi panics on a CS# frame that ends where it starts with a clock edge there, #181"]
fn hostile_known_qspi_frame_ends_before_it_starts() {
    // CS# falls and rises between samples so far apart that both crossings are at sample 1; so is the clock's edge
    let (cs, sck) = ([1e30f32, 0.0, 1e17], [0.0f32, 0.0, 1e30]);
    decode::qspi(&cs, &sck, &[&cs], &QspiCfg::new(1, 1, 1, 3, 0, false), true, THR, HY);
}

/// `edges`, `trigger_edges`, `states` (the comparators under every trigger and decoder) and `trigger::find_all`
/// with every kind of trigger: return for any samples; an edge is at a time that is a number inside the record, the
/// edges come in the order of the samples, and a trigger point is a number.
#[test]
fn hostile_trigger_records() {
    let records = records();
    let seeds: Vec<&[u8]> = records.iter().map(|r| r.as_slice()).collect();
    let when = When::In(2e-6, 40e-6);
    let bits = [Bit::H, Bit::R, Bit::X, Bit::L];
    let levels = [THR; 4];
    let triggers = [
        Trigger::Edge { src: 0, slope: Slope::Either, level: THR },
        Trigger::Pulse { src: 0, pol: Pol::Neg, level: THR, when },
        Trigger::Slope { src: 0, slope: Slope::Rising, lo: 0.5, hi: 2.5, when: When::Lt(1e-3) },
        Trigger::Video { src: 0, std: crate::sim::VideoStd::Pal, sync: VideoSync::AllLines, level: THR },
        Trigger::Video { src: 3, std: crate::sim::VideoStd::Ntsc, sync: VideoSync::Line(7), level: THR },
        Trigger::Pattern { bits, levels },
        Trigger::Duration { bits, levels, when: When::Gt(1e-6) },
        Trigger::Timeout { src: 2, slope: Slope::Falling, level: THR, time: 10e-6 },
        Trigger::Runt { src: 0, pol: Pol::Pos, lo: 0.5, hi: 2.5, when: Some(When::Out(1e-6, 2e-6)) },
        Trigger::Window { src: 0, lo: 0.5, hi: 2.5, when: WinWhen::Inside(when) },
        Trigger::Window { src: 3, lo: 0.5, hi: 2.5, when: WinWhen::Exit },
        Trigger::Delay {
            a: 0,
            slope_a: Slope::Rising,
            level_a: THR,
            b: 1,
            slope_b: Slope::Falling,
            level_b: THR,
            when,
        },
        Trigger::SetupHold {
            data: 0,
            clk: 1,
            clk_slope: Slope::Rising,
            data_level: THR,
            clk_level: THR,
            setup: 2e-6,
            hold: 2e-6,
        },
        Trigger::NthEdge { src: 1, slope: Slope::Rising, level: THR, idle: 20e-6, n: 3 },
    ];
    run("trigger::find_all", &seeds, 4, |b| {
        let l = lines(b);
        let n = l[0].len() as f64;
        for (what, e) in [
            ("edges", crate::edges::edges(&l[0], THR, HY)),
            ("trigger_edges", crate::edges::trigger_edges(&l[0], THR, HY)),
        ] {
            if let Some(x) = e.iter().find(|x| !(x.t >= 0.0 && x.t <= n)) {
                return Err(format!("{what}: an edge at {} in a record of {n} samples", x.t));
            }
            if let Some(w) = e.windows(2).find(|w| w[1].t < w[0].t) {
                return Err(format!("{what}: an edge at {} after one at {}", w[1].t, w[0].t));
            }
        }
        if crate::edges::states(&l[0], THR, HY).len() != l[0].len() {
            return Err("states: not one state a sample".into());
        }
        let inp = Input { ch: [&l[0], &l[1], &l[2], &l[3]], dt: DT, hyst: [HY; 4] };
        for t in &triggers {
            if let Some(x) = find_all(t, &inp).iter().find(|x| !x.is_finite()) {
                return Err(format!("{t:?}: a trigger point at {x}"));
            }
        }
        Ok(())
    });
}
