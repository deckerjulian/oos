// SPDX-License-Identifier: GPL-2.0-or-later
//! Batch 4: the touch model. One long-press time, one hold action per channel, 40 px targets, tappable markers,
//! a drag that moves the trace under the finger, the same Run in every layout, Autoset in two taps.
use super::*;

const LAYOUTS: [Layout; 3] = [Layout::Default, Layout::Minimal, Layout::Studio];

fn layout(l: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = l;
    u.m.ch[1].on = true;
    if l == Layout::Studio { u.studio.enter = true; u.m.studio_zoom_tab = false; u.m.studio_detail = false; }
    for _ in 0..8 { u.step(0.05); }
    u.render(&mut Canvas::new(W, H));
    u
}

fn touch(u: &mut Ui, phase: Phase, x: i32, y: i32) {
    u.handle(Ev::Touch { id: 0, phase, x, y });
}

/// A finger down, held for `secs`, in steps of a frame.
fn hold(u: &mut Ui, x: i32, y: i32, secs: f64) {
    touch(u, Phase::Down, x, y);
    let mut t = 0.0;
    while t < secs { u.step(1.0 / 30.0); t += 1.0 / 30.0; }
}

fn centre(r: Rect) -> (i32, i32) { (r.x + r.w / 2, r.y + r.h / 2) }

fn chip(u: &Ui, c: usize) -> Rect { u.cards().0[c] }

fn popup_rows(u: &Ui) -> Vec<String> { u.workspace.popup.as_ref().map(|p| p.rows.iter().map(|r| r.0.clone()).collect()).unwrap_or_default() }

#[test]
fn there_is_one_long_press_time() {
    assert_eq!(Ui::MEAS_HOLD_S, LONG_PRESS);
    assert_eq!(workspace::HOLD, LONG_PRESS);
    assert_eq!(LONG_PRESS, 0.5);
}

#[test]
fn a_hold_on_a_channel_chip_opens_its_quick_actions_in_every_layout() {
    for l in LAYOUTS {
        let mut u = layout(l);
        let coupling = u.m.ch[0].coupling;
        let (x, y) = centre(chip(&u, 0));
        hold(&mut u, x, y, LONG_PRESS - 0.2);
        assert!(u.workspace.popup.is_none(), "{l:?}: not yet");
        hold_more(&mut u, 0.3);
        let rows = popup_rows(&u);
        assert_eq!(u.workspace.popup.as_ref().map(|p| p.title.as_str()), Some("CH1"), "{l:?} titled");
        for want in ["Coupling", "Invert", "Zero offset", "Turn off", "CH1 settings"] {
            assert!(rows.iter().any(|r| r.starts_with(want)), "{l:?}: {want} in {rows:?}");
        }
        touch(&mut u, Phase::Up, x, y);
        assert_eq!(u.m.ch[0].coupling, coupling, "{l:?}: a hold never flips AC/DC");
        assert!(u.workspace.popup.is_some(), "{l:?}: the lift of the hold keeps the popup");
        // the rows act
        u.m.ch[0].offset = 0.3;
        let i = rows.iter().position(|r| r.starts_with("Zero offset")).unwrap();
        let (rx, ry) = centre(u.workspace.popup.as_ref().unwrap().row_rect(i));
        u.tap(rx, ry);
        assert_eq!(u.m.ch[0].offset, 0.0, "{l:?}");
        assert!(u.workspace.popup.is_none());
    }
}

fn hold_more(u: &mut Ui, secs: f64) {
    let mut t = 0.0;
    while t < secs { u.step(1.0 / 30.0); t += 1.0 / 30.0; }
}

