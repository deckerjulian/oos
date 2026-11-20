// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later
//! System > Overclock: the ADC clock of the next start, kept in limits.conf (docs/models.md 4.1). The cold boot reads it,
//! so a change counts from the next power-on; a start that does not finish sets the file aside (rcS).
//!
//! Every step above 1.25 GHz boots, but at the full rate the records lose samples (DHO924S, 2026-11-16): the item is
//! only in the menu with `adc.overclock = on` in system.conf (ADR 0010), or while limits.conf already sets another
//! clock, so the way back to 1.25 GHz stays.

use super::Ui;
use crate::board::{board, Board};
use oos_hal::fsutil::{DirSync, Opts};
use oos_hal::model::with_adc_clock;
use std::fs;

/// The ADC clock of the next start as the screen keeps it (ADR 0062).
pub struct Next {
    /// limits.conf, where the clock is kept (`--limits` of the command line)
    pub path: String,
    /// the clock limits.conf sets for the next start
    pub hz: f64,
}

impl Default for Next {
    fn default() -> Self {
        Next { path: oos_hal::model::LIMITS_CONF.into(), hz: oos_hal::model::platform().limits.adc_clock_hz }
    }
}

/// The switch that shows System > Overclock (system.conf, ADR 0010).
pub(super) const SWITCH: crate::sysconf::Flag =
    crate::sysconf::Flag { key: "adc.overclock", env: Some("OOS_OVERCLOCK"), on: "on" };

/// Whether System > Overclock is in the menu: the switch is on, or the next start already has another clock.
pub(super) fn shown(switch_on: bool, clock_next: f64) -> bool {
    switch_on || clock_next != board().clock_steps()[0]
}

/// The switch as read once at start.
pub(super) fn switch_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| SWITCH.get())
}

/// Text of a clock step; every step above the standard clock loses samples at the full rate (docs/models.md 4.1).
pub(super) fn label(hz: f64) -> String {
    let ghz = format!("{} GHz", hz / 1e9);
    if hz != board().clock_steps()[0] {
        format!("{} (test)", ghz)
    } else {
        ghz
    }
}

/// Replaces limits.conf in one step, the scope is switched off at the mains. No lines left: no file.
fn store(path: &str, hz: f64) -> std::io::Result<()> {
    // only a missing file is empty: a file that cannot be read stays as it is
    let old = match fs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let new = with_adc_clock(&old, hz);
    if new.trim().is_empty() {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => oos_hal::fsutil::sync_dir(path.as_ref()),
        };
    }
    oos_hal::fsutil::write_atomic(
        path.as_ref(),
        new.as_bytes(),
        &Opts { dir_sync: DirSync::Required, ..Default::default() },
    )
}

impl Ui {
    /// One step up or down the list (`d` from the picker can be several).
    pub(super) fn overclock_step(&mut self, d: i32) {
        let n = board().clock_steps().len() as i32;
        let i = board().clock_steps().iter().position(|&hz| hz == self.overclock.hz).unwrap_or(0) as i32;
        // no wrap: one step down from the standard clock must not land on the test clock
        let hz = board().clock_steps()[(i + d).clamp(0, n - 1) as usize];
        if let Err(e) = store(&self.overclock.path, hz) {
            return self.toast(&format!("Overclock not saved: {}", e));
        }
        self.overclock.hz = hz;
        self.toast(&if hz == board().clock_steps()[0] {
            format!("ADC {} after power off and on", label(hz))
        } else {
            format!("ADC {} after power off and on: records lose samples at the full rate, for tests only", label(hz))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::Item;
    use super::*;

    #[test]
    fn writes_the_step_and_keeps_other_lines() {
        let dir = crate::ui::testkit::Scratch::new("overclock");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limits.conf");
        fs::write(&path, "# my limits\nafg.sine_max_hz = 7e7\nadc.clock_hz = 2e9\n").unwrap();
        let mut ui = crate::ui::testkit::demo_ui();
        ui.overclock.path = path.to_str().unwrap().into();
        assert_eq!(ui.value(Item::Overclock), "1.25 GHz");
        ui.select_option(Item::Overclock, "1.5 GHz (test)");
        assert_eq!(ui.value(Item::Overclock), "1.5 GHz (test)");
        assert_eq!(fs::read_to_string(&path).unwrap(), "adc.clock_hz = 1.5e9\n# my limits\nafg.sine_max_hz = 7e7\n");
        ui.select_option(Item::Overclock, "1.25 GHz");
        assert_eq!(fs::read_to_string(&path).unwrap(), "# my limits\nafg.sine_max_hz = 7e7\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn back_at_the_standard_clock_no_file_is_left() {
        let dir = crate::ui::testkit::Scratch::new("overclock-empty");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limits.conf");
        let p = path.to_str().unwrap();
        store(p, 2e9).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "adc.clock_hz = 2e9\n");
        store(p, board().clock_steps()[0]).unwrap();
        assert!(!path.exists());
        store(p, board().clock_steps()[0]).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn steps_stop_at_both_ends() {
        let dir = crate::ui::testkit::Scratch::new("overclock-ends");
        fs::create_dir_all(&dir).unwrap();
        let mut ui = crate::ui::testkit::demo_ui();
        ui.overclock.path = dir.join("limits.conf").to_str().unwrap().into();
        ui.overclock_step(-1);
        assert_eq!(ui.value(Item::Overclock), "1.25 GHz");
        ui.overclock_step(10);
        assert_eq!(ui.value(Item::Overclock), "2 GHz (test)");
        ui.overclock_step(1);
        assert_eq!(ui.value(Item::Overclock), "2 GHz (test)");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_step_the_file_cannot_take_stays_as_it_was() {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.overclock.path = "/nonexistent-dir/limits.conf".into();
        ui.select_option(Item::Overclock, "1.5 GHz (test)");
        assert_eq!(ui.value(Item::Overclock), "1.25 GHz");
    }

    #[test]
    fn the_menu_shows_overclock_only_with_the_switch_or_another_clock() {
        SWITCH.check_both_files();
        let std = board().clock_steps()[0];
        assert!(!shown(false, std));
        assert!(shown(true, std));
        // a clock set before the switch went off: the item stays, so 1.25 GHz can be chosen again
        assert!(shown(false, board().clock_steps()[1]));
        let mut ui = crate::ui::testkit::demo_ui();
        ui.overclock.hz = std;
        assert_eq!(ui.items(super::super::MenuId::Utility).contains(&Item::Overclock), switch_on());
    }
}
