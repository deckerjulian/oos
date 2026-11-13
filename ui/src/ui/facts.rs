// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Numbers a controller can ask for: one measurement, and the figures on the power, spectrum, magnetic, inverter,
//! double-pulse and bandwidth pages. A source is a channel or a ref slot. A missing wave is an empty number.

use super::*;
use oos_dsp::bandwidth;
use oos_dsp::dpt::{self, Fourth};
use oos_dsp::magnetic::{self, Geom};
use oos_dsp::measure::{self, setup_hold_violations, Analysis, Meas};
use oos_dsp::motor::{self, Theta};
use oos_dsp::power;
use oos_dsp::spectrum;

pub(crate) enum Fact {
    Num(f64),
    Word(&'static str),
}

impl Ui {
    fn on_rec<T>(&self, f: impl FnOnce(&Record) -> T) -> T {
        if let Some(r) = self.last[0].as_ref() {
            f(r)
        } else {
            f(&Self::blank_record())
        }
    }

    fn min_amp(&self, id: WaveId) -> f64 {
        match id {
            WaveId::Ch(c) => 0.4 * self.m.ch[c.min(n_ch() - 1)].volts_per_div() as f64,
            WaveId::Ref(i) => self.refs.get(i).and_then(|r| r.as_ref()).map(|r| 0.4 * r.vpd as f64).unwrap_or(0.0),
        }
    }

    /// Analysis of one source, and the time of its first sample. A channel must be on. A ref may be hidden.
    fn analysis_of(&self, id: WaveId) -> Option<(Analysis, f64)> {
        if let WaveId::Ch(c) = id {
            if c >= n_ch() || !self.m.ch[c].on {
                return None;
            }
        }
        self.on_rec(|rec| {
            let s = self.wave_samples(rec, id)?;
            Some((measure::analyze_refs(&s.v, s.dt, self.min_amp(id), self.m.meas_lpct, self.m.meas_hpct), s.t0))
        })
    }

    pub(crate) fn meas_pair_sources(&self) -> (WaveId, WaveId) {
        (self.meas_src, self.meas_src_b)
    }

    pub(crate) fn fact_meas(&self, k: Meas, id: WaveId) -> f64 {
        match self.analysis_of(id) {
            Some((a, _)) => self.meas_of(&a, k),
            None => f64::NAN,
        }
    }

    /// Rising-clock edges that miss the configured setup or hold limit.
    pub(crate) fn fact_setup_hold(&self, clk: WaveId, data: WaveId) -> f64 {
        let (Some((ca, ta)), Some((da, tb))) = (self.analysis_of(clk), self.analysis_of(data)) else {
            return f64::NAN;
        };
        let (ca, da) = (shift_t0(&ca, ta), shift_t0(&da, tb));
        setup_hold_violations(&ca, &da, self.m.meas_tsetup, self.m.meas_thold)
    }

    fn grid(&self, req: &[WaveId], opt: &[Option<WaveId>]) -> Option<(f64, f64, Vec<Vec<f32>>, Vec<Option<Vec<f32>>>)> {
        self.on_rec(|rec| {
            let t = self.take_waves(rec, req, opt).ok()?;
            Some((t.t0, t.dt, t.v, t.opt))
        })
    }

    fn one_id(over: &[WaveId], fallback: WaveId) -> Result<WaveId, ()> {
        match over {
            [] => Ok(fallback),
            [a] => Ok(*a),
            _ => Err(()),
        }
    }

    fn two_id(over: &[WaveId], a: WaveId, b: WaveId) -> Result<(WaveId, WaveId), ()> {
        match over {
            [] => Ok((a, b)),
            [x, y] => Ok((*x, *y)),
            _ => Err(()),
        }
    }

