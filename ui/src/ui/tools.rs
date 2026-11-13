// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurement rows (single channel and channel pairs), the DVM and the counter, tracking cursors, saved masks and channel
//! labels. All of them work on the records the screen shows.

use super::*;
use oos_dsp::measure::{burst_width, delay, duration_n, hold_time, phase, setup_time, skew, time_outside, Analysis, Pair};

/// Edge times are seconds from the first sample. Add the record's t0 so a pair of different records lines up.
fn shift_edges(a: &Analysis, t0: f64) -> Analysis {
    if t0 == 0.0 {
        return a.clone();
    }
    let mut b = a.clone();
    for t in &mut b.rises {
        *t += t0;
    }
    for t in &mut b.falls {
        *t += t0;
    }
    b.t_vmax += t0;
    b.t_vmin += t0;
    b
}

/// Measurements at once.
pub(super) const MEAS_MAX: usize = 14;
/// Rows per channel in the strip under the plot.
pub(super) const STRIP_PER_ROW: usize = 6;

/// What a channel-pair row measures. The edge flags choose the edges of delay, phase and skew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairKind {
    Delay,
    Phase,
    Skew,
    Setup,
    Hold,
}

/// Channel pairs: name, kind, edge of A rising, edge of B rising. New rows stay at the end so a saved index still means the same row.
pub const PAIRS: [(&str, PairKind, bool, bool); 14] = [
    ("Delay rise-rise", PairKind::Delay, true, true),
    ("Delay fall-fall", PairKind::Delay, false, false),
    ("Delay rise-fall", PairKind::Delay, true, false),
    ("Delay fall-rise", PairKind::Delay, false, true),
    ("Phase rise-rise", PairKind::Phase, true, true),
    ("Phase fall-fall", PairKind::Phase, false, false),
    ("Phase rise-fall", PairKind::Phase, true, false),
    ("Phase fall-rise", PairKind::Phase, false, true),
    ("Skew rise-rise", PairKind::Skew, true, true),
    ("Skew fall-fall", PairKind::Skew, false, false),
    ("Skew rise-fall", PairKind::Skew, true, false),
    ("Skew fall-rise", PairKind::Skew, false, true),
    ("Setup time", PairKind::Setup, true, true),
    ("Hold time", PairKind::Hold, true, true),
];

/// DVM modes and counter modes.
pub const DVM_MODES: [&str; 3] = ["DC", "AC rms", "AC+DC rms"];
pub const CNT_MODES: [&str; 3] = ["Frequency", "Period", "Totalize"];

/// One measurement as the screen shows it.
pub(super) struct MeasRow {
    pub name: String,
    /// colour and strip row: the source, or source A of a pair
    pub src: WaveId,
    pub v: f64,
    pub unit: &'static str,
}

impl MeasRow {
    pub fn text(&self) -> String {
        self.fmt(self.v)
    }

    /// A value of this measurement (the row's own, or its mean).
    pub fn fmt(&self, v: f64) -> String {
        self.fmt_digits(v, 3)
    }

    /// The same with four digits, for values that differ in the last ones (statistics, trends).
    pub fn fmt4(&self, v: f64) -> String {
        self.fmt_digits(v, 4)
    }

    fn fmt_digits(&self, v: f64, digits: usize) -> String {
        if !v.is_finite() {
            "----".into()
        } else if self.unit == "\u{b0}" {
            format!("{:.1}\u{b0}", v)
        } else {
            si_digits(v, self.unit, digits)
        }
    }
}

impl Ui {
    /// Measurement `k` of a channel or a reference. A channel that is off, or a ref with no samples, is NaN.
    pub fn meas_at(&self, id: WaveId, k: Meas) -> f64 {
        match self.meas_analysis(id) {
            Some((a, _)) => self.meas_of(&a, k),
            None => f64::NAN,
        }
    }

