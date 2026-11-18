// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Avionics buses: ARINC 429 (bipolar RZ, 32-bit words: label, SDI, data, SSM, parity; 12.5 or 100 kbit/s or from
//! the pulse spacing) and MIL-STD-1553 (Manchester II at 1 Mbit/s, command / status and data words with their syncs).
//! Built in until OOS 0.1.12, modules since; their keys are the names of the old setups, and a setup that kept the
//! ARINC rate in the shared row brings it along (`legacy`). Checked against generated traffic.

use super::*;
use oos_dsp::decode;
use oos_dsp::decode::Kind;

/// The ARINC 429 bit rates (`decode::ARINC_RATES`).
const ARATE_NAMES: [&str; 3] = ["Auto", "12.5 k", "100 k"];

const ARINC_OPTIONS: [Opt; 1] = [Opt { name: "Bit rate", choices: &ARATE_NAMES, default: 0 }];

/// The bit rate of the options; `None` = from the pulse spacing.
fn arinc_rate(opt: &[usize; OPTS]) -> Option<f64> {
    let r = crate::decode::ARINC_RATES[opt[0]];
    (r > 0.0).then_some(r)
}

pub struct Arinc429;

impl Protocol for Arinc429 {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::ARINC429
    }
    fn options(&self) -> &'static [Opt] {
        &ARINC_OPTIONS
    }
    /// Auto: high speed (the finer sampling).
    fn bit_time(&self, opt: &[usize; OPTS]) -> f64 {
        1.0 / arinc_rate(opt).unwrap_or(crate::decode::ARINC_RATES[2])
    }
    /// Auto: room for a low-speed word.
    fn frame_span(&self, opt: &[usize; OPTS]) -> Option<f64> {
        arinc_rate(opt).is_none().then(|| 200.0 / crate::decode::ARINC_RATES[1])
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(decode::arinc429(l.main(), l.dt, arinc_rate(&l.opt), l.thr, l.hy))
    }
    /// The label in octal.
    fn text(&self, p: &Packet, _: &[usize; OPTS]) -> Option<String> {
        matches!(p.kind, Kind::Id { .. }).then(|| format!("L {:03o}", p.value))
    }
    /// A word starts with its label.
    fn trigger_match(&self, what: BusWhat, _value: u32, p: &Packet) -> Option<bool> {
        (what == BusWhat::Frame && matches!(p.kind, Kind::Id { .. })).then_some(true)
    }
    /// Label 310 (octal) of the demo.
    fn trigger_default(&self) -> (BusWhat, u32) {
        (BusWhat::Id, 0o310)
    }
    /// A word starts with a pulse of either sign.
    fn start_edge(&self) -> Slope {
        Slope::Either
    }
    /// One line against ground (+-5 V): HI / LO at +-2.5 V.
    fn setup(&self) -> Setup {
        Setup { thr: 2.5, ..Setup::default() }
    }
    /// Three high-speed words, 8 bit times apart, +-5 V.
    fn demo(&self) -> Option<Demo> {
        use decode::gen;
        let w = [
            gen::arinc429_word(0o203, 0, 0x0_2710, 3, false),
            gen::arinc429_word(0o310, 1, 0x2_5a5a, 3, false),
            gen::arinc429_word(0o206, 0, 0x0_1388, 0, false),
        ];
        let l = gen::arinc429(&w, 100e3, 5.0, 8);
        let period = l.seg.last().map_or(1e-3, |s| s.0);
        Some(Demo { period, lines: [Some(Box::new(move |t| l.v(t))), None, None, None] })
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.arate, 0, 0, 0])
    }
}

pub struct Mil1553;

impl Protocol for Mil1553 {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::MIL1553
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        1e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(oos_dsp::decode::mil1553(l.main(), l.dt, l.thr, l.hy))
    }
    /// The command word of the demo.
    fn trigger_default(&self) -> (BusWhat, u32) {
        (BusWhat::Id, 0x0c23)
    }
    /// The sync of a word goes either way.
    fn start_edge(&self) -> Slope {
        Slope::Either
    }
    /// A bipolar bus: threshold at 0 V; the trigger on the first rising edge after 3 us of quiet bus (the command sync).
    fn setup(&self) -> Setup {
        Setup {
            thr: 0.0,
            trig: Some(DemoTrig { role: 0, slope: Slope::Rise, idle: 3e-6, level: 2.0 }),
            ..Setup::default()
        }
    }
    /// A command word and three data words, +-7 V, every 120 us.
    fn demo(&self) -> Option<Demo> {
        let l =
            oos_dsp::sim::mil1553_wave(&[(true, 0x0c23), (false, 0xbeef), (false, 0x1234), (false, 0x00ff)], 7.0, 4.0);
        Some(Demo { period: 120e-6, lines: [Some(Box::new(move |t| l.v(t))), None, None, None] })
    }
}

#[cfg(test)]
mod tests {
    // Break it catches: a setup written while ARINC 429 and 1553 were built in losing its protocol or its bit rate
    #[test]
    fn old_setups_keep_protocol_and_rate() {
        let load = |text: &str| crate::store::parse_setup(&format!("dec_on = true\n{}", text)).unwrap();
        let m = load("dec_proto = Arinc429\ndec_arate = 1\n");
        assert_eq!((m.dec_proto.name(), m.dec_opts()[0]), ("ARINC 429", 1));
        assert_eq!(m.bus_bit(), 1.0 / 12.5e3);
        let m = load("dec_proto = Mil1553\ndec_thr = 0\n");
        assert_eq!((m.dec_proto.name(), m.dec_thr, m.bus_bit()), ("1553", 0.0, 1e-6));
        assert!(m.dec_on);
    }
}
