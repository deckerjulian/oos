// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The ARINC 429 decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

/// (label, SDI, data, SSM) of the test words
const WORDS: [(u8, u8, u32, u8); 3] = [(0o310, 1, 0x5_a5a5, 3), (0o203, 0, 0x0_0001, 0), (0o377, 2, 0x7_ffff, 1)];

fn arinc_want(words: &[(u8, u8, u32, u8)]) -> Vec<(Kind, u32, bool)> {
    let mut want = Vec::new();
    for &(label, sdi, data, ssm) in words {
        let b = gen::arinc429_word(label, sdi, data, ssm, false);
        want.extend([
            (ID, label as u32, true),
            (F("SDI"), sdi as u32, true),
            (Kind::Data, data, true),
            (F("SSM"), ssm as u32, true),
            (F("PAR"), b[31] as u32, true),
        ]);
    }
    want
}

#[test]
fn arinc429_words_at_both_rates_fixed_and_auto() {
    let words: Vec<Vec<bool>> = WORDS.iter().map(|&(l, s, d, m)| gen::arinc429_word(l, s, d, m, false)).collect();
    // parity: odd number of ones in every word
    assert!(words.iter().all(|w| w.len() == 32 && w.iter().filter(|&&b| b).count() % 2 == 1));
    // label bit 1 (first on the bus) is the label MSB
    assert_eq!(&words[0][..8], &[true, true, false, false, true, false, false, false]);
    // A - B at +-10 V; one line against ground at +-5 V; a 14 kbit/s transmitter on the low-speed setting
    for (rate, set, a, gap) in [
        (100e3, Some(ARINC429_HIGH), 10.0, 4),
        (100e3, None, 5.0, 4),
        (12.5e3, Some(ARINC429_LOW), 10.0, 8),
        (14e3, None, 5.0, 4),
        (14e3, Some(ARINC429_LOW), 5.0, 4),
    ] {
        let l = gen::arinc429(&words, rate, a, gap);
        let dt = 1.0 / rate / 40.0;
        let mut v = levels(&l, dt);
        add_noise(&mut v, 0.3, 7);
        let p = arinc429(&v, dt, set, (a / 2.0) as f32, 0.5);
        assert_eq!(kv(&p), arinc_want(&WORDS), "rate {} set {:?}", rate, set);
        // the protocol trigger: every word starts with its label, and a label is found by its value
        assert_eq!(p.iter().filter(|x| ARINC429.trigger_match(BusWhat::Frame, 0, x)).count(), WORDS.len());
        assert_eq!(p.iter().filter(|x| ARINC429.trigger_match(BusWhat::Id, 0o310, x)).count(), 1);
        // the label ends after 8 bits
        let bit = 1.0 / rate / dt;
        assert!((p[0].t1 - p[0].t0 - 8.0 * bit).abs() < 0.1 * bit, "{:?}", p[0]);
    }
}

#[test]
fn arinc429_parity_error_bad_words_and_record_edges() {
    let good = gen::arinc429_word(0o310, 1, 0x1234, 3, false);
    let bad = gen::arinc429_word(0o311, 0, 0x4321, 0, true);
    let mut short = good.clone();
    short.pop(); // 31 bits
    let l = gen::arinc429(&[good.clone(), bad, short, good.clone()], 100e3, 5.0, 6);
    let dt = 1e-6 / 4.0;
    let v = levels(&l, dt);
    let p = arinc429(&v, dt, None, 2.5, 0.5);
    let k = kv(&p);
    assert_eq!(k[..5], arinc_want(&[(0o310, 1, 0x1234, 3)])[..]);
    assert_eq!(
        k[5..10].iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(),
        [(ID, true), (F("SDI"), true), (Kind::Data, true), (F("SSM"), true), (F("PAR"), false)]
    );
    assert_eq!(k[9].1, 1, "9 ones: parity bit 0, flipped");
    assert_eq!(k[10], (F("!ERR"), 0, false));
    assert_eq!(k[11..], arinc_want(&[(0o310, 1, 0x1234, 3)])[..]);
    // a record that starts and ends inside a word: the cut words are skipped, no error
    let bit = 10e-6 / dt;
    let w0 = 6.0 * bit; // first word start
    let cut = &v[(w0 + 10.0 * bit) as usize..(w0 + 2.0 * 38.0 * bit + 20.0 * bit) as usize];
    let p = arinc429(cut, dt, Some(ARINC429_HIGH), 2.5, 0.5);
    assert_eq!(
        p.iter().map(|x| (x.kind, x.ok)).collect::<Vec<_>>(),
        [(ID, true), (F("SDI"), true), (Kind::Data, true), (F("SSM"), true), (F("PAR"), false)]
    );
    // nothing on a flat line, a zero threshold or too few samples per bit
    assert!(arinc429(&vec![0.0; 5000], dt, None, 2.5, 0.5).is_empty());
    assert!(arinc429(&v, dt, None, 0.0, 0.5).is_empty());
    assert!(arinc429(&v, dt, Some(2e6), 2.5, 0.5).is_empty());
}
