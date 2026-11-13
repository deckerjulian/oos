// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Auto set-up on the hardware, for the channels that are on. A record only shows what the scale and time base let
//! through, so this one acquires: a wide scale for the levels in Peak mode, then a scale that fits, then time bases from
//! fast to slow until each channel shows a steady period with most of its height (fast first: a coarse time base can
//! fold a fast signal into a period that is not there). A channel with a signal but no steady period keeps the fitted
//! scale and level, at the time base that showed most of its height; one without a signal keeps its settings, one that
//! is off stays off. With no signal on any channel the set-up stays as it was.

use super::*;
use crate::wave::{flat_noise, mid_level, periodic, place, Stats};

/// Time bases of the search: one window of 12 divisions each covers about two decades of period.
const TBASES: [f32; 5] = [20e-9, 2e-6, 200e-6, 2e-3, 20e-3];
/// Hardware source: three records at most instead of five. 2 us/div at the full rate holds periods up to 8 us (125 kHz
/// and faster), and rules out what a slower rate would fold. Then one deep record (1 M points at 2 ms/div, 41.7 MSa/s)
/// holds periods from 0.2 us to 8 ms: 125 Hz to 125 kHz, what the first one did not show. 20 ms/div only for a signal
/// it shows fewer than three rises of. Depth per record (0 = Auto).
const FAST_TBASE: f32 = 2e-6;
const DEEP: (f32, u64) = (2e-3, 1_000_000);
const SLOW_TBASE: f32 = 20e-3;
const HW_PLAN: [(f32, u64); 3] = [(FAST_TBASE, 0), DEEP, (SLOW_TBASE, 0)];
/// The deep record with more than one channel on: 100 k points (4.2 MSa/s at 2 ms/div, still 33 samples a period at
/// 125 kHz). 1 M points of 3 or 4 channels is an 8 MB read in one piece; with it on the unit, 2026-11-13, the scope
/// rebooted twice (docs/fork/trigger-lockin.md).
const DEEP_MULTI: u64 = 100_000;
/// Time base of the levels: long enough for a whole period of a slow signal. Full bandwidth, because Auto closes the
/// filter at slow time bases and a fast signal would look small.
const LEVELS_TBASE: f32 = 20e-3;
/// Less than this is no signal (V peak to peak): an open input reads about 20 mV with the filter open.
const MIN_VPP: f32 = 0.03;
/// Records without a settings stamp (demo, replay): records and seconds after a change before one counts. Every stage
/// goes on without one after `GIVE_UP_S`.
const SETTLE_RECORDS: u64 = 3;
const SETTLE_S: f64 = 0.3;
const GIVE_UP_S: f64 = 3.0;
/// Measure-unit levels (hardware source): runs of 1.7 ms per request (about 40 ms of signal: the full swing of
/// signals from about 25 Hz), the wait for an answer before the records take over, and the requests at most (the
/// wide scale, then one other scale).
const UNIT_RUNS: u32 = 24;
const UNIT_GIVE_UP_S: f64 = 0.5;
const UNIT_TRIES: u32 = 2;

/// The auto set-up reads the levels with the FPGA's measure unit where the source has one, unless system.conf says
/// `autoset.levels = records` (or OOS_AUTOSET_LEVELS=records).
fn unit_levels_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let env = std::env::var("OOS_AUTOSET_LEVELS").ok();
        let conf = std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default();
        crate::drawpipe::conf_value(env.as_deref(), &conf, "autoset.levels").as_deref() != Some("records")
    })
}

/// What the measure-unit step did (`Ui::auto_unit`).
enum UnitStep {
    /// waits for the answer, or asked again at another scale
    Wait,
    /// its levels gave the fit: on to the search
    Fitted,
    /// no channel worth a period
    NoSignal,
    /// the records take over: no answer in time, a channel without a usable reading, or still out of range after the
    /// last try
    Records,
}

