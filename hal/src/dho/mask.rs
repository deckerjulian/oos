// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The FPGA mask (pass/fail) unit (docs/fork/hwmask.md, docs/fork/specs/06-mask-unit.md): a table of 1000 columns with
//! an 8-bit lower and upper limit each, a comparator on one source slot, and 48-bit counters of tested and failed
//! frames. Register facts come from static analysis of the FPGA image; the sample scale the comparator sees and the
//! slot of each channel are assumptions until the unit session.

use crate::dho::bus::Bus;
use crate::dho::Write;

/// Control: bits 23:20 source slot (one-hot, any bit enables the comparator), 28 latch (rising edge copies the counters
/// into the read registers), 29 clear the fail counter, 30 clear the frame counter (both level-sensitive).
pub const CTRL: u32 = 0x1190;
/// Table write port: 7:0 lower limit, 15:8 upper limit, 25:16 column, 26 lower enable, 27 upper enable, 28 write
/// enable (the RAM writes every bus clock while it is set).
pub const TABLE: u32 = 0x1194;
/// Compression of the compared samples: bit 31 on, bit 30 always set, low bits the samples per column.
pub const COMPRESS: u32 = 0x1184;
/// Latched counters: tested frames (low 32, high 16) and failed frames (low 32, high 16).
pub const FRAMES_LO: u32 = 0x11a0;
pub const FRAMES_HI: u32 = 0x11a4;
pub const FAILS_LO: u32 = 0x11b0;
pub const FAILS_HI: u32 = 0x11b4;

/// Bits of `CTRL`.
pub mod ctrl {
    pub const SLOT_SHIFT: u32 = 20;
    pub const SLOT_MASK: u32 = 0xf << SLOT_SHIFT;
    pub const LATCH: u32 = 1 << 28;
    pub const CLEAR_FAILS: u32 = 1 << 29;
    pub const CLEAR_FRAMES: u32 = 1 << 30;
}

/// Bits of `TABLE`.
pub mod table {
    pub const HI_SHIFT: u32 = 8;
    pub const COL_SHIFT: u32 = 16;
    pub const LO_EN: u32 = 1 << 26;
    pub const HI_EN: u32 = 1 << 27;
    pub const WRITE: u32 = 1 << 28;
}

/// Bits of `COMPRESS`.
pub mod compress {
    pub const ON: u32 = 1 << 31;
    pub const BASE: u32 = 1 << 30;
    /// what the stock system leaves with the test off: no compression, one sample per column
    pub const OFF: u32 = BASE | 1;
}

/// Bits of the SCU trigger control 0x4020 that belong to the mask test.
pub mod scu {
    pub const ENABLE: u32 = 1 << 15;
    /// keep frames that pass / that fail (both set while the test runs, as the stock system does)
    pub const SAVE_PASS: u32 = 1 << 14;
    pub const SAVE_FAIL: u32 = 1 << 13;
    /// the FSM stops on a failed frame
    pub const STOP_ON_FAIL: u32 = 1 << 12;
}

/// Columns of the table; the stock system always fills all of them and spreads them over the whole record.
pub const COLUMNS: usize = 1000;

/// The sample scale assumed for the comparator: the read-out word (16 bits, `MID` at the channel's offset,
/// `CODES_PER_DIV` per division, `seq::decode_volts`) shifted right by `SHIFT`. The stock system builds the limits as
/// screen coordinate >> 8, which fits. Change here if the unit says otherwise.
pub const MID: f64 = 32768.0;
pub const CODES_PER_DIV: f64 = 7500.0;
pub const SHIFT: u32 = 8;
/// Largest 8-bit code.
pub const CODE_MAX: f64 = 255.0;

/// The 8-bit comparator code of `volts` on a channel with `scale` V/div and `offset` V (the same values as the
/// decode), unrounded.
pub fn code(volts: f64, scale: f64, offset: f64) -> f64 {
    (MID + (volts + offset) / scale * CODES_PER_DIV) / (1u32 << SHIFT) as f64
}

/// One column of the table. The comparator fails a sample below `lo` (when `lo_en`) or above `hi` (when `hi_en`); a
/// sample equal to a limit passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub lo: u8,
    pub hi: u8,
    pub lo_en: bool,
    pub hi_en: bool,
}

impl Column {
    /// The column for the volt band `lo..=hi`. Limits are rounded outwards, so a sample the software mask passes is
    /// never failed here. A limit at or beyond the end of the code range (or infinite) is disabled.
    pub fn of_band(lo: f32, hi: f32, scale: f64, offset: f64) -> Column {
        let l = code(lo as f64, scale, offset).floor();
        let h = code(hi as f64, scale, offset).ceil();
        let lo_en = l.is_finite() && l > 0.0;
        let hi_en = h.is_finite() && h < CODE_MAX;
        Column { lo: if lo_en { l.min(CODE_MAX) as u8 } else { 0 }, hi: if hi_en { h.max(0.0) as u8 } else { CODE_MAX as u8 }, lo_en, hi_en }
    }

