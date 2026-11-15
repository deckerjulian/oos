// SPDX-License-Identifier: GPL-2.0-or-later
//! Display > Layout > Studio: a top bar with Run, Single, the trigger state and the acquisition chip, a bottom bar with
//! one chip per channel, stacked lanes with pinned measurement badges, a measurement panel and the zoom toolbar.
//! Everything here is drawn and tapped from state the other layouts already have (lanes: `workspace`, measurements:
//! `tools`, zoom: `Ui::split_zoom`); Studio only arranges and re-skins it.
use super::*;

/// One measurement as a badge or a panel row remembers it: by what it measures, so it survives other measurements
/// being added or removed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum MeasKey {
    One(Meas, WaveId),
    Pair(usize, WaveId, WaveId),
    Cat(u16, WaveId, WaveId, WaveId, WaveId),
}

/// How the measurements share the screen (View menu).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum StudioView {
    /// the lanes and the pinned badges only
    #[default]
    Waveform,
    /// the panel at the right of the lanes
    Docked,
    /// the panel over the lanes
    Floating,
    /// the table of all measurements instead of the lanes
    Only,
}

#[derive(Default)]
pub(super) struct Studio {
    /// the layout was just chosen: the lanes are stacked at the next frame (`studio_sync`)
    pub enter: bool,
    /// the measurements shown as badges; None until one is pinned or unpinned: the first three
    pub pinned: Option<Vec<MeasKey>>,
    /// the badge row is hidden (eye button, View > Show pinned badges)
    pub hide_badges: bool,
    /// the badge whose action popup is open
    pub popup: Option<MeasKey>,
    pub view: StudioView,
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

/// The rectangles of the Studio bottom bar.
#[derive(Clone, Copy)]
pub(super) struct StudioBottom {
    pub ch: [Rect; NCH],
    pub time: Rect,
    pub trig: Rect,
    pub panel: Option<Rect>,
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
        let gear_on = open(MenuId::Main) || open(MenuId::Utility) || self.m.touch_lock;
        chip(cv, t.gear, BTN, if gear_on { ACCENT } else { BTN_BORDER });
        icon_gear(cv, t.gear.x + t.gear.w / 2, t.gear.y + t.gear.h / 2, if gear_on { ACCENT } else { TEXT }, BTN);
    }

    /// The rectangles of the Studio bottom bar: a chip per channel, the time base and the trigger.
    pub(super) fn studio_bottom(&self) -> StudioBottom {
        let bottom = self.frame().bottom;
        let (y, h) = (bottom.y + 3, CHIP_H);
        let n = n_ch();
        let mut right = bottom.right() - 4;
        let panel = self.desktop_panel.map(|_| { right -= 46; let r = Rect::new(right, y, 46, h); right -= 4; r });
        let trig_w = 196;
        let trig = Rect::new(right - trig_w, y, trig_w, h);
        let time = Rect::new(trig.x - 9 - 150, y, 150, h);
        // the channels share what is left of the row, an off channel a little less than one that is on
        let room = time.x - 9 - (bottom.x + 4);
        let on = (0..n).filter(|&c| self.m.ch[c].on).count() as i32;
        let (w_on, w_off) = (170, 118);
        let want = on * w_on + (n as i32 - on) * w_off + (n as i32 - 1) * 5;
        let mut ch = [Rect::new(0, y, 0, h); NCH];
        let mut x = bottom.x + 4;
        for (c, r) in ch.iter_mut().enumerate().take(n) {
            let w = if self.m.ch[c].on { w_on } else { w_off };
            let w = if want > room { w * room / want } else { w };
            *r = Rect::new(x, y, w, h);
            x += w + 5;
        }
        StudioBottom { ch, time, trig, panel }
    }

    pub(super) fn studio_cards(&self) -> Cards {
        let b = self.studio_bottom();
        Cards { ch: b.ch, la: None, gen: None, time: b.time, trig: b.trig }
    }

