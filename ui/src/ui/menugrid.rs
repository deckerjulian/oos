// SPDX-License-Identifier: GPL-2.0-or-later
//! The Studio gear sheet is the grouped Menu: a group label at the left and a grid of buttons at the right, in one
//! scrolling page. A button that opens a sub-menu shows a chevron. Every button has a small line figure of its own.
//! Nothing about an entry is decided here: the groups and their items are `main_groups`, a tap is `activate`.
use super::sheet::{SHEET_FOOT, SHEET_HEAD};
use super::*;

const PAD: i32 = 12;
/// Width of the label column of the groups.
pub(super) const GROUP_LABEL_W: i32 = 78;
pub(super) const CELL_H: i32 = 40;
const GAP: i32 = 4;
const GROUP_GAP: i32 = 12;
const ICON: i32 = 18;
/// Room under the last group, so the last row can scroll fully into view.
const BOTTOM_PAD: i32 = 8;
/// The least width of a button, and the most buttons in a row.
const CELL_MIN_W: i32 = 124;
const MAX_COLS: i32 = 6;

pub(super) struct MenuGroupGeo {
    pub name: &'static str,
    pub label: Rect,
    pub items: Vec<(Item, Rect)>,
}

pub(super) struct MenuGeo {
    #[cfg_attr(not(test), allow(dead_code))] // only the tests use it
    pub r: Rect,
    /// the part of the sheet that scrolls
    pub view: Rect,
    pub groups: Vec<MenuGroupGeo>,
    pub content_h: i32,
}

impl MenuGeo {
    pub fn max_scroll(&self) -> i32 {
        (self.content_h - self.view.h).max(0)
    }
}

impl Ui {
    /// The text on a button of the Menu.
    /// Width of the label column: the widest group name at the text size in use, never under `GROUP_LABEL_W`.
    pub(super) fn group_label_w(&self) -> i32 {
        let widest = self.main_groups().iter().map(|g| self.fonts().sb.width(g.0)).max().unwrap_or(0);
        GROUP_LABEL_W.max(widest + 12)
    }

    pub(super) fn menu_cell_label(&self, it: Item) -> String {
        use Item::*;
        use MenuId as M;
        let s = match it {
            RunStop => "Run/Stop",
            SingleAct => "Single",
            AutosetAct => "Autoset",
            ClearDisp => "Clear",
            DefaultAct => "Default",
            Open(M::Acquire) => "Acquire",
            Open(M::Trigger) => "Trigger",
            SlopeAct => "Slope",
            ForceAct => "Force",
            Open(M::Zone) => "Zones",
            Open(M::Measure) => "Measure",
            Open(M::Cursor) => "Cursor",
            Open(M::Math) => "Math",
            Open(M::Ref) => "Reference",
            Open(M::Sim) => "SIM",
            Open(M::Decode) => "Decode",
            Open(M::Bus) => "Bus",
            Open(M::Mask) => "Mask",
            Open(M::Spectrum) => "Spectrum",
            Open(M::Eye) => "Eye/jitter",
            Open(M::Power) => "Power analysis",
            Open(M::Bode) => "Bode",
            Open(M::BwTest) => "Bandwidth",
            IdentifyAct => "Identify",
            Open(M::Magnetic) => "Magnetic",
            Open(M::Inverter) => "Inverter",
            Open(M::Dpt) => "Double pulse",
            Open(M::Afg) => "Generator",
            Open(M::La) => "Logic analyser",
            Open(M::Dvm) => "DVM/counter",
            Open(M::Logger) => "Data logger",
            Open(M::Display) => "Display",
            Open(M::Horizontal) => "Horizontal",
            NavHist => "History",
            NavBack => "Older",
            NavFwd => "Newer",
            Open(M::Storage) => "Storage",
            Shot => "Screenshot",
            QuickRun => return format!("Quick \u{b7} {}", self.m.quick.short()),
            LockAct => "Touch lock",
            Open(M::Utility) => "System",
            PowerAct => "Power off",
            other => return self.item_label(other).to_string(),
        };
        s.to_string()
    }

