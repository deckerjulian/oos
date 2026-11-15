// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 8: the Add panel, the one-tap cursors with readouts, Search in Studio, Studio's entry rules.
//! OOS_SHOTS=<dir> writes pictures (b8-*).
use super::addrail::AddKind;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..10 { u.step(0.05); }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..6 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(std::path::Path::new(&dir).join(format!("b8-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn tap_in(u: &mut Ui, r: Rect) { u.tap(r.x + r.w / 2, r.y + r.h / 2); }

fn px(cv: &Canvas, x: i32, y: i32) -> u32 { cv.px[(y * W + x) as usize] }

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
    u.step(0.01);
}

fn drag(u: &mut Ui, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) {
    touch(u, Phase::Down, x0, y0);
    for k in 1..=6 { touch(u, Phase::Move, x0 + (x1 - x0) * k / 6, y0 + (y1 - y0) * k / 6); }
    touch(u, Phase::Up, x1, y1);
}

fn open_add(u: &mut Ui) {
    let r = u.studio_top().add;
    assert!(r.w >= 40 && r.h >= 30, "the Add button is a target: {r:?}");
    if !(u.menu_open && u.menu == MenuId::AddPanel) { tap_in(u, r); }
    assert!(u.menu_open && u.menu == MenuId::AddPanel);
}

fn open_kind(u: &mut Ui, k: AddKind) {
    open_add(u);
    if u.sheet.add.kind.is_some() {
        let b = u.sheet_chrome().1.expect("Back on a detail page");
        tap_in(u, b);
    }
    let (_, r) = u.rail_geo().rows.into_iter().find(|(kind, _)| *kind == k).unwrap();
    tap_in(u, r);
}

fn foot(u: &Ui, act: super::addrail::RailAct) -> Rect {
    u.rail_geo().foot.into_iter().find(|(a, _)| *a == act).unwrap_or_else(|| panic!("{act:?} in the footer")).1
}

// ------------------------------------------------------------------------------------------------ item 4

#[test]
fn entering_studio_by_any_road_shows_its_bars() {
    // 1. the layout set straight on the model while the screen is full
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Default;
    u.m.density = Density::Full;
    u.apply_density();
    for _ in 0..3 { u.step(0.05); }
    assert!(u.full_on());
    u.m.layout = Layout::Studio;
    let mut cv = Canvas::new(W, H);
    for _ in 0..3 { u.step(0.05); u.render(&mut cv); }
    assert!(!u.full_on() && u.m.density == Density::Scope, "bars are back");
    // 2. a recalled set-up that is Studio with the screen full
    let mut v = Ui::new(Source::Demo { t: 0.0 });
    v.m.layout = Layout::Studio;
    v.studio.enter = true;
    for _ in 0..3 { v.step(0.05); }
    v.m.density = Density::Full;
    v.apply_density();
    let text = crate::store::setup_text(&v.m);
    let dir = std::env::temp_dir().join("oos-b8-recall");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("setup0.txt"), text).unwrap();
    let mut w = Ui::new(Source::Demo { t: 0.0 });
    w.m.layout = Layout::Default;
    for _ in 0..3 { w.step(0.05); }
    w.store_dir = dir.clone();
    w.store_slot = 0;
    w.recall_setup();
    for _ in 0..3 { w.step(0.05); w.render(&mut cv); }
    assert_eq!(w.m.layout, Layout::Studio);
    assert!(!w.full_on(), "a recalled Studio set-up shows its bars");
    // the full-screen button inside Studio still works: it is the user's choice
    w.toggle_expand();
    w.step(0.05);
    assert!(w.full_on());
}

// ------------------------------------------------------------------------------------------------ item 1

#[test]
fn the_add_button_opens_a_panel_that_stays_open_for_several_adds() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    let before = u.plot_rect();
    open_add(&mut u);
    assert!(u.sheet_active());
    let g = u.rail_geo();
    assert_eq!(g.rows.len(), 5);
    for (i, (_, r)) in g.rows.iter().enumerate() { assert!(r.h >= 44 && r.w >= 40, "row {i}"); }
    u.step(0.05);
    // the panel floats over the docks: the waveform keeps its size
    assert_eq!(before.w, u.plot_rect().w);
    shot(&mut u, "add-rail");
    // Cursors, then Search, then a table, then a plot: the panel is still there
    open_kind(&mut u, AddKind::Cursors);
    assert_eq!(u.sheet.add.kind, Some(AddKind::Cursors));
    shot(&mut u, "add-cursors-page");
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    assert_eq!(u.m.cursor, CursorMode::Track);
    assert!(u.menu_open && u.menu == MenuId::AddPanel, "stays open");
    open_kind(&mut u, AddKind::Search);
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    assert!(u.studio.search.is_some());
    assert!(u.menu_open && u.menu == MenuId::AddPanel);
    shot(&mut u, "add-search-page");
    open_kind(&mut u, AddKind::Table);
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    assert!(u.panel_shown(super::sheet::PanelKind::Measurements));
    assert!(u.menu_open, "still open after the third add");
    shot(&mut u, "add-table-page");
    open_kind(&mut u, AddKind::Plot);
    let g = u.rail_geo();
    assert_eq!(g.types.len(), 5, "XY, Eye, Histogram, Spectrum, Trend");
    tap_in(&mut u, g.types[2].1);
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    assert!(u.panel_shown(super::sheet::PanelKind::Histogram));
    assert!(u.menu_open);
    shot(&mut u, "add-plot-page");
}

