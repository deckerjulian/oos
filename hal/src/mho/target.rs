// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The MHO900 described for the neutral code (`crate::target`): its card layout, models and limits (docs/mho.md).
//!
//! How it is known: the card layout and the boot card from the platform probe of one MHO954 (firmware 00.01.00,
//! hardware 3, GitHub issue #4): its `mtdparts` are those of the DHO800/DHO900 word for word. The models, channels and
//! bandwidths from Rigol's data sheet. Sample rates and memory depths are **assumed** from the data sheet; OOS does not
//! acquire on this board, they only shape the demo signal and the menus.

use crate::model::{AfgMax, Caps, Limits};
use crate::target::{Layout, Target, Variant};

const MHO900: &str = "MHO900";

/// The MHO900 models: (name, rated bandwidth in MHz, channels). The MHO98 edition is left out: the name its vendor
/// file gives is not known.
pub const MODEL_TABLE: [(&str, u32, usize); 4] = [
    ("MHO934", 350, 4),
    ("MHO954", 500, 4),
    ("MHO984", 800, 4),
    // not a model: the family name alone (vendor.bin unreadable but the user forces the series)
    ("MHO900", 500, 4),
];

/// One variant: the three models differ in their bandwidth licence only (data sheet). OOS has not run on any.
const VARIANTS: [Variant; 1] =
    [Variant { name: "MHO900, 4 channels", models: &["MHO934", "MHO954", "MHO984"], ran_on: &[] }];

/// Capabilities of a model name (case-insensitive). The generator is an option whose presence vendor.bin is not known
/// to tell, and OOS does not drive it: none. The logic analyser port is there (PLA2216, data sheet), not driven either.
fn caps(name: &str) -> Option<Caps> {
    let n = name.trim().to_ascii_uppercase();
    let &(tn, bw, channels) = MODEL_TABLE.iter().find(|e| e.0 == n)?;
    Some(Caps {
        name: tn.to_string(),
        series: MHO900,
        channels,
        rated_bw_mhz: bw,
        // no stock bandwidth code is known for the MHO900
        bw_code: 0,
        la: true,
        generators: 0,
        ext_trig: false,
        edu: false,
    })
}

/// A model name of the MHO900 family (MHO9xx), also one missing from the table.
pub fn family(name: &str) -> bool {
    let n = name.trim().to_ascii_uppercase();
    let b = n.as_bytes();
    n.starts_with("MHO9") && b.len() >= 5 && b[4].is_ascii_digit()
}

/// The standard limits, **assumed** from the data sheet: 4 GSa/s with one channel, 100 Mpts standard memory.
fn limits() -> Limits {
    Limits {
        max_srate: [4e9, 2e9, 1e9],
        max_mem: [100_000_000, 50_000_000, 25_000_000],
        max_mem_la: [25_000_000, 12_500_000, 10_000_000],
        // no generator is driven
        afg_max_hz: AfgMax { sine: 0.0, square: 0.0, ramp: 0.0, pulse: 0.0, noise: 0.0, dc: 0.0, arb: 0.0 },
        adc_clock_hz: 2e9,
        afg_clock_hz: 0.0,
    }
}

fn set_adc_clock(_: &mut Limits, _: f64) -> Result<(), String> {
    Err("the MHO900's clocks are not driven by OOS".into())
}

const CLOCK_STEPS: [f64; 1] = [2e9];

/// The stock card (probe of an MHO954): the partitions of the DHO800/DHO900, PARM offsets from sector 0x2000.
const BACKUP_LBA: u64 = 319488;

pub const MHO: Target = Target {
    name: "mho",
    family_name: "MHO900",
    update_target: "mho900-recovery",
    layout: Layout {
        parm_lba: 8192,
        bcb_lba: 32800,
        boot_lba: 122880,
        recovery_lba: 188416,
        backup_lba: BACKUP_LBA,
        slot_lba: BACKUP_LBA + 0x10000,
        image_sectors: 0x20000,
        oosdata_lba: 516096,
        oosdata_sectors: 0x8000,
        head_sectors: 73728,
        boot_bytes: 32 << 20,
    },
    parm_marks: &[
        "parm",
        "0x00002000@0x00006000(misc)",
        "0x00010000@0x0001c000(boot)",
        "0x00020000@0x0002c000(recovery)",
        "0x00038000@0x0004c000(backup)",
    ],
    card_controller: "fe320000",
    variants: &VARIANTS,
    assumed_model: "MHO954",
    caps,
    family,
    limits,
    set_adc_clock,
    clock_steps: &CLOCK_STEPS,
    acquisition: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_models_belong_to_the_family_and_no_dho_does() {
        for (m, _, _) in MODEL_TABLE {
            assert!(family(m), "{}", m);
            assert!(caps(m).is_some(), "{}", m);
        }
        for m in ["DHO924S", "DHO804", "MHO", "HDO1074", "MHOx"] {
            assert!(!family(m), "{}", m);
        }
        assert!(family("mho964"), "a name missing from the table still is one of the family");
        assert_eq!(caps("mho954").map(|c| c.rated_bw_mhz), Some(500));
    }

    /// The card layout of the MHO900 is that of the DHO800/DHO900 (the probe's mtdparts are the same); so is the
    /// check of the partition table before anything is written.
    #[test]
    fn the_card_is_laid_out_as_on_the_dho() {
        let d = crate::dho::target::DHO;
        assert_eq!(MHO.layout, d.layout);
        assert_eq!(MHO.parm_marks, d.parm_marks);
        assert_eq!(MHO.card_controller, d.card_controller);
        assert!(!MHO.acquisition && d.acquisition);
        assert!(MHO.verified_models().is_empty(), "OOS has run on no MHO900");
        assert_ne!(MHO.update_target, d.update_target, "a DHO package must not install on an MHO");
    }
}
