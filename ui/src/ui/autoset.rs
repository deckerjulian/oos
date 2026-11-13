// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Auto set-up on the hardware, for the channels that are on. A record only shows what the scale and time base let
//! through, so this one acquires: a wide scale for the levels in Peak mode, then a scale that fits, then time bases from
//! fast to slow until each channel shows a steady period with most of its height (fast first: a coarse time base can
//! fold a fast signal into a period that is not there). A channel without a period keeps its settings, one that is off stays off. With no signal on any
//! channel the set-up stays as it was.
//! The hardware Auto action only targets live ADC channels. Stored/SIM records
//! use the separate Fit record action; its bounded helper is retained below.

use super::*;
use crate::wave::{autoset_shown, periodic, place, Stats};

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
    pub(super) fn autoset_live_only(&mut self) {
        if self.record_mode() || !(0..n_ch()).any(|channel| self.m.ch[channel].on) {
            self.toast("Autoset needs a live channel; use Fit record");
            return;
        }
        if self.src.is_hw() { self.auto_start(); }
        else {
            autoset(&mut self.m, &self.src);
            self.dirty_map = true;
            self.settings_changed();
            self.toast("Auto set-up");
        }
    }
    /// An auto set-up is under way.
    pub fn auto_running(&self) -> bool {
        self.auto.is_some()
    }

    /// AUTO key on the hardware: starts the run, `auto_step` goes on from the step loop.
    pub(super) fn auto_start(&mut self) {
        if self.record_mode() {
            return self.toast("Autoset needs a live channel; use Fit record");
        }
        let Some(first) = (0..n_ch()).find(|&c| self.m.ch[c].on) else {
            return self.toast("Autoset needs a live channel; use Fit record");
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
        m.trig_src = first;
        m.trig_ref = None;
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
        let Some(record) = &self.last[0] else { return (f32::NAN, f32::NAN) };
        let s = crate::wave::autoset_summary(&record.ch[c], record.dt);
        (s.vmin, s.vmax)
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
                        if let Some(p) = self.last[0].as_ref().and_then(|r| periodic(&r.ch[c], dt)) {
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
            self.m.mdepth = a.back.mdepth;
            // the search ran at full bandwidth (a fast signal must not look small); the channel's own choice comes back,
            // Auto by default, which keeps the noise of slow time bases off the trigger (trigger.md 7)
            for &(c, _, _) in &found {
                self.m.ch[c].bw = a.back.ch[c].bw;
            }
            place(&mut self.m, &found);
            self.toast("Auto set-up");
            // the views of the signal on a bar in the waveform window
            self.auto_bar = Some(self.now);
        }
        self.dirty_map = true;
        self.mark_changed();
    }
}

impl Ui {
    /// Whether the separate Fit record action is working with stored references.
    /// The hardware Auto action does not call this reference fitting path.
    pub(super) fn reference_autoset(&self) -> bool {
        if self.sim_play {
            return true;
        }
        let live = (0..n_ch()).any(|c| self.m.ch[c].on);
        !live && self.refs.iter().any(|r| r.as_ref().is_some_and(|r| r.on && r.v.len() >= 4 && r.dt > 0.0))
    }

