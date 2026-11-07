// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Sample processing unit (0x1000 block): sample rate divider, memory size. docs/protocols/horizontal-config.md.

use crate::Write;

/// Sample rate: above 62.5 MSa/s 0x10d0 = 0x8000 |
/// (max rate / rate) and 0x10d8 = 1, else 0x10d0 = 0 and 0x10d8 = 62.5 M / rate; 0x10d4 = 0.
/// `max_rate` = maximum rate of the channel mode (1.25 G / ratio).
pub fn sample_rate(rate: u64, max_rate: u64) -> Vec<Write> {
    // the dividers count ADC clocks: real rates back to the 1.25 GHz domain (identity there), where the second
    // divider's base is 62.5 MHz (ADC / 20)
    let (rate, max_rate) = (crate::clock::to_nominal_rate(rate as i128) as u64, crate::clock::to_nominal_rate(max_rate as i128) as u64);
    let (r10d0, r10d8) = if rate > 62_500_000 { (0x8000 | ((max_rate / rate) as u32 & 0xff), 1) } else { (0, (62_500_000 / rate) as u32) };
    vec![(0x10d0, r10d0), (0x10d8, r10d8), (0x10d4, 0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dividers() {
        assert_eq!(sample_rate(1_250_000_000, 1_250_000_000), [(0x10d0, 0x8001), (0x10d8, 1), (0x10d4, 0)]);
        assert_eq!(sample_rate(312_500_000, 1_250_000_000)[0], (0x10d0, 0x8004));
        assert_eq!(sample_rate(62_500_000, 1_250_000_000)[..2], [(0x10d0, 0), (0x10d8, 1)]);
        assert_eq!(sample_rate(31_250_000, 1_250_000_000)[1], (0x10d8, 2));
    }
}
