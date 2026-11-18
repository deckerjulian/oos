// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! UART (with automatic bit rate and 9-bit multidrop / RS-485), I2C and SPI.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{Kind, Packet, Parity};
use crate::edges::{edges, states};
use crate::protocols::{every_word_is_a_frame, BusWhat, Lines, ModuleInfo, OPTS};

/// The baud rates of the Decode menu, as the options of a module choose them.
pub const BAUDS: [f64; 10] = [1200.0, 2400.0, 4800.0, 9600.0, 19200.0, 38400.0, 57600.0, 115200.0, 460800.0, 921600.0];

/// The baud rate of option row `k` ([`BAUDS`]).
pub fn opt_baud(opt: &[usize; OPTS], k: usize) -> f64 {
    BAUDS[opt[k].min(BAUDS.len() - 1)]
}

/// UART on one line or an RS-485 pair.
pub static UART: ModuleInfo = ModuleInfo::new("Uart", "UART", "serial")
    .differential()
    .trigger(every_word_is_a_frame)
    .bit(|_, sh| 1.0 / sh.baud)
    .decoder(|l| Ok(uart(l.main(), l.dt, l.sh.baud, l.sh.bits, l.sh.parity, l.thr, l.hy, l.sh.uart_inv)));

/// UART: start bit, `bits` data bits LSB first, parity, 1 stop bit; idle high (`invert` = idle low). Samples each
/// bit in its middle, timed from the start-bit edge.
pub fn uart(
    v: &[f32],
    dt: f64,
    baud: f64,
    bits: u32,
    parity: Parity,
    thr: f32,
    hyst: f32,
    invert: bool,
) -> Vec<Packet> {
    let spb = 1.0 / baud / dt;
    let mut out = Vec::new();
    if spb < 2.5 {
        return out;
    }
    let st = states(v, thr, hyst);
    let level = |x: f64| -> Option<bool> { st.get(x as usize).copied().flatten().map(|b| b != invert) };
    let e = edges(v, thr, hyst);
    let nb = bits as f64 + if parity == Parity::None { 0.0 } else { 1.0 };
    let mut busy_until = 0.0;
    for x in &e {
        // a start bit: transition to the active (low) level, not inside the previous frame
        if x.rising != invert || x.t < busy_until {
            continue;
        }
        let s = x.t;
        if level(s + 0.5 * spb) != Some(false) {
            continue;
        }
        let mut val = 0u32;
        let mut ones = 0;
        let mut complete = true;
        for b in 0..bits {
            match level(s + (1.5 + b as f64) * spb) {
                Some(true) => {
                    val |= 1 << b;
                    ones += 1;
                }
                Some(false) => {}
                None => complete = false,
            }
        }
        let par_ok = match parity {
            Parity::None => true,
            p => match level(s + (1.5 + bits as f64) * spb) {
                Some(pb) => (ones + pb as u32).is_multiple_of(2) == (p == Parity::Even),
                None => {
                    complete = false;
                    false
                }
            },
        };
        let stop = level(s + (1.5 + nb) * spb);
        if !complete || stop.is_none() {
            break;
        }
        out.push(Packet::new(Kind::Data, s, s + (2.0 + nb) * spb, val, stop == Some(true) && par_ok));
        busy_until = s + (1.0 + nb + 0.5) * spb;
    }
    out
}

/// I2C on SDA and SCL.
pub static I2C: ModuleInfo = ModuleInfo::new("I2c", "I2C", "serial")
    .roles([Some("SDA"), Some("SCL"), None, None])
    .starts_on(BusWhat::Id, 0x50)
    .bit(|_, _| 2.5e-6)
    .decoder(|l| Ok(i2c(l.main(), l.need(1, "source channel off")?, l.thr, l.hy)));

