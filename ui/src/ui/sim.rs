// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM generates fresh serial captures through one bounded worker. Immutable
//! file references use stored-record replay; moving their viewport is analysis,
//! not a new physical SIM capture.
//!
//! This file holds the worker, the requests and the capture they finish into; `menu.rs` holds the SIM menu and
//! `tests.rs` the tests. A new SIM menu item goes into `menu.rs`, a new step of generation here.

use super::worker::Worker;
use super::*;
use oos_dsp::serial::{self, Lane, LANES};
use std::sync::{mpsc, Arc};

#[path = "sim_bus_candidate.rs"]
mod sim_bus_candidate;
#[cfg(test)]
#[path = "sim_bus_tests.rs"]
mod sim_bus_tests;

mod menu;
#[cfg(test)]
mod tests;

const PRBS: [u8; 6] = [7, 9, 13, 15, 23, 31];

struct Request {
    id: u64,
    lane: Lane,
    slot: usize,
    continuing: bool,
    fresh: bool,
    run: bool,
    guard: String,
    trigger: Option<super::reference_search::SimTrigger>,
    bus_trigger: Option<Result<sim_bus_candidate::Plan, &'static str>>,
}

struct Generated {
    samples: Vec<f32>,
    hits: Option<(usize, Arc<Vec<f64>>)>,
    bus_decode: Option<sim_bus_candidate::Decoded>,
    error: Option<&'static str>,
}

impl Generated {
    fn evaluate(request: &Request, samples: Vec<f32>) -> Self {
        if let Some(plan) = &request.bus_trigger {
            return match plan
                .as_ref()
                .map_err(|error| *error)
                .and_then(|plan| plan.evaluate(&samples, serial::dt_of(&request.lane)))
            {
                Ok(decoded) => Self {
                    samples,
                    hits: Some((decoded.primary_slot, Arc::clone(&decoded.hits))),
                    bus_decode: Some(decoded),
                    error: None,
                },
                Err(error) => Self { samples, hits: None, bus_decode: None, error: Some(error) },
            };
        }
        let hits = request
            .trigger
            .as_ref()
            .map(|trigger| (trigger.primary_slot, trigger.evaluate(&samples, serial::dt_of(&request.lane))));
        Self { samples, hits, bus_decode: None, error: None }
    }
}

/// One worker per UI. Knob changes replace queued requests, never spawn a
/// parallel set of multi-million-sample generators. Only the latest reply wins.
pub(super) struct Generation {
    thread: Worker<Request, (Request, Result<Generated, String>)>,
    next: u64,
    active: Option<u64>,
    run: bool,
    queued: Option<Request>,
    pending: Option<(Request, Generated)>,
}

impl Generation {
    fn new() -> Result<Self, String> {
        let thread = Worker::spawn("oos-sim", |request: Request| {
            let generated = serial::try_generate(&request.lane).map(|samples| Generated::evaluate(&request, samples));
            (request, generated)
        })
        .map_err(|error| format!("Could not start SIM generator: {error}"))?;
        Ok(Self { thread, next: 0, active: None, run: false, queued: None, pending: None })
    }

    fn submit(&mut self, mut request: Request) -> Result<(), String> {
        self.next = self.next.wrapping_add(1);
        request.id = self.next;
        self.active = Some(request.id);
        self.run = request.run;
        if self.thread.busy() {
            self.queued = Some(request);
            return Ok(());
        }
        self.thread.submit(request).map_err(|_| {
            self.active = None;
            "SIM generator stopped; return to Live and try again".into()
        })
    }

    fn advance_queue(&mut self) -> Result<(), String> {
        if self.thread.busy() {
            return Ok(());
        }
        let Some(request) = self.queued.take() else {
            return Ok(());
        };
        if self.active != Some(request.id) {
            return Ok(());
        }
        self.thread.submit(request).map_err(|_| {
            self.active = None;
            "SIM generator stopped".into()
        })
    }
}

/// Serial-record strip: levels, bit period, edges, widths. Two rows of six.
/// First six also have on-trace labels (pk-pk, amp, rise, fall, period, +width).
const SIM_MEAS: [Meas; 12] = [
    Meas::Vpp,
    Meas::Vamp,
    Meas::Rise,
    Meas::Fall,
    Meas::Period,
    Meas::PWidth,
    Meas::Freq,
    Meas::PDuty,
    Meas::Overshoot,
    Meas::NWidth,
    Meas::PSlew,
    Meas::PEdges,
];

