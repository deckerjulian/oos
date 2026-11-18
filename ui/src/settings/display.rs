// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Display group: how the waveform is drawn and how the graticule looks. Their commands are `:DISPlay:...`
//! (scpi/display.rs), their rows the Display menu (ui/setting_items.rs). The two slices are the two places of
//! `store::setup_text` that write them, so the keys of a setup file keep their order.

use super::{Arg, Choice, CustomIo, Scpi, Setting, Ty, Unknown, Val};
use crate::model::{Grid, PERSIST};

/// Both slices, in the order of the docs table.
pub const SLICES: [&[Setting]; 2] = [&VIEW, &DRAW];

/// Written after `mdepth`.
pub const VIEW: [Setting; 5] = [
    Setting {
        key: "persist",
        scpi: Scpi::Cmd("GRADing:TIME"),
        ty: Ty::Custom(&PERSIST_IO),
        label: "Persistence",
        #[cfg(test)]
        doc: "How long a drawn waveform stays on the screen.",
        accept: None,
        get: |m| Val::Index(m.persist),
        put: |m, v| m.persist = v.index(),
    },
    Setting {
        key: "intensity",
        scpi: Scpi::Cmd("WBRightness"),
        ty: Ty::U32 { min: 10, max: 100, step: 5, unit: "%" },
        label: "Intensity",
        #[cfg(test)]
        doc: "Brightness of the waveforms; 0 is refused, a value below 10 sets 10.",
        accept: Some((1.0, 100.0)),
        get: |m| Val::U32(m.intensity),
        put: |m, v| m.intensity = v.index() as u32,
    },
    Setting {
        key: "grid",
        scpi: Scpi::Cmd("GRID"),
        ty: Ty::Choice(
            &[
                Choice { store: "Full", scpi: "FULL", label: "Full" },
                Choice { store: "Light", scpi: "HALF", label: "Light" },
                Choice { store: "None", scpi: "NONE", label: "None" },
            ],
            Unknown::Value,
            Arg::Keyword,
        ),
        label: "Grid",
        #[cfg(test)]
        doc: "The graticule: every division line, a lighter one, or none.",
        accept: None,
        get: |m| Val::Index(GRIDS.iter().position(|&g| g == m.grid).unwrap_or(0)),
        put: |m, v| m.grid = GRIDS[v.index()],
    },
    Setting {
        key: "grid_bright",
        scpi: Scpi::Cmd("GBRightness"),
        ty: Ty::U32 { min: 10, max: 100, step: 10, unit: "%" },
        label: "Grid bright.",
        #[cfg(test)]
        doc: "Brightness of the graticule; a value below 10 sets 10.",
        accept: Some((0.0, 100.0)),
        get: |m| Val::U32(m.grid_bright),
        put: |m, v| m.grid_bright = v.index() as u32,
    },
    Setting {
        key: "grid_frame",
        scpi: Scpi::Cmd("RULers"),
        ty: Ty::Bool,
        label: "Grid frame",
        #[cfg(test)]
        doc: "A frame around the graticule with ticks every fifth of a division; none with the grid off.",
        accept: None,
        get: |m| Val::Bool(m.grid_frame),
        put: |m, v| m.grid_frame = v.index() != 0,
    },
];

/// Written after `zdelay`.
pub const DRAW: [Setting; 3] = [
    Setting {
        key: "color_grade",
        scpi: Scpi::Cmd("COLor"),
        ty: Ty::Bool,
        label: "Colour grade",
        #[cfg(test)]
        doc: "Colours the persistence by how often a point was hit.",
        accept: None,
        get: |m| Val::Bool(m.color_grade),
        put: |m, v| m.color_grade = v.index() != 0,
    },
    Setting {
        key: "dots",
        scpi: Scpi::Cmd("TYPE"),
        ty: Ty::Custom(&DOTS_IO),
        label: "Dots",
        #[cfg(test)]
        doc: "Draws every sample as a dot instead of joining them with lines.",
        accept: None,
        get: |m| Val::Bool(m.dots),
        put: |m, v| m.dots = v.index() != 0,
    },
    Setting {
        key: "bar",
        scpi: Scpi::None {
            #[cfg(test)]
            why: "a choice of the screen's layout, not of the measurement",
        },
        ty: Ty::Choice(
            &[
                Choice { store: "cards", scpi: "", label: "Cards" },
                Choice { store: "compact", scpi: "", label: "Compact" },
            ],
            Unknown::Names,
            Arg::Keyword,
        ),
        label: "Bottom bar",
        #[cfg(test)]
        doc: "The bottom bar as cards or as one compact line.",
        accept: None,
        get: |m| Val::Index(m.bar_compact as usize),
        put: |m, v| m.bar_compact = v.index() != 0,
    },
];

const GRIDS: [Grid; 3] = [Grid::Full, Grid::Light, Grid::None];

const PERSIST_LABELS: [&str; PERSIST.len()] = {
    let mut a = [""; PERSIST.len()];
    let mut i = 0;
    while i < a.len() {
        a[i] = PERSIST[i].0;
        i += 1;
    }
    a
};

/// An index into `PERSIST` in a setup; over SCPI `MIN`, `INFinite` or seconds, snapped to the nearest time of the
/// ladder.
const PERSIST_IO: CustomIo = CustomIo {
    store: |v| v.index().to_string(),
    load: |t| crate::store::p_idx(t, PERSIST.len()).map(Val::Index),
    query: |v| persist_token(v.index()),
    take: |a| persist_index(a).map(Val::Index),
    labels: &PERSIST_LABELS,
    #[cfg(test)]
    values: "`MIN`, `INFinite`, or seconds, set to the nearest time of the persistence ladder; the query answers \
             `MIN`, the seconds or `INF`",
};

/// A switch in a setup and in the menu; over SCPI the stock words of the drawing type.
const DOTS_IO: CustomIo = CustomIo {
    store: |v| (v.index() != 0).to_string(),
    load: |t| crate::store::p_bool(t).map(Val::Bool),
    query: |v| if v.index() != 0 { "DOTS" } else { "VECT" }.to_string(),
    take: |a| {
        if crate::scpi::kw(a, "VECTors") {
            Some(Val::Bool(false))
        } else if crate::scpi::kw(a, "DOTS") {
            Some(Val::Bool(true))
        } else {
            None
        }
    },
    labels: &["Off", "On"],
    #[cfg(test)]
    values: "`DOTS`, `VECTors`; the query answers `DOTS` or `VECT`",
};

fn persist_index(arg: &str) -> Option<usize> {
    let a = arg.trim();
    if crate::scpi::kw(a, "MIN") {
        return Some(0);
    }
    if crate::scpi::kw(a, "INFinite") {
        return Some(PERSIST.len() - 1);
    }
    let v = a.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)?;
    // the f32 seconds of PERSIST in f64 as written (whole milliseconds), so a time half-way picks as before
    let times: Vec<f64> = PERSIST.iter().filter(|p| p.1 >= 0.0).map(|p| (p.1 as f64 * 1e3).round() / 1e3).collect();
    Some(crate::scpi::nearest_index(&times, v))
}

/// The answer to `:DISPlay:GRADing:TIME?` for entry `i` of `PERSIST`.
fn persist_token(i: usize) -> String {
    match PERSIST.get(i).map(|p| p.1) {
        Some(0.0) => "MIN".into(),
        Some(s) if s > 0.0 => s.to_string(),
        _ => "INF".into(),
    }
}
