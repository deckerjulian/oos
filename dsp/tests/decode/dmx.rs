// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The DMX512 with RDM decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

#[test]
fn dmx512_dimmer_frame_and_rdm() {
    let levels: Vec<u8> = (0..24).map(|i| (i * 11) as u8).collect();
    let get = gen::rdm_packet(0x7a70_1234_5678, 0x4d41_0000_0001, 5, 1, 0, 0, 0x20, 0x0060, &[]);
    let set = gen::rdm_packet(0x7a70_1234_5678, 0x4d41_0000_0001, 6, 1, 0, 3, 0x30, 0x00f0, &[0x00, 0x2a]);
    let mut bad = set.clone();
    let n = bad.len();
    bad[n - 1] ^= 0x01;
    let mut w = Wave::new(0.0, 5.0, 0.1e-6);
    w.level(true, 100e-6);
    gen::dmx(&mut w, 0x00, &levels);
    gen::dmx(&mut w, RDM_SC, &get[1..]);
    gen::dmx(&mut w, RDM_SC, &set[1..]);
    gen::dmx(&mut w, RDM_SC, &bad[1..]);
    let dt = 0.25e-6;
    let mut v = w.done().sample(dt, 100e-6);
    add_noise(&mut v, 0.3, 9);
    let p = dmx512(&v, dt, 2.5, 0.5, false);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Break).count(), 4);
    assert_eq!(vals(&p, "MAB"), [12; 4]);
    assert_eq!(vals(&p, "SC"), [0x00, 0xcc, 0xcc, 0xcc]);
    // dimmer slots: level and slot number
    let slots: Vec<(u32, Option<u32>)> =
        p.iter().filter(|x| x.kind == Kind::Data).take(24).map(|x| (x.value, x.value2)).collect();
    assert_eq!(slots, levels.iter().enumerate().map(|(i, &l)| (l as u32, Some(i as u32 + 1))).collect::<Vec<_>>());
    // RDM fields
    let dst: Vec<(u32, Option<u32>)> =
        p.iter().filter(|x| label(x.kind) == "DST").map(|x| (x.value, x.value2)).collect();
    assert_eq!(dst, [(0x1234_5678, Some(0x7a70)); 3]);
    assert_eq!(
        p.iter().find(|x| label(x.kind) == "SRC").map(|x| (x.value, x.value2)),
        Some((0x0000_0001, Some(0x4d41)))
    );
    assert_eq!(vals(&p, "TN"), [5, 6, 6]);
    assert_eq!(vals(&p, "CC"), [0x20, 0x30, 0x30]);
    assert_eq!(vals(&p, "PID"), [0x0060, 0x00f0, 0x00f0]);
    assert_eq!(vals(&p, "SUBDEV"), [0, 3, 3]);
    assert_eq!(vals(&p, "PDL"), [0, 2, 2]);
    assert_eq!(vals(&p, "LEN"), [24, 26, 26]);
    assert_eq!(
        p.iter().filter(|x| x.kind == Kind::Data).skip(24).map(|x| x.value).collect::<Vec<_>>(),
        [0x00, 0x2a, 0x00, 0x2a]
    );
    assert_eq!(oks(&p, "CRC"), [true, true, false]);
    assert!(p.iter().filter(|x| label(x.kind) != "CRC").all(|x| x.ok), "{:?}", lvo(&p));
}

#[test]
fn dmx512_short_break_and_bad_stop_bits() {
    // a 60 us "break" is no break; a slot with a low second stop bit is flagged
    let mut w = Wave::new(0.0, 5.0, 0.1e-6);
    w.level(true, 50e-6);
    w.level(false, 60e-6);
    w.level(true, 50e-6);
    w.level(false, 100e-6);
    w.level(true, 12e-6);
    w.uart(0x00, 8, Parity::None, 2, DMX_BAUD);
    w.uart(0x7f, 8, Parity::None, 1, DMX_BAUD); // one stop bit only, next start bit follows at once
    w.uart(0x01, 8, Parity::None, 2, DMX_BAUD);
    let dt = 0.25e-6;
    let p = dmx512(&w.done().sample(dt, 50e-6), dt, 2.5, 0.5, false);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Break).count(), 1);
    assert_eq!(
        p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect::<Vec<_>>(),
        [(0x7f, false), (0x01, true)]
    );
}
