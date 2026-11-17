// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 11 (docking, part two): the histogram, XY and the analysers are views of their own that tile beside the
//! waveform; every view can float (the last docked one cannot); the measurements and the search events resize by their
//! dividers, move and float. OOS_SHOTS=<dir> writes pictures (b11-*).
use super::dock::{Dir, Drop, VKind, STRIP_H};
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
    u.step(0.05);
    u
}

fn frame(u: &mut Ui) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    u.step(0.05);
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..4 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b11-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
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

fn leaf(u: &Ui, k: VKind) -> Rect { u.dock_leaf(k).unwrap_or_else(|| panic!("{k:?} is docked")) }

fn grip(u: &Ui, k: VKind) -> (i32, i32) {
    let r = u.dock_rect(k).unwrap_or_else(|| panic!("{k:?} is on the screen"));
    (r.x + 24, r.y + STRIP_H / 2)
}

fn handle(u: &Ui, dir: Dir, nth: usize) -> (i32, i32) {
    let d = u.dock_layout().dividers.iter().filter(|d| d.dir == dir).nth(nth).cloned().expect("a divider");
    (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2)
}

fn float_rect_of(u: &Ui, k: VKind) -> Rect { u.dock_float_rect(k).unwrap_or_else(|| panic!("{k:?} floats")) }

// ------------------------------------------------------------------------------------------------ the new views

#[test]
fn histogram_xy_and_the_analysers_tile_beside_the_waveform() {
    let mut u = studio();
    u.m.hist = Hist::Vertical;
    frame(&mut u);
    let (w, h) = (leaf(&u, VKind::Wave), leaf(&u, VKind::Hist));
    assert!(w.right() <= h.x || h.right() <= w.x, "beside, not over: {w:?} {h:?}");
    assert!(!u.view_on(), "the waveform stays");
    u.m.view = AView::Spectrum;
    frame(&mut u);
    let (w, a) = (leaf(&u, VKind::Wave), leaf(&u, VKind::View));
    assert!(w.w > 0 && (w.right() <= a.x || a.right() <= w.x), "{w:?} {a:?}");
    assert_eq!(u.dock_name(VKind::View), "Spectrum");
    shot(&mut u, "tiled-hist-spectrum");
    u.m.view = AView::Scope;
    u.xy = true;
    frame(&mut u);
    assert!(u.dock_leaf(VKind::Xy).is_some() && u.dock_leaf(VKind::View).is_none() && u.dock_leaf(VKind::Wave).is_some());
    shot(&mut u, "tiled-hist-xy");
    // the view's own strip has its name and an X
    let x = leaf(&u, VKind::Xy);
    assert_eq!(u.dock_name(VKind::Xy), "XY");
    assert!(Ui::dock_close_rect(x).h >= 40);
}

#[test]
fn closing_a_view_with_its_x_turns_the_feature_off() {
    let mut u = studio();
    u.m.hist = Hist::Horizontal;
    u.m.view = AView::Bode;
    frame(&mut u);
    let r = leaf(&u, VKind::Hist);
    let c = Ui::dock_close_rect(r);
    drag(&mut u, (c.x + 24, c.y + 20), (c.x + 24, c.y + 20));
    assert_eq!(u.m.hist, Hist::Off);
    assert!(u.dock_leaf(VKind::Hist).is_none());
    let c = Ui::dock_close_rect(leaf(&u, VKind::View));
    drag(&mut u, (c.x + 24, c.y + 20), (c.x + 24, c.y + 20));
    assert_eq!(u.m.view, AView::Scope);
    u.xy = true;
    frame(&mut u);
    let c = Ui::dock_close_rect(leaf(&u, VKind::Xy));
    drag(&mut u, (c.x + 24, c.y + 20), (c.x + 24, c.y + 20));
    assert!(!u.xy && u.dock_leaf(VKind::Xy).is_none());
    assert!(u.dock_leaf(VKind::Wave).is_some());
}

#[test]
fn a_new_view_can_swap_and_float() {
    let mut u = studio();
    u.m.hist = Hist::Vertical;
    frame(&mut u);
    let (w0, h0) = (leaf(&u, VKind::Wave), leaf(&u, VKind::Hist));
    let t = grip(&u, VKind::Hist);
    let w = leaf(&u, VKind::Wave);
    drag(&mut u, t, (w.x + w.w / 2, w.y + w.h / 2));
    let (w1, h1) = (leaf(&u, VKind::Wave), leaf(&u, VKind::Hist));
    assert!(w1 != w0 && h1 != h0, "swapped");
    // floats where it is dropped away from the edges and the centre
    let w = leaf(&u, VKind::Wave);
    let g = grip(&u, VKind::Hist);
    drag(&mut u, g, (w.x + w.w * 31 / 100, w.y + w.h / 2));
    assert!(u.dock_floating(VKind::Hist) && u.dock_leaf(VKind::Hist).is_none());
}

