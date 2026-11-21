// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What every row of the settings table must have, and that a real number reads back as written. That a value survives
//! the setup file and SCPI is tested in oos-ui (`store::tests`, `scpi::tests::settings`), the menu side in
//! ui/setting_items.rs.

use super::testkit::{changed, rows, GROUPS};
use super::*;

const DOCS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/scpi.md");
const SCPI_TXT: &str = include_str!("../../../tests/scpi.txt");

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
        if let Ty::Real { .. } = s.ty {
            assert!(s.step.is_some(), "{}: a real number steps by its row's step", s.key);
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

/// A real number reads back what its row writes, with the precision of its field: an f64 field keeps a value an
/// f32 cannot hold and reads `inf` as the setup lines do; an f32 field refuses it.
#[test]
fn a_real_number_reads_back_as_written() {
    for s in all().filter(|s| matches!(s.ty, Ty::Real { .. })) {
        let m = changed(s, 0);
        let mut back = Model::default();
        s.load(&mut back, 0, &s.store(&m, 0)).unwrap_or_else(|e| panic!("{}: {}", s.key, e));
        assert_eq!((s.get)(&back, 0), (s.get)(&m, 0), "{}", s.key);
        let f64_field = matches!((s.get)(&m, 0), Val::F64(_));
        assert_eq!(s.load(&mut back, 0, "inf").is_ok(), f64_field, "{}", s.key);
        if f64_field && s.load(&mut back, 0, "1.0000000001").is_ok() {
            assert_eq!(s.store(&back, 0), "1.0000000001", "{}", s.key);
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
            "docs/scpi.md differs from the {} table: OOS_BLESS=1 cargo test --locked -q settings::tests in ui/model",
            name
        );
    }
    if bless {
        std::fs::write(DOCS, doc).unwrap();
    }
}
