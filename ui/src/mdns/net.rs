// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The sockets of the responder: the multicast sockets for IPv4 and IPv6 with their options, the list of interfaces
//! and their addresses (`interfaces`, `ipv4_all`), and sending and receiving with the interface a packet came in on
//! (`Net`). `Mdns`'s thread in `mdns.rs` uses it; a new socket option or address family goes here.

use super::*;
use std::io;
use std::net::UdpSocket;
use std::os::fd::{AsRawFd, FromRawFd};

fn opt(fd: i32, level: i32, name: i32, v: i32) -> io::Result<()> {
    // SAFETY: a pointer to an int of the size given, for the duration of the call
    let r = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            &v as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A UDP socket on port 5353 that the system's own responder (a host with one) may share: SO_REUSEADDR and
/// SO_REUSEPORT before the bind, the receiving interface with each packet (PKTINFO).
fn open(v6: bool) -> io::Result<UdpSocket> {
    let fam = if v6 { libc::AF_INET6 } else { libc::AF_INET };
    // SAFETY: plain system calls on a descriptor owned here; the address structures are fully initialised
    unsafe {
        let fd = libc::socket(fam, libc::SOCK_DGRAM, 0);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let s = UdpSocket::from_raw_fd(fd);
        opt(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, 1)?;
        opt(fd, libc::SOL_SOCKET, libc::SO_REUSEPORT, 1)?;
        let r = if v6 {
            opt(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, 1)?;
            opt(fd, libc::IPPROTO_IPV6, libc::IPV6_RECVPKTINFO, 1)?;
            let mut a: libc::sockaddr_in6 = std::mem::zeroed();
            a.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            a.sin6_port = PORT.to_be();
            libc::bind(
                fd,
                &a as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        } else {
            opt(fd, libc::IPPROTO_IP, libc::IP_PKTINFO, 1)?;
            let mut a: libc::sockaddr_in = std::mem::zeroed();
            a.sin_family = libc::AF_INET as libc::sa_family_t;
            a.sin_port = PORT.to_be();
            libc::bind(
                fd,
                &a as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };
        if r != 0 {
            return Err(io::Error::last_os_error());
        }
        if v6 {
            s.set_multicast_loop_v6(true)?;
            opt(fd, libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_HOPS, 255)?;
        } else {
            s.set_multicast_loop_v4(true)?;
            s.set_multicast_ttl_v4(255)?;
        }
        s.set_nonblocking(true)?;
        Ok(s)
    }
}

/// The interfaces that are up, running and take multicast, with their addresses. `only`: these names (a loopback
/// interface too, for a test on a host), else every one but loopback.
pub fn interfaces(only: Option<&[String]>) -> Vec<Iface> {
    let mut out: Vec<Iface> = Vec::new();
    // SAFETY: the list from getifaddrs is read while it is alive and freed once
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
            let f = a.ifa_flags;
            let flag = |x: i32| f & (x as u32) != 0;
            let wanted = match only {
                Some(l) => l.contains(&name),
                None => !flag(libc::IFF_LOOPBACK),
            };
            if !wanted
                || !flag(libc::IFF_UP)
                || !flag(libc::IFF_RUNNING)
                || !flag(libc::IFF_MULTICAST)
                || a.ifa_addr.is_null()
            {
                continue;
            }
            let index = libc::if_nametoindex(a.ifa_name);
            if index == 0 {
                continue;
            }
            let i = match out.iter().position(|i| i.index == index) {
                Some(i) => i,
                None => {
                    out.push(Iface { index, name, v4: None, v6: Vec::new() });
                    out.len() - 1
                }
            };
            match (*a.ifa_addr).sa_family as i32 {
                libc::AF_INET if out[i].v4.is_none() => {
                    let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                    let mask = if a.ifa_netmask.is_null() {
                        0
                    } else {
                        u32::from_be((*(a.ifa_netmask as *const libc::sockaddr_in)).sin_addr.s_addr)
                    };
                    out[i].v4 = Some((Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)), Ipv4Addr::from(mask)));
                }
                libc::AF_INET6 => {
                    let sin = &*(a.ifa_addr as *const libc::sockaddr_in6);
                    out[i].v6.push(Ipv6Addr::from(sin.sin6_addr.s6_addr));
                }
                _ => {}
            }
        }
        libc::freeifaddrs(ifa);
    }
    out.retain(|i| i.v4.is_some() || !i.v6.is_empty());
    out.sort_by_key(|i| i.index);
    out
}

