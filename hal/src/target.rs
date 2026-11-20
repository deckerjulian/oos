// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The description of a board family: everything the neutral code needs to know about it (docs/porting.md). Where
//! OOS lives on the card, what an update package is for, which models belong to it and what they can do.
//!
//! An image is built for one board family, so the choice is made when OOS is built (`ACTIVE`): `OOS_BOARD=<name>`,
//! `dho` by default, sets `cfg(oos_board = "<name>")` for every crate (build.rs, ADR 0067). The scripts in the image
//! and the build read the same values from `rootfs/board/<name>/etc/oos-board`; a test keeps the two equal.

use crate::model::{Caps, Limits};

/// Where OOS lives on the boot card, in sectors of 512 bytes from the start of the card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// the partition table of the stock loader
    pub parm_lba: u64,
    /// the boot command of the stock loader
    pub bcb_lba: u64,
    /// `boot`: its first four bytes select stock or OOS
    pub boot_lba: u64,
    /// the running OOS image
    pub recovery_lba: u64,
    /// the stock copy of `boot` the loader falls back to
    pub backup_lba: u64,
    /// the previous OOS image
    pub slot_lba: u64,
    /// size of `recovery` and of the slot
    pub image_sectors: u64,
    /// the OOS data partition
    pub oosdata_lba: u64,
    pub oosdata_sectors: u64,
    /// everything in front of the partitions the first install saves on its own
    pub head_sectors: u64,
    /// the part of `boot` the loader needs, also the size of the stock copy
    pub boot_bytes: usize,
}

/// Models with the same hardware (board, FPGA, front ends), told apart only by what the stock software makes of it,
/// such as the rated bandwidth.
#[derive(Debug)]
pub struct Variant {
    /// as a person calls it
    pub name: &'static str,
    pub models: &'static [&'static str],
    /// the models of it OOS is known to have run on, and who says so; empty: not verified
    pub ran_on: &'static [(&'static str, &'static str)],
}

impl Variant {
    pub fn verified(&self) -> bool {
        !self.ran_on.is_empty()
    }
}

impl Target {
    /// The hardware variant of a model name (case-insensitive).
    pub fn variant(&self, model: &str) -> Option<&'static Variant> {
        let n = model.trim().to_ascii_uppercase();
        self.variants.iter().find(|v| v.models.contains(&n.as_str()))
    }

    /// Every model of a variant OOS has run on, in the order of the table.
    pub fn verified_models(&self) -> Vec<&'static str> {
        self.variants.iter().filter(|v| v.verified()).flat_map(|v| v.models.iter().copied()).collect()
    }
}

/// One board family.
pub struct Target {
    /// the name of its folders (`hal/src/<name>`, `ui/src/board/<name>`, `rootfs/board/<name>`)
    pub name: &'static str,
    /// the family as a person calls it
    pub family_name: &'static str,
    /// what an update package names as its target: a package for another board is refused
    pub update_target: &'static str,
    pub layout: Layout,
    /// what the stock partition table must say (lower case) before anything is written to the card
    pub parm_marks: &'static [&'static str],
    /// part of the sysfs path of the boot card's controller
    pub card_controller: &'static str,
    /// the hardware variants of the family: the first install from the stock system takes a model without being told
    /// only when OOS has run on a model of its variant
    pub variants: &'static [Variant],
    /// the model assumed when the unit does not say what it is
    pub assumed_model: &'static str,
    /// capabilities of a model name, `None` if it is not in the table
    pub caps: fn(&str) -> Option<Caps>,
    /// a name of this family, also a variant missing from the table
    pub family: fn(&str) -> bool,
    /// the standard limits
    pub limits: fn() -> Limits,
    /// applies an experimental ADC clock to the limits, or says why it cannot be made
    pub set_adc_clock: fn(&mut Limits, f64) -> Result<(), String>,
    /// the ADC clocks of System > Overclock, the standard clock first
    pub clock_steps: &'static [f64],
    /// OOS has code for the acquisition hardware of this board. False for a port of the platform alone (the MHO900):
    /// then nothing touches the FPGA, the front ends, the generator or the front panel, and the screen shows the demo
    /// signal (`model::Platform::drives_hardware`, docs/porting.md)
    pub acquisition: bool,
}

/// The board this build is for: `OOS_BOARD` when OOS is built, `dho` by default (build.rs, ADR 0067).
#[cfg(oos_board = "dho")]
pub const ACTIVE: Target = crate::dho::target::DHO;
#[cfg(oos_board = "mho")]
pub const ACTIVE: Target = crate::mho::target::MHO;

/// Every board family in the tree, `ACTIVE` among them (ADR 0067). `hal/tests/board_families.rs` keeps this list equal
/// to the folders of `rootfs/board/`, and the tests below compare each family's `etc/oos-board` with its `Target`.
pub const FAMILIES: &[Target] = &[crate::dho::target::DHO, crate::mho::target::MHO];

