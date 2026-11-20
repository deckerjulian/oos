// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The USB Power Delivery decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

/// header: message type, number of data objects, message ID; PD 3.0, source, DFP
fn pd_hdr(t: u16, ndo: u16, id: u16) -> u16 {
    t | 1 << 5 | 2 << 6 | 1 << 8 | (id & 7) << 9 | (ndo & 7) << 12
}

#[test]
fn usbpd_source_caps_goodcrc_and_hard_reset() {
    let pdo5 = 100 << 10 | 300; // 5 V 3 A
    let pdo9 = 180 << 10 | 300; // 9 V 3 A
    let pps = 3 << 30 | 210 << 17 | 33 << 8 | 60; // PPS 3.3-21 V 3 A
    let frames = [
        PdFrame::Msg { sop: 0, header: pd_hdr(1, 3, 2), data: vec![pdo5, pdo9, pps], bad_crc: false },
        PdFrame::Msg { sop: 1, header: 0x0041, data: vec![], bad_crc: false }, // GoodCRC to SOP'
        PdFrame::HardReset,
        PdFrame::Msg { sop: 0, header: pd_hdr(2, 1, 3), data: vec![0x1304_b12c], bad_crc: true },
    ];
    // a transmitter 7 % fast
    let l = gen::usbpd(&frames, 300e3 * 1.07, 0.0, 1.1);
    let dt = 1.0 / 12e6;
    let mut w = waves(&[&l], dt);
    add_noise(&mut w[0], 0.05, 3);
    let p = usbpd(&w[0], dt, 0.55, 0.2);
    let pre = (F("PRE"), 64, true);
    #[rustfmt::skip]
    let want = vec![
        pre, (F("SOP"), 0, true), (F("HDR"), pd_hdr(1, 3, 2) as u32, true),
        (F("PDO"), pdo5, true), (F("PDO"), pdo9, true), (F("PDO"), pps, true), (Kind::Crc, 0, true), (F("!EOP"), 0, true),
        pre, (F("SOP'"), 0, true), (F("HDR"), 0x41, true), (Kind::Crc, 0, true), (F("!EOP"), 0, true),
        pre, (F("!HRST"), 0, true),
        pre, (F("SOP"), 0, true), (F("HDR"), pd_hdr(2, 1, 3) as u32, true), (F("PDO"), 0x1304_b12c, true), (Kind::Crc, 0, false), (F("!EOP"), 0, true),
    ];
    let got: Vec<(Kind, u32, bool)> =
        p.iter().map(|x| (x.kind, if x.kind == Kind::Crc { 0 } else { x.value }, x.ok)).collect();
    assert_eq!(got, want);
    assert_eq!(usbpd_msg_name(pd_hdr(1, 3, 2)), "Source_Capabilities");
    assert_eq!(usbpd_msg_name(0x0041), "GoodCRC");
    assert_eq!(usbpd_msg_name(pd_hdr(6, 0, 0)), "PS_RDY");
    assert_eq!(usbpd_msg_name(pd_hdr(2, 1, 0)), "Request");
    assert_eq!(usbpd_pdo(pdo9), Pdo::Fixed { volts: 9.0, amps: 3.0 });
    match usbpd_pdo(pps) {
        Pdo::Pps { min_v, max_v, amps } => {
            assert!((min_v - 3.3).abs() < 1e-9 && (max_v - 21.0).abs() < 1e-9 && (amps - 3.0).abs() < 1e-9)
        }
        x => panic!("{:?}", x),
    }
    assert_eq!(usbpd_crc32(b"123456789"), 0xcbf4_3926);
}

#[test]
fn usbpd_corrupted_symbol_and_slow_transmitter() {
    // a 9 % slow transmitter; a corrupted 4b5b symbol in the header -> !ERR
    let f = PdFrame::Msg { sop: 2, header: pd_hdr(3, 0, 1), data: vec![], bad_crc: false };
    let mut bits = gen::usbpd_bits(&f);
    let h = 64 + 20; // first header symbol
    bits[h..h + 5].copy_from_slice(&[false; 5]); // 00000 is not a symbol
    let mut l = Logic::new(0.0, 1.1, 0.1e-6);
    l.push(0.0, false);
    let ui = 0.5 / (300e3 * 0.91);
    let (mut t, mut lv) = (10.0 * ui, false);
    for &b in &bits {
        lv = !lv;
        l.push(t, lv);
        if b {
            lv = !lv;
        }
        l.push(t + ui, lv);
        t += 2.0 * ui;
    }
    l.push(t, false);
    let dt = 1.0 / 10e6;
    let w = waves(&[&l], dt);
    let p = usbpd(&w[0], dt, 0.55, 0.2);
    assert_eq!(kv(&p[..3]), vec![(F("PRE"), 64, true), (F("SOP''"), 0, true), (F("!ERR"), 0, false)]);
}
