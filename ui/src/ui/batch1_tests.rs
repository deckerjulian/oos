// SPDX-License-Identifier: GPL-2.0-or-later
//! Docking and view state: the fixes of the OOS-plus audit, batch 1.
use super::studio::{MeasKey, StudioView};
use super::workspace::Action;
use super::*;

fn fresh() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.relayout();
    for _ in 0..4 { u.step(0.05); }
    u
}

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..8 { u.step(0.05); }
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0)), (Meas::Vrms, WaveId::Ch(0)), (Meas::Rise, WaveId::Ch(0)),
        (Meas::Freq, WaveId::Ch(1)), (Meas::Vpp, WaveId::Ch(1))];
    u.studio.pinned = Some(vec![MeasKey::One(Meas::Freq, WaveId::Ch(0)), MeasKey::One(Meas::Vpp, WaveId::Ch(0))]);
    u.relayout();
    u
}

fn shot(u: &mut Ui, name: &str) -> Canvas {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..6 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
    cv
}

fn many_measurements(u: &mut Ui) {
    let kinds = [Meas::Freq, Meas::Vpp, Meas::Vrms, Meas::Rise, Meas::Vmax, Meas::Vmin, Meas::Period];
    u.m.meas = kinds.iter().flat_map(|&k| [(k, WaveId::Ch(0)), (k, WaveId::Ch(1))]).collect();
}

#[test]
fn every_dock_choice_docks_the_card_again_also_the_one_already_selected() {
    let mut u = fresh();
    u.stats_on = true;
    u.relayout();
    for dock in [StripDock::Below, StripDock::Right] {
        u.m.strip_dock = dock;
        u.workspace_action(Action::MeasureFloat);
        assert!(u.results_docked().is_none(), "floated");
        u.select_option(Item::StripDock, dock.label());
        u.relayout();
        assert_eq!(u.results_docked(), Some(dock), "{dock:?}: tapping the current segment docks it");
    }
}

#[test]
fn studio_has_no_dock_item_in_the_measure_menu() {
    let mut u = studio();
    assert!(!u.items(MenuId::Measure).contains(&Item::StripDock));
    u.m.layout = Layout::Minimal;
    assert!(u.items(MenuId::Measure).contains(&Item::StripDock));
}

#[test]
fn the_docked_studio_panel_stays_beside_an_analysis_view() {
    let mut u = studio();
    u.studio.view = StudioView::Docked;
    u.relayout();
    u.m.view = AView::Spectrum;
    u.analyzer_update();
    let cv = shot(&mut u, "b1-studio-spectrum");
    let p = u.studio_panel_rect().expect("the panel keeps its place");
    let corner = cv.px[((p.y + 3) * W + p.x + 3) as usize];
    let ink = (p.y..p.bottom()).flat_map(|y| (p.x..p.right()).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] != corner).count();
    assert!(ink > 600, "the docked panel is drawn beside the analysis view ({ink})");
}

#[test]
fn move_to_floating_moves_only_that_measurement_and_keeps_the_view() {
    let mut u = studio();
    u.studio.view = StudioView::Docked;
    u.relayout();
    u.render(&mut Canvas::new(W, H));
    let key = MeasKey::One(Meas::Freq, WaveId::Ch(0));
    u.studio.popup = Some(key);
    let p = u.studio_popup_rect().expect("the badge is there");
    let r = Ui::studio_popup_row(p, 2);
    u.tap(r.x + r.w / 2, r.y + r.h / 2);
    assert_eq!(u.studio.view, StudioView::Docked, "the mode stays");
    assert!(!u.studio_is_pinned(key));
    assert!(u.studio_float_keys().contains(&key), "the floating panel lists it");
    assert!(u.studio_float_keys().contains(&MeasKey::One(Meas::Vpp, WaveId::Ch(0))), "the others stay where they were");
}

#[test]
fn the_floating_panel_lists_what_was_moved_to_it() {
    let mut u = studio();
    let key = MeasKey::One(Meas::Rise, WaveId::Ch(0));
    u.studio.floated.push(key);
    u.studio.view = StudioView::Floating;
    u.relayout();
    assert_eq!(u.studio_float_keys().len(), 3);
    shot(&mut u, "b1-studio-floating");
    let p = u.studio_panel_rect().unwrap();
    assert!(p.h >= 64 + 3 * 46, "three rows fit");
}

#[test]
fn entering_studio_shows_its_bars_even_from_full_screen() {
    let mut u = fresh();
    u.m.density = Density::Full;
    u.apply_density();
    assert!(u.full_on());
    u.select_option(Item::Layout, "Studio");
    let mut cv = Canvas::new(W, H);
    for _ in 0..3 { u.step(0.05); u.render(&mut cv); }
    assert!(!u.full_on(), "Studio starts with its bars");
}

