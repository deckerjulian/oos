//! Regression tests for the compact (Minimal) chrome audit.
use super::*;
use super::workspace::Action;

#[cfg(audit_shots)]
mod audit_shots;

fn fresh() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.relayout();
    for _ in 0..4 { u.step(0.05); }
    u
}

#[test]
fn the_last_visible_source_cannot_be_switched_off() {
    let mut u = fresh();
    assert_eq!(u.trace_sources().len(), 1);
    u.workspace_action(Action::Source(WaveId::Ch(0)));
    assert!(u.m.ch[0].on && u.trace_sources().len() == 1, "CH1 stays on");
}

fn two_channels() -> Ui {
    let mut u = fresh();
    u.activate_live_channel(1);
    u.relayout();
    u
}

#[test]
fn overlay_keeps_a_split_and_stacked_leaves_a_consistent_density() {
    for density in [Density::Split, Density::Four] {
        let mut u = two_channels();
        u.m.density = density;
        u.apply_density();
        u.relayout();
        let layout = u.m.win_layout;
        assert_ne!(layout, WinLayout::One);
        u.trace_mode(false);
        assert_eq!((u.m.density, u.m.win_layout, u.workspace.stacked), (density, layout, false), "Overlay keeps the split");
        u.trace_mode(true);
        assert!(u.workspace.stacked && u.m.win_layout == WinLayout::One && u.m.density == Density::Scope);
        // choosing a split again drops Stacked, so the highlighted pill is the one in force
        u.m.density = density;
        u.apply_density();
        assert!(!u.workspace.stacked && u.m.win_layout != WinLayout::One);
    }
}

fn with_refs(u: &mut Ui, n: usize) {
    for i in 0..n {
        u.refs[i] = Some(RefWave { src: 0, color: i, label: format!("source-{i}"), t0: -2e-9, dt: 1e-10,
            v: ((0..128).map(|k| ((k as f32 + i as f32) * 0.2).sin() * 0.2).collect::<Vec<_>>()).into(), vpd: 0.1, offset: 0.0, on: true });
    }
}

fn tap(u: &mut Ui, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
    u.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
}

#[test]
fn sources_that_do_not_fit_sit_behind_a_plus_pill_that_opens_the_popup() {
    let mut u = fresh();
    assert!(u.source_overflow().is_none());
    for c in 1..4 { u.activate_live_channel(c); }
    with_refs(&mut u, NREF);
    u.relayout();
    let (hidden, pill) = u.source_overflow().expect("14 sources do not fit");
    let shown = u.source_pills();
    assert_eq!(shown.len() + hidden, 4 + NREF);
    assert!(shown.iter().all(|(_, r)| r.x + r.w <= pill.x), "pills do not overlap the +N pill");
    let (overlay, ..) = u.footer_rects();
    assert!(pill.x + pill.w <= overlay.x, "the +N pill stays left of Overlay");
    tap(&mut u, pill.x + 5, pill.y + 5);
    let popup = u.workspace.popup.as_ref().expect("Sources popup");
    assert!(popup.rows.len() >= 4 + NREF, "every source is in the popup");
}

#[test]
fn context_menu_stays_inside_the_plot_and_marks_the_trace_mode_in_force() {
    let mut u = fresh();
    for (x, y) in [(5, 45), (1020, 595), (500, 250), (1000, 100)] {
        u.context_menu(x, y);
        let p = u.workspace.popup.as_ref().unwrap().rect;
        let g = u.grid;
        assert!(p.x >= g.x && p.right() <= g.right() && p.y >= g.y && p.bottom() <= g.bottom(), "({x},{y}) {p:?} in {g:?}");
    }
    u.context_menu(500, 250);
    let row = |u: &Ui, i: i32| { let p = u.workspace.popup.as_ref().unwrap().rect; Rect::new(p.x + p.w - 30, p.y + 4 + i * 30, 24, 30) };
    let ink = |u: &mut Ui, r: Rect| { let mut cv = Canvas::new(W, H); u.render(&mut cv); (r.y..r.y + r.h).flat_map(|y| (r.x..r.x + r.w).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] == ACCENT).count() };
    let (overlay_row, stacked_row) = (row(&u, 0), row(&u, 1));
    assert!(ink(&mut u, overlay_row) > 0 && ink(&mut u, stacked_row) == 0, "check on Overlay");
    u.workspace.stacked = true;
    assert!(ink(&mut u, overlay_row) == 0 && ink(&mut u, stacked_row) > 0, "check on Stacked");
}

