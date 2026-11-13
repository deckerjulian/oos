// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM: load a nominal serial record into a reference slot and show it at the record's own time/div.
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
            Item::SimPlay => String::new(),
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
        self.sim_load();
        true
    }

    fn sim_lane(&self) -> &'static Lane {
        &LANES[self.m.sim_preset.min(LANES.len() - 1)]
    }

    /// Sample rate and length of the loaded SIM slot, when it is shown.
    pub(super) fn sim_rate_depth(&self) -> Option<(f64, u64)> {
        let r = self.refs[self.m.sim_slot].as_ref()?;
        if !r.on || r.dt <= 0.0 {
            return None;
        }
        Some((1.0 / r.dt, r.v.len() as u64))
    }

    /// Writes the selected preset into the selected R slot and sets the plot to about eight unit intervals.
    fn sim_load(&mut self) {
        let lane = self.sim_lane();
        let v = serial::generate(lane);
        let n = v.len();
        let dt = serial::dt_of(lane);
        let span = (lane.v_high - lane.v_low).abs().max(0.2);
        let vpd = nearest_vdiv((span / 4.0) as f32);
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
        // a stored record, not the live analog path
        for c in 0..NCH {
            self.m.ch[c].on = false;
        }
        // about eight unit intervals on screen, using display scales below 1 ns/div when the record needs them
        let want = 8.0 / lane.bits_per_s / HDIV as f64;
        let ts = tscales();
        self.m.tscale = ts.iter().copied().enumerate().min_by(|(_, a), (_, b)| (*a as f64 - want).abs().partial_cmp(&(*b as f64 - want).abs()).unwrap()).map(|(i, _)| i).unwrap_or(0);
        self.m.tfine = 1.0;
        // skip the long run of 1s at the start of an all-1s LFSR seed
        self.m.delay = (n as f64 * dt * 0.15) as f32;
        self.m.eye_src = WaveId::Ref(slot);
        self.m.eye_rate = lane.bits_per_s;
        self.m.run = RunState::Stop;
        self.sim_i = 0;
        self.sim_play = false;
        self.ph.clear();
        self.measure_refs();
        self.analyzer_update();
        self.toast(&format!("{} → R{}", lane.name, slot + 1));
    }

    pub(super) fn sim_tick(&mut self, _dt_wall: f64) {}
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
        assert!(!u.sim_play);
        assert!(!u.m.ch.iter().any(|c| c.on), "SIM is a reference, not the analog channels");
        assert!((u.m.time_per_div() as f64 - 50e-12).abs() < 1e-12, "tdiv {}", u.m.time_per_div());
        u.render(&mut Canvas::new(W, H));
        let names = serial::names().join(" ");
        assert!(!names.to_ascii_lowercase().contains("hss"));
        assert!(!names.contains("Waveform"));
    }

    #[test]
    fn sim_usb32_gen2_uses_the_record_timebase() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "USB 3.2 Gen 2").unwrap();
        u.activate(Item::SimPlay);
        let r = u.refs[0].as_ref().expect("R1");
        assert_eq!(r.label, "USB 3.2 Gen 2");
        assert!((r.dt - 1.0 / (8.0 * 10e9)).abs() < 1e-18);
        // 8 UI at 10 Gb/s is 80 ps/div; nearest 1-2-5 step is 100 ps/div
        assert!((u.m.time_per_div() as f64 - 100e-12).abs() < 1e-12, "tdiv {}", u.m.time_per_div());
        let (sr, n) = u.sim_rate_depth().expect("rate");
        assert!((sr - 80e9).abs() < 1.0, "sr {}", sr);
        assert_eq!(n, 65_536);
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/sim-usb32-gen2-loaded.png", dir), crate::store::png_of(&cv));
        }
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
