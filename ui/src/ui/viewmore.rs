// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
//! The overflow menu of the views that replace the waveform (analysers, XY): a button in the plot leads to what they
//! need most, and above all out of them. The eye keeps its own bar: Full is the page, and Side and Under are extra.

use super::{AView, Item, MenuId, Rect, Ui, BTN_BORDER, PANEL_BG, TEXT};
use crate::gfx::Canvas;

impl Ui {
    /// A view other than the waveform fills the plot. The eye has its own bar and is not one of these.
    pub(super) fn view_on(&self) -> bool {
        self.xy || !matches!(self.m.view, AView::Scope | AView::Eye)
    }

    pub(super) fn view_name(&self) -> &'static str {
        if self.xy {
            return "XY";
        }
        match self.m.view {
            AView::Spectrum => "Spectrum",
            AView::Eye => "Eye / jitter",
            AView::Power => "Power analysis",
            AView::Bode => "Bode plot",
            AView::Step => "Bandwidth test",
            AView::Magnetic => "Magnetic",
            AView::Inverter => "Inverter",
            AView::Dpt => "Double pulse",
            AView::Scope => "Scope",
        }
    }

    pub(super) fn view_btn(&self) -> Rect {
        Rect::new(self.area.x + self.area.w - 48, self.area.y + 6, 42, 36)
    }

    pub(super) fn draw_view_btn(&self, cv: &mut Canvas) {
        let r = self.view_btn();
        cv.round(r, 6, PANEL_BG, 235);
        cv.round_outline(r, 6, BTN_BORDER);
        for k in -1..=1 {
            cv.circle(r.x + r.w / 2, r.y + r.h / 2 + 8 * k, 2, TEXT);
        }
    }

    fn view_menu(&self) -> Option<MenuId> {
        match self.m.view {
            _ if self.xy => None,
            AView::Spectrum => Some(MenuId::Spectrum),
            AView::Eye => Some(MenuId::Eye),
            AView::Power => Some(MenuId::Power),
            AView::Bode => Some(MenuId::Bode),
            AView::Step => Some(MenuId::BwTest),
            AView::Magnetic => Some(MenuId::Magnetic),
            AView::Inverter => Some(MenuId::Inverter),
            AView::Dpt => Some(MenuId::Dpt),
            AView::Scope => None,
        }
    }

    pub(super) fn view_more_items(&self) -> Vec<Item> {
        let mut v = Vec::new();
        if !self.xy {
            match self.m.view {
                AView::Eye => v.push(Item::EyeClear),
                AView::Spectrum => v.push(Item::SpecPeak),
                _ => {}
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
        ui.render(&mut Canvas::new(super::super::W, super::super::H));
        ui
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
