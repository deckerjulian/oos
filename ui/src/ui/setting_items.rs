// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The menu rows of the settings table (settings.rs): kind, label, value, options and a step, from the row of each
//! item. What a change sets off besides `settings_changed` is here, per item.

use super::*;
use crate::settings::Setting;

/// The items the table answers for, by setup key.
const ITEMS: [(Item, &str); 81] = [
    (Item::Persist, "persist"),
    (Item::Intensity, "intensity"),
    (Item::GridStyle, "grid"),
    (Item::GridBright, "grid_bright"),
    (Item::GridFrame, "grid_frame"),
    (Item::ColorGrade, "color_grade"),
    (Item::Dots, "dots"),
    (Item::BottomBar, "bar"),
    (Item::TextSize, "text_size"),
    (Item::TrigSlope, "slope"),
    (Item::TrigMode, "trig_mode"),
    (Item::TrigHoldoff, "holdoff"),
    (Item::TrigNoise, "trig_nrej"),
    (Item::TrigAutoNoise, "auto_nrej"),
    (Item::TrigCoup, "trig_coup"),
    (Item::TrigSlope2, "trig_slope2"),
    (Item::TrigPol, "trig_pol"),
    (Item::TrigT1, "trig_t1"),
    (Item::TrigT2, "trig_t2"),
    (Item::VidStd, "video_std"),
    (Item::VidSync, "video_sync"),
    (Item::VidLine, "video_line"),
    (Item::WinModeI, "win_mode"),
    (Item::NthIdle, "nth_idle"),
    (Item::NthN, "nth_n"),
    (Item::Setup, "setup"),
    (Item::Hold, "hold"),
    (Item::AcqMode, "acq"),
    (Item::AcqCores, "cores"),
    (Item::AcqAvg, "avg_count"),
    (Item::AcqAvgHw, "avg_hw"),
    (Item::AcqSegHw, "seg_hw"),
    (Item::FftWin, "fft_win"),
    (Item::MathFc, "math_fc"),
    (Item::MathBw, "math_bw"),
    (Item::DvmOn, "dvm on"),
    (Item::DvmMode, "dvm mode"),
    (Item::CntOn, "counter on"),
    (Item::CntMode, "counter mode"),
    (Item::CntDigits, "counter digits"),
    (Item::ChFine, "fine_v"),
    (Item::ChExpand, "v_expand_gnd"),
    (Item::MaskOn, "mask_on"),
    (Item::MaskAction, "mask_stop"),
    (Item::MaskInvert, "mask_inv"),
    (Item::Histogram, "hist"),
    (Item::BodeMode, "bode_mode"),
    (Item::BodeStart, "bode start"),
    (Item::BodeStop, "bode stop"),
    (Item::BodePoints, "bode points"),
    (Item::BodeAmp, "bode amp"),
    (Item::BodeLink, "bode_link"),
    (Item::BodeSwTime, "bode_sweep time"),
    (Item::BodeSwLog, "bode_sweep log"),
    (Item::BodeSyncEdge, "bode_sweep edge"),
    (Item::BodeSyncLvl, "bode_sweep level"),
    (Item::BodeRef, "bode_sweep ref"),
    (Item::LaOn, "la on"),
    (Item::SpecCenter, "spec center"),
    (Item::SpecWin, "spec win"),
    (Item::SpecTrace, "spec trace"),
    (Item::SpecRef, "spec ref"),
    (Item::SpecWf, "spec wf"),
    (Item::SpecDist, "spec dist"),
    (Item::SpecAcprW, "spec_acpr width"),
    (Item::SpecAcprS, "spec_acpr spacing"),
    (Item::SpecMargin, "spec_margin"),
    (Item::EyePlace, "eye_place"),
    (Item::EyeBer, "eye_more ber"),
    (Item::EyePnLo, "eye_more pn_lo"),
    (Item::EyePnHi, "eye_more pn_hi"),
    (Item::EyeNpj, "eye_more npj"),
    (Item::EyeNotch, "eye_more notch"),
    (Item::EyeHp, "eye_more hp"),
    (Item::PowMode, "power mode"),
    (Item::PowFline, "power_more fline"),
    (Item::PowFsw, "power_more fsw"),
    (Item::HExpandI, "h_expand"),
    (Item::AutoRoll, "auto_roll"),
    (Item::MeasLabels, "meas_labels"),
    (Item::StripDock, "strip_dock"),
];

