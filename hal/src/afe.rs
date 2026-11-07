// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! AFE (RT1642, one per channel) settings: V/div, offset, coupling, bandwidth limit.
//! A port of `tools/afesim.py`, see docs/protocols/afe-settings.md. The setters only change some
//! bits of a register and keep the rest, so the per-channel shadows must persist across commands.
//!
//! The functions produce `Op`s; the AFE writes go over the AFE SPI bus, the gear
//! over `/dev/dac_gear_change`, the ADC words to FPGA 0x1034.

/// One hardware operation of a setter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// AFE register write: channel 0..3, register, 16-bit value
    Afe { ch: usize, reg: u8, val: u16 },
    /// `ioctl(/dev/dac_gear_change, 0x7801, ch | gear << 4)`
    Gear { ch: usize, gear: u8 },
    /// FPGA 0x1034 (ADC SPI word)
    Adc(u32),
}

/// Unit calibration data (`/rigol/data`): cal_lsb.hex, cal_afe_zero.hex, cal_afe_bandwidth.hex, cal_vertical.hex.
pub struct Cal {
    lsb: [f64; 16],
    zero: [i32; 80],
    bw: [i32; 80],
    vertical: Vec<u8>,
    keys: Vec<i64>,
}

/// One cal_vertical record (56 bytes): scale key (nV/div), 8 gain codes (input gain, VGA, ATT, SGA, SGAR, MUX, MUXR, -),
/// FPGA digital gain, DAC zero code.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rec {
    pub key: i64,
    pub g: [i32; 8],
    pub gain: f64,
    pub zero: i32,
    pub last: i32,
}

const HDR: usize = 28;

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

impl Cal {
    pub fn from_bytes(lsb: &[u8], zero: &[u8], bw: &[u8], vertical: Vec<u8>) -> Cal {
        let mut c = Cal { lsb: [0.0; 16], zero: [0; 80], bw: [0; 80], vertical, keys: Vec::new() };
        for i in 0..16 {
            c.lsb[i] = f64::from_le_bytes(lsb[HDR + 8 * i..HDR + 8 * i + 8].try_into().unwrap());
        }
        for i in 0..80 {
            c.zero[i] = i32_at(zero, HDR + 4 * i);
            c.bw[i] = i32_at(bw, HDR + 4 * i);
        }
        // fixed values that replace these entries of the file
        for ch in 0..4 {
            c.bw[ch * 20 + 4] = 110;
            c.bw[ch * 20 + 10] = 97;
            c.bw[ch * 20 + 16] = 97;
        }
        c.keys = (0..801).map(|i| c.rec(0, 0, i).key).collect();
        c
    }

    /// Loads the four files, each from the first valid copy (both CRCs checked): OOS's own self-cal result
    /// (`calfile::OOS_CAL_DIR`), then `dir` (the factory /rigol/data, read only), then `dir/default`.
    pub fn load(dir: &str) -> std::io::Result<Cal> {
        Ok(Self::load_from(dir)?.0)
    }

    /// `load` and where each file came from.
    pub fn load_from(dir: &str) -> std::io::Result<(Cal, Vec<(String, String)>)> {
        let def = format!("{}/default", dir.trim_end_matches('/'));
        let dirs = [crate::calfile::OOS_CAL_DIR, dir, def.as_str()];
        let mut from = Vec::new();
        let mut rd = |n: &str| {
            let (b, d) = crate::calfile::load_first(n, &dirs).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("{}: no valid copy in {:?}", n, dirs)))?;
            from.push((n.to_string(), d));
            Ok::<_, std::io::Error>(b)
        };
        let (lsb, zero, bw, vert) = (rd("cal_lsb.hex")?, rd("cal_afe_zero.hex")?, rd("cal_afe_bandwidth.hex")?, rd("cal_vertical.hex")?);
        // a good CRC says nothing about the length: `from_bytes` needs these sizes
        if lsb.len() < HDR + 128 || zero.len() < HDR + 320 || bw.len() < HDR + 320 || vert.len() < HDR + 4 * 801 * 56 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "calibration file too short"));
        }
        Ok((Cal::from_bytes(&lsb, &zero, &bw, vert), from))
    }

    /// Record `idx` of channel `ch` in block group `var`. Older files have 12 blocks instead of 16: their last group
    /// stands in for the one they lack [A].
    pub fn rec(&self, var: usize, ch: usize, idx: usize) -> Rec {
        let groups = ((self.vertical.len() - HDR) / (4 * 801 * 56)).max(1);
        let o = HDR + ((var.min(groups - 1) * 4 + ch) * 801 + idx) * 56;
        let b = &self.vertical[o..o + 56];
        let mut g = [0; 8];
        for (k, x) in g.iter_mut().enumerate() {
            *x = i32_at(b, 8 + 4 * k);
        }
        Rec {
            key: i64::from_le_bytes(b[0..8].try_into().unwrap()),
            g,
            gain: f64::from_le_bytes(b[40..48].try_into().unwrap()),
            zero: i32_at(b, 48),
            last: i32_at(b, 52),
        }
    }

    /// V per offset-DAC code of channel `ch` on input-gain path `ig` (cal_lsb, factory only).
    pub fn lsb(&self, ch: usize, ig: usize) -> f64 {
        self.lsb[ch * 4 + ig.min(3)]
    }

    /// Scale key (nV/div) of row `i` of every block.
    pub fn key(&self, i: usize) -> i64 {
        self.keys[i]
    }

    /// The 801 records of block (`var`, `ch`) (self-cal input and comparison).
    pub fn block(&self, var: usize, ch: usize) -> Vec<Rec> {
        (0..801).map(|i| self.rec(var, ch, i)).collect()
    }

    /// The 20 zero trims of channel `ch` (cal_afe_zero).
    pub fn zero_trims(&self, ch: usize) -> [i32; 20] {
        self.zero[ch * 20..ch * 20 + 20].try_into().unwrap()
    }

    /// First record whose scale key is >= `nv` (nV/div). The UI's scales pass through f32, where 100 mV reads
    /// 100000001 nV: a millionth above a key still is that key, the next row is at least 1 % away.
    pub fn scale_index(&self, nv: i64) -> usize {
        self.keys.iter().position(|&k| k as f64 * (1.0 + 1e-6) >= nv as f64).unwrap_or(800)
    }

    pub fn block_filled(&self, var: usize, ch: usize) -> bool {
        self.rec(var, ch, 0).key != 0
    }
}

