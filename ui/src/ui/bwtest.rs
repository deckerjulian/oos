// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Bandwidth test page (Analysers > Bandwidth, docs/bandwidth-test.md): the front end's step response from a fast
//! square wave (pulse generator) on one channel. The record is folded over the signal period (equivalent time, also
//! above Nyquist with a frequency hint), then `oos_dsp::bandwidth::step_response` gives the 10-90 % rise time, the
//! overshoot and |H(f)| with its -3 dB point. Left: the edge, right: the magnitude response. Updated twice a second.

use super::*;
use oos_dsp::bandwidth;

/// Frequency hints: 0 = Auto (the signal is below Nyquist, its tone is taken as it is).
const HINTS: [f64; 16] = [0.0, 1e6, 2e6, 5e6, 10e6, 20e6, 50e6, 100e6, 150e6, 200e6, 250e6, 300e6, 400e6, 500e6, 750e6, 1e9];
/// Bins per period of the folded picture.
const BINS: [usize; 5] = [250, 500, 1000, 2000, 5000];

impl Ui {
    pub(super) fn bw_items(&self) -> Vec<Item> {
        vec![Item::BwOn, Item::BwSrc, Item::BwHint, Item::BwBins]
    }

    pub(super) fn bw_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::BwOn | Item::BwSrc | Item::BwHint | Item::BwBins => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn bw_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::BwTest) => "Bandwidth",
            Item::BwOn => "View",
            Item::BwSrc => "Source",
            Item::BwHint => "Signal freq",
            Item::BwBins => "Bins / period",
            _ => return None,
        })
    }

    fn hint_name(h: f64) -> String {
        if h == 0.0 {
            "Auto".into()
        } else {
            si_short(h, "Hz")
        }
    }

    pub(super) fn bw_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::BwOn => if m.view == AView::Step { "On".into() } else { "Off".into() },
            Item::BwSrc => format!("CH{}", m.bw_src + 1),
            Item::BwHint => Self::hint_name(m.bw_hint),
            Item::BwBins => m.bw_bins.to_string(),
            _ => return None,
        })
    }

    pub(super) fn bw_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::BwOn => vec!["Off".into(), "On".into()],
            Item::BwSrc => ch_names(),
            Item::BwHint => HINTS.iter().map(|&h| Self::hint_name(h)).collect(),
            Item::BwBins => BINS.iter().map(|b| b.to_string()).collect(),
            _ => return None,
        })
    }

    pub(super) fn bw_adjust(&mut self, it: Item, d: i32) -> bool {
        let m = &mut self.m;
        let step = |i: usize, n: usize| (i as i32 + d).clamp(0, n as i32 - 1) as usize;
        match it {
            Item::BwOn => m.view = if m.view == AView::Step { AView::Scope } else { AView::Step },
            Item::BwSrc => m.bw_src = src_step(m.bw_src, NCH, d),
            Item::BwHint => {
                let i = HINTS.iter().position(|&h| h >= m.bw_hint * 0.999).unwrap_or(0);
                m.bw_hint = HINTS[step(i, HINTS.len())];
            }
            Item::BwBins => {
                let i = BINS.iter().position(|&b| b >= m.bw_bins).unwrap_or(BINS.len() - 1);
                m.bw_bins = BINS[step(i, BINS.len())];
            }
            _ => return false,
        }
        self.azr.bw = None;
        true
    }

    /// Per record (analyzer_update), at most twice a second: fold the whole record of the source, step response.
    pub(super) fn bw_update(&mut self, rec: &Record) {
        if self.now - self.azr.bw_t < 0.5 && self.azr.bw.is_some() {
            return;
        }
        self.azr.bw_t = self.now;
        let c = self.m.bw_src.min(NCH - 1);
        // the whole hardware frame when there is one (the longer, the finer the folding), else the record
        let (v, dt) = match &rec.raw {
            Some(r) if !r.frame.ch[c].is_empty() && r.frame.dt > 0.0 => (r.frame.ch[c].clone(), r.frame.dt),
            _ => self.meas_wave(rec, c),
        };
        if v.len() < 1000 {
            self.azr.bw = None;
            self.azr.bw_info = format!("CH{}: record too short ({} points)", c + 1, v.len());
            return;
        }
        let fs = 1.0 / dt;
        let res = bandwidth::tone(&v, dt).and_then(|fa| {
            let f = if self.m.bw_hint > 0.0 { bandwidth::unalias(fa, fs, self.m.bw_hint) } else { fa };
            let (w, step) = bandwidth::fold(&v, dt, 1.0 / f, self.m.bw_bins)?;
            Some((f, step, bandwidth::step_response(&w, step)?))
        });
        match res {
            Some((f, step, r)) => {
                self.azr.bw_info = format!("{} at {}, {} points, picture step {}", si(f, "Hz"), si_short(fs, "Sa/s"), fmt_count(v.len() as f64), si(step, "s"));
                self.azr.bw = Some(r);
            }
            None => {
                self.azr.bw = None;
                self.azr.bw_info = format!("CH{}: no periodic edge found (square wave from a pulse generator?)", c + 1);
            }
        }
    }

    pub(super) fn draw_bw(&self, cv: &mut Canvas) {
        let a = self.area;
        let f = self.fonts();
        let col = CH_COLORS[self.m.bw_src.min(NCH - 1)];
        let Some(r) = &self.azr.bw else {
            let msg = if self.azr.bw_info.is_empty() { "waiting for a record".to_string() } else { self.azr.bw_info.clone() };
            cv.text_in(&f.m, a, 1, &msg, DIM);
            return;
        };
        let half = a.w / 2;
        // ---- left: the edge, 10 / 50 / 90 % lines
        let (l, t, b) = (a.x + 8, a.y + 30, a.y + a.h - 28);
        let w = half - 16;
        let (lo, hi) = r.levels;
        let span = (hi - lo).max(1e-12);
        let (vmin, vmax) = (lo - 0.15 * span, hi + 0.15 * span);
        let y_of = |v: f64| b - (((v - vmin) / (vmax - vmin)) * (b - t) as f64).round() as i32;
        for (p, name) in [(0.1, "10 %"), (0.5, "50 %"), (0.9, "90 %")] {
            let y = y_of(lo + p * span);
            cv.hline(l, l + w, y, LINE);
            cv.text(&f.s, l + 2, y - 2, name, DIM);
        }
        // around the edge, the edge at a third of the width (the plateaus far away carry no information; the
        // result panel sits lower right)
        let n = r.edge.len();
        let (i0, i1) = (r.edge_at.saturating_sub(n / 16), (r.edge_at + n / 8).min(n));
        let mut prev: Option<(i32, i32)> = None;
        for i in i0..i1 {
            let x = l + ((i - i0) as f64 / (i1 - i0).max(1) as f64 * w as f64) as i32;
            let y = y_of(r.edge[i] as f64);
            if let Some((px, py)) = prev {
                cv.line(px, py, x, y, col);
            }
            prev = Some((x, y));
        }
        cv.text(&f.s, l, a.y + 16, "Step (equivalent time)", col);
        // ---- right: |H(f)| in dB on a log axis, 0 .. -20 dB
        let (rx, rw) = (a.x + half + 8, half - 16);
        let fmax = r.mag.last().map_or(1e9, |p| p.0);
        let fmin = (fmax / 1000.0).max(r.mag.get(1).map_or(1e6, |p| p.0));
        let x_of = |hz: f64| rx + (((hz.ln() - fmin.ln()) / (fmax.ln() - fmin.ln())) * rw as f64).round() as i32;
        let db_y = |db: f64| t + ((-db).clamp(0.0, 20.0) / 20.0 * (b - t) as f64).round() as i32;
        for db in [0.0, -3.0, -6.0, -10.0, -20.0] {
            let y = db_y(db);
            cv.hline(rx, rx + rw, y, if db == -3.0 { mix(BG, ACCENT, 120) } else { LINE });
            cv.text(&f.s, rx + rw - 40, y - 2, &format!("{:.0} dB", db), DIM);
        }
        let mut d = 10f64.powf(fmin.log10().ceil());
        while d <= fmax {
            let x = x_of(d);
            cv.vline(x, t, b, LINE);
            cv.text(&f.s, x + 2, b + 14, &si_short(d, "Hz"), DIM);
            d *= 10.0;
        }
        let mut prev: Option<(i32, i32)> = None;
        for &(hz, db) in r.mag.iter().filter(|p| p.0 >= fmin) {
            let (x, y) = (x_of(hz), db_y(db));
            if let Some((px, py)) = prev {
                cv.line(px, py, x, y, col);
            }
            prev = Some((x, y));
        }
        if let Some(f3) = r.f3db {
            let x = x_of(f3);
            cv.vline(x, t, b, ACCENT);
        }
        cv.text(&f.s, rx, a.y + 16, "Magnitude |H(f)| from the step", col);
        // ---- results
        let lines = [
            format!("-3 dB: {}", r.f3db.map_or("beyond the picture".to_string(), |x| si(x, "Hz"))),
            format!("Rise 10-90 %: {}  (0.35 / rise = {})", si(r.rise, "s"), si(r.bw_from_rise, "Hz")),
            format!("Overshoot: {:.1} %", r.overshoot),
            self.azr.bw_info.clone(),
        ];
        let pw = lines.iter().map(|s| f.s.width(s)).max().unwrap_or(0) + 16;
        // lower right of the edge plot: below the high plateau, right of the edge (empty there)
        let ph = lines.len() as i32 * 18 + 8;
        let pr = Rect::new((a.x + half - pw - 8).max(a.x + 8), b - ph - 4, pw, ph);
        cv.round(pr, 5, BAR, 230);
        for (i, s) in lines.iter().enumerate() {
            cv.text(&f.s, pr.x + 8, pr.y + 18 + i as i32 * 18, s, if i == 0 { ACCENT } else { TEXT });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_square_gives_a_step_response() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.activate(Item::Open(MenuId::BwTest));
        ui.adjust(Item::BwOn, 1);
        assert_eq!(ui.m.view, AView::Step);
        for _ in 0..40 {
            ui.step(0.05);
        }
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        // the demo CH1 is a 1 kHz square: an edge is found, the rise time is positive and finite
        let r = ui.azr.bw.as_ref().unwrap_or_else(|| panic!("no result: {}", ui.azr.bw_info));
        assert!(r.rise > 0.0 && r.rise.is_finite(), "{:?}", r.rise);
        assert!(ui.azr.bw_info.contains("Hz"), "{}", ui.azr.bw_info);
        // option lists and stepping stay in range
        ui.adjust(Item::BwHint, 3);
        assert_eq!(ui.m.bw_hint, 5e6);
        ui.adjust(Item::BwBins, 10);
        assert_eq!(ui.m.bw_bins, 5000);
        assert_eq!(ui.options(Item::BwSrc).len(), n_ch());
    }
}
