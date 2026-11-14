// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurement rows (single channel and channel pairs), the DVM and the counter, tracking cursors, saved masks and channel
//! labels. All of them work on the records the screen shows.

use super::*;
use oos_dsp::measure::{burst_width, delay, duration_n, hold_time_at, phase, setup_hold_violations_at, setup_time_at, skew, time_outside, Analysis, Pair};

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

/// A bounded selection can hold the complete catalog for every analog channel and reference slot.
pub(super) const MEAS_MAX: usize = 1024;
/// Measurements on one row of the strip when it runs across the screen.
pub(super) const STRIP_PER_ROW: usize = 6;

/// What a channel-pair row measures. The edge flags choose the edges of delay, phase and skew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairKind {
    Delay,
    Phase,
    Skew,
    Setup,
    Hold,
    SetupAndHold,
}

/// Channel pairs: name, kind, edge of A rising, edge of B rising. New rows stay at the end so a saved index still means the same row.
pub const PAIRS: [(&str, PairKind, bool, bool); 17] = [
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
    ("Setup+Hold", PairKind::SetupAndHold, true, true),
    ("Setup fall", PairKind::Setup, false, true),
    ("Hold fall", PairKind::Hold, false, true),
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
        self.meas_number(id, k)
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
                    PairKind::Setup => setup_time_at(&aa, &bb, ea),
                    PairKind::Hold => hold_time_at(&aa, &bb, ea),
                    PairKind::SetupAndHold => setup_hold_violations_at(&aa, &bb, self.m.meas_tsetup, self.m.meas_thold, ea),
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
                let val = self.meas_number(id, k);
                let unit = self.meas_unit(id, k);
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
            let unit = match kind {
                PairKind::Phase => "\u{b0}",
                PairKind::SetupAndHold => "",
                _ => "s",
            };
            v.push(MeasRow { name: format!("{} {}-{}", name, tag(a), tag(b)), src: a, v: val, unit });
        }
        for &(id, a, b, c, d) in &self.m.cat {
            if let Some(row) = self.cat_row(id, a, b, c, d) {
                v.push(row);
            }
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
                let (samples, t0, dt) = self.reference_slice(i)?;
                if samples.len() < 2 || !(dt > 0.0) {
                    return None;
                }
                let analysis = if self.ref_ana_valid[i] {
                    self.ref_ana[i].clone()
                } else {
                    // Hidden references skip idle capture work, but an explicit
                    // measurement still uses this reference's current gate,
                    // sample interval and thresholds. Do not infer cache validity
                    // from NaN results: a valid DC record may lack timing values.
                    oos_dsp::measure::analyze_refs(samples, dt,
                        0.4 * self.refs[i].as_ref().unwrap().vpd as f64,
                        self.m.meas_lpct, self.m.meas_hpct)
                };
                Some((analysis, t0))
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
            Item::MeasIdle | Item::MeasN | Item::MeasTsetup | Item::MeasThold => Kind::Value,
            Item::MeasOutside | Item::MeasEdges | Item::MeasRefs => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn meas_cfg_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::MeasIdle => "Idle",
            Item::MeasOutside => "Outside",
            Item::MeasN => "N periods",
            Item::MeasEdges => "Edges",
            Item::MeasRefs => "Refs",
            Item::MeasTsetup => "Tsetup",
            Item::MeasThold => "Thold",
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
            Item::MeasRefs => format!("{:.0}/{:.0}", m.meas_lpct * 100.0, m.meas_hpct * 100.0),
            Item::MeasTsetup => si_short(m.meas_tsetup, "s"),
            Item::MeasThold => si_short(m.meas_thold, "s"),
            _ => return None,
        })
    }

    pub(super) fn meas_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::MeasOutside => ["Above", "Below", "Both"].iter().map(|s| s.to_string()).collect(),
            Item::MeasEdges => vec!["Clock".into(), "Data".into()],
            Item::MeasRefs => vec!["10/90".into(), "20/80".into()],
            _ => return None,
        })
    }

    pub(super) fn meas_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::MeasIdle => {
                let cur = if self.m.meas_idle > 0.0 { self.m.meas_idle } else { 0.0 };
                let l: Vec<f64> = std::iter::once(0.0).chain((0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e - 9)))).collect();
                let i = l.iter().position(|&v| v >= cur * 0.999).unwrap_or(0);
                self.m.meas_idle = l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize];
            }
            Item::MeasTsetup | Item::MeasThold => {
                let cur = if it == Item::MeasTsetup { self.m.meas_tsetup } else { self.m.meas_thold };
                let l: Vec<f64> = std::iter::once(0.0).chain((0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e - 9)))).collect();
                let i = l.iter().position(|&v| v >= cur * 0.999).unwrap_or(0);
                let v = l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize];
                if it == Item::MeasTsetup {
                    self.m.meas_tsetup = v;
                } else {
                    self.m.meas_thold = v;
                }
            }
            Item::MeasOutside => {
                let all = [oos_dsp::measure::Outside::Above, oos_dsp::measure::Outside::Below, oos_dsp::measure::Outside::Both];
                let i = all.iter().position(|&x| x == self.m.meas_out).unwrap_or(2);
                self.m.meas_out = all[(i as i32 + d).rem_euclid(all.len() as i32) as usize];
            }
            Item::MeasN => self.m.meas_n = (self.m.meas_n as i32 + d).clamp(1, 32) as u32,
            Item::MeasEdges => self.m.meas_data = !self.m.meas_data,
            Item::MeasRefs => {
                if self.m.meas_lpct < 0.15 {
                    self.m.meas_lpct = 0.2;
                    self.m.meas_hpct = 0.8;
                } else {
                    self.m.meas_lpct = 0.1;
                    self.m.meas_hpct = 0.9;
                }
                self.measure_last();
            }
            _ => return false,
        }
        true
    }

    /// Refuse a full selection explicitly rather than silently removing an older measurement.
    pub(super) fn meas_make_room(&mut self) -> bool {
        if self.m.meas.len() + self.m.meas2.len() + self.m.cat.len() >= MEAS_MAX {
            self.toast("Measurement selection is full. Remove an item in Selected before adding another.");
            return false;
        }
        true
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
            if !self.meas_make_room() { return; }
            self.m.meas2.push(e);
        }
        self.mstat.clear();
    }

    /// DVM and counter: a new record came in. Offline mode reads the reference directly.
    pub(super) fn counter_update(&mut self) {
        if self.offline || !self.m.cnt_on {
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

    pub(crate) fn counter_clear(&mut self) {
        self.cnt_total = 0;
        self.cnt_stat = MStat::default();
    }

    /// DVM reading of its source, volts. Offline mode reads the reference on the card.
    pub fn dvm_value(&self) -> f64 {
        let mode = match self.m.dvm_mode {
            0 => Meas::Vavg,
            1 => Meas::VrmsAc,
            _ => Meas::Vrms,
        };
        if let Some(i) = self.offline_meter() {
            return self.ref_ana[i].get(mode);
        }
        let c = self.m.dvm_src;
        if !self.m.ch[c].on {
            return f64::NAN;
        }
        self.ana[c].get(mode)
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
            let who = match self.offline_meter() {
                Some(i) => format!("R{}", i + 1),
                None => format!("CH{}", self.m.dvm_src + 1),
            };
            v.push((format!("DVM {} {}", who, DVM_MODES[self.m.dvm_mode.min(2)]), t));
        }
        if self.m.cnt_on {
            if let Some(i) = self.offline_meter() {
                let a = &self.ref_ana[i];
                let digits = self.m.cnt_digits as usize;
                let t = match self.m.cnt_mode {
                    2 => fmt_count(a.rises.len() as f64),
                    1 if a.get(Meas::Period).is_finite() => si_digits(a.get(Meas::Period), "s", digits),
                    0 if a.get(Meas::Freq).is_finite() => si_digits(a.get(Meas::Freq), "Hz", digits),
                    _ => "----".into(),
                };
                v.push((format!("Counter R{} {}", i + 1, CNT_MODES[self.m.cnt_mode.min(2)]), t));
            } else {
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
        }
        v
    }

    /// The value of the active channel at time `t` of the last record.
    pub(crate) fn wave_at(&self, t: f64) -> Option<f32> {
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
        (self.cursor_time_at(self.m.cur_a), self.cursor_time_at(self.m.cur_b))
    }

    /// Time at a horizontal cursor fraction (0 is the left edge, 1 the right).
    pub(crate) fn cursor_time_at(&self, frac: f32) -> f64 {
        let (tpd, delay) = self.view();
        ((frac - 0.5) * HDIV * tpd + delay) as f64
    }

    /// Screen fraction for a time, kept on the graticule.
    pub(crate) fn cursor_frac_time(&self, t: f64) -> f32 {
        let (tpd, delay) = self.view();
        let span = HDIV * tpd;
        if span == 0.0 {
            return 0.5;
        }
        ((t as f32 - delay) / span + 0.5).clamp(0.0, 1.0)
    }

    /// Volts at a vertical cursor fraction on the active channel (0 is the top, 1 the bottom).
    pub(crate) fn cursor_volt_at(&self, frac: f32) -> f64 {
        let ch = &self.m.ch[self.m.active.min(n_ch() - 1)];
        ((0.5 - frac) * VDIV * ch.volts_per_div() - ch.offset) as f64
    }

    pub(crate) fn cursor_frac_volt(&self, v: f64) -> f32 {
        let ch = &self.m.ch[self.m.active.min(n_ch() - 1)];
        let span = VDIV * ch.volts_per_div();
        if span == 0.0 {
            return 0.5;
        }
        (0.5 - (v as f32 + ch.offset) / span).clamp(0.0, 1.0)
    }

    /// The source `:MEASure:ITEM` uses when the command names none, and the second source of a pair.
    pub(crate) fn meas_source(&self) -> WaveId {
        self.meas_src
    }

    pub(crate) fn meas_source_b(&self) -> WaveId {
        self.meas_src_b
    }

    pub(crate) fn set_meas_source(&mut self, id: WaveId) {
        self.meas_src = id;
    }

    pub(crate) fn set_meas_source_b(&mut self, id: WaveId) {
        self.meas_src_b = id;
    }

    /// Put one measurement on the readout. Already there is a success. False when the list is full.
    pub(crate) fn meas_add(&mut self, k: Meas, src: WaveId) -> bool {
        let e = (k, src);
        if self.m.meas.iter().any(|item| *item == e) {
            return true;
        }
        if !self.meas_make_room() {
            return false;
        }
        self.m.meas.push(e);
        self.mstat.clear();
        true
    }

    /// Put one pair measurement on the readout. False when the sources match or the list is full.
    pub(crate) fn meas_add_pair(&mut self, k: usize, a: WaveId, b: WaveId) -> bool {
        if a == b {
            return false;
        }
        let e = (k, a, b);
        if self.m.meas2.iter().any(|item| *item == e) {
            return true;
        }
        if !self.meas_make_room() {
            return false;
        }
        self.m.meas2.push(e);
        self.mstat.clear();
        true
    }

    /// `:MEASure:CLEar`: the strip, the pair rows, and the catalog rows.
    pub(crate) fn meas_clear_remote(&mut self) {
        self.m.meas.clear();
        self.m.meas2.clear();
        self.m.cat.clear();
        self.meas_own.clear();
        self.meas_cfg = None;
        self.mstat.clear();
    }

    pub(crate) fn stats_shown(&self) -> bool {
        self.stats_on
    }

    pub(crate) fn set_stats_shown(&mut self, on: bool) {
        self.stats_on = on;
    }

    /// `:MEASure:STATistic:RESet`: the history, not the rows.
    pub(crate) fn stats_reset(&mut self) {
        self.mstat.clear();
    }

    /// Count, current value, min, max, mean, and standard deviation of one strip row.
    /// A row that is not on the strip still has a current value; the history is empty.
    pub(crate) fn meas_remote_stat(&self, k: Meas, src: WaveId) -> (u64, f64, f64, f64, f64, f64) {
        let current = self.meas_at(src, k);
        let idx = self.m.meas.iter().position(|&(kk, id)| kk == k && id == src);
        self.stat_at(idx, current)
    }

    pub(crate) fn pair_remote_stat(&self, k: usize, a: WaveId, b: WaveId) -> (u64, f64, f64, f64, f64, f64) {
        let current = self.pair_at(k, a, b);
        let idx = self.m.meas2.iter().position(|&(kk, aa, bb)| kk == k && aa == a && bb == b).map(|i| self.m.meas.len() + i);
        self.stat_at(idx, current)
    }

    fn stat_at(&self, idx: Option<usize>, current: f64) -> (u64, f64, f64, f64, f64, f64) {
        let Some(st) = idx.and_then(|i| self.mstat.get(i)).filter(|st| st.n > 0) else {
            return (0, current, f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        };
        let mean = st.sum / st.n as f64;
        let dev = if st.n > 1 { (st.sumsq / st.n as f64 - mean * mean).max(0.0).sqrt() } else { f64::NAN };
        (st.n, current, st.min, st.max, mean, dev)
    }

    pub(crate) fn zoom_set_remote(&mut self, on: bool) {
        self.set_zoom(on);
    }

    pub(crate) fn zoom_clamp(&mut self) {
        self.clamp_zoom();
    }

    pub(crate) fn xy_on(&self) -> bool {
        self.xy
    }

    pub(crate) fn xy_set(&mut self, on: bool) {
        self.xy = on;
    }

    /// Builds the pass/fail mask from the source channel of the record on screen.
    pub(crate) fn mask_create(&mut self) -> bool {
        let c = self.m.mask_src;
        let ready = self.last[0].as_ref().is_some_and(|r| c < r.ch.len() && !r.ch[c].is_empty());
        if !ready {
            return false;
        }
        self.create_mask();
        true
    }

    pub(crate) fn mask_counts(&self) -> (u64, u64) {
        (self.mask_pass, self.mask_fail)
    }

    pub(crate) fn mask_counts_reset(&mut self) {
        self.mask_pass = 0;
        self.mask_fail = 0;
    }

    /// Horizontal tolerance is 0 to 2 div. Vertical is 0.04 to 4 div. A mask already on screen is rebuilt.
    pub(crate) fn mask_tolerance(&mut self, x: Option<f32>, y: Option<f32>) -> bool {
        if let Some(v) = x {
            if !(0.0..=2.0).contains(&v) {
                return false;
            }
            self.m.mask_x = v;
        }
        if let Some(v) = y {
            if !(0.04..=4.0).contains(&v) {
                return false;
            }
            self.m.mask_y = v;
        }
        if self.mask_ref.is_some() {
            self.build_mask();
        }
        true
    }

    /// File of a mask in the slot of the Storage page, beside the setups.
    fn mask_path(&self) -> std::path::PathBuf {
        self.setup_path().with_file_name(format!("mask{}.txt", self.store_slot))
    }

    pub(super) fn mask_save(&mut self) {
        let Some(r) = &self.mask_ref else { return self.toast("No mask yet: Create mask first") };
        let t = crate::store::mask_text(self.m.mask_src, self.m.mask_x, self.m.mask_y, self.m.mask_inv, r);
        let slot = self.mask_path();
        if let Err(e) = crate::store::write(&slot, t.as_bytes()) {
            return self.toast(&format!("Save failed: {}", e));
        }
        let p = crate::store::next_name(&self.save_dir(), "mask", "txt");
        let res = crate::store::write(&p, t.as_bytes());
        self.toast_saved(&p, res);
    }

    pub(super) fn mask_load(&mut self) {
        let slot = self.mask_path();
        let p = if slot.is_file() {
            slot
        } else if let Some(p) = crate::store::latest_name(&self.save_dir(), "mask", "txt") {
            p
        } else {
            return self.toast("No mask file in the slot or save folder");
        };
        let Ok(t) = std::fs::read_to_string(&p) else { return self.toast(&format!("No {}", crate::store::file_name(&p))) };
        let mf = match crate::store::parse_mask(&t) {
            Ok(m) if m.v.len() >= 2 => m,
            Ok(_) => return self.toast("That file holds no mask"),
            Err(e) => return self.toast(&format!("Load failed: {}", e)),
        };
        if let Some(src) = mf.src {
            self.m.mask_src = src;
        }
        if let Some(x) = mf.x {
            self.m.mask_x = x;
        }
        if let Some(y) = mf.y {
            self.m.mask_y = y;
        }
        if let Some(inv) = mf.inv {
            self.m.mask_inv = inv;
        }
        self.mask_ref = Some(mf.v);
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

    pub(super) fn ref_load_path(&mut self, path: &std::path::Path) {
        match self.ref_load_file(path) {
            Ok(()) => self.toast(&format!("R{} loaded from {}", self.ref_sel + 1, crate::store::file_name(path))),
            Err(error) => self.toast(&error),
        }
    }

    /// The Slot row: R1 is 0.
    pub(crate) fn ref_slot(&self) -> usize {
        self.ref_sel
    }

    pub(crate) fn ref_slot_set(&mut self, slot: usize) -> bool {
        if slot >= NREF {
            return false;
        }
        self.ref_sel = slot;
        true
    }

    /// Whether the selected slot is drawn. None when it is empty.
    pub(crate) fn ref_shown(&self) -> Option<bool> {
        self.refs.get(self.ref_sel).and_then(|r| r.as_ref()).map(|r| r.on)
    }

    pub(crate) fn ref_show_set(&mut self, on: bool) -> bool {
        let Some(r) = self.refs.get_mut(self.ref_sel).and_then(|r| r.as_mut()) else { return false };
        r.on = on;
        true
    }

    /// Whether slot `slot` is drawn. None when it is empty or out of range.
    pub(crate) fn ref_shown_slot(&self, slot: usize) -> Option<bool> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.on)
    }

    pub(crate) fn ref_show_slot(&mut self, slot: usize, on: bool) -> bool {
        if slot >= NREF {
            return false;
        }
        self.ref_sel = slot;
        self.ref_show_set(on)
    }

    /// Channel a `:REFerence:SAVE` of this slot copies. A saved slot remembers its own source.
    pub(crate) fn ref_source_of(&self, slot: usize) -> usize {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.src).unwrap_or(if slot == self.ref_sel { self.ref_src } else { 0 })
    }

    pub(crate) fn ref_source_set(&mut self, slot: usize, ch: usize) -> bool {
        if slot >= NREF || ch >= n_ch() {
            return false;
        }
        self.ref_sel = slot;
        self.ref_src = ch;
        if let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) {
            r.src = ch;
        }
        true
    }

    pub(crate) fn ref_vscale(&self, slot: usize) -> Option<f32> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.vpd)
    }

    pub(crate) fn ref_vscale_set(&mut self, slot: usize, vpd: f32) -> bool {
        if slot >= NREF || !(vpd > 0.0) {
            return false;
        }
        self.ref_sel = slot;
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        r.vpd = vpd;
        true
    }

    pub(crate) fn ref_voffset(&self, slot: usize) -> Option<f32> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.offset)
    }

    pub(crate) fn ref_voffset_set(&mut self, slot: usize, offset: f32) -> bool {
        if slot >= NREF || !offset.is_finite() {
            return false;
        }
        self.ref_sel = slot;
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        r.offset = offset;
        true
    }

    pub(crate) fn ref_color(&self, slot: usize) -> Option<usize> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.color)
    }

    pub(crate) fn ref_color_set(&mut self, slot: usize, color: usize) -> bool {
        if slot >= NREF {
            return false;
        }
        self.ref_sel = slot;
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        r.color = color;
        true
    }

    /// `:REFerence:RESet`: offset back to zero, scale back to the source channel.
    pub(crate) fn ref_reset_scale(&mut self, slot: usize) -> bool {
        if slot >= NREF {
            return false;
        }
        let Some(src) = self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.src) else { return false };
        let vpd = self.m.ch[src.min(n_ch() - 1)].volts_per_div();
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        r.offset = 0.0;
        r.vpd = vpd.max(1e-9);
        true
    }

    pub(crate) fn ref_label(&self, slot: usize) -> Option<String> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.label.clone())
    }

    pub(crate) fn ref_label_set(&mut self, slot: usize, label: String) -> bool {
        if slot >= NREF {
            return false;
        }
        self.ref_sel = slot;
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        r.label = label;
        true
    }

    /// Empties every reference slot, the same as Clear all.
    pub(crate) fn ref_clear_all(&mut self) {
        self.refs = std::array::from_fn(|_| None);
        self.measure_refs();
        self.analyzer_update();
    }

    /// Writes the Source channel into the selected slot.
    pub(crate) fn ref_save_slot(&mut self) -> Result<(), String> {
        let c = self.ref_src;
        let rec = self.last[0].clone();
        let Some(r) = rec.as_ref().filter(|r| c < r.ch.len() && !r.ch[c].is_empty()) else {
            return Err("source channel has no data".into());
        };
        let (v, dt) = self.meas_wave(r, c);
        if v.len() < 2 || !(dt > 0.0) {
            return Err("source channel has no data".into());
        }
        let t0 = if v.len() > r.ch[c].len() { r.raw.as_ref().map(|x| x.t0).unwrap_or(r.t0) } else { r.t0 };
        self.refs[self.ref_sel] = Some(RefWave {
            src: c,
            color: 0,
            label: String::new(),
            t0,
            dt,
            v,
            vpd: self.m.ch[c].volts_per_div(),
            offset: self.m.ch[c].offset,
            on: true,
        });
        self.measure_refs();
        self.analyzer_update();
        Ok(())
    }

    /// `refN.wfm`, then `refN.bin`, then `refN.csv`, beside the setups. The first file that is there is the one loaded.
    pub(crate) fn ref_import_slot(&mut self) -> Result<(), String> {
        let slot = self.ref_sel + 1;
        let Some(dir) = self.setup_path().parent().map(|d| d.to_path_buf()) else {
            return Err(format!("missing ref{slot}"));
        };
        for ext in ["wfm", "bin", "csv"] {
            let p = dir.join(format!("ref{slot}.{ext}"));
            if p.is_file() {
                return self.ref_load_file(&p);
            }
        }
        Err(format!("missing ref{slot}.wfm, ref{slot}.bin or ref{slot}.csv"))
    }

    /// Loads a csv, an OOS `.bin`, or a `.wfm` (`:WFM#002` or `:WFM#003`) into the selected slot.
    /// A relative path is beside the setups. A missing file starts with `missing `.
    pub(crate) fn ref_load_named(&mut self, path: &str) -> Result<(), String> {
        let raw = path.trim().trim_matches('"');
        if raw.is_empty() {
            return Err("no file".into());
        }
        let p = std::path::Path::new(raw);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.setup_path().parent().map(|d| d.join(p)).unwrap_or_else(|| p.to_path_buf())
        };
        self.ref_load_file(&p)
    }

    fn ref_load_file(&mut self, p: &std::path::Path) -> Result<(), String> {
        if !p.is_file() {
            return Err(format!("missing {}", crate::store::file_name(p)));
        }
        let bytes = std::fs::read(p).map_err(|_| format!("missing {}", crate::store::file_name(p)))?;
        self.load_waveform_bytes(&bytes, &crate::store::file_name(p))
    }
    /// Name of a channel on its card: its label, or CHn.
    pub(super) fn ch_name(&self, c: usize) -> String {
        if self.m.ch_label[c].is_empty() { format!("CH{}", c + 1) } else { self.m.ch_label[c].clone() }
    }
}

fn nearest_ref_vdiv(want: f32) -> f32 {
    VSCALES.iter().copied().min_by(|a, b| (a - want).abs().partial_cmp(&(b - want).abs()).unwrap()).unwrap_or(0.2)
}