#[test]
fn a_hold_on_a_lane_badge_or_marker_opens_the_same_quick_actions() {
    for l in LAYOUTS {
        let mut u = layout(l);
        let (x, y) = if l == Layout::Studio {
            centre(u.studio.lane_badges.borrow().iter().find(|b| b.0 == 0).unwrap().1)
        } else {
            let a = u.area;
            (a.x + 10, u.y_of_volts(0, 0.0))
        };
        hold(&mut u, x, y, LONG_PRESS + 0.1);
        assert_eq!(u.workspace.popup.as_ref().map(|p| p.title.as_str()), Some("CH1"), "{l:?}");
        let rows = popup_rows(&u);
        assert!(rows.iter().any(|r| r.starts_with("Coupling")) && rows.iter().any(|r| r.starts_with("Zero offset")), "{l:?}: {rows:?}");
        touch(&mut u, Phase::Up, x, y);
    }
}

#[test]
fn a_ring_fills_under_a_held_finger_and_the_popup_has_a_title_a_close_and_finger_sized_rows() {
    for l in LAYOUTS {
        let mut u = layout(l);
        let (x, y) = centre(chip(&u, 0));
        touch(&mut u, Phase::Down, x, y);
        assert!(u.press_ring().is_none(), "{l:?}: no ring on a tap");
        hold_more(&mut u, 0.3);
        let (rx, ry, f) = u.press_ring().expect("ring while holding");
        assert!((rx, ry) == (x, y) && f > 0.2 && f < 1.0, "{l:?}: {f}");
        let mut cv = Canvas::new(W, H);
        let before = { u.render(&mut cv); cv.px.clone() };
        let mut u2 = layout(l);
        u2.render(&mut cv);
        assert_ne!(before, cv.px, "{l:?}: the ring is drawn");
        hold_more(&mut u, 0.3);
        assert!(u.press_ring().is_none());
        let p = u.workspace.popup.as_ref().unwrap();
        assert!(p.close_rect().w >= 40 && p.close_rect().h >= 40);
        for i in 0..p.rows.len() { assert!(p.row_rect(i).h >= 40, "{l:?} row {i}"); }
        // the close
        let (cx, cy) = centre(p.close_rect());
        touch(&mut u, Phase::Up, x, y);
        u.tap(cx, cy);
        assert!(u.workspace.popup.is_none(), "{l:?}: close");
    }
}

#[test]
fn the_plot_hold_popup_has_a_title_and_fits_with_rows_of_40_px() {
    for l in LAYOUTS {
        // Studio has a context menu of its own (studioctx.rs, tested in measpanel_redesign_tests)
        if l == Layout::Studio { continue; }
        let mut u = layout(l);
        let g = u.grid;
        u.context_menu(g.x + g.w / 2, g.y + g.h / 2);
        let p = u.workspace.popup.as_ref().unwrap();
        assert_eq!(p.title, "Plot");
        assert!(p.rect.y >= g.y && p.rect.bottom() <= g.bottom() && p.rect.x >= g.x && p.rect.right() <= g.right(), "{l:?} {:?} in {g:?}", p.rect);
        for i in 0..p.rows.len() { assert!(p.row_rect(i).h >= 40 && p.rect.contains(p.row_rect(i).x + 1, p.row_rect(i).y + 1), "{l:?} row {i}"); }
        assert!(p.rows.iter().any(|r| r.0 == "Autoset"), "Autoset is in the plot's hold popup");
    }
}

fn h_of(r: Rect) -> i32 { r.h }

#[test]
fn chips_in_the_compact_bars_take_a_touch_over_a_40_px_height() {
    for l in [Layout::Minimal, Layout::Studio] {
        let mut u = layout(l);
        let r = chip(&u, 0);
        assert!(h_of(r) < 40, "{l:?}: the drawn chip is small ({r:?})");
        let bar = u.frame().bottom;
        // the 2 px over the chip row and the 5 px under it still open the channel's menu
        for dy in [-5, -2, 0, r.h - 1 + 3] {
            u.close_menu();
            u.tap(r.x + r.w / 2, r.y + dy.max(-2));
            assert!(u.menu_open && u.menu == MenuId::Channel(0), "{l:?}: dy {dy}");
        }
        assert!(r.y - 2 >= bar.y - 2 && r.bottom() + 5 >= bar.bottom() - 1);
    }
}