    pub(super) fn draw_studio_bottom(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let bottom = self.frame().bottom;
        cv.fill(bottom, BAR);
        self.draw_studio_badges(cv);
        cv.hline(bottom.x, bottom.right() - 1, bottom.y, LINE);
        let b = self.studio_bottom();
        let root = if self.menu_open { Some(self.menu_root()) } else { None };
        for c in 0..n_ch() {
            let r = b.ch[c];
            let (col, ch) = (CH_COLORS[c], &self.m.ch[c]);
            let sel = root == Some(MenuId::Channel(c));
            let border = if sel { ACCENT } else if ch.on { col } else { BTN_BORDER };
            chip(cv, r, if ch.on { BTN } else { BAR }, border);
            // the channel badge: filled while the channel is on
            let badge = Rect::new(r.x + 8, r.y + 5, 42, 20);
            cv.round(badge, 4, if ch.on { col } else { mix(BAR, col, 70) }, 255);
            cv.text_in(&f.sb, badge, 1, &format!("CH{}", c + 1), if ch.on { DARK_TEXT } else { mix(BAR, col, 200) });
            let text = if ch.on { format!("{}/div", si_short(ch.volts_per_div() as f64, ch.unit.name())) } else { "Off".into() };
            cv.set_clip(Rect::new(badge.right() + 2, r.y, (r.right() - 18 - badge.right() - 2).max(1), r.h));
            cv.text_in(&f.mb, Rect::new(badge.right() + 8, r.y, (r.right() - 20 - badge.right() - 8).max(1), r.h), 0, &text, if ch.on { TEXT } else { DIM });
            cv.no_clip();
            chevron_down(cv, r.right() - 16, r.y + r.h / 2 - 2, DIM);
        }
        // the sections of the row are told apart by a line
        for x in [b.time.x - 5, b.trig.x - 5] {
            cv.vline(x, bottom.y + 6, bottom.bottom() - 6, LINE);
        }
        // time base
        let r = b.time;
        chip(cv, r, BTN, if root == Some(MenuId::Horizontal) { ACCENT } else { BTN_BORDER });
        let cy = r.y + r.h / 2;
        cv.hline(r.x + 10, r.x + 24, cy, TEXT);
        cv.line(r.x + 10, cy, r.x + 14, cy - 3, TEXT);
        cv.line(r.x + 10, cy, r.x + 14, cy + 3, TEXT);
        cv.line(r.x + 24, cy, r.x + 20, cy - 3, TEXT);
        cv.line(r.x + 24, cy, r.x + 20, cy + 3, TEXT);
        cv.text_in(&f.mb, Rect::new(r.x + 32, r.y, r.w - 52, r.h), 0, &format!("{}/div", si_short(self.m.time_per_div() as f64, "s")), TEXT);
        chevron_down(cv, r.right() - 16, cy - 2, DIM);
        // trigger: the slope, the source and the level
        let r = b.trig;
        chip(cv, r, BTN, if root == Some(MenuId::Trigger) { ACCENT } else { BTN_BORDER });
        let source = self.m.trigger_wave();
        let col = self.wave_color(source);
        let edge = matches!(self.m.trig_type, TrigType::Edge | TrigType::Slope | TrigType::Timeout | TrigType::NthEdge | TrigType::Delay);
        let mut x = r.x + 9;
        cv.set_clip(r.inset(2));
        if edge {
            draw_slope(cv, x, r.y + 7, self.m.slope, col);
            x += 20;
        } else {
            x = cv.text(&f.sb, x, r.y + 20, &self.m.trig_type.name().to_uppercase(), LABEL) + 6;
        }
        x = cv.text(&f.mb, x, r.y + 20, &self.trigger_source_label(), col) + 6;
        let levels = if matches!(self.m.trig_type, TrigType::Runt | TrigType::Slope | TrigType::Window) {
            format!("{} / {}", si_short(self.m.trig_level2 as f64, "V"), si_short(self.m.trig_level as f64, "V"))
        } else { si_short(self.m.trig_level as f64, "V") };
        if edge && self.m.slope != Slope::Either {
            // the arrow of the edge: up for rising, down for falling
            let (top, bot) = (r.y + 9, r.y + 21);
            cv.vline(x + 3, top, bot, TEXT);
            let (tip, base) = if self.m.slope == Slope::Rise { (top, top + 4) } else { (bot, bot - 4) };
            cv.line(x + 3, tip, x, base, TEXT);
            cv.line(x + 3, tip, x + 6, base, TEXT);
            x += 12;
        }
        cv.text(&f.mb, x, r.y + 20, &levels, TEXT);
        cv.no_clip();
        chevron_down(cv, r.right() - 16, r.y + r.h / 2 - 2, DIM);
        if let Some(r) = b.panel {
            let on = self.desktop_panel == Some(true);
            chip(cv, r, BTN, if on { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.s, r, 1, "Panel", if on { ACCENT } else { TEXT });
        }
    }

    fn studio_bottom_tap(&mut self, x: i32, y: i32) {
        let b = self.studio_bottom();
        for c in 0..n_ch() {
            if !b.ch[c].contains(x, y) { continue; }
            if !self.m.ch[c].on {
                self.m.ch[c].on = true;
                self.settings_changed();
                self.relayout();
            }
            self.select_workspace_source(WaveId::Ch(c));
            return self.toggle_menu(MenuId::Channel(c));
        }
        if b.time.contains(x, y) {
            self.toggle_menu(MenuId::Horizontal);
        } else if b.trig.contains(x, y) {
            self.toggle_menu(MenuId::Trigger);
        } else if b.panel.is_some_and(|r| r.contains(x, y)) {
            self.desktop_panel = self.desktop_panel.map(|open| !open);
        }
    }

    /// A tap in Studio's own bars and panels. False: the tap belongs to the plot, a menu or a dialog.
    pub(super) fn studio_tap(&mut self, x: i32, y: i32) -> bool {
        self.studio_sync();
        if self.power_dlg || self.wlan.osk.is_some() || self.dialog.is_some() || self.load_slot_ask { return false; }
        if self.menu_open && self.dlg_layout().is_some_and(|d| d.r.contains(x, y)) { return false; }
        if self.studio_popup_tap(x, y) { return true; }
        if self.studio_badge_tap(x, y) { return true; }
        if y >= self.frame().bottom.y {
            self.studio_bottom_tap(x, y);
            return true;
        }
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
            if self.m.touch_lock { self.key(key::TOUCHLOCK); } else { self.toggle_menu(MenuId::Main); }
        }
    }
}