    /// Buttons per row in a Menu `w` wide (the sheet): as many as keep a button at least `CELL_MIN_W` wide, up to six.
    #[cfg_attr(not(test), allow(dead_code))] // only the tests use it
    pub(super) fn menu_cols(w: i32) -> i32 { Self::menu_cols_with(w, GROUP_LABEL_W) }
    fn menu_cols_with(w: i32, label_w: i32) -> i32 {
        let gw = w - 2 * PAD - label_w - 6;
        ((gw + GAP) / (CELL_MIN_W + GAP)).clamp(1, MAX_COLS)
    }

    /// Height of the page of the Menu for `cols` buttons per row, with the room under the last row.
    pub(super) fn menu_content_h(&self, cols: i32) -> i32 {
        let all = self.main_groups();
        let height = |n: usize| { let rows = (n as i32 + cols - 1) / cols; rows * CELL_H + (rows - 1) * GAP };
        (all.iter().map(|(_, items)| height(items.len()) + GROUP_GAP).sum::<i32>() - GROUP_GAP).max(0) + BOTTOM_PAD
    }

    /// The Menu is the wide overlay: it is the main page of the Studio menu and floats over the waveform.
    pub(super) fn menu_is_wide(&self) -> bool {
        self.m.layout == Layout::Studio && self.menu_open && self.menu == MenuId::Main
    }

    /// The place of the wide Menu: nearly the width of the screen, as high as its page (or as the body, when that is shorter).
    pub(super) fn menu_wide_rect(&self) -> Rect {
        let f = self.frame();
        let y = f.top.bottom();
        let w = self.scr.w - 16;
        let avail = (f.bottom.y - y).max(SHEET_HEAD + SHEET_FOOT);
        let want = SHEET_HEAD + 4 + self.menu_content_h(Self::menu_cols_with(w, self.group_label_w())) + 6;
        Rect::new(8, y, w, want.min(avail))
    }

    /// The geometry of the Menu: groups down the page, buttons in rows of up to six. The page is moved up by the scroll, which
    /// is kept inside what the real content height allows (the last row can scroll fully into view, with its padding).
    pub(super) fn menu_geo(&self) -> MenuGeo {
        let r = self.sheet_rect();
        let view = Rect::new(r.x + PAD, r.y + SHEET_HEAD + 4, r.w - 2 * PAD, (r.bottom() - r.y - SHEET_HEAD - 10).max(CELL_H));
        let cols = Self::menu_cols_with(r.w, self.group_label_w());
        let gx = view.x + self.group_label_w();
        let gw = view.w - self.group_label_w() - 6;
        let cw = (gw - (cols - 1) * GAP) / cols;
        let all = self.main_groups();
        let height = |n: usize| { let rows = (n as i32 + cols - 1) / cols; rows * CELL_H + (rows - 1) * GAP };
        let content_h = self.menu_content_h(cols);
        let scroll = self.sheet.scroll.clamp(0, (content_h - view.h).max(0));
        let mut y = 0;
        let mut groups = Vec::new();
        for (name, items) in all {
            let h = height(items.len());
            let cells = items.iter().enumerate().map(|(i, &it)| {
                let (c, row) = (i as i32 % cols, i as i32 / cols);
                (it, Rect::new(gx + c * (cw + GAP), view.y + y - scroll + row * (CELL_H + GAP), cw, CELL_H))
            }).collect();
            groups.push(MenuGroupGeo { name, label: Rect::new(view.x, view.y + y - scroll, self.group_label_w() - 6, CELL_H), items: cells });
            y += h + GROUP_GAP;
        }
        MenuGeo { r, view, groups, content_h }
    }

    pub(super) fn draw_menu_grid(&self, cv: &mut Canvas) {
        let f = self.fonts();
        self.sheet_draw_chrome(cv, &self.menu_title(MenuId::Main));
        let g = self.menu_geo();
        // everything of the page is drawn inside the body of the sheet: what is over its edge is cut
        cv.set_clip(g.view);
        for (n, grp) in g.groups.iter().enumerate() {
            if grp.label.bottom() >= g.view.y && grp.label.y <= g.view.bottom() {
                cv.text_fit(&f.sb, grp.label, 0, grp.name, LABEL);
            }
            if n > 0 {
                let y = grp.label.y - GROUP_GAP / 2 - 1;
                if y >= g.view.y && y < g.view.bottom() { cv.hline(g.view.x, g.view.right() - 8, y, LINE); }
            }
            for &(it, c) in &grp.items {
                if c.bottom() < g.view.y || c.y > g.view.bottom() { continue; }
                self.draw_menu_cell(cv, it, c, g.view);
            }
        }
        cv.no_clip();
        // the scroll bar of a page that is longer than the sheet
        if g.max_scroll() > 0 {
            let track = Rect::new(g.view.right() - 4, g.view.y, 3, g.view.h);
            let h = (track.h * g.view.h / g.content_h).max(24);
            let y = track.y + (track.h - h) * self.sheet.scroll / g.max_scroll();
            cv.round(track, 1, BTN_BORDER, 120);
            cv.round(Rect::new(track.x, y, 3, h), 1, DIM, 255);
        }
    }

