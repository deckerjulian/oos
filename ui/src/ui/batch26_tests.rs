// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 26: the trigger level slider (a tap on the level tag), loaded refs that stay in Live, and floating views that
//! move freely unless they are dragged by their dock handle. OOS_SHOTS=<dir> writes pictures (b26-*).
use super::dock::{DockDrag, VKind, STRIP_H};
use super::numpad::NumKey;
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    let dir = std::env::temp_dir().join(format!("oos-batch26-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    u.store_dir = dir;
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
    u.step(0.05);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..4 { frame(u); }
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b26-{name}.png")), crate::store::png_of(&cv)).unwrap();
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

fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, Phase::Up, b.0, b.1);
    frame(u);
}

fn csv_bytes() -> Vec<u8> {
    let mut csv = String::from("Time (s),Voltage (V)\n");
    for i in 0..4_000 { csv.push_str(&format!("{:e},{}\n", i as f64 * 1e-7, (i as f64 * 0.02).sin())); }
    csv.into_bytes()
}

fn load_csv(u: &mut Ui, slot: usize, name: &str) {
    std::fs::write(u.store_dir.join(name), csv_bytes()).unwrap();
    u.load_waveform_bytes_at(&csv_bytes(), name, slot).expect("the CSV loads");
}

fn open_slider(u: &mut Ui) {
    for _ in 0..3 { frame(u); }
    let tag = u.trigger_tag_hit().expect("the trigger level tag is on the plot");
    assert!(tag.h >= 40, "a large touch target: {tag:?}");
    tap_at(u, tag.x + tag.w / 2, tag.y + tag.h / 2);
    assert!(u.studio.trig_slider, "a tap on the tag opens the slider");
}

#[test]
fn a_tap_on_the_trigger_tag_opens_a_slider_that_sets_the_level_live() {
    let mut u = studio();
    assert!(u.m.trig_ref.is_none());
    open_slider(&mut u);
    assert!(!u.menu_open, "the slider is not the Trigger sheet");
    let g = u.trig_slider_geo();
    assert!(g.panel.w >= 120 && g.fifty.h >= 44 && g.value.h >= 44, "large targets");
    shot(&mut u, "trigger-slider");
    let range = u.trigger_vpd(false) as f64 * 5.0;
    // the thumb dragged up: the level rises with the finger
    let before = u.m.trig_level;
    let ty = u.trig_slider_thumb_y();
    drag(&mut u, (g.x, ty), (g.x, ty - 60));
    assert!(u.studio.trig_slider, "the slider stays open after a drag");
    assert!(u.m.trig_level > before, "{} > {before}", u.m.trig_level);
    let mid = u.m.trig_level;
    // dragging as the finger moves, not only on lift
    let t = u.trig_slider_thumb_y();
    touch(&mut u, Phase::Down, g.x, t);
    touch(&mut u, Phase::Move, g.x, g.top + 5);
    let live = u.m.trig_level;
    assert!(live > mid, "live update while held");
    touch(&mut u, Phase::Up, g.x, g.top + 40);
    // to the end: the limit of the range, not past it
    let t = u.trig_slider_thumb_y();
    drag(&mut u, (g.x, t), (g.x, g.top - 80));
    assert!((u.m.trig_level as f64 - range).abs() < range * 1e-3, "{} vs {range}", u.m.trig_level);
    let t = u.trig_slider_thumb_y();
    drag(&mut u, (g.x, t), (g.x, g.bottom + 80));
    assert!((u.m.trig_level as f64 + range).abs() < range * 1e-3);
    shot(&mut u, "trigger-slider-low");
    // 50%: the middle of the signal
    let t = u.trig_slider_thumb_y();
    drag(&mut u, (g.x, t), (g.x, g.top));
    let s = u.stats[u.m.trig_src];
    assert!(s.valid, "the demo signal is measured");
    tap_at(&mut u, g.fifty.x + g.fifty.w / 2, g.fifty.y + g.fifty.h / 2);
    assert!((u.m.trig_level - (s.vmax + s.vmin) / 2.0).abs() < 0.05 * (s.vmax - s.vmin).abs().max(1e-3), "level {} for {}..{}", u.m.trig_level, s.vmin, s.vmax);
    assert!(u.studio.trig_slider);
    // the value opens the numeric pad; its entry moves the thumb
    tap_at(&mut u, g.value.x + g.value.w / 2, g.value.y + g.value.h / 2);
    assert!(u.numpad.as_ref().is_some_and(|p| p.it == Item::TrigLevel), "the value opens the pad");
    shot(&mut u, "trigger-slider-pad");
    u.numpad_key(NumKey::Cancel);
    assert!(u.numpad.is_none() && u.studio.trig_slider);
    // a tap outside closes it
    let o = (u.grid.x + 20, u.grid.y + 20);
    tap_at(&mut u, o.0, o.1);
    assert!(!u.studio.trig_slider, "a tap outside closes the slider");
}

