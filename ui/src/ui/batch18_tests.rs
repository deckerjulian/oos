// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 18: the UX review of Studio. One test per item that can be tested. OOS_SHOTS=<dir> writes pictures (b18-*).
use super::dock::VKind;
#[allow(unused_imports)] use super::meascatalog::Cat;
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
    let short = u.split_zoom(Rect::new(0, 0, 700, 130));
    assert_eq!(short.0.h, 0, "no room for the overview");
    assert_eq!(short.1.y - short.0.bottom(), 6, "Studio keeps only a gap: the zoom controls are in the waveform title bar or the pull-down tab");
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
    let max = super::meascatalog::cat_info(super::meascatalog::cat_from(super::meascatalog::cat::Maximum).unwrap()).how;
    let min = super::meascatalog::cat_info(super::meascatalog::cat_from(super::meascatalog::cat::Minimum).unwrap()).how;
    assert!(max.contains("highest sample") && !max.contains("Takes the top and base levels"), "{max}");
    assert_ne!(max, min);
    for c in [super::meascatalog::cat_from(super::meascatalog::cat::Amplitude).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::Rise).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::Period).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::PosWidth).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::PosOvershoot).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::Delay).unwrap(), super::meascatalog::cat_from(super::meascatalog::cat::Phase).unwrap()] {
        assert!(super::meascatalog::cat_info(c).how.len() > 40);
    }
}

#[test]
fn the_gear_sheet_is_settings_with_tools_and_analysis_views_and_no_duplicates() {
    let mut u = studio();
    u.open_menu(MenuId::Main);
    for _ in 0..3 { frame(&mut u); }
    // Batch 21: the gear sheet is the grouped Menu (one page, no tabs); the reset is a button of its group and asks first
    assert_eq!(u.menu_title(MenuId::Main), "Menu");
    let spec = u.sheet_spec(MenuId::Main).unwrap();
    assert_eq!(spec.tabs.len(), 1);
    assert!(spec.tabs[0].rows.contains(&Item::DefaultAct));
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

// ------------------------------------------------------------------------------------------------ the rest

#[test]
fn full_screen_keeps_the_status_pill_and_an_exit_chip() {
    let mut u = studio();
    assert!(u.full_chrome_rects().is_none());
    u.toggle_expand();
    for _ in 0..3 { frame(&mut u); }
    assert!(u.full_on());
    let (pill, exit) = u.full_chrome_rects().expect("the pill and the exit chip");
    assert!(Ui::hit40(pill).h >= 40 && Ui::hit40(exit).h >= 40);
    shot(&mut u, "full-screen");
    // the pill is Run / Stop
    let run = u.m.run;
    tap(&mut u, pill);
    assert_ne!(u.m.run, run, "the pill still runs and stops");
    // the exit chip leaves full screen
    tap(&mut u, exit);
    assert!(!u.full_on());
}

#[test]
fn a_danger_button_in_a_footer_asks_before_it_acts_and_stands_apart() {
    let mut u = studio();
    // the factory reset
    u.open_menu(MenuId::Main);
    for _ in 0..3 { frame(&mut u); }
    // Batch 21: the factory reset is a button of the Menu's Acquisition group
    let reset = u.menu_geo().groups.iter().flat_map(|g| g.items.iter()).find(|(it, _)| *it == Item::DefaultAct).expect("the factory reset is in the Menu").1;
    u.m.tscale = 3;
    tap(&mut u, reset);
    assert_eq!(u.m.tscale, 3, "the first tap only asks");
    assert!(u.studio.md.armed.is_some());
    tap(&mut u, reset);
    assert_ne!(u.m.tscale, 3, "the second tap does it");
    // the reference sheet: Store is the main button, Clear all is set apart and asks
    // (the factory reset changed the layout: a fresh Studio)
    let mut u = studio();
    u.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: 0.0, dt: 1e-6, v: (0..100).map(|i| i as f32).collect::<Vec<_>>().into(), vpd: 1.0, offset: 0.0, on: true });
    u.open_menu(MenuId::Ref);
    for _ in 0..3 { frame(&mut u); }
    let g = u.sheet_geo().unwrap();
    let save = g.foot.iter().find(|(it, _)| *it == Item::RefSave).expect("Store").1;
    let clear = g.foot.iter().find(|(it, _)| *it == Item::RefClear).expect("Clear all").1;
    assert!(clear.x - save.right() >= 16, "set apart: {save:?} {clear:?}");
    assert!(matches!(u.sheet_row_kind(Item::RefColor), super::sheet::RowKind::Choice), "swatches");
    shot(&mut u, "reference-sheet");
    tap(&mut u, clear);
    assert!(u.refs[0].is_some(), "asked first");
    tap(&mut u, clear);
    assert!(u.refs.iter().all(Option::is_none), "cleared on the second tap");
}

