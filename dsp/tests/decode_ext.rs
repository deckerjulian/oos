// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Extended decoders (part 1) against generated signals with known content: round trips, corrupted bits, noise.

use oos_dsp::decode::{i2c, Kind, Packet, Parity};
use oos_dsp::decode_ext::gen::{self, FdFrame, FrFrame, Ow, Wave};
use oos_dsp::decode_ext::*;
use oos_dsp::sim::{add_noise, bits_logic, i2c_logic, sample, I2cXfer, Levels, Logic};
use oos_dsp::trigger::Slope;

/// xorshift random numbers
fn rng(seed: u64) -> impl FnMut() -> u32 {
    let mut s = seed | 1;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 16) as u32
    }
}

fn label(k: Kind) -> String {
    match k {
        Kind::Field(l) => l.to_string(),
        Kind::Data => "D".into(),
        Kind::Addr { .. } => "A".into(),
        Kind::Crc => "CRC".into(),
        Kind::Start => "S".into(),
        Kind::Stop => "P".into(),
        Kind::Break => "BRK".into(),
        Kind::Ack => "ACK".into(),
        Kind::Id { .. } => "ID".into(),
        k => format!("{:?}", k),
    }
}

/// (label, value, ok) of every packet
fn lvo(p: &[Packet]) -> Vec<(String, u32, bool)> {
    p.iter().map(|x| (label(x.kind), x.value, x.ok)).collect()
}

fn vals(p: &[Packet], l: &str) -> Vec<u32> {
    p.iter().filter(|x| label(x.kind) == l).map(|x| x.value).collect()
}

fn oks(p: &[Packet], l: &str) -> Vec<bool> {
    p.iter().filter(|x| label(x.kind) == l).map(|x| x.ok).collect()
}

fn e(l: &str, v: u32, ok: bool) -> (String, u32, bool) {
    (l.to_string(), v, ok)
}

fn end(l: &Logic) -> f64 {
    l.seg.last().map_or(0.0, |s| s.0)
}

fn wave(l: &Logic, dt: f64, extra: f64) -> Vec<f32> {
    sample(|t| l.v(t), 0.0, dt, ((end(l) + extra) / dt) as usize)
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn parallel_clocked_and_unclocked() {
    let words = [0x3u32, 0xa, 0xf, 0x0, 0x5, 0x9];
    let t = 1e-6;
    let dt = t / 50.0;
    let mut lines = Vec::new();
    for k in 0..4 {
        let mut l = Logic::new(0.0, 3.3, t / 100.0);
        // line 3 lags by half a sample (skew)
        let skew = if k == 3 { dt / 2.0 } else { 0.0 };
        for (i, w) in words.iter().enumerate() {
            l.push(i as f64 * t + if i > 0 { skew } else { 0.0 }, (w >> k) & 1 == 1);
        }
        lines.push(l);
    }
    let mut clk = Logic::new(0.0, 3.3, t / 100.0);
    clk.push(0.0, false);
    for i in 0..words.len() {
        clk.push(i as f64 * t + t / 2.0, true);
        clk.push((i + 1) as f64 * t, false);
    }
    let n = (words.len() as f64 * t / dt) as usize + 10;
    let v: Vec<Vec<f32>> = lines.iter().map(|l| sample(|x| l.v(x), 0.0, dt, n)).collect();
    let c = sample(|x| clk.v(x), 0.0, dt, n);
    let refs: Vec<&[f32]> = v.iter().map(|x| x.as_slice()).collect();
    let p = parallel(&refs, Some(&c), Slope::Rising, 1.65, 0.3);
    assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), words);
    assert!(p.iter().all(|x| x.ok));
    let p = parallel(&refs, None, Slope::Rising, 1.65, 0.3);
    assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), words, "{:?}", p);
    // a 2-line bus on the falling clock edge sees the next word (data changes with the falling edge)
    let p = parallel(&refs[..2], Some(&c), Slope::Falling, 1.65, 0.3);
    let mut want: Vec<u32> = words[1..].iter().map(|w| w & 3).collect();
    want.push(words[5] & 3);
    assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), want);
}

