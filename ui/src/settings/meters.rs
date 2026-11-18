// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The switches and modes of the DVM and the counter. Their commands are `:DVM:...` (scpi/dvm.rs) and `:COUNter:...`
//! (scpi/counter.rs), their rows the menu DVM and counter (ui/setting_items.rs). A value a command does not take is
//! refused with -224.
//!
//! Each meter keeps its settings in one setup line (`dvm = <on> <source> <mode>`, `counter = <on> <source> <mode>
//! <digits>`), so the keys of the rows name a field of that line (`dvm mode`). store.rs writes and reads the two lines
//! by hand: a line sets none of its fields when one of on, source and mode is wrong, and the counter's digits are read
//! after them, so a wrong count keeps the other three. Also by hand: the sources (`:SOURce` takes the channels, the
//! menu offers the ones the unit has, or the references while it measures them offline).

use super::{Arg, Choice, CustomIo, Scpi, Setting, Ty, Unknown, Val};
use crate::ui::{CNT_MODES, DVM_MODES};

/// The rows of `:DVM`, for the docs table and the SCPI handler.
pub const DVM_SLICES: [&[Setting]; 1] = [&DVM];
/// The rows of `:COUNter`.
pub const COUNTER_SLICES: [&[Setting]; 1] = [&COUNTER];

pub const DVM: [Setting; 2] = [
    Setting {
        key: "dvm on",
        scpi: Scpi::Cmd("ENABle"),
        ty: Ty::Bool,
        label: "DVM",
        #[cfg(test)]
        doc: "The voltmeter's reading of its source in the readouts; from the DVM block where it is switched on and \
              the board has one (docs/fork/dvm.md), else from the measurements of the records.",
        accept: None,
        get: |m, _| Val::Bool(m.dvm_on),
        put: |m, _, v| m.dvm_on = v.index() != 0,
    },
    Setting {
        key: "dvm mode",
        scpi: Scpi::Cmd("MODE"),
        ty: Ty::Custom(&DVM_MODE_IO),
        label: "DVM mode",
        #[cfg(test)]
        doc: "What the voltmeter reads: the mean (DC), the rms without the mean (AC rms) or the whole rms (AC+DC rms).",
        accept: None,
        get: |m, _| Val::Index(m.dvm_mode.min(2)),
        put: |m, _, v| m.dvm_mode = v.index(),
    },
];

pub const COUNTER: [Setting; 3] = [
    Setting {
        key: "counter on",
        scpi: Scpi::Cmd("ENABle"),
        ty: Ty::Bool,
        label: "Counter",
        #[cfg(test)]
        doc: "The counter's reading of its source in the readouts (docs/protocols/counter.md).",
        accept: None,
        get: |m, _| Val::Bool(m.cnt_on),
        put: |m, _, v| m.cnt_on = v.index() != 0,
    },
    Setting {
        key: "counter mode",
        scpi: Scpi::Cmd("MODE"),
        ty: Ty::Choice(
            &[
                Choice { store: "0", scpi: "FREQuency", label: CNT_MODES[0] },
                Choice { store: "1", scpi: "PERiod", label: CNT_MODES[1] },
                Choice { store: "2", scpi: "TOTalize", label: CNT_MODES[2] },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Count mode",
        #[cfg(test)]
        doc: "What the counter shows: the frequency, the period, or the edges counted since `:COUNter:TOTalize:CLEar`.",
        accept: None,
        get: |m, _| Val::Index(m.cnt_mode.min(2)),
        put: |m, _, v| m.cnt_mode = v.index(),
    },
    Setting {
        key: "counter digits",
        scpi: Scpi::Cmd("NDIGits"),
        ty: Ty::U32 { min: 3, max: 7, step: 1, unit: "" },
        label: "Digits",
        #[cfg(test)]
        doc: "The digits of the counter's reading.",
        accept: None,
        get: |m, _| Val::U32(m.cnt_digits),
        put: |m, _, v| m.cnt_digits = v.index() as u32,
    },
];

/// The answers of `:DVM:MODE?`.
const DVM_NAMES: [&str; 3] = ["DC", "ACRMS", "ACDC"];

/// Its place in the setup line; over SCPI `DC`, `ACRMs`, `ACDC` or `ACDCrms` in the long or the short form, and
/// `ACRMS`; the query answers `DC`, `ACRMS` or `ACDC`.
const DVM_MODE_IO: CustomIo = CustomIo {
    store: |v| v.index().to_string(),
    load: |t| crate::store::p_idx(t, DVM_NAMES.len()).map(Val::Index),
    query: |v| DVM_NAMES[v.index()].to_string(),
    take: |a| {
        let kw = crate::scpi::kw;
        if kw(a, "DC") {
            Some(Val::Index(0))
        } else if kw(a, "ACRMs") || a.eq_ignore_ascii_case("ACRMS") {
            Some(Val::Index(1))
        } else if kw(a, "ACDC") || kw(a, "ACDCrms") {
            Some(Val::Index(2))
        } else {
            None
        }
    },
    labels: &DVM_MODES,
    #[cfg(test)]
    values: "`DC`, `ACRMs`, `ACDCrms` (also `ACDC`); the query answers `DC`, `ACRMS` or `ACDC`",
};
