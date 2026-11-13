// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The analysers in the plot area: spectrum analyser (RBW, trace modes, peak markers, waterfall,
//! THD/SNR/SFDR), eye diagram with jitter (eyepage.rs), power analysis, and Identify (names the signals and applies a
//! trigger and decoder). The maths is in oos-dsp (`spectrum`, `eye`, `power`, `identify`); this file holds the menus,
//! the per-record update on every captured sample (`Ui::meas_wave`) and the drawing.

use super::magnetic::missing_wave;
use super::*;
use oos_dsp::{eye, identify, power, spectrum};
use std::{cell::{Cell, RefCell}, sync::{Arc, mpsc}};

const BACKGROUND_MEASUREMENT_POINTS: usize = 262_144;
#[path = "measure_own.rs"]
mod own;
#[cfg(test)]
#[path = "channel_measurement_tests.rs"]
mod channel_measurement_tests;

/// A reference may keep the same allocation between accepted SIM captures.
/// Include its revision as well as the physical gate and analysis settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct RefMeasureKey {
    samples: usize,
    len: usize,
    lo: usize,
    hi: usize,
    dt: u64,
    t0: u64,
    vpd: u32,
    low: u64,
    high: u64,
    epoch: u64,
    revision: u64,
    full: bool,
    basic: bool,
    idle: u64,
    outside: u8,
    periods: u32,
    data: bool,
    conditioning: (u8, bool, usize),
    capture: u64,
}

struct RefMeasureCache {
    key: RefMeasureKey,
    analysis: Arc<oos_dsp::measure::Analysis>,
    readings: Vec<f64>,
}

struct RefMeasureJob {
    source: WaveId,
    key: RefMeasureKey,
    samples: MeasureSamples,
    dt: f64,
    min_amp: f64,
    low: f64,
    high: f64,
    own: bool,
    analysis: Option<Arc<oos_dsp::measure::Analysis>>,
}

enum MeasureSamples {
    Owned(Vec<f32>),
    Native { frame: Arc<crate::board::Frame>, channel: usize, lo: usize, hi: usize },
}

impl MeasureSamples {
    fn slice(&self) -> &[f32] {
        match self { Self::Owned(v) => v, Self::Native { frame, channel, lo, hi } => &frame.ch[*channel][*lo..*hi] }
    }
}

fn basic_analysis(basic: crate::board::BasicStats) -> oos_dsp::measure::Analysis {
    let [vmin, vmax, _, avg, rms, std] = basic.values;
    oos_dsp::measure::Analysis { valid: basic.valid, vmin, vmax, avg, rms, std,
        rise_time: f64::NAN, fall_time: f64::NAN, t_vmax: f64::NAN, t_vmin: f64::NAN,
        ..Default::default() }
}

fn scalar_readings(analysis: &oos_dsp::measure::Analysis, key: RefMeasureKey) -> Vec<f64> {
    use oos_dsp::measure::{burst_width, duration_n, time_outside, Outside};
    let outside = match key.outside { 0 => Outside::Above, 1 => Outside::Below, _ => Outside::Both };
    ALL_MEAS.iter().map(|&kind| match kind {
        Meas::Burst => burst_width(analysis, f64::from_bits(key.idle)),
        Meas::Outside => time_outside(analysis, outside),
        Meas::NPeriods => duration_n(analysis, key.periods.max(1) as usize, key.data),
        _ => analysis.get(kind),
    }).collect()
}

struct RefMeasureResult { source: WaveId, key: RefMeasureKey, analysis: Arc<oos_dsp::measure::Analysis>, readings: Vec<f64>, own: bool }

/// One worker, one in-flight input copy. Waiting sources keep metadata only.
struct RefMeasureWorker {
    input: mpsc::SyncSender<RefMeasureJob>,
    output: mpsc::Receiver<RefMeasureResult>,
    busy: bool,
    submitted: usize,
    own_turn: bool,
}

impl RefMeasureWorker {
    fn open() -> Self {
        let (input, requests) = mpsc::sync_channel::<RefMeasureJob>(1);
        let (results, output) = mpsc::channel();
        std::thread::Builder::new().name("reference-measurements".into()).spawn(move || {
            while let Ok(job) = requests.recv() {
                let analysis = job.analysis.unwrap_or_else(|| Arc::new(if job.key.basic { basic_analysis(crate::board::BasicStats::of(job.samples.slice())) }
                    else { oos_dsp::measure::analyze_refs(job.samples.slice(), job.dt, job.min_amp, job.low, job.high) }));
                let readings = scalar_readings(&analysis, job.key);
                if results.send(RefMeasureResult { source: job.source, key: job.key, analysis, readings, own: job.own }).is_err() { break; }
            }
        }).expect("start bounded reference measurement worker");
        Self { input, output, busy: false, submitted: 0, own_turn: true }
    }
}

/// Analyser state kept between records.
#[derive(Default)]
pub struct State {
    /// The shared worker survives replay resets and additive imports.
    pub(super) reference_search: RefCell<super::reference_search::Cache>,
    pub(super) reference_decode: RefCell<super::reference_decode::Cache>,
    pub(super) measure_pairs: RefCell<super::measure_pairs::State>,
    /// Shared analyses avoid rescanning or copying a long record for each readout.
    ref_measure: [RefCell<Option<RefMeasureCache>>; NREF],
    ref_measure_pending: [Cell<Option<RefMeasureKey>>; NREF],
    ch_measure: [RefCell<Option<RefMeasureCache>>; NCH],
    ch_measure_pending: [Cell<Option<RefMeasureKey>>; NCH],
    ref_measure_worker: RefCell<Option<RefMeasureWorker>>,
    own_measure: RefCell<own::State>,
    measure_view: Option<(u32, u32, bool)>,
    pub spec: Option<spectrum::Spectrum>,
    pub spec_avg: Option<spectrum::Avg>,
    pub wf: Option<spectrum::Waterfall>,
    pub dist: Option<spectrum::Distortion>,
    /// the lines for table and markers, in the order of the table
    pub peaks: Vec<spectrum::Marker>,
    /// top of the band the pane follows (0 = none yet), and when the Results pane last had a spectrum
    pub spec_stop: f64,
    pub spec_t: f64,
    /// the limit line, and of the traces since it was set: how many, how many went over, the worst bin
    pub limit: Option<spectrum::Limit>,
    pub limit_n: u64,
    pub limit_over: u64,
    pub limit_worst: Option<(f64, f32)>,
    pub eye: Option<eye::Eye>,
    pub(super) eye_auto: super::eyepage::automatic::State,
    /// what the eye was built for (source, clock, scale, offset, rate, raster size)
    pub eye_key: (u8, usize, u8, usize, u32, u32, u64, i32, i32),
    /// the ref record already folded in, so a second pass does not count it again
    pub eye_ref: Option<(usize, u64)>,
    pub eye_sim_revision: Option<(usize, u64)>,
    pub eye_res: Option<(eye::EyeMeas, eye::Jitter, f64)>,
    pub page: super::eyepage::Page,
    pub power: Vec<String>,
    pub mag: Option<oos_dsp::magnetic::Magnetic>,
    pub mag_note: String,
    pub dq: Option<oos_dsp::motor::Dq0>,
    pub mech: Option<oos_dsp::motor::Mech>,
    pub inv_note: String,
    pub dpt: Option<oos_dsp::dpt::Pulse>,
    /// E on / E off from `power::switching`, printed beside the double-pulse figures
    pub dpt_loss: Option<(f64, f64)>,
    pub dpt_note: String,
    /// min/max of Vds, Id, Vgs inside the zoom window
    pub dpt_t: (f64, f64),
    pub dpt_v: Vec<(f32, f32)>,
    pub dpt_i: Vec<(f32, f32)>,
    pub dpt_g: Vec<(f32, f32)>,
    pub findings: Vec<identify::Finding>,
    /// protocol watch: on/off, the bus last named per channel group, time of the last check
    pub watch: bool,
    pub watch_seen: Vec<String>,
    pub watch_t: f64,
    /// bandwidth test (bwtest.rs): result, info line, time of the last update
    pub bw: Option<oos_dsp::bandwidth::StepResponse>,
    pub bw_info: String,
    pub bw_t: f64,
}

/// A bus a finding names ("UART 115200 on CH3"), None for plain signals.
fn bus_name(f: &identify::Finding) -> Option<String> {
    use identify::SignalKind as K;
    let chs = f.channels.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join("+");
    let what = match &f.kind {
        K::Uart { baud, .. } => format!("UART {:.0} Bd", baud),
        K::I2c { .. } => "I2C".to_string(),
        K::Spi { mode, .. } => format!("SPI mode {}", mode),
        K::Can { bitrate, .. } => format!("CAN {}", si_short(*bitrate, "b/s")),
        K::Lin { baud } => format!("LIN {:.0} Bd", baud),
        K::OneWire => "1-Wire".to_string(),
        K::Manchester { bitrate } => format!("Manchester {}", si_short(*bitrate, "b/s")),
        K::Video { .. } => "video".to_string(),
        _ => return None,
    };
    Some(format!("{} on {}", what, chs))
}

const TRACES: [&str; 4] = ["Normal", "Average", "Max hold", "Min hold"];
const POWER_MODES: [PowerMode; 6] = [PowerMode::Quality, PowerMode::Harmonics, PowerMode::Switching, PowerMode::Ripple, PowerMode::Capacitance, PowerMode::Rail];

fn power_page(p: PowerMode) -> &'static str {
    match p {
        PowerMode::Quality => "Quality",
        PowerMode::Harmonics => "Harmonics",
        PowerMode::Switching => "Switching",
        PowerMode::Ripple => "Ripple",
        PowerMode::Capacitance => "Cin",
        PowerMode::Rail => "Rail",
    }
}
const FIND_LABELS: [&str; 8] = ["1", "2", "3", "4", "5", "6", "7", "8"];

