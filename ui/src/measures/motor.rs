// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Motor drives from three phase sources (A, B, C): the DQ0 transform, shaft speed, acceleration, direction and angle;
//! with a torque source (D) and the electrical pair of the Inverter page, torque, mechanical power and efficiency.

use super::*;
use Mark::*;

pub const DQ0: u16 = 130;
pub const SPEED: u16 = 131;
pub const ACCEL: u16 = 132;
pub const DIRECTION: u16 = 133;
pub const ANGLE: u16 = 134;

static MOTOR_DEFS: [Def; 5] = [
    module(
        DQ0,
        "DQ0",
        "",
        Loop,
        &[0, 1, 2],
        &[(MOTOR, "DQ0 TRANSFORM")],
        "Direct-axis mean of the three phase sources.",
    ),
    module(
        SPEED,
        "Speed",
        "rpm",
        Slope,
        &[0, 1, 2],
        &[(MOTOR, "SHAFT & EFFICIENCY")],
        "Mean shaft speed taken from the phase vector.",
    ),
    module(
        ACCEL,
        "Acceleration",
        "rpm/s",
        Slope,
        &[0, 1, 2],
        &[(MOTOR, "SHAFT & EFFICIENCY")],
        "Slope of that speed across revolutions.",
    ),
    module(
        DIRECTION,
        "Direction",
        "",
        Slope,
        &[0, 1, 2],
        &[(MOTOR, "SHAFT & EFFICIENCY")],
        "Sign of the speed: forward is 1, stopped is 0, reverse is -1.",
    ),
    module(ANGLE, "Angle", "\u{b0}", Loop, &[0, 1, 2], &[(MOTOR, "SHAFT & EFFICIENCY")], "Shaft angle at the trigger."),
];

pub struct Motor;

impl MeasModule for Motor {
    fn key(&self) -> &'static str {
        "motor"
    }

    fn defs(&self) -> &'static [Def] {
        &MOTOR_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        inverter_settings(m, v);
    }

    fn sources(&self, _id: u16, row: [WaveId; 4], _m: &Model, _ids: &mut Vec<WaveId>) -> Result<(), &'static str> {
        distinct(row)
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let phases = phases(x);
        let shaft = shaft(x, phases.as_deref());
        let nan = f64::NAN;
        derived(ids, |id| match id {
            DQ0 => phases
                .as_ref()
                .and_then(|v| oos_dsp::motor::dq0(v[0], v[1], v[2], x.dt, oos_dsp::motor::Theta::Vector))
                .map_or(nan, |q| q.d),
            SPEED => shaft.as_ref().and_then(|s| s.rpm).unwrap_or(nan),
            ACCEL => shaft.as_ref().and_then(|s| s.accel).unwrap_or(nan),
            DIRECTION => shaft.as_ref().map_or(nan, |s| match s.direction {
                oos_dsp::motor::Direction::Forward => 1.0,
                oos_dsp::motor::Direction::Stopped => 0.0,
                oos_dsp::motor::Direction::Reverse => -1.0,
            }),
            ANGLE => shaft.as_ref().and_then(|s| s.angle_deg).unwrap_or(nan),
            _ => nan,
        })
    }
}

pub const TORQUE: u16 = 135;
pub const MECH_POWER: u16 = 136;
pub const SYS_EFF: u16 = 137;

static MECHANICAL_DEFS: [Def; 3] = [
    module(
        TORQUE,
        "Torque",
        "N\u{b7}m",
        Bars,
        &[0, 1, 2, 3],
        &[(MOTOR, "SHAFT & EFFICIENCY")],
        "Mean of the torque source, in the chosen scale.",
    ),
    module(
        MECH_POWER,
        "Mechanical Power",
        "W",
        Bars,
        &[0, 1, 2, 3],
        &[(MOTOR, "SHAFT & EFFICIENCY")],
        "Torque times the shaft speed.",
    ),
    module(
        SYS_EFF,
        "System Efficiency",
        "",
        Bars,
        &[0, 1, 2, 3],
        &[(MOTOR, "SHAFT & EFFICIENCY")],
        "Mechanical power divided by the electrical power of the configured pair.",
    ),
];

pub struct Mechanical;

impl MeasModule for Mechanical {
    fn key(&self) -> &'static str {
        "motor_mechanical"
    }

    fn defs(&self) -> &'static [Def] {
        &MECHANICAL_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        inverter_settings(m, v);
    }

    fn sources(&self, _id: u16, row: [WaveId; 4], m: &Model, ids: &mut Vec<WaveId>) -> Result<(), &'static str> {
        distinct(row)?;
        if m.inv_ev.is_some() != m.inv_ei.is_some() {
            return Err("Electrical voltage and current sources required together");
        }
        ids.extend(m.inv_ev);
        ids.extend(m.inv_ei);
        Ok(())
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let m = x.model;
        let mech = shaft(x, phases(x).as_deref()).map(|s| {
            let torque = x.wave(x.row[3]);
            let ev = m.inv_ev.and_then(|id| x.wave(id));
            let ei = m.inv_ei.and_then(|id| x.wave(id));
            oos_dsp::motor::mechanical(s, torque, m.inv_scale, ev, ei)
        });
        derived(ids, |id| {
            let v = mech.as_ref().and_then(|m| match id {
                TORQUE => m.torque,
                MECH_POWER => m.p_mech,
                _ => m.eff,
            });
            v.unwrap_or(f64::NAN)
        })
    }
}

/// The settings of both motor modules: the torque scale, the pole count and the electrical sources.
fn inverter_settings(m: &Model, v: &mut Vec<u64>) {
    put(v, m.inv_scale);
    v.push(m.inv_poles as u64);
    for id in [m.inv_ev, m.inv_ei] {
        v.push(id.map_or(u64::MAX, wave_number));
    }
}

fn distinct(row: [WaveId; 4]) -> Result<(), &'static str> {
    if row[0] == row[1] || row[0] == row[2] || row[1] == row[2] {
        return Err("Three distinct phase sources required");
    }
    Ok(())
}

/// The three phases, when they are three different sources.
fn phases<'a>(x: &Ctx<'a>) -> Option<Vec<&'a [f32]>> {
    let [a, b, c, _] = x.row;
    if a == b || a == c || b == c {
        return None;
    }
    x.align(&[a, b, c])
}

fn shaft(x: &Ctx, phases: Option<&[&[f32]]>) -> Option<oos_dsp::motor::Shaft> {
    let v = phases?;
    Some(oos_dsp::motor::shaft_phase(v[0], v[1], v[2], x.dt, x.t0, x.model.inv_poles.max(1)))
}
