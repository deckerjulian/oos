// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The software trigger of all sources: the trigger types of oos-dsp, and the protocol trigger (TrigType::Bus), which
//! decodes the record with the Decode menu's settings and triggers at the end of the matching field.

use crate::model::*;
use oos_dsp::decode::{Kind, Packet};
use oos_dsp::trigger::Input;

/// Packets of the decoder settings in `m` on the channels of `inp` (times = fractional sample indices).
pub fn decode(m: &Model, ch: [&[f32]; NCH], dt: f64) -> Result<Vec<Packet>, &'static str> {
    decode_at(m, ch, dt, m.dec_thr, 0.1 * m.ch[m.dec_src].volts_per_div())
}

/// Packets of logic analyser lines (`Model::dec_la`, the decoder's roles source / second / third / fourth line). The
/// lines become 0 / 1 waves in the channel slots 0..3 and the normal decoders run on them, threshold 0.5; the LA's
/// own comparators have already decided the levels. Times = fractional sample indices of the LA record.
pub fn decode_la(m: &Model, words: &[u16], dt: f64) -> Result<Vec<Packet>, &'static str> {
    let wave = |n: u8| -> Vec<f32> { words.iter().map(|w| (w >> n & 1) as f32).collect() };
    let (thr, hy) = (0.5, 0.1);
    let la = m.dec_la;
    if m.dec_proto == Proto::Parallel {
        // up to 16 data lines upwards from the main source, skipping the clock line
        let first = la[0].unwrap_or(0);
        let lines: Vec<Vec<f32>> = (0..16u8).map(|k| (first + k) % 16).filter(|&n| Some(n) != la[2]).take(m.dec_bits.clamp(1, 16) as usize).map(wave).collect();
        let refs: Vec<&[f32]> = lines.iter().map(|l| &l[..]).collect();
        let clk = la[2].map(wave);
        return Ok(oos_dsp::decode_ext::parallel(&refs, clk.as_deref(), oos_dsp::trigger::Slope::Rising, thr, hy));
    }
    let slots: [Vec<f32>; NCH] = std::array::from_fn(|i| la.get(i).copied().flatten().map_or(Vec::new(), &wave));
    let mut s = m.clone();
    (s.dec_src, s.dec_src2, s.dec_cs, s.dec_src4, s.dec_diff) = (0, 1, la[2].map(|_| 2), la[3].map(|_| 3), None);
    decode_at(&s, std::array::from_fn(|c| &slots[c][..]), dt, thr, hy)
}

