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
/// below it) or at the bottom (the dialog sits above it). It gets the page's `MenuId`, with its channel.
type Anchor = fn(&Ui, MenuId) -> (i32, bool);

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
    /// the signals it reads: a channel among them that is off gets a button to switch it on (`off_sources`)
    pub sources: Option<fn(&Ui) -> Vec<WaveId>>,
}

/// Below the LAN box of the top bar: the pages of the network.
const NET_BOX: Option<Anchor> = Some(|u, _| (u.net_box().x + u.net_box().w / 2, true));

/// Above a card at the bottom: the channel's, the time base's or the trigger's.
const CH_CARD: Option<Anchor> = Some(|u, m| {
    let c = match m {
        MenuId::Channel(c) | MenuId::ChMore(c) => c,
        _ => 0,
    };
    let r = u.cards().0[c];
    (r.x + r.w / 2, false)
});
const TIME_CARD: Option<Anchor> = Some(|u, _| (u.cards().1.x + u.cards().1.w / 2, false));
const TRIG_CARD: Option<Anchor> = Some(|u, _| (u.cards().2.x + u.cards().2.w / 2, false));

/// The channel of a channel page.
fn ch_of(m: MenuId) -> usize {
    match m {
        MenuId::Channel(c) | MenuId::ChMore(c) => c,
        _ => 0,
    }
}

/// A page of a title, rows, an anchor and the signals it reads, without pickers of its own.
#[allow(clippy::type_complexity)]
const fn plain(
    id: MenuId,
    title: fn(&Ui, MenuId) -> String,
    items: fn(&Ui, MenuId) -> Vec<Item>,
    anchor: Option<Anchor>,
    sources: Option<fn(&Ui) -> Vec<WaveId>>,
) -> Page {
    Page { id, title, items, anchor, picks: none, pick: no_pick, wide: none, sources }
}

fn none(_: Item) -> bool {
    false
}

fn no_pick(_: &mut Ui, _: Item, _: usize) {}

/// A page of rows and a title, its signals, no pickers of its own and the dialog where it always was.
macro_rules! page {
    ($id:expr, $title:expr, $items:expr, $sources:expr) => {
        Page {
            id: $id,
            title: |_, _| $title.into(),
            items: $items,
            anchor: None,
            picks: none,
            pick: no_pick,
            wide: none,
            sources: $sources,
        }
    };
}