    fn menu_armed(&self, it: Item) -> bool {
        self.studio.md.armed.is_some_and(|(a, t)| a == it && self.now - t < 3.0)
    }

    fn draw_menu_cell(&self, cv: &mut Canvas, it: Item, c: Rect, view: Rect) {
        let f = self.fonts();
        let armed = self.menu_armed(it);
        let danger = matches!(it, Item::DefaultAct | Item::PowerAct);
        let on = match it {
            Item::RunStop => self.m.run == RunState::Run,
            Item::SingleAct => self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run,
            Item::NavHist => self.hist_pos.is_some(),
            Item::LockAct => self.m.touch_lock,
            _ => false,
        };
        let (fill, border, ink) = if armed {
            (mix(DLG_BG, RED, 90), RED, TEXT)
        } else if on {
            (ACCENT, ACCENT, DARK_TEXT)
        } else if danger {
            (mix(DLG_BG, RED, 30), RED, RED)
        } else {
            (BTN, BTN_BORDER, TEXT)
        };
        btn(cv, c, fill, border);
        draw_menu_icon(cv, it, c.x + 8, c.y + (c.h - ICON) / 2, if on || armed { ink } else if danger { RED } else { ACCENT }, fill);
        let open = matches!(it, Item::Open(_));
        let label = if armed { "Tap again".to_string() } else { self.menu_cell_label(it) };
        let room = c.w - 32 - if open { 16 } else { 6 };
        // a label that does not fit in the larger font goes in the smaller one, then on two lines; the body of the sheet
        // cuts it at its edges
        let outer = cv.save_clip();
        let (top, bottom) = (c.y.max(view.y), c.bottom().min(view.bottom()));
        cv.clip_within(Rect::new(c.x + 30, top, room + 2, (bottom - top).max(0)));
        if f.m.width(&label) <= room { cv.text_in(&f.m, Rect::new(c.x + 30, c.y, room, c.h), 0, &label, ink); }
        else if f.s.width(&label) <= room { cv.text_in(&f.s, Rect::new(c.x + 30, c.y, room, c.h), 0, &label, ink); }
        else { fitted_text(cv, &[&f.s], Rect::new(c.x + 28, c.y, room + 6, c.h), &label, ink); }
        cv.restore_clip(outer);
        if open {
            chevron(cv, c.right() - 12, c.y + c.h / 2, 1, if on { ink } else { DIM });
        }
    }

    /// A tap on the Menu. False: outside the sheet.
    pub(super) fn menu_grid_tap(&mut self, x: i32, y: i32) -> bool {
        let g = self.menu_geo();
        if !g.view.contains(x, y) {
            return true;
        }
        let hit = g.groups.iter().flat_map(|grp| grp.items.iter()).find(|(_, c)| c.contains(x, y)).map(|&(it, _)| it);
        let Some(it) = hit else { return true };
        // the factory reset and power off ask first: the second tap within 3 s does it
        if matches!(it, Item::DefaultAct | Item::PowerAct) {
            if !self.menu_armed(it) {
                self.studio.md.armed = Some((it, self.now));
                return true;
            }
            self.studio.md.armed = None;
        }
        self.activate(it);
        true
    }
}

