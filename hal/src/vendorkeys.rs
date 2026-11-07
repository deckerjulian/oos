// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//! The keys of `vendor.bin`, found at run time in the stock software of the unit itself: this source holds none. A
//! candidate counts only if the unit's own file decodes with it: checksum for the outer key, a known model name for the
//! inner one. What was found is kept on the data partition of the unit, so later starts need no search.

use crate::model::{self, Keys};

/// Where the stock software is: its application package.
const STOCK_FILES: [&str; 1] = ["/rigol/app/Sparrow.apk"];
const KEEP: &str = "/oos/vendor.keys";

fn word(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
}

fn wide(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|c| u64::from_le_bytes(c.try_into().unwrap()))
}

fn half(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|c| u16::from_le_bytes([c[0], c[1]]))
}

/// One loaded part of an image: its address, place in the file, size, and whether it runs.
#[derive(Clone, Copy, Debug)]
struct Seg {
    addr: u64,
    off: usize,
    size: usize,
}

/// The loadable parts of the 64-bit ELF image at `at` of `b`, file offsets from the start of `b`.
fn segments(b: &[u8], at: usize) -> Option<Vec<Seg>> {
    if b.get(at..at + 5)? != b"\x7fELF\x02" {
        return None;
    }
    let (ph, n, size) = (wide(b, at + 32)? as usize, half(b, at + 56)? as usize, half(b, at + 54)? as usize);
    let mut v = Vec::new();
    for i in 0..n {
        let o = at + ph + i * size;
        if word(b, o)? == 1 {
            v.push(Seg { addr: wide(b, o + 16)?, off: at + wide(b, o + 8)? as usize, size: wide(b, o + 32)? as usize });
        }
    }
    (!v.is_empty()).then_some(v)
}

/// Starts of the ELF images in `b`: the file itself or one stored uncompressed in a package, at any offset.
fn images(b: &[u8]) -> Vec<usize> {
    (0..b.len().saturating_sub(64)).filter(|&o| b[o] == 0x7f && segments(b, o).is_some()).collect()
}

fn key_at(b: &[u8], o: usize) -> Option<[u32; 4]> {
    Some([word(b, o)?, word(b, o + 4)?, word(b, o + 8)?, word(b, o + 12)?])
}

/// The outer key: 16 bytes of data that open the file. The data at the end of an image comes first, that is where it is.
fn file_key(b: &[u8], segs: &[Seg], vendor: &[u8]) -> Option<([u32; 4], Vec<u8>)> {
    for s in segs.iter().rev() {
        for o in (s.off..(s.off + s.size).min(b.len()).saturating_sub(16)).step_by(4) {
            let k = key_at(b, o)?;
            if k == [0; 4] {
                continue;
            }
            if let Some(p) = model::vendor_payload(vendor, &k) {
                return Some((k, p));
            }
        }
    }
    None
}

/// `w` a 32-bit constant in two moves to one register, at `o`: (value, register).
fn constant(b: &[u8], o: usize) -> Option<(u32, u32)> {
    let (a, c) = (word(b, o)?, word(b, o + 4)?);
    let lo_ok = a & 0xffe0_0000 == 0x5280_0000;
    let hi_ok = c & 0xffe0_0000 == 0x72a0_0000 && a & 31 == c & 31;
    (lo_ok && hi_ok).then_some((a >> 5 & 0xffff | (c >> 5 & 0xffff) << 16, a & 31))
}

/// Candidates of the inner key in the code: 8 bytes loaded from data, then two 32-bit constants.
fn item_keys(b: &[u8], segs: &[Seg]) -> Vec<[u32; 4]> {
    let mut found = Vec::new();
    for s in segs {
        found.extend((s.off..(s.off + s.size).min(b.len()).saturating_sub(32)).step_by(4).filter_map(|o| {
            let (page, load) = (word(b, o)?, word(b, o + 4)?);
            // page address to a register, then a 64-bit load through it into a SIMD register
            if page & 0x9f00_0000 != 0x9000_0000 || load & 0xffc0_0000 != 0xfd40_0000 || load >> 5 & 31 != page & 31 {
                return None;
            }
            let (k2, _) = constant(b, o + 8)?;
            let k3 = (1..=3).find_map(|i| constant(b, o + 12 + 4 * i))?.0;
            let pc = s.addr + (o - s.off) as u64;
            let imm = ((page >> 5 & 0x7ffff) << 2 | page >> 29 & 3) as i64;
            let imm = (imm << 43) >> 31; // sign extended, in pages
            let at = (pc & !0xfff).wrapping_add(imm as u64).wrapping_add(((load >> 10 & 0xfff) * 8) as u64);
            let seg = segs.iter().find(|x| (x.addr..x.addr + x.size as u64).contains(&at))?;
            let f = seg.off + (at - seg.addr) as usize;
            Some([word(b, f)?, word(b, f + 4)?, k2, k3])
        }));
    }
    found
}

/// Searches the bytes of a stock file for the keys of `vendor`.
pub fn find(b: &[u8], vendor: &[u8]) -> Option<Keys> {
    let mut all = images(b);
    all.sort_by_key(|&o| std::cmp::Reverse(segments(b, o).map_or(0, |s| s.iter().map(|x| x.size).sum::<usize>())));
    all.iter().find_map(|&at| {
        let segs = segments(b, at)?;
        let (file, payload) = file_key(b, &segs, vendor)?;
        item_keys(b, &segs).into_iter().find(|item| model::item_key_fits(&payload, item)).map(|item| Keys { file, item })
    })
}

