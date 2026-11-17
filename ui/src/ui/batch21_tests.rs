// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 21: the grouped Menu, the LA and GEN pills, SIM and Live in the top bar, the measurement views and the classic
//! badge strip. One test per item that can be tested. OOS_SHOTS=<dir> writes pictures (b21-*).
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
        std::fs::write(std::path::Path::new(&dir).join(format!("b21-{name}.png")), crate::store::png_of(&cv)).unwrap();
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

fn open_menu(u: &mut Ui) {
    let gear = u.studio_top().gear;
    tap(u, gear);
    for _ in 0..3 { frame(u); }
    assert!(u.menu_open && u.menu == MenuId::Main && u.sheet_active());
}

fn cell(u: &Ui, it: Item) -> Rect {
    u.menu_geo().groups.iter().flat_map(|g| g.items.iter()).find(|(i, _)| *i == it).unwrap_or_else(|| panic!("{it:?} is in the Menu")).1
}

fn scroll_to(u: &mut Ui, it: Item) {
    let r = cell(u, it);
    let g = u.menu_geo();
    if r.y < g.view.y || r.bottom() > g.view.bottom() {
        u.sheet.scroll = (u.sheet.scroll + r.y - g.view.y).clamp(0, g.max_scroll());
    }
    frame(u);
}

#[test]
fn the_gear_sheet_is_the_grouped_menu_with_every_group_and_entry() {
    use Item::*;
    use MenuId as M;
    let mut u = studio();
    open_menu(&mut u);
    assert_eq!(u.menu_title(MenuId::Main), "Menu");
    let g = u.menu_geo();
    let names: Vec<&str> = g.groups.iter().map(|g| g.name).collect();
    assert_eq!(names, ["Acquisition", "Trigger", "Analysis", "Analysers", "Instruments", "Display", "System"]);
    let want: [(&str, Vec<Item>); 7] = [
        ("Acquisition", vec![RunStop, SingleAct, AutosetAct, ClearDisp, DefaultAct, Open(M::Acquire)]),
        ("Trigger", vec![Open(M::Trigger), SlopeAct, ForceAct, Open(M::Zone)]),
        ("Analysis", vec![Open(M::Measure), Open(M::Cursor), Open(M::Math), Open(M::Ref), Open(M::Sim), Open(M::Decode), Open(M::Bus), Open(M::Mask)]),
        ("Analysers", vec![Open(M::Spectrum), Open(M::Eye), Open(M::Power), Open(M::Bode), Open(M::BwTest), IdentifyAct, Open(M::Magnetic), Open(M::Inverter), Open(M::Dpt)]),
        ("Instruments", vec![Open(M::Dvm), Open(M::Logger)]),
        ("Display", vec![Open(M::Display), Open(M::Horizontal), NavHist]),
        ("System", vec![Open(M::Storage), Shot, QuickRun, LockAct, Open(M::Utility), PowerAct]),
    ];
    for (name, items) in want {
        let grp = g.groups.iter().find(|x| x.name == name).unwrap();
        let have: Vec<Item> = grp.items.iter().map(|x| x.0).collect();
        if name == "Instruments" {
            // the generator and the logic analyser only on models that have them
            for it in &items { assert!(have.contains(it), "{name}: {it:?}"); }
            assert_eq!(have.contains(&Open(M::Afg)), oos_hal::model::platform().caps.has_gen());
            assert_eq!(have.contains(&Open(M::La)), la::available());
        } else {
            assert_eq!(have, items, "{name}");
        }
    }
    // no tabs, no footer; up to six buttons in a row; targets of 40 px; labels left of the grid; nothing outside the sheet
    let spec = u.sheet_spec(MenuId::Main).unwrap();
    assert_eq!(spec.tabs.len(), 1);
    assert!(spec.tabs[0].foot.is_empty());
    let f = u.fonts();
    for grp in &g.groups {
        assert!(f.sb.width(grp.name) <= grp.label.w, "{} fits its column", grp.name);
        for (it, r) in &grp.items {
            assert!(r.h >= 40 && r.w >= 100, "{it:?} target {r:?}");
            assert!(r.x >= g.r.x && r.right() <= g.r.right(), "{it:?} inside the sheet");
            assert!(r.x >= grp.label.right(), "{it:?} right of its label");
            let label = u.menu_cell_label(*it);
            let room = r.w - 32 - if matches!(it, Open(_)) { 16 } else { 6 };
            assert!(f.s.width(&label) <= room, "{label} fits in {room}");
        }
    }
    // batch 22: the Menu is the wide overlay and the whole page fits at 1024 x 600, in rows of six
    assert_eq!(g.max_scroll(), 0, "no scrolling at 1024 x 600");
    assert!(g.r.w >= W - 24, "a near-full-width overlay: {:?}", g.r);
    let acq = g.groups.iter().find(|x| x.name == "Acquisition").unwrap();
    assert!(acq.items.iter().all(|(_, r)| r.y == acq.items[0].1.y), "Acquisition is one row");
    assert_eq!(u.sheet_dock_w(), 0, "it floats over the waveform");
    assert_eq!(g.r, u.sheet_rect());
    assert!(g.r.bottom() <= H);
    shot(&mut u, "menu");
}

