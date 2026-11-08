// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Roll mode (slow time bases): the FPGA streams the acquisition continuously as chunks of a 16-byte header plus 100
//! words; the host polls the fill level and reads whole chunks. See docs/protocols/roll-mode.md.
//! Pure write lists like the
//! other modules; the polling loop is in the caller.

use crate::dho::hz;
use crate::dho::scu::{self, Shadows};
use crate::dho::Write;

/// Bytes of one chunk: 8-word header + 100 data words (all channels interleaved).
pub const CHUNK_BYTES: usize = 216;
/// Data words per chunk.
pub const CHUNK_WORDS: usize = 100;

/// Highest sample rate the roll stream takes (unit 2026-11-03): above it the stream carries one sample per
/// 156.25 MHz clock (1.25 GSa/s gave 8x, 625 MSa/s 4x the computed point spacing). Stock rolls only from 50 ms/div,
/// where its Auto depth gives exactly this rate.
pub const MAX_RATE: i128 = 156_250_000;

/// `MAX_RATE` at the clock in effect (FPGA clock = ADC / 8; `clock`).
pub fn max_rate() -> i128 {
    crate::dho::clock::from_nominal_rate(MAX_RATE)
}

/// Memory depth that keeps roll at or below `MAX_RATE` at `tdiv_fs` (0 = Auto is fine: 50 ms/div and slower).
pub fn depth_for(tdiv_fs: i128) -> i128 {
    // points per screen at the stream's rate: the same in the nominal domain as in real time
    let d = MAX_RATE * 10 * crate::dho::clock::to_nominal_fs(tdiv_fs) / 1_000_000_000_000_000;
    if d >= 78_125_000 {
        0
    } else {
        d
    }
}

/// Whole chunks for a fill level `n` read from 0x11e8 (4 n / 108).
pub fn chunks(n: u32) -> usize {
    (4 * n as usize) / 108
}

/// Display-chain parameters of roll mode (with the roll correction).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub ratio: u32,
    /// compression of the interpolated stream per channel / 10
    pub k: u32,
    /// linear interpolation factor num / den (1/1 = off)
    pub f_num: u64,
    pub f_den: u64,
    /// points per channel per column x 100 columns (TX length before the /10)
    pub len: u32,
    /// time between two points of one channel in fs
    pub spacing_fs: u64,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Roll parameters for channel `mask` at `tdiv_fs` with the acquisition parameters `p` (from `hz::prep`) and the
/// rate `hz::prep` asked for before remapping (`hz::requested_rate`).
pub fn params(mask: u32, tdiv_fs: i128, depth: i128, p: &hz::Params) -> Params {
    let dot = p.dot_fs.max(1);
    let s = ((tdiv_fs / 100) as u64 / dot).max(1); // column = time/div / 100, in sample periods
    let k = (1..=s).find(|&k| s % k == 0 && s / k <= 1000).unwrap_or(s);
    let (mut num, mut den) = (1u64, 1u64);
    if 1000 * s / k < 1_000_000 {
        let req = hz::requested_rate(mask, tdiv_fs, depth) as u64;
        let used = p.rate.max(1);
        if req != used {
            num = req;
            den = used;
        }
        // more than 1000 points per column: scale down to exactly 1 000 000 / 100 columns
        if 1000 * s * num / den / k > 1_000_000 {
            num = 1000 * k;
            den = s;
        }
        if num < den {
            num = 1;
            den = 1;
        }
    }
    let g = gcd(num, den).max(1);
    let (num, den) = (num / g, den / g);
    let len = (1000 * s * num / den / k) as u32;
    let ratio = p.ratio.max(1);
    Params { ratio, k: k as u32, f_num: num, f_den: den, len, spacing_fs: 10 * ratio as u64 * k * dot * den / num }
}

