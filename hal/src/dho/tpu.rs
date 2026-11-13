// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Trigger processing unit (0x2000 block): all trigger types, as in `tools/tpumodel.py`.
//! docs/protocols/trigger.md: edge = writes 1-17 of section 6.1 (configuration, level, holdoff),
//! the other types = section 10 (one sequence each, primitives in 10.1). All TPU registers are
//! write-only. Sweep AUTO/NORMAL lives in the SCU (0x4020 bit0, 0x4060/0x4064), see acquisition-start.md.
//!
//! Each sequence writes only what its type uses: event B/R, the ABR flags 0x205c[10:6], the Nth-edge
//! count/idle time and the video registers keep what an earlier type left, and some types keep 0x201c[15:0]
//! (trigger.md 10.1). Times are in femtoseconds, voltages in nV at the BNC. Sources are numbered:
//! 1..4 = CH1..CH4, 9..24 = D0..D15, 25 = AC line, 26 = EXT; level registers exist for CH1..CH4 only.

use crate::dho::Write;

/// Software copies of the write-only TPU registers the sequences modify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shadows {
    /// event A selection / pattern / time condition
    pub s201c: u32,
    /// event B, same layout
    pub s2034: u32,
    /// trigger mux [4:0], force bit5, ABR flags [10:6], SPU bypass bit12
    pub s205c: u32,
    /// holdoff ticks [47:32] and holdoff type [18:16]
    pub s2088: u32,
    /// event R, same layout as 0x201c (video only)
    pub s204c: u32,
    /// trigger coupling filter
    pub s208c: u32,
    /// event A pattern of inputs 0..15 (pattern, duration)
    pub s2014: u32,
    /// event A pattern of inputs 16..20 [9:0] and logic function [11:10]
    pub s2018: u32,
}

impl Shadows {
    /// The values of the stock system for edge, CH1, rising (RUN/STOP snapshot; 0x204c/0x2014/0x2018 are not in the snapshot
    /// and taken as 0, as in tpumodel.py).
    pub const APP: Shadows =
        Shadows { s201c: 0x00aa_4100, s2034: 0x6060, s205c: 0, s2088: 0x40000, s204c: 0, s208c: 0, s2014: 0, s2018: 0 };
}

/// Edge selection of the edge, window and timeout triggers (codes 0, 1, 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slope {
    Rising,
    Falling,
    Either,
}

/// Coupling of the edge trigger (trigger.md 2.5): a filter of the TPU on the samples of the source, code in
/// 0x208c[1:0]. GND is not a trigger coupling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Coupling {
    #[default]
    Dc,
    Ac,
    LfReject,
    HfReject,
}

impl Coupling {
    /// 0x208c[1:0]: DC 0, HF reject 1, LF reject 2, AC 3.
    pub fn code(self) -> u32 {
        match self {
            Coupling::Dc => 0,
            Coupling::HfReject => 1,
            Coupling::LfReject => 2,
            Coupling::Ac => 3,
        }
    }
}

/// Single edge of the types with a two-way choice (slope, delay, setup/hold clock, Nth edge): 0 rising, 1 falling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Rising,
    Falling,
}

/// Polarity of pulse and runt: 0 positive, 1 negative, 2 any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarity {
    Pos,
    Neg,
    Any,
}

/// Time condition (TPU code 0x201c[26:24]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// t > lower (code 0)
    Gt,
    /// t < upper (1)
    Lt,
    /// lower < t < upper (2)
    In,
    /// outside lower..upper (3)
    Out,
    /// no condition (5; runt turns it into 0 with both limits 0)
    None,
}

impl When {
    fn code(self) -> u32 {
        match self {
            When::Gt => 0,
            When::Lt => 1,
            When::In => 2,
            When::Out => 3,
            When::None => 5,
        }
    }
}

/// Window trigger event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    Enter,
    Exit,
    Time,
}

/// Setup/hold event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShEvent {
    Setup,
    Hold,
    Both,
}

/// Valid data level of the setup/hold trigger: 0 H, 1 L.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataLevel {
    H,
    L,
}

/// One pattern entry (H 0, L 1, X 2, rise 3, fall 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pat {
    H,
    L,
    X,
    Rise,
    Fall,
}

/// Pattern/duration logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logic {
    And,
    Or,
    Nand,
    Nor,
}

/// Video sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoSync {
    AnyLine,
    Line,
    OddField,
    EvenField,
}

/// Video standard (only NTSC and PAL are implemented).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoStd {
    Ntsc,
    Pal,
}

/// Divisor of `video2`, the FPGA clock cycle (trigger.md 10.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoClk {
    /// 4 ns: what the unit runs
    Fast,
    /// 6.4 ns: the TPU tick
    Stock,
}

/// Vertical settings of one channel for its level registers: `scale` nV/div, `offset` nV, `g` digital gain (1.0 if
/// unknown), `noise` hysteresis in div (`noise_div`, or the table value at <= 2 mV/div).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vert {
    pub scale: f64,
    pub offset: f64,
    pub g: f64,
    pub noise: f64,
}

/// Time unit of the event blocks A, B, R: 1.6 ns (2 ADC periods). The tick counts the ADC-derived clock, so real times are
/// taken to the nominal 1.25 GHz domain first (`clock::to_nominal_fs`, identity at 1.25 GHz).
const TICK_FS: i128 = 1_600_000;
/// Holdoff and R timeout unit: 6.4 ns.
const TICK64_FS: i128 = 6_400_000;
/// TPU code "timeout" of 0x201c[26:24] (window "time", timeout trigger).
const WHEN_TIMEOUT: u32 = 4;

/// Level registers (comparator A, B) of channel 1..4.
pub fn level_regs(ch: u32) -> (u32, u32) {
    match ch {
        1 => (0x2004, 0x2008),
        2 => (0x200c, 0x2010),
        3 => (0x20b8, 0x20bc),
        _ => (0x20c0, 0x20c4),
    }
}

/// Comparator code of `v` nV at `scale` nV/div and digital gain `g`: trunc(v * 7500 / scale / g + 32768), negative
/// -> 0, then clamped to 1..0xfffe.
pub fn code(v: f64, scale: f64, g: f64) -> u32 {
    let x = v * 7500.0 / scale / g + 32768.0;
    let c = if x <= 0.0 { 0 } else { x as u32 };
    c.clamp(1, 0xfffe)
}

/// Level A and B register values for level+offset `v` (nV) and hysteresis `hyst` (nV): A = code(v) | code(v-h) << 16,
/// B = code(v+h) | code(v) << 16.
pub fn levels(v: f64, hyst: f64, scale: f64, g: f64) -> (u32, u32) {
    let c = |x: f64| code(x, scale, g);
    (c(v) | c(v - hyst) << 16, c(v + hyst) | c(v) << 16)
}

/// Hysteresis in nV: (s64)(noise * (float)scale), single precision, truncated.
pub fn hyst(noise: f64, scale: f64) -> f64 {
    ((noise as f32) * (scale as f32)) as i64 as f64
}

/// Hysteresis in div for a scale at the BNC (nV/div) of the master source at 1 MOhm: the table up to 2 mV/div, else
/// `noise_div` (trigger.md 2.3). A scale that went through f32 may read a little off its step.
pub fn noise_hysteresis(scale_nv: f64, noise_reject: bool) -> f64 {
    let s = (scale_nv / 100.0).round() as i64 * 100;
    let (off, on) = match s {
        100_000..=199_999 => (2.3, 2.35),
        200_000..=499_999 => (1.15, 1.2),
        500_000..=999_999 => (1.25, 1.3),
        1_000_000..=1_999_999 => (1.0, 1.0),
        2_000_000 => (0.5, 0.7),
        _ if s < 100_000 => (2.3, 2.35),
        _ => return noise_div(noise_reject),
    };
    if noise_reject {
        on
    } else {
        off
    }
}

