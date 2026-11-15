// SPDX-License-Identifier: GPL-2.0-or-later
//! Per-row parameters share the normal physical gate and its one bounded worker.
//! Scalar results remain cached after large edge analyses are evicted.

use super::*;
use super::super::reflook::MeasOwn;
use std::collections::VecDeque;

const ANALYSIS_LIMIT: usize = 4;
const SCALAR_LIMIT: usize = super::super::tools::MEAS_MAX * (NREF + 1);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key { source: WaveId, analysis: RefMeasureKey }

#[derive(Clone, Copy)]
struct Request { key: Key, kind: Meas, owner: WaveId }

struct Cached { key: Key, analysis: Option<Arc<oos_dsp::measure::Analysis>>, readings: Vec<f64> }

#[derive(Default)]
pub(super) struct State {
    cached: VecDeque<Cached>,
    pending: VecDeque<Request>,
}

fn analysis_key(mut key: RefMeasureKey) -> RefMeasureKey {
    key.idle = 0; key.outside = 0; key.periods = 0; key.data = false; key.basic = false;
    key
}

impl Ui {
    /// The gate in use while a row's key or samples are made: the row's own, else the shared one.
    pub(in crate::ui) fn gate_full(&self) -> bool {
        self.row_gate.get().unwrap_or(self.m.meas_full_record)
    }

    fn with_gate<R>(&self, gate: Option<bool>, f: impl FnOnce() -> R) -> R {
        let old = self.row_gate.replace(gate);
        let result = f();
        self.row_gate.set(old);
        result
    }

    /// The row settings of a single-source row, over the shared ones.
    pub(super) fn row_cfg_one(&self, kind: Meas, owner: WaveId) -> crate::model::RowCfg {
        self.m.row_cfg(RowKey::One(kind, owner)).unwrap_or_default()
    }

    /// The row's own analysis settings (levels, base / top, outside level, gate), if it has any.
    pub(in crate::ui) fn own_params(&self, kind: Meas, owner: WaveId) -> Option<MeasOwn> {
        let cfg = self.row_cfg_one(kind, owner);
        let legacy = self.meas_own.get(&(kind, owner)).copied().filter(|own| own.refs_set || (kind == Meas::Burst && own.idle_set)
            || (kind == Meas::Outside && own.side_set) || (kind == Meas::NPeriods && (own.n_set || own.data_set)));
        if legacy.is_none() && !cfg.changes_analysis() { return None; }
        let shared = MeasOwn::shared(&self.m);
        let own = legacy.unwrap_or(shared);
        Some(MeasOwn {
            low: if own.refs_set { own.low } else { shared.low },
            high: if own.refs_set { own.high } else { shared.high },
            idle: if own.idle_set { own.idle } else { shared.idle },
            side: if own.side_set { own.side } else { shared.side },
            n: if own.n_set { own.n } else { shared.n },
            data: if own.data_set { own.data } else { shared.data },
            cfg,
            ..own
        })
    }

    /// The settings of a reading that are taken from the analysis afterwards (cycle edges, overshoot reference, burst
    /// and N-period options). None when the row has none that apply to `kind`.
    pub(super) fn row_opts(&self, kind: Meas, owner: WaveId) -> Option<oos_dsp::measure::RowOpts> {
        use oos_dsp::measure::{EdgePol, NEdge, Outside, RowOpts};
        let c = self.row_cfg_one(kind, owner);
        let applies = match kind {
            Meas::Period | Meas::Freq | Meas::PDuty | Meas::NDuty => c.pol.is_some() || c.off.is_some() || c.step.is_some(),
            Meas::PWidth | Meas::NWidth => c.off.is_some() || c.step.is_some(),
            Meas::Overshoot | Meas::Preshoot => c.os_top.is_some() || c.os_base.is_some(),
            Meas::Burst => c.burst_fall.is_some() || c.idle.is_some(),
            Meas::Outside => c.side.is_some(),
            Meas::NPeriods => c.n.is_some() || c.n_off.is_some() || c.n_step.is_some() || c.n_edge.is_some(),
            _ => false,
        };
        if !applies { return None; }
        let shared = self.meas_own.get(&(kind, owner)).copied().unwrap_or_else(|| MeasOwn::shared(&self.m));
        let (idle, side, n, data) = (
            if shared.idle_set { shared.idle } else { self.m.meas_idle },
            if shared.side_set { shared.side } else { self.m.meas_out },
            if shared.n_set { shared.n } else { self.m.meas_n },
            if shared.data_set { shared.data } else { self.m.meas_data },
        );
        Some(RowOpts {
            pol: match c.pol { Some(1) => EdgePol::Rise, Some(2) => EdgePol::Fall, _ => EdgePol::First },
            offset: c.off.unwrap_or(0),
            step: c.step.unwrap_or(1).max(1),
            os_top: c.os_top,
            os_base: c.os_base,
            idle: c.idle.unwrap_or(idle),
            burst_falling: c.burst_fall.unwrap_or(false),
            side: match c.side { Some(0) => Outside::Above, Some(1) => Outside::Below, Some(_) => Outside::Both, None => side },
            n: c.n.unwrap_or(n).max(1),
            n_edge: match c.n_edge { Some(1) => NEdge::Fall, Some(2) => NEdge::Both, Some(_) => NEdge::Rise, None => if data { NEdge::Both } else { NEdge::Rise } },
            n_offset: c.n_off.unwrap_or(0),
            n_step: c.n_step.unwrap_or(1).max(1),
        })
    }

