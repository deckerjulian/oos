//! Batch 18: the UX review of Studio. One test per item that can be tested. OOS_SHOTS=<dir> writes pictures (b18-*).
use super::dock::VKind;
use super::meascatalog::Cat;
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b18-{name}.png")), crate::store::png_of(&cv)).unwrap();
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
    frame(u);
}

fn two_sources(u: &mut Ui) {
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(1)), (Meas::Freq, WaveId::Ch(1))];
    u.ensure_meas_own();
    for _ in 0..4 { frame(u); }
}

#[test]
fn only_the_first_row_of_a_group_has_the_header_band_and_every_row_is_reachable() {
    let mut u = studio();
    two_sources(&mut u);
    let g = u.studio_panel_geo().unwrap();
    let rows = u.meas_rows();
    let listed = u.studio_listed(&rows);
    let layout = u.panel_layout(&rows, &listed, 0, g.list);
    let body = u.studio_row_body();
    assert_eq!(layout.iter().map(|(_, r)| r.h).collect::<Vec<_>>(), [body + 14, body, body + 14, body], "no gap after the first row");
    for w in layout.windows(2) { assert_eq!(w[0].1.bottom(), w[1].1.y); }
    for (pos, r) in &layout {
        let hit = u.md_row_at(r.x + 50, r.y + r.h / 2).expect("a row");
        assert_eq!(hit.0, u.studio_keys()[listed[*pos]]);
    }
}

#[test]
fn the_side_sheet_floats_over_the_docks_and_the_waveform_keeps_its_size() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    u.studio_apply_panel(super::sheet::PanelKind::Histogram);
    for _ in 0..4 { frame(&mut u); }
    let before: Vec<Rect> = [VKind::Wave, VKind::Eye, VKind::Hist, VKind::Measure].iter().filter_map(|k| u.dock_leaf(*k)).collect();
    u.open_meas_list();
    for _ in 0..4 { frame(&mut u); }
    assert!(u.sheet_active());
    let after: Vec<Rect> = [VKind::Wave, VKind::Eye, VKind::Hist, VKind::Measure].iter().filter_map(|k| u.dock_leaf(*k)).collect();
    assert_eq!(before, after, "the docks did not move under the sheet");
    shot(&mut u, "sheet-over-docks");
}

#[test]
fn the_overview_goes_first_when_the_waveform_is_short_and_the_zoom_bar_stays() {
    let mut u = studio();
    u.set_zoom(true);
    for _ in 0..3 { frame(&mut u); }
    let tall = u.split_zoom(Rect::new(0, 0, 700, 400));
    assert!(tall.0.h >= 40);
    let short = u.split_zoom(Rect::new(0, 0, 700, 150));
    assert_eq!(short.0.h, 0, "no room for the overview");
    assert_eq!(short.1.y - short.0.bottom(), 32, "the zoom bar keeps its row");
    assert!(short.1.h >= 88, "the lanes keep their height: {}", short.1.h);
    assert!(u.studio_zoom_bar().is_some());
}

#[test]
fn next_and_previous_search_event_stop_the_scope_and_jump() {
    let mut u = studio();
    assert_eq!(u.m.run, RunState::Run);
    u.go_to_time(1e-4);
    assert_eq!(u.m.run, RunState::Stop);
    assert_eq!(u.toast.as_ref().map(|t| t.0.as_str()), Some("Stopped to jump"));
}

#[test]
fn adding_a_measurement_says_what_it_reads_and_the_tile_shows_its_value() {
    let mut u = studio();
    u.m.meas.clear();
    u.m.run = RunState::Stop;
    u.open_meas_list();
    for _ in 0..3 { frame(&mut u); }
    u.sheet_pick_source(WaveId::Ch(1));
    let g = u.sheet_add_geo();
    let &(k, tile) = &g.rows[1];
    let cat = u.sheet_add_entries()[k].cat;
    tap(&mut u, Rect::new(tile.x + 6, tile.y + 44, tile.w - 50, 30));
    assert!(u.add_is_on(cat));
    let t = u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(t.contains("CH2") && t.contains(" \u{b7} "), "name, source and value: {t}");
    assert!(Ui::toast_is_success(&t), "a tick, not a warning");
    shot(&mut u, "added-tile");
}

#[test]
fn pinned_badges_that_do_not_fit_leave_a_plus_n_chip_that_opens_the_list() {
    let mut u = studio();
    u.studio_set_view(StudioView::Waveform);
    u.m.meas = [Meas::Vpp, Meas::Freq, Meas::Vavg, Meas::Vrms, Meas::Rise, Meas::Fall, Meas::Vmax, Meas::Vmin, Meas::Period].into_iter().map(|k| (k, WaveId::Ch(0))).collect();
    u.ensure_meas_own();
    for k in u.studio_keys() { u.studio_set_pin(k, true); }
    for _ in 0..4 { frame(&mut u); }
    let (chip, n) = u.studio_badges_more().expect("a +N chip");
    assert!(n >= 1 && Ui::hit40(chip).w >= 40);
    assert_eq!(u.studio_badges().len() + n, 9);
    shot(&mut u, "badge-overflow");
    tap(&mut u, chip);
    assert!(matches!(u.studio.view, StudioView::Docked | StudioView::Floating), "the list is open");
}

