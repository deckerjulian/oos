// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! VXI-11 (VXIbus Consortium VXI-11 TCP/IP Instrument Protocol 1.0): what VISA libraries use for
//! `TCPIP::<ip>::INSTR`. ONC RPC (RFC 5531) with XDR (RFC 4506): a portmapper (program 100000 v2, TCP and UDP port
//! 111) that answers GETPORT for the core channel, and the core channel (program 0x0607AF v1, TCP `CORE_PORT`) with
//! create_link, device_write, device_read, device_readstb, device_clear, remote/local/lock/unlock, destroy_link.
//! The commands go through the SCPI interpreter (`Scpi::exec`); everything runs non-blocking from the main loop.
//! Unauthenticated like SCPI and the web remote: bench LAN only.

use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

pub const PMAP_PROG: u32 = 100_000;
pub const CORE_PROG: u32 = 0x0607AF;
pub const CORE_PORT: u16 = 9009;
/// Most data waiting on one connection: maxRecvSize of create_link and room for the RPC header.
const MAX_RECORD: usize = (1 << 20) + 4096;

/// XDR reader over a call body.
pub struct Xdr<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Xdr<'a> {
    pub fn new(b: &'a [u8]) -> Xdr<'a> {
        Xdr { b, p: 0 }
    }
    pub fn u32(&mut self) -> Option<u32> {
        let v = self.b.get(self.p..self.p + 4)?;
        self.p += 4;
        Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }
    pub fn opaque(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        let v = self.b.get(self.p..self.p + n)?;
        self.p += n.div_ceil(4) * 4;
        Some(v)
    }
}

fn put(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_opaque(out: &mut Vec<u8>, d: &[u8]) {
    put(out, d.len() as u32);
    out.extend_from_slice(d);
    out.resize(out.len() + (4 - d.len() % 4) % 4, 0);
}

/// A parsed RPC call: xid, program, version, procedure and the argument bytes.
pub struct Call<'a> {
    pub xid: u32,
    pub prog: u32,
    pub vers: u32,
    pub proc_: u32,
    pub args: &'a [u8],
}

pub fn parse_call(m: &[u8]) -> Option<Call<'_>> {
    let mut x = Xdr::new(m);
    let xid = x.u32()?;
    if x.u32()? != 0 || x.u32()? != 2 {
        return None; // not a call / not RPC v2
    }
    let (prog, vers, proc_) = (x.u32()?, x.u32()?, x.u32()?);
    x.u32()?;
    x.opaque()?; // credentials
    x.u32()?;
    x.opaque()?; // verifier
    // a verifier without its padding leaves the position past the end
    Some(Call { xid, prog, vers, proc_, args: m.get(x.p..)? })
}

/// Accepted reply header (verifier AUTH_NONE) with `accept_stat` (0 success, 1 prog unavailable, 3 proc unavailable).
fn reply(xid: u32, accept: u32) -> Vec<u8> {
    let mut o = Vec::new();
    for v in [xid, 1, 0, 0, 0, accept] {
        put(&mut o, v);
    }
    o
}

/// Portmapper: GETPORT (3) of the core channel over TCP (6) -> `CORE_PORT`; NULL (0); anything else 0.
pub fn portmap(c: &Call) -> Vec<u8> {
    if c.prog != PMAP_PROG {
        return reply(c.xid, 1);
    }
    let mut o = reply(c.xid, 0);
    match c.proc_ {
        0 => {}
        3 => {
            let mut x = Xdr::new(c.args);
            let (prog, _vers, prot) = (x.u32().unwrap_or(0), x.u32().unwrap_or(0), x.u32().unwrap_or(0));
            put(&mut o, if prog == CORE_PROG && prot == 6 { CORE_PORT as u32 } else { 0 });
        }
        _ => return reply(c.xid, 3),
    }
    o
}

/// One VXI-11 link's output (responses waiting for device_read).
#[derive(Default)]
pub struct Link {
    pub out: Vec<u8>,
}

