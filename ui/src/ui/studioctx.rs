// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the context menu of the plot. A long press (or a right click) on the lanes opens a small popover at the press
//! point: grouped rows with an icon and a label, the state of Overlay / Stacked shown, the actions that need a place
//! (zoom here, trigger level here, cursor here) taking the place of the finger, and the ones that need a lane naming the
//! lane under it. It is kept on the screen and closes on a tap outside.
use super::studio::{icon_cursors, icon_expand};
use super::workspace::Action;
use super::*;

const CTX_W: i32 = 276;
const CTX_ROW: i32 = 28;
const CTX_SEP: i32 = 8;
const CTX_HEAD: i32 = 26;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum CtxRow {
    Overlay,
    Stacked,
    Fit,
    ZoomHere,
    ZoomBox,
    ZoomReset,
    ZoomClose,
    RunStop,
    Single,
    Autoset,
    TrigOn(WaveId),
    LevelHere(WaveId),
    TrigSettings,
    MeasOn(WaveId),
    CursorHere,
    Sources,
    /// every view in tabs of one group, or back from it
    Layout,
}

/// Where the menu was opened and the lane under that point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Ctx {
    pub x: i32,
    pub y: i32,
    pub lane: Option<WaveId>,
}

impl Ui {
    /// The lane under a point: in stacked lanes the one whose area it is, otherwise the selected source.
    fn ctx_lane(&self, x: i32, y: i32) -> Option<WaveId> {
        let n = n_ch();
        if self.wins.len() > 1 {
            if let Some(c) = (0..n).find(|&c| self.m.ch[c].on && self.ch_area(c).contains(x, y)) { return Some(WaveId::Ch(c)); }
        }
        let sel = if self.record_mode() { WaveId::Ref(self.ref_sel) } else { WaveId::Ch(self.m.active) };
        match sel { WaveId::Ch(c) if c < n && self.m.ch[c].on => Some(sel), WaveId::Ref(_) => Some(sel), _ => (0..n).find(|&c| self.m.ch[c].on).map(WaveId::Ch) }
    }

    pub(super) fn studio_ctx_open(&mut self, x: i32, y: i32) {
        if self.menu_open { self.close_menu(); }
        self.studio.view_menu = false;
        self.studio.add_menu = false;
        self.studio.popup = None;
        let lane = self.ctx_lane(x, y);
        self.studio.ctx = Some(Ctx { x, y, lane });
    }

    /// The groups of rows, the group of a lane naming it.
    fn ctx_groups(&self, lane: Option<WaveId>) -> Vec<Vec<(CtxRow, String)>> {
        let name = |id: WaveId| self.wave_name(id);
        let mut view = vec![(CtxRow::Overlay, "Overlay traces".to_string()), (CtxRow::Stacked, "Stacked traces".to_string()), (CtxRow::Fit, "Fit record".to_string()), (CtxRow::ZoomHere, "Zoom here".to_string()), (CtxRow::ZoomBox, "Zoom box".to_string())];
        if self.m.zoom {
            view.push((CtxRow::ZoomReset, "Reset zoom".to_string()));
            view.push((CtxRow::ZoomClose, "Close zoom".to_string()));
        }
        let acquire = vec![(CtxRow::RunStop, if self.m.run == RunState::Run && !self.record_mode() { "Stop" } else { "Run" }.to_string()),
            (CtxRow::Single, "Single".to_string()), (CtxRow::Autoset, "Autoset".to_string())];
        let mut trig = Vec::new();
        if let Some(id) = lane {
            trig.push((CtxRow::TrigOn(id), format!("Trigger on {}", name(id))));
            trig.push((CtxRow::LevelHere(id), "Set level here".to_string()));
        }
        trig.push((CtxRow::TrigSettings, "Trigger settings".to_string()));
        let mut add = Vec::new();
        if let Some(id) = lane { add.push((CtxRow::MeasOn(id), format!("Measurement on {}", name(id)))); }
        add.push((CtxRow::CursorHere, "Cursor here".to_string()));
        let layout = if self.dock_full_state() { "Restore layout" } else { "Maximise view" };
        vec![view, acquire, trig, add, vec![(CtxRow::Sources, "Sources".to_string()), (CtxRow::Layout, layout.to_string())]]
    }

    /// The popover: anchored at the press, flipped and clamped to stay on the screen.
    pub(super) fn studio_ctx_rect(&self) -> Option<Rect> {
        let c = self.studio.ctx?;
        let g = self.ctx_groups(c.lane);
        let rows: i32 = g.iter().map(|x| x.len() as i32).sum();
        let h = CTX_HEAD + rows * CTX_ROW + (g.len() as i32 - 1) * CTX_SEP + 8;
        let h = h.min(self.scr.h - 8);
        let x = if c.x + 8 + CTX_W <= self.scr.w - 4 { c.x + 8 } else { (c.x - 8 - CTX_W).max(4) };
        let y = c.y.min(self.scr.h - h - 4).max(4);
        Some(Rect::new(x, y, CTX_W, h))
    }

