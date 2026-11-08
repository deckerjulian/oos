// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Horizontal parameters from time base, memory depth and channel mask: a port of `tools/hzmodel.py`
//! (docs/protocols/horizontal-config.md). Times are in femtoseconds.

pub const FS: i128 = 1_000_000_000_000_000;
/// ADC sample clock the model is made for. At another clock (`clock`, experimental) `prep` and `requested_rate`
/// take real times and return real rates; inside they work in this nominal domain, so the registers stay the same.
pub const CLOCK: i128 = 1_250_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// interleave factor 1/2/4 (not the channel count: 3 channels run in 4-channel mode)
    pub ratio: u32,
    /// sample rate per channel, Sa/s
    pub rate: u64,
    /// points per channel (0x10e4)
    pub memsel: u32,
    /// sample period in fs
    pub dot_fs: u64,
    /// segment size in 10-point bursts (0x10e0)
    pub seg: u32,
    pub max_frames: u32,
    /// pre / post trigger points (0x4050 / 0x4054)
    pub pre: u32,
    pub post: u32,
}

/// Interleave factor of a channel mask (bit0 = CH1).
pub fn ratio(mask: u32) -> u32 {
    match mask & 0xf {
        0 | 1 | 2 | 4 | 8 => 1,
        3 | 5 | 6 | 9 | 10 | 12 => 2,
        _ => 4,
    }
}

/// Maximum Auto depth by number of channels.
fn max_auto(cnt: u32) -> i128 {
    match cnt {
        0 | 1 => 125_000_000,
        2 => 62_500_000,
        _ => 31_250_000,
    }
}

/// Rates the 1.25 GHz clock cannot make, remapped.
fn modify_rate(r: i128) -> i128 {
    match r {
        2_500 => 2_000,
        200_000 => 125_000,
        1_000_000 => 500_000,
        2_000_000 => 1_250_000,
        5_000_000 => 2_500_000,
        10_000_000 => 6_250_000,
        20_000_000 | 25_000_000 => 12_500_000,
        50_000_000 => 31_250_000,
        100_000_000 | 125_000_000 => 62_500_000,
        200_000_000 | 250_000_000 => 156_250_000,
        400_000_000 | 500_000_000 => 312_500_000,
        1_000_000_000 => 625_000_000,
        x => x,
    }
}

/// The highest rate at or below `r` the SPU dividers make (`spu::sample_rate`): max / k above 62.5 MSa/s, 62.5 M / k
/// below, in the nominal domain.
fn legal_rate(r: i128, max: i128) -> i128 {
    let r = r.max(1);
    if r > 62_500_000 {
        let k = (max + r - 1) / r;
        let x = max / k.max(1);
        if x > 62_500_000 {
            return x;
        }
    }
    let k = (62_500_000 + r - 1) / r;
    62_500_000 / k.max(1)
}

/// a * b / c, rounded up to an even number when not integral.
fn cis_float(a: i128, b: i128, c: i128) -> i128 {
    if (a * b) % c == 0 {
        return a * b / c;
    }
    let x = (a * b + c - 1) / c;
    (x + 1) & !1
}

/// Power-of-two rounding of a burst count.
fn pow2(n: i128) -> i128 {
    let mut v = n - 1;
    let mut bits = 1;
    if v >= 2 {
        loop {
            bits += 1;
            let c = v > 3;
            v >>= 1;
            if !c {
                break;
            }
        }
    }
    1 << bits
}

/// FPGA memory in bursts for the segment cap. Stock reads 16,777,216 from the series parameters, which caps a segment
/// at 7,864,320 bursts = 78.6 M points: deeper records wrap inside the segment and their first (depth - 78.6 M) points
/// are overwritten (unit 2026-11-03: a phase jump at exactly that point of 100 M / 125 M records). With 0x5000000
/// bursts 125 M fits.
pub const MEM_BURSTS: i128 = 0x500_0000;

/// Segment size in 10-point bursts for `pts` raw points (normal mode).
pub fn mem_size(pts: i128) -> i128 {
    let n = ((pts + 0x180) / 10).max(32);
    pow2(n).min((MEM_BURSTS - 0x10_0000) / 2)
}

/// The sample rate `prep` asks for before the remap (roll mode's interpolation correction uses the
/// ratio requested / used, roll-mode.md 3.2).
pub fn requested_rate(mask: u32, tdiv_fs: i128, depth: i128) -> i128 {
    crate::dho::clock::from_nominal_rate(requested_rate_nominal(mask, crate::dho::clock::to_nominal_fs(tdiv_fs), depth))
}

fn requested_rate_nominal(mask: u32, tdiv_fs: i128, depth: i128) -> i128 {
    let mr = CLOCK / ratio(mask) as i128;
    if depth > 0 {
        return (depth * FS / tdiv_fs / 10).min(mr);
    }
    let pts = cis_float(10 * tdiv_fs, mr, FS);
    let amax = max_auto((mask & 0xf).count_ones());
    if pts > amax {
        amax * FS / tdiv_fs / 10
    } else {
        mr
    }
}

