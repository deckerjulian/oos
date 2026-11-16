// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The scale of the read-out words (afe-settings.md 4.1): a 16-bit word, `MID` at the channel's offset and
//! `CODES_PER_DIV` per vertical division, after the read path's digital gain (`wpu`). The record decode
//! (`seq::volts`), the trigger and mask comparators and the measure unit all count in it.

/// The word of the channel's offset (the screen centre).
pub const MID: f64 = 32768.0;
/// Words per vertical division.
pub const CODES_PER_DIV: f64 = 7500.0;
