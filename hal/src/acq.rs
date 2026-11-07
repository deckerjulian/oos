// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The acquisition registers written before every RUN/SINGLE,
//! docs/protocols/horizontal-config.md section 4.1 (writes 5-28). Not written here, so they keep the stock
//! values: the per-channel sample delays 0x1080/0x1084 and LA delay 0x128c (deskew, they depend on the channel set),
//! the DVM block, and the ADC input mode / per-mode ADC calibration (only when the channel set changes).

use crate::hz::Params;
use crate::scu::Shadows;
use crate::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Peak,
}

/// Per-lane field (4 bits, lanes 0..3) of a per-channel flag set `chans` (bit n = CH n+1) for channel `mask`:
/// ratio 1: the channel's flag in all lanes; ratio 2: lanes 0,1 = lower channel, 2,3 = higher; ratio 4: lane n = CH n+1.
pub fn lanes(mask: u32, chans: u32) -> u32 {
    let on: Vec<u32> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
    let f = |c: u32| (chans >> c) & 1;
    match crate::hz::ratio(mask) {
        1 => {
            if on.is_empty() {
                0
            } else {
                f(on[0]) * 0xf
            }
        }
        2 => f(on[0]) * 0x3 | f(on[1]) * 0xc,
        _ => chans & 0xf,
    }
}

/// The acquisition writes for parameters `p` (from `hz::prep`), channel `mask`, per-channel invert and 20 MHz
/// filter flags, acquire mode (average is not implemented yet: own segment layout).
pub fn configure(sh: &mut Shadows, p: &Params, mask: u32, invert: u32, bwl: u32, mode: Mode) -> Vec<Write> {
    configure_la(sh, p, mask, invert, bwl, mode, None, [0; 4])
}

/// One step of the sample delays at ratio 1 (fs).
/// Measured on the unit with one source on two channels: a step of 0x1080 at ratio 2 moves a channel by 1.6 ns, one
/// period of 1.25 GHz per ratio (the stock constant is 600,000). At ratio 4 only every second step moves it, by 6.4 ns.
pub const SA_DELAY_FS: i64 = 800_000;