    fn own_key(&self, source: WaveId, own: MeasOwn) -> Option<(Key, f64)> {
        let c = own.cfg;
        let (mut key, t0) = self.with_gate(c.gate_full, || match source {
            WaveId::Ch(c) => self.channel_measurement_key(c, false),
            WaveId::Ref(i) => self.reference_measurement_key(i),
        })?;
        key.low = own.low.to_bits(); key.high = own.high.to_bits();
        // a row's own low and high replace the shared rise ones (percent mode); the fall levels follow unless they are separate
        let follow = key.lv.fall == key.lv.rise;
        if !key.lv.absolute {
            key.lv.rise[0] = own.low;
            key.lv.rise[2] = own.high;
        }
        // the row's own levels: percent of amplitude, or volts in the volts mode
        if let Some(l) = c.lo { key.lv.rise[0] = l; key.low = l.to_bits(); }
        if let Some(h) = c.hi { key.lv.rise[2] = h; key.high = h.to_bits(); }
        if follow { key.lv.fall = key.lv.rise; }
        if let Some(h) = c.hyst { key.lv.hyst = h; }
        if let Some(b) = c.base_top { key.lv.base_top = b; }
        if let Some(v) = c.out_volts { key.lv.out_volts = v; }
        key.idle = own.idle.to_bits(); key.outside = own.side as u8;
        key.periods = own.n; key.data = own.data; key.basic = false;
        Some((Key { source, analysis: key }, t0))
    }

    fn request_current(&self, request: Request) -> bool {
        self.own_params(request.kind, request.owner)
            .and_then(|own| self.own_key(request.key.source, own))
            .is_some_and(|(key, _)| key == request.key)
    }

    fn reusable_own_analysis(&self, key: Key) -> Option<Arc<oos_dsp::measure::Analysis>> {
        let expected = analysis_key(key.analysis);
        let cache = match key.source { WaveId::Ch(c) => self.azr.ch_measure.get(c)?, WaveId::Ref(i) => self.azr.ref_measure.get(i)? }.borrow();
        if let Some(cache) = cache.as_ref().filter(|cache| !cache.key.basic && analysis_key(cache.key) == expected) {
            return Some(Arc::clone(&cache.analysis));
        }
        self.azr.own_measure.borrow().cached.iter().rev()
            .find(|cache| cache.key.source == key.source && analysis_key(cache.key.analysis) == expected && cache.analysis.is_some())
            .and_then(|cache| cache.analysis.as_ref().map(Arc::clone))
    }

    fn own_job(&self, key: Key) -> Option<RefMeasureJob> {
        self.with_gate(Some(key.analysis.full), || self.own_job_inner(key))
    }

