// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The eye and jitter page. `eye_update` runs per record: the eye and the cheap sums there, everything else (fits,
//! charts, the coloured eye, all text) at most every `REFRESH` seconds into `Page`, so drawing only copies and plots.
//!
//! TIE is pooled over records: each record is timed against its own clock, so its TIE stands alone, and a window with
//! few edges still adds up to a usable distribution. What drifts slower than one record is taken out by the fit.
//!
//! The right column (the four plots and the jitter figures) slides off the right edge; the tab on the eye brings it
//! back. A tap on a plot gives it the whole chart area, and a second tap restores the four.
//!
//! This file holds the constants, the layout, `Page` and the analyser entry; `page.rs` fills `Page`, `update.rs` runs
//! the page per record, `draw.rs` draws it, `automatic.rs` sets the eye up in one press. A new part goes into the file
//! of its step.

use super::magnetic::{missing_wave, WaveSamp};
use super::*;
use oos_dsp::eye::{self, Bathtub, EdgeMeans, Jitter, JitterFit};
use page::{populate_eye_facts, FactInput, FactSettings};

pub(super) mod automatic;
mod draw;
mod page;
#[cfg(test)]
mod tests;
mod update;

/// Seconds between refreshes of the page.
const REFRESH: f64 = 0.2;

/// Figures on the eye page, in the order the boxes draw them. The long form is what `:EYE:ITEM?` accepts
/// (the upper-case letters are the short form). A missing figure is stored as NaN.
pub const EYE_ITEMS: &[&str] = &[
    "ONE",
    "ZERO",
    "AMPLitude",
    "CROSsing",
    "RISe",
    "FALL",
    "HEIGht",
    "WIDTh",
    "Q",
    "HBER",
    "RECords",
    "EDGes",
    "UI",
    "SAMPling",
    "TPP",
    "TRMS",
    "DCD",
    "DDJ",
    "PJRms",
    "PRMS",
    "C2C",
    "F2",
    "F4",
    "F8",
    "SRJ",
    "PN",
    "TPRE",
    "TPOST",
    "PPRE",
    "PPOST",
    "DCCM",
    "ACCM",
    "XOVer",
    "TNT",
    "SSCPpm",
    "SSCRate",
    "TJ",
    "RJ",
    "DJ",
    "OPENing",
    "SEEN",
    "TJBer",
    "EWBer",
    "J2",
    "J9",
    "NPJ",
];

/// The figures `eye_facts` adds beside the ones `text_jitter` and `text_eye` already stored.
const EYE_FACTS: &[&str] = &[
    "HBER", "F2", "F4", "F8", "SRJ", "PN", "TPRE", "TPOST", "PPRE", "PPOST", "DCCM", "ACCM", "XOVer", "TNT", "SSCPpm",
    "SSCRate", "TJBer", "EWBer", "J2", "J9", "NPJ",
];
/// TIE values kept, the newest.
const POOL: usize = 16384;
/// UI of a record that go into the spectrum.
const SPECTRUM_UI: usize = 16384;
/// UI a record needs for a spectrum worth reading.
const SPECTRUM_MIN: usize = 64;
/// Spectral lines summed into PJ.
const PJ_LINES: usize = 8;
/// dB below the peak at the foot of the spectrum chart.
const SPECTRUM_DB: f32 = 60.0;
/// log10 BER at the foot of the bathtub chart.
const TUB_FOOT: f32 = -14.0;
/// A recovered UI this far from the last one is another signal: the page starts over.
const UI_CHANGE: f64 = 0.05;
/// Room above and below the signal in the eye, and how far it may leave the range or shrink in it before a new range.
const EYE_MARGIN: f64 = 0.15;
const EYE_OUT: f64 = 0.25;
const EYE_SHRUNK: f64 = 0.4;