/// Core channel procedures; `exec` runs one program message (may contain several ';'-separated commands) and
/// returns the response bytes (without the newline).
pub fn core(c: &Call, links: &mut Vec<Option<Link>>, exec: &mut dyn FnMut(&str) -> Vec<u8>) -> Vec<u8> {
    if c.prog != CORE_PROG || c.vers != 1 {
        return reply(c.xid, 1);
    }
    let mut x = Xdr::new(c.args);
    let mut o = reply(c.xid, 0);
    match c.proc_ {
        0 => {}
        // create_link(clientId, lockDevice, lock_timeout, device) -> error, lid, abortPort, maxRecvSize
        10 => {
            let lid = match links.iter().position(|l| l.is_none()) {
                None if links.len() >= MAX_LINKS => {
                    put(&mut o, 9); // out of resources
                    put(&mut o, 0);
                    put(&mut o, 0);
                    put(&mut o, 0);
                    return o;
                }
                Some(i) => {
                    links[i] = Some(Link::default());
                    i
                }
                None => {
                    links.push(Some(Link::default()));
                    links.len() - 1
                }
            };
            for v in [0, lid as u32, 0, 1 << 20] {
                put(&mut o, v);
            }
        }
        // device_write(lid, io_timeout, lock_timeout, flags, data) -> error, size
        11 => {
            let lid = x.u32().unwrap_or(u32::MAX) as usize;
            let _ = (x.u32(), x.u32(), x.u32());
            let data = x.opaque().unwrap_or(&[]);
            match links.get_mut(lid).and_then(|l| l.as_mut()) {
                Some(l) => {
                    let text = String::from_utf8_lossy(data);
                    for line in text.split(['\n', '\r']).filter(|s| !s.trim().is_empty()) {
                        if l.out.len() >= crate::scpi::RESP_MAX {
                            break;
                        }
                        let r = exec(line.trim());
                        if !r.is_empty() {
                            l.out.extend_from_slice(&r);
                            l.out.push(b'\n');
                        }
                    }
                    put(&mut o, 0);
                    put(&mut o, data.len() as u32);
                }
                None => {
                    put(&mut o, 4); // invalid link identifier
                    put(&mut o, 0);
                }
            }
        }
        // device_read(lid, requestSize, io_timeout, lock_timeout, flags, termChar) -> error, reason, data
        12 => {
            let lid = x.u32().unwrap_or(u32::MAX) as usize;
            let req = x.u32().unwrap_or(0) as usize;
            match links.get_mut(lid).and_then(|l| l.as_mut()) {
                Some(l) if !l.out.is_empty() => {
                    let n = req.max(1).min(l.out.len());
                    let chunk: Vec<u8> = l.out.drain(..n).collect();
                    put(&mut o, 0);
                    put(&mut o, if l.out.is_empty() { 4 } else { 1 }); // END, or REQCNT (more to come)
                    put_opaque(&mut o, &chunk);
                }
                Some(_) => {
                    put(&mut o, 15); // I/O timeout: nothing to read
                    put(&mut o, 0);
                    put_opaque(&mut o, &[]);
                }
                None => {
                    put(&mut o, 4);
                    put(&mut o, 0);
                    put_opaque(&mut o, &[]);
                }
            }
        }
        // device_readstb -> error, stb
        13 => {
            put(&mut o, 0);
            put(&mut o, 0);
        }
        // device_trigger, device_clear, device_remote, device_local, device_lock, device_unlock -> error
        14..=19 => {
            if c.proc_ == 15 {
                let lid = x.u32().unwrap_or(u32::MAX) as usize;
                if let Some(Some(l)) = links.get_mut(lid) {
                    l.out.clear();
                }
            }
            put(&mut o, 0);
        }
        // destroy_link(lid) -> error
        23 => {
            let lid = x.u32().unwrap_or(u32::MAX) as usize;
            if let Some(l) = links.get_mut(lid) {
                *l = None;
            }
            put(&mut o, 0);
        }
        _ => return reply(c.xid, 3),
    }
    o
}

const MAX_CONNS: usize = 8;
const MAX_LINKS: usize = 16;

