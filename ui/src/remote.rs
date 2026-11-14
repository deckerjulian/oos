// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The telnet shell of System > Network > Remote services, and the kernel's host name. rcS starts busybox `telnetd`
//! at power-on unless `remote.telnet = off`; a change while oos-ui runs ends it or starts it here. (Web, SCPI, VXI-11
//! and mDNS are oos-ui's own: `main.rs` opens and closes them.)

const HOSTNAME: &str = "/proc/sys/kernel/hostname";

/// The kernel's host name, the scope's name on the network (System > Network > Remote services > Name). rcS sets it at
/// power-on from `net.name` and the DHCP clients send it (`-x hostname:`); a new name reaches the router with the next
/// lease.
pub fn hostname(name: &str) {
    if std::fs::read_to_string(HOSTNAME).is_ok_and(|n| n.trim() == name) {
        return;
    }
    match std::fs::write(HOSTNAME, name) {
        Ok(()) => eprintln!("oos-ui: host name {}", name),
        Err(e) => eprintln!("oos-ui: host name {}: {}", name, e),
    }
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
