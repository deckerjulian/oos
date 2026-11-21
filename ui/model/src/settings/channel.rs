// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The channel settings: one row per setting for the four channels (`ch<n>.*`, `:CHANnel<n>:...`), and the two
//! switches of the Channel menu that hold for every channel. Their commands are `:CHANnel<n>:...` (the `:CHANnel`
//! branch of scpi/dispatch.rs), their rows the Channel menu and its More page (ui/setting_items.rs). The slices are
//! the places of `store::setup_text` that write them, so the keys of a setup file keep their order; `ch<n>.unit` is
//! written only when it is not V, which store.rs tests.
//!
//! Still written out by hand: `ch<n>.vdiv` and `ch<n>.vfine` (a setup keeps the step and the fine factor apart,
//! `:SCALe` sets and answers volts per division at the probe tip, the menu steps through the calibration rows with
//! Fine scale on, and around ground the offset follows), `ch<n>.offset` (two commands, `:OFFSet` and `:POSition`; the
//! menu steps by a 25th of the channel's scale in its unit), `ch<n>.coupling` (the menu offers the couplings of the
//! board, `:COUPling` takes `DC50` only where the board has it), `ch<n>.probe` (the menu offers 1x, 10x and 100x and
//! shows any factor), `ch<n>.bias` and `ch<n>.skew` (written only when not 0; the menu steps by the channel's scale or
//! by 600 ps and shows the channel's unit; `:TCALibrate` refuses a change on a slow time base), `ch<n>.label` (the
//! menu edits it on the keyboard, `:LABel:SHOW` sets a default label) and the old keys `ch<n>.vscale` and
//! `ch<n>.bw20`.