/// `ipv4_networks`: every IPv4 address of the interfaces that are up, with its netmask.
pub fn ipv4_all() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    let mut out = Vec::new();
    // SAFETY: the list from getifaddrs is read while it is alive and freed once
    unsafe {
        let mut ifa: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifa) != 0 {
            return out;
        }
        let mut p = ifa;
        while !p.is_null() {
            let a = &*p;
            p = a.ifa_next;
            let f = a.ifa_flags;
            if f & (libc::IFF_LOOPBACK as u32) != 0
                || f & (libc::IFF_UP as u32) == 0
                || a.ifa_addr.is_null()
                || a.ifa_netmask.is_null()
            {
                continue;
            }
            if (*a.ifa_addr).sa_family as i32 == libc::AF_INET {
                let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                let mask = u32::from_be((*(a.ifa_netmask as *const libc::sockaddr_in)).sin_addr.s_addr);
                out.push((Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)), Ipv4Addr::from(mask)));
            }
        }
        libc::freeifaddrs(ifa);
    }
    out
}

/// The sockets and the groups joined on them.
pub struct Net {
    s4: UdpSocket,
    s6: Option<UdpSocket>,
    joined4: Vec<(u32, Ipv4Addr)>,
    joined6: Vec<u32>,
}

/// A received packet: length, sender, interface index, whether it went to the group.
type Got = (usize, SocketAddr, u32, bool);

impl Net {
    pub fn open() -> io::Result<Net> {
        let s4 = open(false)?;
        let s6 = match open(true) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("oos-ui: mDNS: no IPv6 ({})", e);
                None
            }
        };
        Ok(Net { s4, s6, joined4: Vec::new(), joined6: Vec::new() })
    }

    /// The groups on the interfaces as they are now: left where an interface or its address went, joined where
    /// one came.
    pub fn join(&mut self, ifs: &[Iface]) {
        let want4: Vec<(u32, Ipv4Addr)> = ifs.iter().filter_map(|i| i.v4.map(|(a, _)| (i.index, a))).collect();
        for j in self.joined4.iter().filter(|j| !want4.contains(j)) {
            let _ = self.s4.leave_multicast_v4(&GROUP4, &j.1);
        }
        for j in want4.iter().filter(|j| !self.joined4.contains(j)) {
            if let Err(e) = self.s4.join_multicast_v4(&GROUP4, &j.1) {
                if e.kind() != io::ErrorKind::AddrInUse {
                    eprintln!("oos-ui: mDNS: joining 224.0.0.251 on {}: {}", j.1, e);
                }
            }
        }
        self.joined4 = want4;
        let Some(s6) = &self.s6 else { return };
        let want6: Vec<u32> = ifs.iter().filter(|i| !i.v6.is_empty()).map(|i| i.index).collect();
        for j in self.joined6.iter().filter(|j| !want6.contains(j)) {
            let _ = s6.leave_multicast_v6(&GROUP6, *j);
        }
        for j in want6.iter().filter(|j| !self.joined6.contains(j)) {
            if let Err(e) = s6.join_multicast_v6(&GROUP6, *j) {
                if e.kind() != io::ErrorKind::AddrInUse {
                    eprintln!("oos-ui: mDNS: joining ff02::fb on interface {}: {}", j, e);
                }
            }
        }
        self.joined6 = want6;
    }

    /// Waits up to `secs` for a packet on either socket.
    pub fn wait(&self, secs: f64) {
        let mut fds = vec![libc::pollfd { fd: self.s4.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        if let Some(s) = &self.s6 {
            fds.push(libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLIN, revents: 0 });
        }
        let ms = (secs * 1000.0).clamp(0.0, 60_000.0).ceil() as i32;
        // SAFETY: the array lives for the call
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
    }

    /// The next waiting packet of either socket, without waiting.
    pub fn recv(&self, buf: &mut [u8]) -> Option<Got> {
        recv_on(&self.s4, buf).or_else(|| self.s6.as_ref().and_then(|s| recv_on(s, buf)))
    }

    pub fn send(&self, o: &Out, ifs: &[Iface]) {
        let Some(ifc) = ifs.iter().find(|i| i.index == o.iface) else { return };
        let r = match &o.dest {
            Dest::Group4 => {
                let Some((a, _)) = ifc.v4 else { return };
                let ia = libc::in_addr { s_addr: u32::from(a).to_be() };
                // SAFETY: a pointer to an in_addr of the size given
                unsafe {
                    libc::setsockopt(
                        self.s4.as_raw_fd(),
                        libc::IPPROTO_IP,
                        libc::IP_MULTICAST_IF,
                        &ia as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::in_addr>() as libc::socklen_t,
                    )
                };
                self.s4.send_to(&o.pkt, (GROUP4, PORT))
            }
            Dest::Group6 => {
                let Some(s6) = &self.s6 else { return };
                let _ = opt(s6.as_raw_fd(), libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_IF, o.iface as i32);
                s6.send_to(&o.pkt, std::net::SocketAddrV6::new(GROUP6, PORT, 0, o.iface))
            }
            Dest::To(a @ SocketAddr::V4(_)) => self.s4.send_to(&o.pkt, a),
            Dest::To(a) => match &self.s6 {
                Some(s6) => s6.send_to(&o.pkt, a),
                None => return,
            },
        };
        if let Err(e) = r {
            // a cable pulled between the interface check and the send: the next check takes the interface away
            if e.kind() != io::ErrorKind::WouldBlock {
                eprintln!("oos-ui: mDNS: send on {}: {}", ifc.name, e);
            }
        }
    }
}

