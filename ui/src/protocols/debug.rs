// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Debug ports: ARM SWD (SWDIO / SWCLK: line reset, JTAG-to-SWD, request, ACK, data, parity) and JTAG (TMS / TCK /
//! TDI / TDO: the TAP state machine, IR and DR scans). Built in until OOS 0.1.12, modules since; their keys are the
//! names of the old setups. Checked against generated traffic.

use super::*;
use oos_dsp::decode;
use oos_dsp::decode::Kind;

pub struct Swd;

impl Protocol for Swd {
    fn key(&self) -> &'static str {
        "Swd"
    }
    fn name(&self) -> &'static str {
        "SWD"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("SWDIO"), Some("SWCLK"), None, None]
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        0.5e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(decode::swd(l.need(1, "SWCLK source missing")?, l.main(), l.thr, l.hy))
    }
    /// The ACK of a transfer is a field with its three bits, not an I2C style acknowledge.
    fn text(&self, p: &Packet, _: &[usize; OPTS]) -> Option<String> {
        (p.kind == Kind::Ack).then(|| format!("ACK {}", p.value))
    }
    /// SWDIO on CH3, SWCLK on CH4 at 1 MHz: line reset, JTAG-to-SWD, line reset, a read of the DP IDCODE.
    fn demo(&self) -> Option<Demo> {
        use decode::gen::SwdOp;
        let ops = [
            SwdOp::LineReset,
            SwdOp::JtagToSwd,
            SwdOp::LineReset,
            SwdOp::Idle(2),
            SwdOp::Xfer { ap: false, read: true, a: 0, ack: 1, data: 0x2BA0_1477, bad_parity: false },
            SwdOp::Idle(4),
        ];
        let (clk, dio) = decode::gen::swd(&ops, 1e6, 0.0, 3.3);
        let end = |l: &oos_dsp::sim::Logic| l.seg.last().map_or(1e-3, |s| s.0);
        let period = end(&dio).max(end(&clk)) + 20e-6;
        Some(Demo { period, lines: [Some(Box::new(move |t| dio.v(t))), Some(Box::new(move |t| clk.v(t))), None, None] })
    }
}

pub struct Jtag;

impl Protocol for Jtag {
    fn key(&self) -> &'static str {
        "Jtag"
    }
    fn name(&self) -> &'static str {
        "JTAG"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("TMS"), Some("TCK"), Some("TDI"), Some("TDO")]
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        0.5e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        let tdi = l.need(2, "TDI source missing")?;
        Ok(decode::jtag(l.need(1, "TCK source missing")?, l.main(), tdi, l.lines[3], None, l.thr, l.hy))
    }
    /// TMS, TCK, TDI, TDO on CH1 to CH4.
    fn setup(&self) -> Setup {
        Setup { src: [Some(0), Some(1), Some(2), Some(3)], ..Setup::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a setup written while SWD and JTAG were built in losing its protocol or its sources
    #[test]
    fn old_setups_keep_protocol_and_sources() {
        let m = crate::store::parse_setup(
            "dec_on = true\ndec_proto = Jtag\ndec_src = 0\ndec_src2 = 1\ndec_cs = 2\ndec_src4 = 4\n",
        )
        .unwrap();
        assert_eq!((m.dec_proto.name(), m.dec_src, m.dec_src2, m.dec_cs, m.dec_src4), ("JTAG", 0, 1, Some(2), Some(3)));
        let m = crate::store::parse_setup("dec_on = true\ndec_proto = Swd\n").unwrap();
        assert_eq!(m.dec_proto.name(), "SWD");
        assert_eq!(m.bus_bit(), 0.5e-6);
    }

    /// The SWD demo decodes to the IDCODE read with an OK acknowledge, shown as a field.
    #[test]
    fn swd_demo_reads_the_idcode() {
        let d = Swd.demo().unwrap();
        let dt = 10e-9;
        let n = (d.period / dt) as usize;
        let wave =
            |k: usize| -> Vec<f32> { (0..n).map(|i| d.lines[k].as_ref().unwrap()(i as f64 * dt) as f32).collect() };
        let (dio, clk) = (wave(0), wave(1));
        let p = Swd
            .decode(&Lines {
                lines: [Some(&dio), Some(&clk), None, None],
                dt,
                thr: 1.65,
                hy: 0.1,
                opt: [0; OPTS],
                sh: Shared::default(),
                chans: [None; 4],
                src: [None; 4],
            })
            .unwrap();
        assert!(p.iter().any(|x| x.kind == Kind::Data && x.value == 0x2BA0_1477), "{:?}", p);
        let ack = p.iter().find(|x| x.kind == Kind::Ack).unwrap();
        assert_eq!(Swd.text(ack, &[0; OPTS]).as_deref(), Some("ACK 1"));
    }
}