#[test]
fn uart_baud_detects_standard_and_odd_rates() {
    let mut r = rng(11);
    for &rate in &[1200.0, 9600.0, 19200.0, 38400.0, 115200.0, 250000.0, 921600.0, 1e6, 3e6] {
        let bytes: Vec<u8> = (0..40).map(|_| r() as u8).collect();
        let l = oos_dsp::sim::uart_logic(&bytes, rate, 8, None, 0.5, 0.0, 3.3);
        let dt = 1.0 / rate / 13.7;
        let mut v = wave(&l, dt, 5.0 / rate);
        add_noise(&mut v, 0.1, 5);
        assert_eq!(uart_baud(&v, dt, 1.65, 0.4), Some(rate), "rate {}", rate);
    }
    // not a standard rate: the raw estimate
    let rate = 100_000.0;
    let bytes: Vec<u8> = (0..30).map(|_| r() as u8).collect();
    let l = oos_dsp::sim::uart_logic(&bytes, rate, 8, Some(true), 1.0, 0.0, 5.0);
    let dt = 1.0 / rate / 20.0;
    let est = uart_baud(&wave(&l, dt, 5.0 / rate), dt, 2.5, 0.4).unwrap();
    assert!((est - rate).abs() / rate < 0.005, "{}", est);
    // a lone glitch does not count as the bit time
    let mut v = wave(&oos_dsp::sim::uart_logic(b"Hello, world", 9600.0, 8, None, 1.0, 0.0, 3.3), 1e-6, 1e-3);
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
    let d = wave(&l, dt, 5.0 / baud);
    // A = 2.5 + d, B = 2.5 - d, 2 V common mode hum and independent noise on both wires
    let hum = |i: usize| 2.0 * (i as f32 * 0.003).sin();
    let mut a: Vec<f32> = d.iter().enumerate().map(|(i, x)| 2.5 + x + hum(i)).collect();
    let mut b: Vec<f32> = d.iter().enumerate().map(|(i, x)| 2.5 - x + hum(i)).collect();
    add_noise(&mut a, 0.15, 1);
    add_noise(&mut b, 0.15, 2);
    let pair = diff(&a, &b);
    let p = uart9(&pair, dt, baud, 0.0, 0.4, false);
    let got: Vec<(bool, u32, Option<u32>, bool)> = p.iter().map(|x| (matches!(x.kind, Kind::Addr { .. }), x.value, x.value2, x.ok)).collect();
    assert_eq!(
        got,
        [(true, 0x01, Some(1), true), (false, 0x22, Some(0), true), (false, 0x33, Some(0), true), (true, 0xfe, Some(1), true), (false, 0x44, Some(0), true), (false, 0x00, Some(0), true)]
    );
    // the single wire A against ground does not decode through the hum, the pair does
    assert_ne!(uart9(&a, dt, baud, 2.5, 0.4, false).iter().map(|x| x.value).collect::<Vec<_>>(), p.iter().map(|x| x.value).collect::<Vec<_>>());
    // swapped wires = the other polarity convention
    assert_eq!(uart9(&diff(&b, &a), dt, baud, 0.0, 0.4, true).len(), words.len());
}

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
    let mut v = wave(&w.done(), dt, 100e-6);
    add_noise(&mut v, 0.3, 9);
    let p = dmx512(&v, dt, 2.5, 0.5, false);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Break).count(), 4);
    assert_eq!(vals(&p, "MAB"), [12; 4]);
    assert_eq!(vals(&p, "SC"), [0x00, 0xcc, 0xcc, 0xcc]);
    // dimmer slots: level and slot number
    let slots: Vec<(u32, Option<u32>)> = p.iter().filter(|x| x.kind == Kind::Data).take(24).map(|x| (x.value, x.value2)).collect();
    assert_eq!(slots, levels.iter().enumerate().map(|(i, &l)| (l as u32, Some(i as u32 + 1))).collect::<Vec<_>>());
    // RDM fields
    let dst: Vec<(u32, Option<u32>)> = p.iter().filter(|x| label(x.kind) == "DST").map(|x| (x.value, x.value2)).collect();
    assert_eq!(dst, [(0x1234_5678, Some(0x7a70)); 3]);
    assert_eq!(p.iter().find(|x| label(x.kind) == "SRC").map(|x| (x.value, x.value2)), Some((0x0000_0001, Some(0x4d41))));
    assert_eq!(vals(&p, "TN"), [5, 6, 6]);
    assert_eq!(vals(&p, "CC"), [0x20, 0x30, 0x30]);
    assert_eq!(vals(&p, "PID"), [0x0060, 0x00f0, 0x00f0]);
    assert_eq!(vals(&p, "SUBDEV"), [0, 3, 3]);
    assert_eq!(vals(&p, "PDL"), [0, 2, 2]);
    assert_eq!(vals(&p, "LEN"), [24, 26, 26]);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Data).skip(24).map(|x| x.value).collect::<Vec<_>>(), [0x00, 0x2a, 0x00, 0x2a]);
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
    let p = dmx512(&wave(&w.done(), dt, 50e-6), dt, 2.5, 0.5, false);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Break).count(), 1);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Data).map(|x| (x.value, x.ok)).collect::<Vec<_>>(), [(0x7f, false), (0x01, true)]);
}

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
    let mut v = wave(&w.done(), dt, 20.0 / baud);
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
    let p = modbus_ascii(&wave(&w.done(), dt, 20.0 / baud), dt, baud, 7, Parity::Even, 1.65, 0.4, false);
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

