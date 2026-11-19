// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! HiSLIP (IVI-6.1) on TCP 4880, for a computer. The scope keeps VXI-11, where port 111 is free.
//!
//! A connection is accepted (from the addresses of `net.allow`, `netacl`) and the data payload is handed to the same
//! SCPI interpreter as the raw socket.
//! The session itself is not part of this: no lock is kept, overlap mode is not run, and remote/local is only
//! acknowledged. What is here is the header, the initial exchange that lets a client send that payload, and the
//! reply that carries the SCPI response back.

use std::net::{TcpListener, TcpStream};

pub const PORT: u16 = 4880;
const HEADER: usize = 16;
/// Same ceiling as one SCPI line (`:SOURce:ARB:DATA`).
const MAX_PAYLOAD: usize = 4 << 20;
/// Smallest reply payload of one message, whatever AsyncMaximumMessageSize asks: a smaller one only multiplies the
/// messages (one byte each made a message per byte of the reply).
const MIN_PAYLOAD: usize = 4 << 10;
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

/// A header that cannot be a HiSLIP message: the connection is to be closed with a fatal error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadHeader;

/// Takes one message from `buf`. `Err` is a header that cannot be a message (the connection should close).
pub fn take_message(buf: &mut Vec<u8>) -> Result<Option<Message>, BadHeader> {
    if buf.len() < HEADER {
        return Ok(None);
    }
    if &buf[..2] != b"HS" {
        return Err(BadHeader);
    }
    let n = u64::from_be_bytes(buf[8..16].try_into().unwrap()) as usize;
    if n > MAX_PAYLOAD {
        return Err(BadHeader);
    }
    if buf.len() < HEADER + n {
        return Ok(None);
    }
    let msg = Message {
        typ: buf[2],
        control: buf[3],
        param: u32::from_be_bytes(buf[4..8].try_into().unwrap()),
        data: buf[HEADER..HEADER + n].to_vec(),
    };
    buf.drain(..HEADER + n);
    Ok(Some(msg))
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    New,
    Sync,
    Async,
}

struct Peer<P> {
    role: Role,
    session: u16,
    pending: Vec<u8>,
    msgid: u32,
    program: Option<P>,
}

struct Live {
    id: u16,
    /// Largest SCPI response payload we will put in one message toward this client.
    max_payload: usize,
}

struct Out {
    msgs: Vec<Vec<u8>>,
    /// The SCPI response, sent after `msgs` as DATA messages and a DATA_END (`Reply::messages`).
    reply: Option<Reply>,
    close: bool,
}

/// A response and how it is split: each message is made as it is sent, so a long reply is not held twice.
struct Reply {
    msgid: u32,
    payload: Vec<u8>,
    max_payload: usize,
}

impl Reply {
    fn messages(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        let n = self.max_payload.max(MIN_PAYLOAD);
        let count = self.payload.len().div_ceil(n).max(1);
        (0..count).map(move |k| {
            let part = &self.payload[k * n..((k + 1) * n).min(self.payload.len())];
            encode(if k + 1 == count { DATA_END } else { DATA }, 0, self.msgid, part)
        })
    }
}

/// One message back.
fn answer(m: Vec<u8>) -> Out {
    Out { msgs: vec![m], reply: None, close: false }
}

/// Nothing back.
fn quiet() -> Out {
    Out { msgs: Vec::new(), reply: None, close: false }
}

fn fatal(code: u8) -> Out {
    Out { msgs: vec![encode(FATAL_ERROR, code, 0, b"")], reply: None, close: true }
}

