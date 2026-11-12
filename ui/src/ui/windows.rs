// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Waveform windows: the plot split into two or four windows of the same size, each channel and math channel in one
//! of them, all on the same time base; and the plot over the whole screen. The layout itself is in `Ui::split_wins`,
//! the drawing goes window by window in `Ui::render`.

use super::*;

const CH_NAMES: [&str; 4] = ["CH1", "CH2", "CH3", "CH4"];
const MATH_NAMES: [&str; 8] = ["M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8"];

impl Ui {
    /// The Windows page: the layout, full screen, and while the plot is split the window of every channel and math
    /// channel that is on.
    pub(super) fn win_items(&self) -> Vec<Item> {
        let mut v = vec![Item::WinLayout, Item::FullScreen];
        if self.m.win_layout != WinLayout::One {
            v.extend((0..n_ch()).filter(|&c| self.m.ch[c].on).map(Item::WinCh));
            v.extend((0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).map(Item::WinMath));
            if self.m.la_on() {
                v.push(Item::WinLa);
            }
        }
        v
    }

    pub(super) fn win_kind(it: Item) -> Option<Kind> {
        match it {
            Item::WinLayout | Item::FullScreen | Item::WinCh(_) | Item::WinMath(_) | Item::WinLa => Some(Kind::Choice),
            _ => None,
        }
    }

