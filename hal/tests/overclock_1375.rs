// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! A clock off the 1-2-5 grid (1.375 GHz, the first step of the overclocking test), in its own process: the rate the
//! time base model reports must be the rate the SPU dividers it writes really make, or the time axis is wrong.

use oos_hal::{model, dho::{clock, hz, spu}};

/// Sample rate the SPU makes from its registers at the clock in effect (nominal registers scaled to real).
fn rate_of(w: &[(u32, u32)], max: f64) -> f64 {
    let d0 = w.iter().find(|x| x.0 == 0x10d0).unwrap().1;
    let d8 = w.iter().find(|x| x.0 == 0x10d8).unwrap().1;
    if d0 & 0x8000 != 0 {
        max / (d0 & 0xff) as f64
    } else {
        clock::adc_hz() as f64 / 20.0 / d8 as f64
    }
}

#[test]
fn dividers_match_the_model() {
    let p = model::Platform::build(None, Some("adc.clock_hz = 1.375e9\n"), None);
    assert_eq!(p.limits.adc_clock_hz, 1.375e9);
    model::init(p);
    let mut checked = 0;
    for mask in [1u32, 3, 15] {
        for tdiv_ns in [1i128, 5, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 200_000_000] {
            for depth in [0i128, 1_000, 100_000, 1_000_000] {
                let p = hz::prep(mask, tdiv_ns * 1_000_000, depth, 0);
                let max = clock::adc_hz() as f64 / p.ratio as f64;
                let got = rate_of(&spu::sample_rate(p.rate, max as u64), max);
                // the model keeps whole Sa/s: within that rounding (before the fix the error was up to 60 %)
                assert!((got - p.rate as f64).abs() <= 2.0 + p.rate as f64 * 1e-9, "mask {} {} ns/div depth {}: model {} dividers {}", mask, tdiv_ns, depth, p.rate, got);
                // and the period agrees with the rate
                assert!((p.dot_fs as f64 * p.rate as f64 / 1e15 - 1.0).abs() < 1e-3);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 120);
}
