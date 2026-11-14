// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Layout check of every menu: nothing outside the screen or the dialog, no text wider than its control, no row
//! without a label. It walks the menus in all trigger types, decoders and analyser pages, with the extreme values of
//! the settings. A text that runs out of its button looks fine in a test of the logic, so this reads the geometry.

use super::*;
use std::collections::BTreeSet;

/// The screens the layout is checked on: the panel, a 10" panel (1280 x 800) and a Full HD monitor.
pub(super) const SCREENS: [Screen; 3] = [Screen::PANEL, Screen { w: 1280, h: 800 }, Screen { w: 1920, h: 1080 }];

fn demo_on(scr: Screen) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.scr = scr;
    u
}

fn check(ui: &Ui, ctx: &str, out: &mut BTreeSet<String>) {
    let Some(d) = ui.dlg_layout() else { return };
    let f = ui.fonts();
    let mut bad = |what: String| {
        out.insert(format!("{}: {}", ctx, what));
    };
    if d.r.x < 0 || d.r.y < 0 || d.r.right() > ui.scr.w || d.r.bottom() > ui.scr.h {
        bad(format!("dialog off the screen {:?}", d.r));
    }
    for (i, row) in d.rows.iter().enumerate() {
        let c = row.ctrl;
        let name = format!("{:?}", row.it);
        if c.y + c.h > d.r.y + d.r.h || c.x + c.w > d.r.x + d.r.w {
            bad(format!("{} outside the dialog", name));
        }
        if d.rows[i + 1..].iter().any(|o| overlaps(o.ctrl, c)) {
            bad(format!("{} overlaps another row", name));
        }
        let kind = Ui::kind(row.it);
        if row.label.w > 0 && kind != Kind::Action {
            let t = ui.item_label(row.it);
            if t.is_empty() {
                bad(format!("{} has no label", name));
            }
            if f.s.width(t) > row.label.w {
                bad(format!("label '{}' {} > {}", t, f.s.width(t), row.label.w));
            }
        }
        match &row.ctl {
            Ctl::Seg(opts, _) => {
                let n = opts.len() as i32;
                let sw = (c.w - (n - 1) * 4) / n;
                for o in opts {
                    if f.mb.width(o) + 8 > sw {
                        bad(format!("{} option '{}' {} > {}", name, o, f.mb.width(o) + 8, sw));
                    }
                }
            }
            Ctl::Value | Ctl::Pick => {
                let (_, mid, _) = Ui::value_parts(c);
                let v = ui.value(row.it);
                let room = if matches!(row.ctl, Ctl::Pick) { mid.w - 44 } else { mid.w - 8 };
                if f.mb.width(&v) > room {
                    bad(format!("{} value '{}' {} > {}", name, v, f.mb.width(&v), room));
                }
            }
            Ctl::Text => {
                let v = ui.value(row.it);
                if ui.text_row_font(c.w, &v).width(&v) > c.w {
                    bad(format!("{} text too long", name));
                }
            }
            Ctl::Button => {
                let t = match (row.it, ui.pick) {
                    (Item::PickOpt(k), Some(p)) => ui.options(p.it).get(k).cloned().unwrap_or_default(),
                    _ if kind == Kind::Action => ui.item_label(row.it).to_string(),
                    _ => ui.value(row.it),
                };
                if t.is_empty() {
                    bad(format!("{} has no text", name));
                }
                let (font, r, fits) = ui.btn_text(c, matches!(row.it, Item::Open(_)), &t);
                if !fits {
                    bad(format!("{} text '{}' {} > {}", name, t, font.width(&t), r.w - 8));
                }
            }
        }
    }
}

/// The menu as it is, its pickers, and the rows with their extreme values.
fn walk(make: &dyn Fn() -> Ui, ctx: &str, out: &mut BTreeSet<String>) {
    let ui = make();
    check(&ui, ctx, out);
    let items: Vec<Item> = ui.dlg_layout().map_or(Vec::new(), |d| d.rows.iter().map(|r| r.it).collect());
    for it in items {
        if Ui::kind(it) == Kind::Action {
            continue;
        }
        let mut u = make();
        if !u.options(it).is_empty() {
            u.open_picker(it);
            check(&u, &format!("{} > picker {:?}", ctx, it), out);
        }
        for d in [-100_000, 100_000] {
            let mut u = make();
            u.adjust(it, d);
            check(&u, &format!("{} > {:?} {}", ctx, it, if d < 0 { "min" } else { "max" }), out);
        }
    }
}

/// Every menu at every text size, on every screen.
#[test]
fn every_menu_fits() {
    let mut out = BTreeSet::new();
    for scr in SCREENS {
        for ts in TEXT_SIZES {
            menus_fit_at(scr, ts, &mut out);
        }
    }
    let list: Vec<&String> = out.iter().collect();
    for l in &list {
        println!("{}", l);
    }
    assert!(list.is_empty(), "{} layout problems", list.len());
}

fn menus_fit_at(scr: Screen, ts: TextSize, all: &mut BTreeSet<String>) {
    let mut out = BTreeSet::new();
    let open = |m: MenuId, set: &dyn Fn(&mut Ui)| {
        let mut u = demo_on(scr);
        u.m.text_size = ts;
        set(&mut u);
        u.relayout();
        u.open_menu(m);
        u
    };
    use MenuId::*;
    for m in [Main, Channel(0), Channel(3), Horizontal, Acquire, Measure, MeasAdd, MeasAddMore, MeasPair, Dvm, Cursor, Display, Utility, Math, Ref, Storage, Mask, Zone, Spectrum, Eye, Power, Identify, SelfCal, Bode, Afg, BwTest, La, LaLines, LaPat, Network, Remote, Time] {
        walk(&|| open(m, &|_| {}), &format!("{:?}", m), &mut out);
        walk(&|| open(m, &|u| u.m.la.on = true), &format!("{:?} LA", m), &mut out);
    }
    let trend = |u: &mut Ui| {
        u.m.pane = tables::PANE_TREND;
        u.m.meas = vec![(Meas::Vpp, 0), (Meas::Freq, 3)];
        u.m.meas2 = vec![(0, 0, 1)];
    };
    walk(&|| open(Display, &trend), "Display Trend", &mut out);
    walk(&|| open(Bode, &|u| u.m.bode_gen = bode::FREE), "Bode free sweep", &mut out);
    walk(&|| open(Bode, &|u| { u.m.bode_gen = bode::FREE; u.m.bode_sw_in = 0 }), "Bode free sweep no input", &mut out);
    walk(&|| open(Bode, &|u| u.m.bode_gen = 5), "Bode UNI-T LAN", &mut out);
    walk(&|| open(Bode, &|u| { u.m.bode_gen = 5; u.m.bode_usb = true }), "Bode UNI-T USB", &mut out);
    // the clock page in its longest states: a failure after a sync, a zone of its own, a long server name
    let ntp_failed = |u: &mut Ui| {
        u.sys.ntp = true;
        u.sys.ntp_server = "a-rather-long-ntp-server-name.example.org".into();
        u.sys.tz = "<+0545>-5:45<+0645>,M3.5.0/2:30:30,M10.5.0/3".into();
        u.ntp_report(Ok(time::Synced { at: 0.0, server: "x".into(), offset: 0.5, stratum: 1 }));
        u.ntp_report(Err("Address family not supported (os error 97)".into()));
    };
    walk(&|| open(Time, &ntp_failed), "Time failed", &mut out);
    // NTP server = DHCP: the longest failure with the fallback server named
    let dhcp_failed = |u: &mut Ui| {
        u.sys.ntp = true;
        u.ntp_report(Err("Address family not supported (os error 97)".into()));
        u.ntp_dhcp_asked_for_tests("pool.ntp.org", crate::sysconf::NtpOrigin::NoDhcp);
    };
    walk(&|| open(Time, &dhcp_failed), "Time DHCP failed", &mut out);
    walk(&|| open(Time, &|u| { u.sys.ntp = true; u.ntp_dhcp_asked_for_tests("255.255.255.255", crate::sysconf::NtpOrigin::Dhcp("wlan0")); u.ntp_report(Err("Address family not supported (os error 97)".into())) }), "Time DHCP failed lease", &mut out);
    walk(&|| open(Time, &|u| { u.sys.ntp = true; u.ntp_report(Ok(time::Synced { at: 0.0, server: "x".into(), offset: -12.25, stratum: 1 })) }), "Time synced", &mut out);
    walk(&|| open(Network, &|u| { u.eth_link = true; u.eth.speed = Some(100); u.net_ifs = vec![("eth0".into(), "192.168.177.164".into())] }), "Network 100M", &mut out);
    // the longest name on the network, and the longest default (DHO924S with six digits)
    walk(&|| open(Remote, &|u| { u.sys.net_name = "x".repeat(63); u.net = "192.168.177.164".into() }), "Remote long name", &mut out);
    walk(&|| open(Remote, &|u| u.sys.net_name = "dho924s-204527".into()), "Remote default name", &mut out);
    walk(&|| open(Spectrum, &|u| { u.m.pane = tables::PANE_SPECTRUM; u.azr.spec_stop = 1.5e8; }), "Spectrum pane", &mut out);
    for t in TRIG_TYPES {
        walk(&|| open(Trigger, &|u| u.set_trig_type(t)), &format!("Trigger {}", t.name()), &mut out);
        walk(&|| open(Trigger, &|u| { u.m.la.on = true; u.set_trig_type(t) }), &format!("Trigger {} LA", t.name()), &mut out);
    }
    for p in PROTOS.iter().copied() {
        walk(&|| open(Decode, &|u| u.set_dec_proto(p)), &format!("Decode {}", p.name()), &mut out);
        walk(&|| open(Decode, &|u| { u.m.la.on = true; u.m.dec_la_in = true; u.set_dec_proto(p) }), &format!("Decode {} LA", p.name()), &mut out);
    }
    for t in TRIG_TYPES {
        for p in [Proto::Uart, Proto::Can, Proto::I2c] {
            walk(&|| open(Trigger, &|u| { u.set_dec_proto(p); u.set_trig_type(t) }), &format!("Trigger {} {}", t.name(), p.name()), &mut out);
        }
    }
    all.extend(out.into_iter().map(|l| format!("{}x{} {}: {}", scr.w, scr.h, ts.name(), l)));
}

#[test]
fn acquisition_text_in_the_top_bar_opens_the_acquire_page() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.tap(160, 20);
    assert!(u.menu_open && u.menu == MenuId::Acquire);
    u.tap(160, 20);
    assert!(!u.menu_open);
    // what is used most, and the menus without a front-panel key
    assert_eq!(u.top_buttons().iter().map(|b| b.1).collect::<Vec<_>>(), ["Measure", "Cursor", "Display", "Storage", "System"]);
    // the buttons beside it keep working
    let (_, _, r) = u.top_buttons()[0];
    u.tap(r.x + 5, r.y + 5);
    assert!(u.menu_open && u.menu != MenuId::Acquire);
}