/// I2C: START/STOP from SDA edges while SCL is high, bits sampled at SCL rising edges, 9th bit = ACK (low).
/// One threshold and hysteresis for both lines.
pub fn i2c(sda: &[f32], scl: &[f32], thr: f32, hyst: f32) -> Vec<Packet> {
    i2c_at(sda, scl, thr, hyst, thr, hyst)
}

/// I2C with a threshold and hysteresis of its own on the data line and on the clock.
pub fn i2c_at(sda: &[f32], scl: &[f32], sda_thr: f32, sda_hyst: f32, scl_thr: f32, scl_hyst: f32) -> Vec<Packet> {
    let scl_s = states(scl, scl_thr, scl_hyst);
    let sda_s = states(sda, sda_thr, sda_hyst);
    let mut ev: Vec<(f64, u8)> = Vec::new(); // 0 = SCL rising, 1 = SDA falling, 2 = SDA rising
    ev.extend(edges(scl, scl_thr, scl_hyst).iter().filter(|e| e.rising).map(|e| (e.t, 0)));
    ev.extend(edges(sda, sda_thr, sda_hyst).iter().map(|e| (e.t, if e.rising { 2 } else { 1 })));
    ev.sort_by(|a, b| a.0.total_cmp(&b.0));
    let at = |s: &Vec<Option<bool>>, t: f64| s.get((t + 0.5) as usize).copied().flatten().unwrap_or(true);
    let mut out = Vec::new();
    let mut in_frame = false;
    let mut first = true; // next byte is the address
    let (mut bits, mut nbits, mut t0) = (0u32, 0u32, 0.0);
    for (t, k) in ev {
        match k {
            1 | 2 if at(&scl_s, t) => {
                if k == 1 {
                    out.push(Packet::new(if in_frame { Kind::RepStart } else { Kind::Start }, t, t, 0, true));
                    in_frame = true;
                    first = true;
                } else {
                    out.push(Packet::new(Kind::Stop, t, t, 0, true));
                    in_frame = false;
                }
                nbits = 0;
                bits = 0;
            }
            0 if in_frame => {
                let b = at(&sda_s, t);
                if nbits == 0 {
                    t0 = t;
                }
                if nbits < 8 {
                    bits = bits << 1 | b as u32;
                    nbits += 1;
                } else {
                    let ack = !b;
                    let p = if first {
                        Packet::new(Kind::Addr { read: bits & 1 == 1 }, t0, t, bits >> 1, ack)
                    } else {
                        Packet::new(Kind::Data, t0, t, bits, ack)
                    };
                    out.push(p);
                    first = false;
                    nbits = 0;
                    bits = 0;
                }
            }
            _ => {}
        }
    }
    out
}

/// SPI: data, clock and an optional chip select.
pub static SPI: ModuleInfo = ModuleInfo::new("Spi", "SPI", "serial")
    .roles([Some("MOSI"), Some("Clock"), Some("CS"), None])
    .trigger(every_word_is_a_frame)
    .bit(|_, _| 0.5e-6)
    .decoder(spi_lines);

/// SPI on the lines of its module, with the shared rows: a clock pause of the trigger's timeout starts a frame (else
/// 20 samples), and an active-high CS is mirrored at the threshold, since `spi` takes CS active low.
fn spi_lines(l: &Lines) -> Result<Vec<Packet>, &'static str> {
    let clk = l.need(1, "source channel off")?;
    let bit = 1e-9f64.max(l.dt);
    let gap = if l.sh.spi_timeout > 0.0 { l.sh.spi_timeout / l.dt } else { 20.0 * bit / l.dt };
    let cs_hi: Option<Vec<f32>> = match (l.sh.spi_cs_high, l.lines[2]) {
        (true, Some(c)) => Some(c.iter().map(|&x| 2.0 * l.thr - x).collect()),
        _ => None,
    };
    let cs = cs_hi.as_deref().or(l.lines[2]);
    Ok(spi(clk, l.main(), None, cs, l.sh.spi_mode, l.sh.bits, true, l.thr, l.hy, gap))
}

