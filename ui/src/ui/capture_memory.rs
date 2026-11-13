// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The retained acquisition is independent of the screen's interpolated points.
use super::*;
use std::{hash::{Hash, Hasher}, sync::{Arc, mpsc}};

#[derive(Default)]
pub(super) struct State { views: Vec<(u64, Arc<Record>)>, demo: Option<DemoWorker> }

struct DemoJob { key: u64, model: Model, time: f64, elapsed: f64, seed: u64 }
struct DemoResult { key: u64, time: f64, record: Option<Record> }
struct DemoWorker { tx: mpsc::SyncSender<DemoJob>, rx: mpsc::Receiver<DemoResult>, busy: bool, cancelled: bool }
impl DemoWorker {
    fn new() -> Self {
        let (tx, input) = mpsc::sync_channel::<DemoJob>(1);
        let (output, rx) = mpsc::channel();
        std::thread::Builder::new().name("full-depth-demo".into()).spawn(move || {
            while let Ok(job) = input.recv() {
                let mut source = Source::Demo { t: job.time };
                let mut rng = Rng::new(job.seed);
                let record = source.acquire_full_demo(&job.model, job.elapsed, &mut rng).map(|mut rec| {
                    // Retain the signal that was acquired, at native dt. Display,
                    // math and measurements must share this same conditioning.
                    for c in 0..NCH { condition(&job.model, c, &mut rec.ch[c], rec.dt); }
                    let basic = std::array::from_fn(|c| crate::board::BasicStats::of(&rec.ch[c]));
                    let points = rec.ch.iter().map(Vec::len).max().unwrap_or(0);
                    let origin = rec.t0;
                    let ti = -origin / rec.dt;
                    let frame = Arc::new(crate::board::Frame { seq: job.seed, dt: rec.dt,
                        pre: ti.round().clamp(0.0, points.saturating_sub(1) as f64) as usize,
                        ch: std::mem::take(&mut rec.ch), roll: false, envelope: false, scan: false, partial: false,
                        acq: (1.0 / rec.dt, points), stamp: job.model.hw_stamp, basic, la: rec.la.take() });
                    let window = job.model.time_per_div() as f64 * HDIV as f64;
                    crate::board::resample_record(&frame, &job.model, 3600, window,
                        job.model.delay as f64 - window * 0.5, ti, rec.triggered)
                });
                let time = match source { Source::Demo { t } => t, _ => job.time };
                if output.send(DemoResult { key: job.key, time, record }).is_err() { break; }
            }
        }).expect("start bounded full-memory demo worker");
        Self { tx, rx, busy: false, cancelled: false }
    }
}

impl Ui {
    pub(super) fn cancel_demo_acquisition(&mut self) {
        if let Some(worker) = self.capture_views.borrow_mut().demo.as_mut() {
            if worker.busy { worker.cancelled = true; }
            if worker.rx.try_recv().is_ok() { worker.busy = false; }
        }
    }

    pub(super) fn acquire_display_record(&mut self, elapsed: f64) -> Option<Record> {
        let Source::Demo { t } = &self.src else { return self.src.acquire(&self.m, elapsed, &mut self.rng); };
        if mdepths()[self.m.mdepth].1 == 0 { return self.src.acquire(&self.m, elapsed, &mut self.rng); }
        let time = *t;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (crate::store::setup_text(&self.m), self.workflow_epoch, self.m.hw_stamp).hash(&mut h);
        let key = h.finish();
        let mut answer = None;
        {
            let mut state = self.capture_views.borrow_mut();
            let worker = state.demo.get_or_insert_with(DemoWorker::new);
            if let Ok(result) = worker.rx.try_recv() {
                worker.busy = false;
                if result.key == key && !worker.cancelled { answer = Some(result); }
            }
            if answer.is_none() && !worker.busy {
                let job = DemoJob { key, model: self.m.clone(), time, elapsed, seed: self.rng.next() };
                if worker.tx.try_send(job).is_ok() { worker.busy = true; worker.cancelled = false; }
            }
        }
        if let Some(answer) = answer {
            if let Source::Demo { t } = &mut self.src { *t = answer.time; }
            return answer.record;
        }
        None
    }
    pub(super) fn is_basic_frame_row(&self, row: usize) -> bool {
        self.m.meas.get(row).is_some_and(|(kind, source)| matches!(source, WaveId::Ch(_))
            && matches!(kind, Meas::Vmax | Meas::Vmin | Meas::Vpp | Meas::Vavg | Meas::Vrms | Meas::StdDev))
    }

