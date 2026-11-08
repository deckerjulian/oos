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
    /// models OOS has run on: the first install from the stock system takes no other without being told
    pub verified_models: &'static [&'static str],
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
