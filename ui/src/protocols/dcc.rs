// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! NMRA DCC (S-9.2), the digital command signal of model railways: the decoder of `oos_dsp::decode_ext3::dcc` and a
//! view of the layout it controls (`oos_dsp::dcc_yard`): locomotives with direction, speed and functions, turnouts,
//! signals, and whether the track is live or in emergency stop. The view collects over the records until the display
//! is cleared.
//!
//! Decoder, layout model and demo traffic contributed by Arjen Halma (pull request #3). Checked against generated
//! packets only, not yet on a track. On a track the signal is bipolar between the rails: two probes, "B line (A-B)"
//! and a threshold of 0 V (the demo is one bipolar line, +-3 V).

use super::*;
use oos_dsp::dcc_yard::{functions_text, TrackStatus, Yard};
use oos_dsp::decode::Kind;
use oos_dsp::decode_ext3 as x3;

pub struct Dcc;

impl Protocol for Dcc {
    fn key(&self) -> &'static str {
        "Dcc"
    }
    fn name(&self) -> &'static str {
        "DCC"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("Rail"), None, None, None]
    }
    /// The two rails as a pair.
    fn diff_ok(&self) -> bool {
        true
    }
    /// A `1` bit: two halves of 58 us.
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        2.0 * x3::DCC_ONE_HALF
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(x3::dcc(l.main(), l.dt, l.thr, l.hy))
    }
    fn text(&self, p: &Packet, _: &[usize; OPTS]) -> Option<String> {
        Some(match p.kind {
            Kind::Field("PRE") => format!("PRE {}", p.value),
            // the first address byte says what the packet is for
            Kind::Id { .. } => match p.value {
                0 => "Broadcast".into(),
                255 => "Idle".into(),
                v @ 1..=127 => format!("Loco {}", v),
                128..=191 => format!("Acc {:02X}", p.value),
                v @ 192..=231 => format!("Long {:02X}", v),
                v @ 253..=254 => format!("Adv {}", v),
                v => format!("Addr {:02X}", v),
            },
            Kind::Crc => format!("XOR {:02X}{}", p.value, if p.ok { "" } else { " !" }),
            _ => return None,
        })
    }
    /// One row per packet: what it tells which device ("Loco 5  Fwd 50/128", "Turnout 3  Left", "Idle").
    fn events(&self, p: &[Packet], _: &[usize; OPTS]) -> Option<Vec<(f64, String, bool)>> {
        let mut v = Vec::new();
        let mut cur: Option<(f64, u8, Vec<u8>)> = None;
        for x in p {
            match x.kind {
                Kind::Id { .. } => cur = Some((x.t0, x.value as u8, Vec::new())),
                Kind::Data => {
                    if let Some(c) = cur.as_mut() {
                        c.2.push(x.value as u8);
                    }
                }
                Kind::Crc => {
                    if let Some((t, a, d)) = cur.take() {
                        v.push((t, if x.ok { describe(a, &d) } else { format!("{}  XOR wrong", describe(a, &d)) }, x.ok));
                    }
                }
                Kind::Field("!ERR") => v.push((x.t0, "Broken packet".into(), false)),
                _ => {}
            }
        }
        Some(v)
    }
    /// Locomotive 3 of the demo traffic.
    fn trigger_default(&self) -> (BusWhat, u32) {
        (BusWhat::Id, 3)
    }
    /// The signal never idles: every edge may start a packet.
    fn start_edge(&self) -> Slope {
        Slope::Either
    }
    fn setup(&self) -> Setup {
        Setup { src: [Some(2), None, None, None], thr: 0.0, ..Setup::default() }
    }
    /// Five locomotives running (light and horn on number 3), five turnouts, five signals, idle packets between.
    fn demo(&self) -> Option<Demo> {
        let fr = |address: u8, data: Vec<u8>| x3::gen::DccFrame { address, data, xor: None, preamble: 12 };
        // 128-step speed byte: direction in bit 7, speed + 1 below (0 = stop, 1 = emergency stop)
        let speed = |fwd: bool, s: u8| if fwd { 0x80 | (s + 1) } else { s + 1 };
        // accessory n (1..) on board (n - 1) / 4 + 1, output pair (n - 1) % 4
        let acc = |n: u16| (0x80 | ((n - 1) / 4) as u8 + 1, ((n - 1) % 4) as u8);
        let mut v = Vec::new();
        for (addr, fwd, s) in [(1u8, true, 20u8), (2, true, 35), (3, true, 40), (4, false, 25), (5, true, 50)] {
            v.push(fr(0xFF, vec![0x00]));
            v.push(fr(addr, vec![0x3F, speed(fwd, s)]));
        }
        // function group one: FL (light) and F2 (horn)
        v.push(fr(3, vec![0b100_10010]));
        // basic accessory: activate, odd turnouts to the first output ("left")
        for n in 1u16..=5 {
            let (a1, pair) = acc(n);
            v.push(fr(a1, vec![0xF0 | 0x08 | pair << 1 | u8::from(n % 2 == 0)]));
        }
        // extended accessory: second byte 0AAA0AA1, then the aspect
        for (n, aspect) in [(1u16, 0x24u8), (2, 0x12), (3, 0x08), (4, 0x30), (5, 0x04)] {
            let (a1, pair) = acc(n);
            v.push(fr(a1, vec![0x70 | pair << 1 | 1, aspect]));
        }
        v.push(fr(0xFF, vec![0x00]));
        let l = x3::gen::dcc(&v, -3.0, 3.0);
        // a short pause at the end (a gap the decoder must survive) that also keeps the period from being a whole
        // fraction of the screen's refresh: successive records then see different parts of the traffic
        let period = l.seg.last().map_or(1e-3, |s| s.0) + 3.7e-3;
        Some(Demo { period, lines: [Some(Box::new(move |t| l.v(t))), None, None, None] })
    }
    fn view(&self, p: &[Packet], _: &[usize; OPTS], mem: &mut Memory) -> Option<View> {
        let yard: &mut Yard = memory(mem);
        apply_packets(yard, p);
        let tone = match yard.status {
            TrackStatus::Active => Tone::Good,
            TrackStatus::Estop => Tone::Bad,
            TrackStatus::Off => Tone::Neutral,
        };
        let status = format!("{} \u{b7} {} idle, {} broadcast", yard.status.label(), yard.idle_count, yard.broadcast_count);
        let mut sections = vec![
            Section {
                title: format!("Locomotives ({})", yard.locos.len()),
                rows: yard.locos.iter().map(|l| format!("{}  {} {}  {}", l.addr, l.dir_text(), l.speed_text(), functions_text(l.functions)).trim_end().to_string()).collect(),
            },
            Section { title: format!("Turnouts ({})", yard.turnouts.len()), rows: yard.turnouts.iter().map(|t| format!("{}  {}", t.addr, t.side_text())).collect() },
            Section { title: format!("Signals ({})", yard.signals.len()), rows: yard.signals.iter().map(|s| format!("{}  {}", s.addr, s.aspect_text())).collect() },
        ];
        if !yard.advanced.is_empty() {
            sections.push(Section {
                title: "Advanced".into(),
                rows: yard.advanced.iter().map(|a| format!("{}  {}", a.kind, a.data.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" "))).collect(),
            });
        }
        Some(View { status: Some((status, tone)), sections })
    }
}