#[test]
fn the_slider_works_for_a_ref_trigger_and_the_classic_layouts_keep_the_sheet() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    u.set_trigger_source(WaveId::Ref(0), false);
    for _ in 0..4 { frame(&mut u); }
    assert_eq!(u.m.trig_ref, Some(0));
    open_slider(&mut u);
    let g = u.trig_slider_geo();
    let before = u.m.trig_level;
    let ty = u.trig_slider_thumb_y();
    drag(&mut u, (g.x, ty), (g.x, ty + 50));
    assert!(u.m.trig_level < before, "the ref's level follows");
    // Default keeps the Trigger sheet on the tag
    let mut d = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..10 { d.step(0.05); }
    frame(&mut d);
    assert_ne!(d.m.layout, Layout::Studio);
    let r = d.trigger_marker_rects()[0];
    d.plot_tap(r.x + r.w / 2, r.y + r.h / 2);
    assert!(!d.studio.trig_slider && d.menu_open && d.menu_root() == MenuId::Trigger);
}

#[test]
fn a_loaded_ref_keeps_its_pill_and_trace_when_sim_gives_way_to_live() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    assert!(u.refs[0].as_ref().unwrap().on);
    u.activate(Item::SimPlay);
    assert_eq!(u.sim_refs, vec![0]);
    assert_eq!(u.m.trig_ref, Some(0), "SIM triggers on the ref");
    for _ in 0..6 { frame(&mut u); }
    u.return_to_live();
    for _ in 0..3 { frame(&mut u); }
    assert!(!u.sim_mode());
    let b = u.studio_bottom();
    assert!(b.ch[0].w > 0 && b.ch[1].w > 0 && b.refs[0].w > 0, "CH1, CH2 and R1 pills in Live");
    assert_eq!(u.shown_refs(), vec![0], "the trace is shown");
    assert_eq!(u.m.trig_ref, None, "the trigger is on a live channel again");
    assert!(u.trigger_source_label().starts_with("CH"));
    shot(&mut u, "live-ch-ref");
    // the pill toggles the Reference sheet, and the ref can be hidden and shown again
    let r = b.refs[0];
    tap_at(&mut u, r.x + r.w / 2, r.y + r.h / 2);
    assert!(u.menu_open && u.menu == MenuId::Ref);
}

#[test]
fn a_ref_stays_shown_when_live_returns_without_a_saved_session() {
    let mut u = studio();
    load_csv(&mut u, 0, "ref1.csv");
    // a record session that did not remember the live one (the trigger moved to the ref from the Trigger sheet)
    assert!(u.set_trigger_source(WaveId::Ref(0), false));
    assert!(u.record_mode());
    load_csv(&mut u, 1, "ref2.csv");
    u.workflow_hold = None;
    u.return_to_live();
    for _ in 0..3 { frame(&mut u); }
    assert!(!u.record_mode());
    assert_eq!(u.shown_refs(), vec![0, 1], "both refs are still shown");
    let b = u.studio_bottom();
    assert!(b.refs[0].w > 0 && b.refs[1].w > 0);
    assert_eq!(u.m.trig_ref, None);
    assert!(u.trigger_source_label().starts_with("CH"));
    assert!(u.m.ch[u.m.trig_src].on, "a channel that is on");
    // Default and Minimal keep their old behaviour: Live hides the refs of a record session
    let mut d = Ui::new(Source::Demo { t: 0.0 });
    for _ in 0..10 { d.step(0.05); }
    d.load_waveform_bytes_at(&csv_bytes(), "r.csv", 0).unwrap();
    d.set_trigger_source(WaveId::Ref(0), false);
    d.workflow_hold = None;
    d.return_to_live();
    assert!(d.shown_refs().is_empty());
}

#[test]
fn the_trigger_source_returns_to_the_live_channel_it_had() {
    let mut u = studio();
    u.m.set_trigger_wave(WaveId::Ch(1), false);
    load_csv(&mut u, 0, "ref1.csv");
    u.activate(Item::SimPlay);
    assert_eq!(u.m.trig_ref, Some(0));
    u.return_to_live();
    assert_eq!((u.m.trig_ref, u.m.trig_src), (None, 1), "CH2 again, not R1");
}

/// A point on a floating view's title clear of its handle and buttons.
fn title_point(r: Rect) -> (i32, i32) { (r.x + 90, r.y + STRIP_H / 2) }

fn handle_point(r: Rect) -> (i32, i32) { (r.x + 14, r.y + STRIP_H / 2) }

fn float_measure(u: &mut Ui) -> Rect {
    u.studio_set_view(StudioView::Floating);
    for _ in 0..3 { frame(u); }
    u.dock_float_rect(VKind::Measure).expect("a floating panel")
}

