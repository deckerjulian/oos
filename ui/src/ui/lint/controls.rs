// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Checks of the pages and controls driven through the UI: measurements, cursors, math, decoders, scales, setups,
//! remote commands and random input. A check of a control that is not a pane, a reference or a picture goes here.

use super::*;

#[test]
fn measurements_keep_delay_and_phase_without_dropping_older_items() {
    let mut u = with_two_channels();
    u.m.meas.clear();
    for k in ALL_MEAS.iter().take(10) {
        u.m.meas.push((*k, WaveId::Ch(0)));
    }
    u.meas_src_b = WaveId::Ch(1);
    for k in 0..6 {
        u.meas_pair_toggle(k);
    }
    // A larger selection keeps all ten original measurements and all six pairs.
    assert_eq!(u.m.meas.len() + u.m.meas2.len(), 16);
    let rows = u.meas_rows();
    assert_eq!(rows.len(), 16);
    assert!(rows.iter().any(|r| r.name.starts_with("Delay rise-rise 1-2")));
    // the same channel twice makes no sense
    u.meas_src_b = WaveId::Ch(0);
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
    u.m.meas2 = vec![(0, WaveId::Ch(0), WaveId::Ch(1)), (4, WaveId::Ch(0), WaveId::Ch(1))];
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
    assert!(u.cnt.total > 0);
    u.m.cnt_mode = 2;
    assert_ne!(u.dvm_row()[1].1, "----");
    u.counter_clear();
    assert_eq!(u.cnt.total, 0);
    // off source: no number, and the page offers to switch it on
    u.m.dvm_src = 3;
    assert!(!u.dvm_value().is_finite());
    assert!(u.items(MenuId::Dvm).contains(&Item::SrcOn));
}

#[test]
fn dvm_prefers_a_decoded_block_reading_of_its_source() {
    let mut u = with_two_channels();
    u.m.dvm_on = true;
    for _ in 0..4 {
        u.step(0.04);
    }
    let sw = u.dvm_value();
    assert!(sw.is_finite());
    assert!(u.dvm_debug_text().starts_with("off,sw="));
    let r = crate::board::DvmRead {
        ch: 0,
        lane: 0,
        exp: 28,
        raw: [0x39, 1, 2, 3, 4],
        volts: Some([0.25, 0.5, 0.75]),
        n: 3,
    };
    // (reading, mode, value): the mode picks the value; another channel's or a refused reading leaves the software DVM
    let cases: [(crate::board::DvmRead, usize, f64); 4] = [
        (r, 0, 0.25),
        (r, 2, 0.75),
        (crate::board::DvmRead { ch: 1, ..r }, 0, sw),
        (crate::board::DvmRead { volts: None, ..r }, 0, sw),
    ];
    for (hw, mode, want) in cases {
        u.dvm.hw = Some(hw);
        u.m.dvm_mode = mode;
        assert_eq!(u.dvm_value(), want, "{:?} mode {}", hw, mode);
    }
    u.dvm.hw = Some(r);
    u.m.dvm_mode = 0;
    let t = u.dvm_debug_text();
    assert!(t.starts_with("ch=1,lane=0,exp=28,n=3,cfg=0x00000039,sum_lo=0x00000001"), "{}", t);
    assert!(t.contains("hw=2.500000e-1;5.000000e-1;7.500000e-1,mode=0,sw="), "{}", t);
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
    let dir = crate::ui::testkit::Scratch::new("mask");
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.activate(Item::MaskCreate);
    let a = u.mask_sw.reference.clone().unwrap();
    u.activate(Item::MaskSave);
    u.mask_sw.forget();
    u.activate(Item::MaskLoad);
    assert_eq!(u.mask_sw.reference.as_ref().unwrap().len(), a.len());
    assert!(u.mask_sw.mask.is_some() && u.m.mask_on);
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
    let mut u = crate::ui::testkit::demo_ui();
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
    u.m.meas2 = vec![(5, WaveId::Ch(1), WaveId::Ch(2))];
    (u.m.dvm_on, u.m.dvm_src, u.m.dvm_mode) = (true, 2, 1);
    (u.m.cnt_on, u.m.cnt_src, u.m.cnt_mode, u.m.cnt_digits) = (true, 3, 2, 6);
    u.m.math_lin = (3.0, -1.5);
    u.m.cursor = CursorMode::Track;
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!(
        (b.meas2.clone(), b.dvm_src, b.dvm_mode, b.cnt_src, b.cnt_mode, b.cnt_digits),
        (vec![(5, WaveId::Ch(1), WaveId::Ch(2))], 2, 1, 3, 2, 6)
    );
    assert_eq!((b.math_lin, b.cursor), ((3.0, -1.5), CursorMode::Track));
}

#[test]
fn four_decoders_side_by_side() {
    let mut u = with_two_channels();
    u.m.ch[2].on = true;
    u.m.dec_on = true;
    u.m.dec_src = 2;
    u.set_dec_proto(Proto::UART);
    u.m.dec_select(1);
    assert_eq!(u.m.dec_sel, 1);
    assert!(!u.m.dec_on, "a new slot starts off");
    u.m.dec_on = true;
    u.set_dec_proto(Proto::I2C);
    u.m.dec_select(0);
    assert_eq!(u.m.dec_proto, Proto::UART);
    assert!(u.m.dec_is_on(0) && u.m.dec_is_on(1) && !u.m.dec_is_on(2));
    assert_eq!(u.m.dec_view(1).dec_proto, Proto::I2C);
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
    assert_eq!(b.dec_view(1).dec_proto, Proto::I2C);
    assert!(b.dec_is_on(1));
    assert_eq!(b.dec_proto, Proto::UART);
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
    let dir = crate::ui::testkit::Scratch::new("events");
    let mut u = with_two_channels();
    u.store_dir = dir.clone();
    u.m.ch[2].on = true;
    u.m.dec_on = true;
    u.m.dec_src = 2;
    for _ in 0..4 {
        u.step(0.04);
    }
    u.activate(Item::DecTable);
    assert_eq!(u.table.kind, Some(tables::TableKind::Decode));
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
    assert!(u.table.kind.is_none());
}

