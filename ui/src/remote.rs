// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The telnet shell of System > Network > Remote services. rcS starts busybox `telnetd` at power-on unless
//! `remote.telnet = off`; a change while oos-ui runs ends it or starts it here. SSH (`crate::ssh`) is the
//! authenticated alternative; the host name is `crate::hostname`.
//!
//! Also what a change of Remote services > Allowed (`net.allow`, ui/remote.rs) runs on the scope: the firewall built
//! again from the new list, then SSH as a start with that list leaves it (`allow_steps`, docs/network.md 6).

use std::fs::OpenOptions;
use std::process::Stdio;

/// The firewall script of the image (rootfs/initramfs/bin/oos-firewall); oos-ui runs it only where it exists.
pub const FIREWALL: &str = "/bin/oos-firewall";
/// Where rcS logs the firewall too.
const NET_LOG: &str = "/tmp/oos-net.log";

/// One program a change of Allowed runs: its argument, and the log its messages go to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub prog: &'static str,
    pub arg: &'static str,
    pub log: &'static str,
}

/// What runs the steps: the scripts on the scope (`script_runner`), a stand-in in a test. Err says what failed.
pub type Runner = std::sync::Arc<dyn Fn(&Step) -> Result<(), String> + Send + Sync>;

/// After `net.allow` changed, in this order: the firewall from the new list (`oos-firewall reload`: the table removed
/// without a list, the watcher of `onlink6` stopped and started as rcS starts it), then SSH, when its switch is on, as
/// a start with the list leaves it: with a list listening (`restart`: sessions stay), without one stopped with its
/// sessions and started again, which leaves it at `off: needs net.allow`. With the switch off SSH is not running.
pub fn allow_steps(ssh_on: bool, list: bool) -> Vec<Step> {
    let mut v = vec![Step { prog: FIREWALL, arg: "reload", log: NET_LOG }];
    let ssh = |arg| Step { prog: crate::ssh::SCRIPT, arg, log: crate::ssh::LOG };
    match (ssh_on, list) {
        (false, _) => {}
        (true, true) => v.push(ssh("restart")),
        (true, false) => v.extend([ssh("stop"), ssh("start")]),
    }
    v
}

/// The scripts themselves, as `ssh::apply` runs oos-ssh: no input, the messages appended to the step's log. Err names
/// the step and the log, which has the script's reason.
pub fn script_runner() -> Runner {
    std::sync::Arc::new(|s: &Step| {
        let mut c = std::process::Command::new(s.prog);
        c.arg(s.arg).stdin(Stdio::null()).stdout(Stdio::null());
        match OpenOptions::new().create(true).append(true).open(s.log) {
            Ok(f) => c.stderr(f),
            Err(_) => c.stderr(Stdio::null()),
        };
        let name = s.prog.rsplit('/').next().unwrap_or(s.prog);
        match c.status() {
            Ok(st) if st.success() => Ok(()),
            Ok(st) => Err(format!("{} {} failed ({}), see {}", name, s.arg, st, s.log)),
            Err(e) => Err(format!("{} {}: {}", name, s.arg, e)),
        }
    })
}

/// Running `telnetd` processes, by the name in /proc (none on a host without /proc).
fn telnetd_pids() -> Vec<i32> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()))
        .filter(|pid| std::fs::read_to_string(format!("/proc/{}/comm", pid)).is_ok_and(|c| c.trim() == "telnetd"))
        .collect()
}

/// The shell on port 23 on or off. Ending `telnetd` also ends the sessions it serves.
pub fn telnet(on: bool) {
    let pids = telnetd_pids();
    if on && pids.is_empty() {
        // busybox telnetd puts itself in the background: the process started here returns at once
        let r = std::process::Command::new("telnetd")
            .args(["-l", "/bin/sh"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match r {
            Ok(_) => eprintln!("oos-ui: telnet on"),
            Err(e) => eprintln!("oos-ui: telnetd: {}", e),
        }
    } else if !on && !pids.is_empty() {
        for p in pids {
            // SAFETY: a signal to a process id, no memory involved
            #[cfg(unix)]
            unsafe { libc::kill(p, libc::SIGTERM) };
            // no /proc on Windows, so no pids reach here; nothing to signal
            #[cfg(not(unix))]
            let _ = p;
        }
        eprintln!("oos-ui: telnet off");
    }
}
