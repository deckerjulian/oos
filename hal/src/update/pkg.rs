// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The update package `.oosupd`: a signed text header, then the boot image for `recovery`.
//!
//! ```text
//! OOS-UPDATE 1
//! target dho900-recovery
//! build 20261105-165012
//! kernel 6.12.115+
//! size 30689280
//! sha256 <64 hex digits of the image>
//! key <16 hex digits: first 8 bytes of SHA-256 of the signing public key>
//! sig <128 hex digits: Ed25519 over every header byte before this line>
//!                                  <- empty line
//! <size bytes of image>
//! ```
//!
//! Readable with `head -9`. The signature covers the header up to and including the newline before `sig`, and the
//! header names the image's SHA-256, so it covers the image too. The line order is fixed (the parsed header must
//! print back to exactly the signed bytes); later versions can add `name value` lines between `sha256` and `key`.

use super::ed25519;
use super::hash::{hex, sha256, unhex, Sha256};

pub const MAGIC: &str = "OOS-UPDATE 1";
pub const TARGET: &str = "dho900-recovery";
/// Longest header accepted.
pub const MAX_HEADER: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub target: String,
    /// Build id, `YYYYMMDD-HHMMSS` (UTC) first: compared as text for the downgrade check.
    pub build: String,
    pub kernel: String,
    pub size: u64,
    pub sha256: [u8; 32],
    pub key: [u8; 8],
    /// Further signed `name value` lines, in order.
    pub extra: Vec<(String, String)>,
}

impl Manifest {
    /// The signed part of the header.
    pub fn signed_text(&self) -> String {
        let mut s = format!(
            "{}\ntarget {}\nbuild {}\nkernel {}\nsize {}\nsha256 {}\n",
            MAGIC,
            self.target,
            self.build,
            self.kernel,
            self.size,
            hex(&self.sha256)
        );
        for (k, v) in &self.extra {
            s.push_str(&format!("{} {}\n", k, v));
        }
        s.push_str(&format!("key {}\n", hex(&self.key)));
        s
    }

    /// The build id's sortable part (first word).
    pub fn build_key(&self) -> &str {
        self.build.split_whitespace().next().unwrap_or("")
    }
}

/// Key id: the first 8 bytes of SHA-256 of the public key.
pub fn key_id(pk: &[u8; 32]) -> [u8; 8] {
    let h = sha256(pk);
    let mut id = [0u8; 8];
    id.copy_from_slice(&h[..8]);
    id
}

/// Header + image, signed with `seed`.
pub fn build(seed: &[u8; 32], build: &str, kernel: &str, image: &[u8]) -> Result<Vec<u8>, String> {
    build_with(seed, build, kernel, image, &[])
}

/// `build` with further signed `name value` lines (e.g. `trial kexec`, `kexec::LINE`).
pub fn build_with(seed: &[u8; 32], build: &str, kernel: &str, image: &[u8], extra: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    for (n, v) in [("build", build), ("kernel", kernel)] {
        if v.is_empty() || v.contains('\n') {
            return Err(format!("{} must be one non-empty line", n));
        }
    }
    let fixed = ["target", "build", "kernel", "size", "sha256", "key", "sig"];
    for (k, v) in extra {
        if k.is_empty() || k.contains([' ', '\n']) || v.contains('\n') || fixed.contains(k) {
            return Err(format!("bad extra header line '{} {}'", k, v));
        }
    }
    let pk = ed25519::public_key(seed);
    let m = Manifest {
        target: TARGET.into(),
        build: build.into(),
        kernel: kernel.into(),
        size: image.len() as u64,
        sha256: sha256(image),
        key: key_id(&pk),
        extra: extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    };
    let text = m.signed_text();
    let sig = ed25519::sign(seed, text.as_bytes());
    let mut out = format!("{}sig {}\n\n", text, hex(&sig)).into_bytes();
    if out.len() > MAX_HEADER {
        return Err("header too long".into());
    }
    out.extend_from_slice(image);
    Ok(out)
}

