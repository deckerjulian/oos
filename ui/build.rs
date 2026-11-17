// SPDX-License-Identifier: GPL-2.0-or-later
//! The commit the binary was built from, for System > About (`OOS_GIT`): "which build is this" has one answer on the
//! scope and on a desktop. Without git, or outside a checkout, it is left out and About shows the version alone.
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/logs/HEAD");
    let git = |args: &[&str]| Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if let Some(hash) = git(&["rev-parse", "--short=7", "HEAD"]).filter(|h| !h.is_empty()) {
        let date = git(&["log", "-1", "--format=%cd", "--date=format:%Y-%m-%d %H:%M"]).unwrap_or_default();
        println!("cargo:rustc-env=OOS_GIT={hash} {date}");
    }
}
