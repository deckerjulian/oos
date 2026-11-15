// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: gestures of the measurements list. A row swiped left is removed (with an Undo bar for a few seconds); a row
//! held for half a second can be dragged to reorder the list, or onto the badge strip to pin it; a badge held and
//! dragged off the strip unpins it; a tile of the Add measurement panel dragged onto a channel adds it for that
//! channel. Every gesture has a tap equivalent: the row's menu (Settings, Pin, Move up, Move down, Remove).
use super::meascatalog::Preview;
use super::studio::{MeasKey, StudioDrag, StudioView};
use super::*;

/// A swipe starts after this much horizontal movement, when it is more horizontal than vertical.
pub(super) const SWIPE_START: i32 = 16;
/// How long the Undo bar stays.
pub(super) const UNDO_S: f64 = 6.0;
/// A finger that moves more than this cancels a hold.
const HOLD_SLOP: i32 = 12;
/// Height of the Undo bar and of the rows of the row menu.
const BAR_H: i32 = 44;
const MENU_ROW: i32 = 42;
const MENU_W: i32 = 210;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum HoldOn {
    Row(MeasKey),
    Badge(MeasKey),
    /// a math or reference chip of the bottom bar: a hold opens its quick actions
    Math(usize),
    Ref(usize),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Hold {
    pub on: HoldOn,
    pub x0: i32,
    pub y0: i32,
    pub x: i32,
    pub y: i32,
    pub at: f64,
}

/// A removed measurement, kept until the Undo bar goes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Undo {
    pub key: MeasKey,
    pub index: usize,
    pub pinned: bool,
    pub own: Option<reflook::MeasOwn>,
    pub at: f64,
}

#[derive(Default)]
pub(super) struct Md {
    pub hold: Option<Hold>,
    /// the row a finger went down on: it may become a swipe
    pub cand: Option<(MeasKey, i32, i32)>,
    /// the row that is being swiped and how far (<= 0)
    pub swipe: Option<(MeasKey, i32)>,
    /// a row that springs back after a short swipe
    pub spring: Option<(MeasKey, f32)>,
    pub undo: Option<Undo>,
    /// the row menu (the tap equivalent of the gestures)
    pub menu: Option<MeasKey>,
    /// rows that show their statistics although the Statistics switch is off
    pub stats_rows: Vec<MeasKey>,
    /// a finger down on the detailed table: where, and how far it was scrolled
    pub hdown: Option<(i32, i32, i32)>,
    /// the badges were just hidden: a small bar with Undo, since when
    pub hide_undo: Option<f64>,
    /// the info card of a measurement (the catalogue entry) is open
    pub info: Option<super::meascatalog::Cat>,
    /// a tile that needs a channel that is off was tapped: a second tap soon after turns the channel on
    pub need_tap: Option<(super::meascatalog::Cat, f64)>,
}

/// Rows of the row menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RowAct {
    Settings,
    Pin,
    Up,
    Down,
    Remove,
    Stats,
    StatsAll,
    /// Measure > Labels on trace: the levels and spans drawn on the waveform
    Labels,
    /// the info card of the measurement
    About,
}
const ROW_ACTS: [RowAct; 9] = [RowAct::Settings, RowAct::Pin, RowAct::Up, RowAct::Down, RowAct::Remove, RowAct::Stats, RowAct::StatsAll, RowAct::Labels, RowAct::About];

impl Ui {
    // ------------------------------------------------------------------------------------------------ geometry

    /// The row of the docked list under a point: the measurement and the rectangle of the row.
    pub(super) fn md_row_at(&self, x: i32, y: i32) -> Option<(MeasKey, Rect)> {
        let g = self.studio_panel_geo()?;
        if !g.list.contains(x, y) || self.studio_detailed() { return None; }
        let rows = self.meas_rows();
        let listed = self.studio_listed(&rows);
        let keys = self.studio_keys();
        let first = self.studio_first(listed.len(), g.page);
        let k = ((y - g.list.y) / self.studio_panel_row()) as usize;
        let i = *listed.get(first + k)?;
        let key = *keys.get(i)?;
        Some((key, Rect::new(g.list.x, g.list.y + k as i32 * self.studio_panel_row(), g.list.w, self.studio_panel_row())))
    }

