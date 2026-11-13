// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! IPv6 for the services (fork, docs/network.md 8): with `net.ipv6 = on` the web remote, SCPI and VXI-11 listen on
//! `[::]` (both families: rcS sets `bindv6only = 0`, IPv4 peers arrive as `::ffff:a.b.c.d`, which `netacl` maps back)
//! and mDNS publishes the IPv6 addresses. Off (the default of other images): IPv4 only, as upstream. The addresses
//! themselves are the kernel's SLAAC addresses (`oos-net6`); `onlink6` in `net.allow` admits the prefixes on the link,
//! which `oos-firewall watch` keeps in `PREFIX_FILE`.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, UdpSocket};

/// The IPv6 prefixes on the link, one `addr/len` per line (`oos-firewall`).
pub const PREFIX_FILE: &str = "/tmp/oos-net6.prefixes";
/// Shortest prefix taken from the link: a router advertisement with a shorter on-link prefix (a rogue one with
/// `2000::/3`) must not open the scope to the internet. The same bound is in `oos-firewall`.
pub const PREFIX_MIN: u8 = 48;

/// `net.ipv6` (OOS_IPV6 overrides it), read once.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let env = std::env::var("OOS_IPV6").ok().map(|v| if v.is_empty() || v == "0" { "off" } else { "on" });
        enabled_from(env, &crate::sysconf::fork_text())
    })
}

/// `env` if set, else the `net.ipv6` line of `conf`; off unless `on`.
pub fn enabled_from(env: Option<&str>, conf: &str) -> bool {
    crate::drawpipe::conf_value(env, conf, "net.ipv6").as_deref() == Some("on")
}

/// `bind` on `[::]:port` when `v6`, else (or when IPv6 is not there) on `0.0.0.0:port`.
pub fn bind_any<T>(v6: bool, port: u16, bind: impl Fn(SocketAddr) -> std::io::Result<T>) -> Option<T> {
    if v6 {
        if let Ok(s) = bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))) {
            return Some(s);
        }
    }
    bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).ok()
}

/// A listening TCP socket for a service on all addresses of the families in use.
pub fn tcp(port: u16) -> Option<TcpListener> {
    bind_any(enabled(), port, TcpListener::bind)
}

pub fn udp(port: u16) -> Option<UdpSocket> {
    bind_any(enabled(), port, UdpSocket::bind)
}

/// The prefixes of a `PREFIX_FILE` text; what does not parse, is IPv4 or shorter than `PREFIX_MIN` is left out.
pub fn parse_prefixes(text: &str) -> Vec<(Ipv6Addr, u8)> {
    text.split_whitespace()
        .filter_map(|w| {
            let (a, n) = w.split_once('/')?;
            let n = n.parse::<u8>().ok().filter(|n| (PREFIX_MIN..=128).contains(n))?;
            Some((a.parse::<Ipv6Addr>().ok()?, n))
        })
        .collect()
}

/// The prefixes on the link now (none without the file).
pub fn onlink() -> Vec<(Ipv6Addr, u8)> {
    std::fs::read_to_string(PREFIX_FILE).map(|t| parse_prefixes(&t)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_and_prefixes() {
        assert!(!enabled_from(None, ""), "off by default");
        assert!(enabled_from(None, "net.ipv6 = on\n") && !enabled_from(Some("off"), "net.ipv6 = on\n"));
        let p = parse_prefixes("2001:db8:1:2::/64\nfd00:1::/56 10.0.0.0/8 2000::/3 x/64 2001:db8::/129 ::1/128\n");
        let want: Vec<(Ipv6Addr, u8)> = vec![("2001:db8:1:2::".parse().unwrap(), 64), ("fd00:1::".parse().unwrap(), 56), ("::1".parse().unwrap(), 128)];
        assert_eq!(p, want);
    }

    #[test]
    fn dual_stack_or_ipv4() {
        let l = bind_any(true, 0, TcpListener::bind).unwrap();
        // a host without IPv6 falls back to IPv4; with it the socket is [::]
        let a = l.local_addr().unwrap();
        assert!(a.ip().is_unspecified());
        let l4 = bind_any(false, 0, TcpListener::bind).unwrap();
        assert!(l4.local_addr().unwrap().is_ipv4());
    }
}
