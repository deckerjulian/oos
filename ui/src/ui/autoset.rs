// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Auto set-up on the hardware, for the channels that are on. A record only shows what the scale and time base let
//! through, so this one acquires: a wide scale for the levels in Peak mode, then a scale that fits, then time bases from
//! fast to slow until each channel shows a steady period with most of its height (fast first: a coarse time base can
//! fold a fast signal into a period that is not there). A channel without a period keeps its settings, one that is off stays off. With no signal on any
//! channel the set-up stays as it was.

use super::*;
use crate::wave::{periodic, place, Stats};

/// Time bases of the search: one window of 12 divisions each covers about two decades of period.
const TBASES: [f32; 5] = [20e-9, 2e-6, 200e-6, 2e-3, 20e-3];
/// Time base of the levels: long enough for a whole period of a slow signal. Full bandwidth, because Auto closes the
/// filter at slow time bases and a fast signal would look small.
const LEVELS_TBASE: f32 = 20e-3;
/// Less than this is no signal (V peak to peak): an open input reads about 20 mV with the filter open.
const MIN_VPP: f32 = 0.03;
/// Records and seconds after a change before a record counts, and when to go on without one.
const SETTLE_RECORDS: u64 = 3;
const SETTLE_S: f64 = 0.3;
const GIVE_UP_S: f64 = 3.0;

enum Stage {
    Wide,
    Fit,
    Search(usize),
}

pub struct AutoRun {
    back: Box<Model>,
    stage: Stage,
    records: u64,
    since: f64,
    /// a record of the time base set now has come
    fits: bool,
    /// levels and period per channel; `want` = a signal worth a period
    lv: [(f32, f32); NCH],
    want: [bool; NCH],
    period: [Option<f64>; NCH],
    /// the height of the levels stage, and of the window that gave the period
    full: [f32; NCH],
    swing: [f32; NCH],
}

/// The smallest scale of `ch` with at least `vdiv` volts per division.
fn scale_for(ch: &Channel, vdiv: f32) -> usize {
    VSCALES.iter().position(|&x| x * ch.probe >= vdiv).unwrap_or(VSCALES.len() - 1)
}

fn tscale_for(t: f32) -> usize {
    let ts = tscales();
    ts.iter().position(|&x| x >= t * 0.999).unwrap_or(ts.len() - 1)
}

impl Ui {
    /// An auto set-up is under way.
    pub fn auto_running(&self) -> bool {
        self.auto.is_some()
    }

    /// AUTO key on the hardware: starts the run, `auto_step` goes on from the step loop.
    pub(super) fn auto_start(&mut self) {
        let Some(first) = (0..n_ch()).find(|&c| self.m.ch[c].on) else {
            return self.toast("Auto set-up: no channel on");
        };
        let back = Box::new(self.m.clone());
        let m = &mut self.m;
        for c in (0..n_ch()).filter(|&c| back.ch[c].on) {
            let ch = &mut m.ch[c];
            ch.coupling = Coupling::Dc;
            ch.offset = 0.0;
            ch.vfine = 1.0;
            ch.vscale = scale_for(ch, 1.0 * ch.probe);
            ch.bw = BW_FULL;
        }
        m.trig_type = TrigType::Edge;
        m.trig_la = None;
        m.trig_mode = TrigMode::Auto;
        m.trig_src = first;
        m.tscale = tscale_for(LEVELS_TBASE);
        // a fixed deep memory would make every step of the search take seconds, and an auto set-up has no use for it
        m.mdepth = 0;
        // at this slow rate a fast signal can land on the same phase every sample and look flat: Peak sees every sample
        m.acq = AcqMode::Peak;
        m.tfine = 1.0;
        m.delay = 0.0;
        m.run = RunState::Run;
        self.auto = Some(AutoRun { back, stage: Stage::Wide, records: self.records, since: self.now, fits: false, lv: [(0.0, 0.0); NCH], want: [false; NCH], period: [None; NCH], full: [0.0; NCH], swing: [0.0; NCH] });
        self.mark_changed();
        self.toast("Auto set-up...");
    }

    /// The last record spans the screen at the time base set now.
    fn auto_record_fits(&self) -> bool {
        let want = HDIV as f64 * tscales()[self.m.tscale] as f64;
        self.last[0].as_ref().is_some_and(|r| {
            let n = r.ch.iter().map(|c| c.len()).max().unwrap_or(0);
            n > 0 && (r.dt * n as f64 / want - 1.0).abs() < 0.25
        })
    }

    /// Lowest and highest volts of the last record.
    fn auto_levels(&self, c: usize) -> (f32, f32) {
        let v = self.record_volts(c);
        (v.iter().copied().fold(f32::MAX, f32::min).min(f32::MAX), v.iter().copied().fold(f32::MIN, f32::max))
    }

