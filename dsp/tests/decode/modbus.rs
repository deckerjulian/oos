// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Modbus RTU and ASCII decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

#[test]
fn modbus_rtu_frames() {
    assert_eq!(modbus_crc(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0a]), 0xcdc5);
    let baud = 19200.0;
    let frames = [
        gen::modbus_rtu_frame(0x11, &[0x03, 0x00, 0x6b, 0x00, 0x03]),
        gen::modbus_rtu_frame(0x11, &[0x03, 0x06, 0x02, 0x2b, 0x00, 0x00, 0x00, 0x64]),
        gen::modbus_rtu_frame(0x11, &[0x83, 0x02]),
        gen::modbus_rtu_frame(0x01, &[0x10, 0x00, 0x01, 0x00, 0x02, 0x04, 0x00, 0x0a, 0x01, 0x02]),
        gen::modbus_rtu_frame(0x01, &[0x10, 0x00, 0x01, 0x00, 0x02]),
        gen::modbus_rtu_frame(0x07, &[0x2b, 0x0e, 0x01, 0x00]),
    ];
    let mut bad = gen::modbus_rtu_frame(0x11, &[0x06, 0x00, 0x01, 0x00, 0x03]);
    bad[3] ^= 0x10;
    let mut w = Wave::new(0.0, 3.3, 0.02 / baud);
    w.level(true, 50.0 / baud);
    for f in frames.iter().chain([&bad]) {
        for &b in f {
            w.uart(b as u32, 8, Parity::Even, 1, baud);
        }
        w.level(true, 5.0 * 11.0 / baud);
    }
    let dt = 1.0 / baud / 16.0;
    let mut v = w.done().sample(dt, 20.0 / baud);
    add_noise(&mut v, 0.2, 4);
    let p = modbus_rtu(&v, dt, baud, Parity::Even, 1.65, 0.4, false);
    let want = vec![
        e("A", 0x11, true),
        e("FUNC", 3, true),
        e("REG", 0x6b, true),
        e("QTY", 3, true),
        e("CRC", 0x8776, true), // "76 87" on the wire (spec example)
        e("A", 0x11, true),
        e("FUNC", 3, true),
        e("CNT", 6, true),
        e("D", 0x022b, true),
        e("D", 0, true),
        e("D", 0x64, true),
        e("CRC", modbus_crc(&frames[1][..9]) as u32, true),
        e("A", 0x11, true),
        e("FUNC", 0x83, true),
        e("EXC", 2, true),
        e("CRC", modbus_crc(&frames[2][..3]) as u32, true),
        e("A", 1, true),
        e("FUNC", 0x10, true),
        e("REG", 1, true),
        e("QTY", 2, true),
        e("CNT", 4, true),
        e("D", 0x000a, true),
        e("D", 0x0102, true),
        e("CRC", modbus_crc(&frames[3][..11]) as u32, true),
        e("A", 1, true),
        e("FUNC", 0x10, true),
        e("REG", 1, true),
        e("QTY", 2, true),
        e("CRC", modbus_crc(&frames[4][..6]) as u32, true),
        e("A", 7, true),
        e("FUNC", 0x2b, true),
        e("D", 0x0e, true),
        e("D", 0x01, true),
        e("D", 0x00, true),
        e("CRC", modbus_crc(&frames[5][..5]) as u32, true),
        e("A", 0x11, true),
        e("FUNC", 6, true),
        e("REG", 0x11, true),
        e("D", 3, true),
        e("CRC", bad[6] as u32 | (bad[7] as u32) << 8, false),
    ];
    assert_eq!(lvo(&p), want);
    // wrong parity setting: framing/parity errors make every frame fail
    assert!(oks(&modbus_rtu(&v, dt, baud, Parity::Odd, 1.65, 0.4, false), "CRC").iter().all(|&ok| !ok));
}

#[test]
fn modbus_ascii_frames() {
    assert_eq!(gen::modbus_ascii_frame(0x11, &[0x03, 0x00, 0x6b, 0x00, 0x03]), b":1103006B00037E\r\n");
    let baud = 9600.0;
    let good = gen::modbus_ascii_frame(0x11, &[0x03, 0x00, 0x6b, 0x00, 0x03]);
    let resp = gen::modbus_ascii_frame(0x11, &[0x03, 0x02, 0xab, 0xcd]);
    let mut bad = gen::modbus_ascii_frame(0x0a, &[0x05, 0x00, 0x10, 0xff, 0x00]);
    bad[6] = b'9'; // register "0010" -> "0910"
    let junk = b":12G4\r\n".to_vec();
    let mut w = Wave::new(0.0, 3.3, 0.02 / baud);
    w.level(true, 20.0 / baud);
    for f in [&good, &resp, &bad, &junk] {
        for &c in f.iter() {
            w.uart(c as u32, 7, Parity::Even, 1, baud);
        }
        w.level(true, 30.0 / baud);
    }
    let dt = 1.0 / baud / 10.0;
    let p = modbus_ascii(&w.done().sample(dt, 20.0 / baud), dt, baud, 7, Parity::Even, 1.65, 0.4, false);
    let want = vec![
        e("S", 0, true),
        e("A", 0x11, true),
        e("FUNC", 3, true),
        e("REG", 0x6b, true),
        e("QTY", 3, true),
        e("CRC", 0x7e, true),
        e("P", 0, true),
        e("S", 0, true),
        e("A", 0x11, true),
        e("FUNC", 3, true),
        e("CNT", 2, true),
        e("D", 0xabcd, true),
        e("CRC", modbus_lrc(&[0x11, 0x03, 0x02, 0xab, 0xcd]) as u32, true),
        e("P", 0, true),
        e("S", 0, true),
        e("A", 0x0a, true),
        e("FUNC", 5, true),
        e("REG", 0x0910, true),
        e("D", 0xff00, true),
        e("CRC", modbus_lrc(&[0x0a, 0x05, 0x00, 0x10, 0xff, 0x00]) as u32, false),
        e("P", 0, true),
        e("S", 0, true),
        e("!ERR", 0, false),
        e("P", 0, true),
    ];
    assert_eq!(lvo(&p), want);
}