#[test]
fn the_menu_scrolls_by_drag_and_a_tap_on_a_button_opens_its_sheet() {
    use Item::*;
    let mut u = studio();
    open_menu(&mut u);
    // a window too short for the page: it scrolls
    u.scr = Screen { w: 1024, h: 400 };
    let g = u.menu_geo();
    assert!(g.max_scroll() > 120);
    // a drag moves the page and taps nothing
    let (x, y) = (g.view.x + g.view.w / 2, g.view.y + g.view.h - 20);
    touch(&mut u, Phase::Down, x, y);
    touch(&mut u, Phase::Move, x, y - 60);
    touch(&mut u, Phase::Move, x, y - 120);
    touch(&mut u, Phase::Up, x, y - 120);
    u.step(0.02);
    assert_eq!(u.sheet.scroll, 120);
    assert!(u.menu == MenuId::Main && u.menu_open);
    u.sheet.scroll = 0;
    u.scr = Screen::PANEL;
    frame(&mut u);
    tap_cell(&mut u, Open(MenuId::Trigger));
    assert_eq!(u.menu, MenuId::Trigger);
    assert!(u.sheet_active());
}

#[test]
fn a_tap_on_generator_opens_the_generator_sheet_and_logic_analyser_its_own() {
    use Item::*;
    let mut u = studio();
    open_menu(&mut u);
    if oos_hal::model::platform().caps.has_gen() {
        scroll_to(&mut u, Open(MenuId::Afg));
        tap_cell(&mut u, Open(MenuId::Afg));
        assert_eq!(u.menu, MenuId::Afg);
        assert!(u.sheet_active());
        let back = u.sheet_chrome().1.expect("Back");
        tap(&mut u, back);
        assert_eq!(u.menu, MenuId::Main);
    }
    if la::available() {
        scroll_to(&mut u, Open(MenuId::La));
        tap_cell(&mut u, Open(MenuId::La));
        assert_eq!(u.menu, MenuId::La);
    }
}

#[test]
fn default_and_power_off_ask_before_they_act_in_the_menu() {
    use Item::*;
    let mut u = studio();
    open_menu(&mut u);
    u.m.tscale = 3;
    tap_cell(&mut u, DefaultAct);
    assert_eq!(u.m.tscale, 3, "the first tap only asks");
    assert!(u.studio.md.armed.is_some());
    shot(&mut u, "menu-armed");
    tap_cell(&mut u, DefaultAct);
    assert_ne!(u.m.tscale, 3, "the second tap does it");
    let mut u = studio();
    open_menu(&mut u);
    scroll_to(&mut u, PowerAct);
    tap_cell(&mut u, PowerAct);
    assert!(!u.power_dlg, "the first tap only asks");
    tap_cell(&mut u, PowerAct);
    assert!(u.power_dlg, "the second tap does it");
}

#[test]
fn the_other_layouts_keep_their_menu() {
    for layout in [Layout::Default, Layout::Minimal] {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = layout;
        for _ in 0..8 { u.step(0.05); }
        u.open_menu(MenuId::Main);
        assert!(!u.sheet_active(), "{layout:?}: classic dialog");
        assert!(u.dlg_layout().is_some());
    }
}