const GAP: i32 = 4;
/// The workspace header owns Close/source/rate, outside the chart content.
const HEAD_H: i32 = 0;
const ROW: i32 = 15;
const BOX_HEAD: i32 = 25;
const CHART_TITLE: i32 = 17;
const CHART_FOOT: i32 = 14;
const CHART_GUTTER: i32 = 50;
const EYE_TITLE: i32 = 4;
const EYE_GUTTER: i32 = 50;
const EYE_FOOT: i32 = 18;
/// Under this height the text boxes give their room to the plots.
const BOXES_MIN_H: i32 = 300;
/// How long the right column takes to slide in or out.
const SLIDE: f64 = 0.22;
/// The tab that slides the column: wide enough for a finger, tall enough to see.
const TAB_W: i32 = 40;
const TAB_H: i32 = 52;

/// The right column of the page (ADR 0062): it slides from `from` to `to` (0 hidden, 1 shown), starting at `t`; `big`
/// is the one of the four plots that fills the chart area.
pub(super) struct Column {
    pub from: f32,
    pub to: f32,
    pub t: f64,
    pub big: Option<u8>,
}

impl Default for Column {
    fn default() -> Column {
        Column { from: 1.0, to: 1.0, t: 0.0, big: None }
    }
}

/// The rectangles of the page. They tile the area down to its bottom edge: the plots take what the text leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    pub eye: Rect,
    pub plot: Rect,
    pub head: Rect,
    pub charts: [Rect; 4],
    pub plots: [Rect; 4],
    pub fig: Rect,
    pub jit: Rect,
    pub fig_rows: i32,
    pub jit_rows: i32,
}

pub(super) fn layout(a: Rect, fill: bool) -> Layout {
    if fill {
        let empty = Rect::new(a.x, a.y, 0, 0);
        let plot = Rect::new(
            a.x + EYE_GUTTER,
            a.y + EYE_TITLE,
            (a.w - EYE_GUTTER - 16).max(1),
            (a.h - EYE_TITLE - EYE_FOOT).max(1),
        );
        return Layout {
            eye: a,
            plot,
            head: empty,
            charts: [empty; 4],
            plots: [empty; 4],
            fig: empty,
            jit: empty,
            fig_rows: 0,
            jit_rows: 0,
        };
    }
    // Narrow side panes put the chart column under the eye.
    if a.w < 700 {
        return layout_stacked(a);
    }
    let lw = a.w * 57 / 100;
    let (rx, rw) = (a.x + lw + GAP, a.w - lw - GAP);
    let bottom = a.y + a.h;
    let rows =
        |pct: i32, most: i32| if a.h < BOXES_MIN_H { 0 } else { ((a.h * pct / 100 - BOX_HEAD) / ROW).clamp(3, most) };
    let (fig_rows, jit_rows) = (rows(28, 8), rows(42, 16));
    let box_h = |r: i32| if r == 0 { 0 } else { BOX_HEAD + r * ROW };
    let (fh, jh) = (box_h(fig_rows), box_h(jit_rows));
    let fig = Rect::new(a.x, bottom - fh, lw, fh);
    let jit = Rect::new(rx, bottom - jh, rw, jh);
    let eye = Rect::new(a.x, a.y, lw, a.h - fh - if fh > 0 { GAP } else { 0 });
    // the tab on the right edge of the eye has its own margin
    let plot = Rect::new(
        eye.x + EYE_GUTTER,
        eye.y + EYE_TITLE,
        eye.w - EYE_GUTTER - 10 - (TAB_W - 4),
        eye.h - EYE_TITLE - EYE_FOOT,
    );
    let head = Rect::new(rx, a.y, rw - 54, HEAD_H);
    let (gy, gh) = (a.y + HEAD_H, a.h - HEAD_H - jh - if jh > 0 { GAP } else { 0 });
    let (cw, ch) = ((rw - GAP) / 2, (gh - GAP) / 2);
    let cell = |c: i32, r: i32| {
        let (x, y) = (rx + c * (cw + GAP), gy + r * (ch + GAP));
        let w = if c == 0 { cw } else { rx + rw - x };
        let h = if r == 0 { ch } else { gy + gh - y };
        Rect::new(x, y, w, h)
    };
    let charts = [cell(0, 0), cell(1, 0), cell(0, 1), cell(1, 1)];
    let plots = charts.map(|c| {
        Rect::new(
            c.x + CHART_GUTTER,
            c.y + CHART_TITLE,
            c.w - CHART_GUTTER - 10,
            (c.h - CHART_TITLE - CHART_FOOT).max(1),
        )
    });
    Layout { eye, plot, head, charts, plots, fig, jit, fig_rows, jit_rows }
}

