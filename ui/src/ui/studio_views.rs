// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio > View > Offline waveform analysis and SIM: the two views that run on a record session.
//! Offline shows a loaded file with the zoom overview as its navigator, the references in lanes and the measurements
//! docked. SIM docks a panel at the right of the lanes; its rows call the items of the SIM menu (`adjust`, `activate`,
//! `select_option`), so nothing about generating is decided here.
use super::studio::{chip, icon_arrow, icon_play, icon_stop, StudioDrag, StudioView};
use super::*;
use oos_dsp::serial;

/// How many of the loaded references the Offline view stacks: more would be lanes too thin to read.
const OFFLINE_LANES: usize = 4;
/// Height of a row of the SIM list, and the width of the minus and plus chips of a knob.
const SIM_ROW: i32 = 44;
const STEP_W: i32 = 36;

/// The parts of the SIM panel.
pub(super) struct SimGeo {
    pub p: Rect,
    pub close: Rect,
    pub presets: Rect,
    pub knobs: Rect,
    pub list: Rect,
    pub page: usize,
    pub up: Rect,
    pub down: Rect,
    pub play: Rect,
}

impl Ui {
    // ------------------------------------------------------------------------------------------------ Offline

    /// Offline waveform analysis starts: a record session on the loaded references, or an empty one that offers Load file.
    pub(super) fn studio_enter_offline(&mut self) {
        if !self.record_mode() {
            self.remember_live();
            self.set_offline(true);
        }
        let loaded: Vec<usize> = (0..NREF).filter(|&i| self.refs[i].is_some()).collect();
        if let Some(&first) = loaded.iter().find(|&&i| i == self.offline_ref).or(loaded.first()) {
            self.present_record(first);
            let shown: Vec<usize> = std::iter::once(first).chain(loaded.iter().copied().filter(|&i| i != first)).take(OFFLINE_LANES).collect();
            for &i in &loaded {
                if let Some(r) = self.refs[i].as_mut() { r.on = shown.contains(&i); }
            }
            // measurements of the channels would read nothing here
            self.m.meas.retain(|&(_, id)| matches!(id, WaveId::Ref(_)));
            if self.m.meas.is_empty() {
                self.m.meas = [Meas::Vpp, Meas::Freq, Meas::Vrms, Meas::Rise].into_iter().map(|k| (k, WaveId::Ref(first))).collect();
                self.m.meas2.clear();
                self.m.cat.clear();
                self.clear_measurement_statistics();
                self.meas_src = WaveId::Ref(first);
                self.ensure_meas_own();
            }
        }
        self.studio_sync_views();
        self.trace_mode(true);
    }

    /// The card of an empty Offline view and its Load file button.
    pub(super) fn studio_offline_empty(&self) -> Option<(Rect, Rect)> {
        if self.m.layout != Layout::Studio || self.studio.view != StudioView::Offline || self.view_on() { return None; }
        if !self.record_mode() || self.refs.iter().any(Option::is_some) { return None; }
        let g = self.grid;
        let card = Rect::new(g.x + (g.w - 380) / 2, g.y + (g.h - 190) / 2, 380, 190);
        Some((card, Rect::new(card.x + (card.w - 180) / 2, card.bottom() - 64, 180, 42)))
    }

    /// The span and size of the record under the navigator, for its tag.
    fn studio_record_tag(&self) -> Option<String> {
        let r = self.refs.get(self.active_reference())?.as_ref().filter(|r| r.v.len() >= 2 && r.dt > 0.0)?;
        Some(format!("Whole record \u{b7} {} \u{b7} {}", si_digits(r.v.len() as f64 * r.dt, "s", 3), si_digits(r.v.len() as f64, "pts", 3)))
    }

