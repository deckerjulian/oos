// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings of the spectrum page: their rows in the Spectrum menu (ui/setting_items.rs, where a change starts the
//! averaging and the waterfall again and updates the page, as before the table). No command sets them yet.
//!
//! The page keeps most of them in setup lines of several values (`spec = <source> <centre> <span> <rbw> <window>
//! <trace> <ref> <waterfall> <thd>`, `spec_acpr = <width> <spacing>`), so the keys of those rows name a field of a
//! line (`spec center`). store.rs writes and reads the lines by hand, its numbers with `store::p_f64`: a line sets
//! none of its fields after the first that is wrong. `spec_margin` is a key of its own, written by its slice.
//!
//! Still written out by hand: the source (the menu offers the waves the screen shows), the span and the RBW (the menu
//! shows what the last spectrum used when they are on Auto), the harmonics of the pane and the order of the peaks
//! (`spec_peaks`; the menu's names come from the pane).

use super::{math, Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::math::WINDOWS;
use crate::model::{decades, ladder_step, si_short};

/// The slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 3] = [&SPEC, &ACPR, &MARGIN];

/// Why the rows have no command.
#[cfg(test)]
const NONE: &str = "none yet: the page is set in its menu and by a setup file";

/// One step of a frequency on the 1-2-5 ladder from 1 Hz to 5 GHz with 0 in front (full span, Auto RBW); a value on
/// no step starts from 0.
pub fn freq_step(f: f64, d: i32) -> f64 {
    ladder_step(&decades(0, 10, true), f, d, 0)
}

fn hz(f: f64) -> String {
    si_short(f, "Hz")
}

/// The fields of the `spec` line.
pub const SPEC: [Setting; 6] = [
    Setting {
        key: "spec center",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: hz },
        label: "Centre",
        #[cfg(test)]
        doc: "The centre of the band the page shows (with a span), and the frequency Centre on peak sets. The menu \
              steps 1-2-5.",
        accept: None,
        step: Some(|m, _, d| Val::F64(freq_step(m.spec_center, d))),
        get: |m, _| Val::F64(m.spec_center),
        put: |m, _, v| m.spec_center = v.real(),
    },
    Setting {
        key: "spec win",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Choice(&math::WINDOWS_CHOICES, Unknown::Value, Arg::Keyword),
        label: "Window",
        #[cfg(test)]
        doc: "The window the record is multiplied with before the transform.",
        accept: None,
        step: None,
        get: |m, _| Val::Index(WINDOWS.iter().position(|&w| w == m.spec_win).unwrap_or(0)),
        put: |m, _, v| m.spec_win = WINDOWS[v.index()],
    },
    Setting {
        key: "spec trace",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        // the line keeps the number of the choice; no command takes them yet
        ty: Ty::Choice(
            &[
                Choice { store: "0", scpi: "", label: "Normal" },
                Choice { store: "1", scpi: "", label: "Average" },
                Choice { store: "2", scpi: "", label: "Max hold" },
                Choice { store: "3", scpi: "", label: "Min hold" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Trace",
        #[cfg(test)]
        doc: "What the trace shows of the spectra one after the other: the last one, their average, or the largest or \
              smallest value of each frequency.",
        accept: None,
        step: None,
        get: |m, _| Val::Index(m.spec_trace.min(3)),
        put: |m, _, v| m.spec_trace = v.index(),
    },
    Setting {
        key: "spec ref",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| format!("{:.0} dBV", x) },
        label: "Ref level",
        #[cfg(test)]
        doc: "The level at the top of the plot, in dBV. The menu steps by 5 dB.",
        accept: None,
        step: Some(|m, _, d| Val::F32(m.spec_ref + 5.0 * d as f32)),
        get: |m, _| Val::F32(m.spec_ref),
        put: |m, _, v| m.spec_ref = v.secs(),
    },
    Setting {
        key: "spec wf",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Bool,
        label: "Waterfall",
        #[cfg(test)]
        doc: "The spectra one after the other as a waterfall under the trace.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.spec_wf),
        put: |m, _, v| m.spec_wf = v.index() != 0,
    },
    Setting {
        key: "spec dist",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Bool,
        label: "THD / SNR",
        #[cfg(test)]
        doc: "The distortion figures of the largest component (THD, THD+N, SNR, SINAD, SFDR, ENOB) on the page.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.spec_dist),
        put: |m, _, v| m.spec_dist = v.index() != 0,
    },
];

/// One step of a channel width or spacing of the ACPR: 1-2-5 from 1 Hz.
fn acpr_step(f: f64, d: i32) -> f64 {
    freq_step(f.max(1.0), d).max(1.0)
}

/// The fields of the `spec_acpr` line.
pub const ACPR: [Setting; 2] = [
    Setting {
        key: "spec_acpr width",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: hz },
        label: "Ch width",
        #[cfg(test)]
        doc:
            "The width of the main channel and of its neighbours for the adjacent channel power ratio. The menu steps \
              1-2-5 from 1 Hz.",
        accept: None,
        step: Some(|m, _, d| Val::F64(acpr_step(m.spec_acpr_w, d))),
        get: |m, _| Val::F64(m.spec_acpr_w),
        put: |m, _, v| m.spec_acpr_w = v.real(),
    },
    Setting {
        key: "spec_acpr spacing",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: hz },
        label: "Ch space",
        #[cfg(test)]
        doc: "The distance from the centre of the main channel to the centres of its neighbours. The menu steps 1-2-5 \
              from 1 Hz.",
        accept: None,
        step: Some(|m, _, d| Val::F64(acpr_step(m.spec_acpr_s, d))),
        get: |m, _| Val::F64(m.spec_acpr_s),
        put: |m, _, v| m.spec_acpr_s = v.real(),
    },
];

/// Written after `log_ivl`.
pub const MARGIN: [Setting; 1] = [Setting {
    key: "spec_margin",
    scpi: Scpi::None {
        #[cfg(test)]
        why: NONE,
    },
    ty: Ty::Real { range: Some((0.0, 60.0)), shown: |x| format!("{:.0} dB", x) },
    label: "Limit margin",
    #[cfg(test)]
    doc: "How far above the trace (its highest level nearby) Limit from trace puts the limit line, 0 to 60 dB.",
    accept: None,
    step: Some(|m, _, d| Val::F32((m.spec_margin + d as f32).clamp(0.0, 60.0))),
    get: |m, _| Val::F32(m.spec_margin),
    put: |m, _, v| m.spec_margin = v.secs(),
}];
