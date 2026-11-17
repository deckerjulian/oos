// SPDX-License-Identifier: GPL-2.0-or-later
//! Display > Layout: the stock OOS bars (Default), the compact OOS-plus bars (Minimal) and Studio.
use super::studio::{MeasKey, StudioView};
use super::*;

fn shown(layout: Layout) -> (Ui, Canvas) {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = layout;
    for _ in 0..8 {
        u.step(0.05);
    }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    // OOS_SHOTS=<dir> writes the screens for a look
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let name = format!("layout-{}.png", layout.name().to_ascii_lowercase());
        std::fs::write(std::path::Path::new(&dir).join(name), crate::store::png_of(&cv)).unwrap();
    }
    (u, cv)
}

#[test]
fn default_layout_has_the_stock_bars_and_minimal_the_compact_ones() {
    let (d, _) = shown(Layout::Default);
    let (m, _) = shown(Layout::Minimal);
    assert_eq!(d.frame(), Frame::of(d.scr));
    assert!(m.frame().bottom.h < d.frame().bottom.h, "the compact bar gives the plot more room");
    assert!(m.plot_rect().h > d.plot_rect().h);
    // stock top bar: no Single, Load or SIM button
    assert_eq!(d.single_btn().w, 0);
    let names = |u: &Ui| u.top_buttons().into_iter().map(|b| b.1).collect::<Vec<_>>();
    assert!(!names(&d).iter().any(|n| n.starts_with("Load") || *n == "SIM"));
    assert!(names(&m).iter().any(|n| *n == "SIM"));
    assert!(m.single_btn().w > 0);
}

#[test]
fn default_layout_cards_open_their_menus() {
    let (mut u, _) = shown(Layout::Default);
    let ch2 = u.cards().0[1];
    u.tap(ch2.x + ch2.w / 2, ch2.y + ch2.h / 2);
    assert!(u.m.ch[1].on, "a tap on an off channel card switches it on");
    assert_eq!(u.menu, MenuId::Channel(1));
    u.close_menu();
    // the cards are laid out again with CH2 on; the top half of TIME opens Horizontal
    let time = u.cards().1;
    u.tap(time.x + time.w / 2, time.y + 10);
    assert!(u.menu_open && u.menu == MenuId::Horizontal);
}

#[test]
fn layout_is_a_display_setting_kept_in_the_setup() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    assert_eq!(u.m.layout, Layout::Default, "a new setup starts with the stock bars");
    assert!(!u.m.dots, "samples are joined by default");
    assert!(u.items(MenuId::Display).contains(&Item::Layout));
    for layout in LAYOUTS {
        u.m.layout = layout;
        let text = crate::store::setup_text(&u.m);
        assert!(text.contains(&format!("layout = {}", layout.name().to_ascii_lowercase())));
        assert_eq!(crate::store::parse_setup(&text).unwrap().layout, layout);
    }
    // setups from before Studio hold `default` or `minimal`
    let old = crate::store::setup_text(&u.m).replace("layout = studio", "layout = minimal");
    assert_eq!(crate::store::parse_setup(&old).unwrap().layout, Layout::Minimal);
    assert!(crate::store::parse_setup(&old.replace("layout = minimal", "layout = wide")).is_err());
}

#[test]
fn the_display_menu_steps_through_the_three_layouts() {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    let seen: Vec<_> = (0..3).map(|_| { u.adjust(Item::Layout, 1); u.m.layout }).collect();
    assert_eq!(seen, [Layout::Minimal, Layout::Studio, Layout::Default]);
    assert_eq!(u.options(Item::Layout), ["Default", "Minimal", "Studio"]);
    u.select_option(Item::Layout, "Studio");
    assert_eq!(u.m.layout, Layout::Studio);
    assert_eq!(u.value(Item::Layout), "Studio");
    u.step(0.05);
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    assert!(u.stacked(), "Studio draws its sources as lanes");
}