    fn accumulate_basic_values(&mut self, channel: usize, values: &[f32]) {
        self.accumulate_basic_summary(channel, crate::board::BasicStats::of(values));
    }

    pub(super) fn accumulate_basic_summary(&mut self, channel: usize, values: crate::board::BasicStats) {
        if !values.valid { return; }
        self.mstat.resize(self.m.meas.len() + self.m.meas2.len(), MStat::default());
        for (row, &(kind, source)) in self.m.meas.iter().enumerate() {
            if source != WaveId::Ch(channel) { continue; }
            let Some(value) = values.reading(kind) else { continue; };
            let stat = &mut self.mstat[row];
            if stat.n == 0 { stat.min = value; stat.max = value; }
            stat.n += 1; stat.sum += value; stat.sumsq += value * value;
            stat.min = stat.min.min(value); stat.max = stat.max.max(value);
        }
    }

    pub(super) fn accumulate_record_basic(&mut self, record: &Record) {
        for c in 0..NCH {
            if !self.m.ch[c].on || !self.m.meas.iter().any(|&(k, s)| s == WaveId::Ch(c)
                && matches!(k, Meas::Vmax | Meas::Vmin | Meas::Vpp | Meas::Vavg | Meas::Vrms | Meas::StdDev)) { continue; }
            if let Some((values, _, _)) = self.measurement_channel_samples(record, c) {
                if let Some(raw) = record.raw.as_ref().filter(|r| !r.frame.envelope && values.len() == r.frame.ch[c].len()
                    && values.as_ptr() == r.frame.ch[c].as_ptr() && r.frame.basic[c].valid) {
                    self.accumulate_basic_summary(c, raw.frame.basic[c]);
                } else { self.accumulate_basic_values(c, values); }
            }
        }
    }

    pub(super) fn accumulate_fast_basic(&mut self, frame: &crate::board::Frame, trigger: f64) {
        let origin = -trigger * frame.dt;
        for c in 0..NCH {
            if !self.m.ch[c].on { continue; }
            let data = &frame.ch[c];
            let (a, b) = if self.m.meas_full_record { (0, data.len()) } else {
                let half = self.m.time_per_div() as f64 * HDIV as f64 * 0.5;
                let bound = |t: f64| (((t - origin) / frame.dt).ceil().max(0.0) as usize).min(data.len());
                (bound(self.m.delay as f64 - half), bound(self.m.delay as f64 + half))
            };
            if b > a {
                if !frame.envelope && a == 0 && b == data.len() && frame.basic[c].valid { self.accumulate_basic_summary(c, frame.basic[c]); }
                else { self.accumulate_basic_values(c, &data[a..b]); }
            }
        }
    }
    pub(super) fn captured_record_view(&self, rec: &Record, view: (f32, f32)) -> Option<Arc<Record>> {
        let raw = rec.raw.as_ref()?;
        if !raw.frame.dt.is_finite() || raw.frame.dt <= 0.0 { return None; }
        let origin = raw.t0 - raw.i0 as f64 * raw.frame.dt;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (Arc::as_ptr(&raw.frame) as usize, origin.to_bits(), view.0.to_bits(), view.1.to_bits(),
            format!("{:?}", self.m.acq), rec.triggered, self.src.is_hw()).hash(&mut h);
        for ch in &self.m.ch {
            (ch.invert, format!("{:?}", ch.coupling), ch.bw, ch.offset.to_bits(), ch.vfine.to_bits()).hash(&mut h);
        }
        let key = h.finish();
        if let Some((_, cached)) = self.capture_views.borrow().views.iter().find(|(k, _)| *k == key) {
            return Some(Arc::clone(cached));
        }
        let window = view.0 as f64 * HDIV as f64;
        let mut display = crate::board::resample_record(&raw.frame, &self.m, 3600, window,
            view.1 as f64 - window * 0.5, -origin / raw.frame.dt, rec.triggered);
        if !self.src.is_hw() && !raw.conditioned {
            for c in 0..NCH { condition(&self.m, c, &mut display.ch[c], display.dt); }
        }
        let display = Arc::new(display);
        let mut state = self.capture_views.borrow_mut();
        if state.views.len() >= 2 { state.views.remove(0); }
        state.views.push((key, Arc::clone(&display)));
        Some(display)
    }