    fn own_job_inner(&self, key: Key) -> Option<RefMeasureJob> {
        let analysis = self.reusable_own_analysis(key);
        let k = key.analysis;
        let samples = if analysis.is_some() { MeasureSamples::Owned(Vec::new()) } else {
            match key.source {
                WaveId::Ref(i) => MeasureSamples::Owned(self.refs.get(i)?.as_ref()?.v[k.lo..k.hi].to_vec()),
                WaveId::Ch(c) => {
                    let record = self.last[0].as_ref()?;
                    let (values, _, _) = self.measurement_channel_samples(record, c)?;
                    if let Some(raw) = record.raw.as_ref().filter(|raw| {
                        let original = &raw.frame.ch[c];
                        let start = values.as_ptr() as usize;
                        start >= original.as_ptr() as usize && start + std::mem::size_of_val(values)
                            <= original.as_ptr() as usize + std::mem::size_of_val(original.as_slice())
                    }) {
                        let lo = (values.as_ptr() as usize - raw.frame.ch[c].as_ptr() as usize) / std::mem::size_of::<f32>();
                        MeasureSamples::Native { frame: Arc::clone(&raw.frame), channel: c, lo, hi: lo + values.len() }
                    } else { MeasureSamples::Owned(values.to_vec()) }
                }
            }
        };
        Some(RefMeasureJob { source: key.source, key: k, samples, dt: f64::from_bits(k.dt),
            min_amp: 0.4 * f32::from_bits(k.vpd) as f64, lv: k.lv,
            own: true, analysis })
    }

    fn cache_own_result(&self, key: Key, analysis: Arc<oos_dsp::measure::Analysis>, readings: Vec<f64>) {
        let mut state = self.azr.own_measure.borrow_mut();
        state.cached.retain(|cached| cached.key != key);
        state.cached.push_back(Cached { key, analysis: Some(analysis), readings });
        while state.cached.len() > SCALAR_LIMIT { state.cached.pop_front(); }
        // A scalar-only entry still answers all row reads; do not rescan because
        // its large edge arrays no longer occupy one of the four retained slots.
        let mut retained = 0;
        for cached in state.cached.iter_mut().rev() {
            if cached.analysis.is_some() {
                retained += 1;
                if retained > ANALYSIS_LIMIT { cached.analysis = None; }
            }
        }
    }

    /// A single-source row's reading with its own settings: its own analysis (levels, base / top, gate), then the
    /// settings that apply to the analysis afterwards.
    pub(in crate::ui) fn measurement_with_own(&self, source: WaveId, kind: Meas, owner: WaveId) -> f64 {
        let value = self.measurement_with_own_raw(source, kind, owner);
        let Some(opts) = self.row_opts(kind, owner) else { return value };
        match self.meas_number_analysis_raw(source, kind, owner) {
            Some((analysis, _)) => analysis.get_row(kind, &opts),
            None => f64::NAN,
        }
    }

    fn measurement_with_own_raw(&self, source: WaveId, kind: Meas, owner: WaveId) -> f64 {
        let Some(own) = self.own_params(kind, owner) else { return self.meas_at(source, kind) };
        let Some((key, t0)) = self.own_key(source, own) else { return f64::NAN };
        let index = ALL_MEAS.iter().position(|&entry| entry == kind).unwrap();
        if let Some(value) = self.azr.own_measure.borrow().cached.iter().find(|cached| cached.key == key)
            .and_then(|cached| cached.readings.get(index)).copied() {
            return if matches!(kind, Meas::TVmax | Meas::TVmin) { value + t0 } else { value };
        }
        if key.analysis.hi - key.analysis.lo <= BACKGROUND_MEASUREMENT_POINTS {
            let Some(job) = self.own_job(key) else { return f64::NAN };
            let analysis = job.analysis.unwrap_or_else(|| Arc::new(oos_dsp::measure::analyze_levels(
                job.samples.slice(), job.dt, job.min_amp, &job.lv)));
            let readings = scalar_readings(&analysis, key.analysis);
            let value = readings[index];
            self.cache_own_result(key, analysis, readings);
            return if matches!(kind, Meas::TVmax | Meas::TVmin) { value + t0 } else { value };
        }
        {
            let mut state = self.azr.own_measure.borrow_mut();
            state.pending.retain(|request| !(request.key.source == source && request.owner == owner && request.kind == kind && request.key != key));
            if !state.pending.iter().any(|request| request.key == key && request.owner == owner && request.kind == kind) {
                if state.pending.len() >= SCALAR_LIMIT { state.pending.pop_front(); }
                state.pending.push_back(Request { key, kind, owner });
            }
        }
        self.schedule_own_measurement();
        f64::NAN
    }