/// The cursor band and its C1 / C2 tags.
const BAND: Rgb = rgb(0x2d, 0xd4, 0xcf);
/// Height of the row above the lanes that carries the T marker and the cursor tags.
const HEAD_H: i32 = 18;

impl Ui {
    /// Studio keeps a row above the lanes for the trigger position and the cursor tags.
    pub(super) fn studio_head_h(&self) -> i32 {
        if self.m.layout == Layout::Studio && !self.m.zoom && !self.full_on() { HEAD_H } else { 0 }
    }

    /// Screen x of a time (relative to the trigger) in the main view of the plot.
    pub(super) fn studio_x_of_time(&self, t: f64) -> i32 {
        let g = self.grid;
        let (tpd, delay) = self.view();
        g.x + ((0.5 + (t - delay as f64) / (HDIV as f64 * tpd as f64)) * g.w as f64).round() as i32
    }

    /// The channel badge at the left edge of a lane, on the channel's ground line (`draw_markers`).
    pub(super) fn studio_lane_badge(&self, cv: &mut Canvas, c: usize, y: i32) {
        let a = self.area;
        let col = CH_COLORS[c];
        let r = Rect::new(a.x + 3, y - 9, 44, 18);
        cv.round(r, 4, col, 255);
        cv.round(r.inset(1), 3, BG, 255);
        cv.text_in(&self.fonts().sb, r, 1, &format!("CH{}", c + 1), col);
    }

