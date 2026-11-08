// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signed OOS updates with rollback (docs/update.md).
//!
//! - `pkg`: the signed package `.oosupd` (Ed25519 over a text header that names the image's SHA-256).
//! - `card`: the card layout, the image regions (`recovery` and the rollback slot in `backup`) and the boot magic.
//! - `first`: the first install from the running stock system, without opening the scope.
//! - `kexec`: the trial start of a new image by kexec, before anything is written.
//! - here: the state file (/oos/update/state) and the operations `install`, `rollback`, `boot_started`, `judge`,
//!   `confirm`.
//!
//! Safety net, from the bottom up:
//! 1. The stock loader checks every image's id; a damaged `recovery` falls back to the stock copy in `backup`.
//! 2. `oos-oneshot` (first line of rcS) puts `KRNL` back at every OOS start, so a start that hangs after it is
//!    followed by stock Android at the next power-on. Before it, the loader alone decides: a kernel or initramfs that
//!    hangs before rcS would start again at every power-on. That is why a new image is first started by kexec
//!    (`kexec_stage`), with `recovery` untouched, and installs itself only from that start.
//! 3. A new image runs on trial: the watcher (`oos-update watch`) confirms it once oos-ui ran 20 s and the
//!    start-up finished; a crash loop or a start-up that does not finish in 180 s writes the previous image back at
//!    once. A trial image that comes up a second time without having been confirmed rolls back at that start.
//! 4. Persistent mode (`mode persist`) arms OOS again after every confirmed start, so OOS stays the system without
//!    losing step 2.

pub mod card;
pub mod ed25519;
pub mod first;
pub mod hash;
pub mod kexec;
pub mod pkg;

pub use card::Card;
use card::{check_image, Region};
use hash::{hex, unhex};
use std::path::Path;

/// Default place of the state file (OOS data partition).
pub const STATE_PATH: &str = "/oos/update/state";
/// The keys every OOS build trusts (oos/keys/update.pub).
pub const TRUSTED_KEYS: &str = include_str!("../../../keys/update.pub");

pub fn trusted_keys() -> Vec<[u8; 32]> {
    pkg::parse_keys(TRUSTED_KEYS)
}

/// One image the state knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub build: String,
    pub sha256: [u8; 32],
    pub len: u64,
}

impl Entry {
    fn build_key(&self) -> &str {
        self.build.split_whitespace().next().unwrap_or("")
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The image in `recovery` (None: put there by hand, not by an update).
    pub current: Option<Entry>,
    /// The image in the rollback slot.
    pub previous: Option<Entry>,
    /// Some(n): `current` is on trial, started n times without being confirmed.
    pub trial: Option<u32>,
    /// Arm OOS again after every confirmed start.
    pub persist: bool,
    /// The build whose kexec start is under way (`kexec_stage`): nothing of it is on the card yet.
    pub kexec: Option<String>,
    /// Last thing that happened, one line.
    pub event: String,
}

impl State {
    pub fn parse(text: &str) -> State {
        let mut s = State::default();
        let mut cur = (String::new(), None, 0u64);
        let mut prev = (String::new(), None, 0u64);
        for l in text.lines() {
            let Some((k, v)) = l.split_once(' ') else { continue };
            let v = v.trim();
            match k {
                "current.build" => cur.0 = v.into(),
                "current.sha256" => cur.1 = unhex(v).and_then(|b| b.try_into().ok()),
                "current.len" => cur.2 = v.parse().unwrap_or(0),
                "previous.build" => prev.0 = v.into(),
                "previous.sha256" => prev.1 = unhex(v).and_then(|b| b.try_into().ok()),
                "previous.len" => prev.2 = v.parse().unwrap_or(0),
                "trial" => s.trial = v.parse().ok(),
                "mode" => s.persist = v == "persist",
                "kexec" if !v.is_empty() => s.kexec = Some(v.into()),
                "event" => s.event = v.into(),
                _ => {}
            }
        }
        let entry = |(build, sha, len): (String, Option<[u8; 32]>, u64)| {
            sha.filter(|_| len > 0).map(|sha256| Entry { build, sha256, len })
        };
        s.current = entry(cur);
        s.previous = entry(prev);
        s
    }