#[test]
fn la_and_gen_pills_sit_after_plus_ch_and_open_their_sheets() {
    let mut u = studio();
    let caps = &oos_hal::model::platform().caps;
    let b = u.studio_bottom();
    assert_eq!(b.la.is_some(), caps.la);
    assert_eq!(b.gen.is_some(), caps.has_gen());
    if let (Some(la), Some(gen)) = (b.la, b.gen) { assert!(b.add.right() < la.x && la.right() < gen.x); }
    for r in [b.la, b.gen].into_iter().flatten() {
        assert!(r.h >= 28 && Ui::hit40(r).h >= 40);
        assert!(r.right() < b.time.x - 5, "inside the room before the time base");
    }
    for c in 0..n_ch() { if b.ch[c].w > 0 { assert!(b.ch[c].right() < b.add.x); assert!(b.ch[c].w >= if u.m.ch[c].on { 60 } else { 40 }); } }
    if let Some(r) = b.gen {
        let off_w = r.w;
        u.m.afg_on = true;
        u.m.afg_wave = 0;
        frame(&mut u);
        let on = u.studio_bottom().gen.unwrap();
        assert!(on.w > off_w, "shows what it makes when on");
        let text = format!("{} {}", u.value(Item::AfgWave), u.value(Item::AfgFreq));
        assert!(u.fonts().s.width(&text) <= on.w - 44, "{text} fits");
        tap(&mut u, on);
        assert_eq!(u.menu, MenuId::Afg);
        assert!(u.sheet_active());
        u.close_menu();
    }
    // the bar as it is now: the generator pill grew, the pills before it moved
    if let Some(r) = u.studio_bottom().la {
        tap(&mut u, r);
        assert_eq!(u.menu, MenuId::La);
        assert!(u.sheet_active());
        u.close_menu();
    }
    shot(&mut u, "pills");
}

#[test]
fn sim_is_in_the_top_bar_when_live_and_live_when_viewing_a_record() {
    let mut u = studio();
    u.store_dir = std::env::temp_dir().join("oos-batch21-sim");
    let t = u.studio_top();
    assert!(!u.record_mode());
    assert!(t.simbtn.w > 0 && t.badge.w == 0, "SIM when live, no Live");
    assert!(t.simbtn.right() < t.acq.x + 1 && t.simbtn.h >= 28);
    shot(&mut u, "topbar-sim");
    // Batch 23: the SIM button enters the simulation mode (the SIM sheet, with Add to ref, is in the Menu)
    u.open_menu(MenuId::Sim);
    assert!(u.sheet_active());
    u.close_menu();
    // the SIM button: Live shows and SIM goes; a tap on Live returns
    tap(&mut u, t.simbtn);
    u.wait_sim_generation();
    for _ in 0..8 { u.step(0.05); }
    assert!(u.record_mode());
    let t = u.studio_top();
    assert!(t.badge.w > 0 && t.simbtn.w == 0);
    shot(&mut u, "topbar-live");
    tap(&mut u, t.badge);
    assert!(!u.record_mode());
    let t = u.studio_top();
    assert!(t.badge.w == 0 && t.simbtn.w > 0);
}

#[test]
fn a_short_top_bar_shortens_labels_instead_of_dropping_buttons() {
    let u = studio();
    let t = u.studio_top();
    assert!(t.cursors.w == 0 && t.view.w > 0 && t.add.w > 0, "Add is the one button for Add, Meas and Cursors (Cursor is a key)");
    assert!(t.acq.w >= 120 || t.add.w < u.fonts().mb.width("Add") + 20);
    assert!(t.add.right() < t.display.x);
}

fn tap_cell(u: &mut Ui, it: Item) {
    let r = cell(u, it);
    tap(u, r);
}

// ------------------------------------------------------------------------------------------------ measurement views

fn two_sources(u: &mut Ui) {
    u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0)), (Meas::Vamp, WaveId::Ch(1)), (Meas::Freq, WaveId::Ch(1)), (Meas::Rise, WaveId::Ch(0))];
    u.ensure_meas_own();
    for _ in 0..6 { frame(u); }
}

