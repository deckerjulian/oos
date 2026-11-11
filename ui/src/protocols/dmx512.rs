// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! DMX512 (with RDM): 250 kbit/s on an RS-485 pair, break, mark after break, start code, up to 512 slots.

use super::*;
use oos_dsp::decode_ext as x1;

pub struct Dmx512;

impl Protocol for Dmx512 {
    fn key(&self) -> &'static str {
        "Dmx512"
    }
    fn name(&self) -> &'static str {
        "DMX512"
    }
    fn diff_ok(&self) -> bool {
        true
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        4e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(x1::dmx512(l.main(), l.dt, l.thr, l.hy, false))
    }
    /// A break, the start code and eight slots, 3.3 V logic.
    fn demo(&self) -> Option<Demo> {
        let mut w = x1::gen::Wave::new(0.0, 3.3, 20e-9);
        w.level(true, 20e-6);
        x1::gen::dmx(&mut w, 0, &[255, 128, 64, 32, 16, 8, 4, 0]);
        w.level(true, 50e-6);
        let a = w.done();
        let period = a.seg.last().map_or(1e-3, |s| s.0);
        Some(Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] })
    }
}
