// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-update: signed OOS updates with rollback (oos_hal::update, docs/update.md).
//!
//! On the unit:
//!   oos-update status [--quick]            what recovery and the rollback slot hold, trial state, boot mode
//!                                          (--quick: headers only, no SHA-256 over the images)
//!   oos-update install FILE.oosupd [--execute] [--downgrade] [--force]
//!                                          check the signature and the image, keep the running image in the rollback
//!                                          slot, write the new one to recovery, read it back, arm OOS (dry run without
//!                                          --execute); the new image starts on trial at the next power-on
//!   oos-update rollback [--execute]        write the rollback image back to recovery and arm OOS
//!   oos-update gel-install FILE.oosupd --save DIR [--any-model] [--execute]
//!                                          first install from the running stock system, rescue set in DIR
//!   oos-update confirm                     end the trial by hand (the watcher normally does it); only once the image
//!                                          on trial runs
//!   oos-update arm                         select OOS for the next power-on: only a valid image in recovery, never
//!                                          while an update job runs (oos-boot-oos-again)
//!   oos-update mode oneshot|persist        oneshot (default): every power-on after an OOS start boots stock;
//!                                          persist: OOS arms itself again after every confirmed start
//!   oos-update boot                        rcS, early: count the start of a trial image; a trial image that starts
//!                                          a second time without confirmation is rolled back at once
//!   oos-update watch [--status FILE]       rcS, background: judge the start (oos-ui stable 20 s and start-up
//!                                          finished = good; 3 oos-ui crashes or no end in 180 s = bad), then confirm
//!                                          (+ arm in persist mode) or roll back; message in /tmp/oos-update.notice
//! Anywhere:
//!   oos-update verify FILE.oosupd [--keys FILE]   signature and image check, no card access
//!   oos-update keygen [--out FILE]         new signing key (default ~/.config/oos/update-signing.key, mode 0600),
//!                                          prints the public key line for oos/keys/update.pub
//!   oos-update pack --image IMG --build ID [--kernel REL] [--key FILE] --out FILE.oosupd
//!
//! Common options: --card /dev/mmcblk0, --state /oos/update/state, --keys FILE (extra trusted keys: verify, and
//! install on a card file for tests; refused for the unit's card).

use oos_hal::update::{self, card, hash::hex, pkg, Card, InstallOpt, State};
use std::path::{Path, PathBuf};

const NOTICE: &str = "/tmp/oos-update.notice";

struct Args {
    rest: Vec<String>,
    flags: Vec<String>,
    vals: Vec<(String, String)>,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args { rest: Vec::new(), flags: Vec::new(), vals: Vec::new() };
        let mut it = std::env::args().skip(1);
        while let Some(s) = it.next() {
            match s.as_str() {
                "--execute" | "--downgrade" | "--force" | "--quick" | "--any-model" => a.flags.push(s),
                _ if s.starts_with("--") => {
                    let v = it.next().unwrap_or_else(|| die(&format!("{} needs a value", s)));
                    a.vals.push((s, v));
                }
                _ => a.rest.push(s),
            }
        }
        a
    }
    fn flag(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }
    fn val(&self, k: &str) -> Option<&str> {
        self.vals.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
    }
}

fn die(msg: &str) -> ! {
    eprintln!("oos-update: {}", msg);
    std::process::exit(1)
}

/// Lines also to the kernel log (and the panel console, which shows when oos-ui is down).
fn tell(msg: &str) {
    println!("{}", msg);
    for dev in ["/dev/kmsg", "/dev/tty1"] {
        if Path::new(dev).exists() {
            let _ = std::fs::OpenOptions::new().append(true).open(dev).and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "oos-update: {}", msg)
            });
        }
    }
}

fn notice(msg: &str) {
    let _ = std::fs::write(NOTICE, format!("{}\n", msg));
    tell(msg);
}

fn open_card(a: &Args, write: bool) -> Card {
    let path = a.val("--card").unwrap_or("/dev/mmcblk0");
    if path == "/dev/mmcblk0" {
        // the SD card on fe320000, as oos-oneshot checks
        let dev = std::fs::canonicalize("/sys/block/mmcblk0/device").ok();
        if !dev.is_some_and(|d| d.to_string_lossy().contains("fe320000")) {
            die("mmcblk0 is not the SD card on fe320000: not touching it");
        }
    }
    Card::open(Path::new(path), write).unwrap_or_else(|e| die(&format!("{}: {}", path, e)))
}

