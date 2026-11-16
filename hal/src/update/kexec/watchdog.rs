// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The hardware watchdog across a kexec start (docs/update.md, "The watchdog and kexec").
//!
//! dw_wdt keeps counting into the started kernel: the node has no reset line, so neither the kexec shutdown nor the
//! new kernel can stop it, and nothing pings it until the new kernel registers its driver. That gap has to fit into
//! what is left of the 21.5 s hardware period. So the updater takes the single-open device over from `oos-watchdog`
//! (request file, USR1; the supervisor kills its kicker without magic close and acknowledges) and pings it right before
//! `reboot(KEXEC)`. On any failure or return it closes the device, again without magic close, and removes the request:
//! the supervisor starts its kicker again at once.

use std::io::{self, Write};
use std::time::{Duration, Instant};

/// The supervisor's pid (written by `oos-watchdog`).
pub const PID_FILE: &str = "/tmp/oos-watchdog.pid";
/// The updater's request, with its pid; `oos-watchdog` pauses while it exists.
pub const REQUEST: &str = "/tmp/oos-watchdog.handoff";
/// The supervisor's acknowledgement: a copy of the request, written once its kicker is gone.
pub const ACK: &str = "/tmp/oos-watchdog.ack";
const DEVICE: &str = "/dev/watchdog";
const SUPERVISOR: &[u8] = b"/bin/oos-watchdog";
/// How long the supervisor may take to acknowledge and let go of the device.
const TAKEOVER: Duration = Duration::from_secs(2);

/// The take, ping and give back of a jump; a trait so the order can be tested without a device.
trait Handoff {
    fn take(&mut self) -> Result<(), String>;
    fn ping(&mut self) -> Result<(), String>;
    fn restore(&mut self);
}

/// Sync while the supervisor feeds, take the device, then `reboot` (which pings and jumps).
/// Returns only on failure, after the device is given back.
fn jump<W: Handoff>(w: &mut W, sync: impl FnOnce(), reboot: impl FnOnce(&mut W) -> String) -> String {
    // Sync may block indefinitely; keep the supervisor feeding until it finishes.
    sync();
    let r = match w.take() {
        Ok(()) => reboot(w),
        Err(e) => e,
    };
    w.restore();
    r
}

/// Ping, then the syscall: no IO between the ping and the jump.
fn reboot(w: &mut impl Handoff, syscall: impl FnOnce() -> String) -> String {
    match w.ping() {
        Ok(()) => syscall(),
        Err(e) => e,
    }
}

/// Files, processes and the clock; the tests give a fake.
trait Port {
    type Handle: Write;
    fn present(&self) -> io::Result<bool>;
    fn pid(&self) -> io::Result<String>;
    fn cmdline(&self, pid: libc::pid_t) -> io::Result<Vec<u8>>;
    fn requester(&self) -> io::Result<Option<String>>;
    fn ack(&self) -> io::Result<Option<String>>;
    fn request(&self) -> io::Result<()>;
    fn remove_request(&self) -> io::Result<()>;
    fn signal(&self, pid: libc::pid_t, signal: libc::c_int) -> io::Result<()>;
    fn open(&self) -> io::Result<Self::Handle>;
    fn now(&self) -> Instant;
    fn pause(&self);
    fn report(&self, msg: &str);
}

fn pid(text: &str) -> Result<libc::pid_t, String> {
    text.trim().parse().ok().filter(|&n| n > 1).ok_or_else(|| "watchdog: no valid pid".to_string())
}

struct Device<P: Port> {
    port: P,
    requested: bool,
    file: Option<P::Handle>,
    absent: bool,
}

impl<P: Port> Device<P> {
    fn new(port: P) -> Self {
        Self { port, requested: false, file: None, absent: false }
    }

    /// A request left by an updater that is gone (its pid no longer exists) is removed; any other is left alone.
    fn recover_stale_request(&self) -> Result<(), String> {
        let Some(req) = self.port.requester().map_err(|e| format!("{}: {}", REQUEST, e))? else { return Ok(()) };
        let who = pid(&req).map_err(|_| format!("{} without a valid pid: left in place", REQUEST))?;
        match self.port.signal(who, 0) {
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {
                self.port.remove_request().map_err(|e| format!("{}: {}", REQUEST, e))?;
                self.port.report(&format!("removed {}: its updater {} is gone", REQUEST, who));
                Ok(())
            }
            _ => Err(format!("{}: updater {} may still run; left in place", REQUEST, who)),
        }
    }