    pub(super) fn captured_view(&self, view: (f32, f32)) -> Option<Arc<Record>> {
        self.captured_record_view(self.last[0].as_ref()?, view)
    }

    pub(super) fn refresh_capture_view(&mut self) {
        let view = (self.m.time_per_div(), self.m.delay);
        if let Some(rec) = self.captured_view(view) { self.last[0] = Some(rec.as_ref().clone()); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn long_capture() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.run = RunState::Stop;
        ui.m.ch[0].on = true;
        ui.m.ch[0].bw = BW_FULL;
        ui.m.ch[0].coupling = Coupling::Dc;
        ui.m.trig_level = 10.0;
        ui.m.tscale = tscales().iter().position(|&t| (t - 100e-9).abs() < 1e-14).unwrap();
        let mut data = vec![0.0; 100_000];
        data[1234] = 0.25;
        data[88_888] = 0.75;
        let frame = Arc::new(crate::board::Frame { seq: 3, dt: 1e-9, pre: 50_000, ch: [data, Vec::new(), Vec::new(), Vec::new()],
            roll: false, scan: false, partial: false, envelope: false, acq: (0.0, 0), stamp: 0, basic: Default::default(), la: None });
        ui.last[0] = Some(crate::board::to_record(&frame, &ui.m, 3600, 1e-6, -0.5e-6));
        ui
    }
    #[test]
    fn full_capture_survives_stop_pan_zoom_and_out_of_range_return() {
        let mut ui = long_capture();
        assert_eq!(ui.raw_info(0), Some((100_000, 1e-9, -50e-6)));
        assert_eq!(ui.raw_volts(0).unwrap()[88_888], 0.75);
        let (original, captures) = (ui.raw_volts(0).unwrap(), ui.records);
        for delay in [-48.766e-6, 38.888e-6] {
            ui.m.delay = delay;
            ui.step(0.02);
            assert!(ui.last[0].as_ref().unwrap().ch[0].iter().any(|&v| v > 0.2));
            assert!(ui.ph.map[0].iter().any(|&p| p != 0));
            let (window, _, _) = ui.measurement_channel_samples(ui.last[0].as_ref().unwrap(), 0).unwrap();
            assert!(window.iter().any(|&v| v > 0.2));
        }
        ui.m.delay = 1.0;
        ui.step(0.02);
        assert!(ui.last[0].as_ref().unwrap().ch[0].iter().all(|v| v.is_nan()));
        ui.m.delay = 38.888e-6;
        ui.step(0.02);
        assert_eq!(ui.raw_volts(0).unwrap(), original);
        assert_eq!(ui.records, captures);
        assert_eq!(ui.m.run, RunState::Stop);
        let first = ui.captured_view((ui.m.time_per_div(), ui.m.delay)).unwrap();
        assert!(Arc::ptr_eq(&first, &ui.captured_view((ui.m.time_per_div(), ui.m.delay)).unwrap()));
    }
    #[test]
    fn detail_rebuilds_from_native_samples_and_keeps_the_accepted_trigger() {
        let mut ui = long_capture();
        ui.m.zoom = true;
        ui.m.zscale = tscales().iter().position(|&t| (t - 1e-9).abs() < 1e-16).unwrap();
        ui.m.zdelay = 38.888e-6;
        let detail = ui.captured_view((tscales()[ui.m.zscale], ui.m.zdelay)).unwrap();
        assert!(detail.ch[0].iter().any(|&v| v > 0.7));
        assert!(detail.dt < 1e-11, "display interpolation can expand but never replace native samples");
        assert_eq!(detail.raw.as_ref().unwrap().frame.ch[0].len(), 100_000);
        assert!(!detail.triggered, "panning does not retrigger the capture");
    }

    #[test]
    fn each_fast_frame_contributes_all_six_basic_measurements_exactly_once() {
        let mut ui = long_capture();
        ui.m.meas = [Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vavg, Meas::Vrms, Meas::StdDev]
            .into_iter().map(|k| (k, WaveId::Ch(0))).collect();
        ui.m.meas_full_record = true;
        let mut frame = ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame.as_ref().clone();
        frame.ch[0] = vec![-1.0, 1.0];
        for n in 1..=193 {
            frame.seq = n;
            ui.accumulate_fast_basic(&frame, 1.0);
        }
        let expected = [1.0, -1.0, 2.0, 0.0, 1.0, 1.0];
        for (stat, value) in ui.mstat.iter().zip(expected) {
            assert_eq!(stat.n, 193, "no 96-frame refresh cap or duplicate final frame");
            assert!((stat.sum / stat.n as f64 - value).abs() < 1e-12);
        }
    }

    #[test]
    fn explicit_demo_memory_is_fully_filled_off_thread_and_survives_stop() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.mdepth = mdepths().iter().position(|d| d.1 >= 10_000 && d.1 <= 100_000).unwrap();
        let expected = ui.m.srate_depth().1 as usize;
        assert!(ui.acquire_display_record(0.02).is_none(), "full memory is prepared off the event thread");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let rec = loop {
            if let Some(rec) = ui.acquire_display_record(0.02) { break rec; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert_eq!(rec.raw.as_ref().unwrap().frame.ch[0].len(), expected);
        assert_eq!(rec.ch[0].len(), 3600);
        ui.last[0] = Some(rec);
        ui.m.run = RunState::Stop;
        ui.step(0.02);
        assert_eq!(ui.raw_info(0).unwrap().0, expected);
    }

    #[test]
    fn full_demo_conditioning_is_native_once_and_shared_by_display_math_and_measurements() {
        for (coupling, invert, bw) in [(Coupling::Ac, true, BW_20M), (Coupling::Gnd, true, BW_20M), (Coupling::Dc, true, BW_FULL)] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.mdepth = mdepths().iter().position(|d| d.1 >= 10_000 && d.1 <= 100_000).unwrap();
            ui.m.ch[0].coupling = coupling;
            ui.m.ch[0].invert = invert;
            ui.m.ch[0].bw = bw;
            ui.m.meas_full_record = true;
            ui.rng = Rng::new(17);
            let seed = Rng::new(17).next();
            let mut source = Source::Demo { t: 0.0 };
            let mut expected = source.acquire_full_demo(&ui.m, 0.02, &mut Rng::new(seed)).unwrap();
            condition(&ui.m, 0, &mut expected.ch[0], expected.dt);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let record = loop {
                if let Some(record) = ui.acquire_display_record(0.02) { break record; }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            };
            let native = record.raw.as_ref().unwrap();
            assert!(native.conditioned);
            assert_eq!(native.frame.ch[0], expected.ch[0], "native conditioning must occur once at native dt");
            assert_eq!(native.frame.basic[0].values, crate::board::BasicStats::of(&expected.ch[0]).values);
            let original_display = record.ch[0].clone();
            ui.last[0] = Some(record);
            ui.m.run = RunState::Stop;
            ui.measure_last();
            let mean = expected.ch[0].iter().map(|&v| v as f64).sum::<f64>() / expected.ch[0].len() as f64;
            assert!((ui.meas_at(WaveId::Ch(0), Meas::Vavg) - mean).abs() < 1e-12);
            ui.m.math = MathOp::Lin;
            ui.m.math_a = 0;
            ui.m.math_lin = (1.0, 0.0);
            assert_eq!(ui.math_record(0).unwrap().unwrap().v.as_slice(), expected.ch[0].as_slice());
            let display = ui.captured_view((ui.m.time_per_div(), ui.m.delay)).unwrap();
            assert!(display.ch[0].iter().zip(&original_display).all(|(a,b)| a.to_bits() == b.to_bits()), "pan reconstruction must not apply AC/invert/filter twice");
            let original_native = ui.raw_volts(0).unwrap();
            ui.m.delay += ui.m.time_per_div();
            ui.step(0.02);
            ui.m.delay -= ui.m.time_per_div();
            ui.step(0.02);
            assert_eq!(ui.raw_volts(0).unwrap(), original_native);
        }
    }

    #[test]
    fn replay_conditioning_reaches_the_ui_once_and_keeps_native_measurements_consistent() {
        let words: Vec<u16> = (0..20_000).map(|i| if i % 100 < 30 { 1400 } else { 1000 }).collect();
        for coupling in [Coupling::Ac, Coupling::Gnd, Coupling::Dc] {
            let mut ui = Ui::new(Source::Replay { words: words.clone(), xinc: 1e-9, yinc: 0.001, yorg: 0.0, yref: 0.0 });
            ui.m.ch[0].coupling = coupling;
            ui.m.ch[0].invert = true;
            ui.m.ch[0].bw = BW_20M;
            ui.m.meas_full_record = true;
            let mut expected: Vec<f32> = words.iter().map(|&v| (v as f64 * 0.001) as f32).collect();
            condition(&ui.m, 0, &mut expected, 1e-9);
            ui.step(0.05);
            ui.m.run = RunState::Stop;
            let record = ui.last[0].as_ref().unwrap();
            assert!(record.raw.as_ref().unwrap().conditioned);
            assert_eq!(record.raw.as_ref().unwrap().frame.ch[0], expected);
            let raw = record.raw.as_ref().unwrap();
            assert_eq!(record.raw.as_ref().unwrap().frame.basic[0].values, crate::board::BasicStats::of(&expected).values);
            let ti = -raw.t0 / raw.frame.dt;
            for (i, &value) in record.ch[0].iter().enumerate().filter(|(_, v)| v.is_finite()) {
                let at = ti + (record.t0 + i as f64 * record.dt) / raw.frame.dt;
                let k = at.floor() as usize;
                let fraction = (at - k as f64) as f32;
                let shown = expected[k] * (1.0 - fraction) + expected[k + 1] * fraction;
                assert!((value - shown).abs() < 1e-6, "the UI must not condition Replay twice");
            }
            let mean = expected.iter().map(|&v| v as f64).sum::<f64>() / expected.len() as f64;
            assert!((ui.meas_at(WaveId::Ch(0), Meas::Vavg) - mean).abs() < 1e-12);
        }
    }

    #[test]
    fn every_single_press_rearms_and_rejects_a_queued_full_demo_from_before_the_press() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.mdepth = mdepths().iter().position(|d| d.1 >= 10_000 && d.1 <= 100_000).unwrap();
        ui.m.trig_mode = TrigMode::Single;
        ui.m.run = RunState::Run;
        ui.m.hw_stamp = 7;
        ui.single_armed = true;
        assert!(ui.acquire_display_record(0.02).is_none());
        // Single is valid even while already waiting; it starts a fresh arm.
        ui.key(key::SINGLE);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for (captures, stamp) in [(1, 8), (2, 9)] {
            while ui.records < captures {
                ui.step(0.02);
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!((ui.records, ui.m.run, ui.m.hw_stamp), (captures, RunState::Stop, stamp));
            assert_eq!(ui.last[0].as_ref().unwrap().raw.as_ref().unwrap().frame.stamp, stamp);
            if captures == 1 { ui.key(key::SINGLE); }
        }
    }

    #[test]
    fn equal_length_hires_display_never_reuses_unfiltered_native_statistics() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.acq = AcqMode::HiRes;
        ui.m.meas_full_record = true;
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
        let native = vec![-1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0];
        let basic = crate::board::BasicStats::of(&native);
        let frame = Arc::new(crate::board::Frame { dt: 1e-9, ch: [native, Vec::new(), Vec::new(), Vec::new()],
            basic: [basic, Default::default(), Default::default(), Default::default()], ..Default::default() });
        let mut record = Ui::blank_record();
        record.dt = 1e-9;
        record.ch[0] = vec![0.25; 8];
        record.raw = Some(crate::wave::Raw { frame, i0: 0, i1: 8, t0: 0.0, conditioned: true });
        ui.accumulate_record_basic(&record);
        ui.last[0] = Some(record);
        assert_eq!(ui.mstat[0].n, 1);
        assert_eq!(ui.mstat[0].sum, 0.25, "statistics must follow HiRes display samples");
        assert_eq!(ui.meas_at(WaveId::Ch(0), Meas::Vavg), 0.25);
    }
}