/// The memory depths of the Acquire menu of the board this build is for (name, points per channel; 0 = Auto first).
/// Setups store the position. Chosen together with `ACTIVE` (hal test `the_depths_come_from_the_active_family`); not
/// a field of `Target`, which would change the layout of `Target` in every program.
#[cfg(oos_board = "dho")]
pub const DEPTHS: &[(&str, u64)] = &crate::dho::target::DEPTHS;
#[cfg(oos_board = "mho")]
pub const DEPTHS: &[(&str, u64)] = &crate::mho::target::DEPTHS;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn rootfs() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rootfs")
    }

    fn board_file(name: &str) -> HashMap<String, String> {
        let t = std::fs::read_to_string(rootfs().join("board").join(name).join("etc/oos-board")).unwrap();
        t.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string()))
            .collect()
    }

    /// The family this build was made for is the one `OOS_BOARD` named, or `dho` without it (build.rs).
    #[test]
    fn active_is_the_family_oos_board_names() {
        let named = option_env!("OOS_BOARD").filter(|v| !v.is_empty()).unwrap_or("dho");
        assert_eq!(ACTIVE.name, named);
    }

    /// The scripts of the image and the code must agree on where things are on the card, for every family.
    #[test]
    fn the_scripts_see_the_same_board() {
        assert!(FAMILIES.iter().any(|t| t.name == ACTIVE.name), "ACTIVE is not in FAMILIES");
        for t in FAMILIES {
            the_scripts_see_the_same(t);
        }
    }

    fn the_scripts_see_the_same(t: &Target) {
        let f = board_file(t.name);
        let n = |k: &str| f[k].parse::<u64>().unwrap_or_else(|_| panic!("{}: {} = {}", t.name, k, f[k]));
        let l = t.layout;
        assert_eq!(f["OOS_BOARD"], t.name);
        for (k, v) in [
            ("OOS_PARM_LBA", l.parm_lba),
            ("OOS_BCB_LBA", l.bcb_lba),
            ("OOS_BOOT_LBA", l.boot_lba),
            ("OOS_RECOVERY_LBA", l.recovery_lba),
            ("OOS_BACKUP_LBA", l.backup_lba),
            ("OOS_SLOT_LBA", l.slot_lba),
            ("OOS_IMAGE_SECTORS", l.image_sectors),
            ("OOS_OOSDATA_LBA", l.oosdata_lba),
            ("OOS_OOSDATA_SECTORS", l.oosdata_sectors),
            ("OOS_BOOT_SECTORS", l.boot_bytes as u64 / 512),
        ] {
            assert_eq!(n(k), v, "{}: {}", t.name, k);
        }
        assert_eq!(f["OOS_CARD_CONTROLLER"], t.card_controller);
        // the same marks both ways: a mark left out of the file would let the scripts take a card without it. `parm`
        // is the magic, which the scripts check apart.
        let mut file: Vec<&str> = f["OOS_PARM_MARKS"].split_whitespace().collect();
        let mut code: Vec<&str> = t.parm_marks.iter().copied().filter(|m| *m != "parm").collect();
        file.sort_unstable();
        code.sort_unstable();
        assert_eq!(file, code, "{}: OOS_PARM_MARKS and Target::parm_marks", t.name);
        // the kernel gets the data partition from the command line
        let part = format!("blkdevparts=mmcblk0:{}M@{}M(oosdata)", l.oosdata_sectors / 2048, l.oosdata_lba / 2048);
        assert!(f["OOS_BOOTARGS"].contains(&part), "{}", f["OOS_BOOTARGS"]);
    }

    /// The start-up and the common scripts ask the board (`oos-board`, `/etc/oos-board`) and know no address of it.
    #[test]
    fn the_common_scripts_know_no_board() {
        let mut bad = Vec::new();
        let dir = rootfs().join("initramfs");
        let mut files = vec![dir.join("etc/rcS")];
        files.extend(std::fs::read_dir(dir.join("bin")).unwrap().flatten().map(|e| e.path()));
        let mut words: Vec<String> = vec!["oos-fpgad".to_string()];
        for t in FAMILIES {
            let f = board_file(t.name);
            words.push(t.card_controller.to_string());
            words.extend(f.iter().filter(|(k, _)| k.ends_with("_LBA")).map(|(_, v)| v.clone()));
        }
        for p in files {
            for line in std::fs::read_to_string(&p).unwrap().lines().filter(|l| !l.trim_start().starts_with('#')) {
                if words.iter().any(|w| line.contains(w.as_str())) {
                    bad.push(format!("{}: {}", p.display(), line.trim()));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
