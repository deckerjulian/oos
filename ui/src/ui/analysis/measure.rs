// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The measurements of channels and references behind the measurement row: the measurement gate (screen or full
//! record), the cache of a reference's or a long channel's analysis keyed by its settings (`RefMeasureKey`), and the
//! one background worker that analyses large records. A new input to an analysis goes into `RefMeasureKey`.

use super::*;

pub(super) const BACKGROUND_MEASUREMENT_POINTS: usize = 262_144;

/// A reference may keep the same allocation between accepted SIM captures.
/// Include its revision as well as the physical gate and analysis settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) struct RefMeasureKey {
    samples: usize,
    pub(super) len: usize,
    pub(super) lo: usize,
    pub(super) hi: usize,
    pub(super) dt: u64,
    t0: u64,
    pub(super) vpd: u32,
    pub(super) low: u64,
    pub(super) high: u64,
    epoch: u64,
    revision: u64,
    pub(super) full: bool,
    pub(super) basic: bool,
    pub(super) idle: u64,
    pub(super) outside: u8,
    pub(super) periods: u32,
    pub(super) data: bool,
    conditioning: (u8, bool, usize),
    capture: u64,
    /// the samples are min/max pairs (`Ui::measurement_on_pairs`): part of the key, so a cached analysis is not taken
    /// across a change of it (high res switched while stopped)
    pub(super) pairs: bool,
}

pub(super) struct RefMeasureCache {
    pub(super) key: RefMeasureKey,
    pub(super) analysis: Arc<oos_dsp::measure::Analysis>,
    readings: Vec<f64>,
}

pub(super) struct RefMeasureJob {
    pub(super) source: WaveId,
    pub(super) key: RefMeasureKey,
    pub(super) samples: MeasureSamples,
    pub(super) dt: f64,
    pub(super) min_amp: f64,
    pub(super) low: f64,
    pub(super) high: f64,
    /// the samples are min/max pairs (`Ui::measurement_on_pairs`)
    pub(super) pairs: bool,
    pub(super) own: bool,
    pub(super) analysis: Option<Arc<oos_dsp::measure::Analysis>>,
}

impl RefMeasureJob {
    pub(super) fn analyze(&self) -> oos_dsp::measure::Analysis {
        analyze_samples(self.samples.slice(), self.dt, self.min_amp, self.low, self.high, self.pairs)
    }
}

/// The analysis of a channel's or a ref's samples; min/max pairs measured in time on the mean of neighbours (#118).
pub(in crate::ui) fn analyze_samples(
    v: &[f32],
    dt: f64,
    min: f64,
    low: f64,
    high: f64,
    pairs: bool,
) -> oos_dsp::measure::Analysis {
    if pairs {
        oos_dsp::measure::analyze_pairs(v, dt, min, low, high)
    } else {
        oos_dsp::measure::analyze_refs(v, dt, min, low, high)
    }
}

pub(super) enum MeasureSamples {
    Owned(Vec<f32>),
    Native { frame: Arc<crate::board::Frame>, channel: usize, lo: usize, hi: usize },
}

impl MeasureSamples {
    fn slice(&self) -> &[f32] {
        match self {
            Self::Owned(v) => v,
            Self::Native { frame, channel, lo, hi } => &frame.ch[*channel][*lo..*hi],
        }
    }
}

fn basic_analysis(basic: crate::board::BasicStats) -> oos_dsp::measure::Analysis {
    let [vmin, vmax, _, avg, rms, std] = basic.values;
    oos_dsp::measure::Analysis {
        valid: basic.valid,
        vmin,
        vmax,
        avg,
        rms,
        std,
        rise_time: f64::NAN,
        fall_time: f64::NAN,
        t_vmax: f64::NAN,
        t_vmin: f64::NAN,
        ..Default::default()
    }
}

