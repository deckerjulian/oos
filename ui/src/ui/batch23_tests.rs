//! Batch 23: Autoset, Clear and Force and the Display and Storage buttons in the Studio top bar, SIM split into "Add to
//! ref" and the simulation mode, the detail line of the channel pills, and the swipe down that turns a pill off or
//! removes a badge. OOS_SHOTS=<dir> writes pictures (b23-*).
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    u.store_dir = std::env::temp_dir().join("oos-batch23");
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.studio_set_view(StudioView::Waveform);
    frame(&mut u);
    u
}

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    u.step(0.02);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..6 { frame(u); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b23-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn tap(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, Phase::Up, b.0, b.1);
    frame(u);
}

fn centre(r: Rect) -> (i32, i32) { (r.x + r.w / 2, r.y + r.h / 2) }

fn toast_text(u: &Ui) -> String { u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default() }

/// Every button of the top bar, left to right as they are drawn.
fn top_rects(u: &Ui) -> Vec<(&'static str, Rect)> {
    let t = u.studio_top();
    let mut v = vec![("run", t.run), ("single", t.single), ("live", t.badge), ("sim", t.simbtn), ("preset", t.sim), ("autoset", t.autoset), ("clear", t.clear), ("force", t.force),
        ("add", t.add), ("measure", t.measure), ("cursors", t.cursors), ("display", t.display), ("storage", t.storage), ("view", t.view), ("gear", t.gear)];
    v.retain(|(_, r)| r.w > 0);
    v
}

/// The buttons are in the bar, do not overlap, and their texts fit.
fn check_top(u: &Ui) {
    let t = u.studio_top();
    let top = u.frame().top;
    let f = u.fonts();
    let rects = top_rects(u);
    for (name, r) in &rects { assert!(r.x >= top.x && r.right() <= top.right() && r.y >= top.y && r.bottom() <= top.bottom(), "{name} {r:?} in {top:?}"); }
    for p in rects.windows(2) { assert!(p[0].1.right() <= p[1].1.x, "{} {:?} overlaps {} {:?}", p[0].0, p[0].1, p[1].0, p[1].1); }
    let labels = &super::studio::TOP_LABELS[t.tier];
    for (r, label) in [(t.add, labels[3]), (t.cursors, labels[5]), (t.display, labels[6]), (t.storage, labels[7]), (t.view, labels[8])] {
        assert!(r.w > 0 && f.mb.width(label) <= r.w - 4, "{label:?} in {r:?}");
    }
    assert!(t.acq.right() <= t.add.x, "the caption stays left of the buttons");
}

// ------------------------------------------------------------------------------------------------ top bar

#[test]
fn autoset_clear_and_force_are_front_panel_keys_and_not_in_the_top_bar() {
    let mut u = studio();
    check_top(&u);
    let t = u.studio_top();
    assert!(t.autoset.w == 0 && t.clear.w == 0 && t.force.w == 0, "the bar does not repeat the keys of the front panel");
    shot(&mut u, "topbar");
    // a tap where they were opens the acquisition page, like the rest of the read-out
    let (x, y) = centre(t.acq);
    assert!(t.acq.x < t.single.right() + 120, "the read-out starts where the buttons were: {:?}", t.acq);
    u.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
    u.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
    assert!(u.menu_open && u.menu == MenuId::Acquire);
    u.close_menu();
    // the keys act, and the Menu has the three as entries
    u.key(key::AUTO);
    assert!(toast_text(&u).contains("Auto set-up"), "{}", toast_text(&u));
    u.cleared = false;
    u.key(key::CLEAR);
    assert!(u.cleared, "Clear clears the display");
    u.toast = None;
    u.key(key::FORCE);
    assert!(toast_text(&u).contains("Force trigger"), "{}", toast_text(&u));
    let menu: Vec<String> = u.main_groups().into_iter().flat_map(|(_, items)| items).map(|it| u.item_label(it).to_string()).collect();
    for name in ["Autoset", "Clear", "Force"] { assert!(menu.iter().any(|l| l == name), "{name} in {menu:?}"); }
}

#[test]
fn display_and_storage_open_their_sheets() {
    let mut u = studio();
    let t = u.studio_top();
    assert!(t.display.w > 0 && t.storage.w > 0 && t.cursors.right() <= t.display.x && t.display.right() <= t.storage.x && t.storage.right() <= t.view.x);
    tap(&mut u, t.display);
    for _ in 0..3 { frame(&mut u); }
    assert!(u.menu_open && u.menu == MenuId::Display && u.sheet_active());
    let t = u.studio_top();
    tap(&mut u, t.storage);
    for _ in 0..3 { frame(&mut u); }
    assert!(u.menu_open && u.menu == MenuId::Storage && u.sheet_active());
    let t = u.studio_top();
    tap(&mut u, t.storage);
    assert!(!u.menu_open, "a second tap closes it");
}

