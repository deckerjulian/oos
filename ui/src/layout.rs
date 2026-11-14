// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Screen layout: the size of the screen, the sizes of its frame that stay put, and cuts of rectangles.
//!
//! The layout is worked out from the screen size whenever it is needed: a few dozen rectangles, a few microseconds,
//! nothing compared with filling the pixels. It never draws. A rectangle is cut from a bigger one (`cut_top`,
//! `cut_left`, ...), so a part takes the room that is left instead of a fixed position on the 1024 x 600 panel.
//!
//! Code that still places things for the panel uses `PANEL_W` / `PANEL_H`. The lint test `layout_panel_budget` counts
//! those uses per file and fails when a file gets more of them (ui/README.md, "Layout for other screens").

use crate::gfx::Rect;

/// The built-in panel of the DHO800/DHO900.
pub const PANEL_W: i32 = 1024;
pub const PANEL_H: i32 = 600;

/// The size of the screen the GUI is laid out for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Screen {
    pub w: i32,
    pub h: i32,
}

impl Screen {
    pub const PANEL: Screen = Screen { w: PANEL_W, h: PANEL_H };

    pub fn rect(self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    /// The picture drawn for a monitor of `w` x `h` (Display > HDMI = Layout): its own size up to Full HD, a bigger
    /// monitor gets a picture of its shape within 1920 x 1080 and scales it up (the render time grows with the
    /// pixels). None: smaller than the panel, the monitor mirrors the panel instead.
    pub fn for_monitor(w: i32, h: i32) -> Option<Screen> {
        let f = (1920.0 / w as f32).min(1080.0 / h as f32).min(1.0);
        let s = Screen { w: (w as f32 * f).round() as i32, h: (h as f32 * f).round() as i32 };
        (s.w >= PANEL_W && s.h >= PANEL_H).then_some(s)
    }

    /// A screen the layout is made for: at least the panel size, at most 8K.
    pub fn of_size(w: i32, h: i32) -> Option<Screen> {
        ((PANEL_W..=7680).contains(&w) && (PANEL_H..=4320).contains(&h)).then_some(Screen { w, h })
    }

    /// "1920x1080", as `of_size`.
    pub fn parse(s: &str) -> Option<Screen> {
        let (w, h) = s.split_once(['x', 'X'])?;
        Screen::of_size(w.trim().parse().ok()?, h.trim().parse().ok()?)
    }
}

/// Sizes of the frame that are the same at every text size and on every screen: they are touch targets.
pub struct Metrics {
    /// top bar
    pub top_h: i32,
    /// bottom bar with the Menu button and the cards
    pub bottom_h: i32,
    /// room at the left and right edge of the screen
    pub edge: i32,
    /// between two cards
    pub gap: i32,
    /// above and below the cards in the bottom bar
    pub card_pad: i32,
    /// the Menu button left of the cards
    pub menu_btn_w: i32,
}

pub const METRICS: Metrics = Metrics { top_h: 40, bottom_h: 74, edge: 6, gap: 7, card_pad: 4, menu_btn_w: 62 };

/// The frame of the screen: top bar, waveform area, bottom bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    pub top: Rect,
    /// between the bars: the plot, the lanes, the second pane and the measurement rows
    pub body: Rect,
    pub bottom: Rect,
    /// the start menu button at the left of the bottom bar
    pub menu_btn: Rect,
    /// the row of the cards right of it (channels, LA, generator, time, trigger)
    pub cards: Rect,
}

impl Frame {
    pub fn of(s: Screen) -> Frame {
        let m = &METRICS;
        let mut body = s.rect();
        let top = body.cut_top(m.top_h);
        let bottom = body.cut_bottom(m.bottom_h);
        let mut cards = bottom.inset_xy(m.edge, m.card_pad);
        let menu_btn = cards.cut_left(m.menu_btn_w);
        cards.cut_left(m.gap);
        Frame { top, body, bottom, menu_btn, cards }
    }
}