#[test]
fn a_page_offers_to_switch_on_its_channels() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.open_menu(MenuId::Power);
    assert!(!u.m.ch[1].on);
    assert_eq!(u.off_sources(MenuId::Power), [1]);
    assert!(u.items(MenuId::Power).contains(&Item::SrcOn));
    u.activate(Item::SrcOn);
    assert!(u.m.ch[1].on);
    assert!(!u.items(MenuId::Power).contains(&Item::SrcOn));
    // a page with its source on has no such row
    u.open_menu(MenuId::Eye);
    assert!(!u.items(MenuId::Eye).contains(&Item::SrcOn));
    // the decoder on LA lines does not need the channel
    u.m.dec_src = 3;
    assert!(u.items(MenuId::Decode).contains(&Item::SrcOn));
    u.m.la.on = true;
    u.m.dec_la_in = true;
    assert!(!u.items(MenuId::Decode).contains(&Item::SrcOn));
}

#[test]
fn files_go_to_the_stick_while_it_is_there() {
    assert!(crate::store::usb_mounted("/dev/sda1 /mnt/usb vfat rw 0 0\n"));
    assert!(!crate::store::usb_mounted("/dev/mmcblk0p1 /oos ext4 rw 0 0\n/dev/sda1 /mnt/usb2 vfat rw 0 0\n"));
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    assert_eq!(u.save_dir(), std::path::Path::new(crate::store::DEFAULT_DIR));
    assert!(u.menu_title(MenuId::Storage).contains("lost at power-off"));
    u.usb = true;
    assert_eq!(u.save_dir(), std::path::Path::new(crate::store::USB_DIR));
    assert_eq!(u.menu_title(MenuId::Storage), "Storage \u{b7} USB stick");
    // a directory given by hand stays
    u.store_dir = "/oos/shots".into();
    assert_eq!(u.save_dir(), std::path::Path::new("/oos/shots"));
    assert_eq!(u.saved_where(), "");
}

#[test]
fn system_info_has_the_rates() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.wfm_rate = 123.0;
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    assert!(u.sysinfo_rows().iter().flatten().any(|r| matches!(r, super::sysinfo::Row::Kv(k, ..) if k.contains("Waveforms"))));
}

/// All mixes of channels, LA and generator: the row fits, nothing overlaps, every card can be touched.
#[test]
fn bottom_row_fits_in_every_mix() {
    for scr in SCREENS {
        bottom_row_fits_on(scr);
    }
}

fn bottom_row_fits_on(scr: Screen) {
    let mut u = demo_on(scr);
    for bits in 0..64 {
        for c in 0..NCH {
            u.m.ch[c].on = bits >> c & 1 == 1;
        }
        (u.m.la.on, u.m.afg_on) = (bits & 16 != 0, bits & 32 != 0);
        let c = u.card_layout();
        let (la, gen) = (c.la.expect("the DHO924S has an LA port"), c.gen.expect("and a generator"));
        let mut all: Vec<Rect> = c.ch.to_vec();
        all.extend([la, gen, c.time, c.trig]);
        for (i, a) in all.iter().enumerate() {
            assert!(a.w >= 54 && a.right() <= u.scr.w - 6, "{:?} mix {:b}: {:?}", scr, bits, a);
            assert!(!all[i + 1..].iter().any(|b| overlaps(*a, *b)), "mix {:b}: {:?}", bits, a);
        }
        assert!(c.trig.w >= 222 && c.time.w >= 130, "mix {:b}", bits);
        // a channel that is on keeps room for its text
        for k in 0..NCH {
            assert!(!u.m.ch[k].on || c.ch[k].w >= 90, "mix {:b}: CH{} {}", bits, k + 1, c.ch[k].w);
        }
    }
}

/// The main screen at every text size: the top bar keeps its boxes and buttons apart, the text stands inside the bars,
/// boxes and rows, the measurement rows fit between the plot and the cards, a card text fits wherever it fits at the
/// normal size, and the menus stay between the top bar and the cards.
#[test]
fn every_text_size_keeps_the_screen_in_order() {
    for scr in SCREENS {
        screen_in_order_on(scr);
    }
}

fn screen_in_order_on(scr: Screen) {
    let cap = |f: &Font| f.size * 72 / 100;
    let normal = crate::font::Fonts::get(0);
    for ts in TEXT_SIZES {
        let mut u = demo_on(scr);
        u.m.text_size = ts;
        let f = u.fonts();
        // top bar, with and without the stick boxes
        for (usb, wlan) in [(false, false), (true, false), (false, true), (true, true)] {
            (u.usb, u.wlan_stick) = (usb, wlan);
            let pill = u.run_pill();
            assert!(f.mb.width("TRIG'D") + 30 <= pill.w, "{:?}: run pill {:?}", ts, pill);
            let boxes: Vec<Rect> = [Some(u.net_box()), wlan.then(|| u.wlan_box()), usb.then(|| u.usb_box())].into_iter().flatten().collect();
            for (i, b) in boxes.iter().enumerate() {
                assert!(b.y >= 0 && b.bottom() <= u.frame().top.bottom() && b.right() <= u.scr.w - 6, "{:?}: {:?}", ts, b);
                assert!(!boxes[i + 1..].iter().any(|o| overlaps(*o, *b)), "{:?}: {:?}", ts, b);
            }
            for (_, l, r) in u.top_buttons() {
                assert!(r.x > pill.x + pill.w && r.x + r.w <= u.top_right(), "{:?}: {} {:?}", ts, l, r);
                assert!(f.mb.width(l) + 8 <= r.w, "{:?}: {}", ts, l);
            }
        }
        (u.usb, u.wlan_stick) = (false, false);
        // the two lines of the network box: room for an address, the first line's descent above the second's capitals
        let n = u.net_box();
        assert!(20 + f.sb.width("255.255.255.255") <= n.w && 20 + f.sb.width("not connected") <= n.w, "{:?}", ts);
        assert!(n.y + 5 + cap(&f.s) + f.s.descent <= n.y + n.h - 4 - cap(&f.sb), "{:?}: {:?}", ts, n);
        // a strip row holds its text
        let bh = u.lay().strip_row - 2;
        let base = (bh + cap(&f.s)) / 2;
        assert!(base - cap(&f.sb) >= 0 && base + f.s.descent <= bh, "{:?}: strip row", ts);
        // everything on: four channels with six measurements each, cursors, DVM
        for c in 0..NCH {
            u.m.ch[c].on = true;
        }
        u.m.meas = (0..NCH).flat_map(|c| ALL_MEAS.iter().take(6).map(move |&k| (k, c))).collect();
        u.m.cursor = CursorMode::Time;
        u.m.dvm_on = true;
        for _ in 0..3 {
            u.step(0.04);
        }
        u.relayout();
        assert!(u.grid.h >= 160, "{:?}: {:?}", ts, u.grid);
        assert!(u.grid.bottom() + u.strip_rows() * u.lay().strip_row <= u.frame().bottom.y, "{:?}: {:?}", ts, u.grid);
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        // the statistics card stays in the plot (with as many items as the strip shows on one row each)
        let all = std::mem::replace(&mut u.m.meas, ALL_MEAS.iter().take(12).map(|&k| (k, 0)).collect());
        u.stats_on = true;
        u.relayout();
        let (head, body) = u.results_rects().unwrap();
        assert!(head.x >= 0 && body.y + body.h <= u.grid.y + u.grid.h && f.sb.width("Measurements") + 24 + f.s.width("n 999k") + 20 <= head.w, "{:?}", ts);
        u.render(&mut cv);
        (u.stats_on, u.m.meas) = (false, all);
        // the cards in every mix of channels, LA and generator, with long texts
        for c in 0..NCH {
            (u.m.ch[c].offset, u.m.ch[c].probe, u.m.ch[c].invert) = (-12.5, 100.0, true);
        }
        for bits in 0..64 {
            for c in 0..NCH {
                u.m.ch[c].on = bits >> c & 1 == 1;
            }
            (u.m.la.on, u.m.afg_on) = (bits & 16 != 0, bits & 32 != 0);
            let chs = u.cards().0;
            let sub = u.card_sub_font();
            for c in (0..NCH).filter(|&c| u.m.ch[c].on) {
                let (r, t) = (chs[c], u.card_sub(c));
                assert!(sub.width(&t) <= r.w - 22 || normal.s.width(&t) > r.w - 22, "{:?} mix {:b}: CH{} '{}'", ts, bits, c + 1, t);
                let scale = format!("{}/div", si_short(u.m.ch[c].volts_per_div() as f64, "V"));
                assert!(Ui::card_font(f, &scale, r.w - 24).width(&scale) <= r.w - 24 || normal.m.width(&scale) > r.w - 24, "{:?} mix {:b}: CH{}", ts, bits, c + 1);
            }
            u.render(&mut cv);
        }
        // menus under the top bar, on the screen, as high as at the normal size (the rows keep their 44 px)
        use MenuId::*;
        for m in [Main, Channel(0), Horizontal, Trigger, Acquire, Measure, MeasAdd, MeasAddMore, MeasPair, Dvm, Cursor, Display, Utility, Math, Ref, Decode, Storage, Mask, Zone, Spectrum, Eye, Power, Identify, SelfCal, Bode, Afg, BwTest, La, LaLines, LaPat, Network, Remote] {
            u.open_menu(m);
            let d = u.dlg_layout().unwrap();
            assert!(d.r.y >= u.frame().top.bottom() && d.r.bottom() <= u.scr.h && d.r.x >= 0 && d.r.right() <= u.scr.w, "{:?}: {:?} {:?}", ts, m, d.r);
            assert!(d.rows.iter().all(|r| r.ctrl.h >= 40), "{:?}: {:?}", ts, m);
            u.render(&mut cv);
            u.close_menu();
        }
    }
}

#[test]
fn la_and_generator_cards_open_their_pages() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    let tap = |u: &mut Ui, r: Rect| u.tap(r.x + r.w / 2, r.y + r.h / 2);
    // an LA that is off comes on, like a channel; a second tap only closes the page
    let r = u.aux_cards().0.unwrap();
    tap(&mut u, r);
    assert!(u.m.la.on && u.menu_open && u.menu == MenuId::La);
    let r = u.aux_cards().0.unwrap();
    tap(&mut u, r);
    assert!(u.m.la.on && !u.menu_open);
    // the generator page opens, the output stays off
    let r = u.aux_cards().1.unwrap();
    tap(&mut u, r);
    assert!(u.menu_open && u.menu == MenuId::Afg);
    assert!(!u.m.afg_on, "a card must not switch an output on");
}