fn parse(s: &str) -> Option<Keys> {
    let w: Vec<u32> = s.split_whitespace().filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
    (w.len() == 8).then(|| Keys { file: [w[0], w[1], w[2], w[3]], item: [w[4], w[5], w[6], w[7]] })
}

fn text(k: &Keys) -> String {
    k.file.iter().chain(k.item.iter()).map(|w| format!("{:08x}", w)).collect::<Vec<_>>().join(" ") + "\n"
}

/// The keys for `vendor`: those kept on this unit, else from its stock software; `None` if neither opens the file.
pub fn for_file(vendor: &[u8]) -> Option<Keys> {
    let fits = |k: &Keys| model::vendor_payload(vendor, &k.file).is_some_and(|p| model::item_key_fits(&p, &k.item));
    if let Some(k) = std::fs::read_to_string(KEEP).ok().and_then(|s| parse(&s)).filter(fits) {
        return Some(k);
    }
    let found = std::env::var("OOS_STOCK_FILE")
        .ok()
        .into_iter()
        .chain(STOCK_FILES.iter().map(|s| s.to_string()))
        .find_map(|p| std::fs::read(p).ok().and_then(|b| find(&b, vendor)))?;
    keep(&found);
    Some(found)
}

/// Best effort: without it the next start searches again.
fn keep(k: &Keys) {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;
    let tmp = format!("{}.new", KEEP);
    let done = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).and_then(|mut f| {
        f.write_all(text(k).as_bytes())?;
        f.sync_all()
    });
    if done.is_ok() {
        let _ = std::fs::rename(&tmp, KEEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{encode_vendor_with, TEST_KEYS};

    /// A small ELF image with the keys of the tests at their usual places: code in the first part, data in the second.
    fn image() -> Vec<u8> {
        let k = TEST_KEYS;
        let (code, data, size) = (0x1000usize, 0x2000usize, 0x1000usize);
        let mut b = vec![0u8; data + size];
        b[..5].copy_from_slice(b"\x7fELF\x02");
        b[32..40].copy_from_slice(&64u64.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[56..58].copy_from_slice(&2u16.to_le_bytes());
        for (i, off) in [code, data].iter().enumerate() {
            let o = 64 + i * 56;
            b[o..o + 4].copy_from_slice(&1u32.to_le_bytes());
            b[o + 8..o + 16].copy_from_slice(&(*off as u64).to_le_bytes());
            b[o + 16..o + 24].copy_from_slice(&(*off as u64).to_le_bytes());
            b[o + 32..o + 40].copy_from_slice(&(size as u64).to_le_bytes());
        }
        let put = |b: &mut Vec<u8>, o: usize, w: u32| b[o..o + 4].copy_from_slice(&w.to_le_bytes());
        // the inner key: first half in the data, second half in the code
        let at = data + 0x100;
        put(&mut b, at, k.item[0]);
        put(&mut b, at + 4, k.item[1]);
        let c = code + 0x40;
        let page = 1u32; // the data page, one on from the code page
        put(&mut b, c, 0x9000_0009 | (page & 3) << 29 | (page >> 2) << 5);
        put(&mut b, c + 4, 0xfd40_0000 | (0x100 / 8) << 10 | 9 << 5);
        let mv = |v: u32, r: u32| (0x5280_0000 | (v & 0xffff) << 5 | r, 0x72a0_0000 | (v >> 16) << 5 | r);
        let (a, d) = mv(k.item[2], 9);
        put(&mut b, c + 8, a);
        put(&mut b, c + 12, d);
        put(&mut b, c + 16, 0xb900_0909); // a store between the constants
        let (a, d) = mv(k.item[3], 9);
        put(&mut b, c + 20, a);
        put(&mut b, c + 24, d);
        // the outer key
        for (i, w) in k.file.iter().enumerate() {
            put(&mut b, data + 0x200 + 4 * i, *w);
        }
        b
    }

    #[test]
    fn finds_both_keys_in_an_image() {
        let v = encode_vendor_with(&[(5, "DHO924S"), (7, "DHO9X1")], &TEST_KEYS);
        assert_eq!(find(&image(), &v), Some(TEST_KEYS));
    }

    #[test]
    fn finds_an_image_stored_in_a_package() {
        let v = encode_vendor_with(&[(5, "DHO924S")], &TEST_KEYS);
        let mut b = vec![0x50u8; 0x3000];
        b.extend(image());
        b.extend(vec![0u8; 100]);
        assert_eq!(find(&b, &v), Some(TEST_KEYS));
    }

    #[test]
    fn keys_that_do_not_open_the_file_are_not_taken() {
        let other = Keys { file: [1, 2, 3, 4], item: [5, 6, 7, 8] };
        let v = encode_vendor_with(&[(5, "DHO924S")], &other);
        assert_eq!(find(&image(), &v), None);
        assert_eq!(find(&[0u8; 100], &v), None);
    }

    #[test]
    fn kept_keys_read_back() {
        assert_eq!(parse(&text(&TEST_KEYS)), Some(TEST_KEYS));
        assert_eq!(parse("1 2 3"), None);
    }

    /// The real thing, when the environment names a stock file and the unit's vendor file (not distributed).
    #[test]
    fn stock_file_of_the_environment() {
        let (Ok(lib), Ok(ven)) = (std::env::var("OOS_TEST_STOCK"), std::env::var("OOS_TEST_VENDOR")) else { return };
        let (b, v) = (std::fs::read(lib).unwrap(), std::fs::read(ven).unwrap());
        let t = std::time::Instant::now();
        let k = find(&b, &v).expect("keys in the stock file");
        eprintln!("found in {:?}", t.elapsed());
        assert!(model::decode_vendor_with(&v, &k).unwrap().model.is_some_and(|m| model::lookup(&m).is_some()));
    }
}
