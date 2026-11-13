// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The FPGA's DVM block next to the frequency counter (docs/fork/dvm.md, spec 08 section 1.2): it accumulates 2^exp
//! samples of one ADC lane, their sum and their sum of squares, independent of the time base and the record. Fork,
//! opt-in (`dvm.hw = on`).
//!
//! Register facts (BAR1). Write side netlist-proven in simulation; result layout and units measured on the unit
//! (2026-11-13, round 2):
//! - `CFG` 0x2244: bit 0 a pulse (written 1, then 0 with the new fields; the stock driver does this on every
//!   reconfiguration, so it is taken as a restart), [5:1] exp, [9:8] lane. Read back it is not the configuration: the
//!   unit returned `SQ_HI` with bits 7:0 replaced.
//! - `LATCH` 0x2248 bit 0: write 1 then 0 to copy the results into the read registers.
//! - `SUM_LO` 0x224c, `SUM_HI` 0x2250 [31:16]: the sum, 48 bits.
//! - `SQ_LO` 0x2254, `SQ_HI` 0x2258: the sum of squares, 64 bits.
//! - The samples are the stored 12-bit ADC codes, mid-scale 2048 (open input: mean 2046.9, sigma 3.5), before the
//!   read-path gain: their volts need the channel's digital gain (`volts`). A window of 2^28 samples ends within
//!   0.6 s. After STOP one reading is nearly all zero; `decode` refuses it.

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

/// Mid-scale and full scale of the samples the block sums (stored 12-bit ADC codes).
const MID: f64 = 2048.0;
const FULL: f64 = 4096.0;
/// One stored code in read-out codes: the read-out word is the code shifted to 16 bits, then the read-path gain
/// (`wpu.rs`, `levels::volts`).
const READOUT_PER_CODE: f64 = 16.0;
/// A mean below one code is not a reading: after STOP the block returns one window of (nearly) zero words.
const MIN_MEAN: f64 = 1.0;

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

/// `lane` with a requested lane `forced` (0..3): taken only with one acquired channel, where every lane carries it
/// (unit debug: each lane is a different pair of ADC cores, so their DC readings show the core offsets).
pub fn lane_forced(mask: u32, ch: usize, forced: Option<u32>) -> Option<u32> {
    let l = lane(mask, ch)?;
    Some(match forced {
        Some(f) if crate::dho::hz::ratio(mask) == 1 && (mask & 0xf).count_ones() == 1 => f & LANE_MASK,
        _ => l,
    })
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

/// A decoded reading in stored codes: mean and mean square of the samples of one window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub mean: f64,
    pub mean_sq: f64,
}

/// The reading of `r` taken with windows of 2^`exp` samples. None when it cannot be a sum and a sum of squares of
/// 12-bit codes: a mean below one code (the window after STOP), at or above full scale, or a mean square below the
/// square of the mean (Cauchy-Schwarz) or at or above full scale squared.
pub fn decode(r: &Raw, exp: u32) -> Option<Reading> {
    let n = (1u64 << (exp & EXP_MASK)) as f64;
    let mean = r.sum() as f64 / n;
    let mean_sq = r.sq() as f64 / n;
    let ok = (MIN_MEAN..FULL).contains(&mean) && mean_sq < FULL * FULL && mean_sq >= mean * mean * (1.0 - 1e-9) - 1.0;
    ok.then_some(Reading { mean, mean_sq })
}

/// Volts per stored code for the vertical set-up `v`: the read-out's 7500 codes per division, through the read-path
/// gain. A digital gain of 2 and more is split into the ADC x2 of 0x105c, which acts before the samples are stored,
/// and the read-path rest (`levels::volts` does the same for the measure unit).
fn volts_per_code(v: &Vert) -> f64 {
    let dg = if v.gain >= 2.0 { v.gain / 2.0 } else { v.gain };
    READOUT_PER_CODE * dg * v.scale / 7500.0
}