/// `open` is how far the right column is in: 1 is `layout`, 0 has slid it off the right edge and given its room to the eye.
fn placed(a: Rect, open: f32) -> Layout {
    let home = layout(a, false);
    let open = open.clamp(0.0, 1.0);
    if open >= 0.999 {
        return home;
    }
    if a.w < 700 {
        // In a narrow side pane the charts are below the eye. Slide them out sideways,
        // and return their vertical space to the eye instead of widening an already full-width eye.
        let shift = ((1.0 - open) * a.w as f32).round() as i32;
        let grow = ((1.0 - open) * (a.y + a.h - home.eye.y - home.eye.h).max(0) as f32).round() as i32;
        let slide = |r: Rect| Rect::new(r.x + shift, r.y, r.w, r.h);
        return Layout {
            eye: Rect::new(home.eye.x, home.eye.y, home.eye.w, home.eye.h + grow),
            plot: Rect::new(home.plot.x, home.plot.y, home.plot.w, home.plot.h + grow),
            head: slide(home.head),
            charts: home.charts.map(slide),
            plots: home.plots.map(slide),
            ..home
        };
    }
    let edge = a.x + a.w;
    let travel = (edge - home.charts[0].x).max(0);
    let shift = ((1.0 - open) * travel as f32).round() as i32;
    let grow = ((1.0 - open) * (edge - (home.eye.x + home.eye.w)).max(0) as f32).round() as i32;
    let slide = |r: Rect| Rect::new(r.x + shift, r.y, r.w, r.h);
    let widen = |r: Rect| Rect::new(r.x, r.y, r.w + grow, r.h);
    Layout {
        eye: widen(home.eye),
        plot: widen(home.plot),
        head: slide(home.head),
        charts: home.charts.map(slide),
        plots: home.plots.map(slide),
        fig: widen(home.fig),
        jit: slide(home.jit),
        fig_rows: home.fig_rows,
        jit_rows: home.jit_rows,
    }
}

/// The chart cards and their plots. `big` gives that one plot the whole chart area.
fn charts_shown(l: &Layout, big: Option<u8>) -> ([Rect; 4], [Rect; 4]) {
    let mut charts = l.charts;
    let mut plots = l.plots;
    if let Some(k) = big {
        let k = (k as usize).min(3);
        let (x0, y0) = (l.charts[0].x, l.charts[0].y);
        let (x1, y1) = (l.charts[1].x + l.charts[1].w, l.charts[2].y + l.charts[2].h);
        charts[k] = Rect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1));
        plots[k] = Rect::new(
            charts[k].x + CHART_GUTTER,
            charts[k].y + CHART_TITLE,
            (charts[k].w - CHART_GUTTER - 6).max(1),
            (charts[k].h - CHART_TITLE - CHART_FOOT).max(1),
        );
    }
    (charts, plots)
}

/// The tab on the right edge of the eye. It stays below the overflow button.
fn eye_tab_at(a: Rect, l: &Layout) -> Rect {
    let edge = l.eye.x + l.eye.w;
    let x = (edge - TAB_W).clamp(a.x, (a.x + a.w - TAB_W).max(a.x));
    let (y0, y1) = (l.charts[0].y, l.charts[2].y + l.charts[2].h);
    // a narrow pane keeps the charts under the eye: the tab sits in the right margin of the eye, not over a chart
    let mid = if a.w < 700 { l.eye.y + (l.eye.h - TAB_H) / 2 } else { y0 + (y1 - y0 - TAB_H) / 2 };
    let y = mid.clamp(a.y + HEAD_H, (a.y + a.h - TAB_H).max(a.y + HEAD_H));
    Rect::new(x, y, TAB_W, TAB_H)
}

