// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Who may control the scope over the network: `net.allow` in `/oos/system.conf`, with the image default from
//! `/etc/oos/system.conf` (docs/network.md 6). A list of address prefixes, `192.0.2.0/24 fd00::/8`. Empty or
//! absent: everyone, as before. rcS builds the firewall from the same key (`oos-firewall`); this list is the second
//! line behind it, for what can change the scope: `POST /update`, `/mcp`, SCPI and VXI-11. The scope itself
//! (loopback) is always allowed. `onlink6` stands for the IPv6 prefixes on the link (from the router's advertisements,
//! `net6::PREFIX_FILE`), which change with the provider.

use std::net::IpAddr;
use std::path::Path;

/// The image default of the system settings (an image built for one unit, boot/build-dev-image.sh `OOS_UNIT`).
pub const IMAGE_CONF: &str = "/etc/oos/system.conf";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Acl {
    nets: Vec<(IpAddr, u8)>,
    /// `onlink6`: the IPv6 prefixes on the link too, read at each check
    onlink6: bool,
}

/// The `net.allow` word for the IPv6 prefixes on the link (`oos-firewall` has it too).
pub const ONLINK6: &str = "onlink6";

/// The value of `key` in the text of a `key = value` file: the last line wins, `#` starts a comment.
pub fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.split(" #").next().unwrap_or("").trim())
        })
        .last()
}

/// The same address with only the first `bits` bits.
fn masked(ip: IpAddr, bits: u8) -> IpAddr {
    match ip {
        IpAddr::V4(a) => {
            let m = if bits == 0 { 0 } else { u32::MAX << (32 - bits as u32) };
            IpAddr::V4((u32::from(a) & m).into())
        }
        IpAddr::V6(a) => {
            let m = if bits == 0 { 0 } else { u128::MAX << (128 - bits as u32) };
            IpAddr::V6((u128::from(a) & m).into())
        }
    }
}

impl Acl {
    /// A `net.allow` value: prefixes separated by spaces or commas; a bare address is a host. What does not parse is
    /// returned and left out.
    pub fn parse(s: &str) -> (Acl, Vec<String>) {
        let mut acl = Acl::default();
        let mut bad = Vec::new();
        for w in s.split([' ', ',', '\t']).filter(|w| !w.is_empty()) {
            if w == ONLINK6 {
                acl.onlink6 = true;
                continue;
            }
            let (a, b) = w.split_once('/').unwrap_or((w, ""));
            let Ok(ip) = a.parse::<IpAddr>() else {
                bad.push(w.to_string());
                continue;
            };
            let max = if ip.is_ipv4() { 32 } else { 128 };
            let bits = if b.is_empty() { Some(max) } else { b.parse::<u8>().ok().filter(|&n| n <= max) };
            match bits {
                Some(n) => acl.nets.push((masked(ip, n), n)),
                None => bad.push(w.to_string()),
            }
        }
        (acl, bad)
    }

    /// No list: everyone is allowed.
    pub fn is_open(&self) -> bool {
        self.nets.is_empty() && !self.onlink6
    }

    pub fn permits(&self, ip: IpAddr) -> bool {
        self.permits_with(ip, crate::net6::onlink)
    }

    /// `permits` with the prefixes on the link from `onlink` (asked only when `onlink6` is in the list and the address
    /// is IPv6).
    pub fn permits_with(&self, ip: IpAddr, onlink: impl FnOnce() -> Vec<(std::net::Ipv6Addr, u8)>) -> bool {
        let ip = match ip {
            IpAddr::V6(a) => a.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 => v4,
        };
        if self.is_open() || ip.is_loopback() || self.nets.iter().any(|&(net, n)| net.is_ipv4() == ip.is_ipv4() && masked(ip, n) == net) {
            return true;
        }
        self.onlink6 && ip.is_ipv6() && onlink().into_iter().any(|(net, n)| masked(ip, n) == masked(IpAddr::V6(net), n))
    }

    /// From the image default and the settings file (the file wins, also with an empty value). A list that names
    /// something but of which nothing parses closes everything but loopback rather than opening it.
    pub fn from_texts(image: Option<&str>, settings: Option<&str>) -> (Acl, Vec<String>) {
        let v = settings.and_then(|t| value(t, "net.allow")).or_else(|| image.and_then(|t| value(t, "net.allow"))).unwrap_or("");
        let (mut acl, bad) = Acl::parse(v);
        if acl.is_open() && !bad.is_empty() {
            acl.nets.push(("::1".parse().unwrap(), 128));
        }
        (acl, bad)
    }

    /// `IMAGE_CONF` and the settings file `path` (sysconf); problems go to the log.
    pub fn load(path: &Path) -> Acl {
        let image = std::fs::read_to_string(IMAGE_CONF).ok();
        let settings = std::fs::read_to_string(path).ok();
        let (acl, bad) = Acl::from_texts(image.as_deref(), settings.as_deref());
        for b in bad {
            eprintln!("oos-ui: net.allow: {:?} is not an address prefix, left out", b);
        }
        if !acl.is_open() {
            eprintln!("oos-ui: update, MCP, SCPI and VXI-11 only from {}", acl);
        }
        acl
    }
}

