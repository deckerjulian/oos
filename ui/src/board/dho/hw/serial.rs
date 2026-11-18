// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The protocol trigger in the TPU's receivers (`trig.serial_hw`): UART, SPI and I2C from the model, and the
//! debug answers of `:SYSTem:DEBug:UART?` and `:SYSTem:DEBug:SERial?`. A new receiver protocol goes here.

use crate::model::*;
use oos_hal::dho::seq::Trig;
use oos_hal::dho::{serialtrig, uarttrig};

/// `trig.serial_hw = on` in system.conf: the UART protocol trigger runs in the TPU's receiver (docs/fork/hwuart.md).
/// Off by default until it is verified on the unit.
pub fn serial_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| SERIAL_HW.get())
}

pub(super) const SERIAL_HW: crate::sysconf::Flag = crate::sysconf::Flag { key: "trig.serial_hw", env: None, on: "on" };

/// The relation of the TPU's receivers for a model relation.
fn hw_rel(r: Rel) -> uarttrig::Relation {
    use uarttrig::Relation as R;
    match r {
        Rel::Eq => R::Eq,
        Rel::Ne => R::Ne,
        Rel::Gt => R::Gt,
        Rel::Lt => R::Lt,
        Rel::In => R::In,
        Rel::Out => R::Out,
    }
}

/// The UART protocol trigger of the model in the TPU's receiver, where it can do it: a UART decoder on a
/// channel (not the LA, not differential), 1..8 data bits, and a condition it has: frame start, a data byte against
/// the value and limit (no mask: the receiver has none), an error of the chosen kind. Polarity and stop bits as set.
/// The ID condition, wider values and a mask stay with the software trigger.
pub(super) fn uart_of(m: &Model) -> Option<uarttrig::Uart> {
    use uarttrig::{Cond, ErrKind, Parity, Stop, Uart};
    if m.trig_type != TrigType::Bus
        || m.dec_proto != Proto::UART
        || m.dec_on_la()
        || m.dec_diff.is_some()
        || !(1..=8).contains(&m.dec_bits)
    {
        return None;
    }
    let s = &m.ser;
    let fits = |v: u64| v >> m.dec_bits == 0;
    let cond = match m.trig_bus {
        BusWhat::Frame => Cond::Start,
        BusWhat::Data if fits(m.trig_bus_val) && (!s.rel.uses_max() || fits(s.max)) && s.mask & 0xff == 0xff => {
            Cond::Data(
                hw_rel(s.rel),
                m.trig_bus_val as u8,
                if s.rel.uses_max() { s.max as u8 } else { m.trig_bus_val as u8 },
            )
        }
        BusWhat::Error => Cond::Error(match s.uart_err {
            UartErr::Either => ErrKind::Either,
            UartErr::Parity => ErrKind::Parity,
            UartErr::Framing => ErrKind::Framing,
        }),
        _ => return None,
    };
    let parity = match m.dec_parity {
        oos_dsp::decode::Parity::None => Parity::None,
        oos_dsp::decode::Parity::Odd => Parity::Odd,
        oos_dsp::decode::Parity::Even => Parity::Even,
    };
    let stop = match s.uart_stop {
        StopBits::One => Stop::One,
        StopBits::OneHalf => Stop::OneHalf,
        StopBits::Two => Stop::Two,
    };
    Some(Uart { baud: crate::decode::BAUDS[m.dec_baud], bits: m.dec_bits, parity, stop, cond, invert: s.uart_inv })
}

