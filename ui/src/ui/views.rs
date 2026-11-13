// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Display > Views: up to four arrangements of the screen to switch between, instead of one screen with everything on
//! it. A view keeps what is shown and where: the windows, the second pane, the measurements and their bar, the DVM and
//! the counter, the cursors, the zoom, the histogram, the analyser page, XY and full screen. The channels, the time
//! base, the trigger and the acquisition are the same in every view. What changes while a view is shown stays with
//! it; a view not used yet starts with the plot alone.
//!
//! Switching: the numbered buttons in the top bar, a swipe from the left or right edge of the plot inwards (only
//! while there is more than one view; a vertical drag there still moves a channel or the trigger level), the Quick
//! key set to "Next view", SCPI `:DISPlay:VIEW`. The views are part of a setup (`views`, `view1` ...).

use super::*;
use crate::store;

/// The most views.
pub const VIEWS_MAX: usize = 4;
/// Keys of a setup (store.rs) that belong to a view, besides `xy` and `full` (fields of the Ui).
const KEYS: [&str; 21] = [
    "windows", "ch_window", "math_window", "la_window", "meas", "meas2", "dvm", "counter", "cursor", "cur_a", "cur_b", "cur_c", "cur_d", "pane", "meas_labels", "trend_sel", "zoom", "zoom_tdiv", "zdelay", "hist", "view",
];
/// The strip at each edge of the plot where a horizontal swipe switches views (px).
pub(super) const EDGE: i32 = 18;
/// How far the swipe has to go (px).
const SWIPE: i32 = 70;
/// Width of a number in the top bar's view buttons.
const NUM_W: i32 = 28;

fn known(k: &str) -> bool {
    KEYS.contains(&k) || k == "xy" || k == "full"
}

/// A view as kept in a setup: `key=value; key=value ...` of its keys only.
pub fn slot_checked(v: &str) -> Result<String, String> {
    for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let k = part.split_once('=').map(|(k, _)| k.trim()).ok_or_else(|| format!("not key=value: {}", part))?;
        if !known(k) {
            return Err(format!("not part of a view: {}", k));
        }
    }
    Ok(v.trim().to_string())
}

/// The view of a model and the Ui's XY and full screen.
fn snapshot_of(m: &Model, xy: bool, full: bool) -> String {
    let text = store::setup_text(m);
    let mut parts: Vec<String> = text.lines().filter_map(|l| l.split_once(" = ")).filter(|(k, _)| KEYS.contains(k)).map(|(k, v)| format!("{}={}", k, v)).collect();
    parts.push(format!("xy={}", xy));
    parts.push(format!("full={}", full));
    parts.join("; ")
}

impl Ui {
    /// What the shown view is now.
    pub(super) fn view_snapshot(&self) -> String {
        snapshot_of(&self.m, self.xy, self.full)
    }

