// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Direct file, reference, simulation and acquisition controls shared by the screen and web remote.
use super::*;

pub(super) struct LiveSession {
    model: Model,
    visible: [bool; NREF],
}

#[cfg(test)]
mod import_measurement_tests {
    use super::*;

    const CSV: &[u8] = b"Time (s),Volts\n0,-1\n0.000000001,1\n0.000000002,-1\n0.000000003,1\n";

    #[test]
    fn additive_imports_keep_all_configured_single_pair_and_gate_settings() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.load_waveform_auto(CSV, "first.csv").unwrap();
        ui.m.meas = ALL_MEAS.iter().map(|&kind| (kind, WaveId::Ref(0))).collect();
        ui.m.meas2 = (0..PAIRS_N).map(|kind| (kind, WaveId::Ref(0), WaveId::Ref(1))).collect();
        ui.m.meas_full_record = true;
        ui.m.meas_labels = true;
        ui.m.meas_lpct = 0.2;
        ui.m.meas_hpct = 0.8;
        let expected = (ui.m.meas.clone(), ui.m.meas2.clone());
        ui.load_waveform_auto(CSV, "second.csv").unwrap();
        ui.load_waveform_auto(CSV, "third.csv").unwrap();
        assert!(ui.refs[0].is_some() && ui.refs[1].is_some() && ui.refs[2].is_some());
        assert_eq!((ui.m.meas.clone(), ui.m.meas2.clone()), expected);
        assert!(ui.m.meas_full_record && ui.m.meas_labels);
        assert_eq!((ui.m.meas_lpct, ui.m.meas_hpct), (0.2, 0.8));
        assert_eq!(ui.meas_src, WaveId::Ref(2), "new source is selected without replacing existing readouts");
    }

    #[test]
    fn a_first_import_respects_a_deliberately_empty_or_custom_catalog() {
        for custom in [Vec::new(), vec![(Meas::Vmax, WaveId::Ch(0))]] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.meas = custom.clone();
            ui.load_waveform_auto(CSV, "custom.csv").unwrap();
            assert_eq!(ui.m.meas, custom);
        }
        let mut untouched = Ui::new(Source::Demo { t: 0.0 });
        untouched.load_waveform_auto(CSV, "default.csv").unwrap();
        assert!(untouched.m.meas.iter().all(|entry| entry.1 == WaveId::Ref(0)), "fresh defaults follow the first imported source");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.remember_live();
        u.offline = true;
        u.offline_ref = 0;
        u.m.trig_ref = Some(0);
        for ch in &mut u.m.ch { ch.on = false; }
        u.m.la.on = false;
        u.m.sim_slot = 0;
        u.m.trig_level = 0.0;
        u.m.trig_mode = TrigMode::Normal;
        u.m.tscale = tscales().iter().position(|&x| (x - 10e-9).abs() < 1e-14).unwrap();
        let values = (0..5000).map(|i| {
            let amplitude = if i < 2500 { 0.5 } else { 1.0 };
            if i % 40 < 20 { -amplitude } else { amplitude }
        }).collect();
        u.refs[0] = Some(RefWave { src: 0, color: 0, label: "Test record".into(), t0: 0.0, dt: 1e-9, v: values, vpd: 0.5, offset: 0.0, on: true });
        u.meas_src = WaveId::Ref(0);
        u.m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
        u.m.run = RunState::Stop;
        u
    }

    #[test]
    fn single_arms_waits_for_the_condition_and_stops_after_one_capture() {
        let mut u = replay();
        u.m.trig_level = 10.0;
        u.key(key::SINGLE);
        u.step(0.2);
        assert_eq!(u.m.status, TrigStatus::Wait);
        assert_eq!(u.replay.captures, 0);
        assert_eq!(u.m.run, RunState::Run);
        u.m.trig_level = 0.0;
        u.step(0.2);
        assert_eq!(u.replay.captures, 1);
        assert_eq!(u.m.status, TrigStatus::Triggered);
        assert_eq!(u.m.run, RunState::Stop);
        assert!(!u.sim_play && u.last[0].is_none());
        u.step(0.5);
        assert_eq!(u.replay.captures, 1);
    }

    #[test]
    fn run_stop_and_capture_measurements_share_one_state() {
        let mut u = replay();
        u.key(key::RUNSTOP);
        u.step(0.2);
        let first = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        let count = u.replay.captures;
        u.replay.cursor = 3000.0;
        u.step(0.2);
        let second = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        assert!((first - 1.0).abs() < 0.01 && (second - 2.0).abs() < 0.01, "{first} {second}");
        assert!(u.replay.captures > count);
        assert_eq!(u.mstat[0].n, 2);
        u.key(key::RUNSTOP);
        let held = (u.replay.captures, u.view(), second);
        u.step(0.5);
        assert_eq!((u.replay.captures, u.view(), u.meas_at(WaveId::Ref(0), Meas::Vpp)), held);
    }

    #[test]
    fn panning_a_stopped_reference_recomputes_the_visible_measurement_window() {
        let mut u = replay();
        u.m.delay = 1e-6;
        u.step(0.01);
        let first = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        u.m.delay = 4e-6;
        u.step(0.01);
        let second = u.meas_at(WaveId::Ref(0), Meas::Vpp);
        assert!((first - 1.0).abs() < 0.01 && (second - 2.0).abs() < 0.01, "{first} {second}");
        assert_eq!(u.replay.captures, 0, "panning is analysis, not an acquisition");
    }

    #[test]
    fn every_reference_uses_the_same_time_window_at_its_own_sample_interval() {
        let mut u = replay();
        let values = (0..2500).map(|i| if i < 1250 { if i % 20 < 10 { -1.0 } else { 1.0 } } else if i % 20 < 10 { -3.0 } else { 3.0 }).collect();
        u.refs[1] = Some(RefWave { src: 0, color: 1, label: "Second source".into(), t0: 0.0, dt: 2e-9, v: values, vpd: 1.0, offset: 0.0, on: false });
        u.m.delay = 1e-6;
        u.step(0.01);
        assert!((u.meas_at(WaveId::Ref(1), Meas::Vpp) - 2.0).abs() < 0.01);
        u.m.delay = 4e-6;
        u.step(0.01);
        assert!((u.meas_at(WaveId::Ref(0), Meas::Vpp) - 2.0).abs() < 0.01);
        assert!((u.meas_at(WaveId::Ref(1), Meas::Vpp) - 6.0).abs() < 0.01);
        assert_eq!(u.reference_slice(1).unwrap().2, 2e-9);
    }

    #[test]
    fn reference_two_source_triggers_use_secondary_scale_and_rebuild_cached_hits() {
        for kind in [TrigType::Delay, TrigType::SetupHold] {
            for secondary in [WaveId::Ref(1), WaveId::Ch(1)] {
                let mut u = replay();
                u.m.trig_type = kind;
                u.m.trig_level = 0.0;
                u.m.trig_level2 = 0.0;
                u.m.slope = Slope::Rise;
                u.m.trig_slope2 = Slope::Rise;
                u.m.trig_when = WhenKind::Gt;
                u.m.trig_t1 = 0.0;
                u.m.setup = 20e-9;
                u.m.hold = 1e-9;
                u.m.set_trigger_wave(secondary, true);
                let values: Vec<f32> = (0..5000).map(|i| if i % 40 < 30 { -0.05 } else { 0.05 }).collect();
                match secondary {
                    WaveId::Ref(i) => u.refs[i] = Some(RefWave { src: 0, color: 1,
                        label: "Low-amplitude secondary".into(), t0: 0.0, dt: 1e-9,
                        v: values, vpd: 2.0, offset: 0.0, on: false }),
                    WaveId::Ch(c) => {
                        u.m.ch[c].probe = 1.0;
                        u.m.ch[c].vfine = 2.0 / VSCALES[u.m.ch[c].vscale];
                        let mut rec = Ui::blank_record();
                        rec.dt = 1e-9;
                        rec.ch[c] = values;
                        u.last[0] = Some(rec);
                    }
                }

                u.key(key::SINGLE);
                u.replay_acquire(0.2);
                assert_eq!((u.m.status, u.replay.captures, u.m.run),
                    (TrigStatus::Wait, 0, RunState::Run), "{kind:?} {secondary:?}: large secondary hysteresis should reject the small waveform");

                // Change only its scale. The source buffer, thresholds and type
                // are identical, so this also exercises cache invalidation.
                match secondary {
                    WaveId::Ref(i) => u.refs[i].as_mut().unwrap().vpd = 0.02,
                    WaveId::Ch(c) => u.m.ch[c].vfine = 0.02 / VSCALES[u.m.ch[c].vscale],
                }
                u.now += 0.2;
                u.replay_acquire(0.2);
                assert_eq!((u.m.status, u.replay.captures, u.m.run),
                    (TrigStatus::Triggered, 1, RunState::Stop), "{kind:?} {secondary:?}: secondary scale must control its own comparator");

                match secondary {
                    WaveId::Ref(i) => u.refs[i].as_mut().unwrap().vpd = 2.0,
                    WaveId::Ch(c) => u.m.ch[c].vfine = 2.0 / VSCALES[u.m.ch[c].vscale],
                }
                u.key(key::SINGLE);
                u.now += 0.2;
                u.replay_acquire(0.2);
                assert_eq!((u.m.status, u.replay.captures, u.m.run),
                    (TrigStatus::Wait, 1, RunState::Run), "{kind:?} {secondary:?}: cached hits must be removed when secondary hysteresis grows");
            }
        }
    }

    #[test]
    fn leaving_sim_restores_live_settings_and_keeps_the_saved_record() {
        let mut u = replay();
        u.return_to_live();
        assert!(!u.record_mode());
        assert!(u.m.ch[0].on);
        assert_eq!(u.m.run, RunState::Run);
        assert!(u.refs[0].is_some() && !u.refs[0].as_ref().unwrap().on);
        u.step(0.02);
        assert!(u.last[0].is_some());
    }

    #[test]
    fn invalid_file_does_not_replace_a_working_session() {
        let mut u = replay();
        let old = u.refs[0].as_ref().unwrap().v.clone();
        assert!(u.load_waveform_bytes(b"not a waveform", "bad.csv").is_err());
        assert_eq!(u.refs[0].as_ref().unwrap().v, old);
        let mut header = vec![0u8; 36];
        header[..4].copy_from_slice(b"OOSW");
        header[4..8].copy_from_slice(&1u32.to_le_bytes());
        header[8..12].copy_from_slice(&2u32.to_le_bytes());
        header[12..20].copy_from_slice(&u64::MAX.to_le_bytes());
        header[20..28].copy_from_slice(&1e-9f64.to_le_bytes());
        assert!(u.load_waveform_bytes(&header, "oversize.bin").is_err());
    }
}

