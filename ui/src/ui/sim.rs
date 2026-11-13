// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM generates fresh serial captures through one bounded worker. Immutable
//! file references use stored-record replay; moving their viewport is analysis,
//! not a new physical SIM capture.

use super::*;
use oos_dsp::serial::{self, Lane, LANES};
use std::sync::{Arc, mpsc::{self, Receiver, Sender}};

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
}

struct Generated {
    samples: Vec<f32>,
    hits: Option<(usize, Arc<Vec<f64>>)>,
}

impl Generated {
    fn evaluate(request: &Request, samples: Vec<f32>) -> Self {
        let hits = request.trigger.as_ref().map(|trigger| (trigger.primary_slot, trigger.evaluate(&samples, serial::dt_of(&request.lane))));
        Self { samples, hits }
    }
}

/// One worker per UI. Knob changes replace queued requests, never spawn a
/// parallel set of multi-million-sample generators. Only the latest reply wins.
pub(super) struct Generation {
    requests: Sender<Request>,
    results: Receiver<(Request, Result<Generated, String>)>,
    next: u64,
    active: Option<u64>,
    run: bool,
    in_flight: bool,
    queued: Option<Request>,
    pending: Option<(Request, Generated)>,
}

impl Generation {
    fn new() -> Result<Self, String> {
        let (requests, input) = mpsc::channel::<Request>();
        let (output, results) = mpsc::channel();
        std::thread::Builder::new().name("oos-sim".into()).spawn(move || {
            while let Ok(mut request) = input.recv() {
                while let Ok(newer) = input.try_recv() { request = newer; }
                let generated = serial::try_generate(&request.lane).map(|samples| Generated::evaluate(&request, samples));
                if output.send((request, generated)).is_err() { break; }
            }
        }).map_err(|error| format!("Could not start SIM generator: {error}"))?;
        Ok(Self { requests, results, next: 0, active: None, run: false, in_flight: false,
            queued: None, pending: None })
    }

    fn submit(&mut self, mut request: Request) -> Result<(), String> {
        self.next = self.next.wrapping_add(1);
        request.id = self.next;
        self.active = Some(request.id);
        self.run = request.run;
        if self.in_flight { self.queued = Some(request); return Ok(()); }
        self.in_flight = true;
        self.requests.send(request).map_err(|_| {
            self.active = None;
            self.in_flight = false;
            "SIM generator stopped; return to Live and try again".into()
        })
    }

