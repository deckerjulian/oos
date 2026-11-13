// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! LAN announcement (mDNS, RFC 6762, and DNS-SD, RFC 6763) so a controller can find the instrument
//! without a typed address.
//!
//! The HTTP service (`_http._tcp`) and the LXI service (`_lxi._tcp`) carry `path=/lxi/identification`,
//! the identification page the web server already serves. Beside that, each listener that is actually
//! up is announced: the SCPI socket (`_scpi-raw._tcp`), VXI-11 (`_vxi-11._tcp`, on the scope, where port
//! 111 is free) and HiSLIP (`_hislip._tcp`, on a computer).

use std::net::{Ipv4Addr, UdpSocket};
#[cfg(unix)]
use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::{addresses, host_label, join, mcast_socket, send_mcast};

const MDNS_PORT: u16 = 5353;
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
/// Identification page the web server already serves.
pub const IDENTIFICATION_PATH: &str = "/lxi/identification";
const TTL: u32 = 120;
const A: u16 = 1;
const PTR: u16 = 12;
const TXT: u16 = 16;
const SRV: u16 = 33;
const ANY: u16 = 255;
const IN: u16 = 1;
const FLUSH: u16 = 0x8001;

/// One service that is listening.
#[derive(Clone, Debug, PartialEq)]
pub struct Svc {
    pub typ: &'static str,
    pub port: u16,
    /// The port is the web server, so the record points at the identification page.
    pub page: bool,
}

/// The listeners that are up. A missing one is not announced.
pub fn listening(http: Option<u16>, scpi: Option<u16>, vxi11: Option<u16>, hislip: Option<u16>) -> Vec<Svc> {
    let mut v = Vec::new();
    if let Some(p) = http {
        v.push(Svc { typ: "_http._tcp", port: p, page: true });
        v.push(Svc { typ: "_lxi._tcp", port: p, page: true });
    }
    if let Some(p) = scpi {
        v.push(Svc { typ: "_scpi-raw._tcp", port: p, page: false });
    }
    if let Some(p) = vxi11 {
        v.push(Svc { typ: "_vxi-11._tcp", port: p, page: false });
    }
    if let Some(p) = hislip {
        v.push(Svc { typ: "_hislip._tcp", port: p, page: false });
    }
    v
}

/// Names and addresses put in the records.
#[derive(Clone, Debug)]
pub struct Identity {
    pub instance: String,
    pub host: String,
    pub manufacturer: String,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub addresses: Vec<[u8; 4]>,
}

impl Identity {
    pub fn now(addresses: Vec<[u8; 4]>) -> Identity {
        let p = oos_hal::model::platform();
        let model = p.caps.name.to_string();
        let serial = p.serial.clone().unwrap_or_else(|| "0".into());
        let host = host_label();
        Identity {
            instance: clip(&format!("OOS {model} {host}"), 63),
            host,
            manufacturer: "OOS".into(),
            model,
            serial,
            firmware: format!("oos-ui {}", env!("CARGO_PKG_VERSION")),
            addresses,
        }
    }
}

fn clip(s: &str, n: usize) -> String {
    let mut o: String = s.chars().filter(|c| c.is_ascii() && !c.is_control() && *c != '.').take(n).collect();
    if o.is_empty() {
        o = "OOS".into();
    }
    o
}

#[cfg(unix)]
fn host_label() -> String {
    let mut b = [0u8; 256];
    let rc = unsafe { libc::gethostname(b.as_mut_ptr() as *mut libc::c_char, b.len()) };
    if rc != 0 {
        return "oos".into();
    }
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    let s = String::from_utf8_lossy(&b[..n]);
    let ok = (1..64).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') && !s.eq_ignore_ascii_case("localhost");
    if ok {
        s.to_string()
    } else {
        "oos".into()
    }
}

#[derive(Clone)]
struct Rr {
    name: String,
    typ: u16,
    class: u16,
    ttl: u32,
    rdata: Vec<u8>,
}

fn put_u16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_be_bytes());
}
fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_be_bytes());
}

fn put_name(o: &mut Vec<u8>, name: &str) {
    for label in name.trim_end_matches('.').split('.') {
        let b = label.as_bytes();
        o.push(b.len().min(63) as u8);
        o.extend_from_slice(&b[..b.len().min(63)]);
    }
    o.push(0);
}