// ---------------------------------------------------------------------------------------------------------------

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
    let mut v = wave(&l, dt, 100e-6);
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
    ops.extend([Ow::Reset { presence: true }, Ow::Write(0xec), Ow::ReadBit(true), Ow::ReadBit(true), Ow::WriteBit(true)]);
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
    let t0 = end(&l) + 50e-6;
    for (t, b) in gen::onewire_logic(&ops, 0.0, 5.0).seg {
        l.push(t0 + t, b);
    }
    let dt = 0.25e-6;
    let p = onewire(&wave(&l, dt, 50e-6), dt, 2.5, 0.5, false);
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
    let mut ops = vec![Ow::Reset { presence: true }, Ow::Write(0x3c), Ow::Speed(true), Ow::Write(0x4e), Ow::Write(0x81)];
    ops.extend([Ow::Reset { presence: true }, Ow::Write(0x69)]);
    ops.extend(rom.iter().map(|&b| Ow::Write(b)));
    ops.extend([Ow::Write(0xbe), Ow::Read(0x12), Ow::Speed(false), Ow::Reset { presence: true }, Ow::Write(0xcc)]);
    let l = gen::onewire_logic(&ops, 0.0, 3.3);
    let dt = 0.1e-6;
    let mut v = wave(&l, dt, 100e-6);
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
    let n = ((end(&sda) + 5e-6) / dt) as usize;
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

// ---------------------------------------------------------------------------------------------------------------

fn fd(id: u32, ext: bool, brs: bool, data: Vec<u8>) -> FdFrame {
    FdFrame { id, ext, fd: true, brs, esi: false, data }
}

fn ids(p: &[Packet]) -> Vec<(u32, bool, bool)> {
    p.iter().filter_map(|x| if let Kind::Id { ext, rtr } = x.kind { Some((x.value, ext, rtr)) } else { None }).collect()
}

