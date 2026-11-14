// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The desktop front panel uses the exact key and knob commands exposed by the web remote.

use crate::gfx::{rgb, Canvas, Rect};
use crate::model::{RunState, TrigMode};
use crate::ui::{Ui, W};

pub const WIDTH: i32 = 232;
const BG: u32 = rgb(22, 25, 30);
const BUTTON: u32 = rgb(36, 41, 49);
const BORDER: u32 = rgb(66, 73, 84);
const TEXT: u32 = rgb(229, 233, 238);
const DIM: u32 = rgb(164, 174, 186);
const ACCENT: u32 = rgb(240, 169, 59);

pub struct Control {
    pub rect: Rect,
    pub label: &'static str,
    pub command: &'static str,
}

pub fn controls() -> Vec<Control> {
    let x = W + 12;
    let mut out = Vec::new();
    let mut row = |y, labels: &[(&'static str, &'static str)]| {
        let width = (WIDTH - 24 - (labels.len() as i32 - 1) * 6) / labels.len() as i32;
        for (i, &(label, command)) in labels.iter().enumerate() {
            out.push(Control { rect: Rect::new(x + i as i32 * (width + 6), y, width, 29), label, command });
        }
    };
    row(36, &[("Load file", "workflow load"), ("Live", "workflow live"), ("SIM", "workflow sim")]);
    row(86, &[("Run / Stop", "key runstop"), ("Single", "key single")]);
    row(121, &[("Autoset", "key auto"), ("Clear", "key clear"), ("Default", "key default")]);
    row(174, &[("CH1", "key ch1"), ("CH2", "key ch2"), ("CH3", "key ch3"), ("CH4", "key ch4")]);
    row(209, &[("Measure", "workflow measure"), ("Cursor", "key cursor"), ("Decode", "key analyse")]);
    row(244, &[("Math", "key math"), ("Refs", "key ref"), ("Trigger", "key tmenu")]);
    row(279, &[("LA", "key la"), ("Gen", "key gi"), ("Force", "key force"), ("Quick", "key quick")]);
    const KNOBS: &[(&str, &str, &str, &str)] = &[
        ("V scale", "knob vscale -1", "knob vscale +1", "knob vscale push"),
        ("V position", "knob vpos -5", "knob vpos +5", "knob vpos push"),
        ("H scale", "knob hscale -1", "knob hscale +1", "knob hscale push"),
        ("H position", "knob hpos -5", "knob hpos +5", "knob hpos push"),
        ("Trigger", "knob tlevel -5", "knob tlevel +5", "knob tlevel push"),
        ("Multi 1", "knob multi1 -1", "knob multi1 +1", "knob multi1 push"),
        ("Multi 2", "knob multi2 -1", "knob multi2 +1", "knob multi2 push"),
    ];
    for (i, &(_, minus, plus, push)) in KNOBS.iter().enumerate() {
        let y = 337 + i as i32 * 31;
        for (dx, width, label, command) in [(78, 31, "-", minus), (114, 31, "+", plus), (150, 58, "Push", push)] {
            out.push(Control { rect: Rect::new(x + dx, y, width, 27), label, command });
        }
    }
    out
}

pub fn draw(cv: &mut Canvas, ui: &Ui, hover: Option<(i32, i32)>) {
    cv.fill(Rect::new(W, 0, WIDTH, 600), BG);
    cv.vline(W, 0, 599, BORDER);
    let x = W + 12;
    cv.text(&ui.fonts().mb, x, 23, "FRONT PANEL", TEXT);
    for (y, title) in [(81, "ACQUISITION"), (169, "CHANNELS & TOOLS"), (330, "KNOBS")] {
        cv.text(&ui.fonts().sb, x, y, title, DIM);
    }
    for c in controls() {
        let on = (c.command == "key runstop" && ui.m.run == RunState::Run)
            || (c.command == "key single" && ui.m.trig_mode == TrigMode::Single && ui.m.run == RunState::Run);
        let border = if on { ACCENT } else if hover.is_some_and(|(x, y)| c.rect.contains(x, y)) { DIM } else { BORDER };
        cv.round(c.rect, 5, border, 255);
        cv.round(c.rect.inset(1), 4, BUTTON, 255);
        let label = if c.command == "key runstop" { if ui.m.run == RunState::Run { "Stop" } else { "Run" } } else { c.label };
        cv.text_in(&ui.fonts().sb, c.rect, 1, label, if on { ACCENT } else { TEXT });
    }
    for (i, label) in ["V scale", "V position", "H scale", "H position", "Trigger", "Multi 1", "Multi 2"].iter().enumerate() {
        cv.text(&ui.fonts().s, x, 355 + i as i32 * 31, label, TEXT);
    }
    let status = ui.lan_web_port.map(|port| format!("Web :{port}  |  Ctrl+O load")).unwrap_or_else(|| "Web unavailable  |  Ctrl+O load".into());
    cv.text(&ui.fonts().s, x, 581, &status, DIM);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_front_panel_control_has_a_unique_visible_target() {
        let items = controls();
        assert!(items.iter().any(|c| c.command == "key single"));
        assert!(items.iter().any(|c| c.command == "key tmenu"));
        assert!(items.iter().any(|c| c.command == "workflow load"));
        assert!(items.iter().any(|c| c.command == "knob multi2 push"));
        for (i, c) in items.iter().enumerate() {
            assert!(c.rect.x > W && c.rect.x + c.rect.w <= W + WIDTH && c.rect.y > 0 && c.rect.y + c.rect.h <= 600);
            for other in items.iter().skip(i + 1) {
                assert!(c.rect.x + c.rect.w <= other.rect.x || other.rect.x + other.rect.w <= c.rect.x || c.rect.y + c.rect.h <= other.rect.y || other.rect.y + other.rect.h <= c.rect.y, "{} overlaps {}", c.label, other.label);
            }
        }
    }
}
