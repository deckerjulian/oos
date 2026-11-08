// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Cold-boot write lists that do not depend on reads: pure functions over the boot shadows. Register-level recipes of
//! docs/protocols/cold-boot.md section 3; the parts already run on the unit are ports of `tools/hw/oos-hw.c`.
//!
//! Tags as in `boot`: [N] not run under OOS yet, [U] run on the unit under OOS, [O] observed on the unit (agrees with
//! its live data), [A] assumed, or an OOS choice.

use super::Shadows;
use crate::dho::adc::Wr;
use crate::dho::Write;

/// FPGA ADC SPI master: 0x1034 = `0x01000000 | reg << 16 | data` (bit 23 = read, data in 0x1038 after 100 us).
pub const ADC_SPI: u32 = 0x1034;
pub const ADC_RDATA: u32 = 0x1038;

fn adc(pre_us: u32, val: u32, post_us: u32) -> Wr {
    Wr { pre_us, reg: ADC_SPI, val, post_us }
}

/// The 104 words of the ADC init (word = `0x1000000 | addr << 16 | (mask[addr] ^ init) & 0xffff`), as in
/// `oos-hw adc-init` [U].
pub const ADC_INIT: [u32; 104] = [
    0x01000005, 0x01010000, 0x01020000, 0x01030000, 0x01040080, 0x01058080, 0x010601c3, 0x01070000, 0x01080000, 0x01097700,
    0x010a8078, 0x010b0000, 0x010c3fff, 0x010d520f, 0x010e0078, 0x010f0201, 0x01100000, 0x01110000, 0x01120000, 0x01130000,
    0x01140000, 0x01150000, 0x01160044, 0x01170000, 0x01180000, 0x01190000, 0x011a0000, 0x011b0000, 0x011c0000, 0x011d0000,
    0x011e0044, 0x011f0000, 0x01200000, 0x01210000, 0x01220000, 0x01230000, 0x01240000, 0x01250000, 0x01260044, 0x01270000,
    0x01280000, 0x01290000, 0x012a0000, 0x012b0000, 0x012c0000, 0x012d0000, 0x012e0044, 0x012f0000, 0x01300000, 0x01310000,
    0x01320000, 0x01330000, 0x01340000, 0x01350000, 0x01360044, 0x01370000, 0x01380000, 0x01390000, 0x013a0000, 0x013b0000,
    0x013c0000, 0x013d0000, 0x013e0044, 0x013f0000, 0x01400000, 0x01410000, 0x01420000, 0x01430000, 0x01440000, 0x01450000,
    0x01460044, 0x01470000, 0x01480000, 0x01490000, 0x014a0000, 0x014b0000, 0x014c0000, 0x014d0000, 0x014e0044, 0x014f0000,
    0x01500810, 0x01510010, 0x01520010, 0x01530010, 0x01549460, 0x01550004, 0x01560007, 0x01570080, 0x01580000, 0x01590000,
    0x015a0000, 0x015b07de, 0x015c0000, 0x015d0000, 0x015e0000, 0x015f0800, 0x01600008, 0x01616dff, 0x01620060, 0x01630000,
    0x01640000, 0x01650000, 0x01660001, 0x01671402,
];
/// ADC reg 0x0f once more after the two passes (shadow 0x7a01 & ~0xc00 ^ 0x7000) [U].
pub const ADC_INIT_REG0F: u32 = 0x010f_0201;
/// Pre-write sleep of the ADC init: stock 1.04 1.1 ms (the timing run on the unit [U]); 100 us also works [N].
pub const ADC_INIT_PRE_US_STOCK: u32 = 1100;
pub const ADC_INIT_PRE_US_SHORT: u32 = 100;

/// ADC init: the 104 words twice, then reg 0x0f; `pre_us` before and 100 us after each word.
/// Resets the ADC shadows the boot keeps to their start values. Words [N][U], timing see the constants.
pub fn adc_init(sh: &mut Shadows, pre_us: u32) -> Vec<Wr> {
    sh.adc00 = 0xe4;
    sh.adc06 = 0x01c3;
    let mut w: Vec<Wr> = (0..2).flat_map(|_| ADC_INIT.iter().map(|&v| adc(pre_us, v, 100))).collect();
    w.push(adc(100, ADC_INIT_REG0F, 100));
    w
}