    pub(super) fn draw_studio_offline(&self, cv: &mut Canvas) {
        if self.m.layout != Layout::Studio || self.studio.view != StudioView::Offline || self.view_on() { return; }
        let f = self.fonts();
        if let Some((card, load)) = self.studio_offline_empty() {
            cv.round(card, 10, BTN_BORDER, 255);
            cv.round(card.inset(1), 9, PANEL_BG, 255);
            cv.text_in(&f.lb, Rect::new(card.x, card.y + 22, card.w, 30), 1, "No file loaded", TEXT);
            cv.text_in(&f.m, Rect::new(card.x + 16, card.y + 58, card.w - 32, 24), 1, "Load a waveform file to analyse it here.", DIM);
            cv.text_in(&f.s, Rect::new(card.x + 16, card.y + 84, card.w - 32, 20), 1, "Live acquisition comes back with Live.", DIM);
            chip(cv, load, mix(BTN, ACCENT, 40), ACCENT);
            cv.text_in(&f.mb, load, 1, "Load file", TEXT);
            return;
        }
        // the navigator: the overview of the whole record carries its span
        if self.m.zoom && !self.xy && self.refs.iter().any(Option::is_some) {
            if let Some(text) = self.studio_record_tag() {
                let (o, _) = Self::split_zoom(self.grid);
                let w = f.sb.width(&text) + 16;
                // under the lane tags of the overview, clear of its buttons
                let r = Rect::new(o.x + (o.w - w) / 2, o.bottom() - 26, w, 20);
                cv.round(r, 5, BTN_BORDER, 255);
                cv.round(r.inset(1), 4, PANEL_BG, 255);
                cv.text_in(&f.sb, r, 1, &text, TEXT);
            }
        }
    }

    /// A tap on the Load file button of the empty view: the load flow that the Load file button of the top bar starts.
    pub(super) fn studio_offline_tap(&mut self, x: i32, y: i32) -> bool {
        let Some((card, load)) = self.studio_offline_empty() else { return false };
        if load.contains(x, y) { self.request_waveform(); }
        card.contains(x, y)
    }

    // ------------------------------------------------------------------------------------------------ SIM

    pub(super) fn studio_enter_sim(&mut self) {
        self.studio.sim_knobs = false;
        self.trace_mode(true);
    }

    /// The knobs the panel lists: the values and choices of the SIM menu that are not the preset, the show switch
    /// or the advanced page.
    pub(super) fn studio_sim_knobs(&self) -> Vec<Item> {
        self.sim_items().into_iter()
            .filter(|&it| matches!(Self::sim_kind(it), Some(Kind::Value | Kind::Choice)) && !matches!(it, Item::SimPreset | Item::SimShow))
            .collect()
    }

    fn studio_sim_total(&self) -> usize {
        if self.studio.sim_knobs { self.studio_sim_knobs().len() } else { serial::LANES.len() }
    }

    /// The SIM is generating or playing: the button says Stop.
    pub(super) fn studio_sim_running(&self) -> bool {
        self.sim_generating() || (self.session_is_sim && self.m.run == RunState::Run)
    }

    pub(super) fn studio_sim_geo(&self) -> Option<SimGeo> {
        let p = self.studio_panel_rect().filter(|_| self.m.layout == Layout::Studio && self.studio.view == StudioView::Sim)?;
        let tab_w = (p.w - 16) / 2;
        let list_y = p.y + 102;
        let play = Rect::new(p.x + 6, p.bottom() - 46, p.w - 12, 38);
        let nav_y = play.y - 36;
        let page = (((nav_y - 4 - list_y) / SIM_ROW).max(1)) as usize;
        Some(SimGeo {
            p,
            close: Rect::new(p.right() - 42, p.y + 5, 36, 30),
            presets: Rect::new(p.x + 8, p.y + 62, tab_w, 32),
            knobs: Rect::new(p.x + 8 + tab_w, p.y + 62, tab_w, 32),
            list: Rect::new(p.x + 4, list_y, p.w - 8, page as i32 * SIM_ROW),
            page,
            up: Rect::new(p.right() - 82, nav_y, 36, 30),
            down: Rect::new(p.right() - 42, nav_y, 36, 30),
            play,
        })
    }

