// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What every row of the settings table must have, and that a value survives each way the table carries it. The menu
//! side is tested in ui/setting_items.rs.

use super::*;
use crate::ui::testkit::{q, scpi_session};

const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/scpi.md");
const SCPI_TXT: &str = include_str!("../../tests/scpi.txt");

/// Each table: its slices, the SCPI root of its commands, its name in the markers of docs/scpi.md, its menu.
const GROUPS: [(&[&[Setting]], &str, &str, &str); 2] =
    [(&display::SLICES, ":DISPlay", "display", "Display"), (&trigger::SLICES, ":TRIGger", "trigger", "Trigger")];

/// Rows with a command whose query ui/tests/scpi.txt does not ask yet, and why. Adding the query changes the answers
/// the comparison with the base records, so it comes in a pull request of its own.
const NOT_ASKED: &[(&str, &str)] = &[
    (":DISPlay:GBRightness", "not asked before the table; settings_snapshot probes it"),
    (":DISPlay:COLor", "not asked before the table; settings_snapshot probes it"),
    (":TRIGger:COUPling", "not asked before the table"),
];

/// Every row with the SCPI root of its group.
fn rows() -> impl Iterator<Item = (&'static Setting, &'static str)> {
    GROUPS.into_iter().flat_map(|(slices, root, _, _)| slices.iter().flat_map(|g| g.iter()).map(move |s| (s, root)))
}

/// A value other than the default: one step of the menu, the other way at the end of a range.
fn changed(s: &Setting) -> Model {
    let mut m = Model::default();
    s.step(&mut m, 1);
    if (s.get)(&m) == (s.get)(&Model::default()) {
        s.step(&mut m, -2);
    }
    assert_ne!((s.get)(&m), (s.get)(&Model::default()), "{}: a step changes nothing", s.key);
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
                let excused = NOT_ASKED.iter().any(|&(n, _)| n == cmd);
                assert!(
                    asked != excused,
                    "{}: {}? is {} ui/tests/scpi.txt",
                    s.key,
                    cmd,
                    ["not in", "in"][asked as usize]
                );
                assert!(cmds.insert(cmd), "{}: the command {} twice", s.key, c);
            }
            Scpi::None { why } => assert!(!why.is_empty(), "{}: no command, and no reason", s.key),
        }
        let n = s.options().len();
        if s.is_number() {
            assert_eq!(n, 0, "{}: a number has no options", s.key);
        } else {
            assert!(n >= 2, "{}: a choice of {}", s.key, n);
        }
    }
}

#[test]
fn a_changed_value_survives_the_setup_file() {
    for s in all() {
        let m = changed(s);
        let back = crate::store::parse_setup(&crate::store::setup_text(&m)).expect("the setup loads");
        assert_eq!((s.get)(&back), (s.get)(&m), "{}", s.key);
        let line = format!("{} = {}", s.key, s.store(&m));
        assert!(crate::store::setup_text(&m).lines().any(|l| l == line), "{}: no line {:?}", s.key, line);
    }
}

#[test]
fn a_changed_value_set_over_scpi_reads_back() {
    for (s, root) in rows() {
        let Scpi::Cmd(c) = s.scpi else { continue };
        let m = changed(s);
        let want = s.query(&m);
        let (mut sc, mut ui, cv) = scpi_session();
        assert_eq!(q(&mut sc, &mut ui, &cv, &format!("{}:{} {}", root, c, want)), "");
        // the query in the short form of the root (`:DISP`)
        let short: String = root.chars().filter(|c| !c.is_ascii_lowercase()).collect();
        assert_eq!(q(&mut sc, &mut ui, &cv, &format!("{}:{}?", short, c)), want, "{}", s.key);
        assert_eq!((s.get)(&ui.m), (s.get)(&m), "{}", s.key);
        assert!(q(&mut sc, &mut ui, &cv, ":SYSTem:ERRor?").starts_with("0,"), "{}", s.key);
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
