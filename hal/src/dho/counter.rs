// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The FPGA's frequency counter (docs/protocols/counter.md): a reciprocal counter on one channel's comparator. Over a
//! gate it counts the signal's edges and the 0.8 ns ticks from the first to the last of them, so the result has about
//! the same relative resolution at every frequency (1.6 ns over the gate). It also keeps a single period, for signals
//! too slow for an edge pair in the gate, and a running total of edges.

use super::bus::Bus;

/// Registers (BAR1).
pub const THRESHOLD_A: u32 = 0x2200;
pub const THRESHOLD_B: u32 = 0x2204;
pub const ANALOG: u32 = 0x2208;
pub const CH_MASK: u32 = 0x220c;
pub const CTRL: u32 = 0x2210;
pub const GATE: u32 = 0x2214;
pub const LATCH: u32 = 0x2218;
/// results: start, end, edges, single period, total (each 64-bit, low word first)
pub const RESULTS: u32 = 0x221c;

/// `CTRL` bits: run, totalize, clear; the source in [9:5] and again in [14:10].
const RUN: u32 = 1;
const TOTALIZE: u32 = 2;
const CLEAR: u32 = 4;

/// The counter's tick and the gate register's unit.
pub const TICK: f64 = 0.8e-9;
pub const GATE_TICK: f64 = 6.4e-9;

/// What it counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Frequency,
    Period,
    Totalize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// analog channel 0..=3
    pub ch: usize,
    pub mode: Mode,
    /// resolution, significant digits (sets the gate)
    pub digits: u32,
}

/// The gate for a resolution: 0.25 s for 3 digits, 0.5 s for 4, 1 s above.
pub fn gate_s(digits: u32) -> f64 {
    match digits {
        0..=3 => 0.25,
        4 => 0.5,
        _ => 1.0,
    }
}

fn ctrl(c: &Config) -> u32 {
    let src = c.ch as u32 + 1;
    (src << 5) | (src << 10) | if c.mode == Mode::Totalize { TOTALIZE } else { 0 }
}

/// The writes that start the counter on `c` from zero: source, gate, a clear, then run.
pub fn start(c: &Config) -> Vec<(u32, u32)> {
    let w = ctrl(c);
    vec![
        (THRESHOLD_A, 0x8080),
        (THRESHOLD_B, 0x8080),
        (ANALOG, 1),
        (CH_MASK, 1 << c.ch),
        (CTRL, w),
        (GATE, (gate_s(c.digits) / GATE_TICK).round() as u32),
        (CTRL, w | CLEAR),
        (CTRL, w | RUN),
    ]
}

/// The writes that stop it.
pub fn stop() -> Vec<(u32, u32)> {
    vec![(CTRL, 0)]
}

/// One reading.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Raw {
    pub start: u64,
    pub end: u64,
    pub edges: u64,
    pub period: u64,
    pub total: u64,
    pub freq_valid: bool,
    pub period_valid: bool,
}

/// Latches the results and reads them.
pub fn read(bus: &mut impl Bus) -> Raw {
    bus.wr(LATCH, 1);
    let mut w = [0u32; 10];
    for (i, x) in w.iter_mut().enumerate() {
        *x = bus.rd(RESULTS + 4 * i as u32);
    }
    let q = |lo: u32, hi: u32| (hi as u64 & 0x7fff_ffff) << 32 | lo as u64;
    Raw {
        start: q(w[0], w[1]),
        end: q(w[2], w[3]),
        edges: q(w[4], w[5]),
        period: q(w[6], w[7]),
        total: (w[9] as u64) << 32 | w[8] as u64,
        freq_valid: w[1] & 0x8000_0000 != 0,
        period_valid: w[7] & 0x8000_0000 != 0,
    }
}

/// The signal's period from a reading with gate `gate_s`: from the edges in the gate, else (slow signals) from the
/// single period; None without either.
pub fn period(r: &Raw, gate_s: f64) -> Option<f64> {
    let g = (gate_s / TICK).round() as i64;
    if r.freq_valid && r.edges >= 2 {
        let span = g - r.start as i64 - r.end as i64;
        if span > 0 {
            return Some(span as f64 / (r.edges - 1) as f64 * TICK);
        }
    }
    // at least 1.6 ms: below about 625 Hz the edge count says less than one period
    (r.period_valid && r.period >= 2_000_000).then_some(r.period as f64 * TICK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::bus::Recorder;

    #[test]
    fn start_on_ch1_with_a_one_second_gate() {
        let w = start(&Config { ch: 0, mode: Mode::Frequency, digits: 6 });
        assert_eq!(w[3], (CH_MASK, 1));
        assert_eq!(w[4], (CTRL, 0x0420));
        assert_eq!(w[5], (GATE, 0x09502f90));
        assert_eq!(&w[6..], &[(CTRL, 0x0424), (CTRL, 0x0421)]);
        let t = start(&Config { ch: 1, mode: Mode::Totalize, digits: 4 });
        assert_eq!((t[3], t[4], t[5].1), ((CH_MASK, 2), (CTRL, 0x0842), 78_125_000));
    }

    /// Read on a DHO924S: CH1, 1 s gate, the built-in generator at 12345.6 Hz.
    #[test]
    fn a_reading_of_the_unit() {
        let mut b = Recorder::default();
        for (i, v) in [0xa4b0, 0x8000_0000, 0x48a8, 0, 0x303a, 0, 0x18b68, 0x8000_0000, 0x1881, 0].iter().enumerate() {
            b.regs.insert(RESULTS + 4 * i as u32, *v);
        }
        let r = read(&mut b);
        assert_eq!(b.writes, vec![(LATCH, 1)]);
        assert!(r.freq_valid && r.period_valid && r.edges == 12346 && r.total == 0x1881);
        let f = 1.0 / period(&r, 1.0).unwrap();
        assert!((f - 12345.6).abs() < 0.001, "{}", f);
        // no edges: the single period of a slow signal (2 ms), else nothing
        let slow = Raw { period: 2_500_000, period_valid: true, ..Default::default() };
        assert!((period(&slow, 1.0).unwrap() - 2e-3).abs() < 1e-12);
        assert_eq!(period(&Raw::default(), 1.0), None);
    }
}