struct DeferredCaptureStatistics {
    capture: u64,
    context: String,
    rows: Vec<usize>,
}

#[derive(Default)]
pub(super) struct Replay {
    cursor: f64,
    pub(super) next: f64,
    force: bool,
    frame: Option<(usize, usize, usize)>,
    viewport: Option<(usize, f32, f32)>,
    pub captures: u64,
    pub(super) trigger_time: Option<f64>,
    pub(super) sim_revision: u64,
    pub(super) sim_attempt: u64,
    pub(super) sim_next: f64,
    pub(super) fresh_sim: bool,
    pub(super) sim_config_change: bool,
    deferred_statistics: Option<DeferredCaptureStatistics>,
}

impl Replay {
    pub(super) fn reset_search(&mut self) {
        self.cursor = 0.0;
    }
}

impl Ui {
    fn capture_statistics_context(&self) -> String {
        let refs: Vec<_> = self.refs.iter().map(|r| r.as_ref().map(|r|
            (r.v.as_ptr() as usize, r.v.len(), r.dt.to_bits(), r.t0.to_bits(), r.vpd.to_bits()))).collect();
        let channels: Vec<_> = self.m.ch.iter().map(|ch| (ch.volts_per_div().to_bits(), ch.unit.name())).collect();
        let gate = (!self.m.meas_full_record).then(|| { let (scale, delay) = self.view(); (scale.to_bits(), delay.to_bits()) });
        format!("{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}", self.m.meas, self.m.meas2,
            (self.m.meas_full_record, self.m.meas_lpct.to_bits(), self.m.meas_hpct.to_bits(), self.m.meas_idle.to_bits(),
                self.m.meas_n, self.m.meas_data, self.m.meas_out, self.m.meas_tsetup.to_bits(), self.m.meas_thold.to_bits()),
            refs, channels, gate, self.m.trigger(), self.m.trigger_wave(),
            (self.m.trig_type, self.m.trig_bus, self.m.trig_bus_val, self.m.trig_nrej))
    }

