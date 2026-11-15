// SPDX-License-Identifier: GPL-2.0-or-later
//! Studio > View > Offline waveform analysis: a view that runs on a record session.
//! Offline shows a loaded file with the zoom overview as its navigator, the references in lanes and the measurements
//! docked.
use super::studio::{chip, StudioView};
use super::*;

/// How many of the loaded references the Offline view stacks: more would be lanes too thin to read.
const OFFLINE_LANES: usize = 4;

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
        assert!(labels.contains(&"Offline waveform analysis"));
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
        // Reset layout from the Offline view
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
            assert!(d.studio_panel_rect().is_none() && d.studio_offline_empty().is_none());
        }
    }

}
