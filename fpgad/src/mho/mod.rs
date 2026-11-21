// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The commands of oos-fpgad for the MHO900 (docs/mho.md, ADR 0028). OOS does not run an acquisition on this board;
//! nothing starts its hardware by itself.
//!
//!   oos-fpgad bringup --hw-code N [--until power|clock|fpga|pcie] [--yes]
//!       the first bring-up step, started by hand by an owner who tests it (docs/mho.md, "Bring-up, step 1"): the
//!       supplies, the sample clock from the unit's own table, the FPGA from the unit's own image (volatile: lost at
//!       power-off), PCIe, and a read of the first registers. Not yet verified on hardware.
//!
//! Every other command but `model` (answered by the neutral part) and `help` is refused.

#[cfg(target_os = "linux")]
mod bringup;

/// Runs `cmd`: `help` prints what this build does; `bringup` the step above; every other command is refused with exit
/// code 3.
pub(crate) fn run(cmd: &str, args: &[String]) {
    let family = oos_hal::target::ACTIVE.family_name;
    match cmd {
        "help" | "--help" | "-h" => {
            println!(
                "oos-fpgad for the {}: `model` prints the unit; `bringup --hw-code N [--until power|clock|fpga|pcie] \
                 [--yes]` is the first bring-up step, by hand only (docs/mho.md). Nothing else drives {} hardware.",
                family, family
            );
        }
        "bringup" => {
            #[cfg(target_os = "linux")]
            std::process::exit(bringup::run(args));
            #[cfg(not(target_os = "linux"))]
            {
                let _ = args;
                eprintln!("oos-fpgad: bringup runs on the scope only");
                std::process::exit(3);
            }
        }
        _ => {
            eprintln!("oos-fpgad: OOS drives no {} hardware (docs/mho.md)", family);
            std::process::exit(3);
        }
    }
}