/// The SD card as the stock system names it.
fn stock_card() -> Option<String> {
    ["mmcblk0", "mmcblk1"].iter().find_map(|n| {
        let dev = std::fs::canonicalize(format!("/sys/block/{}/device", n)).ok()?;
        dev.to_string_lossy().contains("fe320000").then(|| format!("/dev/block/{}", n))
    })
}

/// First install from the stock system (docs/install-usb.md); `shell/do_extract.sh` of the update file runs this.
fn gel_install(a: &Args) {
    let f = a.rest.get(1).unwrap_or_else(|| die("gel-install FILE.oosupd --save DIR [--any-model] [--execute]"));
    let save = a.val("--save").unwrap_or_else(|| die("gel-install needs --save DIR on the USB stick"));
    let p = read_file(f);
    let opt = update::first::FirstOpt { execute: a.flag("--execute"), any_model: a.flag("--any-model") };
    let path = a.val("--card").map(str::to_string).or_else(stock_card).unwrap_or_else(|| die("no SD card on fe320000"));
    println!("card {}", path);
    let vendor = std::fs::read(a.val("--vendor").unwrap_or("/rigol/data/vendor.bin")).ok();
    let mut c = Card::open(Path::new(&path), opt.execute).unwrap_or_else(|e| die(&format!("{}: {}", path, e)));
    let keys = keys(a, a.val("--card").is_some());
    let r = update::first::first_install(&mut c, &p, &keys, Path::new(save), vendor.as_deref(), &opt, &mut |l| println!("{}", l));
    if let Err(e) = r {
        die(&e);
    }
}

fn state_path(a: &Args) -> PathBuf {
    PathBuf::from(a.val("--state").unwrap_or(update::STATE_PATH))
}

/// The built-in keys; `--keys FILE` adds keys for `verify` only (and for tests on a card file, never on the unit's card).
fn keys(a: &Args, extra: bool) -> Vec<[u8; 32]> {
    let mut k = update::trusted_keys();
    if !extra && a.val("--keys").is_some() {
        die("--keys is for verify (and card-file tests) only");
    }
    if let Some(f) = a.val("--keys") {
        let t = std::fs::read_to_string(f).unwrap_or_else(|e| die(&format!("{}: {}", f, e)));
        k.extend(pkg::parse_keys(&t));
    }
    k
}

/// The state file lives on /oos: refuse to work without it (a fresh state would forget the rollback image).
fn need_data(a: &Args) {
    if a.val("--state").is_some() {
        return;
    }
    if let Ok(m) = std::fs::read_to_string("/proc/mounts") {
        if !m.lines().any(|l| l.split_whitespace().nth(1) == Some("/oos")) {
            die("/oos (OOS data partition) is not mounted: the update state lives there (oos-mount-data)");
        }
    }
}

fn read_file(p: &str) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| die(&format!("{}: {}", p, e)))
}