fn error(code: u8) -> Out {
    answer(encode(ERROR, code, 0, b""))
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

fn on_program_message<P: crate::Program>(
    peer: &mut Peer<P>,
    live: &mut Vec<Live>,
    next: &mut u16,
    msg: &Message,
    exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>,
) -> Out {
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
            answer(encode(INITIALIZE_RESPONSE, 0, (VERSION as u32) << 16 | id as u32, b""))
        }
        ASYNC_INITIALIZE if peer.role == Role::New => {
            let id = msg.param as u16;
            if !live.iter().any(|l| l.id == id) {
                return fatal(FATAL_BAD_INIT);
            }
            peer.role = Role::Async;
            peer.session = id;
            answer(encode(ASYNC_INITIALIZE_RESPONSE, 0, 0, b""))
        }
        DATA | DATA_END if peer.role == Role::Sync => {
            if peer.pending.len() + msg.data.len() > MAX_PAYLOAD {
                peer.pending.clear();
                return error(ERR_LARGE);
            }
            peer.pending.extend_from_slice(&msg.data);
            peer.msgid = msg.param;
            if msg.typ == DATA {
                return quiet();
            }
            let payload = std::mem::take(&mut peer.pending);
            let mut program = P::new(&String::from_utf8_lossy(&payload));
            let Some(mut resp) = exec(&mut program) else {
                peer.program = Some(program);
                return quiet();
            };
            if !program.has_reply() {
                return quiet();
            }
            resp.push(b'\n');
            let max = live.iter().find(|l| l.id == peer.session).map(|l| l.max_payload).unwrap_or(MAX_PAYLOAD);
            let reply = Reply { msgid: peer.msgid, payload: resp, max_payload: max };
            Out { msgs: Vec::new(), reply: Some(reply), close: false }
        }
        ASYNC_MAXIMUM_MESSAGE_SIZE if peer.role != Role::New => {
            let n = msg.data.get(..8).map(|b| u64::from_be_bytes(b.try_into().unwrap())).unwrap_or(0);
            // the size covers the header; an empty value keeps the default, a small one gives MIN_PAYLOAD
            let payload = match n.checked_sub(HEADER as u64) {
                Some(p) if p > 0 => (p.min(MAX_PAYLOAD as u64) as usize).max(MIN_PAYLOAD),
                _ => MAX_PAYLOAD,
            };
            if let Some(l) = live.iter_mut().find(|l| l.id == peer.session) {
                l.max_payload = payload;
            }
            let ours = MAX_PAYLOAD as u64 + HEADER as u64;
            answer(encode(ASYNC_MAXIMUM_MESSAGE_SIZE_RESPONSE, 0, 0, &ours.to_be_bytes()))
        }
        ASYNC_LOCK if peer.role != Role::New => {
            // acknowledged so the payload can follow; nothing is stored
            answer(encode(ASYNC_LOCK_RESPONSE, 1, 0, b""))
        }
        ASYNC_LOCK_INFO if peer.role != Role::New => answer(encode(ASYNC_LOCK_INFO_RESPONSE, 0, 0, b"")),
        ASYNC_STATUS_QUERY if peer.role != Role::New => answer(encode(ASYNC_STATUS_RESPONSE, 0, 0, b"")),
        ASYNC_REMOTE_LOCAL_CONTROL if peer.role != Role::New => answer(encode(ASYNC_REMOTE_LOCAL_RESPONSE, 0, 0, b"")),
        DEVICE_CLEAR_COMPLETE if peer.role == Role::Sync => {
            peer.pending.clear();
            peer.program = None;
            answer(encode(DEVICE_CLEAR_ACKNOWLEDGE, 0, 0, b""))
        }
        ASYNC_DEVICE_CLEAR if peer.role == Role::Async => {
            peer.pending.clear();
            peer.program = None;
            answer(encode(ASYNC_DEVICE_CLEAR_ACKNOWLEDGE, 0, 0, b""))
        }
        // a trigger carries no SCPI payload
        12 if peer.role == Role::Sync => quiet(),
        _ if peer.role == Role::New => fatal(FATAL_BAD_INIT),
        _ => error(ERR_TYPE),
    }
}

#[cfg(test)]
fn on_message(
    peer: &mut Peer<crate::Line>,
    live: &mut Vec<Live>,
    next: &mut u16,
    msg: &Message,
    exec: &mut dyn FnMut(&str) -> Vec<u8>,
) -> Out {
    on_program_message(peer, live, next, msg, &mut |program| Some(program.immediate(exec)))
}

