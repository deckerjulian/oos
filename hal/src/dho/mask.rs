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
/// Largest strobe count the comparator takes (0x1184 bits 9:0).
pub const MAX_COUNT: u32 = 1023;

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

/// Which 16-bit scale the comparator's 8-bit sample is cut from (the debug sweep, docs/fork/hwmask.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Domain {
    /// the read-out words: `CODES_PER_DIV` per division at the channel's V/div
    ReadOut,
    /// before the read path's digital gain: `CODES_PER_DIV / gain` per division (the scale of the trigger comparators
    /// and of the DVM block, whose 12-bit codes are this >> 4)
    PreGain,
}

/// How the table columns spread over the record (the debug sweep).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    /// `COLUMNS` columns, the strobe count = samples per column (points / 1000)
    Record,
    /// `COLUMNS` columns, the strobe count in sample clocks (one clock carries 4 / ratio samples of the channel)
    Clocks,
    /// the stock zone set-up: strobe count 1, 1000 / (4 / ratio) columns over the record
    Stock,
    /// `COLUMNS` columns, this strobe count
    Count(u32),
}

/// The value and time mapping of the comparator (assumptions under test). `Map::default()` is the first guess.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Map {
    pub domain: Domain,
    /// 16-bit codes per division before the domain's gain
    pub codes_per_div: f64,
    /// the 16-bit code at the channel's offset (screen centre)
    pub mid: f64,
    /// right shift from 16 to 8 bits
    pub shift: u32,
    /// added to every 8-bit code
    pub offset: f64,
    /// the code axis upside down (255 at the bottom of the screen)
    pub invert: bool,
    pub layout: Layout,
    /// columns with their limits enabled (others test nothing): to find the failing ones
    pub range: (u16, u16),
}

impl Default for Map {
    fn default() -> Map {
        Map { domain: Domain::ReadOut, codes_per_div: CODES_PER_DIV, mid: MID, shift: SHIFT, offset: 0.0, invert: false, layout: Layout::Record, range: (0, COLUMNS as u16 - 1) }
    }
}

impl Map {
    /// The 8-bit code of `volts` on a channel at `scale` V/div, `offset` V and digital gain `gain`, unrounded.
    pub fn code(&self, volts: f64, scale: f64, offset: f64, gain: f64) -> f64 {
        let g = if self.domain == Domain::PreGain && gain > 0.0 { gain } else { 1.0 };
        let mut d = (volts + offset) / scale * self.codes_per_div / g;
        if self.invert {
            d = -d;
        }
        (self.mid + d) / (1u64 << self.shift.min(16)) as f64 + self.offset
    }

    /// The column layout for `points` per channel with the channel set `mask`: (columns, strobe count, samples per
    /// column).
    /// The strobe count is kept to `MAX_COUNT` (only bits 9:0 reach the comparator) and to what the record holds for
    /// all columns: a count whose columns run past the end of the record stalled the acquisition on the unit (round 5,
    /// 15-25 s without frames). Columns past `MAX_COUNT` cover the start of a long record only.
    pub fn layout(&self, points: u32, mask: u32) -> (usize, u32, f64) {
        let per_clock = (4 / crate::dho::hz::ratio(mask)).max(1) as u64;
        let pts = points.max(1) as u64;
        let fit = |n: u64, unit: u64| n.min(MAX_COUNT as u64).min((pts / COLUMNS as u64 / unit).max(1)).max(1) as u32;
        match self.layout {
            Layout::Record => {
                let n = fit(pts / COLUMNS as u64, 1);
                (COLUMNS, n, n as f64)
            }
            Layout::Clocks => {
                let n = fit(pts / COLUMNS as u64 / per_clock, per_clock);
                (COLUMNS, n, (n as u64 * per_clock) as f64)
            }
            Layout::Stock => (COLUMNS / per_clock as usize, 1, per_clock as f64),
            Layout::Count(n) => {
                let n = fit(n as u64, 1);
                (COLUMNS, n, n as f64)
            }
        }
    }