fn main() {
    let a = Args::parse();
    let cmd = a.rest.first().map(String::as_str).unwrap_or("status");
    match cmd {
        "status" => status(&a),
        "verify" => {
            let f = a.rest.get(1).unwrap_or_else(|| die("verify FILE"));
            let p = read_file(f);
            match pkg::verify(&p, &keys(&a, true)) {
                Ok((h, img)) => {
                    let m = &h.manifest;
                    println!("{}: signature ok (key {}), build {}, {} bytes", f, hex(&m.key), m.build, m.size);
                    match card::check_image(img) {
                        Ok(i) => println!("image '{}', loader id {} ok", i.name, hex(&i.id)),
                        Err(e) => die(&format!("image: {}", e)),
                    }
                }
                Err(e) => die(&e),
            }
        }
        "install" => {
            let f = a.rest.get(1).unwrap_or_else(|| die("install FILE.oosupd [--execute]"));
            need_data(&a);
            let p = read_file(f);
            let opt = InstallOpt { execute: a.flag("--execute"), downgrade: a.flag("--downgrade"), force: a.flag("--force") };
            let mut c = open_card(&a, opt.execute);
            let test_card = a.val("--card").is_some_and(|c| c != "/dev/mmcblk0");
            match update::install(&mut c, &state_path(&a), &p, &keys(&a, test_card), &opt) {
                Ok(r) => r.lines.iter().for_each(|l| println!("{}", l)),
                Err(e) => die(&e),
            }
        }
        "gel-install" => gel_install(&a),
        "rollback" => {
            need_data(&a);
            let ex = a.flag("--execute");
            let mut c = open_card(&a, ex);
            match update::rollback(&mut c, &state_path(&a), "by hand", ex, None) {
                Ok(r) => r.lines.iter().for_each(|l| println!("{}", l)),
                Err(e) => die(&e),
            }
        }
        "confirm" => {
            need_data(&a);
            let running = std::fs::read_to_string("/etc/oos-build").ok();
            match update::confirm_running(&state_path(&a), None, running.as_deref()) {
                Ok(true) => println!("trial ended: the image in recovery is confirmed"),
                Ok(false) => println!("no image on trial"),
                Err(e) => die(&e),
            }
        }
        "arm" => {
            need_data(&a);
            let mut c = open_card(&a, true);
            match update::arm_by_hand(&mut c, &state_path(&a)) {
                Ok(true) => println!("next boot: OOS"),
                Ok(false) => println!("OOS was already selected"),
                Err(e) => die(&e),
            }
        }
        "mode" => {
            need_data(&a);
            let p = state_path(&a);
            match a.rest.get(1).map(String::as_str) {
                // the choice counts from the next power-on: the boot selection follows at once
                Some("persist") => {
                    update::set_mode(&p, true).unwrap_or_else(|e| die(&e));
                    if let Err(e) = update::arm_checked(&mut open_card(&a, true), &p) {
                        println!("not selected for the next power-on yet: {}", e);
                    }
                }
                Some("oneshot") => {
                    update::set_mode(&p, false).unwrap_or_else(|e| die(&e));
                    open_card(&a, true).disarm_oos().unwrap_or_else(|e| die(&e));
                }
                None => {}
                Some(m) => die(&format!("mode '{}': oneshot or persist", m)),
            }
            let st = State::load(&p).unwrap_or_else(|e| die(&e.to_string()));
            println!(
                "mode {}",
                if st.persist {
                    "persist: OOS arms itself again after every confirmed start (an unconfirmed start -> stock next time)"
                } else {
                    "oneshot: the power-on after an OOS start boots stock (oos-boot-oos-again to stay)"
                }
            );
        }
        "boot" => boot(&a),
        "watch" => watch(&a),
        "keygen" => keygen(&a),
        "pack" => pack(&a),
        _ => die(&format!("unknown command '{}' (see the head of fpgad/src/bin/oos-update.rs)", cmd)),
    }
}

fn status(a: &Args) {
    let mut c = open_card(a, false);
    let p = state_path(a);
    let st = match State::load(&p) {
        Ok(s) => s,
        Err(e) => {
            // not "no trial": the values below are defaults then
            println!("state        UNREADABLE ({}): {}; is /oos mounted?", p.display(), e);
            State::default()
        }
    };
    match c.boot_magic() {
        Ok(m) => println!(
            "boot magic   {} ({})",
            String::from_utf8_lossy(&m),
            match &m {
                b"OOSK" => "next power-on: OOS",
                b"KRNL" => "next power-on: stock Android",
                _ => "unexpected",
            }
        ),
        Err(e) => println!("boot magic   unreadable: {}", e),
    }
    if let Err(e) = c.check_layout() {
        println!("layout       {}", e);
    }
    for r in [card::Region::Recovery, card::Region::Slot] {
        match c.header(r) {
            Ok(Some(h)) if a.flag("--quick") => {
                let known = [&st.current, &st.previous]
                    .iter()
                    .filter_map(|e| e.as_ref())
                    .find(|e| e.len == h.len as u64)
                    .map_or("untracked".to_string(), |e| format!("same size as build {}", e.build));
                println!("{:<12} '{}', {} bytes, {}, loader id {}", if r == card::Region::Recovery { "recovery" } else { "rollback" }, h.name, h.len, known, &hex(&h.id)[..16]);
            }
            Ok(Some(h)) => {
                let (b, sha) = c.read_image(r, h.len).unwrap_or_else(|e| die(&e.to_string()));
                let ok = card::check_image(&b).map_or_else(|e| format!("DAMAGED: {}", e), |_| "loader id ok".into());
                let known = [&st.current, &st.previous]
                    .iter()
                    .filter_map(|e| e.as_ref())
                    .find(|e| e.sha256 == sha)
                    .map_or("untracked".to_string(), |e| format!("build {}", e.build));
                println!("{:<12} '{}', {} bytes, {}, {}, sha256 {}", if r == card::Region::Recovery { "recovery" } else { "rollback" }, h.name, h.len, known, ok, &hex(&sha)[..16]);
            }
            Ok(None) => println!("{:<12} no image", if r == card::Region::Recovery { "recovery" } else { "rollback" }),
            Err(e) => println!("{:?}: unreadable: {}", r, e),
        }
    }
    match st.trial {
        Some(n) => println!("trial        on trial, {} start(s) without confirmation", n),
        None => println!("trial        none"),
    }
    println!("mode         {}", if st.persist { "persist" } else { "oneshot" });
    if !st.event.is_empty() {
        println!("last event   {}", st.event);
    }
    if let Ok(b) = std::fs::read_to_string("/etc/oos-build") {
        println!("running      {}", b.trim());
    }
    println!("trusted keys {}", update::trusted_keys().iter().map(|k| hex(&pkg::key_id(k))).collect::<Vec<_>>().join(", "));
}

