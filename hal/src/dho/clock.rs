// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The ADC sample clock and the clocks derived from it (docs/protocols/pll.md, "Places in OOS that assume ...").
//!
//! The FPGA works in clock periods: decimation ratios, segment sizes, trigger ticks (1.6 / 6.4 ns at 1.25 GHz), the
//! roll stream (one sample per FPGA clock = ADC / 8) and the generator DDS (ADC / 8). So the register values the
//! models produce for 1.25 GHz stay right at another clock; only their meaning in seconds and hertz scales. OOS keeps
//! the models (hz, tpu, afg, roll) in the nominal 1.25 GHz domain and converts at their edges:
//! real time t -> nominal t * f / f0 before the model, nominal rate r -> real r * f / f0 after it.
//!
//! The clock is `model::platform().limits.adc_clock_hz`: 1.25 GHz unless limits.conf sets `adc.clock_hz` (experimental,
//! for the overclocking test; the cold boot then writes `lmx2582::table_for` instead of Rigol's table). At 1.25 GHz
//! every conversion is the identity, so nothing changes for normal use.

/// The clock all of the stock tables and the models are made for.
pub const NOMINAL_HZ: i128 = 1_250_000_000;
/// The FPGA logic clock at `NOMINAL_HZ`: ADC / 8, 156.25 MHz.
pub const NOMINAL_FPGA_HZ: i128 = NOMINAL_HZ / 8;
/// One ADC period at `NOMINAL_HZ` in fs: 0.8 ns.
pub const ADC_TICK_FS: i128 = 800_000;
/// The trigger event time unit at `NOMINAL_HZ` in fs: 1.6 ns, two ADC periods.
pub const TPU_TICK_FS: i128 = 2 * ADC_TICK_FS;
/// One FPGA clock period at `NOMINAL_HZ` in fs: 6.4 ns.
pub const FPGA_TICK_FS: i128 = 8 * ADC_TICK_FS;
/// `ADC_TICK_FS` in seconds (the same double as the literal 0.8e-9: both operands are exact).
pub const ADC_TICK_S: f64 = ADC_TICK_FS as f64 / 1e15;
/// `FPGA_TICK_FS` in seconds (the same double as the literal 6.4e-9).
pub const FPGA_TICK_S: f64 = FPGA_TICK_FS as f64 / 1e15;

/// The ADC sample clock in effect (Hz).
pub fn adc_hz() -> i128 {
    (crate::model::platform().limits.adc_clock_hz.round() as i128).max(1)
}

/// FPGA logic / roll stream / generator DDS clock: ADC / 8 (156.25 MHz at 1.25 GHz).
pub fn fpga_hz() -> f64 {
    adc_hz() as f64 / 8.0
}

/// True when the clock differs from 1.25 GHz (experimental).
pub fn overclocked() -> bool {
    adc_hz() != NOMINAL_HZ
}

/// A real time (fs) in the nominal domain: t * f / f0. Identity at 1.25 GHz.
pub fn to_nominal_fs(t_fs: i128) -> i128 {
    scale_by(t_fs, adc_hz(), NOMINAL_HZ)
}

/// A nominal-domain time (fs) as real time: t * f0 / f.
pub fn from_nominal_fs(t_fs: i128) -> i128 {
    scale_by(t_fs, NOMINAL_HZ, adc_hz())
}

/// A nominal-domain rate (Sa/s, Hz) as real rate: r * f / f0.
pub fn from_nominal_rate(r: i128) -> i128 {
    scale_by(r, adc_hz(), NOMINAL_HZ)
}

/// A real rate as nominal-domain rate: r * f0 / f.
pub fn to_nominal_rate(r: i128) -> i128 {
    scale_by(r, NOMINAL_HZ, adc_hz())
}

/// x * num / den, rounded to nearest; exact identity when num == den.
fn scale_by(x: i128, num: i128, den: i128) -> i128 {
    if num == den {
        return x;
    }
    let p = x * num;
    if p >= 0 {
        (p + den / 2) / den
    } else {
        (p - den / 2) / den
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nominal_by_default() {
        assert_eq!(adc_hz(), NOMINAL_HZ);
        assert_eq!(fpga_hz(), 156.25e6);
        assert!(!overclocked());
        assert_eq!((NOMINAL_FPGA_HZ, ADC_TICK_S, FPGA_TICK_S), (156_250_000, 0.8e-9, 6.4e-9));
        assert_eq!((ADC_TICK_S.to_bits(), FPGA_TICK_S.to_bits()), (0.8e-9f64.to_bits(), 6.4e-9f64.to_bits()));
        assert_eq!((to_nominal_fs(123_456), from_nominal_rate(625_000_000)), (123_456, 625_000_000));
    }

    #[test]
    fn scaling() {
        // 2 GHz: 1 ns real = 1.6 ns nominal, nominal 625 MSa/s = 1 GSa/s real
        assert_eq!(scale_by(1_000_000, 2_000_000_000, NOMINAL_HZ), 1_600_000);
        assert_eq!(scale_by(625_000_000, 2_000_000_000, NOMINAL_HZ), 1_000_000_000);
        assert_eq!(scale_by(-5, 3, 2), -8);
    }
}