fn txt(items: &[String]) -> Vec<u8> {
    let mut o = Vec::new();
    for s in items {
        let b = s.as_bytes();
        let n = b.len().min(255);
        o.push(n as u8);
        o.extend_from_slice(&b[..n]);
    }
    if o.is_empty() {
        o.push(0);
    }
    o
}

fn instance_fqdn(id: &Identity, typ: &str) -> String {
    format!("{}.{typ}.local", id.instance)
}

fn service_fqdn(typ: &str) -> String {
    format!("{typ}.local")
}

fn host_fqdn(id: &Identity) -> String {
    format!("{}.local", id.host)
}

fn txt_of(id: &Identity, svc: &Svc) -> Vec<String> {
    let mut t = vec![
        "txtvers=1".into(),
        format!("Manufacturer={}", id.manufacturer),
        format!("Model={}", id.model),
        format!("SerialNumber={}", id.serial),
        format!("FirmwareVersion={}", id.firmware),
    ];
    if svc.page {
        t.push(format!("path={IDENTIFICATION_PATH}"));
    } else {
        let host = host_fqdn(id);
        let addr = match svc.typ {
            "_scpi-raw._tcp" => Some(format!("TCPIP::{host}::{}::SOCKET", svc.port)),
            "_hislip._tcp" => Some(if svc.port == crate::hislip::PORT {
                format!("TCPIP::{host}::hislip0::INSTR")
            } else {
                format!("TCPIP::{host}::hislip0,{}::INSTR", svc.port)
            }),
            "_vxi-11._tcp" => Some(format!("TCPIP::{host}::inst0::INSTR")),
            _ => None,
        };
        if let Some(a) = addr {
            t.push(format!("Address={a}"));
        }
    }
    t
}

fn service_rrs(id: &Identity, svc: &Svc, ttl: u32) -> Vec<Rr> {
    let inst = instance_fqdn(id, svc.typ);
    let st = service_fqdn(svc.typ);
    let host = host_fqdn(id);
    let mut ptr = Vec::new();
    put_name(&mut ptr, &inst);
    let mut srv = Vec::new();
    put_u16(&mut srv, 0);
    put_u16(&mut srv, 0);
    put_u16(&mut srv, svc.port);
    put_name(&mut srv, &host);
    vec![
        Rr { name: st, typ: PTR, class: IN, ttl, rdata: ptr },
        Rr { name: inst.clone(), typ: SRV, class: FLUSH, ttl, rdata: srv },
        Rr { name: inst, typ: TXT, class: FLUSH, ttl, rdata: txt(&txt_of(id, svc)) },
    ]
}

fn address_rrs(id: &Identity, ttl: u32) -> Vec<Rr> {
    let host = host_fqdn(id);
    id.addresses.iter().map(|a| Rr { name: host.clone(), typ: A, class: FLUSH, ttl, rdata: a.to_vec() }).collect()
}

fn pack(id: u16, answer: &[Rr], extra: &[Rr]) -> Vec<u8> {
    let mut o = Vec::new();
    put_u16(&mut o, id);
    put_u16(&mut o, 0x8400);
    put_u16(&mut o, 0);
    put_u16(&mut o, answer.len() as u16);
    put_u16(&mut o, 0);
    put_u16(&mut o, extra.len() as u16);
    for r in answer.iter().chain(extra) {
        put_name(&mut o, &r.name);
        put_u16(&mut o, r.typ);
        put_u16(&mut o, r.class);
        put_u32(&mut o, r.ttl);
        put_u16(&mut o, r.rdata.len() as u16);
        o.extend_from_slice(&r.rdata);
    }
    o
}

/// Unsolicited announcement of every service, with the records in the answer section.
pub fn advertisement(id: &Identity, services: &[Svc]) -> Vec<u8> {
    let mut answer = Vec::new();
    for s in services {
        answer.extend(service_rrs(id, s, TTL));
    }
    answer.extend(address_rrs(id, TTL));
    pack(0, &answer, &[])
}

struct Question {
    name: String,
    typ: u16,
    unicast: bool,
}

fn read_name(msg: &[u8], at: usize) -> Option<(String, usize)> {
    let mut p = at;
    let mut out = String::new();
    let mut jumped = false;
    let mut end = at;
    for _ in 0..20 {
        let n = *msg.get(p)? as usize;
        if n == 0 {
            if !jumped {
                end = p + 1;
            }
            return Some((out.to_ascii_lowercase(), end));
        }
        if n & 0xC0 == 0xC0 {
            let b = *msg.get(p + 1)? as usize;
            if !jumped {
                end = p + 2;
            }
            p = ((n & 0x3F) << 8) | b;
            jumped = true;
            continue;
        }
        if n & 0xC0 != 0 || p + 1 + n > msg.len() {
            return None;
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&String::from_utf8_lossy(&msg[p + 1..p + 1 + n]));
        p += 1 + n;
        if !jumped {
            end = p;
        }
    }
    None
}

