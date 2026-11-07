// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The experimental ADC clock (limits.conf `adc.clock_hz`, `oos_hal::clock`) end to end, in its own process (the
//! platform is set once per process): at 2 GHz the register values equal those of the nominal 1.25 GHz model for the
//! correspondingly scaled times, while rates and periods come out in real units.

use oos_hal::{acq, afg, boot, clock, hz, model, roll, scu::Shadows};

const US: i128 = 1_000_000_000;

fn init() {
    // every test of this file wants the same platform; the first init wins
    let p = model::Platform::build(None, Some("adc.clock_hz = 2e9\n"), Some("DHO924S"));
    assert_eq!(p.limits.adc_clock_hz, 2e9);
    model::init(p);
    assert!(clock::overclocked());
}

#[test]
fn timebase_is_real_registers_are_nominal() {
    init();
    // 125 ns/div real at 2 GHz = 200 ns/div nominal: same points, rate 2 GSa/s instead of 1.25
    let p = hz::prep(1, 125 * US / 1000, 0, 0);
    assert_eq!(p.rate, 2_000_000_000);
    assert_eq!(p.dot_fs, 500_000);
    assert_eq!(p.memsel, 2500, "10 div x 200 ns at 1.25 GSa/s nominal");
    // the acquisition registers see the same rate ratio as at 1.25 GHz
    let w = acq::configure(&mut Shadows::HANDOVER.clone(), &p, 1, 0, 0, acq::Mode::Normal);
    assert!(w.contains(&(0x10d0, 0x8001)), "{:x?}", w);
}

#[test]
fn derived_clocks_follow() {
    init();
    assert_eq!(clock::fpga_hz(), 250e6);
    assert_eq!(roll::max_rate(), 250_000_000);
    assert_eq!(afg::clock_hz(), 250e6);
    // 10 MHz at a 250 MHz DDS clock: f / clock * 2^52
    assert_eq!(afg::increment(10e12), (10e6 / 250e6 * 4_503_599_627_370_496.0) as u64);
    // the 0x1210 count goes as the period (1.375 GHz on the unit), the PLL table is the generated 2 GHz one
    assert_eq!(boot::adc_clk_window(), (39_056, 39_069));
    assert_eq!(boot::pll_table().unwrap(), oos_hal::lmx2582::table_for(2e9).unwrap());
    assert_ne!(boot::pll_table().unwrap(), boot::regs::PLL_TABLE);
    // the eye scan's minimum reference run shrinks with the bit period
    assert_eq!(boot::wnd::min_run(), 8);
}
