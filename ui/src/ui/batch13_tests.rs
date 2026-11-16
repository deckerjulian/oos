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
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
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
    tap(&mut u, seg(s, 2));
    assert!(u.studio_detailed() && u.m.studio_detail);
    assert_eq!(u.studio_panel_row(), super::measview::DETAIL_ROW);
    // saved with the Studio setup, and read back
    let text = crate::store::setup_text(&u.m);
    assert!(!text.contains("studio_meas_view"), "the table is the default: nothing is written");
    let m = crate::store::parse_setup(&text).expect("reads back");
    assert!(m.studio_detail);
    // back to the list
    let s = switch(&u);
    tap(&mut u, seg(s, 0));
    assert!(!u.studio_detailed());
    assert!(crate::store::setup_text(&u.m).contains("studio_meas_view = list"));
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
    tap(&mut u, seg(s, 2));
    // wide: everything
    assert_eq!(Ui::detail_cols(700).0.len(), 8);
    // short of room: sigma and count go first
    let (cols, total) = Ui::detail_cols(560);
    assert_eq!(cols.iter().map(|c| c.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);  // by priority: name, value, mean, min, max, sigma, count
    assert!(total <= 560);
    // narrower than that: columns drop, none is half drawn
    let (_, total) = Ui::detail_cols(200);
    assert!(total <= 200);
    panel_width(&mut u, 200);
    let g = u.studio_panel_geo().unwrap();
    assert!(g.p.w <= 260, "narrow panel: {}", g.p.w);
    // batch 22: the columns that do not fit are dropped, so there is nothing to scroll to
    assert_eq!(u.detail_hmax(g.list.w), 0);
    assert_eq!(u.studio.panel.hx, 0);
    let y = g.list.y + u.studio_panel_row() / 2;
    // a tap on a row of the table still opens its settings
    u.studio.panel.hx = 0;
    tap(&mut u, Rect::new(g.list.x + 40, y - 10, 20, 20));
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

// ------------------------------------------------------------------------------------------------ math and ref chips

fn chip_rect(u: &Ui, math: bool) -> Rect {
    let b = u.studio_bottom();
    if math { b.math[0] } else { b.refs[0] }
}

fn hold(u: &mut Ui, r: Rect) {
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(u, Phase::Down, x, y);
    u.step(0.6);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn popup_rows(u: &Ui) -> Vec<String> {
    u.workspace.popup.as_ref().map(|p| p.rows.iter().map(|r| r.0.clone()).collect()).unwrap_or_default()
}

#[test]
fn a_hold_or_a_right_click_on_a_math_chip_opens_turn_off_and_settings() {
    let mut u = studio();
    u.m.math = crate::math::MathOp::Add;
    frame(&mut u);
    let r = chip_rect(&u, true);
    assert!(r.w > 0, "the M1 chip");
    // a tap selects it and (Batch 21, like a channel pill) opens the Math sheet; it is closed again for the hold
    tap(&mut u, r);
    assert!(u.workspace.popup.is_none());
    assert!(u.menu_open && u.menu == MenuId::Math);
    u.close_menu();
    frame(&mut u);
    // a hold
    hold(&mut u, r);
    assert_eq!(popup_rows(&u), ["Turn off", "M1 settings"]);
    // the lift of the hold did not choose a row; a tap on the first row turns it off
    let p = u.workspace.popup.as_ref().unwrap();
    let row = p.row_rect(0);
    tap(&mut u, row);
    assert_eq!(u.m.math, crate::math::MathOp::Off, "M1 is off");
    assert_eq!(u.studio_bottom().math[0].w, 0, "its chip is gone");
    // a right-click opens the same popup; "M1 settings" opens the math menu
    u.m.math = crate::math::MathOp::Add;
    frame(&mut u);
    let r = chip_rect(&u, true);
    u.context_menu(r.x + r.w / 2, r.y + r.h / 2);
    assert_eq!(popup_rows(&u), ["Turn off", "M1 settings"]);
    let row = u.workspace.popup.as_ref().unwrap().row_rect(1);
    tap(&mut u, row);
    assert!(u.menu_open && u.menu == MenuId::Math);
    // and in Studio the menu itself starts with Turn off
    assert_eq!(u.menu_items(MenuId::Math).first(), Some(&Item::MathOff));
    u.activate(Item::MathOff);
    assert_eq!(u.m.math, crate::math::MathOp::Off);
    // not in the other layouts
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m.math = crate::math::MathOp::Add;
    assert!(!v.menu_items(MenuId::Math).contains(&Item::MathOff));
}

#[test]
fn a_hold_on_a_ref_chip_opens_hide_clear_and_settings() {
    let mut u = studio();
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-6, v: (0..1000).map(|i| (i as f32 * 0.05).sin()).collect::<Vec<_>>().into(), vpd: 1.0, offset: 0.0, on: true });
    frame(&mut u);
    let r = chip_rect(&u, false);
    assert!(r.w > 0, "the R1 chip");
    hold(&mut u, r);
    assert_eq!(popup_rows(&u), ["Hide", "Clear slot", "Horizontal: Follow main", "R1 settings"]);
    let row = u.workspace.popup.as_ref().unwrap().row_rect(0);
    tap(&mut u, row);
    assert!(u.refs[0].as_ref().is_some_and(|r| !r.on), "hidden, not cleared");
    // clear the slot
    u.refs[0].as_mut().unwrap().on = true;
    frame(&mut u);
    let r = chip_rect(&u, false);
    u.context_menu(r.x + r.w / 2, r.y + r.h / 2);
    let row = u.workspace.popup.as_ref().unwrap().row_rect(1);
    tap(&mut u, row);
    assert!(u.refs[0].is_none(), "the slot is clear");
}