    /// One channel-pair row. `k` is an entry of `PAIRS`.
    pub fn pair_at(&self, k: usize, a: WaveId, b: WaveId) -> f64 {
        let (_, kind, ea, eb) = PAIRS[k.min(PAIRS.len() - 1)];
        let p = Pair(ea, eb);
        match (self.meas_analysis(a), self.meas_analysis(b)) {
            (Some((aa, ta)), Some((bb, tb))) => {
                let (aa, bb) = (shift_edges(&aa, ta), shift_edges(&bb, tb));
                match kind {
                    PairKind::Phase => phase(&aa, &bb, p),
                    PairKind::Delay => delay(&aa, &bb, p),
                    PairKind::Skew => skew(&aa, &bb, p),
                    PairKind::Setup => setup_time(&aa, &bb),
                    PairKind::Hold => hold_time(&aa, &bb),
                }
            }
            _ => f64::NAN,
        }
    }

    /// All measurements, in the order of the statistics.
    pub(super) fn meas_rows(&self) -> Vec<MeasRow> {
        let mut v: Vec<MeasRow> = self
            .m
            .meas
            .iter()
            .map(|&(k, id)| {
                let (val, unit) = match self.meas_analysis(id) {
                    Some((a, _)) => (self.meas_of(&a, k), self.meas_unit(id, k)),
                    None => (f64::NAN, self.meas_unit(id, k)),
                };
                MeasRow { name: k.name().to_string(), src: id, v: val, unit }
            })
            .collect();
        for &(k, a, b) in &self.m.meas2 {
            let (name, kind, _, _) = PAIRS[k.min(PAIRS.len() - 1)];
            let val = self.pair_at(k, a, b);
            let tag = |id: WaveId| match id {
                WaveId::Ch(c) => (c + 1).to_string(),
                WaveId::Ref(i) => format!("R{}", i + 1),
            };
            v.push(MeasRow { name: format!("{} {}-{}", name, tag(a), tag(b)), src: a, v: val, unit: if kind == PairKind::Phase { "\u{b0}" } else { "s" } });
        }
        v
    }

