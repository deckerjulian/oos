// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The TPU's SPI and I2C triggers (docs/fork/hwserial.md): word compares on the comparator lines of events A, B and R.
//! Field layout from a simulation of the FPGA netlist; the register writes of the triggers are `tpu::spi` and
//! `tpu::i2c`. Relations as the UART's (`uarttrig::Relation`): == / != / > against min, < against max, ranges
//! inclusive, all after the mask.

use crate::dho::uarttrig::Relation;
use crate::dho::Write;

/// SPI: 0 framing (1 = idle timeout), 3:1 relation, 8:4 width - 1.
pub const SPI_CFG: u32 = 0x2178;
/// SPI idle timeout in 6.4 ns clocks.
pub const SPI_TIMEOUT: u32 = 0x217c;
/// SPI compare value (min), max and mask; right-aligned, the last bit on the wire is bit 0.
pub const SPI_MIN: u32 = 0x2180;
pub const SPI_MAX: u32 = 0x2184;
pub const SPI_MASK: u32 = 0x2188;
/// I2C: 2:0 condition, 5:3 address relation, 8:6 data relation, 10:9 direction, 11 ten-bit, 14:12 data bytes - 1.
pub const I2C_CFG: u32 = 0x218c;
/// I2C address min [9:0], max [19:10], mask [29:20].
pub const I2C_ADDR: u32 = 0x2190;
/// I2C data min, max, mask (bytes 0-3) and byte 4 of each.
pub const I2C_DMIN: u32 = 0x2194;
pub const I2C_DMAX: u32 = 0x2198;
pub const I2C_DMASK: u32 = 0x219c;
pub const I2C_D4: u32 = 0x21a0;

/// Trigger mux values.
pub const MUX_SPI: u32 = 12;
pub const MUX_I2C: u32 = 13;

/// The receiver clock (adc156), s.
pub const CLOCK_S: f64 = 6.4e-9;

/// An SPI trigger: clock on event A, data on event R, chip select on event B (channels 1..4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spi {
    pub clk: u32,
    pub data: u32,
    pub cs: u32,
    /// sample on the falling clock edge (line A inverted)
    pub clk_falling: bool,
    /// chip select active high (line B inverted)
    pub cs_high: bool,
    /// frame by an idle gap of this many seconds instead of chip select
    pub timeout: Option<f64>,
    /// word width 1..32
    pub width: u32,
    pub rel: Relation,
    pub min: u32,
    pub max: u32,
    pub mask: u32,
}

/// The SPI receiver's words, the configuration last.
pub fn spi_words(s: &Spi) -> Vec<Write> {
    let mut w = Vec::new();
    if let Some(t) = s.timeout {
        w.push((SPI_TIMEOUT, (t / CLOCK_S).ceil().clamp(1.0, u32::MAX as f64) as u32));
    }
    w.extend([(SPI_MIN, s.min), (SPI_MAX, s.max), (SPI_MASK, s.mask)]);
    w.push((SPI_CFG, s.timeout.is_some() as u32 | s.rel.code() << 1 | (s.width.clamp(1, 32) - 1) << 4));
    w
}

/// The mask of a `width`-bit word.
pub fn word_mask(width: u32) -> u32 {
    if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width.max(1)) - 1
    }
}

/// What fires the I2C trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cCond {
    Start,
    Stop,
    RepeatedStart,
    /// a missing ACK (also the master's NACK at the end of a read)
    MissingAck,
    Address,
    Data,
    AddressData,
}