    /// The table word for column `c`, write enable set.
    pub fn word(&self, c: usize) -> u32 {
        let mut w = table::WRITE | (c as u32 & 0x3ff) << table::COL_SHIFT | (self.hi as u32) << table::HI_SHIFT | self.lo as u32;
        if self.lo_en {
            w |= table::LO_EN;
        }
        if self.hi_en {
            w |= table::HI_EN;
        }
        w
    }
}

/// The table of the volt bands `lo`/`hi` (one per column, `COLUMNS` of them) for a channel at `scale`/`offset`.
pub fn table(lo: &[f32], hi: &[f32], scale: f64, offset: f64) -> Vec<Column> {
    lo.iter().zip(hi).map(|(&l, &h)| Column::of_band(l, h, scale, offset)).collect()
}

/// Writes that load `cols` (column order 0..), then drop the write enable. The test must be off meanwhile.
pub fn load(cols: &[Column]) -> Vec<Write> {
    let mut w: Vec<Write> = cols.iter().enumerate().map(|(c, k)| (TABLE, k.word(c))).collect();
    w.push((TABLE, 0));
    w
}

/// The source slot of channel `ch` (0..3) with the channel set `mask`: assumed to follow the ADC lane layout
/// (`dvm::lane`): the channel itself with one, three or four channels; with two channels the lower one is slot 0 and the
/// higher slot 2. None when the channel is not acquired. Unverified (docs/fork/hwmask.md).
pub fn slot(mask: u32, ch: usize) -> Option<u32> {
    crate::dho::dvm::lane(mask, ch)
}

/// Samples per column for a record of `points` per channel.
pub fn per_column(points: u64) -> u32 {
    (points / COLUMNS as u64).clamp(1, 0x0fff_ffff) as u32
}

/// The compression word for `n` samples per column (the stock form; only bits 9:0 and 15 reach the comparator logic).
pub fn compress_word(n: u32) -> u32 {
    compress::BASE | if n > 1 { compress::ON } else { 0 } | (n & 0x0fff_ffff)
}

/// Writes that select `slot` in the control shadow `s1190` and, with `clear`, reset both counters.
pub fn select(s1190: &mut u32, slot: u32, clear: bool) -> Vec<Write> {
    *s1190 = (*s1190 & !(ctrl::SLOT_MASK | ctrl::LATCH | ctrl::CLEAR_FAILS | ctrl::CLEAR_FRAMES)) | (1 << (slot & 3)) << ctrl::SLOT_SHIFT;
    if clear {
        vec![(CTRL, *s1190 | ctrl::CLEAR_FAILS | ctrl::CLEAR_FRAMES), (CTRL, *s1190)]
    } else {
        vec![(CTRL, *s1190)]
    }
}

/// The SCU bits 15:12 of 0x4020 in `s4020` for the test on or off.
pub fn scu_bits(s4020: u32, on: bool, stop_on_fail: bool) -> u32 {
    let mut v = s4020 & !(scu::ENABLE | scu::STOP_ON_FAIL);
    if on {
        v |= scu::ENABLE | scu::SAVE_PASS | scu::SAVE_FAIL;
        if stop_on_fail {
            v |= scu::STOP_ON_FAIL;
        }
    }
    v
}

/// The raw counter registers after a latch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Raw {
    pub frames_lo: u32,
    pub frames_hi: u32,
    pub fails_lo: u32,
    pub fails_hi: u32,
}

const HI48: u32 = 0xffff;

impl Raw {
    /// Frames tested, 48 bits.
    pub fn frames(&self) -> u64 {
        ((self.frames_hi & HI48) as u64) << 32 | self.frames_lo as u64
    }

    /// Frames with a violation, 48 bits.
    pub fn fails(&self) -> u64 {
        ((self.fails_hi & HI48) as u64) << 32 | self.fails_lo as u64
    }
}

