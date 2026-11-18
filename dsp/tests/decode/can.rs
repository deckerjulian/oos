// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The CAN FD decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

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
    let d64: Vec<u8> = (0..64)
        .map(|i| {
            if i < 16 {
                0xff
            } else if i < 32 {
                0x00
            } else {
                r() as u8
            }
        })
        .collect();
    let frames = [
        fd(0x123, false, true, d8.clone()),
        fd(0x0123_4567, true, true, d20.clone()),
        fd(0x7ff, false, true, d64.clone()),
        FdFrame {
            id: 0x055,
            ext: false,
            fd: true,
            brs: false,
            esi: true,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        },
        FdFrame { id: 0x0f0, ext: false, fd: false, brs: false, esi: false, data: vec![0xde, 0xad] },
        FdFrame { id: 0x1fff_ffff, ext: true, fd: false, brs: false, esi: false, data: vec![0xff; 8] },
        fd(0x000, false, true, vec![]),
        // ends with 8 equal bits: the fixed stuff bit replaces the dynamic one
        fd(0x100, false, true, vec![0x5a, 0x00]),
    ];
    let (nom, dat) = (500e3, 2e6);
    let l = gen::can_fd_logic(&frames, nom, dat, 0.0, 3.3);
    let dt = 1.0 / 40e6;
    let mut v = l.sample(dt, 20.0 / nom);
    add_noise(&mut v, 0.1, 21);
    let p = can_fd(&v, dt, nom, dat, 1.65, 0.3, false);
    assert_eq!(
        ids(&p),
        vec![
            (0x123, false, false),
            (0x0123_4567, true, false),
            (0x7ff, false, false),
            (0x055, false, false),
            (0x0f0, false, false),
            (0x1fff_ffff, true, false),
            (0, false, false),
            (0x100, false, false)
        ]
    );
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
        w.done().sample(dt, 10.0 / nom)
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
    let mut v = l.sample(dt, 10.0 / nom);
    add_noise(&mut v, 0.05, 2);
    let p = can_fd(&v, dt, nom, dat, 3.0, 0.2, true);
    assert_eq!(ids(&p), vec![(0x3c1, true, false)]);
    assert_eq!(vals(&p, "D"), (0..32).collect::<Vec<u32>>());
    assert_eq!(oks(&p, "CRC"), [true]);
}