/// Eye across the pane, charts in a block under it. The figure tables stay on the wide page.
fn layout_stacked(a: Rect) -> Layout {
    let bottom = a.y + a.h;
    let charts_on = a.w >= 280 && a.h >= 240;
    let ch = if charts_on { (a.h * 42 / 100).clamp(120, 200) } else { 0 };
    let eye = Rect::new(a.x, a.y, a.w, (a.h - ch - if ch > 0 { GAP } else { 0 }).max(1));
    let plot = Rect::new(
        eye.x + EYE_GUTTER,
        eye.y + EYE_TITLE,
        (eye.w - EYE_GUTTER - 10 - if charts_on { TAB_W - 4 } else { 0 }).max(1),
        (eye.h - EYE_TITLE - EYE_FOOT).max(1),
    );
    let gy = eye.y + eye.h + if ch > 0 { GAP } else { 0 };
    let (cw, rh) = if ch > 0 { ((a.w - GAP) / 2, (ch - GAP) / 2) } else { (0, 0) };
    let cell = |c: i32, r: i32| {
        if ch == 0 {
            return Rect::new(a.x, bottom, 0, 0);
        }
        let (x, y) = (a.x + c * (cw + GAP), gy + r * (rh + GAP));
        let w = if c == 0 { cw } else { a.x + a.w - x };
        let h = if r == 0 { rh } else { bottom - y };
        Rect::new(x, y, w.max(1), h.max(1))
    };
    let charts = [cell(0, 0), cell(1, 0), cell(0, 1), cell(1, 1)];
    let plots = charts.map(|c| {
        if c.w < 40 {
            return Rect::new(c.x, c.y, 0, 0);
        }
        Rect::new(
            c.x + CHART_GUTTER,
            c.y + CHART_TITLE,
            (c.w - CHART_GUTTER - 10).max(1),
            (c.h - CHART_TITLE - CHART_FOOT).max(1),
        )
    });
    Layout {
        eye,
        plot,
        head: Rect::new(a.x, a.y, 0, 0),
        charts,
        plots,
        fig: Rect::new(a.x, bottom, 0, 0),
        jit: Rect::new(a.x, bottom, 0, 0),
        fig_rows: 0,
        jit_rows: 0,
    }
}

/// Voltage range for a signal between `lo` and `hi`: whole steps of 1, 2 or 5, about six of them. (low, high, step)
fn eye_range(lo: f64, hi: f64) -> (f64, f64, f64) {
    let m = EYE_MARGIN * (hi - lo);
    let (lo, hi) = (lo - m, hi + m);
    let raw = (hi - lo) / 6.0;
    let d = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * d).find(|&x| x >= raw * 0.999).unwrap_or(10.0 * d);
    ((lo / step).floor() * step, (hi / step).ceil() * step, step)
}

/// Running figures of one value over the records.
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    n: u64,
    sum: f64,
    min: f64,
    max: f64,
}

impl Run {
    fn add(&mut self, v: f64) {
        if v.is_finite() {
            (self.min, self.max) = if self.n == 0 { (v, v) } else { (self.min.min(v), self.max.max(v)) };
            self.n += 1;
            self.sum += v;
        }
    }
    fn mean(&self) -> f64 {
        if self.n == 0 {
            f64::NAN
        } else {
            self.sum / self.n as f64
        }
    }
}

/// What a figure is: counted from the signal, the result of a model, or missing with its reason in the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Measured,
    Model,
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cell {
    pub label: &'static str,
    pub value: String,
    pub kind: Kind,
}