#[test]
fn can_fd_500k_2m_with_8_20_64_bytes() {
    let mut r = rng(77);
    let d8: Vec<u8> = (0..8).map(|_| r() as u8).collect();
    let d20: Vec<u8> = (0..20).map(|_| r() as u8).collect();
    let d64: Vec<u8> = (0..64).map(|i| if i < 16 { 0xff } else if i < 32 { 0x00 } else { r() as u8 }).collect();
    let frames = [
        fd(0x123, false, true, d8.clone()),
        fd(0x1234_567, true, true, d20.clone()),
        fd(0x7ff, false, true, d64.clone()),
        FdFrame { id: 0x055, ext: false, fd: true, brs: false, esi: true, data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] },
        FdFrame { id: 0x0f0, ext: false, fd: false, brs: false, esi: false, data: vec![0xde, 0xad] },
        FdFrame { id: 0x1fff_ffff, ext: true, fd: false, brs: false, esi: false, data: vec![0xff; 8] },
        fd(0x000, false, true, vec![]),
        // ends with 8 equal bits: the fixed stuff bit replaces the dynamic one
        fd(0x100, false, true, vec![0x5a, 0x00]),
    ];
    let (nom, dat) = (500e3, 2e6);
    let l = gen::can_fd_logic(&frames, nom, dat, 0.0, 3.3);
    let dt = 1.0 / 40e6;
    let mut v = wave(&l, dt, 20.0 / nom);
    add_noise(&mut v, 0.1, 21);
    let p = can_fd(&v, dt, nom, dat, 1.65, 0.3, false);
    assert_eq!(ids(&p), vec![(0x123, false, false), (0x1234_567, true, false), (0x7ff, false, false), (0x055, false, false), (0x0f0, false, false), (0x1fff_ffff, true, false), (0, false, false), (0x100, false, false)]);
    assert_eq!(vals(&p, "DLC"), [8, 11, 15, 9, 2, 8, 0, 2]);
    let mut want: Vec<u32> = Vec::new();
    for d in [&d8, &d20, &d64] {
        want.extend(d.iter().map(|&x| x as u32));
    }
    want.extend(1..=12);
    want.extend([0xde, 0xad]);
    want.extend([0xff; 8]);
    want.extend([0x5a, 0x00]);
    assert_eq!(vals(&p, "D"), want);
    assert_eq!(vals(&p, "BRS"), [1, 1, 1, 0, 1, 1]);
    assert_eq!(vals(&p, "ESI"), [0, 0, 0, 1, 0, 0]);
    assert_eq!(oks(&p, "SBC"), [true; 6]);
    assert_eq!(oks(&p, "CRC"), [true; 8], "{:?}", lvo(&p));
    assert_eq!(oks(&p, "ACK"), [true; 8]);
    // CRC-17 up to 16 bytes, CRC-21 above
    let crcs = vals(&p, "CRC");
    assert!(crcs[0] < 1 << 17 && crcs[3] < 1 << 17);
    // the data phase is really 4x faster: the 64-byte frame takes far less than 64*8 nominal bits
    let s = p.iter().filter(|x| x.kind == Kind::Start).nth(2).unwrap().t0;
    let a = p.iter().filter(|x| x.kind == Kind::Ack).nth(2).unwrap().t0;
    assert!((a - s) * dt < 64.0 * 8.0 / nom / 2.0);
}

#[test]
fn can_fd_corrupted_bits_fail() {
    let (nom, dat) = (500e3, 2e6);
    let f = fd(0x2a5, false, true, (0..20u8).map(|i| i.wrapping_mul(37)).collect());
    let good = gen::can_fd_bits(&f, nom, dat);
    let dt = 1.0 / 40e6;
    let mk = |bits: &[(bool, f64)]| {
        let mut w = Wave::new(0.0, 3.3, 0.02 / dat);
        w.level(true, 12.0 / nom);
        for &(b, d) in bits {
            w.level(b, d);
        }
        wave(&w.done(), dt, 10.0 / nom)
    };
    let p = can_fd(&mk(&good), dt, nom, dat, 1.65, 0.3, false);
    assert_eq!(oks(&p, "CRC"), [true]);
    // flip single bits in the data field, the stuff count / CRC field: never a passing CRC
    let n = good.len();
    let mut tested = 0;
    for i in (30..n - 15).step_by(7) {
        let mut b = good.clone();
        b[i].0 = !b[i].0;
        let p = can_fd(&mk(&b), dt, nom, dat, 1.65, 0.3, false);
        assert!(!oks(&p, "CRC").contains(&true) || !oks(&p, "SBC").iter().all(|&x| x), "bit {}: {:?}", i, lvo(&p));
        tested += 1;
    }
    assert!(tested > 20);
    // a missing ACK
    let mut b = good.clone();
    b[n - 3].0 = true;
    assert_eq!(oks(&can_fd(&mk(&b), dt, nom, dat, 1.65, 0.3, false), "ACK"), [false]);
}