/// A header that parsed and whose signature checked against one of `keys`.
#[derive(Clone, Debug)]
pub struct Header {
    pub manifest: Manifest,
    /// Byte offset of the image in the package.
    pub data_offset: usize,
}

/// Parse and verify the header at the start of `pkg` (the image is not looked at: see `check_image`).
pub fn verify_header(pkg: &[u8], keys: &[[u8; 32]]) -> Result<Header, String> {
    let lim = pkg.len().min(MAX_HEADER);
    let end = pkg[..lim]
        .windows(2)
        .position(|w| w == b"\n\n")
        .ok_or("not an OOS update package (no header end in the first 4 KiB)")?;
    let text = std::str::from_utf8(&pkg[..end + 1]).map_err(|_| "header is not text")?;
    let mut lines = text.lines();
    if lines.next() != Some(MAGIC) {
        return Err(format!("not an OOS update package (first line is not '{}')", MAGIC));
    }
    let sig_at = text.rfind("\nsig ").ok_or("header has no signature line")? + 1;
    let signed = &text[..sig_at];
    let sig_line = text[sig_at..].trim_end_matches('\n');
    if sig_line.contains('\n') {
        return Err("signature line is not the last header line".into());
    }
    let sig: [u8; 64] = unhex(&sig_line[4..])
        .and_then(|v| v.try_into().ok())
        .ok_or("signature line is not 64 hex bytes")?;
    let mut m = Manifest {
        target: String::new(),
        build: String::new(),
        kernel: String::new(),
        size: 0,
        sha256: [0; 32],
        key: [0; 8],
        extra: Vec::new(),
    };
    let (mut have_size, mut have_sha, mut have_key) = (false, false, false);
    for l in signed.lines().skip(1) {
        let (k, v) = l.split_once(' ').ok_or_else(|| format!("bad header line '{}'", l))?;
        match k {
            "target" => m.target = v.into(),
            "build" => m.build = v.into(),
            "kernel" => m.kernel = v.into(),
            "size" => {
                m.size = v.parse().map_err(|_| "bad size")?;
                have_size = true
            }
            "sha256" => {
                m.sha256 = unhex(v).and_then(|b| b.try_into().ok()).ok_or("bad sha256")?;
                have_sha = true
            }
            "key" => {
                m.key = unhex(v).and_then(|b| b.try_into().ok()).ok_or("bad key id")?;
                have_key = true
            }
            _ => m.extra.push((k.into(), v.into())),
        }
    }
    if !(have_size && have_sha && have_key) || m.build.is_empty() || m.target.is_empty() {
        return Err("header misses target, build, size, sha256 or key".into());
    }
    let pk = keys
        .iter()
        .find(|pk| key_id(pk) == m.key)
        .ok_or_else(|| format!("signed with an unknown key ({}); this unit trusts {}", hex(&m.key), key_list(keys)))?;
    if !ed25519::verify(pk, signed.as_bytes(), &sig) {
        return Err("SIGNATURE INVALID: the header was changed or not signed with this key".into());
    }
    // the parsed fields must be exactly what was signed (no duplicate or reordered line slipped through)
    if m.signed_text() != signed {
        return Err("header lines are not in the canonical order".into());
    }
    Ok(Header { manifest: m, data_offset: end + 2 })
}

fn key_list(keys: &[[u8; 32]]) -> String {
    if keys.is_empty() {
        return "no key".into();
    }
    keys.iter().map(|k| hex(&key_id(k))).collect::<Vec<_>>().join(", ")
}

