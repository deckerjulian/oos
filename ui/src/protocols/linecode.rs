// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Line codes on one line: Manchester (IEEE 802.3: a rising mid-bit edge is a 1) and MLT-3 (a change of level is a 1),
//! at the baud rate of the option row. Built in until OOS 0.1.12, modules since; their keys are the names of the old
//! setups, and a setup that kept the rate in the shared Baud row brings it along (`legacy`).

use super::*;

const OPTIONS: [Opt; 1] = [Opt { name: "Baud", choices: &BAUD_NAMES, default: BAUD_DEFAULT }];

pub struct Manchester;

impl Protocol for Manchester {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::linecode::MANCHESTER
    }
    fn options(&self) -> &'static [Opt] {
        &OPTIONS
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.baud, 0, 0, 0])
    }
}

pub struct Mlt3;

impl Protocol for Mlt3 {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::linecode::MLT3
    }
    fn options(&self) -> &'static [Opt] {
        &OPTIONS
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.baud, 0, 0, 0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a setup written while the line codes were built in losing its protocol or its baud rate
    #[test]
    fn old_setups_keep_protocol_and_baud() {
        for (key, name) in [("Manchester", "Manchester"), ("Mlt3", "MLT-3")] {
            let m = crate::store::parse_setup(&format!("dec_on = true\ndec_proto = {}\ndec_baud = 3\n", key)).unwrap();
            assert_eq!(m.dec_proto.name(), name);
            assert!(m.dec_on && m.dec_opt[0] == 3, "{} {:?}", key, m.dec_opt);
            assert_eq!(m.bus_bit(), 1.0 / 9600.0);
        }
    }

    #[test]
    fn baud_names_are_the_bauds() {
        for (n, b) in BAUD_NAMES.iter().zip(crate::decode::BAUDS) {
            assert_eq!(n.parse::<f64>().unwrap(), b);
        }
    }
}
