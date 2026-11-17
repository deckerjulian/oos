// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! NRZ rate from the transitions of one source: the unit interval and the data rate it gives.

use super::*;
use Mark::*;

pub const UNIT_INTERVAL: u16 = 15;
pub const DATA_RATE: u16 = 16;

static DEFS: [Def; 2] = [
    module(UNIT_INTERVAL, "Unit Interval", "s", Square, &[0], &[(SERIAL, "CLOCK & UNIT INTERVAL")], "Estimated NRZ UI from transition intervals divided by their fitted bit counts."),
    module(DATA_RATE, "NRZ Data Rate", "b/s", Square, &[0], &[(SERIAL, "CLOCK & UNIT INTERVAL")], "Reciprocal of the estimated mean NRZ UI, not the PRBS cycle frequency. Assumes one bit per symbol; not PAM or C-PHY throughput."),
];

pub struct Rate;

impl MeasModule for Rate {
    fn key(&self) -> &'static str {
        "nrz_rate"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let Some(v) = x.wave(x.row[0]) else {
            return Values {
                values: ids.iter().map(|&id| (id, f64::NAN)).collect(),
                quality: "NRZ estimate: insufficient transitions",
            };
        };
        let analysis = oos_dsp::measure::analyze(v, x.dt);
        let interval = analysis.est_ui;
        // A clock cycle period on random NRZ data averages several UIs: neither it nor a SIM/eye preset is a
        // substitute for the recovered bit spacing.
        let ok = interval.is_finite() && interval >= 2.0 * x.dt && analysis.rises.len() + analysis.falls.len() >= 3;
        let quality = if !interval.is_finite() || interval < 2.0 * x.dt {
            "NRZ estimate: insufficient transitions or sampling"
        } else if interval / x.dt < 10.0 {
            "NRZ estimate: limited sampling"
        } else {
            "NRZ estimate: transition intervals / fitted bit counts"
        };
        let values = ids
            .iter()
            .map(|&id| {
                (
                    id,
                    if !ok {
                        f64::NAN
                    } else if id == UNIT_INTERVAL {
                        interval
                    } else {
                        1.0 / interval
                    },
                )
            })
            .collect();
        Values { values, quality }
    }

    fn pending_quality(&self, _id: u16) -> &'static str {
        "NRZ UI estimate"
    }

    fn missing_reason(&self, _id: u16) -> Option<&'static str> {
        Some("NRZ UI unrecoverable; need transitions and at least 2 samples/UI")
    }
}