#[test]
fn can_fd_on_can_h_1m_5m() {
    // CAN_H against ground: recessive 2.5 V, dominant 3.5 V, 1 Mbit/s nominal, 5 Mbit/s data
    let f = [fd(0x3c1, true, true, (0..32u8).collect())];
    let (nom, dat) = (1e6, 5e6);
    let l = gen::can_fd_logic(&f, nom, dat, 3.5, 2.5);
    let dt = 1.0 / 100e6;
    let mut v = wave(&l, dt, 10.0 / nom);
    add_noise(&mut v, 0.05, 2);
    let p = can_fd(&v, dt, nom, dat, 3.0, 0.2, true);
    assert_eq!(ids(&p), vec![(0x3c1, true, false)]);
    assert_eq!(vals(&p, "D"), (0..32).collect::<Vec<u32>>());
    assert_eq!(oks(&p, "CRC"), [true]);
}

// ---------------------------------------------------------------------------------------------------------------

fn fr(id: u16, cycle: u8, payload: Vec<u8>, channel_b: bool) -> FrFrame {
    FrFrame { ppi: false, nf: true, sync: false, startup: false, id, cycle, payload, channel_b }
}

fn fr_logic(frames: &[FrFrame], tss: usize, flip: Option<usize>) -> Vec<bool> {
    let mut all = Vec::new();
    for f in frames {
        let mut b = gen::flexray_bits(f, tss);
        if let Some(i) = flip {
            b[i] = !b[i];
        }
        all.extend(b);
        all.extend([true; 20]);
    }
    all
}

#[test]
fn flexray_frames_on_rxd() {
    let frames = [
        fr(5, 3, (0..8).collect(), false),
        FrFrame { ppi: false, nf: true, sync: true, startup: true, id: 2047, cycle: 63, payload: (0..16).map(|i| 0xf0 ^ i).collect(), channel_b: true },
        fr(100, 0, vec![], false),
    ];
    let bits = fr_logic(&frames, 11, None);
    let tb = 1e-7;
    let l = bits_logic(&bits, tb, 20, 0.0, 3.3);
    let dt = tb / 8.0;
    let mut v = wave(&l, dt, 20.0 * tb);
    add_noise(&mut v, 0.2, 6);
    let p = flexray(&v, dt, 10e6, 1.65, 0.4, false);
    assert_eq!(vals(&p, "TSS"), [11, 11, 11]);
    assert_eq!(vals(&p, "ID"), [5, 2047, 100]);
    assert_eq!(vals(&p, "IND"), [0b00100, 0b00111, 0b00100]);
    assert_eq!(vals(&p, "LEN"), [4, 8, 0]);
    assert_eq!(vals(&p, "CYC"), [3, 63, 0]);
    assert_eq!(oks(&p, "HCRC"), [true; 3]);
    let mut want: Vec<u32> = (0..8).collect();
    want.extend((0..16).map(|i| 0xf0 ^ i));
    assert_eq!(vals(&p, "D"), want);
    assert_eq!(oks(&p, "CRC"), [true; 3]);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Crc).map(|x| x.value2).collect::<Vec<_>>(), [Some(0), Some(1), Some(0)]);
    assert!(p.iter().all(|x| x.ok), "{:?}", lvo(&p));
}