    pub(super) fn schedule_own_measurement(&self) {
        let request = {
            let mut state = self.azr.own_measure.borrow_mut();
            state.pending.retain(|request| self.request_current(*request));
            state.pending.front().copied()
        };
        let Some(request) = request else { return };
        let mut worker = self.azr.ref_measure_worker.borrow_mut();
        let worker = worker.get_or_insert_with(RefMeasureWorker::open);
        if worker.busy { return; }
        let Some(job) = self.own_job(request.key) else { return };
        if worker.input.try_send(job).is_ok() { worker.busy = true; worker.submitted += 1; }
    }

    pub(super) fn commit_own_measurement(&self, result: RefMeasureResult) -> bool {
        let key = Key { source: result.source, analysis: result.key };
        let accepted = self.azr.own_measure.borrow().pending.iter().any(|request| request.key == key && self.request_current(*request));
        self.azr.own_measure.borrow_mut().pending.retain(|request| request.key != key);
        if accepted { self.cache_own_result(key, result.analysis, result.readings); }
        accepted
    }

    pub(super) fn own_measurement_busy(&self) -> bool {
        self.azr.own_measure.borrow().pending.iter().any(|request| self.request_current(*request))
    }

    pub(in crate::ui) fn meas_number_computing(&self, source: WaveId, kind: Meas) -> bool {
        let Some(own) = self.own_params(kind, source) else { return self.measurement_computing(source) };
        let Some((key, _)) = self.own_key(source, own) else { return false };
        self.azr.own_measure.borrow().pending.iter().any(|request| request.key == key && self.request_current(*request))
    }

    /// Labels only use the matching row analysis; an evicted large analysis may
    /// omit annotations while its numeric value remains cached and available.
    pub(in crate::ui) fn meas_number_analysis(&self, source: WaveId, kind: Meas) -> Option<(Arc<oos_dsp::measure::Analysis>, f64)> {
        self.meas_number_analysis_raw(source, kind, source)
    }

    fn meas_number_analysis_raw(&self, source: WaveId, kind: Meas, owner: WaveId) -> Option<(Arc<oos_dsp::measure::Analysis>, f64)> {
        let Some(own) = self.own_params(kind, owner) else { return self.meas_analysis(source) };
        let (key, t0) = self.own_key(source, own)?;
        let _ = self.measurement_with_own_raw(source, kind, owner);
        self.reusable_own_analysis(key).map(|analysis| (analysis, t0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn reference(count: usize) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.meas_full_record = true;
        ui.m.meas.clear(); ui.m.meas2.clear();
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "row-test".into(), t0: 0.001, dt: 1e-9,
            v: ((0..count).map(|i| { let k = i % 400; match k {
                0..=99 => -0.5, 100..=199 => -0.5 + (k - 100) as f32 / 99.0,
                200..=299 => 0.5, _ => 0.5 - (k - 300) as f32 / 99.0,
            } }).collect::<Vec<_>>()).into(), vpd: 0.2, offset: 0.0, on: true });
        ui
    }

    fn custom(ui: &mut Ui, kind: Meas, low: f64, high: f64) {
        let id = WaveId::Ref(0);
        ui.m.meas.push((kind, id));
        ui.meas_own.insert((kind, id), MeasOwn { refs_set: true, low, high, ..MeasOwn::shared(&ui.m) });
    }

