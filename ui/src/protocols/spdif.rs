// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! S/PDIF (IEC 60958): biphase-mark coded audio frames, preambles B / M / W, 24-bit samples, channel status.

use super::*;
use oos_dsp::decode_ext2 as x2;

pub struct Spdif;

impl Protocol for Spdif {
    fn key(&self) -> &'static str {
        "Spdif"
    }
    fn name(&self) -> &'static str {
        "S/PDIF"
    }
    fn bit_time(&self, _: &[usize; OPTS]) -> f64 {
        0.15e-6
    }
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str> {
        Ok(x2::spdif(l.main(), l.thr, l.hy))
    }
    /// Six stereo frames at 48 kHz, 3.3 V logic.
    fn demo(&self) -> Option<Demo> {
        let a = x2::gen::spdif(&[(0x12345, 0x6789A); 6], &[0u8; 24], 48e3, &[], 0.0, 3.3);
        let period = a.seg.last().map_or(1e-3, |s| s.0);
        Some(Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] })
    }
}
