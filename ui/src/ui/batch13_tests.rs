//! Batch 13: the Studio measurements panel in two modes (list with an optional line of statistics, and a detailed
//! table that collapses when narrow), and rows that have no value say why. OOS_SHOTS=<dir> writes pictures (b13-*).
use super::dock::{Dir, VKind};
use super::meascatalog::Cat;
use super::measview::{DETAIL_COLS, STATS_LINE};
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b13-{name}.png")), crate::store::png_of(&cv)).unwrap();
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
    frame(u);
}

fn tap(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn switch(u: &Ui) -> Rect {
    let r = u.dock_rect(VKind::Measure).expect("the panel");
    u.dock_view_rect(VKind::Measure, r).expect("the view switch")
}

/// The panel made `w` wide by its divider.
fn panel_width(u: &mut Ui, w: i32) {
    let d = u.dock_layout().dividers.iter().find(|d| d.dir == Dir::H).cloned().expect("a divider");
    let (x, y) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
    let have = u.dock_leaf(VKind::Measure).unwrap().w;
    drag(u, (x, y), (x + have - w, y));
}

// ------------------------------------------------------------------------------------------------ the two modes

#[test]
fn the_switch_in_the_header_toggles_list_and_detailed_and_is_saved() {
    let mut u = studio();
    let s = switch(&u);
    assert!(s.h >= 40 && s.w >= 40, "40 px target: {s:?}");
    assert!(!u.studio_detailed());
    tap(&mut u, s);
    assert!(u.studio_detailed() && u.m.studio_detail);
    assert_eq!(u.studio_panel_row(), super::measview::DETAIL_ROW);
    // saved with the Studio setup, and read back
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("studio_meas_view = detailed"), "{}", text.lines().filter(|l| l.starts_with("studio")).collect::<Vec<_>>().join(" | "));
    let m = crate::store::parse_setup(&text).expect("reads back");
    assert!(m.studio_detail);
    // back to the list
    let s = switch(&u);
    tap(&mut u, s);
    assert!(!u.studio_detailed());
    assert!(!crate::store::setup_text(&u.m).contains("studio_meas_view"));
    // the other layouts have no such switch
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m.studio_detail = true;
    v.m.layout = Layout::Default;
    assert!(!v.studio_detailed());
}

#[test]
fn the_detailed_table_has_source_name_value_mean_min_max_sigma_and_count() {
    let mut u = studio();
    let names: Vec<&str> = DETAIL_COLS.iter().map(|c| c.0).collect();
    assert_eq!(names, ["Source", "Name", "Value", "Mean", "Min", "Max", "\u{3c3}", "Count"]);
    // a table wide enough has all eight
    let (cols, total) = Ui::detail_cols(800);
    assert_eq!(cols.len(), 8);
    assert!(total <= 800);
    // statistics gather while the scope runs
    u.m.run = RunState::Run;
    for _ in 0..30 { frame(&mut u); }
    let s = switch(&u);
    tap(&mut u, s);
    let n = u.meas_rows().len();
    assert!(n > 0);
    let st = (0..n).find_map(|i| u.studio_stat_values(i));
    if let Some((mean, min, max, sd, count)) = st {
        assert!(min <= mean + 1e-9 && mean <= max + 1e-9 && sd >= 0.0 && count > 0, "{mean} {min} {max} {sd} {count}");
    }
    shot(&mut u, "detailed");
}

