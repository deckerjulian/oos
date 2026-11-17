// Copyright (C) 2026 Jeroen
// SPDX-License-Identifier: GPL-2.0-or-later

//! Host checks of OOS that read the Rust source. `tools/check.sh` runs them as layers (docs/adr/process/0020);
//! by hand: `cargo run --manifest-path xtask/Cargo.toml -- test-time`.

mod exceptions;
mod sources;
mod test_time;

use std::path::Path;
use std::process::ExitCode;

/// The crates whose tests are checked.
const CRATES: [&str; 4] = ["hal", "dsp", "ui", "fpgad"];

fn check_test_time(repo: &Path) -> Result<Vec<String>, String> {
    let text =
        std::fs::read_to_string(repo.join(exceptions::FILE)).map_err(|e| format!("{}: {e}", exceptions::FILE))?;
    let allowed = exceptions::parse(&text)?;
    let mut found = Vec::new();
    for krate in CRATES {
        for source in sources::of_crate(repo, krate)? {
            found.extend(test_time::findings(&source));
        }
    }
    found.sort();
    Ok(exceptions::apply(&allowed, &found))
}

fn main() -> ExitCode {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new(".."));
    let check = std::env::args().nth(1).unwrap_or_default();
    let result = match check.as_str() {
        "test-time" => check_test_time(repo),
        _ => Err(format!("unknown check '{check}' (checks: test-time)")),
    };
    match result {
        Ok(problems) if problems.is_empty() => ExitCode::SUCCESS,
        Ok(problems) => {
            for p in &problems {
                println!("{p}");
            }
            println!(
                "{} place(s) where a test depends on the speed of the computer (docs/adr/process/0020): wait for the \
                 event instead of sleeping, supply the time instead of reading the clock, give a time-out of at \
                 least a minute as a hang guard",
                problems.len()
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
