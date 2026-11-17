// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! 1-Wire, directly on the data line (reset / presence, time slots, ROM commands, standard speed), and through a
//! DS2482 bridge: the I2C commands of the bridge (SDA / SCL) read as the 1-Wire operations they start. Built in until
//! OOS 0.1.12, modules since; their keys are the names of the old setups. Checked against generated traffic.

use super::*;
use oos_dsp::decode_ext as x1;

pub struct OneWire;

impl Protocol for OneWire {
    fn key(&self) -> &'static str {
        "OneWire"
    }
    fn name(&self) -> &'static str {
        "1-Wire"
    }
    /// The shortest standard-speed slot.
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        15e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(x1::onewire(l.main(), l.dt, l.thr, l.hy, false))
    }
    /// Reset with presence, Read ROM, and the ROM of a DS18B20 (family 28) as far as the third byte.
    fn demo(&self) -> Option<Demo> {
        use x1::gen::Ow;
        let ow = [Ow::Reset { presence: true }, Ow::Write(0x33), Ow::Read(0x28), Ow::Read(0xFF), Ow::Read(0x4C), Ow::Read(0x3D)];
        let a = x1::gen::onewire_logic(&ow, 0.0, 3.3);
        let period = a.seg.last().map_or(1e-3, |s| s.0) + 200e-6;
        Some(Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] })
    }
}

pub struct Ds2482;

impl Protocol for Ds2482 {
    fn key(&self) -> &'static str {
        "Ds2482"
    }
    fn name(&self) -> &'static str {
        "DS2482"
    }
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("SDA"), Some("SCL"), None, None]
    }
    /// An I2C bit at 400 kHz.
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        2.5e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        let scl = l.need(1, "SCL source missing")?;
        Ok(x1::ds2482(&oos_dsp::decode::i2c(l.main(), scl, l.thr, l.hy)))
    }
    /// SDA on CH3, SCL on CH4; on the LA the I2C lines of the demo (D13 SDA, D12 SCL).
    fn setup(&self) -> Setup {
        Setup { la: [Some(13), Some(12), None, None], ..Setup::default() }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Proto;

    // Break it catches: a setup written while 1-Wire and the DS2482 were built in losing its protocol
    #[test]
    fn old_setups_keep_protocol() {
        for (key, name, roles) in [("OneWire", "1-Wire", 1), ("Ds2482", "DS2482", 2)] {
            let m = crate::store::parse_setup(&format!("dec_on = true\ndec_proto = {}\ndec_src = 1\n", key)).unwrap();
            assert_eq!((m.dec_proto.name(), m.dec_src), (name, 1));
            assert!(m.dec_on && Proto::from_key(key) == Some(m.dec_proto));
            assert_eq!(m.dec_proto.roles().iter().flatten().count(), roles);
        }
    }
}
