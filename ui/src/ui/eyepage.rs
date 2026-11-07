// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The eye and jitter page. `eye_update` runs per record: the eye and the cheap sums there, everything else (fits,
//! charts, the coloured eye, all text) at most every `REFRESH` seconds into `Page`, so drawing only copies and plots.
//!
//! TIE is pooled over records: each record is timed against its own clock, so its TIE stands alone, and a window with
//! few edges still adds up to a usable distribution. What drifts slower than one record is taken out by the fit.

use super::*;
use oos_dsp::eye::{self, Bathtub, EdgeMeans, Jitter, JitterFit};

/// Seconds between refreshes of the page.
const REFRESH: f64 = 0.2;
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
/// Top of the right column: stays clear of the overflow button.
const HEAD_H: i32 = 44;
const ROW: i32 = 15;
const BOX_HEAD: i32 = 25;
const CHART_TITLE: i32 = 17;
const CHART_FOOT: i32 = 14;
const CHART_GUTTER: i32 = 50;
const EYE_TITLE: i32 = 20;
const EYE_GUTTER: i32 = 50;
const EYE_FOOT: i32 = 18;
/// Under this height the text boxes give their room to the plots.
const BOXES_MIN_H: i32 = 300;

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

pub(super) fn layout(a: Rect) -> Layout {
    let lw = a.w * 57 / 100;
    let (rx, rw) = (a.x + lw + GAP, a.w - lw - GAP);
    let bottom = a.y + a.h;
    let rows = |pct: i32, most: i32| if a.h < BOXES_MIN_H { 0 } else { ((a.h * pct / 100 - BOX_HEAD) / ROW).clamp(3, most) };
    let (fig_rows, jit_rows) = (rows(22, 5), rows(30, 7));
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
                Chart { title: "Bathtub".into(), ..wait },
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
        }
    }
}

