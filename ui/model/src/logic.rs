// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The record of the logic analyser: the 16 lines as packed words (bit n = Dn), as the demo source makes them and
//! the board reads them after the analog record, and the questions the trigger and the measurements ask of it.

use crate::model::Bit;
use std::sync::Arc;

/// Samples of a demo LA record (8 per pixel column of the full-width plot: the bus-style bands show).
pub const DEMO_SAMPLES: usize = 8192;

/// Whether the word `w` matches `pat`.
pub fn pattern_is(w: u16, pat: &[Bit; 16]) -> bool {
    pat.iter().enumerate().all(|(n, b)| match b {
        Bit::H => w >> n & 1 == 1,
        Bit::L => w >> n & 1 == 0,
        _ => true,
    })
}

/// First sample from `from` on where the lines match `pat` after they did not (the moment the pattern begins, as the
/// TPU fires it); a pattern already true at `from` counts from its next beginning. All X: None.
pub fn pattern_at(words: &[u16], pat: &[Bit; 16], from: usize) -> Option<usize> {
    let (mut mask, mut want) = (0u16, 0u16);
    for (n, b) in pat.iter().enumerate() {
        match b {
            Bit::H => (mask, want) = (mask | 1 << n, want | 1 << n),
            Bit::L => mask |= 1 << n,
            _ => {}
        }
    }
    if mask == 0 {
        return None;
    }
    let hit = |w: u16| w & mask == want;
    (from.max(1)..words.len()).find(|&k| hit(words[k]) && !hit(words[k - 1]))
}

/// Whether line `n` changes anywhere in the record.
pub fn line_moves(la: &LaRec, n: u8) -> bool {
    la.words.windows(2).any(|w| (w[0] ^ w[1]) >> n & 1 == 1)
}

/// The 16 lines of a record as packed words: word k (bit n = Dn) at time t0 + k dt relative to the trigger.
#[derive(Clone, Debug)]
pub struct LaRec {
    pub words: Arc<Vec<u16>>,
    pub t0: f64,
    pub dt: f64,
}
