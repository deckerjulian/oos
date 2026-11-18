// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Who may control the scope over the network: `net.allow` in `/oos/system.conf`, with the image default from
//! `/etc/oos/system.conf` (docs/network.md 6). A list of address prefixes, `192.0.2.0/24 fd00::/8`. Empty or
//! absent: everyone, as before. rcS builds the firewall from the same key (`oos-firewall`); this list is the second
//! line behind it, for what can change the scope: `POST /update`, `/mcp`, SCPI, VXI-11 and HiSLIP. The scope itself
//! (loopback) is always allowed. `onlink6` stands for the IPv6 prefixes on the link (from the router's advertisements,
//! `net6::PREFIX_FILE`), which change with the provider.
//!
//! The servers hold one list (`Shared`) that Remote services > Allowed sets after it saved a change (ui/remote.rs):
//! every check takes the list of that moment, and each server closes the connections it holds from an address the new
//! list leaves out.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Acl {
    nets: Vec<(IpAddr, u8)>,
    /// `onlink6`: the IPv6 prefixes on the link too, read at each check
    onlink6: bool,
}

/// The `net.allow` word for the IPv6 prefixes on the link (`oos-firewall` has it too).
pub const ONLINK6: &str = "onlink6";

/// Longest `net.allow` taken from the keyboard (System > Network > Remote services > Allowed).
pub const ALLOW_MAX: usize = 160;

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
        if self.is_open()
            || ip.is_loopback()
            || self.nets.iter().any(|&(net, n)| net.is_ipv4() == ip.is_ipv4() && masked(ip, n) == net)
        {
            return true;
        }
        self.onlink6 && ip.is_ipv6() && onlink().into_iter().any(|(net, n)| masked(ip, n) == masked(IpAddr::V6(net), n))
    }

    /// From the image default and the settings file (the file wins, also with an empty value). A list that names
    /// something but of which nothing parses closes everything but loopback rather than opening it.
    pub fn from_texts(image: Option<&str>, settings: Option<&str>) -> (Acl, Vec<String>) {
        let v = settings.and_then(|t| crate::sysconf::value(t, "net.allow"));
        let v = v.or_else(|| image.and_then(|t| crate::sysconf::value(t, "net.allow"))).unwrap_or("");
        Acl::from_value(v)
    }

    /// The value of `net.allow` in force (the settings' line or the image default), as `from_texts` takes it.
    pub fn from_value(v: &str) -> (Acl, Vec<String>) {
        let (mut acl, bad) = Acl::parse(v);
        if acl.is_open() && !bad.is_empty() {
            acl.nets.push(("::1".parse().unwrap(), 128));
        }
        (acl, bad)
    }

    /// The image defaults (`sysconf::IMAGE_PATH`) and the settings file `path`; problems go to the log.
    pub fn load(path: &Path) -> Acl {
        let image = std::fs::read_to_string(crate::sysconf::IMAGE_PATH).ok();
        let settings = std::fs::read_to_string(path).ok();
        let (acl, bad) = Acl::from_texts(image.as_deref(), settings.as_deref());
        for b in bad {
            eprintln!("oos-ui: net.allow: {:?} is not an address prefix, left out", b);
        }
        acl.log();
        acl
    }

    /// The list in the log, when there is one.
    pub fn log(&self) {
        if !self.is_open() {
            eprintln!("oos-ui: update, MCP, SCPI, VXI-11 and HiSLIP only from {}", self);
        }
    }
}

/// The list the servers check, one for all of them: Remote services > Allowed sets it after it saved a change. Each
/// check takes the list of that moment; `changed` tells a server to look again at the connections it holds.
#[derive(Clone, Default)]
pub struct Shared(Arc<SharedList>);

#[derive(Default)]
struct SharedList {
    acl: RwLock<Acl>,
    /// counts the changes (`set`)
    changes: AtomicU64,
    /// tests: every peer is taken to be this address (a test connects over loopback, which is always allowed)
    #[cfg(test)]
    peers_as: std::sync::Mutex<Option<IpAddr>>,
}

impl Shared {
    /// The list from now on, for new connections and those held.
    pub fn set(&self, acl: Acl) {
        *self.0.acl.write().unwrap_or_else(PoisonError::into_inner) = acl;
        self.0.changes.fetch_add(1, Ordering::SeqCst);
    }

