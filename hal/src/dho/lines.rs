// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The single lines of the board that the acquisition drives or reads, beside the register window, the stream and the
//! front-end bus (ADR 0053, docs/board-on-a-host.md 4): the gear of each front end and the logic probe's detect line.
//! The executor, the self-calibration and the loop take them as `Lines`, so a host run gives them its own; on the unit
//! they are `UnitLines`, which does what `afe::set_gear` and `la::probe_present` do (afe-settings.md, la.md). The
//! generator's relays and power are its own lines (`afg::GenLines`): they are opened only while the generator runs.

/// The board's single lines, as the acquisition uses them.
pub trait Lines {
    /// Sets the gear of front end `ch` (`afe::Op::Gear`).
    fn gear(&mut self, ch: usize, gear: u8) -> std::io::Result<()>;
    /// Whether the logic probe is plugged in.
    fn la_probe(&mut self) -> std::io::Result<bool>;
}

/// The unit's lines: the gear ioctl (or its GPIO line) and the probe's GPIO line.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default)]
pub struct UnitLines;

#[cfg(target_os = "linux")]
impl Lines for UnitLines {
    fn gear(&mut self, ch: usize, gear: u8) -> std::io::Result<()> {
        crate::dho::afe::set_gear(ch, gear)
    }
    fn la_probe(&mut self) -> std::io::Result<bool> {
        crate::dho::la::probe_present()
    }
}

/// Lines that only remember: every gear set, in order, and the probe answer a test gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorded {
    /// (channel, gear) of every gear set, in order
    pub gears: Vec<(usize, u8)>,
    pub probe: bool,
}

impl Lines for Recorded {
    fn gear(&mut self, ch: usize, gear: u8) -> std::io::Result<()> {
        self.gears.push((ch, gear));
        Ok(())
    }
    fn la_probe(&mut self) -> std::io::Result<bool> {
        Ok(self.probe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: recorded lines that lose the order of the gears or answer the probe wrongly
    #[test]
    fn recorded_lines_keep_every_gear_in_order() {
        let mut l = Recorded { probe: true, ..Recorded::default() };
        for (ch, g) in [(0, 1), (2, 0), (0, 0)] {
            l.gear(ch, g).unwrap();
        }
        assert_eq!(l.gears, [(0, 1), (2, 0), (0, 0)]);
        assert!(l.la_probe().unwrap());
    }
}
