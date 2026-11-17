// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Two ways to look at a loaded reference. Take over fills the screen with one reference and hides
//! the other channels. Offline keeps several references on screen together: they play, and they are
//! measured as a set (table, summary, plots, histogram). Neither mode changes the live scope.

use super::*;

/// Render cost depends on the plot width, never on the retained native depth.
fn plot_samples(v: &[f32]) -> Vec<f32> {
    v.iter().step_by(v.len().div_ceil(200).max(1)).copied().filter(|x| x.is_finite()).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RefLook {
    Live,
    TakeOver,
    Waves,
}

/// Settings that belong to one added measurement. Unset fields follow the shared measure menu.
#[derive(Clone, Copy, Debug)]
pub(super) struct MeasOwn {
    pub low: f64,
    pub high: f64,
    pub idle: f64,
    pub side: oos_dsp::measure::Outside,
    pub n: u32,
    pub data: bool,
    pub refs_set: bool,
    pub idle_set: bool,
    pub side_set: bool,
    pub n_set: bool,
    pub data_set: bool,
    /// The per-row settings of the model (levels, edges, counting, pairing): None follows the shared ones.
    pub cfg: crate::model::RowCfg,
}

impl MeasOwn {
    pub(super) fn shared(m: &Model) -> Self {
        Self {
            low: m.meas_lpct,
            high: m.meas_hpct,
            idle: m.meas_idle,
            side: m.meas_out,
            n: m.meas_n,
            data: m.meas_data,
            refs_set: false,
            idle_set: false,
            side_set: false,
            n_set: false,
            data_set: false,
            cfg: crate::model::RowCfg::default(),
        }
    }
}

impl Ui {
    pub(super) fn set_look(&mut self, look: RefLook) {
        if self.look == look {
            return;
        }
        self.apply_look(look);
        self.toast(match look {
            RefLook::Live => "With live",
            RefLook::TakeOver => "Take over",
            RefLook::Waves => "Offline waveforms",
        });
    }

    pub(super) fn apply_look(&mut self, look: RefLook) {
        match look {
            // the live set-up from before the session comes back, as with the top bar's Live
            RefLook::Live => {
                self.leave_session();
                self.relayout();
            }
            RefLook::TakeOver => {
                if !self.offline {
                    self.remember_live();
                    self.set_offline(true);
                }
                self.look = RefLook::TakeOver;
                let slot = self.active_reference();
                self.present_record(slot);
                self.solo_reference(slot);
            }
            RefLook::Waves => {
                if !self.offline {
                    self.remember_live();
                    self.set_offline(true);
                }
                self.look = RefLook::Waves;
                self.show_loaded_references();
                self.measure_refs();
            }
        }
    }

    /// Only `slot` stays on the screen.
    pub(super) fn solo_reference(&mut self, slot: usize) {
        let slot = slot.min(NREF - 1);
        for (i, slot_ref) in self.refs.iter_mut().enumerate() {
            if let Some(r) = slot_ref {
                r.on = i == slot;
            }
        }
        self.offline_ref = slot;
    }

    /// Every loaded reference with samples stays on screen.
    pub(super) fn show_loaded_references(&mut self) {
        for r in self.refs.iter_mut().flatten() {
            if r.v.len() >= 2 && r.dt > 0.0 {
                r.on = true;
            }
        }
    }

    pub(super) fn waves_on(&self) -> bool {
        self.look == RefLook::Waves && !self.view_on() && !self.xy_over() && self.wins.len() <= 1
    }

    pub(super) fn waves_board_rect(&self) -> Option<Rect> {
        if !self.waves_on() {
            return None;
        }
        let g = self.grid;
        if g.w < 200 || g.h < 80 {
            return None;
        }
        let w = (g.w * 42 / 100).clamp(260, 440);
        Some(Rect::new(g.x + g.w - w, g.y, w, g.h))
    }

    pub(super) fn waves_plot(&self, area: Rect) -> Rect {
        match self.waves_board_rect() {
            Some(b) if area.x < b.x => Rect::new(area.x, area.y, (b.x - 4 - area.x).max(40), area.h),
            _ => area,
        }
    }

    /// Visible references, in slot order.
    pub(super) fn wave_slots(&self) -> Vec<usize> {
        (0..NREF)
            .filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on && r.v.len() >= 2 && r.dt > 0.0))
            .collect()
    }

    /// One added measurement, with shared cached analysis for untouched settings.
    pub(super) fn meas_number(&self, id: WaveId, k: Meas) -> f64 {
        self.measurement_with_own(id, k, id)
    }

    /// A set cell inherits the first added row of this kind unless it has its own row.
    pub(super) fn set_cell(&self, kind: Meas, slot: usize) -> f64 {
        let id = WaveId::Ref(slot);
        let owner = if self.meas_own.contains_key(&(kind, id)) || self.m.meas.contains(&(kind, id)) {
            id
        } else {
            let Some(&(_, source)) = self.m.meas.iter().find(|e| e.0 == kind) else { return f64::NAN };
            source
        };
        self.measurement_with_own(id, kind, owner)
    }

    pub(super) fn set_kinds(&self) -> Vec<Meas> {
        let mut v = Vec::new();
        for &(k, _) in &self.m.meas {
            if !v.contains(&k) {
                v.push(k);
            }
        }
        v
    }

    /// Measurement table of the set: each added kind across the visible references.
    pub(super) fn set_table(&self) -> (Vec<usize>, Vec<(Meas, Vec<f64>)>) {
        let slots = self.wave_slots();
        let rows = self.set_kinds().into_iter().map(|k| {
            let vals = slots.iter().map(|&s| self.set_cell(k, s)).collect();
            (k, vals)
        }).collect();
        (slots, rows)
    }

    /// Min, max and mean of each measurement across the set.
    pub(super) fn set_summary(&self) -> Vec<(Meas, f64, f64, f64)> {
        let (_, rows) = self.set_table();
        rows.into_iter().map(|(k, vals)| {
            let finite: Vec<f64> = vals.into_iter().filter(|v| v.is_finite()).collect();
            if finite.is_empty() {
                (k, f64::NAN, f64::NAN, f64::NAN)
            } else {
                let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
                let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let mean = finite.iter().sum::<f64>() / finite.len() as f64;
                (k, min, max, mean)
            }
        }).collect()
    }

    /// Voltage histogram of the references on screen, 24 bins from the combined range.
    pub(super) fn set_histogram(&self) -> Vec<u32> {
        let mut bins = vec![0u32; 24];
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        let slots = self.wave_slots();
        for &s in &slots {
            if let Some((v, _, _)) = self.reference_slice(s) {
                let step = (v.len() / 4096).max(1);
                for x in v.iter().step_by(step).copied().filter(|x| x.is_finite()) {
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        if !lo.is_finite() || hi <= lo {
            return bins;
        }
        let span = hi - lo;
        let last = bins.len() - 1;
        for &s in &slots {
            if let Some((v, _, _)) = self.reference_slice(s) {
                let step = (v.len() / 4096).max(1);
                for x in v.iter().step_by(step).copied().filter(|x| x.is_finite()) {
                    let i = (((x - lo) / span) * (last as f32 + 0.999)) as usize;
                    bins[i.min(last)] += 1;
                }
            }
        }
        bins
    }

    pub(super) fn draw_waves_board(&self, cv: &mut Canvas) {
        let Some(r) = self.waves_board_rect() else { return };
        let f = &self.fonts();
        cv.fill(r, PANEL_BG);
        cv.rect(r, CARD_BORDER);
        cv.set_clip(r.inset(6));
        let mut y = r.y + 16;
        cv.text(&f.sb, r.x + 8, y, "Offline waveforms", TEXT);
        let state = if self.m.run == RunState::Run { "Playing" } else { "Stopped" };
        cv.text(&f.s, r.x + r.w - 8 - f.s.width(state), y, state, if self.m.run == RunState::Run { GREEN } else { DIM });
        y += 8;
        let (slots, table) = self.set_table();
        y = self.draw_set_table(cv, r, y, &slots, &table);
        y = self.draw_set_summary(cv, r, y);
        y = self.draw_set_plots(cv, r, y, &slots);
        self.draw_set_histogram(cv, r, y);
        cv.no_clip();
    }

    fn draw_set_table(&self, cv: &mut Canvas, r: Rect, mut y: i32, slots: &[usize], table: &[(Meas, Vec<f64>)]) -> i32 {
        let f = &self.fonts();
        y += 16;
        cv.text(&f.s, r.x + 8, y, "Table", LABEL);
        y += 4;
        let shown_slots: Vec<usize> = slots.iter().copied().take(4).collect();
        let name_w = 72;
        let col_w = ((r.w - 16 - name_w) / shown_slots.len().max(1) as i32).max(36);
        let mut x = r.x + 8 + name_w;
        for &s in &shown_slots {
            cv.text(&f.s, x, y + 14, &format!("R{}", s + 1), REF_COLORS[s % REF_COLORS.len()].1);
            x += col_w;
        }
        y += 16;
        if table.is_empty() {
            cv.text(&f.s, r.x + 8, y + 14, "Add a measurement", DIM);
            return y + 18;
        }
        for (k, vals) in table.iter().take(6) {
            cv.text(&f.s, r.x + 8, y + 13, k.name(), TEXT);
            let mut x = r.x + 8 + name_w;
            for (i, v) in vals.iter().take(shown_slots.len()).enumerate() {
                let unit = k.unit();
                let text = if v.is_finite() { si_digits(*v, unit, 2) } else { "----".into() };
                cv.text(&f.s, x, y + 13, &text, REF_COLORS[shown_slots[i] % REF_COLORS.len()].1);
                x += col_w;
            }
            y += 15;
        }
        y
    }

    fn draw_set_summary(&self, cv: &mut Canvas, r: Rect, mut y: i32) -> i32 {
        let f = &self.fonts();
        y += 14;
        cv.text(&f.s, r.x + 8, y, "Summary", LABEL);
        y += 4;
        let rows = self.set_summary();
        if rows.is_empty() {
            cv.text(&f.s, r.x + 8, y + 14, "No measurements yet", DIM);
            return y + 18;
        }
        for (k, min, max, mean) in rows.into_iter().take(4) {
            let line = if min.is_finite() {
                format!("{}  {} .. {}   mean {}", k.name(), si_digits(min, k.unit(), 2), si_digits(max, k.unit(), 2), si_digits(mean, k.unit(), 2))
            } else {
                format!("{}  ----", k.name())
            };
            cv.text(&f.s, r.x + 8, y + 13, &line, TEXT);
            y += 14;
        }
        y
    }

    fn draw_set_plots(&self, cv: &mut Canvas, r: Rect, mut y: i32, slots: &[usize]) -> i32 {
        let f = &self.fonts();
        y += 14;
        cv.text(&f.s, r.x + 8, y, "Plots", LABEL);
        y += 6;
        if slots.is_empty() {
            cv.text(&f.s, r.x + 8, y + 14, "No references on screen", DIM);
            return y + 18;
        }
        let plot_h = 28;
        for &s in slots.iter().take(4) {
            let Some((v, _, _)) = self.reference_slice(s) else { continue };
            let top = y;
            let plot = Rect::new(r.x + 28, top, r.w - 40, plot_h);
            cv.fill(plot, BAR);
            let finite = plot_samples(v);
            if finite.len() >= 2 {
                let lo = finite.iter().copied().fold(f32::INFINITY, f32::min);
                let hi = finite.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let span = (hi - lo).max(1e-9);
                let col = REF_COLORS[s % REF_COLORS.len()].1;
                let mut prev: Option<(i32, i32)> = None;
                for (i, x) in finite.iter().enumerate() {
                    let px = plot.x + (i as i32 * (plot.w - 1)) / (finite.len() as i32 - 1).max(1);
                    let py = plot.y + plot.h - 2 - ((*x - lo) / span * (plot.h - 4) as f32) as i32;
                    if let Some((x0, y0)) = prev {
                        cv.line(x0, y0, px, py, col);
                    }
                    prev = Some((px, py));
                }
            }
            cv.text(&f.s, r.x + 8, top + 16, &format!("R{}", s + 1), REF_COLORS[s % REF_COLORS.len()].1);
            y += plot_h + 4;
        }
        y
    }

    fn draw_set_histogram(&self, cv: &mut Canvas, r: Rect, mut y: i32) {
        let f = &self.fonts();
        y += 10;
        cv.text(&f.s, r.x + 8, y, "Histogram", LABEL);
        y += 6;
        let bins = self.set_histogram();
        let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f32;
        let plot = Rect::new(r.x + 8, y, r.w - 16, (r.y + r.h - y - 8).clamp(24, 72));
        cv.fill(plot, BAR);
        let n = bins.len() as i32;
        let bw = (plot.w / n).max(1);
        for (i, &b) in bins.iter().enumerate() {
            let h = ((b as f32 / peak) * (plot.h - 4) as f32).round() as i32;
            if h > 0 {
                cv.fill(Rect::new(plot.x + i as i32 * bw, plot.y + plot.h - 2 - h, (bw - 1).max(1), h), ACCENT);
            }
        }
    }

    /// Row rects of the measurement table, each opening that measurement's settings.
    pub(super) fn waves_table_hits(&self) -> Vec<(usize, Rect)> {
        let Some(r) = self.waves_board_rect() else { return Vec::new() };
        let kinds = self.set_kinds();
        let y0 = r.y + 16 + 8 + 16 + 4 + 16;
        kinds.into_iter().take(6).enumerate().filter_map(|(row, k)| {
            let index = self.m.meas.iter().position(|e| e.0 == k)?;
            let y = y0 + row as i32 * 15;
            Some((index, Rect::new(r.x + 6, y, r.w - 12, 15)))
        }).collect()
    }

    pub(super) fn waves_board_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.waves_on() {
            return false;
        }
        let Some(hit) = self.waves_table_hits().into_iter().find(|(_, r)| r.contains(x, y)) else { return false };
        self.open_meas_cfg(hit.0);
        true
    }

    /// Every measurement in the list has a settings record.
    pub(super) fn ensure_meas_own(&mut self) {
        let fresh = MeasOwn::shared(&self.m);
        for &(k, id) in &self.m.meas.clone() {
            self.meas_own.entry((k, id)).or_insert(fresh);
        }
    }

    /// Opens the settings card of the selected measurement `index` (single, pair or catalogue row): a page of the Studio
    /// sheet, or a dialog in Minimal and Default.
    pub(super) fn open_meas_cfg(&mut self, index: usize) {
        if index >= self.measurement_count() {
            self.meas_cfg = None;
            return;
        }
        self.ensure_meas_own();
        self.meas_cfg = Some(index as u16);
        if self.measrow_plan().is_empty() {
            self.meas_cfg = None;
            self.toast("No settings for this measurement");
            return;
        }
        if self.menu_open && self.menu != MenuId::MeasRow {
            self.open_submenu(MenuId::MeasRow);
        } else {
            self.open_menu(MenuId::MeasRow);
        }
    }


    pub(super) fn meas_edge_coverage_caption(&self,id:WaveId,kind:Meas)->String {
        let Some((analysis,_))=self.meas_number_analysis(id,kind) else {
            return if self.meas_number_computing(id,kind){"Computing"}else{"Not retained"}.into();
        };
        let (used,total)=if kind==Meas::Rise {(analysis.rise_count,analysis.rises.len())}
            else {(analysis.fall_count,analysis.falls.len())};
        if used==0 {format!("0 / {total}; unavailable")}
        else {format!("{used} / {total} qualified")}
    }

    #[cfg_attr(not(test), allow(dead_code))] // only the tests use it
    pub(crate) fn set_meas_refs(&mut self, index: usize, low: f64, high: f64) {
        if index >= self.m.meas.len() {
            return;
        }
        self.ensure_meas_own();
        let (k, id) = self.m.meas[index];
        if let Some(own) = self.meas_own.get_mut(&(k, id)) {
            own.low = low;
            own.high = high;
            own.refs_set = true;
        }
        self.clear_measurement_statistics();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::new(Source::Demo { t: 0.0 })
    }

    fn square(amp: f32) -> Vec<f32> {
        (0..400).map(|i| if (i / 20) % 2 == 0 { -amp } else { amp }).collect()
    }

    #[test]
    fn plot_sampling_is_bounded_even_when_most_native_samples_are_not_finite() {
        let mut values = vec![f32::NAN; 1_000_003];
        for value in values.iter_mut().step_by(1000) { *value = 1.0; }
        let sampled = plot_samples(&values);
        assert!(sampled.len() <= 200);
        assert_eq!(sampled.first(), Some(&1.0));
        assert_eq!(plot_samples(&[f32::NAN, 0.0, 1.0]), vec![0.0, 1.0]);
    }

    fn edge() -> Vec<f32> {
        let mut v = vec![0.0f32; 20];
        v.extend((0..100).map(|i| i as f32 / 99.0));
        v.extend(vec![1.0; 80]);
        v.extend((0..100).map(|i| 1.0 - i as f32 / 99.0));
        v.extend(vec![0.0; 20]);
        v
    }

    fn put(u: &mut Ui, slot: usize, v: Vec<f32>, on: bool) {
        u.refs[slot] = Some(RefWave { src: 0, color: slot % REF_COLORS.len(), label: format!("R{}", slot + 1), t0: 0.0, dt: 1e-9, v: v.into(), vpd: 1.0, offset: 0.0, on });
    }

    #[test]
    fn take_over_fills_the_screen_with_the_loaded_reference() {
        let dir = std::env::temp_dir().join(format!("oos-takeover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut u = ui();
        u.m.layout = Layout::Default; // the direct slot import of the stock dialog
        u.store_dir = dir.clone();
        put(&mut u, 1, square(1.0), true);
        assert!(u.m.ch[0].on && u.look == RefLook::Live);
        let held = (u.m.tscale, u.m.tfine, u.m.delay, u.m.ch[0].on);
        let mut csv = String::from("Time (s),Volts\n");
        for (i, x) in square(0.5).iter().enumerate() {
            csv.push_str(&format!("{:.12e},{}\n", i as f64 * 1e-9, x));
        }
        std::fs::write(dir.join("ref1.csv"), csv).unwrap();
        u.ref_sel = 0;
        u.activate(Item::RefImport);
        u.m.layout = Layout::Minimal;
        assert_eq!(u.look, RefLook::Live, "import adds a reference without changing the live acquisition");
        assert!(!u.offline && u.m.ch[0].on, "import keeps the live channels");
        assert!(u.refs[0].as_ref().unwrap().on && u.refs[1].as_ref().unwrap().on);
        u.set_look(RefLook::TakeOver);
        assert_eq!(u.look, RefLook::TakeOver);
        assert!(u.offline);
        assert!(u.m.ch.iter().all(|c| !c.on), "other channels are hidden");
        assert!(u.cards().0.iter().all(|c| c.w == 0));
        assert!(u.refs[0].as_ref().unwrap().on);
        assert!(!u.refs[1].as_ref().unwrap().on, "the other reference leaves the screen");
        assert!(u.m.time_per_div() > 0.0);
        u.set_look(RefLook::Live);
        assert_eq!(u.look, RefLook::Live);
        assert!(!u.offline);
        assert_eq!((u.m.tscale, u.m.tfine, u.m.delay, u.m.ch[0].on), held);
        assert!(u.refs[1].as_ref().unwrap().on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_keeps_several_references_and_measures_them_as_a_set() {
        let mut u = ui();
        put(&mut u, 0, square(1.0), true);
        put(&mut u, 1, square(3.0), true);
        u.m.meas.clear();
        u.meas_own.clear();
        u.meas_src = WaveId::Ref(0);
        u.activate(Item::MeasAdd(Meas::Vpp));
        u.set_look(RefLook::Waves);
        assert_eq!(u.look, RefLook::Waves);
        assert!(u.refs[0].as_ref().unwrap().on && u.refs[1].as_ref().unwrap().on);
        assert!(u.m.ch.iter().all(|c| !c.on));
        let (slots, table) = u.set_table();
        assert_eq!(slots, vec![0, 1]);
        let (_, vals) = table.iter().find(|(k, _)| *k == Meas::Vpp).unwrap();
        assert!((vals[0] - 2.0).abs() < 0.05 && (vals[1] - 6.0).abs() < 0.05, "{vals:?}");
        let (_, min, max, mean) = u.set_summary().into_iter().find(|(k, ..)| *k == Meas::Vpp).unwrap();
        assert!((min - 2.0).abs() < 0.05 && (max - 6.0).abs() < 0.05 && (mean - 4.0).abs() < 0.05, "{min} {max} {mean}");
        assert_eq!(u.wave_slots().len(), 2);
        assert!(u.set_histogram().iter().any(|&b| b > 0));
        let mut canvas = Canvas::new(W, H);
        u.render(&mut canvas);
        assert!(u.waves_board_rect().is_some());
        u.key(key::RUNSTOP);
        assert!(u.sim_play && u.m.run == RunState::Run);
        u.step(0.25);
        assert!(u.replay.captures >= 1, "the set plays");
        u.set_look(RefLook::Live);
        assert!(u.m.ch[0].on && !u.offline && u.waves_board_rect().is_none());
    }

    #[test]
    fn every_added_measurement_opens_its_own_settings() {
        let mut u = ui();
        u.m.meas.clear();
        u.meas_own.clear();
        u.activate(Item::MeasAdd(Meas::Vpp));
        u.activate(Item::MeasAdd(Meas::Burst));
        assert!(u.meas_own.contains_key(&(Meas::Vpp, WaveId::Ch(0))));
        assert!(u.meas_own.contains_key(&(Meas::Burst, WaveId::Ch(0))));
        u.key(key::MEASURE);
        let vpp = u.catalog_set_rect(Meas::Vpp).expect("a place to open Vpp settings");
        u.tap(vpp.x + vpp.w / 2, vpp.y + vpp.h / 2);
        assert_eq!(u.menu, MenuId::MeasRow, "Vpp has a settings card");
        let plan: Vec<Item> = u.measrow_plan().into_iter().flat_map(|(_, rows)| rows).collect();
        assert!(plan.contains(&Item::RowF(measrow::RF::Gate)) && !plan.contains(&Item::RowF(measrow::RF::Idle)));
        u.close_menu();
        u.key(key::MEASURE);
        u.meas_cfg = None;
        let mut opened = false;
        for _ in 0..12 {
            let view = u.catalog_view();
            if !opened {
                // TIMING is the third group of Basic
                if let Some(time) = u.catalog_group_rect(2) {
                    if time.y < view.y + view.h - 4 && time.y + time.h > view.y + 4 {
                        let y = (time.y + 4).clamp(view.y + 2, view.y + view.h - 2);
                        u.tap(time.x + 12, y);
                        opened = true;
                        continue;
                    }
                }
            } else if let Some(burst) = u.catalog_set_rect(Meas::Burst) {
                let y = burst.y + burst.h / 2;
                if view.contains(burst.x + burst.w / 2, y) {
                    u.tap(burst.x + burst.w / 2, y);
                    break;
                }
            }
            let next = u.cat_scroll + 80;
            u.scroll_to(super::scroll::List::Catalog, next);
        }
        assert_eq!(u.menu, MenuId::MeasRow, "Burst has a settings card");
        let plan: Vec<Item> = u.measrow_plan().into_iter().flat_map(|(_, rows)| rows).collect();
        assert!(plan.contains(&Item::RowF(measrow::RF::Idle)), "Burst keeps its own idle");
        u.adjust(Item::RowF(measrow::RF::Idle), 1);
        let own = u.m.row_cfg(crate::model::RowKey::One(Meas::Burst, WaveId::Ch(0))).expect("the burst row has its own idle");
        assert!(own.idle.is_some());
        assert!(u.m.row_cfg(crate::model::RowKey::One(Meas::Vpp, WaveId::Ch(0))).is_none(), "Vpp keeps following");
    }

    #[test]
    fn reference_levels_belong_to_the_measurement_that_was_added() {
        let mut u = ui();
        put(&mut u, 0, edge(), true);
        put(&mut u, 1, edge(), true);
        u.m.meas.clear();
        u.meas_own.clear();
        u.meas_src = WaveId::Ref(0);
        u.activate(Item::MeasAdd(Meas::Rise));
        u.meas_src = WaveId::Ref(1);
        u.activate(Item::MeasAdd(Meas::Rise));
        u.measure_refs();
        u.set_meas_refs(0, 0.1, 0.9);
        u.set_meas_refs(1, 0.2, 0.8);
        let slow = u.meas_number(WaveId::Ref(0), Meas::Rise);
        let fast = u.meas_number(WaveId::Ref(1), Meas::Rise);
        assert!((slow - 80e-9).abs() < 8e-9, "{slow}");
        assert!((fast - 60e-9).abs() < 8e-9, "{fast}");
        assert!(fast < slow);
    }
    #[test]
    fn rise_fall_settings_show_qualified_counts_and_missing_partial_transitions(){
        let mut u=ui();u.m.meas_full_record=true;
        let mut v=vec![0.0];v.extend((0..4000).map(|i|if i%2==0{0.4}else{0.6}));v.push(1.0);
        put(&mut u,0,v,true);u.m.meas=vec![(Meas::Rise,WaveId::Ref(0)),(Meas::Fall,WaveId::Ref(0))];
        for (index,kind)in[Meas::Rise,Meas::Fall].into_iter().enumerate(){
            u.open_meas_cfg(index);let caption=u.meas_edge_coverage_caption(WaveId::Ref(0),kind);
            assert!(caption.starts_with("0 /")&&caption.contains("unavailable"),"{caption}");assert!(u.meas_number(WaveId::Ref(0),kind).is_nan());
            let plan: Vec<Item> = u.measrow_plan().into_iter().flat_map(|(_, rows)| rows).collect();
            assert!(plan.contains(&Item::RowF(measrow::RF::Used)), "the card shows the edges used");
            assert!(u.fonts().s.width(&u.measrow_text(Item::RowF(measrow::RF::Used)).unwrap()) <= sheet::SHEET_W - 24);
        }
    }

}
