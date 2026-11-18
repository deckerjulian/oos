// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The decoders through their modules: `ModuleInfo::decode` of the entry in `oos_dsp::protocols::MODULES`, with
//! option rows and shared rows that are not the first choice, on a signal made for exactly those settings. The tests
//! of the families call the decoders themselves; these show that each module hands its settings to its decoder.

use super::*;
use oos_dsp::protocols::{Lines, Shared, MODULES, OPTS};

/// The channels CH1..CH4 of a capture and the channel of each role of the module.
struct Capture {
    chans: Vec<Vec<f32>>,
    src: [Option<usize>; 4],
    dt: f64,
    thr: f32,
    hy: f32,
}

/// One module with settings that are not the first choice, a signal made for them and what it must decode to.
struct Case {
    key: &'static str,
    opt: [usize; OPTS],
    sh: fn(&mut Shared),
    capture: fn() -> Capture,
    /// the packets of these labels, in order, with value and check
    want: Vec<(String, u32, bool)>,
}

/// The shared rows a case starts from: 9600 8N1, SPI mode 0, CAN at 500 kbit/s on RX, USB auto.
fn base() -> Shared {
    Shared {
        baud: 9600.0,
        bits: 8,
        parity: Parity::None,
        spi_mode: 0,
        can_rate: 500e3,
        canh: false,
        usb_speed: UsbSpeed::Auto,
        cphy_rate: 0.0,
        uart_inv: false,
        spi_timeout: 0.0,
        spi_cs_high: false,
    }
}

/// UART characters on one line from 0 V to 3.3 V (`lo` and `hi` swapped: an inverted line), sampled at a twentieth
/// of a bit.
fn uart_line(chars: &[u8], bits: u32, parity: Parity, baud: f64, lo: f64, hi: f64) -> Capture {
    let mut w = Wave::new(lo, hi, 0.02 / baud);
    w.level(true, 20.0 / baud);
    for &c in chars {
        w.uart(c as u32, bits, parity, 1, baud);
    }
    let dt = 1.0 / baud / 20.0;
    Capture { chans: vec![w.done().sample(dt, 20.0 / baud)], src: [Some(0), None, None, None], dt, thr: 1.65, hy: 0.3 }
}

/// The module of `key` decodes the capture with these settings: the packets with the labels of `want`.
fn decoded(
    key: &str,
    c: &Capture,
    opt: [usize; OPTS],
    sh: Shared,
    want: &[(String, u32, bool)],
) -> Vec<(String, u32, bool)> {
    let m = MODULES.iter().find(|m| m.key == key).unwrap_or_else(|| panic!("{}: no module", key));
    let mut chans = [None; 4];
    for (k, v) in c.chans.iter().enumerate() {
        chans[k] = Some(v.as_slice());
    }
    let l = Lines {
        lines: c.src.map(|s| s.and_then(|s| chans[s])),
        dt: c.dt,
        thr: c.thr,
        hy: c.hy,
        opt,
        sh,
        chans,
        src: c.src,
    };
    let p = m.decode(&l).unwrap_or_else(|e| panic!("{}: {}", key, e));
    lvo(&p).into_iter().filter(|x| want.iter().any(|w| w.0 == x.0)).collect()
}

