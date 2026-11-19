// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Scrolling lists by touch: a scroll bar beside the list (drag its thumb, or tap the track to jump there) and a
//! finger drawn over the list itself. Used by the measurement page, the Results pane, the event tables and a picker
//! longer than the screen (the networks of a WLAN scan, Bluetooth devices). A list keeps its position in its own units
//! (pixels or rows); every read goes through [`Bar::top`], which keeps it in range, so a list that became shorter (a
//! group folded, rows removed) can never be left scrolled past its end.

use super::*;

/// Width of the bar's touch target; the drawn bar is narrower and centred in it.
pub(super) const BAR_W: i32 = 40;
const BAR_DRAWN: i32 = 10;
/// A thumb is never shorter than this, so it stays easy to hit.
const THUMB_MIN: i32 = 40;
/// A finger must move this far before a press on a list becomes a scroll and not a tap.
pub(super) const SLOP: i32 = 10;

/// The lists that scroll.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum List {
    /// the measurement page (pixels)
    Catalog,
    /// the Results pane (rows)
    Results,
    /// the event / search / segment table over the plot (rows)
    Table,
    /// a picker with more options than fit on the screen (lines of options)
    Pick,
}

/// A scroll bar: where it is, how much there is, how much shows, and where the list stands. Units are the list's own.
#[derive(Clone, Copy, Debug)]
pub(super) struct Bar {
    pub track: Rect,
    pub total: i32,
    pub shown: i32,
    pub pos: i32,
}

impl Bar {
    /// Whether the list is longer than its window (otherwise no bar is drawn and nothing scrolls).
    pub fn needed(&self) -> bool {
        self.total > self.shown
    }

    /// The largest position.
    pub fn max(&self) -> i32 {
        (self.total - self.shown).max(0)
    }

    /// The position, kept in range.
    pub fn top(&self) -> i32 {
        self.pos.clamp(0, self.max())
    }

    /// The thumb on the track.
    pub fn thumb(&self) -> Rect {
        let t = self.track;
        if !self.needed() {
            return t;
        }
        let h = ((t.h as i64 * self.shown as i64 / self.total.max(1) as i64) as i32).clamp(THUMB_MIN.min(t.h), t.h);
        let y = t.y + ((t.h - h) as i64 * self.top() as i64 / self.max().max(1) as i64) as i32;
        Rect::new(t.x, y, t.w, h)
    }

    /// The position that puts the thumb's top at `y`.
    pub fn pos_for_thumb(&self, y: i32) -> i32 {
        let free = (self.track.h - self.thumb().h).max(1);
        (((y - self.track.y) as i64 * self.max() as i64 + free as i64 / 2) / free as i64) as i32
    }

    pub fn draw(&self, cv: &mut Canvas) {
        if !self.needed() {
            return;
        }
        let narrow = |r: Rect| Rect::new(r.x + (r.w - BAR_DRAWN) / 2, r.y, BAR_DRAWN, r.h);
        cv.round(narrow(self.track), BAR_DRAWN / 2, BTN, 255);
        cv.round_outline(narrow(self.track), BAR_DRAWN / 2, BTN_BORDER);
        cv.round(narrow(self.thumb()).inset(1), BAR_DRAWN / 2 - 1, LABEL, 255);
    }
}

/// What a finger that went down on a list is doing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Grip {
    /// on the list: it scrolls once it moved past the slop, else it is a tap; `pos` is where the list stood
    Content { pos: i32 },
    /// on the thumb (or the track, after jumping there): `grab` is the finger's distance from the thumb's top
    Thumb { grab: i32 },
}

impl Ui {
    /// The bar and the list area of `l`, when the list is on the screen now.
    pub(super) fn scroll_bar(&self, l: List) -> Option<(Bar, Rect)> {
        match l {
            List::Catalog => self.catalog_bar(),
            List::Results => self.results_bar(),
            List::Table => self.table_bar(),
            List::Pick => self.dlg_layout().and_then(|d| d.bar),
        }
    }

    /// Sets the position of `l` (kept in range).
    pub(super) fn scroll_to(&mut self, l: List, pos: i32) {
        let Some((bar, _)) = self.scroll_bar(l) else { return };
        let p = pos.clamp(0, bar.max());
        match l {
            List::Catalog => self.cat.scroll = p,
            List::Results => self.pn.top = p as usize,
            List::Table => self.table_top = p as usize,
            List::Pick => self.pick_scroll_to(bar, p),
        }
    }