#[test]
fn a_narrow_table_drops_sigma_and_count_and_scrolls_sideways() {
    let mut u = studio();
    let s = switch(&u);
    tap(&mut u, s);
    // wide: everything
    assert_eq!(Ui::detail_cols(700).0.len(), 8);
    // short of room: sigma and count go first
    let (cols, total) = Ui::detail_cols(560);
    assert_eq!(cols.iter().map(|c| c.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
    assert!(total <= 560);
    // narrower than what is left: it scrolls
    let (_, total) = Ui::detail_cols(200);
    assert!(total > 200);
    panel_width(&mut u, 200);
    let g = u.studio_panel_geo().unwrap();
    assert!(g.p.w <= 260, "narrow panel: {}", g.p.w);
    assert!(u.detail_hmax(g.list.w) > 0);
    assert_eq!(u.studio.panel.hx, 0);
    let y = g.list.y + u.studio_panel_row() / 2;
    drag(&mut u, (g.list.right() - 20, y), (g.list.x + 20, y));
    assert!(u.studio.panel.hx > 0, "scrolled sideways");
    assert!(u.studio.panel.hx <= u.detail_hmax(g.list.w));
    // a tap on a row of the table still opens its settings
    u.studio.panel.hx = 0;
    tap(&mut u, Rect::new(g.list.x + 100, y - 10, 20, 20));
    assert!(u.meas_cfg.is_some() || u.menu_open);
    u.close_menu();
    shot(&mut u, "detailed-narrow");
}

// ------------------------------------------------------------------------------------------------ statistics in the list

#[test]
fn the_list_shows_a_small_line_of_statistics_only_when_they_are_on() {
    let mut u = studio();
    u.m.run = RunState::Run;
    for _ in 0..30 { frame(&mut u); }
    let off_h = u.studio_panel_row();
    let g = u.studio_panel_geo().unwrap();
    let row0 = Rect::new(g.list.x, g.list.y, g.list.w, off_h);
    // off: nothing more than today
    let mut before = Canvas::new(W, H);
    u.render(&mut before);
    assert!(!u.studio_stats_any());
    // on for all rows: rows grow by the line
    u.studio.panel.stats = true;
    frame(&mut u);
    assert_eq!(u.studio_panel_row(), off_h + STATS_LINE);
    let key = u.studio_keys()[0];
    assert!(u.studio_stats_on(key));
    let mut after = Canvas::new(W, H);
    u.render(&mut after);
    // the new line is drawn in the lower part of the first row
    let line = Rect::new(row0.x + 8, row0.y + off_h, row0.w - 16, STATS_LINE);
    let differs = (line.y..line.bottom()).any(|y| (line.x..line.right()).any(|x| after.px[(y * W + x) as usize] != before.px[(y * W + x) as usize]));
    assert!(differs, "a line of statistics under the value");
    shot(&mut u, "list-stats");
    // off again
    u.studio.panel.stats = false;
    frame(&mut u);
    assert_eq!(u.studio_panel_row(), off_h);
    // for one row, from its settings menu
    let g = u.studio_panel_geo().unwrap();
    let (_, menu) = Ui::md_row_buttons(Rect::new(g.list.x, g.list.y, g.list.w, off_h));
    tap(&mut u, menu);
    let p = u.md_menu_rect().expect("the row menu");
    let rows = 7;
    assert!(rows >= 7);
    tap(&mut u, Ui::md_menu_row(p, 5));
    assert!(u.studio_stats_on(key) && u.studio_stats_any());
    assert!(!u.studio_stats_on(u.studio_keys()[1]), "only that row");
    assert_eq!(u.studio_panel_row(), off_h + STATS_LINE);
}

// ------------------------------------------------------------------------------------------------ jitter

/// Steps the frames, with the worker thread time to answer, until the row has a result or a reason.
fn settle(u: &mut Ui, name: &str) -> String {
    for _ in 0..120 {
        for _ in 0..3 { frame(u); }
        std::thread::sleep(std::time::Duration::from_millis(50));
        let t = u.meas_rows().iter().find(|r| r.name == name).map(|r| r.text()).unwrap_or_default();
        if t != "Computing" && !t.is_empty() { return t; }
    }
    "Computing".into()
}

#[test]
fn jitter_summary_resolves_or_says_why_and_never_computes_for_ever() {
    let mut u = studio();
    assert_eq!(u.m.run, RunState::Run, "live, so every frame brings a new record");
    u.m.cat.push((Cat::JitterSummary as u16, WaveId::Ch(0), WaveId::Ch(0), WaveId::Ch(0), WaveId::Ch(0)));
    u.m.cat.push((Cat::JitterSummary as u16, WaveId::Ch(1), WaveId::Ch(1), WaveId::Ch(1), WaveId::Ch(1)));
    let t = settle(&mut u, "Jitter Summary");
    assert_ne!(t, "Computing", "it ends");
    // either a value or a short reason
    assert!(t != "----", "a value or a reason, not dashes: {t}");
    // the same for the other analysis of the eye
    u.m.cat.push((Cat::EyeHeightBer as u16, WaveId::Ch(0), WaveId::Ch(0), WaveId::Ch(0), WaveId::Ch(0)));
    let t = settle(&mut u, "Eye Height@BER");
    assert!(t != "Computing" && t != "----", "{t}");
}

#[test]
fn a_ref_slot_without_data_says_so() {
    let mut u = studio();
    u.m.cat.push((Cat::EyeHeightBer as u16, WaveId::Ref(0), WaveId::Ref(0), WaveId::Ref(0), WaveId::Ref(0)));
    let rows = u.meas_rows();
    let row = rows.iter().find(|r| r.name == "Eye Height@BER").expect("the row is listed");
    let t = row.text();
    assert!(t.contains("R1") && t.contains("empty"), "{t}");
    assert!(t.chars().count() <= 24, "short: {t}");
}
