// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//! System > Overclock: the ADC clock of the next start, kept in limits.conf (docs/models.md 4.1). The cold boot reads it,
//! so a change counts from the next power-on; a start that does not finish sets the file aside (rcS).

use super::Ui;
use oos_hal::model::{with_adc_clock, CLOCK_STEPS};
use std::{fs, io::Write, path::Path};

/// Text of a clock step; the top step gives wrong samples (docs/models.md 4.1).
pub(super) fn label(hz: f64) -> String {
    let ghz = format!("{} GHz", hz / 1e9);
    if hz == CLOCK_STEPS[CLOCK_STEPS.len() - 1] { format!("{} (test)", ghz) } else { ghz }
}

/// Replaces limits.conf in one step, the scope is switched off at the mains. No lines left: no file.
fn store(path: &str, hz: f64) -> std::io::Result<()> {
    let old = fs::read_to_string(path).unwrap_or_default();
    let new = with_adc_clock(&old, hz);
    if new.trim().is_empty() {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => sync_dir(path),
        };
    }
    let tmp = format!("{}.new", path);
    let mut f = fs::File::create(&tmp)?;
    f.write_all(new.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    sync_dir(path)
}

fn sync_dir(path: &str) -> std::io::Result<()> {
    let dir = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::File::open(dir)?.sync_all()
}

impl Ui {
    /// One step up or down the list (`d` from the picker can be several).
    pub(super) fn overclock_step(&mut self, d: i32) {
        let n = CLOCK_STEPS.len() as i32;
        let i = CLOCK_STEPS.iter().position(|&hz| hz == self.clock_next).unwrap_or(0) as i32;
        let hz = CLOCK_STEPS[(i + d).rem_euclid(n) as usize];
        if let Err(e) = store(&self.limits_path, hz) {
            return self.toast(&format!("Overclock not saved: {}", e));
        }
        self.clock_next = hz;
        let top = CLOCK_STEPS[CLOCK_STEPS.len() - 1];
        self.toast(&if hz == top {
            format!("ADC {} after power off and on: wrong samples, for tests only", label(hz))
        } else if hz == CLOCK_STEPS[0] {
            format!("ADC {} after power off and on", label(hz))
        } else {
            format!("ADC {} after power off and on: experimental, more noise", label(hz))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Item, Source, Ui};
    use super::*;

    #[test]
    fn writes_the_step_and_keeps_other_lines() {
        let dir = std::env::temp_dir().join(format!("oos-overclock-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limits.conf");
        fs::write(&path, "# my limits\nafg.sine_max_hz = 7e7\nadc.clock_hz = 2e9\n").unwrap();
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.limits_path = path.to_str().unwrap().into();
        assert_eq!(ui.value(Item::Overclock), "1.25 GHz");
        ui.select_option(Item::Overclock, "1.5 GHz");
        assert_eq!(ui.value(Item::Overclock), "1.5 GHz");
        assert_eq!(fs::read_to_string(&path).unwrap(), "adc.clock_hz = 1.5e9\n# my limits\nafg.sine_max_hz = 7e7\n");
        ui.select_option(Item::Overclock, "1.25 GHz");
        assert_eq!(fs::read_to_string(&path).unwrap(), "# my limits\nafg.sine_max_hz = 7e7\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn back_at_the_standard_clock_no_file_is_left() {
        let dir = std::env::temp_dir().join(format!("oos-overclock-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limits.conf");
        let p = path.to_str().unwrap();
        store(p, 2e9).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "adc.clock_hz = 2e9\n");
        store(p, CLOCK_STEPS[0]).unwrap();
        assert!(!path.exists());
        store(p, CLOCK_STEPS[0]).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_step_the_file_cannot_take_stays_as_it_was() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.limits_path = "/nonexistent-dir/limits.conf".into();
        ui.select_option(Item::Overclock, "1.5 GHz");
        assert_eq!(ui.value(Item::Overclock), "1.25 GHz");
    }
}