    fn add_capture_statistic(stat: &mut MStat, value: f64) {
        if !value.is_finite() { return; }
        if stat.n == 0 { stat.min = value; stat.max = value; }
        stat.n += 1; stat.sum += value; stat.sumsq += value * value;
        stat.min = stat.min.min(value); stat.max = stat.max.max(value);
    }

    /// Finish only rows pending for this exact accepted record and measurement
    /// gate/configuration. A worker completion or repeated read is not a capture.
    pub(super) fn finish_deferred_capture_statistics(&mut self) {
        let Some(mut pending) = self.replay.deferred_statistics.take() else { return };
        if pending.capture != self.replay.captures || pending.context != self.capture_statistics_context() { return; }
        let rows = self.meas_rows();
        self.mstat.resize(rows.len(), MStat::default());
        pending.rows.retain(|&index| {
            let Some(row) = rows.get(index) else { return false };
            if row.computing { return true; }
            Self::add_capture_statistic(&mut self.mstat[index], row.v);
            false
        });
        if !pending.rows.is_empty() { self.replay.deferred_statistics = Some(pending); }
    }
    pub fn record_mode(&self) -> bool {
        self.offline || self.session_is_sim || (self.m.n_on() == 0 && self.refs.iter().any(Option::is_some))
    }

    pub(super) fn active_reference(&self) -> usize {
        if self.session_is_sim {
            self.m.sim_slot
        } else if self.offline {
            self.offline_ref
        } else if self.refs.get(self.ref_sel).is_some_and(Option::is_some) {
            self.ref_sel
        } else {
            self.refs.iter().position(Option::is_some).unwrap_or(0)
        }
    }