struct Conn {
    s: TcpStream,
    buf: Vec<u8>,
    core: bool,
}

/// The listeners: portmapper TCP/UDP 111, core TCP `CORE_PORT`.
pub struct Server {
    pm_tcp: Option<TcpListener>,
    pm_udp: Option<UdpSocket>,
    core_tcp: Option<TcpListener>,
    conns: Vec<Conn>,
    pub links: Vec<Option<Link>>,
}

impl Server {
    /// Nothing listening. A computer leaves VXI-11 to the scope, where port 111 is free.
    pub fn closed() -> Server {
        Server { pm_tcp: None, pm_udp: None, core_tcp: None, conns: Vec::new(), links: Vec::new() }
    }

    /// The core port, once the portmapper on 111 and the core channel are both up.
    pub fn service_port(&self) -> Option<u16> {
        (self.pm_tcp.is_some() && self.pm_udp.is_some() && self.core_tcp.is_some()).then_some(CORE_PORT)
    }

    pub fn open() -> Server {
        let tcp = |p: u16| TcpListener::bind(("0.0.0.0", p)).ok().inspect(|l| drop(l.set_nonblocking(true)));
        let pm_udp = UdpSocket::bind(("0.0.0.0", 111)).ok().inspect(|u| drop(u.set_nonblocking(true)));
        let s = Server { pm_tcp: tcp(111), pm_udp, core_tcp: tcp(CORE_PORT), conns: Vec::new(), links: Vec::new() };
        eprintln!("oos-ui: VXI-11 portmapper {} / core {}", if s.pm_tcp.is_some() { "111" } else { "unavailable" }, if s.core_tcp.is_some() { CORE_PORT.to_string() } else { "unavailable".into() });
        s
    }

    pub fn poll(&mut self, exec: &mut dyn FnMut(&str) -> Vec<u8>) {
        if let Some(u) = &self.pm_udp {
            let mut b = [0u8; 1500];
            while let Ok((n, from)) = u.recv_from(&mut b) {
                if let Some(c) = parse_call(&b[..n]) {
                    let _ = u.send_to(&portmap(&c), from);
                }
            }
        }
        for (l, core) in [(&self.pm_tcp, false), (&self.core_tcp, true)] {
            if let Some(l) = l {
                while let Ok((s, _)) = l.accept() {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_millis(1)));
                    let _ = s.set_nodelay(true);
                    if self.conns.len() < MAX_CONNS {
                        self.conns.push(Conn { s, buf: Vec::new(), core });
                    }
                }
            }
        }
        let mut i = 0;
        while i < self.conns.len() {
            let mut closed = false;
            let mut tmp = [0u8; 8192];
            loop {
                match self.conns[i].s.read(&mut tmp) {
                    Ok(0) => {
                        closed = true;
                        break;
                    }
                    Ok(n) => self.conns[i].buf.extend_from_slice(&tmp[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                    Err(_) => {
                        closed = true;
                        break;
                    }
                }
            }
            // RPC record marking: 4-byte header (bit31 = last fragment, length), fragments joined
            while let Some(msg) = take_record(&mut self.conns[i].buf) {
                let r = match parse_call(&msg) {
                    Some(c) if self.conns[i].core => core(&c, &mut self.links, exec),
                    Some(c) => portmap(&c),
                    None => continue,
                };
                let mut out = ((r.len() as u32) | 1 << 31).to_be_bytes().to_vec();
                out.extend_from_slice(&r);
                if !crate::scpi::send(&mut self.conns[i].s, &out) {
                    closed = true;
                    break;
                }
            }
            // what is left is one unfinished record: nothing beyond the announced maxRecvSize
            closed |= self.conns[i].buf.len() > MAX_RECORD;
            if closed {
                self.conns.remove(i);
                // a client that left without destroy_link: nobody can read what its link holds
                if !self.conns.iter().any(|c| c.core) {
                    self.links.clear();
                }
            } else {
                i += 1;
            }
        }
    }
}

