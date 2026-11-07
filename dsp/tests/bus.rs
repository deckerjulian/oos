// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! CAN and LIN decoders against generated frames with known content.

use oos_dsp::decode::{can, lin, Kind, Packet};
use oos_dsp::sim::{add_noise, bits_logic, can_bits, can_logic, lin_logic, sample, CanFrame};

fn ids(p: &[Packet]) -> Vec<(u32, bool, bool)> {
    p.iter().filter_map(|x| if let Kind::Id { ext, rtr } = x.kind { Some((x.value, ext, rtr)) } else { None }).collect()
}
fn data(p: &[Packet]) -> Vec<u32> {
    p.iter().filter(|x| x.kind == Kind::Data).map(|x| x.value).collect()
}
fn crcs(p: &[Packet]) -> Vec<bool> {
    p.iter().filter(|x| x.kind == Kind::Crc).map(|x| x.ok).collect()
}

#[test]
fn can_standard_extended_and_remote_frames() {
    let frames = [
        CanFrame { id: 0x123, ext: false, rtr: false, data: vec![0xde, 0xad, 0xbe, 0xef] },
        CanFrame { id: 0x1abc_def0 & 0x1fff_ffff, ext: true, rtr: false, data: vec![0x00, 0xff, 0x55] },
        CanFrame { id: 0x7ff, ext: false, rtr: true, data: vec![0; 2] },
        CanFrame { id: 0x0f0, ext: false, rtr: false, data: vec![0xff; 8] }, // long stuffed runs
    ];
    let br = 500e3;
    let l = can_logic(&frames, br, 0.0, 3.3);
    let dt = 1.0 / 25e6; // 50 samples per bit
    let v = sample(|t| l.v(t), 0.0, dt, 25_000);
    let p = can(&v, dt, br, 1.65, 0.2, false);
    assert_eq!(ids(&p), vec![(0x123, false, false), (0x1abc_def0 & 0x1fff_ffff, true, false), (0x7ff, false, true), (0x0f0, false, false)]);
    let mut want = vec![0xde, 0xad, 0xbe, 0xef, 0x00, 0xff, 0x55];
    want.extend([0xff; 8]);
    assert_eq!(data(&p), want);
    assert_eq!(crcs(&p), vec![true; 4]);
    assert!(p.iter().filter(|x| x.kind == Kind::Ack).all(|x| x.ok));
}

#[test]
fn can_detects_a_flipped_bit_and_a_missing_ack() {
    let f = CanFrame { id: 0x2a5, ext: false, rtr: false, data: vec![0x12, 0x34] };
    let mut bits = can_bits(&f, false);
    // flip one data bit that is not next to a stuff position: CRC must fail (or the frame be rejected)
    let i = 1 + 11 + 3 + 4 + 5;
    bits[i] = !bits[i];
    let l = bits_logic(&bits, 1.0 / 250e3, 12, 0.0, 5.0);
    let dt = 1.0 / 10e6;
    let v = sample(|t| l.v(t), 0.0, dt, 6000);
    let p = can(&v, dt, 250e3, 2.5, 0.2, false);
    assert!(crcs(&p).iter().all(|&ok| !ok), "{:?}", p);
    assert!(p.iter().filter(|x| x.kind == Kind::Ack).all(|x| !x.ok));
}

#[test]
fn can_on_can_h_with_noise() {
    // CAN_H against ground: recessive 2.5 V, dominant 3.5 V (inverted logic), 30 mV rms noise, 1 Mbit/s
    let f = [CanFrame { id: 0x555, ext: false, rtr: false, data: vec![1, 2, 3] }];
    let l = can_logic(&f, 1e6, 3.5, 2.5);
    let dt = 1.0 / 62.5e6;
    let mut v = sample(|t| l.v(t), 0.0, dt, 10_000);
    add_noise(&mut v, 0.03, 7);
    let p = can(&v, dt, 1e6, 3.0, 0.1, true);
    assert_eq!(ids(&p), vec![(0x555, false, false)]);
    assert_eq!(data(&p), vec![1, 2, 3]);
    assert_eq!(crcs(&p), vec![true]);
}