    pub(super) fn remember_live(&mut self) {
        if self.workflow_hold.is_none() && !self.record_mode() {
            self.workflow_hold = Some(LiveSession { model: self.m.clone(), visible: std::array::from_fn(|i| self.refs[i].as_ref().is_some_and(|r| r.on)) });
        }
    }

    pub fn waveform_epoch(&self) -> u64 { self.workflow_epoch }

    pub fn return_to_live(&mut self) {
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.sim_cancel_generation();
        self.set_offline(false);
        self.session_is_sim = false;
        self.sim_play = false;
        self.replay = Replay::default();
        if let Some(saved) = self.workflow_hold.take() {
            self.m = saved.model;
            for (i, r) in self.refs.iter_mut().enumerate() {
                if let Some(r) = r { r.on = saved.visible[i]; }
            }
        } else {
            for r in self.refs.iter_mut().flatten() { r.on = false; }
            self.m.ch[0].on = true;
            self.m.run = RunState::Run;
            self.m.zoom = false;
            self.m.view = AView::Scope;
            self.m.meas = Self::default_meas();
        }
        self.m.floor_live_tdiv();
        self.select_workspace_source(WaveId::Ch(self.m.active));
        self.last[0] = None;
        self.cleared = false;
        self.ph.clear();
        self.phz.clear();
        self.close_menu();
        self.relayout();
        self.settings_changed();
        self.toast("Returned to live acquisition");
    }

