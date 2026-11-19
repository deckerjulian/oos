// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! VXI-11 (VXIbus Consortium VXI-11 TCP/IP Instrument Protocol 1.0): what VISA libraries use for
//! `TCPIP::<ip>::INSTR`. ONC RPC (RFC 5531) with XDR (RFC 4506): a portmapper (program 100000 v2, TCP and UDP port
//! 111) that answers GETPORT for the core channel, and the core channel (program 0x0607AF v1, TCP `CORE_PORT`) with
//! create_link, device_write, device_read, device_readstb, device_clear, remote/local/lock/unlock, destroy_link.
//! The commands go through the SCPI interpreter (`Scpi::exec`); everything runs non-blocking from the main loop.
//! Unauthenticated like SCPI and the web remote: bench LAN only.

use std::net::{TcpListener, TcpStream, UdpSocket};

pub const PMAP_PROG: u32 = 100_000;
pub const CORE_PROG: u32 = 0x0607AF;
pub use oos_system::net::VXI11_CORE_PORT as CORE_PORT;
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
pub struct Link<P> {
    pub out: Vec<u8>,
    programs: std::collections::VecDeque<P>,
}

impl<P> Default for Link<P> {
    fn default() -> Self {
        Link { out: Vec::new(), programs: Default::default() }
    }
}

