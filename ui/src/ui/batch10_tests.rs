//! Batch 10: Studio docking. Every view has a title strip with its name and a close X; dragging a strip (a drag or a
//! long press) docks the view to an edge of another (50/50), swaps it with the centre of another, or floats it; the
//! dividers move; the arrangement is kept in the setup and Reset layout brings the default back.
//! OOS_SHOTS=<dir> writes pictures (b10-*).
use super::dock::{Dir, Drop, Node, Side, VKind, DIV_HIT, STRIP_H};
use super::sheet::PanelKind;
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    // the first frame applies the entry rules of the layout
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..4 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b10-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

/// A finger down at `a`, moved in steps to `b` and lifted there.
fn drag(u: &mut Ui, a: (i32, i32), b: (i32, i32)) {
    touch(u, Phase::Down, a.0, a.1);
    for k in 1..=8 { touch(u, Phase::Move, a.0 + (b.0 - a.0) * k / 8, a.1 + (b.1 - a.1) * k / 8); }
    touch(u, Phase::Up, b.0, b.1);
}

fn leaf(u: &Ui, k: VKind) -> Rect { u.dock_leaf(k).unwrap_or_else(|| panic!("{k:?} is docked")) }

/// A point on a view's strip clear of its buttons.
fn grip(u: &Ui, k: VKind) -> (i32, i32) {
    let r = u.dock_rect(k).unwrap_or_else(|| panic!("{k:?} is on the screen"));
    (r.x + 24, r.y + STRIP_H / 2)
}

fn drag_view(u: &mut Ui, k: VKind, to: (i32, i32)) {
    let g = grip(u, k);
    drag(u, g, to);
}

fn edge(r: Rect, s: Side) -> (i32, i32) {
    match s {
        Side::Left => (r.x + 20, r.y + r.h / 2),
        Side::Right => (r.right() - 20, r.y + r.h / 2),
        Side::Top => (r.x + r.w / 2, r.y + 14),
        Side::Bottom => (r.x + r.w / 2, r.bottom() - 14),
    }
}

fn docked(u: &mut Ui) {
    u.studio_set_view(StudioView::Docked);
    u.step(0.05);
}

fn overlaps(a: Rect, b: Rect) -> bool { a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom() }

// ------------------------------------------------------------------------------------------------ item 1

#[test]
fn every_view_has_a_title_strip_with_its_name_and_a_close_x() {
    let mut u = studio();
    docked(&mut u);
    u.studio_apply_panel(PanelKind::Spectrum);
    u.step(0.05);
    for k in [VKind::Wave, VKind::Measure, VKind::Pane] {
        let r = u.dock_rect(k).unwrap_or_else(|| panic!("{k:?} on the screen"));
        let s = Ui::dock_strip(r);
        assert!(s.h >= 24 && s.h <= 26, "{k:?} strip is {} px", s.h);
        let c = Ui::dock_close_rect(r);
        assert!(c.w >= 40 && c.h >= 40, "{k:?} close X {c:?}");
        assert!(!u.dock_name(k).is_empty());
        let content = u.dock_content(k).unwrap();
        assert_eq!(content.y, r.y + STRIP_H, "the view draws under its strip");
    }
    assert_eq!(u.dock_name(VKind::Measure), "Measurements");
    assert_eq!(u.dock_name(VKind::Pane), "Spectrum");
    // the X of a strip closes the view
    let c = Ui::dock_close_rect(leaf(&u, VKind::Measure));
    touch(&mut u, Phase::Down, c.x + c.w / 2, c.y + c.h / 2);
    touch(&mut u, Phase::Up, c.x + c.w / 2, c.y + c.h / 2);
    assert_eq!(u.studio.view, StudioView::Waveform);
    let c = Ui::dock_close_rect(leaf(&u, VKind::Pane));
    touch(&mut u, Phase::Down, c.x + 20, c.y + 20);
    touch(&mut u, Phase::Up, c.x + 20, c.y + 20);
    assert_eq!(u.m.pane, 0);
    // the eye (it and the second pane do not show together)
    u.studio_apply_panel(PanelKind::Eye);
    u.step(0.05);
    let r = leaf(&u, VKind::Eye);
    assert!(Ui::dock_strip(r).h >= 24 && Ui::dock_close_rect(r).w >= 40 && u.dock_name(VKind::Eye) == "Eye diagram");
    let c = Ui::dock_close_rect(r);
    touch(&mut u, Phase::Down, c.x + 20, c.y + 20);
    touch(&mut u, Phase::Up, c.x + 20, c.y + 20);
    assert!(!u.eye_open());
    // the waveform stays; only an analysis view on it closes
    let c = Ui::dock_close_rect(leaf(&u, VKind::Wave));
    touch(&mut u, Phase::Down, c.x + 20, c.y + 20);
    touch(&mut u, Phase::Up, c.x + 20, c.y + 20);
    assert!(u.dock_leaf(VKind::Wave).is_some());
}

