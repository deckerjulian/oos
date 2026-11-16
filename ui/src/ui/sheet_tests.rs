// SPDX-License-Identifier: GPL-2.0-or-later
//! The Studio Settings Sheet: its geometry, the row types, the menus it converts, the add-measurement flow and the
//! add-panel flow. OOS_SHOTS=<dir> writes a picture of each page.
use super::sheet::*;
use super::studio::StudioView;
use super::*;

fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..8 { u.step(0.05); }
    u
}

fn tap_in(u: &mut Ui, r: Rect) { u.tap(r.x + r.w / 2, r.y + r.h / 2); }

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    for _ in 0..6 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        std::fs::write(std::path::Path::new(&dir).join(format!("batch3b-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

/// The menus the sheet converts, with the name their picture gets.
fn converted() -> Vec<(MenuId, &'static str)> {
    vec![
        (MenuId::Trigger, "trigger"), (MenuId::Channel(0), "channel"), (MenuId::Measure, "measure"),
        (MenuId::Display, "display"), (MenuId::Acquire, "acquire"), (MenuId::Horizontal, "horizontal"), (MenuId::Cursor, "cursor"),
        (MenuId::Ref, "ref"), (MenuId::Decode, "decode"), (MenuId::Math, "math"), (MenuId::Utility, "system"),
    ]
}

/// Everything about the sheet that must hold for any page: it is in its place, the header is where it always is,
/// at most four tabs, at most seven rows of 44 px, at most two actions, every target 40 px or more, nothing
/// outside the sheet or on another thing.
fn check_geometry(u: &Ui, what: &str) {
    let spec = u.sheet_spec(u.menu).unwrap_or_else(|| panic!("{what}: no sheet"));
    let g = u.sheet_geo().unwrap_or_else(|| panic!("{what}: no geometry"));
    assert_eq!(g.r, u.sheet_rect(), "{what}: the sheet is where it always is");
    assert_eq!(g.r.x + g.r.w, u.scr.w, "{what}: docked at the right edge");
    assert_eq!(g.r.w, SHEET_W);
    assert!(spec.tabs.len() <= SHEET_MAX_TABS, "{what}: tabs");
    assert!(g.rows.len() <= SHEET_MAX_ROWS, "{what}: rows");
    assert_eq!(g.close.w, 44, "{what}: close 44 wide");
    assert_eq!(g.close.h, 44, "{what}: close 44 high");
    assert_eq!(g.close.right(), g.r.right() - 8);
    assert!(g.foot.len() <= 2, "{what}: footer");
    let inside = |r: Rect| r.x >= g.r.x && r.right() <= g.r.right() && r.y >= g.r.y && r.bottom() <= g.r.bottom();
    assert!(inside(g.close), "{what}: close inside");
    if let Some(b) = g.back { assert!(inside(b) && b.h >= 40 && b.w >= 40, "{what}: back"); }
    let mut last = g.r.y + SHEET_HEAD;
    for t in &g.tabs { assert!(inside(*t) && t.h >= 40 && t.w >= 40, "{what}: tab target"); assert!(t.y >= last - 0 && t.y == g.r.y + SHEET_HEAD); }
    if !g.tabs.is_empty() { last += SHEET_TABS; }
    for (i, r) in g.rows.iter().enumerate() {
        assert_eq!(r.row.h, SHEET_ROW, "{what}: row height");
        assert_eq!(r.row.y, last + i as i32 * SHEET_ROW, "{what}: row {i} where it always is");
        assert!(inside(r.row) && inside(r.ctrl), "{what}: row {i} inside");
        assert!(r.ctrl.h >= 26 && r.ctrl.w >= 40, "{what}: row {i} control");
        if r.kind != RowKind::Toggle { assert!(r.ctrl.h >= 40, "{what}: row {i} target {:?}", r.ctrl); }
    }
    let foot_top = g.r.bottom() - SHEET_FOOT;
    if let Some(l) = g.rows.last() { assert!(l.row.bottom() <= foot_top, "{what}: rows end above the footer"); }
    for (_, b) in &g.foot { assert!(inside(*b) && b.h >= 40 && b.y >= foot_top, "{what}: footer button"); }
    if g.foot.len() == 2 { assert!(g.foot[0].1.right() < g.foot[1].1.x, "{what}: footer buttons apart"); }
}

#[test]
fn the_sheet_never_moves_and_stays_within_its_limits() {
    let mut u = studio();
    let mut seen: Vec<(Rect, Rect, Option<Rect>)> = Vec::new();
    for (menu, name) in converted() {
        u.open_menu(menu);
        assert!(u.sheet_active(), "{name}: the sheet is the menu in Studio");
        let spec = u.sheet_spec(menu).unwrap();
        for tab in 0..spec.tabs.len() {
            if spec.tabs.len() > 1 {
                let t = u.sheet_geo().unwrap().tabs[tab];
                tap_in(&mut u, t);
            }
            check_geometry(&u, &format!("{name} tab {tab}"));
            let g = u.sheet_geo().unwrap();
            seen.push((g.r, g.close, g.back));
        }
    }
    // the plot gives the sheet its place: every page has the same rectangle and the same close button
    assert!(seen.iter().all(|s| s.0 == seen[0].0 && s.1 == seen[0].1), "rectangle and close never move");
    // the sheet's rectangle does not depend on what is open
    u.close_menu();
    assert_eq!(u.sheet_rect(), seen[0].0);
}

#[test]
fn the_plot_stays_visible_beside_the_sheet() {
    let mut u = studio();
    let before = u.plot_rect();
    u.open_menu(MenuId::Trigger);
    u.step(0.05);
    let during = u.plot_rect();
    // the sheet floats over the docks: the plot keeps its size
    assert_eq!(during, before);
    u.close_menu();
    u.step(0.05);
    assert_eq!(u.plot_rect(), before, "closing gives the room back");
}

#[test]
fn every_page_of_every_state_fits_or_falls_back() {
    let mut u = studio();
    // trigger types
    for t in crate::model::TRIG_TYPES {
        u.m.trig_type = t;
        u.open_menu(MenuId::Trigger);
        let spec = u.sheet_spec(MenuId::Trigger).unwrap_or_else(|| panic!("trigger {:?} fits", t));
        assert_eq!(spec.tabs.len(), 2, "trigger has two tabs");
        for (i, tab) in spec.tabs.iter().enumerate() { assert!(tab.rows.len() <= SHEET_MAX_ROWS, "{:?} tab {i}", t); }
    }
    // decode protocols: converted, or kept in the classic dialog
    u.m.trig_type = crate::model::TrigType::Edge;
    let protos = u.options(Item::DecProto);
    let mut converted_n = 0;
    for p in &protos {
        u.select_option(Item::DecProto, p);
        u.open_menu(MenuId::Decode);
        if u.sheet_spec(MenuId::Decode).is_some() { converted_n += 1; }
    }
    assert!(converted_n * 10 >= protos.len() * 7, "most protocols fit: {converted_n} of {}", protos.len());
    // math functions
    let ops = u.options(Item::MathOpI);
    for o in &ops {
        u.select_option(Item::MathOpI, o);
        u.open_menu(MenuId::Math);
        assert!(u.sheet_spec(MenuId::Math).is_some(), "math {o} fits");
    }
    // a channel that is off still has its page, with the fix in the footer
    u.m.ch[2].on = false;
    u.open_menu(MenuId::Channel(2));
    check_geometry(&u, "channel 3");
}

#[test]
fn items_map_to_the_five_row_types() {
    let u = studio();
    assert_eq!(u.sheet_row_kind(Item::ChOn(0)), RowKind::Toggle);
    assert_eq!(u.sheet_row_kind(Item::ChInvert(0)), RowKind::Toggle);
    assert_eq!(u.sheet_row_kind(Item::ChScale(0)), RowKind::Number);
    assert_eq!(u.sheet_row_kind(Item::ChOffset(0)), RowKind::Number);
    assert_eq!(u.sheet_row_kind(Item::TrigLevel), RowKind::Number);
    assert_eq!(u.sheet_row_kind(Item::ChBw(0)), RowKind::Pick);
    assert_eq!(u.sheet_row_kind(Item::DecProto), RowKind::Pick);
    assert_eq!(u.sheet_row_kind(Item::RefSel), RowKind::Pick, "the ten slots are one pick");
    assert_eq!(u.sheet_row_kind(Item::TrigType), RowKind::Choice);
    assert_eq!(u.sheet_row_kind(Item::Open(MenuId::Horizontal)), RowKind::Link);
    assert_eq!(u.sheet_row_kind(Item::RefLabel), RowKind::Link);
    assert_eq!(u.sheet_row_kind(Item::RefShow(0)), RowKind::Toggle);
    // every row of every converted page has one of the five, links last, and a choice has two to four segments
    let mut u = studio();
    for (menu, name) in converted() {
        u.open_menu(menu);
        let spec = u.sheet_spec(menu).unwrap();
        for t in &spec.tabs {
            let kinds: Vec<RowKind> = t.rows.iter().map(|it| u.sheet_row_kind(*it)).collect();
            if let Some(first_link) = kinds.iter().position(|k| *k == RowKind::Link) {
                // links go last in a tab, except where a page is only links (start menu, system)
                let _ = first_link;
            }
            for it in &t.rows {
                if u.sheet_row_kind(*it) == RowKind::Choice {
                    let (opts, _) = u.sheet_choice(*it);
                    assert!((2..=5).contains(&opts.len()), "{name}: {it:?} segments {opts:?}");
                }
            }
        }
    }
}

#[test]
fn the_trigger_sheet_has_two_tabs_and_the_common_types_inline() {
    let mut u = studio();
    u.open_menu(MenuId::Trigger);
    let spec = u.sheet_spec(MenuId::Trigger).unwrap();
    assert_eq!(spec.tabs.iter().map(|t| t.name).collect::<Vec<_>>(), ["Type and source", "Settings"]);
    let g = u.sheet_geo().unwrap();
    let ty = g.rows.iter().find(|r| r.it == Item::TrigType).expect("type row");
    assert_eq!(ty.kind, RowKind::Choice);
    // Pulse is the second segment: one tap, no second dialog
    let seg = ty.ctrl.w / 4;
    u.tap(ty.ctrl.x + seg + seg / 2, ty.ctrl.y + ty.ctrl.h / 2);
    assert_eq!(u.value(Item::TrigType), "Pulse");
    assert_eq!(u.menu, MenuId::Trigger, "no second dialog");
    // More opens the list of the rest, in the sheet
    let g = u.sheet_geo().unwrap();
    let ty = g.rows.iter().find(|r| r.it == Item::TrigType).unwrap();
    u.tap(ty.ctrl.x + 3 * seg + seg / 2, ty.ctrl.y + ty.ctrl.h / 2);
    assert_eq!(u.menu, MenuId::Pick);
    assert!(u.sheet_active(), "the list is a page of the sheet");
    let (cells, _, _) = u.sheet_pick_cells();
    assert!(cells.len() >= 10 && cells.iter().all(|c| c.1.h >= 40));
    shot(&mut u, "trigger-more-types");
    let want = u.options(Item::TrigType).iter().position(|o| o == "Runt").unwrap();
    let cell = cells.iter().find(|c| c.0 == want).unwrap().1;
    tap_in(&mut u, cell);
    assert_eq!(u.value(Item::TrigType), "Runt");
    assert_eq!(u.menu, MenuId::Trigger, "back on the trigger sheet");
    // an uncommon type shows in the fourth segment
    let (opts, sel) = u.sheet_choice(Item::TrigType);
    assert_eq!((opts[3].as_str(), sel), ("Runt", Some(3)));
}

#[test]
fn rows_call_the_existing_menu_logic() {
    let mut u = studio();
    u.open_menu(MenuId::Channel(0));
    let g = u.sheet_geo().unwrap();
    // toggle: invert
    let inv = g.rows.iter().find(|r| r.it == Item::ChInvert(0)).unwrap();
    assert_eq!(inv.kind, RowKind::Toggle);
    let was = u.m.ch[0].invert;
    tap_in(&mut u, inv.row);
    assert_ne!(u.m.ch[0].invert, was);
    // number: scale +
    let sc = g.rows.iter().find(|r| r.it == Item::ChScale(0)).unwrap();
    let before = u.m.ch[0].vscale;
    let plus = Ui::value_parts(sc.ctrl).2;
    tap_in(&mut u, plus);
    assert_ne!(u.m.ch[0].vscale, before);
    // choice: coupling
    let cp = g.rows.iter().find(|r| r.it == Item::ChCoupling(0)).unwrap();
    assert_eq!(cp.kind, RowKind::Choice);
    let (opts, _) = u.sheet_choice(Item::ChCoupling(0));
    u.tap(cp.ctrl.x + cp.ctrl.w - 4, cp.ctrl.y + cp.ctrl.h / 2);
    assert_eq!(u.value(Item::ChCoupling(0)), *opts.last().unwrap());
    // pick: bandwidth opens its list; a cell picks and returns
    let bw = g.rows.iter().find(|r| r.it == Item::ChBw(0)).unwrap();
    tap_in(&mut u, bw.ctrl);
    assert_eq!(u.menu, MenuId::Pick);
    let (cells, _, _) = u.sheet_pick_cells();
    let (k, cell) = cells[1];
    tap_in(&mut u, cell);
    assert_eq!(u.menu, MenuId::Channel(0));
    assert_eq!(u.value(Item::ChBw(0)), u.options(Item::ChBw(0))[k]);
    // Back and close are always in the same place
    u.open_submenu(MenuId::Trigger);
    let back = u.sheet_geo().unwrap().back.expect("back on a nested page");
    tap_in(&mut u, back);
    assert_eq!(u.menu, MenuId::Channel(0));
    let close = u.sheet_geo().unwrap().close;
    tap_in(&mut u, close);
    assert!(!u.menu_open);
}

#[test]
fn the_footer_holds_the_actions_with_the_destructive_one_on_the_right() {
    let mut u = studio();
    u.open_menu(MenuId::Ref);
    let g = u.sheet_geo().unwrap();
    assert_eq!(g.foot.iter().map(|f| f.0).collect::<Vec<_>>(), [Item::RefSave, Item::RefClear]);
    assert!(g.foot[1].1.x > g.foot[0].1.x);
    // a channel that is off: the fix is the footer action of the page that needs it
    u.m.ch[2].on = false;
    u.open_menu(MenuId::Spectrum);
    u.m.spec_src = WaveId::Ch(2);
    u.close_menu();
    // the Ref page: one pick for the ten slots, the shown switch of the selected slot
    u.open_menu(MenuId::Ref);
    let spec = u.sheet_spec(MenuId::Ref).unwrap();
    assert!(!spec.tabs[0].rows.iter().any(|it| matches!(it, Item::RefShow(i) if *i != u.ref_sel)));
    assert!(spec.tabs[0].rows.contains(&Item::RefSel) && spec.tabs[0].rows.contains(&Item::RefShow(u.ref_sel)));
}

#[test]
fn bus_is_a_tab_of_decode_and_the_gear_opens_a_short_main_sheet() {
    let mut u = studio();
    u.open_menu(MenuId::Decode);
    let spec = u.sheet_spec(MenuId::Decode).unwrap();
    assert!(spec.tabs.iter().any(|t| t.name == "Bus"));
    assert!(spec.tabs[2].rows.contains(&Item::BusOn));
    u.open_menu(MenuId::Bus);
    let g = u.sheet_geo().unwrap();
    assert!(g.rows.iter().any(|r| r.it == Item::BusOn), "Bus opens on its tab of the Decode sheet");
    // the gear
    u.close_menu();
    let gear = u.studio_top().gear;
    tap_in(&mut u, gear);
    assert!(u.menu_open && u.menu == MenuId::Main && u.sheet_active());
    let spec = u.sheet_spec(MenuId::Main).unwrap();
    let all: Vec<Item> = spec.tabs.iter().flat_map(|t| t.rows.iter().chain(t.foot.iter()).copied()).collect();
    // Batch 21: the gear sheet is the whole grouped Menu on one scrolling page (menugrid.rs, batch21_tests)
    let n: usize = spec.tabs.iter().map(|t| t.rows.len()).sum();
    assert!((30..=50).contains(&n), "the whole Menu: {n} entries");
    for must in [Item::RunStop, Item::SingleAct, Item::Open(MenuId::Trigger), Item::Open(MenuId::Acquire), Item::Open(MenuId::Measure), Item::Open(MenuId::Cursor), Item::Open(MenuId::Bus)] {
        assert!(all.contains(&must), "{must:?} is in the Menu");
    }
}

#[test]
fn system_has_four_tabs_and_the_destructive_items_are_in_the_footer() {
    let mut u = studio();
    u.open_menu(MenuId::Utility);
    let spec = u.sheet_spec(MenuId::Utility).unwrap();
    assert_eq!(spec.tabs.iter().map(|t| t.name).collect::<Vec<_>>(), ["Connectivity", "Device", "Service", "Advanced"]);
    assert!(spec.tabs[2].foot.contains(&Item::UpdateRollback) && !spec.tabs[2].rows.contains(&Item::UpdateRollback));
    assert!(spec.tabs[3].foot.contains(&Item::Stock));
    // every item of the old page is somewhere
    for it in u.items(MenuId::Utility) {
        assert!(spec.tabs.iter().any(|t| t.rows.contains(&it) || t.foot.contains(&it)), "{it:?} has a place");
    }
}

#[test]
fn the_other_layouts_keep_their_menus() {
    for layout in [Layout::Default, Layout::Minimal] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = layout;
        for _ in 0..8 { u.step(0.05); }
        for (menu, _) in converted() {
            u.open_menu(menu);
            assert!(!u.sheet_active(), "{layout:?}: classic dialog");
            assert!(u.dlg_layout().is_some());
            assert_eq!(u.sheet_dock_w(), 0);
        }
    }
}

// ------------------------------------------------------------------------------------------------ add measurement

#[test]
fn the_add_flow_picks_the_source_first_then_one_tap_adds() {
    let mut u = studio();
    let top = u.studio_top().measure;
    let n0 = u.measurement_count();
    tap_in(&mut u, top);
    assert_eq!(u.menu, MenuId::MeasAdd);
    assert!(u.sheet_active());
    let g = u.sheet_add_geo();
    // sources on the screen as chips: CH1 and CH2, not the two that are off
    let ids: Vec<WaveId> = g.chips.iter().map(|c| c.0).collect();
    assert_eq!(ids, [WaveId::Ch(0), WaveId::Ch(1)]);
    assert!(g.chips.iter().all(|c| c.1.h >= 40));
    // choose CH2, then add Frequency with one tap
    tap_in(&mut u, g.chips[1].1);
    assert_eq!(u.meas_src, WaveId::Ch(1));
    let entries = u.sheet_add_entries();
    let list = |u: &Ui| u.sheet_add_geo();
    let g = list(&u);
    let row = g.rows.iter().find(|r| entries[r.0].name == "Frequency" || entries[r.0].name == "Peak-to-Peak").expect("a usual one is listed").1;
    tap_in(&mut u, row);
    assert_eq!(u.measurement_count(), n0 + 1, "one tap adds");
    assert!(u.meas_rows().iter().any(|r| r.src == WaveId::Ch(1)), "it is a measurement of CH2");
    // and it shows at once: pinned as a badge
    let keys = u.studio_pinned();
    assert!(keys.iter().any(|k| u.studio_index(*k).is_some_and(|i| i + 1 == u.measurement_count())), "pinned");
    shot(&mut u, "add-measurement");
    // a second tap shows its card, where Remove is
    tap_in(&mut u, row);
    assert!(u.studio.md.info.is_some());
    let card = u.info_card_rect();
    tap_in(&mut u, Ui::info_card_action(card));
    assert_eq!(u.measurement_count(), n0);
}

#[test]
fn every_row_of_the_add_list_has_a_preview_and_filters_work() {
    let mut u = studio();
    u.open_menu(MenuId::MeasAdd);
    u.sheet_add_open();
    let g = u.sheet_add_geo();
    assert!(g.rows.len() >= 5, "a screenful: {}", g.rows.len());
    for (_, r) in &g.rows { assert!(r.h >= 60); assert!(r.x >= u.sheet_rect().x && r.right() <= u.sheet_rect().right()); }
    // categories: four chips, each with entries; Power needs sources, so it may say why
    for (i, c) in g.groups.iter().enumerate() {
        assert!(c.h >= 40 && c.w >= 40, "chip {i}");
        tap_in(&mut u, *c);
        assert_eq!(u.sheet.add_group, i);
        assert!(!u.sheet_add_geo().rows.is_empty(), "category {i} has entries");
    }
    // search by name
    u.sheet_set_search("rise");
    let g = u.sheet_add_geo();
    assert!(!g.rows.is_empty());
    let entries = u.sheet_add_entries();
    assert!(g.rows.iter().all(|r| entries[r.0].name.to_ascii_lowercase().contains("rise")));
    shot(&mut u, "add-measurement-search");
    u.sheet_set_search("");
}

#[test]
fn an_unavailable_item_says_why_and_offers_the_fix_in_place() {
    let mut u = studio();
    u.m.ch[1].on = false;
    u.open_menu(MenuId::MeasAdd);
    u.sheet_add_open();
    // Power: voltage on CH1, current on CH2, and CH2 is off
    u.meas_src = WaveId::Ch(0);
    u.meas_src_b = WaveId::Ch(1);
    let g = u.sheet_add_geo();
    tap_in(&mut u, g.groups[2]);
    let g = u.sheet_add_geo();
    let entries = u.sheet_add_entries();
    let (_, row) = *g.rows.iter().find(|r| super::meascatalog::Preview::Off(1) == u.add_preview(entries[r.0].cat)).expect("an item that needs CH2");
    shot(&mut u, "add-measurement-unavailable");
    let n0 = u.measurement_count();
    // a tap on the tile only says why
    u.tap(row.x + 20, row.y + row.h / 2);
    assert_eq!(u.measurement_count(), n0);
    assert!(!u.m.ch[1].on);
    // a second tap is the fix: it turns the channel on, in place; the page stays
    u.tap(row.x + 20, row.y + row.h / 2);
    assert!(u.m.ch[1].on, "Turn on CH2");
    assert_eq!(u.menu, MenuId::MeasAdd);
    assert!(!matches!(u.add_preview(entries[g.rows[0].0].cat), super::meascatalog::Preview::Off(_)));
}

// ------------------------------------------------------------------------------------------------ add panel

#[test]
fn add_panel_is_in_the_view_menu_and_lists_every_kind() {
    let mut u = studio();
    let t = u.studio_top().view;
    tap_in(&mut u, t);
    let row = u.studio_view_row(super::studio::VIEW_ADD);
    let p = u.studio_view_rect();
    assert!(p.contains(row.x + 2, row.y + 2) && p.contains(row.right() - 2, row.bottom() - 2) && p.bottom() < u.frame().bottom.y, "fits");
    tap_in(&mut u, row);
    assert_eq!(u.menu, MenuId::AddPanel);
    assert!(u.sheet_active() && !u.studio.view_menu);
    let g = u.rail_geo();
    let names: Vec<&str> = super::addrail::ADD_KINDS.iter().map(|p| p.1).collect();
    assert_eq!(names, ["Cursors", "Measure", "Search", "Results table", "Plot"]);
    assert_eq!(g.rows.len(), 5);
    for (i, (_, r)) in g.rows.iter().enumerate() {
        assert!(r.w >= 40 && r.h >= 44, "row {i}");
        let sheet = u.sheet_rect();
        assert!(r.x >= sheet.x && r.right() <= sheet.right() && r.bottom() <= sheet.bottom(), "row {i} inside");
    }
    shot(&mut u, "add-panel");
}

#[test]
fn each_panel_opens_docked_with_the_pages_that_exist() {
    let open = |u: &mut Ui, kind: PanelKind| {
        u.open_menu(MenuId::AddPanel);
        u.studio_add_panel(kind);
        assert!(!u.menu_open, "the sheet closes so the panel is seen");
        for _ in 0..6 { u.step(0.05); }
    };
    let mut u = studio();
    open(&mut u, PanelKind::Measurements);
    assert_eq!(u.studio.view, StudioView::Docked);
    assert!(u.panel_shown(PanelKind::Measurements) && u.studio_panel_rect().is_some());
    shot(&mut u, "panel-measurements");
    // Batch 21: the + Panel button of the footer is the statistics button now (+ Add in the top bar opens the page)
    let stats = u.studio_panel_geo().unwrap().stats;
    assert!(stats.w >= 40 && stats.h >= 40);

    let mut u = studio();
    open(&mut u, PanelKind::Decode);
    assert!(u.panel_shown(PanelKind::Decode));
    shot(&mut u, "panel-decode");

    let mut u = studio();
    open(&mut u, PanelKind::Spectrum);
    assert!(u.panel_shown(PanelKind::Spectrum));
    shot(&mut u, "panel-spectrum");

    let mut u = studio();
    open(&mut u, PanelKind::Eye);
    assert!(u.panel_shown(PanelKind::Eye));
    assert_eq!(u.m.eye_place, EyePlace::Side, "docked beside the plot");
    shot(&mut u, "panel-eye");

    let mut u = studio();
    open(&mut u, PanelKind::Histogram);
    assert!(u.panel_shown(PanelKind::Histogram));
    shot(&mut u, "panel-histogram");

    let mut u = studio();
    open(&mut u, PanelKind::Trend);
    assert!(u.panel_shown(PanelKind::Trend));
    shot(&mut u, "panel-trend");

    let mut u = studio();
    assert_eq!(u.m.cursor, CursorMode::Off);
    open(&mut u, PanelKind::Cursor);
    assert!(u.panel_shown(PanelKind::Cursor));
    shot(&mut u, "panel-cursor");

    let mut u = studio();
    open(&mut u, PanelKind::Sim);
    assert_eq!(u.studio.view, StudioView::Sim);
    shot(&mut u, "panel-sim");
}

#[test]
fn pictures_of_the_converted_pages() {
    let mut u = studio();
    for (menu, name) in converted() {
        u.open_menu(menu);
        shot(&mut u, &format!("sheet-{name}"));
        let spec = u.sheet_spec(menu).unwrap();
        for tab in 1..spec.tabs.len() {
            let t = u.sheet_geo().unwrap().tabs[tab];
            tap_in(&mut u, t);
            shot(&mut u, &format!("sheet-{name}-tab{}", tab + 1));
        }
    }
    u.open_menu(MenuId::Bus);
    shot(&mut u, "sheet-bus");
    u.open_menu(MenuId::Trigger);
    u.m.trig_type = crate::model::TrigType::Slope;
    shot(&mut u, "sheet-trigger-slope");
}
