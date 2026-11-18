// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The acquisition settings: the mode, the averages, the ADC cores and the hardware switches of the Acquire menu.
//! Their commands are `:ACQuire:...` (the `:ACQuire` branches of scpi/dispatch.rs), their rows the Acquire menu
//! (ui/setting_items.rs). The slices are the places of `store::setup_text` that write them, so the keys of a setup
//! file keep their order.
//!
//! Still written out by hand: `segments` (the menu offers and shows what the source can record, `Ui::seg_options`,
//! and `:ACQuire:ULTRa:MAXFrame` sets it too, to the nearest menu step), `seg_interval` (the menu offers a list of
//! times, a setup and SCPI take any seconds within the range) and `mdepth` (its choices are the board's depths, and
//! `:ACQuire:MDEPth?` answers the depth of the record, not the setting).

use super::{Arg, Choice, CustomIo, Scpi, Setting, Ty, Unknown, Val};
use crate::model::AcqMode;

/// Both slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 2] = [&MODE, &SEG_HW];

/// The rows whose command refuses a value it does not take with `-224,"Illegal parameter value"`; a value the other
/// rows do not take falls through to the later branches of `exec_cmd`, which answer `-113`, as before the table.
pub const REFUSES: [&str; 2] = ["acq", "avg_count"];

/// Written after `hold`.
pub const MODE: [Setting; 4] = [
    Setting {
        key: "acq",
        scpi: Scpi::Cmd("TYPE"),
        ty: Ty::Choice(
            &[
                Choice { store: "Normal", scpi: "NORMal", label: "Normal" },
                Choice { store: "Peak", scpi: "PEAK", label: "Peak" },
                Choice { store: "Average", scpi: "AVERages", label: "Average" },
                Choice { store: "HiRes", scpi: "HRESolution", label: "High res" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Mode",
        #[cfg(test)]
        doc: "How a record is made: Normal, Peak, Average (over the count of `avg_count`) or High res (in software, \
              from more samples than screen points).",
        accept: None,
        step: None,
        get: |m, _| Val::Index(ACQ_MODES.iter().position(|&a| a == m.acq).unwrap_or(0)),
        put: |m, _, v| m.acq = ACQ_MODES[v.index()],
    },
    Setting {
        key: "cores",
        scpi: Scpi::Cmd("CORes"),
        ty: Ty::Custom(&CORES_IO),
        label: "ADC cores",
        #[cfg(test)]
        doc: "Matched takes the offsets of the interleaved ADC cores off in software (docs/protocols/adc-mode.md 6); \
              Raw leaves the words as they come.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.cores),
        put: |m, _, v| m.cores = v.index() != 0,
    },
    Setting {
        key: "avg_count",
        scpi: Scpi::Cmd("AVERages"),
        ty: Ty::Steps(&crate::ui::AVERAGES),
        label: "Averages",
        #[cfg(test)]
        doc: "How many records the Average mode averages.",
        accept: None,
        step: None,
        get: |m, _| Val::U32(m.avg_count),
        put: |m, _, v| m.avg_count = v.index() as u32,
    },
    Setting {
        key: "avg_hw",
        scpi: Scpi::Cmd("AVERages:HARDware"),
        ty: Ty::Bool,
        label: "HW average",
        #[cfg(test)]
        doc: "Average mode: the FPGA averages where the board can, else the software does; experimental, not verified \
              on the unit (docs/fork/upstream-audit.md). The menu offers it in Average mode with a hardware source.",
        accept: None,
        step: None,
        get: |m, _| Val::Bool(m.avg_hw),
        put: |m, _, v| m.avg_hw = v.index() != 0,
    },
];

/// Written after `segments`.
pub const SEG_HW: [Setting; 1] = [Setting {
    key: "seg_hw",
    scpi: Scpi::Cmd("SEGMents:HARDware"),
    ty: Ty::Bool,
    label: "HW segments",
    #[cfg(test)]
    doc: "Segments recorded by the FPGA instead of the software history; experimental, not verified on the unit \
          (docs/fork/upstream-audit.md). The menu offers it while segments are on.",
    accept: None,
    step: None,
    get: |m, _| Val::Bool(m.seg_hw),
    put: |m, _, v| m.seg_hw = v.index() != 0,
}];

const ACQ_MODES: [AcqMode; 4] = [AcqMode::Normal, AcqMode::Peak, AcqMode::Average, AcqMode::HiRes];

/// A switch in a setup; over SCPI any word that starts with `MATC` or `RAW`, or `ON`, `OFF`, `1`, `0`, in any case.
const CORES_IO: CustomIo = CustomIo {
    store: |v| (v.index() != 0).to_string(),
    load: |t| crate::words::p_bool(t).map(Val::Bool),
    query: |v| if v.index() != 0 { "MATC" } else { "RAW" }.to_string(),
    take: |a| {
        let a = a.trim().to_ascii_uppercase();
        if a.starts_with("MATC") || a == "ON" || a == "1" {
            Some(Val::Bool(true))
        } else if a.starts_with("RAW") || a == "OFF" || a == "0" {
            Some(Val::Bool(false))
        } else {
            None
        }
    },
    labels: &["Raw", "Matched"],
    #[cfg(test)]
    values: "any word that starts with `MATC` or `RAW`, or `ON`, `OFF`, `1`, `0`; the query answers `MATC` or `RAW`",
};