    /// The list a finger went down on, and how it holds it. A press on the track outside the thumb jumps there.
    pub(super) fn scroll_grip(&mut self, x: i32, y: i32) -> Option<(List, Grip)> {
        // the topmost first: the measurement page covers the table, the table covers the pane
        let order: &[List] = if self.menu_open { &[List::Catalog, List::Pick] } else { &[List::Table, List::Results] };
        for &l in order {
            let Some((bar, area)) = self.scroll_bar(l) else { continue };
            if !bar.needed() {
                continue;
            }
            if bar.track.contains(x, y) {
                let th = bar.thumb();
                if th.contains(x, y) {
                    return Some((l, Grip::Thumb { grab: y - th.y }));
                }
                // jump: the thumb centred under the finger, then held there
                let grab = th.h / 2;
                self.scroll_to(l, bar.pos_for_thumb(y - grab));
                return Some((l, Grip::Thumb { grab }));
            }
            if area.contains(x, y) {
                return Some((l, Grip::Content { pos: bar.top() }));
            }
        }
        None
    }

    /// The finger moved to `y` (it went down at `y0`); true once the list scrolls (no tap on lifting then).
    pub(super) fn scroll_move(&mut self, l: List, g: Grip, y0: i32, y: i32) -> bool {
        let Some((bar, _)) = self.scroll_bar(l) else { return false };
        match g {
            Grip::Thumb { grab } => {
                self.scroll_to(l, bar.pos_for_thumb(y - grab));
                true
            }
            Grip::Content { pos } => {
                let dy = y - y0;
                if dy.abs() < SLOP {
                    return false;
                }
                // rows follow the finger by whole rows of the list's pitch
                let units = match l {
                    List::Catalog => dy,
                    List::Results => dy / self.results_row_pitch().max(1),
                    List::Table => dy / tables::ROW_H,
                    List::Pick => dy / (bar.track.h / bar.shown.max(1)).max(1),
                };
                self.scroll_to(l, pos - units);
                true
            }
        }
    }
}

