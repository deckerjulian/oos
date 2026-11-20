// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the tests of the settings table and of oos-ui ask of every row: the tables with their SCPI roots, the
//! instances of a row and a value other than the default (the feature `testkit`).

use super::*;

/// Each table: its slices, the SCPI root of its commands, its name in the markers of docs/scpi.md, its menu.
pub const GROUPS: [(&[&[Setting]], &str, &str, &str); 14] = [
    (&display::SLICES, ":DISPlay", "display", "Display"),
    (&trigger::SLICES, ":TRIGger", "trigger", "Trigger"),
    (&acquisition::SLICES, ":ACQuire", "acquisition", "Acquire"),
    (&math::SLICES, ":MATH<n>", "math", "Math"),
    (&meters::DVM_SLICES, ":DVM", "dvm", "DVM and counter"),
    (&meters::COUNTER_SLICES, ":COUNter", "counter", "DVM and counter"),
    (&channel::SLICES, ":CHANnel<n>", "channel", "Channel"),
    (&mask::SLICES, ":MASK", "mask", "Mask"),
    (&histogram::SLICES, ":HISTogram", "histogram", "Display"),
    (&bode::SLICES, ":BODE", "bode", "Bode"),
    (&la::SLICES, ":LA", "la", "Logic analyser"),
    (&spectrum::SLICES, ":SPECtrum", "spectrum", "Spectrum"),
    (&eye::SLICES, ":EYE", "eye", "Eye"),
    (&power::SLICES, ":POWer", "power", "Power"),
];

/// Every row with the SCPI root of its group, the first channel for `<n>`.
pub fn rows() -> impl Iterator<Item = (&'static Setting, String)> {
    GROUPS.into_iter().flat_map(|(slices, root, _, _)| {
        slices.iter().flat_map(|g| g.iter()).map(move |s| (s, root.replace("<n>", "1")))
    })
}

/// The instances of a row: the channels of a `ch<n>` row, one of the others.
pub fn instances(s: &Setting) -> std::ops::Range<usize> {
    0..if s.key.starts_with("ch<n>") { crate::model::NCH } else { 1 }
}

/// A value of instance n other than the default: one step of the menu, the other way at the end of a range. The
/// other instances keep theirs.
pub fn changed(s: &Setting, n: usize) -> Model {
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
