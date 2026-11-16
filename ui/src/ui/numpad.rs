// SPDX-License-Identifier: GPL-2.0-or-later
//! The numeric entry pad of the Studio sheets: a tap on the value of a "- value +" row opens it. The typed text and the
//! unit are in the field at the top (the current value is selected: the first key replaces it); the digits, the sign,
//! Exp, Bksp and Clear edit it; a prefix key (G M k m u n p) or None ends the entry with that prefix; Min and Max set
//! the limits of the item; Enter parses and sets the value, Cancel closes with no change.
//! The text is `Ui::keypad`, as for the other keypads. The value goes in through the stepper's own path (`adjust`), or
//! straight into the setting where the stepper's grid is coarser than what can be typed (the trigger level).
use super::*;

/// The pad.
#[derive(Clone, Debug)]
pub(super) struct NumPad {
    pub it: Item,
    /// the text is the current value and the first key replaces it
    pub fresh: bool,
    /// the unit of the item without a prefix: "V", "s", "Hz"
    pub unit: String,
}

/// The keys of the pad, in the order of `numpad_geo`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NumKey {
    Digit(u8),
    Dot,
    Sign,
    Exp,
    Bksp,
    Clear,
    /// a prefix with its power of ten
    Prefix(i32),
    Min,
    Max,
    Cancel,
    Enter,
}

pub(super) const PAD_W: i32 = 484;
pub(super) const PAD_H: i32 = 352;
const KEY_H: i32 = 48;

/// The SI prefixes of the pad: label and power of ten ("None" is 0).
pub(super) const PREFIXES: [(&str, i32); 8] = [("G", 9), ("M", 6), ("k", 3), ("m", -3), ("\u{b5}", -6), ("n", -9), ("p", -12), ("None", 0)];

/// A displayed value split into its number (in base units) and its unit without the prefix: "449 mV" is (0.449, "V").
pub(super) fn split_value(s: &str) -> Option<(f64, String)> {
    let s = s.trim();
    let end = s.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && i == 0))).map_or(s.len(), |(i, _)| i);
    let num: f64 = s[..end].parse().ok()?;
    let rest = s[end..].trim();
    let mut cs = rest.chars();
    let (factor, unit) = match cs.next() {
        Some(p) if cs.as_str().chars().next().is_some_and(|c| c.is_alphabetic() || c == '%') => {
            let f = match p { 'G' => 1e9, 'M' => 1e6, 'k' => 1e3, 'm' => 1e-3, '\u{b5}' | 'u' => 1e-6, 'n' => 1e-9, 'p' => 1e-12, _ => 0.0 };
            if f != 0.0 { (f, cs.as_str().to_string()) } else { (1.0, rest.to_string()) }
        }
        _ => (1.0, rest.to_string()),
    };
    Some((num * factor, unit))
}

/// A number as typed: no exponent notation, no trailing zeros.
fn plain(v: f64) -> String {
    let mut s = format!("{v:.9}");
    if s.contains('.') { s = s.trim_end_matches('0').trim_end_matches('.').to_string(); }
    if s == "-0" || s.is_empty() { s = "0".into(); }
    s
}

impl Ui {
    /// The value of an item as a number in base units, and its unit.
    pub(super) fn numpad_value(&self, it: Item) -> Option<(f64, String)> {
        split_value(&self.value(it))
    }

    /// Opens the pad on a "- value +" item. False when its value is not a number.
    pub(super) fn open_numpad(&mut self, it: Item) -> bool {
        let Some((v, unit)) = self.numpad_value(it) else { return false };
        self.keypad = plain(v);
        self.numpad = Some(NumPad { it, fresh: true, unit });
        true
    }

