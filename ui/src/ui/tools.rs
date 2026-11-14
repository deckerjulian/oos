// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurement rows (single channel and channel pairs), the DVM and the counter, tracking cursors, saved masks and channel
//! labels. All of them work on the records the screen shows.

use super::*;

#[cfg(test)]
#[path = "scpi_helpers_tests.rs"]
mod scpi_helpers_tests;
#[cfg(test)]
#[path = "frozen41_helpers_tests.rs"]
mod frozen41_helpers_tests;
use oos_dsp::measure::{burst_width, duration_n, time_outside, Analysis};
/// Statistics of the on-screen histogram, in the source channel's units.
pub(crate) struct HistStat {
    pub sum: u64,
    pub peaks: u64,
    pub max: f64,
    pub min: f64,
    pub mean: f64,
    pub median: f64,
    pub mode: f64,
    pub bin: f64,
    pub sigma: f64,
}


/// A bounded selection can hold the complete catalog for every analog channel and reference slot.
pub(super) const MEAS_MAX: usize = MEAS_SELECTION_MAX;
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
    pub computing: bool,
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
        if self.computing { "Computing".into() }
        else if !v.is_finite() {
            "----".into()
        } else if self.unit == "\u{b0}" {
            format!("{:.1}\u{b0}", v)
        } else {
            si_digits(v, self.unit, digits)
        }
    }
}

impl Ui {
    pub(super) fn measurement_count(&self) -> usize { self.m.meas.len() + self.m.meas2.len() + self.m.cat.len() }

    pub(crate) fn catalog_export_values(&self) -> Vec<(u16,[WaveId;4],f64)> {
        self.m.cat.iter().filter_map(|&(id,a,b,c,d)| self.cat_row(id,a,b,c,d).map(|row| (id,[a,b,c,d],row.v))).collect()
    }

    pub(super) fn selected_measurement_label(&self, index: usize) -> String {
        if let Some(&(kind, source)) = self.m.meas.get(index) { return format!("{} {}", self.wave_name(source), kind.name()); }
        let index = index.saturating_sub(self.m.meas.len());
        if let Some(&(kind, a, b)) = self.m.meas2.get(index) {
            return format!("{}-{} {}", self.wave_name(a), self.wave_name(b), PAIRS[kind.min(PAIRS.len()-1)].0);
        }
        let Some(&(id, a, b, c, d)) = self.m.cat.get(index.saturating_sub(self.m.meas2.len())) else { return String::new() };
        self.catalog_source_label(id, [a,b,c,d])
    }

    pub(super) fn remove_measurement(&mut self, index: usize) {
        if index < self.m.meas.len() {
            let entry = self.m.meas.remove(index); self.meas_own.remove(&entry);
        } else if index < self.m.meas.len() + self.m.meas2.len() {
            self.m.meas2.remove(index - self.m.meas.len());
        } else if index < self.measurement_count() {
            self.m.cat.remove(index - self.m.meas.len() - self.m.meas2.len());
        } else { return; }
        self.meas_cfg = None; self.clear_measurement_statistics(); self.relayout();
    }

    pub(super) fn accept_catalog_stat(&mut self, entry: (u16,WaveId,WaveId,WaveId,WaveId), value: f64, capture: u64) {
        if capture == 0 || capture != self.wfm_total || self.record_mode() || !value.is_finite() { return; }
        let Some(index) = self.m.cat.iter().position(|row| *row == entry) else { return };
        let index = self.m.meas.len() + self.m.meas2.len() + index;
        self.mstat.resize(self.measurement_count(), MStat::default());
        let stat = &mut self.mstat[index];
        if stat.n == 0 { stat.min = value; stat.max = value; }
        stat.n += 1; stat.sum += value; stat.sumsq += value * value;
        stat.min = stat.min.min(value); stat.max = stat.max.max(value);
    }