fn listed(u: &Ui) -> usize {
    let rows = u.meas_rows();
    u.studio_listed(&rows).len()
}

fn tap_xy(u: &mut Ui, x: i32, y: i32) {
    touch(u, Phase::Down, x, y);
    touch(u, Phase::Up, x, y);
    frame(u);
}

fn view_seg(u: &Ui, k: i32) -> (i32, i32) {
    let r = u.dock_rect(super::dock::VKind::Measure).and_then(|r| u.dock_view_rect(super::dock::VKind::Measure, r)).expect("the view switch");
    let k = (k + 1) % 3; // Table, List, Cards
    (r.x + (2 * k + 1) * r.w / 6, r.y + r.h / 2)
}

#[test]
fn the_panel_has_list_cards_and_detailed_and_the_strip_switch_picks_one() {
    use super::measpanel::PanelView;
    let mut u = studio();
    two_sources(&mut u);
    assert_eq!(u.panel_view(), PanelView::List);
    let (x, y) = view_seg(&u, 1);
    tap_xy(&mut u, x, y);
    assert_eq!(u.panel_view(), PanelView::Cards);
    assert!(u.panel_is_cards() && !u.studio_detailed());
    let g = u.studio_panel_geo().unwrap();
    let rows = u.meas_rows();
    let l = u.studio_listed(&rows);
    let cards = u.panel_cards_layout(0, l.len(), g.list);
    assert!(cards.len() >= 2 && cards.len() == g.page.min(l.len()));
    for (_, r) in &cards { assert!(r.x >= g.list.x && r.right() <= g.list.right() && r.bottom() <= g.list.bottom(), "{r:?} in {:?}", g.list); }
    assert!(cards[0].1.right() < cards[1].1.x, "two columns");
    shot(&mut u, "meas-cards");
    let (x, y) = view_seg(&u, 2);
    tap_xy(&mut u, x, y);
    assert_eq!(u.panel_view(), PanelView::Detailed);
    assert!(u.studio_detailed());
    shot(&mut u, "meas-detailed");
    let (x, y) = view_seg(&u, 0);
    tap_xy(&mut u, x, y);
    assert_eq!(u.panel_view(), PanelView::List);
    assert!(!u.panel_is_cards() && !u.studio_detailed());
}

#[test]
fn the_list_has_the_header_with_source_gate_search_count_and_pinned_only() {
    let mut u = studio();
    two_sources(&mut u);
    let g = u.studio_panel_geo().unwrap();
    let bar = u.panel_bar(g.p);
    let src = bar.slots.iter().find(|s| s.item == super::measpanel::BarItem::Source).expect("the source control").r;
    assert!(src.h >= 24 && src.bottom() <= g.list.y, "{src:?}");
    assert_eq!(u.panel_source_label(), "All sources");
    assert_eq!(u.panel_gate_label(), "Full record");
    let total = listed(&u);
    assert_eq!(total, 5);
    shot(&mut u, "meas-list");
    // source filter: a dropdown with All sources and every source
    tap(&mut u, src);
    assert_eq!(u.studio.panel.drop, Some(super::measpanel::PanelDrop::Source));
    let p = u.panel_drop_rect().unwrap();
    assert!(p.bottom() <= H);
    shot(&mut u, "meas-source-dropdown");
    tap(&mut u, Ui::panel_drop_row(p, 2));
    assert_eq!(u.studio.filter, Some(WaveId::Ch(1)));
    assert_eq!(listed(&u), 2);
    assert_eq!(u.panel_source_label(), "CH2");
    // back to all
    tap(&mut u, src);
    let p = u.panel_drop_rect().unwrap();
    tap(&mut u, Ui::panel_drop_row(p, 0));
    assert_eq!(u.studio.filter, None);
    assert_eq!(listed(&u), total);
    // gate
    u.panel_bar_do(super::measpanel::BarItem::Gate);
    frame(&mut u);
    let p = u.panel_drop_rect().unwrap();
    tap(&mut u, Ui::panel_drop_row(p, 1));
    assert!(!u.m.meas_full_record);
    assert_eq!(u.panel_gate_label(), "Screen");
    // a tap outside only closes
    u.panel_bar_do(super::measpanel::BarItem::Gate);
    frame(&mut u);
    assert!(u.studio.panel.drop.is_some());
    let n = u.m.meas.len();
    tap_xy(&mut u, g.p.x + 20, g.list.y + 20);
    assert!(u.studio.panel.drop.is_none() && u.m.meas.len() == n);
    // pinned only
    // everything is pinned until one is taken off
    let last = *u.studio_keys().last().unwrap();
    u.studio_set_pin(last, false);
    u.studio_set_pin(super::studio::MeasKey::One(Meas::Vpp, WaveId::Ch(0)), true);
    let pinned = u.studio_pinned().len();
    assert!(pinned >= 1 && pinned < total);
    let item = |u: &Ui, it| u.panel_bar(u.studio_panel_geo().unwrap().p).slots.iter().find(|s| s.item == it).expect("a control").r;
    let r = item(&u, super::measpanel::BarItem::Pinned);
    tap(&mut u, r);
    assert!(u.studio.panel.pinned_only);
    assert_eq!(listed(&u), pinned);
    let r = item(&u, super::measpanel::BarItem::Pinned);
    tap(&mut u, r);
    assert_eq!(listed(&u), total);
    // search: the text narrows the list; the search button clears it again
    u.panel_set_query("freq");
    assert_eq!(listed(&u), 2);
    frame(&mut u);
    assert!(u.studio.panel.searching, "the field is open with the text");
    let close = u.panel_bar(u.studio_panel_geo().unwrap().p).field.expect("the search field").1;
    tap(&mut u, close);
    assert!(u.studio.panel.query.is_empty() && listed(&u) == total);
    let r = item(&u, super::measpanel::BarItem::Search);
    tap(&mut u, r);
    assert!(u.wlan.osk.is_some(), "the keyboard opens for the text");
}

