//! Selected-source Eye setup and bounded immutable-reference analysis.
use super::*;
use std::sync::mpsc;

const BACKGROUND_POINTS: usize = 262_144;
const PROBE_POINTS: usize = 131_072;

#[derive(Clone, Debug)]
pub(in crate::ui) struct Summary {
    pub rate: f64,
    pub threshold: f32,
    pub hysteresis: f32,
    pub samples_per_ui: f64,
    pub recovery: &'static str,
    pub warning: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key(Vec<u64>);

struct Job {
    key: Key,
    source_name: String,
    data: WaveSamp,
    clock: Option<WaveSamp>,
    rail: Option<WaveSamp>,
    requested_rate: Option<f64>,
    settings: FactSettings,
    layout: Layout,
    big: Option<u8>,
}

struct Computed {
    summary: Summary,
    eye: eye::Eye,
    page: Page,
    meas: eye::EyeMeas,
    jitter: Jitter,
    period: f64,
}

struct Finished { key: Key, result: Result<Computed, String> }

struct Worker {
    send: mpsc::SyncSender<Job>,
    receive: mpsc::Receiver<Finished>,
    busy: bool,
}

impl Worker {
    fn new() -> Self {
        let (send, requests) = mpsc::sync_channel::<Job>(1);
        let (results, receive) = mpsc::channel();
        std::thread::Builder::new().name("reference-eye".into()).spawn(move || {
            while let Ok(job) = requests.recv() {
                let key = job.key.clone();
                if results.send(Finished { key, result: compute(job) }).is_err() { break; }
            }
        }).expect("start reference Eye worker");
        Self { send, receive, busy: false }
    }
}

#[derive(Default)]
pub(in crate::ui) struct State {
    pub enabled: bool,
    pub summary: Option<Summary>,
    wanted: Option<Key>,
    applied: Option<Key>,
    failed: bool,
    worker: Option<Worker>,
    pub submitted: usize,
    pub applied_results: usize,
}

impl State {
    pub fn invalidate(&mut self) {
        self.wanted = None;
        self.applied = None;
        self.summary = None;
        self.failed = false;
    }
}

fn infer(v: &[f32], dt: f64, requested: Option<f64>, explicit_clock: bool) -> Result<(Summary, eye::Clock), String> {
    if v.len() < 8 || !dt.is_finite() || dt <= 0.0 { return Err("Need at least 8 samples with a valid interval".into()); }
    let st = stats(v, dt);
    let span = st.vmax - st.vmin;
    if !span.is_finite() || span <= 1e-9 { return Err("Flat source: no transitions to recover a clock".into()); }
    let amplitude = if st.valid { (st.top - st.base).max(span * 0.25) } else { span };
    let threshold = if st.valid && !explicit_clock { 0.5 * (st.top + st.base) } else { 0.5 * (st.vmax + st.vmin) };
    // Voltage thresholds follow the data, never the waveform's display zoom.
    let hysteresis = amplitude * if explicit_clock { 0.2 } else { 0.1 };
    let mean_clock = eye::recover_clock(v, dt, threshold, hysteresis, requested)
        .ok_or_else(|| "Need at least 3 usable transitions; set a rate or explicit clock".to_string())?;
    let samples_per_ui = mean_clock.period / dt;
    if !samples_per_ui.is_finite() || samples_per_ui < 2.0 {
        return Err("Recovered UI is below 2 samples: set a known rate/clock or use a faster record".into());
    }
    let clock = if explicit_clock { mean_clock }
        else { eye::recover_clock_pll(v, dt, threshold, hysteresis, Some(mean_clock.rate()), 0.0, true).unwrap_or(mean_clock) };
    let warning = if samples_per_ui < 10.0 { format!("Limited sampling: {:.1} samples/UI", samples_per_ui) }
        else if span < 0.05 { "Low amplitude: check signal/noise before interpreting jitter".into() }
        else { String::new() };
    Ok((Summary { rate: clock.rate(), threshold, hysteresis, samples_per_ui,
        recovery: if explicit_clock { "Explicit clock" } else { "Auto NRZ PLL" }, warning }, clock))
}

pub(super) fn probe_rate(v: &[f32], dt: f64) -> Option<f64> {
    let count = v.len().min(PROBE_POINTS);
    if count < 8 { return None; }
    let mut rates = Vec::new();
    for start in [0, v.len().saturating_sub(count) / 2, v.len().saturating_sub(count)] {
        if let Ok((summary, _)) = infer(&v[start..start + count], dt, None, false) { rates.push(summary.rate); }
        if v.len() <= count { break; }
    }
    rates.sort_by(f64::total_cmp);
    rates.get(rates.len() / 2).copied()
}

fn show_quality(page: &mut Page, summary: &Summary) {
    page.status[0] = format!("{}: {} threshold, {:.1} samples/UI{}", summary.recovery,
        si_short(summary.threshold as f64, "V"), summary.samples_per_ui,
        if summary.warning.is_empty() { "" } else { " (limited)" });
    // Preserve the second line's measured/model/upper-limit state.
    if !summary.warning.is_empty() {
        page.stats.push(cell("Quality", summary.warning.clone(), Kind::Missing));
    }
}

fn compute(mut job: Job) -> Result<Computed, String> {
    let (dt, t0, n) = if let Some(clock) = &job.clock {
        if clock.dt < job.data.dt { (clock.dt, clock.t0, clock.v.len()) } else { (job.data.dt, job.data.t0, job.data.v.len()) }
    } else { (job.data.dt, job.data.t0, job.data.v.len()) };
    let data = eye_on_grid(job.data, t0, dt, n);
    let clock_wave = job.clock.take().map(|clock| eye_on_grid(clock, t0, dt, n));
    let recovery_wave = clock_wave.as_deref().unwrap_or(&data);
    let (summary, clock) = infer(recovery_wave, dt, job.requested_rate, clock_wave.is_some())?;
    let st = stats(&data, dt);
    let (lo, hi, step) = eye_range(st.vmin as f64, st.vmax as f64);
    let mut eye = eye::Eye::new(job.layout.plot.w.max(64) as usize, job.layout.plot.h.max(64) as usize, 2, lo as f32, hi as f32);
    let gated;
    let accumulated = if clock_wave.is_some() { gated = eye::gate_around_clock(&data, dt, &clock, 1.5); &gated[..] } else { &data[..] };
    let fold = if clock_wave.is_some() {
        let height = |candidate: &eye::Clock| {
            let mut preview = eye::Eye::new(48, 48, 2, lo as f32, hi as f32);
            preview.accumulate(accumulated, dt, candidate);
            let h = preview.measure(candidate.period).height;
            if h.is_finite() { h } else { f64::NEG_INFINITY }
        };
        let shifted = eye::center_strobe(&clock);
        if height(&shifted) > height(&clock) { shifted } else { clock.clone() }
    } else { clock.clone() };
    eye.accumulate(accumulated, dt, &fold);
    let meas = eye.measure(clock.period);
    let tie = eye::tie(&clock.edges, &clock);
    let mut page = Page { vstep: step, ..Page::default() };
    page.add(&tie, &clock);
    page.paint(&eye);
    page.levels = (meas.one.is_finite() && meas.zero.is_finite()).then_some((meas.one, meas.zero));
    let mut sorted = page.pool.clone();
    sorted.sort_by(f64::total_cmp);
    let mut jitter = eye::jitter(&sorted);
    let rms = |(sum, count): (f64, u64)| if count == 0 { f64::NAN } else { (sum / count as f64).sqrt() };
    jitter.period_jitter_rms = rms(page.step);
    jitter.c2c_rms = rms(page.step2);
    let trend = eye::tie_trend(&clock.edges, &tie, &clock);
    let (_, plots) = charts_shown(&job.layout, job.big);
    page.chart_trend(&trend, &clock, plots[0].w.max(1) as usize);
    page.chart_hist(&jitter);
    let (pj, why) = page.chart_spec(&trend, &clock, plots[2].w.max(1) as usize);
    page.chart_tub(&sorted, &jitter, clock.period, plots[3].w.max(1) as usize);
    page.text_jitter(&jitter, pj, &why);
    page.text_eye(&eye, &meas, clock.period, dt);
    page.text_status(&jitter);
    page.title = format!("{} - fixed full record - {} - {}", job.source_name, si_short(summary.rate, "b/s"), summary.recovery);
    let means = page.means.clone();
    let (measured, model, eye_model) = populate_eye_facts(&mut page, job.settings, &clock, &tie, &trend,
        &jitter, &meas, &data, dt, &means, job.rail.as_ref().map(|wave| (wave.v.as_slice(), wave.dt)),
        clock_wave.as_ref().map(|wave| (data.as_slice(), wave.as_slice(), dt)));
    page.measured.extend(measured);
    page.model.extend(model);
    page.eye_model.extend(eye_model);
    show_quality(&mut page, &summary);
    Ok(Computed { summary, eye, page, meas, jitter, period: clock.period })
}

impl Ui {
    fn auto_eye_identity(&self, id: WaveId, out: &mut Vec<u64>) -> bool {
        match id {
            WaveId::Ref(i) => {
                let Some(r) = self.refs.get(i).and_then(Option::as_ref) else { return false };
                out.extend([1, i as u64, r.v.as_ptr() as u64, r.v.len() as u64, r.dt.to_bits(), r.t0.to_bits()]);
                let stride = (r.v.len() / 64).max(1);
                out.extend(r.v.iter().step_by(stride).map(|value| value.to_bits() as u64));
            }
            WaveId::Ch(c) => {
                let Some(rec) = self.last[0].as_ref().filter(|r| c < n_ch() && self.m.ch[c].on && !r.ch[c].is_empty()) else { return false };
                out.extend([0, c as u64, self.records as u64, rec.ch[c].as_ptr() as u64, rec.ch[c].len() as u64, rec.dt.to_bits(), rec.t0.to_bits()]);
            }
        }
        true
    }