#[test]
fn the_decode_table_and_the_sim_panel_have_strips_too() {
    let mut u = studio();
    u.table = Some(tables::TableKind::Search);
    u.step(0.05);
    assert!(u.dock_content(VKind::Table).is_some() && u.dock_name(VKind::Table) == "Search events");
    assert!(Ui::dock_strip(leaf(&u, VKind::Table)).h >= 24);
    u.table = None;
    u.studio_set_view(StudioView::Sim);
    u.step(0.05);
    assert_eq!(u.dock_name(VKind::Sim), "SIM");
    assert!(Ui::dock_strip(leaf(&u, VKind::Sim)).h >= 24);
    // the waveform's plot sits inside its leaf, under the strip
    let (w, g) = (leaf(&u, VKind::Wave), u.grid);
    assert!(g.y >= w.y + STRIP_H && g.bottom() <= w.bottom() && g.x >= w.x && g.right() <= w.right(), "{g:?} in {w:?}");
}

// ------------------------------------------------------------------------------------------------ item 2

#[test]
fn dragging_a_strip_to_each_edge_of_another_view_splits_it_in_half() {
    for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
        let mut u = studio();
        docked(&mut u);
        // start from the other side so every edge is a real move
        let w0 = leaf(&u, VKind::Wave);
        drag_view(&mut u, VKind::Measure, edge(w0, side));
        assert_eq!(u.studio.view, StudioView::Docked, "{side:?}: still docked");
        let (m, w) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
        let region = u.dock_region();
        match side {
            Side::Left => assert!(m.right() <= w.x && m.y == w.y && m.h == w.h && (m.w - w.w).abs() <= 2, "{m:?} {w:?}"),
            Side::Right => assert!(w.right() <= m.x && m.y == w.y && m.h == w.h && (m.w - w.w).abs() <= 2, "{m:?} {w:?}"),
            Side::Top => assert!(m.bottom() <= w.y && m.x == w.x && m.w == w.w && (m.h - w.h).abs() <= 2, "{m:?} {w:?}"),
            Side::Bottom => assert!(w.bottom() <= m.y && m.x == w.x && m.w == w.w && (m.h - w.h).abs() <= 2, "{m:?} {w:?}"),
        }
        // together they fill the body
        assert_eq!((m.w * m.h + w.w * w.h) / 1000, (region.w * region.h) / 1000 - ((region.w * region.h) - (m.w * m.h + w.w * w.h)) / 1000, "no stray rooms");
        assert!(m.x >= region.x && w.right() <= region.right() && m.bottom() <= region.bottom() && w.bottom() <= region.bottom());
        // the lanes still draw inside the waveform
        assert!(u.grid.y >= w.y + STRIP_H && u.grid.bottom() <= w.bottom() && u.grid.x >= w.x && u.grid.right() <= w.right(), "{side:?}: {:?} in {w:?}", u.grid);
    }
}

#[test]
fn the_drop_preview_shows_the_half_it_would_take_and_the_centre_swaps() {
    let mut u = studio();
    docked(&mut u);
    let w = leaf(&u, VKind::Wave);
    let (drop, prev) = u.dock_drop_at(VKind::Measure, edge(w, Side::Left).0, edge(w, Side::Left).1, (90, 20));
    assert_eq!(drop, Drop::Split(VKind::Wave, Side::Left));
    assert_eq!((prev.x, prev.y, prev.h), (w.x, w.y, w.h));
    assert!((prev.w - w.w / 2).abs() <= GAP_SLACK, "half: {prev:?} of {w:?}");
    let c = (w.x + w.w / 2, w.y + w.h / 2);
    let (drop, prev) = u.dock_drop_at(VKind::Measure, c.0, c.1, (90, 20));
    assert_eq!((drop, prev), (Drop::Swap(VKind::Wave), w));
    // a long drag with the preview up, as a picture
    let m = grip(&u, VKind::Measure);
    touch(&mut u, Phase::Down, m.0, m.1);
    for k in 1..=6 { touch(&mut u, Phase::Move, m.0 + (edge(w, Side::Left).0 - m.0) * k / 6, m.1 + (edge(w, Side::Left).1 - m.1) * k / 6); }
    assert!(matches!(u.studio.dock.drag, Some(super::dock::DockDrag::Move { .. })), "the strip is picked up");
    shot(&mut u, "drop-preview");
    touch(&mut u, Phase::Up, edge(w, Side::Left).0, edge(w, Side::Left).1);
    assert!(leaf(&u, VKind::Measure).x < leaf(&u, VKind::Wave).x);
}