/// A figure of 18 px at (x, y) in the colour `c`; `bg` is the colour of the button (for the hollow parts).
pub(super) fn draw_menu_icon(cv: &mut Canvas, it: Item, x: i32, y: i32, c: Rgb, bg: Rgb) {
    use Item::*;
    use MenuId as M;
    let mut i = Ic { cv, x, y, c, bg };
    match it {
        RunStop => { i.tri((0.1, 0.1), (0.1, 0.9), (0.55, 0.5)); i.bar(0.68, 0.1, 0.8, 0.9); i.bar(0.9, 0.1, 1.0, 0.9); }
        SingleAct => { i.ring(0.5, 0.5, 8); i.text1(); }
        AutosetAct => { i.wave(); i.l((0.0, 0.1), (0.3, 0.1)); i.l((0.7, 0.9), (1.0, 0.9)); }
        ClearDisp => { i.l((0.15, 0.15), (0.85, 0.85)); i.l((0.15, 0.85), (0.85, 0.15)); }
        DefaultAct => { i.ring(0.5, 0.55, 7); i.cut(0.5, 0.2, 0.8, 0.5); i.tri((0.5, 0.0), (0.5, 0.4), (0.8, 0.2)); }
        Open(M::Acquire) => { i.pl(&[(0.0, 0.5), (0.2, 0.1), (0.5, 0.9), (0.8, 0.1), (1.0, 0.5)]); i.dot(0.2, 0.1); i.dot(0.5, 0.9); i.dot(0.8, 0.1); }
        Open(M::Trigger) => { i.pl(&[(0.0, 0.8), (0.4, 0.8), (0.4, 0.2), (1.0, 0.2)]); i.l((0.7, 0.5), (1.0, 0.5)); i.l((0.9, 0.4), (1.0, 0.5)); i.l((0.9, 0.6), (1.0, 0.5)); }
        SlopeAct => { i.pl(&[(0.0, 0.9), (0.3, 0.9), (0.7, 0.1), (1.0, 0.1)]); }
        ForceAct => { i.pl(&[(0.65, 0.0), (0.25, 0.55), (0.55, 0.55), (0.35, 1.0)]); }
        Open(M::Zone) => { i.dashed_box(0.05, 0.2, 0.95, 0.8); }
        Open(M::Measure) => { i.wave(); i.l((0.0, 1.0), (1.0, 1.0)); }
        Open(M::Cursor) => { i.dash_v(0.25); i.dash_v(0.75); i.l((0.25, 0.5), (0.75, 0.5)); }
        Open(M::Math) => { i.l((0.0, 0.3), (0.5, 0.3)); i.l((0.25, 0.05), (0.25, 0.55)); i.l((0.55, 0.8), (1.0, 0.8)); i.l((0.55, 0.3), (1.0, 0.3)); }
        Open(M::Ref) => { i.pl(&[(0.15, 0.0), (0.15, 1.0), (0.85, 1.0), (0.85, 0.0), (0.5, 0.35), (0.15, 0.0)]); }
        Open(M::Sim) => { i.pl(&[(0.0, 0.8), (0.2, 0.8), (0.2, 0.2), (0.5, 0.2), (0.5, 0.8), (0.8, 0.8), (0.8, 0.2), (1.0, 0.2)]); }
        Open(M::Decode) => { i.pl(&[(0.3, 0.1), (0.0, 0.5), (0.3, 0.9)]); i.pl(&[(0.7, 0.1), (1.0, 0.5), (0.7, 0.9)]); i.dot(0.5, 0.5); }
        Open(M::Bus) => { for v in [0.15, 0.5, 0.85] { i.l((0.0, v), (1.0, v)); } i.bar(0.3, 0.05, 0.4, 0.95); }
        Open(M::Mask) => { i.pl(&[(0.0, 0.9), (0.25, 0.9), (0.35, 0.55), (0.65, 0.55), (0.75, 0.9), (1.0, 0.9)]); i.pl(&[(0.25, 0.1), (0.75, 0.1)]); }
        Open(M::Spectrum) => { for (k, h) in [0.4, 0.9, 0.6, 0.3, 0.5].into_iter().enumerate() { let u = k as f32 * 0.22; i.bar(u, 1.0 - h, u + 0.12, 1.0); } }
        Open(M::Eye) => { i.pl(&[(0.0, 0.5), (0.25, 0.1), (0.75, 0.1), (1.0, 0.5), (0.75, 0.9), (0.25, 0.9), (0.0, 0.5)]); i.pl(&[(0.0, 0.5), (0.3, 0.5)]); i.pl(&[(0.7, 0.5), (1.0, 0.5)]); }
        Open(M::Power) => { i.pl(&[(0.0, 0.5), (0.15, 0.2), (0.35, 0.8), (0.5, 0.5)]); i.pl(&[(0.5, 0.5), (0.65, 0.2), (0.85, 0.8), (1.0, 0.5)]); i.l((0.0, 1.0), (1.0, 1.0)); }
        Open(M::Bode) => { i.pl(&[(0.0, 0.2), (0.5, 0.2), (0.7, 0.4), (1.0, 0.9)]); i.l((0.0, 0.0), (0.0, 1.0)); }
        Open(M::BwTest) => { i.pl(&[(0.0, 0.2), (0.5, 0.2), (0.75, 0.3), (1.0, 0.9)]); i.dash_h(0.35, 0.0, 1.0); }
        IdentifyAct => { i.ring(0.4, 0.4, 6); i.l((0.7, 0.7), (1.0, 1.0)); }
        Open(M::Magnetic) => { i.pl(&[(0.15, 1.0), (0.15, 0.4), (0.3, 0.1), (0.7, 0.1), (0.85, 0.4), (0.85, 1.0)]); i.bar(0.05, 0.85, 0.3, 1.0); i.bar(0.7, 0.85, 0.95, 1.0); }
        Open(M::Inverter) => { i.wave(); i.l((0.0, 0.5), (1.0, 0.5)); }
        Open(M::Dpt) => { i.pl(&[(0.0, 0.9), (0.15, 0.9), (0.15, 0.1), (0.4, 0.1), (0.4, 0.9), (0.6, 0.9), (0.6, 0.1), (0.75, 0.1), (0.75, 0.9), (1.0, 0.9)]); }
        Open(M::Afg) => { i.frame(); i.wave(); }
        Open(M::La) => { for (k, v) in [0.1, 0.5, 0.9].into_iter().enumerate() { let _ = k; i.pl(&[(0.0, v), (0.3, v), (0.3, v - 0.1), (0.6, v - 0.1), (0.6, v), (1.0, v)]); } }
        Open(M::Dvm) => { i.frame(); i.pl(&[(0.25, 0.3), (0.25, 0.7), (0.5, 0.7), (0.5, 0.3), (0.25, 0.3)]); i.l((0.72, 0.3), (0.72, 0.7)); }
        Open(M::Logger) => { i.l((0.0, 0.0), (0.0, 1.0)); i.l((0.0, 1.0), (1.0, 1.0)); i.pl(&[(0.1, 0.7), (0.35, 0.3), (0.6, 0.6), (1.0, 0.1)]); }
        Open(M::Display) => { i.frame_to(0.8); i.l((0.5, 0.8), (0.5, 1.0)); i.l((0.25, 1.0), (0.75, 1.0)); }
        Open(M::Horizontal) => { i.l((0.0, 0.5), (1.0, 0.5)); i.l((0.0, 0.5), (0.2, 0.3)); i.l((0.0, 0.5), (0.2, 0.7)); i.l((1.0, 0.5), (0.8, 0.3)); i.l((1.0, 0.5), (0.8, 0.7)); }
        NavHist => { i.ring(0.5, 0.5, 8); i.l((0.5, 0.5), (0.5, 0.2)); i.l((0.5, 0.5), (0.72, 0.6)); }
        NavBack => { i.pl(&[(0.7, 0.1), (0.25, 0.5), (0.7, 0.9)]); }
        NavFwd => { i.pl(&[(0.3, 0.1), (0.75, 0.5), (0.3, 0.9)]); }
        Open(M::Storage) => { i.pl(&[(0.05, 0.05), (0.95, 0.05), (0.95, 0.95), (0.25, 0.95), (0.05, 0.75), (0.05, 0.05)]); i.pl(&[(0.25, 0.05), (0.25, 0.35), (0.75, 0.35), (0.75, 0.05)]); }
        Shot => { i.frame_to(0.85); i.ring(0.5, 0.45, 4); i.bar(0.15, 0.05, 0.3, 0.2); }
        QuickRun => { i.pl(&[(0.65, 0.0), (0.25, 0.55), (0.55, 0.55), (0.35, 1.0)]); }
        LockAct => { i.frame_from(0.45, 0.2, 0.8, 1.0); i.pl(&[(0.3, 0.45), (0.3, 0.2), (0.5, 0.0), (0.7, 0.2), (0.7, 0.45)]); }
        Open(M::Utility) => { super::studio::icon_gear(i.cv, x + 9, y + 9, c, bg); }
        PowerAct => { i.ring(0.5, 0.55, 7); i.cut(0.4, 0.0, 0.6, 0.45); i.l((0.5, 0.0), (0.5, 0.5)); }
        _ => { i.frame(); }
    }
}