/// SPI: words of `bits` bits sampled on the capture edge of `mode` (0/3: rising, 1/2: falling); a word ends after
/// `bits` edges; `cs` (active low) delimits frames, without CS a clock pause longer than `gap` samples restarts.
pub fn spi(
    clk: &[f32],
    mosi: &[f32],
    miso: Option<&[f32]>,
    cs: Option<&[f32]>,
    mode: u8,
    bits: u32,
    msb_first: bool,
    thr: f32,
    hyst: f32,
    gap: f64,
) -> Vec<Packet> {
    let sample_rising = matches!(mode, 0 | 3);
    let mo = states(mosi, thr, hyst);
    let mi = miso.map(|m| states(m, thr, hyst));
    let cs_e = cs.map(|c| edges(c, thr, hyst)).unwrap_or_default();
    let cs_s = cs.map(|c| states(c, thr, hyst));
    let at = |s: &Vec<Option<bool>>, t: f64| s.get((t + 0.5) as usize).copied().flatten().unwrap_or(false);
    let mut out = Vec::new();
    let (mut a, mut b, mut n, mut t0, mut last) = (0u32, 0u32, 0u32, 0.0, f64::NEG_INFINITY);
    let push_bit = |w: u32, bit: bool, n: u32| if msb_first { w << 1 | bit as u32 } else { w | (bit as u32) << n };
    for e in edges(clk, thr, hyst) {
        if e.rising != sample_rising {
            continue;
        }
        let selected = cs_s.as_ref().is_none_or(|s| !at(s, e.t));
        if !selected {
            n = 0;
            continue;
        }
        // frame boundary: CS edge or clock pause since the last bit
        let restart =
            if cs.is_some() { cs_e.iter().any(|c| !c.rising && c.t > last && c.t < e.t) } else { e.t - last > gap };
        if restart {
            n = 0;
        }
        if n == 0 {
            t0 = e.t;
            a = 0;
            b = 0;
        }
        a = push_bit(a, at(&mo, e.t), n);
        if let Some(m) = &mi {
            b = push_bit(b, at(m, e.t), n);
        }
        n += 1;
        last = e.t;
        if n == bits {
            out.push(Packet { value2: mi.as_ref().map(|_| b), ..Packet::new(Kind::Data, t0, e.t, a, true) });
            n = 0;
        }
    }
    out
}

/// Standard UART bit rates `uart_baud` snaps to (incl. MIDI 31250 and DMX512 250000).
pub const STD_BAUD: [f64; 25] = [
    300.0, 600.0, 1200.0, 2400.0, 4800.0, 9600.0, 14400.0, 19200.0, 28800.0, 31250.0, 38400.0, 57600.0, 76800.0,
    115200.0, 230400.0, 250000.0, 460800.0, 500000.0, 921600.0, 1e6, 1.5e6, 2e6, 2.5e6, 3e6, 4e6,
];

/// Bit rate of an asynchronous serial signal: the shortest pulse width that occurs consistently (at least 2 pulses
/// and 5 % of all, so single glitches do not count) is the first estimate of a bit time; it is refined by a least
/// squares fit over all pulses that are near integer multiples of it (1..12 bits). The result is snapped to the
/// nearest `STD_BAUD` within ±3 %, else returned as estimated. None without enough pulses or below 2 samples/bit.
pub fn uart_baud(v: &[f32], dt: f64, thr: f32, hyst: f32) -> Option<f64> {
    let e = edges(v, thr, hyst);
    let w: Vec<f64> = e.windows(2).map(|p| p[1].t - p[0].t).filter(|&x| x > 0.0).collect();
    if w.len() < 3 || dt.is_nan() || dt <= 0.0 {
        return None;
    }
    let mut s = w.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    let need = (s.len() / 20).max(2);
    let mut tb = None;
    for &c in &s {
        if c < 1.5 {
            continue;
        }
        let support = s.partition_point(|&x| x <= 1.25 * c) - s.partition_point(|&x| x < 0.8 * c);
        if support >= need {
            tb = Some(c);
            break;
        }
    }
    let mut tb = tb?;
    for _ in 0..3 {
        let (mut sw, mut sn) = (0.0, 0.0);
        for &x in &w {
            let n = (x / tb).round();
            if (1.0..=12.0).contains(&n) && (x / tb - n).abs() < 0.3 {
                sw += x;
                sn += n;
            }
        }
        if sn > 0.0 {
            tb = sw / sn;
        }
    }
    let baud = 1.0 / (tb * dt);
    let best = STD_BAUD.iter().copied().min_by(|a, b| ((a - baud).abs() / a).total_cmp(&((b - baud).abs() / b)))?;
    Some(if (best - baud).abs() / best <= 0.03 { best } else { baud })
}

