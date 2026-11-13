// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Remote services: which of the network services run, each switch next to the line that says
//! where to reach it. The choice is a setting of the scope (`/oos/system.conf`, `remote.*`); `main.rs` opens and closes
//! the ports from it, `crate::ssh` the SSH server, whose page is ui/ssh.rs. Web, SCPI and VXI-11 are on by default,
//! SSH is off unless the image or the user switches it on. This fork has no telnet.

use super::*;

impl Ui {
    /// Switches in the left column, what they open in the right one, row by row.
    pub(super) fn remote_items(&self) -> Vec<Item> {
        vec![Item::RemWeb, Item::RemScpi, Item::RemVxi, Item::RemSsh, Item::NetWeb, Item::RemScpiInfo, Item::RemVxiInfo, Item::Open(MenuId::Ssh)]
    }

    pub(super) fn remote_kind(it: Item) -> Option<Kind> {
        match it {
            Item::RemWeb | Item::RemScpi | Item::RemVxi | Item::RemSsh => Some(Kind::Choice),
            _ => Self::ssh_kind(it),
        }
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
        Self::remote_kind(it).filter(|k| *k == Kind::Choice).map(|_| vec!["Off".into(), "On".into()])
    }

    pub(super) fn remote_adjust(&mut self, it: Item, d: i32) -> bool {
        let s = &mut self.sys;
        let (on, what) = match it {
            Item::RemWeb => (&mut s.remote_web, "Web remote"),
            Item::RemScpi => (&mut s.remote_scpi, "SCPI"),
            Item::RemVxi => (&mut s.remote_vxi11, "VXI-11"),
            Item::RemSsh => (&mut s.remote_ssh, "SSH"),
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
        // two columns of four: each switch has its address or port on the same row
        for (sw, info) in [(Item::RemWeb, Item::NetWeb), (Item::RemScpi, Item::RemScpiInfo), (Item::RemVxi, Item::RemVxiInfo), (Item::RemSsh, Item::Open(MenuId::Ssh))] {
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
}
