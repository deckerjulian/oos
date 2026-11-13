// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The FPGA's DVM block next to the frequency counter (docs/fork/dvm.md, spec 08 section 1.2): it accumulates 2^exp
//! samples of one ADC lane, a sum and a second sum, independent of the time base and the record. Fork, opt-in
//! (`dvm.hw = on`), NOT VERIFIED ON THE UNIT.
//!
//! Register facts (BAR1). Write side netlist-proven in simulation, read side netlist-proven as status flip-flops, the
//! meaning of the results not measured yet:
//! - `CFG` 0x2244: bit 0 a pulse (written 1, then 0 with the new fields; the stock driver does this on every
//!   reconfiguration, so it is taken as a restart), [5:1] exp, [9:8] lane. Reads back [9:8,5:0] plus status bits.
//! - `LATCH` 0x2248 bit 0: write 1 then 0 to copy the results into the read registers.
//! - `SUM_LO` 0x224c, `SUM_HI` 0x2250 [31:16]: the first result, 48 bits (bits 15:0 of 0x2250 have no flip-flop).
//! - `SQ_LO` 0x2254, `SQ_HI` 0x2258: the second result, 64 bits.
//!
//! Open (decided by the unit session, `docs/fork/dvm.md`): whether the samples are on the 16-bit read-out scale
//! (`SCALE`), whether the second result is the sum of squares, and how long a window of 2^exp samples takes.
//! The decode refuses readings that cannot be a sum and a sum of squares of 16-bit codes; the UI then keeps the
//! software DVM.

use super::bus::Bus;
use super::levels::Vert;
use super::Write;

pub const CFG: u32 = 0x2244;
pub const LATCH: u32 = 0x2248;
pub const SUM_LO: u32 = 0x224c;
pub const SUM_HI: u32 = 0x2250;
pub const SQ_LO: u32 = 0x2254;
pub const SQ_HI: u32 = 0x2258;

const RESTART: u32 = 1;
const EXP_SHIFT: u32 = 1;
const EXP_MASK: u32 = 0x1f;
const LANE_SHIFT: u32 = 8;
const LANE_MASK: u32 = 3;
/// The upper 16 bits of the first result sit in `SUM_HI` [31:16].
const SUM_HI_SHIFT: u32 = 16;

/// Samples per window, log2. The stock driver's default (2^28).
pub const EXP: u32 = 28;

/// Mid-scale and full scale of the samples the block sums (16-bit codes).
const MID: f64 = 32768.0;
const FULL: f64 = 65536.0;
/// Read-out codes per division (`seq::volts`).
const CODES_PER_DIV: f64 = 7500.0;

/// Which 16-bit scale the block sees. `ReadOut`: the words of the records (after the read-path gain and offset), so
/// `seq::volts` applies. `PreGain`: the scale of the measure unit (before the digital gain), so `levels::volts`
/// applies. An assumption until the unit session; change here if the DC comparison says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    ReadOut,
    PreGain,
}
pub const SCALE: Scale = Scale::ReadOut;

/// The DVM modes of the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dc,
    AcRms,
    AcDcRms,
}

/// The lane of channel `ch` (0..3) for the acquired channel set `mask`: the channel itself with one, three or four
/// channels; with two channels the lower one is lane 0 and the higher lane 2 (`acq::lanes` lays out the pair the same
/// way). None when the channel is not acquired: the block then sees no samples of it.
pub fn lane(mask: u32, ch: usize) -> Option<u32> {
    if ch > 3 || mask & (1 << ch) == 0 {
        return None;
    }
    if crate::dho::hz::ratio(mask) == 2 {
        let lower = (0..ch).all(|c| mask & (1 << c) == 0);
        return Some(if lower { 0 } else { 2 });
    }
    Some(ch as u32)
}

fn fields(lane: u32, exp: u32) -> u32 {
    (lane & LANE_MASK) << LANE_SHIFT | (exp & EXP_MASK) << EXP_SHIFT
}

/// The writes that (re)start the block on `lane` with windows of 2^`exp` samples: the fields with the restart bit,
/// then without it.
pub fn start(lane: u32, exp: u32) -> Vec<Write> {
    let f = fields(lane, exp);
    vec![(CFG, f | RESTART), (CFG, f)]
}