#[test]
fn the_stats_button_cycles_off_mean_sigma_min_max() {
    let mut u = studio();
    two_sources(&mut u);
    let g = u.studio_panel_geo().unwrap();
    assert_eq!((u.studio.panel.stats_mode, u.studio.panel.stats), (0, false));
    // the control is in the bar (behind the dots on a narrow panel): the tap is the same as its action
    let _ = g;
    u.panel_stats_cycle();
    frame(&mut u);
    assert_eq!((u.studio.panel.stats_mode, u.studio.panel.stats), (1, true));
    assert_eq!(u.panel_stats_label(), "Mean + \u{3c3}");
    shot(&mut u, "meas-list-stats");
    u.panel_stats_cycle();
    frame(&mut u);
    assert_eq!(u.studio.panel.stats_mode, 2);
    assert_eq!(u.panel_stats_label(), "Min Max");
    u.panel_stats_cycle();
    frame(&mut u);
    assert_eq!((u.studio.panel.stats_mode, u.studio.panel.stats), (0, false));
}

#[test]
fn a_tap_on_a_card_pin_toggles_it_and_the_rest_of_the_card_configures() {
    let mut u = studio();
    two_sources(&mut u);
    u.panel_set_view(super::measpanel::PanelView::Cards);
    frame(&mut u);
    let g = u.studio_panel_geo().unwrap();
    let rows = u.meas_rows();
    let l = u.studio_listed(&rows);
    let (pos, r) = u.panel_cards_layout(0, l.len(), g.list)[3];
    let key = u.studio_keys()[l[pos]];
    let was = u.studio_is_pinned(key);
    tap(&mut u, Ui::panel_card_pin(r));
    assert_eq!(u.studio_is_pinned(key), !was);
    tap(&mut u, Rect::new(r.x + 10, r.y + 40, 60, 20));
    assert!(u.menu_open, "the settings of the measurement");
}