use super::{Arg, Choice, CustomIo, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{AmpUnit, BWS, BW_20M, BW_FULL};

/// The slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 6] = [&ON, &BW, &INVERT, &FINE, &UNIT, &EXPAND];

/// The rows whose command refuses a value it does not take with `-224,"Illegal parameter value"`; a value the other
/// rows do not take falls through to `-113`, as before the table.
pub const REFUSES: [&str; 3] = ["ch<n>.unit", "fine_v", "v_expand_gnd"];

/// Written first of a channel.
pub const ON: [Setting; 1] = [Setting {
    key: "ch<n>.on",
    scpi: Scpi::Cmd("DISPlay"),
    ty: Ty::Bool,
    label: "Display",
    #[cfg(test)]
    doc: "Shows the channel. The menu switches on only the inputs the model has.",
    accept: None,
    step: None,
    get: |m, n| Val::Bool(m.ch[n].on),
    put: |m, n, v| m.ch[n].on = v.index() != 0,
}];

/// Written after `ch<n>.vfine`, when not V.
pub const UNIT: [Setting; 1] = [Setting {
    key: "ch<n>.unit",
    scpi: Scpi::Cmd("UNITs"),
    ty: Ty::Choice(
        &[
            Choice { store: "V", scpi: "VOLTage", label: "V" },
            Choice { store: "A", scpi: "AMPere", label: "A" },
            Choice { store: "W", scpi: "WATT", label: "W" },
            Choice { store: "U", scpi: "UNKNown", label: "U" },
        ],
        Unknown::Text("no unit"),
        Arg::Keyword,
    ),
    label: "Unit",
    #[cfg(test)]
    doc: "Supported channel unit. Use the configured unit when interpreting cursor/readout values.",
    accept: None,
    step: None,
    get: |m, n| Val::Index(m.ch[n].unit as usize),
    put: |m, n, v| m.ch[n].unit = AmpUnit::ALL[v.index()],
}];

/// Written after `ch<n>.coupling`.
pub const BW: [Setting; 1] = [Setting {
    key: "ch<n>.bw",
    scpi: Scpi::Cmd("BWLimit"),
    ty: Ty::Custom(&BW_IO),
    label: "Bandwidth",
    #[cfg(test)]
    doc: "The channel's bandwidth limit. OOS filtering extends stock choices.",
    accept: None,
    step: None,
    get: |m, n| Val::Index(m.ch[n].bw),
    put: |m, n, v| m.ch[n].bw = v.index(),
}];

/// Written last of a channel.
pub const INVERT: [Setting; 1] = [Setting {
    key: "ch<n>.invert",
    scpi: Scpi::Cmd("INVert"),
    ty: Ty::Bool,
    label: "Invert",
    #[cfg(test)]
    doc: "Inverts the channel.",
    accept: None,
    step: None,
    get: |m, n| Val::Bool(m.ch[n].invert),
    put: |m, n, v| m.ch[n].invert = v.index() != 0,
}];

/// Written after `aux_out`.
pub const FINE: [Setting; 1] = [Setting {
    key: "fine_v",
    scpi: Scpi::Cmd("VERNier"),
    ty: Ty::Bool,
    label: "Fine scale",
    #[cfg(test)]
    doc: "Fine vertical control, one switch for every channel: the scale steps through the rows of the calibration \
          instead of 1-2-5. Off keeps a fine scale until the next coarse step.",
    accept: None,
    step: None,
    get: |m, _| Val::Bool(m.fine_v),
    put: |m, _, v| m.fine_v = v.index() != 0,
}];

/// Written after `fine_t` and `tfine`.
pub const EXPAND: [Setting; 1] = [Setting {
    key: "v_expand_gnd",
    scpi: Scpi::Cmd("EXPand"),
    ty: Ty::Custom(&EXPAND_IO),
    label: "Expand around",
    #[cfg(test)]
    doc: "Vertical expansion reference supported by the model, one switch for every channel: a change of scale keeps \
          the ground line or the screen centre in place.",
    accept: None,
    step: None,
    get: |m, _| Val::Bool(m.v_expand_gnd),
    put: |m, _, v| m.v_expand_gnd = v.index() != 0,
}];

/// The menu's labels of the bandwidth limits.
const BW_LABELS: [&str; BWS.len()] = {
    let mut l = [""; BWS.len()];
    let mut i = 0;
    while i < l.len() {
        l[i] = BWS[i].0;
        i += 1;
    }
    l
};

/// Its name in BWS in a setup; over SCPI `OFF` or `FULL`, `AUTO`, `GND`, `20M` or `20MHZ`, or a filter as the name
/// without spaces, with or without `HZ` (`100M`, `1GHZ`), in any case; the query answers `OFF`, `AUTO`, `20M` or the
/// name upper-cased without spaces.
const BW_IO: CustomIo = CustomIo {
    store: |v| BWS[v.index()].0.to_string(),
    load: |t| {
        BWS.iter().position(|b| b.0 == t.trim()).map(Val::Index).ok_or_else(|| format!("unknown bandwidth {}", t))
    },
    query: |v| match v.index() {
        BW_20M => "20M".to_string(),
        BW_FULL => "OFF".to_string(),
        0 => "AUTO".to_string(),
        i => BWS[i].0.replace(' ', "").to_ascii_uppercase(),
    },
    take: |a| {
        let a = a.trim().to_ascii_uppercase().replace(' ', "");
        match a.as_str() {
            "OFF" | "FULL" => Some(BW_FULL),
            "AUTO" => Some(0),
            "GND" => Some(3),
            "20M" | "20MHZ" => Some(BW_20M),
            _ => BWS.iter().position(|b| {
                b.0.replace(' ', "").to_ascii_uppercase() == a
                    || b.0.replace(' ', "").to_ascii_uppercase() == format!("{}HZ", a)
            }),
        }
        .map(Val::Index)
    },
    labels: &BW_LABELS,
    #[cfg(test)]
    values: "`OFF|20M|AUTO` or a supported filter name",
};

/// `true`/`false` in a setup (`on`/`off`/`1`/`0` read too); over SCPI `GND` or `CENTer` in the long or the short form,
/// the query answers `GND` or `CENT`.
const EXPAND_IO: CustomIo = CustomIo {
    store: |v| (v.index() != 0).to_string(),
    load: |t| crate::words::p_bool(t).map(Val::Bool),
    query: |v| if v.index() != 0 { "GND" } else { "CENT" }.to_string(),
    take: |a| {
        if crate::words::kw(a, "GND") {
            Some(Val::Bool(true))
        } else if crate::words::kw(a, "CENTer") {
            Some(Val::Bool(false))
        } else {
            None
        }
    },
    labels: &["Center", "Ground"],
    #[cfg(test)]
    values: "`GND`, `CENTer`; the query answers `GND` or `CENT`",
};