fn cell(label: &'static str, value: String, kind: Kind) -> Cell {
    Cell { label, value, kind }
}

/// A figure, or `why` in its place when it could not be computed.
fn figure(label: &'static str, v: f64, unit: &str, kind: Kind, why: &str) -> Cell {
    if v.is_finite() {
        cell(label, si(v, unit), kind)
    } else {
        cell(label, why.to_string(), Kind::Missing)
    }
}

/// One chart: its title, the x range under it, the y range beside it, or why it is empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Chart {
    pub title: String,
    pub empty: String,
    pub x: (String, String),
    pub y: (String, String),
}

/// All the page draws, and the sums behind it.
pub struct Page {
    pool: Vec<f64>,
    head: usize,
    means: EdgeMeans,
    edges: u64,
    records: u64,
    /// per record: TIE RMS, UI, sum of squares and count of the TIE steps and of their steps
    rms: Run,
    ui: Run,
    step: (f64, u64),
    step2: (f64, u64),
    /// time of the last refresh
    t: f64,
    /// a record brought too few edges for the pool
    short: usize,
    /// the note under the page head, and whether it speaks of a limit
    pub(super) status: [String; 2],
    pub(super) title: String,
    /// the eye in colours, 0 where nothing was hit
    pub(super) img: Vec<u32>,
    pub(super) img_w: usize,
    /// one and zero level, V
    pub(super) levels: Option<(f64, f64)>,
    /// voltage between two grid lines
    pub(super) vstep: f64,
    pub(super) charts: [Chart; 4],
    /// TIE trend: (low, high) per column, in units of its range
    pub(super) trend: Vec<(f32, f32)>,
    pub(super) hist: Vec<f32>,
    /// spectrum, dB under the peak per column
    pub(super) spec: Vec<f32>,
    pub(super) tub: Bathtub,
    pub(super) measured: Vec<Cell>,
    pub(super) model: Vec<Cell>,
    pub(super) eye_measured: Vec<Cell>,
    pub(super) eye_model: Vec<Cell>,
    pub(super) stats: Vec<Cell>,
    /// One number per figure in `EYE_ITEMS`. NaN where the page has no number.
    pub(super) readings: Vec<(&'static str, f64)>,
}

impl Default for Page {
    fn default() -> Page {
        let wait = Chart { empty: "Waiting for edges".into(), ..Chart::default() };
        Page {
            pool: Vec::new(),
            head: 0,
            means: EdgeMeans::default(),
            edges: 0,
            records: 0,
            rms: Run::default(),
            ui: Run::default(),
            step: (0.0, 0),
            step2: (0.0, 0),
            t: f64::NEG_INFINITY,
            short: 0,
            status: ["Waiting for edges".into(), String::new()],
            title: String::new(),
            img: Vec::new(),
            img_w: 0,
            levels: None,
            vstep: 0.0,
            charts: [
                Chart { title: "TIE trend".into(), ..wait.clone() },
                Chart { title: "TIE histogram".into(), ..wait.clone() },
                Chart { title: "TIE spectrum".into(), ..wait.clone() },
                Chart { title: "Bathtub (model)".into(), ..wait },
            ],
            trend: Vec::new(),
            hist: Vec::new(),
            spec: Vec::new(),
            tub: Bathtub::default(),
            measured: Vec::new(),
            model: Vec::new(),
            eye_measured: Vec::new(),
            eye_model: Vec::new(),
            stats: Vec::new(),
            readings: Vec::new(),
        }
    }
}

/// Room for the label of a figure.
pub(super) const LABEL_W: i32 = 64;

/// Content identity of one stored record, independent of the trace viewport.
fn ref_fp(v: &[f32], dt: f64, t0: f64) -> u64 {
    let mut h = v.len() as u64;
    h = h.wrapping_mul(0x9E37_79B1_85EB_CA87).wrapping_add(dt.to_bits());
    h = h.wrapping_mul(0x9E37_79B1_85EB_CA87).wrapping_add(t0.to_bits());
    for x in v {
        h = h.wrapping_mul(0x9E37_79B1_85EB_CA87).wrapping_add(x.to_bits() as u64);
    }
    h
}

/// Reference-only Eye alignment; ordinary analyser measurement gates are unchanged.
fn eye_common_grid(a: &WaveSamp, b: &WaveSamp) -> Option<(f64, f64, usize)> {
    if a.v.len() < 2
        || b.v.len() < 2
        || !a.dt.is_finite()
        || !b.dt.is_finite()
        || a.dt <= 0.0
        || b.dt <= 0.0
        || !a.t0.is_finite()
        || !b.t0.is_finite()
    {
        return None;
    }
    let grid = if a.dt <= b.dt { a } else { b };
    let start = a.t0.max(b.t0);
    let stop = (a.t0 + (a.v.len() - 1) as f64 * a.dt).min(b.t0 + (b.v.len() - 1) as f64 * b.dt);
    let snap = |x: f64| if (x - x.round()).abs() <= 1e-7 { x.round() } else { x };
    let first = snap((start - grid.t0) / grid.dt).ceil().max(0.0);
    let last = snap((stop - grid.t0) / grid.dt).floor().min((grid.v.len() - 1) as f64);
    if last < first + 1.0 {
        return None;
    }
    Some((grid.dt, grid.t0 + first * grid.dt, (last - first + 1.0) as usize))
}

fn eye_on_grid(mut source: WaveSamp, t0: f64, dt: f64, n: usize) -> Vec<f32> {
    if (source.dt - dt).abs() <= dt.abs() * 1e-9 && (source.t0 - t0).abs() <= dt.abs() * 1e-6 {
        source.v.resize(n, f32::NAN);
        return source.v;
    }
    (0..n)
        .map(|k| {
            let x = (t0 + k as f64 * dt - source.t0) / source.dt;
            if x < 0.0 || x > source.v.len().saturating_sub(1) as f64 {
                return f32::NAN;
            }
            let i = x as usize;
            let a = source.v[i];
            let b = source.v[(i + 1).min(source.v.len() - 1)];
            let fraction = (x - i as f64) as f32;
            if a.is_finite() && b.is_finite() {
                a * (1.0 - fraction) + b * fraction
            } else if a.is_finite() {
                a
            } else {
                b
            }
        })
        .collect()
}

/// The coloured hits into `r`. Same size is copied; a column that is mid-slide is scaled so the eye stays put. Both go
/// through `Canvas::image` (0 is transparent), so a recording canvas (GPU drawing) gets them too.
fn blit_hits(cv: &mut Canvas, r: Rect, img: &[u32], iw: usize) {
    if iw == 0 || r.w <= 0 || r.h <= 0 || img.len() < iw {
        return;
    }
    let ih = img.len() / iw;
    let (w, h) = (r.w as usize, r.h as usize);
    if iw == w && ih == h {
        return cv.image(r, &img[..iw * ih], true);
    }
    let mut px = vec![0u32; w * h];
    for (y, row) in px.chunks_exact_mut(w).enumerate() {
        let sy = (y * ih / h).min(ih - 1);
        let src = &img[sy * iw..(sy + 1) * iw];
        for (x, d) in row.iter_mut().enumerate() {
            *d = src[(x * iw / w).min(iw - 1)];
        }
    }
    cv.image(r, &px, true);
}

/// The eye page (eye diagram, jitter, the charts).
pub(super) struct EyePage;

impl super::analysers::Analyser for EyePage {
    fn view(&self) -> AView {
        AView::Eye
    }

