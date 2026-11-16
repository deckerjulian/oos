// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-update install --card`: only a regular file (also behind a symlink) is a card file for tests; anything else
//! that is not the system card is refused before a card is opened.

use std::process::Command;

fn install(card: &std::path::Path, dir: &std::path::Path) -> (bool, String) {
    let pkg = dir.join("x.oosupd");
    std::fs::write(&pkg, b"not a package").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_oos-update"))
        .args(["install", pkg.to_str().unwrap(), "--card", card.to_str().unwrap(), "--state", dir.join("state").to_str().unwrap()])
        .output()
        .unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stderr).into_owned())
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("oos-card-option-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_device_or_directory_that_is_not_the_system_card_is_refused() {
    let d = tmp("refused");
    for card in [std::path::PathBuf::from("/dev/null"), d.clone(), d.join("missing")] {
        let (ok, err) = install(&card, &d);
        assert!(!ok);
        assert!(err.contains("--card"), "{}: {}", card.display(), err);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_regular_file_behind_a_symlink_is_a_card_file() {
    let d = tmp("file");
    let file = d.join("card.img");
    std::fs::write(&file, vec![0u8; 4096]).unwrap();
    let link = d.join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    for card in [&file, &link] {
        let (ok, err) = install(card, &d);
        // the card file is opened and the package then refused; the card option itself is not
        assert!(!ok);
        assert!(!err.contains("--card"), "{}: {}", card.display(), err);
    }
    let _ = std::fs::remove_dir_all(&d);
}
