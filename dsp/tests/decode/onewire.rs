// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The 1-Wire, on the bus line and through a DS2482 I2C bridge decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

fn rom_with_crc(b7: [u8; 7]) -> [u8; 8] {
    let mut r = [0; 8];
    r[..7].copy_from_slice(&b7);
    r[7] = ow_crc8(&b7);
    r
}

fn ser(r: &[u8; 8]) -> (u32, u32) {
    (u32::from_le_bytes([r[1], r[2], r[3], r[4]]), r[5] as u32 | (r[6] as u32) << 8)
}

#[test]
fn onewire_read_rom_and_functions() {
    assert_eq!(ow_crc8(&[0x02, 0x1c, 0xb8, 0x01, 0x00, 0x00, 0x00]), 0xa2); // AN27 example
    let rom = rom_with_crc([0x28, 0xff, 0x4c, 0x1a, 0x93, 0x16, 0x04]);
    let mut ops = vec![Ow::Reset { presence: true }, Ow::Write(0x33)];
    ops.extend(rom.iter().map(|&b| Ow::Read(b)));
    ops.extend([Ow::Reset { presence: true }, Ow::Write(0xcc), Ow::Write(0x44)]);
    ops.extend([Ow::Reset { presence: true }, Ow::Write(0xcc), Ow::Write(0xbe), Ow::Read(0x50), Ow::Read(0x05)]);
    let l = gen::onewire_logic(&ops, 0.0, 3.3);
    let dt = 0.5e-6;
    let mut v = l.sample(dt, 100e-6);
    add_noise(&mut v, 0.2, 3);
    let p = onewire(&v, dt, 1.65, 0.6, false);
    let (s_lo, s_hi) = ser(&rom);
    assert_eq!(
        lvo(&p),
        vec![
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0x33, true),
            e("FAM", 0x28, true),
            e("SER", s_lo, true),
            e("CRC", rom[7] as u32, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xcc, true),
            e("D", 0x44, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xcc, true),
            e("D", 0xbe, true),
            e("D", 0x50, true),
            e("D", 0x05, true),
        ]
    );
    assert_eq!(p[4].value2, Some(s_hi));
}

#[test]
fn onewire_search_rom_and_errors() {
    let rom = rom_with_crc([0x10, 0x8a, 0x3b, 0x55, 0x01, 0x08, 0x00]);
    let mut ops = vec![Ow::Reset { presence: true }, Ow::Write(0xf0)];
    for i in 0..64 {
        let b = (rom[i / 8] >> (i % 8)) & 1 == 1;
        ops.extend([Ow::ReadBit(b), Ow::ReadBit(!b), Ow::WriteBit(b)]);
    }
    // nobody answers the next search; then a reset without presence
    ops.extend([
        Ow::Reset { presence: true },
        Ow::Write(0xec),
        Ow::ReadBit(true),
        Ow::ReadBit(true),
        Ow::WriteBit(true),
    ]);
    ops.push(Ow::Reset { presence: false });
    // READ ROM with a corrupted CRC
    let mut bad = rom;
    bad[3] ^= 0x04;
    ops.extend([Ow::Reset { presence: true }, Ow::Write(0x33)]);
    ops.extend(bad.iter().map(|&b| Ow::Read(b)));
    let mut w = Wave::new(0.0, 5.0, 0.05e-6);
    // an invalid 200 us low pulse first
    w.level(true, 50e-6);
    w.level(false, 200e-6);
    w.level(true, 100e-6);
    let mut l = w.done();
    let t0 = l.end() + 50e-6;
    for (t, b) in gen::onewire_logic(&ops, 0.0, 5.0).seg {
        l.push(t0 + t, b);
    }
    let dt = 0.25e-6;
    let p = onewire(&l.sample(dt, 50e-6), dt, 2.5, 0.5, false);
    let (s_lo, _) = ser(&rom);
    let (b_lo, _) = ser(&bad);
    assert_eq!(
        lvo(&p),
        vec![
            e("!ERR", 0, false),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xf0, true),
            e("FAM", 0x10, true),
            e("SER", s_lo, true),
            e("CRC", rom[7] as u32, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xec, true),
            e("!ERR", 0, false),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, false),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0x33, true),
            e("FAM", 0x10, true),
            e("SER", b_lo, true),
            e("CRC", rom[7] as u32, false),
        ]
    );
}

