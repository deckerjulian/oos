// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the one Add button of the top bar and the compact grouped menu under it. It replaces the old "+ Add" panel
//! button and the Meas button: Measurement comes first, then the views (results table, plot, cursors, search), the sources
//! (math, reference, bus) and the screen (zoom, the measurement strip). Every row is a short way into a page that exists.
use super::addrail::AddKind;
use super::studio::{icon_cursors, icon_expand, StudioView};
use super::*;

const MENU_W: i32 = 252;
const ROW: i32 = 34;
const SEP: i32 = 9;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum AddRow {
    Measurement,
    ResultsTable,
    Plot,
    Cursors,
    Search,
    Math,
    Reference,
    Bus,
    Zoom,
    ZoomBox,
    ZoomControls,
    Strip,
    MeasureSettings,
}

/// The rows in their groups; a `None` is a separator.
pub(super) const ADD_MENU: [Option<AddRow>; 17] = [
    Some(AddRow::Measurement), Some(AddRow::ResultsTable), None,
    Some(AddRow::Plot), Some(AddRow::Cursors), Some(AddRow::Search), None,
    Some(AddRow::Math), Some(AddRow::Reference), Some(AddRow::Bus), None,
    Some(AddRow::Zoom), Some(AddRow::ZoomBox), Some(AddRow::ZoomControls), Some(AddRow::Strip), Some(AddRow::MeasureSettings), None,
];

