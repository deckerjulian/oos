// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The eye and jitter page. `eye_update` runs per record: the eye and the cheap sums there, everything else (fits,
//! charts, the coloured eye, all text) at most every `REFRESH` seconds into `Page`, so drawing only copies and plots.
//!
//! TIE is pooled over records: each record is timed against its own clock, so its TIE stands alone, and a window with
//! few edges still adds up to a usable distribution. What drifts slower than one record is taken out by the fit.
//!
//! The right column (the four plots and the jitter figures) slides off the right edge; the tab on the eye brings it
//! back. A tap on a plot gives it the whole chart area, and a second tap restores the four.

use super::magnetic::{missing_wave, WaveSamp};
use super::*;
use oos_dsp::eye::{self, Bathtub, EdgeMeans, Jitter, JitterFit};

pub(super) mod automatic;

/// Seconds between refreshes of the page.
const REFRESH: f64 = 0.2;

/// Figures on the eye page, in the order the boxes draw them. The long form is what `:EYE:ITEM?` accepts
/// (the upper-case letters are the short form). A missing figure is stored as NaN.
pub const EYE_ITEMS: &[&str] = &[
    "ONE", "ZERO", "AMPLitude", "CROSsing", "RISe", "FALL", "HEIGht", "WIDTh", "Q", "HBER", "RECords", "EDGes", "UI", "SAMPling",
    "TPP", "TRMS", "DCD", "DDJ", "PJRms", "PRMS", "C2C", "F2", "F4", "F8", "SRJ", "PN", "TPRE", "TPOST", "PPRE", "PPOST", "DCCM", "ACCM", "XOVer", "TNT", "SSCPpm", "SSCRate",
    "TJ", "RJ", "DJ", "OPENing", "SEEN", "TJBer", "EWBer", "J2", "J9", "NPJ",
];

/// The figures `eye_facts` adds beside the ones `text_jitter` and `text_eye` already stored.
const EYE_FACTS: &[&str] = &[
    "HBER", "F2", "F4", "F8", "SRJ", "PN", "TPRE", "TPOST", "PPRE", "PPOST", "DCCM", "ACCM", "XOVer", "TNT", "SSCPpm", "SSCRate", "TJBer", "EWBer", "J2", "J9", "NPJ",
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
        let plot = Rect::new(a.x + EYE_GUTTER, a.y + EYE_TITLE, (a.w - EYE_GUTTER - 16).max(1), (a.h - EYE_TITLE - EYE_FOOT).max(1));
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
    let rows = |pct: i32, most: i32| if a.h < BOXES_MIN_H { 0 } else { ((a.h * pct / 100 - BOX_HEAD) / ROW).clamp(3, most) };
    let (fig_rows, jit_rows) = (rows(28, 8), rows(42, 16));
    let box_h = |r: i32| if r == 0 { 0 } else { BOX_HEAD + r * ROW };
    let (fh, jh) = (box_h(fig_rows), box_h(jit_rows));
    let fig = Rect::new(a.x, bottom - fh, lw, fh);
    let jit = Rect::new(rx, bottom - jh, rw, jh);
    let eye = Rect::new(a.x, a.y, lw, a.h - fh - if fh > 0 { GAP } else { 0 });
    let plot = Rect::new(eye.x + EYE_GUTTER, eye.y + EYE_TITLE, eye.w - EYE_GUTTER - 10, eye.h - EYE_TITLE - EYE_FOOT);
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
    let plots = charts.map(|c| Rect::new(c.x + CHART_GUTTER, c.y + CHART_TITLE, c.w - CHART_GUTTER - 6, (c.h - CHART_TITLE - CHART_FOOT).max(1)));
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
        plots[k] = Rect::new(charts[k].x + CHART_GUTTER, charts[k].y + CHART_TITLE, (charts[k].w - CHART_GUTTER - 6).max(1), (charts[k].h - CHART_TITLE - CHART_FOOT).max(1));
    }
    (charts, plots)
}

/// The tab on the right edge of the eye. It stays below the overflow button.
fn eye_tab_at(a: Rect, l: &Layout) -> Rect {
    let edge = l.eye.x + l.eye.w;
    let x = (edge - TAB_W).clamp(a.x, (a.x + a.w - TAB_W).max(a.x));
    let (y0, y1) = (l.charts[0].y, l.charts[2].y + l.charts[2].h);
    let y = (y0 + (y1 - y0 - TAB_H) / 2).clamp(a.y + HEAD_H, (a.y + a.h - TAB_H).max(a.y + HEAD_H));
    Rect::new(x, y, TAB_W, TAB_H)
}

/// Eye across the pane, charts in a block under it. The figure tables stay on the wide page.
fn layout_stacked(a: Rect) -> Layout {
    let bottom = a.y + a.h;
    let charts_on = a.w >= 280 && a.h >= 240;
    let ch = if charts_on { (a.h * 42 / 100).clamp(120, 200) } else { 0 };
    let eye = Rect::new(a.x, a.y, a.w, (a.h - ch - if ch > 0 { GAP } else { 0 }).max(1));
    let plot = Rect::new(eye.x + EYE_GUTTER, eye.y + EYE_TITLE, (eye.w - EYE_GUTTER - 10).max(1), (eye.h - EYE_TITLE - EYE_FOOT).max(1));
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
        Rect::new(c.x + CHART_GUTTER, c.y + CHART_TITLE, (c.w - CHART_GUTTER - 6).max(1), (c.h - CHART_TITLE - CHART_FOOT).max(1))
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

impl Page {
    fn put_fig(&mut self, name: &'static str, v: f64) {
        debug_assert!(EYE_ITEMS.contains(&name), "{name}");
        self.readings.push((name, v));
    }

    /// Takes the TIE of one record into the sums.
    fn add(&mut self, tie: &[f64], clk: &eye::Clock) {
        self.records += 1;
        self.ui.add(clk.period);
        if tie.len() < eye::MIN_EDGES {
            self.short = tie.len();
            return;
        }
        self.short = 0;
        self.edges += tie.len() as u64;
        self.means.add(tie, clk);
        let mean = tie.iter().sum::<f64>() / tie.len() as f64;
        self.rms.add((tie.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / tie.len() as f64).sqrt());
        let mut prev = None;
        for w in tie.windows(2) {
            let d = w[1] - w[0];
            self.step.0 += d * d;
            self.step.1 += 1;
            if let Some(p) = prev {
                let e: f64 = d - p;
                self.step2.0 += e * e;
                self.step2.1 += 1;
            }
            prev = Some(d);
        }
        for &x in tie {
            if self.pool.len() < POOL {
                self.pool.push(x);
            } else {
                self.pool[self.head] = x;
                self.head = (self.head + 1) % POOL;
            }
        }
    }

    /// Colours the eye: log of the hits on the heat scale, so rare paths stay visible beside the dense levels.
    fn paint(&mut self, e: &eye::Eye) {
        let pal = heat_palette();
        let peak = e.hits.iter().copied().max().unwrap_or(0);
        let k = 255.0 / (1.0 + peak as f32).ln().max(1e-6);
        self.img.clear();
        self.img.extend(e.hits.iter().map(|&h| if h == 0 { 0 } else { pal[(((1.0 + h as f32).ln() * k) as usize).min(255)] }));
        self.img_w = e.w;
    }

    fn chart_trend(&mut self, tr: &[f32], clk: &eye::Clock, cols: usize) {
        self.trend.clear();
        let c = &mut self.charts[0];
        *c = Chart { title: "TIE trend".into(), ..Chart::default() };
        if tr.len() < 2 {
            c.empty = "Needs 2 edges in a record".into();
            return;
        }
        let pk = tr.iter().fold(0.0f32, |a, x| a.max(x.abs())).max(1e-18);
        c.title = format!("TIE trend  peak {}", si(pk as f64, "s"));
        c.x = ("0".into(), si_short(tr.len() as f64 * clk.period, "s"));
        c.y = (format!("+{}", si_short(pk as f64, "s")), format!("-{}", si_short(pk as f64, "s")));
        for k in 0..cols.min(tr.len()) {
            let n = cols.min(tr.len());
            let part = &tr[k * tr.len() / n..((k + 1) * tr.len() / n).max(k * tr.len() / n + 1)];
            self.trend.push(part.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x / pk), b.max(x / pk))));
        }
    }

    fn chart_hist(&mut self, j: &Jitter) {
        self.hist.clear();
        let c = &mut self.charts[1];
        *c = Chart { title: "TIE histogram".into(), ..Chart::default() };
        if j.fit == JitterFit::TooFew {
            c.empty = format!("Needs {} edges", eye::MIN_EDGES);
            return;
        }
        c.title = format!("TIE histogram  \u{3c3} {}", si(j.rms, "s"));
        let top = j.histogram.iter().map(|h| h.1).max().unwrap_or(1).max(1) as f32;
        self.hist.extend(j.histogram.iter().map(|h| h.1 as f32 / top));
        let (lo, hi) = (j.histogram.first().map_or(0.0, |h| h.0), j.histogram.last().map_or(0.0, |h| h.0));
        c.x = (si_short(lo, "s"), si_short(hi, "s"));
        c.y = (format!("{}", top as u32), "0".into());
    }

    /// The spectrum chart; returns PJ (RMS of the lines) or NaN and why not.
    fn chart_spec(&mut self, tr: &[f32], clk: &eye::Clock, cols: usize) -> (f64, String) {
        self.spec.clear();
        let c = &mut self.charts[2];
        *c = Chart { title: "TIE spectrum".into(), ..Chart::default() };
        if tr.len() < SPECTRUM_MIN {
            let why = format!("Needs {} UI in a record", SPECTRUM_MIN);
            c.empty = why.clone();
            return (f64::NAN, why);
        }
        let (df, s) = eye::tie_spectrum(&tr[..tr.len().min(SPECTRUM_UI)], clk.period);
        let Some((k, &pk)) = s.iter().enumerate().skip(2).max_by(|a, b| a.1.total_cmp(b.1)).filter(|p| *p.1 > 0.0) else {
            c.empty = "No jitter in the record".into();
            return (0.0, String::new());
        };
        c.title = format!("TIE spectrum  {} at {}", si(pk, "s"), si(k as f64 * df, "Hz"));
        c.x = ("0".into(), si_short(0.5 / clk.period, "Hz"));
        c.y = (si_short(pk, "s"), format!("-{:.0} dB", SPECTRUM_DB));
        let n = cols.min(s.len() - 2).max(1);
        for col in 0..n {
            let (a, b) = (2 + col * (s.len() - 2) / n, 2 + (col + 1) * (s.len() - 2) / n);
            let v = s[a..b.max(a + 1)].iter().fold(0.0f64, |m, &x| m.max(x));
            self.spec.push(((20.0 * (v.max(1e-30) / pk).log10()) as f32).max(-SPECTRUM_DB));
        }
        let lines = eye::pj_lines(df, &s, PJ_LINES);
        (lines.iter().map(|l| l.1 * l.1).sum::<f64>().sqrt(), String::new())
    }

    fn chart_tub(&mut self, sorted: &[f64], j: &Jitter, period: f64, cols: usize) {
        let c = &mut self.charts[3];
        *c = Chart { title: "Bathtub (model)".into(), ..Chart::default() };
        self.tub = eye::bathtub(sorted, j, period, cols.max(2));
        if self.tub.model.is_empty() {
            c.empty = format!("Needs {} edges", eye::MIN_EDGES);
            return;
        }
        c.title = "Bathtub (model)".into();
        c.x = ("0".into(), "1 UI".into());
        c.y = ("1".into(), format!("1e{:.0}", TUB_FOOT));
    }

    fn text_jitter(&mut self, j: &Jitter, pj: f64, pj_why: &str) {
        let few = format!("needs {} edges", eye::MIN_EDGES);
        let means = format!("needs {} per edge", eye::RUN_MIN);
        let ddj = self.means.ddj();
        let dcd = self.means.dcd();
        self.readings.clear();
        for (name, v) in [("TPP", j.pp), ("TRMS", j.rms), ("DCD", dcd), ("DDJ", ddj), ("PJRms", pj), ("PRMS", j.period_jitter_rms), ("C2C", j.c2c_rms), ("TJ", j.tj_pp), ("RJ", j.rj_rms), ("DJ", j.dj_pp), ("OPENing", self.tub.opening), ("SEEN", self.tub.seen)] {
            self.put_fig(name, v);
        }
        self.measured = vec![
            figure("TIE pp", j.pp, "s", Kind::Measured, &few),
            figure("TIE rms", j.rms, "s", Kind::Measured, &few),
            figure("DCD", dcd, "s", Kind::Measured, &means),
            figure("DDJ", ddj, "s", Kind::Measured, if dcd.is_finite() { "one run length" } else { &means }),
            // no line over the floor is a finding, not a jitter of 0 s
            if pj == 0.0 { cell("PJ rms", "no lines".into(), Kind::Missing) } else { figure("PJ rms", pj, "s", Kind::Measured, if pj_why.is_empty() { &few } else { "needs 64 UI" }) },
            figure("Period rms", j.period_jitter_rms, "s", Kind::Measured, &few),
            figure("Cycle-cycle", j.c2c_rms, "s", Kind::Measured, &few),
        ];
        let at_most = |v: f64| format!("under {}", si(v, "s"));
        self.model = match j.fit {
            JitterFit::TooFew => ["TJ 1e-12", "RJ rms", "DJ", "Opening"].iter().map(|l| cell(l, few.clone(), Kind::Missing)).collect(),
            JitterFit::Limit => vec![
                cell("TJ 1e-12", at_most(j.tj_pp), Kind::Model),
                cell("RJ rms", at_most(j.rj_rms), Kind::Model),
                cell("DJ", format!("needs {} edges", eye::TAIL_EDGES), Kind::Missing),
                cell("Opening", format!("over {:.3} UI", self.tub.opening), Kind::Model),
            ],
            _ => vec![
                figure("TJ 1e-12", j.tj_pp, "s", Kind::Model, &few),
                figure("RJ rms", j.rj_rms, "s", Kind::Model, &few),
                figure("DJ", j.dj_pp, "s", Kind::Model, &few),
                cell("Opening", format!("{:.3} UI", self.tub.opening), Kind::Model),
            ],
        };
        if self.tub.seen.is_finite() {
            self.model.push(cell("Seen open", format!("{:.3} UI", self.tub.seen), Kind::Measured));
        }
    }

    fn text_eye(&mut self, e: &eye::Eye, m: &eye::EyeMeas, period: f64, dt: f64) {
        let empty = "no hits yet";
        // a level noise under one eye row cannot be told from none
        let row = (e.v_max - e.v_min) as f64 / e.h as f64;
        let level = |v: f64, s: f64| match (v.is_finite(), s < row) {
            (false, _) => empty.to_string(),
            (_, true) => format!("{}  \u{3c3} < {}", si(v, "V"), si(row, "V")),
            _ => format!("{}  \u{3c3} {}", si(v, "V"), si(s, "V")),
        };
        let tr = if m.rise.is_finite() && m.fall.is_finite() { format!("{} / {}", si(m.rise, "s"), si(m.fall, "s")) } else { "not resolved".to_string() };
        for (name, v) in [("ONE", m.one), ("ZERO", m.zero), ("AMPLitude", m.amplitude), ("CROSsing", m.crossing_pct), ("RISe", m.rise), ("FALL", m.fall), ("HEIGht", m.height), ("WIDTh", m.width), ("Q", m.q_factor), ("RECords", self.records as f64), ("EDGes", self.pool.len() as f64), ("UI", self.ui.mean()), ("SAMPling", dt)] {
            self.put_fig(name, v);
        }
        self.eye_measured = vec![
            cell("One", level(m.one, m.sigma_one), if m.one.is_finite() { Kind::Measured } else { Kind::Missing }),
            cell("Zero", level(m.zero, m.sigma_zero), if m.zero.is_finite() { Kind::Measured } else { Kind::Missing }),
            figure("Amplitude", m.amplitude, "V", Kind::Measured, empty),
            if m.crossing_pct.is_finite() { cell("Crossing", format!("{:.1} %", m.crossing_pct), Kind::Measured) } else { cell("Crossing", empty.into(), Kind::Missing) },
            cell("Rise / fall", tr, if m.rise.is_finite() && m.fall.is_finite() { Kind::Measured } else { Kind::Missing }),
        ];
        let q = if m.sigma_one + m.sigma_zero < row { cell("Q", format!("over {:.0}", m.amplitude / row), Kind::Model) } else { cell("Q", format!("{:.1}", m.q_factor), Kind::Model) };
        self.eye_model = vec![
            figure("Height", m.height, "V", Kind::Model, empty),
            figure("Width", m.width, "s", Kind::Model, empty),
            if m.amplitude.is_finite() { q } else { cell("Q", empty.into(), Kind::Missing) },
            cell("Height", "levels less 3\u{3c3}".into(), Kind::Missing),
            cell("Width", "1 UI less 6\u{3c3}".into(), Kind::Missing),
        ];
        self.stats = vec![
            cell("Records", format!("{}", self.records), Kind::Measured),
            cell("Edges", format!("{} of {}", self.pool.len(), self.edges), Kind::Measured),
            figure("UI mean", self.ui.mean(), "s", Kind::Measured, empty),
            if self.rms.n > 0 { cell("TIE rms", format!("{} .. {}", si(self.rms.min, "s"), si(self.rms.max, "s")), Kind::Measured) } else { cell("TIE rms", format!("needs {} edges", eye::MIN_EDGES), Kind::Missing) },
            cell("Sampling", format!("{} ({:.0}/UI)", si(dt, "s"), period / dt), Kind::Measured),
        ];
    }

    fn text_status(&mut self, j: &Jitter) {
        let n = self.pool.len();
        self.status[0] = format!("Jitter of {} edges, {} records", n, self.records);
        self.status[1] = if self.short > 0 {
            format!("{} edges in a record: needs {} for jitter", self.short, eye::MIN_EDGES)
        } else {
            match j.fit {
                JitterFit::TooFew => format!("Needs {} edges for a model", eye::MIN_EDGES),
                JitterFit::Quiet => "No jitter above the time resolution".into(),
                JitterFit::Limit if n < eye::TAIL_EDGES => format!("Upper limits: tail fit from {} edges", eye::TAIL_EDGES),
                JitterFit::Limit => "Upper limits: the TIE has no Gaussian tails".into(),
                JitterFit::Tails => "Model: dual-Dirac fit of the TIE tails".into(),
            }
        };
    }
}