#[test]
fn a_lane_badge_takes_a_40_px_touch() {
    let mut u = layout(Layout::Studio);
    let b = u.studio.lane_badges.borrow().iter().find(|b| b.0 == 0).unwrap().1;
    assert!(b.h < 40);
    let (x, y) = centre(b);
    for (dx, dy) in [(0, 0), (-18, 0), (18, 0), (0, -19), (0, 19)] {
        u.close_menu();
        u.tap(x + dx, y + dy);
        assert!(u.menu_open && u.menu == MenuId::Channel(0), "badge touch at {dx},{dy}");
    }
}

#[test]
fn zoom_bar_buttons_take_a_40_px_touch_in_default_and_studio() {
    for l in [Layout::Default, Layout::Studio] {
        let mut u = layout(l);
        u.set_zoom(true);
        u.render(&mut Canvas::new(W, H));
        let row = u.zoom_readout();
        let (target, before): (Rect, i32) = if l == Layout::Studio {
            (u.studio_zoom_bar().unwrap().more, 0)
        } else {
            (u.zoom_controls()[4].0, 0)
        };
        assert!(target.h < 40, "{l:?}: drawn {target:?}");
        let _ = (row, before);
        let (x, y) = centre(target);
        if l == Layout::Default {
            // Box: arms the zoom box, from the edge of a 40 px target

            u.tap(x, y - 19);
            assert!(u.zoom_arm, "{l:?}: a touch 19 px over the middle of Box arms it");
        } else {
            assert!(!u.studio.zoom_menu);
            u.tap(x, y - 19);
            assert!(u.studio.zoom_menu, "{l:?}: the zoom menu button took the touch");
        }
    }
}

fn toast_text(u: &Ui) -> String { u.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default() }

#[test]
fn the_trigger_marker_opens_trigger_in_every_layout() {
    for l in LAYOUTS {
        let mut u = layout(l);
        let rects = u.trigger_marker_rects();
        assert!(!rects.is_empty(), "{l:?}");
        for r in rects {
            u.close_menu();
            let (x, y) = centre(r);
            u.studio.trig_slider = false;
            u.tap(x, y);
            // Studio: the level tag opens the slider (batch 26), the other layouts the Trigger sheet
            if l == Layout::Studio && u.trigger_tag_hit().is_some_and(|t| t.contains(x, y)) { assert!(u.studio.trig_slider, "{l:?}: marker {r:?}"); u.studio.trig_slider = false; continue; }
            assert!(u.menu_open && u.menu == MenuId::Trigger, "{l:?}: marker {r:?}");
            // 40 px: a touch 17 px off the middle of the 16 px marker still counts
            u.close_menu();
            u.tap(x - 17, y);
            assert!(u.menu_open && u.menu == MenuId::Trigger, "{l:?}: marker edge");
        }
    }
}

#[test]
fn a_cursor_tag_selects_that_cursor() {
    for l in LAYOUTS {
        let mut u = layout(l);
        u.m.cursor = CursorMode::Time;
        u.render(&mut Canvas::new(W, H));
        let tags = u.cursor_tags();
        assert!(tags.iter().any(|t| t.0 == 0) && tags.iter().any(|t| t.0 == 1), "{l:?}: {tags:?}");
        for (k, r) in tags {
            u.close_menu();
            let (x, y) = centre(r);
            u.tap(x, y);
            assert_eq!(u.workspace.cursor_sel, Some(k), "{l:?}");
            assert!(u.menu_open && u.menu == MenuId::Cursor);
            let item = u.items(MenuId::Cursor)[u.focus];
            assert_eq!(item, if k == 0 { Item::CurA } else { Item::CurB }, "{l:?}: the knob is on that cursor's row");
        }
    }
}