    pub fn request_waveform(&mut self) {
        self.cancel_bus_candidate(true);
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.close_menu();
        self.open_wave_request = true;
        self.toast("Choose a CSV, BIN or WFM waveform. In the browser, use Load waveform above the screen.");
    }

    pub fn load_waveform_bytes(&mut self, data: &[u8], name: &str) -> Result<(), String> {
        let traces = crate::store::parse_wave(data, name)?;
        self.commit_waveform_traces(traces, name, false)
    }

    /// Native/tool Load adds a file without silently replacing the prior source.
    pub fn load_waveform_auto(&mut self, data: &[u8], name: &str) -> Result<(), String> {
        let traces = crate::store::parse_wave(data, name)?;
        self.commit_waveform_traces(traces, name, true)
    }

    /// Main-thread commit for a native file parsed by the import worker. Validate
    /// before mutating the session; failure leaves the working screen intact.
    pub fn commit_waveform_traces(&mut self, traces: Vec<crate::store::WaveTrace>, name: &str, auto: bool) -> Result<(), String> {
        if traces.is_empty() || traces.iter().any(|t| t.v.len() < 2 || !t.dt.is_finite() || t.dt <= 0.0 || !t.t0.is_finite()) {
            return Err("The file needs at least two samples and a valid sample interval.".into());
        }
        let first = if auto {
            (0..NREF).find(|&start| start + traces.len() <= NREF && self.refs[start..start + traces.len()].iter().all(Option::is_none))
                .ok_or("No empty reference slots; select a slot and use Reference > Import to replace it")?
        } else { self.ref_sel.min(NREF - 1) };
        if first + traces.len() > NREF {
            return Err("The file has more traces than the remaining reference slots".into());
        }
        // Adding a source must not discard a user's catalog or channel-pair setup.
        // A fresh, untouched live default gets useful reference readouts on its first import.
        let initialize_measurements = self.refs.iter().all(Option::is_none)
            && self.m.meas == Self::default_meas() && self.m.meas2.is_empty();
        self.cancel_bus_candidate(true);
        self.workflow_epoch = self.workflow_epoch.wrapping_add(1);
        self.sim_cancel_generation();
        self.remember_live();
        self.set_offline(true);
        self.session_is_sim = false;
        self.sim_play = false;
        self.replay = Replay::default();
        // A file is a new stopped scope workspace, not the previous LA/Eye/math view.
        self.reset_workspace_for_import();
        self.m.la.on = false;
        self.m.view = AView::Scope;
        self.xy = false;
        self.m.eye_b = None;
        self.m.eye_rail = None;
        self.invalidate_reference_eye();
        for (k, trace) in traces.into_iter().enumerate() {
            let slot = first + k;
            // Min/max only: do not run millions of samples through edge/period
            // analysis merely to establish a sensible vertical fit.
            let (lo, hi) = trace.v.iter().copied().filter(|x| x.is_finite())
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
            let (lo, hi) = if lo.is_finite() && hi.is_finite() { (lo, hi) } else { (0.0, 1.0) };
            self.refs[slot] = Some(RefWave { src: 0, color: slot % REF_COLORS.len(), label: if trace.label.is_empty() { name.into() } else { trace.label }, t0: trace.t0, dt: trace.dt, v: trace.v,
                vpd: ((hi - lo).abs() / 6.0).max(1e-6), offset: -0.5 * (hi + lo), on: true });
        }
        self.ref_sel = first;
        self.m.set_trigger_wave(WaveId::Ref(first), false);
        self.m.trig_ref2 = None;
        if initialize_measurements {
            self.m.meas = [Meas::Vpp, Meas::Freq, Meas::Vrms, Meas::Rise].into_iter().map(|k| (k, WaveId::Ref(first))).collect();
        }
        self.mstat.clear();
        self.meas_src = WaveId::Ref(first);
        self.close_menu();
        self.present_record(first);
        self.fit_reference_trigger_levels();
        self.relayout();
        self.toast(&format!("Loaded {} into R{}", name, first + 1));
        Ok(())
    }