    /// The list: its rectangle, the row height, the rows of a page and the rows in all (`studio_touch` scrolls it).
    pub(super) fn studio_sim_list(&self) -> Option<(Rect, i32, usize, usize)> {
        self.studio_sim_geo().map(|g| (g.list, SIM_ROW, g.page, self.studio_sim_total()))
    }

    pub(super) fn draw_studio_sim(&self, cv: &mut Canvas) {
        let Some(g) = self.studio_sim_geo() else { return };
        let f = self.fonts();
        let p = g.p;
        cv.round(p, 8, CARD_BORDER, 255);
        cv.round(p.inset(1), 7, PANEL_BG, 255);
        cv.text(&f.lb, p.x + 14, p.y + 27, "SIM", TEXT);
        chip(cv, g.close, BTN, BTN_BORDER);
        let (cx, cy) = (g.close.x + g.close.w / 2, g.close.y + g.close.h / 2);
        cv.line(cx - 5, cy - 5, cx + 5, cy + 5, TEXT);
        cv.line(cx - 5, cy + 5, cx + 5, cy - 5, TEXT);
        let state = if self.session_is_sim { "on the lanes" } else if self.sim_generating() { "generating" } else { "not playing" };
        cv.set_clip(Rect::new(p.x + 14, p.y + 34, p.w - 70, 28));
        cv.text(&f.s, p.x + 14, p.y + 56, &format!("{} \u{b7} {} \u{b7} {}", self.sim_preset_name(), si_short(self.m.sim_rate, "b/s"), state), DIM);
        cv.no_clip();
        for (r, label, on) in [(g.presets, "Presets", !self.studio.sim_knobs), (g.knobs, "Knobs", self.studio.sim_knobs)] {
            chip(cv, r, if on { mix(BTN, ACCENT, 40) } else { BTN }, if on { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.m, r, 1, label, TEXT);
        }
        let total = self.studio_sim_total();
        let first = self.studio_first(total, g.page);
        cv.set_clip(g.list);
        for k in 0..g.page {
            let i = first + k;
            if i >= total { break; }
            let r = Rect::new(g.list.x, g.list.y + k as i32 * SIM_ROW, g.list.w, SIM_ROW);
            if self.studio.sim_knobs { self.draw_sim_knob(cv, r, self.studio_sim_knobs()[i]); } else { self.draw_sim_preset(cv, r, i); }
            cv.hline(r.x + 6, r.right() - 7, r.bottom() - 1, LINE);
        }
        cv.no_clip();
        Self::draw_scrollbar(cv, g.list, first, g.page, total);
        let end = (first + g.page).min(total);
        cv.text(&f.s, p.x + 14, g.up.y + 20, &format!("{}-{} of {}", first + 1, end, total), DIM);
        for (r, up) in [(g.up, true), (g.down, false)] {
            chip(cv, r, BTN, BTN_BORDER);
            icon_arrow(cv, r.x + r.w / 2, r.y + r.h / 2, up, TEXT);
        }
        let running = self.studio_sim_running();
        chip(cv, g.play, if running { mix(BTN, GREEN, 60) } else { BTN }, if running { GREEN } else { BTN_BORDER });
        if running { icon_stop(cv, g.play.x + g.play.w / 2 - 38, g.play.y + g.play.h / 2, TEXT); } else { icon_play(cv, g.play.x + g.play.w / 2 - 38, g.play.y + g.play.h / 2, TEXT); }
        cv.text_in(&f.mb, Rect::new(g.play.x + g.play.w / 2 - 18, g.play.y, 80, g.play.h), 0, if running { "Stop" } else { "Play" }, TEXT);
    }

    fn draw_sim_preset(&self, cv: &mut Canvas, r: Rect, i: usize) {
        let f = self.fonts();
        let lane = &serial::LANES[i];
        let on = i == self.m.sim_preset;
        if on {
            cv.round(r.inset(2), 6, ACCENT, 255);
            cv.round(r.inset(3), 5, mix(PANEL_BG, ACCENT, 28), 255);
        }
        cv.text(&f.m, r.x + 14, r.y + 19, lane.name, TEXT);
        cv.text(&f.s, r.x + 14, r.y + 37, &si_short(lane.bits_per_s, "b/s"), LABEL);
        if on {
            let (kx, ky) = (r.right() - 28, r.y + r.h / 2);
            cv.line(kx, ky, kx + 4, ky + 4, ACCENT);
            cv.line(kx + 4, ky + 4, kx + 11, ky - 5, ACCENT);
            cv.line(kx, ky + 1, kx + 4, ky + 5, ACCENT);
            cv.line(kx + 4, ky + 5, kx + 11, ky - 4, ACCENT);
        }
    }

    /// The minus and plus chips of a knob row.
    fn sim_steps(r: Rect) -> (Rect, Rect) {
        (Rect::new(r.right() - 2 * STEP_W - 10, r.y + 7, STEP_W, 30), Rect::new(r.right() - STEP_W - 6, r.y + 7, STEP_W, 30))
    }

    fn draw_sim_knob(&self, cv: &mut Canvas, r: Rect, it: Item) {
        let f = self.fonts();
        let (minus, plus) = Self::sim_steps(r);
        cv.set_clip(Rect::new(r.x, r.y, (minus.x - r.x - 4).max(1), r.h));
        cv.text(&f.s, r.x + 12, r.y + 17, Self::sim_label(it).unwrap_or(""), LABEL);
        cv.text(&f.mb, r.x + 12, r.y + 37, &self.sim_value(it).unwrap_or_default(), TEXT);
        cv.no_clip();
        for (b, is_plus) in [(minus, false), (plus, true)] {
            chip(cv, b, BTN, BTN_BORDER);
            cv.hline(b.x + b.w / 2 - 6, b.x + b.w / 2 + 6, b.y + b.h / 2, TEXT);
            if is_plus { cv.vline(b.x + b.w / 2, b.y + b.h / 2 - 6, b.y + b.h / 2 + 6, TEXT); }
        }
    }

    /// A tap on the SIM panel. False: it is not on it.
    pub(super) fn studio_sim_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(g) = self.studio_sim_geo() else { return false };
        if !g.p.contains(x, y) { return false; }
        let total = self.studio_sim_total();
        if g.close.contains(x, y) {
            self.studio_set_view(StudioView::Waveform);
        } else if g.presets.contains(x, y) || g.knobs.contains(x, y) {
            self.studio.sim_knobs = g.knobs.contains(x, y);
            self.studio.panel.first = 0;
        } else if g.up.contains(x, y) || g.down.contains(x, y) {
            self.studio_page(total, g.page, g.down.contains(x, y));
        } else if g.play.contains(x, y) {
            self.activate(Item::SimPlay);
        } else if g.list.contains(x, y) {
            let i = self.studio_first(total, g.page) + ((y - g.list.y) / SIM_ROW) as usize;
            if i < total {
                if self.studio.sim_knobs {
                    let r = Rect::new(g.list.x, g.list.y + (i - self.studio_first(total, g.page)) as i32 * SIM_ROW, g.list.w, SIM_ROW);
                    let (minus, plus) = Self::sim_steps(r);
                    let it = self.studio_sim_knobs()[i];
                    if minus.contains(x, y) { self.adjust(it, -1); } else if plus.contains(x, y) { self.adjust(it, 1); }
                } else {
                    self.select_option(Item::SimPreset, serial::LANES[i].name);
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn studio() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.m.layout = Layout::Studio;
        u.m.ch[1].on = true;
        u.studio.enter = true;
        for _ in 0..8 { u.step(0.05); }
        u
    }

    fn tap_in(u: &mut Ui, r: Rect) { u.tap(r.x + r.w / 2, r.y + r.h / 2); }

    fn shot(u: &mut Ui, name: &str) {
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        for _ in 0..6 { u.step(0.05); }
        u.render(&mut cv);
        if let Some(dir) = std::env::var_os("OOS_SHOTS") {
            std::fs::write(std::path::Path::new(&dir).join(format!("studio-{name}.png")), crate::store::png_of(&cv)).unwrap();
        }
    }

    /// Chooses a view in the View menu with taps, as the user does.
    fn choose(u: &mut Ui, view: StudioView) {
        let chip = u.studio_top().view;
        tap_in(u, chip);
        let i = super::super::studio::STUDIO_VIEWS.iter().position(|(v, _)| *v == view).unwrap();
        let row = u.studio_view_row(i);
        tap_in(u, row);
    }

    /// A square wave of `n` samples, as a record file would have it.
    fn load(u: &mut Ui, name: &str, n: usize, slot: usize) {
        let v: Vec<f32> = (0..n).map(|i| if (i / 50) % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let traces = vec![crate::store::WaveTrace { label: name.into(), t0: 0.0, dt: 1e-9, v }];
        u.commit_waveform_traces_at(traces, name, slot).unwrap();
    }

    #[test]
    fn the_view_menu_lists_the_record_views() {
        let mut u = studio();
        let t = u.studio_top().view;
        tap_in(&mut u, t);
        let p = u.studio_view_rect();
        assert!(p.x >= 0 && p.right() <= W && p.bottom() < u.frame().bottom.y, "the menu fits the screen");
        let labels: Vec<&str> = super::super::studio::STUDIO_VIEWS.iter().map(|v| v.1).collect();
        assert!(labels.contains(&"Offline waveform analysis") && labels.contains(&"SIM"));
        for i in 0..=super::super::studio::VIEW_RESET {
            let r = u.studio_view_row(i);
            assert!(p.contains(r.x + 2, r.y + 2) && p.contains(r.right() - 2, r.bottom() - 2), "row {i} inside the menu");
            for j in 0..i { assert!(!overlaps(r, u.studio_view_row(j)), "rows {i} and {j} apart"); }
        }
        shot(&mut u, "19-view-menu-views");
    }

    fn overlaps(a: Rect, b: Rect) -> bool { a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom() }

    #[test]
    fn offline_without_a_file_offers_load_file_in_record_mode_and_live_returns() {
        let mut u = studio();
        assert!(!u.record_mode());
        choose(&mut u, StudioView::Offline);
        assert_eq!(u.studio.view, StudioView::Offline);
        assert!(u.record_mode() && u.offline, "the view switches to record mode");
        for _ in 0..3 { u.step(0.05); }
        let (card, load) = u.studio_offline_empty().expect("the empty card");
        let g = u.grid;
        assert!(g.contains(card.x, card.y) && g.contains(card.right() - 1, card.bottom() - 1));
        assert!(card.contains(load.x, load.y) && card.contains(load.right() - 1, load.bottom() - 1));
        assert_eq!(u.studio_acq_caption(), "No file loaded");
        shot(&mut u, "20-offline-empty");
        // Load file starts the existing load flow
        assert!(!u.load_slot_ask);
        tap_in(&mut u, load);
        assert!(u.load_slot_ask, "the destination question of the load flow");
        u.cancel_load_slot();
        // the badge is the way back
        let badge = u.studio_top().badge;
        assert_eq!(u.studio_badge_label(), "Live");
        tap_in(&mut u, badge);
        assert!(!u.record_mode() && !u.offline);
        assert_eq!(u.studio.view, StudioView::Waveform, "Live ends the view");
        assert!(u.studio_offline_empty().is_none());
    }

    #[test]
    fn offline_with_files_has_a_navigator_lanes_docked_measurements_and_a_name_chip() {
        let mut u = studio();
        load(&mut u, "bus.csv", 4000, 0);
        load(&mut u, "clk.csv", 4000, 1);
        // loaded onto the live screen: the references are drawn with the channels until the view is chosen
        assert!(!u.record_mode());
        choose(&mut u, StudioView::Offline);
        for _ in 0..4 { u.step(0.05); }
        assert!(u.record_mode() && u.m.ch.iter().all(|c| !c.on), "the channels are off in the record session");
        assert!(u.m.zoom, "the zoom overview is the navigator");
        assert!(u.studio_offline_empty().is_none());
        assert!(u.studio_acq_caption().starts_with("bus.csv"), "{}", u.studio_acq_caption());
        // the references are lanes
        assert_eq!(u.wins.len(), 2, "one lane per reference");
        // the measurements are docked and the plot has given them the width
        let panel = u.studio_panel_rect().expect("docked measurements");
        assert!(panel.x >= u.grid.right() && panel.right() <= W, "the panel is beside the plot");
        assert_eq!(u.studio_panel_geo().map(|g| g.p), Some(panel));
        assert!(u.m.meas.iter().all(|&(_, id)| matches!(id, WaveId::Ref(_))), "the measurements read the records");
        // the tag over the navigator sits inside the overview
        let (o, _) = Ui::split_zoom(u.grid);
        assert!(o.w > 0 && u.studio_record_tag().unwrap().contains("Whole record"));
        shot(&mut u, "21-offline-analysis");
        // Live in the badge returns to the live acquisition the existing way
        let badge = u.studio_top().badge;
        tap_in(&mut u, badge);
        assert!(!u.record_mode() && u.m.ch[0].on && u.studio.view == StudioView::Waveform);
    }

    #[test]
    fn leaving_the_offline_view_returns_to_live_and_reset_layout_does_too() {
        let mut u = studio();
        load(&mut u, "bus.csv", 3000, 0);
        choose(&mut u, StudioView::Offline);
        assert!(u.record_mode());
        choose(&mut u, StudioView::Docked);
        assert_eq!(u.studio.view, StudioView::Docked);
        assert!(!u.record_mode(), "another view is live again");
        // Reset layout from the SIM view
        choose(&mut u, StudioView::Offline);
        assert!(u.record_mode());
        let chip = u.studio_top().view;
        tap_in(&mut u, chip);
        let row = u.studio_view_row(super::super::studio::VIEW_RESET);
        tap_in(&mut u, row);
        assert_eq!(u.studio.view, StudioView::Waveform);
        assert!(!u.record_mode());
    }

    #[test]
    fn offline_view_ends_when_the_session_ends_elsewhere() {
        let mut u = studio();
        load(&mut u, "bus.csv", 3000, 0);
        choose(&mut u, StudioView::Offline);
        u.return_to_live();
        assert_eq!(u.studio.view, StudioView::Waveform);
        // and the Default and Minimal layouts do not know the view
        for layout in [Layout::Default, Layout::Minimal] {
            let mut d = Ui::new(Source::Demo { t: 0.0 });
            d.m.layout = layout;
            for _ in 0..4 { d.step(0.05); }
            assert!(d.studio_panel_rect().is_none() && d.studio_offline_empty().is_none() && d.studio_sim_geo().is_none());
        }
    }

    fn sim_ui() -> Ui {
        let mut u = studio();
        u.store_dir = std::env::temp_dir().join("oos-studio-views-sim");
        choose(&mut u, StudioView::Sim);
        u
    }

    #[test]
    fn sim_view_docks_a_panel_with_presets_knobs_and_play() {
        let mut u = sim_ui();
        assert_eq!(u.studio.view, StudioView::Sim);
        assert!(!u.record_mode(), "nothing plays until Play");
        let g = u.studio_sim_geo().expect("the SIM panel");
        assert!(g.p.x >= u.grid.right() && g.p.right() <= W && g.p.bottom() <= u.frame().bottom.y, "beside the plot, above the bar");
        assert_eq!(u.studio_panel_rect(), Some(g.p));
        for r in [g.close, g.presets, g.knobs, g.up, g.down, g.play, g.list] {
            assert!(g.p.contains(r.x, r.y) && g.p.contains(r.right() - 1, r.bottom() - 1), "{r:?} inside the panel");
        }
        assert!(!overlaps(g.list, g.up) && !overlaps(g.list, g.play) && !overlaps(g.up, g.play) && !overlaps(g.presets, g.list));
        shot(&mut u, "22-sim-presets");
        // a preset row chooses the preset the way the SIM menu does
        let want = 3.min(serial::LANES.len() - 1);
        let first = u.studio_first(serial::LANES.len(), g.page);
        let row = Rect::new(g.list.x, g.list.y + (want - first) as i32 * SIM_ROW, g.list.w, SIM_ROW);
        tap_in(&mut u, row);
        assert_eq!(u.m.sim_preset, want);
        assert_eq!(u.sim_preset_name(), serial::LANES[want].name);
        // the Knobs tab lists the knobs of sim_items() and steps them with the same adjust
        tap_in(&mut u, g.knobs);
        assert!(u.studio.sim_knobs);
        let knobs = u.studio_sim_knobs();
        assert!(knobs.contains(&Item::SimRate) && knobs.contains(&Item::SimNoise));
        assert!(!knobs.contains(&Item::SimPreset) && !knobs.contains(&Item::SimPlay) && !knobs.contains(&Item::SimDefaults));
        let i = knobs.iter().position(|&k| k == Item::SimRate).unwrap();
        let r = Rect::new(g.list.x, g.list.y + i as i32 * SIM_ROW, g.list.w, SIM_ROW);
        let (minus, plus) = Ui::sim_steps(r);
        let rate = u.m.sim_rate;
        tap_in(&mut u, plus);
        let up = u.m.sim_rate;
        assert!(up > rate, "plus raises the rate");
        tap_in(&mut u, minus);
        assert!(u.m.sim_rate < up, "minus lowers it");
        shot(&mut u, "23-sim-knobs");
        // the pager moves the list
        let total = u.studio_sim_knobs().len();
        if total > g.page {
            tap_in(&mut u, g.down);
            assert!(u.studio.panel.first > 0);
            tap_in(&mut u, g.up);
            assert_eq!(u.studio.panel.first, 0);
        }
    }

    #[test]
    fn sim_play_shows_the_simulated_source_in_a_lane_and_stop_and_close_end_it() {
        let mut u = sim_ui();
        let g = u.studio_sim_geo().unwrap();
        assert!(!u.studio_sim_running());
        tap_in(&mut u, g.play);
        u.wait_sim_generation();
        for _ in 0..8 { u.step(0.05); }
        assert!(u.session_is_sim && u.record_mode(), "Play runs the same action as the SIM menu");
        assert_eq!(u.studio_badge_label(), "SIM");
        assert!(u.refs[u.m.sim_slot].as_ref().is_some_and(|r| r.on), "the simulated source is on");
        assert_eq!(u.wins.len(), 1, "a lane for it");
        assert!(u.studio_sim_running());
        shot(&mut u, "24-sim-playing");
        // Stop stops the run, the session stays
        let g = u.studio_sim_geo().unwrap();
        tap_in(&mut u, g.play);
        assert!(!u.studio_sim_running() && u.session_is_sim && u.m.run == RunState::Stop);
        // the knobs regenerate through the same path as the menu
        tap_in(&mut u, g.knobs);
        // closing the panel leaves the view and returns to live
        tap_in(&mut u, g.close);
        assert_eq!(u.studio.view, StudioView::Waveform);
        assert!(!u.session_is_sim && !u.record_mode());
    }

    #[test]
    fn sim_list_scrolls_with_the_finger_and_the_panel_eats_taps() {
        let mut u = sim_ui();
        let g = u.studio_sim_geo().unwrap();
        let total = serial::LANES.len();
        assert!(total > g.page, "the preset list is longer than a page");
        let (x, y) = (g.list.x + 30, g.list.y + 120);
        let chosen = u.m.sim_preset;
        u.touch_ev(Phase::Down, x, y);
        u.touch_ev(Phase::Move, x, y - 3 * SIM_ROW);
        u.touch_ev(Phase::Up, x, y - 3 * SIM_ROW);
        assert_eq!(u.studio.panel.first, 3, "dragging up three rows moves the list three rows");
        assert_eq!(u.m.sim_preset, chosen, "a drag chooses nothing");
        // a tap outside the panel is the plot's
        assert!(!u.studio_sim_tap(g.p.x - 10, g.p.y + 40));
    }
}