/// Hysteresis in div for a scale (nV/div) above 2 mV/div: 0.3, with noise reject 0.7 (below: `noise_hysteresis`).
pub fn noise_div(noise_reject: bool) -> f64 {
    if noise_reject {
        0.7
    } else {
        0.3
    }
}

/// Comparator code of the source's level A (CHn 2n-2, Dk k+32, AC 48, EXT 49, else 51).
fn cmp_a(src: u32) -> u32 {
    match src {
        1..=4 => 2 * src - 2,
        9..=24 => src + 0x17,
        25 => 48,
        26 => 49,
        _ => 51,
    }
}

/// As `cmp_a` with CHn -> 2n-1 (level B).
fn cmp_b(src: u32) -> u32 {
    match src {
        1..=4 => 2 * src - 1,
        _ => cmp_a(src),
    }
}

/// The SPU bypass test of pulse, slope, timeout, delay, setup/hold and Nth edge: 9 <= src < 28 (D0..D15, AC line, EXT).
fn digital_or_ext(src: u32) -> bool {
    (9..0x1c).contains(&src)
}

/// Event blocks: A 0x201c, B 0x2034, R 0x204c.
#[derive(Clone, Copy)]
enum Ev {
    A,
    B,
    R,
}

/// The register primitives (trigger.md 10.1) on the shadows, collecting the writes.
struct Tpu<'a> {
    sh: &'a mut Shadows,
    w: Vec<Write>,
}

impl Tpu<'_> {
    fn ev(&mut self, ev: Ev) -> (u32, &mut u32) {
        match ev {
            Ev::A => (0x201c, &mut self.sh.s201c),
            Ev::B => (0x2034, &mut self.sh.s2034),
            Ev::R => (0x204c, &mut self.sh.s204c),
        }
    }

    /// `source(E; a, ea, b, eb)`: [15:0] = a | ea << 6 | b << 8 | eb << 14.
    fn source(&mut self, ev: Ev, a: u32, ea: u32, b: u32, eb: u32) {
        let (r, s) = self.ev(ev);
        *s = (*s & 0xffff_0000) | (a & 0x3f) | (ea & 3) << 6 | (b & 0x3f) << 8 | (eb & 3) << 14;
        let v = *s;
        self.w.push((r, v));
    }

    /// `pattern(E; p0, p1, p2, p3)`: [23:16] = p0 | p1 << 3 | p2 << 4 | p3 << 7.
    fn pattern(&mut self, ev: Ev, p0: u32, p1: u32, p2: u32, p3: u32) {
        let (r, s) = self.ev(ev);
        *s = (*s & 0xff00_ffff) | (p0 & 7) << 16 | (p1 & 1) << 19 | (p2 & 7) << 20 | (p3 & 1) << 23;
        let v = *s;
        self.w.push((r, v));
    }

    /// `trigger(E; when, t1, t2)`: [26:24] = when, then t1 and t2 in 1.6 ns ticks
    /// (truncated toward 0, 48 bit) at base+4, base+8 and their high parts at base+0xc.
    fn trigger(&mut self, ev: Ev, when: u32, t1_fs: i128, t2_fs: i128) {
        let (t1, t2) = (crate::dho::clock::to_nominal_fs(t1_fs) / TICK_FS, crate::dho::clock::to_nominal_fs(t2_fs) / TICK_FS);
        let (r, s) = self.ev(ev);
        *s = (*s & !0x0700_0000) | (when & 7) << 24;
        let v = *s;
        self.w.push((r, v));
        self.w.push((r + 4, t1 as u32));
        self.w.push((r + 8, t2 as u32));
        self.w.push((r + 12, ((t1 >> 32) as u32 & 0xffff) | ((t2 >> 32) as u32 & 0xffff) << 16));
    }

    /// `mode(m)`: 0x205c[4:0] = trigger mux.
    fn mode(&mut self, m: u32) {
        self.sh.s205c = (self.sh.s205c & !0x1f) | (m & 0x1f);
        self.w.push((0x205c, self.sh.s205c));
    }

    /// `bypass(b)`: 0x205c bit12.
    fn bypass(&mut self, b: bool) {
        self.sh.s205c = (self.sh.s205c & !0x1000) | (b as u32) << 12;
        self.w.push((0x205c, self.sh.s205c));
    }

    /// `iir(0, src, 0x800)` (DC): 0x208c = (s & 0xfffc0000) | ((src-1)&3) << 2 | 0x800 << 4.
    fn iir(&mut self, src: u32) {
        self.iir_code(0, src);
    }

    /// `iir(code, src, 0x800)`: 0x208c = (s & 0xfffc0000) | code | ((src-1)&3) << 2 | 0x800 << 4.
    fn iir_code(&mut self, code: u32, src: u32) {
        self.sh.s208c = (self.sh.s208c & 0xfffc_0000) | code & 3 | (src.wrapping_sub(1) & 3) << 2 | 0x800 << 4;
        self.w.push((0x208c, self.sh.s208c));
    }

    /// `logic(idx, val, op)`: idx 0..15 -> 0x2014[2i+1:2i] (written), 16..20 -> 0x2018[9:0]
    /// (shadow only), then 0x2018[11:10] = op, written.
    fn logic_a(&mut self, idx: u32, val: u32, op: u32) {
        if idx <= 15 {
            let s = 2 * idx;
            self.sh.s2014 = (self.sh.s2014 & !(3 << s)) | val << s;
            self.w.push((0x2014, self.sh.s2014));
        } else if idx <= 20 {
            let s = 2 * (idx - 16);
            let lo = ((self.sh.s2018 & 0xffff) & !(3 << s)) | val << s;
            self.sh.s2018 = (self.sh.s2018 & 0xffff_fc00) | (lo & 0x3ff);
        }
        self.sh.s2018 = (self.sh.s2018 & !0xc00) | (op & 3) << 10;
        self.w.push((0x2018, self.sh.s2018));
    }

    /// Holdoff: type 4 (time), then the time in 6.4 ns ticks.
    fn holdoff(&mut self, t_fs: i128) {
        let t = crate::dho::clock::to_nominal_fs(t_fs) / TICK64_FS;
        self.sh.s2088 = (self.sh.s2088 & !0x70000) | 0x40000;
        self.w.push((0x2088, self.sh.s2088));
        self.w.push((0x2084, t as u32));
        self.sh.s2088 = (self.sh.s2088 & 0xffff_0000) | ((t >> 32) as u32 & 0xffff);
        self.w.push((0x2088, self.sh.s2088));
    }

    /// Level A (`b` false) / level B (`b` true) for CH1..CH4 (other sources: no write). `dir` true: hysteresis band below the level (code(v) | code(v-h) << 16), false: above
    /// (code(v+h) | code(v) << 16).
    fn level(&mut self, b: bool, ch: u32, level: f64, vt: &[Vert; 4], dir: bool) {
        if !(1..=4).contains(&ch) {
            return;
        }
        let k = vt[ch as usize - 1];
        let v = level + k.offset;
        let h = hyst(k.noise, k.scale);
        let c = |x: f64| code(x, k.scale, k.g);
        let val = if dir { c(v) | c(v - h) << 16 } else { c(v + h) | c(v) << 16 };
        let (ra, rb) = level_regs(ch);
        self.w.push((if b { rb } else { ra }, val));
    }

    /// Levels of one source: level A (`va`, `da`), then level B (`vb`, `db`). The stock system writes all four
    /// channels; like tpumodel.py only the sources are written.
    fn levels(&mut self, ch: u32, va: f64, vb: f64, da: bool, db: bool, vt: &[Vert; 4]) {
        self.level(false, ch, va, vt, da);
        self.level(true, ch, vb, vt, db);
    }
}

