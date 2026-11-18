// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The switches of the mask test: its switch (`:MASK:ENABle`, scpi/mask.rs), and what a failure does and the
//! inversion of the mask, which no command sets. Their rows are in the Mask menu (ui/setting_items.rs); the menu's
//! switch builds a mask from the source when there is none, Invert builds the mask again. A value the command does not
//! take is refused with -224. The slices are the places of `store::setup_text` that write them, so the keys of a setup
//! file keep their order.
//!
//! Still written out by hand: `mask_src` (`:MASK:SOURce` takes the channels, the menu offers the ones the unit has, and
//! a new source drops the mask), and `mask_x` and `mask_y` (`:MASK:X` and `:MASK:Y` refuse a tolerance out of range,
//! the menu steps by 0.04 div within it, and either builds the mask again).

use super::{CustomIo, Scpi, Setting, Ty, Val};

/// The slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 2] = [&ON, &TEST];

/// Written after `hist`.
pub const ON: [Setting; 1] = [Setting {
    key: "mask_on",
    scpi: Scpi::Cmd("ENABle"),
    ty: Ty::Bool,
    label: "Mask test",
    #[cfg(test)]
    doc: "The pass/fail test of the source against the mask. A mask needs accepted channel data; enabling it does \
          not prove a valid mask or a physical pass/fail result.",
    accept: None,
    get: |m, _| Val::Bool(m.mask_on),
    put: |m, _, v| m.mask_on = v.index() != 0,
}];

/// Written after `mask_y`.
pub const TEST: [Setting; 2] = [
    Setting {
        key: "mask_stop",
        scpi: Scpi::None {
            #[cfg(test)]
            why: "a choice of the Mask menu that no remote command sets",
        },
        ty: Ty::Custom(&STOP_IO),
        label: "On fail",
        #[cfg(test)]
        doc: "What a failed waveform does: None goes on, Stop stops the acquisition.",
        accept: None,
        get: |m, _| Val::Bool(m.mask_stop),
        put: |m, _, v| m.mask_stop = v.index() != 0,
    },
    Setting {
        key: "mask_inv",
        scpi: Scpi::None {
            #[cfg(test)]
            why: "a choice of the Mask menu that no remote command sets",
        },
        ty: Ty::Bool,
        label: "Invert",
        #[cfg(test)]
        doc: "Turns the mask round: the band becomes the forbidden region, and a waveform passes where it stays \
              outside it.",
        accept: None,
        get: |m, _| Val::Bool(m.mask_inv),
        put: |m, _, v| m.mask_inv = v.index() != 0,
    },
];

/// `true`/`false` in a setup (`on`/`off`/`1`/`0` read too); None or Stop in the menu. No command asks or sets it.
const STOP_IO: CustomIo = CustomIo {
    store: |v| (v.index() != 0).to_string(),
    load: |t| crate::store::p_bool(t).map(Val::Bool),
    query: |v| (v.index() != 0).to_string(),
    take: |_| None,
    labels: &["None", "Stop"],
    #[cfg(test)]
    values: "none",
};