/// A: the roll display chain and the SCU roll bit (after the normal acquisition configuration / trigger / auto trigger of the
/// same settings). `memsel` = points per channel.
pub fn config(sh: &mut Shadows, rp: &Params, memsel: u32) -> Vec<Write> {
    let r = rp.ratio;
    let lin_on = rp.f_num != rp.f_den;
    let intx2 = (rp.f_num / rp.f_den) > 1;
    let r1154 = (!lin_on as u32) | 2 | (intx2 as u32) << 8;
    let r115c = (u32::MAX as u64 * rp.f_den / rp.f_num) as u32;
    let mut w = vec![
        (0x1100, memsel + (0x48 | r << 9) + rp.k),
        (0x1108, 0),
        (0x1118, memsel + 40),
        (0x1130, 0x0001_0101),
        (0x1154, r1154),
        (0x1158, 0),
        (0x115c, if lin_on { r115c } else { 0xffff_ffff }),
        (0x1154, r1154),
        (0x1160, 0),
        (0x1164, 0xffff_ffff),
        (0x11fc, 100 / r),
        (0x11f0, (rp.len + 7) / 8 * 8),
        (0x1170, 0xc000_0000 | (10 * r * rp.k)),
        (0x1174, 0x4000_0001),
    ];
    sh.s1010 &= !(1 << 31); // frame header on, like the run thread
    w.push((0x1010, sh.s1010));
    sh.s4008 &= !(1 << 25);
    w.push((0x4008, sh.s4008));
    sh.s400c &= !0x4206_0000;
    w.push((0x400c, sh.s400c));
    sh.s4008 &= !0x0072_0000;
    w.push((0x4008, sh.s4008));
    sh.s4008 &= !(1 << 22);
    w.push((0x4008, sh.s4008));
    sh.s4008 |= 1 << 17;
    w.push((0x4008, sh.s4008));
    w
}

/// A for scan mode (roll-mode.md 5): the same display chain with the scan bit (0x4008 bit22) instead of the roll bit,
/// then the scan trigger position 0x4058.
pub fn config_scan(sh: &mut Shadows, rp: &Params, memsel: u32, trig_pos: u32) -> Vec<Write> {
    let mut w = config(sh, rp, memsel);
    w.pop(); // the roll bit
    sh.s4008 &= !(1 << 17);
    sh.s4008 |= 1 << 22;
    w.push((0x4008, sh.s4008));
    w.push((0x4058, trig_pos));
    w
}

/// 0x4058 of scan mode: `pos | en << 31`, pos = (delay - 5 time/div) / point period in samples. Enabled only when the
/// trigger point is left of the screen start by at least one column (time/div / 10); else 0.
/// `delay_fs` is the horizontal position of the trigger relative to the screen centre (positive = trigger left).
pub fn scan_trig_pos(delay_fs: i128, tdiv_fs: i128, dot_fs: u64) -> u32 {
    let start = delay_fs - 5 * tdiv_fs; // screen start relative to the trigger point
    let column = (tdiv_fs / 10).max(1);
    if start < column {
        return 0;
    }
    ((start / dot_fs.max(1) as i128).min(0x7fff_ffff) as u32) | 1 << 31
}

/// What the scan read loop learns after every read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanInfo {
    /// 0x1204 bit31 [A: the sweep has triggered]
    pub triggered: bool,
    /// 0x1204 [15:0] [A: position of the trigger in the sweep]
    pub pos: u16,
    /// 0x40d0 != 0: the sweep is complete (stock: return -10, new sweep in RUN, STOP in SINGLE)
    pub done: bool,
}

/// Registers read after every scan read, in this order.
pub const SCAN_INFO_REGS: [u32; 2] = [0x1204, 0x40d0];

pub fn scan_info(r1204: u32, r40d0: u32) -> ScanInfo {
    ScanInfo { triggered: r1204 & 1 << 31 != 0, pos: r1204 as u16, done: r40d0 != 0 }
}

/// After a complete sweep in RUN: the stop sequence and the average clear; the caller waits ~500 ms, writes
/// `config_scan` again and starts the next sweep with `start`.
pub fn scan_done(sh: &mut Shadows) -> Vec<Write> {
    let mut w = Vec::new();
    scu::stop_sequence(&mut w, sh);
    w.push((0x10b4, 1));
    w.push((0x10b4, 0));
    w
}

