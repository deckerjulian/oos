// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The time base settings of one value each: the point the scale expands around and auto roll. Their commands are
//! `:TIMebase:...` (scpi/timebase.rs), their rows the Horizontal menu (ui/setting_items.rs). The two slices are the two
//! places of `store::setup_text` that write them, so the keys of a setup file keep their order.
//!
//! Still written out by hand: the scale and the delay (ladder indices and the zoom's limits), the fine switch (over
//! SCPI its Off also sets the fine factor back to 1, in the menu and a setup it does not) and the zoom (switching it
//! over SCPI sets the zoom's scale and delay).

use super::{CustomIo, Scpi, Setting, Ty, Val};
use crate::model::HExpand;

/// Both slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 2] = [&HREF, &ROLL];

/// Written after `v_expand_gnd`.
pub const HREF: [Setting; 1] = [Setting {
    key: "h_expand",
    scpi: Scpi::Cmd("HREFerence:MODE"),
    ty: Ty::Custom(&HREF_IO),
    label: "Expand around",
    #[cfg(test)]
    doc: "The point a change of the time scale keeps in place: the centre of the screen, its left or right edge, or \
          the trigger.",
    accept: None,
    step: None,
    get: |m, _| Val::Index(HEXPANDS.iter().position(|&h| h == m.h_expand).unwrap_or(0)),
    put: |m, _, v| m.h_expand = HEXPANDS[v.index()],
}];

/// Written after `zoom`.
pub const ROLL: [Setting; 1] = [Setting {
    key: "auto_roll",
    scpi: Scpi::Cmd("ROLL"),
    ty: Ty::Bool,
    label: "Auto Roll",
    #[cfg(test)]
    doc: "Rolls the trace at slow time scales while this is on; the acquisition source can still keep it from \
          rolling.",
    accept: None,
    step: None,
    get: |m, _| Val::Bool(m.auto_roll),
    put: |m, _, v| m.auto_roll = v.index() != 0,
}];

const HEXPANDS: [HExpand; 4] = [HExpand::Center, HExpand::Left, HExpand::Right, HExpand::Trigger];

/// The variant's name in a setup; over SCPI the stock words, with `LEFT` and `RIGHt` for the edges too.
const HREF_IO: CustomIo = CustomIo {
    store: |v| format!("{:?}", HEXPANDS[v.index()]),
    load: |t| {
        HEXPANDS
            .iter()
            .position(|h| format!("{:?}", h) == t)
            .map(Val::Index)
            .ok_or_else(|| format!("unknown value: {}", t))
    },
    query: |v| ["CENT", "LB", "RB", "TRIG"][v.index()].to_string(),
    take: |a| {
        let a = a.trim();
        let kw = crate::words::kw;
        if kw(a, "CENTer") {
            Some(Val::Index(0))
        } else if a.eq_ignore_ascii_case("LB") || kw(a, "LEFT") {
            Some(Val::Index(1))
        } else if a.eq_ignore_ascii_case("RB") || kw(a, "RIGHt") {
            Some(Val::Index(2))
        } else if kw(a, "TRIGger") {
            Some(Val::Index(3))
        } else {
            None
        }
    },
    labels: &["Center", "Left", "Right", "Trigger"],
    #[cfg(test)]
    values: "`CENTer`, `LB` or `LEFT`, `RB` or `RIGHt`, `TRIGger`; the query answers `CENT`, `LB`, `RB` or `TRIG`",
};
