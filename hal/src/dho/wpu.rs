// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Read-path digital gain and offset (afe-settings.md 4.1, cold-boot.md 3.9): the acquisition
//! memory holds raw 12-bit ADC codes, the read-out words are scaled on the way out with a per-lane gain (Q10) and
//! offset. Registers: gain 0x14c8/0x14d4/0x14e0/0x14ec, offset 0x14d0/0x14dc/0x14e8/0x14f4; zoom copies
//! 0x1508..0x1514 / 0x14f8..0x1504 [N].
//! A digital gain >= 2 is split into the ADC x2 of 0x105c and the rest.

use crate::dho::Write;

/// The read-path writes for channel `mask` and the digital gain of each channel (cal_vertical record `gain`, 1.0 for
/// unknown). Lanes as the stock firmware writes them: 1 channel -> all four lanes, 2 -> lanes 0/2 lower and 1/3
/// higher channel, 4 -> lane n = CH n+1. With two channels this differs from the invert and filter bits of 0x1004
/// (`acq::lanes`: 0/1 lower, 2/3 higher): those follow the ADC core groups, the read path the interleaved words.
pub fn analog(mask: u32, dgain: [f64; 4]) -> Vec<Write> {
    analog_gnd(mask, dgain, [None; 4])
}

/// `analog` with GND coupling: a channel with `gnd[c]` set has gain 0 and that offset (afe-settings.md 4.1).
pub fn analog_gnd(mask: u32, dgain: [f64; 4], gnd: [Option<u32>; 4]) -> Vec<Write> {
    let mut w = Vec::new();
    // ADC x2 for channels with gain >= 2 (0x105c, layout as hal::dho::adc::wpu_gain), the rest in the Q10 gain
    let x2 = dgain.map(|g| if g >= 2.0 { 2 } else { 1 });
    for (_, v) in crate::dho::adc::wpu_gain(mask, x2, 0x5555) {
        w.push((0x105c, v));
    }
    let dg = [0, 1, 2, 3].map(|c| if x2[c] == 2 { dgain[c] / 2.0 } else { dgain[c] });
    let on: Vec<usize> = (0..4).filter(|c| mask & (1 << c) != 0).collect();
    let lane_ch: [usize; 4] = match crate::dho::hz::ratio(mask) {
        1 => [on[0]; 4],
        2 => [on[0], on[1], on[0], on[1]],
        _ => [0, 1, 2, 3],
    };
    let gain = |c: usize| if gnd[c].is_some() { 0 } else { (dg[c].min(508.0) * 1024.0) as i32 as u32 };
    let offset = |c: usize| gnd[c].unwrap_or((32768.0 - 32768.0 * dg[c].min(508.0)) as i32 as u32);
    for (k, r) in [0x14c8, 0x14d4, 0x14e0, 0x14ec].into_iter().enumerate() {
        w.push((r, gain(lane_ch[k])));
    }
    for (k, r) in [0x14d0, 0x14dc, 0x14e8, 0x14f4].into_iter().enumerate() {
        w.push((r, offset(lane_ch[k])));
    }
    for (k, r) in [0x1508, 0x150c, 0x1510, 0x1514].into_iter().enumerate() {
        w.push((r, gain(lane_ch[k])));
    }
    for (k, r) in [0x14f8, 0x14fc, 0x1500, 0x1504].into_iter().enumerate() {
        w.push((r, offset(lane_ch[k])));
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_offset_and_lanes() {
        // CH1 at 1 V/div, OFF bandwidth: digital gain 1.07888 (afesim) -> Q10 1104, offset (int)(32768 - 35352.7) = -2584
        let w = analog(1, [1.07888, 1.0, 1.0, 1.0]);
        assert_eq!(w[0], (0x105c, 0x5555));
        assert_eq!(&w[1..5], &[(0x14c8, 1104), (0x14d4, 1104), (0x14e0, 1104), (0x14ec, 1104)]);
        assert_eq!(w[5], (0x14d0, (-2584i32) as u32));
        // two channels (as stock: gains lo, hi, lo, hi): lanes 0/2 = CH2, 1/3 = CH4, gains and offsets, main and
        // zoom. Break it catches the lane order of the invert bits (0/1, 2/3), which put every other word of each
        // channel through the other channel's gain and offset (unit 2026-11-13: CH3 open at +2 V offset read +143 mV
        // with CH1 acquired at another V/div, -30 mV alone)
        let w = analog(0b1010, [1.0, 1.5, 1.0, 1.25]);
        let at = |r: u32| w.iter().find(|x| x.0 == r).unwrap().1;
        assert_eq!([0x14c8, 0x14d4, 0x14e0, 0x14ec].map(at), [1536, 1280, 1536, 1280]);
        assert_eq!([0x1508, 0x150c, 0x1510, 0x1514].map(at), [1536, 1280, 1536, 1280]);
        let off = |g: f64| (32768.0 - 32768.0 * g) as i32 as u32;
        assert_eq!([0x14d0, 0x14dc, 0x14e8, 0x14f4].map(at), [off(1.5), off(1.25), off(1.5), off(1.25)]);
        assert_eq!([0x14f8, 0x14fc, 0x1500, 0x1504].map(at), [off(1.5), off(1.25), off(1.5), off(1.25)]);
        // four channels: lane n = CH n+1
        let w = analog(0b1111, [1.0, 1.25, 1.5, 2.5]);
        let at = |r: u32| w.iter().find(|x| x.0 == r).unwrap().1;
        assert_eq!([0x14c8, 0x14d4, 0x14e0, 0x14ec].map(at), [1024, 1280, 1536, 1280]);
        // gain 3 -> ADC x2 (0xaa bytes) and 1.5 in the Q10 gain
        let w = analog(1, [3.0, 1.0, 1.0, 1.0]);
        assert_eq!(w[0], (0x105c, 0xaaaa));
        assert!(w.contains(&(0x14c8, 1536)));
    }
}
