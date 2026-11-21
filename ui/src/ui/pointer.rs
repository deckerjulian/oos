// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! A mouse and a keyboard on the scope (USB or Bluetooth, crate::hid): the mouse is the finger (a left click is a tap,
//! a drag with the left button a touch drag, the right button the touch-hold context menu, the wheel pans, with Shift
//! the V/div, with Ctrl the zoom, with Alt the trigger level), the keys stand in for the front panel (`hid::command`),
//! and while the on-screen keyboard is open they type into it. The pointer is drawn
//! over everything while the mouse is in use and goes away when it rests.

use super::*;
use crate::hid::{self, Button, Event};

/// How long the pointer stays on the screen after the mouse last did something (s).
const POINTER_S: f64 = 5.0;
/// How far behind the mouse the drawn pointer runs (s). A Bluetooth mouse's reports reach oos-ui in bundles (seen on
/// the unit with an RTL8851BU stick: 2 or 3 reports every 20 ms, 24 ms at the 90th percentile, from a mouse that sends
/// every 11.25 ms), and a frame took one bundle or two: the pointer moved in uneven steps. Drawn this far behind, the
/// place is interpolated between the bundles, and every frame moves it by the same amount.
const SMOOTH_S: f64 = 0.025;
/// Reports this close together are one bundle (s).
const BUNDLE_S: f64 = 0.003;
/// The mouse rested this long: its next bundle starts a new trail (s), from where it rested, as if it left there this
/// long before the bundle came.
const REST_S: f64 = 0.1;
const START_S: f64 = 0.02;

/// Seconds since the first call, from the monotonic clock: when a report came (`Ui::now` is the frame's time).
fn clock() -> f64 {
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}
/// The pointer: white like the accent, with a black edge, so it stands out on traces and on the menus alike.
const POINTER_FILL: Rgb = rgb(0xff, 0xff, 0xff);
const POINTER_EDGE: Rgb = rgb(0x00, 0x00, 0x00);

/// The mouse pointer and the keyboard's modifier keys.
#[derive(Default)]
pub struct Mouse {
    pub x: i32,
    pub y: i32,
    /// the pointer has a place: the mouse moved once
    placed: bool,
    /// shown until this time (`Ui::now`)
    until: f64,
    /// the left button is down: the finger is on the screen
    left: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
    /// where the mouse was when its last bundles came: (clock, x, y), oldest first, for the drawn place
    trail: std::collections::VecDeque<(f64, i32, i32)>,
}

impl Mouse {
    /// The mouse got to (x, y) at time `t` (`clock`); it was at (px, py) before.
    fn moved(&mut self, t: f64, (px, py): (i32, i32)) {
        if self.trail.back().is_none_or(|b| t - b.0 > REST_S) {
            self.trail.clear();
            self.trail.push_back((t - START_S, px, py));
        }
        match self.trail.back_mut() {
            Some(b) if t - b.0 < BUNDLE_S => (b.1, b.2) = (self.x, self.y),
            _ => self.trail.push_back((t, self.x, self.y)),
        }
        while self.trail.len() > 2 && t - self.trail[1].0 > SMOOTH_S + REST_S {
            self.trail.pop_front();
        }
    }

    /// Where the pointer is drawn at time `t`: `SMOOTH_S` behind, between the bundles; where the mouse is once it
    /// rests.
    fn shown(&self, t: f64) -> (i32, i32) {
        let at = t - SMOOTH_S;
        let mut it = self.trail.iter().rev();
        let Some(&(mut tb, mut xb, mut yb)) = it.next().filter(|b| b.0 > at) else { return (self.x, self.y) };
        for &(ta, xa, ya) in it {
            if ta <= at {
                let f = (at - ta) / (tb - ta);
                let lerp = |a: i32, b: i32| a + ((b - a) as f64 * f).round() as i32;
                return (lerp(xa, xb), lerp(ya, yb));
            }
            (tb, xb, yb) = (ta, xa, ya);
        }
        (xb, yb)
    }
}

