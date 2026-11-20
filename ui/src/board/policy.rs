// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the acquisition thread decides by, of the live compression (docs/fork/compress.md) and of the raw reads of
//! deep records for the measurements (docs/fork/deep-meas.md): the switches, the limits, the factor and the rate
//! limits. They belong to the board layer because the thread reads them (ADR 0049); the signal modules `compress`
//! and `deepmeas`, which draw and measure what the thread delivers, take them from here and give them on under their
//! own names (`compress::k_for`, `deepmeas::Want`).

pub mod compress;
pub mod deep;