/// Stage of frames with demo signals on channels 1 and 2.
fn with_two_channels() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.ch[1].on = true;
    for _ in 0..12 {
        u.step(0.04);
    }
    u
}

#[test]
fn fourteen_measurements_with_delay_and_phase() {
    let mut u = with_two_channels();
    u.m.meas.clear();
    for k in ALL_MEAS.iter().take(10) {
        u.m.meas.push((*k, 0));
    }
    u.meas_src_b = 1;
    for k in 0..6 {
        u.meas_pair_toggle(k);
    }
    // never more than the limit: the oldest goes
    assert_eq!(u.m.meas.len() + u.m.meas2.len(), tools::MEAS_MAX);
    let rows = u.meas_rows();
    assert_eq!(rows.len(), tools::MEAS_MAX);
    assert!(rows.iter().any(|r| r.name.starts_with("Delay rise-rise 1-2")));
    // the same channel twice makes no sense
    u.meas_src_b = 0;
    let n = u.m.meas2.len();
    u.meas_pair_toggle(7);
    assert_eq!(u.m.meas2.len(), n);
    // rows wrap in the strip, and the plot keeps its place
    u.relayout();
    assert!(u.strip_rows() >= 3);
    assert!(u.grid.h >= 160);
    // the card of the statistics stays on the screen
    u.stats_on = true;
    let (_, body) = u.results_rects().unwrap();
    assert!(body.y + body.h <= PANEL_H, "{:?}", body);
}

#[test]
fn delay_between_two_edges() {
    let mut u = with_two_channels();
    u.m.meas2 = vec![(0, 0, 1), (4, 0, 1)];
    let r = u.meas_rows();
    assert_eq!(r[r.len() - 2].unit, "s");
    assert_eq!(r[r.len() - 1].unit, "\u{b0}");
    u.m.ch[1].on = false;
    assert!(!u.meas_rows().last().unwrap().v.is_finite());
    assert_eq!(u.meas_rows().last().unwrap().text(), "----");
}

#[test]
fn dvm_and_counter_read_the_source() {
    let mut u = with_two_channels();
    u.m.dvm_on = true;
    u.m.cnt_on = true;
    for _ in 0..4 {
        u.step(0.04);
    }
    assert!(u.dvm_value().is_finite());
    let row = u.dvm_row();
    assert!(row[0].0.starts_with("DVM CH1") && row[1].0.starts_with("Counter CH1"), "{:?}", row);
    assert!(row[1].1.ends_with("Hz"), "{:?}", row);
    assert!(u.cnt_total > 0);
    u.m.cnt_mode = 2;
    assert_ne!(u.dvm_row()[1].1, "----");
    u.counter_clear();
    assert_eq!(u.cnt_total, 0);
    // off source: no number, and the page offers to switch it on
    u.m.dvm_src = 3;
    assert!(!u.dvm_value().is_finite());
    assert!(u.items(MenuId::Dvm).contains(&Item::SrcOn));
}

#[test]
fn tracking_cursors_read_the_trace() {
    let mut u = with_two_channels();
    u.m.cursor = CursorMode::Track;
    let v = u.cursor_values();
    assert_eq!(v.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["X1", "Y1", "X2", "Y2", "\u{394}X", "\u{394}Y"]);
    assert!(v[1].1.ends_with('V') && v[3].1.ends_with('V'), "{:?}", v);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
}

#[test]
fn math_ln_and_linear() {
    let mut u = with_two_channels();
    u.m.math = MathOp::Lin;
    u.m.math_lin = (2.0, 1.0);
    let a = u.math_source(0, 0);
    let w = u.math_wave_of(0);
    assert!(!a.is_empty() && (w[5] - (2.0 * a[5] + 1.0)).abs() < 1e-5);
    assert!(u.items(MenuId::Math).contains(&Item::MathLinA));
    u.m.math = MathOp::Ln;
    let w = u.math_wave_of(0);
    let i = a.iter().position(|v| *v > 0.5).unwrap();
    assert!((w[i] - a[i].ln()).abs() < 1e-4);
}

#[test]
fn ten_references() {
    let mut u = with_two_channels();
    for _ in 0..12 {
        u.activate(Item::RefSave);
    }
    assert_eq!(u.refs.iter().filter(|r| r.is_some()).count(), NREF);
    u.open_menu(MenuId::Ref);
    assert_eq!(u.items(MenuId::Ref).iter().filter(|i| matches!(i, Item::RefShow(_))).count(), NREF);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
}

#[test]
fn a_mask_comes_back_from_its_file() {
    let dir = std::env::temp_dir().join(format!("oos-mask-{}", std::process::id()));
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.activate(Item::MaskCreate);
    let a = u.mask_ref.clone().unwrap();
    u.activate(Item::MaskSave);
    u.mask = None;
    u.mask_ref = None;
    u.activate(Item::MaskLoad);
    assert_eq!(u.mask_ref.as_ref().unwrap().len(), a.len());
    assert!(u.mask.is_some() && u.m.mask_on);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_channel_keeps_its_name() {
    let mut u = with_two_channels();
    u.activate(Item::ChLabel(0));
    assert!(u.wlan.osk.is_some());
    for c in "Vcc".chars() {
        u.osk_type(c);
    }
    u.osk_ok();
    assert!(u.wlan.osk.is_none());
    assert_eq!(u.ch_name(0), "Vcc");
    let t = crate::store::setup_text(&u.m);
    let back = crate::store::parse_setup(&t).unwrap();
    assert_eq!(back.ch_label[0], "Vcc");
}

#[test]
fn holdoff_steps_and_setups() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    assert_eq!(u.value(Item::TrigHoldoff), "Off");
    u.adjust(Item::TrigHoldoff, 1);
    assert!(u.value(Item::TrigHoldoff).ends_with("s"), "{}", u.value(Item::TrigHoldoff));
    u.adjust(Item::TrigHoldoff, -1);
    assert_eq!(u.m.holdoff, 0.0);
    u.adjust(Item::TrigHoldoff, 100);
    assert!(u.m.holdoff <= 10.0);
    let s = crate::board::dho::hw::settings(&u.m, false);
    assert!(s.holdoff > 1.0);
    assert!(crate::board::dho::hw::settings(&Model::default(), false).holdoff == oos_hal::dho::seq::HOLDOFF_MIN);
    for t in TRIG_TYPES {
        u.set_trig_type(t);
        assert!(u.trigger_items().contains(&Item::TrigHoldoff), "{:?}", t);
    }
}

#[test]
fn setups_keep_what_is_new() {
    let mut u = with_two_channels();
    u.m.meas2 = vec![(5, 1, 2)];
    (u.m.dvm_on, u.m.dvm_src, u.m.dvm_mode) = (true, 2, 1);
    (u.m.cnt_on, u.m.cnt_src, u.m.cnt_mode, u.m.cnt_digits) = (true, 3, 2, 6);
    u.m.math_lin = (3.0, -1.5);
    u.m.cursor = CursorMode::Track;
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!((b.meas2.clone(), b.dvm_src, b.dvm_mode, b.cnt_src, b.cnt_mode, b.cnt_digits), (vec![(5, 1, 2)], 2, 1, 3, 2, 6));
    assert_eq!((b.math_lin, b.cursor), ((3.0, -1.5), CursorMode::Track));
}

#[test]
fn four_decoders_side_by_side() {
    let mut u = with_two_channels();
    u.m.ch[2].on = true;
    u.m.dec_on = true;
    u.m.dec_src = 2;
    u.set_dec_proto(Proto::Uart);
    u.m.dec_select(1);
    assert_eq!(u.m.dec_sel, 1);
    assert!(!u.m.dec_on, "a new slot starts off");
    u.m.dec_on = true;
    u.set_dec_proto(Proto::I2c);
    u.m.dec_select(0);
    assert_eq!(u.m.dec_proto, Proto::Uart);
    assert!(u.m.dec_is_on(0) && u.m.dec_is_on(1) && !u.m.dec_is_on(2));
    assert_eq!(u.m.dec_view(1).dec_proto, Proto::I2c);
    for _ in 0..4 {
        u.step(0.04);
    }
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let ev = u.decode_events();
    assert!(ev.iter().any(|e| e.who.starts_with("D1 UART")), "{:?}", &ev[..ev.len().min(3)]);
    assert!(ev.windows(2).all(|w| w[0].t <= w[1].t), "in time order");
    // setups keep all of them
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!(b.dec_sel, 0);
    assert_eq!(b.dec_view(1).dec_proto, Proto::I2c);
    assert!(b.dec_is_on(1));
    assert_eq!(b.dec_proto, Proto::Uart);
}

#[test]
fn decoder_results_are_cached_per_record() {
    let mut u = with_two_channels();
    u.m.ch[2].on = true;
    u.m.dec_on = true;
    u.m.dec_src = 2;
    for _ in 0..4 {
        u.step(0.04);
    }
    u.dec_cache.borrow_mut().clear();
    let a = u.decode_packets().unwrap();
    assert_eq!(u.dec_cache.borrow().len(), 1);
    assert_eq!(u.decode_packets().unwrap(), a);
    assert_eq!(u.dec_cache.borrow().len(), 1);
    // other settings, other result
    u.m.dec_bits = 7;
    let _ = u.decode_packets();
    assert_eq!(u.dec_cache.borrow().len(), 2);
}

#[test]
fn event_table_scrolls_and_goes_to_an_event() {
    let dir = std::env::temp_dir().join(format!("oos-events-{}", std::process::id()));
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.m.ch[2].on = true;
    u.m.dec_on = true;
    u.m.dec_src = 2;
    for _ in 0..4 {
        u.step(0.04);
    }
    u.activate(Item::DecTable);
    assert_eq!(u.table, Some(tables::TableKind::Decode));
    let n = u.table_rows(tables::TableKind::Decode).len();
    assert!(n > 3);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // running: a tap only says so
    let (x, y) = (u.area.x + 100, u.area.y + 8 + 40 + 40);
    let d0 = u.m.delay;
    u.tap(x, y);
    assert_eq!(u.m.delay, d0);
    assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("Stop")));
    // stopped: the view goes there
    u.m.run = RunState::Stop;
    u.tap(x, y);
    assert_ne!(u.m.delay, d0);
    u.activate(Item::DecExport);
    let f = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let t = std::fs::read_to_string(&f).unwrap();
    assert!(t.starts_with("Time (s),Source,Event,Ok") && t.lines().count() == n + 1, "{}", &t[..80.min(t.len())]);
    let _ = std::fs::remove_dir_all(&dir);
    // the close button
    let close = u.area.x + 30 + 420 - 46 + 10;
    u.tap(close, u.area.y + 20);
    assert!(u.table.is_none());
}

