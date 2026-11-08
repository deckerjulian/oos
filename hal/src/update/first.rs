// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! First install from the running stock system, without opening the scope (docs/install-usb.md): an update file
//! of the stock system starts `oos-update gel-install`, which puts OOS beside stock on the only card there is.
//!
//! The order keeps stock startable to the end:
//! 1. every check, nothing written;
//! 2. a rescue set on the USB stick: what is about to change, and what the scope cannot start without;
//! 3. `boot` selects stock (only a reinstall changes anything here), the stock copy goes to `backup`, the image to
//!    `recovery`, each read back from the card;
//! 4. last, one sector: `boot` selects OOS.
//!
//! Only `boot` (its magic), `recovery` and the start of `backup` are written. A failure or a power cut before step 4
//! leaves a card that starts stock as before.
//!
//! Run again on a card that already has OOS (to start it once more in one-shot mode), it keeps what `recovery` holds
//! when that is an image the loader takes: a newer build installed over the network since stays. Only `reinstall`
//! (`oos-reinstall.txt` on the stick) writes the stick's build over it. A damaged OOS image in `recovery` (an update
//! cut by a power loss) is replaced by the rollback image when that one is whole, else by the stick's build.

use super::card::{check_image, Card, Region, BOOT_LBA, RECOVERY_LBA, SECTOR};
use std::borrow::Cow;
use super::hash::{hex, Sha256};
use super::pkg;
use std::io::{Read, Write};
use std::path::Path;

/// Models this has run on. Only a card in a host computer helps if the kernel hangs on another one, and that means
/// opening the scope: docs/install-usb.md.
pub const VERIFIED_MODELS: &[&str] = &["DHO924S"];
/// `boot`, and the part of `backup` that holds its stock copy.
pub const BOOT_BYTES: usize = 32 << 20;
/// Everything in front of the `kernel` partition: loader, vendor storage, `parameter`, `uboot`, `trust`, `misc`, `resource`.
const HEAD_SECTORS: u64 = 73728;
/// The rescue set is complete when this file exists.
pub const SUMS: &str = "SHA256SUMS";
/// The serial of the unit a rescue set is from.
pub const UNIT: &str = "unit.txt";

#[derive(Clone, Copy, Debug, Default)]
pub struct FirstOpt {
    pub execute: bool,
    /// a model that OOS never ran on
    pub any_model: bool,
    /// write the stick's build to `recovery` also over a valid OOS image there
    pub reinstall: bool,
}

/// The model of `/rigol/data/vendor.bin`, if it may be installed on.
pub fn check_model(vendor: Option<&[u8]>, any: bool) -> Result<String, String> {
    let name = vendor.and_then(|v| crate::model::decode_vendor(v).ok()).and_then(|v| v.model);
    match name {
        Some(n) if VERIFIED_MODELS.contains(&n.as_str()) => Ok(n),
        Some(n) if any && crate::model::lookup(&n).is_some() => Ok(format!("{} (never run on this model, accepted by request)", n)),
        Some(n) if crate::model::lookup(&n).is_some() => {
            Err(format!("OOS has never run on the {}: a hang would need the card in a host computer. Nothing written", n))
        }
        Some(n) => Err(format!("unknown model {}: nothing written", n)),
        None if any => Ok("unknown (accepted by request)".into()),
        None => Err("cannot read the model from vendor.bin: nothing written".into()),
    }
}

fn kind(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"KRNL") {
        Some("stock recovery")
    } else if head.starts_with(b"ANDROID!") {
        Some("an OOS image")
    } else if head.iter().all(|&b| b == 0) {
        Some("nothing")
    } else {
        None
    }
}

/// One region of the card into a file on the stick, its hash into `sums`.
fn save_region(card: &mut Card, dir: &Path, name: &str, lba: u64, sectors: u64, sums: &mut String) -> Result<(), String> {
    let path = dir.join(name);
    let e = |x: std::io::Error| format!("{}: {}", path.display(), x);
    let mut f = std::fs::File::create(&path).map_err(e)?;
    let mut s = Sha256::new();
    let step = 8192u64;
    let mut at = 0;
    while at < sectors {
        let n = step.min(sectors - at);
        let b = card.read(lba + at, (n * SECTOR) as usize).map_err(|x| format!("reading the card: {}", x))?;
        s.update(&b);
        f.write_all(&b).map_err(e)?;
        at += n;
    }
    f.sync_all().map_err(e)?;
    drop(f);
    let want = s.finish();
    // and what the stick gives back
    let mut f = std::fs::File::open(&path).map_err(e)?;
    let mut s = Sha256::new();
    let mut b = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut b).map_err(e)?;
        if n == 0 {
            break;
        }
        s.update(&b[..n]);
    }
    if s.finish() != want {
        return Err(format!("{}: the stick does not give back what was saved", path.display()));
    }
    sums.push_str(&format!("{}  {}\n", hex(&want), name));
    Ok(())
}

