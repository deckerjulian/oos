// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-update: signed OOS updates with rollback (oos_hal::update, docs/update.md).
//!
//! On the unit:
//!   oos-update status [--quick]            what recovery and the rollback slot hold, trial state, boot mode
//!                                          (--quick: headers only, no SHA-256 over the images)
//!   oos-update install FILE.oosupd [--execute] [--downgrade] [--force] [--no-kexec | --new-dt]
//!                                          check the signature and the image (dry run without --execute), then start
//!                                          the new image by kexec with nothing written; that start
//!                                          installs it (rollback slot, recovery, read back, OOS armed) and is its
//!                                          first trial start. --no-kexec (also: a package without 'trial kexec') and
//!                                          --new-dt (another device tree, which kexec cannot try) write it directly;
//!                                          it starts on trial at the next power-on
//!   oos-update kexec-check FILE.oosupd     load the package's kernel and initramfs for a kexec start and unload them
//!                                          again: nothing written, no restart
//!   oos-update rollback [--execute]        write the rollback image back to recovery and arm OOS
//!   oos-update gel-install FILE.oosupd --save DIR [--any-model] [--reinstall] [--execute]
//!                                          first install from the running stock system, rescue set in DIR; again on
//!                                          a card with a valid OOS image it only selects OOS (--reinstall: writes it)
//!                                          --any-model: also on a model whose hardware variant OOS has not run on
//!                                          yet (hal target `variants`; docs/install-usb.md 4)
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

use oos_hal::update::{self, card, hash::hex, kexec, pkg, Card, InstallOpt, State};
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
                "--execute" | "--downgrade" | "--force" | "--quick" | "--any-model" | "--reinstall" | "--no-kexec" | "--new-dt" => a.flags.push(s),
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
    // one write per line: every write to /dev/kmsg is a record of its own
    let line = format!("oos-update: {}\n", msg);
    for dev in ["/dev/kmsg", "/dev/tty1"] {
        if Path::new(dev).exists() {
            let _ = std::fs::OpenOptions::new().append(true).open(dev).and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
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
        // the boot card of this board, as oos-oneshot checks
        let ctl = oos_hal::target::ACTIVE.card_controller;
        let dev = std::fs::canonicalize("/sys/block/mmcblk0/device").ok();
        if !dev.is_some_and(|d| d.to_string_lossy().contains(ctl)) {
            die(&format!("mmcblk0 is not the SD card on {}: not touching it", ctl));
        }
    }
    Card::open(Path::new(path), write).unwrap_or_else(|e| die(&format!("{}: {}", path, e)))
}

/// The SD card as the stock system names it.
fn stock_card() -> Option<String> {
    ["mmcblk0", "mmcblk1"].iter().find_map(|n| {
        let dev = std::fs::canonicalize(format!("/sys/block/{}/device", n)).ok()?;
        dev.to_string_lossy().contains(oos_hal::target::ACTIVE.card_controller).then(|| format!("/dev/block/{}", n))
    })
}

