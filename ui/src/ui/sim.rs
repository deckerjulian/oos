// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM: load a nominal serial record into a reference slot.
//! Play moves the timebase window through a record that is longer than the window.
//! A record that already fits stays one picture. Live acquisition is unchanged.

use super::*;
use oos_dsp::serial::{self, Lane, LANES};
use std::sync::mpsc::{self, Receiver, Sender};

const PRBS: [u8; 6] = [7, 9, 13, 15, 23, 31];

struct Request {
    id: u64,
    lane: Lane,
    slot: usize,
    continuing: bool,
    run: bool,
}

/// One worker per UI. Knob changes replace queued requests, never spawn a
/// parallel set of multi-million-sample generators. Only the latest reply wins.
pub(super) struct Generation {
    requests: Sender<Request>,
    results: Receiver<(Request, Result<Vec<f32>, String>)>,
    next: u64,
    active: Option<u64>,
    run: bool,
    #[cfg(test)]
    completed: u64,
}

impl Generation {
    fn new() -> Result<Self, String> {
        let (requests, input) = mpsc::channel::<Request>();
        let (output, results) = mpsc::channel();
        std::thread::Builder::new().name("oos-sim".into()).spawn(move || {
            while let Ok(mut request) = input.recv() {
                while let Ok(newer) = input.try_recv() { request = newer; }
                let generated = serial::try_generate(&request.lane);
                if output.send((request, generated)).is_err() { break; }
            }
        }).map_err(|error| format!("Could not start SIM generator: {error}"))?;
        Ok(Self { requests, results, next: 0, active: None, run: false, #[cfg(test)] completed: 0 })
    }

    fn submit(&mut self, mut request: Request) -> Result<(), String> {
        self.next = self.next.wrapping_add(1);
        request.id = self.next;
        self.active = Some(request.id);
        self.run = request.run;
        self.requests.send(request).map_err(|_| {
            self.active = None;
            "SIM generator stopped; return to Live and try again".into()
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

    pub(super) fn sim_items(&self) -> Vec<Item> {
        vec![
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
        ]
    }

    pub(super) fn sim_advanced_items(&self) -> Vec<Item> {
        vec![Item::SimSsc, Item::SimSscCenter, Item::SimSscDeviation, Item::SimSscHz,
             Item::SimDcd, Item::SimCorrRj, Item::SimCorrHz, Item::SimDefaults]
    }

    pub(super) fn sim_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SimPlay | Item::SimDefaults => Kind::Action,
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

    pub(crate) fn sim_cancel_generation(&mut self) {
        if let Some(generation) = &mut self.sim_generation { generation.active = None; }
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
        self.sim_cancel_generation();
        let lane = self.sim_lane();
        let count = serial::sample_count(&lane)?;
        let request = Request { id: 0, lane, slot: self.m.sim_slot, continuing: self.session_is_sim, run };
        if count <= 65_536 {
            let generated = serial::try_generate(&lane)?;
            self.sim_cancel_generation();
            self.sim_finish(request, generated);
        } else {
            if self.sim_generation.is_none() { self.sim_generation = Some(Generation::new()?); }
            self.sim_generation.as_mut().unwrap().submit(request)?;
            self.toast(&format!("Generating {} ({:.1} M samples)", lane.name, count as f64 / 1e6));
        }
        Ok(())
    }

    pub(super) fn sim_poll(&mut self) {
        loop {
            let Some(generation) = &mut self.sim_generation else { return };
            match generation.results.try_recv() {
                Ok((request, result)) => {
                    #[cfg(test)] { generation.completed = request.id; }
                    if generation.active != Some(request.id) { continue; }
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
        while self.sim_generation.as_ref().is_some_and(|generation| generation.completed < generation.next) {
            self.sim_poll();
            assert!(std::time::Instant::now() < deadline, "SIM worker did not finish");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Commit one successful result. Overview is the stored record; zoom is ~eight UI.
    fn sim_finish(&mut self, request: Request, v: Vec<f32>) {
        let Request { lane, slot, continuing, run, .. } = request;
        self.remember_live();
        self.session_is_sim = true;
        self.replay = Default::default();
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
        self.set_zoom(true);
        self.m.zscale = bit;
        self.m.zdelay = look;
        self.clamp_zoom();
        if !continuing {
            self.m.meas = Self::sim_meas(slot);
            self.m.meas2.clear();
            self.m.meas_labels = false;
        }
        self.meas_src = WaveId::Ref(slot);
        self.m.eye_src = WaveId::Ref(slot);
        self.m.eye_rate = lane.bits_per_s;
        self.m.run = if run { RunState::Run } else { RunState::Stop };
        self.m.status = if run { TrigStatus::Wait } else { TrigStatus::Stop };
        if !continuing { self.m.trig_level = ((lane.v_high + lane.v_low) * 0.5) as f32; }
        self.last[0] = None;
        self.cleared = true;
        self.sim_play = run;
        if run { self.sim_tick(0.0); }
        else { self.refresh_stopped_reference(); }
        self.ph.clear();
        self.measure_refs();
        self.analyzer_update();
        self.toast(&format!("{} → R{}", lane.name, slot + 1));
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

    /// On writes the selected preset into the selected slot, the same as Play. Off stops the walk.
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

    /// Acquires the next reference window using the shared trigger and Run/Stop state.
    pub(super) fn sim_tick(&mut self, dt_wall: f64) {
        self.replay_acquire(dt_wall);
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
            u.m.sim_ssc_hz = 1e6; // complete SSC period, small enough for a fast worker test
            u.sim_play_set(true).unwrap();
            assert!(u.sim_generating());
            assert!(!u.session_is_sim, "starting generation must not replace the live session");
            if import {
                u.load_waveform_bytes(b"Time (s),Volts\n0,-1\n1e-9,1\n2e-9,-1\n", "keep.csv").unwrap();
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
    fn loaded_sim_uses_run_stop_and_returns_to_live_explicitly() {
        let mut u = ui();
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        let before = u.replay.captures;
        u.step(0.5);
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