    pub(super) fn reference_slice(&self, slot: usize) -> Option<(&[f32], f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if let Some((source, lo, hi)) = self.replay.frame.filter(|_| self.record_mode()) {
            let origin = self.refs.get(source)?.as_ref()?;
            let start = origin.t0 + lo as f64 * origin.dt;
            let end = origin.t0 + hi as f64 * origin.dt;
            let lo = (((start - r.t0) / r.dt).floor().max(0.0) as usize).min(r.v.len());
            let hi = (((end - r.t0) / r.dt).ceil().max(0.0) as usize).min(r.v.len()).max(lo);
            return Some((&r.v[lo..hi], r.t0 + lo as f64 * r.dt, r.dt));
        }
        Some((&r.v, r.t0, r.dt))
    }

    pub(crate) fn record_key(&mut self, key_code: u8) -> bool {
        if matches!(key_code, key::RUNSTOP | key::SINGLE | key::FORCE) { self.sim_cancel_generation(); }
        if !self.record_mode() { return false; }
        if key_code == key::DEFAULT {
            self.return_to_live();
            self.key(key_code);
            return true;
        }
        if (0x15..=0x18).contains(&key_code) {
            self.activate_live_channel((key_code - 0x15) as usize);
            return true;
        }
        if matches!(key_code, key::RUNSTOP | key::SINGLE | key::FORCE) && self.refs.get(self.active_reference()).and_then(Option::as_ref).is_none() {
            self.m.run = RunState::Stop;
            self.sim_play = false;
            self.m.status = TrigStatus::Stop;
            self.toast("Load a waveform or start a simulation first");
            return true;
        }
        match key_code {
            key::RUNSTOP => {
                if self.m.run == RunState::Run {
                    self.m.run = RunState::Stop;
                    self.sim_play = false;
                    self.m.status = TrigStatus::Stop;
                } else {
                    if self.m.trig_mode == TrigMode::Single { self.m.trig_mode = TrigMode::Normal; }
                    self.m.run = RunState::Run;
                    self.sim_play = true;
                    self.m.status = TrigStatus::Wait;
                    self.replay.next = 0.0;
                }
            }
            key::SINGLE => {
                self.m.trig_mode = TrigMode::Single;
                self.m.run = RunState::Run;
                self.sim_play = true;
                self.m.status = TrigStatus::Wait;
                self.replay.next = 0.0;
            }
            key::FORCE => {
                self.replay.force = true;
                self.m.run = RunState::Run;
                self.sim_play = true;
                self.replay.next = 0.0;
            }
            _ => return false,
        }
        true
    }

