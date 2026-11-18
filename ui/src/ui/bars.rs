// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The two bars around the plot: the top bar (run state, acquisition, menu buttons, network, USB) with its toast, and
//! the bottom row of cards (Menu, the channels, LA, generator, time base, trigger), plus the front-panel LEDs that follow
//! the same state. A new top bar entry needs a decision record first (ADR 0007); a new card goes into `Ui::card_layout`
//! and `Ui::draw_bottom_cards`.

use super::*;

/// What `Ui::draw_aux_card` shows: the card's name and colour, on and selected, its value `big` and the line under it.
struct AuxCard<'a> {
    name: &'a str,
    col: Rgb,
    on: bool,
    sel: bool,
    big: &'a str,
    sub: &'a str,
}

impl Ui {
    /// Run state pill at the left of the top bar (wider for the larger text sizes).
    pub(super) fn run_pill(&self) -> Rect {
        Rect::new(8, 6, 92.max(30 + self.fonts().mb.width("TRIG'D") + 12), 28)
    }

    /// Network box at the right of the top bar: room for an address, two lines of text (taller at the larger text
    /// sizes).
    pub(super) fn net_box(&self) -> Rect {
        let k = self.m.text_size.index() as i32;
        let w = 122.max(28 + self.fonts().sb.width("255.255.255.255"));
        let top = self.frame().top;
        Rect::new(top.right() - METRICS.edge - w, top.y + 5 - 2 * k, w, 30 + 4 * k)
    }

    /// Left of the network box while a WLAN stick is plugged in.
    pub(super) fn wlan_box(&self) -> Rect {
        let n = self.net_box();
        let w = if self.m.text_size == TextSize::Normal { 104 } else { n.w };
        Rect::new(n.x - 6 - w, n.y, w, n.h)
    }
    /// Start menu button left of the channel cards (all menus and key functions by touch).
    /// The fork's compact bottom bar on the panel; `menu_btn` follows the frame on other screens.
    pub(super) const MENU_BTN: Rect = Rect::new(4, H - BOTTOM_H + 3, 46, 30);
    pub(super) const SINGLE_BTN: Rect = Rect::new(104, 5, 54, 30);
    /// Run state pill at the panel's normal text size (`run_pill` grows with the text).
    #[cfg(test)]
    pub(super) const RUN_PILL: Rect = Rect::new(8, 6, 92, 28);
    pub(super) fn menu_btn(&self) -> Rect {
        self.frame().menu_btn
    }

    /// Single button right of the run state pill (moves over when the pill grows with the text size).
    pub(super) fn single_btn(&self) -> Rect {
        let p = self.run_pill();
        Rect::new(p.right() + 4, Self::SINGLE_BTN.y, Self::SINGLE_BTN.w, Self::SINGLE_BTN.h)
    }