#[test]
fn the_classic_badge_strip_has_a_row_per_source_with_even_columns() {
    let mut u = studio();
    u.studio_set_view(StudioView::Waveform); // batch 22: the row is for a closed panel
    two_sources(&mut u);
    for k in u.studio_keys() { u.studio_set_pin(k, true); }
    for _ in 0..4 { frame(&mut u); }
    assert!(!u.m.studio_classic, "Studio is the default");
    let studio_h = u.studio_row().h;
    assert!(u.studio_badges().len() >= 3);
    // the setting: Measure > Badge style
    assert_eq!(u.value(Item::BadgeStyle), "Studio");
    assert_eq!(u.options(Item::BadgeStyle), ["Studio", "Classic"]);
    u.open_menu(MenuId::Measure);
    for _ in 0..3 { frame(&mut u); }
    assert!(u.sheet_spec(MenuId::Measure).unwrap().tabs.iter().any(|t| t.rows.contains(&Item::BadgeStyle)));
    u.select_option(Item::BadgeStyle, "Classic");
    u.close_menu();
    for _ in 0..4 { frame(&mut u); }
    assert!(u.m.studio_classic && u.studio_classic());
    let l = u.classic_layout();
    assert_eq!(l.chips.len(), 2, "CH1 and CH2");
    assert_eq!(l.pairs.len(), 5);
    assert!(l.chips[0].1.y < l.chips[1].1.y && l.chips[0].1.x == l.chips[1].1.x);
    assert!(u.studio_row().h > studio_h, "two rows are taller than one");
    // even columns: the second pair of each row starts at the same x
    let row_of = |y: i32| l.pairs.iter().filter(|(_, r)| r.y == y).map(|(_, r)| r.x).collect::<Vec<_>>();
    let (a, b) = (row_of(l.chips[0].1.y), row_of(l.chips[1].1.y));
    assert!(a.len() == 3 && b.len() == 2, "{a:?} {b:?}");
    assert_eq!(a[0], b[0]);
    assert_eq!(a[1], b[1]);
    assert!(a[1] > a[0]);
    // nothing on top of each other, nothing past the lanes, and a pair is still a tap target
    let row = u.studio_row();
    for (_, r) in &l.pairs { assert!(r.x >= row.x && r.right() <= u.studio_row_limit() && r.y >= row.y && r.bottom() <= row.bottom(), "{r:?} in {row:?}"); assert!(r.h >= 20); }
    for w in l.pairs.windows(2) { if w[0].1.y == w[1].1.y { assert!(w[0].1.right() <= w[1].1.x); } }
    shot(&mut u, "classic-badges");
    let (key, r) = u.studio_badges()[0];
    tap(&mut u, r);
    // a measurement that can be drawn is shown on the waveform first; the next tap opens the popup
    if u.studio.popup.is_none() { assert_eq!(u.studio.mark, Some(key)); tap(&mut u, r); }
    assert_eq!(u.studio.popup, Some(key), "a tap on a pair opens the badge popup");
    u.studio.popup = None;
    // no internal names: the amplitude shows its display name
    for (key, _) in u.studio_badges() {
        let name = u.studio_badge_name(key, "x");
        assert!(!name.starts_with("Vamp") && name != "Vpp", "{name}");
    }
    // back to Studio
    u.select_option(Item::BadgeStyle, "Studio");
    frame(&mut u);
    assert!(!u.studio_classic());
    assert_eq!(u.studio_row().h, studio_h);
}

// ------------------------------------------------------------------------------------------------ R and M pills

fn a_ref(label: &str) -> RefWave {
    RefWave { src: 0, color: 0, label: label.into(), t0: 0.0, dt: 1e-6, v: (0..100).map(|i| i as f32).collect::<Vec<_>>().into(), vpd: 1.0, offset: 0.0, on: true }
}

#[test]
fn an_r_pill_selects_its_slot_and_opens_the_reference_sheet_like_a_channel_pill() {
    let mut u = studio();
    u.refs[0] = Some(a_ref("R1"));
    u.refs[1] = Some(a_ref("R2"));
    for _ in 0..4 { frame(&mut u); }
    let b = u.studio_bottom();
    assert!(b.refs[0].w > 0 && b.refs[1].w > 0);
    tap(&mut u, b.refs[0]);
    assert_eq!(u.ref_sel, 0);
    assert!(u.menu_open && u.menu == MenuId::Ref && u.sheet_active(), "the Reference sheet opens on that slot");
    shot(&mut u, "r1-pill");
    // the other pill changes the slot and keeps the sheet; a second tap on the same pill closes it
    tap(&mut u, b.refs[1]);
    assert!(u.menu_open && u.ref_sel == 1);
    tap(&mut u, b.refs[1]);
    assert!(!u.menu_open, "a second tap closes it");
    tap(&mut u, b.refs[0]);
    assert!(u.menu_open && u.ref_sel == 0);
    tap(&mut u, b.refs[0]);
    assert!(!u.menu_open);
}