#[test]
fn a_floating_measurement_card_never_covers_the_zoom_bar_or_the_corner_buttons() {
    let mut u = fresh();
    u.set_zoom(true);
    for place in [None, Some((W - 220, TOP.h + 32)), Some((600, 60)), Some((W - 40, 140))] {
        u.workspace_action(Action::MeasureFloat);
        if place.is_some() { u.workspace.measure_float = place; }
        let (head, body) = u.results_rects().unwrap();
        let card = Rect::new(head.x, head.y, head.w, head.h + body.h);
        let hits = |r: Rect| card.x < r.right() && r.x < card.right() && card.y < r.bottom() && r.y < card.bottom();
        assert!(!hits(u.full_btn().0) && !hits(u.zoom_btn().0), "{place:?} covers a corner button: {card:?}");
        assert!(!hits(u.zoom_readout()), "{place:?} covers the zoom bar: {card:?}");
    }
}

#[test]
fn a_docked_measurement_card_reflows_the_plot_instead_of_covering_it() {
    for dock in StripDock::ALL {
        let mut u = fresh();
        u.workspace_action(Action::MeasureDock(dock));
        u.relayout();
        assert_eq!(u.m.strip_dock, dock, "one dock state");
        let (head, body) = u.results_rects().unwrap();
        let card = Rect::new(head.x, head.y, head.w, head.h + body.h);
        let g = u.grid;
        let apart = card.x >= g.right() || g.x >= card.right() || card.y >= g.bottom() || g.y >= card.bottom();
        assert!(apart, "{dock:?}: card {card:?} overlaps the plot {g:?}");
        let (full, zoom) = (u.full_btn().0, u.zoom_btn().0);
        let hits = |r: Rect| card.x < r.right() && r.x < card.right() && card.y < r.bottom() && r.y < card.bottom();
        assert!(!hits(full) && !hits(zoom), "{dock:?} covers a corner button");
        assert!(card.y >= TOP.h && card.bottom() <= BOTTOM.y + 1 && card.x >= 0 && card.right() <= W, "{dock:?} {card:?} on screen");
    }
    // Statistics respects the Dock choice of the Measure menu
    let mut u = fresh();
    u.stats_on = true;
    u.m.strip_dock = StripDock::Under;
    let before = u.grid;
    u.adjust(Item::StripDock, 1);
    u.relayout();
    assert!(u.results_docked().is_some());
    assert_ne!(u.grid, before);
    // Default layout keeps the stock overlay
    let mut u = fresh();
    u.m.minimal = false;
    u.workspace_action(Action::MeasureDock(StripDock::Left));
    assert!(u.results_docked().is_none());
}

#[test]
fn reference_badges_are_fanned_out_and_the_legend_is_cut_with_a_count() {
    let mut taken = Vec::new();
    for _ in 0..6 {
        let spot = badge_spot(&taken, 280, 42, 480);
        assert!(taken.iter().all(|t: &i32| (t - spot).abs() >= 36), "{spot} overlaps {taken:?}");
        taken.push(spot);
    }
    let mut u = fresh();
    with_refs(&mut u, NREF);
    u.relayout();
    let note = u.live_ref_note();
    for width in [140, 300, 480, 800] {
        let cut = u.fit_ref_note(&note, width);
        assert!(u.fonts().s.width(&cut) <= width || cut.ends_with("+10"), "{cut:?} in {width}");
        if cut != note { assert!(cut.contains('+'), "{cut:?}"); }
    }
    assert_eq!(u.fit_ref_note(&note, 5000), note);
    // the note and the Fit button sit above the axis labels, not on them
    let fit = u.live_ref_fit_rect().unwrap();
    assert!(fit.bottom() <= u.grid.bottom() - 20, "{fit:?} over the axis labels of {:?}", u.grid);
}

#[test]
fn eye_tab_in_a_side_pane_clears_the_charts_and_cursor_tags_clear_the_corner_buttons() {
    let hit = |a: Rect, b: Rect| a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom();
    for place in [workspace::Action::EyeRight, workspace::Action::EyeLeft, workspace::Action::EyeBelow] {
        let mut u = fresh();
        u.m.view = AView::Eye;
        u.relayout();
        u.workspace_action(place);
        let page = u.eye_page_rect();
        let l = eyepage::layout(page, false);
        if l.charts[0].w == 0 { continue; }
        let tab = u.eye_tab();
        for (k, c) in l.charts.iter().enumerate() {
            if page.w < 700 { assert!(!hit(tab, *c), "tab {tab:?} over chart {k} {c:?}"); }
        }
    }
    let u = fresh();
    for y in (u.area.y..u.area.bottom()).step_by(3) {
        let x = u.right_tag_x(u.area, y, 24);
        let tag = Rect::new(x, y, 24, 14);
        assert!(!hit(tag, u.full_btn().0) && !hit(tag, u.zoom_btn().0), "tag at y {y} covers a button");
    }
}
