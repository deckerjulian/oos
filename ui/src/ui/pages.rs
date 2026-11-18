// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The menu pages (ADR 0060): `PAGES` holds one entry per kind of `MenuId`, filled in by the module that owns the page,
//! and the shared menu code asks it before its own arms. A kind is the variant: `Channel(c)`, `ChMore(c)` and
//! `Decoders(k)` are one entry each, and the functions get the `MenuId` with its parameter. `Pick` is no page: it is
//! the picker of a row, and the page whose `picks` holds that row answers for a picked line.
//!
//! A new page: its module, its `MenuId` variant and its line here; `every_page_is_complete` names a kind that has
//! neither a line nor a place in `NOT_YET_IN_PAGES`, the pages still answered by the arms of the shared files. That
//! list only gets shorter.

use super::*;

/// Where a dialog stands: the x of the middle of what opened it, and whether that is in the top bar (the dialog hangs
/// below it) or at the bottom (the dialog sits above it).
type Anchor = fn(&Ui) -> (i32, bool);

/// One page of the menus.
pub(super) struct Page {
    /// the kind of `MenuId` this entry answers for
    pub id: MenuId,
    /// the title of its dialog
    pub title: fn(&Ui, MenuId) -> String,
    /// its rows, in order
    pub items: fn(&Ui, MenuId) -> Vec<Item>,
    /// where its dialog stands; none: centred, or under its top-bar button
    pub anchor: Option<Anchor>,
    /// the rows whose pickers it answers for: what a picked line does is `pick`, not the shared choice of an option
    pub picks: fn(Item) -> bool,
    /// a line `k` of the picker of row `it` was picked (after the picker closed)
    pub pick: fn(&mut Ui, Item, usize),
    /// the pickers that are one wide column, for long lines (network and device names)
    pub wide: fn(Item) -> bool,
}

/// Below the LAN box of the top bar: the pages of the network.
const NET_BOX: Option<Anchor> = Some(|u| (u.net_box().x + u.net_box().w / 2, true));

fn none(_: Item) -> bool {
    false
}

pub(super) const PAGES: [Page; 5] = [
    Page {
        id: MenuId::Network,
        title: |_, _| "Network".into(),
        items: |u, _| u.network_items(),
        anchor: NET_BOX,
        picks: |it| matches!(it, Item::WlanNet | Item::WlanForgetNet),
        pick: |u, it, k| if it == Item::WlanNet { u.wlan_pick(k) } else { u.wlan_forget_pick(k) },
        wide: |it| matches!(it, Item::WlanNet | Item::WlanForgetNet),
    },
    Page {
        id: MenuId::Remote,
        title: |_, _| "Remote services".into(),
        items: |u, _| u.remote_items(),
        anchor: NET_BOX,
        picks: |it| matches!(it, Item::RemName | Item::RemAllow),
        pick: Ui::time_pick,
        wide: none,
    },
    Page {
        id: MenuId::Ssh,
        title: |_, _| "SSH".into(),
        items: |u, _| u.ssh_items(),
        anchor: NET_BOX,
        picks: |it| it == Item::SshRemoveKey,
        pick: |u, _, k| u.ssh_remove_pick(k),
        wide: |it| it == Item::SshRemoveKey,
    },
    Page {
        id: MenuId::Bluetooth,
        title: |_, _| "Bluetooth".into(),
        items: |u, _| u.bt_items(),
        anchor: NET_BOX,
        picks: Ui::bt_is_pick,
        pick: Ui::bt_pick,
        wide: Ui::bt_is_pick,
    },
    Page {
        id: MenuId::Time,
        title: |_, _| "Date and time".into(),
        items: |u, _| u.time_items(),
        anchor: None,
        picks: |it| matches!(it, Item::NtpServer | Item::TimeZone),
        pick: Ui::time_pick,
        wide: none,
    },
];

/// The entry of the kind of `m`.
pub(super) fn page(m: MenuId) -> Option<&'static Page> {
    PAGES.iter().find(|p| std::mem::discriminant(&p.id) == std::mem::discriminant(&m))
}

impl Ui {
    /// The page that answers for the picker of row `it`, if one does.
    pub(super) fn page_of_pick(it: Item) -> Option<&'static Page> {
        PAGES.iter().find(|p| (p.picks)(it))
    }

    /// The picker of row `it` is one wide column.
    pub(super) fn wide_pick(it: Item) -> bool {
        PAGES.iter().any(|p| (p.wide)(it))
    }
}