/// Fork, `:SYSTem:DEBug:UART?`: what the TPU's UART receiver gets for the model (`off,...` when the software trigger
/// stays in charge), the words, the event time and the record shift.
pub fn uart_debug(m: &Model) -> String {
    let Some(u) = uart_of(m) else {
        return format!("off,enabled={}", serial_hw_on() as u8);
    };
    let w = uarttrig::words(&u);
    format!(
        "{},src=CH{},baud={},bits={},parity={:?},cond={:?},invert={},clocks={},r2100={:#010x},r2104={:#010x},r221c={:#010x},mux={},event_s={:.4e},shift_s={:.4e}",
        if serial_hw_on() { "on" } else { "off" },
        m.dec_src + 1,
        u.baud,
        u.bits,
        u.parity,
        u.cond,
        u.invert as u8,
        uarttrig::bit_clocks(u.baud),
        w[1].1,
        w[2].1,
        w[0].1,
        uarttrig::MUX,
        uarttrig::event_s(&u),
        uarttrig::sw_offset_s(&u)
    )
}

/// The protocol trigger in the TPU's receivers (`trig.serial_hw = on`): UART (`uart_of`), SPI or I2C (`spi_of`,
/// `i2c_of`); None keeps the edge trigger and the software decoder.
pub fn hw_serial(m: &Model) -> Option<Trig> {
    if !serial_hw_on() {
        return None;
    }
    serial_of(m)
}

/// `hw_serial` without the system.conf switch.
fn serial_of(m: &Model) -> Option<Trig> {
    uart_of(m).map(Trig::Uart).or_else(|| spi_of(m).map(Trig::Spi)).or_else(|| i2c_of(m).map(Trig::I2c))
}

/// The SPI protocol trigger in the receiver: an SPI decoder on channels, framed by chip select (its polarity as set)
/// or, without CS, by the clock pause of `ser.spi_timeout` (a time is needed: the software's default pause is a number
/// of samples), 1..32 bits, and Frame (every word) or Data against value, limit and mask. MSB first, the clock edge of
/// the decoder's mode, as the software decoder.
pub(super) fn spi_of(m: &Model) -> Option<serialtrig::Spi> {
    if m.trig_type != TrigType::Bus || m.dec_proto != Proto::SPI || m.dec_on_la() || !(1..=32).contains(&m.dec_bits) {
        return None;
    }
    let s = &m.ser;
    let timeout = (m.dec_cs.is_none() && s.spi_timeout > 0.0).then_some(s.spi_timeout as f64);
    if m.dec_cs.is_none() && timeout.is_none() {
        return None;
    }
    let full = serialtrig::word_mask(m.dec_bits) as u64;
    let (rel, v, max, mask) = match m.trig_bus {
        BusWhat::Frame => (Rel::Eq, 0, 0, 0),
        BusWhat::Data if m.trig_bus_val & !full == 0 && (!s.rel.uses_max() || s.max & !full == 0) => {
            (s.rel, m.trig_bus_val, if s.rel.uses_max() { s.max } else { m.trig_bus_val }, s.mask & full)
        }
        _ => return None,
    };
    Some(serialtrig::Spi {
        clk: m.dec_src2 as u32 + 1,
        data: m.dec_src as u32 + 1,
        // with timeout framing the receiver ignores CS: its line is the clock's
        cs: m.dec_cs.unwrap_or(m.dec_src2) as u32 + 1,
        clk_falling: !matches!(m.dec_mode, 0 | 3),
        cs_high: s.spi_cs_high,
        timeout,
        width: m.dec_bits,
        rel: hw_rel(rel),
        min: v as u32,
        max: max as u32,
        mask: mask as u32,
    })
}