/// SPU init: 0x1000 1, 0, 0, 0x10, 0; 0x105c = 0x55555555; 0x1010 |= 0x20000000; 0x1014 = (s & 0xff0fffff) |
/// 0xf << 20 | 1 = 0x00f00001 from the start shadow 0 [N]. (`oos-hw spu-init` wrote the final 0x14f01301 instead.)
pub fn spu_init(sh: &mut Shadows) -> Vec<Write> {
    let mut w = Vec::new();
    for v in [0x1, 0x0, 0x0, 0x10, 0x0] {
        sh.s1000 = v;
        w.push((0x1000, v));
    }
    w.push((0x105c, 0x5555_5555));
    sh.s1010 |= 0x2000_0000;
    w.push((0x1010, sh.s1010));
    sh.s1014 = (sh.s1014 & 0xff0f_ffff) | 0xf << 20 | 1;
    w.push((0x1014, sh.s1014));
    w
}

/// SCU init: 0x4004 = s | bit 31 (stop), then s & 0x7ffffffb [N][U].
pub fn scu_init(sh: &mut Shadows) -> Vec<Write> {
    sh.s4004 |= 0x8000_0000;
    let a = (0x4004, sh.s4004);
    sh.s4004 &= 0x7fff_fffb;
    vec![a, (0x4004, sh.s4004)]
}

/// DAC polling (5e9, 5.5e9): 0x1428 low half = 100 << 8 | 110 = 0x646e [N][O].
pub fn dac_polling(sh: &mut Shadows) -> Write {
    sh.s1428 = (sh.s1428 & 0xffff_0000) | 0x646e;
    (0x1428, sh.s1428)
}

/// Memory reset off: 0x1000 = s & ~0xc [N].
pub fn mem_reset_off(sh: &mut Shadows) -> Write {
    sh.s1000 &= !0xc;
    (0x1000, sh.s1000)
}

/// SPU sync: 0x1000 bit 1 pulse (three writes) [N].
pub fn spu_adc_sync(sh: &mut Shadows) -> Vec<Write> {
    let s = sh.s1000;
    sh.s1000 = s & !2;
    vec![(0x1000, s & !2), (0x1000, s | 2), (0x1000, s & !2)]
}

/// SPU reset: 0x1000 bit 0 pulse [N].
pub fn spu_reset(sh: &mut Shadows) -> Vec<Write> {
    let s = sh.s1000;
    sh.s1000 = s & !1;
    vec![(0x1000, s | 1), (0x1000, s & !1)]
}

/// MIG calibration results, read for the log only: (register, mask, name) [N].
pub const MIG_TAPS: [(u32, u32, &str); 5] = [
    (0x14a0, 0x3f, "wrlvl_coarse"),
    (0x14a4, 0xfff, "mpr_cpt"),
    (0x1498, 0xfff, "wrlvl_cpt"),
    (0x14ac, 0xfff, "prbs_final_dqs"),
    (0x14b4, 0x3ff, "dq_idelay"),
];

/// DDR skip taps: 0x1000 |= 8, the six skip taps of cal_ddr.hex, 0x1000 &= ~8 [N]. The tap
/// registers are 16-bit RMW of shadows that start at 0, so the plain masked values are written.
pub fn ddr_taps(sh: &mut Shadows, t: &[u32; 6]) -> Vec<Write> {
    const REGS: [(u32, u32); 6] = [(0x1470, 0x3f), (0x1474, 0xfff), (0x147c, 0xfff), (0x1484, 0xfff), (0x148c, 0x3ff), (0x1494, 0xfff)];
    sh.s1000 |= 0x8;
    let mut w = vec![(0x1000, sh.s1000)];
    w.extend(REGS.iter().zip(t).map(|(&(r, m), &v)| (r, v & m)));
    sh.s1000 &= !0x8;
    w.push((0x1000, sh.s1000));
    w
}