/// Edge trigger on channel `ch` (1..4, or 9..24 = LA line D0..D15), DC coupling: writes 1-17 of trigger.md 6.1 (19 for `Either`, which as in 2.2
/// writes source and pattern of A and B). `level`/`offset` in nV at the BNC,
/// `scale` nV/div, `g` the channel's digital gain (1.0 if unknown), `holdoff_fs` in femtoseconds (8 ns minimum).
pub fn edge(sh: &mut Shadows, ch: u32, slope: Slope, level: f64, offset: f64, scale: f64, g: f64, noise: f64, holdoff_fs: u64) -> Vec<Write> {
    edge_coupled(sh, ch, slope, Coupling::Dc, level, offset, scale, g, noise, holdoff_fs)
}

/// `edge` with a coupling (trigger.md 2.5, 6.1 "Other choices"): AC, LF and HF reject set the bypass (write 9, bit12)
/// and the filter 0x208c after the mode, and AC and LF reject leave the channel offset out of the level. 0x208c is written when the
/// code changes (back to DC too), so DC on a DC unit writes exactly the list of 6.1. LA lines stay DC.
#[allow(clippy::too_many_arguments)]
pub fn edge_coupled(sh: &mut Shadows, ch: u32, slope: Slope, coupling: Coupling, level: f64, offset: f64, scale: f64, g: f64, noise: f64, holdoff_fs: u64) -> Vec<Write> {
    let coupling = if ch > 8 { Coupling::Dc } else { coupling };
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    // trigger(A; 0,0,0), trigger(B; 0,0,0): time condition off, limits 0
    t.trigger(Ev::A, 0, 0, 0);
    t.trigger(Ev::B, 0, 0, 0);
    // bypass(src > 8 || coupling != DC): 0 for CH1..CH4 in DC
    t.bypass(ch > 8 || coupling != Coupling::Dc);
    if (9..=24).contains(&ch) {
        // LA line (src 9..24): the sources do not depend on the slope, only the patterns do
        t.source(Ev::A, a, 0, a, 0);
        t.source(Ev::B, a, 1, a, 1);
        match slope {
            Slope::Rising => t.pattern(Ev::A, 2, 1, 2, 1),
            Slope::Falling => t.pattern(Ev::A, 2, 0, 2, 0),
            Slope::Either => {
                t.pattern(Ev::A, 2, 0, 2, 0);
                t.pattern(Ev::B, 2, 1, 2, 1);
            }
        }
        t.mode(if slope == Slope::Either { 1 } else { 0 });
        // no level: the LA comparators decide (thresholds 0x12d4, la.md 5.1)
        t.holdoff(holdoff_fs as i128);
        return t.w;
    }
    match slope {
        Slope::Rising => {
            t.source(Ev::A, a, 0, b, 1);
            t.pattern(Ev::A, 2, 1, 2, 1);
        }
        Slope::Falling => {
            t.source(Ev::A, b, 1, a, 0);
            t.pattern(Ev::A, 2, 0, 2, 0);
        }
        Slope::Either => {
            // falling in event A, rising in event B, mux 1 = A or B
            t.source(Ev::A, b, 1, a, 0);
            t.source(Ev::B, a, 0, b, 1);
            t.pattern(Ev::A, 2, 0, 2, 0);
            t.pattern(Ev::B, 2, 1, 2, 1);
        }
    }
    t.mode(if slope == Slope::Either { 1 } else { 0 });
    // the filter on a new code, and on a new source while filtering (its channel field, 0x208c[3:2])
    let (code, on) = (t.sh.s208c & 3, (t.sh.s208c >> 2) & 3);
    if coupling.code() != code || (code != 0 && on != (ch.wrapping_sub(1) & 3)) {
        t.iir_code(coupling.code(), ch);
    }
    // AC and LF reject: the filter takes the mean out and its output sits at the middle code, so the level is around 0
    // whatever the offset (trigger.md 2.3 for AC; LF reject the same on a DHO924S, 2026-11-13: 1 Vpp at 1 MHz on 1 V DC
    // with -1 V offset fired at a level of 0.8..1.2 V with the offset in the level, which reads as the screen position)
    let offset = if matches!(coupling, Coupling::Ac | Coupling::LfReject) { 0.0 } else { offset };
    let vt = [Vert { scale, offset, g, noise }; 4];
    t.levels(ch, level, level, true, false, &vt);
    t.holdoff(holdoff_fs as i128);
    t.w
}

/// UART trigger (docs/fork/hwuart.md): the source's comparator (level A, the trigger level with its hysteresis) on
/// event R, the line inverted for idle-low signals (R pattern bit 23), the DC filter of the source, the receiver words
/// (`uarttrig::words`), then the mux on the UART event. Channels only.
pub fn uart(sh: &mut Shadows, ch: u32, u: &crate::dho::uarttrig::Uart, level: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let a = cmp_a(ch);
    t.bypass(false);
    t.levels(ch, level, level, true, false, vt);
    t.iir(ch);
    t.source(Ev::R, a, 0, a, 0);
    t.pattern(Ev::R, 0, 0, 2, u.invert as u32);
    t.w.extend(crate::dho::uarttrig::words(u));
    t.mode(crate::dho::uarttrig::MUX);
    t.w
}

/// A protocol line on event `ev` as the stock SPI/I2C drivers route it: the channel's level-A comparator on both
/// inputs, pattern (2, invert, 2, invert). Only bits 23:20 reach the line (bit 23 inverts it); bits 19:16 are copied
/// from the stock values (docs/fork/hwserial.md).
fn line(t: &mut Tpu, ev: Ev, ch: u32, invert: bool) {
    let a = cmp_a(ch);
    let i = invert as u32;
    t.source(ev, a, 0, a, 0);
    t.pattern(ev, 2, i, 2, i);
}

/// Level A of every distinct channel in `chs` at `level` with its hysteresis.
fn line_levels(t: &mut Tpu, chs: &[u32], level: f64, vt: &[Vert; 4]) {
    let mut done = Vec::new();
    for &c in chs {
        if !done.contains(&c) {
            t.levels(c, level, level, true, false, vt);
            done.push(c);
        }
    }
}

/// SPI trigger (docs/fork/hwserial.md): clock on event A (inverted for the falling edge), data on R, chip select on B
/// (inverted when active high), the levels of the three lines, the filter on the clock's channel, the receiver words,
/// then mux 12. Channels only.
pub fn spi(sh: &mut Shadows, s: &crate::dho::serialtrig::Spi, level: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    t.bypass(false);
    line_levels(&mut t, &[s.clk, s.data, s.cs], level, vt);
    t.iir(s.clk);
    line(&mut t, Ev::A, s.clk, s.clk_falling);
    line(&mut t, Ev::R, s.data, false);
    line(&mut t, Ev::B, s.cs, s.cs_high);
    t.w.extend(crate::dho::serialtrig::spi_words(s));
    t.mode(crate::dho::serialtrig::MUX_SPI);
    t.w
}

/// I2C trigger (docs/fork/hwserial.md): SCL on event A, SDA on event B, their levels, the filter on SCL's channel, the
/// receiver words, then mux 13. Channels only.
pub fn i2c(sh: &mut Shadows, i: &crate::dho::serialtrig::I2c, level: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    t.bypass(false);
    line_levels(&mut t, &[i.scl, i.sda], level, vt);
    t.iir(i.scl);
    line(&mut t, Ev::A, i.scl, false);
    line(&mut t, Ev::B, i.sda, false);
    t.w.extend(crate::dho::serialtrig::i2c_words(i));
    t.mode(crate::dho::serialtrig::MUX_I2C);
    t.w
}

/// Holdoff on its own (trigger.md 2.6): holdoff type 4 (time), 6.4 ns ticks, 48 bit.
pub fn holdoff(sh: &mut Shadows, t_fs: i128) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    t.holdoff(t_fs);
    t.w
}