#[test]
fn a_narrow_top_bar_shortens_labels_and_drops_nothing() {
    let mut u = studio();
    let normal = u.studio_top().tier;
    // the panel is the narrowest screen: a bigger text size is what makes the bar short of room
    for ts in crate::model::TEXT_SIZES {
        for scr in [Screen::PANEL, Screen { w: 1280, h: 800 }] {
            u.scr = scr;
            u.m.text_size = ts;
            for _ in 0..3 { frame(&mut u); }
            check_top(&u);
            for (name, r) in top_rects(&u) { assert!(r.w > 0, "{name} at {ts:?} {scr:?}"); }
        }
    }
    u.scr = Screen::PANEL;
    u.m.text_size = crate::model::TextSize::Larger;
    for _ in 0..3 { frame(&mut u); }
    let t = u.studio_top();
    assert!(t.tier >= normal, "larger text: labels as short or shorter (tier {} against {normal})", t.tier);
    u.m.text_size = crate::model::TextSize::Normal;
    u.scr = Screen { w: 1920, h: 1080 };
    for _ in 0..3 { frame(&mut u); }
    assert_eq!(u.studio_top().tier, 0, "full labels when there is room");
    u.scr = Screen::PANEL;
    u.m.text_size = crate::model::TextSize::Larger;
    for _ in 0..3 { frame(&mut u); }
    shot(&mut u, "topbar-narrow");
    // the shortened buttons still act
    tap(&mut u, t.display);
    assert_eq!(u.menu, MenuId::Display);
    u.close_menu();
    // in the SIM mode (Live, the preset chip) the bar still fits
    let t = u.studio_top();
    tap(&mut u, t.simbtn);
    u.wait_sim_generation();
    for _ in 0..6 { frame(&mut u); }
    check_top(&u);
    assert!(u.studio_top().badge.w > 0);
}

// ------------------------------------------------------------------------------------------------ SIM

fn open_sim_sheet(u: &mut Ui) {
    u.open_menu(MenuId::Sim);
    for _ in 0..4 { frame(u); }
    assert!(u.menu_open && u.menu == MenuId::Sim && u.sheet_active());
}

#[test]
fn the_sim_sheet_adds_a_signal_to_a_ref_without_entering_the_sim_mode() {
    let mut u = studio();
    open_sim_sheet(&mut u);
    // the sheet names the two things apart: Add to R<n> and Run simulation mode
    let g = u.sheet_geo().expect("the sheet");
    let labels: Vec<String> = g.foot.iter().map(|(it, _)| u.sheet_label(*it)).collect();
    assert!(labels.contains(&"Add to R1".to_string()) && labels.contains(&"Run simulation mode".to_string()), "{labels:?}");
    assert!(!labels.iter().any(|l| l.contains("Start") || l == "Play"));
    u.sim_preset_set("SATA I");
    assert!(u.sim_slot_set(0));
    u.adjust(Item::SimSlot, 1);
    assert_eq!(u.sim_slot(), 1);
    let g = u.sheet_geo().unwrap();
    assert!(g.foot.iter().any(|(it, _)| u.sheet_label(*it) == "Add to R2"));
    shot(&mut u, "sim-sheet");
    assert!(u.sim_slot_set(0));
    let was_on = (u.m.ch[0].on, u.m.ch[1].on);
    let g = u.sheet_geo().unwrap();
    let add = g.foot.iter().find(|(it, _)| *it == Item::SimAddRef).unwrap().1;
    tap(&mut u, add);
    u.wait_sim_generation();
    for _ in 0..4 { frame(&mut u); }
    let r1 = u.refs[0].as_ref().expect("R1 holds the signal");
    assert_eq!(r1.label, "SATA I");
    assert!(r1.on);
    assert!(!u.record_mode() && !u.session_is_sim && !u.sim_playing(), "no simulation mode");
    assert_eq!((u.m.ch[0].on, u.m.ch[1].on), was_on, "no channel switched off");
    let first = std::sync::Arc::clone(&r1.v);
    // another signal into R2: R1 keeps its own
    u.sim_preset_set("SATA II");
    assert!(u.sim_slot_set(1));
    u.activate(Item::SimAddRef);
    u.wait_sim_generation();
    for _ in 0..4 { frame(&mut u); }
    assert_eq!(u.refs[1].as_ref().expect("R2").label, "SATA II");
    let r1 = u.refs[0].as_ref().expect("R1 is still there");
    assert_eq!(r1.label, "SATA I");
    assert!(std::sync::Arc::ptr_eq(&r1.v, &first), "R1 was not touched");
    assert!(!u.record_mode());
    assert_eq!((u.m.ch[0].on, u.m.ch[1].on), was_on);
    // the pills say which signal each ref holds
    assert_eq!(u.studio_ref_detail(0), "SATA I");
    assert_eq!(u.studio_ref_detail(1), "SATA II");
    u.close_menu();
    u.m.tscale = 18.min(tscales().len() - 1);
    shot(&mut u, "two-refs-sim");
}