fn boot(a: &Args) {
    let p = state_path(a);
    if !p.parent().is_some_and(|d| d.exists()) {
        return; // no update state yet
    }
    let running = update::running_build();
    match update::boot_started(&p, running.as_deref()) {
        Ok(update::Boot::NoTrial) => {}
        Ok(update::Boot::Trial(n)) => tell(&format!("build {} on trial (start {})", update::short(running.as_deref().unwrap_or("?")), n)),
        Ok(update::Boot::Dropped(m)) => tell(&m),
        Ok(update::Boot::Rollback) => {
            // on the panel: nothing else shows yet, and a power-cycle in this write is what must not happen
            let _ = std::fs::write("/dev/tty1", "\nUpdate not confirmed: restoring the previous build. Do not power off.\n");
            let mut c = open_card(a, true);
            match update::rollback(&mut c, &p, "it started a second time without having been confirmed", true, None) {
                Ok(r) => {
                    r.lines.iter().for_each(|l| println!("{}", l));
                    let st = State::load(&p).unwrap_or_default();
                    notice(&format!("Update {}. Power-cycle the scope to start the restored build.", st.event));
                }
                Err(e) => notice(&format!("Update on trial did not confirm, rollback FAILED: {}", e)),
            }
        }
        Err(e) => tell(&e),
    }
}

/// The oos-ui process: the one followed so far while it lives (a child that oos-ui forks is "oos-ui" until its exec),
/// else the oldest one.
#[cfg(target_os = "linux")]
fn ui_pid(keep: Option<u32>) -> Option<u32> {
    let is_ui = |pid: u32| std::fs::read_to_string(format!("/proc/{}/comm", pid)).is_ok_and(|c| c.trim() == "oos-ui");
    if let Some(k) = keep.filter(|&k| is_ui(k)) {
        return Some(k);
    }
    let mut best: Option<u32> = None;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        if is_ui(pid) {
            best = Some(best.map_or(pid, |b| b.min(pid)));
        }
    }
    best
}

#[cfg(target_os = "linux")]
fn watch(a: &Args) {
    use update::{Health, Verdict};
    let p = state_path(a);
    let status = a.val("--status").unwrap_or("/tmp/oos-boot.status").to_string();
    let autoacq = Path::new("/etc/oos-autoacq").exists();
    // the verdict is about the image that runs now: the state as it was at this start, applied only if unchanged
    let st0 = State::load(&p).unwrap_or_default();
    let running = update::running_build();
    let trial_sha = st0
        .current
        .as_ref()
        .filter(|c| st0.trial.is_some() && running.as_deref() == Some(c.build.as_str()))
        .map(|c| c.sha256);
    let t0 = std::time::Instant::now();
    let (mut pid, mut since, mut restarts, mut seen) = (None::<u32>, 0.0f64, 0u32, false);
    let verdict = loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let now = t0.elapsed().as_secs_f64();
        if Path::new("/tmp/oos-ui.off").exists() {
            tell("oos-ui stopped by hand (/tmp/oos-ui.off): start not judged, trial left open (oos-update confirm)");
            return;
        }
        let cur = ui_pid(pid);
        if cur != pid {
            if cur.is_some() {
                if seen {
                    restarts += 1;
                }
                seen = true;
                since = now;
            }
            pid = cur;
        }
        let startup_done = !autoacq
            || std::fs::read_to_string(&status)
                .is_ok_and(|t| t.lines().any(|l| matches!(l.split('\t').nth(1), Some("end") | Some("abort"))));
        let h = Health { elapsed: now, ui_restarts: restarts, ui_up: if pid.is_some() { now - since } else { 0.0 }, startup_done };
        match update::judge(&h) {
            Verdict::Wait => continue,
            v => break v,
        }
    };
    let st = st0;
    match verdict {
        Verdict::Good => {
            if trial_sha.is_some() {
                match update::confirm(&p, trial_sha) {
                    Ok(true) => tell(&format!("build {} confirmed (trial passed)", st.current.as_ref().map_or("?", |c| update::short(&c.build)))),
                    Ok(false) => tell("start was good, but the update state changed meanwhile: nothing confirmed"),
                    Err(e) => tell(&e),
                }
            }
            if st.persist {
                let mut c = open_card(a, true);
                match update::arm_checked(&mut c, &p) {
                    Ok(true) => tell("persist mode: OOS armed for the next power-on"),
                    Ok(false) => {}
                    Err(e) => tell(&format!("persist mode: {}", e)),
                }
            }
        }
        Verdict::Bad(why) => {
            if trial_sha.is_some() && st.previous.is_some() {
                let mut c = open_card(a, true);
                match update::rollback(&mut c, &p, &why, true, trial_sha) {
                    Ok(r) => {
                        r.lines.iter().for_each(|l| println!("{}", l));
                        let prev = st.previous.as_ref().map_or("?", |e| update::short(&e.build));
                        notice(&format!(
                            "Update {} failed its first start ({}). Build {} restored: power-cycle the scope.",
                            st.current.as_ref().map_or("?", |c| update::short(&c.build)),
                            why,
                            prev
                        ));
                    }
                    Err(e) => notice(&format!("Update failed its first start ({}), rollback FAILED: {}", why, e)),
                }
            } else {
                tell(&format!("start not clean ({}); OOS not armed again, the next power-on boots stock", why));
            }
        }
        Verdict::Wait => unreachable!(),
    }
}