    /// The T marker over the lanes, the cursor tags and the shaded band between the cursors.
    pub(super) fn draw_studio_overlay(&self, cv: &mut Canvas) {
        let head = self.studio_head_h();
        if head == 0 || self.view_on() || self.eye_open() { return; }
        let f = self.fonts();
        let g = self.grid;
        let row = Rect::new(g.x, g.y - head, g.w, head);
        cv.set_clip(Rect::new(g.x - 12, row.y, g.w + 24, g.h + head));
        // the shaded band between two time cursors
        if matches!(self.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) {
            let xs = [self.m.cur_a, self.m.cur_b].map(|frac| g.x + (frac * g.w as f32) as i32);
            let (x0, x1) = (xs[0].min(xs[1]), xs[0].max(xs[1]));
            cv.set_clip(Rect::new(g.x, g.y, g.w, g.h));
            for &lane in &self.wins {
                cv.blend(Rect::new(x0, lane.y, (x1 - x0).max(1), lane.h), BAND, 46);
            }
            cv.set_clip(Rect::new(g.x - 12, row.y, g.w + 24, g.h + head));
            for (x, name) in [(xs[0], "C1"), (xs[1], "C2")] {
                let r = Rect::new((x - 11).clamp(g.x, g.right() - 22), row.y + 1, 22, head - 3);
                cv.round(r, 3, BAND, 255);
                cv.text_in(&f.sb, r, 1, name, DARK_TEXT);
                cv.vline(x, row.bottom() - 2, g.y, BAND);
            }
        }
        // T: where the trigger is in the record, dashed through every lane; at an edge when it is off the screen
        let tx = self.studio_x_of_time(0.0);
        let r = Rect::new(tx.clamp(g.x + 11, g.right() - 11) - 11, row.y + 1, 22, head - 3);
        cv.round(r, 3, AMBER, 255);
        cv.round(r.inset(1), 2, BG, 255);
        cv.text_in(&f.sb, r, 1, "T", AMBER);
        if (g.x..g.right()).contains(&tx) {
            cv.triangle([(tx - 3, r.bottom()), (tx + 3, r.bottom()), (tx, r.bottom() + 3)], AMBER);
            for &lane in &self.wins {
                cv.dashed_vline(tx, lane.y, lane.y + lane.h - 1, 4, 4, AMBER);
            }
        } else if tx < g.x {
            cv.triangle([(r.x - 1, r.y + r.h / 2), (r.x + 4, r.y + 2), (r.x + 4, r.bottom() - 2)], AMBER);
        } else {
            cv.triangle([(r.right() + 1, r.y + r.h / 2), (r.right() - 4, r.y + 2), (r.right() - 4, r.bottom() - 2)], AMBER);
        }
        cv.no_clip();
    }
}

/// Height of the row of pinned badges under the lanes.
const ROW_H: i32 = 34;
/// Width of the action popup of a badge, and the height of its rows.
const POPUP_W: i32 = 300;
const POPUP_ROW: i32 = 34;

fn icon_pin(cv: &mut Canvas, x: i32, cy: i32, filled: bool, c: Rgb) {
    // a round head on a needle that leans to the left
    cv.line(x + 6, cy - 1, x + 1, cy + 6, c);
    cv.circle(x + 8, cy - 3, 3, c);
    if filled {
        cv.fill(Rect::new(x + 4, cy - 1, 8, 2), c);
    } else {
        cv.circle(x + 8, cy - 3, 1, BTN);
        cv.hline(x + 4, x + 11, cy - 1, c);
    }
}