/// B: start streaming (stop sequence, channel mask, FSM run).
pub fn start(sh: &mut Shadows, mask: u32) -> Vec<Write> {
    let mut w = Vec::new();
    scu::stop_sequence(&mut w, sh);
    sh.s4008 = (sh.s4008 & !0xf) | (mask & 0xf);
    w.push((0x4008, sh.s4008));
    sh.s4004 |= 1;
    w.push((0x4004, sh.s4004));
    w
}

/// C: before reading the fill level 0x11e8: the pulse of 0x1010 bit 28.
pub fn fill_pulse(sh: &Shadows) -> [Write; 3] {
    [(0x1010, sh.s1010 & !(1 << 28)), (0x1010, sh.s1010 | 1 << 28), (0x1010, sh.s1010 & !(1 << 28))]
}

/// D: stop streaming and leave roll mode (stop sequence, average clear, roll/scan bits off, 0x11fc back to 1000).
pub fn stop(sh: &mut Shadows) -> Vec<Write> {
    let mut w = Vec::new();
    scu::stop_sequence(&mut w, sh);
    w.push((0x10b4, 1));
    w.push((0x10b4, 0));
    sh.s4008 &= !(1 << 17);
    w.push((0x4008, sh.s4008));
    sh.s4008 &= !(1 << 22);
    w.push((0x4008, sh.s4008));
    w.push((0x11fc, 1000));
    w
}

/// LA compression 0x1298 of roll and scan:
/// (10 x ratio x k_la) >> s, k_la = k (2 k at ratio 4), s = 1 for sample mode m 1 / 2, 2 for m 4.
pub fn la_compress(rp: &Params, m: u32) -> u32 {
    let k_la = if rp.ratio == 4 { 2 * rp.k } else { rp.k };
    (10 * rp.ratio * k_la) >> if m >= 4 { 2 } else { 1 }
}

/// Column of an analog chunk in a scan sweep from its counter (from 1, every sweep). With the LA on, LA and analog chunks
/// alternate and share the counter (seen on the unit: LA 1, analog 2, LA 3, ...), so analog chunk n has the counter 2n.
pub fn scan_column(counter: u16, la: bool) -> usize {
    let c = counter.max(1) as usize;
    if la { (c / 2).max(1) - 1 } else { c - 1 }
}

/// Time between two LA words of the stream (fs). An LA chunk of 100 words spans the time of an analog chunk of
/// 100 / ratio points per channel, so a word is a point spacing over the ratio. Measured on the unit with one, three
/// and four channels at 100 and 500 ms/div: compression over the rate of la.md 3.1 gave 1.6 times too much with one
/// channel.
pub fn la_spacing_fs(rp: &Params) -> u64 {
    rp.spacing_fs / rp.ratio.max(1) as u64
}

/// The LA part of the roll display chain, after `config` / `config_scan`: 0x12cc = ratio x (100 / ratio),
/// 0x12c0 = the length of 0x11f0, LA compression 0x1298 / 0x129c, LA interpolation with the analog
/// fraction (0x1280 bit 2 clear = on; not doubled at ratio 1 in live roll, la.md 13), display delay 0
/// as in `la::passthrough`. The other bits of 0x1280 / 0x1290 / 0x1294 are assumed 0 [A].
pub fn la_config(rp: &Params, t: &crate::dho::la::Timing) -> Vec<Write> {
    let r = rp.ratio;
    let lin_on = rp.f_num != rp.f_den;
    vec![
        (0x12cc, r * (100 / r)),
        (0x12c0, (rp.len + 7) / 8 * 8),
        (0x1298, 0xc000_0000 | (la_compress(rp, t.m) & 0x0fff_ffff)),
        (0x129c, 0x4000_0001),
        (0x1280, if lin_on { 0x08 } else { 0x0c }),
        (0x12b0, 0),
        (0x12b4, if lin_on { (u32::MAX as u64 * rp.f_den / rp.f_num) as u32 } else { 0xffff_ffff }),
        (0x12b8, 0),
        (0x12bc, 0xffff_ffff),
        (0x1290, 0),
        (0x1294, 0),
    ]
}

/// Chunks to read with the LA on: an even count, analog and LA chunks alternate.
pub fn la_chunks(c: usize) -> usize {
    c & !1
}