/// A request for measure-unit levels under way: its number (the stage's settings stamp), how many were made, since
/// when it waits.
#[derive(Clone, Copy, Debug)]
struct Unit {
    id: u64,
    tries: u32,
    since: f64,
}

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
    /// the statistics of the record behind `lv` (top and base for the trigger level)
    st: [Stats; NCH],
    want: [bool; NCH],
    period: [Option<f64>; NCH],
    /// the height of the levels stage, and of the window that gave the period
    full: [f32; NCH],
    swing: [f32; NCH],
    /// per channel: the height and time base (index) of the search window that showed most of it
    best: [Option<(f32, usize)>; NCH],
    /// `Model::hw_stamp` of the current stage: hardware records with an older one are from before it
    stamp: u64,
    /// the time bases of the search and their memory depths (points, 0 = Auto); `deep` = the hardware plan
    plan: Vec<(f32, u64)>,
    deep: bool,
    /// the levels come from the measure unit (hardware source) until it answers, fails or takes too long
    unit: Option<Unit>,
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
        self.auto_bar = None;
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
        // the levels need no trigger, and the old level may not cross the signal at the wide scale: without this each
        // record waits the auto time of 20 ms/div (400 ms)
        m.auto_fast = true;
        m.trig_src = first;
        m.tscale = tscale_for(LEVELS_TBASE);
        // a fixed deep memory would make every step of the search take seconds, and an auto set-up has no use for it
        m.mdepth = 0;
        // at this slow rate a fast signal can land on the same phase every sample and look flat: Peak sees every sample
        m.acq = AcqMode::Peak;
        m.tfine = 1.0;
        m.delay = 0.0;
        m.run = RunState::Run;
        m.hw_stamp += 1;
        let stamp = m.hw_stamp;
        // one channel only: then all eight cores sample it. With more, the slots of a channel are not known (the DDR core
        // order did not match on the unit: with CH1 and CH2 on a signal, CH3's slots showed it)
        let single = (0..n_ch()).filter(|&c| back.ch[c].on).count() == 1;
        let unit = (single && unit_levels_on() && self.src.hw_levels(Some((stamp, stamp, UNIT_RUNS))).is_some()).then_some(Unit { id: stamp, tries: 1, since: self.now });
        self.auto = Some(AutoRun { back, stamp, unit, plan: Vec::new(), deep: false, best: [None; NCH], stage: Stage::Wide, records: self.records, since: self.now, fits: false, lv: [(0.0, 0.0); NCH], st: [Stats::default(); NCH], want: [false; NCH], period: [None; NCH], full: [0.0; NCH], swing: [0.0; NCH] });
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
        if a.unit.is_some() {
            match self.auto_unit(&mut a) {
                UnitStep::Wait => {
                    self.auto = Some(a);
                    return;
                }
                UnitStep::Fitted => return self.auto_next(a),
                UnitStep::NoSignal => return self.auto_end(a),
                // the stage is Wide or, after a rescale, Fit
                UnitStep::Records => {}
            }
        }
        // records of the time base before can still be under way, many of them at a high trigger rate: count from the
        // first one of this one
        if !a.fits && self.auto_record_fits() {
            (a.fits, a.records) = (true, self.records);
        }
        // hardware records carry the settings stamp they were taken under: the first one of this stage's settings is
        // enough. Records without one (demo, replay) settle on a count and a time
        let fresh = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.stamp >= a.stamp);
        let ready = match fresh {
            Some(f) => f && self.auto_record_fits(),
            None => a.fits && self.records >= a.records + SETTLE_RECORDS && self.now >= a.since + SETTLE_S,
        };
        if !ready && self.now < a.since + GIVE_UP_S {
            self.auto = Some(a);
            return;
        }
        match a.stage {
            Stage::Wide => {
                // a channel that clips or uses less than 0.4 div gets another scale and a second look (Fit); when
                // every channel reads well at the wide scale, its record gives the fit at once
                let mut again = false;
                for c in (0..n_ch()).filter(|&c| a.back.ch[c].on) {
                    let (lo, hi) = self.auto_levels(c);
                    let ch = &mut self.m.ch[c];
                    let vpd = ch.volts_per_div();
                    if hi.max(-lo) >= 3.9 * vpd {
                        ch.vscale = scale_for(ch, 5.0 * ch.probe);
                        again = true;
                    } else if hi - lo < 0.4 * vpd {
                        ch.vscale = scale_for(ch, 0.1 * ch.probe);
                        again = true;
                    }
                }
                if again {
                    a.stage = Stage::Fit;
                } else if !self.auto_fit(&mut a, None) {
                    return self.auto_end(a);
                }
            }
            Stage::Fit => {
                if !self.auto_fit(&mut a, None) {
                    return self.auto_end(a);
                }
            }
            Stage::Search(i) => {
                let dt = self.last[0].as_ref().map_or(0.0, |r| r.dt);
                // the samples of the acquisition where the source has them (all of a deep record), else the record
                let raw = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.clone());
                let mut slow = false;
                for c in 0..n_ch() {
                    if dt == 0.0 || !a.want[c] {
                        continue;
                    }
                    let (v, vdt) = match &raw {
                        Some(f) if f.ch[c].len() > 1 && !f.envelope => (f.ch[c].clone(), f.dt),
                        _ => (self.record_volts(c), dt),
                    };
                    // a slow signal is flat here, and what rides on it looks regular: a window counts with most of the
                    // height, and one with clearly more height beats it. Not all of it: hum at a slow time base can
                    // stand higher than a weak fast signal
                    let (lo, hi) = self.auto_levels(c);
                    let swing = hi - lo;
                    if a.best[c].is_none_or(|b| swing > b.0) {
                        a.best[c] = Some((swing, self.m.tscale));
                    }
                    let found = swing >= 0.4 * a.full[c] && swing >= 1.5 * a.swing[c] && {
                        match periodic(&v, vdt) {
                            Some(p) => {
                                (a.period[c], a.swing[c], a.lv[c]) = (Some(p), swing, (lo, hi));
                                a.st[c] = crate::wave::stats(&v, vdt);
                                true
                            }
                            None => false,
                        }
                    };
                    // the deep record missed it: fewer than three rises (or less than its height) is a slower signal;
                    // more without a steady period an irregular one, which no slower record times either
                    if a.deep && i == 1 && !found && a.period[c].is_none() {
                        let rises = oos_dsp::edges::edges(&v, (lo + hi) / 2.0, 0.4 * swing).iter().filter(|e| e.rising).count();
                        slow |= rises < 3 || swing < 0.7 * a.full[c];
                    }
                }
                if a.deep && i == 1 && !slow {
                    a.plan.truncate(2);
                }
                // done at the last time base, or when each channel has its period with nearly the full height
                if i + 1 >= a.plan.len() || (0..n_ch()).all(|c| !a.want[c] || (a.period[c].is_some() && a.swing[c] >= 0.7 * a.full[c])) {
                    return self.auto_end(a);
                }
                self.set_search(&a.plan, i + 1);
                a.stage = Stage::Search(i + 1);
            }
        }
        self.auto_next(a)
    }

    /// Time base and depth of search record `i`.
    fn set_search(&mut self, plan: &[(f32, u64)], i: usize) {
        self.m.tscale = tscale_for(plan[i].0);
        self.m.mdepth = mdepths().iter().position(|d| d.1 == plan[i].1).unwrap_or(0);
    }

    /// A new stage: records taken from here on carry a new stamp.
    fn auto_next(&mut self, mut a: AutoRun) {
        (a.records, a.since, a.fits) = (self.records, self.now, false);
        self.m.hw_stamp += 1;
        a.stamp = self.m.hw_stamp;
        self.auto = Some(a);
        self.mark_changed();
    }

    /// The measure-unit step (`a.unit`), levels in volts as the records show them (hal levels.rs).
    fn auto_unit(&mut self, a: &mut AutoRun) -> UnitStep {
        let Some(u) = a.unit else { return UnitStep::Records };
        let ans = self.src.hw_levels(Some((u.id, a.stamp, UNIT_RUNS))).flatten().filter(|r| r.0 == u.id);
        let Some((_, lv)) = ans else {
            if self.now < u.since + UNIT_GIVE_UP_S {
                return UnitStep::Wait;
            }
            eprintln!("oos-ui auto: no measure-unit answer in {} s, records", UNIT_GIVE_UP_S);
            return self.auto_unit_off(a);
        };
        let on: Vec<usize> = (0..n_ch()).filter(|&c| a.back.ch[c].on).collect();
        if on.iter().any(|&c| lv[c].is_none()) {
            eprintln!("oos-ui auto: measure unit {:?}, records", lv);
            return self.auto_unit_off(a);
        }
        // as the Wide stage: clipping or less than 0.4 div asks again at another scale
        let mut again = false;
        for &c in &on {
            let l = lv[c].unwrap_or_default();
            let ch = &mut self.m.ch[c];
            let vpd = ch.volts_per_div();
            let small = scale_for(ch, 0.1 * ch.probe);
            if l.clipped || l.hi.max(-l.lo) >= 3.9 * vpd {
                ch.vscale = scale_for(ch, 5.0 * ch.probe);
                again = true;
            } else if l.hi - l.lo < 0.4 * vpd && ch.vscale != small {
                ch.vscale = small;
                again = true;
            }
        }
        if again {
            self.m.hw_stamp += 1;
            a.stamp = self.m.hw_stamp;
            self.mark_changed();
            if u.tries >= UNIT_TRIES {
                // the records look at the new scale
                a.stage = Stage::Fit;
                return self.auto_unit_off(a);
            }
            a.unit = Some(Unit { id: a.stamp, tries: u.tries + 1, since: self.now });
            let _ = self.src.hw_levels(Some((a.stamp, a.stamp, UNIT_RUNS)));
            return UnitStep::Wait;
        }
        let levels: [(f32, f32); NCH] = std::array::from_fn(|c| lv.get(c).copied().flatten().map_or((0.0, 0.0), |l| (l.lo, l.hi)));
        eprintln!("oos-ui auto: measure-unit levels {:?}", levels);
        a.unit = None;
        let _ = self.src.hw_levels(None);
        if self.auto_fit(a, Some(levels)) {
            UnitStep::Fitted
        } else {
            UnitStep::NoSignal
        }
    }

    /// The records take over from the measure unit: counted from now, on the stage set.
    fn auto_unit_off(&mut self, a: &mut AutoRun) -> UnitStep {
        a.unit = None;
        let _ = self.src.hw_levels(None);
        (a.records, a.since, a.fits) = (self.records, self.now, false);
        UnitStep::Records
    }

    /// Opt-in (`Model::auto_nrej`): noise reject on when the noise on the trigger signal's two levels, about 6 sigma
    /// peak to peak, is more than the plain hysteresis of the final scale (0.3 div), else off. A signal without two
    /// clear levels leaves it as it is.
    fn auto_noise_reject(&mut self) {
        let c = self.m.trig_src;
        if let Some(sigma) = flat_noise(&self.record_volts(c)) {
            self.m.trig_nrej = 6.0 * sigma > 0.3 * self.m.ch[c].volts_per_div();
        }
    }

    /// Scale, offset and trigger level of every channel worth a period, from `levels` (the measure unit) or else the
    /// last record; then the search. False when no channel is.
    fn auto_fit(&mut self, a: &mut AutoRun, levels: Option<[(f32, f32); NCH]>) -> bool {
        for c in (0..n_ch()).filter(|&c| a.back.ch[c].on) {
            let (lo, hi) = levels.map_or_else(|| self.auto_levels(c), |l| l[c]);
            a.lv[c] = (lo, hi);
            // the extremes only from the unit: the trigger level is their middle until a record of the search has top
            // and base (`place`)
            a.st[c] = if levels.is_some() { Stats { vmin: lo, vmax: hi, valid: true, ..Stats::default() } } else { crate::wave::stats(&self.record_volts(c), self.last[0].as_ref().map_or(1.0, |r| r.dt)) };
            let vpp = hi - lo;
            a.full[c] = vpp;
            a.want[c] = vpp.is_finite() && vpp >= MIN_VPP * self.m.ch[c].probe && vpp >= 0.3 * self.m.ch[c].volts_per_div();
        }
        let Some(first) = (0..n_ch()).find(|&c| a.want[c]) else { return false };
        for c in (0..n_ch()).filter(|&c| a.want[c]) {
            let ch = &mut self.m.ch[c];
            let (lo, hi) = a.lv[c];
            ch.vscale = scale_for(ch, (hi - lo) / 4.0);
            ch.offset = -(hi + lo) / 2.0;
        }
        self.m.trig_src = first;
        self.m.trig_level = mid_level(&a.st[first]);
        // the hardware search starts with one deep record; other sources go through the time bases from fast to slow
        a.deep = self.src.is_hw();
        a.plan = if a.deep { HW_PLAN.to_vec() } else { TBASES.iter().map(|&t| (t, 0)).collect() };
        if a.deep && (0..n_ch()).filter(|&c| a.back.ch[c].on).count() > 1 {
            a.plan[1].1 = DEEP_MULTI;
        }
        self.set_search(&a.plan, 0);
        self.m.acq = a.back.acq;
        // the search needs records that trigger at the new level
        self.m.auto_fast = false;
        a.stage = Stage::Search(0);
        true
    }

    fn auto_end(&mut self, a: AutoRun) {
        self.m.auto_fast = false;
        // the deep record was for the search: Auto depth, as the auto set-up leaves it
        self.m.mdepth = 0;
        let _ = self.src.hw_levels(None);
        eprintln!("oos-ui auto: levels {:?}, periods {:?}", a.lv, a.period);
        let stats = |c: usize| Stats { vmin: a.lv[c].0, vmax: a.lv[c].1, ..a.st[c] };
        // the channels with a period first (the trigger goes to the first), then those with a signal but no steady
        // period (noise, glitches, an irregular bus): they keep the fitted scale and level, the time base comes from the
        // window that showed most of their height
        let mut found: Vec<(usize, Stats, f64)> = (0..n_ch()).filter_map(|c| a.period[c].map(|p| (c, stats(c), p))).collect();
        let timed = !found.is_empty();
        found.extend((0..n_ch()).filter(|&c| a.want[c] && a.period[c].is_none()).map(|c| (c, stats(c), f64::NAN)));
        if found.is_empty() {
            // the stamp only counts up: records of the run must not pass as taken with the set-up restored
            let stamp = self.m.hw_stamp;
            self.m = *a.back;
            self.m.hw_stamp = stamp;
            self.toast("Auto set-up: no signal found");
        } else {
            // the others as they were: on, with their scale
            for c in (0..n_ch()).filter(|&c| !found.iter().any(|f| f.0 == c)) {
                self.m.ch[c] = a.back.ch[c].clone();
            }
            self.m.acq = a.back.acq;
            // the search ran at full bandwidth (a fast signal must not look small); the channel's own choice comes back,
            // Auto by default, which keeps the noise of slow time bases off the trigger (trigger.md 7)
            for &(c, _, _) in &found {
                self.m.ch[c].bw = a.back.ch[c].bw;
            }
            place(&mut self.m, &found);
            if self.m.auto_nrej {
                self.auto_noise_reject();
            }
            if timed {
                self.toast("Auto set-up");
                // the views of the signal on a bar in the waveform window
                self.auto_bar = Some(self.now);
            } else {
                self.m.tscale = a.best[found[0].0].map_or(a.back.tscale, |b| b.1);
                self.m.tfine = 1.0;
                self.toast("Auto set-up: no steady period");
            }
        }
        self.dirty_map = true;
        self.mark_changed();
    }
}