    pub(crate) fn fact_power(&self, item: &str, over: &[WaveId]) -> Result<f64, ()> {
        let miss = f64::NAN;
        let q = |v: WaveId, i: WaveId, ui: &Ui| {
            let (_, dt, w, _) = ui.grid(&[v, i], &[])?;
            Some(power::quality(&w[0], &w[1], dt, None))
        };
        let volt = |ui: &Ui, id: WaveId| {
            let (_, dt, w, _) = ui.grid(&[id], &[])?;
            Some((w.into_iter().next()?, dt))
        };
        Ok(match item {
            "VRMS" | "IRMS" | "REAL" | "APP" | "REAC" | "PF" | "PHAS" | "FREQ" | "CREST" | "CRESTI" => {
                let (v, i) = Self::two_id(over, self.m.pw_v, self.m.pw_i)?;
                let Some(q) = q(v, i, self) else { return Ok(miss) };
                match item {
                    "VRMS" => q.vrms,
                    "IRMS" => q.irms,
                    "REAL" => q.p_real,
                    "APP" => q.s_apparent,
                    "REAC" => q.q_reactive,
                    "PF" => q.pf,
                    "PHAS" => q.phase_deg,
                    "FREQ" => q.freq,
                    "CREST" => q.crest_v,
                    "CRESTI" => q.crest_i,
                    _ => miss,
                }
            }
            "EON" | "EOFF" | "ECOND" | "PAVG" | "DVDTR" | "DVDTF" | "DIDTR" | "DIDTF" | "RDSON" => {
                let (v, i) = Self::two_id(over, self.m.pw_v, self.m.pw_i)?;
                let Some((_, dt, w, _)) = self.grid(&[v, i], &[]) else { return Ok(miss) };
                let s = power::switching(&w[0], &w[1], dt, 10.0);
                match item {
                    "EON" => s.e_on,
                    "EOFF" => s.e_off,
                    "ECOND" => s.e_cond,
                    "PAVG" => s.p_avg,
                    "DVDTR" => s.dvdt_rise,
                    "DVDTF" => s.dvdt_fall,
                    "DIDTR" => s.didt_rise,
                    "DIDTF" => s.didt_fall,
                    "RDSON" => s.rds_on,
                    _ => miss,
                }
            }
            "RIPPLE" | "ACRMS" | "LINE" | "LINER" | "SWITCH" | "SWRMS" => {
                let id = Self::one_id(over, self.m.pw_v)?;
                let Some((v, dt)) = volt(self, id) else { return Ok(miss) };
                match item {
                    "RIPPLE" => power::ripple(&v).0,
                    "ACRMS" => power::ripple(&v).1,
                    "LINE" | "LINER" => {
                        let fl = if self.m.pw_fline > 0.0 { self.m.pw_fline } else { power::line_hz(&v, dt) };
                        let t = power::tone_ripple(&v, dt, fl);
                        if item == "LINE" { t.pp } else { t.rms }
                    }
                    _ => {
                        let fs = if self.m.pw_fsw > 0.0 { self.m.pw_fsw } else { power::switch_hz(&v, dt) };
                        let t = power::switching_ripple(&v, dt, fs);
                        if item == "SWITCH" { t.pp } else { t.rms }
                    }
                }
            }
            "INRUSH" | "I2T" => {
                let id = Self::one_id(over, self.m.pw_i)?;
                let Some((i, dt)) = volt(self, id) else { return Ok(miss) };
                let (pk, _, i2t) = power::inrush(&i, dt);
                if item == "INRUSH" { pk } else { i2t }
            }
            "TON" | "TOFF" => {
                let (vin, vout) = match over {
                    [] => (self.m.pw_vin, self.m.pw_v),
                    [a, b] => (Some(*a), *b),
                    _ => return Err(()),
                };
                let Some(vin) = vin else { return Ok(miss) };
                let Some((_, dt, w, _)) = self.grid(&[vin, vout], &[]) else { return Ok(miss) };
                if item == "TON" { power::turn_on_time(&w[0], &w[1], dt) } else { power::turn_off_time(&w[0], &w[1], dt) }
            }
            "CIN" => {
                let (v, i) = Self::two_id(over, self.m.pw_v, self.m.pw_i)?;
                let Some((_, dt, w, _)) = self.grid(&[v, i], &[]) else { return Ok(miss) };
                power::input_capacitance(&w[0], &w[1], dt)
            }
            "ROVER" | "RUNDER" | "RDC" | "RMAX" => {
                let (rail, vin, clk) = match over {
                    [] => (self.m.pw_v, self.m.pw_vin, self.m.pw_clk),
                    [a] => (*a, None, None),
                    [a, b] => (*a, Some(*b), None),
                    [a, b, c] => (*a, Some(*b), Some(*c)),
                    _ => return Err(()),
                };
                let Some((_, dt, w, opt)) = self.grid(&[rail], &[vin, clk]) else { return Ok(miss) };
                let r = power::rail(&w[0], dt, self.m.pw_ref, opt.first().and_then(|s| s.as_deref()), opt.get(1).and_then(|s| s.as_deref()));
                match item {
                    "ROVER" => r.overshoot,
                    "RUNDER" => r.undershoot,
                    "RDC" => r.dc,
                    _ => r.turn_on,
                }
            }
            _ => return Err(()),
        })
    }

