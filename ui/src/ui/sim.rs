// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SIM: one nominal high-speed serial record in a reference slot.
//!
//! The generator is `oos_dsp::hss`. Play walks a triggered window of that record.
//! Measure and Eye read the stored samples at the slot's own interval. Live
//! channels stay on the front end. This is not a fifth wave source.

use super::*;
use oos_dsp::hss::{self, MENU};
use oos_dsp::trigger::{self, Slope, Trigger};

impl Ui {
    pub(super) fn sim_items(&self) -> Vec<Item> {
        vec![Item::SimPreset, Item::SimSlot, Item::SimPlay, Item::SimShow]
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
        Some(match it {
            Item::SimPreset => MENU.get(self.sim_preset).map(|p| p.name.to_string()).unwrap_or_default(),
            Item::SimSlot => format!("R{}", self.sim_slot + 1),
            Item::SimPlay => onoff(self.sim_play),
            Item::SimShow => onoff(self.refs.get(self.sim_slot).and_then(|r| r.as_ref()).is_some_and(|r| r.on)),
            _ => return None,
        })
    }

    pub(super) fn sim_options(&self, it: Item) -> Option<Vec<String>> {
        Some(match it {
            Item::SimPreset => MENU.iter().map(|p| p.name.to_string()).collect(),
            Item::SimSlot => (1..=NREF).map(|i| format!("R{i}")).collect(),
            Item::SimPlay | Item::SimShow => vec!["Off".into(), "On".into()],
            Item::MeasSrc => self.meas_source_names(false),
            Item::MeasSrcB => self.meas_source_names(true),
            _ => return None,
        })
    }

