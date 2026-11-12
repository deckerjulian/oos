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

/// Frame memory in bursts of the segmented record (stock series parameter, `DevAcquireSpu_getMaxFrm` mode 1): the whole
/// frame memory less the reserved top 1 M bursts. Kept at the stock size until the real size is measured (spec 03, 6.5).
pub const RECORD_BURSTS: u32 = 16_777_216 - 0x10_0000;

/// Most frames a segmented record of segments of `seg` bursts holds (spec 03, 1.5): the whole frame memory, unlike the
/// live batch ring (`Params::max_frames`, half of it).
pub fn max_record_frames(seg: u32) -> u32 {
    RECORD_BURSTS / seg.max(1)
}

/// Segment size in 10-point bursts for `pts` raw points (normal mode).
pub fn mem_size(pts: i128) -> i128 {
    let n = ((pts + 0x180) / 10).max(32);
    pow2(n).min((MEM_BURSTS - 0x10_0000) / 2)
}

/// Bursts at the top of the stock frame memory the FPGA uses for averaging (series parameter "average base",
/// docs/fork/specs/01-hw-averaging.md 1.3). The live ring (the lower half) stays below it.
pub const AVG_BASE_BURSTS: u32 = 5_242_880;

/// Largest segment in average mode: the stock cap. The deep-record cap of `mem_size` does not apply, the average
/// region sits inside the stock memory size.
pub const AVG_SEG_CAP: i128 = 7_864_320;

/// Segment size in 10-point bursts for `pts` raw points (points x ratio) in average mode (spec 01, 1.3): twice the
/// bursts of a normal segment plus two 32-burst blocks of margin, rounded to a power of two below 25 M points and from
/// there to the next multiple of 2048 bursts above (an exact multiple gets one more block, as in the stock rounding).
pub fn mem_size_avg(pts: i128) -> i128 {
    let n = (((pts + 383) / 320 + 2) * 64).max(32);
    let n = if pts < 25_000_000 { pow2(n) } else { (n / 2048 + 1) * 2048 };
    n.min(AVG_SEG_CAP)
}

/// Frames of a live batch in average mode: the lower half of the stock memory less 1 M bursts, in segments of `seg`
/// bursts (the same ring as `Params::max_frames`, with the average segment). 0: the record does not fit.
pub fn max_frames_avg(seg: u32) -> u32 {
    (16_777_216 / 2 - 0x10_0000) / seg.max(1)
}

/// `p` with the average memory layout: the segment of `mem_size_avg` and its frame ring.
pub fn avg_params(p: Params) -> Params {
    let seg = mem_size_avg(p.memsel as i128 * p.ratio as i128) as u32;
    Params { seg, max_frames: max_frames_avg(seg), ..p }
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

    // Break it catches: a segment limit from the half ring of the live batches, or from the deep-record memory size.
    #[test]
    fn record_frames_use_the_whole_stock_memory() {
        // spec 03, 1.5: 1,000 points -> 256 bursts -> 61,440 frames; 12,500 -> 2,048 -> 7,680
        let at = |pts: i128, mask: u32| {
            let p = prep(mask, 1_000_000_000_000_000, pts, 0);
            (p.seg, max_record_frames(p.seg))
        };
        assert_eq!(at(1_000, 1), (256, 61_440));
        assert_eq!(at(12_500, 1), (2_048, 7_680));
        assert_eq!(at(100_000, 1), (16_384, 960));
        assert_eq!(at(1_000, 3), (256, 61_440));
        assert_eq!(at(12_500, 3), (4_096, 3_840));
        assert_eq!(at(100_000, 3), (32_768, 480));
        assert_eq!(at(1_000, 0xf), (512, 30_720));
        assert_eq!(at(12_500, 0xf), (8_192, 1_920));
        assert_eq!(at(100_000, 0xf), (65_536, 240));
        // twice the live ring plus the reserved region
        let p = prep(1, 1_000_000_000_000_000, 12_500, 0);
        assert!(max_record_frames(p.seg) > 2 * p.max_frames);
        assert_eq!(max_record_frames(0), RECORD_BURSTS);
    }

    // Break it catches: the normal layout in average mode, a missing 32-burst floor or cap, rounding above 25 M points,
    // a frame ring computed from the normal segment. Expected values worked out by hand from spec 01, 1.3.
    #[test]
    fn average_segment_layout() {
        // 1,000 points: (1383 / 320 + 2) x 64 = 384 -> 512 (normal 256)
        assert_eq!((mem_size_avg(1_000), mem_size(1_000)), (512, 256));
        // 0 points: (383 / 320 + 2) x 64 = 192 -> 256; the floor of 32 never binds with the margin of 128
        assert_eq!(mem_size_avg(0), 256);
        // 12,500 x ratio 1, 2, 4: (12883 / 320 + 2) x 64 = 2688 -> 4096; 25,383 / 320 = 79 -> 5184 -> 8192; 50,383 / 320 =
        // 157 -> 10176 -> 16384
        assert_eq!([1, 2, 4].map(|r| mem_size_avg(12_500 * r)), [4_096, 8_192, 16_384]);
        // 1,000,000: (1000383 / 320 + 2) x 64 = 3127 x 64 = 200,128 -> 262,144
        assert_eq!(mem_size_avg(1_000_000), 262_144);
        // 4 M x 4: 16,000,383 / 320 = 50,001 -> 3,200,192 -> 4,194,304
        assert_eq!(mem_size_avg(16_000_000), 4_194_304);
        // 25 M points: 78,126 + 2 = 78,128 x 64 = 5,000,192, up to 2,442 x 2048 = 5,001,216; 125 M: capped
        assert_eq!(mem_size_avg(25_000_000), 5_001_216);
        // 25,015,297 points: 78,174 + 2 = 78,176 x 64 = 5,003,264 = 2,443 x 2048 exactly -> one block more, 5,005,312
        assert_eq!(mem_size_avg(25_015_297), 5_005_312);
        assert_eq!(mem_size_avg(125_000_000), AVG_SEG_CAP);
        // frame ring: 7,340,032 bursts
        assert_eq!((max_frames_avg(512), max_frames_avg(262_144), max_frames_avg(AVG_SEG_CAP as u32)), (14_336, 28, 0));
        let p = avg_params(prep(1, 1_000_000_000_000_000, 1_000, 0));
        assert_eq!((p.seg, p.max_frames, p.memsel), (512, 14_336, 1_000));
        // idempotent, everything else as in normal mode
        assert_eq!(avg_params(p), p);
        let q = prep(0xf, 1_000_000_000_000_000, 12_500, 0);
        assert_eq!(avg_params(q), Params { seg: 16_384, max_frames: 448, ..q });
    }

    // The Rust layout against `tools/hzmodel.py avg` (hal/testdata/avg-memsize.txt): both from spec 01, 1.3.
    #[test]
    fn average_layout_matches_the_python_model() {
        let v = include_str!("../../testdata/avg-memsize.txt");
        let mut n = 0;
        for l in v.lines() {
            let f: Vec<i128> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
            let seg = mem_size_avg(f[0] * f[1]);
            assert_eq!((seg, max_frames_avg(seg as u32) as i128), (f[2], f[3]), "{}", l);
            n += 1;
        }
        assert_eq!(n, 36);
    }

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