pub(super) fn scalar_readings(analysis: &oos_dsp::measure::Analysis, key: RefMeasureKey) -> Vec<f64> {
    use oos_dsp::measure::{burst_width, duration_n, time_outside, Outside};
    let outside = match key.outside {
        0 => Outside::Above,
        1 => Outside::Below,
        _ => Outside::Both,
    };
    ALL_MEAS
        .iter()
        .map(|&kind| match kind {
            Meas::Burst => burst_width(analysis, f64::from_bits(key.idle)),
            Meas::Outside => time_outside(analysis, outside),
            Meas::NPeriods => duration_n(analysis, key.periods.max(1) as usize, key.data),
            _ => analysis.get(kind),
        })
        .collect()
}

pub(super) struct RefMeasureResult {
    pub(super) source: WaveId,
    pub(super) key: RefMeasureKey,
    pub(super) analysis: Arc<oos_dsp::measure::Analysis>,
    pub(super) readings: Vec<f64>,
    own: bool,
}

/// One worker, one in-flight input copy. Waiting sources keep metadata only.
pub(super) struct RefMeasureWorker {
    thread: Worker<RefMeasureJob, RefMeasureResult>,
    pub(super) submitted: usize,
    own_turn: bool,
}

impl RefMeasureWorker {
    pub(super) fn open() -> Self {
        let thread = Worker::spawn("reference-measurements", measure_reference)
            .expect("start bounded reference measurement worker");
        Self { thread, submitted: 0, own_turn: true }
    }

    pub(super) fn busy(&self) -> bool {
        self.thread.busy()
    }

    pub(super) fn submit(&mut self, job: RefMeasureJob) {
        if self.thread.submit(job).is_ok() {
            self.submitted += 1;
        }
    }
}

fn measure_reference(job: RefMeasureJob) -> RefMeasureResult {
    let analysis = match &job.analysis {
        Some(a) => Arc::clone(a),
        None if job.key.basic => Arc::new(basic_analysis(crate::board::BasicStats::of(job.samples.slice()))),
        None => Arc::new(job.analyze()),
    };
    let readings = scalar_readings(&analysis, job.key);
    RefMeasureResult { source: job.source, key: job.key, analysis, readings, own: job.own }
}

impl Ui {
    pub(crate) fn set_measurement_full_record(&mut self, full: bool) {
        self.m.meas_full_record = full;
        if !full {
            for pending in &self.azr.ref_measure_pending {
                pending.set(None);
            }
        }
        self.clear_measurement_statistics();
        self.measure_last();
        self.mark_changed();
    }