/// AFE offset words (0x8000), ch 0..3: 0x1418, 0x141c, 0x1420, 0x1424 [N] (meaning unknown [A]).
pub fn afe_offset() -> Vec<Write> {
    (0..4).map(|c| (0x1418 + 4 * c, 0x8000)).collect()
}

/// ADC bit range: 0x1014 = (s & 0xff0fffff) | 0xf << 20 | 1 [N] (channel map 0xf [A/O]).
pub fn bit_range(sh: &mut Shadows) -> Write {
    sh.s1014 = (sh.s1014 & 0xff0f_ffff) | 0xf << 20 | 1;
    (0x1014, sh.s1014)
}

/// SCU enables (LA off, LA processing off, channel processing on): 0x4008 bit 26 = 0, bit 24 = 0, bit 23 = 1 [N].
pub fn proc_enables(sh: &mut Shadows) -> Vec<Write> {
    let mut w = Vec::new();
    sh.s4008 &= !(1 << 26);
    w.push((0x4008, sh.s4008));
    sh.s4008 &= !(1 << 24);
    w.push((0x4008, sh.s4008));
    sh.s4008 |= 1 << 23;
    w.push((0x4008, sh.s4008));
    w
}

/// ADC Gray decoding: 0x1014 bit 3 [N] (only `off` is used at boot).
pub fn adc_gray(sh: &mut Shadows, on: bool) -> Write {
    sh.s1014 = (sh.s1014 & !0x8) | (on as u32) << 3;
    (0x1014, sh.s1014)
}

/// ADC calibration select: 0x1014 bits 19:12 = sel [N].
pub fn cal_select(sh: &mut Shadows, sel: u32) -> Write {
    sh.s1014 = (sh.s1014 & 0xfff0_0fff) | (sel & 0xff) << 12;
    (0x1014, sh.s1014)
}

/// IDELAY calibration enable: 0x1014 bit 2 pulse [N].
pub fn idelay_enable(sh: &mut Shadows) -> Vec<Write> {
    let s = sh.s1014;
    sh.s1014 = s & !4;
    vec![(0x1014, s & !4), (0x1014, s | 4), (0x1014, s & !4)]
}

/// Lane delay (`all`, `lane`, `d`), register 0x1018: `d & 0x1ff`, `d | mask`, `(d | mask) & 0xfff`; mask =
/// 0x3ffff000 for all lanes, `0x1000 << lane | 0x10000000` for lanes 0-13, `0x1000 << (lane - 14) | 0x20000000` for
/// 14-27 [N][U].
pub fn idelay(lane: Option<usize>, d: u32) -> [Write; 3] {
    let d = d & 0x1ff;
    let m = match lane {
        None => 0x3fff_f000,
        Some(l) if l < 14 => (0x1000 << l) | 0x1000_0000,
        Some(l) => (0x1000 << (l - 14)) | 0x2000_0000,
    };
    [(0x1018, d), (0x1018, d | m), (0x1018, (d | m) & 0xfff)]
}

/// Load the lane delays: select 1, then the three 0x1018 writes per lane [N][U].
pub fn idelay_load(sh: &mut Shadows, delays: &[u32; 28]) -> Vec<Write> {
    let mut w = vec![cal_select(sh, 1)];
    for (lane, &d) in delays.iter().enumerate() {
        w.extend(idelay(Some(lane), d));
    }
    w
}

/// Bit-error flags: lanes 0-13 from 0x1024, 14-27 from 0x1028 (an addition: `(a & 0xffff) + ((b & 0xffff) << 14)`) [N].
pub fn ber_err(a: u32, b: u32) -> u32 {
    (a & 0xffff).wrapping_add((b & 0xffff) << 14)
}

/// The bit-error counts (for the log only): `(a >> 16) + ((b >> 2) & 0x3fffc000)` [N].
pub fn ber_count(a: u32, b: u32) -> u32 {
    (a >> 16).wrapping_add((b >> 2) & 0x3fff_c000)
}

/// ADC sync delay: ADC reg 0x06 = (s & ~0x700) | d << 8, written without XOR mask [N][O].
pub fn sync_delay(sh: &mut Shadows, d: u32) -> Wr {
    sh.adc06 = (sh.adc06 & !0x700) | ((d & 7) as u16) << 8;
    adc(100, 0x0106_0000 | sh.adc06 as u32, 100)
}

