// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//! System > Overclock: the ADC clock of the next start, kept in limits.conf (docs/models.md 4.1). The cold boot reads it,
//! so a change counts from the next power-on; a start that does not finish sets the file aside (rcS).

use super::Ui;
use crate::board::{board, Board};
use oos_hal::model::with_adc_clock;
use std::{fs, io::Write};
#[cfg(unix)]
use std::path::Path;

/// Text of a clock step; the top step gives wrong samples (docs/models.md 4.1).
pub(super) fn label(hz: f64) -> String {
    let ghz = format!("{} GHz", hz / 1e9);
    if hz == board().clock_steps()[board().clock_steps().len() - 1] { format!("{} (test)", ghz) } else { ghz }
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

/// Makes the rename or removal durable (Linux). Windows opens no directory as a file, and needs no such step.
#[cfg(not(unix))]
fn sync_dir(_path: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &str) -> std::io::Result<()> {
    let dir = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::File::open(dir)?.sync_all()
}

impl Ui {
    /// One step up or down the list (`d` from the picker can be several).
    pub(super) fn overclock_step(&mut self, d: i32) {
        let n = board().clock_steps().len() as i32;
        let i = board().clock_steps().iter().position(|&hz| hz == self.clock_next).unwrap_or(0) as i32;
        // no wrap: one step down from the standard clock must not land on the test clock
        let hz = board().clock_steps()[(i + d).clamp(0, n - 1) as usize];
        if let Err(e) = store(&self.limits_path, hz) {
            return self.toast(&format!("Overclock not saved: {}", e));
        }
        self.clock_next = hz;
        let top = board().clock_steps()[board().clock_steps().len() - 1];
        self.toast(&if hz == top {
            format!("ADC {} after power off and on: wrong samples, for tests only", label(hz))
        } else if hz == board().clock_steps()[0] {
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
        store(p, board().clock_steps()[0]).unwrap();
        assert!(!path.exists());
        store(p, board().clock_steps()[0]).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn steps_stop_at_both_ends() {
        let dir = std::env::temp_dir().join(format!("oos-overclock-ends-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.limits_path = dir.join("limits.conf").to_str().unwrap().into();
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
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.limits_path = "/nonexistent-dir/limits.conf".into();
        ui.select_option(Item::Overclock, "1.5 GHz");
        assert_eq!(ui.value(Item::Overclock), "1.25 GHz");
    }
}
