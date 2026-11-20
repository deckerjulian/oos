// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Where stored files go and how they are written: the storage directory and the stick, free and latest file names,
//! the check that a save follows no link and writes no device, and the sync before a stick is pulled. A new place to
//! store to, or a new rule for a name, goes here.

use super::*;

/// Default storage directory on the unit (created when missing).
pub const DEFAULT_DIR: &str = "/tmp/oos";
/// Folder on a USB stick (`/bin/oos-usb` mounts it at /mnt/usb).
pub const USB_DIR: &str = "/mnt/usb/OOS";

/// Where CSV, PNG, mask and fast-frame files go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreDest {
    /// USB stick when mounted, otherwise the storage directory
    Auto,
    /// USB stick (`/mnt/usb/OOS`)
    Stick,
    /// The local storage directory, also when a stick is mounted (on the unit `/tmp/oos`, in RAM)
    Local,
}

impl StoreDest {
    pub const ALL: [StoreDest; 3] = [StoreDest::Auto, StoreDest::Stick, StoreDest::Local];

    pub fn name(self) -> &'static str {
        match self {
            StoreDest::Auto => "Auto",
            StoreDest::Stick => "USB",
            StoreDest::Local => "Local",
        }
    }
}

/// A stick is mounted, by the text of /proc/mounts.
pub fn usb_mounted(mounts: &str) -> bool {
    mounts.lines().any(|l| l.split(' ').nth(1) == Some("/mnt/usb"))
}

/// First free `<prefix>-NNN.<ext>` in `dir` (001 .. 999, then the last one is overwritten).
pub fn next_name(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{}-999.{}", prefix, ext));
    for i in 1..1000 {
        let c = dir.join(format!("{}-{:03}.{}", prefix, i, ext));
        // a link counts as taken, also one that points nowhere
        if c.symlink_metadata().is_err() {
            p = c;
            break;
        }
    }
    p
}

/// `<prefix>-<stamp>.<ext>` (`<prefix>-<stamp>-2.<ext>` and on when taken) with a time stamp, else `next_name`.
pub fn next_name_at(dir: &Path, prefix: &str, ext: &str, stamp: Option<&str>) -> PathBuf {
    let Some(st) = stamp else { return next_name(dir, prefix, ext) };
    for i in 1..100 {
        let n = if i == 1 { format!("{}-{}.{}", prefix, st, ext) } else { format!("{}-{}-{}.{}", prefix, st, i, ext) };
        if dir.join(&n).symlink_metadata().is_err() {
            return dir.join(n);
        }
    }
    next_name(dir, prefix, ext)
}

/// File name of a path for toasts ("shot-001.png").
pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A stick is foreign data and this program runs as root: a link or a device node on it, under a name a save would use,
/// could lead straight to the card. So only a regular file or a new one, and on the stick no link on the way there.
pub fn check_target(path: &Path) -> std::io::Result<()> {
    let refuse = |what: &str| {
        Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{}: {}", path.display(), what)))
    };
    if path.starts_with("/mnt/usb") {
        for a in path.ancestors().skip(1).take_while(|a| a.starts_with("/mnt/usb") && *a != Path::new("/mnt/usb")) {
            if a.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                return refuse("a link on the way there");
            }
        }
    }
    match path.symlink_metadata() {
        Ok(m) if !m.is_file() => refuse("not a regular file"),
        _ => Ok(()),
    }
}

/// Writes `data` to `path`, creating the directory first.
pub fn write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    check_target(path)?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, data)
}

/// Highest numbered `<prefix>-NNN.<ext>` in `dir`, if any.
pub fn latest_name(dir: &Path, prefix: &str, ext: &str) -> Option<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let s = e.file_name().to_string_lossy().into_owned();
        let want = format!("{}-", prefix);
        if let Some(rest) = s.strip_prefix(&want).and_then(|r| r.strip_suffix(&format!(".{}", ext))) {
            if let Ok(n) = rest.parse::<u32>() {
                if best.as_ref().is_none_or(|(b, _)| n >= *b) {
                    best = Some((n, e.path()));
                }
            }
        }
    }
    best.map(|x| x.1)
}

/// Writes everything cached to the disks (a stick can be pulled, the scope switched off at the mains).
pub fn sync_disks() {
    unsafe { libc::sync() };
}