/// One packet with the interface it came in on and its destination (IP_PKTINFO / IPV6_PKTINFO).
fn recv_on(s: &UdpSocket, buf: &mut [u8]) -> Option<Got> {
    // SAFETY: recvmsg fills the buffers given; the control messages are read within the length it reports
    unsafe {
        let mut from: libc::sockaddr_storage = std::mem::zeroed();
        let mut iov = libc::iovec { iov_base: buf.as_mut_ptr() as *mut libc::c_void, iov_len: buf.len() };
        let mut ctl = [0u64; 32];
        let mut mh: libc::msghdr = std::mem::zeroed();
        mh.msg_name = &mut from as *mut _ as *mut libc::c_void;
        mh.msg_namelen = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        mh.msg_iov = &mut iov;
        mh.msg_iovlen = 1;
        mh.msg_control = ctl.as_mut_ptr() as *mut libc::c_void;
        mh.msg_controllen = std::mem::size_of_val(&ctl) as _;
        let n = libc::recvmsg(s.as_raw_fd(), &mut mh, libc::MSG_DONTWAIT);
        if n < 0 {
            return None;
        }
        let src = match from.ss_family as i32 {
            libc::AF_INET => {
                let a = &*(&from as *const _ as *const libc::sockaddr_in);
                SocketAddr::from((Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)), u16::from_be(a.sin_port)))
            }
            libc::AF_INET6 => {
                let a = &*(&from as *const _ as *const libc::sockaddr_in6);
                SocketAddr::V6(std::net::SocketAddrV6::new(
                    Ipv6Addr::from(a.sin6_addr.s6_addr),
                    u16::from_be(a.sin6_port),
                    0,
                    a.sin6_scope_id,
                ))
            }
            _ => return Some((0, SocketAddr::from(([0, 0, 0, 0], 0)), 0, false)),
        };
        let (mut index, mut group) = (0u32, true);
        let mut c = libc::CMSG_FIRSTHDR(&mh);
        while !c.is_null() {
            let h = &*c;
            if h.cmsg_level == libc::IPPROTO_IP && h.cmsg_type == libc::IP_PKTINFO {
                let pi = std::ptr::read_unaligned(libc::CMSG_DATA(c) as *const libc::in_pktinfo);
                index = pi.ipi_ifindex as u32;
                group = Ipv4Addr::from(u32::from_be(pi.ipi_addr.s_addr)).is_multicast();
            } else if h.cmsg_level == libc::IPPROTO_IPV6 && h.cmsg_type == libc::IPV6_PKTINFO {
                let pi = std::ptr::read_unaligned(libc::CMSG_DATA(c) as *const libc::in6_pktinfo);
                index = pi.ipi6_ifindex as u32;
                group = Ipv6Addr::from(pi.ipi6_addr.s6_addr).is_multicast();
            }
            c = libc::CMSG_NXTHDR(&mh, c);
        }
        if let SocketAddr::V6(a) = src {
            if index == 0 {
                index = a.scope_id();
            }
        }
        Some((n as usize, src, index, group))
    }
}