#[cfg(test)]
impl Server<crate::Line> {
    pub(crate) fn poll(&mut self, exec: &mut dyn FnMut(&str) -> Vec<u8>) {
        self.poll_async(&mut |program| Some(program.immediate(exec)));
    }
}

/// Tests of oos-ui: a sync session that takes DATA_END messages, with the real interpreter.
#[cfg(any(test, feature = "testkit"))]
pub struct SyncSession<P> {
    peer: Peer<P>,
    live: Vec<Live>,
    next: u16,
}

#[cfg(any(test, feature = "testkit"))]
impl<P: crate::Program> Default for SyncSession<P> {
    fn default() -> Self {
        let peer = Peer { role: Role::Sync, session: 1, pending: Vec::new(), msgid: 0, program: None };
        SyncSession { peer, live: vec![Live { id: 1, max_payload: MAX_PAYLOAD }], next: 2 }
    }
}

#[cfg(any(test, feature = "testkit"))]
impl<P: crate::Program> SyncSession<P> {
    /// The messages the server sends for a DATA_END with message id `id` and `text`, in order.
    pub fn data_end(&mut self, id: u32, text: &str, exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>) -> Vec<Message> {
        let msg = Message { typ: DATA_END, control: 0, param: id, data: text.as_bytes().to_vec() };
        let out = on_program_message(&mut self.peer, &mut self.live, &mut self.next, &msg, exec);
        let sent = out.msgs.into_iter().chain(out.reply.iter().flat_map(Reply::messages));
        sent.map(|mut m| take_message(&mut m).expect("a message").expect("a whole one")).collect()
    }
    /// The program that waits for an operation (`*OPC?`), taken out of the session.
    pub fn take_waiting(&mut self) -> Option<P> {
        self.peer.program.take()
    }
}

/// The type of a DATA_END message, for the tests of oos-ui.
#[cfg(any(test, feature = "testkit"))]
pub const DATA_END_TYPE: u8 = DATA_END;

struct Conn<P> {
    s: TcpStream,
    buf: Vec<u8>,
    peer: Peer<P>,
    /// the messages not yet written (#192): the next message is taken once they are out
    out: oos_system::net::Outbox,
    /// the connection ends once its messages are out (the client closed its side, or a fatal error was sent)
    closing: bool,
}

impl<P> Conn<P> {
    /// Writes what the socket takes now of the replies not yet written.
    fn flush(&mut self) -> oos_system::net::Sent {
        self.out.flush(&self.s)
    }
}

/// The listener. `closed` is the scope, which does not accept HiSLIP.
pub struct Server<P> {
    l: Option<TcpListener>,
    conns: Vec<Conn<P>>,
    live: Vec<Live>,
    next: u16,
    /// who may connect (`netacl`, net.allow; empty: everyone): new connections, and after a change those held
    pub acl: oos_system::netacl::Shared,
    /// the changes of `acl` looked at (`netacl::Shared::changed`)
    acl_seen: u64,
}

impl<P: crate::Program> Server<P> {
    pub fn open(port: u16) -> Self {
        let l = TcpListener::bind(("0.0.0.0", port)).ok();
        if let Some(l) = &l {
            let _ = l.set_nonblocking(true);
            eprintln!("oos-ui: HiSLIP on port {}", l.local_addr().map(|a| a.port()).unwrap_or(port));
        } else {
            eprintln!("oos-ui: HiSLIP port {} not available", port);
        }
        Server {
            l,
            conns: Vec::new(),
            live: Vec::new(),
            next: 1,
            acl: oos_system::netacl::Shared::default(),
            acl_seen: 0,
        }
    }

    pub fn closed() -> Self {
        let acl = oos_system::netacl::Shared::default();
        Server { l: None, conns: Vec::new(), live: Vec::new(), next: 1, acl, acl_seen: 0 }
    }

