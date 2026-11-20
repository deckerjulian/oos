// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The core decoders: UART, I2C, SPI, CAN, LIN, Parallel and MIPI C-PHY. Built in until OOS 0.1.12, modules since;
//! their keys are the names of the old setups. Their settings are the shared rows (`Row`): SCPI (`:BUS<n>:RS232` ...),
//! the hardware protocol triggers of the FPGA (UART, SPI, I2C), SIM and the reference decoding use those fields by
//! name, so they stay where they were. They stand first in `MODULES` at fixed places, which `Proto::UART` ... name.

use super::*;

/// Demo traffic on one line: `a` repeated every `period` seconds.
fn one_line(a: oos_dsp::sim::Logic, period: f64) -> Demo {
    Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] }
}

/// The trigger the bus demos start with: the first falling edge of the main line after `idle` seconds of quiet bus.
const fn idle_fall(idle: f32) -> Option<DemoTrig> {
    Some(DemoTrig { role: 0, slope: Slope::Fall, idle, level: 1.65 })
}

pub struct Uart;

impl Protocol for Uart {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::UART
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::Baud, Row::Bits, Row::Parity, Row::Fmt]
    }
    /// The UART line of the demo is CH3 whatever the decoder (the host's default traffic); 8 data bits.
    fn setup(&self) -> Setup {
        Setup { rows: &[(Row::Bits, 8)], ..Setup::default() }
    }
}

pub struct I2c;

impl Protocol for I2c {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::I2C
    }
    /// SDA on CH3, SCL on CH4 (LA: D13, D12); the trigger on the start (first SDA fall after a quiet bus).
    fn setup(&self) -> Setup {
        Setup {
            la: [Some(13), Some(12), None, None],
            trig: Some(DemoTrig { role: 0, slope: Slope::Fall, idle: 20e-6, level: 1.65 }),
            ..Setup::default()
        }
    }
    /// 400 kHz: write 0x50 [00 42], repeated start, read [4F 53] = "OS", every 200 us.
    fn demo(&self) -> Option<Demo> {
        use oos_dsp::sim::{i2c_logic, I2cXfer};
        let (sda, scl) = i2c_logic(
            &[
                I2cXfer { addr: 0x50, read: false, data: vec![0x00, 0x42], repeated: false, stop: false },
                I2cXfer { addr: 0x50, read: true, data: vec![0x4f, 0x53], repeated: true, stop: true },
            ],
            400e3,
            0.0,
            3.3,
        );
        Some(Demo {
            period: 200e-6,
            lines: [Some(Box::new(move |t| sda.v(t))), Some(Box::new(move |t| scl.v(t))), None, None],
        })
    }
}

pub struct Spi;

impl Protocol for Spi {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::SPI
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::SpiMode, Row::Bits]
    }
    /// A frame starts with the chip select.
    fn start_role(&self) -> usize {
        2
    }
    /// MOSI on CH3, clock on CH4, CS on CH2 (LA: D11, D10, D9); mode 0, 8 bits; the trigger on CS falling.
    fn setup(&self) -> Setup {
        Setup {
            src: [Some(2), Some(3), Some(1), None],
            la: [Some(11), Some(10), Some(9), None],
            rows: &[(Row::SpiMode, 0), (Row::Bits, 8)],
            trig: Some(DemoTrig { role: 2, slope: Slope::Fall, idle: 0.0, level: 1.65 }),
            ..Setup::default()
        }
    }
    /// Mode 0, 2 MHz, words 9F/FF 00/1F 00/40 every 50 us.
    fn demo(&self) -> Option<Demo> {
        // `spi_logic`: CS, clock, MOSI, MISO
        let [cs, clk, mosi, _] =
            oos_dsp::sim::spi_logic(&[(0x9f, 0xff), (0x00, 0x1f), (0x00, 0x40)], 8, 0, 2e6, 0.0, 3.3);
        Some(Demo {
            period: 50e-6,
            lines: [
                Some(Box::new(move |t| mosi.v(t))),
                Some(Box::new(move |t| clk.v(t))),
                Some(Box::new(move |t| cs.v(t))),
                None,
            ],
        })
    }
}

pub struct Can;

impl Protocol for Can {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::CAN
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::CanRate, Row::CanH]
    }
    /// 500 kbit/s on CH3 (RX level); the trigger on the start of frame after the bus idle.
    fn setup(&self) -> Setup {
        Setup { rows: &[(Row::CanRate, 2), (Row::CanH, 0)], trig: idle_fall(20e-6), ..Setup::default() }
    }
    /// A standard and an extended data frame every 500 us.
    fn demo(&self) -> Option<Demo> {
        use oos_dsp::sim::{can_logic, CanFrame};
        let l = can_logic(
            &[
                CanFrame { id: 0x123, ext: false, rtr: false, data: vec![0x11, 0x22, 0x33, 0x44] },
                CanFrame { id: 0x18da_f110, ext: true, rtr: false, data: vec![0x02, 0x10, 0x03] },
            ],
            500e3,
            0.0,
            3.3,
        );
        Some(one_line(l, 500e-6))
    }
}

pub struct Lin;

impl Protocol for Lin {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::LIN
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::Baud]
    }
    /// 19200 baud on CH3; the trigger on the break after the bus idle.
    fn setup(&self) -> Setup {
        Setup { rows: &[(Row::Baud, 4)], trig: idle_fall(20e-6), ..Setup::default() }
    }
    /// One frame with enhanced checksum, a 12 V bus scaled to 3.3 V, every 12 ms.
    fn demo(&self) -> Option<Demo> {
        Some(one_line(oos_dsp::sim::lin_logic(&[(0x21, vec![0x10, 0x20, 0x30, 0x40], true)], 19200.0, 0.0, 3.3), 12e-3))
    }
}

pub struct Parallel;

impl Protocol for Parallel {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::PARALLEL
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::Bits]
    }
    /// D0..D2 on CH1..CH3, the clock on CH4, every channel on (LA: D0).
    fn setup(&self) -> Setup {
        Setup {
            src: [Some(0), None, Some(3), None],
            la: [Some(0), None, None, None],
            rows: &[(Row::Bits, 3)],
            show_all: true,
            ..Setup::default()
        }
    }
}

pub struct Cphy;

impl Protocol for Cphy {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::cphy::CPHY
    }
    fn shared(&self) -> &'static [Row] {
        &[Row::CphyRate]
    }
    /// A, B, C on CH1..CH3, the receiver's 20 mV threshold, 16-bit words; the analog channels, not the LA.
    fn setup(&self) -> Setup {
        Setup {
            src: [Some(0), Some(1), Some(2), None],
            thr: 0.02,
            rows: &[(Row::Bits, 16)],
            la_off: true,
            ..Setup::default()
        }
    }
}
