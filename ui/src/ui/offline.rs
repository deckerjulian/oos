// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Stored-reference session. A loaded reference stays on the live screen, with its own sample rate
//! and length, until Take over or Offline waveforms is chosen. Those hide the analog cards. Take
//! over shows one reference on the record's own time base. Offline waveforms keeps several together.
//! A SIM session is separate: its readout starts with SIM, and Live returns to acquisition.

use super::*;

/// Live settings put back when the mode is turned off.
pub(super) struct LiveHold {
    on: [bool; NCH],
    ref_on: [bool; NREF],
    run: RunState,
    status: TrigStatus,
    tscale: usize,
    tfine: f32,
    delay: f32,
}

impl Ui {
    pub fn set_offline(&mut self, on: bool) {
        if on == self.offline {
            return;
        }
        if on {
            self.offline_hold = Some(LiveHold {
                on: std::array::from_fn(|c| self.m.ch[c].on),
                ref_on: std::array::from_fn(|i| self.refs[i].as_ref().is_some_and(|r| r.on)),
                run: self.m.run,
                status: self.m.status,
                tscale: self.m.tscale,
                tfine: self.m.tfine,
                delay: self.m.delay,
            });
            self.offline = true;
            if self.look == reflook::RefLook::Live {
                self.look = reflook::RefLook::TakeOver;
            }
            self.idle_analog();
            self.ph.clear();
            self.phz.clear();
        } else {
            self.offline = false;
            self.look = reflook::RefLook::Live;
            self.sim_play = false;
            if let Some(h) = self.offline_hold.take() {
                for (c, on) in h.on.into_iter().enumerate() {
                    self.m.ch[c].on = on && c < n_ch();
                }
                for (i, r) in self.refs.iter_mut().enumerate() {
                    if let Some(r) = r {
                        r.on = h.ref_on[i];
                    }
                }
                self.m.run = h.run;
                self.m.status = h.status;
                self.m.tscale = h.tscale;
                self.m.tfine = h.tfine;
                self.m.delay = h.delay;
            }
            self.dirty_map = true;
        }
    }

    /// Analog off and the acquisition held, so the demo source is not on screen.
    pub(super) fn idle_analog(&mut self) {
        for ch in &mut self.m.ch {
            ch.on = false;
        }
        self.m.run = RunState::Stop;
        self.m.status = TrigStatus::Stop;
    }

    /// Import and Play, while the mode is on: show `slot`, analog off, TIME from that record.
    pub(super) fn present_record(&mut self, slot: usize) {
        let slot = slot.min(NREF - 1);
        self.idle_analog();
        self.offline_ref = slot;
        self.sim_play = false;
        let spec = self.refs[slot].as_mut().map(|r| {
            r.on = true;
            (r.t0, r.dt, r.v.len())
        });
        if let Some((t0, dt, n)) = spec {
            self.apply_record_timebase(t0, dt, n);
        }
        self.m.eye_src = WaveId::Ref(slot);
        self.ph.clear();
        self.phz.clear();
        // Install the stopped screen gate before touching other references.
        // A previous full SSC record may be millions of samples long.
        self.refresh_stopped_reference();
    }

    /// Horizontal scale that fits the record, including a factor below the live 1 ns step.
    pub(super) fn apply_record_timebase(&mut self, t0: f64, dt: f64, n: usize) {
        if n < 2 || !(dt > 0.0) {
            return;
        }
        let span = n as f64 * dt;
        let want = (span / HDIV as f64) as f32;
        let ts = tscales();
        if want >= ts[0] {
            self.m.tscale = ts.iter().position(|&t| t as f64 >= want as f64 * 0.999).unwrap_or(ts.len() - 1);
            self.m.tfine = 1.0;
        } else {
            self.m.tscale = 0;
            self.m.tfine = (want / ts[0]).clamp(1e-6, 1.0);
        }
        self.m.delay = (t0 + span / 2.0) as f32;
    }

    pub(super) fn run_caption(&self) -> (&'static str, Rgb) {
        if self.record_mode() {
            return match (self.m.run, self.m.status) {
                (RunState::Stop, _) => ("STOP", RED),
                (_, TrigStatus::Wait) => ("ARMED", AMBER),
                (_, TrigStatus::Triggered) => ("TRIG'D", GREEN),
                _ => ("AUTO", GREEN),
            };
        }
        match (self.m.run, self.m.status) {
            _ if self.cal_running() => ("CAL", ACCENT),
            (RunState::Stop, _) | (_, TrigStatus::Stop) => ("STOP", RED),
            (_, TrigStatus::Triggered) => ("TRIG'D", GREEN),
            (_, TrigStatus::Auto) => ("AUTO", GREEN),
            (_, TrigStatus::Wait) => ("WAIT", AMBER),
        }
    }