    pub fn text(&self) -> String {
        let mut t = String::from("# OOS update state (oos_hal::update, docs/update.md)\n");
        for (n, e) in [("current", &self.current), ("previous", &self.previous)] {
            if let Some(e) = e {
                t.push_str(&format!("{0}.build {1}\n{0}.sha256 {2}\n{0}.len {3}\n", n, e.build, hex(&e.sha256), e.len));
            }
        }
        if let Some(n) = self.trial {
            t.push_str(&format!("trial {}\n", n));
        }
        t.push_str(if self.persist { "mode persist\n" } else { "mode oneshot\n" });
        if let Some(b) = &self.kexec {
            t.push_str(&format!("kexec {}\n", b.replace('\n', " ")));
        }
        if !self.event.is_empty() {
            t.push_str(&format!("event {}\n", self.event.replace('\n', " ")));
        }
        t
    }

    pub fn load(path: &Path) -> std::io::Result<State> {
        match std::fs::read_to_string(path) {
            Ok(t) => Ok(State::parse(&t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e),
        }
    }

    /// Atomic: temp file, fsync, rename.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("new");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(self.text().as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        if let Some(d) = path.parent() {
            if let Ok(f) = std::fs::File::open(d) {
                let _ = f.sync_all();
            }
        }
        Ok(())
    }
}

/// Short form of a build id for messages.
pub fn short(build: &str) -> &str {
    build.split_whitespace().next().unwrap_or(build)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InstallOpt {
    pub execute: bool,
    /// Allow an older build than the current one.
    pub downgrade: bool,
    /// Install even when `recovery` already holds this image.
    pub force: bool,
}

/// What `install` did or (dry run) would do.
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    /// The image was written (OOS armed for the next power-on).
    pub written: bool,
    /// `recovery` already holds the package's image: nothing to do.
    pub unchanged: bool,
}

impl Report {
    fn say(&mut self, l: impl Into<String>) {
        self.lines.push(l.into());
    }
}

fn io(what: &str) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {}", what, e)
}

