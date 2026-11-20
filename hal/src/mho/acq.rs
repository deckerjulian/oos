// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The MHO900's acquisition board as far as it is described (docs/protocols/mho-acquisition.md): the devices, the
//! register blocks, the ADC's SPI bridge, the front ends' bus, the FPGA image per hardware code and the read-out
//! format.
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

/// The front ends' bus (driver `spi2afe_gpio`): the processor drives it bit by bit on GPIOs, one chip select per
/// channel. **Verified on the unit**, read under the stock system with the kernel's `gpio_value` trace (`mho-check.py
/// --afe` on an MHO934 twice, an MHO954 and two MHO98, GitHub issues #4 and #248): the lines, the frames and the
/// registers and bits named here. What the other registers and bits do is not known yet, and most of their values are
/// the unit's own (docs/protocols/mho-acquisition.md, section 8).
pub mod afe {
    /// Clock, data out and data in.
    pub const SCLK: u32 = 75;
    pub const MOSI: u32 = 74;
    pub const MISO: u32 = 73;
    /// The chip select of each channel's front end, CH1 first (the driver's cs3, cs2, cs1 and cs0).
    pub const CS: [u32; 4] = [68, 69, 66, 76];
    /// The select line `a` of each channel, CH1 first: high at 5 and 10 V/div at 1 MOhm, and at 1 V/div at 50 ohm.
    pub const SELECT_A: [u32; 4] = [149, 129, 125, 122];
    /// The select line `b` of each channel, CH1 first (named so by the stock driver; it changed in no step).
    pub const SELECT_B: [u32; 4] = [152, 130, 126, 123];
    /// The select line `c` of each channel, CH1 first: high at 0.1 and 0.2 V/div at 1 MOhm, low at 50 ohm.
    pub const SELECT_C: [u32; 4] = [153, 131, 127, 124];
    /// Bits of a write frame, sent MSB first: the driver sets the data line, then the clock low and high, so a bit is
    /// valid at the rising edge.
    pub const FRAME_BITS: u32 = 24;
    /// Coupling and input stage: bit 15 DC (0 AC), bit 14 set from 1 mV/div to 50 mV/div, bit 13 50 ohm. GND
    /// coupling sends no word: the FPGA sets the lane's gain to 0.
    pub const COUPLING: u8 = 0x07;
    pub const COUPLING_DC: u16 = 1 << 15;
    pub const COUPLING_HIGH_GAIN: u16 = 1 << 14;
    pub const COUPLING_FIFTY: u16 = 1 << 13;
    /// The offset DAC: about 0x8000 at 0 V (the unit's own zero per scale and channel, which moves by up to a dozen
    /// codes between two runs), 471 codes per 0.3 V at 0.1 V/div on every channel; it rises with the offset at 0.1
    /// and 1 V/div and falls with it at 10 mV/div.
    pub const OFFSET: u8 = 0x28;
    /// Filter. The 20 MHz bandwidth limit writes 0x1905 on every channel of every unit. At full bandwidth bits 13:0
    /// are 0x1fcb (0x17cb at 5 V/div) on the MHO954, both MHO98 and the MHO934 after a change of its licences, and
    /// were 0x1dc9 on the MHO934 before (assumed: bits 9 and 1 follow the licensed bandwidth). Bits 15:14 change with
    /// the scale, differently per unit.
    pub const FILTER: u8 = 0x2e;
    pub const FILTER_20M: u16 = 0x1905;
    /// The bits of `FILTER` that differed on the MHO934 before and after the licence change.
    pub const FILTER_LICENCE_BITS: u16 = 0x0202;
    /// A read: 32 clocks, the first byte 0x80 (bit 7 read, register 0), then 24 bits come back on `MISO`. The stock
    /// system reads it twice, 2 ms apart, every few hundred ms while the channel is at 50 ohm, and once when it goes
    /// back to 1 MOhm. It read 0x0c40ff at 50 ohm and 0x0440ff after (assumed: bit 19 a state of the 50 ohm path).
    pub const READ_FRAME_BITS: u32 = 32;
    pub const STATUS_READ: u32 = 0x8000_0000;
    pub const STATUS_FIFTY: u32 = 1 << 19;