/// The 801 scale keys of the vertical table (nV/div), 100 uV to 10 V. Each decade: 1.00 by 0.02, 2.00 by 0.05,
/// 5.0 by 0.1. Fine Scale walks these rows, so every scale it offers is one the unit has calibration for.
pub fn scale_ladder() -> Vec<i64> {
    let mut v = Vec::with_capacity(801);
    for d in 0..5 {
        let b = 100_000 * 10i64.pow(d);
        v.extend((0..50).map(|k| b + k * b / 50));
        v.extend((0..60).map(|k| 2 * b + k * b / 20));
        v.extend((0..50).map(|k| 5 * b + k * b / 10));
    }
    v.push(10_000_000_000);
    v
}

/// Bandwidth values.
pub mod bw {
    pub const FULL: u32 = 0x0;
    pub const OFF: u32 = 0x1;
    pub const S20M: u32 = 0x2;
    pub const GND: u32 = 0x11;
    pub const AUTO: u32 = 0x12;
    pub const M155: u32 = 0x5;
    pub const M315: u32 = 0x6;
    pub const M125: u32 = 0x7;
    pub const M350: u32 = 0xa;
    /// 5..310 MHz in 5 MHz steps (except the special values above)
    pub fn mhz(m: u32) -> u32 {
        if m >= 400 {
            0x30_3000 | (m / 50) << 4
        } else {
            (m / 5) << 4
        }
    }
}

/// Auto filter code at sample rate `fs`: fs / (1.25e9 / 63), at least 1. The filter is analog (about fs / 4 in
/// absolute hertz), so the constant stays at any ADC clock; above 1.25 GSa/s (experimental clock) the code is capped
/// at its maximum 63 (filter open).
pub fn auto_c(fs: u64) -> u32 {
    ((fs / 19_841_269) as u32).clamp(1, 63)
}

/// Bandwidth value -> Hz filter (a, b, c).
pub fn filter(v: u32, fs: u64) -> (u32, u32, u32) {
    match v {
        1 => (3, 3, 63),
        2 => (0, 0, 5),
        0x11 => (0, 0, 0),
        5 => (0, 0, 31),
        6 => (0, 0, 63),
        7 => (0, 0, 25),
        0xa => (3, 3, 7),
        0x12 => (0, 0, auto_c(fs)),
        _ => ((v >> 20) & 0xff, (v >> 12) & 0xff, (v >> 4) & 0xff),
    }
}

/// Choice of the cal_vertical block group for bandwidth `v`; `fs` for Auto.
pub fn block_group(v: u32, fs: u64) -> usize {
    let code = match v {
        1 => 0xa,
        2 | 5 | 6 => v,
        7 => 5,
        _ if v & 0xf != 0 => {
            if v != 0x12 {
                2
            } else {
                let c = auto_c(fs);
                if c <= 0xf {
                    2
                } else if c <= 0x2a {
                    5
                } else {
                    6
                }
            }
        }
        _ => {
            if v <= 0xf0 {
                2
            } else if v <= 0x2a0 {
                5
            } else if v <= 0x3f0 {
                6
            } else {
                0xa
            }
        }
    };
    match code {
        2 => 0,
        5 | 7 => 1,
        0xa => 3,
        6 => 2,
        _ => 1,
    }
}

/// Shadows of one AFE and the setter primitives.
#[derive(Clone)]
pub struct Afe {
    pub ch: usize,
    r: [u16; 0x40],
}

impl Afe {
    /// Shadows after the AFE init + efuse read.
    pub fn new(ch: usize) -> Afe {
        let mut r = [0u16; 0x40];
        for (reg, v) in [
            (0x06, 0x1d00), (0x07, 0x8440), (0x20, 0x0008), (0x21, 0x0a91), (0x22, 0x5a00), (0x23, 0x1000), (0x26, 0x3200),
            (0x27, 0x80c4), (0x28, 0x8000), (0x29, 0x0282), (0x2a, 0x3c44), (0x2b, 0x4c5f), (0x2c, 0x000c), (0x2e, 0x1ff9),
            (0x37, 0xc000),
        ] {
            r[reg] = v;
        }
        Afe { ch, r }
    }

    pub fn reg(&self, reg: u8) -> u16 {
        self.r[reg as usize]
    }

    fn w(&mut self, ops: &mut Vec<Op>, reg: u8) {
        ops.push(Op::Afe { ch: self.ch, reg, val: self.r[reg as usize] });
    }
    fn set(&mut self, ops: &mut Vec<Op>, reg: u8, v: i64) {
        self.r[reg as usize] = (v & 0xffff) as u16;
        self.w(ops, reg);
    }
    fn bf(&mut self, reg: u8, lsb: u32, width: u32, v: i64) {
        let m = (((1u32 << width) - 1) << lsb) as i64;
        let cur = self.r[reg as usize] as i64;
        self.r[reg as usize] = ((cur & !m & 0xffff) | ((v << lsb) & m)) as u16;
    }
    fn and(&mut self, reg: u8, m: i64) {
        self.r[reg as usize] = (self.r[reg as usize] as i64 & m) as u16;
    }
    fn or(&mut self, reg: u8, m: i64) {
        self.r[reg as usize] = (self.r[reg as usize] as i64 | m) as u16;
    }

