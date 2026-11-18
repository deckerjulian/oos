// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! DMX512 (with RDM): 250 kbit/s on an RS-485 pair, break, mark after break, start code, up to 512 slots.

use super::*;
use oos_dsp::decode;

pub struct Dmx512;

impl Protocol for Dmx512 {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::DMX512
    }
    /// A break, the start code and eight slots, 3.3 V logic.
    fn demo(&self) -> Option<Demo> {
        let mut w = decode::gen::Wave::new(0.0, 3.3, 20e-9);
        w.level(true, 20e-6);
        decode::gen::dmx(&mut w, 0, &[255, 128, 64, 32, 16, 8, 4, 0]);
        w.level(true, 50e-6);
        let a = w.done();
        let period = a.seg.last().map_or(1e-3, |s| s.0);
        Some(Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] })
    }
}
