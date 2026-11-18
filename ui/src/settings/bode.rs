// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings of the Bode page: their rows in the Bode menu (ui/setting_items.rs) and the command of the mode,
//! `:BODE:MODE` (the `:BODE` branch of scpi/dispatch.rs; a value it does not take is refused with -224). A change in
//! the menu leaves the rest as it is (no `settings_changed`), as before the table; a new mode also clears the points
//! of the last sweep.
//!
//! The page keeps most of them in setup lines of several values (`bode = <start> <stop> <points> <amplitude> <in>
//! <out> <generator> <address>`, `bode_sweep = <time> <log> <sync> <edge> <level> <input> <ref>` for the free sweep),
//! so the keys of those rows name a field of a line (`bode start`). store.rs writes and reads the lines by hand, its
//! numbers with `store::p_f64`: a line sets none of its fields after the first that is wrong, the points are kept
//! within 2..=1000 and the sync channel 0 counts as 1 (quirks kept). `bode_link` and `bode_mode` are keys of their own,
//! written by their slices where the keys were, so the keys of a setup file keep their order.
//!
//! Still written out by hand: the input and the output (the menu offers the waves the screen shows, the free sweep's
//! input the channels or none), the generator (a list of presets in ui/bode.rs), its address and `:BODE:GENerator`
//! (one word, `usb` or the address, which sets the connection too), and the sync channel (the menu offers the
//! channels of the unit).

use super::{Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{decades, ladder_step, si_short, Slope};
use oos_dsp::bode::Mode;

/// The slices, for the docs table and the SCPI handler.
pub const SLICES: [&[Setting]; 4] = [&MODE, &SWEEP, &LINK, &FREE];

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

/// The amplitudes the menu steps along, 1-2-5 over the allowed 10 mVpp .. 20 Vpp.
pub const AMPS: [f64; 11] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0];

/// Why the rows but the mode have no command.
#[cfg(test)]
const NONE: &str = "none yet: the page is set in its menu and by a setup file";

/// `d` steps along the ladder `l` from `x`; a value above all starts from the top.
fn ladder(l: &[f64], x: f64, d: i32) -> f64 {
    ladder_step(l, x, d, l.len() - 1)
}

/// The frequencies of a sweep: 1-2-5 from 1 Hz to 5 GHz.
fn freq(x: f64, d: i32) -> f64 {
    ladder(&decades(0, 10, false), x, d)
}

/// A choice of two that the menu changes on an odd step only.
fn odd(on: bool, d: i32) -> Val {
    Val::Index((on ^ (d % 2 != 0)) as usize)
}

/// The fields of the `bode` line.
pub const SWEEP: [Setting; 4] = [
    Setting {
        key: "bode start",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "Hz") },
        label: "Start freq",
        #[cfg(test)]
        doc: "The first frequency of the sweep. The menu steps 1-2-5 and keeps it at or below half the stop.",
        accept: None,
        step: Some(|m, _, d| Val::F64(freq(m.bode_start, d).min(m.bode_stop / 2.0))),
        get: |m, _| Val::F64(m.bode_start),
        put: |m, _, v| m.bode_start = v.real(),
    },
    Setting {
        key: "bode stop",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "Hz") },
        label: "Stop freq",
        #[cfg(test)]
        doc: "The last frequency of the sweep. The menu steps 1-2-5 and keeps it at or above twice the start.",
        accept: None,
        step: Some(|m, _, d| Val::F64(freq(m.bode_stop, d).max(m.bode_start * 2.0))),
        get: |m, _| Val::F64(m.bode_stop),
        put: |m, _, v| m.bode_stop = v.real(),
    },
    Setting {
        key: "bode points",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::U32 { min: 5, max: 200, step: 5, unit: "" },
        label: "Points",
        #[cfg(test)]
        doc: "The number of frequencies of the sweep, logarithmically spaced. The menu steps by 5 from 5 to 200; a \
              setup line keeps 2 to 1000.",
        accept: None,
        step: None,
        get: |m, _| Val::U32(m.bode_points as u32),
        put: |m, _, v| m.bode_points = v.index(),
    },
    Setting {
        key: "bode amp",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "Vpp") },
        label: "Amplitude",
        #[cfg(test)]
        doc: "The amplitude the generator puts out, peak to peak. The menu steps 1-2-5 from 10 mVpp to 20 Vpp.",
        accept: None,
        step: Some(|m, _, d| Val::F64(ladder(&AMPS, m.bode_amp, d))),
        get: |m, _| Val::F64(m.bode_amp),
        put: |m, _, v| m.bode_amp = v.real(),
    },
];

