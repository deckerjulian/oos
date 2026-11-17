// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 30: feedback and sources. A new view starts as a copy of the shown one, a chosen count closes the dialog and
//! says which view is shown, Studio has a view switcher in its top bar; a source row given a channel that is off turns
//! it on; the controls of an empty reference slot are greyed and say "Store a reference first"; clearing with nothing
//! to clear says so; Copy meas. to says what it did. OOS_SHOTS=<dir> writes pictures (b30-*).
use super::*;

const LAYOUTS: [Layout; 3] = [Layout::Minimal, Layout::Default, Layout::Studio];

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u.step(0.02);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..4 { frame(u); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b30-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn tap_at(u: &mut Ui, x: i32, y: i32) {
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn tap_rect(u: &mut Ui, r: Rect) { tap_at(u, r.x + r.w / 2, r.y + r.h / 2); }

/// A scope with CH1 and CH2 on, CH3 and CH4 off, and no reference.
fn fresh(layout: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    u.m.ch[1].on = true;
    u.m.ch[2].on = false;
    u.m.ch[3].on = false;
    for r in u.refs.iter_mut() { *r = None; }
    for _ in 0..4 { u.step(0.05); }
    u
}

fn toast(u: &Ui) -> String { u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default() }

fn clear_toast(u: &mut Ui) { u.toast = None; }

fn a_reference() -> RefWave {
    RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-9, v: vec![0.0f32; 128].into(), vpd: 0.5, offset: 0.0, on: true }
}

/// The control of the row of `it` in the open menu: the classic dialog, or the Studio sheet (its tabs are tried).
fn ctrl_of(u: &mut Ui, it: Item) -> (Rect, bool) {
    if u.sheet_active() {
        let tabs = u.sheet_geo().expect("a sheet").tabs.len().max(1);
        for i in 0..tabs {
            let g = u.sheet_geo().expect("a sheet");
            if let Some(t) = g.tabs.get(i) { tap_rect(u, *t); }
            let g = u.sheet_geo().expect("a sheet");
            if let Some(row) = g.rows.iter().find(|r| r.it == it) { return (row.ctrl, true); }
        }
        panic!("{it:?} is a row of the sheet");
    }
    let row = u.dlg_layout().expect("a dialog").rows.into_iter().find(|r| r.it == it).unwrap_or_else(|| panic!("{it:?} is a row"));
    (row.ctrl, false)
}

/// Taps option `k` of `n` of a segmented row.
fn tap_segment(u: &mut Ui, it: Item, k: usize, n: usize) {
    let (c, _) = ctrl_of(u, it);
    let x = c.x + (2 * k as i32 + 1) * c.w / (2 * n as i32);
    tap_at(u, x, c.y + c.h / 2);
}

fn views_row_options(u: &Ui) -> usize { u.options(Item::Views).len() }

#[test]
fn choosing_a_view_count_closes_the_dialog_toasts_and_the_new_view_is_a_copy_in_every_layout() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1))];
        u.m.zoom = true;
        u.open_menu(MenuId::Display);
        // Off, 2, 3, 4: the third option is "3" views
        let n = views_row_options(&u);
        tap_segment(&mut u, Item::Views, 2, n);
        assert_eq!(u.m.views, 3, "{l:?}");
        assert!(!u.menu_open, "{l:?}: the Display dialog closes");
        assert_eq!(toast(&u), "View 1 of 3", "{l:?}");
        // the shown view stays; the new ones are copies of it
        assert_eq!(u.m.meas.len(), 2, "{l:?}");
        u.view_go(1);
        assert_eq!(toast(&u), "View 2 of 3", "{l:?}: switching toasts");
        assert_eq!(u.m.meas, vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1))], "{l:?}: view 2 is a copy, not an empty plot");
        assert!(u.m.zoom, "{l:?}");
        u.view_go(2);
        assert_eq!(u.m.meas.len(), 2, "{l:?}: view 3 too");
        // what changes in a copy stays in it
        u.m.meas.clear();
        u.view_go(0);
        assert_eq!(u.m.meas.len(), 2, "{l:?}: view 1 keeps its own");
        // more views later: the new ones copy what is shown then
        u.m.meas.pop();
        u.views_set(4);
        u.view_go(3);
        assert_eq!(u.m.meas.len(), 1, "{l:?}: view 4 copies the view that was shown");
    }
}

#[test]
fn a_view_count_picked_with_the_stepper_or_by_scpi_also_says_which_view_is_shown() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.views_set(2);
        assert_eq!(toast(&u), "View 1 of 2", "{l:?}");
        u.views_set(1);
        assert_eq!(toast(&u), "Views off", "{l:?}");
    }
}

