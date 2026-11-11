// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! A small SNTP client (RFC 4330, one request and one answer over UDP port 123) and the system clock. The image's
//! busybox has no ntpd, and one exchange an hour is all a scope without an RTC battery needs (System > Date and time).

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub const PORT: u16 = 123;
/// Seconds from 1900-01-01 (NTP) to 1970-01-01 (Unix).
const NTP_UNIX: i64 = 2_208_988_800;
/// How long one server address may take to answer.
pub const TIMEOUT: Duration = Duration::from_secs(3);
/// Offsets below this are left alone (no step for a few milliseconds).
pub const STEP_MIN: f64 = 0.05;

/// Unix seconds as an NTP timestamp (32.32 fixed point).
pub fn to_ntp(unix: f64) -> u64 {
    let s = unix.floor();
    let secs = (s as i64 + NTP_UNIX) as u64 & 0xffff_ffff;
    let frac = (((unix - s) * 4_294_967_296.0) as u64).min(0xffff_ffff);
    secs << 32 | frac
}

/// An NTP timestamp as Unix seconds. Era by the top bit (RFC 4330 section 3): set = 1968..2036, clear = 2036..2104.
pub fn from_ntp(ts: u64) -> f64 {
    let secs = (ts >> 32) as i64;
    let secs = if secs & 0x8000_0000 != 0 { secs } else { secs + (1 << 32) };
    (secs - NTP_UNIX) as f64 + (ts & 0xffff_ffff) as f64 / 4_294_967_296.0
}

/// A client request: version 4, mode 3, our clock in the transmit field (the server echoes it as the originate time).
pub fn request(t1: u64) -> [u8; 48] {
    let mut b = [0u8; 48];
    b[0] = 4 << 3 | 3;
    b[40..48].copy_from_slice(&t1.to_be_bytes());
    b
}

/// The part of an answer that counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reply {
    pub stratum: u8,
    /// receive and transmit time of the server, Unix seconds
    pub t2: f64,
    pub t3: f64,
}

fn ts(b: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(b[at..at + 8].try_into().unwrap())
}

/// Checks an answer to the request that carried `t1`.
pub fn parse(b: &[u8], t1: u64) -> Result<Reply, String> {
    if b.len() < 48 {
        return Err("short answer".into());
    }
    let (li, vn, mode, stratum) = (b[0] >> 6, b[0] >> 3 & 7, b[0] & 7, b[1]);
    if mode != 4 || !(1..=4).contains(&vn) {
        return Err("not an NTP server answer".into());
    }
    if ts(b, 24) != t1 {
        return Err("answer to another request".into());
    }
    if stratum == 0 {
        // kiss-o'-death: the code is in the reference id
        let code: String = b[12..16].iter().map(|&c| if c.is_ascii_alphanumeric() { c as char } else { '?' }).collect();
        return Err(format!("server refused ({})", code));
    }
    if li == 3 || stratum > 15 {
        return Err("server not synchronised".into());
    }
    let (t2, t3) = (ts(b, 32), ts(b, 40));
    if t3 == 0 {
        return Err("answer without a time".into());
    }
    Ok(Reply { stratum, t2: from_ntp(t2), t3: from_ntp(t3) })
}

/// Clock offset (add it to the local clock) and round-trip delay, from the four times of one exchange.
pub fn offset_delay(t1: f64, t2: f64, t3: f64, t4: f64) -> (f64, f64) {
    (((t2 - t1) + (t3 - t4)) / 2.0, (t4 - t1) - (t3 - t2))
}

/// The result of one exchange.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub offset: f64,
    pub delay: f64,
    pub stratum: u8,
    pub addr: SocketAddr,
}