    pub(super) fn studio_ctx_rows(&self) -> Vec<(CtxRow, String, Rect)> {
        let (Some(p), Some(c)) = (self.studio_ctx_rect(), self.studio.ctx) else { return Vec::new() };
        let mut y = p.y + CTX_HEAD + 2;
        let mut out = Vec::new();
        let groups = self.ctx_groups(c.lane);
        // a tall list on a short screen shrinks its rows
        let rows: i32 = groups.iter().map(|g| g.len() as i32).sum();
        let room = p.h - CTX_HEAD - 8 - (groups.len() as i32 - 1) * CTX_SEP;
        let row_h = (room / rows.max(1)).min(CTX_ROW).max(24);
        for (gi, g) in groups.into_iter().enumerate() {
            if gi > 0 { y += CTX_SEP; }
            for (row, label) in g {
                out.push((row, label, Rect::new(p.x + 4, y, p.w - 8, row_h)));
                y += row_h;
            }
        }
        out
    }

    fn ctx_icon(&self, cv: &mut Canvas, row: CtxRow, cx: i32, cy: i32, c: Rgb) {
        let f = self.fonts();
        match row {
            CtxRow::Overlay => { cv.line(cx - 7, cy + 3, cx, cy - 4, c); cv.line(cx, cy - 4, cx + 7, cy + 3, c); cv.line(cx - 7, cy - 1, cx, cy + 5, c); }
            CtxRow::Stacked => { cv.hline(cx - 7, cx + 7, cy - 4, c); cv.hline(cx - 7, cx + 7, cy, c); cv.hline(cx - 7, cx + 7, cy + 4, c); }
            CtxRow::Fit => icon_expand(cv, cx, cy, c),
            CtxRow::ZoomHere | CtxRow::ZoomReset => { cv.circle(cx - 2, cy - 2, 5, c); cv.circle(cx - 2, cy - 2, 3, BAR); cv.line(cx + 2, cy + 2, cx + 7, cy + 7, c); }
            CtxRow::ZoomBox => { cv.round_outline(Rect::new(cx - 7, cy - 5, 14, 10), 1, c); cv.fill(Rect::new(cx - 7, cy - 5, 4, 2), c); }
            CtxRow::ZoomClose => { cv.line(cx - 5, cy - 5, cx + 5, cy + 5, c); cv.line(cx - 5, cy + 5, cx + 5, cy - 5, c); }
            CtxRow::RunStop => {
                if self.m.run == RunState::Run && !self.record_mode() { cv.fill(Rect::new(cx - 5, cy - 5, 3, 10), c); cv.fill(Rect::new(cx + 2, cy - 5, 3, 10), c); }
                else { cv.triangle([(cx - 4, cy - 6), (cx - 4, cy + 6), (cx + 6, cy)], c); }
            }
            CtxRow::Single => cv.text_in(&f.sb, Rect::new(cx - 10, cy - 9, 20, 18), 1, "1", c),
            CtxRow::Autoset => cv.text_in(&f.sb, Rect::new(cx - 10, cy - 9, 20, 18), 1, "A", c),
            CtxRow::TrigOn(_) | CtxRow::TrigSettings => { cv.hline(cx - 7, cx - 2, cy + 4, c); cv.vline(cx - 2, cy - 4, cy + 4, c); cv.hline(cx - 2, cx + 7, cy - 4, c); }
            CtxRow::LevelHere(_) => { cv.hline(cx - 8, cx + 8, cy, c); cv.triangle([(cx - 4, cy - 5), (cx + 4, cy - 5), (cx, cy - 1)], c); }
            CtxRow::MeasOn(_) => { for (i, h) in [6, 11, 8].into_iter().enumerate() { cv.fill(Rect::new(cx - 7 + i as i32 * 5, cy + 6 - h, 3, h), c); } }
            CtxRow::CursorHere => icon_cursors(cv, cx - 5, cy, c),
            CtxRow::Layout => { cv.rect(Rect::new(cx - 6, cy - 6, 12, 12), c); cv.hline(cx - 6, cx + 5, cy - 3, c); }
            CtxRow::Sources => { for dy in [-5, 0, 5] { cv.circle(cx - 5, cy + dy, 1, c); cv.hline(cx - 1, cx + 7, cy + dy, c); } }
        }
    }

