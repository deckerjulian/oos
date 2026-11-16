// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;

fn staged(name: &str, count: usize) -> (std::path::PathBuf, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("oos-calfile-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<_> = (0..count).map(|k| dir.join(format!("{k}.hex")).to_str().unwrap().to_string()).collect();
    for path in &paths {
        std::fs::write(path, b"good").unwrap();
        std::fs::write(format!("{path}.bak"), b"previous").unwrap();
        std::fs::write(format!("{path}.new"), b"next").unwrap();
    }
    (dir, paths)
}

#[test]
fn save_removes_stale_backup_temporary() {
    let (dir, paths) = staged("stale-backup-temporary", 1);
    std::fs::hard_link(&paths[0], dir.join("0.hex.bak.new")).unwrap();
    let result = save(dir.to_str().unwrap(), "0.hex", b"saved", (2000, 1, 1, 0, 0, 0));
    let live = std::fs::read(&paths[0]).unwrap();
    let backup = std::fs::read(dir.join("0.hex.bak")).unwrap();
    let temporary = dir.join("0.hex.bak.new").exists();
    std::fs::remove_dir_all(dir).unwrap();
    result.unwrap();
    assert_eq!(parse(&live), Ok(&b"saved"[..]));
    assert_eq!(backup, b"good");
    assert!(!temporary);
}

#[test]
fn publish_always_keeps_the_live_file() {
    let (dir, paths) = staged("atomic", 1);
    let result = publish_with(&paths, |from, to| {
        if from.ends_with(".new") {
            assert_eq!(std::fs::read(to).unwrap(), b"good");
            assert_eq!(std::fs::read(format!("{to}.bak")).unwrap(), b"good");
        }
        std::fs::rename(from, to)
    });
    assert!(result.is_ok());
    assert_eq!(std::fs::read(&paths[0]).unwrap(), b"next");
    assert_eq!(std::fs::read(format!("{}.bak", paths[0])).unwrap(), b"good");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rollback_attempts_all_restores_and_combines_errors() {
    let (dir, paths) = staged("restore-errors", 4);
    let mut restores = Vec::new();
    let result = publish_with(&paths, |from, to| {
        if from == format!("{}.new", paths[3]) {
            return Err(std::io::Error::other("publish failed"));
        }
        if from.ends_with(".bak") {
            restores.push(to.to_string());
            for (k, error) in [(1, "restore one failed"), (2, "restore two failed")] {
                if to == paths[k] {
                    return Err(std::io::Error::other(error));
                }
            }
        }
        std::fs::rename(from, to)
    });
    let error = result.unwrap_err().to_string();
    let first = std::fs::read(&paths[0]);
    std::fs::remove_dir_all(dir).unwrap();
    assert!(error.contains("publish failed"), "{error}");
    assert!(error.contains("restore one failed"), "{error}");
    assert!(error.contains("restore two failed"), "{error}");
    assert!(restores.contains(&paths[0]));
    assert_eq!(first.unwrap(), b"good");
}

#[test]
fn successful_publish_reports_directory_sync_failure() {
    let (dir, paths) = staged("dir-sync", 1);
    let result = save_all_with_sync(dir.to_str().unwrap(), &[("0.hex", b"saved")], (2000, 1, 1, 0, 0, 0), |_| {
        Err(std::io::Error::other("directory sync failed"))
    });
    assert_eq!(parse(&std::fs::read(&paths[0]).unwrap()), Ok(&b"saved"[..]));
    std::fs::remove_dir_all(dir).unwrap();
    let error = result.unwrap_err();
    assert!(error.to_string().contains("files published; directory fsync failed"));
    assert!(matches!(error, SaveError::PublishedDirectorySync(e) if e.to_string() == "directory sync failed"));
}

#[test]
fn backup_link_failure_preserves_previous_backup() {
    let (dir, paths) = staged("backup-link-failure", 1);
    let result = backup_with(&paths[0], |_, _| Err(std::io::Error::other("link failed")));
    let live = std::fs::read(&paths[0]);
    let backup = std::fs::read(format!("{}.bak", paths[0]));
    let temporary = std::path::Path::new(&format!("{}.bak.new", paths[0])).exists();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(result.unwrap_err().to_string(), "link failed");
    assert_eq!(live.unwrap(), b"good");
    assert_eq!(backup.unwrap(), b"previous");
    assert!(!temporary);
}

#[test]
fn backup_links_before_replacing_previous_backup() {
    let (dir, paths) = staged("backup-link-order", 1);
    backup_with(&paths[0], |from, to| {
        assert_eq!(std::fs::read(format!("{from}.bak")).unwrap(), b"previous");
        assert_eq!(to, format!("{from}.bak.new"));
        std::fs::hard_link(from, to)
    })
    .unwrap();
    assert_eq!(std::fs::read(&paths[0]).unwrap(), b"good");
    assert_eq!(std::fs::read(format!("{}.bak", paths[0])).unwrap(), b"good");
    assert!(!std::path::Path::new(&format!("{}.bak.new", paths[0])).exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn publish_failure_is_not_a_published_sync_warning() {
    let (dir, paths) = staged("publish-sync-failure", 1);
    std::fs::remove_file(format!("{}.bak", paths[0])).unwrap();
    std::fs::create_dir(format!("{}.bak", paths[0])).unwrap();
    let result = save_all_with_sync(dir.to_str().unwrap(), &[("0.hex", b"saved")], (2000, 1, 1, 0, 0, 0), |_| {
        Err(std::io::Error::other("directory sync failed"))
    });
    assert_eq!(std::fs::read(&paths[0]).unwrap(), b"good");
    assert!(!std::path::Path::new(&format!("{}.bak.new", paths[0])).exists());
    assert!(!std::path::Path::new(&format!("{}.new", paths[0])).exists());
    std::fs::remove_dir_all(dir).unwrap();
    assert!(matches!(result, Err(SaveError::Failed(e)) if e.to_string().contains("directory fsync")));
}

#[test]
fn save_restores_files_after_late_rename_error() {
    let dir = std::env::temp_dir().join(format!("oos-calfile-rollback-{}", std::process::id()));
    let ds = dir.to_str().unwrap();
    let stamp = (2000, 1, 1, 0, 0, 0);
    save(ds, "first.hex", &[1], stamp).unwrap();
    save(ds, "last.hex", &[2], stamp).unwrap();
    std::fs::create_dir(dir.join("last.hex.bak")).unwrap();
    assert!(save_all(ds, &[("first.hex", &[3]), ("new.hex", &[4]), ("last.hex", &[5])], stamp).is_err());
    assert_eq!(parse(&std::fs::read(dir.join("first.hex")).unwrap()), Ok(&[1][..]));
    assert_eq!(parse(&std::fs::read(dir.join("last.hex")).unwrap()), Ok(&[2][..]));
    assert!(!dir.join("new.hex").exists());
    assert!(!dir.join("first.hex.new").exists());
    assert!(!dir.join("last.hex.new").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
