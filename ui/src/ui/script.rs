// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Menus and their items by name, for the test scripts (`cmd.rs`: `menu`, `set`, `do`). A tap at fixed coordinates
//! hits something else as soon as the layout or the text size moves the item; a name does not. The mask script tapped
//! a top-bar button that no longer exists and tested the Display menu instead, unnoticed.

use super::*;

impl Ui {
    /// Every menu the start menu and the top bar open, with the start menu itself.
    fn script_menus(&self) -> Vec<MenuId> {
        let mut v = vec![MenuId::Main, MenuId::Display, MenuId::Utility, MenuId::Storage, MenuId::Measure, MenuId::Cursor, MenuId::Horizontal, MenuId::Trigger, MenuId::Acquire];
        v.extend((0..NCH).map(MenuId::Channel));
        for (_, items) in self.main_groups() {
            for it in items {
                if let Item::Open(id) = it {
                    v.push(id);
                }
            }
        }
        v
    }

    /// Opens the menu named `name` (its title, or the label of the entry that opens it; case does not matter).
    pub fn script_menu(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        let id = self
            .script_menus()
            .into_iter()
            .find(|&id| self.menu_title(id).eq_ignore_ascii_case(name) || self.item_label(Item::Open(id)).eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("no menu {:?}", name))?;
        self.open_menu(id);
        Ok(())
    }

    /// The item labelled `label` in the menu open now.
    fn script_item(&self, label: &str) -> Result<Item, String> {
        if !self.menu_open {
            return Err("no menu open".into());
        }
        let label = label.trim();
        self.items(self.menu).into_iter().find(|&it| self.item_label(it).eq_ignore_ascii_case(label)).ok_or_else(|| format!("no item {:?} in {}", label, self.menu_title(self.menu)))
    }

    /// Sets the item `label` of the open menu to the option `value` (as shown on its button).
    pub fn script_set(&mut self, label: &str, value: &str) -> Result<(), String> {
        let it = self.script_item(label)?;
        let value = value.trim();
        let hit = |u: &Ui| u.value(it).eq_ignore_ascii_case(value);
        if Self::kind(it) == Kind::Value {
            // a value with - and +: down, then up, until it shows `value` or stops changing
            for d in [-1, 1] {
                for _ in 0..400 {
                    let before = self.value(it);
                    if hit(self) {
                        break;
                    }
                    self.adjust(it, d);
                    if self.value(it) == before {
                        break;
                    }
                }
            }
        } else {
            self.select_option(it, value);
        }
        if !self.value(it).eq_ignore_ascii_case(value) {
            return Err(format!("{} is {:?}, not {:?}", label.trim(), self.value(it), value));
        }
        Ok(())
    }

    /// Opens the picker of the item `label` of the open menu (the grid of all its options, as a tap on its value).
    pub fn script_pick(&mut self, label: &str) -> Result<(), String> {
        let it = self.script_item(label)?;
        if self.options(it).is_empty() {
            return Err(format!("{} has no options to pick from", label.trim()));
        }
        self.open_picker(it);
        Ok(())
    }

    /// Presses the item `label` of the open menu (an action, or a sub-menu).
    pub fn script_do(&mut self, label: &str) -> Result<(), String> {
        let it = self.script_item(label)?;
        self.activate(it);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_and_items_by_name() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        assert!(u.script_menu("nothing").is_err());
        u.script_menu("Mask").unwrap();
        assert_eq!(u.menu, MenuId::Mask);
        u.script_menu("display").unwrap();
        u.script_set("Graphics", "CPU").unwrap();
        assert!(!u.sys.gpu);
        assert!(u.script_set("Graphics", "Purple").is_err());
        assert!(u.script_do("No such item").is_err());
        u.script_menu("Trigger").unwrap();
        u.script_pick("Type").unwrap();
        assert_eq!(u.menu, MenuId::Pick);
        // a value item, down and up
        u.script_menu("Mask").unwrap();
        u.script_set("Tolerance X", "0.04 div").unwrap();
        u.script_set("Tolerance X", "0.36 div").unwrap();
        assert!((u.m.mask_x - 0.36).abs() < 1e-6);
    }
}