#[test]
fn every_page_has_a_picture_a_line_and_targets_of_40_px() {
    let mut u = studio();
    for k in [AddKind::Cursors, AddKind::Search, AddKind::Table, AddKind::Plot] {
        open_kind(&mut u, k);
        let g = u.rail_geo();
        let sheet = u.sheet_rect();
        assert!(g.preview.w > 200 && g.preview.h >= 80, "{k:?} picture");
        assert!(g.desc.h >= 32);
        for (_, r) in g.types.iter() { assert!(r.h >= 40 && r.w >= 40, "{k:?} type"); }
        for (_, r) in g.srcs.iter() { assert!(r.h >= 40 && r.w >= 40, "{k:?} source"); }
        for (_, r) in g.foot.iter() { assert!(r.h >= 44 && r.bottom() <= sheet.bottom(), "{k:?} footer"); }
        for r in g.types.iter().map(|t| t.1).chain(g.srcs.iter().map(|t| t.1)).chain(std::iter::once(g.preview)) {
            assert!(r.x >= sheet.x && r.right() <= sheet.right() && r.bottom() <= sheet.bottom(), "{k:?} inside");
        }
        let ids: Vec<&str> = if k == AddKind::Table { vec!["table_meas", "table_search", "table_decode"] } else if k == AddKind::Plot { vec!["xy", "eye", "hist", "spectrum", "trend"] } else { vec![if k == AddKind::Search { "search" } else { "cursors" }] };
        for id in ids {
            let mut cv = Canvas::new(W, H);
            cv.clear(BG);
            let r = Rect::new(10, 10, 356, 88);
            u.draw_rail_preview(&mut cv, r, id);
            let lit = (r.y..r.bottom()).any(|y| (r.x + 4..r.right() - 4).any(|x| px(&cv, x, y) != px(&cv, r.x + 2, r.y + 2) && px(&cv, x, y) != px(&cv, r.x + 1, r.y + 1)));
            assert!(lit, "{id} draws something");
        }
    }
}

#[test]
fn measure_math_and_ref_open_the_pages_they_have_and_back_returns() {
    let mut u = studio();
    open_kind(&mut u, AddKind::Measure);
    // Measure is a page (picture, line, Choose); Choose opens the list
    let choose = foot(&u, super::addrail::RailAct::Choose);
    tap_in(&mut u, choose);
    assert_eq!(u.menu, MenuId::MeasAdd);
    assert!(u.sheet_active());
    let b = u.sheet_chrome().1.expect("back");
    tap_in(&mut u, b);
    assert_eq!(u.menu, MenuId::AddPanel);
    // math and ref are added from the channel popup now, not from this hub
    assert!(u.rail_geo().rows.iter().all(|(k, _)| *k != AddKind::Math && *k != AddKind::Ref));
}

#[test]
fn the_results_table_adds_measurements_search_events_and_decode_needs_a_decoder() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    open_kind(&mut u, AddKind::Table);
    let g = u.rail_geo();
    assert_eq!(g.types.len(), 3);
    // search events: makes Search 1 when there is none, then lists its events
    tap_in(&mut u, g.types[1].1);
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    assert!(u.studio.search.is_some());
    assert_eq!(u.table, Some(tables::TableKind::Search));
    assert_eq!(u.table_rows(tables::TableKind::Search).len(), u.studio_search_events().len());
    assert!(!u.table_rows(tables::TableKind::Search).is_empty());
    // decode without a decoder says so and adds nothing
    let g = u.rail_geo();
    tap_in(&mut u, g.types[2].1);
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    assert_ne!(u.table, Some(tables::TableKind::Decode));
    // Remove takes the table away again
    let g = u.rail_geo();
    tap_in(&mut u, g.types[1].1);
    let rm = foot(&u, super::addrail::RailAct::Remove);
    tap_in(&mut u, rm);
    assert!(u.table.is_none());
}

