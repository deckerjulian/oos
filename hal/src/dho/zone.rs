// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The FPGA zone trigger (docs/fork/hwzone.md): two zone tables on the mask unit's sample path and column strobe, a
//! per-zone "touched" result, and the combined pass flag that qualifies the trigger. Register facts come from static
//! analysis of the FPGA image; the time and value mapping are assumptions until the unit session.

use crate::dho::bus::Bus;
use crate::dho::{mask, zonegeom, Write};

/// Zone control: bit 24/25 zone 1/2 on, bit 26/27 zone 1/2 mode (1 = pass when touched, 0 = pass when not touched).
pub const CTRL: u32 = 0x11d0;
/// Table write ports of zone 1 and 2: 7:0 lower limit, 15:8 upper limit, 16 column in the zone, 27:18 column, 28 write
/// enable (level-sensitive).
pub const TABLES: [u32; 2] = [0x11d8, 0x11dc];
/// Status reads: 0x4030 bit 28 zone logic armed, 0x4038 bit 19 the latched zone pass flag.
pub const ARMED_REG: u32 = 0x4030;
pub const ARMED: u32 = 1 << 28;
pub const PASS_REG: u32 = 0x4038;
pub const PASS: u32 = 1 << 19;

/// Bits of `CTRL`.
pub mod ctrl {
    pub const ON_SHIFT: u32 = 24;
    pub const MODE_SHIFT: u32 = 26;
}

/// Bits of the table words.
pub mod table {
    pub const HI_SHIFT: u32 = 8;
    pub const IN_ZONE: u32 = 1 << 16;
    pub const COL_SHIFT: u32 = 18;
    pub const WRITE: u32 = 1 << 28;
}

/// The SCU bit of 0x4020 that arms the zone logic.
pub const SCU_ENABLE: u32 = 1 << 1;

/// One table column: the column lies in the zone's time span (`on`), and the zone covers codes `lo..=hi` there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub lo: u8,
    pub hi: u8,
    pub on: bool,
}

impl Column {
    pub const OUT: Column = Column { lo: 0, hi: 0, on: false };

    /// The column for the zone's value range `lo..=hi` (volts) on a channel at `scale`/`offset`, rounded outwards to
    /// whole codes and clipped to the code range; off when the range lies wholly outside it.
    pub fn of_range(lo: f64, hi: f64, scale: f64, offset: f64) -> Column {
        let l = mask::code(lo, scale, offset).floor();
        let h = mask::code(hi, scale, offset).ceil();
        if !(l.is_finite() && h.is_finite()) || h < 0.0 || l > mask::CODE_MAX || h < l {
            return Column::OUT;
        }
        Column { lo: l.max(0.0) as u8, hi: h.min(mask::CODE_MAX) as u8, on: true }
    }

    /// The table word for column `c`, write enable set.
    pub fn word(&self, c: usize) -> u32 {
        let flag = if self.on { table::IN_ZONE } else { 0 };
        table::WRITE | (c as u32 & 0x3ff) << table::COL_SHIFT | flag | (self.hi as u32) << table::HI_SHIFT | self.lo as u32
    }
}

/// A zone as the hardware takes it: the outline (time relative to the trigger in s, volts of the source channel) and
/// whether a frame passes when it touches the zone (`inside`) or when it does not.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub poly: Vec<(f64, f64)>,
    pub inside: bool,
}

/// The column layout over a record: `mask::COLUMNS` columns of `mask::per_column(points)` samples, the same as the mask
/// unit (shared strobe). The stock system leaves the strobe at one count and writes fewer columns (1000 / (4 / ratio));
/// docs/fork/hwzone.md says what the bench decides.
pub fn columns(points: u32) -> (usize, u32) {
    (mask::COLUMNS, mask::per_column(points as u64))
}

/// The table of zone `z` for a record of `points` samples at `rate` with the trigger at sample `pre`, on a channel at
/// `scale`/`offset`. Column c covers samples c n .. (c + 1) n, at times (k - pre) / rate.
pub fn table(z: &Zone, points: u32, rate: u64, pre: u32, scale: f64, offset: f64) -> Vec<Column> {
    let (cols, n) = columns(points);
    let dt = 1.0 / rate.max(1) as f64;
    let poly = box_or_poly(&z.poly);
    (0..cols)
        .map(|c| {
            let ta = (c as f64 * n as f64 - pre as f64) * dt;
            let tb = ((c + 1) as f64 * n as f64 - pre as f64) * dt;
            zonegeom::extent(&poly, ta, tb).map_or(Column::OUT, |(lo, hi)| Column::of_range(lo, hi, scale, offset))
        })
        .collect()
}

/// Two corner points are a box: its four corners.
fn box_or_poly(p: &[(f64, f64)]) -> Vec<(f64, f64)> {
    match p {
        [(t0, v0), (t1, v1)] => vec![(*t0, *v0), (*t1, *v0), (*t1, *v1), (*t0, *v1)],
        _ => p.to_vec(),
    }
}

