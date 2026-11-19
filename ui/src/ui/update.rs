// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signed updates in the GUI (docs/update.md): System > Update shows the update state, System > Roll back update
//! writes the kept image back (two taps within 5 s), System > Start OOS again selects OOS for the next power-on once
//! more (two taps, `oos-update arm`, what `oos-boot-oos-again` runs), the web page /update takes a package (http.rs).
//! The work is done by /bin/oos-update in a child process (the card writes take a few seconds); its result comes as a
//! dialog.
//! A notice from the start-up watcher (/tmp/oos-update.notice: rolled back after a failed start) is shown once.

use super::*;
use oos_hal::update::{self, pkg, State};
use std::path::Path;

const BIN: &str = "/bin/oos-update";
const NOTICE: &str = "/tmp/oos-update.notice";
const UPLOAD: &str = "/tmp/oos-update.upload";
const JOB_LOG: &str = "/tmp/oos-update.job";

#[derive(Default)]
pub(super) struct UpdateUi {
    job: Option<(std::process::Child, String)>,
    /// first tap on Roll back: the second must come before this time
    arm_until: f64,
    /// the same for Start OOS again
    again_until: f64,
    next_check: f64,
    notice_shown: String,
    /// output of the last job (web page)
    pub(super) last: String,
}

fn state() -> Option<State> {
    Path::new(update::STATE_PATH).parent().filter(|d| d.exists())?;
    State::load(Path::new(update::STATE_PATH)).ok()
}

/// What a tap of System > Start OOS again does.
#[derive(Debug, PartialEq)]
enum Again {
    /// an update job runs: refused
    Busy,
    /// OOS starts at every power-on already: refused
    Persist,
    /// the first tap, or one after the 5 s ran out: asks
    Ask,
    /// the second tap within 5 s: `oos-update` with these arguments (what `oos-boot-oos-again` runs)
    Run(&'static [&'static str]),
}

/// The step of a tap at `now`, with the second tap open until `until` (0: no first tap yet).
fn again_step(busy: bool, persist: bool, now: f64, until: f64) -> Again {
    if busy {
        Again::Busy
    } else if persist {
        Again::Persist
    } else if until == 0.0 || now > until {
        Again::Ask
    } else {
        Again::Run(&["arm"])
    }
}

impl Ui {
    /// Dialog lines of System > Update.
    pub(super) fn update_lines(&self) -> Vec<String> {
        let mut d = vec!["Update".to_string()];
        if let Ok(b) = std::fs::read_to_string("/etc/oos-build") {
            d.push(format!("Running {}", update::short(b.trim())));
        }
        match state() {
            None => d.push("No update installed yet".into()),
            Some(st) => {
                let b = |e: &Option<update::Entry>| {
                    e.as_ref().map_or("-".to_string(), |e| update::short(&e.build).to_string())
                };
                d.push(format!("Recovery {}{}", b(&st.current), if st.trial.is_some() { " (on trial)" } else { "" }));
                d.push(format!("Rollback image {}", b(&st.previous)));
                d.push(format!("Boot mode {}", if st.persist { "persist" } else { "one-shot" }));
                if !st.event.is_empty() {
                    // the event may quote a build name from a package: any characters
                    let mut e: String = st.event.chars().take(43).collect();
                    if e.len() < st.event.len() {
                        e.push('…');
                    }
                    d.push(e);
                }
            }
        }
        if self.upd.job.is_some() {
            d.push("Working ...".into());
        }
        d.extend(["".into(), "Install: web page :8080/update".into(), "Tap to close".into()]);
        d
    }