#[test]
fn lin_classic_and_enhanced_frames() {
    let frames = [(0x10u8, vec![0x01, 0x02, 0x03], false), (0x3c, vec![0xaa, 0x55, 0x00, 0xff, 0x12, 0x34, 0x56, 0x78], true)];
    let l = lin_logic(&frames, 19200.0, 0.0, 12.0);
    let dt = 1.0 / 1e6;
    let v = sample(|t| l.v(t), 0.0, dt, 20_000);
    let p = lin(&v, dt, 19200.0, 6.0, 0.5);
    assert_eq!(p.iter().filter(|x| x.kind == Kind::Break).count(), 2);
    assert_eq!(ids(&p).iter().map(|x| x.0).collect::<Vec<_>>(), vec![0x10, 0x3c]);
    assert!(p.iter().filter(|x| matches!(x.kind, Kind::Id { .. })).all(|x| x.ok), "PID parity");
    let d = data(&p);
    // sync bytes 0x55 come first in each frame
    assert_eq!(d, vec![0x55, 0x01, 0x02, 0x03, 0x55, 0xaa, 0x55, 0x00, 0xff, 0x12, 0x34, 0x56, 0x78]);
    assert_eq!(crcs(&p), vec![true, true]);
}

#[test]
fn lin_wrong_checksum_is_flagged() {
    // build a frame with a corrupted last data byte after the checksum was computed: regenerate with other data
    let good = lin_logic(&[(0x22, vec![9, 8, 7], true)], 9600.0, 0.0, 12.0);
    let dt = 1.0 / 500e3;
    let mut v = sample(|t| good.v(t), 0.0, dt, 9000);
    // invert one data bit of the second data byte (byte 4 of the frame incl. sync/PID, bit 3)
    let bit = 1.0 / 9600.0;
    let t0 = 12.0 * bit + 14.0 * bit + 4.0 * 11.0 * bit + (1.0 + 3.0 + 0.2) * bit;
    let (i0, i1) = ((t0 / dt) as usize, ((t0 + 0.6 * bit) / dt) as usize);
    let lo_v = v[i0];
    for x in &mut v[i0..i1] {
        *x = if lo_v > 6.0 { 0.0 } else { 12.0 };
    }
    let p = lin(&v, dt, 9600.0, 6.0, 0.5);
    assert_eq!(crcs(&p), vec![false], "{:?}", p);
}

/// A stuff bit right after the last CRC bit (ISO 11898-1: stuffing covers SOF .. CRC sequence) must not be taken for
/// the CRC delimiter.
#[test]
fn can_stuff_bit_after_the_crc() {
    let br = 500e3;
    let dt = 1.0 / 25e6;
    let mut checked = 0;
    for id in 0..0x800u32 {
        let f = CanFrame { id, ext: false, rtr: false, data: vec![id as u8, 0x5a] };
        // does the stuffed bit stream end its CRC with five equal bits? (the frame before the delimiter)
        let crc_end = {
            let mut raw = vec![false];
            (0..11).rev().for_each(|i| raw.push((id >> i) & 1 == 1));
            raw.extend([false, false, false]); // RTR, IDE, r0
            (0..4).rev().for_each(|i| raw.push((2u32 >> i) & 1 == 1));
            for b in &f.data {
                (0..8).rev().for_each(|i| raw.push((b >> i) & 1 == 1));
            }
            let crc = oos_dsp::decode::can_crc15(&raw);
            (0..15).rev().for_each(|i| raw.push((crc >> i) & 1 == 1));
            // run length at the end after stuffing
            let (mut run, mut last, mut cnt) = (0, None, 0);
            for &b in &raw {
                if Some(b) == last {
                    run += 1;
                } else {
                    run = 1;
                    last = Some(b);
                }
                if run == 5 {
                    run = 1;
                    last = Some(!b);
                    cnt = 0;
                } else {
                    cnt += 1;
                }
            }
            cnt == 0 // the last bit completed a run of five: a stuff bit follows
        };
        if !crc_end {
            continue;
        }
        let mut bits = can_bits(&f, true);
        bits.extend([true; 11]);
        let l = bits_logic(&bits, 1.0 / br, 12, 0.0, 3.3);
        let v = sample(|t| l.v(t), 0.0, dt, 8_000);
        let p = can(&v, dt, br, 1.65, 0.2, false);
        assert_eq!(crcs(&p), vec![true], "id {:#x}", id);
        assert!(p.iter().any(|x| x.kind == Kind::Ack && x.ok), "id {:#x}: {:?}", id, p.iter().filter(|x| x.kind == Kind::Ack).collect::<Vec<_>>());
        checked += 1;
        if checked >= 5 {
            break;
        }
    }
    assert!(checked > 0, "no frame with a stuff bit after the CRC found");
}
