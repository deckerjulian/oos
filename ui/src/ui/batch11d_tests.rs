//! Batch 11c: a view draws only inside its own rectangle, a narrow spectrum keeps its width for the plot, a narrow
//! table puts its controls on a row of their own, and the "+ CH" chip stays when every channel is on.
//! OOS_SHOTS=<dir> writes pictures (b11c-*).
use super::dock::VKind;
use super::studio::{PickAdd, StudioView};
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

fn render(u: &mut Ui) -> Canvas {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..4 { u.step(0.05); }
    cv = Canvas::new(W, H);
    u.render(&mut cv);
    cv
}

fn shot(u: &mut Ui, name: &str) {
    let cv = render(u);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b11c-{name}.png")), crate::store::png_of(&cv)).unwrap();
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
}

#[test]
fn a_narrow_spectrum_keeps_the_plot_wide_and_draws_only_in_its_view() {
    let mut u = studio();
    u.m.view = AView::Spectrum;
    u.m.hist = Hist::Vertical;
    for _ in 0..6 { render(&mut u); }
    let r = u.dock_rect(VKind::View).expect("the analyser view");
    assert!(r.w < panes::SPEC_NARROW_W, "a narrow view: {}", r.w);
    // no column of peaks: the plot has the width
    let b = u.spec_box(u.dock_content(VKind::View).unwrap(), false);
    assert_eq!((b.sort.w, b.list.w), (0, 0));
    assert!(b.plot.w >= r.w - 80, "{} of {}", b.plot.w, r.w);
    // a wide one has the column
    let wide = u.spec_box(Rect::new(0, 0, 700, 400), false);
    assert!(wide.sort.w > 0 && wide.list.w > 0);
    // what the views draw stays inside them: on a canvas of one colour, nothing outside their rectangles changes
    let mut cv = Canvas::new(W, H);
    cv.fill(Rect::new(0, 0, W, H), 0x123456);
    u.draw_view_tiles(&mut cv);
    let tiles: Vec<Rect> = [VKind::Hist, VKind::View].iter().filter_map(|k| u.dock_rect(*k)).collect();
    assert_eq!(tiles.len(), 2);
    for y in 0..H {
        for x in 0..W {
            if !tiles.iter().any(|t| t.contains(x, y)) { assert_eq!(cv.px[(y * W + x) as usize], 0x123456, "drawn outside the views at {x},{y}"); }
        }
    }
    shot(&mut u, "spectrum-narrow");
}

#[test]
fn a_narrow_search_table_has_its_controls_on_a_row_of_their_own() {
    let mut u = studio();
    u.table = Some(tables::TableKind::Search);
    for _ in 0..4 { render(&mut u); }
    let r = u.dock_content(VKind::Table).expect("the table view");
    // wide: one row
    let wide = Rect::new(0, 0, 500, 300);
    assert!(!u.table_narrow(wide));
    // narrow: squeeze it by dragging its divider
    let d = u.dock_layout().dividers.iter().find(|d| d.dir == super::dock::Dir::H).cloned();
    if let Some(d) = d {
        let (x, y) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
        touch(&mut u, Phase::Down, x, y);
        for k in 1..=8 { touch(&mut u, Phase::Move, x + k * 30, y); }
        touch(&mut u, Phase::Up, x + 240, y);
    }
    render(&mut u);
    let r2 = u.dock_content(VKind::Table).unwrap_or(r);
    let narrow = Rect::new(r2.x, r2.y, 200, r2.h);
    assert!(u.table_narrow(narrow));
    assert!(u.head_h(narrow) > u.head_h(wide));
    let b = u.table_buttons(narrow);
    assert!(b.iter().all(|x| x.h >= 40 && x.w >= 40), "40 px targets: {b:?}");
    assert!(b.iter().all(|x| x.y >= narrow.y + 40), "under the title row");
    assert!(b[0].right() <= b[1].x && b[1].right() <= b[2].x, "not jammed: {b:?}");
    assert!(b[2].right() <= narrow.right());
    shot(&mut u, "narrow-search");
}

#[test]
fn the_source_chip_stays_with_every_channel_on() {
    let mut u = studio();
    for c in 0..n_ch() { u.m.ch[c].on = true; }
    render(&mut u);
    let b = u.studio_bottom();
    assert!(b.add.w >= 40, "the chip stays");
    tap(&mut u, b.add);
    let (chans, extras) = u.studio_picker_rows().expect("the popup");
    assert_eq!(chans.len(), n_ch(), "every channel is listed, to turn off");
    assert_eq!(extras.iter().map(|e| e.0).collect::<Vec<_>>(), [PickAdd::Math, PickAdd::Ref, PickAdd::Bus]);
    // a channel row turns the channel off
    tap(&mut u, chans[n_ch() - 1].1);
    assert!(!u.m.ch[n_ch() - 1].on && !u.studio.ch_picker);
    shot(&mut u, "source-chip");
    // Default and Minimal have no such chip
    for layout in [Layout::Default, Layout::Minimal] {
        let mut v = Ui::new(Source::Demo { t: 0.0 });
        v.m.layout = layout;
        for _ in 0..4 { render(&mut v); }
        assert!(v.studio.ch_picker == false);
    }
}
