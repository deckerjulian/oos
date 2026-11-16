//! Batch 34: a source row turns on only the channel it ends on. Stepping a source row through the channels gives back
//! the ones it turned on on the way; a picker sets its choice straight away; setting up the eye never turns on a
//! secondary source; the Measurements panel of a view without measurements says so.
use super::studio::StudioView;
use super::*;

const LAYOUTS: [Layout; 3] = [Layout::Minimal, Layout::Default, Layout::Studio];

/// Only CH1 on, no reference.
fn only_ch1(layout: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    u.m.ch[0].on = true;
    for c in 1..NCH { u.m.ch[c].on = false; }
    for r in u.refs.iter_mut() { *r = None; }
    u.m.meas.clear(); u.m.meas2.clear(); u.m.cat.clear();
    for _ in 0..4 { u.step(0.05); }
    u
}

fn on(u: &Ui) -> Vec<usize> { (0..NCH).filter(|&c| u.m.ch[c].on).collect() }

#[test]
fn eye_on_ch1_in_view_two_turns_on_nothing_else_in_every_layout() {
    for l in LAYOUTS {
        let mut u = only_ch1(l);
        u.views_set(2);
        u.view_go(1);
        // the eye was last set to a channel further along: the owner's source is CH1
        u.m.eye_src = WaveId::Ch(3);
        u.sheet.add.plot = 1;
        u.sheet.add.src = Some(WaveId::Ch(0));
        u.rail_plot_apply_sources(1);
        u.studio_apply_panel(super::sheet::PanelKind::Eye);
        u.eye_make();
        for _ in 0..6 { u.step(0.05); }
        assert_eq!(u.m.eye_src, WaveId::Ch(0), "{l:?}");
        assert_eq!(on(&u), vec![0], "{l:?}");
        assert_eq!((u.m.eye_b, u.m.eye_rail), (None, None), "{l:?}");
    }
}

#[test]
fn stepping_a_source_leaves_only_the_final_channel_turned_on_by_the_row() {
    for l in LAYOUTS {
        let mut u = only_ch1(l);
        u.m.eye_src = WaveId::Ch(0);
        u.adjust(Item::EyeSrc, 1);
        assert_eq!(on(&u), vec![0, 1], "{l:?}");
        u.adjust(Item::EyeSrc, 1);
        assert_eq!(on(&u), vec![0, 2], "CH2 given back {l:?}");
        u.adjust(Item::EyeSrc, 1);
        assert_eq!(on(&u), vec![0, 3], "{l:?}");
        // and back to CH1: the last one goes too
        for _ in 0..3 { u.adjust(Item::EyeSrc, -1); }
        assert_eq!(u.m.eye_src, WaveId::Ch(0));
        assert_eq!(on(&u), vec![0], "{l:?}");
    }
}

#[test]
fn a_channel_that_something_else_uses_or_the_user_turned_on_stays() {
    let mut u = only_ch1(Layout::Default);
    u.m.eye_src = WaveId::Ch(0);
    // CH2 is the user's: switched on by hand
    u.adjust(Item::ChOn(1), 1);
    u.adjust(Item::EyeSrc, 1);
    u.adjust(Item::EyeSrc, 1);
    assert_eq!(on(&u), vec![0, 1, 2]);
    // CH3 is measured: stepping away keeps it
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(2))];
    u.adjust(Item::EyeSrc, 1);
    assert_eq!(on(&u), vec![0, 1, 2, 3]);
    u.adjust(Item::EyeSrc, 1);
    assert_eq!(on(&u), vec![0, 1, 2], "CH4 given back, CH2 and CH3 kept");
    // the Spectrum on the screen uses CH4: the eye leaving it does not turn it off
    u.m.view = AView::Spectrum;
    for _ in 0..3 { u.adjust(Item::EyeSrc, 1); }
    assert_eq!(u.m.eye_src, WaveId::Ch(3));
    u.m.spec_src = WaveId::Ch(3);
    u.adjust(Item::EyeSrc, 1);
    assert_eq!(on(&u), vec![0, 1, 2, 3]);
}

#[test]
fn a_picker_sets_the_source_directly() {
    for l in LAYOUTS {
        let mut u = only_ch1(l);
        u.m.eye_src = WaveId::Ch(0);
        u.select_option(Item::EyeSrc, "CH4");
        assert_eq!(u.m.eye_src, WaveId::Ch(3), "{l:?}");
        assert_eq!(on(&u), vec![0, 3], "CH2 and CH3 were never visited {l:?}");
        u.select_option(Item::EyeSrc, "CH1");
        assert_eq!(on(&u), vec![0], "{l:?}");
        u.select_option(Item::SpecSrc, "CH3");
        assert_eq!((u.m.spec_src, on(&u)), (WaveId::Ch(2), vec![0, 2]), "{l:?}");
        u.select_option(Item::EyeB, "CH2");
        assert_eq!(on(&u), vec![0, 1, 2], "{l:?}");
        u.select_option(Item::EyeB, "Off");
        assert_eq!((u.m.eye_b, on(&u)), (None, vec![0, 2]), "{l:?}");
    }
}

#[test]
fn setting_up_the_eye_leaves_the_secondary_sources_unset() {
    for l in LAYOUTS {
        let mut u = only_ch1(l);
        u.m.eye_b = Some(WaveId::Ch(1));
        u.m.eye_rail = Some(WaveId::Ch(2));
        u.m.pw_vin = None;
        u.eye_make();
        assert_eq!((u.m.eye_b, u.m.eye_rail, u.m.pw_vin, u.m.pw_clk), (None, None, None, None), "{l:?}");
        assert_eq!(on(&u), vec![0], "{l:?}");
    }
}

#[test]
fn a_view_without_measurements_says_so_and_offers_to_add_one() {
    let mut u = only_ch1(Layout::Studio);
    u.studio_set_view(StudioView::Docked);
    u.relayout();
    for _ in 0..4 { u.step(0.05); }
    u.m.meas.clear(); u.m.meas2.clear(); u.m.cat.clear();
    assert!(u.meas_rows().is_empty());
    let g = u.studio_panel_geo().expect("the docked panel");
    let b = Ui::panel_empty_button(g.list);
    assert!(b.w > 0 && g.list.contains(b.x, b.y) && g.list.contains(b.right() - 1, b.bottom() - 1), "{b:?} in {:?}", g.list);
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    // the button is drawn: not the panel colour
    assert_ne!(cv.px[((b.y + b.h / 2) * cv.w + b.x + 2) as usize], cv.px[((g.list.y + 2) * cv.w + g.list.x + 2) as usize], "button drawn");
    assert!(u.studio_panel_tap(b.x + b.w / 2, b.y + b.h / 2));
    assert!(u.menu_open, "the Add measurement list opens");
}
