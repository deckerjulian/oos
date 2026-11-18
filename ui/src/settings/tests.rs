// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What every row of the settings table must have, and that a value survives each way the table carries it. The menu
//! side is tested in ui/setting_items.rs.

use super::*;
use crate::ui::testkit::{q, scpi_session};

const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/scpi.md");
const SCPI_TXT: &str = include_str!("../../tests/scpi.txt");

/// Each table: its slices, the SCPI root of its commands, its name in the markers of docs/scpi.md, its menu.
const GROUPS: [(&[&[Setting]], &str, &str, &str); 11] = [
    (&display::SLICES, ":DISPlay", "display", "Display"),
    (&trigger::SLICES, ":TRIGger", "trigger", "Trigger"),
    (&acquisition::SLICES, ":ACQuire", "acquisition", "Acquire"),
    (&math::SLICES, ":MATH<n>", "math", "Math"),
    (&meters::DVM_SLICES, ":DVM", "dvm", "DVM and counter"),
    (&meters::COUNTER_SLICES, ":COUNter", "counter", "DVM and counter"),
    (&channel::SLICES, ":CHANnel<n>", "channel", "Channel"),
    (&system::SLICES, ":SYSTem", "system", "Acquire"),
    (&mask::SLICES, ":MASK", "mask", "Mask"),
    (&histogram::SLICES, ":HISTogram", "histogram", "Display"),
    (&bode::SLICES, ":BODE", "bode", "Bode"),
];

/// Every row with the SCPI root of its group, the first channel for `<n>`.
fn rows() -> impl Iterator<Item = (&'static Setting, String)> {
    GROUPS.into_iter().flat_map(|(slices, root, _, _)| {
        slices.iter().flat_map(|g| g.iter()).map(move |s| (s, root.replace("<n>", "1")))
    })
}

/// The instances of a row: the channels of a `ch<n>` row, one of the others.
fn instances(s: &Setting) -> std::ops::Range<usize> {
    0..if s.key.starts_with("ch<n>") { crate::model::NCH } else { 1 }
}

/// A value of instance n other than the default: one step of the menu, the other way at the end of a range. The
/// other instances keep theirs.
fn changed(s: &Setting, n: usize) -> Model {
    let d = Model::default();
    let mut m = Model::default();
    s.step(&mut m, n, 1);
    if (s.get)(&m, n) == (s.get)(&d, n) {
        s.step(&mut m, n, -2);
    }
    assert_ne!((s.get)(&m, n), (s.get)(&d, n), "{}: a step changes nothing", s.key);
    for k in instances(s).filter(|&k| k != n) {
        assert_eq!((s.get)(&m, k), (s.get)(&d, k), "{}: a step of {} changes {}", s.key, n, k);
    }
    m
}

#[test]
fn every_row_is_complete() {
    assert_eq!(rows().count(), all().count(), "a table that GROUPS does not name");
    let mut keys = std::collections::HashSet::new();
    let mut cmds = std::collections::HashSet::new();
    for (s, root) in rows() {
        assert!(!s.key.is_empty() && keys.insert(s.key), "a key empty or twice: {:?}", s.key);
        assert!(!s.label.is_empty() && !s.doc.is_empty(), "{}: a menu label and a docs line", s.key);
        match s.scpi {
            Scpi::Cmd(c) => {
                let cmd = format!("{}:{}", root, c);
                let asked = SCPI_TXT
                    .lines()
                    .filter(|l| l.starts_with("scpi "))
                    .any(|l| l.split_whitespace().nth(2).is_some_and(|h| h.eq_ignore_ascii_case(&format!("{}?", cmd))));
                assert!(asked, "{}: {}? is not in ui/tests/scpi.txt", s.key, cmd);
                assert!(cmds.insert(cmd), "{}: the command {} twice", s.key, c);
            }
            Scpi::None { why } => assert!(!why.is_empty(), "{}: no command, and no reason", s.key),
        }
        let n = s.options().len();
        if let Ty::Steps(list) = s.ty {
            assert!(list.len() >= 2 && list.is_sorted(), "{}: steps in order", s.key);
            assert!(list.contains(&((s.get)(&Model::default(), 0).index() as u32)), "{}: the default a step", s.key);
        } else if s.is_number() {
            assert_eq!(n, 0, "{}: a number has no options", s.key);
        } else {
            assert!(n >= 2, "{}: a choice of {}", s.key, n);
        }
    }
}

#[test]
fn a_changed_value_survives_the_setup_file() {
    for s in all() {
        for n in instances(s) {
            let m = changed(s, n);
            let back = crate::store::parse_setup(&crate::store::setup_text(&m)).expect("the setup loads");
            assert_eq!((s.get)(&back, n), (s.get)(&m, n), "{} {}", s.key, n);
            // the line of the fields of a setup line is written by hand
            if s.line().is_some() {
                continue;
            }
            let line = format!("{} = {}", s.key_at(n), s.store(&m, n));
            assert!(crate::store::setup_text(&m).lines().any(|l| l == line), "{}: no line {:?}", s.key, line);
        }
    }
}

#[test]
fn a_changed_value_set_over_scpi_reads_back() {
    for (slices, root, _, _) in GROUPS {
        for s in slices.iter().flat_map(|g| g.iter()) {
            let Scpi::Cmd(c) = s.scpi else { continue };
            for n in instances(s) {
                let m = changed(s, n);
                let want = s.query(&m, n);
                let root = root.replace("<n>", &(n + 1).to_string());
                let (mut sc, mut ui, cv) = scpi_session();
                assert_eq!(q(&mut sc, &mut ui, &cv, &format!("{}:{} {}", root, c, want)), "");
                // the query in the short form of the root (`:DISP`)
                let short: String = root.chars().filter(|c| !c.is_ascii_lowercase()).collect();
                assert_eq!(q(&mut sc, &mut ui, &cv, &format!("{}:{}?", short, c)), want, "{} {}", s.key, n);
                for k in instances(s) {
                    assert_eq!((s.get)(&ui.m, k), (s.get)(&m, k), "{} {} {}", s.key, n, k);
                }
                assert!(q(&mut sc, &mut ui, &cv, ":SYSTem:ERRor?").starts_with("0,"), "{}", s.key);
            }
        }
    }
}

/// The table of docs/scpi.md between the markers of each group is the one its rows make.
/// `OOS_BLESS=1 cargo test --locked -q settings::tests` writes them.
#[test]
fn the_docs_tables_are_the_rows() {
    let mut doc = std::fs::read_to_string(DOCS).expect("docs/scpi.md");
    let bless = std::env::var("OOS_BLESS").is_ok_and(|v| v == "1");
    for (slices, root, name, menu) in GROUPS {
        let (begin, end) = (format!("<!-- settings:{}:begin -->\n", name), format!("<!-- settings:{}:end -->", name));
        let a = doc.find(&begin).unwrap_or_else(|| panic!("docs/scpi.md has no {}", begin.trim())) + begin.len();
        let b = a + doc[a..].find(&end).unwrap_or_else(|| panic!("docs/scpi.md has no {}", end));
        let want = docs_table(slices, root, menu);
        if bless {
            doc = format!("{}{}{}", &doc[..a], want, &doc[b..]);
            continue;
        }
        assert_eq!(
            &doc[a..b],
            want,
            "docs/scpi.md differs from the {} table: OOS_BLESS=1 cargo test --locked -q settings::tests",
            name
        );
    }
    if bless {
        std::fs::write(DOCS, doc).unwrap();
    }
}
