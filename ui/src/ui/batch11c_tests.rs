// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 11 (part three): the zoom gestures of Studio (overview strip against the zoomed waveform), the height of the
//! overview strip, the cursor readout at a zero distance, and the extra sources in the "+ CH" popup.
use super::studio::{PickAdd, StudioView};
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.studio_set_view(StudioView::Docked);
    u
}

fn zoomed() -> Ui {
    let mut u = studio();
    u.m.tscale = u.m.tscale.max(12);
    u.m.zscale = u.m.tscale - 4;
    u.set_zoom(true);
    frame(&mut u);
    u
}

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.step(0.05);
}

fn touch(u: &mut Ui, id: i32, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id, phase, x, y });
    u.step(0.01);
}

fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, 0, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, 0, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, 0, Phase::Up, b.0, b.1);
    frame(u);
}

fn tap(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, 0, Phase::Down, x, y);
    touch(u, 0, Phase::Up, x, y);
}

fn wheel(u: &mut Ui, x: i32, y: i32, steps: i32) {
    u.handle(Ev::Wheel { x, y, steps, ctrl: false, shift: false, alt: false });
    frame(u);
}

fn pinch(u: &mut Ui, at: (i32, i32)) {
    touch(u, 0, Phase::Down, at.0, at.1);
    touch(u, 1, Phase::Down, at.0 + 60, at.1);
    for k in 1..=6 { touch(u, 1, Phase::Move, at.0 + 60 + k * 30, at.1); }
    touch(u, 1, Phase::Up, at.0 + 240, at.1);
    touch(u, 0, Phase::Up, at.0, at.1);
    frame(u);
}

fn overview(u: &Ui) -> Rect { u.split_zoom(u.grid).0 }
fn main_view(u: &Ui) -> Rect { u.wins[0] }

// ------------------------------------------------------------------------------------------------ the gestures

#[test]
fn the_wheel_and_a_pinch_over_the_zoomed_waveform_leave_the_zoom_alone() {
    let mut u = zoomed();
    let w = main_view(&u);
    let before = (u.m.zscale, u.m.zdelay);
    wheel(&mut u, w.x + w.w / 2, w.y + w.h / 2, 3);
    wheel(&mut u, w.x + w.w / 2, w.y + w.h / 2, -3);
    u.handle(Ev::Wheel { x: w.x + w.w / 2, y: w.y + w.h / 2, steps: 3, ctrl: true, shift: false, alt: false });
    assert_eq!((u.m.zscale, u.m.zdelay), before, "the wheel");
    pinch(&mut u, (w.x + w.w / 2 - 100, w.y + w.h / 2));
    assert_eq!((u.m.zscale, u.m.zdelay), before, "the pinch");
}

#[test]
fn the_wheel_and_a_pinch_over_the_overview_change_the_width_of_the_box() {
    let mut u = zoomed();
    let o = overview(&u);
    let (xa, xb) = u.zoom_window_x(o);
    let before = u.m.zscale;
    wheel(&mut u, (xa + xb) / 2, o.y + o.h / 2, 2);
    assert!(u.m.zscale < before, "narrower: {} -> {}", before, u.m.zscale);
    let (ya, yb) = u.zoom_window_x(o);
    assert!(yb - ya < xb - xa);
    let now = u.m.zscale;
    wheel(&mut u, (ya + yb) / 2, o.y + o.h / 2, -3);
    assert!(u.m.zscale > now, "wider");
    // two fingers spread: narrower
    let now = u.m.zscale;
    let (za, zb) = u.zoom_window_x(o);
    pinch(&mut u, ((za + zb) / 2 - 30, o.y + o.h / 2));
    assert!(u.m.zscale < now, "pinch: {} -> {}", now, u.m.zscale);
}

#[test]
fn a_drag_in_the_zoomed_waveform_pans_it_and_the_box_moves_the_other_way() {
    let mut u = zoomed();
    let o = overview(&u);
    let w = main_view(&u);
    let (xa, xb) = u.zoom_window_x(o);
    let z0 = u.m.zdelay;
    drag(&mut u, (w.x + w.w / 2, w.y + w.h / 2 + 40), (w.x + w.w / 2 + 120, w.y + w.h / 2 + 40));
    assert!(u.m.zdelay < z0, "the content follows the finger to the right: the window moves left");
    let (ya, yb) = u.zoom_window_x(o);
    assert!(ya < xa && yb < xb && yb - ya == xb - xa, "the box moved left, same width: {xa},{xb} -> {ya},{yb}");
    // clamped to the record
    drag(&mut u, (w.x + 20, w.y + w.h / 2 + 40), (w.x + w.w - 20, w.y + w.h / 2 + 40));
    drag(&mut u, (w.x + 20, w.y + w.h / 2 + 40), (w.x + w.w - 20, w.y + w.h / 2 + 40));
    let (ya, _) = u.zoom_window_x(o);
    assert!(ya >= o.x - 1, "inside the overview: {ya} {o:?}");
}

