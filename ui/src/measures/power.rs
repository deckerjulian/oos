// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Power analysis: input power and quality, harmonics, inrush, input capacitance, turn-on and turn-off times, switching
//! losses and slopes, ripple, PSRR and impedance. Source A is the voltage, source B the current (or the second voltage).

use super::*;
use Mark::*;

pub const POWER_QUALITY: u16 = 90;
pub const POWER_FACTOR: u16 = 118;

static QUALITY_DEFS: [Def; 2] = [
    module(
        POWER_QUALITY,
        "Power Quality",
        "W",
        Bars,
        &[0, 1],
        &[(POWER, "INPUT")],
        "Real power over the whole cycles of the voltage and the current.",
    ),
    module(
        POWER_FACTOR,
        "Power Factor",
        "",
        Bars,
        &[0, 1],
        &[(POWER, "POWER FACTOR")],
        "Real power divided by the apparent power.",
    ),
];

pub struct Quality;

impl MeasModule for Quality {
    fn key(&self) -> &'static str {
        "power_quality"
    }

    fn defs(&self) -> &'static [Def] {
        &QUALITY_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        put(v, m.pw_fline);
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let fline = (x.model.pw_fline > 0.0).then_some(x.model.pw_fline);
        let q = x.align(&[a, b]).map(|v| oos_dsp::power::quality(v[0], v[1], x.dt, fline));
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    POWER_QUALITY => q.map_or(f64::NAN, |q| q.p_real),
                    POWER_FACTOR => q.map_or(f64::NAN, |q| q.pf),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const HARMONICS: u16 = 91;

static HARMONICS_DEFS: [Def; 1] = [module(
    HARMONICS,
    "Harmonics",
    "%",
    Bars,
    &[1],
    &[(POWER, "INPUT")],
    "Current above the fundamental, as a percent of the fundamental.",
)];

pub struct Harmonics;

impl MeasModule for Harmonics {
    fn key(&self) -> &'static str {
        "harmonics"
    }

    fn defs(&self) -> &'static [Def] {
        &HARMONICS_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        put(v, m.pw_fline);
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    HARMONICS => x.wave(b).map_or(f64::NAN, |i| {
                        let h = oos_dsp::power::harmonics(
                            i,
                            x.dt,
                            (x.model.pw_fline > 0.0).then_some(x.model.pw_fline),
                            40,
                        );
                        let fund = h.iter().find(|x| x.0 == 1).map(|x| x.1).unwrap_or(0.0);
                        let rest = h.iter().filter(|x| x.0 > 1).map(|x| x.1 * x.1).sum::<f64>().sqrt();
                        if fund > 0.0 {
                            100.0 * rest / fund
                        } else {
                            f64::NAN
                        }
                    }),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const INRUSH: u16 = 92;

static INRUSH_DEFS: [Def; 1] = [module(
    INRUSH,
    "Inrush Current",
    "A",
    Slope,
    &[1],
    &[(POWER, "INPUT")],
    "Largest absolute current in the record.",
)];

pub struct Inrush;

impl MeasModule for Inrush {
    fn key(&self) -> &'static str {
        "inrush"
    }

    fn defs(&self) -> &'static [Def] {
        &INRUSH_DEFS
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    INRUSH => x.wave(b).map_or(f64::NAN, |i| oos_dsp::power::inrush(i, x.dt).0),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const INPUT_C: u16 = 93;

static INPUT_C_DEFS: [Def; 1] = [module(
    INPUT_C,
    "Input Capacitance",
    "F",
    Loop,
    &[0, 1],
    &[(POWER, "INPUT")],
    "Charge moved, divided by the voltage swing. The record is the turn-on.",
)];

pub struct InputC;

impl MeasModule for InputC {
    fn key(&self) -> &'static str {
        "input_capacitance"
    }

    fn defs(&self) -> &'static [Def] {
        &INPUT_C_DEFS
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    INPUT_C => {
                        x.align(&[a, b]).map_or(f64::NAN, |v| oos_dsp::power::input_capacitance(v[0], v[1], x.dt))
                    }
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const TURN_ON: u16 = 100;
pub const TURN_OFF: u16 = 101;

