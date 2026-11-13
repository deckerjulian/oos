// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Windows sockets and adapter addresses for the shared DNS-SD responder.

use super::{GROUP, MDNS_PORT};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::os::windows::io::{AsRawSocket, FromRawSocket};
use windows_sys::Win32::NetworkManagement::IpHelper::*;
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock as ws;

pub(super) fn host_label() -> String {
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    if (1..64).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        name
    } else {
        "oos".into()
    }
}

pub(super) fn addresses() -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    // An aligned working buffer; the adapter list contains pointers into this allocation.
    let mut size = 15_000u32;
    for _ in 0..3 {
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let result = unsafe { GetAdaptersAddresses(ws::AF_INET as u32, flags, std::ptr::null(), first, &mut size) };
        if result == 111 { // ERROR_BUFFER_OVERFLOW: the interface list changed or exceeded the initial buffer.
            continue;
        }
        if result != 0 {
            break;
        }
        unsafe {
            let mut adapter = first;
            while let Some(a) = adapter.as_ref() {
                if a.OperStatus == IfOperStatusUp {
                    let mut unicast = a.FirstUnicastAddress;
                    while let Some(u) = unicast.as_ref() {
                        let addr = u.Address.lpSockaddr;
                        if !addr.is_null() && u.Address.iSockaddrLength as usize >= std::mem::size_of::<ws::SOCKADDR_IN>() && (*addr).sa_family == ws::AF_INET {
                            let ip = Ipv4Addr::from((*addr.cast::<ws::SOCKADDR_IN>()).sin_addr.S_un.S_addr.to_ne_bytes());
                            if !ip.is_loopback() && !ip.is_unspecified() && !out.contains(&ip.octets()) {
                                out.push(ip.octets());
                            }
                        }
                        unicast = u.Next;
                    }
                }
                adapter = a.Next;
            }
        }
        break;
    }
    // Preserve adapter order here; the shared responder prefers the routed primary
    // address while retaining this order for the other addresses.
    out
}

pub(super) fn mcast_socket() -> Option<UdpSocket> {
    // Initialize Winsock through std before using the raw socket API.
    let _init = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    unsafe {
        let raw = ws::socket(ws::AF_INET as i32, ws::SOCK_DGRAM, 0);
        if raw == ws::INVALID_SOCKET {
            return None;
        }
        let one = 1i32;
        ws::setsockopt(raw, ws::SOL_SOCKET, ws::SO_REUSEADDR, (&one as *const i32).cast(), 4);
        let addr = ws::SOCKADDR_IN { sin_family: ws::AF_INET, sin_port: MDNS_PORT.to_be(), ..Default::default() };
        if ws::bind(raw, (&addr as *const ws::SOCKADDR_IN).cast(), std::mem::size_of_val(&addr) as i32) != 0 {
            ws::closesocket(raw);
            return None;
        }
        let sock = UdpSocket::from_raw_socket(raw as _);
        sock.set_nonblocking(true).ok()?;
        sock.set_multicast_ttl_v4(255).ok()?;
        sock.set_multicast_loop_v4(true).ok()?;
        Some(sock)
    }
}

pub(super) fn join(sock: &UdpSocket, addr: [u8; 4]) {
    let _ = sock.join_multicast_v4(&GROUP, &Ipv4Addr::from(addr));
}

pub(super) fn send_mcast(sock: &UdpSocket, packet: &[u8], iface: Option<[u8; 4]>) {
    if let Some(addr) = iface {
        let ip = u32::from_ne_bytes(addr);
        unsafe {
            ws::setsockopt(sock.as_raw_socket() as _, ws::IPPROTO_IP, ws::IP_MULTICAST_IF, (&ip as *const u32).cast(), 4);
        }
    }
    let _ = sock.send_to(packet, SocketAddrV4::new(GROUP, MDNS_PORT));
}
