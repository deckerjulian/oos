// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The I3C SDR decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

#[test]
fn i3c_daa_ccc_private_i2c_ibi_and_hdr() {
    let mut b = I3cBus::new(1e6, 0.0, 1.8);
    // RSTDAA
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x06, false);
    b.stop();
    // ENTDAA: one target, then nobody answers
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x07, false);
    b.start();
    b.addr(0x7e, true, true);
    b.bits(0x0123_4567_89ab, 48);
    b.bits(0x27, 8);
    b.bits(0x44, 8);
    let da = 0x08u8;
    b.bits((da as u64) << 1 | da.count_ones().is_multiple_of(2) as u64, 8);
    b.bit(false); // ACK
    b.start();
    b.addr(0x7e, true, false);
    b.stop();
    // private write and read with T-bits
    b.start();
    b.addr(0x7e, false, true);
    b.start();
    b.addr(da, false, true);
    b.byte_t(0x55, false);
    b.byte_t(0xaa, true); // wrong T-bit
    b.start();
    b.addr(da, true, true);
    b.read_t(0x11, true);
    b.read_t(0x22, false);
    b.stop();
    // legacy I2C device
    b.start();
    b.addr(0x50, false, true);
    b.byte_ack(0x00, true);
    b.start();
    b.addr(0x50, true, true);
    b.byte_ack(0xcd, false);
    b.stop();
    // SETDASA: static address 0x51 -> dynamic 0x09
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x87, false);
    b.start();
    b.addr(0x51, false, true);
    b.byte_t(0x09 << 1, false);
    b.stop();
    // IBI from 0x08 with one mandatory byte
    b.start();
    b.addr(da, true, true);
    b.read_t(0xae, false);
    b.stop();
    // HDR
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x20, false);
    b.hdr_junk(60);
    b.hdr_exit();
    // I2C after HDR, to the new address 0x09 (now I3C: T-bit)
    b.start();
    b.addr(0x09, false, true);
    b.byte_t(0x3c, false);
    b.stop();
    let dt = 1.0 / 40e6;
    let w = waves(&[&b.sda, &b.scl], dt);
    let p = i3c(&w[0], &w[1], &[], 0.9, 0.2);
    let a = |v: u32, read: bool, ok: bool| (Kind::Addr { read }, v, ok);
    let (st, sr, sp) = ((Kind::Start, 0, true), (Kind::RepStart, 0, true), (Kind::Stop, 0, true));
    let d = |v: u32, ok: bool| (Kind::Data, v, ok);
    #[rustfmt::skip]
    let want = vec![
        st, a(0x7e, false, true), (F("CCC"), 0x06, true), sp,
        st, a(0x7e, false, true), (F("CCC"), 0x07, true), sr, a(0x7e, true, true),
        (F("PID"), 0x4567_89ab, true), (F("BCR"), 0x27, true), (F("DCR"), 0x44, true), (F("DA"), 0x08, true),
        sr, a(0x7e, true, false), sp,
        st, a(0x7e, false, true), sr, a(0x08, false, true), d(0x55, true), d(0xaa, false),
        sr, a(0x08, true, true), d(0x11, true), d(0x22, true), (F("!EOD"), 0, true), sp,
        st, a(0x50, false, true), d(0x00, true), sr, a(0x50, true, true), d(0xcd, false), sp,
        st, a(0x7e, false, true), (F("CCC"), 0x87, true), sr, a(0x51, false, true), (F("DA"), 0x09, true), sp,
        st, (F("IBI"), 0x08, true), d(0xae, true), (F("!EOD"), 0, true), sp,
        st, a(0x7e, false, true), (F("CCC"), 0x20, true), (F("!HDR"), 0, true), (F("!HDREXIT"), 0, true), sp,
        st, a(0x09, false, true), d(0x3c, true), sp,
    ];
    assert_eq!(kv(&p), want);
    let pid = p.iter().find(|x| x.kind == F("PID")).unwrap();
    assert_eq!(pid.value2, Some(0x0123));
    assert_eq!(i3c_ccc_name(0x07), "ENTDAA");
    assert_eq!(i3c_ccc_name(0x8d), "GETPID");
}

#[test]
fn i3c_bad_parity_and_known_addresses() {
    let mut b = I3cBus::new(4e6, 0.0, 3.3);
    b.start();
    b.addr(0x7e, false, true);
    b.byte_t(0x01, true); // DISEC with a wrong T-bit
    b.byte_t(0x0b, false);
    b.stop();
    // 0x30 is unknown in the capture: decoded as I2C unless listed
    b.start();
    b.addr(0x30, false, true);
    b.byte_t(0x81, false); // two ones -> T = 1
    b.stop();
    let dt = 1.0 / 100e6;
    let mut w = waves(&[&b.sda, &b.scl], dt);
    add_noise(&mut w[0], 0.1, 9);
    let p = i3c(&w[0], &w[1], &[], 1.65, 0.4);
    let ccc: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == F("CCC")).map(|x| (x.value, x.ok)).collect();
    assert_eq!(ccc, vec![(0x01, false)]);
    let data: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect();
    // as I2C the T-bit (1) is a NACK
    assert_eq!(data, vec![(0x0b, true), (0x81, false)]);
    let p = i3c(&w[0], &w[1], &[0x30], 1.65, 0.4);
    let data: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect();
    assert_eq!(data, vec![(0x0b, true), (0x81, true)]);
}
