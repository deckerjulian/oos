// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Parallel bus, clocked or sampled on every change.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{Kind, Packet};
use crate::edges::{edges, states};
use crate::protocols::{every_word_is_a_frame, ModuleInfo};
use crate::trigger::Slope;

/// A parallel bus: the data lines from the main one upwards, and a clock.
pub static PARALLEL: ModuleInfo = ModuleInfo::new("Parallel", "Parallel", "parallel")
    .roles([Some("D0"), None, Some("Clock"), None])
    .trigger(every_word_is_a_frame)
    .bit(|_, _| 0.5e-6);

/// Parallel bus: the data lines (`data[0]` = LSB, at most 32 are used) sampled on each `edge` of `clk`, or without a
/// clock after every change of any line (changes less than 2 samples apart count as one, for line skew; the first
/// packet is the bus word at the record start, when all lines are defined). One Data packet per sample: value = the bus word, t0 = the sampling
/// time, t1 = the next one (or the record end); `ok` = every line had a defined level.
pub fn parallel(data: &[&[f32]], clk: Option<&[f32]>, edge: Slope, thr: f32, hyst: f32) -> Vec<Packet> {
    let data = &data[..data.len().min(32)];
    let mut out = Vec::new();
    let n = data.iter().map(|d| d.len()).min().unwrap_or(0);
    if n == 0 {
        return out;
    }
    let st: Vec<Vec<Option<bool>>> = data.iter().map(|d| states(d, thr, hyst)).collect();
    let word = |i: usize| {
        let i = i.min(n - 1);
        let mut v = 0u32;
        let mut ok = true;
        for (k, s) in st.iter().enumerate() {
            match s[i] {
                Some(true) => v |= 1 << k,
                Some(false) => {}
                None => ok = false,
            }
        }
        (v, ok)
    };
    // (time, sample index to read)
    let mut at: Vec<(f64, usize)> = Vec::new();
    if let Some(c) = clk {
        for e in edges(c, thr, hyst) {
            let m = match edge {
                Slope::Rising => e.rising,
                Slope::Falling => !e.rising,
                Slope::Either => true,
            };
            if m {
                at.push((e.t, (e.t + 0.5) as usize));
            }
        }
    } else {
        const SKEW: f64 = 2.0;
        let mut ts: Vec<f64> = data.iter().flat_map(|d| edges(&d[..n], thr, hyst)).map(|e| e.t).collect();
        ts.sort_by(|a, b| a.total_cmp(b));
        let mut i = 0;
        while i < ts.len() {
            let t0 = ts[i];
            let mut t1 = t0;
            while i + 1 < ts.len() && ts[i + 1] - t1 <= SKEW {
                i += 1;
                t1 = ts[i];
            }
            at.push((t0, t1.ceil() as usize + 1));
            i += 1;
        }
        let before = at.first().map_or(n - 1, |a| (a.0.floor() as usize).saturating_sub(1));
        if word(before).1 {
            at.insert(0, (0.0, before));
        }
    }
    for (k, &(t, i)) in at.iter().enumerate() {
        let (v, ok) = word(i);
        let t1 = at.get(k + 1).map_or((n - 1) as f64, |a| a.0);
        out.push(Packet::new(Kind::Data, t, t1.max(t), v, ok));
    }
    out
}