// ------------------------------------------------------------------------------------------------ item 2

#[test]
fn one_tap_on_cursors_puts_two_waveform_cursors_on_the_selected_trace() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    assert_eq!(u.m.cursor, CursorMode::Off);
    let src = u.cursor_source();
    let r = u.studio_top().cursors;
    tap_in(&mut u, r);
    assert_eq!(u.m.cursor, CursorMode::Track, "waveform cursors");
    assert!(!u.menu_open, "no mode picker first");
    assert_eq!(u.cursor_source(), src);
    let (ta, tb) = u.cursor_times();
    assert!(tb > ta);
    shot(&mut u, "cursors-one-tap");
    // a second tap opens the settings
    tap_in(&mut u, r);
    assert!(u.menu_open && u.menu == MenuId::Cursor && u.sheet_active());
    shot(&mut u, "cursors-settings");
}

#[test]
fn cursor_readouts_have_a_box_at_each_cursor_and_a_delta_box() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.studio_cursors_add(None);
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let a = u.area;
    // the delta box is a CURSOR-coloured frame near the bottom of the lane, between the cursors
    let (ta, tb) = u.cursor_times();
    assert!(u.wave_at_source(u.cursor_source(), ta).is_some() && u.wave_at_source(u.cursor_source(), tb).is_some());
    let mut found = 0;
    for y in a.y..a.bottom() { for x in a.x..a.right() { if px(&cv, x, y) == CURSOR { found += 1; } } }
    assert!(found > 400, "cursor lines and boxes are drawn ({found} px)");
    // in badge mode the boxes leave the graticule and the chip carries the values
    let before = found;
    u.studio.cur_badge = true;
    u.render(&mut cv);
    let mut after = 0;
    for y in a.y..a.bottom() { for x in a.x..a.right() { if px(&cv, x, y) == CURSOR { after += 1; } } }
    assert!(after < before, "boxes gone from the graticule");
    let chip = u.studio_cursor_chip(0).expect("a badge chip").1;
    assert!(chip.contains('X') || chip.contains("t"), "{chip}");
    u.studio.cur_badge = false;
    shot(&mut u, "cursor-readouts");
}

#[test]
fn cursors_show_in_the_overview_and_in_the_zoom_view() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.set_zoom(true);
    for _ in 0..4 { u.step(0.05); }
    u.studio_cursors_add(None);
    // put the cursors where the zoom window is, so the overview and the zoom view both hold them
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let (o, _) = u.split_zoom(u.grid);
    let (ta, tb) = u.cursor_times();
    let tdiv = u.m.time_per_div();
    let x_of = |t: f64| o.x + (o.w as f32 / 2.0 + (t as f32 - u.m.delay) / tdiv * (o.w as f32 / HDIV)) as i32;
    let mut seen = 0;
    for t in [ta, tb] {
        let x = x_of(t);
        if x < o.x || x >= o.right() { continue; }
        let hits = (o.y..o.bottom()).filter(|&y| px(&cv, x, y) == CURSOR).count();
        if hits > 8 { seen += 1; }
    }
    assert!(seen >= 1, "overview shows a cursor line ({ta} {tb} in {o:?})");
    // and the zoom view has its own lines at the fractions
    let lane = u.area;
    let zx = lane.x + (u.m.cur_a * lane.w as f32) as i32;
    let hits = (lane.y..lane.bottom()).filter(|&y| px(&cv, zx, y) == CURSOR).count();
    assert!(hits > 8, "zoom view shows cursor A");
    shot(&mut u, "cursors-overview-and-zoom");
}