/// Removes one complete RPC record (all fragments up to the last) from `buf`.
pub fn take_record(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let mut p = 0;
    let mut msg = Vec::new();
    loop {
        let h = buf.get(p..p + 4)?;
        let h = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
        let n = (h & 0x7fff_ffff) as usize;
        let frag = buf.get(p + 4..p + 4 + n)?;
        msg.extend_from_slice(frag);
        p += 4 + n;
        if h & 1 << 31 != 0 {
            buf.drain(..p);
            return Some(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(xid: u32, prog: u32, proc_: u32, args: &[u8]) -> Vec<u8> {
        let mut m = Vec::new();
        for v in [xid, 0, 2, prog, 1, proc_, 0, 0, 0, 0] {
            put(&mut m, v);
        }
        m.extend_from_slice(args);
        m
    }

    #[test]
    fn getport_and_a_query_round_trip() {
        // portmapper GETPORT(core, v1, TCP)
        let mut a = Vec::new();
        for v in [CORE_PROG, 1, 6, 0] {
            put(&mut a, v);
        }
        let m = call(7, PMAP_PROG, 3, &a);
        let r = portmap(&parse_call(&m).unwrap());
        let mut x = Xdr::new(&r);
        assert_eq!([x.u32(), x.u32(), x.u32(), x.u32(), x.u32(), x.u32(), x.u32()], [Some(7), Some(1), Some(0), Some(0), Some(0), Some(0), Some(CORE_PORT as u32)]);
        // create_link, device_write "*IDN?\n", device_read, destroy_link
        let mut links = Vec::new();
        let mut exec = |c: &str| if c == "*IDN?" { b"OOS,DHO924S".to_vec() } else { Vec::new() };
        let mut a = Vec::new();
        for v in [1, 0, 0] {
            put(&mut a, v);
        }
        put_opaque(&mut a, b"inst0");
        let r = core(&parse_call(&call(8, CORE_PROG, 10, &a)).unwrap(), &mut links, &mut exec);
        let mut x = Xdr::new(&r[24..]);
        assert_eq!((x.u32(), x.u32()), (Some(0), Some(0)));
        let mut a = Vec::new();
        for v in [0, 1000, 0, 8] {
            put(&mut a, v);
        }
        put_opaque(&mut a, b"*IDN?\n");
        let r = core(&parse_call(&call(9, CORE_PROG, 11, &a)).unwrap(), &mut links, &mut exec);
        let mut x = Xdr::new(&r[24..]);
        assert_eq!((x.u32(), x.u32()), (Some(0), Some(6)));
        let mut a = Vec::new();
        for v in [0, 1024, 1000, 0, 0, 0] {
            put(&mut a, v);
        }
        let r = core(&parse_call(&call(10, CORE_PROG, 12, &a)).unwrap(), &mut links, &mut exec);
        let mut x = Xdr::new(&r[24..]);
        assert_eq!((x.u32(), x.u32()), (Some(0), Some(4)));
        assert_eq!(x.opaque(), Some(&b"OOS,DHO924S\n"[..]));
        let mut a = Vec::new();
        put(&mut a, 0);
        core(&parse_call(&call(11, CORE_PROG, 23, &a)).unwrap(), &mut links, &mut exec);
        assert!(links[0].is_none());
    }

    #[test]
    fn verifier_without_its_padding_is_refused() {
        let mut m = Vec::new();
        for v in [1, 0, 2, CORE_PROG, 1, 0, 0, 0, 0, 1] {
            put(&mut m, v);
        }
        m.push(0xff); // verifier of length 1, the 3 pad bytes are missing
        assert!(parse_call(&m).is_none());
    }

    #[test]
    fn record_marking_joins_fragments() {
        let mut b = Vec::new();
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(b"abc");
        b.extend_from_slice(&(2u32 | 1 << 31).to_be_bytes());
        b.extend_from_slice(b"de");
        b.extend_from_slice(&[0, 0]); // start of the next record
        assert_eq!(take_record(&mut b), Some(b"abcde".to_vec()));
        assert_eq!(b, vec![0, 0]);
        assert_eq!(take_record(&mut b), None);
    }
}
