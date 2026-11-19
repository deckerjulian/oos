// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What every board family has (ADR 0067, CONTRIBUTING.md "A new board family"): a folder of `rootfs/board/` is a
//! family, and it needs its `Target` in `oos_hal::target::FAMILIES` (`hal/src/target.rs` compares its values with
//! `etc/oos-board`), its module in `ui/src/board/<name>/` with an `impl Board for`, its matrix of register sequences
//! `tools/board/<name>/plan-matrix.sh`, the keys of `etc/oos-board` the build and the scripts read, a `bin/oos-board`
//! that answers every command the common scripts ask, and its row in the table "Board families" of docs/porting.md,
//! whose family name is the `Target`'s. A row or a `Target` without a folder fails too. `oos-fpgad` is not checked
//! yet: it is the DHO800/DHO900 tool until it is split (ADR 0067).

use oos_hal::target::FAMILIES;
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The keys of `etc/oos-board` that `boot/`, the image scripts and the hal test of `Target` read.
const KEYS: [&str; 20] = [
    "OOS_BOARD",
    "OOS_IMAGE_NAME",
    "OOS_KERNEL_DTB",
    "OOS_BOOTARGS",
    "OOS_MODULES",
    "OOS_MODULE_NAMES",
    "OOS_GEL_STEM",
    "OOS_CARD_CONTROLLER",
    "OOS_PARM_MARKS",
    "OOS_PARM_LBA",
    "OOS_BCB_LBA",
    "OOS_BOOT_LBA",
    "OOS_BOOT_SECTORS",
    "OOS_RECOVERY_LBA",
    "OOS_BACKUP_LBA",
    "OOS_SLOT_LBA",
    "OOS_IMAGE_SECTORS",
    "OOS_OOSDATA_LBA",
    "OOS_OOSDATA_SECTORS",
    "OOS_RIGOL_LBA",
];

/// The states a family can have in docs/porting.md.
const STATES: [&str; 2] = ["in development", "ready"];

fn top() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {}", p.display(), e))
}

/// The folders of `rootfs/board/`.
fn folders() -> BTreeSet<String> {
    std::fs::read_dir(top().join("rootfs/board"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// The rows of the table "Board families" in docs/porting.md: folder, family name, state.
fn rows(text: &str) -> Vec<(String, String, String)> {
    let section = text.split("\n## Board families\n").nth(1).expect("docs/porting.md has no section Board families");
    let section = section.split("\n## ").next().unwrap();
    section
        .lines()
        .filter(|l| l.starts_with('|') && !l.starts_with("| Family") && !l.starts_with("|---"))
        .map(|l| {
            let c: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
            assert_eq!(c.len(), 4, "docs/porting.md, Board families: a row has four columns: {}", l);
            (c[1].trim_matches('`').to_string(), c[0].to_string(), c[3].to_string())
        })
        .collect()
}

/// Every command the common scripts of the image ask of `oos-board`.
fn commands() -> BTreeSet<String> {
    let dir = top().join("rootfs/initramfs");
    let mut files = vec![dir.join("etc/rcS")];
    files.extend(std::fs::read_dir(dir.join("bin")).unwrap().flatten().map(|e| e.path()));
    let mut out = BTreeSet::new();
    for p in files.iter().filter(|p| p.is_file()) {
        let Ok(t) = std::fs::read_to_string(p) else { continue };
        for line in t.lines() {
            for part in line.split("oos-board ").skip(1) {
                let w: String = part.chars().take_while(|c| c.is_ascii_lowercase() || *c == '-').collect();
                if !w.is_empty() {
                    out.insert(w);
                }
            }
        }
    }
    out
}

/// The Rust sources of a module: the files under its folder, or its one file.
fn rust_files(dir: &Path, file: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_to_string(file).into_iter().collect();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.extend(std::fs::read_to_string(&p));
            }
        }
    }
    out
}

/// Whether the shell script answers `cmd` with a `case` branch of its own.
fn answers(script: &str, cmd: &str) -> bool {
    script.lines().map(str::trim_start).any(|l| {
        let label = l.split(')').next().unwrap_or("");
        l.contains(')') && label.split('|').any(|w| w.trim() == cmd)
    })
}

