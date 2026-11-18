// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The trigger values: how a trigger of the selected type fires, its times and the video trigger's choices. Their
//! commands are `:TRIGger:...` (the `:TRIGger` branch of scpi/dispatch.rs), their rows the Trigger menu
//! (ui/setting_items.rs). The slices are the places of `store::setup_text` that write them, so the keys of a setup
//! file keep their order.
//!
//! An SCPI value a row does not take falls through to the later branches of `exec_cmd`, which answer
//! `-113,"Undefined header"`, as before the table. Still written out by hand: `trig_when` (its menu offers Off only
//! for Runt), `video_line` (the menu keeps it below the line count of the standard) and `nth_n` (the menu keeps it
//! within 1 to 65535), where a row would change what they do, a setup reading any number into the last two; and the
//! sources, levels and patterns, which are not one value each.

use super::{Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{step_index, time_ladder, Pol, Slope, TrigMode, VSync, VideoStd, WinMode, TRIG_COUPS};

/// Every slice, in the order of the docs table.
pub const SLICES: [&[Setting]; 6] = [&EDGE, &REJECT, &SECOND, &TIMES, &WINDOW, &SETUP_HOLD];

/// Why the rows of a trigger type's own values have no command.
#[cfg(test)]
const NO_SUBTREE: &str = "OOS has no `:TRIGger` subtree of this trigger type yet";

/// Written after `trig_level`.
pub const EDGE: [Setting; 3] = [
    Setting {
        key: "slope",
        scpi: Scpi::Cmd("EDGE:SLOPe"),
        ty: Ty::Choice(&SLOPE_CHOICES, Unknown::Value, Arg::Prefix),
        label: "Slope",
        #[cfg(test)]
        doc: "The edge that triggers; also of the slope, timeout and Nth edge triggers, and A of the delay trigger.",
        accept: None,
        get: |m| Val::Index(SLOPES.iter().position(|&s| s == m.slope).unwrap_or(0)),
        put: |m, v| m.slope = SLOPES[v.index()],
    },
    Setting {
        key: "trig_mode",
        scpi: Scpi::Cmd("SWEep"),
        ty: Ty::Choice(
            &[
                Choice { store: "Auto", scpi: "AUTO", label: "Auto" },
                Choice { store: "Normal", scpi: "NORMal", label: "Normal" },
                Choice { store: "Single", scpi: "SINGle", label: "Single" },
            ],
            Unknown::Value,
            Arg::Prefix,
        ),
        label: "Sweep",
        #[cfg(test)]
        doc: "Auto draws without a trigger after a while, Normal only on a trigger, Single once; the menu's Single \
              also starts the acquisition.",
        accept: None,
        get: |m| Val::Index(TRIG_MODES.iter().position(|&t| t == m.trig_mode).unwrap_or(0)),
        put: |m, v| m.trig_mode = TRIG_MODES[v.index()],
    },
    Setting {
        key: "holdoff",
        scpi: Scpi::Cmd("HOLDoff"),
        ty: Ty::Seconds { ladder: holdoff_step, zero: Some("Off") },
        label: "Holdoff",
        #[cfg(test)]
        doc: "The time after a trigger in which no other one is taken; 0 is off. The menu steps 1-2-5 from 100 ns.",
        accept: Some((0.0, 10.0)),
        get: |m| Val::F32(m.holdoff),
        put: |m, v| m.holdoff = v.secs(),
    },
];

/// Written after `trig_level2`.
pub const REJECT: [Setting; 3] = [
    Setting {
        key: "trig_nrej",
        scpi: Scpi::Cmd("NREJect"),
        ty: Ty::Bool,
        label: "Noise reject",
        #[cfg(test)]
        doc: "A wider hysteresis of the trigger comparator (the values by scale: docs/protocols/trigger.md 2.3).",
        accept: None,
        get: |m| Val::Bool(m.trig_nrej),
        put: |m, v| m.trig_nrej = v.index() != 0,
    },
    Setting {
        key: "auto_nrej",
        scpi: Scpi::None {
            #[cfg(test)]
            why: "a choice of the auto set-up, not of the trigger",
        },
        ty: Ty::Bool,
        label: "Auto noise rej.",
        #[cfg(test)]
        doc: "The auto set-up sets noise reject from the noise on the flat levels of the trigger signal.",
        accept: None,
        get: |m| Val::Bool(m.auto_nrej),
        put: |m, v| m.auto_nrej = v.index() != 0,
    },
    Setting {
        key: "trig_coup",
        scpi: Scpi::Cmd("COUPling"),
        ty: Ty::Choice(
            &[
                Choice { store: "Dc", scpi: "DC", label: "DC" },
                Choice { store: "Ac", scpi: "AC", label: "AC" },
                Choice { store: "LfReject", scpi: "LFReject", label: "LF reject" },
                Choice { store: "HfReject", scpi: "HFReject", label: "HF reject" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Coupling",
        #[cfg(test)]
        doc: "A filter in the trigger path of the edge trigger; the trace keeps the channel's coupling \
              (docs/protocols/trigger.md 2.5).",
        accept: None,
        get: |m| Val::Index(TRIG_COUPS.iter().position(|&c| c == m.trig_coup).unwrap_or(0)),
        put: |m, v| m.trig_coup = TRIG_COUPS[v.index()],
    },
];

/// Written after `trig_ref2`.
pub const SECOND: [Setting; 2] = [
    Setting {
        key: "trig_slope2",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Choice(&SLOPE_CHOICES, Unknown::Value, Arg::Keyword),
        label: "Slope B",
        #[cfg(test)]
        doc: "The edge of B (delay trigger) or of the clock (setup/hold trigger).",
        accept: None,
        get: |m| Val::Index(SLOPES.iter().position(|&s| s == m.trig_slope2).unwrap_or(0)),
        put: |m, v| m.trig_slope2 = SLOPES[v.index()],
    },
    Setting {
        key: "trig_pol",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Choice(
            &[
                Choice { store: "Pos", scpi: "", label: "Positive" },
                Choice { store: "Neg", scpi: "", label: "Negative" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Polarity",
        #[cfg(test)]
        doc: "The polarity of the pulse (pulse and runt triggers).",
        accept: None,
        get: |m| Val::Index((m.trig_pol == Pol::Neg) as usize),
        put: |m, v| m.trig_pol = [Pol::Pos, Pol::Neg][v.index()],
    },
];

/// Written after `trig_when`.
pub const TIMES: [Setting; 4] = [
    Setting {
        key: "trig_t1",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Seconds { ladder: time_ladder_step, zero: None },
        label: "Time",
        #[cfg(test)]
        doc: "The time the condition compares with, the lower limit of Inside and Outside; the timeout of the timeout \
              trigger.",
        accept: None,
        get: |m| Val::F32(m.trig_t1),
        put: |m, v| m.trig_t1 = v.secs(),
    },
    Setting {
        key: "trig_t2",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Seconds { ladder: time_ladder_step, zero: None },
        label: "Upper limit",
        #[cfg(test)]
        doc: "The upper time limit of Inside and Outside.",
        accept: None,
        get: |m| Val::F32(m.trig_t2),
        put: |m, v| m.trig_t2 = v.secs(),
    },
    Setting {
        key: "video_std",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Choice(
            &[Choice { store: "Pal", scpi: "", label: "PAL" }, Choice { store: "Ntsc", scpi: "", label: "NTSC" }],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Standard",
        #[cfg(test)]
        doc: "The video standard of the video trigger.",
        accept: None,
        get: |m| Val::Index((m.video_std == VideoStd::Ntsc) as usize),
        put: |m, v| m.video_std = [VideoStd::Pal, VideoStd::Ntsc][v.index()],
    },
    Setting {
        key: "video_sync",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Choice(
            &[
                Choice { store: "AllLines", scpi: "", label: "Lines" },
                Choice { store: "Line", scpi: "", label: "Line n" },
                Choice { store: "Odd", scpi: "", label: "Odd" },
                Choice { store: "Even", scpi: "", label: "Even" },
                Choice { store: "AllFields", scpi: "", label: "Fields" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Sync",
        #[cfg(test)]
        doc: "What the video trigger fires on: every line, line n, odd or even fields, every field.",
        accept: None,
        get: |m| Val::Index(VSYNCS.iter().position(|&s| s == m.video_sync).unwrap_or(0)),
        put: |m, v| m.video_sync = VSYNCS[v.index()],
    },
];

/// Written after `pat_levels`.
pub const WINDOW: [Setting; 2] = [
    Setting {
        key: "win_mode",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Choice(
            &[
                Choice { store: "Enter", scpi: "", label: "Enter" },
                Choice { store: "Exit", scpi: "", label: "Exit" },
                Choice { store: "Inside", scpi: "", label: "Inside" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Trigger on",
        #[cfg(test)]
        doc: "When the window trigger fires: the signal enters the window, leaves it, or stays inside.",
        accept: None,
        get: |m| Val::Index(WIN_MODES.iter().position(|&w| w == m.win_mode).unwrap_or(0)),
        put: |m, v| m.win_mode = WIN_MODES[v.index()],
    },
    Setting {
        key: "nth_idle",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Seconds { ladder: time_ladder_step, zero: None },
        label: "Idle time",
        #[cfg(test)]
        doc: "The quiet time before the edges the Nth edge trigger counts.",
        accept: None,
        get: |m| Val::F32(m.nth_idle),
        put: |m, v| m.nth_idle = v.secs(),
    },
];

/// Written after `nth_n`.
pub const SETUP_HOLD: [Setting; 2] = [
    Setting {
        key: "setup",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Seconds { ladder: time_ladder_step, zero: None },
        label: "Setup",
        #[cfg(test)]
        doc: "The setup time the setup/hold trigger checks the data against the clock edge.",
        accept: None,
        get: |m| Val::F32(m.setup),
        put: |m, v| m.setup = v.secs(),
    },
    Setting {
        key: "hold",
        scpi: Scpi::None {
            #[cfg(test)]
            why: NO_SUBTREE,
        },
        ty: Ty::Seconds { ladder: time_ladder_step, zero: None },
        label: "Hold",
        #[cfg(test)]
        doc: "The hold time the setup/hold trigger checks the data against after the clock edge.",
        accept: None,
        get: |m| Val::F32(m.hold),
        put: |m, v| m.hold = v.secs(),
    },
];

/// The edges in the order of the choices; the Bode plot's sync edge takes the first two.
pub const SLOPES: [Slope; 3] = [Slope::Rise, Slope::Fall, Slope::Either];
const TRIG_MODES: [TrigMode; 3] = [TrigMode::Auto, TrigMode::Normal, TrigMode::Single];
const VSYNCS: [VSync; 5] = [VSync::AllLines, VSync::Line, VSync::Odd, VSync::Even, VSync::AllFields];
const WIN_MODES: [WinMode; 3] = [WinMode::Enter, WinMode::Exit, WinMode::Inside];

/// The edges; over SCPI (`:TRIGger:EDGE:SLOPe`) any word that starts with `POS`, `NEG` or `RFAL`.
const SLOPE_CHOICES: [Choice; 3] = [
    Choice { store: "Rise", scpi: "POSitive", label: "Rising" },
    Choice { store: "Fall", scpi: "NEGative", label: "Falling" },
    Choice { store: "Either", scpi: "RFALl", label: "Either" },
];

/// One step on the 1-2-5 time ladder from the nearest ladder value.
fn time_ladder_step(x: f32, d: i32) -> f32 {
    let l = time_ladder();
    let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
    l[step_index(i, l.len(), d)]
}

/// Next holdoff on the 1-2-5 ladder from 100 ns to 10 s; 0 is off.
fn holdoff_step(h: f32, d: i32) -> f32 {
    let mut steps = vec![0.0f32];
    let mut x = 1e-7f32;
    while x < 11.0 {
        steps.extend([x, 2.0 * x, 5.0 * x]);
        x *= 10.0;
    }
    steps.retain(|&s| s <= 10.0);
    let i = steps.iter().position(|&s| s >= h * 0.999).unwrap_or(steps.len() - 1) as i32;
    steps[(i + d).clamp(0, steps.len() as i32 - 1) as usize]
}
