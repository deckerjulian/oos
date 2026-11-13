// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Date and time: the clock from an NTP server (off by default: OOS sends nothing to the outside unless asked),
//! the server (by default the one the DHCP lease names, option 42), the time zone, the clock by hand; and System >
//! Network > 10 Mbit retry. The settings live in
//! `/oos/system.conf` (sysconf.rs). A worker thread asks the server (sntp.rs), so the frame loop never waits on the
//! network. Once the clock is set, file names carry the local date and time (`Ui::new_file`).

use super::*;
use crate::clock::{self, Local, Tz, ZONES};
use crate::sntp;
use crate::sysconf::{self, NtpOrigin};
use std::sync::mpsc::{channel, Receiver, Sender};

/// A sync again after this long (the clock of the RK3399 drifts a few seconds a day).
pub const RESYNC: f64 = 3600.0;
/// After a link came up: a moment for DHCP to set the route and the name server.
const NET_SETTLE: f64 = 3.0;
/// The first retry after a failure; each further one waits twice as long, up to `RETRY_MAX`.
const RETRY_FIRST: f64 = 15.0;
const RETRY_MAX: f64 = 900.0;
/// Left in /tmp (RAM) once the clock was set in this boot: a restarted oos-ui knows the clock is right.
const CLOCK_SET: &str = "/tmp/oos-clock.set";
/// Servers offered in the picker (`dhcp` shows as "DHCP"); another one through "Other".
pub const SERVERS: [&str; 6] = [sysconf::DHCP, "pool.ntp.org", "time.google.com", "time.cloudflare.com", "time.nist.gov", "ptbtime1.ptb.de"];
const DHCP_LABEL: &str = "DHCP";
const OTHER: &str = "Other ...";
/// Characters of a server or zone string shown in a row.
const SHOW_MAX: usize = 18;

/// Characters of a failure reason on the page.
pub const ERR_MAX: usize = 26;

/// A failure reason short enough for the state line: without the errno, cut to `ERR_MAX`.
pub fn short_err(e: &str) -> String {
    let e = e.split(" (os error").next().unwrap_or(e).trim();
    if e.chars().count() <= ERR_MAX {
        return e.to_string();
    }
    format!("{}\u{2026}", e.chars().take(ERR_MAX - 1).collect::<String>())
}

/// Wait before retry number `fails` (1 = after the first failure).
pub fn retry_wait(fails: u32) -> f64 {
    (RETRY_FIRST * 2f64.powi(fails.saturating_sub(1).min(16) as i32)).min(RETRY_MAX)
}

/// A sync that worked.
#[derive(Clone, Debug, PartialEq)]
pub struct Synced {
    /// Unix time after the sync
    pub at: f64,
    pub server: String,
    pub offset: f64,
    pub stratum: u8,
}

/// When to ask the server: on demand, after a link came up, after a failure with a growing wait, every hour after a
/// success. `now` is the frame loop's monotonic time (`Ui::now`).
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub busy: bool,
    pub due: Option<f64>,
    pub fails: u32,
    pub last_ok: Option<Synced>,
    /// Unix time and reason of the last failure (cleared by a success)
    pub last_err: Option<(f64, String)>,
    net_was_up: bool,
}

impl Schedule {
    /// A sync at once (switched on, other server, Sync now).
    pub fn kick(&mut self, now: f64) {
        self.due = Some(now);
        self.fails = 0;
    }

    /// True when a request is to go out now.
    pub fn tick(&mut self, now: f64, on: bool, net_up: bool) -> bool {
        let came_up = net_up && !self.net_was_up;
        self.net_was_up = net_up;
        if !on {
            self.due = None;
            return false;
        }
        if came_up {
            let t = now + NET_SETTLE;
            self.due = Some(self.due.map_or(t, |d| d.min(t)));
        }
        if self.busy || !net_up {
            return false;
        }
        let due = *self.due.get_or_insert(now);
        if now < due {
            return false;
        }
        self.busy = true;
        self.due = None;
        true
    }

