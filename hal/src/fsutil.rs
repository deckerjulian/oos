// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Files replaced in one step: a temporary file next to the target, fsync, rename, then the directory synced. The scope
//! is switched off at the mains, so a later start finds either the old file or the new one, never a part.

use std::io::Write;
use std::path::{Path, PathBuf};

/// What a failed sync of the directory after the rename means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DirSync {
    /// the directory is not synced
    Skip,
    /// synced, a failure is ignored
    #[default]
    BestEffort,
    /// synced, a failure is the caller's error (Unix only: Windows opens no directory as a file)
    Required,
}

/// How `write_atomic` writes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opts<'a> {
    /// the temporary file; None = the target's path with `.new` appended
    pub tmp: Option<&'a Path>,
    /// the old file is kept as the target's path with `.bak` appended
    pub bak: bool,
    /// the mode a new file gets (Unix), e.g. 0o600 for keys
    pub mode: Option<u32>,
    pub dir_sync: DirSync,
}

/// `path` with `ext` appended to its whole name (`a.conf` -> `a.conf.new`).
fn appended(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// Replaces `path` with `bytes` in one step (see the module). The directory must exist.
pub fn write_atomic(path: &Path, bytes: &[u8], o: &Opts) -> std::io::Result<()> {
    let tmp = o.tmp.map_or_else(|| appended(path, ".new"), Path::to_path_buf);
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if let Some(m) = o.mode {
        std::os::unix::fs::OpenOptionsExt::mode(&mut open, m);
    }
    {
        let mut f = open.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if o.bak && path.exists() {
        std::fs::rename(path, appended(path, ".bak"))?;
    }
    std::fs::rename(&tmp, path)?;
    match o.dir_sync {
        DirSync::Skip => Ok(()),
        DirSync::BestEffort => {
            let _ = sync_dir(path);
            Ok(())
        }
        DirSync::Required => sync_dir(path),
    }
}

/// Makes a rename or a removal of `path` durable: syncs the directory it is in (Unix; elsewhere nothing to do).
pub fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        std::fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oos-fsutil-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // Break it catches: a temporary file left behind, a backup of the new instead of the old content, a key file
    // readable by others
    #[test]
    fn a_file_is_replaced_with_backup_and_mode() {
        let d = dir("replace");
        let p = d.join("a.hex");
        write_atomic(&p, b"one", &Opts::default()).unwrap();
        write_atomic(&p, b"two", &Opts { bak: true, mode: Some(0o600), dir_sync: DirSync::Required, ..Default::default() }).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(std::fs::read(d.join("a.hex.bak")).unwrap(), b"one");
        assert!(!d.join("a.hex.new").exists());
        let tmp = d.join("b.tmp");
        write_atomic(&d.join("b"), b"x", &Opts { tmp: Some(&tmp), mode: Some(0o600), ..Default::default() }).unwrap();
        assert!(!tmp.exists() && std::fs::read(d.join("b")).unwrap() == b"x");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(d.join("b")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
