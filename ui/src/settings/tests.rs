// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What every row of the settings table must have, and that a value survives each way the table carries it. The menu
//! side is tested in ui/setting_items.rs.

use super::*;
use crate::ui::testkit::{q, scpi_session};

const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/scpi.md");
const SCPI_TXT: &str = include_str!("../../tests/scpi.txt");

/// Rows with a command whose query ui/tests/scpi.txt does not ask yet, and why. Adding the query changes the answers
/// the comparison with the base records, so it comes in a pull request of its own.
const NOT_ASKED: &[(&str, &str)] = &[
    ("GBRightness", "not asked before the table; settings_snapshot probes it"),
    ("COLor", "not asked before the table; settings_snapshot probes it"),
];

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
    let mut keys = std::collections::HashSet::new();
    let mut cmds = std::collections::HashSet::new();
    for s in all() {
        assert!(!s.key.is_empty() && keys.insert(s.key), "a key empty or twice: {:?}", s.key);
        assert!(!s.label.is_empty() && !s.doc.is_empty(), "{}: a menu label and a docs line", s.key);
        match s.scpi {
            Scpi::Cmd(c) => {
                assert!(cmds.insert(c), "{}: the command {} twice", s.key, c);
                let asked = SCPI_TXT.lines().filter(|l| l.starts_with("scpi ")).any(|l| {
                    l.split_whitespace().nth(2).is_some_and(|h| h.eq_ignore_ascii_case(&format!(":DISPlay:{}?", c)))
                });
                let excused = NOT_ASKED.iter().any(|&(n, _)| n == c);
                assert!(
                    asked != excused,
                    "{}: :DISPlay:{}? is {} ui/tests/scpi.txt",
                    s.key,
                    c,
                    ["not in", "in"][asked as usize]
                );
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
    for s in all() {
        let Scpi::Cmd(c) = s.scpi else { continue };
        let m = changed(s);
        let want = s.query(&m);
        let (mut sc, mut ui, cv) = scpi_session();
        assert_eq!(q(&mut sc, &mut ui, &cv, &format!(":DISPlay:{} {}", c, want)), "");
        assert_eq!(q(&mut sc, &mut ui, &cv, &format!(":DISP:{}?", c)), want, "{}", s.key);
        assert_eq!((s.get)(&ui.m), (s.get)(&m), "{}", s.key);
        assert!(q(&mut sc, &mut ui, &cv, ":SYSTem:ERRor?").starts_with("0,"), "{}", s.key);
    }
}

/// The table of docs/scpi.md between the markers of the Display group is the one the rows make.
/// `OOS_BLESS=1 cargo test --locked -q settings::tests` writes it.
#[test]
fn the_docs_table_is_the_rows() {
    let (begin, end) = ("<!-- settings:display:begin -->\n", "<!-- settings:display:end -->");
    let doc = std::fs::read_to_string(DOCS).expect("docs/scpi.md");
    let a = doc.find(begin).expect("docs/scpi.md has no settings:display:begin marker") + begin.len();
    let b = a + doc[a..].find(end).expect("docs/scpi.md has no settings:display:end marker");
    let want = docs_table(&display::GROUPS, ":DISPlay", "Display");
    if std::env::var("OOS_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(DOCS, format!("{}{}{}", &doc[..a], want, &doc[b..])).unwrap();
        return;
    }
    assert_eq!(
        &doc[a..b],
        want,
        "docs/scpi.md differs from the table: OOS_BLESS=1 cargo test --locked -q settings::tests"
    );
}
