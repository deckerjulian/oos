// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The MHO900's acquisition board as far as it is described (docs/protocols/mho-acquisition.md): the devices, the
//! register blocks, the ADC's SPI bridge, the FPGA image per hardware code and the read-out format.
//!
//! **Not yet verified on hardware.** Everything here is known from how the stock MHO900 system (00.01.00.00.26) starts
//! the board and uses its FPGA, compared with the DHO800/DHO900. Nothing in OOS calls it on a unit: the board has
//! `acquisition: false` (`target::MHO`, ADR 0028), so no step built here is ever executed. It is kept as data and pure
//! functions with tests, so the bring-up at the bench starts from it.

/// The register window: the XDMA bypass device, mapped whole. The stock system maps 16 MiB; the blocks below reach
/// 0x30000.
pub const BAR_DEVICE: &str = "/dev/xdma0_bypass";
/// Bytes the stock system maps of `BAR_DEVICE`.
pub const BAR_LEN: usize = 16 << 20;
/// The read-out stream (card to host), read with a plain blocking `read()` as on the DHO800/DHO900.
pub const C2H_DEVICE: &str = "/dev/xdma0_c2h_0";

/// A register block of the FPGA: (first offset, last offset, name). The offsets are the ones the stock system uses;
/// a block may hold more registers than listed.
pub type Block = (u32, u32, &'static str);

/// The register blocks of the MHO900's FPGA, in address order.
pub const BLOCKS: [Block; 12] = [
    (0x0004, 0x0004, "version"),
    (0x1000, 0x1574, "SPU: ADC lanes, sample rate, memory, filters, interpolation, read-out window, LA, probes"),
    (0x2000, 0x2274, "TPU: trigger, serial triggers, auto trigger"),
    (0x2438, 0x2438, "WAU: mask control"),
    (0x3000, 0x3004, "ADC SPI bridge (DHO800/DHO900: the generator)"),
    (0x4000, 0x4104, "SCU: run, record and play, status, time stamps, stop reason"),
    (0x5000, 0x5040, "Bode analysis"),
    (0x6058, 0x6064, "WAU: Bode source"),
    (0x610c, 0x6158, "WAU: mask and pass/fail (DHO800/DHO900: 0x1184..0x11cc)"),
    (0x10100, 0x10108, "peripheral PWM"),
    (0x20004, 0x20818, "JESD204 receiver of the two ADCs"),
    (0x30000, 0x30000, "multi-channel sync"),
];

/// SCU registers the run and read-out sequences use: the same offsets and bits as on the DHO800/DHO900
/// (acquisition-start.md, record-play.md).
pub mod scu {
    /// bit 0 FSM start, bit 2 play-last pulse, bit 31 stop.
    pub const CTRL: u32 = 0x4004;
    /// bits 3:0 the FSM's channel mask, bit 17 roll, bit 22 scan, bits 23/24 analog/LA stream select.
    pub const MODE: u32 = 0x4008;
    /// record and play mode bits (bits 25, 28, 29 in the record sequence).
    pub const RECORD: u32 = 0x400c;
    /// frame window of a record or play: first, current, last.
    pub const WINDOW: [u32; 3] = [0x4010, 0x4014, 0x4018];
    /// trigger and done status (read; written to clear).
    pub const STATUS: u32 = 0x4034;
    /// a second status word, read together with `STATUS`.
    pub const STATUS2: u32 = 0x4030;
    /// the record sequence's wait after each frame and its second word, written as on the DHO800/DHO900.
    pub const RECORD_EXTRA: [u32; 2] = [0x4068, 0x406c];
    /// why the last acquisition stopped (read, written to clear).
    pub const STOP_REASON: u32 = 0x4100;
}

/// The ADC's SPI bridge: one word per transfer into `ADC_SPI`, written with the strobe and again without it.
pub const ADC_SPI: u32 = 0x3000;
/// Where a read transfer's 16 data bits come back.
pub const ADC_SPI_READ: u32 = 0x3004;
/// The wait after each of the two writes of a transfer, in microseconds.
pub const ADC_SPI_WAIT_US: u64 = 100;
const ADC_STROBE: u32 = 1 << 26;
const ADC_READ: u32 = 1 << 23;

/// The two words of one ADC register write: data in bits 15:0, register address in bits 22:16, the ADC select in bit
/// 24 (ADC 0) or 25 (ADC 1), the strobe in bit 26 (first word only). The stock system XORs the data with a fixed mask
/// per register before it writes it; that mask is the caller's. `None` for an ADC other than 0 or 1, or an address
/// that does not fit 7 bits.
pub fn adc_write(adc: u32, addr: u32, data: u16) -> Option<[(u32, u32); 2]> {
    let w = adc_select(adc)? | adc_addr(addr)? << 16 | data as u32;
    Some([(ADC_SPI, w | ADC_STROBE), (ADC_SPI, w)])
}

/// The two words of one ADC register read (bit 23 set); the data is read from `ADC_SPI_READ` after the second.
pub fn adc_read(adc: u32, addr: u32) -> Option<[(u32, u32); 2]> {
    let w = adc_select(adc)? | adc_addr(addr)? << 16 | ADC_READ;
    Some([(ADC_SPI, w | ADC_STROBE), (ADC_SPI, w)])
}

fn adc_select(adc: u32) -> Option<u32> {
    match adc {
        0 => Some(1 << 24),
        1 => Some(1 << 25),
        _ => None,
    }
}

fn adc_addr(addr: u32) -> Option<u32> {
    (addr < 0x80).then_some(addr)
}

/// The FPGA image the stock system loads for a hardware code (the code its `hdcode` driver reports), as a file name
/// under `/rigol/FPGA` of the unit. OOS ships no image (ADR 0013): a loader would read the unit's own file.
pub fn fpga_image(hw_code: u32) -> Option<&'static str> {
    match hw_code {
        1 => Some("sparrow3_K160T.bit"),
        2..=7 => Some("sparrow3_RT688.bit"),
        8..=11 => Some("sparrow3_K160T_LR.bit"),
        _ => None,
    }
}

