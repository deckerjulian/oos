// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Layout check of every menu: nothing outside the screen or the dialog, no text wider than its control, no row
//! without a label. It walks the menus in all trigger types, decoders and analyser pages, with the extreme values of
//! the settings. A text that runs out of its button looks fine in a test of the logic, so this reads the geometry.
//!
//! The checks live in the modules below, one per topic; this file keeps what they share. A new check goes into the
//! module of its topic.

use super::*;

#[cfg(test)]
mod controls;
#[cfg(test)]
mod gallery;
#[cfg(test)]
mod loaded_refs;
#[cfg(test)]
mod menu_layout;
#[cfg(test)]
mod pane_views;
#[cfg(test)]
mod screen_layout;

/// The screens the layout is checked on: the panel, a 10" panel (1280 x 800) and a Full HD monitor.
pub(super) const SCREENS: [Screen; 3] = [Screen::PANEL, Screen { w: 1280, h: 800 }, Screen { w: 1920, h: 1080 }];

fn demo_on(scr: Screen) -> Ui {
    let mut u = crate::ui::testkit::demo_ui();
    u.scr = scr;
    u
}

/// Stage of frames with demo signals on channels 1 and 2.
fn with_two_channels() -> Ui {
    let mut u = crate::ui::testkit::demo_ui();
    u.m.ch[1].on = true;
    for _ in 0..12 {
        u.step(0.04);
    }
    u
}
