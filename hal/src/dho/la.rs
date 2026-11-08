// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Logic analyser D0..D15 (PLA2216 probe, docs/protocols/la.md): threshold DACs (FPGA 0x12d4, one 16-bit code per
//! pod), live line states (0x12dc), probe detection (GPIO `LA_PROBE_DETECT`, 1 = present) and the factory zero codes
//! of `cal_la.hex` (read only; the probe calibration writes /rigol/data and is never run by OOS), and the acquisition
//! path of la.md section 8 (0x4008 bit 26, LA rate block, LA doubling, the second raw read-out). la.md says what was
//! seen on the unit.

use crate::dho::{hz, Write};

/// FPGA register of both threshold codes: [15:0] D0-D7, [31:16] D8-D15.
pub const THRESHOLD: u32 = 0x12d4;
/// Live line states: bit n = Dn.
pub const LINES: u32 = 0x12dc;
/// DAC codes per volt.
pub const CODES_PER_V: f64 = 310.07;

/// The two pod zero codes (DAC code for 0 V).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cal {
    pub zero: [u32; 2],
}

impl Default for Cal {
    /// The unit's factory values (cal_la.hex 2013-01-18).
    fn default() -> Cal {
        Cal { zero: [8609, 8580] }
    }
}

impl Cal {
    pub fn from_file_bytes(d: &[u8]) -> Option<Cal> {
        let p = crate::dho::calfile::parse(d).ok()?;
        let z = |o: usize| Some(u32::from_le_bytes(p.get(o..o + 4)?.try_into().ok()?));
        Some(Cal { zero: [z(0)?, z(4)?] })
    }
    /// cal_la.hex from `dir`, else `dir/default`.
    pub fn load(dir: &str) -> Option<Cal> {
        let def = format!("{}/default", dir.trim_end_matches('/'));
        let (b, _) = crate::dho::calfile::load_first("cal_la.hex", &[dir, def.as_str()])?;
        Cal::from_file_bytes(&b)
    }
}

/// Threshold code (la.md 5.1, bit-exact): trunc(zero + f32(f32(V) * 1e-9) * 310.07) (the sum in double, then
/// truncated), V in nV clamped to +-15 V.
pub fn code(v_nv: f64, zero: u32) -> u32 {
    let v = v_nv.clamp(-15e9, 15e9) as f32;
    let volts = v * 1e-9f32;
    (zero as f64 + volts as f64 * CODES_PER_V) as u32
}

/// The threshold write for pod 1 (D0-D7) and pod 2 (D8-D15), volts.
pub fn thresholds(cal: &Cal, pod1_v: f64, pod2_v: f64) -> Write {
    (THRESHOLD, code(pod2_v * 1e9, cal.zero[1]) << 16 | code(pod1_v * 1e9, cal.zero[0]))
}

/// Line states from a 0x12dc read: bit n = Dn high.
pub fn lines(r: u32) -> [bool; 16] {
    std::array::from_fn(|n| r >> n & 1 == 1)
}

/// Probe plugged in (GPIO `LA_PROBE_DETECT` reads 1).
#[cfg(target_os = "linux")]
pub fn probe_present() -> std::io::Result<bool> {
    Ok(crate::dho::gpio::Line::input("LA_PROBE_DETECT")?.get())
}

// ---------------------------------------------------------------------------------------------- acquisition
//
// la.md sections 3 and 4.2, NOT run on the unit yet (no PLA2216 test so far).
// Everything here is only used while the LA is switched on (`seq::Settings::la`); with the LA off the analog path
// writes exactly what it wrote before (the acquisition configuration clears 0x4008 bit 26 itself).

/// LA part of the acquisition settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Set {
    /// threshold per pod in volts: D0-D7, D8-D15
    pub thr: [f64; 2],
    /// at least one analog channel is displayed: the analog rate is halved then (la.md 3.1 "rate")
    pub analog_shown: bool,
    /// the pod zero codes (cal_la.hex)
    pub cal: Cal,
    /// roll or scan: the LA words come with the stream (la.md 4.4)
    pub roll: bool,
}

impl Default for Set {
    /// TTL thresholds (1.4 V, the stock `CLa` default), an analog channel shown, the unit's factory zero codes.
    fn default() -> Set {
        Set { thr: [1.4, 1.4], analog_shown: true, cal: Cal::default(), roll: false }
    }
}

