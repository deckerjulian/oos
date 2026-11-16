//! Batch 17: colour carries the source. One small header per source in the list (none with one source), a thin colour
//! bar on the rows, the table names the source only in the first row of a group, badges have no chip.
//! OOS_SHOTS=<dir> writes pictures (b17-*).
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
    u.m.run = RunState::Stop;
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b17-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn two_sources(u: &mut Ui) {
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1)), (Meas::Freq, WaveId::Ch(1))];
    u.ensure_meas_own();
    for _ in 0..4 { frame(u); }
}

fn one_source(u: &mut Ui) {
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vavg, WaveId::Ch(0))];
    u.ensure_meas_own();
    for _ in 0..4 { frame(u); }
}

/// Pixels of exactly this colour in `r` of a canvas.
fn count(cv: &Canvas, r: Rect, col: Rgb) -> usize {
    (r.y..r.bottom()).flat_map(|y| (r.x..r.right()).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] == col).count()
}

#[test]
fn with_one_source_a_row_has_no_chip_and_the_list_has_no_header() {
    let mut u = studio();
    one_source(&mut u);
    assert!(!u.studio_multi_source(), "one source");
    let g = u.studio_panel_geo().unwrap();
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let yellow = u.wave_color(WaveId::Ch(0));
    // the colour is in a thin bar at the left edge only: nothing of that colour right of it in the first row
    let row = Rect::new(g.list.x, g.list.y, g.list.w, u.studio_panel_row());
    assert!(count(&cv, Rect::new(row.x + 3, row.y + 4, 5, row.h - 8), yellow) > 20, "the colour bar");
    // Batch 21: the second line of a row names its source in its colour (the list view of the mockups); the first line (name, value) has none
    assert_eq!(count(&cv, Rect::new(row.x + 9, row.y, row.w - 9, 26), yellow), 0, "no chip of the source in the first line of the row");
    // no band for a header: the first row starts with its name
    assert_eq!(u.studio_panel_row(), if u.studio.compact { 36 } else { 46 });
    shot(&mut u, "one-source");
}

#[test]
fn with_several_sources_each_has_one_header_and_the_rows_are_grouped() {
    let mut u = studio();
    two_sources(&mut u);
    assert!(u.studio_multi_source());
    let rows = u.meas_rows();
    let listed = u.studio_listed(&rows);
    let srcs: Vec<WaveId> = listed.iter().map(|&i| rows[i].src).collect();
    assert_eq!(srcs, [WaveId::Ch(0), WaveId::Ch(0), WaveId::Ch(1), WaveId::Ch(1)], "grouped");
    let g = u.studio_panel_geo().unwrap();
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let layout = u.panel_layout(&rows, &listed, 0, g.list);
    assert_eq!(layout.len(), 4);
    // only the first row of a group has the band
    let body = u.studio_row_body();
    assert_eq!(layout.iter().map(|(_, r)| r.h).collect::<Vec<_>>(), [body + 14, body, body + 14, body]);
    let band = |k: usize| Rect::new(layout[k].1.x + 8, layout[k].1.y, 60, 14);
    let (c0, c1) = (u.wave_color(WaveId::Ch(0)), u.wave_color(WaveId::Ch(1)));
    assert!(count(&cv, band(0), c0) > 5, "the header CH1 over the first group");
    assert_eq!(count(&cv, band(1), c0), 0, "no second header in the same group");
    assert!(count(&cv, band(2), c1) > 5, "the header CH2 over the second group");
    assert_eq!(count(&cv, band(3), c1), 0);
    shot(&mut u, "two-sources");
}

#[test]
fn the_detailed_table_names_the_source_only_in_the_first_row_of_a_group() {
    let mut u = studio();
    two_sources(&mut u);
    u.studio_set_detailed(true);
    for _ in 0..4 { frame(&mut u); }
    let g = u.studio_panel_geo().unwrap();
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let pitch = u.studio_panel_row();
    let c0 = u.wave_color(WaveId::Ch(0));
    let name = |k: i32| Rect::new(g.list.x + 10, g.list.y + k * pitch, 24, pitch);
    assert!(count(&cv, name(0), c0) > 5, "CH1 in the first row");
    assert_eq!(count(&cv, name(1), c0), 0, "not in the second");
    // with one source the column has only the bar
    one_source(&mut u);
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    assert_eq!(count(&cv, name(0), c0), 0, "no name with one source");
    shot(&mut u, "table-one-source");
}

#[test]
fn a_badge_has_no_chip_a_colour_bar_and_the_short_name() {
    let mut u = studio();
    two_sources(&mut u);
    u.studio_set_view(StudioView::Waveform);
    for _ in 0..3 { frame(&mut u); }
    let badges = u.studio_badges();
    assert!(!badges.is_empty());
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let (key, r) = badges[0];
    let col = u.wave_color(WaveId::Ch(0));
    assert!(count(&cv, Rect::new(r.x + 3, r.y + 4, 5, r.h - 8), col) > 10, "the colour bar at the edge");
    assert_eq!(count(&cv, Rect::new(r.x + 9, r.y, r.w - 9, r.h), col), 0, "no chip in the badge");
    assert_eq!(u.studio_badge_name(key, "Peak-to-Peak"), "Pk-pk");
    shot(&mut u, "badges");
}