    /// Harmonic order and rms amperes, up to the 40th.
    pub(crate) fn fact_harmonics(&self, over: &[WaveId]) -> Result<Vec<(u32, f64)>, ()> {
        let id = Self::one_id(over, self.m.pw_i)?;
        let Some((_, dt, w, _)) = self.grid(&[id], &[]) else { return Ok(Vec::new()) };
        Ok(power::harmonics(&w[0], dt, None, 40).into_iter().map(|(n, a, _, _)| (n, a)).collect())
    }

    pub(crate) fn fact_spectrum(&self, item: &str, over: &[WaveId]) -> Result<f64, ()> {
        let id = Self::one_id(over, self.m.spec_src)?;
        let Some((_, dt, w, _)) = self.grid(&[id], &[]) else { return Ok(f64::NAN) };
        let v = &w[0];
        if matches!(item, "ACPR" | "ACPRLO" | "ACPRHI" | "FLOOR") {
            let s = spectrum::Settings {
                center: self.m.spec_center,
                span: self.m.spec_span,
                rbw: self.m.spec_rbw,
                window: self.m.spec_win,
                averaging: 1,
                trace: spectrum::Trace::Normal,
                detector: spectrum::Detector::Peak,
                points: 801,
                ref_level_dbv: self.m.spec_ref as f64,
            };
            let mut avg = None;
            let sp = spectrum::analyze(v, dt, &s, &mut avg);
            let ac = spectrum::acpr(&sp, self.m.spec_acpr_w, self.m.spec_acpr_s);
            return Ok(match item {
                "ACPR" => ac.ratio,
                "ACPRLO" => ac.lower_db,
                "ACPRHI" => ac.upper_db,
                _ => sp.floor_dbv,
            });
        }
        let Some(d) = spectrum::distortion(v, dt, None, 10) else { return Ok(f64::NAN) };
        Ok(match item {
            "F1" => d.f1,
            "THD" => d.thd_db,
            "THDPCT" => d.thd_pct,
            "THDN" => d.thdn_db,
            "SNR" => d.snr_db,
            "SINAD" => d.sinad_db,
            "SFDR" => d.sfdr_dbc,
            "ENOB" => d.enob,
            _ => return Err(()),
        })
    }