#[test]
fn fewer_views_keep_the_content_on_screen_in_every_layout() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.views_set(4);
        u.view_go(3);
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(0))];
        u.m.zoom = true;
        u.views_set(2);
        assert_eq!((u.m.views, u.m.view_cur), (2, 1), "{l:?}");
        assert!(u.m.zoom && u.m.meas.len() == 1, "{l:?}: the shown content stays");
        u.views_set(1);
        assert_eq!((u.m.views, u.m.view_cur), (1, 0), "{l:?}");
        assert!(u.m.zoom && u.m.meas.len() == 1, "{l:?}: and again");
        assert!(u.m.view_slots.iter().all(|s| s.is_empty()), "{l:?}");
    }
}

#[test]
fn every_layout_has_numbered_view_buttons_that_switch_and_toast() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        assert!(u.view_box().is_none(), "{l:?}: one view, no buttons");
        u.views_set(4);
        let r = u.view_box().unwrap_or_else(|| panic!("{l:?}: the view buttons are in the top bar"));
        assert!(r.x >= 0 && r.right() <= u.scr.w && r.y >= 0 && r.bottom() <= u.frame().top.bottom(), "{l:?}: inside the top bar {r:?}");
        for want in [2usize, 0, 3] {
            let r = u.view_box().unwrap();
            let w = r.w / 4;
            clear_toast(&mut u);
            tap_at(&mut u, r.x + 1 + w * want as i32 + w / 2, r.y + r.h / 2);
            assert_eq!(u.m.view_cur, want, "{l:?}: a tap on {}", want + 1);
            assert_eq!(toast(&u), format!("View {} of 4", want + 1), "{l:?}");
        }
    }
}

#[test]
fn the_studio_view_switcher_fits_the_top_bar_beside_the_other_buttons() {
    let mut u = fresh(Layout::Studio);
    let before = u.studio_top();
    assert_eq!(before.vbox.w, 0, "one view: no switcher");
    for n in 2..=4usize {
        u.views_set(n);
        let t = u.studio_top();
        assert!(t.vbox.w > 0 && t.vbox.h == t.add.h && t.vbox.y == t.add.y, "{n} views: a chip of the bar height");
        // left of + Add, right of the status caption, nothing overlaps, everything stays on the bar
        assert!(t.vbox.right() < t.add.x && t.acq.right() < t.vbox.x, "{n} views: {t_v:?} {t_a:?}", t_v = t.vbox, t_a = t.add);
        assert!(t.acq.w >= 40, "{n} views: the caption keeps a little room");
        assert!(t.vbox.x > t.force.right(), "{n} views: not over Autoset, Clear, Force");
        for r in [t.add, t.measure, t.cursors, t.display, t.storage, t.view, t.gear] {
            assert!(r.x >= t.vbox.right() || r.right() <= t.vbox.x, "{n} views: no overlap {r:?}");
        }
    }
    // the bar still picks a shorter label tier before dropping anything
    u.views_set(4);
    assert!(u.studio_top().tier >= before.tier);
}

#[test]
fn a_source_row_given_a_channel_that_is_off_turns_it_on_and_says_so_in_every_layout() {
    let rows = [
        (Item::SpecSrc, "CH3"),
        (Item::EyeSrc, "CH4"),
        (Item::BusData, "CH3"),
        (Item::BusClk, "CH4"),
        (Item::PowV, "CH3"),
        (Item::PowI, "CH4"),
        (Item::EyeB, "CH3"),
        (Item::EyeRail, "CH4"),
        (Item::PowVin, "CH3"),
        (Item::PowClk, "CH4"),
    ];
    for l in LAYOUTS {
        for (it, want) in rows {
            let mut u = fresh(l);
            let c = if want == "CH3" { 2 } else { 3 };
            assert!(!u.m.ch[c].on);
            u.select_option(it, want);
            assert_eq!(u.value(it), want, "{l:?} {it:?}: the source is the channel that was off");
            assert!(u.m.ch[c].on, "{l:?} {it:?}: the channel is on");
            assert_eq!(toast(&u), format!("{want} turned on"), "{l:?} {it:?}");
            // the other channels did not change
            assert!(u.m.ch[0].on && u.m.ch[1].on && (u.m.ch[5 - c].on == false), "{l:?} {it:?}");
            // a channel that is on is taken without a word
            let mut v = fresh(l);
            clear_toast(&mut v);
            v.select_option(it, "CH2");
            assert_eq!(v.value(it), "CH2");
            assert!(toast(&v).is_empty(), "{l:?} {it:?}: {}", toast(&v));
        }
    }
}