    /// Hz enable of the channel.
    pub fn hz_enable(&mut self, o: &mut Vec<Op>, en: bool) {
        let mode = self.r[0x07] & 0x4000;
        if en {
            if mode == 0x4000 {
                self.bf(0x23, 11, 3, 5);
                self.w(o, 0x23);
                self.and(0x29, !0x300);
                self.w(o, 0x29);
                self.and(0x2a, 0x0fff);
                self.w(o, 0x2a);
            } else {
                self.bf(0x23, 11, 3, 2);
                self.w(o, 0x23);
                self.bf(0x29, 8, 2, 2);
                self.w(o, 0x29);
                self.r[0x2a] = (self.r[0x2a] & 0xfff) | 0x2000;
                self.w(o, 0x2a);
            }
        } else {
            self.and(0x23, !0x3800);
            self.w(o, 0x23);
            self.or(0x29, 0x300);
            self.w(o, 0x29);
            self.or(0x2a, 0xc000);
            self.w(o, 0x2a);
        }
    }
    fn hz_input_gain(&mut self, o: &mut Vec<Op>, g: i32) {
        if g == 0 {
            self.bf(0x07, 13, 2, 2);
            self.w(o, 0x07);
            self.bf(0x23, 11, 3, 5);
            self.w(o, 0x23);
            return;
        }
        self.bf(0x07, 13, 2, 0);
        self.w(o, 0x07);
        if g == 1 {
            self.r[0x20] = ((self.r[0x20] as i64 & 0x1ff8) | 0xa000) as u16;
        } else {
            self.bf(0x20, 0, 3, if g == 2 { 4 } else { 6 });
        }
        self.bf(0x23, 11, 3, 2);
        self.w(o, 0x20);
        self.w(o, 0x23);
    }
    fn hz_vga_gain(&mut self, o: &mut Vec<Op>, v: i32) {
        if v < 2 {
            self.bf(0x07, 13, 2, 2);
            self.w(o, 0x07);
            self.bf(0x29, 8, 2, v as i64);
        } else if v == 2 {
            self.bf(0x07, 13, 2, 0);
            self.w(o, 0x07);
            self.bf(0x29, 8, 2, 2);
        } else {
            self.or(0x29, 0x300);
        }
        self.w(o, 0x29);
    }
    fn hz_att_gain(&mut self, o: &mut Vec<Op>, v: i32) {
        self.bf(0x29, 6, 2, v as i64);
        self.w(o, 0x29);
    }
    fn hz_sga_gain(&mut self, o: &mut Vec<Op>, v: i32) {
        self.bf(0x07, 13, 2, if v >= 2 { 0 } else { 2 });
        self.w(o, 0x07);
        self.bf(0x2a, 12, 2, v as i64);
        self.w(o, 0x2a);
    }
    fn hz_sgar_gain(&mut self, o: &mut Vec<Op>, v: i32) {
        self.bf(0x2a, 14, 2, v as i64);
        self.w(o, 0x2a);
    }
    fn mux(&mut self, o: &mut Vec<Op>, v: i32) {
        self.bf(0x2e, 11, 3, v as i64);
        self.w(o, 0x2e);
    }
    fn muxr(&mut self, o: &mut Vec<Op>, v: i32) {
        self.bf(0x2e, 14, 2, v as i64);
        self.w(o, 0x2e);
    }
    fn hz_output(&mut self, o: &mut Vec<Op>, which: u32, en: bool) {
        self.bf(0x2b, if which == 0 { 10 } else { 11 }, 1, en as i64);
        self.w(o, 0x2b);
    }
    /// Coupling: r07[15] = 1 for DC, 0 for AC.
    pub fn coupling_dc(&mut self, o: &mut Vec<Op>, dc: bool) {
        self.bf(0x07, 15, 1, dc as i64);
        self.w(o, 0x07);
    }
    /// Hz filter (a, b, c): reg 0x2e bits 7:6 = a, 10:9 = b, 5:0 = c.
    pub fn hz_filter(&mut self, o: &mut Vec<Op>, (a, b, c): (u32, u32, u32)) {
        let v = (self.r[0x2e] as u32 & 0xf900) | (a & 3) << 6 | (b & 3) << 9 | (c & 0x3f);
        self.set(o, 0x2e, v as i64);
    }
    /// The zero trims of the 20-int `cal_afe_zero` record for the gain path.
    pub fn apply_zero_trims(&mut self, o: &mut Vec<Op>, z: &[i32], ig: i32, vga: i32) {
        if (1..=3).contains(&ig) {
            if vga == 2 {
                self.cal_zero_first(o, 0, z[14], z[15]);
                self.cal_zero_sga(o, z[16], z[17]);
                self.cal_zero_mux(o, z[18], z[19]);
            }
        } else if ig == 0 {
            if vga == 0 {
                self.cal_zero_first(o, 0x4000, z[0], z[1]);
                self.cal_zero_vga4000(o, z[2], true);
                self.cal_zero_sga(o, z[3], z[4]);
                self.cal_zero_mux(o, z[5], z[6]);
            } else if vga == 1 {
                self.cal_zero_first(o, 0x4000, z[7], z[8]);
                self.cal_zero_vga4000(o, z[9], false);
                self.cal_zero_sga(o, z[10], z[11]);
                self.cal_zero_mux(o, z[12], z[13]);
            }
        }
    }