impl Page {
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
        *c = Chart { title: "Bathtub".into(), ..Chart::default() };
        self.tub = eye::bathtub(sorted, j, period, cols.max(2));
        if self.tub.model.is_empty() {
            c.empty = format!("Needs {} edges", eye::MIN_EDGES);
            return;
        }
        let limit = if j.fit == JitterFit::Limit { "over " } else { "" };
        c.title = format!("Bathtub  {}{:.3} UI at 1e-12", limit, self.tub.opening);
        c.x = ("0".into(), "1 UI".into());
        c.y = ("1".into(), format!("1e{:.0}", TUB_FOOT));
    }

    fn text_jitter(&mut self, j: &Jitter, pj: f64, pj_why: &str) {
        let few = format!("needs {} edges", eye::MIN_EDGES);
        let means = format!("needs {} per edge", eye::RUN_MIN);
        let ddj = self.means.ddj();
        let dcd = self.means.dcd();
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
    /// Eye page, per record.
    pub(super) fn eye_update(&mut self, rec: &Record) {
        let c = self.m.eye_src;
        if rec.ch[c].is_empty() {
            self.azr.page.status = [format!("CH{} is off", c + 1), "Switch the source on".into()];
            return;
        }
        let (v, dt) = self.meas_wave(rec, c);
        let ch = &self.m.ch[c];
        let vpd = ch.volts_per_div();
        let lay = layout(self.area);
        let (pw, ph) = (lay.plot.w.max(64), lay.plot.h.max(64));
        let key = (c, vpd.to_bits(), ch.offset.to_bits(), self.m.eye_rate.to_bits(), pw, ph);
        let st = self.stats[c];
        let thr = if st.valid { 0.5 * (st.top + st.base) } else { 0.5 * (st.vmax + st.vmin) };
        let rate = (self.m.eye_rate > 0.0).then_some(self.m.eye_rate);
        // a record without edges changes nothing: an idle bus must not cost the eye its range or its hits
        let Some(clk) = eye::recover_clock(&v, dt, thr, 0.1 * vpd, rate) else {
            self.azr.page.status[1] = format!("Under 3 edges on CH{} in the last record", c + 1);
            return;
        };
        let left = self.azr.eye.as_ref().is_some_and(|e| {
            let (lo, hi) = (e.v_min as f64, e.v_max as f64);
            let (span, swing) = (hi - lo, (st.vmax - st.vmin) as f64);
            st.vmax as f64 > hi + EYE_OUT * span || (st.vmin as f64) < lo - EYE_OUT * span || swing < EYE_SHRUNK * span
        });
        if self.azr.eye.is_none() || self.azr.eye_key != key || left {
            // the eye fills its plot whatever the scale of the channel; the signal itself sets the range
            let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x as f64), b.max(x as f64)));
            let (lo, hi, step) = eye_range(lo, hi);
            self.azr.eye = Some(eye::Eye::new(pw as usize, ph as usize, 2, lo as f32, hi as f32));
            self.azr.eye_key = key;
            self.azr.eye_res = None;
            self.azr.page = Page { vstep: step, ..Page::default() };
        }
        let last = self.azr.page.ui.mean();
        if last.is_finite() && (clk.period / last - 1.0).abs() > UI_CHANGE {
            self.azr.eye.as_mut().unwrap().clear();
            self.azr.page = Page { vstep: self.azr.page.vstep, ..Page::default() };
        }
        self.azr.eye.as_mut().unwrap().accumulate(&v, dt, &clk);
        let tie = eye::tie(&clk.edges, &clk);
        self.azr.page.add(&tie, &clk);
        if self.now - self.azr.page.t >= REFRESH {
            self.azr.page.t = self.now;
            self.eye_refresh(&clk, &tie, dt, &lay);
        }
    }

    /// The heavy part of the page, from the pool and the last record.
    fn eye_refresh(&mut self, clk: &eye::Clock, tie: &[f64], dt: f64, lay: &Layout) {
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
        p.chart_trend(&trend, clk, lay.plots[0].w as usize);
        p.chart_hist(&j);
        let (pj, why) = p.chart_spec(&trend, clk, lay.plots[2].w as usize);
        p.chart_tub(&sorted, &j, clk.period, lay.plots[3].w as usize);
        p.text_jitter(&j, pj, &why);
        p.text_eye(e, &meas, clk.period, dt);
        p.text_status(&j);
        let clock = if self.m.eye_rate > 0.0 { "clock fitted near the set rate" } else { "clock fitted to the edges" };
        p.title = format!("CH{} \u{b7} {} \u{b7} {}", self.m.eye_src + 1, si(1.0 / clk.period, "b/s"), clock);
        self.azr.eye_res = Some((meas, j, clk.period));
    }

    pub(super) fn draw_eye(&self, cv: &mut Canvas) {
        let a = self.area;
        let p = &self.azr.page;
        let l = layout(a);
        cv.fill(a, BG);
        self.eye_plot(cv, &l);
        self.eye_head(cv, &l);
        let col = CH_COLORS[self.m.eye_src];
        for k in 0..4 {
            self.chart_frame(cv, l.charts[k], l.plots[k], &p.charts[k]);
            if p.charts[k].empty.is_empty() {
                match k {
                    0 => Self::plot_trend(cv, l.plots[0], &p.trend, col),
                    1 => Self::plot_hist(cv, l.plots[1], &p.hist, col),
                    2 => Self::plot_spec(cv, l.plots[2], &p.spec, col),
                    _ => self.plot_tub(cv, l.plots[3], &p.tub),
                }
            }
        }
        if l.fig_rows > 0 {
            self.eye_box(cv, l.fig, "Eye figures", l.fig_rows, &[("Measured", &p.eye_measured), ("Model", &p.eye_model), ("Records", &p.stats)]);
        }
        if l.jit_rows > 0 {
            self.eye_box(cv, l.jit, "Jitter breakdown", l.jit_rows, &[("Measured", &p.measured), ("Model", &p.model)]);
        }
    }

    fn eye_head(&self, cv: &mut Canvas, l: &Layout) {
        let f = &self.fonts;
        let p = &self.azr.page;
        cv.text(&f.sb, l.head.x + 2, l.head.y + 15, &p.status[0], TEXT);
        cv.text(&f.s, l.head.x + 2, l.head.y + 33, &p.status[1], MODELLED);
    }

    /// The eye: frame, axes in volts and UI, the coloured hits and the two levels.
    fn eye_plot(&self, cv: &mut Canvas, l: &Layout) {
        let f = &self.fonts;
        let p = &self.azr.page;
        let r = l.plot;
        cv.text(&f.sb, l.eye.x + 6, l.eye.y + 14, if p.title.is_empty() { "Eye" } else { &p.title }, TEXT);
        // the density scale
        let (kx, ky) = (r.x + r.w - 128, l.eye.y + 5);
        let pal = heat_palette();
        cv.text(&f.s, kx - 28, ky + 10, "rare", DIM);
        for i in 0..64 {
            cv.fill(Rect::new(kx + i, ky, 1, 10), pal[(i * 4) as usize]);
        }
        cv.text(&f.s, kx + 68, ky + 10, "dense", DIM);
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
            if p.img_w == r.w as usize {
                for (y, row) in p.img.chunks_exact(p.img_w).take(r.h as usize).enumerate() {
                    let o = ((r.y + y as i32) * cv.w + r.x) as usize;
                    for (d, &c) in cv.px[o..o + row.len()].iter_mut().zip(row) {
                        if c != 0 {
                            *d = c;
                        }
                    }
                }
            }
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

/// Room for the label of a figure.
pub(super) const LABEL_W: i32 = 64;

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
    fn the_page_tiles_the_area_to_its_bottom_edge() {
        for h in [160, 240, 296, 300, 352, 400, 440, 480] {
            let a = Rect::new(0, 40, W, h);
            let l = layout(a);
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
        for h in [160, 352, 480] {
            let a = Rect::new(0, 40, W, h);
            let l = layout(a);
            let btn = Rect::new(a.x + a.w - 48, a.y, 48, 42);
            for r in [l.eye, l.head, l.fig, l.jit].iter().chain(&l.charts) {
                assert!(!overlaps(*r, btn), "{:?}", r);
            }
        }
    }

    /// Every title and figure inside its box, with the longest values the page can show.
    #[test]
    fn the_text_fits_its_boxes() {
        for s in [CLEAN, NOISY, Sig { uis: 6, ..CLEAN }] {
            let ui = page(s, 8);
            let (f, p) = (&ui.fonts, &ui.azr.page);
            let l = layout(ui.area);
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

    /// The colours come from the names in ui.rs, so a new look reaches this page.
    #[test]
    fn no_hard_coded_colours() {
        let src = include_str!("eyepage.rs");
        let call = ["rgb", "("].concat();
        assert!(!src.contains(&call));
    }

    /// Pictures for a visual check: `OOS_SHOTS=<dir> cargo test eye_page_shots -- --ignored`.
    #[test]
    #[ignore]
    fn eye_page_shots() {
        let dir = std::env::var("OOS_SHOTS").unwrap_or_else(|_| ".".into());
        let few = Sig { uis: 10, ..CLEAN };
        for (name, s, n) in [("eye-clean", CLEAN, 40), ("eye-noisy", NOISY, 60), ("eye-few", few, 6), ("eye-tiny", Sig { uis: 6, ..CLEAN }, 3)] {
            let mut ui = page(s, n);
            let mut cv = Canvas::new(W, H);
            ui.render(&mut cv);
            std::fs::write(format!("{}/{}.png", dir, name), crate::store::png_of(&cv)).unwrap();
            ui.m.run = RunState::Stop;
        }
    }
}