/// The writes that stop it (as the stock driver: a restart pulse, then everything zero).
pub fn stop() -> Vec<Write> {
    vec![(CFG, RESTART), (CFG, 0)]
}

/// The registers of one reading, as read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Raw {
    /// `CFG` read back (status bits included)
    pub cfg: u32,
    pub sum_lo: u32,
    pub sum_hi: u32,
    pub sq_lo: u32,
    pub sq_hi: u32,
}

/// Latches the results and reads them.
pub fn read(bus: &mut impl Bus) -> Raw {
    bus.wr(LATCH, 1);
    bus.wr(LATCH, 0);
    Raw { cfg: bus.rd(CFG), sum_lo: bus.rd(SUM_LO), sum_hi: bus.rd(SUM_HI), sq_lo: bus.rd(SQ_LO), sq_hi: bus.rd(SQ_HI) }
}

impl Raw {
    /// The first result, 48 bits.
    pub fn sum(&self) -> u64 {
        ((self.sum_hi >> SUM_HI_SHIFT) as u64) << 32 | self.sum_lo as u64
    }

    /// The second result, 64 bits.
    pub fn sq(&self) -> u64 {
        (self.sq_hi as u64) << 32 | self.sq_lo as u64
    }
}

/// A decoded reading in codes: mean and mean square of the samples of one window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub mean: f64,
    pub mean_sq: f64,
}

/// The reading of `r` taken with windows of 2^`exp` samples. None when it cannot be a sum and a sum of squares of
/// 16-bit codes: both zero (the block idle), a mean outside the code range, or a mean square below the square of the
/// mean (Cauchy-Schwarz) or above full scale squared.
pub fn decode(r: &Raw, exp: u32) -> Option<Reading> {
    let (sum, sq) = (r.sum(), r.sq());
    if sum == 0 && sq == 0 {
        return None;
    }
    let n = (1u64 << (exp & EXP_MASK)) as f64;
    let mean = sum as f64 / n;
    let mean_sq = sq as f64 / n;
    let ok = mean < FULL && mean_sq < FULL * FULL && mean_sq >= mean * mean * (1.0 - 1e-9) - 1.0;
    ok.then_some(Reading { mean, mean_sq })
}

/// Volts per code for the vertical set-up `v` on scale `s`.
fn volts_per_code(v: &Vert, s: Scale) -> f64 {
    let dg = match s {
        Scale::ReadOut => 1.0,
        Scale::PreGain if v.gain >= 2.0 => v.gain / 2.0,
        Scale::PreGain => v.gain,
    };
    dg * v.scale / CODES_PER_DIV
}

