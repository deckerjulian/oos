// SPDX-License-Identifier: GPL-2.0-or-later
//! Bounded catalog jobs over immutable sources; no waveform DSP on the render thread.
use super::*;
use crate::ui::worker::Worker;
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

type Row = (u16, WaveId, WaveId, WaveId, WaveId);
const MAX_POINTS: usize = 32_000_000;
const MAX_ALIGNED_BYTES: usize = 256 << 20;
const MAX_SMALL_COPY: usize = 262_144;
/// The module of a measurement: what is computed together and cached under one key.
fn module_of(c: Cat) -> usize {
    measures::find(c.id).map_or(0, |x| x.0)
}
#[derive(Clone, PartialEq, Eq, Debug)]
struct SourceKey {
    id: WaveId,
    version: u64,
    lo: usize,
    hi: usize,
    len: usize,
    t0: u64,
    dt: u64,
    capture: u64,
    conditioning: (u8, bool, usize, u8),
}
#[derive(Clone, PartialEq, Eq, Debug)]
struct Key {
    module: usize,
    sources: Vec<SourceKey>,
    roles: Vec<WaveId>,
    settings: Vec<u64>,
    epoch: u64,
    full: bool,
}
#[derive(Clone)]
enum Samples {
    Reference(Arc<Vec<f32>>),
    Native(Arc<crate::board::Frame>, usize),
    Small(Arc<Vec<f32>>),
}
impl Samples {
    fn values(&self) -> &[f32] {
        match self {
            Self::Reference(v) | Self::Small(v) => v,
            Self::Native(f, c) => &f.ch[*c],
        }
    }
}
#[derive(Clone)]
struct Input {
    id: WaveId,
    samples: Samples,
    lo: usize,
    hi: usize,
    t0: f64,
    dt: f64,
}
impl Input {
    fn start(&self) -> f64 {
        self.t0 + self.lo as f64 * self.dt
    }
    fn end(&self) -> f64 {
        self.t0 + (self.hi - 1) as f64 * self.dt
    }
}
enum Owner {
    Reference(Weak<Vec<f32>>),
    Native(Weak<crate::board::Frame>),
    Small(usize, u64, u64),
}
struct Version {
    id: WaveId,
    owner: Owner,
    version: u64,
}
impl Owner {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Reference(a), Self::Reference(b)) => Weak::ptr_eq(a, b),
            (Self::Native(a), Self::Native(b)) => Weak::ptr_eq(a, b),
            (Self::Small(a, ac, ag), Self::Small(b, bc, bg)) => a == b && ac == bc && ag == bg,
            _ => false,
        }
    }
}
struct Job {
    key: Key,
    row: Row,
    inputs: Vec<Input>,
    model: Model,
    capture: u64,
    stats_epoch: u64,
}
struct Batch {
    values: Vec<(u16, f64)>,
    quality: &'static str,
}
impl std::ops::Deref for Batch {
    type Target = [(u16, f64)];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
struct Finished {
    key: Key,
    result: Result<Batch, &'static str>,
    capture: u64,
    stats_epoch: u64,
}
fn open_worker() -> Worker<Job, Finished> {
    Worker::spawn("catalog-measurements", |job: Job| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compute(&job)))
            .unwrap_or(Err("Catalog analysis failed"));
        Finished { key: job.key, result, capture: job.capture, stats_epoch: job.stats_epoch }
    })
    .expect("start catalog worker")
}
#[derive(Default)]
pub(in crate::ui) struct State {
    versions: Vec<Version>,
    next_version: u64,
    cache: VecDeque<Finished>,
    pending: VecDeque<(Key, Row)>,
    worker: Option<Worker<Job, Finished>>,
    submitted: usize,
    stats_epoch: u64,
    stats: Vec<(Row, u64, Key, u64)>,
    small: Vec<Small>,
}

