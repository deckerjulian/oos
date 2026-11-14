// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! HiSLIP (IVI-6.1) on TCP 4880, for a computer. The scope keeps VXI-11, where port 111 is free.
//!
//! A connection is accepted and the data payload is handed to the same SCPI interpreter as the raw socket.
//! The session itself is not part of this: no lock is kept, overlap mode is not run, and remote/local is only
//! acknowledged. What is here is the header, the initial exchange that lets a client send that payload, and the
//! reply that carries the SCPI response back.

use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

pub const PORT: u16 = 4880;
const HEADER: usize = 16;
/// Same ceiling as one SCPI line (`:SOURce:ARB:DATA`).
const MAX_PAYLOAD: usize = 4 << 20;
const MAX_CONNS: usize = 8;
/// Protocol 1.0. 2.0 (secure connection) is not spoken, so the negotiated version stays 1.0.
const VERSION: u16 = 0x0100;

const INITIALIZE: u8 = 0;
const INITIALIZE_RESPONSE: u8 = 1;
const FATAL_ERROR: u8 = 2;
const ERROR: u8 = 3;
const ASYNC_LOCK: u8 = 4;
const ASYNC_LOCK_RESPONSE: u8 = 5;
const DATA: u8 = 6;
const DATA_END: u8 = 7;
const DEVICE_CLEAR_COMPLETE: u8 = 8;
const DEVICE_CLEAR_ACKNOWLEDGE: u8 = 9;
const ASYNC_REMOTE_LOCAL_CONTROL: u8 = 10;
const ASYNC_REMOTE_LOCAL_RESPONSE: u8 = 11;
const ASYNC_MAXIMUM_MESSAGE_SIZE: u8 = 15;
const ASYNC_MAXIMUM_MESSAGE_SIZE_RESPONSE: u8 = 16;
const ASYNC_INITIALIZE: u8 = 17;
const ASYNC_INITIALIZE_RESPONSE: u8 = 18;
const ASYNC_DEVICE_CLEAR: u8 = 19;
const ASYNC_STATUS_QUERY: u8 = 21;
const ASYNC_STATUS_RESPONSE: u8 = 22;
const ASYNC_DEVICE_CLEAR_ACKNOWLEDGE: u8 = 23;
const ASYNC_LOCK_INFO: u8 = 24;
const ASYNC_LOCK_INFO_RESPONSE: u8 = 25;

const FATAL_BAD_HEADER: u8 = 1;
const FATAL_BAD_INIT: u8 = 3;
const ERR_TYPE: u8 = 1;
const ERR_LARGE: u8 = 4;

/// One HiSLIP message. `param` is the 32-bit message parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub typ: u8,
    pub control: u8,
    pub param: u32,
    pub data: Vec<u8>,
}

pub fn encode(typ: u8, control: u8, param: u32, data: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(HEADER + data.len());
    o.extend_from_slice(b"HS");
    o.push(typ);
    o.push(control);
    o.extend_from_slice(&param.to_be_bytes());
    o.extend_from_slice(&(data.len() as u64).to_be_bytes());
    o.extend_from_slice(data);
    o
}

/// Takes one message from `buf`. `Err` is a header that cannot be a message (the connection should close).
pub fn take_message(buf: &mut Vec<u8>) -> Result<Option<Message>, ()> {
    if buf.len() < HEADER {
        return Ok(None);
    }
    if &buf[..2] != b"HS" {
        return Err(());
    }
    let n = u64::from_be_bytes(buf[8..16].try_into().unwrap()) as usize;
    if n > MAX_PAYLOAD {
        return Err(());
    }
    if buf.len() < HEADER + n {
        return Ok(None);
    }
    let msg = Message { typ: buf[2], control: buf[3], param: u32::from_be_bytes(buf[4..8].try_into().unwrap()), data: buf[HEADER..HEADER + n].to_vec() };
    buf.drain(..HEADER + n);
    Ok(Some(msg))
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    New,
    Sync,
    Async,
}

struct Peer {
    role: Role,
    session: u16,
    pending: Vec<u8>,
    msgid: u32,
    program: Option<crate::scpi::Program>,
}

struct Live {
    id: u16,
    /// Largest SCPI response payload we will put in one message toward this client.
    max_payload: usize,
}

struct Out {
    msgs: Vec<Vec<u8>>,
    close: bool,
}

fn fatal(code: u8) -> Out {
    Out { msgs: vec![encode(FATAL_ERROR, code, 0, b"")], close: true }
}