fn be_u16(msg: &[u8], p: usize) -> Option<u16> {
    let b: [u8; 2] = msg.get(p..p + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(b))
}

fn read_questions(msg: &[u8]) -> Option<(Vec<Question>, Vec<(String, u16)>, usize)> {
    if msg.len() < 12 {
        return None;
    }
    let flags = u16::from_be_bytes([msg[2], msg[3]]);
    if flags & 0x8000 != 0 || flags & 0x7800 != 0 {
        return None; // a response, or not a standard query
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut p = 12;
    let mut qs = Vec::new();
    for _ in 0..qd {
        let (name, n) = read_name(msg, p)?;
        p = n;
        let typ = be_u16(msg, p)?;
        let class = be_u16(msg, p + 2)?;
        p += 4;
        qs.push(Question { name, typ, unicast: class & 0x8000 != 0 });
    }
    let mut known = Vec::new();
    for _ in 0..an {
        let (name, n) = read_name(msg, p)?;
        p = n;
        let typ = be_u16(msg, p)?;
        p += 8; // type, class, ttl
        let rd = be_u16(msg, p)? as usize;
        p += 2 + rd;
        known.push((name, typ));
    }
    Some((qs, known, u16::from_be_bytes([msg[0], msg[1]]) as usize))
}

fn wants(q: &Question, name: &str, typ: u16) -> bool {
    q.name == name.to_ascii_lowercase() && (q.typ == typ || q.typ == ANY)
}

/// A response to one query, or nothing when the query is not for us.
pub fn answer(query: &[u8], id: &Identity, services: &[Svc]) -> Option<(Vec<u8>, bool)> {
    let (qs, known, qid) = read_questions(query)?;
    let mut ans = Vec::new();
    let mut extra = Vec::new();
    let mut unicast = false;
    let host = host_fqdn(id).to_ascii_lowercase();
    for q in &qs {
        unicast |= q.unicast;
        if q.name == "_services._dns-sd._udp.local" && (q.typ == PTR || q.typ == ANY) {
            for s in services {
                let mut rdata = Vec::new();
                put_name(&mut rdata, &service_fqdn(s.typ));
                ans.push(Rr { name: "_services._dns-sd._udp.local".into(), typ: PTR, class: IN, ttl: TTL, rdata });
            }
            continue;
        }
        if wants(q, &host, A) {
            ans.extend(address_rrs(id, TTL));
            continue;
        }
        for s in services {
            let st = service_fqdn(s.typ).to_ascii_lowercase();
            let inst = instance_fqdn(id, s.typ).to_ascii_lowercase();
            let rrs = service_rrs(id, s, TTL);
            if wants(q, &st, PTR) {
                ans.push(rrs[0].clone());
                extra.extend(rrs.into_iter().skip(1));
                extra.extend(address_rrs(id, TTL));
            } else if wants(q, &inst, SRV) || wants(q, &inst, TXT) {
                ans.extend(rrs.into_iter().filter(|r| r.typ == q.typ || q.typ == ANY));
                extra.extend(address_rrs(id, TTL));
            }
        }
    }
    let known_has = |r: &Rr| known.iter().any(|(n, t)| n == &r.name.to_ascii_lowercase() && *t == r.typ);
    ans.retain(|r| !known_has(r));
    extra.retain(|r| !known_has(r));
    if ans.is_empty() && extra.is_empty() {
        return None;
    }
    Some((pack(qid as u16, &ans, &extra), unicast))
}

/// Non-loopback IPv4 addresses that are up.
#[cfg(unix)]
fn addresses() -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    unsafe {
        let mut ifa: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifa) != 0 {
            return out;
        }
        let mut p = ifa;
        while !p.is_null() {
            let a = &*p;
            let up = a.ifa_flags & (libc::IFF_UP as u32) != 0 && a.ifa_flags & (libc::IFF_LOOPBACK as u32) == 0;
            if up && !a.ifa_addr.is_null() && (*a.ifa_addr).sa_family as i32 == libc::AF_INET {
                let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                out.push(u32::from_be(sin.sin_addr.s_addr).to_be_bytes());
            }
            p = a.ifa_next;
        }
        libc::freeifaddrs(ifa);
    }
    out
}

