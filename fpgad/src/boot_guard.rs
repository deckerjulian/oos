// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

// A second BOOT while PCIe/XDMA was loaded hung the SoC on the unit (2026-11-03).
// --force deliberately bypasses both module checks; it does not make a retry safe.
pub(super) fn check(cmd: &str, force: bool, loaded: impl Fn(&str) -> bool) -> Result<(), String> {
    if (cmd == "cold-boot" || cmd == "fpga-boot")
        && ["xdma", "pcierockchiphost"].iter().any(|name| loaded(name))
        && !force
    {
        return Err(concat!(
            "XDMA or the PCIe host driver is already loaded (an earlier cold boot or hand-over in this boot): ",
            "power cycle the scope instead of retrying"
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_refuses_either_loaded_module_unless_forced() {
        for cmd in ["cold-boot", "fpga-boot", "status", "pll"] {
            for modules in [vec![], vec!["xdma"], vec!["pcierockchiphost"], vec!["xdma", "pcierockchiphost"]] {
                for force in [false, true] {
                    let blocked = matches!(cmd, "cold-boot" | "fpga-boot") && !force && !modules.is_empty();
                    assert_eq!(
                        check(cmd, force, |n| modules.contains(&n)).is_err(),
                        blocked,
                        "{cmd} {modules:?} force={force}"
                    );
                }
            }
        }
    }
}