#[test]
fn search_list_shows_the_marks() {
    let mut u = with_two_channels();
    u.search_on = true;
    assert!(u.trigger_items().contains(&Item::SearchTable));
    u.activate(Item::SearchTable);
    assert_eq!(u.table, Some(tables::TableKind::Search));
    assert_eq!(u.table_rows(tables::TableKind::Search).len(), u.search_events().len());
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
}

#[test]
fn binary_waveform_can_be_read_back() {
    let u = with_two_channels();
    let rec = u.last[0].as_ref().unwrap();
    let b = crate::store::wave_bin(rec, &u.m);
    assert_eq!(&b[..4], b"OOSW");
    let le32 = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    assert_eq!((le32(4), le32(8)), (1, 2));
    let n = u64::from_le_bytes(b[12..20].try_into().unwrap()) as usize;
    assert_eq!(n, rec.ch[0].len());
    assert_eq!(f64::from_le_bytes(b[20..28].try_into().unwrap()), rec.dt);
    assert_eq!(le32(36), 1);
    assert_eq!(f32::from_le_bytes(b[40..44].try_into().unwrap()), rec.ch[0][0]);
    assert_eq!(le32(40 + 4 * n), 2);
    assert_eq!(b.len(), 36 + 2 * (4 + 4 * n));
}

#[test]
fn second_pane_fits_with_everything_else() {
    for (pane, la, ts, scr) in (1..tables::PANES.len()).flat_map(|p| [false, true].into_iter().flat_map(move |l| TEXT_SIZES.into_iter().flat_map(move |t| SCREENS.map(|s| (p, l, t, s))))) {
        {
            let mut u = with_two_channels();
            u.scr = scr;
            u.m.text_size = ts;
            u.m.pane = pane;
            u.m.la.on = la;
            u.m.cursor = CursorMode::Time;
            u.m.dvm_on = true;
            for _ in 0..3 {
                u.step(0.04);
            }
            u.relayout();
            assert!(u.grid.h >= 160, "pane {} la {}: {:?}", pane, la, u.grid);
            if u.pane_h() > 0 {
                let r = u.pane_rect();
                assert!(r.y > u.grid.y + u.grid.h && r.bottom() + u.strip_rows() * u.lay().strip_row <= u.frame().bottom.y, "pane {} la {}: {:?}", pane, la, r);
            }
            let mut cv = Canvas::new(u.scr.w, u.scr.h);
            u.render(&mut cv);
        }
    }
    // the close button
    let mut u = with_two_channels();
    u.m.pane = 2;
    u.relayout();
    let r = u.pane_rect();
    u.tap(r.x + r.w - 30, r.y + 12);
    assert_eq!(u.m.pane, 0);
    // the spectrum runs beside the scope
    let mut u = with_two_channels();
    u.m.pane = 1;
    for _ in 0..4 {
        u.step(0.04);
    }
    u.analyzer_update();
    assert!(u.azr.spec.is_some());
}

/// Taps and knobs at random with everything new switched on: nothing may panic (a panic ends oos-ui).
#[test]
fn random_input_on_the_new_pages() {
    for scr in SCREENS {
        random_input_on(scr);
    }
}

fn random_input_on(scr: Screen) {
    let mut u = with_two_channels();
    u.scr = scr;
    let mut cv = Canvas::new(scr.w, scr.h);
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let pages = [MenuId::Logger, MenuId::ChMore(0), MenuId::Measure, MenuId::MeasPair, MenuId::Dvm, MenuId::Cursor, MenuId::Math, MenuId::Ref, MenuId::Mask, MenuId::Decode, MenuId::Trigger, MenuId::Display, MenuId::Storage, MenuId::Bode];
    u.store_dir = std::env::temp_dir().join(format!("oos-rand-{}", std::process::id()));
    for i in 0..3_000 {
        if i % 40 == 0 {
            u.open_menu(pages[next() as usize % pages.len()]);
        }
        match next() % 6 {
            0 => u.key([0xba, 0xbb, 0xbc, 0xaa, 0xab, 0xac][next() as usize % 6]),
            1 => u.step(0.02),
            _ => u.tap((next() % scr.w as u64) as i32, (next() % scr.h as u64) as i32),
        }
        if u.wlan.osk.is_some() && next() % 3 == 0 {
            u.wlan.osk = None;
        }
        u.dialog = None;
        u.power_dlg = false;
        u.power_action = None;
        if i % 200 == 0 {
            u.step(0.02);
            u.render(&mut cv);
        }
    }
    let _ = std::fs::remove_dir_all(&u.store_dir);
}

#[test]
fn every_cursor_mode_can_be_chosen() {
    let mut u = with_two_channels();
    for (want, m) in [("Time", CursorMode::Time), ("Volt", CursorMode::Volt), ("Track", CursorMode::Track), ("XY", CursorMode::Xy), ("Off", CursorMode::Off)] {
        u.select_option(Item::CurMode, want);
        assert_eq!(u.m.cursor, m, "{}", want);
    }
}

#[test]
fn xy_cursors_show_both_axes() {
    let mut u = with_two_channels();
    u.m.cursor = CursorMode::Xy;
    let v = u.cursor_values();
    assert_eq!(v.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["X1", "X2", "Y1", "Y2", "\u{394}X", "\u{394}Y"]);
    assert_eq!(u.items(MenuId::Cursor).len(), 5);
    assert!(u.value(Item::CurC).ends_with('V'));
    u.adjust(Item::CurD, 20);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!((b.cursor, b.cur_d), (CursorMode::Xy, u.m.cur_d));
}

#[test]
fn fine_scale_walks_between_the_steps() {
    let mut u = with_two_channels();
    let v0 = u.m.ch[0].volts_per_div();
    u.adjust(Item::ChFine, 1);
    assert!(u.m.fine_v);
    u.adjust(Item::ChScale(0), 1);
    let v1 = u.m.ch[0].volts_per_div();
    assert!((v1 / v0 - 1.02).abs() < 1e-4, "{} {}", v0, v1);
    assert!(u.value(Item::ChScale(0)).contains("1.02") || u.value(Item::ChScale(0)).contains("102"), "{}", u.value(Item::ChScale(0)));
    // up through the next step of the ladder
    for _ in 0..200 {
        u.adjust(Item::ChScale(0), 1);
    }
    assert!(u.m.ch[0].volts_per_div() > 3.0 * v0);
    assert!(u.m.ch[0].vfine >= 1.0 && u.m.ch[0].vfine < 2.6);
    // setups keep it, coarse goes back onto the ladder
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!(b.ch[0].volts_per_div(), u.m.ch[0].volts_per_div());
    u.adjust(Item::ChFine, 1);
    u.adjust(Item::ChScale(0), 1);
    assert_eq!(u.m.ch[0].vfine, 1.0);
    // and the extremes hold
    u.adjust(Item::ChFine, 1);
    u.adjust(Item::ChScale(0), 100_000);
    assert!(u.m.ch[0].volts_per_div().is_finite());
    u.adjust(Item::ChScale(0), -100_000);
    assert!(u.m.ch[0].volts_per_div() > 0.0);
}

#[test]
fn fine_scale_stands_on_every_row_of_the_table() {
    for probe in [1.0, 10.0, 100.0] {
        let mut u = with_two_channels();
        u.m.fine_v = true;
        u.m.ch[0].probe = probe;
        u.adjust(Item::ChScale(0), -100_000);
        for &k in &oos_hal::dho::afe::scale_ladder() {
            // what the hardware path hands the AFE
            let nv = u.m.ch[0].volts_per_div() as f64 / probe as f64 * 1e9;
            assert!((nv / k as f64 - 1.0).abs() < 1e-6, "{} at row {}", nv, k);
            u.adjust(Item::ChScale(0), 1);
        }
    }
}

#[test]
fn the_fine_time_base_leaves_the_acquisition_on_its_step() {
    let mut u = with_two_channels();
    let t0 = u.m.time_per_div();
    u.adjust(Item::HFine, 1);
    u.adjust(Item::HScale, -1);
    assert!((u.m.time_per_div() / t0 - 0.99).abs() < 1e-4);
    assert_eq!(u.m.hw_time_per_div(), t0);
    // the whole way up and down: never a fine value from the roll time base on, and it always moves
    u.adjust(Item::HScale, -100_000);
    let mut n = 0;
    loop {
        let (now, hw) = (u.m.time_per_div(), u.m.hw_time_per_div());
        assert!(hw >= now * 0.9999, "{} {}", now, hw);
        assert!(hw as f64 * 1.0001 < crate::board::board().roll_tdiv() || u.m.tfine == 1.0, "{} {}", now, hw);
        u.adjust(Item::HScale, 1);
        n += 1;
        if u.m.time_per_div() == now {
            break;
        }
        assert!(u.m.time_per_div() > now, "stuck at {}", now);
    }
    assert!(n > 1000, "{}", n);
    for _ in 0..30 {
        let now = u.m.time_per_div();
        u.adjust(Item::HScale, -1);
        assert!(u.m.time_per_div() < now, "stuck at {}", now);
    }
    // setups keep it, coarse goes back onto the step
    u.m.tscale = tscales().iter().position(|&t| (t - 1e-6).abs() < 1e-9).unwrap();
    u.m.tfine = 1.0;
    u.adjust(Item::HScale, -3);
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!(b.time_per_div(), u.m.time_per_div());
    u.adjust(Item::HFine, 1);
    u.adjust(Item::HScale, 1);
    assert_eq!(u.m.tfine, 1.0);
}

#[test]
fn the_expand_point_stays_put() {
    let mut u = with_two_channels();
    u.m.delay = 1e-4;
    let at = |m: &Model, r: f32| m.delay + r * HDIV * m.time_per_div();
    for (e, r) in [(HExpand::Left, -0.5), (HExpand::Right, 0.5), (HExpand::Center, 0.0)] {
        u.m.h_expand = e;
        let t = at(&u.m, r);
        u.adjust(Item::HScale, 1);
        assert!((at(&u.m, r) - t).abs() < 1e-6 * t.abs().max(1e-3), "{:?}", e);
    }
    u.m.h_expand = HExpand::Trigger;
    let f = u.m.delay / u.m.time_per_div();
    u.adjust(Item::HScale, -2);
    assert!((u.m.delay / u.m.time_per_div() / f - 1.0).abs() < 1e-5);
    // the vertical one: where ground is on the screen, in div
    u.m.ch[0].offset = 1.5;
    let div = |m: &Model| m.ch[0].offset / m.ch[0].volts_per_div();
    let (d0, o0) = (div(&u.m), u.m.ch[0].offset);
    u.adjust(Item::ChScale(0), 1);
    assert_eq!(u.m.ch[0].offset, o0, "center is the default");
    u.adjust(Item::ChScale(0), -1);
    u.adjust(Item::ChExpand, 1);
    u.adjust(Item::ChScale(0), 1);
    assert!((div(&u.m) / d0 - 1.0).abs() < 1e-5);
}

