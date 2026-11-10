// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Calibration file container of the stock system (docs/protocols/self-cal.md 7.1): u32 CRC32 of the
//! 20-byte header, u32 0x14, header (version 0, time, date, payload length, CRC32 of the payload), payload. OOS reads
//! the factory files in /rigol/data and writes its own results with the same layout to its own directory (the /oos
//! data partition), atomically (temporary file, fsync, rename; the previous file kept as .bak). Paths under /rigol
//! are refused.

/// Directory of OOS's own calibration files.
pub const OOS_CAL_DIR: &str = "/oos/cal";

/// zlib / IEEE 802.3 CRC32 (also used by the neutral part of the crate).
pub use crate::crc32;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Short,
    HeaderSize,
    HeaderCrc,
    Length,
    PayloadCrc,
}

/// Time stamp of the header: (year, month, day, hour, minute, second).
pub type Stamp = (u32, u32, u32, u32, u32, u32);

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// The payload of a container, both CRCs and the length checked.
pub fn parse(d: &[u8]) -> Result<&[u8], Error> {
    let (hcrc, hsize) = (u32_at(d, 0).ok_or(Error::Short)?, u32_at(d, 4).ok_or(Error::Short)?);
    if hsize != 0x14 {
        return Err(Error::HeaderSize);
    }
    let h = d.get(8..28).ok_or(Error::Short)?;
    if crc32(h) != hcrc {
        return Err(Error::HeaderCrc);
    }
    let len = u32_at(d, 20).ok_or(Error::Short)? as usize;
    let p = d.get(28..28 + len).ok_or(Error::Length)?;
    if crc32(p) != u32_at(d, 24).ok_or(Error::Short)? {
        return Err(Error::PayloadCrc);
    }
    Ok(p)
}

/// The header stamp of a container.
pub fn stamp(d: &[u8]) -> Option<Stamp> {
    let (t, dt) = (u32_at(d, 12)?, u32_at(d, 16)?);
    Some((dt >> 8, dt & 0xff, t >> 24, (t >> 16) & 0xff, (t >> 8) & 0xff, t & 0xff))
}

/// A container around `payload`.
pub fn build(payload: &[u8], s: Stamp) -> Vec<u8> {
    let mut h = Vec::with_capacity(20);
    for v in [0, s.2 << 24 | s.3 << 16 | s.4 << 8 | s.5, s.0 << 8 | s.1, payload.len() as u32, crc32(payload)] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = crc32(&h).to_le_bytes().to_vec();
    out.extend_from_slice(&0x14u32.to_le_bytes());
    out.extend_from_slice(&h);
    out.extend_from_slice(payload);
    out
}

/// UTC now as a stamp (the unit has no time zone set under OOS).
pub fn now() -> Stamp {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86400, secs % 86400);
    // civil from days (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y as u32, m as u32, d as u32, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

/// Writes `name` in `dir` atomically: `name.new`, fsync, the old file to `name.bak`, rename. Never under /rigol.
pub fn save(dir: &str, name: &str, payload: &[u8], s: Stamp) -> std::io::Result<()> {
    use std::io::Write;
    let refuse = |m: &str| std::io::Error::new(std::io::ErrorKind::PermissionDenied, m.to_string());
    let canon = std::path::Path::new(dir).components().collect::<std::path::PathBuf>();
    if canon.starts_with("/rigol") || dir.contains("..") || name.contains('/') {
        return Err(refuse("calibration files are never written under /rigol"));
    }
    // without the data partition, /oos is a directory of the initramfs: the file would be gone at the next start
    if cfg!(target_os = "linux") && canon.starts_with("/oos") && !std::fs::read_to_string("/proc/mounts").is_ok_and(|m| oos_mounted(&m)) {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "/oos is not mounted (see /tmp/oos-data.log): the result would be lost at the next start"));
    }
    std::fs::create_dir_all(dir)?;
    let path = format!("{}/{}", dir.trim_end_matches('/'), name);
    let tmp = format!("{}.new", path);
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&build(payload, s))?;
    f.sync_all()?;
    drop(f);
    if std::path::Path::new(&path).exists() {
        std::fs::rename(&path, format!("{}.bak", path))?;
    }
    std::fs::rename(&tmp, &path)?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// The data partition is mounted at /oos, by the text of /proc/mounts.
