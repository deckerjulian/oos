// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The network interfaces and their addresses as getifaddrs lists them, read once into plain values: the one unsafe
//! walk of that list for mDNS, the web remote's announcement and the top bar's address. A caller that needs more of
//! `struct ifaddrs` gets a field of `Entry` here, not a walk of its own.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One entry of the list: an interface with one of its addresses, or none (a link-layer entry).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// `IFF_*` of the interface
    pub flags: u32,
    /// the address, when it is IPv4 or IPv6
    pub addr: Option<IpAddr>,
    /// the netmask of an IPv4 address, when the list gives one
    pub netmask: Option<Ipv4Addr>,
}

impl Entry {
    /// Whether the interface has `flag` (`libc::IFF_UP`, ...).
    pub fn has(&self, flag: i32) -> bool {
        self.flags & flag as u32 != 0
    }

    /// The IPv4 address, if this entry has one.
    pub fn v4(&self) -> Option<Ipv4Addr> {
        match self.addr {
            Some(IpAddr::V4(a)) => Some(a),
            _ => None,
        }
    }
}

/// Every entry, in the order getifaddrs gives them; none when the call fails.
pub fn list() -> Vec<Entry> {
    let mut out = Vec::new();
    // SAFETY: the list from getifaddrs is read while it is alive and freed once; each address is read as the
    // sockaddr its family names
    unsafe {
        let mut ifa: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifa) != 0 {
            return out;
        }
        let mut p = ifa;
        while !p.is_null() {
            let a = &*p;
            p = a.ifa_next;
            let name = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
            let addr = if a.ifa_addr.is_null() {
                None
            } else {
                match (*a.ifa_addr).sa_family as i32 {
                    libc::AF_INET => {
                        let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                        Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr))))
                    }
                    libc::AF_INET6 => {
                        let sin = &*(a.ifa_addr as *const libc::sockaddr_in6);
                        Some(IpAddr::V6(Ipv6Addr::from(sin.sin6_addr.s6_addr)))
                    }
                    _ => None,
                }
            };
            let netmask = match addr {
                Some(IpAddr::V4(_)) if !a.ifa_netmask.is_null() => {
                    let m = &*(a.ifa_netmask as *const libc::sockaddr_in);
                    Some(Ipv4Addr::from(u32::from_be(m.sin_addr.s_addr)))
                }
                _ => None,
            };
            out.push(Entry { name, flags: a.ifa_flags, addr, netmask });
        }
        libc::freeifaddrs(ifa);
    }
    out
}

/// The index of interface `name` (if_nametoindex); 0 for none.
pub fn index_of(name: &str) -> u32 {
    // SAFETY: the name is a NUL-terminated string that lives for the call
    std::ffi::CString::new(name).map_or(0, |c| unsafe { libc::if_nametoindex(c.as_ptr()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host's loopback interface is listed, up, with 127.0.0.1 and its netmask, and has an index.
    #[test]
    fn lists_the_loopback_interface() {
        let all = list();
        let lo = all.iter().find(|e| e.v4() == Some(Ipv4Addr::LOCALHOST)).expect("an entry with 127.0.0.1");
        assert!(lo.has(libc::IFF_UP) && lo.has(libc::IFF_LOOPBACK), "{lo:?}");
        assert_eq!(lo.netmask, Some(Ipv4Addr::new(255, 0, 0, 0)), "{lo:?}");
        assert_ne!(index_of(&lo.name), 0, "{lo:?}");
        assert_eq!(index_of("no-such-interface"), 0);
        assert_eq!(index_of("a\0b"), 0, "a name with a NUL is none");
    }
}