/// Core channel procedures; `exec` runs one program message (may contain several ';'-separated commands) and
/// returns the response bytes (without the newline).
pub fn core_async<P: crate::Program>(
    c: &Call,
    links: &mut Vec<Option<Link<P>>>,
    exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>,
) -> Vec<u8> {
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
                    let queued: usize = l.programs.iter().map(|program| program.bytes()).sum();
                    if queued + data.len() > MAX_RECORD || l.programs.len() >= 16 {
                        put(&mut o, 9);
                        put(&mut o, 0);
                        return o;
                    }
                    l.programs.push_back(P::new(&String::from_utf8_lossy(data)));
                    pump_link(l, exec);
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
                    l.programs.clear();
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

fn pump_link<P: crate::Program>(link: &mut Link<P>, exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>) {
    while let Some(mut program) = link.programs.pop_front() {
        if link.out.len() >= oos_system::net::RESP_MAX {
            link.programs.clear();
            break;
        }
        let Some(response) = exec(&mut program) else {
            link.programs.push_front(program);
            break;
        };
        if program.has_reply() {
            link.out.extend_from_slice(&response);
            link.out.push(b'\n');
        }
    }
}

#[cfg(test)]
pub(crate) fn core(
    c: &Call,
    links: &mut Vec<Option<Link<crate::Line>>>,
    exec: &mut dyn FnMut(&str) -> Vec<u8>,
) -> Vec<u8> {
    core_async(c, links, &mut |program| Some(program.immediate(exec)))
}

/// Tests of oos-ui: a link driven as a device_write and a device_read would drive it, with the real interpreter.
#[cfg(any(test, feature = "testkit"))]
impl<P: crate::Program> Link<P> {
    /// `text` queued as a device_write queues it, and run as far as `exec` gets.
    pub fn write(&mut self, text: &str, exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>) {
        self.programs.push_back(P::new(text));
        pump_link(self, exec);
    }
    /// The programs that wait (for an operation, `*OPC?`) are run as far as `exec` gets now.
    pub fn pump(&mut self, exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>) {
        pump_link(self, exec);
    }
    /// How many programs wait.
    pub fn waiting(&self) -> usize {
        self.programs.len()
    }
}

/// Tests of oos-ui: a device_read of link 0 through the core channel, as (error, reason, data).
#[cfg(any(test, feature = "testkit"))]
pub fn device_read<P: crate::Program>(
    links: &mut Vec<Option<Link<P>>>,
    exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>,
) -> (u32, u32, Vec<u8>) {
    let mut m = Vec::new();
    for v in [77, 0, 2, CORE_PROG, 1, 12, 0, 0, 0, 0, 0, 1024, 1000, 0, 0, 0] {
        put(&mut m, v);
    }
    let reply = core_async(&parse_call(&m).expect("a call"), links, exec);
    let mut body = Xdr::new(&reply[24..]);
    (body.u32().unwrap_or(u32::MAX), body.u32().unwrap_or(u32::MAX), body.opaque().unwrap_or(&[]).to_vec())
}

const MAX_CONNS: usize = 8;
const MAX_LINKS: usize = 16;

struct Conn {
    s: TcpStream,
    buf: Vec<u8>,
    core: bool,
    links: Vec<usize>,
    /// the replies not yet written (#192): the next call is taken once they are out
    out: oos_system::net::Outbox,
    /// the client closed its side: the connection goes once its replies are out
    closing: bool,
}

impl Conn {
    /// Writes what the socket takes now of the replies not yet written.
    fn flush(&mut self) -> oos_system::net::Sent {
        self.out.flush(&self.s)
    }
}

/// The listeners: portmapper TCP/UDP 111, core TCP `CORE_PORT`.
pub struct Server<P> {
    pm_tcp: Option<TcpListener>,
    pm_udp: Option<UdpSocket>,
    core_tcp: Option<TcpListener>,
    conns: Vec<Conn>,
    pub links: Vec<Option<Link<P>>>,
    /// who may connect (`netacl`, net.allow; empty: everyone): new connections, and after a change those held
    pub acl: oos_system::netacl::Shared,
    /// the changes of `acl` looked at (`netacl::Shared::changed`)
    acl_seen: u64,
}

impl<P: crate::Program> Server<P> {
    /// Nothing listening. A computer leaves VXI-11 to the scope, where port 111 is free.
    pub fn closed() -> Self {
        let acl = oos_system::netacl::Shared::default();
        Server { pm_tcp: None, pm_udp: None, core_tcp: None, conns: Vec::new(), links: Vec::new(), acl, acl_seen: 0 }
    }

    /// The core port, once the portmapper on 111 and the core channel are both up.
    pub fn service_port(&self) -> Option<u16> {
        (self.pm_tcp.is_some() && self.pm_udp.is_some() && self.core_tcp.is_some()).then_some(CORE_PORT)
    }

    pub fn open() -> Self {
        let mut s = Server::closed();
        s.set_open(true);
        s
    }

    /// System > Network > Remote services: portmapper and core port open, or closed with their connections.
    pub fn set_open(&mut self, on: bool) {
        if !on {
            if self.pm_tcp.take().is_some() | self.core_tcp.take().is_some() | self.pm_udp.take().is_some() {
                eprintln!("oos-ui: VXI-11 off");
            }
            self.conns.clear();
            return;
        }
        if self.pm_tcp.is_some() || self.core_tcp.is_some() {
            return;
        }
        let tcp = |p: u16| oos_system::net6::tcp(p).inspect(|l| drop(l.set_nonblocking(true)));
        self.pm_udp = oos_system::net6::udp(111).inspect(|u| drop(u.set_nonblocking(true)));
        (self.pm_tcp, self.core_tcp) = (tcp(111), tcp(CORE_PORT));
        eprintln!(
            "oos-ui: VXI-11 portmapper {} / core {}",
            if self.pm_tcp.is_some() { "111" } else { "unavailable" },
            if self.core_tcp.is_some() { CORE_PORT.to_string() } else { "unavailable".into() }
        );
    }

    pub fn poll_async(&mut self, exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>) {
        if let Some(u) = &self.pm_udp {
            let mut b = [0u8; 1500];
            while let Ok((n, from)) = u.recv_from(&mut b) {
                if !self.acl.permits_peer(Ok(from)) {
                    continue;
                }
                if let Some(c) = parse_call(&b[..n]) {
                    let _ = u.send_to(&portmap(&c), from);
                }
            }
        }
        for (l, core) in [(&self.pm_tcp, false), (&self.core_tcp, true)] {
            if let Some(l) = l {
                while let Ok((s, peer)) = l.accept() {
                    if !self.acl.permits_peer(Ok(peer)) {
                        continue;
                    }
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_nodelay(true);
                    if self.conns.len() < MAX_CONNS {
                        self.conns.push(Conn {
                            s,
                            buf: Vec::new(),
                            core,
                            links: Vec::new(),
                            out: Default::default(),
                            closing: false,
                        });
                    }
                }
            }
        }
        // net.allow changed on the screen: a connection the list now leaves out is closed with its links
        let recheck = self.acl.changed(&mut self.acl_seen);
        let mut i = 0;
        while i < self.conns.len() {
            let mut closed = recheck && !self.acl.permits_peer(self.conns[i].s.peer_addr());
            // what is still to be written goes first, as much as the socket takes now (#192)
            closed |= self.conns[i].flush() == oos_system::net::Sent::Gone;
            let mut eof = self.conns[i].closing;
            let mut tmp = [0u8; 8192];
            let mut read_bytes = 0;
            while !closed && !eof {
                match oos_system::net::read_now(&self.conns[i].s, &mut tmp) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        if self.conns[i].buf.len() + n > MAX_RECORD {
                            closed = true;
                            break;
                        }
                        self.conns[i].buf.extend_from_slice(&tmp[..n]);
                        read_bytes += n;
                        if read_bytes >= 65536 {
                            break;
                        }
                    }
                    Err(e) if oos_system::net::later(&e) => break,
                    Err(_) => {
                        closed = true;
                        break;
                    }
                }
            }
            // RPC record marking: 4-byte header (bit31 = last fragment, length), fragments joined; the next call once the
            // replies before it are out, also the calls a client sent before it closed its side
            while !closed && self.conns[i].out.is_empty() {
                let Some(msg) = take_record(&mut self.conns[i].buf) else {
                    break;
                };
                let r = match parse_call(&msg) {
                    Some(c) if self.conns[i].core => {
                        let response = core_async(&c, &mut self.links, exec);
                        if c.proc_ == 10 && response.get(24..28) == Some(&0u32.to_be_bytes()) {
                            if let Some(id) = response.get(28..32) {
                                let id = u32::from_be_bytes(id.try_into().unwrap()) as usize;
                                for connection in &mut self.conns {
                                    connection.links.retain(|old| *old != id);
                                }
                                self.conns[i].links.push(id);
                            }
                        }
                        response
                    }
                    Some(c) => portmap(&c),
                    None => continue,
                };
                let mut out = ((r.len() as u32) | 1 << 31).to_be_bytes().to_vec();
                out.extend_from_slice(&r);
                self.conns[i].out.push(&out);
                if self.conns[i].flush() == oos_system::net::Sent::Gone {
                    closed = true;
                    break;
                }
            }
            if !closed {
                for &id in &self.conns[i].links {
                    if let Some(Some(link)) = self.links.get_mut(id) {
                        pump_link(link, exec);
                    }
                }
            }
            // what is left is one unfinished record: nothing beyond the announced maxRecvSize
            closed |= self.conns[i].buf.len() > MAX_RECORD;
            // a client that closed its side goes once every whole call it sent is answered and the replies are out
            if eof && !closed {
                closed = self.conns[i].out.is_empty();
                self.conns[i].closing = true;
            }
            if closed {
                let connection = self.conns.remove(i);
                for id in connection.links {
                    if let Some(link) = self.links.get_mut(id) {
                        *link = None;
                    }
                }
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
        assert_eq!(
            [x.u32(), x.u32(), x.u32(), x.u32(), x.u32(), x.u32(), x.u32()],
            [Some(7), Some(1), Some(0), Some(0), Some(0), Some(0), Some(CORE_PORT as u32)]
        );
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

    /// One RPC call as a record: the last-fragment mark and the length, the call header, the arguments.
    fn rpc(xid: u32, prog: u32, proc_: u32, args: &[u32], data: &[u8]) -> Vec<u8> {
        let mut m = Vec::new();
        for v in [xid, 0, 2, prog, 1, proc_, 0, 0, 0, 0].iter().chain(args) {
            m.extend_from_slice(&v.to_be_bytes());
        }
        if !data.is_empty() {
            m.extend_from_slice(&(data.len() as u32).to_be_bytes());
            m.extend_from_slice(data);
            m.resize(m.len().next_multiple_of(4), 0);
        }
        let mut rec = (0x8000_0000 | m.len() as u32).to_be_bytes().to_vec();
        rec.extend(m);
        rec
    }

    /// `take_record`, `parse_call`, `portmap` and `core` (VXI-11 on TCP 111 and its core port): return for any bytes;
    /// a record taken is no longer than the bytes it came from and leaves the buffer shorter; an answer is whole XDR
    /// words and starts with the call's number; there are never more than 16 links.
    #[test]
    fn hostile_vxi11_records() {
        let two_fragments = {
            let whole = rpc(9, CORE_PROG, 11, &[0, 1000, 1000, 8], b"*IDN?\n");
            let body = &whole[4..];
            let mut b = (12u32).to_be_bytes().to_vec();
            b.extend_from_slice(&body[..12]);
            b.extend_from_slice(&(0x8000_0000 | (body.len() - 12) as u32).to_be_bytes());
            b.extend_from_slice(&body[12..]);
            b
        };
        let session: Vec<u8> = [
            rpc(1, PMAP_PROG, 3, &[CORE_PROG, 1, 6, 0], b""),
            rpc(2, CORE_PROG, 10, &[77, 0, 1000], b"inst0"),
            rpc(3, CORE_PROG, 11, &[0, 1000, 1000, 8], b":CHAN1:SCAL 0.5;:CHAN1:SCAL?\n"),
            rpc(4, CORE_PROG, 12, &[0, 1024, 1000, 1000, 0, 10], b""),
            rpc(5, CORE_PROG, 15, &[0, 0, 1000, 1000], b""),
            rpc(6, CORE_PROG, 23, &[0], b""),
        ]
        .concat();
        let read = rpc(7, CORE_PROG, 12, &[0, 1, 1000, 1000, 0, 10], b"");
        let seeds: [&[u8]; 3] = [&session, &two_fragments, &read];
        crate::hostile::run("vxi11::core", &seeds, 1, |b| {
            let mut buf = b.to_vec();
            // one link is open, with an answer waiting
            let mut links = vec![Some(Link::default())];
            if let Some(l) = links[0].as_mut() {
                l.out.extend_from_slice(b"OOS,unit\n");
            }
            let mut exec = |c: &str| if c.ends_with('?') { b"1".to_vec() } else { Vec::new() };
            loop {
                let before = buf.len();
                let Some(rec) = take_record(&mut buf) else { break };
                if buf.len() + 4 > before || rec.len() + buf.len() > before {
                    return Err(format!("a record of {} bytes left {} of {} bytes", rec.len(), buf.len(), before));
                }
                let Some(c) = parse_call(&rec) else { continue };
                for reply in [portmap(&c), core(&c, &mut links, &mut exec)] {
                    if reply.len() % 4 != 0 || reply.len() < 24 || reply[..4] != c.xid.to_be_bytes() {
                        return Err(format!("the answer {reply:02x?} to call {}", c.xid));
                    }
                }
                if links.len() > 16 {
                    return Err(format!("{} links", links.len()));
                }
            }
            if buf.len() > b.len() {
                return Err("the buffer grew".into());
            }
            Ok(())
        });
    }
}