#[test]
fn a_channel_can_measure_something_else() {
    let mut u = with_two_channels();
    u.select_option(Item::ChUnit(0), "A");
    assert!(u.value(Item::ChScale(0)).ends_with('A'), "{}", u.value(Item::ChScale(0)));
    assert_eq!(AmpUnit::W.of("V/s"), "W/s");
    assert_eq!(AmpUnit::A.of("Hz"), "Hz");
    u.m.meas = vec![(Meas::Vpp, 0), (Meas::Vpp, 1)];
    let r = u.meas_rows();
    assert_eq!((r[0].unit, r[1].unit), ("A", "V"));
    u.adjust(Item::ChSkew(0), 5);
    assert!((u.m.ch[0].skew - 3e-9).abs() < 1e-12);
    u.adjust(Item::ChSkew(0), 100_000);
    assert_eq!(u.m.ch[0].skew, SKEW_MAX);
    u.adjust(Item::ChBias(0), 3);
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!((b.ch[0].unit, b.ch[0].bias, b.ch[0].skew), (AmpUnit::A, u.m.ch[0].bias, SKEW_MAX));
    assert_eq!(crate::board::dho::hw::settings(&u.m, false).ch[0].skew, SKEW_MAX as f64);
    // the mean of the input goes into the correction
    for _ in 0..24 {
        u.step(0.04);
    }
    u.m.ch[0].bias = 0.0;
    let mean = |u: &Ui| { let v = &u.last[0].as_ref().unwrap().ch[0]; v.iter().sum::<f32>() / v.len() as f32 };
    let m0 = mean(&u);
    u.activate(Item::ChBiasZero(0));
    assert!((u.m.ch[0].bias - m0).abs() < 1e-3, "{} {}", u.m.ch[0].bias, m0);
}

#[test]
fn old_setups_at_20_volts_land_on_the_top_step() {
    let t = crate::store::setup_text(&Model::default());
    let top = VSCALES.len() - 1;
    let m = crate::store::parse_setup(&t.replace("ch1.vdiv = 1\n", "ch1.vdiv = 20\n")).unwrap();
    assert_eq!(m.ch[0].vscale, top);
    assert_eq!(VSCALES[top], 10.0);
    let m = crate::store::parse_setup(&t.replace("ch1.vdiv = 1\n", "ch1.vscale = 14\n")).unwrap();
    assert_eq!(m.ch[0].vscale, top);
}

#[test]
fn what_the_screen_says_reaches_the_settings() {
    let hw = |m: &Model| crate::board::dho::hw::settings(m, false);
    let mut m = Model::default();
    // SINGLE and NORMAL wait for a trigger, only AUTO fires by itself
    for (t, auto) in [(TrigMode::Auto, true), (TrigMode::Normal, false), (TrigMode::Single, false)] {
        m.trig_mode = t;
        assert_eq!(hw(&m).auto, auto, "{:?}", t);
    }
    m.trig_mode = TrigMode::Auto;
    m.ch[0].coupling = Coupling::Ac;
    m.ch[0].invert = true;
    m.acq = AcqMode::Peak;
    m.delay = 2e-4;
    let s = hw(&m);
    assert_eq!((s.ch[0].couple, s.ch[0].invert, s.mode), (oos_hal::dho::seq::Couple::Ac, true, oos_hal::dho::acq::Mode::Peak));
    assert!((s.delay - 2e-4).abs() < 1e-9);
    // a trigger source that is switched off is acquired all the same, and keeps its role
    let mut m = Model::default();
    m.trig_src = 1;
    assert_eq!((hw(&m).mask, hw(&m).trig_src), (0b11, 2));
    m.trig_src = 0;
    m.trig_type = TrigType::Delay;
    m.trig_src2 = 3;
    assert_eq!(hw(&m).mask, 0b1001);
    // edges left over from Pattern are no edges for Duration
    let mut m = Model::default();
    m.pat[0] = Bit::R;
    m.trig_type = TrigType::Duration;
    match hw(&m).trigger {
        oos_hal::dho::seq::Trig::Duration { pats, .. } => assert_eq!(pats[0], oos_hal::dho::tpu::Pat::X),
        t => panic!("{:?}", t),
    }
    // roll and scan: no depth below 1 M, the delay stays out of it; a small depth at the slowest time base is no crash
    let mut m = Model::default();
    m.tscale = tscales().len() - 1;
    m.mdepth = mdepths().iter().position(|d| d.1 == 1000).unwrap();
    m.delay = 1.0;
    let s = hw(&m);
    assert!(s.depth >= 1_000_000 && s.delay == 0.0, "{}", s.depth);
    oos_hal::dho::hz::prep(1, oos_hal::dho::seq::tdiv_fs(1000.0), 1000, 0);
}

#[test]
fn the_offset_stays_in_the_range_of_the_scale() {
    let mut u = with_two_channels();
    let at = |v: f32| VSCALES.iter().position(|&s| (s - v).abs() < v * 1e-3).unwrap();
    u.m.ch[0].vscale = at(0.1);
    u.m.ch[0].offset = 3.0;
    u.step(0.04);
    assert_eq!(u.m.ch[0].offset, 3.0);
    u.m.ch[0].vscale = at(0.05);
    u.step(0.04);
    assert_eq!(u.m.ch[0].offset, 2.0);
    // at the tip, behind a probe, ten times that
    u.m.ch[0].probe = 10.0;
    u.m.ch[0].offset = -30.0;
    u.step(0.04);
    assert_eq!(u.m.ch[0].offset, -20.0);
}

#[test]
fn scpi_errors_and_answers_stay_bounded() {
    let mut u = with_two_channels();
    let cv = Canvas::new(PANEL_W, PANEL_H);
    let mut s = crate::scpi::Scpi::open(0);
    for _ in 0..1000 {
        s.exec_line(&mut u, &cv, &format!(":NOPE{}", "x".repeat(10_000)));
    }
    let mut n = 0;
    let mut last = String::new();
    loop {
        let e = String::from_utf8_lossy(&s.exec_line(&mut u, &cv, ":SYST:ERR?")).into_owned();
        if e.starts_with("0,") {
            break;
        }
        assert!(e.len() < 300, "{}", e.len());
        last = e;
        n += 1;
    }
    assert_eq!(n, 20);
    assert!(last.contains("-350"), "{}", last);
    // the holdoff is taken without an error
    s.exec_line(&mut u, &cv, ":TRIG:HOLD 0.001");
    assert!(String::from_utf8_lossy(&s.exec_line(&mut u, &cv, ":SYST:ERR?")).starts_with("0,"));
    assert!((u.m.holdoff - 1e-3).abs() < 1e-6);
}

#[test]
fn a_remote_knob_turn_has_a_limit() {
    let mut u = with_two_channels();
    let d0 = u.m.delay;
    let t = std::time::Instant::now();
    crate::cmd::run(&mut u, "knob hpos 2000000000");
    assert!(t.elapsed().as_secs() < 5);
    assert!(u.m.delay != d0);
}

#[test]
fn the_event_table_can_be_reached_from_the_menu() {
    let u = with_two_channels();
    let v = u.items(MenuId::Decode);
    assert!(v.contains(&Item::DecTable) && v.contains(&Item::DecExport));
}

#[test]
fn a_formula_is_a_math_channel() {
    let mut u = with_two_channels();
    for _ in 0..24 {
        u.step(0.04);
    }
    u.m.math = MathOp::Expr;
    assert!(u.math_wave().is_empty(), "no formula yet");
    assert_eq!(u.value(Item::MathExprI), "none");
    u.activate(Item::MathExprEdit);
    u.wlan.osk.as_mut().unwrap().text = "CH1*(2+".into();
    u.osk_done();
    assert!(u.wlan.osk.is_some(), "a bad formula stays on the keyboard");
    assert!(u.m.math_expr[0].is_empty());
    u.wlan.osk.as_mut().unwrap().text = "2*CH1-CH2".into();
    u.osk_done();
    assert!(u.wlan.osk.is_none());
    let (rec, w) = (u.last[0].clone().unwrap(), u.math_wave());
    assert_eq!(w.len(), rec.ch[0].len().min(rec.ch[1].len()));
    assert!((w[5] - (2.0 * rec.ch[0][5] - rec.ch[1][5])).abs() < 1e-4);
    // a formula with no channel in it runs along the record
    u.m.math_expr[0] = "t".into();
    assert!((u.math_wave()[1] as f64 - rec.t0 - rec.dt).abs() < rec.dt * 1e-3 + 1e-9);
    u.m.math_expr[0] = "2*CH1-CH2".into();
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!((b.math, b.math_expr[0].as_str()), (MathOp::Expr, "2*CH1-CH2"));
    assert_eq!(u.items(MenuId::Math).iter().filter(|i| **i == Item::MathA).count(), 0);
}

#[test]
fn the_logger_writes_rows_at_its_interval() {
    let dir = std::env::temp_dir().join(format!("oos-log-{}", std::process::id()));
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.m.meas = vec![];
    u.m.meas2 = vec![];
    u.adjust(Item::LogOn, 1);
    assert_eq!(u.value(Item::LogOn), "Off", "nothing to log");
    u.m.meas = vec![(Meas::Vpp, 0), (Meas::Freq, 1)];
    for _ in 0..3 {
        u.step(0.04);
    }
    u.adjust(Item::LogOn, 1);
    assert_eq!(u.value(Item::LogOn), "On");
    for _ in 0..130 {
        u.step(0.04);
    }
    assert!(u.value(Item::LogInfo).contains("rows"));
    u.adjust(Item::LogOn, 1);
    let name = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "csv")).unwrap();
    let t = std::fs::read_to_string(&name).unwrap();
    let l: Vec<&str> = t.lines().collect();
    assert!(l[0].starts_with("Elapsed (s),Unix time (s),") && l[0].contains("CH2 (Hz)"), "{}", l[0]);
    assert!((5..=7).contains(&l.len()), "5.2 s at 1 s: {} lines", l.len());
    assert_eq!(l[1].split(',').count(), 4);
    assert!(l[1].split(',').nth(2).unwrap().parse::<f64>().is_ok());
    // a changed list ends the log, it no longer fits the head
    u.adjust(Item::LogOn, 1);
    u.step(1.1);
    u.m.meas.pop();
    u.step(1.1);
    assert!(u.log.file.is_none());
    assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("changed")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_trend_pane_follows_a_measurement() {
    let mut u = with_two_channels();
    u.m.meas = vec![(Meas::Vpp, 0), (Meas::Freq, 0)];
    u.m.pane = 4;
    for _ in 0..40 {
        u.step(0.04);
    }
    assert_eq!(u.trend.len(), 2);
    assert!(u.trend[0].pts.len() > 5);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // the first two of the list side by side; a tap moves a chart on
    assert_eq!(u.trend_charts(), [(0, 0), (1, 1)]);
    let r = u.pane_rect();
    assert!(u.pane_tap(r.x + 100, r.y + 40));
    assert_eq!(u.trend_charts(), [(0, 1), (1, 1)]);
    u.render(&mut cv);
    // a shorter list: the row and the trends follow
    u.m.meas.pop();
    u.mstat.clear();
    u.step(0.04);
    u.step(0.04);
    u.render(&mut cv);
    assert_eq!(u.trend.len(), 1);
    u.m.meas.clear();
    u.render(&mut cv);
}