#[test]
fn the_power_voltage_and_current_rows_set_their_own_source() {
    // the function check flagged these as WRONG-TARGET by a name heuristic (`pw_v` against "Voltage"): they are right
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.select_option(Item::PowV, "CH2");
        u.select_option(Item::PowI, "CH3");
        assert_eq!((u.m.pw_v, u.m.pw_i), (WaveId::Ch(1), WaveId::Ch(2)), "{l:?}");
        assert!(u.m.ch[2].on);
    }
}

#[test]
fn the_source_stepper_turns_the_next_channel_on_instead_of_skipping_it() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.select_option(Item::SpecSrc, "CH2");
        clear_toast(&mut u);
        u.adjust(Item::SpecSrc, 1);
        assert_eq!(u.m.spec_src, WaveId::Ch(2), "{l:?}");
        assert!(u.m.ch[2].on);
        assert_eq!(toast(&u), "CH3 turned on", "{l:?}");
    }
}

#[test]
fn the_controls_of_an_empty_reference_slot_are_greyed_and_say_what_to_do_in_every_layout() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        for it in [Item::RefShow(0), Item::RefShow(9), Item::RefColor, Item::RefOffset, Item::RefVpd] {
            assert_eq!(u.item_blocked(it), Some("Store a reference first"), "{l:?} {it:?}");
        }
        // what does not need a stored reference stays open
        for it in [Item::RefSave, Item::RefSaveNext, Item::RefSel, Item::RefSrc] {
            assert_eq!(u.item_blocked(it), None, "{l:?} {it:?}");
        }
        for it in [Item::RefShow(0), Item::RefColor, Item::RefOffset, Item::RefVpd] {
            clear_toast(&mut u);
            u.activate(it);
            assert_eq!(toast(&u), "Store a reference first", "{l:?} {it:?}");
            clear_toast(&mut u);
            u.adjust(it, 1);
            assert_eq!(toast(&u), "Store a reference first", "{l:?} adjust {it:?}");
        }
        // with a stored reference they work and say nothing
        u.refs[0] = Some(a_reference());
        u.ref_sel = 0;
        for it in [Item::RefShow(0), Item::RefColor, Item::RefOffset, Item::RefVpd] {
            assert_eq!(u.item_blocked(it), None, "{l:?} {it:?}");
        }
        assert_eq!(u.item_blocked(Item::RefShow(1)), Some("Store a reference first"), "{l:?}: the next slot is still empty");
        clear_toast(&mut u);
        u.adjust(Item::RefOffset, 1);
        assert!(toast(&u).is_empty(), "{l:?}");
    }
}

#[test]
fn tapping_a_greyed_reference_row_in_the_dialog_or_the_sheet_shows_the_toast() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.open_menu(MenuId::Ref);
        frame(&mut u);
        // the offset of the slot: the plus of its number row
        clear_toast(&mut u);
        let (c, _) = ctrl_of(&mut u, Item::RefOffset);
        let (_, _, plus) = Ui::value_parts(c);
        tap_rect(&mut u, plus);
        assert_eq!(toast(&u), "Store a reference first", "{l:?}: slot offset");
        assert_eq!(u.refs.iter().flatten().count(), 0, "{l:?}: nothing was made");
        // the value of the number row does not open the pad
        clear_toast(&mut u);
        let (c, _) = ctrl_of(&mut u, Item::RefOffset);
        let (_, mid, _) = Ui::value_parts(c);
        tap_rect(&mut u, mid);
        assert_eq!(toast(&u), "Store a reference first", "{l:?}: the value");
        assert!(u.numpad.is_none(), "{l:?}");
        if l == Layout::Default {
            // the slot rows R1..R10 of the classic dialog
            clear_toast(&mut u);
            let r = u.dlg_layout().unwrap().rows.into_iter().find(|r| r.it == Item::RefShow(2)).expect("R3 row").ctrl;
            tap_rect(&mut u, r);
            assert_eq!(toast(&u), "Store a reference first", "{l:?}: R3");
        }
    }
}

