// SPDX-License-Identifier: GPL-2.0-or-later
//! Docking and view state: the fixes of the OOS-plus audit, batch 1 (PR #8; the Studio tests are not taken). The
//! measurement card docks with the compact bottom bar (Display > Bottom bar).
use super::workspace::Action;
use super::*;

fn fresh() -> Ui {
    super::testkit::fresh(true, 4)
}

fn shot(u: &mut Ui, name: &str) -> Canvas {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..6 {
        u.step(0.05);
    }
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
    for dock in [StripDock::Under, StripDock::Right] {
        u.m.strip_dock = dock;
        u.workspace_action(Action::MeasureFloat);
        assert!(u.results_docked().is_none(), "floated");
        u.select_option(Item::StripDock, dock.label());
        u.relayout();
        assert_eq!(u.results_docked(), Some(dock), "{dock:?}: tapping the current segment docks it");
    }
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
    for dock in [StripDock::Under, StripDock::Above] {
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
