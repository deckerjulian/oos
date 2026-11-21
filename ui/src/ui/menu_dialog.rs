// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The menu dialog: opening and closing a menu, the option pickers, the layout of the dialog (rows, segmented buttons,
//! the measurement list), drawing it and the taps on it, and the overlay over the screen. What a row shows and what it
//! does are in `menu_text.rs` and `menu_actions.rs`; a new kind of control goes into `Ui::ctl` and `Ui::draw_menu`.

use super::*;

impl Ui {
    pub(super) fn open_menu(&mut self, id: MenuId) {
        self.zoom_box.cancel();
        self.menu = id;
        self.menu_open = true;
        self.parent = None;
        self.menu_history.clear();
        self.pick = None;
        self.focus = 0;
    }

    pub(super) fn open_submenu(&mut self, id: MenuId) {
        let mut history = std::mem::take(&mut self.menu_history);
        history.push((self.menu, self.focus));
        self.open_menu(id);
        self.parent = history.last().map(|entry| entry.0);
        self.menu_history = history;
    }

    /// How long a finger stays down on Measure before the short menu opens. A shorter press is a tap.
    const MEAS_HOLD_S: f64 = 0.45;

    /// Usual measurements, in the order the Add meas shortcut offers them. Anything left follows `ALL_MEAS`.
    const USUAL_MEAS: [Meas; 6] = [Meas::Vpp, Meas::Freq, Meas::Period, Meas::Vrms, Meas::Vavg, Meas::Rise];

    /// The next usual measurement that is not already on the measure source.
    pub(super) fn next_usual_meas(&self) -> Option<Meas> {
        let src = self.sources.a;
        let on = |k: Meas| self.m.meas.iter().any(|e| e.0 == k && e.1 == src);
        Self::USUAL_MEAS.into_iter().chain(ALL_MEAS).find(|k| !on(*k))
    }

    /// Rectangle of the Measure button on the top bar.
    pub(super) fn measure_btn(&self) -> Option<Rect> {
        self.top_buttons().into_iter().find(|b| b.0 == TopBtn::Menu(MenuId::Measure)).map(|b| b.2)
    }

    fn on_measure_btn(&self, x: i32, y: i32) -> bool {
        self.measure_btn().is_some_and(|r| self.top_hit(r).contains(x, y))
    }

    /// A tap on Measure opens the measurement list. Press again to close it.
    pub(super) fn open_meas_list(&mut self) {
        self.ensure_meas_own();
        if self.menu_open && self.menu == MenuId::MeasAdd {
            self.close_menu();
        } else {
            self.open_menu(MenuId::MeasAdd);
        }
    }

    /// A finger held on Measure opens the short menu. The lift is handled by `meas_hold_up`, not as a tap.
    pub(super) fn meas_hold_tick(&mut self) {
        let t = &self.touch;
        if !t.down
            || t.held
            || self.m.touch_lock
            || self.full_on()
            || self.notices.info.is_some()
            || self.power.open
            || self.wlan.osk.is_some()
        {
            return;
        }
        if t.drag != Drag::None && t.drag != Drag::Pending {
            return;
        }
        if (t.x - t.x0).abs() > 12 || (t.y - t.y0).abs() > 12 || self.now - t.t0 < Self::MEAS_HOLD_S {
            return;
        }
        if !self.on_measure_btn(t.x0, t.y0) {
            return;
        }
        self.touch.held = true;
        self.open_menu(MenuId::MeasHold);
    }

    /// Lift after the short menu opened. On the Measure button, or on Add meas, that shortcut runs.
    /// A lift anywhere else leaves the menu so the shortcut can still be tapped.
    pub(super) fn meas_hold_up(&mut self, x: i32, y: i32) {
        if !(self.menu_open && self.menu == MenuId::MeasHold) {
            return;
        }
        if self.tap_menu(x, y) {
            return;
        }
        if self.on_measure_btn(x, y) {
            self.activate(Item::AddMeas);
            self.close_menu();
        }
    }

    pub(super) fn toggle_menu(&mut self, id: MenuId) {
        if self.menu_open && self.menu_root() == id {
            self.close_menu();
        } else {
            self.open_menu(id);
        }
    }

