// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurement rows (single channel and channel pairs), the DVM and the counter, tracking cursors, saved masks and channel
//! labels. All of them work on the records the screen shows.

use super::*;
use oos_dsp::measure::{delay, phase, Pair};

/// Measurements at once.
pub(super) const MEAS_MAX: usize = 14;
/// Rows per channel in the strip under the plot.
pub(super) const STRIP_PER_ROW: usize = 6;

/// Delay and phase between two channels: name, phase (else delay), edge of A and B rising.
pub const PAIRS: [(&str, bool, bool, bool); 8] = [
    ("Delay rise-rise", false, true, true),
    ("Delay fall-fall", false, false, false),
    ("Delay rise-fall", false, true, false),
    ("Delay fall-rise", false, false, true),
    ("Phase rise-rise", true, true, true),
    ("Phase fall-fall", true, false, false),
    ("Phase rise-fall", true, true, false),
    ("Phase fall-rise", true, false, true),
];

/// DVM modes and counter modes.
pub const DVM_MODES: [&str; 3] = ["DC", "AC rms", "AC+DC rms"];
pub const CNT_MODES: [&str; 3] = ["Frequency", "Period", "Totalize"];

/// One measurement as the screen shows it.
pub(super) struct MeasRow {
    pub name: String,
    /// colour and strip row: the channel, or channel A of a pair
    pub ch: usize,
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
    /// All measurements, in the order of the statistics.
    pub(super) fn meas_rows(&self) -> Vec<MeasRow> {
        let mut v: Vec<MeasRow> = self
            .m
            .meas
            .iter()
            .map(|&(k, c)| MeasRow { name: k.name().to_string(), ch: c, v: if self.m.ch[c].on { self.ana[c].get(k) } else { f64::NAN }, unit: self.m.ch[c].unit.of(k.unit()) })
            .collect();
        for &(k, a, b) in &self.m.meas2 {
            let (name, is_phase, ea, eb) = PAIRS[k.min(PAIRS.len() - 1)];
            let on = self.m.ch[a].on && self.m.ch[b].on;
            let p = Pair(ea, eb);
            let val = match (on, is_phase) {
                (false, _) => f64::NAN,
                (true, true) => phase(&self.ana[a], &self.ana[b], p),
                (true, false) => delay(&self.ana[a], &self.ana[b], p),
            };
            v.push(MeasRow { name: format!("{} {}-{}", name, a + 1, b + 1), ch: a, v: val, unit: if is_phase { "\u{b0}" } else { "s" } });
        }
        v
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
            return self.toast("Source A and B are the same channel");
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
    pub(super) fn dvm_value(&self) -> f64 {
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

    /// A CSV of that form into the slot of the Slot row: at least two rows, even time steps taken from the first two.
    pub(super) fn ref_import(&mut self) {
        let p = self.ref_path();
        let Ok(t) = std::fs::read_to_string(&p) else { return self.toast(&format!("No {}", crate::store::file_name(&p))) };
        let rows: Vec<(f64, f32)> = t
            .lines()
            .filter_map(|l| {
                let (a, b) = l.split_once(',')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .collect();
        let dt = match rows[..] {
            [a, b, ..] if b.0 > a.0 => b.0 - a.0,
            _ => return self.toast("That file holds no waveform"),
        };
        let c = self.ref_src;
        self.refs[self.ref_sel] = Some(RefWave {
            src: c,
            color: 0,
            label: String::new(),
            t0: rows[0].0,
            dt,
            v: rows.iter().map(|r| r.1).collect(),
            vpd: self.m.ch[c].volts_per_div(),
            offset: self.m.ch[c].offset,
            on: true,
        });
        self.toast(&format!("R{} loaded from {}", self.ref_sel + 1, crate::store::file_name(&p)));
    }

    /// Name of a channel on its card: its label, or CHn.
    pub(super) fn ch_name(&self, c: usize) -> String {
        if self.m.ch_label[c].is_empty() { format!("CH{}", c + 1) } else { self.m.ch_label[c].clone() }
    }
}