/// First install from the stock system (docs/install-usb.md); `shell/do_extract.sh` of the update file runs this.
fn gel_install(a: &Args) {
    let f = a.rest.get(1).unwrap_or_else(|| die("gel-install FILE.oosupd --save DIR [--any-model] [--reinstall] [--execute]"));
    let save = a.val("--save").unwrap_or_else(|| die("gel-install needs --save DIR on the USB stick"));
    let p = read_file(f);
    let opt = update::first::FirstOpt { execute: a.flag("--execute"), any_model: a.flag("--any-model"), reinstall: a.flag("--reinstall") };
    let path = a.val("--card").map(str::to_string).or_else(stock_card).unwrap_or_else(|| die(&format!("no SD card on {}", oos_hal::target::ACTIVE.card_controller)));
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
        "install" => install(&a),
        "gel-install" => gel_install(&a),
        "kexec-check" => kexec_check(&a),
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

fn install(a: &Args) {
    let f = a.rest.get(1).unwrap_or_else(|| die("install FILE.oosupd [--execute] [--no-kexec | --new-dt]"));
    need_data(a);
    let p = read_file(f);
    let opt = InstallOpt { execute: a.flag("--execute"), downgrade: a.flag("--downgrade"), force: a.flag("--force") };
    let mut c = open_card(a, opt.execute);
    let test_card = a.val("--card").is_some_and(|c| c != "/dev/mmcblk0");
    let keys = keys(a, test_card);
    let sp = state_path(a);
    let announced = pkg::verify_header(&p, &keys).is_ok_and(|h| kexec::announced(&h.manifest));
    // a card file on the build machine cannot be started
    let by_kexec = !test_card && !a.flag("--no-kexec") && !a.flag("--new-dt") && announced;
    if !by_kexec {
        if !test_card && !a.flag("--no-kexec") && !a.flag("--new-dt") {
            println!("the package has no 'trial kexec' line (an older build): written directly, without the kexec trial start");
        }
        if a.flag("--new-dt") {
            println!("--new-dt: written directly; a kernel or device tree that hangs before rcS starts again at every power-on");
        }
        match update::install(&mut c, &sp, &p, &keys, &opt) {
            Ok(r) => r.lines.iter().for_each(|l| println!("{}", l)),
            Err(e) => die(&e),
        }
        return;
    }
    let booted = std::fs::read(kexec::BOOTED_DT).ok();
    let running = update::running_build();
    match update::kexec_stage(&mut c, &sp, &p, &keys, &opt, booted.as_deref(), running.as_deref()) {
        Err(e) if e.contains("device tree") => die(&format!(
            "{}. Try the image on a card you can take out; then --new-dt writes it directly, without the kexec trial start",
            e
        )),
        Err(e) => die(&e),
        Ok((r, None)) => r.lines.iter().for_each(|l| println!("{}", l)),
        Ok((r, Some(s))) => {
            r.lines.iter().for_each(|l| println!("{}", l));
            drop(p);
            start_kexec(&sp, s, kexec::Flags { downgrade: opt.downgrade, force: opt.force });
        }
    }
}

/// The oos-ui loop of rcS, started again after a kexec start that failed.
#[cfg(target_os = "linux")]
const UI_LOOP: &str = "while [ ! -e /tmp/oos-ui.off ]; do /bin/oos-ui $(cat /tmp/oos-ui.args) >> /tmp/oos-ui.log 2>&1; sleep 2; done";

/// What was stopped for the kexec start, to put back after a failure.
#[cfg(target_os = "linux")]
struct Quiet {
    links: Vec<String>,
    routes: Vec<String>,
    display: bool,
}

#[cfg(target_os = "linux")]
const DRM: &str = "/sys/bus/platform/drivers/rockchip-drm";

/// Devices that write to memory on their own stop before the kexec start; the kernel does not stop them all. The
/// Ethernet controller has no shutdown hook: its receive DMA would go on writing packets into buffers of this kernel,
/// which is memory of the next one by then (seen: the package arrived damaged). The display keeps its IOMMU on, which
/// the next kernel then cannot take over (seen: a dark screen).
#[cfg(target_os = "linux")]
fn quiesce() -> Quiet {
    let out = |args: &[&str]| {
        std::process::Command::new(args[0]).args(&args[1..]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    };
    let routes: Vec<String> = out(&["ip", "-4", "route", "show"]).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let mut links = Vec::new();
    if let Ok(d) = std::fs::read_dir("/sys/class/net") {
        for e in d.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let up = std::fs::read_to_string(format!("/sys/class/net/{}/flags", n))
                .ok()
                .and_then(|f| u32::from_str_radix(f.trim().trim_start_matches("0x"), 16).ok())
                .is_some_and(|f| f & 1 != 0);
            if n != "lo" && up && std::process::Command::new("ip").args(["link", "set", "dev", &n, "down"]).status().is_ok_and(|x| x.success()) {
                links.push(n);
            }
        }
    }
    // the display subsystem lets go of the VOPs and their IOMMUs while it still owns them
    let display = Path::new(&format!("{}/display-subsystem", DRM)).exists() && std::fs::write(format!("{}/unbind", DRM), "display-subsystem").is_ok();
    tell(&format!("stopped for the kexec start: links {:?}, display {}", links, if display { "unbound" } else { "left" }));
    Quiet { links, routes, display }
}

#[cfg(target_os = "linux")]
fn restore(q: &Quiet) {
    if q.display {
        let _ = std::fs::write(format!("{}/bind", DRM), "display-subsystem");
    }
    for n in &q.links {
        let _ = std::process::Command::new("ip").args(["link", "set", "dev", n, "up"]).status();
    }
    // a link that went down took its routes along
    for r in &q.routes {
        let mut a = vec!["route", "replace"];
        a.extend(r.split_whitespace());
        let _ = std::process::Command::new("ip").args(&a).status();
    }
}

/// Load the staged image and start it. Returns only after a failure, which leaves everything as before the install.
#[cfg(target_os = "linux")]
fn start_kexec(sp: &Path, s: update::Staged, f: kexec::Flags) {
    let short = update::short(&s.build).to_string();
    // stopped: None before oos-ui and the devices were stopped
    let undo = |why: &str, stopped: Option<&Quiet>| -> ! {
        let _ = kexec::unload();
        if stopped.is_some() {
            let _ = std::process::Command::new("mount").args(["-o", "remount,rw", "/oos"]).status();
        }
        let _ = update::kexec_clear(sp, &format!("kexec start of build {} failed ({}): nothing installed", short, why));
        if let Some(q) = stopped {
            restore(q);
            let _ = std::fs::remove_file("/tmp/oos-ui.off");
            let _ = std::process::Command::new("setsid")
                .args(["sh", "-c", UI_LOOP])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        notice(&format!("Update {}: the kexec start failed ({}); nothing installed, this build keeps running", short, why));
        std::process::exit(1)
    };
    let cmd = kexec::cmdline(&kexec::running_cmdline(), f);
    if let Err(e) = kexec::load(&s.kernel, &s.initrd, &cmd) {
        undo(&e, None);
    }
    drop(s);
    tell(&format!("build {} loaded; restarting into it by kexec, where it installs itself", short));
    // under the update lock: a write of the start-up watcher (a rollback, the persist arm) finishes first, then the
    // watcher is stopped, so no card write runs into the restart
    let lock = update::lock(sp).unwrap_or_else(|e| undo(&e, None));
    stop_watcher();
    stop_ui();
    let q = quiesce();
    // the lock file is open for writing: /oos can only go read only without it
    drop(lock);
    unsafe { libc::sync() };
    // the kernel does not sync or unmount on kexec
    if !std::process::Command::new("mount").args(["-o", "remount,ro", "/oos"]).status().is_ok_and(|x| x.success()) {
        println!("warning: /oos stays writable (busy); synced");
    }
    let e = kexec::start();
    undo(&e, Some(&q));
}

#[cfg(not(target_os = "linux"))]
fn start_kexec(sp: &Path, s: update::Staged, _: kexec::Flags) {
    let _ = update::kexec_clear(sp, "kexec runs on the unit only");
    die(&format!("kexec start of build {}: on the unit only", s.build));
}

/// The kernel takes the image for a kexec start (CPUs can go offline, the Image header is accepted).
#[cfg(target_os = "linux")]
fn kexec_check(a: &Args) {
    let f = a.rest.get(1).unwrap_or_else(|| die("kexec-check FILE.oosupd"));
    let p = read_file(f);
    let keys = update::trusted_keys();
    let (h, img) = pkg::verify(&p, &keys).unwrap_or_else(|e| die(&e));
    let (k, rd, dt) = kexec::parts(img).unwrap_or_else(|e| die(&e));
    let key = keys.iter().find(|k| pkg::key_id(k) == h.manifest.key).unwrap_or_else(|| die("no key"));
    let initrd = kexec::initrd(rd, &p, key, dt);
    let cmd = kexec::cmdline(&kexec::running_cmdline(), kexec::Flags::default());
    if kexec::loaded() {
        die("a kexec kernel is loaded already: left alone");
    }
    let t = std::time::Instant::now();
    kexec::load(k, &initrd, &cmd).unwrap_or_else(|e| die(&e));
    let ok = kexec::loaded();
    let ms = t.elapsed().as_millis();
    kexec::unload().unwrap_or_else(|e| die(&e));
    let tree = match std::fs::read(kexec::BOOTED_DT) {
        Ok(b) if b == dt => "the same as the running one",
        Ok(_) => "DIFFERS from the running one (install needs --new-dt)",
        Err(_) => "running one unknown",
    };
    println!(
        "build {}: kernel {} bytes, initramfs {} bytes loaded in {} ms ({}), unloaded again; announced 'trial kexec': {}; device tree {}",
        update::short(&h.manifest.build),
        k.len(),
        initrd.len(),
        ms,
        if ok { "kexec_loaded 1" } else { "kexec_loaded NOT 1" },
        kexec::announced(&h.manifest),
        tree
    );
    println!("command line: {}", cmd);
    if !ok || kexec::loaded() {
        die("unexpected kexec_loaded state");
    }
}

#[cfg(not(target_os = "linux"))]
fn kexec_check(_: &Args) {
    die("kexec-check runs on the unit only");
}

/// oos-ui off for the kexec start: no restart, no transfer of the acquisition hardware in flight.
#[cfg(target_os = "linux")]
fn stop_ui() {
    let _ = std::fs::write("/tmp/oos-ui.off", "");
    for i in 0..50 {
        let Some(pid) = ui_pid(None) else { return };
        unsafe { libc::kill(pid as i32, if i < 30 { libc::SIGTERM } else { libc::SIGKILL }) };
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The start-up watcher (`oos-update watch`) of this start: stopped before a kexec start (called with the lock held).
#[cfg(target_os = "linux")]
fn stop_watcher() {
    let me = std::process::id();
    let Ok(d) = std::fs::read_dir("/proc") else { return };
    for e in d.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        if pid == me {
            continue;
        }
        let cmd = std::fs::read(format!("/proc/{}/cmdline", pid)).unwrap_or_default();
        let args: Vec<&[u8]> = cmd.split(|&b| b == 0).collect();
        let is_update = args.first().is_some_and(|a0| a0.ends_with(b"oos-update"));
        if is_update && args.get(1) == Some(&&b"watch"[..]) {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
    }
}

/// rcS, in the start by kexec: this kernel reached rcS and found the card and /oos. Now the package is installed for
/// real. The kexec start is no trial start (its device tree and loader path are the old build's, the display comes up
/// from another state): the next power-on is the first one. False when nothing was installed.
fn finish_kexec(a: &Args, p: &Path, f: kexec::Flags) -> bool {
    let pk = read_file(kexec::PACKAGE);
    let build = update::running_build().unwrap_or_default();
    let short = update::short(&build).to_string();
    let _ = std::fs::write("/dev/tty1", format!("\nUpdate {}: started by kexec, installing it. Do not power off.\n", short));
    tell(&format!("build {} started by kexec: kernel, card and /oos work; installing it", short));
    // where the previous kernel put the initramfs, for a damaged package
    let at = |n: &str| std::fs::read(format!("/proc/device-tree/chosen/linux,{}", n)).ok().map(|b| b.iter().fold(0u64, |a, &x| a << 8 | x as u64));
    if let (Some(a0), Some(a1)) = (at("initrd-start"), at("initrd-end")) {
        tell(&format!("initramfs at {:#x}..{:#x}", a0, a1));
    }
    // the running build verified the package; this build may trust other keys (a key change by update)
    let mut keys = update::trusted_keys();
    if let Some(k) = std::fs::read_to_string(kexec::KEY_FILE).ok().and_then(|t| update::hash::unhex(t.trim())).and_then(|b| <[u8; 32]>::try_from(b).ok()) {
        keys.push(k);
    }
    let opt = InstallOpt { execute: true, downgrade: f.downgrade, force: f.force };
    let mut c = open_card(a, true);
    let done = match update::install(&mut c, p, &pk, &keys, &opt) {
        Ok(r) => {
            r.lines.iter().for_each(|l| println!("{}", l));
            if r.written {
                notice(&format!(
                    "Update {} installed after its kexec start. Power-cycle the scope: its first power-on is the trial start, it confirms itself about 20 s after the start-up",
                    short
                ));
            } else {
                let _ = update::kexec_clear(p, &format!("started build {} by kexec: nothing to install", short));
            }
            r.written
        }
        Err(e) => {
            // a write that began names this build on trial: the previous image goes back, so the watcher of this start
            // cannot confirm a damaged recovery
            let st = State::load(p).unwrap_or_default();
            let began = st.trial.is_some() && st.current.as_ref().is_some_and(|c| c.build.trim() == build.trim());
            let back = if began && st.previous.is_some() {
                match update::rollback(&mut c, p, "its install after the kexec start failed", true, None) {
                    Ok(_) => "the previous build was written back, it starts at the next power-on".to_string(),
                    Err(r) => format!("writing the previous build back FAILED too ({}): the next power-on starts stock", r),
                }
            } else {
                "the next power-on starts the previous build (or stock in one-shot mode)".to_string()
            };
            let m = format!("Update {} ran when started by kexec, but its install failed: {}; {}", short, e, back);
            let _ = update::kexec_clear(p, &m);
            notice(&m);
            // kept in RAM until the next start: what damaged it shows in the bytes
            tell(&format!("the package stays at {} until the next start", kexec::PACKAGE));
            return false;
        }
    };
    drop(pk);
    let _ = std::fs::remove_file(kexec::PACKAGE);
    done
}

/// The device tree section of the image the loader started, for a later kexec start (`kexec::BOOTED_DT`): in an
/// ordinary start `recovery` (nothing has written it yet), in a kexec start the tree the previous start passed on.
fn keep_booted_tree(a: &Args, kexec_start: bool) {
    let dt = if kexec_start {
        std::fs::read(kexec::DT_FILE).ok()
    } else {
        // the header page and the tree only: no hashing of the whole image at every start
        let mut c = open_card(a, false);
        let lba = card::Region::Recovery.lba();
        c.read(lba, 2048).ok().and_then(|h| kexec::tree_at(&h)).and_then(|(off, len)| {
            let first = off / card::SECTOR as usize;
            let skip = off % card::SECTOR as usize;
            let b = c.read(lba + first as u64, (skip + len).div_ceil(card::SECTOR as usize) * card::SECTOR as usize).ok()?;
            Some(b[skip..skip + len].to_vec())
        })
    };
    match dt {
        Some(d) => {
            let _ = std::fs::write(kexec::BOOTED_DT, d);
        }
        None => tell("the device tree of this start is unknown: an update needs a power-cycle first or --new-dt"),
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
    let word = kexec::flags(&kexec::running_cmdline());
    let kx = word.filter(|_| Path::new(kexec::PACKAGE).exists());
    keep_booted_tree(a, word.is_some());
    if !p.parent().is_some_and(|d| d.exists()) {
        if kx.is_some() {
            notice("Update started by kexec, but /oos is not there: not installed. The next power-on starts the previous build");
        }
        return; // no update state yet
    }
    match kx {
        // not the trial start of anything: a failed install, or the install, whose trial begins at the next power-on
        Some(f) => {
            finish_kexec(a, &p, f);
            return;
        }
        None if word.is_some() => {
            let m = "Update started by kexec, but its package did not arrive: not installed";
            let _ = update::kexec_clear(&p, m);
            notice(m);
            return;
        }
        None => match update::kexec_missed(&p, false) {
            Ok(Some(m)) => notice(&m),
            Ok(None) => {}
            Err(e) => tell(&e),
        },
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
    // a kexec start says nothing about a power-on (device tree, loader, display state): neither confirm nor roll back.
    // The next power-on is the trial start (the install armed OOS for it). Without an install, persist mode arms OOS
    // again (oos-oneshot disarmed it at rcS).
    if kexec::flags(&kexec::running_cmdline()).is_some() {
        tell("kexec start: not judged, the next power-on is the trial start");
        if st0.persist && st0.trial.is_none() {
            let mut c = open_card(a, true);
            match update::arm_checked(&mut c, &p) {
                Ok(true) => tell("persist mode: OOS armed for the next power-on"),
                Ok(false) => {}
                Err(e) => tell(&format!("persist mode: {}", e)),
            }
        }
        return;
    }
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
    // this tree's oos-update finishes a kexec start (update::kexec): the image says so in its package
    let p = pkg::build_with(&seed, build, &kernel, &img, &[kexec::LINE]).unwrap_or_else(|e| die(&e));
    pkg::verify(&p, &[pk]).unwrap_or_else(|e| die(&format!("self-check: {}", e)));
    std::fs::write(out, &p).unwrap_or_else(|e| die(&format!("{}: {}", out, e)));
    println!("{}: build {}, {} bytes, key {}", out, build, p.len(), hex(&pkg::key_id(&pk)));
}
