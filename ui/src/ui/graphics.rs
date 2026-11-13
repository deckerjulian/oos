// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Graphics: the picture drawn on the CPU (as always), or put together on the GPU (`crate::gpu`): the CPU
//! then draws the screen without the traces, and the Mali adds them and scales the picture for a monitor. A setting
//! of the scope (`/oos/system.conf`, `graphics`); `main.rs` starts and stops the GPU helper from it and falls back to
//! the CPU when the GPU is not there or fails.

use super::*;

impl Ui {
    pub(super) fn graphics_kind(it: Item) -> Option<Kind> {
        (it == Item::Graphics).then_some(Kind::Choice)
    }

    pub(super) fn graphics_label(it: Item) -> Option<&'static str> {
        (it == Item::Graphics).then_some("Graphics")
    }

    pub(super) fn graphics_value(&self, it: Item) -> Option<String> {
        (it == Item::Graphics).then(|| if self.sys.gpu { "GPU".to_string() } else { "CPU".to_string() })
    }

    pub(super) fn graphics_options(&self, it: Item) -> Option<Vec<String>> {
        (it == Item::Graphics).then(|| vec!["CPU".into(), "GPU".into()])
    }

    pub(super) fn graphics_adjust(&mut self, it: Item, _d: i32) -> bool {
        if it != Item::Graphics {
            return false;
        }
        self.sys.gpu = !self.sys.gpu;
        self.sys_save();
        if !self.sys.gpu {
            self.toast("Graphics: CPU");
        }
        // the GPU's start, or why it is not there, is told by main.rs
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_a_setting_of_the_scope() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        assert!(u.sys.gpu, "the GPU by default");
        assert!(u.items(MenuId::Utility).contains(&Item::Graphics) && !u.items(MenuId::Display).contains(&Item::Graphics));
        u.select_option(Item::Graphics, "CPU");
        assert!(!u.sys.gpu && u.value(Item::Graphics) == "CPU");
        assert!(!crate::sysconf::SysConf::parse(&u.sys.text()).0.gpu);
        u.select_option(Item::Graphics, "GPU");
        assert!(u.sys.gpu && u.value(Item::Graphics) == "GPU");
        assert!(crate::sysconf::SysConf::parse(&u.sys.text()).0.gpu);
        // a file without the line (older versions): the default
        assert!(crate::sysconf::SysConf::parse("ntp = off\n").0.gpu);
    }
}