fn icon_eye(cv: &mut Canvas, cx: i32, cy: i32, c: Rgb) {
    cv.round_outline(Rect::new(cx - 8, cy - 5, 17, 11), 5, c);
    cv.circle(cx, cy, 2, c);
}

impl Ui {
    /// The measurements in the order of the list: single, pair, catalogue.
    pub(super) fn studio_keys(&self) -> Vec<MeasKey> {
        let m = &self.m;
        m.meas.iter().map(|&(k, id)| MeasKey::One(k, id))
            .chain(m.meas2.iter().map(|&(k, a, b)| MeasKey::Pair(k, a, b)))
            .chain(m.cat.iter().map(|&(i, a, b, c, d)| MeasKey::Cat(i, a, b, c, d)))
            .collect()
    }

    /// The measurements shown as badges: those pinned, or the first three until the user pins one.
    pub(super) fn studio_pinned(&self) -> Vec<MeasKey> {
        let keys = self.studio_keys();
        match &self.studio.pinned {
            Some(pinned) => keys.into_iter().filter(|k| pinned.contains(k)).collect(),
            None => keys.into_iter().take(3).collect(),
        }
    }

    pub(super) fn studio_is_pinned(&self, key: MeasKey) -> bool {
        self.studio_pinned().contains(&key)
    }

    pub(super) fn studio_set_pin(&mut self, key: MeasKey, on: bool) {
        let mut pinned = self.studio_pinned();
        pinned.retain(|k| *k != key);
        if on { pinned.push(key); }
        self.studio.pinned = Some(pinned);
        self.relayout();
    }

    /// Index of a measurement in the list (`meas_rows`).
    pub(super) fn studio_index(&self, key: MeasKey) -> Option<usize> {
        self.studio_keys().iter().position(|k| *k == key)
    }

    pub(super) fn studio_owns_measurements(&self) -> bool {
        self.m.layout == Layout::Studio
    }

    /// Whether the row under the lanes is there: pinned measurements, or the cursor read-out, unless hidden.
    pub(super) fn studio_badges_visible(&self) -> bool {
        self.m.layout == Layout::Studio && !self.studio.hide_badges && self.studio.view != StudioView::Only
            && (!self.studio_pinned().is_empty() || self.m.cursor != CursorMode::Off)
    }

    /// Height the badge row takes from the lanes.
    pub(super) fn studio_row_h(&self) -> i32 {
        if self.studio_badges_visible() { ROW_H } else { 0 }
    }

    /// The badge row: under the lanes, as wide as they are.
    pub(super) fn studio_row(&self) -> Rect {
        let (x, w) = self.plot_span();
        Rect::new(x, self.frame().bottom.y - ROW_H, w, ROW_H)
    }

    /// The eye button at the right end of the row.
    pub(super) fn studio_eye(&self) -> Rect {
        let r = self.studio_row();
        Rect::new(r.right() - 38, r.y + 3, 34, 28)
    }

    /// The row of badges as (measurement, rectangle), as many as fit.
    pub(super) fn studio_badges(&self) -> Vec<(MeasKey, Rect)> {
        let f = self.fonts();
        let row = self.studio_row();
        let rows = self.meas_rows();
        let limit = self.studio_eye().x - 6;
        let mut x = row.x + 4;
        let mut out = Vec::new();
        for key in self.studio_pinned() {
            let Some(r) = self.studio_index(key).and_then(|i| rows.get(i)) else { continue };
            let w = 14 + 6 + (f.sb.width("CH4") + 10) + 6 + f.s.width(&r.name) + 8 + f.sb.width(&r.text()).max(f.sb.width("0.000 MHz")) + 10;
            if x + w > limit { break; }
            out.push((key, Rect::new(x, row.y + 3, w, 28)));
            x += w + 5;
        }
        out
    }

