// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Inverter pages: DQ0 and the mechanical shaft (`oos_dsp::motor`). Quadrature and Hall are logic lines.

use super::magnetic::{clash_wave, missing_wave, needs_more_channels, num, wave_clash};
use super::*;
use oos_dsp::motor::{self, Theta};

impl Ui {
    pub(super) fn inverter_items(&self) -> Vec<Item> {
        use Item::{InvA, InvAn, InvAng, InvB, InvC, InvCpr, InvDeg, InvEi, InvElec, InvEv, InvHa, InvHb, InvHc, InvOff, InvOn, InvPage, InvPoles, InvQa, InvQb, InvQi, InvQty, InvScale, InvSwap, InvTq};
        let m = &self.m;
        let mut v = vec![InvOn, InvPage];
        if m.inv_page == crate::model::InvPage::Dq0 {
            v.extend([InvA, InvB, InvC, InvQty, InvAng]);
            if m.inv_ang == InvAngle::Analog {
                v.push(InvAn);
            }
            if m.inv_ang == InvAngle::Phase || (m.inv_ang == InvAngle::Analog && m.inv_elec) || m.inv_ang == InvAngle::Off {
                v.push(InvPoles);
            }
            if m.inv_ang == InvAngle::Analog {
                v.extend([InvDeg, InvOff, InvElec]);
            }
            if m.inv_ang == InvAngle::Quad {
                v.extend([InvQa, InvQb, InvQi, InvCpr, InvSwap]);
            }
            if m.inv_ang == InvAngle::Hall {
                v.extend([InvHa, InvHb, InvHc]);
            }
        } else {
            v.push(InvAng);
            match if m.inv_ang == InvAngle::Off { InvAngle::Phase } else { m.inv_ang } {
                InvAngle::Phase | InvAngle::Off => v.extend([InvA, InvB, InvC, InvPoles]),
                InvAngle::Analog => {
                    v.push(InvAn);
                    v.extend([InvDeg, InvOff, InvElec]);
                    if m.inv_elec {
                        v.push(InvPoles);
                    }
                }
                InvAngle::Quad => v.extend([InvQa, InvQb, InvQi, InvCpr, InvSwap]),
                InvAngle::Hall => v.extend([InvHa, InvHb, InvHc]),
            }
            v.extend([InvTq, InvScale, InvEv, InvEi]);
        }
        v
    }

