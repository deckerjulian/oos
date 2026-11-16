// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The unit an image was built for (`OOS_UNIT`, docs/models.md 4.2; docs/update.md, "Unit").
//!
//! An image built with `OOS_UNIT=<name>` carries that unit's defaults and device tree changes (a fixed MAC, the UHS-I
//! tree). Installed on that unit, a plain image (or one of another unit) would drop them without a word. So the image
//! names its unit in `/etc/oos-unit` (`none` without `OOS_UNIT`), `oos-update pack` puts it into the signed header
//! (`unit <name>`), and `oos-update install` refuses a package whose unit is not the running image's, unless told
//! `--any-unit`. A package or an image from before this has no unit: `none`.

use super::pkg::Manifest;

/// The unit of the running image (from its initramfs).
pub const FILE: &str = "/etc/oos-unit";
/// No `OOS_UNIT`.
pub const NONE: &str = "none";
/// The signed header line.
pub const KEY: &str = "unit";

/// A unit is a name from `rootfs/unit/`: 1 to 64 letters, digits, '.', '_' or '-', starting with a letter or digit.
pub fn valid(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
    if ok {
        Ok(name)
    } else {
        Err(format!("'{}' is no unit name (1 to 64 letters, digits, '.', '_', '-')", name.escape_debug()))
    }
}

/// The unit in `/etc/oos-unit` text; none when the file is missing (`None`).
pub fn of_text(text: Option<&str>) -> Result<String, String> {
    match text {
        None => Ok(NONE.into()),
        Some(t) => valid(t.trim()).map(str::to_string),
    }
}

/// The running image's unit.
pub fn running() -> Result<String, String> {
    match std::fs::read_to_string(FILE) {
        Ok(t) => of_text(Some(&t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => of_text(None),
        Err(e) => Err(format!("{}: {}", FILE, e)),
    }
}

/// The unit an image was built for (its initramfs' `/etc/oos-unit`), for `pack`.
pub fn of_image(img: &[u8]) -> Result<String, String> {
    let (_, ramdisk, _) = super::kexec::parts(img)?;
    let rd = super::inflate::gunzip(ramdisk, 512 << 20).map_err(|e| format!("initramfs: {}", e))?;
    let text = super::inflate::cpio_file(&rd, "etc/oos-unit").map(|b| std::str::from_utf8(b).map_err(|_| "etc/oos-unit is not text"));
    of_text(text.transpose()?)
}

/// The unit a package names; none for a package without the line.
pub fn of_package(m: &Manifest) -> Result<String, String> {
    let mut v = m.extra.iter().filter(|(k, _)| k == KEY).map(|(_, v)| v.as_str());
    let name = v.next().unwrap_or(NONE);
    if v.next().is_some() {
        return Err("the package names its unit twice".into());
    }
    valid(name).map(str::to_string)
}

/// Package against running image: `Ok(None)` when they match, `Ok(Some(warning))` for a mismatch with `any`, else
/// the refusal.
pub fn check(m: &Manifest, running: &str, any: bool) -> Result<Option<String>, String> {
    let unit = of_package(m)?;
    if unit == running {
        return Ok(None);
    }
    let what = format!("the package is built for unit '{}', this image for '{}'", unit, running);
    if any {
        Ok(Some(format!("warning: {} (--any-unit)", what)))
    } else {
        Err(format!("{}: refused, nothing written. --any-unit installs it anyway", what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &[(&str, &str)]) -> Manifest {
        Manifest {
            target: "dho".into(),
            build: "20261116-000000".into(),
            kernel: "6.12".into(),
            size: 0,
            sha256: [0; 32],
            key: [0; 8],
            extra: extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    #[test]
    fn names() {
        for n in ["none", "dho804", "a.b_c-1", &"x".repeat(64)] {
            assert!(valid(n).is_ok(), "{}", n);
        }
        for n in ["", "-x", ".x", "a b", "a/b", "../x", &"x".repeat(65)] {
            assert!(valid(n).is_err(), "{}", n);
        }
        assert_eq!(of_text(None).unwrap(), NONE);
        assert_eq!(of_text(Some("dho804\n")).unwrap(), "dho804");
        assert!(of_text(Some("")).is_err());
    }

    #[test]
    fn a_package_without_the_line_is_none() {
        assert_eq!(of_package(&manifest(&[("trial", "kexec")])).unwrap(), NONE);
        assert_eq!(of_package(&manifest(&[(KEY, "dho804")])).unwrap(), "dho804");
        assert!(of_package(&manifest(&[(KEY, "a"), (KEY, "b")])).is_err());
        assert!(of_package(&manifest(&[(KEY, "a/b")])).is_err());
    }

    #[test]
    fn mismatch_refused_unless_any() {
        let unit = manifest(&[(KEY, "dho804")]);
        let plain = manifest(&[]);
        assert_eq!(check(&unit, "dho804", false), Ok(None));
        assert_eq!(check(&plain, NONE, false), Ok(None));
        for (m, running) in [(&plain, "dho804"), (&unit, NONE), (&unit, "other")] {
            assert!(check(m, running, false).unwrap_err().contains("refused"));
            assert!(check(m, running, true).unwrap().unwrap().contains("--any-unit"));
        }
    }

    #[test]
    fn the_line_is_signed() {
        let seed = [7u8; 32];
        let key = crate::update::ed25519::public_key(&seed);
        let img = crate::update::tests::bootimg_with_ramdisk(vec![0; 64]);
        let p = crate::update::pkg::build_with(&seed, "20261116-000000", "6.12", &img, &[(KEY, "dho804")]).unwrap();
        let h = crate::update::pkg::verify_header(&p, &[key]).unwrap();
        assert_eq!(of_package(&h.manifest).unwrap(), "dho804");
        let text = String::from_utf8_lossy(&p).replacen("unit dho804", "unit dho805", 1);
        assert!(crate::update::pkg::verify_header(text.as_bytes(), &[key]).is_err(), "a changed unit breaks the signature");
    }

    #[test]
    fn the_unit_of_an_image() {
        let img = |files: &[(&str, u32, &[u8])]| {
            let mut rd = crate::update::inflate::tests::gzip_stored(&crate::update::kexec::newc(files));
            rd.resize(rd.len().div_ceil(4) * 4, 0);
            crate::update::tests::bootimg_with_ramdisk(rd)
        };
        assert_eq!(of_image(&img(&[("etc/oos-unit", 0o100644, b"dho804\n")])).unwrap(), "dho804");
        assert_eq!(of_image(&img(&[("etc/oos-build", 0o100644, b"x\n")])).unwrap(), NONE);
        assert!(of_image(&img(&[("etc/oos-unit", 0o100644, b"a b\n")])).is_err());
    }
}