    /// The rectangle of a measurement's row in the docked list, when it is on the page.
    pub(super) fn md_row_rect(&self, key: MeasKey) -> Option<Rect> {
        let g = self.studio_panel_geo()?;
        let rows = self.meas_rows();
        let listed = self.studio_listed(&rows);
        let keys = self.studio_keys();
        let first = self.studio_first(listed.len(), g.page);
        let k = listed.iter().skip(first).take(g.page).position(|&i| keys.get(i) == Some(&key))?;
        Some(Rect::new(g.list.x, g.list.y + k as i32 * self.studio_panel_row(), g.list.w, self.studio_panel_row()))
    }

    /// The pin and the menu button of a row: two targets of 44 px at its right end.
    pub(super) fn md_row_buttons(r: Rect) -> (Rect, Rect) {
        (Rect::new(r.right() - 88, r.y, 44, r.h), Rect::new(r.right() - 44, r.y, 44, r.h))
    }

    /// The Undo bar: the bottom of the list.
    pub(super) fn md_undo_bar(&self) -> Option<(Rect, Rect)> {
        self.studio.md.undo?;
        let g = self.studio_panel_geo()?;
        let bar = Rect::new(g.list.x, g.list.bottom() - BAR_H, g.list.w, BAR_H);
        Some((bar, Rect::new(bar.right() - 92, bar.y, 92, bar.h)))
    }

    /// The math or reference chip of the bottom bar under a point.
    pub(super) fn md_chip_at(&self, x: i32, y: i32) -> Option<HoldOn> {
        if self.m.layout != Layout::Studio || self.full_on() { return None; }
        let b = self.studio_bottom();
        if let Some(k) = (0..NMATH).find(|&k| b.math[k].w > 0 && b.math[k].contains(x, y)) { return Some(HoldOn::Math(k)); }
        self.shown_refs().into_iter().find(|&i| b.refs[i].w > 0 && b.refs[i].contains(x, y)).map(HoldOn::Ref)
    }

    /// The badge strip is the target of a pin: the row under the lanes.
    fn md_strip(&self) -> Option<Rect> {
        self.studio_badges_visible().then(|| self.studio_row())
    }

    // ------------------------------------------------------------------------------------------------ remove, undo, move

    /// Removes a measurement and keeps what is needed to bring it back at the same place.
    pub(super) fn md_remove(&mut self, key: MeasKey) {
        let Some(index) = self.studio_index(key) else { return };
        let pinned = self.studio_is_pinned(key);
        let own = match key { MeasKey::One(k, id) => self.meas_own.get(&(k, id)).copied(), _ => None };
        self.studio.md.undo = Some(Undo { key, index, pinned, own, at: self.now });
        self.studio_set_pin(key, false);
        self.remove_measurement(index);
        self.studio.md.menu = None;
    }

    /// Brings the removed measurement back: the same place in the list, pinned again when it was.
    pub(super) fn md_undo(&mut self) {
        let Some(u) = self.studio.md.undo.take() else { return };
        match u.key {
            MeasKey::One(k, id) => {
                let at = u.index.min(self.m.meas.len());
                self.m.meas.insert(at, (k, id));
                if let Some(o) = u.own { self.meas_own.insert((k, id), o); }
            }
            MeasKey::Pair(k, a, b) => {
                let at = u.index.saturating_sub(self.m.meas.len()).min(self.m.meas2.len());
                self.m.meas2.insert(at, (k, a, b));
            }
            MeasKey::Cat(i, a, b, c, d) => {
                let at = u.index.saturating_sub(self.m.meas.len() + self.m.meas2.len()).min(self.m.cat.len());
                self.m.cat.insert(at, (i, a, b, c, d));
            }
        }
        if u.pinned { self.studio_set_pin(u.key, true); }
        self.clear_measurement_statistics();
        self.relayout();
    }