static TURN_DEFS: [Def; 2] = [
    module(
        TURN_ON,
        "Turn On Time",
        "s",
        Edge,
        &[0, 1],
        &[(POWER, "TIMING"), (RAILS, "RAIL SEQUENCING")],
        "From the second source rising through mid-level to the first source reaching its top.",
    ),
    module(
        TURN_OFF,
        "Turn Off Time",
        "s",
        Edge,
        &[0, 1],
        &[(POWER, "TIMING"), (RAILS, "RAIL SEQUENCING")],
        "From the second source falling through mid-level to the first source reaching its base.",
    ),
];

pub struct Turn;

impl MeasModule for Turn {
    fn key(&self) -> &'static str {
        "turn_on_off"
    }

    fn defs(&self) -> &'static [Def] {
        &TURN_DEFS
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    TURN_ON => x.align(&[a, b]).map_or(f64::NAN, |v| oos_dsp::power::turn_on_time(v[1], v[0], x.dt)),
                    TURN_OFF => x.align(&[a, b]).map_or(f64::NAN, |v| oos_dsp::power::turn_off_time(v[1], v[0], x.dt)),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const E_ON: u16 = 102;
pub const E_OFF: u16 = 103;
pub const E_COND: u16 = 104;
pub const DVDT_RISE: u16 = 105;
pub const DVDT_FALL: u16 = 106;
pub const DIDT_RISE: u16 = 107;
pub const DIDT_FALL: u16 = 108;
pub const RDS_ON: u16 = 109;

static SWITCHING_DEFS: [Def; 8] = [
    module(E_ON, "E on", "J", Slope, &[0, 1], &[(POWER, "SWITCHING")], "Mean energy of a turn-on transition."),
    module(E_OFF, "E off", "J", Slope, &[0, 1], &[(POWER, "SWITCHING")], "Mean energy of a turn-off transition."),
    module(E_COND, "E cond", "J", Slope, &[0, 1], &[(POWER, "SWITCHING")], "Mean energy while the switch stays on."),
    module(
        DVDT_RISE,
        "dv/dt rise",
        "V/s",
        Slope,
        &[0, 1],
        &[(POWER, "SWITCHING")],
        "Slope of the voltage on a rising edge.",
    ),
    module(
        DVDT_FALL,
        "dv/dt fall",
        "V/s",
        Slope,
        &[0, 1],
        &[(POWER, "SWITCHING")],
        "Slope of the voltage on a falling edge.",
    ),
    module(
        DIDT_RISE,
        "di/dt rise",
        "A/s",
        Slope,
        &[0, 1],
        &[(POWER, "SWITCHING")],
        "Slope of the current on a rising edge.",
    ),
    module(
        DIDT_FALL,
        "di/dt fall",
        "A/s",
        Slope,
        &[0, 1],
        &[(POWER, "SWITCHING")],
        "Slope of the current on a falling edge.",
    ),
    module(
        RDS_ON,
        "RDS(on)",
        "\u{3a9}",
        Slope,
        &[0, 1],
        &[(POWER, "SWITCHING")],
        "On-state voltage divided by the on-state current.",
    ),
];

pub struct Switching;

impl MeasModule for Switching {
    fn key(&self) -> &'static str {
        "switching"
    }

    fn defs(&self) -> &'static [Def] {
        &SWITCHING_DEFS
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let s = x.align(&[a, b]).map(|v| oos_dsp::power::switching(v[0], v[1], x.dt, 10.0));
        let s = s.as_ref();
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    E_ON => s.map_or(f64::NAN, |s| s.e_on),
                    E_OFF => s.map_or(f64::NAN, |s| s.e_off),
                    E_COND => s.map_or(f64::NAN, |s| s.e_cond),
                    DVDT_RISE => s.map_or(f64::NAN, |s| s.dvdt_rise),
                    DVDT_FALL => s.map_or(f64::NAN, |s| s.dvdt_fall),
                    DIDT_RISE => s.map_or(f64::NAN, |s| s.didt_rise),
                    DIDT_FALL => s.map_or(f64::NAN, |s| s.didt_fall),
                    RDS_ON => s.map_or(f64::NAN, |s| s.rds_on),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const RIPPLE: u16 = 113;

