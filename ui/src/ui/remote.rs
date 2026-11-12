// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System > Network > Remote services: which of the network services run, each switch next to the line that says
//! where to reach it. The choice is a setting of the scope (`/oos/system.conf`, `remote.*`); `main.rs` opens and closes
//! the ports from it, `crate::remote` the telnet shell. All are on by default.

use super::*;

impl Ui {
    /// Switches in the left column, what they open in the right one, row by row.
    pub(super) fn remote_items(&self) -> Vec<Item> {
        vec![Item::RemWeb, Item::RemScpi, Item::RemVxi, Item::RemTelnet, Item::NetWeb, Item::RemScpiInfo, Item::RemVxiInfo, Item::RemTelnetInfo]
    }

    pub(super) fn remote_kind(it: Item) -> Option<Kind> {
        match it {
            Item::RemWeb | Item::RemScpi | Item::RemVxi | Item::RemTelnet => Some(Kind::Choice),
            _ => None,
        }
    }

    /// Rows that are only text.
    pub(super) fn remote_text(it: Item) -> bool {
        matches!(it, Item::NetWeb | Item::RemScpiInfo | Item::RemVxiInfo | Item::RemTelnetInfo)
    }

    pub(super) fn remote_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Remote) => "Remote services",
            Item::RemWeb => "Web remote",
            Item::RemScpi => "SCPI",
            Item::RemVxi => "VXI-11",
            Item::RemTelnet => "telnet",
            Item::NetWeb => "Address",
            Item::RemScpiInfo | Item::RemVxiInfo | Item::RemTelnetInfo => "Port",
            _ => return None,
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
            Item::NetWeb if !s.remote_web => "off".into(),
            Item::NetWeb if self.net.is_empty() => "not connected".into(),
            Item::NetWeb => format!("http://{}:8080", self.net),
            Item::RemScpiInfo => port(s.remote_scpi, "5555 (TCP)".into()),
            Item::RemVxiInfo => port(s.remote_vxi11, format!("111 and {} (LXI)", crate::vxi11::CORE_PORT)),
            Item::RemTelnetInfo => port(s.remote_telnet, "23, a shell without password".into()),
            _ => return None,
        })
    }

    pub(super) fn remote_options(&self, it: Item) -> Option<Vec<String>> {
        Self::remote_kind(it).map(|_| vec!["Off".into(), "On".into()])
    }

    pub(super) fn remote_adjust(&mut self, it: Item, _d: i32) -> bool {
        let s = &mut self.sys;
        let (on, what) = match it {
            Item::RemWeb => (&mut s.remote_web, "Web remote"),
            Item::RemScpi => (&mut s.remote_scpi, "SCPI"),
            Item::RemVxi => (&mut s.remote_vxi11, "VXI-11"),
            Item::RemTelnet => (&mut s.remote_telnet, "telnet"),
            _ => return false,
        };
        *on = !*on;
        let now_on = *on;
        self.sys_save();
        // what is lost with it
        match (it, now_on) {
            (Item::RemWeb, false) => self.toast("Web remote off: updates then come from a USB stick"),
            (Item::RemTelnet, false) => self.toast("telnet off: no remote shell for service"),
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
        for (sw, info) in [(Item::RemWeb, Item::NetWeb), (Item::RemScpi, Item::RemScpiInfo), (Item::RemVxi, Item::RemVxiInfo), (Item::RemTelnet, Item::RemTelnetInfo)] {
            let a = d.rows.iter().find(|r| r.it == sw).unwrap().ctrl;
            let b = d.rows.iter().find(|r| r.it == info).unwrap().ctrl;
            assert!(a.y == b.y && a.x < b.x, "{:?} {:?}", sw, info);
        }
    }

    #[test]
    fn a_service_switched_off_says_so() {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        assert!(u.sys.remote_web && u.sys.remote_scpi && u.sys.remote_vxi11 && u.sys.remote_telnet, "all on by default");
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
