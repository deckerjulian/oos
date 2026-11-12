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
            Item::MagV => self.wave_name(m.mag_v),
            Item::MagI => self.wave_name(m.mag_i),
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
            Item::MagV | Item::MagI => Some(self.wave_names()),
            Item::MagPlot if self.m.mag_ae > 0.0 && self.m.mag_le > 0.0 => v(&["I-\u{3c8}", "B-H"]),
            Item::MagPlot => v(&["I-\u{3c8}"]),
            _ => None,
        }
    }

    pub(super) fn magnetic_adjust(&mut self, it: Item, d: i32) -> bool {
        if matches!(it, Item::MagV | Item::MagI) {
            let cur = if it == Item::MagV { self.m.mag_v } else { self.m.mag_i };
            let next = self.step_wave(cur, d);
            if it == Item::MagV { self.m.mag_v = next; } else { self.m.mag_i = next; }
            self.analyzer_update();
            return true;
        }
        let m = &mut self.m;
        match it {
            Item::MagOn => m.view = if m.view == AView::Magnetic { AView::Scope } else { AView::Magnetic },
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
            self.azr.mag_note = format!("{} is both voltage and current", self.wave_name(cv));
            return;
        }
        let taken = match self.take_waves(rec, &[cv, ci], &[]) {
            Ok(t) => t,
            Err(id) => {
                self.azr.mag_note = missing_wave(id);
                return;
            }
        };
        let (v, i, dt) = (&taken.v[0], &taken.v[1], taken.dt);
        let geom = Geom {
            turns: self.m.mag_n.max(1.0),
            area: (self.m.mag_ae > 0.0).then_some(self.m.mag_ae),
            length: (self.m.mag_le > 0.0).then_some(self.m.mag_le),
            rcu: self.m.mag_rcu.max(0.0),
        };
        let mut note = String::new();
        if geom.area.is_some() && geom.length.is_some() {
            if let WaveId::Ch(c) = ci {
                if self.m.ch[c].coupling == Coupling::Ac {
                    note = format!("CH{} is AC coupled", c + 1);
                }
            }
        }
        if taken.moved {
            if !note.is_empty() {
                note.push_str(". ");
            }
            note.push_str("on the finest ref interval");
        }
        let got = magnetic::measure(v, i, dt, geom);
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
            let z = self.wave_color(m.mag_i);
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
        lines.push(format!("{}   {}", self.wave_caption(m.mag_v), self.wave_caption(m.mag_i)));
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

/// Samples one named wave. A channel is `meas_wave` (the hardware frame when it is longer than the screen).
/// A ref is the stored volts at the slot's own `dt`: no coupling, no bandwidth limit, no licence clip.
/// An empty slot, and a channel with no samples, return nothing. Hiding the trace does not.
pub(super) struct WaveSamp {
    pub v: Vec<f32>,
    pub t0: f64,
    pub dt: f64,
}

/// Waves placed on one sample grid. `moved` means a coarser wave was interpolated onto a finer ref.
pub(super) struct Taken {
    pub t0: f64,
    pub dt: f64,
    pub v: Vec<Vec<f32>>,
    pub opt: Vec<Option<Vec<f32>>>,
    pub moved: bool,
}

impl Ui {
    /// Channels, then each loaded ref. An empty slot is not offered.
    pub(super) fn wave_ids(&self) -> Vec<WaveId> {
        let mut v: Vec<WaveId> = (0..n_ch()).map(WaveId::Ch).collect();
        for i in 0..NREF {
            if self.refs[i].as_ref().is_some_and(|r| r.v.len() >= 2 && r.dt > 0.0) {
                v.push(WaveId::Ref(i));
            }
        }
        v
    }

    pub(super) fn wave_names(&self) -> Vec<String> {
        self.wave_ids().iter().map(|&id| self.wave_name(id)).collect()
    }

    /// "Off" and then `wave_names`, for a source that may be unused.
    pub(super) fn wave_names_opt(&self) -> Vec<String> {
        let mut v = vec!["Off".into()];
        v.extend(self.wave_names());
        v
    }

    pub(super) fn wave_name(&self, id: WaveId) -> String {
        match id {
            WaveId::Ch(c) => format!("CH{}", c + 1),
            WaveId::Ref(i) => format!("R{}", i + 1),
        }
    }

    /// Channel unit in parentheses. A ref is the stored numbers, with no front-end unit.
    pub(super) fn wave_caption(&self, id: WaveId) -> String {
        match id {
            WaveId::Ch(c) => format!("CH{} ({})", c + 1, self.m.ch[c.min(NCH - 1)].unit.name()),
            WaveId::Ref(i) => format!("R{}", i + 1),
        }
    }

    pub(super) fn wave_color(&self, id: WaveId) -> Rgb {
        match id {
            WaveId::Ch(c) => CH_COLORS[c.min(NCH - 1)],
            WaveId::Ref(i) => {
                let k = self.refs.get(i).and_then(|r| r.as_ref()).map(|r| r.color).unwrap_or(0);
                REF_COLORS[k % REF_COLORS.len()].1
            }
        }
    }

    pub(super) fn step_wave(&self, cur: WaveId, d: i32) -> WaveId {
        let v = self.wave_ids();
        if v.is_empty() {
            return cur;
        }
        let i = v.iter().position(|&x| x == cur).unwrap_or(0) as i32;
        v[(i + d).rem_euclid(v.len() as i32) as usize]
    }

    /// Steps Off, then the channels, then the loaded refs.
    pub(super) fn step_wave_opt(&self, cur: Option<WaveId>, d: i32) -> Option<WaveId> {
        let v = self.wave_ids();
        let n = v.len() as i32 + 1;
        let i = match cur {
            None => 0,
            Some(id) => v.iter().position(|&x| x == id).map(|k| k as i32 + 1).unwrap_or(0),
        };
        let j = (i + d).rem_euclid(n);
        if j == 0 { None } else { Some(v[(j - 1) as usize]) }
    }

    pub(super) fn wave_samples(&self, rec: &Record, id: WaveId) -> Option<WaveSamp> {
        match id {
            WaveId::Ch(c) => {
                if c >= n_ch() || rec.ch[c].is_empty() {
                    return None;
                }
                let (v, dt) = self.meas_wave(rec, c);
                let t0 = if v.len() > rec.ch[c].len() { rec.raw.as_ref().map(|r| r.t0).unwrap_or(rec.t0) } else { rec.t0 };
                (v.len() >= 2 && dt > 0.0).then_some(WaveSamp { v, t0, dt })
            }
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                (r.v.len() >= 2 && r.dt > 0.0).then_some(WaveSamp { v: r.v.clone(), t0: r.t0, dt: r.dt })
            }
        }
    }

    /// `required` must all be present. Each `optional` entry is `None` when that source is off or empty.
    /// The grid is the finest loaded ref among the waves that were read, and that ref is not resampled.
    /// With no ref, the first required wave's own interval is the grid. Channels stay on the hardware path
    /// until they are placed on a finer ref.
    pub(super) fn take_waves(&self, rec: &Record, required: &[WaveId], optional: &[Option<WaveId>]) -> Result<Taken, WaveId> {
        let mut req = Vec::with_capacity(required.len());
        for &id in required {
            req.push((id, self.wave_samples(rec, id).ok_or(id)?));
        }
        let mut opt_s = Vec::with_capacity(optional.len());
        for id in optional {
            opt_s.push(id.and_then(|id| self.wave_samples(rec, id).map(|s| (id, s))));
        }
        if req.is_empty() && opt_s.iter().all(|s| s.is_none()) {
            return Ok(Taken { t0: 0.0, dt: 0.0, v: Vec::new(), opt: vec![None; optional.len()], moved: false });
        }
        let pool: Vec<(WaveId, &WaveSamp)> = req.iter().map(|(id, s)| (*id, s)).chain(opt_s.iter().flatten().map(|(id, s)| (*id, s))).collect();
        let refs: Vec<(WaveId, &WaveSamp)> = pool.iter().copied().filter(|(id, _)| matches!(id, WaveId::Ref(_))).collect();
        let grid = if refs.is_empty() {
            pool[0]
        } else {
            let best = refs.iter().map(|(_, s)| s.dt).min_by(|a, b| a.total_cmp(b)).unwrap_or(0.0);
            *refs.iter().filter(|(_, s)| (s.dt - best).abs() <= best.abs() * 1e-9).max_by_key(|(_, s)| s.v.len()).unwrap()
        };
        let (t0, dt, n) = (grid.1.t0, grid.1.dt, grid.1.v.len());
        let mut moved = false;
        let mut v = Vec::with_capacity(req.len());
        for (_, s) in &req {
            let (col, m) = on_grid(s, t0, dt, n);
            moved |= m;
            v.push(col);
        }
        let mut opt = Vec::with_capacity(opt_s.len());
        for s in &opt_s {
            match s {
                Some((_, w)) => {
                    let (col, m) = on_grid(w, t0, dt, n);
                    moved |= m;
                    opt.push(Some(col));
                }
                None => opt.push(None),
            }
        }
        Ok(Taken { t0, dt, v, opt, moved })
    }
}