/// A wave kept for the small measurements: (wave, pointer, frame count, statistics epoch, samples).
type Small = (WaveId, usize, u64, u64, Arc<Vec<f32>>);
pub(in crate::ui) struct Reading {
    pub value: f64,
    pub computing: bool,
    pub reason: Option<&'static str>,
    pub quality: &'static str,
}
impl State {
    fn version(&mut self, id: WaveId, owner: Owner) -> u64 {
        if let Some(old) = self.versions.iter().find(|old| old.id == id && old.owner.same(&owner)) {
            return old.version;
        }
        self.next_version = self.next_version.checked_add(1).expect("catalog source version exhausted");
        self.versions.retain(|old| old.id != id);
        self.versions.push(Version { id, owner, version: self.next_version });
        self.next_version
    }
    fn put(&mut self, answer: Finished) {
        self.cache.retain(|old| old.key != answer.key);
        self.pending.retain(|(key, _)| *key != answer.key);
        self.cache.push_front(answer);
        self.cache.truncate(tools::MEAS_MAX);
    }
}
fn settings(m: &Model, module: usize) -> Vec<u64> {
    let mut v = vec![];
    measures::get(module).settings(m, &mut v);
    v
}
fn role_ids(c: Cat, row: [WaveId; 4], m: &Model) -> Result<Vec<WaveId>, &'static str> {
    let mut ids = catalog_roles(c).iter().map(|&r| row[r]).collect::<Vec<_>>();
    measures::get(module_of(c)).sources(c.id, row, m, &mut ids)?;
    if ids.is_empty() {
        return Err("Catalog source roles missing");
    }
    Ok(ids)
}
impl Ui {
    fn catalog_input(&self, id: WaveId, state: &mut State) -> Result<(Input, SourceKey), &'static str> {
        let (samples, len, t0, dt, owner, capture, conditioning) = match id {
            WaveId::Ref(slot) => {
                let r = self.refs.get(slot).and_then(Option::as_ref).ok_or("Reference source empty")?;
                let revision =
                    if self.session_is_sim && self.m.sim_slot == slot { self.replay.sim_revision } else { 0 };
                (
                    Samples::Reference(Arc::clone(&r.v)),
                    r.v.len(),
                    self.source_time_origin(id, r.t0),
                    r.dt,
                    Owner::Reference(Arc::downgrade(&r.v)),
                    revision,
                    (0, false, 0, 0),
                )
            }
            WaveId::Ch(c) => {
                if c >= n_ch() || !self.m.ch[c].on {
                    return Err("Channel source is off");
                }
                let record = self.last[0].as_ref().ok_or("No acquired record")?;
                let config = &self.m.ch[c];
                let conditioning = (config.coupling as u8, config.invert, config.bw, self.m.acq as u8);
                if let Some(raw) = record
                    .raw
                    .as_ref()
                    .filter(|raw| self.m.acq != AcqMode::HiRes && !raw.frame.envelope && raw.frame.ch[c].len() >= 2)
                {
                    (
                        Samples::Native(Arc::clone(&raw.frame), c),
                        raw.frame.ch[c].len(),
                        self.source_time_origin(id, raw.t0 - raw.i0 as f64 * raw.frame.dt),
                        raw.frame.dt,
                        Owner::Native(Arc::downgrade(&raw.frame)),
                        raw.frame.seq,
                        conditioning,
                    )
                } else {
                    let values = &record.ch[c];
                    if values.len() > MAX_SMALL_COPY {
                        return Err("Large processed channel needs an immutable captured record");
                    }
                    // Processed fallback is bounded to 1MiB. A Clear/reset generation is part
                    // of both owner and snapshot identities because capture numbers can restart.
                    // Native long records never enter this copy path.
                    (
                        {
                            let pointer = values.as_ptr() as usize;
                            if state.small.iter().all(|(id, p, capture, generation, _)| {
                                *id != WaveId::Ch(c)
                                    || *p != pointer
                                    || *capture != self.wfm_total
                                    || *generation != state.stats_epoch
                            }) {
                                state.small.retain(|(id, _, _, _, _)| *id != WaveId::Ch(c));
                                state.small.push((
                                    WaveId::Ch(c),
                                    pointer,
                                    self.wfm_total,
                                    state.stats_epoch,
                                    Arc::new(values.clone()),
                                ));
                            }
                            Samples::Small(Arc::clone(
                                &state.small.iter().find(|(id, _, _, _, _)| *id == WaveId::Ch(c)).unwrap().4,
                            ))
                        },
                        values.len(),
                        self.source_time_origin(id, record.t0),
                        record.dt,
                        Owner::Small(values.as_ptr() as usize, self.wfm_total, state.stats_epoch),
                        self.wfm_total,
                        conditioning,
                    )
                }
            }
        };
        if !(2..=MAX_POINTS).contains(&len) || !t0.is_finite() || !dt.is_finite() || dt <= 0.0 {
            return Err("Invalid waveform source");
        }
        let (lo, hi) = self.measurement_bounds(len, t0, dt);
        if hi < lo + 2 {
            return Err("No samples in measurement gate");
        }
        let version = state.version(id, owner);
        Ok((
            Input { id, samples, lo, hi, t0, dt },
            SourceKey { id, version, lo, hi, len, t0: t0.to_bits(), dt: dt.to_bits(), capture, conditioning },
        ))
    }
    fn catalog_snapshot(&self, c: Cat, row: [WaveId; 4]) -> Result<(Key, Vec<Input>), &'static str> {
        if c.kind != Kind::Module {
            return Err("Catalog entry uses a legacy measurement");
        }
        let roles = role_ids(c, row, &self.m)?;
        let mut state = self.azr.catalog_measurements.borrow_mut();
        let mut inputs = Vec::new();
        let mut keys = Vec::new();
        for &id in &roles {
            if inputs.iter().any(|input: &Input| input.id == id) {
                continue;
            }
            let (input, key) = self.catalog_input(id, &mut state)?;
            inputs.push(input);
            keys.push(key);
        }
        Ok((
            Key {
                module: module_of(c),
                sources: keys,
                roles,
                settings: settings(&self.m, module_of(c)),
                epoch: self.workflow_epoch,
                full: self.m.meas_full_record,
            },
            inputs,
        ))
    }
    pub(in crate::ui) fn catalog_reading(&self, c: Cat, row: [WaveId; 4]) -> Reading {
        let module = measures::get(module_of(c));
        let quality = module.pending_quality(c.id);
        let (key, inputs) = match self.catalog_snapshot(c, row) {
            Ok(value) => value,
            Err(reason) => return Reading { value: f64::NAN, computing: false, reason: Some(reason), quality },
        };
        let mut state = self.azr.catalog_measurements.borrow_mut();
        if let Some(answer) = state.cache.iter().find(|answer| answer.key == key) {
            return match &answer.result {
                Ok(values) => {
                    let value = values.iter().find(|(id, _)| *id == c.id).map_or(f64::NAN, |(_, v)| *v);
                    Reading {
                        value,
                        computing: false,
                        reason: if !value.is_finite() { module.missing_reason(c.id) } else { None },
                        quality: values.quality,
                    }
                }
                Err(reason) => Reading { value: f64::NAN, computing: false, reason: Some(*reason), quality },
            };
        }
        let tuple = (c.id, row[0], row[1], row[2], row[3]);
        if !state.pending.iter().any(|(old, _)| *old == key) {
            state.pending.push_back((key.clone(), tuple));
            while state.pending.len() > tools::MEAS_MAX {
                state.pending.pop_front();
            }
        }
        let stats_epoch = state.stats_epoch;
        let state = &mut *state;
        let worker = state.worker.get_or_insert_with(open_worker);
        if !worker.busy() {
            let job = Job { key, row: tuple, inputs, model: self.m.clone(), capture: self.wfm_total, stats_epoch };
            if worker.submit(job).is_ok() {
                state.submitted += 1;
            }
        }
        Reading { value: f64::NAN, computing: true, reason: None, quality }
    }
    pub(in crate::ui) fn catalog_measurement_busy(&self) -> bool {
        let pending = self.azr.catalog_measurements.borrow().pending.iter().map(|(_, r)| *r).collect::<Vec<_>>();
        pending
            .iter()
            .any(|&(id, a, b, c, d)| cat_from(id).is_some_and(|cat| self.catalog_snapshot(cat, [a, b, c, d]).is_ok()))
    }
    pub(in crate::ui) fn queue_catalog_statistics(&self, row: Row, capture: u64) {
        let (id, a, b, c, d) = row;
        let Some(cat) = cat_from(id) else { return };
        let Ok((key, _)) = self.catalog_snapshot(cat, [a, b, c, d]) else { return };
        let mut state = self.azr.catalog_measurements.borrow_mut();
        state.stats.retain(|(stored, _, _, _)| *stored != row);
        let epoch = state.stats_epoch;
        state.stats.push((row, capture, key, epoch));
        state.stats.truncate(tools::MEAS_MAX);
    }
    pub(in crate::ui) fn reset_catalog_statistics(&self) {
        let mut state = self.azr.catalog_measurements.borrow_mut();
        state.stats.clear();
        state.stats_epoch = state.stats_epoch.checked_add(1).expect("catalog statistics generation exhausted");
    }
    pub(in crate::ui) fn poll_catalog_measurements(&mut self) {
        let finished = {
            let mut state = self.azr.catalog_measurements.borrow_mut();
            state.worker.as_mut().and_then(|worker| worker.poll().ok())
        };
        let mut changed = false;
        if let Some(answer) = finished {
            let rows = self.m.cat.clone();
            let current = rows.iter().any(|&(id, a, b, c, d)| {
                cat_from(id)
                    .is_some_and(|cat| self.catalog_snapshot(cat, [a, b, c, d]).is_ok_and(|(key, _)| key == answer.key))
            });
            let mut accepted = Vec::new();
            {
                let mut state = self.azr.catalog_measurements.borrow_mut();
                state.pending.retain(|(key, _)| *key != answer.key);
                if current {
                    if let Ok(values) = &answer.result {
                        for (row, capture, key, epoch) in &state.stats {
                            if *key == answer.key
                                && *capture == answer.capture
                                && *capture == self.wfm_total
                                && *epoch == answer.stats_epoch
                                && *epoch == state.stats_epoch
                            {
                                if let Some((_, value)) = values.iter().find(|(id, _)| *id == row.0) {
                                    accepted.push((*row, *value, *capture));
                                }
                            }
                        }
                    }
                    state.stats.retain(|(_, capture, key, _)| *capture == self.wfm_total && *key != answer.key);
                    state.put(answer);
                    changed = true;
                }
            }
            for (row, value, capture) in accepted {
                self.accept_catalog_stat(row, value, capture);
            }
        }
        let pending = self.azr.catalog_measurements.borrow().pending.iter().map(|(_, row)| *row).collect::<Vec<_>>();
        for row @ (id, a, b, c, d) in pending {
            let current =
                cat_from(id).and_then(|cat| self.catalog_snapshot(cat, [a, b, c, d]).ok().map(|(key, _)| key));
            let mut state = self.azr.catalog_measurements.borrow_mut();
            state.pending.retain(|(key, stored)| *stored != row || Some(key) == current.as_ref());
        }
        let next = self.azr.catalog_measurements.borrow().pending.front().cloned();
        if let Some((_, (id, a, b, c, d))) = next {
            if let Some(cat) = cat_from(id) {
                let _ = self.catalog_reading(cat, [a, b, c, d]);
            }
        }
        if changed {
            self.finish_deferred_capture_statistics();
            self.dirty_map = true;
        }
    }
}
fn snap(x: f64) -> f64 {
    if (x - x.round()).abs() < 1e-7 {
        x.round()
    } else {
        x
    }
}
#[derive(Debug)]
struct Aligned {
    dt: f64,
    t0: f64,
    inputs: Vec<(WaveId, Vec<f32>)>,
}
fn align_inputs(inputs: &[Input]) -> Result<Aligned, &'static str> {
    let slow = inputs.iter().max_by(|a, b| a.dt.total_cmp(&b.dt)).ok_or("No source roles")?;
    let start = inputs.iter().map(Input::start).fold(f64::NEG_INFINITY, f64::max);
    let end = inputs.iter().map(Input::end).fold(f64::INFINITY, f64::min);
    let dt = slow.dt;
    let first = snap((start - slow.t0) / dt).ceil();
    let last = snap((end - slow.t0) / dt).floor();
    if !first.is_finite() || !last.is_finite() || last - first < 1.0 {
        return Err("Catalog sources do not overlap");
    }
    let n = (last - first + 1.0) as usize;
    if n > MAX_POINTS
        || n.checked_mul(inputs.len()).and_then(|n| n.checked_mul(4)).is_none_or(|bytes| bytes > MAX_ALIGNED_BYTES)
    {
        return Err("Catalog alignment exceeds memory bound; shorten measurement gate");
    }
    let t0 = slow.t0 + first * dt;
    let mut out = Vec::new();
    for input in inputs {
        let values = input.samples.values();
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            let x = snap((t0 + i as f64 * dt - input.t0) / input.dt);
            if x < input.lo as f64 || x > (input.hi - 1) as f64 {
                return Err("Source outside common physical interval");
            }
            let k = x.floor() as usize;
            let fraction = (x - k as f64) as f32;
            let value = if fraction == 0.0 { values[k] } else { values[k] + (values[k + 1] - values[k]) * fraction };
            data.push(value);
        }
        out.push((input.id, data));
    }
    Ok(Aligned { dt, t0, inputs: out })
}
/// A job on the worker: the module computes every measurement of the row's that reads the same roles.
fn compute(job: &Job) -> Result<Batch, &'static str> {
    let aligned = align_inputs(&job.inputs)?;
    let (id, a, b, c, d) = job.row;
    let asked = cat_from(id).ok_or("Unknown catalog measurement")?;
    let module = measures::get(job.key.module);
    let ids: Vec<u16> =
        module.defs().iter().filter(|x| x.kind == Kind::Module && x.roles == asked.roles).map(|x| x.id).collect();
    let ctx =
        measures::Ctx { model: &job.model, t0: aligned.t0, dt: aligned.dt, row: [a, b, c, d], waves: &aligned.inputs };
    let out = module.compute(&ctx, &ids);
    Ok(Batch { values: out.values, quality: out.quality })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn input(id: usize, t0: f64, dt: f64, v: Vec<f32>) -> Input {
        let hi = v.len();
        Input { id: WaveId::Ref(id), samples: Samples::Reference(Arc::new(v)), lo: 0, hi, t0, dt }
    }
    fn fixture() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.ch.iter_mut().for_each(|c| c.on = false);
        ui.m.meas.clear();
        ui.m.meas2.clear();
        ui.m.cat.clear();
        ui.m.meas_full_record = true;
        ui
    }
    fn put(ui: &mut Ui, r: usize, v: Vec<f32>, t0: f64, dt: f64) {
        ui.refs[r] = Some(RefWave {
            src: 0,
            color: r,
            label: String::new(),
            t0,
            dt,
            v: v.into(),
            vpd: 1.0,
            offset: 0.0,
            on: true,
        });
    }
    fn row(c: Cat) -> Row {
        (c.id, WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3))
    }
    fn wait(ui: &mut Ui, c: Cat) -> Reading {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            ui.poll_catalog_measurements();
            let r = ui.catalog_reading(c, [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)]);
            if !r.computing {
                return r;
            }
            assert!(Instant::now() < end, "catalog never completed");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn alignment_uses_common_intersection_slowest_grid_and_subsample_origins() {
        let a = input(0, -2.0, 1.0, vec![0., 1., 2., 3., 4., 5.]);
        let b = input(1, -1.5, 0.5, vec![0., 1., 2., 3., 4., 5., 6., 7.]);
        let v = align_inputs(&[a, b]).unwrap();
        assert_eq!((v.t0, v.dt), (-1.0, 1.0));
        assert_eq!(v.inputs[0].1, [1., 2., 3., 4.]);
        assert_eq!(v.inputs[1].1, [1., 3., 5., 7.]);
        assert!(align_inputs(&[input(0, 0., 1., vec![0., 1.]), input(1, 10., 1., vec![0., 1.])])
            .unwrap_err()
            .contains("overlap"));
    }
    #[test]
    fn gate_and_fourth_probe_join_all_dpt_roles_before_alignment() {
        let mut ui = fixture();
        for r in 0..4 {
            put(&mut ui, r, vec![1.; if r == 2 { 4 } else { 20 }], if r == 2 { 5.0 } else { 0.0 }, 1.0);
        }
        ui.m.dpt_4th = DptFourth::Gate;
        ui.m.dpt_4ch = WaveId::Ref(3);
        let (_, inputs) = ui
            .catalog_snapshot(
                tile(measures::double_pulse::VPEAK),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)],
            )
            .unwrap();
        assert_eq!(inputs.len(), 4);
        let aligned = align_inputs(&inputs).unwrap();
        assert_eq!((aligned.t0, aligned.dt, aligned.inputs[0].1.len()), (5.0, 1.0, 4));
        ui.refs[3].as_mut().unwrap().t0 = 20.0;
        let (_, inputs) = ui
            .catalog_snapshot(
                tile(measures::double_pulse::VPEAK),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)],
            )
            .unwrap();
        assert!(align_inputs(&inputs).is_err());
    }
    #[test]
    fn families_queue_once_share_immutable_data_and_never_count_stopped_reads() {
        let mut ui = fixture();
        put(&mut ui, 0, (0..400_000).map(|i| if i % 40 < 20 { -0.5 } else { 0.5 }).collect(), 0.0, 1e-9);
        ui.m.cat = vec![row(tile(measures::jitter::EYE_HEIGHT)), row(tile(measures::jitter::TIE))];
        for _ in 0..100 {
            let _ = ui.cat_row(
                row(tile(measures::jitter::EYE_HEIGHT)).0,
                row(tile(measures::jitter::EYE_HEIGHT)).1,
                row(tile(measures::jitter::EYE_HEIGHT)).2,
                row(tile(measures::jitter::EYE_HEIGHT)).3,
                row(tile(measures::jitter::EYE_HEIGHT)).4,
            );
            let _ = ui.cat_row(
                row(tile(measures::jitter::TIE)).0,
                row(tile(measures::jitter::TIE)).1,
                row(tile(measures::jitter::TIE)).2,
                row(tile(measures::jitter::TIE)).3,
                row(tile(measures::jitter::TIE)).4,
            );
        }
        assert_eq!(ui.azr.catalog_measurements.borrow().submitted, 1, "100 reads queue one job");
        assert!(Arc::strong_count(&ui.refs[0].as_ref().unwrap().v) >= 2, "worker must hold the same immutable source");
        let height = wait(&mut ui, tile(measures::jitter::EYE_HEIGHT));
        assert!(height.value.is_finite());
        assert!(height.quality.contains("NRZ"));
        assert!(wait(&mut ui, tile(measures::jitter::TIE)).value.is_finite());
        assert_eq!(ui.azr.catalog_measurements.borrow().submitted, 1);
        assert_eq!((ui.wfm_total, ui.replay.captures, ui.m.run), (0, 0, RunState::Stop));
        assert!(ui.measurement_statistics_snapshot_for_test().iter().all(|(n, _)| *n == 0));
        ui.m.delay = 100.;
        assert!(
            !ui.catalog_reading(
                tile(measures::jitter::TIE),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)]
            )
            .computing
        );
        assert_eq!(ui.azr.catalog_measurements.borrow().submitted, 1);
    }
    #[test]
    fn replacing_unsampled_interior_data_discards_old_answer_and_has_unique_owner_version() {
        let mut ui = fixture();
        put(&mut ui, 0, vec![0.; 400_000], 0.0, 1e-9);
        ui.m.cat = vec![row(tile(measures::power::RIPPLE))];
        assert!(
            ui.catalog_reading(
                tile(measures::power::RIPPLE),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)]
            )
            .computing
        );
        let old = ui
            .catalog_snapshot(
                tile(measures::power::RIPPLE),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)],
            )
            .unwrap()
            .0;
        Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v)[123_456] = 7.;
        let new = ui
            .catalog_snapshot(
                tile(measures::power::RIPPLE),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)],
            )
            .unwrap()
            .0;
        assert_ne!(old, new);
        let reading = wait(&mut ui, tile(measures::power::RIPPLE));
        assert_eq!(reading.value, 7.);
        assert!(ui.azr.catalog_measurements.borrow().cache.iter().all(|entry| entry.key != old));
        // No full source owner is held by the compact result cache. A retained Weak
        // still forces make_mut to disassociate and receive a new monotonic ID.
        Arc::make_mut(&mut ui.refs[0].as_mut().unwrap().v)[123_457] = 9.;
        let newest = ui
            .catalog_snapshot(
                tile(measures::power::RIPPLE),
                [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)],
            )
            .unwrap()
            .0;
        assert_ne!(newest, new);
        assert_eq!(wait(&mut ui, tile(measures::power::RIPPLE)).value, 9.);
        assert_eq!(ui.wfm_total, 0);
    }
    #[test]
    fn screen_pan_changes_physical_key_full_pan_and_unused_roles_do_not() {
        let mut ui = fixture();
        put(&mut ui, 0, vec![0.; 10_000], -5e-6, 1e-9);
        let ids = [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)];
        let full = ui.catalog_snapshot(tile(measures::power::RIPPLE), ids).unwrap().0;
        ui.m.delay = 1e-6;
        assert_eq!(ui.catalog_snapshot(tile(measures::power::RIPPLE), ids).unwrap().0, full);
        ui.m.meas_full_record = false;
        ui.m.tscale = tscales().iter().position(|&v| (v - 100e-9).abs() < 1e-14).unwrap();
        ui.m.delay = 0.;
        let screen = ui.catalog_snapshot(tile(measures::power::RIPPLE), ids).unwrap().0;
        ui.m.delay = 1e-6;
        assert_ne!(ui.catalog_snapshot(tile(measures::power::RIPPLE), ids).unwrap().0, screen);
        assert_eq!(
            ui.catalog_snapshot(tile(measures::power::RIPPLE), [ids[0], WaveId::Ch(3), ids[2], ids[3]]).unwrap().0,
            ui.catalog_snapshot(tile(measures::power::RIPPLE), ids).unwrap().0
        );
    }
    #[test]
    fn missing_data_and_alignment_budget_are_explicit_not_zero() {
        let mut ui = fixture();
        put(&mut ui, 0, vec![0.; 4000], 0., 1e-9);
        ui.m.cat = vec![row(tile(measures::jitter::EYE_HEIGHT))];
        let r = wait(&mut ui, tile(measures::jitter::EYE_HEIGHT));
        assert!(r.value.is_nan());
        let missing = ui.catalog_reading(
            tile(measures::double_pulse::VPEAK),
            [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)],
        );
        assert!(!missing.computing && missing.value.is_nan() && missing.reason.is_some());
        // Metadata-only impossible span verifies the guard runs before indexing/allocation.
        let a = Input {
            id: WaveId::Ref(0),
            samples: Samples::Reference(Arc::new(vec![0., 1.])),
            lo: 0,
            hi: MAX_POINTS + 1,
            t0: 0.,
            dt: 1.,
        };
        assert!(align_inputs(&[a]).unwrap_err().contains("bound"));
    }
    #[test]
    fn every_wave_id_completes_through_backend_with_missing_domain_data_not_placeholder_values() {
        let mut ui = fixture();
        for r in 0..4 {
            put(&mut ui, r, vec![1.; 4000], 0., 1e-9);
        }
        let catalog = listed()
            .into_iter()
            .filter(|&c| {
                c.kind == Kind::Module && measures::find(c.id).is_some_and(|x| measures::get(x.0).key() != "example")
            })
            .collect::<Vec<_>>();
        assert_eq!(catalog.len(), 83);
        ui.m.cat = catalog.iter().map(|&c| row(c)).collect();
        for &cat in &catalog {
            let r = wait(&mut ui, cat);
            assert!(!r.computing);
        }
        assert_eq!(wait(&mut ui, tile(measures::power::RIPPLE)).value, 0.);
        assert_eq!(wait(&mut ui, tile(measures::power::INRUSH)).value, 1.);
        assert_eq!(wait(&mut ui, tile(measures::jitter::DC_COMMON)).value, 1.);
        assert!(wait(&mut ui, tile(measures::jitter::EYE_HEIGHT)).value.is_nan());
        assert!(wait(&mut ui, tile(measures::double_pulse::TD_ON)).value.is_nan());
        assert!(ui.azr.catalog_measurements.borrow().cache.len() < catalog.len(), "family results must be shared");
    }
    #[test]
    fn only_explicit_accepted_capture_queue_counts_a_completed_result_once() {
        let mut ui = fixture();
        put(&mut ui, 0, (0..400_000).map(|i| if i % 20 < 10 { 0.0 } else { 2.0 }).collect(), 0., 1e-9);
        ui.m.cat = vec![row(tile(measures::power::RIPPLE))];
        ui.wfm_total = 1;
        let ids = [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)];
        assert!(ui.catalog_reading(tile(measures::power::RIPPLE), ids).computing);
        ui.queue_catalog_statistics(row(tile(measures::power::RIPPLE)), 1);
        assert_eq!(wait(&mut ui, tile(measures::power::RIPPLE)).value, 2.);
        assert_eq!(ui.measurement_statistics_snapshot_for_test()[0].0, 1);
        for _ in 0..20 {
            ui.poll_catalog_measurements();
            let _ = ui.catalog_reading(tile(measures::power::RIPPLE), ids);
        }
        assert_eq!(ui.measurement_statistics_snapshot_for_test()[0].0, 1);
        ui.m.meas_full_record = false;
        ui.m.tscale = tscales().iter().position(|&v| (v - 100e-9).abs() < 1e-14).unwrap();
        ui.m.delay = 1e-6;
        assert_eq!(wait(&mut ui, tile(measures::power::RIPPLE)).value, 2.);
        assert_eq!(ui.measurement_statistics_snapshot_for_test()[0].0, 1, "stopped pan is not a capture");
    }

    #[test]
    fn mixed_origin_interval_readouts_match_independent_common_mode_math() {
        let mut ui = fixture();
        put(&mut ui, 0, vec![0., 1., 2., 3., 4., 5., 6., 7.], -1.5, 0.5);
        put(&mut ui, 1, vec![0., 1., 2., 3., 4., 5.], -2.0, 1.0);
        ui.m.cat = vec![row(tile(measures::jitter::DC_COMMON)), row(tile(measures::jitter::AC_COMMON))];
        ui.m.eye_hp = false;
        assert_eq!(wait(&mut ui, tile(measures::jitter::DC_COMMON)).value, 3.25);
        assert_eq!(wait(&mut ui, tile(measures::jitter::AC_COMMON)).value, 4.5);
        assert_eq!(ui.azr.catalog_measurements.borrow().submitted, 1);
    }

    #[test]
    fn clear_cancels_old_accounting_even_when_capture_number_is_reused() {
        let mut ui = fixture();
        put(&mut ui, 0, (0..400_000).map(|i| if i % 20 < 10 { 0.0 } else { 2.0 }).collect(), 0., 1e-9);
        ui.m.cat = vec![row(tile(measures::power::RIPPLE))];
        ui.wfm_total = 1;
        let ids = [WaveId::Ref(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)];
        let _ = ui.catalog_reading(tile(measures::power::RIPPLE), ids);
        ui.queue_catalog_statistics(row(tile(measures::power::RIPPLE)), 1);
        // Hold a finished answer before UI delivery, then clear and reuse sequence1.
        let answer =
            ui.azr.catalog_measurements.borrow_mut().worker.as_mut().unwrap().wait(Duration::from_secs(20)).unwrap();
        ui.clear_display();
        ui.wfm_total = 1;
        ui.queue_catalog_statistics(row(tile(measures::power::RIPPLE)), 1);
        let (worker, _jobs, send) = Worker::manual();
        ui.azr.catalog_measurements.borrow_mut().worker = Some(worker);
        assert!(send.send(answer).is_ok());
        ui.poll_catalog_measurements();
        assert_eq!(ui.catalog_reading(tile(measures::power::RIPPLE), ids).value, 2.0, "a valid value may still cache");
        assert!(
            ui.measurement_statistics_snapshot_for_test().is_empty(),
            "pre-Clear capture must never repopulate statistics"
        );
    }

    #[test]
    fn processed_channel_reused_buffer_and_capture_after_clear_cannot_reuse_old_samples() {
        let mut ui = fixture();
        ui.m.ch[0].on = true;
        ui.wfm_total = 1;
        ui.last[0] = Some(Record {
            t0: 0.0,
            dt: 1e-9,
            ch: [vec![0.0, 2.0, 0.0, 2.0], Vec::new(), Vec::new(), Vec::new()],
            triggered: true,
            partial: false,
            raw: None,
            la: None,
            pairs: false,
        });
        let ids = [WaveId::Ch(0), WaveId::Ref(1), WaveId::Ref(2), WaveId::Ref(3)];
        ui.m.cat = vec![(measures::power::RIPPLE, ids[0], ids[1], ids[2], ids[3])];
        let read = |ui: &mut Ui| {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                ui.poll_catalog_measurements();
                let r = ui.catalog_reading(tile(measures::power::RIPPLE), ids);
                if !r.computing {
                    return r.value;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        assert_eq!(read(&mut ui), 2.0);
        let pointer = ui.last[0].as_ref().unwrap().ch[0].as_ptr();
        // Clear's reset generation changes while the accepted sequence is reused.
        // Mutate the same allocation to make allocator reuse deterministic.
        ui.reset_catalog_statistics();
        ui.wfm_total = 1;
        ui.last[0].as_mut().unwrap().ch[0][1] = 9.0;
        assert_eq!(ui.last[0].as_ref().unwrap().ch[0].as_ptr(), pointer);
        assert_eq!(read(&mut ui), 9.0, "pre-clear processed snapshot must not survive reused pointer/capture");
    }

    #[test]
    fn pcie4_default_full_ssc_samples_report_nrz_rate_and_stopped_pan_recomputes_the_window() {
        let lane = oos_dsp::serial::LANES[3];
        let dt = oos_dsp::serial::dt_of(&lane);
        let samples = oos_dsp::serial::try_generate(&lane).unwrap();
        assert_eq!(samples.len(), 3_878_788);
        let mut ui = fixture();
        ui.m.meas_full_record = false;
        ui.m.zoom = false;
        ui.m.tscale = tscales().iter().position(|&s| s == 50e-9f32).unwrap();
        ui.m.tfine = 1.0;
        ui.m.delay = 28.6e-6;
        put(&mut ui, 0, samples, 0.0, dt);
        ui.refs[0].as_mut().unwrap().vpd = 0.2;
        ui.session_is_sim = true;
        ui.m.sim_slot = 0;
        ui.replay.sim_revision = 1;
        ui.m.sim_rate = 1e9;
        ui.m.cat = vec![
            row(tile(measures::serial::DATA_RATE)),
            row(tile(measures::jitter::BIT_RATE)),
            row(tile(measures::serial::UNIT_INTERVAL)),
        ];
        ui.relayout();
        let owner = Arc::clone(&ui.refs[0].as_ref().unwrap().v);
        let counters = (ui.wfm_total, ui.records, ui.replay.captures);
        let ideal = oos_dsp::hss::ssc_edge_times(
            owner.len() / 8 + 4096,
            lane.bits_per_s,
            lane.ssc_deviation,
            lane.ssc_hz,
            "Down-spread only",
        );
        let bits = oos_dsp::hss::prbs_bits(lane.prbs as u32, ideal.len()).unwrap();
        let independent_rate = |ui: &Ui| {
            let r = ui.refs[0].as_ref().unwrap();
            let (lo, hi) = ui.measurement_bounds(r.v.len(), r.t0, r.dt);
            let v = &r.v[lo..hi];
            let low = v.iter().copied().fold(f32::INFINITY, f32::min);
            let high = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let threshold = (low + high) as f64 * 0.5;
            let crossings = v
                .windows(2)
                .enumerate()
                .filter_map(|(i, p)| {
                    let (a, b) = (p[0] as f64, p[1] as f64);
                    ((a < threshold) != (b < threshold))
                        .then(|| (((lo + i) as f64 + (threshold - a) / (b - a)) * dt, b > a))
                })
                .collect::<Vec<_>>();
            // Known generated PRBS symbol ordinals are an independent run-count oracle.
            // Match observed transitions to the pre-jitter SSC lattice; measure elapsed time
            // from actual samples, never return the preset rate or use the DSP UI estimator.
            let indices = crossings
                .iter()
                .map(|&(time, rising)| {
                    let j = ideal.partition_point(|edge| *edge < time);
                    let index = if j == ideal.len() || j > 0 && (ideal[j - 1] - time).abs() < (ideal[j] - time).abs() {
                        j - 1
                    } else {
                        j
                    };
                    assert!(index > 0 && bits[index] != bits[index - 1]);
                    assert_eq!(bits[index] == 1, rising);
                    index
                })
                .collect::<Vec<_>>();
            assert!(indices.windows(2).all(|pair| pair[1] > pair[0]), "one observation per actual PRBS transition");
            (indices.last().unwrap() - indices[0]) as f64 / (crossings.last().unwrap().0 - crossings[0].0)
        };
        let oracle = independent_rate(&ui);
        let rate = wait(&mut ui, tile(measures::serial::DATA_RATE));
        let clock = wait(&mut ui, tile(measures::jitter::BIT_RATE));
        let interval = wait(&mut ui, tile(measures::serial::UNIT_INTERVAL));
        assert!(rate.quality.contains("NRZ") && rate.quality.contains("limited"));
        assert!((rate.value - oracle).abs() / oracle < 0.001, "{} vs oracle{}", rate.value, oracle);
        assert!((clock.value - oracle).abs() / oracle < 0.001);
        assert!((interval.value * rate.value - 1.0).abs() < 1e-12);
        assert!(rate.value > 15.9e9 && rate.value < 16.1e9, "accepted data, not the pending1G SIM setting");
        assert_eq!(tile(measures::serial::DATA_RATE).unit, "b/s");
        assert_eq!(tile(measures::jitter::BIT_RATE).unit, "b/s");
        let a = ui.meas_analysis(WaveId::Ref(0)).unwrap().0;
        assert!(rate.value / a.get(Meas::Freq) > 3.8 && rate.value / a.get(Meas::Freq) < 4.2);
        assert!((a.est_ui * rate.value - 1.0).abs() < 1e-12);
        assert!((a.pll_ui * clock.value - 1.0).abs() < 0.001);
        let area = ui.wave_area();
        let (x, y) = (area.x + area.w / 3, area.y + area.h / 2);
        for _ in 0..11 {
            ui.handle(Ev::Wheel { x, y, steps: 32, ctrl: false, shift: false, alt: false });
        }
        ui.step(0.0);
        let panned = wait(&mut ui, tile(measures::serial::DATA_RATE));
        let panned_clock = wait(&mut ui, tile(measures::jitter::BIT_RATE));
        let oracle = independent_rate(&ui);
        assert!((panned.value - oracle).abs() / oracle < 0.001);
        assert!((panned_clock.value - oracle).abs() / oracle < 0.001);
        assert!((panned.value - rate.value).abs() > 1e6, "SSC rate follows the stopped physical gate");
        assert!(Arc::ptr_eq(&owner, &ui.refs[0].as_ref().unwrap().v));
        assert_eq!((ui.wfm_total, ui.records, ui.replay.captures), counters);
        assert_eq!(ui.m.run, RunState::Stop);
    }

    #[test]
    fn nrz_rate_is_missing_when_no_ui_can_be_recovered_instead_of_using_a_cycle_fallback() {
        let mut ui = fixture();
        put(&mut ui, 0, vec![0.0; 1000], 0.0, 1e-9);
        ui.m.cat = vec![row(tile(measures::serial::DATA_RATE)), row(tile(measures::serial::UNIT_INTERVAL))];
        for cat in [tile(measures::serial::DATA_RATE), tile(measures::serial::UNIT_INTERVAL)] {
            let r = wait(&mut ui, cat);
            assert!(r.value.is_nan() && r.reason.is_some());
        }
    }
}