    pub(super) fn sim_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::SimPreset => {
                let n = MENU.len() as i32;
                if n == 0 {
                    return true;
                }
                let i = (self.sim_preset as i32 + d).rem_euclid(n) as usize;
                if !MENU[i].enabled {
                    self.toast("That preset's analog fields are not copied");
                    return true;
                }
                self.sim_preset = i;
                if self.sim_play {
                    self.sim_write();
                }
            }
            Item::SimSlot => {
                self.sim_slot = (self.sim_slot as i32 + d).rem_euclid(NREF as i32) as usize;
                if self.sim_play {
                    self.sim_write();
                }
            }
            Item::SimPlay => {
                self.sim_play = !self.sim_play;
                if self.sim_play {
                    self.sim_write();
                } else {
                    self.sim_map.fill(0);
                    self.sim_map_z.fill(0);
                }
            }
            Item::SimShow => {
                let Some(r) = self.refs[self.sim_slot].as_mut() else {
                    self.toast("Nothing in that slot yet");
                    return true;
                };
                r.on = !r.on;
            }
            Item::MeasSrc => self.step_meas_source(false, d),
            Item::MeasSrcB => self.step_meas_source(true, d),
            _ => return false,
        }
        true
    }

    /// Channels, then every reference that holds samples.
    fn meas_choices(&self) -> Vec<WavePick> {
        let mut v: Vec<WavePick> = (0..n_ch()).map(WavePick::Ch).collect();
        v.extend(self.loaded_refs().into_iter().map(WavePick::Ref));
        v
    }

    fn loaded_refs(&self) -> Vec<usize> {
        self.refs.iter().enumerate().filter_map(|(i, r)| r.as_ref().filter(|w| !w.v.is_empty()).map(|_| i)).collect()
    }

    fn meas_source_names(&self, b: bool) -> Vec<String> {
        let _ = b;
        self.meas_choices().iter().map(|p| p.name()).collect()
    }

    pub(super) fn meas_source_label(&self, b: bool) -> String {
        let pick = if b { self.meas_pick_b() } else { self.meas_pick() };
        pick.name()
    }

    fn meas_pick(&self) -> WavePick {
        match self.meas_ref {
            Some(i) => WavePick::Ref(i),
            None => WavePick::Ch(self.meas_src.min(n_ch().saturating_sub(1))),
        }
    }

    fn meas_pick_b(&self) -> WavePick {
        match self.meas_ref_b {
            Some(i) => WavePick::Ref(i),
            None => WavePick::Ch(self.meas_src_b.min(n_ch().saturating_sub(1))),
        }
    }

    fn step_meas_source(&mut self, b: bool, d: i32) {
        let choices = self.meas_choices();
        if choices.is_empty() {
            return;
        }
        let cur = if b { self.meas_pick_b() } else { self.meas_pick() };
        let p = choices.iter().position(|c| *c == cur).unwrap_or(0) as i32;
        let next = choices[(p + d).rem_euclid(choices.len() as i32) as usize];
        match (b, next) {
            (false, WavePick::Ch(c)) => {
                self.meas_ref = None;
                self.meas_src = c;
            }
            (false, WavePick::Ref(i)) => {
                self.meas_ref = Some(i);
                self.measure_slot(i);
            }
            (true, WavePick::Ch(c)) => {
                self.meas_ref_b = None;
                self.meas_src_b = c;
            }
            (true, WavePick::Ref(i)) => {
                self.meas_ref_b = Some(i);
                self.measure_slot(i);
            }
        }
    }

    pub(super) fn eye_source_names(&self) -> Vec<String> {
        self.meas_choices().iter().map(|p| p.name()).collect()
    }

    pub(super) fn step_eye_source(&mut self, d: i32) {
        let choices = self.meas_choices();
        if choices.is_empty() {
            return;
        }
        let cur = match self.eye_ref {
            Some(i) => WavePick::Ref(i),
            None => WavePick::Ch(self.m.eye_src.min(n_ch().saturating_sub(1))),
        };
        let p = choices.iter().position(|c| *c == cur).unwrap_or(0) as i32;
        match choices[(p + d).rem_euclid(choices.len() as i32) as usize] {
            WavePick::Ch(c) => {
                self.eye_ref = None;
                self.m.eye_src = c;
                self.eye_fold = None;
                self.azr.eye = None;
            }
            WavePick::Ref(i) => {
                self.eye_ref = Some(i);
                self.eye_fold = None;
                self.eye_update_ref(i);
            }
        }
    }

    /// Writes the selected preset into the selected slot. Does not touch the time base,
    /// the delay, the live source, or the channel records.
    fn sim_write(&mut self) {
        if self.sim_preset != hss::PCIE3_INDEX || !MENU[hss::PCIE3_INDEX].enabled {
            self.toast("That preset's analog fields are not copied");
            self.sim_play = false;
            return;
        }
        let settings = hss::pcie3_nominal();
        let (dt, n) = hss::sample_grid(&settings);
        let wave = match hss::generate(&settings, dt, n) {
            Ok(v) => v,
            Err(e) => {
                self.toast(&e);
                self.sim_play = false;
                return;
            }
        };
        let (vpd, offset) = hss::vertical(&settings);
        let level = (0.5 * (settings.v_high_mv + settings.v_low_mv) * 1e-3) as f32;
        self.sim_gen = self.sim_gen.wrapping_add(1);
        let gen = self.sim_gen;
        let slot = self.sim_slot.min(NREF - 1);
        self.sim_slot = slot;
        self.refs[slot] = Some(RefWave {
            src: 0,
            color: 0,
            label: settings.name.to_string(),
            t0: 0.0,
            dt,
            v: wave,
            vpd,
            offset,
            on: true,
            sim_gen: gen,
            sim_level: level,
        });
        self.sim_cursor = 0.0;
        self.eye_fold = None;
        self.ref_sel = slot;
        self.measure_slot(slot);
        self.sim_meas_gen = gen;
        self.toast(&format!("{} on R{}, {} samples, one SSC period", settings.name, slot + 1, n));
    }

    pub(super) fn measure_slot(&mut self, slot: usize) {
        let Some(w) = self.refs.get(slot).and_then(|r| r.as_ref()) else { return };
        if w.v.is_empty() {
            return;
        }
        let min_amp = 0.4 * w.vpd as f64;
        let a = oos_dsp::measure::analyze_min(&w.v, w.dt, min_amp);
        self.ana_ref[slot] = a;
    }

    /// One play step. The stored samples stay put. The window is the open horizontal scale.
    pub(super) fn sim_frame(&mut self, dt_wall: f64, persist: f32) {
        if let Some(slot) = self.eye_ref {
            self.eye_update_ref(slot);
        }
        if !self.sim_play {
            return;
        }
        let slot = self.sim_slot;
        let (gen, dt, n, level, vpd, offset, show) = {
            let Some(w) = self.refs[slot].as_ref() else {
                self.sim_play = false;
                return;
            };
            if w.sim_gen == 0 || w.v.is_empty() || w.dt <= 0.0 {
                self.sim_play = false;
                return;
            }
            (w.sim_gen, w.dt, w.v.len(), w.sim_level, w.vpd, w.offset, w.on)
        };
        if self.sim_meas_gen != gen {
            self.measure_slot(slot);
            self.sim_meas_gen = gen;
        }
        let record = n as f64 * dt;
        let (tdiv, _) = self.open_view();
        let window = tdiv as f64 * HDIV as f64;
        let fits = record <= window + dt;
        let hyst = (0.1 * vpd).max(1e-6);
        let from = if fits { 0.0 } else { self.sim_cursor };
        let (ti, main_draw, zoom_draw) = {
            let Some(w) = self.refs[slot].as_ref() else { return };
            let v = w.v.as_slice();
            let ti = find_sim_edge(v, dt, level, hyst, from, window).or_else(|| find_sim_edge(v, dt, level, hyst, 0.0, window)).unwrap_or(0.0);
            let main = window_samples(v, dt, ti, self.m.time_per_div(), self.m.delay, self.ph.w);
            let zoom = if self.m.zoom { window_samples(v, dt, ti, tscales()[self.m.zscale], self.m.zdelay, self.phz.w) } else { None };
            (ti, main, zoom)
        };
        let f = if persist > 0.0 { (-dt_wall / persist as f64).exp() as f32 } else { 0.0 };
        if persist == 0.0 || fits {
            self.sim_map.fill(0);
            self.sim_map_z.fill(0);
        } else {
            decay_map(&mut self.sim_map, f);
            decay_map(&mut self.sim_map_z, f);
        }
        if show {
            if let Some((rec_t0, step_dt, draw)) = main_draw {
                self.deposit_sim(true, vpd, offset, self.m.time_per_div(), self.m.delay, rec_t0, step_dt, &draw);
            }
            if let Some((rec_t0, step_dt, draw)) = zoom_draw {
                self.deposit_sim(false, vpd, offset, tscales()[self.m.zscale], self.m.zdelay, rec_t0, step_dt, &draw);
            }
        }
        self.sim_vmax = self.sim_map.iter().copied().max().unwrap_or(0) as u32;
        self.sim_vmaxz = self.sim_map_z.iter().copied().max().unwrap_or(0) as u32;
        if self.sim_play && !fits {
            let advance = (window / dt).max(1.0);
            self.sim_cursor = ti + advance;
            if self.sim_cursor >= n as f64 - 4.0 {
                self.sim_cursor = 0.0;
            }
        }
    }

    fn open_view(&self) -> (f32, f32) {
        if self.m.zoom {
            (tscales()[self.m.zscale], self.m.zdelay)
        } else {
            (self.m.time_per_div(), self.m.delay)
        }
    }

    fn deposit_sim(&mut self, main: bool, vpd: f32, offset: f32, tdiv: f32, delay: f32, rec_t0: f64, dt: f64, draw: &[f32]) {
        let (w, h, map) = if main {
            (self.ph.w, self.ph.h, &mut self.sim_map)
        } else {
            (self.phz.w, self.phz.h, &mut self.sim_map_z)
        };
        deposit(map, w, h, self.m.dots, vpd, offset, tdiv, delay, rec_t0, dt, draw, 24);
    }

    pub(super) fn draw_sim_map(&self, cv: &mut Canvas, area: Rect, map: &[u16], vmax: u32) {
        if !self.sim_play || area.w <= 0 || area.h <= 0 {
            return;
        }
        let slot = self.sim_slot;
        let Some(w) = self.refs.get(slot).and_then(|r| r.as_ref()) else { return };
        if !w.on || map.len() != (area.w as usize) * (area.h as usize) {
            return;
        }
        let col = REF_COLORS[w.color.min(REF_COLORS.len() - 1)].1;
        let inten = self.m.intensity as f32 / 100.0;
        let floor = 30.0 + 140.0 * inten;
        let gain = 0.35 + 1.3 * inten;
        let (ww, hh) = (area.w as usize, area.h as usize);
        let vmax = vmax.max(1);
        let mut shift = 0;
        while (vmax >> shift) > 1023 {
            shift += 1;
        }
        let top = ((vmax >> shift).max(1)) as f32;
        let mut lut = [0u32; 1024];
        for (i, l) in lut.iter_mut().enumerate() {
            let x = (i as f32 + 0.5) / top;
            *l = if i == 0 && shift == 0 { 0 } else { (floor + (255.0 - floor) * (x * gain).min(1.0).sqrt()).min(255.0) as u32 };
        }
        let (cr, cg, cb) = ((col >> 16) & 255, (col >> 8) & 255, col & 255);
        for y in 0..hh {
            let row = &map[y * ww..(y + 1) * ww];
            let o = ((area.y + y as i32) * cv.w + area.x) as usize;
            for (x, &v) in row.iter().enumerate() {
                if v == 0 {
                    continue;
                }
                let a = lut[((v as usize) >> shift).min(1023)].max(floor as u32 / 2);
                let p = &mut cv.px[o + x];
                let d = *p;
                let r = (((d >> 16) & 255) * (255 - a) / 255 + cr * a / 255).min(255);
                let g = (((d >> 8) & 255) * (255 - a) / 255 + cg * a / 255).min(255);
                let b = ((d & 255) * (255 - a) / 255 + cb * a / 255).min(255);
                *p = rgb(r, g, b);
            }
        }
    }

    /// Still references, using the visible span of each slot's own sample interval.
    pub(super) fn draw_refs_span(&self, cv: &mut Canvas) {
        if self.m.zoom {
            let (o, z) = Self::split_zoom(self.grid);
            self.draw_refs_in(cv, o, self.m.time_per_div(), self.m.delay);
            self.draw_refs_in(cv, z, tscales()[self.m.zscale], self.m.zdelay);
        } else if let Some(a) = self.wins.first().copied() {
            self.draw_refs_in(cv, a, self.m.time_per_div(), self.m.delay);
        }
    }

    fn draw_refs_in(&self, cv: &mut Canvas, area: Rect, tdiv: f32, delay: f32) {
        if area.w <= 1 || area.h <= 1 {
            return;
        }
        let f = &self.fonts;
        cv.set_clip(area);
        let ppy = area.h as f32 / VDIV;
        let cy = area.y as f32 + area.h as f32 / 2.0;
        let tdiv = tdiv as f64;
        let delay = delay as f64;
        let half = tdiv * HDIV as f64 / 2.0;
        let ppx = area.w as f64 / HDIV as f64;
        for (i, r) in self.refs.iter().enumerate() {
            let Some(r) = r else { continue };
            if !r.on || (self.sim_play && i == self.sim_slot) || !(r.dt > 0.0) || r.v.is_empty() {
                continue;
            }
            let col = REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1;
            let t_to_k = |t: f64| -> usize {
                let x = (t - r.t0) / r.dt;
                if x <= 0.0 { 0 } else { (x.floor() as usize).min(r.v.len() - 1) }
            };
            let k0 = t_to_k(delay - half);
            let k1 = t_to_k(delay + half);
            if k0 > k1 {
                continue;
            }
            let span = (k1 - k0).max(1);
            let step = (span / (area.w as usize * 2).max(1)).max(1);
            let mut prev: Option<(i32, i32)> = None;
            let mut first = None;
            let mut k = k0;
            loop {
                let v = r.v[k];
                if v.is_finite() {
                    let t = r.t0 + k as f64 * r.dt - delay;
                    let x = (area.x as f64 + area.w as f64 / 2.0 + t / tdiv * ppx).round() as i32;
                    let y = (cy - (v + r.offset) / r.vpd.max(1e-12) * ppy).round() as i32;
                    if let Some((x0, y0)) = prev {
                        cv.line(x0, y0, x, y, col);
                    }
                    if first.is_none() && x >= area.x {
                        first = Some(y);
                    }
                    prev = Some((x, y));
                } else {
                    prev = None;
                }
                if k >= k1 {
                    break;
                }
                k = (k + step).min(k1);
                if step == 0 {
                    break;
                }
            }
            if let Some(y) = first {
                let yy = y.clamp(area.y + 10, area.y + area.h - 10);
                let name = if r.label.is_empty() { format!("R{}", i + 1) } else { r.label.clone() };
                let b = Rect::new(area.x + 80, yy - 8, (f.s.width(&name) + 8).max(24), 17);
                cv.round(b, 3, col, 255);
                cv.text_in(&f.s, b, 1, &name, rgb(0, 0, 0));
            }
        }
        cv.no_clip();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WavePick {
    Ch(usize),
    Ref(usize),
}

impl WavePick {
    fn name(self) -> String {
        match self {
            WavePick::Ch(c) => format!("CH{}", c + 1),
            WavePick::Ref(i) => format!("R{}", i + 1),
        }
    }
}

fn find_sim_edge(v: &[f32], dt: f64, level: f32, hyst: f32, from: f64, window: f64) -> Option<f64> {
    if v.len() < 4 || !(dt > 0.0) {
        return None;
    }
    let i0 = from.floor().max(0.0) as usize;
    if i0 + 2 >= v.len() {
        return None;
    }
    let span = ((window / dt).ceil() as usize).max(4096).min(v.len() - i0);
    let slice = &v[i0..i0 + span];
    let blank: &[f32] = &[];
    let inp = trigger::Input { ch: [slice, blank, blank, blank], dt, hyst: [hyst, 0.0, 0.0, 0.0] };
    let trig = Trigger::Edge { src: 0, slope: Slope::Rising, level };
    trigger::find_first(&trig, &inp, (from - i0 as f64).max(0.0)).map(|t| t + i0 as f64)
}

fn window_samples(v: &[f32], dt: f64, ti: f64, tdiv: f32, delay: f32, width: usize) -> Option<(f64, f64, Vec<f32>)> {
    if !(dt > 0.0) || !(tdiv > 0.0) || v.is_empty() {
        return None;
    }
    let half = tdiv as f64 * HDIV as f64 / 2.0;
    let k0 = (ti + (delay as f64 - half) / dt).floor().max(0.0) as usize;
    if k0 >= v.len() {
        return None;
    }
    let k1 = ((ti + (delay as f64 + half) / dt).ceil() as usize + 1).min(v.len());
    if k1 <= k0 {
        return None;
    }
    let step = ((k1 - k0) / (width.max(1) * 2)).max(1);
    let draw: Vec<f32> = v[k0..k1].iter().step_by(step).copied().collect();
    Some(((k0 as f64 - ti) * dt, dt * step as f64, draw))
}

fn onoff(b: bool) -> String {
    if b { "On".into() } else { "Off".into() }
}

fn decay_map(map: &mut [u16], f: f32) {
    let k = (f.clamp(0.0, 1.0) * 65536.0) as u32;
    if k >= 65536 {
        return;
    }
    for x in map.iter_mut() {
        if *x != 0 {
            *x = ((*x as u32 * k) >> 16) as u16;
        }
    }
}

fn deposit(map: &mut [u16], w: usize, h: usize, dots: bool, vpd: f32, offset: f32, tdiv: f32, delay: f32, rec_t0: f64, dt: f64, v: &[f32], weight: u16) {
    if w == 0 || h == 0 || map.len() != w * h || !(vpd > 0.0) || !(tdiv > 0.0) || !(dt > 0.0) {
        return;
    }
    let wd = w as f64;
    let ht = h as f32;
    let ppd_x = wd / HDIV as f64;
    let ppd_y = ht / VDIV;
    let cy = ht / 2.0;
    let tdiv = tdiv as f64;
    let delay = delay as f64;
    let x_of = |k: usize| -> f32 { (((rec_t0 + k as f64 * dt - delay) / tdiv) * ppd_x + wd / 2.0) as f32 };
    let y_of = |val: f32| -> f32 { cy - (val + offset) / vpd * ppd_y };
    let (wi, hi) = (w as i32, h as i32);
    let mut prev: Option<(f32, f32)> = None;
    for (k, &val) in v.iter().enumerate() {
        if !val.is_finite() {
            prev = None;
            continue;
        }
        let p = (x_of(k), y_of(val));
        if dots {
            let (x, y) = (p.0.floor() as i32, p.1.round() as i32);
            if (0..wi).contains(&x) && (0..hi).contains(&y) {
                let idx = (y * wi + x) as usize;
                map[idx] = map[idx].saturating_add(weight);
            }
            continue;
        }
        if let Some(q) = prev {
            let (x0, y0, x1, y1) = if q.0 <= p.0 { (q.0, q.1, p.0, p.1) } else { (p.0, p.1, q.0, q.1) };
            let cx0 = x0.floor() as i32;
            let cx1 = x1.floor() as i32;
            if cx1 >= 0 && cx0 < wi {
                for cx in cx0.max(0)..=cx1.min(wi - 1) {
                    let (ya, yb) = if cx0 == cx1 {
                        (y0, y1)
                    } else {
                        let ta = ((cx as f32 - x0) / (x1 - x0)).clamp(0.0, 1.0);
                        let tb = ((cx as f32 + 1.0 - x0) / (x1 - x0)).clamp(0.0, 1.0);
                        (y0 + (y1 - y0) * ta, y0 + (y1 - y0) * tb)
                    };
                    let (lo, hi_) = if ya < yb { (ya, yb) } else { (yb, ya) };
                    let r0 = (lo.round() as i32).max(0);
                    let r1 = (hi_.round() as i32).min(hi - 1);
                    let mut r = r0;
                    while r <= r1 {
                        let idx = (r * wi + cx) as usize;
                        map[idx] = map[idx].saturating_add(weight);
                        r += 1;
                    }
                }
            }
        }
        prev = Some(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn ui() -> Ui {
        Ui::new(Source::Demo { t: 0.0 })
    }

    fn put(u: &mut Ui, slot: usize, v: Vec<f32>, dt: f64, vpd: f32) {
        u.refs[slot] = Some(RefWave {
            src: 0,
            color: 0,
            label: "sine".into(),
            t0: 0.0,
            dt,
            v,
            vpd,
            offset: 0.0,
            on: true,
            sim_gen: 0,
            sim_level: 0.0,
        });
    }

    #[test]
    fn sim_follows_reference_in_the_start_menu() {
        let u = ui();
        let g = u.main_groups();
        let ana = g.iter().find(|g| g.0 == "Analysis").unwrap();
        let i = ana.1.iter().position(|it| *it == Item::Open(MenuId::Ref)).unwrap();
        assert_eq!(ana.1[i + 1], Item::Open(MenuId::Sim));
    }

    #[test]
    fn ref_sine_ignores_front_end_bandwidth() {
        let mut u = ui();
        let dt = 1e-6;
        let v: Vec<f32> = (0..8000).map(|k| (2.0 * std::f64::consts::PI * 1000.0 * k as f64 * dt).sin() as f32).collect();
        put(&mut u, 0, v, dt, 0.5);
        u.measure_slot(0);
        let f1 = u.ana_ref[0].get(oos_dsp::measure::Meas::Freq);
        assert!((f1 - 1000.0).abs() < 20.0, "{f1}");
        u.m.ch[0].bw = BW_20M;
        let stored = u.refs[0].as_ref().unwrap().v.clone();
        u.measure_slot(0);
        let f2 = u.ana_ref[0].get(oos_dsp::measure::Meas::Freq);
        assert!((f1 - f2).abs() < 1e-9);
        assert_eq!(stored, u.refs[0].as_ref().unwrap().v);
        assert!(matches!(u.src, Source::Demo { .. }));
        assert!(u.last[0].is_none());
    }

    #[test]
    fn eye_folds_a_ref_once() {
        let mut u = ui();
        let dt = 1.0e-9;
        let v: Vec<f32> = (0..20_000).map(|k| if (k / 8) % 2 == 0 { -0.5 } else { 0.5 }).collect();
        put(&mut u, 0, v, dt, 0.2);
        u.refs[0].as_mut().unwrap().sim_gen = 1;
        u.area = Rect::new(0, 40, 800, 480);
        u.eye_ref = Some(0);
        u.eye_update_ref(0);
        assert_eq!(u.azr.eye.as_ref().unwrap().records, 1);
        let hits: u64 = u.azr.eye.as_ref().unwrap().hits.iter().map(|&h| h as u64).sum();
        u.eye_update_ref(0);
        assert_eq!(u.azr.eye.as_ref().unwrap().records, 1);
        let hits2: u64 = u.azr.eye.as_ref().unwrap().hits.iter().map(|&h| h as u64).sum();
        assert_eq!(hits, hits2);
        assert!(hits > 0);
        let long = oos_dsp::eye::recover_clock(&u.refs[0].as_ref().unwrap().v, dt, 0.0, 0.02, None).unwrap();
        let short = oos_dsp::eye::recover_clock(&u.refs[0].as_ref().unwrap().v[..3600], dt, 0.0, 0.02, None).unwrap();
        assert!(long.edges.len() > short.edges.len());
    }

    #[test]
    fn play_writes_one_ref_and_leaves_the_live_source() {
        let mut u = ui();
        u.sim_preset = hss::PCIE3_INDEX;
        u.sim_slot = 0;
        u.sim_play = true;
        u.sim_write();
        let w = u.refs[0].as_ref().unwrap();
        let (dt, n) = hss::sample_grid(&hss::pcie3_nominal());
        assert_eq!(w.v.len(), n);
        assert!((w.dt - dt).abs() < 1e-24);
        assert!(w.on);
        assert_eq!(w.label, "PCIe 3.0");
        assert!(n as f64 * w.dt + 1e-15 >= 1.0 / 33.0e3);
        assert!(matches!(u.src, Source::Demo { .. }));
        assert!(u.last[0].is_none());
        u.m.tscale = 0;
        let before = u.sim_cursor;
        u.sim_frame(0.016, 0.0);
        assert!(u.sim_cursor > before);
        assert!(matches!(u.src, Source::Demo { .. }));
    }
}