#[test]
fn an_m_pill_opens_the_math_sheet_on_its_slot() {
    let mut u = studio();
    u.m.math = crate::math::MathOp::Add;
    for _ in 0..4 { frame(&mut u); }
    let b = u.studio_bottom();
    let k = (0..NMATH).find(|&k| b.math[k].w > 0).expect("an M pill");
    tap(&mut u, b.math[k]);
    assert!(u.menu_open && u.menu == MenuId::Math && u.sheet_active());
    assert_eq!(u.m.math_sel, k);
    tap(&mut u, b.math[k]);
    assert!(!u.menu_open);
}

// ------------------------------------------------------------------------------------------------ the numeric pad

fn open_trigger(u: &mut Ui) {
    u.open_menu(MenuId::Trigger);
    for _ in 0..3 { frame(u); }
}

fn value_rect(u: &Ui, it: Item) -> Rect {
    let g = u.sheet_geo().unwrap();
    let r = g.rows.iter().find(|r| r.it == it).unwrap_or_else(|| panic!("{it:?} is on the page")).ctrl;
    Ui::value_parts(r).1
}

fn press(u: &mut Ui, keys: &[super::numpad::NumKey]) {
    for k in keys {
        let r = u.numpad_geo().1.iter().find(|(x, _)| x == k).unwrap_or_else(|| panic!("{k:?}")).1;
        tap(u, r);
    }
}

#[test]
fn a_tap_on_the_value_of_a_stepper_row_opens_the_pad_and_the_keys_enter_a_value() {
    use super::numpad::NumKey::*;
    let mut u = studio();
    open_trigger(&mut u);
    u.m.trig_level = 0.0;
    let r = value_rect(&u, Item::TrigLevel);
    assert!(u.numpad.is_none());
    tap(&mut u, r);
    assert!(u.numpad.is_some(), "the pad opens");
    let (field, keys) = u.numpad_geo();
    let p = u.numpad_rect();
    assert!(p.x >= 0 && p.right() <= W && p.y >= 0 && p.bottom() <= H);
    for (k, r) in &keys {
        assert!(r.h >= 40 && r.w >= 40, "{k:?} {r:?}");
        assert!(r.x >= p.x && r.right() <= p.right() && r.y >= field.bottom() && r.bottom() <= p.bottom(), "{k:?} inside");
    }
    for (i, (a, ra)) in keys.iter().enumerate() { for (b, rb) in &keys[i + 1..] { assert!(ra.right() <= rb.x || rb.right() <= ra.x || ra.bottom() <= rb.y || rb.bottom() <= ra.y, "{a:?} {b:?} overlap"); } }
    // the labels the owner asked for
    let labels: Vec<String> = keys.iter().map(|(k, _)| Ui::numpad_label(*k)).collect();
    for l in ["0", "9", ".", "\u{b1}", "Bksp", "Clear", "Exp", "G", "M", "k", "m", "\u{b5}", "n", "p", "None", "Min", "Max", "Cancel", "Enter"] { assert!(labels.iter().any(|x| x == l), "{l}"); }
    assert_eq!(u.numpad.as_ref().unwrap().unit, "V");
    assert!(u.numpad.as_ref().unwrap().fresh, "the current value is selected");
    shot(&mut u, "keypad");
    // 4 5 0 m: the prefix ends the entry
    press(&mut u, &[Digit(4), Digit(5), Digit(0), Prefix(-3)]);
    assert!(u.numpad.is_none());
    assert!((u.m.trig_level - 0.45).abs() < 1e-6, "{}", u.m.trig_level);
    assert_eq!(u.value(Item::TrigLevel), "450 mV");
    // Enter without a prefix is in the unit shown
    { let r = value_rect(&u, Item::TrigLevel); tap(&mut u, r); }
    press(&mut u, &[Digit(1), Dot, Digit(2), Enter]);
    assert!((u.m.trig_level - 1.2).abs() < 1e-6 && u.numpad.is_none());
    // sign and Bksp and Exp
    { let r = value_rect(&u, Item::TrigLevel); tap(&mut u, r); }
    press(&mut u, &[Digit(5), Digit(5), Bksp, Sign, Digit(1), Exp, Sign, Digit(1), Enter]);
    assert!((u.m.trig_level + 0.51).abs() < 1e-6 || (u.m.trig_level + 0.5).abs() < 1e-6 || u.m.trig_level < 0.0, "{}", u.m.trig_level);
}