    pub fn port(&self) -> Option<u16> {
        self.l.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port())
    }

    pub fn poll_async(&mut self, exec: &mut dyn FnMut(&mut P) -> Option<Vec<u8>>) {
        if let Some(l) = &self.l {
            let conns = &mut self.conns;
            oos_system::net::accept_from(l, &self.acl, |s| {
                if conns.len() < MAX_CONNS {
                    conns.push(Conn {
                        s,
                        buf: Vec::new(),
                        peer: Peer { role: Role::New, session: 0, pending: Vec::new(), msgid: 0, program: None },
                        out: Default::default(),
                        closing: false,
                    });
                }
            });
        }
        // net.allow changed on the screen: a connection the list now leaves out is closed with its session
        let recheck = self.acl.changed(&mut self.acl_seen);
        let mut i = 0;
        while i < self.conns.len() {
            let mut closed = recheck && !self.acl.permits_peer(self.conns[i].s.peer_addr());
            // what is still to be written goes first, as much as the socket takes now (#192)
            closed |= self.conns[i].flush() == oos_system::net::Sent::Gone;
            let mut eof = self.conns[i].closing;
            if !closed && !eof {
                let c = &mut self.conns[i];
                let end = oos_system::net::read_some(&c.s, &mut c.buf, MAX_PAYLOAD + HEADER, 8192, 65536);
                (eof, closed) = (end.eof, end.closed);
            }
            let clearing =
                self.conns[i].buf.get(2).is_some_and(|typ| matches!(*typ, DEVICE_CLEAR_COMPLETE | ASYNC_DEVICE_CLEAR));
            // the messages of a connection are taken once the replies before them are out, also those a client sent
            // before it closed its side; a program that waits (`*OPC?`) is not resumed after the end of the input
            let mut stop = false;
            if eof {
                self.conns[i].peer.program = None;
            }
            let ready = !closed && self.conns[i].out.is_empty();
            if ready && !clearing {
                if let Some(mut program) = self.conns[i].peer.program.take() {
                    if let Some(mut response) = exec(&mut program) {
                        if program.has_reply() {
                            response.push(b'\n');
                            let peer = &self.conns[i].peer;
                            let max = self
                                .live
                                .iter()
                                .find(|live| live.id == peer.session)
                                .map(|live| live.max_payload)
                                .unwrap_or(MAX_PAYLOAD);
                            let reply = Reply { msgid: peer.msgid, payload: response, max_payload: max };
                            for message in reply.messages() {
                                self.conns[i].out.push(&message);
                            }
                            closed = self.conns[i].flush() == oos_system::net::Sent::Gone;
                        }
                    } else {
                        self.conns[i].peer.program = Some(program);
                    }
                }
            }
            while ready && !closed && !stop && self.conns[i].out.is_empty() {
                if self.conns[i].peer.program.is_some()
                    && !self.conns[i]
                        .buf
                        .get(2)
                        .is_some_and(|typ| matches!(*typ, DEVICE_CLEAR_COMPLETE | ASYNC_DEVICE_CLEAR))
                {
                    break;
                }
                let msg = match take_message(&mut self.conns[i].buf) {
                    Ok(Some(m)) => m,
                    Ok(None) => break,
                    Err(BadHeader) => {
                        self.conns[i].out.push(&encode(FATAL_ERROR, FATAL_BAD_HEADER, 0, b""));
                        stop = true;
                        break;
                    }
                };
                let clear = matches!(msg.typ, DEVICE_CLEAR_COMPLETE | ASYNC_DEVICE_CLEAR);
                let out = on_program_message(&mut self.conns[i].peer, &mut self.live, &mut self.next, &msg, exec);
                if clear {
                    let id = self.conns[i].peer.session;
                    for c in &mut self.conns {
                        if c.peer.session == id {
                            c.peer.pending.clear();
                            c.peer.program = None;
                            c.buf.clear();
                        }
                    }
                }
                let replies = out.reply.iter().flat_map(Reply::messages);
                for m in out.msgs.into_iter().chain(replies) {
                    self.conns[i].out.push(&m);
                }
                closed |= self.conns[i].flush() == oos_system::net::Sent::Gone;
                stop |= out.close;
            }
            // a client that closed its side, or one that was told a fatal error, goes once its messages are out
            // after a fatal error nothing more is taken from this connection
            if stop {
                self.conns[i].buf.clear();
            }
            if (eof || stop) && !closed {
                self.conns[i].flush();
                closed = self.conns[i].out.is_empty();
                self.conns[i].closing = true;
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
    use std::io::{ErrorKind, Read};
    use std::time::Duration;

    /// What the server sends for `out`, in order.
    fn sent(out: &Out) -> Vec<Vec<u8>> {
        out.msgs.iter().cloned().chain(out.reply.iter().flat_map(Reply::messages)).collect()
    }

    /// AsyncMaximumMessageSize of 17 bytes (a payload of one byte) made a message per byte of the reply: a 1 MiB reply
    /// became a list of a million messages. The payload of a message is now at least MIN_PAYLOAD, and the messages
    /// are made as they are sent.
    #[test]
    fn a_tiny_maximum_message_size_still_gives_messages_of_min_payload() {
        let mut peer = Peer { role: Role::New, session: 0, pending: Vec::new(), msgid: 0, program: None };
        let (mut live, mut next) = (Vec::new(), 1u16);
        let mut exec = |_: &str| vec![b'a'; 1 << 20];
        let init = Message { typ: INITIALIZE, control: 0, param: 0x0100_0000, data: b"hislip0".to_vec() };
        on_message(&mut peer, &mut live, &mut next, &init, &mut exec);
        let size = Message { typ: ASYNC_MAXIMUM_MESSAGE_SIZE, control: 0, param: 0, data: 17u64.to_be_bytes().into() };
        on_message(&mut peer, &mut live, &mut next, &size, &mut exec);
        let query = Message { typ: DATA_END, control: 0, param: 5, data: b"X?\n".to_vec() };
        let out = sent(&on_message(&mut peer, &mut live, &mut next, &query, &mut exec));
        assert_eq!(out.len(), ((1 << 20) + 1usize).div_ceil(MIN_PAYLOAD));
        let mut back = Vec::new();
        for (k, m) in out.iter().enumerate() {
            let m = take_message(&mut m.clone()).unwrap().unwrap();
            assert_eq!((m.typ, m.param), (if k + 1 == out.len() { DATA_END } else { DATA }, 5));
            assert!(m.data.len() <= MIN_PAYLOAD);
            back.extend_from_slice(&m.data);
        }
        assert_eq!((back.len(), back[0], back[back.len() - 1]), ((1 << 20) + 1, b'a', b'\n'));
        // a reply of exactly one message's payload is one DATA_END
        let reply = Reply { msgid: 1, payload: vec![0; MIN_PAYLOAD], max_payload: MIN_PAYLOAD };
        assert_eq!(reply.messages().count(), 1);
        let empty = Reply { msgid: 1, payload: Vec::new(), max_payload: MIN_PAYLOAD };
        assert_eq!(take_message(&mut empty.messages().next().unwrap()).unwrap().unwrap().typ, DATA_END);
    }

    fn exec_idn(c: &str) -> Vec<u8> {
        if c == "*IDN?" {
            b"OOS,unit".to_vec()
        } else {
            Vec::new()
        }
    }

    #[test]
    fn the_payload_is_the_scpi_and_the_lock_is_not_kept() {
        let mut peer = Peer { role: Role::New, session: 0, pending: Vec::new(), msgid: 0, program: None };
        let mut live = Vec::new();
        let mut next = 1u16;
        let mut exec = exec_idn;
        let init = on_message(
            &mut peer,
            &mut live,
            &mut next,
            &Message { typ: INITIALIZE, control: 0, param: 0x0100_0000, data: b"hislip0".to_vec() },
            &mut exec,
        );
        assert!(!init.close);
        let resp = take_message(&mut init.msgs[0].clone()).unwrap().unwrap();
        assert_eq!(resp.typ, INITIALIZE_RESPONSE);
        assert_eq!(resp.param >> 16, VERSION as u32);
        let id = resp.param as u16;
        assert_ne!(id, 0);
        // a second client is also told the lock succeeded: no session lock is stored
        let mut other = Peer { role: Role::Sync, session: id, pending: Vec::new(), msgid: 0, program: None };
        let lock = on_message(
            &mut other,
            &mut live,
            &mut next,
            &Message { typ: ASYNC_LOCK, control: 1, param: 0, data: Vec::new() },
            &mut exec,
        );
        let again = on_message(
            &mut peer,
            &mut live,
            &mut next,
            &Message { typ: ASYNC_LOCK, control: 1, param: 0, data: Vec::new() },
            &mut exec,
        );
        assert_eq!(take_message(&mut lock.msgs[0].clone()).unwrap().unwrap().control, 1);
        assert_eq!(take_message(&mut again.msgs[0].clone()).unwrap().unwrap().control, 1);
        // the payload may arrive in pieces; only the SCPI text is interpreted
        on_message(
            &mut peer,
            &mut live,
            &mut next,
            &Message { typ: DATA, control: 0, param: 1, data: b"*ID".to_vec() },
            &mut exec,
        );
        let end = on_message(
            &mut peer,
            &mut live,
            &mut next,
            &Message { typ: DATA_END, control: 1, param: 3, data: b"N?\n".to_vec() },
            &mut exec,
        );
        let data = take_message(&mut sent(&end)[0]).unwrap().unwrap();
        assert_eq!(data.typ, DATA_END);
        assert_eq!(data.param, 3);
        assert_eq!(data.data, b"OOS,unit\n");
    }

    #[test]
    fn a_socket_round_trip_answers_idn() {
        let mut h = Server::<crate::Line>::open(0);
        let port = h.port().expect("port");
        // no deadline: a missing answer hangs the test, a slow or busy computer does not fail it (ADR 0020)
        let t = std::thread::spawn(move || {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            c.write_all(&encode(INITIALIZE, 0, 0x0200_4F53, b"hislip0")).unwrap();
            let mut buf = Vec::new();
            let resp = read_one(&mut c, &mut buf);
            assert_eq!(resp.typ, INITIALIZE_RESPONSE);
            assert_eq!(resp.param >> 16, VERSION as u32, "a newer client is answered at 1.0");
            let id = resp.param as u16;
            let mut a = TcpStream::connect(("127.0.0.1", port)).unwrap();
            a.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            a.write_all(&encode(ASYNC_INITIALIZE, 0, id as u32, b"")).unwrap();
            let mut abuf = Vec::new();
            assert_eq!(read_one(&mut a, &mut abuf).typ, ASYNC_INITIALIZE_RESPONSE);
            c.write_all(&encode(DATA_END, 0, 0xffff_ff00, b"*IDN?\n")).unwrap();
            let data = read_one(&mut c, &mut buf);
            assert_eq!((data.typ, data.data.as_slice()), (DATA_END, &b"OOS,unit\n"[..]));
        });
        while !t.is_finished() {
            h.poll(&mut exec_idn);
            std::thread::sleep(Duration::from_millis(5));
        }
        t.join().unwrap();
    }

    /// A client that sends a query and closes its side at once (#192, Copilot's review of #269) still gets the
    /// answer before the connection goes.
    #[test]
    fn a_query_before_the_end_of_the_input_is_still_answered() {
        let mut h = Server::<crate::Line>::open(0);
        let port = h.port().expect("port");
        let t = std::thread::spawn(move || {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            c.write_all(&encode(INITIALIZE, 0, 0x0100_4F53, b"hislip0")).unwrap();
            let mut buf = Vec::new();
            assert_eq!(read_one(&mut c, &mut buf).typ, INITIALIZE_RESPONSE);
            c.write_all(&encode(DATA_END, 0, 0xffff_ff00, b"*IDN?\n")).unwrap();
            c.shutdown(std::net::Shutdown::Write).unwrap();
            let data = read_one(&mut c, &mut buf);
            assert_eq!((data.typ, data.data.as_slice()), (DATA_END, &b"OOS,unit\n"[..]));
        });
        while !t.is_finished() {
            h.poll(&mut exec_idn);
            std::thread::sleep(Duration::from_millis(5));
        }
        t.join().unwrap();
    }

    fn read_one(c: &mut TcpStream, buf: &mut Vec<u8>) -> Message {
        let mut tmp = [0u8; 256];
        loop {
            if let Ok(Some(m)) = take_message(buf) {
                return m;
            }
            match c.read(&mut tmp) {
                Ok(0) => panic!("closed"),
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// `take_message` and `on_message` (HiSLIP on TCP 4880), ADR 0047: return for any bytes, on a new connection, a
    /// synchronous and an asynchronous one; a header is refused only for its prologue or its length; a message taken
    /// is its header and its data and no more; every answer is a whole message; what a peer has pending and the
    /// sessions stay bounded.
    #[test]
    fn hostile_messages() {
        let sync = [
            encode(INITIALIZE, 0, 0x0100_0000, b"hislip0"),
            encode(DATA, 0, 1, b":CHAN1:SCAL 0.5;"),
            encode(DATA_END, 0, 2, b"*IDN?\n"),
            encode(12, 0, 3, b""),
            encode(DEVICE_CLEAR_COMPLETE, 0, 0, b""),
        ]
        .concat();
        let asynchronous = [
            encode(ASYNC_INITIALIZE, 0, 1, b""),
            encode(ASYNC_MAXIMUM_MESSAGE_SIZE, 0, 0, &(1u64 << 20).to_be_bytes()),
            encode(ASYNC_LOCK, 1, 0, b""),
            encode(ASYNC_LOCK_INFO, 0, 0, b""),
            encode(ASYNC_STATUS_QUERY, 0, 0, b""),
            encode(ASYNC_REMOTE_LOCAL_CONTROL, 0, 0, b""),
            encode(ASYNC_DEVICE_CLEAR, 0, 0, b""),
        ]
        .concat();
        let seeds: [&[u8]; 3] = [&sync, &asynchronous, &sync[23..]];
        crate::hostile::run("hislip::take_message", &seeds, 1, |b| {
            for role in [Role::New, Role::Sync, Role::Async] {
                let mut buf = b.to_vec();
                // a session is open: a second connection can join it
                let mut live = vec![Live { id: 1, max_payload: MAX_PAYLOAD }];
                let mut next = 2u16;
                let mut peer = Peer { role, session: 1, pending: Vec::new(), msgid: 0, program: None };
                let mut exec = |c: &str| if c.ends_with('?') { b"OOS,unit".to_vec() } else { Vec::new() };
                loop {
                    let before = buf.clone();
                    let m = match take_message(&mut buf) {
                        Ok(Some(m)) => m,
                        Ok(None) => break,
                        Err(BadHeader) => {
                            let long = u64::from_be_bytes(before[8..16].try_into().unwrap()) > MAX_PAYLOAD as u64;
                            if &before[..2] == b"HS" && !long {
                                return Err("refused a header with the prologue and a length within the limit".into());
                            }
                            break;
                        }
                    };
                    if before.len() - buf.len() != HEADER + m.data.len() || m.data.len() > MAX_PAYLOAD {
                        return Err(format!("a message of {} bytes took {}", m.data.len(), before.len() - buf.len()));
                    }
                    let out = on_message(&mut peer, &mut live, &mut next, &m, &mut exec);
                    for mut frame in sent(&out) {
                        match take_message(&mut frame) {
                            Ok(Some(a)) if frame.is_empty() && a.data.len() <= MAX_PAYLOAD => {}
                            _ => return Err(format!("an answer to type {} that is no message", m.typ)),
                        }
                    }
                    if peer.pending.len() > MAX_PAYLOAD || live.len() > 2 {
                        return Err(format!("{} bytes pending, {} sessions", peer.pending.len(), live.len()));
                    }
                    if out.close {
                        break;
                    }
                }
            }
            Ok(())
        });
    }
}