/// Pulse width (type 1, trigger.md 10.2): bypass, levels, coupling filter,
/// `trigger(A; when, lower, upper)`, source and pattern A (positive fires on the falling edge at the end of the
/// pulse), mux 0. `Polarity::Any` writes no source/pattern at all (0x201c[23:0] keep the previous type's value).
pub fn pulse(sh: &mut Shadows, ch: u32, pol: Polarity, when: When, lower: i128, upper: i128, level: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    t.bypass(digital_or_ext(ch));
    t.levels(ch, level, level, true, false, vt);
    t.iir(ch);
    t.trigger(Ev::A, when.code(), lower, upper);
    match pol {
        Polarity::Pos => {
            t.source(Ev::A, b, 1, a, 0);
            t.pattern(Ev::A, 2, 0, 1, 0);
        }
        Polarity::Neg => {
            t.source(Ev::A, a, 0, b, 1);
            t.pattern(Ev::A, 2, 1, 1, 0);
        }
        Polarity::Any => {}
    }
    t.mode(0);
    t.w
}

/// Slope / transition time (type 2, trigger.md 10.3), then `trigger(A; when, lower, upper)`, which is written when the
/// condition or width changes (the sequence itself never writes the time condition). Level A = `hi`, level B = `lo`; rising: both dir 1, falling: both dir 0.
pub fn slope(sh: &mut Shadows, ch: u32, edge: Edge, when: When, lower: i128, upper: i128, hi: f64, lo: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    let rising = edge == Edge::Rising;
    t.bypass(digital_or_ext(ch));
    t.levels(ch, hi, lo, rising, rising, vt);
    t.iir(ch);
    if rising {
        t.source(Ev::A, a, 0, b, 0);
        t.pattern(Ev::A, 2, 1, 1, 0);
    } else {
        t.source(Ev::A, b, 1, a, 1);
        t.pattern(Ev::A, 2, 0, 1, 0);
    }
    t.mode(0);
    t.trigger(Ev::A, when.code(), lower, upper);
    t.w
}

/// Runt (type 7, trigger.md 10.4). `from_settype`: prepend the bypass-0 write of the type change (the sequence itself
/// does not touch bit12). Level A = `hi`, B = `lo`.
/// `When::None` becomes when 0 with both limits 0. `Polarity::Any` = positive in event A, negative in event B, mux 1.
pub fn runt(sh: &mut Shadows, ch: u32, pol: Polarity, when: When, lower: i128, upper: i128, hi: f64, lo: f64, vt: &[Vert; 4], from_settype: bool) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    if from_settype {
        t.bypass(false);
    }
    t.levels(ch, hi, lo, true, false, vt);
    t.iir(ch);
    let (w, lower, upper) = if when == When::None { (0, 0, 0) } else { (when.code(), lower, upper) };
    match pol {
        Polarity::Pos => {
            t.source(Ev::A, a, 0, b, 0);
            t.pattern(Ev::A, 3, 0, 6, 0);
            t.trigger(Ev::A, w, lower, upper);
            t.mode(0);
        }
        Polarity::Neg => {
            t.source(Ev::A, b, 1, a, 1);
            t.pattern(Ev::A, 3, 1, 6, 0);
            t.trigger(Ev::A, w, lower, upper);
            t.mode(0);
        }
        Polarity::Any => {
            // p2 = 1 instead of 6 here (trigger.md 10.16 question 4)
            t.source(Ev::A, a, 0, b, 0);
            t.pattern(Ev::A, 3, 0, 1, 0);
            t.trigger(Ev::A, w, lower, upper);
            t.source(Ev::B, b, 1, a, 1);
            t.pattern(Ev::B, 3, 1, 1, 0);
            t.trigger(Ev::B, w, lower, upper);
            t.mode(1);
        }
    }
    t.w
}

/// Window (type 8 "Over", trigger.md 10.5). `from_settype` as for `runt`.
/// Level A = `hi` (dir = event != exit), B = `lo` (dir = event == exit). Event `Time` uses when 4 (timeout) with
/// t1 = t2 = `time`, the others when 0 and no time. `Either` = rising in A, falling in B, mux 1.
pub fn window(sh: &mut Shadows, ch: u32, slope: Slope, event: WindowEvent, time: i128, hi: f64, lo: f64, vt: &[Vert; 4], from_settype: bool) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    let exit = event == WindowEvent::Exit;
    if from_settype {
        t.bypass(false);
    }
    t.levels(ch, hi, lo, !exit, exit, vt);
    t.iir(ch);
    t.source(Ev::A, a, 0, b, 0);
    t.source(Ev::B, b, 0, a, 0);
    let (w, tm) = if event == WindowEvent::Time { (WHEN_TIMEOUT, time) } else { (0, 0) };
    // (2,1,2,1) for rising enter, (2,0,2,0) for rising exit/time, the other way round for falling
    let enter = event == WindowEvent::Enter;
    let (up, down) = if enter { (1, 0) } else { (0, 1) };
    match slope {
        Slope::Rising => {
            t.trigger(Ev::A, w, tm, tm);
            t.pattern(Ev::A, 2, up, 2, up);
            t.mode(0);
        }
        Slope::Falling => {
            t.source(Ev::A, b, 0, a, 0);
            t.trigger(Ev::A, w, tm, tm);
            t.pattern(Ev::A, 2, down, 2, down);
            t.mode(0);
        }
        Slope::Either => {
            t.trigger(Ev::A, w, tm, tm);
            t.trigger(Ev::B, w, tm, tm);
            t.pattern(Ev::A, 2, up, 2, up);
            t.pattern(Ev::B, 2, down, 2, down);
            t.mode(1);
        }
    }
    t.w
}

/// Timeout (type 6, trigger.md 10.6): events A and B always both get
/// selection and `trigger(E; 4, time, 0)`; `Either` = A or B (mux 1).
pub fn timeout(sh: &mut Shadows, ch: u32, slope: Slope, time: i128, level: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    t.bypass(digital_or_ext(ch));
    t.levels(ch, level, level, true, false, vt);
    t.iir(ch);
    t.source(Ev::A, a, 0, b, 0);
    t.source(Ev::B, b, 0, a, 0);
    t.trigger(Ev::A, WHEN_TIMEOUT, time, 0);
    t.trigger(Ev::B, WHEN_TIMEOUT, time, 0);
    match slope {
        Slope::Rising => {
            t.pattern(Ev::A, 2, 0, 2, 0);
            t.mode(0);
        }
        Slope::Falling => {
            t.source(Ev::A, b, 0, a, 0);
            t.pattern(Ev::A, 2, 1, 2, 1);
            t.mode(0);
        }
        Slope::Either => {
            t.pattern(Ev::A, 2, 0, 2, 0);
            t.pattern(Ev::B, 2, 1, 2, 1);
            t.mode(1);
        }
    }
    t.w
}

/// Delay (type 10, trigger.md 10.7): time from the edge `ea` on `srca` to the
/// edge `eb` on `srcb`. Levels of both sources, bypass if either is digital/EXT, coupling filter for both,
/// `trigger(A; when, lower, upper)`, `source(A; B edge, A edge)`, pattern (1,0,1,0), mux 0.
pub fn delay(sh: &mut Shadows, srca: u32, ea: Edge, srcb: u32, eb: Edge, when: When, lower: i128, upper: i128, levela: f64, levelb: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (sa, sb) = ((ea == Edge::Falling) as u32, (eb == Edge::Falling) as u32);
    t.levels(srca, levela, levela, true, false, vt);
    t.levels(srcb, levelb, levelb, true, false, vt);
    t.bypass(digital_or_ext(srca) || digital_or_ext(srcb));
    t.iir(srca);
    t.iir(srcb);
    t.trigger(Ev::A, when.code(), lower, upper);
    let ca = if sa == 0 { cmp_a(srca) } else { cmp_b(srca) };
    let cb = if sb == 0 { cmp_a(srcb) } else { cmp_b(srcb) };
    t.source(Ev::A, cb, sb, ca, sa);
    t.pattern(Ev::A, 1, 0, 1, 0);
    t.mode(0);
    t.w
}

