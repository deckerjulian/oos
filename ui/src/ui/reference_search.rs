// SPDX-License-Identifier: GPL-2.0-or-later
//! Full-record reference trigger hits shared by Search and replay acquisition.
use super::*;
use std::sync::{Arc, mpsc};

const BACKGROUND_POINTS: usize = 262_144;

struct Job {
    key: String,
    model: Model,
    primary: Vec<f32>,
    secondary: Option<super::magnetic::WaveSamp>,
    dt: f64,
    t0: f64,
    hyst: [f32; NCH],
}

fn scan_condition(model: &Model, primary: &[f32], dt: f64, t0: f64,
    secondary: Option<(&[f32], f64, f64)>, hyst: [f32; NCH]) -> Arc<Vec<f64>> {
    let aligned;
    let second = if let Some((values, start, step)) = secondary {
        if start == t0 && step == dt && values.len() == primary.len() { values }
        else {
            aligned = (0..primary.len()).map(|i| {
                let index = (t0 + i as f64 * dt - start) / step;
                if !index.is_finite() || index < 0.0 || index >= values.len().saturating_sub(1) as f64 { return f32::NAN; }
                let k = index.floor() as usize;
                values[k] + index.fract() as f32 * (values[k + 1] - values[k])
            }).collect::<Vec<_>>();
            aligned.as_slice()
        }
    } else { &[] };
    let input = oos_dsp::trigger::Input { ch: [primary, second, &[], &[]], dt, hyst };
    let mut times: Vec<f64> = crate::trig::find_all(model, &input).into_iter().map(|index| t0 + index * dt).collect();
    times.sort_by(f64::total_cmp);
    Arc::new(times)
}

/// Captured on the UI thread, evaluated by the existing SIM generation worker.
/// Only sources participating in the condition are copied; generated samples
/// are borrowed in place, never installed merely to start an asynchronous scan.
pub(super) struct SimTrigger {
    pub primary_slot: usize,
    model: Model,
    primary: Option<super::magnetic::WaveSamp>,
    secondary: Option<super::magnetic::WaveSamp>,
    secondary_generated: bool,
    hyst: [f32; NCH],
    generated_t0: f64,
}

impl SimTrigger {
    pub(super) fn evaluate(&self, generated: &[f32], generated_dt: f64) -> Arc<Vec<f64>> {
        let (primary, t0, dt) = self.primary.as_ref().map_or((generated, self.generated_t0, generated_dt), |wave| (wave.v.as_slice(), wave.t0, wave.dt));
        let secondary = if self.secondary_generated { Some((generated, self.generated_t0, generated_dt)) }
            else { self.secondary.as_ref().map(|wave| (wave.v.as_slice(), wave.t0, wave.dt)) };
        scan_condition(&self.model, primary, dt, t0, secondary, self.hyst)
    }
}

impl Job {
    fn run(self) -> ResultSet {
        let secondary = self.secondary.as_ref().map(|wave| (wave.v.as_slice(), wave.t0, wave.dt));
        ResultSet { key: self.key, times: scan_condition(&self.model, &self.primary, self.dt, self.t0, secondary, self.hyst) }
    }
}

struct ResultSet { key: String, times: Arc<Vec<f64>> }
struct Worker {
    input: mpsc::SyncSender<Job>,
    output: mpsc::Receiver<ResultSet>,
    busy: bool,
}

impl Worker {
    fn open() -> Self {
        let (input, requests) = mpsc::sync_channel::<Job>(1);
        let (results, output) = mpsc::channel();
        std::thread::Builder::new().name("reference-search".into()).spawn(move || {
            while let Ok(job) = requests.recv() {
                if results.send(job.run()).is_err() { break; }
            }
        }).expect("start bounded reference search worker");
        Self { input, output, busy: false }
    }
}

#[derive(Default)]
pub(super) struct Cache {
    key: String,
    requested_key: String,
    times: Option<Arc<Vec<f64>>>,
    worker: Option<Worker>,
    scans: usize,
}