#[test]
fn cursor_settings_type_source_readout_linked_and_positions() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.studio_cursors_add(None);
    u.open_menu(MenuId::Cursor);
    assert!(u.sheet_active());
    let spec = u.sheet_spec(MenuId::Cursor).unwrap();
    assert!(spec.tabs[0].rows.contains(&Item::CurType) && spec.tabs[0].rows.contains(&Item::CurSrc) && spec.tabs[0].rows.contains(&Item::CurReadout) && spec.tabs[0].rows.contains(&Item::CurLink));
    assert!(spec.tabs[1].rows.contains(&Item::CurA) && spec.tabs[1].rows.contains(&Item::CurB));
    assert_eq!(u.value(Item::CurType), "Waveform");
    for (name, mode) in [("V bars", CursorMode::Time), ("H bars", CursorMode::Volt), ("V and H", CursorMode::Xy), ("Waveform", CursorMode::Track)] {
        u.select_option(Item::CurType, name);
        assert_eq!(u.m.cursor, mode, "{name}");
        assert_eq!(u.value(Item::CurType), name);
        let spec = u.sheet_spec(MenuId::Cursor).unwrap();
        assert!(spec.tabs.iter().all(|t| t.rows.len() <= 7), "{name} fits the sheet");
    }
    // source: a channel; second source: split
    u.select_option(Item::CurSrc, "CH2");
    assert_eq!(u.cursor_source(), WaveId::Ch(1));
    u.select_option(Item::CurSrcB, "CH1");
    assert_eq!(u.cursor_b_source(), WaveId::Ch(0));
    u.select_option(Item::CurSrcB, "Same");
    assert_eq!(u.m.cur_b_source, None);
    // readout place, linked
    u.select_option(Item::CurReadout, "Badge");
    assert!(u.studio.cur_badge);
    u.select_option(Item::CurLink, "Linked");
    assert!(u.studio.cur_linked);
    // position fields step the cursors
    let a = u.m.cur_a;
    u.adjust(Item::CurA, 4);
    assert!(u.m.cur_a > a);
    // remove
    u.activate(Item::CurRemove);
    assert_eq!(u.m.cursor, CursorMode::Off);
    assert!(!u.menu_open);
}

#[test]
fn linked_cursors_move_together_and_independent_ones_do_not() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.studio_cursors_add(None);
    let lane = u.area;
    let x = lane.x + (u.m.cur_a * lane.w as f32) as i32;
    let y = lane.y + lane.h / 2;
    let (a0, b0) = (u.m.cur_a, u.m.cur_b);
    drag(&mut u, (x, y), (x + 40, y));
    assert!(u.m.cur_a > a0 + 0.02, "A moved");
    assert!((u.m.cur_b - b0).abs() < 1e-6, "B stays: independent");
    u.studio.cur_linked = true;
    let (a1, b1) = (u.m.cur_a, u.m.cur_b);
    let x = lane.x + (u.m.cur_a * lane.w as f32) as i32;
    drag(&mut u, (x, y), (x + 40, y));
    assert!(u.m.cur_a > a1 + 0.02 && u.m.cur_b > b1 + 0.02, "both moved");
    assert!(((u.m.cur_b - u.m.cur_a) - (b1 - a1)).abs() < 1e-4, "the distance is kept");
}

// ------------------------------------------------------------------------------------------------ item 3

#[test]
fn add_search_makes_an_edge_search_with_level_at_50_percent_and_marks() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    open_kind(&mut u, AddKind::Search);
    let add = foot(&u, super::addrail::RailAct::Add);
    tap_in(&mut u, add);
    let sp = u.studio.search.expect("Search 1");
    assert_eq!(sp.src, u.cursor_source());
    let (w, _, _) = u.deskew_samples(sp.src).unwrap();
    let (lo, hi) = w.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    assert!((sp.level - (lo + hi) / 2.0).abs() < 1e-4, "level is the middle of the swing");
    let ev = u.studio_search_events();
    assert!(!ev.is_empty(), "events");
    assert!(ev.windows(2).all(|p| p[0] < p[1]), "in time order");
    // the badge says how many
    let (_, label, _, text) = u.studio_search_chip().expect("badge");
    assert!(text.contains(&format!("{} event", ev.len())), "{text}");
    assert!(label.h >= 40);
    // the marks are on the waveform: amber pixels at the top of the plot
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    let a = u.area;
    let amber = (a.x..a.right()).filter(|&x| (a.y..a.y + 10).any(|y| px(&cv, x, y) == AMBER)).count();
    assert!(amber > 0, "marks");
    shot(&mut u, "search-added");
}

#[test]
fn search_slope_selects_the_edges_and_level_moves_them() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.studio_search_add(Some(WaveId::Ch(0)));
    assert_eq!(u.studio.search.unwrap().slope, 2, "every edge by default");
    let either = u.studio_search_events().len();
    u.select_option(Item::SrchSlope, "Rising");
    let rising = u.studio_search_events().len();
    u.select_option(Item::SrchSlope, "Falling");
    let falling = u.studio_search_events().len();
    assert!(rising > 0 && falling > 0);
    assert!((rising as i64 - falling as i64).abs() <= 1);
    u.select_option(Item::SrchSlope, "Either");
    assert_eq!(u.studio_search_events().len(), either);
    assert!(either >= rising + falling - 1);
    // a level above the signal finds nothing
    let top = u.studio.search.unwrap().level * 100.0 + 100.0;
    u.studio.search = Some(super::addrail::SearchSpec { level: top, ..u.studio.search.unwrap() });
    assert!(u.studio_search_events().is_empty());
    // Set to 50 % brings it back
    u.activate(Item::Srch50);
    assert_eq!(u.studio_search_events().len(), either);
}

