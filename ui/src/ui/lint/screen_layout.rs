// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Checks of the main screen: the top bar, the cards in every mix of sources, the screen at every text size, the
//! docked measurement strip and the HDMI layout. A check of what stands where on the main screen goes here.

use super::*;

#[test]
fn acquisition_text_in_the_top_bar_opens_the_acquire_page() {
    let mut u = crate::ui::testkit::demo_ui();
    u.tap(160, 20);
    assert!(u.menu_open && u.menu == MenuId::Acquire);
    u.tap(160, 20);
    assert!(!u.menu_open);
    // The bar keeps what is used most. System and SIM are in the start menu (System first in its group), Load file is a
    // row of the Reference menu.
    assert_eq!(u.top_buttons().iter().map(|b| b.1).collect::<Vec<_>>(), ["Measure", "Cursor", "Display", "Storage"]);
    let system = u.main_groups().into_iter().find(|g| g.0 == "System").unwrap().1;
    assert_eq!(system[0], Item::Open(MenuId::Utility));
    assert!(u.main_groups().iter().any(|g| g.1.contains(&Item::Open(MenuId::Sim))));
    assert!(u.items(MenuId::Ref).contains(&Item::RefLoad));
    // The destination dialog of a waveform load.
    u.request_waveform();
    assert!(u.load_slot_ask && !u.open_wave_request && !u.menu_open);
    u.dismiss_context();
    assert!(!u.load_slot_ask && !u.open_wave_request);
    assert!(u.refs.iter().all(Option::is_none));
    u.request_waveform();
    u.tap(8, 8);
    assert!(!u.load_slot_ask && !u.open_wave_request, "an outside tap cancels without loading");
    u.request_waveform();
    let r3 = Ui::load_slot_button(2);
    u.tap(r3.x + 4, r3.y + 4);
    assert!(!u.load_slot_ask && u.open_wave_request);
    assert_eq!(u.waveform_destination(), Some(2));
    assert_eq!(u.ref_slot(), 0, "destination selection is separate from the current reference");
    u.open_wave_request = false;
    // Measurement choices remain one tap away, without going through acquisition or another menu.
    let (_, _, r) = u.top_buttons().into_iter().find(|b| b.0 == TopBtn::Menu(MenuId::Measure)).unwrap();
    u.tap(r.x + 5, r.y + 5);
    assert!(u.menu_open && u.menu == MenuId::MeasAdd);
}

