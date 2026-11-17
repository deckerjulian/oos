// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 19: reference sheet, ref badge, peaks header, histogram header. One test per item that can be tested. OOS_SHOTS=<dir> writes pictures (b19-*).
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.studio_set_view(StudioView::Docked);
    frame(&mut u);
    u
}

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.step(0.02);
}

fn shot(u: &mut Ui, name: &str) {
    for _ in 0..6 { frame(u); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b19-{name}.png")), crate::store::png_of(&cv)).unwrap();
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

fn a_ref(label: &str) -> RefWave {
    RefWave { src: 0, color: 0, label: label.into(), t0: 0.0, dt: 1e-6, v: (0..100).map(|i| i as f32).collect::<Vec<_>>().into(), vpd: 1.0, offset: 0.0, on: true }
}

fn row_of(u: &Ui, it: Item) -> Option<Rect> {
    u.sheet_geo().unwrap().rows.iter().find(|r| r.it == it).map(|r| r.ctrl)
}

#[test]
fn store_goes_into_the_selected_slot_and_a_full_one_offers_replace_and_next_free() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    for _ in 0..4 { frame(&mut u); }
    u.refs[0] = Some(a_ref("R1"));
    // an empty selected slot: Store names it, and no second action is offered
    u.ref_sel = 1;
    u.open_menu(MenuId::Ref);
    for _ in 0..3 { frame(&mut u); }
    assert_eq!(u.ref_store_label(), "Store CH1 as R2");
    assert!(row_of(&u, Item::RefSaveNext).is_none());
    u.activate(Item::RefSave);
    assert!(u.refs[1].is_some() && u.refs[2].is_none(), "into R2, the selected slot");
    // a full selected slot: the button replaces it, the secondary action takes the next free one
    u.ref_sel = 0;
    frame(&mut u);
    assert_eq!(u.ref_store_label(), "Replace R1 with CH1");
    assert_eq!(u.ref_store_next_label(), "Store as R3 (next free)");
    let next = row_of(&u, Item::RefSaveNext).expect("the secondary action is offered");
    assert!(next.h >= 40, "a touch target");
    shot(&mut u, "reference-sheet");
    let before = u.refs[0].as_ref().unwrap().v.len();
    tap(&mut u, next);
    assert!(u.refs[2].is_some(), "into the next free slot");
    assert_eq!(u.refs[0].as_ref().unwrap().v.len(), before, "R1 stays");
    u.activate(Item::RefSave);
    assert_eq!(u.refs[0].as_ref().unwrap().src, 0);
    assert_ne!(u.refs[0].as_ref().unwrap().v.len(), 100, "R1 was replaced by the live record");
}

#[test]
fn the_reference_title_does_not_say_the_slot_twice() {
    let mut u = studio();
    u.refs[0] = Some(a_ref("R1"));
    u.ref_sel = 0;
    assert_eq!(u.menu_title(MenuId::Ref), "Reference R1");
    u.refs[0].as_mut().unwrap().label = String::new();
    assert_eq!(u.menu_title(MenuId::Ref), "Reference R1");
    u.refs[0].as_mut().unwrap().label = "Golden".into();
    assert_eq!(u.menu_title(MenuId::Ref), "Reference R1 \u{b7} Golden");
}

#[test]
fn the_ref_badge_has_two_rows_and_keeps_clear_of_the_fit_row() {
    let mut u = studio();
    u.refs[0] = Some(a_ref("R1"));
    u.ref_sel = 0;
    for _ in 0..4 { frame(&mut u); }
    assert!(!u.live_ref_note().is_empty());
    // the Fit row of the note is 48 px above the bottom of the lane; the badge may not go below that
    assert!(u.ref_badge_pad(u.area) >= 48, "pad {}", u.ref_badge_pad(u.area));
    assert!(super::REF_BADGE_H >= 38, "a row for the name and a row for the scale and rate");
    shot(&mut u, "ref-badge");
}

#[test]
fn the_peaks_list_has_a_fixed_header_row_above_its_first_line() {
    let u = studio();
    for pane in [true, false] {
        let b = u.spec_box(Rect::new(0, 0, 760, 300), pane);
        assert!(b.list.y - b.sort.bottom() >= super::panes::PEAK_HEAD, "pane {pane}: sort {:?} list {:?}", b.sort, b.list);
    }
}

#[test]
fn the_histogram_header_starts_right_of_its_volts_axis_and_stays_inside() {
    let a = Rect::new(100, 50, 220, 200);
    let r = Ui::hist_head_rect(a, super::HIST_AXIS_W, 400);
    assert!(r.x >= a.x + super::HIST_AXIS_W, "right of the axis");
    assert!(r.right() <= a.right(), "inside the view");
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Histogram);
    for _ in 0..4 { frame(&mut u); }
    shot(&mut u, "histogram-header");
}

#[test]
fn saving_a_channel_into_a_reference_slot_shows_a_success_toast() {
    assert!(Ui::toast_is_success("CH1 saved to R2"), "a tick, not a warning");
    assert!(!Ui::toast_is_success("Save failed: disk full"));
}