    /// Moves `key` before `before` (None: to the end). The list keeps its three kinds in their order, so a row moves
    /// among those of its own kind.
    pub(super) fn md_reorder(&mut self, key: MeasKey, before: Option<MeasKey>) {
        let mut keys = self.studio_keys();
        let Some(from) = keys.iter().position(|k| *k == key) else { return };
        if before == Some(key) { return; }
        keys.remove(from);
        let to = before.and_then(|b| keys.iter().position(|k| *k == b)).unwrap_or(keys.len());
        keys.insert(to, key);
        self.m.meas = keys.iter().filter_map(|k| if let MeasKey::One(m, id) = k { Some((*m, *id)) } else { None }).collect();
        self.m.meas2 = keys.iter().filter_map(|k| if let MeasKey::Pair(m, a, b) = k { Some((*m, *a, *b)) } else { None }).collect();
        self.m.cat = keys.iter().filter_map(|k| if let MeasKey::Cat(i, a, b, c, d) = k { Some((*i, *a, *b, *c, *d)) } else { None }).collect();
        self.clear_measurement_statistics();
        self.relayout();
    }

    /// One place up or down in the list.
    pub(super) fn md_step(&mut self, key: MeasKey, down: bool) {
        let keys = self.studio_keys();
        let Some(i) = keys.iter().position(|k| *k == key) else { return };
        if down { self.md_reorder(key, keys.get(i + 2).copied()); } else if i > 0 { self.md_reorder(key, Some(keys[i - 1])); }
    }

    // ------------------------------------------------------------------------------------------------ per frame

    /// Every frame: the Undo bar times out, a short swipe springs back, a held finger picks the row or badge up.
    pub(super) fn md_tick(&mut self) {
        if self.studio.md.undo.is_some_and(|u| self.now - u.at > UNDO_S) { self.studio.md.undo = None; }
        if self.studio.md.hide_undo.is_some_and(|t| self.now - t > UNDO_S) { self.studio.md.hide_undo = None; }
        if let Some((key, off)) = self.studio.md.spring {
            let off = off * 0.6;
            self.studio.md.spring = if off.abs() < 2.0 { None } else { Some((key, off)) };
        }
        let Some(h) = self.studio.md.hold else { return };
        if self.now - h.at < LONG_PRESS || (h.x - h.x0).abs().max((h.y - h.y0).abs()) > HOLD_SLOP { return; }
        self.studio.md.hold = None;
        self.studio.md.cand = None;
        self.studio.drag = match h.on {
            HoldOn::Row(key) => Some(StudioDrag::Reorder { key, x: h.x, y: h.y }),
            HoldOn::Badge(key) => Some(StudioDrag::BadgeDrag { key, x: h.x, y: h.y }),
            chip => {
                // the quick actions: the lift of this finger is no tap
                self.workspace.lift_pending = true;
                self.studio_chip_popup(chip, h.x, h.y);
                None
            }
        };
    }

    /// The ring that fills under a finger held on a row or a badge.
    pub(super) fn md_ring(&self) -> Option<(i32, i32, f32)> {
        let h = self.studio.md.hold?;
        let f = ((self.now - h.at) / LONG_PRESS) as f32;
        ((h.x - h.x0).abs().max((h.y - h.y0).abs()) <= HOLD_SLOP && f > 0.2 && f < 1.0).then_some((h.x0, h.y0, f))
    }

    // ------------------------------------------------------------------------------------------------ touch

