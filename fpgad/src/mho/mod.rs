// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The commands of oos-fpgad for the MHO900 (docs/mho.md, ADR 0028): none yet. OOS drives no hardware of its
//! acquisition board, so every command but `model` (answered by the neutral part) and `help` is refused.

/// Runs `cmd`: `help` prints what this build does; every other command is refused with exit code 3.
pub(crate) fn run(cmd: &str, args: &[String]) {
    let _ = args; // no command of this family takes options
    let family = oos_hal::target::ACTIVE.family_name;
    if matches!(cmd, "help" | "--help" | "-h") {
        println!(
            "oos-fpgad for the {}: `model` prints the unit; OOS drives no {} hardware (docs/mho.md)",
            family, family
        );
        return;
    }
    eprintln!("oos-fpgad: OOS drives no {} hardware (docs/mho.md)", family);
    std::process::exit(3);
}