    fn feature(&self) -> (&'static str, &'static str) {
        ("eye", "Eye")
    }

    fn name(&self) -> &'static str {
        "Eye / jitter"
    }

    fn menu(&self) -> MenuId {
        MenuId::Eye
    }

    fn scpi(&self) -> Option<&'static str> {
        Some("EYE")
    }

    fn more(&self, _ui: &Ui) -> Vec<Item> {
        vec![Item::EyeClear]
    }

    fn without_record(&self, ui: &Ui) -> bool {
        let is_ref = |id: WaveId| matches!(id, WaveId::Ref(_));
        is_ref(ui.m.eye_src) || ui.m.eye_b.is_some_and(is_ref) || ui.m.eye_rail.is_some_and(is_ref)
    }

    fn update(&self, ui: &mut Ui, rec: &Record, _pane: bool) {
        ui.eye_update(rec);
    }

    fn draw(&self, ui: &Ui, cv: &mut Canvas) {
        ui.draw_eye(cv);
    }
}

#[cfg(test)]
mod automatic_setup_tests {
    use super::*;

    fn reference(values: Vec<f32>, dt: f64) -> Ui {
        let mut ui = crate::ui::testkit::demo_ui();
        ui.set_offline(true);
        ui.refs[0] = Some(RefWave { label: "Auto test".into(), vpd: 0.2, ..RefWave::test(values, dt, -1e-6) });
        ui.select_workspace_source(WaveId::Ref(0));
        ui.m.run = RunState::Stop;
        ui
    }

