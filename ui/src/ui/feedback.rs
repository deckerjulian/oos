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

    /// The source a source row holds now (None = the optional row is Off).
    pub(super) fn source_item_value(&self, it: Item) -> Option<WaveId> {
        let m = &self.m;
        match it {
            Item::SpecSrc => Some(m.spec_src),
            Item::EyeSrc => Some(m.eye_src),
            Item::PowV => Some(m.pw_v),
            Item::PowI => Some(m.pw_i),
            Item::EyeB => m.eye_b,
            Item::EyeRail => m.eye_rail,
            Item::PowVin => m.pw_vin,
            Item::PowClk => m.pw_clk,
            Item::BusClk => Some(m.bus.clk),
            Item::BusData => Some(m.bus.data),
            _ => None,
        }
    }

    /// Something other than a source row's own turn-on keeps channel `c` in use: a source row, a measurement, the trigger.
    fn channel_in_use(&self, c: usize) -> bool {
        let id = WaveId::Ch(c);
        let m = &self.m;
        // a source row counts while its analyser is on the screen; the ones at rest only hold a default
        let spec = m.view == AView::Spectrum || m.pane == tables::PANE_SPECTRUM;
        let (eye, power, bus) = (m.view == AView::Eye, m.view == AView::Power, m.bus.on);
        let mut used = vec![m.trigger_wave()];
        if spec { used.push(m.spec_src); }
        if eye { used.extend([Some(m.eye_src), m.eye_b, m.eye_rail].into_iter().flatten()); }
        if power { used.extend([Some(m.pw_v), Some(m.pw_i), m.pw_vin, m.pw_clk].into_iter().flatten()); }
        if bus { used.extend([m.bus.clk, m.bus.data]); }
        used.contains(&id)
            || m.meas.iter().any(|&(_, s)| s == id)
            || m.meas2.iter().any(|&(_, a, b)| a == id || b == id)
            || m.cat.iter().any(|&(_, a, b, c2, d)| [a, b, c2, d].contains(&id))
    }

    /// A channel given to a source row (Spectrum, Eye, Bus, Power) that is off is turned on, with a toast. The channel
    /// is remembered as turned on by a row, so that it goes off again when the row moves to another source.
    pub(super) fn source_turn_on(&mut self, id: WaveId) {
        if let WaveId::Ch(c) = id {
            if c < NCH && !self.m.ch[c].on {
                self.select_option(Item::ChOn(c), "On");
                if self.m.ch[c].on {
                    self.auto_on.push(c);
                    self.toast(&format!("CH{} turned on", c + 1));
                }
            }
        }
    }

    /// A row has left `old`: a channel the rows turned on by themselves goes off again unless something still uses it.
    pub(super) fn source_release(&mut self, old: Option<WaveId>) {
        if let Some(WaveId::Ch(c)) = old {
            if c < NCH && self.auto_on.contains(&c) && self.m.ch[c].on && !self.channel_in_use(c) {
                self.select_option(Item::ChOn(c), "Off");
            }
        }
        self.auto_on.retain(|&c| c < NCH && self.m.ch[c].on);
    }

    /// A source row moves to `next` (None = Off): only `next` is turned on, and the channel the row left is given back.
    /// The caller has already stored `next`.
    pub(super) fn source_moved(&mut self, old: Option<WaveId>, next: Option<WaveId>) {
        if old != next { self.source_release(old); }
        if let Some(id) = next { self.source_turn_on(id); }
    }

    /// Gives a Spectrum / Eye / Power source row `next` straight away (never by stepping through the sources in
    /// between). Only the chosen channel is turned on.
    pub(super) fn ana_set_source(&mut self, it: Item, next: Option<WaveId>) {
        let old = self.source_item_value(it);
        match (it, next) {
            (Item::SpecSrc, Some(n)) => self.m.spec_src = n,
            (Item::EyeSrc, Some(n)) => {
                self.m.eye_src = n;
                self.eye_src_chosen = true;
                if self.m.eye_from_record { self.m.eye_rate = self.waveform_bit_rate(n).unwrap_or(0.0); }
                self.azr.eye = None;
                self.azr.eye_ref = None;
                self.azr.eye_res = None;
                self.azr.page = super::eyepage::Page::default();
            }
            (Item::PowV, Some(n)) => self.m.pw_v = n,
            (Item::PowI, Some(n)) => self.m.pw_i = n,
            (Item::EyeB, n) => self.m.eye_b = n,
            (Item::EyeRail, n) => self.m.eye_rail = n,
            (Item::PowVin, n) => self.m.pw_vin = n,
            (Item::PowClk, n) => self.m.pw_clk = n,
            _ => return,
        }
        self.source_moved(old, next);
        if matches!(it, Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI) {
            self.azr.spec_avg = None;
            if let Some(w) = &mut self.azr.wf { w.clear(); }
        }
        self.analyzer_update();
    }

    /// Gives the Bus clock or data row `next` straight away.
    pub(super) fn bus_set_source(&mut self, it: Item, next: WaveId) {
        let old = self.source_item_value(it);
        if it == Item::BusClk { self.m.bus.clk = next; } else { self.m.bus.data = next; }
        self.source_moved(old, Some(next));
    }

    /// A choice made in a picker for a source row is set as it is, not reached by stepping from the old one. False
    /// when `it` is not a source row.
    pub(super) fn pick_source_direct(&mut self, it: Item, want: &str) -> bool {
        let opt = matches!(it, Item::EyeB | Item::EyeRail | Item::PowVin | Item::PowClk);
        let analyser = opt || matches!(it, Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI);
        if !analyser && !matches!(it, Item::BusClk | Item::BusData) { return false; }
        let ids = if analyser { self.wave_ids() } else { self.bus_ids() };
        let found = ids.into_iter().find(|&id| self.wave_name(id) == want);
        let next = match (found, opt && want == "Off") { (Some(id), _) => Some(id), (None, true) => None, _ => return false };
        if analyser { self.ana_set_source(it, next); } else if let Some(id) = next { self.bus_set_source(it, id); }
        true
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
