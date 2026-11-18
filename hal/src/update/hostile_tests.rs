// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hostile input for the update (ADR 0047): the package a browser or a stick brings, the boot image in it and on
//! the card, its ramdisk and device tree, the trusted keys file and the state file. Each test states the contract it
//! holds its reader to; the generator and the list of findings that still stand are in `crate::hostile`.

use super::card::{check_image, image_header, IMAGE_MAX};
use super::fdt::{rsce_file, Fdt, Node};
use super::inflate::{cpio_file, gunzip, image_build, inflate};
use super::tests::{bootimg, bootimg_build, bootimg_second, package, pk, SEED};
use super::*;
use crate::hostile::{known, run, KNOWN};

/// `pkg::verify` and `pkg::verify_header` (an update package from the web page or a stick): return for any bytes;
/// a package they take carries one of the signed headers and the image that header names, whatever was changed
/// around them.
#[test]
fn hostile_update_packages() {
    let with_line =
        pkg::build_with(&SEED, "20261105-170000 two", "6.12.115+", &bootimg(2), &[("trial", "kexec")]).unwrap();
    let files = [package(1, "20261105-165012"), with_line];
    let signed: Vec<(pkg::Manifest, Vec<u8>)> =
        files.iter().map(|f| pkg::verify(f, &[pk()]).map(|(h, img)| (h.manifest, img.to_vec())).unwrap()).collect();
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    run("update::pkg::verify", &seeds, 8, |b| {
        if let Ok(h) = pkg::verify_header(b, &[pk()]) {
            if h.data_offset > b.len() || !signed.iter().any(|s| s.0 == h.manifest) {
                return Err(format!("took a header that was never signed: {:?}", h.manifest));
            }
        }
        match pkg::verify(b, &[pk()]) {
            Ok((h, img)) if !signed.iter().any(|s| s.0 == h.manifest && s.1 == img) => {
                Err(format!("took a package of build {:?} that was never signed", h.manifest.build))
            }
            Err(e) if e.is_empty() => Err("refused without a message".into()),
            _ => Ok(()),
        }
    });
}

/// `pkg::parse_keys` (`/etc/oos/trusted-keys`, a file of the image): returns for any text; every key it gives is the
/// first word of a line outside its comment, 64 hex digits, and there are not more keys than such words.
#[test]
fn hostile_trusted_keys() {
    let file = format!("# trusted keys\n{}  # release\n\n{}\nnot a key\n", hash::hex(&pk()), hash::hex(&[0x5a; 32]));
    let seeds: [&[u8]; 1] = [file.as_bytes()];
    run("update::pkg::parse_keys", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let keys = pkg::parse_keys(&text);
        let key_of = |l: &'_ str| l.split('#').next().and_then(|l| l.split_whitespace().next()).map(str::to_string);
        let words: Vec<String> = text.lines().filter_map(key_of).map(|w| w.to_ascii_lowercase()).collect();
        match keys.iter().find(|k| !words.contains(&hash::hex(&k[..]))) {
            Some(k) => Err(format!("a key that is no first word of a line outside its comment: {}", hash::hex(k))),
            None if keys.len() > words.len() => Err("more keys than lines with a word".into()),
            None => Ok(()),
        }
    });
}

// Break it catches: a plus sign taken for a hex digit (#181)
#[test]
fn hostile_unhex_takes_a_plus_sign() {
    assert_eq!(hash::unhex("+f"), None);
    assert!(pkg::parse_keys(&format!("{}+f\n", "00".repeat(31))).is_empty());
}

#[test]
#[ignore = "fdt::rsce_file walks a table of any length: entries of no blocks are read up to 2^32 times, #181"]
pub(crate) fn hostile_known_rsce_table_not_bounded() {
    let mut b = rsce(fdt::TREE, b"tree");
    b[10] = 0; // entries of no blocks: every entry is the first
    b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(rsce_file(&b, fdt::TREE).is_err(), "a table of 2^32 - 1 entries in a container of {} bytes", b.len());
}

/// `card::image_header`, `card::check_image`, `kexec::parts`, `kexec::tree_at` and `inflate::image_build` (the boot
/// image of a package, and the ones read back from the card): return for any bytes; an image they take fits
/// `recovery`, has 2048-byte pages, and its three sections lie inside the bytes one after the other.
#[test]
fn hostile_boot_images() {
    let files = [bootimg(1), bootimg_build(2, "20261105-165012"), bootimg_second(3, vec![0x33; 4000])];
    let seeds: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    run("update::card::check_image", &seeds, 16, |b| {
        if let Ok(i) = image_header(b) {
            if i.len > IMAGE_MAX || i.len < 2048 || i.len % 2048 != 0 {
                return Err(format!("a header for an image of {} bytes", i.len));
            }
            match kexec::tree_at(b) {
                Some((at, _)) if at >= 2048 && at <= i.len => {}
                other => return Err(format!("the device tree section at {other:?} in an image of {} bytes", i.len)),
            }
        }
        if let Ok(i) = check_image(b) {
            let Ok((k, r, s)) = kexec::parts(b) else { return Err("a checked image without its sections".into()) };
            if i.len > b.len() || 2048 + k.len() + r.len() + s.len() > i.len || k.is_empty() {
                return Err(format!("sections of {}, {} and {} bytes in {}", k.len(), r.len(), s.len(), i.len));
            }
        }
        if image_build(b).is_some_and(|build| build.is_empty() || build != build.trim()) {
            return Err("an empty build name".into());
        }
        Ok(())
    });
}