    /// Changes nothing: no request, a device, and its supervisor running. `None`: no watchdog device at all.
    fn preflight(&self) -> Result<Option<libc::pid_t>, String> {
        if self.port.requester().map_err(|e| format!("{}: {}", REQUEST, e))?.is_some() {
            return Err(format!("{} exists: a watchdog handoff is already under way", REQUEST));
        }
        if !self.port.present().map_err(|e| format!("{}: {}", DEVICE, e))? {
            return Ok(None);
        }
        let sup = pid(&self.port.pid().map_err(|e| format!("{}: {}", PID_FILE, e))?)?;
        let cmd = self.port.cmdline(sup).map_err(|e| format!("watchdog supervisor {}: {}", sup, e))?;
        if !cmd.split(|&b| b == 0).any(|a| a == SUPERVISOR) {
            return Err(format!("process {} is not oos-watchdog", sup));
        }
        Ok(Some(sup))
    }
}

impl<P: Port> Handoff for Device<P> {
    fn take(&mut self) -> Result<(), String> {
        let Some(sup) = self.preflight()? else {
            self.absent = true;
            self.port.report("no /dev/watchdog: kexec start without a watchdog ping");
            return Ok(());
        };
        self.port.request().map_err(|e| format!("{}: {}", REQUEST, e))?;
        self.requested = true;
        self.port.signal(sup, libc::SIGUSR1).map_err(|e| format!("watchdog supervisor {}: {}", sup, e))?;
        let until = self.port.now() + TAKEOVER;
        // The supervisor acknowledges this request only after its kicker is gone; it holds still until removal.
        let mine = self.port.requester().map_err(|e| format!("{}: {}", REQUEST, e))?.unwrap_or_default();
        loop {
            match self.port.ack().map_err(|e| format!("{}: {}", ACK, e))? {
                Some(a) if !mine.trim().is_empty() && a.trim() == mine.trim() => break,
                _ if self.port.now() < until => self.port.pause(),
                _ => return Err("oos-watchdog did not acknowledge the handoff in 2 s".into()),
            }
        }
        // single open: busy until the kicker's file is closed
        loop {
            match self.port.open() {
                Ok(f) => {
                    self.file = Some(f);
                    return Ok(());
                }
                Err(e) if e.raw_os_error() == Some(libc::EBUSY) && self.port.now() < until => self.port.pause(),
                Err(e) => return Err(format!("{}: {}", DEVICE, e)),
            }
        }
    }

    fn ping(&mut self) -> Result<(), String> {
        if self.absent {
            return Ok(());
        }
        // any byte but 'V' pings, and withdraws a magic close
        self.file.as_mut().ok_or("watchdog not taken over")?.write_all(&[0]).map_err(|e| format!("watchdog ping: {}", e))
    }

    fn restore(&mut self) {
        // closed without 'V': the counter runs on and the supervisor has 30 s to open it again
        drop(self.file.take());
        if self.requested {
            match self.port.remove_request() {
                Ok(()) => self.requested = false,
                Err(e) if e.kind() == io::ErrorKind::NotFound => self.requested = false,
                Err(e) => self.port.report(&format!("{}: {}; oos-watchdog stays paused, the watchdog resets the SoC", REQUEST, e)),
            }
        }
    }
}

impl<P: Port> Drop for Device<P> {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(target_os = "linux")]
mod native {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::time::{Duration, Instant};

    pub struct Native;

    impl super::Port for Native {
        type Handle = File;

        fn present(&self) -> io::Result<bool> {
            std::path::Path::new(super::DEVICE).try_exists()
        }

        fn pid(&self) -> io::Result<String> {
            std::fs::read_to_string(super::PID_FILE)
        }

        fn cmdline(&self, pid: libc::pid_t) -> io::Result<Vec<u8>> {
            std::fs::read(format!("/proc/{}/cmdline", pid))
        }

