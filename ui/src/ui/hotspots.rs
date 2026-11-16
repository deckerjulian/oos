// SPDX-License-Identifier: GPL-2.0-or-later
//! What a finger can tap on the plot besides the traces: the trigger marker, the cursor tags, the channel markers and the
//! traces and lanes themselves. One list for every layout, each with a hit area of at least 40 px (`Ui::hit40`).
use super::*;

/// A cursor, as the tags name them: 0 and 1 are A and B (X1 / X2 or, with voltage cursors, Y1 / Y2), 2 and 3 are C and D (Y1 / Y2 with XY).
pub(super) type CursorK = usize;

impl Ui {
    /// The trigger markers on the plot: the level marker at the right edge, and in Studio the T over the lanes.
    pub(super) fn trigger_marker_rects(&self) -> Vec<Rect> {
        let mut out = Vec::new();
        let a = self.area;
        let ts = self.m.trig_src;
        if self.m.trig_ref.is_none() && self.m.ch[ts].on && self.shows(ts) && self.m.trig_line().is_none() && self.trigger_absolute_level_visible(false) {
            let y = self.y_of_volts(ts, self.trig_level_shown()).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
            out.push(Rect::new(a.x + a.w - MARK_W + 8, y - 8, 16, 16));
        }
        if self.studio_tags_on() && !self.view_on() && !self.eye_open() {
            out.push(self.studio_tag_rect(self.studio_x_of_time(0.0)));
        }
        out
    }

    /// The tags of the cursors that are on the plot, each with the cursor it stands for.
    pub(super) fn cursor_tags(&self) -> Vec<(CursorK, Rect)> {
        let mut out = Vec::new();
        if self.m.cursor == CursorMode::Off || self.view_on() || self.eye_open() { return out; }
        let source = self.cursor_source();
        if !self.workspace_source_visible_here(source)
            && !(self.m.cursor == CursorMode::Track && self.workspace_source_visible_here(self.cursor_b_source())) { return out; }
        let a = self.area;
        let f = self.fonts();
        let w = |s: &str| f.s.width(s) + 8;
        let side = |y: i32, w: i32| Rect::new(self.right_tag_x(a, y - 16, w), y - 16, w, 14);
        match self.m.cursor {
            CursorMode::Time | CursorMode::Track | CursorMode::Xy => {
                if self.m.cursor == CursorMode::Xy {
                    for (k, frac, name) in [(2, self.m.cur_c, "Y1"), (3, self.m.cur_d, "Y2")] {
                        out.push((k, side(a.y + (frac * a.h as f32) as i32, w(name))));
                    }
                }
                for (k, frac, name) in [(0, self.m.cur_a, "X1"), (1, self.m.cur_b, "X2")] {
                    if !(0.0..=1.0).contains(&frac) { continue; }
                    let x = a.x + (frac * a.w as f32) as i32;
                    out.push((k, Rect::new(x + 2, a.y + a.h - 36, w(name), 14)));
                }
            }
            CursorMode::Volt => {
                for (k, frac, name) in [(0, self.m.cur_a, "Y1"), (1, self.m.cur_b, "Y2")] {
                    out.push((k, side(a.y + (frac * a.h as f32) as i32, w(name))));
                }
            }
            CursorMode::Off => {}
        }
        // Studio: C1 / C2 in the row over the lanes
        if self.studio_tags_on() && matches!(self.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) {
            let g = self.grid;
            for (k, frac) in [(0, self.m.cur_a), (1, self.m.cur_b)] {
                if !(0.0..=1.0).contains(&frac) { continue; }
                let x = g.x + (frac.clamp(0.0, 1.0) * g.w as f32) as i32;
                out.push((k, self.studio_tag_rect(x)));
            }
        }
        out
    }

    /// The item of the Cursor menu that sets cursor `k`.
    fn cursor_item(&self, k: CursorK) -> Item {
        [Item::CurA, Item::CurB, Item::CurC, Item::CurD][k.min(3)]
    }

    /// A tap on a cursor tag selects that cursor: its tag is marked and the knob (the Cursor menu's focused row) is on it.
    pub(super) fn select_cursor(&mut self, k: CursorK) {
        self.workspace.cursor_sel = Some(k);
        let item = self.cursor_item(k);
        self.open_menu(MenuId::Cursor);
        if let Some(i) = self.items(MenuId::Cursor).iter().position(|&it| it == item) { self.focus = i; }
    }

    /// The source whose trace is under the point, within reach of a finger; in a lane or window of its own, the lane's
    /// source wherever the finger is in it.
    pub(super) fn trace_under(&self, x: i32, y: i32) -> Option<WaveId> {
        let a = self.area;
        if !a.contains(x, y) || self.xy || self.view_on() { return None; }
        let (tdiv, delay) = self.view();
        let t = delay as f64 + (x - (a.x + a.w / 2)) as f64 / (a.w as f32 / HDIV) as f64 * tdiv as f64;
        let mut ids: Vec<WaveId> = (0..n_ch()).filter(|&c| self.m.ch[c].on && self.shows(c)).map(WaveId::Ch).collect();
        ids.extend((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on) && self.ref_win_of(i) == self.cur_win).map(WaveId::Ref));
        let alone = self.wins.len() > 1;
        ids.into_iter()
            .map(|id| (self.wave_at_source(id, t).map_or(i32::MAX, |v| (self.y_of_wave(id, v) - y).abs()), id))
            .filter(|&(d, _)| d <= 24 || alone)
            .min_by_key(|&(d, _)| d)
            .map(|(_, id)| id)
    }

    /// A tap on the plot that is on something: the trigger marker (Trigger), a cursor tag (that cursor), a channel marker
    /// or a trace or lane (that source). False: nothing there, the tap is the plot's.
    pub(super) fn plot_tap(&mut self, x: i32, y: i32) -> bool {
        if self.full_on() || self.view_on() || self.dialog.is_some() || self.m.touch_lock { return false; }
        if self.trigger_marker_rects().into_iter().any(|r| Self::hit40(r).contains(x, y)) {
            self.toggle_menu(MenuId::Trigger);
            return true;
        }
        if let Some(&(k, _)) = self.cursor_tags().iter().find(|(_, r)| Self::hit40(*r).contains(x, y)) {
            self.select_cursor(k);
            return true;
        }
        if let Some(c) = self.channel_hotspot(x, y) {
            self.select_workspace_source(WaveId::Ch(c));
            return true;
        }
        // with a menu open the tap on the plot closes it, as it always did
        if self.menu_open || !self.grid.contains(x, y) { return false; }
        if let Some(id) = self.trace_under(x, y) {
            self.select_workspace_source(id);
            return true;
        }
        false
    }
}

impl Ui {
    /// The cursors are drawn in the window of their source (`draw_cursors`): there they can be grabbed.
    pub(super) fn cursor_visible_here(&self) -> bool {
        self.workspace_source_visible_here(self.cursor_source())
            || (self.m.cursor == CursorMode::Track && self.workspace_source_visible_here(self.cursor_b_source()))
    }
}