    /// The limits of an item: its own, or where the stepper stops.
    pub(super) fn numpad_limits(&mut self, it: Item) -> Option<(f64, f64)> {
        if matches!(it, Item::TrigLevel | Item::TrigLevel2) {
            let secondary = it == Item::TrigLevel2 && matches!(self.m.trig_type, TrigType::Delay | TrigType::SetupHold);
            let r = self.trigger_vpd(secondary) as f64 * 5.0;
            return Some((-r, r));
        }
        let (cur, _) = self.numpad_value(it)?;
        let saved = self.m.clone();
        let mut ends = [cur, cur];
        for (k, dir) in [-1, 1].into_iter().enumerate() {
            let mut last = cur;
            for _ in 0..8 {
                self.adjust(it, dir * 512);
                let Some((v, _)) = self.numpad_value(it) else { break };
                if (v - last).abs() <= last.abs() * 1e-9 { break; }
                last = v;
            }
            ends[k] = last;
            self.m = saved.clone();
        }
        Some((ends[0].min(ends[1]), ends[0].max(ends[1])))
    }

    /// Sets an item to `want` (clamped to its limits) and returns the value it has now.
    pub(super) fn numpad_apply(&mut self, it: Item, want: f64) -> f64 {
        let limits = self.numpad_limits(it);
        let want = limits.map_or(want, |(lo, hi)| want.clamp(lo, hi));
        if matches!(it, Item::TrigLevel | Item::TrigLevel2) {
            if it == Item::TrigLevel { self.m.trig_level = want as f32; } else { self.m.trig_level2 = want as f32; }
            self.replay.reset_search();
            self.settings_changed();
            return want;
        }
        // the stepper's own path: big steps first, then smaller ones, as long as the value does not pass the wanted one
        let mut cur = self.numpad_value(it).map_or(want, |v| v.0);
        for d in [1024, 256, 64, 16, 4, 1] {
            for _ in 0..80 {
                let dir = if want > cur { 1 } else { -1 };
                let before = self.m.clone();
                self.adjust(it, dir * d);
                let Some((nv, _)) = self.numpad_value(it) else { break };
                if (nv - cur).abs() <= cur.abs() * 1e-12 { break; }
                let past = (dir > 0 && nv > want) || (dir < 0 && nv < want);
                if past && !(d == 1 && (nv - want).abs() < (cur - want).abs()) {
                    self.m = before;
                    break;
                }
                cur = nv;
                if past { break; }
            }
        }
        self.settings_changed();
        self.numpad_value(it).map_or(want, |v| v.0)
    }

    /// Enters `v` (base units): sets the item, closes the pad, and says so when the value had to be limited.
    fn numpad_enter(&mut self, it: Item, v: f64) {
        let (lo, hi) = self.numpad_limits(it).unwrap_or((f64::MIN, f64::MAX));
        let clamped = v < lo || v > hi;
        self.numpad_apply(it, v);
        self.numpad = None;
        if clamped { let s = self.value(it); self.toast(&format!("Limited to {s}")); }
    }

    // ------------------------------------------------------------------------------------------------ geometry

    pub(super) fn numpad_rect(&self) -> Rect {
        self.scr.rect().center(PAD_W, PAD_H)
    }

