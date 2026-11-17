// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 27: one shared zoom for every lane, the zoom source popover (hold the Zoom button), and a horizontal scale
//! of their own for references (hold an R pill, or Fit). OOS_SHOTS=<dir> writes pictures (b27-*).
use super::sharedzoom::{RPart, ZPart};
use super::studio::StudioView;
use super::workspace::Trace;
use super::*;

fn studio() -> Ui {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    let dir = std::env::temp_dir().join(format!("oos-batch27-{}-{n}", std::process::id()));
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b27-{name}.png")), crate::store::png_of(&cv)).unwrap();
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

/// Press, hold past the long-press time, lift.
fn hold_rect(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, Phase::Down, x, y);
    u.step(LONG_PRESS + 0.1);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn set_tdiv(u: &mut Ui, tdiv: f32) {
    u.m.tscale = tscales().iter().position(|&s| (s - tdiv).abs() < tdiv * 1e-3).expect("a ladder step");
    u.m.tfine = 1.0;
    u.m.delay = 0.0;
    frame(u);
}

/// R1 "SATA I": a 12 GSa/s record of 65 536 points (about 5.5 us) of a 1.5 Gb/s pattern.
fn sata_ref(u: &mut Ui) {
    let v: Vec<f32> = (0..65_536usize).map(|i| if ((i / 8).wrapping_mul(2654435761usize) >> 7) & 1 == 1 { 0.4 } else { -0.4 }).collect();
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "SATA I".into(), t0: 0.0, dt: 1.0 / 12e9, v: v.into(), vpd: 0.2, offset: 0.0, on: true });
    u.select_workspace_source(WaveId::Ref(0));
    frame(u);
}

fn lane_views(u: &mut Ui) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for i in 0..u.wins.len() { u.cur_win = i; out.push(u.view()); }
    u.cur_win = 0;
    out
}

fn main_view(u: &Ui) -> (f32, f32) { (u.m.time_per_div(), u.m.delay) }

#[test]
fn the_zoom_box_applies_to_every_lane_at_once() {
    let mut u = studio();
    sata_ref(&mut u);
    set_tdiv(&mut u, 5e-3);
    assert_eq!(u.wins.len(), 3, "CH1, CH2 and R1 each have a lane");
    u.set_zoom(true);
    frame(&mut u);
    let z = (tscales()[u.m.zscale], u.m.zdelay);
    assert!(z.0 < 5e-3, "the zoom is finer than the main time base");
    // a lane per source, one time window for all of them
    for v in lane_views(&mut u) { assert_eq!(v, z); }
    // narrowing the box moves every lane
    let area = u.wave_area();
    u.zoom_horizontal_step(-1, area, 0.5);
    frame(&mut u);
    let z2 = (tscales()[u.m.zscale], u.m.zdelay);
    assert!(z2.0 < z.0);
    for v in lane_views(&mut u) { assert_eq!(v, z2); }
    // the overview keeps the main time base and carries every source
    assert_eq!(main_view(&u).0, 5e-3);
    shot(&mut u, "overview-all-lanes");
}

