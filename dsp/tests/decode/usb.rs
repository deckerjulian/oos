// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The USB low and full speed decoders against generated signals with known content.
//!
//! The tests of a new decoder of the family go here, as the decoder goes into the family's file in `dsp/src/decode/`;
//! its protocol module, registered in `ui/src/protocols/mod.rs` (ADR 0002), has its own tests in the ui.

use super::*;

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
    assert_eq!(
        [1, 9, 5, 13, 3, 11, 2, 10, 14, 12].map(usb_pid),
        [0xE1, 0x69, 0xA5, 0x2D, 0xC3, 0x4B, 0xD2, 0x5A, 0x1E, 0x3C]
    );
    assert_eq!(usb_pid_name(0xC3), "DATA0");
    assert_eq!(usb_pid_name(0xC4), "?");
}

/// What `usb` shows for a packet (PID first) at `kbit`.
fn usb_want(bytes: &[u8], kbit: u32) -> Vec<(Kind, u32, bool)> {
    let mut w = vec![(F("SYNC"), kbit, true), (F("PID"), bytes[0] as u32, true)];
    let le = |a: usize| bytes[a] as u32 | (bytes[a + 1] as u32) << 8;
    match bytes[0] {
        0xA5 => w.extend([(F("FRAME"), le(1) & 0x7ff, true), (Kind::Crc, le(1) >> 11, true)]),
        0xE1 | 0x69 | 0x2D | 0xB4 => w.extend([
            (F("ADDR"), le(1) & 0x7f, true),
            (F("ENDP"), le(1) >> 7 & 15, true),
            (Kind::Crc, le(1) >> 11, true),
        ]),
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
    let pk = [
        gen::usb_token(0x69, 0x05, 1),
        gen::usb_data(0x4B, &[0x12, 0x01, 0x10, 0x01, 0xff, 0x00, 0x00, 0x08]),
        vec![0xD2],
    ];
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
    let mut items = vec![
        Idle(2e-6),
        Packet { bits: gen::usb_stuff(&gen::usb_bits(&[0x3C])), rate: USB_FULL, eop: false },
        Idle(4.0 / USB_FULL),
        gen::UsbItem::packet(&pk[0], USB_LOW),
    ];
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
    want.extend([
        (F("SYNC"), 12000, true),
        (F("PID"), 0xC4, false),
        (Kind::Data, 0xAA, true),
        (Kind::Data, 0x55, true),
        (F("!EOP"), 0, true),
    ]);
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
    let (p4, m4): (Vec<f32>, Vec<f32>) =
        (dp.iter().step_by(4).copied().collect(), dm.iter().step_by(4).copied().collect());
    assert!(usb(&p4, Some(&m4), dt * 4.0, UsbSpeed::Auto, 1.65, 0.3).iter().all(|x| x.kind != F("PID")));
    assert!(usb(&dp, Some(&dm), 0.0, UsbSpeed::Auto, 1.65, 0.3).is_empty());
}