impl Ui {
    fn sim_meas(slot: usize) -> Vec<(Meas, WaveId)> {
        SIM_MEAS.into_iter().map(|k| (k, WaveId::Ref(slot))).collect()
    }

    fn sim_lane(&self) -> Lane {
        let base = LANES[self.m.sim_preset.min(LANES.len() - 1)];
        Lane {
            name: base.name,
            bits_per_s: self.m.sim_rate.max(1e6),
            prbs: self.m.sim_prbs,
            v_high: self.m.sim_vhigh,
            v_low: self.m.sim_vlow,
            rise: self.m.sim_rise.max(1e-12),
            noise_rms: self.m.sim_noise.max(0.0),
            bw: self.m.sim_bw.max(1e6),
            deemph_db: self.m.sim_deemph.max(0.0),
            ssc: self.m.sim_ssc,
            ssc_center: self.m.sim_ssc_center,
            ssc_deviation: self.m.sim_ssc_deviation,
            ssc_hz: self.m.sim_ssc_hz,
            rj_rms: self.m.sim_rj.max(0.0),
            pj_peak: self.m.sim_pj.max(0.0),
            pj_hz: self.m.sim_pjhz.max(0.0),
            dcd_pp: self.m.sim_dcd_pp,
            corr_rj_rms: self.m.sim_corr_rj,
            corr_rj_hz: self.m.sim_corr_hz,
            seed: base.seed,
        }
    }

    /// Sample rate and length of the loaded SIM slot, when it is shown.
    #[cfg(test)]
    pub(super) fn sim_rate_depth(&self) -> Option<(f64, u64)> {
        let r = self.refs[self.m.sim_slot].as_ref()?;
        if !r.on || r.dt <= 0.0 {
            return None;
        }
        Some((1.0 / r.dt, r.v.len() as u64))
    }

    pub(super) fn sim_operation(&self) -> Option<(u64, u64)> {
        self.sim_generation.as_ref().and_then(|generation| generation.active.map(|id| (self.workflow_epoch, id)))
    }

    pub(crate) fn sim_generating(&self) -> bool {
        self.sim_generation.as_ref().is_some_and(|generation| generation.active.is_some())
    }

    fn sim_requested_running(&self) -> bool {
        self.sim_generation
            .as_ref()
            .filter(|generation| generation.active.is_some())
            .map_or(self.m.run == RunState::Run, |generation| generation.run)
    }

    fn sim_trigger_model(&self, lane: &Lane, slot: usize, continuing: bool) -> Model {
        let mut model = self.m.clone();
        if !continuing || model.trig_ref.is_none() {
            model.set_trigger_wave(WaveId::Ref(slot), false);
        }
        if !continuing {
            model.trig_level = (0.5 * (lane.v_high + lane.v_low)) as f32;
        }
        model
    }

    fn sim_candidate_vpd(&self, lane: &Lane, slot: usize, continuing: bool, run: bool) -> f32 {
        if continuing && run {
            self.refs[slot].as_ref().map_or(((lane.v_high - lane.v_low) / 6.0) as f32, |r| r.vpd)
        } else {
            nearest_vdiv(((lane.v_high - lane.v_low).abs().max(0.2) / 4.0) as f32)
        }
    }

