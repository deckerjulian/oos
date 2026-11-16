// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::dho::calfile;

#[test]
fn adc_skips_wrong_size_and_bad_crc_at_each_location() {
    let dir = std::env::temp_dir().join(format!("oos-adc-fallback-{}", std::process::id()));
    let paths = [dir.join("own"), dir.join("factory"), dir.join("factory/default")];
    for p in &paths {
        std::fs::create_dir_all(p).unwrap();
    }
    let dirs: Vec<&str> = paths.iter().map(|p| p.to_str().unwrap()).collect();
    let stamp = (2000, 1, 1, 0, 0, 0);
    for (k, p) in paths.iter().enumerate() {
        std::fs::write(p.join("cal_adc.hex"), calfile::build(&vec![k as u8; 0x738], stamp)).unwrap();
    }
    assert_eq!(CalAdc::load_dirs(&dirs).unwrap().payload(), &[0; 0x738]);
    let good = calfile::build(&vec![7; 0x738], stamp);
    let mut header = good.clone();
    header[0] ^= 1;
    let mut payload = good.clone();
    payload[28] ^= 1;
    let short = good[..good.len() - 1].to_vec();
    for invalid in [calfile::build(&[0; 4], stamp), header, payload, short] {
        for (k, path) in paths.iter().enumerate() {
            for p in &paths {
                let _ = std::fs::remove_file(p.join("cal_adc.hex"));
            }
            std::fs::write(path.join("cal_adc.hex"), &invalid).unwrap();
            if let Some(next) = paths.get(k + 1) {
                std::fs::write(next.join("cal_adc.hex"), &good).unwrap();
                assert_eq!(CalAdc::load_dirs(&dirs).unwrap().payload(), &[7; 0x738]);
            } else {
                assert!(CalAdc::load_dirs(&dirs).is_err());
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