/// The DVM value of a reading in volts as the records show them: DC = the mean; AC rms = the standard deviation;
/// AC+DC rms = the rms of the volts (offset included).
pub fn volts(r: &Reading, mode: Mode, v: &Vert) -> f64 {
    let k = volts_per_code(v);
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
    fn forced_lanes_only_with_one_channel() {
        let cases: [(u32, usize, Option<u32>, Option<u32>); 6] = [
            (0b0001, 0, Some(3), Some(3)),
            (0b0100, 2, Some(0), Some(0)),
            (0b0100, 2, None, Some(2)),
            // two or four channels: the lane is the channel's own
            (0b0011, 1, Some(0), Some(2)),
            (0b1111, 2, Some(1), Some(2)),
            (0b0001, 1, Some(1), None),
        ];
        for (mask, ch, forced, want) in cases {
            assert_eq!(lane_forced(mask, ch, forced), want, "mask {:#b} ch {} forced {:?}", mask, ch, forced);
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

    /// The raw words of a window of 2^28 samples with this mean and standard deviation in stored codes, laid out as
    /// the unit returns them (the low half of `SUM_HI` holds other bits).
    fn unit_raw(mean: f64, sigma: f64) -> Raw {
        let n = (1u64 << EXP) as f64;
        let sum = (mean * n).round() as u64;
        let sq = ((mean * mean + sigma * sigma) * n).round() as u64;
        Raw { cfg: (sq >> 32) as u32 & !0xff | 0x38, sum_lo: sum as u32, sum_hi: ((sum >> 32) as u32) << 16 | 0x00b8, sq_lo: sq as u32, sq_hi: (sq >> 32) as u32 }
    }

    // The readings of the unit session (2026-11-13, round 2), decoded and converted. Break it catches: the 16-bit
    // read-out scale (the old decode showed -0.67 V for 0.15 V), a missing digital gain, the STOP window taken as a
    // reading, the low half of SUM_HI in the sum.
    #[test]
    fn unit_readings_decode_to_their_volts() {
        // (mean, sigma, V/div, offset V, digital gain from the unit's cal file, DC V, AC rms V)
        let cases: [(f64, f64, f64, f64, f64, f64, f64); 4] = [
            // CH3 open, 1 V/div: a few mV
            (2046.9, 3.5, 1.0, 0.0, 1.0755, -0.00252, 0.00803),
            // CH3 open, 1 V/div, offset +2 V: mean 2904.0 (856 codes = 2 div at about 428 codes per division)
            (2904.0, 3.5, 1.0, 2.0, 1.0755, -0.035994, 0.00803),
            // CH1 probe-comp square (0..0.3 V), 500 mV/div: mean 0.15 V, AC rms = half the amplitude
            (2174.2, 131.5, 0.5, 0.0, 1.0715, 0.14424, 0.15030),
            // the same square at 100 mV/div, offset -0.15 V: the unit's sigma of 300 codes
            (2048.0, 300.0, 0.1, -0.15, 1.1707, 0.15, 0.07492),
        ];
        for (mean, sigma, scale, offset, gain, dc, ac) in cases {
            let r = decode(&unit_raw(mean, sigma), EXP).unwrap();
            assert!((r.mean - mean).abs() < 1e-6 && (r.mean_sq - mean * mean - sigma * sigma).abs() < 1e-3, "{:?}", r);
            let v = Vert { scale, offset, gain };
            let got = (volts(&r, Mode::Dc, &v), volts(&r, Mode::AcRms, &v));
            assert!((got.0 - dc).abs() < 2e-5 && (got.1 - ac).abs() < 2e-5, "mean {} sigma {}: {:?} != ({}, {})", mean, sigma, got, dc, ac);
            let acdc = volts(&r, Mode::AcDcRms, &v);
            assert!((acdc - got.0.hypot(got.1)).abs() < 1e-9, "{} vs {:?}", acdc, got);
        }
    }

    #[test]
    fn decode_refuses_what_cannot_be_a_window() {
        // idle block, and the STOP window (sum 0x900 over 2^28 samples)
        assert_eq!(decode(&Raw::default(), EXP), None);
        assert_eq!(decode(&Raw { sum_lo: 0x900, ..Raw::default() }, EXP), None);
        // mean square below the mean squared: not a sum of squares
        let mut r = unit_raw(2174.2, 131.5);
        r.sq_hi /= 2;
        assert_eq!(decode(&r, EXP), None);
        // mean beyond 12 bits: a sum read with the wrong exponent
        assert_eq!(decode(&unit_raw(2048.0, 3.0), 20), None);
        // the last valid code and a clipped-high input still decode
        assert!(decode(&unit_raw(4095.0, 0.0), EXP).is_some());
    }

    #[test]
    fn gain_of_two_and_more_is_split() {
        // 0.5 mV/div: digital gain 2.14, of which the ADC x2 acts before the samples are stored
        let r = Reading { mean: 2048.0 + 100.0, mean_sq: 2148.0f64.powi(2) };
        let v = Vert { scale: 0.5e-3, offset: 0.0, gain: 2.14 };
        assert!((volts(&r, Mode::Dc, &v) - 100.0 * 16.0 * 1.07 * 0.5e-3 / 7500.0).abs() < 1e-12);
    }
}
