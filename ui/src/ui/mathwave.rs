// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Timed, cached math over acquired, stored and earlier math waveforms.
use super::*;
use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, sync::{Arc, mpsc}};

const BACKGROUND: usize = 262_144;
const MAX_POINTS: usize = 32_000_000;
const MAX_CONV_TERMS: usize = 16_000_000;

pub(super) struct Signal { pub t0: f64, pub dt: f64, pub v: Arc<Vec<f32>> }
struct Job { slot: usize, key: u64, config: MathCh, expr: String, inputs: Vec<(usize, Arc<Signal>)> }
struct Answer { slot: usize, key: u64, signal: Result<Arc<Signal>, &'static str> }
struct Worker { tx: mpsc::SyncSender<Job>, rx: mpsc::Receiver<Answer>, busy: bool }
#[derive(Default)]
pub(super) struct State {
    cache: [Option<(u64, Result<Arc<Signal>, &'static str>)>; NMATH],
    worker: Option<Worker>,
}

impl Worker {
    fn new() -> Self {
        let (tx, input) = mpsc::sync_channel::<Job>(1);
        let (output, rx) = mpsc::channel();
        std::thread::Builder::new().name("waveform-math".into()).spawn(move || {
            while let Ok(job) = input.recv() {
                let answer = Answer { slot: job.slot, key: job.key, signal: compute_job(&job).map(Arc::new) };
                if output.send(answer).is_err() { break; }
            }
        }).expect("start bounded math worker");
        Self { tx, rx, busy: false }
    }
}

/// Sample only within the source's physical record. Never extrapolate a rail.
fn sample(w: &Signal, time: f64) -> f32 {
    let x = (time - w.t0) / w.dt;
    let nearest = x.round();
    let x = if (x - nearest).abs() < 1e-6 { nearest } else { x };
    if x < 0.0 || x > w.v.len().saturating_sub(1) as f64 { return f32::NAN; }
    let i = x.floor() as usize;
    let a = w.v[i];
    let fraction = (x - i as f64) as f32;
    if fraction == 0.0 { return a; }
    let b = w.v.get(i + 1).copied().unwrap_or(a);
    a + (b - a) * fraction
}

fn compute_job(job: &Job) -> Result<Signal, &'static str> {
    if job.inputs.is_empty() { return Err("No source data"); }
    let start = job.inputs.iter().map(|(_, w)| w.t0).fold(f64::NEG_INFINITY, f64::max);
    let end = job.inputs.iter().map(|(_, w)| w.t0 + w.v.len().saturating_sub(1) as f64 * w.dt).fold(f64::INFINITY, f64::min);
    // The common grid cannot claim more detail than its slowest input.
    let dt = job.inputs.iter().map(|(_, w)| w.dt).fold(0.0f64, f64::max);
    if !start.is_finite() || !end.is_finite() || !dt.is_finite() || dt <= 0.0 || end < start { return Err("Sources do not overlap"); }
    let n = (((end - start) / dt + 1e-6).floor() as usize).saturating_add(1);
    if n > MAX_POINTS { return Err("Math record exceeds 32M points"); }
    // Direct convolution is quadratic on the common grid. Reject it before
    // allocating aligned copies or monopolizing the sole background worker.
    if job.config.op == MathOp::Conv && n.checked_mul(n).is_none_or(|work| work > MAX_CONV_TERMS) {
        return Err("Convolution kernel too long; use a shorter reference");
    }
    let aligned: Vec<_> = job.inputs.iter().map(|(id, w)| (*id, (0..n).map(|i| sample(w, start + i as f64 * dt)).collect::<Vec<_>>())).collect();
    let c = job.config;
    let v = if c.op == MathOp::Expr {
        let e = parse_expr(&job.expr).map_err(|_| "Invalid formula")?;
        let mut sources = vec![&[][..]; MATH_SOURCES];
        for (id, data) in &aligned { sources[*id] = data; }
        e.eval_sources(&sources, start, dt, n)
    } else {
        let a = &aligned[0].1;
        if c.op == MathOp::Fft {
            let (df, spectrum) = oos_dsp::math::spectrum(a, dt, c.win);
            return Ok(Signal { t0: 0.0, dt: df, v: Arc::new(spectrum.iter().map(|&v| oos_dsp::math::dbv(v) as f32).collect()) });
        }
        let b = aligned.get(1).map_or(&[][..], |x| x.1.as_slice());
        let mut v = compute(c.op, a, b, dt, c.fc as f64, c.bw as f64);
        if c.op == MathOp::Lin { for v in &mut v { *v = c.lin.0 * *v + c.lin.1; } }
        v
    };
    Ok(Signal { t0: start, dt, v: Arc::new(v) })
}

impl Ui {
    pub(super) fn math_source_ids(&self, below: usize) -> Vec<usize> {
        (0..n_ch()).chain(NCH..NCH + below.min(NMATH))
            .chain((0..NREF).filter(|&r| self.refs[r].is_some()).map(|r| MATH_REF_BASE + r)).collect()
    }