impl Ui {
    /// One event of a mouse or keyboard. A key that stands for a front-panel key gives its command (cmd.rs syntax),
    /// which the caller runs: commands are a layer above the screen (ADR 0049).
    pub fn hid(&mut self, e: Event) -> Option<&'static str> {
        match e {
            Event::Move { dx, dy } => {
                self.pointer_show();
                let was = (self.mouse.x, self.mouse.y);
                self.mouse.x = (self.mouse.x + dx).clamp(0, self.scr.w - 1);
                self.mouse.y = (self.mouse.y + dy).clamp(0, self.scr.h - 1);
                self.mouse.moved(clock(), was);
                if self.mouse.left {
                    let (x, y) = (self.mouse.x, self.mouse.y);
                    self.handle(Ev::Touch { id: 0, phase: Phase::Move, x, y });
                }
            }
            Event::Button { button: Button::Left, down } => {
                self.pointer_show();
                let (x, y) = (self.mouse.x, self.mouse.y);
                if down && !self.mouse.left {
                    self.mouse.left = true;
                    self.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
                } else if !down && self.mouse.left {
                    self.mouse.left = false;
                    self.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
                }
            }
            Event::Button { button: Button::Right, down: true } if !self.mouse.left => {
                self.pointer_show();
                let (x, y) = (self.mouse.x, self.mouse.y);
                self.context_menu(x, y);
            }
            Event::Button { .. } => {}
            Event::Wheel { steps } if steps != 0 => {
                self.pointer_show();
                let (x, y) = (self.mouse.x, self.mouse.y);
                let (ctrl, shift, alt) = (self.mouse.ctrl, self.mouse.shift, self.mouse.alt);
                self.handle(Ev::Wheel { x, y, steps, ctrl, shift, alt });
            }
            Event::Wheel { .. } => {}
            Event::Key { code, down, repeat } => return self.hid_key(code, down, repeat),
        }
        None
    }

    /// The pointer comes back (and gets its first place, in the middle of the screen).
    fn pointer_show(&mut self) {
        if !self.mouse.placed {
            self.mouse.placed = true;
            self.mouse.x = self.scr.w / 2;
            self.mouse.y = self.scr.h / 2;
        }
        self.mouse.until = self.now + POINTER_S;
    }

    fn hid_key(&mut self, code: u16, down: bool, repeat: bool) -> Option<&'static str> {
        match code {
            hid::KEY_LEFTSHIFT | hid::KEY_RIGHTSHIFT => self.mouse.shift = down,
            hid::KEY_LEFTCTRL | hid::KEY_RIGHTCTRL => self.mouse.ctrl = down,
            hid::KEY_LEFTALT | hid::KEY_RIGHTALT => self.mouse.alt = down,
            _ => return self.hid_act(code, down, repeat),
        }
        None
    }

    /// A key that is not a modifier.
    fn hid_act(&mut self, code: u16, down: bool, repeat: bool) -> Option<&'static str> {
        if !down || (repeat && !hid::repeats(code)) {
            return None;
        }
        if self.wlan.osk.is_some() && self.osk_hid_key(code, self.mouse.shift) {
            return None;
        }
        // Ctrl with a key is left for later shortcuts; it does not also act as the key alone
        if self.mouse.ctrl {
            return None;
        }
        match code {
            hid::KEY_ESC => self.dismiss_context(),
            hid::KEY_F12 => {
                let p = crate::store::next_name(&self.save_dir(), "shot", "png");
                self.asks.shot = Some(p.to_string_lossy().into_owned());
            }
            _ => return hid::command(code),
        }
        None
    }

    /// The pointer over everything, while the mouse is in use: an arrow with its tip on the point.
    pub(super) fn draw_pointer(&self, cv: &mut Canvas) {
        if !self.mouse.placed || self.now >= self.mouse.until {
            return;
        }
        let (x, y) = self.mouse.shown(clock());
        // head and stem: (0,0) tip, (0,16) and (11,11) the head's corners, the stem from (4,12)/(6,11) to (8,18)
        let arrow = |dx: i32, dy: i32| {
            let p = |a: i32, b: i32| (x + a + dx, y + b + dy);
            [[p(0, 0), p(0, 16), p(11, 11)], [p(4, 12), p(7, 19), p(9, 18)], [p(4, 12), p(9, 18), p(6, 11)]]
        };
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
            for t in arrow(dx, dy) {
                cv.triangle(t, POINTER_EDGE);
            }
        }
        for t in arrow(0, 0) {
            cv.triangle(t, POINTER_FILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mid(r: Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// One event, and the command of a key run, as main.rs does.
    fn feed(u: &mut Ui, e: Event) {
        if let Some(c) = u.hid(e) {
            crate::cmd::run(u, c);
        }
    }

    fn ui() -> Ui {
        let mut u = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        u.step(0.033);
        u
    }

    #[test]
    fn the_mouse_moves_the_pointer_within_the_screen() {
        let mut u = ui();
        feed(&mut u, Event::Move { dx: 10, dy: 0 });
        assert_eq!((u.mouse.x, u.mouse.y), (u.scr.w / 2 + 10, u.scr.h / 2));
        feed(&mut u, Event::Move { dx: -5000, dy: 5000 });
        assert_eq!((u.mouse.x, u.mouse.y), (0, u.scr.h - 1));
    }

    /// Bundles of reports at uneven times (as from the Bluetooth stick on the unit): the drawn pointer moves by even
    /// steps between them, runs `SMOOTH_S` behind, and is where the mouse is once it rests.
    #[test]
    fn the_pointer_moves_evenly_between_bundles() {
        let mut m = Mouse::default();
        // 10 px every 10 ms, at rest at 0 before; the bundles come at 20, 44, 60 and 84 ms, of 2 or 3 reports
        let mut t_x = Vec::new();
        for (t, reports) in [(0.020, 2), (0.044, 3), (0.060, 1), (0.084, 3)] {
            for k in 0..reports {
                let was = (m.x, 0);
                m.x += 10;
                m.moved(t + k as f64 * 0.0001, was);
            }
            t_x.push((t, m.x));
        }
        assert_eq!(m.x, 90);
        // frames every 10 ms from 30 ms: each one moves the pointer, none jumps
        let shown: Vec<i32> = (0..10).map(|k| m.shown(0.030 + k as f64 * 0.010).0).collect();
        for w in shown.windows(2) {
            assert!((0..=16).contains(&(w[1] - w[0])), "even steps: {:?}", shown);
        }
        assert_eq!(m.shown(0.020).0, 0, "at rest before the first bundle, SMOOTH_S later");
        assert_eq!(m.shown(0.084 + SMOOTH_S).0, 90, "the last bundle, SMOOTH_S later");
        assert_eq!(m.shown(1.0), (90, 0), "at rest: where the mouse is");
    }

    #[test]
    fn a_left_click_is_a_tap_and_the_pointer_goes_away_at_rest() {
        let mut u = ui();
        feed(&mut u, Event::Move { dx: 0, dy: 0 });
        let (x, y) = mid(u.cards().0[1]);
        let (dx, dy) = (x - u.mouse.x, y - u.mouse.y);
        feed(&mut u, Event::Move { dx, dy });
        feed(&mut u, Event::Button { button: Button::Left, down: true });
        feed(&mut u, Event::Button { button: Button::Left, down: false });
        assert!(u.m.ch[1].on && u.menu_open && u.menu == MenuId::Channel(1), "CH2's card tapped");
        // the mouse rests: the pointer is drawn where it is (the trail is for a moving one)
        u.mouse.trail.clear();
        let mut cv = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv);
        assert_eq!(cv.px[((y + 6) * u.scr.w + x + 2) as usize] & 0xffffff, 0xffffff, "the pointer is drawn");
        u.step(POINTER_S + 0.1);
        let mut cv2 = Canvas::new(u.scr.w, u.scr.h);
        u.render(&mut cv2);
        assert_ne!(cv2.px[((y + 6) * u.scr.w + x + 2) as usize] & 0xffffff, 0xffffff, "and gone at rest");
    }

    #[test]
    fn keys_act_as_the_front_panel_and_type_into_the_keyboard() {
        let mut u = ui();
        let key = |u: &mut Ui, code: u16| {
            feed(u, Event::Key { code, down: true, repeat: false });
            feed(u, Event::Key { code, down: false, repeat: false });
        };
        key(&mut u, 3); // 2: CH2
        assert!(u.m.ch[1].on);
        key(&mut u, hid::KEY_ESC);
        assert!(!u.menu_open, "Escape closes the menu");
        // a held key that does not repeat acts once
        let run = u.m.run;
        feed(&mut u, Event::Key { code: 57, down: true, repeat: false });
        feed(&mut u, Event::Key { code: 57, down: true, repeat: true });
        feed(&mut u, Event::Key { code: 57, down: false, repeat: false });
        assert_ne!(u.m.run, run, "Space: Run/Stop once");
        // the on-screen keyboard takes the characters, Shift for capitals, Backspace, Enter
        u.wlan_open_osk_for_test();
        feed(&mut u, Event::Key { code: hid::KEY_LEFTSHIFT, down: true, repeat: false });
        key(&mut u, 35); // H
        feed(&mut u, Event::Key { code: hid::KEY_LEFTSHIFT, down: false, repeat: false });
        key(&mut u, 23); // i
        key(&mut u, 2); // 1
        key(&mut u, hid::KEY_BACKSPACE);
        assert_eq!(u.wlan.osk.as_ref().unwrap().text, "Hi");
        key(&mut u, hid::KEY_ESC);
        assert!(u.wlan.osk.is_none(), "Escape cancels the keyboard");
    }

    /// A device that goes with Ctrl or the left button down (input.rs sends `hid::Held::release`) leaves nothing held.
    #[test]
    fn a_device_that_goes_leaves_nothing_held() {
        let mut u = ui();
        let mut held = hid::Held::default();
        let (x, y) = mid(u.cards().0[1]);
        for e in [
            Event::Move { dx: 0, dy: 0 },
            Event::Move { dx: x - u.scr.w / 2, dy: y - u.scr.h / 2 },
            Event::Key { code: hid::KEY_LEFTCTRL, down: true, repeat: false },
            Event::Button { button: Button::Left, down: true },
        ] {
            held.track(&e);
            feed(&mut u, e);
        }
        assert!(u.mouse.left && u.mouse.ctrl);
        for e in held.release(&[]) {
            feed(&mut u, e);
        }
        assert!(!u.mouse.left && !u.mouse.ctrl);
        assert!(u.m.ch[1].on && u.menu_open && u.menu == MenuId::Channel(1), "the touch-up came: CH2's card tapped");
        feed(&mut u, Event::Key { code: 4, down: true, repeat: false });
        assert!(u.m.ch[2].on, "a key acts again: Ctrl is not held");
    }
}