    /// The options of a choice item for segmented buttons (empty: shown as - value +).
    pub(super) fn options(&self, it: Item) -> Vec<String> {
        if let Some(o) = PARTS.iter().find_map(|p| p.options.and_then(|f| f(self, it))) {
            return o;
        }
        let v: &[&str] = match it {
            Item::StripDock => return StripDock::ALL.iter().map(|s| s.label().to_string()).collect(),
            Item::MeasStats | Item::MeasLabels | Item::Zoom | Item::AutoRoll | Item::DecOn | Item::SearchOn => {
                &["Off", "On"]
            }
            Item::MaskSrc => return ch_names(),
            Item::ChCoupling(_) => return board().couplings().iter().map(|c| c.name().to_string()).collect(),

            Item::ChProbe(_) => &["1x", "10x", "100x"],
            Item::TrigSrc | Item::TrigSrc2 => {
                return self.trigger_sources().into_iter().map(Self::trigger_wave_label).collect()
            }
            Item::DecSrc => return self.decoder_sources().into_iter().map(decode_source_name).collect(),
            Item::RefSrc => return ch_names(),
            Item::MeasSrc | Item::MeasSrcB | Item::CurSrcB | Item::CatSourceC | Item::CatSourceD => {
                return self.wave_names()
            }
            Item::MeasCopyTo => {
                return self.wave_names().into_iter().filter(|n| *n != self.wave_name(self.sources.a)).collect()
            }
            Item::MathA | Item::MathB => {
                return self.math_source_ids(self.m.math_sel).into_iter().map(src_name).collect()
            }
            Item::MathSel => return (0..NMATH).map(|k| format!("M{}", k + 1)).collect(),
            Item::TrigWhen => {
                if self.m.trig_type == TrigType::Runt {
                    &["Off", ">", "<", "Inside", "Outside"]
                } else {
                    &[">", "<", "Inside", "Outside"]
                }
            }
            Item::Pat(_) => {
                if self.m.trig_type == TrigType::Duration {
                    &["H", "L", "X"]
                } else {
                    &["H", "L", "X", "Rise", "Fall"]
                }
            }
            Item::DecProto => return self.protos_on().iter().map(|p| p.name().to_string()).collect(),
            Item::DecUsbSpeed => &USPEED_NAMES,
            Item::DecOpt(k) => {
                return self
                    .m
                    .dec_proto
                    .module()
                    .and_then(|p| p.options().get(k))
                    .map_or(Vec::new(), |o| o.choices.iter().map(|c| c.to_string()).collect())
            }
            Item::DecSrc4 | Item::DecDiff => {
                return std::iter::once("None".into())
                    .chain(self.decoder_sources().into_iter().map(decode_source_name))
                    .collect()
            }
            Item::DecCanH => &["RX/TX", "CAN_H"],
            Item::TrigBusWhat => &["Frame", "ID/Addr", "Data", "Error"],
            Item::DecSrc2 => return self.decoder_sources().into_iter().map(decode_source_name).collect(),
            Item::DecCs => {
                return (if self.m.dec_proto == Proto::CPHY { Vec::new() } else { vec!["None".into()] })
                    .into_iter()
                    .chain(self.decoder_sources().into_iter().map(decode_source_name))
                    .collect()
            }
            Item::DecMode => &["0", "1", "2", "3"],
            Item::DecParity => &["None", "Even", "Odd"],
            Item::CurMode => &["Off", "Time", "Volt", "Track", "XY"],
            Item::ZoneOn => &["Off", "On"],
            Item::Hdmi => &["Mirror", "Waveform", "Layout"],
            Item::TextSize => return TEXT_SIZES.iter().map(|t| t.name().to_string()).collect(),
            Item::ZoneSel => &["Z1", "Z2"],
            Item::ZoneSrc => return ch_names(),
            Item::ZoneMode => &["Intersect", "Not intersect"],
            Item::ZInput => return opt_ch_names("Off"),
            Item::AuxOut => {
                return AuxOut::choices(board().mask_hw() || board().zone_hw())
                    .iter()
                    .map(|a| a.name().to_string())
                    .collect()
            }
            Item::DecFmt => &["Hex", "ASCII"],
            Item::XyMode => &["YT", "XY"],
            Item::DecSel => &["D1", "D2", "D3", "D4"],
            Item::PaneSel => &tables::PANES,
            Item::TrendSel(_) => return self.trend_options(),
            Item::DvmSrc | Item::CntSrc => return (0..n_ch()).map(|c| format!("CH{}", c + 1)).collect(),
            Item::HFine | Item::LogOn => &["Off", "On"],
            Item::HExpandI => &["Center", "Left", "Right", "Trigger"],
            Item::BootMode => &["One-shot", "Persist"],
            // long lists: shown as a picker (see `ctl`)
            Item::MathOpI => return self.math_ops_in_family().iter().map(|o| o.name().to_string()).collect(),
            Item::MathFam => return MathFamily::ALL.iter().map(|f| f.name().to_string()).collect(),
            Item::StoreDest => return crate::store::StoreDest::ALL.iter().map(|d| d.name().to_string()).collect(),
            Item::TrigType => return TRIG_TYPES.iter().map(|t| t.name().to_string()).collect(),
            Item::AcqDepth => return mdepths().iter().map(|d| d.0.to_string()).collect(),
            Item::AcqSeg => {
                return self.seg_options().iter().map(|&n| if n == 0 { "Off".into() } else { n.to_string() }).collect()
            }
            Item::AcqSegIvl => return SEG_IVLS.iter().map(|&x| seg_ivl_name(x)).collect(),
            Item::DecBaud => return crate::decode::BAUDS.iter().map(|b| b.to_string()).collect(),
            Item::DecCphyRate => {
                return crate::decode::CPHY_RATES.into_iter().map(crate::decode::cphy_rate_label).collect()
            }
            Item::DecCanRate => return crate::decode::CAN_RATES.iter().map(|&b| si(b, "bit/s")).collect(),
            // the value picker is a hex keypad
            Item::TrigBusVal | Item::SerMax | Item::SerMask => return KEYPAD.iter().map(|k| k.to_string()).collect(),
            Item::SerRel
            | Item::UartPol
            | Item::UartStop
            | Item::UartErr
            | Item::SpiCs
            | Item::I2cDir
            | Item::I2cAddrBits
            | Item::I2cBytes => return Self::serial_options(it),
            Item::AfgFreqEntry => return FREQ_KEYS.iter().map(|k| k.to_string()).collect(),
            Item::NumEntry => return self.numpad_keys(),
            Item::WlanNet => return self.wlan_net_options(),
            Item::WlanForgetNet => return self.wlan_forget_options(),
            Item::RefImport => return self.ref_page.files.iter().map(|p| crate::store::file_name(p)).collect(),
            Item::QuickAct => return QUICKS.iter().map(|q| q.name().to_string()).collect(),
            Item::Overclock => return board().clock_steps().iter().map(|&hz| overclock::label(hz)).collect(),
            _ => &[],
        };
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Control of a menu row whose control rectangle is `w` px wide: segmented buttons when all options fit, a
    /// picker (- value +, the value opens the list of all options) for longer option lists, else - value +.
    pub(super) fn ctl(&self, it: Item, w: i32) -> Ctl {
        // - and + step, the value opens the keypad (`open_picker`); the name's and the address list's value open their
        // list even when short
        if matches!(it, Item::AfgFreq | Item::RemName | Item::RemAllow) {
            return Ctl::Pick;
        }
        if let Item::RefShow(_) = it {
            return Ctl::Button;
        }
        // a finding is one wide button; its text takes a smaller font instead of being cut
        if let Item::IdFind(_) = it {
            let cur = self.value(it);
            return Ctl::Seg(vec![cur.clone()], Some(0));
        }
        let text = matches!(it, Item::MathExprI | Item::LogInfo | Item::CalStatus | Item::NetEth | Item::NetMac);
        let text = text || matches!(it, Item::NetPorts | Item::NetWlan | Item::AcqAlias);
        if text || PARTS.iter().any(|p| p.text.is_some_and(|f| f(it))) {
            return Ctl::Text;
        }
        match Self::kind(it) {
            Kind::Action => Ctl::Button,
            k => {
                let opts = self.options(it);
                let n = opts.len() as i32;
                let f = self.fonts();
                if n == 0 {
                    Ctl::Value
                } else if k == Kind::Choice && opts.iter().all(|o| f.mb.width(o) + 12 <= (w - (n - 1) * 4) / n) {
                    let cur = self.value(it);
                    let sel = opts.iter().position(|o| *o == cur);
                    Ctl::Seg(opts, sel)
                } else {
                    Ctl::Pick
                }
            }
        }
    }

    /// Opens the picker of a choice item of the current menu; picking an option returns to this menu.
    pub(super) fn open_picker(&mut self, it: Item) {
        let it = if it == Item::AfgFreq { Item::AfgFreqEntry } else { it };
        if Self::hex_item(it) {
            self.keypad = format!("{:X}", self.hex_value(it));
        }
        if it == Item::AfgFreqEntry {
            self.keypad.clear();
        }
        let opts = self.options(it);
        let cur = self.value(it);
        self.pick = Some(Pick { it, menu: self.menu, parent: self.parent, focus: self.focus, top: 0 });
        self.parent = Some(self.menu);
        self.menu = MenuId::Pick;
        self.focus = opts.iter().position(|o| *o == cur).unwrap_or(0);
        self.pick_follow_focus();
    }

    /// Leaves the picker for the menu it was opened from (after a pick, or with Back).
    pub(super) fn close_picker(&mut self) {
        if let Some(p) = self.pick.take() {
            (self.menu, self.parent, self.focus) = (p.menu, p.parent, p.focus);
        }
    }

    /// Sets a choice item to the option `want`: one adjust step by the distance in the option list (every item with
    /// options moves by index, cycling or clamped), else stepping until it shows `want`.
    pub(super) fn select_option(&mut self, it: Item, want: &str) {
        if it == Item::DecCphyRate {
            if let Some(rate) =
                crate::decode::CPHY_RATES.into_iter().find(|&rate| crate::decode::cphy_rate_label(rate) == want)
            {
                self.m.dec_cphy_rate = rate;
                self.settings_changed();
            }
            return;
        }
        if it == Item::MeasCopyTo {
            if let Some(to) = self.wave_ids().into_iter().find(|&w| self.wave_name(w) == want) {
                let from = self.sources.a;
                let n = self.copy_measurements(from, to);
                self.toast(&format!(
                    "{} measurement{} copied {} -> {}",
                    n,
                    if n == 1 { "" } else { "s" },
                    self.wave_name(from),
                    want
                ));
            }
            return;
        }
        if it == Item::EyePlace {
            // a placement choice always places the eye, also the one it already has while it floats
            if let Some(place) = [(EyePlace::Full, "Full"), (EyePlace::Side, "Side"), (EyePlace::Under, "Under")]
                .into_iter()
                .find(|(_, l)| *l == want)
                .map(|(p, _)| p)
            {
                self.workspace.eye_float = None;
                self.eye_set_place(place);
                self.relayout();
            }
            return;
        }
        if it == Item::StripDock {
            // every dock choice docks the card, also the one already selected after it was floated
            if let Some(dock) = StripDock::ALL.into_iter().find(|d| d.label() == want) {
                self.m.strip_dock = dock;
                self.workspace.measure_docked = true;
                self.workspace.measure_float = None;
                self.relayout();
            }
            return;
        }
        let opts = self.options(it);
        let cur = self.value(it);
        if let (Some(k), Some(i)) = (opts.iter().position(|o| o == want), opts.iter().position(|o| *o == cur)) {
            if k != i {
                self.adjust(it, k as i32 - i as i32);
            }
            return;
        }
        for _ in 0..8 {
            if self.value(it) == want {
                return;
            }
            self.adjust(it, 1);
        }
    }

    /// x centre of what opened the menu, and whether that is in the top bar (the dialog hangs below it) or a card
    /// (the dialog sits above it).
    fn menu_anchor(&self) -> (i32, bool) {
        if let Some(a) = pages::page(self.menu_root()).and_then(|p| p.anchor) {
            return a(self, self.menu_root());
        }
        // under its top-bar button, if it has one; else centred above the cards
        let m = self.menu_root();
        self.top_buttons()
            .iter()
            .find(|t| t.0 == TopBtn::Menu(m))
            .map_or((self.scr.w / 2, false), |t| (t.2.x + t.2.w / 2, true))
    }

    pub(super) fn dlg_layout(&self) -> Option<Dlg> {
        if !self.menu_open {
            return None;
        }
        if self.menu == MenuId::MeasAdd {
            return Some(self.catalog_dialog());
        }
        if self.menu == MenuId::MeasSelected {
            return Some(self.measurement_chooser_layout());
        }
        if self.menu == MenuId::Main {
            return Some(self.main_layout());
        }
        if self.menu == MenuId::Measure {
            return Some(self.measure_layout());
        }
        let items = self.items(self.menu);
        let n = items.len() as i32;
        let grid = items.iter().all(|&it| Self::kind(it) == Kind::Action);
        let wide = self.pick.is_some_and(|p| Self::wide_pick(p.it));
        let l = self.lay();
        let (cols, cw, rh, gap) = if self.menu == MenuId::MeasHold {
            (1, 220 + l.grid_col - 150, DLG_ROW, 16)
        } else if wide {
            (2, l.dlg_col, DLG_ROW + 2, 8)
        } else if self.menu == MenuId::ViewMore {
            (1, l.grid_col + 40, DLG_ROW + 2, 8)
        } else if grid && self.menu == MenuId::MeasAdd {
            // the whole catalogue in one grid: narrower columns where the larger text sizes would run off the screen
            if n > 54 {
                (7, ((self.scr.w - 72) / 7).min(l.grid_col - 20), DLG_ROW + 2, 8)
            } else {
                (6, ((self.scr.w - 64) / 6).min(l.grid_col), DLG_ROW + 2, 8)
            }
        } else if grid {
            (4, l.grid_col, DLG_ROW + 2, 8)
        } else if n > 5 {
            (2, l.dlg_col, DLG_ROW, 16)
        } else {
            (1, l.dlg_col, DLG_ROW, 16)
        };
        let nrows = (n + cols - 1) / cols;
        let fr = self.frame();
        // a picker longer than the screen shows a window of its lines and scrolls (ADR 0006)
        let picker = matches!(items.first(), Some(Item::PickOpt(_)));
        let (shown, first) = if picker { self.pick_window(nrows, cols, rh) } else { (nrows, 0) };
        let scrolls = shown < nrows;
        let lines_w = cols * cw + (cols - 1) * gap;
        let w = 24 + lines_w + if scrolls { scroll::BAR_W + 4 } else { 0 };
        let h = DLG_HEAD + shown * rh + 6;
        let (ax, top) = self.menu_anchor();
        let x = (ax - w / 2).clamp(METRICS.edge, self.scr.w - METRICS.edge - w);
        let y = if top { fr.top.bottom() + 4 } else { (fr.bottom.y - 4 - h).max(fr.top.bottom() + 4) };
        let r = Rect::new(x, y, w, h);
        let close = Rect::new(r.x + r.w - 52, r.y + 6, 44, DLG_CTRL);
        let back = self.parent.map(|_| Rect::new(close.x - 88, close.y, 82, DLG_CTRL));
        let bar = scrolls.then(|| {
            let area = Rect::new(r.x + 12, r.y + DLG_HEAD, lines_w, shown * rh - (rh - DLG_CTRL));
            let track = Rect::new(area.right() + 4, area.y, scroll::BAR_W, area.h);
            (scroll::Bar { track, total: nrows, shown, pos: first }, area)
        });
        let mut rows = Vec::new();
        for (i, &it) in items.iter().enumerate() {
            let i = i as i32;
            if grid {
                let (col, row) = (i % cols, i / cols);
                // a line scrolled out of the window has no place: it is neither drawn nor hit
                let ctrl = if (first..first + shown).contains(&row) {
                    Rect::new(r.x + 12 + col * (cw + gap), r.y + DLG_HEAD + (row - first) * rh, cw, DLG_CTRL)
                } else {
                    Rect::new(r.x, r.y, 0, 0)
                };
                rows.push(Row { it, label: Rect::new(ctrl.x, ctrl.y, 0, 0), ctrl, ctl: Ctl::Button });
                continue;
            }
            let (col, row) = (i / nrows, i % nrows);
            let cx = r.x + 12 + col * (cw + gap);
            let ry = r.y + DLG_HEAD + row * rh;
            let label = Rect::new(cx, ry, l.dlg_label, DLG_CTRL);
            let lw = l.dlg_label + 4;
            let ctrl = if Self::kind(it) == Kind::Action {
                Rect::new(cx, ry, cw, DLG_CTRL)
            } else {
                Rect::new(cx + lw, ry, cw - lw, DLG_CTRL)
            };
            rows.push(Row { it, label, ctl: self.ctl(it, ctrl.w), ctrl });
        }
        Some(Dlg { r, close, back, rows, heads: Vec::new(), bar })
    }

    /// The lines of the open picker that show, and the first of them: all of them when they fit between the bars,
    /// else as many as fit, from the stored position moved just far enough that the focused option is among them.
    fn pick_window(&self, lines: i32, cols: i32, rh: i32) -> (i32, i32) {
        let Some(p) = self.pick else { return (lines, 0) };
        let fr = self.frame();
        let room = fr.bottom.y - 4 - (fr.top.bottom() + 4);
        let fit = ((room - DLG_HEAD - 6) / rh).max(1);
        if lines <= fit {
            return (lines, 0);
        }
        let line = self.focus as i32 / cols;
        (fit, p.top.clamp(line + 1 - fit, line).clamp(0, lines - fit))
    }

    /// Keeps the position of a long picker where it shows the focused option (after the knob moved the focus).
    pub(super) fn pick_follow_focus(&mut self) {
        let Some(top) = self.dlg_layout().and_then(|d| d.bar).map(|(b, _)| b.pos) else { return };
        if let Some(p) = self.pick.as_mut() {
            p.top = top;
        }
    }

    /// Start menu: one row of up to 6 buttons per group, the group label left of it; above the Menu button.
    fn main_layout(&self) -> Dlg {
        let (lw, cw, gap) = (104, 140, 6);
        let groups = self.main_groups();
        // six buttons per line; a longer group continues on the next line (label on its first line only)
        const PER_LINE: usize = 6;
        let lines: usize = groups.iter().map(|g| g.1.len().div_ceil(PER_LINE).max(1)).sum();
        let w = 24 + lw + PER_LINE as i32 * cw + (PER_LINE as i32 - 1) * gap;
        let h = DLG_HEAD + lines as i32 * (DLG_ROW + 2) + 6;
        let r = Rect::new(METRICS.edge, self.frame().bottom.y - 4 - h, w, h);
        let close = Rect::new(r.x + r.w - 52, r.y + 6, 44, DLG_CTRL);
        let (mut rows, mut heads) = (Vec::new(), Vec::new());
        let mut line = 0;
        for (name, items) in groups {
            let y0 = r.y + DLG_HEAD + line as i32 * (DLG_ROW + 2);
            heads.push((Rect::new(r.x + 14, y0, lw, DLG_CTRL), name));
            let n = items.len().div_ceil(PER_LINE).max(1);
            for (i, it) in items.into_iter().enumerate() {
                let y = y0 + (i / PER_LINE) as i32 * (DLG_ROW + 2);
                let ctrl = Rect::new(r.x + 12 + lw + (i % PER_LINE) as i32 * (cw + gap), y, cw, DLG_CTRL);
                rows.push(Row { it, label: Rect::new(ctrl.x, ctrl.y, 0, 0), ctrl, ctl: Ctl::Button });
            }
            line += n;
        }
        Dlg { r, close, back: None, rows, heads, bar: None }
    }

    /// The group of the measurement settings a row belongs to (`MEASURE_GROUPS`).
    fn measure_group(it: Item) -> usize {
        match it {
            Item::MeasSrc | Item::MeasSrcB | Item::MeasCopyTo => 0,
            Item::MeasGate | Item::MeasRefs => 1,
            Item::MeasStats | Item::MeasLabels | Item::StripDock => 2,
            Item::MeasIdle | Item::MeasTsetup | Item::MeasThold | Item::MeasOutside | Item::MeasN | Item::MeasEdges => {
                3
            }
            _ => 4,
        }
    }

    /// The groups of the measurement settings: name and column.
    const MEASURE_GROUPS: [(&'static str, usize); 5] =
        [("Sources", 0), ("Measure over", 0), ("Display", 0), ("Parameters", 1), ("More", 1)];

    /// Measurement settings: two columns of named groups (what is measured, over what, how the results show; the
    /// parameters of single measurements, and the pages beside them), below the top bar. The pages of the last group
    /// share a line.
    fn measure_layout(&self) -> Dlg {
        const HEAD: i32 = 24;
        let l = self.lay();
        let gap = 16;
        let cw = l.dlg_col;
        let w = 24 + 2 * cw + gap;
        let (ax, _) = self.menu_anchor();
        let x = (ax - w / 2).clamp(METRICS.edge, self.scr.w - METRICS.edge - w);
        let y = self.frame().top.bottom() + 4;
        let mut col_y = [y + DLG_HEAD - 8; 2];
        let (mut rows, mut heads) = (Vec::new(), Vec::new());
        let mut last = usize::MAX;
        // the left half of a line of two pages is taken: the next page goes right of it
        let mut half: Option<i32> = None;
        for it in self.items(MenuId::Measure) {
            let g = Self::measure_group(it);
            let (name, col) = Self::MEASURE_GROUPS[g];
            let cx = x + 12 + col as i32 * (cw + gap);
            if g != last {
                heads.push((Rect::new(cx, col_y[col], cw, HEAD), name));
                col_y[col] += HEAD;
                last = g;
                half = None;
            }
            if let Item::Open(_) = it {
                let hw = (cw - 6) / 2;
                let ctrl = match half.take() {
                    Some(ry) => Rect::new(cx + hw + 6, ry, cw - hw - 6, DLG_CTRL),
                    None => {
                        let ry = col_y[col];
                        col_y[col] += DLG_ROW;
                        half = Some(ry);
                        Rect::new(cx, ry, hw, DLG_CTRL)
                    }
                };
                rows.push(Row { it, label: Rect::new(ctrl.x, ctrl.y, 0, 0), ctrl, ctl: Ctl::Button });
                continue;
            }
            half = None;
            let ry = col_y[col];
            col_y[col] += DLG_ROW;
            let label = Rect::new(cx, ry, l.dlg_label, DLG_CTRL);
            let lw = l.dlg_label + 4;
            let ctrl = if Self::kind(it) == Kind::Action {
                Rect::new(cx, ry, cw, DLG_CTRL)
            } else {
                Rect::new(cx + lw, ry, cw - lw, DLG_CTRL)
            };
            rows.push(Row { it, label, ctl: self.ctl(it, ctrl.w), ctrl });
        }
        let h = col_y.into_iter().max().unwrap_or(y) - y + 6;
        let r = Rect::new(x, y, w, h);
        let close = Rect::new(r.x + r.w - 52, r.y + 6, 44, DLG_CTRL);
        let back = self.parent.map(|_| Rect::new(close.x - 88, close.y, 82, DLG_CTRL));
        Dlg { r, close, back, rows, heads, bar: None }
    }

    /// Font and box of the text of a menu button, and whether it fits: centred in the button while it fits, else
    /// centred left of the chevron of a button that opens a menu, else one size smaller (long labels at the large text
    /// sizes).
    pub(super) fn btn_text(&self, c: Rect, open: bool, t: &str) -> (&'static Font, Rect, bool) {
        let f = self.fonts();
        if f.mb.width(t) + if open { 40 } else { 8 } <= c.w {
            return (&f.mb, c, true);
        }
        let r = if open { Rect::new(c.x, c.y, c.w - 18, c.h) } else { c };
        let font = if f.mb.width(t) + 8 <= r.w { &f.mb } else { &f.sb };
        (font, r, font.width(t) + 8 <= r.w)
    }

    /// Value control parts: minus, value, plus.
    pub(super) fn value_parts(c: Rect) -> (Rect, Rect, Rect) {
        (Rect::new(c.x, c.y, 44, c.h), Rect::new(c.x + 48, c.y, c.w - 96, c.h), Rect::new(c.x + c.w - 44, c.y, 44, c.h))
    }

    pub(super) fn draw_menu(&self, cv: &mut Canvas) {
        if !self.menu_open {
            return;
        }
        if self.menu == MenuId::MeasAdd {
            self.draw_catalog(cv);
            return;
        }
        let Some(d) = self.dlg_layout() else { return };
        let f = self.fonts();
        cv.framed(d.r, 8, 2, ACCENT, 7, DLG_BG);
        let tcol = match self.menu_root() {
            MenuId::Channel(c) => CH_COLORS[c],
            _ => TEXT,
        };
        // long titles (storage directory) keep their end
        let mut title = self.menu_title(self.menu);
        let room = d.back.unwrap_or(d.close).x - d.r.x - 30;
        if self.menu == MenuId::Ref {
            while f.mb.width(&title) > room && title.chars().count() > 2 {
                let keep = title.chars().count() - 2;
                title = format!("{}…", title.chars().take(keep).collect::<String>());
            }
        } else {
            while f.mb.width(&title) > room && title.chars().count() > 2 {
                let mut c = title.chars();
                c.next();
                c.next();
                title = format!("\u{2026}{}", c.as_str());
            }
        }
        if self.menu == MenuId::MeasPair {
            self.draw_pair_header(cv, &d);
        } else {
            cv.text(&f.mb, d.r.x + 14, d.r.y + 31, &title, tcol);
        }
        btn(cv, d.close, BTN, BTN_BORDER);
        cv.text_in(&f.mb, d.close, 1, "\u{d7}", TEXT);
        if let Some(b) = d.back {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.sb, b, 1, "Back", TEXT);
        }
        self.draw_measurement_groups(cv);
        for (r, name) in &d.heads {
            cv.text_in(&f.sb, *r, 0, name, DIM);
        }
        if let Some((bar, _)) = d.bar {
            bar.draw(cv);
        }
        for (i, row) in d.rows.iter().enumerate() {
            if row.ctrl.w == 0 {
                continue; // scrolled out of a long picker
            }
            let focus = i == self.focus;
            let kind = Self::kind(row.it);
            if row.label.w > 0 && kind != Kind::Action {
                cv.text_in(&f.s, row.label, 0, self.item_label(row.it), if focus { ACCENT } else { LABEL });
            }
            let c = row.ctrl;
            match &row.ctl {
                Ctl::Seg(opts, sel) => {
                    let n = opts.len() as i32;
                    let sw = (c.w - (n - 1) * 4) / n;
                    for (k, o) in opts.iter().enumerate() {
                        let r = Rect::new(c.x + k as i32 * (sw + 4), c.y, sw, c.h);
                        if *sel == Some(k) {
                            cv.round(r, 5, ACCENT, 255);
                            fitted_text(cv, &[&f.mb, &f.m, &f.s], r, o, DARK_TEXT);
                        } else {
                            btn(cv, r, BTN, BTN_BORDER);
                            fitted_text(cv, &[&f.m, &f.s], r, o, TEXT);
                        }
                    }
                }
                Ctl::Text => {
                    let v = self.value(row.it);
                    cv.text_in(self.text_row_font(c.w, &v), c, 1, &v, TEXT)
                }
                Ctl::Value | Ctl::Pick => {
                    let (mi, mid, pl) = Self::value_parts(c);
                    btn(cv, mi, BTN, BTN_BORDER);
                    cv.text_in(&f.mb, mi, 1, "-", TEXT);
                    btn(cv, mid, BTN, if focus { ACCENT } else { BTN_BORDER });
                    cv.text_in(&f.mb, mid, 1, &self.value(row.it), TEXT);
                    if let Ctl::Pick = row.ctl {
                        // the value opens the list
                        chevron_down(cv, mid.x + mid.w - 20, mid.y + mid.h / 2 - 3, DIM);
                    }
                    btn(cv, pl, BTN, BTN_BORDER);
                    cv.text_in(&f.mb, pl, 1, "+", TEXT);
                }
                Ctl::Button => {
                    let picked = match (row.it, self.pick) {
                        (Item::PickOpt(k), Some(p)) => {
                            self.options(p.it).get(k).map(|o| (o.clone(), *o == self.value(p.it)))
                        }
                        _ => None,
                    };
                    // orange: added measurement, current option of a picker, active state of a start menu entry
                    let on = match row.it {
                        Item::MeasAdd(k) => self.m.meas.contains(&(k, self.sources.a)),
                        Item::MeasPair(k) => self.m.meas2.contains(&(k, self.sources.a, self.sources.b)),
                        Item::NavHist => self.history.pos.is_some(),
                        Item::SingleAct => self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run,
                        Item::RunStop => self.m.run == RunState::Run,
                        _ => false,
                    };
                    let added = on || picked.as_ref().is_some_and(|p| p.1);
                    let txt = match picked {
                        Some((o, _)) => o,
                        None if row.it == Item::QuickRun => format!("Quick \u{b7} {}", self.m.quick.short()),
                        None if matches!(row.it, Item::MeasRemove(_)) => {
                            if let Item::MeasRemove(index) = row.it {
                                format!("Remove {}", self.selected_measurement_label(index as usize))
                            } else {
                                unreachable!()
                            }
                        }
                        None if kind == Kind::Action => self.item_label(row.it).to_string(),
                        None => self.value(row.it),
                    };
                    let (tf, tr, _) = self.btn_text(c, matches!(row.it, Item::Open(_)), &txt);
                    if added {
                        cv.round(c, 5, ACCENT, 255);
                        cv.text_in(tf, tr, 1, &txt, DARK_TEXT);
                    } else {
                        btn(cv, c, BTN, if focus { ACCENT } else { BTN_BORDER });
                        cv.text_in(tf, tr, 1, &txt, TEXT);
                    }
                    if let Item::Open(_) = row.it {
                        chevron(cv, c.x + c.w - 14, c.y + c.h / 2, 1, DIM);
                    }
                }
            }
            if focus {
                if let Ctl::Seg(..) = row.ctl {
                    cv.round_outline(Rect::new(c.x - 3, c.y - 3, c.w + 6, c.h + 6), 7, ACCENT);
                }
            }
        }
    }

    /// Back in a sub menu: the menu it was opened from, with the focus it had.
    pub(super) fn menu_back(&mut self) {
        if let Some((menu, focus)) = self.menu_history.pop() {
            self.menu = menu;
            self.focus = focus;
            self.parent = self.menu_history.last().map(|entry| entry.0);
        } else {
            self.menu = self.parent.take().unwrap_or(MenuId::Main);
            self.focus = 0;
        }
    }

    /// Tap inside the menu dialog; false when (x, y) is outside it.
    pub(super) fn tap_menu(&mut self, x: i32, y: i32) -> bool {
        if self.menu_open && self.menu == MenuId::MeasAdd {
            return self.catalog_tap(x, y);
        }
        if self.measurement_group_tap(x, y) {
            return true;
        }
        let Some(d) = self.dlg_layout() else { return false };
        if !d.r.contains(x, y) {
            return false;
        }
        if d.close.contains(x, y) {
            self.close_menu();
            return true;
        }
        if let Some(b) = d.back {
            if b.contains(x, y) {
                if self.menu == MenuId::Pick {
                    self.close_picker();
                } else {
                    self.menu_back();
                }
                return true;
            }
        }
        if self.menu == MenuId::MeasPair {
            for (item, rect) in self.pair_header_sources(&d) {
                if rect.contains(x, y) {
                    self.open_picker(item);
                    return true;
                }
            }
        }
        for (i, row) in d.rows.iter().enumerate() {
            // the label of a picker row opens the list as well
            let on_label = matches!(row.ctl, Ctl::Pick) && row.label.contains(x, y);
            if !row.ctrl.contains(x, y) && !on_label {
                continue;
            }
            self.focus = i;
            if on_label {
                self.open_picker(row.it);
                break;
            }
            match &row.ctl {
                Ctl::Text => {}
                Ctl::Seg(opts, _) => {
                    let n = opts.len() as i32;
                    let k = ((x - row.ctrl.x) * n / row.ctrl.w).clamp(0, n - 1) as usize;
                    self.select_option(row.it, &opts[k]);
                }
                Ctl::Value | Ctl::Pick => {
                    let (mi, _, pl) = Self::value_parts(row.ctrl);
                    if mi.contains(x, y) {
                        self.adjust(row.it, -1);
                    } else if pl.contains(x, y) {
                        self.adjust(row.it, 1);
                    } else if matches!(row.ctl, Ctl::Value) && self.numpad_unit(row.it).is_some() {
                        // the number itself: type it (issue #22)
                        self.open_numpad(row.it);
                    } else if let Ctl::Pick = row.ctl {
                        self.open_picker(row.it);
                    }
                }
                Ctl::Button => self.activate(row.it),
            }
            break;
        }
        true
    }

    pub(super) fn close_menu(&mut self) {
        self.menu_open = false;
        self.menu = MenuId::Main;
        self.parent = None;
        self.menu_history.clear();
        self.pick = None;
    }

    /// Power dialog in the middle of the screen: its box, and the buttons Power off, Restart, Cancel.
    pub(super) fn pwr_layout(&self) -> (Rect, [Rect; 3]) {
        let b = self.scr.rect().center(500, 200);
        let btn = |dx: i32| Rect::new(b.x + 250 + dx, b.y + 90, 140, 64);
        (b, [btn(-230), btn(-70), btn(90)])
    }

    pub(super) fn draw_overlay(&self, cv: &mut Canvas) {
        self.draw_load_slot(cv);
        let f = self.fonts();
        self.draw_osk(cv);
        if self.power.open {
            cv.blend(self.scr.rect(), rgb(0, 0, 0), 160);
            let (r, [off, restart, cancel]) = self.pwr_layout();
            cv.framed(r, 8, 2, ACCENT, 7, DLG_BG);
            cv.text_in(&f.lb, Rect::new(r.x, r.y + 18, r.w, 28), 1, "Power", ACCENT);
            cv.text_in(&f.m, Rect::new(r.x, r.y + 52, r.w, 24), 1, "Power key again or tap outside to cancel", DIM);
            for (b, t, c) in [(off, "Power off", RED), (restart, "Restart", ACCENT), (cancel, "Cancel", TEXT)] {
                btn(cv, b, mix(DLG_BG, c, 40), c);
                cv.text_in(&f.mb, b, 1, t, c);
            }
        }
        if self.notices.info.is_some() && self.sysinfo.rows.is_some() {
            self.draw_sysinfo(cv);
        } else if let Some(lines) = &self.notices.info {
            let scr = self.scr.rect();
            cv.blend(scr, rgb(0, 0, 0), 140);
            // taller for long texts (About with experimental limits)
            let h = (40 + lines.len() as i32 * 30).clamp(260, scr.h - 20);
            // wider for long lines at the larger text sizes
            let w = lines.iter().map(|l| f.m.width(l) + 40).max().unwrap_or(0).clamp(480, scr.w - 20);
            let r = scr.center(w, h);
            cv.framed(r, 8, 2, ACCENT, 7, DLG_BG);
            for (i, l) in lines.iter().enumerate() {
                let fnt = if i == 0 { &f.lb } else { &f.m };
                cv.text_in(
                    fnt,
                    Rect::new(r.x, r.y + 20 + i as i32 * 30, r.w, 26),
                    1,
                    l,
                    if i == 0 { ACCENT } else { TEXT },
                );
            }
        }
    }
}

/// `s` centred in `r` in the first of `fonts` that fits; a text too long for the last one goes on two lines of it.
fn fitted_text(cv: &mut Canvas, fonts: &[&'static Font], r: Rect, s: &str, c: Rgb) {
    let room = r.w - 12;
    let f = fit_font(fonts, room, s);
    if f.width(s) <= room {
        return cv.text_in(f, r, 1, s, c);
    }
    let words: Vec<&str> = s.split(' ').collect();
    let first = (1..words.len()).rev().find(|&k| f.width(&words[..k].join(" ")) <= room).unwrap_or(1);
    let (a, b) = (words[..first].join(" "), words[first..].join(" "));
    cv.set_clip(r);
    cv.text_in(f, Rect::new(r.x, r.y, r.w, r.h / 2 + 1), 1, &a, c);
    cv.text_in(f, Rect::new(r.x, r.y + r.h / 2 - 1, r.w, r.h / 2 + 1), 1, &b, c);
    cv.no_clip();
}