const GAP_SLACK: i32 = 6;

#[test]
fn the_centre_of_a_view_swaps_two_views() {
    let mut u = studio();
    docked(&mut u);
    let (m0, w0) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    assert!(w0.x < m0.x);
    drag_view(&mut u, VKind::Measure, (w0.x + w0.w / 2, w0.y + w0.h / 2));
    let (m, w) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    assert!(m.x < w.x && m.bottom() == w.bottom(), "the two traded places: {m:?} {w:?}");
    assert!((m.w - m0.w).abs() <= 2 && (w.w - w0.w).abs() <= 2, "each kept its size: {m:?} {w:?} from {m0:?} {w0:?}");
    assert_eq!(u.studio.view, StudioView::Docked);
}

#[test]
fn a_long_press_picks_a_strip_up_without_moving_far_and_a_short_tap_does_not() {
    let mut u = studio();
    docked(&mut u);
    let g = grip(&u, VKind::Measure);
    touch(&mut u, Phase::Down, g.0, g.1);
    assert!(u.studio.dock.drag.is_none());
    u.step(LONG_PRESS + 0.05);
    assert!(matches!(u.studio.dock.drag, Some(super::dock::DockDrag::Move { .. })), "held for {LONG_PRESS} s");
    let w = leaf(&u, VKind::Wave);
    let e = edge(w, Side::Bottom);
    touch(&mut u, Phase::Move, e.0, e.1);
    touch(&mut u, Phase::Up, e.0, e.1);
    assert!(leaf(&u, VKind::Measure).y > leaf(&u, VKind::Wave).y, "dropped below the waveform");
    // a quick press and lift changes nothing
    let before = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    let g = grip(&u, VKind::Measure);
    touch(&mut u, Phase::Down, g.0, g.1);
    touch(&mut u, Phase::Up, g.0, g.1);
    assert_eq!(before, (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave)));
    // a small wobble is not a drag either (a second tap right after the first would be a double tap: batch 35)
    u.step(0.6);
    touch(&mut u, Phase::Down, g.0, g.1);
    touch(&mut u, Phase::Move, g.0 + 5, g.1 + 3);
    touch(&mut u, Phase::Up, g.0 + 5, g.1 + 3);
    assert_eq!(before, (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave)));
}

// ------------------------------------------------------------------------------------------------ item 3

#[test]
fn dropping_away_from_every_edge_floats_the_view_and_a_drag_to_an_edge_docks_it_again() {
    let mut u = studio();
    docked(&mut u);
    let w = leaf(&u, VKind::Wave);
    // in the ring between the edge zones and the centre of the waveform
    let p = (w.x + w.w * 31 / 100, w.y + w.h / 2);
    drag_view(&mut u, VKind::Measure, p);
    assert_eq!(u.studio.view, StudioView::Floating);
    let f = u.dock_float_rect(VKind::Measure).expect("a floating panel");
    assert!(f.w >= 220 && f.h >= 170);
    let region = u.dock_region();
    assert!(f.x >= region.x && f.right() <= region.right() && f.y >= region.y && f.bottom() <= region.bottom(), "{f:?} in {region:?}");
    // the waveform has the whole body now
    assert_eq!(leaf(&u, VKind::Wave), region);
    shot(&mut u, "floating-panel");
    // its strip moves it
    let g = grip(&u, VKind::Measure);
    let c = (w.x + w.w / 2, w.y + w.h / 2 - 100);
    drag(&mut u, g, c);
    let f2 = u.dock_float_rect(VKind::Measure).unwrap();
    assert!((f2.x - (c.0 - 24)).abs() <= 1 && f2.y != f.y, "{f:?} -> {f2:?}");
    assert_eq!(u.studio.view, StudioView::Floating);
    // to an edge of the waveform: docked again, on the right
    let w = leaf(&u, VKind::Wave);
    let g = grip(&u, VKind::Measure);
    drag(&mut u, g, edge(w, Side::Right));
    assert_eq!(u.studio.view, StudioView::Docked);
    assert!(u.dock_float_rect(VKind::Measure).is_none());
    let (m, w) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    assert!(w.right() <= m.x && (m.w - w.w).abs() <= GAP_SLACK);
}

