// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 11: the last gaps of the Studio hands-on audit. The zoom keeps its factor and its place in the overview
//! when the main time base changes; each measurement's card has a Stats page; a press on a title strip moves the view
//! even where a divider's wide hit area reaches into the strip.
use super::dock::{Side, VKind, STRIP_H};
use super::measrow::RF;
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, Phase::Up, b.0, b.1);
}

#[test]
fn the_zoom_keeps_its_factor_and_place_when_the_time_base_changes() {
    let mut u = studio();
    u.set_zoom(true);
    let factor0 = u.studio_time_zoom();
    // put the window a quarter of the screen right of the centre
    let place = 0.25;
    u.m.zdelay = u.m.delay + place * u.m.time_per_div() * HDIV;
    for d in [-1, -1, 1, 1, 1] {
        u.adjust(Item::HScale, d);
        let f = u.studio_time_zoom();
        assert!((f / factor0 - 1.0).abs() < 0.3, "factor {factor0} -> {f}");
        let p = (u.m.zdelay - u.m.delay) / (u.m.time_per_div() * HDIV);
        assert!((p - place).abs() < 1e-3, "place {place} -> {p}");
    }
}

#[test]
fn a_measurement_card_has_a_stats_page_with_mean_range_population_and_trend() {
    let mut u = studio();
    for _ in 0..20 { u.step(0.05); }
    assert!(!u.m.meas.is_empty());
    u.open_meas_cfg(0);
    let plan = u.measrow_plan();
    let stats = plan.iter().find(|(name, _)| *name == "Stats").expect("a Stats page");
    for f in [RF::StMean, RF::StRange, RF::StN, RF::Trend] { assert!(stats.1.contains(&Item::RowF(f)), "{f:?}"); }
    assert!(stats.1.contains(&Item::MeasPop) && stats.1.contains(&Item::MeasStatReset));
    assert!(stats.1.len() <= 7);
    let mean = u.measrow_text(Item::RowF(RF::StMean)).unwrap();
    let n = u.measrow_text(Item::RowF(RF::StN)).unwrap();
    assert!(mean.starts_with("Mean"), "{mean}");
    assert!(n.starts_with("Population"), "{n}");
    for f in [RF::StMean, RF::StRange, RF::StN] {
        let t = u.measrow_text(Item::RowF(f)).unwrap();
        assert!(u.fonts().s.width(&t) <= sheet::SHEET_W - 24, "{t}");
    }
    assert!(u.measrow_activate(Item::RowF(RF::Trend)));
    assert_eq!(u.m.pane, tables::PANE_TREND);
}

#[test]
fn opening_the_trend_keeps_the_statistics() {
    let mut u = studio();
    for _ in 0..20 { u.step(0.05); }
    let n0 = u.mstat.first().map_or(0, |s| s.n);
    assert!(n0 > 0);
    u.open_meas_cfg(0);
    assert!(u.measrow_activate(Item::RowF(RF::Trend)));
    u.adjust(Item::TrendSel(0), 1);
    assert!(u.mstat.first().map_or(0, |s| s.n) >= n0, "the population restarted");
}

#[test]
fn trend_time_labels_never_overlap() {
    let width = |s: &str| s.chars().count() as i32 * 7;
    for w in (40..900).step_by(7) {
        for window in [10.0, 15.0, 75.0, 150.0, 600.0, 3600.0, 7200.0] {
            let l = panes::trend_time_labels(window, w, width);
            assert!(!l.is_empty());
            assert_eq!(l.last().unwrap().1, "now");
            for p in l.windows(2) {
                if l.len() > 2 { assert!(p[0].0 + width(&p[0].1) + 4 <= p[1].0, "w {w} window {window}: {l:?}"); }
            }
        }
    }
}

#[test]
fn a_narrow_tall_trend_pane_stacks_the_charts_and_keeps_the_plot_wide() {
    // the size of a trend view docked at the right of the waveform
    let r = Rect::new(0, 0, 510, 480);
    let c = Ui::chart_rects(r, 2);
    assert!(c[0].w == r.w && c[0].bottom() <= c[1].y, "{c:?}");
    let (g, card) = Ui::chart_parts(c[0]);
    assert!(card.is_some() && g.w >= 220, "{g:?}");
    // narrower still: the values go on a line under the title and the plot keeps the width
    let (g, card) = Ui::chart_parts(Rect::new(0, 0, 300, 200));
    assert!(card.is_none() && g.w >= 200, "{g:?}");
    // a wide lower pane keeps them side by side
    let c = Ui::chart_rects(Rect::new(0, 0, 1200, 180), 2);
    assert_eq!(c[0].y, c[1].y);
}

#[test]
fn a_press_on_a_title_strip_under_a_divider_moves_the_view() {
    let mut u = studio();
    u.studio_set_view(StudioView::Docked);
    u.step(0.05);
    // stack the measurements under the waveform
    let w = u.dock_leaf(VKind::Wave).unwrap();
    let m = u.dock_rect(VKind::Measure).unwrap();
    drag(&mut u, (m.x + 24, m.y + STRIP_H / 2), (w.x + w.w / 2, w.bottom() - 14));
    let (w, m) = (u.dock_leaf(VKind::Wave).unwrap(), u.dock_leaf(VKind::Measure).unwrap());
    assert!(w.bottom() <= m.y, "stacked: {w:?} {m:?}");
    // a press in the upper part of the strip, inside the divider's hit area, still picks the view up
    let d = u.dock_layout().dividers.iter().find(|d| d.hit.contains(m.x + 60, m.y + 6)).cloned();
    assert!(d.is_some(), "the divider reaches into the strip");
    drag(&mut u, (m.x + 60, m.y + 6), (w.right() - 20, w.y + w.h / 2));
    let (w, m) = (u.dock_leaf(VKind::Wave).unwrap(), u.dock_leaf(VKind::Measure).unwrap());
    assert!(w.right() <= m.x && m.y == w.y, "docked to the right: {w:?} {m:?}");
    let _ = Side::Right;
}
