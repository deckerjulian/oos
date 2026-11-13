// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM: load a nominal serial record into a reference slot and walk it on the phosphor.
//! Live analog at 16 Gb/s is not the path; this is the offline record.

use super::*;
use oos_dsp::serial::{self, Lane, LANES};

impl Ui {
    pub(super) fn sim_items(&self) -> Vec<Item> {
        vec![Item::SimPreset, Item::SimSlot, Item::SimPlay, Item::SimShow]
    }

    pub(super) fn sim_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SimPlay => Kind::Action,
            Item::SimPreset | Item::SimSlot | Item::SimShow => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn sim_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Sim) => "SIM",
            Item::SimPreset => "Preset",
            Item::SimSlot => "Slot",
            Item::SimPlay => "Play",
            Item::SimShow => "Show",
            _ => return None,
        })
    }

    pub(super) fn sim_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::SimPreset => self.sim_lane().name.into(),
            Item::SimSlot => format!("R{}", m.sim_slot + 1),
            Item::SimPlay => if self.sim_play { "Stop".into() } else { String::new() },
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
            Item::SimSlot => (0..NREF).map(|i| format!("R{}", i + 1)).collect(),
            Item::SimShow => vec!["Off".into(), "On".into()],
            _ => return None,
        })
    }

    pub(super) fn sim_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::SimPreset => {
                let n = LANES.len() as i32;
                self.m.sim_preset = (self.m.sim_preset as i32 + d).rem_euclid(n) as usize;
            }
            Item::SimSlot => self.m.sim_slot = (self.m.sim_slot as i32 + d).rem_euclid(NREF as i32) as usize,
            Item::SimShow => {
                if let Some(r) = &mut self.refs[self.m.sim_slot] {
                    r.on = !r.on;
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn sim_activate(&mut self, it: Item) -> bool {
        if it != Item::SimPlay {
            return false;
        }
        if self.sim_play {
            self.sim_play = false;
            self.toast("SIM stopped");
            return true;
        }
        self.sim_load();
        true
    }

    fn sim_lane(&self) -> &'static Lane {
        &LANES[self.m.sim_preset.min(LANES.len() - 1)]
    }

    /// Writes the selected preset into the selected R slot and starts the phosphor walk.
    fn sim_load(&mut self) {
        let lane = self.sim_lane();
        let v = serial::generate(lane);
        let dt = serial::dt_of(lane);
        let span = (lane.v_high - lane.v_low).abs().max(0.2);
        let vpd = nearest_vdiv((span / 6.0) as f32);
        let slot = self.m.sim_slot;
        self.refs[slot] = Some(RefWave {
            src: 0,
            color: 0,
            label: lane.name.into(),
            t0: 0.0,
            dt,
            v,
            vpd,
            offset: 0.0,
            on: true,
        });
        if self.offline {
            self.present_record(slot);
            self.m.eye_rate = lane.bits_per_s;
            self.sim_i = 0;
            self.sim_play = true;
            self.toast(&format!("{} → R{}", lane.name, slot + 1));
            return;
        }
        self.m.ch[0].on = true;
        self.m.ch[0].vscale = VSCALES.iter().position(|&s| (s - vpd).abs() <= s * 1e-3).unwrap_or(11);
        self.m.ch[0].offset = 0.0;
        // about eight unit intervals on screen
        let want = 8.0 / lane.bits_per_s / HDIV as f64;
        let ts = tscales();
        self.m.tscale = ts.iter().position(|&t| t as f64 >= want).unwrap_or(ts.len() - 1);
        self.m.delay = 0.0;
        self.m.eye_src = WaveId::Ref(slot);
        self.m.eye_rate = lane.bits_per_s;
        self.m.run = RunState::Stop;
        self.sim_i = 0;
        self.sim_play = true;
        self.ph.clear();
        self.measure_refs();
        self.analyzer_update();
        self.toast(&format!("{} → R{}", lane.name, slot + 1));
    }

    /// Walks rising edges of the loaded slot and folds each window into the CH1 phosphor.
    pub(super) fn sim_tick(&mut self, dt_wall: f64) {
        if self.offline {
            return;
        }
        let slot = self.m.sim_slot;
        let Some(r) = self.refs[slot].as_ref() else {
            self.sim_play = false;
            return;
        };
        if r.v.len() < 64 {
            self.sim_play = false;
            return;
        }
        let persist = PERSIST[self.m.persist.min(PERSIST.len() - 1)].1;
        if persist > 0.0 {
            let f = (-dt_wall / persist as f64).exp() as f32;
            self.ph.decay(f);
        }
        let (lo, hi) = r.v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |s, &x| (s.0.min(x), s.1.max(x)));
        let mid = 0.5 * (lo + hi);
        let hyst = 0.08 * (hi - lo).max(0.05);
        let rising: Vec<f64> = oos_dsp::edges::edges(&r.v, mid, hyst).into_iter().filter(|e| e.rising).map(|e| e.t).collect();
        if rising.is_empty() {
            return;
        }
        let view = (self.m.time_per_div(), self.m.delay);
        let v = r.v.clone();
        let dt = r.dt;
        let n = 8.min(rising.len());
        for _ in 0..n {
            let t = rising[self.sim_i % rising.len()];
            self.sim_i += 1;
            self.ph.draw_ex(0, &self.m, view, -t * dt, dt, &v, 24, None);
        }
        self.m.status = TrigStatus::Stop;
        self.wfm_rate = n as f32 / dt_wall.max(1e-3) as f32;
    }
}

fn nearest_vdiv(want: f32) -> f32 {
    VSCALES.iter().copied().min_by(|a, b| (a - want).abs().partial_cmp(&(b - want).abs()).unwrap()).unwrap_or(0.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn ui() -> Ui {
        Ui::new(Source::Demo { t: 0.0 })
    }

    #[test]
    fn sim_play_loads_the_preset_into_the_slot() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
        u.m.sim_slot = 2;
        u.activate(Item::SimPlay);
        let r = u.refs[2].as_ref().expect("R3");
        assert_eq!(r.label, "PCIe 4.0");
        assert!(r.on);
        assert_eq!(r.v.len(), 65_536);
        assert!((r.dt - 1.0 / (8.0 * 16e9)).abs() < 1e-18);
        assert_eq!(u.m.eye_src, WaveId::Ref(2));
        assert!((u.m.eye_rate - 16e9).abs() < 1.0);
        assert!(u.sim_play);
        let names = serial::names().join(" ");
        assert!(!names.to_ascii_lowercase().contains("hss"));
        assert!(!names.contains("Waveform"));
    }

    #[test]
    fn sim_show_toggles_the_slot() {
        let mut u = ui();
        u.activate(Item::SimPlay);
        assert!(u.refs[0].as_ref().unwrap().on);
        u.adjust(Item::SimShow, 1);
        assert!(!u.refs[0].as_ref().unwrap().on);
    }
}
