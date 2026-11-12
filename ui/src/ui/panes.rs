// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The second pane under the plot: the frame of the spectrum (also the full view) with its peak list, the results table
//! and the trend charts. Everything that takes time happens per record or per trend point, the drawing only reads it.
//! A pane is as high as its content; what is left over goes to the plot.

use super::*;
use oos_dsp::spectrum;
use std::collections::VecDeque;

/// Harmonics of the measured frequency the span of the pane covers; 0 = the whole band.
pub const PANE_HARMS: [usize; 5] = [0, 5, 10, 20, 50];
/// Values of `Model::trend_sel`: by the list, off, one per measurement.
pub const TREND_SEL_N: usize = tools::MEAS_MAX + 2;
/// Lines in the peak list and on the trace.
pub(super) const PEAKS_MAX: usize = 8;
pub(super) const DB_PER_DIV: f32 = 10.0;

/// Peak list width; left axis; title line and frequency labels of the spectrum.
const PEAK_W: i32 = 212;
const AXIS_W: i32 = 34;
const TITLE_H: i32 = 20;
const XLABEL_H: i32 = 16;
/// Height of a pane with only a line of text.
const HINT_H: i32 = 60;
/// Band above the results table, as high as the close button.
const BAND_H: i32 = 40;
/// Row heights of the results table.
const ROW_MIN: i32 = 19;
const ROW_WANT: i32 = 26;
/// Width of the side column of the Results pane.
const SIDE_W: i32 = 300;
/// Time a shorter list of events waits before the pane shrinks (s): the plot should not jump with every record.
const SHRINK_AFTER: f64 = 3.0;

pub(super) fn harm_names() -> Vec<String> {
    PANE_HARMS.iter().map(|&h| if h == 0 { "All".to_string() } else { h.to_string() }).collect()
}

/// Columns and rows per column for `n` events in a height: one column while they are few.
pub(super) fn event_grid(n: usize, h: i32) -> (usize, usize) {
    let fit = (h / 20).max(1) as usize;
    let cols = if n <= 4 { 1 } else { 2 };
    (cols, n.div_ceil(cols).clamp(1, fit))
}

/// What the second pane keeps between frames.
#[derive(Default)]
pub struct PaneState {
    /// first row of the results table
    pub top: usize,
    /// decoder events, refreshed a few times a second
    pub events: Vec<tables::EventRow>,
    events_t: f64,
    /// rows the Events pane is high enough for, and when the list last needed them
    event_rows: usize,
    event_seen: f64,
}

/// Where the spectrum's parts go.
pub(super) struct SpecBox {
    pub title: Rect,
    pub plot: Rect,
    /// the sort button, and the list under it
    pub sort: Rect,
    pub list: Rect,
}

/// The nearest "round" step at or above `x`.
fn round_step(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return 1.0;
    }
    let d = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|k| k * d).find(|&s| s >= x * (1.0 - 1e-9)).unwrap_or(10.0 * d)
}

/// Grid of a chart: `nx` by `ny` divisions.
fn chart_grid(cv: &mut Canvas, g: Rect, nx: i32, ny: i32) {
    for i in 1..nx {
        cv.vline(g.x + i * (g.w - 1) / nx, g.y, g.y + g.h - 1, GRID_LINE);
    }
    for j in 1..ny {
        cv.hline(g.x, g.x + g.w - 1, g.y + j * (g.h - 1) / ny, GRID_LINE);
    }
    cv.rect(g, GRID_AXIS);
}

/// The largest of `fonts` in which every text is at most `w` wide.
fn fit_font<'a>(fonts: &[&'a Font], w: i32, texts: impl Iterator<Item = &'a str> + Clone) -> &'a Font {
    let fits = |f: &Font| texts.clone().all(|t| f.width(t) <= w);
    fonts.iter().copied().find(|f| fits(f)).unwrap_or(fonts[fonts.len() - 1])
}

// ---------------------------------------------------------------------------------------------------------------
// spectrum

impl Ui {
    /// Height of the waterfall under the spectrum.
    pub(super) fn spec_wf_h(&self, a: Rect, pane: bool) -> i32 {
        if self.m.spec_wf && !pane { a.h * 2 / 5 } else { 0 }
    }

    /// Layout of the spectrum in `a`. In the full view the list starts under the overflow button, in the pane the close
    /// button sits beside the sort button.
    pub(super) fn spec_box(&self, a: Rect, pane: bool) -> SpecBox {
        let col_x = a.x + a.w - PEAK_W;
        let title = Rect::new(a.x + AXIS_W, a.y, a.w - AXIS_W - PEAK_W - 8, TITLE_H);
        let plot = Rect::new(title.x, a.y + TITLE_H, title.w, a.h - self.spec_wf_h(a, pane) - TITLE_H - XLABEL_H);
        let sort = if pane { Rect::new(col_x, a.y + 2, PEAK_W - 50, 36) } else { Rect::new(col_x, a.y + 46, PEAK_W - 6, 36) };
        let y = sort.y + sort.h + 2;
        SpecBox { title, plot, sort, list: Rect::new(col_x, y, PEAK_W - 6, a.y + a.h - y) }
    }

    /// Lines shown on the trace and in the list: the numbers must match, so no more than the list has room for.
    fn peaks_shown(&self, b: &SpecBox) -> usize {
        self.azr.peaks.len().min((b.list.h / 18).max(0) as usize)
    }