impl Ui {
    fn normalized_reference_model(model: &Model) -> Model {
        let mut normalized = model.clone();
        normalized.trig_src = 0;
        normalized.trig_src2 = 1;
        if normalized.trig_type == TrigType::Bus { normalized.dec_src = 0; }
        normalized
    }
    fn source_stamp(&self, id: WaveId, candidate: Option<(usize, f32)>) -> Option<(usize, usize, u64, u64)> {
        match id {
            WaveId::Ref(i) if candidate.is_some_and(|(slot, _)| slot == i) => Some((0, 0, 0, self.source_time_shift(WaveId::Ref(i)).to_bits())),
            WaveId::Ref(i) => self.refs[i].as_ref().map(|r| (r.v.as_ptr() as usize, r.v.len(), r.dt.to_bits(), self.source_time_origin(id, r.t0).to_bits())),
            WaveId::Ch(c) => self.last[0].as_ref().map(|r| (r.ch[c].as_ptr() as usize, r.ch[c].len(), r.dt.to_bits(), self.source_time_origin(id, r.t0).to_bits())),
        }
    }
    fn source_vpd(&self, id: WaveId, candidate: Option<(usize, f32)>) -> f32 {
        match id {
            WaveId::Ref(i) if candidate.is_some_and(|(slot, _)| slot == i) => candidate.unwrap().1,
            WaveId::Ref(i) => self.refs[i].as_ref().map_or(1.0, |r| r.vpd),
            WaveId::Ch(c) => self.m.ch[c].volts_per_div(),
        }.max(1e-6)
    }
    pub(super) fn reference_trigger_fingerprint(&self, model: &Model, slot: usize, candidate: Option<(usize, f32)>) -> String {
        let normalized = Self::normalized_reference_model(model);
        let paired = matches!(model.trig_type, TrigType::Delay | TrigType::SetupHold);
        let second = model.trigger_wave2();
        let primary_hyst = self.source_vpd(WaveId::Ref(slot), candidate) * model.trig_hyst_div();
        let secondary_hyst = self.source_vpd(second, candidate) * model.trig_hyst_div();
        let sim_used = self.session_is_sim && (slot == self.m.sim_slot || (paired && second == WaveId::Ref(self.m.sim_slot)));
        format!("{:?}|{:?}|{:?}|{}|{:?}|{:?}|{:?}|{}|{}|{}|{}", normalized.trigger(), normalized.dec_cfg(),
            (model.trig_type, model.trig_bus, model.trig_bus_val), slot,
            self.source_stamp(WaveId::Ref(slot), candidate), paired.then(|| (second, self.source_stamp(second, candidate))), model.trigger_wave(),
            primary_hyst.to_bits(), secondary_hyst.to_bits(), self.workflow_epoch, if sim_used { self.replay.sim_attempt } else { 0 })
    }
    pub(super) fn prepare_sim_trigger(&self, model: &Model, slot: usize, vpd: f32) -> Option<SimTrigger> {
        let primary_slot = model.trig_ref?;
        let paired = matches!(model.trig_type, TrigType::Delay | TrigType::SetupHold);
        let second = model.trigger_wave2();
        if primary_slot != slot && !(paired && second == WaveId::Ref(slot)) { return None; }
        if matches!(model.trig_type, TrigType::Pattern | TrigType::Duration)
            || (model.trig_type == TrigType::Bus && (model.bus_on_la() || model.dec_proto.roles()[1].is_some() || model.dec_diff.is_some())) { return None; }
        let sample = |id| match id {
            WaveId::Ref(i) => self.refs[i].as_ref().map(|r| super::magnetic::WaveSamp { v: r.v.clone(), t0: self.source_time_origin(id, r.t0), dt: r.dt }),
            WaveId::Ch(_) => self.last[0].as_ref().and_then(|record| self.wave_samples(record, id)),
        };
        let primary = if primary_slot == slot { None } else { Some(sample(WaveId::Ref(primary_slot))?) };
        let secondary_generated = paired && second == WaveId::Ref(slot);
        let secondary = if paired && !secondary_generated { sample(second) } else { None };
        let primary_hyst = self.source_vpd(WaveId::Ref(primary_slot), Some((slot, vpd))) * model.trig_hyst_div();
        let secondary_hyst = self.source_vpd(second, Some((slot, vpd))) * model.trig_hyst_div();
        Some(SimTrigger { primary_slot, model: Self::normalized_reference_model(model), primary, secondary,
            secondary_generated, hyst: [primary_hyst, secondary_hyst, primary_hyst, primary_hyst],
            generated_t0: self.source_time_origin(WaveId::Ref(slot), 0.0) })
    }
    pub(super) fn install_reference_trigger_hits(&self, slot: usize, times: Arc<Vec<f64>>) {
        let key = self.reference_trigger_fingerprint(&self.m, slot, None);
        let mut cache = self.azr.reference_search.borrow_mut();
        cache.key = key.clone();
        cache.requested_key = key;
        cache.times = Some(times);
    }
    pub(super) fn reference_search_supported(&self) -> bool {
        if self.m.trig_type == TrigType::Bus && Self::decoder_uses_refs(&self.m) { return true; }
        !matches!(self.m.trig_type, TrigType::Pattern | TrigType::Duration)
            && !(self.m.trig_type == TrigType::Bus && (self.m.bus_on_la() || self.m.dec_proto.roles()[1].is_some()
                || self.m.dec_diff.is_some()))
    }