/// A point on the trace of channel `c` in the window the finger would be in.
fn on_trace(u: &Ui, c: usize) -> (i32, i32) {
    let a = u.ch_area(c);
    let (tdiv, delay) = u.view();
    let x = a.x + a.w / 2 + 40;
    let t = delay as f64 + (x - (a.x + a.w / 2)) as f64 / (a.w as f32 / HDIV) as f64 * tdiv as f64;
    let v = u.wave_at_source(WaveId::Ch(c), t).expect("demo samples");
    (x, u.y_of_volts(c, v))
}

#[test]
fn a_tap_on_a_trace_or_lane_selects_that_source() {
    for l in LAYOUTS {
        let mut u = layout(l);
        u.select_workspace_source(WaveId::Ch(0));
        let (x, y) = on_trace(&u, 1);
        finger_tap(&mut u, x, y);
        assert_eq!(u.selected_workspace_source(), Some(WaveId::Ch(1)), "{l:?}");
        u.select_workspace_source(WaveId::Ch(0));
        if l == Layout::Studio {
            // anywhere in the lane
            let lane = u.ch_area(1);
            finger_tap(&mut u, lane.x + 300, lane.y + 4);
            assert_eq!(u.selected_workspace_source(), Some(WaveId::Ch(1)), "{l:?}: lane");
        }
    }
}

#[test]
fn a_vertical_drag_moves_the_trace_under_the_finger() {
    for l in LAYOUTS {
        let mut u = layout(l);
        u.select_workspace_source(WaveId::Ch(0));
        let (o0, o1) = (u.m.ch[0].offset, u.m.ch[1].offset);
        let (x, y) = on_trace(&u, 1);
        touch(&mut u, Phase::Down, x, y);
        for k in 1..=4 { touch(&mut u, Phase::Move, x, y + k * 10); }
        touch(&mut u, Phase::Up, x, y + 40);
        assert_eq!(u.m.ch[0].offset, o0, "{l:?}: the active channel stays");
        assert!(u.m.ch[1].offset != o1, "{l:?}: the trace under the finger moved");
        assert_eq!(u.selected_workspace_source(), Some(WaveId::Ch(1)), "{l:?}");
    }
}

#[test]
fn a_cursor_drags_in_any_lane() {
    let mut u = layout(Layout::Studio);
    u.m.cursor = CursorMode::Time;
    u.m.cur_a = 0.3;
    u.select_workspace_source(WaveId::Ch(0));
    u.render(&mut Canvas::new(W, H));
    let lane = u.ch_area(1);
    assert!(lane != u.ch_area(0));
    let x = lane.x + (0.3 * lane.w as f32) as i32;
    let y = lane.y + lane.h / 2;
    touch(&mut u, Phase::Down, x, y);
    for k in 1..=4 { touch(&mut u, Phase::Move, x + k * 15, y); }
    touch(&mut u, Phase::Up, x + 60, y);
    assert!(u.m.cur_a > 0.33, "cursor A followed the finger in the other lane: {}", u.m.cur_a);
}

#[test]
fn run_is_the_same_in_every_layout() {
    for l in LAYOUTS {
        let mut u = layout(l);
        let pill = u.run_pill();
        let (x, y) = centre(pill);
        let first = u.m.run;
        u.tap(x, y);
        assert!(u.m.run != first, "{l:?}: the status pill toggles Run / Stop");
        u.tap(x, y);
        assert_eq!(u.m.run, first, "{l:?}: and back");
        assert!(!u.menu_open, "{l:?}: the pill opens no menu");
    }
}