struct Ic<'a> {
    cv: &'a mut Canvas,
    x: i32,
    y: i32,
    c: Rgb,
    bg: Rgb,
}

type P = (f32, f32);

impl Ic<'_> {
    fn p(&self, u: f32, v: f32) -> (i32, i32) {
        (self.x + (u * (ICON - 1) as f32).round() as i32, self.y + (v * (ICON - 1) as f32).round() as i32)
    }
    fn l(&mut self, a: P, b: P) {
        let (p, q) = (self.p(a.0, a.1), self.p(b.0, b.1));
        let c = self.c;
        self.cv.line(p.0, p.1, q.0, q.1, c);
    }
    fn pl(&mut self, pts: &[P]) {
        for w in pts.windows(2) { self.l(w[0], w[1]); }
    }
    fn dot(&mut self, u: f32, v: f32) {
        let p = self.p(u, v);
        let c = self.c;
        self.cv.circle(p.0, p.1, 1, c);
    }
    fn bar(&mut self, u0: f32, v0: f32, u1: f32, v1: f32) {
        let (a, b) = (self.p(u0, v0), self.p(u1, v1));
        let c = self.c;
        self.cv.fill(Rect::new(a.0, a.1, (b.0 - a.0).max(2), (b.1 - a.1).max(1)), c);
    }
    fn tri(&mut self, a: P, b: P, d: P) {
        let (a, b, d) = (self.p(a.0, a.1), self.p(b.0, b.1), self.p(d.0, d.1));
        let c = self.c;
        self.cv.triangle([a, b, d], c);
    }
    fn ring(&mut self, u: f32, v: f32, r: i32) {
        let p = self.p(u, v);
        let (c, bg) = (self.c, self.bg);
        self.cv.circle(p.0, p.1, r, c);
        self.cv.circle(p.0, p.1, r - 2, bg);
    }
    /// A gap in a ring: the button colour over a box.
    fn cut(&mut self, u0: f32, v0: f32, u1: f32, v1: f32) {
        let (a, b) = (self.p(u0, v0), self.p(u1, v1));
        let bg = self.bg;
        self.cv.fill(Rect::new(a.0, a.1, (b.0 - a.0).max(1), (b.1 - a.1).max(1)), bg);
    }
    fn text1(&mut self) {
        self.l((0.4, 0.35), (0.55, 0.25));
        self.l((0.55, 0.25), (0.55, 0.75));
    }
    fn wave(&mut self) {
        self.pl(&[(0.0, 0.5), (0.12, 0.15), (0.25, 0.5), (0.38, 0.85), (0.5, 0.5), (0.62, 0.15), (0.75, 0.5), (0.88, 0.85), (1.0, 0.5)]);
    }
    fn frame(&mut self) { self.frame_from(0.0, 0.0, 1.0, 1.0) }
    fn frame_to(&mut self, v1: f32) { self.frame_from(0.0, 0.0, 1.0, v1) }
    fn frame_from(&mut self, u0: f32, v0: f32, u1: f32, v1: f32) {
        let (a, b) = (self.p(u0, v0), self.p(u1, v1));
        let c = self.c;
        self.cv.rect(Rect::new(a.0, a.1, b.0 - a.0 + 1, b.1 - a.1 + 1), c);
    }
    fn dashed_box(&mut self, u0: f32, v0: f32, u1: f32, v1: f32) {
        let (a, b) = (self.p(u0, v0), self.p(u1, v1));
        let c = self.c;
        self.cv.dashed_hline(a.0, b.0, a.1, 3, 2, c);
        self.cv.dashed_hline(a.0, b.0, b.1, 3, 2, c);
        self.cv.dashed_vline(a.0, a.1, b.1, 3, 2, c);
        self.cv.dashed_vline(b.0, a.1, b.1, 3, 2, c);
    }
    fn dash_v(&mut self, u: f32) {
        let (a, b) = (self.p(u, 0.0), self.p(u, 1.0));
        let c = self.c;
        self.cv.dashed_vline(a.0, a.1, b.1, 3, 2, c);
    }
    fn dash_h(&mut self, v: f32, u0: f32, u1: f32) {
        let (a, b) = (self.p(u0, v), self.p(u1, v));
        let c = self.c;
        self.cv.dashed_hline(a.0, b.0, a.1, 3, 2, c);
    }
}