impl Ui {
    /// A long picker scrolled to line `top`: the focus stays on an option that shows (or the window would follow it
    /// back).
    fn pick_scroll_to(&mut self, bar: Bar, top: i32) {
        let Some(d) = self.dlg_layout() else { return };
        let n = d.rows.len();
        // the options per line: those beside the first one that shows
        let y0 = d.rows.iter().find(|r| r.ctrl.w > 0).map_or(0, |r| r.ctrl.y);
        let cols = d.rows.iter().filter(|r| r.ctrl.w > 0 && r.ctrl.y == y0).count().max(1) as i32;
        let Some(p) = self.pick.as_mut() else { return };
        p.top = top;
        let line = (self.focus as i32 / cols).clamp(top, top + bar.shown - 1);
        self.focus = ((line * cols + self.focus as i32 % cols) as usize).min(n.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(ui: &mut Ui, x: i32, y: i32, to: (i32, i32)) {
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        for s in 1..=6 {
            ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x: x + (to.0 - x) * s / 6, y: y + (to.1 - y) * s / 6 });
        }
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: to.0, y: to.1 });
    }

    fn page() -> Ui {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.m.meas.clear();
        ui.key(key::MEASURE);
        ui
    }

    /// The report: open a group at the end, fold it again, and the list stayed scrolled past its end with no way
    /// back. A folded group now leaves every header in reach.
    #[test]
    fn folding_a_group_never_strands_the_list() {
        let mut ui = page();
        let last = crate::measures::groups(0).len() - 1;
        for _ in 0..2 {
            let g = ui.catalog_group_rect(last).unwrap();
            let view = ui.catalog_view();
            let y = g.y.clamp(view.y + 2, view.y + view.h - 2);
            if g.y + g.h > view.y + view.h {
                // reach the last header with the bar first
                let (bar, _) = ui.catalog_bar().unwrap();
                let th = bar.thumb();
                press(&mut ui, th.x + 5, th.y + 5, (th.x + 5, bar.track.y + bar.track.h + 50));
            }
            let g = ui.catalog_group_rect(last).unwrap();
            ui.tap(g.x + 20, g.y + g.h / 2);
            let _ = y;
        }
        assert_eq!(ui.cat.open[0], 0, "folded again");
        let view = ui.catalog_view();
        let first = ui.catalog_group_rect(0).unwrap();
        assert!(
            first.y >= view.y && first.y + first.h <= view.y + view.h,
            "the first header is in view: {first:?} in {view:?}"
        );
        let (bar, _) = ui.catalog_bar().unwrap();
        assert!(!bar.needed() || bar.top() <= bar.max());
    }

    /// The decoder's event table: the bar and a finger scroll it; a press on a row still goes to that event.
    #[test]
    fn the_event_table_scrolls_by_bar_and_finger() {
        // the demo UART on CH3, at the first time base that gives more events than the table shows
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        let mut ui = (0..6)
            .map(|k| {
                let mut ui = crate::ui::testkit::demo_ui();
                ui.m.ch[2].on = true;
                ui.m.dec_on = true;
                ui.m.dec_src = 2;
                crate::cmd::run(&mut ui, &format!("knob hscale +{k}"));
                for _ in 0..4 {
                    ui.step(0.04);
                }
                ui.activate(Item::DecTable);
                ui.render(&mut cv);
                ui
            })
            .find(|ui| ui.table_bar().is_some_and(|(b, _)| b.needed()))
            .expect("a time base with more events than the table shows");
        let (bar, rows) = ui.table_bar().expect("table open");
        let th = bar.thumb();
        press(&mut ui, th.x + 20, th.y + 5, (th.x + 20, bar.track.y + bar.track.h + 50));
        assert_eq!(ui.table_top as i32, bar.max());
        press(&mut ui, rows.x + 100, rows.y + rows.h - 10, (rows.x + 100, rows.y + rows.h - 10 + 3 * tables::ROW_H));
        assert_eq!(ui.table_top as i32, bar.max() - 3, "three rows back by a finger");
        ui.m.run = RunState::Stop;
        let d0 = ui.m.delay;
        press(&mut ui, rows.x + 100, rows.y + 10, (rows.x + 101, rows.y + 12));
        assert_ne!(ui.m.delay, d0, "a press on a row goes to its event");
    }

    /// A finger drawn over the tiles scrolls them and adds nothing; a press without moving is still a tap.
    #[test]
    fn a_finger_scrolls_the_page_and_a_press_still_taps() {
        let mut ui = page();
        ui.cat.open[0] = 0b11111;
        let (bar, view) = ui.catalog_bar().unwrap();
        assert!(bar.needed());
        let (x, y) = (view.x + 60, view.y + view.h - 30);
        press(&mut ui, x, y, (x, y - 150));
        assert_eq!(ui.cat_top(), 150, "the list follows the finger");
        assert!(ui.m.meas.is_empty(), "a scroll adds nothing");
        // the thumb to the bottom is the end of the list
        let (bar, _) = ui.catalog_bar().unwrap();
        let th = bar.thumb();
        press(&mut ui, th.x + 20, th.y + 10, (th.x + 20, bar.track.y + bar.track.h + 100));
        assert_eq!(ui.cat_top(), bar.max());
        // a press on the track jumps there
        press(&mut ui, th.x + 20, bar.track.y + 1, (th.x + 20, bar.track.y + 1));
        assert_eq!(ui.cat_top(), 0);
        // a press on a tile without moving adds it
        let at = ui.catalog_tile_rect(crate::measures::std_id(Meas::Vamp)).unwrap();
        press(&mut ui, at.x + 10, at.y + at.h / 2, (at.x + 12, at.y + at.h / 2 + 3));
        assert_eq!(ui.m.meas.len(), 1);
    }

    #[test]
    fn the_thumb_spans_the_track_by_position() {
        let b = Bar { track: Rect::new(0, 100, 40, 200), total: 400, shown: 100, pos: 0 };
        assert_eq!(b.thumb(), Rect::new(0, 100, 40, 50));
        let end = Bar { pos: 10_000, ..b };
        assert_eq!(end.top(), 300, "a position past the end reads as the end");
        assert_eq!(end.thumb().y + end.thumb().h, 300);
        assert_eq!(b.pos_for_thumb(250), 300);
        assert_eq!(b.pos_for_thumb(100), 0);
        assert_eq!(b.pos_for_thumb(175), 150);
        let short = Bar { total: 80, ..b };
        assert!(!short.needed() && short.top() == 0);
        let long = Bar { total: 100_000, ..b };
        assert_eq!(long.thumb().h, THUMB_MIN, "a long list keeps a thumb that can be hit");
    }
}