#[cfg(unix)]
fn mcast_socket() -> Option<UdpSocket> {
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return None;
        }
        let one: libc::c_int = 1;
        for opt in [libc::SO_REUSEADDR, libc::SO_REUSEPORT] {
            libc::setsockopt(fd, libc::SOL_SOCKET, opt, &one as *const _ as *const _, 4);
        }
        let addr = libc::sockaddr_in { sin_family: libc::AF_INET as libc::sa_family_t, sin_port: MDNS_PORT.to_be(), sin_addr: libc::in_addr { s_addr: 0 }, sin_zero: [0; 8] };
        if libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of_val(&addr) as libc::socklen_t) != 0 {
            libc::close(fd);
            return None;
        }
        let ttl: libc::c_int = 255;
        libc::setsockopt(fd, libc::IPPROTO_IP, libc::IP_MULTICAST_TTL, &ttl as *const _ as *const _, 4);
        libc::setsockopt(fd, libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP, &one as *const _ as *const _, 4);
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        use std::os::fd::FromRawFd;
        Some(UdpSocket::from_raw_fd(fd))
    }
}

#[cfg(unix)]
fn join(sock: &UdpSocket, addr: [u8; 4]) {
    let mreq = libc::ip_mreq {
        imr_multiaddr: libc::in_addr { s_addr: u32::from(GROUP).to_be() },
        imr_interface: libc::in_addr { s_addr: u32::from(Ipv4Addr::from(addr)).to_be() },
    };
    unsafe {
        libc::setsockopt(sock.as_raw_fd(), libc::IPPROTO_IP, libc::IP_ADD_MEMBERSHIP, &mreq as *const _ as *const _, std::mem::size_of_val(&mreq) as libc::socklen_t);
    }
}

#[cfg(unix)]
fn send_mcast(sock: &UdpSocket, pkt: &[u8], iface: Option<[u8; 4]>) {
    if let Some(a) = iface {
        let ip = libc::in_addr { s_addr: u32::from(Ipv4Addr::from(a)).to_be() };
        unsafe {
            libc::setsockopt(sock.as_raw_fd(), libc::IPPROTO_IP, libc::IP_MULTICAST_IF, &ip as *const _ as *const _, 4);
        }
    }
    let _ = sock.send_to(pkt, SocketAddrV4::new(GROUP, MDNS_PORT));
}

/// Joins the announcement group and answers queries. Empty when port 5353 could not be taken.
pub struct Announcer {
    sock: Option<UdpSocket>,
    services: Vec<Svc>,
    joined: Vec<[u8; 4]>,
    last: Vec<[u8; 4]>,
    burst: u8,
    next: Instant,
    refresh: Instant,
    interfaces_next: Instant,
}

impl Announcer {
    pub fn open(services: &[Svc]) -> Announcer {
        let sock = mcast_socket();
        let names: Vec<&str> = services.iter().map(|s| s.typ).collect();
        match &sock {
            Some(_) => eprintln!("oos-ui: announcing {} (identification {})", names.join(", "), IDENTIFICATION_PATH),
            None => eprintln!("oos-ui: announcement not available"),
        }
        Announcer { sock, services: services.to_vec(), joined: Vec::new(), last: Vec::new(), burst: 2, next: Instant::now(), refresh: Instant::now() + Duration::from_secs(60), interfaces_next: Instant::now() }
    }

    /// The responder owns a socket and has real listeners to announce.
    pub fn available(&self) -> bool {
        self.sock.is_some() && !self.services.is_empty()
    }

    /// Addresses in the most recent announcement, excluding down and loopback interfaces.
    pub fn local_addresses(&self) -> &[[u8; 4]] {
        &self.last
    }