    /// The entry field, and the keys with their rectangles.
    pub(super) fn numpad_geo(&self) -> (Rect, Vec<(NumKey, Rect)>) {
        let p = self.numpad_rect();
        let field = Rect::new(p.x + 12, p.y + 12, p.w - 24, 56);
        let (x0, y0, kw, gap) = (p.x + 12, p.y + 76, 76, 4);
        let col = |c: i32| x0 + c * (kw + gap);
        let row = |r: i32| y0 + r * (KEY_H + gap);
        let mut keys = Vec::new();
        let layout = [
            [NumKey::Digit(7), NumKey::Digit(8), NumKey::Digit(9), NumKey::Bksp],
            [NumKey::Digit(4), NumKey::Digit(5), NumKey::Digit(6), NumKey::Clear],
            [NumKey::Digit(1), NumKey::Digit(2), NumKey::Digit(3), NumKey::Sign],
        ];
        for (r, line) in layout.iter().enumerate() {
            for (c, k) in line.iter().enumerate() { keys.push((*k, Rect::new(col(c as i32), row(r as i32), kw, KEY_H))); }
        }
        keys.push((NumKey::Digit(0), Rect::new(col(0), row(3), kw * 2 + gap, KEY_H)));
        keys.push((NumKey::Dot, Rect::new(col(2), row(3), kw, KEY_H)));
        keys.push((NumKey::Exp, Rect::new(col(3), row(3), kw, KEY_H)));
        // the prefixes: two columns at the right
        let px = col(4) + 8;
        for (i, (_, e)) in PREFIXES.iter().enumerate() {
            keys.push((NumKey::Prefix(*e), Rect::new(px + (i as i32 % 2) * 64, row(i as i32 / 2), 60, KEY_H)));
        }
        // the bottom row: Min, Max, Cancel, Enter
        let by = row(4) + 4;
        let bw = (p.w - 24 - 3 * gap) / 4;
        for (i, k) in [NumKey::Min, NumKey::Max, NumKey::Cancel, NumKey::Enter].into_iter().enumerate() {
            keys.push((k, Rect::new(x0 + i as i32 * (bw + gap), by, bw, KEY_H)));
        }
        (field, keys)
    }

    pub(super) fn numpad_label(k: NumKey) -> String {
        match k {
            NumKey::Digit(d) => d.to_string(),
            NumKey::Dot => ".".into(),
            NumKey::Sign => "\u{b1}".into(),
            NumKey::Exp => "Exp".into(),
            NumKey::Bksp => "Bksp".into(),
            NumKey::Clear => "Clear".into(),
            NumKey::Prefix(e) => PREFIXES.iter().find(|p| p.1 == e).map_or("", |p| p.0).to_string(),
            NumKey::Min => "Min".into(),
            NumKey::Max => "Max".into(),
            NumKey::Cancel => "Cancel".into(),
            NumKey::Enter => "Enter".into(),
        }
    }

    // ------------------------------------------------------------------------------------------------ keys

    /// The typed text as a number (None while it is not one).
    pub(super) fn numpad_typed(&self) -> Option<f64> {
        self.keypad.trim().parse::<f64>().ok().filter(|v| v.is_finite())
    }

    pub(super) fn numpad_key(&mut self, k: NumKey) {
        let Some(pad) = self.numpad.clone() else { return };
        match k {
            NumKey::Digit(d) => {
                if pad.fresh { self.keypad.clear(); }
                if self.keypad == "0" { self.keypad.clear(); }
                if self.keypad == "-0" { self.keypad = "-".into(); }
                if self.keypad.len() < 14 { self.keypad.push((b'0' + d) as char); }
            }
            NumKey::Dot => {
                if pad.fresh { self.keypad.clear(); }
                let tail = self.keypad.rsplit(['e', 'E']).next().unwrap_or("");
                if !self.keypad.contains(['e', 'E']) && !tail.contains('.') {
                    if self.keypad.is_empty() || self.keypad == "-" { self.keypad.push('0'); }
                    self.keypad.push('.');
                }
            }
            NumKey::Sign => {
                if let Some(p) = self.keypad.find(['e', 'E']) {
                    // the sign of the exponent
                    if self.keypad[p + 1..].starts_with('-') { self.keypad.remove(p + 1); } else { self.keypad.insert(p + 1, '-'); }
                } else if self.keypad.starts_with('-') { self.keypad.remove(0); } else { self.keypad.insert(0, '-'); }
            }
            NumKey::Exp => {
                if pad.fresh { self.keypad.clear(); }
                if !self.keypad.contains(['e', 'E']) && !self.keypad.is_empty() && self.keypad != "-" { self.keypad.push('e'); }
            }
            NumKey::Bksp => {
                if pad.fresh { self.keypad.clear(); } else { self.keypad.pop(); }
            }
            NumKey::Clear => self.keypad.clear(),
            NumKey::Prefix(e) => {
                if let Some(v) = self.numpad_typed() { self.numpad_enter(pad.it, v * 10f64.powi(e)); }
                return;
            }
            NumKey::Min | NumKey::Max => {
                if let Some((lo, hi)) = self.numpad_limits(pad.it) { self.numpad_enter(pad.it, if k == NumKey::Min { lo } else { hi }); }
                return;
            }
            NumKey::Cancel => {
                self.numpad = None;
                return;
            }
            NumKey::Enter => {
                match self.numpad_typed() {
                    Some(v) => self.numpad_enter(pad.it, v),
                    None => self.toast("Enter a number"),
                }
                return;
            }
        }
        if let Some(p) = &mut self.numpad { p.fresh = false; }
    }