    pub fn done(&mut self, now: f64, r: Result<Synced, String>) {
        self.busy = false;
        match r {
            Ok(s) => {
                self.fails = 0;
                self.last_ok = Some(s);
                self.last_err = None;
                self.due = Some(now + RESYNC);
            }
            Err(e) => {
                self.fails += 1;
                self.last_err = Some((clock::unix_now(), short_err(&e)));
                self.due = Some(now + retry_wait(self.fails));
            }
        }
    }
}

#[derive(Default)]
pub(super) struct NtpUi {
    pub st: Schedule,
    tx: Option<Sender<String>>,
    rx: Option<Receiver<Result<Synced, String>>>,
    /// the offset last given to the kernel (FAT time stamps), and when it was checked
    kernel_tz: Option<i32>,
    tz_checked: f64,
    /// the server of the last request and where it came from (NTP server = DHCP: the lease or the fallback)
    asked: Option<(String, NtpOrigin)>,
    /// tests: the directory of the lease files (`oos-dhcp-ntp.<interface>`) instead of /tmp
    lease_dir: Option<std::path::PathBuf>,
}

/// One sync: ask, then set the clock.
fn sync(server: &str) -> Result<Synced, String> {
    let s = sntp::query(server)?;
    sntp::step(s.offset).map_err(|e| format!("clock not set: {}", e))?;
    eprintln!("oos-ui: NTP {} ({}, stratum {}): offset {:+.3} s, delay {:.3} s", server, s.addr, s.stratum, s.offset, s.delay);
    Ok(Synced { at: clock::unix_now(), server: server.into(), offset: s.offset, stratum: s.stratum })
}

fn worker(jobs: Receiver<String>, out: Sender<Result<Synced, String>>) {
    while let Ok(server) = jobs.recv() {
        let r = sync(&server);
        if let Err(e) = &r {
            eprintln!("oos-ui: NTP {}: {}", server, e);
        }
        if out.send(r).is_err() {
            return;
        }
    }
}

/// A server as the page shows it.
fn server_label(s: &str) -> String {
    if s == sysconf::DHCP {
        DHCP_LABEL.into()
    } else {
        short(s)
    }
}

fn short(s: &str) -> String {
    if s.chars().count() <= SHOW_MAX {
        return s.to_string();
    }
    format!("{}\u{2026}", s.chars().take(SHOW_MAX - 1).collect::<String>())
}

impl Ui {
    // ------------------------------------------------------------------------------------------------ settings

    /// At the start on the unit: the settings file, the time zone for the kernel, whether the clock was set already.
    pub fn load_system(&mut self, path: std::path::PathBuf) {
        self.sys = sysconf::SysConf::load(&path);
        self.sys_path = Some(path);
        self.clock_known = std::path::Path::new(CLOCK_SET).exists();
        self.apply_tz();
        self.apply_backlight();
    }

    /// Writes the settings (not in tests: there `sys_path` is none).
    pub(super) fn sys_save(&mut self) {
        let Some(p) = self.sys_path.clone() else { return };
        if let Err(e) = self.sys.save(&p) {
            self.toast(&format!("Not saved: {}", e));
        }
    }

    /// The time zone of the settings.
    pub fn zone(&self) -> Tz {
        self.sys.zone()
    }

    /// Local time now.
    pub fn local_now(&self) -> Local {
        self.zone().local(clock::unix_now().floor() as i64)
    }

    /// The local offset for the kernel (FAT time stamps on a stick), when it changed.
    fn apply_tz(&mut self) {
        let off = self.zone().offset_at(clock::unix_now() as i64).0;
        if self.ntp.kernel_tz == Some(off) || cfg!(test) {
            return;
        }
        // a failure is logged once per offset (not as root on a host: EPERM)
        if let Err(e) = sntp::set_kernel_tz(off) {
            eprintln!("oos-ui: time zone for the kernel: {}", e);
        }
        self.ntp.kernel_tz = Some(off);
    }

