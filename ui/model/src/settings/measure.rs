// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! How the measurements show: the labels on the trace and where the strip of results docks. Their commands are
//! `:MEASure:...` (scpi/measure.rs), their rows the Measure settings menu (ui/setting_items.rs).
//!
//! Still written out by hand: the selected measurements and their sources (wave sources), the statistics switch (not
//! stored), the gate (`meas_full_record`: over SCPI it also clears the statistics) and the parameters of
//! `meas_cfg` (one setup line of several values).

use super::{Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{step_index, StripDock};

/// The one slice, in the order of the docs table.
pub const SLICES: [&[Setting]; 1] = [&VIEW];

/// The row of `:MEASure:INDicator`, which `:CURSor:MEASure:INDicator` sets too.
pub const INDICATOR: &Setting = &VIEW[0];

/// Written after `pane`.
pub const VIEW: [Setting; 2] = [
    Setting {
        key: "meas_labels",
        scpi: Scpi::Cmd("INDicator"),
        ty: Ty::Bool,
        label: "Labels on trace",
        #[cfg(test)]
        doc: "Draws the labels of the measurements on the trace; `:CURSor:MEASure:INDicator[?]` is the same switch.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.meas_labels),
        put: |m, _, v| m.meas_labels = v.index() != 0,
    },
    Setting {
        key: "strip_dock",
        scpi: Scpi::None {
            #[cfg(test)]
            why: "a choice of the screen's layout, not of the measurement",
        },
        ty: Ty::Choice(
            &[
                Choice { store: "Under", scpi: "", label: "Under" },
                Choice { store: "Left", scpi: "", label: "Left" },
                Choice { store: "Right", scpi: "", label: "Right" },
                Choice { store: "Above", scpi: "", label: "Above" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Dock",
        #[cfg(test)]
        doc: "Where the strip of results docks to the plot; the menu steps to the end of the list and stops.",
        accept: None,
        step: Some(|m, _, d| {
            let i = StripDock::ALL.iter().position(|&s| s == m.strip_dock).unwrap_or(0);
            Val::Index(step_index(i, StripDock::ALL.len(), d))
        }),
        get: |m, _| Val::Index(StripDock::ALL.iter().position(|&s| s == m.strip_dock).unwrap_or(0)),
        put: |m, _, v| m.strip_dock = StripDock::ALL[v.index()],
    },
];