    fn math_inputs(&self, k: usize) -> Result<Vec<usize>, &'static str> {
        let c = self.m.math_ch(k);
        if c.op == MathOp::Off { return Err("Off"); }
        if c.op == MathOp::Expr {
            let parsed = parse_expr(&self.m.math_expr[k]).map_err(|_| "Invalid formula")?;
            if !parsed.used_sources.is_empty() { return Ok(parsed.used_sources); }
            // A constant/time expression follows the selected source or last record.
            return Ok(vec![if self.record_mode() { MATH_REF_BASE + self.active_reference() } else { 0 }]);
        }
        Ok(if c.op.two_sources() { vec![c.a, c.b] } else { vec![c.a] })
    }

    fn math_key(&self, k: usize) -> Result<u64, &'static str> {
        if k >= NMATH { return Err("Invalid math slot"); }
        let mut h = DefaultHasher::new();
        let c = self.m.math_ch(k);
        // Display scale/offset are deliberately excluded: they cannot change samples.
        (format!("{:?}", c.op), c.a, c.b, c.fc.to_bits(), c.bw.to_bits(), format!("{:?}", c.win),
            c.lin.0.to_bits(), c.lin.1.to_bits(), &self.m.math_expr[k]).hash(&mut h);
        for id in self.math_inputs(k)? {
            id.hash(&mut h);
            if id < NCH {
                let r = self.last[0].as_ref().ok_or("No acquired record")?;
                if let Some(raw) = r.raw.as_ref().filter(|r| !r.frame.envelope) {
                    let v = &raw.frame.ch[id];
                    if v.is_empty() { return Err("Source has no data"); }
                    (raw.frame.seq, v.as_ptr() as usize, v.len(), raw.frame.dt.to_bits(),
                        (raw.t0 - raw.i0 as f64 * raw.frame.dt).to_bits()).hash(&mut h);
                } else {
                    if r.ch[id].is_empty() { return Err("Source has no data"); }
                    (self.wfm_total, r.ch[id].as_ptr() as usize, r.ch[id].len(), r.t0.to_bits(), r.dt.to_bits()).hash(&mut h);
                }
            } else if id < MATH_REF_BASE {
                let other = id - NCH;
                if other >= k { return Err("Math cycle/forward source"); }
                if self.m.math_ch(other).op == MathOp::Fft { return Err("FFT is a frequency-domain source"); }
                self.math_key(other)?.hash(&mut h);
            } else {
                let r = self.refs.get(id - MATH_REF_BASE).and_then(Option::as_ref).ok_or("Reference is empty")?;
                if r.v.is_empty() { return Err("Reference is empty"); }
                (r.v.as_ptr() as usize, r.v.len(), r.t0.to_bits(), r.dt.to_bits(), self.workflow_epoch,
                    self.replay.sim_revision).hash(&mut h);
            }
        }
        Ok(h.finish())
    }