    pub(crate) fn fact_magnetic(&self, item: &str, over: &[WaveId]) -> Result<f64, ()> {
        let (v, i) = Self::two_id(over, self.m.mag_v, self.m.mag_i)?;
        let Some((_, dt, w, _)) = self.grid(&[v, i], &[]) else { return Ok(f64::NAN) };
        let g = Geom {
            turns: self.m.mag_n.max(1.0),
            area: (self.m.mag_ae > 0.0).then_some(self.m.mag_ae),
            length: (self.m.mag_le > 0.0).then_some(self.m.mag_le),
            rcu: self.m.mag_rcu,
        };
        let m = magnetic::measure(&w[0], &w[1], dt, g);
        let n = |x: Option<f64>| x.unwrap_or(f64::NAN);
        Ok(match item {
            "L" => n(m.l),
            "B" => n(m.b_peak),
            "H" => n(m.h_peak),
            "MUR" => n(m.mu_r),
            "LOSS" => n(m.loss),
            _ => return Err(()),
        })
    }

    pub(crate) fn fact_inverter(&self, item: &str, over: &[WaveId]) -> Result<Fact, ()> {
        if !over.is_empty() && matches!(self.m.inv_ang, InvAngle::Quad | InvAngle::Hall) {
            return Err(());
        }
        let (a, b, c, an) = match over {
            [] => (self.m.inv_a, self.m.inv_b, self.m.inv_c, self.m.inv_an),
            [n] if self.m.inv_ang == InvAngle::Analog => (self.m.inv_a, self.m.inv_b, self.m.inv_c, *n),
            [x, y, z] => (*x, *y, *z, self.m.inv_an),
            [x, y, z, n] => (*x, *y, *z, *n),
            _ => return Err(()),
        };
        let pack = self.inv_pack(a, b, c, an);
        let num = |x: Option<f64>| Fact::Num(x.unwrap_or(f64::NAN));
        let miss = Fact::Num(f64::NAN);
        Ok(match item {
            "D" => pack.as_ref().and_then(|p| p.dq.as_ref()).map(|q| Fact::Num(q.d)).unwrap_or(miss),
            "Q" => pack.as_ref().and_then(|p| p.dq.as_ref()).map(|q| Fact::Num(q.q)).unwrap_or(miss),
            "ZERO" => pack.as_ref().and_then(|p| p.dq.as_ref()).map(|q| Fact::Num(q.zero)).unwrap_or(miss),
            "FREQ" => pack.as_ref().and_then(|p| p.dq.as_ref()).map(|q| Fact::Num(q.freq)).unwrap_or(miss),
            "SPEED" => pack.as_ref().map(|p| num(p.mech.shaft.rpm)).unwrap_or(miss),
            "ACCEL" => pack.as_ref().map(|p| num(p.mech.shaft.accel)).unwrap_or(miss),
            "DIR" => match pack.as_ref() {
                Some(p) => Fact::Word(match p.mech.shaft.direction {
                    motor::Direction::Forward => "FORWARD",
                    motor::Direction::Reverse => "REVERSE",
                    motor::Direction::Stopped => "STOPPED",
                }),
                None => Fact::Num(f64::NAN),
            },
            "ANGLE" => pack.as_ref().map(|p| num(p.mech.shaft.angle_deg)).unwrap_or(miss),
            "TRAVEL" => pack.as_ref().map(|p| num(p.mech.shaft.travel_deg)).unwrap_or(miss),
            "TORQUE" => pack.as_ref().map(|p| num(p.mech.torque)).unwrap_or(miss),
            "PMECH" => pack.as_ref().map(|p| num(p.mech.p_mech)).unwrap_or(miss),
            "PELEC" => pack.as_ref().map(|p| num(p.mech.p_elec)).unwrap_or(miss),
            "EFF" => pack.as_ref().map(|p| num(p.mech.eff)).unwrap_or(miss),
            _ => return Err(()),
        })
    }