fn on_grid(src: &WaveSamp, t0: f64, dt: f64, n: usize) -> (Vec<f32>, bool) {
    let same = (src.dt - dt).abs() <= dt.abs() * 1e-9 && (src.t0 - t0).abs() <= dt.abs() * 1e-6;
    if same {
        let mut v = src.v.clone();
        v.truncate(n);
        if v.len() < n {
            v.resize(n, f32::NAN);
        }
        return (v, false);
    }
    let m = src.v.len();
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let x = (t0 + k as f64 * dt - src.t0) / src.dt;
        if m == 0 || !(src.dt > 0.0) || x < 0.0 || x > (m - 1) as f64 {
            out.push(f32::NAN);
            continue;
        }
        let i = x as usize;
        let f = (x - i as f64) as f32;
        let a = src.v[i];
        let b = src.v[(i + 1).min(m - 1)];
        if a.is_finite() && b.is_finite() {
            out.push(a * (1.0 - f) + b * f);
        } else if a.is_finite() {
            out.push(a);
        } else {
            out.push(b);
        }
    }
    (out, true)
}

pub(super) fn missing_wave(id: WaveId) -> String {
    match id {
        WaveId::Ch(c) => format!("CH{} is off", c + 1),
        WaveId::Ref(i) => format!("R{} is empty", i + 1),
    }
}

pub(super) fn clash_wave(id: WaveId) -> String {
    match id {
        WaveId::Ch(c) => format!("CH{} is used twice", c + 1),
        WaveId::Ref(i) => format!("R{} is used twice", i + 1),
    }
}

pub(super) fn wave_clash(ids: &[WaveId]) -> Option<WaveId> {
    let mut seen = Vec::new();
    for &id in ids {
        if seen.contains(&id) {
            return Some(id);
        }
        seen.push(id);
    }
    None
}

/// Three live channels on a model that does not have them. A ref is not one of those channels.
pub(super) fn needs_more_channels(ids: &[WaveId]) -> bool {
    ids.len() > n_ch() && ids.iter().all(|id| id.ch().is_some())
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
