// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal levels from the FPGA's ADC measure unit (fork, docs/fork/trigger-lockin.md "(a)"): the lowest, highest and
//! mean value of every channel without an acquisition, for the auto set-up's vertical step.
//!
//! The unit (private RE notes docs/fpga.md, "Measure unit"; self-cal.md 2.4): 0x1040 bit 31 starts a run of 2^e samples
//! per ADC core (e = 0x1040[4:0]) at 156.25 MHz, bit 30 reports it done. 0x1040[10:8] selects the core (slot) the read
//! registers show: 0x1048 = its 12-bit maximum [27:16] and minimum [11:0], 0x1044 = its mean on the 16-bit scale. The
//! values are taken after the ADC x2 of 0x105c and before the Q10 read-path gain and offset (wpu.rs), so a code c on
//! the 16-bit scale is the read-out word 32768 + (c - 32768) * dg, dg = the channel's digital gain without the x2.
//!
//! The slots of a channel follow the core order of the DDR words (fpga.md "Block map and data paths"): 4 channels
//! 6, 4, 2, 0, 7, 5, 3, 1 (CH1 = 6/7, CH2 = 4/5, CH3 = 2/3, CH4 = 0/1), 2 channels 6, 2, 4, 0, 7, 3, 5, 1 (the lower
//! channel 6/4/7/5, the higher 2/0/3/1), 1 channel all eight. That the measure unit numbers the cores like the DDR
//! words is assumed, not verified on the unit: `channel_levels` refuses a channel whose slots disagree, and the auto
//! set-up then measures from records.

use crate::dho::bus::Bus;

/// One slot of one run: 12-bit minimum and maximum, mean on the 16-bit scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    pub min: u16,
    pub max: u16,
    pub mean: f64,
}

/// Exponent of a run: 2^18 samples per core, 1.68 ms at 156.25 MHz (the largest the 19-bit sample counter takes).
pub const RUN_EXP: u32 = 18;
const RUN_US: u32 = ((1u64 << RUN_EXP) * 1_000_000 / 156_250_000) as u32 + 1;

/// One run of the measure unit: start, wait for done, read every slot. None when the unit does not finish or a read
/// is not ready.
pub fn run(bus: &mut dyn Bus, sleep_us: &mut dyn FnMut(u32)) -> Option<[Slot; 8]> {
    let cfg = (bus.rd(0x1040) & 0x77ff_f0e0) | RUN_EXP;
    bus.wr(0x1040, cfg);
    bus.wr(0x1040, cfg | 0x8000_0000);
    sleep_us(RUN_US);
    let mut done = bus.rd(0x1040) & 0x4000_0000 != 0;
    for _ in 0..5 {
        if done {
            break;
        }
        sleep_us(500);
        done = bus.rd(0x1040) & 0x4000_0000 != 0;
    }
    if !done {
        return None;
    }
    let sh = bus.rd(0x1040);
    let mut out = [Slot::default(); 8];
    for (k, o) in out.iter_mut().enumerate() {
        bus.wr(0x1040, (sh & !0xf00) | (k as u32) << 8);
        let (m, p) = (bus.rd(0x1044), bus.rd(0x1048));
        if m == 0xeeee_eeee || p == 0xeeee_eeee {
            return None;
        }
        *o = Slot { min: (p & 0xfff) as u16, max: (p >> 16 & 0xfff) as u16, mean: (m & 0xffff) as f64 };
    }
    Some(out)
}

/// The slots of channel `ch` (0..3) for the channel set `mask` (module comment); empty when it is not in the set.
pub fn slots(mask: u32, ch: usize) -> Vec<usize> {
    if mask & (1 << ch) == 0 {
        return Vec::new();
    }
    match crate::dho::hz::ratio(mask) {
        1 => (0..8).collect(),
        2 => {
            let lower = (0..ch).all(|c| mask & (1 << c) == 0);
            if lower { vec![6, 4, 7, 5] } else { vec![2, 0, 3, 1] }
        }
        _ => [[6, 7], [4, 5], [2, 3], [0, 1]][ch].to_vec(),
    }
}

