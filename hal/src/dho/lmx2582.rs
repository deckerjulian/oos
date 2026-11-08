// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! TI LMX2582 sample-clock synthesizer (docs/protocols/pll.md): field decode of a 46-word table like Rigol's
//! `PLL_TABLE`, and tables for other output frequencies, so the unit can later be tested above 1.25 GSa/s.
//!
//! Field positions and limits are from the TI datasheet SNAS680E (rev. E, August 2022): register field descriptions
//! (Tables 7-5..7-50), input path boundaries (Table 7-1), MASH order vs. minimum N and PFD delay (Table 7-2), channel
//! divider vs. VCO frequency (Table 7-4). Pure code, no hardware access: `table_for` only builds the words, writing
//! them stays with `oos-fpgad pll` / the cold boot.
//!
//! `table_for` starts from `PLL_TABLE` and changes only the frequency-dependent fields (PLL_N, PLL_NUM, PLL_DEN, the
//! channel divider segments, VTUNE_ADJ). The reference path (100 MHz OSCin straight to a 100 MHz PFD), charge pump,
//! output B buffer, MASH order / PFD delay and the calibration set-up stay Rigol's: those are the parts that are proven
//! on the board, and the loop filter on the PCB was designed around them. None of the other tables has run on a unit.

use crate::dho::boot::regs::PLL_TABLE;

/// One LMX2582 table: `[addr, data hi, data lo]`, written in this order (R0 with FCAL_EN last starts the calibration).
pub type Table = [[u8; 3]; 46];

/// OSCin of the board, inferred from Rigol's table [G, from the values]: the reference path is bypassed (OSC_2X 0,
/// PLL_R_PRE 1, MULT 1, PLL_R 1), so f_PD = f_OSC, and f_OUT = f_OSC x 2 x 25 / 4 = 12.5 x f_OSC = 1.25 GHz only
/// for 100 MHz. Consistent with the rest of the table: CAL_CLK_DIV 0 (calibration clock = OSCin must be <= 200 MHz),
/// ACAL_CMP_DLY 25 (> 10 is the datasheet's figure for 100 MHz), FCAL_HPFD_ADJ 0 (PFD <= 100 MHz) and
/// FCAL_LPFD_ADJ 0 (PFD >= 10 MHz); the VCO range alone would allow 71..142 MHz. Not measured on the PCB.
pub const F_OSC_HZ: u64 = 100_000_000;

/// VCO range of the datasheet (7.3.5).
pub const VCO_MIN_HZ: u64 = 3_550_000_000;
pub const VCO_MAX_HZ: u64 = 7_100_000_000;
/// Largest PLL_N (12-bit field R38[12:1]).
pub const N_MAX: u32 = 4095;
/// VTUNE_ADJ (R30[7:6]) must be 3 from this VCO frequency up, else 0 (Table 7-25).
pub const VTUNE_ADJ_FROM_HZ: u64 = 6_500_000_000;

/// The CHDIV 4 limit of Table 7-4 (VCO <= 4736 MHz) is raised to Rigol's own operating point: the stock table runs
/// the VCO at 5000 MHz through /4 on every DHO800/900, i.e. 264 MHz above rev. E of the datasheet (the limit was
/// changed in that revision, the TICS Pro export is older). Only this one proven point is admitted, nothing above.
pub const CHDIV4_STOCK_VCO_HZ: u64 = 5_000_000_000;

/// A channel divider setting of Table 7-4: total division, the divide values of segments 1..3 (1 = segment not in
/// the path) and the VCO range allowed with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChDiv {
    pub total: u32,
    pub seg: [u32; 3],
    pub vco_min_hz: u64,
    pub vco_max_hz: u64,
}

const fn cd(total: u32, s1: u32, s2: u32, s3: u32, min_mhz: u64, max_mhz: u64) -> ChDiv {
    ChDiv { total, seg: [s1, s2, s3], vco_min_hz: min_mhz * 1_000_000, vco_max_hz: max_mhz * 1_000_000 }
}

