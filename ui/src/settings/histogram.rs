// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The waveform histogram of the Display menu (ui/setting_items.rs) and its command `:HISTogram:TYPE`
//! (scpi/histogram.rs). A value the command does not take is refused with -224. The slice is the place of
//! `store::setup_text` that writes it, so the keys of a setup file keep their order.
//!
//! Still written out by hand: `:HISTogram:ENABle` (a switch on the same setting: on keeps the direction and starts
//! with Vertical from Off), and `:HISTogram:SOURce` (the measurement source; setting it switches the channel on).

use super::{CustomIo, Scpi, Setting, Ty, Val};
use crate::model::Hist;

/// The slice, for the docs table and the SCPI handler.
pub const SLICES: [&[Setting]; 1] = [&HIST];

/// The histograms by their place in the menu.
const ALL: [Hist; 3] = [Hist::Off, Hist::Vertical, Hist::Horizontal];

/// Written after `z_src`.
pub const HIST: [Setting; 1] = [Setting {
    key: "hist",
    scpi: Scpi::Cmd("TYPE"),
    ty: Ty::Custom(&HIST_IO),
    label: "Histogram",
    #[cfg(test)]
    doc: "The waveform histogram of the Display menu: off, vertical or horizontal.",
    accept: None,
    step: None,
    get: |m, _| Val::Index(ALL.iter().position(|&h| h == m.hist).unwrap_or(0)),
    put: |m, _, v| m.hist = ALL[v.index()],
}];

/// `Off`, `Vertical` or `Horizontal` in a setup; over SCPI `HORizontal` or `VERTical` in the long or the short form
/// (Off only through `:HISTogram:ENABle`); the query answers `HOR`, else `VERT`.
const HIST_IO: CustomIo = CustomIo {
    store: |v| format!("{:?}", ALL[v.index()]),
    load: |t| {
        ALL.iter().position(|h| format!("{:?}", h) == t).map(Val::Index).ok_or_else(|| format!("unknown value: {}", t))
    },
    query: |v| if ALL[v.index()] == Hist::Horizontal { "HOR" } else { "VERT" }.to_string(),
    take: |a| {
        if crate::words::kw(a, "HORizontal") {
            Some(Val::Index(2))
        } else if crate::words::kw(a, "VERTical") {
            Some(Val::Index(1))
        } else {
            None
        }
    },
    labels: &["Off", "Vertical", "Horizontal"],
    #[cfg(test)]
    values: "`HORizontal`, `VERTical`; the query answers `HOR` or `VERT` (`VERT` while it is off)",
};