#[test]
fn search_list_shows_the_marks() {
    let mut u = with_two_channels();
    u.search_on = true;
    assert!(u.trigger_items().contains(&Item::SearchTable));
    u.activate(Item::SearchTable);
    assert_eq!(u.table.kind, Some(tables::TableKind::Search));
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
    for (pane, la, ts, scr) in (1..tables::PANES.len()).flat_map(|p| {
        [false, true]
            .into_iter()
            .flat_map(move |l| TEXT_SIZES.into_iter().flat_map(move |t| SCREENS.map(|s| (p, l, t, s))))
    }) {
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
                assert!(
                    r.y > u.grid.y + u.grid.h && r.bottom() + u.strip_rows() * u.lay().strip_row <= u.frame().bottom.y,
                    "pane {} la {}: {:?}",
                    pane,
                    la,
                    r
                );
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
    let pages = [
        MenuId::Logger,
        MenuId::ChMore(0),
        MenuId::Measure,
        MenuId::MeasPair,
        MenuId::Dvm,
        MenuId::Cursor,
        MenuId::Math,
        MenuId::Ref,
        MenuId::Sim,
        MenuId::Mask,
        MenuId::Decode,
        MenuId::Bus,
        MenuId::Trigger,
        MenuId::Display,
        MenuId::Storage,
        MenuId::Bode,
    ];
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
        u.power.open = false;
        u.power.action = None;
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
    for (want, m) in [
        ("Time", CursorMode::Time),
        ("Volt", CursorMode::Volt),
        ("Track", CursorMode::Track),
        ("XY", CursorMode::Xy),
        ("Off", CursorMode::Off),
    ] {
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
fn track_cursor_b_follows_its_own_source() {
    let mut u = with_two_channels();
    u.m.cursor = CursorMode::Track;
    assert_eq!(u.items(MenuId::Cursor).len(), 4);
    assert_eq!(u.value(Item::CurSrcB), "CH1", "follows cursor A's source until set");
    u.adjust(Item::CurSrcB, 1);
    assert_eq!((u.m.cur_b_source, u.value(Item::CurSrcB).as_str()), (Some(WaveId::Ch(1)), "CH2"));
    let (ta, _) = u.cursor_times();
    let mut r = u.last[0].clone().unwrap();
    r.t0 = ta - 1.0;
    r.dt = 1.0;
    r.ch[0] = vec![0.0, 1.0, 2.0];
    r.ch[1] = vec![0.0, 5.0, 6.0];
    u.last[0] = Some(r);
    assert_eq!((u.wave_at_source(WaveId::Ch(0), ta), u.wave_at_source(WaveId::Ch(1), ta)), (Some(1.0), Some(5.0)));
    let v = u.cursor_values();
    assert_ne!(v[1].1, v[3].1, "Y1 and Y2 read different sources");
    let mut cv = Canvas::new(PANEL_W, PANEL_H);
    u.render(&mut cv);
    let b = crate::store::parse_setup(&crate::store::setup_text(&u.m)).unwrap();
    assert_eq!(b.cur_b_source, Some(WaveId::Ch(1)));
    // the other modes have no such row
    u.m.cursor = CursorMode::Time;
    assert!(!u.items(MenuId::Cursor).contains(&Item::CurSrcB));
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
    assert!(
        u.value(Item::ChScale(0)).contains("1.02") || u.value(Item::ChScale(0)).contains("102"),
        "{}",
        u.value(Item::ChScale(0))
    );
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
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1))];
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
    let mean = |u: &Ui| {
        let v = &u.last[0].as_ref().unwrap().ch[0];
        v.iter().sum::<f32>() / v.len() as f32
    };
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
    assert_eq!(
        (s.ch[0].couple, s.ch[0].invert, s.mode),
        (oos_hal::dho::seq::Couple::Ac, true, oos_hal::dho::acq::Mode::Peak)
    );
    assert!((s.delay - 2e-4).abs() < 1e-9);
    // a trigger source that is switched off is acquired all the same, and keeps its role
    let mut m = Model { trig_src: 1, ..Default::default() };
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
    let m = Model {
        tscale: tscales().len() - 1,
        mdepth: mdepths().iter().position(|d| d.1 == 1000).unwrap(),
        delay: 1.0,
        ..Default::default()
    };
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
    crate::cmd::run(&mut u, "knob hpos 2000000000");
    assert!(u.m.delay != d0);
    // two billion detents are taken as the limit of 100, not run one by one
    let mut hundred = with_two_channels();
    crate::cmd::run(&mut hundred, "knob hpos 100");
    assert_eq!(u.m.delay, hundred.m.delay);
}

#[test]
fn the_event_table_can_be_reached_from_the_menu() {
    let u = with_two_channels();
    let v = u.items(MenuId::Decode);
    assert!(v.contains(&Item::DecTable) && v.contains(&Item::DecExport));
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
