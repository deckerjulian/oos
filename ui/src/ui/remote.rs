// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Remote services: which of the network services run, each switch next to the line that says
//! where to reach it. The choice is a setting of the scope (`/oos/system.conf`, `remote.*`); `main.rs` opens and closes
//! the ports from it, `crate::ssh` the SSH server, whose page is ui/ssh.rs. Web, SCPI and VXI-11 are on by default;
//! SSH, the only shell on the network (there is no telnet, ADR 0054), is off unless the image or the user switches it
//! on.
//!
//! The last row: the mDNS responder (`crate::mdns`, on by default: it answers on the local link only) next to the
//! scope's name on the network, `dho924s-123456.local` (`net.name`). The name is also the kernel's host name, which
//! the DHCP client sends (rcS, oos-wlan).
//!
//! Below it: Allowed, the address prefixes that may reach the scope (`net.allow`, docs/network.md 6), and Firewall,
//! the state in force. rcS builds the firewall from the key at power-on and main.rs oos-ui's address list
//! (`crate::netacl`). A change takes effect at once: once it is saved, the list the servers share
//! (`netacl::Shared`) is set, which closes the connections it leaves out, and on the scope a thread runs the
//! firewall again and starts or stops SSH as a start with the new list would (`crate::remote::allow_steps`); the
//! toast says when that is done, or what failed. A computer has no firewall: there only the list changes. An entry is
//! checked by `netacl::canonical` and refused whole when a word is not a prefix: nothing is written then.

use super::worker::Worker;
use super::*;
use crate::remote::{Runner, Step};
use crate::{mdns, netacl};
use std::sync::mpsc::{TryRecvError, TrySendError};

/// A run of the firewall and SSH after a change, and what it answered (Err: what failed).
type AllowRuns = Worker<(Runner, Vec<Step>), Result<(), String>>;

/// Remote services > Allowed.
#[derive(Default)]
pub(super) struct RemoteUi {
    /// the address list the network servers check, shared with them (`Ui::acl`); set after a change was saved
    pub acl: netacl::Shared,
    /// runs the firewall and SSH after a change (`crate::remote::script_runner` on the scope); none on a computer,
    /// which has no firewall, and in tests unless one is given
    pub runner: Option<Runner>,
    /// the thread of the runs, one at a time, from the first change on
    runs: Option<AllowRuns>,
    /// the run of a change made while another ran: it goes when that one answered, whose answer is then old. Of
    /// several such changes the newest is kept; each run reads the settings file as it is then, so it is the one
    /// that counts.
    waiting: Option<(Runner, Vec<Step>)>,
    /// the last run failed: the Firewall row says so
    pub failed: bool,
    /// the networks the scope is on, `192.168.1.0/24` (every IPv4 address with its netmask, `netacl::lans`), read
    /// while the page is open
    pub lans: Vec<String>,
}

const OTHER: &str = "Other ...";
/// Characters of a name in the picker.
const NAME_SHOW: usize = 18;

fn short_name(n: &str) -> String {
    if n.chars().count() <= NAME_SHOW {
        return n.to_string();
    }
    format!("{}\u{2026}", n.chars().take(NAME_SHOW - 1).collect::<String>())
}

impl Ui {
    /// Switches in the left column, what they open in the right one, row by row.
    pub(super) fn remote_items(&self) -> Vec<Item> {
        vec![
            Item::RemWeb,
            Item::RemScpi,
            Item::RemVxi,
            Item::RemSsh,
            Item::RemMdns,
            Item::RemAllow,
            Item::NetWeb,
            Item::RemScpiInfo,
            Item::RemVxiInfo,
            Item::Open(MenuId::Ssh),
            Item::RemName,
            Item::RemFirewall,
        ]
    }

    pub(super) fn remote_kind(it: Item) -> Option<Kind> {
        match it {
            Item::RemWeb
            | Item::RemScpi
            | Item::RemVxi
            | Item::RemSsh
            | Item::RemMdns
            | Item::RemName
            | Item::RemAllow => Some(Kind::Choice),
            _ => Self::ssh_kind(it),
        }
    }

    /// The name from model and serial number (`dho924s-123456`).
    fn name_default() -> String {
        let p = oos_hal::model::platform();
        mdns::default_name(&p.caps.name, p.serial.as_deref())
    }

    /// The scope's name on the network: the setting, else the one from model and serial number.
    pub fn net_name(&self) -> String {
        if self.sys.net_name.is_empty() {
            Self::name_default()
        } else {
            self.sys.net_name.clone()
        }
    }

