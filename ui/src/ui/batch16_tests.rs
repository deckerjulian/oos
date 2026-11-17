// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 16: a figure of its own for every catalogue measurement, short names on the trace and in the badges, and
//! labels that do not collide. OOS_SHOTS=<dir> writes pictures (b16-*).
use super::meascatalog::{cat_from, Cat};
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b16-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn all_cats() -> Vec<Cat> { (0..512u16).filter_map(cat_from).collect() }

#[test]
fn every_measurement_has_a_figure_of_its_own_that_stays_inside_its_rectangle() {
    let cats = all_cats();
    assert!(cats.len() >= 100, "the whole catalogue: {}", cats.len());
    let f = crate::font::Fonts::get(0);
    let r = Rect::new(40, 30, 72, 36);
    let mut seen: Vec<(u64, Cat)> = Vec::new();
    for c in cats {
        let mut cv = Canvas::new(160, 100);
        cv.fill(Rect::new(0, 0, 160, 100), 0x101010);
        super::caticons::draw_cat_icon(&mut cv, f, r, c, 0xffffff, 0xffa000);
        let mut h: u64 = 1469598103934665603;
        let mut ink = 0;
        for y in 0..100 {
            for x in 0..160 {
                let p = cv.px[(y * 160 + x) as usize];
                if p != 0x101010 {
                    assert!(r.contains(x, y), "{c:?} drew outside its rectangle at {x},{y}");
                    if r.contains(x, y) { ink += 1; }
                    h = (h ^ (p as u64 + (x * 7 + y * 13) as u64)).wrapping_mul(1099511628211);
                }
            }
        }
        assert!(ink > 25, "{c:?} has a figure: {ink} pixels");
        // the tiles upstream added from id 200 on (pair variants, levels) take the figure of their family
        if c.id >= 200 { continue; }
        if let Some((_, other)) = seen.iter().find(|(k, _)| *k == h) { panic!("{c:?} and {other:?} draw the same figure"); }
        seen.push((h, c));
    }
}

#[test]
fn the_tile_grid_shows_the_pictures_on_two_pages() {
    let mut u = studio();
    u.open_meas_list();
    for _ in 0..3 { frame(&mut u); }
    shot(&mut u, "tiles-page1");
    let g = u.sheet_add_geo();
    assert!(u.tile_scroll_max() > 0);
    u.studio.md.tile_scroll = g.list.h;
    for _ in 0..2 { frame(&mut u); }
    shot(&mut u, "tiles-page2");
    u.sheet.add_group = 1;
    shot(&mut u, "tiles-jitter");
}

#[test]
fn trace_labels_and_badges_use_short_names_and_the_labels_do_not_overlap() {
    use super::meascatalog::meas_short_name;
    assert_eq!(meas_short_name(Meas::Vpp), "Pk-pk");
    assert_eq!(meas_short_name(Meas::Vamp), "Ampl");
    assert_eq!(meas_short_name(Meas::PWidth), "+Width");
    assert_eq!(meas_short_name(Meas::Overshoot), "Ovshoot+");
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.m.meas = vec![(Meas::Vtop, WaveId::Ch(0)), (Meas::Vbase, WaveId::Ch(0)), (Meas::PWidth, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0)), (Meas::Vavg, WaveId::Ch(0))];
    u.m.meas_labels = true;
    u.ensure_meas_own();
    for _ in 0..8 { frame(&mut u); }
    let rects = u.studio.label_rects.borrow().clone();
    assert!(rects.len() >= 4, "labels were placed: {}", rects.len());
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            let over = a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom();
            assert!(!over, "two labels cover each other: {a:?} {b:?}");
        }
    }
    // the badges of the bottom row are short
    let badges = u.studio_badges();
    assert!(!badges.is_empty());
    let (key, _) = badges[1];
    assert_eq!(u.studio_badge_name(key, "Long Name"), meas_short_name(Meas::Vbase));
    shot(&mut u, "labels-square");
    shot(&mut u, "badges");
}
