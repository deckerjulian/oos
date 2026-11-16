// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Remote services: which of the network services run, each switch next to the line that says
//! where to reach it. The choice is a setting of the scope (`/oos/system.conf`, `remote.*`); `main.rs` opens and closes
//! the ports from it, `crate::ssh` the SSH server, whose page is ui/ssh.rs. Web, SCPI and VXI-11 are on by default,
//! telnet (a shell without password, for bring-up on the bench LAN) is on by default as before; SSH, the
//! authenticated shell, is off unless the image or the user switches it on.
//!
//! The last row: the mDNS responder (`crate::mdns`, on by default: it answers on the local link only) next to the
//! scope's name on the network, `dho924s-204527.local` (`net.name`). The name is also the kernel's host name, which
//! the DHCP client sends (rcS, oos-wlan).
//!
//! Below it: Allowed, the address prefixes that may reach the scope (`net.allow`, docs/network.md 6), and Firewall,
//! whether that list is in force. rcS builds the firewall from the key and oos-ui its address list (`crate::netacl`),
//! both at the start only, so a change applies at the next start. An entry is checked by `netacl::canonical` and
//! refused whole when a word is not a prefix: nothing is written then.

use super::*;
use crate::{mdns, netacl};

/// Remote services > Allowed.
#[derive(Default)]
pub(super) struct RemoteUi {
    /// `net.allow` at the start (`load_system`): what the firewall and oos-ui use until the next one
    pub allow_start: String,
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
            Item::RemTelnet,
            Item::RemSsh,
            Item::RemMdns,
            Item::RemAllow,
            Item::NetWeb,
            Item::RemScpiInfo,
            Item::RemVxiInfo,
            Item::RemTelnetInfo,
            Item::Open(MenuId::Ssh),
            Item::RemName,
            Item::RemFirewall,
        ]
    }

    pub(super) fn remote_kind(it: Item) -> Option<Kind> {
        match it {
            Item::RemWeb | Item::RemScpi | Item::RemVxi | Item::RemTelnet | Item::RemSsh | Item::RemMdns | Item::RemName | Item::RemAllow => Some(Kind::Choice),
            _ => Self::ssh_kind(it),
        }
    }

    /// The name from model and serial number (`dho924s-204527`).
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
                services.push(mdns::Service { kind: kind.into(), port, txt: txt.iter().map(|t| t.to_string()).collect() });
            }
        };
        add(s.remote_web, "_http._tcp", web_port, &["path=/"]);
        add(s.remote_scpi, "_scpi-raw._tcp", scpi_port, &[]);
        add(s.remote_vxi11, "_vxi-11._tcp", 111, &[]);
        // SSH (docs/network.md 7): announced only while oos-ssh would listen (it needs `net.allow` as well)
        add(s.remote_ssh && crate::ssh::listens(), "_ssh._tcp", 22, &[]);
        mdns::Config { on: s.remote_mdns, instance: mdns::instance_name(&p.caps.name, p.serial.as_deref(), &name), name, services, v6: crate::net6::enabled() }
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
        let room = self.dlg_layout().and_then(|d| d.rows.iter().find(|r| r.it == Item::RemName).map(|r| Self::value_parts(r.ctrl).1.w - 44));
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

    /// Writes `net.allow` (a value `netacl::canonical` gave; empty: the settings' line goes, the image default applies).
    fn set_allow(&mut self, v: &str) {
        self.sys.set_net_allow((!v.is_empty()).then_some(v));
        self.sys_save();
        let now = self.sys.net_allow();
        let what = if now.is_empty() { "No address list: everyone".to_string() } else { format!("Allowed: {}", now) };
        let when = if now == self.remote.allow_start { "" } else { ", from the next start" };
        self.toast(&format!("{}{}", what, when));
    }

    /// Allowed's row: the list in use, cut to the room of its row.
    fn allow_value(&self) -> String {
        let now = self.sys.net_allow();
        if now.is_empty() {
            return "everyone".into();
        }
        let room = self.dlg_layout().and_then(|d| d.rows.iter().find(|r| r.it == Item::RemAllow).map(|r| Self::value_parts(r.ctrl).1.w - 44));
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

    /// Firewall's row: the list of the start in force, or a change that waits for the next start.
    fn firewall_value(&self) -> String {
        let now = self.sys.net_allow();
        if now != self.remote.allow_start {
            "from the next start".into()
        } else if now.is_empty() {
            "off: everyone".into()
        } else {
            "on".into()
        }
    }

    /// Rows that are only text.
    pub(super) fn remote_text(it: Item) -> bool {
        matches!(it, Item::NetWeb | Item::RemScpiInfo | Item::RemVxiInfo | Item::RemTelnetInfo | Item::RemFirewall) || Self::ssh_text(it)
    }

    pub(super) fn remote_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Remote) => "Remote services",
            Item::RemWeb => "Web remote",
            Item::RemScpi => "SCPI",
            Item::RemVxi => "VXI-11",
            Item::RemTelnet => "telnet",
            Item::RemSsh => "SSH",
            Item::RemMdns => "mDNS",
            Item::RemName => "Name",
            Item::RemAllow => "Allowed",
            Item::RemFirewall => "Firewall",
            Item::NetWeb => "Address",
            Item::RemScpiInfo | Item::RemVxiInfo | Item::RemTelnetInfo => "Port",
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
            Item::RemTelnet => onoff(s.remote_telnet),
            Item::RemSsh => onoff(s.remote_ssh),
            Item::RemMdns => onoff(s.remote_mdns),
            Item::RemName => self.name_value(),
            Item::RemAllow => self.allow_value(),
            Item::RemFirewall => self.firewall_value(),
            Item::NetWeb if !s.remote_web => "off".into(),
            Item::NetWeb if self.net.is_empty() => "not connected".into(),
            Item::NetWeb => format!("http://{}:8080", self.net),
            Item::RemScpiInfo => port(s.remote_scpi, "5555 (TCP)".into()),
            Item::RemVxiInfo => port(s.remote_vxi11, format!("111 and {} (LXI)", crate::vxi11::CORE_PORT)),
            Item::RemTelnetInfo => port(s.remote_telnet, "23, a shell without password".into()),
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
            Item::RemTelnet => (&mut s.remote_telnet, "telnet"),
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
            (Item::RemTelnet, false) => self.toast("telnet off: no remote shell without password"),
            (Item::RemSsh, false) => self.toast("SSH off: its sessions end"),
            _ => self.toast(&format!("{} {}", what, if now_on { "on" } else { "off" })),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_stand_next_to_what_they_open() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.open_menu(MenuId::Remote);
        let d = u.dlg_layout().unwrap();
        // two columns of seven: each switch has its address, port, settings or name on the same row, the address list
        // its firewall
        for (sw, info) in [(Item::RemWeb, Item::NetWeb), (Item::RemScpi, Item::RemScpiInfo), (Item::RemVxi, Item::RemVxiInfo), (Item::RemTelnet, Item::RemTelnetInfo), (Item::RemSsh, Item::Open(MenuId::Ssh)), (Item::RemMdns, Item::RemName), (Item::RemAllow, Item::RemFirewall)] {
            let a = d.rows.iter().find(|r| r.it == sw).unwrap().ctrl;
            let b = d.rows.iter().find(|r| r.it == info).unwrap().ctrl;
            assert!(a.y == b.y && a.x < b.x, "{:?} {:?}", sw, info);
        }
    }

    #[test]
    fn a_service_switched_off_says_so() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        assert!(u.sys.remote_web && u.sys.remote_scpi && u.sys.remote_vxi11 && u.sys.remote_telnet, "on by default");
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
        let mut u = Ui::new(Source::Demo { t: 0.0 });
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
    // shown as in force before the start that loads it
    #[test]
    fn the_address_list_from_the_screen() {
        let dir = std::env::temp_dir().join(format!("oos-ui-allow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("system.conf");
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.sys_path = Some(p.clone());
        u.open_menu(MenuId::Remote);
        // the row is there, with no list: everyone, no firewall
        assert!(u.dlg_layout().unwrap().rows.iter().any(|r| r.it == Item::RemAllow));
        assert_eq!((u.value(Item::RemAllow).as_str(), u.value(Item::RemFirewall).as_str()), ("everyone", "off: everyone"));
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
        // a good one is written in its plain form, and waits for the next start
        u.wlan.osk.as_mut().unwrap().text = "192.0.2.77/24, fd00::/8".into();
        u.osk_ok();
        assert!(u.wlan.osk.is_none());
        assert_eq!(u.toast.as_ref().unwrap().0, "Allowed: 192.0.2.0/24 fd00::/8, from the next start");
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(netacl::value(&text, "net.allow"), Some("192.0.2.0/24 fd00::/8"));
        let (acl, bad) = netacl::Acl::from_texts(None, Some(&text));
        assert!(bad.is_empty() && acl.permits("192.0.2.9".parse().unwrap()) && !acl.permits("198.51.100.1".parse().unwrap()));
        assert_eq!(crate::sysconf::SysConf::load(&p).net_allow(), "192.0.2.0/24 fd00::/8");
        assert_eq!(u.value(Item::RemAllow), "192.0.2.0/24 fd00::/8");
        assert_eq!(u.value(Item::RemFirewall), "from the next start");
        // after a start with it: in force
        let mut v = Ui::new(Source::Demo { t: 0.0 });
        v.sys = crate::sysconf::SysConf::load(&p);
        v.sys_path = Some(p.clone());
        // as `load_system` takes it at the start (that also sets the backlight, which a host test must not)
        v.remote.allow_start = v.sys.net_allow();
        assert_eq!(v.value(Item::RemFirewall), "on");
        // the list in use (cut as a name is) and the network are offered; the network sets the list from the picker
        v.remote.lans = vec!["198.51.100.0/24".into(), "203.0.113.0/24".into()];
        assert_eq!(v.options(Item::RemAllow), vec!["192.0.2.0/24 fd00\u{2026}".to_string(), "198.51.100.0/24".into(), "203.0.113.0/24".into(), OTHER.into()]);
        v.open_menu(MenuId::Remote);
        v.open_picker(Item::RemAllow);
        v.activate(Item::PickOpt(1));
        assert_eq!(v.sys.net_allow(), "198.51.100.0/24");
        // the knob does not change it
        v.adjust(Item::RemAllow, 1);
        assert_eq!(v.sys.net_allow(), "198.51.100.0/24");
        // an empty entry removes the line: no list (no image default on the host)
        v.allow_pick(v.options(Item::RemAllow).len() - 1);
        v.wlan.osk.as_mut().unwrap().text.clear();
        v.osk_ok();
        assert!(v.wlan.osk.is_none());
        assert_eq!(v.toast.as_ref().unwrap().0, "No address list: everyone, from the next start");
        assert_eq!(netacl::value(&std::fs::read_to_string(&p).unwrap(), "net.allow"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