    pub fn permits(&self, ip: IpAddr) -> bool {
        self.0.acl.read().unwrap_or_else(PoisonError::into_inner).permits(ip)
    }

    /// The peer of a connection (`TcpStream::peer_addr`, or the address `accept` gave); none is refused.
    pub fn permits_peer(&self, peer: std::io::Result<SocketAddr>) -> bool {
        #[cfg(test)]
        if let Some(ip) = *self.0.peers_as.lock().unwrap_or_else(PoisonError::into_inner) {
            return self.permits(ip);
        }
        peer.is_ok_and(|a| self.permits(a.ip()))
    }

    /// True once after each change since the server last asked (`seen` is its count of changes).
    pub fn changed(&self, seen: &mut u64) -> bool {
        let n = self.0.changes.load(Ordering::SeqCst);
        std::mem::replace(seen, n) != n
    }

    /// Tests: every peer is taken to be `ip` (None: its own address again).
    #[cfg(test)]
    pub fn peers_as(&self, ip: Option<IpAddr>) {
        *self.0.peers_as.lock().unwrap() = ip;
    }
}

/// A `net.allow` value from the screen in the form it is written: the prefixes of `Acl::parse` with the host bits
/// cleared, a bare address as `/32` or `/128`, `onlink6` last; empty for an empty entry. Err names the words not
/// taken: what `parse` leaves out, and an IPv6 address written with dotted IPv4, which `oos-firewall` leaves out. A list
/// with such a word is refused whole, so a typo never reaches the firewall (which would close or open the wrong way).
pub fn canonical(s: &str) -> Result<String, String> {
    let bad: Vec<&str> = s
        .split([' ', ',', '\t'])
        .filter(|w| !w.is_empty())
        .filter(|w| {
            let (a, bad) = Acl::parse(w);
            let t = a.to_string();
            !bad.is_empty() || (t.contains(':') && t.contains('.'))
        })
        .collect();
    if !bad.is_empty() {
        return Err(format!("Not an address prefix: {}", bad.join(" ")));
    }
    Ok(Acl::parse(s).0.to_string())
}

/// The network of the scope's IPv4 address `a` with its netmask, `192.168.1.0/24`: what Allowed offers. None for a
/// mask that is no prefix or is empty (that would be everyone) and for loopback or no address.
pub fn lan_of(a: Ipv4Addr, mask: Ipv4Addr) -> Option<String> {
    let m = u32::from(mask);
    let n = m.leading_ones();
    if n == 0 || m.count_ones() != n || a.is_loopback() || a.is_unspecified() {
        return None;
    }
    Some(format!("{}/{}", masked(IpAddr::V4(a), n as u8), n))
}