#[test]
fn flexray_differential_pair() {
    // BP/BM: idle both 2.5 V (0 V difference), Data_1 = BP 3.0 / BM 2.0, Data_0 the inverse
    let frames = [fr(0x1a, 9, vec![0x12, 0x34, 0x56, 0x78], false), fr(0x1b, 10, vec![0xaa, 0x55], false)];
    let tb = 1e-7;
    let (mut bp, mut bm) = (Levels { seg: vec![(0.0, 2.5)], tr: tb / 10.0 }, Levels { seg: vec![(0.0, 2.5)], tr: tb / 10.0 });
    let mut t = 30.0 * tb;
    for f in &frames {
        for b in gen::flexray_bits(f, 9) {
            bp.seg.push((t, if b { 3.0 } else { 2.0 }));
            bm.seg.push((t, if b { 2.0 } else { 3.0 }));
            t += tb;
        }
        bp.seg.push((t, 2.5));
        bm.seg.push((t, 2.5));
        t += 25.0 * tb;
    }
    let dt = tb / 8.0;
    let n = (t / dt) as usize;
    let mut a = sample(|x| bp.v(x), 0.0, dt, n);
    let mut b = sample(|x| bm.v(x), 0.0, dt, n);
    add_noise(&mut a, 0.05, 1);
    add_noise(&mut b, 0.05, 2);
    let p = flexray(&diff(&a, &b), dt, 10e6, 0.0, 0.3, false);
    assert_eq!(vals(&p, "ID"), [0x1a, 0x1b]);
    assert_eq!(vals(&p, "D"), [0x12, 0x34, 0x56, 0x78, 0xaa, 0x55]);
    assert_eq!(oks(&p, "CRC"), [true, true]);
    assert_eq!(oks(&p, "HCRC"), [true, true]);
}

#[test]
fn flexray_crc_errors() {
    let f = [fr(0x33, 1, vec![1, 2, 3, 4], false)];
    let tb = 1e-7;
    let dt = tb / 8.0;
    let run = |flip: usize| {
        let l = bits_logic(&fr_logic(&f, 5, Some(flip)), tb, 20, 0.0, 3.3);
        flexray(&wave(&l, dt, 20.0 * tb), dt, 10e6, 1.65, 0.4, false)
    };
    // bit index of byte k, bit j (MSB = 0): TSS 5 + FSS 1 + 10 per byte + BSS 2
    let at = |k: usize, j: usize| 5 + 1 + 10 * k + 2 + j;
    // a payload bit: header CRC fine, frame CRC fails
    let p = run(at(6, 3));
    assert_eq!((oks(&p, "HCRC"), oks(&p, "CRC")), (vec![true], vec![false]));
    assert_eq!(vals(&p, "D"), [1, 2 ^ 0x10, 3, 4]);
    // a frame ID bit: both fail
    let p = run(at(1, 2));
    assert_eq!((oks(&p, "HCRC"), oks(&p, "CRC")), (vec![false], vec![false]));
    // a BSS low bit missing: the frame breaks off with an error
    let p = run(at(7, 0) - 1);
    assert!(oks(&p, "!ERR").contains(&false), "{:?}", lvo(&p));
}

/// A record that starts or ends inside a frame: the cut frames give no errors (the protocol trigger's "Error" would
/// fire on them), the whole frame in between decodes.
#[test]
fn flexray_frames_cut_by_the_record() {
    // zero bytes: long low runs that look like a TSS inside the frame
    let frames = vec![fr(0x40, 1, vec![0, 0, 0x01, 0, 0x10, 0, 0, 0], false); 3];
    let tb = 1e-7;
    let dt = tb / 8.0;
    let v = wave(&bits_logic(&fr_logic(&frames, 11, None), tb, 20, 0.0, 3.3), dt, 20.0 * tb);
    // frames of 11 + 1 + 10 * 16 + 2 bits, 20 idle bits apart, the first after 20 idle bits
    let len = 11.0 + 1.0 + 160.0 + 2.0 + 20.0;
    for cut in [30.0, 60.0, 95.0, 120.0, 150.0] {
        let a = ((20.0 + cut) * 8.0) as usize;
        let b = ((20.0 + 2.0 * len + cut) * 8.0) as usize;
        let p = flexray(&v[a..b], dt, 10e6, 1.65, 0.4, false);
        assert!(p.iter().all(|x| x.ok), "cut at {}: {:?}", cut, lvo(&p));
        // the second frame, and the header of the third when the record holds it
        let ids = vals(&p, "ID");
        assert!(ids == [0x40] || (cut > 52.0 && ids == [0x40, 0x40]), "cut at {}: {:?}", cut, lvo(&p));
        assert_eq!(oks(&p, "CRC"), [true], "cut at {}", cut);
    }
}