#[test]
fn next_and_previous_event_walk_the_events() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.studio_search_add(Some(WaveId::Ch(0)));
    let ev = u.studio_search_events();
    assert!(ev.len() >= 2);
    let (_, _, next, _) = u.studio_search_chip().unwrap();
    assert!(next.w >= 40 && next.h >= 40);
    let d0 = u.m.delay;
    tap_in(&mut u, next);
    assert_eq!(u.studio.search_at, 1 % ev.len());
    assert!((u.m.delay as f64 - ev[u.studio.search_at]).abs() < 1e-6, "the view is on the event");
    assert!(u.m.delay != d0 || ev[0] == d0 as f64);
    let (prev, _, _, _) = u.studio_search_chip().unwrap();
    assert!(prev.w >= 40 && prev.h >= 40);
    tap_in(&mut u, prev);
    assert_eq!(u.studio.search_at, 0);
    // the Event row steps the same way
    u.adjust(Item::SrchEvent, 1);
    assert_eq!(u.studio.search_at, 1);
    assert!(u.value(Item::SrchEvent).starts_with("2 / "));
}

#[test]
fn search_copies_to_and_from_the_trigger_and_has_its_settings_page() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.studio_search_add(Some(WaveId::Ch(0)));
    // to the trigger
    u.select_option(Item::SrchSlope, "Falling");
    u.activate(Item::SrchToTrig);
    assert_eq!(u.m.trig_type, TrigType::Edge);
    assert_eq!(u.m.slope, Slope::Fall);
    assert_eq!(u.m.trig_level, u.studio.search.unwrap().level);
    // from the trigger
    u.m.trig_level = 0.25;
    u.m.slope = Slope::Rise;
    u.activate(Item::SrchFromTrig);
    let sp = u.studio.search.unwrap();
    assert_eq!((sp.level, sp.slope), (0.25, 0));
    // the page
    u.open_menu(MenuId::Search);
    assert!(u.sheet_active());
    let spec = u.sheet_spec(MenuId::Search).unwrap();
    assert_eq!(spec.tabs.len(), 1);
    assert!(spec.tabs[0].rows.len() <= 7 && spec.tabs[0].foot.len() <= 2);
    shot(&mut u, "search-settings");
    u.activate(Item::SrchRemove);
    assert!(u.studio.search.is_none() && !u.menu_open);
    assert!(u.studio_search_chip().is_none());
}

#[test]
fn the_search_badge_does_not_push_the_measurement_badges_out_of_the_row() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    let n = u.studio_badges().len();
    u.studio_search_add(Some(WaveId::Ch(0)));
    let (_, _, next, _) = u.studio_search_chip().unwrap();
    for (_, r) in u.studio_badges() { assert!(r.x >= next.right(), "badges start after the search badge"); }
    assert!(u.studio_badges().len() <= n);
}

// ------------------------------------------------------------------------------------------------ item 5

#[test]
fn a_floating_panel_stays_off_the_zoom_bar_and_the_docked_panel_closes_with_an_x() {
    let mut u = studio();
    u.m.run = RunState::Stop;
    u.set_zoom(true);
    for _ in 0..4 { u.step(0.05); }
    u.studio_set_view(super::studio::StudioView::Floating);
    for at in [(600, 0), (700, 60), (300, 100), (0, 0)] {
        u.studio.panel.at = Some(at);
        u.relayout();
        let p = u.studio_panel_rect().expect("floating panel");
        let bar = u.zoom_readout();
        assert!(p.right() <= bar.x || p.x >= bar.right() || p.bottom() <= bar.y || p.y >= bar.bottom(), "{at:?}: {p:?} covers the zoom bar {bar:?}");
    }
    shot(&mut u, "floating-clear-of-zoom-bar");
    u.studio_set_view(super::studio::StudioView::Docked);
    u.step(0.05);
    let close = Ui::dock_close_rect(u.dock_leaf(super::dock::VKind::Measure).expect("docked"));
    assert!(close.w >= 40 && close.h >= 40);
    u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: close.x + 24, y: close.y + 20 });
    u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: close.x + 24, y: close.y + 20 });
    assert_eq!(u.studio.view, super::studio::StudioView::Waveform, "the X closes the panel");
}