    // ---- self-calibration primitives (docs/protocols/self-cal.md 2-3; the comparator input/self-check register is 0x2a)

    /// Gain tuple (IG, VGA, ATT, SGA, SGAR, MUX, MUXR) with Hz output 0 on and Hz output 1 off, as the calibration sets it.
    pub fn set_tuple(&mut self, o: &mut Vec<Op>, g: [i32; 7]) {
        self.hz_input_gain(o, g[0]);
        self.hz_vga_gain(o, g[1]);
        self.hz_att_gain(o, g[2]);
        self.hz_sga_gain(o, g[3]);
        self.hz_sgar_gain(o, g[4]);
        self.mux(o, g[5]);
        self.muxr(o, g[6]);
        self.hz_output(o, 0, true);
        self.hz_output(o, 1, false);
    }
    /// Input mode of the path set (r07[14:13] as the cal reads it: 0x4000 non-inverting IG 0, 0 inverting IG 1..3).
    pub fn input_mode(&self) -> u32 {
        self.r[0x07] as u32 & 0x4000
    }
    /// Zero calibration self check (`mode`, `en`).
    pub fn cz_self_check(&mut self, o: &mut Vec<Op>, mode: u32, en: bool) {
        if mode == 0x4000 {
            if en {
                self.r[0x24] = ((self.r[0x24] as u32 & !0x7800) | 0x4400) as u16;
                self.or(0x23, 0x2000);
                self.w(o, 0x23);
            } else {
                self.and(0x24, 0xbbff);
            }
            self.w(o, 0x24);
        } else {
            self.r[0x26] = if en { 0x9800 | (self.r[0x26] & 0x7ff) } else { self.r[0x26] & 0x37ff };
            self.w(o, 0x26);
        }
    }
    /// Zero calibration mode: r26 = 0x5800 | r26[10:0].
    pub fn cz_mode(&mut self, o: &mut Vec<Op>) {
        let v = 0x5800 | (self.r[0x26] as i64 & 0x7ff);
        self.set(o, 0x26, v);
    }
    /// Comparator self check: r2a |= 0x160.
    pub fn comp_self_check(&mut self, o: &mut Vec<Op>) {
        self.or(0x2a, 0x160);
        self.w(o, 0x2a);
    }
    /// Comparator input: SGA (r2a[6:5] = 2 for the input modes 0 and 0x4000) or MUX (r2a[6:5] = 1).
    pub fn comp_input(&mut self, o: &mut Vec<Op>, mux: bool) {
        self.bf(0x2a, 5, 2, if mux { 1 } else { 2 });
        self.w(o, 0x2a);
    }
    /// Comparator trim r07[6:0].
    pub fn comp_trim(&mut self, o: &mut Vec<Op>, v: i32) {
        self.bf(0x07, 0, 7, v as i64);
        self.w(o, 0x07);
    }
    /// VGA output in mode 0x4000: r29[3:0] = sel ? 0 : 1.
    pub fn vga_output(&mut self, o: &mut Vec<Op>, sel: bool) {
        self.bf(0x29, 0, 4, if sel { 0 } else { 1 });
        self.w(o, 0x29);
    }
    /// Buffer trim: r33[7:0] = a | b << 4.
    pub fn cz_buf(&mut self, o: &mut Vec<Op>, a: i32, b: i32) {
        self.bf(0x33, 0, 8, ((a & 0xf) | (b & 0xf) << 4) as i64);
        self.w(o, 0x33);
    }
    /// Offset DAC on/off (r27 bits 15 and 7).
    pub fn dac_enable(&mut self, o: &mut Vec<Op>, en: bool) {
        self.bf(0x27, 7, 1, en as i64);
        self.bf(0x27, 15, 1, en as i64);
        self.w(o, 0x27);
    }
    /// Offset DAC r28.
    pub fn set_dac(&mut self, o: &mut Vec<Op>, code: i32) {
        self.set(o, 0x28, code as i64);
    }
    /// Zero trim writers with the search results (public for the self-cal).
    pub fn cz_first(&mut self, o: &mut Vec<Op>, mode: u32, sign: i32, mag: i32) {
        self.cal_zero_first(o, mode, sign, mag);
    }
    pub fn cz_vga(&mut self, o: &mut Vec<Op>, code: i32, sel: bool) {
        self.cal_zero_vga4000(o, code, sel);
    }
    pub fn cz_sga(&mut self, o: &mut Vec<Op>, p: i32, n: i32) {
        self.cal_zero_sga(o, p, n);
    }
    pub fn cz_mux(&mut self, o: &mut Vec<Op>, p: i32, n: i32) {
        self.cal_zero_mux(o, p, n);
    }