/// Common part of pattern and duration: levels of the channels that are not X (`level[ch-1]`), coupling filter for
/// CH1..CH4, 21 x `logic(i, sel, fun)` (sel: H -> 2, L -> 3, else 0; fun: OR -> 1, NOR -> 2, AND/NAND -> 0).
/// Returns the 21 entries and the index of the first rise/fall entry.
fn pattern_common(t: &mut Tpu, pats: &[Pat], logic: Logic, level: &[f64; 4], vt: &[Vert; 4]) -> ([Pat; 21], Option<usize>) {
    let mut arr = [Pat::X; 21];
    arr[..pats.len()].copy_from_slice(pats);
    for ch in 1..=4u32 {
        if arr[ch as usize - 1] != Pat::X {
            t.levels(ch, level[ch as usize - 1], level[ch as usize - 1], true, false, vt);
        }
    }
    for ch in 1..=4 {
        t.iir(ch);
    }
    let fun = match logic {
        Logic::Or => 1,
        Logic::Nor => 2,
        Logic::And | Logic::Nand => 0,
    };
    for (i, p) in arr.iter().enumerate() {
        let sel = match p {
            Pat::H => 2,
            Pat::L => 3,
            _ => 0,
        };
        t.logic_a(i as u32, sel, fun);
    }
    let e = arr.iter().position(|p| matches!(p, Pat::Rise | Pat::Fall));
    (arr, e)
}

/// Pattern (type 4): type change (bypass 1), then the sequence of trigger.md 10.8.
/// `pats`: up to 21 entries, CH1..CH4, D0..D15, EXT (missing = X). With an edge entry e: `source(A; c, fall, c, 2)`,
/// c = `cmp_a(e + 1)` (only CH1..CH4 give their own code), pattern (2, rise, 0, 0); without: pattern (0,0,0,0) and
/// 0x201c[15:0] keep the previous value. Then `trigger(A; 0,0,0)`, mux 0.
pub fn pattern(sh: &mut Shadows, pats: &[Pat], logic: Logic, level: &[f64; 4], vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    t.bypass(true);
    let (arr, e) = pattern_common(&mut t, pats, logic, level, vt);
    match e {
        Some(e) => {
            let c = cmp_a(e as u32 + 1);
            t.source(Ev::A, c, (arr[e] != Pat::Rise) as u32, c, 2);
            t.pattern(Ev::A, 2, (arr[e] == Pat::Rise) as u32, 0, 0);
        }
        None => t.pattern(Ev::A, 0, 0, 0, 0),
    }
    t.trigger(Ev::A, 0, 0, 0);
    t.mode(0);
    t.w
}

/// Duration (type 5): type change (bypass 0), then the sequence of trigger.md 10.9.
/// As `pattern`, but with an edge the comparator is `cmp_a(e)` (index, not index + 1, the same as the stock system: a
/// CH1 edge gives code 51 and a CH2 edge CH1's code), pattern (0, 0, 2, rise), and `trigger(A; when, lower, upper)`.
pub fn duration(sh: &mut Shadows, pats: &[Pat], logic: Logic, when: When, lower: i128, upper: i128, level: &[f64; 4], vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    t.bypass(false);
    let (arr, e) = pattern_common(&mut t, pats, logic, level, vt);
    match e {
        Some(e) => {
            let c = cmp_a(e as u32);
            t.source(Ev::A, c, (arr[e] != Pat::Rise) as u32, c, 2);
            t.pattern(Ev::A, 0, 0, 2, (arr[e] == Pat::Rise) as u32);
        }
        None => t.pattern(Ev::A, 0, 0, 0, 0),
    }
    t.trigger(Ev::A, when.code(), lower, upper);
    t.mode(0);
    t.w
}

/// Setup/hold (type 11, trigger.md 10.10). C = clock edge `edge` on `clk`,
/// Din/Dout = data edge into/out of the valid level `dtype`. Setup: A = (C, Din), pattern (2, clk rising, 3, L),
/// `trigger(A; 1, 0, setup)`; hold: A = (Dout, C), pattern (2, L, 6, 0), `trigger(A; 1, 0, hold)`; both: setup in A,
/// hold in B, mux 1.
pub fn setup_hold(sh: &mut Shadows, clk: u32, data: u32, edge: Edge, dtype: DataLevel, event: ShEvent, setup: i128, hold: i128, levelc: f64, leveld: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let s = (edge == Edge::Falling) as u32;
    let d = (dtype == DataLevel::L) as u32;
    t.bypass(digital_or_ext(clk) || digital_or_ext(data));
    t.levels(clk, levelc, levelc, true, false, vt);
    t.levels(data, leveld, leveld, true, false, vt);
    t.iir(clk);
    t.iir(data);
    let cc = if s == 0 { cmp_a(clk) } else { cmp_b(clk) };
    let din = if d == 0 { cmp_a(data) } else { cmp_b(data) };
    let dout = if d == 1 { cmp_a(data) } else { cmp_b(data) };
    if event != ShEvent::Hold {
        t.source(Ev::A, cc, s, din, d);
        t.pattern(Ev::A, 2, 1 - s, 3, d);
        t.trigger(Ev::A, 1, 0, setup);
    }
    if event == ShEvent::Hold {
        t.source(Ev::A, dout, 1 - d, cc, s);
        t.pattern(Ev::A, 2, d, 6, 0);
        t.trigger(Ev::A, 1, 0, hold);
    }
    if event == ShEvent::Both {
        t.source(Ev::B, dout, 1 - d, cc, s);
        t.pattern(Ev::B, 2, d, 6, 0);
        t.trigger(Ev::B, 1, 0, hold);
    }
    t.mode(if event == ShEvent::Both { 1 } else { 0 });
    t.w
}

/// Nth edge (type 12, trigger.md 10.11): event A = the edge, `trigger(A; 0,0,0)`, `trigger(B; 0,0,0)`,
/// `abr(1,1,1,0,1)` (0x205c bits 6, 7, 8, 10; never cleared by the other types),
/// `abr_count(count)` 0x2070, `r_timeout(idle)` 0x2068/0x206c (6.4 ns ticks), mux 3 (ABR).
pub fn nth(sh: &mut Shadows, ch: u32, edge: Edge, count: u32, idle: i128, level: f64, vt: &[Vert; 4]) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let (a, b) = (cmp_a(ch), cmp_b(ch));
    let s = (edge == Edge::Falling) as u32;
    t.bypass(digital_or_ext(ch));
    t.levels(ch, level, level, true, false, vt);
    t.iir(ch);
    t.source(Ev::A, a, s, b, s);
    if s == 1 {
        t.source(Ev::A, b, s, a, s);
        t.pattern(Ev::A, 2, 0, 2, 0);
    } else {
        t.pattern(Ev::A, 2, 1, 2, 1);
    }
    t.trigger(Ev::A, 0, 0, 0);
    t.trigger(Ev::B, 0, 0, 0);
    // abr(1,1,1,0,1)
    t.sh.s205c = (t.sh.s205c & !0x7c0) | 0b10111 << 6;
    t.w.push((0x205c, t.sh.s205c));
    // abr_count, r_timeout
    t.w.push((0x2070, count));
    let idle = crate::dho::clock::to_nominal_fs(idle) / TICK64_FS;
    t.w.push((0x2068, idle as u32));
    t.w.push((0x206c, (idle >> 32) as u32));
    t.mode(3);
    t.w
}