    /// Shows a kept view (empty: the plot alone, as after a reset).
    fn view_apply(&mut self, slot: &str) {
        let clean;
        let slot = if slot.trim().is_empty() {
            clean = snapshot_of(&Model::default(), false, false);
            &clean
        } else {
            slot
        };
        for part in slot.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let Some((k, v)) = part.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "xy" => self.xy = v == "true",
                "full" => self.full = v == "true",
                _ if KEYS.contains(&k) => {
                    if let Err(e) = store::set(&mut self.m, k, v) {
                        eprintln!("oos-ui: view: {}: {}", k, e);
                    }
                }
                _ => {}
            }
        }
        if self.m.zscale > self.m.tscale {
            self.m.zscale = self.m.tscale;
        }
        self.settings_changed();
    }

    /// Shows view `j` (0-based); the one shown so far keeps its state.
    pub(crate) fn view_go(&mut self, j: usize) {
        let n = self.m.views;
        if n < 2 || j >= n || j == self.m.view_cur {
            return;
        }
        let cur = self.m.view_cur.min(n - 1);
        self.m.view_slots[cur] = self.view_snapshot();
        let s = std::mem::take(&mut self.m.view_slots[j]);
        self.view_apply(&s);
        self.m.view_cur = j;
        self.toast(&format!("View {} of {}", j + 1, n));
    }

    /// The next (`d` = 1) or previous view, round.
    pub(crate) fn view_step(&mut self, d: i32) {
        let n = self.m.views;
        if n >= 2 {
            self.view_go((self.m.view_cur as i32 + d).rem_euclid(n as i32) as usize);
        }
    }

    /// Display > Views: Off, 2, 3, 4. Fewer views: the ones dropped are forgotten; the shown one stays on screen.
    pub(crate) fn views_set(&mut self, n: usize) {
        let n = n.clamp(1, VIEWS_MAX);
        if n < self.m.views && self.m.view_cur >= n {
            if n >= 2 {
                self.view_go(n - 1);
            } else {
                self.m.view_cur = 0;
            }
        }
        if n == 1 {
            self.m.view_cur = 0;
        }
        for s in self.m.view_slots.iter_mut().skip(n) {
            s.clear();
        }
        if n == 1 {
            self.m.view_slots[0].clear();
        }
        self.m.views = n;
    }

    // ------------------------------------------------------------------------------------------------ the menu item

    pub(super) fn views_kind(it: Item) -> Option<Kind> {
        (it == Item::Views).then_some(Kind::Choice)
    }

    pub(super) fn views_label(it: Item) -> Option<&'static str> {
        (it == Item::Views).then_some("Views")
    }

    pub(super) fn views_value(&self, it: Item) -> Option<String> {
        (it == Item::Views).then(|| if self.m.views < 2 { "Off".to_string() } else { self.m.views.to_string() })
    }

    pub(super) fn views_options(&self, it: Item) -> Option<Vec<String>> {
        (it == Item::Views).then(|| (1..=VIEWS_MAX).map(|n| if n == 1 { "Off".to_string() } else { n.to_string() }).collect())
    }

    pub(super) fn views_adjust(&mut self, it: Item, d: i32) -> bool {
        if it != Item::Views {
            return false;
        }
        self.views_set((self.m.views as i32 + d).clamp(1, VIEWS_MAX as i32) as usize);
        true
    }

    // ------------------------------------------------------------------------------------------------ the top bar

    /// The numbered buttons in the top bar, left of the menu buttons, while there is more than one view.
    pub(super) fn view_box(&self) -> Option<Rect> {
        if self.m.views < 2 {
            return None;
        }
        let first = self.top_buttons().first().map_or(self.top_right(), |b| b.2.x);
        let w = NUM_W * self.m.views as i32 + 2;
        Some(Rect::new(first - 8 - w, 5, w, 30))
    }

    pub(super) fn draw_view_box(&self, cv: &mut Canvas) {
        let Some(r) = self.view_box() else { return };
        let f = self.fonts();
        cv.round(r, 5, BTN_BORDER, 255);
        cv.round(r.inset(1), 4, BTN, 255);
        for i in 0..self.m.views {
            let c = Rect::new(r.x + 1 + NUM_W * i as i32, r.y + 1, NUM_W, r.h - 2);
            let on = i == self.m.view_cur;
            if on {
                cv.round(c.inset(2), 3, ACCENT, 255);
            }
            cv.text_in(&f.mb, c, 1, &(i + 1).to_string(), if on { BG } else { TEXT });
        }
    }

    /// A tap at (x, y) on the view buttons: that view.
    pub(super) fn view_box_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(r) = self.view_box() else { return false };
        if !self.top_hit(r).contains(x, y) {
            return false;
        }
        let i = ((x - r.x - 1) / NUM_W).clamp(0, self.m.views as i32 - 1) as usize;
        self.view_go(i);
        true
    }

    // ------------------------------------------------------------------------------------------------ the swipe

    /// A finger went down at x in the plot: whether it is in an edge strip where a swipe switches views.
    pub(super) fn view_edge(&self, x: i32) -> bool {
        self.m.views >= 2 && (x < self.grid.x + EDGE || x >= self.grid.x + self.grid.w - EDGE)
    }

    /// The finger moved by (dx, dy) from x0 in an edge strip: Some(true) switched, Some(false) still open, None it is
    /// not a swipe (a vertical drag: the usual drags take it).
    pub(super) fn view_swipe(&mut self, x0: i32, dx: i32, dy: i32) -> Option<bool> {
        if dy.abs() > 10 && dy.abs() >= dx.abs() {
            return None;
        }
        if dx.abs() < SWIPE {
            return Some(false);
        }
        let from_left = x0 < self.grid.x + EDGE;
        // inwards only: from the left edge to the right shows the view before, from the right edge to the left the next
        match (from_left, dx > 0) {
            (true, true) => self.view_step(-1),
            (false, false) => self.view_step(1),
            _ => return None,
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.step(1.0 / 30.0);
        u
    }

    #[test]
    fn each_view_keeps_its_arrangement_and_shares_the_rest() {
        let mut u = ui();
        u.views_set(3);
        u.m.meas = vec![(Meas::Freq, 0), (Meas::Vpp, 0)];
        u.m.zoom = true;
        let tdiv = u.m.tscale;
        u.view_go(1);
        // a new view: the plot alone, the time base as it was
        assert!(u.m.meas.is_empty() && !u.m.zoom && u.m.view_cur == 1 && u.m.tscale == tdiv);
        u.m.pane = 1;
        u.xy = true;
        u.view_go(0);
        assert_eq!(u.m.meas, vec![(Meas::Freq, 0), (Meas::Vpp, 0)]);
        assert!(u.m.zoom && u.m.pane == 0 && !u.xy);
        u.view_step(1);
        assert!(u.m.pane == 1 && u.xy && u.m.meas.is_empty());
        // round: from the last to the first
        u.view_step(1);
        u.view_step(1);
        assert_eq!(u.m.view_cur, 0);
    }

    #[test]
    fn views_go_with_a_setup() {
        let mut u = ui();
        u.views_set(2);
        u.m.meas = vec![(Meas::Freq, 0)];
        u.view_go(1);
        u.m.cursor = CursorMode::Time;
        let text = store::setup_text(&u.m);
        assert!(text.contains("views = 2 2"), "{}", text);
        let m = store::parse_setup(&text).unwrap();
        assert_eq!((m.views, m.view_cur), (2, 1));
        assert!(m.view_slots[0].contains("meas=Freq:1"), "{}", m.view_slots[0]);
        assert!(m.view_slots[1].is_empty());
        // a slot with a key that is not part of a view is refused
        assert!(store::parse_setup("view1 = tdiv=3").is_err());
        assert!(store::parse_setup("views = 5 1").is_err());
    }

    #[test]
    fn fewer_views_keep_the_screen() {
        let mut u = ui();
        u.views_set(3);
        u.view_go(2);
        u.m.zoom = true;
        u.views_set(2);
        // view 3 is gone: view 2 is shown, as it was kept
        assert_eq!(u.m.view_cur, 1);
        assert!(!u.m.zoom);
        u.views_set(1);
        assert_eq!((u.m.views, u.m.view_cur), (1, 0));
        assert!(u.m.view_slots.iter().all(|s| s.is_empty()));
        // off: nothing switches
        u.view_step(1);
        assert_eq!(u.m.view_cur, 0);
    }

    #[test]
    fn edge_swipes_and_buttons_switch_views() {
        let mut u = ui();
        let g = u.grid;
        let y = g.y + g.h / 2;
        // off: a swipe from the right edge moves the waveform as always
        let d0 = u.m.delay;
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: g.x + g.w - 5, y });
        u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: g.x + g.w - 200, y });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: g.x + g.w - 200, y });
        assert!(u.m.delay != d0 && u.m.view_cur == 0);
        u.m.delay = d0;
        u.views_set(3);
        let swipe = |u: &mut Ui, x0: i32, x1: i32| {
            u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y });
            for s in 1..=4 {
                u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: x0 + (x1 - x0) * s / 4, y });
            }
            u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: x1, y });
        };
        swipe(&mut u, g.x + g.w - 5, g.x + g.w - 200);
        assert_eq!(u.m.view_cur, 1);
        assert_eq!(u.m.delay, d0);
        swipe(&mut u, g.x + 4, g.x + 200);
        assert_eq!(u.m.view_cur, 0);
        // in the middle it is still the time position
        swipe(&mut u, g.x + g.w / 2, g.x + g.w / 2 - 150);
        assert!(u.m.delay != d0 && u.m.view_cur == 0);
        // a vertical drag at the left edge still moves a channel
        let c = u.m.active;
        let off = u.m.ch[c].offset;
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: g.x + 4, y: u.y_of_volts(c, 0.0) });
        u.handle(Ev::Touch { id: 0, phase: Phase::Move, x: g.x + 6, y: u.y_of_volts(c, 0.0) + 60 });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: g.x + 6, y: u.y_of_volts(c, 0.0) + 60 });
        assert!(u.m.ch[u.m.active].offset != off || u.m.active != c);
        // the buttons in the top bar
        let r = u.view_box().unwrap();
        u.handle(Ev::Touch { id: 0, phase: Phase::Down, x: r.x + 2 * NUM_W + 10, y: r.y + 15 });
        u.handle(Ev::Touch { id: 0, phase: Phase::Up, x: r.x + 2 * NUM_W + 10, y: r.y + 15 });
        assert_eq!(u.m.view_cur, 2);
        // they leave room for the menu buttons
        assert!(r.x + r.w <= u.top_buttons()[0].2.x);
    }

    #[test]
    fn views_by_scpi() {
        let mut u = ui();
        let cv = Canvas::new(PANEL_W, PANEL_H);
        let mut scpi = crate::scpi::Scpi::open(0);
        assert_eq!(scpi.exec_line(&mut u, &cv, ":DISP:VIEW:COUN?"), b"1");
        scpi.exec_line(&mut u, &cv, ":DISP:VIEW:COUN 3");
        scpi.exec_line(&mut u, &cv, ":DISPlay:VIEW 3");
        assert_eq!(scpi.exec_line(&mut u, &cv, ":DISP:VIEW?"), b"3");
        assert_eq!(u.m.view_cur, 2);
        scpi.exec_line(&mut u, &cv, ":DISP:VIEW 4");
        assert_eq!(u.m.view_cur, 2);
        assert!(scpi.exec_line(&mut u, &cv, ":SYST:ERR?").starts_with(b"-224"));
    }

    #[test]
    fn the_quick_key_steps_through_the_views() {
        let mut u = ui();
        u.views_set(2);
        u.m.quick = Quick::NextView;
        u.key(key::QUICK);
        assert_eq!(u.m.view_cur, 1);
        u.key(key::QUICK);
        assert_eq!(u.m.view_cur, 0);
    }
}