    fn auto_eye_key(&self) -> Option<Key> {
        let mut key = vec![self.waveform_epoch(), self.m.eye_from_record as u64,
            if self.m.eye_from_record { 0 } else { self.m.eye_rate.to_bits() }, self.replay.sim_revision];
        if !self.auto_eye_identity(self.m.eye_src, &mut key) { return None; }
        for companion in [self.m.eye_b, self.m.eye_rail] {
            key.push(companion.is_some() as u64);
            if let Some(id) = companion { if !self.auto_eye_identity(id, &mut key) { return None; } }
        }
        key.extend([self.m.eye_ber.to_bits(), self.m.eye_pn_lo.to_bits(), self.m.eye_pn_hi.to_bits(),
            self.m.eye_notch.to_bits(), self.m.eye_npj as u64, self.m.eye_hp as u64]);
        let lay = layout(self.eye_page_rect(), self.full_on());
        key.extend([lay.plot.w as u64, lay.plot.h as u64, self.eye_big.map_or(u64::MAX, u64::from)]);
        Some(Key(key))
    }

    fn auto_eye_failure(&mut self, message: String) {
        self.azr.eye = None;
        self.azr.eye_res = None;
        self.azr.eye_auto.summary = None;
        self.azr.eye_auto.failed = true;
        self.azr.page = Page::default();
        self.azr.page.status = [format!("Auto Eye: {}", self.wave_name(self.m.eye_src)), message];
    }