    pub(in crate::ui) fn measurement_gate_label(&self) -> &'static str {
        if self.m.meas_full_record {
            "Full record"
        } else {
            "Screen"
        }
    }

    pub(in crate::ui) fn measurement_bounds(&self, len: usize, t0: f64, dt: f64) -> (usize, usize) {
        if self.m.meas_full_record {
            return (0, len);
        }
        let (scale, delay) = self.view();
        let half = scale as f64 * HDIV as f64 * 0.5;
        let index = |time: f64| {
            let fractional = (time - t0) / dt;
            let integer = fractional.round();
            // The view is f32: e.g. 200 ps/div expands an exact 100-sample
            // window by ~0.00000067 samples. Snap only this numerical fringe,
            // bounded to one hundred-thousandth of a sample, before ceil.
            let fractional = if (fractional - integer).abs() <= 1e-5 { integer } else { fractional };
            (fractional.ceil().max(0.0) as usize).min(len)
        };
        // Half-open physical time: left <= sample_time < right. Both ends use
        // ceil, so neither an earlier left sample nor the right border leaks in.
        let lo = index(delay as f64 - half);
        let hi = index(delay as f64 + half).max(lo);
        (lo, hi)
    }

    /// Whether `measurement_channel_samples` gives min/max pairs: the screen record of peak detect, or one resampled from
    /// a frame of pairs (roll, scan, the live compression). High res averages them.
    pub(in crate::ui) fn measurement_on_pairs(&self, rec: &Record, c: usize) -> bool {
        if self.m.acq == AcqMode::HiRes {
            return false;
        }
        match rec.raw.as_ref() {
            Some(raw) if !raw.frame.envelope && raw.frame.ch.get(c).is_some_and(|v| v.len() >= 2) => false,
            raw => rec.pairs || raw.is_some_and(|r| r.frame.envelope),
        }
    }

    /// The measurement gate is independent of replay routing, so live CH and R
    /// overlays use the same physical screen times at their own sample intervals.
    pub(in crate::ui) fn measurement_channel_samples<'a>(
        &self,
        rec: &'a Record,
        c: usize,
    ) -> Option<(&'a [f32], f64, f64)> {
        let screen = rec.ch.get(c)?;
        let (samples, t0, dt) = if let Some(raw) = rec.raw.as_ref().filter(|raw| {
            self.m.acq != AcqMode::HiRes && !raw.frame.envelope && raw.frame.ch.get(c).is_some_and(|v| v.len() >= 2)
        }) {
            (raw.frame.ch.get(c)?.as_slice(), raw.t0 - raw.i0 as f64 * raw.frame.dt, raw.frame.dt)
        } else {
            (screen.as_slice(), rec.t0, rec.dt)
        };
        let t0 = self.source_time_origin(WaveId::Ch(c), t0);
        if samples.len() < 2 || !dt.is_finite() || dt <= 0.0 {
            return None;
        }
        let (lo, hi) = self.measurement_bounds(samples.len(), t0, dt);
        (hi >= lo + 2).then_some((&samples[lo..hi], t0 + lo as f64 * dt, dt))
    }

    pub(super) fn reference_measurement_key(&self, slot: usize) -> Option<(RefMeasureKey, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !r.dt.is_finite() || r.dt <= 0.0 {
            return None;
        }
        let t0 = self.source_time_origin(WaveId::Ref(slot), r.t0);
        let (lo, hi) = self.measurement_bounds(r.v.len(), t0, r.dt);
        if hi < lo + 2 {
            return None;
        }
        let key = RefMeasureKey {
            samples: r.v.as_ptr() as usize,
            len: r.v.len(),
            lo,
            hi,
            dt: r.dt.to_bits(),
            t0: t0.to_bits(),
            vpd: r.vpd.to_bits(),
            low: self.m.meas_lpct.to_bits(),
            high: self.m.meas_hpct.to_bits(),
            epoch: self.workflow_epoch,
            revision: if self.session_is_sim && self.m.sim_slot == slot { self.replay.sim_revision } else { 0 },
            full: self.m.meas_full_record,
            basic: false,
            idle: self.m.meas_idle.to_bits(),
            outside: self.m.meas_out as u8,
            periods: self.m.meas_n,
            data: self.m.meas_data,
            conditioning: (0, false, 0),
            capture: 0,
            pairs: false,
        };
        Some((key, t0 + lo as f64 * r.dt))
    }

    pub(in crate::ui) fn reference_measurement(&self, slot: usize) -> Option<(Arc<oos_dsp::measure::Analysis>, f64)> {
        let (key, t0) = self.reference_measurement_key(slot)?;
        if let Some(cached) = self.azr.ref_measure[slot].borrow().as_ref().filter(|cached| cached.key == key) {
            return Some((Arc::clone(&cached.analysis), t0));
        }
        let r = self.refs[slot].as_ref()?;
        if key.hi - key.lo > BACKGROUND_MEASUREMENT_POINTS {
            self.azr.ref_measure_pending[slot].set(Some(key));
            let mut worker = self.azr.ref_measure_worker.borrow_mut();
            let worker = worker.get_or_insert_with(RefMeasureWorker::open);
            if !worker.busy() {
                let job = RefMeasureJob {
                    source: WaveId::Ref(slot),
                    key,
                    samples: MeasureSamples::Owned(r.v[key.lo..key.hi].to_vec()),
                    dt: r.dt,
                    min_amp: 0.4 * r.vpd as f64,
                    low: self.m.meas_lpct,
                    high: self.m.meas_hpct,
                    pairs: false,
                    own: false,
                    analysis: None,
                };
                worker.submit(job);
            }
            return None;
        }
        let mut cache = self.azr.ref_measure[slot].borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.key != key) {
            let analysis = oos_dsp::measure::analyze_refs(
                &r.v[key.lo..key.hi],
                r.dt,
                0.4 * r.vpd as f64,
                self.m.meas_lpct,
                self.m.meas_hpct,
            );
            let readings = scalar_readings(&analysis, key);
            *cache = Some(RefMeasureCache { key, analysis: Arc::new(analysis), readings });
        }
        Some((Arc::clone(&cache.as_ref()?.analysis), t0))
    }

    pub(in crate::ui) fn channel_basic_kind(kind: Meas) -> bool {
        matches!(kind, Meas::Vmin | Meas::Vmax | Meas::Vpp | Meas::Vavg | Meas::Vrms | Meas::StdDev | Meas::VrmsAc)
    }

    fn channel_needs_full_analysis(&self, channel: usize) -> bool {
        let id = WaveId::Ch(channel);
        self.m.meas.iter().any(|&(kind, source)| source == id && !Self::channel_basic_kind(kind))
            || self.m.meas2.iter().any(|&(_, a, b)| a == id || b == id)
            || (self.m.cnt_on && self.m.cnt_src == channel)
            || (self.m.view == AView::Spectrum && self.m.spec_src == id)
            || (self.m.pane == tables::PANE_SPECTRUM && self.m.spec_src == id)
    }

    pub(super) fn channel_measurement_key(&self, channel: usize, basic: bool) -> Option<(RefMeasureKey, f64)> {
        if channel >= n_ch() || !self.m.ch[channel].on {
            return None;
        }
        let record = self.last[0].as_ref()?;
        let (samples, t0, dt) = self.measurement_channel_samples(record, channel)?;
        Some((
            RefMeasureKey {
                samples: samples.as_ptr() as usize,
                len: samples.len(),
                lo: 0,
                hi: samples.len(),
                dt: dt.to_bits(),
                t0: t0.to_bits(),
                vpd: self.m.ch[channel].volts_per_div().to_bits(),
                low: self.m.meas_lpct.to_bits(),
                high: self.m.meas_hpct.to_bits(),
                epoch: self.workflow_epoch,
                revision: record.raw.as_ref().map_or(self.wfm_total, |raw| raw.frame.seq),
                full: self.m.meas_full_record,
                basic,
                idle: self.m.meas_idle.to_bits(),
                outside: self.m.meas_out as u8,
                periods: self.m.meas_n,
                data: self.m.meas_data,
                conditioning: (self.m.ch[channel].coupling as u8, self.m.ch[channel].invert, self.m.ch[channel].bw),
                capture: self.wfm_total,
                pairs: self.measurement_on_pairs(record, channel),
            },
            t0,
        ))
    }

    fn native_channel_basic(&self, channel: usize) -> Option<crate::board::BasicStats> {
        if channel >= n_ch() || !self.m.ch[channel].on {
            return None;
        }
        let record = self.last[0].as_ref()?;
        let raw = record.raw.as_ref()?;
        let full = raw.frame.ch.get(channel)?;
        let (samples, _, _) = self.measurement_channel_samples(record, channel)?;
        let basic = raw.frame.basic[channel];
        (basic.valid && samples.len() == full.len() && samples.as_ptr() == full.as_ptr()).then_some(basic)
    }

    pub(in crate::ui) fn channel_basic_reading(&self, channel: usize, kind: Meas) -> Option<f64> {
        if !Self::channel_basic_kind(kind) {
            return None;
        }
        let kind = if kind == Meas::VrmsAc { Meas::StdDev } else { kind };
        if let Some(basic) = self.native_channel_basic(channel) {
            return basic.reading(kind);
        }
        self.channel_measurement(channel, false).map(|(analysis, _)| analysis.get(kind))
    }

    /// Shared with reference jobs: one worker and one retained native frame.
    /// Large CH gates submit an Arc plus indices, never a full-record UI copy.
    pub(in crate::ui) fn channel_measurement(
        &self,
        channel: usize,
        full_analysis: bool,
    ) -> Option<(Arc<oos_dsp::measure::Analysis>, f64)> {
        let (mut key, t0) = self.channel_measurement_key(channel, !full_analysis)?;
        let large = key.len > BACKGROUND_MEASUREMENT_POINTS;
        if !large || self.channel_needs_full_analysis(channel) {
            key.basic = false;
        }
        // A scalar reader must not downgrade an outstanding explicit edge query.
        if let Some(pending) = self.azr.ch_measure_pending[channel].get() {
            let mut comparable = key;
            comparable.basic = pending.basic;
            if comparable == pending && !pending.basic {
                key.basic = false;
            }
        }
        if let Some(cache) = self.azr.ch_measure[channel].borrow().as_ref() {
            let mut comparable = key;
            comparable.basic = cache.key.basic;
            if cache.key == key || (key.basic && !cache.key.basic && comparable == cache.key) {
                return Some((Arc::clone(&cache.analysis), t0));
            }
        }
        if key.basic {
            if let Some(basic) = self.native_channel_basic(channel) {
                let analysis = Arc::new(basic_analysis(basic));
                let readings = scalar_readings(&analysis, key);
                *self.azr.ch_measure[channel].borrow_mut() =
                    Some(RefMeasureCache { key, analysis: Arc::clone(&analysis), readings });
                return Some((analysis, t0));
            }
        }
        let record = self.last[0].as_ref()?;
        let (samples, _, dt) = self.measurement_channel_samples(record, channel)?;
        if large {
            self.azr.ch_measure_pending[channel].set(Some(key));
            let mut worker = self.azr.ref_measure_worker.borrow_mut();
            let worker = worker.get_or_insert_with(RefMeasureWorker::open);
            if !worker.busy() {
                let input = if let Some(raw) = &record.raw {
                    let values = &raw.frame.ch[channel];
                    let offset = (samples.as_ptr() as usize).checked_sub(values.as_ptr() as usize)?;
                    let lo = offset / std::mem::size_of::<f32>();
                    if lo.checked_add(samples.len())? > values.len() {
                        return None;
                    }
                    MeasureSamples::Native { frame: Arc::clone(&raw.frame), channel, lo, hi: lo + samples.len() }
                } else {
                    MeasureSamples::Owned(samples.to_vec())
                };
                let job = RefMeasureJob {
                    source: WaveId::Ch(channel),
                    key,
                    samples: input,
                    dt,
                    min_amp: 0.4 * self.m.ch[channel].volts_per_div() as f64,
                    low: self.m.meas_lpct,
                    high: self.m.meas_hpct,
                    pairs: key.pairs,
                    own: false,
                    analysis: None,
                };
                worker.submit(job);
            }
            return None;
        }
        let analysis = Arc::new(analyze_samples(
            samples,
            dt,
            0.4 * self.m.ch[channel].volts_per_div() as f64,
            self.m.meas_lpct,
            self.m.meas_hpct,
            key.pairs,
        ));
        let readings = scalar_readings(&analysis, key);
        *self.azr.ch_measure[channel].borrow_mut() =
            Some(RefMeasureCache { key, analysis: Arc::clone(&analysis), readings });
        Some((analysis, t0))
    }

    pub(in crate::ui) fn measure_channel_sources(&mut self) {
        for channel in 0..NCH {
            let full = self.channel_needs_full_analysis(channel);
            let analysis = self.channel_measurement(channel, full).map(|(analysis, _)| analysis);
            // a large record measured in the background: the values of the last one stay until the worker answers (a
            // deep record measured raw once a second showed "not available" for that time, unit 2026-11-15)
            if analysis.is_none()
                && self.m.ch[channel].on
                && self.computing_like_last(channel)
                && self.ana[channel].valid
            {
                continue;
            }
            let basic = self.native_channel_basic(channel);
            let stats = analysis.as_ref().map(|a| Stats {
                valid: a.valid,
                vmin: a.vmin as f32,
                vmax: a.vmax as f32,
                top: a.top as f32,
                base: a.base as f32,
                period: a.get(Meas::Period),
                pwidth: self.cached_measurement_reading(WaveId::Ch(channel), Meas::PWidth, a).unwrap_or(f64::NAN),
                rise: a.rise_time,
                fall: a.fall_time,
            });
            self.stats[channel] = stats
                .or_else(|| {
                    basic.map(|b| Stats {
                        valid: b.valid,
                        vmin: b.values[0] as f32,
                        vmax: b.values[1] as f32,
                        ..Stats::default()
                    })
                })
                .unwrap_or_default();
            self.ana[channel] = analysis.unwrap_or_default();
            if !self.m.ch[channel].on {
                *self.azr.ch_measure[channel].borrow_mut() = None;
                self.azr.ch_measure_pending[channel].set(None);
            }
        }
    }

    pub(in crate::ui) fn cached_measurement_reading(
        &self,
        id: WaveId,
        kind: Meas,
        analysis: &Arc<oos_dsp::measure::Analysis>,
    ) -> Option<f64> {
        let cache = match id {
            WaveId::Ch(c) => self.azr.ch_measure.get(c)?,
            WaveId::Ref(i) => self.azr.ref_measure.get(i)?,
        }
        .borrow();
        let cache = cache.as_ref()?;
        if !Arc::ptr_eq(&cache.analysis, analysis) || (cache.key.basic && !Self::channel_basic_kind(kind)) {
            return None;
        }
        let index = ALL_MEAS.iter().position(|&entry| entry == kind)?;
        cache.readings.get(index).copied()
    }

    pub(in crate::ui) fn measurement_identity(&self, id: WaveId) -> Option<(RefMeasureKey, usize)> {
        let (cache, current) = match id {
            WaveId::Ch(c) => (self.azr.ch_measure.get(c)?, self.channel_measurement_key(c, false)),
            WaveId::Ref(i) => (self.azr.ref_measure.get(i)?, self.reference_measurement_key(i)),
        };
        let cache = cache.borrow();
        let cache = cache.as_ref()?;
        (current.is_some_and(|(key, _)| key == cache.key) && !cache.key.basic)
            .then_some((cache.key, Arc::as_ptr(&cache.analysis) as usize))
    }

    /// A large record of channel `channel` is measured in the background, and the last finished measurement had the
    /// same settings (points, sample spacing, start time, V/div, levels): its value may stand in until the new one is
    /// there. After a change of settings it may not: the old value would read as the new one (unit 2026-11-15, the
    /// frequency of the time base before in roll mode).
    pub(in crate::ui) fn computing_like_last(&self, channel: usize) -> bool {
        if channel >= NCH || !self.measurement_computing(WaveId::Ch(channel)) {
            return false;
        }
        // only for the raw window of a deep record (deepmeas.rs), read once in a while: where records come faster
        // than the worker (roll at 0.2 to 0.5 s/div) the last finished value could be from before a change of the
        // signal and stood for seconds (time_axis_at_every_channel_count on the unit, 2026-11-15)
        if !self.last[0]
            .as_ref()
            .and_then(|r| r.raw.as_ref())
            .is_some_and(|r| crate::deepmeas::deep_raw_frame(&r.frame))
        {
            return false;
        }
        let Some(pending) = self.azr.ch_measure_pending[channel].get() else { return false };
        let cache = self.azr.ch_measure[channel].borrow();
        cache.as_ref().is_some_and(|c| {
            let k = c.key;
            (k.len, k.dt, k.t0, k.vpd, k.low, k.high, k.epoch, k.full)
                == (
                    pending.len,
                    pending.dt,
                    pending.t0,
                    pending.vpd,
                    pending.low,
                    pending.high,
                    pending.epoch,
                    pending.full,
                )
        })
    }

    pub(in crate::ui) fn measurement_computing(&self, id: WaveId) -> bool {
        if let WaveId::Ch(channel) = id {
            return channel < NCH
                && self.azr.ch_measure_pending[channel].get().is_some_and(|key| {
                    self.channel_measurement_key(channel, key.basic).is_some_and(|(current, _)| current == key)
                });
        }
        let WaveId::Ref(slot) = id else { unreachable!() };
        let Some((key, _)) = self.reference_measurement_key(slot) else {
            return false;
        };
        key.hi - key.lo > BACKGROUND_MEASUREMENT_POINTS
            && self.azr.ref_measure[slot].borrow().as_ref().is_none_or(|cached| cached.key != key)
    }

    pub(crate) fn measurement_busy(&self) -> bool {
        self.catalog_measurement_busy()
            || self.own_measurement_busy()
            || self.azr.ch_measure_pending.iter().enumerate().any(|(channel, pending)| {
                pending.get().is_some_and(|key| {
                    self.channel_measurement_key(channel, key.basic).is_some_and(|(current, _)| current == key)
                })
            })
            || self.pair_measurement_busy()
            || self.azr.ref_measure_pending.iter().enumerate().any(|(slot, pending)| {
                pending
                    .get()
                    .is_some_and(|key| self.reference_measurement_key(slot).is_some_and(|(current, _)| current == key))
            })
    }

    pub(in crate::ui) fn poll_reference_measurements(&mut self) {
        let mut completed = Vec::new();
        if let Some(worker) = self.azr.ref_measure_worker.borrow_mut().as_mut() {
            while let Ok(result) = worker.thread.poll() {
                worker.own_turn = !result.own;
                completed.push(result);
            }
        }
        let mut changed = false;
        for result in completed {
            if result.own {
                changed |= self.commit_own_measurement(result);
                continue;
            }
            let (pending, cache, current) = match result.source {
                WaveId::Ref(slot) => (
                    &self.azr.ref_measure_pending[slot],
                    &self.azr.ref_measure[slot],
                    self.reference_measurement_key(slot),
                ),
                WaveId::Ch(channel) => (
                    &self.azr.ch_measure_pending[channel],
                    &self.azr.ch_measure[channel],
                    self.channel_measurement_key(channel, result.key.basic),
                ),
            };
            if pending.get() == Some(result.key) {
                pending.set(None);
                if current.is_some_and(|(current, _)| current == result.key) {
                    *cache.borrow_mut() =
                        Some(RefMeasureCache { key: result.key, analysis: result.analysis, readings: result.readings });
                    changed = true;
                }
            }
        }
        let own_turn = self.azr.ref_measure_worker.borrow().as_ref().is_some_and(|worker| worker.own_turn);
        if own_turn {
            self.schedule_own_measurement();
        }
        for channel in 0..NCH {
            if let Some(key) = self.azr.ch_measure_pending[channel].get() {
                if self.channel_measurement_key(channel, key.basic).is_some_and(|(current, _)| current == key) {
                    let _ = self.channel_measurement(channel, !key.basic);
                } else {
                    self.azr.ch_measure_pending[channel].set(None);
                }
            }
        }
        for slot in 0..NREF {
            if let Some(key) = self.azr.ref_measure_pending[slot].get() {
                if self.reference_measurement_key(slot).is_some_and(|(current, _)| current == key) {
                    let _ = self.reference_measurement(slot);
                } else {
                    self.azr.ref_measure_pending[slot].set(None);
                }
            }
        }
        self.schedule_own_measurement();
        if changed {
            self.measure_channel_sources();
            self.measure_refs();
            self.finish_deferred_capture_statistics();
            self.dirty_map = true;
        }
        self.poll_pair_measurements();
    }

    pub(in crate::ui) fn measure_reference_sources(&mut self) {
        let meter = if self.m.dvm_on || self.m.cnt_on { self.offline_meter() } else { None };
        for i in 0..NREF {
            let id = WaveId::Ref(i);
            let requested = self.refs[i].as_ref().is_some_and(|r| r.on)
                || self.m.meas.iter().any(|entry| entry.1 == id)
                || self.m.meas2.iter().any(|entry| entry.1 == id || entry.2 == id)
                || meter == Some(i);
            let analysis = requested.then(|| self.reference_measurement(i)).flatten();
            self.ref_ana_valid[i] = analysis.is_some();
            self.ref_ana[i] = analysis.map_or_else(Default::default, |(a, _)| a);
            if self.refs[i].is_none() {
                *self.azr.ref_measure[i].borrow_mut() = None;
            }
        }
    }

    pub(in crate::ui) fn refresh_stopped_measurements(&mut self) {
        let (scale, delay) = self.view();
        let key = (scale.to_bits(), delay.to_bits(), self.m.meas_full_record);
        if self.azr.measure_view == Some(key) {
            return;
        }
        self.azr.measure_view = Some(key);
        self.measure_last();
    }
}
