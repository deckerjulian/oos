// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Identify against generated signals: DC, noise and periodic shapes, clocks and pulse trains, modulation, serial
//! buses and their roles, and video. A new kind of signal Identify names gets its test here.

use super::*;

// ------------------------------------------------------------------------------------------------ identify

fn top(ch: &[&[f32]], dt: f64) -> Finding {
    let f = identify::identify(ch, dt);
    assert!(!f.is_empty());
    f.into_iter().next().unwrap()
}

#[test]
fn identify_dc_noise_and_sine() {
    let dt = 1e-6;
    let mut dc = vec![1.5f32; 10_000];
    add_noise(&mut dc, 0.001, 1);
    let f = top(&[&dc], dt);
    assert!(matches!(f.kind, SignalKind::Dc { level } if close(level, 1.5, 0.01)), "{:?}", f);
    assert_eq!(f.trigger, TriggerHint::None);
    let mut g = Gauss(2);
    let n: Vec<f32> = (0..50_000).map(|_| g.next(0.2) as f32).collect();
    let f = top(&[&n], dt);
    assert!(matches!(f.kind, SignalKind::Noise { rms } if close(rms, 0.2, 0.01)), "{:?}", f);
    let mut s = sample(|t| 0.8 * (2.0 * PI * 12_500.0 * t).sin() + 0.2, 0.0, dt, 20_000);
    add_noise(&mut s, 0.01, 3);
    let f = top(&[&s], dt);
    assert!(
        matches!(f.kind, SignalKind::Sine { freq, amp } if close(freq, 12_500.0, 10.0) && close(amp, 0.8, 0.02)),
        "{:?}",
        f
    );
    assert!(
        matches!(f.trigger, TriggerHint::Edge { source: 0, level, rising: true, .. } if close(level as f64, 0.2, 0.05)),
        "{:?}",
        f
    );
    assert!(f.summary.starts_with("Sine 12.50 kHz"), "{}", f.summary);
}

#[test]
fn identify_mains_triangle_sawtooth() {
    let dt = 20e-6;
    let m = sample(|t| 325.0 * (2.0 * PI * 50.0 * t).sin(), 0.0, dt, 10_000);
    let f = top(&[&m], dt);
    assert!(
        matches!(f.kind, SignalKind::Mains { freq, vrms } if close(freq, 50.0, 0.1) && close(vrms, 229.8, 1.0)),
        "{:?}",
        f
    );
    let dt = 1e-6;
    let tri = sample(
        |t| {
            let x = (t * 1000.0).fract();
            if x < 0.5 {
                4.0 * x - 1.0
            } else {
                3.0 - 4.0 * x
            }
        },
        0.0,
        dt,
        20_000,
    );
    let f = top(&[&tri], dt);
    assert!(
        matches!(f.kind, SignalKind::Triangle { freq, amp } if close(freq, 1000.0, 1.0) && close(amp, 1.0, 0.02)),
        "{:?}",
        f
    );
    let saw = sample(|t| 2.0 * (t * 1000.0).fract() - 1.0, 0.0, dt, 20_000);
    let f = top(&[&saw], dt);
    assert!(matches!(f.kind, SignalKind::Sawtooth { freq, rising: true, .. } if close(freq, 1000.0, 1.0)), "{:?}", f);
    let saw: Vec<f32> = saw.iter().map(|x| -x).collect();
    assert!(matches!(top(&[&saw], dt).kind, SignalKind::Sawtooth { rising: false, .. }));
}

