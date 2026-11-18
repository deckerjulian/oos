// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the parsers of oos-ui that read what comes from outside the program (ADR 0047): files, the
//! network, remote commands, the signal on the probes. Each parser has one test: valid inputs, mutated from a fixed
//! seed (hal's `hostile/gen.rs`), go in; it must return, not panic, and what it accepts must obey the contract the
//! test states. `cargo test hostile` runs them all, `OOS_HOSTILE_ITERS` raises the number of inputs.
//!
//! The tests are here by kind of input, or in the tests of the parser's own module where they need its private parts
//! (`store/hostile_tests.rs`, `http/tests/hostile.rs`, `hislip.rs`, `mdns.rs`, `announce.rs`); each has `hostile` in
//! its path. A new parser of outside input comes with its test. A finding is not fixed in the test: its smallest
//! input becomes an ignored test, and `KNOWN` names it so the run steps over that one case until the fix is merged.

mod conf;
// the generator of all three crates, one file
#[path = "../../hal/src/hostile/gen.rs"]
mod gen;
mod input;
mod network;
mod remote;
mod signals;

pub(crate) use gen::*;

/// The findings that still stand, each with its smallest input as an ignored test. The list only shrinks: a fix
/// removes its line and the `#[ignore]` of its test (`hostile_known_findings_still_stand` fails until it does).
pub(crate) const KNOWN: &[Known] = &[
    Known {
        name: "mdns-answer-it-cannot-read",
        what: "mdns: the answer to a question of control bytes and pointers is a packet mdns::parse refuses",
        repro: crate::mdns::tests::hostile_known_mdns_answer_it_cannot_read,
    },
    #[cfg(feature = "dec-qspi")]
    Known {
        name: "qspi-zero-frame",
        what: "the QSPI decoder (oos-dsp) panics on a CS# frame that ends where it starts with a clock edge there",
        repro: signals::hostile_known_qspi_zero_frame,
    },
];

/// Whether the finding `name` is on `KNOWN`.
pub(crate) fn is_known(name: &str) -> bool {
    known(KNOWN, name)
}

// Break it catches: a finding that was fixed and is still stepped over
#[test]
fn hostile_known_findings_still_stand() {
    still_stand(KNOWN);
}