/// A state the layout lint opens a page in (`lint/menu_layout.rs`), beyond the default one: the page, a name, what
/// sets the state, and the pickers to open in it that a row does not open (those of actions).
#[cfg(test)]
pub(super) type Layout = (MenuId, &'static str, fn(&mut Ui), &'static [Item]);

/// The longest states of the pages of `PAGES`, for the layout lint.
#[cfg(test)]
pub(super) const LAYOUTS: &[Layout] = &[
    (MenuId::Bluetooth, "crowded", Ui::bt_crowded_for_tests, &[Item::BtFound, Item::BtPairedPick, Item::BtForgetPick]),
    // the clock page: a failure after a sync, a zone of its own, a long server name
    (
        MenuId::Time,
        "failed",
        |u| {
            u.sys.ntp = true;
            u.sys.ntp_server = "a-rather-long-ntp-server-name.example.org".into();
            u.sys.tz = "<+0545>-5:45<+0645>,M3.5.0/2:30:30,M10.5.0/3".into();
            u.ntp_report(Ok(time::Synced { at: 0.0, server: "x".into(), offset: 0.5, stratum: 1 }));
            u.ntp_report(Err("Address family not supported (os error 97)".into()));
        },
        &[],
    ),
    // NTP server = DHCP: the longest failure with the fallback server named
    (
        MenuId::Time,
        "DHCP failed",
        |u| {
            u.sys.ntp = true;
            u.ntp_report(Err("Address family not supported (os error 97)".into()));
            u.ntp_dhcp_asked_for_tests("pool.ntp.org", crate::sysconf::NtpOrigin::NoDhcp);
        },
        &[],
    ),
    (
        MenuId::Time,
        "DHCP failed lease",
        |u| {
            u.sys.ntp = true;
            u.ntp_dhcp_asked_for_tests("255.255.255.255", crate::sysconf::NtpOrigin::Dhcp("wlan0"));
            u.ntp_report(Err("Address family not supported (os error 97)".into()))
        },
        &[],
    ),
    (
        MenuId::Time,
        "synced",
        |u| {
            u.sys.ntp = true;
            u.ntp_report(Ok(time::Synced { at: 0.0, server: "x".into(), offset: -12.25, stratum: 1 }))
        },
        &[],
    ),
    (
        MenuId::Network,
        "100M",
        |u| {
            u.eth_link = true;
            u.eth.speed = Some(100);
            u.net_ifs = vec![("eth0".into(), "192.168.177.164".into())]
        },
        &[],
    ),
    // the longest name on the network, and the longest default (DHO924S with six digits)
    (
        MenuId::Remote,
        "long name",
        |u| {
            u.sys.net_name = "x".repeat(63);
            u.net = "192.168.177.164".into()
        },
        &[],
    ),
    (MenuId::Remote, "default name", |u| u.sys.net_name = "dho924s-123456".into(), &[]),
    // the longest address list, the network beside it, and a change waiting for the next start
    (
        MenuId::Remote,
        "long allow list",
        |u| {
            u.sys.set_net_allow(Some(&"2001:db8:aaaa:bbbb::/64 ".repeat(6)));
            u.remote.lans = vec!["192.168.177.0/24".into(), "10.20.0.0/24".into()]
        },
        &[],
    ),
];

/// The kinds whose pages are still answered by the arms of `menu_items.rs`, `menu_text.rs`, `menu_dialog.rs` and
/// `menu_actions.rs`, until they move into `PAGES` (ADR 0060). Only gets shorter.
#[cfg(test)]
const NOT_YET_IN_PAGES: [MenuId; 43] = [
    MenuId::Main,
    MenuId::Channel(0),
    MenuId::ChMore(0),
    MenuId::Logger,
    MenuId::Horizontal,
    MenuId::Trigger,
    MenuId::Acquire,
    MenuId::Measure,
    MenuId::MeasAdd,
    MenuId::MeasHold,
    MenuId::MeasPair,
    MenuId::Dvm,
    MenuId::Cursor,
    MenuId::Display,
    MenuId::Utility,
    MenuId::Math,
    MenuId::Ref,
    MenuId::Decode,
    MenuId::Bus,
    MenuId::Storage,
    MenuId::Mask,
    MenuId::Zone,
    MenuId::Spectrum,
    MenuId::Eye,
    MenuId::Power,
    MenuId::Magnetic,
    MenuId::Inverter,
    MenuId::Dpt,
    MenuId::Identify,
    MenuId::SelfCal,
    MenuId::Features,
    MenuId::Decoders(0),
    MenuId::Bode,
    MenuId::Sim,
    MenuId::SimAdvanced,
    MenuId::Afg,
    MenuId::BwTest,
    MenuId::ViewMore,
    MenuId::La,
    MenuId::LaLines,
    MenuId::LaPat,
    MenuId::Windows,
    MenuId::MeasSelected,
];

