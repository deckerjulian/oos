// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The description of a board family: everything the neutral code needs to know about it (docs/porting.md). Where
//! OOS lives on the card, what an update package is for, which models belong to it and what they can do.
//!
//! An image is built for one board family, so the choice is made when OOS is built (`ACTIVE`). The scripts in the
//! image and the build read the same values from `rootfs/board/<name>/etc/oos-board`; a test keeps the two equal.

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
}

/// The board this build is for.
pub const ACTIVE: Target = crate::dho::target::DHO;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn rootfs() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rootfs")
    }

    fn board_file() -> HashMap<String, String> {
        let t = std::fs::read_to_string(rootfs().join("board").join(ACTIVE.name).join("etc/oos-board")).unwrap();
        t.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string()))
            .collect()
    }

    /// The scripts of the image and the code must agree on where things are on the card.
    #[test]
    fn the_scripts_see_the_same_board() {
        let f = board_file();
        let n = |k: &str| f[k].parse::<u64>().unwrap_or_else(|_| panic!("{} = {}", k, f[k]));
        let l = ACTIVE.layout;
        assert_eq!(f["OOS_BOARD"], ACTIVE.name);
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
            assert_eq!(n(k), v, "{}", k);
        }
        assert_eq!(f["OOS_CARD_CONTROLLER"], ACTIVE.card_controller);
        for m in f["OOS_PARM_MARKS"].split_whitespace() {
            assert!(ACTIVE.parm_marks.contains(&m), "{}", m);
        }
        // the kernel gets the data partition from the command line
        let part = format!("blkdevparts=mmcblk0:{}M@{}M(oosdata)", l.oosdata_sectors / 2048, l.oosdata_lba / 2048);
        assert!(f["OOS_BOOTARGS"].contains(&part), "{}", f["OOS_BOOTARGS"]);
    }

    /// The start-up and the common scripts ask the board (`oos-board`, `/etc/oos-board`) and know no address of it.
    #[test]
    fn the_common_scripts_know_no_board() {
        let f = board_file();
        let mut bad = Vec::new();
        let dir = rootfs().join("initramfs");
        let mut files = vec![dir.join("etc/rcS")];
        files.extend(std::fs::read_dir(dir.join("bin")).unwrap().flatten().map(|e| e.path()));
        let mut words: Vec<String> = ["oos-fpgad".to_string(), ACTIVE.card_controller.to_string()].to_vec();
        words.extend(f.iter().filter(|(k, _)| k.ends_with("_LBA")).map(|(_, v)| v.clone()));
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