    /// At the start on the unit: a settings file without a name gets the one from model and serial number, so that rcS
    /// gives it to the kernel and to DHCP from the next start on (rcS runs before oos-ui). Only with a serial number:
    /// a start that could not read it does not keep the name without one.
    pub(super) fn net_name_init(&mut self) {
        if self.sys.net_name.is_empty() && oos_hal::model::platform().serial.is_some() {
            self.sys.net_name = Self::name_default();
            self.sys_save();
        }
    }

    /// What the mDNS responder announces (main.rs), with the ports the services run on.
    pub fn mdns_config(&self, web_port: u16, scpi_port: u16) -> mdns::Config {
        let p = oos_hal::model::platform();
        let s = &self.sys;
        let name = self.net_name();
        let mut services = Vec::new();
        let mut add = |on: bool, kind: &str, port: u16, txt: &[&str]| {
            if on {
                services.push(mdns::Service {
                    kind: kind.into(),
                    port,
                    txt: txt.iter().map(|t| t.to_string()).collect(),
                });
            }
        };
        add(s.remote_web, "_http._tcp", web_port, &["path=/"]);
        add(s.remote_scpi, "_scpi-raw._tcp", scpi_port, &[]);
        add(s.remote_vxi11, "_vxi-11._tcp", 111, &[]);
        // SSH (docs/network.md 7): announced only while oos-ssh would listen (it needs `net.allow` as well)
        add(s.remote_ssh && crate::ssh::listens(), "_ssh._tcp", 22, &[]);
        mdns::Config {
            on: s.remote_mdns,
            instance: mdns::instance_name(&p.caps.name, p.serial.as_deref(), &name),
            name,
            services,
            v6: crate::net6::enabled(),
        }
    }

    /// The name's row: `<name>.local` while mDNS is on (the one in use: `-2` after a conflict), else the name.
    fn name_value(&self) -> String {
        let st = mdns::status();
        let n = match st.state {
            mdns::State::Probing | mdns::State::Running if !st.name.is_empty() => st.name,
            _ => self.net_name(),
        };
        let suffix = if self.sys.remote_mdns { ".local" } else { "" };
        let full = format!("{}{}", n, suffix);
        // a long name: as much as its row has room for, `.local` kept
        let room = self
            .dlg_layout()
            .and_then(|d| d.rows.iter().find(|r| r.it == Item::RemName).map(|r| Self::value_parts(r.ctrl).1.w - 44));
        let f = &self.fonts().mb;
        let Some(room) = room.filter(|&w| f.width(&full) > w) else { return full };
        let mut c: Vec<char> = n.chars().collect();
        while c.pop().is_some() {
            let t = format!("{}\u{2026}{}", c.iter().collect::<String>(), suffix);
            if f.width(&t) <= room {
                return t;
            }
        }
        format!("\u{2026}{}", suffix)
    }

    /// The name set, when it is not the one from model and serial number.
    fn name_own(&self) -> Option<String> {
        let n = &self.sys.net_name;
        (!n.is_empty() && *n != Self::name_default()).then(|| n.clone())
    }

    fn name_options(&self) -> Vec<String> {
        let mut v = vec![short_name(&Self::name_default())];
        v.extend(self.name_own().map(|n| short_name(&n)));
        v.push(OTHER.into());
        v
    }

    fn set_net_name(&mut self, n: &str) {
        if self.net_name() != n || self.sys.net_name.is_empty() {
            self.sys.net_name = n.to_string();
            self.sys_save();
            self.toast(&format!("Name {}: DHCP sends it from the next start", n));
        }
    }

    /// A line of the name's picker: the name from model and serial, the one set, or Other on the keyboard.
    pub(super) fn remote_pick(&mut self, k: usize) {
        let n = self.name_options().len();
        if k + 1 == n {
            self.wlan.osk = Some(wlan::Osk::with_text(wlan::OskFor::NetName, &self.net_name()));
        } else if k == 0 {
            self.set_net_name(&Self::name_default());
        }
        // the name set: stays
    }

    /// OK on the keyboard of the name. False: the keyboard stays open to correct it.
    pub(super) fn remote_osk_done(&mut self, text: &str) -> bool {
        let t = text.trim().to_ascii_lowercase();
        if !mdns::name_ok(&t) {
            self.toast("Letters, digits and -, up to 63");
            return false;
        }
        self.set_net_name(&t);
        true
    }

    /// The network the scope is on, for the picker and the keyboard of Allowed (`RemoteUi::lan`).
    pub(super) fn allow_lan_read(&mut self) {
        self.remote.lans = netacl::lans(&mdns::ipv4_networks());
    }