/// Sample mode from the channels (la.md 3.1): m = 1 for one channel, 2 for any two, 4 for more. The stock
/// table has 4 for the pairs 3 and 12 too, but on the unit CH1+CH2 at m = 4 gave LA words of half the real spacing with seams.
pub fn sample_mode(mask: u32) -> u32 {
    match mask & 0xf {
        0 | 1 | 2 | 4 | 8 => 1,
        3 | 5 | 6 | 9 | 10 | 12 => 2,
        _ => 4,
    }
}

/// LA timing of an acquisition (la.md 3.1, 3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// sample mode (1/2/4)
    pub m: u32,
    /// LA sample rate in Sa/s (real, like `hz::Params::rate`)
    pub rate: u64,
    /// LA samples in the record: memsel x m / 2
    pub n: u32,
    /// LA index of the trigger point: pre x m / 2 [A until test T1]
    pub pre: u32,
}

/// LA rate, record length and trigger index for the (already LA-adjusted, `prep`) analog parameters: r_la = rate x
/// m / 2, so the LA record spans the same time as the analog one.
pub fn timing(p: &hz::Params, mask: u32) -> Timing {
    let m = sample_mode(mask);
    Timing { m, rate: p.rate * m as u64 / 2, n: p.memsel * m / 2, pre: p.pre * m / 2 }
}

/// The analog parameters with the LA on (la.md 3.1): with an analog channel
/// shown the rate is halved (156.25 MSa/s becomes 62.5, not 78.125), memsel is NOT recomputed (the record then spans
/// two screens), the segment is doubled and the frame ring follows the doubled segment. Pre/post stay in analog points
/// here; `config` doubles them in the registers.
pub fn prep(mut p: hz::Params, analog_shown: bool) -> hz::Params {
    if analog_shown {
        // the rule is written for the nominal 1.25 GHz rates (`clock`: identity at the stock clock)
        let nom = crate::dho::clock::to_nominal_rate(p.rate as i128);
        let half = if nom == 156_250_000 { 62_500_000 } else { nom / 2 };
        p.rate = (crate::dho::clock::from_nominal_rate(half) as u64).max(1);
        p.dot_fs = crate::dho::clock::from_nominal_fs(hz::FS / half.max(1)) as u64;
    }
    p.seg *= 2;
    // `hz::prep`'s frame ring formula on the doubled segment [A: frame count from the doubled size]
    p.max_frames = ((16_777_216 / 2 - 0x10_0000) / p.seg.max(1)).max(1);
    p
}

/// LA channel delay 0x128c: 424 for m = 1, else 145.
pub fn deskew(m: u32) -> u32 {
    if m == 1 {
        424
    } else {
        145
    }
}

/// LA sample-rate block 0x12a4 / 0x12a8 / 0x12ac (la.md 3.2) for `rate` in Sa/s
/// (real; the encoding works on the nominal rate).
pub fn rate_block(rate: u64) -> [Write; 3] {
    let r = (crate::dho::clock::to_nominal_rate(rate as i128) as u64).max(1);
    let (a4, ac) = if r > 62_500_000 {
        let d = 625_000_000 / r;
        // la.md 3.2: d = 5 (125 MSa/s) writes 0x8000
        (if d == 5 { 0x8000 } else { 0x8000 | (d as u32 & 0xff) }, 1)
    } else {
        (0, (125_000_000 / r) as u32)
    };
    [(0x12a4, a4), (0x12a8, 0), (0x12ac, ac)]
}

/// Wave-early word: x = 0 / 3 / 6 for ratio 1 / 2 / 4 with the
/// LA on, 4 with it off (`None`). 0x131c = 0x2580000 | x << 4 | 4 (bits [15:12] stay 0).
pub fn wave_early(la_ratio: Option<u32>) -> Write {
    let x = match la_ratio {
        Some(1) => 0,
        Some(2) => 3,
        Some(_) => 6,
        None => 4,
    };
    (0x131c, 0x0258_0000 | x << 4 | 4)
}

