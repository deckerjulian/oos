// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The rear AUX OUT BNC (docs/fork/auxout.md). One FPGA output with a source multiplexer in 0x1430 bits 2:0 and a
//! pulse generator for mask/zone results configured by 0x119c. Static analysis of the FPGA image, not yet measured
//! on the unit.

use crate::dho::Write;

/// Source select of the AUX OUT pin. Bits 2:0 choose the source; the other bits do not reach the pin and power up 0.
pub const SELECT: u32 = 0x1430;
/// Pass/fail pulse configuration, see `PassFail`.
pub const PASSFAIL: u32 = 0x119c;

/// Select values (0x1430 bits 2:0). 4..7 behave like 0; 1 (a trigger status flag) is not offered.
pub mod sel {
    /// about 100 ns positive pulse per trigger; the FPGA power-up value, so OOS has always had it
    pub const TRIGGER_OUT: u32 = 0;
    /// constant low
    pub const LOW: u32 = 2;
    /// the pulse of the mask/zone result unit (`PassFail`)
    pub const PASS_FAIL: u32 = 3;
}

/// Fields of 0x119c.
pub mod pf {
    /// bits 23:0: pulse width in FPGA sample clocks
    pub const WIDTH_MASK: u32 = 0x00ff_ffff;
    /// one width count in seconds (156.25 MHz)
    pub const CLOCK_S: f64 = 6.4e-9;
    /// bit 29: result source, 1 = zone trigger, 0 = mask test
    pub const SRC_ZONE: u32 = 1 << 29;
    /// bit 30: idle level, 1 = idle high with a low pulse, 0 = idle low with a high pulse
    pub const IDLE_HIGH: u32 = 1 << 30;
    /// bit 31: event, 1 = pulse on a pass (no violation), 0 = pulse on a fail
    pub const ON_PASS: u32 = 1 << 31;
}

/// Where the pass/fail result comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// the FPGA mask comparator (0x1190..0x11dc, docs/fork/specs/06-mask-unit.md); OOS does not drive it yet
    Mask,
    /// the zone trigger result
    Zone,
}

/// Which result gives the pulse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Fail,
    Pass,
}

/// The pass/fail pulse: at most one per acquisition, when the result of `source` is `event`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassFail {
    /// pulse width in seconds, rounded to whole clocks (6.4 ns .. 107 ms)
    pub width: f64,
    pub source: Source,
    pub event: Event,
    /// idle high with a low-going pulse (false: idle low, high-going pulse)
    pub idle_high: bool,
}

impl PassFail {
    /// OOS choice: 1 us high-going pulse on a mask failure.
    pub const DEFAULT: PassFail = PassFail { width: 1e-6, source: Source::Mask, event: Event::Fail, idle_high: false };

    /// The 0x119c word. Bits 28:24 stay 0.
    pub fn word(&self) -> u32 {
        let n = (self.width / pf::CLOCK_S).round().clamp(1.0, pf::WIDTH_MASK as f64) as u32;
        let mut w = n & pf::WIDTH_MASK;
        if self.source == Source::Zone {
            w |= pf::SRC_ZONE;
        }
        if self.idle_high {
            w |= pf::IDLE_HIGH;
        }
        if self.event == Event::Pass {
            w |= pf::ON_PASS;
        }
        w
    }
}

/// What the AUX OUT BNC carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuxOut {
    /// a pulse per trigger (power-up state)
    TriggerOut,
    /// constant low
    Off,
    /// the pass/fail pulse; needs the hardware mask unit (or the zone trigger) to produce results
    PassFail(PassFail),
}

impl Default for AuxOut {
    fn default() -> AuxOut {
        AuxOut::TriggerOut
    }
}

/// The writes that put `a` on the BNC: the pulse configuration before the select, so the output never runs on the
/// previous configuration.
pub fn writes(a: &AuxOut) -> Vec<Write> {
    match a {
        AuxOut::TriggerOut => vec![(SELECT, sel::TRIGGER_OUT)],
        AuxOut::Off => vec![(SELECT, sel::LOW)],
        AuxOut::PassFail(p) => vec![(PASSFAIL, p.word()), (SELECT, sel::PASS_FAIL)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_values() {
        assert_eq!(writes(&AuxOut::TriggerOut), vec![(0x1430, 0)]);
        assert_eq!(writes(&AuxOut::Off), vec![(0x1430, 2)]);
        assert_eq!(writes(&AuxOut::default()), writes(&AuxOut::TriggerOut));
    }

    #[test]
    fn pass_fail_configures_before_it_selects() {
        let w = writes(&AuxOut::PassFail(PassFail::DEFAULT));
        assert_eq!(w, vec![(0x119c, 156), (0x1430, 3)]);
    }

    #[test]
    fn pass_fail_word_fields() {
        let p = PassFail::DEFAULT;
        // 1 us = 156.25 clocks
        assert_eq!(p.word(), 156);
        assert_eq!(PassFail { idle_high: true, ..p }.word(), 156 | 1 << 30);
        assert_eq!(PassFail { event: Event::Pass, ..p }.word(), 156 | 1 << 31);
        assert_eq!(PassFail { source: Source::Zone, ..p }.word(), 156 | 1 << 29);
        // the zone trigger's own set-up: 68 clocks, zone, idle high, on fail
        assert_eq!(PassFail { width: 68.0 * 6.4e-9, source: Source::Zone, event: Event::Fail, idle_high: true }.word(), 0x6000_0044);
        let all = PassFail { width: 1.0, source: Source::Zone, event: Event::Pass, idle_high: true };
        assert_eq!(all.word(), 0xe0ff_ffff);
    }

    #[test]
    fn width_is_clamped_to_the_counter() {
        let p = PassFail::DEFAULT;
        assert_eq!(PassFail { width: 0.0, ..p }.word(), 1);
        assert_eq!(PassFail { width: -1.0, ..p }.word(), 1);
        assert_eq!(PassFail { width: 10.0, ..p }.word(), 0x00ff_ffff);
        assert_eq!(PassFail { width: 6.4e-9, ..p }.word(), 1);
    }
}