    /// The lines of Allowed's picker: the list in use (stays), the network the scope is on, Other on the keyboard.
    /// The value each one sets, None for the keyboard.
    fn allow_choices(&self) -> Vec<(String, Option<String>)> {
        let now = self.sys.net_allow();
        let mut v = Vec::new();
        if !now.is_empty() {
            v.push((short_name(&now), Some(now.clone())));
        }
        for l in self.remote.lans.iter().filter(|l| **l != now) {
            v.push((short_name(l), Some(l.clone())));
        }
        v.push((OTHER.into(), None));
        v
    }

    /// A line of Allowed's picker.
    pub(super) fn allow_pick(&mut self, k: usize) {
        match self.allow_choices().get(k) {
            Some((_, Some(v))) if *v != self.sys.net_allow() => self.set_allow(&v.clone()),
            Some((_, None)) => {
                let now = self.sys.net_allow();
                let start = if now.is_empty() { self.remote.lans.first().cloned().unwrap_or_default() } else { now };
                self.wlan.osk = Some(wlan::Osk::with_text(wlan::OskFor::NetAllow, &start));
            }
            _ => {}
        }
    }

    /// OK on the keyboard of Allowed. False: refused, the keyboard stays open to correct it and nothing is written.
    pub(super) fn allow_osk_done(&mut self, text: &str) -> bool {
        match netacl::canonical(text) {
            Ok(v) => {
                self.set_allow(&v);
                true
            }
            Err(m) => {
                self.toast(&m);
                false
            }
        }
    }

    /// The address list the network servers check (main.rs hands it to them); Allowed sets it.
    pub fn acl(&self) -> netacl::Shared {
        self.remote.acl.clone()
    }

    /// On the scope: what runs the firewall and SSH after a change of Allowed (`crate::remote::script_runner`).
    pub fn set_allow_runner(&mut self, r: Runner) {
        self.remote.runner = Some(r);
    }

    /// Writes `net.allow` (a value `netacl::canonical` gave; empty: the settings' line goes, the image default applies)
    /// and puts it in force: the servers' list at once, then the firewall and SSH (`allow_run`). A save that fails
    /// changes nothing.
    fn set_allow(&mut self, v: &str) {
        let before = self.sys.clone();
        self.sys.set_net_allow((!v.is_empty()).then_some(v));
        if !self.sys_save() {
            self.sys = before;
            return;
        }
        let now = self.sys.net_allow();
        let shown = if now.is_empty() { "(none): everyone" } else { &now };
        eprintln!("oos-ui: net.allow = {} (Remote services > Allowed)", shown);
        self.remote.acl.set(netacl::Acl::from_value(&now).0);
        match self.remote.runner.clone() {
            Some(run) => self.allow_run(run, &now),
            None => self.allow_done(Ok(())),
        }
    }

    /// The firewall and SSH for the list `now`, on the thread of the runs: the scripts take a moment, the frame loop
    /// does not wait. A change made while a run is out waits for its answer.
    fn allow_run(&mut self, run: Runner, now: &str) {
        let steps = crate::remote::allow_steps(self.sys.remote_ssh, !now.is_empty());
        self.remote.failed = false;
        self.allow_submit((run, steps));
    }

    /// Hands a run to the thread of the runs, which it starts first when there is none; while one is out, the run
    /// waits in its place.
    fn allow_submit(&mut self, job: (Runner, Vec<Step>)) {
        if self.remote.runs.is_none() {
            match Worker::spawn("net-allow", |(run, steps): (Runner, Vec<Step>)| {
                let mut r = Ok(());
                for s in &steps {
                    if let Err(e) = run(s) {
                        eprintln!("oos-ui: net.allow: {}", e);
                        r = r.and(Err(e));
                    }
                }
                r
            }) {
                Ok(w) => self.remote.runs = Some(w),
                Err(e) => return self.allow_done(Err(format!("no thread for the firewall: {}", e))),
            }
        }
        let Some(runs) = self.remote.runs.as_mut() else { return };
        match runs.submit(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => self.remote.waiting = Some(job),
            Err(TrySendError::Disconnected(job)) => {
                self.remote.waiting = Some(job);
                self.allow_gone();
            }
        }
    }

    /// The thread of the runs ended without an answer: the run that waited goes on a new thread, so the newest
    /// change is still put in force; without one the change that was out failed.
    fn allow_gone(&mut self) {
        self.remote.runs = None;
        match self.remote.waiting.take() {
            Some(job) => self.allow_submit(job),
            None => self.allow_done(Err("the firewall's run ended without an answer".into())),
        }
    }

    /// A run of a change is out or waiting: the Firewall row says "applying".
    fn allow_applying(&self) -> bool {
        self.remote.runs.as_ref().is_some_and(Worker::busy) || self.remote.waiting.is_some()
    }

