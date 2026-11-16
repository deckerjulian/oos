// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Rigol MHO900 (MHO934, MHO954, MHO984), a port of the platform alone (docs/mho.md): the same RK3399 main board
//! family as the DHO800/DHO900 with another acquisition board (a Kintex-7 the SoC loads at every start, two ADCs
//! behind JESD204, other front ends). OOS has no code for that board: `target::MHO` says so (`acquisition: false`),
//! and nothing here touches it.

pub mod board;
pub mod target;