/// Full check of a package in memory: signature, target, size and SHA-256 of the image. Returns the header and the image.
pub fn verify<'a>(pkg: &'a [u8], keys: &[[u8; 32]]) -> Result<(Header, &'a [u8]), String> {
    let h = verify_header(pkg, keys)?;
    let m = &h.manifest;
    if m.target != TARGET {
        return Err(format!("package is for '{}', this unit takes '{}'", m.target, TARGET));
    }
    let img = &pkg[h.data_offset..];
    if img.len() as u64 != m.size {
        return Err(format!("image is {} bytes, the header says {} (truncated download?)", img.len(), m.size));
    }
    let mut s = Sha256::new();
    for c in img.chunks(1 << 20) {
        s.update(c);
    }
    if s.finish() != m.sha256 {
        return Err("IMAGE SHA-256 MISMATCH: the package is damaged".into());
    }
    Ok((h, img))
}

/// Public keys from text: one 64-digit hex key per line, `#` starts a comment.
pub fn parse_keys(text: &str) -> Vec<[u8; 32]> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .filter_map(|l| unhex(l.split_whitespace().next()?).and_then(|b| b.try_into().ok()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [42; 32];

    #[test]
    fn round_trip_and_tampering() {
        let img: Vec<u8> = (0..5000u32).map(|i| (i * 13) as u8).collect();
        let pk = ed25519::public_key(&SEED);
        let pkg = build(&SEED, "20261105-170000 kernel 6.12.115+", "6.12.115+", &img).unwrap();
        let (h, data) = verify(&pkg, &[pk]).unwrap();
        assert_eq!(data, &img[..]);
        assert_eq!(h.manifest.build_key(), "20261105-170000");
        assert_eq!(h.manifest.size, 5000);
        assert!(std::str::from_utf8(&pkg[..h.data_offset]).unwrap().starts_with("OOS-UPDATE 1\ntarget dho900-recovery\n"));

        // unknown key
        let other = ed25519::public_key(&[1; 32]);
        assert!(verify(&pkg, &[other]).unwrap_err().contains("unknown key"));
        // changed header byte (build id)
        let mut p2 = pkg.clone();
        let at = p2.windows(8).position(|w| w == b"20261105").unwrap();
        p2[at + 7] = b'9';
        assert!(verify(&p2, &[pk]).unwrap_err().contains("SIGNATURE INVALID"));
        // changed image byte
        let mut p3 = pkg.clone();
        let n = p3.len();
        p3[n - 10] ^= 1;
        assert!(verify(&p3, &[pk]).unwrap_err().contains("SHA-256 MISMATCH"));
        // truncated
        assert!(verify(&pkg[..pkg.len() - 1], &[pk]).unwrap_err().contains("truncated"));
        // a line added after the signature is not accepted as part of the header
        let text_end = h.data_offset - 1;
        let mut p4 = pkg[..text_end].to_vec();
        p4.extend_from_slice(b"target other\n\n");
        p4.extend_from_slice(&img);
        assert!(verify(&p4, &[pk]).is_err());
        // not a package at all
        assert!(verify(b"ANDROID!....", &[pk]).is_err());
    }

    #[test]
    fn extra_lines_are_signed_and_kept() {
        let pk = ed25519::public_key(&SEED);
        let pkg = build_with(&SEED, "20261108-080000", "6.12.115+", b"image", &[("trial", "kexec")]).unwrap();
        let (h, _) = verify(&pkg, &[pk]).unwrap();
        assert_eq!(h.manifest.extra, vec![("trial".to_string(), "kexec".to_string())]);
        let mut p2 = pkg.clone();
        let at = p2.windows(5).position(|w| w == b"kexec").unwrap();
        p2[at] = b'K';
        assert!(verify(&p2, &[pk]).unwrap_err().contains("SIGNATURE INVALID"));
        assert!(build_with(&SEED, "b", "k", b"i", &[("size", "1")]).is_err());
    }

    #[test]
    fn key_file() {
        let pk = ed25519::public_key(&SEED);
        let text = format!("# OOS update keys\n{}  test key 2026-11-05\n\nzz\n", hex(&pk));
        assert_eq!(parse_keys(&text), vec![pk]);
    }
}