// ---------------------------------------------------------------------------------------------------------------

#[test]
fn noise_and_garbage_never_panic() {
    let mut r = rng(1234);
    for round in 0..6 {
        // random levels held for random lengths, plus noise and some NaNs
        let n = 20_000 + round * 3000;
        let mut v = Vec::with_capacity(n);
        while v.len() < n {
            let lvl = (r() % 1000) as f32 / 1000.0 * 4.0 - 0.5;
            let len = 1 + (r() % [3, 30, 300, 3000][round % 4]) as usize;
            v.extend(std::iter::repeat(lvl).take(len));
        }
        v.truncate(n);
        add_noise(&mut v, 0.3, round as u64 + 1);
        if round % 2 == 1 {
            v[100] = f32::NAN;
            v[n / 2] = f32::INFINITY;
        }
        let w: Vec<f32> = v.iter().rev().copied().collect();
        for &dt in &[1e-9, 2.5e-8, 1e-7, 1e-6, 1e-5] {
            for inv in [false, true] {
                let thr = 1.65;
                let _ = parallel(&[&v, &w], Some(&w), Slope::Either, thr, 0.2);
                let _ = parallel(&[&v, &w, &v[..n / 2]], None, Slope::Rising, thr, 0.2);
                let _ = uart_baud(&v, dt, thr, 0.2);
                let _ = uart9(&diff(&v, &w), dt, 115200.0, 0.0, 0.2, inv);
                let _ = dmx512(&v, dt, thr, 0.2, inv);
                let _ = modbus_rtu(&v, dt, 9600.0, Parity::Even, thr, 0.2, inv);
                let _ = modbus_rtu(&v, dt, 1e6, Parity::None, thr, 0.2, inv);
                let _ = modbus_ascii(&v, dt, 9600.0, 7, Parity::Even, thr, 0.2, inv);
                let _ = onewire(&v, dt, thr, 0.2, inv);
                let _ = can_fd(&v, dt, 500e3, 2e6, thr, 0.2, inv);
                let _ = can_fd(&v, dt, 1e6, 8e6, thr, 0.2, inv);
                let _ = flexray(&v, dt, 10e6, thr, 0.2, inv);
                let _ = flexray(&v, dt, 2.5e6, thr, 0.2, inv);
            }
        }
        // the DS2482 interpreter on decoded garbage and on random packets
        let _ = ds2482(&i2c(&v, &w, 1.65, 0.2));
        let kinds = [Kind::Start, Kind::RepStart, Kind::Stop, Kind::Addr { read: false }, Kind::Addr { read: true }, Kind::Data, Kind::Ack];
        let pk: Vec<Packet> = (0..3000)
            .map(|i| {
                let k = kinds[(r() % kinds.len() as u32) as usize];
                let value = if matches!(k, Kind::Addr { .. }) { 0x18 + r() % 8 } else { r() % 256 };
                Packet { t0: i as f64, t1: i as f64 + 0.5, kind: k, value, value2: None, ok: r() % 2 == 0 }
            })
            .collect();
        let _ = ds2482(&pk);
    }
    // degenerate inputs
    let empty: [f32; 0] = [];
    assert!(parallel(&[], None, Slope::Rising, 0.0, 0.0).is_empty());
    assert!(parallel(&[&empty], None, Slope::Rising, 0.0, 0.0).is_empty());
    assert_eq!(uart_baud(&empty, 1e-6, 0.0, 0.1), None);
    assert!(dmx512(&empty, 1e-6, 0.0, 0.1, false).is_empty());
    assert!(onewire(&empty, 0.0, 0.0, 0.1, false).is_empty());
    assert!(can_fd(&empty, f64::NAN, 500e3, 2e6, 0.0, 0.1, false).is_empty());
    assert!(flexray(&[1.0; 10], 1e-8, 0.0, 0.0, 0.1, false).is_empty());
    assert!(modbus_rtu(&[1.0; 10], 1e-6, 0.0, Parity::None, 0.0, 0.1, false).is_empty());
    assert!(ds2482(&[]).is_empty());
}