/// 1-2-5 frequency ladder 1 Hz .. 1 GHz (0 = full span / auto RBW in front).
fn freq_ladder() -> Vec<f64> {
    let mut v = vec![0.0];
    let mut d = 1.0;
    while d < 2e9 {
        v.extend([d, 2.0 * d, 5.0 * d]);
        d *= 10.0;
    }
    v
}

fn step_ladder(v: f64, d: i32) -> f64 {
    let l = freq_ladder();
    let i = l.iter().position(|&x| x >= v * 0.999).unwrap_or(0);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}

impl Ui {
    pub(crate) fn set_measurement_full_record(&mut self, full: bool) {
        self.m.meas_full_record = full;
        if !full { for pending in &self.azr.ref_measure_pending { pending.set(None); } }
        self.mstat.clear();
        self.measure_last();
        self.mark_changed();
    }

    pub(super) fn measurement_gate_label(&self) -> &'static str {
        if self.m.meas_full_record { "Full record" } else { "Screen" }
    }

    fn measurement_bounds(&self, len: usize, t0: f64, dt: f64) -> (usize, usize) {
        if self.m.meas_full_record { return (0, len); }
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

    /// The measurement gate is independent of replay routing, so live CH and R
    /// overlays use the same physical screen times at their own sample intervals.
    pub(super) fn measurement_channel_samples<'a>(&self, rec: &'a Record, c: usize) -> Option<(&'a [f32], f64, f64)> {
        let screen = rec.ch.get(c)?;
        let (samples, t0, dt) = if let Some(raw) = rec.raw.as_ref().filter(|raw| self.m.acq != AcqMode::HiRes
            && !raw.frame.envelope && raw.frame.ch.get(c).is_some_and(|v| v.len() >= 2)) {
            (raw.frame.ch.get(c)?.as_slice(), raw.t0 - raw.i0 as f64 * raw.frame.dt, raw.frame.dt)
        } else { (screen.as_slice(), rec.t0, rec.dt) };
        let t0 = self.source_time_origin(WaveId::Ch(c), t0);
        if samples.len() < 2 || !dt.is_finite() || dt <= 0.0 { return None; }
        let (lo, hi) = self.measurement_bounds(samples.len(), t0, dt);
        (hi >= lo + 2).then_some((&samples[lo..hi], t0 + lo as f64 * dt, dt))
    }

    fn reference_measurement_key(&self, slot: usize) -> Option<(RefMeasureKey, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !r.dt.is_finite() || r.dt <= 0.0 { return None; }
        let t0 = self.source_time_origin(WaveId::Ref(slot), r.t0);
        let (lo, hi) = self.measurement_bounds(r.v.len(), t0, r.dt);
        if hi < lo + 2 { return None; }
        let key = RefMeasureKey { samples: r.v.as_ptr() as usize, len: r.v.len(), lo, hi,
            dt: r.dt.to_bits(), t0: t0.to_bits(), vpd: r.vpd.to_bits(),
            low: self.m.meas_lpct.to_bits(), high: self.m.meas_hpct.to_bits(), epoch: self.workflow_epoch,
            revision: if self.session_is_sim && self.m.sim_slot == slot { self.replay.sim_revision } else { 0 }, full: self.m.meas_full_record, basic: false,
            idle: self.m.meas_idle.to_bits(), outside: self.m.meas_out as u8, periods: self.m.meas_n, data: self.m.meas_data,
            conditioning: (0, false, 0), capture: 0 };
        Some((key, t0 + lo as f64 * r.dt))
    }

    pub(super) fn reference_measurement(&self, slot: usize) -> Option<(Arc<oos_dsp::measure::Analysis>, f64)> {
        let (key, t0) = self.reference_measurement_key(slot)?;
        if let Some(cached) = self.azr.ref_measure[slot].borrow().as_ref().filter(|cached| cached.key == key) {
            return Some((Arc::clone(&cached.analysis), t0));
        }
        let r = self.refs[slot].as_ref()?;
        if key.hi - key.lo > BACKGROUND_MEASUREMENT_POINTS {
            self.azr.ref_measure_pending[slot].set(Some(key));
            let mut worker = self.azr.ref_measure_worker.borrow_mut();
            let worker = worker.get_or_insert_with(RefMeasureWorker::open);
            if !worker.busy {
                let job = RefMeasureJob { source: WaveId::Ref(slot), key, samples: MeasureSamples::Owned(r.v[key.lo..key.hi].to_vec()), dt: r.dt,
                    min_amp: 0.4 * r.vpd as f64, low: self.m.meas_lpct, high: self.m.meas_hpct, own: false, analysis: None };
                if worker.input.try_send(job).is_ok() { worker.busy = true; worker.submitted += 1; }
            }
            return None;
        }
        let mut cache = self.azr.ref_measure[slot].borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.key != key) {
            let analysis = oos_dsp::measure::analyze_refs(&r.v[key.lo..key.hi], r.dt,
                0.4 * r.vpd as f64, self.m.meas_lpct, self.m.meas_hpct);
            let readings = scalar_readings(&analysis, key);
            *cache = Some(RefMeasureCache { key, analysis: Arc::new(analysis), readings });
        }
        Some((Arc::clone(&cache.as_ref()?.analysis), t0))
    }

    pub(super) fn channel_basic_kind(kind: Meas) -> bool {
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

    fn channel_measurement_key(&self, channel: usize, basic: bool) -> Option<(RefMeasureKey, f64)> {
        if channel >= n_ch() || !self.m.ch[channel].on { return None; }
        let record = self.last[0].as_ref()?;
        let (samples, t0, dt) = self.measurement_channel_samples(record, channel)?;
        Some((RefMeasureKey { samples: samples.as_ptr() as usize, len: samples.len(), lo: 0, hi: samples.len(),
            dt: dt.to_bits(), t0: t0.to_bits(), vpd: self.m.ch[channel].volts_per_div().to_bits(),
            low: self.m.meas_lpct.to_bits(), high: self.m.meas_hpct.to_bits(), epoch: self.workflow_epoch,
            revision: record.raw.as_ref().map_or(self.wfm_total, |raw| raw.frame.seq), full: self.m.meas_full_record, basic,
            idle: self.m.meas_idle.to_bits(), outside: self.m.meas_out as u8, periods: self.m.meas_n, data: self.m.meas_data,
            conditioning: (self.m.ch[channel].coupling as u8, self.m.ch[channel].invert, self.m.ch[channel].bw),
            capture: self.wfm_total }, t0))
    }

    fn native_channel_basic(&self, channel: usize) -> Option<crate::board::BasicStats> {
        if channel >= n_ch() || !self.m.ch[channel].on { return None; }
        let record = self.last[0].as_ref()?;
        let raw = record.raw.as_ref()?;
        let full = raw.frame.ch.get(channel)?;
        let (samples, _, _) = self.measurement_channel_samples(record, channel)?;
        let basic = raw.frame.basic[channel];
        (basic.valid && samples.len() == full.len() && samples.as_ptr() == full.as_ptr()).then_some(basic)
    }

    pub(super) fn channel_basic_reading(&self, channel: usize, kind: Meas) -> Option<f64> {
        if !Self::channel_basic_kind(kind) { return None; }
        let kind = if kind == Meas::VrmsAc { Meas::StdDev } else { kind };
        if let Some(basic) = self.native_channel_basic(channel) { return basic.reading(kind); }
        self.channel_measurement(channel, false).map(|(analysis, _)| analysis.get(kind))
    }

    /// Shared with reference jobs: one worker and one retained native frame.
    /// Large CH gates submit an Arc plus indices, never a full-record UI copy.
    pub(super) fn channel_measurement(&self, channel: usize, full_analysis: bool) -> Option<(Arc<oos_dsp::measure::Analysis>, f64)> {
        let (mut key, t0) = self.channel_measurement_key(channel, !full_analysis)?;
        let large = key.len > BACKGROUND_MEASUREMENT_POINTS;
        if !large || self.channel_needs_full_analysis(channel) { key.basic = false; }
        // A scalar reader must not downgrade an outstanding explicit edge query.
        if let Some(pending) = self.azr.ch_measure_pending[channel].get() {
            let mut comparable = key; comparable.basic = pending.basic;
            if comparable == pending && !pending.basic { key.basic = false; }
        }
        if let Some(cache) = self.azr.ch_measure[channel].borrow().as_ref() {
            let mut comparable = key; comparable.basic = cache.key.basic;
            if cache.key == key || (key.basic && !cache.key.basic && comparable == cache.key) {
                return Some((Arc::clone(&cache.analysis), t0));
            }
        }
        if key.basic {
            if let Some(basic) = self.native_channel_basic(channel) {
                let analysis = Arc::new(basic_analysis(basic));
                let readings = scalar_readings(&analysis, key);
                *self.azr.ch_measure[channel].borrow_mut() = Some(RefMeasureCache { key, analysis: Arc::clone(&analysis), readings });
                return Some((analysis, t0));
            }
        }
        let record = self.last[0].as_ref()?;
        let (samples, _, dt) = self.measurement_channel_samples(record, channel)?;
        if large {
            self.azr.ch_measure_pending[channel].set(Some(key));
            let mut worker = self.azr.ref_measure_worker.borrow_mut();
            let worker = worker.get_or_insert_with(RefMeasureWorker::open);
            if !worker.busy {
                let input = if let Some(raw) = &record.raw {
                    let values = &raw.frame.ch[channel];
                    let offset = (samples.as_ptr() as usize).checked_sub(values.as_ptr() as usize)?;
                    let lo = offset / std::mem::size_of::<f32>();
                    if lo.checked_add(samples.len())? > values.len() { return None; }
                    MeasureSamples::Native { frame: Arc::clone(&raw.frame), channel, lo, hi: lo + samples.len() }
                } else { MeasureSamples::Owned(samples.to_vec()) };
                let job = RefMeasureJob { source: WaveId::Ch(channel), key, samples: input, dt,
                    min_amp: 0.4 * self.m.ch[channel].volts_per_div() as f64, low: self.m.meas_lpct, high: self.m.meas_hpct, own: false, analysis: None };
                if worker.input.try_send(job).is_ok() { worker.busy = true; worker.submitted += 1; }
            }
            return None;
        }
        let analysis = Arc::new(oos_dsp::measure::analyze_refs(samples, dt, 0.4 * self.m.ch[channel].volts_per_div() as f64,
            self.m.meas_lpct, self.m.meas_hpct));
        let readings = scalar_readings(&analysis, key);
        *self.azr.ch_measure[channel].borrow_mut() = Some(RefMeasureCache { key, analysis: Arc::clone(&analysis), readings });
        Some((analysis, t0))
    }

    pub(super) fn measure_channel_sources(&mut self) {
        for channel in 0..NCH {
            let full = self.channel_needs_full_analysis(channel);
            let analysis = self.channel_measurement(channel, full).map(|(analysis, _)| analysis);
            let basic = self.native_channel_basic(channel);
            let stats = analysis.as_ref().map(|a| Stats { valid: a.valid, vmin: a.vmin as f32, vmax: a.vmax as f32,
                vavg: a.avg as f32, vrms: a.rms as f32, top: a.top as f32, base: a.base as f32,
                period: a.get(Meas::Period), pwidth: self.cached_measurement_reading(WaveId::Ch(channel), Meas::PWidth, a).unwrap_or(f64::NAN), rise: a.rise_time, fall: a.fall_time });
            self.stats[channel] = stats.or_else(|| basic.map(|b| Stats { valid: b.valid, vmin: b.values[0] as f32,
                vmax: b.values[1] as f32, vavg: b.values[3] as f32, vrms: b.values[4] as f32, ..Stats::default() })).unwrap_or_default();
            self.ana[channel] = analysis.unwrap_or_default();
            if !self.m.ch[channel].on {
                *self.azr.ch_measure[channel].borrow_mut() = None;
                self.azr.ch_measure_pending[channel].set(None);
            }
        }
    }

    pub(super) fn cached_measurement_reading(&self, id: WaveId, kind: Meas, analysis: &Arc<oos_dsp::measure::Analysis>) -> Option<f64> {
        let cache = match id { WaveId::Ch(c) => self.azr.ch_measure.get(c)?, WaveId::Ref(i) => self.azr.ref_measure.get(i)? }.borrow();
        let cache = cache.as_ref()?;
        if !Arc::ptr_eq(&cache.analysis, analysis) || (cache.key.basic && !Self::channel_basic_kind(kind)) { return None; }
        let index = ALL_MEAS.iter().position(|&entry| entry == kind)?;
        cache.readings.get(index).copied()
    }

    pub(super) fn measurement_identity(&self, id: WaveId) -> Option<(RefMeasureKey, usize)> {
        let (cache, current) = match id {
            WaveId::Ch(c) => (self.azr.ch_measure.get(c)?, self.channel_measurement_key(c, false)),
            WaveId::Ref(i) => (self.azr.ref_measure.get(i)?, self.reference_measurement_key(i)),
        };
        let cache = cache.borrow(); let cache = cache.as_ref()?;
        (current.is_some_and(|(key, _)| key == cache.key) && !cache.key.basic)
            .then_some((cache.key, Arc::as_ptr(&cache.analysis) as usize))
    }

    pub(super) fn measurement_computing(&self, id: WaveId) -> bool {
        if let WaveId::Ch(channel) = id {
            return channel < NCH && self.azr.ch_measure_pending[channel].get().is_some_and(|key|
                self.channel_measurement_key(channel, key.basic).is_some_and(|(current, _)| current == key));
        }
        let WaveId::Ref(slot) = id else { unreachable!() };
        let Some((key, _)) = self.reference_measurement_key(slot) else { return false; };
        key.hi - key.lo > BACKGROUND_MEASUREMENT_POINTS
            && self.azr.ref_measure[slot].borrow().as_ref().is_none_or(|cached| cached.key != key)
    }

    pub(crate) fn measurement_busy(&self) -> bool {
        self.own_measurement_busy() || self.azr.ch_measure_pending.iter().enumerate().any(|(channel, pending)| pending.get().is_some_and(|key|
            self.channel_measurement_key(channel, key.basic).is_some_and(|(current, _)| current == key)))
        || self.pair_measurement_busy()
        || self.azr.ref_measure_pending.iter().enumerate().any(|(slot, pending)|
            pending.get().is_some_and(|key| self.reference_measurement_key(slot).is_some_and(|(current, _)| current == key)))
    }

    pub(super) fn poll_reference_measurements(&mut self) {
        let mut completed = Vec::new();
        if let Some(worker) = self.azr.ref_measure_worker.borrow_mut().as_mut() {
            while let Ok(result) = worker.output.try_recv() { worker.busy = false; worker.own_turn = !result.own; completed.push(result); }
        }
        let mut changed = false;
        for result in completed {
            if result.own {
                changed |= self.commit_own_measurement(result);
                continue;
            }
            let (pending, cache, current) = match result.source {
                WaveId::Ref(slot) => (&self.azr.ref_measure_pending[slot], &self.azr.ref_measure[slot], self.reference_measurement_key(slot)),
                WaveId::Ch(channel) => (&self.azr.ch_measure_pending[channel], &self.azr.ch_measure[channel], self.channel_measurement_key(channel, result.key.basic)),
            };
            if pending.get() == Some(result.key) {
                pending.set(None);
                if current.is_some_and(|(current, _)| current == result.key) {
                    *cache.borrow_mut() = Some(RefMeasureCache { key: result.key, analysis: result.analysis, readings: result.readings });
                    changed = true;
                }
            }
        }
        let own_turn = self.azr.ref_measure_worker.borrow().as_ref().is_some_and(|worker| worker.own_turn);
        if own_turn { self.schedule_own_measurement(); }
        for channel in 0..NCH {
            if let Some(key) = self.azr.ch_measure_pending[channel].get() {
                if self.channel_measurement_key(channel, key.basic).is_some_and(|(current, _)| current == key) {
                    let _ = self.channel_measurement(channel, !key.basic);
                } else { self.azr.ch_measure_pending[channel].set(None); }
            }
        }
        for slot in 0..NREF {
            if let Some(key) = self.azr.ref_measure_pending[slot].get() {
                if self.reference_measurement_key(slot).is_some_and(|(current, _)| current == key) {
                    let _ = self.reference_measurement(slot);
                } else { self.azr.ref_measure_pending[slot].set(None); }
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

    pub(super) fn measure_reference_sources(&mut self) {
        let meter = if self.m.dvm_on || self.m.cnt_on { self.offline_meter() } else { None };
        for i in 0..NREF {
            let id = WaveId::Ref(i);
            let requested = self.refs[i].as_ref().is_some_and(|r| r.on)
                || self.m.meas.iter().any(|entry| entry.1 == id)
                || self.m.meas2.iter().any(|entry| entry.1 == id || entry.2 == id) || meter == Some(i);
            let analysis = requested.then(|| self.reference_measurement(i)).flatten();
            self.ref_ana_valid[i] = analysis.is_some();
            self.ref_ana[i] = analysis.map_or_else(Default::default, |(a, _)| a);
            if self.refs[i].is_none() { *self.azr.ref_measure[i].borrow_mut() = None; }
        }
    }

    pub(super) fn refresh_stopped_measurements(&mut self) {
        let (scale, delay) = self.view();
        let key = (scale.to_bits(), delay.to_bits(), self.m.meas_full_record);
        if self.azr.measure_view == Some(key) { return; }
        self.azr.measure_view = Some(key);
        self.measure_last();
    }

    /// Remote view changes must refresh stopped references too: there may be no
    /// next acquisition to build the newly opened analyser's results.
    pub(crate) fn set_analysis_view(&mut self, view: AView) {
        self.m.view = view;
        self.relayout();
        self.analyzer_update();
    }

    pub(super) fn ana_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SpecCenter | Item::SpecSpan | Item::SpecRbw | Item::SpecRef | Item::SpecAcprW | Item::SpecAcprS | Item::EyeRate | Item::EyeBer | Item::EyePnLo | Item::EyePnHi | Item::EyeNotch | Item::PowFline | Item::PowFsw | Item::PowRef => Kind::Value,
            Item::SpecPeak | Item::SpecLimitSet | Item::SpecLimitClear | Item::EyeClear | Item::EyeAct | Item::IdentifyAct => Kind::Action,
            Item::SpecMargin => Kind::Value,
            Item::ProtoWatch => Kind::Choice,
            // a finding is a row with its summary as the only option: tapping it applies it
            Item::IdFind(_) => Kind::Choice,
            Item::SpecOn | Item::SpecSrc | Item::SpecHarm | Item::SpecWin | Item::SpecTrace | Item::SpecWf | Item::SpecDist | Item::EyeOn | Item::EyePlace | Item::EyeSrc | Item::EyeB | Item::EyeHp | Item::EyeNpj | Item::EyeRail | Item::PowOn | Item::PowV | Item::PowI | Item::PowMode | Item::PowVin | Item::PowClk => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn ana_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Spectrum) => "Spectrum",
            Item::Open(MenuId::Eye) => "Eye / jitter",
            Item::Open(MenuId::Power) => "Power analysis",
            Item::IdentifyAct => "Identify",
            Item::ProtoWatch => "Protocol watch",
            Item::SpecOn | Item::EyeOn | Item::PowOn => "View",
            Item::SpecSrc | Item::EyeSrc => "Source",
            Item::SpecCenter => "Centre",
            Item::SpecSpan => "Span",
            Item::SpecHarm => "Harmonics",
            Item::SpecRbw => "RBW",
            Item::SpecWin => "Window",
            Item::SpecTrace => "Trace",
            Item::SpecRef => "Ref level",
            Item::SpecWf => "Waterfall",
            Item::SpecDist => "THD / SNR",
            Item::SpecPeak => "Centre on peak",
            Item::SpecLimitSet => "Limit from trace",
            Item::SpecLimitClear => "Clear limit",
            Item::SpecMargin => "Limit margin",
            Item::EyeAct => "Auto Eye/Jitter",
            Item::EyePlace => "Place",
            Item::EyeRate => "Bit rate",
            Item::EyeBer => "BER",
            Item::EyeB => "Clock",
            Item::EyeHp => "Highpass",
            Item::EyePnLo => "Pn low",
            Item::EyePnHi => "Pn high",
            Item::EyeNpj => "NPJ",
            Item::EyeRail => "Rail",
            Item::EyeNotch => "Notch",
            Item::EyeClear => "Clear eye",
            Item::PowV => "Voltage",
            Item::PowI => "Current",
            Item::PowMode => "Page",
            Item::PowFline => "Line f",
            Item::PowFsw => "Switch f",
            Item::PowVin => "Input",
            Item::PowRef => "Reference",
            Item::PowClk => "Clock",
            Item::SpecAcprW => "Ch width",
            Item::SpecAcprS => "Ch space",
            Item::IdFind(k) => FIND_LABELS[k.min(7)],
            _ => return None,
        })
    }

    pub(super) fn ana_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::SpecOn => onoff(m.view == AView::Spectrum),
            Item::ProtoWatch => onoff(self.azr.watch),
            Item::EyeOn => onoff(m.view == AView::Eye),
            Item::EyePlace => match m.eye_place {
                EyePlace::Full => "Full".into(),
                EyePlace::Side => "Side".into(),
                EyePlace::Under => "Under".into(),
            },
            Item::PowOn => onoff(m.view == AView::Power),
            Item::SpecSrc => self.wave_name(m.spec_src),
            Item::EyeSrc => self.wave_name(m.eye_src),
            Item::PowV => self.wave_name(m.pw_v),
            Item::PowI => self.wave_name(m.pw_i),
            Item::SpecCenter => si_short(m.spec_center, "Hz"),
            Item::SpecSpan if m.spec_span > 0.0 => si_short(m.spec_span, "Hz"),
            Item::SpecSpan if self.spec_follows() && self.azr.spec_stop > 0.0 => format!("Auto {}", si_short(self.azr.spec_stop, "Hz")),
            Item::SpecSpan => "Full".into(),
            Item::SpecHarm => panes::harm_names()[m.spec_harm.min(PANE_HARMS.len() - 1)].clone(),
            Item::SpecRbw => match &self.azr.spec {
                Some(s) if m.spec_rbw == 0.0 => format!("Auto {}", si_short(s.rbw, "Hz")),
                _ if m.spec_rbw == 0.0 => "Auto".into(),
                _ => si_short(m.spec_rbw, "Hz"),
            },
            Item::SpecWin => m.spec_win.name().into(),
            Item::SpecTrace => TRACES[m.spec_trace.min(3)].into(),
            Item::SpecRef => format!("{:.0} dBV", m.spec_ref),
            Item::SpecWf => onoff(m.spec_wf),
            Item::SpecDist => onoff(m.spec_dist),
            Item::EyeRate => if m.eye_from_record && m.eye_rate > 0.0 {
                format!("{} from record", si_short(m.eye_rate, "b/s"))
            } else if m.eye_rate == 0.0 {
                "Auto".into()
            } else {
                si_short(m.eye_rate, "b/s")
            },
            Item::EyeBer => format!("{:.0e}", m.eye_ber),
            Item::EyeB => m.eye_b.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::EyeHp => onoff(m.eye_hp),
            Item::EyePnLo => si_short(m.eye_pn_lo, "Hz"),
            Item::EyePnHi => si_short(m.eye_pn_hi, "Hz"),
            Item::EyeNpj => onoff(m.eye_npj),
            Item::EyeRail => m.eye_rail.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::EyeNotch => if m.eye_notch > 0.0 { si_short(m.eye_notch, "Hz") } else { "off".into() },
            Item::PowMode => power_page(m.pw_mode).into(),
            Item::PowFline => if m.pw_fline > 0.0 { si_short(m.pw_fline, "Hz") } else { "auto".into() },
            Item::PowFsw => if m.pw_fsw > 0.0 { si_short(m.pw_fsw, "Hz") } else { "auto".into() },
            Item::PowVin => m.pw_vin.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::PowRef => m.pw_ref.map(|v| si_short(v, "V")).unwrap_or_else(|| "Top".into()),
            Item::PowClk => m.pw_clk.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into()),
            Item::SpecAcprW => si_short(m.spec_acpr_w, "Hz"),
            Item::SpecAcprS => si_short(m.spec_acpr_s, "Hz"),
            Item::IdFind(k) => self.azr.findings.get(k).map_or(String::new(), |f| {
                // short enough for the row's button (longer text would turn it into a - value + picker)
                let mut s = format!("CH{} {}", f.channels.first().map_or(0, |c| c + 1), f.summary);
                if s.chars().count() > 34 {
                    s = s.chars().take(32).collect::<String>() + "..";
                }
                s
            }),
            Item::SpecPeak | Item::SpecLimitSet | Item::SpecLimitClear | Item::EyeClear | Item::EyeAct | Item::IdentifyAct => String::new(),
            Item::SpecMargin => format!("{:.0} dB", self.m.spec_margin),
            _ => return None,
        })
    }

    pub(super) fn ana_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::SpecOn | Item::EyeOn | Item::PowOn | Item::SpecWf | Item::SpecDist | Item::EyeHp | Item::EyeNpj | Item::ProtoWatch => v(&["Off", "On"]),
            Item::EyePlace => v(&["Full", "Side", "Under"]),
            Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI => self.wave_names(),
            Item::EyeB | Item::EyeRail | Item::PowVin | Item::PowClk => self.wave_names_opt(),
            Item::SpecHarm => panes::harm_names(),
            Item::SpecWin => crate::math::WINDOWS.iter().map(|w| w.name().to_string()).collect(),
            Item::SpecTrace => v(&TRACES),
            Item::PowMode => POWER_MODES.iter().map(|p| power_page(*p).to_string()).collect(),
            Item::IdFind(_) => vec![self.ana_value(it).unwrap_or_default()],
            _ => return None,
        })
    }

    /// Adjusts an analyser item; false = not an analyser item.
    pub(super) fn ana_adjust(&mut self, it: Item, d: i32) -> bool {
        if let Item::IdFind(k) = it {
            if let Some(f) = self.azr.findings.get(k).cloned() {
                self.apply_finding(&f);
                self.close_menu();
                self.toast(&format!("Applied: {}", f.summary));
            }
            return true;
        }
        if it == Item::ProtoWatch {
            self.azr.watch = !self.azr.watch;
            self.azr.watch_seen.clear();
            self.azr.watch_t = 0.0;
            return true;
        }
        if matches!(it, Item::SpecSrc | Item::EyeSrc | Item::PowV | Item::PowI) {
            let cur = match it {
                Item::SpecSrc => self.m.spec_src,
                Item::EyeSrc => self.m.eye_src,
                Item::PowV => self.m.pw_v,
                _ => self.m.pw_i,
            };
            let next = self.step_wave(cur, d);
            match it {
                Item::SpecSrc => self.m.spec_src = next,
                Item::EyeSrc => {
                    self.m.eye_src = next;
                    if self.m.eye_from_record { self.m.eye_rate = self.waveform_bit_rate(next).unwrap_or(0.0); }
                    self.azr.eye = None;
                    self.azr.eye_ref = None;
                    self.azr.eye_res = None;
                    self.azr.page = super::eyepage::Page::default();
                }
                Item::PowV => self.m.pw_v = next,
                _ => self.m.pw_i = next,
            }
            self.azr.spec_avg = None;
            if let Some(w) = &mut self.azr.wf {
                w.clear();
            }
            self.analyzer_update();
            return true;
        }
        if matches!(it, Item::EyeB | Item::EyeRail | Item::PowVin | Item::PowClk) {
            let cur = match it {
                Item::EyeB => self.m.eye_b,
                Item::EyeRail => self.m.eye_rail,
                Item::PowVin => self.m.pw_vin,
                _ => self.m.pw_clk,
            };
            let next = self.step_wave_opt(cur, d);
            match it {
                Item::EyeB => self.m.eye_b = next,
                Item::EyeRail => self.m.eye_rail = next,
                Item::PowVin => self.m.pw_vin = next,
                _ => self.m.pw_clk = next,
            }
            self.analyzer_update();
            return true;
        }
        let cyc = |i: usize, n: usize| (i as i32 + d).rem_euclid(n as i32) as usize;
        let view = |m: &mut Model, v: AView| m.view = if m.view == v { AView::Scope } else { v };
        let m = &mut self.m;
        match it {
            Item::SpecOn => view(m, AView::Spectrum),
            Item::EyeOn => view(m, AView::Eye),
            Item::PowOn => view(m, AView::Power),
            Item::SpecCenter => m.spec_center = step_ladder(m.spec_center, d),
            Item::SpecSpan => m.spec_span = step_ladder(m.spec_span, d),
            Item::SpecRbw => m.spec_rbw = step_ladder(m.spec_rbw, d),
            Item::SpecHarm => {
                m.spec_harm = cyc(m.spec_harm, PANE_HARMS.len());
                self.azr.spec_stop = 0.0;
            }
            Item::SpecMargin => m.spec_margin = (m.spec_margin + d as f32).clamp(0.0, 60.0),
            Item::SpecWin => {
                let w = crate::math::WINDOWS;
                m.spec_win = w[cyc(w.iter().position(|&x| x == m.spec_win).unwrap_or(0), w.len())];
            }
            Item::SpecTrace => m.spec_trace = cyc(m.spec_trace, 4),
            Item::SpecRef => m.spec_ref += 5.0 * d as f32,
            Item::SpecWf => m.spec_wf = !m.spec_wf,
            Item::SpecDist => m.spec_dist = !m.spec_dist,
            Item::EyeRate => {
                m.eye_rate = step_ladder(m.eye_rate, d);
                m.eye_from_record = false;
            }
            Item::EyePlace => {
                let all = [EyePlace::Full, EyePlace::Side, EyePlace::Under];
                let i = all.iter().position(|&p| p == m.eye_place).unwrap_or(0);
                m.eye_place = all[(i as i32 + d).rem_euclid(all.len() as i32) as usize];
            }
            Item::EyeBer => {
                const BERS: [f64; 5] = [1e-3, 1e-6, 1e-9, 1e-12, 1e-15];
                let i = BERS.iter().position(|&x| (x - m.eye_ber).abs() < x * 0.01).unwrap_or(3);
                m.eye_ber = BERS[(i as i32 + d).clamp(0, BERS.len() as i32 - 1) as usize];
            }
            Item::EyeHp => m.eye_hp = !m.eye_hp,
            Item::EyePnLo => m.eye_pn_lo = step_ladder(m.eye_pn_lo, d).max(1.0),
            Item::EyePnHi => m.eye_pn_hi = step_ladder(m.eye_pn_hi, d).max(1.0),
            Item::EyeNpj => m.eye_npj = !m.eye_npj,
            Item::EyeNotch => m.eye_notch = step_ladder(m.eye_notch, d),
            Item::SpecAcprW => m.spec_acpr_w = step_ladder(m.spec_acpr_w.max(1.0), d).max(1.0),
            Item::SpecAcprS => m.spec_acpr_s = step_ladder(m.spec_acpr_s.max(1.0), d).max(1.0),
            Item::PowFline => m.pw_fline = step_ladder(m.pw_fline, d),
            Item::PowFsw => m.pw_fsw = step_ladder(m.pw_fsw, d),
            Item::PowRef => {
                let cur = m.pw_ref.unwrap_or(0.0);
                let next = step_ladder(cur, d);
                m.pw_ref = if next == 0.0 { None } else { Some(next) };
            }
            Item::PowMode => m.pw_mode = POWER_MODES[cyc(POWER_MODES.iter().position(|&p| p == m.pw_mode).unwrap_or(0), POWER_MODES.len())],
            _ => return false,
        }
        // the averaging/waterfall/eye restart on any analyser change
        self.azr.spec_avg = None;
        if let Some(w) = &mut self.azr.wf {
            w.clear();
        }
        self.analyzer_update();
        true
    }

    /// Runs an analyser action; false = not one.
    pub(super) fn ana_activate(&mut self, it: Item) -> bool {
        match it {
            Item::SpecLimitSet => {
                let Some(sp) = &self.azr.spec else {
                    self.toast("Spectrum: no data on the source");
                    return true;
                };
                self.azr.limit = Some(spectrum::limit_from(sp, self.m.spec_margin, 8));
                (self.azr.limit_n, self.azr.limit_over, self.azr.limit_worst) = (0, 0, None);
                return true;
            }
            Item::SpecLimitClear => {
                self.azr.limit = None;
                return true;
            }
            Item::SpecPeak => {
                if let Some(p) = self.strongest_line() {
                    self.m.spec_center = p.f;
                    if self.m.spec_span == 0.0 {
                        self.m.spec_span = (p.f / 5.0).max(1.0);
                    }
                    self.azr.spec_avg = None;
                }
            }
            Item::EyeClear => {
                self.azr.eye = None;
                self.azr.eye_res = None;
                self.azr.page = Default::default();
            }
            Item::EyeAct => {
                self.eye_make();
                return true;
            }
            Item::IdentifyAct => {
                self.identify_now();
                self.open_menu(MenuId::Identify);
            }
            _ => return false,
        }
        true
    }

    /// The strongest spectral line above DC.
    pub(super) fn strongest_line(&self) -> Option<spectrum::Marker> {
        self.azr.peaks.iter().copied().max_by(|a, b| a.dbv.total_cmp(&b.dbv))
    }

    /// The spectrum is on the screen and its span is left to the signal.
    fn spec_follows(&self) -> bool {
        let shown = self.m.view == AView::Spectrum || self.m.view == AView::Scope && matches!(self.m.pane, tables::PANE_SPECTRUM | tables::PANE_RESULTS);
        shown && self.m.spec_span == 0.0 && PANE_HARMS[self.m.spec_harm.min(PANE_HARMS.len() - 1)] > 0
    }

    /// Puts the lines into the order of the table: by frequency or by level.
    pub(super) fn peaks_sort(&mut self) {
        if self.m.spec_sort == 0 {
            self.azr.peaks.sort_by(|a, b| a.f.total_cmp(&b.f));
        } else {
            self.azr.peaks.sort_by(|a, b| b.dbv.total_cmp(&a.dbv));
        }
    }

    /// A stopped scope can still measure a loaded ref. A live channel needs a record.
    fn view_reads_ref(&self, view: AView) -> bool {
        let is_ref = |id: WaveId| matches!(id, WaveId::Ref(_));
        match view {
            AView::Spectrum => is_ref(self.m.spec_src),
            AView::Eye => is_ref(self.m.eye_src) || self.m.eye_b.is_some_and(is_ref) || self.m.eye_rail.is_some_and(is_ref),
            AView::Power => is_ref(self.m.pw_v) || is_ref(self.m.pw_i) || self.m.pw_vin.is_some_and(is_ref) || self.m.pw_clk.is_some_and(is_ref),
            _ => false,
        }
    }

    /// The source exists: a channel that is on, or a ref that holds samples.
    pub(super) fn wave_ready(&self, id: WaveId) -> bool {
        match id {
            WaveId::Ch(c) => c < NCH && self.m.ch[c].on,
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).is_some_and(|r| r.v.len() >= 2 && r.dt > 0.0),
        }
    }

    fn wave_vpd(&self, id: WaveId) -> f64 {
        match id {
            WaveId::Ch(c) => self.m.ch[c.min(NCH - 1)].volts_per_div() as f64,
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).map(|r| r.vpd as f64).unwrap_or(1.0),
        }
    }

    /// Frequency of the wave, for the spectrum's auto span. A ref is measured here, not from `ana`.
    fn wave_freq(&self, rec: &Record, id: WaveId) -> f64 {
        if let WaveId::Ch(c) = id {
            if c < NCH && self.m.ch[c].on {
                return self.meas_at(WaveId::Ch(c), Meas::Freq);
            }
        }
        let Some(s) = self.wave_samples(rec, id) else { return f64::NAN };
        oos_dsp::measure::analyze_min(&s.v, s.dt, 0.4 * self.wave_vpd(id)).get(Meas::Freq)
    }

    /// The spectrum of the source from the record: the view, or `pane` beside the waveform.
    fn spectrum_update(&mut self, rec: &Record, pane: bool) {
        let id = self.m.spec_src;
        let Some(s) = self.wave_samples(rec, id) else {
            self.azr.spec = None;
            self.azr.peaks.clear();
            return;
        };
        let (v, dt) = (s.v, s.dt);
        let f1 = self.wave_freq(rec, id);
        let (mut center, mut span) = (self.m.spec_center, self.m.spec_span);
        if self.spec_follows() {
            // no frequency this record: the span stays
            self.azr.spec_stop = spectrum::auto_stop(f1, PANE_HARMS[self.m.spec_harm.min(PANE_HARMS.len() - 1)], self.azr.spec_stop);
            (center, span) = (self.azr.spec_stop / 2.0, self.azr.spec_stop);
        }
        let area = if pane { self.pane_rect() } else { self.area };
        // Auto RBW: the finest the record allows (window ENBW / record time)
        let finest = spectrum::enbw(self.m.spec_win) / (v.len().max(1) as f64 * dt);
        let s = spectrum::Settings {
            center,
            span,
            rbw: if self.m.spec_rbw > 0.0 { self.m.spec_rbw } else { finest },
            window: self.m.spec_win,
            averaging: 16,
            trace: [spectrum::Trace::Normal, spectrum::Trace::Average, spectrum::Trace::MaxHold, spectrum::Trace::MinHold][self.m.spec_trace.min(3)],
            detector: spectrum::Detector::Peak,
            points: self.spec_box(area, pane).plot.w.max(100) as usize,
            ref_level_dbv: self.m.spec_ref as f64,
        };
        let sp = spectrum::analyze(&v, dt, &s, &mut self.azr.spec_avg);
        self.azr.peaks = spectrum::rank_lines(&sp, Some(f1), panes::PEAKS_MAX);
        self.peaks_sort();
        if self.m.spec_wf && !pane {
            self.azr.wf.get_or_insert_with(|| spectrum::Waterfall::new(120)).push(&sp);
        }
        self.azr.dist = if self.m.spec_dist && !pane { spectrum::distortion(&v, dt, None, 10) } else { None };
        if let Some(l) = &self.azr.limit {
            self.azr.limit_n += 1;
            if let Some(w) = spectrum::limit_check(&sp, l) {
                self.azr.limit_over += 1;
                if self.azr.limit_worst.is_none_or(|x| w.1 > x.1) {
                    self.azr.limit_worst = Some(w);
                }
            }
        }
        self.azr.spec = Some(sp);
    }

    pub(super) fn identify_items(&self) -> Vec<Item> {
        let mut v = vec![Item::ProtoWatch];
        v.extend((0..self.azr.findings.len().min(8)).map(Item::IdFind));
        v
    }

    /// Protocol watch: every 2 s Identify on the screen record of the channels that are on; a bus that was
    /// not there before is announced, the findings are ready in the Identify menu.
    pub(super) fn protocol_watch(&mut self) {
        if !self.azr.watch || self.now - self.azr.watch_t < 2.0 {
            return;
        }
        self.azr.watch_t = self.now;
        let Some(rec) = self.last[0].clone() else { return };
        // every captured sample of the window (the screen record undersamples fast buses), as the measurements do
        let mut dt = rec.dt;
        let owned: Vec<Vec<f32>> = (0..NCH)
            .map(|c| {
                if !self.m.ch[c].on || rec.ch[c].is_empty() {
                    return Vec::new();
                }
                let (v, d) = self.meas_wave(&rec, c);
                dt = d;
                v
            })
            .collect();
        let ws: Vec<&[f32]> = owned.iter().map(|v| v.as_slice()).collect();
        let found = identify::identify(&ws, dt);
        let buses: Vec<String> = found.iter().filter(|f| f.confidence >= 0.6).filter_map(bus_name).collect();
        for b in &buses {
            if !self.azr.watch_seen.contains(b) {
                self.toast(&format!("Protocol watch: {} found (Identify to apply)", b));
            }
        }
        self.azr.watch_seen = buses;
        self.azr.findings = found;
    }

    /// SCPI / MCP: Identify now, one finding per line ("CH1 Clock 1.000 kHz ... (90 %)").
    pub fn identify_text(&mut self) -> String {
        self.identify_now();
        self.azr.findings.iter().map(|f| format!("{} {} ({:.0} %)", f.channels.iter().map(|c| format!("CH{}", c + 1)).collect::<Vec<_>>().join("+"), f.summary, f.confidence * 100.0)).collect::<Vec<_>>().join(";")
    }

    /// Identify on every captured sample of the channels that are on.
    fn identify_now(&mut self) {
        let Some(rec) = self.last[0].clone() else {
            self.azr.findings.clear();
            return;
        };
        let mut dt = rec.dt;
        let ws: Vec<Vec<f32>> = (0..NCH)
            .map(|c| {
                if self.m.ch[c].on && !rec.ch[c].is_empty() {
                    let (v, d) = self.meas_wave(&rec, c);
                    dt = d;
                    v
                } else {
                    Vec::new()
                }
            })
            .collect();
        let refs: Vec<&[f32]> = ws.iter().map(|v| v.as_slice()).collect();
        self.azr.findings = identify::identify(&refs, dt);
        if self.azr.findings.is_empty() {
            self.toast("Identify: nothing found");
        }
    }

    /// Trigger and decoder of an Identify finding.
    fn apply_finding(&mut self, f: &identify::Finding) {
        use identify::{DecoderHint as D, TriggerHint as T};
        let m = &mut self.m;
        match f.trigger {
            T::Edge { source, level, rising, .. } => {
                (m.trig_type, m.trig_src, m.trig_level, m.slope) = (TrigType::Edge, source, level, if rising { Slope::Rise } else { Slope::Fall });
            }
            T::PulseWidth { source, level, positive, less_than, greater_than } => {
                (m.trig_type, m.trig_src, m.trig_level) = (TrigType::Pulse, source, level);
                m.trig_pol = if positive { Pol::Pos } else { Pol::Neg };
                match (less_than, greater_than) {
                    (Some(t), _) => (m.trig_when, m.trig_t1) = (WhenKind::Lt, t as f32),
                    (None, Some(t)) => (m.trig_when, m.trig_t1) = (WhenKind::Gt, t as f32),
                    _ => {}
                }
            }
            T::Video { source, ntsc } => {
                (m.trig_type, m.trig_src) = (TrigType::Video, source);
                m.video_std = if ntsc { VideoStd::Ntsc } else { VideoStd::Pal };
            }
            T::None => {}
        }
        let nearest = |l: &[f64], x: f64| l.iter().enumerate().min_by(|a, b| (a.1 - x).abs().partial_cmp(&(b.1 - x).abs()).unwrap()).map_or(0, |e| e.0);
        let Some(d) = &f.decoder else { return };
        m.dec_on = true;
        match d {
            D::Uart { ch, baud, bits, parity, thr, .. } => {
                (m.dec_proto, m.dec_src, m.dec_bits, m.dec_parity, m.dec_thr) = (Proto::Uart, *ch, *bits, *parity, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *baud);
            }
            D::I2c { sda, scl, thr } => (m.dec_proto, m.dec_src, m.dec_src2, m.dec_thr) = (Proto::I2c, *sda, *scl, *thr),
            D::Spi { clk, data, cs, mode, bits, thr } => {
                (m.dec_proto, m.dec_src2, m.dec_cs, m.dec_mode, m.dec_bits, m.dec_thr) = (Proto::Spi, *clk, *cs, *mode, *bits, *thr);
                m.dec_src = data.first().copied().unwrap_or(0);
            }
            D::Can { ch, bitrate, thr, invert } => {
                (m.dec_proto, m.dec_src, m.dec_thr, m.dec_canh) = (Proto::Can, *ch, *thr, *invert);
                m.dec_canrate = nearest(&crate::decode::CAN_RATES, *bitrate);
            }
            D::Lin { ch, baud, thr } => {
                (m.dec_proto, m.dec_src, m.dec_thr) = (Proto::Lin, *ch, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *baud);
            }
            D::OneWire { ch, thr } => (m.dec_proto, m.dec_src, m.dec_thr) = (Proto::OneWire, *ch, *thr),
            D::Manchester { ch, bitrate, thr } => {
                (m.dec_proto, m.dec_src, m.dec_thr) = (Proto::Manchester, *ch, *thr);
                m.dec_baud = nearest(&crate::decode::BAUDS, *bitrate);
            }
        }
    }

    /// Recomputes the active analyser from the last record (every captured sample of the window).
    pub(super) fn analyzer_update(&mut self) {
        // a source that is off shows nothing: the first channel that is on takes over
        if let Some(on) = (0..NCH).find(|&c| self.m.ch[c].on) {
            for src in [&mut self.m.spec_src, &mut self.m.eye_src] {
                if let WaveId::Ch(c) = *src {
                    if !self.m.ch[c].on {
                        *src = WaveId::Ch(on);
                    }
                }
            }
        }
        // the second pane runs the spectrum beside the scope; the peak list of the Results pane needs it less often
        let beside = self.m.view == AView::Scope && self.pane_h() > 0 && matches!(self.m.pane, tables::PANE_SPECTRUM | tables::PANE_RESULTS);
        if beside && self.m.pane == tables::PANE_RESULTS {
            if self.azr.spec.is_some() && (self.now - self.azr.spec_t).abs() < 0.5 {
                return;
            }
            self.azr.spec_t = self.now;
        }
        let view = if beside { AView::Spectrum } else { self.m.view };
        if view == AView::Scope {
            return;
        }
        // A loaded ref is measured with the scope stopped or still waiting. Live channels still need a record.
        let rec = match self.last[0].clone() {
            Some(r) => r,
            None if matches!(view, AView::Magnetic | AView::Inverter | AView::Dpt) || self.view_reads_ref(view) => Self::blank_record(),
            None => return,
        };
        match view {
            AView::Spectrum => self.spectrum_update(&rec, beside),
            AView::Eye => self.eye_update(&rec),
            AView::Power => {
                let mode = self.m.pw_mode;
                let (v_id, i_id) = (self.m.pw_v, self.m.pw_i);
                let (fline, fsw, reference) = (self.m.pw_fline, self.m.pw_fsw, self.m.pw_ref);
                let taken = match mode {
                    PowerMode::Ripple => self.take_waves(&rec, &[v_id], &[Some(i_id), self.m.pw_vin]),
                    PowerMode::Rail => self.take_waves(&rec, &[v_id], &[self.m.pw_vin, self.m.pw_clk]),
                    _ => self.take_waves(&rec, &[v_id, i_id], &[]),
                };
                let taken = match taken {
                    Ok(t) => t,
                    Err(id) => {
                        self.azr.power = vec![format!("{}: Switch on source in the menu", missing_wave(id))];
                        return;
                    }
                };
                let v = taken.v[0].clone();
                let dt = taken.dt;
                let opt = |k: usize| taken.opt.get(k).and_then(|s| s.clone()).unwrap_or_default();
                let i = if matches!(mode, PowerMode::Ripple) { opt(0) } else if matches!(mode, PowerMode::Rail) { Vec::new() } else { taken.v[1].clone() };
                self.azr.power = match mode {
                    PowerMode::Quality => {
                        let q = power::quality(&v, &i, dt, None);
                        vec![
                            format!("Vrms {}   Irms {}", si(q.vrms, "V"), si(q.irms, "A")),
                            format!("P {}   S {}   Q {}", si(q.p_real, "W"), si(q.s_apparent, "VA"), si(q.q_reactive, "var")),
                            format!("PF {:.3}   phase {:.1}\u{b0}   f {}", q.pf, q.phase_deg, si(q.freq, "Hz")),
                            format!("crest V {:.2}   crest I {:.2}   {} cycles", q.crest_v, q.crest_i, q.cycles),
                        ]
                    }
                    PowerMode::Harmonics => {
                        let h = power::harmonics(&i, dt, None, 15);
                        let mut out = vec!["Current harmonics vs. IEC 61000-3-2 class A".to_string()];
                        out.extend(h.iter().map(|(n, a, lim, pass)| format!("H{:<2} {:>10}  limit {:>10}  {}", n, si(*a, "A"), if lim.is_finite() { si(*lim, "A") } else { "-".into() }, if *pass { "pass" } else { "FAIL" })));
                        out
                    }
                    PowerMode::Switching => {
                        let s = power::switching(&v, &i, dt, 10.0);
                        vec![
                            format!("E on {}   E off {}", si(s.e_on, "J"), si(s.e_off, "J")),
                            format!("E cond {}   P avg {}", si(s.e_cond, "J"), si(s.p_avg, "W")),
                            format!("dv/dt {} / {}", si(s.dvdt_rise, "V/s"), si(s.dvdt_fall, "V/s")),
                            format!("di/dt {} / {}", si(s.didt_rise, "A/s"), si(s.didt_fall, "A/s")),
                            format!("RDS(on) {}", si(s.rds_on, "\u{3a9}")),
                            format!("{} transitions", s.transitions.len()),
                        ]
                    }
                    PowerMode::Ripple => {
                        let (pp, ac) = power::ripple(&v);
                        let (pk, tpk, i2t) = if i.is_empty() { (f64::NAN, f64::NAN, f64::NAN) } else { power::inrush(&i, dt) };
                        let fl = if fline > 0.0 { fline } else { power::line_hz(&v, dt) };
                        let fs = if fsw > 0.0 { fsw } else { power::switch_hz(&v, dt) };
                        let line = power::tone_ripple(&v, dt, fl);
                        let sw = power::switching_ripple(&v, dt, fs);
                        let mut rows = vec![
                            format!("Ripple {} pp   {} rms AC", si(pp, "V"), si(ac, "V")),
                            format!("Line {}  {} pp  {} rms", si(fl, "Hz"), si(line.pp, "V"), si(line.rms, "V")),
                            format!("Switch {}  {} pp  {} rms", si(fs, "Hz"), si(sw.pp, "V"), si(sw.rms, "V")),
                            format!("Inrush peak {} at {}   I\u{b2}t {}", si(pk, "A"), si(tpk, "s"), si(i2t, "A\u{b2}s")),
                        ];
                        let vin = opt(1);
                        if !vin.is_empty() {
                            rows.push(format!("Turn on {}   turn off {}", si(power::turn_on_time(&vin, &v, dt), "s"), si(power::turn_off_time(&vin, &v, dt), "s")));
                        }
                        rows
                    }
                    PowerMode::Capacitance => {
                        vec![
                            format!("C {}", si(power::input_capacitance(&v, &i, dt), "F")),
                            "The record is the turn-on event".into(),
                        ]
                    }
                    PowerMode::Rail => {
                        let vin = opt(0);
                        let clk = opt(1);
                        let r = power::rail(&v, dt, reference, (!vin.is_empty()).then_some(vin.as_slice()), (!clk.is_empty()).then_some(clk.as_slice()));
                        let refn = reference.map(|v| si(v, "V")).unwrap_or_else(|| "top".into());
                        vec![
                            format!("Overshoot {}   undershoot {}", si(r.overshoot, "V"), si(r.undershoot, "V")),
                            format!("DC {}   turn-on max {}", si(r.dc, "V"), si(r.turn_on, "V")),
                            format!("Reference {}", refn),
                        ]
                    }
                };
            }
            AView::Step => self.bw_update(&rec),
            AView::Magnetic => self.magnetic_update(&rec),
            AView::Inverter => self.inverter_update(&rec),
            AView::Dpt => self.dpt_update(&rec),
            AView::Scope | AView::Bode => {}
        }
    }

    /// The active analyser in the plot area.
    pub(super) fn draw_analyzer(&self, cv: &mut Canvas) {
        match self.m.view {
            AView::Spectrum => self.draw_spectrum(cv, self.area, false),
            AView::Eye => self.draw_eye(cv),
            AView::Power => self.draw_power(cv),
            AView::Bode => self.draw_bode(cv),
            AView::Step => self.draw_bw(cv),
            AView::Magnetic => self.draw_magnetic(cv),
            AView::Inverter => self.draw_inverter(cv),
            AView::Dpt => self.draw_dpt(cv),
            AView::Scope => {}
        }
    }

    pub(crate) fn panel(&self, cv: &mut Canvas, x: i32, y: i32, lines: &[String], col: Rgb) {
        let f = &self.fonts;
        let w = lines.iter().map(|l| f.s.width(l)).max().unwrap_or(0) + 16;
        let r = Rect::new(x, y, w, lines.len() as i32 * 18 + 8);
        cv.round(r, 4, PANEL_BG, 230);
        for (k, l) in lines.iter().enumerate() {
            cv.text(&f.s, r.x + 8, r.y + 17 + 18 * k as i32, l, col);
        }
    }

    /// The spectrum in `a`: the plot area, or the second pane (`pane`).
    pub(super) fn draw_spectrum(&self, cv: &mut Canvas, a: Rect, pane: bool) {
        let f = &self.fonts;
        let col = self.wave_color(self.m.spec_src);
        let Some(sp) = &self.azr.spec else {
            let t = if self.wave_ready(self.m.spec_src) { "Spectrum: waiting for a record".to_string() } else { format!("Spectrum: {}", missing_wave(self.m.spec_src)) };
            cv.text(&f.sb, a.x + 20, a.y + a.h / 2 + 5, &t, DIM);
            return;
        };
        cv.fill(a, BG);
        let b = self.spec_box(a, pane);
        let g = b.plot;
        let wf_h = self.spec_wf_h(a, pane);
        let n = sp.dbv.len().max(2);
        let (top, dbdiv) = (self.m.spec_ref, panes::DB_PER_DIV);
        let y_of = |db: f32| (g.y as f32 + (top - db) / dbdiv * (g.h as f32 / VDIV)).clamp(g.y as f32 - 2.0, (g.y + g.h) as f32 + 2.0) as i32;
        let x_of = |i: usize| g.x + (i as i64 * (g.w as i64 - 1) / (n as i64 - 1)) as i32;
        self.draw_spec_frame(cv, &b, sp);
        cv.set_clip(g);
        let mut prev: Option<(i32, i32)> = None;
        for (i, &db) in sp.dbv.iter().enumerate() {
            let p = (x_of(i), y_of(db));
            if let Some(q) = prev {
                cv.line(q.0, q.1, p.0, p.1, col);
            }
            prev = Some(p);
        }
        // the limit, over the frequencies of the trace on the screen
        if let Some(l) = &self.azr.limit {
            let mut prev: Option<(i32, i32)> = None;
            for i in 0..n {
                let Some(db) = spectrum::limit_at(l, sp.freq(i)) else {
                    prev = None;
                    continue;
                };
                let p = (x_of(i), y_of(db));
                if let Some(q) = prev {
                    cv.line(q.0, q.1, p.0, p.1, RED);
                }
                prev = Some(p);
            }
        }
        cv.no_clip();
        self.draw_spec_marks(cv, &b, sp, top);
        self.draw_peaks(cv, &b, sp, pane);
        if let (true, Some(w)) = (wf_h > 0, &self.azr.wf) {
            let wr = Rect::new(g.x, a.y + a.h - wf_h, g.w, wf_h);
            let rows = w.rows.len().max(1);
            let rh = (wf_h as usize / rows.max(wf_h as usize / 3)).max(1) as i32;
            for (k, row) in w.rows.iter().rev().enumerate() {
                let y = wr.y + k as i32 * rh;
                if y >= wr.y + wr.h {
                    break;
                }
                let m = row.len().max(2);
                for px in 0..wr.w {
                    let i = (px as usize * (m - 1)) / (wr.w as usize - 1).max(1);
                    let (r, gg, b) = spectrum::color(row[i.min(m - 1)], top, 90.0);
                    cv.fill(Rect::new(wr.x + px, y, 1, rh), rgb(r as u32, gg as u32, b as u32));
                }
            }
            cv.hline(wr.x, wr.x + wr.w, wr.y, DIM);
        }
    }

    fn draw_power(&self, cv: &mut Canvas) {
        let a = self.area;
        let mut l = vec![format!("Power  V = {}  I = {}  ({:?})", self.wave_name(self.m.pw_v), self.wave_name(self.m.pw_i), self.m.pw_mode)];
        l.extend(self.azr.power.iter().cloned());
        self.panel(cv, a.x + 20, a.y + 30, &l, TEXT);
    }
}