/// Latch (0, 1, 0 on bit 28 of the control shadow) and read both counters.
pub fn read(bus: &mut impl Bus, s1190: u32) -> Raw {
    let base = s1190 & !ctrl::LATCH;
    bus.wr(CTRL, base);
    bus.wr(CTRL, base | ctrl::LATCH);
    bus.wr(CTRL, base);
    Raw { frames_lo: bus.rd(FRAMES_LO), frames_hi: bus.rd(FRAMES_HI), fails_lo: bus.rd(FAILS_LO), fails_hi: bus.rd(FAILS_HI) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_of_volts() {
        // (volts, scale, offset) -> code: mid-scale is 128, one division is 7500/256 = 29.30 codes
        let cases: [(f64, f64, f64, f64); 6] = [
            (0.0, 1.0, 0.0, 128.0),
            (1.0, 1.0, 0.0, 128.0 + 7500.0 / 256.0),
            (-1.0, 1.0, 0.0, 128.0 - 7500.0 / 256.0),
            (0.5, 0.5, 0.0, 128.0 + 7500.0 / 256.0),
            // the offset moves the trace: 0 V with +1 V offset sits one division up
            (0.0, 1.0, 1.0, 128.0 + 7500.0 / 256.0),
            (4.0, 1.0, 0.0, 128.0 + 4.0 * 7500.0 / 256.0),
        ];
        for (v, s, o, want) in cases {
            assert!((code(v, s, o) - want).abs() < 1e-9, "{} {} {}: {}", v, s, o, code(v, s, o));
        }
    }

    #[test]
    fn columns_round_outwards_and_disable_at_the_ends() {
        let c = Column::of_band(-1.0, 1.0, 1.0, 0.0);
        assert_eq!(c, Column { lo: 98, hi: 158, lo_en: true, hi_en: true });
        // no data in the reference: both limits off
        assert_eq!(Column::of_band(f32::NEG_INFINITY, f32::INFINITY, 1.0, 0.0), Column { lo: 0, hi: 255, lo_en: false, hi_en: false });
        // beyond the code range (more than about 4.4 div from the centre): off
        assert_eq!(Column::of_band(-5.0, 5.0, 1.0, 0.0), Column { lo: 0, hi: 255, lo_en: false, hi_en: false });
        // a band on one side of the range keeps the other limit
        let c = Column::of_band(2.0, 9.0, 1.0, 0.0);
        assert!(c.lo_en && !c.hi_en && c.lo == 186);
    }

    #[test]
    fn table_words() {
        let k = Column { lo: 0x12, hi: 0x34, lo_en: true, hi_en: true };
        assert_eq!(k.word(999), 0x1c00_0000 | 999 << 16 | 0x3412);
        assert_eq!(Column { lo_en: false, ..k }.word(0), 0x1800_3412);
        assert_eq!(Column { hi_en: false, ..k }.word(1), 0x1401_3412);
        let w = load(&[k; COLUMNS]);
        assert_eq!(w.len(), COLUMNS + 1);
        assert!(w[..COLUMNS].iter().enumerate().all(|(c, x)| x.0 == 0x1194 && (x.1 >> 16) & 0x3ff == c as u32 && x.1 & table::WRITE != 0));
        assert_eq!(w[COLUMNS], (0x1194, 0));
    }

    #[test]
    fn slots_follow_the_lanes() {
        // (mask, channel) -> slot
        let cases = [(0b0001, 0, Some(0)), (0b0010, 1, Some(1)), (0b0011, 0, Some(0)), (0b0011, 1, Some(2)), (0b1100, 2, Some(0)), (0b1100, 3, Some(2)), (0b1111, 3, Some(3)), (0b0111, 2, Some(2)), (0b0001, 1, None)];
        for (m, c, want) in cases {
            assert_eq!(slot(m, c), want, "{:#b} CH{}", m, c + 1);
        }
    }

    #[test]
    fn compression_and_control() {
        assert_eq!(per_column(2_500_000), 2500);
        assert_eq!(per_column(500), 1);
        assert_eq!(compress_word(1), 0x4000_0001);
        assert_eq!(compress_word(1), compress::OFF);
        assert_eq!(compress_word(2500), 0xc000_09c4);
        let mut s = ctrl::LATCH | 0x00f0_0000;
        assert_eq!(select(&mut s, 2, true), vec![(0x1190, 0x6040_0000), (0x1190, 0x0040_0000)]);
        assert_eq!(select(&mut s, 0, false), vec![(0x1190, 0x0010_0000)]);
        assert_eq!(scu_bits(0x2001, true, false), 0xe001);
        assert_eq!(scu_bits(0x2001, true, true), 0xf001);
        assert_eq!(scu_bits(0xf001, false, true), 0x6001);
    }

    #[test]
    fn counters_are_48_bits_after_a_latch() {
        struct B(Vec<Write>);
        impl Bus for B {
            fn wr(&mut self, o: u32, v: u32) {
                self.0.push((o, v));
            }
            fn rd(&mut self, o: u32) -> u32 {
                match o {
                    FRAMES_LO => 0xffff_fff0,
                    FRAMES_HI => 0xdead_0001,
                    FAILS_LO => 7,
                    FAILS_HI => 0,
                    _ => 0,
                }
            }
        }
        let mut b = B(vec![]);
        let r = read(&mut b, 0x0010_0000 | ctrl::LATCH);
        assert_eq!(b.0, vec![(0x1190, 0x0010_0000), (0x1190, 0x1010_0000), (0x1190, 0x0010_0000)]);
        assert_eq!((r.frames(), r.fails()), (0x1_ffff_fff0, 7));
    }
}
