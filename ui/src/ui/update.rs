// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signed updates in the GUI (docs/update.md): System > Update shows the update state, System > Roll back update
//! writes the kept image back (two taps within 5 s), the web page /update takes a package (http.rs). The work is
//! done by /bin/oos-update in a child process (the card writes take a few seconds); its result comes as a dialog.
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
    next_check: f64,
    notice_shown: String,
    /// output of the last job (web page)
    pub(super) last: String,
}

fn state() -> Option<State> {
    Path::new(update::STATE_PATH).parent().filter(|d| d.exists())?;
    State::load(Path::new(update::STATE_PATH)).ok()
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
                let b = |e: &Option<update::Entry>| e.as_ref().map_or("-".to_string(), |e| update::short(&e.build).to_string());
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

    /// The web page /update.
    pub fn update_page(&self) -> String {
        page(&self.upd.last)
    }

    /// System > Boot mode.
    pub(super) fn boot_mode(&self) -> &'static str {
        if state().is_some_and(|s| s.persist) { "Persist" } else { "One-shot" }
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
        if let Ok(n) = std::fs::read_to_string(NOTICE).map_err(|_| ()).and_then(|n| if cfg!(test) { Err(()) } else { Ok(n) }) {
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

/// The web page /update: state and an upload form.
fn page(last: &str) -> String {
    let status = std::process::Command::new(BIN)
        .args(["status", "--quick"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
        .unwrap_or_else(|e| format!("{}: {}", BIN, e));
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>OOS update</title>
<style>{}</style></head>
<body><header><b>OOS update</b><nav><a href="/">Remote</a></nav></header>
<pre>{}</pre>
<p>Package (<code>.oosupd</code>, signed): <input type="file" id="f" accept=".oosupd"> <button onclick="up()">Install</button></p>
<pre id="r">{}</pre>
<p>The new build starts on trial at the next power-on; a failed start writes the previous build back.
Command line: <code>curl --data-binary @oos-dev.oosupd http://&lt;scope&gt;:8080/update</code></p>
<script>
function up(){{const f=document.getElementById('f').files[0];if(!f)return;const r=document.getElementById('r');
r.textContent='uploading '+f.size+' bytes ...';
fetch('/update',{{method:'POST',body:f}}).then(x=>x.text()).then(t=>{{r.textContent=t;setTimeout(()=>location.reload(),15000)}})}}
</script></body></html>"#,
        crate::http::STYLE,
        esc(&status),
        esc(last)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_words() {
        let l = wrap_lines("Update 20261105-170000 failed its first start (oos-ui crashed 3 times). Build x restored.", 30);
        assert!(l.iter().all(|s| s.chars().count() <= 30), "{:?}", l);
        assert_eq!(l.join(" "), "Update 20261105-170000 failed its first start (oos-ui crashed 3 times). Build x restored.");
    }

    #[test]
    fn upload_refuses_unsigned_packages() {
        let mut u = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let e = u.update_upload(b"OOS-UPDATE 1\ntarget dho900-recovery\n\nxx").unwrap_err();
        assert!(e.contains("signature") || e.contains("header"), "{}", e);
        assert!(u.upd.job.is_none());
    }
}