/// The networks Allowed offers, from every IPv4 address of the scope with its netmask (`lan_of`), each once: the
/// link-local one (169.254.0.0/16) only when there is no other, since a list of it alone shuts out every computer
/// that reaches the scope through a router or by a DHCP address (unit 2026-11-16: the top bar showed the link-local
/// address of the Ethernet port, which also had 10.20.0.51/24, and the WLAN had 192.168.177.75/24).
pub fn lans(addrs: &[(Ipv4Addr, Ipv4Addr)]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for &(a, m) in addrs.iter().filter(|x| !x.0.is_link_local()).chain(addrs.iter().filter(|x| x.0.is_link_local())) {
        if a.is_link_local() && !v.is_empty() {
            break;
        }
        if let Some(l) = lan_of(a, m).filter(|l| !v.contains(l)) {
            v.push(l);
        }
    }
    v
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
        let image = "# unit\nnet.allow = 192.0.2.0/24\nremote.ssh = on\n";
        let (a, _) = Acl::from_texts(Some(image), Some("ntp = on\n"));
        assert!(a.permits(ip("192.0.2.9")) && !a.permits(ip("192.168.1.9")));
        let (a, _) = Acl::from_texts(Some(image), Some("net.allow = 192.168.1.0/24 # new LAN\n"));
        assert!(!a.permits(ip("192.0.2.9")) && a.permits(ip("192.168.1.9")));
        // an empty value in the settings switches the image default off
        let (a, _) = Acl::from_texts(Some(image), Some("net.allow =\n"));
        assert!(a.is_open());
    }

    // Break it catches: an entry from the screen stored with a word the firewall leaves out (a list of which nothing
    // is usable closes the scope), or stored with host bits that nft may refuse
    #[test]
    fn an_entry_from_the_screen() {
        assert_eq!(canonical("").as_deref(), Ok(""));
        assert_eq!(canonical("  ").as_deref(), Ok(""));
        assert_eq!(
            canonical("192.0.2.7/24, onlink6 fd00::1:0/8 10.1.2.3").as_deref(),
            Ok("192.0.2.0/24 fd00::/8 10.1.2.3/32 onlink6")
        );
        assert_eq!(canonical("169.254.0.0/16").as_deref(), Ok("169.254.0.0/16"));
        for (s, bad) in [
            ("192.0.2.0/33", "192.0.2.0/33"),
            ("192.0.2.0/24 x", "x"),
            ("300.1.1.1", "300.1.1.1"),
            ("::ffff:192.0.2.1", "::ffff:192.0.2.1"),
            ("::ffff:c000:201/128", "::ffff:c000:201/128"),
            ("fe80::1%eth0", "fe80::1%eth0"),
            ("010.0.0.1", "010.0.0.1"),
            ("192.0.2.0/24;reboot", "192.0.2.0/24;reboot"),
        ] {
            assert_eq!(canonical(s), Err(format!("Not an address prefix: {}", bad)), "{}", s);
        }
        // what is written reads back as the same list, and the firewall's checks take every word of it
        let c = canonical("192.0.2.200/25 2001:db8::/32 onlink6").unwrap();
        assert_eq!(Acl::parse(&c), (Acl::parse("192.0.2.128/25 2001:db8::/32 onlink6").0, Vec::new()));
    }

    // Break it catches: a server that kept the list of the start (a change on the screen counted only after a restart),
    // or never looked again at the connections it held
    #[test]
    fn one_list_for_every_server_that_follows_a_change() {
        let a = Shared::default();
        let b = a.clone();
        let (mut seen_a, mut seen_b) = (0, 0);
        assert!(!a.changed(&mut seen_a) && b.permits(ip("198.51.100.7")), "open, as without net.allow");
        a.set(Acl::parse("192.0.2.0/24").0);
        assert!(!b.permits(ip("198.51.100.7")) && b.permits(ip("192.0.2.7")) && b.permits(ip("127.0.0.1")));
        assert!(b.changed(&mut seen_b) && !b.changed(&mut seen_b), "once per change");
        assert!(a.changed(&mut seen_a), "each server sees it");
        let peer = |s: &str| Ok(SocketAddr::new(ip(s), 5555));
        assert!(b.permits_peer(peer("192.0.2.1")) && !b.permits_peer(peer("198.51.100.7")));
        assert!(!b.permits_peer(Err(std::io::ErrorKind::NotConnected.into())), "no address: refused");
        a.set(Acl::from_value("").0);
        assert!(b.changed(&mut seen_b) && b.permits_peer(peer("198.51.100.7")));
    }

    #[test]
    fn the_network_the_scope_is_on() {
        let v4 = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert_eq!(lan_of(v4("192.168.1.148"), v4("255.255.255.0")).as_deref(), Some("192.168.1.0/24"));
        assert_eq!(lan_of(v4("169.254.7.9"), v4("255.255.0.0")).as_deref(), Some("169.254.0.0/16"));
        assert_eq!(lan_of(v4("10.1.2.3"), v4("255.255.255.255")).as_deref(), Some("10.1.2.3/32"));
        assert_eq!(lan_of(v4("10.1.2.3"), v4("0.0.0.0")), None, "no mask: that would be everyone");
        assert_eq!(lan_of(v4("10.1.2.3"), v4("255.0.255.0")), None);
        assert_eq!(lan_of(v4("127.0.0.1"), v4("255.0.0.0")), None);
        // every network once, the link-local one only alone
        let m24 = v4("255.255.255.0");
        let ll = (v4("169.254.124.87"), v4("255.255.0.0"));
        assert_eq!(
            lans(&[ll, (v4("10.20.0.51"), m24), (v4("192.168.177.75"), m24), (v4("10.20.0.9"), m24)]),
            vec!["10.20.0.0/24", "192.168.177.0/24"]
        );
        assert_eq!(lans(&[ll]), vec!["169.254.0.0/16"]);
        assert!(lans(&[]).is_empty());
    }
}
