// SPDX-License-Identifier: GPL-2.0-or-later
//! Display > Layout: the stock OOS bars (Default) and the compact OOS-plus bars (Minimal).
use super::*;

fn shown(minimal: bool) -> (Ui, Canvas) {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.minimal = minimal;
    for _ in 0..8 {
        u.step(0.05);
    }
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    // OOS_SHOTS=<dir> writes the screens for a look
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        let name = if minimal { "layout-minimal.png" } else { "layout-default.png" };
        std::fs::write(std::path::Path::new(&dir).join(name), crate::store::png_of(&cv)).unwrap();
    }
    (u, cv)
}

#[test]
fn default_layout_has_the_stock_bars_and_minimal_the_compact_ones() {
    let (d, _) = shown(false);
    let (m, _) = shown(true);
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
    let (mut u, _) = shown(false);
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
    assert!(u.m.minimal, "OOS-plus starts with its own bars");
    assert!(!u.m.dots, "samples are joined by default");
    assert!(u.items(MenuId::Display).contains(&Item::Layout));
    u.m.minimal = false;
    let text = crate::store::setup_text(&u.m);
    assert!(text.contains("layout = default"));
    assert!(!crate::store::parse_setup(&text).unwrap().minimal);
}