#[test]
fn identify_clock_pwm_and_glitchy_pulse_train() {
    let dt = 1e-8;
    let clk = PulseTrain::square(1e6, 0.0, 3.3, 10e-9);
    let mut v = sample(|t| clk.v(t), 0.0, dt, 5000);
    add_noise(&mut v, 0.05, 4);
    let f = top(&[&v], dt);
    assert!(
        matches!(f.kind, SignalKind::Clock { freq, duty, lo, hi } if close(freq, 1e6, 1e3) && close(duty, 0.5, 0.02) && close(lo, 0.0, 0.05) && close(hi, 3.3, 0.05)),
        "{:?}",
        f
    );
    assert!(matches!(f.trigger, TriggerHint::Edge { level, rising: true, .. } if close(level as f64, 1.65, 0.05)));
    // PWM 10 kHz, duty 20 .. 80 % (the first period starts before the record, the last one is incomplete)
    let fr = 10e3;
    let v = sample(
        |t| {
            let k = (t * fr).floor();
            let d = 0.2 + 0.6 * (k / 19.0).min(1.0);
            if t * fr - k < d {
                5.0
            } else {
                0.0
            }
        },
        0.0,
        1e-7,
        20_000,
    );
    let f = top(&[&v], 1e-7);
    assert!(
        matches!(f.kind, SignalKind::Pwm { freq, duty_min, duty_max } if close(freq, fr, 10.0) && close(duty_min, 0.2 + 0.6 / 19.0, 0.01) && close(duty_max, 0.2 + 0.6 * 18.0 / 19.0, 0.01)),
        "{:?}",
        f
    );
    // narrow pulses: 5 % duty
    let p = PulseTrain { width: 0.05e-3, ..PulseTrain::square(1e3, 0.0, 5.0, 1e-7) };
    let v = sample(|t| p.v(t), 0.0, 1e-6, 20_000);
    let f = top(&[&v], 1e-6);
    assert!(
        matches!(f.kind, SignalKind::PulseTrain { freq, width, positive: true, anomalies: 0 } if close(freq, 1e3, 1.0) && close(width, 50e-6, 2e-6)),
        "{:?}",
        f
    );
    // every 5th pulse is a glitch of 5 % of the width: pulse width trigger below half the normal width
    let p = PulseTrain {
        width: 0.3e-3,
        every: 5,
        at: 2,
        special: sim::Pulse { width: 0.015e-3, height: 1.0 },
        ..PulseTrain::square(1e3, 0.0, 5.0, 1e-7)
    };
    let v = sample(|t| p.v(t), 0.0, 1e-6, 20_000);
    let f = top(&[&v], 1e-6);
    assert!(matches!(f.kind, SignalKind::PulseTrain { anomalies: 4, positive: true, .. }), "{:?}", f);
    assert!(
        matches!(f.trigger, TriggerHint::PulseWidth { positive: true, less_than: Some(w), .. } if close(w, 0.15e-3, 0.01e-3)),
        "{:?}",
        f
    );
}

#[test]
fn identify_am_fm_and_bursts() {
    let dt = 1e-6;
    let v = sample(|t| (1.0 + 0.5 * (2.0 * PI * 500.0 * t).sin()) * (2.0 * PI * 10e3 * t).sin(), 0.0, dt, 100_000);
    let f = top(&[&v], dt);
    assert!(
        matches!(f.kind, SignalKind::Am { carrier, modulation, depth } if close(carrier, 10e3, 20.0) && close(modulation, 500.0, 10.0) && close(depth, 0.5, 0.05)),
        "{:?}",
        f
    );
    let v = sample(|t| (2.0 * PI * 10e3 * t + 5.0 * (2.0 * PI * 200.0 * t).sin()).sin(), 0.0, dt, 100_000);
    let f = top(&[&v], dt);
    assert!(
        matches!(f.kind, SignalKind::Fm { carrier, modulation, deviation } if close(carrier, 10e3, 300.0) && close(modulation, 200.0, 5.0) && close(deviation, 1000.0, 100.0)),
        "{:?}",
        f
    );
    // 20 cycles of 100 kHz every 1 ms
    let dt = 1e-7;
    let v = sample(|t| if t % 1e-3 < 0.2e-3 { (2.0 * PI * 100e3 * t).sin() } else { 0.0 }, 0.0, dt, 50_000);
    let f = top(&[&v], dt);
    assert!(
        matches!(f.kind, SignalKind::Burst { freq, length, period } if close(freq, 100e3, 5e3) && close(length, 0.2e-3, 0.01e-3) && close(period, 1e-3, 0.01e-3)),
        "{:?}",
        f
    );
    assert!(matches!(f.trigger, TriggerHint::Edge { holdoff, .. } if holdoff > 0.2e-3 && holdoff < 0.8e-3), "{:?}", f);
}