/// The DVM value of a reading in volts as the records show them: DC = the mean; AC rms = the standard deviation;
/// AC+DC rms = the rms of the volts (offset included).
pub fn volts(r: &Reading, mode: Mode, v: &Vert, s: Scale) -> f64 {
    let k = volts_per_code(v, s);
    // first and second moment of the sample around mid-scale, in codes
    let m1 = r.mean - MID;
    let m2 = r.mean_sq - 2.0 * MID * r.mean + MID * MID;
    match mode {
        Mode::Dc => k * m1 - v.offset,
        Mode::AcRms => k * (r.mean_sq - r.mean * r.mean).max(0.0).sqrt(),
        Mode::AcDcRms => (k * k * m2 - 2.0 * k * v.offset * m1 + v.offset * v.offset).max(0.0).sqrt(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dho::bus::Recorder;

    #[test]
    fn lanes_follow_the_channel_set() {
        let cases: [(u32, usize, Option<u32>); 9] = [
            (0b0001, 0, Some(0)),
            (0b0100, 2, Some(2)),
            (0b0011, 0, Some(0)),
            (0b0011, 1, Some(2)),
            (0b1010, 1, Some(0)),
            (0b1010, 3, Some(2)),
            (0b1111, 2, Some(2)),
            (0b0111, 1, Some(1)),
            (0b0011, 2, None),
        ];
        for (mask, ch, want) in cases {
            assert_eq!(lane(mask, ch), want, "mask {:#b} ch {}", mask, ch);
        }
    }

    #[test]
    fn start_and_stop_writes() {
        assert_eq!(start(0, 28), vec![(CFG, 0x39), (CFG, 0x38)]);
        assert_eq!(start(2, 20), vec![(CFG, 0x229), (CFG, 0x228)]);
        assert_eq!(start(3, 0x3f)[1].1, 0x33e);
        assert_eq!(stop(), vec![(CFG, 1), (CFG, 0)]);
    }

    #[test]
    fn read_latches_then_reads() {
        let mut b = Recorder::default();
        for (r, v) in [(CFG, 0x38), (SUM_LO, 0x8000_0000), (SUM_HI, 0x0012_beef), (SQ_LO, 5), (SQ_HI, 7)] {
            b.regs.insert(r, v);
        }
        let r = read(&mut b);
        assert_eq!(b.writes, vec![(LATCH, 1), (LATCH, 0)]);
        assert_eq!(r.sum(), 0x12_8000_0000);
        assert_eq!(r.sq(), 0x7_0000_0005);
        assert_eq!(r.cfg, 0x38);
    }

    /// Raw words for `n` = 2^exp samples of mean `mean` and mean square `ms` (both exact in codes).
    fn raw(exp: u32, mean: u64, ms: u64) -> Raw {
        let (s, q) = (mean << exp, ms << exp);
        Raw { cfg: 0, sum_lo: s as u32, sum_hi: ((s >> 32) as u32) << 16 | 0xabcd, sq_lo: q as u32, sq_hi: (q >> 32) as u32 }
    }

    #[test]
    fn decode_accepts_sums_and_refuses_the_impossible() {
        let ok = |exp, m: u64, ms: u64| decode(&raw(exp, m, ms), exp);
        // DC at mid-scale, 2^28 samples: the low half of SUM_HI is ignored
        assert_eq!(ok(28, 32768, 32768 * 32768), Some(Reading { mean: 32768.0, mean_sq: 1073741824.0 }));
        // a square wave 30000 / 36000: mean 33000, mean square (30000^2 + 36000^2) / 2
        assert_eq!(ok(20, 33000, 1_098_000_000).map(|r| r.mean), Some(33000.0));
        // idle block
        assert_eq!(decode(&Raw::default(), 28), None);
        // mean square below the mean squared: not a sum of squares
        assert_eq!(ok(20, 33000, 1_000_000_000), None);
        // mean beyond the 16-bit range (a 48-bit sum with the wrong exponent)
        assert_eq!(decode(&raw(28, 32768, 32768 * 32768), 10), None);
    }

    #[test]
    fn volts_by_mode_and_scale() {
        let v = Vert { scale: 1.0, offset: 0.5, gain: 2.4 };
        // square wave between 32768 - 7500 and 32768 + 7500 codes: +-1 div around mid-scale
        let lo = 32768.0 - 7500.0;
        let hi = 32768.0 + 7500.0;
        let r = Reading { mean: (lo + hi) / 2.0, mean_sq: (lo * lo + hi * hi) / 2.0 };
        let cases: [(Mode, Scale, f64); 6] = [
            (Mode::Dc, Scale::ReadOut, -0.5),
            (Mode::AcRms, Scale::ReadOut, 1.0),
            // volts -1.5 and 0.5: rms sqrt((2.25 + 0.25) / 2)
            (Mode::AcDcRms, Scale::ReadOut, 1.25f64.sqrt()),
            (Mode::Dc, Scale::PreGain, -0.5),
            (Mode::AcRms, Scale::PreGain, 1.2),
            (Mode::AcDcRms, Scale::PreGain, ((1.2f64 - 0.5).powi(2) / 2.0 + (1.2f64 + 0.5).powi(2) / 2.0).sqrt()),
        ];
        for (mode, s, want) in cases {
            let got = volts(&r, mode, &v, s);
            assert!((got - want).abs() < 1e-9, "{:?} {:?}: {} != {}", mode, s, got, want);
        }
        // a DC level: AC rms zero, AC+DC rms the absolute level
        let dc = Reading { mean: 32768.0 + 3750.0, mean_sq: (32768.0f64 + 3750.0).powi(2) };
        let v0 = Vert { scale: 2.0, offset: 0.0, gain: 1.0 };
        assert!((volts(&dc, Mode::Dc, &v0, Scale::ReadOut) - 1.0).abs() < 1e-12);
        assert!(volts(&dc, Mode::AcRms, &v0, Scale::ReadOut).abs() < 1e-3);
        assert!((volts(&dc, Mode::AcDcRms, &v0, Scale::ReadOut) - 1.0).abs() < 1e-9);
    }
}