    pub(super) fn auto_step(&mut self) {
        let Some(mut a) = self.auto.take() else { return };
        // records of the time base before can still be under way, many of them at a high trigger rate: count from the
        // first one of this one
        if !a.fits && self.auto_record_fits() {
            (a.fits, a.records) = (true, self.records);
        }
        let ready = a.fits && self.records >= a.records + SETTLE_RECORDS && self.now >= a.since + SETTLE_S;
        if !ready && self.now < a.since + GIVE_UP_S {
            self.auto = Some(a);
            return;
        }
        match a.stage {
            Stage::Wide => {
                for c in (0..n_ch()).filter(|&c| a.back.ch[c].on) {
                    let (lo, hi) = self.auto_levels(c);
                    let ch = &mut self.m.ch[c];
                    let vpd = ch.volts_per_div();
                    if hi.max(-lo) >= 3.9 * vpd {
                        ch.vscale = scale_for(ch, 5.0 * ch.probe);
                    } else if hi - lo < 0.4 * vpd {
                        ch.vscale = scale_for(ch, 0.1 * ch.probe);
                    }
                }
                a.stage = Stage::Fit;
            }
            Stage::Fit => {
                for c in (0..n_ch()).filter(|&c| a.back.ch[c].on) {
                    let (lo, hi) = self.auto_levels(c);
                    a.lv[c] = (lo, hi);
                    let vpp = hi - lo;
                    a.full[c] = vpp;
                    a.want[c] = vpp.is_finite() && vpp >= MIN_VPP * self.m.ch[c].probe && vpp >= 0.3 * self.m.ch[c].volts_per_div();
                }
                let Some(first) = (0..n_ch()).find(|&c| a.want[c]) else { return self.auto_end(a) };
                for c in (0..n_ch()).filter(|&c| a.want[c]) {
                    let ch = &mut self.m.ch[c];
                    let (lo, hi) = a.lv[c];
                    ch.vscale = scale_for(ch, (hi - lo) / 4.0);
                    ch.offset = -(hi + lo) / 2.0;
                }
                self.m.trig_src = first;
                self.m.trig_level = (a.lv[first].0 + a.lv[first].1) / 2.0;
                self.m.tscale = tscale_for(TBASES[0]);
                self.m.acq = a.back.acq;
                a.stage = Stage::Search(0);
            }
            Stage::Search(i) => {
                let dt = self.last[0].as_ref().map_or(0.0, |r| r.dt);
                for c in 0..n_ch() {
                    if dt == 0.0 || !a.want[c] {
                        continue;
                    }
                    // a slow signal is flat here, and what rides on it looks regular: a window counts with most of the
                    // height, and one with clearly more height beats it. Not all of it: hum at a slow time base can
                    // stand higher than a weak fast signal
                    let (lo, hi) = self.auto_levels(c);
                    let swing = hi - lo;
                    if swing >= 0.4 * a.full[c] && swing >= 1.5 * a.swing[c] {
                        if let Some(p) = periodic(&self.record_volts(c), dt) {
                            (a.period[c], a.swing[c], a.lv[c]) = (Some(p), swing, (lo, hi));
                        }
                    }
                }
                // done at the last time base, or when each channel has its period with nearly the full height
                if i + 1 == TBASES.len() || (0..n_ch()).all(|c| !a.want[c] || (a.period[c].is_some() && a.swing[c] >= 0.7 * a.full[c])) {
                    return self.auto_end(a);
                }
                self.m.tscale = tscale_for(TBASES[i + 1]);
                a.stage = Stage::Search(i + 1);
            }
        }
        (a.records, a.since, a.fits) = (self.records, self.now, false);
        self.auto = Some(a);
        self.mark_changed();
    }

    fn auto_end(&mut self, a: AutoRun) {
        eprintln!("oos-ui auto: levels {:?}, periods {:?}", a.lv, a.period);
        let found: Vec<(usize, Stats, f64)> = (0..n_ch())
            .filter_map(|c| a.period[c].map(|p| (c, Stats { vmin: a.lv[c].0, vmax: a.lv[c].1, ..Stats::default() }, p)))
            .collect();
        if found.is_empty() {
            self.m = *a.back;
            self.toast("Auto set-up: no signal found");
        } else {
            // the others as they were: on, with their scale
            for c in (0..n_ch()).filter(|&c| a.period[c].is_none()) {
                self.m.ch[c] = a.back.ch[c].clone();
            }
            self.m.acq = a.back.acq;
            place(&mut self.m, &found);
            self.toast("Auto set-up");
        }
        self.dirty_map = true;
        self.mark_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ui: &mut Ui) {
        ui.auto_start();
        for _ in 0..4000 {
            ui.step(0.02);
            if ui.auto.is_none() {
                return;
            }
        }
        panic!("auto set-up never ended");
    }

    #[test]
    fn looks_only_at_the_channels_that_are_on() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for c in 0..NCH {
            ui.m.ch[c].on = c == 0;
        }
        ui.m.trig_src = 1;
        run(&mut ui);
        assert_eq!((0..NCH).filter(|&c| ui.m.ch[c].on).collect::<Vec<_>>(), [0]);
        assert_eq!(ui.m.trig_src, 0);
        assert_eq!(ui.m.ch[0].bw, BW_FULL);
        assert_eq!(ui.m.run, RunState::Run);
    }

    #[test]
    fn with_no_channel_on_nothing_happens() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for c in 0..NCH {
            ui.m.ch[c].on = false;
        }
        let m = ui.m.clone();
        ui.auto_start();
        assert!(ui.auto.is_none() && ui.m == m);
    }
}