#[test]
fn identify_uart_and_inverted_uart() {
    let baud = 115_200.0;
    let l = sim::uart_logic(b"Hello, OOS! 0123456789", baud, 8, None, 1.0, 0.0, 3.3);
    let dt = 1.0 / baud / 20.0;
    let n = (25.0 * 12.0 * 20.0) as usize + 400;
    let mut v = sample(|t| l.v(t), 0.0, dt, n);
    add_noise(&mut v, 0.05, 6);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Uart { baud: b, idle_high: true } if b == baud), "{:?}", f);
    assert!(
        matches!(f.decoder, Some(DecoderHint::Uart { ch: 0, baud: b, bits: 8, parity: Parity::None, invert: false, .. }) if b == baud),
        "{:?}",
        f
    );
    assert!(matches!(f.trigger, TriggerHint::Edge { rising: false, .. }));
    assert!(f.confidence > 0.8);
    assert_eq!(identify::estimate_baud(&v, dt, 1.65, 0.3), Some(baud));
    // RS-232 line side: idle low
    let l = sim::uart_logic(b"\x00\xffAbc", 9600.0, 8, None, 2.0, 5.0, -5.0);
    let dt = 1.0 / 9600.0 / 16.0;
    let v = sample(|t| l.v(t), 0.0, dt, 16 * 80);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Uart { baud: b, idle_high: false } if b == 9600.0), "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::Uart { invert: true, .. })));
    // 0x55 sync bytes with half-bit idle gaps: nearly periodic, but a decoder-confirmed UART at exactly 1 MBd
    let l = sim::uart_logic(&[0x55; 40], 1e6, 8, None, 0.5, 0.0, 3.3);
    let dt = 1e-8;
    let v = sample(|t| l.v(t), 0.0, dt, 50_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Uart { baud: b, idle_high: true } if b == 1e6), "{:?}", f);
}

#[test]
fn identify_can_lin_onewire_manchester() {
    let frames = [
        sim::CanFrame { id: 0x123, ext: false, rtr: false, data: vec![1, 2, 3, 4] },
        sim::CanFrame { id: 0x7e0, ext: false, rtr: false, data: vec![0x55; 8] },
    ];
    let l = sim::can_logic(&frames, 500e3, 0.0, 3.3);
    let dt = 1.0 / 25e6;
    let v = sample(|t| l.v(t), 0.0, dt, 15_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Can { bitrate, dominant_low: true } if bitrate == 500e3), "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::Can { bitrate, invert: false, .. }) if bitrate == 500e3));
    let l = sim::lin_logic(&[(0x10, vec![1, 2, 3], false), (0x3c, vec![0xaa, 0x55], true)], 19200.0, 0.0, 12.0);
    let dt = 1e-6;
    let v = sample(|t| l.v(t), 0.0, dt, 9_000);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Lin { baud } if baud == 19200.0), "{:?}", f);
    // 1-Wire: reset 480 us, presence, then 16 bit slots (1: 6 us low, 0: 60 us low; slots of 70 us)
    let mut ow = Logic::new(0.0, 3.3, 0.5e-6);
    ow.push(0.0, true);
    let mut t = 50e-6;
    ow.push(t, false);
    t += 480e-6;
    ow.push(t, true);
    t += 70e-6;
    ow.push(t, false);
    t += 120e-6;
    ow.push(t, true);
    t += 300e-6;
    for k in 0..16 {
        let low = if (0xa5c3u16 >> k) & 1 == 1 { 6e-6 } else { 60e-6 };
        ow.push(t, false);
        ow.push(t + low, true);
        t += 70e-6;
    }
    let v = sample(|x| ow.v(x), 0.0, 0.5e-6, ((t + 100e-6) / 0.5e-6) as usize);
    let f = top(&[&v], 0.5e-6);
    assert_eq!(f.kind, SignalKind::OneWire, "{:?}", f);
    assert!(matches!(f.decoder, Some(DecoderHint::OneWire { ch: 0, .. })));
    // Manchester (IEEE 802.3: 1 = low -> high), 10 kbit/s
    let br = 10e3;
    let mut half = Vec::new();
    for b in prbs7(300) {
        half.push(!b);
        half.push(b);
    }
    let l = sim::bits_logic(&half, 0.5 / br, 4, 0.0, 3.3);
    let dt = 1e-6;
    let v = sample(|t| l.v(t), 0.0, dt, (310.0 / br / dt) as usize);
    let f = top(&[&v], dt);
    assert!(matches!(f.kind, SignalKind::Manchester { bitrate } if bitrate == br), "{:?}", f);
}

