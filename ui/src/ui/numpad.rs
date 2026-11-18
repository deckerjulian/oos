// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The number pad (issue #22): a tap on the middle of a value row whose number can be typed (the scale and offset of
//! a channel, the time base and its position, the trigger level) opens a picker of keys: the digits, the point, the
//! sign, and a key for each prefix of the row's unit (`nV` ... `kV`, `ns` ... `ks`), which ends the entry and sets the
//! row. The value is taken as SCPI takes it (`:CHANnel<n>:SCALe`, `:TIMebase:SCALe`, ...): onto the nearest step of a
//! ladder, the fine scale onto its calibrated row.

use super::menu_actions::keep_expand_point;
use super::*;

/// The row the pad enters into, while it is open.
#[derive(Default)]
pub(super) struct NumPad {
    pub target: Option<Item>,
}

/// The keys, four in a row; a prefix key (`n`, `u`, `m`, `1`, `k`) is shown with the row's unit and ends the entry.
const KEYS: [&str; 20] =
    ["7", "8", "9", "n", "4", "5", "6", "u", "1", "2", "3", "m", "0", ".", "-", "1x", "Del", "Clear", "Cancel", "k"];

/// The factor and the shown prefix of a prefix key.
fn prefix(key: &str) -> Option<(f64, &'static str)> {
    Some(match key {
        "n" => (1e-9, "n"),
        "u" => (1e-6, "\u{b5}"),
        "m" => (1e-3, "m"),
        "1x" => (1.0, ""),
        "k" => (1e3, "k"),
        _ => return None,
    })
}

/// Longest entry, in characters.
const ENTRY_MAX: usize = 12;

