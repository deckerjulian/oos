// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Reading the remote-control connections (web remote, SCPI, VXI-11) from the frame loop.

use std::io;
use std::net::TcpStream;

/// What has arrived on `s`, without waiting: `WouldBlock` when nothing has. The sockets stay blocking for the answers
/// (a whole answer is written at once); only this read must not wait.
///
/// The servers used to read with a read timeout of 1 ms and a read after the last data to see that nothing more came.
/// The kernel rounds the timeout up to whole timer ticks (4 ms at 250 Hz, up to 8 ms), and it was paid in the frame
/// loop: for every web request, and for every open SCPI connection on every pass of the loop. 23 small web requests a
/// second took the scope's screen from 28 to 22 frames a second.
#[cfg(unix)]
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

/// Windows desktop build: no `MSG_DONTWAIT`; the socket is made non-blocking for this one read and blocking again
/// after, which gives the same answer (`WouldBlock` when nothing has arrived).
#[cfg(not(unix))]
pub fn read_now(s: &TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    use std::io::Read;
    s.set_nonblocking(true)?;
    let r = (&*s).read(buf);
    s.set_nonblocking(false)?;
    r
}

/// Whether a read error only means "nothing now" (try again in the next pass).
pub fn later(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted)
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
        let t = std::time::Instant::now();
        for _ in 0..100 {
            let e = read_now(&s, &mut b).unwrap_err();
            assert!(later(&e), "{:?}", e);
        }
        assert!(t.elapsed() < std::time::Duration::from_millis(50), "{:?} for 100 reads", t.elapsed());
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
}
