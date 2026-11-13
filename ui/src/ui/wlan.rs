// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The network page (network box in the top bar, or System > Network): overview of the interfaces, and finding, joining
//! and forgetting WLAN networks through a USB stick (docs/wlan.md). `/bin/oos-wlan`
//! runs the driver, wpa_supplicant and DHCP; a worker thread here talks to wpa_supplicant, so that nothing waits on
//! the radio in the frame loop. Passwords come from the on-screen keyboard (`Osk`) and only their derived key leaves
//! this file.

use super::*;
use oos_hal::wlan::{self, Ctrl, Net, Saved, Security, Status};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// How long a join may take before it counts as failed.
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);
/// wpa_supplicant's state is read this often.
const POLL: Duration = Duration::from_secs(2);
/// Lines of the two-column network pickers that fit the screen.
const LIST_MAX: usize = 18;
/// Characters of a network name in a picker line.
const NAME_MAX: usize = 24;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum How {
    /// a network already saved
    Saved(u32),
    Open,
    Pass(String),
}

enum Job {
    Scan,
    Join(Vec<u8>, How),
    Forget(u32),
}

pub(super) enum Report {
    Link(Link),
    Scan(Result<Vec<Net>, String>),
    Done(Result<String, String>),
}

/// What the worker last saw.
#[derive(Clone, Debug, Default)]
pub(super) struct Link {
    /// wlan0 exists: a stick with a working driver
    pub stick: bool,
    /// wpa_supplicant answers
    pub ctrl: bool,
    pub status: Status,
    pub signal: Option<i32>,
    pub saved: Vec<Saved>,
}

#[derive(Default)]
pub(super) struct WlanUi {
    pub link: Link,
    pub nets: Vec<Net>,
    /// a scan or join is running (the text is for the status dialog)
    pub busy: Option<&'static str>,
    pub osk: Option<Osk>,
    tx: Option<Sender<Job>>,
    rx: Option<Receiver<Report>>,
}

fn short(name: &str) -> String {
    if name.chars().count() <= NAME_MAX {
        return name.to_string();
    }
    format!("{}\u{2026}", name.chars().take(NAME_MAX - 1).collect::<String>())
}

pub(super) fn stick_there() -> bool {
    Path::new("/sys/class/net").join(wlan::IFACE).exists()
}

fn link() -> (Option<Ctrl>, Link) {
    let mut l = Link { stick: stick_there(), ..Link::default() };
    if !l.stick {
        return (None, l);
    }
    let Ok(c) = Ctrl::open(Path::new(wlan::CTRL)) else { return (None, l) };
    if let Ok(s) = c.status() {
        l.ctrl = true;
        l.signal = if s.connected() { c.signal() } else { None };
        l.status = s;
        l.saved = c.networks().unwrap_or_default();
    }
    (Some(c), l)
}

fn with_ctrl<T>(f: impl FnOnce(&Ctrl) -> Result<T, String>) -> Result<T, String> {
    if !stick_there() {
        return Err("No WLAN stick found".into());
    }
    f(&Ctrl::open(Path::new(wlan::CTRL)).map_err(|_| "WLAN is not ready yet".to_string())?)
}

fn scan() -> Result<Vec<Net>, String> {
    with_ctrl(|c| {
        c.scan()?;
        // a software scan of 13 channels takes a few seconds; the list also holds earlier results
        std::thread::sleep(Duration::from_secs(4));
        let mut v = c.scan_results().map_err(|e| e.to_string())?;
        if v.is_empty() {
            std::thread::sleep(Duration::from_secs(3));
            v = c.scan_results().map_err(|e| e.to_string())?;
        }
        Ok(v)
    })
}

fn saved_to_card(c: &Ctrl) -> Result<(), String> {
    c.save()?;
    // the scope is usually switched off at the mains
    crate::store::sync_disks();
    Ok(())
}

/// Joins a network and keeps it only if that worked.
fn join(ssid: &[u8], how: How) -> Result<String, String> {
    let name = wlan::name(ssid);
    with_ctrl(|c| {
        let (id, new) = match &how {
            How::Saved(id) => (*id, false),
            How::Open => (c.add(ssid, None)?, true),
            How::Pass(p) => (c.add(ssid, Some(&wlan::psk(p, ssid)))?, true),
        };
        let wrong = if how == How::Open { "no answer" } else { "wrong password?" };
        c.select(id)?;
        let end = Instant::now() + JOIN_TIMEOUT;
        let done = loop {
            std::thread::sleep(Duration::from_millis(500));
            let st = c.status().map_err(|e| e.to_string())?;
            if st.connected() && st.ssid == ssid {
                break true;
            }
            let gave_up = c.networks().is_ok_and(|v| v.iter().any(|n| n.id == id && n.temp_disabled));
            if gave_up || Instant::now() > end {
                break false;
            }
        };
        // SELECT_NETWORK switched the other saved networks off
        c.enable_all()?;
        if !done {
            if new {
                c.remove(id)?;
            }
            return Err(format!("Could not join {}: {}", name, wrong));
        }
        saved_to_card(c)?;
        Ok(format!("Connected to {}", name))
    })
}

fn forget(id: u32) -> Result<String, String> {
    with_ctrl(|c| {
        c.remove(id)?;
        saved_to_card(c)?;
        Ok("Network forgotten".to_string())
    })
}