/// One `MenuId` of every kind but `Pick`. The `match` makes a new variant a compile error here until it is listed.
#[cfg(test)]
pub(super) fn every_kind() -> Vec<MenuId> {
    use MenuId::*;
    let all = vec![
        Main,
        Channel(0),
        ChMore(0),
        Logger,
        Horizontal,
        Trigger,
        Acquire,
        Measure,
        MeasAdd,
        MeasHold,
        MeasPair,
        Dvm,
        Cursor,
        Display,
        Utility,
        Math,
        Ref,
        Decode,
        Bus,
        Storage,
        Mask,
        Zone,
        Spectrum,
        Eye,
        Power,
        Magnetic,
        Inverter,
        Dpt,
        Identify,
        SelfCal,
        Network,
        Features,
        Decoders(0),
        Remote,
        Ssh,
        Bluetooth,
        Time,
        Bode,
        Sim,
        SimAdvanced,
        Afg,
        BwTest,
        ViewMore,
        La,
        LaLines,
        LaPat,
        Windows,
        MeasSelected,
    ];
    for m in &all {
        match m {
            Main | Channel(_) | ChMore(_) | Logger | Horizontal | Trigger | Acquire | Measure | MeasAdd | MeasHold
            | MeasPair | Dvm | Cursor | Display | Utility | Math | Ref | Decode | Bus | Storage | Mask | Zone
            | Spectrum | Eye | Power | Magnetic | Inverter | Dpt | Identify | SelfCal | Network | Features
            | Decoders(_) | Remote | Ssh | Bluetooth | Time | Bode | Sim | SimAdvanced | Afg | BwTest | ViewMore
            | La | LaLines | LaPat | Windows | MeasSelected => {}
            Pick => unreachable!("Pick is no page"),
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: MenuId, b: MenuId) -> bool {
        std::mem::discriminant(&a) == std::mem::discriminant(&b)
    }

    /// Every kind of page is in `PAGES` or in `NOT_YET_IN_PAGES`, not in both, each once; every entry of `PAGES` has
    /// a title and rows (ADR 0060).
    #[test]
    fn every_page_is_complete() {
        let ui = crate::ui::testkit::demo_ui();
        let mut missing = Vec::new();
        for m in every_kind() {
            let listed = PAGES.iter().filter(|p| same(p.id, m)).count();
            let waiting = NOT_YET_IN_PAGES.iter().filter(|w| same(**w, m)).count();
            if listed + waiting != 1 {
                missing.push(format!("{:?}: {} in PAGES, {} in NOT_YET_IN_PAGES", m, listed, waiting));
            }
        }
        for p in &PAGES {
            if (p.title)(&ui, p.id).is_empty() {
                missing.push(format!("{:?}: no title", p.id));
            }
            if (p.items)(&ui, p.id).is_empty() {
                missing.push(format!("{:?}: no rows", p.id));
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    /// A picker is answered by one page at most: of the rows of every page, and of the pickers a page opens itself.
    #[test]
    fn a_picker_has_one_page() {
        let ui = crate::ui::testkit::demo_ui();
        let opened = [Item::WlanNet, Item::WlanForgetNet, Item::SshRemoveKey, Item::BtFound, Item::BtPairedPick];
        for it in PAGES.iter().flat_map(|p| (p.items)(&ui, p.id)).chain(opened.into_iter().chain([Item::BtForgetPick]))
        {
            let n = PAGES.iter().filter(|p| (p.picks)(it)).count();
            assert!(n <= 1, "{:?} is answered by {} pages", it, n);
        }
    }
}