#[test]
fn holding_zoom_opens_the_source_popover_and_one_lane_can_zoom_alone() {
    let mut u = studio();
    sata_ref(&mut u);
    set_tdiv(&mut u, 5e-3);
    assert!(!u.m.zoom);
    // a hold opens the popover and does not toggle the zoom
    let b = u.zoom_btn().1;
    hold_rect(&mut u, b);
    assert!(u.studio.sz.pop, "the popover is open");
    assert!(!u.m.zoom, "the lift of a hold is not a tap");
    shot(&mut u, "zoom-popover");
    let (_, parts) = u.studio_sz_parts().expect("parts");
    assert!(parts.iter().any(|(p, _)| *p == ZPart::All));
    for t in [Trace::Channel(0), Trace::Channel(1), Trace::Reference(0)] { assert!(parts.iter().any(|(p, _)| *p == ZPart::Lane(t)), "{t:?}"); }
    // pick CH1: zoom turns on, only that lane zooms
    let r = parts.iter().find(|(p, _)| *p == ZPart::Lane(Trace::Channel(0))).unwrap().1;
    tap_rect(&mut u, r);
    assert!(u.studio.sz.pop && u.m.zoom);
    assert_eq!(u.studio.sz.lane, Some(Trace::Channel(0)));
    let zoomed = (tscales()[u.m.zscale], u.m.zdelay);
    let v = lane_views(&mut u);
    assert_eq!(v[0], zoomed, "CH1 zooms");
    assert_eq!(v[1], main_view(&u), "CH2 stays at the main time base");
    assert_eq!(v[2], main_view(&u), "R1 stays at the main time base");
    assert!(zoomed.0 < main_view(&u).0);
    assert!(u.studio_zoom_covers(0) && !u.studio_zoom_covers(1));
    // vertical zoom of the zoomed view
    let (_, parts) = u.studio_sz_parts().unwrap();
    let plus = parts.iter().find(|(p, _)| *p == ZPart::YPlus).unwrap().1;
    tap_rect(&mut u, plus);
    assert!(u.zoom_vfactor > 1.0);
    shot(&mut u, "zoom-single-lane");
    // all lanes again, and Reset zoom
    let (_, parts) = u.studio_sz_parts().unwrap();
    tap_rect(&mut u, parts.iter().find(|(p, _)| *p == ZPart::All).unwrap().1);
    assert_eq!(u.studio.sz.lane, None);
    let z = (tscales()[u.m.zscale], u.m.zdelay);
    for v in lane_views(&mut u) { assert_eq!(v, z); }
    let (_, parts) = u.studio_sz_parts().unwrap();
    tap_rect(&mut u, parts.iter().find(|(p, _)| *p == ZPart::Reset).unwrap().1);
    assert_eq!(u.zoom_vfactor, 1.0);
    assert_eq!(u.m.zscale, u.m.tscale, "the window is the whole record again");
    // a tap outside closes it; a plain tap on Zoom still toggles
    tap_at(&mut u, 300, 300);
    assert!(!u.studio.sz.pop);
    let b = u.zoom_btn().1;
    tap_rect(&mut u, b);
    assert!(!u.m.zoom, "a plain tap toggles as before");
}

