// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Reading and writing the remote-control connections (web remote, SCPI, VXI-11, HiSLIP) from the frame loop, without
//! waiting: nothing in the loop may block on the network (#192).

use std::io;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

/// A reply that moves no byte for this long gives its client up: it takes no data.
pub const SEND_STALL: Duration = Duration::from_secs(2);
/// A reply that is not out after this long gives its client up too.
pub const SEND_LIMIT: Duration = Duration::from_secs(30);
/// The most one message of a client may answer: one answer may be a long record, ten thousand of them in one line
/// may not.
pub const RESP_MAX: usize = 64 << 20;
/// The most one connection writes in one pass of the frame loop: a client that reads fast still gets its reply over
/// several passes (at 30 passes a second still 30 MB/s), so it cannot keep the loop writing either.
pub const FLUSH_MAX: usize = 1 << 20;

/// The TCP port of the VXI-11 core channel (oos-remote's vxi11.rs serves it, its portmapper on 111 names it; System >
/// Network > Remote services shows it).
pub const VXI11_CORE_PORT: u16 = 9009;

/// What has arrived on `s`, without waiting: `WouldBlock` when nothing has. The sockets themselves stay blocking; this
/// read and `write_now` do not wait, each for itself.
///
/// The servers used to read with a read timeout of 1 ms and a read after the last data to see that nothing more came.
/// The kernel rounds the timeout up to whole timer ticks (4 ms at 250 Hz, up to 8 ms), and it was paid in the frame
/// loop: for every web request, and for every open SCPI connection on every pass of the loop. 23 small web requests a
/// second took the scope's screen from 28 to 22 frames a second.
pub fn read_now(s: &TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    use std::os::fd::AsRawFd;
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes; the descriptor belongs to `s` for the call.
    let n = unsafe { libc::recv(s.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len(), libc::MSG_DONTWAIT) };
    if n >= 0 {
        Ok(n as usize)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Whether a read error only means "nothing now" (try again in the next pass).
pub fn later(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted)
}

/// Takes the connections waiting on `l`, without waiting. One from a peer `acl` leaves out is dropped, which closes it;
/// the others are made blocking again, with no Nagle delay, and handed to `take` (which may drop one too, as when the
/// server has all the connections it takes).
pub fn accept_from(l: &TcpListener, acl: &crate::netacl::Shared, mut take: impl FnMut(TcpStream)) {
    while let Ok((s, peer)) = l.accept() {
        if !acl.permits_peer(Ok(peer)) {
            continue;
        }
        let _ = s.set_nonblocking(false);
        let _ = s.set_nodelay(true);
        take(s);
    }
}

/// What `read_some` saw: the peer closed its side (`eof`), or the connection is to be closed (`closed`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadEnd {
    pub eof: bool,
    pub closed: bool,
}

/// Reads what has arrived on `s` into `buf`, without waiting, in pieces of up to `chunk` bytes (at most 8192), and stops
/// after `budget` bytes, so the frame loop serves every connection in its turn. `closed` when `buf` would grow over
/// `cap` or the read fails.
pub fn read_some(s: &TcpStream, buf: &mut Vec<u8>, cap: usize, chunk: usize, budget: usize) -> ReadEnd {
    let mut tmp = [0u8; 8192];
    let tmp = &mut tmp[..chunk.clamp(1, 8192)];
    let mut end = ReadEnd::default();
    let mut read = 0;
    loop {
        match read_now(s, tmp) {
            Ok(0) => {
                end.eof = true;
                break;
            }
            Ok(n) => {
                if buf.len() + n > cap {
                    end.closed = true;
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                read += n;
                if read >= budget {
                    break;
                }
            }
            Err(e) if later(&e) => break,
            Err(_) => {
                end.closed = true;
                break;
            }
        }
    }
    end
}

/// Writes what `s` takes of `buf` now, without waiting: `WouldBlock` when it takes nothing. The socket does not block
/// for this one write and blocks again after it (MSG_DONTWAIT, as `read_now` uses it, is taken on a receive only on
/// macOS). A peer that is gone gives an error, not a signal: Rust's runtime ignores SIGPIPE.
pub fn write_now(s: &TcpStream, buf: &[u8]) -> io::Result<usize> {
    use std::io::Write;
    s.set_nonblocking(true)?;
    let r = (&*s).write(buf);
    s.set_nonblocking(false)?;
    r
}

/// What a server still has to write to one client. The frame loop writes what the socket takes on each pass and goes
/// on (#192): a client that reads slowly costs a write that does not wait, not the screen. Before, a reply was written
/// at once, and a client that took a little every 2 s held the loop for up to 30 s per reply.
#[derive(Default)]
pub struct Outbox {
    data: Vec<u8>,
    at: usize,
    /// when the reply now going out was started, and when it last moved a byte
    started: Option<Instant>,
    moved: Option<Instant>,
    limits: Limits,
}

/// When an `Outbox` gives its client up: no byte taken for `stall`, or a reply not out after `total`. The servers use
/// the default (`SEND_STALL`, `SEND_LIMIT`); a test that keeps a client from reading on purpose sets them far out, so
/// the time a loaded computer takes does not decide it (ADR 0020).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub stall: Duration,
    pub total: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { stall: SEND_STALL, total: SEND_LIMIT }
    }
}