/// Power and detect lines of the acquisition board the stock start script drives through sysfs, in its order:
/// (GPIO number, name, value written). The LA probe detect is an input. None of them is named in OOS's device tree.
pub const POWER_LINES: [(u32, &str, Option<u8>); 4] = [
    (1, "GPIO0_A1 ADC supply enable", Some(1)),
    (2, "GPIO0_A2 front-end supply enable", Some(1)),
    (32, "GPIO1_A0 FPGA supply enable", Some(1)),
    (120, "GPIO3_D0 LA probe detect", None),
];

/// Read-out words: 16 bits little endian, 7500 codes per division around 32768, the same as on the DHO800/DHO900. The
/// channels of a read-out share the words as there: one channel alone, two on even and odd words, three or four in
/// groups of four (horizontal-config.md 2.2).
pub const CODES_PER_DIV: f64 = 7500.0;
/// The read-out code of the screen centre (0 V at zero offset).
pub const CODE_CENTRE: u16 = 32768;

/// Volts of read-out word `x` at `scale` V/div and `offset` V: the stock system's vertical increment (V/div / 7500)
/// and ground (32768 plus the offset in codes).
pub fn volts(x: u16, scale: f64, offset: f64) -> f64 {
    (x as f64 - CODE_CENTRE as f64) / CODES_PER_DIV * scale - offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocks_are_ordered_and_apart() {
        assert!(BLOCKS.iter().all(|b| b.0 <= b.1 && b.1 % 4 == 0 && b.0 % 4 == 0));
        assert!(BLOCKS.windows(2).all(|w| w[0].1 < w[1].0));
        assert!(BLOCKS.iter().all(|b| (b.1 as usize) < BAR_LEN));
    }

    #[test]
    fn an_adc_write_is_two_words_with_the_strobe_first() {
        assert_eq!(adc_write(0, 0x12, 0xbeef), Some([(0x3000, 0x0512_beef), (0x3000, 0x0112_beef)]));
        assert_eq!(adc_write(1, 0x7f, 0), Some([(0x3000, 0x067f_0000), (0x3000, 0x027f_0000)]));
        assert_eq!(adc_write(2, 0, 0), None);
        assert_eq!(adc_write(0, 0x80, 0), None);
    }

    #[test]
    fn an_adc_read_sets_bit_23() {
        assert_eq!(adc_read(0, 0x05), Some([(0x3000, 0x0585_0000), (0x3000, 0x0185_0000)]));
        assert_eq!(adc_read(1, 0x05), Some([(0x3000, 0x0685_0000), (0x3000, 0x0285_0000)]));
    }

    #[test]
    fn every_hardware_code_of_the_stock_table_has_an_image() {
        assert_eq!(fpga_image(1), Some("sparrow3_K160T.bit"));
        assert!((2..=7).all(|c| fpga_image(c) == Some("sparrow3_RT688.bit")));
        assert!((8..=11).all(|c| fpga_image(c) == Some("sparrow3_K160T_LR.bit")));
        assert_eq!(fpga_image(0), None);
        assert_eq!(fpga_image(12), None);
    }

    #[test]
    fn a_read_out_word_is_converted_as_on_the_dho() {
        assert_eq!(volts(32768, 1.0, 0.0), 0.0);
        assert!((volts(32768 + 7500, 0.5, 0.0) - 0.5).abs() < 1e-12);
        assert!((volts(32768 - 15000, 2.0, 0.25) + 4.25).abs() < 1e-12);
        // the DHO800/DHO900's decode of the same words gives the same volts
        let w = [0u16, 1000, 32768, 40268, 65535];
        let dho = crate::dho::seq::volts(&w, 0.2, 0.05);
        for (x, d) in w.iter().zip(dho) {
            assert!((volts(*x, 0.2, 0.05) as f32 - d).abs() < 1e-5);
        }
    }
}