    /// What the web page /update shows (http.rs): the state as `oos-update status --quick` gives it, and the result
    /// of the last upload.
    pub fn update_state(&self) -> (String, &str) {
        let status = std::process::Command::new(BIN)
            .args(["status", "--quick"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
            .unwrap_or_else(|e| format!("{}: {}", BIN, e));
        (status, &self.upd.last)
    }

    /// System > Boot mode.
    pub(super) fn boot_mode(&self) -> &'static str {
        if state().is_some_and(|s| s.persist) {
            "Persist"
        } else {
            "One-shot"
        }
    }

    /// Persist keeps OOS as the system from now on, One-shot makes the next power-on start stock.
    pub(super) fn boot_mode_toggle(&mut self) {
        if self.update_busy() {
            return self.toast("An update job is running");
        }
        let to = if state().is_some_and(|s| s.persist) { "oneshot" } else { "persist" };
        self.update_spawn(&["mode", to], "Boot mode");
    }

    /// System > Back to stock: says what the next power-on really does.
    pub(super) fn stock_tap(&mut self) {
        if state().is_some_and(|s| s.persist) {
            self.toast("Boot mode is Persist: set it to One-shot, then power-cycle");
        } else {
            self.toast("Power-cycle the scope: stock boots next");
        }
    }

    /// System > Start OOS again: with Boot mode One-shot, the next power-on starts OOS once more instead of stock.
    /// First tap asks, the second (within 5 s) runs `oos-update arm`, the command `oos-boot-oos-again` runs: it checks
    /// that `recovery` holds an image the loader takes and only then writes the magic of `boot` (docs/boot.md). The
    /// dialog of the job says what it did.
    pub(super) fn boot_again_tap(&mut self) {
        let persist = state().is_some_and(|s| s.persist);
        match again_step(self.update_busy(), persist, self.now, self.upd.again_until) {
            Again::Busy => self.toast("An update job is running"),
            Again::Persist => self.toast("Boot mode is Persist: OOS starts at every power-on"),
            Again::Ask => {
                self.upd.again_until = self.now + 5.0;
                self.toast("Tap again: OOS starts at the next power-on, once more");
            }
            Again::Run(args) => {
                self.upd.again_until = 0.0;
                self.update_spawn(args, "Start OOS again");
            }
        }
    }

    /// System > Roll back update: first tap asks, second (within 5 s) runs it.
    pub(super) fn update_rollback_tap(&mut self) {
        if self.update_busy() {
            return self.toast("An update job is running");
        }
        let Some(st) = state() else { return self.toast("No rollback image (no update installed)") };
        let Some(prev) = st.previous else { return self.toast("No rollback image kept") };
        if st.current.as_ref().is_some_and(|c| c.sha256 == prev.sha256) {
            return self.toast("Recovery already holds the rollback image");
        }
        if self.now > self.upd.arm_until {
            self.upd.arm_until = self.now + 5.0;
            return self.toast(&format!("Tap again to roll back to {}", update::short(&prev.build)));
        }
        self.upd.arm_until = 0.0;
        self.update_spawn(&["rollback", "--execute"], "Roll back");
    }

    /// A package from the web page: checked here (signature, image hash) before the child process writes the card.
    pub fn update_upload(&mut self, body: &[u8]) -> Result<String, String> {
        if self.update_busy() {
            return Err("an update job is running".into());
        }
        let (h, _) = pkg::verify(body, &update::trusted_keys())?;
        std::fs::write(UPLOAD, body).map_err(|e| format!("{}: {}", UPLOAD, e))?;
        let b = update::short(&h.manifest.build).to_string();
        if !self.update_spawn(&["install", UPLOAD, "--execute"], "Install") {
            return Err(format!("{} could not be started, nothing installed", BIN));
        }
        Ok(if update::kexec::announced(&h.manifest) {
            format!("signature ok, build {}: the scope restarts into it by kexec and installs it from there, unless a check refuses it (about a minute; System > Update shows the result). Then power-cycle the scope: that start is its trial", b)
        } else {
            format!("signature ok, installing build {} (the result shows on the scope and on this page)", b)
        })
    }

    /// A job of ours is running, or another process (the start-up watcher) holds the update lock: the card is being
    /// written, so no second job and no power-off from here.
    pub(super) fn update_busy(&self) -> bool {
        self.upd.job.is_some() || update::busy(std::path::Path::new(update::STATE_PATH))
    }

    /// Starts `oos-update`; false when that failed (toast).
    fn update_spawn(&mut self, args: &[&str], what: &str) -> bool {
        let log = match std::fs::File::create(JOB_LOG) {
            Ok(f) => f,
            Err(e) => {
                self.toast(&format!("{}: {}", JOB_LOG, e));
                return false;
            }
        };
        let err = log.try_clone();
        let mut c = std::process::Command::new(BIN);
        c.args(args).stdout(log).stdin(std::process::Stdio::null());
        if let Ok(e) = err {
            c.stderr(e);
        }
        match c.spawn() {
            Ok(ch) => {
                self.upd.job = Some((ch, what.into()));
                self.toast(&format!("{} ...", what));
                true
            }
            Err(e) => {
                self.toast(&format!("{}: {}", BIN, e));
                false
            }
        }
    }

    /// From `step`: the running job, and the watcher's notice (twice a second).
    pub(super) fn update_poll(&mut self) {
        if self.now < self.upd.next_check {
            return;
        }
        self.upd.next_check = self.now + 0.5;
        if let Some((ch, what)) = &mut self.upd.job {
            if let Ok(Some(st)) = ch.try_wait() {
                let what = what.clone();
                self.upd.job = None;
                let out = std::fs::read_to_string(JOB_LOG).unwrap_or_default();
                self.upd.last = out.clone();
                let mut d = vec![format!("{} {}", what, if st.success() { "done" } else { "FAILED" })];
                d.extend(wrap_lines(&out, 44).into_iter().rev().take(9).collect::<Vec<_>>().into_iter().rev());
                d.push("Tap to close".into());
                self.dialog = Some(d);
                if what == "Install" {
                    let _ = std::fs::remove_file(UPLOAD);
                }
            }
        }
        // (not in unit tests: a notice file on the build machine must not open dialogs there)
        if let Ok(n) =
            std::fs::read_to_string(NOTICE).map_err(|_| ()).and_then(|n| if cfg!(test) { Err(()) } else { Ok(n) })
        {
            let n = n.trim().to_string();
            if !n.is_empty() && n != self.upd.notice_shown && self.dialog.is_none() && !self.start_up_active() {
                self.upd.notice_shown = n.clone();
                let mut d = vec!["Update".to_string()];
                d.extend(wrap_lines(&n, 44));
                d.push("Tap to close".into());
                self.dialog = Some(d);
            }
        }
    }
}

/// Text wrapped at word boundaries to `w` characters.
fn wrap_lines(t: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in t.lines().filter(|l| !l.trim().is_empty()) {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > w {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_words() {
        let l =
            wrap_lines("Update 20261105-170000 failed its first start (oos-ui crashed 3 times). Build x restored.", 30);
        assert!(l.iter().all(|s| s.chars().count() <= 30), "{:?}", l);
        assert_eq!(
            l.join(" "),
            "Update 20261105-170000 failed its first start (oos-ui crashed 3 times). Build x restored."
        );
    }

    // Break it catches: Start OOS again writing the card on a single tap, or standing away from Boot mode
    #[test]
    fn start_oos_again_asks_first_and_stands_under_boot_mode() {
        let mut u = crate::ui::testkit::demo_ui();
        let items = u.items(MenuId::Utility);
        let i = items.iter().position(|&it| it == Item::BootMode).unwrap();
        assert_eq!(items.get(i + 1), Some(&Item::BootAgain));
        assert!(matches!(Ui::kind(Item::BootAgain), Kind::Action));
        u.boot_again_tap();
        assert!(u.upd.job.is_none(), "one tap writes nothing");
        assert!(u.toast.as_ref().unwrap().0.starts_with("Tap again"), "{:?}", u.toast);
        // the second tap only after the first has run out: asks again
        u.now += 6.0;
        u.boot_again_tap();
        assert!(u.upd.job.is_none() && u.toast.as_ref().unwrap().0.starts_with("Tap again"));
    }

    #[test]
    fn start_oos_again_runs_arm_on_the_second_tap_and_refuses_when_it_must() {
        assert_eq!(again_step(false, false, 3.0, 0.0), Again::Ask, "no first tap yet");
        assert_eq!(again_step(false, false, 3.0, 5.0), Again::Run(&["arm"]), "the second tap within 5 s");
        assert_eq!(again_step(false, false, 5.0, 5.0), Again::Run(&["arm"]), "at the last moment");
        assert_eq!(again_step(false, false, 5.1, 5.0), Again::Ask, "after the 5 s");
        assert_eq!(again_step(true, false, 3.0, 5.0), Again::Busy, "an update job runs");
        assert_eq!(again_step(false, true, 3.0, 5.0), Again::Persist, "Boot mode Persist");
        assert_eq!(again_step(true, true, 3.0, 5.0), Again::Busy, "a running job first");
        // what the shell's `oos-boot-oos-again` runs, so the two do the same
        let script = include_str!("../../../rootfs/initramfs/bin/oos-boot-oos-again");
        assert!(script.lines().any(|l| l.trim() == "exec oos-update arm"), "{script}");
    }

    #[test]
    fn upload_refuses_unsigned_packages() {
        let mut u = crate::ui::testkit::demo_ui();
        let e = u.update_upload(b"OOS-UPDATE 1\ntarget dho900-recovery\n\nxx").unwrap_err();
        assert!(e.contains("signature") || e.contains("header"), "{}", e);
        assert!(u.upd.job.is_none());
    }
}