/// What `Outbox::flush` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// everything is out
    Done,
    /// some is left for a later pass
    Later,
    /// the client is given up: an error, or no byte for `Limits::stall`, or not out after `Limits::total`
    Gone,
}

impl Outbox {
    /// An empty outbox that gives its client up at `limits`.
    pub fn with(limits: Limits) -> Outbox {
        Outbox { limits, ..Outbox::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.at >= self.data.len()
    }

    /// `d` after what is there.
    pub fn push(&mut self, d: &[u8]) {
        if self.is_empty() {
            self.data.clear();
            self.at = 0;
        }
        self.data.extend_from_slice(d);
    }

    /// Writes what `s` takes now.
    pub fn flush(&mut self, s: &TcpStream) -> Sent {
        self.flush_at(s, Instant::now())
    }

    fn flush_at(&mut self, s: &TcpStream, now: Instant) -> Sent {
        if self.is_empty() {
            return Sent::Done;
        }
        let started = *self.started.get_or_insert(now);
        let mut budget = FLUSH_MAX;
        while budget > 0 {
            let end = self.data.len().min(self.at + budget);
            match write_now(s, &self.data[self.at..end]) {
                Ok(0) => return Sent::Gone,
                Ok(n) => {
                    self.at += n;
                    budget -= n;
                    self.moved = Some(now);
                    if self.is_empty() {
                        // a long reply does not keep its memory
                        *self = Outbox::with(self.limits);
                        return Sent::Done;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if later(&e) => break,
                Err(_) => return Sent::Gone,
            }
        }
        let moved = *self.moved.get_or_insert(now);
        if now.duration_since(moved) >= self.limits.stall || now.duration_since(started) >= self.limits.total {
            Sent::Gone
        } else {
            Sent::Later
        }
    }
}

/// The answers of the web remote above `SEND_INLINE` of oos-remote's http.rs, and its live pictures, go out on threads
/// of their own: over WLAN a picture takes long enough to stall the frame loop. At most this many at once; more are
/// answered 503, and the page tries again.
pub const MAX_SENDERS: usize = 2;
pub const SEND_TIMEOUT_S: u64 = 30;
pub static SENDERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A large answer in the background. False when too many run already, or no thread could be started.
pub fn send_in_background(s: &TcpStream, head: String, body: impl AsRef<[u8]> + Send + 'static) -> bool {
    answer_in_background(s, move || (head, body))
}

/// An answer made and sent in the background, one of the `MAX_SENDERS`: `make` gives its head and body there (an
/// answer that takes a while to make, as GET /update's state). False when too many run already, or no thread could
/// be started; then `make` is not called.
pub fn answer_in_background<B: AsRef<[u8]>>(
    s: &TcpStream,
    make: impl FnOnce() -> (String, B) + Send + 'static,
) -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    let Ok(mut c) = s.try_clone() else { return false };
    if SENDERS.fetch_add(1, SeqCst) >= MAX_SENDERS {
        SENDERS.fetch_sub(1, SeqCst);
        return false;
    }
    let t = std::thread::Builder::new().name("web-send".into()).spawn(move || {
        crate::system::lower_priority();
        let (head, body) = make();
        let _ = c.set_write_timeout(Some(Duration::from_secs(SEND_TIMEOUT_S)));
        use std::io::Write;
        let _ = c.write_all(head.as_bytes());
        let _ = c.write_all(body.as_ref());
        SENDERS.fetch_sub(1, SeqCst);
    });
    if t.is_err() {
        SENDERS.fetch_sub(1, SeqCst);
        return false;
    }
    true
}

/// 503 on a connection nothing was written to yet: under 200 bytes, which its empty send buffer takes at once.
pub fn busy_answer(s: &TcpStream) {
    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
    let msg = "busy, try again";
    use std::io::Write;
    let _ = (&*s).write_all(
        format!(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
            Cache-Control: no-store\r\nConnection: close\r\n\r\n{}",
            msg.len(),
            msg
        )
        .as_bytes(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Nothing there: at once WouldBlock, also on a socket that blocks for writes; then the data; then 0 at the end.
    #[test]
    fn reads_without_waiting() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let mut b = [0u8; 16];
        // nothing is sent before these reads: one that waited would never return
        for _ in 0..100 {
            let e = read_now(&s, &mut b).unwrap_err();
            assert!(later(&e), "{:?}", e);
        }
        c.write_all(b"*IDN?\n").unwrap();
        let t = std::time::Instant::now();
        let n = loop {
            match read_now(&s, &mut b) {
                Ok(n) => break n,
                Err(e) if later(&e) && t.elapsed().as_secs() < 2 => continue,
                Err(e) => panic!("{:?}", e),
            }
        };
        assert_eq!(&b[..n], b"*IDN?\n");
        drop(c);
        let t = std::time::Instant::now();
        loop {
            match read_now(&s, &mut b) {
                Ok(0) => break,
                Err(e) if later(&e) && t.elapsed().as_secs() < 2 => continue,
                r => panic!("{:?}", r),
            }
        }
    }

    /// `read_some` (the reads of SCPI, VXI-11 and HiSLIP): nothing there returns at once with no end; the data come in
    /// calls of at most the budget, in pieces; the end after the data; a piece that would take the buffer over its cap
    /// closes the connection and is not kept.
    #[test]
    fn read_some_stops_at_its_budget_the_end_and_the_cap() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let mut buf = Vec::new();
        assert_eq!(read_some(&s, &mut buf, 1000, 10, 30), ReadEnd::default());
        assert!(buf.is_empty());
        c.write_all(&[7u8; 100]).unwrap();
        let t = Instant::now();
        let mut calls = 0;
        while buf.len() < 100 && t.elapsed().as_secs() < 2 {
            let before = buf.len();
            assert_eq!(read_some(&s, &mut buf, 1000, 10, 30), ReadEnd::default());
            assert!(buf.len() - before <= 30, "one call read {} of a budget of 30", buf.len() - before);
            calls += (buf.len() > before) as usize;
        }
        assert_eq!(buf, vec![7u8; 100]);
        assert!(calls >= 4, "100 bytes in calls of at most 30: {} calls", calls);
        drop(c);
        let t = Instant::now();
        let end = loop {
            let e = read_some(&s, &mut buf, 1000, 10, 30);
            if e != ReadEnd::default() || t.elapsed().as_secs() >= 2 {
                break e;
            }
        };
        assert_eq!(end, ReadEnd { eof: true, closed: false });
        assert_eq!(buf.len(), 100);
        // 40 bytes already there, a cap of 45: the first piece of 10 would go over it
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        c.write_all(&[1u8; 64]).unwrap();
        let mut buf = vec![0u8; 40];
        let t = Instant::now();
        let end = loop {
            let e = read_some(&s, &mut buf, 45, 10, 30);
            if e != ReadEnd::default() || t.elapsed().as_secs() >= 2 {
                break e;
            }
        };
        assert_eq!(end, ReadEnd { eof: false, closed: true });
        assert_eq!(buf, vec![0u8; 40], "the piece over the cap is not kept");
    }

    /// A client that reads nothing: the first flush returns at once with most of a long reply left, the reply goes out
    /// as the client reads, and a client that stops taking data is given up after `SEND_STALL`, not waited for.
    #[test]
    fn a_reply_goes_out_as_the_client_takes_it() {
        use std::io::Read;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let reply: Vec<u8> = (0..16 << 20).map(|i| i as u8).collect();
        let mut out = Outbox::default();
        out.push(&reply);
        // the flush returns with the reply not out: nothing reads it yet
        assert_eq!(out.flush(&s), Sent::Later);
        assert!(!out.is_empty());
        let reader = std::thread::spawn(move || {
            let mut got = Vec::new();
            c.read_to_end(&mut got).unwrap();
            got
        });
        let t = Instant::now();
        while out.flush(&s) == Sent::Later {
            assert!(t.elapsed() < Duration::from_secs(20));
            std::thread::yield_now();
        }
        assert!(out.is_empty());
        drop(s);
        assert!(reader.join().unwrap() == reply, "the reply arrives whole and in order");
    }

    /// However fast the client reads, one flush writes at most FLUSH_MAX: the rest waits for the next pass.
    #[test]
    fn one_flush_writes_at_most_its_budget() {
        use std::io::Read;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let reader = std::thread::spawn(move || {
            let mut got = Vec::new();
            c.read_to_end(&mut got).unwrap();
            got.len()
        });
        let mut out = Outbox::default();
        out.push(&vec![7u8; 4 * FLUSH_MAX]);
        let mut passes = 0;
        loop {
            let left = out.data.len() - out.at;
            let sent = out.flush(&s);
            assert!(left - (out.data.len().saturating_sub(out.at)) <= FLUSH_MAX, "one pass wrote more than its budget");
            passes += 1;
            if sent == Sent::Done {
                break;
            }
            assert_ne!(sent, Sent::Gone);
            std::thread::yield_now();
        }
        assert!(passes >= 4, "{} passes", passes);
        drop(s);
        assert_eq!(reader.join().unwrap(), 4 * FLUSH_MAX);
    }

    #[test]
    fn a_client_that_takes_nothing_is_given_up() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let _c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        let mut out = Outbox::default();
        out.push(&vec![0u8; 16 << 20]);
        let t0 = Instant::now();
        // the socket takes what its buffers hold (on Linux several MB), then nothing. The kernel may still move bytes
        // to the peer a while later (sooner or later under load), and the next flush then writes again: the clock
        // goes on in quarter steps until the outbox gives the client up, which must be a whole `SEND_STALL` after the
        // last byte moved, never sooner, and must come (the client never reads)
        assert_eq!(out.flush_at(&s, t0), Sent::Later, "a reply of 16 MB does not fit at once");
        let mut t = t0;
        let gone_at = loop {
            t += SEND_STALL / 4;
            match out.flush_at(&s, t) {
                Sent::Later => assert!(t < t0 + 1000 * SEND_STALL, "never given up"),
                Sent::Gone => break t,
                Sent::Done => panic!("a client that reads nothing took 16 MB"),
            }
        };
        let moved = out.moved.expect("bytes moved at the first flush");
        assert!(gone_at.duration_since(moved) >= SEND_STALL, "given up {:?} after the last move", gone_at - moved);
        assert!(gone_at.duration_since(moved) < SEND_STALL + SEND_STALL / 4, "not given up a step after the stall");
        let mut empty = Outbox::default();
        assert_eq!(empty.flush(&s), Sent::Done);
    }

    /// The 503 has the headers of every other answer of the web server: its type, its length, `no-store`.
    #[test]
    fn busy_answer_is_a_whole_answer() {
        use std::io::Read;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        busy_answer(&s);
        drop(s);
        let mut got = String::new();
        c.read_to_string(&mut got).unwrap();
        let (head, body) = got.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 503 Service Unavailable\r\n"), "{head}");
        for h in ["Content-Type: text/plain", "Content-Length: 15", "Cache-Control: no-store", "Connection: close"] {
            assert!(head.lines().any(|l| l == h), "{h} in {head}");
        }
        assert_eq!(body, "busy, try again");
    }
}