    pub(super) fn inverter_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::InvPoles | Item::InvScale | Item::InvDeg | Item::InvOff | Item::InvQa | Item::InvQb | Item::InvQi | Item::InvCpr | Item::InvHa | Item::InvHb | Item::InvHc => Kind::Value,
            Item::InvOn | Item::InvPage | Item::InvA | Item::InvB | Item::InvC | Item::InvAng | Item::InvAn | Item::InvQty | Item::InvTq | Item::InvEv | Item::InvEi | Item::InvElec | Item::InvSwap => Kind::Choice,
            _ => return None,
        })
    }

    pub(super) fn inverter_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::InvOn => "View",
            Item::InvPage => "Page",
            Item::InvA => "Phase A",
            Item::InvB => "Phase B",
            Item::InvC => "Phase C",
            Item::InvAng => "Angle",
            Item::InvPoles => "Poles",
            Item::InvAn => "Analog",
            Item::InvQty => "Signal",
            Item::InvTq => "Torque",
            Item::InvEv => "Bus V",
            Item::InvEi => "Bus I",
            Item::InvScale => "Scale",
            Item::InvDeg => "Deg/V",
            Item::InvOff => "Offset",
            Item::InvElec => "Angle is",
            Item::InvQa => "A lead",
            Item::InvQb => "B",
            Item::InvQi => "Index",
            Item::InvCpr => "Counts",
            Item::InvSwap => "Swap",
            Item::InvHa => "Hall A",
            Item::InvHb => "Hall B",
            Item::InvHc => "Hall C",
            _ => return None,
        })
    }

    pub(super) fn inverter_value(&self, it: Item) -> Option<String> {
        let m = &self.m;
        let onoff = |b: bool| if b { "On".into() } else { "Off".into() };
        let opt = |c: Option<WaveId>| c.map(|id| self.wave_name(id)).unwrap_or_else(|| "Off".into());
        let line = |n: u8| format!("D{}", n.min(15));
        Some(match it {
            Item::InvOn => onoff(m.view == AView::Inverter),
            Item::InvPage => if m.inv_page == InvPage::Dq0 { "DQ0".into() } else { "Mechanical".into() },
            Item::InvA => self.wave_name(m.inv_a),
            Item::InvB => self.wave_name(m.inv_b),
            Item::InvC => self.wave_name(m.inv_c),
            Item::InvAng => self.ang_name(),
            Item::InvPoles => format!("{}", m.inv_poles.max(1)),
            Item::InvAn => self.wave_name(m.inv_an),
            Item::InvQty => if m.inv_volt { "Voltage".into() } else { "Current".into() },
            Item::InvTq => opt(m.inv_tq),
            Item::InvEv => opt(m.inv_ev),
            Item::InvEi => opt(m.inv_ei),
            Item::InvScale => short_num(m.inv_scale),
            Item::InvDeg => format!("{} \u{b0}/V", short_num(m.inv_degv)),
            Item::InvOff => format!("{} \u{b0}", short_num(m.inv_off)),
            Item::InvElec => if m.inv_elec { "Elec".into() } else { "Mech".into() },
            Item::InvQa => line(m.inv_qa),
            Item::InvQb => line(m.inv_qb),
            Item::InvQi => m.inv_qi.map(line).unwrap_or_else(|| "Off".into()),
            Item::InvCpr => format!("{:.0}", m.inv_cpr),
            Item::InvSwap => onoff(m.inv_swap),
            Item::InvHa => line(m.inv_ha),
            Item::InvHb => line(m.inv_hb),
            Item::InvHc => line(m.inv_hc),
            _ => return None,
        })
    }

    pub(super) fn inverter_options(&self, it: Item) -> Option<Vec<String>> {
        let v = |o: &[&str]| Some(o.iter().map(|s| s.to_string()).collect());
        match it {
            Item::InvOn | Item::InvSwap => v(&["Off", "On"]),
            Item::InvPage => v(&["DQ0", "Mechanical"]),
            Item::InvA | Item::InvB | Item::InvC | Item::InvAn => Some(self.wave_names()),
            Item::InvAng => Some(if self.m.inv_page == InvPage::Dq0 { vec!["Off".into(), "Phase".into(), "Analog".into(), "Quad".into(), "Hall".into()] } else { vec!["Phase".into(), "Analog".into(), "Quad".into(), "Hall".into()] }),
            Item::InvQty => v(&["Voltage", "Current"]),
            Item::InvTq | Item::InvEv | Item::InvEi => Some(self.wave_names_opt()),
            Item::InvElec => v(&["Mech", "Elec"]),
            _ => None,
        }
    }

    fn ang_name(&self) -> String {
        let a = if self.m.inv_page == InvPage::Mech && self.m.inv_ang == InvAngle::Off { InvAngle::Phase } else { self.m.inv_ang };
        match a {
            InvAngle::Off => "Off",
            InvAngle::Phase => "Phase",
            InvAngle::Analog => "Analog",
            InvAngle::Quad => "Quad",
            InvAngle::Hall => "Hall",
        }
        .into()
    }

    fn set_ang(&mut self, name: &str) {
        self.m.inv_ang = match name {
            "Phase" => InvAngle::Phase,
            "Analog" => InvAngle::Analog,
            "Quad" => InvAngle::Quad,
            "Hall" => InvAngle::Hall,
            _ => InvAngle::Off,
        };
    }

    pub(super) fn inverter_adjust(&mut self, it: Item, d: i32) -> bool {
        let opts = self.inverter_options(it);
        let pick = |cur: &str, opts: &[String]| {
            let i = opts.iter().position(|o| o == cur).unwrap_or(0);
            opts[(i as i32 + d).rem_euclid(opts.len() as i32) as usize].clone()
        };
        match it {
            Item::InvOn => self.m.view = if self.m.view == AView::Inverter { AView::Scope } else { AView::Inverter },
            Item::InvPage => {
                self.m.inv_page = if self.m.inv_page == InvPage::Dq0 { InvPage::Mech } else { InvPage::Dq0 };
                if self.m.inv_page == InvPage::Mech && self.m.inv_ang == InvAngle::Off {
                    self.m.inv_ang = InvAngle::Phase;
                }
            }
            Item::InvA => {
                let cur = self.m.inv_a;
                self.m.inv_a = self.step_wave(cur, d);
            }
            Item::InvB => {
                let cur = self.m.inv_b;
                self.m.inv_b = self.step_wave(cur, d);
            }
            Item::InvC => {
                let cur = self.m.inv_c;
                self.m.inv_c = self.step_wave(cur, d);
            }
            Item::InvAn => {
                let cur = self.m.inv_an;
                self.m.inv_an = self.step_wave(cur, d);
            }
            Item::InvAng => {
                let cur = self.ang_name();
                let name = pick(&cur, opts.as_deref().unwrap_or(&[]));
                self.set_ang(&name);
            }
            Item::InvPoles => self.m.inv_poles = (self.m.inv_poles as i32 + d).clamp(1, 32) as u32,
            Item::InvQty => self.m.inv_volt = !self.m.inv_volt,
            Item::InvTq => {
                let cur = self.m.inv_tq;
                self.m.inv_tq = self.step_wave_opt(cur, d);
            }
            Item::InvEv => {
                let cur = self.m.inv_ev;
                self.m.inv_ev = self.step_wave_opt(cur, d);
            }
            Item::InvEi => {
                let cur = self.m.inv_ei;
                self.m.inv_ei = self.step_wave_opt(cur, d);
            }
            Item::InvScale => self.m.inv_scale = eng_ladder(self.m.inv_scale, d, 1e-3, 1e3, false),
            Item::InvDeg => self.m.inv_degv = eng_ladder(self.m.inv_degv, d, 1e-3, 1e4, false),
            Item::InvOff => self.m.inv_off = (self.m.inv_off + d as f64).clamp(-3600.0, 3600.0),
            Item::InvElec => self.m.inv_elec = !self.m.inv_elec,
            Item::InvQa => self.m.inv_qa = step_line(self.m.inv_qa, d),
            Item::InvQb => self.m.inv_qb = step_line(self.m.inv_qb, d),
            Item::InvQi => self.m.inv_qi = step_index(self.m.inv_qi, d),
            Item::InvCpr => self.m.inv_cpr = (self.m.inv_cpr + d as f64).clamp(1.0, 100_000.0),
            Item::InvSwap => self.m.inv_swap = !self.m.inv_swap,
            Item::InvHa => self.m.inv_ha = step_line(self.m.inv_ha, d),
            Item::InvHb => self.m.inv_hb = step_line(self.m.inv_hb, d),
            Item::InvHc => self.m.inv_hc = step_line(self.m.inv_hc, d),
            _ => return false,
        }
        self.analyzer_update();
        true
    }

    pub(super) fn inverter_update(&mut self, rec: &Record) {
        self.azr.dq = None;
        self.azr.mech = None;
        self.azr.inv_note.clear();
        if self.m.inv_page == InvPage::Mech && self.m.inv_ang == InvAngle::Off {
            self.m.inv_ang = InvAngle::Phase;
        }
        if self.m.inv_page == InvPage::Dq0 {
            self.dq_update(rec);
        } else {
            self.mech_update(rec);
        }
    }

    fn dq_update(&mut self, rec: &Record) {
        let m = &self.m;
        let mut used = vec![m.inv_a, m.inv_b, m.inv_c];
        if m.inv_ang == InvAngle::Analog {
            used.push(m.inv_an);
        }
        if needs_more_channels(&used[..3]) {
            self.azr.inv_note = "DQ0 needs three analog channels".into();
            return;
        }
        if let Some(id) = wave_clash(&used) {
            self.azr.inv_note = clash_wave(id);
            return;
        }
        let analog = (m.inv_ang == InvAngle::Analog).then_some(m.inv_an);
        let (degv, off, elec, poles) = (m.inv_degv, m.inv_off, m.inv_elec, m.inv_poles.max(1));
        let ang = m.inv_ang;
        let taken = match self.take_waves(rec, &used[..3], &[analog]) {
            Ok(t) => t,
            Err(id) => {
                self.azr.inv_note = missing_wave(id);
                return;
            }
        };
        if analog.is_some() && taken.opt[0].is_none() {
            self.azr.inv_note = missing_wave(m.inv_an);
            return;
        }
        let (a, b, c, dt, t0) = (&taken.v[0], &taken.v[1], &taken.v[2], taken.dt, taken.t0);
        let owned;
        let theta = match ang {
            InvAngle::Off | InvAngle::Phase => Theta::Vector,
            InvAngle::Analog => {
                let v = taken.opt[0].as_deref().unwrap();
                owned = motor::analog_elec(v, degv, off, elec, poles);
                Theta::Elec(&owned)
            }
            InvAngle::Quad | InvAngle::Hall => {
                let Some(rad) = self.shaft_rad(rec, a.len(), dt, t0) else { return };
                owned = rad.iter().map(|r| if r.is_finite() { r * poles as f64 } else { f64::NAN }).collect();
                Theta::Elec(&owned)
            }
        };
        match motor::dq0(a, b, c, dt, theta) {
            Some(q) => {
                let mut note = String::new();
                if q.short {
                    note = "cycle window is short".into();
                }
                if taken.moved {
                    if !note.is_empty() {
                        note.push_str(". ");
                    }
                    note.push_str("on the finest ref interval");
                }
                self.azr.inv_note = note;
                self.azr.dq = Some(q);
            }
            None => self.azr.inv_note = "no DQ0".into(),
        }
    }

    fn mech_update(&mut self, rec: &Record) {
        let m = &self.m;
        let mut used = Vec::new();
        match m.inv_ang {
            InvAngle::Phase | InvAngle::Off => used.extend([m.inv_a, m.inv_b, m.inv_c]),
            InvAngle::Analog => used.push(m.inv_an),
            InvAngle::Quad | InvAngle::Hall => {}
        }
        let roles = [m.inv_tq, m.inv_ev, m.inv_ei];
        for c in roles.into_iter().flatten() {
            used.push(c);
        }
        if needs_more_channels(&used[..used.len().min(3)]) && matches!(m.inv_ang, InvAngle::Phase | InvAngle::Off) {
            self.azr.inv_note = "a phase angle needs three analog channels".into();
            return;
        }
        if let Some(id) = wave_clash(&used) {
            self.azr.inv_note = clash_wave(id);
            return;
        }
        if matches!(m.inv_ang, InvAngle::Quad | InvAngle::Hall) {
            return self.mech_digital(rec);
        }
        let (poles, degv, off, elec, ang) = (m.inv_poles.max(1), m.inv_degv, m.inv_off, m.inv_elec, m.inv_ang);
        let nreq = if ang == InvAngle::Analog { 1 } else { 3 };
        let taken = match self.take_waves(rec, &used[..nreq], &roles) {
            Ok(t) => t,
            Err(id) => {
                self.azr.inv_note = missing_wave(id);
                return;
            }
        };
        for (slot, id) in roles.into_iter().enumerate() {
            if id.is_some() && taken.opt[slot].is_none() {
                self.azr.inv_note = missing_wave(id.unwrap());
                return;
            }
        }
        let shaft = match ang {
            InvAngle::Phase | InvAngle::Off => motor::shaft_phase(&taken.v[0], &taken.v[1], &taken.v[2], taken.dt, taken.t0, poles),
            InvAngle::Analog => motor::shaft_analog(&taken.v[0], taken.dt, taken.t0, degv, off, elec, poles),
            InvAngle::Quad | InvAngle::Hall => return,
        };
        let moved = taken.moved;
        self.finish_mech(shaft, taken.opt[0].as_deref(), taken.opt[1].as_deref(), taken.opt[2].as_deref(), moved);
    }

    fn mech_digital(&mut self, rec: &Record) {
        if !crate::ui::la::available() {
            self.azr.inv_note = "this model has no logic port".into();
            return;
        }
        let Some(la) = rec.la.as_ref() else {
            self.azr.inv_note = "no logic record".into();
            return;
        };
        if la.words.is_empty() || !(la.dt > 0.0) {
            self.azr.inv_note = "no logic record".into();
            return;
        }
        let m = &self.m;
        let bit = |line: u8| la.words.iter().map(|w| w >> line & 1 == 1).collect::<Vec<_>>();
        let shaft = if m.inv_ang == InvAngle::Hall {
            motor::shaft_hall(&bit(m.inv_ha), &bit(m.inv_hb), &bit(m.inv_hc), la.dt, la.t0)
        } else {
            let ix = m.inv_qi.map(|q| bit(q));
            motor::shaft_quad(&bit(m.inv_qa), &bit(m.inv_qb), ix.as_deref(), la.dt, la.t0, m.inv_cpr, m.inv_swap)
        };
        let roles = [self.m.inv_tq, self.m.inv_ev, self.m.inv_ei];
        let taken = match self.take_waves(rec, &[], &roles) {
            Ok(t) => t,
            Err(id) => {
                self.azr.inv_note = missing_wave(id);
                return;
            }
        };
        for (slot, id) in roles.into_iter().enumerate() {
            if id.is_some() && taken.opt[slot].is_none() {
                self.azr.inv_note = missing_wave(id.unwrap());
                return;
            }
        }
        self.finish_mech(shaft, taken.opt[0].as_deref(), taken.opt[1].as_deref(), taken.opt[2].as_deref(), taken.moved);
    }

    fn finish_mech(&mut self, shaft: oos_dsp::motor::Shaft, tq: Option<&[f32]>, ev: Option<&[f32]>, ei: Option<&[f32]>, moved: bool) {
        let mut note = shaft.note.to_string();
        if moved {
            if !note.is_empty() {
                note.push_str(". ");
            }
            note.push_str("on the finest ref interval");
        }
        let mech = motor::mechanical(shaft, tq, self.m.inv_scale, ev, ei);
        self.azr.inv_note = note;
        self.azr.mech = Some(mech);
    }

    /// Mechanical radians of the quadrature or Hall source, one per phase sample.
    fn shaft_rad(&mut self, rec: &Record, n: usize, dt: f64, t0: f64) -> Option<Vec<f64>> {
        if !crate::ui::la::available() {
            self.azr.inv_note = "this model has no logic port".into();
            return None;
        }
        let Some(la) = rec.la.as_ref() else {
            self.azr.inv_note = "no logic record".into();
            return None;
        };
        if la.words.is_empty() || !(la.dt > 0.0) || n < 2 || !(dt > 0.0) {
            self.azr.inv_note = "no logic record".into();
            return None;
        }
        let m = &self.m;
        let at = |line: u8, i: usize| -> bool {
            let t = t0 + i as f64 * dt;
            let k = ((t - la.t0) / la.dt).round();
            if k < 0.0 {
                return false;
            }
            let k = k as usize;
            la.words.get(k).is_some_and(|w| w >> line & 1 == 1)
        };
        let col = |line: u8| (0..n).map(|i| at(line, i)).collect::<Vec<_>>();
        let shaft = if m.inv_ang == InvAngle::Hall {
            motor::shaft_hall(&col(m.inv_ha), &col(m.inv_hb), &col(m.inv_hc), dt, t0)
        } else {
            let ix = m.inv_qi.map(|q| col(q));
            motor::shaft_quad(&col(m.inv_qa), &col(m.inv_qb), ix.as_deref(), dt, t0, m.inv_cpr, m.inv_swap)
        };
        if shaft.rad.is_empty() {
            self.azr.inv_note = if shaft.note.is_empty() { "no angle".into() } else { shaft.note.into() };
            return None;
        }
        Some(shaft.rad)
    }

    pub(super) fn draw_inverter(&self, cv: &mut Canvas) {
        if self.m.inv_page == InvPage::Dq0 {
            self.draw_dq(cv);
        } else {
            self.draw_mech(cv);
        }
    }

    fn draw_dq(&self, cv: &mut Canvas) {
        let a = self.area;
        let f = &self.fonts();
        let mut lines = vec![format!("{}   angle {}", if self.m.inv_volt { "voltage" } else { "current" }, self.ang_name())];
        if let Some(q) = &self.azr.dq {
            lines.push(format!("D {}   Q {}   zero {}", num(Some(q.d), ""), num(Some(q.q), ""), num(Some(q.zero), "")));
            lines.push(format!("f {}", if q.freq > 0.0 { num(Some(q.freq), "Hz") } else { "----".into() }));
            if q.from_vector {
                lines.push("\u{3b8} from the phase vector".into());
            }
            cv.set_clip(a);
            let plot = Rect::new(a.x, a.y + 78, a.w - 168, a.h - 86);
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for &(d, qv, z) in &q.wave {
                for v in [d, qv, z] {
                    if v.is_finite() {
                        lo = lo.min(v);
                        hi = hi.max(v);
                    }
                }
            }
            let (lo, hi) = pad(lo, hi);
            let col = [CH_COLORS[0], CH_COLORS[1], CH_COLORS[2]];
            for k in 0..3 {
                let n = q.wave.len().max(2);
                let mut prev: Option<(i32, i32)> = None;
                for (i, p) in q.wave.iter().enumerate() {
                    let yv = [p.0, p.1, p.2][k];
                    if !yv.is_finite() {
                        prev = None;
                        continue;
                    }
                    let x = plot.x + (i as i32 * plot.w) / (n as i32 - 1).max(1);
                    let y = plot.y + plot.h - ((yv - lo) / (hi - lo) * plot.h as f64) as i32;
                    if let Some((px, py)) = prev {
                        cv.line(px, py, x, y, col[k]);
                    }
                    prev = Some((x, y));
                }
            }
            cv.text(&f.s, plot.x + 4, plot.y + 14, "D  Q  zero", DIM);
            let s = 150.min(a.h / 2);
            let sq = Rect::new(a.x + a.w - s - 8, a.y + a.h - s - 8, s, s);
            let mut r = 1.0_f64;
            for &(al, be) in &q.ab {
                r = r.max(al.abs()).max(be.abs());
            }
            let cx = sq.x + sq.w / 2;
            let cy = sq.y + sq.h / 2;
            cv.line(sq.x, cy, sq.x + sq.w, cy, LINE);
            cv.line(cx, sq.y, cx, sq.y + sq.h, LINE);
            let mut prev = None;
            for &(al, be) in &q.ab {
                if !al.is_finite() || !be.is_finite() {
                    prev = None;
                    continue;
                }
                let x = cx + (al / r * (sq.w as f64 / 2.0 - 4.0)) as i32;
                let y = cy - (be / r * (sq.h as f64 / 2.0 - 4.0)) as i32;
                if let Some((px, py)) = prev {
                    cv.line(px, py, x, y, CH_COLORS[0]);
                }
                prev = Some((x, y));
            }
            cv.text(&f.s, sq.x + 4, sq.y + 14, "\u{3b1} \u{3b2}", DIM);
            cv.no_clip();
        }
        if !self.azr.inv_note.is_empty() {
            lines.push(self.azr.inv_note.clone());
        }
        self.panel(cv, a.x + 8, a.y + 8, &lines, TEXT);
    }

    fn draw_mech(&self, cv: &mut Canvas) {
        let a = self.area;
        let mut lines = vec![format!("angle {}", self.ang_name())];
        if let Some(me) = &self.azr.mech {
            let s = &me.shaft;
            let sync = if s.synchronous { " synchronous" } else { "" };
            lines.push(format!("speed {}{}   accel {}", num(s.rpm, "rpm"), sync, num(s.accel, "rpm/s")));
            lines.push(format!("{:?}", s.direction));
            lines.push(format!("angle {}   travel {}", num(s.angle_deg, "\u{b0}"), num(s.travel_deg, "\u{b0}")));
            lines.push(format!("torque {}   Pmech {}", num(me.torque, "N\u{b7}m"), num(me.p_mech, "W")));
            lines.push(format!("Pelec {}   eff {}", num(me.p_elec, "W"), me.eff.map(|e| format!("{:.1} %", e * 100.0)).unwrap_or_else(|| "----".into())));
            if me.generating {
                lines.push("generating".into());
            }
            if s.no_index {
                lines.push("no index yet".into());
            }
            if s.hall_faults > 0 {
                lines.push(format!("Hall faults {}", s.hall_faults));
            }
            if !s.trend.is_empty() {
                cv.set_clip(a);
                let top = a.y + 28 + 18 * lines.len() as i32;
                let plot = Rect::new(a.x + 8, top, a.w - 16, (a.y + a.h - 8 - top).max(20));
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for &(_, r) in &s.trend {
                    if r.is_finite() {
                        lo = lo.min(r);
                        hi = hi.max(r);
                    }
                }
                let (lo, hi) = pad(lo, hi);
                let n = s.trend.len().max(2);
                let mut prev = None;
                for (i, &(_, r)) in s.trend.iter().enumerate() {
                    if !r.is_finite() {
                        prev = None;
                        continue;
                    }
                    let x = plot.x + (i as i32 * plot.w) / (n as i32 - 1).max(1);
                    let y = plot.y + plot.h - ((r - lo) / (hi - lo) * plot.h as f64) as i32;
                    if let Some((px, py)) = prev {
                        cv.line(px, py, x, y, CH_COLORS[1]);
                    }
                    prev = Some((x, y));
                }
                cv.text(&self.fonts().s, plot.x, plot.y + 14, "rpm / rev", DIM);
                cv.no_clip();
            }
        }
        if !self.azr.inv_note.is_empty() {
            lines.push(self.azr.inv_note.clone());
        }
        self.panel(cv, a.x + 8, a.y + 8, &lines, TEXT);
    }
}

fn short_num(x: f64) -> String {
    if !x.is_finite() {
        return "----".into();
    }
    if x.abs() >= 100.0 {
        format!("{:.0}", x)
    } else if x.abs() >= 10.0 {
        format!("{:.1}", x)
    } else {
        format!("{:.3}", x)
    }
}

fn step_line(n: u8, d: i32) -> u8 {
    (n as i32 + d).rem_euclid(16) as u8
}

fn step_index(n: Option<u8>, d: i32) -> Option<u8> {
    let i = n.map(|x| x as i32 + 1).unwrap_or(0);
    let j = (i + d).rem_euclid(17);
    if j == 0 { None } else { Some((j - 1) as u8) }
}

fn pad(lo: f64, hi: f64) -> (f64, f64) {
    if !lo.is_finite() || !hi.is_finite() || hi - lo < 1e-15 {
        let c = if lo.is_finite() { lo } else { 0.0 };
        return (c - 1.0, c + 1.0);
    }
    let p = (hi - lo) * 0.08;
    (lo - p, hi + p)
}
