// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

use super::card::*;
use super::*;
use std::path::PathBuf;

const SEED: [u8; 32] = [9; 32];

fn pk() -> [u8; 32] {
    ed25519::public_key(&SEED)
}

/// A boot image as boot/oos-bootimg.py builds it (tiny sections, distinct content per `tag`).
fn bootimg(tag: u8) -> Vec<u8> {
    bootimg_dt(tag, tag ^ 0xaa)
}

/// `bootimg` with the device tree section (`second`) filled with `dt`.
fn bootimg_dt(tag: u8, dt: u8) -> Vec<u8> {
    let page = 2048usize;
    let mut kernel = vec![tag; 4096 + 512];
    kernel[0x38..0x3c].copy_from_slice(b"ARM\x64");
    let ramdisk = vec![tag ^ 0x55; 1000];
    let second = vec![dt; 300];
    let mut h = vec![0u8; page];
    h[..8].copy_from_slice(b"ANDROID!");
    let put = |h: &mut Vec<u8>, at: usize, v: u32| h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut h, 8, kernel.len() as u32);
    put(&mut h, 12, 0x0028_0000);
    put(&mut h, 16, ramdisk.len() as u32);
    put(&mut h, 20, 0x04bf_0000);
    put(&mut h, 24, second.len() as u32);
    put(&mut h, 36, page as u32);
    h[48..58].copy_from_slice(b"oos-dho900");
    let id = hash::sha1(&[
        &kernel,
        &(kernel.len() as u32).to_le_bytes(),
        &ramdisk,
        &(ramdisk.len() as u32).to_le_bytes(),
        &second,
        &(second.len() as u32).to_le_bytes(),
        &h[32..576],
    ]);
    h[576..596].copy_from_slice(&id);
    let mut out = h;
    for s in [&kernel, &ramdisk, &second] {
        out.extend_from_slice(s);
        out.resize(out.len().div_ceil(page) * page, 0);
    }
    out
}

fn package(tag: u8, build: &str) -> Vec<u8> {
    pkg::build(&SEED, build, "6.12.115+", &bootimg(tag)).unwrap()
}

struct Fake {
    dir: PathBuf,
}