impl I2cCond {
    fn code(self) -> u32 {
        match self {
            I2cCond::Start => 0,
            I2cCond::Stop => 1,
            I2cCond::RepeatedStart => 2,
            I2cCond::MissingAck => 3,
            I2cCond::Address => 4,
            I2cCond::Data => 5,
            I2cCond::AddressData => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Write,
    Read,
    Either,
}

/// An I2C trigger: SCL on event A, SDA on event B (channels 1..4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct I2c {
    pub scl: u32,
    pub sda: u32,
    pub cond: I2cCond,
    pub dir: Dir,
    pub ten_bit: bool,
    pub addr_rel: Relation,
    /// address min, max, mask (7 or 10 bits)
    pub addr: (u32, u32, u32),
    pub data_rel: Relation,
    /// data bytes compared, 1..5
    pub bytes: u32,
    /// data min, max, mask: the first `bytes` bytes after the address, big-endian (the first byte is the most
    /// significant)
    pub data: (u64, u64, u64),
}

/// The I2C receiver's words, the configuration last.
pub fn i2c_words(i: &I2c) -> Vec<Write> {
    let a = |x: u32| x & 0x3ff;
    let (dmin, dmax, dmask) = i.data;
    let lo = |x: u64| x as u32;
    let b4 = |x: u64| (x >> 32) as u32 & 0xff;
    let dir = match i.dir {
        Dir::Write => 0,
        Dir::Read => 1,
        Dir::Either => 2,
    };
    vec![
        (I2C_ADDR, a(i.addr.0) | a(i.addr.1) << 10 | a(i.addr.2) << 20),
        (I2C_DMIN, lo(dmin)),
        (I2C_DMAX, lo(dmax)),
        (I2C_DMASK, lo(dmask)),
        (I2C_D4, b4(dmin) | b4(dmax) << 8 | b4(dmask) << 16),
        (I2C_CFG, i.cond.code() | i.addr_rel.code() << 3 | i.data_rel.code() << 6 | dir << 9 | (i.ten_bit as u32) << 11 | (i.bytes.clamp(1, 5) - 1) << 12),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spi() -> Spi {
        Spi { clk: 2, data: 1, cs: 3, clk_falling: false, cs_high: false, timeout: None, width: 8, rel: Relation::Eq, min: 0x5a, max: 0x5a, mask: 0xff }
    }

    #[test]
    fn spi_words_by_relation_width_and_framing() {
        assert_eq!(spi_words(&spi()), vec![(0x2180, 0x5a), (0x2184, 0x5a), (0x2188, 0xff), (0x2178, 7 << 4)]);
        // (set-up, 0x2178)
        let cases = [
            (Spi { rel: Relation::Ne, ..spi() }, 1 << 1 | 7 << 4),
            (Spi { rel: Relation::In, width: 16, ..spi() }, 4 << 1 | 15 << 4),
            (Spi { rel: Relation::Out, width: 32, ..spi() }, 5 << 1 | 31 << 4),
            (Spi { width: 1, ..spi() }, 0),
        ];
        for (s, cfg) in cases {
            assert_eq!(*spi_words(&s).last().unwrap(), (0x2178, cfg), "{:?}", s);
        }
        // idle timeout framing: 1 us = 157 clocks, framing bit set, the timeout first
        let w = spi_words(&Spi { timeout: Some(1e-6), ..spi() });
        assert_eq!((w[0], w[4]), ((0x217c, 157), (0x2178, 1 | 7 << 4)));
        assert_eq!((word_mask(8), word_mask(1), word_mask(32), word_mask(0)), (0xff, 1, u32::MAX, 1));
    }

    #[test]
    fn i2c_words_by_condition() {
        let base = I2c { scl: 1, sda: 2, cond: I2cCond::Address, dir: Dir::Either, ten_bit: false, addr_rel: Relation::Eq, addr: (0x50, 0x50, 0x7f), data_rel: Relation::Eq, bytes: 1, data: (0, 0, 0) };
        let w = i2c_words(&base);
        assert_eq!(w[0], (0x2190, 0x50 | 0x50 << 10 | 0x7f << 20));
        assert_eq!(*w.last().unwrap(), (0x218c, 4 | 2 << 9));
        // (condition, 0x218c [2:0])
        for (c, code) in [(I2cCond::Start, 0), (I2cCond::Stop, 1), (I2cCond::RepeatedStart, 2), (I2cCond::MissingAck, 3), (I2cCond::Data, 5), (I2cCond::AddressData, 6)] {
            assert_eq!(i2c_words(&I2c { cond: c, ..base }).last().unwrap().1 & 7, code);
        }
        // 5 data bytes, big-endian: byte 4 (the first on the bus) in 0x21a0
        let d = I2c { cond: I2cCond::Data, data_rel: Relation::In, bytes: 5, data: (0x01_0203_0405, 0xff_0203_0405, 0xff_ffff_ffff), dir: Dir::Read, ten_bit: true, ..base };
        let w = i2c_words(&d);
        assert_eq!(&w[1..5], &[(0x2194, 0x0203_0405), (0x2198, 0x0203_0405), (0x219c, 0xffff_ffff), (0x21a0, 0x01 | 0xff << 8 | 0xff << 16)]);
        assert_eq!(w[5], (0x218c, 5 | 4 << 6 | 1 << 9 | 1 << 11 | 4 << 12));
    }
}
