// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

// The choice of the board family, for the build scripts of oos-hal and oos-ui (ADR 0067). They take it
// with `include!` (a build script can share code with no dependency only this way) and call `board_cfg`, from the
// folder of their crate.
//
// One switch for the whole build: `OOS_BOARD=<name>`, as boot/build-dev-image.sh and the image scripts read it, `dho`
// when it is not set. `<name>` must be a folder of rootfs/board/. The crate is compiled with `cfg(oos_board =
// "<name>")`, and every folder of rootfs/board/ is a value the compiler knows, so a misspelt `cfg` is a warning.

/// Picks the board family for the crate whose folder is `crate_dir`, and tells Cargo. `shared` is the path of this
/// file as the build script includes it: with `rerun-if` lines Cargo watches only what they name.
fn board_cfg(crate_dir: &std::path::Path, shared: &str) {
    let boards = crate_dir.join("../rootfs/board");
    let mut names: Vec<String> = std::fs::read_dir(&boards)
        .unwrap_or_else(|e| panic!("{}: {}", boards.display(), e))
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let name = std::env::var("OOS_BOARD").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "dho".to_string());
    if !names.contains(&name) {
        panic!("OOS_BOARD={}: no such board family, rootfs/board/ has {:?} (ADR 0067)", name, names);
    }
    let values: Vec<String> = names.iter().map(|n| format!("\"{}\"", n)).collect();
    println!("cargo::rustc-check-cfg=cfg(oos_board, values({}))", values.join(", "));
    println!("cargo::rustc-cfg=oos_board=\"{}\"", name);
    println!("cargo::rerun-if-env-changed=OOS_BOARD");
    println!("cargo::rerun-if-changed={}", boards.display());
    println!("cargo::rerun-if-changed={}", crate_dir.join(shared).display());
    println!("cargo::rerun-if-changed=build.rs");
}