    /// The 24-bit frame of one register write: the address in bits 23:16, the data in bits 15:0.
    pub fn frame(reg: u8, data: u16) -> u32 {
        (reg as u32) << 16 | data as u32
    }

    /// The range field of register 0x29, bits 9:8: 0 from 1 to 5 mV/div, 1 from 10 to 50 mV/div, 2 from 0.1 V/div, 3
    /// at 50 ohm. Bits 15:10 of the register are the unit's own.
    pub const RANGE: u8 = 0x29;
    pub fn range(scale_uv: u32, fifty: bool) -> u16 {
        match (fifty, scale_uv) {
            (true, _) => 3,
            (false, 0..=9_999) => 0,
            (false, 10_000..=99_999) => 1,
            (false, _) => 2,
        }
    }
}

/// The 20 MHz bandwidth limit after the ADC: one bit per ADC lane in each of the two SPU words, bit 20 + lane (lane 0
/// first). With four channels lane k carries CH k+1; CH1 alone sets all four. **Verified on the unit** (MHO934
/// `--map`, each channel's limit with all four on; MHO98 and MHO934 with CH1 alone).
pub const BW_LIMIT_REGS: [u32; 2] = [0x1004, 0x1014];
pub fn bw_limit_bit(lane: u32) -> Option<u32> {
    (lane < 4).then(|| 1 << (20 + lane))
}

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
    fn a_front_end_frame_is_address_then_data() {
        // MHO934 --afe: CH1's offset at -0.3 V and DC coupling at 1 V/div, as sent on the bus
        assert_eq!(afe::frame(afe::OFFSET, 0x7e11), 0x28_7e11);
        assert_eq!(afe::frame(afe::COUPLING, 0x8440), 0x07_8440);
        assert!(afe::frame(0xff, 0xffff) < 1 << afe::FRAME_BITS);
        assert_eq!(0x1fcb ^ 0x1dc9, afe::FILTER_LICENCE_BITS);
        assert_eq!(afe::frame(afe::FILTER, afe::FILTER_20M), 0x2e_1905);
        // the coupling word of CH1 at 10 mV/div, DC, 50 ohm (MHO934 --fifty)
        assert_eq!(0x8440 | afe::COUPLING_HIGH_GAIN | afe::COUPLING_FIFTY, 0xe440);
        assert_eq!(afe::STATUS_READ >> 24, 0x80);
        assert_eq!((0x0c40ff & afe::STATUS_FIFTY, 0x0440ff & afe::STATUS_FIFTY), (afe::STATUS_FIFTY, 0));
        let mut lines: Vec<u32> =
            afe::CS.iter().chain(&afe::SELECT_A).chain(&afe::SELECT_B).chain(&afe::SELECT_C).copied().collect();
        lines.extend([afe::SCLK, afe::MOSI, afe::MISO]);
        let n = lines.len();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), n, "every line once");
    }

    #[test]
    fn the_range_field_switches_at_ten_millivolts_and_a_tenth_of_a_volt() {
        // 0x29 of CH1 on the MHO934: 0x7c42 at 2 mV/div, 0x7d42 at 20 mV/div, 0x7e42 at 1 V/div, 0x7f82 at 50 ohm
        for (uv, fifty, word) in
            [(2_000, false, 0x7c42u16), (20_000, false, 0x7d42), (1_000_000, false, 0x7e42), (10_000, true, 0x7f82)]
        {
            assert_eq!(afe::range(uv, fifty), word >> 8 & 3);
        }
        assert_eq!(afe::range(9_999, false), 0);
        assert_eq!(afe::range(100_000, false), 2);
    }

    #[test]
    fn the_bandwidth_limit_has_one_bit_per_lane() {
        // MHO934 --map: 0x1004 0x4004a50 with no limit, 0x4104a50 with CH1's, 0x4804a50 with CH4's (all four on)
        assert_eq!(0x0400_4a50 | bw_limit_bit(0).unwrap(), 0x0410_4a50);
        assert_eq!(0x0400_4a50 | bw_limit_bit(3).unwrap(), 0x0480_4a50);
        assert_eq!((0..4).map(|l| bw_limit_bit(l).unwrap()).sum::<u32>(), 0xf << 20);
        assert_eq!(bw_limit_bit(4), None);
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
