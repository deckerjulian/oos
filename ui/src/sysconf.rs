// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Settings of the scope itself, not of a measurement: `/oos/system.conf`, `key = value` lines. A saved or recalled
//! setup does not touch them. Unknown lines of a newer version or written by hand stay in the file.
//!
//! ```text
//! ntp = off                 # System > Date and time > Time from NTP
//! ntp.server = dhcp        # dhcp: the server of the DHCP lease (option 42), else pool.ntp.org
//! tz = CET-1CEST,M3.5.0,M10.5.0/3
//! eth.retry_10m = on        # System > Network > 10 Mbit retry
//! remote.web = on           # System > Network > Remote services (also remote.scpi, remote.vxi11, remote.ssh,
//!                           # remote.mdns)
//! ssh.password = on         # System > Network > Remote services > SSH settings > Password login
//! net.name = dho924s-204527 # the scope's name on the network (host name, <name>.local); written at the first start
//! graphics = gpu            # System > Graphics (gpu or cpu)
//! backlight = 100            # Display > Backlight, percent (10..100)
//! ```

use crate::clock::Tz;
use std::io::Write;
use std::path::{Path, PathBuf};

/// On OOS's own data partition.
pub const PATH: &str = "/oos/system.conf";
/// The defaults of an image built for one unit (boot/build-dev-image.sh `OOS_UNIT`, docs/network.md 6); rcS reads
/// it too (`oos-conf`).
pub const IMAGE_PATH: &str = "/etc/oos/system.conf";
/// Without the data partition: in RAM, lost at power-off.
pub const TMP_PATH: &str = "/tmp/oos-system.conf";
/// `ntp.server = dhcp`: the NTP server the DHCP lease names (option 42, RFC 2132), else `FALLBACK_SERVER`.
pub const DHCP: &str = "dhcp";
pub const DEFAULT_SERVER: &str = DHCP;
/// Asked with `ntp.server = dhcp` while no lease names a server.
pub const FALLBACK_SERVER: &str = "pool.ntp.org";
/// The servers of a lease, space separated, one file per interface (`udhcpc.script`): `<this><interface>`.
pub const DHCP_NTP_FILE: &str = "/tmp/oos-dhcp-ntp.";
/// The interfaces whose lease counts, in this order.
pub const DHCP_IFACES: [&str; 2] = ["eth0", "wlan0"];
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
    /// the network services (System > Network > Remote services): web page with the update path (8080), SCPI (5555),
    /// VXI-11 (111 and its core port), SSH (22, `/bin/oos-ssh` reads it too; off by default, docs/network.md 7)
    pub remote_web: bool,
    pub remote_scpi: bool,
    pub remote_vxi11: bool,
    pub remote_ssh: bool,
    /// SSH login with the generated password as well as with a trusted key (`/bin/oos-ssh`)
    pub ssh_password: bool,
    /// the mDNS responder (mdns.rs): `<net_name>.local` and the services for computers that browse
    pub remote_mdns: bool,
    /// the scope's name on the network (`mdns::name_ok`): host name, sent to DHCP, `<name>.local`. Empty: not set yet,
    /// then the one from model and serial number (`Ui::net_name`)
    pub net_name: String,
    /// System > Graphics: the picture put together on the GPU (`gpu.rs`), else on the CPU. The GPU by default: a
    /// third less load on the frame loop, the same picture (gpu/README.md); where it does not start, the CPU draws.
    pub gpu: bool,
    /// Display > Backlight, percent (ui/backlight.rs)
    pub backlight: u32,
    /// lines this version does not know
    extra: Vec<String>,
}

impl Default for SysConf {
    fn default() -> Self {
        SysConf {
            ntp: false,
            ntp_server: DEFAULT_SERVER.into(),
            tz: DEFAULT_TZ.into(),
            eth_retry_10m: true,
            remote_web: true,
            remote_scpi: true,
            remote_vxi11: true,
            remote_ssh: false,
            ssh_password: true,
            remote_mdns: true,
            net_name: String::new(),
            gpu: true,
            backlight: 100,
            extra: Vec::new(),
        }
    }
}

/// A host name or an address: letters, digits, `.`, `-`, `:` (IPv6).
pub fn server_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= SERVER_MAX && s.bytes().all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c)) && !s.starts_with(['-', '.'])
}

