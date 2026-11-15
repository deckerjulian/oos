// SPDX-License-Identifier: GPL-2.0-or-later
//! Display > Layout: the stock OOS bars (Default), the compact OOS-plus bars (Minimal) and Studio.
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
    assert_eq!(u.m.layout, Layout::Minimal, "OOS-plus starts with its own bars");
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
    assert_eq!(seen, [Layout::Studio, Layout::Default, Layout::Minimal]);
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
    u.m.ch[1].on = true;
    u.studio.enter = true;
    for _ in 0..8 {
        u.step(0.05);
    }
    u
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
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
    let all = [t.badge, t.run, t.single, t.state, t.acq, t.measure, t.cursors, t.view, t.gear];
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
    tap_in(&mut u, t.cursors);
    assert_eq!(u.menu, MenuId::Cursor);
    u.close_menu();
    tap_in(&mut u, t.measure);
    assert!(u.menu_open);
    u.close_menu();
    // Load file and SIM stay in the menus, not in the bar
    assert!(!u.top_buttons().iter().any(|b| b.1 == "SIM" || b.1.starts_with("Load")));
}