/// The item of instance n of a numbered family.
type Instance = fn(usize) -> Item;

/// The items of a numbered family, by the item of an instance and the row's key.
const NUMBERED: [(Instance, &str); 4] = [
    (Item::ChOn, "ch<n>.on"),
    (Item::ChBw, "ch<n>.bw"),
    (Item::ChInvert, "ch<n>.invert"),
    (Item::ChUnit, "ch<n>.unit"),
];

/// The row of an item and the index of its instance (0 for a row that is not numbered).
fn row(it: Item) -> Option<(&'static Setting, usize)> {
    let numbered = || NUMBERED.iter().find_map(|&(f, key)| (0..NCH).find(|&n| f(n) == it).map(|n| (key, n)));
    let (key, n) = ITEMS.iter().find(|(i, _)| *i == it).map(|&(_, key)| (key, 0)).or_else(numbered)?;
    crate::settings::find(key).map(|s| (s, n))
}

impl Ui {
    pub(super) fn setting_kind(it: Item) -> Option<Kind> {
        row(it).map(|(s, _)| if s.is_number() { Kind::Value } else { Kind::Choice })
    }

    pub(super) fn setting_label(it: Item) -> Option<&'static str> {
        row(it).map(|(s, _)| s.label)
    }

    pub(super) fn setting_value(&self, it: Item) -> Option<String> {
        row(it).map(|(s, n)| s.shown(&self.m, n))
    }

    pub(super) fn setting_options(&self, it: Item) -> Option<Vec<String>> {
        row(it).map(|(s, _)| s.options())
    }

    pub(super) fn setting_adjust(&mut self, it: Item, d: i32) -> bool {
        let Some((s, n)) = row(it) else { return false };
        let before = (s.get)(&self.m, n);
        s.step(&mut self.m, n, d);
        match it {
            // the statistics card follows the dock the strip uses; moving the strip does not change what was measured
            // (no `settings_changed`)
            Item::StripDock => {
                self.workspace.measure_docked = true;
                self.workspace.measure_float = None;
                self.relayout();
                return true;
            }
            // only the inputs the model has switch on
            Item::ChOn(c) => {
                self.m.ch[c].on &= c < n_ch();
                if self.m.ch[c].on && self.m.run == RunState::Run {
                    self.m.floor_live_tdiv();
                }
            }
            // only a model with the LA port switches it on
            Item::LaOn => self.m.la.on &= crate::ui::la::available(),
            // the persistence map of the old time does not hold for the new one
            Item::Persist => self.ph.clear(),
            Item::Dots | Item::BottomBar => self.dirty_map = true,
            // a switched-on test without a mask builds one from the source
            Item::MaskOn if self.m.mask_on && self.mask_sw.mask.is_none() => self.create_mask(),
            Item::MaskInvert => self.build_mask(),
            // the menu's Single also starts the acquisition
            Item::TrigMode if self.m.trig_mode == TrigMode::Single => self.m.run = RunState::Run,
            // the hardware switches of the Acquire menu leave the rest as it is (no `settings_changed`), as before
            // the table
            Item::AcqAvgHw => {
                if self.m.avg_hw && !board().avg_hw(&self.m) {
                    self.toast("HW average needs a count of 2..1024 and ordinary records: software average");
                }
                return true;
            }
            // the points of the last sweep are of the other mode; the Bode page leaves the rest as it is (no
            // `settings_changed`), as before the table
            Item::BodeMode => {
                if (s.get)(&self.m, n) != before {
                    self.bode.points.clear();
                    self.bode.cursor = 0;
                }
                return true;
            }
            // an analyser page starts its averaging again and leaves the rest as it is (no `settings_changed`), as
            // before the table
            Item::SpecCenter
            | Item::SpecWin
            | Item::SpecTrace
            | Item::SpecRef
            | Item::SpecWf
            | Item::SpecDist
            | Item::SpecAcprW
            | Item::SpecAcprS
            | Item::SpecMargin
            | Item::EyeBer
            | Item::EyePnLo
            | Item::EyePnHi
            | Item::EyeNpj
            | Item::EyeNotch
            | Item::EyeHp
            | Item::PowMode
            | Item::PowFline
            | Item::PowFsw => {
                self.ana_restart();
                return true;
            }
            // a new place moves the eye: `eye_set_place` compares it with the old one, and keeps the eye beside the
            // trace when there is no room under it; Full, Side and Under also bring a floated eye back into its place
            Item::EyePlace => {
                let place = self.m.eye_place;
                (s.put)(&mut self.m, n, before);
                self.workspace.eye_float = None;
                self.eye_set_place(place);
                return true;
            }
            // the rest of the Bode page, likewise
            Item::BodeStart
            | Item::BodeStop
            | Item::BodePoints
            | Item::BodeAmp
            | Item::BodeLink
            | Item::BodeSwTime
            | Item::BodeSwLog
            | Item::BodeSyncEdge
            | Item::BodeSyncLvl
            | Item::BodeRef => return true,
            Item::AcqSegHw => {
                // the other limit applies from now on
                self.m.segments = self.seg_count();
                return true;
            }
            _ => {}
        }
        self.settings_changed();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The item is in the Display menu, in the Horizontal menu, in the Measure settings, in the DVM menu, in the Mask
    /// menu, in the LA menu, in a menu of an analyser page, in the Acquire menu (in Average mode with segments, so their
    /// rows show; HW average shows only with a hardware source, which the demo is not), in a Channel menu or its More
    /// page, in the Bode menu of a generator, in the Math menu of an operator, or in the Trigger menu of a trigger type (with a time range, so both limits
    /// show, and the video trigger on line n, so its line shows).
    fn in_a_menu(it: Item) -> bool {
        let mut u = crate::ui::testkit::demo_ui();
        (u.m.acq, u.m.segments) = (AcqMode::Average, 10);
        let channels = (0..NCH).flat_map(|c| [MenuId::Channel(c), MenuId::ChMore(c)]);
        if it == Item::AcqAvgHw
            || [
                MenuId::Display,
                MenuId::Horizontal,
                MenuId::Measure,
                MenuId::Dvm,
                MenuId::Acquire,
                MenuId::Mask,
                MenuId::La,
                MenuId::Spectrum,
                MenuId::Eye,
            ]
            .into_iter()
            .chain(channels)
            .any(|m| u.items(m).contains(&it))
        {
            return true;
        }
        // the rows of each page of the power analysis
        if crate::settings::power::MODES.iter().any(|&p| {
            u.m.pw_mode = p;
            u.items(MenuId::Power).contains(&it)
        }) {
            return true;
        }
        // the free sweep's rows (with no input channel, so the reference level shows), the connection of an external
        // generator
        u.m.bode_sw_in = 0;
        if (0..crate::model::bode::GENERATORS.len()).any(|g| {
            u.m.bode_gen = g;
            u.items(MenuId::Bode).contains(&it)
        }) {
            return true;
        }
        if crate::math::MATH_OPS.iter().any(|&op| {
            u.m.math = op;
            u.items(MenuId::Math).contains(&it)
        }) {
            return true;
        }
        (u.m.trig_when, u.m.video_sync) = (WhenKind::In, crate::model::VSync::Line);
        TRIG_TYPES.iter().any(|&t| {
            u.m.trig_type = t;
            u.items(MenuId::Trigger).contains(&it)
        })
    }

    #[test]
    fn every_row_has_its_item_and_a_step_changes_it() {
        for s in crate::settings::all() {
            // a numbered row by the item of its first instance
            let it = ITEMS.iter().find(|(_, k)| *k == s.key).map(|(i, _)| *i);
            let it = it.or_else(|| NUMBERED.iter().find(|(_, k)| *k == s.key).map(|(f, _)| f(0)));
            let it = it.unwrap_or_else(|| panic!("{}: no menu item in ITEMS or NUMBERED", s.key));
            assert!(in_a_menu(it), "{:?} is in no menu", it);
            let mut u = crate::ui::testkit::demo_ui();
            assert_eq!(u.item_label(it), s.label);
            let before = u.value(it);
            u.adjust(it, 1);
            if u.value(it) == before {
                u.adjust(it, -1);
            }
            assert_ne!(u.value(it), before, "{:?}", it);
            assert_eq!(u.value(it), s.shown(&u.m, 0), "{:?}", it);
            if !s.is_number() {
                assert!(u.options(it).contains(&u.value(it)), "{:?}", it);
            }
        }
    }
}
