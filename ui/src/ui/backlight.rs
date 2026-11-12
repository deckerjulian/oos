// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Display > Backlight: the brightness of the panel's LED backlight, 10 to 100 %. A setting of the scope
//! (`/oos/system.conf`, `backlight`), set at the start and at every change through the kernel's backlight device (a PWM
//! of 0 to 255 on the DHO800/DHO900). The percent follow what the eye sees: the PWM goes with the square of it, so
//! the low steps are not all lost in the bright end, and 10 % still lights the panel.

use super::*;

/// The darkest and the step of the knob, in percent.
pub const MIN: u32 = 10;
pub const STEP: u32 = 5;

/// The backlight level for `pct` percent on a device of `max` steps (at least 1: never dark).
pub fn level(pct: u32, max: u32) -> u32 {
    let f = pct.clamp(MIN, 100) as f64 / 100.0;
    ((max as f64 * f * f).round() as u32).clamp(1, max.max(1))
}

/// Sets every backlight device of the kernel to `pct`. Quietly nothing where there is none.
pub fn apply(pct: u32) {
    let Ok(d) = std::fs::read_dir("/sys/class/backlight") else { return };
    for e in d.flatten() {
        let p = e.path();
        let Some(max) = std::fs::read_to_string(p.join("max_brightness")).ok().and_then(|s| s.trim().parse::<u32>().ok()) else { continue };
        if let Err(err) = std::fs::write(p.join("brightness"), level(pct, max).to_string()) {
            eprintln!("oos-ui: backlight {}: {}", p.display(), err);
        }
    }
}

impl Ui {
    pub(super) fn backlight_kind(it: Item) -> Option<Kind> {
        (it == Item::Backlight).then_some(Kind::Value)
    }

    pub(super) fn backlight_label(it: Item) -> Option<&'static str> {
        (it == Item::Backlight).then_some("Backlight")
    }

    pub(super) fn backlight_value(&self, it: Item) -> Option<String> {
        (it == Item::Backlight).then(|| format!("{} %", self.sys.backlight))
    }

    pub(super) fn backlight_adjust(&mut self, it: Item, d: i32) -> bool {
        if it != Item::Backlight {
            return false;
        }
        self.sys.backlight = (self.sys.backlight as i32 + STEP as i32 * d).clamp(MIN as i32, 100) as u32;
        self.sys_save();
        self.apply_backlight();
        true
    }

    /// On the unit only (tests and host runs have no settings file).
    pub(super) fn apply_backlight(&self) {
        if self.sys_path.is_some() {
            apply(self.sys.backlight);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backlight_follows_the_eye() {
        assert_eq!((level(100, 255), level(50, 255), level(10, 255), level(0, 255)), (255, 64, 3, 3));
        assert_eq!(level(10, 10), 1, "never dark");
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        assert_eq!(u.value(Item::Backlight), "100 %");
        assert!(u.items(MenuId::Display).contains(&Item::Backlight));
        u.adjust(Item::Backlight, -3);
        assert_eq!(u.sys.backlight, 85);
        u.adjust(Item::Backlight, -100);
        assert_eq!(u.value(Item::Backlight), "10 %");
        assert_eq!(crate::sysconf::SysConf::parse(&u.sys.text()).0.backlight, 10);
    }
}