fn ask(addr: SocketAddr) -> Result<Sample, String> {
    let any: SocketAddr = if addr.is_ipv4() { "0.0.0.0:0".parse().unwrap() } else { "[::]:0".parse().unwrap() };
    let s = UdpSocket::bind(any).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    s.connect(addr).map_err(|e| e.to_string())?;
    let t1f = crate::clock::unix_now();
    let t1 = to_ntp(t1f);
    s.send(&request(t1)).map_err(|e| e.to_string())?;
    let mut b = [0u8; 128];
    // a stray datagram does not end the wait
    let end = std::time::Instant::now() + TIMEOUT;
    loop {
        let n = match s.recv(&mut b) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Err("no answer".into()),
            Err(e) => return Err(e.to_string()),
        };
        let t4 = crate::clock::unix_now();
        match parse(&b[..n], t1) {
            Ok(r) => {
                let (offset, delay) = offset_delay(t1f, r.t2, r.t3, t4);
                return Ok(Sample { offset, delay, stratum: r.stratum, addr });
            }
            Err(e) if e == "answer to another request" && std::time::Instant::now() < end => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Asks `server` (a name or an address) for the time; the first of its addresses that answers counts.
pub fn query(server: &str) -> Result<Sample, String> {
    let addrs: Vec<SocketAddr> = (server, PORT).to_socket_addrs().map_err(|_| "name not found (DNS)".to_string())?.collect();
    // IPv4 first: OOS gets no IPv6 route from DHCP
    let mut v: Vec<SocketAddr> = addrs.iter().filter(|a| a.is_ipv4()).chain(addrs.iter().filter(|a| a.is_ipv6())).copied().collect();
    v.truncate(4);
    let mut err = "name without an address".to_string();
    for a in v {
        match ask(a) {
            Ok(s) => return Ok(s),
            Err(e) => err = e,
        }
    }
    Err(err)
}

/// Sets the system clock to `unix` seconds.
#[cfg(target_os = "linux")]
pub fn set_clock(unix: f64) -> Result<(), String> {
    let ts = libc::timespec { tv_sec: unix.floor() as _, tv_nsec: ((unix - unix.floor()) * 1e9) as _ };
    if unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &ts) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_clock(_: f64) -> Result<(), String> {
    Err("the clock is only set on the unit".into())
}

/// Moves the system clock by `offset` seconds (nothing below `STEP_MIN`).
pub fn step(offset: f64) -> Result<(), String> {
    if offset.abs() < STEP_MIN {
        return Ok(());
    }
    set_clock(crate::clock::unix_now() + offset)
}

/// Tells the kernel the local offset (seconds east of UTC): FAT keeps local time in its directory entries and converts
/// with it, so files on a FAT stick show the right time on a computer. exFAT and ext4 keep UTC.
#[cfg(target_os = "linux")]
pub fn set_kernel_tz(east: i32) -> Result<(), String> {
    #[repr(C)]
    struct Timezone {
        minuteswest: libc::c_int,
        dsttime: libc::c_int,
    }
    let call = |m: i32| {
        let tz = Timezone { minuteswest: m, dsttime: 0 };
        // the syscall: musl's settimeofday() ignores the zone
        unsafe { libc::syscall(libc::SYS_settimeofday, std::ptr::null::<libc::timeval>(), &tz as *const Timezone) }
    };
    // The first call of a boot with a zone and without a time also shifts the clock by the zone ("warp", for RTCs
    // in local time). A zero zone first uses that up without a shift.
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| {
        call(0);
    });
    if call(-(east / 60)) != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_kernel_tz(_: i32) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_both_ways_and_across_2036() {
        // 1970 and the first second of NTP era 1 (2036-02-07 06:28:16 UTC)
        assert_eq!(to_ntp(0.0) >> 32, NTP_UNIX as u64);
        assert_eq!(from_ntp(to_ntp(0.0)), 0.0);
        let era1 = (1i64 << 32) - NTP_UNIX;
        assert_eq!(to_ntp(era1 as f64), 0);
        assert_eq!(from_ntp(0), era1 as f64);
        for t in [1_794_398_400.25, 1_356_998_400.5, era1 as f64 + 1000.75, 4_000_000_000.0] {
            assert!((from_ntp(to_ntp(t)) - t).abs() < 1e-6, "{}", t);
        }
        // 2026-11-11 00:00:00 UTC
        assert_eq!(to_ntp(1_794_355_200.0) >> 32, 1_794_355_200 + NTP_UNIX as u64);
    }

    fn answer(t1: u64, t2: f64, t3: f64) -> [u8; 48] {
        let mut b = [0u8; 48];
        b[0] = 4 << 3 | 4;
        b[1] = 2;
        b[12..16].copy_from_slice(b"GPS\0");
        b[24..32].copy_from_slice(&t1.to_be_bytes());
        b[32..40].copy_from_slice(&to_ntp(t2).to_be_bytes());
        b[40..48].copy_from_slice(&to_ntp(t3).to_be_bytes());
        b
    }

    #[test]
    fn request_and_answer() {
        // the unit thinks it is 2013, the server says 2026
        let t1f = 1_356_998_400.0;
        let t1 = to_ntp(t1f);
        let q = request(t1);
        assert_eq!(q.len(), 48);
        assert_eq!(q[0], 0x23);
        assert!(q[1..40].iter().all(|&b| b == 0));
        assert_eq!(ts(&q, 40), t1);
        let real = 1_794_398_400.0;
        let b = answer(t1, real + 0.010, real + 0.011);
        let r = parse(&b, t1).unwrap();
        assert_eq!(r.stratum, 2);
        let (off, delay) = offset_delay(t1f, r.t2, r.t3, t1f + 0.030);
        assert!((delay - 0.029).abs() < 1e-5, "{}", delay);
        // the server's clock at t1: half the round trip before its receive time
        assert!((t1f + off - (real - 0.0045)).abs() < 1e-5, "{}", off);
    }

    #[test]
    fn answers_that_do_not_count() {
        let t1 = to_ntp(1_794_398_400.0);
        let good = answer(t1, 1_794_398_400.0, 1_794_398_400.0);
        assert!(parse(&good, t1).is_ok());
        assert!(parse(&good[..47], t1).is_err());
        assert_eq!(parse(&good, t1 + 1).unwrap_err(), "answer to another request");
        let mut b = good;
        b[0] = 4 << 3 | 3; // a request, not an answer
        assert!(parse(&b, t1).is_err());
        let mut b = good;
        b[1] = 0;
        b[12..16].copy_from_slice(b"RATE");
        assert_eq!(parse(&b, t1).unwrap_err(), "server refused (RATE)");
        let mut b = good;
        b[0] |= 3 << 6; // alarm: not synchronised
        assert_eq!(parse(&b, t1).unwrap_err(), "server not synchronised");
        let mut b = good;
        b[1] = 16;
        assert!(parse(&b, t1).is_err());
        let mut b = good;
        b[40..48].fill(0);
        assert!(parse(&b, t1).is_err());
        // version 3 servers answer too
        let mut b = good;
        b[0] = 3 << 3 | 4;
        assert!(parse(&b, t1).is_ok());
    }

    /// One exchange with a local server thread.
    #[test]
    fn query_against_a_local_server() {
        let srv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut b = [0u8; 48];
            let (_, from) = srv.recv_from(&mut b).unwrap();
            let t1 = ts(&b, 40);
            let now = crate::clock::unix_now() + 100.0;
            srv.send_to(&answer(t1, now, now), from).unwrap();
        });
        let s = ask(format!("127.0.0.1:{}", port).parse().unwrap()).unwrap();
        assert!((s.offset - 100.0).abs() < 1.0, "{:?}", s);
        assert!(s.delay >= 0.0 && s.delay < 1.0);
    }

    #[test]
    fn no_server_means_an_error() {
        // a closed port on this machine: refused or no answer, never a time
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let a = s.local_addr().unwrap();
        drop(s);
        assert!(ask(a).is_err());
        assert!(query("name.invalid").is_err());
    }
}