/// ADC SPI sync: ADC reg 0x00 bits 7:6 = 10 (on) / 00 (off), data = shadow ^ 0xe1:
/// `0x01000045` / `0x010000c5` [N][O].
pub fn spi_sync(sh: &mut Shadows, on: bool) -> Wr {
    sh.adc00 = (sh.adc00 & !0xc0) | if on { 0x80 } else { 0 };
    adc(100, 0x0100_0000 | (sh.adc00 ^ 0xe1) as u32, 100)
}

/// ADC reg 0x0d test pattern [N].
pub const DATA_MODE_TEST: u32 = 0x010d_550f;
/// ADC reg 0x0d normal data (also the init value) [N].
pub const DATA_MODE_NORMAL: u32 = 0x010d_520f;

pub fn data_mode(test: bool) -> Wr {
    adc(100, if test { DATA_MODE_TEST } else { DATA_MODE_NORMAL }, 100)
}

/// ADC output code: reg 0x16 + 8n = 0x0044 (bit 15 = 0), n = 0..7; 100 us before each, the
/// sleep after only for the last word [N].
pub fn output_code_zero() -> Vec<Wr> {
    (0..8).map(|n| adc(100, 0x0100_0000 | (0x16 + 8 * n) << 16 | 0x0044, if n == 7 { 100 } else { 0 })).collect()
}

/// ADC register read request: 0x1034 = `0x01800000 | reg << 16`, no sleep before, 100 us,
/// then 0x1038 [N].
pub fn read_request(reg: u32) -> u32 {
    0x0180_0000 | (reg & 0x7f) << 16
}

/// ADC internal trim, core `n`: reg 0x13 + 8n, data 0x4000 / 0 (shadow 0x1b9b, bit 14, ^ 0x1b9b); sleeps
/// 200 us before, 800 us after [N].
pub fn qi_trim(core: u32, on: bool) -> Wr {
    adc(200, 0x0100_0000 | (0x13 + 8 * core) << 16 | if on { 0x4000 } else { 0 }, 800)
}

/// LMX2582 1.25 GHz table of Rigol's `spi2pll_lxm2582` (TICSPro export), highest register first, R0 (FCAL_EN) last;
/// `[addr, data hi, data lo]` [U] (pll.md, checked byte for byte against the stock tool).
pub const PLL_TABLE: [[u8; 3]; 46] = [
    [70, 0x00, 0x00], [69, 0x00, 0x00], [68, 0x00, 0x89], [64, 0x00, 0x77], [62, 0x00, 0x00], [61, 0x00, 0x01], [59, 0x00, 0x00],
    [48, 0x03, 0xfc], [47, 0x00, 0xdf], [46, 0x0f, 0x63], [45, 0x00, 0x00], [44, 0x00, 0x00], [43, 0x00, 0x00], [42, 0x00, 0x00],
    [41, 0x00, 0x04], [40, 0x00, 0x00], [39, 0x82, 0x04], [38, 0x00, 0x32], [37, 0x40, 0x00], [36, 0x0c, 0x21], [35, 0x02, 0x9b],
    [34, 0xc3, 0xea], [33, 0x2a, 0x0a], [32, 0x21, 0x0a], [31, 0x06, 0x01], [30, 0x00, 0x34], [29, 0x00, 0x84], [28, 0x29, 0x24],
    [25, 0x00, 0x00], [24, 0x05, 0x09], [23, 0x88, 0x42], [22, 0x23, 0x00], [20, 0x01, 0x2c], [19, 0x09, 0x65], [14, 0x04, 0x20],
    [13, 0x40, 0x00], [12, 0x70, 0x01], [11, 0x00, 0x18], [10, 0x10, 0xd8], [9, 0x03, 0x02], [8, 0x10, 0x84], [7, 0x28, 0xb2],
    [4, 0x19, 0x43], [2, 0x05, 0x00], [1, 0x08, 0x08], [0, 0x22, 0x1c],
];
/// Pause after each LMX2582 word (Rigol's tool: 50 ms) [U].
pub const PLL_WORD_GAP_US: u64 = 50_000;
/// The same in `early_start`.
pub const PLL_EARLY_GAP_US: u64 = 1_000;

