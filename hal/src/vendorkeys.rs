// Copyright (C) 2026 dekajoule
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
    b.get(o..)?.first_chunk().copied().map(u64::from_le_bytes)
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
        let o = at.checked_add(ph)?.checked_add(i.checked_mul(size)?)?;
        if word(b, o)? == 1 {
            // a part that does not lie inside the bytes at hand is cut to them: anything else is no image of ours
            let off = at.checked_add(usize::try_from(wide(b, o + 8)?).ok()?)?;
            let size = usize::try_from(wide(b, o + 32)?).ok()?.min(b.len().saturating_sub(off));
            if size > 0 {
                v.push(Seg { addr: wide(b, o + 16)?, off, size });
            }
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

/// The outer key: 16 bytes of data that open the file. The last part of an image is searched first, that is where it is.
fn file_key(b: &[u8], segs: &[Seg], vendor: &[u8]) -> Option<([u32; 4], Vec<u8>)> {
    for s in segs.iter().rev() {
        for o in (s.off..(s.off + s.size).saturating_sub(16)).step_by(4) {
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
        found.extend((s.off..(s.off + s.size).saturating_sub(32)).step_by(4).filter_map(|o| {
            let (page, load) = (word(b, o)?, word(b, o + 4)?);
            // page address to a register, then a 64-bit load through it into a SIMD register
            if page & 0x9f00_0000 != 0x9000_0000 || load & 0xffc0_0000 != 0xfd40_0000 || load >> 5 & 31 != page & 31 {
                return None;
            }
            let (k2, _) = constant(b, o + 8)?;
            let k3 = (1..=3).find_map(|i| constant(b, o + 12 + 4 * i))?.0;
            let pc = s.addr.wrapping_add((o - s.off) as u64);
            let imm = ((page >> 5 & 0x7ffff) << 2 | page >> 29 & 3) as i64;
            let imm = (imm << 43) >> 31; // sign extended, in pages
            let at = (pc & !0xfff).wrapping_add(imm as u64).wrapping_add(((load >> 10 & 0xfff) * 8) as u64);
            let seg = segs.iter().find(|x| (x.addr..x.addr.saturating_add(x.size as u64)).contains(&at))?;
            let f = seg.off.checked_add(usize::try_from(at - seg.addr).ok()?)?;
            Some([word(b, f)?, word(b, f + 4)?, k2, k3])
        }));
    }
    found
}

/// What a search for the keys went through, for the warning when it found nothing.
#[derive(Debug, Default)]
struct Report {
    /// stock files read
    files: usize,
    /// programs (ELF images) looked at, of them unpacked from a package
    images: usize,
    packed: usize,
    /// programs with the code pattern of the inner key, and with an outer key that opens the file
    pattern: usize,
    outer: usize,
}

impl Report {
    fn text(&self) -> String {
        format!(
            "{} stock files read, {} programs ({} unpacked), inner-key pattern in {}, outer key in {}",
            self.files, self.images, self.packed, self.pattern, self.outer
        )
    }
}

/// Searches the bytes of a stock file for the keys of `vendor`.
pub fn find(b: &[u8], vendor: &[u8]) -> Option<Keys> {
    search(b, vendor, &mut Report::default())
}

fn search(b: &[u8], vendor: &[u8], r: &mut Report) -> Option<Keys> {
    let mut all = images(b);
    r.images += all.len();
    all.sort_by_key(|&o| std::cmp::Reverse(segments(b, o).map_or(0, |s| s.iter().map(|x| x.size).sum::<usize>())));
    all.iter().find_map(|&at| {
        let segs = segments(b, at)?;
        // the code pattern is cheap to look for, the outer key is not: an image without the pattern is passed over
        let items = item_keys(b, &segs);
        if items.is_empty() {
            return None;
        }
        r.pattern += 1;
        let (file, payload) = file_key(b, &segs, vendor)?;
        r.outer += 1;
        items.into_iter().find(|item| model::item_key_fits(&payload, item)).map(|item| Keys { file, item })
    })
}

/// At most this much of one packed library is unpacked.
const PACKED_MAX: usize = 256 << 20;

/// The shared libraries a package (zip) keeps compressed with deflate: a package that its system unpacks at install
/// time stores its native code that way, where `images` cannot see it. Zip local headers: signature `PK 3 4`, method
/// at 8, name length at 26, extra length at 28, name at 30, then the data.
fn packed_libs(b: &[u8]) -> Vec<Vec<u8>> {
    let mut libs = Vec::new();
    let mut o = 0;
    while let Some(p) = b.get(o..).and_then(|t| t.windows(4).position(|w| w == b"PK\x03\x04")) {
        let at = o + p;
        o = at + 4;
        let (Some(method), Some(n), Some(x)) = (half(b, at + 8), half(b, at + 26), half(b, at + 28)) else { break };
        let data = at + 30 + n as usize + x as usize;
        let Some(name) = b.get(at + 30..at + 30 + n as usize) else { break };
        if method == 8 && name.ends_with(b".so") {
            if let Some(Ok((lib, used))) = b.get(data..).map(|d| crate::update::inflate::inflate(d, PACKED_MAX)) {
                libs.push(lib);
                o = data + used;
            }
        }
    }
    libs
}

/// Searches a stock file: the programs stored in it as they are, then those packed in it.
fn search_file(b: &[u8], vendor: &[u8], r: &mut Report) -> Option<Keys> {
    r.files += 1;
    search(b, vendor, r).or_else(|| {
        packed_libs(b).iter().find_map(|lib| {
            r.packed += 1;
            search(lib, vendor, r)
        })
    })
}

fn parse(s: &str) -> Option<Keys> {
    let w: Vec<u32> = s.split_whitespace().filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
    (w.len() == 8).then(|| Keys { file: [w[0], w[1], w[2], w[3]], item: [w[4], w[5], w[6], w[7]] })
}

fn text(k: &Keys) -> String {
    k.file.iter().chain(k.item.iter()).map(|w| format!("{:08x}", w)).collect::<Vec<_>>().join(" ") + "\n"
}

fn stock_files() -> Vec<String> {
    std::env::var("OOS_STOCK_FILE").ok().into_iter().chain(STOCK_FILES.iter().map(|s| s.to_string())).collect()
}

/// The way of searching: a new one searches again where an older one found nothing (2: also the packed libraries).
const SEARCH: u32 = 2;

/// What a search depends on: the way of searching, the vendor file and the size and time of each stock file. A search
/// that found nothing is kept with it, so that it runs again only when one of them changed.
fn inputs(vendor: &[u8]) -> String {
    let mut s = format!("s{} {:08x}", SEARCH, crate::crc32(vendor));
    for p in stock_files() {
        if let Ok(m) = std::fs::metadata(&p) {
            let t =
                m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            s.push_str(&format!(" {}:{}", m.len(), t));
        }
    }
    s
}

/// The keys for `vendor`: those kept on this unit, else from its stock software; what was searched if neither opens
/// the file.
pub fn for_file(vendor: &[u8]) -> Result<Keys, String> {
    let fits = |k: &Keys| model::vendor_payload(vendor, &k.file).is_some_and(|p| model::item_key_fits(&p, &k.item));
    let kept = std::fs::read_to_string(KEEP).unwrap_or_default();
    let (first, why) = kept.split_once('\n').unwrap_or((kept.as_str(), ""));
    if let Some(k) = parse(first).filter(fits) {
        return Ok(k);
    }
    let sig = inputs(vendor);
    if first.trim() == format!("none {}", sig) {
        return Err(why.trim().trim_start_matches("# ").to_string());
    }
    let mut r = Report::default();
    let found = stock_files().iter().find_map(|p| std::fs::read(p).ok().and_then(|b| search_file(&b, vendor, &mut r)));
    match found {
        Some(k) => {
            keep(&text(&k));
            Ok(k)
        }
        None => {
            keep(&format!("none {}\n# {}\n", sig, r.text()));
            Err(r.text())
        }
    }
}

/// Best effort: without it the next start searches again.
fn keep(line: &str) {
    use crate::fsutil::{write_atomic, DirSync, Opts};
    let _ = write_atomic(
        KEEP.as_ref(),
        line.as_bytes(),
        &Opts { mode: Some(0o600), dir_sync: DirSync::Skip, ..Default::default() },
    );
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
    fn finds_an_image_packed_in_a_package() {
        let v = encode_vendor_with(&[(5, "DHO924S")], &TEST_KEYS);
        let img = image();
        // deflate in two stored blocks, the first cut inside the ELF signature: the image is not there as it is
        let mut d = Vec::new();
        for (last, c) in [(0u8, &img[..2]), (1, &img[2..])] {
            d.push(last);
            d.extend((c.len() as u16).to_le_bytes());
            d.extend((!(c.len() as u16)).to_le_bytes());
            d.extend(c);
        }
        let name = b"lib/arm64-v8a/libstock.so";
        let mut zip = vec![0x50u8; 64];
        zip.extend(b"PK\x03\x04");
        zip.extend([20, 0, 0, 0, 8, 0]);
        zip.extend([0u8; 16]);
        zip.extend((name.len() as u16).to_le_bytes());
        zip.extend([0, 0]);
        zip.extend(name);
        zip.extend(d);
        assert_eq!(find(&zip, &v), None);
        let mut r = Report::default();
        assert_eq!(search_file(&zip, &v, &mut r), Some(TEST_KEYS));
        assert_eq!((r.files, r.packed, r.pattern, r.outer), (1, 1, 1, 1));
    }

    #[test]
    fn keys_that_do_not_open_the_file_are_not_taken() {
        let other = Keys { file: [1, 2, 3, 4], item: [5, 6, 7, 8] };
        let v = encode_vendor_with(&[(5, "DHO924S")], &other);
        assert_eq!(find(&image(), &v), None);
        assert_eq!(find(&[0u8; 100], &v), None);
    }

    #[test]
    fn bogus_headers_are_passed_over() {
        let mut b = image();
        // a second "image" whose part claims an offset and size far beyond the bytes
        let at = b.len();
        b.extend(vec![0u8; 256]);
        b[at..at + 5].copy_from_slice(b"\x7fELF\x02");
        b[at + 32..at + 40].copy_from_slice(&64u64.to_le_bytes());
        b[at + 54..at + 56].copy_from_slice(&56u16.to_le_bytes());
        b[at + 56..at + 58].copy_from_slice(&1u16.to_le_bytes());
        b[at + 64..at + 68].copy_from_slice(&1u32.to_le_bytes());
        b[at + 72..at + 80].copy_from_slice(&u64::MAX.to_le_bytes());
        b[at + 96..at + 104].copy_from_slice(&u64::MAX.to_le_bytes());
        let v = encode_vendor_with(&[(5, "DHO924S")], &TEST_KEYS);
        assert_eq!(find(&b, &v), Some(TEST_KEYS));
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