    /// Sample rate and depth for the top line. The bool is the live acquire mode that follows it.
    pub(super) fn acq_caption(&self) -> (String, bool) {
        if self.record_mode() {
            let kind = if self.session_is_sim { "SIM" } else { "REF" };
            if let Some(r) = self.refs.get(self.active_reference()).and_then(|r| r.as_ref()).filter(|r| r.v.len() >= 2 && r.dt > 0.0) {
                let sr = 1.0 / r.dt;
                return (format!("{} \u{b7} {} \u{b7} {}", kind, si_digits(sr, "Sa/s", 3), si_digits(r.v.len() as f64, "pts", 3)), false);
            }
            return (format!("{kind} \u{b7} \u{2014}"), false);
        }
        let (sr, depth) = self.src.hw_rate_depth().map(|(r, n)| (r, n as u64)).unwrap_or_else(|| self.m.srate_depth());
        (format!("{} \u{b7} {}", si_digits(sr, "Sa/s", 3), si_digits(depth as f64, "pts", 3)), true)
    }

    /// Slot the DVM and the counter read while the mode is on.
    pub(super) fn offline_meter(&self) -> Option<usize> {
        self.record_mode().then_some(self.active_reference())
    }

    pub(super) fn ref_card_lines(&self) -> (String, String) {
        let i = self.offline_ref.min(NREF - 1);
        match &self.refs[i] {
            Some(r) => {
                let sub = if r.label.is_empty() { format!("{} pts", r.v.len()) } else { r.label.clone() };
                (format!("R{}", i + 1), sub)
            }
            None => (format!("R{}", i + 1), "empty".into()),
        }
    }

    pub(super) fn draw_offline_card(&self, cv: &mut Canvas, r: Rect, sel: bool) {
        let f = &self.fonts;
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        let (name, sub) = self.ref_card_lines();
        cv.set_clip(r.inset(3));
        cv.text(&f.sb, r.x + 12, r.y + 18, &name, LABEL);
        cv.text(Self::card_font(f, &sub, r.w - 24), r.x + 12, r.y + 48, &sub, TEXT);
        cv.no_clip();
    }

    /// Run pill while the mode is on: play the loaded record, or hold it. It does not start the demo.
    pub(super) fn toggle_offline_run(&mut self) {
        if self.sim_play {
            self.sim_play = false;
            return;
        }
        let ready = self.refs.get(self.offline_ref).and_then(|r| r.as_ref()).is_some_and(|r| r.v.len() >= 2 && r.dt > 0.0);
        if ready {
            self.sim_play = true;
        }
    }

    pub(super) fn offline_kind(it: Item) -> Option<Kind> {
        match it {
            Item::Offline => Some(Kind::Choice),
            _ => None,
        }
    }