pub fn oos_mounted(mounts: &str) -> bool {
    mounts.lines().any(|l| l.split_whitespace().nth(1) == Some("/oos"))
}

/// The file `name`: OOS's own copy if it is valid, else /rigol/data, else /rigol/data/default (stock order with the
/// OOS directory in front). Returns the bytes and the directory used.
pub fn load_first(name: &str, dirs: &[&str]) -> Option<(Vec<u8>, String)> {
    dirs.iter().find_map(|d| {
        let b = std::fs::read(format!("{}/{}", d.trim_end_matches('/'), name)).ok()?;
        parse(&b).ok()?;
        Some((b, d.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_partition_mounted() {
        assert!(oos_mounted("rootfs / rootfs rw 0 0\n/dev/mmcblk0p1 /oos ext4 rw,noatime 0 0\n"));
        assert!(!oos_mounted("rootfs / rootfs rw 0 0\n/dev/sda1 /oos/usb vfat rw 0 0\n/dev/mmcblk0p9 /rigol ext4 ro 0 0\n"));
    }

    fn unit_dir() -> String {
        std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal-full").into())
    }

    #[test]
    fn unit_files_parse_and_rebuild_byte_for_byte() {
        let dir = unit_dir();
        let mut n = 0;
        for f in ["cal_adc.hex", "cal_afe_bandwidth.hex", "cal_afe_zero.hex", "cal_lsb.hex", "cal_vertical.hex", "cal_ddr.hex", "cal_la.hex", "cal_afg.hex"] {
            let Ok(b) = std::fs::read(format!("{}/{}", dir, f)) else { continue };
            let p = parse(&b).unwrap_or_else(|e| panic!("{}: {:?}", f, e));
            assert_eq!(build(p, stamp(&b).unwrap()), b, "{}", f);
            n += 1;
        }
        if n == 0 {
            eprintln!("calfile: no unit files in {}, skipped", dir);
        }
    }

    #[test]
    fn errors_and_save_refuses_rigol() {
        let b = build(&[1, 2, 3, 4], (2026, 11, 3, 21, 5, 9));
        assert_eq!(parse(&b), Ok(&[1u8, 2, 3, 4][..]));
        assert_eq!(stamp(&b), Some((2026, 11, 3, 21, 5, 9)));
        let mut bad = b.clone();
        bad[30] ^= 1;
        assert_eq!(parse(&bad), Err(Error::PayloadCrc));
        bad = b.clone();
        bad[12] ^= 1;
        assert_eq!(parse(&bad), Err(Error::HeaderCrc));
        assert_eq!(parse(&b[..20]), Err(Error::Short));
        assert!(save("/rigol/data", "cal_vertical.hex", &[0], now()).is_err());
        assert!(save("/rigol//data/", "x", &[0], now()).is_err());
        assert!(save("/tmp/../rigol/data", "x", &[0], now()).is_err());
        // atomic save with .bak
        let d = std::env::temp_dir().join(format!("oos-calfile-{}", std::process::id()));
        let ds = d.to_str().unwrap();
        save(ds, "c.hex", &[7; 10], now()).unwrap();
        save(ds, "c.hex", &[8; 10], now()).unwrap();
        assert_eq!(parse(&std::fs::read(d.join("c.hex")).unwrap()), Ok(&[8u8; 10][..]));
        assert_eq!(parse(&std::fs::read(d.join("c.hex.bak")).unwrap()), Ok(&[7u8; 10][..]));
        let (_, from) = load_first("c.hex", &["/nonexistent", ds]).unwrap();
        assert_eq!(from, ds);
        std::fs::remove_dir_all(&d).unwrap();
        let (y, m, day, ..) = now();
        assert!(y >= 2026 && (1..=12).contains(&m) && (1..=31).contains(&day));
    }
}