/// Table 7-4 (SNAS680E), CHDIV 4 extended to the stock point (`CHDIV4_STOCK_VCO_HZ`). The VCO-direct path
/// (OUTB_MUX = 1, 3.55..7.1 GHz out) is deliberately not offered: the board feeds output B through the divider.
pub const CHDIVS: [ChDiv; 15] = [
    cd(2, 2, 1, 1, 3550, 7100),
    cd(3, 3, 1, 1, 3552, 6600),
    cd(4, 2, 2, 1, 3552, CHDIV4_STOCK_VCO_HZ / 1_000_000),
    cd(6, 3, 2, 1, 3552, 5328),
    cd(8, 2, 4, 1, 3552, 4736),
    cd(12, 2, 6, 1, 3552, 5328),
    cd(16, 2, 8, 1, 3552, 4736),
    cd(24, 3, 8, 1, 3552, 5328),
    cd(32, 2, 8, 2, 3552, 4736),
    cd(36, 3, 6, 2, 3564, 3996),
    cd(48, 3, 8, 2, 3552, 4752),
    cd(64, 2, 8, 4, 3584, 4736),
    cd(96, 2, 8, 6, 3552, 5376),
    cd(128, 2, 8, 8, 3584, 4736),
    cd(192, 3, 8, 8, 3840, 5376),
];
/// Table 7-4 as printed: VCO maximum for CHDIV 4 (only used to flag the stock point in `Fields::notes`).
const CHDIV4_DATASHEET_MAX_HZ: u64 = 4_736_000_000;

/// Minimum PLL_N per MASH order 0 (integer) .. 4 (Table 7-2).
pub const N_MIN: [u32; 5] = [9, 11, 16, 18, 30];
/// Recommended PFD_DLY code per MASH order 0..4 (Table 7-2).
pub const PFD_DLY_REC: [u32; 5] = [1, 1, 2, 2, 8];

fn word(t: &Table, addr: u8) -> Option<u16> {
    t.iter().find(|w| w[0] == addr).map(|w| (w[1] as u16) << 8 | w[2] as u16)
}

fn bits(v: u16, msb: u32, lsb: u32) -> u32 {
    (v as u32 >> lsb) & ((1 << (msb - lsb + 1)) - 1)
}

/// Replace bits msb..lsb of register `addr` in `t` (the register must be in the table).
fn set(t: &mut Table, addr: u8, msb: u32, lsb: u32, val: u32) {
    let w = t.iter_mut().find(|w| w[0] == addr).expect("register in table");
    let mask = ((1u32 << (msb - lsb + 1)) - 1) << lsb;
    let v = ((w[1] as u32) << 8 | w[2] as u32) & !mask | (val << lsb) & mask;
    (w[1], w[2]) = ((v >> 8) as u8, v as u8);
}

/// Register code of a CHDIV_SEG2/SEG3 divide value (2/4/6/8 -> 1/2/4/8).
fn seg_code(div: u32) -> u32 {
    match div {
        2 => 1,
        4 => 2,
        6 => 4,
        8 => 8,
        _ => 0,
    }
}

