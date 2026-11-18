// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The limits and tables of the screen that the model, the settings and the setup files count with too: they belong
//! to what the user can set, not to how it is drawn. The screen's own tables (`ui::tools::PAIRS`, `ui::tables::PANES`,
//! `ui::logger::LOG_STEPS`) are as long as the counts here; the compiler holds them to it.

use super::MEAS_SELECTION_MAX;

/// The most views.
pub const VIEWS_MAX: usize = 4;
/// Most hardware segments (the stock UI limit; the frame memory holds up to 491,520 of the smallest).
pub const SEG_HW_MAX: usize = 100_000;
/// The longest interval after a hardware segment that a setup or a command may set, in seconds.
pub const SEG_IVL_MAX: f32 = 10.0;
/// The counts of averaging.
pub const AVERAGES: [u32; 10] = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];
/// DVM modes and counter modes.
pub const DVM_MODES: [&str; 3] = ["DC", "AC rms", "AC+DC rms"];
pub const CNT_MODES: [&str; 3] = ["Frequency", "Period", "Totalize"];
/// Kinds of delay and phase measurement and of second panes, for the setups (`ui::tools::PAIRS`).
pub const PAIRS_N: usize = 17;
/// The second panes (`ui::tables::PANES`).
pub const PANES_N: usize = 6;
/// The intervals of the logger (`ui::logger::LOG_STEPS`).
pub const LOG_STEPS_N: usize = 10;
/// Harmonics of the measured frequency the span of the pane covers; 0 = the whole band.
pub const PANE_HARMS: [usize; 5] = [0, 5, 10, 20, 50];
/// Values of `Model::trend_sel`: by the list, off, one per measurement.
pub const TREND_SEL_N: usize = MEAS_SELECTION_MAX + 2;
