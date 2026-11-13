// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The kernel's host name, the scope's name on the network (System > Network > Remote services > Name). Upstream keeps
//! this in `remote.rs` next to telnet; this fork has no telnet and no `remote.rs` (docs/fork/sync-checklist.md 3).

const HOSTNAME: &str = "/proc/sys/kernel/hostname";

/// rcS sets it at power-on from `net.name` and the DHCP clients send it (`-x hostname:`); a new name reaches the router
/// with the next lease.
pub fn set(name: &str) {
    if std::fs::read_to_string(HOSTNAME).is_ok_and(|n| n.trim() == name) {
        return;
    }
    match std::fs::write(HOSTNAME, name) {
        Ok(()) => eprintln!("oos-ui: host name {}", name),
        Err(e) => eprintln!("oos-ui: host name {}: {}", name, e),
    }
}