    /// A finger went down. True when a gesture of the list, the badges or the Add measurement panel takes it.
    pub(super) fn md_down(&mut self, x: i32, y: i32) -> bool {
        self.studio.md.hold = None;
        self.studio.md.cand = None;
        self.studio.md.spring = None;
        // a tile of the Add measurement panel: it follows the finger once it has moved
        if self.menu_open && self.menu == MenuId::MeasAdd && self.sheet_active() {
            let g = self.sheet_add_geo();
            if let Some(&(k, _)) = g.rows.iter().find(|(_, r)| r.contains(x, y) && !Self::tile_info_rect(*r).contains(x, y)) {
                self.studio.drag = Some(StudioDrag::Tile { k, x0: x, y0: y, x, y, moved: false });
                return true;
            }
            return false;
        }
        if self.menu_open || self.studio.view == StudioView::Sim { return false; }
        // the Undo bars are buttons
        if let Some((bar, _)) = self.md_undo_bar() {
            if bar.contains(x, y) { return self.studio_eat(x, y); }
        }
        if let Some((bar, _)) = self.md_hide_bar() {
            if bar.contains(x, y) { return self.studio_eat(x, y); }
        }
        // a badge: a tap opens its popup, a hold picks it up
        if self.studio_badges_visible() && self.studio_row().contains(x, y) {
            if let Some((key, _)) = self.studio_badges().into_iter().find(|(_, r)| r.contains(x, y)) {
                self.studio.md.hold = Some(Hold { on: HoldOn::Badge(key), x0: x, y0: y, x, y, at: self.now });
                return self.studio_eat(x, y);
            }
        }
        // the detailed table: a sideways drag scrolls it
        if self.studio_detailed() {
            if let Some(g) = self.studio_panel_geo().filter(|g| g.list.contains(x, y)) {
                self.studio.md.hdown = Some((x, y, self.studio.panel.hx.clamp(0, self.detail_hmax(g.list.w))));
            }
            return false;
        }
        // a math or reference chip: a tap selects it, a hold opens its quick actions
        if let Some(chip) = self.md_chip_at(x, y) {
            self.studio.md.hold = Some(Hold { on: chip, x0: x, y0: y, x, y, at: self.now });
            return self.studio_eat(x, y);
        }
        // a row of the list: a drag scrolls, a clear sideways drag swipes it, a hold picks it up
        if let Some((key, _)) = self.md_row_at(x, y) {
            if let Some(g) = self.studio_panel_geo() {
                let rows = self.meas_rows();
                let first0 = self.studio_first(self.studio_listed(&rows).len(), g.page);
                self.studio.drag = Some(StudioDrag::Scroll { y0: y, first0, moved: false });
                self.studio.md.cand = Some((key, x, y));
                self.studio.md.hold = Some(Hold { on: HoldOn::Row(key), x0: x, y0: y, x, y, at: self.now });
                return true;
            }
        }
        false
    }

    /// The finger moved. True when a gesture has it; the scroll of the list is left to the caller otherwise.
    pub(super) fn md_move(&mut self, x: i32, y: i32) -> bool {
        if let Some(h) = &mut self.studio.md.hold { h.x = x; h.y = y; }
        match self.studio.drag {
            Some(StudioDrag::Scroll { .. }) if self.studio.md.hdown.is_some() => {
                let (x0, y0, hx0) = self.studio.md.hdown.unwrap_or_default();
                if (x - x0).abs() > 8 && (x - x0).abs() > (y - y0).abs() {
                    self.studio.drag = Some(StudioDrag::HScroll { x0, hx0 });
                    self.studio.md.hdown = None;
                }
                false
            }
            Some(StudioDrag::HScroll { x0, hx0 }) => {
                let max = self.studio_panel_geo().map_or(0, |g| self.detail_hmax(g.list.w));
                self.studio.panel.hx = (hx0 - (x - x0)).clamp(0, max);
                true
            }
            Some(StudioDrag::Scroll { .. }) => {
                if let Some((key, x0, y0)) = self.studio.md.cand {
                    let (dx, dy) = (x - x0, y - y0);
                    if dx < -SWIPE_START && dx.abs() > dy.abs() {
                        self.studio.drag = Some(StudioDrag::Swipe { key, x0, y0 });
                        self.studio.md.cand = None;
                        self.studio.md.hold = None;
                        self.studio.md.swipe = Some((key, dx));
                        return true;
                    }
                }
                false
            }
            Some(StudioDrag::Swipe { key, x0, .. }) => {
                self.studio.md.swipe = Some((key, (x - x0).min(0)));
                true
            }
            Some(StudioDrag::Reorder { key, .. }) => { self.studio.drag = Some(StudioDrag::Reorder { key, x, y }); true }
            Some(StudioDrag::BadgeDrag { key, .. }) => { self.studio.drag = Some(StudioDrag::BadgeDrag { key, x, y }); true }
            Some(StudioDrag::Tile { k, x0, y0, moved, .. }) => {
                let moved = moved || (x - x0).abs().max((y - y0).abs()) > SWIPE_START;
                self.studio.drag = Some(StudioDrag::Tile { k, x0, y0, x, y, moved });
                true
            }
            Some(StudioDrag::Eat { .. }) if self.studio.md.hold.is_some() => {
                // a badge or the Undo bar: a finger that leaves the hold is no hold
                if let Some(h) = self.studio.md.hold { if (x - h.x0).abs().max((y - h.y0).abs()) > HOLD_SLOP { self.studio.md.hold = None; } }
                false
            }
            _ => false,
        }
    }