/// `inflate::gunzip`, `inflate::inflate` and `inflate::cpio_file` (the ramdisk of an image): return for any bytes;
/// what is unpacked is at most the limit given; a file found in an archive lies inside it.
#[test]
fn hostile_ramdisks() {
    let cpio = kexec::newc(&[("etc/oos-build", 0o100644, b"20261105-165012\n"), ("./bin/sh", 0o100755, &[7; 300])]);
    let stored = inflate::tests::gzip_stored(&cpio);
    // "hello hello hello hello\n" with fixed codes and a back reference, and with a name and a comment in the header
    let fixed = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0xb8, 0, 0]
        .iter()
        .chain(&[0u8; 8])
        .copied()
        .collect::<Vec<u8>>();
    let named = [&[0x1f, 0x8b, 8, 0x1c, 0, 0, 0, 0, 0, 3, 2, 0, 9, 9][..], b"name\0comment\0", &stored[10..]].concat();
    let seeds: [&[u8]; 4] = [&stored, &fixed, &named, &cpio];
    run("update::inflate::gunzip", &seeds, 4, |b| {
        const LIMIT: usize = 1 << 16;
        if let Ok(out) = gunzip(b, LIMIT) {
            if out.len() > LIMIT {
                return Err(format!("{} bytes out with a limit of {}", out.len(), LIMIT));
            }
        }
        if let Ok((out, used)) = inflate(b, LIMIT) {
            if out.len() > LIMIT || used > b.len() {
                return Err(format!("{} bytes out of {} of {} bytes in", out.len(), used, b.len()));
            }
        }
        for name in ["etc/oos-build", "bin/sh", ""] {
            if let Some(f) = cpio_file(b, name) {
                let inside =
                    b.as_ptr_range().start <= f.as_ptr_range().start && f.as_ptr_range().end <= b.as_ptr_range().end;
                if !inside {
                    return Err(format!("the file {name:?} of {} bytes lies outside the archive", f.len()));
                }
            }
        }
        Ok(())
    });
}

/// A tree as the loader hands one over, as a blob.
fn tree(bootargs: &[u8]) -> Vec<u8> {
    let node = |name: &str, props: &[(&str, &[u8])], kids: Vec<Node>| Node {
        name: name.into(),
        props: props.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect(),
        kids,
    };
    let root = node(
        "",
        &[("compatible", b"rigol,dho900\0"), ("#address-cells", &[0, 0, 0, 2])],
        vec![
            node("chosen", &[("bootargs", bootargs), ("linux,initrd-start", &[0, 0, 0, 0, 0x0a, 0x20, 0, 0])], vec![]),
            node("gpu@ff9a0000", &[("status", b"okay\0"), ("phandle", &[0, 0, 0, 7])], vec![node("port", &[], vec![])]),
            node("memory", &[("reg", &[0; 16])], vec![]),
        ],
    );
    Fdt { rsv: vec![(0x460_0000, 0x17000)], boot_cpu: 0, root }.to_bytes()
}

/// An RSCE container with one file.
fn rsce(name: &str, file: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 1024];
    b[..4].copy_from_slice(b"RSCE");
    (b[9], b[10]) = (1, 1);
    b[12..16].copy_from_slice(&1u32.to_le_bytes());
    b[512 + 4..512 + 4 + name.len()].copy_from_slice(name.as_bytes());
    b[512 + 260..512 + 264].copy_from_slice(&2u32.to_le_bytes());
    b[512 + 264..512 + 268].copy_from_slice(&(file.len() as u32).to_le_bytes());
    b.extend_from_slice(file);
    b
}

/// An RSCE container whose table makes `rsce_file` read the same entry again and again: entries of no blocks, and
/// more of them than the test waits for (the finding `rsce-table-not-bounded`).
fn rsce_reads_one_entry_for_ever(b: &[u8]) -> bool {
    b.len() >= 16 && b[10] == 0 && u32::from_le_bytes([b[12], b[13], b[14], b[15]]) > 4096
}