#[test]
fn onewire_overdrive_switch() {
    let rom = rom_with_crc([0x3a, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    let mut ops =
        vec![Ow::Reset { presence: true }, Ow::Write(0x3c), Ow::Speed(true), Ow::Write(0x4e), Ow::Write(0x81)];
    ops.extend([Ow::Reset { presence: true }, Ow::Write(0x69)]);
    ops.extend(rom.iter().map(|&b| Ow::Write(b)));
    ops.extend([Ow::Write(0xbe), Ow::Read(0x12), Ow::Speed(false), Ow::Reset { presence: true }, Ow::Write(0xcc)]);
    let l = gen::onewire_logic(&ops, 0.0, 3.3);
    let dt = 0.1e-6;
    let mut v = l.sample(dt, 100e-6);
    add_noise(&mut v, 0.1, 8);
    let p = onewire(&v, dt, 1.65, 0.4, false);
    let (s_lo, _) = ser(&rom);
    assert_eq!(
        lvo(&p),
        vec![
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0x3c, true),
            e("D", 0x4e, true),
            e("D", 0x81, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0x69, true),
            e("FAM", 0x3a, true),
            e("SER", s_lo, true),
            e("CRC", rom[7] as u32, true),
            e("D", 0xbe, true),
            e("D", 0x12, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xcc, true),
        ]
    );
    // starting in overdrive: the first (standard, 480 us) reset switches back to standard speed, same result
    assert_eq!(onewire(&v, dt, 1.65, 0.4, true), p);
}

#[test]
fn ds2482_bridge_to_onewire() {
    let rom = rom_with_crc([0x28, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    let w = |data: &[u8]| I2cXfer { addr: 0x18, read: false, data: data.to_vec(), repeated: false, stop: true };
    let r = |data: &[u8]| I2cXfer { addr: 0x18, read: true, data: data.to_vec(), repeated: false, stop: true };
    let mut x = vec![
        w(&[0xf0]),
        w(&[0xd2, 0xe1]),
        w(&[0xc3, 0xe1]),
        w(&[0xb4]),
        r(&[0x03, 0x02]), // busy, then done with presence
        w(&[0xa5, 0xcc]),
        r(&[0x00]),
        w(&[0xa5, 0xbe]),
        w(&[0x96]),
        r(&[0x01, 0x00]),
        w(&[0xe1, 0xe1]),
        r(&[0x50]),
        w(&[0x96]), // no poll
        w(&[0xe1, 0xe1]),
        r(&[0x05]),
        // another I2C device on the bus is ignored
        I2cXfer { addr: 0x50, read: false, data: vec![0xb4, 0xa5, 0x33], repeated: false, stop: true },
        w(&[0xb4]),
        r(&[0x02]),
        w(&[0xa5, 0xf0]),
    ];
    for i in 0..64 {
        let b = (rom[i / 8] >> (i % 8)) & 1 == 1;
        x.push(w(&[0x78, if b { 0x80 } else { 0 }]));
        x.push(r(&[(b as u8) << 5 | (!b as u8) << 6 | (b as u8) << 7]));
    }
    x.push(w(&[0xb4]));
    x.push(r(&[0x00])); // no presence
    let rate = 400e3;
    let (sda, scl) = i2c_logic(&x, rate, 0.0, 3.3);
    let dt = 0.1e-6;
    let n = ((sda.end() + 5e-6) / dt) as usize;
    let a = sample(|t| sda.v(t), 0.0, dt, n);
    let c = sample(|t| scl.v(t), 0.0, dt, n);
    let p = ds2482(&i2c(&a, &c, 1.65, 0.3));
    let (s_lo, _) = ser(&rom);
    assert_eq!(
        lvo(&p),
        vec![
            e("!DRST", 0, true),
            e("CFG", 1, true),
            e("CH", 1, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xcc, true),
            e("D", 0xbe, true),
            e("D", 0x50, true),
            e("D", 0x05, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, true),
            e("ROM", 0xf0, true),
            e("FAM", 0x28, true),
            e("SER", s_lo, true),
            e("CRC", rom[7] as u32, true),
            e("!RESET", 0, true),
            e("!PRESENCE", 0, false),
        ]
    );
    // a bad configuration byte (upper nibble not the complement)
    let bad = [
        Packet { t0: 0.0, t1: 1.0, kind: Kind::Start, value: 0, value2: None, ok: true },
        Packet { t0: 1.0, t1: 2.0, kind: Kind::Addr { read: false }, value: 0x19, value2: None, ok: true },
        Packet { t0: 2.0, t1: 3.0, kind: Kind::Data, value: 0xd2, value2: None, ok: true },
        Packet { t0: 3.0, t1: 4.0, kind: Kind::Data, value: 0x11, value2: None, ok: true },
    ];
    assert_eq!(lvo(&ds2482(&bad)), vec![e("CFG", 1, false)]);
}
