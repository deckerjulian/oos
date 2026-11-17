// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Double-pulse test of a power switch: switching times, slopes and peaks from the voltage (A), the current (B) and the
//! gate (C); diode recovery, with the optional fourth probe of the Double pulse page.

use super::*;
use crate::model::DptFourth;
use Mark::*;

pub const VPEAK: u16 = 170;
pub const IPEAK: u16 = 171;
pub const TD_ON: u16 = 172;
pub const TD_OFF: u16 = 175;
pub const TR: u16 = 173;
pub const TF: u16 = 176;
pub const TON: u16 = 174;
pub const TOFF: u16 = 177;
pub const DPDT_V: u16 = 178;
pub const DPDT_I: u16 = 179;
pub const TDT: u16 = 180;
pub const TRR: u16 = 181;
pub const QRR: u16 = 182;
pub const ERR: u16 = 183;
pub const IRRM: u16 = 184;
pub const DIODE_DDT: u16 = 185;
pub const QOSS: u16 = 186;

static DEFS: [Def; 17] = [
    module(VPEAK, "Vpeak", "V", Pulse, &[0, 1, 2], &[(DPT, "SWITCHING")], "Highest voltage on the turn-off edge."),
    module(IPEAK, "Ipeak", "A", Pulse, &[0, 1, 2], &[(DPT, "SWITCHING")], "Highest current on the turn-on edge."),
    module(TD_ON, "Td(on)", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "Delay from the gate rising through its threshold to the current starting to rise."),
    module(TD_OFF, "Td(off)", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "Delay from the gate falling through its threshold to the current starting to fall."),
    module(TR, "Tr", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "Current rise time of the turn-on."),
    module(TF, "Tf", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "Current fall time of the turn-off."),
    module(TON, "Ton", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "Turn-on delay plus the current rise."),
    module(TOFF, "Toff", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "Turn-off delay plus the current fall."),
    module(DPDT_V, "dv/dt", "V/s", Slope, &[0, 1, 2], &[(DPT, "SWITCHING")], "Slope of the voltage on the pulse edge."),
    module(DPDT_I, "di/dt", "A/s", Slope, &[0, 1, 2], &[(DPT, "SWITCHING")], "Slope of the current on the pulse edge."),
    module(TDT, "Tdt", "s", Edge, &[0, 1, 2], &[(DPT, "SWITCHING")], "The smaller of the two dead times."),
    module(TRR, "Trr", "s", Edge, &[0, 1, 2], &[(DPT, "DIODE RECOVERY")], "Reverse-recovery time of the diode current."),
    module(QRR, "Qrr", "C", Pulse, &[0, 1, 2], &[(DPT, "DIODE RECOVERY")], "Reverse-recovery charge."),
    module(ERR, "Err", "J", Pulse, &[0, 1, 2], &[(DPT, "DIODE RECOVERY")], "Energy of the reverse-recovery interval."),
    module(IRRM, "Irrm", "A", Pulse, &[0, 1, 2], &[(DPT, "DIODE RECOVERY")], "Peak reverse-recovery current."),
    module(DIODE_DDT, "Diode d/dt", "A/s", Slope, &[0, 1, 2], &[(DPT, "DIODE RECOVERY")], "Slope of the recovery current up to its peak."),
    module(QOSS, "Qoss", "C", Pulse, &[0, 1, 2], &[(DPT, "DIODE RECOVERY")], "Charge moved while the voltage rises."),
];

pub struct DoublePulse;

impl MeasModule for DoublePulse {
    fn key(&self) -> &'static str {
        "double_pulse"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        put(v, m.dpt_pct);
        v.push(m.dpt_4th as u64);
        v.push(wave_number(m.dpt_4ch));
    }

    fn sources(&self, _id: u16, _row: [WaveId; 4], m: &Model, ids: &mut Vec<WaveId>) -> Result<(), &'static str> {
        if m.dpt_4th != DptFourth::Off {
            ids.push(m.dpt_4ch);
        }
        Ok(())
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let m = x.model;
        let pulse = x.align(&[x.row[0], x.row[1], x.row[2]]).map(|v| {
            let fourth = match (m.dpt_4th, x.wave(m.dpt_4ch)) {
                (DptFourth::Gate, Some(w)) => oos_dsp::dpt::Fourth::Gate(w),
                (DptFourth::DiodeV, Some(w)) => oos_dsp::dpt::Fourth::DiodeV(w),
                (DptFourth::DiodeI, Some(w)) => oos_dsp::dpt::Fourth::DiodeI(w),
                _ => oos_dsp::dpt::Fourth::Off,
            };
            oos_dsp::dpt::measure(v[0], v[1], Some(v[2]), fourth, x.dt, m.dpt_pct)
        });
        let values = ids
            .iter()
            .map(|&id| {
                let v = pulse.as_ref().and_then(|p| match id {
                    VPEAK => p.vpeak,
                    IPEAK => p.ipeak,
                    TD_ON => p.td_on,
                    TD_OFF => p.td_off,
                    TR => p.tr,
                    TF => p.tf,
                    TON => p.ton,
                    TOFF => p.toff,
                    DPDT_V => p.dvdt,
                    DPDT_I => p.didt,
                    TDT => p.tdt,
                    TRR => p.trr,
                    QRR => p.qrr,
                    ERR => p.err,
                    IRRM => p.irrm,
                    DIODE_DDT => p.diode_didt,
                    _ => p.qoss,
                });
                (id, v.unwrap_or(f64::NAN))
            })
            .collect();
        let quality = pulse.as_ref().map_or("DPT: no qualifying pulse", |p| {
            if p.sample_limit {
                "DPT estimate: sample limited"
            } else if p.diode_from_id || p.err_from_vds || p.qoss_approx {
                "DPT estimate: inferred recovery or charge"
            } else {
                "Derived DPT result"
            }
        });
        Values { values, quality }
    }
}