/// A chunk of the LA stream: header word 1 bit 1.
pub fn is_la_chunk(chunk: &[u8]) -> bool {
    chunk.len() >= 4 && u16::from_le_bytes([chunk[2], chunk[3]]) & 2 != 0
}

/// Splits a read with the LA on: analog points per channel as `split`, and the LA words in order (100 per LA chunk).
pub fn split_la(bytes: &[u8], mask: u32) -> ([Vec<u16>; 4], Vec<u16>) {
    let mut analog = Vec::with_capacity(bytes.len());
    let mut la = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(CHUNK_BYTES) {
        if is_la_chunk(chunk) {
            la.extend(chunk[16..].chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]])));
        } else {
            analog.extend_from_slice(chunk);
        }
    }
    (split(&analog, mask), la)
}

/// Chunk counter in header word 4 (counts 1, 2, 3, ... from the start of the stream; in scan mode from the start of
/// each sweep, so `counter - 1` is the column of the sweep: unit 2026-11-03).
pub fn counter(chunk: &[u8]) -> u16 {
    u16::from_le_bytes([chunk[8], chunk[9]])
}

/// Splits a read of whole chunks into per-channel point lists: the header skipped, the words of each chunk in the
/// layout of a read-out (`seq::deinterleave`; 3 channels run as 4, with an unused slot). Without a channel on there are
/// no analog words to split.
pub fn split(bytes: &[u8], mask: u32) -> [Vec<u16>; 4] {
    let mut out: [Vec<u16>; 4] = Default::default();
    if mask & 0xf == 0 {
        return out;
    }
    for chunk in bytes.chunks_exact(CHUNK_BYTES) {
        for (o, c) in out.iter_mut().zip(crate::dho::seq::deinterleave(&chunk[16..], mask)) {
            o.extend(c);
        }
    }
    out
}

