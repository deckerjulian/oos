// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Build script of oos-fpgad: the board family this build is for, `cfg(oos_board = "<name>")` from `OOS_BOARD`
//! (ADR 0067, hal/board_cfg.rs).

include!("../hal/board_cfg.rs");

fn main() {
    board_cfg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
}
