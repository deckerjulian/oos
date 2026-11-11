// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Settings of the scope itself, not of a measurement: `/oos/system.conf`, `key = value` lines. A saved or recalled
//! setup does not touch them. Unknown lines of a newer version or written by hand stay in the file.
//!
//! ```text
//! ntp = off                 # System > Date and time > Time from NTP
//! ntp.server = pool.ntp.org
//! tz = CET-1CEST,M3.5.0,M10.5.0/3
//! eth.retry_10m = on        # System > Network > 10 Mbit retry
//! ```

use crate::clock::Tz;
use std::io::Write;
use std::path::{Path, PathBuf};

/// On OOS's own data partition.
pub const PATH: &str = "/oos/system.conf";
/// Without the data partition: in RAM, lost at power-off.
pub const TMP_PATH: &str = "/tmp/oos-system.conf";
pub const DEFAULT_SERVER: &str = "pool.ntp.org";
pub const DEFAULT_TZ: &str = "UTC0";
/// Longest server name taken.
pub const SERVER_MAX: usize = 63;

#[derive(Clone, Debug, PartialEq)]
pub struct SysConf {
    /// set the clock from an NTP server once a network link is up (off: OOS sends nothing to the outside)
    pub ntp: bool,
    pub ntp_server: String,
    /// POSIX time zone string (clock.rs)
    pub tz: String,
    /// a link that comes up at 10 Mbit/s gets one more autonegotiation (ethlink.rs)
    pub eth_retry_10m: bool,
    /// lines this version does not know
    extra: Vec<String>,
}

impl Default for SysConf {
    fn default() -> Self {
        SysConf { ntp: false, ntp_server: DEFAULT_SERVER.into(), tz: DEFAULT_TZ.into(), eth_retry_10m: true, extra: Vec::new() }
    }
}

/// A host name or an address: letters, digits, `.`, `-`, `:` (IPv6).
pub fn server_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= SERVER_MAX && s.bytes().all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c)) && !s.starts_with(['-', '.'])
}

fn on_off(v: &str) -> Option<bool> {
    match v {
        "on" | "true" | "1" => Some(true),
        "off" | "false" | "0" => Some(false),
        _ => None,
    }
}

fn word(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

impl SysConf {
    /// The settings of a file's text, and what was wrong in it (those keys keep their defaults).
    pub fn parse(text: &str) -> (SysConf, Vec<String>) {
        let mut c = SysConf::default();
        let mut bad = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else {
                bad.push(format!("not key = value: {}", l));
                continue;
            };
            let (k, v) = (k.trim(), v.split(" #").next().unwrap_or("").trim());
            let ok = match k {
                "ntp" => on_off(v).map(|b| c.ntp = b).is_some(),
                "ntp.server" => server_ok(v).then(|| c.ntp_server = v.into()).is_some(),
                "tz" => Tz::parse(v).is_ok().then(|| c.tz = v.into()).is_some(),
                "eth.retry_10m" => on_off(v).map(|b| c.eth_retry_10m = b).is_some(),
                _ => {
                    c.extra.push(line.to_string());
                    true
                }
            };
            if !ok {
                bad.push(format!("{}: {:?} not taken", k, v));
            }
        }
        (c, bad)
    }

    pub fn text(&self) -> String {
        let mut s = String::from("# OOS system settings: key = value (oos-ui, System menu)\n");
        s.push_str(&format!("ntp = {}\n", word(self.ntp)));
        s.push_str(&format!("ntp.server = {}\n", self.ntp_server));
        s.push_str(&format!("tz = {}\n", self.tz));
        s.push_str(&format!("eth.retry_10m = {}\n", word(self.eth_retry_10m)));
        for l in &self.extra {
            s.push_str(l);
            s.push('\n');
        }
        s
    }

    /// The time zone (UTC if the string is not one).
    pub fn zone(&self) -> Tz {
        Tz::parse(&self.tz).unwrap_or_else(|_| Tz::utc())
    }

    /// From a file; a missing one gives the defaults. Problems go to the log.
    pub fn load(path: &Path) -> SysConf {
        match std::fs::read(path) {
            Ok(b) => {
                let (c, bad) = SysConf::parse(&String::from_utf8_lossy(&b));
                for e in bad {
                    eprintln!("oos-ui: {}: {}", path.display(), e);
                }
                c
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("oos-ui: {}: {}", path.display(), e);
                }
                SysConf::default()
            }
        }
    }

    /// Replaces the file in one step and syncs it: the scope is switched off at the mains.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("conf.new");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(self.text().as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        std::fs::File::open(dir)?.sync_all()
    }
}

/// `/oos/system.conf` when the data partition is there (it has `/oos/setups`), else the file in RAM.
pub fn default_path() -> PathBuf {
    if Path::new("/oos/setups").is_dir() {
        PATH.into()
    } else {
        TMP_PATH.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_send_nothing_out() {
        let c = SysConf::default();
        assert!(!c.ntp);
        assert_eq!(c.ntp_server, "pool.ntp.org");
        assert_eq!(c.zone(), Tz::utc());
        assert!(c.eth_retry_10m);
        assert_eq!(SysConf::parse("").0, c);
    }

    #[test]
    fn round_trip_keeps_unknown_lines() {
        let mut c = SysConf::default();
        c.ntp = true;
        c.ntp_server = "ptbtime1.ptb.de".into();
        c.tz = "CET-1CEST,M3.5.0,M10.5.0/3".into();
        c.eth_retry_10m = false;
        let (back, bad) = SysConf::parse(&c.text());
        assert!(bad.is_empty(), "{:?}", bad);
        assert_eq!(back, c);
        let (d, bad) = SysConf::parse("# by hand\nntp = on # comment\nfuture.key = 7\n");
        assert!(bad.is_empty() && d.ntp);
        assert!(d.text().contains("\nfuture.key = 7\n"));
        assert_eq!(SysConf::parse(&d.text()).0, d);
    }

    #[test]
    fn bad_values_keep_the_default() {
        let (c, bad) = SysConf::parse("ntp = maybe\nntp.server = a b;rm\ntz = Mars+99\neth.retry_10m = 2\nnonsense\n");
        assert_eq!(c, SysConf::default());
        assert_eq!(bad.len(), 5, "{:?}", bad);
        for s in ["pool.ntp.org", "192.168.1.1", "fe80::1", "time-a.example"] {
            assert!(server_ok(s), "{}", s);
        }
        for s in ["", "-x", ".x", "a/b", "a b", &"x".repeat(64)] {
            assert!(!server_ok(s), "{}", s);
        }
    }

    #[test]
    fn saved_and_loaded() {
        let dir = std::env::temp_dir().join(format!("oos-sysconf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("system.conf");
        let _ = std::fs::remove_file(&p);
        assert_eq!(SysConf::load(&p), SysConf::default());
        let mut c = SysConf::default();
        c.ntp = true;
        c.tz = "JST-9".into();
        c.save(&p).unwrap();
        assert_eq!(SysConf::load(&p), c);
        assert!(!dir.join("system.conf.new").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