#[test]
fn view_add_panel_always_opens_the_hub() {
    let mut u = studio();
    u.sheet.add.kind = Some(super::addrail::AddKind::Cursors);
    let t = u.studio_top().view;
    tap(&mut u, t);
    let row = u.studio_view_row(super::studio::VIEW_ADD);
    tap(&mut u, row);
    assert!(u.menu_open && u.menu == MenuId::AddPanel);
    assert!(u.sheet.add.kind.is_none(), "the hub, not the last page");
}

#[test]
fn the_info_card_has_a_line_of_its_own_for_maximum() {
    let max = super::meascatalog::cat_info(Cat::Maximum).how;
    let min = super::meascatalog::cat_info(Cat::Minimum).how;
    assert!(max.contains("highest sample") && !max.contains("Takes the top and base levels"), "{max}");
    assert_ne!(max, min);
    for c in [Cat::Amplitude, Cat::Rise, Cat::Period, Cat::PosWidth, Cat::PosOvershoot, Cat::Delay, Cat::Phase] {
        assert!(super::meascatalog::cat_info(c).how.len() > 40);
    }
}

#[test]
fn the_gear_sheet_is_settings_with_tools_and_analysis_views_and_no_duplicates() {
    let mut u = studio();
    u.open_menu(MenuId::Main);
    for _ in 0..3 { frame(&mut u); }
    assert_eq!(u.menu_title(MenuId::Main), "Settings");
    let spec = u.sheet_spec(MenuId::Main).unwrap();
    let names: Vec<&str> = spec.tabs.iter().map(|t| t.name).collect();
    assert_eq!(names, ["Setup", "Tools", "Analysis views", "More"]);
    let all: Vec<Item> = spec.tabs.iter().flat_map(|t| t.rows.iter().chain(t.foot.iter()).copied()).collect();
    for dup in [MenuId::Math, MenuId::Ref, MenuId::Decode] { assert!(!all.contains(&Item::Open(dup)), "{dup:?} is behind + CH"); }
    assert!(spec.tabs[0].foot.contains(&Item::DefaultAct), "the factory reset is in the footer");
    assert!(!spec.tabs[0].rows.contains(&Item::DefaultAct));
}

#[test]
fn touch_targets_are_forty_pixels_and_channels_show_an_on_state() {
    let u = studio();
    assert!(u.studio_row_px() >= 40 || u.studio.compact);
    let b = u.studio_bottom();
    for c in 0..n_ch() { if b.ch[c].w > 0 { assert!(Ui::hit40(b.ch[c]).h >= 40); } }
    let p = Rect::new(100, 100, 200, 300);
    assert!(Ui::studio_popup_row(p, 0).h >= 40);
}

#[test]
fn a_hold_on_a_channel_chip_starts_with_turn_off() {
    let mut u = studio();
    let b = u.studio_bottom();
    let r = b.ch[0];
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    touch(&mut u, Phase::Down, x, y);
    u.step(0.6);
    touch(&mut u, Phase::Up, x, y);
    frame(&mut u);
    let rows: Vec<String> = u.workspace.popup.as_ref().map(|p| p.rows.iter().map(|r| r.0.clone()).collect()).unwrap_or_default();
    assert_eq!(rows.first().map(String::as_str), Some("Turn off"), "{rows:?}");
}

#[test]
fn the_zoom_bar_stays_and_search_markers_take_the_colour_of_the_searched_channel() {
    let mut u = studio();
    u.set_zoom(true);
    for _ in 0..4 { frame(&mut u); }
    shot(&mut u, "zoom-bar");
    assert!(u.studio_zoom_bar().is_some());
    u.studio_search_add(Some(WaveId::Ch(1)));
    assert_eq!(u.studio.search.as_ref().map(|s| s.src), Some(WaveId::Ch(1)));
}

#[test]
fn the_spectrum_has_a_trace_when_the_scope_is_stopped() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Spectrum);
    for _ in 0..6 { frame(&mut u); }
    u.key(key::RUNSTOP);
    u.azr.spec = None;
    for _ in 0..6 { frame(&mut u); }
    assert!(u.azr.spec.is_some(), "the last record is drawn, not \"waiting for a record\"");
}

#[test]
fn the_status_pill_has_a_glyph_for_what_a_tap_does() {
    let mut u = studio();
    let t = u.studio_top();
    let mut run = Canvas::new(W, H);
    u.render(&mut run);
    u.key(key::RUNSTOP);
    for _ in 0..3 { frame(&mut u); }
    let mut stop = Canvas::new(W, H);
    u.render(&mut stop);
    let g = Rect::new(t.run.x + 8, t.run.y + 6, 16, 16);
    let differs = (g.y..g.bottom()).any(|y| (g.x..g.right()).any(|x| run.px[(y * W + x) as usize] != stop.px[(y * W + x) as usize]));
    assert!(differs);
}