#[test]
fn floating_views_never_cover_the_bars_or_the_zoom_bar_and_can_be_resized() {
    let mut u = studio();
    u.studio_apply_panel(PanelKind::Eye);
    u.step(0.05);
    u.set_zoom(true);
    u.step(0.05);
    let w = leaf(&u, VKind::Wave);
    // float the eye and push it at the top bar, the bottom bar and the zoom bar
    drag_view(&mut u, VKind::Eye, (w.x + w.w * 31 / 100, w.y + w.h / 2));
    assert!(u.dock_floating(VKind::Eye), "the eye floats");
    for target in [(300, 0), (300, 700), (700, 330), (10, 10), (1020, 590)] {
        let g = grip(&u, VKind::Eye);
        drag(&mut u, g, target);
        let f = u.dock_float_rect(VKind::Eye).unwrap();
        let fr = u.frame();
        assert!(f.y >= fr.top.bottom() && f.bottom() <= fr.bottom.y, "{target:?}: {f:?} between the bars");
        let bar = u.zoom_readout();
        assert!(!overlaps(f, bar), "{target:?}: {f:?} clear of the zoom bar {bar:?}");
        assert!(!overlaps(f, fr.top) && !overlaps(f, fr.bottom));
    }
    // the corner resizes, down to a minimum
    let f = u.dock_float_rect(VKind::Eye).unwrap();
    let corner = (f.right() - 10, f.bottom() - 10);
    drag(&mut u, corner, (corner.0 - 400, corner.1 - 400));
    let g = u.dock_float_rect(VKind::Eye).unwrap();
    assert_eq!((g.w, g.h), (220, 170), "the least size");
    let corner = (g.right() - 10, g.bottom() - 10);
    drag(&mut u, corner, (corner.0 + 60, corner.1 + 30));
    let h = u.dock_float_rect(VKind::Eye).unwrap();
    assert!(h.w > g.w && h.h > g.h);
}

// ------------------------------------------------------------------------------------------------ item 4

#[test]
fn dividers_drag_with_a_wide_hit_area_and_a_minimum_view_size() {
    let mut u = studio();
    docked(&mut u);
    let lay = u.dock_layout();
    let d = lay.dividers.first().expect("a divider between the waveform and the measurements").clone();
    assert_eq!(d.dir, Dir::H);
    assert!(d.hit.w >= 40 && d.hit.w >= DIV_HIT && d.hit.h >= 300, "{:?}", d.hit);
    let m0 = leaf(&u, VKind::Measure);
    // a finger 15 px beside the line still has it
    let (x, y) = (d.hit.x + 4, d.hit.y + d.hit.h / 2);
    drag(&mut u, (x, y), (x - 120, y));
    let m1 = leaf(&u, VKind::Measure);
    assert!(m1.w > m0.w + 100, "{} -> {}", m0.w, m1.w);
    assert!(u.studio.dock.tree.is_some());
    // as far as it goes: both views keep their least size
    let d = u.dock_layout().dividers[0].clone();
    drag(&mut u, (d.hit.x + 20, y), (5, y));
    let (m, w) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    assert!(w.w >= 300, "the waveform keeps {} px", w.w);
    assert!(m.w >= 150);
    let d = u.dock_layout().dividers[0].clone();
    drag(&mut u, (d.hit.x + 20, y), (1020, y));
    let (m, w) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    assert!(m.w >= 150 && w.w >= 300, "{} {}", m.w, w.w);
    // a stacked split: the divider is horizontal and moves up and down
    let e = leaf(&u, VKind::Wave);
    drag_view(&mut u, VKind::Measure, edge(e, Side::Bottom));
    let d = u.dock_layout().dividers.iter().find(|d| d.dir == Dir::V).expect("a stacked split").clone();
    assert!(d.hit.h >= 40 && d.hit.w >= 300);
    let (m0, w0) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    drag(&mut u, (d.hit.x + d.hit.w / 2, d.hit.y + d.hit.h / 2), (d.hit.x + d.hit.w / 2, d.hit.y + d.hit.h / 2 - 40));
    let (m1, w1) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Wave));
    assert!(w1.h < w0.h && m1.h > m0.h, "{w0:?}->{w1:?} {m0:?}->{m1:?}");
    assert!(w1.h >= 210);
}

