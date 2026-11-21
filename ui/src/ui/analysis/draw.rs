// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The drawing of the analysers in the plot area (spectrum, power, the text panel) and the spectrum and power pages
//! behind the analyser registry (`analysers.rs`). A new page gets its `Analyser` implementation here or in its own
//! file.

use super::*;

impl Ui {
    /// The active analyser in the plot area.
    pub(in crate::ui) fn draw_analyzer(&self, cv: &mut Canvas) {
        if let Some(a) = super::analysers::of(self.m.view) {
            a.draw(self, cv);
        }
    }

    pub(crate) fn panel(&self, cv: &mut Canvas, x: i32, y: i32, lines: &[String], col: Rgb) {
        let f = self.fonts();
        let w = lines.iter().map(|l| f.s.width(l)).max().unwrap_or(0) + 16;
        let r = Rect::new(x, y, w, lines.len() as i32 * 18 + 8);
        cv.round(r, 4, PANEL_BG, 230);
        for (k, l) in lines.iter().enumerate() {
            cv.text(&f.s, r.x + 8, r.y + 17 + 18 * k as i32, l, col);
        }
    }

    /// The spectrum in `a`: the plot area, or the second pane (`pane`).
    pub(in crate::ui) fn draw_spectrum(&self, cv: &mut Canvas, a: Rect, pane: bool) {
        let f = self.fonts();
        let col = self.wave_color(self.m.spec_src);
        let Some(sp) = &self.azr.spec else {
            let t = if self.wave_ready(self.m.spec_src) {
                "Spectrum: waiting for a record".to_string()
            } else {
                format!("Spectrum: {}", missing_wave(self.m.spec_src))
            };
            cv.text(&f.sb, a.x + 20, a.y + a.h / 2 + 5, &t, DIM);
            return;
        };
        cv.fill(a, BG);
        let b = self.spec_box(a, pane);
        let g = b.plot;
        let wf_h = self.spec_wf_h(a, pane);
        let n = sp.dbv.len().max(2);
        let (top, dbdiv) = (self.m.spec_ref, panes::DB_PER_DIV);
        let y_of = |db: f32| {
            (g.y as f32 + (top - db) / dbdiv * (g.h as f32 / VDIV)).clamp(g.y as f32 - 2.0, (g.y + g.h) as f32 + 2.0)
                as i32
        };
        let x_of = |i: usize| g.x + (i as i64 * (g.w as i64 - 1) / (n as i64 - 1)) as i32;
        self.draw_spec_frame(cv, &b, sp);
        cv.set_clip(g);
        let mut prev: Option<(i32, i32)> = None;
        for (i, &db) in sp.dbv.iter().enumerate() {
            let p = (x_of(i), y_of(db));
            if let Some(q) = prev {
                cv.line(q.0, q.1, p.0, p.1, col);
            }
            prev = Some(p);
        }
        // the limit, over the frequencies of the trace on the screen
        if let Some(l) = &self.azr.limit {
            let mut prev: Option<(i32, i32)> = None;
            for i in 0..n {
                let Some(db) = spectrum::limit_at(l, sp.freq(i)) else {
                    prev = None;
                    continue;
                };
                let p = (x_of(i), y_of(db));
                if let Some(q) = prev {
                    cv.line(q.0, q.1, p.0, p.1, RED);
                }
                prev = Some(p);
            }
        }
        cv.no_clip();
        self.draw_spec_marks(cv, &b, sp, top);
        self.draw_peaks(cv, &b, sp, pane);
        if let (true, Some(w)) = (wf_h > 0, &self.azr.wf) {
            let wr = Rect::new(g.x, a.y + a.h - wf_h, g.w, wf_h);
            let rows = w.rows.len().max(1);
            let rh = (wf_h as usize / rows.max(wf_h as usize / 3)).max(1) as i32;
            for (k, row) in w.rows.iter().rev().enumerate() {
                let y = wr.y + k as i32 * rh;
                if y >= wr.y + wr.h {
                    break;
                }
                let m = row.len().max(2);
                for px in 0..wr.w {
                    let i = (px as usize * (m - 1)) / (wr.w as usize - 1).max(1);
                    let (r, gg, b) = spectrum::color(row[i.min(m - 1)], top, 90.0);
                    cv.fill(Rect::new(wr.x + px, y, 1, rh), rgb(r as u32, gg as u32, b as u32));
                }
            }
            cv.hline(wr.x, wr.x + wr.w, wr.y, DIM);
        }
    }

    fn draw_power(&self, cv: &mut Canvas) {
        let a = self.plot.area;
        let mut l = vec![format!(
            "Power  V = {}  I = {}  ({:?})",
            self.wave_name(self.m.pw_v),
            self.wave_name(self.m.pw_i),
            self.m.pw_mode
        )];
        l.extend(self.azr.power.iter().cloned());
        self.panel(cv, a.x + 20, a.y + 30, &l, TEXT);
    }
}

/// The spectrum page (FFT of one source, with the pane beside the waveform).
pub(in crate::ui) struct SpectrumPage;

impl super::analysers::Analyser for SpectrumPage {
    fn view(&self) -> AView {
        AView::Spectrum
    }

    fn feature(&self) -> (&'static str, &'static str) {
        ("spectrum", "Spectrum")
    }

    fn name(&self) -> &'static str {
        "Spectrum"
    }

    fn menu(&self) -> MenuId {
        MenuId::Spectrum
    }

    fn scpi(&self) -> Option<&'static str> {
        Some("SPECtrum")
    }

    fn more(&self, _ui: &Ui) -> Vec<Item> {
        vec![Item::SpecPeak]
    }

    /// A stopped scope can still measure a loaded ref. A live channel needs a record.
    fn without_record(&self, ui: &Ui) -> bool {
        matches!(ui.m.spec_src, WaveId::Ref(_))
    }

    fn update(&self, ui: &mut Ui, rec: &Record, pane: bool) {
        ui.spectrum_update(rec, pane);
    }

    fn draw(&self, ui: &Ui, cv: &mut Canvas) {
        ui.draw_spectrum(cv, ui.plot.area, false);
    }
}

/// The power analysis page (quality, harmonics, switching, ripple, capacitance, rails).
pub(in crate::ui) struct PowerPage;

impl super::analysers::Analyser for PowerPage {
    fn view(&self) -> AView {
        AView::Power
    }

    fn feature(&self) -> (&'static str, &'static str) {
        ("power", "Power")
    }

    fn name(&self) -> &'static str {
        "Power analysis"
    }

    fn menu(&self) -> MenuId {
        MenuId::Power
    }

    fn scpi(&self) -> Option<&'static str> {
        Some("POWer")
    }

    fn without_record(&self, ui: &Ui) -> bool {
        let is_ref = |id: WaveId| matches!(id, WaveId::Ref(_));
        is_ref(ui.m.pw_v) || is_ref(ui.m.pw_i) || ui.m.pw_vin.is_some_and(is_ref) || ui.m.pw_clk.is_some_and(is_ref)
    }

    fn update(&self, ui: &mut Ui, rec: &Record, _pane: bool) {
        ui.power_update(rec);
    }

    fn draw(&self, ui: &Ui, cv: &mut Canvas) {
        ui.draw_power(cv);
    }
}