    fn advance_queue(&mut self) -> Result<(), String> {
        if self.in_flight { return Ok(()); }
        let Some(request) = self.queued.take() else { return Ok(()); };
        if self.active != Some(request.id) { return Ok(()); }
        self.in_flight = true;
        self.requests.send(request).map_err(|_| { self.in_flight = false; self.active = None; "SIM generator stopped".into() })
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

    pub(super) fn sim_items(&self) -> Vec<Item> {
        let mut items = vec![
            Item::SimPreset,
            Item::SimRate,
            Item::SimPat,
            Item::SimHigh,
            Item::SimLow,
            Item::SimRise,
            Item::SimBw,
            Item::SimDeemph,
            Item::SimNoise,
            Item::SimSsc,
            Item::Open(MenuId::SimAdvanced),
            Item::SimRj,
            Item::SimPj,
            Item::SimPjHz,
            Item::SimSlot,
            Item::SimPlay,
            Item::SimShow,
            Item::SimDefaults,
        ];
        if self.record_mode() { items.insert(0, Item::SimLive); }
        items
    }

    pub(super) fn sim_advanced_items(&self) -> Vec<Item> {
        vec![Item::SimSsc, Item::SimSscCenter, Item::SimSscDeviation, Item::SimSscHz,
             Item::SimDcd, Item::SimCorrRj, Item::SimCorrHz, Item::SimDefaults]
    }

    pub(super) fn sim_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SimPlay | Item::SimLive | Item::SimDefaults => Kind::Action,
            Item::SimPreset | Item::SimPat | Item::SimSsc | Item::SimSscCenter | Item::SimSlot | Item::SimShow => Kind::Choice,
            Item::SimRate | Item::SimHigh | Item::SimLow | Item::SimRise | Item::SimBw | Item::SimDeemph | Item::SimNoise | Item::SimRj | Item::SimPj | Item::SimPjHz
            | Item::SimSscDeviation | Item::SimSscHz | Item::SimDcd | Item::SimCorrRj | Item::SimCorrHz => Kind::Value,
            _ => return None,
        })
    }

    pub(super) fn sim_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Sim) => "SIM",
            Item::Open(MenuId::SimAdvanced) => "Advanced",
            Item::SimPreset => "Preset",
            Item::SimRate => "Rate",
            Item::SimPat => "Pattern",
            Item::SimHigh => "High",
            Item::SimLow => "Low",
            Item::SimRise => "Rise 20-80%",
            Item::SimBw => "Bandwidth",
            Item::SimDeemph => "De-emph",
            Item::SimNoise => "Noise",
            Item::SimSsc => "Spread",
            Item::SimSscCenter => "SSC profile",
            Item::SimSscDeviation => "SSC deviation",
            Item::SimSscHz => "SSC frequency",
            Item::SimDcd => "DCD p-p",
            Item::SimCorrRj => "Corr RJ RMS",
            Item::SimCorrHz => "Corr RJ corner",
            Item::SimRj => "RJ RMS",
            Item::SimPj => "PJ peak",
            Item::SimPjHz => "PJ freq",
            Item::SimSlot => "Slot",
            Item::SimPlay => "Play",
            Item::SimLive => "Live",
            Item::SimShow => "Show",
            Item::SimDefaults => "Defaults",
            _ => return None,
        })
    }

    pub(super) fn sim_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::SimPreset => self.sim_lane().name.into(),
            Item::SimRate => si_short(m.sim_rate, "b/s"),
            Item::SimPat => format!("PRBS-{}", m.sim_prbs),
            Item::SimHigh => si_short(m.sim_vhigh, "V"),
            Item::SimLow => si_short(m.sim_vlow, "V"),
            Item::SimRise => si_short(m.sim_rise, "s"),
            Item::SimBw => si_short(m.sim_bw, "Hz"),
            Item::SimDeemph => format!("{:.1} dB", m.sim_deemph),
            Item::SimNoise => si_short(m.sim_noise, "V"),
            Item::SimSsc => if m.sim_ssc { "On".into() } else { "Off".into() },
            Item::SimSscCenter => if m.sim_ssc_center { "Center".into() } else { "Down, DC removed".into() },
            Item::SimSscDeviation => format!("{:.2} % {}", 100.0 * m.sim_ssc_deviation, if m.sim_ssc_center { "pk" } else { "p-p" }),
            Item::SimSscHz => si_short(m.sim_ssc_hz, "Hz"),
            Item::SimDcd => si_short(m.sim_dcd_pp, "s"),
            Item::SimCorrRj => si_short(m.sim_corr_rj, "s"),
            Item::SimCorrHz => si_short(m.sim_corr_hz, "Hz"),
            Item::SimRj => si_short(m.sim_rj, "s"),
            Item::SimPj => si_short(m.sim_pj, "s"),
            Item::SimPjHz => if m.sim_pj > 0.0 { si_short(m.sim_pjhz, "Hz") } else { "off".into() },
            Item::SimSlot => format!("R{}", m.sim_slot + 1),
            Item::SimPlay => String::new(),
            Item::SimDefaults => String::new(),
            Item::SimShow => match &self.refs[m.sim_slot] {
                None => "empty".into(),
                Some(r) => {
                    let name = if r.label.is_empty() { format!("R{}", m.sim_slot + 1) } else { r.label.clone() };
                    format!("{} {}", name, if r.on { "on" } else { "off" })
                }
            },
            _ => return None,
        })
    }

    pub(super) fn sim_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::SimPreset => serial::names().into_iter().map(|s| s.to_string()).collect(),
            Item::SimPat => PRBS.iter().map(|n| format!("PRBS-{n}")).collect(),
            Item::SimSsc => vec!["Off".into(), "On".into()],
            Item::SimSscCenter => vec!["Down, DC removed".into(), "Center".into()],
            Item::SimSlot => (0..NREF).map(|i| format!("R{}", i + 1)).collect(),
            Item::SimShow => vec!["Off".into(), "On".into()],
            _ => return None,
        })
    }

    pub(super) fn sim_adjust(&mut self, it: Item, d: i32) -> bool {
        let running = self.sim_requested_running();
        match it {
            Item::SimPreset => {
                let n = LANES.len() as i32;
                self.m.sim_preset = (self.m.sim_preset as i32 + d).rem_euclid(n) as usize;
                self.m.apply_sim_preset();
            }
            Item::SimRate => self.m.sim_rate = eng_ladder(self.m.sim_rate.max(1e8), d, 1e8, 64e9, false),
            Item::SimPat => {
                let i = PRBS.iter().position(|&p| p == self.m.sim_prbs).unwrap_or(0);
                self.m.sim_prbs = PRBS[(i as i32 + d).rem_euclid(PRBS.len() as i32) as usize];
            }
            Item::SimHigh => self.m.sim_vhigh = (self.m.sim_vhigh + 0.05 * d as f64).clamp(-2.0, 2.0),
            Item::SimLow => self.m.sim_vlow = (self.m.sim_vlow + 0.05 * d as f64).clamp(-2.0, 2.0),
            Item::SimRise => self.m.sim_rise = eng_ladder(self.m.sim_rise.max(5e-12), d, 5e-12, 2e-9, false),
            Item::SimBw => self.m.sim_bw = eng_ladder(self.m.sim_bw.max(10e6), d, 10e6, 50e9, false),
            Item::SimDeemph => self.m.sim_deemph = (self.m.sim_deemph + 0.5 * d as f64).clamp(0.0, 12.0),
            Item::SimNoise => self.m.sim_noise = eng_ladder(self.m.sim_noise.max(100e-6), d, 100e-6, 50e-3, false),
            Item::SimSsc => self.m.sim_ssc = !self.m.sim_ssc,
            Item::SimSscCenter => self.m.sim_ssc_center = !self.m.sim_ssc_center,
            Item::SimSscDeviation => self.m.sim_ssc_deviation = (self.m.sim_ssc_deviation + d as f64 * 0.0005).clamp(0.0, 0.05),
            Item::SimSscHz => self.m.sim_ssc_hz = eng_ladder(self.m.sim_ssc_hz, d, 1e3, 1e6, false),
            Item::SimDcd => self.m.sim_dcd_pp = eng_ladder(self.m.sim_dcd_pp, d, 0.5e-12, 100e-12, true),
            Item::SimCorrRj => self.m.sim_corr_rj = eng_ladder(self.m.sim_corr_rj, d, 0.1e-12, 50e-12, true),
            Item::SimCorrHz => self.m.sim_corr_hz = eng_ladder(self.m.sim_corr_hz, d, 1e3, 100e6, false),
            Item::SimRj => self.m.sim_rj = eng_ladder(self.m.sim_rj.max(0.1e-12), d, 0.1e-12, 50e-12, false),
            Item::SimPj => self.m.sim_pj = eng_ladder(self.m.sim_pj, d, 0.5e-12, 50e-12, true),
            Item::SimPjHz => self.m.sim_pjhz = eng_ladder(self.m.sim_pjhz.max(1e5), d, 1e5, 50e6, false),
            Item::SimSlot => self.m.sim_slot = (self.m.sim_slot as i32 + d).rem_euclid(NREF as i32) as usize,
            Item::SimShow => {
                if let Some(r) = &mut self.refs[self.m.sim_slot] {
                    r.on = !r.on;
                }
            }
            _ => return false,
        }
        if (self.session_is_sim || self.sim_generating()) && it != Item::SimShow {
            self.sim_request_ui(running);
        }
        true
    }

    pub(super) fn sim_activate(&mut self, it: Item) -> bool {
        match it {
            Item::SimLive => self.return_to_live(),
            Item::SimPlay if self.sim_generating() => {
                self.sim_cancel_generation();
                self.toast("SIM generation cancelled");
            }
            Item::SimPlay if self.session_is_sim => { self.record_key(key::RUNSTOP); },
            Item::SimPlay => self.sim_request_ui(true),
            Item::SimDefaults => {
                let regenerate = self.session_is_sim || self.sim_generating();
                let running = self.sim_requested_running();
                self.m.apply_sim_preset();
                if regenerate { self.sim_request_ui(running); }
                else { self.toast("SIM knobs from the preset"); }
            }
            _ => return false,
        }
        true
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
    pub(super) fn sim_rate_depth(&self) -> Option<(f64, u64)> {
        let r = self.refs[self.m.sim_slot].as_ref()?;
        if !r.on || r.dt <= 0.0 {
            return None;
        }
        Some((1.0 / r.dt, r.v.len() as u64))
    }

    pub(crate) fn sim_generating(&self) -> bool {
        self.sim_generation.as_ref().is_some_and(|generation| generation.active.is_some())
    }

    fn sim_requested_running(&self) -> bool {
        self.sim_generation.as_ref().filter(|generation| generation.active.is_some())
            .map_or(self.m.run == RunState::Run, |generation| generation.run)
    }

    fn sim_trigger_model(&self, lane: &Lane, slot: usize, continuing: bool) -> Model {
        let mut model = self.m.clone();
        if !continuing || model.trig_ref.is_none() { model.set_trigger_wave(WaveId::Ref(slot), false); }
        if !continuing { model.trig_level = (0.5 * (lane.v_high + lane.v_low)) as f32; }
        model
    }

    fn sim_candidate_vpd(&self, lane: &Lane, slot: usize, continuing: bool, run: bool) -> f32 {
        if continuing && run {
            self.refs[slot].as_ref().map_or(((lane.v_high - lane.v_low) / 6.0) as f32, |r| r.vpd)
        } else { nearest_vdiv(((lane.v_high - lane.v_low).abs().max(0.2) / 4.0) as f32) }
    }

    fn sim_trigger_guard(&self, lane: &Lane, slot: usize, continuing: bool, run: bool) -> String {
        let effective = self.sim_trigger_model(lane, slot, continuing);
        let vpd = self.sim_candidate_vpd(lane, slot, continuing, run);
        // Include the user's current configuration as well as the effective
        // initial-SIM defaults, but not unrelated live-frame allocations.
        format!("{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}", self.m.trigger(), self.m.dec_cfg(),
            (self.m.trig_type, self.m.trig_bus, self.m.trig_bus_val), self.m.trigger_wave(), self.m.trigger_wave2(),
            self.trigger_vpd(false).to_bits(), self.trigger_vpd(true).to_bits(),
            self.reference_trigger_fingerprint(&effective, effective.trig_ref.unwrap_or(slot), Some((slot, vpd))))
    }

    fn sim_capture_request(&self, lane: Lane, slot: usize, continuing: bool, fresh: bool, run: bool) -> Request {
        let model = self.sim_trigger_model(&lane, slot, continuing);
        let vpd = self.sim_candidate_vpd(&lane, slot, continuing, run);
        Request { id: 0, lane, slot, continuing, fresh, run,
            guard: self.sim_trigger_guard(&lane, slot, continuing, run), trigger: self.prepare_sim_trigger(&model, slot, vpd) }
    }

    pub(crate) fn sim_cancel_generation(&mut self) {
        if let Some(generation) = &mut self.sim_generation {
            generation.active = None; generation.queued = None; generation.pending = None;
        }
    }

    pub(crate) fn take_sim_error(&mut self) -> Option<String> {
        self.sim_error.take()
    }

    fn sim_request_ui(&mut self, run: bool) {
        if let Err(error) = self.sim_load(run) { self.toast(&format!("SIM: {error}")); }
    }

    /// Preflight before changing a live session or its working reference. Long
    /// SSC records are generated off the UI thread; ordinary records stay fast.
    fn sim_load(&mut self, run: bool) -> Result<(), String> {
        self.sim_error = None;
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.sim_cancel_generation();
        let lane = self.sim_lane();
        let count = serial::sample_count(&lane)?;
        let request = self.sim_capture_request(lane, self.m.sim_slot, self.session_is_sim, false, run);
        if count <= 65_536 {
            let generated = serial::try_generate(&lane)?;
            self.sim_cancel_generation();
            let generated = Generated::evaluate(&request, generated);
            self.sim_finish(request, generated);
        } else {
            if self.sim_generation.is_none() { self.sim_generation = Some(Generation::new()?); }
            self.sim_generation.as_mut().unwrap().submit(request)?;
            self.toast(&format!("Generating {} ({:.1} M samples)", lane.name, count as f64 / 1e6));
        }
        Ok(())
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
            match generation.results.try_recv() {
                Ok((request, result)) => {
                    generation.in_flight = false;
                    if generation.active != Some(request.id) {
                        if let Err(error) = generation.advance_queue() { self.sim_error = Some(error); }
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
        while self.sim_generation.as_ref().is_some_and(|generation| generation.in_flight || generation.queued.is_some()) {
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
            if self.session_is_sim { self.replay.sim_next = self.now; }
            else { self.toast("SIM trigger changed while generating; press Play to use the new condition"); }
            return;
        }
        if request.fresh || (request.continuing && request.run) { self.sim_finish_capture(request, generated); return; }
        let Generated { samples: v, hits } = generated;
        let Request { lane, slot, continuing, run, .. } = request;
        let initialize_measurements = self.refs.iter().all(Option::is_none)
            && self.m.meas == Self::default_meas() && self.m.meas2.is_empty();
        self.remember_live();
        self.session_is_sim = true;
        self.replay = Default::default();
        self.mstat.clear();
        let n = v.len();
        let dt = serial::dt_of(&lane);
        let span = (lane.v_high - lane.v_low).abs().max(0.2);
        let vpd = nearest_vdiv((span / 4.0) as f32);
        self.refs[slot] = Some(RefWave {
            src: 0,
            color: 0,
            label: lane.name.into(),
            t0: 0.0,
            dt,
            v,
            vpd,
            offset: (-0.5 * (lane.v_high + lane.v_low)) as f32,
            on: true,
        });
        self.invalidate_reference_eye();
        if self.offline { self.offline_ref = slot; }
        // a stored record, not the live analog path
        for c in 0..NCH {
            self.m.ch[c].on = false;
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
                    if !self.m.meas.contains(&item) { self.m.meas.push(item); }
                }
            }
        }
        self.select_workspace_source(WaveId::Ref(slot));
        self.m.eye_src = WaveId::Ref(slot);
        self.m.eye_rate = lane.bits_per_s;
        if !continuing || self.m.trig_ref.is_none() { self.m.set_trigger_wave(WaveId::Ref(slot), false); }
        self.m.run = if run { RunState::Run } else { RunState::Stop };
        self.m.status = if run { TrigStatus::Wait } else { TrigStatus::Stop };
        if !continuing { self.m.trig_level = ((lane.v_high + lane.v_low) * 0.5) as f32; }
        self.last[0] = None;
        self.cleared = true;
        self.sim_play = run;
        self.replay.fresh_sim = true;
        self.replay.sim_next = self.now + 0.5;
        if let Some((source, times)) = hits { self.install_reference_trigger_hits(source, times); }
        if run { self.replay_acquire(0.0); }
        else { self.refresh_stopped_reference(); }
        self.ph.clear();
        self.measure_refs();
        self.analyzer_update();
        self.toast(&format!("{} to R{}", lane.name, slot + 1));
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
        if !self.session_is_sim { self.replay_acquire(dt_wall); return; }
        // A large initial or external-reference scan must be serviced before
        // scheduling a replacement. Its source stays installed while pending.
        if self.m.run == RunState::Run && self.sim_play && self.replay.fresh_sim && self.replay.captures == 0 && !self.sim_generating() {
            self.replay_acquire(dt_wall);
            if self.reference_search_busy() { return; }
        }
        if self.m.run != RunState::Run || !self.sim_play || self.sim_generating() || self.now < self.replay.sim_next { return; }
        // Finish analysis/statistics for the accepted record before replacing
        // it, so large Full-record readouts cannot be perpetually superseded.
        if self.measurement_busy() { return; }
        self.replay.sim_next = self.now + 0.5;
        let mut lane = self.sim_lane();
        self.replay.sim_attempt = self.replay.sim_attempt.wrapping_add(1);
        lane.seed ^= self.replay.sim_attempt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        if let Err(error) = serial::sample_count(&lane) { self.sim_error = Some(error); self.m.run = RunState::Stop; self.sim_play = false; return; }
        if self.sim_generation.is_none() {
            match Generation::new() { Ok(worker) => self.sim_generation = Some(worker), Err(error) => { self.sim_error = Some(error); return; } }
        }
        let request = self.sim_capture_request(lane, self.m.sim_slot, true, true, true);
        if let Err(error) = self.sim_generation.as_mut().unwrap().submit(request) { self.sim_error = Some(error); }
    }

    fn sim_finish_capture(&mut self, request: Request, generated: Generated) {
        if !self.session_is_sim || !self.sim_play || self.m.run != RunState::Run || request.slot != self.m.sim_slot { return; }
        if generated.hits.is_none() {
            if !self.reference_search_supported() { self.m.status = TrigStatus::Wait; return; }
            let source = self.m.trig_ref.unwrap_or_else(|| self.active_reference());
            if self.reference_trigger_hits(source).is_none() {
                // The actual external trigger source has not finished scanning.
                // Retain one candidate off-screen; polling never installs it.
                if self.sim_generation.is_none() {
                    match Generation::new() { Ok(worker) => self.sim_generation = Some(worker), Err(error) => { self.sim_error = Some(error); return; } }
                }
                let generation = self.sim_generation.as_mut().unwrap();
                generation.active = Some(request.id);
                generation.run = request.run;
                generation.pending = Some((request, generated));
                self.m.status = TrigStatus::Wait;
                return;
            }
        }
        let Generated { samples, hits } = generated;
        let previous = self.refs[request.slot].take();
        let dt = serial::dt_of(&request.lane);
        let midpoint = 0.5 * (request.lane.v_high + request.lane.v_low);
        self.refs[request.slot] = Some(RefWave {
            src: 0, color: previous.as_ref().map_or(0, |r| r.color), label: request.lane.name.into(),
            t0: 0.0, dt, v: samples,
            vpd: previous.as_ref().map_or(((request.lane.v_high - request.lane.v_low) / 6.0) as f32, |r| r.vpd),
            offset: previous.as_ref().map_or(-midpoint as f32, |r| r.offset), on: true,
        });
        let captures = self.replay.captures;
        let old_rate = self.m.eye_rate;
        if !request.fresh { self.m.eye_rate = request.lane.bits_per_s; }
        self.replay.sim_config_change = !request.fresh;
        self.replay.fresh_sim = true;
        self.replay.next = 0.0;
        if let Some((source, times)) = hits { self.install_reference_trigger_hits(source, times); }
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
    ts.iter().copied().enumerate().min_by(|(_, a), (_, b)| (*a as f64 - want).abs().partial_cmp(&(*b as f64 - want).abs()).unwrap()).map(|(i, _)| i).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn ui() -> Ui {
        Ui::new(Source::Demo { t: 0.0 })
    }

    #[test]
    fn advanced_sim_controls_are_reachable_and_use_explicit_units() {
        let mut u = ui();
        u.open_menu(MenuId::Sim);
        assert!(u.items(MenuId::Sim).contains(&Item::Open(MenuId::SimAdvanced)));
        u.activate(Item::Open(MenuId::SimAdvanced));
        assert_eq!(u.menu, MenuId::SimAdvanced);
        assert_eq!(u.parent, Some(MenuId::Sim));
        assert_eq!(u.menu_root(), MenuId::Sim);
        let items = u.items(MenuId::SimAdvanced);
        for expected in [Item::SimSscCenter, Item::SimSscDeviation, Item::SimSscHz,
                         Item::SimDcd, Item::SimCorrRj, Item::SimCorrHz] {
            assert!(items.contains(&expected));
        }
        assert!(u.value(Item::SimSscDeviation).contains("p-p"));
        u.adjust(Item::SimSscCenter, 1);
        assert!(u.value(Item::SimSscDeviation).contains("pk"));
        assert!(u.options(Item::SimPat).contains(&"PRBS-13".into()));
        u.m.sim_prbs = 9;
        u.adjust(Item::SimPat, 1);
        assert_eq!(u.sim_lane().prbs, 13);
        let dialog = u.dlg_layout().unwrap();
        for row in &dialog.rows {
            assert!(row.ctrl.x >= dialog.r.x && row.ctrl.x + row.ctrl.w <= dialog.r.x + dialog.r.w);
            assert!(row.ctrl.y >= dialog.r.y && row.ctrl.y + row.ctrl.h <= dialog.r.y + dialog.r.h);
        }
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/sim-advanced.png"), crate::store::png_of(&cv));
        }
        let back = dialog.back.unwrap();
        u.tap_menu(back.x + back.w / 2, back.y + back.h / 2);
        assert_eq!(u.menu, MenuId::Sim);
    }

    #[test]
    fn defaults_regenerate_a_stopped_sim_and_reset_advanced_knobs() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 3.2 Gen 2").unwrap();
        u.m.apply_sim_preset();
        u.m.sim_dcd_pp = 20e-12;
        u.m.sim_corr_rj = 4e-12;
        u.m.sim_ssc_center = true;
        u.m.sim_ssc_deviation = 0.01;
        u.sim_play_set(true).unwrap();
        let altered = u.refs[0].as_ref().unwrap().v.clone();
        u.sim_play_set(false).unwrap();
        u.activate(Item::SimDefaults);
        assert_eq!(u.m.sim_dcd_pp, 0.0);
        assert_eq!(u.m.sim_corr_rj, 0.0);
        assert!(!u.m.sim_ssc_center);
        assert_eq!(u.m.sim_ssc_deviation, 0.005);
        assert_ne!(u.refs[0].as_ref().unwrap().v, altered);
        assert_eq!(u.m.run, RunState::Stop);
        assert!(!u.sim_playing());
    }

    #[test]
    fn asymmetric_sim_rails_are_centered_without_changing_signal_volts() {
        let mut u = ui();
        u.m.sim_ssc = false;
        u.m.sim_vlow = 1.0;
        u.m.sim_vhigh = 2.0;
        u.m.sim_noise = 0.0;
        u.m.sim_deemph = 0.0;
        u.sim_play_set(true).unwrap();
        let reference = u.refs[0].as_ref().unwrap();
        assert_eq!(reference.offset, -1.5);
        let (low, high) = reference.v.iter().copied().fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), value| (low.min(value), high.max(value)));
        assert!(low > 0.9 && high > 1.9 && high < 2.1, "samples retain their actual voltage: {low}..{high}");
        assert!((low + reference.offset).abs() < 4.0 * reference.vpd);
        assert!((high + reference.offset).abs() < 4.0 * reference.vpd);
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/sim-asymmetric-rails.png"), crate::store::png_of(&cv));
        }
    }

    #[test]
    fn generation_error_preserves_the_working_waveform_and_session() {
        let mut u = ui();
        u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
        let original = u.refs[0].as_ref().unwrap().v.clone();
        let state = u.workflow_state();
        u.m.sim_rate = 32e9;
        u.m.sim_ssc = true;
        u.m.sim_ssc_hz = 1e3;
        let error = u.sim_play_set(true).unwrap_err();
        assert!(error.contains("sample") || error.contains("record"), "{error}");
        assert_eq!(u.refs[0].as_ref().unwrap().v, original);
        assert_eq!(u.workflow_state(), state);
        assert!(!u.session_is_sim && !u.sim_generating());
        u.activate(Item::SimPlay);
        assert!(u.toast.as_ref().unwrap().0.contains(&error));
        assert_eq!(u.refs[0].as_ref().unwrap().v, original);
    }

    #[test]
    fn asynchronous_generation_error_preserves_sources_and_is_reported_once() {
        let mut u = ui();
        u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
        let original = u.refs[0].as_ref().unwrap().v.clone();
        u.m.sim_ssc_hz = 1e6;
        u.m.sim_rj = 50e-12; // seeded random edges cross at this amplitude
        let state = u.workflow_state();
        u.sim_play_set(true).unwrap();
        assert!(u.sim_generating());
        u.wait_sim_generation();
        assert_eq!(u.refs[0].as_ref().unwrap().v, original);
        assert!(!u.session_is_sim);
        assert_eq!(u.workflow_state(), state);
        let error = u.take_sim_error().expect("the asynchronous error reaches SCPI");
        assert!(error.contains("jitter"), "{error}");
        assert!(u.take_sim_error().is_none());
        assert!(u.toast.as_ref().unwrap().0.contains(&error));
    }

    #[test]
    fn live_skips_unrequested_refs_but_keeps_hidden_measurements_and_remote_facts() {
        let mut u = ui();
        u.m.sim_ssc = false;
        u.sim_play_set(true).unwrap();
        u.return_to_live();
        assert!(!u.refs[0].as_ref().unwrap().on);
        u.measure_refs();
        assert!(u.ref_ana[0].get(Meas::Vpp).is_nan(), "an unused hidden ref has no cached analysis");
        assert!(!u.ref_ana_valid[0]);
        assert!(u.meas_at(WaveId::Ref(0), Meas::Vpp) > 0.5, "direct UI measurement also analyzes on demand");
        assert!(!u.ref_ana_valid[0], "an on-demand read must not enable repeated idle rescans");
        assert!(u.fact_meas(Meas::Vpp, WaveId::Ref(0)) > 0.5, "explicit SCPI facts still analyze a hidden ref");
        u.m.meas.push((Meas::Vpp, WaveId::Ref(0)));
        u.measure_refs();
        assert!(u.ref_ana[0].get(Meas::Vpp) > 0.5, "a hidden measured source remains current");
        u.m.meas.clear();
        u.m.meas2.push((0, WaveId::Ref(0), WaveId::Ch(0)));
        u.measure_refs();
        assert!(u.ref_ana[0].get(Meas::Vpp) > 0.5, "pairs retain their hidden reference source");
        u.m.meas2.clear();
        u.set_offline(true);
        u.m.dvm_on = true;
        u.measure_refs();
        assert!(u.ref_ana[0].get(Meas::Vpp) > 0.5, "the offline meter retains its source");
    }

    #[test]
    fn cancelled_ssc_generation_cannot_replace_live_or_an_imported_record() {
        for import in [false, true] {
            let mut u = ui();
            let live = (u.m.n_on(), u.m.run, u.record_mode());
            u.m.sim_ssc_hz = 1e6; // complete SSC period, small enough for a fast worker test
            u.sim_play_set(true).unwrap();
            assert!(u.sim_generating());
            assert!(!u.session_is_sim, "starting generation must not replace the live session");
            if import {
                u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
                assert_eq!((u.m.n_on(), u.m.run, u.record_mode()), live, "cancelling pending SIM must preserve additive Live import");
            } else {
                u.return_to_live();
            }
            assert!(!u.sim_generating());
            let state = u.workflow_state();
            u.wait_sim_worker();
            assert_eq!(u.workflow_state(), state, "a stale background reply must not start SIM");
            assert!(!u.session_is_sim);
            if import { assert_eq!(u.refs[0].as_ref().unwrap().label, "keep.csv"); }
        }
    }

    #[test]
    fn newest_ssc_request_wins_and_stop_cancels_pending_work() {
        let mut u = ui();
        u.m.sim_ssc_hz = 1e6;
        u.sim_play_set(true).unwrap();
        u.m.sim_slot = 1;
        u.m.sim_prbs = 13;
        u.sim_load(false).unwrap();
        u.wait_sim_generation();
        assert!(u.session_is_sim);
        assert!(u.refs[0].is_none(), "superseded request must not populate its old slot");
        assert!(u.refs[1].is_some());
        assert_eq!(u.m.run, RunState::Stop);
        u.sim_load(true).unwrap();
        assert!(u.sim_generating());
        u.key(key::SINGLE); // arm the existing reference and cancel a pending replacement
        assert!(!u.sim_generating());
        u.key(key::RUNSTOP);
        let state = u.workflow_state();
        u.wait_sim_worker();
        assert_eq!(u.workflow_state(), state);
        assert_eq!(u.m.run, RunState::Stop);
    }

    #[test]
    fn entering_sim_preserves_configured_measurements_pairs_and_settings() {
        let mut u = ui();
        u.m.sim_ssc = false;
        u.m.sim_slot = 4;
        u.m.meas = ALL_MEAS.iter().map(|&kind| (kind, WaveId::Ref(1))).collect();
        u.m.meas2 = (0..super::tools::PAIRS.len()).map(|kind| (kind, WaveId::Ref(1), WaveId::Ref(2))).collect();
        u.m.meas_full_record = true;
        u.m.meas_labels = true;
        let singles = u.m.meas.clone();
        let pairs = u.m.meas2.clone();
        u.sim_load(false).unwrap();
        u.wait_sim_generation();
        assert!(u.m.meas.starts_with(&singles));
        assert_eq!(u.m.meas.len(), singles.len() + SIM_MEAS.len());
        assert_eq!(u.m.meas2, pairs);
        assert!(u.m.meas_full_record && u.m.meas_labels);
        assert_eq!(u.m.run, RunState::Stop);
        let configured = u.m.meas.clone();
        u.sim_load(false).unwrap();
        u.wait_sim_generation();
        assert_eq!(u.m.meas, configured, "regenerating does not duplicate SIM readouts");
    }

    #[test]
    fn sim_play_loads_the_preset_into_the_slot() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
        u.m.apply_sim_preset();
        u.m.sim_slot = 2;
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        let r = u.refs[2].as_ref().expect("R3");
        assert_eq!(r.label, "PCIe 4.0");
        assert!(r.on);
        assert_eq!(r.v.len(), serial::sample_count(&u.sim_lane()).unwrap());
        assert!((r.dt - 1.0 / (8.0 * 16e9)).abs() < 1e-18);
        assert_eq!(u.m.eye_src, WaveId::Ref(2));
        assert!((u.m.eye_rate - 16e9).abs() < 1.0);
        assert!(!u.m.ch.iter().any(|c| c.on), "SIM is a reference, not the analog channels");
        assert!(u.last[0].is_none());
        assert_eq!(u.meas_src, WaveId::Ref(2));
        assert!(u.m.zoom);
        // The overview fits the complete SSC period; zoom retains the bit scale.
        let record_span = r.v.len() as f64 * r.dt;
        let expected_scale = tscales().iter().copied().find(|&scale| scale as f64 * HDIV as f64 >= record_span).unwrap();
        assert_eq!(u.m.time_per_div(), expected_scale);
        assert!((tscales()[u.m.zscale] as f64 - 50e-12).abs() < 1e-12, "zoom {}", tscales()[u.m.zscale]);
        assert!((u.m.delay as f64 - record_span * 0.5).abs() < record_span * 1e-6);
        assert!(u.m.meas.iter().all(|(_, id)| *id == WaveId::Ref(2)));
        assert_eq!(u.m.meas.len(), 12);
        u.render(&mut Canvas::new(W, H));
        let names = serial::names().join(" ");
        assert!(!names.to_ascii_lowercase().contains("hss"));
        assert!(!names.contains("Waveform"));
        let labels: String = u.sim_items().iter().filter_map(|&it| Ui::sim_label(it)).collect::<Vec<_>>().join(" ");
        assert!(!labels.to_ascii_lowercase().contains("hss"));
    }

    #[test]
    fn sim_usb32_gen2_uses_the_record_timebase() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "USB 3.2 Gen 2").unwrap();
        u.m.apply_sim_preset();
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        let r = u.refs[0].as_ref().expect("R1");
        assert_eq!(r.label, "USB 3.2 Gen 2");
        assert!((r.dt - 1.0 / (8.0 * 10e9)).abs() < 1e-18);
        assert!((u.m.time_per_div() as f64 - 100e-9).abs() < 1e-12, "overview tdiv {}", u.m.time_per_div());
        assert!((tscales()[u.m.zscale] as f64 - 100e-12).abs() < 1e-12, "zoom {}", tscales()[u.m.zscale]);
        let (sr, n) = u.sim_rate_depth().expect("rate");
        assert!((sr - 80e9).abs() < 1.0, "sr {}", sr);
        assert_eq!(n, 65_536);
        let rows = u.meas_rows();
        assert_eq!(rows.len(), 12);
        for want in ["Pk-pk", "Freq", "Period", "Rise", "Fall", "+Duty", "Amp", "Overshoot", "+Width", "-Width", "+Slew", "+Edges"] {
            let row = rows.iter().find(|r| r.name == want).unwrap_or_else(|| panic!("missing {want}"));
            assert!(!row.v.is_infinite(), "{want} {}", row.v);
        }
        let vpp = rows.iter().find(|r| r.name == "Pk-pk").unwrap().v;
        assert!(vpp > 0.5 && vpp < 1.2, "vpp {}", vpp);
        assert!(!u.m.meas_labels, "labels are opt-in so initial SIM does not hide the waveform");
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
            let png = crate::store::png_of(&cv);
            let _ = std::fs::write(format!("{}/sim-usb32-gen2-loaded.png", dir), &png);
            let _ = std::fs::write(format!("{}/sim-usb32-measurements.png", dir), &png);
        }
    }

    #[test]
    fn sim_knobs_change_the_record() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "USB 3.2 Gen 2").unwrap();
        u.m.apply_sim_preset();
        u.m.sim_rate = 5e9;
        u.m.sim_prbs = 7;
        u.m.sim_deemph = 3.5;
        u.m.sim_ssc = true;
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        let r = u.refs[0].as_ref().unwrap();
        assert!((r.dt - 1.0 / (8.0 * 5e9)).abs() < 1e-18);
        u.activate(Item::SimDefaults);
        assert!((u.m.sim_rate - 10e9).abs() < 1.0);
        assert_eq!(u.m.sim_prbs, 15);
        assert!(!u.m.sim_ssc);
        u.open_menu(MenuId::Sim);
        let labels: Vec<_> = u.sim_items().iter().filter_map(|&it| Ui::sim_label(it)).collect();
        for want in ["Rate", "Pattern", "High", "Low", "Rise 20-80%", "Bandwidth", "De-emph", "Noise", "Spread", "RJ RMS", "PJ peak"] {
            assert!(labels.contains(&want), "missing {want} in {labels:?}");
        }
        u.render(&mut Canvas::new(W, H));
        if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
            let mut cv = Canvas::new(W, H);
            u.render(&mut cv);
            let _ = std::fs::write(format!("{}/sim-menu-knobs.png", dir), crate::store::png_of(&cv));
        }
    }

    #[test]
    fn sim_play_does_not_arm_the_adc() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
        u.m.apply_sim_preset();
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        assert!(u.last[0].is_none());
        u.m.run = RunState::Run;
        u.step(1.0 / 60.0);
        assert!(u.last[0].is_none(), "Run with analog off must not acquire");
        assert!(!u.m.ch.iter().any(|c| c.on));
        u.return_to_live();
        u.m.tscale = 0;
        u.m.ch[0].on = true;
        u.m.floor_live_tdiv();
        assert!((u.m.time_per_div() as f64 - 1e-9).abs() < 1e-12);
        u.step(1.0 / 60.0);
        assert!(u.last[0].is_some(), "Run with analog on acquires");
    }

    #[test]
    fn sim_show_toggles_the_slot() {
        let mut u = ui();
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        assert!(u.refs[0].as_ref().unwrap().on);
        u.adjust(Item::SimShow, 1);
        assert!(!u.refs[0].as_ref().unwrap().on);
    }

    fn set_tdiv(u: &mut Ui, tdiv: f32) {
        let ts = tscales();
        u.m.tscale = ts.iter().position(|&t| (t - tdiv).abs() <= tdiv * 1e-4).expect("timebase");
        u.m.tfine = 1.0;
    }

    /// A step in the record, low then high, so the edge on screen has a place to move.
    fn put_step(u: &mut Ui, n: usize, dt: f64, tdiv: f32) {
        let v: Vec<f32> = (0..n).map(|i| if i < n / 5 { -0.5 } else { 0.5 }).collect();
        u.refs[0] = Some(RefWave { src: 0, color: 0, label: "rec".into(), t0: 0.0, dt, v, vpd: 0.2, offset: 0.0, on: true });
        u.m.sim_slot = 0;
        u.m.ch[0].on = false;
        u.m.run = RunState::Run;
        u.m.trig_mode = TrigMode::Normal;
        u.m.trig_level = 0.0;
        u.m.delay = 0.0;
        set_tdiv(u, tdiv);
        u.sim_play = true;
    }

    fn window_of(u: &Ui) -> f64 {
        u.view().0 as f64 * HDIV as f64
    }

    #[test]
    fn replay_locks_the_view_to_the_trigger_condition() {
        let mut u = ui();
        put_step(&mut u, 5_000, 1e-9, 100e-9);
        u.step(0.25);
        assert_eq!(u.m.status, TrigStatus::Triggered);
        assert_eq!(u.replay.captures, 1);
        assert!((u.m.delay as f64 - 999.5e-9).abs() < 1e-9);
        let position = u.m.delay;
        u.step(0.25);
        assert_eq!(u.m.delay, position, "a repeated trigger holds the edge in place");
        assert_eq!(u.replay.captures, 2);
        assert!(u.ph.map.iter().all(|m| m.iter().all(|&p| p == 0)));
    }

    #[test]
    fn normal_replay_waits_and_force_captures_without_a_match() {
        let mut u = ui();
        put_step(&mut u, 5_000, 1e-9, 100e-9);
        u.m.trig_level = 10.0;
        u.step(0.25);
        assert_eq!(u.m.status, TrigStatus::Wait);
        assert_eq!(u.replay.captures, 0);
        u.key(key::FORCE);
        u.step(0.25);
        assert_eq!(u.replay.captures, 1);
        assert_eq!(u.m.status, TrigStatus::Auto);
    }

    #[test]
    fn play_holds_a_record_that_fits() {
        let mut u = ui();
        put_step(&mut u, 200, 1e-9, 100e-9);
        let r = u.refs[0].as_ref().unwrap();
        let end = (r.v.len() - 1) as f64 * r.dt;
        let window = window_of(&u);
        assert!(end <= window);
        u.step(0.0);
        let delay = u.m.delay;
        let left = delay as f64 - window * 0.5;
        assert!(left <= 0.0 && left + window >= end);
        u.key(key::RUNSTOP);
        let first = picture(&mut u);
        u.step(1.0);
        assert_eq!(u.m.delay, delay);
        let second = picture(&mut u);
        assert_eq!(first.px, second.px);
    }

    #[test]
    fn main_loop_sim_changes_samples_measurements_and_eye_only_on_accepted_captures() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 2.0 HS").unwrap();
        u.m.apply_sim_preset();
        u.m.sim_ssc = false;
        u.m.meas_full_record = true;
        u.sim_play_set(true).unwrap();
        u.wait_sim_generation();
        u.eye_make();
        let records = |u: &Ui| u.eye_readings().iter().find(|(name, _)| *name == "RECords").unwrap().1;
        let before = u.refs[0].as_ref().unwrap().v.clone();
        let value = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        let captures = u.replay.captures;
        let eye_records = records(&u);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while u.replay.captures < captures + 2 && std::time::Instant::now() < deadline {
            u.step(0.01);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(u.replay.captures >= captures + 2);
        assert_ne!(u.refs[0].as_ref().unwrap().v, before, "capture counters alone do not prove fresh acquisition");
        assert_ne!(u.meas_at(WaveId::Ref(0), Meas::Vpp), value, "full-record SIM measurements must use the new noisy samples");
        assert!(records(&u) >= eye_records + 2.0, "Eye accumulates each accepted revision");
        u.key(key::RUNSTOP);
        let held = u.refs[0].as_ref().unwrap().v.clone();
        let held_count = u.replay.captures;
        let held_eye = records(&u);
        u.step(1.0);
        u.wait_sim_worker();
        u.step(0.1);
        assert_eq!(u.refs[0].as_ref().unwrap().v, held);
        assert_eq!(u.replay.captures, held_count);
        assert_eq!(records(&u), held_eye);
        u.key(key::SINGLE);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while u.m.run != RunState::Stop && std::time::Instant::now() < deadline {
            u.step(0.01);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(u.m.run, RunState::Stop);
        assert_eq!(u.replay.captures, held_count + 1);
        assert_ne!(u.refs[0].as_ref().unwrap().v, held, "Single also needs a fresh generated record");
        assert_eq!(records(&u), held_eye + 1.0);
    }

    fn drive_until(u: &mut Ui, done: impl Fn(&Ui) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !done(u) && std::time::Instant::now() < deadline {
            u.step(0.01);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(done(u), "SIM main loop did not reach the expected capture/analysis state");
    }

    #[test]
    fn actual_default_ssc_accepts_fresh_large_captures_and_honors_stop_single_wait_force() {
        let mut u = ui();
        assert_eq!(u.sim_lane().name, "PCIe 4.0");
        assert!(u.sim_lane().ssc);
        let count = serial::sample_count(&u.sim_lane()).unwrap();
        assert!(count > 3_000_000, "the actual default SSC record must exercise asynchronous triggering");
        u.sim_play_set(true).unwrap();
        drive_until(&mut u, |u| u.replay.captures >= 1);
        assert_eq!(u.refs[0].as_ref().unwrap().v.len(), count);
        assert!(!u.reference_search_busy(), "initial acceptance uses worker-computed hits");
        let first = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
        let first_value = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        drive_until(&mut u, |u| u.replay.captures >= 2);
        assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], first);
        assert_ne!(u.meas_at(WaveId::Ref(0), Meas::Vpp), first_value);
        u.key(key::RUNSTOP);
        let held = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
        let held_count = u.replay.captures;
        u.wait_sim_worker();
        u.step(0.2);
        assert_eq!(u.replay.captures, held_count);
        assert_eq!(u.refs[0].as_ref().unwrap().v[..8192], held);
        u.key(key::SINGLE);
        drive_until(&mut u, |u| u.m.run == RunState::Stop);
        assert_eq!(u.replay.captures, held_count + 1);
        assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], held);
        let waiting = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
        let before = u.replay.captures;
        u.m.trig_level = 10.0;
        u.key(key::SINGLE);
        let attempt = u.replay.sim_attempt;
        drive_until(&mut u, |u| u.replay.sim_attempt >= attempt + 2);
        assert_eq!(u.m.status, TrigStatus::Wait);
        assert_eq!(u.replay.captures, before);
        assert_eq!(u.refs[0].as_ref().unwrap().v[..8192], waiting, "no-hit candidates never become the visible accepted record");
        u.key(key::FORCE);
        drive_until(&mut u, |u| u.m.run == RunState::Stop);
        assert_eq!(u.replay.captures, before + 1);
        assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], waiting);
        println!("default SSC: {count} samples; fresh captures, Stop, Single, WAIT and Force passed");
    }

    #[test]
    fn large_full_record_sim_backpressure_finishes_values_and_counts_statistics_once_per_capture() {
        let mut u = ui();
        assert!(serial::sample_count(&u.sim_lane()).unwrap() > 3_000_000);
        u.m.meas_full_record = true;
        u.sim_play_set(true).unwrap();
        drive_until(&mut u, |u| u.replay.captures >= 1);
        drive_until(&mut u, |u| u.mstat.first().is_some_and(|stat| stat.n >= 1));
        assert!(u.meas_at(WaveId::Ref(0), Meas::Vpp).is_finite());
        assert_eq!(u.m.run, RunState::Run, "Full record must become ready without stopping");
        let first = u.refs[0].as_ref().unwrap().v[..8192].to_vec();
        let value = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        drive_until(&mut u, |u| u.mstat.first().is_some_and(|stat| stat.n >= 2));
        assert_eq!(u.mstat[0].n, u.replay.captures);
        assert_ne!(u.refs[0].as_ref().unwrap().v[..8192], first);
        assert_ne!(u.meas_at(WaveId::Ref(0), Meas::Vpp), value);
        u.key(key::RUNSTOP);
        let n = u.mstat[0].n;
        for _ in 0..10 { u.poll_reference_measurements(); u.finish_deferred_capture_statistics(); let _ = u.meas_rows(); }
        assert_eq!(u.mstat[0].n, n, "worker polling and reads cannot count an accepted revision twice");
        u.wait_sim_worker();
    }

    #[test]
    fn generated_hits_reject_changed_trigger_source_threshold_hysteresis_and_secondary_record() {
        for change in 0..7 {
            let mut u = ui();
            u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 2.0 HS").unwrap();
            u.m.apply_sim_preset();
            u.sim_play_set(true).unwrap();
            u.refs[1] = Some(RefWave { src: 0, color: 1, label: "External".into(), t0: 0.0, dt: 2e-9,
                v: (0..1024).map(|i| if i % 16 < 8 { -0.5 } else { 0.5 }).collect(), vpd: 0.2, offset: 0.0, on: false });
            if change == 5 {
                u.m.trig_type = TrigType::Delay;
                u.m.set_trigger_wave(WaveId::Ref(1), true);
            } else if change == 6 {
                u.m.trig_type = TrigType::Bus;
                u.m.dec_proto = Proto::Uart;
                u.m.trig_bus = BusWhat::Data;
                u.m.trig_bus_val = 0x55;
            }
            let mut lane = u.sim_lane();
            lane.seed ^= 19;
            let request = u.sim_capture_request(lane, 0, true, true, true);
            let generated = Generated::evaluate(&request, serial::try_generate(&lane).unwrap());
            let pointer = u.refs[0].as_ref().unwrap().v.as_ptr();
            let captures = u.replay.captures;
            match change {
                0 => u.m.trig_level = 10.0,
                1 => u.m.trig_type = TrigType::Pulse,
                2 => u.m.set_trigger_wave(WaveId::Ref(1), false),
                3 => u.refs[0].as_mut().unwrap().vpd *= 2.0,
                4 => u.m.trig_nrej = !u.m.trig_nrej,
                5 => u.refs[1].as_mut().unwrap().v = vec![0.0; 1024],
                _ => u.m.trig_bus_val = 0xaa,
            }
            u.sim_finish(request, generated);
            assert_eq!(u.refs[0].as_ref().unwrap().v.as_ptr(), pointer);
            assert_eq!(u.replay.captures, captures, "change {change}: stale worker hits cannot validate a new condition");
        }
    }

    #[test]
    fn external_reference_scan_holds_one_candidate_without_exposing_it_and_keeps_its_own_timebase() {
        for source in [1, 2] {
            let mut u = ui();
            u.m.sim_preset = LANES.iter().position(|lane| lane.name == "USB 2.0 HS").unwrap();
            u.m.apply_sim_preset();
            u.sim_play_set(true).unwrap();
            u.refs[source] = Some(RefWave { src: 0, color: source, label: "Long external trigger".into(), t0: 20e-9, dt: 10e-12,
                v: (0..263_168).map(|i| if i % 16 < 8 { -0.5 } else { 0.5 }).collect(), vpd: 0.2, offset: 0.0, on: false });
            assert!(u.set_trigger_source(WaveId::Ref(source), false));
            u.m.trig_level = 0.0;
            u.m.trig_mode = TrigMode::Single;
            let captures = u.replay.captures;
            let pointer = u.refs[0].as_ref().unwrap().v.as_ptr();
            let mut lane = u.sim_lane();
            lane.seed ^= 23;
            let request = u.sim_capture_request(lane, 0, true, true, true);
            assert!(request.trigger.is_none(), "independent external triggers use their existing shared cache");
            let generated = Generated::evaluate(&request, serial::try_generate(&lane).unwrap());
            u.sim_finish(request, generated);
            assert_eq!(u.refs[0].as_ref().unwrap().v.as_ptr(), pointer);
            assert_eq!(u.replay.captures, captures);
            assert!(u.sim_generation.as_ref().unwrap().pending.is_some());
            u.wait_sim_generation();
            assert_eq!(u.replay.captures, captures + 1);
            assert_eq!(u.m.run, RunState::Stop);
            assert!((u.replay.trigger_time.unwrap() - (20e-9 + 7.5 * 10e-12)).abs() < 1e-15);
            let hits = u.reference_trigger_hits(source).unwrap();
            u.replay.sim_attempt += 1;
            assert!(Arc::ptr_eq(&hits, &u.reference_trigger_hits(source).unwrap()), "unrelated SIM attempts must not invalidate the external hit cache");
        }
    }

    #[test]
    fn loaded_sim_uses_run_stop_and_returns_to_live_explicitly() {
        let mut u = ui();
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        let before = u.replay.captures;
        u.step(0.5);
        // A fresh capture is generated off-thread. Advancing simulated time is
        // not evidence that the worker has run, particularly in the full suite.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while u.replay.captures == before && std::time::Instant::now() < deadline {
            u.step(0.01);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(u.replay.captures > before && u.sim_play);
        assert!(u.last[0].is_none());
        u.key(key::RUNSTOP);
        let held = u.replay.captures;
        u.step(0.5);
        assert_eq!(u.replay.captures, held);
        u.return_to_live();
        u.step(0.02);
        assert!(!u.sim_play && u.last[0].is_some());
    }

    fn picture(u: &mut Ui) -> crate::gfx::Canvas {
        let mut cv = crate::gfx::Canvas::new(W, H);
        u.render(&mut cv);
        cv
    }

    /// Left edge of the level that starts furthest right: the step in the record.
    fn high_edge(cv: &crate::gfx::Canvas) -> i32 {
        let col = super::super::REF_COLORS[0].1;
        let mut best = -1;
        for y in 0..cv.h {
            let mut n = 0i32;
            let mut minx = cv.w;
            for x in 0..cv.w {
                if cv.px[(y * cv.w + x) as usize] == col {
                    n += 1;
                    if x < minx {
                        minx = x;
                    }
                }
            }
            if n > 80 && minx > best {
                best = minx;
            }
        }
        assert!(best >= 0, "no reference trace");
        best
    }
}