    pub(super) fn reference_search_busy(&self) -> bool {
        if self.m.trig_type == TrigType::Bus && Self::decoder_uses_refs(&self.m) { return self.decode_busy(); }
        let cache = self.azr.reference_search.borrow();
        cache.worker.as_ref().is_some_and(|worker| worker.busy) && (cache.key != cache.requested_key || cache.times.is_none())
    }

    /// Times are absolute record times, never viewport-relative sample indices.
    /// Panning does not enter this cache key. Large scans retain one input copy
    /// in one worker; changed conditions wait without accumulating stale jobs.
    pub(super) fn reference_trigger_hits(&self, slot: usize) -> Option<Arc<Vec<f64>>> {
        if self.m.trig_type == TrigType::Bus && Self::decoder_uses_refs(&self.m) {
            return self.decoder_trigger_hits(&self.m);
        }
        if !self.reference_search_supported() { return None; }
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !r.dt.is_finite() || r.dt <= 0.0 || !r.t0.is_finite() { return None; }
        let model = Self::normalized_reference_model(&self.m);
        let paired = matches!(model.trig_type, TrigType::Delay | TrigType::SetupHold);
        let second = self.m.trigger_wave2();
        let primary_hyst = r.vpd.max(1e-6) * self.m.trig_hyst_div();
        let secondary_hyst = self.trigger_vpd(true).max(1e-6) * self.m.trig_hyst_div();
        let key = self.reference_trigger_fingerprint(&self.m, slot, None);
        let mut cache = self.azr.reference_search.borrow_mut();
        if cache.requested_key != key { cache.requested_key = key.clone(); }
        let completed = cache.worker.as_mut().and_then(|worker| worker.output.try_recv().ok().map(|result| { worker.busy = false; result }));
        if let Some(result) = completed {
            if result.key == key { cache.key = result.key; cache.times = Some(result.times); }
        }
        if cache.key == key { return cache.times.as_ref().map(Arc::clone); }
        if cache.worker.as_ref().is_some_and(|worker| worker.busy) { return None; }
        let auxiliary = if paired {
            match second {
                WaveId::Ref(i) => self.refs[i].as_ref().map(|r| super::magnetic::WaveSamp { v: r.v.clone(), t0: self.source_time_origin(second, r.t0), dt: r.dt }),
                WaveId::Ch(_) => self.last[0].as_ref().and_then(|rec| self.wave_samples(rec, second)),
            }
        } else { None };
        let job = Job { key, model, primary: r.v.clone(), secondary: auxiliary, dt: r.dt, t0: self.source_time_origin(WaveId::Ref(slot), r.t0),
            hyst: [primary_hyst, secondary_hyst, primary_hyst, primary_hyst] };
        cache.scans += 1;
        if r.v.len() > BACKGROUND_POINTS {
            let worker = cache.worker.get_or_insert_with(Worker::open);
            if worker.input.try_send(job).is_ok() { worker.busy = true; }
            None
        } else {
            let result = job.run();
            cache.key = result.key;
            cache.times = Some(result.times);
            cache.times.as_ref().map(Arc::clone)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn reference(dt: f64, t0: f64, samples: Vec<f32>) -> RefWave {
        RefWave { src: 0, color: 0, label: "Search fixture".into(), t0, dt, v: samples, vpd: 0.5, offset: 0.0, on: true }
    }

    fn fixture(points: usize) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_offline(true);
        let values = || (0..points).map(|i| if i % 16 < 8 { -1.0 } else { 1.0 }).collect();
        ui.refs[0] = Some(reference(1e-9, -5e-6, values()));
        ui.refs[1] = Some(reference(2e-9, -7e-6, values()));
        ui.m.set_trigger_wave(WaveId::Ref(1), false);
        ui.m.trig_type = TrigType::Edge;
        ui.m.slope = Slope::Rise;
        ui.m.trig_level = 0.0;
        ui.m.status = TrigStatus::Stop;
        ui
    }

    fn wait(ui: &Ui) -> Arc<Vec<f64>> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(times) = ui.reference_trigger_hits(ui.m.trig_ref.unwrap()) { return times; }
            assert!(Instant::now() < deadline, "bounded reference search did not complete");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn stopped_reference_search_uses_physical_times_and_reuses_hits_during_pan() {
        let mut ui = fixture(1600);
        assert!(ui.last[0].is_none());
        ui.replay.captures = 9;
        ui.mstat = vec![MStat { n: 7, sum: 14.0, ..MStat::default() }];
        ui.adjust(Item::SearchOn, 1);
        let events = ui.search_events();
        assert_eq!(events.len(), 100);
        assert!((events[0] - (-7e-6 + 7.5 * 2e-9)).abs() < 1e-15);
        assert!((events[99] - (-7e-6 + (7.5 + 99.0 * 16.0) * 2e-9)).abs() < 1e-15);
        for i in 0..100 {
            ui.m.delay = i as f32 * 1e-9;
            let again = ui.search_events();
            assert!(Arc::ptr_eq(&events, &again), "panning must reuse the same cached full-record hit list");
        }
        assert_eq!(ui.azr.reference_search.borrow().scans, 1);
        assert_eq!(ui.replay.captures, 9);
        assert_eq!((ui.mstat[0].n, ui.mstat[0].sum), (7, 14.0));
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.m.status, TrigStatus::Stop);
        assert!(ui.last[0].is_none());
        ui.m.set_trigger_wave(WaveId::Ref(0), false);
        let other = ui.search_events();
        assert_eq!(other.len(), 100);
        assert!((other[0] - (-5e-6 + 7.5e-9)).abs() < 1e-15);
    }