    /// The clock is right (set by NTP or by hand in this boot).
    fn clock_set(&mut self) {
        self.clock_known = true;
        if !cfg!(test) {
            let _ = std::fs::write(CLOCK_SET, b"");
        }
    }

    /// `YYYYMMDD-HHMMSS` in the time zone of the settings, once the clock is right: file names on the scope and of the
    /// web page's downloads (`/stamp`).
    pub fn file_stamp(&self) -> Option<String> {
        self.clock_known.then(|| self.local_now().stamp())
    }

    /// A new file in the storage directory: `<prefix>-YYYYMMDD-HHMMSS.<ext>` in local time once the clock is right,
    /// else the first free `<prefix>-NNN.<ext>`.
    pub(super) fn new_file(&self, dir: &std::path::Path, prefix: &str, ext: &str) -> std::path::PathBuf {
        crate::store::next_name_at(dir, prefix, ext, self.file_stamp().as_deref())
    }

    // ------------------------------------------------------------------------------------------------ NTP

    /// The text of an interface's lease file (`sysconf::DHCP_NTP_FILE`), None without one.
    fn lease(&self, iface: &str) -> Option<String> {
        match &self.ntp.lease_dir {
            Some(d) => std::fs::read_to_string(d.join(format!("oos-dhcp-ntp.{}", iface))).ok(),
            None if cfg!(test) => None,
            None => std::fs::read_to_string(format!("{}{}", sysconf::DHCP_NTP_FILE, iface)).ok(),
        }
    }

    /// The server to ask now: `dhcp` is looked up at each request, the lease may have changed.
    fn ntp_target(&self) -> (String, NtpOrigin) {
        sysconf::ntp_server(&self.sys.ntp_server, |i| self.lease(i))
    }

    fn net_up(&self) -> bool {
        (self.eth_link && self.addr_of("eth0").is_some()) || self.addr_of(oos_hal::wlan::IFACE).is_some()
    }

