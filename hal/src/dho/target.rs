// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The DHO800/DHO900 described for the neutral code (`crate::target`): its card layout, models and limits.

use crate::model::{bw_code_mhz, AfgMax, Caps, Limits};
use crate::target::{Layout, Target, Variant};

const DHO800: &str = "DHO800";
const DHO900: &str = "DHO900";

/// The DHO800/DHO900 models: (name, bandwidth code, channels, series). DHO914S is taken as DHO914 + generator.
pub const MODEL_TABLE: [(&str, u32, usize, &str); 12] = [
    ("DHO802", 5, 2, DHO800),
    ("DHO804", 5, 4, DHO800),
    ("DHO812", 6, 2, DHO800),
    ("DHO814", 6, 4, DHO800),
    ("DHO824", 10, 4, DHO800),
    ("DHO914", 7, 4, DHO900),
    ("DHO914S", 7, 4, DHO900),
    ("DHO914-EDU", 7, 4, DHO900),
    ("DHO924", 10, 4, DHO900),
    ("DHO924S", 10, 4, DHO900),
    ("DHO924-EDU", 10, 4, DHO900),
    // not a model: the family name alone (vendor.bin unreadable but the user forces the series)
    ("DHO900", 10, 4, DHO900),
];

/// The hardware variants (docs/models.md 2.1). A model counts as verified for the first install when OOS has run on a
/// model of its variant; the risk it guards against is a kernel that hangs before the first program of the image runs.
///
/// Models are grouped only where they differ in nothing but the bandwidth code: the same series, the same number of
/// channels, the same generator and the same edition. The bandwidth code is what the stock software makes of the same
/// front end (it limits the RT1642 filter to it, docs/models.md 2), and the bandwidth is sold as a licence: a DHO804
/// has been seen identifying as a DHO824 in vendor.bin. Kept apart, because the hardware is not known to be the same:
/// - 2 and 4 channels (two front ends fewer, an EXT trigger input instead);
/// - a DHO900 with and without generator (the output stage), and the EDU editions (whether they have the LA port is
///   not known);
/// - DHO800 and DHO900 (logic analyser port, front panel).
///
/// "DHO900" in the model table is not a model (the series forced by hand) and belongs to no variant.
const VARIANTS: [Variant; 5] = [
    Variant { name: "DHO800, 4 channels", models: &["DHO804", "DHO814", "DHO824"], ran_on: &[("DHO804", "user report, several releases")] },
    Variant { name: "DHO800, 2 channels", models: &["DHO802", "DHO812"], ran_on: &[] },
    Variant { name: "DHO900 with generator", models: &["DHO914S", "DHO924S"], ran_on: &[("DHO924S", "the developer's unit")] },
    Variant { name: "DHO900 without generator", models: &["DHO914", "DHO924"], ran_on: &[] },
    Variant { name: "DHO900 EDU", models: &["DHO914-EDU", "DHO924-EDU"], ran_on: &[] },
];

/// Capabilities of a model name (case-insensitive): the table plus what the name implies (S = generator, DHO900 =
/// logic analyser port, 2 channels = EXT).
fn caps(name: &str) -> Option<Caps> {
    let n = name.trim().to_ascii_uppercase();
    let &(tn, code, channels, series) = MODEL_TABLE.iter().find(|e| e.0 == n)?;
    Some(Caps {
        name: tn.to_string(),
        series,
        channels,
        rated_bw_mhz: bw_code_mhz(code).unwrap_or(0),
        bw_code: code,
        la: series == DHO900,
        generators: tn.ends_with('S') as usize,
        ext_trig: channels == 2,
        edu: tn.ends_with("-EDU"),
    })
}

/// A model name of the DHO800/DHO900 family (DHO8xx, DHO9xx), also one missing from the table (a new variant). The
/// DHO1000 (DHO10xx) and DHO4000 (DHO40xx) are other boards.
pub fn family(name: &str) -> bool {
    let n = name.trim().to_ascii_uppercase();
    let b = n.as_bytes();
    n.starts_with("DHO") && b.len() >= 6 && matches!(b[3], b'8' | b'9') && b[4].is_ascii_digit() && b[5].is_ascii_digit()
}

/// The standard limits (the same for every DHO800/DHO900).
fn limits() -> Limits {
    Limits {
        max_srate: [1.25e9, 625e6, 312.5e6],
        max_mem: [125_000_000, 62_500_000, 31_250_000],
        max_mem_la: [31_250_000, 12_500_000, 10_000_000],
        // measured on a DHO924S: a ramp is clean to 5 MHz, a pulse with its 11 ns edges too. Above 60 MHz the image at
        // 156.25 MHz - f comes within 10 dB of a sine, and a square above 15 MHz is a sine
        afg_max_hz: AfgMax { sine: 60e6, square: 15e6, ramp: 5e6, pulse: 5e6, noise: 60e6, dc: 60e6, arb: 10e6 },
        adc_clock_hz: 1.25e9,
        afg_clock_hz: 156.25e6,
    }
}

/// Overclocking test (docs/protocols/pll.md): only clocks the LMX2582 generator can make, 1.25..2.0 GHz. Sample rates
/// and the generator clock follow; the cold boot writes the matching PLL table.
fn set_adc_clock(l: &mut Limits, hz: f64) -> Result<(), String> {
    if !(1.25e9..=2.0e9).contains(&hz) {
        return Err(format!("{:e} outside 1.25e9..2.0e9", hz));
    }
    super::lmx2582::table_for(hz)?;
    l.adc_clock_hz = hz;
    l.afg_clock_hz = hz / 8.0;
    l.max_srate = [hz, hz / 2.0, hz / 4.0];
    let nyq = l.afg_clock_hz / 2.0;
    let a = &mut l.afg_max_hz;
    for x in [&mut a.sine, &mut a.square, &mut a.ramp, &mut a.pulse, &mut a.noise, &mut a.dc, &mut a.arb] {
        *x = x.min(nyq);
    }
    Ok(())
}

/// The ADC clocks of System > Overclock, the standard clock first (measurements: docs/models.md 4.1).
const CLOCK_STEPS: [f64; 5] = [1.25e9, 1.375e9, 1.5e9, 1.75e9, 2e9];

/// The stock card of the DHO800/DHO900 (docs/boot.md): PARM offsets count from sector 0x2000.
const BACKUP_LBA: u64 = 319488;

pub const DHO: Target = Target {
    name: "dho",
    family_name: "DHO800/DHO900",
    update_target: "dho900-recovery",
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
        // loader, vendor storage, `parameter`, `uboot`, `trust`, `misc`, `resource`
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
    assumed_model: "DHO924S",
    caps,
    family,
    limits,
    set_adc_clock,
    clock_steps: &CLOCK_STEPS,
};