#[test]
fn autoset_is_two_taps_away_in_every_layout() {
    for l in LAYOUTS {
        // hold on the plot, then Autoset
        let mut u = layout(l);
        let g = u.grid;
        u.context_menu(g.x + g.w / 2, g.y + g.h / 2);
        let (x, y) = if l == Layout::Studio {
            centre(u.studio_ctx_rows().into_iter().find(|r| r.0 == super::studioctx::CtxRow::Autoset).unwrap().2)
        } else { centre({ let p = u.workspace.popup.as_ref().unwrap(); p.row_rect(p.rows.iter().position(|r| r.0 == "Autoset").unwrap()) }) };
        u.tap(x, y);
        assert_eq!(toast_text(&u), "Auto set-up", "{l:?}: hold popup");
        // or Menu, then Autoset
        let mut u = layout(l);
        let (x, y) = if l == Layout::Studio { centre(u.studio_top().gear) } else if l == Layout::Minimal { centre(Ui::MENU_BTN) } else { centre(u.menu_btn()) };
        u.tap(x, y);
        assert_eq!(u.menu, MenuId::Main, "{l:?}");
        let (x, y) = if l == Layout::Studio { centre(u.menu_geo().groups.iter().flat_map(|g| g.items.iter()).find(|(it, _)| *it == Item::AutosetAct).expect("Autoset in the Menu").1) }
            else { centre(u.dlg_layout().expect("menu").rows.into_iter().find(|r| r.it == Item::AutosetAct).expect("Autoset row").ctrl) };
        u.tap(x, y);
        assert_eq!(toast_text(&u), "Auto set-up", "{l:?}: Menu");
    }
}

/// A touch down and up, as the panel sends it: the window the finger is in takes it.
fn finger_tap(u: &mut Ui, x: i32, y: i32) {
    touch(u, Phase::Down, x, y);
    u.step(0.05);
    touch(u, Phase::Up, x, y);
}

#[test]
fn the_default_time_and_trigger_cards_have_no_hidden_split_targets() {
    let mut u = layout(Layout::Default);
    let (_, tr, trg) = u.cards();
    // the lower half of the Time card opens Horizontal; it no longer toggles the zoom
    let zoom = u.m.zoom;
    u.tap(tr.x + tr.w / 2, tr.y + tr.h - 6);
    assert!(u.menu_open && u.menu == MenuId::Horizontal);
    assert_eq!(u.m.zoom, zoom);
    u.close_menu();
    // the drawn Source button cycles the source, the Mode button the mode; anywhere else on the card opens Trigger
    let (sb, mb) = Ui::trig_parts(trg);
    let (src, mode) = (u.m.trig_src, u.m.trig_mode);
    let (x, y) = centre(sb);
    u.tap(x, y);
    assert!(u.m.trig_src != src && !u.menu_open);
    let (x, y) = centre(mb);
    u.tap(x, y);
    assert!(u.m.trig_mode != mode && !u.menu_open);
    // the right edge of the card, beside the buttons, and the lower half under the level are not buttons
    u.tap(trg.x + 20, trg.y + trg.h - 6);
    assert!(u.menu_open && u.menu == MenuId::Trigger);
    u.close_menu();
    let src = u.m.trig_src;
    u.tap(sb.right() + 5, sb.y + sb.h + 2);
    assert_eq!(u.m.trig_src, src);
    assert!(u.menu_open && u.menu == MenuId::Trigger, "the gap between the buttons is the card's");
}

fn shot(u: &mut Ui, name: &str) {
    let mut cv = Canvas::new(W, H);
    u.render(&mut cv);
    if let Some(dir) = std::env::var_os("OOS_SHOTS") {
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), crate::store::png_of(&cv)).unwrap();
    }
}

#[test]
fn shots_hold_ring_and_popups() {
    for l in LAYOUTS {
        let name = format!("{l:?}").to_lowercase();
        let mut u = layout(l);
        let (x, y) = centre(chip(&u, 0));
        touch(&mut u, Phase::Down, x, y);
        hold_more(&mut u, 0.3);
        shot(&mut u, &format!("b4-{name}-ring"));
        hold_more(&mut u, 0.3);
        touch(&mut u, Phase::Up, x, y);
        shot(&mut u, &format!("b4-{name}-chip-popup"));
        u.dismiss_context();
        let g = u.grid;
        u.context_menu(g.x + g.w / 2, g.y + g.h / 2);
        shot(&mut u, &format!("b4-{name}-plot-popup"));
    }
}