    /// Top-bar buttons, right-aligned before the network box: what is used most, and the menus without a front-panel key
    /// (Display, Storage). Everything else is in the start menu: System first in its group, SIM under Analysis; Load file
    /// is a row of the Reference menu.
    pub(super) fn top_buttons(&self) -> Vec<(TopBtn, &'static str, Rect)> {
        let f = self.fonts();
        let mut all = vec![
            (TopBtn::Menu(MenuId::Measure), "Measure"),
            (TopBtn::Menu(MenuId::Cursor), "Cursor"),
            (TopBtn::Menu(MenuId::Display), "Display"),
            (TopBtn::Menu(MenuId::Storage), "Storage"),
        ];
        all.retain(|(b, _)| match b {
            TopBtn::Menu(m) => !self.menu_left_out(*m),
            _ => true,
        });
        if self.record_mode() {
            all.insert(0, (TopBtn::Live, "Live"));
        }
        let required = self.single_btn().right() + 8 + f.s.width(&self.acq_caption().0) + 12;
        let fits = |items: &[(TopBtn, &'static str)], padding: i32| {
            let width = items.iter().map(|(_, label)| (f.mb.width(label) + padding).max(40)).sum::<i32>()
                + items.len().saturating_sub(1) as i32 * 6;
            self.top_right() - 8 - width >= required
        };
        let mut padding = 28;
        if !fits(&all, padding) {
            padding = 20;
        }
        for (button, short) in [
            (TopBtn::Menu(MenuId::Storage), "Store"),
            (TopBtn::Menu(MenuId::Display), "Disp"),
            (TopBtn::Menu(MenuId::Measure), "Meas"),
        ] {
            if fits(&all, padding) {
                break;
            }
            if let Some((_, label)) = all.iter_mut().find(|(b, _)| *b == button) {
                *label = short;
            }
        }
        let mut x = self.top_right() - 8;
        let mut out = Vec::new();
        for &(b, l) in all.iter().rev() {
            let w = (f.mb.width(l) + padding).max(40);
            x -= w;
            out.push((b, l, Rect::new(x, 5, w, 30)));
            x -= 6;
        }
        out.reverse();
        out
    }

    pub(super) fn top_acq_caption(&self, width: i32) -> (String, bool) {
        let (caption, live) = self.acq_caption();
        if !live && self.fonts().s.width(&caption) > width {
            if let Some(r) = self.refs.get(self.active_reference()).and_then(Option::as_ref) {
                return (
                    format!(
                        "{} · {}",
                        if self.session_is_sim { "SIM" } else { "REF" },
                        si_digits(r.v.len() as f64, "pts", 3)
                    ),
                    false,
                );
            }
        }
        (caption, live)
    }

    /// While a Bluetooth stick is there: left of the network and WLAN boxes (a combo stick brings both).
    pub(super) fn bt_box(&self) -> Rect {
        let next = if self.wlan_stick { self.wlan_box().x } else { self.net_box().x };
        Rect::new(next - 52, 5, 46, 30)
    }

    /// While a USB stick is mounted: left of the network, WLAN and Bluetooth boxes; the top-bar buttons move over for
    /// it.
    pub(super) fn usb_box(&self) -> Rect {
        let next = match (self.bt_stick, self.wlan_stick) {
            (true, _) => self.bt_box().x,
            (false, true) => self.wlan_box().x,
            _ => self.net_box().x,
        };
        Rect::new(next - 52, 5, 46, 30)
    }

    /// Where the buttons of the top bar end: the leftmost of the boxes on the right.
    pub(super) fn top_right(&self) -> i32 {
        match (self.usb, self.bt_stick, self.wlan_stick) {
            (true, _, _) => self.usb_box().x,
            (false, true, _) => self.bt_box().x,
            (false, false, true) => self.wlan_box().x,
            _ => self.net_box().x,
        }
    }

    /// A small box of the top bar: a dot (green: in use) and a short name, the border in the accent while `open`.
    fn draw_chip(&self, cv: &mut Canvas, r: Rect, name: &str, used: bool, open: bool) {
        cv.round(r, 5, if open { ACCENT } else { BTN_BORDER }, 255);
        cv.round(r.inset(1), 4, BG, 255);
        cv.circle(r.x + 9, r.y + 15, 3, if used { GREEN } else { DIM });
        cv.text(&self.fonts().sb, r.x + 16, r.y + 20, name, TEXT);
    }

    /// A box of the top bar with a state dot and two lines.
    fn draw_link(&self, cv: &mut Canvas, r: Rect, l1: &str, addr: Option<&str>) {
        let f = self.fonts();
        cv.round(r, 5, if self.menu_open && self.menu_root() == MenuId::Network { ACCENT } else { BTN_BORDER }, 255);
        cv.round(r.inset(1), 4, BG, 255);
        // first line under the top edge, the second on the bottom edge
        let (cap, y1) = (f.s.size * 72 / 100, r.y + 5 + f.s.size * 72 / 100);
        cv.circle(r.x + 11, y1 - cap / 2 + 1, 3, if addr.is_some() { GREEN } else { DIM });
        cv.text(&f.s, r.x + 20, y1, l1, DIM);
        cv.text(&f.sb, r.x + 20, r.y + r.h - 4, addr.unwrap_or("not connected"), TEXT);
    }

    /// The address of an interface, if it has one.
    pub(super) fn addr_of(&self, name: &str) -> Option<&str> {
        self.net_ifs.iter().find(|i| i.0 == name).map(|i| i.1.as_str())
    }

    /// Touch area of a top-bar button: the full bar height.
    pub(super) fn top_hit(&self, r: Rect) -> Rect {
        let top = self.frame().top;
        Rect::new(r.x - 2, top.y, r.w + 4, top.h)
    }

    /// The acquisition mode as the top bar names it.
    fn acq_name(&self) -> String {
        match self.m.acq {
            AcqMode::Normal => "Normal".to_string(),
            AcqMode::Peak => "Peak".to_string(),
            AcqMode::Average if self.src.is_hw() && board().avg_hw(&self.m) => {
                format!("Average {} HW", self.m.avg_count)
            }
            AcqMode::Average => format!("Average {}", self.m.avg_count),
            AcqMode::HiRes => "High res".to_string(),
        }
    }

    /// The acquisition text of the top bar from `x` on, before the buttons at `room`: sample rate and depth in the bold
    /// font, else in the regular one, else without the spaces; the mode only when it fits too. Larger text with a long
    /// depth ran under the buttons. (bold, rate and depth, mode)
    pub(super) fn top_acq(
        f: &Fonts,
        x: i32,
        room: i32,
        sr: f64,
        depth: f64,
        mode: String,
    ) -> (bool, String, Option<String>) {
        let (rate, pts) = (si_digits(sr, "Sa/s", 3), si_digits(depth, "pts", 3));
        let full = format!("{} \u{b7} {}", rate, pts);
        let (bold, main) = if x + f.mb.width(&full) <= room {
            (true, full)
        } else if x + f.m.width(&full) <= room {
            (false, full)
        } else {
            // tighter, then the rate alone (large text with the view buttons, a USB stick, a WLAN stick)
            let tight = format!("{}\u{b7}{}", rate.replace(' ', ""), pts.replace(' ', ""));
            let alone = rate.replace(' ', "");
            let main = [tight, alone].into_iter().find(|t| x + f.m.width(t) <= room).unwrap_or_default();
            (false, main)
        };
        let end = x + if bold { f.mb.width(&main) } else { f.m.width(&main) };
        let mode = format!(" \u{b7} {}", mode);
        let mode = (end + f.m.width(&mode) <= room).then_some(mode);
        (bold, main, mode)
    }

    pub(super) fn draw_top(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let top = self.frame().top;
        cv.fill(top, BAR);
        cv.hline(top.x, top.right() - 1, top.bottom() - 1, LINE);
        // run / trigger state; offline mode reads the record (playing or held) instead of a trigger
        let (txt, col) = self.run_caption();
        let r = self.run_pill();
        cv.round(r, 6, mix(BAR, col, 80), 255);
        cv.round(r.inset(1), 5, mix(BAR, col, 36), 255);
        cv.circle(r.x + 16, r.y + 14, 4, col);
        cv.text_in(&f.mb, Rect::new(r.x + 30, r.y, r.w - 30, r.h), 0, txt, col);
        let single = self.single_btn();
        btn(cv, single, BTN, BTN_BORDER);
        cv.text_in(&f.s, single, 1, "Single", TEXT);
        // acquisition: sample rate and depth of the hardware when it runs, else of the model; mode.
        // Offline mode shows the reference's own rate and length, with no acquire mode.
        let buttons = self.top_buttons();
        let room = self.view_box().map_or(buttons.first().map_or(self.top_right(), |b| b.2.x), |v| v.x) - 12;
        let x0 = single.right() + 8;
        let (caption, live_acq) = self.top_acq_caption((room - x0).max(1));
        let acq_open =
            self.menu_open && self.menu_root() == if self.record_mode() { MenuId::Ref } else { MenuId::Acquire };
        let tcol = if acq_open { ACCENT } else { TEXT };
        cv.set_clip(Rect::new(x0, top.y, (room - x0).max(1), top.h));
        if live_acq {
            let (sr, depth) =
                self.src.hw_rate_depth().map(|(r, n)| (r, n as _)).unwrap_or_else(|| self.m.srate_depth());
            let (bold, main, mode) = Self::top_acq(f, x0, room, sr, depth as f64, self.acq_name());
            let x = cv.text(if bold { &f.mb } else { &f.m }, x0, 25, &main, tcol);
            if let Some(m) = mode {
                cv.text(&f.m, x, 25, &m, tcol);
            }
        } else {
            cv.text(&f.s, x0, 25, &caption, tcol);
        }
        cv.no_clip();
        self.draw_view_box(cv);
        // buttons (the open menu in orange)
        let root = self.menu_root();
        for (b, l, r) in buttons {
            let hl = match b {
                TopBtn::Live => true,
                TopBtn::Menu(m) => self.menu_open && root == m,
            };
            btn(cv, r, BTN, if hl { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.mb, r, 1, l, if hl { ACCENT } else { TEXT });
        }
        // LAN: an address without a cable is only the fallback waiting for one
        let lan = self.addr_of("eth0").filter(|_| self.eth_link);
        self.draw_link(
            cv,
            self.net_box(),
            if lan.is_none() {
                "LAN"
            } else if self.net_fixed {
                "LAN \u{b7} local"
            } else {
                "LAN \u{b7} DHCP"
            },
            lan,
        );
        // WLAN, Bluetooth and USB sticks, while they are there; Bluetooth's dot is green while a device is connected
        if self.wlan_stick {
            self.draw_link(cv, self.wlan_box(), "WLAN", self.addr_of(oos_hal::wlan::IFACE));
        }
        if self.bt_stick {
            let open = self.menu_open && self.menu_root() == MenuId::Bluetooth;
            self.draw_chip(cv, self.bt_box(), "BT", self.bt_inputs > 0, open);
        }
        if self.usb {
            let open = self.menu_open && self.menu_root() == MenuId::Storage;
            self.draw_chip(cv, self.usb_box(), "USB", true, open);
        }
    }

    /// Messages as an amber pill at the top centre of the plot; next to or under an open menu dialog that would
    /// cover it (it never hides dialog controls).
    pub(super) fn draw_toast(&self, cv: &mut Canvas) {
        let Some((s, _)) = &self.toast else { return };
        let f = self.fonts();
        let tw = f.mb.width(s) + 44;
        let body = self.frame().body;
        let mut r = Rect::new(body.x + body.w / 2 - tw / 2, body.y + 4, tw, 28);
        if self.eye_open() {
            let pane = self.eye_pane_rect();
            if overlaps(pane, r) {
                let wave = self.grid;
                let room = pane.x - wave.x - 12;
                if room >= tw {
                    r.x = wave.x + (room - tw) / 2;
                }
            }
        }
        let dlg = self.dlg_layout().map(|d| d.r).filter(|d| overlaps(*d, r));
        if let Some(d) = dlg {
            let (left, right) = (d.x - 6, body.right() - 6 - d.right());
            if right >= tw + 16 {
                r.x = d.x + d.w + (right - tw) / 2;
            } else if left >= tw + 16 {
                r.x = 6 + (left - tw) / 2;
            } else {
                r.y = d.y + d.h + 6;
            }
        }
        cv.round(r, 6, mix(BAR, AMBER, 110), 255);
        cv.round(r.inset(1), 5, mix(BAR, AMBER, 34), 255);
        cv.triangle([(r.x + 11, r.y + 20), (r.x + 25, r.y + 20), (r.x + 18, r.y + 7)], AMBER);
        cv.vline(r.x + 18, r.y + 11, r.y + 15, DARK_TEXT);
        cv.pixel(r.x + 18, r.y + 18, DARK_TEXT, 255);
        cv.text(&f.mb, r.x + 32, r.y + 19, s, AMBER);
    }

    /// The menu a sub menu or picker belongs to (button and card highlights, dialog position).
    pub(super) fn menu_root(&self) -> MenuId {
        let m = match (self.menu, self.pick) {
            (MenuId::Pick, Some(p)) => p.menu,
            (m, _) => m,
        };
        if matches!(m, MenuId::MeasAdd | MenuId::MeasHold | MenuId::MeasSelected) {
            MenuId::Measure
        } else if m == MenuId::SimAdvanced {
            MenuId::Sim
        } else {
            m
        }
    }

    /// Bottom cards after the Menu button: channels (wide when on, up to 213 px), TIME (at least 130 px), trigger
    /// (at least 222 px).
    pub(super) fn cards(&self) -> ([Rect; NCH], Rect, Rect) {
        let c = self.card_layout();
        (c.ch, c.time, c.trig)
    }

    /// The cards of the logic analyser and the generator, where the model has them.
    pub(super) fn aux_cards(&self) -> (Option<Rect>, Option<Rect>) {
        let c = self.card_layout();
        (c.la, c.gen)
    }

    /// All bottom cards. LA and generator sit behind the channels, wide like them when they are on and the channels keep
    /// room for their text, else narrow.
    pub(super) fn card_layout(&self) -> Cards {
        if self.m.bar_compact {
            self.compact_cards()
        } else if self.record_mode() {
            // a stored record or the simulator on the screen (Offline, Take over, SIM): no analog channel, LA or
            // generator cards, the time base and trigger stay (as the compact bar has it, PR #8)
            let c = self.card_layout_cards();
            Cards { ch: c.ch.map(|r| Rect::new(r.x, r.y, 0, r.h)), la: None, gen: None, time: c.time, trig: c.trig }
        } else {
            self.card_layout_cards()
        }
    }

    /// The cards of the card bar (Display > Bottom bar = Cards, the default).
    fn card_layout_cards(&self) -> Cards {
        let row = self.frame().cards;
        let (y, h) = (row.y, row.h);
        // 2-channel models: no cards for CH3/CH4 (zero width, never drawn or hit)
        let n = n_ch();
        let n_on = (0..n).filter(|&c| self.m.ch[c].on).count() as i32;
        let n_off = n as i32 - n_on;
        let caps = &oos_hal::model::platform().caps;
        let aux = [(caps.la, self.m.la.on), (caps.has_gen(), self.m.afg_on)];
        let start = row.x;
        let room = |wide: bool| {
            let a: i32 = aux.iter().filter(|a| a.0).map(|a| if a.1 && wide { AUX_WIDE } else { AUX_NARROW } + 7).sum();
            row.right() - start - n_off * (56 + 7) - n_on * 7 - a - (130 + 7) - 222
        };
        let wide = n_on == 0 || room(true) / n_on.max(1) >= CH_MIN;
        let won = if n_on > 0 { (room(wide) / n_on).min(213) } else { 0 };
        let mut x = start;
        let mut ch = [Rect::new(0, 0, 0, 0); NCH];
        for (c, r) in ch.iter_mut().enumerate() {
            if c >= n {
                *r = Rect::new(x, y, 0, h);
                continue;
            }
            let w = if self.m.ch[c].on { won } else { 56 };
            *r = Rect::new(x, y, w, h);
            x += w + 7;
        }
        let mut next = |(has, on): (bool, bool)| {
            has.then(|| {
                let w = if on && wide { AUX_WIDE } else { AUX_NARROW };
                let r = Rect::new(x, y, w, h);
                x += w + 7;
                r
            })
        };
        let (la, gen) = (next(aux[0]), next(aux[1]));
        let rest = row.right() - x - 7;
        let tw = ((rest as f32 * 0.42) as i32).min(rest - 222).max(130);
        let tw = tw.min(TIME_CARD_MAX);
        let trw = (rest - tw).min(TRIG_CARD_MAX);
        // a wide screen: TIME and trigger keep their largest size and move to the right edge, the room stays between
        // the channels and them (as in the top bar)
        let x = row.right() - trw - 7 - tw;
        Cards { ch, la, gen, time: Rect::new(x, y, tw, h), trig: Rect::new(x + tw + 7, y, trw, h) }
    }

    /// The big digits of a card while `s` fits `room`, else the next size: nothing is cut off with every card on.
    pub(super) fn card_font(f: &'static Fonts, s: &str, room: i32) -> &'static Font {
        fit_font(&[&f.l, &f.m, &Fonts::get(0).m], room, s)
    }

    /// The line under V/div of a channel card: coupling, probe, bandwidth, invert, offset.
    pub(super) fn card_sub(&self, c: usize) -> String {
        let ch = &self.m.ch[c];
        let mut sub = vec![ch.coupling.name().to_string(), format!("{}\u{d7}", ch.probe)];
        if ch.bw != 0 && ch.bw != BW_FULL {
            sub.push(format!("BW {}", BWS[ch.bw].0));
        }
        if ch.invert {
            sub.push("Inv".into());
        }
        sub.push(si_short(ch.offset as f64, ch.unit.name()));
        sub.join(" \u{b7} ")
    }

    /// Font of that line, one for all channel cards: the normal size when a line would run out of its card at the
    /// text size.
    pub(super) fn card_sub_font(&self) -> &'static Font {
        let (f, chs) = (self.fonts(), self.cards().0);
        let all_fit = (0..NCH)
            .filter(|&c| chs[c].w > 0 && self.m.ch[c].on)
            .all(|c| f.s.width(&self.card_sub(c)) <= chs[c].w - 22);
        if all_fit {
            &f.s
        } else {
            &Fonts::get(0).s
        }
    }

    /// Trigger card buttons, one over the other at the right edge (touch): source, sweep; the rest of the card, and with
    /// it the level, opens the menu.
    pub(super) fn trig_parts(r: Rect) -> (Rect, Rect) {
        let (w, h) = (66, (r.h - 12 - 4) / 2);
        let x = r.x + r.w - w - 6;
        (Rect::new(x, r.y + 6, w, h), Rect::new(x, r.y + 6 + h + 4, w, h))
    }

    /// A card like the channels': on, wide with its values; on, narrow with a full dot; off, with a ring.
    fn draw_aux_card(&self, cv: &mut Canvas, r: Rect, card: AuxCard) {
        let AuxCard { name, col, on, sel, big, sub } = card;
        let f = self.fonts();
        cv.round(r, 7, if sel || on { col } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.set_clip(r.inset(3));
        if on && r.w > AUX_NARROW {
            cv.fill(Rect::new(r.x + 6, r.y + 10, 3, r.h - 20), col);
            let (yn, yb, ys) = self.lay().card;
            cv.text(&f.sb, r.x + 16, r.y + yn, name, col);
            cv.circle(r.x + r.w - 16, r.y + 14, 5, col);
            cv.text(fit_font(&[&f.mb, &Fonts::get(0).mb], r.w - 22, big), r.x + 16, r.y + yb, big, TEXT);
            cv.text(fit_font(&[&f.s, &Fonts::get(0).s], r.w - 22, sub), r.x + 16, r.y + ys, sub, DIM);
        } else {
            cv.text_in(&f.sb, Rect::new(r.x, r.y + 8, r.w, 18), 1, name, if on { col } else { LABEL });
            let (cx, cy) = (r.x + r.w / 2, r.y + 45);
            cv.circle(cx, cy, 7, col);
            if !on {
                cv.circle(cx, cy, 6, BAR);
            }
        }
        cv.no_clip();
    }

    pub(super) fn draw_bottom(&self, cv: &mut Canvas) {
        if self.m.bar_compact {
            self.draw_workspace_footer(cv);
        } else {
            self.draw_bottom_cards(cv);
        }
    }

    /// The card bar: Menu, a card per channel, LA, generator, time base and trigger.
    fn draw_bottom_cards(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let (yn, yb, ys) = self.lay().card;
        // a line that would run out of a narrow card takes the normal size
        let small = |s: &str, room: i32| fit_font(&[&f.s, &Fonts::get(0).s], room, s);
        cv.fill(self.frame().bottom, BG);
        let root = if self.menu_open { Some(self.menu_root()) } else { None };
        self.draw_menu_button(cv, f, root);
        let (chs, tr, trg) = self.cards();
        let sub_font = self.card_sub_font();
        for (c, &r) in chs.iter().enumerate() {
            if r.w == 0 {
                continue;
            }
            let ch = &self.m.ch[c];
            let col = CH_COLORS[c];
            let sel = root == Some(MenuId::Channel(c));
            let border = if ch.on && (self.m.active == c || sel) { col } else { CARD_BORDER };
            cv.round(r, 7, border, 255);
            cv.round(r.inset(2), 5, BAR, 255);
            // narrow cards: the text stays inside
            cv.set_clip(r.inset(3));
            let name = self.ch_name(c);
            if ch.on {
                cv.fill(Rect::new(r.x + 6, r.y + 10, 3, r.h - 20), col);
                cv.text(&f.sb, r.x + 16, r.y + yn, &name, col);
                cv.circle(r.x + r.w - 16, r.y + 14, 5, col);
                let scale = format!("{}/div", si_short(ch.volts_per_div() as f64, ch.unit.name()));
                cv.text(Self::card_font(f, &scale, r.w - 24), r.x + 16, r.y + yb, &scale, TEXT);
                cv.text(sub_font, r.x + 16, r.y + ys, &self.card_sub(c), DIM);
            } else {
                cv.text_in(&f.sb, Rect::new(r.x, r.y + 8, r.w, 18), 1, &name, LABEL);
                let (cx, cy) = (r.x + r.w / 2, r.y + 45);
                cv.circle(cx, cy, 7, col);
                cv.circle(cx, cy, 6, BAR);
            }
            cv.no_clip();
        }
        // logic analyser and generator, in the manner of the channels
        let (la, gen) = self.aux_cards();
        if let Some(r) = la {
            let sel = matches!(root, Some(MenuId::La | MenuId::LaLines | MenuId::LaPat));
            let l = &self.m.la;
            let sub = format!("{} \u{b7} {}", si_short(l.thr(0) as f64, "V"), si_short(l.thr(1) as f64, "V"));
            // grey without a probe: nothing to switch on
            let col = if l.on || self.la_probe_ok() { la::line_color(0) } else { CARD_BORDER };
            let big = format!("{} lines", l.lines().len());
            self.draw_aux_card(cv, r, AuxCard { name: "LA", col, on: l.on, sel, big: &big, sub: &sub });
        }
        if let Some(r) = gen {
            let (big, sub) = self.afg_card_text();
            let sel = root == Some(MenuId::Afg);
            self.draw_aux_card(
                cv,
                r,
                AuxCard { name: "GEN", col: GEN_COLOR, on: self.m.afg_on, sel, big: &big, sub: &sub },
            );
        }
        // time base (sample rate and depth are in the top bar)
        let r = tr;
        let sel = root == Some(MenuId::Horizontal);
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.text(&f.sb, r.x + 12, r.y + yn, "TIME", LABEL);
        let tdiv = format!("{}/div", si_short(self.m.time_per_div() as f64, "s"));
        cv.text(Self::card_font(f, &tdiv, r.w - 20), r.x + 12, r.y + yb, &tdiv, TEXT);
        if self.m.zoom {
            cv.text(
                &f.sb,
                r.x + 12,
                r.y + ys,
                &format!("zoom {}/div", si_short(tscales()[self.m.zscale] as f64, "s")),
                ACCENT,
            );
        } else {
            let t = format!("delay {}", si_short(self.m.delay as f64, "s"));
            cv.text(small(&t, r.w - 18), r.x + 12, r.y + ys, &t, DIM);
        }
        self.draw_trigger_card(cv, f, trg, root, yn);
    }

    /// The start menu button: three bars and "Menu"; amber "Locked" while touch is locked (a tap unlocks).
    fn draw_menu_button(&self, cv: &mut Canvas, f: &'static Fonts, root: Option<MenuId>) {
        let r = self.menu_btn();
        let (hl, lock) = (root == Some(MenuId::Main), self.m.touch_lock);
        cv.round(
            r,
            7,
            if hl {
                ACCENT
            } else if lock {
                AMBER
            } else {
                CARD_BORDER
            },
            255,
        );
        cv.round(r.inset(2), 5, BAR, 255);
        let col = if hl {
            ACCENT
        } else if lock {
            AMBER
        } else {
            TEXT
        };
        for k in 0..3 {
            cv.fill(Rect::new(r.x + r.w / 2 - 11, r.y + 16 + k * 7, 22, 3), col);
        }
        cv.text_in(&f.sb, Rect::new(r.x, r.y + 42, r.w, 16), 1, if lock { "Locked" } else { "Menu" }, col);
    }

    /// The trigger card: type, slope / polarity / standard and level; source and sweep buttons.
    fn draw_trigger_card(&self, cv: &mut Canvas, f: &'static Fonts, r: Rect, root: Option<MenuId>, yn: i32) {
        let sel = root == Some(MenuId::Trigger);
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        // type and slope on the first line, the level under them: it has the width up to the buttons
        let tx = cv.text(&f.sb, r.x + 12, r.y + yn, &self.m.trig_type.name().to_uppercase(), LABEL) + 8;
        if matches!(
            self.m.trig_type,
            TrigType::Edge | TrigType::Slope | TrigType::Timeout | TrigType::NthEdge | TrigType::Delay
        ) {
            draw_slope(cv, tx, r.y + 6, self.m.slope, ACCENT);
        } else {
            let tag = match self.m.trig_type {
                TrigType::Pulse | TrigType::Runt => {
                    if self.m.trig_pol == Pol::Pos {
                        "+"
                    } else {
                        "-"
                    }
                }
                TrigType::Video => {
                    if self.m.video_std == VideoStd::Pal {
                        "P"
                    } else {
                        "N"
                    }
                }
                _ => "",
            };
            cv.text(&f.sb, tx, r.y + yn, tag, ACCENT);
        }
        // an LA line has the pod's threshold, not a level of its own
        let lv = match self.m.trig_line() {
            Some(n) => si(self.m.la.thr(n as usize / 8) as f64, "V"),
            None => si(self.trig_level_shown() as f64, self.m.unit(self.m.trig_src)),
        };
        let (sb, mb) = Self::trig_parts(r);
        // the big digits while they fit
        cv.text(Self::card_font(f, &lv, sb.x - r.x - 18), r.x + 12, r.y + 50, &lv, TEXT);
        let mode = match self.m.trig_mode {
            TrigMode::Auto => "Auto",
            TrigMode::Normal => "Normal",
            TrigMode::Single => "Single",
        };
        let ts = self.m.trig_src;
        let (src, scol) = match self.m.trig_line() {
            Some(n) => (format!("D{}", n), la::line_color(n)),
            None => (format!("CH{}", ts + 1), CH_COLORS[ts]),
        };
        for (b, val, col) in [(sb, src, scol), (mb, mode.to_string(), TEXT)] {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.sb, b, 1, &val, col);
        }
    }

    /// Front-panel LED mask (19 LEDs, stock order; `Inputs::set_leds` maps it to
    /// the MCU's hardware bits):
    /// 0-3 CH1-CH4, 4 RUN green, 5 STOP red, 6 SINGLE red, 7 SINGLE orange, 8 LA, 9 G1, 10 G2, 11 TOUCH,
    /// 12 A up/down, 13 B left/right, 14 SLOPE1, 15 SLOPE2, 16 A left/right, 17 B up/down, 18 INTENSITY.
    pub fn leds(&self) -> u32 {
        let mut m = 0u32;
        for c in 0..NCH {
            if self.m.ch[c].on {
                m |= 1 << c;
            }
        }
        if self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run {
            m |= (1 << 6) | (1 << 7); // the stock system always drives both SINGLE LEDs together
        } else if self.m.run == RunState::Run {
            m |= 1 << 4;
        } else {
            m |= 1 << 5;
        }
        if self.m.touch_lock {
            m |= 1 << 11;
        }
        match self.m.slope {
            Slope::Rise => m |= 1 << 14,
            Slope::Fall => m |= 1 << 15,
            Slope::Either => m |= (1 << 14) | (1 << 15),
        }
        m
    }
}

fn draw_slope(cv: &mut Canvas, x: i32, y: i32, s: Slope, col: Rgb) {
    draw_slope1(cv, x, y, s, col);
    draw_slope1(cv, x + 1, y, s, col);
}

fn draw_slope1(cv: &mut Canvas, x: i32, y: i32, s: Slope, col: Rgb) {
    // 14 x 16 icon: rising / falling edge
    let (x0, x1, x2, x3) = (x, x + 5, x + 9, x + 14);
    let (ylo, yhi) = (y + 14, y + 2);
    match s {
        Slope::Rise => {
            cv.hline(x0, x1, ylo, col);
            cv.line(x1, ylo, x2, yhi, col);
            cv.hline(x2, x3, yhi, col);
        }
        Slope::Fall => {
            cv.hline(x0, x1, yhi, col);
            cv.line(x1, yhi, x2, ylo, col);
            cv.hline(x2, x3, ylo, col);
        }
        Slope::Either => {
            cv.line(x0, ylo, x0 + 6, yhi, col);
            cv.line(x0 + 7, yhi, x3, ylo, col);
        }
    }
}
