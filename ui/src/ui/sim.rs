// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM: load a nominal serial record into a reference slot.
//! Play moves the timebase window through a record that is longer than the window.
//! A record that already fits stays one picture. Live acquisition is unchanged.

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

    /// Writes the selected preset into the selected R slot and starts Play.
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
        self.m.ch[0].on = true;
        self.m.ch[0].vscale = VSCALES.iter().position(|&s| (s - vpd).abs() <= s * 1e-3).unwrap_or(11);
        self.m.ch[0].offset = 0.0;
        // about eight unit intervals on screen
        let want = 8.0 / lane.bits_per_s / HDIV as f64;
        let ts = tscales();
        self.m.tscale = ts.iter().position(|&t| t as f64 >= want).unwrap_or(ts.len() - 1);
        self.m.eye_src = WaveId::Ref(slot);
        self.m.eye_rate = lane.bits_per_s;
        self.m.run = RunState::Stop;
        self.sim_play = true;
        self.sim_tick(0.0);
        self.ph.clear();
        self.measure_refs();
        self.analyzer_update();
        self.toast(&format!("{} → R{}", lane.name, slot + 1));
    }

    /// Moves the timebase window through the selected record.
    ///
    /// The window travels its own width each second, then returns to the start of the record.
    /// A record that already fits is placed fully on screen and held, so the picture stays still.
    pub(super) fn sim_tick(&mut self, dt_wall: f64) {
        let slot = self.m.sim_slot;
        let Some(r) = self.refs[slot].as_ref() else {
            self.sim_play = false;
            return;
        };
        if r.v.is_empty() || !(r.dt > 0.0) {
            self.sim_play = false;
            return;
        }
        let start = r.t0;
        let end = r.t0 + (r.v.len() - 1) as f64 * r.dt;
        let span = end - start;
        let window = self.m.time_per_div() as f64 * HDIV as f64;
        if !(window > 0.0) {
            return;
        }
        if span <= window {
            let left = self.m.delay as f64 - window * 0.5;
            if left > start || left + window < end {
                self.m.delay = ((start + end) * 0.5) as f32;
            }
            return;
        }
        let lo = start + window * 0.5;
        let hi = end - window * 0.5;
        let travel = hi - lo;
        let mut d = self.m.delay as f64;
        // a fraction of the window keeps a rounded delay from snapping back to the start
        let slop = window * 1e-4;
        if d < lo - slop || d > hi + slop {
            d = lo;
        } else {
            d = d.clamp(lo, hi);
        }
        if dt_wall > 0.0 && travel > 0.0 {
            let advance = (window * dt_wall) % travel;
            d = lo + (d - lo + advance) % travel;
        }
        self.m.delay = d as f32;
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
        u.m.run = RunState::Stop;
        u.m.delay = 0.0;
        set_tdiv(u, tdiv);
        u.sim_play = true;
    }

    fn window_of(u: &Ui) -> f64 {
        u.m.time_per_div() as f64 * HDIV as f64
    }

    #[test]
    fn play_walks_a_record_longer_than_the_window() {
        let mut u = ui();
        put_step(&mut u, 5_000, 1e-9, 100e-9);
        let r = u.refs[0].as_ref().unwrap();
        let span = (r.v.len() - 1) as f64 * r.dt;
        let window = window_of(&u);
        assert!(span > window);
        u.step(0.25);
        let lo = window * 0.5;
        let got = u.m.delay as f64;
        assert!((got - (lo + 0.25 * window)).abs() / window < 1e-3, "delay {got} window {window}");
        assert!(u.ph.map.iter().all(|m| m.iter().all(|&p| p == 0)), "play must not paint the live trace");
        let x0 = high_edge(&picture(&mut u));
        u.step(0.25);
        let x1 = high_edge(&picture(&mut u));
        assert!(x1 < x0 - 20, "window moved left from {x0} to {x1}");
    }

    #[test]
    fn play_returns_to_the_start_after_the_record() {
        let mut u = ui();
        put_step(&mut u, 5_000, 1e-9, 100e-9);
        let r = u.refs[0].as_ref().unwrap();
        let end = (r.v.len() - 1) as f64 * r.dt;
        let window = window_of(&u);
        let travel = (end - window * 0.5) - window * 0.5;
        u.step(travel / window + 0.25);
        let got = u.m.delay as f64;
        let expect = window * 0.5 + 0.25 * window;
        assert!((got - expect).abs() / window < 1e-3, "delay {got} expect {expect}");
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
        let first = picture(&mut u);
        u.step(1.0);
        assert_eq!(u.m.delay, delay);
        let second = picture(&mut u);
        assert_eq!(first.px, second.px);
    }

    #[test]
    fn play_walks_the_loaded_preset() {
        let mut u = ui();
        u.activate(Item::SimPlay);
        let r = u.refs[0].as_ref().unwrap();
        let span = (r.v.len() - 1) as f64 * r.dt;
        let window = window_of(&u);
        assert!(span > window, "span {span} window {window}");
        let d0 = u.m.delay;
        u.step(0.5);
        assert!(u.m.delay > d0);
        assert!(u.sim_play);
        assert!(u.ph.map.iter().all(|m| m.iter().all(|&p| p == 0)));
        u.m.run = RunState::Run;
        u.step(0.02);
        assert!(!u.sim_play);
        assert!(u.last[0].is_some());
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