/// Written after `bode`.
pub const LINK: [Setting; 1] = [Setting {
    key: "bode_link",
    scpi: Scpi::None {
        #[cfg(test)]
        why: "set with the address by `:BODE:GENerator`",
    },
    ty: Ty::Choice(
        &[Choice { store: "lan", scpi: "", label: "LAN" }, Choice { store: "usb", scpi: "", label: "USB" }],
        Unknown::Text("not lan or usb:"),
        Arg::Keyword,
    ),
    label: "Connection",
    #[cfg(test)]
    doc: "How an external generator is reached: on the network at its address, or on the USB-A port. The menu \
          changes it on an odd step.",
    accept: None,
    step: Some(|m, _, d| odd(m.bode_usb, d)),
    get: |m, _| Val::Index(m.bode_usb as usize),
    put: |m, _, v| m.bode_usb = v.index() != 0,
}];

/// The sync edges by their place in the menu.
const EDGES: [Slope; 2] = [Slope::Rise, Slope::Fall];

/// The fields of the `bode_sweep` line (the free sweep).
pub const FREE: [Setting; 5] = [
    Setting {
        key: "bode_sweep time",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "s") },
        label: "Sweep time",
        #[cfg(test)]
        doc: "How long the generator takes for one sweep. The menu steps 1-2-5 from 1 ms to 500 s.",
        accept: None,
        step: Some(|m, _, d| Val::F64(ladder(&decades(-3, 3, false), m.bode_sw_time, d))),
        get: |m, _| Val::F64(m.bode_sw_time),
        put: |m, _, v| m.bode_sw_time = v.real(),
    },
    Setting {
        key: "bode_sweep log",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        // the line keeps the switch as `true` or `false`
        ty: Ty::Choice(
            &[Choice { store: "false", scpi: "", label: "Lin" }, Choice { store: "true", scpi: "", label: "Log" }],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Sweep",
        #[cfg(test)]
        doc: "Whether the generator sweeps the frequency linearly or logarithmically over the sweep time. The menu \
              changes it on an odd step.",
        accept: None,
        step: Some(|m, _, d| odd(m.bode_sw_log, d)),
        get: |m, _| Val::Index(m.bode_sw_log as usize),
        put: |m, _, v| m.bode_sw_log = v.index() != 0,
    },
    Setting {
        key: "bode_sweep edge",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Choice(
            &[
                Choice { store: "Rise", scpi: "", label: "Rising" },
                Choice { store: "Fall", scpi: "", label: "Falling" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Sync edge",
        #[cfg(test)]
        doc: "The edge of the sync channel that marks the start of a sweep. The menu changes it on an odd step.",
        accept: None,
        step: Some(|m, _, d| odd(m.bode_sync_slope == Slope::Fall, d)),
        get: |m, _| Val::Index((m.bode_sync_slope == Slope::Fall) as usize),
        put: |m, _, v| m.bode_sync_slope = EDGES[v.index()],
    },
    Setting {
        key: "bode_sweep level",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| si_short(x, "V") },
        label: "Sync level",
        #[cfg(test)]
        doc: "The level of the sync edge. The menu steps by 0.1 V from -20 V to 20 V.",
        accept: None,
        step: Some(|m, _, d| {
            Val::F32((((m.bode_sync_level * 10.0).round() as i64 + d as i64).clamp(-200, 200) as f32) / 10.0)
        }),
        get: |m, _| Val::F32(m.bode_sync_level),
        put: |m, _, v| m.bode_sync_level = v.secs(),
    },
    Setting {
        key: "bode_sweep ref",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NONE,
        },
        ty: Ty::Real { range: None, shown: |x| format!("{:.1} dBV", x) },
        label: "Ref level",
        #[cfg(test)]
        doc: "The level the gain is measured against when the free sweep has no input channel, in dBV. The menu \
              steps by 1 dB from -80 to 40 dBV.",
        accept: None,
        step: Some(|m, _, d| Val::F64((m.bode_ref.round() + d as f64).clamp(-80.0, 40.0))),
        get: |m, _| Val::F64(m.bode_ref),
        put: |m, _, v| m.bode_ref = v.real(),
    },
];