    pub(super) fn draw_studio_ctx(&self, cv: &mut Canvas) {
        let (Some(p), Some(c)) = (self.studio_ctx_rect(), self.studio.ctx) else { return };
        let f = self.fonts();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        let title = c.lane.map_or("Plot".to_string(), |id| self.wave_name(id));
        let tcol = c.lane.map_or(TEXT, |id| self.wave_color(id));
        cv.text(&f.sb, p.x + 12, p.y + 18, &title, tcol);
        cv.hline(p.x + 8, p.right() - 9, p.y + CTX_HEAD - 2, LINE);
        let rows = self.studio_ctx_rows();
        for (i, (row, label, r)) in rows.iter().enumerate() {
            // a line between the groups
            if i > 0 && r.y - rows[i - 1].2.bottom() >= CTX_SEP { cv.hline(p.x + 10, p.right() - 11, r.y - CTX_SEP / 2, LINE); }
            let on = match row { CtxRow::Overlay => !self.trace_stacked(), CtxRow::Stacked => self.trace_stacked(), CtxRow::ZoomHere => self.m.zoom, _ => false };
            self.ctx_icon(cv, *row, r.x + 16, r.y + r.h / 2, if on { ACCENT } else { TEXT });
            cv.set_clip(Rect::new(r.x + 32, r.y, r.w - 62, r.h));
            cv.text_in(&f.m, Rect::new(r.x + 34, r.y, r.w - 62, r.h), 0, label, TEXT);
            cv.no_clip();
            if matches!(row, CtxRow::Overlay | CtxRow::Stacked) && on {
                // the check of the mode in force
                let (cx, cy) = (r.right() - 16, r.y + r.h / 2);
                cv.line(cx - 5, cy, cx - 2, cy + 4, ACCENT);
                cv.line(cx - 2, cy + 4, cx + 5, cy - 4, ACCENT);
            }
        }
    }

    /// The volts of lane `c` at screen row `y`: the inverse of `y_of_volts`, by bisection (it is monotonic).
    fn ctx_volts_at(&self, c: usize, y: i32) -> f32 {
        let vpd = self.m.ch[c].volts_per_div().max(1e-9);
        let (mut lo, mut hi) = (-20.0 * vpd - self.m.ch[c].offset, 20.0 * vpd - self.m.ch[c].offset);
        let up = self.y_of_volts(c, hi) < self.y_of_volts(c, lo);
        for _ in 0..30 {
            let mid = (lo + hi) / 2.0;
            let below = self.y_of_volts(c, mid) > y;
            if below == up { hi = mid; } else { lo = mid; }
        }
        (lo + hi) / 2.0
    }

    fn ctx_do(&mut self, row: CtxRow, at: Ctx) {
        match row {
            CtxRow::Overlay => self.workspace_action(Action::Overlay),
            CtxRow::Stacked => self.workspace_action(Action::Stacked),
            CtxRow::Fit => self.workspace_action(Action::FitRecord),
            CtxRow::ZoomHere => {
                // the zoom window goes to the time under the finger
                let a = self.area;
                let (tdiv, delay) = self.view();
                let t = delay + (at.x - a.x - a.w / 2) as f32 / (a.w as f32 / HDIV) * tdiv;
                if !self.m.zoom { self.set_zoom(true); }
                self.m.zdelay = t;
                self.clamp_zoom();
                self.relayout();
            }
            CtxRow::ZoomBox => self.arm_zoom_area(),
            CtxRow::ZoomReset => self.workspace_action(Action::ZoomReset),
            CtxRow::ZoomClose => self.workspace_action(Action::ZoomClose),
            CtxRow::RunStop => self.workspace_action(Action::Run),
            CtxRow::Single => self.workspace_action(Action::Single),
            CtxRow::Autoset => self.workspace_action(Action::Autoset),
            CtxRow::TrigOn(id) => self.workspace_action(Action::Trigger(id)),
            CtxRow::LevelHere(id) => {
                if let WaveId::Ch(c) = id {
                    self.set_trigger_source(id, false);
                    let v = self.ctx_volts_at(c, at.y);
                    self.m.trig_level = v;
                    self.settings_changed();
                    self.relayout();
                }
            }
            CtxRow::TrigSettings => self.workspace_action(Action::Menu(MenuId::Trigger)),
            CtxRow::MeasOn(id) => { self.meas_src = id; self.open_meas_list(); }
            CtxRow::CursorHere => self.studio_cursors_add(None),
            CtxRow::Sources => self.workspace_action(Action::Sources),
            CtxRow::Layout => { if !self.dock_exit_full() { self.dock_maximise(super::dock::VKind::Wave); } }
        }
    }

    /// A tap while the menu is open: a row does its action, a tap outside closes it. Always taken.
    pub(super) fn studio_ctx_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(c) = self.studio.ctx else { return false };
        let hit = self.studio_ctx_rows().into_iter().find(|(_, _, r)| r.contains(x, y)).map(|(row, _, _)| row);
        self.studio.ctx = None;
        if let Some(row) = hit { self.ctx_do(row, c); }
        true
    }
}