/// The rescue set. A complete set is kept: after the first install `recovery` holds OOS, and the stock one is only here.
///
/// A set from another scope is no rescue for this one, and the install overwrites `recovery`: it stops there.
fn save_set(card: &mut Card, dir: &Path, serial: Option<&str>, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if dir.join(SUMS).exists() {
        let unit = std::fs::read_to_string(dir.join(UNIT)).unwrap_or_default();
        if let Some(s) = serial.filter(|s| *s != unit.trim()) {
            return Err(format!("the rescue set on this stick is from unit '{}', this is {}: use another stick. Nothing written", unit.trim(), s));
        }
        log(&format!("rescue set in {} kept", dir.display()));
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let mut sums = String::new();
    save_region(card, dir, "card-head.bin", 0, HEAD_SECTORS, &mut sums)?;
    save_region(card, dir, "boot.bin", BOOT_LBA, BOOT_BYTES as u64 / SECTOR, &mut sums)?;
    save_region(card, dir, "recovery.bin", RECOVERY_LBA, Region::Recovery.max() as u64 / SECTOR, &mut sums)?;
    if let Some(s) = serial {
        std::fs::write(dir.join(UNIT), s).map_err(|e| format!("{}: {}", UNIT, e))?;
    }
    let p = dir.join(SUMS);
    let e = |x: std::io::Error| format!("{}: {}", p.display(), x);
    let mut f = std::fs::File::create(&p).map_err(e)?;
    f.write_all(sums.as_bytes()).map_err(e)?;
    f.sync_all().map_err(e)?;
    log(&format!("rescue set saved to {}", dir.display()));
    Ok(())
}

/// See the module text. `save` is the rescue directory on the stick; `opt.execute` false stops before the card is written.
pub fn first_install(
    card: &mut Card,
    package: &[u8],
    keys: &[[u8; 32]],
    save: &Path,
    vendor: Option<&[u8]>,
    opt: &FirstOpt,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    log(&format!("model {}", check_model(vendor, opt.any_model)?));
    let (h, img) = pkg::verify(package, keys)?;
    check_image(img)?;
    log(&format!("package ok: build {}", h.manifest.build));
    card.check_layout()?;
    log("partition table: stock layout");

    let io = |e: std::io::Error| format!("reading the card: {}", e);
    let magic = card.boot_magic().map_err(io)?;
    if &magic != b"KRNL" && &magic != b"OOSK" {
        return Err(format!("boot magic is '{}', neither KRNL nor OOSK: nothing written", String::from_utf8_lossy(&magic)));
    }
    log(&format!("selected now: {}", if &magic == b"KRNL" { "stock" } else { "OOS" }));

    // the fallback: `boot` with the stock magic, whatever is selected now
    let mut copy = card.read(BOOT_LBA, BOOT_BYTES).map_err(io)?;
    copy[..4].copy_from_slice(b"KRNL");
    let head = card.read(Region::Fallback.lba(), BOOT_BYTES).map_err(io)?;
    // empty, or the first part of the copy from an interrupted install, or the copy with a test magic in front
    let same_to = head.iter().zip(&copy).take_while(|(a, b)| a == b).count();
    let partial = head[same_to..].iter().all(|&b| b == 0);
    let need_copy = if head == copy {
        log("backup already holds the stock copy");
        false
    } else if partial || (head.starts_with(b"BAKK") && head[4..] == copy[4..]) {
        log("backup: the stock copy goes here");
        true
    } else {
        return Err("`backup` holds something else than a stock copy: nothing written".into());
    };

    let current = card.read(RECOVERY_LBA, 4096).map_err(io)?;
    let what = kind(&current).ok_or("`recovery` holds something unknown: nothing written")?;
    log(&format!("recovery holds {}", what));
    let (on_card, _) = card.read_image(Region::Recovery, img.len()).map_err(io)?;
    // what goes to recovery: None keeps what is there
    let put: Option<(Cow<[u8]>, &str)> = if on_card == img {
        log("recovery already holds this image");
        None
    } else if what == "an OOS image" {
        let valid = card.header(Region::Recovery).map_err(io)?.map(|h| card.read_image(Region::Recovery, h.len)).transpose().map_err(io)?;
        match valid.as_ref().map(|(b, _)| check_image(b)) {
            Some(Ok(i)) if !opt.reinstall => {
                log(&format!(
                    "recovery holds a valid OOS image ('{}', {} bytes), maybe a newer build installed over the network: kept, only selected (oos-reinstall.txt on the stick writes the stick's build)",
                    i.name, i.len
                ));
                None
            }
            Some(Ok(_)) => Some((Cow::Borrowed(img), "recovery: the stick's build goes here (oos-reinstall.txt)")),
            _ => match card.header(Region::Slot).map_err(io)? {
                Some(h) => {
                    let (b, _) = card.read_image(Region::Slot, h.len).map_err(io)?;
                    match check_image(&b) {
                        Ok(_) if !opt.reinstall => Some((Cow::Owned(b), "recovery holds a damaged OOS image: the rollback image goes back there")),
                        _ => Some((Cow::Borrowed(img), "recovery holds a damaged OOS image: the stick's build goes here")),
                    }
                }
                None => Some((Cow::Borrowed(img), "recovery holds a damaged OOS image: the stick's build goes here")),
            },
        }
    } else {
        Some((Cow::Borrowed(img), "recovery: the image goes here"))
    };
    if let Some((_, why)) = &put {
        log(why);
    }

    let serial = vendor.and_then(|v| crate::model::decode_vendor(v).ok()).and_then(|v| v.serial);
    save_set(card, save, serial.as_deref(), log)?;

    if !opt.execute {
        log("dry run: the card was not written");
        return Ok(());
    }
    // a power cut in the write must start stock, not the half-written image
    if put.is_some() {
        card.disarm_oos()?;
    }
    if need_copy {
        card.write_image(Region::Fallback, &copy)?;
        log("backup: stock copy written and read back");
    }
    if let Some((b, _)) = &put {
        card.write_image(Region::Recovery, b)?;
        log("recovery: image written and read back");
    }
    card.arm_oos()?;
    log("OOS selected for the next power-on");
    Ok(())
}
