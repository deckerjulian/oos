// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later
//! The overflow menu of the views that replace the waveform (analysers, XY): a button in the plot leads to what they
//! need most, and above all out of them. The eye keeps its own bar: Full is the page, and Side and Under are extra.

use super::{AView, Item, MenuId, Rect, Ui, BTN_BORDER, PANEL_BG, TEXT};
use crate::gfx::Canvas;

impl Ui {
    /// A view other than the waveform fills the plot. The eye has its own bar and is not one of these.
    /// A view other than the waveform fills the plot. Studio docks them as views of their own instead (`view_tile`),
    /// beside the waveform, which stays.
    pub(super) fn view_on(&self) -> bool {
        !self.dock_on() && self.view_tile()
    }

    /// XY or an analyser is on.
    pub(super) fn view_tile(&self) -> bool {
        self.xy || !matches!(self.m.view, AView::Scope | AView::Eye)
    }

    /// XY replaces the waveform, except in Studio where it is a view of its own.
    pub(super) fn xy_over(&self) -> bool { self.xy && !self.dock_on() }

    pub(super) fn view_name(&self) -> &'static str {
        if self.xy {
            return "XY";
        }
        super::analysers::of(self.m.view).map_or("Scope", |a| a.name())
    }

    /// The tile that holds the XY picture or the analyser (Studio): its content rectangle.
    pub(super) fn view_tile_rect(&self) -> Option<Rect> {
        if !self.dock_on() { return None; }
        let k = if self.xy { super::dock::VKind::Xy } else { super::dock::VKind::View };
        self.dock_content(k)
    }

    pub(super) fn view_btn(&self) -> Rect {
        if let Some(t) = self.view_tile_rect() { return Rect::new(t.right() - 48, t.y + 6, 42, 36); }
        Rect::new(self.area.x + self.area.w - 48, self.area.y + 6, 42, 36)
    }

    pub(super) fn draw_view_btn(&self, cv: &mut Canvas) {
        let r = self.view_btn();
        cv.round(r, 6, PANEL_BG, 235);
        cv.round_outline(r, 6, BTN_BORDER);
        for k in -1..=1 {
            cv.circle(r.x + r.w / 2, r.y + r.h / 2 + 8 * k, 2, TEXT);
        }
        self.draw_view_fix(cv);
    }

    /// What stands between the view and a result, as a button in its middle: a source that is off (the same fix as the
    /// footer of its menu), or a Bode plot without a sweep. (button, label)
    pub(super) fn view_fix_btn(&self) -> Option<(Rect, String)> {
        let menu = self.view_menu()?;
        if !self.view_tile() || self.menu_open { return None; }
        let off = self.off_sources(menu);
        let label = if !off.is_empty() {
            format!("Turn on {}", off.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join(", "))
        } else if self.m.view == AView::Bode && !self.bode.running && self.bode.points.is_empty() {
            "Start sweep".to_string()
        } else {
            return None;
        };
        let t = self.view_tile_rect().unwrap_or(self.area);
        let w = 200.min(t.w - 16);
        Some((Rect::new(t.x + (t.w - w) / 2, t.y + t.h / 2 - 20, w, 40), label))
    }

    fn draw_view_fix(&self, cv: &mut Canvas) {
        let Some((r, label)) = self.view_fix_btn() else { return };
        cv.round(r, 8, PANEL_BG, 245);
        cv.round_outline(r, 8, TEXT);
        cv.text_in(&self.fonts().sb, r, 1, &label, TEXT);
    }

    /// A tap on the fix button of the view. True when it took the tap.
    pub(super) fn view_fix_tap(&mut self, x: i32, y: i32) -> bool {
        let Some((r, _)) = self.view_fix_btn() else { return false };
        if !r.contains(x, y) { return false; }
        let Some(menu) = self.view_menu() else { return false };
        let off = self.off_sources(menu);
        if off.is_empty() {
            self.activate(Item::BodeRun);
        } else {
            for &c in &off { self.m.ch[c].on = true; }
            self.settings_changed();
            self.relayout();
            self.toast(&format!("{} on", off.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join(", ")));
        }
        true
    }

    fn view_menu(&self) -> Option<MenuId> {
        if self.xy {
            return None;
        }
        super::analysers::of(self.m.view).map(|a| a.menu())
    }

    pub(super) fn view_more_items(&self) -> Vec<Item> {
        let mut v = Vec::new();
        if !self.xy_over() {
            if let Some(a) = super::analysers::of(self.m.view) {
                v.extend(a.more(self));
            }
        }
        if self.view_menu().is_some() {
            v.push(Item::ViewSettings);
        }
        v.push(Item::ViewExit);
        v
    }

    /// Back to the waveform; a running Bode sweep ends and gives the generator back.
    pub(super) fn view_exit(&mut self) {
        if self.bode.running {
            self.bode_finish("Bode sweep stopped");
        }
        self.xy = false;
        self.m.view = AView::Scope;
        self.close_menu();
        self.analyzer_update();
    }

    pub(super) fn view_settings(&mut self) {
        if let Some(id) = self.view_menu() {
            self.open_menu(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Item, MenuId, Source, Ui};
    use super::*;

    fn at(view: AView) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.view = view;
        ui.step(1.0 / 30.0);
        ui.render(&mut Canvas::new(super::super::PANEL_W, super::super::PANEL_H));
        ui
    }

    #[test]
    fn a_view_with_a_source_off_offers_to_switch_it_on() {
        for layout in [crate::model::Layout::Minimal, crate::model::Layout::Studio] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.layout = layout;
            ui.relayout();
            ui.m.view = AView::Power;
            for _ in 0..3 { ui.step(1.0 / 30.0); }
            ui.render(&mut Canvas::new(super::super::PANEL_W, super::super::PANEL_H));
            assert!(!ui.m.ch[1].on);
            let (r, label) = ui.view_fix_btn().expect("power needs CH2");
            assert_eq!(label, "Turn on CH2", "{layout:?}");
            ui.tap(r.x + r.w / 2, r.y + r.h / 2);
            assert!(ui.m.ch[1].on, "{layout:?}");
            ui.step(1.0 / 30.0);
            assert!(ui.view_fix_btn().is_none(), "{layout:?}");
        }
        let mut ui = at(AView::Bode);
        ui.m.ch[1].on = true;
        ui.step(1.0 / 30.0);
        let (r, label) = ui.view_fix_btn().expect("no sweep yet");
        assert_eq!(label, "Start sweep");
        ui.tap(r.x + r.w / 2, r.y + r.h / 2);
        assert!(ui.bode.running);
    }

    #[test]
    fn the_button_opens_the_menu_and_exit_ends_the_view() {
        for view in [AView::Spectrum, AView::Power, AView::Bode, AView::Step, AView::Magnetic, AView::Inverter, AView::Dpt] {
            let mut ui = at(view);
            let r = ui.view_btn();
            ui.tap(r.x + 5, r.y + 5);
            assert!(ui.menu_open && ui.menu == MenuId::ViewMore, "{:?}", view);
            assert!(ui.view_more_items().contains(&Item::ViewExit));
            ui.activate(Item::ViewExit);
            assert_eq!(ui.m.view, AView::Scope);
            assert!(!ui.menu_open);
        }
    }

    #[test]
    fn eye_can_be_cleared_from_the_menu_and_stays_on() {
        let mut ui = at(AView::Eye);
        assert_eq!(ui.view_more_items(), vec![Item::EyeClear, Item::ViewSettings, Item::ViewExit]);
        ui.azr.eye_res = None;
        ui.activate(Item::EyeClear);
        assert_eq!(ui.m.view, AView::Eye);
        assert!(ui.azr.eye.is_none());
    }

    #[test]
    fn xy_has_only_exit() {
        let mut ui = at(AView::Scope);
        ui.xy = true;
        assert_eq!(ui.view_more_items(), vec![Item::ViewExit]);
        ui.activate(Item::ViewExit);
        assert!(!ui.xy);
    }

    #[test]
    fn the_waveform_has_no_button() {
        let mut ui = at(AView::Scope);
        let r = ui.view_btn();
        ui.tap(r.x + 5, r.y + 5);
        assert!(!(ui.menu_open && ui.menu == MenuId::ViewMore));
    }
}