#[test]
fn the_sim_button_runs_the_simulation_mode_and_live_returns() {
    let mut u = studio();
    assert!(!u.record_mode());
    let t = u.studio_top();
    assert!(t.simbtn.w > 0 && t.badge.w == 0);
    // no ref is shown, so the SIM sheet's signal plays (a shown ref plays instead: batch 24)
    u.sim_preset_set("SATA I");
    tap(&mut u, t.simbtn);
    u.wait_sim_generation();
    for _ in 0..8 { frame(&mut u); }
    assert!(u.record_mode() && u.session_is_sim, "the SIM button enters the simulation mode");
    let t = u.studio_top();
    assert!(t.badge.w > 0 && t.simbtn.w == 0, "Live is highlighted in the bar");
    check_top(&u);
    shot(&mut u, "sim-mode");
    tap(&mut u, t.badge);
    assert!(!u.record_mode() && !u.session_is_sim);
    let t = u.studio_top();
    assert!(t.badge.w == 0 && t.simbtn.w > 0);
}

#[test]
fn adding_while_the_simulation_runs_goes_to_live_and_adds_one_more_ref() {
    let mut u = studio();
    u.activate(Item::SimPlay);
    u.wait_sim_generation();
    assert!(u.session_is_sim);
    let before = u.refs.iter().filter(|r| r.is_some()).count();
    assert_eq!(before, 1, "the played signal is in its slot");
    // another signal: the knobs of now, not those from before Play
    u.select_option(Item::SimPreset, "USB 2.0 HS");
    u.wait_sim_generation();
    u.activate(Item::SimAddRef);
    u.wait_sim_generation();
    for _ in 0..6 { frame(&mut u); }
    assert!(!u.sim_mode(), "Live while the new ref is made");
    assert_eq!(u.refs.iter().filter(|r| r.is_some()).count(), before + 1, "{}", toast_text(&u));
    assert!(u.refs[1].as_ref().is_some_and(|r| r.label == "USB 2.0 HS" && r.on), "the first free slot has the new signal");
    assert!(u.refs[0].as_ref().is_some_and(|r| r.on), "the first signal stays shown");
    // Play now runs both
    u.activate(Item::SimPlay);
    assert!(u.sim_mode() && u.sim_refs == vec![0, 1], "{:?}", u.sim_refs);
}

// ------------------------------------------------------------------------------------------------ pills

#[test]
fn the_channel_pills_have_a_detail_line_with_coupling_probe_and_offset() {
    let mut u = studio();
    u.m.ch[0].coupling = Coupling::Dc;
    u.m.ch[0].probe = 1.0;
    u.m.ch[0].offset = 4.75;
    u.m.ch[1].coupling = Coupling::Ac;
    u.m.ch[1].probe = 10.0;
    u.m.ch[1].offset = -0.5;
    for _ in 0..3 { frame(&mut u); }
    assert_eq!(u.studio_ch_detail(0, 400), "DC \u{b7} 1\u{d7} \u{b7} 4.75 V");
    assert_eq!(u.studio_ch_detail(1, 400), "AC \u{b7} 10\u{d7} \u{b7} -500 mV");
    // short of room the offset goes first, then the probe: never wider than the pill
    let f = u.fonts();
    let two = f.s.width("DC \u{b7} 1\u{d7}");
    assert_eq!(u.studio_ch_detail(0, two + 1), "DC \u{b7} 1\u{d7}");
    assert_eq!(u.studio_ch_detail(0, f.s.width("DC") + 1), "DC");
    let b = u.studio_bottom();
    let bottom = u.frame().bottom;
    for c in 0..2 {
        let r = b.ch[c];
        assert!(r.h <= 30 && r.bottom() <= bottom.bottom() && r.y >= bottom.y, "the pill stays in the bar: {r:?} in {bottom:?}");
        let text = u.studio_ch_detail(c, r.w - 14);
        assert!(f.s.width(&text) <= r.w - 14, "{text:?} fits the pill {r:?}");
    }
    // R and M pills say what they are, in the room they have
    u.activate(Item::SimAddRef);
    u.wait_sim_generation();
    u.m.math = MathOp::Add;
    u.relayout();
    for _ in 0..4 { frame(&mut u); }
    let b = u.studio_bottom();
    assert!(b.refs[0].w > 0 && b.math[0].w > 0);
    assert!(b.refs[0].h == b.ch[0].h && b.math[0].h == b.ch[0].h);
    assert!(f.s.width("SATA I") + 6 <= b.refs[0].w, "the ref pill is as wide as its signal name");
    assert!(f.s.width("A+B") + 6 <= b.math[0].w);
    shot(&mut u, "ch-pills");
}