fn error(code: u8) -> Out {
    Out { msgs: vec![encode(ERROR, code, 0, b"")], close: false }
}

fn subaddress_ok(data: &[u8]) -> bool {
    let s = data.split(|&b| b == 0).next().unwrap_or(data);
    s.is_empty() || s == b"hislip0"
}

fn alloc_id(live: &[Live], next: &mut u16) -> u16 {
    loop {
        let id = *next;
        *next = next.wrapping_add(1);
        if id != 0 && live.iter().all(|l| l.id != id) {
            return id;
        }
    }
}

fn data_out(msgid: u32, payload: &[u8], max_payload: usize) -> Vec<Vec<u8>> {
    let n = max_payload.max(1);
    if payload.len() <= n {
        return vec![encode(DATA_END, 0, msgid, payload)];
    }
    let mut o = Vec::new();
    let mut p = 0;
    while p + n < payload.len() {
        o.push(encode(DATA, 0, msgid, &payload[p..p + n]));
        p += n;
    }
    o.push(encode(DATA_END, 0, msgid, &payload[p..]));
    o
}

fn on_program_message(peer: &mut Peer, live: &mut Vec<Live>, next: &mut u16, msg: &Message, exec: &mut dyn FnMut(&mut crate::scpi::Program) -> Option<Vec<u8>>) -> Out {
    match msg.typ {
        INITIALIZE if peer.role == Role::New => {
            let client = (msg.param >> 16) as u16;
            // 1.0 is the oldest version we speak; a newer client is answered at 1.0
            if client < VERSION || !subaddress_ok(&msg.data) {
                return fatal(FATAL_BAD_INIT);
            }
            let id = alloc_id(live, next);
            live.push(Live { id, max_payload: MAX_PAYLOAD });
            peer.role = Role::Sync;
            peer.session = id;
            Out { msgs: vec![encode(INITIALIZE_RESPONSE, 0, (VERSION as u32) << 16 | id as u32, b"")], close: false }
        }
        ASYNC_INITIALIZE if peer.role == Role::New => {
            let id = msg.param as u16;
            if !live.iter().any(|l| l.id == id) {
                return fatal(FATAL_BAD_INIT);
            }
            peer.role = Role::Async;
            peer.session = id;
            Out { msgs: vec![encode(ASYNC_INITIALIZE_RESPONSE, 0, 0, b"")], close: false }
        }
        DATA | DATA_END if peer.role == Role::Sync => {
            if peer.pending.len() + msg.data.len() > MAX_PAYLOAD {
                peer.pending.clear();
                return error(ERR_LARGE);
            }
            peer.pending.extend_from_slice(&msg.data);
            peer.msgid = msg.param;
            if msg.typ == DATA {
                return Out { msgs: Vec::new(), close: false };
            }
            let payload = std::mem::take(&mut peer.pending);
            let mut program = crate::scpi::Program::new(&String::from_utf8_lossy(&payload));
            let Some(mut resp) = exec(&mut program) else {
                peer.program = Some(program);
                return Out { msgs: Vec::new(), close: false };
            };
            if !program.has_reply {
                return Out { msgs: Vec::new(), close: false };
            }
            resp.push(b'\n');
            let max = live.iter().find(|l| l.id == peer.session).map(|l| l.max_payload).unwrap_or(MAX_PAYLOAD);
            Out { msgs: data_out(peer.msgid, &resp, max), close: false }
        }
        ASYNC_MAXIMUM_MESSAGE_SIZE if peer.role != Role::New => {
            let n = msg.data.get(..8).map(|b| u64::from_be_bytes(b.try_into().unwrap())).unwrap_or(0);
            // the size covers the header; a tiny or empty value keeps the default
            let payload = if n > HEADER as u64 { (n as usize - HEADER).clamp(1, MAX_PAYLOAD) } else { MAX_PAYLOAD };
            if let Some(l) = live.iter_mut().find(|l| l.id == peer.session) {
                l.max_payload = payload;
            }
            Out { msgs: vec![encode(ASYNC_MAXIMUM_MESSAGE_SIZE_RESPONSE, 0, 0, &(MAX_PAYLOAD as u64 + HEADER as u64).to_be_bytes())], close: false }
        }
        ASYNC_LOCK if peer.role != Role::New => {
            // acknowledged so the payload can follow; nothing is stored
            Out { msgs: vec![encode(ASYNC_LOCK_RESPONSE, 1, 0, b"")], close: false }
        }
        ASYNC_LOCK_INFO if peer.role != Role::New => Out { msgs: vec![encode(ASYNC_LOCK_INFO_RESPONSE, 0, 0, b"")], close: false },
        ASYNC_STATUS_QUERY if peer.role != Role::New => Out { msgs: vec![encode(ASYNC_STATUS_RESPONSE, 0, 0, b"")], close: false },
        ASYNC_REMOTE_LOCAL_CONTROL if peer.role != Role::New => Out { msgs: vec![encode(ASYNC_REMOTE_LOCAL_RESPONSE, 0, 0, b"")], close: false },
        DEVICE_CLEAR_COMPLETE if peer.role == Role::Sync => {
            peer.pending.clear(); peer.program = None;
            Out { msgs: vec![encode(DEVICE_CLEAR_ACKNOWLEDGE, 0, 0, b"")], close: false }
        }
        ASYNC_DEVICE_CLEAR if peer.role == Role::Async => {
            peer.pending.clear(); peer.program = None;
            Out { msgs: vec![encode(ASYNC_DEVICE_CLEAR_ACKNOWLEDGE, 0, 0, b"")], close: false }
        }
        // a trigger carries no SCPI payload
        12 if peer.role == Role::Sync => Out { msgs: Vec::new(), close: false },
        _ if peer.role == Role::New => fatal(FATAL_BAD_INIT),
        _ => error(ERR_TYPE),
    }
}