// ------------------------------------------------------------------------------------------------ floating

#[test]
fn the_waveform_can_float_but_the_last_docked_view_cannot() {
    let mut u = studio();
    assert!(u.dock_can_float(VKind::Wave));
    let r = Rect::new(200, 120, 520, 330);
    u.dock_apply(VKind::Wave, Drop::Float(r));
    frame(&mut u);
    assert!(u.dock_floating(VKind::Wave) && u.dock_leaf(VKind::Wave).is_none());
    let f = float_rect_of(&u, VKind::Wave);
    assert!(f.w >= 300 && f.h >= 250, "{f:?}");
    // the plot is in the floating panel
    assert!(u.grid.x >= f.x && u.grid.right() <= f.right() + 1 && u.grid.y >= f.y, "{:?} in {f:?}", u.grid);
    // the measurements are the last docked view: they stay
    assert!(!u.dock_can_float(VKind::Measure));
    let before = u.studio.view;
    u.dock_apply(VKind::Measure, Drop::Float(Rect::new(300, 200, 300, 250)));
    assert_eq!(u.studio.view, before);
    assert!(u.dock_leaf(VKind::Measure).is_some());
    // dragging it out of its place is refused too
    let g = grip(&u, VKind::Measure);
    drag(&mut u, g, (400, 300));
    assert!(u.dock_leaf(VKind::Measure).is_some() && !u.dock_floating(VKind::Measure));
    // the waveform docks again
    let m = leaf(&u, VKind::Measure);
    let g = grip(&u, VKind::Wave);
    drag(&mut u, g, (m.x + 20, m.y + m.h / 2));
    assert!(u.dock_leaf(VKind::Wave).is_some() && !u.dock_floating(VKind::Wave));
    // alone, the waveform cannot float
    u.studio_set_view(StudioView::Waveform);
    frame(&mut u);
    assert!(!u.dock_can_float(VKind::Wave));
    u.dock_apply(VKind::Wave, Drop::Float(r));
    assert!(!u.dock_floating(VKind::Wave) && u.dock_leaf(VKind::Wave).is_some());
}

#[test]
fn offline_measurements_can_float() {
    let mut u = studio();
    u.studio_set_view(StudioView::Offline);
    frame(&mut u);
    if u.studio.view != StudioView::Offline { return; }
    assert!(u.dock_can_float(VKind::Measure));
    u.dock_apply(VKind::Measure, Drop::Float(Rect::new(300, 150, 320, 280)));
    assert!(u.dock_floating(VKind::Measure));
    u.studio_set_view(StudioView::Docked);
    assert!(!u.dock_floating(VKind::Measure));
}

// ------------------------------------------------------------------------------------------------ the measurements and the search panel

