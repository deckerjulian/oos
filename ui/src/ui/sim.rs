// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM: load a nominal serial record into a reference slot and show it at the record's own time/div.
//! Live analog at 16 Gb/s is not the path; this is the offline record. The knobs override the preset;
//! Play writes the current knobs into the R slot.

use super::*;
use oos_dsp::serial::{self, Lane, LANES};

const PRBS: [u8; 5] = [7, 9, 15, 23, 31];

impl Ui {
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
            Item::SimRj,
            Item::SimPj,
            Item::SimPjHz,
            Item::SimSlot,
            Item::SimPlay,
            Item::SimShow,
            Item::SimDefaults,
        ]
    }

    pub(super) fn sim_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::SimPlay | Item::SimDefaults => Kind::Action,
            Item::SimPreset | Item::SimPat | Item::SimSsc | Item::SimSlot | Item::SimShow => Kind::Choice,
            Item::SimRate | Item::SimHigh | Item::SimLow | Item::SimRise | Item::SimBw | Item::SimDeemph | Item::SimNoise | Item::SimRj | Item::SimPj | Item::SimPjHz => Kind::Value,
            _ => return None,
        })
    }

    pub(super) fn sim_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::Sim) => "SIM",
            Item::SimPreset => "Preset",
            Item::SimRate => "Rate",
            Item::SimPat => "Pattern",
            Item::SimHigh => "High",
            Item::SimLow => "Low",
            Item::SimRise => "Rise",
            Item::SimBw => "Bandwidth",
            Item::SimDeemph => "De-emph",
            Item::SimNoise => "Noise",
            Item::SimSsc => "Spread",
            Item::SimRj => "RJ",
            Item::SimPj => "PJ",
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
        true
    }

    pub(super) fn sim_activate(&mut self, it: Item) -> bool {
        match it {
            Item::SimPlay => self.sim_load(),
            Item::SimDefaults => {
                self.m.apply_sim_preset();
                self.toast("SIM knobs from the preset");
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
            rj_rms: self.m.sim_rj.max(0.0),
            pj_peak: self.m.sim_pj.max(0.0),
            pj_hz: self.m.sim_pjhz.max(0.0),
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

    /// Writes the current knobs into the selected R slot. Overview is the stored record; zoom is ~eight UI.
    fn sim_load(&mut self) {
        let lane = self.sim_lane();
        let v = serial::generate(&lane);
        let n = v.len();
        let dt = serial::dt_of(&lane);
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
        let rec_s = n as f64 * dt;
        let bit = nearest_tscale(8.0 / lane.bits_per_s / HDIV as f64);
        let overview = nearest_tscale(rec_s / HDIV as f64);
        self.m.tfine = 1.0;
        self.m.tscale = overview.max(bit);
        let look = (rec_s * 0.15) as f32;
        self.m.delay = look;
        self.set_zoom(true);
        self.m.zscale = bit;
        self.m.zdelay = look;
        self.clamp_zoom();
        self.m.meas = Self::default_meas().into_iter().map(|(k, _)| (k, WaveId::Ref(slot))).collect();
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
    fn sim_play_loads_the_preset_into_the_slot() {
        let mut u = ui();
        u.m.sim_preset = LANES.iter().position(|l| l.name == "PCIe 4.0").unwrap();
        u.m.apply_sim_preset();
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
        assert!(u.m.zoom);
        assert!((u.m.time_per_div() as f64 - 50e-9).abs() < 1e-12, "overview tdiv {}", u.m.time_per_div());
        assert!((tscales()[u.m.zscale] as f64 - 50e-12).abs() < 1e-12, "zoom {}", tscales()[u.m.zscale]);
        assert!(u.m.meas.iter().all(|(_, id)| *id == WaveId::Ref(2)));
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
        let r = u.refs[0].as_ref().expect("R1");
        assert_eq!(r.label, "USB 3.2 Gen 2");
        assert!((r.dt - 1.0 / (8.0 * 10e9)).abs() < 1e-18);
        assert!((u.m.time_per_div() as f64 - 100e-9).abs() < 1e-12, "overview tdiv {}", u.m.time_per_div());
        assert!((tscales()[u.m.zscale] as f64 - 100e-12).abs() < 1e-12, "zoom {}", tscales()[u.m.zscale]);
        let (sr, n) = u.sim_rate_depth().expect("rate");
        assert!((sr - 80e9).abs() < 1.0, "sr {}", sr);
        assert_eq!(n, 65_536);
        let vpp = u.meas_rows().into_iter().find(|r| r.name == "Pk-pk").expect("pk-pk").v;
        assert!(vpp > 0.5 && vpp < 1.2, "vpp {}", vpp);
        let mut cv = Canvas::new(W, H);
        u.render(&mut cv);
        if let Ok(dir) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/sim-usb32-gen2-loaded.png", dir), crate::store::png_of(&cv));
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
        let r = u.refs[0].as_ref().unwrap();
        assert!((r.dt - 1.0 / (8.0 * 5e9)).abs() < 1e-18);
        u.activate(Item::SimDefaults);
        assert!((u.m.sim_rate - 10e9).abs() < 1.0);
        assert_eq!(u.m.sim_prbs, 15);
        assert!(!u.m.sim_ssc);
        u.open_menu(MenuId::Sim);
        let labels: Vec<_> = u.sim_items().iter().filter_map(|&it| Ui::sim_label(it)).collect();
        for want in ["Rate", "Pattern", "High", "Low", "Rise", "Bandwidth", "De-emph", "Noise", "Spread", "RJ", "PJ"] {
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
    fn sim_show_toggles_the_slot() {
        let mut u = ui();
        u.activate(Item::SimPlay);
        assert!(u.refs[0].as_ref().unwrap().on);
        u.adjust(Item::SimShow, 1);
        assert!(!u.refs[0].as_ref().unwrap().on);
    }
}