#[test]
fn a_reference_with_its_own_horizontal_fills_its_lane_next_to_channels_at_5_ms() {
    let mut u = studio();
    sata_ref(&mut u);
    set_tdiv(&mut u, 5e-3);
    let span = 65_535.0 / 12e9;
    // at the main time base the record is a sliver
    assert!(span < 5e-3 * 10.0 * 0.001);
    // the Fit button of the lane: the ref gets its own scale, the live time base stays
    let fit = u.live_ref_fit_rect().expect("the Fit button is there");
    tap_rect(&mut u, fit);
    let (tdiv, delay) = u.studio.sz.ref_h.get(&0).copied().expect("R1 has its own horizontal");
    assert!((tdiv as f64 - span / 10.0).abs() < span / 10.0 * 1e-3, "{tdiv}");
    assert!((delay as f64 - span / 2.0).abs() < span * 1e-3);
    assert_eq!(u.m.time_per_div(), 5e-3, "the channels stay at 5 ms/div");
    let v = lane_views(&mut u);
    assert_eq!(v[0], main_view(&u));
    assert_eq!(v[1], main_view(&u));
    assert_eq!(v[2], (tdiv, delay));
    // the trace spans the whole lane: its colour is at both edges
    let mut cv = Canvas::new(u.scr.w, u.scr.h);
    u.render(&mut cv);
    let lane = u.wins[2];
    let col = REF_COLORS[0].1;
    let has = |x0: i32, x1: i32| (x0..x1).any(|x| (lane.y..lane.bottom()).any(|y| cv.px[(y * cv.w + x) as usize] == col));
    assert!(has(lane.x, lane.x + lane.w / 10), "the record starts at the left edge");
    assert!(has(lane.right() - lane.w / 10, lane.right()), "and ends at the right edge");
    assert!(has(lane.x + lane.w * 4 / 10, lane.x + lane.w * 6 / 10), "and is in the middle too");
    shot(&mut u, "ref-own-fit");
    // its lane labels its own time scale
    u.cur_win = 2;
    assert!(u.studio_lane_has_own_time());
    u.cur_win = 0;
    // hold the R pill: Follow main / Own, steppers, Fit
    u.studio.sz.ref_h.clear();
    let pill = u.studio_bottom().refs[0];
    hold_rect(&mut u, pill);
    let popup = u.workspace.popup.as_ref().expect("the hold menu of the R chip");
    assert_eq!(popup.title, "R1");
    let row = (0..popup.rows.len()).find(|&i| matches!(popup.rows[i].1, super::workspace::Action::RefHorizontal(0))).expect("a Horizontal row");
    assert!(popup.rows[row].0.starts_with("Horizontal: Follow main"));
    let r = popup.row_rect(row);
    tap_rect(&mut u, r);
    assert_eq!(u.studio.sz.ref_pop, Some(0));
    shot(&mut u, "ref-popover");
    tap_part(&mut u, RPart::Own);
    assert!(u.studio.sz.ref_h.contains_key(&0));
    let t0 = u.studio.sz.ref_h[&0].0;
    tap_part(&mut u, RPart::TPlus);
    assert!(u.studio.sz.ref_h[&0].0 > t0, "slower time/div");
    let p0 = u.studio.sz.ref_h[&0].1;
    tap_part(&mut u, RPart::PPlus);
    assert!(u.studio.sz.ref_h[&0].1 > p0, "position moves");
    tap_part(&mut u, RPart::Fit);
    assert!((u.studio.sz.ref_h[&0].0 as f64 - span / 10.0).abs() < span / 10.0 * 1e-3);
    tap_part(&mut u, RPart::Follow);
    assert!(u.studio.sz.ref_h.is_empty(), "Follow main drops it");
    assert_eq!(lane_views(&mut u)[2], main_view(&u));
}

#[test]
fn default_and_minimal_are_unchanged() {
    for layout in [Layout::Default, Layout::Minimal] {
        let mut u = studio();
        sata_ref(&mut u);
        set_tdiv(&mut u, 5e-3);
        u.m.layout = layout;
        frame(&mut u);
        // state that Studio would use has no effect here
        u.studio.sz.lane = Some(Trace::Channel(0));
        u.studio.sz.ref_h.insert(0, (1e-7, 1e-6));
        u.set_zoom(true);
        let z = (tscales()[u.m.zscale], u.m.zdelay);
        for i in 0..u.wins.len().max(1) { u.cur_win = i.min(u.wins.len().saturating_sub(1)); assert_eq!(u.view(), z, "{layout:?}"); }
        u.cur_win = 0;
        assert!(u.studio_zoom_lane().is_none() && u.studio_ref_own(0).is_none());
        u.set_zoom(false);
        u.studio.sz.ref_h.clear();
        // a hold on the Zoom button opens nothing
        let b = u.zoom_btn().1;
        hold_rect(&mut u, b);
        assert!(!u.studio.sz.pop, "{layout:?}");
        // Fit still moves the live time base onto the record
        u.m.layout = layout;
        let before = u.m.tscale;
        u.fit_shown_reference();
        assert_ne!(u.m.tscale, before, "{layout:?}: Fit sets the main time base as before");
        assert!(u.studio.sz.ref_h.is_empty());
    }
}

fn tap_part(u: &mut Ui, k: RPart) {
    let r = u.studio_ref_pop_parts().expect("the reference popover is open").1.iter().find(|(p, _)| *p == k).unwrap().1;
    tap_rect(u, r);
}
