// SPDX-License-Identifier: GPL-2.0-or-later
//! Display > Layout > Studio: a top bar with Run, Single, the trigger state and the acquisition chip, a bottom bar with
//! one chip per channel, stacked lanes with pinned measurement badges, a measurement panel and the zoom toolbar.
//! Everything here is drawn and tapped from state the other layouts already have (lanes: `workspace`, measurements:
//! `tools`, zoom: `Ui::split_zoom`); Studio only arranges and re-skins it.
use super::*;

#[derive(Default)]
pub(super) struct Studio {
    /// the layout was just chosen: the lanes are stacked at the next frame (`studio_sync`)
    pub enter: bool,
}

/// The rectangles of the Studio top bar.
#[derive(Clone, Copy)]
pub(super) struct StudioTop {
    pub badge: Rect,
    pub run: Rect,
    pub single: Rect,
    pub state: Rect,
    pub acq: Rect,
    pub measure: Rect,
    pub cursors: Rect,
    pub view: Rect,
    pub gear: Rect,
}

const CHIP_H: i32 = 30;
const CHIP_Y: i32 = 5;

/// A play triangle, 12 px high, left edge at `x`.
fn icon_play(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.triangle([(x, cy - 6), (x, cy + 6), (x + 11, cy)], c);
}

fn icon_stop(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.fill(Rect::new(x, cy - 5, 10, 10), c);
}

/// Three bars of different height.
fn icon_bars(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    for (i, h) in [7, 13, 10].into_iter().enumerate() {
        cv.fill(Rect::new(x + i as i32 * 5, cy + 6 - h, 3, h), c);
    }
}

/// Two cursor lines.
fn icon_cursors(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.fill(Rect::new(x, cy - 6, 2, 13), c);
    cv.fill(Rect::new(x + 7, cy - 6, 2, 13), c);
}

/// A screen on a foot.
fn icon_view(cv: &mut Canvas, x: i32, cy: i32, c: Rgb) {
    cv.round_outline(Rect::new(x, cy - 6, 15, 10), 1, c);
    cv.hline(x + 4, x + 10, cy + 6, c);
    cv.vline(x + 7, cy + 4, cy + 6, c);
}

/// A ring with teeth.
fn icon_gear(cv: &mut Canvas, cx: i32, cy: i32, c: Rgb, bg: Rgb) {
    for (dx, dy) in [(0, -8), (0, 8), (-8, 0), (8, 0), (-6, -6), (6, -6), (-6, 6), (6, 6)] {
        cv.fill(Rect::new(cx + dx - 1, cy + dy - 1, 3, 3), c);
    }
    cv.circle(cx, cy, 6, c);
    cv.circle(cx, cy, 4, bg);
    cv.circle(cx, cy, 2, c);
}

/// A chip: the rounded body with a 1 px border.
pub(super) fn chip(cv: &mut Canvas, r: Rect, fill: Rgb, border: Rgb) {
    cv.round(r, 6, border, 255);
    cv.round(r.inset(1), 5, fill, 255);
}

impl Ui {
    /// Applies a layout change made in the Display menu: Studio always draws its sources as lanes.
    pub(super) fn studio_sync(&mut self) {
        if !std::mem::take(&mut self.studio.enter) { return; }
        if self.m.layout == Layout::Studio {
            self.trace_mode(true);
        }
    }

    /// What the badge at the left of the top bar says about the source of the waveforms.
    pub(super) fn studio_source(&self) -> (&'static str, Rgb) {
        if self.record_mode() { return (if self.session_is_sim { "SIM" } else { "REF" }, ACCENT); }
        match self.src {
            Source::Demo { .. } => ("DEMO", ACCENT),
            Source::Replay { .. } => ("REPLAY", ACCENT),
            _ => ("LIVE", GREEN),
        }
    }

    pub(super) fn studio_top(&self) -> StudioTop {
        let f = self.fonts();
        let top = self.frame().top;
        let chip = |x: i32, w: i32| Rect::new(x, CHIP_Y, w, CHIP_H);
        let mut x = top.x + 6;
        let badge = chip(x, (f.sb.width(self.studio_source().0) + 22).max(52));
        x = badge.right() + 8;
        let run = chip(x, 78);
        x = run.right() + 6;
        let single = chip(x, 86);
        x = single.right() + 10;
        let state = chip(x, f.mb.width("TRIG'D").max(f.mb.width("WAIT")) + 22);
        x = state.right() + 8;
        // the right group is laid out from the edge
        let mut r = top.right() - 6;
        let gear = chip(r - 34, 34);
        r = gear.x - 6;
        let view = chip(r - (f.mb.width("View") + 48), f.mb.width("View") + 48);
        r = view.x - 6;
        let cursors = chip(r - (f.mb.width("Cursors") + 46), f.mb.width("Cursors") + 46);
        r = cursors.x - 6;
        let measure = chip(r - (f.mb.width("Measure") + 46), f.mb.width("Measure") + 46);
        let acq = chip(x, (measure.x - 10 - x).clamp(60, 250));
        StudioTop { badge, run, single, state, acq, measure, cursors, view, gear }
    }

    /// Measure and Cursors as the top-bar buttons (`top_buttons`); the others are Studio's own.
    pub(super) fn studio_top_buttons(&self) -> Vec<(TopBtn, &'static str, Rect)> {
        let t = self.studio_top();
        vec![(TopBtn::Menu(MenuId::Measure), "Measure", t.measure), (TopBtn::Menu(MenuId::Cursor), "Cursors", t.cursors)]
    }