    fn square(samples: usize, samples_per_ui: usize) -> Vec<f32> {
        (0..samples).map(|i| if (i / samples_per_ui).is_multiple_of(2) { -1.0 } else { 1.0 }).collect()
    }

    #[test]
    fn auto_eye_is_one_action_uses_selected_data_and_closes_settings() {
        let mut ui = reference(square(20_000, 20), 50e-12);
        ui.m.eye_b = Some(WaveId::Ref(9));
        ui.m.eye_rail = Some(WaveId::Ref(8));
        ui.m.eye_notch = 10e6;
        ui.m.eye_rate = 123.0;
        ui.m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
        ui.open_menu(MenuId::Eye);
        let before = (ui.m.run, ui.replay.captures, ui.m.trigger_wave(), ui.m.meas.clone());
        ui.activate(Item::EyeAct);
        assert!(!ui.menu_open);
        assert_eq!(ui.m.eye_src, WaveId::Ref(0));
        assert_eq!((ui.m.eye_b, ui.m.eye_rail, ui.m.eye_notch), (None, None, 0.0));
        assert!(ui.m.eye_from_record && (ui.m.eye_rate / 1e9 - 1.0).abs() < 0.01);
        assert_eq!((ui.m.run, ui.replay.captures, ui.m.trigger_wave(), ui.m.meas.clone()), before);
        let summary = ui.azr.eye_auto.summary.as_ref().unwrap();
        assert!(summary.threshold.abs() < 0.01 && summary.hysteresis > 0.0);
        assert!(ui.azr.page.records > 0 && !ui.azr.page.readings.is_empty());
        assert!(ui.azr.page.status[0].contains("Auto"));
    }

    #[test]
    fn auto_eye_flat_or_undersampled_data_has_explicit_missing_results() {
        for (values, dt, reason) in [(vec![0.5; 1000], 1e-9, "Flat"), (square(1000, 1), 1e-9, "2 samples")] {
            let mut ui = reference(values, dt);
            ui.eye_make();
            assert!(ui.azr.eye.is_none() && ui.azr.eye_res.is_none());
            assert!(ui.azr.page.readings.is_empty());
            assert!(ui.azr.page.status[1].contains(reason), "{:?}", ui.azr.page.status);
            assert_eq!(ui.m.run, RunState::Stop);
        }
    }

    #[test]
    fn auto_eye_reports_low_sample_density_without_inventing_compliance() {
        let mut ui = reference(square(8000, 4), 0.25e-9);
        ui.eye_make();
        let summary = ui.azr.eye_auto.summary.as_ref().unwrap();
        assert!((summary.samples_per_ui - 4.0).abs() < 0.01);
        assert!(summary.warning.contains("Limited sampling"));
        assert!(ui.azr.eye.is_some() && ui.azr.page.status[0].contains("limited"));
    }