/// The I2C protocol trigger in the receiver: Frame (start), Error (missing ACK), ID/Addr against value and limit
/// (7 or 10 bits, the direction), and Data: the first `ser.bytes` (1..5) bytes after the address, big-endian,
/// against value, limit and mask, in the chosen direction. Data in any byte (`bytes` 0) stays with the software.
pub(super) fn i2c_of(m: &Model) -> Option<serialtrig::I2c> {
    use serialtrig::{Dir, I2cCond};
    if m.trig_type != TrigType::Bus || m.dec_proto != Proto::I2C || m.dec_on_la() {
        return None;
    }
    let s = &m.ser;
    let amax: u64 = if s.ten_bit { 0x3ff } else { 0x7f };
    let bytes = s.bytes.clamp(1, 5) as u32;
    // any data byte of a frame: the receiver compares only the first ones
    if m.trig_bus == BusWhat::Data && s.bytes == 0 {
        return None;
    }
    let dfull: u64 = (1u64 << (8 * bytes)) - 1;
    let dir = match s.dir {
        I2cDir::Either => Dir::Either,
        I2cDir::Write => Dir::Write,
        I2cDir::Read => Dir::Read,
    };
    let none = (0u32, 0u32, 0u32);
    let (cond, addr, data_rel, data) = match m.trig_bus {
        BusWhat::Frame => (I2cCond::Start, none, Rel::Eq, (0, 0, 0)),
        BusWhat::Error => (I2cCond::MissingAck, none, Rel::Eq, (0, 0, 0)),
        BusWhat::Id if m.trig_bus_val <= amax && s.addr_max as u64 <= amax => {
            let hi = if s.addr_rel.uses_max() { s.addr_max } else { m.trig_bus_val as u32 };
            (I2cCond::Address, (m.trig_bus_val as u32, hi, amax as u32), Rel::Eq, (0, 0, 0))
        }
        BusWhat::Data if m.trig_bus_val <= dfull && (!s.rel.uses_max() || s.max <= dfull) => {
            let hi = if s.rel.uses_max() { s.max } else { m.trig_bus_val };
            (I2cCond::Data, none, s.rel, (m.trig_bus_val, hi, s.mask & dfull))
        }
        _ => return None,
    };
    Some(serialtrig::I2c {
        scl: m.dec_src2 as u32 + 1,
        sda: m.dec_src as u32 + 1,
        cond,
        dir,
        ten_bit: s.ten_bit,
        addr_rel: hw_rel(if cond == I2cCond::Address { s.addr_rel } else { Rel::Eq }),
        addr,
        data_rel: hw_rel(data_rel),
        bytes,
        data,
    })
}

/// Every comparator line of an SPI or I2C receiver is an acquired channel (`mask`, bit 0 = CH1); other triggers pass.
pub(super) fn serial_lines_acquired(t: &Trig, mask: u32) -> bool {
    let lines: Vec<u32> = match t {
        Trig::Spi(x) => vec![x.clk, x.data, x.cs],
        Trig::I2c(x) => vec![x.scl, x.sda],
        _ => return true,
    };
    lines.iter().all(|&c| (1..=4).contains(&c) && mask & 1 << (c - 1) != 0)
}

/// Fork, `:SYSTem:DEBug:SERial?`: the receiver and its words for the model (UART: `uart_debug`), `off,...` where the
/// software trigger stays in charge.
pub fn serial_debug(m: &Model) -> String {
    let on = if serial_hw_on() { "on" } else { "off" };
    match serial_of(m) {
        Some(Trig::Uart(_)) => uart_debug(m),
        Some(Trig::Spi(s)) => {
            let w: Vec<String> =
                serialtrig::spi_words(&s).iter().map(|(r, v)| format!("r{:04x}={:#010x}", r, v)).collect();
            format!(
                "{},proto=SPI,clk=CH{},data=CH{},cs=CH{},clk_falling={},width={},{},mux={}",
                on,
                s.clk,
                s.data,
                s.cs,
                s.clk_falling as u8,
                s.width,
                w.join(","),
                serialtrig::MUX_SPI
            )
        }
        Some(Trig::I2c(i)) => {
            let w: Vec<String> =
                serialtrig::i2c_words(&i).iter().map(|(r, v)| format!("r{:04x}={:#010x}", r, v)).collect();
            format!(
                "{},proto=I2C,scl=CH{},sda=CH{},cond={:?},{},mux={}",
                on,
                i.scl,
                i.sda,
                i.cond,
                w.join(","),
                serialtrig::MUX_I2C
            )
        }
        _ => format!("off,enabled={}", serial_hw_on() as u8),
    }
}
