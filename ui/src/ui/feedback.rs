// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Batch 30: controls that cannot act say why. A row that needs a stored reference is shown greyed while its slot is
//! empty, and a tap on it says "Store a reference first" (the store action, "Save to next R", is in the same menu). A
//! source row that is given a channel that is off turns the channel on and says so.

use super::*;

/// What a tap on a control of an empty reference slot says.
pub(super) const STORE_A_REFERENCE: &str = "Store a reference first";

impl Ui {
    /// Why `it` cannot do anything now (and is drawn greyed), or None. R1..R10 rows and the colour, offset and scale of
    /// the selected slot need a stored reference.
    pub(super) fn item_blocked(&self, it: Item) -> Option<&'static str> {
        match it {
            Item::RefShow(i) if self.refs.get(i).is_some_and(|r| r.is_none()) => Some(STORE_A_REFERENCE),
            Item::RefColor | Item::RefOffset | Item::RefVpd if self.refs.get(self.ref_sel).is_some_and(|r| r.is_none()) => Some(STORE_A_REFERENCE),
            _ => None,
        }
    }

    /// True (after saying why) when `it` is blocked: the caller does nothing more.
    pub(super) fn item_blocked_toast(&mut self, it: Item) -> bool {
        let Some(why) = self.item_blocked(it) else { return false };
        self.toast(why);
        true
    }

    /// A channel given to a source row (Spectrum, Eye, Bus, Power) that is off is turned on, with a toast.
    pub(super) fn source_turn_on(&mut self, id: WaveId) {
        if let WaveId::Ch(c) = id {
            if c < NCH && !self.m.ch[c].on {
                self.select_option(Item::ChOn(c), "On");
                self.toast(&format!("CH{} turned on", c + 1));
            }
        }
    }

    /// Greys a drawn row: a veil over its label and its control.
    pub(super) fn draw_blocked_veil(cv: &mut Canvas, label: Rect, ctrl: Rect) {
        for r in [label, ctrl] {
            if r.w > 0 && r.h > 0 {
                cv.blend(r, DLG_BG, 150);
            }
        }
    }
}