/// What the bar after an auto set-up offers (drawn in the waveform window, `AUTO_BAR_S` s or until a tap elsewhere).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AutoView {
    /// about three periods
    Few,
    /// about ten periods
    Many,
    /// the rising edge across about two divisions
    Rise,
    Fall,
}

const AUTO_VIEWS: [AutoView; 4] = [AutoView::Few, AutoView::Many, AutoView::Rise, AutoView::Fall];
/// The bar goes after this long without a tap on it.
const AUTO_BAR_S: f64 = 30.0;
const AB_W: i32 = 88;
/// Low enough for the free band above a signal the auto set-up placed (80 % of the height: 0.8 div at the top).
const AB_H: i32 = 34;
/// Wide enough that the trigger position mark at the top centre stays clear.
const AB_GAP: i32 = 16;
const AB_TOP: i32 = 6;

impl Ui {
    /// The buttons of the bar at the very top of the waveform window, the views centred, the close button (None) right of
    /// them.
    pub(super) fn auto_bar_rects(&self) -> Vec<(Rect, Option<AutoView>)> {
        let g = self.grid;
        let n = AUTO_VIEWS.len() as i32;
        let w = n * AB_W + (n - 1) * AB_GAP;
        let x0 = g.x + (g.w - w) / 2;
        let y = g.y + AB_TOP;
        let mut v: Vec<(Rect, Option<AutoView>)> = AUTO_VIEWS.iter().enumerate().map(|(i, &v)| (Rect::new(x0 + i as i32 * (AB_W + AB_GAP), y, AB_W, AB_H), Some(v))).collect();
        v.push((Rect::new(x0 + w + AB_GAP, y, AB_H, AB_H), None));
        v
    }