impl Ui {
    /// Generated and imported references are immutable records. Eye analysis reads
    /// the whole record even when ordinary measurements follow the zoom window.
    fn eye_samples(&self, rec: &Record, id: WaveId) -> Option<WaveSamp> {
        match id {
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                (r.v.len() >= 2 && r.dt.is_finite() && r.dt > 0.0)
                    .then(|| WaveSamp { v: r.v.clone(), t0: r.t0, dt: r.dt })
            }
            WaveId::Ch(_) => self.wave_samples(rec, id),
        }
    }

    fn eye_source_ready(&self, rec: &Record, id: WaveId) -> bool {
        match id {
            WaveId::Ref(i) => self.refs.get(i).and_then(Option::as_ref)
                .is_some_and(|r| r.v.len() >= 2 && r.dt.is_finite() && r.dt > 0.0),
            WaveId::Ch(i) => i < n_ch() && self.m.ch[i].on && rec.ch[i].len() >= 2 && rec.dt > 0.0,
        }
    }

    /// Keep live channel/strobe alignment unchanged. A reference source instead
    /// puts its full record and reference clock on their finest stored grid.
    fn eye_pair(&self, rec: &Record, id: WaveId, clock: WaveId) -> Option<(Vec<f32>, Vec<f32>, f64, f64)> {
        if matches!(id, WaveId::Ch(_)) {
            let mut t = self.take_waves(rec, &[id, clock], &[]).ok()?;
            let b = t.v.pop()?;
            let a = t.v.pop()?;
            return Some((a, b, t.dt, t.t0));
        }
        let a = self.eye_samples(rec, id)?;
        let b = self.eye_samples(rec, clock)?;
        let grid = if matches!(clock, WaveId::Ref(_)) &&
            (b.dt < a.dt || ((b.dt - a.dt).abs() <= a.dt * 1e-9 && b.v.len() > a.v.len())) { &b } else { &a };
        let (dt, t0, n) = (grid.dt, grid.t0, grid.v.len());
        Some((eye_on_grid(a, t0, dt, n), eye_on_grid(b, t0, dt, n), dt, t0))
    }

    /// Called after replacing a generated reference, including a same-grid record.
    pub(super) fn invalidate_reference_eye(&mut self) {
        self.azr.eye = None;
        self.azr.eye_ref = None;
        self.azr.eye_sim_revision = None;
        self.azr.eye_res = None;
        self.azr.page = Page::default();
        self.azr.eye_auto.invalidate();
    }

    /// Hash every reference sample before making copies or computing statistics.
    /// Live companion waves still accumulate once per live capture as before.
    fn eye_reference_fingerprint(&self, id: WaveId, clock: Option<WaveId>) -> Option<(usize, u64)> {
        let WaveId::Ref(slot) = id else { return None };
        let mut h = 0u64;
        for source in [Some(id), clock, self.m.eye_rail].into_iter().flatten() {
            let WaveId::Ref(i) = source else { return None };
            let Some(r) = self.refs.get(i).and_then(Option::as_ref) else {
                h = h.rotate_left(13) ^ i as u64 ^ u64::MAX;
                continue;
            };
            let data_identity = if self.session_is_sim && i == self.m.sim_slot {
                self.replay.sim_revision ^ r.dt.to_bits() ^ r.t0.to_bits()
            } else { ref_fp(&r.v, r.dt, r.t0) };
            h = h.rotate_left(13) ^ data_identity ^ i as u64;
            h = h.rotate_left(13) ^ r.vpd.to_bits() as u64 ^ ((r.offset.to_bits() as u64) << 32);
        }
        for setting in [self.m.eye_ber, self.m.eye_pn_lo, self.m.eye_pn_hi, self.m.eye_notch] {
            h = h.rotate_left(13) ^ setting.to_bits();
        }
        h ^= (self.m.eye_npj as u64) | ((self.m.eye_hp as u64) << 1) | ((self.m.eye_from_record as u64) << 2);
        Some((slot, h))
    }

    fn eye_scale(&self, id: WaveId) -> (f32, f32) {
        match id {
            WaveId::Ch(c) => {
                let ch = &self.m.ch[c.min(NCH - 1)];
                (ch.volts_per_div(), ch.offset)
            }
            WaveId::Ref(i) => {
                let r = self.refs[i].as_ref().unwrap();
                (r.vpd, r.offset)
            }
        }
    }

    /// Eye page, per record. Clock, when set, is the strobe: its edges fold the source (DQ vs DQS).
    pub(super) fn eye_update(&mut self, rec: &Record) {
        if self.eye_auto_update(rec) { return; }
        let id = self.m.eye_src;
        let clk_id = self.m.eye_b;
        if !self.eye_source_ready(rec, id) {
            self.azr.page.status = [missing_wave(id), "Switch the source on".into()];
            return;
        }
        if let Some(b) = clk_id {
            if !self.eye_source_ready(rec, b) {
                self.azr.page.status = [missing_wave(b), "Switch the clock on".into()];
                return;
            }
        }
        let (vpd, offset) = self.eye_scale(id);
        let lay = layout(self.eye_page_rect(), self.full_on());
        let (pw, ph) = (lay.plot.w.max(64), lay.plot.h.max(64));
        let tag = match id {
            WaveId::Ch(c) => (0u8, c),
            WaveId::Ref(i) => (1, i),
        };
        let ctag = match clk_id {
            None => (0u8, 0usize),
            Some(WaveId::Ch(c)) => (1, c),
            Some(WaveId::Ref(i)) => (2, i),
        };
        let key = (tag.0, tag.1, ctag.0, ctag.1, vpd.to_bits(), offset.to_bits(), self.m.eye_rate.to_bits(), pw, ph);
        let fp = self.eye_reference_fingerprint(id, clk_id);
        if fp.is_some() && fp == self.azr.eye_ref && self.azr.eye.is_some() && self.azr.eye_key == key {
            return;
        }
        let sim_capture = self.session_is_sim && id == WaveId::Ref(self.m.sim_slot) && self.replay.sim_revision > 0;
        let reference_changed = fp.is_some() && fp != self.azr.eye_ref && !sim_capture;
        let (v, clk_v, dt) = if let Some(b) = clk_id {
            let Some((v, clock, dt, _)) = self.eye_pair(rec, id, b) else { return };
            (v, Some(clock), dt)
        } else {
            let s = self.eye_samples(rec, id).unwrap();
            (s.v, None, s.dt)
        };
        let recov = clk_v.as_deref().unwrap_or(&v);
        let recov_id = clk_id.unwrap_or(id);
        let st = match recov_id {
            WaveId::Ch(c) if clk_id.is_none() => self.stats[c],
            _ => stats(recov, dt),
        };
        let (clk_vpd, _) = self.eye_scale(recov_id);
        // A strobe idles near one rail between bursts; histogram top/base sit on that rail and the
        // comparator chatters. Peak-to-peak with a wide hysteresis keeps the burst edges.
        let (thr, hyst) = if let Some(summary) = self.azr.eye_auto.summary.as_ref().filter(|_| self.azr.eye_auto.enabled) {
            (summary.threshold, summary.hysteresis)
        } else if clk_id.is_some() {
            let (lo, hi) = (st.vmin, st.vmax);
            (0.5 * (lo + hi), (0.2 * (hi - lo)).max(0.1 * clk_vpd))
        } else {
            let thr = if st.valid { 0.5 * (st.top + st.base) } else { 0.5 * (st.vmax + st.vmin) };
            (thr, 0.1 * clk_vpd)
        };
        let rate = (self.m.eye_rate > 0.0).then_some(self.m.eye_rate);
        // An idle live bus keeps its accumulated eye. A replacement reference
        // without edges must not display the previous reference's results.
        let recovered = if self.azr.eye_auto.enabled && clk_id.is_none() {
            eye::recover_clock_pll(recov, dt, thr, hyst, rate, 0.0, true)
        } else { eye::recover_clock(recov, dt, thr, hyst, rate) };
        let Some(clk) = recovered else {
            if reference_changed { self.invalidate_reference_eye(); }
            self.azr.page.status[1] = format!("Under 3 edges on {} in the last record", self.wave_name(recov_id));
            return;
        };
        let data_st = stats(&v, dt);
        let left = self.azr.eye.as_ref().is_some_and(|e| {
            let (lo, hi) = (e.v_min as f64, e.v_max as f64);
            let (span, swing) = (hi - lo, (data_st.vmax - data_st.vmin) as f64);
            data_st.vmax as f64 > hi + EYE_OUT * span || (data_st.vmin as f64) < lo - EYE_OUT * span || swing < EYE_SHRUNK * span
        });
        if self.azr.eye.is_none() || self.azr.eye_key != key || left || reference_changed {
            // the eye fills its plot whatever the scale of the channel; the signal itself sets the range
            let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x as f64), b.max(x as f64)));
            let (lo, hi, step) = eye_range(lo, hi);
            self.azr.eye = Some(eye::Eye::new(pw as usize, ph as usize, 2, lo as f32, hi as f32));
            self.azr.eye_key = key;
            self.azr.eye_sim_revision = None;
            self.azr.eye_res = None;
            self.azr.page = Page { vstep: step, ..Page::default() };
        }
        let last = self.azr.page.ui.mean();
        if last.is_finite() && (clk.period / last - 1.0).abs() > UI_CHANGE {
            self.azr.eye.as_mut().unwrap().clear();
            self.azr.page = Page { vstep: self.azr.page.vstep, ..Page::default() };
        }
        let (fold, data) = if clk_id.is_some() {
            let gated = eye::gate_around_clock(&v, dt, &clk, 1.5);
            let e = self.azr.eye.as_ref().unwrap();
            let height = |c: &eye::Clock| {
                let mut t = eye::Eye::new(48, 48, 2, e.v_min, e.v_max);
                t.accumulate(&gated, dt, c);
                let h = t.measure(c.period).height;
                if h.is_finite() {
                    h
                } else {
                    f64::NEG_INFINITY
                }
            };
            let shifted = eye::center_strobe(&clk);
            let fold = if height(&shifted) > height(&clk) { shifted } else { clk.clone() };
            (fold, gated)
        } else {
            (clk.clone(), v.clone())
        };
        if sim_capture && self.azr.eye_sim_revision == Some((self.m.sim_slot, self.replay.sim_revision)) {
            let tie = eye::tie(&clk.edges, &clk);
            self.azr.eye_ref = fp;
            self.eye_refresh(&clk, &tie, dt, &lay, &v, rec);
            return;
        }
        if sim_capture { self.azr.eye_sim_revision = Some((self.m.sim_slot, self.replay.sim_revision)); }
        self.azr.eye.as_mut().unwrap().accumulate(&data, dt, &fold);
        self.azr.eye_ref = fp;
        let tie = eye::tie(&clk.edges, &clk);
        self.azr.page.add(&tie, &clk);
        if fp.is_some() || self.now - self.azr.page.t >= REFRESH {
            self.azr.page.t = self.now;
            self.eye_refresh(&clk, &tie, dt, &lay, &v, rec);
        }
    }

    /// The heavy part of the page, from the pool and the last record.
    fn eye_refresh(&mut self, clk: &eye::Clock, tie: &[f64], dt: f64, lay: &Layout, wave: &[f32], rec: &Record) {
        let clock = if let Some(b) = self.m.eye_b {
            format!("clock from {}", self.wave_name(b))
        } else if self.m.eye_from_record {
            "bit rate from the record".into()
        } else if self.m.eye_rate > 0.0 {
            "clock fitted near the set rate".into()
        } else {
            "clock fitted to the edges".into()
        };
        let source = if let WaveId::Ref(i) = self.m.eye_src { format!("{} · {}", self.wave_name(self.m.eye_src), if self.session_is_sim && i == self.m.sim_slot { "live SIM captures" } else { "fixed full record" }) } else { self.wave_name(self.m.eye_src) };
        let title = format!("{} \u{b7} {} \u{b7} {}", source, si(1.0 / clk.period, "b/s"), clock);
        let Some(e) = self.azr.eye.as_ref() else { return };
        let meas = e.measure(clk.period);
        let p = &mut self.azr.page;
        p.paint(e);
        p.levels = (meas.one.is_finite() && meas.zero.is_finite()).then_some((meas.one, meas.zero));
        let mut sorted = p.pool.clone();
        sorted.sort_by(f64::total_cmp);
        let mut j = eye::jitter(&sorted);
        // steps between edges only mean something inside a record: those come from the sums
        let rms = |s: (f64, u64)| if s.1 == 0 { f64::NAN } else { (s.0 / s.1 as f64).sqrt() };
        (j.period_jitter_rms, j.c2c_rms) = (rms(p.step), rms(p.step2));
        let trend = eye::tie_trend(&clk.edges, tie, clk);
        let (_, plots) = charts_shown(lay, self.eye_big);
        p.chart_trend(&trend, clk, plots[0].w as usize);
        p.chart_hist(&j);
        let (pj, why) = p.chart_spec(&trend, clk, plots[2].w as usize);
        p.chart_tub(&sorted, &j, clk.period, plots[3].w as usize);
        p.text_jitter(&j, pj, &why);
        p.text_eye(e, &meas, clk.period, dt);
        p.text_status(&j);
        p.title = title;
        let means = self.azr.page.means.clone();
        let (measured, model, eye_model) = self.eye_facts(clk, tie, &trend, &j, &meas, wave, dt, rec, &means);
        self.azr.page.measured.extend(measured);
        self.azr.page.model.extend(model);
        self.azr.page.eye_model.extend(eye_model);
        self.azr.eye_res = Some((meas, j, clk.period));
        self.eye_auto_finish_status();
    }

    /// Gather optional companions; the DSP/figure builder is also used off-thread.
    fn eye_facts(&mut self, clk: &eye::Clock, tie: &[f64], trend: &[f32], j: &Jitter, meas: &eye::EyeMeas, wave: &[f32], dt: f64, rec: &Record, means: &EdgeMeans) -> (Vec<Cell>, Vec<Cell>, Vec<Cell>) {
        let settings = FactSettings::from(&self.m);
        let rail = self.m.eye_rail.and_then(|id| self.eye_samples(rec, id)).map(|s| (s.v, s.dt));
        let companion = self.m.eye_b.and_then(|id| self.eye_pair(rec, self.m.eye_src, id));
        populate_eye_facts(&mut self.azr.page, settings, clk, tie, trend, j, meas, wave, dt, means,
            rail.as_ref().map(|(v, dt)| (v.as_slice(), *dt)),
            companion.as_ref().map(|(a, b, dt, _)| (a.as_slice(), b.as_slice(), *dt)))
    }

    /// Where the column sits this frame: 0 has left the screen, 1 is home.
    fn eye_column_open(&self) -> f32 {
        let u = ((self.now - self.eye_t) / SLIDE).clamp(0.0, 1.0) as f32;
        let e = u * u * (3.0 - 2.0 * u);
        self.eye_from + (self.eye_to - self.eye_from) * e
    }

    fn eye_tab(&self) -> Rect {
        eye_tab_at(self.eye_page_rect(), &placed(self.eye_page_rect(), self.eye_column_open()))
    }

    /// Slide the column the other way. A tap in the middle of a move reverses it.
    fn eye_slide(&mut self) {
        self.eye_from = self.eye_column_open();
        self.eye_to = if self.eye_to > 0.5 { 0.0 } else { 1.0 };
        self.eye_t = self.now;
    }

    /// The column tab, or a plot. A plot that is already open goes back to the four. Full screen has neither.
    pub(super) fn eye_column_tap(&mut self, x: i32, y: i32) -> bool {
        let area = self.eye_page_rect();
        if self.full_on() || !area.contains(x, y) || layout(area, false).charts[0].w == 0 {
            return false;
        }
        if self.eye_tab().contains(x, y) {
            self.eye_slide();
            return true;
        }
        if self.eye_column_open() < 0.5 {
            return false;
        }
        let l = placed(self.eye_page_rect(), self.eye_column_open());
        let (charts, _) = charts_shown(&l, self.eye_big);
        if let Some(k) = self.eye_big {
            if charts[k as usize].contains(x, y) {
                self.eye_big = None;
                self.azr.page.t = f64::NEG_INFINITY;
                return true;
            }
            return false;
        }
        for (k, r) in charts.iter().enumerate() {
            if r.contains(x, y) {
                self.eye_big = Some(k as u8);
                self.azr.page.t = f64::NEG_INFINITY;
                return true;
            }
        }
        false
    }

    pub(super) fn eye_pane_close(&self) -> Rect {
        let a = self.eye_pane_rect();
        Rect::new(a.x + a.w - 50, a.y + 1, 48, workspace::HEADER - 2)
    }

    pub(super) fn eye_set_place(&mut self, place: EyePlace) {
        let previous = self.m.eye_place;
        self.m.eye_place = place;
        if place == EyePlace::Under && self.eye_below() == 0 {
            self.m.eye_place = EyePlace::Side;
            self.toast("Eye stays beside the trace: not enough room below. Hide the logic lanes or measurements to use Under.");
        }
        if self.m.eye_place == previous { return; }
        self.azr.eye = None;
        self.relayout();
        self.analyzer_update();
    }

    /// Bit rate of `id` from its edges. Nothing was typed.
    pub(super) fn waveform_bit_rate(&self, id: WaveId) -> Option<f64> {
        match id {
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                automatic::probe_rate(&r.v, r.dt)
            }
            WaveId::Ch(c) => {
                let rec = self.last[0].as_ref()?;
                (c < n_ch() && self.m.ch[c].on).then(|| automatic::probe_rate(&rec.ch[c], rec.dt)).flatten()
            }
        }
    }

    /// One press: look at the waveform, take the bit rate from that record, and open the eye.
    pub(super) fn eye_make(&mut self) {
        // Selection is intentional: never switch away from a flat/weak selected
        // signal merely because another visible channel has an easier clock.
        let id = self.selected_workspace_source().or_else(|| {
            (0..n_ch()).filter(|&c| self.m.ch[c].on).map(WaveId::Ch)
                .chain((0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).map(WaveId::Ref)).next()
        });
        self.close_menu();
        if let Some(id) = id { self.m.eye_src = id; }
        self.m.eye_from_record = true;
        self.m.eye_rate = 0.0;
        self.m.eye_b = None;
        self.m.eye_rail = None;
        self.m.eye_notch = 0.0;
        self.m.eye_hp = false;
        self.m.view = AView::Eye;
        self.eye_big = None;
        self.eye_to = 1.0;
        self.eye_from = 1.0;
        self.invalidate_reference_eye();
        self.azr.eye_auto.enabled = true;
        self.relayout();
        let rec = self.last[0].clone().unwrap_or_else(Self::blank_record);
        self.eye_update(&rec);
        self.toast(if id.is_some() { "Automatic Eye/Jitter on the selected source" } else { "No active or loaded waveform: select a source" });
    }

    /// The eye in its pane, with Side / Under and Close on the bar. The trace stays in the plot.
    pub(super) fn draw_eye_pane(&self, cv: &mut Canvas) {
        if !self.eye_open() {
            return;
        }
        let a = self.eye_pane_rect();
        cv.fill(a, BG);
        cv.rect(a, CARD_BORDER);
        self.draw_eye_bar(cv, a);
        self.draw_eye(cv);
    }

    pub(super) fn eye_header_source(&self) -> Rect {
        let a = self.eye_pane_rect();
        Rect::new(a.x + 4, a.y + 1, 88.min((a.w - 58).max(1)), workspace::HEADER - 2)
    }

    pub(super) fn eye_source_mode_label(&self) -> &'static str {
        match self.m.eye_src {
            WaveId::Ref(i) if self.session_is_sim && i == self.m.sim_slot => "SIM captures",
            WaveId::Ref(_) => "Full record",
            WaveId::Ch(_) if self.m.run == RunState::Stop => "Stopped",
            WaveId::Ch(_) => "Live",
        }
    }

    pub(super) fn eye_header_tap(&mut self, x: i32, y: i32) {
        let source = self.eye_header_source().contains(x, y);
        self.open_menu(MenuId::Eye);
        if source { self.open_picker(Item::EyeSrc); }
    }

    pub(super) fn eye_rate_caption(&self) -> String {
        if self.m.eye_rate > 0.0 {
            format!("{}{}", if self.azr.eye_auto.enabled { "Est." } else { "" }, si_short(self.m.eye_rate, "b/s"))
        } else { "Auto rate".into() }
    }

    fn draw_eye_bar(&self, cv: &mut Canvas, a: Rect) {
        let close = self.eye_pane_close();
        cv.fill(Rect::new(a.x + 1, a.y + 1, a.w - 2, workspace::HEADER - 1), BAR);
        let source = match self.m.eye_src { WaveId::Ch(c) => format!("CH{}", c + 1), WaveId::Ref(i) => format!("R{}", i + 1) };
        let rate = self.eye_rate_caption();
        let pick = self.eye_header_source();
        btn(cv, pick, BTN, BTN_BORDER);
        cv.text(&self.fonts.sb, pick.x + 5, a.y + 18, &format!("Eye {}", source), self.wave_color(self.m.eye_src));
        chevron_down(cv, pick.x + pick.w - 14, pick.y + pick.h / 2 - 2, DIM);
        let settings = Rect::new(pick.x + pick.w + 4, pick.y, (close.x - pick.x - pick.w - 8).max(1), pick.h);
        btn(cv, settings, BTN, BTN_BORDER);
        cv.set_clip(Rect::new(settings.x + 5, settings.y, (settings.w - 23).max(1), settings.h));
        cv.text(&self.fonts.s, settings.x + 5, a.y + 18, &format!("{} · {}", self.eye_source_mode_label(), rate), TEXT);
        cv.no_clip();
        chevron_down(cv, settings.x + settings.w - 14, settings.y + settings.h / 2 - 2, DIM);
        btn(cv, close, BTN, BTN_BORDER);
        cv.text_in(&self.fonts.s, close, 1, "Close", TEXT);
    }

    pub(super) fn draw_eye(&self, cv: &mut Canvas) {
        let a = self.eye_page_rect();
        let p = &self.azr.page;
        // full screen drops the column: the eye is the whole plot, and the plot button still goes back
        if self.full_on() {
            let l = layout(a, true);
            cv.fill(a, BG);
            self.eye_plot(cv, &l);
            return;
        }
        let l = placed(a, self.eye_column_open());
        cv.fill(a, BG);
        self.eye_plot(cv, &l);
        if l.fig_rows > 0 {
            self.eye_box(cv, l.fig, "Eye figures", l.fig_rows, &[("Measured", &p.eye_measured), ("Model", &p.eye_model), ("Records", &p.stats)]);
        }
        if l.charts[0].x < a.x + a.w {
            cv.set_clip(a);
            // One workspace header owns source/rate and docking; charts need no duplicate title.
            let (charts, plots) = charts_shown(&l, self.eye_big);
            let col = self.wave_color(self.m.eye_src);
            for k in 0..4 {
                if self.eye_big.is_some_and(|b| b as usize != k) {
                    continue;
                }
                if charts[k].w < 40 || charts[k].h < 36 {
                    continue;
                }
                self.chart_frame(cv, charts[k], plots[k], &p.charts[k]);
                if p.charts[k].empty.is_empty() {
                    match k {
                        0 => Self::plot_trend(cv, plots[0], &p.trend, col),
                        1 => Self::plot_hist(cv, plots[1], &p.hist, col),
                        2 => Self::plot_spec(cv, plots[2], &p.spec, col),
                        _ => self.plot_tub(cv, plots[3], &p.tub),
                    }
                }
            }
            if l.jit_rows > 0 {
                self.eye_box(cv, l.jit, "Jitter breakdown", l.jit_rows, &[("Measured", &p.measured), ("Model", &p.model)]);
            }
            cv.no_clip();
        }
        self.draw_eye_tab(cv);
    }

    /// Left edge of the heat scale on plot `r`. On the usual split it is the top right of the plot; with the column
    /// away that corner belongs to the full-screen button, so the scale stops short of it.
    fn heat_x(&self, r: Rect) -> i32 {
        let src = match self.src.name() {
            "DEMO" => "Demo signals",
            "REPLAY" => "Replay",
            _ => "",
        };
        let info = if src.is_empty() { 0 } else { self.fonts.s.width(src) + 16 };
        let end = 68 + self.fonts.s.width("dense") + 4;
        let button = self.full_btn().0;
        let candidate = r.x + r.w - 128;
        if button.x >= r.x && button.x < r.x + r.w {
            let limit = button.x - 8 - info;
            candidate.min(limit - end).max(r.x)
        } else {
            candidate.max(r.x)
        }
    }

    fn draw_eye_tab(&self, cv: &mut Canvas) {
        if layout(self.eye_page_rect(), false).charts[0].w == 0 {
            return;
        }
        let r = self.eye_tab();
        cv.round(r, 6, PANEL_BG, 235);
        cv.round_outline(r, 6, BTN_BORDER);
        chevron(cv, r.x + r.w / 2, r.y + r.h / 2, if self.eye_to > 0.5 { 1 } else { -1 }, TEXT);
    }

    fn eye_head(&self, cv: &mut Canvas, l: &Layout) {
        if l.head.w < 40 || l.head.h < 16 {
            return;
        }
        let f = &self.fonts;
        let p = &self.azr.page;
        cv.set_clip(l.head);
        cv.text(&f.sb, l.head.x + 2, l.head.y + 15, &p.status[0], TEXT);
        cv.text(&f.s, l.head.x + 2, l.head.y + 33, &p.status[1], MODELLED);
        cv.no_clip();
    }

    /// The eye: frame, axes in volts and UI, the coloured hits and the two levels.
    fn eye_plot(&self, cv: &mut Canvas, l: &Layout) {
        let f = &self.fonts;
        let p = &self.azr.page;
        let r = l.plot;
        cv.fill(r, BAR);
        // half a UI per time line, the crossings stand out
        for k in 0..=4 {
            let x = r.x + k * (r.w - 1) / 4;
            cv.vline(x, r.y, r.y + r.h - 1, if k % 2 == 1 { GRID_AXIS } else { GRID_LINE });
            let s = ["-0.5", "0", "0.5", "1", "1.5 UI"][k as usize];
            let w = f.s.width(s);
            let tx = if k == 4 { x - w } else if k == 0 { x } else { x - w / 2 };
            cv.text(&f.s, tx, r.y + r.h + 13, s, AXIS_TEXT);
        }
        if let Some(e) = &self.azr.eye {
            let (top, span) = (e.v_max as f64, (e.v_max - e.v_min) as f64);
            let lines = if p.vstep > 0.0 { (span / p.vstep).round() as i32 } else { 0 };
            for k in 0..=lines {
                let v = top - k as f64 * p.vstep;
                let y = r.y + k * (r.h - 1) / lines.max(1);
                cv.hline(r.x, r.x + r.w - 1, y, GRID_LINE);
                // a sum of steps misses zero by a rounding error
                let s = si_short(if v.abs() < 1e-3 * p.vstep { 0.0 } else { v }, "V");
                let ty = if k == 0 { y + 10 } else if k == lines { y } else { y + 4 };
                cv.text(&f.s, r.x - 4 - f.s.width(&s), ty, &s, AXIS_TEXT);
            }
            blit_hits(cv, r, &p.img, p.img_w);
            if let Some((one, zero)) = p.levels {
                let span = (e.v_max - e.v_min) as f64;
                for v in [one, zero] {
                    let y = r.y + ((e.v_max as f64 - v) / span * (r.h - 1) as f64) as i32;
                    if y > r.y && y < r.y + r.h - 1 {
                        cv.dashed_hline(r.x, r.x + r.w - 1, y, 6, 6, LABEL);
                    }
                }
            }
        }
        if p.img.is_empty() {
            cv.text_in(&f.sb, r, 1, &p.status[0], DIM);
        }
        cv.rect(r, GRID_FRAME);
    }

    /// Box, title, axis ends of a chart, or the reason it is empty.
    fn chart_frame(&self, cv: &mut Canvas, c: Rect, r: Rect, ch: &Chart) {
        let f = &self.fonts;
        cv.round(c, 4, BAR, 255);
        cv.round_outline(c, 4, CARD_BORDER);
        cv.text(&f.sb, c.x + 6, c.y + 13, &ch.title, TEXT);
        cv.fill(r, BG);
        cv.rect(r, GRID_AXIS);
        if !ch.empty.is_empty() {
            cv.text_in(&f.s, r, 1, &ch.empty, DIM);
            return;
        }
        cv.text(&f.s, r.x - 3 - f.s.width(&ch.y.0), r.y + 10, &ch.y.0, AXIS_TEXT);
        cv.text(&f.s, r.x - 3 - f.s.width(&ch.y.1), r.y + r.h - 1, &ch.y.1, AXIS_TEXT);
        cv.text(&f.s, r.x, r.y + r.h + 12, &ch.x.0, AXIS_TEXT);
        cv.text(&f.s, r.x + r.w - f.s.width(&ch.x.1), r.y + r.h + 12, &ch.x.1, AXIS_TEXT);
    }

    fn plot_trend(cv: &mut Canvas, r: Rect, t: &[(f32, f32)], col: Rgb) {
        let mid = r.y + r.h / 2;
        cv.hline(r.x + 1, r.x + r.w - 2, mid, GRID_AXIS);
        let y = |v: f32| mid - (v * (r.h / 2 - 2) as f32) as i32;
        let n = t.len().max(2) as i32 - 1;
        let x = |k: i32| r.x + 1 + k * (r.w - 3) / n;
        let mut prev: Option<(i32, i32)> = None;
        for (k, &(lo, hi)) in t.iter().enumerate() {
            let (px, c) = (x(k as i32), y(0.5 * (lo + hi)));
            cv.vline(px, y(hi), y(lo), col);
            if let Some(q) = prev {
                cv.line(q.0, q.1, px, c, col);
            }
            prev = Some((px, c));
        }
    }

    fn plot_hist(cv: &mut Canvas, r: Rect, h: &[f32], col: Rgb) {
        let n = h.len().max(1) as i32;
        for (k, &v) in h.iter().enumerate() {
            let k = k as i32;
            let (x0, x1) = (r.x + 1 + k * (r.w - 2) / n, r.x + 1 + (k + 1) * (r.w - 2) / n);
            let bh = ((v * (r.h - 3) as f32) as i32).max((v > 0.0) as i32);
            cv.fill(Rect::new(x0, r.y + r.h - 1 - bh, (x1 - x0).max(1), bh), col);
        }
    }

    fn plot_spec(cv: &mut Canvas, r: Rect, s: &[f32], col: Rgb) {
        let y = |db: f32| r.y + 2 + (-db / SPECTRUM_DB * (r.h - 4) as f32) as i32;
        let n = (s.len() as i32 - 1).max(1);
        let mut prev: Option<(i32, i32)> = None;
        for (k, &db) in s.iter().enumerate() {
            let p = (r.x + 1 + k as i32 * (r.w - 3) / n, y(db));
            if let Some(q) = prev {
                cv.line(q.0, q.1, p.0, p.1, col);
            }
            prev = Some(p);
        }
    }

    fn plot_tub(&self, cv: &mut Canvas, r: Rect, b: &Bathtub) {
        let y = |l: f32| r.y + 1 + ((l / TUB_FOOT).clamp(0.0, 1.0) * (r.h - 3) as f32) as i32;
        cv.dashed_hline(r.x + 1, r.x + r.w - 2, y(-12.0), 4, 4, GRID_FRAME);
        let n = (b.ui.len() as i32 - 1).max(1);
        let x = |k: usize| r.x + 1 + k as i32 * (r.w - 3) / n;
        let f = &self.fonts;
        let x0 = r.x + r.w / 2 - 44;
        let x1 = cv.text(&f.s, x0, r.y + r.h + 12, "counted", MEASURED);
        cv.text(&f.s, x1 + 8, r.y + r.h + 12, "model", MODELLED);
        for (curve, c) in [(&b.model, MODELLED), (&b.measured, MEASURED)] {
            let mut prev: Option<(i32, i32)> = None;
            for (k, &l) in curve.iter().enumerate() {
                if !l.is_finite() || l < TUB_FOOT {
                    prev = None;
                    continue;
                }
                let p = (x(k), y(l));
                if let Some(q) = prev {
                    cv.line(q.0, q.1, p.0, p.1, c);
                }
                prev = Some(p);
            }
        }
    }

    /// A text box: a title, then columns of figures under their heads. Rows that do not fit stay out, the first are
    /// the ones that matter most.
    fn eye_box(&self, cv: &mut Canvas, r: Rect, title: &str, rows: i32, cols: &[(&str, &[Cell])]) {
        let f = &self.fonts;
        cv.round(r, 4, BAR, 255);
        cv.round_outline(r, 4, CARD_BORDER);
        let cw = (r.w - 12) / cols.len() as i32;
        for (i, (head, cells)) in cols.iter().enumerate() {
            let x = r.x + 8 + i as i32 * cw;
            let tone = if *head == "Model" { MODELLED } else { LABEL };
            let name = if i == 0 { format!("{}: {}", title, head) } else { head.to_string() };
            cv.text(&f.sb, x, r.y + 15, &name, tone);
            if cells.is_empty() {
                cv.text(&f.s, x, r.y + BOX_HEAD + 10, "no data yet", DIM);
            }
            for (k, c) in cells.iter().take(rows as usize).enumerate() {
                let y = r.y + BOX_HEAD + 10 + k as i32 * ROW;
                let tone = match c.kind {
                    Kind::Measured => MEASURED,
                    Kind::Model => MODELLED,
                    Kind::Missing => DIM,
                };
                cv.text(&f.s, x, y, c.label, DIM);
                cv.text(&f.s, x + LABEL_W, y, &c.value, tone);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct FactSettings { ber: f64, npj: bool, pn_lo: f64, pn_hi: f64, notch: f64, hp: bool }

impl From<&Model> for FactSettings {
    fn from(m: &Model) -> Self {
        Self { ber: m.eye_ber, npj: m.eye_npj, pn_lo: m.eye_pn_lo, pn_hi: m.eye_pn_hi, notch: m.eye_notch, hp: m.eye_hp }
    }
}

#[allow(clippy::too_many_arguments)]
fn populate_eye_facts(page: &mut Page, settings: FactSettings, clk: &eye::Clock, tie: &[f64], trend: &[f32], j: &Jitter, meas: &eye::EyeMeas, wave: &[f32], dt: f64, means: &EdgeMeans, rail: Option<(&[f32], f64)>, companion: Option<(&[f32], &[f32], f64)>) -> (Vec<Cell>, Vec<Cell>, Vec<Cell>) {
    let few = format!("needs {} edges", eye::MIN_EDGES);
    let miss = |l| cell(l, few.clone(), Kind::Missing);
    if j.fit == JitterFit::TooFew {
        for name in EYE_FACTS {
            page.put_fig(name, f64::NAN);
        }
        let model = ["TJ@BER", "EW@BER", "J2", "J9", "NPJ"].map(miss).to_vec();
        let measured = ["F/2", "F/4", "F/8", "SRJ", "Pn rms", "Pn note", "TIE pre", "TIE post", "DC CM", "AC CM", "Cross", "T/nT", "SSC ppm", "SSC rate"].map(miss).to_vec();
        let eye_model = vec![cell("Ht@BER", "no eye".into(), Kind::Missing)];
        return (measured, model, eye_model);
    }
    let ber = settings.ber;
    let (tjber, ewber, j2, j9) = (eye::tj_at(j, ber), eye::eye_width_at(j, clk.period, ber), eye::tj_at(j, eye::BER_J2), eye::tj_at(j, eye::BER_J9));
    let npj = if settings.npj { eye::npj(tie, clk, trend, means) } else { f64::NAN };
    let (f2, f4, f8) = (eye::subrate_pp(trend, clk.period, 2), eye::subrate_pp(trend, clk.period, 4), eye::subrate_pp(trend, clk.period, 8));
    let srj = eye::srj_pp(trend, clk.period);
    let pn = eye::phase_noise(trend, clk.period, settings.pn_lo, settings.pn_hi);
    let ps = eye::psij(trend, clk, tie, rail, settings.notch, meas.height, meas.width, Some((wave, dt)));
    let (dc, ac, cross) = companion.map_or_else(
        || (f64::NAN, eye::ac_common_pp(wave, None, dt, settings.hp), f64::NAN),
        |(a, b, dt)| (eye::dc_common(a, b), eye::ac_common_pp(a, Some(b), dt, settings.hp), eye::diff_crossover(a, b)));
    let tnt = eye::tnt_ratio(wave, dt, clk);
    let (ppm, _) = eye::ssc_deviation(clk);
    let ssc_hz = eye::ssc_rate(clk);
    let hber = meas.height_at(ber);
    for (name, v) in [("HBER", hber), ("F2", f2), ("F4", f4), ("F8", f8), ("SRJ", srj), ("PN", pn), ("TPRE", ps.before.tie_pp), ("TPOST", ps.after.tie_pp), ("PPRE", ps.before.pj), ("PPOST", ps.after.pj), ("DCCM", dc), ("ACCM", ac), ("XOVer", cross), ("TNT", tnt), ("SSCPpm", ppm), ("SSCRate", ssc_hz), ("TJBer", tjber), ("EWBer", ewber), ("J2", j2), ("J9", j9), ("NPJ", npj)] {
        page.put_fig(name, v);
    }
    let mut model = vec![
        figure("TJ@BER", tjber, "s", Kind::Model, &few),
        figure("EW@BER", ewber, "UI", Kind::Model, &few),
        figure("J2", j2, "s", Kind::Model, &few),
        figure("J9", j9, "s", Kind::Model, &few),
    ];
    if settings.npj {
        model.push(figure("NPJ", npj, "s", Kind::Model, &few));
    } else {
        model.push(cell("NPJ", "off".into(), Kind::Missing));
    }
    let mut measured = vec![
        figure("F/2", f2, "s", Kind::Measured, &few),
        figure("F/4", f4, "s", Kind::Measured, &few),
        figure("F/8", f8, "s", Kind::Measured, &few),
        figure("SRJ", srj, "s", Kind::Measured, &few),
        figure("Pn rms", pn, "s", Kind::Measured, &few),
        cell("Pn note", if eye::is_clock(clk) { "clock" } else { "data" }.into(), if eye::is_clock(clk) { Kind::Measured } else { Kind::Missing }),
    ];
    measured.push(figure("TIE pre", ps.before.tie_pp, "s", Kind::Measured, &few));
    measured.push(figure("TIE post", ps.after.tie_pp, "s", Kind::Measured, &few));
    measured.push(figure("PJ pre", ps.before.pj, "s", Kind::Measured, &few));
    measured.push(figure("PJ post", ps.after.pj, "s", Kind::Measured, &few));
    measured.push(figure("DC CM", dc, "V", Kind::Measured, "no B"));
    measured.push(figure("AC CM", ac, "V", Kind::Measured, &few));
    measured.push(figure("Cross", cross, "V", Kind::Measured, "no B"));
    measured.push(figure("T/nT", tnt, "dB", Kind::Measured, &few));
    measured.push(figure("SSC ppm", ppm, "ppm", Kind::Measured, &few));
    measured.push(figure("SSC rate", ssc_hz, "Hz", Kind::Measured, &few));
    let eye_model = vec![figure("Ht@BER", hber, "V", Kind::Model, "no eye")];
    (measured, model, eye_model)
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
fn eye_on_grid(mut source: WaveSamp, t0: f64, dt: f64, n: usize) -> Vec<f32> {
    if (source.dt - dt).abs() <= dt.abs() * 1e-9 && (source.t0 - t0).abs() <= dt.abs() * 1e-6 {
        source.v.resize(n, f32::NAN);
        return source.v;
    }
    (0..n).map(|k| {
        let x = (t0 + k as f64 * dt - source.t0) / source.dt;
        if x < 0.0 || x > source.v.len().saturating_sub(1) as f64 { return f32::NAN; }
        let i = x as usize;
        let a = source.v[i];
        let b = source.v[(i + 1).min(source.v.len() - 1)];
        let fraction = (x - i as f64) as f32;
        if a.is_finite() && b.is_finite() { a * (1.0 - fraction) + b * fraction }
        else if a.is_finite() { a } else { b }
    }).collect()
}

/// The coloured hits into `r`. Same size is copied; a column that is mid-slide is scaled so the eye stays put.
fn blit_hits(cv: &mut Canvas, r: Rect, img: &[u32], iw: usize) {
    if iw == 0 || r.w <= 0 || r.h <= 0 || img.len() < iw {
        return;
    }
    let ih = img.len() / iw;
    let x0 = r.x.max(0);
    let y0 = r.y.max(0);
    let x1 = (r.x + r.w).min(cv.w);
    let y1 = (r.y + r.h).min(cv.h);
    if x1 <= x0 || y1 <= y0 || ih == 0 {
        return;
    }
    if iw == r.w as usize && ih == r.h as usize && x0 == r.x && y0 == r.y {
        for (y, row) in img.chunks_exact(iw).take(r.h as usize).enumerate() {
            let o = ((r.y + y as i32) * cv.w + r.x) as usize;
            for (d, &c) in cv.px[o..o + row.len()].iter_mut().zip(row) {
                if c != 0 {
                    *d = c;
                }
            }
        }
        return;
    }
    for y in y0..y1 {
        let sy = ((y - r.y) as usize * ih / r.h as usize).min(ih - 1);
        let src = &img[sy * iw..(sy + 1) * iw];
        let o = (y * cv.w) as usize;
        for x in x0..x1 {
            let c = src[((x - r.x) as usize * iw / r.w as usize).min(iw - 1)];
            if c != 0 {
                cv.px[o + x as usize] = c;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise for the test signals.
    struct Noise(u64);
    impl Noise {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }
        fn gauss(&mut self, sigma: f64) -> f64 {
            let (u, v) = (self.uniform(), self.uniform());
            sigma * (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
        }
    }

    /// What a test signal carries.
    #[derive(Clone, Copy)]
    struct Sig {
        rate: f64,
        uis: usize,
        prbs: bool,
        /// sample steps per UI
        spu: usize,
        rj: f64,
        pj: f64,
        volt_noise: f64,
    }

    /// NRZ of +-1 V with linear edges of a quarter UI.
    fn record(s: Sig, n: &mut Noise) -> Record {
        let ui = 1.0 / s.rate;
        let dt = ui / s.spu as f64;
        let start = n.uniform() * ui;
        let (mut lfsr, mut level) = (0x7fu8, false);
        let mut edges = Vec::new();
        for k in 0..s.uis {
            let bit = if s.prbs {
                let b = ((lfsr >> 6) ^ (lfsr >> 5)) & 1;
                lfsr = ((lfsr << 1) | b) & 0x7f;
                b == 1
            } else {
                k % 2 == 0
            };
            if bit != level {
                let t = k as f64 * ui;
                edges.push((t + n.gauss(s.rj) + s.pj * (2.0 * std::f64::consts::PI * t * s.rate / 37.0).sin(), bit));
                level = bit;
            }
        }
        let (tr, mut at) = (0.25 * ui, 0usize);
        let mut v = Vec::with_capacity(s.uis * s.spu);
        for i in 0..s.uis * s.spu {
            let t = i as f64 * dt + start;
            while at + 1 < edges.len() && edges[at + 1].0 - tr / 2.0 <= t {
                at += 1;
            }
            let (te, up) = edges[at];
            let from = if up { -1.0 } else { 1.0 };
            let y = from + (-2.0 * from) * ((t - te) / tr + 0.5).clamp(0.0, 1.0);
            v.push((y + n.gauss(s.volt_noise)) as f32);
        }
        let mut ch: [Vec<f32>; NCH] = Default::default();
        ch[0] = v;
        Record { t0: 0.0, dt, ch, triggered: true, partial: false, raw: None, la: None }
    }

    fn page(s: Sig, records: usize) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.step(0.05);
        ui.m.run = RunState::Stop;
        ui.m.view = AView::Eye;
        ui.relayout();
        ui.azr.eye = None;
        ui.azr.page = Page::default();
        let mut n = Noise(0x2545_f491_4f6c_dd1d);
        for _ in 0..records {
            ui.last[0] = Some(record(s, &mut n));
            ui.measure_last();
            ui.analyzer_update();
            ui.now += REFRESH;
        }
        ui
    }

    const CLEAN: Sig = Sig { rate: 10e6, uis: 400, prbs: false, spu: 125, rj: 0.0, pj: 0.0, volt_noise: 0.002 };
    const NOISY: Sig = Sig { rate: 100e6, uis: 2000, prbs: true, spu: 12, rj: 0.25e-9, pj: 0.3e-9, volt_noise: 0.04 };

    #[test]
    fn a_tiny_reference_zoom_keeps_full_record_eye_ssc_and_tie_without_duplicate_folds() {
        let rate = 20e6;
        let modulation = 20e3;
        let dt = 1.0 / (64.0 * rate);
        let edges = oos_dsp::hss::ssc_edge_times(4002, rate, 0.01, modulation, "Down-spread only");
        let mut at = 0;
        let samples: Vec<f32> = (0..256_000).map(|i| {
            let t = i as f64 * dt;
            while at + 1 < edges.len() && edges[at + 1] <= t { at += 1; }
            let level = if at % 2 == 0 { 1.0 } else { -1.0 };
            // A short linear transition, with all edge times coming from the SSC ramp.
            let fraction = ((t - edges[at]) / (0.1 / rate)).clamp(0.0, 1.0);
            (-level + 2.0 * level * fraction) as f32
        }).collect();
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.session_is_sim = false;
        ui.set_offline(true);
        ui.m.sim_slot = 0;
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "SSC clock".into(), t0: 0.0, dt, v: samples, vpd: 0.5, offset: 0.0, on: true });
        ui.m.eye_src = WaveId::Ref(0);
        ui.m.eye_rate = rate;
        ui.m.zoom = true;
        ui.m.zscale = tscales().iter().position(|&scale| scale as f64 >= 0.2 / rate / HDIV as f64).unwrap();
        ui.m.zdelay = 50e-6;
        ui.refresh_stopped_reference();
        assert!(ui.reference_slice(0).unwrap().0.len() < 64, "ordinary measurements still use the tiny view");
        ui.m.view = AView::Eye;
        ui.relayout();
        ui.eye_update(&Ui::blank_record());
        assert_eq!(ui.azr.page.records, 1);
        assert_eq!(ui.azr.eye.as_ref().unwrap().records, 1);
        assert!(ui.azr.page.edges > 3000, "full record TIE: {} edges", ui.azr.page.edges);
        assert!(ui.azr.page.title.contains("full record"), "{}", ui.azr.page.title);
        let reading = |ui: &Ui, name| ui.azr.page.readings.iter().find(|(n, _)| *n == name).unwrap().1;
        let ssc = reading(&ui, "SSCRate");
        assert!((ssc / modulation - 1.0).abs() < 0.25, "SSC {ssc} Hz, expected {modulation}");
        // Downspread has a 1% peak-to-peak local rate span. Same-polarity
        // averaging and interpolation at 64 samples/UI introduce a small error.
        let ppm = reading(&ui, "SSCPpm");
        assert!((ppm / 10_000.0 - 1.0).abs() < 0.03, "SSC depth {ppm} ppm, expected 10000 ppm");
        let hits = ui.azr.eye.as_ref().unwrap().hits.clone();
        let identity = ui.azr.eye_ref;
        for delay in [20e-6, 80e-6, 120e-6] {
            ui.m.zdelay = delay;
            ui.now += REFRESH;
            ui.refresh_stopped_reference();
            assert_eq!(ui.azr.page.records, 1);
            assert_eq!(ui.azr.eye.as_ref().unwrap().hits, hits);
            assert_eq!(ui.azr.eye_ref, identity);
            assert_eq!(reading(&ui, "SSCRate"), ssc);
        }
        // A waveform edit away from the old three fingerprint samples is still a new record.
        ui.refs[0].as_mut().unwrap().v[1000] *= 0.8;
        ui.eye_update(&Ui::blank_record());
        assert_ne!(ui.azr.eye_ref, identity);
        assert_eq!(ui.azr.page.records, 1);
        let waveform_identity = ui.azr.eye_ref;
        ui.m.eye_ber = 1e-6;
        ui.eye_update(&Ui::blank_record());
        assert_ne!(ui.azr.eye_ref, waveform_identity, "settings refresh the cached figures");
        assert_eq!(ui.azr.page.records, 1);
        ui.refs[0].as_mut().unwrap().v.fill(0.0);
        ui.eye_update(&Ui::blank_record());
        assert!(ui.azr.eye.is_none() && ui.azr.eye_res.is_none(), "an edgeless replacement cannot keep stale results");
    }

    #[test]
    fn the_page_tiles_the_area_to_its_bottom_edge() {
        for h in [160, 240, 296, 300, 352, 400, 440, 480] {
            let a = Rect::new(0, 40, W, h);
            let l = layout(a, false);
            let bottom = a.y + a.h;
            assert_eq!(l.charts[2].y + l.charts[2].h + if l.jit_rows > 0 { GAP + l.jit.h } else { 0 }, bottom, "right column, height {}", h);
            assert_eq!(l.charts[3].y + l.charts[3].h, l.charts[2].y + l.charts[2].h);
            assert_eq!(l.eye.y + l.eye.h + if l.fig_rows > 0 { GAP + l.fig.h } else { 0 }, bottom, "left column, height {}", h);
            assert_eq!(l.charts[1].x + l.charts[1].w, a.x + a.w, "height {}", h);
            assert!(l.plots.iter().all(|p| p.h >= 1 && p.w > 40), "{:?}", l.plots);
            assert_eq!(l.fig_rows > 0, h >= BOXES_MIN_H);
        }
    }

    #[test]
    fn full_screen_gives_the_eye_the_whole_plot() {
        let a = Rect::new(0, 0, W, H);
        let l = layout(a, true);
        assert_eq!((l.fig_rows, l.jit_rows), (0, 0));
        assert!(l.plot.w > 900 && l.plot.h > 500, "{:?}", l.plot);
        assert!(l.charts.iter().all(|c| c.w == 0));
    }

    #[test]
    fn narrow_pane_stacks_the_charts_and_hiding_them_gives_the_eye_its_height() {
        let area = Rect::new(600, 80, 400, 400);
        let home = layout(area, false);
        assert_eq!((home.eye.x, home.eye.w), (area.x, area.w));
        assert!(home.charts.iter().all(|chart| chart.y >= home.eye.y + home.eye.h));
        let hidden = placed(area, 0.0);
        assert_eq!((hidden.eye.y, hidden.eye.h), (area.y, area.h));
        assert!(hidden.charts.iter().all(|chart| chart.x >= area.x + area.w));
        let short = layout(Rect::new(0, 350, 900, 160), false);
        assert!(short.eye.h > 0 && short.plot.h > 0);
    }

    #[test]
    fn the_eye_range_holds_the_signal_on_whole_steps() {
        for (lo, hi) in [(-1.0, 1.0), (0.0, 3.3), (0.1, 0.18), (-0.002, 0.003), (47.0, 53.0)] {
            let (a, b, step) = eye_range(lo, hi);
            assert!(a < lo && b > hi, "{} {}", a, b);
            let n = (b - a) / step;
            assert!((n - n.round()).abs() < 1e-6 && (3.0..=9.0).contains(&n), "{} {} {}", a, b, step);
            assert!((hi - lo) / (b - a) > 0.4, "{} {}", a, b);
        }
    }

    /// The eye fits the signal, not the scale of the channel, and a change of the signal gets a new range.
    #[test]
    fn the_eye_follows_the_signal() {
        let mut ui = page(CLEAN, 3);
        let e = ui.azr.eye.as_ref().unwrap();
        assert!(e.v_max >= 1.0 && e.v_max < 2.0 && e.v_min <= -1.0 && e.v_min > -2.0, "{} {}", e.v_min, e.v_max);
        let mut rec = ui.last[0].clone().unwrap();
        rec.ch[0].iter_mut().for_each(|x| *x *= 0.1);
        ui.last[0] = Some(rec);
        ui.measure_last();
        ui.analyzer_update();
        let e = ui.azr.eye.as_ref().unwrap();
        assert!(e.v_max < 0.5, "{}", e.v_max);
    }

    /// Few edges of a clock, every record on the same polarity: the figures are those of the signal.
    #[test]
    fn a_clock_with_few_edges_has_a_full_eye() {
        let ui = page(Sig { uis: 10, ..CLEAN }, 6);
        let (m, _, _) = ui.azr.eye_res.clone().expect("eye result");
        assert!((m.amplitude - 2.0).abs() < 0.1, "{:?}", m);
        assert!(m.width.is_finite() && m.crossing_pct.is_finite(), "{:?}", m);
    }

    /// An idle record neither restarts the page nor moves the range.
    #[test]
    fn a_record_without_edges_keeps_the_eye() {
        let mut ui = page(CLEAN, 5);
        let (top, n) = (ui.azr.eye.as_ref().unwrap().v_max, ui.azr.page.records);
        let mut rec = ui.last[0].clone().unwrap();
        rec.ch[0].iter_mut().for_each(|x| *x = 0.01);
        ui.last[0] = Some(rec);
        ui.measure_last();
        ui.analyzer_update();
        assert_eq!((ui.azr.eye.as_ref().unwrap().v_max, ui.azr.page.records), (top, n));
        assert!(ui.azr.page.status[1].contains("Under 3 edges"), "{:?}", ui.azr.page.status);
    }

    #[test]
    fn disabled_eye_source_and_clock_ignore_samples_retained_in_the_last_record() {
        let mut ui = page(CLEAN, 2);
        let mut rec = ui.last[0].clone().unwrap();
        assert!(!rec.ch[0].is_empty());
        let records = ui.azr.page.records;
        ui.m.ch[0].on = false;
        ui.eye_update(&rec);
        assert_eq!(ui.azr.page.status[0], "CH1 is off");
        assert_eq!(ui.azr.page.records, records);
        ui.m.ch[0].on = true;
        rec.ch[1] = rec.ch[0].clone();
        ui.m.ch[1].on = false;
        ui.m.eye_b = Some(WaveId::Ch(1));
        ui.eye_update(&rec);
        assert_eq!(ui.azr.page.status[0], "CH2 is off");
        assert_eq!(ui.azr.page.records, records);
    }

    /// Every figure the page draws has one number for SCPI, including the ones that came out missing.
    #[test]
    fn every_figure_has_one_number() {
        let ui = page(CLEAN, 2);
        let got: Vec<_> = ui.azr.page.readings.iter().map(|(n, _)| *n).collect();
        for name in EYE_ITEMS {
            assert_eq!(got.iter().filter(|n| *n == name).count(), 1, "{name} in {got:?}");
        }
        assert_eq!(got.len(), EYE_ITEMS.len());
        assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "HEIGht" && v.is_finite()));
        assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "WIDTh" && v.is_finite()));
        assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "UI" && v.is_finite()));
        assert!(ui.azr.page.readings.iter().any(|(n, v)| *n == "TJ" && v.is_finite()));
    }

    /// Time of the parts on a deep record: `cargo test --release eye_page_timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn eye_page_timing() {
        let s = Sig { rate: 10e6, uis: 8000, prbs: true, spu: 125, rj: 50e-12, pj: 0.0, volt_noise: 0.01 };
        let mut ui = page(s, 3);
        let mut cv = Canvas::new(W, H);
        let t = std::time::Instant::now();
        for _ in 0..20 {
            ui.analyzer_update();
        }
        println!("update, no refresh {:.2} ms", t.elapsed().as_secs_f64() * 50.0);
        let t = std::time::Instant::now();
        for _ in 0..20 {
            ui.now += REFRESH;
            ui.analyzer_update();
        }
        println!("update with refresh {:.2} ms", t.elapsed().as_secs_f64() * 50.0);
        let t = std::time::Instant::now();
        for _ in 0..200 {
            ui.draw_eye(&mut cv);
        }
        println!("draw {:.3} ms", t.elapsed().as_secs_f64() * 5.0);
        let rec = ui.last[0].clone().unwrap();
        let v = &rec.ch[0];
        let clk = eye::recover_clock(v, rec.dt, 0.0, 0.1, None).unwrap();
        for span in [1, 2] {
            let mut e = eye::Eye::new(530, 340, span, -1.5, 1.5);
            let t = std::time::Instant::now();
            for _ in 0..10 {
                e.accumulate(v, rec.dt, &clk);
            }
            println!("accumulate {} samples, span {}: {:.2} ms", v.len(), span, t.elapsed().as_secs_f64() * 100.0);
        }
    }

    #[test]
    fn nothing_sits_under_the_overflow_button() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.view = AView::Eye;
        for w in [640, W] {
            for h in [160, 352, 480] {
                ui.workspace.eye_float = Some(Rect::new(0, 40, w, h));
                ui.relayout();
                let content = ui.eye_page_rect();
                let close = ui.eye_pane_close();
                assert!(content.y >= close.y + close.h, "workspace header must reserve Close before laying out any charts");
                let l = layout(content, false);
                for r in [l.eye, l.head, l.fig, l.jit].iter().chain(&l.charts) {
                    assert!(!overlaps(*r, close), "{:?} sits under actual Close {:?}", r, close);
                }
            }
        }
    }

    /// Every title and figure inside its box, with the longest values the page can show.
    #[test]
    fn the_text_fits_its_boxes() {
        for s in [CLEAN, NOISY, Sig { uis: 6, ..CLEAN }] {
            let ui = page(s, 8);
            let (f, p) = (&ui.fonts, &ui.azr.page);
            let l = layout(Rect::new(0, 40, W, 400), false);
            for (k, c) in p.charts.iter().enumerate() {
                assert!(f.sb.width(&c.title) + 12 <= l.charts[k].w, "'{}'", c.title);
                assert!(f.s.width(&c.empty) <= l.plots[k].w, "'{}'", c.empty);
                assert!(f.s.width(&c.y.0).max(f.s.width(&c.y.1)) + 3 <= CHART_GUTTER, "{:?}", c.y);
            }
            assert!(f.sb.width(&p.status[0]) + 2 <= l.head.w && f.s.width(&p.status[1]) + 2 <= l.head.w, "{:?}", p.status);
            assert!(f.sb.width(&p.title) + 6 + 196 <= l.eye.w, "'{}'", p.title);
            let room = |r: Rect, n: i32| (r.w - 12) / n - 4;
            for cells in [&p.eye_measured, &p.eye_model, &p.stats] {
                for c in cells {
                    assert!(f.s.width(c.label) <= LABEL_W - 2, "'{}'", c.label);
                    assert!(LABEL_W + f.s.width(&c.value) <= room(l.fig, 3), "'{}'", c.value);
                }
            }
            for cells in [&p.measured, &p.model] {
                for c in cells {
                    assert!(f.s.width(c.label) <= LABEL_W - 2, "'{}'", c.label);
                    assert!(LABEL_W + f.s.width(&c.value) <= room(l.jit, 2), "'{}'", c.value);
                }
            }
        }
    }

    #[test]
    fn few_edges_per_record_pool_into_a_result() {
        let ui = page(Sig { uis: 12, ..CLEAN }, 200);
        let (_, j, _) = ui.azr.eye_res.clone().expect("eye result");
        assert!(j.rj_rms.is_finite() && j.tj_pp.is_finite(), "{:?}", j.fit);
        assert_ne!(j.fit, JitterFit::TooFew);
        assert!(ui.azr.page.pool.len() > 1000);
    }

    /// No number stands in for one that was not computed.
    #[test]
    fn too_little_data_says_so() {
        let ui = page(Sig { uis: 6, ..CLEAN }, 3);
        let p = &ui.azr.page;
        assert!(p.status[1].contains("needs"), "{:?}", p.status);
        assert!(p.pool.is_empty());
        assert!(!p.charts[1].empty.is_empty() && !p.charts[2].empty.is_empty() && !p.charts[3].empty.is_empty());
        assert!(p.model.iter().all(|c| c.kind == Kind::Missing), "{:?}", p.model);
        for c in p.measured.iter().chain(&p.model).chain(&p.eye_measured).chain(&p.eye_model).chain(&p.stats) {
            assert!(!c.value.contains("---") && !c.value.contains("NaN") && !c.value.contains("inf"), "{:?}", c);
        }
    }

    #[test]
    fn a_noisy_signal_fills_every_chart() {
        let ui = page(NOISY, 30);
        let p = &ui.azr.page;
        let (_, j, period) = ui.azr.eye_res.clone().expect("eye result");
        assert_eq!(j.fit, JitterFit::Tails);
        assert!((period - 10e-9).abs() < 1e-11, "{}", period);
        assert!(p.charts.iter().all(|c| c.empty.is_empty()), "{:?}", p.charts);
        assert!(!p.trend.is_empty() && !p.hist.is_empty() && !p.spec.is_empty() && !p.tub.model.is_empty());
        assert!(p.measured.iter().filter(|c| c.label == "DDJ" || c.label == "PJ rms").all(|c| c.kind == Kind::Measured), "{:?}", p.measured);
        assert!(p.model.iter().any(|c| c.label == "TJ 1e-12" && c.kind == Kind::Model));
    }

    #[test]
    fn clear_starts_the_page_over() {
        let mut ui = page(CLEAN, 5);
        assert!(!ui.azr.page.img.is_empty());
        ui.activate(Item::EyeClear);
        assert_eq!(ui.m.view, AView::Eye);
        assert!(ui.azr.eye.is_none() && ui.azr.page.pool.is_empty() && ui.azr.page.img.is_empty());
        ui.render(&mut Canvas::new(W, H));
    }

    #[test]
    fn another_signal_starts_the_page_over() {
        let mut ui = page(CLEAN, 5);
        let mut n = Noise(77);
        ui.last[0] = Some(record(Sig { rate: 20e6, ..CLEAN }, &mut n));
        ui.analyzer_update();
        assert_eq!(ui.azr.page.records, 1);
    }

    #[test]
    fn it_draws_at_every_height_and_with_the_source_off() {
        let mut ui = page(NOISY, 4);
        let mut cv = Canvas::new(W, H);
        for h in [160, 240, 352, 480] {
            ui.area.h = h;
            ui.analyzer_update();
            ui.draw_eye(&mut cv);
        }
        let mut rec = ui.last[0].clone().unwrap();
        rec.ch[0].clear();
        ui.last[0] = Some(rec);
        ui.analyzer_update();
        assert!(ui.azr.page.status[0].contains("off"));
        ui.draw_eye(&mut cv);
    }

    fn separated(a: Rect, b: Rect) -> bool {
        a.w > 0 && b.w > 0 && a.h > 0 && b.h > 0 && !(a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h)
    }

    #[test]
    fn selected_eye_source_keeps_its_identity_and_updates_only_automatic_rate() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_offline(true);
        let mut noise = Noise(3);
        for (slot, signal) in [(0, CLEAN), (1, Sig { rate: CLEAN.rate * 2.0, ..CLEAN })] {
            let rec = record(signal, &mut noise);
            ui.refs[slot] = Some(RefWave { src: 0, color: slot, label: format!("Rate {slot}"), t0: rec.t0, dt: rec.dt,
                v: rec.ch[0].clone(), vpd: 0.2, offset: 0.0, on: true });
        }
        ui.select_workspace_source(WaveId::Ref(1));
        ui.eye_make();
        assert_eq!(ui.m.eye_src, WaveId::Ref(1));
        assert!((ui.m.eye_rate - CLEAN.rate * 2.0).abs() / (CLEAN.rate * 2.0) < 0.02);
        ui.adjust(Item::EyeSrc, -1);
        assert_eq!(ui.m.eye_src, WaveId::Ref(0));
        assert!((ui.m.eye_rate - CLEAN.rate).abs() / CLEAN.rate < 0.02);
        ui.m.eye_from_record = false;
        ui.m.eye_rate = 125e6;
        ui.adjust(Item::EyeSrc, 1);
        assert_eq!(ui.m.eye_rate, 125e6, "a deliberately manual rate survives source selection");
        ui.refs[1].as_mut().unwrap().v.fill(0.0);
        ui.eye_make();
        assert_eq!(ui.m.eye_src, WaveId::Ref(1));
        assert_eq!(ui.m.eye_rate, 0.0, "a flat selected record does not silently use another source's clock");
    }

    /// One press reads the bit rate from the record. The eye starts beside the trace; Under stacks it below.
    #[test]
    fn one_press_reads_the_rate_and_keeps_the_trace() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.eye_rate = 123.0;
        let mut n = Noise(3);
        ui.last[0] = Some(record(CLEAN, &mut n));
        ui.render(&mut Canvas::new(W, H));
        ui.open_menu(MenuId::Eye);
        let b = ui.dlg_layout().unwrap().rows.into_iter().find(|row| row.it == Item::EyeAct).unwrap().ctrl;
        for phase in [Phase::Down, Phase::Up] { ui.handle(Ev::Touch { id: 0, phase, x: b.x+b.w/2, y: b.y+b.h/2 }); }
        assert_eq!(ui.m.view, AView::Eye);
        assert!(ui.m.eye_from_record);
        assert_eq!(ui.m.eye_place, EyePlace::Side);
        assert!((ui.m.eye_rate - CLEAN.rate).abs() / CLEAN.rate < 0.02, "rate {}", ui.m.eye_rate);
        ui.render(&mut Canvas::new(W, H));
        let (wave, eye) = (ui.grid, ui.eye_pane_rect());
        assert!(separated(wave, eye), "wave {:?} eye {:?}", wave, eye);
        assert!(eye.x >= wave.x + wave.w);
        assert!(wave.w > eye.w && eye.w > 250, "wave {:?} eye {:?}", wave, eye);
        let (x, y) = (eye.x + 20, eye.y + 12);
        ui.context_menu(x, y);
        let popup_x = x.clamp(4, W - 244 - 4);
        let popup_y = y.clamp(TOP.h, (BOTTOM.y - (6 * 30 + 8)).max(TOP.h));
        ui.tap(popup_x + 24, popup_y + 4 + 2 * 30 + 15); // Dock Eye below.
        ui.render(&mut Canvas::new(W, H));
        let (wave, eye) = (ui.grid, ui.eye_pane_rect());
        assert_eq!(ui.m.eye_place, EyePlace::Under);
        assert!(separated(wave, eye), "wave {:?} eye {:?}", wave, eye);
        assert!(eye.y >= wave.y + wave.h - 1);
        assert!(eye.w + 2 >= wave.w);
        let c = ui.eye_pane_close();
        ui.tap(c.x + 4, c.y + 4);
        assert_eq!(ui.m.view, AView::Scope);
        assert!(!ui.eye_open());
    }

    /// The colours come from the names in ui.rs, so a new look reaches this page.
    #[test]
    fn no_hard_coded_colours() {
        let src = include_str!("eyepage.rs");
        let call = ["rgb", "("].concat();
        assert!(!src.contains(&call));
    }

    #[test]
    fn the_column_slides_off_and_the_eye_takes_its_room() {
        let mut ui = page(CLEAN, 2);
        let home = layout(ui.eye_page_rect(), false);
        assert_eq!(placed(ui.eye_page_rect(), 1.0), home);
        let tab = ui.eye_tab();
        ui.tap(tab.x + 8, tab.y + 8);
        assert_eq!(ui.eye_to, 0.0);
        assert!(ui.eye_column_open() > 0.9, "{}", ui.eye_column_open());
        ui.now += SLIDE * 0.5;
        let mid = ui.eye_column_open();
        assert!(mid > 0.35 && mid < 0.65, "{}", mid);
        let l = placed(ui.eye_page_rect(), mid);
        assert!(l.charts[0].x > home.charts[0].x && (l.eye.w > home.eye.w || l.eye.h > home.eye.h), "{:?}", l);
        ui.now += SLIDE;
        assert!(ui.eye_column_open() < 0.001, "{}", ui.eye_column_open());
        let shut = placed(ui.eye_page_rect(), 0.0);
        assert_eq!(shut.eye.x + shut.eye.w, ui.eye_page_rect().x + ui.eye_page_rect().w);
        assert!(shut.charts[0].x >= ui.eye_page_rect().x + ui.eye_page_rect().w);
        if home.fig_rows > 0 {
            assert_eq!((shut.fig.x, shut.fig.w), (shut.eye.x, shut.eye.w));
        }
        let tab = ui.eye_tab();
        ui.tap(tab.x + 8, tab.y + 8);
        ui.now += SLIDE;
        assert!((ui.eye_column_open() - 1.0).abs() < 0.001, "{}", ui.eye_column_open());
        assert_eq!(placed(ui.eye_page_rect(), ui.eye_column_open()), home);
    }

    /// Hiding the column must not throw away the eye or the pooled edges.
    #[test]
    fn sliding_the_column_keeps_the_eye() {
        let mut ui = page(CLEAN, 5);
        let (n, pool, w) = (ui.azr.page.records, ui.azr.page.pool.len(), ui.azr.page.img_w);
        assert!(w > 0 && n > 1 && pool > 100, "{} {} {}", n, pool, w);
        let tab = ui.eye_tab();
        ui.tap(tab.x + tab.w / 2, tab.y + tab.h / 2);
        ui.now += SLIDE;
        ui.analyzer_update();
        assert!(ui.azr.page.records > 1 && ui.azr.page.pool.len() >= pool, "{} {}", ui.azr.page.records, ui.azr.page.pool.len());
        assert_eq!(ui.azr.page.img_w, w);
        assert!(ui.eye_column_open() < 0.01);
    }

    #[test]
    fn the_column_tab_stays_clear_of_the_overflow_button() {
        let mut ui = page(CLEAN, 1);
        for open in [0.0, 0.35, 1.0] {
            ui.eye_from = open;
            ui.eye_to = open;
            ui.eye_t = ui.now;
            let tab = ui.eye_tab();
            assert!(!overlaps(tab, ui.view_btn()), "open {} tab {:?} btn {:?}", open, tab, ui.view_btn());
            assert!(tab.w >= 40 && tab.h >= 40, "{:?}", tab);
            assert!(ui.eye_page_rect().contains(tab.x + 1, tab.y + 1) && ui.eye_page_rect().contains(tab.x + tab.w - 2, tab.y + tab.h - 2), "{:?}", tab);
        }
    }

    #[test]
    fn a_tap_on_a_plot_fills_the_chart_area_and_a_second_tap_restores_the_four() {
        let mut ui = page(NOISY, 4);
        let home = layout(ui.eye_page_rect(), false);
        for k in 0..4 {
            let r = home.charts[k];
            ui.tap(r.x + r.w / 2, r.y + r.h / 2);
            assert_eq!(ui.eye_big, Some(k as u8));
            let (charts, plots) = charts_shown(&home, Some(k as u8));
            let span = home.charts[1].x + home.charts[1].w - home.charts[0].x;
            assert_eq!(charts[k].w, span);
            assert!(plots[k].w > home.plots[k].w);
            ui.tap(charts[k].x + 12, charts[k].y + 12);
            assert_eq!(ui.eye_big, None);
        }
    }

    /// With the column away, a tap where a plot used to be does not open one.
    #[test]
    fn a_hidden_column_has_no_plot_to_open() {
        let mut ui = page(CLEAN, 2);
        let home = layout(ui.eye_page_rect(), false);
        ui.eye_from = 0.0;
        ui.eye_to = 0.0;
        ui.eye_t = ui.now;
        let r = home.charts[2];
        ui.tap(r.x + 8, r.y + 8);
        assert_eq!(ui.eye_big, None);
    }

    #[test]
    fn the_heat_scale_stays_clear_of_the_top_buttons() {
        let mut ui = page(CLEAN, 2);
        let home = layout(ui.eye_page_rect(), false);
        assert_eq!(ui.heat_x(home.plot), home.plot.x + home.plot.w - 128);
        ui.eye_from = 0.0;
        ui.eye_to = 0.0;
        ui.eye_t = ui.now;
        let r = placed(ui.eye_page_rect(), 0.0).plot;
        let kx = ui.heat_x(r);
        let scale = Rect::new(kx - 30, ui.eye_page_rect().y, 68 + ui.fonts.s.width("dense") + 34, 18);
        if ui.full_btn().0.x >= r.x {
            assert!(scale.x + scale.w <= ui.full_btn().0.x - 8, "{:?} {:?}", scale, ui.full_btn().0);
        }
        assert!(!overlaps(scale, ui.full_btn().0) && !overlaps(scale, ui.view_btn()), "{:?}", scale);
    }

    #[test]
    fn it_draws_while_the_column_moves_and_a_plot_is_open() {
        let mut ui = page(NOISY, 4);
        let mut cv = Canvas::new(W, H);
        for (open, big) in [(1.0, None), (0.6, Some(1)), (0.25, Some(3)), (0.0, None)] {
            ui.eye_from = open;
            ui.eye_to = open;
            ui.eye_t = ui.now;
            ui.eye_big = big;
            ui.draw_eye(&mut cv);
        }
    }

    /// Pictures for a visual check: `OOS_SHOTS=<dir> cargo test eye_page_shots -- --ignored`.
    #[test]
    #[ignore]
    fn eye_page_shots() {
        let dir = std::env::var("OOS_SHOTS").unwrap_or_else(|_| ".".into());
        let few = Sig { uis: 10, ..CLEAN };
        let save = |ui: &mut Ui, name: &str| {
            let mut cv = Canvas::new(W, H);
            ui.render(&mut cv);
            std::fs::write(format!("{}/{}.png", dir, name), crate::store::png_of(&cv)).unwrap();
        };
        for (name, s, n) in [("eye-clean", CLEAN, 40), ("eye-noisy", NOISY, 60), ("eye-few", few, 6), ("eye-tiny", Sig { uis: 6, ..CLEAN }, 3)] {
            let mut ui = page(s, n);
            save(&mut ui, name);
            ui.m.run = RunState::Stop;
        }
        let mut shown = page(NOISY, 40);
        save(&mut shown, "eye-column-shown");
        let tab = shown.eye_tab();
        shown.tap(tab.x + 8, tab.y + 8);
        shown.now += SLIDE;
        save(&mut shown, "eye-column-hidden");
        let mut open = page(NOISY, 40);
        let r = layout(open.eye_page_rect(), false).charts[3];
        open.tap(r.x + r.w / 2, r.y + r.h / 2);
        open.now += REFRESH;
        open.analyzer_update();
        save(&mut open, "eye-plot-expanded");
    }
}
#[cfg(test)]
mod automatic_setup_tests {
    use super::*;

    fn reference(values: Vec<f32>, dt: f64) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_offline(true);
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "Auto test".into(), t0: -1e-6,
            dt, v: values, vpd: 0.2, offset: 0.0, on: true });
        ui.select_workspace_source(WaveId::Ref(0));
        ui.m.run = RunState::Stop;
        ui
    }

    fn square(samples: usize, samples_per_ui: usize) -> Vec<f32> {
        (0..samples).map(|i| if i / samples_per_ui % 2 == 0 { -1.0 } else { 1.0 }).collect()
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
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
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
        for _ in 0..20 { ui.m.zdelay += 1e-9; ui.analyzer_update(); }
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
        ui.refs[0].as_mut().unwrap().v = square(300_000, 10);
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
        for _ in 0..30 { ui.m.zdelay += 1e-9; ui.analyzer_update(); }
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
