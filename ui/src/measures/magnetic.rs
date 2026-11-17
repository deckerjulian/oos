// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Magnetics from the voltage across a winding (source A) and the current through it (source B): inductance,
//! permeability and core loss, with the core geometry of the Magnetic page.

use super::*;
use Mark::*;

pub const INDUCTANCE: u16 = 110;
pub const MAG_PROP: u16 = 111;
pub const MAG_LOSS: u16 = 112;

static DEFS: [Def; 3] = [
    module(
        INDUCTANCE,
        "Inductance",
        "H",
        Loop,
        &[0, 1],
        &[(POWER, "MAGNETICS")],
        "Slope of flux linkage against current on the rising branch.",
    ),
    module(
        MAG_PROP,
        "Magnetic Property",
        "",
        Loop,
        &[0, 1],
        &[(POWER, "MAGNETICS")],
        "Relative permeability when turns, area and path length are set.",
    ),
    module(
        MAG_LOSS,
        "Magnetic Loss",
        "W",
        Loop,
        &[0, 1],
        &[(POWER, "MAGNETICS")],
        "Power left after the copper loss is taken out.",
    ),
];

pub struct Magnetic;

impl MeasModule for Magnetic {
    fn key(&self) -> &'static str {
        "magnetics"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        for x in [m.mag_n, m.mag_ae, m.mag_le, m.mag_rcu] {
            put(v, x);
        }
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let (a, b) = (x.row[0], x.row[1]);
        let _ = (a, b);
        let m = x.model;
        let geom = oos_dsp::magnetic::Geom {
            turns: m.mag_n.max(1.0),
            area: (m.mag_ae > 0.0).then_some(m.mag_ae),
            length: (m.mag_le > 0.0).then_some(m.mag_le),
            rcu: m.mag_rcu.max(0.0),
        };
        let g = x.align(&[a, b]).map(|v| oos_dsp::magnetic::measure(v[0], v[1], x.dt, geom));
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    INDUCTANCE => g.as_ref().and_then(|g| g.l).unwrap_or(f64::NAN),
                    MAG_PROP => g.as_ref().and_then(|g| g.mu_r).unwrap_or(f64::NAN),
                    MAG_LOSS => g.as_ref().and_then(|g| g.loss).unwrap_or(f64::NAN),
                    _ => f64::NAN,
                };
                (id, v)
            })
            .collect();
        Values { values, quality: DERIVED }
    }
}