impl std::fmt::Display for Acl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut v: Vec<String> = self.nets.iter().map(|(a, n)| format!("{}/{}", a, n)).collect();
        if self.onlink6 {
            v.push(ONLINK6.into());
        }
        f.write_str(&v.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn prefixes() {
        let (a, bad) = Acl::parse("192.0.2.0/24, fd00::/8 10.1.2.3");
        assert!(bad.is_empty(), "{:?}", bad);
        assert!(a.permits(ip("192.0.2.10")) && a.permits(ip("192.0.2.255")));
        assert!(!a.permits(ip("192.168.78.1")) && !a.permits(ip("192.168.177.64")));
        assert!(a.permits(ip("10.1.2.3")) && !a.permits(ip("10.1.2.4")));
        assert!(a.permits(ip("fd12::1")) && !a.permits(ip("fe80::1")));
        // loopback, also as an IPv4-mapped address of a dual-stack socket
        assert!(a.permits(ip("127.0.0.1")) && a.permits(ip("::1")));
        assert!(a.permits(ip("::ffff:192.0.2.5")) && !a.permits(ip("::ffff:8.8.8.8")));
        assert_eq!(a.to_string(), "192.0.2.0/24 fd00::/8 10.1.2.3/32");
        // host bits in the prefix are cleared
        assert_eq!(Acl::parse("192.0.2.145/24").0, Acl::parse("192.0.2.0/24").0);
        let (a, _) = Acl::parse("0.0.0.0/0");
        assert!(a.permits(ip("8.8.8.8")) && !a.permits(ip("2001:db8::1")));
    }

    #[test]
    fn prefixes_on_the_link() {
        let (a, bad) = Acl::parse("192.0.2.0/24 fe80::/10 onlink6");
        assert!(bad.is_empty() && !a.is_open(), "{:?}", bad);
        assert_eq!(a.to_string(), "192.0.2.0/24 fe80::/10 onlink6");
        let link = || crate::net6::parse_prefixes("2001:db8:5:7::/64 fd12:3456::/48");
        assert!(a.permits_with(ip("2001:db8:5:7::abcd"), link) && a.permits_with(ip("fd12:3456:0:9::1"), link));
        assert!(!a.permits_with(ip("2001:db8:5:8::1"), link) && !a.permits_with(ip("2a00:1450::1"), link));
        assert!(a.permits_with(ip("fe80::1"), link) && a.permits_with(ip("::ffff:192.0.2.3"), link));
        assert!(!a.permits_with(ip("::ffff:8.8.8.8"), link));
        // no prefixes yet (or no router): only the fixed list
        assert!(!a.permits_with(ip("2001:db8:5:7::abcd"), Vec::new) && a.permits_with(ip("fe80::1"), Vec::new));
        // IPv4 never asks for the prefixes
        assert!(!a.permits_with(ip("10.0.0.1"), || panic!("asked")));
        // onlink6 alone closes what is not on the link
        let (a, _) = Acl::parse("onlink6");
        assert!(!a.is_open() && !a.permits_with(ip("192.0.2.3"), link) && a.permits_with(ip("2001:db8:5:7::1"), link));
    }

    #[test]
    fn empty_is_open() {
        let a = Acl::default();
        assert!(a.is_open() && a.permits(ip("8.8.8.8")));
        let (a, bad) = Acl::from_texts(None, None);
        assert!(a.is_open() && bad.is_empty());
    }

    #[test]
    fn bad_entries() {
        let (a, bad) = Acl::parse("192.0.2.0/33 300.1.1.1 x 10.0.0.0/8");
        assert_eq!(bad, vec!["192.0.2.0/33", "300.1.1.1", "x"]);
        assert!(a.permits(ip("10.9.9.9")) && !a.permits(ip("192.0.2.1")));
        // nothing usable: closed except for loopback, never open by mistake
        let (a, bad) = Acl::from_texts(Some("net.allow = 192.0.2.0/99\n"), None);
        assert_eq!(bad.len(), 1);
        assert!(!a.is_open() && !a.permits(ip("192.0.2.1")) && a.permits(ip("127.0.0.1")));
    }

    #[test]
    fn settings_win_over_the_image() {
        let image = "# unit\nnet.allow = 192.0.2.0/24\nremote.telnet = off\n";
        let (a, _) = Acl::from_texts(Some(image), Some("ntp = on\n"));
        assert!(a.permits(ip("192.0.2.9")) && !a.permits(ip("192.168.1.9")));
        let (a, _) = Acl::from_texts(Some(image), Some("net.allow = 192.168.1.0/24 # new LAN\n"));
        assert!(!a.permits(ip("192.0.2.9")) && a.permits(ip("192.168.1.9")));
        // an empty value in the settings switches the image default off
        let (a, _) = Acl::from_texts(Some(image), Some("net.allow =\n"));
        assert!(a.is_open());
        assert_eq!(value("a = 1\n#a = 2\na = 3 # c\n", "a"), Some("3"));
    }
}
