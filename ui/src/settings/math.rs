// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The math settings of the selected math channel that are one value each: the FFT window and the filters'
//! frequencies. Their commands are `:MATH<n>:...` (scpi/math.rs), which reads and sets channel n as if it were the
//! selected one; their rows the Math menu (ui/setting_items.rs). The slices are the places of `store::setup_text`
//! that write them, so the keys of a setup file keep their order.
//!
//! Still written out by hand: `math` (the menu offers the operators of the category the menu shows, `:OPERator?`
//! answers `OFF` that `:OPERator` does not take, `:DISPlay`, `:FFT:SOURce`, `:FFT:SCALe` and `:FILTer:TYPE` set it
//! too), `math_a` and `math_b` (the menu offers the sources the channel may use; `:SOURce1` and `:SOURce2` refuse an
//! empty reference with -230), `math_scale` (an index into the dB steps under FFT and into the volt steps otherwise;
//! `:SCALe` sets the nearest step of the operator's), `math_offset` (the menu shows divisions with two decimals and
//! steps by 0.1 without a limit, `:OFFSet` keeps it within 1e9), `math_lin` (one key for the menu rows A and B),
//! `math_sel` (the menu swaps the channels' settings, `Model::math_select`; a setup only sets the number),
//! `math_expr<N>`, `math_window`, and the other channels, `m<N>.*`: one block per channel under other keys (`op`,
//! `win`, ...), which an indexed row would carry for three of their nine fields.

use super::{Arg, Choice, Scpi, Setting, Ty, Unknown, Val};
use crate::math::WINDOWS;
use crate::model::{step_index, time_ladder};

/// Both slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 2] = [&WINDOW_BW, &FC];

/// Why the filters' frequencies have no command of their own.
#[cfg(test)]
const CORNERS: &str = "set through `:MATH<n>:FILTer:W1` and `:W2`, the corner frequencies (below)";

/// Written after `math_offset`.
pub const WINDOW_BW: [Setting; 2] = [
    Setting {
        key: "fft_win",
        scpi: Scpi::Cmd("FFT:WINDow"),
        ty: Ty::Choice(
            &[
                Choice { store: "Rect", scpi: "RECTangle", label: "Rect" },
                Choice { store: "Hann", scpi: "HANNing", label: "Hann" },
                Choice { store: "Hamming", scpi: "HAMMing", label: "Hamming" },
                Choice { store: "Blackman", scpi: "BLACkman", label: "Blackman" },
                Choice { store: "Flattop", scpi: "FLATtop", label: "Flattop" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Window",
        #[cfg(test)]
        doc: "The window the FFT multiplies the record with before the transform.",
        accept: None,
        get: |m| Val::Index(WINDOWS.iter().position(|&w| w == m.fft_win).unwrap_or(0)),
        put: |m, v| m.fft_win = WINDOWS[v.index()],
    },
    Setting {
        key: "math_bw",
        scpi: Scpi::None {
            #[cfg(test)]
            why: CORNERS,
        },
        ty: Ty::Hertz { ladder: freq_step },
        label: "Bandwidth",
        #[cfg(test)]
        doc: "The width of the band of the band-pass and band-stop filters. The menu steps 1-2-5 in periods.",
        accept: None,
        get: |m| Val::F32(m.math_bw),
        put: |m, v| m.math_bw = v.secs(),
    },
];

/// Written after `math_scale`.
pub const FC: [Setting; 1] = [Setting {
    key: "math_fc",
    scpi: Scpi::None {
        #[cfg(test)]
        why: CORNERS,
    },
    ty: Ty::Hertz { ladder: freq_step },
    label: "Frequency",
    #[cfg(test)]
    doc: "The cut-off frequency of the low- and high-pass filters, the centre of the band filters. The menu steps \
          1-2-5 in periods.",
    accept: None,
    get: |m| Val::F32(m.math_fc),
    put: |m, v| m.math_fc = v.secs(),
}];

/// One step of a frequency on the 1-2-5 ladder of its period, from the nearest period (the first one at the start
/// when the period is longer than all); a step up is a shorter period.
fn freq_step(f: f32, d: i32) -> f32 {
    let l = time_ladder();
    let per = 1.0 / f.max(1e-6);
    let i = l.iter().position(|&x| x >= per * 0.999).unwrap_or(0);
    1.0 / l[step_index(i, l.len(), -d)]
}
