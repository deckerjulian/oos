// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What every module of the board's hardware layer (`hal/src/dho/mod.rs`) has: in its header the page of `docs/` it
//! is written from (ADR 0030: a hardware fact is in `docs/` before the code uses it), and a test. The step lists
//! themselves are compared by `tools/compare-base.sh` (`tools/plan-matrix.sh`). A new module fails here until it
//! names its page and has a test; what cannot have one stands in a list below with its reason.

use std::path::{Path, PathBuf};

/// The modules whose header names no page of `docs/`, each with the reason. One in this list that names a page fails
/// the test, so the list only gets shorter.
const NO_PAGE: [(&str, &str); 7] = [
    ("avg", "hardware averaging: the header says what is assumed, no page of docs/ describes it yet"),
    ("bus", "the access to the register window, no block of the FPGA"),
    ("gpio", "the GPIO character device of Linux, no part of the scope"),
    ("seg", "segmented memory: the header holds what was seen on the unit, no page of docs/ describes it yet"),
    ("seq", "it puts the blocks' steps in order and names the folder docs/protocols; each block names its page"),
    ("target", "the models and the card layout for the neutral code; the header does not name docs/models.md"),
    ("zonegeom", "geometry only, no register; `zone` names the page of the zone tables"),
];

/// The pages a header names that are not in the tree: module, name, reason.
const PAGE_NOT_THERE: [(&str, &str, &str); 1] =
    [("coretrim", "rt8847.md", "notes on the ADC that are not in the tree; docs/fork/core-offset.md names them too")];

/// The modules without a test, each with the reason. One in this list that has a test fails the test.
const NO_TEST: [(&str, &str); 2] = [
    ("readout", "two constants, no code"),
    ("target", "tables of the models and the card: the tests of `oos_hal::model` and `oos_hal::update` read them"),
];

/// The folders a header's page may be in, from the top of the tree: as written, or by its file name alone.
const FOLDERS: [&str; 5] = ["", "docs/protocols", "docs/fork", "docs", "docs/hardware"];

fn dho() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dho")
}

/// The Rust files of module `name`: `name.rs`, and every file below `name/`.
fn files(name: &str) -> Vec<PathBuf> {
    fn below(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for path in entries.map(|e| e.unwrap().path()) {
            if path.is_dir() {
                below(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = vec![dho().join(format!("{}.rs", name))];
    out.retain(|p| p.exists());
    below(&dho().join(name), &mut out);
    out
}

/// The pages (`*.md`) the `//!` lines of `text` name.
fn pages(text: &str) -> Vec<String> {
    let header = text.lines().filter(|l| l.starts_with("//!")).collect::<Vec<_>>().join(" ");
    let words = header.split(|c: char| !(c.is_ascii_alphanumeric() || "_-./".contains(c)));
    words.map(|w| w.trim_end_matches('.')).filter(|w| w.ends_with(".md")).map(String::from).collect()
}

#[test]
fn every_module_names_its_page_and_has_a_test() {
    let top = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let list = std::fs::read_to_string(dho().join("mod.rs")).unwrap();
    let modules: Vec<&str> = list.lines().filter_map(|l| l.strip_prefix("pub mod ")?.strip_suffix(';')).collect();
    assert!(modules.len() > 30, "hal/src/dho/mod.rs lists the modules as `pub mod <name>;`");
    let names = |l: &[(&'static str, &'static str)]| l.iter().map(|x| x.0).collect::<Vec<_>>();
    known_once("NO_PAGE", &names(&NO_PAGE), &modules);
    known_once("NO_TEST", &names(&NO_TEST), &modules);
    let pairs: Vec<String> = PAGE_NOT_THERE.iter().map(|x| format!("{} {}", x.0, x.1)).collect();
    for (i, x) in PAGE_NOT_THERE.iter().enumerate() {
        assert!(modules.contains(&x.0), "PAGE_NOT_THERE: {} is no module of hal/src/dho/mod.rs, take it out", x.0);
        assert!(!pairs[..i].contains(&pairs[i]), "PAGE_NOT_THERE: {} stands twice", pairs[i]);
    }
    let mut not_there_seen = Vec::new();
    for &m in &modules {
        let texts: Vec<String> = files(m).iter().map(|f| std::fs::read_to_string(f).unwrap()).collect();
        assert!(!texts.is_empty(), "{}: no file hal/src/dho/{}.rs and none below hal/src/dho/{}/", m, m, m);
        let named: Vec<String> = texts.iter().flat_map(|t| pages(t)).collect();
        for page in &named {
            let there = FOLDERS.iter().any(|f| top.join(f).join(page).is_file());
            let listed = PAGE_NOT_THERE.iter().any(|x| x.0 == m && x.1 == page);
            if listed {
                not_there_seen.push(format!("{} {}", m, page));
            }
            assert!(
                there != listed,
                "{}: its header names {}, which is {}",
                m,
                page,
                if there { "there: take it out of PAGE_NOT_THERE" } else { "not in docs/" }
            );
        }
        let has_page = named.iter().any(|page| FOLDERS.iter().any(|f| top.join(f).join(page).is_file()));
        match NO_PAGE.iter().find(|x| x.0 == m) {
            Some(x) => assert!(!has_page && !x.1.is_empty(), "{}: it names its page, take it out of NO_PAGE", m),
            None => assert!(has_page, "{}: its header names no page of docs/, and no reason in NO_PAGE", m),
        }
        let has_test = texts.iter().any(|t| t.contains("#[test]"));
        match NO_TEST.iter().find(|x| x.0 == m) {
            Some(x) => assert!(!has_test && !x.1.is_empty(), "{}: it has a test, take it out of NO_TEST", m),
            None => assert!(has_test, "{}: no test in hal/src/dho/{}*, and no reason in NO_TEST", m, m),
        }
    }
    for p in &pairs {
        assert!(not_there_seen.contains(p), "PAGE_NOT_THERE: {}: the header names that page no more, take it out", p);
    }
}

/// Every name of an exemption list is a module of `hal/src/dho/mod.rs`, and stands in the list once: an entry left
/// behind by a removed or renamed module fails, so the list cannot hold debt that is not there.
fn known_once(list: &str, keys: &[&str], modules: &[&str]) {
    for (i, k) in keys.iter().enumerate() {
        assert!(modules.contains(k), "{}: {} is no module of hal/src/dho/mod.rs, take it out", list, k);
        assert!(!keys[..i].contains(k), "{}: {} stands twice", list, k);
    }
}