    pub fn poll(&mut self) {
        // Adapter enumeration is expensive on Windows and Linux; a fresh list once per second is enough.
        let ifs = if Instant::now() >= self.interfaces_next {
            self.interfaces_next = Instant::now() + Duration::from_secs(1);
            addresses()
        } else {
            self.last.clone()
        };
        if self.sock.is_none() || self.services.is_empty() {
            // The UI can still show a real address when another mDNS responder owns the port.
            self.last = ifs;
            return;
        }
        let fresh: Vec<[u8; 4]> = ifs.iter().copied().filter(|a| !self.joined.contains(a)).collect();
        if let Some(sock) = self.sock.as_ref() {
            for a in &fresh {
                join(sock, *a);
            }
        }
        self.joined.extend(fresh);
        let addrs = ifs;
        if addrs != self.last {
            self.last = addrs;
            self.burst = self.burst.max(1);
            self.next = Instant::now();
        }
        let id = Identity::now(self.last.clone());
        let due = (self.burst > 0 && Instant::now() >= self.next) || Instant::now() >= self.refresh;
        if due {
            let pkt = advertisement(&id, &self.services);
            self.send_all(&pkt);
            if self.burst > 0 {
                self.burst -= 1;
                self.next = Instant::now() + Duration::from_secs(1);
            }
            self.refresh = Instant::now() + Duration::from_secs(TTL as u64 / 2);
        }
        let mut got = Vec::new();
        if let Some(sock) = self.sock.as_ref() {
            let mut buf = [0u8; 9000];
            while let Ok((n, from)) = sock.recv_from(&mut buf) {
                got.push((buf[..n].to_vec(), from));
            }
        }
        for (buf, from) in got {
            let Some((pkt, unicast)) = answer(&buf, &id, &self.services) else { continue };
            if unicast {
                if let Some(sock) = self.sock.as_ref() {
                    let _ = sock.send_to(&pkt, from);
                }
            } else {
                self.send_all(&pkt);
            }
        }
    }

    fn send_all(&self, pkt: &[u8]) {
        let Some(sock) = self.sock.as_ref() else { return };
        if self.joined.is_empty() {
            send_mcast(sock, pkt, None);
        } else {
            for a in &self.joined {
                send_mcast(sock, pkt, Some(*a));
            }
        }
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        let Some(sock) = self.sock.as_ref() else { return };
        if self.services.is_empty() {
            return;
        }
        let id = Identity::now(self.last.clone());
        let mut answer = Vec::new();
        for s in &self.services {
            answer.extend(service_rrs(&id, s, 0));
        }
        answer.extend(address_rrs(&id, 0));
        let pkt = pack(0, &answer, &[]);
        if self.joined.is_empty() {
            send_mcast(sock, &pkt, None);
        } else {
            for a in &self.joined {
                send_mcast(sock, &pkt, Some(*a));
            }
        }
    }
}