fn seg_div(code: u32) -> u32 {
    match code {
        1 => 2,
        2 => 4,
        4 => 6,
        8 => 8,
        _ => 0,
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// The fields of a table that matter for the frequency, the outputs and the calibration (SNAS680E names; register
/// and bit positions in the comments). Divider/multiplier fields hold the factor (1 = bypassed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fields {
    // reference path: f_PD = f_OSC x (OSC_2X ? 2 : 1) / PLL_R_PRE x MULT / PLL_R
    /// R9[11]
    pub osc_2x: bool,
    /// R12[11:0]
    pub pll_r_pre: u32,
    /// R10[11:7] (1 = bypass)
    pub mult: u32,
    /// R11[11:4]
    pub pll_r: u32,
    // N divider: f_VCO = f_PD x PLL_N_PRE x (PLL_N + PLL_NUM / PLL_DEN)
    /// R37[12]: 0 = /2, 1 = /4; holds the factor
    pub pll_n_pre: u32,
    /// R38[12:1]
    pub pll_n: u32,
    /// R44 (31:16), R45 (15:0)
    pub pll_num: u32,
    /// R40 (31:16), R41 (15:0)
    pub pll_den: u32,
    /// R46[2:0], 0 = integer mode
    pub mash_order: u32,
    /// R30[10]
    pub mash_dither: bool,
    /// R42 (31:16), R43 (15:0)
    pub mash_seed: u32,
    /// R39[13:8] code (1/2/4/8/16 = 4/6/8/12/16 cycles)
    pub pfd_dly: u32,
    // channel divider and distribution
    /// R34[5]
    pub chdiv_en: bool,
    /// divide values of segment 1 (R35[2]: 2/3), 2 (R35[12:9]) and 3 (R36[3:0]); 0 = code PD
    pub chdiv_seg: [u32; 3],
    /// R35[1], R35[7], R35[8]
    pub chdiv_seg_en: [bool; 3],
    /// R36[6:4]: 1 = seg 1, 2 = seg 1+2, 4 = seg 1+2+3
    pub chdiv_seg_sel: u32,
    /// R31[9], R31[10], R31[7]
    pub vco_dista_pd: bool,
    pub vco_distb_pd: bool,
    pub chdiv_dist_pd: bool,
    /// R36[10], R36[11]
    pub chdiv_dista_en: bool,
    pub chdiv_distb_en: bool,
    // outputs
    /// R46[6], R46[7]
    pub outa_pd: bool,
    pub outb_pd: bool,
    /// R46[13:8], R47[5:0] (0..31, 48..63 boost)
    pub outa_pow: u32,
    pub outb_pow: u32,
    /// R47[12:11], R48[1:0]: 0 = channel divider, 1 = VCO
    pub outa_mux: u32,
    pub outb_mux: u32,
    // phase detector / charge pump
    /// R13[14]
    pub cp_en: bool,
    /// R13[1:0]: 0 = dual PFD, 3 = single
    pub pfd_ctl: u32,
    /// R14[6:2], R14[11:7] (bit 4..0 = 1.25 / 2.5 / 0.625 / 0.312 / 0.156 mA)
    pub cp_iup: u32,
    pub cp_idn: u32,
    /// R14[1:0]: x1 / x2 / x1.5 / x2.5
    pub cp_icoarse: u32,
    // calibration and lock detect
    /// R0[13], R0[4], R0[3], R0[2]
    pub ld_en: bool,
    pub acal_en: bool,
    pub fcal_en: bool,
    pub muxout_sel: bool,
    /// R0[8:7], R0[6:5]
    pub fcal_hpfd_adj: u32,
    pub fcal_lpfd_adj: u32,
    /// R1[2:0]: calibration clock = OSCin / 2^n
    pub cal_clk_div: u32,
    /// R4[15:8]
    pub acal_cmp_dly: u32,
    /// R8[13], R8[10]
    pub vco_idac_ovr: bool,
    pub vco_capctrl_ovr: bool,
    /// R19[11:3], R20[8:0], R22[7:0]
    pub vco_idac: u32,
    pub acal_vco_idac_strt: u32,
    pub vco_capctrl: u32,
    /// R23[14], R23[13:11], R23[10]
    pub fcal_vco_sel_strt: bool,
    pub vco_sel: u32,
    pub vco_sel_force: bool,
    /// R30[7:6]
    pub vtune_adj: u32,
    /// R61[0]: 1 = Vtune detect
    pub ld_type: u32,
    /// R64[9], R64[8], R64[7:5], R64[3:0]
    pub acal_fast: bool,
    pub fcal_fast: bool,
    pub ajump_size: u32,
    pub fjump_size: u32,
}

impl Fields {
    /// Decode a table; every register named above must be in it.
    pub fn decode(t: &Table) -> Result<Fields, String> {
        let r = |a: u8| word(t, a).ok_or_else(|| format!("R{} missing", a));
        let (r0, r30, r31, r35, r36, r46, r47) = (r(0)?, r(30)?, r(31)?, r(35)?, r(36)?, r(46)?, r(47)?);
        let b = |v: u16, m, l| bits(v, m, l);
        let f = |v: u16, i| bits(v, i, i) != 0;
        let long = |hi: u8, lo: u8| -> Result<u32, String> { Ok((r(hi)? as u32) << 16 | r(lo)? as u32) };
        let (r14, r23, r64) = (r(14)?, r(23)?, r(64)?);
        Ok(Fields {
            osc_2x: f(r(9)?, 11),
            pll_r_pre: b(r(12)?, 11, 0),
            mult: b(r(10)?, 11, 7),
            pll_r: b(r(11)?, 11, 4),
            pll_n_pre: if f(r(37)?, 12) { 4 } else { 2 },
            pll_n: b(r(38)?, 12, 1),
            pll_num: long(44, 45)?,
            pll_den: long(40, 41)?,
            mash_order: b(r46, 2, 0),
            mash_dither: f(r30, 10),
            mash_seed: long(42, 43)?,
            pfd_dly: b(r(39)?, 13, 8),
            chdiv_en: f(r(34)?, 5),
            chdiv_seg: [if f(r35, 2) { 3 } else { 2 }, seg_div(b(r35, 12, 9)), seg_div(b(r36, 3, 0))],
            chdiv_seg_en: [f(r35, 1), f(r35, 7), f(r35, 8)],
            chdiv_seg_sel: b(r36, 6, 4),
            vco_dista_pd: f(r31, 9),
            vco_distb_pd: f(r31, 10),
            chdiv_dist_pd: f(r31, 7),
            chdiv_dista_en: f(r36, 10),
            chdiv_distb_en: f(r36, 11),
            outa_pd: f(r46, 6),
            outb_pd: f(r46, 7),
            outa_pow: b(r46, 13, 8),
            outb_pow: b(r47, 5, 0),
            outa_mux: b(r47, 12, 11),
            outb_mux: b(r(48)?, 1, 0),
            cp_en: f(r(13)?, 14),
            pfd_ctl: b(r(13)?, 1, 0),
            cp_iup: b(r14, 6, 2),
            cp_idn: b(r14, 11, 7),
            cp_icoarse: b(r14, 1, 0),
            ld_en: f(r0, 13),
            acal_en: f(r0, 4),
            fcal_en: f(r0, 3),
            muxout_sel: f(r0, 2),
            fcal_hpfd_adj: b(r0, 8, 7),
            fcal_lpfd_adj: b(r0, 6, 5),
            cal_clk_div: b(r(1)?, 2, 0),
            acal_cmp_dly: b(r(4)?, 15, 8),
            vco_idac_ovr: f(r(8)?, 13),
            vco_capctrl_ovr: f(r(8)?, 10),
            vco_idac: b(r(19)?, 11, 3),
            acal_vco_idac_strt: b(r(20)?, 8, 0),
            vco_capctrl: b(r(22)?, 7, 0),
            fcal_vco_sel_strt: f(r23, 14),
            vco_sel: b(r23, 13, 11),
            vco_sel_force: f(r23, 10),
            vtune_adj: b(r30, 7, 6),
            ld_type: b(r(61)?, 0, 0),
            acal_fast: f(r64, 9),
            fcal_fast: f(r64, 8),
            ajump_size: b(r64, 7, 5),
            fjump_size: b(r64, 3, 0),
        })
    }

    /// Total channel division in the path selected by CHDIV_SEG_SEL (None for PD / an invalid code).
    pub fn chdiv(&self) -> Option<u32> {
        let n = match self.chdiv_seg_sel {
            1 => 1,
            2 => 2,
            4 => 3,
            _ => return None,
        };
        let s = &self.chdiv_seg[..n];
        if s.contains(&0) {
            return None;
        }
        Some(s.iter().product())
    }

    /// f_PD as numerator / denominator in Hz, for OSCin `f_osc`.
    fn pd_ratio(&self, f_osc: u64) -> (u128, u128) {
        let num = f_osc as u128 * if self.osc_2x { 2 } else { 1 } * self.mult.max(1) as u128;
        (num, self.pll_r_pre.max(1) as u128 * self.pll_r.max(1) as u128)
    }

    /// f_VCO as numerator / denominator in Hz, for OSCin `f_osc`.
    fn vco_ratio(&self, f_osc: u64) -> (u128, u128) {
        let (pn, pd) = self.pd_ratio(f_osc);
        let den = self.pll_den.max(1) as u128;
        (pn * self.pll_n_pre as u128 * (self.pll_n as u128 * den + self.pll_num as u128), pd * den)
    }

    /// Phase detector frequency with the board's OSCin.
    pub fn f_pd_hz(&self) -> f64 {
        let (n, d) = self.pd_ratio(F_OSC_HZ);
        n as f64 / d as f64
    }

    /// VCO frequency with the board's OSCin.
    pub fn f_vco_hz(&self) -> f64 {
        let (n, d) = self.vco_ratio(F_OSC_HZ);
        n as f64 / d as f64
    }

    /// VCO frequency if it is a whole number of Hz (the exact rational, no float rounding).
    pub fn f_vco_exact(&self) -> Option<u64> {
        let (n, d) = self.vco_ratio(F_OSC_HZ);
        (n % d == 0).then(|| (n / d) as u64)
    }

    /// Frequency at output B (the ADC clock): VCO / CHDIV, or the VCO with OUTB_MUX = 1; None if B is off.
    pub fn f_out_b_exact(&self) -> Option<u64> {
        if self.outb_pd {
            return None;
        }
        let (n, d) = self.vco_ratio(F_OSC_HZ);
        let div = match self.outb_mux {
            0 if self.chdiv_en && self.chdiv_distb_en && !self.chdiv_dist_pd => self.chdiv()? as u128,
            1 if !self.vco_distb_pd => 1,
            _ => return None,
        };
        (n % (d * div) == 0).then(|| (n / (d * div)) as u64)
    }

    /// Output B frequency as a float (NaN if B carries nothing).
    pub fn f_out_b_hz(&self) -> f64 {
        match self.outb_mux {
            0 => self.chdiv().map_or(f64::NAN, |c| self.f_vco_hz() / c as f64),
            1 => self.f_vco_hz(),
            _ => f64::NAN,
        }
    }

    /// Where the table leaves the datasheet (SNAS680E) or the board's usual set-up; empty = all within limits.
    pub fn notes(&self) -> Vec<String> {
        let mut n = Vec::new();
        let vco = self.f_vco_hz();
        if !(VCO_MIN_HZ as f64..=VCO_MAX_HZ as f64).contains(&vco) {
            n.push(format!("VCO {:.6} MHz outside 3550..7100 MHz", vco / 1e6));
        }
        if self.outb_mux == 0 {
            match self.chdiv().and_then(|c| CHDIVS.iter().find(|d| d.total == c)) {
                Some(d) if d.total == 4 && vco > CHDIV4_DATASHEET_MAX_HZ as f64 && vco <= d.vco_max_hz as f64 => {
                    n.push(format!("CHDIV 4 at VCO {:.0} MHz: above the 4736 MHz of Table 7-4 (rev. E); Rigol's stock operating point", vco / 1e6))
                }
                Some(d) if vco < d.vco_min_hz as f64 || vco > d.vco_max_hz as f64 => n.push(format!(
                    "CHDIV {} needs VCO {}..{} MHz (Table 7-4), table has {:.3} MHz",
                    d.total,
                    d.vco_min_hz / 1_000_000,
                    d.vco_max_hz / 1_000_000,
                    vco / 1e6
                )),
                Some(_) => {}
                None => n.push("channel divider setting not in Table 7-4".into()),
            }
        }
        let order = self.mash_order.min(4) as usize;
        if self.pll_n < N_MIN[order] {
            n.push(format!("PLL_N {} below the minimum {} for MASH order {} (Table 7-2)", self.pll_n, N_MIN[order], self.mash_order));
        }
        if self.pfd_dly != PFD_DLY_REC[order] {
            n.push(format!("PFD_DLY {} differs from the recommended {} for MASH order {}", self.pfd_dly, PFD_DLY_REC[order], self.mash_order));
        }
        if self.pll_num >= self.pll_den.max(1) {
            n.push(format!("PLL_NUM {} >= PLL_DEN {}", self.pll_num, self.pll_den));
        }
        let want_vtune = if vco >= VTUNE_ADJ_FROM_HZ as f64 { 3 } else { 0 };
        if self.vtune_adj != want_vtune {
            n.push(format!("VTUNE_ADJ {} but {} for VCO {:.0} MHz (Table 7-25)", self.vtune_adj, want_vtune, vco / 1e6));
        }
        if !self.fcal_en {
            n.push("FCAL_EN = 0: R0 does not start the VCO calibration".into());
        }
        n
    }

    /// Human-readable decode, one field group per line (for `oos-fpgad pll --dry`).
    pub fn lines(&self) -> Vec<String> {
        let cp_ma = |c: u32| [0.156, 0.312, 0.625, 2.5, 1.25].iter().enumerate().filter(|(i, _)| c >> i & 1 != 0).map(|(_, m)| m).sum::<f64>();
        let coarse = [1.0, 2.0, 1.5, 2.5][self.cp_icoarse as usize & 3];
        let chdiv = self.chdiv().map_or("PD".into(), |c| c.to_string());
        let out = |hz: f64| if hz.is_nan() { "-".to_string() } else { format!("{:.6} MHz", hz / 1e6) };
        vec![
            format!(
                "reference  OSCin {} MHz (inferred), OSC_2X {}, PLL_R_PRE {}, MULT {}, PLL_R {} -> f_PD {} (FCAL_HPFD_ADJ {}, FCAL_LPFD_ADJ {})",
                F_OSC_HZ / 1_000_000,
                self.osc_2x as u8,
                self.pll_r_pre,
                self.mult,
                self.pll_r,
                out(self.f_pd_hz()),
                self.fcal_hpfd_adj,
                self.fcal_lpfd_adj
            ),
            format!(
                "N divider  PLL_N_PRE /{}, PLL_N {}, PLL_NUM {}, PLL_DEN {}, MASH order {} (dither {}, seed {}), PFD_DLY {}",
                self.pll_n_pre, self.pll_n, self.pll_num, self.pll_den, self.mash_order, self.mash_dither as u8, self.mash_seed, self.pfd_dly
            ),
            format!("VCO        {} (VTUNE_ADJ {})", out(self.f_vco_hz()), self.vtune_adj),
            format!(
                "CHDIV      /{} (SEG_SEL {}, SEG1 /{} en {}, SEG2 /{} en {}, SEG3 /{} en {}, CHDIV_EN {})",
                chdiv,
                self.chdiv_seg_sel,
                self.chdiv_seg[0],
                self.chdiv_seg_en[0] as u8,
                self.chdiv_seg[1],
                self.chdiv_seg_en[1] as u8,
                self.chdiv_seg[2],
                self.chdiv_seg_en[2] as u8,
                self.chdiv_en as u8
            ),
            format!(
                "outputs    B: {} MUX {} POW {} -> {}; A: {} MUX {} POW {} (VCO_DISTA_PD {}, VCO_DISTB_PD {}, CHDIV_DIST_PD {}, CHDIV_DISTA/B_EN {}/{})",
                if self.outb_pd { "off" } else { "on" },
                self.outb_mux,
                self.outb_pow,
                out(if self.outb_pd { f64::NAN } else { self.f_out_b_hz() }),
                if self.outa_pd { "off" } else { "on" },
                self.outa_mux,
                self.outa_pow,
                self.vco_dista_pd as u8,
                self.vco_distb_pd as u8,
                self.chdiv_dist_pd as u8,
                self.chdiv_dista_en as u8,
                self.chdiv_distb_en as u8
            ),
            format!(
                "charge pump CP_EN {}, PFD_CTL {}, CP_IUP {:#x} / CP_IDN {:#x} x{} = {:.3} mA",
                self.cp_en as u8,
                self.pfd_ctl,
                self.cp_iup,
                self.cp_idn,
                coarse,
                cp_ma(self.cp_iup) * coarse
            ),
            format!(
                "calibration FCAL_EN {}, ACAL_EN {}, CAL_CLK_DIV {}, ACAL_CMP_DLY {}, VCO_IDAC {} (ovr {}), IDAC_STRT {}, CAPCTRL {} (ovr {}), VCO_SEL {} (force {}, start {}), fast A/F {}/{}",
                self.fcal_en as u8,
                self.acal_en as u8,
                self.cal_clk_div,
                self.acal_cmp_dly,
                self.vco_idac,
                self.vco_idac_ovr as u8,
                self.acal_vco_idac_strt,
                self.vco_capctrl,
                self.vco_capctrl_ovr as u8,
                self.vco_sel,
                self.vco_sel_force as u8,
                self.fcal_vco_sel_strt as u8,
                self.acal_fast as u8,
                self.fcal_fast as u8
            ),
            format!("lock detect LD_EN {}, MUXOUT_SEL {}, LD_TYPE {} (no readback here: SDIO is write-only on this board, pll.md)", self.ld_en as u8, self.muxout_sel as u8, self.ld_type),
        ]
    }
}

/// A table as in pll.md: `R70=0000 R69=0000 ...`, ten words per line.
pub fn format_table(t: &Table) -> String {
    t.chunks(10).map(|c| c.iter().map(|w| format!("R{}={:02x}{:02x}", w[0], w[1], w[2])).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n")
}

/// Table for `f_out_hz` at output B: `PLL_TABLE` with only PLL_N, PLL_NUM, PLL_DEN, the channel divider segments
/// (R35, R36) and VTUNE_ADJ (R30) changed; R0 stays last so the VCO calibration runs with the new values.
///
/// Choice: every channel divider of `CHDIVS` whose VCO range (also 3.55..7.1 GHz) holds f_out x CHDIV; integer-N
/// first (fractional spurs land in the sample clock), then the VCO closest to Rigol's 5 GHz (where the board's loop
/// filter, charge pump and calibration are proven), then the smaller divider. The fraction is reduced; when its
/// denominator divides Rigol's PLL_DEN (4) that denominator is kept, so 1.25 GHz gives `PLL_TABLE` exactly. MASH
/// order 3 and PFD_DLY 2 stay (Rigol runs 3rd order even at NUM 0); PLL_N must be >= 18 (Table 7-2, 3rd order).
/// Refuses non-integral Hz, frequencies no divider reaches within the limits, and the VCO-direct range.
pub fn table_for(f_out_hz: f64) -> Result<Table, String> {
    if !f_out_hz.is_finite() || f_out_hz <= 0.0 {
        return Err(format!("{} Hz is not a frequency", f_out_hz));
    }
    if f_out_hz.fract() != 0.0 || f_out_hz > 1e12 {
        return Err(format!("{} Hz: only whole Hz up to the VCO range are made", f_out_hz));
    }
    let f = f_out_hz as u64;
    let base = Fields::decode(&PLL_TABLE)?;
    let (pn, pd) = base.pd_ratio(F_OSC_HZ);
    if pn % pd != 0 {
        return Err("reference path of PLL_TABLE gives a non-integral f_PD".into());
    }
    // VCO step per unit of N
    let step = (pn / pd) as u64 * base.pll_n_pre as u64;
    let base_vco = base.f_vco_exact().ok_or("PLL_TABLE VCO not integral")?;
    let order = base.mash_order.min(4) as usize;
    let mut best: Option<((bool, u64, u32), ChDiv, u32, u32, u32)> = None;
    for d in CHDIVS {
        let Some(vco) = f.checked_mul(d.total as u64) else { continue };
        if vco < VCO_MIN_HZ.max(d.vco_min_hz) || vco > VCO_MAX_HZ.min(d.vco_max_hz) {
            continue;
        }
        let n = (vco / step) as u32;
        let rem = vco % step;
        let g = gcd(rem, step);
        let (mut num, mut den) = (rem / g, step / g);
        if base.pll_den % den as u32 == 0 {
            num *= (base.pll_den / den as u32) as u64;
            den = base.pll_den as u64;
        }
        if n < N_MIN[order] || n > N_MAX || den > u32::MAX as u64 {
            continue;
        }
        let key = (rem != 0, vco.abs_diff(base_vco), d.total);
        if best.as_ref().map_or(true, |b| key < b.0) {
            best = Some((key, d, n, num as u32, den as u32));
        }
    }
    let Some((_, d, n, num, den)) = best else {
        return Err(format!(
            "{} Hz: no channel divider of Table 7-4 puts the VCO into its range (output via CHDIV: 20 MHz .. 3.55 GHz)",
            f
        ));
    };
    let mut t = PLL_TABLE;
    set(&mut t, 38, 12, 1, n);
    set(&mut t, 44, 15, 0, num >> 16);
    set(&mut t, 45, 15, 0, num & 0xffff);
    set(&mut t, 40, 15, 0, den >> 16);
    set(&mut t, 41, 15, 0, den & 0xffff);
    let segs = match d.seg {
        [_, 1, 1] => 1,
        [_, _, 1] => 2,
        _ => 3,
    };
    // segments outside the path: enable 0, divide code left at /2 like the TICS Pro export (SEG3 in PLL_TABLE)
    let code = |i: usize| if i < segs { seg_code(d.seg[i]) } else { 1 };
    set(&mut t, 35, 2, 2, (d.seg[0] == 3) as u32);
    set(&mut t, 35, 1, 1, 1);
    set(&mut t, 35, 12, 9, code(1));
    set(&mut t, 35, 7, 7, (segs >= 2) as u32);
    set(&mut t, 35, 8, 8, (segs >= 3) as u32);
    set(&mut t, 36, 3, 0, code(2));
    set(&mut t, 36, 6, 4, [0, 1, 2, 4][segs]);
    let vco = f * d.total as u64;
    set(&mut t, 30, 7, 6, if vco >= VTUNE_ADJ_FROM_HZ { 3 } else { 0 });
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_table_decode() {
        let f = Fields::decode(&PLL_TABLE).unwrap();
        assert_eq!((f.osc_2x, f.pll_r_pre, f.mult, f.pll_r), (false, 1, 1, 1));
        assert_eq!((f.pll_n_pre, f.pll_n, f.pll_num, f.pll_den, f.mash_order, f.pfd_dly), (2, 25, 0, 4, 3, 2));
        assert_eq!((f.chdiv(), f.chdiv_seg, f.chdiv_seg_en, f.chdiv_seg_sel), (Some(4), [2, 2, 2], [true, true, false], 2));
        assert_eq!((f.outb_pd, f.outb_mux, f.outb_pow, f.outa_pd), (false, 0, 31, true));
        assert_eq!((f.cp_iup, f.cp_idn, f.cp_icoarse, f.pfd_ctl), (8, 8, 0, 0));
        assert!(f.fcal_en && f.acal_en && !f.vco_capctrl_ovr && !f.vco_idac_ovr);
        assert_eq!((f.cal_clk_div, f.acal_cmp_dly, f.vco_idac, f.vtune_adj), (0, 25, 300, 0));
        assert_eq!(f.f_vco_exact(), Some(5_000_000_000));
        assert_eq!(f.f_out_b_exact(), Some(1_250_000_000));
        // the only remark: CHDIV 4 above the rev. E limit (Rigol's point)
        let n = f.notes();
        assert_eq!(n.len(), 1, "{:?}", n);
        assert!(n[0].contains("CHDIV 4"));
    }

    #[test]
    fn stock_frequency_reproduces_table() {
        assert_eq!(table_for(1.25e9).unwrap(), PLL_TABLE);
    }

    #[test]
    fn faster_clocks_exact() {
        // (f_out, CHDIV, VCO, N, NUM, DEN)
        let want: [(f64, u32, u64, u32, u32, u32); 5] = [
            (1.375e9, 3, 4_125_000_000, 20, 5, 8),
            (1.5e9, 3, 4_500_000_000, 22, 2, 4),
            (1.625e9, 3, 4_875_000_000, 24, 3, 8),
            (1.75e9, 3, 5_250_000_000, 26, 1, 4),
            (2.0e9, 2, 4_000_000_000, 20, 0, 4),
        ];
        for (fo, div, vco, n, num, den) in want {
            let t = table_for(fo).unwrap();
            let f = Fields::decode(&t).unwrap();
            assert_eq!(f.f_vco_exact(), Some(vco), "{}", fo);
            assert_eq!(f.f_out_b_exact(), Some(fo as u64), "{}", fo);
            assert_eq!((f.chdiv(), f.pll_n, f.pll_num, f.pll_den), (Some(div), n, num, den), "{}", fo);
            assert!(f.notes().is_empty(), "{}: {:?}", fo, f.notes());
            // nothing else than N/NUM/DEN, CHDIV and VTUNE_ADJ moved
            for (a, b) in t.iter().zip(PLL_TABLE.iter()) {
                assert_eq!(a[0], b[0]);
                if a != b {
                    assert!([30, 35, 36, 38, 40, 41, 44, 45].contains(&a[0]), "{}: R{} changed", fo, a[0]);
                }
            }
        }
    }

    #[test]
    fn chdiv_encoding() {
        // /2: SEG1 only, the other segments powered down
        let f = Fields::decode(&table_for(2.0e9).unwrap()).unwrap();
        assert_eq!((f.chdiv_seg_sel, f.chdiv_seg_en, f.chdiv_seg[0]), (1, [true, false, false], 2));
        // /3: SEG1 = 3
        let f = Fields::decode(&table_for(1.5e9).unwrap()).unwrap();
        assert_eq!((f.chdiv_seg_sel, f.chdiv_seg_en, f.chdiv_seg[0]), (1, [true, false, false], 3));
        // VCO >= 6.5 GHz: VTUNE_ADJ 3
        let f = Fields::decode(&table_for(3.3e9).unwrap()).unwrap();
        assert_eq!((f.chdiv(), f.f_vco_exact(), f.vtune_adj), (Some(2), Some(6_600_000_000), 3));
        // three segments (/192)
        let f = Fields::decode(&table_for(25e6).unwrap()).unwrap();
        assert_eq!(f.f_out_b_exact(), Some(25_000_000));
        assert!(f.notes().is_empty(), "{:?}", f.notes());
    }

    #[test]
    fn refused() {
        for f in [0.0, -1.0, f64::NAN, f64::INFINITY, 1.25e9 + 0.5, 3.6e9, 5e9, 7e9, 10e6] {
            assert!(table_for(f).is_err(), "{}", f);
        }
        // /4 is admitted up to the stock 5 GHz VCO: 1.2 GHz (4800 MHz) stays on /4, 1.26 GHz (5040 MHz) moves to /3
        assert_eq!(Fields::decode(&table_for(1.2e9).unwrap()).unwrap().chdiv(), Some(4));
        assert_eq!(Fields::decode(&table_for(1.26e9).unwrap()).unwrap().chdiv(), Some(3));
    }

    #[test]
    fn table_text() {
        let s = format_table(&PLL_TABLE);
        assert!(s.starts_with("R70=0000 R69=0000 R68=0089 R64=0077"));
        assert!(s.ends_with("R4=1943 R2=0500 R1=0808 R0=221c"));
        assert_eq!(s.lines().count(), 5);
    }
}