    /// Measurement `k` of a channel or a reference. A channel that is off, or a ref with no samples, is NaN.
    pub fn meas_at(&self, id: WaveId, k: Meas) -> f64 {
        if let WaveId::Ch(channel) = id {
            if Self::channel_basic_kind(k) { return self.channel_basic_reading(channel, k).unwrap_or(f64::NAN); }
        }
        match self.meas_analysis(id) {
            Some((a, t0)) => {
                let value = self.cached_measurement_reading(id, k, &a).unwrap_or_else(|| self.meas_of(&a, k));
                if matches!(k, Meas::TVmax | Meas::TVmin) { t0 + value } else { value }
            }
            None => f64::NAN,
        }
    }

    /// One channel-pair row. `k` is an entry of `PAIRS`.
    pub fn pair_at(&self, k: usize, a: WaveId, b: WaveId) -> f64 {
        self.cached_pair(k, a, b)
    }

    /// All measurements, in the order of the statistics.
    pub(super) fn meas_rows(&self) -> Vec<MeasRow> {
        let mut v: Vec<MeasRow> = self
            .m
            .meas
            .iter()
            .map(|&(k, id)| {
                let val = self.meas_number(id, k);
                MeasRow { name: k.name().to_string(), src: id, v: val, unit: self.meas_unit(id, k), computing: !val.is_finite() && self.meas_number_computing(id, k) }
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
            v.push(MeasRow { name: format!("{} {}-{}", name, tag(a), tag(b)), src: a, v: val, unit,
                computing: self.measurement_computing(a) || self.measurement_computing(b) || self.pair_measurement_computing(k, a, b) });
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
    pub(super) fn meas_analysis(&self, id: WaveId) -> Option<(std::sync::Arc<oos_dsp::measure::Analysis>, f64)> {
        match id {
            WaveId::Ch(c) => {
                if c >= n_ch() || !self.m.ch[c].on {
                    return None;
                }
                self.channel_measurement(c, true)
            }
            WaveId::Ref(i) => {
                self.reference_measurement(i)
            }
        }
    }

    /// Extremum times use the physical waveform coordinate, including the gate's
    /// start. Keep DSP timestamps relative so durations and edge pairing do not change.
    pub(super) fn meas_of_at(&self, a: &Analysis, k: Meas, t0: f64) -> f64 {
        let value = self.meas_of(a, k);
        if matches!(k, Meas::TVmax | Meas::TVmin) { t0 + value } else { value }
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
            Item::MeasOutside | Item::MeasEdges | Item::MeasRefs | Item::MeasGate => Kind::Choice,
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
            Item::MeasGate => "Gate",
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
            Item::MeasGate => self.measurement_gate_label().into(),
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
            Item::MeasGate => vec!["Screen".into(), "Full record".into()],
            _ => return None,
        })
    }

    pub(super) fn meas_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::MeasGate => {
                if d.rem_euclid(2) != 0 { self.set_measurement_full_record(!self.m.meas_full_record); }
            }
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
        self.clear_measurement_statistics();
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
        let v = self.meas_at(WaveId::Ch(c), Meas::Freq);
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
            return self.meas_at(WaveId::Ref(i), mode);
        }
        let c = self.m.dvm_src;
        if !self.m.ch[c].on {
            return f64::NAN;
        }
        self.meas_at(WaveId::Ch(c), mode)
    }

    /// Counter reading: frequency, period, or the totalised edge count. NaN when that mode has nothing to show.
    pub fn counter_value(&self) -> f64 {
        if !self.m.cnt_on {
            return f64::NAN;
        }
        let c = self.m.cnt_src.min(NCH - 1);
        match self.m.cnt_mode {
            2 => self.cnt_total as f64,
            1 if self.m.ch[c].on => self.meas_at(WaveId::Ch(c), Meas::Period),
            0 if self.m.ch[c].on => self.meas_at(WaveId::Ch(c), Meas::Freq),
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
                let period = self.meas_at(WaveId::Ref(i), Meas::Period);
                let frequency = self.meas_at(WaveId::Ref(i), Meas::Freq);
                let t = match self.m.cnt_mode {
                    2 => fmt_count(a.rises.len() as f64),
                    1 if period.is_finite() => si_digits(period, "s", digits),
                    0 if frequency.is_finite() => si_digits(frequency, "Hz", digits),
                    _ => "----".into(),
                };
                v.push((format!("Counter R{} {}", i + 1, CNT_MODES[self.m.cnt_mode.min(2)]), t));
            } else {
                let c = self.m.cnt_src;
                let digits = self.m.cnt_digits as usize;
                let on = self.m.ch[c].on;
                let period = self.meas_at(WaveId::Ch(c), Meas::Period);
                let frequency = self.meas_at(WaveId::Ch(c), Meas::Freq);
                let t = match self.m.cnt_mode {
                    2 => fmt_count(self.cnt_total as f64),
                    1 if on && period.is_finite() => si_digits(period, "s", digits),
                    0 if on && frequency.is_finite() => si_digits(frequency, "Hz", digits),
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

    /// A source at physical time, using original native samples and deskew.
    pub(crate) fn wave_at_source(&self, source: WaveId, t: f64) -> Option<f32> {
        if matches!(source, WaveId::Ch(c) if c >= n_ch() || !self.m.ch[c].on) { return None; }
        let (w, dt, t0) = self.deskew_samples(source)?;
        if w.is_empty() || !dt.is_finite() || dt <= 0.0 { return None; }
        let index = (t - t0) / dt;
        if !index.is_finite() || index < 0.0 || index > w.len().saturating_sub(1) as f64 { return None; }
        let i = index.floor() as usize;
        let a = *w.get(i)?;
        let b = w.get(i + 1).copied().unwrap_or(a);
        let value = a + index.fract() as f32 * (b - a);
        value.is_finite().then_some(value)
    }

    pub(crate) fn wave_at_ch(&self, c: usize, t: f64) -> Option<f32> {
        self.wave_at_source(WaveId::Ch(c), t)
    }

    /// Cursor A follows the selected workspace source.
    pub(crate) fn wave_at(&self, t: f64) -> Option<f32> {
        self.wave_at_source(self.selected_workspace_source()?, t)
    }

    pub(crate) fn cursor_b_source(&self) -> WaveId { self.m.cur_b_source.unwrap_or_else(|| self.cursor_source()) }

    pub(crate) fn set_cursor_b_source(&mut self, source: WaveId) -> bool {
        let valid = match source { WaveId::Ch(c) => c < n_ch(), WaveId::Ref(i) => self.refs.get(i).is_some_and(Option::is_some) };
        if valid { self.m.cur_b_source = Some(source); }
        valid
    }

    pub(crate) fn cursor_source_unit(&self, source: WaveId) -> &'static str {
        match source { WaveId::Ch(c) if c < n_ch() => self.m.unit(c), _ => "V" }
    }

    /// Times of the two cursors.
    pub(super) fn cursor_times(&self) -> (f64, f64) {
        (self.cursor_time_at(self.m.cur_a), self.cursor_time_at(self.m.cur_b))
    }

    /// Time at a horizontal cursor fraction (0 is the left edge, 1 the right).
    pub(crate) fn cursor_time_at(&self, frac: f32) -> f64 {
        let (tpd, delay) = self.view();
        (frac as f64 - 0.5) * HDIV as f64 * tpd as f64 + delay as f64
    }

    /// Screen fraction for a time, kept on the graticule.
    pub(crate) fn cursor_frac_time(&self, t: f64) -> f32 {
        let (tpd, delay) = self.view();
        let span = HDIV as f64 * tpd as f64;
        if !t.is_finite() || !span.is_finite() || span <= 0.0 {
            return 0.5;
        }
        ((t - delay as f64) / span + 0.5).clamp(0.0, 1.0) as f32
    }

    /// Cursor focus follows the displayed source, independently of acquisition.
    pub(crate) fn cursor_source(&self) -> WaveId {
        self.selected_workspace_source().unwrap_or(WaveId::Ch(self.m.active.min(n_ch() - 1)))
    }

    pub(crate) fn set_cursor_source(&mut self, source: WaveId) -> bool {
        let available = match source {
            WaveId::Ch(c) => c < n_ch(),
            WaveId::Ref(i) => self.refs.get(i).is_some_and(Option::is_some),
        };
        if !available { return false; }
        self.select_workspace_source(source);
        self.relayout();
        true
    }

    /// Physical volts at the selected lane's screen fraction, including vertical zoom.
    pub(crate) fn cursor_volt_at(&self, frac: f32) -> f64 {
        let Some((source, scale, offset, _)) = self.cursor_source_context() else { return f64::NAN; };
        let area = self.source_area(source).unwrap_or(self.area);
        self.zoom_divs_at(area, frac) as f64 * scale as f64 - offset as f64
    }

    pub(crate) fn cursor_frac_volt(&self, v: f64) -> f32 {
        let Some((source, scale, offset, _)) = self.cursor_source_context() else { return 0.5; };
        let area = self.source_area(source).unwrap_or(self.area);
        let (factor, center) = self.zoom_vertical_transform(area);
        if !v.is_finite() || !scale.is_finite() || scale <= 0.0 { return 0.5; }
        (0.5 - ((v + offset as f64) / scale as f64 - center as f64)
            * factor as f64 / VDIV as f64).clamp(0.0, 1.0) as f32
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

    /// Ok(true) adds a row; Ok(false) preserves an existing row and its statistics.
    pub(crate) fn meas_add(&mut self, k: Meas, src: WaveId) -> Result<bool, &'static str> {
        let e = (k, src);
        if self.m.meas.iter().any(|item| *item == e) {
            return Ok(false);
        }
        if !self.meas_make_room() {
            return Err("Measurement selection is full");
        }
        self.m.meas.push(e);
        self.ensure_meas_own();
        self.mark_changed();
        Ok(true)
    }

    /// Put one pair measurement on the readout, with the same idempotent contract.
    pub(crate) fn meas_add_pair(&mut self, k: usize, a: WaveId, b: WaveId) -> Result<bool, &'static str> {
        if a == b {
            return Err("Source A and B are the same source");
        }
        let e = (k, a, b);
        if self.m.meas2.iter().any(|item| *item == e) {
            return Ok(false);
        }
        if !self.meas_make_room() {
            return Err("Measurement selection is full");
        }
        self.m.meas2.push(e);
        self.mark_changed();
        Ok(true)
    }

    /// `:MEASure:CLEar`: the strip, the pair rows, and the catalog rows.
    pub(crate) fn meas_clear_remote(&mut self) -> bool {
        if self.m.meas.is_empty() && self.m.meas2.is_empty() && self.m.cat.is_empty() { return false; }
        self.m.meas.clear();
        self.m.meas2.clear();
        self.m.cat.clear();
        self.meas_own.clear();
        self.meas_cfg = None;
        self.mark_changed();
        true
    }

    pub(crate) fn stats_shown(&self) -> bool {
        self.stats_on
    }

    pub(crate) fn set_stats_shown(&mut self, on: bool) {
        self.stats_on = on;
    }

    /// `:MEASure:STATistic:RESet`: the history, not the rows.
    pub(crate) fn stats_reset(&mut self) {
        self.clear_measurement_statistics();
    }

    /// Count, current value, min, max, mean, and standard deviation of one strip row.
    /// A row that is not on the strip still has a current value; the history is empty.
    pub(crate) fn meas_remote_stat(&self, k: Meas, src: WaveId) -> (u64, f64, f64, f64, f64, f64) {
        let current = self.meas_number(src, k);
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

    /// The channel the histogram reads: the measurement source when that channel is on, otherwise the first channel on.
    pub(crate) fn hist_source(&self) -> Option<usize> {
        self.hist_src()
    }

    /// Histogram statistics in volts (vertical) or seconds (horizontal). None when the histogram is off.
    pub(crate) fn hist_stat(&self) -> Option<HistStat> {
        let (bins, mean_px, sd_px) = self.hist_bins()?;
        let c = self.hist_src()?;
        let n: u64 = bins.iter().sum();
        let ch = &self.m.ch[c];
        let vertical = self.m.hist == Hist::Vertical;
        let (scale, mid, origin, high_is_low_index) = if vertical {
            let h = bins.len().max(1) as f64;
            let area = self.source_area(WaveId::Ch(c)).unwrap_or(self.wave_area());
            let (factor, center) = self.zoom_vertical_transform(area);
            let vpp = ch.volts_per_div() as f64 * VDIV as f64 / h / factor as f64;
            (vpp, h / 2.0, center as f64 * ch.volts_per_div() as f64 - ch.offset as f64, true)
        } else {
            let w = bins.len().max(1) as f64;
            let (tdiv, delay) = self.view();
            let spp = tdiv as f64 / (w / HDIV as f64);
            (spp, w / 2.0, delay as f64, false)
        };
        let axis = |pix: f64| {
            if high_is_low_index { (mid - pix) * scale + origin } else { (pix - mid) * scale + origin }
        };
        if n == 0 {
            return Some(HistStat { sum: 0, peaks: 0, max: f64::NAN, min: f64::NAN, mean: f64::NAN, median: f64::NAN, mode: f64::NAN, bin: scale, sigma: f64::NAN });
        }
        let first = bins.iter().position(|&b| b > 0).unwrap() as f64;
        let last = bins.iter().rposition(|&b| b > 0).unwrap() as f64;
        let (min_px, max_px) = if high_is_low_index { (last, first) } else { (first, last) };
        let mut peaks = 0u64;
        for i in 0..bins.len() {
            let b = bins[i];
            if b == 0 {
                continue;
            }
            let left = if i == 0 { 0 } else { bins[i - 1] };
            let right = bins.get(i + 1).copied().unwrap_or(0);
            if b > left && b >= right {
                peaks += 1;
            }
        }
        let mut acc = 0u64;
        let mut med = 0usize;
        for (i, &b) in bins.iter().enumerate() {
            acc += b;
            if acc * 2 >= n {
                med = i;
                break;
            }
        }
        let mode_i = bins.iter().enumerate().max_by_key(|(_, b)| *b).map(|(i, _)| i).unwrap_or(0);
        Some(HistStat {
            sum: n,
            peaks,
            max: axis(max_px),
            min: axis(min_px),
            mean: axis(mean_px),
            median: axis(med as f64),
            mode: axis(mode_i as f64),
            bin: scale,
            sigma: sd_px * scale,
        })
    }

    /// Horizontal tolerance is 0 to 2 div. Vertical is 0.04 to 4 div. A mask already on screen is rebuilt.
    pub(crate) fn mask_tolerance(&mut self, x: Option<f32>, y: Option<f32>) -> bool {
        if x.is_some_and(|v| !(0.0..=2.0).contains(&v)) || y.is_some_and(|v| !(0.04..=4.0).contains(&v)) { return false; }
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

    /// Waveforms Import slot can open: the slot's own files, then Storage. Same extensions as `load`.
    fn ref_wave_candidates(&self) -> Vec<std::path::PathBuf> {
        let slot = self.ref_sel + 1;
        let mut v = Vec::new();
        let mut add = |p: std::path::PathBuf| {
            if p.is_file() && !v.iter().any(|q| q == &p) {
                v.push(p);
            }
        };
        if let Some(dir) = self.setup_path().parent() {
            for ext in crate::store::WAVE_EXTS {
                add(dir.join(format!("ref{}.{ext}", slot)));
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.save_dir()) {
            let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            files.sort();
            for p in files {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if crate::store::WAVE_EXTS.iter().any(|e| *e == ext) {
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

    /// Opens a captured waveform into the selected R slot.
    pub(super) fn ref_import(&mut self) {
        let slot = self.ref_sel + 1;
        if let Some(dir) = self.setup_path().parent() {
            for ext in crate::store::WAVE_EXTS {
                let p = dir.join(format!("ref{}.{ext}", slot));
                if p.is_file() {
                    return self.ref_load_path(&p);
                }
            }
        }
        self.ref_files = self.ref_wave_candidates();
        match self.ref_files.len() {
            0 => self.toast("No CSV, BIN, WFM, TRC, ISF, TR0 or WFMX in storage"),
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
        if self.refs.get(slot).is_none_or(Option::is_none) {
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
        if slot >= NREF || !vpd.is_finite() || vpd <= 0.0 {
            return false;
        }
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        self.ref_sel = slot;
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
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        self.ref_sel = slot;
        r.offset = offset;
        true
    }

    pub(crate) fn ref_color(&self, slot: usize) -> Option<usize> {
        self.refs.get(slot).and_then(|r| r.as_ref()).map(|r| r.color)
    }

    pub(crate) fn ref_color_set(&mut self, slot: usize, color: usize) -> bool {
        if slot >= NREF || color >= REF_COLORS.len() {
            return false;
        }
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        self.ref_sel = slot;
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
        let Some(r) = self.refs.get_mut(slot).and_then(|r| r.as_mut()) else { return false };
        self.ref_sel = slot;
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
            v: v.into(),
            vpd: self.m.ch[c].volts_per_div(),
            offset: self.m.ch[c].offset,
            on: true,
        });
        self.m.deskew_ref[self.ref_sel] = self.m.deskew_ch[c];
        self.measure_refs();
        self.analyzer_update();
        Ok(())
    }

    /// `refN` beside the setups, in `WAVE_EXTS` order. The first file that is there is the one loaded.
    pub(crate) fn ref_import_slot(&mut self) -> Result<(), String> {
        let slot = self.ref_sel + 1;
        let Some(dir) = self.setup_path().parent().map(|d| d.to_path_buf()) else {
            return Err(format!("missing ref{slot}"));
        };
        for ext in crate::store::WAVE_EXTS {
            let p = dir.join(format!("ref{slot}.{ext}"));
            if p.is_file() {
                return self.ref_load_file(&p);
            }
        }
        Err(format!("missing ref{slot} waveform"))
    }

    /// Loads a waveform into the selected slot.
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

#[cfg(test)]
mod physical_extremum_time_tests {
    use super::*;

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-15, "{actual:e} != {expected:e}");
    }

    #[test]
    fn extremum_times_follow_physical_origins_and_screen_gate_for_channels_and_refs() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.zoom = false;
        ui.m.tscale = tscales().iter().position(|&v| (v - 10e-9).abs() < 1e-14).unwrap();
        ui.m.tfine = 1.0;
        ui.m.delay = 200e-9;
        let (t0, dt) = (-500e-9, 10e-9);
        let mut values = vec![0.0; 100];
        values[5] = 3.0;
        values[80] = -2.0;
        values[68] = 1.0;
        values[72] = -1.0;
        let mut rec = Ui::blank_record();
        rec.t0 = t0;
        rec.dt = dt;
        rec.ch[0] = values.clone();
        ui.last[0] = Some(rec);
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "nonzero origin".into(),
            t0, dt, v: (values).into(), vpd: 0.5, offset: 0.0, on: true });
        ui.m.meas = vec![(Meas::TVmax, WaveId::Ch(0)), (Meas::TVmin, WaveId::Ref(0)), (Meas::Vpp, WaveId::Ref(0))];

        for (full, max_time, min_time, vpp) in [(true, -450e-9, 300e-9, 5.0), (false, 180e-9, 220e-9, 2.0)] {
            ui.set_measurement_full_record(full);
            for id in [WaveId::Ch(0), WaveId::Ref(0)] {
                close(ui.meas_at(id, Meas::TVmax), max_time);
                close(ui.meas_at(id, Meas::TVmin), min_time);
                close(ui.fact_meas(Meas::TVmax, id), max_time);
                close(ui.fact_meas(Meas::TVmin, id), min_time);
                assert_eq!(ui.meas_at(id, Meas::Vpp), vpp, "voltage measurements must not gain a time offset");
            }
            let rows = ui.meas_rows();
            close(rows[0].v, max_time);
            close(rows[1].v, min_time);
            assert_eq!(rows[2].v, vpp);
        }
    }

    #[test]
    fn first_reference_maximum_reports_its_record_origin() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let origin = -9.99990584751956e-7;
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "clock origin".into(),
            t0: origin, dt: 20e-12, v: (vec![1.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0, 0.0]).into(),
            vpd: 0.5, offset: 0.0, on: true });
        ui.set_measurement_full_record(true);
        close(ui.meas_at(WaveId::Ref(0), Meas::TVmax), origin);
        close(ui.fact_meas(Meas::TVmax, WaveId::Ref(0)), origin);
        close(ui.meas_at(WaveId::Ref(0), Meas::TVmin), origin + 40e-12);
    }
}