/// Points of one channel in a chunk (the column height of a scan sweep).
pub fn points_per_chunk(mask: u32) -> usize {
    CHUNK_WORDS / hz::ratio(mask) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_columns_count_analog_chunks_only() {
        assert_eq!((scan_column(1, false), scan_column(2, false), scan_column(9, false)), (0, 1, 8));
        // the unit with the LA on: LA 1, analog 2, LA 3, analog 4
        assert_eq!((scan_column(2, true), scan_column(4, true), scan_column(10, true)), (0, 1, 4));
    }

    fn p(mask: u32, tdiv_s: f64) -> (Params, hz::Params) {
        let t = (tdiv_s * 1e15) as i128;
        let hp = hz::prep(mask, t, 0, 0);
        (params(mask, t, 0, &hp), hp)
    }

    #[test]
    fn ch1_100ms_matches_the_static_table() {
        // roll-mode.md 3.4: 62.5 MSa/s, memsel 62.5 M, k 100, f 8/5, 0x1170 0xc00003e8, 0x11fc 100, 0x11f0 0xf4240,
        // 0x1100 0x3b9af4c, 0x115c 0x9fffffff, 10 us per point, 1000 chunks/s
        let (rp, hp) = p(1, 0.1);
        assert_eq!((hp.rate, hp.memsel), (62_500_000, 62_500_000));
        assert_eq!((rp.k, rp.f_num, rp.f_den, rp.len), (100, 8, 5, 1_000_000));
        assert_eq!(rp.spacing_fs, 10_000_000_000);
        let w = config(&mut Shadows { ..Shadows::HANDOVER }, &rp, hp.memsel);
        let get = |r: u32| w.iter().find(|x| x.0 == r).unwrap().1;
        assert_eq!((get(0x1170), get(0x11fc), get(0x11f0), get(0x1100), get(0x115c)), (0xc000_03e8, 100, 0xf4240, 0x3b9_af4c, 0x9fff_ffff));
        assert_eq!(get(0x1154), 2);
        assert_eq!(w.last().unwrap().1 & (1 << 17), 1 << 17);
    }

    #[test]
    fn two_channels_and_slow_bases() {
        let (rp, _) = p(3, 0.1);
        assert_eq!((rp.k, rp.f_num, rp.f_den, rp.len), (100, 1, 1, 625_000));
        let w = config(&mut Shadows { ..Shadows::HANDOVER }, &rp, 62_500_000);
        assert!(w.contains(&(0x1170, 0xc000_07d0)) && w.contains(&(0x11fc, 50)) && w.contains(&(0x1154, 3)));
        // 1 s/div, CH1: 12.5 MSa/s, 125 M points, k 125, f 1, 10 ms per chunk
        let (rp, hp) = p(1, 1.0);
        assert_eq!((hp.rate, rp.k, rp.f_num, rp.len), (12_500_000, 125, 1, 1_000_000));
        assert_eq!(rp.spacing_fs * 100, 10_000_000_000_000); // 100 points = one column = 10 ms
    }

    #[test]
    fn scan_bits_and_trigger_position() {
        let (rp, hp) = p(1, 0.2);
        let mut sh = Shadows { ..Shadows::HANDOVER };
        let w = config_scan(&mut sh, &rp, hp.memsel, 0);
        let last4008 = w.iter().rev().find(|x| x.0 == 0x4008).unwrap().1;
        assert_eq!((last4008 >> 17 & 1, last4008 >> 22 & 1), (0, 1));
        assert_eq!(*w.last().unwrap(), (0x4058, 0));
        // the roll write list without its roll bit is the scan list without 0x4058 and the scan bit
        let r = config(&mut Shadows { ..Shadows::HANDOVER }, &rp, hp.memsel);
        assert_eq!(&r[..r.len() - 1], &w[..w.len() - 2]);
        // trigger at the centre or right of the screen start: disabled
        let t = 200_000_000_000_000i128; // 200 ms in fs
        assert_eq!(scan_trig_pos(0, t, hp.dot_fs), 0);
        assert_eq!(scan_trig_pos(5 * t, t, hp.dot_fs), 0);
        // trigger one division left of the screen start: 200 ms of samples, enabled
        let v = scan_trig_pos(6 * t, t, hp.dot_fs);
        assert_eq!(v >> 31, 1);
        assert_eq!((v & 0x7fff_ffff) as i128, t / hp.dot_fs as i128);
        // leaving scan clears both bits; the sweep restart only stops and clears the average
        let s = stop(&mut sh);
        assert_eq!(sh.s4008 & (1 << 17 | 1 << 22), 0);
        assert!(s.contains(&(0x11fc, 1000)));
        let d = scan_done(&mut sh);
        assert!(d.ends_with(&[(0x10b4, 1), (0x10b4, 0)]));
        assert_eq!(scan_info(0x8000_0123, 0), ScanInfo { triggered: true, pos: 0x123, done: false });
        assert!(scan_info(0, 1).done);
    }

    #[test]
    fn fast_roll_stays_at_the_stream_rate() {
        // 5 ms/div: 7.8125 M points at 156.25 MSa/s, k 9 -> 0.576 us per point (unit: 1736.11 points per 1 kHz period)
        let t = 5_000_000_000_000i128;
        assert_eq!(depth_for(t), 7_812_500);
        let hp = hz::prep(1, t, depth_for(t), 0);
        assert_eq!(hp.rate as i128, MAX_RATE);
        let rp = params(1, t, depth_for(t), &hp);
        assert_eq!((rp.k, rp.spacing_fs), (9, 576_000_000));
        // 20 ms/div: 3.2 us; 50 ms/div and slower keep Auto
        let t = 20_000_000_000_000i128;
        assert_eq!(params(1, t, depth_for(t), &hz::prep(1, t, depth_for(t), 0)).spacing_fs, 3_200_000_000);
        assert_eq!(depth_for(50_000_000_000_000), 0);
    }

    #[test]
    fn chunk_split() {
        assert_eq!(chunks(243), 9);
        let mut b = Vec::new();
        for c in 0..2u16 {
            b.extend([0u8; 16]);
            for i in 0..100u16 {
                b.extend((c * 1000 + i).to_le_bytes());
            }
        }
        let s = split(&b, 0b11);
        assert_eq!((s[0].len(), s[1].len()), (100, 100));
        assert_eq!((s[0][0], s[1][0], s[0][50], s[1][99]), (0, 1, 1000, 1099));
        let mut h = [0u8; CHUNK_BYTES];
        h[..16].copy_from_slice(&[0x05, 0xfa, 0x84, 0, 0x7b, 0, 0, 0, 0xf5, 0x01, 0x6b, 0x04, 0x0b, 0x40, 0x82, 0x6e]);
        assert_eq!(counter(&h), 501);
    }

    #[test]
    fn three_channels_use_the_four_channel_slots() {
        let mut b = vec![0u8; 16];
        for i in 0..100u16 {
            b.extend(i.to_le_bytes());
        }
        // CH1, CH2, CH4: word 4i + k is CH k+1 whichever channels are on; slot 2 belongs to the channel that is off
        let s = split(&b, 0b1011);
        assert_eq!((s[0].len(), s[1].len(), s[2].len(), s[3].len()), (25, 25, 0, 25));
        assert_eq!((s[0][1], s[1][1], s[3][1]), (4, 5, 7));
        assert_eq!((points_per_chunk(0b1011), points_per_chunk(0b1111), points_per_chunk(0b0101), points_per_chunk(0b0100)), (25, 25, 50, 100));
        assert!(split(&b, 0).iter().all(|c| c.is_empty()));
    }
}

