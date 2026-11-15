// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio: the Add measurement panel as a grid of tiles (a picture and a name each, nothing computed while browsing),
//! and the info card of a measurement: the picture, what it is, how it is computed, its sources and its unit.
use super::meascatalog::{cat_info, cat_of_key, Cat};
use super::sheet::SHEET_FOOT;
use super::studio::MeasKey;
use super::*;

/// Height of a tile, and the two columns of the grid.
pub(super) const TILE_H: i32 = 84;
pub(super) const TILE_COLS: i32 = 2;
const CARD_W: i32 = 470;
const CARD_H: i32 = 340;

impl Ui {
    /// The "i" of a tile: a 40 px target in its top right corner.
    pub(super) fn tile_info_rect(tile: Rect) -> Rect { Rect::new(tile.right() - 40, tile.y, 40, 40) }

    /// How many tiles one page of the panel holds.
    pub(super) fn tile_fit(avail_h: i32) -> usize { ((avail_h / TILE_H).max(1) * TILE_COLS) as usize }

    pub(super) fn tile_rect(list: Rect, i: usize) -> Rect {
        let w = (list.w - 4) / TILE_COLS;
        let (col, row) = (i as i32 % TILE_COLS, i as i32 / TILE_COLS);
        Rect::new(list.x + col * (w + 4), list.y + row * TILE_H, w, TILE_H - 4)
    }

    pub(super) fn tiles_avail(&self, list_y: i32) -> i32 {
        let (r, _, _) = self.sheet_chrome();
        r.bottom() - SHEET_FOOT - list_y
    }

    /// One tile: a picture over its name, a mark when it is added, the "i".
    pub(super) fn draw_add_tile(&self, cv: &mut Canvas, tile: Rect, cat: Cat) {
        let f = self.fonts();
        let info = cat_info(cat);
        let added = self.add_is_on(cat);
        let need = self.add_preview(cat);
        let off = matches!(need, super::meascatalog::Preview::Off(_) | super::meascatalog::Preview::Reason(_));
        cv.round(tile, 6, if added { ACCENT } else { BTN_BORDER }, 255);
        cv.round(tile.inset(1), 5, if added { mix(BTN, ACCENT, 28) } else { BTN }, 255);
        let col = if off { DIM } else if added { ACCENT } else { TEXT };
        super::caticons::draw_cat_icon(cv, f, Rect::new(tile.x + tile.w / 2 - 36, tile.y + 6, 72, 36), cat, col, if off { DIM } else { ACCENT });
        let lines = super::meascatalog::wrap_lines(&f.s, info.name, tile.w - 10);
        for (i, l) in lines.iter().take(2).enumerate() {
            cv.text_in(&f.s, Rect::new(tile.x + 4, tile.y + 42 + i as i32 * 15, tile.w - 8, 16), 1, l, col);
        }
        if added {
            let (x, y) = (tile.x + 10, tile.y + 12);
            for d in [0, 1] { cv.line(x, y + d, x + 4, y + 5 + d, ACCENT); cv.line(x + 4, y + 5 + d, x + 11, y - 5 + d, ACCENT); }
        }
        // the "i": a circle with a stem
        let i = Self::tile_info_rect(tile);
        let (cx, cy) = (i.x + i.w / 2 + 4, i.y + 16);
        cv.circle(cx, cy, 8, if off { DIM } else { LABEL });
        cv.circle(cx, cy, 7, BTN);
        cv.fill(Rect::new(cx - 1, cy - 1, 2, 6), TEXT);
        cv.fill(Rect::new(cx - 1, cy - 5, 2, 2), TEXT);
    }

    // ------------------------------------------------------------------------------------------------ the info card

    pub(super) fn info_card_rect(&self) -> Rect {
        let top = self.frame().top.bottom() + 10;
        Rect::new((self.scr.w - CARD_W) / 2, top, CARD_W, CARD_H)
    }

    pub(super) fn info_card_close(card: Rect) -> Rect { Rect::new(card.right() - 48, card.y + 4, 44, 44) }
    pub(super) fn info_card_action(card: Rect) -> Rect { Rect::new(card.right() - 150, card.bottom() - 54, 138, 44) }

    /// The info card of a measurement of the list ("About this measurement" in the row menu).
    pub(super) fn md_about(&mut self, key: MeasKey) {
        if let Some(c) = cat_of_key(key) {
            // the card's Add / Remove speaks of the source of the row
            if let MeasKey::One(_, id) | MeasKey::Pair(_, id, _) | MeasKey::Cat(_, id, ..) = key { self.meas_src = id; }
            self.studio.md.info = Some(c);
        } else { self.toast("No description for this measurement"); }
    }

    pub(super) fn draw_info_card(&self, cv: &mut Canvas) {
        let Some(cat) = self.studio.md.info else { return };
        let f = self.fonts();
        let info = cat_info(cat);
        let card = self.info_card_rect();
        cv.blend(Rect::new(0, 0, self.scr.w, self.scr.h), BG, 140);
        cv.round(card, 10, BTN_BORDER, 255);
        cv.round(card.inset(1), 9, PANEL_BG, 255);
        let pic = Rect::new(card.x + 16, card.y + 16, 110, 72);
        cv.round(pic, 6, BTN_BORDER, 255);
        cv.round(pic.inset(1), 5, BG, 255);
        super::caticons::draw_cat_icon(cv, f, pic.inset(6), cat, TEXT, ACCENT);
        cv.text(&f.lb, pic.right() + 16, card.y + 38, info.name, TEXT);
        cv.text(&f.s, pic.right() + 16, card.y + 62, &format!("Unit: {}", if info.unit.is_empty() { "none" } else { info.unit }), DIM);
        cv.text(&f.s, pic.right() + 16, card.y + 80, &info.sources, DIM);
        let close = Self::info_card_close(card);
        btn(cv, close.inset(4), BTN, BTN_BORDER);
        cv.line(close.x + 15, close.y + 15, close.right() - 15, close.bottom() - 15, TEXT);
        cv.line(close.x + 15, close.bottom() - 15, close.right() - 15, close.y + 15, TEXT);
        let w = card.w - 32;
        let mut y = card.y + 108;
        for (head, body) in [("What it is", info.blurb.to_string()), ("How it is computed", info.how.clone()), ("Settings", info.settings.clone())] {
            cv.text(&f.sb, card.x + 16, y, head, LABEL);
            y += 6;
            for l in super::meascatalog::wrap_lines(&f.s, &body, w) {
                y += 16;
                cv.text(&f.s, card.x + 16, y, &l, TEXT);
            }
            y += 16;
        }
        let a = Self::info_card_action(card);
        let added = self.add_is_on(cat);
        btn(cv, a, BTN, if added { RED } else { ACCENT });
        cv.text_in(&f.mb, a, 1, if added { "Remove" } else { "Add" }, if added { RED } else { ACCENT });
    }

    /// A tap while the card is open: its buttons, or anywhere else closes it. Always taken.
    pub(super) fn info_card_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(cat) = self.studio.md.info else { return false };
        let card = self.info_card_rect();
        if !card.contains(x, y) || Self::info_card_close(card).contains(x, y) { self.studio.md.info = None; return true; }
        if Self::info_card_action(card).contains(x, y) {
            self.sheet_add_toggle(cat);
            if !self.add_is_on(cat) { self.studio.md.info = None; }
        }
        true
    }
}