#[cfg(unix)]
use std::os::fd::AsRawFd;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Identity, Vec<Svc>) {
        let id = Identity {
            instance: "OOS DHO924S oos".into(),
            host: "oos".into(),
            manufacturer: "OOS".into(),
            model: "DHO924S".into(),
            serial: "1".into(),
            firmware: "oos-ui 0.1.5".into(),
            addresses: vec![[192, 168, 1, 9]],
        };
        let services = listening(Some(8080), Some(5555), Some(9009), Some(4880));
        (id, services)
    }

    fn names(msg: &[u8]) -> Vec<(String, u16, Vec<u8>)> {
        let mut p = 12;
        let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
        let extra = u16::from_be_bytes([msg[10], msg[11]]) as usize;
        let mut o = Vec::new();
        for _ in 0..an + extra {
            let (name, n) = read_name(msg, p).unwrap();
            p = n;
            let typ = u16::from_be_bytes(msg[p..p + 2].try_into().unwrap());
            p += 8;
            let rd = u16::from_be_bytes(msg[p..p + 2].try_into().unwrap()) as usize;
            p += 2;
            o.push((name, typ, msg[p..p + rd].to_vec()));
            p += rd;
        }
        o
    }

    fn srv_port(data: &[u8]) -> u16 {
        u16::from_be_bytes([data[4], data[5]])
    }

    #[test]
    fn the_announcement_names_the_listeners_and_the_identification_page() {
        let (id, services) = sample();
        let msg = advertisement(&id, &services);
        let recs = names(&msg);
        let ptrs: Vec<&str> = recs.iter().filter(|r| r.1 == PTR).map(|r| r.0.as_str()).collect();
        assert!(ptrs.contains(&"_scpi-raw._tcp.local"));
        assert!(ptrs.contains(&"_vxi-11._tcp.local"));
        assert!(ptrs.contains(&"_hislip._tcp.local"));
        assert!(ptrs.contains(&"_http._tcp.local"));
        let port = |typ: &str| {
            recs.iter().find(|r| r.1 == SRV && r.0.ends_with(&format!(".{typ}.local"))).map(|r| srv_port(&r.2)).unwrap()
        };
        assert_eq!((port("_scpi-raw._tcp"), port("_vxi-11._tcp"), port("_hislip._tcp"), port("_http._tcp")), (5555, 9009, 4880, 8080));
        let txts: Vec<String> = recs.iter().filter(|r| r.1 == TXT).map(|r| String::from_utf8_lossy(&r.2).into_owned()).collect();
        assert!(txts.iter().any(|t| t.contains("path=/lxi/identification")), "{txts:?}");
        assert!(txts.iter().any(|t| t.contains("_http._tcp") || t.contains("path=/lxi/identification")));
        // the page path is on the web services, and the socket records name their own ports
        let http = txts.iter().find(|t| t.contains("path=/lxi/identification")).unwrap();
        assert!(http.contains("Manufacturer=OOS") && http.contains("Model=DHO924S"));
        assert!(txts.iter().any(|t| t.contains("TCPIP::oos.local::5555::SOCKET")));
        assert!(txts.iter().any(|t| t.contains("TCPIP::oos.local::hislip0::INSTR")));
        assert!(txts.iter().any(|t| t.contains("TCPIP::oos.local::inst0::INSTR")));
        let a = recs.iter().find(|r| r.1 == A).unwrap();
        assert_eq!(a.2, vec![192, 168, 1, 9]);
    }

    #[test]
    fn a_hislip_query_returns_its_port_and_a_page_query_returns_the_path() {
        let (id, services) = sample();
        let q = |name: &str| {
            let mut m = Vec::new();
            put_u16(&mut m, 7);
            put_u16(&mut m, 0);
            put_u16(&mut m, 1);
            put_u16(&mut m, 0);
            put_u16(&mut m, 0);
            put_u16(&mut m, 0);
            put_name(&mut m, name);
            put_u16(&mut m, PTR);
            put_u16(&mut m, IN | 0x8000); // unicast requested
            m
        };
        let (pkt, uni) = answer(&q("_hislip._tcp.local"), &id, &services).unwrap();
        assert!(uni);
        let recs = names(&pkt);
        let srv = recs.iter().find(|r| r.1 == SRV).unwrap();
        assert_eq!(srv_port(&srv.2), 4880);
        assert!(recs.iter().filter(|r| r.1 == TXT).all(|r| !String::from_utf8_lossy(&r.2).contains("path=")));
        let (pkt, _) = answer(&q("_http._tcp.local"), &id, &services).unwrap();
        let recs = names(&pkt);
        assert!(recs.iter().any(|r| r.1 == TXT && String::from_utf8_lossy(&r.2).contains("path=/lxi/identification")));
        assert!(answer(&advertisement(&id, &services), &id, &services).is_none(), "a response is not answered");
    }

    #[test]
    fn only_what_is_listening_is_announced() {
        let (id, _) = sample();
        let services = listening(Some(8080), Some(5555), None, Some(4880));
        let msg = advertisement(&id, &services);
        let text = String::from_utf8_lossy(&msg);
        assert!(!text.contains("_vxi-11"));
        assert!(text.contains("_hislip"));
        assert!(text.contains("_scpi-raw"));
        assert!(text.contains("path=/lxi/identification"));
    }

    #[test]
    fn a_query_to_the_socket_is_answered() {
        let services = listening(Some(8080), Some(5555), None, Some(4880));
        let mut a = Announcer::open(&services);
        // Give this responder its own port so another mDNS service cannot consume the test query.
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let destination = socket.local_addr().unwrap();
        a.sock = Some(socket);
        let mut q = Vec::new();
        put_u16(&mut q, 9);
        put_u16(&mut q, 0);
        put_u16(&mut q, 1);
        put_u16(&mut q, 0);
        put_u16(&mut q, 0);
        put_u16(&mut q, 0);
        put_name(&mut q, "_scpi-raw._tcp.local");
        put_u16(&mut q, PTR);
        put_u16(&mut q, IN | 0x8000);
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        c.set_nonblocking(true).unwrap();
        c.send_to(&q, destination).unwrap();
        let mut buf = [0u8; 2048];
        let deadline = Instant::now() + Duration::from_secs(2);
        let n = loop {
            a.poll();
            match c.recv_from(&mut buf) {
                Ok((n, _)) => break n,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                Err(error) => panic!("no discovery response: {error}"),
            }
        };
        let recs = names(&buf[..n]);
        assert!(recs.iter().any(|r| r.1 == SRV && srv_port(&r.2) == 5555), "{recs:?}");
    }
}
