// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings of the power analysis page: their rows in the Power menu (ui/setting_items.rs, where a change starts
//! the averaging again and updates the page, as before the table). No command sets them yet; `:POWer:ITEM?` and
//! `:POWer:HARMonics?` read the results (scpi/dispatch.rs).
//!
//! The page keeps its settings in setup lines of several values (`power = <voltage> <current> <page>`, `power_more =
//! <line f> <switch f> <input> <reference> <clock>`), so the keys of the rows name a field of a line (`power mode`).
//! store.rs writes and reads the lines by hand, the numbers with `store::p_f64`: a line stops at its first wrong field.
//!
//! Still written out by hand: the voltage and the current (the menu offers the waves the screen shows), the input and
//! the clock (optional waves, `-` for none) and the reference of the rail (an optional value, `-` for the top of the
//! record, which the menu shows as Top).

use super::{spectrum::freq_step, Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{si_short, PowerMode};

/// The slice, for the docs table.
pub const SLICES: [&[Setting]; 1] = [&POWER];

/// Why the rows have no command.
#[cfg(test)]
const NONE: &str = "none yet: the page is set in its menu and by a setup file";

/// The pages by their place in the menu; a setup names them as here.
pub const MODES: [PowerMode; 6] = [
    PowerMode::Quality,
    PowerMode::Harmonics,
    PowerMode::Switching,
    PowerMode::Ripple,
    PowerMode::Capacitance,
    PowerMode::Rail,
];

/// A frequency the page measures when it is 0.
fn auto_hz(f: f64) -> String {
    if f > 0.0 {
        si_short(f, "Hz")
    } else {
        "auto".into()
    }
}

/// The fields of the `power` and the `power_more` line.
pub const POWER: [Setting; 3] = [
    Setting {
        key: "power mode",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Choice(
            &[
                Choice { store: "Quality", scpi: "", label: "Quality" },
                Choice { store: "Harmonics", scpi: "", label: "Harmonics" },
                Choice { store: "Switching", scpi: "", label: "Switching" },
                Choice { store: "Ripple", scpi: "", label: "Ripple" },
                Choice { store: "Capacitance", scpi: "", label: "Cin" },
                Choice { store: "Rail", scpi: "", label: "Rail" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Page",
        #[cfg(test)]
        doc: "What the page analyses: the power quality, the harmonics, the switching losses, the ripple, the input \
              capacitance or a supply rail.",
        accept: None,
        step: None,
        get: |m, _| Val::Index(MODES.iter().position(|&p| p == m.pw_mode).unwrap_or(0)),
        put: |m, _, v| m.pw_mode = MODES[v.index()],
    },
    Setting {
        key: "power_more fline",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: auto_hz },
        label: "Line f",
        #[cfg(test)]
        doc: "The line frequency of the pages that need one, measured from the record when 0 (auto). The menu \
              steps 1-2-5 from 0.",
        accept: None,
        step: Some(|m, _, d| Val::F64(freq_step(m.pw_fline, d))),
        get: |m, _| Val::F64(m.pw_fline),
        put: |m, _, v| m.pw_fline = v.real(),
    },
    Setting {
        key: "power_more fsw",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: auto_hz },
        label: "Switch f",
        #[cfg(test)]
        doc: "The switching frequency of the ripple page, measured from the record when 0 (auto). The menu steps \
              1-2-5 from 0.",
        accept: None,
        step: Some(|m, _, d| Val::F64(freq_step(m.pw_fsw, d))),
        get: |m, _| Val::F64(m.pw_fsw),
        put: |m, _, v| m.pw_fsw = v.real(),
    },
];