    pub(super) fn math_source_record(&self, id: usize, below: usize) -> Option<Arc<Signal>> {
        if id < NCH {
            let r = self.last[0].as_ref()?;
            if let Some(raw) = r.raw.as_ref().filter(|r| !r.frame.envelope) {
                return Some(Arc::new(Signal { t0: raw.t0 - raw.i0 as f64 * raw.frame.dt, dt: raw.frame.dt, v: Arc::new(raw.frame.ch[id].clone()) }));
            }
            return Some(Arc::new(Signal { t0: r.t0, dt: r.dt, v: Arc::new(r.ch[id].clone()) }));
        }
        if id < MATH_REF_BASE {
            return (id - NCH < below).then(|| self.math_record(id - NCH).ok().flatten()).flatten();
        }
        let r = self.refs.get(id - MATH_REF_BASE)?.as_ref()?;
        Some(Arc::new(Signal { t0: r.t0, dt: r.dt, v: Arc::new(r.v.clone()) }))
    }

    pub(super) fn math_record(&self, k: usize) -> Result<Option<Arc<Signal>>, &'static str> {
        let key = self.math_key(k)?;
        {
            let mut state = self.math_state.borrow_mut();
            if let Some(worker) = &mut state.worker {
                if let Ok(answer) = worker.rx.try_recv() {
                    worker.busy = false;
                    state.cache[answer.slot] = Some((answer.key, answer.signal));
                }
            }
            if let Some((stored, value)) = &state.cache[k] {
                if *stored == key { return value.clone().map(Some); }
            }
            if state.worker.as_ref().is_some_and(|w| w.busy) { return Ok(None); }
        }
        let mut inputs = Vec::new();
        for id in self.math_inputs(k)? {
            let Some(w) = self.math_source_record(id, k) else { return Ok(None); };
            if w.v.is_empty() || !w.dt.is_finite() || w.dt <= 0.0 { return Err("Source has no data"); }
            inputs.push((id, w));
        }
        let large = inputs.iter().any(|(_, w)| w.v.len() > BACKGROUND);
        let job = Job { slot: k, key, config: self.m.math_ch(k), expr: self.m.math_expr[k].clone(), inputs };
        if large {
            let mut state = self.math_state.borrow_mut();
            let worker = state.worker.get_or_insert_with(Worker::new);
            if worker.tx.try_send(job).is_ok() { worker.busy = true; }
            return Ok(None);
        }
        let signal = compute_job(&job).map(Arc::new);
        self.math_state.borrow_mut().cache[k] = Some((key, signal.clone()));
        signal.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn refs() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.last[0] = None;
        for ch in &mut ui.m.ch { ch.on = false; }
        for (i, t0, dt, v) in [(0, -2.0, 1.0, vec![0., 1., 2., 3., 4., 5.]), (1, -1.5, 0.5, vec![0., 1., 2., 3., 4., 5., 6., 7.])] {
            ui.refs[i] = Some(RefWave { src: 0, color: i, label: format!("R{}", i + 1), t0, dt, v, vpd: 1.0, offset: 0.0, on: true });
        }
        ui.m.math = MathOp::Sub;
        ui.m.math_a = MATH_REF_BASE;
        ui.m.math_b = MATH_REF_BASE + 1;
        ui
    }
    #[test]
    fn reference_math_aligns_physical_time_without_a_live_record() {
        let ui = refs();
        let w = ui.math_record(0).unwrap().unwrap();
        assert_eq!((w.t0, w.dt), (-1.5, 1.0));
        assert_eq!(w.v.as_slice(), &[0.5, -0.5, -1.5, -2.5]);
        let same = ui.math_record(0).unwrap().unwrap();
        assert!(Arc::ptr_eq(&w, &same));
    }
    #[test]
    fn reference_math_sources_picker_chain_formula_and_setup_roundtrip() {
        let mut ui = refs();
        assert!(ui.options(Item::MathA).contains(&"R2".into()));
        ui.select_option(Item::MathA, "R2");
        assert_eq!(ui.m.math_a, MATH_REF_BASE + 1);
        ui.m.math_a = MATH_REF_BASE;
        ui.m.math_select(1);
        ui.m.math = MathOp::Abs;
        ui.m.math_a = NCH;
        assert_eq!(ui.math_wave_of(1), vec![0.5, 0.5, 1.5, 2.5]);
        ui.m.math = MathOp::Expr;
        ui.m.math_expr[1] = "abs(M1)+R1-R2".into();
        assert_eq!(ui.math_wave_of(1), vec![1.0, 0.0, 0.0, 0.0]);
        let loaded = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!(loaded.math_ch(0).a, MATH_REF_BASE);
        assert_eq!(loaded.math_ch(0).b, MATH_REF_BASE + 1);
        ui.m.math_expr[1] = "M2".into();
        assert_eq!(ui.math_record(1).err(), Some("Math cycle/forward source"));
    }
    #[test]
    fn reference_math_fft_uses_original_interval_and_cache_invalidates_on_reload() {
        let mut ui = refs();
        ui.m.math = MathOp::Fft;
        let original = ui.refs[0].as_ref().unwrap().v.clone();
        ui.refs[0].as_mut().unwrap().v = (0..128).map(|i| (i as f32 * std::f32::consts::TAU / 16.0).sin()).collect();
        let w = ui.math_record(0).unwrap().unwrap();
        assert!(w.dt > 0.0 && !w.v.is_empty());
        assert!((w.dt - 1.0 / 128.0).abs() < 1e-12);
        ui.refs[0].as_mut().unwrap().v = original;
        ui.m.math = MathOp::Sub;
        let first = ui.math_record(0).unwrap().unwrap();
        ui.refs[1].as_mut().unwrap().t0 = 100.0;
        assert_eq!(ui.math_record(0).err(), Some("Sources do not overlap"));
        assert_eq!(first.v[0], 0.5);
    }
    #[test]
    fn large_reference_math_is_background_cached_and_rejects_stale_settings() {
        let mut ui = refs();
        ui.refs[0].as_mut().unwrap().v = vec![2.0; BACKGROUND + 1];
        ui.m.math = MathOp::Lin;
        ui.m.math_lin = (2.0, 1.0);
        assert!(ui.math_record(0).unwrap().is_none());
        ui.m.math_lin = (3.0, 1.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let w = loop {
            if let Some(w) = ui.math_record(0).unwrap() { break w; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(w.v[0], 7.0);
        let cached = ui.math_record(0).unwrap().unwrap();
        assert!(Arc::ptr_eq(&w, &cached));
        assert_eq!(ui.m.run, RunState::Stop);
    }

    #[test]
    fn oversized_convolution_returns_bounded_error_and_does_not_block_next_math() {
        let mut ui = refs();
        for slot in 0..2 {
            let r = ui.refs[slot].as_mut().unwrap();
            r.t0 = 0.0; r.dt = 1.0; r.v = vec![2.0; BACKGROUND + 1];
        }
        ui.m.math = MathOp::Conv;
        assert!(ui.math_record(0).unwrap().is_none());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match ui.math_record(0) {
                Err(message) => { assert_eq!(message, "Convolution kernel too long; use a shorter reference"); break; }
                Ok(None) => { assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(2)); }
                Ok(Some(_)) => panic!("oversized quadratic work must not run"),
            }
        }
        ui.m.math = MathOp::Lin;
        ui.m.math_lin = (3.0, 1.0);
        let value = loop {
            if let Some(value) = ui.math_record(0).unwrap() { break value; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(value.v.len(), BACKGROUND + 1);
        assert!(value.v.iter().all(|&v| v == 7.0));
    }
}