impl Ui {
    pub(super) fn add_row_label(&self, r: AddRow) -> &'static str {
        match r {
            AddRow::Measurement => "Measurement",
            AddRow::ResultsTable => "Results table",
            AddRow::Plot => "Plot",
            AddRow::Cursors => "Cursors",
            AddRow::Search => "Search",
            AddRow::Math => "Math",
            AddRow::Reference => "Reference",
            AddRow::Bus => "Bus",
            AddRow::Zoom => "Zoom",
            AddRow::ZoomBox => "Zoom box",
            AddRow::ZoomControls => if self.m.studio_zoom_tab { "Zoom bar: pull-down tab" } else { "Zoom bar: title bar" },
            AddRow::Strip => if self.studio.hide_badges { "Show measurement strip" } else { "Measurement strip" },
            AddRow::MeasureSettings => "Measure settings",
        }
    }

    /// The row says "On" when what it adds is on the screen.
    fn add_row_on(&self, r: AddRow) -> bool {
        match r {
            AddRow::Measurement => self.measurement_count() > 0,
            AddRow::ResultsTable => self.studio_panel_shown(),
            AddRow::Plot => self.rail_kind_on_pub(AddKind::Plot),
            AddRow::Cursors => self.m.cursor != CursorMode::Off,
            AddRow::Search => self.studio.search.is_some(),
            AddRow::Math => self.m.math != MathOp::Off,
            AddRow::Reference => self.refs.iter().any(Option::is_some),
            AddRow::Bus => self.rail_kind_on_pub(AddKind::Bus),
            AddRow::Zoom => self.m.zoom,
            AddRow::ZoomControls => false,
            AddRow::ZoomBox => self.zoom_arm,
            AddRow::Strip => !self.studio.hide_badges,
            AddRow::MeasureSettings => false,
        }
    }

    /// The menu: under the Add button, as high as its rows, kept on the screen.
    pub(super) fn studio_add_rect(&self) -> Rect {
        let top = self.frame().top;
        let btn = self.studio_top().add;
        let h = 8 + ADD_MENU.iter().map(|r| if r.is_some() { ROW } else { SEP }).sum::<i32>();
        let x = (btn.right() - MENU_W).clamp(4, self.scr.w - MENU_W - 4);
        Rect::new(x, top.bottom() + 6, MENU_W, h.min(self.scr.h - top.bottom() - 12))
    }

    /// The rows with their rectangles.
    pub(super) fn studio_add_rows(&self) -> Vec<(AddRow, Rect)> {
        let p = self.studio_add_rect();
        let mut y = p.y + 4;
        let mut out = Vec::new();
        for r in ADD_MENU {
            match r {
                Some(row) => { out.push((row, Rect::new(p.x + 4, y, p.w - 8, ROW))); y += ROW; }
                None => y += SEP,
            }
        }
        out
    }

    pub(super) fn studio_add_open(&mut self) {
        if self.menu_open { self.close_menu(); }
        self.studio.view_menu = false;
        self.studio.popup = None;
        self.studio.add_menu = true;
    }

    fn add_row_icon(&self, cv: &mut Canvas, row: AddRow, cx: i32, cy: i32, c: Rgb) {
        let f = self.fonts();
        match row {
            AddRow::Measurement => { for (i, h) in [6, 11, 8].into_iter().enumerate() { cv.fill(Rect::new(cx - 7 + i as i32 * 5, cy + 6 - h, 3, h), c); } }
            AddRow::ResultsTable => { cv.rect(Rect::new(cx - 7, cy - 6, 14, 12), c); cv.hline(cx - 7, cx + 6, cy, c); cv.vline(cx, cy - 6, cy + 5, c); }
            AddRow::Plot => { cv.line(cx - 7, cy + 5, cx - 2, cy - 3, c); cv.line(cx - 2, cy - 3, cx + 2, cy + 3, c); cv.line(cx + 2, cy + 3, cx + 7, cy - 5, c); }
            AddRow::Cursors => icon_cursors(cv, cx - 5, cy, c),
            AddRow::Search => { cv.circle(cx - 2, cy - 2, 5, c); cv.circle(cx - 2, cy - 2, 3, PANEL_BG); cv.line(cx + 2, cy + 2, cx + 7, cy + 7, c); }
            AddRow::Math => cv.text_in(&f.sb, Rect::new(cx - 10, cy - 9, 20, 18), 1, "fx", c),
            AddRow::Reference => cv.text_in(&f.sb, Rect::new(cx - 10, cy - 9, 20, 18), 1, "R", c),
            AddRow::Bus => { for dy in [-5, 0, 5] { cv.hline(cx - 7, cx + 7, cy + dy, c); } cv.vline(cx - 3, cy - 7, cy + 7, c); cv.vline(cx + 3, cy - 7, cy + 7, c); }
            AddRow::ZoomBox => { cv.round_outline(Rect::new(cx - 7, cy - 5, 14, 10), 1, c); cv.fill(Rect::new(cx - 7, cy - 5, 4, 2), c); }
            AddRow::ZoomControls => { cv.hline(cx - 7, cx + 7, cy - 4, c); cv.fill(Rect::new(cx - 3, cy - 1, 7, 5), c); }
            AddRow::Zoom => { cv.circle(cx - 2, cy - 2, 5, c); cv.circle(cx - 2, cy - 2, 3, PANEL_BG); cv.line(cx + 2, cy + 2, cx + 7, cy + 7, c); cv.hline(cx - 4, cx, cy - 2, c); cv.vline(cx - 2, cy - 4, cy, c); }
            AddRow::Strip => { cv.round_outline(Rect::new(cx - 8, cy - 5, 17, 11), 5, c); cv.circle(cx, cy, 2, c); }
            AddRow::MeasureSettings => icon_expand(cv, cx, cy, c),
        }
    }

    pub(super) fn draw_studio_add_menu(&self, cv: &mut Canvas) {
        if !self.studio.add_menu { return; }
        let f = self.fonts();
        let p = self.studio_add_rect();
        cv.round(p, 8, BTN_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        let btn = self.studio_top().add;
        let x = btn.x + btn.w / 2;
        cv.triangle([(x - 7, p.y + 1), (x + 7, p.y + 1), (x, p.y - 6)], BTN_BORDER);
        cv.triangle([(x - 6, p.y + 2), (x + 6, p.y + 2), (x, p.y - 4)], PANEL_BG);
        let mut sep_y = p.y + 4;
        for r in ADD_MENU {
            match r {
                Some(_) => sep_y += ROW,
                None => { cv.hline(p.x + 10, p.right() - 11, sep_y + SEP / 2, LINE); sep_y += SEP; }
            }
        }
        for (row, r) in self.studio_add_rows() {
            let on = self.add_row_on(row);
            self.add_row_icon(cv, row, r.x + 18, r.y + r.h / 2, if on { ACCENT } else { TEXT });
            cv.set_clip(Rect::new(r.x + 34, r.y, r.w - 70, r.h));
            cv.text_in(&f.m, Rect::new(r.x + 36, r.y, r.w - 70, r.h), 0, self.add_row_label(row), TEXT);
            cv.no_clip();
            if on && row != AddRow::Measurement { cv.text_in(&f.sb, Rect::new(r.right() - 40, r.y, 34, r.h), 2, "On", ACCENT); }
            if row == AddRow::Measurement && self.measurement_count() > 0 { cv.text_in(&f.sb, Rect::new(r.right() - 44, r.y, 38, r.h), 2, &self.measurement_count().to_string(), DIM); }
        }
    }

    /// A tap while the Add menu is open: a row does its action, a tap outside closes it. Always taken.
    pub(super) fn studio_add_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.studio.add_menu { return false; }
        let p = self.studio_add_rect();
        let chip = self.top_hit(self.studio_top().add);
        self.studio.add_menu = false;
        if chip.contains(x, y) || !p.contains(x, y) { return true; }
        let Some((row, _)) = self.studio_add_rows().into_iter().find(|(_, r)| r.contains(x, y)) else { return true };
        match row {
            AddRow::Measurement => self.open_meas_list(),
            AddRow::ResultsTable => self.studio_set_view(StudioView::Docked),
            AddRow::Plot => self.add_open_kind(AddKind::Plot),
            AddRow::Cursors => self.add_open_kind(AddKind::Cursors),
            AddRow::Search => self.add_open_kind(AddKind::Search),
            AddRow::Math => self.add_open_kind(AddKind::Math),
            AddRow::Reference => self.add_open_kind(AddKind::Ref),
            AddRow::Bus => self.add_open_kind(AddKind::Bus),
            AddRow::Zoom => { let on = !self.m.zoom; self.set_zoom(on); }
            AddRow::ZoomBox => self.arm_zoom_area(),
            AddRow::ZoomControls => { self.m.studio_zoom_tab = !self.m.studio_zoom_tab; self.zoom_tab_set(false); self.settings_changed(); self.relayout(); }
            AddRow::Strip => {
                self.studio.hide_badges = !self.studio.hide_badges;
                self.studio.md.hide_undo = None;
                self.relayout();
            }
            AddRow::MeasureSettings => self.toggle_menu(MenuId::Measure),
        }
        true
    }

    /// A page of the Add panel, straight from the menu.
    fn add_open_kind(&mut self, k: AddKind) {
        self.open_menu(MenuId::AddPanel);
        self.sheet.add.kind = None;
        self.rail_open_pub(k);
    }

    /// The chip of the Add button: lit while its menu is open.
    pub(super) fn studio_add_chip_on(&self) -> bool { self.studio.add_menu || (self.menu_open && self.menu == MenuId::AddPanel) }
}