    pub(super) fn offline_label(it: Item) -> Option<&'static str> {
        match it {
            Item::Offline => Some("Record"),
            _ => None,
        }
    }

    pub(super) fn offline_value(&self, it: Item) -> Option<String> {
        match it {
            Item::Offline => Some(match self.look {
                reflook::RefLook::Live => "With live".into(),
                reflook::RefLook::TakeOver => "Take over".into(),
                reflook::RefLook::Waves => "Offline".into(),
            }),
            Item::DvmSrc | Item::CntSrc if self.offline => Some(format!("R{}", self.offline_ref.min(NREF - 1) + 1)),
            _ => None,
        }
    }

    pub(super) fn offline_options(&self, it: Item) -> Option<Vec<String>> {
        match it {
            Item::Offline => Some(vec!["With live".into(), "Take over".into(), "Offline".into()]),
            Item::DvmSrc | Item::CntSrc if self.offline => Some((0..NREF).map(|i| format!("R{}", i + 1)).collect()),
            _ => None,
        }
    }

    pub(super) fn offline_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::Offline => {
                if d != 0 {
                    let order = [reflook::RefLook::Live, reflook::RefLook::TakeOver, reflook::RefLook::Waves];
                    let i = order.iter().position(|x| *x == self.look).unwrap_or(0);
                    let n = (i as i32 + d).rem_euclid(order.len() as i32) as usize;
                    self.set_look(order[n]);
                }
                true
            }
            Item::DvmSrc | Item::CntSrc if self.offline => {
                if d != 0 {
                    self.offline_ref = (self.offline_ref as i32 + d).rem_euclid(NREF as i32) as usize;
                    self.measure_refs();
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::new(Source::Demo { t: 0.0 })
    }

    #[test]
    fn the_live_screen_is_unchanged_until_the_mode_is_on() {
        let mut u = ui();
        assert!(!u.offline);
        assert!(u.m.ch[0].on);
        assert_eq!(u.run_caption().0, "WAIT");
        assert!(u.acq_caption().1);
        let cards = u.cards().0.map(|r| r.w);
        assert!(cards[0] >= 54);
        u.set_offline(true);
        assert!(u.cards().0.iter().all(|r| r.w == 0));
        assert!(u.m.ch.iter().all(|c| !c.on));
        assert_eq!(u.run_caption().0, "STOP");
        assert!(!u.acq_caption().1);
        u.set_offline(false);
        assert!(u.m.ch[0].on);
        assert!(u.m.ch[1..].iter().all(|c| !c.on));
        assert_eq!(u.m.run, RunState::Run);
        assert_eq!(u.cards().0.map(|r| r.w), cards);
        assert_eq!(u.run_caption().0, "WAIT");
    }

    #[test]
    fn a_session_can_start_with_no_demo_on_the_channels() {
        let mut u = ui();
        u.set_offline(true);
        for _ in 0..4 {
            u.step(0.04);
        }
        assert_eq!(u.wfm_total, 0);
        assert!(u.last[0].is_none());
        assert!(u.m.ch.iter().all(|c| !c.on));
        assert!(u.ph.map.iter().all(|m| m.iter().all(|&h| h == 0)));
        assert_eq!(u.run_caption().0, "STOP");
        let r = Ui::RUN_PILL;
        u.tap(r.x + 8, r.y + 8);
        assert_eq!(u.run_caption().0, "STOP");
    }

    #[test]
    fn import_matches_play_on_the_record() {
        let dir = std::env::temp_dir().join(format!("oos-offline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut live = ui();
        live.store_dir = dir.clone();
        let scale = live.m.tscale;
        let dt = 5e-9;
        let n = 400;
        let sine: Vec<f32> = (0..n).map(|k| (0.5 + (2.0 * std::f64::consts::PI * 40e6 * k as f64 * dt).sin()) as f32).collect();
        write_csv(&dir, 1, 0.0, dt, &sine);
        live.activate(Item::RefImport);
        assert!(live.m.ch[0].on && !live.offline, "import stays with the live channels");
        assert!(live.refs[0].as_ref().unwrap().on);
        let _ = scale;

        let mut play = ui();
        play.activate(Item::SimPlay);
        play.wait_sim_generation();
        assert!(play.m.ch.iter().all(|channel| !channel.on), "SIM plays a stored reference");
        assert!(play.refs[play.m.sim_slot].is_some());

        let mut u = ui();
        u.store_dir = dir.clone();
        let held = (u.m.tscale, u.m.tfine, u.m.delay);
        u.set_offline(true);
        u.m.sim_slot = 2;
        u.m.sim_preset = oos_dsp::serial::LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
        u.activate(Item::SimPlay);
        u.wait_sim_generation();
        assert!(u.m.ch.iter().all(|c| !c.on));
        assert!(u.sim_play);
        assert!(matches!(u.run_caption().0, "ARMED" | "AUTO" | "TRIG'D"));
        assert_eq!(u.ref_card_lines(), ("R3".into(), "PCIe 4.0".into()));
        let rec = u.refs[2].as_ref().unwrap();
        assert!(u.m.zoom);
        assert!(u.m.time_per_div() as f64 * HDIV as f64 >= rec.v.len() as f64 * rec.dt);
        let (play_sr, _) = u.acq_caption();
        assert!(play_sr.contains("Sa/s") && !play_sr.contains("Normal"), "{}", play_sr);
        u.m.dvm_on = true;
        u.m.cnt_on = true;
        let row = u.dvm_row();
        assert!(row[0].0.starts_with("DVM R3") && row[1].0.starts_with("Counter R3"), "{:?}", row);
        assert!(u.dvm_value().is_finite());

        u.ref_sel = 0;
        u.activate(Item::RefImport);
        assert!(u.m.ch.iter().all(|c| !c.on));
        assert!(!u.sim_play);
        assert_eq!(u.run_caption().0, "STOP");
        assert_eq!(u.ref_card_lines().0, "R1");
        let loaded = u.refs[0].as_ref().unwrap();
        assert_eq!(loaded.v, sine);
        assert!(u.m.time_per_div() > 0.0 && u.m.time_per_div() as f64 * HDIV as f64 <= n as f64 * dt * 1.01, "import fits a useful view of the record");
        let (sr, live_mode) = u.acq_caption();
        assert!(!live_mode);
        assert!(sr.contains("200 MSa/s") || sr.contains("200.000"), "{}", sr);
        u.m.dvm_mode = 0;
        assert!((u.dvm_value() - 0.5).abs() < 0.05, "{}", u.dvm_value());
        u.m.dvm_mode = 1;
        assert!((u.dvm_value() - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.05, "{}", u.dvm_value());
        u.m.cnt_mode = 0;
        let hz = u.dvm_row().into_iter().find(|r| r.0.starts_with("Counter")).unwrap();
        assert!(hz.0.starts_with("Counter R1") && hz.1.contains("Hz") && hz.1 != "----", "{:?}", hz);
        u.m.cnt_mode = 2;
        assert_ne!(u.dvm_row()[1].1, "----");

        u.return_to_live();
        assert!(u.m.ch[0].on);
        assert_eq!((u.m.tscale, u.m.tfine, u.m.delay), held);
        assert!(!u.sim_play);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_keeps_the_live_window_until_take_over() {
        let mut u = ui();
        u.refs[1] = Some(RefWave {
            src: 0, color: 1, label: "previous capture".into(), t0: 0.0, dt: 1e-9,
            v: (0..20_000).map(|index| {
                let amplitude = if index < 1000 { 1.0 } else { 5.0 };
                if index % 4 < 2 { -amplitude } else { amplitude }
            }).collect(), vpd: 1.0, offset: 0.0, on: true,
        });
        let held = (u.m.tscale, u.m.tfine, u.m.delay);
        let mut csv = String::from("Time (s),Volts\n");
        for index in 0..100 {
            csv.push_str(&format!("{},{}\n", index as f64 * 1e-9, if index % 4 < 2 { -0.5 } else { 0.5 }));
        }
        u.load_waveform_bytes(csv.as_bytes(), "new record.csv").unwrap();
        assert_eq!((u.m.tscale, u.m.tfine, u.m.delay), held);
        assert!(u.m.ch[0].on && !u.offline);
        assert!(u.refs[0].is_some() && u.refs[1].as_ref().unwrap().on);
        u.set_look(reflook::RefLook::TakeOver);
        assert_timebase(&u, 0.0, 1e-9, 100);
        assert!(u.m.ch.iter().all(|c| !c.on));
    }

    #[test]
    fn a_fast_record_stays_with_the_live_channels() {
        let mut u = ui();
        let live = u.m.time_per_div();
        let mut csv = String::from("Time (s),Volts\n");
        for index in 0..400 {
            csv.push_str(&format!("{},{}\n", index as f64 * 1e-12, if index % 2 == 0 { -0.2 } else { 0.2 }));
        }
        u.load_waveform_bytes(csv.as_bytes(), "fast.csv").unwrap();
        let (acq, live_mode) = u.acq_caption();
        assert!(live_mode, "{}", acq);
        assert_eq!(u.m.time_per_div(), live);
        assert!(u.m.ch[0].on && !u.offline && !u.session_is_sim);
        let note = u.live_ref_note();
        assert!(note.contains("R1") && note.contains("Sa/s"), "{note}");
        u.fit_shown_reference();
        assert!(u.m.ch[0].on && !u.offline);
        assert!(u.m.time_per_div() < live);
        u.session_is_sim = true;
        assert!(u.acq_caption().0.starts_with("SIM"), "{}", u.acq_caption().0);
    }

    fn assert_timebase(u: &Ui, t0: f64, dt: f64, n: usize) {
        let span = n as f64 * dt;
        let want = span / HDIV as f64;
        let got = u.m.time_per_div() as f64;
        assert!(got + 1e-15 >= want, "time/div {} shorter than the record {}", got, want);
        assert!(got < want * 2.5 + 1e-12, "time/div {} is not the record's step", got);
        assert!((u.m.delay as f64 - (t0 + span / 2.0)).abs() < span * 1e-4 + 1e-9);
    }

    fn write_csv(dir: &std::path::Path, slot: usize, t0: f64, dt: f64, v: &[f32]) {
        let mut t = String::from("Time (s),Volts\n");
        for (k, x) in v.iter().enumerate() {
            t.push_str(&format!("{:.12e},{}\n", t0 + k as f64 * dt, x));
        }
        std::fs::write(dir.join(format!("ref{}.csv", slot)), t).unwrap();
    }
}