    /// The finger lifted at (x, y) ending `drag`. True when the lift was a gesture of this file.
    pub(super) fn md_up(&mut self, drag: StudioDrag, x: i32, y: i32) -> bool {
        self.studio.md.hold = None;
        self.studio.md.cand = None;
        self.studio.md.hdown = None;
        match drag {
            StudioDrag::HScroll { .. } => true,
            StudioDrag::Swipe { key, x0, .. } => {
                let dx = (x - x0).min(0);
                self.studio.md.swipe = None;
                let w = self.studio_panel_geo().map_or(240, |g| g.list.w);
                if -dx >= (w * 2 / 5).max(80) { self.md_remove(key); } else { self.studio.md.spring = Some((key, dx as f32)); }
                true
            }
            StudioDrag::Reorder { key, .. } => { self.md_drop_row(key, x, y); true }
            StudioDrag::BadgeDrag { key, .. } => {
                // off the strip: unpinned
                if !self.studio_row().contains(x, y) { self.studio_set_pin(key, false); }
                true
            }
            StudioDrag::Tile { k, moved, .. } => {
                if !moved { return false; }
                self.md_drop_tile(k, x, y);
                true
            }
            _ => false,
        }
    }

    /// A row dropped: on the badge strip it is pinned, on the list it moves there.
    fn md_drop_row(&mut self, key: MeasKey, x: i32, y: i32) {
        if self.md_strip().is_some_and(|s| s.contains(x, y)) {
            self.studio_set_pin(key, true);
            return;
        }
        if let Some(before) = self.md_insert_at(x, y) { self.md_reorder(key, before); }
    }

    /// Where a row dropped at a point would go: before which row (None: at the end); None when not over the list.
    pub(super) fn md_insert_at(&self, x: i32, y: i32) -> Option<Option<MeasKey>> {
        let g = self.studio_panel_geo()?;
        if !g.list.contains(x, y) { return None; }
        let rows = self.meas_rows();
        let listed = self.studio_listed(&rows);
        let keys = self.studio_keys();
        let first = self.studio_first(listed.len(), g.page);
        let pitch = self.studio_panel_row();
        let slot = ((y - g.list.y + pitch / 2) / pitch).max(0) as usize;
        Some(listed.get(first + slot).and_then(|&i| keys.get(i)).copied())
    }

    // ------------------------------------------------------------------------------------------------ tiles

    /// The channel a tile dropped at a point is for: its trace (the lane it is in), its badge in the lane, or its chip
    /// in the bottom bar.
    pub(super) fn md_tile_target(&self, x: i32, y: i32) -> Option<usize> {
        if let Some(&(c, _)) = self.studio.lane_badges.borrow().iter().find(|(_, r)| Self::hit40(*r).contains(x, y)) { return Some(c); }
        let b = self.studio_bottom();
        if let Some(c) = (0..n_ch()).find(|&c| b.ch[c].w > 0 && b.ch[c].contains(x, y)) { return Some(c); }
        let lane = self.wins.iter().position(|a| a.contains(x, y))?;
        match self.trace_sources().get(lane)? {
            workspace::Trace::Channel(c) => Some(*c),
            _ => None,
        }
    }