/// A tree of `depth` nodes, each inside the one before.
fn nested(depth: usize) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [0xd00d_feedu32, 0, 56, 0, 40, 17, 16, 0, 0, 0] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(&[0; 16]);
    b.extend([0, 0, 0, 1, 0, 0, 0, 0].repeat(depth));
    b.extend([0, 0, 0, 2].repeat(depth));
    b.extend_from_slice(&[0, 0, 0, 9]);
    let total = (b.len() as u32).to_be_bytes();
    b[4..8].copy_from_slice(&total);
    b[12..16].copy_from_slice(&total);
    b
}

// Break it catches: a tree nested so deep that writing or dropping it overflows the stack (#181)
#[test]
fn hostile_fdt_nesting_not_bounded() {
    let deepest = Fdt::parse(&nested(fdt::DEPTH_MAX)).expect("a tree of DEPTH_MAX levels is taken");
    assert_eq!(Fdt::parse(&deepest.to_bytes()), Ok(deepest));
    assert!(Fdt::parse(&nested(fdt::DEPTH_MAX + 1)).is_err(), "a tree one level deeper is taken");
    // 30 000 levels ended the process
    assert!(Fdt::parse(&nested(30_000)).is_err(), "a tree of 30 000 nested nodes is taken");
}

/// `Fdt::parse`, `Fdt::to_bytes`, `fdt::rsce_file` and `fdt::with_loader_changes` (the device tree section of an
/// image and the tree the running kernel got): return for any bytes; a tree taken is written as a blob that reads
/// as the same tree; a file found in a container lies inside it.
#[test]
fn hostile_device_trees() {
    let (old, running) = (tree(b"console=ttyS2\0"), tree(b"console=ttyS2 quiet\0"));
    let container = rsce(fdt::TREE, &old);
    let seeds: [&[u8]; 3] = [&old, &running, &container];
    run("update::fdt::parse", &seeds, 4, |b| {
        if let Ok(t) = Fdt::parse(b) {
            match Fdt::parse(&t.to_bytes()) {
                Ok(again) if again == t => {}
                other => return Err(format!("written out it reads as {:?}", other.map(|t| t.root.name))),
            }
        }
        if !(rsce_reads_one_entry_for_ever(b) && known(KNOWN, "rsce-table-not-bounded")) {
            if let Ok(f) = rsce_file(b, fdt::TREE) {
                if f.as_ptr_range().start < b.as_ptr_range().start || f.as_ptr_range().end > b.as_ptr_range().end {
                    return Err(format!("a file of {} bytes outside the container", f.len()));
                }
            }
        }
        for (new, was, now) in [(b, &old[..], &running[..]), (&old[..], b, &running[..]), (&old[..], &old[..], b)] {
            if let Ok((merged, _)) = fdt::with_loader_changes(new, was, now) {
                if Fdt::parse(&merged).is_err() {
                    return Err("the tree with the loader's changes is no tree".into());
                }
            }
        }
        Ok(())
    });
}

/// `State::parse` (the update state file on the data partition), `kexec::flags` and `kexec::segments_from` (the
/// kernel's command line and log): return for any text; a state read is written as text that reads as the same
/// state; an image entry has a length above zero; the segments start at a whole GiB at or above the first.
#[test]
fn hostile_update_state() {
    let entry = |build: &str, tag: u8| Entry { build: build.into(), sha256: [tag; 32], len: 30_689_280 };
    let state = State {
        current: Some(entry("20261105-165012", 1)),
        previous: Some(entry("20261101-120000 older", 2)),
        trial: Some(1),
        persist: true,
        kexec: Some("20261106-090000".into()),
        event: "installed 20261105-165012".into(),
    };
    let log = "[    0.000000] GICv3: CPU0: using allocated LPI pending table @a340000\n\
               [    0.012345] ITS@0x00000000fee20000: allocated 65536 Devices @a380000 (flat, esz 8, psz 64K, shr 0)\n";
    let cmdline = "console=ttyS2 oos.kexec=install,downgrade,force root=/dev/ram0";
    let (full, empty) = (state.text(), State::default().text());
    let seeds: [&[u8]; 4] = [full.as_bytes(), empty.as_bytes(), log.as_bytes(), cmdline.as_bytes()];
    run("update::State::parse", &seeds, 1, |b| {
        let text = String::from_utf8_lossy(b);
        let s = State::parse(&text);
        if [&s.current, &s.previous].iter().any(|e| e.as_ref().is_some_and(|e| e.len == 0)) {
            return Err("an image of no bytes".into());
        }
        let again = State::parse(&s.text());
        if State::parse(&again.text()) != again {
            return Err(format!(
                "written out and read again it differs:\n  first:  {again:?}\n  text:   {:?}",
                again.text()
            ));
        }
        let _ = kexec::flags(&text);
        let from = kexec::segments_from(&text);
        if from < kexec::SEGMENTS_FROM || !from.is_multiple_of(1 << 30) {
            return Err(format!("segments from {from:#x}"));
        }
        Ok(())
    });
}