/// What a packet says, in words: address first, then the instruction.
fn describe(addr: u8, data: &[u8]) -> String {
    match addr {
        255 => "Idle".into(),
        0 => format!("All  {}", instruction(data)),
        1..=127 => format!("Loco {}  {}", addr, instruction(data)),
        192..=231 if !data.is_empty() => format!("Loco {}  {}", ((addr as u16 & 0x3f) << 8) | data[0] as u16, instruction(&data[1..])),
        128..=191 => {
            let y = Yard::default().apply(addr, data);
            if let Some(t) = y.turnouts.first() {
                format!("Turnout {}  {}{}", t.addr, t.side_text(), if t.active { "" } else { " (off)" })
            } else if let Some(s) = y.signals.first() {
                format!("Signal {}  {}", s.addr, s.aspect_text())
            } else {
                format!("Accessory {:02X}  {}", addr, hex(data))
            }
        }
        253 | 254 => format!("Advanced {}  {}", addr, hex(data)),
        _ => format!("Addr {:02X}  {}", addr, hex(data)),
    }
}

/// A multi-function instruction: speed and direction, a function group, else its bytes.
fn instruction(data: &[u8]) -> String {
    let Some(&b) = data.first() else { return String::new() };
    let y = Yard::default().apply(3, data);
    let l = y.locos.first();
    let fns = |lo: u32, hi: u32| {
        let f = l.map_or(0, |l| l.functions);
        let on: Vec<String> = (lo..=hi).filter(|n| f >> n & 1 == 1).map(|n| match oos_dsp::dcc_yard::function_label(n as u8) {
            "" => format!("F{}", n),
            s => s.to_string(),
        }).collect();
        format!("F{}-F{}: {}", lo, hi, if on.is_empty() { "off".to_string() } else { on.join(" ") })
    };
    match b >> 5 {
        0b001 if b & 0x1f == 0x1f => l.map_or_else(|| hex(data), |l| format!("{} {}", l.dir_text(), l.speed_text())).replace("E-Stop E-Stop", "E-Stop"),
        0b010 | 0b011 => l.map_or_else(|| hex(data), |l| format!("{} {}", l.dir_text(), l.speed_text())).replace("E-Stop E-Stop", "E-Stop"),
        0b100 => fns(0, 4),
        0b101 if b & 0x10 != 0 => fns(5, 8),
        0b101 => fns(9, 12),
        0b110 if b == 0xde => fns(13, 20),
        0b110 if b == 0xdf => fns(21, 28),
        _ => hex(data),
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

/// Applies the complete packets (address, data ..., XOR that matches) in time order; broken packets are left out.
fn apply_packets(yard: &mut Yard, p: &[Packet]) {
    let mut addr: Option<u8> = None;
    let mut data = Vec::new();
    for x in p {
        match x.kind {
            Kind::Id { .. } => {
                addr = Some(x.value as u8);
                data.clear();
            }
            Kind::Data if addr.is_some() => data.push(x.value as u8),
            Kind::Crc => {
                if let (Some(a), true) = (addr.take(), x.ok) {
                    *yard = yard.apply(a, &data);
                }
                data.clear();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The demo traffic gives the whole layout: five locomotives (number 3 with light and horn), turnouts and signals,
    /// track active; a broadcast emergency stop turns the status red.
    #[test]
    fn demo_layout_and_emergency_stop() {
        let d = Dcc.demo().unwrap();
        let dt = x3::DCC_ONE_HALF / 10.0;
        let v: Vec<f32> = (0..(1.2 * d.period / dt) as usize).map(|k| d.lines[0].as_ref().unwrap()(k as f64 * dt) as f32).collect();
        let p = Dcc.decode(&Lines { lines: [Some(&v), None, None, None], dt, thr: 0.0, hy: 0.3, opt: [0; OPTS] }).unwrap();
        let mut mem = None;
        let view = Dcc.view(&p, &[0; OPTS], &mut mem).unwrap();
        assert_eq!(view.status.as_ref().unwrap().1, Tone::Good, "{:?}", view);
        assert_eq!(view.sections[0].rows.len(), 5, "{:?}", view.sections[0]);
        assert!(view.sections[0].rows[2].starts_with("3  Fwd 40/128  Light Horn"), "{:?}", view.sections[0].rows);
        assert_eq!((view.sections[1].rows.len(), view.sections[2].rows.len()), (5, 5));
        assert_eq!(view.sections[1].rows[0], "1  Left");
        // the memory keeps the layout; an emergency stop to all (broadcast, 128 steps: speed byte 1) turns it red
        let estop = [
            Packet { t0: 0.0, t1: 1.0, kind: Kind::Id { ext: false, rtr: false }, value: 0, value2: None, ok: true },
            Packet { t0: 1.0, t1: 2.0, kind: Kind::Data, value: 0x3F, value2: None, ok: true },
            Packet { t0: 2.0, t1: 3.0, kind: Kind::Data, value: 0x01, value2: None, ok: true },
            Packet { t0: 3.0, t1: 4.0, kind: Kind::Crc, value: 0x3E, value2: None, ok: true },
        ];
        let view = Dcc.view(&estop, &[0; OPTS], &mut mem).unwrap();
        assert_eq!(view.status.as_ref().unwrap().1, Tone::Bad, "{:?}", view);
        assert!(view.sections[0].rows.iter().all(|r| r.contains("E-Stop")), "{:?}", view.sections[0].rows);
        // a packet with a wrong XOR changes nothing
        let mut bad = estop.to_vec();
        bad[3].ok = false;
        let mut fresh = None;
        assert_eq!(Dcc.view(&bad, &[0; OPTS], &mut fresh).unwrap().sections[0].rows.len(), 0);
    }

    /// The event table: a row per command, in words.
    #[test]
    fn a_row_per_command() {
        assert_eq!(describe(5, &[0x3f, 0x80 | 51]), "Loco 5  Fwd 50/128");
        assert_eq!(describe(4, &[0x3f, 26]), "Loco 4  Rev 25/128");
        assert_eq!(describe(3, &[0b011_00010]), "Loco 3  Fwd 1/28");
        assert_eq!(describe(3, &[0b100_10010]), "Loco 3  F0-F4: Light Horn");
        assert_eq!(describe(0, &[0x3f, 0x01]), "All  E-Stop");
        assert_eq!(describe(255, &[0]), "Idle");
        assert_eq!(describe(0xc1, &[0x02, 0x3f, 0x85]), "Loco 258  Fwd 4/128");
        assert_eq!(describe(0x81, &[0xf8]), "Turnout 1  Left");
        assert_eq!(describe(0x81, &[0x71, 0x24]), "Signal 1  Aspect 24");
        let id = |t: f64, v| Packet { t0: t, t1: t, kind: Kind::Id { ext: false, rtr: false }, value: v, value2: None, ok: true };
        let d = |t: f64, v| Packet { t0: t, t1: t, kind: Kind::Data, value: v, value2: None, ok: true };
        let x = |t: f64, ok| Packet { t0: t, t1: t, kind: Kind::Crc, value: 0, value2: None, ok };
        let p = [id(1.0, 5), d(2.0, 0x3f), d(3.0, 0x80 | 51), x(4.0, true), id(5.0, 255), d(6.0, 0), x(7.0, false)];
        assert_eq!(Dcc.events(&p, &[0; OPTS]).unwrap(), vec![(1.0, "Loco 5  Fwd 50/128".to_string(), true), (5.0, "Idle  XOR wrong".to_string(), false)]);
    }

    #[test]
    fn labels() {
        let pk = |kind, value| Packet { t0: 0.0, t1: 1.0, kind, value, value2: None, ok: true };
        let id = |v| pk(Kind::Id { ext: false, rtr: false }, v);
        let t = |p: Packet| Dcc.text(&p, &[0; OPTS]).unwrap();
        assert_eq!(t(id(3)), "Loco 3");
        assert_eq!(t(id(255)), "Idle");
        assert_eq!(t(id(0)), "Broadcast");
        assert_eq!(t(pk(Kind::Field("PRE"), 14)), "PRE 14");
        assert_eq!(t(pk(Kind::Crc, 0x3c)), "XOR 3C");
    }
}
