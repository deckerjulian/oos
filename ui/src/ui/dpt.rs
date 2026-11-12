// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Wide-bandgap double pulse page. E on and E off are the existing switching-loss pair, not recomputed here.

use super::magnetic::{clash_wave, missing_wave, needs_more_channels, num, wave_clash};
use super::*;
use oos_dsp::dpt::{self, Fourth};
use oos_dsp::power;

impl Ui {
    pub(super) fn dpt_items(&self) -> Vec<Item> {
        use Item::{Dpt4, DptFourth, DptG, DptI, DptOn, DptPct, DptV, DptZoom};
        let mut v = vec![DptOn, DptV, DptI, DptG, DptFourth];
        if self.m.dpt_4th != crate::model::DptFourth::Off {
            v.push(Dpt4);
        }
        v.extend([DptPct, DptZoom]);
        v
    }

    pub(super) fn dpt_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::DptPct => Kind::Value,
            Item::DptOn | Item::DptV | Item::DptI | Item::DptG | Item::DptFourth | Item::Dpt4 | Item::DptZoom => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn dpt_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::DptOn => "View",
            Item::DptV => "Vds",
            Item::DptI => "Id",
            Item::DptG => "Vgs",
            Item::DptFourth => "Fourth",
            Item::Dpt4 => "Channel",
            Item::DptPct => "Gate",
            Item::DptZoom => "Zoom",
            _ => return None,
        })
    }

    pub(super) fn dpt_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        Some(match it {
            Item::DptOn => if m.view == AView::Dpt { "On".into() } else { "Off".into() },
            Item::DptV => self.wave_name(m.dpt_v),
            Item::DptI => self.wave_name(m.dpt_i),
            Item::DptG => self.wave_name(m.dpt_g),
            Item::DptFourth => match m.dpt_4th {
                DptFourth::Off => "Off",
                DptFourth::Gate => "Gate",
                DptFourth::DiodeV => "Diode V",
                DptFourth::DiodeI => "Diode I",
            }
            .into(),
            Item::Dpt4 => self.wave_name(m.dpt_4ch),
            Item::DptPct => format!("{:.0} %", m.dpt_pct),
            Item::DptZoom => match m.dpt_zoom {
                DptZoom::Both => "Both",
                DptZoom::TurnOn => "Turn-on",
                DptZoom::TurnOff => "Turn-off",
            }
            .into(),
            _ => return None,
        })
    }

    pub(super) fn dpt_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |o: &[&str]| Some(o.iter().map(|s| s.to_string()).collect());
        match it {
            Item::DptOn => v(&["Off", "On"]),
            Item::DptV | Item::DptI | Item::DptG | Item::Dpt4 => Some(self.wave_names()),
            Item::DptFourth => v(&["Off", "Gate", "Diode V", "Diode I"]),
            Item::DptZoom => v(&["Both", "Turn-on", "Turn-off"]),
            _ => None,
        }
    }

    pub(super) fn dpt_adjust(&mut self, it: Item, d: i32) -> bool {
        if matches!(it, Item::DptV | Item::DptI | Item::DptG | Item::Dpt4) {
            let cur = match it {
                Item::DptV => self.m.dpt_v,
                Item::DptI => self.m.dpt_i,
                Item::DptG => self.m.dpt_g,
                _ => self.m.dpt_4ch,
            };
            let next = self.step_wave(cur, d);
            match it {
                Item::DptV => self.m.dpt_v = next,
                Item::DptI => self.m.dpt_i = next,
                Item::DptG => self.m.dpt_g = next,
                _ => self.m.dpt_4ch = next,
            }
            self.analyzer_update();
            return true;
        }
        let m = &mut self.m;
        match it {
            Item::DptOn => m.view = if m.view == AView::Dpt { AView::Scope } else { AView::Dpt },
            Item::DptFourth => {
                let order = [DptFourth::Off, DptFourth::Gate, DptFourth::DiodeV, DptFourth::DiodeI];
                let i = order.iter().position(|x| *x == m.dpt_4th).unwrap_or(0);
                let step = if d == 0 { 0 } else { d.signum() };
                m.dpt_4th = order[(i as i32 + step).rem_euclid(order.len() as i32) as usize];
            }
            Item::DptPct => m.dpt_pct = (m.dpt_pct + d as f64).clamp(1.0, 49.0),
            Item::DptZoom => {
                m.dpt_zoom = match (m.dpt_zoom, d >= 0) {
                    (DptZoom::Both, true) | (DptZoom::TurnOff, false) => DptZoom::TurnOn,
                    (DptZoom::TurnOn, true) | (DptZoom::Both, false) => DptZoom::TurnOff,
                    (DptZoom::TurnOff, true) | (DptZoom::TurnOn, false) => DptZoom::Both,
                };
            }
            _ => return false,
        }
        self.analyzer_update();
        true
    }

    pub(super) fn dpt_update(&mut self, rec: &Record) {
        self.azr.dpt = None;
        self.azr.dpt_loss = None;
        self.azr.dpt_note.clear();
        self.azr.dpt_v.clear();
        self.azr.dpt_i.clear();
        self.azr.dpt_g.clear();
        self.azr.dpt_t = (0.0, 0.0);
        let m = &self.m;
        let mut used = vec![m.dpt_v, m.dpt_i, m.dpt_g];
        if m.dpt_4th != DptFourth::Off {
            used.push(m.dpt_4ch);
        }
        if needs_more_channels(&used) {
            self.azr.dpt_note = "double pulse needs Vds, Id and Vgs".into();
            return;
        }
        if let Some(id) = wave_clash(&used) {
            self.azr.dpt_note = clash_wave(id);
            return;
        }
        let fourth_id = (m.dpt_4th != DptFourth::Off).then_some(m.dpt_4ch);
        let taken = match self.take_waves(rec, &[m.dpt_v, m.dpt_i], &[Some(m.dpt_g), fourth_id]) {
            Ok(t) => t,
            Err(id) => {
                self.azr.dpt_note = missing_wave(id);
                return;
            }
        };
        let (vds, id, dt) = (&taken.v[0], &taken.v[1], taken.dt);
        let gate = taken.opt[0].as_ref();
        let mut note = if gate.is_none() { missing_wave(m.dpt_g) } else { String::new() };
        let fourth_s = taken.opt[1].as_ref();
        if m.dpt_4th != DptFourth::Off && fourth_s.is_none() {
            if !note.is_empty() {
                note.push_str(". ");
            }
            note.push_str(&missing_wave(m.dpt_4ch));
        }
        if taken.moved {
            if !note.is_empty() {
                note.push_str(". ");
            }
            note.push_str("on the finest ref interval");
        }
        let fourth = match (m.dpt_4th, fourth_s) {
            (DptFourth::Gate, Some(v)) => Fourth::Gate(v),
            (DptFourth::DiodeV, Some(v)) => Fourth::DiodeV(v),
            (DptFourth::DiodeI, Some(v)) => Fourth::DiodeI(v),
            _ => Fourth::Off,
        };
        let pulse = dpt::measure(vds, id, gate.map(|g| g.as_slice()), fourth, dt, m.dpt_pct);
        if !pulse.note.is_empty() {
            if !note.is_empty() {
                note.push_str(". ");
            }
            note.push_str(pulse.note);
        }
        let sw = power::switching(vds, id, dt, 10.0);
        self.azr.dpt_loss = if sw.transitions.is_empty() { None } else { Some((sw.e_on, sw.e_off)) };
        let end = (vds.len().saturating_sub(1)) as f64 * dt;
        let (mut t0, mut t1) = match m.dpt_zoom {
            DptZoom::TurnOn => pulse.zoom_on,
            DptZoom::TurnOff => pulse.zoom_off,
            DptZoom::Both => pulse.zoom_both,
        };
        if !(t1 > t0) {
            t0 = 0.0;
            t1 = end.max(dt);
        }
        self.azr.dpt_t = (t0, t1);
        self.azr.dpt_v = columns(vds, dt, t0, t1);
        self.azr.dpt_i = columns(id, dt, t0, t1);
        self.azr.dpt_g = gate.map(|g| columns(g, dt, t0, t1)).unwrap_or_default();
        self.azr.dpt_note = note;
        self.azr.dpt = Some(pulse);
    }

    pub(super) fn draw_dpt(&self, cv: &mut Canvas) {
        let a = self.area;
        let fig_w = 250.min(a.w / 3);
        let plot = Rect::new(a.x, a.y, a.w - fig_w, a.h);
        let (t0, t1) = self.azr.dpt_t;
        cv.set_clip(plot);
        let bands = [
            (&self.azr.dpt_v, "Vds", self.wave_color(self.m.dpt_v)),
            (&self.azr.dpt_i, "Id", self.wave_color(self.m.dpt_i)),
            (&self.azr.dpt_g, "Vgs", self.wave_color(self.m.dpt_g)),
        ];
        let h = plot.h / 3;
        for (k, (cols, name, col)) in bands.iter().enumerate() {
            let band = Rect::new(plot.x, plot.y + k as i32 * h, plot.w, h - 4);
            if k > 0 {
                cv.line(band.x, band.y, band.x + band.w, band.y, LINE);
            }
            cv.text(&self.fonts.s, band.x + 6, band.y + 16, name, DIM);
            draw_cols(cv, band, cols, *col);
        }
        if let Some(p) = &self.azr.dpt {
            for t in [p.marks.gate10, p.marks.gate90_off, p.marks.i10, p.marks.i90, p.marks.i10_off, p.marks.i90_off, p.marks.vpeak, p.marks.ipeak, p.marks.trr_end].into_iter().flatten() {
                if t >= t0 && t <= t1 && t1 > t0 {
                    let x = plot.x + ((t - t0) / (t1 - t0) * plot.w as f64) as i32;
                    cv.line(x, plot.y, x, plot.y + plot.h, AMBER);
                }
            }
        }
        cv.no_clip();
        let mut lines = Vec::new();
        if let Some(p) = &self.azr.dpt {
            let loss = self.azr.dpt_loss;
            lines.push(format!("Vpeak {}   Ipeak {}", num(p.vpeak, "V"), num(p.ipeak, "A")));
            lines.push(format!("E on {}   E off {}", num(loss.map(|l| l.0), "J"), num(loss.map(|l| l.1), "J")));
            lines.push(format!("Td(on) {}   Tr {}   Ton {}", num(p.td_on, "s"), num(p.tr, "s"), num(p.ton, "s")));
            lines.push(format!("Td(off) {}   Tf {}   Toff {}", num(p.td_off, "s"), num(p.tf, "s"), num(p.toff, "s")));
            lines.push(format!("dv/dt {}   di/dt {}", num(p.dvdt, "V/s"), num(p.didt, "A/s")));
            if p.sample_limit {
                lines.push("10-90 is one sample".into());
            }
            lines.push(format!("Tdt {}   A {}   B {}", num(p.tdt, "s"), num(p.tdt_a, "s"), num(p.tdt_b, "s")));
            lines.push(format!("Trr {}   Qrr {}", num(p.trr, "s"), num(p.qrr, "C")));
            lines.push(format!("Err {}   Irrm {}", num(p.err, "J"), num(p.irrm, "A")));
            lines.push(format!("diode d/dt {}", num(p.diode_didt, "A/s")));
            lines.push(format!("Qoss {}{}", num(p.qoss, "C"), if p.qoss_approx { " approx" } else { "" }));
            if p.inverted {
                lines.push("current sign flipped".into());
            }
            if p.err_from_vds {
                lines.push("Err from Vds".into());
            }
            if p.diode_from_id {
                lines.push("recovery from Id".into());
            }
        }
        if !self.azr.dpt_note.is_empty() {
            lines.push(self.azr.dpt_note.clone());
        }
        if lines.is_empty() {
            lines.push("Double pulse".into());
        }
        self.panel(cv, a.x + a.w - fig_w + 6, a.y + 8, &lines, TEXT);
    }
}