    /// From `step`: requests when due, answers of the worker, the kernel's time zone once a minute (daylight saving).
    pub(super) fn ntp_poll(&mut self) {
        if self.now - self.ntp.tz_checked >= 60.0 {
            self.ntp.tz_checked = self.now;
            self.apply_tz();
        }
        let up = self.net_up();
        if self.ntp.st.tick(self.now, self.sys.ntp, up) {
            if self.ntp.tx.is_none() && !cfg!(test) {
                let (tx, jobs) = channel();
                let (out, rx) = channel();
                if std::thread::Builder::new().name("ntp".into()).spawn(move || worker(jobs, out)).is_ok() {
                    self.ntp.tx = Some(tx);
                    self.ntp.rx = Some(rx);
                }
            }
            let (server, origin) = self.ntp_target();
            self.ntp.asked = Some((server.clone(), origin));
            if !self.ntp.tx.as_ref().is_some_and(|tx| tx.send(server).is_ok()) && !cfg!(test) {
                self.ntp.st.done(self.now, Err("no worker".into()));
            }
        }
        while let Some(r) = self.ntp.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.ntp_report(r);
        }
    }

    pub(super) fn ntp_report(&mut self, r: Result<Synced, String>) {
        let ok = r.is_ok();
        self.ntp.st.done(self.now, r);
        if ok {
            self.clock_set();
            self.apply_tz();
        }
    }

    fn ntp_sync_now(&mut self) {
        if !self.sys.ntp {
            return self.toast("Switch Time from NTP on first");
        }
        if !self.net_up() {
            return self.toast("No network link");
        }
        self.ntp.st.kick(self.now);
        self.toast(&format!("Asking {} ...", short(&self.ntp_target().0)));
    }

    /// With NTP server = DHCP: the server of the last request and where it came from, `192.168.1.1 (DHCP)` or
    /// `pool.ntp.org (no DHCP)`.
    fn dhcp_tag(&self) -> Option<String> {
        if self.sys.ntp_server != sysconf::DHCP {
            return None;
        }
        let (s, o) = self.ntp.asked.as_ref()?;
        Some(match o {
            NtpOrigin::Dhcp(_) => format!("{} (DHCP)", short(s)),
            _ => format!("{} (no DHCP)", short(s)),
        })
    }

    /// Tests: as if the last request had gone to `server`.
    #[cfg(test)]
    pub(super) fn ntp_dhcp_asked_for_tests(&mut self, server: &str, origin: NtpOrigin) {
        self.ntp.asked = Some((server.into(), origin));
    }

    /// The state line of the page.
    pub(super) fn ntp_status(&self) -> String {
        let st = &self.ntp.st;
        let at = |unix: f64| self.zone().local(unix as i64).hms();
        let hm = |unix: f64| self.zone().local(unix as i64).hms()[..5].to_string();
        if !self.sys.ntp {
            return if self.clock_known { "Off, clock set by hand".into() } else { "Off".into() };
        }
        // DHCP: which server it was, in place of the offset and the times
        if let Some(tag) = self.dhcp_tag() {
            match (st.busy, &st.last_ok, &st.last_err) {
                (true, _, _) => return format!("Asking {} ...", tag),
                (_, _, Some((_, e))) => return format!("{}, {}", e, tag),
                (_, Some(ok), None) => return format!("Synced {}, {}", at(ok.at), tag),
                _ => {}
            }
        }
        if st.busy {
            return "Asking the server ...".into();
        }
        match (&st.last_ok, &st.last_err) {
            (_, Some((t, e))) => {
                match &st.last_ok {
                    Some(ok) => format!("{} {}, synced {}", e, hm(*t), hm(ok.at)),
                    None => format!("{} at {}", e, at(*t)),
                }
            }
            (Some(ok), None) if ok.offset.abs() >= 60.0 => format!("Synced at {}, clock set", at(ok.at)),
            (Some(ok), None) => format!("Synced at {} ({:+.2} s)", at(ok.at), ok.offset),
            (None, None) if !self.net_up() => "Waiting for a network link".into(),
            (None, None) => "Starting".into(),
        }
    }

    // ------------------------------------------------------------------------------------------------ the page

    pub(super) fn time_items(&self) -> Vec<Item> {
        vec![Item::TimeNow, Item::NtpOn, Item::NtpServer, Item::NtpStatus, Item::NtpSync, Item::TimeZone, Item::TimeSet]
    }

    pub(super) fn time_kind(it: Item) -> Option<Kind> {
        match it {
            Item::NtpSync | Item::TimeSet => Some(Kind::Action),
            Item::TimeNow | Item::NtpStatus | Item::NtpOn | Item::NtpServer | Item::TimeZone | Item::NetRetry10 => Some(Kind::Choice),
            _ => None,
        }
    }

    /// Rows that are only text.
    pub(super) fn time_text(it: Item) -> bool {
        matches!(it, Item::TimeNow | Item::NtpStatus)
    }

    pub(super) fn time_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Time) => "Date and time",
            Item::TimeNow => "Now",
            Item::NtpOn => "Time from NTP",
            Item::NtpServer => "NTP server",
            Item::NtpStatus => "NTP state",
            Item::NtpSync => "Sync now",
            Item::TimeZone => "Time zone",
            Item::TimeSet => "Set the clock",
            Item::NetRetry10 => "10 Mbit retry",
            _ => return None,
        })
    }

    pub(super) fn time_value(&self, it: Item) -> Option<String> {
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::TimeNow => {
                let t = clock::unix_now() as i64;
                let z = self.zone();
                let s = format!("{} {}", z.local(t).text(), z.abbr_at(t));
                if self.clock_known { s } else { format!("{} (not set)", s) }
            }
            Item::NtpOn => onoff(self.sys.ntp),
            Item::NtpServer => server_label(&self.sys.ntp_server),
            Item::NtpStatus => self.ntp_status(),
            Item::TimeZone => match clock::zone_name(&self.sys.tz) {
                Some(n) => n.to_string(),
                None => short(&self.sys.tz),
            },
            Item::NetRetry10 => onoff(self.sys.eth_retry_10m),
            _ => return None,
        })
    }

    fn server_options(&self) -> Vec<String> {
        let mut v: Vec<String> = SERVERS.iter().map(|s| server_label(s)).collect();
        if !SERVERS.contains(&self.sys.ntp_server.as_str()) {
            v.push(short(&self.sys.ntp_server));
        }
        v.push(OTHER.into());
        v
    }

    fn zone_options(&self) -> Vec<String> {
        let mut v: Vec<String> = ZONES.iter().map(|z| z.0.to_string()).collect();
        if clock::zone_name(&self.sys.tz).is_none() {
            v.push(short(&self.sys.tz));
        }
        v.push(OTHER.into());
        v
    }

    pub(super) fn time_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::NtpOn | Item::NetRetry10 => vec!["Off".into(), "On".into()],
            Item::NtpServer => self.server_options(),
            Item::TimeZone => self.zone_options(),
            Item::TimeNow | Item::NtpStatus => Vec::new(),
            _ => return None,
        })
    }

    fn set_server(&mut self, s: &str) {
        if self.sys.ntp_server != s {
            self.sys.ntp_server = s.to_string();
            self.sys_save();
            self.ntp.st.last_ok = None;
            self.ntp.st.last_err = None;
            self.ntp.asked = None;
            self.ntp.st.kick(self.now);
        }
    }

    fn set_tz(&mut self, tz: &str) {
        if self.sys.tz != tz {
            self.sys.tz = tz.to_string();
            self.sys_save();
            self.apply_tz();
        }
    }

    /// Knob and +/-: through the listed servers and zones (Other opens the keyboard, from the picker).
    pub(super) fn time_adjust(&mut self, it: Item, d: i32) -> bool {
        let cyc = |i: usize, n: usize| (i as i32 + d).rem_euclid(n as i32) as usize;
        match it {
            Item::NtpOn => {
                self.sys.ntp = !self.sys.ntp;
                self.sys_save();
                if self.sys.ntp {
                    self.ntp.st.kick(self.now);
                }
            }
            Item::NetRetry10 => {
                self.sys.eth_retry_10m = !self.sys.eth_retry_10m;
                self.sys_save();
            }
            Item::NtpServer => {
                let i = SERVERS.iter().position(|&s| s == self.sys.ntp_server).unwrap_or(0);
                self.set_server(SERVERS[cyc(i, SERVERS.len())]);
            }
            Item::TimeZone => {
                let i = ZONES.iter().position(|z| z.1 == self.sys.tz).unwrap_or(0);
                self.set_tz(ZONES[cyc(i, ZONES.len())].1);
            }
            Item::TimeNow | Item::NtpStatus => {}
            _ => return false,
        }
        true
    }

    pub(super) fn time_activate(&mut self, it: Item) -> bool {
        match it {
            Item::NtpSync => self.ntp_sync_now(),
            Item::TimeSet => self.wlan.osk = Some(wlan::Osk::with_text(wlan::OskFor::DateTime, &self.local_now().text_min())),
            _ => return false,
        }
        true
    }

    /// A line of the server or zone picker.
    pub(super) fn time_pick(&mut self, it: Item, k: usize) {
        let opts = self.options(it);
        let Some(o) = opts.get(k) else { return };
        match it {
            Item::NtpServer if o == OTHER => {
                let now = if self.sys.ntp_server == sysconf::DHCP { "" } else { self.sys.ntp_server.as_str() };
                self.wlan.osk = Some(wlan::Osk::with_text(wlan::OskFor::NtpServer, now));
            }
            Item::TimeZone if o == OTHER => self.wlan.osk = Some(wlan::Osk::with_text(wlan::OskFor::Tz, &self.sys.tz)),
            Item::NtpServer => {
                if let Some(s) = SERVERS.iter().find(|s| server_label(s) == *o) {
                    self.set_server(s);
                }
            }
            Item::TimeZone => {
                if let Some(z) = ZONES.iter().find(|z| z.0 == o) {
                    self.set_tz(z.1);
                }
            }
            _ => {}
        }
    }

    /// OK on the keyboard of a server, a zone or the time. False: the keyboard stays open to correct the entry.
    pub(super) fn time_osk_done(&mut self, what: &wlan::OskFor, text: &str) -> bool {
        let t = text.trim();
        match what {
            wlan::OskFor::NtpServer => {
                if !sysconf::server_ok(t) {
                    self.toast("Not a server name or address");
                    return false;
                }
                self.set_server(t);
            }
            wlan::OskFor::Tz => match Tz::parse(t) {
                Ok(_) => self.set_tz(t),
                Err(e) => {
                    self.toast(&format!("Time zone: {}", e));
                    return false;
                }
            },
            wlan::OskFor::DateTime => {
                let l = match Local::parse(t) {
                    Ok(l) => l,
                    Err(e) => {
                        self.toast(&e);
                        return false;
                    }
                };
                let unix = self.zone().to_unix(&l);
                if !cfg!(test) {
                    if let Err(e) = sntp::set_clock(unix as f64) {
                        self.toast(&format!("Clock not set: {}", e));
                        return true;
                    }
                    eprintln!("oos-ui: clock set by hand to {} ({})", l.text(), self.sys.tz);
                }
                self.clock_set();
                self.apply_tz();
                self.toast(&format!("Clock set to {}", l.text()));
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    #[test]
    fn failure_reasons_stay_short() {
        assert_eq!(short_err("Network is unreachable (os error 101)"), "Network is unreachable");
        assert_eq!(short_err("no answer"), "no answer");
        assert_eq!(short_err(&"x".repeat(40)).chars().count(), ERR_MAX);
    }

    #[test]
    fn retries_wait_longer_up_to_a_limit() {
        assert_eq!(retry_wait(1), 15.0);
        assert_eq!(retry_wait(2), 30.0);
        assert_eq!(retry_wait(4), 120.0);
        assert_eq!(retry_wait(7), 900.0);
        assert_eq!(retry_wait(1000), 900.0);
    }

    fn ok(at: f64, offset: f64) -> Result<Synced, String> {
        Ok(Synced { at, server: "pool.ntp.org".into(), offset, stratum: 2 })
    }

    #[test]
    fn schedule_waits_for_a_link_retries_and_syncs_hourly() {
        let mut s = Schedule::default();
        // off: nothing, also with a link
        assert!(!s.tick(0.0, false, true));
        // on without a link: nothing; the link comes up: after a moment for DHCP
        assert!(!s.tick(1.0, true, false));
        assert!(!s.tick(2.0, true, true));
        assert!(!s.tick(4.9, true, true));
        assert!(s.tick(5.0, true, true));
        // one request at a time
        assert!(!s.tick(6.0, true, true));
        s.done(10.0, Err("no answer".into()));
        assert!(!s.tick(24.9, true, true));
        assert!(s.tick(25.0, true, true));
        s.done(28.0, Err("no answer".into()));
        assert!(!s.tick(57.9, true, true));
        assert!(s.tick(58.0, true, true));
        s.done(60.0, ok(1e9, 0.01));
        assert_eq!(s.fails, 0);
        assert!(s.last_err.is_none());
        assert!(!s.tick(60.0 + RESYNC - 1.0, true, true));
        assert!(s.tick(60.0 + RESYNC, true, true));
        s.done(4000.0, ok(1e9, 0.01));
        // a link that went and came back asks soon, not in an hour
        assert!(!s.tick(4001.0, true, false));
        assert!(!s.tick(4002.0, true, true));
        assert!(s.tick(4002.0 + NET_SETTLE, true, true));
    }

    #[test]
    fn switched_on_with_a_link_it_asks_at_once() {
        let mut s = Schedule::default();
        s.tick(0.0, false, true);
        s.kick(1.0);
        assert!(s.tick(1.0, true, true));
    }

    fn ui() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.net_ifs = vec![("eth0".into(), "192.168.1.20".into())];
        u.eth_link = true;
        u
    }

    #[test]
    fn page_shows_the_state_and_takes_the_settings() {
        let mut u = ui();
        assert!(!u.sys.ntp, "off by default");
        assert_eq!(u.value(Item::NtpServer), "DHCP");
        u.select_option(Item::NtpServer, "pool.ntp.org");
        assert_eq!(u.value(Item::NtpStatus), "Off");
        assert!(u.value(Item::TimeNow).ends_with("UTC (not set)"), "{}", u.value(Item::TimeNow));
        u.select_option(Item::NtpOn, "On");
        assert!(u.sys.ntp);
        u.step(0.04);
        assert!(u.ntp.st.busy);
        assert_eq!(u.value(Item::NtpStatus), "Asking the server ...");
        u.ntp_report(Err("no answer".into()));
        assert!(u.value(Item::NtpStatus).starts_with("no answer at "), "{}", u.value(Item::NtpStatus));
        u.ntp_report(ok(clock::unix_now(), 412_000_000.0));
        assert!(u.clock_known);
        assert!(u.value(Item::NtpStatus).ends_with(", clock set"));
        assert!(!u.value(Item::TimeNow).contains("not set"));
        u.ntp_report(ok(clock::unix_now(), -0.25));
        assert!(u.value(Item::NtpStatus).ends_with("(-0.25 s)"), "{}", u.value(Item::NtpStatus));
        // zone and server from the pickers
        u.select_option(Item::TimeZone, "Berlin, Paris, Rome");
        assert_eq!(u.sys.tz, "CET-1CEST,M3.5.0,M10.5.0/3");
        let z = u.value(Item::TimeNow);
        assert!(z.ends_with(" CET") || z.ends_with(" CEST"), "{}", z);
        u.select_option(Item::NtpServer, "ptbtime1.ptb.de");
        assert_eq!(u.sys.ntp_server, "ptbtime1.ptb.de");
        // a new server is asked at once
        assert!(u.ntp.st.due.is_some_and(|d| d <= u.now));
    }

    #[test]
    fn dhcp_asks_the_server_of_the_lease_at_each_request() {
        let dir = std::env::temp_dir().join(format!("oos-time-dhcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut u = ui();
        u.ntp.lease_dir = Some(dir.clone());
        assert_eq!(u.sys.ntp_server, sysconf::DHCP, "the default");
        assert_eq!(u.options(Item::NtpServer)[0], "DHCP", "first in the picker");
        u.select_option(Item::NtpOn, "On");
        // no lease file: the fallback, and the line says so
        u.step(0.04);
        assert_eq!(u.ntp.asked, Some(("pool.ntp.org".into(), NtpOrigin::NoDhcp)));
        assert_eq!(u.value(Item::NtpStatus), "Asking pool.ntp.org (no DHCP) ...");
        u.ntp_report(Err("no answer".into()));
        assert_eq!(u.value(Item::NtpStatus), "no answer, pool.ntp.org (no DHCP)");
        // a lease on wlan0, then one on eth0 that wins: read at the request, not at the start
        std::fs::write(dir.join("oos-dhcp-ntp.wlan0"), "10.1.2.3\n").unwrap();
        u.ntp_sync_now();
        u.step(0.04);
        assert_eq!(u.ntp.asked, Some(("10.1.2.3".into(), NtpOrigin::Dhcp("wlan0"))));
        std::fs::write(dir.join("oos-dhcp-ntp.eth0"), "192.168.1.1 192.168.1.2\n").unwrap();
        u.ntp_report(Err("no answer".into()));
        u.ntp_sync_now();
        u.step(0.04);
        assert_eq!(u.value(Item::NtpStatus), "Asking 192.168.1.1 (DHCP) ...");
        u.ntp_report(Ok(Synced { at: clock::unix_now(), server: "192.168.1.1".into(), offset: 0.02, stratum: 2 }));
        let s = u.value(Item::NtpStatus);
        assert!(s.starts_with("Synced ") && s.ends_with(", 192.168.1.1 (DHCP)"), "{}", s);
        // a server of its own: the line as before
        u.select_option(Item::NtpServer, "time.nist.gov");
        u.step(0.04);
        assert_eq!(u.value(Item::NtpStatus), "Asking the server ...");
        u.select_option(Item::NtpServer, "DHCP");
        assert_eq!(u.sys.ntp_server, "dhcp");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn other_opens_the_keyboard_and_checks_the_entry() {
        let mut u = ui();
        let opts = u.options(Item::TimeZone);
        assert_eq!(opts.last().map(|s| s.as_str()), Some(OTHER));
        u.time_pick(Item::TimeZone, opts.len() - 1);
        assert_eq!(u.wlan.osk.as_ref().map(|o| o.text.as_str()), Some("UTC0"));
        u.wlan.osk.as_mut().unwrap().text = "Mars+99".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_some(), "a bad zone keeps the keyboard");
        u.wlan.osk.as_mut().unwrap().text = "<+0545>-5:45".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_none());
        assert_eq!(u.sys.tz, "<+0545>-5:45");
        assert_eq!(u.value(Item::TimeZone), "<+0545>-5:45");
        assert!(u.options(Item::TimeZone).contains(&"<+0545>-5:45".to_string()));
        // server
        let n = u.options(Item::NtpServer).len();
        u.time_pick(Item::NtpServer, n - 1);
        u.wlan.osk.as_mut().unwrap().text = "bad name!".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_some());
        u.wlan.osk.as_mut().unwrap().text = "ntp.example.org".into();
        u.osk_ok();
        assert_eq!(u.sys.ntp_server, "ntp.example.org");
        assert_eq!(u.value(Item::NtpServer), "ntp.example.org");
        // the clock by hand (not set on the host)
        u.activate(Item::TimeSet);
        let o = u.wlan.osk.as_mut().unwrap();
        assert_eq!(o.text.len(), 16, "{}", o.text);
        o.text = "2026-02-30 10:00".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_some());
        u.wlan.osk.as_mut().unwrap().text = "2026-11-11 14:30".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_none() && u.clock_known);
    }

    #[test]
    fn file_names_carry_the_local_time_once_the_clock_is_set() {
        let dir = std::env::temp_dir().join(format!("oos-time-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut u = ui();
        assert_eq!(crate::store::file_name(&u.new_file(&dir, "shot", "png")), "shot-001.png");
        u.clock_known = true;
        u.sys.tz = "JST-9".into();
        let p = u.new_file(&dir, "shot", "png");
        let name = crate::store::file_name(&p);
        let want = format!("shot-{}.png", u.local_now().stamp());
        // the second may have turned between the two readings
        assert!(name == want || name.len() == want.len(), "{} {}", name, want);
        std::fs::write(&p, b"x").unwrap();
        let q = crate::store::file_name(&crate::store::next_name_at(&dir, "shot", "png", Some(&name[5..20])));
        assert_eq!(q, format!("shot-{}-2.png", &name[5..20]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_settings_go_to_the_file() {
        let dir = std::env::temp_dir().join(format!("oos-time-conf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("system.conf");
        let _ = std::fs::remove_file(&p);
        let mut u = ui();
        u.sys_path = Some(p.clone());
        u.select_option(Item::NtpOn, "On");
        u.select_option(Item::TimeZone, "New York, Toronto");
        u.select_option(Item::NetRetry10, "Off");
        let c = sysconf::SysConf::load(&p);
        assert!(c.ntp && !c.eth_retry_10m);
        assert_eq!(c.tz, "EST5EDT,M3.2.0,M11.1.0");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