    /// Parses `key=value` into the map: domain readout|pregain, cpd <codes/div>, mid <code>, shift <bits>, offs
    /// <8-bit codes>, invert 0|1, cols record|clocks|stock|<count>, range <a>-<b>.
    pub fn set(&mut self, kv: &str) -> Result<(), String> {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("no '=' in {}", kv))?;
        let (k, v) = (k.trim(), v.trim());
        let num = |v: &str| v.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("not a number: {}", v));
        match k {
            "domain" => {
                self.domain = match v {
                    "readout" => Domain::ReadOut,
                    "pregain" => Domain::PreGain,
                    _ => return Err(format!("domain readout|pregain: {}", v)),
                }
            }
            "cpd" => self.codes_per_div = num(v)?.max(1.0),
            "mid" => self.mid = num(v)?,
            "shift" => self.shift = num(v)?.clamp(0.0, 16.0) as u32,
            "offs" => self.offset = num(v)?,
            "invert" => self.invert = matches!(v, "1" | "on" | "true"),
            "cols" => {
                self.layout = match v {
                    "record" => Layout::Record,
                    "clocks" => Layout::Clocks,
                    "stock" => Layout::Stock,
                    n => Layout::Count(num(n)?.clamp(1.0, 0x0fff_ffff as f64) as u32),
                }
            }
            "range" => {
                let (a, b) = v.split_once('-').ok_or_else(|| format!("range a-b: {}", v))?;
                let (a, b) = (num(a)? as u16, num(b)? as u16);
                self.range = (a.min(b), a.max(b));
            }
            _ => return Err(format!("unknown key {}", k)),
        }
        Ok(())
    }

    /// The map as `set` takes it, one line.
    pub fn text(&self) -> String {
        let cols = match self.layout {
            Layout::Record => "record".to_string(),
            Layout::Clocks => "clocks".to_string(),
            Layout::Stock => "stock".to_string(),
            Layout::Count(n) => n.to_string(),
        };
        let d = if self.domain == Domain::PreGain { "pregain" } else { "readout" };
        format!("domain={} cpd={} mid={} shift={} offs={} invert={} cols={} range={}-{}", d, self.codes_per_div, self.mid, self.shift, self.offset, self.invert as u8, cols, self.range.0, self.range.1)
    }
}

/// The union of the bands of the equal source columns `lo`/`hi` (spread over the record) that overlap the record
/// fraction `f0..f1`.
pub fn rebin(lo: &[f32], hi: &[f32], f0: f64, f1: f64) -> (f32, f32) {
    let n = lo.len().min(hi.len());
    if n == 0 || f1 <= 0.0 || f0 >= 1.0 {
        return (f32::NEG_INFINITY, f32::INFINITY);
    }
    let a = ((f0.max(0.0) * n as f64).floor() as usize).min(n - 1);
    let b = ((f1.min(1.0) * n as f64).ceil() as usize).clamp(a + 1, n);
    (lo[a..b].iter().copied().fold(f32::INFINITY, f32::min), hi[a..b].iter().copied().fold(f32::NEG_INFINITY, f32::max))
}

/// Where the record and the screen lie in time: the record has `points` samples at `rate` with the trigger at sample
/// `pre` (channel set `mask`); the screen (and the software mask, which spreads over it) starts `screen.0` seconds
/// from the trigger and is `screen.1` seconds wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub points: u32,
    pub pre: u32,
    pub rate: u64,
    pub mask: u32,
    pub screen: (f64, f64),
}

impl Timing {
    /// The screen fraction (0 = left edge, 1 = right edge) at sample `k` of the record.
    pub fn screen_at(&self, k: f64) -> f64 {
        ((k - self.pre as f64) / self.rate.max(1) as f64 - self.screen.0) / self.screen.1.max(1e-15)
    }
}