    /// The rectangle that shows the channel under the finger: its lane, or its chip.
    fn md_tile_target_rect(&self, x: i32, y: i32) -> Option<(usize, Rect)> {
        let c = self.md_tile_target(x, y)?;
        if let Some(&(_, r)) = self.studio.lane_badges.borrow().iter().find(|(k, r)| *k == c && Self::hit40(*r).contains(x, y)) { return Some((c, r)); }
        let b = self.studio_bottom();
        if b.ch[c].w > 0 && b.ch[c].contains(x, y) { return Some((c, b.ch[c])); }
        let lane = self.wins.iter().position(|a| a.contains(x, y))?;
        Some((c, self.wins[lane]))
    }

    fn md_drop_tile(&mut self, k: usize, x: i32, y: i32) {
        let Some(c) = self.md_tile_target(x, y) else { return };
        let Some(cat) = self.sheet_add_entries().get(k).map(|e| e.cat) else { return };
        let id = WaveId::Ch(c);
        self.sheet_pick_source(id);
        match self.add_preview(cat) {
            Preview::Off(ch) => self.toast(&format!("Needs CH{} on", ch + 1)),
            Preview::Reason(s) => self.toast(&s),
            _ if self.add_is_on(cat) => self.toast(&format!("Already measured on CH{}", c + 1)),
            _ => self.sheet_add_toggle(cat),
        }
    }

    // ------------------------------------------------------------------------------------------------ the row menu

    pub(super) fn md_menu_rect(&self) -> Option<Rect> {
        let key = self.studio.md.menu?;
        let row = self.md_row_rect(key)?;
        let g = self.studio_panel_geo()?;
        let h = ROW_ACTS.len() as i32 * MENU_ROW + 8;
        let x = (g.p.x - MENU_W - 6).max(4);
        Some(Rect::new(x, row.y.min(self.scr.h - h - 4).max(4), MENU_W, h))
    }

    pub(super) fn md_menu_row(p: Rect, i: usize) -> Rect { Rect::new(p.x + 4, p.y + 4 + i as i32 * MENU_ROW, p.w - 8, MENU_ROW) }