    fn cal_zero_first(&mut self, o: &mut Vec<Op>, mode: u32, a: i32, b: i32) {
        let reg = if mode == 0x4000 { 0x23 } else { 0x26 };
        let v = (self.r[reg as usize] as i64 & 0xf800) | ((a as i64) & 1) << 10 | ((b as i64) & 0x3ff);
        self.set(o, reg, v);
    }
    fn cal_zero_vga4000(&mut self, o: &mut Vec<Op>, val: i32, sel: bool) {
        if sel {
            self.bf(0x35, 0, 9, val as i64);
            self.w(o, 0x35);
        } else {
            self.r[0x29] = ((self.r[0x29] as i64 & 0x3ff) | (((val as i64) << 7) & 0xfc00)) as u16;
            self.bf(0x35, 9, 3, (val & 7) as i64);
            self.w(o, 0x29);
            self.w(o, 0x35);
        }
    }
    fn cal_zero_sga(&mut self, o: &mut Vec<Op>, a: i32, b: i32) {
        let v = (self.r[0x27] as i64 & 0x8080) | ((a as i64) & 0x7f) << 8 | ((b as i64) & 0x7f);
        self.set(o, 0x27, v);
    }
    fn cal_zero_mux(&mut self, o: &mut Vec<Op>, a: i32, b: i32) {
        let v = (self.r[0x06] as i64 & 0x8080) | ((b as i64) & 0x7f) << 8 | ((a as i64) & 0x7f);
        self.set(o, 0x06, v);
    }
    fn cal_bw(&mut self, o: &mut Vec<Op>, sel: u32, v: i32) {
        let (reg, lsb, width) = match sel {
            1 => (0x22, 8, 8),
            6 => (0x20, 4, 3),
            7 => (0x21, 7, 5),
            8 => (0x21, 0, 7),
            9 => (0x21, 12, 4),
            10 => (0x24, 0, 10),
            _ => (0x25, 0, 10),
        };
        self.bf(reg, lsb, width, v as i64);
        self.w(o, reg);
    }

    /// Gain codes of the record for `scale_nv` in block group `var`, gear 1, zero and
    /// bandwidth trims. Returns the record (FPGA digital gain, DAC zero).
    pub fn set_scale(&mut self, o: &mut Vec<Op>, cal: &Cal, scale_nv: i64, var: usize) -> Rec {
        // a block group the file leaves empty has gain 0 everywhere: a dead channel, the first group is close enough
        let var = if cal.block_filled(var, self.ch) { var } else { 0 };
        let r = cal.rec(var, self.ch, cal.scale_index(scale_nv));
        let g: [i32; 7] = r.g[..7].try_into().unwrap();
        let z = cal.zero_trims(self.ch);
        self.set_config(o, cal, g, &z);
        r
    }

    /// The `set_scale` writes for an explicit gain tuple and zero trims (the self-cal measures every configuration in
    /// exactly the state normal operation puts it in): gain codes, outputs, gear 1, zero trims, bandwidth trims.
    pub fn set_config(&mut self, o: &mut Vec<Op>, cal: &Cal, g: [i32; 7], z: &[i32]) {
        let ch = self.ch;
        let [ig, vga, att, sga, sgar, mux, muxr] = g;
        self.hz_enable(o, true);
        self.hz_input_gain(o, ig);
        self.hz_vga_gain(o, vga);
        self.hz_att_gain(o, att);
        self.hz_sga_gain(o, sga);
        self.hz_sgar_gain(o, sgar);
        self.mux(o, mux);
        self.muxr(o, muxr);
        self.hz_output(o, 0, true);
        self.hz_output(o, 1, false);
        o.push(Op::Gear { ch, gear: 1 });
        self.apply_zero_trims(o, z, ig, vga);
        let b = &cal.bw[ch * 20..ch * 20 + 20];
        if (1..=3).contains(&ig) {
            let off = ((ig * 0x18 - 0x14) / 4) as usize;
            let mut vals = [b[off], b[off + 1], b[off + 2], b[off + 3], b[off + 4]];
            // bandwidth trims: r21[11:7] = 17, r21[6:0] = 126 fixed
            vals[2] = 17;
            vals[3] = 126;
            for (sel, v) in [1, 6, 7, 8, 9].into_iter().zip(vals) {
                self.cal_bw(o, sel, v);
            }
        } else if ig == 0 {
            self.cal_bw(o, 10, b[18]);
            self.cal_bw(o, 11, b[19]);
        }
    }

    /// Offset DAC (AFE reg 0x28) code = trunc(-(sign*offset + position) / 1e9 / lsb + zero).
    pub fn set_offset(&mut self, o: &mut Vec<Op>, cal: &Cal, r: &Rec, offset_nv: i64, position_nv: i64, invert: bool) -> i64 {
        let lsb = cal.lsb[self.ch * 4 + r.g[0] as usize];
        let sign = if invert { -1.0 } else { 1.0 };
        // past the end of the DAC it stays at the end; wrapped it would throw the trace the other way (3.2)
        let code = if lsb != 0.0 { ((-(sign * offset_nv as f64 + position_nv as f64) / 1e9 / lsb + r.zero as f64) as i64).clamp(0, 0xffff) } else { r.zero as i64 };
        self.set(o, 0x28, code);
        code
    }
}

/// Access to the four AFEs (24-bit SPI frames: 8-bit register, 16-bit value, MSB first; read = register | 0x80, then
/// 16 bits in). `GpioAfe` is the bit-banged bus of the unit, tests use a recorder.
pub trait AfeBus {
    fn write(&mut self, ch: usize, reg: u8, val: u16);
    fn read(&mut self, ch: usize, reg: u8) -> u16;
}

/// AFE init table (registers 0x20..0x37), same for all channels.
const INIT_TABLE: [(u8, u16); 24] = [
    (0x20, 0x0008), (0x21, 0x0a91), (0x22, 0x5a00), (0x23, 0x1000), (0x24, 0x0000), (0x25, 0x0000), (0x26, 0x3200), (0x27, 0x0044),
    (0x28, 0x0000), (0x29, 0x0282), (0x2a, 0x3c40), (0x2b, 0x4c00), (0x2c, 0x000c), (0x2d, 0x0000), (0x2e, 0x1ff9), (0x2f, 0x5980),
    (0x30, 0x0000), (0x31, 0x0000), (0x32, 0xc000), (0x33, 0x0088), (0x34, 0x0009), (0x35, 0x0000), (0x36, 0x0000), (0x37, 0xc000),
];