    /// The answer of the firewall's run, once it is there (every frame, `step`).
    pub(super) fn allow_poll(&mut self) {
        let Some(runs) = self.remote.runs.as_mut().filter(|w| w.busy()) else { return };
        match runs.poll() {
            Ok(r) => self.allow_answer(r),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.allow_gone(),
        }
    }

    /// A run answered: the change that waited goes now, and the answer is old; else it is the answer.
    fn allow_answer(&mut self, r: Result<(), String>) {
        match self.remote.waiting.take() {
            Some(job) => self.allow_submit(job),
            None => self.allow_done(r),
        }
    }

    /// Tests: waits for the answer of the firewall's run, and of the change that waited for it.
    #[cfg(test)]
    fn allow_wait(&mut self) {
        while let Some(runs) = self.remote.runs.as_mut().filter(|w| w.busy()) {
            match runs.wait(std::time::Duration::from_secs(60)) {
                Ok(r) => self.allow_answer(r),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => self.allow_gone(),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("the firewall's run did not answer in 60 s"),
            }
        }
    }

    /// The change is in force, or what failed: the toast and the Firewall row.
    fn allow_done(&mut self, r: Result<(), String>) {
        self.remote.failed = r.is_err();
        let now = self.sys.net_allow();
        let what = if now.is_empty() { "No address list: everyone".to_string() } else { format!("Allowed: {}", now) };
        match r {
            Ok(()) => self.toast(&format!("{}, in force", what)),
            Err(e) => self.toast(&format!("{}: {}", what, e)),
        }
    }

    /// Allowed's row: the list in use, cut to the room of its row.
    fn allow_value(&self) -> String {
        let now = self.sys.net_allow();
        if now.is_empty() {
            return "everyone".into();
        }
        let room = self
            .dlg_layout()
            .and_then(|d| d.rows.iter().find(|r| r.it == Item::RemAllow).map(|r| Self::value_parts(r.ctrl).1.w - 44));
        let f = &self.fonts().mb;
        let Some(room) = room.filter(|&w| f.width(&now) > w) else { return now };
        let mut c: Vec<char> = now.chars().collect();
        while c.pop().is_some() {
            let t = format!("{}\u{2026}", c.iter().collect::<String>());
            if f.width(&t) <= room {
                return t;
            }
        }
        "\u{2026}".into()
    }

    /// Firewall's row: the state in force, or the run of a change that has not answered yet or failed.
    fn firewall_value(&self) -> String {
        if self.allow_applying() {
            "applying".into()
        } else if self.remote.failed {
            "failed: see the log".into()
        } else if self.sys.net_allow().is_empty() {
            "off: everyone".into()
        } else {
            "on".into()
        }
    }

    /// Rows that are only text.
    pub(super) fn remote_text(it: Item) -> bool {
        matches!(it, Item::NetWeb | Item::RemScpiInfo | Item::RemVxiInfo | Item::RemFirewall) || Self::ssh_text(it)
    }