#[test]
fn clearing_with_nothing_to_clear_says_so_and_clearing_something_says_what_was_cleared() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        for it in [Item::EyeClear, Item::CntClear, Item::MaskReset, Item::SpecLimitClear, Item::ZoneClear, Item::MaskClear] {
            clear_toast(&mut u);
            u.activate(it);
            assert_eq!(toast(&u), "Nothing to clear", "{l:?} {it:?}");
        }
        // something to clear: the normal result
        u.azr.eye = Some(oos_dsp::eye::Eye::new(8, 8, 2, -1.0, 1.0));
        u.activate(Item::EyeClear);
        assert_eq!(toast(&u), "Eye cleared", "{l:?}");
        assert!(u.azr.eye.is_none());
        u.activate(Item::EyeClear);
        assert_eq!(toast(&u), "Nothing to clear", "{l:?}: now it is empty");

        u.cnt_total = 12;
        u.activate(Item::CntClear);
        assert_eq!((toast(&u), u.cnt_total), ("Counter cleared".to_string(), 0), "{l:?}");

        u.mask_pass = 3;
        u.mask_fail = 1;
        u.activate(Item::MaskReset);
        assert_eq!((toast(&u), u.mask_pass, u.mask_fail), ("Counts reset".to_string(), 0, 0), "{l:?}");

        u.azr.limit = Some(Default::default());
        u.activate(Item::SpecLimitClear);
        assert_eq!(toast(&u), "Limit cleared", "{l:?}");
        assert!(u.azr.limit.is_none());

        u.zone_sel = 0;
        u.m.zones[0].pts = vec![(0.0, 0.0), (1e-6, 1.0)];
        u.activate(Item::ZoneClear);
        assert_eq!(toast(&u), "Zone 1 deleted", "{l:?}");
        assert!(u.m.zones[0].pts.is_empty());
    }
}

#[test]
fn information_is_not_drawn_as_a_warning() {
    for s in ["Nothing to clear", "View 2 of 4", "Views off", "CH3 turned on"] {
        assert!(Ui::toast_is_info(s) && !Ui::toast_is_success(s), "{s}");
    }
    for s in ["Eye cleared", "Counter cleared", "Limit cleared", "Counts reset", "Zone 1 deleted", "Copied 2 measurements to CH2"] {
        assert!(Ui::toast_is_success(s) && !Ui::toast_is_info(s), "{s}");
    }
    // a real warning stays one
    assert!(!Ui::toast_is_info("Store a reference first") && !Ui::toast_is_success("Store a reference first"));
    let mut u = fresh(Layout::Default);
    u.activate(Item::EyeClear);
    shot(&mut u, "nothing-to-clear");
}

#[test]
fn copy_meas_to_says_what_it_copied_and_the_row_stops_saying_pick() {
    for l in LAYOUTS {
        let mut u = fresh(l);
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0))];
        u.meas_src = WaveId::Ch(0);
        assert_eq!(u.value(Item::MeasCopyTo), "Pick", "{l:?}");
        u.select_option(Item::MeasCopyTo, "CH2");
        assert_eq!(toast(&u), "Copied 2 measurements to CH2", "{l:?}");
        assert_eq!(u.value(Item::MeasCopyTo), "Copied to CH2", "{l:?}: no stale Pick");
        assert!(u.m.meas.contains(&(Meas::Vpp, WaveId::Ch(1))), "{l:?}");
        // another source: nothing copied from it yet
        u.meas_src = WaveId::Ch(1);
        assert_eq!(u.value(Item::MeasCopyTo), "Pick", "{l:?}");
        // one measurement
        u.m.meas = vec![(Meas::Freq, WaveId::Ch(1))];
        u.select_option(Item::MeasCopyTo, "CH1");
        assert_eq!(toast(&u), "Copied 1 measurement to CH1", "{l:?}");
    }
}

#[test]
fn pictures_of_the_views_the_studio_bar_an_off_source_and_a_greyed_slot() {
    // Views 2, copied: the plot with a measurement in both views
    let mut u = fresh(Layout::Default);
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0))];
    u.open_menu(MenuId::Display);
    let n = views_row_options(&u);
    tap_segment(&mut u, Item::Views, 1, n);
    u.view_go(1);
    shot(&mut u, "views2-copied");

    // the Studio top bar with four views
    let mut s = fresh(Layout::Studio);
    s.views_set(4);
    s.view_go(1);
    shot(&mut s, "studio-topbar-views");

    // an off channel picked as the spectrum source
    let mut p = fresh(Layout::Default);
    p.select_option(Item::SpecOn, "On");
    p.select_option(Item::SpecSrc, "CH3");
    shot(&mut p, "spectrum-off-channel-source");

    // an empty reference slot: greyed, in the dialog and in the Studio sheet
    let mut r = fresh(Layout::Default);
    r.open_menu(MenuId::Ref);
    let (c, _) = ctrl_of(&mut r, Item::RefShow(2));
    tap_rect(&mut r, c);
    shot(&mut r, "empty-ref-slot-greyed");
    let mut t = fresh(Layout::Studio);
    t.open_menu(MenuId::Ref);
    frame(&mut t);
    shot(&mut t, "empty-ref-slot-greyed-studio");
}
