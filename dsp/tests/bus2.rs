// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! I2S and MIL-STD-1553 decoders against generated streams with known content.

use oos_dsp::decode::{i2s, mil1553, Kind};
use oos_dsp::sim::{add_noise, i2s_logic, mil1553_wave, sample};

#[test]
fn i2s_stereo_16_and_24_bit() {
    for bits in [16u32, 24] {
        let frames = [(0x1234i32, -2i32), (0x7fff, 0x0100), (-32768, 5)];
        let mask = (1u32 << bits) - 1;
        let (sck, ws, sd) = i2s_logic(&frames, bits, 48e3, 0.0, 3.3);
        let tb = 1.0 / (48e3 * 2.0 * bits as f64);
        let dt = tb / 20.0;
        let n = ((frames.len() as f64 * 2.0 * bits as f64 + 8.0) * tb / dt) as usize;
        let (c, w, d) =
            (sample(|t| sck.v(t), 0.0, dt, n), sample(|t| ws.v(t), 0.0, dt, n), sample(|t| sd.v(t), 0.0, dt, n));
        let p = i2s(&c, Some(&w), &d, bits, 1.65, 0.2);
        let got: Vec<(bool, u32)> = p
            .iter()
            .filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None })
            .collect();
        let want: Vec<(bool, u32)> =
            frames.iter().flat_map(|&(l, r)| [(false, l as u32 & mask), (true, r as u32 & mask)]).collect();
        // the first word may be cut (no WS edge before it); all complete words must match
        assert!(got.len() >= want.len() - 1, "{} bits: {:?}", bits, got);
        let off = want.len() - got.len().min(want.len());
        assert_eq!(&got[..want.len() - off], &want[off..], "{} bits", bits);
    }
}

#[test]
fn mil1553_command_and_data_words_with_noise() {
    let words = [(true, 0x0c21u16), (false, 0xbeef), (false, 0x0000), (false, 0xffff)];
    let l = mil1553_wave(&words, 7.0, 4.0);
    let dt = 1e-6 / 50.0;
    let mut v = sample(|t| l.v(t), 0.0, dt, 50 * 110);
    add_noise(&mut v, 0.3, 3);
    let p = mil1553(&v, dt, 0.0, 1.0);
    let got: Vec<(bool, u32, bool)> = p
        .iter()
        .filter_map(|x| if let Kind::Word { cmd } = x.kind { Some((cmd, x.value, x.ok)) } else { None })
        .collect();
    assert_eq!(got, vec![(true, 0x0c21, true), (false, 0xbeef, true), (false, 0, true), (false, 0xffff, true)]);
}

/// A bus a few percent off its rate is read: the decoder follows the edge in the middle of every bit. On the 1 us
/// grid of the edge in the sync the last bits of such a word were read in the wrong half.
#[test]
fn mil1553_follows_a_bus_off_its_rate() {
    let words = [(true, 0x8bf2u16), (false, 0x0008), (true, 0x9001), (false, 0x0005), (true, 0x8842), (false, 0xffff)];
    let l = mil1553_wave(&words, 2.3, 9.0);
    let dt = 1e-6 / 50.0;
    let mut v = sample(|t| l.v(t), 0.0, dt, 50 * 180);
    add_noise(&mut v, 0.1, 5);
    let want: Vec<(bool, u32, bool)> = words.iter().map(|&(cmd, w)| (cmd, w as u32, true)).collect();
    // the same samples with a sample interval 4 and 5 % shorter and longer: a bus that much slow and fast
    for off in [1.0, 0.96, 1.04, 0.95, 1.05] {
        let p = mil1553(&v, dt * off, 0.0, 0.3);
        let word = |x: &oos_dsp::decode::Packet| match x.kind {
            Kind::Word { cmd } => Some((cmd, x.value, x.ok)),
            _ => None,
        };
        let got: Vec<(bool, u32, bool)> = p.iter().filter_map(word).collect();
        assert_eq!(got, want, "sample interval times {off}");
    }
}

#[test]
fn mil1553_bad_parity_is_flagged() {
    let l = mil1553_wave(&[(false, 0x1234)], 7.0, 4.0);
    let dt = 1e-6 / 50.0;
    let mut v = sample(|t| l.v(t), 0.0, dt, 50 * 30);
    // swap the halves of the parity bit (bit 17 starts 4 + 3 + 16 us after t = 0)
    let t = 4.0 + 3.0 + 16.0;
    let (a, b) = ((t * 50.0) as usize, ((t + 1.0) * 50.0) as usize);
    for x in &mut v[a..b] {
        *x = -*x;
    }
    let p = mil1553(&v, dt, 0.0, 1.0);
    assert_eq!(p.len(), 1);
    assert!(!p[0].ok);
    assert_eq!(p[0].value, 0x1234);
}