    pub(super) fn auto_bar_on(&self) -> bool {
        self.auto_bar.is_some_and(|t| self.now - t < AUTO_BAR_S) && !self.view_on()
    }

    /// A tap while the bar shows: a button sets its view and keeps the bar; anywhere else closes it (and the tap goes on
    /// to what is under it).
    pub(super) fn auto_bar_tap(&mut self, x: i32, y: i32) -> bool {
        if !self.auto_bar_on() {
            return false;
        }
        if let Some((_, v)) = self.auto_bar_rects().into_iter().find(|(r, _)| r.contains(x, y)) {
            match v {
                Some(v) => {
                    self.auto_view(v);
                    self.auto_bar = Some(self.now);
                }
                None => self.auto_bar = None,
            }
            return true;
        }
        self.auto_bar = None;
        false
    }

    /// The time base (and slope) for view `v` of the trigger channel, from its measurements now.
    pub(super) fn auto_view(&mut self, v: AutoView) {
        let c = self.m.trig_src;
        let ok = |x: f64| x.is_finite() && x > 0.0;
        let period = Some(self.meas(c, Meas::Period)).filter(|&p| ok(p));
        let Some(p) = period else { return self.toast("No period on the trigger channel") };
        let (t, slope) = match v {
            AutoView::Few => (p * 3.0 / HDIV as f64, None),
            AutoView::Many => (p * 10.0 / HDIV as f64, None),
            // the 10-90 % time over about two divisions; a period / 20 when there is no edge time
            AutoView::Rise => (Some(self.meas(c, Meas::Rise)).filter(|&x| ok(x)).unwrap_or(p / 10.0) / 2.0, Some(Slope::Rise)),
            AutoView::Fall => (Some(self.meas(c, Meas::Fall)).filter(|&x| ok(x)).unwrap_or(p / 10.0) / 2.0, Some(Slope::Fall)),
        };
        self.m.tscale = tscale_for(t as f32);
        self.m.tfine = 1.0;
        self.m.delay = 0.0;
        if let Some(s) = slope {
            self.m.slope = s;
        }
        self.dirty_map = true;
        self.settings_changed();
        self.mark_changed();
    }

