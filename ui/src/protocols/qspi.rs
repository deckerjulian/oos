// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Quad-SPI flash: CS#, SCK, IO0 and IO1; a single-line opcode, then the opcode decides the lines and the address of
//! what follows. Built in until OOS 0.1.12, a module since; the key is the name of the old setups. Checked against
//! generated traffic.

use super::*;
use oos_dsp::decode;

pub struct Qspi;

impl Protocol for Qspi {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::QSPI
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        0.5e-6
    }
    /// Single-line opcode and three address bytes to start with; the opcode switches the lines from there.
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        let cs = l.need(2, "CS# source missing")?;
        let mut io = vec![l.main()];
        if let Some(c) = l.lines[3] {
            io.push(c);
        }
        Ok(decode::qspi(
            cs,
            l.need(1, "SCK source missing")?,
            &io,
            &decode::QspiCfg::new(1, 1, 1, 3, 0, false),
            true,
            l.thr,
            l.hy,
        ))
    }
    /// IO0, SCK, CS#, IO1 on CH1 to CH4.
    fn setup(&self) -> Setup {
        Setup { src: [Some(0), Some(1), Some(2), Some(3)], ..Setup::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a setup written while Quad-SPI was built in losing its protocol or its sources
    #[test]
    fn old_setups_keep_protocol_and_sources() {
        let m = crate::store::parse_setup(
            "dec_on = true\ndec_proto = Qspi\ndec_src = 0\ndec_src2 = 1\ndec_cs = 2\ndec_src4 = 4\n",
        )
        .unwrap();
        assert_eq!(
            (m.dec_proto.name(), m.dec_src, m.dec_src2, m.dec_cs, m.dec_src4),
            ("Quad-SPI", 0, 1, Some(2), Some(3))
        );
    }

    /// Without CS# the decoder says what is missing.
    #[test]
    fn needs_cs() {
        let v = vec![0.0f32; 100];
        let l = Lines {
            lines: [Some(&v), Some(&v), None, None],
            dt: 1e-8,
            thr: 1.65,
            hy: 0.1,
            opt: [0; OPTS],
            sh: Shared::default(),
            chans: [None; 4],
            src: [None; 4],
        };
        assert_eq!(Qspi.decode(&l).unwrap_err(), "CS# source missing");
    }
}