impl Afe {
    fn wb(&mut self, bus: &mut impl AfeBus, reg: u8, v: u16) {
        self.r[reg as usize] = v;
        bus.write(self.ch, reg, v);
    }

    /// One factory-trim word, bit by bit through regs 0x2a/0x2b and reg 0 bit 0.
    fn efuse(&mut self, bus: &mut impl AfeBus, addr: u16) -> u16 {
        let idx = ((((addr & 0xf0) + 0x1c0) >> 1) & 0xf8) | (addr & 0xf);
        let (mut r2a, mut r2b) = (self.r[0x2a], self.r[0x2b]);
        r2b |= 0x100;
        self.wb(bus, 0x2b, r2b);
        r2b |= 0x200;
        self.wb(bus, 0x2b, r2b);
        r2a &= !0x200;
        self.wb(bus, 0x2a, r2a);
        r2b = (r2b & !0x70) | (((((addr << 4) + 0x10) >> 4) & 7) << 4);
        self.wb(bus, 0x2b, r2b);
        let bits = if idx == 0x14 { 5 } else { 16 };
        let mut res = 0u16;
        for i in 0..bits {
            if idx == 0x14 {
                r2a = (r2a & !7) | i;
                self.wb(bus, 0x2a, r2a);
            } else {
                r2b = (r2b & !0xf) | i;
                self.wb(bus, 0x2b, r2b);
            }
            r2b &= !0x1000;
            self.wb(bus, 0x2b, r2b);
            r2a &= !0x200;
            self.wb(bus, 0x2a, r2a);
            res |= (bus.read(self.ch, 0) & 1) << i;
        }
        r2b &= !0x100;
        self.wb(bus, 0x2b, r2b);
        r2b &= !0x200;
        self.wb(bus, 0x2b, r2b);
        res
    }

    /// AFE init (as `oos-hw afe-init`, run on the unit 2026-11-02): table, per-channel registers, the six
    /// efuse words 0x57, 0x60..0x64 and the trim registers 0x2f..0x34. Returns the AFE with its shadows and the efuse.
    pub fn init(bus: &mut impl AfeBus, ch: usize) -> (Afe, [u16; 6]) {
        let mut a = Afe { ch, r: [0; 0x40] };
        for (reg, v) in INIT_TABLE {
            a.wb(bus, reg, v);
        }
        a.wb(bus, 0x06, 0x1d00);
        a.wb(bus, 0x07, 0x8440);
        a.wb(bus, 0x27, 0x0044 | 0x8080);
        a.wb(bus, 0x28, 0x8000);
        let r2b = a.r[0x2b] | 0x300;
        a.wb(bus, 0x2b, r2b);
        let mut e = [0u16; 6];
        for (k, addr) in [0x57, 0x60, 0x61, 0x62, 0x63, 0x64].into_iter().enumerate() {
            e[k] = a.efuse(bus, addr);
        }
        let r2b = a.r[0x2b] & !0x300;
        a.wb(bus, 0x2b, r2b);
        a.wb(bus, 0x2f, e[0] | 0xc);
        a.wb(bus, 0x30, e[1] & !0x70);
        a.wb(bus, 0x31, e[2]);
        a.wb(bus, 0x32, 0x8000 | (e[3] & 0x3fff));
        a.wb(bus, 0x33, e[4]);
        a.wb(bus, 0x34, e[5]);
        (a, e)
    }

    /// Sends the AFE writes of `ops` to `bus` (gear and ADC ops are for the caller).
    pub fn apply(bus: &mut impl AfeBus, ops: &[Op]) {
        for op in ops {
            if let Op::Afe { ch, reg, val } = *op {
                bus.write(ch, reg, val);
            }
        }
    }
}

/// The bit-banged AFE bus of the unit (protocol of Rigol's spi2afe_gpio.ko: SPI mode 0, MSB first, ~5 us half period,
/// chip selects idle high), GPIO lines `spi_afe_sclk/mosi/miso`, `spi_afe_cs_0..3` from the device tree.
#[cfg(target_os = "linux")]
pub struct GpioAfe {
    sclk: crate::gpio::Line,
    mosi: crate::gpio::Line,
    miso: crate::gpio::Line,
    cs: [crate::gpio::Line; 4],
}

#[cfg(target_os = "linux")]
impl GpioAfe {
    pub fn open() -> std::io::Result<GpioAfe> {
        use crate::gpio::Line;
        let cs = |n: usize| Line::output(&format!("spi_afe_cs_{}", n), true);
        Ok(GpioAfe {
            sclk: Line::output("spi_afe_sclk", false)?,
            mosi: Line::output("spi_afe_mosi", false)?,
            miso: Line::input("spi_afe_miso")?,
            cs: [cs(0)?, cs(1)?, cs(2)?, cs(3)?],
        })
    }
    /// Half a clock period, 5 us. Spun: a sleep of 5 us takes 60 us or more on this kernel, which made one AFE set-up
    /// at power-on 11 s and every change of a scale slow.
    fn half() {
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_micros(5) {
            std::hint::spin_loop();
        }
    }
    fn set(l: &crate::gpio::Line, v: bool) {
        l.set(v);
        Self::half();
    }
    fn byte_out(&self, v: u8) {
        Self::set(&self.sclk, false);
        for i in (0..8).rev() {
            Self::set(&self.mosi, (v >> i) & 1 != 0);
            Self::set(&self.sclk, false);
            Self::set(&self.sclk, true);
        }
        Self::set(&self.sclk, false);
    }
    fn byte_in(&self) -> u8 {
        let mut v = 0u8;
        Self::set(&self.sclk, false);
        for _ in 0..8 {
            Self::set(&self.sclk, false);
            Self::set(&self.sclk, true);
            v = (v << 1) | self.miso.get() as u8;
        }
        Self::set(&self.sclk, false);
        v
    }
}