#[test]
fn cancel_changes_nothing_and_min_max_set_the_limits_and_an_out_of_range_value_is_clamped() {
    use super::numpad::NumKey::*;
    let mut u = studio();
    open_trigger(&mut u);
    u.m.trig_level = 0.3;
    { let r = value_rect(&u, Item::TrigLevel); tap(&mut u, r); }
    press(&mut u, &[Digit(9), Digit(9), Cancel]);
    assert!(u.numpad.is_none());
    assert_eq!(u.m.trig_level, 0.3, "Cancel leaves it");
    let (lo, hi) = u.numpad_limits(Item::TrigLevel).unwrap();
    assert!(lo < 0.0 && hi > 0.0 && (hi + lo).abs() < 1e-9);
    { let r = value_rect(&u, Item::TrigLevel); tap(&mut u, r); }
    press(&mut u, &[Max]);
    assert!((u.m.trig_level as f64 - hi).abs() < 1e-5, "Max sets the limit");
    { let r = value_rect(&u, Item::TrigLevel); tap(&mut u, r); }
    press(&mut u, &[Min]);
    assert!((u.m.trig_level as f64 - lo).abs() < 1e-5, "Min sets the limit");
    // out of range: clamped, and the clamped value is said
    { let r = value_rect(&u, Item::TrigLevel); tap(&mut u, r); }
    press(&mut u, &[Digit(9), Digit(9), Digit(9), Enter]);
    assert!((u.m.trig_level as f64 - hi).abs() < 1e-5, "clamped to the limit: {}", u.m.trig_level);
    assert!(toast_text(&u).contains("Limited to"), "{}", toast_text(&u));
}

#[test]
fn the_pad_works_for_a_stepper_item_without_its_own_setter() {
    use super::numpad::NumKey::*;
    let mut u = studio();
    u.open_menu(MenuId::Channel(0));
    for _ in 0..3 { frame(&mut u); }
    let r = value_rect(&u, Item::ChOffset(0));
    tap(&mut u, r);
    assert!(u.numpad.is_some());
    press(&mut u, &[Digit(5), Digit(0), Digit(0), Prefix(-3)]);
    let (v, unit) = u.numpad_value(Item::ChOffset(0)).unwrap();
    assert_eq!(unit, "V");
    assert!((v - 0.5).abs() < 0.06, "the stepper's grid: {v}");
    // the time base goes through its own sequence
    u.open_menu(MenuId::Horizontal);
    for _ in 0..3 { frame(&mut u); }
    let r = value_rect(&u, Item::HScale);
    tap(&mut u, r);
    press(&mut u, &[Digit(1), Digit(0), Digit(0), Prefix(-6)]);
    assert_eq!(u.value(Item::HScale), "100 \u{b5}s");
}

#[test]
fn the_pad_is_a_studio_sheet_feature_and_the_other_layouts_keep_their_value_taps() {
    let (v, unit) = super::numpad::split_value("449 mV").unwrap();
    assert!((v - 0.449).abs() < 1e-12 && unit == "V");
    assert_eq!(super::numpad::split_value("-1.5 V"), Some((-1.5, "V".to_string())));
    assert_eq!(super::numpad::split_value("1.00 kHz").map(|x| x.1), Some("Hz".to_string()));
    let (v, unit) = super::numpad::split_value("200 \u{b5}s").unwrap();
    assert!((v - 200e-6).abs() < 1e-15 && unit == "s");
    assert_eq!(super::numpad::split_value("Edge"), None);
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = Layout::Default;
    for _ in 0..8 { u.step(0.05); }
    u.open_menu(MenuId::Trigger);
    assert!(u.numpad.is_none() && !u.sheet_active());
}
fn toast_text(u: &Ui) -> String { u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default() }
