// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The menu rows of the settings table (settings.rs): kind, label, value, options and a step, from the row of each
//! item. What a change sets off on the screen besides `settings_changed` is here, per item.

use super::*;
use crate::settings::Setting;

/// The items the table answers for, by setup key.
const ITEMS: [(Item, &str); 8] = [
    (Item::Persist, "persist"),
    (Item::Intensity, "intensity"),
    (Item::GridStyle, "grid"),
    (Item::GridBright, "grid_bright"),
    (Item::GridFrame, "grid_frame"),
    (Item::ColorGrade, "color_grade"),
    (Item::Dots, "dots"),
    (Item::BottomBar, "bar"),
];

fn row(it: Item) -> Option<&'static Setting> {
    ITEMS.iter().find(|(i, _)| *i == it).and_then(|(_, key)| crate::settings::find(key))
}

impl Ui {
    pub(super) fn setting_kind(it: Item) -> Option<Kind> {
        row(it).map(|s| if s.is_number() { Kind::Value } else { Kind::Choice })
    }

    pub(super) fn setting_label(it: Item) -> Option<&'static str> {
        row(it).map(|s| s.label)
    }

    pub(super) fn setting_value(&self, it: Item) -> Option<String> {
        row(it).map(|s| s.shown(&self.m))
    }

    pub(super) fn setting_options(&self, it: Item) -> Option<Vec<String>> {
        row(it).map(Setting::options)
    }

    pub(super) fn setting_adjust(&mut self, it: Item, d: i32) -> bool {
        let Some(s) = row(it) else { return false };
        s.step(&mut self.m, d);
        match it {
            // the persistence map of the old time does not hold for the new one
            Item::Persist => self.ph.clear(),
            Item::Dots | Item::BottomBar => self.dirty_map = true,
            _ => {}
        }
        self.settings_changed();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_its_item_and_a_step_changes_it() {
        for s in crate::settings::all() {
            let it = ITEMS.iter().find(|(_, k)| *k == s.key).map(|(i, _)| *i);
            let it = it.unwrap_or_else(|| panic!("{}: no menu item in ITEMS", s.key));
            let mut u = crate::ui::testkit::demo_ui();
            assert!(u.items(MenuId::Display).contains(&it), "{:?} is not in the Display menu", it);
            assert_eq!(u.item_label(it), s.label);
            let before = u.value(it);
            u.adjust(it, 1);
            if u.value(it) == before {
                u.adjust(it, -1);
            }
            assert_ne!(u.value(it), before, "{:?}", it);
            assert_eq!(u.value(it), s.shown(&u.m), "{:?}", it);
            if !s.is_number() {
                assert!(u.options(it).contains(&u.value(it)), "{:?}", it);
            }
        }
    }
}
