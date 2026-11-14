// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The visible native buffer is the only source of truth for mouse coordinates.

use super::panel;
use crate::ui;

#[derive(Clone, Copy)]
pub struct Geometry {
    pub panel_open: bool,
}

impl Geometry {
    pub fn width(self) -> i32 {
        ui::W + if self.panel_open { panel::WIDTH } else { 0 }
    }

    /// Inverse of minifb's AspectRatioStretch, excluding letterbox bars.
    pub fn pointer(self, size: (usize, usize), x: f32, y: f32) -> Option<(i32, i32)> {
        let (ww, wh) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let width = self.width() as f32;
        let scale = (ww / width).min(wh / ui::H as f32);
        let (ox, oy) = ((ww - width * scale) * 0.5, (wh - ui::H as f32 * scale) * 0.5);
        let (sx, sy) = ((x - ox) / scale, (y - oy) / scale);
        (sx >= 0.0 && sy >= 0.0 && sx < width && sy < ui::H as f32)
            .then_some((sx as i32, sy as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_canvas_has_no_invisible_sidebar_or_shifted_targets() {
        let compact = Geometry { panel_open: false };
        assert_eq!(compact.width(), ui::W);
        for point in [(0, 0), (511, 299), (1023, 599)] {
            assert_eq!(compact.pointer((1024, 600), point.0 as f32, point.1 as f32), Some(point));
            assert_eq!(compact.pointer((2048, 1200), point.0 as f32 * 2.0, point.1 as f32 * 2.0), Some(point));
        }
        assert_eq!(compact.pointer((1024, 600), 1024.0, 300.0), None);
        assert_eq!(compact.pointer((1024, 600), -1.0, 300.0), None);
    }

    #[test]
    fn expanded_panel_and_letterbox_mapping_match_the_rendered_buffer() {
        let expanded = Geometry { panel_open: true };
        assert_eq!(expanded.width(), 1256);
        let button = panel::controls().into_iter().find(|c| c.command == "key single").unwrap().rect;
        let point = (button.x + button.w / 2, button.y + button.h / 2);
        assert_eq!(expanded.pointer((1256, 600), point.0 as f32, point.1 as f32), Some(point));
        // At half-size in a taller window, only the actual centred image is clickable.
        let y_offset = 100.0;
        assert_eq!(expanded.pointer((628, 500), point.0 as f32 * 0.5, point.1 as f32 * 0.5 + y_offset), Some(point));
        assert_eq!(expanded.pointer((628, 500), 300.0, y_offset - 1.0), None);
        assert_eq!(expanded.pointer((628, 500), 300.0, y_offset + 300.0), None);
        assert_eq!(expanded.pointer((1256, 600), 1023.5, 300.0), Some((1023, 300)));
        assert_eq!(expanded.pointer((1256, 600), 1024.0, 300.0), Some((1024, 300)));
    }
}
