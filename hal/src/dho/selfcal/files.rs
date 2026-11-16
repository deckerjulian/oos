// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

use super::{rec_bytes, Rec, Results};
use crate::dho::calfile::{self, SaveError};

/// Writes the results to OOS's own files (the current OOS file, else the factory file, as the base for the rows and
/// channels not calibrated). Returns the files written.
pub fn save(r: &Results, rigol_dir: &str) -> Result<Vec<String>, SaveError> {
    save_to(r, rigol_dir, crate::dho::calfile::OOS_CAL_DIR)
}

fn save_to(r: &Results, rigol_dir: &str, own_dir: &str) -> Result<Vec<String>, SaveError> {
    let base = |n: &str| -> Result<Vec<u8>, String> {
        let (b, _) = calfile::load_first(n, &[own_dir, rigol_dir]).ok_or(format!("no valid {}", n))?;
        Ok(calfile::parse(&b).map_err(|e| format!("{}: {:?}", n, e))?.to_vec())
    };
    let mut files = Vec::new();
    let stamp = calfile::now();
    if !r.blocks.is_empty() {
        let p = vertical_payload(&base("cal_vertical.hex").map_err(std::io::Error::other)?, &r.blocks)
            .map_err(std::io::Error::other)?;
        files.push(("cal_vertical.hex", p));
    }
    if let Some(adc) = &r.adc {
        files.push(("cal_adc.hex", adc.payload().to_vec()));
    }
    if !r.zero.is_empty() {
        let mut p = base("cal_afe_zero.hex").map_err(std::io::Error::other)?;
        for (ch, z) in &r.zero {
            p = zero_payload(&p, *ch, z);
        }
        files.push(("cal_afe_zero.hex", p));
    }
    let refs: Vec<_> = files.iter().map(|(name, payload)| (*name, payload.as_slice())).collect();
    calfile::save_all(own_dir, &refs, stamp)?;
    Ok(files.iter().map(|(name, _)| format!("{}/{}", own_dir, name)).collect())
}

/// cal_vertical payload with some blocks replaced: `base` = the current payload (12 or 16 blocks x 801 x 56 bytes).
/// Extend missing groups from the last group, matching `Cal::rec` for untouched channels.
pub fn vertical_payload(base: &[u8], blocks: &[(usize, usize, Vec<Rec>)]) -> Result<Vec<u8>, String> {
    let group = 4 * 801 * 56;
    if base.is_empty() || !base.len().is_multiple_of(group) {
        return Err("cal_vertical.hex: payload must contain whole nonempty groups".into());
    }
    let mut p = base.to_vec();
    for (var, ch, recs) in blocks {
        while p.len() < (var + 1) * group {
            p.extend_from_within(p.len() - group..);
        }
        for (i, r) in recs.iter().enumerate().take(801) {
            let o = ((var * 4 + ch) * 801 + i) * 56;
            p[o..o + 56].copy_from_slice(&rec_bytes(r));
        }
    }
    Ok(p)
}

/// cal_afe_zero payload (4 x 20 s32) with channel `ch` replaced.
pub fn zero_payload(base: &[u8], ch: usize, z: &[i32; 20]) -> Vec<u8> {
    let mut p = base.to_vec();
    for (k, v) in z.iter().enumerate() {
        let o = (ch * 20 + k) * 4;
        p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    const GROUP: usize = 4 * 801 * 56;
    const STAMP: calfile::Stamp = (2000, 1, 1, 0, 0, 0);

    fn record() -> Rec {
        Rec { key: 123, g: [2; 8], gain: 1.5, zero: 17, last: 0 }
    }

    #[test]
    fn vertical_rejects_partial_or_empty_groups() {
        for len in [0, 1, GROUP - 1, GROUP + 1, 3 * GROUP + 56] {
            let base = vec![0; len];
            assert!(vertical_payload(&base, &[]).is_err(), "accepted {len} bytes");
            assert!(vertical_payload(&base, &[(3, 0, vec![])]).is_err());
        }
    }

    #[test]
    fn vertical_extends_legacy_groups_preserving_fallback() {
        let base: Vec<u8> = (0..3 * GROUP).map(|i| (i / GROUP + 1) as u8).collect();
        let got = vertical_payload(&base, &[(3, 1, vec![record()])]).unwrap();
        assert_eq!(got.len(), 4 * GROUP);
        assert_eq!(&got[..base.len()], base);
        let mut expected = base[2 * GROUP..].to_vec();
        expected[801 * 56..801 * 56 + 56].copy_from_slice(&rec_bytes(&record()));
        assert_eq!(&got[3 * GROUP..], expected);
        assert_eq!(vertical_payload(&base, &[(2, 1, vec![record()])]).unwrap().len(), base.len());
    }

    #[test]
    fn save_stages_every_file_before_replacing_any() {
        let dir = std::env::temp_dir().join(format!("oos-selfcal-stage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let own = dir.to_str().unwrap();
        let mut r = Results::default();
        r.blocks.push((0, 0, vec![record()]));
        r.adc = Some(crate::dho::adc::CalAdc::from_payload(vec![9; 0x738]).unwrap());
        r.zero.push((0, [7; 20]));
        let files = [
            ("cal_vertical.hex", vec![0; 4 * GROUP]),
            ("cal_adc.hex", vec![0; 0x738]),
            ("cal_afe_zero.hex", vec![0; 320]),
        ];
        for (name, payload) in &files {
            std::fs::write(dir.join(name), calfile::build(payload, STAMP)).unwrap();
        }
        std::fs::create_dir(dir.join("cal_afe_zero.hex.new")).unwrap();
        assert!(save_to(&r, own, own).is_err());
        for (name, payload) in &files {
            assert_eq!(
                std::fs::read(dir.join(name)).unwrap(),
                calfile::build(payload, STAMP),
                "{name} changed on failure"
            );
        }
        std::fs::remove_dir(dir.join("cal_afe_zero.hex.new")).unwrap();
        assert_eq!(save_to(&r, own, own).unwrap().len(), 3);
        for (name, payload) in &files {
            assert_eq!(std::fs::read(dir.join(format!("{name}.bak"))).unwrap(), calfile::build(payload, STAMP));
            assert!(calfile::parse(&std::fs::read(dir.join(name)).unwrap()).is_ok());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_resolves_every_base_before_publishing() {
        let dir = std::env::temp_dir().join(format!("oos-selfcal-base-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let own = dir.to_str().unwrap();
        let old = calfile::build(&vec![0; 4 * GROUP], STAMP);
        std::fs::write(dir.join("cal_vertical.hex"), &old).unwrap();
        let mut r = Results::default();
        r.blocks.push((0, 0, vec![record()]));
        r.zero.push((0, [7; 20]));
        assert!(save_to(&r, own, own).is_err());
        assert_eq!(std::fs::read(dir.join("cal_vertical.hex")).unwrap(), old);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