    fn wait(ui: &mut Ui, kinds: &[Meas]) -> Vec<f64> {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            ui.poll_reference_measurements();
            let values: Vec<_> = kinds.iter().map(|&kind| ui.meas_number(WaveId::Ref(0), kind)).collect();
            if values.iter().all(|v| v.is_finite()) { return values; }
            assert!(Instant::now() < end, "custom row jobs did not finish: {values:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn custom_rows_keep_physical_extremum_time_and_scpi_row_parity() {
        let mut ui = reference(800);
        custom(&mut ui, Meas::TVmax, 0.2, 0.8);
        let id = WaveId::Ref(0);
        let time = ui.meas_number(id, Meas::TVmax);
        assert!((time - (0.001 + 199e-9)).abs() < 1e-14, "{time}");
        assert_eq!(ui.fact_meas(Meas::TVmax, id), time);
        assert_eq!(ui.meas_rows()[0].v, time);
        assert_eq!(ui.set_cell(Meas::TVmax, 0), time);
        assert!(!ui.meas_rows()[0].computing);
        assert_eq!((ui.records, ui.wfm_total), (0, 0));
    }

    #[test]
    fn large_distinct_row_levels_share_one_worker_without_cache_thrash() {
        let mut ui = reference(600_000);
        custom(&mut ui, Meas::Rise, 0.1, 0.9);
        custom(&mut ui, Meas::Fall, 0.2, 0.8);
        let start = Instant::now();
        assert!(ui.meas_number(WaveId::Ref(0), Meas::Rise).is_nan());
        assert!(ui.meas_number(WaveId::Ref(0), Meas::Fall).is_nan());
        assert!(start.elapsed() < Duration::from_millis(250), "render reader blocked");
        assert!(ui.meas_rows().iter().all(|row| row.computing));
        let values = wait(&mut ui, &[Meas::Rise, Meas::Fall]);
        assert!((values[0] - 79.2e-9).abs() < 3e-9, "{values:?}");
        assert!((values[1] - 59.4e-9).abs() < 3e-9, "{values:?}");
        let submitted = ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted;
        for _ in 0..100 {
            assert_eq!(ui.meas_number(WaveId::Ref(0), Meas::Rise), values[0]);
            assert_eq!(ui.fact_meas(Meas::Fall, WaveId::Ref(0)), values[1]);
            assert!(ui.meas_rows().iter().all(|row| !row.computing));
        }
        assert_eq!(ui.azr.ref_measure_worker.borrow().as_ref().unwrap().submitted, submitted);
        assert_eq!(ui.azr.own_measure.borrow().cached.len(), 2);
        assert_eq!((ui.m.run, ui.records, ui.wfm_total), (RunState::Stop, 0, 0));
    }

    #[test]
    fn pending_row_rejects_changed_gate_levels_and_replaced_reference() {
        let mut ui = reference(600_000);
        custom(&mut ui, Meas::Rise, 0.1, 0.9);
        assert!(ui.meas_number(WaveId::Ref(0), Meas::Rise).is_nan());
        let old = ui.azr.own_measure.borrow().pending[0].key;
        ui.m.meas_full_record = false;
        ui.m.tscale = tscales().iter().position(|&v| (v - 50e-6).abs() < 1e-10).unwrap();
        ui.m.delay = 0.0013;
        ui.meas_own.get_mut(&(Meas::Rise, WaveId::Ref(0))).unwrap().low = 0.2;
        ui.meas_own.get_mut(&(Meas::Rise, WaveId::Ref(0))).unwrap().high = 0.8;
        let original = ui.refs[0].as_ref().unwrap().v.clone();
        ui.refs[0].as_mut().unwrap().v = original;
        ui.workflow_epoch = ui.workflow_epoch.wrapping_add(1);
        let value = wait(&mut ui, &[Meas::Rise])[0];
        assert!((value - 59.4e-9).abs() < 3e-9, "{value}");
        let state = ui.azr.own_measure.borrow();
        assert!(!state.cached.iter().any(|cache| cache.key == old), "stale result entered the cache");
        assert!(state.cached.iter().all(|cache| !cache.key.analysis.full && cache.key.analysis.low == 0.2f64.to_bits()));
    }

    #[test]
    fn evicting_edge_arrays_preserves_scalar_values_without_rescanning() {
        let mut ui = reference(800);
        let kinds = [Meas::Vpp, Meas::Vrms, Meas::Freq, Meas::Rise, Meas::Fall, Meas::TVmax];
        for (i, &kind) in kinds.iter().enumerate() { custom(&mut ui, kind, 0.1 + i as f64 * 0.01, 0.9); }
        let first = ui.meas_number(WaveId::Ref(0), kinds[0]);
        for &kind in &kinds[1..] { assert!(ui.meas_number(WaveId::Ref(0), kind).is_finite()); }
        let state = ui.azr.own_measure.borrow();
        assert_eq!(state.cached.len(), kinds.len());
        assert_eq!(state.cached.iter().filter(|cache| cache.analysis.is_some()).count(), ANALYSIS_LIMIT);
        assert!(state.cached.front().unwrap().analysis.is_none());
        drop(state);
        assert_eq!(ui.meas_number(WaveId::Ref(0), kinds[0]), first);
        assert!(ui.azr.ref_measure_worker.borrow().is_none(), "scalar read must not recreate evicted edges");
        assert_eq!(ui.azr.own_measure.borrow().cached.len(), kinds.len());
    }

}