fn missing(name: &str, family: Option<&str>, cmds: &BTreeSet<String>) -> Vec<String> {
    let t = top();
    let mut out = Vec::new();
    // compiled in: a family in FAMILIES has a `Target` its module declares; the values of etc/oos-board are compared
    // with it by `oos_hal::target`'s own tests
    match FAMILIES.iter().find(|f| f.name == name) {
        Some(tg) => {
            if let Some(f) = family {
                if tg.family_name != f {
                    out.push(format!(
                        "family_name \"{}\" of its `Target`, \"{}\" in docs/porting.md",
                        tg.family_name, f
                    ));
                }
            }
        }
        None => out.push(format!("its `Target` (hal/src/{}/target.rs) in `oos_hal::target::FAMILIES`", name)),
    }
    if !read(&t.join("hal/src/lib.rs")).contains(&format!("pub mod {};", name)) {
        out.push(format!("`pub mod {};` in hal/src/lib.rs", name));
    }
    let board = t.join("ui/src/board");
    if !rust_files(&board.join(name), &board.join(format!("{}.rs", name))).iter().any(|s| s.contains("impl Board for "))
    {
        out.push(format!("ui/src/board/{}/ with its `impl Board for ...`", name));
    }
    if !read(&board.join("mod.rs")).contains(&format!("pub mod {};", name)) {
        out.push(format!("`pub mod {};` in ui/src/board/mod.rs", name));
    }
    let dir = t.join("rootfs/board").join(name);
    match std::fs::read_to_string(dir.join("etc/oos-board")) {
        Ok(s) => {
            for k in KEYS {
                if !s.lines().any(|l| l.trim_start().starts_with(&format!("{}=", k))) {
                    out.push(format!("rootfs/board/{}/etc/oos-board: {}", name, k));
                }
            }
            if !s.lines().any(|l| l.trim() == format!("OOS_BOARD={}", name)) {
                out.push(format!("rootfs/board/{}/etc/oos-board: OOS_BOARD={}", name, name));
            }
        }
        Err(_) => out.push(format!("rootfs/board/{}/etc/oos-board", name)),
    }
    let matrix = format!("tools/board/{}/plan-matrix.sh", name);
    if !t.join(&matrix).is_file() {
        out.push(format!("{}: its matrix of register sequences (tools/plan-matrix.sh)", matrix));
    }
    let script = dir.join("bin/oos-board");
    match std::fs::read_to_string(&script) {
        Ok(s) => {
            let exec = std::fs::metadata(&script).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
            if !exec {
                out.push(format!("rootfs/board/{}/bin/oos-board: executable", name));
            }
            for c in cmds.iter().filter(|c| !answers(&s, c)) {
                out.push(format!("rootfs/board/{}/bin/oos-board: the command `{}`", name, c));
            }
        }
        Err(_) => out.push(format!("rootfs/board/{}/bin/oos-board", name)),
    }
    out
}

#[test]
fn every_board_family_is_complete() {
    let rows = rows(&read(&top().join("docs/porting.md")));
    let folders = folders();
    let cmds = commands();
    assert!(cmds.contains("bring-up"), "the common scripts ask oos-board nothing: {:?}", cmds);
    let mut bad = Vec::new();
    for (folder, family, state) in &rows {
        if !folders.contains(folder) {
            bad.push(format!("docs/porting.md: the family {} has no folder rootfs/board/{}", family, folder));
        }
        if !STATES.contains(&state.as_str()) {
            bad.push(format!("docs/porting.md: the state of {} is \"{}\", not one of {:?}", family, state, STATES));
        }
    }
    for f in FAMILIES.iter().filter(|f| !folders.contains(f.name)) {
        bad.push(format!("oos_hal::target::FAMILIES: {} has no folder rootfs/board/{}", f.family_name, f.name));
    }
    for name in &folders {
        let family = rows.iter().find(|r| &r.0 == name).map(|r| r.1.as_str());
        if family.is_none() {
            bad.push(format!("{}: its row in docs/porting.md, Board families", name));
        }
        bad.extend(missing(name, family, &cmds).into_iter().map(|m| format!("{}: {}", name, m)));
    }
    assert!(
        bad.is_empty(),
        "board families with a part missing (CONTRIBUTING.md, A new board family):\n{}",
        bad.join("\n")
    );
}

#[test]
fn a_case_branch_is_an_answer() {
    let s = "case \"$1\" in\nhas-hardware) hw;;\nearly|late) x;;\n*) echo usage;;\nesac\n";
    assert!(answers(s, "has-hardware"));
    assert!(answers(s, "late"));
    assert!(!answers(s, "bring-up"));
    assert!(!answers(s, "usage"));
}

/// The family that each `pub const NAME` of hal/src/target.rs takes its value from (`crate::<family>::...`), in order.
fn chosen(src: &str, name: &str) -> Vec<String> {
    let head = format!("pub const {}:", name);
    src.lines()
        .filter(|l| l.starts_with(&head))
        .filter_map(|l| Some(l.split("crate::").nth(1)?.split("::").next()?.to_string()))
        .collect()
}

#[test]
fn the_depths_come_from_the_active_family() {
    let src = read(&top().join("hal/src/target.rs"));
    let active = chosen(&src, "ACTIVE");
    assert!(!active.is_empty(), "hal/src/target.rs has no `pub const ACTIVE`");
    assert_eq!(chosen(&src, "DEPTHS"), active, "hal/src/target.rs: `DEPTHS` is not chosen with `ACTIVE`");
    for f in FAMILIES {
        let p = top().join("hal/src").join(f.name).join("target.rs");
        assert!(read(&p).contains("pub const DEPTHS:"), "{}: its `DEPTHS` (the Acquire menu)", p.display());
    }
}