// ------------------------------------------------------------------------------------------------ the axes as handles

fn touch_id(u: &mut Ui, id: i32, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id, phase, x, y });
    u.step(0.01);
}

/// Two fingers `gap` apart along an axis, spread to `to` apart.
fn pinch_at(u: &mut Ui, p: (i32, i32), horizontal: bool, gap: i32, to: i32) {
    let at = |d: i32| if horizontal { (p.0 + d, p.1) } else { (p.0, p.1 + d) };
    touch_id(u, 0, Phase::Down, p.0, p.1);
    touch_id(u, 1, Phase::Down, at(gap).0, at(gap).1);
    for k in 1..=8 { let d = gap + (to - gap) * k / 8; touch_id(u, 1, Phase::Move, at(d).0, at(d).1); }
    touch_id(u, 1, Phase::Up, at(to).0, at(to).1);
    touch_id(u, 0, Phase::Up, p.0, p.1);
    frame(u);
}

#[test]
fn a_pinch_on_the_time_axis_changes_the_time_div_and_the_strip_is_finger_sized() {
    let mut u = studio();
    let s = u.axis_time_strip().expect("the time strip");
    assert!(s.h >= 40, "40 px: {s:?}");
    let t0 = u.m.tscale;
    // a spread zooms in: a shorter time/div, in 1-2-5 steps
    pinch_at(&mut u, (s.x + s.w / 2 - 100, s.y + s.h - 12), true, 80, 260);
    assert!(u.m.tscale < t0, "spread: {t0} -> {}", u.m.tscale);
    let t1 = u.m.tscale;
    pinch_at(&mut u, (s.x + s.w / 2 - 100, s.y + s.h - 12), true, 260, 80);
    assert!(u.m.tscale > t1, "pinched in: {t1} -> {}", u.m.tscale);
    // with the zoom on, the main time base changes and the zoom keeps its factor
    u.m.tscale = u.m.tscale.max(12);
    u.set_zoom(true);
    frame(&mut u);
    let (z, factor) = (u.m.zscale, u.m.tscale as i64 - u.m.zscale as i64);
    let t0 = u.m.tscale;
    let s = u.axis_time_strip().unwrap();
    pinch_at(&mut u, (s.x + s.w / 2 - 100, s.y + s.h - 12), true, 80, 260);
    assert!(u.m.tscale < t0, "the main time base changed: {t0} -> {}", u.m.tscale);
    assert_eq!(u.m.tscale as i64 - u.m.zscale as i64, factor, "the zoom keeps its factor ({z})");
}