/// Cuts: each takes a strip off one side of the rectangle and returns it; the rectangle keeps the rest. A strip never
/// is larger than what there is.
impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn cut_top(&mut self, h: i32) -> Rect {
        let h = h.clamp(0, self.h);
        let r = Rect::new(self.x, self.y, self.w, h);
        self.y += h;
        self.h -= h;
        r
    }

    pub fn cut_bottom(&mut self, h: i32) -> Rect {
        let h = h.clamp(0, self.h);
        self.h -= h;
        Rect::new(self.x, self.y + self.h, self.w, h)
    }

    pub fn cut_left(&mut self, w: i32) -> Rect {
        let w = w.clamp(0, self.w);
        let r = Rect::new(self.x, self.y, w, self.h);
        self.x += w;
        self.w -= w;
        r
    }

    /// Smaller by `dx` at the left and right, by `dy` at the top and bottom.
    pub fn inset_xy(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w - 2 * dx, self.h - 2 * dy)
    }

    /// A `w` x `h` rectangle in the middle of this one.
    pub fn center(&self, w: i32, h: i32) -> Rect {
        Rect::new(self.x + self.w / 2 - w / 2, self.y + self.h / 2 - h / 2, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_frame_is_the_one_the_panel_always_had() {
        let f = Frame::of(Screen::PANEL);
        assert_eq!(f.top, Rect::new(0, 0, 1024, 40));
        assert_eq!(f.body, Rect::new(0, 40, 1024, 486));
        assert_eq!(f.bottom, Rect::new(0, 526, 1024, 74));
        assert_eq!(f.menu_btn, Rect::new(6, 530, 62, 66));
        // the cards start 7 px after the Menu button and end 6 px before the right edge
        assert_eq!(f.cards, Rect::new(75, 530, 943, 66));
    }

    #[test]
    fn a_bigger_screen_keeps_the_bars_and_gives_the_rest_to_the_body() {
        let f = Frame::of(Screen::parse("1920x1080").unwrap());
        assert_eq!((f.top.h, f.bottom.h, f.menu_btn.w), (40, 74, 62));
        assert_eq!(f.body, Rect::new(0, 40, 1920, 1080 - 40 - 74));
        assert_eq!((f.bottom.y, f.cards.right()), (1080 - 74, 1920 - 6));
    }

    #[test]
    fn cuts_take_no_more_than_there_is() {
        let mut r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.cut_top(15), Rect::new(10, 20, 100, 15));
        assert_eq!(r.cut_bottom(5), Rect::new(10, 65, 100, 5));
        assert_eq!(r, Rect::new(10, 35, 100, 30));
        assert_eq!(r.cut_left(30), Rect::new(10, 35, 30, 30));
        assert_eq!(r.cut_top(99), Rect::new(40, 35, 70, 30));
        assert_eq!(r.h, 0);
        assert_eq!(r.cut_left(-3).w, 0);
        assert_eq!(Rect::new(0, 0, 100, 60).center(40, 20), Rect::new(30, 20, 40, 20));
        assert_eq!(Rect::new(0, 0, 100, 60).inset_xy(6, 4), Rect::new(6, 4, 88, 52));
    }

    /// Files that still place things for the 1024 x 600 panel, with the number of `PANEL_W` / `PANEL_H` in their code
    /// (tests do not count). A file may only get fewer: then lower its number here, to 0 drop it.
    const PANEL_BUDGET: [(&str, usize); 2] = [
        // the re-export for main.rs, and the fork's fixed panel frame `W` / `H` (with TOP, BOTTOM, PLOT_Y, ...) that
        // its panel-only screens still place things from
        ("ui.rs", 4),
        // the --size message and the panel-sized power-on picture
        ("main.rs", 6),
    ];

    /// The code outside `#[cfg(test)]` and comment lines (as in `board::boundary`).
    fn production(text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let end = (0..lines.len().saturating_sub(1))
            .find(|&i| {
                let next = lines[i + 1].trim_start();
                lines[i].trim() == "#[cfg(test)]" && next.starts_with("mod ") && next.ends_with('{')
            })
            .unwrap_or(lines.len());
        lines[..end].iter().filter(|l| !l.trim_start().starts_with("//")).cloned().collect::<Vec<_>>().join("\n")
    }

    fn panel_uses(code: &str) -> usize {
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        ["PANEL_W", "PANEL_H"]
            .iter()
            .map(|n| {
                code.match_indices(n)
                    .filter(|&(i, _)| !code[..i].ends_with(word) && !code[i + n.len()..].starts_with(word))
                    .count()
            })
            .sum()
    }

    fn rs_files(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rs_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// The way to every screen size goes one file at a time; no file goes back to fixed panel positions.
    #[test]
    fn layout_panel_budget() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rs_files(&src, &mut files);
        let mut errs = Vec::new();
        for f in files {
            let name = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            // the definition, and a module that is test code as a whole
            if name == "layout.rs" || name == "ui/lint.rs" {
                continue;
            }
            let n = panel_uses(&production(&std::fs::read_to_string(&f).unwrap()));
            let budget = PANEL_BUDGET.iter().find(|b| b.0 == name).map_or(0, |b| b.1);
            if n > budget {
                errs.push(format!("{}: {} uses of PANEL_W/PANEL_H, at most {}: lay it out from Ui::scr / Ui::frame", name, n, budget));
            } else if n < budget {
                errs.push(format!("{}: {} uses of PANEL_W/PANEL_H, fewer than {}: lower its PANEL_BUDGET", name, n, budget));
            }
        }
        assert!(errs.is_empty(), "{}", errs.join("\n"));
        assert_eq!(panel_uses("PANEL_W PANEL_WX X_PANEL_H PANEL_H"), 2);
    }

    #[test]
    fn a_monitor_gets_a_picture_of_its_shape_up_to_full_hd() {
        assert_eq!(Screen::for_monitor(1920, 1080), Some(Screen { w: 1920, h: 1080 }));
        assert_eq!(Screen::for_monitor(1280, 1024), Some(Screen { w: 1280, h: 1024 }));
        assert_eq!(Screen::for_monitor(3840, 2160), Some(Screen { w: 1920, h: 1080 }));
        assert_eq!(Screen::for_monitor(2560, 1600), Some(Screen { w: 1728, h: 1080 }));
        assert_eq!(Screen::for_monitor(800, 600), None);
        assert_eq!(Screen::for_monitor(1024, 600), Some(Screen::PANEL));
    }

    #[test]
    fn screen_sizes_from_the_command_line() {
        assert_eq!(Screen::parse("1280x800"), Some(Screen { w: 1280, h: 800 }));
        assert_eq!(Screen::parse("1920X1080"), Some(Screen { w: 1920, h: 1080 }));
        for bad in ["", "1920", "800x480", "x", "99999x1080", "a x b"] {
            assert_eq!(Screen::parse(bad), None, "{}", bad);
        }
    }
}
