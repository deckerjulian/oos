// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Calibration files the cold boot reads (never writes): `cal_adc.hex` (ADC parameters of mode 0) and `cal_ddr.hex` (DDR skip
//! taps). Search order: `<dir>/<name>`, then `<dir>/default/<name>` (cold-boot.md 1.3.1). Container:
//! 20-byte header, u32 payload length, u32 crc32 (zlib/IEEE) of the payload, payload.
//! Only `std::fs::read` is used here: nothing under /rigol/data is ever created or modified.

use crate::dho::adc::CalAdc;

pub use crate::crc32;

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
        Self::load_from(Some(crate::dho::calfile::OOS_CAL_DIR), dir)
    }

    fn load_from(own: Option<&str>, dir: &str) -> CalFiles {
        let mut notes = Vec::new();
        let adc = read_first(own, dir, "cal_adc.hex", |d| CalAdc::from_file_bytes(d).is_some(), &mut notes)
            .and_then(|d| CalAdc::from_file_bytes(&d));
        let ddr = read_first(None, dir, "cal_ddr.hex", |d| DdrCal::from_file_bytes(d).is_some(), &mut notes).and_then(|d| DdrCal::from_file_bytes(&d));
        CalFiles { adc, ddr, notes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adc_boot_and_runtime_share_acceptance() {
        let dir = std::env::temp_dir().join(format!("oos-adc-acceptance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ds = dir.to_str().unwrap();
        let good = crate::dho::calfile::build(&vec![7; 0x738], (2000, 1, 1, 0, 0, 0));
        let mut header = good.clone();
        header[0] ^= 1;
        let mut payload = good.clone();
        payload[28] ^= 1;
        let wrong_size = crate::dho::calfile::build(&[7; 4], (2000, 1, 1, 0, 0, 0));
        for (d, valid) in [(&good, true), (&header, false), (&payload, false), (&wrong_size, false)] {
            std::fs::write(dir.join("cal_adc.hex"), d).unwrap();
            assert_eq!(CalFiles::load_from(None, ds).adc.is_some(), valid, "cold boot");
            assert_eq!(CalAdc::from_file_bytes(d).is_some(), valid, "runtime parser");
            assert_eq!(CalAdc::load(ds).is_ok(), valid, "runtime load");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        let dir = crate::dho::calfile::test_dir(false);
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