    /// Unit of a measurement: the channel's unit, or volts for a ref (the slot stores volts).
    fn meas_unit(&self, id: WaveId, k: Meas) -> &'static str {
        match id {
            WaveId::Ch(c) => self.m.ch[c.min(NCH - 1)].unit.of(k.unit()),
            WaveId::Ref(_) => k.unit(),
        }
    }

    /// Analysis of one source, and the time of its first sample. A channel must be on. A ref is the stored
    /// record, hidden or not. None when that wave is missing.
    pub(super) fn meas_analysis(&self, id: WaveId) -> Option<(oos_dsp::measure::Analysis, f64)> {
        match id {
            WaveId::Ch(c) => {
                if c >= n_ch() || !self.m.ch[c].on {
                    return None;
                }
                let rec = self.last[0].as_ref()?;
                let s = self.wave_samples(rec, id)?;
                Some((self.ana[c].clone(), s.t0))
            }
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                if r.v.len() < 2 || !(r.dt > 0.0) {
                    return None;
                }
                Some((self.ref_ana[i].clone(), r.t0))
            }
        }
    }

    /// A measurement with the Measure menu's idle, outside and N settings. The other items are `Analysis::get`.
    pub(super) fn meas_of(&self, a: &Analysis, k: Meas) -> f64 {
        match k {
            Meas::Burst => burst_width(a, self.m.meas_idle),
            Meas::Outside => time_outside(a, self.m.meas_out),
            Meas::NPeriods => duration_n(a, self.m.meas_n.max(1) as usize, self.m.meas_data),
            _ => a.get(k),
        }
    }

    pub(super) fn meas_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::MeasIdle | Item::MeasN => Kind::Value,
            Item::MeasOutside | Item::MeasEdges => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn meas_cfg_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::MeasIdle => "Idle",
            Item::MeasOutside => "Outside",
            Item::MeasN => "N periods",
            Item::MeasEdges => "Edges",
            _ => return None,
        })
    }

    pub(super) fn meas_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::MeasIdle => if m.meas_idle > 0.0 { si_short(m.meas_idle, "s") } else { "period".into() },
            Item::MeasOutside => format!("{:?}", m.meas_out),
            Item::MeasN => m.meas_n.to_string(),
            Item::MeasEdges => if m.meas_data { "Data".into() } else { "Clock".into() },
            _ => return None,
        })
    }

    pub(super) fn meas_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::MeasOutside => ["Above", "Below", "Both"].iter().map(|s| s.to_string()).collect(),
            Item::MeasEdges => vec!["Clock".into(), "Data".into()],
            _ => return None,
        })
    }

    pub(super) fn meas_adjust(&mut self, it: Item, d: i32) -> bool {
        let m = &mut self.m;
        match it {
            Item::MeasIdle => {
                let cur = if m.meas_idle > 0.0 { m.meas_idle } else { 0.0 };
                let l: Vec<f64> = std::iter::once(0.0).chain((0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e - 9)))).collect();
                let i = l.iter().position(|&v| v >= cur * 0.999).unwrap_or(0);
                m.meas_idle = l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize];
            }
            Item::MeasOutside => {
                let all = [oos_dsp::measure::Outside::Above, oos_dsp::measure::Outside::Below, oos_dsp::measure::Outside::Both];
                let i = all.iter().position(|&x| x == m.meas_out).unwrap_or(2);
                m.meas_out = all[(i as i32 + d).rem_euclid(all.len() as i32) as usize];
            }
            Item::MeasN => m.meas_n = (m.meas_n as i32 + d).clamp(1, 32) as u32,
            Item::MeasEdges => m.meas_data = !m.meas_data,
            _ => return false,
        }
        true
    }

    /// Room for one more: the oldest one goes when all are taken.
    pub(super) fn meas_make_room(&mut self) {
        if self.m.meas.len() + self.m.meas2.len() >= MEAS_MAX {
            if self.m.meas.is_empty() {
                self.m.meas2.remove(0);
            } else {
                self.m.meas.remove(0);
            }
            self.mstat.clear();
        }
    }

    /// Delay or phase `k` between the sources of the Measure page: on when it was off, off when it was on.
    pub(super) fn meas_pair_toggle(&mut self, k: usize) {
        let e = (k, self.meas_src, self.meas_src_b);
        if e.1 == e.2 {
            return self.toast("Source A and B are the same source");
        }
        if let Some(i) = self.m.meas2.iter().position(|x| *x == e) {
            self.m.meas2.remove(i);
        } else {
            self.meas_make_room();
            self.m.meas2.push(e);
        }
        self.mstat.clear();
    }

    /// DVM and counter: a new record came in.
    pub(super) fn counter_update(&mut self) {
        if !self.m.cnt_on {
            return;
        }
        let c = self.m.cnt_src;
        if !self.m.ch[c].on {
            return;
        }
        let a = &self.ana[c];
        self.cnt_total += a.rises.len() as u64;
        let v = a.get(Meas::Freq);
        if v.is_finite() {
            let st = &mut self.cnt_stat;
            if st.n == 0 {
                (st.min, st.max) = (v, v);
            }
            st.n += 1;
            st.sum += v;
            st.min = st.min.min(v);
            st.max = st.max.max(v);
        }
    }

    pub(super) fn counter_clear(&mut self) {
        self.cnt_total = 0;
        self.cnt_stat = MStat::default();
    }

    /// DVM reading of its source, volts.
    pub fn dvm_value(&self) -> f64 {
        let c = self.m.dvm_src;
        if !self.m.ch[c].on {
            return f64::NAN;
        }
        self.ana[c].get(match self.m.dvm_mode {
            0 => Meas::Vavg,
            1 => Meas::VrmsAc,
            _ => Meas::Vrms,
        })
    }

    /// Counter reading: frequency, period, or the totalised edge count. NaN when that mode has nothing to show.
    pub fn counter_value(&self) -> f64 {
        if !self.m.cnt_on {
            return f64::NAN;
        }
        let c = self.m.cnt_src.min(NCH - 1);
        match self.m.cnt_mode {
            2 => self.cnt_total as f64,
            1 if self.m.ch[c].on => self.ana[c].get(Meas::Period),
            0 if self.m.ch[c].on => self.ana[c].get(Meas::Freq),
            _ => f64::NAN,
        }
    }

    /// The strip row of DVM and counter: (label, value) pairs.
    pub(super) fn dvm_row(&self) -> Vec<(String, String)> {
        let mut v = Vec::new();
        if self.m.dvm_on {
            let x = self.dvm_value();
            let t = if x.is_finite() { si_digits(x, "V", 4) } else { "----".into() };
            v.push((format!("DVM CH{} {}", self.m.dvm_src + 1, DVM_MODES[self.m.dvm_mode.min(2)]), t));
        }
        if self.m.cnt_on {
            let c = self.m.cnt_src;
            let a = &self.ana[c];
            let digits = self.m.cnt_digits as usize;
            let on = self.m.ch[c].on;
            let t = match self.m.cnt_mode {
                2 => fmt_count(self.cnt_total as f64),
                1 if on && a.get(Meas::Period).is_finite() => si_digits(a.get(Meas::Period), "s", digits),
                0 if on && a.get(Meas::Freq).is_finite() => si_digits(a.get(Meas::Freq), "Hz", digits),
                _ => "----".into(),
            };
            v.push((format!("Counter CH{} {}", c + 1, CNT_MODES[self.m.cnt_mode.min(2)]), t));
            let st = self.cnt_stat;
            if self.m.cnt_mode == 0 && st.n > 1 {
                v.push(("min / max".into(), format!("{} / {}", si_digits(st.min, "Hz", digits), si_digits(st.max, "Hz", digits))));
            }
        }
        v
    }

    /// The value of the active channel at time `t` of the last record.
    pub(super) fn wave_at(&self, t: f64) -> Option<f32> {
        let r = self.last[0].as_ref()?;
        let w = &r.ch[self.m.active];
        if w.is_empty() || r.dt <= 0.0 {
            return None;
        }
        let i = ((t - r.t0) / r.dt).round();
        (i >= 0.0 && (i as usize) < w.len()).then(|| w[i as usize]).filter(|v| v.is_finite())
    }

    /// Times of the two cursors.
    pub(super) fn cursor_times(&self) -> (f64, f64) {
        let (tpd, delay) = self.view();
        let t = |c: f32| ((c - 0.5) * HDIV * tpd + delay) as f64;
        (t(self.m.cur_a), t(self.m.cur_b))
    }

    /// File of a mask in the slot of the Storage page, beside the setups.
    fn mask_path(&self) -> std::path::PathBuf {
        self.setup_path().with_file_name(format!("mask{}.txt", self.store_slot))
    }

    pub(super) fn mask_save(&mut self) {
        let Some(r) = &self.mask_ref else { return self.toast("No mask yet: Create mask first") };
        let mut t = String::from("# OOS mask: the reference waveform in volts, one value per line\n");
        for v in r {
            t.push_str(&format!("{}\n", v));
        }
        let p = self.mask_path();
        let res = crate::store::write(&p, t.as_bytes());
        self.toast_saved(&p, res);
    }

    pub(super) fn mask_load(&mut self) {
        let p = self.mask_path();
        let Ok(t) = std::fs::read_to_string(&p) else { return self.toast(&format!("No {}", crate::store::file_name(&p))) };
        let v: Vec<f32> = t.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.trim().parse().ok()).filter(|v: &f32| v.is_finite()).collect();
        if v.len() < 2 {
            return self.toast("That file holds no mask");
        }
        self.mask_ref = Some(v);
        self.build_mask();
        self.m.mask_on = true;
        self.mask_pass = 0;
        self.mask_fail = 0;
        self.toast(&format!("Mask loaded from {}", crate::store::file_name(&p)));
    }

    /// The mean of the last record of channel `c` becomes part of its zero correction: for an input shorted or
    /// left open, at DC coupling.
    pub(super) fn zero_input(&mut self, c: usize) {
        let mean = self.last[0].as_ref().and_then(|r| {
            let v: Vec<f64> = r.ch[c].iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
            (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
        });
        let Some(mean) = mean else { return self.toast("Source channel has no data") };
        self.m.ch[c].bias += mean as f32;
        self.toast(&format!("CH{} zero bias {}", c + 1, si(self.m.ch[c].bias as f64, self.m.unit(c))));
    }

    /// File of a reference slot, beside the setups.
    fn ref_path(&self) -> std::path::PathBuf {
        self.setup_path().with_file_name(format!("ref{}.csv", self.ref_sel + 1))
    }

    /// CSV / OOS `.bin` / `.wfm` (`:WFM#002` or `:WFM#003`) that Import slot can open: the slot's own files, then Storage.
    fn ref_wave_candidates(&self) -> Vec<std::path::PathBuf> {
        let slot = self.ref_sel + 1;
        let mut v = Vec::new();
        let mut add = |p: std::path::PathBuf| {
            if p.is_file() && !v.iter().any(|q| q == &p) {
                v.push(p);
            }
        };
        if let Some(dir) = self.setup_path().parent() {
            for ext in ["csv", "bin", "wfm"] {
                add(dir.join(format!("ref{}.{ext}", slot)));
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.save_dir()) {
            let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            files.sort();
            for p in files {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if matches!(ext.as_str(), "csv" | "bin" | "wfm") {
                    add(p);
                }
            }
        }
        v
    }

    /// The reference of the Slot row as CSV: Time (s),Volts.
    pub(super) fn ref_export(&mut self) {
        let Some(r) = &self.refs[self.ref_sel] else { return self.toast("That slot is empty") };
        let mut t = String::from("Time (s),Volts\n");
        for (k, v) in r.v.iter().enumerate() {
            t.push_str(&format!("{:.12e},{}\n", r.t0 + k as f64 * r.dt, v));
        }
        let p = self.ref_path();
        let res = crate::store::write(&p, t.as_bytes());
        self.toast_saved(&p, res);
    }

    /// Opens a captured waveform into the selected R slot: CSV, OOS `.bin`, or `.wfm` (`:WFM#002` or `:WFM#003`).
    pub(super) fn ref_import(&mut self) {
        let slot = self.ref_sel + 1;
        if let Some(dir) = self.setup_path().parent() {
            for ext in ["wfm", "bin", "csv"] {
                let p = dir.join(format!("ref{}.{ext}", slot));
                if p.is_file() {
                    return self.ref_load_path(&p);
                }
            }
        }
        self.ref_files = self.ref_wave_candidates();
        match self.ref_files.len() {
            0 => self.toast("No CSV, BIN or WFM in storage"),
            1 => {
                let p = self.ref_files[0].clone();
                self.ref_load_path(&p);
            }
            _ => self.open_picker(Item::RefImport),
        }
    }

    pub(super) fn ref_load_path(&mut self, p: &std::path::Path) {
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(_) => return self.toast(&format!("No {}", crate::store::file_name(p))),
        };
        let traces = match crate::store::parse_wave(&bytes, &crate::store::file_name(p)) {
            Ok(t) => t,
            Err(e) => return self.toast(&e),
        };
        if traces.is_empty() {
            return self.toast("That file holds no waveform");
        }
        let c = self.ref_src;
        let n = traces.len().min(NREF);
        for (k, tr) in traces.into_iter().take(n).enumerate() {
            let slot = (self.ref_sel + k) % NREF;
            let span = tr.v.iter().copied().fold(0.0_f32, |m, x| m.max(x.abs())).max(0.1);
            self.refs[slot] = Some(RefWave {
                src: c,
                color: 0,
                label: tr.label,
                t0: tr.t0,
                dt: tr.dt,
                v: tr.v,
                vpd: nearest_ref_vdiv(span / 4.0),
                offset: 0.0,
                on: true,
            });
        }
        self.measure_refs();
        self.analyzer_update();
        self.toast(&format!("R{} loaded from {}", self.ref_sel + 1, crate::store::file_name(p)));
    }

    /// Name of a channel on its card: its label, or CHn.
    pub(super) fn ch_name(&self, c: usize) -> String {
        if self.m.ch_label[c].is_empty() { format!("CH{}", c + 1) } else { self.m.ch_label[c].clone() }
    }
}

fn nearest_ref_vdiv(want: f32) -> f32 {
    VSCALES.iter().copied().min_by(|a, b| (a - want).abs().partial_cmp(&(b - want).abs()).unwrap()).unwrap_or(0.2)
}