/// A record as the hardware delivers it: a 200 kHz square, 70 k points at 1.25 GSa/s behind the screen record.
fn square_on_ch1(u: &mut Ui) {
    let dt = 0.8e-9;
    let n = 70_000;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    };
    let v: Vec<f32> = (0..n).map(|i| if (i as f64 * dt * 200e3).rem_euclid(1.0) < 0.5 { 2.0 } else { 0.0 } + 4e-3 * noise()).collect();
    u.m.tscale = tscales().iter().position(|&x| (x - 5e-6).abs() < 1e-9).unwrap();
    u.m.meas = vec![(Meas::Freq, 0), (Meas::Vpp, 0)];
    u.m.run = RunState::Stop;
    u.step(0.04);
    let mut r = u.last[0].clone().expect("a record");
    r.dt = dt;
    r.t0 = -28e-6;
    r.ch[0] = v;
    u.last[0] = Some(r);
    u.measure_last();
}

/// The list as the table shows it, in Hz.
fn peak_hz(u: &Ui) -> Vec<f64> {
    u.azr.peaks.iter().map(|p| p.f).collect()
}

#[test]
fn the_pane_follows_a_square_wave_and_finds_its_harmonics() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.pane = tables::PANE_SPECTRUM;
    u.step(0.04);
    square_on_ch1(&mut u);
    u.relayout();
    u.analyzer_update();
    let sp = u.azr.spec.as_ref().unwrap();
    assert_eq!(sp.freq(sp.dbv.len() - 1).round(), 2.5e6);
    assert!(sp.rbw < 35e3, "RBW {}", sp.rbw);
    assert_eq!(u.value(Item::SpecSpan), "Auto 2.5 MHz");
    let f = peak_hz(&u);
    assert!(f.len() >= 6 && (f[0] - 200e3).abs() < 1e3, "{:?}", f);
    for (k, x) in f.iter().take(6).enumerate() {
        assert!((x / ((2 * k + 1) as f64 * 200e3) - 1.0).abs() < 0.01, "{:?}", f);
    }
    // the user's span wins; the full view keeps the whole band and still finds the fundamental
    u.m.spec_span = 1e6;
    u.m.spec_center = 5e5;
    u.analyzer_update();
    assert_eq!(u.azr.spec.as_ref().unwrap().freq(0), 0.0);
    assert!(u.azr.spec.as_ref().unwrap().freq(u.azr.spec.as_ref().unwrap().dbv.len() - 1) <= 1e6 + 1.0);
    u.m.spec_span = 0.0;
    u.m.view = AView::Spectrum;
    u.relayout();
    u.analyzer_update();
    // the full view follows the signal too, All shows the whole band
    assert!(u.value(Item::SpecSpan).starts_with("Auto"), "{}", u.value(Item::SpecSpan));
    assert!((u.strongest_line().unwrap().f - 200e3).abs() < 1e3);
    u.m.spec_harm = PANE_HARMS.iter().position(|&h| h == 0).unwrap();
    u.analyzer_update();
    assert_eq!(u.value(Item::SpecSpan), "Full");
    assert!((u.strongest_line().unwrap().f - 200e3).abs() < 1e3);
}

#[test]
fn the_sort_button_works_by_touch_in_pane_and_view() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.pane = tables::PANE_SPECTRUM;
    u.step(0.04);
    square_on_ch1(&mut u);
    u.relayout();
    u.analyzer_update();
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let by_f = peak_hz(&u);
    assert!(by_f.windows(2).all(|w| w[0] < w[1]));
    let b = u.spec_box(u.pane_rect(), true).sort;
    u.tap(b.x + 5, b.y + 5);
    assert_eq!(u.m.spec_sort, 1);
    assert!(u.azr.peaks.windows(2).all(|w| w[0].dbv >= w[1].dbv));
    // the view: under the overflow button, which stays free
    u.m.view = AView::Spectrum;
    u.relayout();
    u.analyzer_update();
    u.render(&mut cv);
    let b = u.spec_box(u.area, false);
    assert!(!overlaps(b.sort, u.view_btn()) && !overlaps(b.list, u.view_btn()) && !overlaps(b.plot, u.view_btn()) && !overlaps(b.title, u.view_btn()));
    u.tap(b.sort.x + 5, b.sort.y + 5);
    assert_eq!(u.m.spec_sort, 0);
    let v = u.view_btn();
    u.tap(v.x + 5, v.y + 5);
    assert!(u.menu_open && u.menu == MenuId::ViewMore);
    // the close button of the pane too
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.pane = tables::PANE_SPECTRUM;
    u.step(0.04);
    u.relayout();
    let r = u.pane_rect();
    assert!(!overlaps(u.spec_box(r, true).sort, Ui::pane_close(r)));
    u.tap(r.x + r.w - 30, r.y + 12);
    assert_eq!(u.m.pane, 0);
}

/// Everything the panes write stays inside its pane, and in the width it was given.
#[test]
fn pane_text_fits() {
    let f = crate::font::Fonts::get(0);
    // the widest cell against the narrowest column, with the side column open
    let x = panes::tests_result_cols(1024 - 308 - 16);
    let w = |k: usize| x[k + 1] - x[k] - 8;
    assert!(f.sb.width("CH1-2") <= w(0));
    for &k in ALL_MEAS.iter() {
        assert!(f.s.width(k.name()) <= w(1), "{}", k.name());
    }
    for (n, ..) in tools::PAIRS {
        assert!(f.s.width(&format!("{} 1-2", n)) <= x[2] - x[1] - 6, "{}", n);
    }
    for k in 2..7 {
        assert!(f.sb.width("-1.250 mV") <= w(k) && f.sb.width("199.9 kHz") <= w(k));
    }
    assert!(f.s.width("12.34 M") <= w(7));
    // the peak list
    assert!(f.sb.width("625.00 MHz") + f.s.width("-123.4 dBV") + 26 <= 206);
    // a trend card
    assert!(f.sb.width("-1.250 mV") + f.s.width("Max") + 16 <= 128);
}

/// A pane uses its height: a short list makes it lower and the plot larger, and the rows reach its end.
#[test]
fn panes_leave_no_empty_rows() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.pane = tables::PANE_RESULTS;
    u.m.meas = vec![(Meas::Vpp, 0)];
    u.m.ch[0].on = false;
    u.step(0.04);
    u.relayout();
    let few = u.pane_h();
    let plot = u.grid.h;
    let (fit, pitch) = Ui::results_fit(u.pane_rect(), 1);
    assert_eq!(fit, 1);
    assert!(u.pane_rect().h - 40 - pitch < 4);
    for &k in ALL_MEAS.iter().take(12) {
        u.m.meas.push((k, 0));
    }
    u.relayout();
    assert!(u.pane_h() > few && u.grid.h < plot);
    let (fit, pitch) = Ui::results_fit(u.pane_rect(), 13);
    assert!(fit < 13 && u.pane_rect().h - 40 - fit as i32 * pitch < 8, "{} rows of {}", fit, pitch);
    // nothing to show: a line of text, not a black box
    u.m.meas.clear();
    u.relayout();
    assert!(u.pane_h() <= 70);
    u.m.pane = tables::PANE_TREND;
    u.relayout();
    assert!(u.pane_h() <= 70);
    u.m.pane = tables::PANE_EVENTS;
    u.relayout();
    assert!(u.pane_h() <= 70);
}

#[test]
fn a_long_results_list_scrolls_by_touch() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.pane = tables::PANE_RESULTS;
    u.m.meas = ALL_MEAS.iter().take(14).map(|&k| (k, 0)).collect();
    for _ in 0..3 {
        u.step(0.04);
    }
    u.relayout();
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let r = u.pane_rect();
    let (fit, _) = Ui::results_fit(r, 14);
    let [up, down] = Ui::results_scroll_btns(r);
    u.tap(down.x + 5, down.y + 5);
    assert_eq!(u.pn.top, fit.min(14 - fit));
    u.tap(down.x + 5, down.y + 5);
    assert_eq!(u.pn.top, 14 - fit);
    u.render(&mut cv);
    u.tap(up.x + 5, up.y + 5);
    assert!(u.pn.top < 14 - fit);
    // a finger up the table goes down the list
    u.pn.top = 0;
    crate::cmd::run(&mut u, &format!("drag 300 {} 300 {}", r.y + r.h - 10, r.y + 50));
    assert!(u.pn.top > 0);
    // still there after the list got shorter
    u.m.meas.truncate(2);
    u.relayout();
    u.render(&mut cv);
}

#[test]
fn the_results_pane_shows_cursors_and_peaks() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.pane = tables::PANE_RESULTS;
    u.m.cursor = CursorMode::Time;
    u.step(0.04);
    square_on_ch1(&mut u);
    u.relayout();
    u.analyzer_update();
    assert!(!u.azr.peaks.is_empty());
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // the pane's own close button still works
    let r = u.pane_rect();
    u.tap(r.x + r.w - 30, r.y + 12);
    assert_eq!(u.m.pane, 0);
}

#[test]
fn a_trend_scale_holds_on_noise_and_widens_for_a_step() {
    let mut t = panes::Trend::default();
    let mut seed = 1u32;
    let mut now = 0.0;
    let mut scales = BTreeSet::new();
    let mut run = |t: &mut panes::Trend, now: &mut f64, level: f64, secs: f64, scales: &mut BTreeSet<String>| {
        for _ in 0..(secs / 0.05) as usize {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (seed >> 8) as f64 / (1u32 << 24) as f64 - 0.5;
            t.push(*now, level + noise * 10.0);
            *now += 0.05;
            if *now > 3.0 {
                scales.insert(format!("{:?}", t.scale));
            }
        }
    };
    run(&mut t, &mut now, 1000.0, 20.0, &mut scales);
    assert!(scales.len() <= 3, "{:?}", scales);
    let (lo, hi, tick) = t.scale.unwrap();
    assert!(lo < 995.0 && hi > 1005.0 && hi - lo < 60.0 && tick > 0.0, "{:?}", t.scale);
    // one stray value does not set the scale
    t.push(now, 1e6);
    now += 0.2;
    run(&mut t, &mut now, 1000.0, 2.0, &mut scales);
    assert!(t.scale.unwrap().1 < 2000.0, "{:?}", t.scale);
    // a step does, at once
    run(&mut t, &mut now, 2000.0, 1.0, &mut scales);
    assert!(t.scale.unwrap().1 > 2000.0, "{:?}", t.scale);
}