/// Each case decodes to what it wants only with its settings: with the first choice of every option row and the
/// shared rows of `base` the same signal decodes to something else, so the case depends on the settings it makes.
///
/// How the cases were checked: each part of the mappings in the modules was broken in turn, and the case of that
/// module failed every time (UART: baud, bits, parity and the inverted line each replaced by its value in `base`,
/// Odd read as Even; CAN-FD: the two rate rows swapped, the CAN-H row ignored; Modbus ASCII: the bits row ignored, its
/// parity row read the other way round, the baud row read from row 1; Modbus RTU: `PARITIES` read with row 2, the
/// baud row read from row 1; Manchester: the baud row read from row 1, or 0 (the rate from the edges) instead of it;
/// Parallel: the Bits row ignored, the data lines started at CH1 instead of the D0 source, the clock role ignored).
#[test]
fn every_module_hands_its_settings_to_its_decoder() {
    let cases = [
        Case {
            // 7 bits, odd parity, 19200 Bd, inverted: 0x07 has a parity bit 0 (a missing parity row reads it as a
            // stop bit and fails), 0x41 and 0x5a a parity bit 1 (8 bits read it as bit 7)
            key: "Uart",
            opt: [0; OPTS],
            sh: |s| (s.baud, s.bits, s.parity, s.uart_inv) = (19200.0, 7, Parity::Odd, true),
            capture: || uart_line(&[0x07, 0x41, 0x5a], 7, Parity::Odd, 19200.0, 3.3, 0.0),
            want: vec![e("D", 0x07, true), e("D", 0x41, true), e("D", 0x5a, true)],
        },
        Case {
            // 250 kbit/s nominal (row 0: 1), 4 Mbit/s data (row 1: 2), CAN-H (row 2: 1, the line inverted)
            key: "CanFd",
            opt: [1, 2, 1, 0],
            sh: |_| {},
            capture: || {
                let f = FdFrame { id: 0x123, ext: false, fd: true, brs: true, esi: false, data: (1..=12).collect() };
                let dt = 1.0 / 4e6 / 20.0;
                let v = gen::can_fd_logic(&[f], 250e3, 4e6, 3.5, 2.5).sample(dt, 20.0 / 250e3);
                Capture { chans: vec![v], src: [Some(0), None, None, None], dt, thr: 3.0, hy: 0.2 }
            },
            want: std::iter::once(e("ID", 0x123, true)).chain((1..=12).map(|d| e("D", d, true))).collect(),
        },
        Case {
            // 19200 Bd (row 0: 4), 8 bits (row 1: 1), odd parity (row 2: 1)
            key: "ModbusAscii",
            opt: [4, 1, 1, 0],
            sh: |_| {},
            capture: || {
                uart_line(
                    &gen::modbus_ascii_frame(0x11, &[0x03, 0x00, 0x6b, 0x00, 0x03]),
                    8,
                    Parity::Odd,
                    19200.0,
                    0.0,
                    3.3,
                )
            },
            want: vec![
                e("S", 0, true),
                e("A", 0x11, true),
                e("FUNC", 3, true),
                e("REG", 0x6b, true),
                e("QTY", 3, true),
                e("CRC", 0x7e, true),
                e("P", 0, true),
            ],
        },
        Case {
            // 38400 Bd (row 0: 5), odd parity (row 1: 2)
            key: "ModbusRtu",
            opt: [5, 2, 0, 0],
            sh: |_| {},
            capture: || {
                uart_line(
                    &gen::modbus_rtu_frame(0x11, &[0x03, 0x00, 0x6b, 0x00, 0x03]),
                    8,
                    Parity::Odd,
                    38400.0,
                    0.0,
                    3.3,
                )
            },
            want: vec![
                e("A", 0x11, true),
                e("FUNC", 3, true),
                e("REG", 0x6b, true),
                e("QTY", 3, true),
                e("CRC", 0x8776, true),
            ],
        },
        Case {
            // no demo traffic in the ui: 57600 bit/s (row 0: 6), IEEE 802.3 cells (a 1 is low, then high)
            key: "Manchester",
            opt: [6, 0, 0, 0],
            sh: |_| {},
            capture: || {
                let half = 0.5 / 57600.0;
                let cells: Vec<bool> = [0xd5u8, 0x3c]
                    .iter()
                    .flat_map(|&b| (0..8).flat_map(move |k| [b >> k & 1 == 0, b >> k & 1 == 1]))
                    .collect();
                let dt = half / 20.0;
                let v = bits_logic(&cells, half, 8, 0.0, 3.3).sample(dt, 8.0 * half);
                Capture { chans: vec![v], src: [Some(0), None, None, None], dt, thr: 1.65, hy: 0.3 }
            },
            want: vec![e("D", 0xd5, true), e("D", 0x3c, true)],
        },
        Case {
            // no demo traffic in the ui: Bits 2 (shared), D0 on CH3 so the data lines are CH3 and CH4, the clock on
            // CH1, CH2 high (a bus of more lines, or one started at CH1, reads it)
            key: "Parallel",
            opt: [0; OPTS],
            sh: |s| s.bits = 2,
            capture: || {
                let (words, t) = ([2u32, 1, 3, 0, 2], 1e-6);
                let line = |k: u32| -> Vec<bool> { words.iter().map(|w| w >> k & 1 == 1).collect() };
                // rising in the middle of each word, high at the end of the last one
                let clk: Vec<bool> = words.iter().flat_map(|_| [false, true]).collect();
                let dt = t / 50.0;
                let mut chans = vec![
                    bits_logic(&clk, t / 2.0, 2, 0.0, 3.3).sample(dt, t),
                    Vec::new(),
                    bits_logic(&line(0), t, 1, 0.0, 3.3).sample(dt, t),
                    bits_logic(&line(1), t, 1, 0.0, 3.3).sample(dt, t),
                ];
                chans[1] = vec![3.3; chans[0].len()];
                Capture { chans, src: [Some(2), None, Some(0), None], dt, thr: 1.65, hy: 0.3 }
            },
            want: [2, 1, 3, 0, 2].map(|w| e("D", w, true)).to_vec(),
        },
    ];
    for c in &cases {
        let cap = (c.capture)();
        let mut sh = base();
        (c.sh)(&mut sh);
        assert_eq!(decoded(c.key, &cap, c.opt, sh, &c.want), c.want, "{}: with its settings", c.key);
        let first = decoded(c.key, &cap, [0; OPTS], base(), &c.want);
        assert_ne!(
            first, c.want,
            "{}: the signal decodes the same with the first choices, the case shows nothing",
            c.key
        );
    }
}