#[test]
fn a_tiled_two_by_two_layout() {
    let mut u = studio();
    docked(&mut u);
    u.studio_apply_panel(PanelKind::Spectrum);
    u.table = Some(tables::TableKind::Search);
    u.step(0.05);
    for k in [VKind::Wave, VKind::Measure, VKind::Pane, VKind::Table] { assert!(u.dock_leaf(k).is_some(), "{k:?} is docked"); }
    // the waveform and the search events side by side on top, the spectrum and the measurements under them
    let w = leaf(&u, VKind::Wave);
    drag_view(&mut u, VKind::Table, edge(w, Side::Right));
    let p = leaf(&u, VKind::Pane);
    drag_view(&mut u, VKind::Measure, edge(p, Side::Right));
    let (w, t, p, m) = (leaf(&u, VKind::Wave), leaf(&u, VKind::Table), leaf(&u, VKind::Pane), leaf(&u, VKind::Measure));
    assert!(w.right() <= t.x && p.right() <= m.x, "two columns: {w:?} {t:?} / {p:?} {m:?}");
    assert!(w.bottom() <= p.y && t.bottom() <= m.y, "two rows");
    let rects = [w, t, p, m];
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] { assert!(!overlaps(*a, *b), "{a:?} {b:?}"); }
    }
    let region = u.dock_region();
    for r in &rects { assert!(r.x >= region.x && r.right() <= region.right() && r.y >= region.y && r.bottom() <= region.bottom()); }
    shot(&mut u, "tiled");

}

#[test]
fn the_arrangement_is_kept_in_the_setup_and_old_setups_load_the_default() {
    let mut u = studio();
    docked(&mut u);
    let w = leaf(&u, VKind::Wave);
    drag_view(&mut u, VKind::Measure, edge(w, Side::Bottom));
    u.studio_apply_panel(PanelKind::Eye);
    u.step(0.05);
    let w = leaf(&u, VKind::Wave);
    drag_view(&mut u, VKind::Eye, (w.x + w.w * 31 / 100, w.y + w.h / 2));
    assert!(u.dock_floating(VKind::Eye));
    let d = u.dock_layout().dividers[0].clone();
    drag(&mut u, (d.hit.x + d.hit.w / 2, d.hit.y + d.hit.h / 2), (d.hit.x + d.hit.w / 2, d.hit.y + d.hit.h / 2 + 30));
    for _ in 0..3 { u.step(0.05); }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("studio_dock = "), "kept: {}", text.lines().filter(|l| l.starts_with("studio")).collect::<Vec<_>>().join(" | "));
    // a new session reads it back
    let m = crate::store::parse_setup(&text).expect("the setup reads back");
    assert_eq!(m.studio_dock, u.m.studio_dock);
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m = m;
    v.restore_view_state();
    for _ in 0..10 { v.step(0.05); }
    assert_eq!(v.studio.dock.tree, u.studio.dock.tree, "the same tree");
    assert_eq!(v.studio.view, u.studio.view);
    assert_eq!(v.dock_leaf(VKind::Measure), u.dock_leaf(VKind::Measure));
    assert_eq!(v.dock_leaf(VKind::Wave), u.dock_leaf(VKind::Wave));
    assert_eq!(v.studio.dock.float_rect[VKind::Eye.idx()], u.studio.dock.float_rect[VKind::Eye.idx()]);
    assert_eq!(v.studio.dock.float_on[VKind::Eye.idx()], true);
    // the text itself is a small tree
    assert_eq!(Node::parse(&Node::text(u.studio.dock.tree.as_ref().unwrap())).as_ref(), u.studio.dock.tree.as_ref());
    // an old setup has no such line: today's default arrangement
    let old: String = text.lines().filter(|l| !l.starts_with("studio_dock")).map(|l| format!("{l}\n")).collect();
    let m = crate::store::parse_setup(&old).expect("an old setup loads");
    assert!(m.studio_dock.is_empty());
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m = m;
    v.restore_view_state();
    for _ in 0..10 { v.step(0.05); }
    assert!(v.studio.dock.tree.is_none() && v.studio.dock.float_rect.iter().all(Option::is_none));
    assert_eq!(leaf(&v, VKind::Measure).w, v.studio_panel_w(), "the measurements have their default width, to the right");
    assert!(leaf(&v, VKind::Wave).x < leaf(&v, VKind::Measure).x && !v.dock_floating(VKind::Eye), "the default arrangement");
    // broken text is the default too
    for bad in ["H0.5(W", "H0.99(W,M)", "X", "H0.5(W,W)", "W;E@1:2:3", "H0.5(W,M);M@a:b:c:d"] {
        let mut v = studio();
        v.dock_restore(bad);
        assert!(v.studio.dock.tree.as_ref().is_none_or(|t| { let mut l = Vec::new(); t.leaves(&mut l); !l.is_empty() }), "{bad}");
        for _ in 0..3 { v.step(0.05); }
        assert!(v.dock_leaf(VKind::Wave).is_some());
    }
}