    pub(super) fn remote_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Remote) => "Remote services",
            Item::RemWeb => "Web remote",
            Item::RemScpi => "SCPI",
            Item::RemVxi => "VXI-11",
            Item::RemSsh => "SSH",
            Item::RemMdns => "mDNS",
            Item::RemName => "Name",
            Item::RemAllow => "Allowed",
            Item::RemFirewall => "Firewall",
            Item::NetWeb => "Address",
            Item::RemScpiInfo | Item::RemVxiInfo => "Port",
            _ => return Self::ssh_label(it),
        })
    }

    pub(super) fn remote_value(&self, it: Item) -> Option<String> {
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        let s = &self.sys;
        let port = |on: bool, text: String| if on { text } else { "off".into() };
        Some(match it {
            Item::RemWeb => onoff(s.remote_web),
            Item::RemScpi => onoff(s.remote_scpi),
            Item::RemVxi => onoff(s.remote_vxi11),
            Item::RemSsh => onoff(s.remote_ssh),
            Item::RemMdns => onoff(s.remote_mdns),
            Item::RemName => self.name_value(),
            Item::RemAllow => self.allow_value(),
            Item::RemFirewall => self.firewall_value(),
            Item::NetWeb if !s.remote_web => "off".into(),
            Item::NetWeb if self.net.is_empty() => "not connected".into(),
            Item::NetWeb => format!("http://{}:8080", self.net),
            Item::RemScpiInfo => port(s.remote_scpi, "5555 (TCP)".into()),
            Item::RemVxiInfo => port(s.remote_vxi11, format!("111 and {} (LXI)", crate::net::VXI11_CORE_PORT)),
            _ => return self.ssh_value(it),
        })
    }

    pub(super) fn remote_options(&self, it: Item) -> Option<Vec<String>> {
        if let Some(o) = self.ssh_options(it) {
            return Some(o);
        }
        match it {
            Item::RemName => Some(self.name_options()),
            Item::RemAllow => Some(self.allow_choices().into_iter().map(|c| c.0).collect()),
            _ => Self::remote_kind(it).filter(|k| *k == Kind::Choice).map(|_| vec!["Off".into(), "On".into()]),
        }
    }

    pub(super) fn remote_adjust(&mut self, it: Item, d: i32) -> bool {
        let s = &mut self.sys;
        let (on, what) = match it {
            Item::RemWeb => (&mut s.remote_web, "Web remote"),
            Item::RemScpi => (&mut s.remote_scpi, "SCPI"),
            Item::RemVxi => (&mut s.remote_vxi11, "VXI-11"),
            Item::RemSsh => (&mut s.remote_ssh, "SSH"),
            Item::RemMdns => (&mut s.remote_mdns, "mDNS"),
            // the name and the address list change in their pickers only
            Item::RemName | Item::RemAllow => return true,
            _ => return self.ssh_adjust(it, d),
        };
        *on = !*on;
        let now_on = *on;
        self.sys_save();
        // what is lost with it
        match (it, now_on) {
            (Item::RemWeb, false) => self.toast("Web remote off: updates then come from a USB stick"),
            (Item::RemSsh, false) => self.toast("SSH off: its sessions end"),
            _ => self.toast(&format!("{} {}", what, if now_on { "on" } else { "off" })),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn switches_stand_next_to_what_they_open() {
        let mut u = crate::ui::testkit::demo_ui();
        u.open_menu(MenuId::Remote);
        let d = u.dlg_layout().unwrap();
        // two columns of six: each switch has its address, port, settings or name on the same row, the address list
        // its firewall
        for (sw, info) in [
            (Item::RemWeb, Item::NetWeb),
            (Item::RemScpi, Item::RemScpiInfo),
            (Item::RemVxi, Item::RemVxiInfo),
            (Item::RemSsh, Item::Open(MenuId::Ssh)),
            (Item::RemMdns, Item::RemName),
            (Item::RemAllow, Item::RemFirewall),
        ] {
            let a = d.rows.iter().find(|r| r.it == sw).unwrap().ctrl;
            let b = d.rows.iter().find(|r| r.it == info).unwrap().ctrl;
            assert!(a.y == b.y && a.x < b.x, "{:?} {:?}", sw, info);
        }
    }

    #[test]
    fn a_service_switched_off_says_so() {
        let mut u = crate::ui::testkit::demo_ui();
        assert!(u.sys.remote_web && u.sys.remote_scpi && u.sys.remote_vxi11, "on by default");
        assert!(!u.sys.remote_ssh, "SSH off by default");
        u.net = "192.168.1.9".into();
        assert_eq!(u.value(Item::NetWeb), "http://192.168.1.9:8080");
        u.select_option(Item::RemWeb, "Off");
        assert!(!u.sys.remote_web);
        assert_eq!(u.value(Item::NetWeb), "off");
        assert!(u.toast.as_ref().unwrap().0.contains("USB stick"));
        u.select_option(Item::RemScpi, "Off");
        assert_eq!(u.value(Item::RemScpiInfo), "off");
        u.select_option(Item::RemScpi, "On");
        assert_eq!(u.value(Item::RemScpiInfo), "5555 (TCP)");
    }

    #[test]
    fn the_name_and_what_mdns_announces() {
        let mut u = crate::ui::testkit::demo_ui();
        let p = oos_hal::model::platform();
        let d = mdns::default_name(&p.caps.name, p.serial.as_deref());
        assert!(u.sys.remote_mdns, "on by default");
        assert_eq!(u.net_name(), d);
        assert_eq!(u.value(Item::RemName), format!("{}.local", d));
        let c = u.mdns_config(8080, 5555);
        assert!(c.on && c.name == d);
        let kinds: Vec<(&str, u16)> = c.services.iter().map(|s| (s.kind.as_str(), s.port)).collect();
        assert_eq!(kinds, [("_http._tcp", 8080), ("_scpi-raw._tcp", 5555), ("_vxi-11._tcp", 111)]);
        // a service switched off is not announced
        u.select_option(Item::RemScpi, "Off");
        assert!(!u.mdns_config(8080, 5555).services.iter().any(|s| s.kind == "_scpi-raw._tcp"));
        // the name from the keyboard: checked, lower case
        let opts = u.options(Item::RemName);
        assert_eq!(opts, vec![d.clone(), OTHER.to_string()]);
        u.remote_pick(1);
        assert_eq!(u.wlan.osk.as_ref().map(|o| o.text.clone()), Some(d.clone()));
        u.wlan.osk.as_mut().unwrap().text = "lab scope".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_some(), "a bad name keeps the keyboard");
        u.wlan.osk.as_mut().unwrap().text = "Lab-Scope-3".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_none());
        assert_eq!(u.sys.net_name, "lab-scope-3");
        assert_eq!(u.value(Item::RemName), "lab-scope-3.local");
        assert_eq!(u.mdns_config(8080, 5555).instance, format!("Rigol {} OOS (lab-scope-3)", p.caps.name));
        assert_eq!(u.options(Item::RemName), vec![d.clone(), "lab-scope-3".into(), OTHER.into()]);
        // back to the one from model and serial through the picker
        u.open_menu(MenuId::Remote);
        u.open_picker(Item::RemName);
        u.activate(Item::PickOpt(0));
        assert_eq!(u.sys.net_name, d);
        // mDNS off: the name without .local, nothing announced
        u.select_option(Item::RemMdns, "Off");
        assert!(!u.mdns_config(8080, 5555).on);
        assert_eq!(u.value(Item::RemName), d);
        // the knob does not change the name
        u.adjust(Item::RemName, 1);
        assert_eq!(u.sys.net_name, d);
    }

    // Break it catches: no way to set net.allow on the scope (SSH says "off: needs net.allow" and nothing on the
    // screen fixes it); a typo written to the file (the firewall would close the scope or leave a word out); a change
    // that waits for the next start (the servers' list kept, no firewall run)
    #[test]
    fn the_address_list_from_the_screen() {
        let dir = crate::ui::testkit::Scratch::new("ui-allow");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("system.conf");
        let mut u = crate::ui::testkit::demo_ui();
        u.sys_path = Some(p.clone());
        let servers = u.acl();
        let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap();
        u.open_menu(MenuId::Remote);
        // the row is there, with no list: everyone, no firewall
        assert!(u.dlg_layout().unwrap().rows.iter().any(|r| r.it == Item::RemAllow));
        assert_eq!(
            (u.value(Item::RemAllow).as_str(), u.value(Item::RemFirewall).as_str()),
            ("everyone", "off: everyone")
        );
        // the network the scope is on is offered and starts the keyboard
        u.remote.lans = vec!["192.0.2.0/24".into()];
        assert_eq!(u.options(Item::RemAllow), vec!["192.0.2.0/24".to_string(), OTHER.into()]);
        u.open_picker(Item::RemAllow);
        u.activate(Item::PickOpt(1));
        assert_eq!(u.wlan.osk.as_ref().map(|o| o.text.clone()), Some("192.0.2.0/24".into()));
        // a typo is refused with a message, the keyboard stays open and nothing is written
        u.wlan.osk.as_mut().unwrap().text = "192.0.2.0/24 192.168.1.0/33".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_some(), "a bad entry keeps the keyboard");
        assert_eq!(u.toast.as_ref().unwrap().0, "Not an address prefix: 192.168.1.0/33");
        assert_eq!(u.sys.net_allow(), "");
        assert!(!p.exists(), "nothing written");
        assert!(servers.permits(ip("198.51.100.1")), "the servers' list unchanged");
        // a good one is written in its plain form and is in force at once: no firewall here (a computer), the servers'
        // list follows
        u.wlan.osk.as_mut().unwrap().text = "192.0.2.77/24, fd00::/8".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_none());
        assert_eq!(u.toast.as_ref().unwrap().0, "Allowed: 192.0.2.0/24 fd00::/8, in force");
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(crate::sysconf::value(&text, "net.allow"), Some("192.0.2.0/24 fd00::/8"));
        let (acl, bad) = netacl::Acl::from_texts(None, Some(&text));
        assert!(bad.is_empty() && acl.permits(ip("192.0.2.9")) && !acl.permits(ip("198.51.100.1")));
        assert_eq!(crate::sysconf::SysConf::load(&p).net_allow(), "192.0.2.0/24 fd00::/8");
        assert_eq!(u.value(Item::RemAllow), "192.0.2.0/24 fd00::/8");
        assert_eq!(u.value(Item::RemFirewall), "on");
        assert!(servers.permits(ip("192.0.2.9")) && servers.permits(ip("fd12::1")));
        assert!(!servers.permits(ip("198.51.100.1")));
        // the list in use (cut as a name is) and the network are offered; the network sets the list from the picker
        u.remote.lans = vec!["198.51.100.0/24".into(), "203.0.113.0/24".into()];
        assert_eq!(
            u.options(Item::RemAllow),
            vec![
                "192.0.2.0/24 fd00\u{2026}".to_string(),
                "198.51.100.0/24".into(),
                "203.0.113.0/24".into(),
                OTHER.into()
            ]
        );
        u.open_menu(MenuId::Remote);
        u.open_picker(Item::RemAllow);
        u.activate(Item::PickOpt(1));
        assert_eq!(u.sys.net_allow(), "198.51.100.0/24");
        assert!(servers.permits(ip("198.51.100.1")) && !servers.permits(ip("192.0.2.9")));
        // the knob does not change it
        u.adjust(Item::RemAllow, 1);
        assert_eq!(u.sys.net_allow(), "198.51.100.0/24");
        // an empty entry removes the line: no list (no image default on the host), everyone again
        u.allow_pick(u.options(Item::RemAllow).len() - 1);
        u.wlan.osk.as_mut().unwrap().text.clear();
        u.osk_ok();
        assert!(u.wlan.osk.is_none());
        assert_eq!(u.toast.as_ref().unwrap().0, "No address list: everyone, in force");
        assert_eq!(u.value(Item::RemFirewall), "off: everyone");
        assert_eq!(crate::sysconf::value(&std::fs::read_to_string(&p).unwrap(), "net.allow"), None);
        assert!(servers.permits(ip("203.0.113.5")));
        // a save that fails changes nothing: neither the setting nor the servers' list
        let blocked = dir.join("a-file");
        std::fs::write(&blocked, "").unwrap();
        u.sys_path = Some(blocked.join("system.conf"));
        u.allow_osk_done("192.0.2.0/24");
        assert!(u.toast.as_ref().unwrap().0.starts_with("Not saved"), "{:?}", u.toast);
        assert_eq!(u.sys.net_allow(), "");
        assert!(servers.permits(ip("203.0.113.5")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A runner that keeps what it was asked to run and fails `fail` (the scope's scripts are never run in a test).
    fn recorder(fail: Option<&'static str>) -> (Runner, std::sync::Arc<Mutex<Vec<String>>>) {
        let ran = std::sync::Arc::new(Mutex::new(Vec::new()));
        let r = ran.clone();
        let run: Runner = std::sync::Arc::new(move |s: &crate::remote::Step| {
            r.lock().unwrap().push(format!("{} {} >> {}", s.prog, s.arg, s.log));
            if fail == Some(s.prog) {
                return Err(format!("{} {} failed (exit status: 1), see {}", s.prog, s.arg, s.log));
            }
            Ok(())
        });
        (run, ran)
    }

    // Break it catches: a change of Allowed that leaves the firewall of the boot in place (or runs it in the frame
    // loop), SSH not started when a list appears with its switch on, or still listening with its sessions when the
    // list is gone; a failure of the firewall not told
    #[test]
    fn a_change_of_allowed_runs_the_firewall_and_ssh() {
        let fw = "/bin/oos-firewall reload >> /tmp/oos-net.log";
        let ssh = |a: &str| format!("/bin/oos-ssh {} >> /tmp/oos-ssh.log", a);
        let mut u = crate::ui::testkit::demo_ui();
        let (run, ran) = recorder(None);
        u.set_allow_runner(run);
        u.open_menu(MenuId::Remote);
        // SSH off: the firewall alone; the row says so until the run answered, then the state in force
        assert!(u.allow_osk_done("192.0.2.0/24"));
        assert!(u.allow_applying(), "the run is on a thread of its own");
        assert_eq!(u.value(Item::RemFirewall), "applying");
        assert!(!u.acl().permits("198.51.100.1".parse().unwrap()), "the servers' list does not wait for it");
        u.allow_wait();
        assert_eq!(*ran.lock().unwrap(), vec![fw.to_string()]);
        assert_eq!(u.toast.as_ref().unwrap().0, "Allowed: 192.0.2.0/24, in force");
        assert_eq!(u.value(Item::RemFirewall), "on");
        // SSH on: it listens with a list (the sessions stay) ...
        u.sys.remote_ssh = true;
        ran.lock().unwrap().clear();
        assert!(u.allow_osk_done("198.51.100.0/24"));
        u.allow_wait();
        assert_eq!(*ran.lock().unwrap(), vec![fw.to_string(), ssh("restart")]);
        // ... and without one ends with its sessions and stays off, as a start without net.allow leaves it
        ran.lock().unwrap().clear();
        assert!(u.allow_osk_done(""));
        u.allow_wait();
        assert_eq!(*ran.lock().unwrap(), vec![fw.to_string(), ssh("stop"), ssh("start")]);
        assert_eq!(u.toast.as_ref().unwrap().0, "No address list: everyone, in force");
        assert_eq!(u.value(Item::RemFirewall), "off: everyone");
        // the answer also comes through the frame loop's poll
        assert!(u.allow_osk_done("192.0.2.0/24"));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while u.allow_applying() {
            assert!(std::time::Instant::now() < until, "no answer");
            std::thread::sleep(std::time::Duration::from_millis(1));
            u.allow_poll();
        }
        assert_eq!(u.value(Item::RemFirewall), "on");
        // the firewall fails: the toast names it, the row says so, SSH still follows the list; the servers' list is
        // the new one all the same
        let (run, ran) = recorder(Some(crate::remote::FIREWALL));
        u.set_allow_runner(run);
        assert!(u.allow_osk_done("203.0.113.0/24"));
        u.allow_wait();
        assert_eq!(*ran.lock().unwrap(), vec![fw.to_string(), ssh("restart")]);
        let toast = "Allowed: 203.0.113.0/24: /bin/oos-firewall reload failed (exit status: 1), see /tmp/oos-net.log";
        assert_eq!(u.toast.as_ref().unwrap().0, toast);
        assert_eq!(u.value(Item::RemFirewall), "failed: see the log");
        assert!(u.acl().permits("203.0.113.9".parse().unwrap()) && !u.acl().permits("192.0.2.9".parse().unwrap()));
    }

    /// A runner that keeps what it ran and waits for a release before each step, so a test can make changes while a
    /// run is out; `panic_first` makes the first step end the thread.
    fn gated(panic_first: bool) -> (Runner, std::sync::Arc<Mutex<Vec<String>>>, std::sync::mpsc::Sender<()>) {
        let ran = std::sync::Arc::new(Mutex::new(Vec::new()));
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let first = std::sync::atomic::AtomicBool::new(panic_first);
        let r = ran.clone();
        let run: Runner = std::sync::Arc::new(move |s: &crate::remote::Step| {
            let _ = gate.lock().unwrap().recv();
            if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                panic!("the test ends the thread of the runs");
            }
            r.lock().unwrap().push(format!("{} {}", s.prog, s.arg));
            Ok(())
        });
        (run, ran, release)
    }

    // Break it catches: changes made while a run is out that each run again (stale lists, SSH stopped and started for
    // nothing), the answer of an old run shown as the new one's, the newest change lost when the thread ends
    #[test]
    fn changes_while_a_run_is_out_wait_and_only_the_newest_runs() {
        let mut u = crate::ui::testkit::demo_ui();
        let (run, ran, release) = gated(false);
        u.set_allow_runner(run);
        u.open_menu(MenuId::Remote);
        assert!(u.allow_osk_done("192.0.2.0/24"));
        assert!(u.allow_osk_done("198.51.100.0/24"));
        u.sys.remote_ssh = true;
        assert!(u.allow_osk_done(""));
        assert!(u.allow_applying());
        assert_eq!(u.value(Item::RemFirewall), "applying");
        // the first run (the firewall), then the newest change alone (the firewall, SSH stopped and started)
        for _ in 0..4 {
            release.send(()).unwrap();
        }
        u.allow_wait();
        let fw = "/bin/oos-firewall reload".to_string();
        let ssh = |a: &str| format!("/bin/oos-ssh {}", a);
        assert_eq!(*ran.lock().unwrap(), vec![fw.clone(), fw, ssh("stop"), ssh("start")]);
        assert_eq!(u.toast.as_ref().unwrap().0, "No address list: everyone, in force");
        assert!(!u.allow_applying());
    }

    #[test]
    fn the_newest_change_still_runs_when_the_thread_of_the_runs_ends() {
        let mut u = crate::ui::testkit::demo_ui();
        let (run, ran, release) = gated(true);
        u.set_allow_runner(run);
        u.open_menu(MenuId::Remote);
        assert!(u.allow_osk_done("192.0.2.0/24"));
        assert!(u.allow_osk_done("198.51.100.0/24"));
        // the first run ends its thread; the change that waited goes on a new one
        release.send(()).unwrap();
        release.send(()).unwrap();
        u.allow_wait();
        assert_eq!(*ran.lock().unwrap(), vec!["/bin/oos-firewall reload".to_string()]);
        assert_eq!(u.toast.as_ref().unwrap().0, "Allowed: 198.51.100.0/24, in force");
        assert_eq!(u.value(Item::RemFirewall), "on");
    }
}