    fn inv_pack(&self, a: WaveId, b: WaveId, c: WaveId, an: WaveId) -> Option<InvPack> {
        let m = &self.m;
        let poles = m.inv_poles.max(1);
        match m.inv_ang {
            InvAngle::Quad | InvAngle::Hall => self.inv_digital(),
            InvAngle::Analog => {
                let (t0, dt, v, opt) = self.grid(&[a, b, c], &[Some(an), m.inv_tq, m.inv_ev, m.inv_ei])?;
                let ang = opt.first().and_then(|s| s.as_deref())?;
                let theta = motor::analog_elec(ang, m.inv_degv, m.inv_off, m.inv_elec, poles);
                let dq = motor::dq0(&v[0], &v[1], &v[2], dt, Theta::Elec(&theta));
                let shaft = motor::shaft_analog(ang, dt, t0, m.inv_degv, m.inv_off, m.inv_elec, poles);
                let mech = motor::mechanical(shaft, opt.get(1).and_then(|s| s.as_deref()), m.inv_scale, opt.get(2).and_then(|s| s.as_deref()), opt.get(3).and_then(|s| s.as_deref()));
                Some(InvPack { dq, mech })
            }
            InvAngle::Phase | InvAngle::Off => {
                let (t0, dt, v, opt) = self.grid(&[a, b, c], &[m.inv_tq, m.inv_ev, m.inv_ei])?;
                let dq = motor::dq0(&v[0], &v[1], &v[2], dt, Theta::Vector);
                let shaft = motor::shaft_phase(&v[0], &v[1], &v[2], dt, t0, poles);
                let mech = motor::mechanical(shaft, opt.first().and_then(|s| s.as_deref()), m.inv_scale, opt.get(1).and_then(|s| s.as_deref()), opt.get(2).and_then(|s| s.as_deref()));
                Some(InvPack { dq, mech })
            }
        }
    }

    fn inv_digital(&self) -> Option<InvPack> {
        if !crate::ui::la::available() {
            return None;
        }
        let mech = self.on_rec(|rec| {
            let la = rec.la.as_ref()?;
            if la.words.is_empty() || !(la.dt > 0.0) {
                return None;
            }
            let m = &self.m;
            let bit = |line: u8| la.words.iter().map(|w| w >> line & 1 == 1).collect::<Vec<_>>();
            let shaft = if m.inv_ang == InvAngle::Hall {
                motor::shaft_hall(&bit(m.inv_ha), &bit(m.inv_hb), &bit(m.inv_hc), la.dt, la.t0)
            } else {
                let ix = m.inv_qi.map(|q| bit(q));
                motor::shaft_quad(&bit(m.inv_qa), &bit(m.inv_qb), ix.as_deref(), la.dt, la.t0, m.inv_cpr, m.inv_swap)
            };
            let roles = [m.inv_tq, m.inv_ev, m.inv_ei];
            let taken = self.take_waves(rec, &[], &roles).ok()?;
            Some(motor::mechanical(shaft, taken.opt.first().and_then(|s| s.as_deref()), m.inv_scale, taken.opt.get(1).and_then(|s| s.as_deref()), taken.opt.get(2).and_then(|s| s.as_deref())))
        })?;
        Some(InvPack { dq: None, mech })
    }