#[cfg(test)]
fn on_message(peer: &mut Peer, live: &mut Vec<Live>, next: &mut u16, msg: &Message, exec: &mut dyn FnMut(&str) -> Vec<u8>) -> Out {
    on_program_message(peer, live, next, msg, &mut |program| Some(program.immediate(exec)))
}

struct Conn {
    s: TcpStream,
    buf: Vec<u8>,
    peer: Peer,
}

/// The listener. `closed` is the scope, which does not accept HiSLIP.
pub struct Server {
    l: Option<TcpListener>,
    conns: Vec<Conn>,
    live: Vec<Live>,
    next: u16,
}

impl Server {
    pub fn open(port: u16) -> Server {
        let l = TcpListener::bind(("0.0.0.0", port)).ok();
        if let Some(l) = &l {
            let _ = l.set_nonblocking(true);
            eprintln!("oos-ui: HiSLIP on port {}", l.local_addr().map(|a| a.port()).unwrap_or(port));
        } else {
            eprintln!("oos-ui: HiSLIP port {} not available", port);
        }
        Server { l, conns: Vec::new(), live: Vec::new(), next: 1 }
    }

    pub fn closed() -> Server {
        Server { l: None, conns: Vec::new(), live: Vec::new(), next: 1 }
    }

    pub fn port(&self) -> Option<u16> {
        self.l.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port())
    }

    #[cfg(test)]
    pub fn poll(&mut self, exec: &mut dyn FnMut(&str) -> Vec<u8>) {
        self.poll_async(&mut |program| Some(program.immediate(exec)));
    }

    pub fn poll_async(&mut self, exec: &mut dyn FnMut(&mut crate::scpi::Program) -> Option<Vec<u8>>) {
        if let Some(l) = &self.l {
            while let Ok((s, _)) = l.accept() {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_millis(1)));
                let _ = s.set_nodelay(true);
                if self.conns.len() < MAX_CONNS {
                    self.conns.push(Conn { s, buf: Vec::new(), peer: Peer { role: Role::New, session: 0, pending: Vec::new(), msgid: 0, program: None } });
                }
            }
        }
        let mut i = 0;
        while i < self.conns.len() {
            let mut tmp = [0u8; 8192];
            let mut closed = false;
            let mut read_bytes = 0;
            loop {
                match self.conns[i].s.read(&mut tmp) {
                    Ok(0) => {
                        closed = true;
                        break;
                    }
                    Ok(n) => {
                        if self.conns[i].buf.len() + n > MAX_PAYLOAD + HEADER { closed = true; break; }
                        self.conns[i].buf.extend_from_slice(&tmp[..n]); read_bytes += n;
                        if read_bytes >= 65536 { break; }
                    },
                    Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                    Err(_) => {
                        closed = true;
                        break;
                    }
                }
            }
            let clearing = self.conns[i].buf.get(2).is_some_and(|typ| matches!(*typ, DEVICE_CLEAR_COMPLETE | ASYNC_DEVICE_CLEAR));
            if !closed && !clearing { if let Some(mut program) = self.conns[i].peer.program.take() {
                if let Some(mut response) = exec(&mut program) {
                    if program.has_reply {
                        response.push(b'\n');
                        let peer = &self.conns[i].peer;
                        let max = self.live.iter().find(|live| live.id == peer.session).map(|live| live.max_payload).unwrap_or(MAX_PAYLOAD);
                        for message in data_out(peer.msgid, &response, max) {
                            if !crate::scpi::send(&mut self.conns[i].s, &message) { closed = true; break; }
                        }
                    }
                } else { self.conns[i].peer.program = Some(program); }
            }
            }
            while !closed {
                if self.conns[i].peer.program.is_some() && !self.conns[i].buf.get(2)
                    .is_some_and(|typ| matches!(*typ, DEVICE_CLEAR_COMPLETE | ASYNC_DEVICE_CLEAR)) { break; }
                let msg = match take_message(&mut self.conns[i].buf) {
                    Ok(Some(m)) => m,
                    Ok(None) => break,
                    Err(()) => {
                        let _ = crate::scpi::send(&mut self.conns[i].s, &encode(FATAL_ERROR, FATAL_BAD_HEADER, 0, b""));
                        closed = true;
                        break;
                    }
                };
                let clear = matches!(msg.typ, DEVICE_CLEAR_COMPLETE | ASYNC_DEVICE_CLEAR);
                let out = on_program_message(&mut self.conns[i].peer, &mut self.live, &mut self.next, &msg, exec);
                if clear {
                    let id = self.conns[i].peer.session;
                    for c in &mut self.conns {
                        if c.peer.session == id {
                            c.peer.pending.clear(); c.peer.program = None; c.buf.clear();
                        }
                    }
                }
                for m in &out.msgs {
                    if !crate::scpi::send(&mut self.conns[i].s, m) {
                        closed = true;
                        break;
                    }
                }
                closed |= out.close;
            }
            if closed {
                if self.conns[i].peer.role == Role::Sync {
                    let id = self.conns[i].peer.session;
                    self.live.retain(|l| l.id != id);
                }
                self.conns.remove(i);
            } else {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn exec_idn(c: &str) -> Vec<u8> {
        if c == "*IDN?" {
            b"OOS,unit".to_vec()
        } else {
            Vec::new()
        }
    }

    #[test]
    fn empty_query_emits_data_end_but_writes_and_cancelled_fences_do_not() {
        let mut ui = crate::ui::Ui::new(crate::wave::Source::Demo { t: 0.0 });
        let cv = crate::gfx::Canvas::new(crate::ui::W, crate::ui::H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut peer = Peer { role: Role::Sync, session: 1, pending: Vec::new(), msgid: 0, program: None };
        let mut live = vec![Live { id: 1, max_payload: MAX_PAYLOAD }];
        let mut next = 2;
        let message = |id, command: &str| Message { typ: DATA_END, control: 0, param: id, data: command.as_bytes().to_vec() };
        let empty = on_program_message(&mut peer, &mut live, &mut next, &message(7, ":MEAS:CLEAR;:MEAS:STRIP?"),
            &mut |p| scpi.poll_program(&mut ui, &cv, p));
        assert_eq!(empty.msgs.len(), 1);
        let reply = take_message(&mut empty.msgs[0].clone()).unwrap().unwrap();
        assert_eq!((reply.typ, reply.param, reply.data), (DATA_END, 7, vec![b'\n']));
        let write = on_program_message(&mut peer, &mut live, &mut next, &message(8, ":MEAS:CLEAR"),
            &mut |p| scpi.poll_program(&mut ui, &cv, p));
        assert!(write.msgs.is_empty());
        let compound = on_program_message(&mut peer, &mut live, &mut next, &message(9, ":MEAS:STRIP?;*OPC?"),
            &mut |p| scpi.poll_program(&mut ui, &cv, p));
        assert_eq!(take_message(&mut compound.msgs[0].clone()).unwrap().unwrap().data, b";1\n");
        let pending = on_program_message(&mut peer, &mut live, &mut next,
            &message(10, ":MEAS:STRIP?;:TRIG:EDGE:LEV 10;:SINGLE;*OPC?;:RUN"),
            &mut |p| scpi.poll_program(&mut ui, &cv, p));
        assert!(pending.msgs.is_empty(), "an earlier empty query must not emit a premature response at a pending fence");
        let mut program = peer.program.take().unwrap();
        assert!(program.has_reply);
        scpi.exec_line(&mut ui, &cv, ":STOP");
        assert_eq!(scpi.poll_program(&mut ui, &cv, &mut program), Some(Vec::new()));
        assert!(!program.has_reply, "cancelled queries must not emit an empty DATA_END response");
        assert_eq!(ui.m.run, crate::model::RunState::Stop);
    }

    #[test]
    fn the_payload_is_the_scpi_and_the_lock_is_not_kept() {
        let mut peer = Peer { role: Role::New, session: 0, pending: Vec::new(), msgid: 0, program: None };
        let mut live = Vec::new();
        let mut next = 1u16;
        let mut exec = exec_idn;
        let init = on_message(&mut peer, &mut live, &mut next, &Message { typ: INITIALIZE, control: 0, param: 0x0100_0000, data: b"hislip0".to_vec() }, &mut exec);
        assert!(!init.close);
        let resp = take_message(&mut init.msgs[0].clone()).unwrap().unwrap();
        assert_eq!(resp.typ, INITIALIZE_RESPONSE);
        assert_eq!(resp.param >> 16, VERSION as u32);
        let id = resp.param as u16;
        assert_ne!(id, 0);
        // a second client is also told the lock succeeded: no session lock is stored
        let mut other = Peer { role: Role::Sync, session: id, pending: Vec::new(), msgid: 0, program: None };
        let lock = on_message(&mut other, &mut live, &mut next, &Message { typ: ASYNC_LOCK, control: 1, param: 0, data: Vec::new() }, &mut exec);
        let again = on_message(&mut peer, &mut live, &mut next, &Message { typ: ASYNC_LOCK, control: 1, param: 0, data: Vec::new() }, &mut exec);
        assert_eq!(take_message(&mut lock.msgs[0].clone()).unwrap().unwrap().control, 1);
        assert_eq!(take_message(&mut again.msgs[0].clone()).unwrap().unwrap().control, 1);
        // the payload may arrive in pieces; only the SCPI text is interpreted
        on_message(&mut peer, &mut live, &mut next, &Message { typ: DATA, control: 0, param: 1, data: b"*ID".to_vec() }, &mut exec);
        let end = on_message(&mut peer, &mut live, &mut next, &Message { typ: DATA_END, control: 1, param: 3, data: b"N?\n".to_vec() }, &mut exec);
        let data = take_message(&mut end.msgs[0].clone()).unwrap().unwrap();
        assert_eq!(data.typ, DATA_END);
        assert_eq!(data.param, 3);
        assert_eq!(data.data, b"OOS,unit\n");
    }

    #[test]
    fn a_socket_round_trip_answers_idn() {
        let mut h = Server::open(0);
        let port = h.port().expect("port");
        let t = std::thread::spawn(move || {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            c.write_all(&encode(INITIALIZE, 0, 0x0200_4F53, b"hislip0")).unwrap();
            let mut buf = Vec::new();
            let resp = read_one(&mut c, &mut buf);
            assert_eq!(resp.typ, INITIALIZE_RESPONSE);
            assert_eq!(resp.param >> 16, VERSION as u32, "a newer client is answered at 1.0");
            let id = resp.param as u16;
            let mut a = TcpStream::connect(("127.0.0.1", port)).unwrap();
            a.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            a.write_all(&encode(ASYNC_INITIALIZE, 0, id as u32, b"")).unwrap();
            let mut abuf = Vec::new();
            assert_eq!(read_one(&mut a, &mut abuf).typ, ASYNC_INITIALIZE_RESPONSE);
            c.write_all(&encode(DATA_END, 0, 0xffff_ff00, b"*IDN?\n")).unwrap();
            let data = read_one(&mut c, &mut buf);
            assert_eq!((data.typ, data.data.as_slice()), (DATA_END, &b"OOS,unit\n"[..]));
        });
        let start = std::time::Instant::now();
        while !t.is_finished() && start.elapsed() < Duration::from_secs(4) {
            h.poll(&mut exec_idn);
            std::thread::sleep(Duration::from_millis(5));
        }
        t.join().unwrap();
    }

    fn read_one(c: &mut TcpStream, buf: &mut Vec<u8>) -> Message {
        let mut tmp = [0u8; 256];
        let start = std::time::Instant::now();
        loop {
            if let Ok(Some(m)) = take_message(buf) {
                return m;
            }
            match c.read(&mut tmp) {
                Ok(0) => panic!("closed"),
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    if start.elapsed() > Duration::from_secs(3) {
                        panic!("timeout");
                    }
                }
                Err(e) => panic!("{e}"),
            }
        }
    }
}
