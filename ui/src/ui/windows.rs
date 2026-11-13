// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! View densities on the Windows row: Scope, Full, Split and Four, shared by the 1024×600 panel and a scaled window
//! of it. Split is a number pane under the plot, or two stacked traces. Four is the existing four-up. The expand
//! button moves between Scope and Full. The external display's existing choice sits on this row: mirror copies the
//! panel, waveform puts the plot on the display and leaves the panel for controls. The same four densities apply to
//! that plot. The layout itself is in `Ui::split_wins`, the drawing goes window by window in `Ui::render`.

use super::*;

const CH_NAMES: [&str; 4] = ["CH1", "CH2", "CH3", "CH4"];
const MATH_NAMES: [&str; 8] = ["M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8"];

impl Ui {
    /// The Windows row: the density, the external display, and while Split or Four has more than one window the
    /// window of every channel and math channel that is on. Split also chooses what is under the plot.
    pub(super) fn win_items(&self) -> Vec<Item> {
        let mut v = vec![Item::WinLayout, Item::Hdmi];
        if self.m.density == Density::Split {
            v.push(Item::PaneSel);
        }
        if self.m.win_layout != WinLayout::One {
            v.extend((0..n_ch()).filter(|&c| self.m.ch[c].on).map(Item::WinCh));
            v.extend((0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).map(Item::WinMath));
            if self.m.la_on() {
                v.push(Item::WinLa);
            }
        }
        v
    }

    /// Expand button and the Full density: Scope goes to Full, Full goes back to Scope, Split and Four go to Full.
    pub(super) fn toggle_expand(&mut self) {
        self.m.density = if self.m.density == Density::Full { Density::Scope } else { Density::Full };
        self.apply_density();
    }

    /// Makes the windows, the lower pane and the panel bars follow the density.
    /// Waveform on the external display keeps the bars: the density changes the plot, the panel stays the controls.
    pub(super) fn apply_density(&mut self) {
        let prev = self.m.win_layout;
        match self.m.density {
            Density::Scope | Density::Full => {
                if self.m.pane != 0 {
                    self.m.split_pane = self.m.pane;
                }
                self.m.pane = 0;
                self.m.win_layout = WinLayout::One;
            }
            Density::Four => {
                if self.m.pane != 0 {
                    self.m.split_pane = self.m.pane;
                }
                self.m.pane = 0;
                self.m.win_layout = WinLayout::Quad;
            }
            Density::Split => {
                self.m.pane = self.m.split_pane;
                self.m.win_layout = if self.m.pane == 0 { WinLayout::Rows } else { WinLayout::One };
            }
        }
        self.sync_full();
        if self.m.win_layout != prev && self.m.win_layout != WinLayout::One {
            self.spread_wins();
            if self.m.zoom {
                self.toast("Windows show while zoom is off");
            }
        }
    }

    /// Panel bars hide only for Full while the external display copies the panel.
    pub(crate) fn sync_full(&mut self) {
        self.full = self.m.density == Density::Full && !self.m.hdmi_wave;
    }

    pub(super) fn win_kind(it: Item) -> Option<Kind> {
        match it {
            Item::WinLayout | Item::WinCh(_) | Item::WinMath(_) | Item::WinLa => Some(Kind::Choice),
            _ => None,
        }
    }