/// Min and max in each of 800 columns, so a one-sample overshoot is not skipped.
fn columns(v: &[f32], dt: f64, t0: f64, t1: f64) -> Vec<(f32, f32)> {
    let n = 800;
    let mut cols = vec![(f32::NAN, f32::NAN); n];
    if !(t1 > t0) || !(dt > 0.0) {
        return cols;
    }
    for (i, &x) in v.iter().enumerate() {
        if !x.is_finite() {
            continue;
        }
        let t = i as f64 * dt;
        if t < t0 || t > t1 {
            continue;
        }
        let c = ((t - t0) / (t1 - t0) * (n as f64 - 1.0)).round() as usize;
        let c = c.min(n - 1);
        let (lo, hi) = cols[c];
        cols[c] = (if lo.is_nan() { x } else { lo.min(x) }, if hi.is_nan() { x } else { hi.max(x) });
    }
    cols
}

fn draw_cols(cv: &mut Canvas, band: Rect, cols: &[(f32, f32)], col: Rgb) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for &(a, b) in cols {
        if a.is_finite() {
            lo = lo.min(a);
            hi = hi.max(b);
        }
    }
    if !lo.is_finite() {
        return;
    }
    if hi - lo < 1e-6 {
        lo -= 1.0;
        hi += 1.0;
    }
    let pad = (hi - lo) * 0.08;
    lo -= pad;
    hi += pad;
    let n = cols.len().max(2) as i32;
    for (i, &(a, b)) in cols.iter().enumerate() {
        if !a.is_finite() {
            continue;
        }
        let x = band.x + (i as i32 * band.w) / (n - 1).max(1);
        let y0 = band.y + band.h - ((a - lo) / (hi - lo) * band.h as f32) as i32;
        let y1 = band.y + band.h - ((b - lo) / (hi - lo) * band.h as f32) as i32;
        cv.line(x, y0, x, y1, col);
    }
}
