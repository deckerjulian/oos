// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-model: what the user has set (ADR 0064): the model with its ladders and number formats, the math channels,
//! the zones, the decoder tables, the logic analyser's record and the settings table (one row per setting, ADR 0043).
//! No files, no screen, no board. It names no module of oos-ui; the compiler keeps it that way. oos-ui brings each
//! module into its crate root (`main.rs`), so `crate::model` and the other paths of oos-ui stay as they were.

pub mod decode;
pub mod logic;
pub mod math;
pub mod model;
pub mod settings;
pub mod words;
pub mod zone;