#[test]
fn a_trend_keeps_its_length_and_reaches_back() {
    let mut t = panes::Trend::default();
    for i in 0..200_000 {
        t.push(i as f64 * 0.05, 1.0);
    }
    assert!(t.pts.len() <= 480);
    let age = t.pts.back().unwrap().t - t.pts.front().unwrap().t;
    assert!(age > 9000.0, "{} s", age);
    // a gap stays a gap
    let mut t = panes::Trend::default();
    for i in 0..40 {
        t.push(i as f64 * 0.05, if i < 20 { 1.0 } else { f64::NAN });
    }
    assert!(t.pts.iter().any(|p| p.v.is_nan()) && t.range == Some((1.0, 1.0)));
}

#[test]
fn trend_charts_are_chosen_in_the_display_menu() {
    let mut u = with_two_channels();
    u.m.meas = vec![(Meas::Vpp, 0), (Meas::Freq, 0), (Meas::Vavg, 1)];
    assert!(!u.items(MenuId::Display).contains(&Item::TrendSel(0)));
    u.m.pane = tables::PANE_TREND;
    assert!(u.items(MenuId::Display).contains(&Item::TrendSel(1)));
    assert_eq!(u.value(Item::TrendSel(0)), "Auto");
    u.adjust(Item::TrendSel(1), 1);
    assert_eq!(u.value(Item::TrendSel(1)), "Off");
    assert_eq!(u.trend_charts(), [(0, 0)]);
    u.adjust(Item::TrendSel(1), 3);
    assert_eq!(u.value(Item::TrendSel(1)), "CH2 Vavg".replace("Vavg", Meas::Vavg.name()));
    assert_eq!(u.trend_charts(), [(0, 0), (1, 2)]);
    // the list gets shorter: back to the default
    u.m.meas.truncate(1);
    assert_eq!(u.trend_charts(), [(0, 0)]);
    assert_eq!(u.value(Item::TrendSel(1)), "Auto");
}

/// Pictures of the panes for looking at: `OOS_PANES=dir cargo test pane_pictures -- --ignored`.
#[test]
#[ignore]
fn pane_pictures() {
    let Ok(dir) = std::env::var("OOS_PANES") else { return };
    let dir = std::path::PathBuf::from(dir);
    let shot = |u: &mut Ui, n: &str| {
        u.relayout();
        u.analyzer_update();
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&dir.join(format!("{}.png", n)), &crate::store::png_of(&cv));
    };
    let square = |pane: usize, f: &dyn Fn(&mut Ui)| {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.pane = pane;
        u.step(0.04);
        square_on_ch1(&mut u);
        f(&mut u);
        u
    };
    let mut u = square(1, &|_| {});
    shot(&mut u, "spectrum-pane");
    let mut u = square(1, &|u| u.m.spec_sort = 1);
    shot(&mut u, "spectrum-pane-by-level");
    let mut u = square(0, &|u| u.m.view = AView::Spectrum);
    shot(&mut u, "spectrum-view-full-band");
    let mut u = square(0, &|u| { u.m.view = AView::Spectrum; u.m.spec_span = 2.5e6; u.m.spec_center = 1.25e6; u.m.spec_dist = true; });
    shot(&mut u, "spectrum-view-span");
    let mut u = square(0, &|u| { u.m.view = AView::Spectrum; u.m.spec_span = 2.5e6; u.m.spec_center = 1.25e6; u.m.spec_wf = true; });
    for _ in 0..10 { u.analyzer_update(); }
    shot(&mut u, "spectrum-view-waterfall");
    let mut u = square(2, &|_| {});
    shot(&mut u, "results-two-rows");
    let mut u = square(2, &|u| { u.m.spec_src = 1; });
    shot(&mut u, "results-table-only");
    let live = |pane: usize, f: &dyn Fn(&mut Ui), secs: usize| {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.pane = pane;
        u.m.ch[1].on = true;
        f(&mut u);
        for _ in 0..secs * 25 {
            u.step(0.04);
        }
        u
    };
    let mut u = live(2, &|u| {
        u.m.cursor = CursorMode::Track;
        u.m.meas = ALL_MEAS.iter().take(12).map(|&k| (k, 0)).collect();
        u.m.meas2 = vec![(0, 0, 1), (4, 0, 1)];
    }, 2);
    shot(&mut u, "results-many");
    // a protocol module's own view (the example module, compiled in the tests)
    let mut u = live(tables::PANE_BUS, &|u| { u.set_dec_proto(Proto::from_key("Example").unwrap()); u.m.dec_on = true; u.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap(); }, 1);
    shot(&mut u, "bus-view");
    let mut u = live(tables::PANE_BUS, &|_| {}, 1);
    shot(&mut u, "bus-view-empty");
    // DCC: the demo layout, collected over two seconds of records
    let mut u = live(0, &|u| { u.set_dec_proto(Proto::from_key("Dcc").unwrap()); u.m.dec_on = true; u.m.tscale = tscales().iter().position(|&x| (x - 2e-3).abs() < 1e-6).unwrap(); }, 3);
    shot(&mut u, "bus-view-dcc");
    let mut u = live(2, &|u| u.m.meas = vec![(Meas::Vpp, 0), (Meas::Freq, 1), (Meas::Vrms, 1)], 2);
    shot(&mut u, "results-three");
    let mut u = live(2, &|u| { u.m.meas = vec![(Meas::Vpp, 0)]; u.m.ch[0].on = true; u.m.spec_src = 3; }, 2);
    shot(&mut u, "results-one-no-side");
    let mut u = live(4, &|u| u.m.meas = vec![(Meas::Vpp, 0), (Meas::Freq, 1)], 40);
    shot(&mut u, "trend-two");
    let mut u = live(4, &|u| u.m.meas = vec![(Meas::Vrms, 1)], 15);
    shot(&mut u, "trend-one");
    let mut u = live(3, &|u| { u.m.ch[2].on = true; u.m.dec_on = true; u.m.dec_src = 2; }, 2);
    shot(&mut u, "events");
    let mut u = live(4, &|_| {}, 1);
    u.m.meas.clear();
    shot(&mut u, "trend-empty");
}

#[test]
fn a_spectrum_limit_counts_what_goes_over() {
    let mut u = with_two_channels();
    u.m.view = AView::Spectrum;
    u.activate(Item::SpecLimitSet);
    assert!(u.azr.limit.is_none());
    for _ in 0..24 {
        u.step(0.04);
    }
    u.adjust(Item::SpecMargin, 4);
    assert_eq!(u.value(Item::SpecMargin), "14 dB");
    u.activate(Item::SpecLimitSet);
    assert!(u.azr.limit.is_some());
    for _ in 0..24 {
        u.step(0.04);
    }
    assert!(u.azr.limit_n > 0);
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // a limit far below the trace: every one goes over
    for p in u.azr.limit.as_mut().unwrap() {
        p.1 -= 200.0;
    }
    (u.azr.limit_n, u.azr.limit_over) = (0, 0);
    for _ in 0..24 {
        u.step(0.04);
    }
    assert!(u.azr.limit_n > 0 && u.azr.limit_over == u.azr.limit_n);
    u.render(&mut cv);
    u.activate(Item::SpecLimitClear);
    assert!(u.azr.limit.is_none());
}

#[test]
fn a_reference_has_its_colour_and_name() {
    let mut u = with_two_channels();
    for _ in 0..24 {
        u.step(0.04);
    }
    u.activate(Item::RefLabel);
    assert!(u.wlan.osk.is_none(), "an empty slot has no name");
    u.activate(Item::RefSave);
    u.adjust(Item::RefColor, 2);
    assert_eq!(u.value(Item::RefColor), "Green");
    u.activate(Item::RefLabel);
    u.wlan.osk.as_mut().unwrap().text = "golden".into();
    u.osk_done();
    assert_eq!(u.refs[0].as_ref().unwrap().label, "golden");
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
}