// ------------------------------------------------------------------------------------------------ item 4: the old choices

#[test]
fn the_measurement_choices_of_the_view_menu_still_work_and_map_onto_the_tree() {
    let mut u = studio();
    // Waveform focus: the waveform has the body
    assert_eq!(leaf(&u, VKind::Wave), u.dock_region());
    // Docked: a leaf beside the waveform
    docked(&mut u);
    assert!(u.dock_leaf(VKind::Measure).is_some() && u.dock_float_rect(VKind::Measure).is_none());
    // Floating: a floating panel
    u.studio_set_view(StudioView::Floating);
    u.step(0.05);
    assert!(u.dock_leaf(VKind::Measure).is_none() && u.dock_float_rect(VKind::Measure).is_some());
    assert_eq!(leaf(&u, VKind::Wave), u.dock_region());
    // Only: the measurements alone fill the body (no waveform leaf; the strip has no move and no maximize)
    u.studio_set_view(StudioView::Only);
    u.step(0.05);
    assert_eq!(leaf(&u, VKind::Measure), u.dock_region());
    assert_eq!(u.studio_table_rect(), u.dock_content(VKind::Measure));
    assert!(u.dock_leaf(VKind::Wave).is_none());
    // back to the arrangement
    docked(&mut u);
    assert!(u.dock_leaf(VKind::Wave).is_some() && u.dock_leaf(VKind::Measure).is_some());
    // the maximize button of the strip is Only
    let r = leaf(&u, VKind::Measure);
    let b = Rect::new(r.right() - 96, r.y, 48, STRIP_H);
    touch(&mut u, Phase::Down, b.x + 24, b.y + 20);
    touch(&mut u, Phase::Up, b.x + 24, b.y + 20);
    assert_eq!(u.studio.view, StudioView::Only);
}

// ------------------------------------------------------------------------------------------------ item 5

#[test]
fn reset_layout_in_the_view_menu_brings_the_default_back() {
    let mut u = studio();
    docked(&mut u);
    let default = leaf(&u, VKind::Wave);
    let w = leaf(&u, VKind::Wave);
    drag_view(&mut u, VKind::Measure, edge(w, Side::Left));
    u.studio_apply_panel(PanelKind::Eye);
    let w = leaf(&u, VKind::Wave);
    drag_view(&mut u, VKind::Eye, (w.x + w.w * 31 / 100, w.y + w.h / 2));
    assert!(u.studio.dock.tree.is_some() && u.dock_floating(VKind::Eye));
    assert_ne!(leaf(&u, VKind::Wave), default);
    // the View chip, then the Reset layout row, as the user taps them
    let chip = u.studio_top().view;
    u.tap(chip.x + chip.w / 2, chip.y + chip.h / 2);
    assert!(u.studio.view_menu);
    let row = u.studio_view_row(super::studio::VIEW_RESET);
    assert!(row.h >= 40);
    u.tap(row.x + row.w / 2, row.y + row.h / 2);
    for _ in 0..3 { u.step(0.05); }
    assert!(!u.studio.view_menu);
    assert!(u.studio.dock.tree.is_none() && !u.dock_floating(VKind::Eye) && u.studio.dock.float_rect.iter().all(Option::is_none));
    assert!(u.m.studio_dock.is_empty(), "the setup forgets it too: {:?}", u.m.studio_dock);
    assert_eq!(u.studio.view, StudioView::Waveform);
    // docked again after the reset: the default places
    docked(&mut u);
    assert!(leaf(&u, VKind::Wave).x < leaf(&u, VKind::Measure).x);
}

#[test]
fn only_studio_has_the_dock() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    for l in [Layout::Default, Layout::Minimal] {
        u.m.layout = l;
        u.step(0.05);
        assert!(!u.dock_on());
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        assert!(u.studio.dock.tree.is_none() && u.m.studio_dock.is_empty());
    }
}