/// The LA additions to the acquisition configuration (la.md 3.1), written after `acq::configure_la` (which sets the
/// LA enable and the deskew 0x128c before its pre-processing reset) with the parameters from `prep`: 0x4008 bit 23 (analog stream), bit 24
/// (LA stream: only in roll/scan), 0x131c, the LA rate block, and the "LA doubling" of 0x10e4 (not in roll), 0x4050 / 0x4054 and 0x10e0 (segment, already
/// doubled by `prep`).
pub fn config(sh: &mut crate::dho::scu::Shadows, p: &hz::Params, mask: u32, roll: bool) -> Vec<Write> {
    let t = timing(p, mask);
    let mut w = Vec::new();
    sh.s4008 |= 1 << 23;
    w.push((0x4008, sh.s4008));
    sh.s4008 = (sh.s4008 & !(1 << 24)) | (roll as u32) << 24;
    w.push((0x4008, sh.s4008));
    w.push(wave_early(Some(p.ratio)));
    w.extend(rate_block(t.rate));
    w.push((0x10e4, if roll { p.memsel } else { 2 * p.memsel }));
    w.push((0x4050, 2 * p.pre));
    w.push((0x4054, 2 * p.post));
    w.push((0x10e0, p.seg));
    w
}

/// Pass-through LA play chain before a raw LA read (la.md 4.2 step 1):
/// compression off with k = 1 (0x1298 / 0x129c, bits 28-29 assumed 0), interpolation off for main and zoom
/// (0x1280 bits 2 and 3; the other bits of that byte are assumed 0 [A], no OOS shadow), step registers, display delay
/// 0 (upper halves of 0x1290 / 0x1294 assumed 0 [A]).
pub fn passthrough() -> Vec<Write> {
    vec![
        (0x1298, 0x4000_0001),
        (0x129c, 0x4000_0001),
        (0x1280, 0x0c),
        (0x12b0, 0),
        (0x12b4, 0xffff_ffff),
        (0x12b8, 0),
        (0x12bc, 0xffff_ffff),
        (0x1290, 0),
        (0x1294, 0),
    ]
}

/// Raw LA window read (la.md 4.2 steps 2-5, replayed word-exact in Phase 0.5 step B after a stock acquisition): TX
/// engine to the LA stream, window 0x1100 = 10 x N and 0x1108 = start, LA TX length 0x12c0 =
/// 4 x roundup4(N) / ratio, no frame header, record sequence off, stop sequence, play-last pulse, FSM run. Then ONE
/// read of `read_bytes(n)`. The unit of `start` is open (test T1); OOS reads from 0 unless the record is longer than
/// `READ_MAX`. Follow with `restore`.
pub fn readout(sh: &mut crate::dho::scu::Shadows, n: u32, start: u32, ratio: u32, fsm: u32) -> Vec<Write> {
    let r4 = n.div_ceil(4) * 4;
    let mut w = Vec::new();
    sh.s4008 &= !(1 << 23);
    w.push((0x4008, sh.s4008));
    sh.s4008 |= 1 << 24;
    w.push((0x4008, sh.s4008));
    w.push((0x1100, 10 * n));
    w.push((0x1108, start));
    w.push((0x12c0, 4 * r4 / ratio.max(1)));
    sh.s1010 |= 1 << 31;
    w.push((0x1010, sh.s1010));
    sh.s400c &= !(1 << 25);
    w.push((0x400c, sh.s400c));
    crate::dho::scu::stop_sequence(&mut w, sh);
    sh.s4004 &= !4;
    w.push((0x4004, sh.s4004));
    sh.s4004 |= 4;
    w.push((0x4004, sh.s4004));
    sh.s4008 = (sh.s4008 & !0xf) | fsm;
    w.push((0x4008, sh.s4008));
    sh.s4004 |= 1;
    w.push((0x4004, sh.s4004));
    w
}

/// Bytes of the raw LA read of `n` words: 2 x roundup4(n) (la.md 4.2 step 6).
pub fn read_bytes(n: u32) -> usize {
    2 * n.div_ceil(4) as usize * 4
}

/// After the LA read: analog stream back on, LA stream off.
pub fn restore(sh: &mut crate::dho::scu::Shadows) -> Vec<Write> {
    sh.s4008 |= 1 << 23;
    let a = (0x4008, sh.s4008);
    sh.s4008 &= !(1 << 24);
    vec![a, (0x4008, sh.s4008)]
}

/// Longest LA read of one acquisition (words), above the deepest LA record (31.25 M points). The stock system stops at
/// 1 M, the hardware does not: 12.5 M words read on the unit without a gap.
pub const READ_MAX: u32 = 40_000_000;

