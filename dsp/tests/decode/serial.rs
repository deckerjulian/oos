// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The UART auto baud and 9-bit multidrop / RS-485 decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

#[test]
fn uart_baud_detects_standard_and_odd_rates() {
    let mut r = rng(11);
    for &rate in &[1200.0, 9600.0, 19200.0, 38400.0, 115200.0, 250000.0, 921600.0, 1e6, 3e6] {
        let bytes: Vec<u8> = (0..40).map(|_| r() as u8).collect();
        let l = oos_dsp::sim::uart_logic(&bytes, rate, 8, None, 0.5, 0.0, 3.3);
        let dt = 1.0 / rate / 13.7;
        let mut v = l.sample(dt, 5.0 / rate);
        add_noise(&mut v, 0.1, 5);
        assert_eq!(uart_baud(&v, dt, 1.65, 0.4), Some(rate), "rate {}", rate);
    }
    // not a standard rate: the raw estimate
    let rate = 100_000.0;
    let bytes: Vec<u8> = (0..30).map(|_| r() as u8).collect();
    let l = oos_dsp::sim::uart_logic(&bytes, rate, 8, Some(true), 1.0, 0.0, 5.0);
    let dt = 1.0 / rate / 20.0;
    let est = uart_baud(&l.sample(dt, 5.0 / rate), dt, 2.5, 0.4).unwrap();
    assert!((est - rate).abs() / rate < 0.005, "{}", est);
    // a lone glitch does not count as the bit time
    let mut v = oos_dsp::sim::uart_logic(b"Hello, world", 9600.0, 8, None, 1.0, 0.0, 3.3).sample(1e-6, 1e-3);
    for x in &mut v[3..5] {
        *x = 0.0;
    }
    assert_eq!(uart_baud(&v, 1e-6, 1.65, 0.4), Some(9600.0));
    assert_eq!(uart_baud(&[0.0; 100], 1e-6, 1.65, 0.4), None);
}

#[test]
fn uart9_multidrop_on_rs485_pair() {
    let baud = 115200.0;
    let words = [0x101u32, 0x22, 0x33, 0x1fe, 0x44, 0x00];
    let mut w = Wave::new(-1.0, 1.0, 0.02 / baud);
    w.level(true, 10.0 / baud);
    for &x in &words {
        w.uart(x, 9, Parity::None, 1, baud);
        w.level(true, 1.5 / baud);
    }
    let l = w.done();
    let dt = 1.0 / baud / 16.0;
    let d = l.sample(dt, 5.0 / baud);
    // A = 2.5 + d, B = 2.5 - d, 2 V common mode hum and independent noise on both wires
    let hum = |i: usize| 2.0 * (i as f32 * 0.003).sin();
    let mut a: Vec<f32> = d.iter().enumerate().map(|(i, x)| 2.5 + x + hum(i)).collect();
    let mut b: Vec<f32> = d.iter().enumerate().map(|(i, x)| 2.5 - x + hum(i)).collect();
    add_noise(&mut a, 0.15, 1);
    add_noise(&mut b, 0.15, 2);
    let pair = diff(&a, &b);
    let p = uart9(&pair, dt, baud, 0.0, 0.4, false);
    let got: Vec<(bool, u32, Option<u32>, bool)> =
        p.iter().map(|x| (matches!(x.kind, Kind::Addr { .. }), x.value, x.value2, x.ok)).collect();
    assert_eq!(
        got,
        [
            (true, 0x01, Some(1), true),
            (false, 0x22, Some(0), true),
            (false, 0x33, Some(0), true),
            (true, 0xfe, Some(1), true),
            (false, 0x44, Some(0), true),
            (false, 0x00, Some(0), true)
        ]
    );
    // the protocol trigger: every data word starts a frame; an address word is an ID
    let frames: Vec<u32> = p.iter().filter(|x| UART9.trigger_match(BusWhat::Frame, 0, x)).map(|x| x.value).collect();
    assert_eq!(frames, [0x22, 0x33, 0x44, 0x00]);
    assert_eq!(p.iter().filter(|x| UART9.trigger_match(BusWhat::Id, 0xfe, x)).count(), 1);
    // the single wire A against ground does not decode through the hum, the pair does
    assert_ne!(
        uart9(&a, dt, baud, 2.5, 0.4, false).iter().map(|x| x.value).collect::<Vec<_>>(),
        p.iter().map(|x| x.value).collect::<Vec<_>>()
    );
    // swapped wires = the other polarity convention
    assert_eq!(uart9(&diff(&b, &a), dt, baud, 0.0, 0.4, true).len(), words.len());
}