    #[test]
    fn runt_search_counts_only_incomplete_pulses_and_replay_shares_the_cache() {
        let mut ui = fixture(1000);
        let pulse = [0.0, 0.0, 0.6, 0.6, 0.0, 0.0, 1.2, 1.2, 0.0, 0.0];
        ui.refs[1] = Some(reference(20e-12, -1000e-9, (0..1000).map(|i| pulse[i % pulse.len()]).collect()));
        ui.m.trig_type = TrigType::Runt;
        ui.m.trig_level = 0.9;
        ui.m.trig_level2 = 0.3;
        ui.m.trig_pol = Pol::Pos;
        ui.m.trig_when = WhenKind::Off;
        let events = ui.search_events();
        assert_eq!(events.len(), 100);
        assert!((events[0] - (-1000e-9 + 3.5 * 20e-12)).abs() < 1e-15);
        ui.m.run = RunState::Run;
        ui.m.trig_mode = TrigMode::Single;
        ui.sim_play = true;
        ui.replay_acquire(0.01);
        assert_eq!(ui.replay.captures, 1);
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.m.status, TrigStatus::Triggered);
        assert_eq!(ui.azr.reference_search.borrow().scans, 1, "Search and replay use one condition cache");
        assert!(events.iter().any(|&time| Some(time) == ui.replay.trigger_time));
    }

    #[test]
    fn long_search_has_one_worker_rejects_stale_conditions_and_stays_stopped() {
        let mut ui = fixture(BACKGROUND_POINTS + 1024);
        assert!(ui.reference_trigger_hits(1).is_none());
        assert!(ui.reference_search_busy());
        ui.replay = Default::default();
        assert!(ui.reference_search_busy(), "resetting replay must retain the one shared worker");
        ui.m.trig_level = 2.0;
        for i in 0..100 {
            ui.m.delay = i as f32 * 1e-9;
            let _ = ui.search_events();
        }
        let empty = wait(&ui);
        assert!(empty.is_empty(), "old threshold results must not appear as current Search hits");
        assert_eq!(ui.azr.reference_search.borrow().scans, 2, "one old job and one current job, without queued copies per pan");
        let before = ui.azr.reference_search.borrow().scans;
        for _ in 0..50 { assert!(Arc::ptr_eq(&empty, &ui.search_events())); }
        assert_eq!(ui.azr.reference_search.borrow().scans, before);
        assert_eq!(ui.replay.captures, 0);
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.m.status, TrigStatus::Stop);
    }

    #[test]
    fn reference_search_does_not_fabricate_pattern_or_multiwire_bus_inputs() {
        let mut ui = fixture(1600);
        ui.m.trig_type = TrigType::Pattern;
        assert!(!ui.reference_search_supported());
        assert!(ui.search_events().is_empty());
        ui.m.trig_type = TrigType::Bus;
        ui.m.dec_proto = Proto::Spi;
        assert!(!ui.reference_search_supported());
        assert!(ui.search_events().is_empty());
        assert_eq!(ui.azr.reference_search.borrow().scans, 0);
    }

    #[test]
    fn changing_bus_match_conditions_cannot_reuse_an_edge_or_previous_packet_hit_list() {
        let mut ui = fixture(1600);
        let edge = ui.search_events();
        ui.m.trig_type = TrigType::Bus;
        ui.m.dec_proto = Proto::Uart;
        ui.m.trig_bus = BusWhat::Error;
        let errors = ui.search_events();
        assert!(!Arc::ptr_eq(&edge, &errors));
        ui.m.trig_bus = BusWhat::Data;
        ui.m.trig_bus_val = 0x55;
        let data = ui.search_events();
        assert!(!Arc::ptr_eq(&errors, &data));
        ui.m.trig_bus_val = 0xaa;
        assert!(!Arc::ptr_eq(&data, &ui.search_events()));
        assert_eq!(ui.azr.reference_search.borrow().scans, 4);
    }

    #[test]
    fn sim_trigger_snapshot_keeps_external_primary_grid_and_uses_generated_secondary_samples() {
        let mut ui = fixture(1600);
        let primary = ui.refs[1].as_mut().unwrap();
        primary.t0 = 8e-9;
        primary.dt = 2e-9;
        ui.m.trig_type = TrigType::Delay;
        ui.m.set_trigger_wave(WaveId::Ref(1), false);
        ui.m.set_trigger_wave(WaveId::Ref(0), true);
        ui.m.slope = Slope::Rise;
        ui.m.trig_slope2 = Slope::Rise;
        ui.m.trig_level = 0.0;
        ui.m.trig_level2 = 0.0;
        ui.m.trig_when = WhenKind::Gt;
        ui.m.trig_t1 = 1e-9;
        let plan = ui.prepare_sim_trigger(&ui.m, 0, 0.5).unwrap();
        assert_eq!(plan.primary_slot, 1);
        let generated: Vec<_> = (0..4096).map(|i| if i % 32 < 12 { -1.0 } else { 1.0 }).collect();
        let times = plan.evaluate(&generated, 1e-9);
        assert_eq!(times.len(), 99);
        assert!((times[0] - 43e-9).abs() < 1e-15);
        assert!((times[98] - 3179e-9).abs() < 1e-15);
    }
}
