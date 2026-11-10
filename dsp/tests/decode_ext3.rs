// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! ARINC 429, SENT, USB and NMRA DCC decoders against generated signals with known content.

use oos_dsp::decode::{Kind, Packet};
use oos_dsp::decode_ext3::gen::{self, DccFrame, SentFrame};
use oos_dsp::decode_ext3::*;
use oos_dsp::sim::{add_noise, sample, Levels, Logic};

fn kv(p: &[Packet]) -> Vec<(Kind, u32, bool)> {
    p.iter().map(|x| (x.kind, x.value, x.ok)).collect()
}

/// `kv` with the measured SENT tick rounded to 10 ns
fn skv(p: &[Packet]) -> Vec<(Kind, u32, bool)> {
    p.iter().map(|x| (x.kind, if x.kind == F("SYNC") { (x.value + 5) / 10 * 10 } else { x.value }, x.ok)).collect()
}

const F: fn(&'static str) -> Kind = Kind::Field;
const ID: Kind = Kind::Id { ext: false, rtr: false };

fn levels(l: &Levels, dt: f64) -> Vec<f32> {
    let end = l.seg.last().map_or(0.0, |s| s.0);
    sample(|t| l.v(t), 0.0, dt, (end / dt) as usize)
}

fn logic(l: &Logic, dt: f64, extra: f64) -> Vec<f32> {
    let end = l.seg.last().map_or(0.0, |s| s.0);
    sample(|t| l.v(t), 0.0, dt, ((end + extra) / dt) as usize)
}

// ------------------------------------------------------------------------------------------------- ARINC 429

/// (label, SDI, data, SSM) of the test words
const WORDS: [(u8, u8, u32, u8); 3] = [(0o310, 1, 0x5_a5a5, 3), (0o203, 0, 0x0_0001, 0), (0o377, 2, 0x7_ffff, 1)];

fn arinc_want(words: &[(u8, u8, u32, u8)]) -> Vec<(Kind, u32, bool)> {
    let mut want = Vec::new();
    for &(label, sdi, data, ssm) in words {
        let b = gen::arinc429_word(label, sdi, data, ssm, false);
        want.extend([(ID, label as u32, true), (F("SDI"), sdi as u32, true), (Kind::Data, data, true), (F("SSM"), ssm as u32, true), (F("PAR"), b[31] as u32, true)]);
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
    for (rate, set, a, gap) in [(100e3, Some(ARINC429_HIGH), 10.0, 4), (100e3, None, 5.0, 4), (12.5e3, Some(ARINC429_LOW), 10.0, 8), (14e3, None, 5.0, 4), (14e3, Some(ARINC429_LOW), 5.0, 4)] {
        let l = gen::arinc429(&words, rate, a, gap);
        let dt = 1.0 / rate / 40.0;
        let mut v = levels(&l, dt);
        add_noise(&mut v, 0.3, 7);
        let p = arinc429(&v, dt, set, (a / 2.0) as f32, 0.5);
        assert_eq!(kv(&p), arinc_want(&WORDS), "rate {} set {:?}", rate, set);
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
    assert_eq!(k[5..10].iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), [(ID, true), (F("SDI"), true), (Kind::Data, true), (F("SSM"), true), (F("PAR"), false)]);
    assert_eq!(k[9].1, 1, "9 ones: parity bit 0, flipped");
    assert_eq!(k[10], (F("!ERR"), 0, false));
    assert_eq!(k[11..], arinc_want(&[(0o310, 1, 0x1234, 3)])[..]);
    // a record that starts and ends inside a word: the cut words are skipped, no error
    let bit = 10e-6 / dt;
    let w0 = 6.0 * bit; // first word start
    let cut = &v[(w0 + 10.0 * bit) as usize..(w0 + 2.0 * 38.0 * bit + 20.0 * bit) as usize];
    let p = arinc429(cut, dt, Some(ARINC429_HIGH), 2.5, 0.5);
    assert_eq!(p.iter().map(|x| (x.kind, x.ok)).collect::<Vec<_>>(), [(ID, true), (F("SDI"), true), (Kind::Data, true), (F("SSM"), true), (F("PAR"), false)]);
    // nothing on a flat line, a zero threshold or too few samples per bit
    assert!(arinc429(&vec![0.0; 5000], dt, None, 2.5, 0.5).is_empty());
    assert!(arinc429(&v, dt, None, 0.0, 0.5).is_empty());
    assert!(arinc429(&v, dt, Some(2e6), 2.5, 0.5).is_empty());
}

// ------------------------------------------------------------------------------------------------- SENT

#[test]
fn sent_crc_table_and_methods() {
    // one nibble step of the CRC is the J2716 lookup table: crc(seed s, no data, legacy) = s, so check via data
    // [x] from seed 5: the table entry of 5 xor x
    let table = [0u8, 13, 7, 10, 14, 3, 9, 4, 1, 12, 6, 11, 15, 2, 8, 5];
    for x in 0..16u8 {
        assert_eq!(sent_crc(&[x], true), table[5] ^ x);
    }
    // the recommended method appends a zero nibble: processing the data and its CRC (legacy) gives 0
    for data in [vec![0u8, 0, 0, 0, 0, 0], vec![0xa, 0xb, 0xc, 1, 2, 3], vec![7], vec![15, 15, 15]] {
        let c = sent_crc(&data, false);
        assert!(c < 16);
        let mut d = data.clone();
        d.push(c);
        assert_eq!(sent_crc(&d, true), 0, "{:?}", data);
        assert_eq!(c, table[sent_crc(&data, true) as usize]);
    }
}

fn frames() -> Vec<SentFrame> {
    vec![
        SentFrame { status: 0x0, data: vec![0xa, 0xb, 0xc, 0x1, 0x2, 0x3], crc: None, pause: 0 },
        SentFrame { status: 0x8, data: vec![0x0, 0x0, 0x0, 0xf, 0xf, 0xf], crc: None, pause: 100 },
        SentFrame { status: 0x3, data: vec![0x7, 0x8, 0x9, 0x5, 0x5, 0x5], crc: None, pause: 56 },
        SentFrame { status: 0xf, data: vec![0x1, 0x2, 0x3, 0x4, 0x5, 0x6], crc: None, pause: 0 },
    ]
}

fn sent_want(fr: &[SentFrame], tick_ns: u32) -> Vec<(Kind, u32, bool)> {
    let mut want = Vec::new();
    for f in fr {
        let value = f.data.iter().fold(0u32, |a, &x| a << 4 | x as u32);
        want.extend([(F("SYNC"), tick_ns, true), (F("STAT"), f.status as u32, true), (Kind::Data, value, true), (Kind::Crc, sent_crc(&f.data, false) as u32, true)]);
        if f.pause > 0 {
            want.push((F("PAUSE"), f.pause, true));
        }
    }
    want
}

#[test]
fn sent_frames_with_pause_and_tick_tolerance() {
    let fr = frames();
    // a nominal 3 us transmitter, one 15 % slow (3.45 us), and one at a 10 us tick decoded without a nominal tick
    for (tick, nominal) in [(3e-6, Some(3e-6)), (3.45e-6, Some(3e-6)), (3e-6, None), (10e-6, None)] {
        let l = gen::sent(&fr, tick, 0.0, 5.0);
        let dt = tick / 10.0;
        let mut v = logic(&l, dt, 10.0 * tick);
        add_noise(&mut v, 0.1, 3);
        let p = sent(&v, dt, nominal, 6, 2.5, 0.5);
        assert_eq!(skv(&p), sent_want(&fr, (tick * 1e9).round() as u32), "tick {} nominal {:?}", tick, nominal);
        // the status nibble spans 12 + 0 ticks
        assert!((p[1].t1 - p[1].t0 - 12.0 * tick / dt).abs() < 0.2 * tick / dt);
        assert_eq!(p[2].value2, Some(6));
    }
    // a tick 30 % off the nominal: not taken for a sync
    let v = logic(&gen::sent(&fr, 3.9e-6, 0.0, 5.0), 0.3e-6, 0.0);
    assert!(sent(&v, 0.3e-6, Some(3e-6), 6, 2.5, 0.5).is_empty());
    // fewer data nibbles
    let fr3 = vec![SentFrame { status: 1, data: vec![0x1, 0x2, 0x3], crc: None, pause: 20 }, SentFrame { status: 2, data: vec![0xf, 0xe, 0xd], crc: None, pause: 0 }];
    let v = logic(&gen::sent(&fr3, 3e-6, 0.0, 5.0), 0.3e-6, 0.0);
    assert_eq!(skv(&sent(&v, 0.3e-6, Some(3e-6), 3, 2.5, 0.5)), sent_want(&fr3, 3000));
}

#[test]
fn sent_crc_error_invalid_nibble_and_cut_frames() {
    let mut fr = frames();
    let good = sent_crc(&fr[1].data, false);
    fr[1].crc = Some(good ^ 1);
    let tick = 3e-6;
    let dt = tick / 8.0;
    let v = logic(&gen::sent(&fr, tick, 0.0, 5.0), dt, 0.0);
    let p = sent(&v, dt, Some(tick), 6, 2.5, 0.5);
    let crcs: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Crc).map(|x| (x.value, x.ok)).collect();
    assert_eq!(crcs, [(sent_crc(&fr[0].data, false) as u32, true), (good as u32 ^ 1, false), (sent_crc(&fr[2].data, false) as u32, true), (sent_crc(&fr[3].data, false) as u32, true)]);
    // a nibble of 30 ticks (value 18) in the second frame: an error with the nominal tick, skipped without one
    let mut t = gen::sent_ticks(&frames());
    t[9 + 3] = 30; // frame 2 (after 9 intervals): sync, status, data 1, data 2
    let v = logic(&gen::sent_logic(&t, tick, 0.0, 5.0), dt, 0.0);
    let p = sent(&v, dt, Some(tick), 6, 2.5, 0.5);
    let k = skv(&p);
    assert_eq!(k[..4], sent_want(&frames()[..1], 3000)[..]);
    assert_eq!(k[4..6], [(F("SYNC"), 3000, true), (F("!ERR"), 0, false)]);
    assert_eq!(k[6..], sent_want(&frames()[2..], 3000)[..]);
    let p = sent(&v, dt, None, 6, 2.5, 0.5);
    assert_eq!(skv(&p), [sent_want(&frames()[..1], 3000), sent_want(&frames()[2..], 3000)].concat());
    // a record that ends inside a frame: no error
    let w = logic(&gen::sent(&frames(), tick, 0.0, 5.0), dt, 0.0);
    let half = &w[..w.len() * 3 / 5];
    assert!(sent(half, dt, Some(tick), 6, 2.5, 0.5).iter().all(|x| x.ok));
}

// ------------------------------------------------------------------------------------------------- USB

/// Bytes as bits, LSB first
fn lsb(bytes: &[u8]) -> Vec<bool> {
    bytes.iter().flat_map(|&x| (0..8).map(move |k| x >> k & 1 == 1)).collect()
}

/// `n` bits of `v` reversed
fn rev(v: u32, n: u32) -> u32 {
    (0..n).fold(0, |a, k| a | (v >> k & 1) << (n - 1 - k))
}

#[test]
fn usb_crc_known_vectors_and_residues() {
    // CRC-5/USB and CRC-16/USB check values over "123456789"
    assert_eq!(usb_crc5(&lsb(b"123456789")), 0x19);
    assert_eq!(usb_crc16(b"123456789"), 0xb4c8);
    // the examples of the USB-IF CRC note, which lists the CRC MSB first (shift register order): SETUP / OUT / IN
    // tokens, an SOF, two data packets
    for (pid, addr, endp, crc) in [(0x2D, 0x15, 0xe, 0x17), (0xE1, 0x3a, 0xa, 0x1c), (0x69, 0x70, 0x4, 0x0e)] {
        let t = gen::usb_token(pid, addr, endp);
        assert_eq!(rev((t[1] as u32 | (t[2] as u32) << 8) >> 11, 5), crc, "addr {:x}", addr);
    }
    let t = gen::usb_sof(0x710);
    assert_eq!(rev((t[1] as u32 | (t[2] as u32) << 8) >> 11, 5), 0x14);
    assert_eq!(rev(usb_crc16(&[0x00, 0x01, 0x02, 0x03]) as u32, 16), 0xf75e);
    assert_eq!(rev(usb_crc16(&[0x23, 0x45, 0x67, 0x89]) as u32, 16), 0x7038);
    // packets as bus analysers show them: SETUP to address 0, a zero-length DATA1, GET_DESCRIPTOR(device)
    assert_eq!(gen::usb_token(0x2D, 0, 0), [0x2D, 0x00, 0x10]);
    assert_eq!(gen::usb_data(0x4B, &[]), [0x4B, 0x00, 0x00]);
    assert_eq!(gen::usb_data(0xC3, &[0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x40, 0x00])[9..], [0xDD, 0x94]);
    // the residues of the spec over the field and its CRC: 01100 and 1000000000001101 (MSB first, before inversion)
    for t in [gen::usb_token(0x69, 5, 1), gen::usb_token(0xE1, 0x7f, 15), gen::usb_sof(0x2a5)] {
        assert_eq!(rev(usb_crc5(&lsb(&t[1..])) as u32 ^ 0x1f, 5), 0b01100);
    }
    for d in [gen::usb_data(0xC3, &[1, 2, 3]), gen::usb_data(0x4B, &[0xff; 9]), gen::usb_data(0x87, &[])] {
        assert_eq!(rev(usb_crc16(&d[1..]) as u32 ^ 0xffff, 16), 0x800d);
    }
    // PIDs: check nibble = complement of the type
    assert_eq!([1, 9, 5, 13, 3, 11, 2, 10, 14, 12].map(usb_pid), [0xE1, 0x69, 0xA5, 0x2D, 0xC3, 0x4B, 0xD2, 0x5A, 0x1E, 0x3C]);
    assert_eq!(usb_pid_name(0xC3), "DATA0");
    assert_eq!(usb_pid_name(0xC4), "?");
}

/// What `usb` shows for a packet (PID first) at `kbit`.
fn usb_want(bytes: &[u8], kbit: u32) -> Vec<(Kind, u32, bool)> {
    let mut w = vec![(F("SYNC"), kbit, true), (F("PID"), bytes[0] as u32, true)];
    let le = |a: usize| bytes[a] as u32 | (bytes[a + 1] as u32) << 8;
    match bytes[0] {
        0xA5 => w.extend([(F("FRAME"), le(1) & 0x7ff, true), (Kind::Crc, le(1) >> 11, true)]),
        0xE1 | 0x69 | 0x2D | 0xB4 => w.extend([(F("ADDR"), le(1) & 0x7f, true), (F("ENDP"), le(1) >> 7 & 15, true), (Kind::Crc, le(1) >> 11, true)]),
        0xC3 | 0x4B | 0x87 | 0x0F => {
            let n = bytes.len();
            w.extend(bytes[1..n - 2].iter().map(|&b| (Kind::Data, b as u32, true)));
            w.push((Kind::Crc, le(n - 2), true));
        }
        _ => {}
    }
    if bytes[0] != 0x3C {
        w.push((F("!EOP"), 0, true));
    }
    w
}

/// D+ and D- of USB traffic sampled every `dt` with 0.1 V of noise (0 / 3.3 V), and 5 us of idle after it.
fn usb_lines(items: &[gen::UsbItem], low: bool, skew: f64, dt: f64) -> (Vec<f32>, Vec<f32>) {
    let (p, m) = gen::usb(items, low, skew, 0.0, 3.3);
    let n = ((p.seg.last().map_or(0.0, |s| s.0) + 5e-6) / dt) as usize;
    let (mut a, mut b) = (sample(|t| p.v(t), 0.0, dt, n), sample(|t| m.v(t), 0.0, dt, n));
    add_noise(&mut a, 0.1, 11);
    add_noise(&mut b, 0.1, 12);
    (a, b)
}

/// Packets with `gap` of idle before each (s), after 1 us of idle.
fn usb_items(packets: &[Vec<u8>], rate: f64, gap: f64) -> Vec<gen::UsbItem> {
    let mut v = vec![gen::UsbItem::Idle(1e-6)];
    v.extend(packets.iter().flat_map(|p| [gen::UsbItem::Idle(gap), gen::UsbItem::packet(p, rate)]));
    v
}

fn fs_packets() -> Vec<Vec<u8>> {
    vec![
        gen::usb_sof(0x710),
        gen::usb_token(0x2D, 0x15, 0xe),
        gen::usb_data(0xC3, &[0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x40, 0x00]),
        vec![0xD2],
        gen::usb_token(0x69, 0x3a, 0xa),
        // stuffing all over
        gen::usb_data(0x4B, &[0xff, 0xff, 0xff, 0x7f, 0x3f, 0xfe, 0x00, 0xff, 0x80]),
        vec![0xD2],
        gen::usb_token(0xE1, 0x70, 4),
        gen::usb_data(0x87, &[]),
        vec![0x5A],
        vec![0x1E],
        vec![0x96],
        gen::usb_token(0xB4, 0x7f, 15),
        gen::usb_data(0x0F, &[1, 2, 3]),
    ]
}

#[test]
fn usb_full_speed_round_trip() {
    let pk = fs_packets();
    let want: Vec<_> = pk.iter().flat_map(|p| usb_want(p, 12000)).collect();
    // a nominal transmitter and one 0.25 % fast; 8 samples per bit; D- 15 ns late (crossover SE0 / SE1)
    for rate in [USB_FULL, USB_FULL * 1.0025] {
        // gaps of 2 bit times (J after the EOP) up to idle
        for gap in [1.0 / rate, 1e-6] {
            let dt = 1.0 / USB_FULL / 8.0;
            let (dp, dm) = usb_lines(&usb_items(&pk, rate, gap), false, 15e-9, dt);
            for speed in [UsbSpeed::Auto, UsbSpeed::Full] {
                let p = usb(&dp, Some(&dm), dt, speed, 1.65, 0.3);
                assert_eq!(kv(&p), want, "rate {} gap {} {:?}", rate, gap, speed);
                // D+ alone
                let p = usb(&dp, None, dt, speed, 1.65, 0.3);
                assert_eq!(kv(&p), want, "D+ alone: rate {} gap {} {:?}", rate, gap, speed);
            }
            // the low-speed setting reads the full-speed bus inverted: nothing
            assert!(usb(&dp, Some(&dm), dt, UsbSpeed::Low, 1.65, 0.3).iter().all(|x| x.kind != F("PID")));
        }
    }
    // field times: the SOF's SYNC and PID are 8 bits each, the frame number 11
    let dt = 1.0 / USB_FULL / 8.0;
    let (dp, dm) = usb_lines(&usb_items(&pk[..1], USB_FULL, 1e-6), false, 0.0, dt);
    let p = usb(&dp, Some(&dm), dt, UsbSpeed::Auto, 1.65, 0.3);
    for (k, bits) in [(0, 8.0), (1, 8.0), (2, 11.0), (3, 5.0), (4, 2.0)] {
        assert!((p[k].t1 - p[k].t0 - bits * 8.0).abs() < 1.0, "{:?}", p[k]);
    }
}

#[test]
fn usb_low_speed_keep_alive_reset_and_pre() {
    use gen::UsbItem::*;
    let bit = 1.0 / USB_LOW;
    let dt = bit / 10.0;
    let pk = [gen::usb_token(0x69, 0x05, 1), gen::usb_data(0x4B, &[0x12, 0x01, 0x10, 0x01, 0xff, 0x00, 0x00, 0x08]), vec![0xD2]];
    let want: Vec<_> = pk.iter().flat_map(|p| usb_want(p, 1500)).collect();
    // keep-alive, the transaction, a reset; a transmitter 1.5 % slow
    for rate in [USB_LOW, USB_LOW * 0.985] {
        let mut items = vec![Idle(5e-6), Se0(2.0 * bit), Idle(5e-6)];
        items.extend(usb_items(&pk, rate, 2.0 * bit));
        items.extend([Idle(5e-6), Se0(12e-6), Idle(3e-6)]);
        let (dp, dm) = usb_lines(&items, true, 100e-9, dt);
        for speed in [UsbSpeed::Auto, UsbSpeed::Low] {
            let p = usb(&dp, Some(&dm), dt, speed, 1.65, 0.3);
            let k = kv(&p);
            assert_eq!(k[0], (F("!EOP"), 0, true), "{:?}", speed);
            assert_eq!(k[1..k.len() - 1], want[..], "rate {} {:?}", rate, speed);
            assert_eq!(k[k.len() - 1], (F("!RESET"), 0, true));
            assert!((p[0].t1 - p[0].t0 - 2.0 * bit / dt).abs() < 1.0);
            // D+ alone: keep-alive and reset look like idle
            let p = usb(&dp, None, dt, speed, 1.65, 0.3);
            assert_eq!(kv(&p), want, "D+ alone: rate {} {:?}", rate, speed);
        }
        assert!(usb(&dp, Some(&dm), dt, UsbSpeed::Full, 1.65, 0.3).iter().all(|x| x.kind != F("PID")));
    }
    // PRE on a full-speed bus: the low-speed packets follow at 1.5 Mbit/s with full-speed polarity
    // (the hub setup: 4 full-speed bit times of J)
    let mut items = vec![Idle(2e-6), Packet { bits: gen::usb_stuff(&gen::usb_bits(&[0x3C])), rate: USB_FULL, eop: false }, Idle(4.0 / USB_FULL), gen::UsbItem::packet(&pk[0], USB_LOW)];
    items.extend(pk[1..].iter().flat_map(|p| [Idle(2.0 * bit), gen::UsbItem::packet(p, USB_LOW)]));
    items.extend(usb_items(&[gen::usb_sof(0x123)], USB_FULL, 1e-6));
    let dt = 1.0 / USB_FULL / 8.0;
    let (dp, dm) = usb_lines(&items, false, 0.0, dt);
    let mut want = usb_want(&[0x3C], 12000);
    want.extend(pk.iter().flat_map(|p| usb_want(p, 1500)));
    want.extend(usb_want(&gen::usb_sof(0x123), 12000));
    for speed in [UsbSpeed::Auto, UsbSpeed::Full] {
        assert_eq!(kv(&usb(&dp, Some(&dm), dt, speed, 1.65, 0.3)), want, "{:?}", speed);
        assert_eq!(kv(&usb(&dp, None, dt, speed, 1.65, 0.3)), want, "D+ alone {:?}", speed);
    }
}

#[test]
fn usb_crc_pid_stuff_and_length_errors() {
    use gen::UsbItem::*;
    let rate = USB_FULL;
    let dt = 1.0 / rate / 8.0;
    let mut tok = gen::usb_token(0x69, 0x05, 1);
    tok[2] ^= 0x40; // a CRC5 bit
    let mut dat = gen::usb_data(0xC3, &[1, 2, 3, 4]);
    dat[2] ^= 0x01; // a payload bit
    let good = gen::usb_data(0x4B, &[0xff; 4]);
    let pkt = |b: &[u8]| gen::UsbItem::packet(b, rate);
    let items = vec![
        Idle(1e-6),
        pkt(&tok),
        Idle(1e-6),
        pkt(&dat),
        Idle(1e-6),
        // a wrong check nibble
        pkt(&[0xC4, 0xAA, 0x55]),
        Idle(1e-6),
        // not stuffed: seven ones
        Packet { bits: gen::usb_bits(&good), rate, eop: true },
        Idle(1e-6),
        // a token two bytes long, a handshake with a byte more
        pkt(&[0x69, 0x05]),
        Idle(1e-6),
        pkt(&[0xD2, 0x00]),
        Idle(1e-6),
        pkt(&gen::usb_sof(5)),
        Idle(1e-6),
        // an EOP of 5 bit times
        Packet { bits: gen::usb_stuff(&gen::usb_bits(&[0x5A])), rate, eop: false },
        Se0(5.0 / rate),
        Idle(1e-6),
    ];
    let (dp, dm) = usb_lines(&items, false, 0.0, dt);
    let p = usb(&dp, Some(&dm), dt, UsbSpeed::Auto, 1.65, 0.3);
    let k = kv(&p);
    let mut want = usb_want(&tok, 12000);
    want[4].2 = false;
    let mut w = usb_want(&dat, 12000);
    w[6].2 = false;
    want.extend(w);
    want.extend([(F("SYNC"), 12000, true), (F("PID"), 0xC4, false), (Kind::Data, 0xAA, true), (Kind::Data, 0x55, true), (F("!EOP"), 0, true)]);
    // the rest of the packet is skipped up to its EOP
    want.extend([(F("SYNC"), 12000, true), (F("PID"), 0x4B, true), (F("!STUFF"), 0, false), (F("!EOP"), 0, true)]);
    want.extend([(F("SYNC"), 12000, true), (F("PID"), 0x69, true), (F("!ERR"), 0, false), (F("!EOP"), 0, true)]);
    want.extend([(F("SYNC"), 12000, true), (F("PID"), 0xD2, true), (F("!ERR"), 0, false), (F("!EOP"), 0, true)]);
    want.extend(usb_want(&gen::usb_sof(5), 12000));
    want.extend([(F("SYNC"), 12000, true), (F("PID"), 0x5A, true), (F("!EOP"), 0, false)]);
    assert_eq!(k, want);
    // the stuff error sits at the seventh one: 0x4B ends with a 0 (bit 7), then 0xff: data bit 6
    let s = p.iter().find(|x| x.kind == F("!STUFF")).unwrap();
    let pid = p.iter().find(|x| x.kind == F("PID") && x.value == 0x4B).unwrap();
    assert!((s.t0 - (pid.t1 + 6.0 * 8.0)).abs() < 1.0, "{:?} {:?}", s, pid);
    // D+ alone: the same up to the stuff error (D+ alone takes the seven ones for the idle)
    let p = usb(&dp, None, dt, UsbSpeed::Auto, 1.65, 0.3);
    let upto = want.iter().position(|x| x.0 == F("!STUFF")).unwrap() - 2;
    assert_eq!(kv(&p)[..upto], want[..upto]);
}

#[test]
fn usb_cut_packets_flat_lines_and_slow_sampling() {
    let pk = fs_packets();
    let dt = 1.0 / USB_FULL / 8.0;
    let (dp, dm) = usb_lines(&usb_items(&pk, USB_FULL, 1e-6), false, 0.0, dt);
    let full = usb(&dp, Some(&dm), dt, UsbSpeed::Auto, 1.65, 0.3);
    // a record from the middle of the DATA0 packet to the middle of the DATA1 packet: the whole packets between
    let d0 = full.iter().position(|x| x.kind == F("PID") && x.value == 0xC3).unwrap();
    let d1 = full.iter().position(|x| x.kind == F("PID") && x.value == 0x4B).unwrap();
    let (a, b) = (full[d0].t1 as usize + 40, full[d1].t1 as usize + 40);
    let want: Vec<_> = [vec![0xD2], gen::usb_token(0x69, 0x3a, 0xa)].iter().flat_map(|p| usb_want(p, 12000)).collect();
    // with both lines the EOP of the cut packet is still seen
    let k = kv(&usb(&dp[a..b], Some(&dm[a..b]), dt, UsbSpeed::Auto, 1.65, 0.3));
    assert_eq!(k[0], (F("!EOP"), 0, true));
    assert_eq!(k[1..], want[..]);
    assert_eq!(kv(&usb(&dp[a..b], None, dt, UsbSpeed::Auto, 1.65, 0.3)), want);
    // nothing on flat lines, too few samples per bit, or a bad dt
    assert!(usb(&vec![3.3; 5000], Some(&vec![0.0; 5000]), dt, UsbSpeed::Auto, 1.65, 0.3).is_empty());
    let (p4, m4): (Vec<f32>, Vec<f32>) = (dp.iter().step_by(4).copied().collect(), dm.iter().step_by(4).copied().collect());
    assert!(usb(&p4, Some(&m4), dt * 4.0, UsbSpeed::Auto, 1.65, 0.3).iter().all(|x| x.kind != F("PID")));
    assert!(usb(&dp, Some(&dm), 0.0, UsbSpeed::Auto, 1.65, 0.3).is_empty());
}


// ------------------------------------------------------------------------------------------------- robustness

// ------------------------------------------------------------------------------------------------- NMRA DCC

fn dcc_frames() -> Vec<DccFrame> {
    vec![
        DccFrame { address: 0x03, data: vec![0x3F], xor: None, preamble: 14 },
        DccFrame { address: 0x28, data: vec![0x00, 0x28], xor: None, preamble: 12 },
    ]
}

fn dcc_want(fr: &[DccFrame]) -> Vec<(Kind, u32, bool)> {
    let mut want = Vec::new();
    for f in fr {
        let mut payload = vec![f.address];
        payload.extend_from_slice(&f.data);
        let x = f.xor.unwrap_or_else(|| dcc_xor(&payload));
        want.push((F("PRE"), f.preamble.max(10), true));
        want.push((ID, f.address as u32, true));
        for &d in &f.data {
            want.push((Kind::Data, d as u32, true));
        }
        want.push((Kind::Crc, x as u32, f.xor.map_or(true, |g| g == dcc_xor(&payload))));
    }
    want
}

#[test]
fn dcc_xor_and_baseline_packets() {
    assert_eq!(dcc_xor(&[0x03, 0x3F]), 0x3C);
    assert_eq!(dcc_xor(&[0x28, 0x00, 0x28]), 0x00);
    let fr = dcc_frames();
    let dt = DCC_ONE_HALF / 8.0;
    let mut v = logic(&gen::dcc(&fr, 0.0, 3.3), dt, 200e-6);
    add_noise(&mut v, 0.05, 9);
    let p = dcc(&v, dt, 1.65, 0.3);
    assert_eq!(kv(&p), dcc_want(&fr));
    // nothing on a flat line or with too few samples per half-bit
    assert!(dcc(&vec![1.65; 5000], dt, 1.65, 0.3).is_empty());
    assert!(dcc(&v, 50e-6, 1.65, 0.3).is_empty());
}


#[test]
fn dcc_bad_xor_and_cut_packet() {
    let mut fr = dcc_frames();
    fr[0].xor = Some(0x3C ^ 0xFF);
    let dt = DCC_ONE_HALF / 8.0;
    let v = logic(&gen::dcc(&fr, 0.0, 3.3), dt, 200e-6);
    let p = dcc(&v, dt, 1.65, 0.3);
    let crcs: Vec<(u32, bool)> = p.iter().filter(|x| x.kind == Kind::Crc).map(|x| (x.value, x.ok)).collect();
    assert_eq!(crcs, [(0x3C ^ 0xFF, false), (0x00, true)]);
    // a record that ends inside the first packet: no partial decode
    let full = logic(&gen::dcc(&dcc_frames()[..1], 0.0, 3.3), dt, 200e-6);
    let cut = &full[..full.len() / 2];
    assert!(dcc(cut, dt, 1.65, 0.3).is_empty());
}

#[test]
fn garbage_never_panics() {
    let mut s = 0x0bad_cafe_1234_5678u64;
    let mut rnd = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for round in 0..12 {
        let n = [0usize, 1, 3, 100, 5000, 20000][round % 6];
        let mut v = Vec::with_capacity(n);
        while v.len() < n {
            let run = 1 + (rnd() % if round < 6 { 60 } else { 4 }) as usize;
            let lvl = [-5.0f32, 0.0, 5.0][(rnd() % 3) as usize];
            v.extend((0..run).map(|_| lvl));
        }
        v.truncate(n);
        if round % 3 == 2 {
            add_noise(&mut v, 4.0, round as u64 + 1);
        }
        if round == 11 && n > 0 {
            v[n / 2] = f32::NAN;
        }
        for dt in [1e-8, 1e-7, 1e-6, 0.0, f64::NAN] {
            let _ = arinc429(&v, dt, None, 2.5, 0.5);
            let _ = arinc429(&v, dt, Some(ARINC429_HIGH), -2.5, 0.5);
            let _ = arinc429(&v, dt, Some(f64::INFINITY), 2.5, 0.5);
            for nib in [0, 1, 6, 99] {
                let _ = sent(&v, dt, None, nib, 2.5, 0.5);
                let _ = sent(&v, dt, Some(3e-6), nib, 0.0, 0.5);
                let _ = sent(&v, dt, Some(0.0), nib, 2.5, 0.5);
            }
            for speed in [UsbSpeed::Auto, UsbSpeed::Low, UsbSpeed::Full] {
                let _ = usb(&v, Some(&v[v.len() / 3..]), dt, speed, 2.5, 0.5);
                let _ = usb(&v, None, dt, speed, 2.5, 0.5);
                let w: Vec<f32> = v.iter().rev().copied().collect();
                let _ = usb(&v, Some(&w), dt, speed, 0.0, 0.5);
            }
            let _ = dcc(&v, dt, 1.65, 0.3);
            let _ = dcc(&v, dt, 0.0, 0.5);
        }
    }
}