/// Video (type 3), NTSC/PAL (trigger.md 10.12): levels, coupling filter, event R =
/// (cmpA, 0, cmpA, 0) with pattern (0,0,2,0), only the pattern of event A (`pol`: which value is positive sync is not
/// known; 0x201c[15:0] keep the previous value), mux 14, `video2` (sync width, line period / `clk`),
/// `video3`, `video1(sync != any, line)`. Odd/even field: `line` is replaced by the field's line
/// (NTSC 4/267, PAL 1/313). Bit12 is not written.
pub fn video(sh: &mut Shadows, ch: u32, std: VideoStd, pol: bool, sync: VideoSync, line: u32, level: f64, vt: &[Vert; 4], clk: VideoClk) -> Vec<Write> {
    let mut t = Tpu { sh, w: Vec::new() };
    let a = cmp_a(ch);
    let line = match (std, sync) {
        (VideoStd::Ntsc, VideoSync::OddField) => 4,
        (VideoStd::Ntsc, VideoSync::EvenField) => 267,
        (VideoStd::Pal, VideoSync::OddField) => 1,
        (VideoStd::Pal, VideoSync::EvenField) => 313,
        _ => line,
    };
    t.levels(ch, level, level, true, false, vt);
    t.iir(ch);
    t.source(Ev::R, a, 0, a, 0);
    t.pattern(Ev::R, 0, 0, 2, 0);
    t.pattern(Ev::A, 2, pol as u32, 2, pol as u32);
    t.mode(14);
    // (sync pulse width, line period) in fs, video3 (start?, lines per frame, progressive, interlaced)
    let (t1, t2, c3a, lines) = match std {
        VideoStd::Ntsc => (4_700_000_000i128, 58_790_000_000i128, 5, 525),
        VideoStd::Pal => (4_700_000_000, 59_300_000_000, 2, 625),
    };
    // Stock: 6.4 ns FPGA-clock ticks (scale with the ADC clock); Fast: 4 ns, probably the PS's 250 MHz FCLK3
    // (fpga-image.md), independent of the ADC clock [A]
    let (t1, t2, div) = match clk {
        VideoClk::Fast => (t1, t2, 4_000_000),
        VideoClk::Stock => (crate::dho::clock::to_nominal_fs(t1), crate::dho::clock::to_nominal_fs(t2), 6_400_000),
    };
    // video2, video3, video1
    t.w.push((0x21a8, ((t1 / div) as u32 & 0xffff) | ((t2 / div) as u32 & 0xffff) << 16));
    t.w.push((0x21ac, (c3a & 0x7ff) | (lines & 0x7ff) << 11 | 1 << 23));
    t.w.push((0x21a4, (sync != VideoSync::AnyLine) as u32 | (line & 0x7ff) << 2));
    t.w
}