    pub(crate) fn fact_dpt(&self, item: &str, over: &[WaveId]) -> Result<f64, ()> {
        let (v, i, g) = match over {
            [] => (self.m.dpt_v, self.m.dpt_i, Some(self.m.dpt_g)),
            [a, b] => (*a, *b, None),
            [a, b, c] => (*a, *b, Some(*c)),
            _ => return Err(()),
        };
        let fourth_id = (self.m.dpt_4th != DptFourth::Off).then_some(self.m.dpt_4ch);
        let Some((_, dt, w, opt)) = self.grid(&[v, i], &[g, fourth_id]) else { return Ok(f64::NAN) };
        let gate = opt.first().and_then(|s| s.as_deref());
        let fourth_s = opt.get(1).and_then(|s| s.as_deref());
        let fourth = match (self.m.dpt_4th, fourth_s) {
            (DptFourth::Gate, Some(s)) => Fourth::Gate(s),
            (DptFourth::DiodeV, Some(s)) => Fourth::DiodeV(s),
            (DptFourth::DiodeI, Some(s)) => Fourth::DiodeI(s),
            _ => Fourth::Off,
        };
        let pulse = dpt::measure(&w[0], &w[1], gate, fourth, dt, self.m.dpt_pct);
        let sw = power::switching(&w[0], &w[1], dt, 10.0);
        let n = |x: Option<f64>| x.unwrap_or(f64::NAN);
        Ok(match item {
            "VPEAK" => n(pulse.vpeak),
            "IPEAK" => n(pulse.ipeak),
            "EON" => sw.e_on,
            "EOFF" => sw.e_off,
            "TDON" => n(pulse.td_on),
            "TR" => n(pulse.tr),
            "TON" => n(pulse.ton),
            "TDOFF" => n(pulse.td_off),
            "TF" => n(pulse.tf),
            "TOFF" => n(pulse.toff),
            "DVDT" => n(pulse.dvdt),
            "DIDT" => n(pulse.didt),
            "TDT" => n(pulse.tdt),
            "TRR" => n(pulse.trr),
            "QRR" => n(pulse.qrr),
            "ERR" => n(pulse.err),
            "IRRM" => n(pulse.irrm),
            "DDIDT" => n(pulse.diode_didt),
            "QOSS" => n(pulse.qoss),
            _ => return Err(()),
        })
    }

    pub(crate) fn fact_bandwidth(&self, item: &str, over: &[WaveId]) -> Result<f64, ()> {
        let id = match over {
            [] => WaveId::Ch(self.m.bw_src.min(n_ch() - 1)),
            [a] => *a,
            _ => return Err(()),
        };
        let Some((v, dt)) = self.bw_samples(id) else { return Ok(f64::NAN) };
        if v.len() < 1000 || !(dt > 0.0) {
            return Ok(f64::NAN);
        }
        let fs = 1.0 / dt;
        let res = bandwidth::tone(&v, dt).and_then(|fa| {
            let f = if self.m.bw_hint > 0.0 { bandwidth::unalias(fa, fs, self.m.bw_hint) } else { fa };
            let (w, step) = bandwidth::fold(&v, dt, 1.0 / f, self.m.bw_bins)?;
            bandwidth::step_response(&w, step)
        });
        let Some(r) = res else { return Ok(f64::NAN) };
        Ok(match item {
            "RISE" => r.rise,
            "BW" => r.bw_from_rise,
            "OVER" => r.overshoot,
            _ => return Err(()),
        })
    }

    fn bw_samples(&self, id: WaveId) -> Option<(Vec<f32>, f64)> {
        match id {
            WaveId::Ref(i) => {
                let r = self.refs.get(i)?.as_ref()?;
                (r.v.len() >= 2 && r.dt > 0.0).then_some((r.v.clone(), r.dt))
            }
            WaveId::Ch(c) => self.on_rec(|rec| match &rec.raw {
                Some(r) if c < r.frame.ch.len() && !r.frame.ch[c].is_empty() && r.frame.dt > 0.0 => Some((r.frame.ch[c].clone(), r.frame.dt)),
                _ => {
                    let (v, dt) = self.meas_wave(rec, c);
                    (v.len() >= 2 && dt > 0.0).then_some((v, dt))
                }
            }),
        }
    }
}

struct InvPack {
    dq: Option<oos_dsp::motor::Dq0>,
    mech: oos_dsp::motor::Mech,
}

fn shift_t0(a: &Analysis, t0: f64) -> Analysis {
    if t0 == 0.0 {
        return a.clone();
    }
    let mut b = a.clone();
    for t in &mut b.rises {
        *t += t0;
    }
    for t in &mut b.falls {
        *t += t0;
    }
    b.t_vmax += t0;
    b.t_vmin += t0;
    b
}