#[cfg(test)]
mod la_tests {
    use super::*;
    use crate::dho::la::Timing;

    fn rp(ratio: u32, k: u32) -> Params {
        Params { ratio, k, f_num: 1, f_den: 1, len: 1000, spacing_fs: 0 }
    }

    /// la.md 4.4: (10 x ratio x k_la) >> s.
    #[test]
    fn compression() {
        assert_eq!(la_compress(&rp(1, 8), 1), 40);
        assert_eq!(la_compress(&rp(2, 8), 2), 80);
        // ratio 4: k_la = 2 k, s = 2
        assert_eq!(la_compress(&rp(4, 8), 4), 160);
    }

    /// An LA chunk (100 words) spans the time of an analog chunk (100 / ratio points).
    #[test]
    fn la_and_analog_chunks_span_the_same_time() {
        for ratio in [1u32, 2, 4] {
            let r = Params { spacing_fs: 40_000_000_000, ..rp(ratio, 6) };
            assert_eq!(100 * la_spacing_fs(&r), 100 / ratio as u64 * r.spacing_fs, "ratio {}", ratio);
        }
    }

    #[test]
    fn config_values() {
        let w = la_config(&rp(4, 8), &Timing { m: 4, rate: 1, n: 0, pre: 0 });
        assert!(w.contains(&(0x12cc, 100)) && w.contains(&(0x12c0, 1000)) && w.contains(&(0x1298, 0xc000_00a0)));
        assert!(w.contains(&(0x1280, 0x0c)) && w.contains(&(0x12b4, 0xffff_ffff)));
        let w = la_config(&Params { f_num: 2, f_den: 1, ratio: 3, ..rp(1, 1) }, &Timing { m: 1, rate: 1, n: 0, pre: 0 });
        assert!(w.contains(&(0x12cc, 99)), "ratio x (n / ratio)");
        assert!(w.contains(&(0x1280, 0x08)) && w.contains(&(0x12b4, u32::MAX / 2)));
    }

    #[test]
    fn stream_is_separated_by_the_header_bit() {
        let chunk = |la: bool, v: u16| {
            let mut c = vec![0u8; CHUNK_BYTES];
            c[2] = if la { 2 } else { 0 };
            for w in c[16..].chunks_exact_mut(2) {
                w.copy_from_slice(&v.to_le_bytes());
            }
            c
        };
        let bytes = [chunk(false, 1), chunk(true, 0xabcd), chunk(false, 2), chunk(true, 0x1234)].concat();
        let (a, l) = split_la(&bytes, 0b11);
        assert_eq!((a[0].len(), a[1].len(), l.len()), (100, 100, 200));
        assert_eq!((a[0][0], a[0][99], l[0], l[199]), (1, 2, 0xabcd, 0x1234));
        assert_eq!((la_chunks(7), la_chunks(8)), (6, 8));
    }
}