#[test]
fn a_floating_view_dragged_by_its_title_moves_freely_into_a_corner_without_docking() {
    let mut u = studio();
    let r = float_measure(&mut u);
    let reg = u.dock_region();
    let from = title_point(r);
    // over the middle of the waveform first: no dock target is shown
    let w = u.dock_leaf(VKind::Wave).unwrap();
    touch(&mut u, Phase::Down, from.0, from.1);
    touch(&mut u, Phase::Move, w.x + w.w / 2, w.y + w.h / 2);
    touch(&mut u, Phase::Move, w.x + w.w / 2 + 3, w.y + w.h / 2 + 3);
    match &u.studio.dock.drag { Some(DockDrag::Move { free, .. }) => assert!(*free, "a free move"), d => panic!("not dragging: {d:?}") }
    frame(&mut u);
    // on to the top-right corner (past the screen edge: clamped)
    touch(&mut u, Phase::Move, reg.right() + 200, reg.y - 100);
    touch(&mut u, Phase::Up, reg.right() + 200, reg.y - 100);
    frame(&mut u);
    assert_eq!(u.studio.view, StudioView::Floating, "it did not dock");
    let f = u.dock_float_rect(VKind::Measure).unwrap();
    assert_eq!(f.y, reg.y, "at the top: {f:?} of {reg:?}");
    assert!(f.right() >= reg.right() - 60, "in the top-right corner (clear of the plot's corner buttons): {f:?} of {reg:?}");
    assert_eq!(f.w, r.w);
    shot(&mut u, "float-top-right");
    // and the bottom-left, over the waveform
    let from = title_point(f);
    drag(&mut u, from, (reg.x - 300, reg.bottom() + 300));
    let f = u.dock_float_rect(VKind::Measure).unwrap();
    assert_eq!(f.x, reg.x);
    assert!(f.bottom() >= reg.bottom() - 120, "in the bottom-left corner: {f:?} of {reg:?}");
    assert_eq!(u.studio.view, StudioView::Floating);
}

#[test]
fn a_floating_view_dragged_by_its_handle_shows_the_dock_targets_and_docks() {
    let mut u = studio();
    let r = float_measure(&mut u);
    shot(&mut u, "float-handle");
    let g = handle_point(r);
    let w = u.dock_leaf(VKind::Wave).unwrap();
    // the right edge of the waveform
    let target = (w.right() - 20, w.y + w.h / 2);
    touch(&mut u, Phase::Down, g.0, g.1);
    touch(&mut u, Phase::Move, g.0 + 30, g.1 + 30);
    for k in 1..=6 { touch(&mut u, Phase::Move, g.0 + (target.0 - g.0) * k / 6, g.1 + (target.1 - g.1) * k / 6); }
    match &u.studio.dock.drag { Some(DockDrag::Move { free, .. }) => assert!(!*free), d => panic!("not dragging: {d:?}") }
    let DockDrag::Move { kind, x, y, grab, .. } = u.studio.dock.drag.clone().unwrap() else { unreachable!() };
    assert_ne!(u.dock_drop_at(kind, x, y, grab).0, super::dock::Drop::Stay, "a dock target is shown");
    shot(&mut u, "float-dock-target");
    touch(&mut u, Phase::Up, target.0, target.1);
    frame(&mut u);
    assert_eq!(u.studio.view, StudioView::Docked, "docked by the handle");
    assert!(u.dock_float_rect(VKind::Measure).is_none());
}


#[test]
fn auto_eye_keeps_the_source_the_sheet_shows_a_ref_included() {
    let mut u = studio();
    // R1: a 1 Gb/s square wave at 50 ps
    let v: Vec<f32> = (0..20_000).map(|i| if i / 20 % 2 == 0 { -1.0 } else { 1.0 }).collect();
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "Eye test".into(), t0: -1e-6, dt: 50e-12, v: v.into(), vpd: 0.5, offset: 0.0, on: true });
    // CH2 is the selected lane, the sheet's source is R1
    u.select_workspace_source(WaveId::Ch(1));
    u.open_menu(MenuId::Eye);
    let name = u.wave_name(WaveId::Ref(0));
    u.select_option(Item::EyeSrc, &name);
    assert_eq!(u.m.eye_src, WaveId::Ref(0));
    u.activate(Item::EyeAct);
    for _ in 0..30 { frame(&mut u); }
    assert_eq!(u.m.eye_src, WaveId::Ref(0), "Auto keeps R1");
    assert!((u.m.eye_rate / 1e9 - 1.0).abs() < 0.02, "the rate comes from R1's record: {}", u.m.eye_rate);
    // a source that is gone falls back to the selected one
    u.refs[0] = None;
    u.select_workspace_source(WaveId::Ch(1));
    u.eye_make();
    assert_eq!(u.m.eye_src, WaveId::Ch(1));
}