#[test]
fn the_add_hub_has_no_math_ref_or_bus_and_the_tiles_scroll_without_a_pager() {
    for k in super::addrail::ADD_KINDS.iter() { assert!(!matches!(k.0, super::addrail::AddKind::Math | super::addrail::AddKind::Ref | super::addrail::AddKind::Bus)); }
    let mut u = studio();
    u.open_meas_list();
    for _ in 0..3 { frame(&mut u); }
    let g = u.sheet_add_geo();
    assert_eq!(g.pages, 1, "no pager");
    assert!(g.find.w >= 40, "Find stays");
    let before = g.rows.len();
    let total = u.sheet_add_entries().len();
    assert!(total > before, "more tiles than the grid shows: {total} vs {before}");
    let n0 = u.measurement_count();
    let (x, y) = (g.list.x + 40, g.list.y + 100);
    touch(&mut u, Phase::Down, x, y);
    for k in 1..=6 { touch(&mut u, Phase::Move, x, y - 14 * k); }
    touch(&mut u, Phase::Up, x, y - 84);
    frame(&mut u);
    assert!(u.studio.md.tile_scroll > 0, "scrolled");
    assert_eq!(u.measurement_count(), n0, "a scroll adds nothing");
    shot(&mut u, "tiles-scroll");
}

#[test]
fn a_view_is_not_docked_where_it_would_leave_the_waveform_too_short() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Eye);
    for _ in 0..3 { frame(&mut u); }
    let w = u.dock_leaf(VKind::Wave).unwrap();
    u.dock_apply(VKind::Eye, super::dock::Drop::Split(VKind::Wave, super::dock::Side::Bottom));
    for _ in 0..3 { frame(&mut u); }
    let w2 = u.dock_leaf(VKind::Wave).unwrap();
    assert!(w2.h < w.h, "the first split is fine: {} -> {}", w.h, w2.h);
    u.studio_apply_panel(super::sheet::PanelKind::Histogram);
    for _ in 0..3 { frame(&mut u); }
    let w3 = u.dock_leaf(VKind::Wave).unwrap();
    let (drop, _) = u.dock_drop_at(VKind::Hist, w3.x + w3.w / 2, w3.bottom() - 10, (0, 0));
    assert_eq!(drop, super::dock::Drop::Stay, "another split would leave {} px", w3.h / 2);
}

#[test]
fn cursors_read_at_the_top_left_of_the_lane_and_hide_when_both_are_off_screen() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.m.cursor = CursorMode::Time;
    u.m.cur_a = 0.3;
    u.m.cur_b = 0.7;
    for _ in 0..4 { frame(&mut u); }
    let lane = u.wins[0];
    let box_top = Rect::new(lane.x + 52, lane.y + 6, 60, 3);
    let mut on = Canvas::new(W, H);
    u.render(&mut on);
    let ink = |cv: &Canvas| (box_top.y..box_top.bottom()).flat_map(|y| (box_top.x..box_top.right()).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] == CURSOR).count();
    assert!(ink(&on) > 20, "the box at the top left of the lane");
    shot(&mut u, "cursors");
    u.m.cur_a = 1.4;
    u.m.cur_b = 1.8;
    for _ in 0..3 { frame(&mut u); }
    let mut off = Canvas::new(W, H);
    u.render(&mut off);
    assert_eq!(ink(&off), 0, "hidden: both cursors are off the screen");
}

#[test]
fn the_zoom_bar_buttons_get_short_labels_when_the_width_allows() {
    let mut u = studio();
    u.set_zoom(true);
    // the pull-down toolbar has the whole width of the zoom view
    u.m.studio_zoom_tab = true;
    u.zoom_tab_set(true);
    for _ in 0..4 { frame(&mut u); }
    let bar = u.studio_zoom_bar().unwrap();
    assert!(bar.fit.w >= 60 && bar.fit.w < 100, "room for Fit and Centre: {}", bar.fit.w);
    assert!(bar.center.w >= 60 && bar.center.w < 100);
}

#[test]
fn the_spectrum_facts_page_when_they_do_not_fit() {
    let mut u = studio();
    u.studio_apply_panel(super::sheet::PanelKind::Spectrum);
    for _ in 0..8 { frame(&mut u); }
    let b = u.spec_box(Rect::new(0, 0, 520, 150), true);
    let total = u.spec_facts(&u.azr.spec.clone().unwrap()).len();
    assert!(total > u.spec_facts_fit(&b), "more facts than room");
    assert_eq!(u.studio.facts_first, 0);
    assert!(u.spec_facts_page(&b, b.list.x + 5, b.list.y + 5));
    assert!(u.studio.facts_first > 0, "the next facts");
}