/// The three LMX2582 bus lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PllPin {
    Le,
    Sclk,
    Sdio,
}

/// Line changes for one 24-bit LMX2582 word (protocol of `spi2pll_lxm2582_gpio.ko`, as `oos-hw pll-init` [U]): LE low;
/// per byte SCLK low, then per bit MSB first SDIO = bit, SCLK low, SCLK high (data taken on the rising edge); SCLK low
/// after the byte; then LE high, LE low (latch). R/W bit (bit 7 of the first byte) = 0 = write.
pub fn pll_edges(w: [u8; 3]) -> Vec<(PllPin, bool)> {
    let mut e = vec![(PllPin::Le, false)];
    for (b, &byte) in w.iter().enumerate() {
        let v = if b == 0 { byte & 0x7f } else { byte };
        e.push((PllPin::Sclk, false));
        for i in (0..8).rev() {
            e.push((PllPin::Sdio, (v >> i) & 1 != 0));
            e.push((PllPin::Sclk, false));
            e.push((PllPin::Sclk, true));
        }
        e.push((PllPin::Sclk, false));
    }
    e.push((PllPin::Le, true));
    e.push((PllPin::Le, false));
    e
}

/// FPGA image slots in the QSPI flash the boot monitor can start (fpga-boot.md).
/// Slot 0x800000 is deliberately not allowed: the factory DDR calibration flashes a self-test image there (cold-boot.md,
/// never-do list), its content and behaviour are unknown.
pub const FPGA_SLOTS: [u32; 2] = [0x00_0000, 0x40_0000];
/// Slot `start_rigol_app.sh` boots.
pub const FPGA_SLOT_APP: u32 = 0x40_0000;