/// Studio with CH1 and CH2 on, as the mocks show it.
fn studio() -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..8 {
        u.step(0.05);
    }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    // the maps are filled for the lanes that the first frame lays out
    u.render(&mut cv);
    for _ in 0..6 { u.step(0.05); }
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        std::fs::write(std::path::Path::new(&dir).join(format!("studio-{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

fn tap_in(u: &mut Ui, r: Rect) {
    u.tap(r.x + r.w / 2, r.y + r.h / 2);
}

#[test]
fn studio_top_bar_has_run_single_state_acquisition_and_the_menu_chips() {
    let mut u = studio();
    shot(&mut u, "01-top");
    let t = u.studio_top();
    let all = [t.run, t.single, t.acq, t.add, t.view, t.gear];
    for (i, a) in all.iter().enumerate() {
        assert!(a.x >= 0 && a.right() <= W && a.y >= 0 && a.bottom() <= TOP.h, "chip {i} inside the bar");
        for b in &all[i + 1..] { assert!(!overlaps(*a, *b), "chips do not overlap"); }
    }
    assert_eq!(u.single_btn(), t.single);
    assert_eq!(u.run_pill(), t.run);
    // Run and Stop toggle the acquisition, Single arms one
    let run = u.m.run;
    tap_in(&mut u, t.run);
    assert_ne!(u.m.run, run);
    tap_in(&mut u, t.single);
    assert_eq!(u.m.trig_mode, TrigMode::Single);
    // the acquisition chip opens Acquire, the gear the Main menu, Cursors its page
    tap_in(&mut u, t.acq);
    assert_eq!(u.menu, MenuId::Acquire);
    u.close_menu();
    tap_in(&mut u, t.gear);
    assert_eq!(u.menu, MenuId::Main);
    u.close_menu();
    // Cursors: the first tap puts the cursors on (no menu), the next opens their settings (batch 8)
    u.studio_cursors_tap();
    assert_eq!(u.m.cursor, CursorMode::Track);
    assert!(!u.menu_open);
    u.studio_cursors_tap();
    assert_eq!(u.menu, MenuId::Cursor);
    u.close_menu();
    // Add opens the grouped Add menu (Measurement first); Measure is its first row
    tap_in(&mut u, t.add);
    assert!(u.studio.add_menu && !u.menu_open);
    let first = u.studio_add_rows()[0];
    assert_eq!(first.0, super::addmenu::AddRow::Measurement);
    tap_in(&mut u, first.1);
    assert_eq!(u.menu, MenuId::MeasAdd);
    u.close_menu();
    // Load file and SIM stay in the menus, not in the bar
    assert!(!u.top_buttons().iter().any(|b| b.1 == "SIM" || b.1.starts_with("Load")));
}

#[test]
fn studio_bottom_bar_has_a_chip_per_channel_the_time_base_and_the_trigger() {
    let mut u = studio();
    shot(&mut u, "02-bottom");
    let b = u.studio_bottom();
    assert_eq!(u.studio_cards().time, b.time);
    // every channel has a chip: wide while it is on, small while it is off
    let mut all: Vec<Rect> = b.ch.iter().copied().filter(|r| r.w > 0).collect();
    assert_eq!(all.len(), 4);
    assert_eq!(all.iter().filter(|r| r.w >= 96).count(), 2);
    all.extend([b.add, b.time, b.trig]);
    for (i, a) in all.iter().enumerate() {
        assert!(a.w > 0 && a.x >= 0 && a.right() <= W && a.y >= u.frame().bottom.y && a.bottom() <= H, "chip {i} inside the bar");
        for o in &all[i + 1..] { assert!(!overlaps(*a, *o), "chips do not overlap"); }
    }
    assert!(b.ch[2].w == 44 && b.ch[3].w == 44, "small chips for the channels that are off");
    // an on channel opens its page
    let b = u.studio_bottom();
    tap_in(&mut u, b.ch[0]);
    assert_eq!(u.menu, MenuId::Channel(0));
    u.close_menu();
    tap_in(&mut u, b.time);
    assert_eq!(u.menu, MenuId::Horizontal);
    u.close_menu();
    tap_in(&mut u, b.trig);
    assert_eq!(u.menu, MenuId::Trigger);
    u.close_menu();
    // all four channels on still fit
    for c in 0..n_ch() { u.m.ch[c].on = true; }
    let b = u.studio_bottom();
    assert!(b.ch[n_ch() - 1].right() < b.time.x && b.trig.right() <= W);
}

#[test]
fn studio_plot_has_lanes_with_channel_badges_the_t_marker_and_cursor_tags() {
    let mut u = studio();
    u.m.cursor = CursorMode::Time;
    u.m.cur_a = 0.62;
    u.m.cur_b = 0.80;
    shot(&mut u, "03-plot");
    // one lane per channel that is on, one under the other, inside the plot under the tag row
    assert_eq!(u.wins.len(), 2);
    assert!(u.wins[0].bottom() <= u.wins[1].y);
    assert!(u.studio_tags_on());
    assert_eq!(u.grid.y, u.frame().body.y + super::dock::STRIP_H, "the strip of the waveform, then the scale: no tag row");
    let t = u.studio_tag_rect(u.studio_x_of_time(0.0));
    assert!(t.y > u.grid.y && t.bottom() < u.grid.y + 26, "T sits inside the scale, on its top edge");
    // the tag row, the lanes and the bars do not overlap
    let top = u.studio_top();
    assert!(u.grid.y - super::dock::STRIP_H >= top.run.bottom());
    assert!(u.grid.bottom() <= u.frame().bottom.y);
    // the T tag sits at the trigger position: 0 s at the centre with no delay
    u.m.delay = 0.0;
    assert_eq!(u.studio_x_of_time(0.0), u.grid.x + u.grid.w / 2);
    // Default and Minimal keep their plot exactly
    u.m.layout = Layout::Minimal;
    assert!(!u.studio_tags_on());
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
    u.m.zoom = true;
    assert!(!u.studio_tags_on(), "the zoom view brings its own toolbar");
}

fn badges() -> Ui {
    let mut u = studio();
    u.m.meas.clear();
    u.m.meas2.clear();
    u.m.cat.clear();
    u.m.meas = vec![(Meas::Freq, WaveId::Ch(0)), (Meas::Vpp, WaveId::Ch(0)), (Meas::Vrms, WaveId::Ch(0)), (Meas::Rise, WaveId::Ch(0)),
        (Meas::Freq, WaveId::Ch(1)), (Meas::Vpp, WaveId::Ch(1))];
    u.studio.pinned = Some(vec![MeasKey::One(Meas::Freq, WaveId::Ch(0)), MeasKey::One(Meas::Vpp, WaveId::Ch(0)), MeasKey::One(Meas::Vpp, WaveId::Ch(1))]);
    u.relayout();
    u
}

#[test]
fn studio_pinned_badges_sit_under_the_lanes_and_the_eye_hides_them() {
    let mut u = badges();
    shot(&mut u, "04-badges");
    let row = u.studio_row();
    assert_eq!(row.bottom(), u.frame().bottom.y);
    assert!(u.grid.bottom() <= row.y, "the lanes end above the badges");
    let list = u.studio_badges();
    assert_eq!(list.len(), 3);
    for (_, r) in &list { assert!(row.contains(r.x, r.y) && r.right() <= u.studio_eye().x, "badge inside the row"); }
    // the row is paid for by the lanes: hiding the badges gives the height back (mock 02)
    let with = u.grid.h;
    let eye = u.studio_eye();
    tap_in(&mut u, eye);
    assert!(u.studio.hide_badges);
    assert_eq!(u.studio_row_h(), 0);
    assert!(u.grid.h > with);
    shot(&mut u, "05-badges-hidden");
}

#[test]
fn studio_badge_popup_configures_unpins_floats_and_removes() {
    let mut u = badges();
    u.studio_set_view(StudioView::Waveform); // batch 22: the row is for a closed panel
    u.step(0.05);
    let vpp = MeasKey::One(Meas::Vpp, WaveId::Ch(0));
    let open = |u: &mut Ui, key: MeasKey| {
        let r = u.studio_badges().into_iter().find(|(k, _)| *k == key).unwrap().1;
        tap_in(u, r);
        assert_eq!(u.studio.popup, Some(key));
        u.studio_popup_rect().unwrap()
    };
    // a tap outside closes it and does nothing else
    let p = open(&mut u, vpp);
    shot(&mut u, "06-badge-actions");
    u.tap(p.x + 2, p.y - 80);
    assert!(u.studio.popup.is_none() && u.m.meas.len() == 6);
    // Configure opens the measurement's own settings
    let p = open(&mut u, vpp);
    tap_in(&mut u, Ui::studio_popup_row(p, 0));
    assert_eq!(u.meas_cfg, Some(1));
    u.close_menu();
    // Unpin leaves the measurement in the list
    let p = open(&mut u, vpp);
    tap_in(&mut u, Ui::studio_popup_row(p, 1));
    assert!(!u.studio_is_pinned(vpp) && u.m.meas.contains(&(Meas::Vpp, WaveId::Ch(0))));
    assert_eq!(u.studio_badges().len(), 2);
    // Move to floating panel unpins and shows the panel over the lanes
    let key = MeasKey::One(Meas::Freq, WaveId::Ch(0));
    let p = open(&mut u, key);
    tap_in(&mut u, Ui::studio_popup_row(p, 2));
    assert!(!u.studio_is_pinned(key));
    assert_eq!(u.studio.view, StudioView::Floating);
    // batch 22: the floating panel lists them, so the row is gone; close it to have the row again
    u.studio_set_view(StudioView::Waveform);
    u.step(0.05);
    // Remove takes the measurement out of the list, and its pin with it
    let key = MeasKey::One(Meas::Vpp, WaveId::Ch(1));
    let p = open(&mut u, key);
    tap_in(&mut u, Ui::studio_popup_row(p, 3));
    assert!(!u.m.meas.contains(&(Meas::Vpp, WaveId::Ch(1))) && u.m.meas.len() == 5);
    assert!(u.studio_pinned().is_empty());
}

#[test]
fn studio_pins_survive_other_measurements_coming_and_going() {
    let mut u = badges();
    let freq = MeasKey::One(Meas::Freq, WaveId::Ch(1));
    u.studio_set_pin(freq, true);
    assert_eq!(u.studio_pinned().len(), 4);
    u.remove_measurement(0);
    assert_eq!(u.studio_pinned().len(), 3, "the pin of the removed one is gone, the others stay");
    assert!(u.studio_is_pinned(freq));
    // every measurement is pinned from the start, and one that is added is pinned too
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Studio;
    u.m.studio_detail = false;
    u.m.studio_zoom_tab = false;
    assert_eq!(u.studio_pinned().len(), u.studio_keys().len());
    u.studio_set_pin(MeasKey::One(Meas::Vpp, WaveId::Ch(0)), false);
    u.m.meas.push((Meas::Vmax, WaveId::Ch(0)));
    assert!(u.studio_is_pinned(MeasKey::One(Meas::Vmax, WaveId::Ch(0))), "the new one is pinned");
    assert!(!u.studio_is_pinned(MeasKey::One(Meas::Vpp, WaveId::Ch(0))), "the one taken off stays off");
}

#[test]
fn studio_view_menu_chooses_the_arrangement_the_badges_and_resets() {
    let mut u = badges();
    let view = u.studio_top().view;
    tap_in(&mut u, view);
    assert!(u.studio.view_menu);
    shot(&mut u, "07-view-menu");
    let p = u.studio_view_rect();
    assert!(p.x >= 0 && p.right() <= W && p.bottom() < u.frame().bottom.y);
    for i in 0..8 { assert!(p.contains(u.studio_view_row(i).x + 2, u.studio_view_row(i).y + 2), "row {i} inside the menu"); }
    // the chip that opened it closes it
    tap_in(&mut u, view);
    assert!(!u.studio.view_menu);
    // each arrangement, one at a time; a tap closes the menu
    tap_in(&mut u, view);
    for (i, (v, _)) in studio::STUDIO_VIEWS.into_iter().enumerate() {
        if !u.studio.view_menu { tap_in(&mut u, view); }
        let row = u.studio_view_row(i);
        tap_in(&mut u, row);
        assert_eq!(u.studio.view, v);
        assert!(!u.studio.view_menu);
    }
    // the badge switch keeps the menu open
    tap_in(&mut u, view);
    u.studio.view = StudioView::Docked;
    let row = u.studio_view_row(studio::VIEW_BADGES);
    tap_in(&mut u, row);
    assert!(u.studio.hide_badges && u.studio.view_menu);
    tap_in(&mut u, row);
    assert!(!u.studio.hide_badges);
    // Reset layout: the focus view, the badges, the default pins
    u.studio.pinned = Some(Vec::new());
    let row = u.studio_view_row(studio::VIEW_RESET);
    tap_in(&mut u, row);
    assert_eq!((u.studio.view, u.studio.hide_badges, u.studio.pinned.is_none(), u.studio.view_menu), (StudioView::Waveform, false, true, false));
    // a tap outside only closes the menu
    tap_in(&mut u, view);
    u.tap(20, 300);
    assert!(!u.studio.view_menu && !u.menu_open);
}

/// Twelve measurements on CH1 and CH2, three of them pinned, as in the mocks.
fn twelve() -> Ui {
    let mut u = studio();
    u.m.meas = [Meas::Freq, Meas::Vpp, Meas::Vrms, Meas::Rise, Meas::PDuty, Meas::Vavg].into_iter().map(|k| (k, WaveId::Ch(0)))
        .chain([Meas::Freq, Meas::Vpp, Meas::Vrms, Meas::Rise, Meas::Fall, Meas::Vavg].into_iter().map(|k| (k, WaveId::Ch(1)))).collect();
    u.m.meas2.clear();
    u.m.cat.clear();
    u.mstat.clear();
    u.studio.pinned = None;
    u.relayout();
    u
}

#[test]
fn studio_docked_panel_lists_pages_pins_and_hands_back_the_width() {
    let mut u = twelve();
    let full = u.grid.w;
    u.studio_set_view(StudioView::Docked);
    for _ in 0..4 { u.step(0.05); }
    shot(&mut u, "08-docked");
    let g = u.studio_panel_geo().expect("docked panel");
    assert_eq!(u.grid.w, full - 274, "the lanes give their width to the panel");
    assert!(g.p.x >= u.grid.right() && g.p.right() <= W && g.p.bottom() <= u.frame().bottom.y && g.p.y >= TOP.h);
    assert!(g.list.bottom() <= g.p.bottom(), "no pager and no footer: the list has the room");
    assert!(g.page >= 5, "about six rows to a page, like the mock");
    // the badges sit under the lanes only, the panel keeps its column
    assert_eq!(u.studio_row().right(), u.grid.right());
    // there is no pager: the list scrolls by pixels and stops at both ends
    let total = 12;
    let max = u.panel_scroll_max(total, &g);
    assert!(max > 0);
    u.panel_scroll_set(max + 500, total, &g);
    assert_eq!(u.panel_scroll_px(), max);
    shot(&mut u, "11-scroll");
    u.panel_scroll_set(-50, total, &g);
    assert_eq!(u.panel_scroll_px(), 0);
    u.studio.panel.first = 0;
    // the pin left of the row menu pins and unpins it
    let all = u.studio_pinned().len();
    assert_eq!(all, u.studio_keys().len(), "every measurement is pinned until it is taken off");
    let row = Rect::new(g.list.right() - 76, g.list.y + 3 * u.studio_panel_row() + 5, 20, 30);
    tap_in(&mut u, row);
    assert_eq!(u.studio_pinned().len(), all - 1);
    tap_in(&mut u, row);
    assert_eq!(u.studio_pinned().len(), all);
    // a tap on the name opens the settings of the measurement
    tap_in(&mut u, Rect::new(g.list.x + 80, g.list.y + 5, 40, 30));
    assert_eq!(u.meas_cfg, Some(0));
    u.close_menu();
    // + Add measurement opens the chooser
    tap_in(&mut u, g.add);
    assert_eq!(u.menu, MenuId::MeasAdd);
    u.close_menu();
    // the strip's X goes back to the lanes, its maximize button to the table
    let strip = |u: &mut Ui, r: Rect| { u.touch_ev(Phase::Down, r.x + r.w / 2, r.y + 20); u.touch_ev(Phase::Up, r.x + r.w / 2, r.y + 20); };
    let leaf = u.dock_leaf(super::dock::VKind::Measure).unwrap();
    strip(&mut u, Ui::dock_close_rect(leaf));
    assert_eq!(u.studio.view, StudioView::Waveform);
    assert_eq!(u.grid.w, full);
    u.studio_set_view(StudioView::Docked);
    let leaf = u.dock_leaf(super::dock::VKind::Measure).unwrap();
    strip(&mut u, Rect::new(leaf.right() - 96, leaf.y, 48, 40));
    assert_eq!(u.studio.view, StudioView::Only);
}

const PANEL_ROW_TEST: i32 = 46;

#[test]
fn studio_list_scrolls_with_the_finger() {
    let mut u = twelve();
    u.studio_set_view(StudioView::Docked);
    let g = u.studio_panel_geo().unwrap();
    let (x, y) = (g.list.x + 60, g.list.y + g.list.h - 10);
    u.touch_ev(Phase::Down, x, y);
    u.touch_ev(Phase::Move, x, y - 70);
    u.touch_ev(Phase::Move, x, y - 130);
    u.touch_ev(Phase::Up, x, y - 130);
    assert_eq!(u.studio.panel.first, 2, "the list follows an upward finger by rows");
    assert!(u.meas_cfg.is_none(), "a scroll is not a tap");
    // a drag the other way brings it back
    u.touch_ev(Phase::Down, x, y - 100);
    u.touch_ev(Phase::Move, x, y + 40);
    u.touch_ev(Phase::Up, x, y + 10);
    assert_eq!(u.studio.panel.first, 0);
    // a tap through the touch path still reaches the row
    u.touch_ev(Phase::Down, g.list.x + 60, g.list.y + 5);
    u.touch_ev(Phase::Up, g.list.x + 60, g.list.y + 5);
    assert_eq!(u.meas_cfg, Some(0));
}

#[test]
fn studio_floating_panel_lists_the_measurements_moves_docks_and_opens_the_table() {
    // Batch 10: the floating panel is a dock view: its title strip moves it, the X closes it, the strip's maximize
    // opens the table and a drag to an edge docks it (batch10_tests has the drops)
    let mut u = twelve();
    u.studio_set_view(StudioView::Floating);
    shot(&mut u, "09-floating");
    let r = u.dock_float_rect(super::dock::VKind::Measure).expect("floating panel");
    let p = u.studio_panel_rect().unwrap();
    assert!(r.x >= 0 && r.right() <= W && r.y >= TOP.h && r.bottom() <= u.frame().bottom.y);
    assert!(p.y == r.y + super::dock::STRIP_H && p.h == r.h - super::dock::STRIP_H, "the panel draws under its strip");
    assert_eq!(u.grid.w, W, "a floating panel takes no room from the lanes");
    // it follows a finger on its title
    let (x, y) = (r.x + 100, r.y + 12);
    u.touch_ev(Phase::Down, x, y);
    u.touch_ev(Phase::Move, x - 200, y + 60);
    u.touch_ev(Phase::Move, x - 200, y + 60);
    u.touch_ev(Phase::Up, x - 200, y + 60);
    let moved = u.dock_float_rect(super::dock::VKind::Measure).unwrap();
    assert_eq!((moved.x, moved.y), (r.x - 200, r.y + 60));
    assert_eq!(u.studio.view, StudioView::Floating, "dragging the title is not a tap");
    // a touch on the lanes outside it is the plot's, not the panel's
    let delay = u.m.delay;
    let lane = u.wins[0];
    let (lx, ly) = (lane.x + 120, lane.y + 40);
    u.touch_ev(Phase::Down, lx, ly);
    u.touch_ev(Phase::Move, lx + 50, ly);
    u.touch_ev(Phase::Up, lx + 50, ly);
    assert_ne!(u.m.delay, delay, "the lanes still pan");
    // the strip's maximize button: the table
    let r = u.dock_float_rect(super::dock::VKind::Measure).unwrap();
    let max = Rect::new(r.right() - 96, r.y, 48, 40);
    u.touch_ev(Phase::Down, max.x + 24, max.y + 20);
    u.touch_ev(Phase::Up, max.x + 24, max.y + 20);
    assert_eq!(u.studio.view, StudioView::Only);
    u.studio_set_view(StudioView::Floating);
    let r = u.dock_float_rect(super::dock::VKind::Measure).unwrap();
    let close = Ui::dock_close_rect(r);
    u.touch_ev(Phase::Down, close.x + 24, close.y + 20);
    u.touch_ev(Phase::Up, close.x + 24, close.y + 20);
    assert_eq!(u.studio.view, StudioView::Waveform);
}

#[test]
fn studio_measurements_only_is_a_table_with_statistics_and_a_source_filter() {
    let mut u = twelve();
    u.studio_set_view(StudioView::Only);
    for _ in 0..6 { u.step(0.05); }
    assert_eq!(u.studio_row_h(), 0, "the table needs no badge row");
    shot(&mut u, "10-table");
    let [back, filter, stats, add] = u.studio_table_parts().unwrap();
    for (i, a) in [back, filter, stats, add].iter().enumerate() {
        assert!(a.x >= 0 && a.right() <= W, "control {i} on the screen");
    }
    // statistics add the mean, min and max columns
    assert!(!u.studio.panel.stats);
    tap_in(&mut u, stats);
    assert!(u.studio.panel.stats);
    for _ in 0..6 { u.step(0.05); }
    shot(&mut u, "12-table-statistics");
    // the filter walks through the sources and back to all
    tap_in(&mut u, filter);
    assert_eq!(u.studio.filter, Some(WaveId::Ch(0)));
    let rows = u.meas_rows();
    assert_eq!(u.studio_listed(&rows).len(), 6);
    tap_in(&mut u, filter);
    assert_eq!(u.studio.filter, Some(WaveId::Ch(1)));
    tap_in(&mut u, filter);
    assert_eq!(u.studio.filter, None);
    // Reset statistics starts the counting again
    let (_, _, [reset, ..]) = u.studio_table_geo().unwrap();
    assert!(u.mstat.iter().any(|s| s.n > 0));
    tap_in(&mut u, reset);
    assert!(u.mstat.iter().all(|s| s.n <= 1));
    // the table has the body: a tap on its empty part does not reach the lanes, Waveform goes back
    tap_in(&mut u, add);
    assert_eq!(u.menu, MenuId::MeasAdd);
    u.close_menu();
    tap_in(&mut u, back);
    assert_eq!(u.studio.view, StudioView::Waveform);
    // the list button of the badge row (it carries the count; the Meas tab is gone) brings the panel back
    assert!(u.studio_tab().is_none());
    let tab = u.studio_compact_chip();
    tap_in(&mut u, tab);
    assert_eq!(u.studio.view, StudioView::Docked);
}

/// Studio zoomed in on the trigger, five times.
fn zoomed() -> Ui {
    let mut u = twelve();
    u.m.run = RunState::Stop;
    u.set_zoom(true);
    u.m.zscale = u.m.tscale.saturating_sub(3);
    u.m.zdelay = 0.0;
    u.relayout();
    u
}

#[test]
fn studio_zoom_has_a_toolbar_an_overview_and_a_menu() {
    let mut u = zoomed();
    shot(&mut u, "13-zoom");
    let bar = u.studio_zoom_bar().expect("toolbar");
    let row = u.zoom_readout();
    for (i, r) in [bar.label, bar.tdiv, bar.plus, bar.minus, bar.close, bar.center, bar.fit, bar.more].iter().enumerate() {
        assert!(r.x >= row.x && r.right() <= row.right() && r.y >= row.y && r.bottom() <= row.bottom(), "chip {i} in the strip");
    }
    assert!(bar.tdiv.right() <= bar.center.x, "the chips do not overlap");
    // the overview is over the toolbar, the zoomed lanes under it
    let (overview, detail) = u.split_zoom(u.grid);
    assert!(overview.bottom() <= detail.y, "the strip is in the waveform title bar, not between them");
    // the factor is the ratio of the two time bases
    assert!((u.studio_time_zoom() - u.m.time_per_div() / tscales()[u.m.zscale]).abs() < 1e-3);
    // the hint of a pinch shows the factor under the fingers (mock 08)
    u.pinch.active = true;
    u.pinch.detail = true;
    shot(&mut u, "18-zoom-pinch");
    u.pinch.active = false;
    // + and - on the menu step the zoom, Center trigger and Fit record move the window
    tap_in(&mut u, bar.more);
    assert!(u.studio.zoom_menu);
    shot(&mut u, "14-zoom-menu");
    let before = u.m.zscale;
    let plus = u.studio_zoom_rows().into_iter().find(|(r, _)| *r == studio::ZoomRow::Plus).unwrap().1;
    tap_in(&mut u, plus);
    assert!(u.m.zscale < before, "Plus zooms in");
    let minus = u.studio_zoom_rows().into_iter().find(|(r, _)| *r == studio::ZoomRow::Minus).unwrap().1;
    tap_in(&mut u, minus);
    assert_eq!(u.m.zscale, before);
    // a tap outside closes the menu and nothing else
    u.tap(20, 40 + 4);
    assert!(!u.studio.zoom_menu && u.m.zoom);
    u.m.zdelay = 3.0 * u.m.time_per_div();
    assert!(u.studio_trigger_off_screen_for_test());
    tap_in(&mut u, bar.center);
    assert_eq!(u.m.zdelay, 0.0);
    u.m.zscale = before.saturating_sub(2);
    tap_in(&mut u, bar.fit);
    assert_eq!(u.m.zscale, u.m.tscale, "Fit record shows the whole record again");
    // the time/div chip opens Horizontal, Close zoom is on the menu
    tap_in(&mut u, bar.tdiv);
    assert_eq!(u.menu, MenuId::Horizontal);
    u.close_menu();
    tap_in(&mut u, bar.more);
    let close = u.studio_zoom_rows().into_iter().find(|(r, _)| *r == studio::ZoomRow::Close).unwrap().1;
    tap_in(&mut u, close);
    assert!(!u.m.zoom && !u.studio.zoom_menu);
}

#[test]
fn studio_zoom_overview_window_drags_and_a_lane_badge_scales_vertically() {
    let mut u = zoomed();
    for _ in 0..4 { u.step(0.05); }
    shot(&mut u, "15-zoom-navigator");
    // the overview window moves with a finger on it (the existing gesture, restyled)
    let (overview, _) = u.split_zoom(u.grid);
    let before = u.m.zdelay;
    let (x, y) = (overview.x + overview.w / 2, overview.y + 45);
    u.touch_ev(Phase::Down, x, y);
    u.touch_ev(Phase::Move, x + 80, y);
    u.touch_ev(Phase::Up, x + 80, y);
    assert!(u.m.zdelay > before, "the window follows the finger to later time");
    // a pan in the detail lanes moves the time too
    let lane = u.wins[0];
    let before = u.m.zdelay;
    u.touch_ev(Phase::Down, lane.x + 300, lane.y + 60);
    u.touch_ev(Phase::Move, lane.x + 360, lane.y + 60);
    shot(&mut u, "16-zoom-pan");
    u.touch_ev(Phase::Up, lane.x + 360, lane.y + 60);
    assert!(u.m.zdelay < before, "the content follows the finger");
    // a channel badge opens the display scale of its lane
    u.render(&mut Canvas::new(W, H));
    let (c, badge) = u.studio.lane_badges.borrow()[1];
    assert_eq!(c, 1);
    tap_in(&mut u, badge);
    assert_eq!(u.studio.yzoom, Some(1));
    let (_, minus, plus, reset) = u.studio_yzoom_parts().unwrap();
    tap_in(&mut u, plus);
    tap_in(&mut u, plus);
    assert_eq!(u.zoom_vfactor, 4.0);
    shot(&mut u, "17-zoom-vertical");
    tap_in(&mut u, minus);
    assert_eq!(u.zoom_vfactor, 2.0);
    tap_in(&mut u, reset);
    assert_eq!(u.zoom_vfactor, 1.0);
    u.tap(300, 120);
    assert!(u.studio.yzoom.is_none());
    // outside the zoom a badge opens the channel menu
    u.set_zoom(false);
    u.render(&mut Canvas::new(W, H));
    let badge = u.studio.lane_badges.borrow()[0].1;
    tap_in(&mut u, badge);
    assert_eq!(u.menu, MenuId::Channel(0));
}
