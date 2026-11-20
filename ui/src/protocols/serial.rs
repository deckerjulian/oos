// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! UART variants on one line (or an RS-485 pair): 9-bit UART (the ninth bit marks an address), Modbus RTU (binary
//! frames, CRC-16, split by the 3.5 character pause) and Modbus ASCII (':' ... LRC CR LF). Built in until OOS 0.1.12,
//! modules since; their keys are the names of the old setups, and a setup that kept the baud rate, parity and data bits
//! in the shared rows brings them along (`legacy`). Checked against generated frames.

use super::*;
use oos_dsp::decode;
use oos_dsp::decode::Parity;

/// Index of `p` in `decode::PARITIES`.
fn parity_index(p: Parity) -> usize {
    decode::PARITIES.iter().position(|&x| x == p).unwrap_or(0)
}

/// Demo traffic: the words as 3.3 V UART frames with one stop bit, five idle bits around them, then `idle` seconds.
fn uart_demo(words: &[u32], bits: u32, parity: Parity, baud: f64, idle: f64) -> Demo {
    let mut w = decode::gen::Wave::new(0.0, 3.3, 20e-9);
    w.level(true, 5.0 / baud);
    for &b in words {
        w.uart(b, bits, parity, 1, baud);
    }
    w.level(true, 5.0 / baud);
    let a = w.done();
    let period = a.seg.last().map_or(1e-3, |s| s.0) + idle;
    Demo { period, lines: [Some(Box::new(move |t| a.v(t))), None, None, None] }
}

pub struct Uart9;

const UART9_OPTIONS: [Opt; 1] = [Opt { name: "Baud", choices: &BAUD_NAMES, default: 3 }];

impl Protocol for Uart9 {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::UART9
    }
    fn options(&self) -> &'static [Opt] {
        &UART9_OPTIONS
    }
    /// An address word and two data words at 9600 baud.
    fn demo(&self) -> Option<Demo> {
        Some(uart_demo(&[0x1A5, 0x0C3, 0x07E], 9, Parity::None, 9600.0, 1e-3))
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.baud, 0, 0, 0])
    }
}

pub struct ModbusRtu;

const PARITY_NAMES: [&str; 3] = ["None", "Even", "Odd"];

const RTU_OPTIONS: [Opt; 2] = [
    Opt { name: "Baud", choices: &BAUD_NAMES, default: 4 },
    Opt { name: "Parity", choices: &PARITY_NAMES, default: 1 },
];

impl Protocol for ModbusRtu {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::MODBUS_RTU
    }
    fn options(&self) -> &'static [Opt] {
        &RTU_OPTIONS
    }
    /// Read holding registers (function 3) of device 0x11, 19200 baud, even parity.
    fn demo(&self) -> Option<Demo> {
        let f: Vec<u32> =
            decode::gen::modbus_rtu_frame(0x11, &[0x03, 0x00, 0x6B, 0x00, 0x03]).into_iter().map(u32::from).collect();
        Some(uart_demo(&f, 8, Parity::Even, 19200.0, 2e-3))
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.baud, parity_index(old.parity), 0, 0])
    }
}

pub struct ModbusAscii;

const ASCII_BITS: [&str; 2] = ["7", "8"];
/// Modbus ASCII always has a parity bit (the old shared row's "None" decoded as even).
const ASCII_PARITY: [&str; 2] = ["Even", "Odd"];

const ASCII_OPTIONS: [Opt; 3] = [
    Opt { name: "Baud", choices: &BAUD_NAMES, default: BAUD_DEFAULT },
    Opt { name: "Data bits", choices: &ASCII_BITS, default: 0 },
    Opt { name: "Parity", choices: &ASCII_PARITY, default: 0 },
];

impl Protocol for ModbusAscii {
    fn info(&self) -> &'static ModuleInfo {
        &oos_dsp::decode::MODBUS_ASCII
    }
    fn options(&self) -> &'static [Opt] {
        &ASCII_OPTIONS
    }
    fn legacy(&self, old: &Legacy) -> Option<[usize; OPTS]> {
        Some([old.baud, old.bits.clamp(7, 8) as usize - 7, usize::from(old.parity == Parity::Odd), 0])
    }
}

#[cfg(test)]
mod tests {
    // Break it catches: a setup written while these were built in losing its protocol, baud rate, parity or word size
    #[test]
    fn old_setups_keep_protocol_and_options() {
        let load = |text: &str| crate::store::parse_setup(&format!("dec_on = true\n{}", text)).unwrap();
        let m = load("dec_proto = Uart9\ndec_baud = 5\n");
        assert_eq!((m.dec_proto.name(), m.dec_opts()[0]), ("UART 9-bit", 5));
        assert_eq!(m.bus_bit(), 1.0 / 38400.0);
        let m = load("dec_proto = ModbusRtu\ndec_baud = 4\ndec_parity = Odd\n");
        assert_eq!((m.dec_proto.name(), m.dec_opts()[..2].to_vec()), ("Modbus RTU", vec![4, 2]));
        let m = load("dec_proto = ModbusAscii\ndec_baud = 3\ndec_bits = 8\ndec_parity = None\n");
        assert_eq!((m.dec_proto.name(), m.dec_opts()[..3].to_vec()), ("Modbus ASCII", vec![3, 1, 0]));
        assert!(m.dec_on);
    }
}