/// The only boot-monitor command OOS ever sends: `\rBOOT\r` + slot address big endian (`spi2boot`) [U]. Returns None
/// for an address that is not one of the allowed image slots (0x0, 0x400000). No other monitor command (PROG, flash streams) exists here.
pub fn boot_message(addr: u32) -> Option<[u8; 10]> {
    if !FPGA_SLOTS.contains(&addr) {
        return None;
    }
    let a = addr.to_be_bytes();
    Some([b'\r', b'B', b'O', b'O', b'T', b'\r', a[0], a[1], a[2], a[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adc_init_list_like_oos_hw() {
        let mut sh = Shadows::default();
        let w = adc_init(&mut sh, ADC_INIT_PRE_US_STOCK);
        assert_eq!(w.len(), 209);
        assert_eq!((w[0].val, w[0].pre_us, w[0].post_us), (0x0100_0005, 1100, 100));
        assert_eq!(w[103].val, 0x0167_1402);
        assert_eq!(w[104].val, 0x0100_0005);
        assert_eq!(w[208].val, 0x010f_0201);
        // the input mode words of mode 0 and the normal data mode are part of the init list
        assert!(ADC_INIT.contains(&0x0154_9460) && ADC_INIT.contains(&0x0155_0004) && ADC_INIT.contains(&DATA_MODE_NORMAL));
        assert_eq!((sh.adc00, sh.adc06), (0xe4, 0x01c3));
    }

    #[test]
    fn spu_scu_once_only() {
        let mut sh = Shadows::default();
        let w = spu_init(&mut sh);
        assert_eq!(w, [(0x1000, 1), (0x1000, 0), (0x1000, 0), (0x1000, 0x10), (0x1000, 0), (0x105c, 0x5555_5555), (0x1010, 0x2000_0000), (0x1014, 0x00f0_0001)]);
        assert_eq!(scu_init(&mut sh), [(0x4004, 0x8000_0000), (0x4004, 0)]);
        assert_eq!(dac_polling(&mut sh), (0x1428, 0x646e));
        assert_eq!(mem_reset_off(&mut sh), (0x1000, 0));
        assert_eq!(afe_offset(), [(0x1418, 0x8000), (0x141c, 0x8000), (0x1420, 0x8000), (0x1424, 0x8000)]);
        assert_eq!(proc_enables(&mut sh), [(0x4008, 0), (0x4008, 0), (0x4008, 0x80_0000)]);
        assert_eq!(spu_adc_sync(&mut sh), [(0x1000, 0), (0x1000, 2), (0x1000, 0)]);
        assert_eq!(spu_reset(&mut sh), [(0x1000, 1), (0x1000, 0)]);
        assert_eq!(sh.s1000, 0);
    }

    #[test]
    fn ddr_taps_like_oos_hw() {
        // oos-hw ddr-cal (this unit's cal_ddr.hex hard-coded)
        let mut sh = Shadows::default();
        let w = ddr_taps(&mut sh, &[0x12, 0x4d6, 0x8a2, 0x924, 0x1ef, 0]);
        assert_eq!(w, [(0x1000, 8), (0x1470, 0x12), (0x1474, 0x4d6), (0x147c, 0x8a2), (0x1484, 0x924), (0x148c, 0x1ef), (0x1494, 0), (0x1000, 0)]);
    }

    #[test]
    fn adc_words() {
        let mut sh = Shadows { adc00: 0xe4, adc06: 0x01c3, ..Default::default() };
        let d: Vec<u32> = (0..8).map(|d| sync_delay(&mut sh, d).val).collect();
        assert_eq!(d[0], 0x0106_00c3);
        assert_eq!(d[7], 0x0106_07c3);
        assert_eq!(spi_sync(&mut sh, true).val, 0x0100_0045);
        assert_eq!(spi_sync(&mut sh, false).val, 0x0100_00c5);
        assert_eq!(sh.adc00, 0x24); // live shadow [O]
        let oc: Vec<u32> = output_code_zero().iter().map(|w| w.val).collect();
        assert_eq!(oc, [0x01160044, 0x011e0044, 0x01260044, 0x012e0044, 0x01360044, 0x013e0044, 0x01460044, 0x014e0044]);
        let q: Vec<u32> = (0..8).map(|n| qi_trim(n, true).val).collect();
        assert_eq!(q, [0x01134000, 0x011b4000, 0x01234000, 0x012b4000, 0x01334000, 0x013b4000, 0x01434000, 0x014b4000]);
        assert_eq!(read_request(7), 0x0187_0000);
        assert_eq!(read_request(0x16 + 8 * 7), 0x01ce_0000);
    }

    #[test]
    fn idelay_words_like_oos_hw() {
        assert_eq!(idelay(None, 5), [(0x1018, 5), (0x1018, 0x3fff_f005), (0x1018, 5)]);
        assert_eq!(idelay(Some(0), 12), [(0x1018, 12), (0x1018, 0x1000_100c), (0x1018, 12)]);
        assert_eq!(idelay(Some(13), 13)[1], (0x1018, 0x1200_000d));
        assert_eq!(idelay(Some(14), 12)[1], (0x1018, 0x2000_100c));
        assert_eq!(idelay(Some(27), 13)[1], (0x1018, 0x2200_000d));
        assert_eq!(ber_err(0x0000_3fff, 0x0000_3fff), 0x0fff_ffff);
    }

    #[test]
    fn pll_bits_and_table() {
        assert_eq!(PLL_TABLE.len(), 46);
        assert_eq!(PLL_TABLE[45], [0, 0x22, 0x1c]);
        let e = pll_edges([0x80 | 70, 0x12, 0x34]);
        assert_eq!(e.len(), 1 + 3 * (2 + 8 * 3) + 2);
        // sample SDIO on every rising SCLK edge
        let (mut sdio, mut bits) = (false, 0u32);
        for &(p, v) in &e {
            match p {
                PllPin::Sdio => sdio = v,
                PllPin::Sclk if v => bits = bits << 1 | sdio as u32,
                _ => {}
            }
        }
        assert_eq!(bits, 70 << 16 | 0x1234); // R/W bit forced to 0
    }

    #[test]
    fn only_boot_to_known_slots() {
        assert_eq!(boot_message(0x40_0000).unwrap(), [13, b'B', b'O', b'O', b'T', 13, 0, 0x40, 0, 0]);
        assert!(boot_message(0x12_3456).is_none());
    }
}