    pub(in crate::ui) fn eye_auto_poll(&mut self) {
        let finished = self.azr.eye_auto.worker.as_mut().and_then(|worker| {
            match worker.receive.try_recv() {
                Ok(result) => { worker.busy = false; Some(result) }
                Err(_) => None,
            }
        });
        let Some(finished) = finished else { return };
        if !self.azr.eye_auto.enabled || !self.eye_open() { return; }
        if self.auto_eye_key().as_ref() != Some(&finished.key) {
            // Keep only metadata while old work finishes, then submit the latest
            // source/settings without requiring another click or acquisition.
            self.analyzer_update();
            return;
        }
        self.azr.eye_auto.applied = Some(finished.key);
        match finished.result {
            Ok(result) => {
                if self.m.eye_from_record { self.m.eye_rate = result.summary.rate; }
                self.azr.eye_auto.summary = Some(result.summary);
                self.azr.eye_auto.failed = false;
                self.azr.eye_auto.applied_results += 1;
                self.azr.eye = Some(result.eye);
                self.azr.page = result.page;
                self.azr.eye_res = Some((result.meas, result.jitter, result.period));
            }
            Err(message) => self.auto_eye_failure(message),
        }
    }

    /// Returns true when the async path or an explicit failure owns this update.
    pub(in crate::ui) fn eye_auto_update(&mut self, rec: &Record) -> bool {
        if !self.azr.eye_auto.enabled { return false; }
        self.eye_auto_poll();
        let Some(key) = self.auto_eye_key() else {
            self.auto_eye_failure("No samples on the selected source; choose a loaded or active source".into());
            return true;
        };
        let large = matches!(self.m.eye_src, WaveId::Ref(i) if !(self.session_is_sim && i == self.m.sim_slot)
            && self.refs[i].as_ref().is_some_and(|r| r.v.len() > BACKGROUND_POINTS));
        if self.azr.eye_auto.applied.as_ref() == Some(&key) {
            // A stopped live record is one observation, however often menus,
            // panning or repainting request analysis of those same samples.
            return large || matches!(self.m.eye_src, WaveId::Ch(_)) || self.azr.eye_auto.failed;
        }
        self.azr.eye_auto.wanted = Some(key.clone());
        let requested = (!self.m.eye_from_record && self.m.eye_rate > 0.0).then_some(self.m.eye_rate);
        if large {
            if self.azr.eye_auto.worker.is_none() { self.azr.eye_auto.worker = Some(Worker::new()); }
            if self.azr.eye_auto.worker.as_ref().unwrap().busy {
                self.azr.eye = None; self.azr.eye_res = None;
                self.azr.page = Page::default();
                self.azr.page.status = ["Computing Eye/Jitter".into(), "Waiting for the previous analysis; new settings are queued".into()];
                return true;
            }
            let Some(data) = self.eye_samples(rec, self.m.eye_src) else { return true };
            let clock = self.m.eye_b.and_then(|id| self.eye_samples(rec, id));
            let rail = self.m.eye_rail.and_then(|id| self.eye_samples(rec, id));
            let job = Job { key, source_name: self.wave_name(self.m.eye_src), data, clock, rail,
                requested_rate: requested, settings: FactSettings::from(&self.m), layout: layout(self.eye_page_rect(), self.full_on()), big: self.eye_big };
            let worker = self.azr.eye_auto.worker.as_mut().unwrap();
            if worker.send.try_send(job).is_ok() { worker.busy = true; self.azr.eye_auto.submitted += 1; }
            self.azr.eye = None; self.azr.eye_res = None;
            self.azr.page = Page::default();
            self.azr.page.status = ["Computing Eye/Jitter".into(), format!("{} - full stored record", self.wave_name(self.m.eye_src))];
            return true;
        }
        let Some(wave) = self.eye_samples(rec, self.m.eye_b.unwrap_or(self.m.eye_src)) else { return true };
        match infer(&wave.v, wave.dt, requested, self.m.eye_b.is_some()) {
            Ok((mut summary, _)) => {
                if self.m.eye_from_record {
                    // Noise changes the fitted mean slightly between captures;
                    // preserve its nominal UI so real fresh captures accumulate.
                    if self.m.eye_rate > 0.0 && (summary.rate / self.m.eye_rate - 1.0).abs() < 0.05 {
                        summary.rate = self.m.eye_rate;
                    }
                    self.m.eye_rate = summary.rate;
                }
                self.azr.eye_auto.summary = Some(summary);
                self.azr.eye_auto.failed = false;
                self.azr.eye_auto.applied = Some(key);
                false
            }
            Err(message) => {
                self.azr.eye_auto.applied = Some(key);
                self.auto_eye_failure(message);
                true
            }
        }
    }

    pub(in crate::ui) fn eye_auto_finish_status(&mut self) {
        if self.azr.eye_auto.enabled {
            if let Some(summary) = &self.azr.eye_auto.summary { show_quality(&mut self.azr.page, summary); }
        }
    }
}