    /// A tap while the pad is open: a key, or outside it (nothing: the pad is modal). Always taken.
    pub(super) fn numpad_tap(&mut self, x: i32, y: i32) -> bool {
        if self.numpad.is_none() { return false; }
        let (_, keys) = self.numpad_geo();
        if let Some(&(k, _)) = keys.iter().find(|(_, r)| Self::hit40(*r).contains(x, y) && r.contains(x, y)) {
            self.numpad_key(k);
        }
        true
    }

    // ------------------------------------------------------------------------------------------------ drawing

    pub(super) fn draw_numpad(&self, cv: &mut Canvas) {
        let Some(pad) = &self.numpad else { return };
        let f = self.fonts();
        let p = self.numpad_rect();
        cv.blend(self.scr.rect(), BG, 120);
        cv.round(p, 10, ACCENT, 255);
        cv.round(p.inset(2), 9, DLG_BG, 255);
        let (field, keys) = self.numpad_geo();
        cv.round(field, 6, BTN_BORDER, 255);
        cv.round(field.inset(1), 5, BG, 255);
        // the name of the item, small, over the typed text and the unit
        cv.text(&f.s, field.x + 10, field.y + 16, &self.item_label(pad.it), DIM);
        let text = if self.keypad.is_empty() { "0".to_string() } else { self.keypad.clone() };
        let sel = pad.fresh;
        let tw = f.lb.width(&text);
        if sel { cv.fill(Rect::new(field.x + 8, field.y + 21, tw + 4, 30), mix(BG, ACCENT, 90)); }
        cv.text(&f.lb, field.x + 10, field.y + 44, &text, if sel { DARK_TEXT } else { TEXT });
        cv.text(&f.mb, field.x + 16 + tw, field.y + 44, &pad.unit, DIM);
        let (lo, hi) = (self.numpad_limit_text(pad.it, false), self.numpad_limit_text(pad.it, true));
        if let (Some(lo), Some(hi)) = (lo, hi) {
            cv.text_in(&f.s, Rect::new(field.x, field.y + 4, field.w - 10, 20), 2, &format!("{lo} to {hi}"), DIM);
        }
        for (k, r) in keys {
            let label = Self::numpad_label(k);
            let (fill, border, ink) = match k {
                NumKey::Enter => (ACCENT, ACCENT, DARK_TEXT),
                NumKey::Cancel => (mix(DLG_BG, RED, 30), RED, RED),
                NumKey::Prefix(_) | NumKey::Min | NumKey::Max | NumKey::Exp => (mix(DLG_BG, ACCENT, 25), BTN_BORDER, TEXT),
                _ => (BTN, BTN_BORDER, TEXT),
            };
            btn(cv, r, fill, border);
            cv.text_in(&f.mb, r, 1, &label, ink);
        }
    }

    /// The limit of an item as text (for the field); only for the items whose limits are known without a probe.
    fn numpad_limit_text(&self, it: Item, high: bool) -> Option<String> {
        if !matches!(it, Item::TrigLevel | Item::TrigLevel2) { return None; }
        let secondary = it == Item::TrigLevel2 && matches!(self.m.trig_type, TrigType::Delay | TrigType::SetupHold);
        let r = self.trigger_vpd(secondary) as f64 * 5.0;
        let unit = self.numpad_value(it).map(|v| v.1).unwrap_or_default();
        Some(si(if high { r } else { -r }, &unit))
    }
}
