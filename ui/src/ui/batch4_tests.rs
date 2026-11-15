//! Batch 4: the touch model. One long-press time, one hold action per channel, 40 px targets, tappable markers,
//! a drag that moves the trace under the finger, the same Run in every layout, Autoset in two taps.
use super::*;

const LAYOUTS: [Layout; 3] = [Layout::Default, Layout::Minimal, Layout::Studio];

fn layout(l: Layout) -> Ui {
    let mut u = Ui::new(Source::Demo { t: 0.0 });
    u.m.layout = l;
    u.m.ch[1].on = true;
    if l == Layout::Studio { u.studio.enter = true; }
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