    fn sim_trigger_guard(&self, lane: &Lane, slot: usize, continuing: bool, run: bool) -> String {
        let effective = self.sim_trigger_model(lane, slot, continuing);
        let vpd = self.sim_candidate_vpd(lane, slot, continuing, run);
        // Include the user's current configuration as well as the effective
        // initial-SIM defaults, but not unrelated live-frame allocations.
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}",
            self.m.trigger(),
            self.m.dec_cfg(),
            (self.m.trig_type, self.m.trig_bus, self.m.trig_bus_val),
            self.m.trigger_wave(),
            self.m.trigger_wave2(),
            self.trigger_vpd(false).to_bits(),
            self.trigger_vpd(true).to_bits(),
            self.reference_trigger_fingerprint(&effective, effective.trig_ref.unwrap_or(slot), Some((slot, vpd)))
        ) + &self.sim_bus_guard(&effective, slot)
    }

    fn sim_capture_request(&self, lane: Lane, slot: usize, continuing: bool, fresh: bool, run: bool) -> Request {
        let model = self.sim_trigger_model(&lane, slot, continuing);
        let vpd = self.sim_candidate_vpd(&lane, slot, continuing, run);
        Request {
            id: 0,
            lane,
            slot,
            continuing,
            fresh,
            run,
            guard: self.sim_trigger_guard(&lane, slot, continuing, run),
            trigger: self.prepare_sim_trigger(&model, slot, vpd),
            bus_trigger: if self.m.trig_on_la() {
                Some(Err("SIM qualification on logic-analyzer inputs is unavailable"))
            } else if matches!(model.trig_type, TrigType::Pattern | TrigType::Duration) {
                Some(Err("SIM Pattern/Duration qualification is unavailable"))
            } else if model.trig_type == TrigType::Bus {
                Some(self.prepare_sim_bus(&model, slot, vpd))
            } else {
                None
            },
        }
    }

    pub(crate) fn sim_cancel_generation(&mut self) {
        if let Some(generation) = &mut self.sim_generation {
            generation.active = None;
            generation.queued = None;
            generation.pending = None;
        }
    }

    pub(crate) fn take_sim_error(&mut self) -> Option<String> {
        self.sim_error.take()
    }

    fn sim_request_ui(&mut self, run: bool) {
        if let Err(error) = self.sim_load(run) {
            self.toast(&format!("SIM: {error}"));
        }
    }

    /// Preflight before changing a live session or its working reference. Long
    /// SSC records are generated off the UI thread; ordinary records stay fast.
    fn sim_load(&mut self, run: bool) -> Result<(), String> {
        self.sim_error = None;
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.sim_cancel_generation();
        let lane = self.sim_lane();
        let count = serial::sample_count(&lane)?;
        let continuing = self.session_is_sim && !self.replay.sim_initial_armed;
        if !continuing {
            self.sim_pick_free_slot(lane.name);
        }
        let mut request = self.sim_capture_request(lane, self.m.sim_slot, continuing, false, run);
        if let Some(Err(error)) = &request.bus_trigger {
            return Err((*error).into());
        }
        if request.bus_trigger.is_some() && run && !continuing {
            // Arming changes control ownership, never the accepted data. A miss
            // must keep retrying even before the first SIM reference exists.
            self.remember_live();
            self.session_is_sim = true;
            self.sim_play = true;
            self.replay.sim_initial_armed = true;
            self.m.run = RunState::Run;
            self.m.status = TrigStatus::Wait;
            self.m.set_trigger_wave(WaveId::Ref(self.m.sim_slot), false);
            request = self.sim_capture_request(lane, self.m.sim_slot, false, false, true);
        }
        if count <= 65_536 && request.bus_trigger.is_none() {
            let generated = serial::try_generate(&lane)?;
            self.sim_cancel_generation();
            let generated = Generated::evaluate(&request, generated);
            self.sim_finish(request, generated);
        } else {
            if self.sim_generation.is_none() {
                self.sim_generation = Some(Generation::new()?);
            }
            self.sim_generation.as_mut().unwrap().submit(request)?;
            self.toast(&format!("Generating {} ({:.1} M samples)", lane.name, count as f64 / 1e6));
        }
        Ok(())
    }

    /// A new simulation does not overwrite a ref the user loaded: it moves to the first free slot. With every slot in
    /// use it keeps the chosen one, and the toast at the end says which ref it replaced. A slot that holds the output
    /// of an earlier run of the same preset is simply reused.
    fn sim_pick_free_slot(&mut self, preset: &str) {
        let slot = self.m.sim_slot;
        if self.refs.get(slot).and_then(Option::as_ref).is_none_or(|r| r.label == preset) {
            return;
        }
        if let Some(free) = (0..NREF).find(|&i| self.refs[i].is_none()) {
            self.m.sim_slot = free;
        }
    }

    pub(super) fn sim_poll(&mut self) {
        let pending = self.sim_generation.as_mut().and_then(|generation| generation.pending.take());
        if let Some((request, generated)) = pending {
            if self.sim_generation.as_ref().is_some_and(|generation| generation.active == Some(request.id)) {
                self.sim_generation.as_mut().unwrap().active = None;
                self.sim_finish(request, generated);
            }
        }
        loop {
            let Some(generation) = &mut self.sim_generation else { return };
            match generation.thread.poll() {
                Ok((request, result)) => {
                    if generation.active != Some(request.id) {
                        if let Err(error) = generation.advance_queue() {
                            self.sim_error = Some(error);
                        }
                        continue;
                    }
                    generation.active = None;
                    match result {
                        Ok(generated) => self.sim_finish(request, generated),
                        Err(error) => {
                            self.sim_error = Some(error.clone());
                            self.toast(&format!("SIM: {error}"));
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if generation.active.take().is_some() {
                        self.sim_error = Some("SIM generator stopped; try again".into());
                        self.toast("SIM generator stopped; try again");
                    }
                    self.sim_generation = None;
                    return;
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn wait_sim_generation(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.sim_generating() {
            self.sim_poll();
            assert!(std::time::Instant::now() < deadline, "SIM generation timed out");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[cfg(test)]
    fn wait_sim_worker(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self
            .sim_generation
            .as_ref()
            .is_some_and(|generation| generation.thread.busy() || generation.queued.is_some())
        {
            self.sim_poll();
            assert!(std::time::Instant::now() < deadline, "SIM worker did not finish");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Commit one successful result. Overview is the stored record; zoom is ~eight UI.
    fn sim_finish(&mut self, request: Request, generated: Generated) {
        if request.guard != self.sim_trigger_guard(&request.lane, request.slot, request.continuing, request.run) {
            // This result belongs to a superseded trigger/source/scale snapshot.
            // Keep the last accepted record; the next running tick retries.
            if self.session_is_sim {
                self.replay.sim_next = self.now;
            } else {
                self.toast("SIM trigger changed while generating; press Play to use the new condition");
            }
            return;
        }
        if let Some(error) = generated.error {
            self.sim_error = Some(error.into());
            self.m.status = TrigStatus::Wait;
            self.toast(&format!("SIM: {error}"));
            return;
        }
        let operation = OperationToken::Sim(self.workflow_epoch, request.id);
        if request.bus_trigger.is_some()
            && request.run
            && generated.hits.as_ref().is_some_and(|(_, hits)| hits.is_empty())
            && self.m.trig_mode != TrigMode::Auto
            && !self.replay.force_requested()
        {
            self.scpi_finish_operation(operation);
            self.m.status = TrigStatus::Wait;
            return;
        }
        if request.fresh || (request.continuing && request.run) {
            self.sim_finish_capture(request, generated);
            if !self.sim_generating() {
                self.scpi_finish_operation(operation);
            }
            return;
        }
        self.scpi_finish_operation(operation);
        let Generated { samples: v, hits, bus_decode, .. } = generated;
        let Request { lane, slot, continuing, run, .. } = request;
        let initialize_measurements =
            self.refs.iter().all(Option::is_none) && self.m.meas == Self::default_meas() && self.m.meas2.is_empty();
        self.remember_live();
        self.session_is_sim = true;
        let forced = self.replay.force_requested();
        self.replay = Default::default();
        if forced {
            self.replay.request_force();
        }
        self.clear_measurement_statistics();
        let n = v.len();
        let dt = serial::dt_of(&lane);
        let span = (lane.v_high - lane.v_low).abs().max(0.2);
        let vpd = nearest_vdiv((span / 4.0) as f32);
        let replaced =
            self.refs[slot].as_ref().filter(|r| !continuing && r.label != lane.name).map(|r| r.label.clone());
        self.refs[slot] = Some(RefWave {
            src: 0,
            color: 0,
            label: lane.name.into(),
            t0: 0.0,
            dt,
            v: v.into(),
            vpd,
            offset: (-0.5 * (lane.v_high + lane.v_low)) as f32,
            on: true,
        });
        self.invalidate_reference_eye();
        if self.offline {
            self.offline_ref = slot;
        }
        // Required CH bus auxiliaries remain accepted immutable snapshots.
        let retained_channels = bus_decode.as_ref().map_or([false; NCH], |decoded| decoded.channels);
        // The SIM never switches an input off; the inputs its bus decode needs are switched on.
        for c in 0..NCH {
            self.m.ch[c].on |= retained_channels[c];
        }
        self.m.la.on = false;
        let rec_s = n as f64 * dt;
        let bit = nearest_tscale(8.0 / lane.bits_per_s / HDIV as f64);
        let want_ov = rec_s / HDIV as f64;
        let ts = tscales();
        let overview = ts.iter().position(|&t| t as f64 + 1e-18 >= want_ov).unwrap_or(ts.len() - 1);
        self.m.tfine = 1.0;
        self.m.tscale = overview.max(bit);
        let look = (rec_s * 0.5) as f32;
        self.m.delay = look;
        self.set_record_zoom(true);
        self.m.zscale = bit;
        self.m.zdelay = look;
        self.clamp_zoom();
        if !continuing {
            if initialize_measurements {
                self.m.meas = Self::sim_meas(slot);
                self.m.meas_labels = false;
            } else if !self.m.meas.is_empty() {
                // Starting a simulator must not erase the loaded-file catalog.
                // Add useful SIM readouts without duplicating an existing item.
                for item in Self::sim_meas(slot) {
                    if !self.m.meas.contains(&item) {
                        self.m.meas.push(item);
                    }
                }
            }
        }
        self.select_workspace_source(WaveId::Ref(slot));
        self.m.eye_src = WaveId::Ref(slot);
        self.m.eye_rate = lane.bits_per_s;
        if !continuing || self.m.trig_ref.is_none() {
            self.m.set_trigger_wave(WaveId::Ref(slot), false);
        }
        self.m.run = if run { RunState::Run } else { RunState::Stop };
        self.m.status = if run { TrigStatus::Wait } else { TrigStatus::Stop };
        if !continuing {
            self.m.trig_level = ((lane.v_high + lane.v_low) * 0.5) as f32;
        }
        if !retained_channels.iter().any(|on| *on) {
            self.last[0] = None;
        }
        self.cleared = true;
        self.sim_play = run;
        self.replay.fresh_sim = true;
        self.replay.sim_next = self.now + 0.5;
        if let Some(decoded) = bus_decode {
            self.seed_sim_bus_decode(&self.m, decoded.packets, Arc::clone(&decoded.hits));
        }
        if let Some((source, times)) = hits {
            self.install_reference_trigger_hits(source, times);
        }
        if run {
            self.replay_acquire(0.0);
        } else {
            self.refresh_stopped_reference();
        }
        self.ph.clear();
        self.measure_refs();
        self.analyzer_update();
        match replaced {
            Some(old) => self.toast(&format!("{} to R{}, replacing {}", lane.name, slot + 1, old)),
            None => self.toast(&format!("{} to R{}", lane.name, slot + 1)),
        }
    }

    pub(crate) fn sim_preset_name(&self) -> &'static str {
        self.sim_lane().name
    }

    /// Selects a preset by the name on the SIM menu.
    pub(crate) fn sim_preset_set(&mut self, name: &str) -> bool {
        let name = name.trim().trim_matches('"');
        match serial::LANES.iter().position(|l| l.name.eq_ignore_ascii_case(name)) {
            Some(i) => {
                self.sim_cancel_generation();
                self.m.sim_preset = i;
                self.m.apply_sim_preset();
                true
            }
            None => false,
        }
    }

    pub(crate) fn sim_slot(&self) -> usize {
        self.m.sim_slot
    }

    pub(crate) fn sim_slot_set(&mut self, slot: usize) -> bool {
        if slot >= NREF {
            return false;
        }
        self.sim_cancel_generation();
        self.m.sim_slot = slot;
        true
    }

    pub(crate) fn sim_playing(&self) -> bool {
        self.sim_play
    }

    /// On starts fresh captures of the selected preset. Off holds the accepted record.
    pub(crate) fn sim_play_set(&mut self, on: bool) -> Result<(), String> {
        if on {
            self.sim_load(true)?;
        } else {
            self.sim_cancel_generation();
            self.sim_play = false;
            self.m.run = RunState::Stop;
            self.m.status = TrigStatus::Stop;
        }
        Ok(())
    }

    pub(crate) fn sim_shown(&self) -> Option<bool> {
        self.refs.get(self.m.sim_slot).and_then(|r| r.as_ref()).map(|r| r.on)
    }

    pub(crate) fn sim_show_set(&mut self, on: bool) -> bool {
        let Some(r) = self.refs.get_mut(self.m.sim_slot).and_then(|r| r.as_mut()) else { return false };
        r.on = on;
        true
    }

    /// Each SIM acquisition is a newly generated physical record, not an animated
    /// viewport over one seeded record. The single worker bounds cadence and memory.
    pub(super) fn sim_tick(&mut self, dt_wall: f64) {
        if !self.session_is_sim {
            self.replay_acquire(dt_wall);
            return;
        }
        // A large initial or external-reference scan must be serviced before
        // scheduling a replacement. Its source stays installed while pending.
        if self.m.run == RunState::Run
            && self.sim_play
            && self.replay.fresh_sim
            && self.replay.captures == 0
            && !self.sim_generating()
        {
            self.replay_acquire(dt_wall);
            if self.reference_search_busy() {
                return;
            }
        }
        if self.m.run != RunState::Run || !self.sim_play || self.sim_generating() || self.now < self.replay.sim_next {
            return;
        }
        // Finish analysis/statistics for the accepted record before replacing
        // it, so large Full-record readouts cannot be perpetually superseded.
        if self.measurement_busy() {
            return;
        }
        self.replay.sim_next = self.now + 0.5;
        let mut lane = self.sim_lane();
        self.replay.sim_attempt = self.replay.sim_attempt.wrapping_add(1);
        lane.seed ^= self.replay.sim_attempt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        if let Err(error) = serial::sample_count(&lane) {
            self.sim_error = Some(error);
            self.m.run = RunState::Stop;
            self.sim_play = false;
            return;
        }
        if self.sim_generation.is_none() {
            match Generation::new() {
                Ok(worker) => self.sim_generation = Some(worker),
                Err(error) => {
                    self.sim_error = Some(error);
                    return;
                }
            }
        }
        let initial = self.replay.sim_initial_armed;
        let request = self.sim_capture_request(lane, self.m.sim_slot, !initial, !initial, true);
        if let Some(Err(error)) = &request.bus_trigger {
            self.sim_error = Some((*error).into());
            self.m.status = TrigStatus::Wait;
            return;
        }
        if let Err(error) = self.sim_generation.as_mut().unwrap().submit(request) {
            self.sim_error = Some(error);
        }
    }

    fn sim_finish_capture(&mut self, request: Request, generated: Generated) {
        if !self.session_is_sim || !self.sim_play || self.m.run != RunState::Run || request.slot != self.m.sim_slot {
            return;
        }
        if generated.hits.is_none() {
            if !self.reference_search_supported() {
                self.m.status = TrigStatus::Wait;
                return;
            }
            let source = self.m.trig_ref.unwrap_or_else(|| self.active_reference());
            if self.reference_trigger_hits(source).is_none() {
                // The actual external trigger source has not finished scanning.
                // Retain one candidate off-screen; polling never installs it.
                if self.sim_generation.is_none() {
                    match Generation::new() {
                        Ok(worker) => self.sim_generation = Some(worker),
                        Err(error) => {
                            self.sim_error = Some(error);
                            return;
                        }
                    }
                }
                let generation = self.sim_generation.as_mut().unwrap();
                generation.active = Some(request.id);
                generation.run = request.run;
                generation.pending = Some((request, generated));
                self.m.status = TrigStatus::Wait;
                return;
            }
        }
        let Generated { samples, hits, bus_decode, .. } = generated;
        let previous = self.refs[request.slot].take();
        let dt = serial::dt_of(&request.lane);
        let midpoint = 0.5 * (request.lane.v_high + request.lane.v_low);
        self.refs[request.slot] = Some(RefWave {
            src: 0,
            color: previous.as_ref().map_or(0, |r| r.color),
            label: request.lane.name.into(),
            t0: 0.0,
            dt,
            v: (samples).into(),
            vpd: previous.as_ref().map_or(((request.lane.v_high - request.lane.v_low) / 6.0) as f32, |r| r.vpd),
            offset: previous.as_ref().map_or(-midpoint as f32, |r| r.offset),
            on: true,
        });
        let captures = self.replay.captures;
        let old_rate = self.m.eye_rate;
        if !request.fresh {
            self.m.eye_rate = request.lane.bits_per_s;
        }
        self.replay.sim_config_change = !request.fresh;
        self.replay.fresh_sim = true;
        self.replay.next = 0.0;
        if let Some(decoded) = bus_decode {
            self.seed_sim_bus_decode(&self.m, decoded.packets, Arc::clone(&decoded.hits));
        }
        if let Some((source, times)) = hits {
            self.install_reference_trigger_hits(source, times);
        }
        self.replay_acquire(0.0);
        if self.replay.captures == captures {
            // Normal/Single without a hit holds the last accepted acquisition.
            self.refs[request.slot] = previous;
            self.m.eye_rate = old_rate;
            self.replay.reset_search();
            self.replay.fresh_sim = false;
            self.replay.sim_config_change = false;
        }
    }
}

fn nearest_vdiv(want: f32) -> f32 {
    VSCALES.iter().copied().min_by(|a, b| (a - want).abs().partial_cmp(&(b - want).abs()).unwrap()).unwrap_or(0.2)
}

fn nearest_tscale(want: f64) -> usize {
    let ts = tscales();
    ts.iter()
        .copied()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a as f64 - want).abs().partial_cmp(&(*b as f64 - want).abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0)
}
