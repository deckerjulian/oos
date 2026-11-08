// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Calibration files the cold boot reads (never writes): `cal_adc.hex` (ADC parameters of mode 0) and `cal_ddr.hex` (DDR skip
//! taps). Search order: `<dir>/<name>`, then `<dir>/default/<name>` (cold-boot.md 1.3.1). Container:
//! 20-byte header, u32 payload length, u32 crc32 (zlib/IEEE) of the payload, payload.
//! Only `std::fs::read` is used here: nothing under /rigol/data is ever created or modified.

use crate::dho::adc::CalAdc;

/// CRC-32 (IEEE 802.3, reflected, as zlib.crc32).
pub fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Payload of a cal container if the length and the crc32 are right.
pub fn payload(d: &[u8]) -> Option<&[u8]> {
    let p = crate::dho::adc::container(d)?;
    let crc = u32::from_le_bytes(d.get(24..28)?.try_into().ok()?);
    (crc32(p) == crc).then_some(p)
}

/// `cal_ddr.hex`: 48-byte payload (cold-boot.md 3.2) [N].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DdrCal {
    /// wrlvl_coarse, wrlvl_cpt, mpr_cpt, prbs_final_dqs, dq_idelay, (always 0): the values for 0x1470, 0x1474, 0x147c,
    /// 0x1484, 0x148c, 0x1494
    pub taps: [u32; 6],
}

impl DdrCal {
    pub fn from_file_bytes(d: &[u8]) -> Option<DdrCal> {
        let p = payload(d)?;
        if p.len() != 48 {
            return None;
        }
        // u32 (+4 pad), four u64 (only the low word matters for the 12-bit fields), u32 (+4 pad)
        let mut taps = [0u32; 6];
        for (k, t) in taps.iter_mut().enumerate() {
            *t = u32::from_le_bytes(p[8 * k..8 * k + 4].try_into().unwrap());
        }
        Some(DdrCal { taps })
    }
}

/// The calibration data of the cold boot, with what the loader said.
#[derive(Default)]
pub struct CalFiles {
    pub adc: Option<CalAdc>,
    pub ddr: Option<DdrCal>,
    /// one line per file (path used, or why it was skipped)
    pub notes: Vec<String>,
}

fn read_first(own: Option<&str>, dir: &str, name: &str, parse: impl Fn(&[u8]) -> bool, notes: &mut Vec<String>) -> Option<Vec<u8>> {
    let dir = dir.trim_end_matches('/');
    let first = own.map(|d| format!("{}/{}", d, name));
    for path in first.into_iter().chain([format!("{}/{}", dir, name), format!("{}/default/{}", dir, name)]) {
        match std::fs::read(&path) {
            Ok(d) if parse(&d) => {
                notes.push(format!("{}: {} bytes, crc ok", path, d.len()));
                return Some(d);
            }
            Ok(d) => notes.push(format!("{}: {} bytes, bad container or crc, ignored", path, d.len())),
            Err(_) => {}
        }
    }
    notes.push(format!("{}: not found in {} or {}/default", name, dir, dir));
    None
}

impl CalFiles {
    /// Reads `cal_adc.hex` (OOS's own self-cal result first, as `adc::CalAdc::load` does) and `cal_ddr.hex` (OOS never
    /// makes one) from `dir` (on the unit `/rigol/data`), read only.
    pub fn load(dir: &str) -> CalFiles {
        let mut notes = Vec::new();
        let adc = read_first(Some(crate::dho::calfile::OOS_CAL_DIR), dir, "cal_adc.hex", |d| payload(d).is_some() && CalAdc::from_file_bytes(d).is_some(), &mut notes)
            .and_then(|d| CalAdc::from_file_bytes(&d));
        let ddr = read_first(None, dir, "cal_ddr.hex", |d| DdrCal::from_file_bytes(d).is_some(), &mut notes).and_then(|d| DdrCal::from_file_bytes(&d));
        CalFiles { adc, ddr, notes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container as found on the unit (header words of the unit's cal_ddr.hex).
    pub fn container(p: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        for w in [0xad2b_9fefu32, 0x14, 0, 0x1209_0c2e, 0x0007_dd01, p.len() as u32, crc32(p)] {
            d.extend(w.to_le_bytes());
        }
        d.extend(p);
        d
    }

    #[test]
    fn crc_and_ddr_payload() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        let mut p = vec![0u8; 48];
        for (k, v) in [0x12u32, 0x4d6, 0x8a2, 0x924, 0x1ef, 0].iter().enumerate() {
            p[8 * k..8 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        let d = container(&p);
        assert_eq!(DdrCal::from_file_bytes(&d).unwrap().taps, [0x12, 0x4d6, 0x8a2, 0x924, 0x1ef, 0]);
        let mut bad = d.clone();
        bad[30] ^= 1;
        assert!(DdrCal::from_file_bytes(&bad).is_none());
        assert!(DdrCal::from_file_bytes(&d[..60]).is_none());
    }

    /// The unit's files (device data, not in the tree); skipped when missing.
    #[test]
    fn unit_files() {
        let dir = std::env::var("OOS_CAL_DIR").unwrap_or(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal").into());
        let c = CalFiles::load(&dir);
        if let Some(d) = c.ddr {
            assert_eq!(d.taps, [0x12, 0x4d6, 0x8a2, 0x924, 0x1ef, 0]); // = oos-hw ddr-cal, cold-boot.md 3.2
        } else {
            eprintln!("cal_ddr.hex not found, skipped");
        }
        let none = CalFiles::load("/nonexistent-oos-dir");
        assert!(none.adc.is_none() && none.ddr.is_none() && none.notes.len() == 2);
    }
}