    pub(super) fn draw_auto_bar(&self, cv: &mut Canvas) {
        if !self.auto_bar_on() {
            return;
        }
        for (r, v) in self.auto_bar_rects() {
            cv.round(r, 6, PANEL_BG, 225);
            cv.round_outline(r, 6, BTN_BORDER);
            match v {
                Some(v) => auto_icon(cv, r, v),
                None => {
                    // a cross, 2 px
                    let (cx, cy, d) = (r.x + r.w / 2, r.y + r.h / 2, 6);
                    for k in 0..2 {
                        cv.line(cx - d + k, cy - d, cx + d + k, cy + d, TEXT);
                        cv.line(cx + d + k, cy - d, cx - d + k, cy + d, TEXT);
                    }
                }
            }
        }
    }
}

/// The symbol of a view, drawn in lines 2 px wide: square waves for the periods, a ramp between two levels for an edge.
fn auto_icon(cv: &mut Canvas, r: Rect, v: AutoView) {
    let (x0, x1) = (r.x + 16, r.x + r.w - 16);
    let (hi, lo) = (r.y + 9, r.y + r.h - 9);
    let seg = |cv: &mut Canvas, a: (i32, i32), b: (i32, i32)| {
        cv.line(a.0, a.1, b.0, b.1, TEXT);
        if a.0 == b.0 {
            cv.line(a.0 + 1, a.1, b.0 + 1, b.1, TEXT);
        } else {
            cv.line(a.0, a.1 + 1, b.0, b.1 + 1, TEXT);
        }
    };
    match v {
        AutoView::Few | AutoView::Many => {
            // half periods across the width, starting low
            let halves = if v == AutoView::Few { 5 } else { 10 };
            let w = (x1 - x0) as f32 / halves as f32;
            let mut y = lo;
            let mut x = x0;
            for k in 1..=halves {
                let nx = x0 + (k as f32 * w).round() as i32;
                seg(cv, (x, y), (nx, y));
                if k < halves {
                    let ny = if y == lo { hi } else { lo };
                    seg(cv, (nx, y), (nx, ny));
                    y = ny;
                }
                x = nx;
            }
        }
        AutoView::Rise | AutoView::Fall => {
            let (a, b) = if v == AutoView::Rise { (lo, hi) } else { (hi, lo) };
            let (m0, m1) = (x0 + (x1 - x0) * 3 / 10, x0 + (x1 - x0) * 7 / 10);
            seg(cv, (x0, a), (m0, a));
            seg(cv, (m0, a), (m1, b));
            seg(cv, (m1, b), (x1, b));
            // the trigger level through the middle of the edge, dashed
            let my = (hi + lo) / 2;
            let mut x = x0;
            while x < x1 {
                cv.hline(x, (x + 2).min(x1), my, DIM);
                x += 6;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar after an auto set-up: periods and edges of the trigger channel, gone after a tap elsewhere.
    #[test]
    fn view_bar_sets_periods_and_edges() {
        let mut u = Ui::new(crate::wave::Source::Demo { t: 0.0 });
        u.m.trig_src = 0;
        for _ in 0..20 {
            u.step(0.05);
        }
        let p = u.meas(0, Meas::Period);
        assert!((p - 1e-3).abs() < 1e-5, "demo CH1 period {}", p);
        u.auto_bar = Some(u.now);
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        u.render(&mut cv);
        if let Ok(d) = std::env::var("OOS_SHOT_DIR") {
            let _ = std::fs::write(format!("{}/auto-bar.png", d), crate::store::png_of(&cv));
        }
        let tap = |u: &mut Ui, v: AutoView| {
            let r = u.auto_bar_rects().into_iter().find(|b| b.1 == Some(v)).unwrap().0;
            u.tap(r.x + r.w / 2, r.y + r.h / 2);
        };
        // about three and about ten periods on the screen
        tap(&mut u, AutoView::Few);
        let span = |u: &Ui| u.m.time_per_div() as f64 * HDIV as f64 / p;
        assert!((3.0..6.0).contains(&span(&u)), "{}", span(&u));
        tap(&mut u, AutoView::Many);
        assert!((9.9..20.0).contains(&span(&u)), "{}", span(&u));
        // the edges: the slope follows, the time base is near the edge time
        for _ in 0..10 {
            u.step(0.05);
        }
        tap(&mut u, AutoView::Fall);
        assert_eq!(u.m.slope, Slope::Fall);
        assert!(span(&u) < 0.2, "{}", span(&u));
        tap(&mut u, AutoView::Rise);
        assert_eq!(u.m.slope, Slope::Rise);
        assert!(u.auto_bar_on());
        // the cross closes it
        let x = u.auto_bar_rects().into_iter().find(|b| b.1.is_none()).unwrap().0;
        u.tap(x.x + x.w / 2, x.y + x.h / 2);
        assert!(!u.auto_bar_on());
        // so does a tap elsewhere
        u.auto_bar = Some(u.now);
        u.tap(u.grid.x + 20, u.grid.y + u.grid.h - 30);
        assert!(!u.auto_bar_on());
        // inside the plot, above the band an auto set-up leaves free at the top (0.8 div), clear of the trigger mark
        let g = u.grid;
        let b = u.auto_bar_rects();
        assert!(b.iter().all(|(r, _)| r.y > g.y && r.y + r.h < g.y + (0.8 * g.h as f32 / 8.0) as i32 && r.x > g.x && r.x + r.w < g.x + g.w));
        let mid = g.x + g.w / 2;
        assert!(b.iter().all(|(r, _)| r.x > mid + 6 || r.x + r.w < mid - 6));
    }

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
        // the search runs at full bandwidth, the channel keeps its own choice: Auto by default, Full when chosen
        assert_eq!(ui.m.ch[0].bw, BW_AUTO);
        assert_eq!(ui.m.run, RunState::Run);
        ui.m.ch[0].bw = BW_FULL;
        run(&mut ui);
        assert_eq!(ui.m.ch[0].bw, BW_FULL);
    }

    #[test]
    fn noise_reject_menu_setup_scpi_and_hardware() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        // a new set-up: Auto bandwidth, noise reject off
        assert!(ui.m.ch.iter().all(|c| c.bw == BW_AUTO) && !ui.m.trig_nrej);
        assert!(ui.trigger_items().contains(&Item::TrigNoise));
        ui.select_option(Item::TrigNoise, "On");
        assert!(ui.m.trig_nrej && ui.m.trig_hyst_div() == 0.7);
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert!(back.trig_nrej);
        assert!(crate::board::dho::hw::settings(&ui.m, false).noise_reject);
        ui.m.trig_nrej = false;
        let mut scpi = crate::scpi::Scpi::open(0);
        let cv = Canvas::new(PANEL_W, PANEL_H);
        scpi.exec_line(&mut ui, &cv, ":TRIG:NREJ ON");
        assert_eq!(scpi.exec_line(&mut ui, &cv, ":TRIG:NREJ?"), b"1");
        assert!(ui.m.trig_nrej);
    }

    // Break it catches: the levels stages waiting the full auto time per record, or the fast auto trigger staying on
    // after the auto set-up (docs/fork/trigger-lockin.md)
    #[test]
    fn levels_stages_use_the_fast_auto_trigger_only() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert!(!crate::board::dho::hw::settings(&ui.m, false).auto_fast);
        ui.auto_start();
        assert!(crate::board::dho::hw::settings(&ui.m, false).auto_fast);
        let mut seen_search = false;
        for _ in 0..4000 {
            ui.step(0.02);
            match ui.auto.as_ref().map(|a| matches!(a.stage, Stage::Search(_))) {
                Some(true) => {
                    seen_search = true;
                    assert!(!ui.m.auto_fast, "the search triggers at the new level");
                }
                Some(false) => assert!(ui.m.auto_fast),
                None => break,
            }
        }
        assert!(seen_search && ui.auto.is_none() && !ui.m.auto_fast);
        // a failed run (no channel worth a period) ends without it too
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for c in 0..NCH {
            ui.m.ch[c].on = c == 3;
        }
        run(&mut ui);
        assert!(!ui.m.auto_fast);
    }

    /// A hardware-like record of CH1 under settings stamp `stamp`: the signal `sig` of the time (s) over the window of
    /// the time base set now, 8000 samples, the hardware point in the middle.
    fn hw_record_of(ui: &Ui, stamp: u64, sig: &dyn Fn(f64) -> f32) -> crate::wave::Record {
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 8000usize;
        let dt = window * 1.25 / n as f64;
        let v: Vec<f32> = (0..n).map(|i| sig(i as f64 * dt)).collect();
        let f = std::sync::Arc::new(crate::board::Frame { seq: 1, dt, pre: n / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], stamp, ..Default::default() });
        crate::board::to_record(&f, &ui.m, 3600, window, ui.m.delay as f64 - window / 2.0)
    }

    /// Runs an auto set-up on hardware-like records (`hw_record_of`) without letting time pass; returns the stages seen.
    fn run_hw(ui: &mut Ui, lo: f32, hi: f32) -> Vec<String> {
        run_hw_of(ui, &|t| if ((t + 0.3e-3) % 1e-3) < 0.5e-3 { hi } else { lo })
    }

    fn run_hw_of(ui: &mut Ui, sig: &dyn Fn(f64) -> f32) -> Vec<String> {
        for c in 0..NCH {
            ui.m.ch[c].on = c == 0;
        }
        ui.auto_start();
        let mut stages = Vec::new();
        for _ in 0..20 {
            let Some(a) = ui.auto.as_ref() else { return stages };
            let stamp = a.stamp;
            stages.push(match a.stage { Stage::Wide => "wide".to_string(), Stage::Fit => "fit".to_string(), Stage::Search(i) => format!("search {}", i) });
            // a record of the stage before does not count
            ui.last[0] = Some(hw_record_of(ui, stamp - 1, sig));
            ui.auto_step();
            assert_eq!(ui.auto.as_ref().map(|a| a.stamp), Some(stamp), "an old record ended a stage");
            ui.last[0] = Some(hw_record_of(ui, stamp, sig));
            ui.auto_step();
        }
        panic!("auto set-up never ended: {:?}", stages);
    }

    // Break it catches: stages that wait for several records and a time on the hardware (2.5 to 5.4 s on the unit),
    // or that take a record of the stage before (docs/fork/trigger-lockin.md)
    #[test]
    fn hardware_records_settle_a_stage_at_once() {
        // probe-comp output: 0.3 V at 1 V/div is under 0.4 div, so the wide stage rescales and looks again
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let now = ui.now;
        let stages = run_hw(&mut ui, 0.0, 0.3);
        assert_eq!(ui.now, now);
        assert_eq!(&stages[..2], ["wide", "fit"]);
        assert!(ui.auto.is_none() && ui.m.ch[0].on);
        assert!((ui.m.trig_level - 0.15).abs() < 0.02, "level {}", ui.m.trig_level);
        let p = ui.m.time_per_div() as f64 * HDIV as f64;
        assert!((1.5e-3..=12e-3).contains(&p), "window {} s for a 1 ms period", p);
        // a signal that reads well at the wide scale: no second look
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let stages = run_hw(&mut ui, -1.0, 2.0);
        assert_eq!(&stages[..2], ["wide", "search 0"]);
        assert!(ui.auto.is_none() && (ui.m.trig_level - 0.5).abs() < 0.1, "level {}", ui.m.trig_level);
    }

    // Break it catches: an auto set-up that throws away the scale and level it found because the signal has no steady
    // period (the noisy 5 V signal of docs/fork/trigger-lockin.md), or one that keeps a channel without a signal
    #[test]
    fn a_signal_without_a_steady_period_keeps_scale_and_level() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let back = ui.m.clone();
        // 0..5 V pulses at irregular distances (0.2 to 1.9 ms), seen at every time base
        let edges: Vec<f64> = {
            let (mut t, mut x, mut v) = (0.0f64, 7u32, Vec::new());
            while t < 1.0 {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                t += 0.2e-3 + (x >> 8) as f64 / (1u64 << 24) as f64 * 1.7e-3;
                v.push(t);
            }
            v
        };
        let sig = move |t: f64| {
            let k = edges.partition_point(|&e| e <= t);
            if k % 2 == 1 { 5.0 } else { 0.0 }
        };
        run_hw_of(&mut ui, &sig);
        assert!(ui.auto.is_none());
        assert!(ui.m.ch[0].on && ui.m.ch[0] != back.ch[0], "the fit stays");
        let vpd = ui.m.ch[0].volts_per_div();
        assert!((5.0 / vpd) > 2.0 && (5.0 / vpd) <= 8.0, "5 V over {} div", 5.0 / vpd);
        assert!((ui.m.trig_level - 2.5).abs() < 0.3, "level {}", ui.m.trig_level);
        assert_eq!(ui.m.trig_src, 0);
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("no steady period")));
        assert!(!ui.auto_bar_on());
        // a flat input is still no signal: the set-up stays
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let back = ui.m.clone();
        run_hw_of(&mut ui, &|_| 0.004);
        assert_eq!(ui.m.ch[0], back.ch[0]);
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("no signal")));
    }

    // Break it catches: the opt-in noise reject left off on a signal whose noise exceeds the hysteresis (it then
    // retriggers on the noise), turned on for a clean one, or touched while the option is off
    #[test]
    fn opt_in_noise_reject_follows_the_noise_on_the_levels() {
        let noisy = |t: f64| {
            // 0..5 V, 1 kHz, +-0.6 V pseudo-random noise (about 0.35 V sigma, more than 0.3 div at 1 or 2 V/div / 6)
            let k = (t * 1e9) as u64;
            let r = (k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 40) as f32 / (1u64 << 24) as f32;
            (if ((t + 0.3e-3) % 1e-3) < 0.5e-3 { 5.0 } else { 0.0 }) + (r - 0.5) * 1.2
        };
        let clean = |t: f64| if ((t + 0.3e-3) % 1e-3) < 0.5e-3 { 5.0 } else { 0.0 };
        for (opt, sig, want) in [(true, &noisy as &dyn Fn(f64) -> f32, true), (true, &clean, false), (false, &noisy, false)] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.auto_nrej = opt;
            run_hw_of(&mut ui, sig);
            assert_eq!(ui.m.trig_nrej, want, "opt-in {} noise {:?}", opt, crate::wave::flat_noise(&ui.record_volts(0)));
        }
        // the option is part of the set-up and of the trigger menu
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert!(ui.trigger_items().contains(&Item::TrigAutoNoise));
        ui.select_option(Item::TrigAutoNoise, "On");
        assert!(crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap().auto_nrej);
    }

    /// A hardware source whose thread is played by hand, CH1 alone.
    fn hw_ui() -> (Ui, crate::board::HwShared) {
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        for c in 0..NCH {
            ui.m.ch[c].on = c == 0;
        }
        (ui, shared)
    }

    /// The thread's answer to the request in `shared`: CH1 from `lo` to `hi` V (None: no usable reading).
    fn answer(shared: &crate::board::HwShared, ui: &Ui, ch1: Option<(f32, f32, bool)>) {
        let mut g = shared.lock().unwrap();
        let (id, need, runs) = g.levels_want.expect("a request");
        assert_eq!((need, runs), (ui.auto.as_ref().unwrap().stamp, UNIT_RUNS));
        let l = ch1.map(|(lo, hi, clipped)| crate::board::ChLevels { lo, hi, mean: (lo + hi) / 2.0, clipped });
        g.levels = Some((id, [l, None, None, None]));
    }

    // Break it catches: the measure-unit levels not used (the 20 ms/div records again), used from an answer to an
    // earlier request, or a failed reading that stops the auto set-up instead of handing over to the records
    #[test]
    fn measure_unit_levels_skip_the_level_records() {
        let (mut ui, shared) = hw_ui();
        ui.auto_start();
        let now = ui.now;
        assert!(ui.auto.as_ref().unwrap().unit.is_some());
        // waiting: no answer yet, and an answer to another request does not count
        ui.auto_step();
        assert!(matches!(ui.auto.as_ref().unwrap().stage, Stage::Wide));
        shared.lock().unwrap().levels = Some((0, [None; 4]));
        ui.auto_step();
        assert!(ui.auto.as_ref().unwrap().unit.is_some());
        // the probe-comp output at 1 V/div: under 0.4 div, asked again at 0.1 V/div
        answer(&shared, &ui, Some((0.0, 0.3, false)));
        ui.auto_step();
        assert_eq!(ui.m.ch[0].volts_per_div(), 0.1);
        assert_eq!(ui.auto.as_ref().unwrap().unit.map(|u| u.tries), Some(2));
        answer(&shared, &ui, Some((0.0, 0.3, false)));
        ui.auto_step();
        let a = ui.auto.as_ref().unwrap();
        assert!(matches!(a.stage, Stage::Search(0)) && a.unit.is_none(), "no record was needed");
        assert!((ui.m.trig_level - 0.15).abs() < 1e-6 && (ui.m.ch[0].offset + 0.15).abs() < 1e-6);
        assert_eq!(ui.now, now);
        assert_eq!(shared.lock().unwrap().levels_want, None);

        // no usable reading for CH1: the records take over in the Wide stage
        let (mut ui, shared) = hw_ui();
        ui.auto_start();
        answer(&shared, &ui, None);
        ui.auto_step();
        let a = ui.auto.as_ref().unwrap();
        assert!(a.unit.is_none() && matches!(a.stage, Stage::Wide));
        // no answer in time: the same
        let (mut ui, _shared) = hw_ui();
        ui.auto_start();
        ui.now += UNIT_GIVE_UP_S + 0.01;
        ui.auto_step();
        assert!(ui.auto.as_ref().unwrap().unit.is_none());
        // clipping at both tries: the records look at the last scale (Fit)
        let (mut ui, shared) = hw_ui();
        ui.auto_start();
        for _ in 0..UNIT_TRIES {
            answer(&shared, &ui, Some((-1.0, 30.0, true)));
            ui.auto_step();
        }
        let a = ui.auto.as_ref().unwrap();
        assert!(a.unit.is_none() && matches!(a.stage, Stage::Fit));
        assert_eq!(ui.m.ch[0].volts_per_div(), 5.0);
    }

    // Break it catches: the hardware search going through the five time bases again (a record each), a deep record
    // left behind, a 20 ms/div record for a signal the deep one timed, or a fast signal timed from a folded record
    #[test]
    fn one_deep_record_finds_the_period() {
        let square = |f: f64| move |t: f64| if (t * f + 0.3) % 1.0 < 0.5 { 3.0f32 } else { 0.0 };
        for (f, want) in [(1e3, vec![FAST_TBASE, DEEP.0]), (50.0, vec![FAST_TBASE, DEEP.0, SLOW_TBASE]), (2e7, vec![FAST_TBASE])] {
            let (mut ui, shared) = hw_ui();
            ui.auto_start();
            answer(&shared, &ui, Some((0.0, 3.0, false)));
            ui.auto_step();
            assert!(matches!(ui.auto.as_ref().unwrap().stage, Stage::Search(0)));
            assert_eq!(tscales()[ui.m.tscale], tscales()[tscale_for(FAST_TBASE)], "the full rate first");
            let sig = square(f);
            let mut seen = Vec::new();
            for _ in 0..6 {
                let Some(a) = ui.auto.as_ref() else { break };
                seen.push(tscales()[ui.m.tscale]);
                // the samples of the record at the rate of the time base (deep: 1 M points over 12 div)
                let window = ui.m.time_per_div() as f64 * HDIV as f64;
                let n = if mdepths()[ui.m.mdepth].1 == DEEP.1 { 200_000 } else { 20_000 };
                let dt = window / n as f64;
                let v: Vec<f32> = (0..n).map(|k| sig(k as f64 * dt)).collect();
                let fr = std::sync::Arc::new(crate::board::Frame { seq: 1, dt, pre: n / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], stamp: a.stamp, ..Default::default() });
                ui.last[0] = Some(crate::board::to_record(&fr, &ui.m, 3600, window, -window / 2.0));
                ui.auto_step();
            }
            assert!(ui.auto.is_none(), "{} Hz: {:?}", f, seen);
            let want: Vec<f32> = want.iter().map(|&t| tscales()[tscale_for(t)]).collect();
            assert_eq!(seen, want, "{} Hz", f);
            assert!(ui.m.ch[0].on);
            assert_eq!(mdepths()[ui.m.mdepth].1, 0, "Auto depth after the search");
            let period = ui.m.time_per_div() as f64 * HDIV as f64;
            assert!(period > 1.0 / f && period < 30.0 / f, "{} Hz: {} s on the screen", f, period);
        }
    }

    // Break it catches: the deep search record at 1 M points with several channels on (an 8 MB read: the unit rebooted),
    // or measure-unit levels asked for with several channels on (their slots are not known)
    #[test]
    fn several_channels_keep_the_deep_record_small_and_skip_the_measure_unit() {
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        for c in 0..NCH {
            ui.m.ch[c].on = c < 3;
        }
        ui.auto_start();
        assert!(ui.auto.as_ref().unwrap().unit.is_none());
        assert_eq!(shared.lock().unwrap().levels_want, None);
        let mut a = ui.auto.take().unwrap();
        a.want[0] = true;
        let levels = [(0.0f32, 3.0f32); NCH];
        assert!(ui.auto_fit(&mut a, Some(levels)));
        assert_eq!(a.plan[1], (DEEP.0, DEEP_MULTI));
        // one channel: the deep record and the measure unit
        let (mut ui, shared) = hw_ui();
        ui.auto_start();
        assert!(ui.auto.as_ref().unwrap().unit.is_some() && shared.lock().unwrap().levels_want.is_some());
        let mut a = ui.auto.take().unwrap();
        assert!(ui.auto_fit(&mut a, Some(levels)));
        assert_eq!(a.plan[1], DEEP);
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
