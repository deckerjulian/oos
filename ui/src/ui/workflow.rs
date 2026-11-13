// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Direct file, reference, simulation and acquisition controls shared by the screen and web remote.
use super::*;

pub(super) struct LiveSession {
    model: Model,
    visible: [bool; NREF],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay() -> Ui {
        let mut u = Ui::new(Source::Demo { t: 0.0 });
        u.remember_live();
        u.session_is_sim = true;
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

#[derive(Default)]
pub(super) struct Replay {
    cursor: f64,
    next: f64,
    force: bool,
    frame: Option<(usize, usize, usize)>,
    viewport: Option<(usize, f32, f32)>,
    pub captures: u64,
}

impl Ui {
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

    pub fn return_to_live(&mut self) {
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
        self.meas_src = WaveId::Ch(self.m.active);
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
        self.close_menu();
        self.open_wave_request = true;
        self.toast("Choose a CSV, BIN or WFM waveform. In the browser, use Load waveform above the screen.");
    }

    pub fn load_waveform_bytes(&mut self, data: &[u8], name: &str) -> Result<(), String> {
        // Validate before changing the session, so a bad file leaves the working display intact.
        let traces = crate::store::parse_wave(data, name)?;
        if traces.is_empty() || traces.iter().any(|t| t.v.len() < 2 || !t.dt.is_finite() || t.dt <= 0.0 || !t.t0.is_finite()) {
            return Err("The file needs at least two samples and a valid sample interval.".into());
        }
        self.remember_live();
        self.set_offline(true);
        self.session_is_sim = false;
        self.replay = Replay::default();
        let first = self.ref_sel.min(NREF - 1);
        for (k, trace) in traces.into_iter().take(NREF - first).enumerate() {
            let slot = first + k;
            let span = trace.v.iter().copied().filter(|x| x.is_finite()).fold(0.1f32, |a, b| a.max(b.abs()));
            self.refs[slot] = Some(RefWave { src: 0, color: slot % REF_COLORS.len(), label: if trace.label.is_empty() { name.into() } else { trace.label }, t0: trace.t0, dt: trace.dt, v: trace.v, vpd: (span / 3.0).max(1e-6), offset: 0.0, on: true });
        }
        self.m.zoom = false;
        self.present_record(first);
        self.meas_src = WaveId::Ref(first);
        self.m.meas = [Meas::Vpp, Meas::Freq, Meas::Vrms, Meas::Rise].into_iter().map(|k| (k, WaveId::Ref(first))).collect();
        self.m.meas2.clear();
        self.m.meas_labels = false;
        self.close_menu();
        self.autoset_reference();
        self.measure_refs();
        self.relayout();
        self.toast(&format!("Loaded {} into R{}", name, first + 1));
        Ok(())
    }

    pub(super) fn reference_slice(&self, slot: usize) -> Option<(&[f32], f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if let Some((source, lo, hi)) = self.replay.frame.filter(|f| self.record_mode() && f.0 == slot) {
            let _ = source;
            return Some((&r.v[lo.min(r.v.len())..hi.min(r.v.len())], r.t0 + lo as f64 * r.dt, r.dt));
        }
        Some((&r.v, r.t0, r.dt))
    }

    pub(super) fn record_key(&mut self, key_code: u8) -> bool {
        if !self.record_mode() { return false; }
        if key_code == key::DEFAULT || (0x15..=0x18).contains(&key_code) {
            self.return_to_live();
            self.key(key_code);
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
        let slot = self.active_reference();
        let Some(r) = self.refs.get(slot).and_then(Option::as_ref) else { return; };
        if r.v.len() < 8 || r.dt <= 0.0 { return; }
        let count = ((self.view().0 as f64 * HDIV as f64 / r.dt).ceil() as usize).clamp(8, r.v.len());
        let mut model = self.m.clone();
        model.trig_src = 0;
        let empty = [];
        let input = oos_dsp::trigger::Input {
            ch: std::array::from_fn(|c| if c == 0 { r.v.as_slice() } else {
                self.refs.get((slot + c) % NREF).and_then(Option::as_ref).filter(|other| (other.dt - r.dt).abs() < r.dt * 1e-6 && (other.t0 - r.t0).abs() < r.dt * 0.1).map_or(&empty[..], |other| other.v.as_slice())
            }),
            dt: r.dt,
            hyst: [r.vpd.max(1e-6) * 0.1; NCH],
        };
        let hit = crate::trig::find_first(&model, &input, self.replay.cursor)
            .or_else(|| crate::trig::find_first(&model, &input, 0.0));
        if hit.is_none() && self.m.trig_mode != TrigMode::Auto && !self.replay.force {
            self.m.status = TrigStatus::Wait;
            return;
        }
        let middle = hit.unwrap_or((self.replay.cursor + count as f64 * 0.5).min((r.v.len() - 1) as f64));
        let lo = (middle.max(count as f64 * 0.5) as usize).saturating_sub(count / 2).min(r.v.len() - count);
        let hi = lo + count;
        let centre = r.t0 + middle * r.dt;
        self.replay.cursor = (middle + (count / 2).max(1) as f64) % r.v.len() as f64;
        self.replay.frame = Some((slot, lo, hi));
        self.replay.force = false;
        self.replay.captures += 1;
        if self.m.zoom { self.m.zdelay = centre as f32; } else { self.m.delay = centre as f32; }
        self.replay.viewport = Some((slot, self.view().0, self.view().1));
        self.m.status = if hit.is_some() { TrigStatus::Triggered } else { TrigStatus::Auto };
        if self.m.trig_mode == TrigMode::Single {
            self.m.run = RunState::Stop;
            self.sim_play = false;
        }
        self.measure_refs();
        self.analyzer_update();
        let rows = self.meas_rows();
        self.mstat.resize(rows.len(), MStat::default());
        for (row, stat) in rows.iter().zip(&mut self.mstat) {
            if row.v.is_finite() {
                if stat.n == 0 { stat.min = row.v; stat.max = row.v; }
                stat.n += 1; stat.sum += row.v; stat.sumsq += row.v * row.v;
                stat.min = stat.min.min(row.v); stat.max = stat.max.max(row.v);
            }
        }
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
        format!("{} R{} · capture {}", if self.session_is_sim { "SIM" } else { "Reference" }, slot + 1, self.replay.captures)
    }

    pub fn workflow_state(&self) -> String {
        use crate::json::J;
        let status = match self.m.status {
            TrigStatus::Triggered => "TRIGGERED", TrigStatus::Wait => "WAIT", TrigStatus::Auto => "AUTO", TrigStatus::Stop => "STOP",
        };
        J::obj(&[
            ("mode", J::s(if self.session_is_sim { "sim" } else if self.record_mode() { "reference" } else { "live" })),
            ("running", J::Bool(self.m.run == RunState::Run)),
            ("source", J::s(&if self.record_mode() { format!("R{}", self.active_reference() + 1) } else { format!("CH{}", self.m.active + 1) })),
            ("captures", J::Num(self.replay.captures as f64)),
            ("status", J::s(status)),
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
