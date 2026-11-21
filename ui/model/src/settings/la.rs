// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The switch of the logic analyser: its command `:LA:STATe` (scpi/la.rs, `ENABle` is another name of it) and its row
//! in the LA menu (ui/setting_items.rs). On a model without the LA port every `:LA` command is refused with -241 and
//! the menu does not switch it on; a value the command does not take is refused with -224.
//!
//! The LA keeps its settings in one setup line (`la = <on> <lines> <preset D0-7> <preset D8-15> <user D0-7>
//! <user D8-15> <height>`), so the key of the row names a field of that line (`la on`). store.rs writes and reads the
//! line by hand (`LaSet::to_text`, `LaSet::parse`): a line sets none of its fields when one is wrong. Also by hand: the
//! lines shown (`:LA:DIGital<n>:DISPlay`, the menu's groups and Line select), the thresholds (`:LA:POD<n>:THReshold`,
//! the menu's presets and a user value) and the lane height.

use super::{Scpi, Setting, Ty, Val};

/// The slice, for the docs table and the SCPI handler.
pub const SLICES: [&[Setting]; 1] = [&ON];

pub const ON: [Setting; 1] = [Setting {
    key: "la on",
    scpi: Scpi::Cmd("STATe"),
    ty: Ty::Bool,
    label: "LA",
    #[cfg(test)]
    doc: "The logic analyser, where the model has the LA port: its 16 lines are recorded with the channels. `ENABle` \
          is another name of `STATe`.",
    accept: None,
    step: None,
    get: |m, _| Val::Bool(m.la.on),
    put: |m, _, v| m.la.on = v.index() != 0,
}];