/// The table for the volt bands `lo`/`hi` (spread over the screen, as the software mask) under `map`: one column per
/// hardware column of `map.layout`, each the union of the bands its samples cover on the screen; columns outside
/// `map.range`, beyond the record or not wholly on the screen test nothing. The record is longer than the screen (1.6 times at
/// 10k points from 20 us/div, unit round 5), so the columns map by time, not by fraction of the record.
pub fn table_map(lo: &[f32], hi: &[f32], t: &Timing, scale: f64, offset: f64, gain: f64, map: &Map) -> Vec<Column> {
    let (cols, _, per) = map.layout(t.points, t.mask);
    let pts = t.points.max(1) as f64;
    (0..cols)
        .map(|c| {
            let (k0, k1) = (c as f64 * per, (c + 1) as f64 * per);
            let (f0, f1) = (t.screen_at(k0), t.screen_at(k1));
            // a column reaching past a screen edge holds samples the software mask never sees: it tests nothing
            if k0 >= pts || f0 < 0.0 || f1 > 1.0 || c < map.range.0 as usize || c > map.range.1 as usize {
                return Column { lo: 0, hi: CODE_MAX as u8, lo_en: false, hi_en: false };
            }
            let (l, h) = rebin(lo, hi, f0, f1);
            let (cl, ch) = (map.code(l as f64, scale, offset, gain), map.code(h as f64, scale, offset, gain));
            // inverted: the lower volt limit becomes the upper code limit
            let (a, b) = if cl <= ch { (cl, ch) } else { (ch, cl) };
            Column::of_codes(a, b)
        })
        .collect()
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
        Column::of_codes(code(lo as f64, scale, offset), code(hi as f64, scale, offset))
    }

    /// The column for the code band `lo..=hi`, rounded outwards; a limit at or beyond the code range is disabled.
    pub fn of_codes(lo: f64, hi: f64) -> Column {
        let (l, h) = (lo.floor(), hi.ceil());
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


/// Writes that load `cols` (column order 0..), then drop the write enable. The test must be off meanwhile.
pub fn load(cols: &[Column]) -> Vec<Write> {
    let mut w: Vec<Write> = cols.iter().enumerate().map(|(c, k)| (TABLE, k.word(c))).collect();
    w.push((TABLE, 0));
    w
}

/// The source slot of channel `ch` (0..3) with the channel set `mask`: a stream slot, not an ADC lane (libscope
/// `SetMaskChanEn`, regmap): slot 0 with one channel; with two the lower channel 0, the higher 1; with three or four
/// the channel itself. None when the channel is not acquired.
pub fn slot(mask: u32, ch: usize) -> Option<u32> {
    if ch > 3 || mask & (1 << ch) == 0 {
        return None;
    }
    Some(match crate::dho::hz::ratio(mask) {
        1 => 0,
        2 => (0..ch).any(|c| mask & (1 << c) != 0) as u32,
        _ => ch as u32,
    })
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
    fn map_codes_by_domain_shift_offset_and_inversion() {
        let d = Map::default();
        // the default is the first guess: the read-out word >> 8
        assert_eq!(d.code(1.0, 1.0, 0.0, 1.095), code(1.0, 1.0, 0.0));
        // before the digital gain: 7500 / g codes per division (the DVM block's 428 12-bit codes at g 1.095)
        let pg = Map { domain: Domain::PreGain, ..d };
        assert!((pg.code(1.0, 1.0, 0.0, 1.095) - (128.0 + 7500.0 / 1.095 / 256.0)).abs() < 1e-9);
        assert!(((pg.code(1.0, 1.0, 0.0, 1.095) - 128.0) * 16.0 - 428.0).abs() < 1.0);
        // a 12-bit code >> 4 is the same as a 16-bit >> 8 of the left-aligned word: shift 4 with mid 2048, 469 codes/div
        let twelve = Map { mid: 2048.0, codes_per_div: 7500.0 / 16.0, shift: 4, ..d };
        assert!((twelve.code(1.0, 1.0, 0.0, 1.0) - d.code(1.0, 1.0, 0.0, 1.0)).abs() < 1e-9);
        assert_eq!(Map { offset: 3.0, ..d }.code(0.0, 1.0, 0.0, 1.0), 131.0);
        assert!((Map { invert: true, ..d }.code(1.0, 1.0, 0.0, 1.0) - (128.0 - 7500.0 / 256.0)).abs() < 1e-9);
    }

    #[test]
    fn layouts() {
        // (layout, points, channel set) -> (columns, strobe count, samples per column)
        let cases: [(Layout, u32, u32, (usize, u32, f64)); 9] = [
            // 2.5 Mpts would need 2500: kept to 1023, the columns cover the first 1.02 M samples
            (Layout::Record, 2_500_000, 1, (1000, 1023, 1023.0)),
            (Layout::Record, 10_000, 1, (1000, 10, 10.0)),
            (Layout::Record, 500, 1, (1000, 1, 1.0)),
            (Layout::Clocks, 2_500_000, 1, (1000, 625, 2500.0)),
            (Layout::Clocks, 10_000, 0b11, (1000, 5, 10.0)),
            (Layout::Stock, 10_000, 1, (250, 1, 4.0)),
            (Layout::Stock, 10_000, 0b1111, (1000, 1, 1.0)),
            (Layout::Count(7), 10_000, 1, (1000, 7, 7.0)),
            // a count that runs past the record (round 5: 400 at 10k stalled the acquisition): kept to the record
            (Layout::Count(400), 10_000, 1, (1000, 10, 10.0)),
        ];
        for (l, pts, m, want) in cases {
            assert_eq!(Map { layout: l, ..Map::default() }.layout(pts, m), want, "{:?} {} {:#b}", l, pts, m);
        }
    }

    #[test]
    fn map_keys_round_trip() {
        let mut m = Map::default();
        for kv in ["domain=pregain", "cpd=6848", "mid=32000", "shift=8", "offs=-2.5", "invert=1", "cols=clocks", "range=10-20"] {
            m.set(kv).unwrap();
        }
        assert_eq!(m.text(), "domain=pregain cpd=6848 mid=32000 shift=8 offs=-2.5 invert=1 cols=clocks range=10-20");
        let mut back = Map::default();
        for kv in m.text().split(' ') {
            back.set(kv).unwrap();
        }
        assert_eq!(back, m);
        m.set("cols=37").unwrap();
        assert_eq!(m.layout, Layout::Count(37));
        assert!(m.set("cols=").is_err() && m.set("domain=raw").is_err() && m.set("nope=1").is_err() && m.set("cpd").is_err());
    }

    #[test]
    fn table_map_rebins_limits_the_range_and_inverts() {
        // 1000 source bands over the screen: -1..1 V, band 500 (the screen centre) reaches +2 V
        let (mut lo, mut hi) = (vec![-1.0f32; 1000], vec![1.0f32; 1000]);
        hi[500] = 2.0;
        let d = Map::default();
        // a record that is the screen: 10,000 samples at 10 MSa/s, trigger in the middle, screen -0.5..0.5 ms
        let same = Timing { points: 10_000, pre: 5000, rate: 10_000_000, mask: 1, screen: (-0.5e-3, 1e-3) };
        let t = table_map(&lo, &hi, &same, 1.0, 0.0, 1.0, &d);
        assert_eq!(t.len(), 1000);
        assert_eq!(t[0], Column::of_band(-1.0, 1.0, 1.0, 0.0));
        assert_eq!(t[500], Column::of_band(-1.0, 2.0, 1.0, 0.0));
        // stock layout: 250 columns of 4 samples at the start of the record
        let t = table_map(&lo, &hi, &same, 1.0, 0.0, 1.0, &Map { layout: Layout::Stock, ..d });
        assert_eq!((t.len(), t[0]), (250, Column::of_band(-1.0, 1.0, 1.0, 0.0)));
        // only columns 10..=20 test
        let t = table_map(&lo, &hi, &same, 1.0, 0.0, 1.0, &Map { range: (10, 20), ..d });
        assert!(t.iter().enumerate().all(|(c, k)| (k.lo_en && k.hi_en) == (10..=20).contains(&c)));
        // inverted: the volt band maps to the mirrored codes, lower limit below the upper
        let t = table_map(&lo, &hi, &same, 1.0, 0.0, 1.0, &Map { invert: true, ..d });
        assert_eq!(t[0], Column::of_codes(128.0 - 7500.0 / 256.0, 128.0 + 7500.0 / 256.0));
        assert!(t[500].lo < t[0].lo);
        // a band without data stays open
        lo[3] = f32::NEG_INFINITY;
        hi[3] = f32::INFINITY;
        assert!(!table_map(&lo, &hi, &same, 1.0, 0.0, 1.0, &d)[3].lo_en);
    }

    /// Round 5: the record is 1.6 screens long (10k points at 6.25 MSa/s, 100 us/div). The columns map by time: the
    /// screen centre band lands in the record's centre column, the record's ends off the screen test nothing.
    #[test]
    fn a_record_longer_than_the_screen_maps_by_time() {
        let (lo, mut hi) = (vec![-1.0f32; 1000], vec![1.0f32; 1000]);
        hi[500] = 2.0;
        let t = Timing { points: 10_000, pre: 5000, rate: 6_250_000, mask: 1, screen: (-0.5e-3, 1e-3) };
        let tab = table_map(&lo, &hi, &t, 1.0, 0.0, 1.0, &Map::default());
        // column 500 holds samples 5000..5010, t = 0..1.6 us: screen fraction 0.5..0.5016, band 500
        assert_eq!(tab[500], Column::of_band(-1.0, 2.0, 1.0, 0.0));
        // the screen spans samples 1875..8125: columns 187..812; outside, nothing is tested
        assert!(!tab[100].lo_en && !tab[100].hi_en && !tab[900].hi_en);
        assert!(tab[200].lo_en && tab[800].hi_en);
        assert_eq!(t.screen_at(5000.0), 0.5);
        assert!((t.screen_at(1875.0)).abs() < 1e-9 && (t.screen_at(8125.0) - 1.0).abs() < 1e-9);
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
    fn slots_are_stream_slots() {
        // (mask, channel) -> slot
        let cases = [(0b0001, 0, Some(0)), (0b0010, 1, Some(0)), (0b1000, 3, Some(0)), (0b0011, 0, Some(0)), (0b0011, 1, Some(1)), (0b1100, 2, Some(0)), (0b1100, 3, Some(1)), (0b1111, 3, Some(3)), (0b0111, 2, Some(2)), (0b0001, 1, None)];
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