/// Levels of one channel in volts at the BNC side of the scale (as the read-out: `seq::volts`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    pub lo: f64,
    pub hi: f64,
    pub mean: f64,
    /// a sample reached the end of the ADC range: the scale is too small
    pub clipped: bool,
}

/// The vertical set-up of one channel the levels are converted with: V/div, offset (V), digital gain (cal record,
/// 1.0 if unknown).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
    pub scale: f64,
    pub offset: f64,
    pub gain: f64,
}

/// Volts of a code on the 16-bit scale of the measure unit: the read-out word 32768 + (c - 32768) * dg through
/// `seq::volts`. dg = the digital gain without the ADC x2 (wpu.rs).
pub fn volts(c: f64, v: &Vert) -> f64 {
    let dg = if v.gain >= 2.0 { v.gain / 2.0 } else { v.gain };
    (c - 32768.0) * dg / 7500.0 * v.scale - v.offset
}

/// The levels of every channel of `mask` from one or more runs (lowest and highest over all runs, mean of the
/// means). None for a channel without slots, or whose slots disagree: their means further apart than a tenth of the
/// channel's swing plus 64 codes, which a wrong slot mapping gives unless the channels carry the same signal.
pub fn channel_levels(runs: &[[Slot; 8]], mask: u32, vert: &[Vert; 4]) -> [Option<Levels>; 4] {
    std::array::from_fn(|ch| {
        let sl = slots(mask, ch);
        if sl.is_empty() || runs.is_empty() {
            return None;
        }
        let lo = runs.iter().flat_map(|r| sl.iter().map(move |&k| r[k].min)).min()?;
        let hi = runs.iter().flat_map(|r| sl.iter().map(move |&k| r[k].max)).max()?;
        if lo > hi {
            return None;
        }
        let means: Vec<f64> = sl.iter().map(|&k| runs.iter().map(|r| r[k].mean).sum::<f64>() / runs.len() as f64).collect();
        let (mlo, mhi) = means.iter().fold((f64::MAX, f64::MIN), |(a, b), &m| (a.min(m), b.max(m)));
        let swing = (hi - lo) as f64 * 16.0;
        if mhi - mlo > 0.1 * swing + 64.0 {
            return None;
        }
        let mean = means.iter().sum::<f64>() / means.len() as f64;
        // the 12-bit extremes on the 16-bit scale: the middle of the code's step
        let c16 = |c: u16| c as f64 * 16.0 + 8.0;
        let v = &vert[ch];
        // the mean lies between the extremes, or the readings do not belong together
        if mean < c16(lo) - 16.0 || mean > c16(hi) + 16.0 {
            return None;
        }
        let (a, b) = (volts(c16(lo), v), volts(c16(hi), v));
        Some(Levels { lo: a.min(b), hi: a.max(b), mean: volts(mean, v), clipped: lo == 0 || hi == 4095 })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measure unit of a signal per slot: slot k sees `sig(k)` = (min, max, mean) as raw 16-bit codes.
    struct Unit {
        sel: usize,
        started: bool,
        sig: Vec<(f64, f64, f64)>,
    }

    impl Bus for Unit {
        fn wr(&mut self, off: u32, v: u32) {
            if off == 0x1040 {
                self.sel = (v >> 8 & 7) as usize;
                self.started |= v & 0x8000_0000 != 0;
            }
        }
        fn rd(&mut self, off: u32) -> u32 {
            let (lo, hi, mean) = self.sig[self.sel];
            match off {
                0x1040 => (self.started as u32) << 30 | (self.sel as u32) << 8 | RUN_EXP,
                0x1044 => mean as u32,
                0x1048 => ((hi / 16.0) as u32) << 16 | (lo / 16.0) as u32,
                _ => 0,
            }
        }
    }

    /// The read-out word of a raw 16-bit code through the read path (wpu.rs): x2 then Q10 gain dg around 32768.
    fn word(raw: f64, gain: f64) -> f64 {
        let dg = if gain >= 2.0 { gain / 2.0 } else { gain };
        32768.0 + (raw - 32768.0) * dg
    }

    // Break it catches: levels from the measure unit that differ from what the record path shows for the same
    // samples (another gain, the x2 counted twice, the offset the wrong way), or slots of another channel
    #[test]
    fn levels_equal_the_record_path_for_the_same_codes() {
        for (gain, scale, offset) in [(1.077, 1.0, 0.0), (1.077, 0.1, -0.148), (2.6, 0.002, 0.003), (1.0, 5.0, 2.5)] {
            let v = Vert { scale, offset, gain };
            // CH1 alone, all slots alike: a square from raw 20000 to 45000 (16-bit), mean halfway
            let (lo, hi) = (20000.0, 45000.0);
            let mut u = Unit { sel: 0, started: false, sig: vec![(lo, hi, (lo + hi) / 2.0); 8] };
            let r = run(&mut u, &mut |_| {}).unwrap();
            let l = channel_levels(&[r], 1, &[v; 4])[0].unwrap();
            // the record path: words through seq::volts
            let rec = |raw: f64| crate::dho::seq::volts(&[word(raw, gain).round() as u16], scale, offset)[0] as f64;
            let tol = scale * 24.0 * gain / 7500.0 + 1e-6;
            assert!((l.lo - rec(lo)).abs() < tol && (l.hi - rec(hi)).abs() < tol, "gain {} scale {}: {:?} vs {} {}", gain, scale, l, rec(lo), rec(hi));
            assert!((l.mean - rec((lo + hi) / 2.0)).abs() < tol);
            assert!(!l.clipped);
        }
    }

    #[test]
    fn slots_follow_the_ddr_core_order_and_disagreeing_slots_are_refused() {
        assert_eq!(slots(1, 0), (0..8).collect::<Vec<_>>());
        assert_eq!((slots(0b1010, 1), slots(0b1010, 3)), (vec![6, 4, 7, 5], vec![2, 0, 3, 1]));
        assert_eq!((slots(0xf, 0), slots(0xf, 3), slots(0b1011, 2)), (vec![6, 7], vec![0, 1], vec![]));
        let v = [Vert { scale: 1.0, offset: 0.0, gain: 1.0 }; 4];
        // four channels: CH1 on slots 6/7 a 2 V square, CH4 on 0/1 flat; the others flat at other levels
        let mut s = [Slot { min: 2040, max: 2056, mean: 32768.0 }; 8];
        s[6] = Slot { min: 1500, max: 2600, mean: 32800.0 };
        s[7] = Slot { min: 1502, max: 2598, mean: 32790.0 };
        let l = channel_levels(&[s], 0xf, &v);
        assert!(l[0].is_some_and(|x| x.hi - x.lo > 2.0) && l[3].is_some_and(|x| x.hi - x.lo < 0.05));
        // a slot of CH1 that shows another signal (wrong mapping): refused
        s[7] = Slot { min: 3000, max: 3100, mean: 3050.0 * 16.0 };
        assert!(channel_levels(&[s], 0xf, &v)[0].is_none());
        // clipping at the top of the range
        s[7] = Slot { min: 1500, max: 4095, mean: 32800.0 };
        s[6].max = 4095;
        assert!(channel_levels(&[s], 0xf, &v)[0].is_some_and(|x| x.clipped));
        // several runs: extremes over all of them
        let a = [Slot { min: 1000, max: 2000, mean: 24000.0 }; 8];
        let b = [Slot { min: 1500, max: 3000, mean: 36000.0 }; 8];
        let l = channel_levels(&[a, b], 1, &v)[0].unwrap();
        assert!((l.lo - volts(1000.0 * 16.0 + 8.0, &v[0])).abs() < 1e-9 && (l.hi - volts(3000.0 * 16.0 + 8.0, &v[0])).abs() < 1e-9);
        // a unit that never finishes: no reading
        struct Dead;
        impl Bus for Dead {
            fn wr(&mut self, _: u32, _: u32) {}
            fn rd(&mut self, _: u32) -> u32 {
                0
            }
        }
        assert!(run(&mut Dead, &mut |_| {}).is_none());
    }
}