    pub(super) fn replay_acquire(&mut self, _dt_wall: f64) {
        if self.m.run != RunState::Run || !self.sim_play || self.now < self.replay.next { return; }
        self.replay.next = self.now + 0.1;
        let slot = self.m.trig_ref.unwrap_or_else(|| self.active_reference());
        let Some(r) = self.refs.get(slot).and_then(Option::as_ref) else { return; };
        if r.v.len() < 8 || r.dt <= 0.0 { return; }
        let count = ((self.view().0 as f64 * HDIV as f64 / r.dt).ceil() as usize).clamp(8, r.v.len());
        let Some(hits) = self.reference_trigger_hits(slot) else { self.m.status = TrigStatus::Wait; return; };
        let after = r.t0 + self.replay.cursor * r.dt;
        let next = hits.partition_point(|&time| time <= after);
        let hit = hits.get(next).or_else(|| hits.first()).copied();
        if hit.is_none() && self.m.trig_mode != TrigMode::Auto && !self.replay.force {
            self.m.status = TrigStatus::Wait;
            return;
        }
        let middle = hit.map(|time| (time - r.t0) / r.dt).unwrap_or((self.replay.cursor + count as f64 * 0.5).min((r.v.len() - 1) as f64));
        let lo = (middle.max(count as f64 * 0.5) as usize).saturating_sub(count / 2).min(r.v.len() - count);
        let hi = lo + count;
        let centre = r.t0 + middle * r.dt;
        self.replay.cursor = (middle + (count / 2).max(1) as f64) % r.v.len() as f64;
        self.replay.frame = Some((slot, lo, hi));
        self.replay.force = false;
        self.replay.captures += 1;
        self.replay.trigger_time = hit;
        if self.session_is_sim && self.replay.fresh_sim {
            self.replay.sim_revision = self.replay.sim_revision.wrapping_add(1);
            self.replay.fresh_sim = false;
        }
        if self.m.zoom { self.m.zdelay = centre as f32; } else { self.m.delay = centre as f32; }
        self.replay.viewport = Some((slot, self.view().0, self.view().1));
        self.m.status = if hit.is_some() { TrigStatus::Triggered } else { TrigStatus::Auto };
        if self.m.trig_mode == TrigMode::Single {
            self.m.run = RunState::Stop;
            self.sim_play = false;
        }
        if self.replay.sim_config_change {
            self.invalidate_reference_eye();
            self.mstat.clear();
            self.replay.sim_config_change = false;
        }
        self.measure_refs();
        self.analyzer_update();
        let rows = self.meas_rows();
        self.mstat.resize(rows.len(), MStat::default());
        let mut pending = Vec::new();
        for (index, (row, stat)) in rows.iter().zip(&mut self.mstat).enumerate() {
            if row.computing { pending.push(index); }
            else { Self::add_capture_statistic(stat, row.v); }
        }
        self.replay.deferred_statistics = (!pending.is_empty()).then(|| DeferredCaptureStatistics {
            capture: self.replay.captures, context: self.capture_statistics_context(), rows: pending });
    }

    pub fn workflow_command(&mut self, action: &str) {
        match action {
            "live" => self.return_to_live(),
            "sim" => self.toggle_menu(MenuId::Sim),
            "load" => self.request_waveform(),
            "measure" => self.open_meas_list(),
            "settings" => self.toggle_menu(MenuId::Measure),
            _ => {}
        }
    }

    pub(super) fn refresh_stopped_reference(&mut self) {
        let slot = self.active_reference();
        let (scale, delay) = self.view();
        let viewport = (slot, scale, delay);
        if self.replay.viewport == Some(viewport) { return; }
        self.replay.viewport = Some(viewport);
        let Some(r) = self.refs.get(slot).and_then(Option::as_ref) else { return; };
        if r.dt <= 0.0 || r.v.len() < 2 { return; }
        let half = scale as f64 * HDIV as f64 * 0.5;
        let lo = (((delay as f64 - half - r.t0) / r.dt).floor().max(0.0) as usize).min(r.v.len());
        let hi = (((delay as f64 + half - r.t0) / r.dt).ceil().max(0.0) as usize).min(r.v.len());
        self.replay.frame = Some((slot, lo, hi.max(lo)));
        self.measure_refs();
        self.analyzer_update();
    }

    pub(super) fn record_caption(&self) -> String {
        let slot = self.active_reference();
        format!("{} R{} · {} measurements · #{}", if self.session_is_sim { "SIM" } else { "Reference" }, slot + 1, self.measurement_gate_label(), self.replay.captures)
    }