pub(super) const PAGES: [Page; 36] = [
    Page {
        id: MenuId::Network,
        title: |_, _| "Network".into(),
        items: |u, _| u.network_items(),
        anchor: NET_BOX,
        picks: |it| matches!(it, Item::WlanNet | Item::WlanForgetNet),
        pick: |u, it, k| if it == Item::WlanNet { u.wlan_pick(k) } else { u.wlan_forget_pick(k) },
        wide: |it| matches!(it, Item::WlanNet | Item::WlanForgetNet),
        sources: None,
    },
    Page {
        id: MenuId::Remote,
        title: |_, _| "Remote services".into(),
        items: |u, _| u.remote_items(),
        anchor: NET_BOX,
        picks: |it| matches!(it, Item::RemName | Item::RemAllow),
        pick: Ui::time_pick,
        wide: none,
        sources: None,
    },
    Page {
        id: MenuId::Ssh,
        title: |_, _| "SSH".into(),
        items: |u, _| u.ssh_items(),
        anchor: NET_BOX,
        picks: |it| it == Item::SshRemoveKey,
        pick: |u, _, k| u.ssh_remove_pick(k),
        wide: |it| it == Item::SshRemoveKey,
        sources: None,
    },
    Page {
        id: MenuId::Bluetooth,
        title: |_, _| "Bluetooth".into(),
        items: |u, _| u.bt_items(),
        anchor: NET_BOX,
        picks: Ui::bt_is_pick,
        pick: Ui::bt_pick,
        wide: Ui::bt_is_pick,
        sources: None,
    },
    Page {
        id: MenuId::Time,
        title: |_, _| "Date and time".into(),
        items: |u, _| u.time_items(),
        anchor: None,
        picks: |it| matches!(it, Item::NtpServer | Item::TimeZone),
        pick: Ui::time_pick,
        wide: none,
        sources: None,
    },
    page!(MenuId::Logger, "Data logger", |_, _| vec![Item::LogOn, Item::LogIvl, Item::LogInfo], None),
    page!(
        MenuId::Power,
        "Power analysis",
        |u, _| u.power_items(),
        Some(|u| match u.m.pw_mode {
            PowerMode::Ripple | PowerMode::Rail => vec![u.m.pw_v],
            _ => vec![u.m.pw_v, u.m.pw_i],
        })
    ),
    page!(MenuId::Spectrum, "Spectrum analyser", |u, _| u.spectrum_items(), Some(|u| vec![u.m.spec_src])),
    page!(MenuId::Eye, "Eye diagram / jitter", |u, _| u.eye_items(), Some(|u| vec![u.m.eye_src])),
    page!(
        MenuId::Bode,
        "Bode plot",
        |u, _| u.bode_items(),
        Some(|u| {
            let m = &u.m;
            if m.bode_gen == bode::FREE {
                [Some(m.bode_out), m.bode_sw_in.checked_sub(1).map(WaveId::Ch), Some(WaveId::Ch(m.bode_sync))]
                    .into_iter()
                    .flatten()
                    .collect()
            } else {
                vec![m.bode_in, m.bode_out]
            }
        })
    ),
    page!(MenuId::Magnetic, "Magnetic", |u, _| u.magnetic_items(), Some(|u| vec![u.m.mag_v, u.m.mag_i])),
    page!(
        MenuId::Inverter,
        "Inverter",
        |u, _| u.inverter_items(),
        Some(|u| {
            let m = &u.m;
            if m.inv_page == InvPage::Dq0 {
                let mut v = vec![m.inv_a, m.inv_b, m.inv_c];
                if m.inv_ang == InvAngle::Analog {
                    v.push(m.inv_an);
                }
                return v;
            }
            let mut v = match m.inv_ang {
                InvAngle::Analog => vec![m.inv_an],
                InvAngle::Quad | InvAngle::Hall | InvAngle::Off => Vec::new(),
                InvAngle::Phase => vec![m.inv_a, m.inv_b, m.inv_c],
            };
            v.extend(m.inv_tq);
            v.extend(m.inv_ev);
            v.extend(m.inv_ei);
            v
        })
    ),
    page!(
        MenuId::Dpt,
        "Double pulse",
        |u, _| u.dpt_items(),
        Some(|u| {
            let m = &u.m;
            let mut v = vec![m.dpt_v, m.dpt_i, m.dpt_g];
            if m.dpt_4th != DptFourth::Off {
                v.push(m.dpt_4ch);
            }
            v
        })
    ),
    page!(
        MenuId::BwTest,
        "Bandwidth test (step response)",
        |u, _| u.bw_items(),
        Some(|u| vec![WaveId::Ch(u.m.bw_src)])
    ),
    page!(MenuId::Identify, "Identify: tap to apply", |u, _| u.identify_items(), None),
    page!(MenuId::SelfCal, "Self-calibration", |u, _| u.cal_items(), None),
    page!(MenuId::Windows, "Windows", |u, _| u.win_items(), None),
    page!(MenuId::Zone, "Zone trigger", |u, _| u.zone_items(), None),
    page!(MenuId::Mask, "Mask test", |u, _| u.mask_items(), Some(|u| vec![WaveId::Ch(u.m.mask_src)])),
    page!(MenuId::Utility, "System", |u, _| u.utility_items(), None),
    plain(MenuId::Channel(0), |_, m| format!("CH{}", ch_of(m) + 1), |u, m| u.channel_items(ch_of(m)), CH_CARD, None),
    plain(
        MenuId::ChMore(0),
        |_, m| format!("CH{} more", ch_of(m) + 1),
        |u, m| u.ch_more_items(ch_of(m)),
        CH_CARD,
        None,
    ),
    plain(MenuId::Horizontal, |_, _| "Horizontal".into(), |u, _| u.horizontal_items(), TIME_CARD, None),
    plain(MenuId::Trigger, |_, _| "Trigger".into(), |u, _| u.trigger_items(), TRIG_CARD, None),
    page!(MenuId::Acquire, "Acquire", |u, _| u.acquire_items(), None),
    plain(
        MenuId::Cursor,
        |u, _| {
            let src = u.selected_workspace_source().map_or("No source".into(), |id| u.wave_name(id));
            format!("Cursor \u{b7} {}", src)
        },
        |u, _| u.cursor_items(),
        None,
        None,
    ),
    page!(MenuId::Display, "Display", |u, _| u.display_items(), None),
    page!(MenuId::Math, "Math", |u, _| u.math_items(), None),
    plain(
        MenuId::Ref,
        |u, _| {
            u.refs[u.ref_sel].as_ref().map_or(format!("Reference R{} \u{b7} empty", u.ref_sel + 1), |r| {
                format!("Reference R{}{}{}", u.ref_sel + 1, if r.label.is_empty() { "" } else { " \u{b7} " }, r.label)
            })
        },
        |u, _| u.ref_items(),
        None,
        None,
    ),
    page!(
        MenuId::Decode,
        "Decode",
        |u, _| u.decode_items(),
        Some(|u| if u.m.dec_on_la() { Vec::new() } else { decode_wave(u.m.dec_src).into_iter().collect() })
    ),
    page!(MenuId::Bus, "Bus \u{b7} I2C", |u, _| u.bus_items(), None),
    plain(
        MenuId::Storage,
        // where the files go now; /tmp is in RAM: say so
        |u, _| {
            if u.save_dir() == std::path::Path::new(crate::store::USB_DIR) {
                "Storage \u{b7} USB stick".into()
            } else if u.save_dir().starts_with("/tmp") {
                format!("Storage \u{b7} {} (lost at power-off)", u.save_dir().display())
            } else {
                format!("Storage \u{b7} {}", u.save_dir().display())
            }
        },
        |u, _| u.storage_items(),
        None,
        None,
    ),
    page!(MenuId::Sim, "SIM", |u, _| u.sim_items(), None),
    page!(MenuId::SimAdvanced, "SIM advanced", |u, _| u.sim_advanced_items(), None),
    plain(
        MenuId::MeasPair,
        |u, _| format!("Pairs {} to {}", u.pair_source_caption(u.meas_src), u.pair_source_caption(u.meas_src_b)),
        |_, _| (0..tools::PAIRS.len()).map(Item::MeasPair).collect(),
        None,
        None,
    ),
    page!(
        MenuId::Dvm,
        "DVM and counter",
        |u, _| u.dvm_items(),
        Some(|u| {
            let m = &u.m;
            if u.offline {
                return Vec::new();
            }
            [m.dvm_on.then_some(WaveId::Ch(m.dvm_src)), m.cnt_on.then_some(WaveId::Ch(m.cnt_src))]
                .into_iter()
                .flatten()
                .collect()
        })
    ),
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
    // the number pad with the longest entry, its title the longest row name of a channel
    (
        MenuId::Channel(0),
        "number pad",
        |u| {
            u.numpad.target = Some(Item::ChOffset(0));
            u.keypad = "-1234.5678901".into();
        },
        &[Item::NumEntry],
    ),
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
const NOT_YET_IN_PAGES: [MenuId; 12] = [
    MenuId::Main,
    MenuId::Measure,
    MenuId::MeasAdd,
    MenuId::MeasHold,
    MenuId::Features,
    MenuId::Decoders(0),
    MenuId::Afg,
    MenuId::ViewMore,
    MenuId::La,
    MenuId::LaLines,
    MenuId::LaPat,
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

    /// What each page with `sources` reads, in each of its modes: the channels whose button `off_sources` shows
    /// when every channel is off, in the order of the page.
    #[test]
    fn the_sources_of_every_page_in_each_mode() {
        use WaveId::Ch;
        assert!(n_ch() >= 4, "the cases use four channels");
        type Set = fn(&mut Model);
        let cases: [(MenuId, &str, Set, &[usize]); 15] = [
            (MenuId::Power, "ripple", |m| (m.pw_mode, m.pw_v, m.pw_i) = (PowerMode::Ripple, Ch(1), Ch(2)), &[1]),
            (MenuId::Power, "rail", |m| (m.pw_mode, m.pw_v, m.pw_i) = (PowerMode::Rail, Ch(1), Ch(2)), &[1]),
            (MenuId::Power, "quality", |m| (m.pw_mode, m.pw_v, m.pw_i) = (PowerMode::Quality, Ch(1), Ch(2)), &[1, 2]),
            (MenuId::Spectrum, "", |m| m.spec_src = Ch(2), &[2]),
            (MenuId::Eye, "", |m| m.eye_src = Ch(3), &[3]),
            (
                MenuId::Bode,
                "free with an input",
                |m| (m.bode_gen, m.bode_out, m.bode_sw_in, m.bode_sync) = (bode::FREE, Ch(0), 2, 3),
                &[0, 1, 3],
            ),
            (
                MenuId::Bode,
                "free without an input",
                |m| (m.bode_gen, m.bode_out, m.bode_sw_in, m.bode_sync) = (bode::FREE, Ch(0), 0, 3),
                &[0, 3],
            ),
            (MenuId::Bode, "generator", |m| (m.bode_gen, m.bode_in, m.bode_out) = (5, Ch(1), Ch(2)), &[1, 2]),
            (MenuId::Magnetic, "", |m| (m.mag_v, m.mag_i) = (Ch(0), Ch(1)), &[0, 1]),
            (
                MenuId::Inverter,
                "dq0 analog",
                |m| {
                    (m.inv_page, m.inv_ang, m.inv_a, m.inv_b, m.inv_c, m.inv_an) =
                        (InvPage::Dq0, InvAngle::Analog, Ch(0), Ch(1), Ch(2), Ch(3))
                },
                &[0, 1, 2, 3],
            ),
            (
                MenuId::Inverter,
                "mechanical, phases and torque",
                |m| {
                    (m.inv_page, m.inv_ang, m.inv_a, m.inv_b, m.inv_c) =
                        (InvPage::Mech, InvAngle::Phase, Ch(2), Ch(1), Ch(0));
                    (m.inv_tq, m.inv_ev, m.inv_ei) = (Some(Ch(3)), None, None);
                },
                &[2, 1, 0, 3],
            ),
            (
                MenuId::Inverter,
                "mechanical, hall",
                |m| {
                    (m.inv_page, m.inv_ang) = (InvPage::Mech, InvAngle::Hall);
                    (m.inv_tq, m.inv_ev, m.inv_ei) = (None, Some(Ch(1)), Some(Ch(2)));
                },
                &[1, 2],
            ),
            (
                MenuId::Dpt,
                "three",
                |m| (m.dpt_v, m.dpt_i, m.dpt_g, m.dpt_4th) = (Ch(0), Ch(1), Ch(2), DptFourth::Off),
                &[0, 1, 2],
            ),
            (MenuId::BwTest, "", |m| m.bw_src = 2, &[2]),
            (MenuId::Mask, "", |m| m.mask_src = 1, &[1]),
        ];
        for (menu, mode, set, want) in cases {
            let mut u = crate::ui::testkit::demo_ui();
            for c in 0..n_ch() {
                u.m.ch[c].on = false;
            }
            set(&mut u.m);
            assert_eq!(u.off_sources(menu), want, "{:?} {}", menu, mode);
        }
        // a fourth signal of the double pulse
        let mut u = crate::ui::testkit::demo_ui();
        for c in 0..n_ch() {
            u.m.ch[c].on = false;
        }
        (u.m.dpt_v, u.m.dpt_i, u.m.dpt_g, u.m.dpt_4ch) = (Ch(0), Ch(1), Ch(2), Ch(3));
        u.m.dpt_4th = DptFourth::Gate;
        assert_eq!(u.off_sources(MenuId::Dpt), vec![0, 1, 2, 3]);
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