    pub(super) fn win_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Windows) => "Windows",
            Item::WinLayout => "Density",
            Item::WinCh(c) => CH_NAMES.get(c).copied().unwrap_or("CH"),
            Item::WinMath(k) => MATH_NAMES.get(k).copied().unwrap_or("M"),
            Item::WinLa => "LA",
            _ => return None,
        })
    }

    pub(super) fn win_value(&self, it: Item) -> Option<String> {
        Some(match it {
            Item::WinLayout => self.m.density.label().to_string(),
            Item::WinCh(c) => (self.m.ch_win[c].min(self.m.win_layout.count() - 1) + 1).to_string(),
            Item::WinMath(k) => (self.m.math_win[k].min(self.m.win_layout.count() - 1) + 1).to_string(),
            Item::WinLa => (self.m.la_win.min(self.m.win_layout.count() - 1) + 1).to_string(),
            _ => return None,
        })
    }

    pub(super) fn win_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::WinLayout => Density::ALL.iter().map(|d| d.label().to_string()).collect(),
            Item::WinCh(_) | Item::WinMath(_) | Item::WinLa => (1..=self.m.win_layout.count()).map(|i| i.to_string()).collect(),
            _ => return None,
        })
    }

    pub(super) fn win_adjust(&mut self, it: Item, d: i32) -> bool {
        let n = self.m.win_layout.count();
        let cyc = |i: usize, n: usize| (i as i32 + d).rem_euclid(n as i32) as usize;
        match it {
            Item::WinLayout => {
                let all = Density::ALL;
                let i = all.iter().position(|&l| l == self.m.density).unwrap_or(0);
                self.m.density = all[cyc(i, all.len())];
                self.apply_density();
            }
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
        let mut cv = Canvas::new(W, H);
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
        u.select_option(Item::WinLayout, "Split");
        assert_eq!(u.m.density, Density::Split);
        assert_eq!(u.m.win_layout, WinLayout::Rows);
        assert_eq!(&u.m.ch_win[..n_ch()], &(0..n_ch()).map(|c| c % 2).collect::<Vec<_>>()[..]);
        render(&mut u);
        // a channel's volts map into its own window; Split stacks them
        let (a, b) = (u.wins[0], u.wins[1]);
        assert!(b.y >= a.y + a.h);
        assert_eq!(u.ch_area(0), a);
        assert_eq!(u.ch_area(1), b);
        let y = u.y_of_volts(1, 0.0);
        assert!(y >= b.y && y < b.y + b.h);
        // moved by the page: the value reads back
        u.adjust(Item::WinCh(1), 1);
        assert_eq!(u.value(Item::WinCh(1)), "1");
        assert_eq!(u.ch_area(1), a);
        // back to one window: everything in it, whatever the per-channel choice says
        u.select_option(Item::WinLayout, "Scope");
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
        assert_eq!(u.win_items(), vec![Item::WinLayout, Item::Hdmi]);
        u.select_option(Item::WinLayout, "Split");
        let v = u.win_items();
        assert!(v.contains(&Item::PaneSel));
        assert!(v.contains(&Item::WinCh(0)) && !v.iter().any(|i| matches!(i, Item::WinMath(_))));
        for it in [Item::WinLayout, Item::WinCh(0), Item::Hdmi, Item::PaneSel] {
            let opts = u.options(it);
            for o in opts.iter().rev().chain(opts.iter()) {
                u.select_option(it, o);
                assert_eq!(&u.value(it), o, "{:?} {}", it, o);
            }
        }
    }

    #[test]
    fn full_screen_button_enlarges_and_goes_back() {
        let mut u = ui();
        // mirror: the panel is the picture, so Full gives it the bars
        u.select_option(Item::Hdmi, "Mirror");
        render(&mut u);
        let normal = u.grid;
        let (_, hit) = u.full_btn();
        u.tap(hit.x + hit.w / 2, hit.y + hit.h / 2);
        assert_eq!(u.m.density, Density::Full);
        assert!(u.full);
        render(&mut u);
        assert!(u.grid.y < normal.y && u.grid.h > normal.h, "{:?} {:?}", u.grid, normal);
        // the cards are gone: a tap where CH1's card was opens nothing
        let (chs, _, _) = u.cards();
        u.tap(chs[0].x + 5, chs[0].y + 5);
        assert!(!u.menu_open);
        let (_, hit) = u.full_btn();
        u.tap(hit.x + hit.w / 2, hit.y + hit.h / 2);
        assert_eq!(u.m.density, Density::Scope);
        assert!(!u.full);
        render(&mut u);
        assert_eq!(u.grid, normal);
        // the touch lock needs the Menu button: no full screen while it is on
        u.m.density = Density::Full;
        u.sync_full();
        u.m.touch_lock = true;
        render(&mut u);
        assert_eq!(u.grid, normal);
    }

    #[test]
    fn densities_are_scope_full_split_and_four() {
        let mut u = ui();
        u.select_option(Item::Hdmi, "Mirror");
        assert_eq!(u.options(Item::WinLayout), vec!["Scope", "Full", "Split", "Four"]);
        u.select_option(Item::WinLayout, "Full");
        render(&mut u);
        assert!(u.full && u.wins.len() == 1 && u.grid.y == 0);
        u.select_option(Item::WinLayout, "Split");
        render(&mut u);
        assert!(!u.full && u.wins.len() == 2 && u.wins[1].y > u.wins[0].y && u.wins[0].x == u.wins[1].x);
        assert_eq!(u.value(Item::PaneSel), "Traces");
        u.select_option(Item::PaneSel, "Results");
        render(&mut u);
        assert_eq!((u.wins.len(), u.m.win_layout, u.m.pane, u.m.split_pane, u.m.density), (1, WinLayout::One, tables::PANE_RESULTS, tables::PANE_RESULTS, Density::Split));
        assert!(u.pane_h() > 0);
        u.select_option(Item::PaneSel, "Spectrum");
        assert_eq!(u.m.pane, tables::PANE_SPECTRUM);
        u.select_option(Item::PaneSel, "Trend");
        assert_eq!(u.m.pane, tables::PANE_TREND);
        u.select_option(Item::WinLayout, "Four");
        render(&mut u);
        assert_eq!(u.wins.len(), 4);
        assert_eq!(u.pane_h(), 0);
        u.select_option(Item::WinLayout, "Scope");
        render(&mut u);
        assert_eq!((u.wins.len(), u.pane_h()), (1, 0));
        // Split comes back to the lower pane it had
        u.select_option(Item::WinLayout, "Split");
        render(&mut u);
        assert_eq!(u.value(Item::PaneSel), "Trend");
        assert_eq!(u.wins.len(), 1);
        assert!(u.pane_h() > 0);
    }

    #[test]
    fn waveform_keeps_the_panel_bars_and_the_densities_change_the_plot() {
        let mut u = ui();
        assert!(u.win_items().contains(&Item::Hdmi));
        assert_eq!(u.value(Item::Hdmi), "Waveform");
        u.m.meas = vec![(Meas::Vpp, WaveId::Ch(0))];
        u.stats_on = true;
        render(&mut u);
        let normal = u.grid;
        u.hdmi_snap_want = true;
        u.select_option(Item::WinLayout, "Full");
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        assert_eq!(u.m.density, Density::Full);
        assert!(!u.full && u.grid == normal, "the panel stays the controls");
        u.select_option(Item::Hdmi, "Mirror");
        u.render(&mut cv);
        assert!(u.full && u.grid.y == 0, "mirror shows Full on the panel");
        u.select_option(Item::Hdmi, "Waveform");
        u.render(&mut cv);
        assert!(!u.full && u.grid == normal, "waveform gives the panel its bars back");
        let (head, _) = u.results_rects().unwrap();
        let at = (head.y * W + head.x + 8) as usize;
        let snap_at = ((head.y - TOP.h) * W + head.x + 8) as usize;
        assert_eq!(u.hdmi_snap.len(), (W * (BOTTOM.y - TOP.h)) as usize);
        assert_ne!(u.hdmi_snap[snap_at], cv.px[at], "Full leaves the statistics card off the external plot");
        u.select_option(Item::WinLayout, "Scope");
        u.render(&mut cv);
        assert_eq!(u.hdmi_snap[snap_at], cv.px[at], "Scope copies the plot, strip and card");
        u.select_option(Item::WinLayout, "Four");
        u.render(&mut cv);
        assert_eq!(u.wins.len(), 4);
        assert!(u.wins.iter().all(|w| w.y >= TOP.h && w.y + w.h <= BOTTOM.y));
        assert!(!u.full);
        u.select_option(Item::WinLayout, "Split");
        u.select_option(Item::PaneSel, "Results");
        u.render(&mut cv);
        assert!(u.pane_h() > 0 && u.wins.len() == 1 && !u.full);
        let pane = u.pane_rect();
        assert!(pane.y >= TOP.h && pane.y + pane.h <= BOTTOM.y);
    }

    #[test]
    fn zoom_area_button_arms_one_drag() {
        use crate::model::{tscales, HDIV};
        let mut u = ui();
        render(&mut u);
        let (full, fhit) = u.full_btn();
        let (zb, zhit) = u.zoom_btn();
        // under the full-screen button, and not on top of its touch target
        assert_eq!((zb.x, zb.w, zb.h), (full.x, full.w, full.h));
        assert!(zb.y >= fhit.y + fhit.h, "zoom button {:?} full hit {:?}", zb, fhit);
        assert!(!overlaps(full, zb) && !overlaps(fhit, zhit));
        let full_at = u.full_btn();
        let (zx, zy) = (zhit.x + zhit.w / 2, zhit.y + zhit.h / 2);
        // the press is a tap on the button, not a drag on the plot
        u.touch_ev(Phase::Down, zx, zy);
        u.touch_ev(Phase::Up, zx, zy);
        assert!(u.zoom_arm && !u.m.zoom && !u.full);
        assert_eq!(u.full_btn(), full_at);
        // a second press puts the arm away, and a third puts it back
        u.touch_ev(Phase::Down, zx, zy);
        u.touch_ev(Phase::Up, zx, zy);
        assert!(!u.zoom_arm);
        u.touch_ev(Phase::Down, zx, zy);
        u.touch_ev(Phase::Up, zx, zy);
        assert!(u.zoom_arm);

        let a = u.area;
        let tdiv = u.m.time_per_div();
        let delay = u.m.delay;
        let off = u.m.ch[0].offset;
        // a tap is not the zoom, and a second tap is not either
        let (x, y) = (a.x + 80, a.y + a.h / 2);
        u.touch_ev(Phase::Down, x, y);
        u.touch_ev(Phase::Up, x, y);
        u.touch_ev(Phase::Down, x + 200, y + 40);
        u.touch_ev(Phase::Up, x + 200, y + 40);
        assert!(u.zoom_arm && !u.m.zoom);
        assert_eq!((u.m.delay, u.m.ch[0].offset), (delay, off));
        // a sideways drag that is not a box does not zoom
        u.touch_ev(Phase::Down, x, y);
        u.touch_ev(Phase::Move, x + 80, y + 2);
        u.touch_ev(Phase::Up, x + 80, y + 2);
        assert!(u.zoom_arm && !u.m.zoom);
        assert_eq!((u.m.delay, u.m.ch[0].offset), (delay, off));

        // one drag draws the box, and that box is the zoom window
        let (x0, y0) = (a.x + a.w / 4, a.y + a.h / 4);
        let (x1, y1) = (a.x + a.w / 2, a.y + 3 * a.h / 4);
        u.touch_ev(Phase::Down, x0, y0);
        u.touch_ev(Phase::Move, x1, y1);
        assert!(u.touch.drag == Drag::ZoomBox);
        u.touch_ev(Phase::Up, x1, y1);
        assert!(u.m.zoom && !u.zoom_arm);
        let ppd = a.w as f32 / HDIV;
        let at = |px: i32| delay + (px - (a.x + a.w / 2)) as f32 / ppd * tdiv;
        let (t0, t1) = (at(x0), at(x1));
        let span = t1 - t0;
        assert!((u.m.zdelay - (t0 + t1) * 0.5).abs() < tdiv * 1e-3, "{} vs {}", u.m.zdelay, (t0 + t1) * 0.5);
        let scales = tscales();
        let zw = scales[u.m.zscale] * HDIV;
        assert!(zw + span * 1e-4 >= span, "window {} span {}", zw, span);
        if u.m.zscale > 0 {
            assert!(scales[u.m.zscale - 1] * HDIV < span * 0.999);
        }
        assert!(u.m.zscale <= u.m.tscale);

        // the drag is gone: the next drag is the ordinary one, not another box
        let zscale = u.m.zscale;
        let zdelay = u.m.zdelay;
        let b = u.area;
        let (mx, my) = (b.x + b.w / 2, b.y + b.h / 2);
        u.touch_ev(Phase::Down, mx, my);
        u.touch_ev(Phase::Move, mx - 40, my);
        assert!(u.touch.drag == Drag::Delay);
        u.touch_ev(Phase::Up, mx - 40, my);
        assert_eq!(u.m.zscale, zscale);
        assert!(u.m.zdelay > zdelay);

        // press again, then a drag on the zoomed view tightens the window
        let (zx, zy) = (u.zoom_btn().1.x + u.zoom_btn().1.w / 2, u.zoom_btn().1.y + u.zoom_btn().1.h / 2);
        u.touch_ev(Phase::Down, zx, zy);
        u.touch_ev(Phase::Up, zx, zy);
        assert!(u.zoom_arm);
        let b = u.area;
        let (x0, y0) = (b.x + b.w / 4, b.y + 20);
        let (x1, y1) = (b.x + b.w / 2, b.y + b.h / 2);
        u.touch_ev(Phase::Down, x0, y0);
        u.touch_ev(Phase::Move, x1, y1);
        u.touch_ev(Phase::Up, x1, y1);
        assert!(u.m.zoom && !u.zoom_arm);
        assert!(u.m.zscale < zscale);

        // a view that replaces the waveform does not arm
        u.xy = true;
        render(&mut u);
        let (full2, _) = u.full_btn();
        let (zb2, zh2) = u.zoom_btn();
        assert_eq!(zb2.x, full2.x);
        assert!(zb2.y > full2.y);
        u.tap(zh2.x + zh2.w / 2, zh2.y + zh2.h / 2);
        assert!(!u.zoom_arm);
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
        u.m.density = Density::Four;
        u.m.win_layout = WinLayout::Quad;
        u.m.ch_win = std::array::from_fn(|c| (3 - c) % 4);
        u.m.math_win[2] = 3;
        u.m.split_pane = 2;
        let t = crate::store::setup_text(&u.m);
        let mut back = Model::default();
        for l in t.lines().filter(|l| !l.starts_with('#')) {
            let (k, v) = l.split_once(" = ").unwrap();
            crate::store::set(&mut back, k, v).unwrap();
        }
        assert_eq!((back.win_layout, back.density, back.split_pane, back.ch_win, back.math_win), (u.m.win_layout, u.m.density, u.m.split_pane, u.m.ch_win, u.m.math_win));
        // a setup written before the density row
        let old = crate::store::parse_setup("windows = Cols\npane = 0\n").unwrap();
        assert_eq!((old.density, old.win_layout), (Density::Split, WinLayout::Rows));
        let quad = crate::store::parse_setup("windows = Quad\n").unwrap();
        assert_eq!((quad.density, quad.win_layout), (Density::Four, WinLayout::Quad));
        let pane = crate::store::parse_setup("pane = 2\n").unwrap();
        assert_eq!((pane.density, pane.pane, pane.split_pane), (Density::Split, 2, 2));
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
        // the small corner control vs the same plot with the bars gone
        u.close_menu();
        u.m.win_layout = WinLayout::One;
        u.spread_wins();
        u.stats_on = true;
        u.full = false;
        for _ in 0..12 {
            u.step(0.05);
        }
        let cv = render(&mut u);
        std::fs::write(format!("{}/fullscreen-btn.png", d), crate::store::png_of(&cv)).unwrap();
        let (_, hit) = u.full_btn();
        u.tap(hit.x + hit.w / 2, hit.y + hit.h / 2);
        for _ in 0..8 {
            u.step(0.05);
        }
        let cv = render(&mut u);
        std::fs::write(format!("{}/fullscreen-expanded.png", d), crate::store::png_of(&cv)).unwrap();
    }
}