impl Fake {
    /// A sparse file laid out like the card: stock PARM text, `KRNL` in boot, `recovery` holding image `tag`
    /// (None: empty).
    fn new(name: &str, recovery: Option<u8>) -> Fake {
        let dir = std::env::temp_dir().join(format!("oos-update-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = std::fs::File::create(dir.join("card")).unwrap();
        f.set_len((OOSDATA_LBA + 0x8000) * SECTOR).unwrap();
        drop(f);
        let fk = Fake { dir };
        let parm = b"PARM\0\0\0\0FIRMWARE_VER:9.0\nCMDLINE:mtdparts=rk29xxnand:0x00002000@0x00002000(uboot),0x00002000@0x00004000(trust),0x00002000@0x00006000(misc),0x00008000@0x00008000(resource),0x0000C000@0x00010000(kernel),0x00010000@0x0001C000(boot),0x00020000@0x0002C000(recovery),0x00038000@0x0004C000(backup),-@0x004CE440(userdata:grow)\n";
        fk.poke(PARM_LBA, parm);
        fk.poke(BOOT_LBA, b"KRNL\x00\x10\x00\x00");
        if let Some(t) = recovery {
            fk.poke(RECOVERY_LBA, &bootimg(t));
        }
        fk
    }
    fn card(&self) -> Card {
        Card::open(&self.dir.join("card"), true).unwrap()
    }
    fn state(&self) -> PathBuf {
        self.dir.join("oos/update/state")
    }
    fn poke(&self, lba: u64, b: &[u8]) {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new().write(true).open(self.dir.join("card")).unwrap();
        f.seek(SeekFrom::Start(lba * SECTOR)).unwrap();
        f.write_all(b).unwrap();
    }
    fn region(&self, r: Region) -> Vec<u8> {
        let mut c = self.card();
        let n = c.header(r).unwrap().map_or(2048, |h| h.len);
        c.read_image(r, n).unwrap().0
    }
    fn magic(&self) -> [u8; 4] {
        self.card().boot_magic().unwrap()
    }
    fn st(&self) -> State {
        State::load(&self.state()).unwrap()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const RUN: InstallOpt = InstallOpt { execute: true, downgrade: false, force: false };

#[test]
fn image_check_matches_the_python_packer() {
    let img = bootimg(1);
    let info = check_image(&img).unwrap();
    assert_eq!(info.len, img.len());
    assert_eq!(info.name, "oos-dho900");
    let mut bad = img.clone();
    bad[3000] ^= 1;
    assert!(check_image(&bad).unwrap_err().contains("id mismatch"));
    let mut bad = img.clone();
    bad[37] = 0; // page size 2048 -> 0
    assert!(check_image(&bad).is_err());
}

#[test]
fn first_install_keeps_the_hand_installed_image() {
    let f = Fake::new("first", Some(1));
    let r = install(&mut f.card(), &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).unwrap();
    assert!(r.written, "{:?}", r.lines);
    assert_eq!(f.region(Region::Recovery), bootimg(2));
    assert_eq!(f.region(Region::Slot), bootimg(1));
    assert_eq!(&f.magic(), b"OOSK");
    let st = f.st();
    assert_eq!(st.trial, Some(0));
    assert_eq!(st.current.as_ref().unwrap().build, "20261105-170000 kernel x");
    assert!(st.previous.as_ref().unwrap().build.starts_with("untracked-"));
    // the stock copy at the start of backup and /oos after the slot stay untouched
    let mut c = f.card();
    assert!(c.read(BACKUP_LBA, 4096).unwrap().iter().all(|&b| b == 0));
    assert!(c.read(OOSDATA_LBA, 4096).unwrap().iter().all(|&b| b == 0));
}

#[test]
fn dry_run_writes_nothing() {
    let f = Fake::new("dry", Some(1));
    let opt = InstallOpt { execute: false, ..RUN };
    let r = install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &opt).unwrap();
    assert!(!r.written);
    assert!(r.lines.iter().any(|l| l.contains("dry run")));
    assert_eq!(f.region(Region::Recovery), bootimg(1));
    assert_eq!(&f.magic(), b"KRNL");
    assert!(!f.state().exists());
}

#[test]
fn trial_confirm_and_next_install() {
    let f = Fake::new("confirm", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    assert_eq!(boot_started(&f.state(), Some("20261105-170000")).unwrap(), Boot::Trial(1)); // first start of the trial image
    assert_eq!(f.st().trial, Some(1));
    assert!(confirm(&f.state(), None).unwrap());
    assert_eq!(f.st().trial, None);
    assert!(!confirm(&f.state(), None).unwrap());
    // the next update keeps the confirmed build 2 as the rollback image
    install(&mut f.card(), &f.state(), &package(3, "20261106-090000"), &[pk()], &RUN).unwrap();
    assert_eq!(f.region(Region::Slot), bootimg(2));
    assert_eq!(f.region(Region::Recovery), bootimg(3));
    assert_eq!(f.st().previous.unwrap().build, "20261105-170000");
}

#[test]
fn unconfirmed_trial_rolls_back_at_its_second_start() {
    let f = Fake::new("second", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    assert_eq!(boot_started(&f.state(), Some("20261105-170000")).unwrap(), Boot::Trial(1));
    // no confirmation, power cycle, OOS again
    assert_eq!(boot_started(&f.state(), Some("20261105-170000")).unwrap(), Boot::Rollback);
    let r = rollback(&mut f.card(), &f.state(), "not confirmed", true, None).unwrap();
    assert!(r.written);
    assert_eq!(f.region(Region::Recovery), bootimg(1));
    let st = f.st();
    assert_eq!(st.trial, None);
    assert!(st.event.contains("rolled back from 20261105-170000"), "{}", st.event);
    // a second rollback has nothing to do
    assert!(!rollback(&mut f.card(), &f.state(), "again", true, None).unwrap().written);
    // and the next install does not write the slot again (it already holds that image)
    let r = install(&mut f.card(), &f.state(), &package(4, "20261107-090000"), &[pk()], &RUN).unwrap();
    assert!(r.lines.iter().any(|l| l.contains("already holds")), "{:?}", r.lines);
    assert_eq!(f.region(Region::Slot), bootimg(1));
}

#[test]
fn install_over_a_trial_image_keeps_the_last_good_one() {
    let f = Fake::new("overtrial", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    // build 2 never confirmed; build 3 comes before
    install(&mut f.card(), &f.state(), &package(3, "20261105-180000"), &[pk()], &RUN).unwrap();
    assert_eq!(f.region(Region::Slot), bootimg(1));
    assert_eq!(f.region(Region::Recovery), bootimg(3));
}

#[test]
fn refusals_write_nothing() {
    let f = Fake::new("refuse", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    confirm(&f.state(), None).unwrap();
    let before = (f.region(Region::Recovery), f.region(Region::Slot), f.st());

    // older build
    let e = install(&mut f.card(), &f.state(), &package(5, "20261104-000000"), &[pk()], &RUN).unwrap_err();
    assert!(e.contains("older"), "{}", e);
    // unknown key
    let other = pkg::build(&[3; 32], "20261106-000000", "k", &bootimg(5)).unwrap();
    assert!(install(&mut f.card(), &f.state(), &other, &[pk()], &RUN).unwrap_err().contains("unknown key"));
    // a signed package with an image the loader would refuse
    let mut img = bootimg(5);
    img[5000] ^= 1;
    let p = pkg::build(&SEED, "20261106-000000", "k", &img).unwrap();
    assert!(install(&mut f.card(), &f.state(), &p, &[pk()], &RUN).unwrap_err().contains("id mismatch"));
    // the same image again
    let r = install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    assert!(!r.written);

    assert_eq!((f.region(Region::Recovery), f.region(Region::Slot), f.st()), before);

    // downgrade on request
    let opt = InstallOpt { downgrade: true, ..RUN };
    assert!(install(&mut f.card(), &f.state(), &package(5, "20261104-000000"), &[pk()], &opt).unwrap().written);
}

#[test]
fn foreign_slot_data_and_layout_are_refused() {
    let f = Fake::new("foreign", Some(1));
    f.poke(SLOT_LBA + 1000, b"something");
    let e = install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap_err();
    assert!(e.contains("not an OOS image"), "{}", e);
    assert_eq!(f.region(Region::Recovery), bootimg(1));

    let g = Fake::new("layout", Some(1));
    g.poke(PARM_LBA, &[b'x'; 1024]);
    let e = install(&mut g.card(), &g.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap_err();
    assert!(e.contains("partition table"), "{}", e);
}

#[test]
fn rollback_checks_the_slot_first() {
    let f = Fake::new("slotcheck", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    f.poke(SLOT_LBA + 8, b"\x77");
    let e = rollback(&mut f.card(), &f.state(), "test", true, None).unwrap_err();
    assert!(e.contains("does not hold"), "{}", e);
    assert_eq!(f.region(Region::Recovery), bootimg(2));
}

#[test]
fn state_text_round_trip() {
    let s = State {
        current: Some(Entry { build: "20261105-170000 kernel 6.12.115+".into(), sha256: [1; 32], len: 30689280 }),
        previous: Some(Entry { build: "untracked-0123456789ab".into(), sha256: [2; 32], len: 1000 }),
        trial: Some(1),
        persist: true,
        kexec: Some("20261108-090000".into()),
        event: "installed 20261105-170000".into(),
    };
    assert_eq!(State::parse(&s.text()), s);
    assert_eq!(State::parse(""), State::default());
}

#[test]
fn trial_rule() {
    let h = |elapsed, ui_restarts, ui_up, startup_done| Health { elapsed, ui_restarts, ui_up, startup_done };
    assert_eq!(judge(&h(10.0, 0, 10.0, false)), Verdict::Wait);
    assert_eq!(judge(&h(30.0, 0, 25.0, false)), Verdict::Wait); // cold boot still running
    assert_eq!(judge(&h(40.0, 0, 21.0, true)), Verdict::Good);
    assert_eq!(judge(&h(40.0, 1, 21.0, true)), Verdict::Good); // one restart, then stable
    assert!(matches!(judge(&h(15.0, 3, 1.0, false)), Verdict::Bad(_)));
    // a hanging cold boot with a stable oos-ui is hardware trouble, not a bad build
    assert_eq!(judge(&h(180.0, 0, 170.0, false)), Verdict::Good);
    assert!(matches!(judge(&h(180.0, 2, 5.0, true)), Verdict::Bad(_)));
    assert!(matches!(judge(&h(180.0, 0, 0.0, false)), Verdict::Bad(_)));
}

#[test]
fn trial_follows_the_running_image() {
    let f = Fake::new("running", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    // recovery rewritten by hand (dd) and that image started: no count, no rollback, trial dropped
    match boot_started(&f.state(), Some("20261105-190000 kernel x")).unwrap() {
        Boot::Dropped(m) => assert!(m.contains("not the one on trial"), "{}", m),
        b => panic!("{:?}", b),
    }
    let st = f.st();
    assert_eq!((st.trial, st.current), (None, None));
    assert_eq!(f.region(Region::Recovery), bootimg(2));
}

#[test]
fn a_verdict_about_an_older_state_is_not_applied() {
    let f = Fake::new("expect", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    let seen = f.st().current.unwrap().sha256;
    confirm(&f.state(), Some(seen)).unwrap();
    // the watcher of this start still runs when build 3 is installed from the web page
    install(&mut f.card(), &f.state(), &package(3, "20261105-180000"), &[pk()], &RUN).unwrap();
    assert!(!confirm(&f.state(), Some(seen)).unwrap(), "build 3 confirmed without having run");
    assert_eq!(f.st().trial, Some(0));
    assert!(rollback(&mut f.card(), &f.state(), "late verdict", true, Some(seen)).unwrap_err().contains("changed"));
    assert_eq!(f.region(Region::Recovery), bootimg(3));
}

#[test]
fn force_with_the_same_image_keeps_the_slot() {
    let f = Fake::new("force", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    confirm(&f.state(), None).unwrap();
    let opt = InstallOpt { force: true, ..RUN };
    assert!(install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &opt).unwrap().written);
    assert_eq!(f.region(Region::Slot), bootimg(1));
    assert!(f.st().previous.unwrap().build.starts_with("untracked"));
}

#[test]
fn mode_and_lock() {
    let f = Fake::new("mode", None);
    set_mode(&f.state(), true).unwrap();
    assert!(f.st().persist);
    let l = lock(&f.state()).unwrap();
    // a second lock from another thread waits until the first is dropped
    let p = f.state();
    let t = std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        set_mode(&p, false).unwrap();
        t0.elapsed()
    });
    std::thread::sleep(std::time::Duration::from_millis(200));
    drop(l);
    assert!(t.join().unwrap() >= std::time::Duration::from_millis(150));
    assert!(!f.st().persist);
}

#[test]
fn trusted_keys_file_parses() {
    // every line that is not a comment must be a key (a typo would silently drop it)
    let lines = TRUSTED_KEYS.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).count();
    assert_eq!(trusted_keys().len(), lines);
}

/// Review of 2026-11-05: a power cut while `recovery` is written must leave a card that boots.
#[test]
fn cut_install_with_oos_armed_boots_stock() {
    let f = Fake::new("cut-armed", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).unwrap();
    confirm(&f.state(), None).unwrap();
    // persist mode: OOSK stays set; a second install is cut in the recovery write
    assert_eq!(&f.magic(), b"OOSK");
    let mut c = f.card();
    c.cut = Some(Region::Recovery);
    assert!(install(&mut c, &f.state(), &package(3, "20261105-180000 kernel x"), &[pk()], &RUN).is_err());
    assert_eq!(&f.magic(), b"KRNL", "the next power-on starts stock, not the half-written recovery");
    // the rollback slot still holds the last good build, and a retry finishes the job and arms OOS again
    assert_eq!(f.region(Region::Slot), bootimg(2));
    let r = install(&mut f.card(), &f.state(), &package(3, "20261105-180000 kernel x"), &[pk()], &RUN).unwrap();
    assert!(r.written);
    assert_eq!(f.region(Region::Recovery), bootimg(3));
    assert_eq!(&f.magic(), b"OOSK");
}

#[test]
fn cut_rollback_can_be_repeated() {
    let f = Fake::new("cut-rollback", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).unwrap();
    assert_eq!(&f.magic(), b"OOSK");
    let mut c = f.card();
    c.cut = Some(Region::Recovery);
    assert!(rollback(&mut c, &f.state(), "test", true, None).is_err());
    assert_eq!(&f.magic(), b"KRNL");
    // the state still has the trial: the bad image is not taken for the good one
    let st = f.st();
    assert!(st.trial.is_some());
    assert_ne!(st.current.as_ref().map(|c| c.sha256), st.previous.as_ref().map(|c| c.sha256));
    // the retry really writes, and only the card's content ends it
    let r = rollback(&mut f.card(), &f.state(), "test", true, None).unwrap();
    assert!(r.written, "{:?}", r.lines);
    assert_eq!(f.region(Region::Recovery), bootimg(1));
    assert_eq!(f.st().trial, None);
    assert_eq!(&f.magic(), b"OOSK");
    let r = rollback(&mut f.card(), &f.state(), "test", true, None).unwrap();
    assert!(!r.written, "now it holds the image");
}

#[test]
fn cut_slot_write_does_not_block_the_next_install() {
    let f = Fake::new("cut-slot", Some(1));
    let mut c = f.card();
    c.cut = Some(Region::Slot);
    assert!(install(&mut c, &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).is_err());
    // out-of-order writeback: data in the slot, but no header
    f.poke(SLOT_LBA, &[0u8; 2048]);
    f.poke(SLOT_LBA + 16, &[0x55; 512]);
    let r = install(&mut f.card(), &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).unwrap();
    assert!(r.written, "{:?}", r.lines);
    assert_eq!(f.region(Region::Slot), bootimg(1));
}

#[test]
fn persist_arm_needs_a_good_image_and_no_trial() {
    let f = Fake::new("arm", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).unwrap();
    f.card().disarm_oos().unwrap();
    assert!(arm_checked(&mut f.card(), &f.state()).is_err(), "on trial");
    confirm(&f.state(), None).unwrap();
    assert_eq!(arm_checked(&mut f.card(), &f.state()), Ok(true));
    f.card().disarm_oos().unwrap();
    // a damaged recovery is not armed
    f.poke(RECOVERY_LBA + 8, &[0xff; 512]);
    assert!(arm_checked(&mut f.card(), &f.state()).is_err());
    assert_eq!(&f.magic(), b"KRNL");
}

#[test]
fn busy_follows_the_lock() {
    let f = Fake::new("busy", Some(1));
    assert!(!busy(&f.state()), "no lock file yet");
    let l = lock(&f.state()).unwrap();
    assert!(busy(&f.state()));
    drop(l);
    assert!(!busy(&f.state()));
}

// ---- first install from the stock system

/// The model check has its own test, `only_verified_models_by_default`.
fn opt(execute: bool) -> first::FirstOpt {
    first::FirstOpt { execute, any_model: true, reinstall: false }
}

fn reinstall() -> first::FirstOpt {
    first::FirstOpt { reinstall: true, ..opt(true) }
}

/// A stock card: a stock image in `boot` and `recovery`, `backup` empty.
fn stock_card(name: &str) -> Fake {
    let fk = Fake::new(name, None);
    fk.poke(RECOVERY_LBA, b"KRNL\x00\x20\x00\x00stock recovery");
    fk.poke(BOOT_LBA + 1000, b"stock boot, further in");
    fk.poke(100, b"loader");
    fk
}

fn run(fk: &Fake, p: &[u8], o: &first::FirstOpt) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let mut c = Card::open(&fk.dir.join("card"), o.execute).unwrap();
    first::first_install(&mut c, p, &[pk()], &fk.dir.join("save"), None, o, &mut |l| lines.push(l.to_string())).map(|_| lines)
}

fn card_bytes(fk: &Fake) -> Vec<u8> {
    std::fs::read(fk.dir.join("card")).unwrap()
}

#[test]
fn first_install_from_stock() {
    let fk = stock_card("gel-stock");
    let boot0 = fk.card().read(BOOT_LBA, first::BOOT_BYTES).unwrap();
    let rec0 = fk.card().read(RECOVERY_LBA, IMAGE_MAX).unwrap();
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).expect("install");
    let mut c = fk.card();
    assert_eq!(&c.boot_magic().unwrap(), b"OOSK");
    assert_eq!(c.read(RECOVERY_LBA, bootimg(1).len()).unwrap(), bootimg(1));
    // the fallback is the stock image, with the stock magic
    assert_eq!(c.read(BACKUP_LBA, first::BOOT_BYTES).unwrap(), boot0);
    // the rescue set holds what was there before
    assert_eq!(std::fs::read(fk.dir.join("save/boot.bin")).unwrap(), boot0);
    assert_eq!(std::fs::read(fk.dir.join("save/recovery.bin")).unwrap(), rec0);
    assert_eq!(std::fs::read(fk.dir.join("save/card-head.bin")).unwrap().len(), 73728 * 512);
    let sums = std::fs::read_to_string(fk.dir.join("save/SHA256SUMS")).unwrap();
    assert!(sums.contains(&format!("{}  boot.bin", hex(&hash::sha256(&boot0)))), "{}", sums);
}

#[test]
fn first_install_writes_only_its_regions() {
    let fk = stock_card("first-only");
    let before = card_bytes(&fk);
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    let after = card_bytes(&fk);
    let s = SECTOR as usize;
    let ok = [(BOOT_LBA as usize * s, BOOT_LBA as usize * s + 4), (RECOVERY_LBA as usize * s, (RECOVERY_LBA as usize) * s + IMAGE_MAX), (BACKUP_LBA as usize * s, BACKUP_LBA as usize * s + first::BOOT_BYTES)];
    let bad = before.iter().zip(&after).enumerate().find(|(i, (a, b))| a != b && !ok.iter().any(|r| (r.0..r.1).contains(i)));
    assert!(bad.is_none(), "byte {:?} changed", bad.map(|b| b.0));
}

#[test]
fn first_install_dry_run_leaves_the_card_alone() {
    let fk = stock_card("first-dry");
    let before = card_bytes(&fk);
    let l = run(&fk, &package(1, "20260101-000000 kernel x"), &opt(false)).unwrap();
    assert!(l.last().unwrap().contains("dry run"));
    assert!(card_bytes(&fk) == before);
    assert!(fk.dir.join("save/SHA256SUMS").exists());
}

#[test]
fn first_install_again_keeps_the_rescue_set() {
    let fk = stock_card("first-again");
    let rec0 = fk.card().read(RECOVERY_LBA, IMAGE_MAX).unwrap();
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    // OOS started once: the one-shot put KRNL back. The same file selects it again and writes nothing else
    fk.card().disarm_oos().unwrap();
    let l = run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    assert!(l.iter().any(|x| x.contains("already holds this image")) && l.iter().any(|x| x.contains("already holds the stock copy")), "{:?}", l);
    assert_eq!(&fk.card().boot_magic().unwrap(), b"OOSK");
    // a newer build, with OOS selected, and oos-reinstall.txt: same fallback, stock recovery still on the stick
    run(&fk, &package(2, "20260102-000000 kernel x"), &reinstall()).unwrap();
    let mut c = fk.card();
    assert_eq!(&c.boot_magic().unwrap(), b"OOSK");
    assert_eq!(c.read(RECOVERY_LBA, bootimg(2).len()).unwrap(), bootimg(2));
    assert_eq!(std::fs::read(fk.dir.join("save/recovery.bin")).unwrap(), rec0);
}

#[test]
fn the_stick_again_keeps_a_newer_build_and_only_selects_it() {
    let fk = stock_card("first-keep");
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    // a newer build came over the network since; OOS started once, stock is selected
    fk.poke(RECOVERY_LBA, &bootimg(3));
    fk.card().disarm_oos().unwrap();
    let before = card_bytes(&fk);
    let l = run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    assert!(l.iter().any(|x| x.contains("kept, only selected")), "{:?}", l);
    assert_eq!(fk.card().read(RECOVERY_LBA, bootimg(3).len()).unwrap(), bootimg(3), "the newer build stays");
    let after = card_bytes(&fk);
    let s = SECTOR as usize;
    let changed: Vec<usize> = before.iter().zip(&after).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect();
    assert!(changed.iter().all(|&i| (BOOT_LBA as usize * s..BOOT_LBA as usize * s + 4).contains(&i)), "only the magic changes");
    assert_eq!(&fk.card().boot_magic().unwrap(), b"OOSK");
}

#[test]
fn the_stick_again_repairs_a_damaged_recovery_from_the_slot() {
    let fk = stock_card("first-repair");
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    // an update was cut: the slot holds the last good build, recovery a half-written one, stock selected
    fk.poke(SLOT_LBA, &bootimg(3));
    let mut half = bootimg(4);
    let n = half.len();
    half[n / 2..].iter_mut().for_each(|b| *b = 0);
    fk.poke(RECOVERY_LBA, &half);
    fk.card().disarm_oos().unwrap();
    let l = run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    assert!(l.iter().any(|x| x.contains("rollback image goes back")), "{:?}", l);
    assert_eq!(fk.card().read(RECOVERY_LBA, bootimg(3).len()).unwrap(), bootimg(3));
    assert_eq!(&fk.card().boot_magic().unwrap(), b"OOSK");
    // with an empty slot the stick's build goes there
    let fk = stock_card("first-repair-empty");
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    fk.poke(RECOVERY_LBA, &half);
    fk.card().disarm_oos().unwrap();
    run(&fk, &package(2, "20260102-000000 kernel x"), &opt(true)).unwrap();
    assert_eq!(fk.card().read(RECOVERY_LBA, bootimg(2).len()).unwrap(), bootimg(2));
}

#[test]
fn first_install_refuses_and_writes_nothing() {
    let good = package(1, "20260101-000000 kernel x");
    let check = |name: &str, poke: &dyn Fn(&Fake), p: &[u8], want: &str| {
        let fk = stock_card(name);
        poke(&fk);
        let before = card_bytes(&fk);
        let e = run(&fk, p, &opt(true)).expect_err(name);
        assert!(e.contains(want), "{}: {}", name, e);
        assert!(card_bytes(&fk) == before, "{} wrote the card", name);
    };
    check("first-parm", &|f| f.poke(PARM_LBA, &[b'x'; 1024]), &good, "partition table");
    check("first-magic", &|f| f.poke(BOOT_LBA, b"XXXX"), &good, "boot magic");
    check("first-rec", &|f| f.poke(RECOVERY_LBA, b"something else"), &good, "unknown");
    check("first-bak", &|f| f.poke(BACKUP_LBA + 5, b"data"), &good, "backup");
    let mut bad = good.clone();
    let n = bad.len();
    bad[n - 100] ^= 1;
    check("first-pkg", &|_| {}, &bad, "");
}

#[test]
fn first_install_stops_when_the_stick_fails() {
    let fk = stock_card("first-stick");
    std::fs::write(fk.dir.join("save"), b"a file where the directory should go").unwrap();
    let before = card_bytes(&fk);
    assert!(run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).is_err());
    assert!(card_bytes(&fk) == before);
}

#[test]
fn first_install_power_cut_leaves_stock_selected() {
    for r in [Region::Fallback, Region::Recovery] {
        let fk = stock_card(&format!("first-cut-{:?}", r));
        let mut c = fk.card();
        c.cut = Some(r);
        let mut l = |_: &str| {};
        let p = package(1, "20260101-000000 kernel x");
        assert!(first::first_install(&mut c, &p, &[pk()], &fk.dir.join("save"), None, &opt(true), &mut l).is_err());
        assert_eq!(&c.boot_magic().unwrap(), b"KRNL", "{:?}", r);
        drop(c);
        // the same file finishes the job, also over a half-written copy
        run(&fk, &p, &opt(true)).unwrap();
        assert_eq!(&fk.card().boot_magic().unwrap(), b"OOSK");
        assert_eq!(fk.card().read(RECOVERY_LBA, bootimg(1).len()).unwrap(), bootimg(1));
    }
}

#[test]
fn first_install_reinstall_with_oos_selected_goes_through_stock() {
    let fk = stock_card("first-cut-oos");
    run(&fk, &package(1, "20260101-000000 kernel x"), &opt(true)).unwrap();
    let mut c = fk.card();
    c.cut = Some(Region::Recovery);
    let mut l = |_: &str| {};
    let p = package(2, "20260102-000000 kernel x");
    assert!(first::first_install(&mut c, &p, &[pk()], &fk.dir.join("save"), None, &reinstall(), &mut l).is_err());
    assert_eq!(&c.boot_magic().unwrap(), b"KRNL", "a half-written recovery must not be selected");
}

#[test]
fn a_rescue_set_from_another_unit_stops_the_install() {
    let fk = stock_card("first-other-unit");
    let vendor = |sn: &str| crate::model::encode_vendor(&[(5, "DHO924S"), (7, sn), (9, "00:19:AF:00:00:01")]);
    let go = |sn: &str, build: &str, n: u8| {
        let mut c = fk.card();
        first::first_install(&mut c, &package(n, build), &[pk()], &fk.dir.join("save"), Some(&vendor(sn)), &opt(true), &mut |_| {})
    };
    go("DHO9A", "20260101-000000 kernel x", 1).unwrap();
    assert_eq!(std::fs::read_to_string(fk.dir.join("save").join(first::UNIT)).unwrap(), "DHO9A");
    let before = card_bytes(&fk);
    let e = go("DHO9B", "20260102-000000 kernel x", 2).unwrap_err();
    assert!(e.contains("DHO9A") && e.contains("DHO9B"), "{}", e);
    assert!(card_bytes(&fk) == before, "nothing written");
    go("DHO9A", "20260103-000000 kernel x", 3).unwrap();
}

#[test]
fn only_verified_models_by_default() {
    assert!(first::check_model(None, false).is_err());
    assert!(first::check_model(None, true).is_ok());
    assert!(first::check_model(Some(b"not a vendor file"), false).is_err());
    let dir = std::env::var("OOS_CAL_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal").to_string());
    if let Ok(v) = std::fs::read(format!("{}/vendor.bin", dir)) {
        assert_eq!(first::check_model(Some(&v), false).unwrap(), "DHO924S");
    }
}

/// Verified per hardware variant: a model OOS ran on, and the others of its variant (a DHO804 with the identity of
/// a DHO824, as seen in a user report). The refusal names the model, the verified models, the risk and the way on.
#[test]
fn models_are_verified_per_hardware_variant() {
    let check = |m: &str, any: bool| first::check_model(Some(&crate::model::encode_vendor(&[(5, m), (7, "DHO8A000000001")])), any);
    assert_eq!(check("DHO924S", false).unwrap(), "DHO924S");
    assert_eq!(check("DHO804", false).unwrap(), "DHO804");
    for (m, ran) in [("DHO824", "DHO804"), ("DHO814", "DHO804"), ("DHO914S", "DHO924S")] {
        let ok = check(m, false).unwrap();
        assert!(ok.starts_with(m) && ok.contains(ran) && ok.contains("same hardware"), "{}", ok);
    }
    assert_eq!(first::verified_models(), ["DHO804", "DHO814", "DHO824", "DHO914S", "DHO924S"]);
    for m in ["DHO802", "DHO812", "DHO914", "DHO924", "DHO914-EDU", "DHO924-EDU"] {
        let e = check(m, false).unwrap_err();
        assert!(e.contains(&format!("OOS has not run on a {} yet", m)), "{}", e);
        assert!(e.contains("Nothing written") && e.contains("DHO804, DHO814, DHO824, DHO914S, DHO924S"), "{}", e);
        assert!(e.contains(first::ANY_MODEL_FILE) && e.contains(first::ANY_MODEL_FLAG) && e.contains("open the scope"), "{}", e);
        let ok = check(m, true).unwrap();
        assert!(ok.starts_with(m) && ok.contains("accepted by request"), "{}", ok);
    }
    // unreadable: the same help; another family is refused also with the file
    let e = first::check_model(Some(b"not a vendor file"), false).unwrap_err();
    assert!(e.contains("vendor.bin") && e.contains(first::ANY_MODEL_FILE), "{}", e);
    assert!(check("MHO984", true).unwrap_err().contains("unknown model MHO984"));
}

/// The variants of the board family: every model of the table in one at most, and the models of one variant differ
/// in nothing but the bandwidth code (docs/models.md 2.1).
#[test]
fn hardware_variants_differ_only_in_bandwidth() {
    let t = &crate::target::ACTIVE;
    let mut seen = Vec::new();
    for v in t.variants {
        assert!(!v.models.is_empty(), "{}", v.name);
        for (r, src) in v.ran_on {
            assert!(v.models.contains(r) && !src.is_empty(), "{}: {}", v.name, r);
        }
        let c0 = crate::model::lookup(v.models[0]).unwrap_or_else(|| panic!("{} not in the table", v.models[0]));
        for m in v.models.iter() {
            assert!(!seen.contains(m), "{} in two variants", m);
            seen.push(*m);
            let c = crate::model::lookup(m).unwrap();
            assert_eq!((c.series, c.channels, c.la, c.generators, c.ext_trig, c.edu), (c0.series, c0.channels, c0.la, c0.generators, c0.ext_trig, c0.edu), "{} vs {}", m, v.models[0]);
            assert_eq!(t.variant(&m.to_ascii_lowercase()).map(|x| x.name), Some(v.name));
        }
    }
    // every real model has a variant, verified or not
    for m in ["DHO802", "DHO804", "DHO812", "DHO814", "DHO824", "DHO914", "DHO914S", "DHO914-EDU", "DHO924", "DHO924S", "DHO924-EDU"] {
        assert!(t.variant(m).is_some(), "{}", m);
    }
    assert!(t.variant("DHO900").is_none());
}

#[test]
fn arming_by_hand_needs_a_valid_recovery() {
    let f = Fake::new("arm-hand", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    // a cut install leaves stock selected and a half-written recovery: arming by hand refuses it
    let mut c = f.card();
    c.cut = Some(Region::Recovery);
    assert!(install(&mut c, &f.state(), &package(3, "20261105-180000"), &[pk()], &RUN).is_err());
    assert_eq!(&f.magic(), b"KRNL");
    assert!(arm_by_hand(&mut f.card(), &f.state()).is_err());
    assert_eq!(&f.magic(), b"KRNL", "the half-written image stays unselected");
    // after a finished install it arms, a trial image included
    install(&mut f.card(), &f.state(), &package(3, "20261105-180000"), &[pk()], &RUN).unwrap();
    f.card().disarm_oos().unwrap();
    assert!(arm_by_hand(&mut f.card(), &f.state()).unwrap());
    assert_eq!(&f.magic(), b"OOSK");
}

#[test]
fn confirm_needs_the_image_on_trial_to_run() {
    let f = Fake::new("confirm-run", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000"), &[pk()], &RUN).unwrap();
    // written, not started: the old build still runs
    assert!(confirm_running(&f.state(), None, Some("20261101-000000\n")).is_err());
    assert_eq!(f.st().trial, Some(0));
    assert!(confirm_running(&f.state(), None, Some("20261105-170000\n")).unwrap());
    assert_eq!(f.st().trial, None);
}

#[test]
fn build_ids_compare_without_surrounding_space() {
    let f = Fake::new("trim", Some(1));
    install(&mut f.card(), &f.state(), &package(2, "20261105-170000 kernel x"), &[pk()], &RUN).unwrap();
    assert_eq!(boot_started(&f.state(), Some("20261105-170000 kernel x\n")).unwrap(), Boot::Trial(1));
}

#[test]
fn images_outside_the_loader_limits_are_refused() {
    let mut b = bootimg(5);
    b[12..16].copy_from_slice(&0x0008_0000u32.to_le_bytes());
    // the id covers the header: a moved load address is refused for the address, not only for the id
    let e = card::check_image(&b).unwrap_err();
    assert!(e.contains("load address") || e.contains("id mismatch"), "{}", e);
    assert!(card::check_image(&bootimg(5)).is_ok());
}

// ---- trial start by kexec

/// A package whose image finishes a kexec start, with device tree `dt`.
fn kexec_package(tag: u8, dt: u8) -> Vec<u8> {
    pkg::build_with(&SEED, KX_BUILD, "6.12.115+", &bootimg_dt(tag, dt), &[kexec::LINE]).unwrap()
}

const KX_BUILD: &str = "20261108-090000";

/// The device tree section `bootimg_dt(_, dt)` has.
fn tree(dt: u8) -> Vec<u8> {
    vec![dt; 300]
}

fn stage(f: &Fake, p: &[u8], booted: Option<&[u8]>, running: Option<&str>) -> Result<(Report, Option<Staged>), String> {
    kexec_stage(&mut f.card(), &f.state(), p, &[pk()], &RUN, booted, running)
}

#[test]
fn kexec_stage_writes_nothing_to_the_card() {
    let f = Fake::new("kexec-stage", None);
    f.poke(RECOVERY_LBA, &bootimg_dt(1, 7));
    f.card().arm_oos().unwrap();
    let before = std::fs::read(f.dir.join("card")).unwrap();
    let p = kexec_package(2, 7);
    let (r, staged) = stage(&f, &p, Some(&tree(7)), None).unwrap();
    let staged = staged.expect("a kexec start");
    assert!(r.lines.iter().any(|l| l.contains("device tree: the same")), "{:?}", r.lines);
    let img = bootimg_dt(2, 7);
    let (k, rd, _) = kexec::parts(&img).unwrap();
    assert_eq!(staged.kernel, k);
    assert_eq!(&staged.initrd[..rd.len()], rd);
    assert!(staged.initrd.windows(64).any(|w| w == &p[..64]), "the package rides along");
    assert!(staged.initrd.windows(64).any(|w| w == hex(&pk()).as_bytes()), "and its key");
    // the card as it was, OOS still selected: a hang of the new kernel ends in the running build again
    assert!(std::fs::read(f.dir.join("card")).unwrap() == before);
    assert_eq!(&f.magic(), b"OOSK");
    assert_eq!(f.st().kexec.as_deref(), Some(KX_BUILD));
    assert_eq!(f.st().trial, None);

    // the started image installs itself: on trial, this start counted, the mark gone
    let r = install(&mut f.card(), &f.state(), &p, &[pk()], &RUN).unwrap();
    assert!(r.written);
    assert_eq!(f.st().kexec, None);
    assert_eq!(boot_started(&f.state(), Some(KX_BUILD)).unwrap(), Boot::Trial(1));
    assert_eq!(&f.magic(), b"OOSK");
    assert_eq!(f.region(Region::Recovery), img);
    assert_eq!(f.region(Region::Slot), bootimg_dt(1, 7), "the previous build is the rollback image");
}

#[test]
fn kexec_stage_refuses_what_it_cannot_try() {
    let f = Fake::new("kexec-refuse", None);
    f.poke(RECOVERY_LBA, &bootimg_dt(1, 7));
    let before = std::fs::read(f.dir.join("card")).unwrap();
    // another device tree than the running one
    let e = stage(&f, &kexec_package(2, 8), Some(&tree(7)), None).unwrap_err();
    assert!(e.contains("device tree"), "{}", e);
    // the running tree differs from recovery's (an image written without a power-cycle since): the running one counts
    let e = stage(&f, &kexec_package(2, 7), Some(&tree(8)), None).unwrap_err();
    assert!(e.contains("device tree"), "{}", e);
    // a package of a build that does not finish a kexec start
    let e = stage(&f, &package(2, KX_BUILD), Some(&tree(7)), None).unwrap_err();
    assert!(e.contains("--no-kexec"), "{}", e);
    // the running tree unknown, and recovery not known to hold the running build
    let e = stage(&f, &kexec_package(2, 7), None, Some("20261101-000000")).unwrap_err();
    assert!(e.contains("not known"), "{}", e);
    assert!(std::fs::read(f.dir.join("card")).unwrap() == before, "nothing written");
    assert_eq!(f.st().kexec, None);
    // the same image: nothing to do, no kexec start
    let (r, staged) = stage(&f, &kexec_package(1, 7), Some(&tree(7)), None).unwrap();
    assert!(staged.is_none() && r.unchanged);
    assert!(r.lines.last().unwrap().contains("already holds"), "{:?}", r.lines);
}

#[test]
fn kexec_stage_takes_the_tree_from_recovery_when_it_runs() {
    let f = Fake::new("kexec-rec", Some(1));
    install(&mut f.card(), &f.state(), &package(3, "20261101-000000"), &[pk()], &RUN).unwrap();
    confirm(&f.state(), None).unwrap();
    let dt3 = kexec::parts(&bootimg(3)).unwrap().2.to_vec();
    let p = pkg::build_with(&SEED, KX_BUILD, "k", &bootimg_dt(4, dt3[0]), &[kexec::LINE]).unwrap();
    assert!(stage(&f, &p, None, Some("20261101-000000")).unwrap().1.is_some());
}

#[test]
fn a_kexec_start_that_did_not_finish_is_reported_once() {
    let f = Fake::new("kexec-missed", None);
    f.poke(RECOVERY_LBA, &bootimg_dt(1, 7));
    stage(&f, &kexec_package(2, 7), Some(&tree(7)), None).unwrap();
    // the kexec start itself does not report
    assert_eq!(kexec_missed(&f.state(), true).unwrap(), None);
    // power-cycled after a hang: the previous OOS build again
    let m = kexec_missed(&f.state(), false).unwrap().expect("a message");
    assert!(m.contains(KX_BUILD) && m.contains("not installed"), "{}", m);
    assert_eq!(kexec_missed(&f.state(), false).unwrap(), None);
    assert_eq!(f.region(Region::Recovery), bootimg_dt(1, 7));
    // a failed load is forgotten the same way
    stage(&f, &kexec_package(2, 7), Some(&tree(7)), None).unwrap();
    kexec_clear(&f.state(), "kexec load failed").unwrap();
    assert_eq!(f.st().kexec, None);
    assert_eq!(f.st().event, "kexec load failed");
    // an install in the kexec start cut by a power loss keeps the mark for the next start
    stage(&f, &kexec_package(2, 7), Some(&tree(7)), None).unwrap();
    let mut c = f.card();
    c.cut = Some(Region::Recovery);
    assert!(install(&mut c, &f.state(), &kexec_package(2, 7), &[pk()], &RUN).is_err());
    assert_eq!(f.st().kexec.as_deref(), Some(KX_BUILD));
}