static RIPPLE_DEFS: [Def; 1] =
    [module(RIPPLE, "Ripple", "V", Pulse, &[0], &[(POWER, "OUTPUT")], "Peak-to-peak of the whole voltage record.")];

pub struct Ripple;

impl MeasModule for Ripple {
    fn key(&self) -> &'static str {
        "ripple"
    }

    fn defs(&self) -> &'static [Def] {
        &RIPPLE_DEFS
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    RIPPLE => x.wave(a).map_or(f64::NAN, |v| oos_dsp::power::ripple(v).0),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const LINE_RIPPLE: u16 = 114;

static LINE_RIPPLE_DEFS: [Def; 1] = [module(
    LINE_RIPPLE,
    "Line Ripple",
    "V",
    Pulse,
    &[0],
    &[(POWER, "OUTPUT")],
    "Peak-to-peak of the voltage at the line frequency.",
)];

pub struct LineRipple;

impl MeasModule for LineRipple {
    fn key(&self) -> &'static str {
        "line_ripple"
    }

    fn defs(&self) -> &'static [Def] {
        &LINE_RIPPLE_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        put(v, m.pw_fline);
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    LINE_RIPPLE => x.wave(a).map_or(f64::NAN, |v| {
                        let f =
                            if x.model.pw_fline > 0.0 { x.model.pw_fline } else { oos_dsp::power::line_hz(v, x.dt) };
                        oos_dsp::power::tone_ripple(v, x.dt, f).pp
                    }),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const SW_RIPPLE: u16 = 115;

static SW_RIPPLE_DEFS: [Def; 1] = [module(
    SW_RIPPLE,
    "Switching Ripple",
    "V",
    Pulse,
    &[0],
    &[(POWER, "OUTPUT")],
    "Peak-to-peak of the voltage at the switching frequency.",
)];

pub struct SwRipple;

impl MeasModule for SwRipple {
    fn key(&self) -> &'static str {
        "switching_ripple"
    }

    fn defs(&self) -> &'static [Def] {
        &SW_RIPPLE_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        put(v, m.pw_fsw);
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    SW_RIPPLE => x.wave(a).map_or(f64::NAN, |v| {
                        let f = if x.model.pw_fsw > 0.0 { x.model.pw_fsw } else { oos_dsp::power::switch_hz(v, x.dt) };
                        oos_dsp::power::switching_ripple(v, x.dt, f).pp
                    }),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

pub const PSRR: u16 = 116;
pub const IMPEDANCE: u16 = 117;

static RATIO_DEFS: [Def; 2] = [
    module(
        PSRR,
        "PSRR",
        "dB",
        Bars,
        &[0, 1],
        &[(POWER, "PSRR & IMPEDANCE")],
        "Rejection of the first source by the second, at the measured frequency.",
    ),
    module(
        IMPEDANCE,
        "Impedance",
        "",
        Bars,
        &[0, 1],
        &[(POWER, "PSRR & IMPEDANCE")],
        "Linear ratio of the second source to the first at that frequency.",
    ),
];

pub struct Ratio;

impl MeasModule for Ratio {
    fn key(&self) -> &'static str {
        "psrr_impedance"
    }

    fn defs(&self) -> &'static [Def] {
        &RATIO_DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        put(v, m.pw_fline);
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    PSRR => ratio(x, a, b, oos_dsp::bode::Mode::Psrr),
                    IMPEDANCE => ratio(x, a, b, oos_dsp::bode::Mode::Z),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}

/// The ratio of source B to source A at the line frequency (or the frequency found in A).
fn ratio(x: &Ctx, a: WaveId, b: WaveId, mode: oos_dsp::bode::Mode) -> f64 {
    let Some(v) = x.align(&[a, b]) else { return f64::NAN };
    let f = if x.model.pw_fline > 0.0 { x.model.pw_fline } else { oos_dsp::power::line_hz(v[0], x.dt) };
    oos_dsp::bode::response(mode, v[0], v[1], x.dt, f).map(|p| p.0).unwrap_or(f64::NAN)
}