/// Reference > Load file on the scope: the file comes from storage and lands in the Slot row's reference.
#[test]
fn load_file_in_the_reference_menu_takes_a_file_from_storage() {
    let dir = crate::ui::testkit::Scratch::new("refload");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.activate(Item::RefLoad);
    assert!(u.refs.iter().all(Option::is_none), "nothing in storage, nothing loaded");
    let rec = u.last[0].as_ref().unwrap().clone();
    std::fs::write(dir.join("capture.bin"), crate::store::wave_bin(&rec, &u.m)).unwrap();
    u.ref_sel = 2;
    u.activate(Item::RefLoad);
    let r = u.refs[2].as_ref().expect("R3");
    assert_eq!(*r.v, rec.ch[0]);
    assert!(!u.load_slot_ask && !u.open_wave_request, "no file dialog on the scope");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_page_offers_to_switch_on_its_channels() {
    let mut u = crate::ui::testkit::demo_ui();
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
    let mut u = crate::ui::testkit::demo_ui();
    assert_eq!(u.save_dir(), std::path::Path::new(crate::store::DEFAULT_DIR));
    assert!(u.menu_title(MenuId::Storage).contains("lost at power-off"));
    u.stick.mounted = true;
    assert_eq!(u.save_dir(), std::path::Path::new(crate::store::USB_DIR));
    assert_eq!(u.menu_title(MenuId::Storage), "Storage \u{b7} USB stick");
    // a directory given by hand stays
    u.store_dir = "/oos/shots".into();
    assert_eq!(u.save_dir(), std::path::Path::new("/oos/shots"));
    assert_eq!(u.saved_where(), "");
    // Local keeps the files in the storage directory while the stick is there, and the title says so
    u.store_dir = crate::store::DEFAULT_DIR.into();
    u.store_dest = crate::store::StoreDest::Local;
    assert_eq!(u.save_dir(), std::path::Path::new(crate::store::DEFAULT_DIR));
    assert_eq!(u.saved_where(), " locally, not on the stick");
    assert!(u.menu_title(MenuId::Storage).contains("lost at power-off"));
}

#[test]
fn system_info_has_the_rates() {
    let mut u = crate::ui::testkit::demo_ui();
    u.counts.wfm_rate = 123.0;
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    assert!(u
        .sysinfo_rows()
        .iter()
        .flatten()
        .any(|r| matches!(r, super::sysinfo::Row::Kv(k, ..) if k.contains("Waveforms"))));
}

/// The cards fit every mix of channels, LA and generator; the compact bar's source controls too, auxiliary tools
/// there remain in Sources.
#[test]
fn bottom_row_fits_in_every_mix() {
    for scr in SCREENS {
        bottom_row_fits_on(scr);
        bottom_row_fits_on_compact(scr);
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

fn bottom_row_fits_on_compact(scr: Screen) {
    let mut u = demo_on(scr);
    u.m.bar_compact = true;
    for bits in 0..64 {
        for c in 0..NCH {
            u.m.ch[c].on = bits >> c & 1 == 1;
        }
        (u.m.la.on, u.m.afg_on) = (bits & 16 != 0, bits & 32 != 0);
        let c = u.card_layout();
        assert!(c.la.is_none() && c.gen.is_none(), "auxiliary tools do not reserve LCD footer space");
        let mut all: Vec<Rect> = c.ch.into_iter().filter(|rect| rect.w > 0).collect();
        all.extend([c.time, c.trig]);
        for (i, a) in all.iter().enumerate() {
            assert!(a.w >= 38 && a.h >= 30 && a.right() <= u.scr.w, "{:?} mix {:b}: {:?}", scr, bits, a);
            assert!(!all[i + 1..].iter().any(|b| overlaps(*a, *b)), "mix {:b}: {:?}", bits, a);
        }
        assert!(c.trig.w >= 222 && c.time.w >= 124, "mix {:b}", bits);
        // A visible channel keeps a distinct source hit target.
        for k in 0..NCH {
            assert!(!u.m.ch[k].on || c.ch[k].w >= 38, "mix {:b}: CH{} {}", bits, k + 1, c.ch[k].w);
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
        // top bar, with every mix of the status boxes that can be there (ADR 0063): those that come and go are set by
        // the state their `shown` reads, so a box that is not set up here fails the count below
        let sticks: [fn(&mut Ui, bool); 3] =
            [|u, on| u.net.wlan_stick = on, |u, on| u.bt.stick = on, |u, on| u.stick.mounted = on];
        for k in 0..1usize << sticks.len() {
            for (i, set) in sticks.iter().enumerate() {
                set(&mut u, k & 1 << i != 0);
            }
            let pill = u.run_pill();
            assert!(f.mb.width("TRIG'D") + 30 <= pill.w, "{:?}: run pill {:?}", ts, pill);
            let boxes: Vec<Rect> = u.status_boxes().into_iter().map(|(_, r)| r).collect();
            assert_eq!(boxes.len(), 1 + k.count_ones() as usize, "a status box without its switch in this lint");
            for (i, b) in boxes.iter().enumerate() {
                assert!(
                    b.y >= 0 && b.bottom() <= u.frame().top.bottom() && b.right() <= u.scr.w - 6,
                    "{:?}: {:?}",
                    ts,
                    b
                );
                assert!(!boxes[i + 1..].iter().any(|o| overlaps(*o, *b)), "{:?}: {:?}", ts, b);
            }
            for (_, l, r) in u.top_buttons() {
                assert!(r.x > pill.x + pill.w && r.x + r.w <= u.top_right(), "{:?}: {} {:?}", ts, l, r);
                assert!(f.mb.width(l) + 8 <= r.w, "{:?}: {}", ts, l);
            }
        }
        for set in sticks {
            set(&mut u, false);
        }
        // the two lines of the network box: room for an address, the first line's descent above the second's capitals;
        // every status box of two lines has the room for an address too (issue #23)
        let n = u.net_box();
        assert!(20 + f.sb.width("255.255.255.255") <= n.w && 20 + f.sb.width("not connected") <= n.w, "{:?}", ts);
        for set in sticks {
            set(&mut u, true);
        }
        for (b, r) in u.status_boxes().into_iter().filter(|(b, _)| !b.chip) {
            assert!(20 + f.sb.width("255.255.255.255") <= r.w, "{:?}: {} box too narrow for an address", ts, b.label);
        }
        for set in sticks {
            set(&mut u, false);
        }
        assert!(n.y + 5 + cap(&f.s) + f.s.descent <= n.y + n.h - 4 - cap(&f.sb), "{:?}: {:?}", ts, n);
        // a strip row holds its text
        let bh = u.lay().strip_row - 2;
        let base = (bh + cap(&f.s)) / 2;
        assert!(base - cap(&f.sb) >= 0 && base + f.s.descent <= bh, "{:?}: strip row", ts);
        // everything on: four channels with six measurements each, cursors, DVM
        for c in 0..NCH {
            u.m.ch[c].on = true;
        }
        u.m.meas = (0..NCH).flat_map(|c| ALL_MEAS.iter().take(6).map(move |&k| (k, WaveId::Ch(c)))).collect();
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
        let all = std::mem::replace(&mut u.m.meas, ALL_MEAS.iter().take(12).map(|&k| (k, WaveId::Ch(0))).collect());
        u.stats_on = true;
        u.relayout();
        let (head, body) = u.results_rects().unwrap();
        assert!(
            head.x >= 0
                && body.y + body.h <= u.grid.y + u.grid.h
                && f.sb.width("Measurements") + 24 + f.s.width("n 999k") + 20 <= head.w,
            "{:?}",
            ts
        );
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
                assert!(
                    sub.width(&t) <= r.w - 22 || normal.s.width(&t) > r.w - 22,
                    "{:?} mix {:b}: CH{} '{}'",
                    ts,
                    bits,
                    c + 1,
                    t
                );
                let scale = format!("{}/div", si_short(u.m.ch[c].volts_per_div() as f64, "V"));
                assert!(
                    Ui::card_font(f, &scale, r.w - 24).width(&scale) <= r.w - 24 || normal.m.width(&scale) > r.w - 24,
                    "{:?} mix {:b}: CH{}",
                    ts,
                    bits,
                    c + 1
                );
            }
            u.render(&mut cv);
        }
        // menus under the top bar, on the screen, as high as at the normal size (the rows keep their 44 px)
        use MenuId::*;
        for m in [
            Main,
            Channel(0),
            Horizontal,
            Trigger,
            Acquire,
            Measure,
            MeasAdd,
            MeasPair,
            Dvm,
            Cursor,
            Display,
            Utility,
            Math,
            Ref,
            Decode,
            Storage,
            Mask,
            Zone,
            Spectrum,
            Eye,
            Power,
            Identify,
            SelfCal,
            Bode,
            Afg,
            BwTest,
            La,
            LaLines,
            LaPat,
            Network,
            Remote,
        ] {
            u.open_menu(m);
            let d = u.dlg_layout().unwrap();
            assert!(
                d.r.y >= u.frame().top.bottom() && d.r.bottom() <= u.scr.h && d.r.x >= 0 && d.r.right() <= u.scr.w,
                "{:?}: {:?} {:?}",
                ts,
                m,
                d.r
            );
            assert!(d.rows.iter().all(|r| r.ctrl.h >= 40), "{:?}: {:?}", ts, m);
            u.render(&mut cv);
            u.close_menu();
        }
    }
}

#[test]
fn sources_menu_opens_la_and_generator_without_starting_generator_output() {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.bar_compact = true;
    u.tap(80, BOTTOM.y + 15);
    let (x, y) = fix_tests::row_xy(&u, "Logic analyser");
    u.tap(x, y); // Sources: Logic analyser, immediately above Generator.
    assert!(u.m.la.on && u.menu_open && u.menu == MenuId::La);
    u.close_menu();
    u.tap(80, BOTTOM.y + 15);
    let (x, y) = fix_tests::row_xy(&u, "Generator");
    u.tap(x, y);
    assert!(u.menu_open && u.menu == MenuId::Afg);
    assert!(!u.m.afg_on, "a card must not switch an output on");
}

/// Measure > Dock: under the plot, left, right, or above the trace. The plot gives that side up and keeps the rest.
#[test]
fn the_measurement_strip_docks_on_four_sides() {
    let mut bare = crate::ui::testkit::demo_ui();
    bare.m.meas.clear();
    bare.m.meas2.clear();
    bare.m.cursor = CursorMode::Off;
    bare.m.dvm_on = false;
    bare.m.cnt_on = false;
    bare.relayout();
    let empty = bare.grid;
    for side in StripDock::ALL {
        bare.m.strip_dock = side;
        bare.relayout();
        assert_eq!(bare.grid, empty, "an empty strip stays where it is on {:?}", side);
    }

    let mut u = with_two_channels();
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vrms, WaveId::Ch(1))];
    u.m.cursor = CursorMode::Time;
    u.m.strip_dock = StripDock::Under;
    u.relayout();
    let under = u.grid;
    assert_eq!((under.x, under.w, under.y), (0, W, PLOT_Y));
    let band = u.strip_rect().unwrap();
    assert!(!overlaps(band, under), "under {:?}", band);
    assert!(band.y >= under.y + under.h);

    u.m.strip_dock = StripDock::Above;
    u.relayout();
    assert_eq!((u.grid.x, u.grid.w, u.grid.h), (0, W, under.h));
    assert_eq!(u.grid.y, PLOT_Y + u.strip_rows_h());
    let band = u.strip_rect().unwrap();
    assert!(!overlaps(band, u.grid), "above {:?}", band);
    assert!(band.y + band.h <= u.grid.y);

    u.m.strip_dock = StripDock::Left;
    u.relayout();
    assert_eq!((u.grid.x, u.grid.w), (STRIP_SIDE, W - STRIP_SIDE));
    assert!(u.grid.h > under.h, "the plot gets the rows back");
    assert_eq!(u.grid.y, PLOT_Y);
    let col = u.strip_rect().unwrap();
    assert!(!overlaps(col, u.grid), "left {:?}", col);
    assert_eq!(col.x + col.w, u.grid.x);

    let left_h = u.grid.h;
    u.m.strip_dock = StripDock::Right;
    u.relayout();
    assert_eq!((u.grid.x, u.grid.w, u.grid.h, u.grid.y), (0, W - STRIP_SIDE, left_h, PLOT_Y));
    let col = u.strip_rect().unwrap();
    assert!(!overlaps(col, u.grid), "right {:?}", col);
    assert_eq!(col.x, u.grid.x + u.grid.w);

    // a second pane stays in the plot column (Results is the pane that replaces the strip)
    u.m.pane = 4;
    u.relayout();
    assert!(u.pane_h() > 0 && u.strip_rows() > 0);
    assert!(!overlaps(u.pane_rect(), u.strip_rect().unwrap()));
    u.m.pane = 0;

    u.full = true;
    u.m.strip_dock = StripDock::Above;
    u.relayout();
    assert_eq!(u.grid.y, u.strip_rows_h());
    assert!(u.strip_rect().unwrap().y < u.grid.y);
    u.full = false;

    // the control on the Measure page picks the side
    u.m.strip_dock = StripDock::Under;
    u.open_menu(MenuId::Measure);
    let hit = |ui: &Ui, k: usize| -> (i32, i32) {
        let d = ui.dlg_layout().unwrap();
        let row = d.rows.iter().find(|r| r.it == Item::StripDock).expect("dock row");
        let Ctl::Seg(opts, sel) = &row.ctl else { panic!("dock is a row of buttons") };
        assert_eq!(opts.as_slice(), ["Under", "Left", "Right", "Above"]);
        assert_eq!(*sel, Some(StripDock::ALL.iter().position(|&s| s == ui.m.strip_dock).unwrap()));
        let n = opts.len() as i32;
        let sw = (row.ctrl.w - (n - 1) * 4) / n;
        (row.ctrl.x + k as i32 * (sw + 4) + sw / 2, row.ctrl.y + row.ctrl.h / 2)
    };
    for (k, side) in StripDock::ALL.into_iter().enumerate() {
        let (x, y) = hit(&u, k);
        u.tap(x, y);
        assert_eq!(u.m.strip_dock, side);
        assert!(u.menu_open);
    }
    // the ends stay put
    u.m.strip_dock = StripDock::Under;
    u.adjust(Item::StripDock, -1);
    assert_eq!(u.m.strip_dock, StripDock::Under);
    u.m.strip_dock = StripDock::Above;
    u.adjust(Item::StripDock, 1);
    assert_eq!(u.m.strip_dock, StripDock::Above);

    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("strip_dock = Above"));
    assert_eq!(crate::store::parse_setup(&text).unwrap().strip_dock, StripDock::Above);
    let old: String = text.lines().filter(|l| !l.starts_with("strip_dock")).collect::<Vec<_>>().join("\n");
    assert_eq!(crate::store::parse_setup(&old).unwrap().strip_dock, StripDock::Under);
    assert!(crate::store::parse_setup("strip_dock = Beside").is_err());

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
    let _ = std::fs::create_dir_all(&dir);
    let shot = |u: &mut Ui, n: &str| {
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        let _ = crate::store::write(&dir.join(n), &crate::store::png_of(&cv));
    };
    u.close_menu();
    for (side, name) in [
        (StripDock::Under, "strip-under.png"),
        (StripDock::Left, "strip-left.png"),
        (StripDock::Right, "strip-right.png"),
        (StripDock::Above, "strip-above.png"),
    ] {
        u.m.strip_dock = side;
        shot(&mut u, name);
        let g = u.grid;
        let s = u.strip_rect().unwrap();
        assert!(!overlaps(g, s), "{name} plot {g:?} strip {s:?}");
    }
    u.m.strip_dock = StripDock::Left;
    u.open_menu(MenuId::Measure);
    shot(&mut u, "strip-menu.png");
    u.close_menu();
    u.m.meas2 = vec![(0, WaveId::Ch(0), WaveId::Ch(1))];
    shot(&mut u, "strip-left-pair.png");
    let s = u.strip_rect().unwrap();
    assert!(!overlaps(u.grid, s));
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
        let before = (u.grid, u.plot.wins.clone(), u.area, u.ph.w, u.ph.h, u.counts.frames, u.scr);
        let mut hc = Canvas::new(1920, 1080);
        u.render_view(&mut hc);
        assert_eq!(before, (u.grid, u.plot.wins.clone(), u.area, u.ph.w, u.ph.h, u.counts.frames, u.scr), "{}", name);
        let mut again = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut again);
        assert!(again.px == cv.px, "{}: the panel picture changed", name);
        if !u.xy && u.m.view == AView::Scope {
            let yellow =
                hc.px.iter().filter(|&&p| (p >> 16) & 255 > 150 && (p >> 8) & 255 > 120 && p & 255 < 90).count();
            assert!(yellow > 2000, "{}: {} trace pixels on the monitor", name, yellow);
        }
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = crate::store::write(
                &std::path::Path::new(&d).join(format!("hdmi-layout-{}.png", name.replace(' ', "-"))),
                &crate::store::png_of(&hc),
            );
        }
    }
}