// ------------------------------------------------------------------------------------------------ swipe down

fn undo_button(u: &Ui) -> Rect { u.md_undo_bar().expect("the Undo bar").1 }

#[test]
fn a_swipe_down_on_a_channel_pill_turns_it_off_and_undo_brings_it_back() {
    let mut u = studio();
    assert!(u.m.ch[0].on && u.m.ch[1].on);
    let r = u.studio_bottom().ch[1];
    // a tap still opens the channel sheet
    tap(&mut u, r);
    assert_eq!(u.menu, MenuId::Channel(1));
    u.close_menu();
    // a horizontal drag does not turn it off
    let (x, y) = centre(r);
    drag(&mut u, (x, y), (x + 80, y));
    assert!(u.m.ch[1].on, "a sideways drag keeps it on");
    u.close_menu();
    // a short drag down that stays on the screen does not either
    drag(&mut u, (x, y), (x, y + 10));
    assert!(u.m.ch[1].on);
    u.close_menu();
    drag(&mut u, (x, y), (x, H - 1));
    assert!(!u.m.ch[1].on, "CH2 is off");
    assert!(u.m.ch[0].on);
    assert!(u.md_undo_bar().is_some());
    shot(&mut u, "swipe-undo");
    let b = undo_button(&u);
    tap(&mut u, b);
    assert!(u.m.ch[1].on, "Undo brings CH2 back");
    assert!(u.md_undo_bar().is_none());
}

#[test]
fn a_swipe_down_on_ref_and_math_pills_turns_them_off_and_undo_restores() {
    let mut u = studio();
    u.sim_preset_set("SATA I");
    u.sim_slot_set(2);
    u.activate(Item::SimAddRef);
    u.wait_sim_generation();
    u.m.math = MathOp::Add;
    u.relayout();
    for _ in 0..4 { frame(&mut u); }
    let b = u.studio_bottom();
    let (x, y) = centre(b.refs[2]);
    drag(&mut u, (x, y), (x, H - 2));
    assert!(!u.refs[2].as_ref().unwrap().on, "R3 is off");
    let btn = undo_button(&u);
    tap(&mut u, btn);
    assert!(u.refs[2].as_ref().unwrap().on);
    let b = u.studio_bottom();
    let (x, y) = centre(b.math[0]);
    drag(&mut u, (x, y), (x, H - 2));
    assert_eq!(u.m.math_ch(0).op, MathOp::Off, "M1 is off");
    let btn = undo_button(&u);
    tap(&mut u, btn);
    assert_eq!(u.m.math_ch(0).op, MathOp::Add);
}

fn swipe_badge(style_classic: bool) {
    let mut u = studio();
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vamp, WaveId::Ch(1))];
    u.ensure_meas_own();
    u.studio_set_classic(style_classic);
    for _ in 0..6 { frame(&mut u); }
    assert!(u.studio_badges_visible(), "the badge row");
    let n = u.studio_keys().len();
    let (key, r) = u.studio_badges().first().copied().expect("a badge");
    let (x, y) = centre(r);
    // a short drag down is no removal
    drag(&mut u, (x, y), (x, y + 12));
    assert_eq!(u.studio_keys().len(), n);
    u.studio.popup = None;
    drag(&mut u, (x, y), (x, H - 1));
    assert_eq!(u.studio_keys().len(), n - 1, "the badge is removed");
    assert!(u.studio_index(key).is_none());
    assert!(u.md_undo_bar().is_some());
    if !style_classic { shot(&mut u, "swipe-badge"); }
    let b = undo_button(&u);
    tap(&mut u, b);
    assert_eq!(u.studio_keys().len(), n, "Undo brings it back");
    assert!(u.studio_index(key).is_some());
}

#[test]
fn a_swipe_down_on_a_badge_removes_it_in_both_styles() {
    swipe_badge(false);
    swipe_badge(true);
}