#[test]
fn the_measurements_resize_move_and_float() {
    let mut u = studio();
    u.table = Some(tables::TableKind::Search);
    frame(&mut u);
    let (m0, t0) = (leaf(&u, VKind::Measure), leaf(&u, VKind::Table));
    // a divider with a visible handle between the waveform and the column
    let d = u.dock_layout().dividers.iter().find(|d| d.dir == Dir::H).cloned().unwrap();
    assert!(d.hit.w >= 44);
    shot(&mut u, "handles");
    let p = handle(&u, Dir::H, 0);
    drag(&mut u, p, (p.0 - 100, p.1));
    let m1 = leaf(&u, VKind::Measure);
    assert!(m1.w > m0.w + 60, "{} -> {}", m0.w, m1.w);
    // small: it keeps a usable size
    let p = handle(&u, Dir::H, 0);
    drag(&mut u, p, (1020, p.1));
    let m2 = leaf(&u, VKind::Measure);
    assert!(m2.w >= 150 && m2.w < m1.w, "{}", m2.w);
    // the search panel and the measurements trade height by their own divider
    // the search panel under the measurements, in one column
    let m = leaf(&u, VKind::Measure);
    let g = grip(&u, VKind::Table);
    drag(&mut u, g, (m.x + m.w / 2, m.bottom() - 20));
    let (t, m) = (leaf(&u, VKind::Table), leaf(&u, VKind::Measure));
    assert!(m.bottom() <= t.y && t.x == m.x, "{m:?} {t:?}");
    let p = handle(&u, Dir::V, 0);
    let _ = t0;
    let (t_a, m_a) = (leaf(&u, VKind::Table), leaf(&u, VKind::Measure));
    drag(&mut u, p, (p.0, p.1 - 60));
    let (t_b, m_b) = (leaf(&u, VKind::Table), leaf(&u, VKind::Measure));
    assert!(m_b.h < m_a.h && t_b.h > t_a.h, "{t_a:?}->{t_b:?} {m_a:?}->{m_b:?}");
    let p = handle(&u, Dir::V, 0);
    drag(&mut u, p, (p.0, p.1 + 40));
    let (t_c, m_c) = (leaf(&u, VKind::Table), leaf(&u, VKind::Measure));
    assert!(m_c.h > m_b.h && t_c.h < t_b.h, "the search panel resizes both ways");
    assert!(m_c.h >= STRIP_H + 100);
    // it moves: under the waveform
    let w = leaf(&u, VKind::Wave);
    let g = grip(&u, VKind::Measure);
    drag(&mut u, g, (w.x + w.w / 2, w.bottom() - 14));
    let (w, m) = (leaf(&u, VKind::Wave), leaf(&u, VKind::Measure));
    assert!(w.bottom() <= m.y, "{w:?} {m:?}");
    // it floats and resizes by its corner and its edges
    let w = leaf(&u, VKind::Wave);
    let g = grip(&u, VKind::Measure);
    drag(&mut u, g, (w.x + w.w * 31 / 100, w.y + w.h / 2));
    assert_eq!(u.studio.view, StudioView::Floating);
    let f = float_rect_of(&u, VKind::Measure);
    drag(&mut u, (f.right() - 10, f.bottom() - 10), (f.right() + 50, f.bottom() + 40));
    let g = float_rect_of(&u, VKind::Measure);
    assert!(g.w > f.w && g.h > f.h, "corner: {f:?} {g:?}");
    drag(&mut u, (g.right() - 5, g.y + g.h / 2), (g.right() - 65, g.y + g.h / 2));
    let h = float_rect_of(&u, VKind::Measure);
    assert!(h.w < g.w && h.x == g.x, "right edge: {g:?} {h:?}");
    drag(&mut u, (h.x + 5, h.y + h.h / 2), (h.x - 40, h.y + h.h / 2));
    let i = float_rect_of(&u, VKind::Measure);
    assert!(i.w > h.w && i.right() == h.right(), "left edge: {h:?} {i:?}");
    drag(&mut u, (i.x + i.w / 2, i.bottom() - 5), (i.x + i.w / 2, i.bottom() - 60));
    let j = float_rect_of(&u, VKind::Measure);
    assert!(j.h < i.h, "bottom edge: {i:?} {j:?}");
    shot(&mut u, "floating-measure");
}

// ------------------------------------------------------------------------------------------------ old setups, other layouts

#[test]
fn old_layout_strings_still_load() {
    let mut u = studio();
    u.dock_restore("H0.600(W,M)");
    assert!(u.studio.dock.tree.is_some());
    u.dock_restore("H0.600(W,M);E@10:20:300:250;E!");
    assert!(u.studio.dock.tree.is_some() && u.studio.dock.float_on[VKind::Eye.idx()]);
    // the new kinds read back
    u.dock_restore("H0.500(W,V0.500(I,A))");
    assert_eq!(u.studio.dock.tree.as_ref().map(|t| t.contains(VKind::Hist) && t.contains(VKind::View)), Some(true));
    // an unknown kind: the default arrangement
    u.dock_restore("H0.600(W,Z)");
    assert!(u.studio.dock.tree.is_none());
    u.dock_restore("garbage;Q!");
    assert!(u.studio.dock.tree.is_none());
    frame(&mut u);
    assert!(u.dock_leaf(VKind::Wave).is_some() && u.dock_leaf(VKind::Measure).is_some());
    // the text of a tree with the new views round-trips
    u.m.hist = Hist::Vertical;
    frame(&mut u);
    let t = u.dock_text();
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m.layout = Layout::Studio;
    v.dock_restore(&t);
    assert_eq!(v.studio.dock.tree, u.studio.dock.tree);
}

#[test]
fn default_and_minimal_layouts_keep_their_overlays() {
    for layout in [Layout::Default, Layout::Minimal] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = layout;
        for _ in 0..10 { u.step(0.05); }
        u.m.hist = Hist::Vertical;
        frame(&mut u);
        assert!(!u.dock_on() && !u.view_on(), "{layout:?}: the histogram is an overlay of the waveform");
        u.xy = true;
        assert!(u.view_on() && u.xy_over() && u.view_tile_rect().is_none(), "{layout:?}: XY fills the plot");
        frame(&mut u);
        u.xy = false;
        u.m.view = AView::Spectrum;
        assert!(u.view_on(), "{layout:?}: an analyser fills the plot");
        frame(&mut u);
    }
}