/// Parameters for channel `mask`, time base `tdiv_fs` per division, memory depth `depth` (0 = Auto) and trigger
/// offset `offset_fs` (0 = centred).
/// Times in and rates out are real: at an experimental ADC clock (`clock`) they are converted to and from the nominal
/// 1.25 GHz domain the model works in (points and register values do not change).
pub fn prep(mask: u32, tdiv_fs: i128, depth: i128, offset_fs: i128) -> Params {
    let mut p = prep_nominal(mask, crate::dho::clock::to_nominal_fs(tdiv_fs), depth, crate::dho::clock::to_nominal_fs(offset_fs));
    if crate::dho::clock::overclocked() {
        p.rate = crate::dho::clock::from_nominal_rate(p.rate as i128) as u64;
        p.dot_fs = crate::dho::clock::from_nominal_fs(p.dot_fs as i128) as u64;
    }
    p
}

fn prep_nominal(mask: u32, tdiv_fs: i128, depth: i128, offset_fs: i128) -> Params {
    let ratio = ratio(mask) as i128;
    let mr = CLOCK / ratio;
    let cnt = (mask & 0xf).count_ones();
    let (mut memsel, mut rate);
    if depth > 0 {
        memsel = depth;
        rate = (depth * FS / tdiv_fs / 10).min(mr);
    } else {
        let pts = cis_float(10 * tdiv_fs, mr, FS);
        let amax = max_auto(cnt);
        if pts > amax {
            memsel = amax;
            rate = amax * FS / tdiv_fs / 10;
        } else {
            memsel = pts;
            rate = mr;
        }
    }
    let m = modify_rate(rate);
    if m != rate {
        rate = m;
        if depth == 0 {
            memsel = 10 * tdiv_fs * rate / FS;
        }
    }
    // experimental ADC clock: the scaled time base gives rates off the 1-2-5 grid of `modify_rate`, and the SPU only
    // divides by whole numbers. Take the next rate the dividers can make, or the model's dt would not match the data.
    if crate::dho::clock::overclocked() {
        let s = legal_rate(rate, mr);
        if s != rate {
            rate = s;
            if depth == 0 {
                memsel = 10 * tdiv_fs * rate / FS;
            }
        }
    }
    // a few points over a very long screen: the integer rate would be 0
    let rate = rate.max(1);
    let dot = FS / rate;
    let seg = mem_size(memsel * ratio);
    // frame ring for batches: kept at the stock memory size (verified on the unit; more frames would fit)
    let max_frames = (16_777_216 / 2 - 0x10_0000) / seg;
    let memstart = offset_fs - (dot * memsel).div_euclid(2);
    let x = memsel * dot + memstart;
    let post = -((-x).div_euclid(dot));
    let pre = (memsel - post).max(0);
    Params { ratio: ratio as u32, rate: rate as u64, memsel: memsel as u32, dot_fs: dot as u64, seg: seg as u32, max_frames: max_frames as u32, pre: pre as u32, post: post as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_records_fit_one_segment() {
        // 100 M / 125 M points: 2^24 bursts = 167.8 M points, no wrap (stock cap 7,864,320 bursts = 78.6 M)
        assert_eq!(mem_size(100_000_000), 1 << 24);
        assert_eq!(mem_size(125_000_000), 1 << 24);
        assert_eq!(mem_size(62_500_000), 1 << 23);
        assert_eq!(mem_size(2_500_000), 0x40000);
    }
    const US: i128 = 1_000_000_000;

    #[test]
    fn ch1_200us_auto_matches_the_unit() {
        // observed on the unit: 2.5 Mpts at 1.25 GSa/s, memsel 0x2625a0
        let p = prep(1, 200 * US, 0, 0);
        assert_eq!((p.ratio, p.rate, p.memsel, p.seg, p.pre, p.post), (1, 1_250_000_000, 2_500_000, 0x40000, 1_250_000, 1_250_000));
    }

    #[test]
    fn ch12_1us_auto() {
        let p = prep(3, US, 0, 0);
        assert_eq!((p.ratio, p.rate, p.memsel, p.seg, p.pre, p.post), (2, 625_000_000, 6250, 0x800, 3125, 3125));
    }

    #[test]
    fn other_observed_depths() {
        // horizontal-config.md 1.3: 6.25 M at 500 us/div, 25 k at 2 us/div, 6250 per channel with 4 channels at 2 us/div
        assert_eq!(prep(1, 500 * US, 0, 0).memsel, 6_250_000);
        assert_eq!(prep(1, 2 * US, 0, 0).memsel, 25_000);
        assert_eq!(prep(0xf, 2 * US, 0, 0).memsel, 6250);
    }
}