#[test]
fn a_placement_choice_places_a_floated_eye() {
    for place in ["Full", "Side", "Under"] {
        let mut u = fresh();
        u.m.view = AView::Eye;
        u.workspace.eye_float = Some(Rect::new(360, 100, 620, 360));
        u.select_option(Item::EyePlace, place);
        assert!(u.workspace.eye_float.is_none(), "{place}: no longer floating");
    }
    // stepping the value through the menu does the same
    let mut u = fresh();
    u.m.view = AView::Eye;
    u.workspace.eye_float = Some(Rect::new(360, 100, 620, 360));
    u.adjust(Item::EyePlace, 1);
    assert!(u.workspace.eye_float.is_none());
}

#[test]
fn a_card_under_the_plot_pages_many_rows_at_once() {
    for dock in [StripDock::Below, StripDock::Above] {
        let mut u = fresh();
        many_measurements(&mut u);
        u.stats_on = true;
        u.workspace_action(Action::MeasureDock(dock));
        u.relayout();
        assert!(u.results_page_size() >= 6, "{dock:?}: {} rows per page", u.results_page_size());
        assert!(u.results_band_h() <= 170, "{dock:?}: the band stays small");
        let (head, body) = u.results_rects().unwrap();
        assert!(head.x >= 0 && head.right() <= W && body.bottom() <= H, "{dock:?} on screen");
        shot(&mut u, &format!("b1-stats-{dock:?}"));
    }
}

#[test]
fn time_cursors_keep_their_times_while_zoomed() {
    let mut u = fresh();
    u.m.cursor = CursorMode::Time;
    u.m.cur_a = 0.48;
    u.m.cur_b = 0.52;
    u.step(0.05);
    let (ta, tb) = u.cursor_times();
    u.set_zoom(true);
    u.m.zscale = u.m.tscale.saturating_sub(3);
    u.m.zdelay = u.m.delay;
    u.step(0.05);
    let (za, zb) = u.cursor_times();
    assert!((za - ta).abs() < 1e-9 && (zb - tb).abs() < 1e-9, "{ta} {tb} -> {za} {zb}");
    // further in they may lie outside the view: they only keep their time
    u.m.zscale = u.m.tscale.saturating_sub(6);
    u.step(0.05);
    let (za, zb) = u.cursor_times();
    assert!((za - ta).abs() < 1e-9 && (zb - tb).abs() < 1e-9);
    // and back out they are where they were
    u.set_zoom(false);
    u.step(0.05);
    assert!((u.m.cur_a - 0.48).abs() < 1e-3 && (u.m.cur_b - 0.52).abs() < 1e-3, "{} {}", u.m.cur_a, u.m.cur_b);
}

#[test]
fn studio_cursor_tags_stay_while_zoomed() {
    let mut u = studio();
    u.m.cursor = CursorMode::Time;
    u.m.cur_a = 0.49;
    u.m.cur_b = 0.51;
    u.step(0.05);
    let (ta, tb) = u.cursor_times();
    u.set_zoom(true);
    u.m.zscale = u.m.tscale.saturating_sub(3);
    u.m.zdelay = u.m.delay;
    u.step(0.05);
    let (za, zb) = u.cursor_times();
    assert!((za - ta).abs() < 1e-9 && (zb - tb).abs() < 1e-9);
    assert!((0.0..=1.0).contains(&u.m.cur_a) && (0.0..=1.0).contains(&u.m.cur_b), "both are in the view");
    shot(&mut u, "b1-studio-cursors-zoom");
}

#[test]
fn the_floating_studio_panel_clears_the_corner_buttons_and_the_head_row() {
    let mut u = studio();
    u.studio.view = StudioView::Floating;
    u.relayout();
    for at in [None, Some((W - 300, 0)), Some((W - 280, u.grid.y + 8)), Some((W - 40, 100))] {
        u.studio.panel.at = at;
        let p = u.studio_panel_rect().unwrap();
        let hits = |r: Rect| p.x < r.right() && r.x < p.right() && p.y < r.bottom() && r.y < p.bottom();
        assert!(!hits(u.full_btn().0) && !hits(u.zoom_btn().0), "{at:?} covers a corner button: {p:?}");
        assert!(p.y >= u.grid.y, "{at:?} stays under the head row");
    }
    u.studio.panel.at = None;
    shot(&mut u, "b1-studio-floating-corner");
}

#[test]
fn layout_and_studio_view_come_back_after_a_restart() {
    let mut u = studio();
    u.studio.view = StudioView::Docked;
    u.studio_frame_start();
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("studio_view = docked") && text.contains("layout = studio"));
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m = crate::store::parse_setup(&text).unwrap();
    v.restore_view_state();
    for _ in 0..4 { v.step(0.05); }
    assert_eq!((v.m.layout, v.studio.view), (Layout::Studio, StudioView::Docked));
    assert!(v.stacked(), "its lanes are drawn");
    assert!(crate::store::parse_setup(&text.replace("studio_view = docked", "studio_view = nonsense")).is_err());
    // a record view is kept as the plain waveform
    let mut u = studio();
    u.studio.view = StudioView::Sim;
    u.studio_frame_start();
    assert_eq!(u.m.studio_view, "waveform");
}
