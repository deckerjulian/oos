// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Acquisition: run state, trigger status, acquisition mode, the time base the ADC runs at, and the sample rate and
//! memory depth the board allows. A new acquisition setting or limit goes here.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    Run,
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigStatus {
    Triggered,
    Auto,
    Wait,
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcqMode {
    Normal,
    Peak,
    Average,
    HiRes,
}

impl Model {
    pub fn time_per_div(&self) -> f32 {
        tscales()[self.tscale] * self.tfine
    }

    /// The time base the acquisition runs at: the step, which covers at least the screen. Display scales below
    /// 1 ns/div stay on the plot; the ADC still takes a 1 ns/div window.
    pub fn hw_time_per_div(&self) -> f32 {
        tscales()[self.tscale].max(HW_MIN_TDIV)
    }

    /// Live analog / LA cannot display below 1 ns/div; the ADC window is that floor.
    pub fn floor_live_tdiv(&mut self) {
        if tscales()[self.tscale] < HW_MIN_TDIV {
            self.tscale = tscales().iter().position(|&t| t >= HW_MIN_TDIV).unwrap_or(0);
            self.tfine = 1.0;
        }
    }

    /// Maximum sample rate for the number of enabled channels (1.25 GSa/s shared by the ADC cores; platform limits,
    /// `oos_hal::model`).
    pub fn max_srate(&self) -> f64 {
        let r = oos_hal::model::platform().limits.srate(self.n_on());
        // the analog rate is halved while the LA is on and a channel is shown (la.md 3.1)
        if self.la_on() && self.n_on() > 0 {
            r / 2.0
        } else {
            r
        }
    }

    pub fn la_on(&self) -> bool {
        self.la.on && crate::model::la::available()
    }

    pub fn max_mem(&self) -> u64 {
        // limits 125 / 62.5 / 31.25 M (stock: 50 / 25 / 10 M); above 78.6 M only with the larger segment cap
        // (`hal::dho::hz::MEM_BURSTS`); overridable in limits.conf (`mem.1ch` ...)
        // with the LA on: 31.25 / 12.5 / 10 M (`max_mem_la`, la.md 3.2)
        oos_hal::model::platform().limits.mem(self.n_on(), self.la_on())
    }
    /// (sample rate, memory depth) as the acquisition would use them.
    pub fn srate_depth(&self) -> (f64, u64) {
        let window = self.hw_time_per_div() as f64 * HDIV as f64;
        let want = mdepths()[self.mdepth].1;
        let maxsr = self.max_srate();
        if want == 0 {
            let depth_cap = 1_000_000u64.min(self.max_mem());
            let sr = maxsr.min(depth_cap as f64 / window);
            let sr = quantize_srate(sr, maxsr);
            (sr, ((sr * window).round() as u64).max(1000).min(depth_cap))
        } else {
            let depth = want.min(self.max_mem());
            let sr = quantize_srate(maxsr.min(depth as f64 / window), maxsr);
            (sr, depth)
        }
    }
}

/// Sample rates are the ADC rate divided by an integer decimation (1-2-5 steps here).
fn quantize_srate(sr: f64, max: f64) -> f64 {
    let mut best = max;
    let mut d = 1.0;
    while best > sr * 1.0001 && d < 1e12 {
        for m in [1.0, 2.0, 5.0] {
            let c = max / (d * m);
            if c <= sr * 1.0001 {
                return c;
            }
        }
        d *= 10.0;
        best = max / d;
    }
    best
}
