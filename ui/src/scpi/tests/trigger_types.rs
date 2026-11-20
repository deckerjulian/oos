// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What a trigger type of the model (`TrigType`, ui/model/src/model/trigger.rs) needs besides its variant: its
//! words in setups and over SCPI, the texts that name it and its pictures.

use crate::model::*;
use crate::ui::testkit::{document, exempt_once, names, q, scpi_session, section};

/// Every trigger type with its words: the one a setup stores (`trig_type = <word>`, the variant's name), the one
/// `:TRIGger:MODE` takes and the one its query answers. A word that was released never changes: setups and
/// scripts hold it. A new type gets its row here; `row` does not compile without it.
const TYPES: [(TrigType, &str, &str, &str); 13] = [
    (TrigType::Edge, "Edge", "EDGE", "EDGE"),
    (TrigType::Pulse, "Pulse", "PULSE", "PULS"),
    (TrigType::Slope, "Slope", "SLOPE", "SLOP"),
    (TrigType::Video, "Video", "VIDEO", "VID"),
    (TrigType::Pattern, "Pattern", "PATTERN", "PATT"),
    (TrigType::Duration, "Duration", "DURATION", "DUR"),
    (TrigType::Timeout, "Timeout", "TIMEOUT", "TIM"),
    (TrigType::Runt, "Runt", "RUNT", "RUNT"),
    (TrigType::Window, "Window", "WINDOW", "WIND"),
    (TrigType::Delay, "Delay", "DELAY", "DEL"),
    (TrigType::SetupHold, "SetupHold", "SETUPHOLD", "SET"),
    (TrigType::NthEdge, "NthEdge", "NTHEDGE", "NEDG"),
    (TrigType::Bus, "Bus", "BUS", "BUS"),
];

/// The row of `t` in `TYPES`.
fn row(t: TrigType) -> usize {
    match t {
        TrigType::Edge => 0,
        TrigType::Pulse => 1,
        TrigType::Slope => 2,
        TrigType::Video => 3,
        TrigType::Pattern => 4,
        TrigType::Duration => 5,
        TrigType::Timeout => 6,
        TrigType::Runt => 7,
        TrigType::Window => 8,
        TrigType::Delay => 9,
        TrigType::SetupHold => 10,
        TrigType::NthEdge => 11,
        TrigType::Bus => 12,
    }
}

/// The types the pictures of the comparison (ui/tests/triggers.txt) do not show, each with the reason. A type in
/// this list that the script shows fails the test, so the list only gets shorter.
const NOT_SHOT: [(TrigType, &str); 1] =
    [(TrigType::Bus, "the script walks the types of the hardware trigger; the protocol trigger has no picture yet")];

/// What a trigger type needs besides its variant, each failure with its name and the part that is missing: its
/// place in `TRIG_TYPES` (the menu's list), a name, its stored word and its SCPI words as `TYPES` has them, the
/// lines of docs/scpi.md and of the wiki that name them, and its pictures in the comparison.
#[test]
fn every_trigger_type_is_complete() {
    let (scpi_md, wiki_scpi) = (document("docs/scpi.md"), document("wiki/SCPI-and-VXI-11.md"));
    let words = scpi_md.lines().find(|l| l.starts_with("Trigger type names are")).expect("the line of the words");
    let (long_words, short_words) = words.split_once("MODE replies").expect("the long and the short words");
    let wiki_row = wiki_scpi.lines().find(|l| l.starts_with("| `:TRIGger:MODE[?]`")).expect("the wiki's row");
    let features = section(&document("wiki/Features.md"), "**Trigger**");
    let shots = include_str!("../../../tests/triggers.txt");
    assert_eq!(TRIG_TYPES.len(), TYPES.len(), "TRIG_TYPES and TYPES list the same types");
    exempt_once("NOT_SHOT", &NOT_SHOT.map(|x| x.0), &TRIG_TYPES);
    let mut seen = std::collections::HashSet::new();
    for (k, (t, stored, long, short)) in TYPES.into_iter().enumerate() {
        assert_eq!(row(t), k, "{:?}: its row in TYPES", t);
        assert!(TRIG_TYPES.contains(&t), "{:?}: not in TRIG_TYPES, so in no menu", t);
        assert!(!t.name().is_empty() && seen.insert(t.name()), "{:?}: a name of its own", t);
        // the setup file
        assert_eq!(format!("{:?}", t), stored, "{:?}: the word a setup stores never changes", t);
        let m = Model { trig_type: t, ..Default::default() };
        let text = crate::store::setup_text(&m);
        assert!(text.lines().any(|l| l == format!("trig_type = {}", stored)), "{:?}: its setup line", t);
        assert_eq!(crate::store::parse_setup(&text).map(|m| m.trig_type), Ok(t), "{:?}: a setup reads it back", t);
        // the remote commands
        for word in [long, short] {
            let (mut sc, mut ui, cv) = scpi_session();
            assert_eq!(q(&mut sc, &mut ui, &cv, &format!(":TRIGger:MODE {}", word)), "", "{:?}", t);
            assert_eq!(ui.m.trig_type, t, ":TRIGger:MODE {} sets {:?}", word, t);
            assert_eq!(q(&mut sc, &mut ui, &cv, ":TRIGger:MODE?"), short, "{:?}: the answer of :TRIGger:MODE?", t);
            assert_eq!(q(&mut sc, &mut ui, &cv, ":TRIGger:TYPe?"), t.name(), "{:?}", t);
        }
        // the texts
        let tick = |w: &str| format!("`{}`", w);
        assert!(long_words.contains(&tick(long)), "{:?}: {} is not among the type names of docs/scpi.md", t, long);
        assert!(short_words.contains(&tick(short)), "{:?}: {} is not among the MODE replies of docs/scpi.md", t, short);
        assert!(wiki_row.contains(&tick(long)), "{:?}: {} is not in the wiki's row of :TRIGger:MODE", t, long);
        assert!(names(&features, t.name()), "{:?}: {} is not under Trigger in wiki/Features.md", t, t.name());
        // the pictures
        let shot = format!("-trig-{}.ppm", stored.to_lowercase());
        let shown = shots.lines().any(|l| l.starts_with("shot ") && l.ends_with(&shot));
        match NOT_SHOT.iter().find(|x| x.0 == t) {
            Some(x) => assert!(!shown && !x.1.is_empty(), "{:?}: it has its picture, take it out of NOT_SHOT", t),
            None => {
                assert!(shown, "{:?}: no picture *{} in ui/tests/triggers.txt, and no reason in NOT_SHOT", t, shot)
            }
        }
    }
}
