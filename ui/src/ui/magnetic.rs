// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Magnetic page: inductance, B / H / μr, cycle loss, and the I vs ∫V loop (`oos_dsp::magnetic`).

use super::*;
use oos_dsp::magnetic::{self, Geom};

impl Ui {
    pub(super) fn magnetic_items(&self) -> Vec<Item> {
        use Item::*;
        let mut v = vec![MagOn, MagV, MagI, MagN, MagAe, MagLe, MagRcu, MagPlot];
        if self.azr.mag.as_ref().is_some_and(|m| m.loops.iter().any(|l| !l.is_empty())) {
            v.push(MagCur);
        }
        v
    }

    pub(super) fn magnetic_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::MagN | Item::MagAe | Item::MagLe | Item::MagRcu | Item::MagCur => Kind::Value,
            Item::MagOn | Item::MagV | Item::MagI | Item::MagPlot => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn magnetic_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::MagOn => "View",
            Item::MagV => "Voltage",
            Item::MagI => "Current",
            Item::MagN => "Turns",
            Item::MagAe => "Area",
            Item::MagLe => "Length",
            Item::MagRcu => "Rcu",
            Item::MagPlot => "Plot",
            Item::MagCur => "Cursor",
            _ => return None,
        })
    }

    pub(super) fn magnetic_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".into() } else { "Off".into() };
        let geom = |x: f64, unit: &str| if x > 0.0 { si(x, unit) } else { "off".into() };
        Some(match it {
            Item::MagOn => onoff(m.view == AView::Magnetic),
            Item::MagV => format!("CH{}", m.mag_v + 1),
            Item::MagI => format!("CH{}", m.mag_i + 1),
            Item::MagN => format!("{:.0}", m.mag_n),
            Item::MagAe => geom(m.mag_ae, "m\u{b2}"),
            Item::MagLe => geom(m.mag_le, "m"),
            Item::MagRcu => if m.mag_rcu > 0.0 { si(m.mag_rcu, "\u{3a9}") } else { "0 \u{3a9}".into() },
            Item::MagPlot => {
                if m.mag_plot == MagPlot::BH && m.mag_ae > 0.0 && m.mag_le > 0.0 {
                    "B-H".into()
                } else {
                    "I-\u{3c8}".into()
                }
            }
            Item::MagCur => format!("{}", m.mag_cur),
            _ => return None,
        })
    }

    pub(super) fn magnetic_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |o: &[&str]| Some(o.iter().map(|s| s.to_string()).collect());
        match it {
            Item::MagOn => v(&["Off", "On"]),
            Item::MagV | Item::MagI => Some(ch_names()),
            Item::MagPlot if self.m.mag_ae > 0.0 && self.m.mag_le > 0.0 => v(&["I-\u{3c8}", "B-H"]),
            Item::MagPlot => v(&["I-\u{3c8}"]),
            _ => None,
        }
    }

    pub(super) fn magnetic_adjust(&mut self, it: Item, d: i32) -> bool {
        let m = &mut self.m;
        match it {
            Item::MagOn => m.view = if m.view == AView::Magnetic { AView::Scope } else { AView::Magnetic },
            Item::MagV => m.mag_v = src_step(m.mag_v, NCH, d),
            Item::MagI => m.mag_i = src_step(m.mag_i, NCH, d),
            Item::MagN => m.mag_n = (m.mag_n + d as f64).clamp(1.0, 10_000.0),
            Item::MagAe => m.mag_ae = eng_ladder(m.mag_ae, d, 1e-6, 0.1, true),
            Item::MagLe => m.mag_le = eng_ladder(m.mag_le, d, 1e-3, 10.0, true),
            Item::MagRcu => m.mag_rcu = eng_ladder(m.mag_rcu, d, 1e-3, 1e3, true).max(0.0),
            Item::MagPlot => {
                if m.mag_ae > 0.0 && m.mag_le > 0.0 {
                    m.mag_plot = if m.mag_plot == MagPlot::IPsi { MagPlot::BH } else { MagPlot::IPsi };
                } else {
                    m.mag_plot = MagPlot::IPsi;
                }
            }
            Item::MagCur => {
                let n = self.azr.mag.as_ref().and_then(|g| g.loops.last()).map(|l| l.len()).unwrap_or(0);
                m.mag_cur = if n == 0 { 0 } else { (m.mag_cur as i32 + d).clamp(0, n as i32 - 1) as usize };
            }
            _ => return false,
        }
        self.analyzer_update();
        true
    }

    pub(super) fn magnetic_update(&mut self, rec: &Record) {
        self.azr.mag = None;
        self.azr.mag_note.clear();
        let (cv, ci) = (self.m.mag_v, self.m.mag_i);
        if cv == ci {
            self.azr.mag_note = format!("CH{} is both voltage and current", cv + 1);
            return;
        }
        let Some((v, _, dt)) = ana_samples(self, rec, cv) else {
            self.azr.mag_note = format!("CH{} is off", cv + 1);
            return;
        };
        let Some((i, _, _)) = ana_samples(self, rec, ci) else {
            self.azr.mag_note = format!("CH{} is off", ci + 1);
            return;
        };
        let geom = Geom {
            turns: self.m.mag_n.max(1.0),
            area: (self.m.mag_ae > 0.0).then_some(self.m.mag_ae),
            length: (self.m.mag_le > 0.0).then_some(self.m.mag_le),
            rcu: self.m.mag_rcu.max(0.0),
        };
        let mut note = String::new();
        if geom.area.is_some() && geom.length.is_some() && self.m.ch[ci].coupling == Coupling::Ac {
            note = format!("CH{} is AC coupled", ci + 1);
        }
        let got = magnetic::measure(&v, &i, dt, geom);
        if !got.note.is_empty() {
            if !note.is_empty() {
                note.push_str(". ");
            }
            note.push_str(got.note);
        }
        self.azr.mag_note = note;
        let n = got.loops.last().map(|l| l.len()).unwrap_or(0);
        if n > 0 {
            self.m.mag_cur = self.m.mag_cur.min(n - 1);
        }
        self.azr.mag = Some(got);
    }

    pub(super) fn draw_magnetic(&self, cv: &mut Canvas) {
        let a = self.area;
        let m = &self.m;
        let bh = m.mag_plot == MagPlot::BH && m.mag_ae > 0.0 && m.mag_le > 0.0;
        let (n, ae, le) = (m.mag_n.max(1.0), m.mag_ae, m.mag_le);
        let map = |psi: f64, i: f64| if bh { (psi / (n * ae), n * i / le) } else { (psi, i) };
        let g = self.azr.mag.as_ref();
        let loops = g.map(|g| &g.loops[..]).unwrap_or(&[]);
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for lp in loops {
            for &(psi, i) in lp {
                let (x, y) = map(psi, i);
                if x.is_finite() && y.is_finite() {
                    xs.push(x);
                    ys.push(y);
                }
            }
        }
        cv.set_clip(a);
        if !xs.is_empty() {
            let (x0, x1) = pad(xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            let (y0, y1) = pad(ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            let px = |x: f64| a.x + ((x - x0) / (x1 - x0) * (a.w - 16) as f64) as i32 + 8;
            let py = |y: f64| a.y + a.h - 16 - ((y - y0) / (y1 - y0) * (a.h - 28) as f64) as i32;
            let z = CH_COLORS[m.mag_i.min(NCH - 1)];
            for (k, lp) in loops.iter().enumerate() {
                let col = if k + 1 == loops.len() { z } else { DIM };
                let pts: Vec<(i32, i32)> = lp.iter().filter_map(|&(psi, i)| {
                    let (x, y) = map(psi, i);
                    (x.is_finite() && y.is_finite()).then_some((px(x), py(y)))
                }).collect();
                for w in pts.windows(2) {
                    cv.line(w[0].0, w[0].1, w[1].0, w[1].1, col);
                }
            }
            let f = &self.fonts;
            let (xl, yl) = if bh { ("B", "H") } else { ("\u{3c8}", "i") };
            cv.text(&f.s, a.x + 8, a.y + a.h - 4, xl, DIM);
            cv.text(&f.s, a.x + 8, a.y + 16, yl, DIM);
            if let Some(lp) = loops.last().filter(|l| !l.is_empty()) {
                let k = m.mag_cur.min(lp.len() - 1);
                let (psi, i) = lp[k];
                let (x, y) = map(psi, i);
                if x.is_finite() && y.is_finite() {
                    let (cx, cy) = (px(x), py(y));
                    cv.line(cx - 6, cy, cx + 6, cy, ACCENT);
                    cv.line(cx, cy - 6, cx, cy + 6, ACCENT);
                    let slope = if k > 0 {
                        let (p0, i0) = lp[k - 1];
                        let di = i - i0;
                        if di.abs() > 1e-12 { (psi - p0) / di } else { f64::NAN }
                    } else {
                        f64::NAN
                    };
                    let (xu, yu) = if bh { ("T", "A/m") } else { ("V\u{b7}s", "A") };
                    let t = format!("{}  {}  \u{394}{}/{} {}", num(Some(x), xu), num(Some(y), yu), xl, yl, num(slope.is_finite().then_some(slope), if bh { "" } else { "H" }));
                    cv.text(&f.s, (cx + 8).min(a.x + a.w - 220), (cy - 4).max(a.y + 14), &t, ACCENT);
                }
            }
        }
        cv.no_clip();
        let mut lines = Vec::new();
        let vu = self.m.ch[m.mag_v].unit.name();
        let iu = self.m.ch[m.mag_i].unit.name();
        lines.push(format!("CH{} ({})   CH{} ({})", m.mag_v + 1, vu, m.mag_i + 1, iu));
        if let Some(g) = g {
            lines.push(format!("L {}{}", num(g.l, "H"), if g.saturated { "  saturated" } else { "" }));
            if g.l.is_some() {
                lines.push(format!("fit {:.3} .. {:.3} A", g.i_lo, g.i_hi));
            }
            if let Some(ls) = g.l_sine {
                lines.push(format!("sine check {}", si(ls, "H")));
            }
            lines.push(format!("Bpk {}   Hpk {}", num(g.b_peak, "T"), num(g.h_peak, "A/m")));
            lines.push(format!("\u{3bc}r {}", num(g.mu_r, "")));
            let loss = if g.copper > 0.0 && g.loss.is_some() {
                format!("loss {}   copper {}", num(g.loss, "W"), si(g.copper, "W"))
            } else {
                format!("loss {}", num(g.loss, "W"))
            };
            lines.push(loss);
            lines.push(format!("{} cycles, {} kept", g.cycles, g.kept));
        }
        if !self.azr.mag_note.is_empty() {
            lines.push(self.azr.mag_note.clone());
        }
        if lines.len() > 1 || !self.azr.mag_note.is_empty() {
            self.panel(cv, a.x + 8, a.y + 8, &lines, TEXT);
        }
    }
}

/// Voltage or current of one channel, with the time of the first sample and the step.
pub(super) fn ana_samples(ui: &Ui, rec: &Record, c: usize) -> Option<(Vec<f32>, f64, f64)> {
    if c >= n_ch() || rec.ch[c].is_empty() {
        return None;
    }
    if let Some(r) = rec.raw.as_ref().filter(|_| ui.m.acq != AcqMode::HiRes) {
        let v = &r.frame.ch[c];
        if r.i1 <= v.len() && r.i1 > r.i0 && r.i1 - r.i0 > rec.ch[c].len() {
            return Some((v[r.i0..r.i1].to_vec(), r.t0, r.frame.dt));
        }
    }
    Some((rec.ch[c].clone(), rec.t0, rec.dt))
}

pub(super) fn ana_clash(chs: &[usize]) -> Option<usize> {
    let mut seen = [false; NCH];
    for &c in chs {
        if c >= NCH || seen[c] {
            return Some(c.min(NCH - 1));
        }
        seen[c] = true;
    }
    None
}

pub(super) fn num(x: Option<f64>, unit: &str) -> String {
    match x {
        Some(v) if v.is_finite() && unit.is_empty() => {
            if v.abs() >= 100.0 {
                format!("{:.1}", v)
            } else if v.abs() >= 10.0 {
                format!("{:.2}", v)
            } else {
                format!("{:.3}", v)
            }
        }
        Some(v) if v.is_finite() => si(v, unit),
        _ => "----".into(),
    }
}

fn pad(lo: f64, hi: f64) -> (f64, f64) {
    if !lo.is_finite() || !hi.is_finite() || hi - lo < 1e-15 {
        let c = if lo.is_finite() { lo } else { 0.0 };
        return (c - 1.0, c + 1.0);
    }
    let p = (hi - lo) * 0.08;
    (lo - p, hi + p)
}