    /// What the cursors read, as one chip after the badges: the differences only.
    fn studio_cursor_chip(&self, after: i32) -> Option<(Rect, String)> {
        if self.m.cursor == CursorMode::Off { return None; }
        let text = self.cursor_values().into_iter().filter(|(l, _)| l.contains('\u{394}'))
            .map(|(l, v)| format!("{l} {v}")).collect::<Vec<_>>().join("   ");
        if text.is_empty() { return None; }
        let w = 30 + self.fonts().sb.width(&text) + 10;
        let row = self.studio_row();
        (after + w <= self.studio_eye().x - 6).then(|| (Rect::new(after, row.y + 3, w, 28), text))
    }

    pub(super) fn draw_studio_badges(&self, cv: &mut Canvas) {
        if !self.studio_badges_visible() { return; }
        let f = self.fonts();
        let row = self.studio_row();
        cv.fill(row, BG);
        let rows = self.meas_rows();
        let mut end = row.x + 4;
        for (key, r) in self.studio_badges() {
            let Some(m) = self.studio_index(key).and_then(|i| rows.get(i)) else { continue };
            let col = self.wave_color(m.src);
            chip(cv, r, BTN, if self.studio.popup == Some(key) { ACCENT } else { BTN_BORDER });
            let cy = r.y + r.h / 2;
            icon_pin(cv, r.x + 8, cy, true, TEXT);
            let tag = Rect::new(r.x + 28, r.y + 5, f.sb.width("CH4") + 10, r.h - 10);
            cv.round(tag, 4, col, 255);
            cv.text_in(&f.sb, tag, 1, &self.wave_name(m.src), DARK_TEXT);
            let x = cv.text(&f.s, tag.right() + 6, cy + 4, &m.name, DIM) + 8;
            cv.set_clip(Rect::new(x, r.y, (r.right() - x - 4).max(1), r.h));
            cv.text(&f.sb, x, cy + 4, &m.text(), TEXT);
            cv.no_clip();
            end = r.right() + 5;
        }
        if let Some((r, text)) = self.studio_cursor_chip(end) {
            chip(cv, r, BAR, BTN_BORDER);
            icon_cursors(cv, r.x + 9, r.y + r.h / 2, BAND);
            cv.text_in(&f.sb, Rect::new(r.x + 28, r.y, r.w - 30, r.h), 0, &text, TEXT);
        }
        let eye = self.studio_eye();
        cv.vline(eye.x - 3, row.y + 6, row.bottom() - 6, LINE);
        icon_eye(cv, eye.x + eye.w / 2, eye.y + eye.h / 2, TEXT);
    }

    /// The popup of a badge: where it is.
    pub(super) fn studio_popup_rect(&self) -> Option<Rect> {
        let key = self.studio.popup?;
        let (_, anchor) = self.studio_badges().into_iter().find(|(k, _)| *k == key)?;
        let h = 40 + 3 * POPUP_ROW + 10 + POPUP_ROW + 6;
        let x = (anchor.x + anchor.w / 2 - POPUP_W / 2).clamp(4, self.scr.w - POPUP_W - 4);
        Some(Rect::new(x, anchor.y - h - 8, POPUP_W, h))
    }

    /// Row `i` of the popup: 0 Configure, 1 Unpin, 2 Float, 3 Remove.
    pub(super) fn studio_popup_row(p: Rect, i: i32) -> Rect {
        let y = p.y + 40 + i * POPUP_ROW + if i == 3 { 10 } else { 0 };
        Rect::new(p.x + 4, y, p.w - 8, POPUP_ROW)
    }