#[test]
fn identify_i2c_pair_among_other_channels() {
    let x = [
        sim::I2cXfer { addr: 0x50, read: false, data: vec![0x00, 0x10], repeated: false, stop: false },
        sim::I2cXfer { addr: 0x50, read: true, data: vec![0xde, 0xad, 0xbe], repeated: true, stop: true },
    ];
    let (sda, scl) = sim::i2c_logic(&x, 100e3, 0.0, 3.3);
    let dt = 1e-7;
    let n = 70_000;
    let (a, b) = (sample(|t| sda.v(t), 0.0, dt, n), sample(|t| scl.v(t), 0.0, dt, n));
    let s = sample(|t| (2.0 * PI * 3e3 * t).sin(), 0.0, dt, n);
    let f = identify::identify(&[&s, &b, &a], dt);
    let i2c = f.iter().find(|x| matches!(x.kind, SignalKind::I2c { .. })).expect("I2C");
    assert!(matches!(i2c.kind, SignalKind::I2c { rate } if rate == 100e3), "{:?}", i2c);
    assert_eq!(i2c.decoder, Some(DecoderHint::I2c { sda: 2, scl: 1, thr: 1.65 }));
    assert!(matches!(i2c.trigger, TriggerHint::Edge { source: 2, rising: false, .. }));
    assert_eq!(i2c.channels, vec![2, 1]);
    assert_eq!(f.len(), 2);
    assert!(f.iter().any(|x| matches!(x.kind, SignalKind::Sine { .. }) && x.channels == vec![0]));
}

#[test]
fn identify_spi_roles_and_mode() {
    for mode in [0u8, 3] {
        let words = [(0xa5u32, 0x3c), (0x0f, 0xf0), (0x81, 0x7e), (0x5a, 0x00)];
        let [cs, clk, mosi, miso] = sim::spi_logic(&words, 8, mode, 1e6, 0.0, 3.3);
        let dt = 2e-8;
        let n = 2500;
        let s = |l: &Logic| sample(|t| l.v(t), 0.0, dt, n);
        let (cs, clk, mosi, miso) = (s(&cs), s(&clk), s(&mosi), s(&miso));
        let f = identify::identify(&[&mosi, &cs, &clk, &miso], dt);
        let spi = f
            .iter()
            .find(|x| matches!(x.kind, SignalKind::Spi { .. }))
            .unwrap_or_else(|| panic!("mode {}: {:?}", mode, f));
        assert!(
            matches!(spi.kind, SignalKind::Spi { rate, mode: m, bits: 8 } if rate == 1e6 && m == mode),
            "{:?}",
            spi
        );
        match &spi.decoder {
            Some(DecoderHint::Spi { clk: 2, data, cs: Some(1), mode: m, bits: 8, .. }) => {
                assert_eq!(*m, mode);
                assert!(data.contains(&0) && data.contains(&3), "{:?}", data);
            }
            d => panic!("{:?}", d),
        }
        assert_eq!(f.len(), 1, "{:?}", f);
    }
}

#[test]
fn identify_video_pal_and_ntsc() {
    for (std, ntsc) in [(sim::VideoStd::Pal, false), (sim::VideoStd::Ntsc, true)] {
        let dt = 0.1e-6;
        let t0 = sim::video_line_time(std, 1, 20);
        let v = sample(|t| sim::video(std, t), t0, dt, 20_000);
        let f = top(&[&v], dt);
        assert!(
            matches!(f.kind, SignalKind::Video { ntsc: n, line_rate } if n == ntsc && close(line_rate, 1.0 / std.line(), 5.0)),
            "{:?}",
            f
        );
        assert_eq!(f.trigger, TriggerHint::Video { source: 0, ntsc });
    }
}

#[test]
fn identify_bad_input() {
    assert!(identify::identify(&[], 1e-6).is_empty());
    assert!(identify::identify(&[&[]], 1e-6).is_empty());
    assert!(identify::identify(&[&[1.0; 8]], 1e-6).is_empty());
    assert!(identify::identify(&[&[0.0; 100]], 0.0).is_empty());
    let f = identify::identify(&[&[f32::NAN; 1000]], 1e-6);
    assert!(f.len() <= 1);
    let step: Vec<f32> = (0..1000).map(|k| if k < 500 { 0.0 } else { 1.0 }).collect();
    assert!(matches!(top(&[&step], 1e-6).kind, SignalKind::Step { rising: true, .. }));
    assert_eq!(identify::snap_rate(9500.0), 9600.0);
    assert_eq!(identify::snap_rate(12345.0), 12345.0);
}
