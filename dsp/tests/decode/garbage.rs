// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Every decoder on noise, garbage and degenerate input: it must not panic.
//!
//! A new decoder of any family gets its call here, in the test of the buses it is closest to.

use super::*;

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
            v.extend(std::iter::repeat_n(lvl, len));
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
        let kinds = [
            Kind::Start,
            Kind::RepStart,
            Kind::Stop,
            Kind::Addr { read: false },
            Kind::Addr { read: true },
            Kind::Data,
            Kind::Ack,
        ];
        let pk: Vec<Packet> = (0..3000)
            .map(|i| {
                let k = kinds[(r() % kinds.len() as u32) as usize];
                let value = if matches!(k, Kind::Addr { .. }) { 0x18 + r() % 8 } else { r() % 256 };
                Packet { t0: i as f64, t1: i as f64 + 0.5, kind: k, value, value2: None, ok: r().is_multiple_of(2) }
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

#[test]
fn garbage_never_panics_on_debug_and_media_buses() {
    let mut s = 0x1234_5678_9abc_def1u64;
    let mut rnd = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for round in 0..12 {
        let n = [0usize, 1, 3, 100, 5000, 20000][round % 6];
        // random logic with random run lengths, or pure noise
        let mut chan = || -> Vec<f32> {
            let mut v = Vec::with_capacity(n);
            let mut lv = false;
            while v.len() < n {
                let run = 1 + (rnd() % if round < 6 { 40 } else { 3 }) as usize;
                lv = rnd() % 2 == 0 || !lv;
                v.extend((0..run).map(|_| if lv { 3.3 } else { 0.0 }));
            }
            v.truncate(n);
            if round % 3 == 2 {
                v.iter_mut().for_each(|x| *x = 1.65);
                add_noise(&mut v, 2.0, round as u64 + 1);
            }
            if round == 11 && n > 0 {
                v[n / 2] = f32::NAN;
            }
            v
        };
        let c: Vec<Vec<f32>> = (0..6).map(|_| chan()).collect();
        let _ = swd(&c[0], &c[1], 1.65, 0.2);
        let _ = jtag(&c[0], &c[1], &c[2], Some(&c[3]), None, 1.65, 0.2);
        let _ = jtag(&c[0], &c[1], &c[2], None, Some(TapState::ShiftDr), 1.65, 0.2);
        let io: Vec<&[f32]> = c[2..6].iter().map(|v| v.as_slice()).collect();
        let _ = qspi(&c[0], &c[1], &io, &QspiCfg::new(1, 4, 4, 4, 6, true), true, 1.65, 0.2);
        let _ = qspi(&c[0], &c[1], &io[..1], &QspiCfg::new(3, 0, 9, 9, 1000, true), false, 1.65, 0.2);
        let _ = qspi(&c[0], &c[1], &[], &QspiCfg::new(4, 4, 4, 3, 0, false), true, 1.65, 0.2);
        let _ = i3c(&c[0], &c[1], &[0x08, 0x7e], 1.65, 0.2);
        let _ = spdif(&c[0], 1.65, 0.2);
        let _ = spdif_channel_status(&spdif(&c[2], 1.65, 0.2));
        for dt in [1e-9, 1e-7, 0.0, -1.0, f64::NAN] {
            let _ = usbpd(&c[0], dt, 1.65, 0.2);
        }
    }
    for x in 0..=255u8 {
        let _ = (qspi_opcode_name(x), qspi_opcode_cfg(x, x % 2 == 0, 3, 6), i3c_ccc_name(x));
    }
    for k in 0..2000u32 {
        let h = (rnd() & 0xffff) as u16;
        let _ = (usbpd_msg_name(h), usbpd_pdo(rnd() as u32 ^ k));
    }
}

#[test]
fn garbage_never_panics_on_avionics_usb_and_dcc() {
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