/// Writes that load table `k` (0 or 1) with `cols`, then drop the write enable.
pub fn load(k: usize, cols: &[Column]) -> Vec<Write> {
    let reg = TABLES[k & 1];
    let mut w: Vec<Write> = cols.iter().enumerate().map(|(c, x)| (reg, x.word(c))).collect();
    w.push((reg, 0));
    w
}

/// The control word for up to two zones.
pub fn ctrl_word(zones: &[Zone]) -> u32 {
    zones.iter().take(2).enumerate().fold(0, |w, (k, z)| w | 1 << (ctrl::ON_SHIFT + k as u32) | (z.inside as u32) << (ctrl::MODE_SHIFT + k as u32))
}

/// 0x4020 with the zone logic armed or not.
pub fn scu_bit(s4020: u32, on: bool) -> u32 {
    if on {
        s4020 | SCU_ENABLE
    } else {
        s4020 & !SCU_ENABLE
    }
}

/// A reading of the zone unit: the status words and the mask unit's counters (its frame counter is qualified by the
/// zone result).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Raw {
    pub s4030: u32,
    pub s4038: u32,
    pub counters: mask::Raw,
}

impl Raw {
    pub fn armed(&self) -> bool {
        self.s4030 & ARMED != 0
    }

    pub fn pass(&self) -> bool {
        self.s4038 & PASS != 0
    }
}

/// The status words and the latched counters.
pub fn read(bus: &mut impl Bus, s1190: u32) -> Raw {
    let counters = mask::read(bus, s1190);
    Raw { s4030: bus.rd(ARMED_REG), s4038: bus.rd(PASS_REG), counters }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_of_value_ranges() {
        // 1 V/div: 0 V is code 128, one division 29.3 codes; rounded outwards
        assert_eq!(Column::of_range(-1.0, 1.0, 1.0, 0.0), Column { lo: 98, hi: 158, on: true });
        // clipped at the ends of the code range
        assert_eq!(Column::of_range(-9.0, 0.0, 1.0, 0.0), Column { lo: 0, hi: 128, on: true });
        assert_eq!(Column::of_range(3.0, 9.0, 1.0, 0.0), Column { lo: 215, hi: 255, on: true });
        // wholly off screen: off
        assert_eq!(Column::of_range(6.0, 9.0, 1.0, 0.0), Column::OUT);
        assert_eq!(Column::of_range(-9.0, -6.0, 1.0, 0.0), Column::OUT);
    }

    #[test]
    fn table_words_and_control() {
        let k = Column { lo: 0x12, hi: 0x34, on: true };
        assert_eq!(k.word(999), 0x1000_0000 | 999 << 18 | 1 << 16 | 0x3412);
        assert_eq!(Column::OUT.word(1), 0x1000_0000 | 1 << 18);
        let w = load(1, &[k; 3]);
        assert_eq!(w, vec![(0x11dc, k.word(0)), (0x11dc, k.word(1)), (0x11dc, k.word(2)), (0x11dc, 0)]);
        let z = |inside| Zone { poly: vec![], inside };
        assert_eq!(ctrl_word(&[]), 0);
        assert_eq!(ctrl_word(&[z(true)]), 0x0500_0000);
        assert_eq!(ctrl_word(&[z(false)]), 0x0100_0000);
        assert_eq!(ctrl_word(&[z(true), z(false)]), 0x0700_0000);
        assert_eq!(ctrl_word(&[z(false), z(true)]), 0x0b00_0000);
        assert_eq!((scu_bit(0x2001, true), scu_bit(0x2003, false)), (0x2003, 0x2001));
    }

    #[test]
    fn a_box_lands_in_its_columns() {
        // 10,000 points at 1 GSa/s, trigger in the middle (sample 5000): 10 samples = 10 ns per column, columns
        // 500.. at t >= 0; a box from 100 ns to 200 ns, 0.5..1.5 V at 1 V/div
        let z = Zone { poly: vec![(100e-9, 0.5), (200e-9, 1.5)], inside: true };
        let t = table(&z, 10_000, 1_000_000_000, 5000, 1.0, 0.0);
        assert_eq!(t.len(), 1000);
        let on: Vec<usize> = (0..1000).filter(|&c| t[c].on).collect();
        // the columns over 100..200 ns: 510..519, and the neighbours that only touch a border may count (rounding)
        assert!((509..=510).contains(&on[0]) && (519..=520).contains(on.last().unwrap()), "{:?}", on);
        assert!((510..=519).all(|c| t[c].on));
        assert_eq!(t[515], Column::of_range(0.5, 1.5, 1.0, 0.0));
        assert!(!t[0].on && !t[999].on);
    }

    #[test]
    fn status_and_counters() {
        struct B;
        impl Bus for B {
            fn wr(&mut self, _: u32, _: u32) {}
            fn rd(&mut self, o: u32) -> u32 {
                match o {
                    0x4030 => 1 << 28,
                    0x4038 => 1 << 19,
                    0x11a0 => 42,
                    _ => 0,
                }
            }
        }
        let r = read(&mut B, 0x0010_0000);
        assert!(r.armed() && r.pass());
        assert_eq!(r.counters.frames(), 42);
    }
}