#[test]
fn a_reference_travels_through_a_file() {
    let dir = std::env::temp_dir().join(format!("oos-ref-{}", std::process::id()));
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.activate(Item::RefSave);
    let a = u.refs[0].as_ref().unwrap().v.clone();
    u.activate(Item::RefExport);
    u.ref_sel = 4;
    u.activate(Item::RefExport);
    assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("empty")));
    std::fs::rename(dir.join("ref1.csv"), dir.join("ref5.csv")).unwrap();
    u.activate(Item::RefImport);
    let r = u.refs[4].as_ref().unwrap();
    assert_eq!(r.v.len(), a.len());
    assert!((r.v[3] - a[3]).abs() < 1e-5);
    assert!(r.dt > 0.0);
    // scale and offset of the slot
    u.adjust(Item::RefVpd, 1);
    u.adjust(Item::RefOffset, 3);
    assert_ne!(u.value(Item::RefOffset), "-");
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    // a file that is not one
    std::fs::write(dir.join("ref5.csv"), "nothing here\n").unwrap();
    u.activate(Item::RefImport);
    assert!(u.toast.as_ref().is_some_and(|t| t.0.contains("no waveform")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every screen as a picture, for looking at: `OOS_GALLERY=dir cargo test gallery -- --ignored`, one directory per
/// text size (`OOS_TEXT_SIZE=Large` for one of them; `OOS_SIZE=1920x1080` for another screen than the panel).
#[test]
#[ignore]
fn gallery() {
    let Ok(base) = std::env::var("OOS_GALLERY") else { return };
    let only = std::env::var("OOS_TEXT_SIZE").ok();
    for ts in TEXT_SIZES.into_iter().filter(|t| only.as_deref().map_or(true, |o| o == t.name())) {
        gallery_at(&std::path::PathBuf::from(&base).join(ts.name().to_lowercase()), ts);
    }
}

fn gallery_at(dir: &std::path::Path, ts: TextSize) {
    use MenuId as M;
    let _ = std::fs::create_dir_all(dir);
    let scr = std::env::var("OOS_SIZE").ok().and_then(|s| Screen::parse(&s)).unwrap_or(Screen::PANEL);
    let new = || {
        let mut u = demo_on(scr);
        u.m.text_size = ts;
        u.relayout();
        u
    };
    let k = |u: &mut Ui, s: &str| {
        crate::cmd::run(u, &format!("key {}", s));
    };
    type S = (&'static str, fn(&mut Ui));
    let menus: Vec<(&str, M)> = vec![("menu-main", M::Main), ("menu-ch", M::Channel(0)), ("menu-hor", M::Horizontal), ("menu-trig", M::Trigger), ("menu-acq", M::Acquire), ("menu-measure", M::Measure), ("menu-measadd", M::MeasAdd), ("menu-cursor", M::Cursor), ("menu-display", M::Display), ("menu-system", M::Utility), ("menu-math", M::Math), ("menu-ref", M::Ref), ("menu-decode", M::Decode), ("menu-storage", M::Storage), ("menu-mask", M::Mask), ("menu-zone", M::Zone), ("menu-spectrum", M::Spectrum), ("menu-eye", M::Eye), ("menu-power", M::Power), ("menu-identify", M::Identify), ("menu-selfcal", M::SelfCal), ("menu-bode", M::Bode), ("menu-afg", M::Afg), ("menu-bw", M::BwTest), ("menu-la", M::La), ("menu-lalines", M::LaLines), ("menu-lapat", M::LaPat), ("menu-network", M::Network), ("menu-remote", M::Remote), ("menu-time", M::Time)];
    let shot = |u: &mut Ui, n: &str| {
        for _ in 0..24 {
            u.step(0.04);
        }
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let _ = crate::store::write(&dir.join(format!("{}.png", n)), &crate::store::png_of(&cv));
    };
    for (n, m) in menus {
        let mut u = new();
        u.open_menu(m);
        shot(&mut u, n);
    }
    let scenes: Vec<S> = vec![
        ("s-default", |_| {}),
        ("s-all-ch", |u| for c in 0..4 { u.m.ch[c].on = true; }),
        ("s-meas6", |u| { u.m.meas.clear(); for i in 0..6 { u.m.meas.push((ALL_MEAS[i], 0)); } }),
        ("s-cursor", |u| u.m.cursor = CursorMode::Time),
        ("s-more", |u| {
            u.m.ch[0].unit = AmpUnit::A;
            u.open_menu(MenuId::ChMore(0));
        }),
        ("s-logger", |u| u.open_menu(MenuId::Logger)),
        ("s-trend", |u| {
            u.m.pane = 4;
            for _ in 0..60 {
                u.step(0.04);
            }
        }),
        ("s-formula", |u| {
            u.m.math = MathOp::Expr;
            u.m.math_expr[0] = "CH1*sin(2*pi*5000*t)".into();
            u.m.math_scale = 6;
            for _ in 0..4 {
                u.step(0.04);
            }
            u.open_menu(MenuId::Math);
        }),
        ("s-limit", |u| {
            u.m.view = AView::Spectrum;
            for _ in 0..12 {
                u.step(0.04);
            }
            u.activate(Item::SpecLimitSet);
        }),
        ("s-cursor-xy", |u| {
            u.m.cursor = CursorMode::Xy;
            u.open_menu(MenuId::Cursor);
        }),
        ("s-fine", |u| {
            u.m.fine_v = true;
            u.adjust(Item::ChScale(0), 7);
            u.open_menu(MenuId::Channel(0));
        }),
        ("s-ref", |u| {
            for _ in 0..24 {
                u.step(0.04);
            }
            u.activate(Item::RefSave);
            u.adjust(Item::RefOffset, 8);
            u.open_menu(MenuId::Ref);
        }),
        ("s-zoom", |u| u.set_zoom(true)),
        ("s-xy", |u| { u.m.ch[1].on = true; u.xy = true; }),
        ("s-stop", |u| u.m.run = RunState::Stop),
        ("s-spectrum", |u| u.m.view = AView::Spectrum),
        ("s-eye", |u| u.m.view = AView::Eye),
        ("s-power", |u| u.m.view = AView::Power),
        ("s-bode", |u| u.m.view = AView::Bode),
        ("s-step", |u| u.m.view = AView::Step),
        ("s-sysinfo", |u| { u.sysinfo_open(); u.dialog = Some(vec![]); }),
        ("s-power-dlg", |u| u.power_dlg = true),
        ("s-touchlock", |u| u.m.touch_lock = true),
        ("s-la", |u| u.m.la.on = true),
        ("s-cards-1", |u| { u.m.la.on = true; u.m.afg_on = true; }),
        ("s-cards-4", |u| { u.m.la.on = true; u.m.afg_on = true; for c in 0..4 { u.m.ch[c].on = true; } }),
        ("s-pane-spec", |u| { u.m.ch[1].on = true; u.m.pane = 1; }),
        ("s-pane-res", |u| { u.m.ch[1].on = true; u.m.pane = 2; u.m.meas2 = vec![(0, 0, 1), (4, 0, 1)]; u.m.dvm_on = true; u.m.cnt_on = true; }),
        ("s-decoders", |u| {
            u.m.ch[2].on = true; u.m.dec_on = true; u.m.dec_src = 2;
            u.m.dec_select(1); u.m.dec_on = true; u.m.dec_src = 2; u.set_dec_proto(Proto::Spi); u.m.dec_select(0);
            u.table = Some(tables::TableKind::Decode);
        }),
        ("s-track", |u| { u.m.cursor = CursorMode::Track; }),
        ("s-network", |u| {
            (u.net, u.net_if) = ("192.168.177.64".into(), "wlan0".into());
            u.net_ifs = vec![("eth0".into(), "192.0.2.51".into()), ("wlan0".into(), "192.168.177.64".into())];
            u.wlan.link = wlan::tests_link("Labor", "192.168.177.64");
            u.open_menu(MenuId::Network);
        }),
        ("s-decode", |u| { u.m.ch[2].on = true; u.m.dec_on = true; u.m.dec_src = 2; }),
        ("s-4ch-meas", |u| {
            for c in 0..4 {
                u.m.ch[c].on = true;
            }
            u.m.meas.extend([(Meas::Vpp, 1), (Meas::Freq, 1), (Meas::Vrms, 1), (Meas::Vpp, 2), (Meas::Period, 2), (Meas::Vmax, 3), (Meas::Vmin, 3)]);
            u.m.cursor = CursorMode::Time;
        }),
        ("s-events", |u| { u.m.ch[2].on = true; u.m.dec_on = true; u.m.dec_src = 2; u.m.pane = tables::PANE_EVENTS; }),
        ("s-stats", |u| { u.m.ch[1].on = true; u.m.meas.push((Meas::Freq, 1)); u.stats_on = true; }),
        ("s-osk", |u| u.activate(Item::ChLabel(0))),
        ("s-startup", |u| {
            use oos_hal::bootstatus::line;
            u.start_up_begin(None);
            let t = line(3.9, "ok", "Linux", "6.12.115") + &line(4.0, "ok", "FPGA image", "") + &line(14.3, "warn", "Calibration data", "missing: cal_afg.hex") + &line(14.4, "run", "Sample clock (PLL lock)", "waiting for the lock");
            u.start_up_update(&t, 16.0);
        }),
    ];
    for (n, f) in scenes {
        let mut u = new();
        f(&mut u);
        shot(&mut u, n);
    }
    let mut u = new();
    k(&mut u, "measure");
    shot(&mut u, "k-measure");
    let mut u = new();
    u.activate(Item::About);
    shot(&mut u, "s-about");
    let mut u = new();
    u.activate(Item::Update);
    shot(&mut u, "s-update");
}

/// Display > HDMI = Layout: the monitor's picture leaves the panel's layout and maps as they were (the panel picture
/// after it is the same), and its plot shows the traces, stretched from the panel's maps, in every kind of view.
#[test]
fn the_hdmi_layout_leaves_the_panel_alone() {
    type View = (&'static str, fn(&mut Ui));
    let views: [View; 8] = [
        ("plain", |_| {}),
        ("zoom", |u| u.set_zoom(true)),
        ("colour grade", |u| u.m.color_grade = true),
        ("histogram rows", |u| u.m.hist = Hist::Vertical),
        ("histogram columns", |u| u.m.hist = Hist::Horizontal),
        ("xy", |u| {
            u.m.ch[1].on = true;
            u.xy = true;
        }),
        ("spectrum", |u| u.m.view = AView::Spectrum),
        ("trend pane", |u| u.m.pane = tables::PANE_TREND),
    ];
    for (name, set) in views {
        let mut u = with_two_channels();
        set(&mut u);
        for _ in 0..8 {
            u.step(0.04);
        }
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        let before = (u.grid, u.wins.clone(), u.area, u.ph.w, u.ph.h, u.frames, u.scr);
        let mut hc = Canvas::new(1920, 1080);
        u.render_view(&mut hc);
        assert_eq!(before, (u.grid, u.wins.clone(), u.area, u.ph.w, u.ph.h, u.frames, u.scr), "{}", name);
        let mut again = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut again);
        assert!(again.px == cv.px, "{}: the panel picture changed", name);
        if !u.xy && u.m.view == AView::Scope {
            let yellow = hc.px.iter().filter(|&&p| (p >> 16) & 255 > 150 && (p >> 8) & 255 > 120 && p & 255 < 90).count();
            assert!(yellow > 2000, "{}: {} trace pixels on the monitor", name, yellow);
        }
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = crate::store::write(&std::path::Path::new(&d).join(format!("hdmi-layout-{}.png", name.replace(' ', "-"))), &crate::store::png_of(&hc));
        }
    }
}

#[test]
fn trigger_coupling_by_menu_scpi_and_setup() {
    let mut u = with_two_channels();
    let cv = Canvas::new(PANEL_W, PANEL_H);
    let mut s = crate::scpi::Scpi::open(0);
    assert!(u.trigger_items().contains(&Item::TrigCoup));
    s.exec_line(&mut u, &cv, ":TRIG:COUP AC");
    assert_eq!(u.m.trig_coup, TrigCoup::Ac);
    assert_eq!(String::from_utf8_lossy(&s.exec_line(&mut u, &cv, ":TRIG:COUP?")).trim(), "AC");
    u.select_option(Item::TrigCoup, "HF reject");
    assert_eq!(u.m.trig_coup, TrigCoup::HfReject);
    let back = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!(back.trig_coup, TrigCoup::HfReject);
    // only the edge trigger on a channel is filtered; the hardware gets it
    assert_eq!(board().want(&u.m).trig_couple, oos_hal::dho::tpu::Coupling::HfReject);
    u.m.trig_type = TrigType::Pulse;
    assert_eq!(u.m.trig_coupling(), TrigCoup::Dc);
    assert!(!u.trigger_items().contains(&Item::TrigCoup));
}