#[cfg(not(target_os = "linux"))]
fn watch(_: &Args) {
    die("watch runs on the unit only");
}

fn default_key() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/oos/update-signing.key")
}

fn load_seed(path: &Path) -> [u8; 32] {
    let t = std::fs::read_to_string(path).unwrap_or_else(|e| die(&format!("{}: {} (oos-update keygen)", path.display(), e)));
    t.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .find(|l| !l.is_empty())
        .and_then(update::hash::unhex)
        .and_then(|b| b.try_into().ok())
        .unwrap_or_else(|| die(&format!("{}: no 64-digit hex secret key", path.display())))
}

fn keygen(a: &Args) {
    let out = a.val("--out").map(PathBuf::from).unwrap_or_else(default_key);
    if out.exists() {
        die(&format!("{} exists: not overwritten (a lost key means every unit needs a hand-installed image)", out.display()));
    }
    let mut seed = [0u8; 32];
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut seed)).unwrap_or_else(|e| die(&e.to_string()));
    }
    let pk = oos_hal::update::ed25519::public_key(&seed);
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).unwrap_or_else(|e| die(&e.to_string()));
    }
    let text = format!(
        "# OOS update signing key (Ed25519 secret seed). Keep it secret and backed up; never put it in the repository.\n# public key {} (id {})\n{}\n",
        hex(&pk),
        hex(&pkg::key_id(&pk)),
        hex(&seed)
    );
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&out)
            .unwrap_or_else(|e| die(&format!("{}: {}", out.display(), e)));
        f.write_all(text.as_bytes()).unwrap_or_else(|e| die(&e.to_string()));
    }
    println!("secret key: {} (mode 0600)", out.display());
    println!("add this line to oos/keys/update.pub:");
    println!("{}  # id {}", hex(&pk), hex(&pkg::key_id(&pk)));
}

fn pack(a: &Args) {
    let img = read_file(a.val("--image").unwrap_or_else(|| die("pack --image IMG --build ID --out FILE")));
    let build = a.val("--build").unwrap_or_else(|| die("pack needs --build ID (the image's /etc/oos-build)"));
    let out = a.val("--out").unwrap_or_else(|| die("pack needs --out FILE"));
    let kernel = a.val("--kernel").map(str::to_string).unwrap_or_else(|| build.split(" kernel ").nth(1).unwrap_or("?").to_string());
    let info = card::check_image(&img).unwrap_or_else(|e| die(&format!("image: {}", e)));
    if info.len != img.len() {
        die(&format!("image is {} bytes, its sections end at {}", img.len(), info.len));
    }
    let seed = load_seed(&a.val("--key").map(PathBuf::from).unwrap_or_else(default_key));
    let pk = oos_hal::update::ed25519::public_key(&seed);
    if !update::trusted_keys().contains(&pk) {
        eprintln!("oos-update: WARNING: key {} is not in oos/keys/update.pub: units will refuse this package", hex(&pkg::key_id(&pk)));
    }
    let p = pkg::build(&seed, build, &kernel, &img).unwrap_or_else(|e| die(&e));
    pkg::verify(&p, &[pk]).unwrap_or_else(|e| die(&format!("self-check: {}", e)));
    std::fs::write(out, &p).unwrap_or_else(|e| die(&format!("{}: {}", out, e)));
    println!("{}: build {}, {} bytes, key {}", out, build, p.len(), hex(&pkg::key_id(&pk)));
}
