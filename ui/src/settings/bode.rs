// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The mode of the Bode page (ui/setting_items.rs) and its command `:BODE:MODE` (the `:BODE` branch of
//! scpi/dispatch.rs). A value the command does not take is refused with -224. A change in the menu clears the points of
//! the last sweep and leaves the rest as it is (no `settings_changed`), as before the table. The slice is the place of
//! `store::setup_text` that writes it, so the keys of a setup file keep their order.
//!
//! Still written out by hand: the sweep (`bode = <start> <stop> <points> <amplitude> <in> <out> <generator>
//! <address>`, a line of several values, with clamps between them and steps of their own), `:BODE:GENerator` (one
//! word, `usb` or the generator's LAN address) and the free sweep's settings.

use super::{Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use oos_dsp::bode::Mode;

/// The slice, for the docs table and the SCPI handler.
pub const SLICES: [&[Setting]; 1] = [&MODE];

/// The modes by their place in the list.
const MODES: [Mode; 3] = [Mode::Loop, Mode::Psrr, Mode::Z];

/// Written after `spec_acpr`.
pub const MODE: [Setting; 1] = [Setting {
    key: "bode_mode",
    scpi: Scpi::Cmd("MODE"),
    ty: Ty::Choice(
        &[
            Choice { store: "Loop", scpi: "LOOP", label: "Loop" },
            Choice { store: "Psrr", scpi: "PSRR", label: "PSRR" },
            Choice { store: "Z", scpi: "IMPedance", label: "Z" },
        ],
        Unknown::Value,
        Arg::Keyword,
    ),
    label: "Mode",
    #[cfg(test)]
    doc: "What a sweep stores in the middle field of each point: the gain of a control loop in dB, the power-supply \
          rejection (the negated gain) in dB, or for an impedance the linear ratio of output to input, not ohms. A \
          free sweep always measures the gain.",
    accept: None,
    step: None,
    get: |m, _| Val::Index(MODES.iter().position(|&x| x == m.bode_mode).unwrap_or(0)),
    put: |m, _, v| m.bode_mode = MODES[v.index()],
}];
