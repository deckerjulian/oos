// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The running processes, as /proc lists them (Linux; on a host without /proc there is no list).

/// The pids in /proc that `pred` accepts; None when /proc cannot be read.
pub fn pids_where(mut pred: impl FnMut(u32) -> bool) -> Option<Vec<u32>> {
    let d = std::fs::read_dir("/proc").ok()?;
    Some(d.flatten().filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok()).filter(|&p| pred(p)).collect())
}

/// The process `pid` has the name `name` (/proc/<pid>/comm).
pub fn comm_is(pid: u32, name: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{}/comm", pid)).is_ok_and(|c| c.trim() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a filter that drops numbers or lets the other entries (self, sys, ...) through
    #[test]
    fn this_process_is_listed_where_proc_exists() {
        let me = std::process::id();
        if let Some(p) = pids_where(|p| p == me) {
            assert_eq!(p, vec![me]);
        }
        assert!(!comm_is(u32::MAX, "oos-ui"));
    }
}
