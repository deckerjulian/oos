// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Serial protocol decoders on analog channels (threshold + hysteresis): UART, I2C, SPI, CAN, LIN, I2S,
//! MIL-STD-1553. Times in the packets are
//! fractional sample indices like everywhere in oos-dsp.

use crate::edges::{edges, states};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// UART character / SPI word / I2C data byte
    Data,
    /// I2C address byte (value = 7-bit address, `read` flag)
    Addr { read: bool },
    Start,
    RepStart,
    Stop,
    /// CAN identifier (value = 11- or 29-bit ID) / LIN protected identifier (value = 6-bit ID, `ok` = parity)
    Id { ext: bool, rtr: bool },
    /// CAN CRC-15 / LIN checksum (`ok` = it matches)
    Crc,
    /// CAN ACK slot (`ok` = acknowledged, i.e. dominant)
    Ack,
    /// LIN break field
    Break,
    /// I2S audio sample (value = the word, two's complement in `bits`; `right` = right channel)
    Sample { right: bool },
    /// MIL-STD-1553 word (value = 16 bits; `cmd` = command/status sync, else data sync; `ok` = odd parity)
    Word { cmd: bool },
    /// any other labelled field of the extended decoders (`decode_ext`, `decode_ext2`): the label is shown before the
    /// value ("CMD", "ADDR", "ROM", "SOF", "HDR", ...); `ok` = check passed. A label starting with '!' marks an
    /// event without a value (shown as a tag like Start/Stop).
    Field(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub t0: f64,
    pub t1: f64,
    pub kind: Kind,
    pub value: u32,
    /// second value (SPI MISO)
    pub value2: Option<u32>,
    /// UART: stop bit and parity ok; I2C: acknowledged
    pub ok: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

/// UART: start bit, `bits` data bits LSB first, parity, 1 stop bit; idle high (`invert` = idle low). Samples each
/// bit in its middle, timed from the start-bit edge.
pub fn uart(v: &[f32], dt: f64, baud: f64, bits: u32, parity: Parity, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
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
                Some(pb) => ((ones + pb as u32) % 2 == 0) == (p == Parity::Even),
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
        out.push(Packet { t0: s, t1: s + (2.0 + nb) * spb, kind: Kind::Data, value: val, value2: None, ok: stop == Some(true) && par_ok });
        busy_until = s + (1.0 + nb + 0.5) * spb;
    }
    out
}

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
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let at = |s: &Vec<Option<bool>>, t: f64| s.get((t + 0.5) as usize).copied().flatten().unwrap_or(true);
    let mut out = Vec::new();
    let mut in_frame = false;
    let mut first = true; // next byte is the address
    let (mut bits, mut nbits, mut t0) = (0u32, 0u32, 0.0);
    for (t, k) in ev {
        match k {
            1 | 2 if at(&scl_s, t) => {
                if k == 1 {
                    out.push(Packet { t0: t, t1: t, kind: if in_frame { Kind::RepStart } else { Kind::Start }, value: 0, value2: None, ok: true });
                    in_frame = true;
                    first = true;
                } else {
                    out.push(Packet { t0: t, t1: t, kind: Kind::Stop, value: 0, value2: None, ok: true });
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
                        Packet { t0, t1: t, kind: Kind::Addr { read: bits & 1 == 1 }, value: bits >> 1, value2: None, ok: ack }
                    } else {
                        Packet { t0, t1: t, kind: Kind::Data, value: bits, value2: None, ok: ack }
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

/// SPI: words of `bits` bits sampled on the capture edge of `mode` (0/3: rising, 1/2: falling); a word ends after
/// `bits` edges; `cs` (active low) delimits frames, without CS a clock pause longer than `gap` samples restarts.
pub fn spi(clk: &[f32], mosi: &[f32], miso: Option<&[f32]>, cs: Option<&[f32]>, mode: u8, bits: u32, msb_first: bool, thr: f32, hyst: f32, gap: f64) -> Vec<Packet> {
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
        let selected = cs_s.as_ref().map_or(true, |s| !at(s, e.t));
        if !selected {
            n = 0;
            continue;
        }
        // frame boundary: CS edge or clock pause since the last bit
        let restart = if cs.is_some() { cs_e.iter().any(|c| !c.rising && c.t > last && c.t < e.t) } else { e.t - last > gap };
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
            out.push(Packet { t0, t1: e.t, kind: Kind::Data, value: a, value2: mi.as_ref().map(|_| b), ok: true });
            n = 0;
        }
    }
    out
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
        assert!(uart(&sample(|t| uart_logic(msg, baud, 8, None, 1.0, 0.0, 3.3).v(t), 0.0, dt, 1200), dt, baud * 1.5, 8, Parity::None, 1.65, 0.2, false).iter().any(|x| !x.ok));
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

/// CRC-15/CAN (polynomial 0x4599) over a bit stream (SOF .. last data bit, unstuffed).
pub fn can_crc15(bits: &[bool]) -> u16 {
    let mut crc: u16 = 0;
    for &b in bits {
        let top = (crc >> 14) & 1 == 1;
        crc = (crc << 1) & 0x7fff;
        if top != b {
            crc ^= 0x4599;
        }
    }
    crc
}

/// CAN 2.0A/B data and remote frames at `bitrate`. `invert` = false: logic level as on the RX/TX pin (dominant low,
/// idle high); true for CAN_H probed against ground (dominant high). Hard sync on the start of frame after at least 7
/// recessive bit times, resynchronisation on every edge, sampling at 70 % of the bit, bit de-stuffing, CRC check.
pub fn can(v: &[f32], dt: f64, bitrate: f64, thr: f32, hyst: f32, invert: bool) -> Vec<Packet> {
    let spb = 1.0 / bitrate / dt;
    let mut out = Vec::new();
    if spb < 3.0 {
        return out;
    }
    let st = states(v, thr, hyst);
    // recessive = true
    let rec = |x: f64| -> Option<bool> { st.get(x as usize).copied().flatten().map(|b| b != invert) };
    let e = edges(v, thr, hyst);
    let edge_near = |from: f64, to: f64| -> Option<f64> {
        let i = e.partition_point(|x| x.t < from);
        e.get(i).filter(|x| x.t < to).map(|x| x.t)
    };
    let mut busy_until = 0.0;
    let mut last_rec_start: Option<f64> = None;
    for (k, x) in e.iter().enumerate() {
        // a recessive run starts at a recessive-going edge
        let to_rec = x.rising != invert;
        if to_rec {
            last_rec_start = Some(x.t);
            continue;
        }
        if x.t < busy_until {
            continue;
        }
        // SOF: dominant edge after >= 7 recessive bit times (or the record start when the bus was idle before)
        let idle_from = last_rec_start.unwrap_or(if k == 0 && rec(x.t - 1.0) == Some(true) { 0.0 } else { f64::INFINITY });
        if x.t - idle_from < 7.0 * spb {
            continue;
        }
        if let Some((pk, end)) = can_frame(x.t, spb, &rec, &edge_near) {
            busy_until = end;
            out.extend(pk);
        }
    }
    out
}

/// One CAN frame from its SOF edge at `sof`: the packets and the time after the ACK delimiter.
fn can_frame(sof: f64, spb: f64, rec: &dyn Fn(f64) -> Option<bool>, edge_near: &dyn Fn(f64, f64) -> Option<f64>) -> Option<(Vec<Packet>, f64)> {
    let mut start = sof;
    let mut raw: Vec<(bool, f64, f64)> = Vec::new(); // (level, bit start, bit end)
    let mut next = || -> Option<(bool, f64, f64)> {
        let b = rec(start + 0.7 * spb)?;
        let s = start;
        let nominal = start + spb;
        start = match edge_near(start + 0.5 * spb, start + 1.5 * spb) {
            Some(te) if (te - nominal).abs() < 0.5 * spb => te,
            _ => nominal,
        };
        Some((b, s, start))
    };
    // de-stuffed bits with their times
    let mut bits: Vec<(bool, f64, f64)> = Vec::new();
    let (mut run, mut last) = (0, None::<bool>);
    let mut need = 19usize; // SOF + ID 11 + RTR/SRR + IDE + r0 + DLC 4, grows when the header is known
    let (mut ext, mut known) = (false, false);
    while bits.len() < need {
        let b = next()?;
        raw.push(b);
        if run == 5 {
            // stuff bit: must be the complement, not part of the data
            if Some(b.0) == last {
                return None; // stuff error
            }
            run = 1;
            last = Some(b.0);
            continue;
        }
        if Some(b.0) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(b.0);
        }
        bits.push(b);
        if !known && bits.len() == 14 {
            // bit 13 = IDE (recessive = extended)
            ext = bits[13].0;
            if ext {
                need = 1 + 11 + 1 + 1 + 18 + 1 + 2 + 4;
            }
        }
        if !known && bits.len() == need {
            known = true;
            let dlc_at = need - 4;
            let dlc = bits[dlc_at..need].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
            let rtr = if ext { bits[32].0 } else { bits[12].0 };
            let n = if rtr { 0 } else { dlc.min(8) as usize };
            need += 8 * n + 15;
        }
    }
    let val = |r: std::ops::Range<usize>| bits[r].iter().fold(0u32, |a, b| a << 1 | b.0 as u32);
    let (id, rtr, dlc_at) = if ext { (val(1..12) << 18 | val(14..32), bits[32].0, 35) } else { (val(1..12), bits[12].0, 15) };
    let dlc = val(dlc_at..dlc_at + 4);
    let n = if rtr { 0 } else { dlc.min(8) as usize };
    let data_at = dlc_at + 4;
    let crc_at = data_at + 8 * n;
    let crc_calc = can_crc15(&bits[..crc_at].iter().map(|b| b.0).collect::<Vec<_>>());
    let crc_rx = val(crc_at..crc_at + 15) as u16;
    // stuffing covers SOF .. CRC sequence: a run of five at the end of the CRC is followed by one more stuff bit
    if run == 5 {
        let (_, _, _) = next()?;
    }
    // CRC delimiter, ACK slot, ACK delimiter are not stuffed
    let (_, _, _) = next()?;
    let ack = next()?;
    let ack_del = next()?;
    let pk = |kind, a: usize, b: usize, value: u32, ok: bool| Packet { t0: bits[a].1, t1: bits[b].2, kind, value, value2: None, ok };
    let mut out = vec![Packet { t0: bits[0].1, t1: bits[0].2, kind: Kind::Start, value: 0, value2: None, ok: true }];
    let id_end = if ext { 31 } else { 11 };
    out.push(pk(Kind::Id { ext, rtr }, 1, id_end, id, true));
    for i in 0..n {
        out.push(pk(Kind::Data, data_at + 8 * i, data_at + 8 * i + 7, val(data_at + 8 * i..data_at + 8 * i + 8), true));
    }
    out.push(pk(Kind::Crc, crc_at, crc_at + 14, crc_rx as u32, crc_rx == crc_calc));
    out.push(Packet { t0: ack.1, t1: ack.2, kind: Kind::Ack, value: 0, value2: None, ok: !ack.0 });
    Some((out, ack_del.2))
}

/// LIN protected identifier: ID with the two parity bits P0 = ID0^ID1^ID2^ID4, P1 = !(ID1^ID3^ID4^ID5).
pub fn lin_pid(id: u8) -> u8 {
    let b = |i: u8| (id >> i) & 1;
    let p0 = b(0) ^ b(1) ^ b(2) ^ b(4);
    let p1 = 1 ^ (b(1) ^ b(3) ^ b(4) ^ b(5));
    (id & 0x3f) | p0 << 6 | p1 << 7
}

/// LIN checksum: inverted 8-bit sum with carry wrap-around over `bytes` (classic: data; enhanced: PID + data).
pub fn lin_checksum(bytes: &[u8]) -> u8 {
    let mut s: u16 = 0;
    for &b in bytes {
        s += b as u16;
        if s > 0xff {
            s -= 0xff;
        }
    }
    !(s as u8)
}

/// LIN frames: break (dominant >= 11 bit times), sync 0x55, protected ID, data bytes and the checksum (the last byte
/// before the next break or the end; classic or enhanced checksum accepted). Bytes are UART 8N1, idle high.
pub fn lin(v: &[f32], dt: f64, baud: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let spb = 1.0 / baud / dt;
    let mut out = Vec::new();
    if spb < 2.5 {
        return out;
    }
    let e = edges(v, thr, hyst);
    // breaks: falling edge followed by the next rising edge >= 11 bit times later
    let mut breaks = Vec::new();
    for w in e.windows(2) {
        if !w[0].rising && w[1].rising && w[1].t - w[0].t >= 11.0 * spb {
            breaks.push((w[0].t, w[1].t));
        }
    }
    let bytes = uart(v, dt, baud, 8, Parity::None, thr, hyst, false);
    for (k, &(b0, b1)) in breaks.iter().enumerate() {
        let end = breaks.get(k + 1).map_or(f64::INFINITY, |b| b.0);
        let fr: Vec<&Packet> = bytes.iter().filter(|p| p.t0 > b1 && p.t0 < end).collect();
        out.push(Packet { t0: b0, t1: b1, kind: Kind::Break, value: 0, value2: None, ok: true });
        if fr.len() < 2 || fr[0].value != 0x55 {
            continue;
        }
        out.push(Packet { kind: Kind::Data, ..fr[0].clone() }); // sync
        let pid = fr[1].value as u8;
        out.push(Packet { kind: Kind::Id { ext: false, rtr: false }, value: (pid & 0x3f) as u32, ok: lin_pid(pid & 0x3f) == pid, ..fr[1].clone() });
        if fr.len() >= 3 {
            let data: Vec<u8> = fr[2..fr.len() - 1].iter().map(|p| p.value as u8).collect();
            for p in &fr[2..fr.len() - 1] {
                out.push(Packet { kind: Kind::Data, ..(*p).clone() });
            }
            let cs = fr[fr.len() - 1];
            let mut enh = vec![pid];
            enh.extend(&data);
            let ok = cs.value as u8 == lin_checksum(&data) || cs.value as u8 == lin_checksum(&enh);
            out.push(Packet { kind: Kind::Crc, ok, ..cs.clone() });
        }
    }
    out
}

/// I2S (Philips): SD sampled on the rising SCK edge, MSB first, the word starts one SCK period after the WS edge
/// (WS low = left, high = right). `bits` = word length to keep (MSB-aligned; longer slots are cut, shorter ones end at
/// the next WS edge). Without `ws` the words are cut every `bits` clocks (left only).
pub fn i2s(sck: &[f32], ws: Option<&[f32]>, sd: &[f32], bits: u32, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let ce: Vec<f64> = edges(sck, thr, hyst).into_iter().filter(|e| e.rising).map(|e| e.t).collect();
    let sds = states(sd, thr, hyst);
    let wss = ws.map(|w| states(w, thr, hyst));
    let level = |st: &Vec<Option<bool>>, t: f64| st.get(t as usize).copied().flatten();
    // per rising clock edge: WS level and SD bit
    let mut cur: Option<(bool, f64, u32, u32)> = None; // (right, t0, value, bits so far)
    let mut prev_ws: Option<bool> = None;
    let flush = |cur: &mut Option<(bool, f64, u32, u32)>, t1: f64, out: &mut Vec<Packet>| {
        if let Some((right, t0, v, n)) = cur.take() {
            if n > 0 {
                // MSB-aligned word of `bits`; shorter slots are padded with zeros
                let v = if n < bits { v << (bits - n) } else { v };
                out.push(Packet { t0, t1, kind: Kind::Sample { right }, value: v, value2: None, ok: n >= bits });
            }
        }
    };
    for (k, &t) in ce.iter().enumerate() {
        let w = match &wss {
            Some(st) => level(st, t),
            None => Some(false),
        };
        let Some(w) = w else { continue };
        let Some(b) = level(&sds, t) else { continue };
        let period = if k > 0 { t - ce[k - 1] } else { 0.0 };
        if wss.is_some() {
            if prev_ws.is_some() && prev_ws != Some(w) {
                // WS changed: the previous word ends with this bit (the one-clock delay), the new word starts next
                if let Some(c) = cur.as_mut() {
                    if c.3 < bits {
                        c.2 = c.2 << 1 | b as u32;
                    }
                    c.3 += 1;
                }
                flush(&mut cur, t + period / 2.0, &mut out);
                cur = Some((w, t + period / 2.0, 0, 0));
                prev_ws = Some(w);
                continue;
            }
            prev_ws = Some(w);
            if let Some(c) = cur.as_mut() {
                if c.3 < bits {
                    c.2 = c.2 << 1 | b as u32;
                }
                c.3 += 1;
            }
        } else {
            let c = cur.get_or_insert((false, t - period / 2.0, 0, 0));
            c.2 = c.2 << 1 | b as u32;
            c.3 += 1;
            if c.3 == bits {
                flush(&mut cur, t + period / 2.0, &mut out);
            }
        }
    }
    // the last word has no WS edge after it: keep it when complete
    if cur.map_or(false, |c| c.3 >= bits) {
        let t1 = ce.last().copied().unwrap_or(0.0);
        flush(&mut cur, t1, &mut out);
    }
    out
}

/// MIL-STD-1553: Manchester II at 1 Mbit/s on a bipolar bus signal (threshold normally 0 V). A word = 3 us sync
/// (command/status: 1.5 us positive then 1.5 us negative; data: the inverse), 16 data bits MSB first (1 = positive
/// then negative half), odd parity. `ok` = halves complementary and parity odd.
pub fn mil1553(v: &[f32], dt: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let us = 1e-6 / dt; // samples per microsecond
    let mut out = Vec::new();
    if us < 4.0 {
        return out;
    }
    let st = states(v, thr, hyst);
    let level = |t: f64| st.get(t as usize).copied().flatten();
    let e = edges(v, thr, hyst);
    let mut busy_until = 0.0;
    for b in &e {
        // sync: the edge in the middle of the 3 us sync. Data bits never hold a level longer than 1 us, so a level
        // held for >= 1.3 us on both sides of the edge only occurs there (the bus idles at 0 V before a word, which
        // gives no usable edge at the start of the sync)
        let mid = b.t;
        if mid < busy_until {
            continue;
        }
        let first_pos = !b.rising; // positive before the middle edge
        // the bus must really be driven (beyond the hysteresis), not idle at 0 V where the comparator keeps its state
        let driven = |t: f64, lv: bool| v.get(t as usize).map_or(false, |&x| if lv { x > thr + hyst } else { x < thr - hyst });
        let held = |from: f64, to: f64, lv: bool| (0..=6).all(|i| driven(from + (to - from) * i as f64 / 6.0, lv));
        if !held(mid - 1.3 * us, mid - 0.15 * us, first_pos) || !held(mid + 0.15 * us, mid + 1.3 * us, !first_pos) {
            continue;
        }
        let start = mid + 1.5 * us; // first data bit
        let mut val = 0u32;
        let mut ones = 0;
        let mut ok = true;
        let mut complete = true;
        for i in 0..17 {
            let t = start + i as f64 * us;
            match (level(t + 0.25 * us), level(t + 0.75 * us)) {
                (Some(h1), Some(h2)) => {
                    ok &= h1 != h2;
                    if i < 16 {
                        val = val << 1 | h1 as u32;
                    }
                    ones += h1 as u32;
                }
                _ => complete = false,
            }
        }
        if !complete {
            continue;
        }
        ok &= ones % 2 == 1;
        out.push(Packet { t0: mid - 1.5 * us, t1: start + 17.0 * us, kind: Kind::Word { cmd: first_pos }, value: val, value2: None, ok });
        busy_until = start + 17.0 * us - 0.5 * us;
    }
    out
}
