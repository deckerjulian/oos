// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings of the eye and jitter page: their rows in the Eye menu (ui/setting_items.rs, where a change starts the
//! averaging again and updates the page, and a new place moves the eye, as before the table). No command sets them
//! yet; `:EYE:ITEM?` reads the figures (scpi/analysis.rs).
//!
//! `eye_place` is a key of its own, written by its slice where the key was. The other rows are fields of the setup
//! line `eye_more = <ber> <pn low> <pn high> <npj> <rail> <notch> <clock> <highpass>`, which store.rs writes and reads
//! by hand, its numbers with `store::p_f64`: the line stops at its first wrong field.
//!
//! Still written out by hand: the `eye` line (the source, which the menu offers from the waves the screen shows; the
//! bit rate, whose menu value says whether it was measured from the record, and a step clears that), the rail and
//! the clock (optional waves, `-` for none).

use super::{spectrum::freq_step, Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{si_short, step_index, EyePlace};

/// The slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 2] = [&PLACE, &MORE];

/// Why the rows have no command.
#[cfg(test)]
const NONE: &str = "none yet: the page is set in its menu and by a setup file";

/// The places by their place in the menu.
const PLACES: [EyePlace; 3] = [EyePlace::Full, EyePlace::Side, EyePlace::Under];

/// Written after `eye`.
pub const PLACE: [Setting; 1] = [Setting {
    key: "eye_place",
    scpi: Scpi::None {
        #[cfg(test)]
        why: NONE,
    },
    ty: Ty::Choice(
        &[
            Choice { store: "Full", scpi: "", label: "Full" },
            Choice { store: "Side", scpi: "", label: "Side" },
            Choice { store: "Under", scpi: "", label: "Under" },
        ],
        Unknown::Value,
        Arg::Keyword,
    ),
    label: "Place",
    #[cfg(test)]
    doc: "Where the eye is drawn: over the whole plot, beside the trace, or under it (beside it when there is no \
          room under it). A choice also brings a floated eye back into its place.",
    accept: None,
    step: None,
    get: |m, _| Val::Index(PLACES.iter().position(|&p| p == m.eye_place).unwrap_or(0)),
    put: |m, _, v| m.eye_place = PLACES[v.index()],
}];

/// The bit error ratios the menu offers for the total jitter.
const BERS: [f64; 5] = [1e-3, 1e-6, 1e-9, 1e-12, 1e-15];

/// One step of a frequency of the phase noise band: 1-2-5 from 1 Hz.
fn band_step(f: f64, d: i32) -> f64 {
    freq_step(f, d).max(1.0)
}

/// The fields of the `eye_more` line.
pub const MORE: [Setting; 6] = [
    Setting {
        key: "eye_more ber",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| format!("{:.0e}", x) },
        label: "BER",
        #[cfg(test)]
        doc: "The bit error ratio the total jitter is given for. The menu offers 1e-3 to 1e-15, a factor of 1000 a \
              step; a value on none starts from 1e-12.",
        accept: None,
        step: Some(|m, _, d| {
            let i = BERS.iter().position(|&x| (x - m.eye_ber).abs() < x * 0.01).unwrap_or(3);
            Val::F64(BERS[step_index(i, BERS.len(), d)])
        }),
        get: |m, _| Val::F64(m.eye_ber),
        put: |m, _, v| m.eye_ber = v.real(),
    },
    Setting {
        key: "eye_more pn_lo",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "Hz") },
        label: "Pn low",
        #[cfg(test)]
        doc: "The lower end of the band of the phase noise. The menu steps 1-2-5 from 1 Hz.",
        accept: None,
        step: Some(|m, _, d| Val::F64(band_step(m.eye_pn_lo, d))),
        get: |m, _| Val::F64(m.eye_pn_lo),
        put: |m, _, v| m.eye_pn_lo = v.real(),
    },
    Setting {
        key: "eye_more pn_hi",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "Hz") },
        label: "Pn high",
        #[cfg(test)]
        doc: "The upper end of the band of the phase noise. The menu steps 1-2-5 from 1 Hz.",
        accept: None,
        step: Some(|m, _, d| Val::F64(band_step(m.eye_pn_hi, d))),
        get: |m, _| Val::F64(m.eye_pn_hi),
        put: |m, _, v| m.eye_pn_hi = v.real(),
    },
    Setting {
        key: "eye_more npj",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Bool,
        label: "NPJ",
        #[cfg(test)]
        doc: "The figure NPJ of the jitter model on the page; off leaves it out.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.eye_npj),
        put: |m, _, v| m.eye_npj = v.index() != 0,
    },
    Setting {
        key: "eye_more notch",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| if x > 0.0 { si_short(x, "Hz") } else { "off".into() } },
        label: "Notch",
        #[cfg(test)]
        doc: "A frequency that is fitted as a sine and taken out of the TIE trend before the eye is measured again \
              (power-supply-induced jitter), off at 0. The menu steps 1-2-5 from 0.",
        accept: None,
        step: Some(|m, _, d| Val::F64(freq_step(m.eye_notch, d))),
        get: |m, _| Val::F64(m.eye_notch),
        put: |m, _, v| m.eye_notch = v.real(),
    },
    Setting {
        key: "eye_more hp",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Bool,
        label: "Highpass",
        #[cfg(test)]
        doc: "The 30 kHz high-pass on the common mode before its peak-to-peak is measured.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.eye_hp),
        put: |m, _, v| m.eye_hp = v.index() != 0,
    },
];