impl Ui {
    /// The unit of a row whose number can be typed, or none.
    pub(super) fn numpad_unit(&self, it: Item) -> Option<&'static str> {
        let m = &self.m;
        match it {
            Item::ChScale(c) | Item::ChOffset(c) if c < NCH => Some(m.unit(c)),
            Item::HScale | Item::HDelay => Some("s"),
            Item::TrigLevel if m.trig_src < NCH => Some(m.unit(m.trig_src)),
            _ => None,
        }
    }

    /// Opens the pad for row `it`, with an empty entry.
    pub(super) fn open_numpad(&mut self, it: Item) {
        self.numpad.target = Some(it);
        self.keypad.clear();
        self.open_picker(Item::NumEntry);
    }

    /// The keys of the pad, the prefix keys with the unit of its row.
    pub(super) fn numpad_keys(&self) -> Vec<String> {
        let unit = self.numpad.target.and_then(|it| self.numpad_unit(it)).unwrap_or("");
        KEYS.iter()
            .map(|&k| match prefix(k) {
                Some((_, p)) => format!("{}{}", p, unit),
                None => k.to_string(),
            })
            .collect()
    }

    /// The title of the pad: the row and the entry so far.
    pub(super) fn numpad_title(&self) -> String {
        let row = self.numpad.target.map_or(String::new(), |it| self.item_label(it).to_string());
        format!("{} \u{b7} {}_", row, self.keypad)
    }

    /// Key `k` of the pad was tapped.
    pub(super) fn numpad_key(&mut self, k: usize) {
        let Some(&key) = KEYS.get(k) else { return };
        if let Some((factor, _)) = prefix(key) {
            let typed = self.keypad.parse::<f64>().ok().filter(|v| v.is_finite());
            let target = self.numpad.target.take();
            self.close_picker();
            if let (Some(v), Some(it)) = (typed, target) {
                self.numpad_set(it, v * factor);
            }
            return;
        }
        match key {
            "Cancel" => {
                self.numpad.target = None;
                self.close_picker();
            }
            "Clear" => self.keypad.clear(),
            "Del" => {
                self.keypad.pop();
            }
            "." if self.keypad.contains('.') => {}
            "-" if !self.keypad.is_empty() => {}
            d if self.keypad.chars().count() < ENTRY_MAX => self.keypad.push_str(d),
            _ => {}
        }
    }

    /// Sets row `it` to `v` (in its unit), as SCPI would.
    fn numpad_set(&mut self, it: Item, v: f64) {
        let m = &mut self.m;
        let v = v as f32;
        match it {
            Item::ChScale(c) => {
                if v <= 0.0 {
                    return self.toast("A scale is above 0");
                }
                let old = m.ch[c].volts_per_div();
                let want = v / m.ch[c].probe;
                (m.ch[c].vscale, m.ch[c].vfine) = Self::vscale_for(want, m.fine_v);
                // around ground: the zero line keeps its place on the screen
                if m.v_expand_gnd && old > 0.0 {
                    m.ch[c].offset *= m.ch[c].volts_per_div() / old;
                }
            }
            Item::ChOffset(c) => m.ch[c].offset = v,
            Item::HScale => {
                if v <= 0.0 {
                    return self.toast("A scale is above 0");
                }
                let old = m.time_per_div();
                (m.tscale, m.tfine) = (nearest_step(&tscales(), v), 1.0);
                keep_expand_point(m, old);
            }
            Item::HDelay => m.delay = v,
            Item::TrigLevel => m.trig_level = v,
            _ => return,
        }
        self.settings_changed();
    }

    /// The `vscale` step and its fine factor for `want` V/div at the BNC: the nearest step of the ladder, or with the
    /// fine scale the first row of the calibrated table at or above it, so the range never cuts the signal off (SCPI's
    /// `:CHANnel<n>:SCALe` and the pad).
    pub(crate) fn vscale_for(want: f32, fine: bool) -> (usize, f32) {
        if !fine {
            return (nearest_step(&VSCALES, want), 1.0);
        }
        let rows = board().vscale_steps_nv();
        let nv = want as f64 * 1e9 * (1.0 - 1e-6);
        let row = rows.iter().copied().find(|&k| k as f64 >= nv).unwrap_or(rows[rows.len() - 1]);
        let row = (row as f64 * 1e-9) as f32;
        let i = VSCALES.iter().rposition(|&s| s <= row * 1.000001).unwrap_or(0);
        let f = row / VSCALES[i];
        (i, if (f - 1.0).abs() < 1e-6 { 1.0 } else { f })
    }

    pub(super) fn numpad_kind(it: Item) -> Option<Kind> {
        (it == Item::NumEntry).then_some(Kind::Choice)
    }

    pub(super) fn numpad_label(it: Item) -> Option<&'static str> {
        (it == Item::NumEntry).then_some("Enter a value")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(u: &mut Ui, keys: &[&str]) {
        let shown = u.numpad_keys();
        for k in keys {
            let i = shown.iter().position(|s| s == k).unwrap_or_else(|| panic!("no key {:?} in {:?}", k, shown));
            u.activate(Item::PickOpt(i));
        }
    }

    fn opened(it: Item) -> Ui {
        let mut u = crate::ui::testkit::demo_ui();
        u.open_menu(MenuId::Channel(0));
        u.open_numpad(it);
        assert!(u.menu == MenuId::Pick && u.pick.is_some_and(|p| p.it == Item::NumEntry));
        u
    }

    #[test]
    fn a_typed_scale_offset_time_and_level_are_set() {
        let mut u = opened(Item::ChScale(0));
        assert_eq!(u.numpad_keys()[11], "mV");
        press(&mut u, &["2", "0", "0", "mV"]);
        assert!((u.m.ch[0].volts_per_div() - 0.2).abs() < 1e-6, "{}", u.m.ch[0].volts_per_div());
        assert!(u.menu != MenuId::Pick && u.numpad.target.is_none(), "the pad closes");

        let mut u = opened(Item::ChOffset(0));
        press(&mut u, &["-", "1", ".", "5", "V"]);
        assert_eq!(u.m.ch[0].offset, -1.5);

        let mut u = opened(Item::HScale);
        press(&mut u, &["5", "\u{b5}s"]);
        assert!((u.m.time_per_div() - 5e-6).abs() < 1e-12, "{}", u.m.time_per_div());

        let mut u = opened(Item::HDelay);
        press(&mut u, &["1", "2", "ns"]);
        assert!((u.m.delay - 12e-9).abs() < 1e-15);

        let mut u = opened(Item::TrigLevel);
        press(&mut u, &["3", "0", "0", "mV"]);
        assert!((u.m.trig_level - 0.3).abs() < 1e-6);
    }

    #[test]
    fn the_entry_keys_and_cancel() {
        let mut u = opened(Item::ChOffset(0));
        let before = u.m.ch[0].offset;
        press(&mut u, &["1", ".", ".", "2", "-", "Del", "3"]);
        assert_eq!(u.keypad, "1.3", "one point, the sign only first, Del takes the last");
        assert!(u.numpad_title().ends_with("1.3_"), "{}", u.numpad_title());
        press(&mut u, &["Clear", "Cancel"]);
        assert_eq!(u.m.ch[0].offset, before);
        assert!(u.menu != MenuId::Pick && u.numpad.target.is_none());
        // nothing typed: the unit key closes and sets nothing; a scale of 0 is refused
        let mut u = opened(Item::ChScale(0));
        let scale = u.m.ch[0].volts_per_div();
        press(&mut u, &["V"]);
        assert_eq!(u.m.ch[0].volts_per_div(), scale);
        let mut u = opened(Item::ChScale(0));
        press(&mut u, &["0", "V"]);
        assert_eq!(u.m.ch[0].volts_per_div(), scale);
        assert_eq!(u.toast.as_ref().unwrap().0, "A scale is above 0");
    }

    /// A tap on the middle of a value row of a number opens the pad; of another value row it does not.
    #[test]
    fn a_tap_on_the_value_opens_the_pad() {
        let mut u = crate::ui::testkit::demo_ui();
        u.open_menu(MenuId::Channel(0));
        let row = |u: &Ui, it: Item| u.dlg_layout().unwrap().rows.into_iter().find(|r| r.it == it).unwrap().ctrl;
        let r = row(&u, Item::ChScale(0));
        u.tap(r.x + r.w / 2, r.y + r.h / 2);
        assert_eq!(u.numpad.target, Some(Item::ChScale(0)));
        assert!(u.pick.is_some_and(|p| p.it == Item::NumEntry));
    }
}