#[cfg(test)]
mod measurement_gate_tests {
    use super::*;

    fn process_memory() -> Option<(usize, usize)> {
        #[cfg(target_os = "windows")]
        {
            #[repr(C)]
            struct Counters { cb: u32, faults: u32, sizes: [usize; 9] }
            #[link(name = "kernel32")]
            extern "system" {
                fn GetCurrentProcess() -> *mut std::ffi::c_void;
                fn K32GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, cb: u32) -> i32;
            }
            let mut counters = Counters { cb: std::mem::size_of::<Counters>() as u32, faults: 0, sizes: [0; 9] };
            let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
            return (ok != 0).then_some((counters.sizes[0], counters.sizes[8]));
        }
        #[cfg(not(target_os = "windows"))]
        None
    }

    /// Actual user file only, never part of unattended tests or CI.
    #[test]
    #[ignore = "requires OOS_GATE_WAVEFORM pointing at a supplied large waveform"]
    fn actual_large_reference_full_record_cache_latency_and_memory() {
        let path = std::env::var("OOS_GATE_WAVEFORM").expect("supply the actual read-only waveform path");
        let baseline = process_memory();
        let load = std::time::Instant::now();
        let bytes = std::fs::read(&path).unwrap();
        let traces = crate::store::parse_wave(&bytes, &path).unwrap();
        drop(bytes);
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.commit_waveform_traces(traces, &path, true).unwrap();
        let points = ui.refs[0].as_ref().unwrap().v.len();
        assert!(points <= crate::store::MAX_WAVEFORM_SAMPLES);
        let loaded = load.elapsed();
        let scan = std::time::Instant::now();
        ui.set_measurement_full_record(true);
        let dispatch = scan.elapsed();
        let mut worst_tick = std::time::Duration::ZERO;
        while ui.measurement_busy() {
            let tick = std::time::Instant::now();
            ui.step(0.0);
            worst_tick = worst_tick.max(tick.elapsed());
            assert!(scan.elapsed().as_secs() < 60, "full record analysis did not finish within its local test bound");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let full_scan = scan.elapsed();
        let (first, _) = ui.reference_measurement(0).unwrap();
        let cached = std::time::Instant::now();
        for k in 0..100 {
            ui.m.zdelay += 1e-9;
            ui.m.delay += 1e-9;
            ui.measure_refs();
            let (analysis, _) = ui.reference_measurement(0).unwrap();
            assert!(Arc::ptr_eq(&first, &analysis), "pan {k} rescanned the immutable full record");
            assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), first.get(Meas::Vpp));
        }
        println!("file={path} points={points} load+initial_screen={loaded:?} full_dispatch={dispatch:?} full_result_ready={full_scan:?} worst_event_tick={worst_tick:?} cached_100_pan_readouts={:?} memory_peak_private_bytes_baseline={baseline:?} memory_peak_private_bytes_after={:?}", cached.elapsed(), process_memory());
    }

    fn sources() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.zoom = false;
        ui.m.tscale = tscales().iter().position(|&v| (v - 100e-9).abs() < 1e-12).unwrap();
        ui.m.tfine = 1.0;
        ui.m.delay = 500e-9;
        let mut rec = Ui::blank_record();
        rec.dt = 1e-9;
        rec.ch[0] = (0..4000).map(|i| {
            let amplitude = if i < 2000 { 1.0 } else { 3.0 };
            if i % 100 < 50 { amplitude } else { -amplitude }
        }).collect();
        ui.last[0] = Some(rec);
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "2ns reference".into(),
            t0: 0.0, dt: 2e-9, v: (0..2000).map(|i| {
                let amplitude = if i < 1000 { 2.0 } else { 4.0 };
                if i % 50 < 25 { amplitude } else { -amplitude }
            }).collect(), vpd: 1.0, offset: 0.0, on: true });
        ui
    }

    #[test]
    fn screen_gate_is_half_open_at_f32_200ps_boundaries_with_nonzero_origins() {
        let dt = 20e-12;
        for center in [0.0, 2f64.powi(-20)] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.run = RunState::Stop;
            ui.m.zoom = false;
            ui.m.tscale = 0;
            ui.m.tfine = 200e-12f32 / tscales()[0];
            ui.m.delay = center as f32;
            assert_eq!(ui.view().0, 200e-12f32);
            let t0 = center - 1e-6;
            let (lo, hi) = (49_950, 50_050);
            assert_eq!(ui.measurement_bounds(100_000, t0, dt), (lo, hi));
            let mut values = vec![0.0; 100_000];
            for (i, value) in values[lo..hi].iter_mut().enumerate() { *value = if i % 2 == 0 { -0.02 } else { 0.02 }; }
            values[lo] = -0.08; // exact left border belongs to the gate
            values[hi - 1] = 0.04;
            values[lo - 1] = -0.6; // the old floor bound leaked this in
            values[hi] = 0.8; // the right border must never be included
            let expected_rms = (values[lo..hi].iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / 100.0).sqrt();
            let mut rec = Ui::blank_record();
            rec.t0 = t0;
            rec.dt = dt;
            rec.ch[0] = values.clone();
            ui.last[0] = Some(rec);
            ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "border outliers".into(),
                t0, dt, v: values, vpd: 0.02, offset: 0.0, on: true });
            ui.measure_last();
            for source in [WaveId::Ch(0), WaveId::Ref(0)] {
                assert!((ui.fact_meas(Meas::Vpp, source) - 0.12).abs() < 1e-8);
                assert!((ui.fact_meas(Meas::Vrms, source) - expected_rms).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn screen_gate_keeps_real_near_boundary_samples_without_rounding_the_view() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.zoom = false;
        ui.m.tscale = 0;
        ui.m.tfine = 200e-12f32 / tscales()[0];
        ui.m.delay = 0.0;
        let dt = 20e-12;
        // Two hundred-thousandths of a sample is a real interior shift, beyond
        // the bounded numerical fringe. Do not round either physical endpoint.
        let shift = 2e-5 * dt;
        assert_eq!(ui.measurement_bounds(100_000, -1e-6 - shift, dt), (49_951, 50_051));
        assert_eq!(ui.measurement_bounds(100_000, -1e-6 + shift, dt), (49_950, 50_050));
        assert_eq!(ui.m.delay, 0.0, "index snapping must not modify the selected view");
    }

    #[test]
    fn screen_and_full_record_gate_mixed_sources_in_physical_time() {
        let mut ui = sources();
        assert!(!ui.m.meas_full_record);
        assert!(!ui.record_mode(), "this checks mixed live and reference overlays");
        ui.measure_last();
        assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 2.0);
        assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 4.0);
        ui.m.delay = 3e-6;
        ui.refresh_stopped_measurements();
        assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 6.0);
        assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 8.0);
        ui.set_measurement_full_record(true);
        ui.m.delay = 500e-9;
        ui.refresh_stopped_measurements();
        assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 6.0);
        assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 8.0);
        ui.set_measurement_full_record(false);
        assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vpp), 2.0);
        assert_eq!(ui.meas_at(WaveId::Ref(0), Meas::Vpp), 4.0);
    }

    #[test]
    fn long_reference_cache_reuses_analysis_and_invalidates_accepted_sim_settings() {
        let mut ui = sources();
        ui.m.meas_full_record = true;
        ui.session_is_sim = true;
        ui.m.sim_slot = 0;
        ui.replay.sim_revision = 1;
        let (first, _) = ui.reference_measurement(0).unwrap();
        ui.m.delay = 3e-6;
        ui.measure_refs();
        let (pan, _) = ui.reference_measurement(0).unwrap();
        assert!(Arc::ptr_eq(&first, &pan), "full-record panning and capture ticks must reuse the exact analysis");
        assert!(Arc::ptr_eq(&pan, &ui.ref_ana[0]), "every readout shares the cached result");
        ui.refs[0].as_mut().unwrap().vpd = 2.0;
        let (scale, _) = ui.reference_measurement(0).unwrap();
        assert!(!Arc::ptr_eq(&pan, &scale));
        ui.m.meas_lpct = 0.2;
        ui.m.meas_hpct = 0.8;
        let (thresholds, _) = ui.reference_measurement(0).unwrap();
        assert!(!Arc::ptr_eq(&scale, &thresholds));
        // A worker may fill the existing Vec: pointer/length alone are insufficient.
        for value in &mut ui.refs[0].as_mut().unwrap().v { *value *= 2.0; }
        ui.replay.sim_revision += 1;
        let (fresh, _) = ui.reference_measurement(0).unwrap();
        assert!(!Arc::ptr_eq(&thresholds, &fresh));
        assert_eq!(fresh.get(Meas::Vpp), 16.0);
    }

    #[test]
    fn measurement_gate_is_visible_selectable_and_keeps_old_setups_on_screen() {
        let mut ui = sources();
        ui.open_menu(MenuId::MeasAdd);
        let row = ui.dlg_layout().unwrap().rows.into_iter().find(|row| row.it == Item::MeasGate).unwrap();
        ui.tap_menu(row.ctrl.x + row.ctrl.w * 3 / 4, row.ctrl.y + row.ctrl.h / 2);
        assert!(ui.m.meas_full_record);
        assert_eq!(ui.value(Item::MeasGate), "Full record");
        let text = crate::store::setup_text(&ui.m);
        assert!(crate::store::parse_setup(&text).unwrap().meas_full_record);
        let old = text.lines().filter(|line| !line.starts_with("meas_full_record =")).collect::<Vec<_>>().join("\n");
        assert!(!crate::store::parse_setup(&old).unwrap().meas_full_record);
    }

    fn large() -> Ui {
        let mut ui = sources();
        ui.set_offline(true);
        ui.refs[0].as_mut().unwrap().v = (0..400_000).map(|i| if i % 100 < 50 { 1.0 } else { -1.0 }).collect();
        ui.m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
        ui
    }

    fn pump(ui: &mut Ui) {
        let start = std::time::Instant::now();
        loop {
            ui.step(0.0);
            if !ui.azr.ref_measure_worker.borrow().as_ref().is_some_and(|worker| worker.busy) && !ui.measurement_busy() { break; }
            assert!(start.elapsed().as_secs() < 10, "bounded worker did not complete");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn large_full_measurements_are_nonblocking_and_use_one_worker_without_fake_captures() {
        let mut ui = large();
        let start = std::time::Instant::now();
        ui.set_measurement_full_record(true);
        assert!(start.elapsed().as_millis() < 500, "dispatch must not perform the full scan on the event thread");
        assert!(ui.measurement_busy());
        for _ in 0..100 { assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan()); }
        assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted, 1, "readouts must not queue duplicate jobs");
        assert_eq!(ui.meas_rows()[0].text(), "Computing");
        assert!(ui.workflow_state().contains("\"measurementBusy\":true"));
        let captures = ui.replay.captures;
        ui.mstat = vec![MStat { n: 7, ..Default::default() }];
        pump(&mut ui);
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 2.0);
        assert_eq!(ui.replay.captures, captures);
        assert_eq!(ui.mstat[0].n, 7, "analysis completion must preserve acquisition statistics without adding a sample");
        assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted, 1);
    }

    #[test]
    fn stale_large_results_cannot_replace_a_new_source_or_gate() {
        let mut ui = large();
        ui.set_measurement_full_record(true);
        // Replace the source while its immutable worker snapshot is being measured.
        ui.workflow_epoch += 1;
        for value in &mut ui.refs[0].as_mut().unwrap().v { *value *= 3.0; }
        assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
        pump(&mut ui);
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 6.0);
        assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted, 2);
        ui.session_is_sim = true;
        ui.m.sim_slot = 0;
        ui.replay.sim_revision += 1;
        assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
        ui.set_measurement_full_record(false);
        assert!(!ui.measurement_busy());
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 6.0);
        pump(&mut ui);
        assert!(!ui.m.meas_full_record);
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 6.0);
        assert!(ui.workflow_state().contains("\"gate\":\"screen\""));
    }

    #[test]
    fn a_large_screen_gate_runs_in_background_and_discards_stale_pan_results() {
        let mut ui = large();
        ui.refs[0].as_mut().unwrap().v = (0..400_000).map(|i| {
            let amplitude = if i < 200_000 { 3.0 } else { 1.0 };
            if i % 100 < 50 { amplitude } else { -amplitude }
        }).collect();
        ui.m.tscale = tscales().iter().position(|&v| (v - 100e-6).abs() < 1e-10).unwrap();
        ui.m.delay = 400e-6;
        ui.m.meas_full_record = false;
        let started = std::time::Instant::now();
        ui.measure_refs();
        assert!(started.elapsed().as_millis() < 500, "a zoomed-out Screen gate must not scan on the event thread");
        assert!(ui.measurement_busy());
        assert!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)).is_nan());
        assert_eq!(ui.meas_rows()[0].text(), "Computing");
        // Pan to a smaller, low-amplitude region before the old job completes.
        ui.m.tscale = tscales().iter().position(|&v| (v - 50e-6).abs() < 1e-10).unwrap();
        ui.m.delay = 700e-6;
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 2.0);
        let wanted = ui.reference_measurement_key(0).unwrap().0;
        pump(&mut ui);
        assert!(ui.azr.ref_measure[0].borrow().as_ref().is_some_and(|cached| cached.key == wanted), "the old broad Screen result cannot replace the panned gate");
        assert_eq!(ui.fact_meas(Meas::Vpp, WaveId::Ref(0)), 2.0);
        assert_eq!(ui.replay.captures, 0);
    }
}