/// Where the server asked came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NtpOrigin {
    /// the setting names it
    Set,
    /// `dhcp`: the lease of this interface
    Dhcp(&'static str),
    /// `dhcp`, but no lease names a server: `FALLBACK_SERVER`
    NoDhcp,
}

/// The server to ask for the setting `ntp.server`: the setting itself, or with `dhcp` the first good server of the
/// lease on eth0, else on wlan0, else `FALLBACK_SERVER`. `lease` gives the text of an interface's file
/// (`DHCP_NTP_FILE`), None without one. Called at each request: a lease can change at any time.
pub fn ntp_server(setting: &str, lease: impl Fn(&str) -> Option<String>) -> (String, NtpOrigin) {
    if setting != DHCP {
        return (setting.to_string(), NtpOrigin::Set);
    }
    for i in DHCP_IFACES {
        if let Some(s) = lease(i).and_then(|t| t.split_whitespace().find(|s| server_ok(s)).map(str::to_string)) {
            return (s, NtpOrigin::Dhcp(i));
        }
    }
    (FALLBACK_SERVER.to_string(), NtpOrigin::NoDhcp)
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
        SysConf::parse_over(SysConf::default(), text)
    }

    /// `parse` with the defaults of `base` instead of the built-in ones (the image defaults, `IMAGE_PATH`).
    pub fn parse_over(base: SysConf, text: &str) -> (SysConf, Vec<String>) {
        let mut c = base;
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
                "ntp.server" if v.eq_ignore_ascii_case(DHCP) => {
                    c.ntp_server = DHCP.into();
                    true
                }
                "ntp.server" => server_ok(v).then(|| c.ntp_server = v.into()).is_some(),
                "tz" => Tz::parse(v).is_ok().then(|| c.tz = v.into()).is_some(),
                "eth.retry_10m" => on_off(v).map(|b| c.eth_retry_10m = b).is_some(),
                "remote.web" => on_off(v).map(|b| c.remote_web = b).is_some(),
                "remote.scpi" => on_off(v).map(|b| c.remote_scpi = b).is_some(),
                "remote.vxi11" => on_off(v).map(|b| c.remote_vxi11 = b).is_some(),
                "remote.ssh" => on_off(v).map(|b| c.remote_ssh = b).is_some(),
                "ssh.password" => on_off(v).map(|b| c.ssh_password = b).is_some(),
                "remote.mdns" => on_off(v).map(|b| c.remote_mdns = b).is_some(),
                "net.name" => {
                    let n = v.to_ascii_lowercase();
                    crate::mdns::name_ok(&n).then(|| c.net_name = n).is_some()
                }
                "graphics" => match v {
                    "cpu" | "gpu" => {
                        c.gpu = v == "gpu";
                        true
                    }
                    _ => false,
                },
                "backlight" => v.parse::<u32>().ok().filter(|p| (10..=100).contains(p)).map(|p| c.backlight = p).is_some(),
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
        s.push_str(&format!("remote.web = {}\n", word(self.remote_web)));
        s.push_str(&format!("remote.scpi = {}\n", word(self.remote_scpi)));
        s.push_str(&format!("remote.vxi11 = {}\n", word(self.remote_vxi11)));
        s.push_str(&format!("remote.ssh = {}\n", word(self.remote_ssh)));
        s.push_str(&format!("ssh.password = {}\n", word(self.ssh_password)));
        s.push_str(&format!("remote.mdns = {}\n", word(self.remote_mdns)));
        if !self.net_name.is_empty() {
            s.push_str(&format!("net.name = {}\n", self.net_name));
        }
        s.push_str(&format!("graphics = {}\n", if self.gpu { "gpu" } else { "cpu" }));
        s.push_str(&format!("backlight = {}\n", self.backlight));
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

    /// The defaults of an image built for one unit: the known keys of `image` (`IMAGE_PATH`); its other lines
    /// (`net.allow`, `eth.mac`: rcS and `netacl` read them there) are not copied into the settings file.
    pub fn image_defaults(image: &str) -> (SysConf, Vec<String>) {
        let (mut c, bad) = SysConf::parse(image);
        c.extra.clear();
        (c, bad)
    }

    /// From a file over the image defaults (`IMAGE_PATH`, none on most images); a missing file gives the defaults.
    /// Problems go to the log.
    pub fn load(path: &Path) -> SysConf {
        let base = match std::fs::read_to_string(IMAGE_PATH) {
            Ok(t) => {
                let (c, bad) = SysConf::image_defaults(&t);
                for e in bad {
                    eprintln!("oos-ui: {}: {}", IMAGE_PATH, e);
                }
                c
            }
            Err(_) => SysConf::default(),
        };
        SysConf::load_over(base, path)
    }

    /// `load` with the given defaults.
    pub fn load_over(base: SysConf, path: &Path) -> SysConf {
        match std::fs::read(path) {
            Ok(b) => {
                let (c, bad) = SysConf::parse_over(base, &String::from_utf8_lossy(&b));
                for e in bad {
                    eprintln!("oos-ui: {}: {}", path.display(), e);
                }
                c
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("oos-ui: {}: {}", path.display(), e);
                }
                base
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

/// The text the fork's keys (`acq.*`, `perf.*`, `trig.*`, ...; `drawpipe::conf_value`) are looked up in: the settings
/// file (`default_path`), then the image defaults (`IMAGE_PATH`). `conf_value` takes the first line of a key, so the
/// settings file wins, also with an empty value, as `oos-conf` has it for the scripts.
pub fn fork_text() -> String {
    fork_text_of(&std::fs::read_to_string(default_path()).unwrap_or_default(), &std::fs::read_to_string(IMAGE_PATH).unwrap_or_default())
}

/// `fork_text` from the two files' text.
pub fn fork_text_of(settings: &str, image: &str) -> String {
    format!("{}\n{}", settings, image)
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

    // Break it catches: a fork key (acq.pipeline, perf.place, acq.prearm) that only /oos/system.conf can set, so the
    // unit image's defaults (/etc/oos/system.conf) do nothing; or an image default that beats the user's setting
    #[test]
    fn fork_keys_take_the_image_default_unless_the_settings_set_them() {
        let image = "acq.pipeline = batch\nperf.place = on\n";
        let v = |settings: &str, k: &str| crate::drawpipe::conf_value(None, &super::fork_text_of(settings, image), k);
        assert_eq!(v("", "acq.pipeline").as_deref(), Some("batch"));
        assert_eq!(v("perf.place = off\n", "perf.place").as_deref(), Some("off"));
        assert_eq!(v("acq.pipeline =\n", "acq.pipeline").as_deref(), Some(""), "an empty value in the settings wins too");
        assert_eq!(v("", "acq.prearm"), None);
    }
    use super::*;

    #[test]
    fn defaults_send_nothing_out() {
        let c = SysConf::default();
        assert!(!c.ntp);
        assert_eq!(c.ntp_server, "dhcp");
        assert_eq!(c.zone(), Tz::utc());
        assert!(c.eth_retry_10m);
        // no shell on the network unless an image or the user switches SSH on
        assert!(!c.remote_ssh && c.ssh_password);
        assert_eq!(SysConf::parse("").0, c);
    }

    #[test]
    fn round_trip_keeps_unknown_lines() {
        let mut c = SysConf::default();
        c.ntp = true;
        c.ntp_server = "ptbtime1.ptb.de".into();
        c.tz = "CET-1CEST,M3.5.0,M10.5.0/3".into();
        c.eth_retry_10m = false;
        (c.remote_web, c.remote_ssh, c.ssh_password, c.remote_mdns) = (false, true, false, false);
        c.net_name = "lab-scope-3".into();
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
    fn the_name_on_the_network() {
        let c = SysConf::default();
        assert!(c.remote_mdns, "mDNS on by default: it stays on the link");
        assert!(c.net_name.is_empty() && !c.text().contains("net.name"));
        let (d, bad) = SysConf::parse("net.name = Lab-Scope-3\nremote.mdns = off\n");
        assert!(bad.is_empty());
        assert_eq!((d.net_name.as_str(), d.remote_mdns), ("lab-scope-3", false));
        assert!(d.text().contains("\nnet.name = lab-scope-3\n") && d.text().contains("\nremote.mdns = off\n"));
        for bad_name in ["lab scope", "-x", "a.b", "x_y", &"x".repeat(64)] {
            let (e, bad) = SysConf::parse(&format!("net.name = {}\n", bad_name));
            assert_eq!((e.net_name.as_str(), bad.len()), ("", 1), "{}", bad_name);
        }
    }

    #[test]
    fn dhcp_server_round_trip() {
        let mut c = SysConf::default();
        c.ntp = true;
        assert!(c.text().contains("\nntp.server = dhcp\n"));
        assert_eq!(SysConf::parse(&c.text()).0, c);
        // a file of an older version keeps its server; DHCP by hand in any case
        assert_eq!(SysConf::parse("ntp.server = pool.ntp.org\n").0.ntp_server, "pool.ntp.org");
        let (d, bad) = SysConf::parse("ntp.server = DHCP\n");
        assert!(bad.is_empty());
        assert_eq!(d.ntp_server, DHCP);
        assert!(d.text().contains("\nntp.server = dhcp\n"));
    }

    #[test]
    fn dhcp_server_resolves_from_the_lease() {
        let none = |_: &str| None;
        assert_eq!(ntp_server("ptbtime1.ptb.de", |_| Some("10.0.0.1".into())), ("ptbtime1.ptb.de".into(), NtpOrigin::Set));
        assert_eq!(ntp_server(DHCP, none), ("pool.ntp.org".into(), NtpOrigin::NoDhcp));
        // eth0 first, the first server of the list
        let both = |i: &str| Some(if i == "eth0" { "192.168.1.1 192.168.1.2\n" } else { "10.0.0.1" }.to_string());
        assert_eq!(ntp_server(DHCP, both), ("192.168.1.1".into(), NtpOrigin::Dhcp("eth0")));
        let wlan = |i: &str| (i == "wlan0").then(|| "10.0.0.1\n".to_string());
        assert_eq!(ntp_server(DHCP, wlan), ("10.0.0.1".into(), NtpOrigin::Dhcp("wlan0")));
        // an empty file or one with nothing usable counts as none
        let junk = |i: &str| Some(if i == "eth0" { "" } else { "a;b -x" }.to_string());
        assert_eq!(ntp_server(DHCP, junk), ("pool.ntp.org".into(), NtpOrigin::NoDhcp));
        let skip = |_: &str| Some("bad/one 172.16.0.9".to_string());
        assert_eq!(ntp_server(DHCP, skip).0, "172.16.0.9");
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

    // the DHO804 unit image: clock from NTP in CET, the server of the DHCP lease unless the settings name one
    #[test]
    fn unit_image_takes_the_time_from_the_dhcp_server() {
        let (base, bad) = SysConf::image_defaults(include_str!("../../rootfs/unit/dho804/system.conf"));
        assert!(bad.is_empty(), "{:?}", bad);
        assert!(base.ntp && base.tz == "CET-1CEST,M3.5.0,M10.5.0/3" && base.ntp_server == DHCP);
        let lease = |i: &str| (i == "eth0").then(|| "192.0.2.1".to_string());
        let (fresh, _) = SysConf::parse_over(base.clone(), "");
        assert_eq!(ntp_server(&fresh.ntp_server, lease), ("192.0.2.1".into(), NtpOrigin::Dhcp("eth0")));
        let (user, _) = SysConf::parse_over(base, "ntp.server = pool.ntp.org\nremote.telnet = on\n");
        assert_eq!(ntp_server(&user.ntp_server, lease), ("pool.ntp.org".into(), NtpOrigin::Set));
    }

    #[test]
    fn image_defaults_under_the_settings() {
        let image = "# unit\nnet.allow = 192.0.2.0/24\nremote.ssh = on\neth.mac = 02:00:00:00:00:01\n";
        let (base, bad) = SysConf::image_defaults(image);
        assert!(bad.is_empty() && base.remote_ssh && base.ssh_password && base.remote_web);
        // the unit's own keys stay in the image file, not in the settings written later
        assert!(!base.text().contains("net.allow") && !base.text().contains("eth.mac"));
        let (c, _) = SysConf::parse_over(base.clone(), "ntp = on\n");
        assert!(c.ntp && c.remote_ssh);
        let (c, _) = SysConf::parse_over(base, "remote.ssh = off\nssh.password = off\n");
        assert!(!c.remote_ssh && !c.ssh_password);
        // telnet is not part of this fork: an old line stays in the file and starts nothing
        let (c, bad) = SysConf::parse("remote.telnet = on\n");
        assert!(bad.is_empty() && c.text().contains("\nremote.telnet = on\n") && !c.remote_ssh);
    }
}