/// Force trigger: 0x205c bit5 pulse.
pub fn force(sh: &Shadows) -> Vec<Write> {
    vec![(0x205c, sh.s205c | 0x20), (0x205c, sh.s205c)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn spi_and_i2c_route_their_lines() {
        use crate::dho::serialtrig::{Dir, I2c, I2cCond, Spi};
        use crate::dho::uarttrig::Relation;
        let vt = [Vert { scale: 1e9, offset: 0.0, g: 1.0, noise: 0.0 }; 4];
        let last = |w: &[Write], r: u32| w.iter().filter(|x| x.0 == r).last().map(|x| x.1);
        let mut sh = Shadows::APP;
        let s = Spi { clk: 2, data: 1, cs: 3, clk_falling: true, cs_high: false, timeout: None, width: 8, rel: Relation::Eq, min: 0x5a, max: 0x5a, mask: 0xff };
        let w = spi(&mut sh, &s, 1.5e9, &vt);
        // the stock patterns: A = CH2 level A (code 2) (2, 1, 2, 1) for the falling clock, R = CH1 (code 0) (2, 0, 2, 0),
        // B = CH3 (code 4) (2, 0, 2, 0) for an active-low CS
        assert_eq!(last(&w, 0x201c).unwrap() & 0x00ff_ffff, 2 | 2 << 8 | 2 << 16 | 1 << 19 | 2 << 20 | 1 << 23);
        assert_eq!(last(&w, 0x204c).unwrap() & 0x00ff_ffff, 2 << 16 | 2 << 20);
        assert_eq!(last(&w, 0x2034).unwrap() & 0x00ff_ffff, 4 | 4 << 8 | 2 << 16 | 2 << 20);
        assert!([0x2004, 0x200c, 0x20b8].iter().all(|r| w.iter().any(|x| x.0 == *r)), "levels of CH1..CH3");
        assert_eq!(*w.last().unwrap(), (0x205c, 12));
        assert_eq!(last(&w, 0x2178), Some(7 << 4));
        let i = I2c { scl: 1, sda: 2, cond: I2cCond::Start, dir: Dir::Either, ten_bit: false, addr_rel: Relation::Eq, addr: (0, 0, 0), data_rel: Relation::Eq, bytes: 1, data: (0, 0, 0) };
        let w = i2c(&mut sh, &i, 1.5e9, &vt);
        assert_eq!(last(&w, 0x201c).unwrap() & 0x00ff_ffff, 2 << 16 | 2 << 20);
        assert_eq!(last(&w, 0x2034).unwrap() & 0x00ff_ffff, 2 | 2 << 8 | 2 << 16 | 2 << 20);
        assert_eq!(*w.last().unwrap(), (0x205c, 13));
        assert!(w.iter().all(|x| !(0x2200..=0x2218).contains(&x.0)));
    }

    #[test]
    fn uart_routes_the_line_and_selects_the_receiver() {
        use crate::dho::uarttrig::{Cond, Parity, Stop, Uart};
        let mut sh = Shadows::APP;
        let vt = [Vert { scale: 1e9, offset: 0.0, g: 1.0, noise: 0.0 }; 4];
        let u = Uart { baud: 9600.0, bits: 8, parity: Parity::None, stop: Stop::One, cond: Cond::Start, invert: true };
        let w = uart(&mut sh, 2, &u, 1.5e9, &vt);
        // event R: comparator level A of CH2 (code 2) on both inputs, pattern (0, 0, 2) with the inversion bit
        let r: Vec<u32> = w.iter().filter(|x| x.0 == 0x204c).map(|x| x.1).collect();
        assert_eq!(*r.last().unwrap(), 2 | 2 << 8 | 2 << 20 | 1 << 23);
        assert!(w.contains(&(0x2100, 16276)) && w.contains(&(0x2104, 7)) && w.contains(&(0x221c, 0)));
        assert_eq!(*w.last().unwrap(), (0x205c, 4));
        assert!(w.iter().any(|x| x.0 == 0x200c), "CH2 level A");
        assert_eq!(w.iter().find(|x| x.0 == 0x208c).unwrap().1 >> 2 & 3, 1);
        // the counter's write registers stay untouched
        assert!(w.iter().all(|x| !(0x2200..=0x2218).contains(&x.0)));
        // not inverted: bit 23 clear
        let w = uart(&mut sh, 2, &Uart { invert: false, ..u }, 1.5e9, &vt);
        assert_eq!(w.iter().filter(|x| x.0 == 0x204c).last().unwrap().1 & 1 << 23, 0);
    }

    #[test]
    fn level_codes_from_the_doc_example() {
        // CH1 500 mV/div, offset 0, g 1, level +1 V, noise reject off
        assert_eq!(levels(1e9, 0.3 * 5e8, 5e8, 1.0), (0xb1ce_ba98, 0xba98_c362));
        assert_eq!(levels(0.0, 0.3 * 5e8, 5e8, 1.0), (0x7736_8000, 0x8000_88ca));
        assert_eq!(code(-1e12, 5e8, 1.0), 1);
        assert_eq!(code(1e12, 5e8, 1.0), 0xfffe);
        assert_eq!(hyst(0.3, 5e8), 1.5e8);
    }

    #[test]
    fn ch1_rising_ends_in_the_stock_state() {
        let mut sh = Shadows::APP;
        let w = edge(&mut sh, 1, Slope::Rising, 0.0, 0.0, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(w.len(), 17);
        assert_eq!(sh, Shadows::APP);
        assert_eq!(w[15], (0x2084, 1));
        // falling on CH1: 0x00220041
        let mut sh = Shadows::APP;
        edge(&mut sh, 1, Slope::Falling, 0.0, 0.0, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(sh.s201c, 0x0022_0041);
    }

    #[test]
    fn edge_coupling_sets_bypass_filter_and_ac_level() {
        let last = |w: &[Write], r: u32| w.iter().rev().find(|x| x.0 == r).map(|x| x.1);
        // CH1 AC: bypass, 0x208c = 3 | (1-1) << 2 | 0x8000, the level without the offset (+1 V offset, level 0)
        let mut sh = Shadows::APP;
        let w = edge_coupled(&mut sh, 1, Slope::Rising, Coupling::Ac, 0.0, 1e9, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x205c).unwrap() & 0x1000, 0x1000);
        assert_eq!(last(&w, 0x208c), Some(0x8003));
        assert_eq!(last(&w, 0x2008).unwrap() & 0xffff, 0x8000 + 2250, "level B upper: code(0 + hyst)");
        // the same again writes no filter; another source does; HF reject keeps the offset
        let w = edge_coupled(&mut sh, 1, Slope::Rising, Coupling::Ac, 0.0, 1e9, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x208c), None);
        let w = edge_coupled(&mut sh, 3, Slope::Rising, Coupling::Ac, 0.0, 1e9, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x208c), Some(0x800b));
        let w = edge_coupled(&mut sh, 1, Slope::Rising, Coupling::HfReject, 0.0, 1e9, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x208c), Some(0x8001));
        assert_eq!(last(&w, 0x2008).unwrap() & 0xffff, 0x8000 + 2 * 7500 + 2250);
        let w = edge_coupled(&mut sh, 1, Slope::Rising, Coupling::LfReject, 0.0, 1e9, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x208c), Some(0x8002));
        assert_eq!(last(&w, 0x2008).unwrap() & 0xffff, 0x8000 + 2250, "LF reject: no offset either");
        // back to DC: no bypass, filter code 0
        let w = edge_coupled(&mut sh, 1, Slope::Rising, Coupling::Dc, 0.0, 0.0, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x205c).unwrap() & 0x1000, 0);
        assert_eq!(sh.s208c & 3, 0);
        // an LA line stays DC
        let mut sh = Shadows::APP;
        let w = edge_coupled(&mut sh, 9, Slope::Rising, Coupling::Ac, 0.0, 0.0, 5e8, 1.0, noise_div(false), 8_000_000);
        assert_eq!(last(&w, 0x208c), None);
    }

    #[test]
    fn doc_examples_of_section_10() {
        let vt = [Vert { scale: 5e8, offset: 0.0, g: 1.0, noise: 0.3 }; 4];
        let last = |w: &[Write], r: u32| w.iter().rev().find(|x| x.0 == r).unwrap().1;
        // 10.2 pulse CH1 positive > 100 ns
        let mut sh = Shadows::APP;
        let w = pulse(&mut sh, 1, Polarity::Pos, When::Gt, 100_000_000, 0, 0.0, &vt);
        assert_eq!((sh.s201c, last(&w, 0x2020)), (0x0012_0041, 62));
        // 10.3 slope CH1 rising < 20 ns, high 1 V, low 0.2 V
        let mut sh = Shadows::APP;
        let w = slope(&mut sh, 1, Edge::Rising, When::Lt, 0, 20_000_000, 1e9, 2e8, &vt);
        assert_eq!((last(&w, 0x2004), last(&w, 0x2008), sh.s201c, last(&w, 0x2024)), (0xb1ce_ba98, 0x82ee_8bb8, 0x011a_0100, 12));
        // 10.5 window any, time 1 us
        let mut sh = Shadows::APP;
        let w = window(&mut sh, 1, Slope::Either, WindowEvent::Time, 1_000_000_000, 1e9, -1e9, &vt, true);
        assert_eq!((sh.s201c, sh.s2034, last(&w, 0x2020), last(&w, 0x203c), sh.s205c & 0x1f), (0x0422_0100, 0x04aa_0001, 625, 625, 1));
        // 10.7 delay CH1 rising -> CH2 falling > 50 ns
        let mut sh = Shadows::APP;
        delay(&mut sh, 1, Edge::Rising, 2, Edge::Falling, When::Gt, 50_000_000, 0, 0.0, 0.0, &vt);
        assert_eq!((sh.s201c, sh.s208c), (0x0011_0043, 0x8004));
        // 10.8 pattern CH1 H, CH2 L, CH3 rise, CH4 X
        let mut sh = Shadows::APP;
        pattern(&mut sh, &[Pat::H, Pat::L, Pat::Rise, Pat::X], Logic::And, &[0.0; 4], &vt);
        assert_eq!((sh.s2014, sh.s2018, sh.s201c, sh.s205c), (0xe, 0, 0x000a_8404, 0x1000));
        // 10.10 setup/hold both, 10.11 Nth edge, 10.12 video PAL line 100 (4 ns clock)
        let mut sh = Shadows::APP;
        setup_hold(&mut sh, 1, 2, Edge::Rising, DataLevel::H, ShEvent::Both, 5_000_000, 2_000_000, 0.0, 0.0, &vt);
        assert_eq!((sh.s201c, sh.s2034, sh.s205c), (0x013a_0200, 0x0162_0043, 1));
        let mut sh = Shadows::APP;
        let w = nth(&mut sh, 1, Edge::Rising, 3, 1_000_000_000, 0.0, &vt);
        assert_eq!((sh.s201c, last(&w, 0x2068), sh.s205c), (0x00aa_0100, 156, 0x5c3));
        let mut sh = Shadows::APP;
        let w = video(&mut sh, 1, VideoStd::Pal, false, VideoSync::Line, 100, 0.0, &vt, VideoClk::Fast);
        assert_eq!(&w[w.len() - 3..], &[(0x21a8, 0x39e9_0497), (0x21ac, 0x0093_8802), (0x21a4, 0x191)]);
    }

    /// Runs one fixture step (`type key=value ...`, see tools/tpu_fixture.py) and returns its writes.
    fn run_step(sh: &mut Shadows, step: &str) -> Vec<Write> {
        let mut it = step.split_whitespace();
        let typ = it.next().unwrap();
        let kv: HashMap<&str, &str> = it.map(|x| x.split_once('=').unwrap()).collect();
        let s = |k: &str| *kv.get(k).unwrap_or_else(|| panic!("{}: {} missing", step, k));
        let f = |k: &str| s(k).parse::<f64>().unwrap();
        let i = |k: &str| s(k).parse::<i128>().unwrap();
        let u = |k: &str| s(k).parse::<u32>().unwrap();
        let slope_ = |k: &str| match s(k) {
            "rising" => Slope::Rising,
            "falling" => Slope::Falling,
            "any" => Slope::Either,
            x => panic!("slope {}", x),
        };
        let edge_ = |k: &str| match s(k) {
            "rising" => Edge::Rising,
            "falling" => Edge::Falling,
            x => panic!("edge {}", x),
        };
        let pol = || match s("pol") {
            "pos" => Polarity::Pos,
            "neg" => Polarity::Neg,
            "any" => Polarity::Any,
            x => panic!("pol {}", x),
        };
        let when = || match s("when") {
            "gt" => When::Gt,
            "lt" => When::Lt,
            "in" => When::In,
            "out" => When::Out,
            "none" => When::None,
            x => panic!("when {}", x),
        };
        let pats = || -> Vec<Pat> {
            s("pats")
                .split(',')
                .map(|p| match p {
                    "H" => Pat::H,
                    "L" => Pat::L,
                    "X" => Pat::X,
                    "R" => Pat::Rise,
                    "F" => Pat::Fall,
                    x => panic!("pattern {}", x),
                })
                .collect()
        };
        let logic = || match s("logic") {
            "and" => Logic::And,
            "or" => Logic::Or,
            "nand" => Logic::Nand,
            "nor" => Logic::Nor,
            x => panic!("logic {}", x),
        };
        if typ == "holdoff" {
            return holdoff(sh, i("t_fs"));
        }
        let vt = [Vert { scale: f("scale"), offset: f("offset"), g: f("g"), noise: f("noise") }; 4];
        match typ {
            "edge" => {
                let nr = match s("noise") {
                    "0.3" => false,
                    "0.7" => true,
                    x => panic!("edge noise {}", x),
                };
                edge(sh, u("ch"), slope_("slope"), f("level"), f("offset"), f("scale"), f("g"), noise_div(nr), i("holdoff_fs") as u64)
            }
            "pulse" => pulse(sh, u("ch"), pol(), when(), i("lower"), i("upper"), f("level"), &vt),
            "slope" => slope(sh, u("ch"), edge_("slope"), when(), i("lower"), i("upper"), f("hi"), f("lo"), &vt),
            "runt" => runt(sh, u("ch"), pol(), when(), i("lower"), i("upper"), f("hi"), f("lo"), &vt, s("from_settype") == "1"),
            "window" => {
                let ev = match s("event") {
                    "enter" => WindowEvent::Enter,
                    "exit" => WindowEvent::Exit,
                    "time" => WindowEvent::Time,
                    x => panic!("event {}", x),
                };
                window(sh, u("ch"), slope_("slope"), ev, i("time"), f("hi"), f("lo"), &vt, s("from_settype") == "1")
            }
            "timeout" => timeout(sh, u("ch"), slope_("slope"), i("time"), f("level"), &vt),
            "delay" => delay(sh, u("srca"), edge_("slopea"), u("srcb"), edge_("slopeb"), when(), i("lower"), i("upper"), f("levela"), f("levelb"), &vt),
            "pattern" => pattern(sh, &pats(), logic(), &[f("level"); 4], &vt),
            "duration" => duration(sh, &pats(), logic(), when(), i("lower"), i("upper"), &[f("level"); 4], &vt),
            "setuphold" => {
                let dt = if s("dtype") == "L" { DataLevel::L } else { DataLevel::H };
                let ev = match s("event") {
                    "setup" => ShEvent::Setup,
                    "hold" => ShEvent::Hold,
                    "both" => ShEvent::Both,
                    x => panic!("event {}", x),
                };
                setup_hold(sh, u("clk"), u("data"), edge_("slope"), dt, ev, i("setup"), i("hold"), f("levelc"), f("leveld"), &vt)
            }
            "nth" => nth(sh, u("ch"), edge_("slope"), u("count"), i("idle"), f("level"), &vt),
            "video" => {
                let std = if s("std") == "ntsc" { VideoStd::Ntsc } else { VideoStd::Pal };
                let sync = match s("sync") {
                    "any" => VideoSync::AnyLine,
                    "line" => VideoSync::Line,
                    "odd" => VideoSync::OddField,
                    "even" => VideoSync::EvenField,
                    x => panic!("sync {}", x),
                };
                let clk = if s("vclk") == "stock" { VideoClk::Stock } else { VideoClk::Fast };
                video(sh, u("ch"), std, s("polarity") == "1", sync, u("line"), f("level"), &vt, clk)
            }
            x => panic!("type {}", x),
        }
    }

    /// Every write (register, value, order) and the end shadows equal tools/tpumodel.py for the cases of
    /// tools/tpu_fixture.py (testdata/tpu_expected.txt).
    #[test]
    fn matches_tpumodel() {
        let want = include_str!("../../testdata/tpu_expected.txt");
        let mut n = 0;
        for case in want.split("== ").filter(|c| !c.is_empty()) {
            let (head, body) = case.split_once('\n').unwrap();
            let mut sh = Shadows::APP;
            let mut got = Vec::new();
            for step in head.split(" ; ") {
                got.extend(run_step(&mut sh, step).iter().map(|(r, v)| format!("{:#06x} {:#010x}", r, v)));
            }
            got.push(format!(
                "sh 0x2014={:#010x} 0x2018={:#010x} 0x201c={:#010x} 0x2034={:#010x} 0x204c={:#010x} 0x205c={:#010x} 0x2088={:#010x} 0x208c={:#010x}",
                sh.s2014, sh.s2018, sh.s201c, sh.s2034, sh.s204c, sh.s205c, sh.s2088, sh.s208c
            ));
            let want: Vec<&str> = body.lines().collect();
            for (k, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(g, w, "case `{}`, line {}", head, k + 1);
            }
            assert_eq!(got.len(), want.len(), "case `{}`", head);
            n += 1;
        }
        assert_eq!(n, 80);
    }
}