#[test]
fn the_overview_box_moves_resizes_and_is_drawn_new() {
    let mut u = zoomed();
    let o = overview(&u);
    let (xa, xb) = u.zoom_window_x(o);
    let y = o.y + o.h / 2;
    assert!(o.h >= 40, "a target of at least 40 px: {}", o.h);
    // inside the box: pans it, the main view follows
    let z0 = u.m.zdelay;
    drag(&mut u, ((xa + xb) / 2, y), ((xa + xb) / 2 + 60, y));
    let (ya, yb) = u.zoom_window_x(o);
    assert!(u.m.zdelay > z0 && ya > xa && yb - ya == xb - xa, "{xa},{xb} -> {ya},{yb}");
    // a side handle: resizes it
    let (w0, z0) = (yb - ya, u.m.zscale);
    drag(&mut u, (yb, y), (yb + 100, y));
    let (za, zb) = u.zoom_window_x(o);
    assert!(zb - za != w0 && u.m.zscale != z0, "resized: {w0} -> {}", zb - za);
    // outside the box: a new one
    let (za, zb) = u.zoom_window_x(o);
    let far = if za > o.x + 120 { o.x + 30 } else { o.right() - 30 };
    let before = u.m.zscale;
    drag(&mut u, (far, y), (far + if far < za { 80 } else { -80 }, y));
    assert!(u.m.zscale != before || u.zoom_window_x(o) != (za, zb), "a new box");
}

// ------------------------------------------------------------------------------------------------ overview height

#[test]
fn the_overview_height_drags_clamps_and_is_saved() {
    let mut u = zoomed();
    let (hit, handle) = u.ov_divider().expect("a divider under the overview");
    assert!(hit.h >= 40 && handle.w >= 40, "{hit:?} {handle:?}");
    let h0 = overview(&u).h;
    let (x, y) = (hit.x + hit.w / 2, hit.y + hit.h / 2);
    drag(&mut u, (x, y), (x, y + 40));
    let h1 = overview(&u).h;
    assert!(h1 > h0 + 30, "taller: {h0} -> {h1}");
    let (hit, _) = u.ov_divider().unwrap();
    drag(&mut u, (hit.x + hit.w / 2, hit.y + hit.h / 2), (hit.x + hit.w / 2, hit.y + hit.h / 2 - 25));
    assert!(overview(&u).h < h1, "shorter");
    // clamped: down to the box, up to 60 % of the plot
    let (hit, _) = u.ov_divider().unwrap();
    drag(&mut u, (hit.x + hit.w / 2, hit.y + hit.h / 2), (hit.x + hit.w / 2, 5));
    assert_eq!(overview(&u).h, 40);
    let (hit, _) = u.ov_divider().unwrap();
    drag(&mut u, (hit.x + hit.w / 2, hit.y + hit.h / 2), (hit.x + hit.w / 2, 4000));
    assert_eq!(overview(&u).h, u.grid.h * 6 / 10);
    // kept with the setup, next to the dock
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("studio_ov = "), "{}", text.lines().filter(|l| l.starts_with("studio")).collect::<Vec<_>>().join(" | "));
    let m = crate::store::parse_setup(&text).expect("reads back");
    assert_eq!(m.studio_ov, u.m.studio_ov);
    // other layouts keep 96
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m.studio_ov = 200;
    v.m.layout = Layout::Default;
    assert_eq!(overview_of(&v), 96);
}

fn overview_of(u: &Ui) -> i32 { u.split_zoom(u.grid).0.h }

// ------------------------------------------------------------------------------------------------ cursors

#[test]
fn the_cursors_start_apart_and_a_zero_distance_reads_dashes() {
    let mut u = studio();
    u.studio_cursors_add(None);
    frame(&mut u);
    let (ta, tb) = u.cursor_times();
    assert!(tb > ta, "{ta} {tb}");
    assert!((u.m.cur_a - 1.0 / 3.0).abs() < 1e-6 && (u.m.cur_b - 2.0 / 3.0).abs() < 1e-6);
    // with the zoom on too
    let mut z = zoomed();
    z.studio_cursors_add(None);
    frame(&mut z);
    let (ta, tb) = z.cursor_times();
    assert!(tb > ta);
    assert_eq!(Ui::inv_dt(0.0), "----");
    assert_eq!(Ui::inv_dt(1e-16), "----");
    assert_ne!(Ui::inv_dt(1e-6), "----");
    // the same spot: no huge frequency
    u.m.cursor = CursorMode::Time;
    u.m.cur_b = u.m.cur_a;
    let v = u.cursor_values();
    assert_eq!(v.iter().find(|(l, _)| l.starts_with("1/")).map(|x| x.1.as_str()), Some("----"));
}

// ------------------------------------------------------------------------------------------------ the + CH popup

#[test]
fn the_source_popup_lists_math_ref_and_bus_below_the_channels_and_each_opens_its_flow() {
    for (k, root) in [(PickAdd::Math, MenuId::Math), (PickAdd::Ref, MenuId::Ref), (PickAdd::Bus, MenuId::Decode)] {
        let mut u = studio();
        assert!(!u.m.ch[2].on);
        let b = u.studio_bottom();
        tap(&mut u, b.add);
        assert!(u.studio.ch_picker);
        let (chans, extras) = u.studio_picker_rows().expect("the popup");
        assert_eq!(extras.iter().map(|e| e.0).collect::<Vec<_>>(), [PickAdd::Math, PickAdd::Ref, PickAdd::Bus]);
        assert_eq!(extras.iter().map(|e| e.0.label()).collect::<Vec<_>>(), ["Add math", "Add ref", "Add bus"]);
        let (last_ch, first_extra) = (chans.last().unwrap().1, extras[0].1);
        assert!(first_extra.y >= last_ch.bottom() + 8, "set apart from the channels");
        assert!(extras.iter().all(|e| e.1.h >= 40));
        let r = extras.iter().find(|e| e.0 == k).unwrap().1;
        tap(&mut u, r);
        assert!(!u.studio.ch_picker);
        assert!(u.menu_open && u.menu_root() == root, "{k:?} opens {root:?}");
    }
}
