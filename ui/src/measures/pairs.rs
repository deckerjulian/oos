// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The two-source variants of the scope (delay, phase, skew, setup and hold from source A to source B) on the
//! measurement page: the table of the variants (`PAIRS`), which the scope computes, and their tiles.

use super::*;
use Mark::*;

/// What a channel-pair row measures. The edge flags choose the edges of delay, phase and skew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairKind {
    Delay,
    Phase,
    Skew,
    Setup,
    Hold,
    SetupAndHold,
}

/// Channel pairs: name, kind, edge of A rising, edge of B rising. New rows stay at the end so a saved index still
/// means the same row.
pub const PAIRS: [(&str, PairKind, bool, bool); crate::model::PAIRS_N] = [
    ("Delay rise-rise", PairKind::Delay, true, true),
    ("Delay fall-fall", PairKind::Delay, false, false),
    ("Delay rise-fall", PairKind::Delay, true, false),
    ("Delay fall-rise", PairKind::Delay, false, true),
    ("Phase rise-rise", PairKind::Phase, true, true),
    ("Phase fall-fall", PairKind::Phase, false, false),
    ("Phase rise-fall", PairKind::Phase, true, false),
    ("Phase fall-rise", PairKind::Phase, false, true),
    ("Skew rise-rise", PairKind::Skew, true, true),
    ("Skew fall-fall", PairKind::Skew, false, false),
    ("Skew rise-fall", PairKind::Skew, true, false),
    ("Skew fall-rise", PairKind::Skew, false, true),
    ("Setup time", PairKind::Setup, true, true),
    ("Hold time", PairKind::Hold, true, true),
    ("Setup+Hold", PairKind::SetupAndHold, true, true),
    ("Setup fall", PairKind::Setup, false, true),
    ("Hold fall", PairKind::Hold, false, true),
];

/// The short names of the two-source variants (strip, SCPI).
pub fn pair_short_name(k: usize) -> &'static str {
    PAIRS[k.min(crate::model::PAIRS_N - 1)].0
}
/// The unit of a two-source variant: degrees for phase, none for the setup/hold violation count, else seconds.
pub fn pair_unit(k: usize) -> &'static str {
    match PAIRS[k.min(crate::model::PAIRS_N - 1)].1 {
        PairKind::Phase => "\u{b0}",
        PairKind::SetupAndHold => "",
        _ => "s",
    }
}

static DEFS: [Def; 17] = [
    pair(
        20,
        "Delay rise-rise",
        "s",
        Edge,
        0,
        &[(TWO, "DELAY")],
        "Time from the first rising edge of A to the next rising edge of B.",
    ),
    pair(
        230,
        "Delay fall-fall",
        "s",
        Edge,
        1,
        &[(TWO, "DELAY")],
        "Time from the first falling edge of A to the next falling edge of B.",
    ),
    pair(
        231,
        "Delay rise-fall",
        "s",
        Edge,
        2,
        &[(TWO, "DELAY")],
        "Time from the first rising edge of A to the next falling edge of B.",
    ),
    pair(
        232,
        "Delay fall-rise",
        "s",
        Edge,
        3,
        &[(TWO, "DELAY")],
        "Time from the first falling edge of A to the next rising edge of B.",
    ),
    pair(
        23,
        "Phase rise-rise",
        "\u{b0}",
        Edge,
        4,
        &[(TWO, "PHASE")],
        "Rising-edge delay from A to B as a fraction of the period, in degrees.",
    ),
    pair(
        233,
        "Phase fall-fall",
        "\u{b0}",
        Edge,
        5,
        &[(TWO, "PHASE")],
        "Falling-edge delay from A to B as a fraction of the period, in degrees.",
    ),
    pair(
        234,
        "Phase rise-fall",
        "\u{b0}",
        Edge,
        6,
        &[(TWO, "PHASE")],
        "Delay from a rising edge of A to a falling edge of B, in degrees.",
    ),
    pair(
        235,
        "Phase fall-rise",
        "\u{b0}",
        Edge,
        7,
        &[(TWO, "PHASE")],
        "Delay from a falling edge of A to a rising edge of B, in degrees.",
    ),
    pair(
        19,
        "Skew rise-rise",
        "s",
        Edge,
        8,
        &[(TWO, "SKEW")],
        "Mean gap from each rising edge of A to the nearest rising edge of B.",
    ),
    pair(
        236,
        "Skew fall-fall",
        "s",
        Edge,
        9,
        &[(TWO, "SKEW")],
        "Mean gap from each falling edge of A to the nearest falling edge of B.",
    ),
    pair(
        237,
        "Skew rise-fall",
        "s",
        Edge,
        10,
        &[(TWO, "SKEW")],
        "Mean gap from each rising edge of A to the nearest falling edge of B.",
    ),
    pair(
        238,
        "Skew fall-rise",
        "s",
        Edge,
        11,
        &[(TWO, "SKEW")],
        "Mean gap from each falling edge of A to the nearest rising edge of B.",
    ),
    pair(
        30,
        "Setup Time",
        "s",
        Edge,
        12,
        &[(TWO, "SETUP & HOLD")],
        "How long the data edge (A) leads the rising clock edge (B).",
    ),
    pair(
        31,
        "Hold Time",
        "s",
        Edge,
        13,
        &[(TWO, "SETUP & HOLD")],
        "How long the data (A) stays put after the rising clock edge (B).",
    ),
    pair(
        239,
        "Setup + Hold",
        "",
        Edge,
        14,
        &[(TWO, "SETUP & HOLD")],
        "Rising clock edges (B) where the data (A) misses the Tsetup or Thold limit of the Measure menu.",
    ),
    pair(
        240,
        "Setup fall",
        "s",
        Edge,
        15,
        &[(TWO, "SETUP & HOLD")],
        "Setup time of the data (A) against the falling clock edge (B).",
    ),
    pair(
        241,
        "Hold fall",
        "s",
        Edge,
        16,
        &[(TWO, "SETUP & HOLD")],
        "Hold time of the data (A) after the falling clock edge (B).",
    ),
];

pub struct Pairs;

impl MeasModule for Pairs {
    fn key(&self) -> &'static str {
        "pairs"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }
}