fn worker(jobs: Receiver<Job>, out: Sender<Report>) {
    loop {
        let (_, l) = link();
        if out.send(Report::Link(l)).is_err() {
            return;
        }
        match jobs.recv_timeout(POLL) {
            Ok(Job::Scan) => {
                let _ = out.send(Report::Scan(scan()));
            }
            Ok(Job::Join(ssid, how)) => {
                let _ = out.send(Report::Done(join(&ssid, how)));
            }
            Ok(Job::Forget(id)) => {
                let _ = out.send(Report::Done(forget(id)));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

impl Ui {
    /// From `step`: the worker's reports; the worker itself starts with the first look at the menu.
    pub(super) fn wlan_poll(&mut self) {
        let in_menu = self.menu_open && (self.menu == MenuId::Network || self.pick.is_some_and(|p| p.menu == MenuId::Network));
        if in_menu && self.wlan.tx.is_none() && !cfg!(test) {
            let (tx, jobs) = channel();
            let (out, rx) = channel();
            if std::thread::Builder::new().name("wlan".into()).spawn(move || worker(jobs, out)).is_ok() {
                self.wlan.tx = Some(tx);
                self.wlan.rx = Some(rx);
            }
        }
        while let Some(r) = self.wlan.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.wlan_report(r);
        }
    }

    pub(super) fn wlan_report(&mut self, r: Report) {
        match r {
            Report::Link(l) => self.wlan.link = l,
            Report::Scan(r) => {
                self.wlan.busy = None;
                match r {
                    Ok(mut v) => {
                        // strongest first; the last line stays for "Other network"
                        v.truncate(LIST_MAX - 1);
                        self.wlan.nets = v;
                        if self.menu_open && self.menu == MenuId::Network {
                            self.open_picker(Item::WlanNet);
                        }
                    }
                    Err(e) => self.toast(&e),
                }
            }
            Report::Done(r) => {
                self.wlan.busy = None;
                self.toast(r.as_ref().unwrap_or_else(|e| e));
            }
        }
    }

    fn wlan_send(&mut self, j: Job, what: &'static str) {
        let sent = self.wlan.tx.as_ref().is_some_and(|tx| tx.send(j).is_ok());
        if sent {
            self.wlan.busy = Some(what);
            self.toast(&format!("{} ...", what));
        } else {
            self.toast("WLAN is not ready yet");
        }
    }

    /// Why nothing can be done yet, for a toast.
    fn wlan_blocked(&self) -> Option<&'static str> {
        if self.wlan.busy.is_some() {
            Some("WLAN is busy")
        } else if !self.wlan.link.stick && !stick_there() {
            Some("No WLAN stick in the front USB port")
        } else {
            None
        }
    }

    pub(super) fn wlan_scan(&mut self) {
        match self.wlan_blocked() {
            Some(m) => self.toast(m),
            None => self.wlan_send(Job::Scan, "Looking for networks"),
        }
    }

    pub(super) fn wlan_saved_id(&self, ssid: &[u8]) -> Option<u32> {
        self.wlan.link.saved.iter().find(|n| n.ssid == ssid).map(|n| n.id)
    }

    /// The list of the scan, then a line for a network that does not show its name.
    pub(super) fn wlan_net_options(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .wlan
            .nets
            .iter()
            .map(|n| {
                let tag = match n.security {
                    _ if self.wlan_saved_id(&n.ssid).is_some() => " \u{b7} saved".to_string(),
                    Security::Open => " \u{b7} open".to_string(),
                    Security::Psk => String::new(),
                    Security::Other(s) => format!(" \u{b7} {}", s),
                };
                format!("{}  {} dBm{}", short(&n.name()), n.signal, tag)
            })
            .collect();
        v.push("Other network ...".into());
        v
    }

    pub(super) fn wlan_forget_options(&self) -> Vec<String> {
        self.wlan.link.saved.iter().take(LIST_MAX).map(|n| short(&wlan::name(&n.ssid))).collect()
    }

    /// A line of the scan list was picked.
    pub(super) fn wlan_pick(&mut self, k: usize) {
        let Some(n) = self.wlan.nets.get(k).cloned() else {
            self.wlan.osk = Some(Osk::new(OskFor::Name));
            return;
        };
        if let Some(id) = self.wlan_saved_id(&n.ssid) {
            return self.wlan_join(n.ssid, How::Saved(id));
        }
        match n.security {
            Security::Open => self.wlan_join(n.ssid, How::Open),
            Security::Psk => self.wlan.osk = Some(Osk::new(OskFor::Pass(n.ssid))),
            Security::Other(s) => self.toast(&format!("{} networks are not supported", s)),
        }
    }

    fn wlan_join(&mut self, ssid: Vec<u8>, how: How) {
        match self.wlan_blocked() {
            Some(m) => self.toast(m),
            None => self.wlan_send(Job::Join(ssid, how), "Connecting"),
        }
    }

    pub(super) fn wlan_forget_tap(&mut self) {
        if let Some(m) = self.wlan_blocked() {
            self.toast(m);
        } else if self.wlan.link.saved.is_empty() {
            self.toast("No saved networks");
        } else {
            self.open_picker(Item::WlanForgetNet);
        }
    }

    pub(super) fn wlan_forget_pick(&mut self, k: usize) {
        if let Some(n) = self.wlan.link.saved.get(k) {
            let id = n.id;
            self.wlan_send(Job::Forget(id), "Forgetting");
        }
    }

    /// Text rows of the network page.
    pub(super) fn net_value(&self, it: Item) -> String {
        #[cfg(windows)]
        if !self.lan_status.is_empty() {
            match it {
                Item::NetEth => return if self.net.is_empty() { "No active LAN address".into() } else { format!("{} \u{b7} connected", self.net) },
                Item::NetWeb => return self.lan_web_port.map_or_else(|| "Web port unavailable; choose another --web-port".into(), |port| format!("http://{}:{}", if self.net.is_empty() { "localhost" } else { &self.net }, port)),
                Item::NetPorts => return self.lan_status.clone(),
                _ => {}
            }
        }
        let addr = |n: &str| self.net_ifs.iter().find(|i| i.0 == n).map(|i| i.1.as_str());
        let file = |p: &str| std::fs::read_to_string(p).map(|t| t.trim().to_string()).unwrap_or_default();
        match it {
            Item::NetEth => {
                let link = file("/sys/class/net/eth0/carrier") == "1";
                match (addr("eth0"), link) {
                    (Some(a), true) => format!("{} \u{b7} link up", a),
                    (Some(a), false) => format!("{} \u{b7} no cable", a),
                    (None, true) => "link up, no address yet".into(),
                    (None, false) => "no cable".into(),
                }
            }
            Item::NetMac => Some(file("/sys/class/net/eth0/address")).filter(|m| !m.is_empty()).unwrap_or("-".into()),
            Item::NetWeb if self.net.is_empty() => "not connected".into(),
            Item::NetWeb => format!("http://{}:8080", self.net),
            Item::NetPorts => "SCPI 5555, VXI-11, telnet 23".into(),
            _ => {
                let l = &self.wlan.link;
                if !l.stick {
                    "no stick".into()
                } else if !l.ctrl {
                    "stick found, starting".into()
                } else if l.status.connected() {
                    let ip = if l.status.ip.is_empty() { "waiting for an address" } else { l.status.ip.as_str() };
                    format!("{} \u{b7} {}", short(&wlan::name(&l.status.ssid)), ip)
                } else {
                    format!("not connected ({})", l.status.state.to_lowercase())
                }
            }
        }
    }

    /// Dialog lines of WLAN details.
    pub(super) fn wlan_lines(&self) -> Vec<String> {
        let l = &self.wlan.link;
        let mut d = vec!["WLAN".to_string()];
        if !l.stick {
            d.push("No stick in the front USB port".into());
            d.push("RTL8188EU sticks work, see the documentation".into());
        } else if !l.ctrl {
            d.push("Stick found, starting".into());
        } else if l.status.connected() {
            d.push(format!("Connected to {}", wlan::name(&l.status.ssid)));
            if let Some(s) = l.signal {
                d.push(format!("Signal {} dBm, {} MHz", s, l.status.freq));
            }
            d.push(if l.status.ip.is_empty() { "Waiting for an address".to_string() } else { format!("Address {}", l.status.ip) });
        } else {
            d.push(format!("Not connected ({})", l.status.state.to_lowercase()));
        }
        if !l.status.mac.is_empty() {
            d.push(format!("Stick {}", l.status.mac));
        }
        if !l.saved.is_empty() {
            d.push(format!("{} saved network{}", l.saved.len(), if l.saved.len() == 1 { "" } else { "s" }));
        }
        if let Some(b) = self.wlan.busy {
            d.push(format!("{} ...", b));
        }
        d.push("Tap to close".into());
        d
    }
}

/// For pictures of the page in tests.
#[cfg(test)]
pub(super) fn tests_link(ssid: &str, ip: &str) -> Link {
    let status = wlan::parse_status(&format!("wpa_state=COMPLETED\nssid={}\nip_address={}\n", ssid, ip));
    Link { stick: true, ctrl: true, status, ..Link::default() }
}

// ------------------------------------------------------------------ on-screen keyboard

/// What the keyboard is asking for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum OskFor {
    /// the name of a network that does not show it
    Name,
    /// the password of this network
    Pass(Vec<u8>),
    /// the name of a channel
    Label(usize),
    /// the name of a reference
    RefLabel(usize),
    /// the formula of a math channel
    MathExpr(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Key {
    Ch(char),
    Shift,
    Symbols,
    Del,
    /// the insertion point one character back or on
    Left,
    Right,
    Show,
    Cancel,
    Ok,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum Layer {
    #[default]
    Lower,
    Upper,
    Symbols,
}

#[derive(Clone, Debug)]
pub(super) struct Osk {
    what: OskFor,
    pub(super) text: String,
    layer: Layer,
    show: bool,
    /// key chosen with a knob
    focus: usize,
    /// insertion point, in characters from the start (clamped where it is used: tests set `text` directly)
    cursor: usize,
}

/// Longest channel name: it has to fit a narrow card.
const LABEL_MAX: usize = 10;

const KEY_H: i32 = 76;
const KEY_GAP: i32 = 6;
const KEYS_Y: i32 = 188;
const KEYS_W: i32 = W - 24;
/// The entry field, and the part of it the text stands in.
const FIELD: Rect = Rect::new(12, 70, W - 24, 60);
const FIELD_PAD: i32 = 12;

/// Evenly wide keys of one row, with the row centred.
fn row(out: &mut Vec<(Rect, Key)>, y: i32, keys: &[(Key, i32)]) {
    let units: i32 = keys.iter().map(|k| k.1).sum();
    let u = (KEYS_W + KEY_GAP) / 10;
    let mut x = (W - units * u + KEY_GAP) / 2;
    for (k, w) in keys {
        out.push((Rect::new(x, y, w * u - KEY_GAP, KEY_H), *k));
        x += w * u;
    }
}

fn chars(s: &str) -> Vec<(Key, i32)> {
    s.chars().map(|c| (Key::Ch(c), 1)).collect()
}

impl Osk {
    /// The keyboard for the name of channel `c`, with its name so far.
    pub(super) fn for_label(c: usize, now: &str) -> Osk {
        let mut o = Osk::new(OskFor::Label(c));
        o.text = now.to_string();
        o.cursor = o.text.chars().count();
        o
    }

    /// The keyboard for the formula of math channel `k`; the symbols are one layer up.
    pub(super) fn for_expr(k: usize, now: &str) -> Osk {
        let mut o = Osk::new(OskFor::MathExpr(k));
        o.text = now.to_string();
        o.cursor = o.text.chars().count();
        o
    }

    /// The keyboard for the name of reference `i`.
    pub(super) fn for_ref(i: usize, now: &str) -> Osk {
        let mut o = Osk::new(OskFor::RefLabel(i));
        o.text = now.to_string();
        o.cursor = o.text.chars().count();
        o
    }

    pub(super) fn new(what: OskFor) -> Osk {
        let show = !matches!(what, OskFor::Pass(_));
        Osk { what, text: String::new(), layer: Layer::Lower, show, focus: 0, cursor: 0 }
    }

    fn max_len(&self) -> usize {
        match self.what {
            OskFor::Name => wlan::SSID_MAX,
            OskFor::Pass(_) => wlan::PASS_LEN.1,
            OskFor::Label(_) | OskFor::RefLabel(_) => LABEL_MAX,
            OskFor::MathExpr(_) => oos_dsp::expr::EXPR_MAX,
        }
    }

    fn keys(&self) -> Vec<(Rect, Key)> {
        let rows: [&str; 4] = match self.layer {
            Layer::Lower => ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm"],
            Layer::Upper => ["1234567890", "QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"],
            Layer::Symbols => ["!@#$%^&*()", "-_=+[]{}\\|", ";:'\",.<>/", "?~`"],
        };
        let y = |i: i32| KEYS_Y + i * (KEY_H + KEY_GAP);
        let mut v = Vec::new();
        row(&mut v, y(0), &chars(rows[0]));
        row(&mut v, y(1), &chars(rows[1]));
        row(&mut v, y(2), &chars(rows[2]));
        let mut r3 = vec![(Key::Shift, 1)];
        r3.extend(chars(rows[3]));
        r3.push((Key::Del, 2));
        row(&mut v, y(3), &r3);
        row(&mut v, y(4), &[(Key::Symbols, 1), (Key::Left, 1), (Key::Ch(' '), 3), (Key::Right, 1), (Key::Show, 1), (Key::Cancel, 1), (Key::Ok, 2)]);
        v
    }

    /// Password, or the name of the network, as it stands on the screen.
    fn shown(&self) -> String {
        if self.show { self.text.clone() } else { "*".repeat(self.text.chars().count()) }
    }

    fn cur(&self) -> usize {
        self.cursor.min(self.text.chars().count())
    }

    /// Byte offset of character `i`.
    fn byte_at(&self, i: usize) -> usize {
        self.text.char_indices().nth(i).map_or(self.text.len(), |(b, _)| b)
    }

    /// The characters of the entry that fit the field, as (first, past the last): the end stays in view, and the
    /// insertion point too when it goes back further, with a few characters before it.
    fn window(&self, f: &Font) -> (usize, usize) {
        let s: Vec<char> = self.shown().chars().collect();
        let fits = |a: usize, b: usize| f.width(&s[a..b].iter().collect::<String>()) <= FIELD.w - 2 * FIELD_PAD - 16;
        let cur = self.cur();
        let mut a = 0;
        while !fits(a, s.len()) {
            a += 1;
        }
        if cur >= a {
            return (a, s.len());
        }
        a = cur.saturating_sub(4);
        let mut b = a;
        while b < s.len() && fits(a, b + 1) {
            b += 1;
        }
        (a, b)
    }

    fn label(&self, k: Key) -> String {
        match k {
            Key::Ch(' ') => "Space".into(),
            Key::Ch(c) => c.to_string(),
            Key::Shift => if self.layer == Layer::Upper { "abc".into() } else { "Shift".into() },
            Key::Symbols => if self.layer == Layer::Symbols { "abc".into() } else { "#+=".into() },
            Key::Del => "Delete".into(),
            // drawn as arrows: the font has none
            Key::Left | Key::Right => String::new(),
            Key::Show => if self.show { "Hide".into() } else { "Show".into() },
            Key::Cancel => "Back".into(),
            Key::Ok => "OK".into(),
        }
    }
}

impl Ui {
    /// A key of the keyboard: a tap, or a knob push on the chosen key.
    fn osk_press(&mut self, k: Key) {
        let Some(o) = self.wlan.osk.as_mut() else { return };
        match k {
            Key::Ch(c) => {
                if o.text.chars().count() < o.max_len() {
                    let (i, b) = (o.cur(), o.byte_at(o.cur()));
                    o.text.insert(b, c);
                    o.cursor = i + 1;
                }
                if o.layer == Layer::Upper {
                    o.layer = Layer::Lower;
                }
            }
            Key::Shift => o.layer = if o.layer == Layer::Lower { Layer::Upper } else { Layer::Lower },
            Key::Symbols => o.layer = if o.layer == Layer::Symbols { Layer::Lower } else { Layer::Symbols },
            Key::Del => {
                let i = o.cur();
                if i > 0 {
                    let b = o.byte_at(i - 1);
                    o.text.remove(b);
                    o.cursor = i - 1;
                }
            }
            Key::Left => o.cursor = o.cur().saturating_sub(1),
            Key::Right => o.cursor = (o.cur() + 1).min(o.text.chars().count()),
            Key::Show => o.show = !o.show,
            Key::Cancel => self.wlan.osk = None,
            Key::Ok => self.osk_done(),
        }
        // a layer with fewer keys: the choice stays on the keyboard
        if let Some(o) = self.wlan.osk.as_mut() {
            o.focus = o.focus.min(o.keys().len() - 1);
        }
    }

    pub(super) fn osk_done(&mut self) {
        let Some(o) = self.wlan.osk.as_ref() else { return };
        match o.what.clone() {
            OskFor::Label(c) => {
                self.m.ch_label[c] = o.text.trim().to_string();
                self.wlan.osk = None;
            }
            OskFor::MathExpr(k) => {
                let t = o.text.trim().to_string();
                // the keyboard stays open on an error: the formula is still there to correct
                if let Some(e) = (!t.is_empty()).then(|| oos_dsp::expr::Expr::parse(&t).err()).flatten() {
                    return self.toast(&e);
                }
                self.m.math_expr[k] = t;
                self.wlan.osk = None;
            }
            OskFor::RefLabel(i) => {
                let t = o.text.trim().to_string();
                if let Some(r) = &mut self.refs[i] {
                    r.label = t;
                }
                self.wlan.osk = None;
            }
            OskFor::Name if o.text.is_empty() => self.toast("Enter the name of the network"),
            OskFor::Name => {
                let ssid = o.text.clone().into_bytes();
                self.wlan.osk = Some(Osk::new(OskFor::Pass(ssid)));
            }
            OskFor::Pass(ssid) => {
                let p = o.text.clone();
                // the keyboard asked for a name first: an empty password means an open network
                let hidden = !self.wlan.nets.iter().any(|n| n.ssid == ssid);
                if p.is_empty() && hidden {
                    self.wlan.osk = None;
                    return self.wlan_join(ssid, How::Open);
                }
                if let Some(m) = wlan::pass_problem(&p) {
                    return self.toast(&m);
                }
                self.wlan.osk = None;
                self.wlan_join(ssid, How::Pass(p));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn osk_type(&mut self, c: char) {
        self.osk_press(Key::Ch(c));
    }

    #[cfg(test)]
    pub(super) fn osk_ok(&mut self) {
        self.osk_press(Key::Ok);
    }

    /// Touch while the keyboard is open.
    pub(super) fn osk_tap(&mut self, x: i32, y: i32) {
        let Some(o) = &self.wlan.osk else { return };
        // a tap into the entry puts the insertion point at the nearest gap between two characters
        if FIELD.contains(x, y) {
            let f = &self.fonts.lb;
            let (a, b) = o.window(f);
            let s: Vec<char> = o.shown().chars().collect();
            let mut gx = FIELD.x + FIELD_PAD;
            let mut at = a;
            for (i, c) in s[a..b].iter().enumerate() {
                let w = f.width(&c.to_string());
                if x < gx + w / 2 {
                    break;
                }
                gx += w;
                at = a + i + 1;
            }
            if let Some(o) = self.wlan.osk.as_mut() {
                o.cursor = at;
            }
            return;
        }
        if let Some((_, k)) = o.keys().into_iter().find(|(r, _)| r.contains(x, y)) {
            self.osk_press(k);
        }
    }

    /// Panel keys while the keyboard is open: any knob moves the choice and takes it with a push, other keys do
    /// nothing.
    pub(super) fn osk_key(&mut self, k: u8) {
        let sub = k & 0x0f;
        if !(0x8..=0xe).contains(&(k >> 4)) || !(0xa..=0xc).contains(&sub) {
            return;
        }
        let Some(o) = self.wlan.osk.as_mut() else { return };
        let keys = o.keys();
        match sub {
            0xc => {
                let key = keys[o.focus.min(keys.len() - 1)].1;
                self.osk_press(key);
            }
            0xa => o.focus = (o.focus + 1) % keys.len(),
            _ => o.focus = (o.focus + keys.len() - 1) % keys.len(),
        }
    }

    pub(super) fn draw_osk(&self, cv: &mut Canvas) {
        let Some(o) = &self.wlan.osk else { return };
        let f = &self.fonts;
        cv.fill(Rect::new(0, 0, W, H), DLG_BG);
        let title = match &o.what {
            OskFor::Name => "Name of the network".to_string(),
            OskFor::Label(c) => format!("Name of CH{} (empty: CH{})", c + 1, c + 1),
            OskFor::RefLabel(i) => format!("Name of R{} (empty: R{})", i + 1, i + 1),
            OskFor::MathExpr(k) => format!("Formula of M{}: CH1-4, t, pi, sin cos tan sqrt abs ln log exp min max", k + 1),
            OskFor::Pass(s) if self.wlan.nets.iter().any(|n| n.ssid == *s) => format!("Password for {}", wlan::name(s)),
            OskFor::Pass(s) => format!("Password for {} (empty: open network)", wlan::name(s)),
        };
        cv.text_in(&f.lb, Rect::new(12, 18, W - 24, 34), 1, &title, ACCENT);
        let r = FIELD;
        cv.round(r, 6, BTN_BORDER, 255);
        cv.round(r.inset(1), 5, BG, 255);
        let (a, b) = o.window(&f.lb);
        let s: Vec<char> = o.shown().chars().collect();
        let t: String = s[a..b].iter().collect();
        let tr = r.inset(FIELD_PAD);
        cv.text_in(&f.lb, tr, 0, &t, TEXT);
        // the insertion point: a bar between two characters
        let cx = tr.x + f.lb.width(&s[a..o.cur().clamp(a, b)].iter().collect::<String>());
        let hh = f.lb.size * 60 / 100;
        cv.fill(Rect::new(cx, tr.y + tr.h / 2 - hh, 2, 2 * hh), ACCENT);
        cv.text_in(&f.s, Rect::new(12, 138, W - 24, 24), 2, &format!("{} / {}", o.text.chars().count(), o.max_len()), DIM);
        for (i, (r, k)) in o.keys().into_iter().enumerate() {
            let c = if k == Key::Ok { ACCENT } else { TEXT };
            btn(cv, r, if i == o.focus { mix(DLG_BG, ACCENT, 60) } else { mix(DLG_BG, c, 24) }, if i == o.focus { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.lb, r, 1, &o.label(k), c);
            if matches!(k, Key::Left | Key::Right) {
                let (mx, my, d) = (r.x + r.w / 2, r.y + r.h / 2, if k == Key::Left { -1 } else { 1 });
                cv.triangle([(mx + d * 10, my), (mx - d * 8, my - 12), (mx - d * 8, my + 12)], c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn net(name: &str, s: Security) -> Net {
        Net { ssid: name.as_bytes().to_vec(), signal: -50, freq: 2437, security: s }
    }

    fn tap_key(ui: &mut Ui, want: Key) {
        let r = ui.wlan.osk.as_ref().unwrap().keys().into_iter().find(|(_, k)| *k == want).unwrap_or_else(|| panic!("no key {:?}", want)).0;
        ui.tap(r.x + 5, r.y + 5);
    }

    /// Through the keys, as a finger would: Shift or the symbols first where the character needs them.
    fn type_text(ui: &mut Ui, t: &str) {
        for c in t.chars() {
            let layer = match c {
                'A'..='Z' => Layer::Upper,
                'a'..='z' | '0'..='9' | ' ' => Layer::Lower,
                _ => Layer::Symbols,
            };
            let now = ui.wlan.osk.as_ref().unwrap().layer;
            if now != layer {
                match (now, layer) {
                    (Layer::Symbols, _) | (_, Layer::Symbols) => tap_key(ui, Key::Symbols),
                    _ => tap_key(ui, Key::Shift),
                }
                if ui.wlan.osk.as_ref().unwrap().layer != layer {
                    tap_key(ui, Key::Shift);
                }
            }
            tap_key(ui, Key::Ch(c));
        }
    }

    #[test]
    fn every_password_character_has_a_key() {
        let o = |l| Osk { what: OskFor::Name, text: String::new(), layer: l, show: true, focus: 0, cursor: 0 };
        let mut seen = std::collections::BTreeSet::new();
        for l in [Layer::Lower, Layer::Upper, Layer::Symbols] {
            for (r, k) in o(l).keys() {
                assert!(r.x >= 0 && r.y >= 0 && r.x + r.w <= W && r.y + r.h <= H, "{:?} {:?}", l, r);
                if let Key::Ch(c) = k {
                    seen.insert(c);
                }
            }
        }
        assert_eq!(seen.into_iter().collect::<String>(), (' '..='~').collect::<String>());
    }

    #[test]
    fn keys_do_not_overlap() {
        let v = Osk::new(OskFor::Name).keys();
        for (i, a) in v.iter().enumerate() {
            for b in &v[i + 1..] {
                assert!(!overlaps(a.0, b.0), "{:?} {:?}", a, b);
            }
        }
    }

    #[test]
    fn typing_a_password() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.wlan.osk = Some(Osk::new(OskFor::Pass(b"Labor".to_vec())));
        type_text(&mut ui, "aB3 #x");
        let o = ui.wlan.osk.as_ref().unwrap();
        assert_eq!(o.text, "aB3 #x");
        assert_eq!(o.shown(), "******");
        // too short: stays open with a message
        let ok = o.keys().into_iter().find(|(_, k)| *k == Key::Ok).unwrap().0;
        ui.tap(ok.x + 5, ok.y + 5);
        assert!(ui.wlan.osk.is_some());
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("8 to 63")));
    }

    #[test]
    fn entry_stops_at_the_limit_and_del_is_safe() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.wlan.osk = Some(Osk::new(OskFor::Name));
        ui.osk_press(Key::Del);
        for _ in 0..40 {
            ui.osk_press(Key::Ch('z'));
        }
        assert_eq!(ui.wlan.osk.as_ref().unwrap().text.len(), wlan::SSID_MAX);
    }

    #[test]
    fn arrows_fix_a_typo_without_deleting_the_rest() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.wlan.osk = Some(Osk::new(OskFor::Pass(b"Labor".to_vec())));
        type_text(&mut ui, "abxdef");
        // back to the x, out with it, the c in its place
        for _ in 0..3 {
            tap_key(&mut ui, Key::Left);
        }
        tap_key(&mut ui, Key::Del);
        type_text(&mut ui, "c");
        assert_eq!(ui.wlan.osk.as_ref().unwrap().text, "abcdef");
        // the ends hold
        for _ in 0..10 {
            tap_key(&mut ui, Key::Left);
        }
        tap_key(&mut ui, Key::Del);
        type_text(&mut ui, "0");
        for _ in 0..10 {
            tap_key(&mut ui, Key::Right);
        }
        type_text(&mut ui, "9");
        assert_eq!(ui.wlan.osk.as_ref().unwrap().text, "0abcdef9");
        // a full entry takes nothing in the middle either
        let o = ui.wlan.osk.as_mut().unwrap();
        o.text = "z".repeat(o.max_len());
        o.cursor = 3;
        tap_key(&mut ui, Key::Ch('a'));
        assert!(!ui.wlan.osk.as_ref().unwrap().text.contains('a'));
    }

    #[test]
    fn a_tap_into_the_entry_moves_the_insertion_point() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.wlan.osk = Some(Osk::for_label(0, "abcd"));
        assert_eq!(ui.wlan.osk.as_ref().unwrap().cur(), 4);
        let f = &ui.fonts.lb;
        let x = FIELD.x + FIELD_PAD + f.width("ab") + 1;
        ui.tap(x, FIELD.y + FIELD.h / 2);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().cur(), 2);
        ui.tap(FIELD.x + 2, FIELD.y + FIELD.h / 2);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().cur(), 0);
        ui.tap(FIELD.x + FIELD.w - 3, FIELD.y + FIELD.h / 2);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().cur(), 4);
    }

    #[test]
    fn a_long_entry_keeps_the_insertion_point_in_view() {
        let ui = Ui::new(Source::Demo { t: 0.0 });
        let f = &ui.fonts.lb;
        let mut o = Osk::new(OskFor::Pass(b"Labor".to_vec()));
        o.show = true;
        o.text = (0..63).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        for cur in [63, 40, 5, 0] {
            o.cursor = cur;
            let (a, b) = o.window(f);
            assert!(a <= cur && cur <= b, "{} not in {}..{}", cur, a, b);
            let t: String = o.text.chars().skip(a).take(b - a).collect();
            assert!(f.width(&t) <= FIELD.w - 2 * FIELD_PAD, "{}", cur);
        }
        // at the end: the end of the entry is what shows, as before
        o.cursor = 63;
        assert_eq!(o.window(f).1, 63);
    }

    #[test]
    fn knobs_reach_every_key() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.wlan.osk = Some(Osk::new(OskFor::Name));
        let n = ui.wlan.osk.as_ref().unwrap().keys().len();
        for _ in 0..n {
            ui.key(0xba);
        }
        assert_eq!(ui.wlan.osk.as_ref().unwrap().focus, 0);
        ui.key(0xbb);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().focus, n - 1);
        // the last key is OK; an empty name gets a message, not a crash
        ui.key(0xbc);
        assert!(ui.wlan.osk.is_some());
        // other panel keys do nothing here
        ui.key(0x0f);
        assert!(!ui.menu_open);
        // the symbols layer has fewer keys: the choice follows
        let o = ui.wlan.osk.as_mut().unwrap();
        o.focus = n - 1;
        o.layer = Layer::Symbols;
        let s = o.keys().len();
        ui.osk_press(Key::Show);
        assert!(ui.wlan.osk.as_ref().unwrap().focus < s);
    }

    #[test]
    fn scan_list_and_the_way_to_each_network() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.open_menu(MenuId::Network);
        ui.wlan.link.stick = true;
        ui.wlan.link.saved = vec![Saved { id: 7, ssid: b"Bekannt".to_vec(), temp_disabled: false, disabled: false }];
        let list = vec![net("Labor", Security::Psk), net("Gast", Security::Open), net("Neu", Security::Other("WPA3")), net("Bekannt", Security::Psk)];
        ui.wlan_report(Report::Scan(Ok(list)));
        assert_eq!(ui.menu, MenuId::Pick);
        let o = ui.options(Item::WlanNet);
        assert_eq!(o.len(), 5);
        assert!(o[1].ends_with("open") && o[2].ends_with("WPA3") && o[3].ends_with("saved") && o[4].starts_with("Other"));
        // password network: the keyboard, and the menu is back underneath
        ui.activate(Item::PickOpt(0));
        assert_eq!(ui.menu, MenuId::Network);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().what, OskFor::Pass(b"Labor".to_vec()));
        ui.wlan.osk = None;
        // not supported: says so
        ui.wlan_pick(2);
        assert!(ui.wlan.osk.is_none());
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("WPA3")));
        // a hidden network: name first, then the password
        ui.wlan_pick(4);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().what, OskFor::Name);
        type_text(&mut ui, "Versteckt");
        ui.osk_press(Key::Ok);
        assert_eq!(ui.wlan.osk.as_ref().unwrap().what, OskFor::Pass(b"Versteckt".to_vec()));
        // a worker that is not running (host): a message, no crash
        ui.osk_press(Key::Ok);
        assert!(ui.wlan.osk.is_none());
    }

    #[test]
    fn forget_needs_a_saved_network() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.open_menu(MenuId::Network);
        ui.wlan.link.stick = true;
        ui.wlan_forget_tap();
        assert_eq!(ui.menu, MenuId::Network);
        ui.wlan.link.saved = vec![Saved { id: 1, ssid: b"A".to_vec(), temp_disabled: false, disabled: false }];
        ui.wlan_forget_tap();
        assert_eq!(ui.menu, MenuId::Pick);
        assert_eq!(ui.options(Item::WlanForgetNet), ["A"]);
    }

    #[test]
    fn network_page_says_what_is_there() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.tap(950, 20);
        assert!(ui.menu_open && ui.menu == MenuId::Network);
        assert_eq!(ui.net_value(Item::NetWeb), "not connected");
        assert_eq!(ui.net_value(Item::NetWlan), "no stick");
        (ui.net, ui.net_if) = ("192.168.177.64".into(), "eth0".into());
        ui.net_ifs = vec![("eth0".into(), "192.168.177.64".into())];
        assert_eq!(ui.net_value(Item::NetWeb), "http://192.168.177.64:8080");
        assert!(ui.net_value(Item::NetEth).starts_with("192.168.177.64"));
        ui.wlan.link = Link { stick: true, ctrl: true, ..Link::default() };
        ui.wlan.link.status = wlan::parse_status("wpa_state=COMPLETED\nssid=Ein sehr langer Netzname mit 32 Z\nip_address=192.168.100.200\n");
        // the longest line fits its row
        let d = ui.dlg_layout().unwrap();
        let row = d.rows.iter().find(|r| r.it == Item::NetWlan).unwrap();
        assert!(ui.fonts.m.width(&ui.net_value(Item::NetWlan)) <= row.ctrl.w, "{}", ui.net_value(Item::NetWlan));
        ui.tap(950, 20);
        assert!(!ui.menu_open);
    }

    #[test]
    fn status_lines() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert!(ui.wlan_lines()[1].starts_with("No stick"));
        ui.wlan.link = Link { stick: true, ctrl: true, signal: Some(-55), ..Link::default() };
        ui.wlan.link.status = wlan::parse_status("wpa_state=COMPLETED\nssid=Labor\nip_address=10.0.0.7\nfreq=2437\naddress=00:11:22:33:44:55\n");
        let l = ui.wlan_lines();
        assert_eq!(l[1], "Connected to Labor");
        assert!(l.iter().any(|x| x.contains("10.0.0.7")) && l.iter().any(|x| x.contains("-55 dBm")));
    }

    /// Whatever arrives on the touch panel and the knobs, with hostile names on the list.
    #[test]
    fn random_input() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let mut cv = Canvas::new(W, H);
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        ui.wlan.link.stick = true;
        let mut names = vec![net("Küche 🏠", Security::Psk), net("Gast", Security::Open)];
        names.push(Net { ssid: vec![0xff; 32], signal: -90, freq: 2412, security: Security::Psk });
        names.push(Net { ssid: b"\n\t\x1b[2J%s%n".to_vec(), signal: -91, freq: 2412, security: Security::Other("WEP") });
        for i in 0..40_000 {
            if ui.wlan.osk.is_none() {
                ui.open_menu(MenuId::Network);
                ui.wlan_report(Report::Scan(Ok(names.clone())));
                let k = next() as usize % (names.len() + 1);
                ui.activate(Item::PickOpt(k));
                ui.wlan.busy = None;
            }
            match next() % 5 {
                0 => ui.key([0xba, 0xbb, 0xbc, 0xaa, 0xab, 0xac, 0x0f, 0x08][next() as usize % 8]),
                _ => ui.tap((next() % W as u64) as i32, (next() % H as u64) as i32),
            }
            ui.dialog = None;
            ui.power_dlg = false;
            if i % 500 == 0 {
                ui.render(&mut cv);
            }
        }
    }

    /// Against a real wpa_supplicant, `oos-wlan` and access points on simulated radios: tools/wlan-sim.sh runs this
    /// inside a virtual machine. Nothing here runs in a normal `cargo test`.
    #[test]
    #[ignore]
    fn live_radio() {
        let conf = "/oos/wlan/wpa_supplicant.conf";
        let wait = |what: &str, secs: u64, f: &dyn Fn() -> bool| {
            let end = Instant::now() + Duration::from_secs(secs);
            while !f() {
                assert!(Instant::now() < end, "timed out: {}", what);
                std::thread::sleep(Duration::from_millis(250));
            }
            println!("ok: {}", what);
        };
        wait("control socket", 40, &|| link().1.ctrl);

        let nets = scan().unwrap();
        println!("scan: {:?}", nets);
        let find = |n: &str| nets.iter().find(|x| x.ssid == n.as_bytes()).unwrap_or_else(|| panic!("{} not in the scan", n)).clone();
        assert_eq!(find("OOS-Test").security, Security::Psk);
        assert_eq!(find("OOS-Open").security, Security::Open);
        assert!(!nets.iter().any(|x| x.ssid == b"OOS-Hidden"));

        let e = join(b"OOS-Test", How::Pass("wrong-password".into())).unwrap_err();
        println!("wrong password: {}", e);
        assert!(e.contains("wrong password"), "{}", e);
        assert!(link().1.saved.is_empty(), "a failed join must not stay");
        assert!(!std::fs::read_to_string(conf).unwrap().contains("network="), "a failed join must not be saved");

        // a password with the characters that would break a quoted file
        assert_eq!(join(b"OOS-Test", How::Pass("se\"cr#t p\\ss".into())).unwrap(), "Connected to OOS-Test");
        let st = link().1;
        assert!(st.status.connected() && st.signal.is_some(), "{:?}", st);
        wait("address over DHCP", 30, &|| link().1.status.ip.starts_with("192.168.50."));
        let file = std::fs::read_to_string(conf).unwrap();
        println!("{}", file.replace(&wlan::hex(&wlan::psk("se\"cr#t p\\ss", b"OOS-Test")), "<the key>"));
        assert!(file.contains(&wlan::hex(&wlan::psk("se\"cr#t p\\ss", b"OOS-Test"))));
        assert!(!file.contains("cr#t"), "the password itself must not be stored");

        // a second network, then back to the first without a password
        assert_eq!(join(b"OOS-Open", How::Open).unwrap(), "Connected to OOS-Open");
        let l = link().1;
        assert_eq!(l.saved.len(), 2);
        assert!(l.saved.iter().all(|n| !n.disabled), "{:?}", l.saved);
        let id = l.saved.iter().find(|n| n.ssid == b"OOS-Test").unwrap().id;
        assert_eq!(join(b"OOS-Test", How::Saved(id)).unwrap(), "Connected to OOS-Test");

        // a hidden network by name
        assert_eq!(join(b"OOS-Hidden", How::Pass("hidden-pass".into())).unwrap(), "Connected to OOS-Hidden");
        assert_eq!(link().1.saved.len(), 3);
        // this one has no DHCP server: oos-wlan has to go on all the same, which the steps below need
        wait("oos-wlan on the network without DHCP", 15, &|| {
            std::fs::read_to_string("/tmp/oos-wlan.log").is_ok_and(|t| t.contains("joined OOS-Hidden"))
        });
        std::thread::sleep(Duration::from_secs(5));

        // forgetting the current one
        let id = link().1.saved.iter().find(|n| n.ssid == b"OOS-Hidden").unwrap().id;
        forget(id).unwrap();
        assert_eq!(link().1.saved.len(), 2);
        assert!(!std::fs::read_to_string(conf).unwrap().contains("hidden"));
        wait("back on a saved network by itself", 40, &|| link().1.status.connected());

        // wpa_supplicant dies: oos-wlan starts it again and the saved network comes back
        let pid = std::fs::read_to_string("/run/wpa_supplicant.pid").unwrap();
        std::process::Command::new("kill").args(["-9", pid.trim()]).status().unwrap();
        let _ = std::fs::remove_file(wlan::CTRL);
        std::thread::sleep(Duration::from_secs(3));
        wait("restart and rejoin", 40, &|| link().1.status.connected());
        wait("address after the restart", 30, &|| link().1.status.ip.starts_with("192.168.50."));
        println!("LIVE RADIO TEST PASSED");
    }

    #[test]
    fn pictures() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
        let _ = std::fs::create_dir_all(&dir);
        let shot = |ui: &mut Ui, n: &str| {
            let mut cv = Canvas::new(W, H);
            ui.render(&mut cv);
            let _ = crate::store::write(&dir.join(n), &crate::store::png_of(&cv));
        };
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.wlan.link.stick = true;
        ui.open_menu(MenuId::Utility);
        shot(&mut ui, "wlan-0-system.png");
        ui.open_menu(MenuId::Network);
        shot(&mut ui, "wlan-1-menu.png");
        let list = vec![net("Labor", Security::Psk), net("Gast", Security::Open), net("Ein sehr langer Netzname mit 32 Zeichen", Security::Psk), net("Neu", Security::Other("WPA3"))];
        let mut many = list.clone();
        for i in 0..30 {
            many.push(net(&format!("Nachbar {}", i), Security::Psk));
        }
        ui.wlan_report(Report::Scan(Ok(many)));
        assert_eq!(ui.options(Item::WlanNet).len(), LIST_MAX);
        let d = ui.dlg_layout().unwrap();
        assert!(d.r.y >= 0 && d.r.y + d.r.h <= H && d.rows.iter().all(|r| r.ctrl.y + r.ctrl.h <= d.r.y + d.r.h), "{:?}", d.r);
        shot(&mut ui, "wlan-2-many.png");
        ui.close_picker();
        ui.wlan_report(Report::Scan(Ok(list)));
        shot(&mut ui, "wlan-2-list.png");
        ui.close_picker();
        ui.wlan.osk = Some(Osk::new(OskFor::Pass(b"Labor".to_vec())));
        ui.wlan.nets.truncate(1);
        type_text(&mut ui, "geheim");
        shot(&mut ui, "wlan-3-keys.png");
        ui.osk_press(Key::Symbols);
        ui.osk_press(Key::Show);
        shot(&mut ui, "wlan-4-symbols.png");
    }
}