#[test]
fn a_pinch_on_the_volts_labels_changes_the_v_div_of_the_channel() {
    let mut u = studio();
    let s = u.axis_volts_strip(0).expect("the volts strip");
    assert!(s.w >= 40, "40 px: {s:?}");
    let v0 = u.m.ch[0].vscale;
    // away from the ground marker of the lane
    let y = s.y + 24;
    assert_eq!(u.axis_at(s.x + 30, y), Some(super::axis::Axis::Volts(0)));
    pinch_at(&mut u, (s.x + 30, y), false, 60, 200);
    assert!(u.m.ch[0].vscale < v0, "spread: {v0} -> {}", u.m.ch[0].vscale);
    let v1 = u.m.ch[0].vscale;
    pinch_at(&mut u, (s.x + 30, y), false, 200, 60);
    assert!(u.m.ch[0].vscale > v1);
    // the second lane has its own channel
    let s2 = u.axis_volts_strip(1).unwrap();
    assert_eq!(u.axis_at(s2.x + 30, s2.y + 24), Some(super::axis::Axis::Volts(1)));
}

#[test]
fn the_wheel_and_a_drag_on_the_axes_work_and_the_strip_lights_up() {
    let mut u = studio();
    let t = u.axis_time_strip().unwrap();
    let t0 = u.m.tscale;
    assert!(u.pointer_wheel(t.x + t.w / 2, t.y + t.h - 10, 1, false));
    assert!(u.m.tscale < t0, "wheel on the time axis");
    let v = u.axis_volts_strip(0).unwrap();
    let v0 = u.m.ch[0].vscale;
    assert!(u.pointer_wheel(v.x + 30, v.y + 24, -1, false));
    assert!(u.m.ch[0].vscale > v0, "wheel on the volts axis");
    // a drag along the time axis, lit while it is touched
    let t1 = u.m.tscale;
    let (x, y) = (t.x + t.w / 2, t.y + t.h - 10);
    let mut plain = Canvas::new(W, H);
    u.render(&mut plain);
    touch(&mut u, Phase::Down, x, y);
    for k in 1..=6 { touch(&mut u, Phase::Move, x + k * 25, y); }
    let mut lit = Canvas::new(W, H);
    u.render(&mut lit);
    assert!(u.studio.axis.touch.is_some());
    assert!(lit.px[((t.y + 3) * W + t.x + 200) as usize] != plain.px[((t.y + 3) * W + t.x + 200) as usize], "the strip is lit");
    shot(&mut u, "axis-lit");
    touch(&mut u, Phase::Up, x + 150, y);
    frame(&mut u);
    assert!(u.m.tscale < t1, "a drag to the right zooms in: {t1} -> {}", u.m.tscale);
    // and a vertical drag on the volts labels
    let v1 = u.m.ch[0].vscale;
    drag(&mut u, (v.x + 30, v.y + 80), (v.x + 30, v.y + 80 - 120));
    assert!(u.m.ch[0].vscale < v1, "a drag up zooms in");
}

#[test]
fn a_pinch_on_the_waveform_itself_does_nothing_and_ctrl_wheel_is_as_it_was() {
    let mut u = studio();
    u.m.tscale = u.m.tscale.max(12);
    u.set_zoom(true);
    frame(&mut u);
    let lane = u.wins[0];
    let (x, y) = (lane.x + lane.w / 2, lane.y + lane.h / 3);
    assert_eq!(u.axis_at(x, y), None, "not on a strip");
    let before = (u.m.tscale, u.m.zscale, u.m.ch[0].vscale, u.m.ch[1].vscale);
    pinch_at(&mut u, (x - 100, y), true, 80, 260);
    pinch_at(&mut u, (x, y - 60), false, 60, 200);
    assert_eq!((u.m.tscale, u.m.zscale, u.m.ch[0].vscale, u.m.ch[1].vscale), before);
    // Ctrl-wheel over the waveform (no zoom): the main time base, as before
    u.set_zoom(false);
    frame(&mut u);
    let lane = u.wins[0];
    let t0 = u.m.tscale;
    assert!(u.pointer_wheel(lane.x + lane.w / 2, lane.y + lane.h / 3, 1, true));
    assert_ne!(u.m.tscale, t0);
}

/// Segment `k` (0 List, 1 Cards, 2 Detailed) of the three-way switch.
fn seg(s: Rect, k: i32) -> Rect {
    // the switch reads Table, List, Cards now: the old index (List, Cards, Detailed) moves one place
    let k = (k + 1) % 3;
    Rect::new(s.x + (2 * k + 1) * s.w / 6 - 2, s.y, 4, s.h)
}
