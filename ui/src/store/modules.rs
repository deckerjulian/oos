// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the store asks of oos-ui, which stays above it (ADR 0049, ADR 0064): the registries of the measurement and
//! the decoder modules (`Registries`), and the screen's record (`Samples`). The readers of a setup and of OOS's
//! waveform files take them as arguments; the functions here pass oos-ui's in, so the callers say
//! `store::parse_setup_lenient(text)` and `store::parse_ff(bytes)` as before.

use super::{Registries, Samples};
use crate::model::{Model, NCH};
use crate::wave::Record;
use oos_dsp::protocols::OPTS;

/// The registries of oos-ui: `measures::MODULES` and `protocols::MODULES`.
struct Modules;

impl Registries for Modules {
    fn catalog_id_valid(&self, id: u16) -> bool {
        crate::measures::catalog_id_valid(id)
    }

    fn legacy_opts(&self, i: u8, m: &Model) -> Option<[u8; OPTS]> {
        let old = crate::protocols::Legacy {
            baud: m.dec_baud,
            bits: m.dec_bits,
            parity: m.dec_parity,
            canrate: m.dec_canrate,
            canh: m.dec_canh,
            fdrate: m.dec_fdrate,
            arate: m.dec_arate,
            tick: m.dec_tick,
            nibbles: m.dec_nibbles,
        };
        let o = crate::protocols::get(i).legacy(&old)?;
        Some(crate::protocols::opt_fit(i, o.map(|x| x as u8)).map(|x| x as u8))
    }
}

impl Samples for Record {
    fn t0(&self) -> f64 {
        self.t0
    }
    fn dt(&self) -> f64 {
        self.dt
    }
    fn ch(&self) -> &[Vec<f32>; NCH] {
        &self.ch
    }
    fn from_file(t0: f64, dt: f64, ch: [Vec<f32>; NCH]) -> Record {
        Record { t0, dt, ch, triggered: true, partial: false, raw: None, la: None, pairs: false }
    }
}

/// Takes one `key = value` line of a setup into the model (`setup_read::set`).
pub fn set(m: &mut Model, k: &str, v: &str) -> Result<(), String> {
    super::setup_read::set(m, k, v, &Modules)
}

/// A setup file on top of the default set-up, with the lines it could not take (`setup_read::parse_setup_lenient`).
pub fn parse_setup_lenient(text: &str) -> (Model, Vec<String>) {
    super::setup_read::parse_setup_lenient(text, &Modules)
}

/// Parses a setup file on top of the default set-up (keys missing in the file keep their defaults). Errors name the
/// line and the key; unknown keys are ignored.
#[cfg(test)] // the strict form, for the tests
pub fn parse_setup(text: &str) -> Result<Model, String> {
    let (m, bad) = parse_setup_lenient(text);
    match bad.into_iter().next() {
        Some(e) => Err(e),
        None => Ok(m),
    }
}

/// One OOSW blob as a record of the screen, and the number of bytes consumed (`waveform::parse_wave_bin`).
#[cfg(test)] // the frame files are read whole (`parse_ff`); the tests read one blob
pub fn parse_wave_bin(b: &[u8]) -> Result<(Record, usize), String> {
    super::waveform::parse_wave_bin(b)
}

/// The records of a fast-frame file (`waveform::parse_ff`).
pub fn parse_ff(b: &[u8]) -> Result<Vec<Record>, String> {
    super::waveform::parse_ff(b)
}