    pub(super) fn win_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Windows) => "Windows",
            Item::WinLayout => "Layout",
            Item::FullScreen => "Full screen",
            Item::WinCh(c) => CH_NAMES.get(c).copied().unwrap_or("CH"),
            Item::WinMath(k) => MATH_NAMES.get(k).copied().unwrap_or("M"),
            Item::WinLa => "LA",
            _ => return None,
        })
    }

    pub(super) fn win_value(&self, it: Item) -> Option<String> {
        Some(match it {
            Item::WinLayout => self.m.win_layout.label().to_string(),
            Item::FullScreen => if self.full { "On" } else { "Off" }.to_string(),
            Item::WinCh(c) => (self.m.ch_win[c].min(self.m.win_layout.count() - 1) + 1).to_string(),
            Item::WinMath(k) => (self.m.math_win[k].min(self.m.win_layout.count() - 1) + 1).to_string(),
            Item::WinLa => (self.m.la_win.min(self.m.win_layout.count() - 1) + 1).to_string(),
            _ => return None,
        })
    }

    pub(super) fn win_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::WinLayout => WinLayout::ALL.iter().map(|l| l.label().to_string()).collect(),
            Item::FullScreen => vec!["Off".into(), "On".into()],
            Item::WinCh(_) | Item::WinMath(_) | Item::WinLa => (1..=self.m.win_layout.count()).map(|i| i.to_string()).collect(),
            _ => return None,
        })
    }

    pub(super) fn win_adjust(&mut self, it: Item, d: i32) -> bool {
        let n = self.m.win_layout.count();
        let cyc = |i: usize, n: usize| (i as i32 + d).rem_euclid(n as i32) as usize;
        match it {
            Item::WinLayout => {
                let all = WinLayout::ALL;
                let i = all.iter().position(|&l| l == self.m.win_layout).unwrap_or(0);
                self.m.win_layout = all[cyc(i, all.len())];
                // a new split shows something in every window at once; the choice per channel is on this page
                self.spread_wins();
                if self.m.zoom && self.m.win_layout != WinLayout::One {
                    self.toast("Windows show while zoom is off");
                }
            }
            Item::FullScreen => self.full = !self.full,
            Item::WinCh(c) => self.m.ch_win[c] = cyc(self.m.ch_win[c].min(n - 1), n),
            Item::WinMath(k) => self.m.math_win[k] = cyc(self.m.math_win[k].min(n - 1), n),
            Item::WinLa => self.m.la_win = cyc(self.m.la_win.min(n - 1), n),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn ui() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        for c in 0..n_ch() {
            u.m.ch[c].on = true;
        }
        u
    }

    fn render(u: &mut Ui) -> Canvas {
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        u.render(&mut cv);
        cv
    }

    #[test]
    fn layouts_split_the_plot_into_equal_windows() {
        let mut u = ui();
        for l in WinLayout::ALL {
            u.m.win_layout = l;
            render(&mut u);
            assert_eq!(u.wins.len(), l.count(), "{:?}", l);
            let g = u.grid;
            for (i, a) in u.wins.iter().enumerate() {
                assert_eq!((a.w, a.h), (u.wins[0].w, u.wins[0].h), "{:?}", l);
                assert!(a.x >= g.x && a.y >= g.y && a.x + a.w <= g.x + g.w && a.y + a.h <= g.y + g.h, "{:?} {:?}", l, a);
                assert_eq!(a.h % 8, 0, "{:?}", l);
                for b in &u.wins[i + 1..] {
                    assert!(!overlaps(*a, *b), "{:?}", l);
                }
            }
            // one phosphor map per channel at the size of a window
            assert_eq!((u.ph.w, u.ph.h), (u.wins[0].w as usize, u.wins[0].h as usize));
        }
    }

    #[test]
    fn a_new_layout_spreads_the_channels_and_each_draws_in_its_window() {
        let mut u = ui();
        u.activate(Item::WinLayout); // side by side
        assert_eq!(u.m.win_layout, WinLayout::Cols);
        assert_eq!(&u.m.ch_win[..n_ch()], &[0, 1, 0, 1][..n_ch()]);
        render(&mut u);
        // a channel's volts map into its own window
        let (a, b) = (u.wins[0], u.wins[1]);
        assert_eq!(u.ch_area(0), a);
        assert_eq!(u.ch_area(1), b);
        let y = u.y_of_volts(1, 0.0);
        assert!(y >= b.y && y < b.y + b.h);
        // moved by the page: the value reads back
        u.adjust(Item::WinCh(1), 1);
        assert_eq!(u.value(Item::WinCh(1)), "1");
        assert_eq!(u.ch_area(1), a);
        // back to one window: everything in it, whatever the per-channel choice says
        u.m.win_layout = WinLayout::One;
        render(&mut u);
        assert_eq!(u.wins.len(), 1);
        assert!((0..n_ch()).all(|c| u.ch_area(c) == u.area));
    }

    #[test]
    fn zoom_and_views_keep_one_window() {
        let mut u = ui();
        u.m.win_layout = WinLayout::Quad;
        u.m.zoom = true;
        render(&mut u);
        assert_eq!(u.wins.len(), 1);
        u.m.zoom = false;
        render(&mut u);
        assert_eq!(u.wins.len(), 4);
    }

    #[test]
    fn page_lists_the_signals_only_while_split() {
        let mut u = ui();
        assert_eq!(u.win_items(), vec![Item::WinLayout, Item::FullScreen]);
        u.m.win_layout = WinLayout::Rows;
        let v = u.win_items();
        assert!(v.contains(&Item::WinCh(0)) && !v.iter().any(|i| matches!(i, Item::WinMath(_))));
        for it in [Item::WinLayout, Item::WinCh(0), Item::FullScreen] {
            let opts = u.options(it);
            for o in opts.iter().rev().chain(opts.iter()) {
                u.select_option(it, o);
                assert_eq!(&u.value(it), o, "{:?}", it);
            }
        }
    }

    #[test]
    fn full_screen_button_enlarges_and_goes_back() {
        let mut u = ui();
        render(&mut u);
        let normal = u.grid;
        let (_, hit) = u.full_btn();
        u.tap(hit.x + hit.w / 2, hit.y + hit.h / 2);
        assert!(u.full);
        render(&mut u);
        assert!(u.grid.y < normal.y && u.grid.h > normal.h, "{:?} {:?}", u.grid, normal);
        // the cards are gone: a tap where CH1's card was opens nothing
        let (chs, _, _) = u.cards();
        u.tap(chs[0].x + 5, chs[0].y + 5);
        assert!(!u.menu_open);
        let (_, hit) = u.full_btn();
        u.tap(hit.x + hit.w / 2, hit.y + hit.h / 2);
        assert!(!u.full);
        render(&mut u);
        assert_eq!(u.grid, normal);
        // the touch lock needs the Menu button: no full screen while it is on
        u.full = true;
        u.m.touch_lock = true;
        render(&mut u);
        assert_eq!(u.grid, normal);
    }

    #[test]
    fn a_drag_in_the_second_window_moves_a_channel_of_that_window() {
        let mut u = ui();
        u.m.win_layout = WinLayout::Cols;
        u.spread_wins();
        u.m.active = 0;
        render(&mut u);
        let b = u.wins[1];
        // CH2's marker at the left edge of the right window
        let y = u.y_of_volts(1, 0.0);
        let x = b.x + 10;
        let before = u.m.ch[1].offset;
        u.touch_ev(Phase::Down, x, y);
        u.touch_ev(Phase::Move, x, y - 40);
        u.touch_ev(Phase::Move, x, y - 60);
        u.touch_ev(Phase::Up, x, y - 60);
        assert!(u.m.ch[1].offset > before);
        assert_eq!(u.m.active, 1);
    }

    #[test]
    fn setup_keeps_the_windows() {
        let mut u = ui();
        u.m.win_layout = WinLayout::Quad;
        u.m.ch_win = std::array::from_fn(|c| (3 - c) % 4);
        u.m.math_win[2] = 3;
        let t = crate::store::setup_text(&u.m);
        let mut back = Model::default();
        for l in t.lines().filter(|l| !l.starts_with('#')) {
            let (k, v) = l.split_once(" = ").unwrap();
            crate::store::set(&mut back, k, v).unwrap();
        }
        assert_eq!((back.win_layout, back.ch_win, back.math_win), (u.m.win_layout, u.m.ch_win, u.m.math_win));
    }

    #[test]
    fn auto_set_up_places_the_channels_per_window() {
        use crate::wave::{place, Stats};
        let mut m = Model::default();
        m.win_layout = WinLayout::Rows;
        m.ch_win = [0, 1, 1, 0];
        let s = Stats { vmin: -1.0, vmax: 1.0, ..Stats::default() };
        place(&mut m, &[(0, s, 1e-3), (1, s, 1e-3), (2, s, 1e-3)]);
        // CH1 alone in window 1: its 2 Vpp fill about 80 % of 8 div, centred
        assert!(2.0 / m.ch[0].volts_per_div() >= 3.9 && m.ch[0].offset.abs() < 1e-6, "{} {}", m.ch[0].volts_per_div(), m.ch[0].offset);
        // CH2 and CH3 share window 2: half the height each, one above the other
        assert!(2.0 / m.ch[1].volts_per_div() <= 3.2 && m.ch[1].offset > 0.0 && m.ch[2].offset < 0.0, "{} {} {}", m.ch[1].volts_per_div(), m.ch[1].offset, m.ch[2].offset);
        // one window: three bands
        m.win_layout = WinLayout::One;
        place(&mut m, &[(0, s, 1e-3), (1, s, 1e-3), (2, s, 1e-3)]);
        assert!(2.0 / m.ch[0].volts_per_div() <= 2.2 && m.ch[0].offset > 0.0);
    }

    #[test]
    fn the_la_lanes_stand_in_their_window() {
        let mut u = ui();
        u.m.la.on = true;
        if !u.m.la_on() {
            return; // a build for a board without LA
        }
        u.m.win_layout = WinLayout::Quad;
        u.spread_wins();
        // four channels and the LA: the LA goes round to window 1, which it shares with CH1
        assert_eq!(u.m.la_win, 0);
        render(&mut u);
        let (band, pitch) = u.la_band(u.m.la.lines().len());
        let w = u.wins[0];
        // at the bottom of the window, half of it or what 16 lanes of the smallest pitch need
        let n = u.m.la.lines().len() as i32;
        assert!(band.y >= w.y && band.y + band.h == w.y + w.h - 22 && band.x == w.x && band.w == w.w, "{:?} {:?}", band, w);
        assert!(band.h <= (w.h / 2).max(n * 9 + 18), "{:?} {:?}", band, w);
        assert!(pitch >= 9);
        // the plot keeps its height: no strip under it
        assert_eq!(u.la_strip_h(), 0);
        // alone in a window the lanes take all of it
        u.m.ch[3].on = false;
        u.m.la_win = 3;
        render(&mut u);
        let (band, _) = u.la_band(u.m.la.lines().len());
        let w = u.wins[3];
        assert!(band.h > w.h * 3 / 4 && band.y >= w.y, "{:?} {:?}", band, w);
    }

    /// Pictures of the layouts for a look by eye: OOS_SHOT_DIR=dir cargo test windows -- --ignored
    #[test]
    #[ignore]
    fn shots() {
        let Ok(d) = std::env::var("OOS_SHOT_DIR") else { return };
        let mut u = ui();
        for _ in 0..30 {
            u.step(0.05);
        }
        for (name, l, full) in [("win-one", WinLayout::One, false), ("win-cols", WinLayout::Cols, false), ("win-rows", WinLayout::Rows, false), ("win-quad", WinLayout::Quad, true), ("win-full", WinLayout::One, true)] {
            u.m.win_layout = l;
            u.spread_wins();
            u.full = full;
            for _ in 0..10 {
                u.step(0.05);
            }
            let cv = render(&mut u);
            std::fs::write(format!("{}/{}.png", d, name), crate::store::png_of(&cv)).unwrap();
        }
        // the LA in a window of its own
        u.m.la.on = true;
        u.m.ch[3].on = false;
        u.m.win_layout = WinLayout::Quad;
        u.spread_wins();
        u.full = false;
        for _ in 0..10 {
            u.step(0.05);
        }
        let cv = render(&mut u);
        std::fs::write(format!("{}/win-la.png", d), crate::store::png_of(&cv)).unwrap();
        u.m.la.on = false;
        u.m.ch[3].on = true;
        u.m.win_layout = WinLayout::Cols;
        u.spread_wins();
        u.open_menu(MenuId::Windows);
        let cv = render(&mut u);
        std::fs::write(format!("{}/win-menu.png", d), crate::store::png_of(&cv)).unwrap();
    }
}
