// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Vehicle buses: CAN FD (arbitration at the CAN rate, data phase at the FD rate after BRS), FlexRay at 10 Mbit/s on
//! RxD and SENT (SAE J2716: sync pulse, status and data nibbles, CRC). Built in until OOS 0.1.12, modules since; their
//! keys are the names of the old setups, and a setup that kept the rates in the shared rows brings them along
//! (`legacy`). Checked against generated traffic.

use super::*;
use oos_dsp::decode;
use oos_dsp::decode::Kind;

/// Demo traffic on one line: `a` and then `idle` seconds.
fn one_line(a: oos_dsp::sim::Logic, idle: f64) -> Demo {
    let period = a.seg.last().map_or(1e-3, |s| s.0) + idle;
    Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] }
}

/// The CAN bit rates (`decode::CAN_RATES`).
const CAN_RATE_NAMES: [&str; 5] = ["125 kbit/s", "250 kbit/s", "500 kbit/s", "800 kbit/s", "1 Mbit/s"];
/// The data phase rates (`decode::FD_RATES`).
const FD_RATE_NAMES: [&str; 5] = ["1 M", "2 M", "4 M", "5 M", "8 M"];

const CANFD_OPTIONS: [Opt; 3] = [
    Opt { name: "Bit rate", choices: &CAN_RATE_NAMES, default: 2 },
    Opt { name: "Data rate", choices: &FD_RATE_NAMES, default: 1 },
    Opt { name: "Signal", choices: &["RX/TX", "CAN_H"], default: 0 },
];

pub struct CanFd;

impl Protocol for CanFd {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::CAN_FD
    }
    fn options(&self) -> &'static [Opt] {
        &CANFD_OPTIONS
    }
    /// A CAN FD frame with ID 123, bit rate switch and 12 bytes, 500 kbit/s and 2 Mbit/s.
    fn demo(&self) -> Option<Demo> {
        let fd =
            decode::gen::FdFrame { id: 0x123, ext: false, fd: true, brs: true, esi: false, data: (1..=12).collect() };
        Some(one_line(decode::gen::can_fd_logic(&[fd], 500e3, 2e6, 0.0, 3.3), 40e-6))
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.canrate, old.fdrate, usize::from(old.canh), 0])
    }
}

pub struct FlexRay;

impl Protocol for FlexRay {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::FLEXRAY
    }
    /// A static frame (ID 2A, cycle 5) with 8 payload bytes on RxD.
    fn demo(&self) -> Option<Demo> {
        let fr = decode::gen::FrFrame {
            ppi: false,
            nf: true,
            sync: false,
            startup: false,
            id: 0x2a,
            cycle: 5,
            payload: vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
            channel_b: false,
        };
        Some(one_line(oos_dsp::sim::bits_logic(&decode::gen::flexray_bits(&fr, 11), 1e-7, 20, 0.0, 3.3), 10e-6))
    }
}

/// The SENT clock ticks (`decode::SENT_TICKS`; the last one: any tick, from the sync pulses).
const TICK_NAMES: [&str; 8] =
    ["3 \u{b5}s", "5 \u{b5}s", "10 \u{b5}s", "20 \u{b5}s", "30 \u{b5}s", "50 \u{b5}s", "90 \u{b5}s", "Auto"];

const SENT_OPTIONS: [Opt; 2] = [
    Opt { name: "Clock tick", choices: &TICK_NAMES, default: 0 },
    Opt { name: "Data nibbles", choices: &["1", "2", "3", "4", "5", "6"], default: 5 },
];

pub struct Sent;

impl Protocol for Sent {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::SENT
    }
    fn options(&self) -> &'static [Opt] {
        &SENT_OPTIONS
    }
    /// The tick the sync pulse gave; the data as its nibbles.
    fn text(&self, p: &Packet, _: &[usize; OPTS]) -> Option<String> {
        match p.kind {
            Kind::Field("SYNC") => {
                Some(format!("SYNC {}{}", crate::model::si(p.value as f64 * 1e-9, "s"), if p.ok { "" } else { " !" }))
            }
            Kind::Data => Some(format!("{:0w$X}", p.value, w = p.value2.unwrap_or(1) as usize)),
            _ => None,
        }
    }
    /// 5 V logic.
    fn setup(&self) -> Setup {
        Setup { thr: 2.5, ..Setup::default() }
    }
    /// Two fast-channel frames of 6 data nibbles with a pause pulse, 3 us tick, 5 V.
    fn demo(&self) -> Option<Demo> {
        let sf =
            |status: u8, data: [u8; 6]| decode::gen::SentFrame { status, data: data.to_vec(), crc: None, pause: 40 };
        Some(one_line(
            decode::gen::sent(
                &[sf(0x0, [0x7, 0xf, 0x3, 0x8, 0x0, 0xc]), sf(0x4, [0x7, 0xf, 0x4, 0x8, 0x0, 0xb])],
                3e-6,
                0.0,
                5.0,
            ),
            0.0,
        ))
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.tick, old.nibbles.clamp(1, 6) as usize - 1, 0, 0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a setup written while CAN FD, FlexRay and SENT were built in losing its protocol or rates
    #[test]
    fn old_setups_keep_protocol_and_options() {
        let load = |text: &str| crate::store::parse_setup(&format!("dec_on = true\n{}", text)).unwrap();
        let m = load("dec_proto = CanFd\ndec_canrate = 4\ndec_fdrate = 3\ndec_canh = true\n");
        assert_eq!((m.dec_proto.name(), m.dec_opts()[..3].to_vec()), ("CAN-FD", vec![4, 3, 1]));
        assert_eq!(m.bus_bit(), 1e-6);
        let m = load("dec_proto = FlexRay\n");
        assert_eq!((m.dec_proto.name(), m.bus_bit()), ("FlexRay", 0.1e-6));
        let m = load("dec_proto = Sent\ndec_tick = 2\ndec_nibbles = 4\n");
        assert_eq!((m.dec_proto.name(), m.dec_opts()[..2].to_vec()), ("SENT", vec![2, 3]));
        assert_eq!(m.bus_bit(), 40e-6);
        assert!(m.dec_on);
    }

    #[test]
    fn rate_names_are_the_rates() {
        for (n, r) in TICK_NAMES.iter().zip(crate::decode::SENT_TICKS) {
            assert_eq!(*n, if r > 0.0 { crate::model::si_short(r, "s") } else { "Auto".into() });
        }
        for (n, r) in CAN_RATE_NAMES.iter().zip(crate::decode::CAN_RATES) {
            assert_eq!(
                n.split(' ').next().unwrap().parse::<f64>().unwrap() * if n.contains('M') { 1e6 } else { 1e3 },
                r
            );
        }
        for (n, r) in FD_RATE_NAMES.iter().zip(crate::decode::FD_RATES) {
            assert_eq!(n.split(' ').next().unwrap().parse::<f64>().unwrap() * 1e6, r);
        }
    }
}
