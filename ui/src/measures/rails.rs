// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Power rail sequencing: the rail's DC level, overshoot and undershoot (source A), and the overshoot after the input
//! (source B) turns on.

use super::*;
use Mark::*;

pub const RAIL_OVER: u16 = 150;
pub const RAIL_UNDER: u16 = 151;
pub const RAIL_TURN: u16 = 152;
pub const DC_RAIL: u16 = 153;

static DEFS: [Def; 4] = [
    module(RAIL_OVER, "Overshoot", "V", Pulse, &[0], &[(RAILS, "RAIL SEQUENCING")], "How far the rail rises above its reference."),
    module(RAIL_UNDER, "Undershoot", "V", Pulse, &[0], &[(RAILS, "RAIL SEQUENCING")], "How far the rail falls below its reference."),
    module(RAIL_TURN, "Turn-on Overshoot", "V", Pulse, &[0, 1], &[(RAILS, "RAIL SEQUENCING")], "Highest rail voltage after the input rises through mid-level."),
    module(DC_RAIL, "DC Rail", "V", Pulse, &[0], &[(RAILS, "RAIL SEQUENCING")], "Mean voltage of the rail."),
];

pub struct Rails;

impl MeasModule for Rails {
    fn key(&self) -> &'static str {
        "rails"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        v.push(m.pw_ref.map_or(u64::MAX, f64::to_bits));
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let rail = x.wave(a).map(|v| {
            let vin = if a != b { x.wave(b) } else { None };
            oos_dsp::power::rail(v, x.dt, x.model.pw_ref.filter(|v| v.is_finite() && *v != 0.0), vin, None)
        });
        let values = ids
            .iter()
            .map(|&id| {
                let v = rail.as_ref().map_or(f64::NAN, |r| match id {
                    RAIL_OVER => r.overshoot,
                    RAIL_UNDER => r.undershoot,
                    RAIL_TURN => r.turn_on,
                    _ => r.dc,
                });
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}