/// Exclusive lock on the update state: rcS, the watcher, the GUI job and a shell call never interleave their
/// read-modify-write of the state and the card (held until dropped).
pub struct Lock(#[allow(dead_code)] std::fs::File);

/// `State::event` while the rollback slot is being written.
const SLOT_WRITING: &str = "writing the rollback slot";

pub fn lock(state_path: &Path) -> Result<Lock, String> {
    if let Some(d) = state_path.parent() {
        std::fs::create_dir_all(d).map_err(io("creating the update state directory"))?;
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state_path.with_extension("lock"))
        .map_err(io("opening the update lock"))?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(format!("locking the update state: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(Lock(f))
}

/// The build id of the running image (/etc/oos-build, the same text `pack --build` puts into the package).
pub fn running_build() -> Option<String> {
    std::fs::read_to_string("/etc/oos-build").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Verify `package`, keep the image in `recovery` as the rollback image (when it is a confirmed or hand-installed
/// one), write the new image, read it back, mark it on trial and arm OOS for the next power-on.
pub fn install(card: &mut Card, state_path: &Path, package: &[u8], keys: &[[u8; 32]], opt: &InstallOpt) -> Result<Report, String> {
    let mut r = Report::default();
    let (h, img) = pkg::verify(package, keys)?;
    let m = &h.manifest;
    r.say(format!("package: build {}, kernel {}, {} bytes, signature ok (key {})", short(&m.build), m.kernel, m.size, hex(&m.key)));
    let info = check_image(img)?;
    if info.len != img.len() {
        return Err(format!("image is {} bytes, its sections end at {}", img.len(), info.len));
    }
    r.say(format!("image: '{}', loader id {} ok", info.name, &hex(&info.id)[..16]));
    card.check_layout()?;
    let _l = if opt.execute { Some(lock(state_path)?) } else { None };
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    let new = Entry { build: m.build.clone(), sha256: m.sha256, len: m.size };

    if let Some(cur) = st.current.as_ref().filter(|c| !c.build.starts_with("untracked")) {
        if cur.build_key() > new.build_key() && !opt.downgrade {
            return Err(format!("build {} is older than the installed {} (give --downgrade to install it anyway)", short(&new.build), short(&cur.build)));
        }
    }
    // what recovery holds now
    let cur_hdr = card.header(Region::Recovery).map_err(io("reading recovery"))?;
    let mut cur_img = None;
    if let Some(hdr) = &cur_hdr {
        let (b, sha) = card.read_image(Region::Recovery, hdr.len).map_err(io("reading recovery"))?;
        if sha == new.sha256 {
            if !opt.force {
                r.say(format!("recovery already holds build {}: nothing to do (--force writes it again)", short(&new.build)));
                r.unchanged = true;
                return Ok(r);
            }
            // the same image again: never copied into the slot (that would replace the last good one)
        } else {
            match check_image(&b) {
                Ok(_) => cur_img = Some((b, sha)),
                Err(e) => r.say(format!("recovery holds a damaged image ({}): not kept", e)),
            }
        }
    } else {
        r.say("recovery holds no OOS image: nothing to keep");
    }

    // keep it as the rollback image? Only a confirmed or hand-installed one: an image still on trial is not known
    // to work, the slot keeps the last good one then.
    let mut save: Option<(Vec<u8>, Entry)> = None;
    match (&cur_img, st.trial) {
        (Some((b, sha)), None) => {
            let e = Entry {
                build: match &st.current {
                    Some(c) if c.sha256 == *sha => c.build.clone(),
                    _ => format!("untracked-{}", &hex(sha)[..12]),
                },
                sha256: *sha,
                len: b.len() as u64,
            };
            if st.previous.as_ref().is_some_and(|p| p.sha256 == *sha) && slot_holds(card, &e)? {
                r.say(format!("rollback slot already holds build {}", short(&e.build)));
            } else {
                r.say(format!("keep build {} ({} bytes) in the {}", short(&e.build), e.len, Region::Slot.name()));
                save = Some((b.clone(), e));
            }
        }
        (Some(_), Some(n)) => r.say(format!(
            "the image in recovery is on trial ({} starts, not confirmed): not kept, the slot keeps {}",
            n,
            st.previous.as_ref().map_or("nothing".into(), |p| short(&p.build).to_string())
        )),
        (None, _) => {}
    }
    if save.is_some() {
        let slot_hdr = card.header(Region::Slot).map_err(io("reading the rollback slot"))?;
        // (a write of the slot cut by a power loss can leave data without its header: then it is ours to overwrite)
        if slot_hdr.is_none() && st.event != SLOT_WRITING && !card.region_empty(Region::Slot).map_err(io("reading the rollback slot"))? {
            return Err("the rollback slot (backup + 32 MiB) holds data that is not an OOS image: nothing written, inspect it first".into());
        }
    }
    if save.is_none() && st.previous.is_none() {
        r.say("WARNING: no rollback image; a failed start falls back to stock Android only");
    }
    r.say(format!("write build {} to recovery, on trial; arm OOS for the next power-on", short(&new.build)));
    if !opt.execute {
        r.say("dry run: nothing written (--execute)");
        return Ok(r);
    }

    // the state file must be writable before the card is touched (a full /oos would leave the new image untracked)
    st.save(state_path).map_err(io("writing the update state (nothing written to the card)"))?;
    if let Some((b, e)) = save {
        // while the slot is being written it holds no valid image
        st.previous = None;
        st.event = SLOT_WRITING.into();
        st.save(state_path).map_err(io("writing the update state"))?;
        card.write_image(Region::Slot, &b)?;
        st.previous = Some(e);
        st.save(state_path).map_err(io("writing the update state"))?;
        r.say("rollback slot written and read back");
    }
    let old = st.current.clone();
    // stock for the next power-on while recovery is not a valid image (`Card::disarm_oos`); armed again below
    card.disarm_oos()?;
    // recorded before the write: a failed or cut write leaves a state that names the new image on trial (a rollback or a
    // new install repairs it)
    st.current = Some(new.clone());
    st.trial = Some(0);
    st.event = format!("writing build {}", short(&new.build));
    st.save(state_path).map_err(io("writing the update state"))?;
    card.write_image(Region::Recovery, img)?;
    r.written = true;
    // this was the install a kexec start was for (or one that replaces it); a cut one keeps the mark, which the next
    // ordinary start reports
    st.kexec = None;
    st.event = format!(
        "installed {} over {}",
        short(&new.build),
        old.as_ref().map_or("an untracked image".into(), |o| short(&o.build).to_string())
    );
    st.save(state_path).map_err(io("writing the update state"))?;
    r.say("recovery written and read back");
    match card.arm_oos() {
        Ok(_) => r.say(format!("power-cycle the scope to start build {} (on trial)", short(&new.build))),
        Err(e) => r.say(format!("WARNING: {}; start OOS with oos-boot-oos-again", e)),
    }
    Ok(r)
}

/// What a kexec start needs: kernel, initramfs (with the package, the key that verified it and the running device
/// tree), the build.
#[derive(Debug)]
pub struct Staged {
    pub kernel: Vec<u8>,
    pub initrd: Vec<u8>,
    pub build: String,
}

/// The first half of an install by kexec start (module `kexec`): every check of `install` (nothing written), the device
/// tree, then the state names the build. The boot selection stays: `recovery` still holds the running build, so a
/// power-cycle after a hang starts that one (or stock in one-shot mode). The caller loads and starts the kernel; the
/// second half is `install` run by the started image itself (`oos-update boot`). None: nothing to do.
///
/// `booted_dt`: the device tree section of the image the loader started (`kexec::BOOTED_DT`, saved by `oos-update
/// boot`); without it only an image in `recovery` that is the running build (`running`) tells.
pub fn kexec_stage(
    card: &mut Card,
    state_path: &Path,
    package: &[u8],
    keys: &[[u8; 32]],
    opt: &InstallOpt,
    booted_dt: Option<&[u8]>,
    running: Option<&str>,
) -> Result<(Report, Option<Staged>), String> {
    let mut r = install(card, state_path, package, keys, &InstallOpt { execute: false, ..*opt })?;
    if r.unchanged {
        return Ok((r, None));
    }
    // (the dry run's own last line says nothing was written: true, but the kexec start follows)
    r.lines.pop();
    let (h, img) = pkg::verify(package, keys)?;
    let m = &h.manifest;
    if !kexec::announced(m) {
        return Err(format!("build {} does not finish a kexec start (its package has no 'trial kexec' line): install it with --no-kexec", short(&m.build)));
    }
    let (kernel, ramdisk, dt) = kexec::parts(img)?;
    // the tree the running kernel got from the loader, which the kexec start keeps
    let booted: Vec<u8> = match booted_dt {
        Some(b) => b.to_vec(),
        None => {
            let st = State::load(state_path).map_err(io("reading the update state"))?;
            let cur_is_running = st.current.as_ref().zip(running).is_some_and(|(c, run)| c.build.trim() == run.trim());
            if !cur_is_running {
                return Err(format!("the device tree this scope started with is not known ({} missing, recovery may hold another build): power-cycle once, or install with --new-dt", kexec::BOOTED_DT));
            }
            let cur = card.header(Region::Recovery).map_err(io("reading recovery"))?.ok_or("recovery holds no OOS image: no device tree to compare with")?;
            let (cur_img, _) = card.read_image(Region::Recovery, cur.len).map_err(io("reading recovery"))?;
            kexec::parts(&cur_img).map_err(|e| format!("recovery: {}: no device tree to compare with", e))?.2.to_vec()
        }
    };
    if booted != dt {
        return Err("the image brings another device tree than the one this scope started with: a kexec start keeps the running tree and cannot try it".into());
    }
    r.say("device tree: the same as the running one");
    let key = keys.iter().find(|k| pkg::key_id(k) == m.key).ok_or("no key for the package")?;
    let staged = Staged { kernel: kernel.to_vec(), initrd: kexec::initrd(ramdisk, package, key, &booted), build: m.build.clone() };
    if !opt.execute {
        r.say(format!("start build {} by kexec; it installs itself once it runs (dry run: --execute)", short(&staged.build)));
        return Ok((r, None));
    }
    let _l = lock(state_path)?;
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    st.kexec = Some(staged.build.clone());
    st.event = format!("starting build {} by kexec for its trial", short(&staged.build));
    st.save(state_path).map_err(io("writing the update state (nothing written to the card)"))?;
    r.say(format!("starting build {} by kexec: it installs itself once it runs", short(&staged.build)));
    Ok((r, Some(staged)))
}

/// The kexec start is over without the install it was for (the load or the start failed, or the started image could
/// not install itself): forget it, with `event` as the last event.
pub fn kexec_clear(state_path: &Path, event: &str) -> Result<(), String> {
    let _l = lock(state_path)?;
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    if st.kexec.take().is_none() {
        return Ok(());
    }
    st.event = event.into();
    st.save(state_path).map_err(io("writing the update state"))
}

/// Early in rcS of an ordinary start: a kexec start that was under way did not finish its install (the new kernel or its
/// start-up hung, or the power went off). Returns the message, once.
pub fn kexec_missed(state_path: &Path, kexec_start: bool) -> Result<Option<String>, String> {
    if kexec_start {
        return Ok(None);
    }
    let _l = lock(state_path)?;
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    let Some(b) = st.kexec.take() else { return Ok(None) };
    let msg = format!("Update {} did not finish when started by kexec (its kernel or start-up hung, or the power went off): not installed", short(&b));
    st.event = msg.clone();
    st.save(state_path).map_err(io("writing the update state"))?;
    Ok(Some(msg))
}

fn slot_holds(card: &mut Card, e: &Entry) -> Result<bool, String> {
    let (_, sha) = card.read_image(Region::Slot, e.len as usize).map_err(io("reading the rollback slot"))?;
    Ok(sha == e.sha256)
}

/// Write the rollback image back to `recovery` (checked against its SHA-256 first) and arm OOS. `expect`: only when
/// the state still names this image as current (the watcher's view at its start).
pub fn rollback(card: &mut Card, state_path: &Path, reason: &str, execute: bool, expect: Option<[u8; 32]>) -> Result<Report, String> {
    let mut r = Report::default();
    card.check_layout()?;
    let _l = if execute { Some(lock(state_path)?) } else { None };
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    if let Some(x) = expect {
        if st.current.as_ref().map(|c| c.sha256) != Some(x) {
            return Err("the update state changed meanwhile (another install or rollback): not rolled back".into());
        }
    }
    let prev = st.previous.clone().ok_or("no rollback image (nothing was kept by an update)")?;
    let (b, sha) = card.read_image(Region::Slot, prev.len as usize).map_err(io("reading the rollback slot"))?;
    if sha != prev.sha256 {
        return Err(format!("the rollback slot does not hold build {} any more (SHA-256 differs): nothing written", short(&prev.build)));
    }
    check_image(&b)?;
    // the card decides, not the state: a cut write may have left something else there
    let (_, in_recovery) = card.read_image(Region::Recovery, prev.len as usize).map_err(io("reading recovery"))?;
    if in_recovery == prev.sha256 {
        r.say(format!("recovery already holds build {}", short(&prev.build)));
        if execute && (st.trial.is_some() || st.current.as_ref().map(|c| c.sha256) != Some(prev.sha256)) {
            st.current = Some(prev.clone());
            st.trial = None;
            st.save(state_path).map_err(io("writing the update state"))?;
        }
        return Ok(r);
    }
    let from = st.current.as_ref().map_or("an untracked image".to_string(), |c| short(&c.build).to_string());
    r.say(format!("write build {} from the rollback slot back to recovery (replaces {})", short(&prev.build), from));
    if !execute {
        r.say("dry run: nothing written (--execute)");
        return Ok(r);
    }
    // the state keeps naming the image and its trial until the write is done: a cut write stays a rollback to repeat
    st.event = format!("rolling back from {} to {}", from, short(&prev.build));
    st.save(state_path).map_err(io("writing the update state (nothing written to the card)"))?;
    card.disarm_oos()?;
    card.write_image(Region::Recovery, &b)?;
    r.written = true;
    st.current = Some(prev.clone());
    st.trial = None;
    st.event = format!("rolled back from {} to {}: {}", from, short(&prev.build), reason);
    st.save(state_path).map_err(io("writing the update state"))?;
    r.say("recovery written and read back");
    match card.arm_oos() {
        Ok(_) => r.say(format!("power-cycle the scope to start build {}", short(&prev.build))),
        Err(e) => r.say(format!("WARNING: {}", e)),
    }
    Ok(r)
}

/// Persist mode after a good start: arm OOS, under the lock and only with a valid image in `recovery` and no trial. Ok(true)
/// when armed now.
pub fn arm_checked(card: &mut Card, state_path: &Path) -> Result<bool, String> {
    let _l = lock(state_path)?;
    let st = State::load(state_path).map_err(io("reading the update state"))?;
    if st.trial.is_some() {
        return Err("an image is on trial: not armed".into());
    }
    let h = card.header(Region::Recovery).map_err(io("reading recovery"))?.ok_or("no image in recovery: not armed")?;
    let (b, _) = card.read_image(Region::Recovery, h.len).map_err(io("reading recovery"))?;
    check_image(&b)?;
    card.arm_oos()
}

/// Selects OOS for the next power-on by hand (`oos-boot-oos-again`): only an image in `recovery` the loader takes, and
/// never while an install or rollback runs. Unlike `arm_checked` it arms a trial image too: its second start is what
/// rolls it back.
pub fn arm_by_hand(card: &mut Card, state_path: &Path) -> Result<bool, String> {
    let _l = lock(state_path)?;
    let h = card.header(Region::Recovery).map_err(io("reading recovery"))?.ok_or("no image in recovery: not armed")?;
    let (b, _) = card.read_image(Region::Recovery, h.len).map_err(io("reading recovery"))?;
    check_image(&b).map_err(|e| format!("recovery: {}: not armed", e))?;
    card.arm_oos()
}

/// An install or rollback holds the update lock right now (also one of another process: the start-up watcher).
pub fn busy(state_path: &Path) -> bool {
    let Ok(f) = std::fs::OpenOptions::new().write(true).open(state_path.with_extension("lock")) else { return false };
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return true;
        }
    }
    drop(f);
    false
}

/// What `boot_started` found.
#[derive(Clone, Debug, PartialEq)]
pub enum Boot {
    /// no image on trial
    NoTrial,
    /// the trial image started, this many times now
    Trial(u32),
    /// the trial image started a second time without confirmation: the caller rolls back
    Rollback,
    /// the running image is not the one on trial (recovery written by hand): the trial was dropped
    Dropped(String),
}

/// Early in rcS: count this start of a trial image. `running`: the build id of the running image; the count only
/// applies when it is the image on trial.
pub fn boot_started(state_path: &Path, running: Option<&str>) -> Result<Boot, String> {
    let _l = lock(state_path)?;
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    let Some(n) = st.trial else { return Ok(Boot::NoTrial) };
    let cur = st.current.as_ref().map(|c| c.build.clone()).unwrap_or_default();
    if let Some(run) = running {
        if run.trim() != cur.trim() {
            let msg = format!("running build {} is not the one on trial ({}): trial dropped, recovery treated as hand-installed", short(run), short(&cur));
            st.trial = None;
            st.current = None;
            st.event = msg.clone();
            st.save(state_path).map_err(io("writing the update state"))?;
            return Ok(Boot::Dropped(msg));
        }
    }
    st.trial = Some(n + 1);
    st.save(state_path).map_err(io("writing the update state"))?;
    Ok(if n + 1 >= 2 && st.previous.is_some() { Boot::Rollback } else { Boot::Trial(n + 1) })
}

/// The image passed its trial. Returns true when it was on trial. `expect`: only when the state still names this
/// image as current.
pub fn confirm(state_path: &Path, expect: Option<[u8; 32]>) -> Result<bool, String> {
    confirm_running(state_path, expect, None)
}

/// `confirm`, and with `running` (the build of the running image) only if that is the build on trial: an image that was
/// written but never started is not confirmed.
pub fn confirm_running(state_path: &Path, expect: Option<[u8; 32]>, running: Option<&str>) -> Result<bool, String> {
    let _l = lock(state_path)?;
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    if st.trial.is_none() {
        return Ok(false);
    }
    if let Some(run) = running {
        let cur = st.current.as_ref().map(|c| c.build.trim().to_string()).unwrap_or_default();
        if run.trim() != cur {
            return Err(format!("the running build {} is not the one on trial ({}): it has not started yet", run.trim(), cur));
        }
    }
    if let Some(x) = expect {
        if st.current.as_ref().map(|c| c.sha256) != Some(x) {
            return Ok(false);
        }
    }
    st.trial = None;
    st.event = format!("confirmed {}", st.current.as_ref().map_or("?", |c| short(&c.build)));
    st.save(state_path).map_err(io("writing the update state"))?;
    Ok(true)
}

/// Boot mode (persist = arm OOS after every confirmed start).
pub fn set_mode(state_path: &Path, persist: bool) -> Result<(), String> {
    let _l = lock(state_path)?;
    let mut st = State::load(state_path).map_err(io("reading the update state"))?;
    st.persist = persist;
    st.save(state_path).map_err(io("writing the update state"))
}

/// What the watcher sees after `elapsed` seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Health {
    pub elapsed: f64,
    /// Times oos-ui was (re)started after the first.
    pub ui_restarts: u32,
    /// Seconds the current oos-ui process has been running (0 = none).
    pub ui_up: f64,
    /// The start-up sequence wrote `end` or `abort` (or there is none: no acquisition hardware in this image).
    pub startup_done: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Wait,
    Good,
    Bad(String),
}

pub const UI_STABLE_S: f64 = 20.0;
pub const UI_RESTARTS_BAD: u32 = 3;
pub const START_TIMEOUT_S: f64 = 180.0;

/// The trial rule: the image's software must run. oos-ui up 20 s without crashing and the start-up finished (`end`
/// or `abort`) is good at once; a start-up that hangs while oos-ui runs stable is good at the timeout too (the
/// screen names the stage, Utility > Roll back update is there): hardware trouble must not roll a good build back.
/// Bad: 3 oos-ui crashes, or no stable oos-ui by the timeout.
pub fn judge(h: &Health) -> Verdict {
    if h.ui_restarts >= UI_RESTARTS_BAD {
        return Verdict::Bad(format!("oos-ui crashed {} times", h.ui_restarts));
    }
    if h.ui_up >= UI_STABLE_S && (h.startup_done || h.elapsed >= START_TIMEOUT_S) {
        return Verdict::Good;
    }
    if h.elapsed >= START_TIMEOUT_S {
        return Verdict::Bad(format!("oos-ui did not run stable within {} s", START_TIMEOUT_S));
    }
    Verdict::Wait
}

#[cfg(test)]
mod tests;