    /// What floats over the lanes and the bars: the badge popup.
    pub(super) fn draw_studio_floating(&self, cv: &mut Canvas) {
        let (Some(key), Some(p)) = (self.studio.popup, self.studio_popup_rect()) else { return };
        let f = self.fonts();
        let rows = self.meas_rows();
        let Some(m) = self.studio_index(key).and_then(|i| rows.get(i)) else { return };
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        if let Some((_, a)) = self.studio_badges().into_iter().find(|(k, _)| *k == key) {
            let x = a.x + a.w / 2;
            cv.triangle([(x - 7, p.bottom() - 1), (x + 7, p.bottom() - 1), (x, p.bottom() + 6)], BTN_BORDER);
            cv.triangle([(x - 6, p.bottom() - 2), (x + 6, p.bottom() - 2), (x, p.bottom() + 4)], PANEL_BG);
        }
        let tag = Rect::new(p.x + 10, p.y + 9, f.sb.width("CH4") + 10, 22);
        cv.round(tag, 4, self.wave_color(m.src), 255);
        cv.text_in(&f.sb, tag, 1, &self.wave_name(m.src), DARK_TEXT);
        let x = cv.text(&f.mb, tag.right() + 8, p.y + 25, &m.name, TEXT) + 10;
        cv.text(&f.mb, x, p.y + 25, &m.text(), TEXT);
        let (cx, cy) = (p.right() - 24, p.y + 20);
        cv.line(cx - 5, cy - 5, cx + 5, cy + 5, TEXT);
        cv.line(cx - 5, cy + 5, cx + 5, cy - 5, TEXT);
        cv.hline(p.x + 8, p.right() - 9, p.y + 38, LINE);
        for (i, (label, col)) in [("Configure...", TEXT), ("Unpin from waveform", TEXT), ("Move to floating panel", TEXT), ("Remove measurement", RED)].into_iter().enumerate() {
            let r = Self::studio_popup_row(p, i as i32);
            if i == 3 { cv.hline(p.x + 8, p.right() - 9, r.y - 5, LINE); }
            let (ix, iy) = (r.x + 18, r.y + r.h / 2);
            match i {
                0 => icon_gear(cv, ix, iy, col, PANEL_BG),
                1 => { icon_pin(cv, ix - 6, iy, false, col); cv.line(ix - 8, iy + 8, ix + 8, iy - 8, col); }
                2 => { cv.round_outline(Rect::new(ix - 8, iy - 6, 14, 12), 2, col); cv.line(ix, iy, ix + 8, iy - 8, col); }
                _ => { cv.round_outline(Rect::new(ix - 7, iy - 5, 13, 13), 2, col); cv.hline(ix - 9, ix + 8, iy - 7, col); }
            }
            cv.text_in(&f.mb, Rect::new(r.x + 42, r.y, r.w - 44, r.h), 0, label, col);
        }
    }

    /// A tap while a badge popup is open: it either does one of its actions or closes it. Always taken.
    fn studio_popup_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(key) = self.studio.popup else { return false };
        let Some(p) = self.studio_popup_rect() else { self.studio.popup = None; return false };
        self.studio.popup = None;
        if !p.contains(x, y) { return true; }
        let hit = (0..4).find(|&i| Self::studio_popup_row(p, i).contains(x, y));
        let Some(index) = self.studio_index(key) else { return true };
        match hit {
            Some(0) => if index < self.m.meas.len() { self.open_meas_cfg(index) } else { self.open_menu(MenuId::Measure) },
            Some(1) => self.studio_set_pin(key, false),
            Some(2) => { self.studio_set_pin(key, false); self.studio.view = StudioView::Floating; self.relayout(); }
            Some(3) => {
                self.studio_set_pin(key, false);
                self.remove_measurement(index);
            }
            _ => {}
        }
        true
    }

    /// A tap in the badge row: the eye hides it, a badge opens its popup.
    fn studio_badge_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.studio_badges_visible() || !self.studio_row().contains(x, y) { return false; }
        if self.studio_eye().contains(x, y) {
            self.studio.hide_badges = true;
            self.relayout();
            return true;
        }
        if let Some((key, _)) = self.studio_badges().into_iter().find(|(_, r)| r.contains(x, y)) {
            self.studio.popup = Some(key);
        }
        true
    }
}