    pub fn workflow_state(&self) -> String {
        use crate::json::J;
        let status = match self.m.status {
            TrigStatus::Triggered => "TRIGGERED", TrigStatus::Wait => "WAIT", TrigStatus::Auto => "AUTO", TrigStatus::Stop => "STOP",
        };
        J::obj(&[
            ("mode", J::s(if self.session_is_sim { "sim" } else if self.record_mode() { "reference" } else { "live" })),
            ("running", J::Bool(self.m.run == RunState::Run)),
            ("busy", J::Bool(self.sim_generating())),
            ("measurementBusy", J::Bool(self.measurement_busy())),
            ("source", J::s(&if self.record_mode() { format!("R{}", self.active_reference() + 1) } else { format!("CH{}", self.m.active + 1) })),
            ("captures", J::Num(self.replay.captures as f64)),
            ("status", J::s(status)),
            ("gate", J::s(if self.m.meas_full_record { "full-record" } else { "screen" })),
            ("loadRect", self.top_buttons().iter().find(|button| button.0 == TopBtn::Load).map_or(J::Null, |button| {
                let r = button.2;
                J::Arr(vec![J::Num(r.x as f64), J::Num(r.y as f64), J::Num(r.w as f64), J::Num(r.h as f64)])
            })),
        ]).to_string()
    }

    pub(super) fn record_cards(&self) -> Vec<(usize, Rect)> {
        let slots: Vec<_> = (0..NREF).filter(|&i| self.refs[i].is_some()).collect();
        let mut shown: Vec<_> = slots.iter().copied().take(4).collect();
        let active = self.active_reference();
        if !shown.contains(&active) && self.refs[active].is_some() {
            if shown.len() == 4 { shown.pop(); }
            shown.push(active);
        }
        let width = 574 / shown.len().max(1) as i32 - 7;
        shown.into_iter().enumerate().map(|(n, i)| (i, Rect::new(75 + n as i32 * (width + 7), 530, width, 66))).collect()
    }

    pub(super) fn draw_record_cards(&self, cv: &mut Canvas) {
        for (slot, rect) in self.record_cards() {
            let r = self.refs[slot].as_ref().unwrap();
            let col = REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1;
            btn(cv, rect, BAR, if r.on { col } else { CARD_BORDER });
            cv.set_clip(rect.inset(7));
            cv.text(&self.fonts.sb, rect.x + 10, rect.y + 18, &format!("R{}", slot + 1), col);
            cv.text(&self.fonts.s, rect.x + rect.w - 34, rect.y + 18, if r.on { "On" } else { "Off" }, if r.on { col } else { DIM });
            cv.text(&self.fonts.mb, rect.x + 10, rect.y + 39, if r.label.is_empty() { "Waveform" } else { &r.label }, TEXT);
            cv.text(&self.fonts.s, rect.x + 10, rect.y + 57, &format!("{}/div", si_short(r.vpd as f64, "V")), DIM);
            cv.no_clip();
        }
    }

    pub(super) fn session_buttons(r: Rect) -> (Rect, Rect) {
        (Rect::new(r.x + 6, r.y + 22, r.w / 2 - 9, 40), Rect::new(r.x + r.w / 2 + 3, r.y + 22, r.w / 2 - 9, 40))
    }

    pub(super) fn draw_session_card(&self, cv: &mut Canvas, r: Rect) {
        btn(cv, r, BAR, CARD_BORDER);
        let title = if self.session_is_sim { "SIMULATION" } else { "STORED WAVEFORM" };
        cv.text(&self.fonts.sb, r.x + 10, r.y + 16, title, ACCENT);
        let (pause, exit) = Self::session_buttons(r);
        btn(cv, pause, BTN, BTN_BORDER);
        cv.text_in(&self.fonts.sb, pause, 1, if self.m.run == RunState::Run { "Stop" } else { "Run" }, TEXT);
        btn(cv, exit, BTN, BTN_BORDER);
        cv.text_in(&self.fonts.sb, exit, 1, "Live", TEXT);
    }
}