    pub(super) fn draw_studio_top(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let top = self.frame().top;
        cv.fill(top, BAR);
        cv.hline(top.x, top.right() - 1, top.bottom() - 1, LINE);
        let t = self.studio_top();
        let root = self.menu_root();
        let open = |m: MenuId| self.menu_open && root == m;
        // the source of the waveforms
        let (label, col) = self.studio_source();
        chip(cv, t.badge, BAR, mix(BAR, col, 120));
        cv.text_in(&f.sb, t.badge, 1, label, col);
        // Run is green while the scope runs; Single arms one acquisition
        let running = !matches!(self.m.run, RunState::Stop) && !matches!(self.m.status, TrigStatus::Stop);
        if running {
            chip(cv, t.run, mix(BAR, GREEN, 70), GREEN);
        } else {
            chip(cv, t.run, BTN, BTN_BORDER);
        }
        icon_play(cv, t.run.x + 14, t.run.y + t.run.h / 2, TEXT);
        cv.text_in(&f.mb, Rect::new(t.run.x + 32, t.run.y, t.run.w - 34, t.run.h), 0, "Run", TEXT);
        chip(cv, t.single, BTN, BTN_BORDER);
        icon_stop(cv, t.single.x + 13, t.single.y + t.single.h / 2, TEXT);
        cv.text_in(&f.mb, Rect::new(t.single.x + 31, t.single.y, t.single.w - 33, t.single.h), 0, "Single", TEXT);
        // trigger state in its own colour
        let (text, scol) = self.run_caption();
        chip(cv, t.state, BAR, scol);
        cv.text_in(&f.mb, t.state, 1, text, scol);
        // sample rate and depth: opens the page that sets them
        let (caption, _) = self.acq_caption();
        let acq_open = self.menu_open && root == if self.record_mode() { MenuId::Ref } else { MenuId::Acquire };
        chip(cv, t.acq, BTN, if acq_open { ACCENT } else { BTN_BORDER });
        cv.set_clip(Rect::new(t.acq.x + 4, t.acq.y, (t.acq.w - 28).max(1), t.acq.h));
        cv.text_in(&f.m, Rect::new(t.acq.x + 10, t.acq.y, (t.acq.w - 32).max(1), t.acq.h), 0, &caption, TEXT);
        cv.no_clip();
        chevron_down(cv, t.acq.right() - 18, t.acq.y + t.acq.h / 2 - 2, DIM);
        // Measure, Cursors, View, settings
        let measuring = open(MenuId::Measure) || self.results_panel();
        let cursors = open(MenuId::Cursor) || self.m.cursor != CursorMode::Off;
        for (r, label, on) in [(t.measure, "Measure", measuring), (t.cursors, "Cursors", cursors), (t.view, "View", open(MenuId::Display))] {
            let c = if on { ACCENT } else { TEXT };
            chip(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            let cy = r.y + r.h / 2;
            match label {
                "Measure" => icon_bars(cv, r.x + 13, cy, c),
                "Cursors" => icon_cursors(cv, r.x + 13, cy, c),
                _ => icon_view(cv, r.x + 12, cy, c),
            }
            cv.text_in(&f.mb, Rect::new(r.x + 34, r.y, r.w - 36, r.h), 0, label, c);
        }
        let gear_on = open(MenuId::Main) || open(MenuId::Utility);
        chip(cv, t.gear, BTN, if gear_on { ACCENT } else { BTN_BORDER });
        icon_gear(cv, t.gear.x + t.gear.w / 2, t.gear.y + t.gear.h / 2, if gear_on { ACCENT } else { TEXT }, BTN);
    }

    /// The bottom bar in Studio: for now the compact cards (replaced in the next step).
    pub(super) fn studio_cards(&self) -> Cards {
        self.compact_cards()
    }

    pub(super) fn draw_studio_bottom(&self, cv: &mut Canvas) {
        self.draw_workspace_footer(cv);
    }

    /// A tap in Studio's own bars and panels. False: the tap belongs to the plot, a menu or a dialog.
    pub(super) fn studio_tap(&mut self, x: i32, y: i32) -> bool {
        self.studio_sync();
        if self.power_dlg || self.wlan.osk.is_some() || self.dialog.is_some() || self.load_slot_ask { return false; }
        if self.menu_open && self.dlg_layout().is_some_and(|d| d.r.contains(x, y)) { return false; }
        if y < self.frame().top.bottom() {
            self.studio_top_tap(x, y);
            return true;
        }
        false
    }

    fn studio_top_tap(&mut self, x: i32, y: i32) {
        let t = self.studio_top();
        let hit = |r: Rect| self.top_hit(r).contains(x, y);
        if hit(t.badge) {
            if self.record_mode() { self.return_to_live(); } else { self.workspace_action(workspace::Action::Sources); }
        } else if hit(t.run) {
            self.key(key::RUNSTOP);
        } else if hit(t.single) {
            self.key(key::SINGLE);
        } else if hit(t.state) {
            self.toggle_menu(MenuId::Trigger);
        } else if hit(t.acq) {
            self.toggle_menu(if self.record_mode() { MenuId::Ref } else { MenuId::Acquire });
        } else if hit(t.measure) {
            self.open_meas_list();
        } else if hit(t.cursors) {
            self.toggle_menu(MenuId::Cursor);
        } else if hit(t.view) {
            self.toggle_menu(MenuId::Display);
        } else if hit(t.gear) {
            self.toggle_menu(MenuId::Main);
        }
    }
}