/// (words, first word) of the LA read for an LA record of `t.n` words: the record from word 0.
pub fn window(t: &Timing) -> (u32, u32) {
    (t.n.min(READ_MAX), 0)
}

/// LA words of a raw read (little-endian, bit n = Dn), the read padding dropped.
pub fn words(bytes: &[u8], n: usize) -> Vec<u16> {
    bytes.chunks_exact(2).take(n).map(|b| u16::from_le_bytes([b[0], b[1]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_analysis() {
        // la.md 5.1: TTL 1.4 V -> 9043 / 9014, 0 V -> the zero codes, +-15 V -> 13260 / 3957 for D0-7
        let c = Cal::default();
        assert_eq!(thresholds(&c, 1.4, 1.4), (0x12d4, 0x2336_2353));
        assert_eq!(thresholds(&c, 0.0, 0.0).1, 0x2184_21a1);
        assert_eq!((code(15e9, 8609), code(-15e9, 8609), code(99e9, 8609)), (13260, 3957, 13260));
        assert_eq!(code(2.5e9, 8580), 9355);
        assert_eq!(lines(0x8001)[0] && lines(0x8001)[15] && !lines(0x8001)[1], true);
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal-full");
        if let Some(f) = Cal::load(dir) {
            assert_eq!(f, Cal::default());
        }
    }

    #[test]
    fn rate_block_matches_section_3_2() {
        // la.md 3.2 examples: 625 M -> 0x8001/1, 312.5 M -> 0x8002/1, 62.5 M -> 0/2, 250 k -> 0/500; 125 M -> 0x8000
        assert_eq!(rate_block(625_000_000), [(0x12a4, 0x8001), (0x12a8, 0), (0x12ac, 1)]);
        assert_eq!(rate_block(312_500_000)[0], (0x12a4, 0x8002));
        assert_eq!(rate_block(62_500_000), [(0x12a4, 0), (0x12a8, 0), (0x12ac, 2)]);
        assert_eq!(rate_block(250_000)[2], (0x12ac, 500));
        assert_eq!(rate_block(125_000_000)[0], (0x12a4, 0x8000));
    }

    #[test]
    fn sample_mode_and_timing() {
        // la.md 3.1: by the number of channels
        let m: Vec<u32> = (0..16).map(sample_mode).collect();
        assert_eq!(m, [1, 1, 1, 2, 1, 2, 2, 4, 1, 2, 2, 4, 2, 4, 4, 4]);
        assert_eq!((deskew(1), deskew(2), deskew(4)), (424, 145, 145));
        // live check 2026-10-10 (10 ms/div, 100 kpts): LA alone 500 kSa/s -> LA period 4 us; with CH1 shown the rate
        // is halved (250 kSa/s, 8 us), memsel and pre stay
        let p = hz::Params { ratio: 1, rate: 500_000, memsel: 100_000, dot_fs: 2_000_000_000, seg: 16384, max_frames: 100, pre: 50_000, post: 50_000 };
        let alone = prep(p, false);
        assert_eq!((alone.rate, alone.memsel, alone.seg), (500_000, 100_000, 32768));
        assert_eq!(timing(&alone, 1), Timing { m: 1, rate: 250_000, n: 50_000, pre: 25_000 });
        let shown = prep(p, true);
        assert_eq!((shown.rate, shown.dot_fs, shown.memsel, shown.pre), (250_000, 4_000_000_000, 100_000, 50_000));
        assert_eq!(timing(&shown, 1).rate, 125_000);
        // 156.25 MSa/s halves to 62.5, not 78.125
        assert_eq!(prep(hz::Params { rate: 156_250_000, ..p }, true).rate, 62_500_000);
        // the fastest LA rate: one channel hidden at 1.25 GSa/s, or m = 4 at 312.5 MSa/s per channel unhalved
        assert_eq!(timing(&hz::Params { rate: 1_250_000_000, ..p }, 1).rate, 625_000_000);
        assert_eq!(timing(&hz::Params { rate: 312_500_000, ratio: 4, ..p }, 0xf).rate, 625_000_000);
    }

    #[test]
    fn config_sets_the_enable_and_doubles_the_record() {
        let mut sh = crate::dho::scu::Shadows::HANDOVER;
        let p = prep(hz::prep(1, 200_000_000_000, 100_000, 0), true);
        // enable and deskew in the acquisition configuration, before the pre-processing reset (0x1000 bit 5)
        let a = crate::dho::acq::configure_la(&mut sh, &p, 1, 0, 0, crate::dho::acq::Mode::Normal, Some(424), [0; 4]);
        let at = |r: u32| a.iter().position(|x| x.0 == r).unwrap();
        let reset = a.iter().position(|x| x.0 == 0x1000 && x.1 & 0x20 != 0).unwrap();
        assert!(a[at(0x4008)].1 & 1 << 26 != 0 && at(0x4008) < reset && at(0x128c) < reset);
        let w = config(&mut sh, &p, 1, false);
        assert_eq!(sh.s4008 & (1 << 26 | 1 << 24 | 1 << 23), 1 << 26 | 1 << 23);
        assert_eq!(w.iter().filter(|x| x.0 == 0x4008).count(), 2);
        assert!(w.contains(&(0x10e4, 2 * p.memsel)) && w.contains(&(0x4050, 2 * p.pre)) && w.contains(&(0x4054, 2 * p.post)));
        assert!(w.contains(&(0x10e0, p.seg)));
        // 0x131c: x = 0 at ratio 1 (the live shadow 0x2580004), 4 with the LA off
        assert!(w.contains(&(0x131c, 0x0258_0004)));
        assert_eq!((wave_early(Some(2)).1, wave_early(Some(4)).1, wave_early(None).1), (0x0258_0034, 0x0258_0064, 0x0258_0044));
        // roll: 0x10e4 not doubled, LA stream on
        let w = config(&mut sh, &p, 1, true);
        assert!(w.contains(&(0x10e4, p.memsel)) && sh.s4008 & (1 << 24) != 0);
    }

    #[test]
    fn raw_window_read_like_step_b() {
        // la.md 4.2: N = 1000 -> 0x1100 = 10 000, 0x12c0 = 4000, one read of 2000 bytes; TX engine on the LA stream
        let mut sh = crate::dho::scu::Shadows::HANDOVER;
        let w = readout(&mut sh, 1000, 0, 1, 1);
        assert_eq!(&w[..2], &[(0x4008, 0x80_0001 & !(1 << 23)), (0x4008, (0x80_0001 & !(1 << 23)) | 1 << 24)]);
        assert!(w.contains(&(0x1100, 10_000)) && w.contains(&(0x1108, 0)) && w.contains(&(0x12c0, 4000)));
        assert_eq!(read_bytes(1000), 2000);
        assert_eq!(read_bytes(1001), 2008);
        // play-last pulse and FSM run at the end
        assert_eq!(*w.last().unwrap(), (0x4004, sh.s4004));
        assert!(sh.s4004 & 5 == 5 && sh.s1010 >> 31 == 1);
        let r = restore(&mut sh);
        assert_eq!(sh.s4008 & (1 << 23 | 1 << 24), 1 << 23);
        assert_eq!(r.len(), 2);
        assert_eq!(passthrough()[0], (0x1298, 0x4000_0001));
        // long records: all of them, up to READ_MAX
        assert_eq!(window(&Timing { m: 1, rate: 1, n: 1000, pre: 500 }), (1000, 0));
        assert_eq!(window(&Timing { m: 1, rate: 1, n: 10_000_000, pre: 5_000_000 }), (10_000_000, 0));
        assert_eq!(window(&Timing { m: 1, rate: 1, n: 50_000_000, pre: 5_000_000 }), (READ_MAX, 0));
        assert_eq!(words(&[1, 0, 0, 0x80, 9, 9], 2), [1, 0x8000]);
    }

    #[test]
    fn threshold_presets_to_codes() {
        // la.md 5.1 formula on the UI presets (pod 1 zero 8609): TTL 1.4 V, CMOS 5 V 2.5 V, CMOS 3.3 V 1.65 V,
        // LVCMOS 2.5 / 1.8 / 1.2 V (1.25 / 0.9 / 0.6 V), ECL -1.3 V
        let c = Cal::default();
        let codes: Vec<u32> = [1.4, 2.5, 1.65, 1.25, 0.9, 0.6, -1.3].iter().map(|v| thresholds(&c, *v, 0.0).1 & 0xffff).collect();
        assert_eq!(codes, [9043, 9384, 9120, 8996, 8888, 8795, 8205]);
    }
}
