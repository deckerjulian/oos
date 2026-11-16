// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! An example measurement module (compiled in the tests only): crest factor and form factor of one source. It uses
//! every hook of [`MeasModule`]; `docs/measurement-modules.md` walks through it.

use super::*;
use Mark::*;

/// Numbers: unique over all modules and never reused (saved setups and SCPI `CAT<id>` refer to them).
pub const CREST_FACTOR: u16 = 1000;
pub const FORM_FACTOR: u16 = 1001;

static DEFS: [Def; 2] = [
    module(CREST_FACTOR, "Crest Factor", "", Bars, &[0], &[(BASIC, "EXAMPLE")], "Peak magnitude divided by the RMS."),
    module(FORM_FACTOR, "Form Factor", "", Bars, &[0], &[(BASIC, "EXAMPLE")], "RMS divided by the mean magnitude."),
];

pub struct Example;

impl MeasModule for Example {
    fn key(&self) -> &'static str {
        "example"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }

    /// The values depend on nothing but the source here; a module with a setting of its own pushes it, so a change
    /// computes the values again (the eye modules push the BER, the power modules the line frequency).
    fn settings(&self, _m: &Model, _v: &mut Vec<u64>) {}

    /// Two measurements on the same source need no checks; a module that needs distinct sources says so here, and a
    /// module that reads a source outside the row (a fourth probe set on its page) pushes it.
    fn sources(&self, _id: u16, _row: [WaveId; 4], _m: &Model, _ids: &mut Vec<WaveId>) -> Result<(), &'static str> {
        Ok(())
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let v = x.wave(x.row[0]).unwrap_or(&[]);
        let n = v.len().max(1) as f64;
        let rms = (v.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / n).sqrt();
        let peak = v.iter().map(|s| s.abs() as f64).fold(0.0, f64::max);
        let mean_abs = v.iter().map(|s| s.abs() as f64).sum::<f64>() / n;
        let ratio = |a: f64, b: f64| if b > 0.0 && !v.is_empty() { a / b } else { f64::NAN };
        let values = ids.iter().map(|&id| (id, if id == CREST_FACTOR { ratio(peak, rms) } else { ratio(rms, mean_abs) })).collect();
        Values { values, quality: DERIVED }
    }

    fn pending_quality(&self, _id: u16) -> &'static str {
        DERIVED
    }

    fn missing_reason(&self, _id: u16) -> Option<&'static str> {
        Some("Source is zero")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine has a crest factor of sqrt(2) and a form factor of pi / (2 sqrt(2)).
    #[test]
    fn crest_and_form_factor_of_a_sine() {
        let m = Model::default();
        let sine: Vec<f32> = (0..10_000).map(|k| (k as f32 * std::f32::consts::TAU / 1000.0).sin()).collect();
        let row = [WaveId::Ch(0); 4];
        let waves = vec![(WaveId::Ch(0), sine)];
        let x = Ctx { model: &m, t0: 0.0, dt: 1e-6, row, waves: &waves };
        let v = Example.compute(&x, &[CREST_FACTOR, FORM_FACTOR]).values;
        assert!((v[0].1 - 2f64.sqrt()).abs() < 1e-3, "{v:?}");
        assert!((v[1].1 - std::f64::consts::PI / (2.0 * 2f64.sqrt())).abs() < 1e-3, "{v:?}");
    }
}