    /// A tap while the row menu is open: one of its actions, or it closes. Always taken.
    pub(super) fn md_menu_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(key) = self.studio.md.menu else { return false };
        let Some(p) = self.md_menu_rect() else { self.studio.md.menu = None; return false };
        self.studio.md.menu = None;
        let Some(act) = ROW_ACTS.iter().enumerate().find(|(i, _)| Self::md_menu_row(p, *i).contains(x, y)).map(|(_, a)| *a) else { return true };
        match act {
            RowAct::Settings => if let Some(i) = self.studio_index(key) { self.studio_configure(i); },
            RowAct::Pin => { let on = self.studio_is_pinned(key); self.studio_set_pin(key, !on); }
            RowAct::Up => self.md_step(key, false),
            RowAct::Down => self.md_step(key, true),
            RowAct::Remove => self.md_remove(key),
            RowAct::Stats => {
                if let Some(p) = self.studio.md.stats_rows.iter().position(|k| *k == key) { self.studio.md.stats_rows.remove(p); } else { self.studio.md.stats_rows.push(key); }
                self.relayout();
            }
            RowAct::StatsAll => { self.studio.panel.stats = !self.studio.panel.stats; self.relayout(); }
            RowAct::Labels => { self.m.meas_labels = !self.m.meas_labels; self.dirty_map = true; }
            RowAct::About => self.md_about(key),
        }
        true
    }

    /// The bar that follows hiding the badges: where it is, and its Undo button.
    pub(super) fn md_hide_bar(&self) -> Option<(Rect, Rect)> {
        self.studio.md.hide_undo?;
        let w = 300;
        let top = self.frame().top.bottom() + 6;
        let bar = Rect::new((self.scr.w - w) / 2, top, w, BAR_H);
        Some((bar, Rect::new(bar.right() - 92, bar.y, 92, bar.h)))
    }

    /// A tap on the Undo bar. True when it was on the bar (a tap on the button brings the row back).
    pub(super) fn md_undo_tap(&mut self, x: i32, y: i32) -> bool {
        if let Some((bar, button)) = self.md_hide_bar() {
            if bar.contains(x, y) {
                if button.contains(x, y) { self.studio.hide_badges = false; self.studio.md.hide_undo = None; self.relayout(); }
                return true;
            }
        }
        let Some((bar, button)) = self.md_undo_bar() else { return false };
        if !bar.contains(x, y) { return false; }
        if button.contains(x, y) { self.md_undo(); }
        true
    }

    // ------------------------------------------------------------------------------------------------ drawing

    /// The Undo bar, the row menu, and what follows a finger that drags a row, a badge or a tile.
    pub(super) fn draw_md_overlays(&self, cv: &mut Canvas) {
        if self.m.layout != Layout::Studio { return; }
        let f = self.fonts();
        if let Some((bar, button)) = self.md_hide_bar() {
            cv.round(bar, 6, BTN_BORDER, 255);
            cv.round(bar.inset(1), 5, PANEL_BG, 255);
            cv.text_in(&f.s, Rect::new(bar.x + 10, bar.y, button.x - bar.x - 14, bar.h), 0, "Measurements hidden", TEXT);
            btn(cv, button.inset(4), BTN, ACCENT);
            cv.text_in(&f.mb, button, 1, "Undo", ACCENT);
        }
        if let (Some((bar, button)), Some(u)) = (self.md_undo_bar(), self.studio.md.undo) {
            cv.round(bar, 6, BTN_BORDER, 255);
            cv.round(bar.inset(1), 5, PANEL_BG, 255);
            let name = self.md_key_label(u.key);
            cv.set_clip(Rect::new(bar.x, bar.y, (button.x - bar.x).max(1), bar.h));
            cv.text_in(&f.s, Rect::new(bar.x + 10, bar.y, button.x - bar.x - 14, bar.h), 0, &format!("Removed {name}"), TEXT);
            cv.no_clip();
            // the time left, as a line along the bottom
            let left = (1.0 - ((self.now - u.at) / UNDO_S) as f32).clamp(0.0, 1.0);
            cv.fill(Rect::new(bar.x + 6, bar.bottom() - 4, ((bar.w - 12) as f32 * left) as i32, 2), ACCENT);
            btn(cv, button.inset(4), BTN, ACCENT);
            cv.text_in(&f.mb, button, 1, "Undo", ACCENT);
        }
        if let (Some(key), Some(p)) = (self.studio.md.menu, self.md_menu_rect()) {
            cv.round(p, 8, BTN_BORDER, 255);
            cv.round(p.inset(1), 7, PANEL_BG, 255);
            for (i, act) in ROW_ACTS.iter().enumerate() {
                let r = Self::md_menu_row(p, i);
                let (label, col) = match act {
                    RowAct::Settings => ("Settings...", TEXT),
                    RowAct::Pin => (if self.studio_is_pinned(key) { "Unpin from waveform" } else { "Pin to waveform" }, TEXT),
                    RowAct::Up => ("Move up", TEXT),
                    RowAct::Down => ("Move down", TEXT),
                    RowAct::Remove => ("Remove", RED),
                    RowAct::Stats => (if self.studio.md.stats_rows.contains(&key) { "Hide statistics (row)" } else { "Show statistics (row)" }, TEXT),
                    RowAct::StatsAll => (if self.studio.panel.stats { "Hide statistics (all)" } else { "Show statistics (all)" }, TEXT),
                    RowAct::Labels => (if self.m.meas_labels { "Labels on trace: on" } else { "Labels on trace: off" }, TEXT),
                    RowAct::About => ("About this measurement", TEXT),
                };
                if *act == RowAct::Remove { cv.hline(p.x + 8, p.right() - 9, r.y - 1, LINE); }
                cv.text_in(&f.m, Rect::new(r.x + 12, r.y, r.w - 14, r.h), 0, label, col);
            }
        }
        match self.studio.drag {
            Some(StudioDrag::Reorder { key, x, y }) => {
                let strip = self.md_strip().filter(|s| s.contains(x, y));
                if let Some(s) = strip {
                    cv.blend(s, ACCENT, 60);
                    cv.rect(s, ACCENT);
                    cv.text_in(&f.lb, s, 1, "Pin", TEXT);
                } else if let (Some(g), Some(before)) = (self.studio_panel_geo(), self.md_insert_at(x, y)) {
                    // the drop line: between the rows
                    let rows = self.meas_rows();
                    let listed = self.studio_listed(&rows);
                    let keys = self.studio_keys();
                    let first = self.studio_first(listed.len(), g.page);
                    let slot = before.and_then(|b| listed.iter().skip(first).take(g.page).position(|&i| keys.get(i) == Some(&b)))
                        .unwrap_or(listed.len().saturating_sub(first).min(g.page));
                    let ly = g.list.y + slot as i32 * self.studio_panel_row();
                    cv.fill(Rect::new(g.list.x + 4, ly - 1, g.list.w - 8, 3), ACCENT);
                }
                // the row follows the finger
                let rows = self.meas_rows();
                if let Some(row) = self.studio_index(key).and_then(|i| rows.get(i)) {
                    let w = self.studio_panel_geo().map_or(240, |g| g.list.w);
                    let r = Rect::new(x - w / 2, y - self.studio_panel_row() / 2, w, self.studio_panel_row());
                    cv.round(r, 6, ACCENT, 255);
                    cv.round(r.inset(1), 5, PANEL_BG, 235);
                    self.draw_panel_row(cv, r, row, self.studio_is_pinned(key));
                }
            }
            Some(StudioDrag::BadgeDrag { key, x, y }) => {
                let off = !self.studio_row().contains(x, y);
                let label = self.md_key_label(key);
                let w = f.mb.width(&label) + 28;
                let tag = Rect::new((x - w / 2).clamp(0, self.scr.w - w), (y - 24).max(0), w, 36);
                cv.round(tag, 6, if off { RED } else { ACCENT }, 255);
                cv.round(tag.inset(1), 5, PANEL_BG, 245);
                cv.text_in(&f.mb, tag, 1, &label, TEXT);
                if off { cv.text_in(&f.s, Rect::new(tag.x, tag.bottom() + 2, tag.w, 18), 1, "Unpin", RED); }
            }
            Some(StudioDrag::Tile { k, x, y, moved: true, .. }) => {
                if let Some((c, r)) = self.md_tile_target_rect(x, y) {
                    let col = CH_COLORS[c];
                    cv.blend(r, col, 40);
                    cv.rect(r, col);
                    cv.rect(r.inset(1), col);
                }
                let name = self.sheet_add_entries().get(k).map_or("", |e| e.name);
                let w = f.m.width(name) + 32;
                let tile = Rect::new((x - w / 2).clamp(0, self.scr.w - w), (y - 22).max(0), w, 44);
                cv.round(tile, 6, ACCENT, 255);
                cv.round(tile.inset(1), 5, PANEL_BG, 235);
                cv.text_in(&f.m, tile, 1, name, TEXT);
            }
            _ => {}
        }
    }

    /// "CH1 Vpp" for a message or a ghost.
    fn md_key_label(&self, key: MeasKey) -> String {
        match self.studio_index(key).and_then(|i| self.meas_rows().into_iter().nth(i)) {
            Some(r) => format!("{} {}", self.wave_name(r.src), r.name),
            None => match key {
                MeasKey::One(k, id) => format!("{} {}", self.wave_name(id), super::meascatalog::meas_display_name(k)),
                _ => "measurement".to_string(),
            },
        }
    }
}