    #[test]
    fn auto_eye_live_updates_on_new_captures_but_stopped_pan_is_not_new_data() {
        let mut ui = crate::ui::testkit::demo_ui();
        let mut rec = Ui::blank_record();
        rec.dt = 50e-12;
        rec.t0 = -0.5e-6;
        rec.ch[0] = square(20_000, 20);
        ui.last[0] = Some(rec);
        ui.select_workspace_source(WaveId::Ch(0));
        ui.m.run = RunState::Run;
        ui.eye_make();
        assert_eq!(ui.m.eye_src, WaveId::Ch(0));
        assert_eq!(ui.azr.page.records, 1);
        let first_hits = ui.azr.eye.as_ref().unwrap().hits.iter().map(|&n| n as u64).sum::<u64>();
        let mut fresh = ui.last[0].clone().unwrap();
        fresh.ch[0].rotate_left(3);
        ui.last[0] = Some(fresh);
        ui.records += 1;
        ui.now += REFRESH;
        ui.analyzer_update();
        assert_eq!(ui.azr.page.records, 2);
        assert!(ui.azr.eye.as_ref().unwrap().hits.iter().map(|&n| n as u64).sum::<u64>() > first_hits);
        let threshold = ui.azr.eye_auto.summary.as_ref().unwrap().threshold;
        ui.m.run = RunState::Stop;
        for _ in 0..20 {
            ui.m.zdelay += 1e-9;
            ui.analyzer_update();
        }
        assert_eq!(ui.azr.page.records, 2, "stopped panning cannot manufacture observations");
        assert_eq!(ui.azr.eye_auto.summary.as_ref().unwrap().threshold, threshold);
        assert!(!ui.azr.page.img.is_empty() && !ui.azr.page.hist.is_empty());
        ui.render(&mut Canvas::new(W, H));
    }

    #[test]
    fn auto_eye_background_rejects_old_source_and_does_not_recount_pan() {
        let mut ui = reference(square(300_000, 20), 50e-12);
        ui.eye_make();
        assert!(ui.azr.page.status[0].contains("Computing"));
        assert_eq!(ui.azr.eye_auto.submitted, 1);
        ui.refs[0].as_mut().unwrap().v = square(300_000, 10).into();
        ui.eye_make();
        let start = std::time::Instant::now();
        while ui.azr.eye_res.is_none() && start.elapsed().as_secs() < 20 {
            ui.step(0.01);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ui.azr.eye_res.is_some(), "{:?}", ui.azr.page.status);
        assert!((ui.m.eye_rate / 2e9 - 1.0).abs() < 0.01);
        let submitted = ui.azr.eye_auto.submitted;
        let records = ui.azr.page.records;
        for _ in 0..30 {
            ui.m.zdelay += 1e-9;
            ui.analyzer_update();
        }
        assert_eq!((ui.azr.eye_auto.submitted, ui.azr.page.records), (submitted, records));
        assert_eq!((ui.m.run, ui.replay.captures), (RunState::Stop, 0));
    }

    #[test]
    fn auto_eye_stored_reference_stays_background_when_another_slot_is_simulated() {
        let mut ui = reference(square(300_000, 20), 50e-12);
        ui.session_is_sim = true;
        ui.m.sim_slot = 1;
        ui.eye_make();
        assert_eq!(ui.azr.eye_auto.submitted, 1);
        assert!(ui.azr.eye_res.is_none() && ui.azr.page.status[0].contains("Computing"));
        let start = std::time::Instant::now();
        while ui.azr.eye_res.is_none() && start.elapsed().as_secs() < 20 {
            ui.eye_auto_poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ui.azr.eye_res.is_some(), "{:?}", ui.azr.page.status);
        assert_eq!(ui.azr.page.records, 1);
        assert!(ui.azr.page.title.contains("fixed full record"));
    }
}
