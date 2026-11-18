// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! MIPI I3C on SDA / SCL, with legacy I2C devices and the CCCs on the same bus. Built in until OOS 0.1.12, a module
//! since; the key is the name of the old setups. Checked against generated traffic.

use super::*;
use oos_dsp::decode;

pub struct I3c;

impl Protocol for I3c {
    fn key(&self) -> &'static str {
        "I3c"
    }
    fn name(&self) -> &'static str {
        "I3C"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("SDA"), Some("SCL"), None, None]
    }
    /// A bit at 10 MHz (SDR).
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        0.1e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(decode::i3c(l.main(), l.need(1, "SCL source missing")?, &[], l.thr, l.hy))
    }
    /// SDA on CH3, SCL on CH4; on the LA the I2C lines of the demo (D13 SDA, D12 SCL).
    fn setup(&self) -> Setup {
        Setup { la: [Some(13), Some(12), None, None], ..Setup::default() }
    }
}

#[cfg(test)]
mod tests {
    // Break it catches: a setup written while I3C was built in losing its protocol
    #[test]
    fn old_setups_keep_protocol() {
        let m = crate::store::parse_setup("dec_on = true\ndec_proto = I3c\ndec_src = 1\n").unwrap();
        assert_eq!((m.dec_proto.name(), m.dec_src, m.bus_bit()), ("I3C", 1, 0.1e-6));
    }
}