/// `decode` with the threshold and hysteresis given.
fn decode_at(m: &Model, ch: [&[f32]; NCH], dt: f64, thr: f32, hy: f32) -> Result<Vec<Packet>, &'static str> {
    use oos_dsp::decode::{can, i2c, i2s, lin, mil1553, spi, uart, Parity};
    use oos_dsp::{decode_ext as x1, decode_ext2 as x2, decode_ext3 as x3};
    let get = |c: usize| if ch[c].is_empty() { Err("source channel off") } else { Ok(ch[c]) };
    let opt = |c: Option<usize>| -> Result<Option<&[f32]>, &'static str> { c.map(get).transpose() };
    // RS-485 / differential: A - B, threshold around 0 V like the pair's receiver
    let diff;
    let line: &[f32] = match (m.dec_proto.diff_ok(), m.dec_diff) {
        (true, Some(b)) => {
            diff = x1::diff(get(m.dec_src)?, get(b)?);
            &diff
        }
        _ => get(m.dec_src)?,
    };
    let baud = crate::decode::BAUDS[m.dec_baud];
    let can_rate = crate::decode::CAN_RATES[m.dec_canrate];
    Ok(match m.dec_proto {
        Proto::Uart => uart(line, dt, baud, m.dec_bits, m.dec_parity, thr, hy, false),
        Proto::I2c => i2c(get(m.dec_src)?, get(m.dec_src2)?, thr, hy),
        Proto::Spi => {
            let bit = 1e-9f64.max(dt);
            spi(get(m.dec_src2)?, get(m.dec_src)?, None, opt(m.dec_cs)?, m.dec_mode, m.dec_bits, true, thr, hy, 20.0 * bit / dt)
        }
        Proto::Can => can(line, dt, can_rate, thr, hy, m.dec_canh),
        Proto::Lin => lin(line, dt, baud, thr, hy),
        Proto::I2s => i2s(get(m.dec_src2)?, opt(m.dec_cs)?, get(m.dec_src)?, m.dec_bits, thr, hy),
        Proto::Mil1553 => mil1553(get(m.dec_src)?, dt, thr, hy),
        Proto::Parallel => {
            // dec_bits data lines from the main source upwards, skipping the clock
            let clk = m.dec_cs;
            let lines: Vec<&[f32]> = (0..NCH).map(|k| (m.dec_src + k) % NCH).filter(|&c| Some(c) != clk).take(m.dec_bits.clamp(1, 4) as usize).map(get).collect::<Result<_, _>>()?;
            x1::parallel(&lines, opt(clk)?, oos_dsp::trigger::Slope::Rising, thr, hy)
        }
        Proto::CanFd => x1::can_fd(line, dt, can_rate, crate::decode::FD_RATES[m.dec_fdrate], thr, hy, m.dec_canh),
        Proto::FlexRay => x1::flexray(line, dt, 10e6, thr, hy, false),
        Proto::Dmx512 => x1::dmx512(line, dt, thr, hy, false),
        Proto::ModbusRtu => x1::modbus_rtu(line, dt, baud, m.dec_parity, thr, hy, false),
        Proto::ModbusAscii => x1::modbus_ascii(line, dt, baud, m.dec_bits.clamp(7, 8), if m.dec_parity == Parity::None { Parity::Even } else { m.dec_parity }, thr, hy, false),
        Proto::OneWire => x1::onewire(get(m.dec_src)?, dt, thr, hy, false),
        Proto::Ds2482 => x1::ds2482(&i2c(get(m.dec_src)?, get(m.dec_src2)?, thr, hy)),
        Proto::Uart9 => x1::uart9(line, dt, baud, thr, hy, false),
        Proto::Swd => x2::swd(get(m.dec_src2)?, get(m.dec_src)?, thr, hy),
        Proto::Jtag => {
            let tdi = opt(m.dec_cs)?.ok_or("TDI source missing")?;
            x2::jtag(get(m.dec_src2)?, get(m.dec_src)?, tdi, opt(m.dec_src4)?, None, thr, hy)
        }
        Proto::Qspi => {
            let cs = opt(m.dec_cs)?.ok_or("CS# source missing")?;
            let mut io = vec![get(m.dec_src)?];
            if let Some(c) = opt(m.dec_src4)? {
                io.push(c);
            }
            x2::qspi(cs, get(m.dec_src2)?, &io, &x2::QspiCfg::new(1, 1, 1, 3, 0, false), true, thr, hy)
        }
        Proto::I3c => x2::i3c(get(m.dec_src)?, get(m.dec_src2)?, &[], thr, hy),
        Proto::Spdif => x2::spdif(get(m.dec_src)?, thr, hy),
        Proto::UsbPd => x2::usbpd(get(m.dec_src)?, dt, thr, hy),
        Proto::Arinc429 => {
            let r = crate::decode::ARINC_RATES[m.dec_arate];
            x3::arinc429(line, dt, if r > 0.0 { Some(r) } else { None }, thr, hy)
        }
        Proto::Sent => {
            let t = crate::decode::SENT_TICKS[m.dec_tick];
            x3::sent(get(m.dec_src)?, dt, if t > 0.0 { Some(t) } else { None }, m.dec_nibbles as usize, thr, hy)
        }
        // D- optional (D+ alone: no SE0)
        Proto::Usb => x3::usb(get(m.dec_src)?, opt(m.dec_cs)?, dt, crate::decode::USB_SPEEDS[m.dec_uspeed], thr, hy),
        // NMRA DCC track: half-bit timing on one channel (or A-B via diff)
        Proto::Dcc => x3::dcc(get(m.dec_src)?, dt, thr, hy),
    })
}

/// Whether a packet satisfies the protocol trigger condition of `m`.
pub fn matches(m: &Model, p: &Packet) -> bool {
    let v = m.trig_bus_val;
    match m.trig_bus {
        BusWhat::Frame => match p.kind {
            Kind::Start | Kind::Break | Kind::Word { cmd: true } => true,
            Kind::Data => matches!(m.dec_proto, Proto::Uart | Proto::Spi | Proto::Uart9 | Proto::Parallel),
            Kind::Sample { right: false } => true,
            // an ARINC 429 word starts with its label
            Kind::Id { .. } => m.dec_proto == Proto::Arinc429,
            // the first field of a frame of the extended decoders
            Kind::Field(l) => matches!(l, "!RESET" | "PRE" | "TSS" | "CMD" | "REQ" | "BLK" | "IR" | "DR" | "SYNC"),
            _ => false,
        },
        // USB: the token address or the PID byte
        BusWhat::Id if m.dec_proto == Proto::Usb => matches!(p.kind, Kind::Field("ADDR" | "PID")) && p.value == v,
        BusWhat::Id => (matches!(p.kind, Kind::Id { .. } | Kind::Addr { .. } | Kind::Word { cmd: true }) || p.kind == Kind::Field("ROM") || p.kind == Kind::Field("CMD") || p.kind == Kind::Field("HDR") || p.kind == Kind::Field("STAT")) && p.value == v,
        BusWhat::Data => matches!(p.kind, Kind::Data | Kind::Sample { .. } | Kind::Word { cmd: false }) && p.value == v,
        BusWhat::Error => !p.ok,
    }
}

/// All trigger points of `m`'s condition in `inp` (fractional sample indices).
pub fn find_all(m: &Model, inp: &Input) -> Vec<f64> {
    if m.trig_type == TrigType::Bus {
        return decode(m, inp.ch, inp.dt).unwrap_or_default().iter().filter(|p| matches(m, p)).map(|p| p.t1).collect();
    }
    oos_dsp::trigger::find_all(&m.trigger(), inp)
}

/// The first trigger point after `from`.
pub fn find_first(m: &Model, inp: &Input, from: f64) -> Option<f64> {
    find_all(m, inp).into_iter().find(|&t| t > from)
}