    fn sort_name(&self) -> &'static str {
        if self.m.spec_sort == 0 { "Peaks by frequency" } else { "Peaks by level" }
    }

    pub(super) fn sort_toggle(&mut self) {
        self.m.spec_sort = 1 - self.m.spec_sort.min(1);
        self.peaks_sort();
    }

    /// Title, grid and axis labels.
    pub(super) fn draw_spec_frame(&self, cv: &mut Canvas, b: &SpecBox, sp: &spectrum::Spectrum) {
        let f = &self.fonts;
        let g = b.plot;
        let src = self.m.spec_src;
        let name = match src {
            WaveId::Ch(c) => self.ch_name(c),
            WaveId::Ref(i) => format!("R{}", i + 1),
        };
        let x = cv.text(&f.sb, b.title.x, b.title.y + 14, &name, self.wave_color(src));
        let mut t = format!("{:.0} dB/div  \u{b7}  Ref {:.0} dBV  \u{b7}  RBW {}", DB_PER_DIV, self.m.spec_ref, si_short(sp.rbw, "Hz"));
        if self.azr.limit.is_some() {
            t.push_str(&format!("  \u{b7}  Limit: {} of {} over", self.azr.limit_over, self.azr.limit_n));
        }
        cv.text(&f.s, x + 10, b.title.y + 14, &t, LABEL);
        chart_grid(cv, g, HDIV as i32, VDIV as i32);
        // every division while the labels have room
        let every = if g.h as f32 / VDIV < 14.0 { 2 } else { 1 };
        for j in (0..=VDIV as i32).step_by(every) {
            let s = format!("{:.0}", self.m.spec_ref - j as f32 * DB_PER_DIV);
            let y = (g.y + j * (g.h - 1) / VDIV as i32 + 4).clamp(g.y + 9, g.y + g.h);
            cv.text(&f.s, g.x - 5 - f.s.width(&s), y, &s, AXIS_TEXT);
        }
        let (f0, f1) = (sp.f0, sp.freq(sp.dbv.len().saturating_sub(1)));
        // from the right, so the end of the band always has its label; one that would touch the last is left out
        let mut free = g.x + g.w;
        for i in (0..=HDIV as i32).rev() {
            let s = si_short(f0 + (f1 - f0) * i as f64 / HDIV as f64, "Hz");
            let w = f.s.width(&s);
            let x = (g.x + i * (g.w - 1) / HDIV as i32 - w / 2).clamp(g.x - AXIS_W + 2, g.x + g.w - w);
            if x + w + 10 <= free || i == HDIV as i32 {
                cv.text(&f.s, x, g.y + g.h + 13, &s, AXIS_TEXT);
                free = x;
            }
        }
    }

    /// Numbered tags on the trace, the same numbers as in the list. Tags that would collide move up.
    pub(super) fn draw_spec_marks(&self, cv: &mut Canvas, b: &SpecBox, sp: &spectrum::Spectrum, top: f32) {
        let f = &self.fonts;
        let g = b.plot;
        let span = sp.df * (sp.dbv.len().max(2) - 1) as f64;
        if !(span > 0.0) {
            return;
        }
        cv.set_clip(Rect::new(g.x, g.y, g.w + 8, g.h));
        let mut placed: Vec<Rect> = Vec::new();
        for (k, m) in self.azr.peaks.iter().take(self.peaks_shown(b)).enumerate() {
            let x = g.x + ((m.f - sp.f0) / span * (g.w - 1) as f64).round() as i32;
            let y = (g.y as f32 + (top - m.dbv as f32) / DB_PER_DIV * (g.h as f32 / VDIV)) as i32;
            let mut r = Rect::new(x - 7, (y - 20).max(g.y + 1), 15, 14);
            while placed.iter().any(|p| overlaps(*p, r)) {
                r.y += 15;
            }
            placed.push(r);
            cv.vline(x, r.y + r.h, y.max(r.y + r.h), ACCENT);
            cv.round(r, 3, ACCENT, 255);
            cv.text_in(&f.sb, r, 1, &format!("{}", k + 1), DARK_TEXT);
        }
        cv.no_clip();
    }

    /// Facts beside the peaks in the list: first what the view would otherwise not say.
    fn spec_facts(&self, sp: &spectrum::Spectrum) -> Vec<(String, String)> {
        let db = |x: f64, u: &str| format!("{:.1} {}", x, u);
        let mut v = Vec::new();
        if self.azr.limit.is_some() {
            v.push(("Limit".to_string(), format!("{} of {} over", self.azr.limit_over, self.azr.limit_n)));
            if let Some((hz, over)) = self.azr.limit_worst {
                v.push(("Worst".to_string(), format!("+{:.1} dB {}", over, si_short(hz, "Hz"))));
            }
        }
        if let Some(d) = &self.azr.dist {
            v.push(("f1".to_string(), format!("{} {}", si(d.f1, "Hz"), db(d.a1_dbv, "dBV"))));
            v.push(("THD".to_string(), format!("{} ({:.3} %)", db(d.thd_db, "dB"), d.thd_pct)));
            v.push(("THD+N".to_string(), db(d.thdn_db, "dB")));
            v.push(("SNR".to_string(), db(d.snr_db, "dB")));
            v.push(("SINAD".to_string(), db(d.sinad_db, "dB")));
            v.push(("SFDR".to_string(), db(d.sfdr_dbc, "dBc")));
            v.push(("ENOB".to_string(), format!("{:.2} bit", d.enob)));
        }
        v.push(("Noise floor".to_string(), db(sp.floor_dbv, "dBV")));
        let stop = sp.freq(sp.dbv.len().saturating_sub(1));
        v.push(("Band".to_string(), format!("{} to {}", si_short(sp.f0, "Hz"), si_short(stop, "Hz"))));
        v.push(("Window".to_string(), self.m.spec_win.name().to_string()));
        v.push(("Trace".to_string(), ["Normal", "Average", "Max hold", "Min hold"][self.m.spec_trace.min(3)].to_string()));
        v
    }

    /// The list: sort button, one row per line with a bar for its height over the noise, facts in what is left. The rows
    /// share the whole height, so a short list is a large one, not a gap.
    pub(super) fn draw_peaks(&self, cv: &mut Canvas, b: &SpecBox, sp: &spectrum::Spectrum, pane: bool) {
        let f = &self.fonts;
        btn(cv, b.sort, BTN, BTN_BORDER);
        cv.text_in(&f.mb, b.sort, 1, self.sort_name(), TEXT);
        let r = b.list;
        let n = self.peaks_shown(b) as i32;
        let facts = self.spec_facts(sp);
        let nf = (facts.len() as i32).min((r.h - n * 20) / 17).max(0);
        // a line gets up to two text rows, the facts the rest
        let pitch = if n > 0 { ((r.h - nf * 17) / n).min(if nf < facts.len() as i32 { r.h } else { 46 }) } else { 0 };
        let top = self.azr.peaks.iter().take(n as usize).map(|m| m.dbv).fold(f64::MIN, f64::max);
        let strongest = self.azr.peaks.iter().map(|m| m.dbv).fold(f64::MIN, f64::max);
        for (k, m) in self.azr.peaks.iter().take(n as usize).enumerate() {
            let y = r.y + k as i32 * pitch;
            let row = Rect::new(r.x, y, r.w, pitch);
            if k % 2 == 1 {
                cv.fill(row, ROW_ALT);
            }
            let tag = Rect::new(r.x + 2, y + (pitch.min(20) - 14) / 2, 15, 14);
            cv.round(tag, 3, ACCENT, 255);
            cv.text_in(&f.sb, tag, 1, &format!("{}", k + 1), DARK_TEXT);
            let ty = tag.y + 12;
            cv.text(&f.sb, r.x + 22, ty, &si_digits(m.f, "Hz", 5), TEXT);
            let s = format!("{:.1} dBV", m.dbv);
            cv.text(&f.s, r.x + r.w - 4 - f.s.width(&s), ty, &s, LABEL);
            // height over the noise on the scale of the list
            let span = (top - sp.floor_dbv).max(1.0);
            let w = (((m.dbv - sp.floor_dbv) / span).clamp(0.0, 1.0) * (r.w - 26) as f64) as i32;
            if pitch >= 36 {
                let s = if m.dbv == strongest { "strongest".to_string() } else { format!("{:.1} dBc", m.dbv - strongest) };
                cv.text(&f.s, r.x + 22, ty + 15, &s, DIM);
            }
            cv.fill(Rect::new(r.x + 22, y + pitch - 3, w, 2), scale(ACCENT, 150));
        }
        if n == 0 {
            cv.text(&f.s, r.x + 4, r.y + 14, "No lines above the noise", DIM);
        }
        let y0 = r.y + n * pitch + if n == 0 { 20 } else { 0 };
        let room = r.y + r.h - y0;
        let step = if nf > 0 { (room / nf).min(if pane { 40 } else { 24 }) } else { 0 };
        for (k, (l, v)) in facts.iter().take(nf as usize).enumerate() {
            let y = y0 + k as i32 * step + (step + 9) / 2;
            cv.text(&f.s, r.x + 4, y, l, DIM);
            cv.text(&f.s, r.x + r.w - 4 - f.s.width(v), y, v, LABEL);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// results

/// Columns of the results table: heading, relative width. The last one is a bar and goes first when room is short.
const RESULT_COLS: [(&str, i32); 9] = [("Source", 54), ("Measurement", 136), ("Now", 88), ("Mean", 88), ("Min", 88), ("Max", 88), ("\u{3c3}", 80), ("Count", 60), ("Range", 90)];

/// Left edges of the columns in a table `w` wide, and the end.
fn result_cols(w: i32) -> Vec<i32> {
    let sum: i32 = RESULT_COLS.iter().map(|c| c.1).sum();
    let bar = w >= sum;
    let used = sum - if bar { 0 } else { RESULT_COLS[8].1 };
    let mut x = 0;
    let mut v = vec![0];
    for c in &RESULT_COLS[..if bar { 9 } else { 8 }] {
        x += c.1 * w / used;
        v.push(x);
    }
    v
}

#[cfg(test)]
pub(super) fn tests_result_cols(w: i32) -> Vec<i32> {
    result_cols(w)
}

/// The cells of a results row.
struct ResultRow {
    ch: WaveId,
    cells: [String; 8],
    now: f64,
    lo: f64,
    hi: f64,
}

impl Ui {
    /// Rows of the side column of the Results pane: cursors, peak list.
    fn results_side_rows(&self) -> i32 {
        let cur = match self.m.cursor {
            CursorMode::Off => 0,
            CursorMode::Time | CursorMode::Volt => 3,
            _ => 4,
        };
        cur + if self.wave_ready(self.m.spec_src) { 5 } else { 0 }
    }

    /// Height the current pane wants.
    pub(super) fn pane_want(&self) -> i32 {
        let n = (self.m.meas.len() + self.m.meas2.len()) as i32;
        match self.m.pane {
            tables::PANE_RESULTS => {
                let side = self.results_side_rows();
                if n == 0 && side == 0 {
                    return HINT_H;
                }
                (BAND_H + n.max(1) * ROW_WANT).max(BAND_H + side * 18)
            }
            tables::PANE_EVENTS if self.pn.event_rows > 0 => self.pn.event_rows as i32 * 24,
            tables::PANE_EVENTS => HINT_H,
            tables::PANE_TREND if n == 0 => HINT_H,
            tables::PANE_SPECTRUM if !self.wave_ready(self.m.spec_src) => HINT_H,
            _ => i32::MAX / 2,
        }
    }

    /// Per frame: the slow content of the pane, a few times a second.
    pub(super) fn pane_tick(&mut self) {
        if self.m.pane != tables::PANE_EVENTS || (self.now - self.pn.events_t).abs() < 0.25 {
            return;
        }
        self.pn.events_t = self.now;
        self.pn.events = self.decode_events();
        let n = self.pn.events.len();
        let want = n.div_ceil(if n <= 4 { 1 } else { 2 });
        if want >= self.pn.event_rows {
            (self.pn.event_rows, self.pn.event_seen) = (want, self.now);
        } else if self.now - self.pn.event_seen > SHRINK_AFTER {
            self.pn.event_rows = want;
        }
    }

    fn result_rows(&self) -> Vec<ResultRow> {
        self.meas_rows()
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let st = self.mstat.get(i).copied().unwrap_or_default();
                let n = st.n.max(1) as f64;
                let mean = st.sum / n;
                let sd = (st.sumsq / n - mean * mean).max(0.0).sqrt();
                let val = |v: f64| if st.n == 0 { "----".to_string() } else { r.fmt4(v) };
                // a pair names both channels
                let src = self.m.meas2.get(i.wrapping_sub(self.m.meas.len())).map_or(self.wave_name(r.src), |p| format!("{}-{}", self.wave_name(p.1), self.wave_name(p.2)));
                ResultRow { ch: r.src, cells: [src, r.name.clone(), r.fmt4(r.v), val(mean), val(st.min), val(st.max), val(sd), fmt_count(st.n as f64)], now: r.v, lo: st.min, hi: st.max }
            })
            .collect()
    }

    /// Band, table rectangle, side column of the pane in `r`.
    fn results_box(&self, r: Rect) -> (Rect, Option<Rect>) {
        let side = self.results_side_rows() > 0;
        let w = if side { r.w - SIDE_W - 8 } else { r.w };
        (Rect::new(r.x, r.y, w, r.h), side.then(|| Rect::new(r.x + r.w - SIDE_W, r.y + BAND_H, SIDE_W - 6, r.h - BAND_H)))
    }

    /// Rows that fit, and their pitch: the rows share the height of the pane.
    pub(super) fn results_fit(r: Rect, n: usize) -> (usize, i32) {
        let room = r.h - BAND_H;
        let fit = ((room / ROW_MIN).max(1) as usize).min(n.max(1));
        (fit, room / fit as i32)
    }

    pub(super) fn results_scroll_btns(r: Rect) -> [Rect; 2] {
        let b = |i: i32| Rect::new(r.x + r.w - 46 * i, r.y + 2, 42, 36);
        [b(3), b(2)]
    }

    fn results_scroll(&mut self, d: i32) {
        let n = self.meas_rows().len();
        let r = self.pane_rect();
        let (fit, _) = Self::results_fit(r, n);
        let last = n.saturating_sub(fit);
        self.pn.top = (self.pn.top.min(last) as i32 + d).clamp(0, last as i32) as usize;
    }

    fn draw_results_pane(&self, cv: &mut Canvas, r: Rect) {
        let f = &self.fonts;
        let rows = self.result_rows();
        let (t, side) = self.results_box(r);
        if rows.is_empty() {
            cv.text(&f.sb, t.x + 12, t.y + (r.h + 9) / 2, "No measurements: Measure > Add item", DIM);
        } else {
            self.draw_result_table(cv, r, t, &rows);
        }
        if let Some(s) = side {
            cv.vline(s.x - 5, r.y + 4, r.y + r.h - 5, LINE);
            self.draw_results_side(cv, s);
        }
    }

    fn draw_result_table(&self, cv: &mut Canvas, r: Rect, t: Rect, rows: &[ResultRow]) {
        let f = &self.fonts;
        let (fit, pitch) = Self::results_fit(r, rows.len());
        let top = self.pn.top.min(rows.len() - fit);
        let band = format!("Measurements  \u{b7}  {}", rows.len());
        cv.text(&f.sb, t.x + 12, t.y + 16, &band, TEXT);
        if rows.len() > fit {
            let s = format!("rows {} to {}", top + 1, top + fit);
            cv.text(&f.s, t.x + 150, t.y + 16, &s, DIM);
        }
        let x = result_cols(t.w - 16);
        let x0 = t.x + 12;
        let head_y = t.y + BAND_H - 6;
        for (k, c) in RESULT_COLS.iter().enumerate().take(x.len() - 1) {
            // words on the left, numbers on the right of their column
            let tx = if k < 2 || k == 8 { x0 + x[k] } else { x0 + x[k + 1] - 8 - f.s.width(c.0) };
            cv.text(&f.s, tx, head_y, c.0, DIM);
        }
        cv.hline(t.x + 6, t.x + t.w - 6, t.y + BAND_H - 1, LINE);
        // the largest font every cell of a column fits in
        let col_w = (2..7).map(|k| x[k + 1] - x[k] - 6).min().unwrap_or(40);
        let shown = &rows[top..top + fit];
        let fonts: &[&Font] = if pitch >= 40 { &[&f.lb, &f.mb, &f.sb] } else if pitch >= 24 { &[&f.mb, &f.sb] } else { &[&f.sb] };
        let vf = fit_font(fonts, col_w, shown.iter().map(|w| w.cells[2].as_str()));
        let stats: &[&Font] = if pitch >= 24 { &[&f.m, &f.s] } else { &[&f.s] };
        let plain = fit_font(stats, col_w, shown.iter().flat_map(|w| w.cells[3..7].iter().map(|s| s.as_str())));
        for (i, w) in shown.iter().enumerate() {
            let y = t.y + BAND_H + i as i32 * pitch;
            let row = Rect::new(t.x, y, t.w, pitch);
            if i % 2 == 1 {
                cv.fill(row, ROW_ALT);
            }
            let col = self.wave_color(w.ch);
            cv.fill(Rect::new(t.x + 2, y + 1, 3, pitch - 2), col);
            let ty = y + (pitch + vf.size * 3 / 4) / 2;
            cv.text(&f.sb, x0 + x[0], ty, &w.cells[0], col);
            let name_font = if plain.width(&w.cells[1]) <= x[2] - x[1] - 6 { plain } else { &f.s };
            cv.text(name_font, x0 + x[1], ty, &w.cells[1], LABEL);
            for k in 2..8 {
                let (font, c) = match k {
                    2 => (vf, col),
                    7 => (&f.s, DIM),
                    _ => (plain, LABEL),
                };
                cv.text(font, x0 + x[k + 1] - 8 - font.width(&w.cells[k]), ty, &w.cells[k], c);
            }
            if x.len() > 9 {
                Self::range_bar(cv, Rect::new(x0 + x[8], y + pitch / 2 - 3, x[9] - x[8] - 6, 6), w, col);
            }
        }
    }

    /// Where the value stands between the smallest and the largest seen.
    fn range_bar(cv: &mut Canvas, b: Rect, w: &ResultRow, col: Rgb) {
        cv.fill(Rect::new(b.x, b.y + b.h / 2, b.w, 1), LINE);
        cv.vline(b.x, b.y, b.y + b.h, LINE);
        cv.vline(b.x + b.w - 1, b.y, b.y + b.h, LINE);
        if w.hi > w.lo && w.now.is_finite() {
            let x = b.x + (((w.now - w.lo) / (w.hi - w.lo)).clamp(0.0, 1.0) * (b.w - 3) as f64) as i32;
            cv.fill(Rect::new(x, b.y - 1, 3, b.h + 2), col);
        }
    }

    /// Cursor read-out and peak list beside the table; their rows share the height.
    fn draw_results_side(&self, cv: &mut Canvas, s: Rect) {
        let f = &self.fonts;
        let cur = self.cursor_values();
        let peaks = self.azr.spec.as_ref().map(|_| &self.azr.peaks);
        let cur_rows = if cur.is_empty() { 0 } else { 1 + cur.len().div_ceil(2) as i32 };
        let peak_rows = peaks.map_or(0, |p| 1 + (p.len().div_ceil(2) as i32).max(1));
        let rows = cur_rows + peak_rows;
        if rows == 0 {
            return;
        }
        let pitch = s.h / rows;
        let half = s.w / 2;
        let line = |k: i32| s.y + k * pitch + (pitch + 9) / 2;
        if !cur.is_empty() {
            let mode = match self.m.cursor {
                CursorMode::Volt => "Volt",
                CursorMode::Track => "Track",
                CursorMode::Xy => "XY",
                _ => "Time",
            };
            cv.text(&f.sb, s.x, line(0), &format!("Cursors  \u{b7}  {}  \u{b7}  {}", mode, self.ch_name(self.m.active)), TEXT);
            for (i, (l, v)) in cur.iter().enumerate() {
                let (x, y) = (s.x + (i as i32 % 2) * half, line(1 + i as i32 / 2));
                cv.text(&f.s, x, y, l, DIM);
                cv.text(&f.sb, x + half - 8 - f.sb.width(v), y, v, TEXT);
            }
        }
        let Some(p) = peaks else { return };
        let y0 = cur_rows;
        let spec = match self.m.spec_src {
            WaveId::Ch(c) => self.ch_name(c),
            WaveId::Ref(i) => format!("R{}", i + 1),
        };
        cv.text(&f.sb, s.x, line(y0), &format!("Peaks  \u{b7}  {}  \u{b7}  dBV", spec), TEXT);
        if p.is_empty() {
            cv.text(&f.s, s.x, line(y0 + 1), "No lines above the noise", DIM);
        }
        let per = p.len().div_ceil(2).max(1);
        for (i, m) in p.iter().enumerate() {
            let (x, y) = (s.x + (i / per) as i32 * half, line(y0 + 1 + (i % per) as i32));
            let tag = Rect::new(x, y - 11, 15, 14);
            cv.round(tag, 3, ACCENT, 255);
            cv.text_in(&f.sb, tag, 1, &format!("{}", i + 1), DARK_TEXT);
            cv.text(&f.s, x + 19, y, &si_digits(m.f, "Hz", 4), LABEL);
            let v = format!("{:.1}", m.dbv);
            cv.text(&f.sb, x + half - 8 - f.sb.width(&v), y, &v, TEXT);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// trend

/// Points of a trend, and the time one covers at the start: when it is full, neighbours merge and the time doubles, so a
/// trend reaches back as far as the scope has run.
const TREND_PTS: usize = 480;
const TREND_STEP: f64 = 0.1;
/// Values of a chart's scale: the fraction cut off at each end (a stray record must not set the scale), the margin, the
/// smallest span relative to the value (the last digits are no signal), the hold before it tightens (points).
const SCALE_CUT: f64 = 0.02;
const SCALE_MARGIN: f64 = 0.15;
const SCALE_MIN_REL: f64 = 1e-3;
const SCALE_HOLD: u32 = 30;
/// Time axes of a chart, s: fifths of them are round numbers.
const TREND_WINDOWS: [f64; 16] = [10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 75.0, 100.0, 150.0, 200.0, 300.0, 600.0, 1200.0, 3600.0];
/// Width of the value card and of the y labels of a chart.
const CARD_W: i32 = 128;
const YLABEL_W: i32 = 68;

/// A stretch of time of a trend: its mean, smallest and largest value.
#[derive(Clone, Copy, Debug)]
pub struct TrendPt {
    pub t: f64,
    pub v: f32,
    pub lo: f32,
    pub hi: f32,
}

/// History of one measurement.
#[derive(Clone, Debug)]
pub struct Trend {
    pub pts: VecDeque<TrendPt>,
    step: f64,
    /// open point: start, sum, count, smallest, largest
    open: (f64, f64, u32, f32, f32),
    /// the scale of the chart: bottom, top, tick distance
    pub scale: Option<(f64, f64, f64)>,
    /// points the data has been far inside the scale
    slack: u32,
    /// smallest and largest value of the history
    pub range: Option<(f64, f64)>,
}

impl Default for Trend {
    fn default() -> Trend {
        Trend { pts: VecDeque::new(), step: TREND_STEP, open: (f64::NAN, 0.0, 0, f32::MAX, f32::MIN), scale: None, slack: 0, range: None }
    }
}

impl Trend {
    /// A new value at time `t`.
    pub fn push(&mut self, t: f64, v: f64) {
        if self.open.0.is_nan() {
            self.open.0 = t;
        }
        if t - self.open.0 >= self.step {
            self.close();
            self.open.0 = t;
        }
        if v.is_finite() {
            let o = &mut self.open;
            (o.1, o.2, o.3, o.4) = (o.1 + v, o.2 + 1, o.3.min(v as f32), o.4.max(v as f32));
        }
    }

    /// The open point goes into the history (a stretch without a value is a gap), the scale follows.
    fn close(&mut self) {
        let (t, sum, n, lo, hi) = self.open;
        let p = if n > 0 { TrendPt { t, v: (sum / n as f64) as f32, lo, hi } } else { TrendPt { t, v: f32::NAN, lo: f32::NAN, hi: f32::NAN } };
        self.pts.push_back(p);
        self.open = (t, 0.0, 0, f32::MAX, f32::MIN);
        if self.pts.len() >= TREND_PTS {
            self.merge();
        }
        self.rescale();
    }

    fn merge(&mut self) {
        let old = std::mem::take(&mut self.pts);
        let mut it = old.into_iter();
        while let Some(a) = it.next() {
            let p = match it.next() {
                Some(b) if a.v.is_nan() => b,
                Some(b) if b.v.is_nan() => a,
                Some(b) => TrendPt { t: a.t, v: (a.v + b.v) / 2.0, lo: a.lo.min(b.lo), hi: a.hi.max(b.hi) },
                None => a,
            };
            self.pts.push_back(p);
        }
        self.step *= 2.0;
    }

    /// Scale of the chart: the data without its extremes and a margin, on round ticks. It widens at once, and tightens
    /// only when the data has used less than half of it for a while, so noise does not make it jump.
    fn rescale(&mut self) {
        let mut v: Vec<f32> = self.pts.iter().map(|p| p.v).filter(|x| x.is_finite()).collect();
        if v.is_empty() {
            return;
        }
        self.range = self.pts.iter().filter(|p| p.lo.is_finite()).fold(None, |r: Option<(f64, f64)>, p| Some(r.map_or((p.lo as f64, p.hi as f64), |(a, b)| (a.min(p.lo as f64), b.max(p.hi as f64)))));
        // where the data is now: the middle of the last three, a single stray point is not that
        let mut end: Vec<f32> = v[v.len().saturating_sub(3)..].to_vec();
        end.sort_by(f32::total_cmp);
        let last = end[end.len() / 2] as f64;
        v.sort_by(f32::total_cmp);
        let cut = (v.len() as f64 * SCALE_CUT) as usize;
        let (lo, hi) = ((v[cut] as f64).min(last), (v[v.len() - 1 - cut] as f64).max(last));
        let mid = (lo + hi) / 2.0;
        let span = (hi - lo).max(mid.abs() * SCALE_MIN_REL).max(1e-12);
        let (lo, hi) = (mid - span * (0.5 + SCALE_MARGIN), mid + span * (0.5 + SCALE_MARGIN));
        let snap = |lo: f64, hi: f64| {
            let tick = round_step((hi - lo) / 4.0);
            ((lo / tick).floor() * tick, (hi / tick).ceil() * tick, tick)
        };
        match self.scale {
            None => self.scale = Some(snap(lo, hi)),
            Some((a, b, _)) if lo < a || hi > b => {
                self.scale = Some(snap(lo.min(a), hi.max(b)));
                self.slack = 0;
            }
            Some((a, b, _)) if hi - lo < 0.5 * (b - a) => {
                self.slack += 1;
                if self.slack >= SCALE_HOLD {
                    self.scale = Some(snap(lo, hi));
                    self.slack = 0;
                }
            }
            _ => self.slack = 0,
        }
    }
}

impl Ui {
    /// New measurement values for the trends; `fresh` means the list changed or was reset.
    pub(super) fn trend_update(&mut self, fresh: bool, rows: &[tools::MeasRow]) {
        if fresh {
            self.trend.clear();
        }
        self.trend.resize(rows.len(), Trend::default());
        for (t, r) in self.trend.iter_mut().zip(rows) {
            t.push(self.now, r.v);
        }
    }

    /// Choices of a chart: by the list, off, the measurements.
    pub(super) fn trend_options(&self) -> Vec<String> {
        let mut v = vec!["Auto".to_string(), "Off".to_string()];
        // a pair says its channels in its name
        let single = self.m.meas.len();
        v.extend(self.meas_rows().iter().enumerate().map(|(i, r)| if i < single { format!("{} {}", self.wave_name(r.src), r.name) } else { r.name.clone() }));
        v
    }

    /// Rows the charts show, in chart order: (chart, row).
    pub(super) fn trend_charts(&self) -> Vec<(usize, usize)> {
        let n = self.meas_rows().len();
        (0..2)
            .filter_map(|k| match self.m.trend_sel[k] {
                1 => None,
                s if s >= 2 && s - 2 < n => Some((k, s - 2)),
                _ => (k < n).then_some((k, k)),
            })
            .collect()
    }

    /// The charts side by side.
    fn chart_rects(r: Rect, n: usize) -> Vec<Rect> {
        let w = (r.w - 8 * (n as i32 - 1).max(0)) / (n as i32).max(1);
        (0..n as i32).map(|i| Rect::new(r.x + i * (w + 8), r.y, w, r.h)).collect()
    }

    fn draw_trends(&self, cv: &mut Canvas, r: Rect) {
        let f = &self.fonts;
        let rows = self.meas_rows();
        let charts = self.trend_charts();
        if rows.is_empty() || charts.is_empty() {
            let s = if rows.is_empty() { "No measurements: Measure > Add item" } else { "Both charts off: Display > Trend chart 1, 2" };
            cv.text(&f.sb, r.x + 12, r.y + (r.h + 9) / 2, s, DIM);
            return;
        }
        for (c, &(_, row)) in Self::chart_rects(r, charts.len()).into_iter().zip(&charts) {
            self.draw_chart(cv, c, &rows[row], self.trend.get(row));
        }
    }

    /// One chart: title and plot on the left, a card with the value on the right.
    fn draw_chart(&self, cv: &mut Canvas, c: Rect, row: &tools::MeasRow, t: Option<&Trend>) {
        let f = &self.fonts;
        let col = self.wave_color(row.src);
        let x = cv.text(&f.sb, c.x + 8, c.y + 15, &self.wave_name(row.src), col);
        cv.text(&f.sb, x + 8, c.y + 15, &row.name, TEXT);
        let g = Rect::new(c.x + YLABEL_W, c.y + TITLE_H, c.w - YLABEL_W - CARD_W - 8, c.h - TITLE_H - XLABEL_H);
        let card = Rect::new(c.x + c.w - CARD_W - 2, c.y + 2, CARD_W, c.h - 4);
        self.draw_trend_card(cv, card, row, t);
        let Some((t, (lo, hi, tick))) = t.and_then(|t| t.scale.map(|s| (t, s))) else {
            chart_grid(cv, g, 5, 4);
            cv.text_in(&f.s, g, 1, "collecting values", DIM);
            return;
        };
        let now = t.pts.back().map_or(0.0, |p| p.t);
        let age = now - t.pts.front().map_or(now, |p| p.t);
        let window = TREND_WINDOWS.iter().copied().find(|&w| w >= age).unwrap_or((age / 3600.0).ceil() * 3600.0);
        chart_grid(cv, g, 5, 1);
        let y_of = |v: f64| g.y + g.h - 1 - ((v - lo) / (hi - lo) * (g.h - 1) as f64).round() as i32;
        let big = lo.abs().max(hi.abs()) as f32;
        let mut v = (lo / tick).ceil() * tick;
        while v <= hi + tick * 1e-6 {
            let y = y_of(v);
            cv.hline(g.x + 1, g.x + g.w - 2, y, GRID_LINE);
            let s = axis_label(v as f32, tick as f32, 0.0, big, row.unit);
            cv.text(&f.s, g.x - 5 - f.s.width(&s), (y + 4).clamp(g.y + 9, g.y + g.h), &s, AXIS_TEXT);
            v += tick;
        }
        for i in 0..=5 {
            let s = if i == 5 { "now".to_string() } else { format!("-{:.0} s", window * (5 - i) as f64 / 5.0) };
            let w = f.s.width(&s);
            let x = g.x + i * (g.w - 1) / 5 - w / 2;
            cv.text(&f.s, x.clamp(g.x, g.x + g.w - w), g.y + g.h + 13, &s, AXIS_TEXT);
        }
        cv.set_clip(Rect::new(g.x + 1, g.y + 1, g.w - 2, g.h - 2));
        let x_of = |tp: f64| g.x + g.w - 1 - ((now - tp) / window * (g.w - 1) as f64).round() as i32;
        let band = mix(BG, col, 80);
        let mut prev: Option<(i32, i32)> = None;
        for p in &t.pts {
            if !p.v.is_finite() {
                prev = None;
                continue;
            }
            let x = x_of(p.t);
            cv.vline(x, y_of(p.hi as f64), y_of(p.lo as f64), band);
            let q = (x, y_of(p.v as f64));
            if let Some(o) = prev {
                cv.line(o.0, o.1, q.0, q.1, col);
            }
            prev = Some(q);
        }
        cv.no_clip();
    }

    /// The value large, under it largest and smallest of the history; the rows share the card.
    fn draw_trend_card(&self, cv: &mut Canvas, r: Rect, row: &tools::MeasRow, t: Option<&Trend>) {
        let f = &self.fonts;
        cv.round(r, 5, BAR, 255);
        // the close button may be over the top right corner of the card
        cv.text(&f.s, r.x + 8, r.y + 18, "Now", DIM);
        let now = row.fmt4(row.v);
        let font = fit_font(&[&f.xl, &f.lb, &f.mb], r.w - 12, std::iter::once(now.as_str()));
        let body = r.h - 40;
        let col = self.wave_color(row.src);
        cv.text(font, r.x + r.w - 6 - font.width(&now), r.y + 40 + body / 3 - 4, &now, col);
        let (lo, hi) = t.and_then(|t| t.range).map_or(("----".to_string(), "----".to_string()), |(a, b)| (row.fmt4(a), row.fmt4(b)));
        for (k, (l, v)) in [("Max", hi), ("Min", lo)].iter().enumerate() {
            let y = r.y + 40 + body / 3 + (k as i32 + 1) * (body * 2 / 3) / 3 + 4;
            cv.text(&f.s, r.x + 8, y, l, DIM);
            cv.text(&f.sb, r.x + r.w - 8 - f.sb.width(v), y, v, LABEL);
        }
    }

    /// A tap on a chart: its next measurement.
    fn trend_tap(&mut self, x: i32, y: i32) {
        let n = self.meas_rows().len();
        let charts = self.trend_charts();
        if n == 0 {
            return;
        }
        for (c, &(k, row)) in Self::chart_rects(self.pane_rect(), charts.len()).iter().zip(&charts) {
            if c.contains(x, y) {
                self.m.trend_sel[k] = (row + 1) % n + 2;
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------------
    // the pane as a whole

    /// The content of the pane.
    pub(super) fn draw_pane(&self, cv: &mut Canvas) {
        let r = self.pane_rect();
        cv.fill(r, BG);
        cv.hline(r.x, r.x + r.w - 1, r.y, LINE);
        match self.m.pane {
            tables::PANE_SPECTRUM => self.draw_spectrum(cv, r, true),
            _ => {
                cv.set_clip(r);
                match self.m.pane {
                    tables::PANE_RESULTS => self.draw_results_pane(cv, r),
                    tables::PANE_EVENTS => self.draw_pane_events(cv, r),
                    tables::PANE_TREND => self.draw_trends(cv, r),
                    _ => {}
                }
                cv.no_clip();
            }
        }
        if self.m.pane == tables::PANE_RESULTS {
            let (fit, _) = Self::results_fit(r, self.meas_rows().len());
            if self.meas_rows().len() > fit {
                for (b, t) in Self::results_scroll_btns(r).iter().zip(["Up", "Dn"]) {
                    btn(cv, *b, BTN, BTN_BORDER);
                    cv.text_in(&self.fonts.mb, *b, 1, t, TEXT);
                }
            }
        }
    }

    /// A tap on the pane, or on the sort button of the full view; true when it was used.
    pub(super) fn pane_tap(&mut self, x: i32, y: i32) -> bool {
        if self.m.view == AView::Spectrum && !self.xy && self.azr.spec.is_some() {
            let b = self.spec_box(self.area, false);
            if b.sort.contains(x, y) {
                self.sort_toggle();
                return true;
            }
        }
        if self.pane_h() == 0 {
            return false;
        }
        let r = self.pane_rect();
        if Self::pane_close(r).contains(x, y) {
            self.m.pane = 0;
            return true;
        }
        if !r.contains(x, y) {
            return false;
        }
        match self.m.pane {
            tables::PANE_SPECTRUM => {
                let b = self.spec_box(r, true);
                if b.sort.contains(x, y) {
                    self.sort_toggle();
                } else if b.title.contains(x, y) {
                    self.toggle_menu(MenuId::Spectrum);
                }
            }
            tables::PANE_RESULTS => {
                let (fit, _) = Self::results_fit(r, self.meas_rows().len());
                let [up, down] = Self::results_scroll_btns(r);
                if self.meas_rows().len() > fit && up.contains(x, y) {
                    self.results_scroll(-(fit as i32));
                } else if self.meas_rows().len() > fit && down.contains(x, y) {
                    self.results_scroll(fit as i32);
                }
            }
            tables::PANE_TREND => self.trend_tap(x, y),
            _ => {}
        }
        true
    }

    /// A finger drawn over the results table moves it by the rows it passed.
    pub(super) fn pane_swipe(&mut self, x0: i32, y0: i32, y1: i32) {
        if self.pane_h() > 0 && self.m.pane == tables::PANE_RESULTS && self.pane_rect().contains(x0, y0) {
            let (_, pitch) = Self::results_fit(self.pane_rect(), self.meas_rows().len());
            self.results_scroll((y0 - y1) / pitch.max(1));
        }
    }
}