#[cfg(test)]
mod la_source_tests {
    use super::*;

    /// LA line D0 (src 9, code 32): 0x201c / 0x2034 low halves, patterns by slope.
    #[test]
    fn edge_on_an_la_line() {
        let w = edge(&mut Shadows::APP.clone(), 9, Slope::Rising, 0.0, 0.0, 1e9, 1.0, noise_div(false), 8_000_000);
        let last = |r: u32| w.iter().rev().find(|x| x.0 == r).map(|x| x.1).unwrap();
        // source A (c,0,c,0), source B (c,1,c,1) = the 0x6060 of trigger.md; pattern A rising 0xaa; bypass on; mux 0
        assert_eq!(last(0x201c) & 0x00ff_ffff, 0x00aa_2020);
        assert_eq!(last(0x2034) & 0xffff, 0x6060);
        assert_eq!((last(0x205c) >> 12 & 1, last(0x205c) & 0x1f), (1, 0));
        // no level register of any channel
        assert!(!w.iter().any(|x| matches!(x.0, 0x2004 | 0x2008 | 0x200c | 0x2010 | 0x20b8..=0x20c4)));

        let w = edge(&mut Shadows::APP.clone(), 24, Slope::Falling, 0.0, 0.0, 1e9, 1.0, noise_div(false), 8_000_000);
        let last = |r: u32| w.iter().rev().find(|x| x.0 == r).map(|x| x.1).unwrap();
        // D15: code 47, the same sources, pattern 0x22
        assert_eq!(last(0x201c) & 0x00ff_ffff, 0x0022_2f2f);

        let w = edge(&mut Shadows::APP.clone(), 9, Slope::Either, 0.0, 0.0, 1e9, 1.0, noise_div(false), 8_000_000);
        let last = |r: u32| w.iter().rev().find(|x| x.0 == r).map(|x| x.1).unwrap();
        assert_eq!((last(0x201c) >> 16 & 0xff, last(0x2034) >> 16 & 0xff, last(0x205c) & 0x1f), (0x22, 0xaa, 1));
    }
}

#[cfg(test)]
mod la_pattern_tests {
    use super::*;

    /// Slots of trigger.md 10.8: 0..3 CH1..CH4, 4..19 D0..D15. D0 = slot 4 in 0x2014, D15 = slot 19 in 0x2018.
    #[test]
    fn d_lines_in_the_pattern_slots() {
        let vt = [Vert { scale: 1e9, offset: 0.0, g: 1.0, noise: noise_div(false) }; 4];
        let mut p = [Pat::X; 20];
        (p[4], p[19]) = (Pat::H, Pat::L);
        let mut sh = Shadows::APP;
        let w = pattern(&mut sh, &p, Logic::And, &[0.0; 4], &vt);
        assert_eq!(sh.s2014, 2 << 8);
        assert_eq!(sh.s2018 & 0x3ff, 3 << 6);
        assert_eq!(w.iter().rev().find(|x| x.0 == 0x2018).unwrap().1 & 0x3ff, 3 << 6);
    }
}
