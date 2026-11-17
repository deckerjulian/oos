// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! A snapshot of what every stored setup key and every documented SCPI setter does with valid, edge and bad values,
//! so that moving the settings into one declaration table (or any other refactoring) keeps their behaviour byte for
//! byte. `tools/compare-base.sh` sees the default set-up and a few scripted remote commands; this sees the rest.
//!
//! - `setup_probes`: every key `setup_text` writes (of the numbered families the first instance it writes: `ch1.*`,
//!   `m2.*`, `d2.*`), the legacy keys `set` still reads, and a few keys it refuses. Each probe value is loaded as one
//!   line through the setup reader (`parse_setup_lenient`); recorded are the key's line in `setup_text` afterwards, the
//!   reader's errors, and the other lines that changed with it.
//! - `scpi_probes`: every setter of `docs/scpi.md` with a query form (first instance of `<n>`), on a fresh SCPI
//!   session per probe: the setter's reply, the query's answer and the drained `:SYSTem:ERRor?` queue.
//!
//! The lines are sorted and compared with `ui/tests/golden/settings.txt` (not `ui/tests/*.txt`: those are screen
//! scripts that `build.sh test` and the comparison run). A change that is meant to change what a setting does
//! rewrites the file and shows the difference in the pull request:
//!
//! ```sh
//! (cd ui && OOS_BLESS=1 cargo test --locked -q settings_snapshot)
//! ```
//!
//! The two lists of what is probed are kept by hand, so two more tests keep them complete: every key `setup_text`
//! writes for a model with all its optional lines and decoder slots, and every key `set` names in its source, must be
//! in `KEYS` or `DECODER`; every setter `docs/scpi.md` writes with `[?]` must be in `SETTERS` or in `LEFT_OUT` with
//! a reason. Each fails with the list of what is missing.

mod scpi_probes;
mod setup_probes;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/settings.txt");

const HEADER: &str = "\
# What every stored setup key and every documented SCPI setter does with valid, edge and bad values, recorded from
# today's code by ui/src/store/settings_snapshot.rs; its test fails when the code does something else. A refactoring
# keeps this file byte for byte. A change meant to change behaviour rewrites it and shows the difference:
#   (cd ui && OOS_BLESS=1 cargo test --locked -q settings_snapshot)
# setup <key> <probe> -> <the key's line after loading `<key> = <probe>`>; error: <reader>; also: <other lines changed>
# scpi <header> <probe> -> set: <reply>; query: <answer>; errors: <:SYSTem:ERRor? until 0>, each on a fresh session
#   on the demo signal (:REFerence:* and :SIM:SHOW with R1 saved from CH1 first, :MASK:OPERate with the mask on)
";

/// A value as it goes into a line of the file: quoted when it is empty, has spaces at an end or control characters.
fn show(v: &str) -> String {
    if v.is_empty() || v.trim() != v || v.chars().any(char::is_control) {
        format!("{:?}", v)
    } else {
        v.to_string()
    }
}

/// `items` without repeats, in their first order.
fn unique(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|p| seen.insert(p.clone())).collect()
}

fn snapshot() -> String {
    let mut lines = setup_probes::lines();
    lines.extend(scpi_probes::lines());
    lines.sort();
    lines.dedup();
    let mut text = String::from(HEADER);
    for l in lines {
        text.push_str(&l);
        text.push('\n');
    }
    text
}

#[test]
// the golden file holds the keys and commands of every decoder; a build with the core decoders only has fewer
#[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
fn every_stored_key_and_scpi_setter_does_what_the_golden_file_says() {
    let text = snapshot();
    if std::env::var("OOS_BLESS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, &text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(GOLDEN).unwrap_or_default();
    if want == text {
        return;
    }
    let old: std::collections::HashSet<&str> = want.lines().collect();
    let new: std::collections::HashSet<&str> = text.lines().collect();
    let gone: Vec<&str> = want.lines().filter(|l| !new.contains(l)).take(20).collect();
    let came: Vec<&str> = text.lines().filter(|l| !old.contains(l)).take(20).collect();
    panic!(
        "settings behave differently from tests/golden/settings.txt\n--- the file says (first 20):\n{}\n+++ the code \
         does (first 20):\n{}\nA change meant to change behaviour: OOS_BLESS=1 cargo test --locked -q \
         settings_snapshot",
        gone.join("\n"),
        came.join("\n")
    );
}
