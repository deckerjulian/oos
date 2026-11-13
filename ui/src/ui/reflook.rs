// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Two ways to look at a loaded reference. Take over fills the screen with one reference and hides
//! the other channels. Offline keeps several references on screen together: they play, and they are
//! measured as a set (table, summary, plots, histogram). Neither mode changes the live scope.

use super::*;

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
}

impl MeasOwn {
    fn shared(m: &Model) -> Self {
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
        }
    }
}

/// Geometry of the settings dialog. Source and reference levels are always there.
pub(super) struct CfgGeom {
    pub panel: Rect,
    pub close: Rect,
    pub source_m: Rect,
    pub source_p: Rect,
    pub refs: Rect,
    pub idle_m: Option<Rect>,
    pub idle_p: Option<Rect>,
    pub side: Option<Rect>,
    pub n_m: Option<Rect>,
    pub n_p: Option<Rect>,
    pub edges: Option<Rect>,
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
            RefLook::Live => self.set_offline(false),
            RefLook::TakeOver => {
                if !self.offline {
                    self.set_offline(true);
                }
                self.look = RefLook::TakeOver;
                let slot = self.active_reference();
                self.present_record(slot);
                self.solo_reference(slot);
            }
            RefLook::Waves => {
                if !self.offline {
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
        self.look == RefLook::Waves && !self.view_on() && !self.xy && self.wins.len() <= 1
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

    /// One added measurement, using its own settings when those have been set.
    pub(super) fn meas_number(&self, id: WaveId, k: Meas) -> f64 {
        let own = self.meas_own.get(&(k, id)).copied();
        let (low, high) = match own.filter(|o| o.refs_set) {
            Some(o) => (o.low, o.high),
            None => (self.m.meas_lpct, self.m.meas_hpct),
        };
        let Some(a) = self.analysis_for(id, low, high) else { return f64::NAN };
        let idle = own.filter(|o| o.idle_set).map(|o| o.idle).unwrap_or(self.m.meas_idle);
        let side = own.filter(|o| o.side_set).map(|o| o.side).unwrap_or(self.m.meas_out);
        let n = own.filter(|o| o.n_set).map(|o| o.n).unwrap_or(self.m.meas_n);
        let data = own.filter(|o| o.data_set).map(|o| o.data).unwrap_or(self.m.meas_data);
        match k {
            Meas::Burst => oos_dsp::measure::burst_width(&a, idle),
            Meas::Outside => oos_dsp::measure::time_outside(&a, side),
            Meas::NPeriods => oos_dsp::measure::duration_n(&a, n.max(1) as usize, data),
            _ => a.get(k),
        }
    }

    fn analysis_for(&self, id: WaveId, low: f64, high: f64) -> Option<oos_dsp::measure::Analysis> {
        if (low - self.m.meas_lpct).abs() < 1e-12 && (high - self.m.meas_hpct).abs() < 1e-12 {
            return self.meas_analysis(id).map(|(a, _)| a);
        }
        match id {
            WaveId::Ch(c) => {
                if c >= n_ch() || !self.m.ch[c].on {
                    return None;
                }
                let rec = self.last[0].as_ref()?;
                let (v, dt) = self.meas_wave(rec, c);
                if v.len() < 2 || !(dt > 0.0) {
                    return None;
                }
                Some(oos_dsp::measure::analyze_refs(&v, dt, 0.4 * self.m.ch[c].volts_per_div() as f64, low, high))
            }
            WaveId::Ref(i) => {
                let (samples, _, dt) = self.reference_slice(i)?;
                if samples.len() < 2 || !(dt > 0.0) {
                    return None;
                }
                let vpd = self.refs[i].as_ref().map(|r| r.vpd).unwrap_or(0.2);
                Some(oos_dsp::measure::analyze_refs(samples, dt, 0.4 * vpd as f64, low, high))
            }
        }
    }

    /// A value of `kind` on one reference of the set. Settings follow that reference when it has
    /// this measurement, otherwise the first added copy of the kind.
    pub(super) fn set_cell(&self, kind: Meas, slot: usize) -> f64 {
        let id = WaveId::Ref(slot);
        if self.meas_own.contains_key(&(kind, id)) || self.m.meas.iter().any(|e| *e == (kind, id)) {
            return self.meas_number(id, kind);
        }
        let Some(&(_, src)) = self.m.meas.iter().find(|e| e.0 == kind) else { return f64::NAN };
        let own = self.meas_own.get(&(kind, src)).copied();
        let (low, high) = match own.filter(|o| o.refs_set) {
            Some(o) => (o.low, o.high),
            None => (self.m.meas_lpct, self.m.meas_hpct),
        };
        let Some(a) = self.analysis_for(id, low, high) else { return f64::NAN };
        let idle = own.filter(|o| o.idle_set).map(|o| o.idle).unwrap_or(self.m.meas_idle);
        let side = own.filter(|o| o.side_set).map(|o| o.side).unwrap_or(self.m.meas_out);
        let n = own.filter(|o| o.n_set).map(|o| o.n).unwrap_or(self.m.meas_n);
        let data = own.filter(|o| o.data_set).map(|o| o.data).unwrap_or(self.m.meas_data);
        match kind {
            Meas::Burst => oos_dsp::measure::burst_width(&a, idle),
            Meas::Outside => oos_dsp::measure::time_outside(&a, side),
            Meas::NPeriods => oos_dsp::measure::duration_n(&a, n.max(1) as usize, data),
            _ => a.get(kind),
        }
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
        let f = &self.fonts;
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
        let f = &self.fonts;
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
        let f = &self.fonts;
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
        let f = &self.fonts;
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
            let finite: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).step_by((v.len() / 200).max(1)).collect();
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
        let f = &self.fonts;
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

    pub(super) fn open_meas_cfg(&mut self, index: usize) {
        if index >= self.m.meas.len() {
            self.meas_cfg = None;
            return;
        }
        self.ensure_meas_own();
        self.meas_cfg = Some(index as u16);
    }

    pub(super) fn cfg_geom(&self) -> Option<CfgGeom> {
        let index = self.meas_cfg? as usize;
        let (k, _) = *self.m.meas.get(index)?;
        let extras = match k {
            Meas::Burst => 1,
            Meas::Outside => 1,
            Meas::NPeriods => 2,
            _ => 0,
        };
        let rows = 2 + extras;
        let h = 52 + rows * 48 + 16;
        let panel = Rect::new((W - 480) / 2, (H - h) / 2, 480, h);
        let close = Rect::new(panel.x + panel.w - 48, panel.y + 8, 36, 28);
        let row = |n: i32| Rect::new(panel.x + 168, panel.y + 48 + n * 48, 280, 36);
        let source = row(0);
        let refs = row(1);
        let (idle_m, idle_p, side, n_m, n_p, edges) = match k {
            Meas::Burst => {
                let idle = row(2);
                (Some(Rect::new(idle.x, idle.y, 44, idle.h)), Some(Rect::new(idle.x + idle.w - 44, idle.y, 44, idle.h)), None, None, None, None)
            }
            Meas::Outside => (None, None, Some(row(2)), None, None, None),
            Meas::NPeriods => {
                let n = row(2);
                let edges = row(3);
                (None, None, None, Some(Rect::new(n.x, n.y, 44, n.h)), Some(Rect::new(n.x + n.w - 44, n.y, 44, n.h)), Some(edges))
            }
            _ => (None, None, None, None, None, None),
        };
        Some(CfgGeom {
            panel,
            close,
            source_m: Rect::new(source.x, source.y, 44, source.h),
            source_p: Rect::new(source.x + source.w - 44, source.y, 44, source.h),
            refs,
            idle_m,
            idle_p,
            side,
            n_m,
            n_p,
            edges,
        })
    }

    pub(super) fn draw_meas_cfg(&self, cv: &mut Canvas) {
        let Some(g) = self.cfg_geom() else { return };
        let index = self.meas_cfg.unwrap() as usize;
        let Some(&(k, id)) = self.m.meas.get(index) else { return };
        let own = self.meas_own.get(&(k, id)).copied().unwrap_or_else(|| MeasOwn::shared(&self.m));
        let f = &self.fonts;
        cv.round(g.panel, 8, ACCENT, 255);
        cv.round(g.panel.inset(2), 7, DLG_BG, 255);
        cv.text(&f.mb, g.panel.x + 16, g.panel.y + 26, &format!("{} settings", k.name()), TEXT);
        btn(cv, g.close, BTN, BTN_BORDER);
        cv.text_in(&f.sb, g.close, 1, "X", TEXT);
        self.cfg_caption(cv, g.panel, 0, "Source");
        self.cfg_caption(cv, g.panel, 1, "Reference");
        let source = Rect::new(g.source_m.x, g.source_m.y, g.source_p.x + g.source_p.w - g.source_m.x, g.source_m.h);
        self.draw_stepper(cv, g.source_m, g.source_p, source, &self.wave_name(id));
        let (low, high) = if own.refs_set { (own.low, own.high) } else { (self.m.meas_lpct, self.m.meas_hpct) };
        btn(cv, g.refs, BTN, BTN_BORDER);
        cv.text_in(&f.mb, g.refs, 1, &format!("{:.0}/{:.0}", low * 100.0, high * 100.0), TEXT);
        if let (Some(mi), Some(pl)) = (g.idle_m, g.idle_p) {
            self.cfg_caption(cv, g.panel, 2, "Idle");
            let idle = if own.idle_set { own.idle } else { self.m.meas_idle };
            let text = if idle > 0.0 { si_short(idle, "s") } else { "period".into() };
            self.draw_stepper(cv, mi, pl, Rect::new(mi.x, mi.y, pl.x + pl.w - mi.x, mi.h), &text);
        }
        if let Some(side) = g.side {
            self.cfg_caption(cv, g.panel, 2, "Outside");
            let which = if own.side_set { own.side } else { self.m.meas_out };
            btn(cv, side, BTN, BTN_BORDER);
            cv.text_in(&f.mb, side, 1, &format!("{:?}", which), TEXT);
        }
        if let (Some(mi), Some(pl)) = (g.n_m, g.n_p) {
            self.cfg_caption(cv, g.panel, 2, "N periods");
            let n = if own.n_set { own.n } else { self.m.meas_n };
            self.draw_stepper(cv, mi, pl, Rect::new(mi.x, mi.y, pl.x + pl.w - mi.x, mi.h), &n.to_string());
        }
        if let Some(edges) = g.edges {
            self.cfg_caption(cv, g.panel, 3, "Edges");
            let data = if own.data_set { own.data } else { self.m.meas_data };
            btn(cv, edges, BTN, BTN_BORDER);
            cv.text_in(&f.mb, edges, 1, if data { "Data" } else { "Clock" }, TEXT);
        }
    }

    fn cfg_caption(&self, cv: &mut Canvas, panel: Rect, n: i32, text: &str) {
        cv.text(&self.fonts.s, panel.x + 16, panel.y + 72 + n * 48, text, LABEL);
    }

    fn draw_stepper(&self, cv: &mut Canvas, minus: Rect, plus: Rect, full: Rect, value: &str) {
        let f = &self.fonts;
        btn(cv, minus, BTN, BTN_BORDER);
        cv.text_in(&f.mb, minus, 1, "-", TEXT);
        btn(cv, plus, BTN, BTN_BORDER);
        cv.text_in(&f.mb, plus, 1, "+", TEXT);
        let mid = Rect::new(minus.x + minus.w + 4, full.y, (plus.x - minus.x - minus.w - 8).max(20), full.h);
        btn(cv, mid, BTN, BTN_BORDER);
        cv.text_in(&f.mb, mid, 1, value, TEXT);
    }

    pub(super) fn meas_cfg_tap(&mut self, x: i32, y: i32) -> bool {
        let Some(g) = self.cfg_geom() else { return false };
        if !g.panel.contains(x, y) {
            self.meas_cfg = None;
            return true;
        }
        if g.close.contains(x, y) {
            self.meas_cfg = None;
            return true;
        }
        if g.source_m.contains(x, y) {
            self.meas_cfg_source(-1);
        } else if g.source_p.contains(x, y) {
            self.meas_cfg_source(1);
        } else if g.refs.contains(x, y) {
            self.meas_cfg_refs();
        } else if g.idle_m.is_some_and(|r| r.contains(x, y)) {
            self.meas_cfg_idle(-1);
        } else if g.idle_p.is_some_and(|r| r.contains(x, y)) {
            self.meas_cfg_idle(1);
        } else if g.side.is_some_and(|r| r.contains(x, y)) {
            self.meas_cfg_side();
        } else if g.n_m.is_some_and(|r| r.contains(x, y)) {
            self.meas_cfg_n(-1);
        } else if g.n_p.is_some_and(|r| r.contains(x, y)) {
            self.meas_cfg_n(1);
        } else if g.edges.is_some_and(|r| r.contains(x, y)) {
            self.meas_cfg_edges();
        }
        true
    }

    fn cfg_entry(&mut self) -> Option<(usize, Meas, WaveId)> {
        let index = self.meas_cfg? as usize;
        let (k, id) = *self.m.meas.get(index)?;
        self.ensure_meas_own();
        Some((index, k, id))
    }

    fn meas_cfg_source(&mut self, d: i32) {
        let Some((index, k, id)) = self.cfg_entry() else { return };
        let srcs = self.meas_sources();
        let at = srcs.iter().position(|s| *s == id).unwrap_or(0);
        let next = srcs[(at as i32 + d).rem_euclid(srcs.len() as i32) as usize];
        if next == id {
            return;
        }
        if self.m.meas.iter().enumerate().any(|(j, e)| j != index && *e == (k, next)) {
            self.toast("That measurement is already on this source");
            return;
        }
        let own = self.meas_own.remove(&(k, id));
        self.m.meas[index].1 = next;
        if let Some(own) = own {
            self.meas_own.insert((k, next), own);
        }
        self.mstat.clear();
    }

    fn meas_sources(&self) -> Vec<WaveId> {
        let mut v: Vec<WaveId> = (0..n_ch()).map(WaveId::Ch).collect();
        for i in 0..NREF {
            if self.refs[i].is_some() {
                v.push(WaveId::Ref(i));
            }
        }
        if v.is_empty() {
            v.push(WaveId::Ch(0));
        }
        v
    }

    pub(super) fn set_meas_refs(&mut self, index: usize, low: f64, high: f64) {
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
    }

    fn meas_cfg_refs(&mut self) {
        let Some((_, k, id)) = self.cfg_entry() else { return };
        let cur = self.meas_own.get(&(k, id)).copied().unwrap_or_else(|| MeasOwn::shared(&self.m));
        let low = if cur.refs_set { cur.low } else { self.m.meas_lpct };
        let (low, high) = if low < 0.15 { (0.2, 0.8) } else { (0.1, 0.9) };
        if let Some(own) = self.meas_own.get_mut(&(k, id)) {
            own.low = low;
            own.high = high;
            own.refs_set = true;
        }
    }

    fn meas_cfg_idle(&mut self, d: i32) {
        let Some((_, k, id)) = self.cfg_entry() else { return };
        let cur = self.meas_own.get(&(k, id)).map(|o| if o.idle_set { o.idle } else { self.m.meas_idle }).unwrap_or(self.m.meas_idle);
        let l: Vec<f64> = std::iter::once(0.0).chain((0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e - 9)))).collect();
        let i = l.iter().position(|&v| v >= cur * 0.999).unwrap_or(0);
        let v = l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize];
        if let Some(own) = self.meas_own.get_mut(&(k, id)) {
            own.idle = v;
            own.idle_set = true;
        }
    }

    fn meas_cfg_side(&mut self) {
        let Some((_, k, id)) = self.cfg_entry() else { return };
        let all = [oos_dsp::measure::Outside::Above, oos_dsp::measure::Outside::Below, oos_dsp::measure::Outside::Both];
        let cur = self.meas_own.get(&(k, id)).map(|o| if o.side_set { o.side } else { self.m.meas_out }).unwrap_or(self.m.meas_out);
        let i = all.iter().position(|&x| x == cur).unwrap_or(2);
        let v = all[(i as i32 + 1).rem_euclid(all.len() as i32) as usize];
        if let Some(own) = self.meas_own.get_mut(&(k, id)) {
            own.side = v;
            own.side_set = true;
        }
    }

    fn meas_cfg_n(&mut self, d: i32) {
        let Some((_, k, id)) = self.cfg_entry() else { return };
        let cur = self.meas_own.get(&(k, id)).map(|o| if o.n_set { o.n } else { self.m.meas_n }).unwrap_or(self.m.meas_n);
        let v = (cur as i32 + d).clamp(1, 32) as u32;
        if let Some(own) = self.meas_own.get_mut(&(k, id)) {
            own.n = v;
            own.n_set = true;
        }
    }

    fn meas_cfg_edges(&mut self) {
        let Some((_, k, id)) = self.cfg_entry() else { return };
        let cur = self.meas_own.get(&(k, id)).map(|o| if o.data_set { o.data } else { self.m.meas_data }).unwrap_or(self.m.meas_data);
        if let Some(own) = self.meas_own.get_mut(&(k, id)) {
            own.data = !cur;
            own.data_set = true;
        }
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

    fn edge() -> Vec<f32> {
        let mut v = vec![0.0f32; 20];
        v.extend((0..100).map(|i| i as f32 / 99.0));
        v.extend(vec![1.0; 80]);
        v.extend((0..100).map(|i| 1.0 - i as f32 / 99.0));
        v.extend(vec![0.0; 20]);
        v
    }

    fn put(u: &mut Ui, slot: usize, v: Vec<f32>, on: bool) {
        u.refs[slot] = Some(RefWave { src: 0, color: slot % REF_COLORS.len(), label: format!("R{}", slot + 1), t0: 0.0, dt: 1e-9, v, vpd: 1.0, offset: 0.0, on });
    }

    #[test]
    fn take_over_fills_the_screen_with_the_loaded_reference() {
        let dir = std::env::temp_dir().join(format!("oos-takeover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut u = ui();
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
        assert_eq!(u.look, RefLook::Live);
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
        let g = u.cfg_geom().expect("Vpp has a settings dialog");
        assert!(g.source_m.w > 0 && g.refs.w > 0 && g.idle_m.is_none());
        u.meas_cfg = None;
        let mut opened = false;
        for _ in 0..12 {
            let view = u.catalog_view();
            if !opened {
                if let Some(time) = u.catalog_group_rect(1) {
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
            let down = u.catalog_scroll_down().expect("scroll");
            u.tap(down.x + 4, down.y + 4);
        }
        let g = u.cfg_geom().expect("Burst has a settings dialog");
        assert!(g.refs.w > 0 && g.idle_m.is_some(), "Burst keeps its own idle");
        u.meas_cfg_tap(g.idle_p.unwrap().x + 4, g.idle_p.unwrap().y + 4);
        let burst_idle = u.meas_own.get(&(Meas::Burst, WaveId::Ch(0))).unwrap().idle;
        let vpp_own = u.meas_own.get(&(Meas::Vpp, WaveId::Ch(0))).unwrap();
        assert!(!vpp_own.idle_set);
        assert!(u.meas_own.get(&(Meas::Burst, WaveId::Ch(0))).unwrap().idle_set);
        assert_ne!(burst_idle, vpp_own.idle);
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
}