/// `configure` with the LA on when `la_deskew` is given (0x128c): the LA enable and the deskew go in before
/// the pre-processing reset.
/// `skew`: per-channel deskew in fs.
#[allow(clippy::too_many_arguments)]
pub fn configure_la(sh: &mut Shadows, p: &Params, mask: u32, invert: u32, bwl: u32, mode: Mode, la_deskew: Option<u32>, skew: [i64; 4]) -> Vec<Write> {
    let mut w = Vec::new();
    // real rates (`hz::prep`), so the real clock: the ratio max_rate / rate is the same at any clock
    let max_rate = crate::clock::adc_hz() as u64 / p.ratio as u64;
    // ADC order
    sh.s1004 |= 1 << 26;
    w.push((0x1004, sh.s1004));
    // LA enable, channel processing on, LA processing off
    sh.s4008 = (sh.s4008 & !(1 << 26)) | (la_deskew.is_some() as u32) << 26;
    w.push((0x4008, sh.s4008));
    sh.s4008 |= 1 << 23;
    w.push((0x4008, sh.s4008));
    sh.s4008 &= !(1 << 24);
    w.push((0x4008, sh.s4008));
    // bit2 = rate < max rate / 8
    sh.s1004 = (sh.s1004 & !4) | if p.rate < max_rate / 8 { 4 } else { 0 };
    w.push((0x1004, sh.s1004));
    // sample delays (cold-boot.md 3.10): base 480 / 240 / 120 plus d / (ratio T)
    let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
    let d = |k: usize| on.get(k).map_or(0, |&c| skew[c]);
    let at = |d: i64, base: i64, ratio: i64| (base + (d as f64 / (ratio * SA_DELAY_FS) as f64).round() as i64).clamp(0, 0xffff) as u32;
    match p.ratio {
        1 => sh.s1080 = at(d(0), 480, 1) << 16 | (sh.s1080 & 0xffff),
        2 => sh.s1080 = at(d(0), 240, 2) << 16 | at(d(1), 240, 2),
        _ => {
            sh.s1080 = at(skew[0], 120, 4) << 16 | at(skew[1], 120, 4);
            sh.s1084 = at(skew[2], 120, 4) << 16 | at(skew[3], 120, 4);
        }
    }
    w.push((0x1080, sh.s1080));
    w.push((0x1084, sh.s1084));
    if let Some(d) = la_deskew {
        w.push((0x128c, d));
    }
    // pre-processing reset
    sh.s1000 |= 0x20;
    w.push((0x1000, sh.s1000));
    sh.s1000 &= !0x20;
    w.push((0x1000, sh.s1000));
    // channel invert [19:16], channel filter [23:20]
    sh.s1004 = (sh.s1004 & !0x000f_0000) | lanes(mask, invert) << 16;
    w.push((0x1004, sh.s1004));
    sh.s1004 = (sh.s1004 & !0x00f0_0000) | lanes(mask, bwl) << 20;
    w.push((0x1004, sh.s1004));
    w.extend(crate::spu::sample_rate(p.rate, max_rate));
    // acquisition mode: bits 0,1,3,12 cleared; peak = bit1 (forced to normal at the maximum rate)
    let peak = mode == Mode::Peak && p.rate < max_rate;
    sh.s1004 = (sh.s1004 & !0x100b) | if peak { 2 } else { 0 };
    w.push((0x1004, sh.s1004));
    // averaging (count 2), pre-sample compensation 5
    w.push((0x10b0, 1));
    w.push((0x1148, 640 / p.ratio + 32));
    w.push((0x404c, 5));
    // record depth, pre and post points, memory size
    w.push((0x10e4, p.memsel));
    w.push((0x4050, p.pre));
    w.push((0x4054, p.post));
    w.push((0x10e0, p.seg));
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hz::prep;
    const US: i128 = 1_000_000_000;

    fn last(w: &[Write], reg: u32) -> u32 {
        w.iter().rev().find(|x| x.0 == reg).unwrap().1
    }

    #[test]
    fn example1_ch1_200us() {
        let mut sh = Shadows::HANDOVER;
        let w = configure(&mut sh, &prep(1, 200 * US, 0, 0), 1, 0, 0, Mode::Normal);
        assert_eq!(last(&w, 0x1004), 0x0400_0000); // the stock value
        assert_eq!(last(&w, 0x10d0), 0x8001);
        assert_eq!(last(&w, 0x1148), 672);
        assert_eq!(last(&w, 0x10e4), 0x2625a0);
        assert_eq!((last(&w, 0x4050), last(&w, 0x4054)), (0x1312d0, 0x1312d0));
        assert_eq!(last(&w, 0x10e0), 0x40000);
        assert_eq!(sh.s4008, 0x80_0001);
        assert_eq!((last(&w, 0x1080), last(&w, 0x1084)), (0x01e0_0000, 0));
        // deskew: 8 ns are 10 steps at ratio 1, 5 at ratio 2, -3.2 ns one down at ratio 4
        let mut sh = Shadows::HANDOVER;
        let w = configure_la(&mut sh, &prep(1, 200 * US, 0, 0), 1, 0, 0, Mode::Normal, None, [8_000_000, 0, 0, 0]);
        assert_eq!(last(&w, 0x1080), 490 << 16);
        let w = configure_la(&mut sh, &prep(0b101, 200 * US, 0, 0), 0b101, 0, 0, Mode::Normal, None, [0, 0, 8_000_000, 0]);
        assert_eq!(last(&w, 0x1080), 240 << 16 | 245);
        let w = configure_la(&mut sh, &prep(0b1111, 200 * US, 0, 0), 0b1111, 0, 0, Mode::Normal, None, [0, 0, 0, -3_200_000]);
        assert_eq!((last(&w, 0x1080), last(&w, 0x1084)), (0x0078_0078, 120 << 16 | 119));
    }

    #[test]
    fn example2_ch12_1us() {
        let mut sh = Shadows::HANDOVER;
        let w = configure(&mut sh, &prep(3, US, 0, 0), 3, 0b10, 0b01, Mode::Normal);
        assert_eq!(last(&w, 0x1148), 352);
        assert_eq!(last(&w, 0x10e4), 0x186a);
        assert_eq!(last(&w, 0x4050), 0xc35);
        assert_eq!(last(&w, 0x10e0), 0x800);
        // horizontal-config.md 4.2 step 10: 0x00f000f0 for zero delays
        assert_eq!(last(&w, 0x1080), 0x00f0_00f0);
        // CH2 inverted -> lanes 2,3; CH1 BW limit -> lanes 0,1
        assert_eq!(last(&w, 0x1004) & 0x00ff_0000, 0x003c_0000);
    }

    #[test]
    fn lane_fields() {
        assert_eq!(lanes(1, 1), 0xf);
        assert_eq!(lanes(4, 4), 0xf);
        assert_eq!(lanes(0xa, 0x8), 0xc);
        assert_eq!(lanes(0xf, 0x5), 0x5);
    }
}
