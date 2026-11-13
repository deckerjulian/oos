// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Remote services: which of the network services run, each switch next to the line that says
//! where to reach it. The choice is a setting of the scope (`/oos/system.conf`, `remote.*`); `main.rs` opens and closes
//! the ports from it, `crate::ssh` the SSH server, whose page is ui/ssh.rs. Web, SCPI and VXI-11 are on by default,
//! SSH is off unless the image or the user switches it on. This fork has no telnet.
//!
//! The last row: the mDNS responder (`crate::mdns`, on by default: it answers on the local link only) next to the
//! scope's name on the network, `dho924s-204527.local` (`net.name`). The name is also the kernel's host name, which
//! the DHCP client sends (rcS, oos-wlan).

use super::*;
use crate::mdns;

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
            Item::NetWeb,
            Item::RemScpiInfo,
            Item::RemVxiInfo,
            Item::Open(MenuId::Ssh),
            Item::RemName,
        ]
    }

    pub(super) fn remote_kind(it: Item) -> Option<Kind> {
        match it {
            Item::RemWeb | Item::RemScpi | Item::RemVxi | Item::RemSsh | Item::RemMdns | Item::RemName => Some(Kind::Choice),
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
        // the fork's SSH (docs/network.md 7): announced only while oos-ssh would listen (it needs `net.allow` as well)
        add(s.remote_ssh && crate::ssh::listens(), "_ssh._tcp", 22, &[]);
        mdns::Config { on: s.remote_mdns, instance: mdns::instance_name(&p.caps.name, p.serial.as_deref(), &name), name, services }
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

    /// Rows that are only text.
    pub(super) fn remote_text(it: Item) -> bool {
        matches!(it, Item::NetWeb | Item::RemScpiInfo | Item::RemVxiInfo) || Self::ssh_text(it)
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
            Item::NetWeb if !s.remote_web => "off".into(),
            Item::NetWeb if self.net.is_empty() => "not connected".into(),
            Item::NetWeb => format!("http://{}:8080", self.net),
            Item::RemScpiInfo => port(s.remote_scpi, "5555 (TCP)".into()),
            Item::RemVxiInfo => port(s.remote_vxi11, format!("111 and {} (LXI)", crate::vxi11::CORE_PORT)),
            _ => return self.ssh_value(it),
        })
    }

    pub(super) fn remote_options(&self, it: Item) -> Option<Vec<String>> {
        if let Some(o) = self.ssh_options(it) {
            return Some(o);
        }
        match it {
            Item::RemName => Some(self.name_options()),
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
            // the name changes in its picker only
            Item::RemName => return true,
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

    #[test]
    fn switches_stand_next_to_what_they_open() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.open_menu(MenuId::Remote);
        let d = u.dlg_layout().unwrap();
        // two columns of five: each switch has its address, port, settings or name on the same row
        for (sw, info) in [(Item::RemWeb, Item::NetWeb), (Item::RemScpi, Item::RemScpiInfo), (Item::RemVxi, Item::RemVxiInfo), (Item::RemSsh, Item::Open(MenuId::Ssh)), (Item::RemMdns, Item::RemName)] {
            let a = d.rows.iter().find(|r| r.it == sw).unwrap().ctrl;
            let b = d.rows.iter().find(|r| r.it == info).unwrap().ctrl;
            assert!(a.y == b.y && a.x < b.x, "{:?} {:?}", sw, info);
        }
    }

    #[test]
    fn a_service_switched_off_says_so() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
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
}