/// Differential pair (RS-485 / RS-422 A/B, FlexRay BP/BM, CAN H/L): A − B sample by sample (the shorter length).
/// Decode the result with a threshold of 0 V and a hysteresis of the receiver (RS-485: ±200 mV → 0.4).
pub fn diff(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x - y).collect()
}

/// 9-bit UART: the ninth bit marks an address.
pub static UART9: ModuleInfo = ModuleInfo::new("Uart9", "UART 9-bit", "serial")
    .differential()
    .trigger(every_word_is_a_frame)
    .bit(|opt, _| 1.0 / opt_baud(opt, 0))
    .decoder(|l| Ok(uart9(l.main(), l.dt, opt_baud(&l.opt, 0), l.thr, l.hy, false)));

/// 9-bit multidrop UART (start, 9 data bits LSB first, stop; the 9th bit marks an address character, as in RS-485
/// multidrop / MDB): `Addr { read: false }` with value = the low 8 bits when the 9th bit is set, else Data;
/// value2 = the 9th bit; `ok` = stop bit.
pub fn uart9(v: &[f32], dt: f64, baud: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    uart(v, dt, baud, 9, Parity::None, thr, hyst, invert)
        .into_iter()
        .map(|p| {
            let a = (p.value >> 8) & 1;
            Packet {
                kind: if a == 1 { Kind::Addr { read: false } } else { Kind::Data },
                value: p.value & 0xff,
                value2: Some(a),
                ..p
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{i2c_logic, sample, spi_logic, uart_logic, I2cXfer};

    #[test]
    fn uart_8n1_and_parity() {
        let baud = 115200.0;
        let dt = 1.0 / baud / 10.0;
        let msg = b"OOS 0.1";
        let l = uart_logic(msg, baud, 8, None, 1.0, 0.0, 3.3);
        let v = sample(|t| l.v(t), 0.0, dt, 1200);
        let p = uart(&v, dt, baud, 8, Parity::None, 1.65, 0.2, false);
        assert_eq!(p.iter().map(|x| x.value as u8).collect::<Vec<_>>(), msg);
        assert!(p.iter().all(|x| x.ok));
        // even parity, 7 bits
        let l = uart_logic(b"AZ", baud, 7, Some(true), 0.5, 0.0, 3.3);
        let v = sample(|t| l.v(t), 0.0, dt, 400);
        let p = uart(&v, dt, baud, 7, Parity::Even, 1.65, 0.2, false);
        assert_eq!(p.iter().map(|x| (x.value, x.ok)).collect::<Vec<_>>(), [(b'A' as u32, true), (b'Z' as u32, true)]);
        // decoded as odd parity: parity errors
        assert!(uart(&v, dt, baud, 7, Parity::Odd, 1.65, 0.2, false).iter().all(|x| !x.ok));
        // wrong baud rate: framing errors
        assert!(uart(
            &sample(|t| uart_logic(msg, baud, 8, None, 1.0, 0.0, 3.3).v(t), 0.0, dt, 1200),
            dt,
            baud * 1.5,
            8,
            Parity::None,
            1.65,
            0.2,
            false
        )
        .iter()
        .any(|x| !x.ok));
    }

    #[test]
    fn i2c_write_then_read() {
        let rate = 100e3;
        let dt = 0.25e-6;
        let (sda, scl) = i2c_logic(
            &[
                I2cXfer { addr: 0x50, read: false, data: vec![0x00, 0x12], repeated: false, stop: false },
                I2cXfer { addr: 0x50, read: true, data: vec![0xab, 0xcd], repeated: true, stop: true },
            ],
            rate,
            0.0,
            3.3,
        );
        let a = sample(|t| sda.v(t), 0.0, dt, 2400);
        let c = sample(|t| scl.v(t), 0.0, dt, 2400);
        let p = i2c(&a, &c, 1.65, 0.3);
        let k: Vec<(Kind, u32, bool)> = p.iter().map(|x| (x.kind, x.value, x.ok)).collect();
        assert_eq!(
            k,
            [
                (Kind::Start, 0, true),
                (Kind::Addr { read: false }, 0x50, true),
                (Kind::Data, 0x00, true),
                (Kind::Data, 0x12, true),
                (Kind::RepStart, 0, true),
                (Kind::Addr { read: true }, 0x50, true),
                (Kind::Data, 0xab, true),
                (Kind::Data, 0xcd, false), // master NACKs the last read byte
                (Kind::Stop, 0, true),
            ]
        );
    }

    /// Clock and data can sit at different levels. A threshold that fits the data misses a smaller clock.
    #[test]
    fn i2c_separate_thresholds() {
        let rate = 100e3;
        let dt = 0.25e-6;
        let (sda, scl) = i2c_logic(
            &[I2cXfer { addr: 0x50, read: false, data: vec![0x42], repeated: false, stop: true }],
            rate,
            0.0,
            3.3,
        );
        let a = sample(|t| sda.v(t), 0.0, dt, 1600);
        let c: Vec<f32> = sample(|t| scl.v(t), 0.0, dt, 1600).into_iter().map(|v| v / 3.3).collect();
        let p = i2c_at(&a, &c, 1.65, 0.2, 0.4, 0.1);
        let k: Vec<(Kind, u32, bool)> = p.iter().map(|x| (x.kind, x.value, x.ok)).collect();
        assert_eq!(
            k,
            [
                (Kind::Start, 0, true),
                (Kind::Addr { read: false }, 0x50, true),
                (Kind::Data, 0x42, true),
                (Kind::Stop, 0, true),
            ]
        );
        assert!(i2c(&a, &c, 1.65, 0.2).iter().all(|x| !matches!(x.kind, Kind::Addr { .. } | Kind::Data)));
    }

    #[test]
    fn spi_all_modes() {
        let words = [(0x9f, 0xff), (0x00, 0xef), (0x5a, 0x40)];
        for mode in 0..4u8 {
            let [cs, clk, mosi, miso] = spi_logic(&words, 8, mode, 1e6, 0.0, 3.3);
            let dt = 25e-9;
            let s = |l: &crate::sim::Logic| sample(|t| l.v(t), 0.0, dt, 2400);
            let (vc, vk, vo, vi) = (s(&cs), s(&clk), s(&mosi), s(&miso));
            let p = spi(&vk, &vo, Some(&vi), Some(&vc), mode, 8, true, 1.65, 0.3, 1e9);
            assert_eq!(p.iter().map(|x| (x.value, x.value2.unwrap())).collect::<Vec<_>>(), words, "mode {}", mode);
            // without CS: frames from the clock alone
            let p = spi(&vk, &vo, None, None, mode, 8, true, 1.65, 0.3, 100.0);
            assert_eq!(p.iter().map(|x| x.value).collect::<Vec<_>>(), [0x9f, 0x00, 0x5a], "mode {} no CS", mode);
        }
    }
}