        fn requester(&self) -> io::Result<Option<String>> {
            match std::fs::read_to_string(super::REQUEST) {
                Ok(p) => Ok(Some(p)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        fn ack(&self) -> io::Result<Option<String>> {
            match std::fs::read_to_string(super::ACK) {
                Ok(a) => Ok(Some(a)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        fn request(&self) -> io::Result<()> {
            let mut f = OpenOptions::new().write(true).create_new(true).open(super::REQUEST)?;
            if let Err(e) = writeln!(f, "{}", std::process::id()) {
                // a request without the pid could never be recovered
                let _ = std::fs::remove_file(super::REQUEST);
                return Err(e);
            }
            Ok(())
        }

        fn remove_request(&self) -> io::Result<()> {
            std::fs::remove_file(super::REQUEST)
        }

        fn signal(&self, pid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
            // pid > 1 (checked by `pid`); signal is USR1 or 0 (only a check that the process exists)
            if unsafe { libc::kill(pid, signal) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }

        fn open(&self) -> io::Result<File> {
            OpenOptions::new().write(true).open(super::DEVICE)
        }

        fn now(&self) -> Instant {
            Instant::now()
        }

        fn pause(&self) {
            std::thread::sleep(Duration::from_millis(10));
        }

        fn report(&self, msg: &str) {
            let line = format!("oos-update: {}\n", msg);
            // stderr may be gone (a closed terminal): never panic on it
            let _ = io::stderr().write_all(line.as_bytes());
            let _ = OpenOptions::new().write(true).open("/dev/kmsg").and_then(|mut f| f.write_all(line.as_bytes()));
        }
    }
}

/// Before anything is stopped for a kexec start, under the update lock: a request of a dead updater is removed, then
/// the handoff must be possible (or there is no watchdog device).
#[cfg(target_os = "linux")]
pub fn preflight() -> Result<(), String> {
    let d = Device::new(native::Native);
    d.recover_stale_request()?;
    d.preflight().map(|_| ())
}

/// Sync with the supervisor feeding, take the watchdog over, ping, `reboot(KEXEC)`.
/// Returns only on failure, with the watchdog given back.
#[cfg(target_os = "linux")]
pub fn start() -> String {
    jump(
        &mut Device::new(native::Native),
        || unsafe { libc::sync() },
        |w| {
            reboot(w, || {
                let r = unsafe { libc::reboot(libc::LINUX_REBOOT_CMD_KEXEC) };
                format!("reboot(KEXEC) returned {}: {}", r, io::Error::last_os_error())
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Order<'a> {
        calls: &'a RefCell<Vec<&'static str>>,
        fail: &'static str,
    }

    impl Handoff for Order<'_> {
        fn take(&mut self) -> Result<(), String> {
            self.calls.borrow_mut().push("take");
            if self.fail == "take" {
                Err("take failed".into())
            } else {
                Ok(())
            }
        }
        fn ping(&mut self) -> Result<(), String> {
            self.calls.borrow_mut().push("ping");
            if self.fail == "ping" {
                Err("ping failed".into())
            } else {
                Ok(())
            }
        }
        fn restore(&mut self) {
            self.calls.borrow_mut().push("restore");
        }
    }

    #[test]
    fn ping_right_before_the_jump_and_give_back_on_return() {
        let calls = RefCell::new(vec![]);
        let mut w = Order { calls: &calls, fail: "" };
        let r = jump(
            &mut w,
            || calls.borrow_mut().push("sync"),
            |w| {
                reboot(w, || {
                    calls.borrow_mut().push("reboot");
                    "returned".into()
                })
            },
        );
        assert_eq!(*calls.borrow(), ["sync", "take", "ping", "reboot", "restore"]);
        assert_eq!(r, "returned");
    }

    #[test]
    fn no_jump_without_take_and_ping() {
        for fail in ["take", "ping"] {
            let calls = RefCell::new(vec![]);
            let mut w = Order { calls: &calls, fail };
            let r = jump(&mut w, || calls.borrow_mut().push("sync"), |w| reboot(w, || panic!("must not jump")));
            assert_eq!(r, format!("{} failed", fail));
            assert_eq!(calls.borrow().last(), Some(&"restore"));
        }
    }

    struct State {
        present: bool,
        pid: Option<String>,
        cmdline: Option<Vec<u8>>,
        request: Option<String>,
        ack: Ack,
        signal_error: Option<i32>,
        alive_error: Option<i32>,
        busy: usize,
        open_error: Option<i32>,
        remove_error: Option<i32>,
        calls: Vec<String>,
        elapsed: Duration,
    }

    /// The fake supervisor's acknowledgement: after n polls, one of another request, none.
    #[derive(Clone, Copy)]
    enum Ack {
        After(usize),
        Other,
        Never,
    }

    struct Fake {
        s: Rc<RefCell<State>>,
        start: Instant,
    }

    struct Handle(Rc<RefCell<State>>);

    impl Write for Handle {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            assert_eq!(b, &[0], "never a magic close");
            self.0.borrow_mut().calls.push("ping".into());
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            self.0.borrow_mut().calls.push("close".into());
        }
    }

    fn res(e: Option<i32>) -> io::Result<()> {
        e.map_or(Ok(()), |n| Err(io::Error::from_raw_os_error(n)))
    }

    impl Port for Fake {
        type Handle = Handle;
        fn present(&self) -> io::Result<bool> {
            Ok(self.s.borrow().present)
        }
        fn pid(&self) -> io::Result<String> {
            self.s.borrow().pid.clone().ok_or_else(|| io::ErrorKind::NotFound.into())
        }
        fn cmdline(&self, pid: libc::pid_t) -> io::Result<Vec<u8>> {
            assert_eq!(pid, 42);
            self.s.borrow().cmdline.clone().ok_or_else(|| io::ErrorKind::NotFound.into())
        }
        fn requester(&self) -> io::Result<Option<String>> {
            Ok(self.s.borrow().request.clone())
        }
        fn ack(&self) -> io::Result<Option<String>> {
            let mut s = self.s.borrow_mut();
            match s.ack {
                Ack::After(0) => {
                    let r = s.request.clone();
                    s.calls.push("ack".into());
                    Ok(r)
                }
                Ack::After(n) => {
                    s.ack = Ack::After(n - 1);
                    Ok(None)
                }
                Ack::Other => Ok(Some("123\n".into())),
                Ack::Never => Ok(None),
            }
        }
        fn request(&self) -> io::Result<()> {
            let mut s = self.s.borrow_mut();
            if s.request.is_some() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            s.calls.push("request".into());
            s.request = Some("99\n".into());
            Ok(())
        }
        fn remove_request(&self) -> io::Result<()> {
            let mut s = self.s.borrow_mut();
            s.calls.push("remove".into());
            res(s.remove_error)?;
            s.request = None;
            Ok(())
        }
        fn signal(&self, pid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
            let mut s = self.s.borrow_mut();
            if signal == 0 {
                assert_eq!(pid, 99);
                s.calls.push("probe".into());
                res(s.alive_error)
            } else {
                assert_eq!((pid, signal), (42, libc::SIGUSR1));
                s.calls.push("signal".into());
                res(s.signal_error)
            }
        }
        fn open(&self) -> io::Result<Handle> {
            let mut s = self.s.borrow_mut();
            s.calls.push("open".into());
            if s.busy > 0 {
                s.busy -= 1;
                return Err(io::Error::from_raw_os_error(libc::EBUSY));
            }
            res(s.open_error)?;
            Ok(Handle(self.s.clone()))
        }
        fn now(&self) -> Instant {
            self.start + self.s.borrow().elapsed
        }
        fn pause(&self) {
            self.s.borrow_mut().elapsed += Duration::from_millis(10);
        }
        fn report(&self, msg: &str) {
            self.s.borrow_mut().calls.push(format!("report: {}", msg));
        }
    }

    fn device() -> (Device<Fake>, Rc<RefCell<State>>) {
        let s = Rc::new(RefCell::new(State {
            present: true,
            pid: Some("42\n".into()),
            cmdline: Some(b"/bin/sh\0/bin/oos-watchdog\0".to_vec()),
            request: None,
            ack: Ack::After(0),
            signal_error: None,
            alive_error: None,
            busy: 0,
            open_error: None,
            remove_error: None,
            calls: vec![],
            elapsed: Duration::ZERO,
        }));
        (Device::new(Fake { s: s.clone(), start: Instant::now() }), s)
    }

    #[test]
    fn preflight_checks_the_supervisor_and_changes_nothing() {
        let (d, s) = device();
        assert_eq!(d.preflight(), Ok(Some(42)));
        for v in [None, Some(""), Some("abc"), Some("-1"), Some("0"), Some("1"), Some("2147483648")] {
            s.borrow_mut().pid = v.map(str::to_owned);
            assert!(d.preflight().unwrap_err().contains("pid"), "{:?}", v);
        }
        s.borrow_mut().pid = Some("42".into());
        for cmd in [None, Some(b"oos-watchdog".to_vec()), Some(b"/bin/oos-watchdog-old\0".to_vec())] {
            s.borrow_mut().cmdline = cmd;
            assert!(d.preflight().is_err());
        }
        assert!(s.borrow().calls.is_empty());
    }

    #[test]
    fn without_a_device_no_ping_but_a_log_line() {
        let (mut d, s) = device();
        s.borrow_mut().present = false;
        s.borrow_mut().pid = None;
        assert_eq!(d.preflight(), Ok(None));
        d.take().unwrap();
        d.ping().unwrap();
        drop(d);
        assert_eq!(s.borrow().calls.len(), 1);
        assert!(s.borrow().calls[0].contains("without a watchdog ping"));
    }

    #[test]
    fn a_left_request_is_removed_only_when_its_updater_is_gone() {
        for alive in [None, Some(libc::EPERM), Some(libc::ESRCH)] {
            let (d, s) = device();
            s.borrow_mut().request = Some("99\n".into());
            s.borrow_mut().alive_error = alive;
            assert!(d.preflight().unwrap_err().contains(REQUEST));
            assert!(s.borrow().calls.is_empty(), "preflight changes nothing");
            let r = d.recover_stale_request();
            assert_eq!(r.is_ok(), alive == Some(libc::ESRCH));
            assert_eq!(s.borrow().request.is_none(), r.is_ok());
        }
        for who in ["", "junk", "-1", "1"] {
            let (d, s) = device();
            s.borrow_mut().request = Some(who.into());
            assert!(d.recover_stale_request().unwrap_err().contains("left in place"));
            assert!(s.borrow().calls.is_empty());
            assert!(s.borrow().request.is_some());
        }
    }

    #[test]
    fn a_busy_device_is_tried_again_for_two_seconds() {
        for busy in [3, 500] {
            let (mut d, s) = device();
            s.borrow_mut().busy = busy;
            assert_eq!(d.take().is_ok(), busy == 3);
            drop(d);
            let s = s.borrow();
            assert_eq!(s.calls.iter().filter(|&c| c == "open").count(), if busy == 3 { 4 } else { 201 });
            assert_eq!(s.elapsed, Duration::from_millis(if busy == 3 { 30 } else { 2000 }));
            assert!(s.request.is_none());
        }
    }

    #[test]
    fn a_failed_signal_or_open_removes_the_request() {
        for signal in [true, false] {
            let (mut d, s) = device();
            if signal {
                s.borrow_mut().signal_error = Some(libc::ESRCH);
            } else {
                s.borrow_mut().open_error = Some(libc::EIO);
            }
            let r = jump(&mut d, || {}, |_| panic!("must not jump"));
            assert!(r.contains(if signal { "supervisor" } else { "/dev/watchdog" }), "{}", r);
            drop(d);
            let s = s.borrow();
            assert!(s.request.is_none());
            assert_eq!(s.calls.iter().filter(|&c| c == "remove").count(), 1);
        }
    }

    #[test]
    fn closed_before_the_supervisor_resumes_and_only_once() {
        let (mut d, s) = device();
        d.take().unwrap();
        d.ping().unwrap();
        d.restore();
        d.restore();
        drop(d);
        assert_eq!(s.borrow().calls, ["request", "signal", "ack", "open", "ping", "close", "remove"]);
    }

    #[test]
    fn the_device_is_opened_only_after_the_acknowledgement() {
        // late (the supervisor ends its backoff sleep first), still in time
        let (mut d, s) = device();
        s.borrow_mut().ack = Ack::After(50);
        d.take().unwrap();
        {
            let s = s.borrow();
            let at = |what: &str| s.calls.iter().position(|c| c == what).unwrap();
            assert!(at("ack") < at("open"));
            assert_eq!(s.elapsed, Duration::from_millis(500));
        }
        drop(d);
        // none, or one of another request: no open, the request removed, the supervisor resumes
        for ack in [Ack::Never, Ack::Other] {
            let (mut d, s) = device();
            s.borrow_mut().ack = ack;
            let r = jump(&mut d, || {}, |_| panic!("must not jump"));
            assert!(r.contains("did not acknowledge"), "{}", r);
            drop(d);
            let s = s.borrow();
            assert!(!s.calls.iter().any(|c| c == "open"));
            assert_eq!(s.elapsed, TAKEOVER);
            assert!(s.request.is_none());
        }
    }

    #[test]
    fn a_failed_removal_is_reported_and_tried_again() {
        let (mut d, s) = device();
        d.take().unwrap();
        s.borrow_mut().remove_error = Some(libc::EIO);
        d.restore();
        assert!(s.borrow().calls.last().unwrap().contains("stays paused"));
        s.borrow_mut().remove_error = None;
        drop(d);
        assert!(s.borrow().request.is_none());
        assert_eq!(s.borrow().calls.iter().filter(|&c| c == "close").count(), 1);
        assert_eq!(s.borrow().calls.last().unwrap(), "remove");
    }
}