#[cfg(target_os = "linux")]
impl AfeBus for GpioAfe {
    fn write(&mut self, ch: usize, reg: u8, val: u16) {
        Self::set(&self.cs[ch], false);
        self.byte_out(reg);
        self.byte_out((val >> 8) as u8);
        self.byte_out(val as u8);
        Self::set(&self.cs[ch], true);
    }
    fn read(&mut self, ch: usize, reg: u8) -> u16 {
        Self::set(&self.cs[ch], false);
        self.byte_out(reg | 0x80);
        let hi = self.byte_in() as u16;
        let lo = self.byte_in() as u16;
        Self::set(&self.cs[ch], true);
        hi << 8 | lo
    }
}

/// `ioctl(/dev/dac_gear_change, 0x7801, &(ch | gear << 4))`: per-channel gear GPIO (afe1..afe4). Under OOS (no
/// `dac_gpio.ko`) the line `afe<ch+1>` is driven directly through the GPIO character device, as `dac_ioctl` does
/// (`gpiod_set_raw_value`); the line handle is kept open for the life of the process.
#[cfg(target_os = "linux")]
pub fn set_gear(ch: usize, gear: u8) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    if !std::path::Path::new("/dev/dac_gear_change").exists() {
        use std::sync::Mutex;
        static LINES: Mutex<Vec<(usize, crate::gpio::Line)>> = Mutex::new(Vec::new());
        let mut lines = LINES.lock().unwrap();
        if let Some((_, l)) = lines.iter().find(|(c, _)| *c == ch) {
            l.set(gear & 1 != 0);
        } else {
            lines.push((ch, crate::gpio::Line::output(&format!("afe{}", ch + 1), gear & 1 != 0)?));
        }
        return Ok(());
    }
    let f = std::fs::OpenOptions::new().read(true).write(true).open("/dev/dac_gear_change")?;
    let mut b: u8 = ch as u8 | gear << 4;
    if unsafe { libc::ioctl(f.as_raw_fd(), 0x7801 as _, &mut b) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The two ADC words every band-limit change sends to FPGA 0x1034 first.
pub const BANDLIMIT_ADC: [u32; 2] = [0x0154_5251, 0x0155_000c];

/// ADC input-mode words (ADC regs 0x54/0x55 through 0x1034) of channel `mask` (table in
/// horizontal-config.md 2.3 / adc-mode.md). The restore after a band-limit change writes the same two words
/// from the shadows (reg 0x54 ^ 0x1041, reg 0x55 ^ 4).
pub fn adc_mode_words(mask: u32) -> [u32; 2] {
    let (a, b) = match mask & 0xf {
        1 => (0x9460, 0x0004),
        2 => (0x1803, 0x0005),
        4 => (0x3149, 0x0000),
        8 => (0x00c5, 0x0006),
        3 => (0x1860, 0x0005),
        5 => (0x3060, 0x0000),
        9 => (0x0060, 0x0006),
        6 => (0x3003, 0x0000),
        0xa => (0x0003, 0x0006),
        0xc => (0x0149, 0x0006),
        _ => (0x0000, 0x0000),
    };
    [0x0154_0000 | a, 0x0155_0000 | b]
}

/// Full channel set-up: ADC band-limit words, Hz filter of bandwidth `bwv`, scale, offset, then the input mode
/// restore, which brings back the ADC input mode of the current channel
/// set `mask` (the band-limit words leave the ADC in another mode). Op::Adc words need 100 us gaps.
pub fn configure(afe: &mut Afe, cal: &Cal, scale_nv: i64, offset_nv: i64, bwv: u32, fs: u64, mask: u32) -> (Vec<Op>, Rec, i64) {
    configure_ch(afe, cal, scale_nv, offset_nv, bwv, fs, mask, false, Some(true))
}

/// `configure` with the mirrored offset of an inverted channel (3.5) and the coupling: `dc` Some(true) DC, Some(false)
/// AC, None leaves it as it is (GND, 3.3). The coupling is written only when it changes.
#[allow(clippy::too_many_arguments)]
pub fn configure_ch(afe: &mut Afe, cal: &Cal, scale_nv: i64, offset_nv: i64, bwv: u32, fs: u64, mask: u32, invert: bool, dc: Option<bool>) -> (Vec<Op>, Rec, i64) {
    let mut o: Vec<Op> = BANDLIMIT_ADC.iter().map(|&w| Op::Adc(w)).collect();
    afe.hz_filter(&mut o, filter(bwv, fs));
    let r = afe.set_scale(&mut o, cal, scale_nv, block_group(bwv, fs));
    let code = afe.set_offset(&mut o, cal, &r, offset_nv, 0, invert);
    if let Some(dc) = dc.filter(|&d| (afe.r[0x07] >> 15 & 1 == 1) != d) {
        afe.coupling_dc(&mut o, dc);
    }
    o.extend(adc_mode_words(mask).iter().map(|&w| Op::Adc(w)));
    (o, r, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal");

    #[derive(Default)]
    struct Rec {
        w: Vec<(usize, u8, u16)>,
        efuse_bit: u16,
    }
    impl AfeBus for Rec {
        fn write(&mut self, ch: usize, reg: u8, val: u16) {
            self.w.push((ch, reg, val));
        }
        fn read(&mut self, _ch: usize, _reg: u8) -> u16 {
            self.efuse_bit
        }
    }

    #[test]
    fn init_leaves_the_stock_shadows() {
        // efuse all zero: the shadows after init are those afe-settings.md / afesim.py start from
        let mut bus = Rec::default();
        let (a, e) = Afe::init(&mut bus, 0);
        assert_eq!(e, [0; 6]);
        let fresh = Afe::new(0);
        for reg in [0x06u8, 0x07, 0x20, 0x21, 0x22, 0x23, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2e, 0x37] {
            assert_eq!(a.reg(reg), fresh.reg(reg), "reg {:#x}", reg);
        }
        assert_eq!((a.reg(0x2a), a.reg(0x2b)), (0x3c44, 0x4c5f));
        // 24 table writes, 5 single, 5 x (4 + 16 x 3 + 2) + (4 + 5 x 3 + 2) efuse, 1 + 6 trim
        assert_eq!(bus.w.len(), 24 + 5 + 5 * 54 + 21 + 7);
        // all efuse bits 1 -> words of 16 / 5 ones, trims written from them
        let mut bus = Rec { efuse_bit: 1, ..Default::default() };
        let (_, e) = Afe::init(&mut bus, 2);
        assert_eq!(e, [0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0x1f]);
        assert_eq!(bus.w.last(), Some(&(2, 0x34, 0x1f)));
    }

    #[test]
    fn filters_and_groups() {
        assert_eq!(filter(bw::OFF, 1_250_000_000), (3, 3, 63));
        assert_eq!(filter(bw::AUTO, 1_250_000_000), (0, 0, 63));
        assert_eq!(filter(bw::mhz(100), 0), (0, 0, 20));
        assert_eq!(filter(bw::mhz(900), 0), (3, 3, 18));
        assert_eq!(block_group(bw::OFF, 0), 3);
        assert_eq!(block_group(bw::S20M, 0), 0);
        assert_eq!(block_group(bw::mhz(100), 0), 1);
        assert_eq!(block_group(bw::mhz(300), 0), 2);
        assert_eq!(block_group(bw::AUTO, 1_250_000_000), 2);
    }

    /// Compares the full write sequence with tools/afesim.py (testdata/afe_expected.txt) for six channel set-ups.
    /// Needs the unit's calibration files (device data, not in the tree); skipped when they are missing.
    #[test]
    fn the_ladder_is_the_table() {
        let l = scale_ladder();
        assert_eq!((l.len(), l[160], l[270], l[480], l[530], l[640], l[800]), (801, 1_000_000, 5_000_000, 100_000_000, 200_000_000, 1_000_000_000, 10_000_000_000));
        assert!(l.windows(2).all(|w| w[0] < w[1] && w[1] as f64 / w[0] as f64 <= 1.0251));
        let Ok(cal) = Cal::load(&std::env::var("OOS_CAL_DIR").unwrap_or(CAL_DIR.into())) else {
            eprintln!("calibration files not found, skipped");
            return;
        };
        assert_eq!(l, (0..801).map(|i| cal.key(i)).collect::<Vec<_>>());
        // a scale that went through f32 finds its own row
        for (i, &k) in l.iter().enumerate() {
            let f = (k as f64 * 1e-9) as f32;
            assert_eq!(cal.scale_index((f as f64 * 1e9) as i64), i, "{} nV", k);
        }
    }

    #[test]
    fn matches_afesim() {
        let Ok(cal) = Cal::load(&std::env::var("OOS_CAL_DIR").unwrap_or(CAL_DIR.into())) else {
            eprintln!("calibration files not found, skipped");
            return;
        };
        let want = include_str!("../testdata/afe_expected.txt");
        let bwv = |s: &str| match s {
            "OFF" => bw::OFF,
            "S20M" => bw::S20M,
            "GND" => bw::GND,
            "Auto" => bw::AUTO,
            "350M" => bw::M350,
            m => bw::mhz(m.trim_end_matches('M').parse().unwrap()),
        };
        for case in want.split("== ").filter(|c| !c.is_empty()) {
            let mut lines = case.lines();
            let a: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
            let ch: usize = a[0].parse::<usize>().unwrap() - 1;
            let scale = (a[1].parse::<f64>().unwrap() * 1e9) as i64;
            let off = (a[2].parse::<f64>().unwrap() * 1e9) as i64;
            let v = bwv(a[3]);
            let mut afe = Afe::new(ch);
            let (ops, r, code) = configure(&mut afe, &cal, scale, off, v, 1_250_000_000, 1 << ch);
            // ends with the restore: the input mode of the channel set
            assert_eq!(&ops[ops.len() - 2..], &[Op::Adc(adc_mode_words(1 << ch)[0]), Op::Adc(adc_mode_words(1 << ch)[1])]);
            let (fa, fb, fc) = filter(v, 1_250_000_000);
            let mut got = vec![format!("filter {} {} {} group {}", fa, fb, fc, block_group(v, 1_250_000_000))];
            for op in &ops {
                match op {
                    Op::Afe { ch, reg, val } => got.push(format!("w {} {:#04x} {:#06x}", ch + 1, reg, val)),
                    Op::Gear { ch, gear } => got.push(format!("gear {:#04x}", ch | (*gear as usize) << 4)),
                    Op::Adc(_) => {}
                }
            }
            got.push(format!("dac {} gain {:.5}", code, r.gain));
            let want: Vec<&str> = lines.collect();
            assert_eq!(got, want, "case {:?}", a);
        }
    }
}