    /// Bounded Fit record helper: scale/position/time base from shown refs;
    /// never called by hardware Auto and never turns an ADC channel on.
    pub(super) fn autoset_reference(&mut self) {
        let shown: Vec<usize> = self
            .refs
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let r = r.as_ref()?;
                (r.on && r.v.len() >= 4 && r.dt.is_finite() && r.dt > 0.0).then_some(i)
            })
            .collect();
        if shown.is_empty() {
            self.toast("Fit record: no reference on");
            return;
        }
        // Borrow the records: four 20M files must not create four extra 80MB vectors.
        let waves: Vec<(&[f32], f64)> = shown.iter().map(|&i| { let r = self.refs[i].as_ref().unwrap(); (r.v.as_slice(), r.dt) }).collect();
        let views = autoset_shown(&mut self.m, &waves);
        let mut any = false;
        let mut fitted = Vec::new();
        for (&i, view) in shown.iter().zip(views) {
            if let Some((vpd, offset)) = view {
                if let Some(r) = &mut self.refs[i] {
                    r.vpd = vpd;
                    r.offset = offset;
                    any = true;
                    fitted.push(i);
                }
            }
        }
        if !any {
            self.toast("Fit record: no signal found");
            return;
        }
        // Keep the chosen time window in an actual record, including files whose
        // first sample is far from zero. Prefer the user's selected shown source.
        let selected = self.selected_workspace_source().and_then(|id| match id { WaveId::Ref(i) if fitted.contains(&i) => Some(i), _ => None });
        if let Some(reference) = selected.or_else(|| fitted.first().copied()).and_then(|i| self.refs[i].as_ref()) {
            let end = reference.t0 + reference.dt * reference.v.len().saturating_sub(1) as f64;
            let half = self.m.time_per_div() as f64 * HDIV as f64 * 0.5;
            let center = self.m.delay as f64;
            if center + half < reference.t0 || center - half > end {
                self.m.delay = (reference.t0 + 0.5 * (end - reference.t0)) as f32;
            }
        }
        if self.m.zoom { self.reset_zoom(); }
        self.ph.clear();
        self.phz.clear();
        self.measure_refs();
        self.dirty_map = true;
        self.toast("Record fitted");
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
        // The bar navigates the fitted ADC capture. A previous edge view may
        // make the Screen measurement gate too short to contain even one full
        // period; that must not disable the next edge/period navigation action.
        let native = self.last[0].as_ref().map(|record| {
            if let Some(raw) = record.raw.as_ref().filter(|raw| !raw.frame.envelope && !raw.frame.ch[c].is_empty()) {
                crate::wave::autoset_summary(&raw.frame.ch[c], raw.frame.dt)
            } else { crate::wave::autoset_summary(&record.ch[c], record.dt) }
        });
        let period = native.as_ref().map(|summary| summary.period).filter(|&p| ok(p))
            .or_else(|| Some(self.meas(c, Meas::Period)).filter(|&p| ok(p)));
        let Some(p) = period else { return self.toast("No period on the trigger channel") };
        let (t, slope) = match v {
            AutoView::Few => (p * 3.0 / HDIV as f64, None),
            AutoView::Many => (p * 10.0 / HDIV as f64, None),
            // the 10-90 % time over about two divisions; a period / 20 when there is no edge time
            AutoView::Rise => (native.as_ref().map(|summary| summary.rise).filter(|&x| ok(x))
                .or_else(|| Some(self.meas(c, Meas::Rise)).filter(|&x| ok(x))).unwrap_or(p / 10.0) / 2.0, Some(Slope::Rise)),
            AutoView::Fall => (native.as_ref().map(|summary| summary.fall).filter(|&x| ok(x))
                .or_else(|| Some(self.meas(c, Meas::Fall)).filter(|&x| ok(x))).unwrap_or(p / 10.0) / 2.0, Some(Slope::Fall)),
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

    #[test]
    fn reference_autoset_reuses_samples_and_recenters_a_record_away_from_zero() {
        let mut ui=Ui::new(Source::Demo{t:0.0});
        ui.m.ch.iter_mut().for_each(|ch|ch.on=false);ui.m.run=RunState::Stop;
        ui.refs[0]=Some(sine_ref(2000,10.0,1e-7));
        ui.refs[0].as_mut().unwrap().t0=0.123;
        let pointer=ui.refs[0].as_ref().unwrap().v.as_ptr();
        let trigger=(ui.m.trigger_wave(),ui.m.trig_level,ui.m.trig_mode);
        ui.autoset_reference();
        let record=ui.refs[0].as_ref().unwrap();
        assert_eq!(record.v.as_ptr(),pointer);
        assert!(ui.m.delay as f64>=record.t0 && ui.m.delay as f64<=record.t0+record.dt*record.v.len() as f64);
        assert_eq!((ui.m.trigger_wave(),ui.m.trig_level,ui.m.trig_mode),trigger);
        assert_eq!((ui.m.run,ui.replay.captures),(RunState::Stop,0));
    }

    #[test]
    fn successful_hardware_autoset_restores_configured_memory_depth() {
        let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.mdepth=1;
        let back=Box::new(ui.m.clone());ui.m.mdepth=0;
        ui.auto_end(AutoRun{back,stage:Stage::Search(0),records:0,since:0.0,fits:true,
            lv:[(-1.0,1.0);NCH],want:[true;NCH],period:[Some(1e-6),None,None,None],full:[2.0;NCH],swing:[2.0;NCH]});
        assert_eq!(ui.m.mdepth,1);
    }

    #[test]
    #[ignore="local user-provided D-drive files; no app, window or services"]
    fn actual_gen7_20m_reference_fit_is_bounded_and_keeps_native_records() {
        let mut ui=Ui::new(Source::Demo{t:0.0});
        for path in ["D:/Gen7_65GB_CP_Q6_n_Ch2.wfm","D:/Gen7_65GB_CP_Q6_p_Ch1.wfm"] {
            let bytes=std::fs::read(path).unwrap();ui.load_waveform_auto(&bytes,path.rsplit('/').next().unwrap()).unwrap();
        }
        let identity:Vec<_>=ui.refs.iter().flatten().map(|r|(r.v.as_ptr(),r.v.len(),r.dt,r.t0)).collect();
        let acquisition=(ui.m.run,ui.m.trigger_wave(),ui.m.trig_mode,ui.m.trig_level,ui.records,ui.replay.captures);
        let start=std::time::Instant::now();
        for slot in 0..2 {
            let other=ui.refs[1-slot].as_ref().map(|r|(r.vpd,r.offset));
            ui.select_workspace_source(WaveId::Ref(slot));
            assert!(ui.fit_selected_record());
            assert_eq!(other,ui.refs[1-slot].as_ref().map(|r|(r.vpd,r.offset)));
            let record=ui.refs[slot].as_ref().unwrap();
            let half=ui.m.time_per_div() as f64*HDIV as f64*0.5;
            assert!(ui.m.delay as f64-half<=record.t0+1e-14);
            assert!(ui.m.delay as f64+half>=record.t0+(record.v.len()-1) as f64*record.dt-1e-14);
        }
        let elapsed=start.elapsed();
        assert_eq!(identity,ui.refs.iter().flatten().map(|r|(r.v.as_ptr(),r.v.len(),r.dt,r.t0)).collect::<Vec<_>>());
        assert_eq!(acquisition,(ui.m.run,ui.m.trigger_wave(),ui.m.trig_mode,ui.m.trig_level,ui.records,ui.replay.captures));
        assert!(ui.refs.iter().flatten().all(|r|r.vpd>0.0&&r.offset.is_finite()));
        assert!(elapsed<std::time::Duration::from_secs(3),"bounded setup took {elapsed:?}");
        eprintln!("actual Gen7 two20M explicit selected-reference Fit: {elapsed:?}; native pointers/metadata, other refs and acquisition state unchanged");
    }

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
        let mut cv = Canvas::new(W, H);
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
        let cv = Canvas::new(W, H);
        scpi.exec_line(&mut ui, &cv, ":TRIG:NREJ ON");
        assert_eq!(scpi.exec_line(&mut ui, &cv, ":TRIG:NREJ?"), b"1");
        assert!(ui.m.trig_nrej);
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

    fn sine_ref(n: usize, cycles: f64, dt: f64) -> RefWave {
        let v: Vec<f32> = (0..n).map(|k| (k as f64 / n as f64 * cycles * std::f64::consts::TAU).sin() as f32).collect();
        RefWave { src: 0, color: 0, label: String::new(), t0: 0.0, dt, v: v.into(), vpd: 10.0, offset: 3.0, on: true }
    }

    #[test]
    fn live_autoset_ignores_a_shown_reference() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.ch[1].on = false;
        ui.refs[0] = Some(sine_ref(2000, 10.0, 1e-7));
        ui.m.trig_level = 9.0;
        ui.m.trig_src = 2;
        ui.select_workspace_source(WaveId::Ref(0));
        let reference = { let r = ui.refs[0].as_ref().unwrap(); (r.v.as_ptr(), r.v.len(), r.dt, r.t0) };
        ui.key(key::AUTO);
        let r = ui.refs[0].as_ref().unwrap();
        assert!((r.vpd - 10.0).abs() < 1e-6 && (r.offset - 3.0).abs() < 1e-6);
        assert_eq!((r.v.as_ptr(), r.v.len(), r.dt, r.t0), reference);
        assert!(!ui.m.ch[1].on);
        assert_eq!(ui.m.trig_src, 0);
        assert!(ui.m.trig_level < 2.0, "level {}", ui.m.trig_level);
        assert_ne!(ui.m.ch[0].vscale, 12);
    }

    #[test]
    fn hardware_autoset_entry_cannot_bypass_offline_mode_even_with_a_retained_adc_flag() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_offline(true);
        ui.m.ch[0].on = true;
        let before = ui.m.clone();
        ui.auto_start();
        assert_eq!(ui.m, before); assert!(!ui.auto_running());
        assert!(ui.toast.as_ref().unwrap().0.contains("Autoset needs a live channel; use Fit record"));
    }

    #[test]
    fn offline_autoset_is_a_noop_and_directs_the_user_to_fit_record() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for c in 0..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.run = RunState::Stop;
        ui.m.trig_level = 9.0;
        ui.m.trig_src = 2;
        ui.refs[9] = Some(sine_ref(2000, 10.0, 1e-7));
        ui.refs[0] = Some(sine_ref(2000, 10.0, 1e-7));
        ui.refs[0].as_mut().unwrap().on = false;
        let before = ui.m.clone();
        let references: Vec<_> = ui.refs.iter().flatten().map(|reference| (reference.v.as_ptr(), reference.v.len(), reference.dt, reference.t0, reference.vpd, reference.offset, reference.on)).collect();
        ui.key(key::AUTO);
        assert_eq!(ui.m, before);
        assert_eq!(references, ui.refs.iter().flatten().map(|reference| (reference.v.as_ptr(), reference.v.len(), reference.dt, reference.t0, reference.vpd, reference.offset, reference.on)).collect::<Vec<_>>());
        assert!(!ui.auto_running());
        assert!(ui.toast.as_ref().unwrap().0.contains("Autoset needs a live channel; use Fit record"));
        assert!(ui.m.ch.iter().all(|c| !c.on));
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!((ui.m.trig_src, ui.m.trig_level), (2, 9.0));
        let hidden = ui.refs[0].as_ref().unwrap();
        assert!((hidden.vpd - 10.0).abs() < 1e-6 && (hidden.offset - 3.0).abs() < 1e-6);
        ui.activate(Item::AutosetAct);
        assert_eq!(ui.m, before, "the menu action follows the same ADC-only rule");
    }

    #[test]
    fn sim_autoset_does_not_fit_regenerate_or_stop_the_playing_record() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.sim_slot = 4;
        ui.m.ch[1].on = true;
        ui.activate(Item::SimPlay);
        ui.wait_sim_generation();
        assert!(ui.sim_play && ui.m.run == RunState::Run);
        let ch: Vec<(bool, usize, f32)> = ui.m.ch.iter().map(|c| (c.on, c.vscale, c.offset)).collect();
        ui.m.trig_level = 9.0;
        ui.m.trig_src = 2;
        ui.m.tscale = tscales().len() - 1;
        let r = ui.refs[4].as_mut().unwrap();
        r.vpd = 10.0;
        r.offset = 3.0;
        let before = ui.m.clone();
        let source = { let r = ui.refs[4].as_ref().unwrap(); (r.v.as_ptr(), r.v.len(), r.dt, r.t0, r.vpd, r.offset) };
        let captures = ui.replay.captures;
        ui.key(key::AUTO);
        assert_eq!(ui.m, before);
        let r = ui.refs[4].as_ref().unwrap();
        assert_eq!((r.v.as_ptr(), r.v.len(), r.dt, r.t0, r.vpd, r.offset), source);
        assert_eq!(ui.replay.captures, captures);
        assert!(ui.toast.as_ref().unwrap().0.contains("Autoset needs a live channel; use Fit record"));
        assert!(ui.sim_play && ui.m.run == RunState::Run);
        assert_eq!(ui.m.ch.iter().map(|c| (c.on, c.vscale, c.offset)).collect::<Vec<_>>(), ch);
        assert_eq!((ui.m.trig_src, ui.m.trig_level), (2, 9.0));
        assert!(ui.refs[0].is_none());
        assert!(!ui.auto_running());
    }
}
