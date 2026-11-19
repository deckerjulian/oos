// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Reading and writing the remote-control connections (web remote, SCPI, VXI-11, HiSLIP) from the frame loop, without
//! waiting: nothing in the loop may block on the network (#192).

use std::io;
use std::net::TcpStream;
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
}

/// What `Outbox::flush` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// everything is out
    Done,
    /// some is left for a later pass
    Later,
    /// the client is given up: an error, or no byte for `SEND_STALL`, or not out after `SEND_LIMIT`
    Gone,
}

impl Outbox {
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
                        *self = Outbox::default();
                        return Sent::Done;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if later(&e) => break,
                Err(_) => return Sent::Gone,
            }
        }
        let moved = *self.moved.get_or_insert(now);
        if now.duration_since(moved) >= SEND_STALL || now.duration_since(started) >= SEND_LIMIT {
            Sent::Gone
        } else {
            Sent::Later
        }
    }
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
        // the socket takes what its buffers hold (on Linux several MB), then nothing
        let mut left = usize::MAX;
        while out.data.len() - out.at < left {
            left = out.data.len() - out.at;
            assert_eq!(out.flush_at(&s, t0), Sent::Later);
        }
        assert_eq!(out.flush_at(&s, t0 + SEND_STALL / 2), Sent::Later);
        assert_eq!(out.flush_at(&s, t0 + SEND_STALL), Sent::Gone);
        let mut empty = Outbox::default();
        assert_eq!(empty.flush(&s), Sent::Done);
    }
}
