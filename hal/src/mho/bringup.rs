// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The first bring-up of the MHO900's acquisition board under OOS (docs/mho.md, "Bring-up, step 1";
//! docs/protocols/mho-acquisition.md, section 1): the supplies, the sample clock from the unit's own register table,
//! the FPGA from the unit's own image, then PCIe. These are the pure parts: the GPIO lines by name, the clock table's
//! words and their frames on the 3-wire bus, and the payload of a bitstream file. `oos-fpgad bringup` runs them, only
//! when an owner starts it by hand.
//!
//! **Not yet verified on hardware.** The lines, the order and the files are those of the stock system (platform
//! probes and the start-up of section 1). The frame of the clock chip is that of its data sheet (TI LMK04821: 24 bits,
//! MSB first, a read/write bit, two zero bits, 13 address bits, 8 data bits; data in at the rising clock edge, read
//! data out after the falling one); its frames have not been recorded on a unit.

/// The GPIO lines of the bring-up, by the names OOS's device tree gives them (kernel/dts/rk3399-rigol-mho900.dts).
pub mod line {
    /// Supplies in the stock order: ADC, front ends, then (after the clock) the FPGA.
    pub const ADC_POWER: &str = "adc_power";
    pub const AFE_POWER: &str = "afe_power";
    pub const FPGA_POWER: &str = "fpga_power";
    /// The clock chip's 3-wire bus: clock, chip select (active low), the one data line, and `sysref_reset`, which
    /// the stock system leaves at 0. These four are all the stock clock driver takes. GPIO1_C4 next to them (the
    /// `reset` of the stock GPIO list) is the DSI panel's reset, not the clock chip's: the bring-up never touches it
    /// (it stopped there with "Resource busy" on an MHO98, GitHub issue #4, because the panel driver holds it).
    pub const PLL_SCLK: &str = "pll_sclk";
    pub const PLL_CS: &str = "pll_cs";
    pub const PLL_SDIO: &str = "pll_sdio";
    pub const SYSREF_RESET: &str = "sysref_reset";
    /// The FPGA's configuration lines (Kintex-7, slave serial): PROGRAM_B out, INIT_B and DONE in.
    pub const PROGRAM_B: &str = "fpga_programb";
    pub const INIT_B: &str = "fpga_initb";
    pub const DONE: &str = "fpga_done";
    /// The hardware code straps (inputs), bit 0 first.
    pub const HD_CODE: [&str; 4] = ["hd_code0", "hd_code1", "hd_code2", "hd_code3"];
}

/// The state of each line as the stock system leaves it once the board runs (platform probe of an MHO98, hardware
/// code 3: `/sys/kernel/debug/gpio`). Outputs only; the bring-up ends with these.
pub const STOCK_OUTPUTS: [(&str, bool); 7] = [
    (line::ADC_POWER, true),
    (line::AFE_POWER, true),
    (line::FPGA_POWER, true),
    (line::PLL_SCLK, false),
    (line::PLL_CS, true),
    (line::PLL_SDIO, false),
    (line::SYSREF_RESET, false),
];

/// The spidev of the FPGA's configuration stream (spi5, data and clock only).
pub const FPGA_SPI: &str = "/dev/spidev5.0";
/// The most bytes one `write()` of spidev takes (its default buffer).
pub const SPI_CHUNK: usize = 4096;
/// Where the unit keeps its clock tables, one per hardware code.
pub const CLOCK_DIR: &str = "/rigol/resource/conf";
/// Where the unit keeps its FPGA images (`acq::fpga_image` names them).
pub const FPGA_DIR: &str = "/rigol/FPGA";

/// The clock table the stock system writes for a hardware code: codes 1, 2, and 3 and up.
pub fn clock_table(hw_code: u32) -> Option<&'static str> {
    match hw_code {
        1 => Some("init_all_clock_V00_01.txt"),
        2 => Some("init_all_clock_V00_02.txt"),
        3..=11 => Some("init_all_clock_V00_03.txt"),
        _ => None,
    }
}

/// The 24-bit words of a clock table: one line `R<n> 0x<aaaadd>` per register (address in bits 23:8, data in 7:0), the
/// fields apart by a tab or spaces. A line may carry a label in brackets after `R<n>`: the first line of the units'
/// tables is `R0 (INIT)` with the reset word 0x000090 (verified on a unit: hp197's MHO98 in GitHub issue #4). Empty
/// lines are passed over; anything else, a word with the read bit (23), more than 24 bits or an address that is not
/// `<n>` is an error.
pub fn parse_clock_table(text: &str) -> Result<Vec<u32>, String> {
    let mut words = Vec::new();
    for (n, l) in text.lines().enumerate() {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        let bad = |what: &str| format!("line {}: {:?} is not `R<n> 0x<word>`{}", n + 1, l, what);
        let f: Vec<&str> = l.split_whitespace().collect();
        let (r, v) = match f[..] {
            [r, v] => (r, v),
            [r, label, v] if label.starts_with('(') && label.ends_with(')') => (r, v),
            _ => return Err(bad("")),
        };
        let reg = r.strip_prefix('R').filter(|d| !d.is_empty()).and_then(|d| d.parse::<u32>().ok());
        let w = v.strip_prefix("0x").or(v.strip_prefix("0X")).and_then(|h| u32::from_str_radix(h, 16).ok());
        match (reg, w) {
            (Some(reg), Some(w)) if w < 1 << 23 && w >> 8 == reg => words.push(w),
            _ => return Err(bad(" with a 23-bit write word to register <n>")),
        }
    }
    if words.is_empty() {
        return Err("no register words".into());
    }
    Ok(words)
}

/// The 24 bits of a word on the 3-wire bus, most significant first.
pub fn frame_bits(word: u32) -> [bool; 24] {
    std::array::from_fn(|i| word >> (23 - i) & 1 != 0)
}

/// The first 16 bits of a read of register `addr` (read bit set); the 8 data bits come back on the same line.
pub fn read_word(addr: u16) -> u32 {
    1 << 23 | (addr as u32 & 0x1fff) << 8
}

/// Registers of the clock chip that identify it (data sheet): device type, product and vendor ID, read before the
/// table is written so that a wrong bus or pin stops the bring-up first. (address, what).
pub const ID_REGS: [(u16, &str); 5] = [
    (0x003, "device type"),
    (0x004, "product ID high"),
    (0x005, "product ID low"),
    (0x00c, "vendor ID high"),
    (0x00d, "vendor ID low"),
];

/// The vendor ID of the data sheet (registers 0x00c, 0x00d): Texas Instruments.
pub const VENDOR_ID: [u8; 2] = [0x51, 0x04];

/// A bitstream file of the unit (`.bit`): the part it is for and the configuration data to stream. The header is a
/// run of fields after a fixed 13-byte start: a key (`a` design, `b` part, `c` date, `d` time) with a 16-bit big-endian
/// length and that many bytes, then `e` with a 32-bit length and the data. The data must hold the sync word
/// AA 99 55 66 near its start.
pub fn bit_payload(b: &[u8]) -> Result<(String, &[u8]), String> {
    let mut at = 13;
    let mut part = String::new();
    loop {
        let key = *b.get(at).ok_or("bitstream header cut short")?;
        if key == b'e' {
            let n = b.get(at + 1..at + 5).ok_or("bitstream header cut short")?;
            let n = u32::from_be_bytes([n[0], n[1], n[2], n[3]]) as usize;
            let data = b.get(at + 5..at + 5 + n).ok_or("bitstream shorter than its header says")?;
            let head = &data[..data.len().min(256)];
            if !head.windows(4).any(|w| w == [0xaa, 0x99, 0x55, 0x66]) {
                return Err("no sync word at the start of the bitstream".into());
            }
            return Ok((part, data));
        }
        if !matches!(key, b'a'..=b'd') {
            return Err(format!("bitstream header: field {:#04x} unknown", key));
        }
        let n = b.get(at + 1..at + 3).ok_or("bitstream header cut short")?;
        let n = u16::from_be_bytes([n[0], n[1]]) as usize;
        let v = b.get(at + 3..at + 3 + n).ok_or("bitstream header cut short")?;
        if key == b'b' {
            part = String::from_utf8_lossy(v).trim_end_matches('\0').to_string();
        }
        at += 3 + n;
    }
}

/// FPGA registers read once PCIe is up, to compare with the stock system's (tools/mho-check.py reads the same):
/// (offset, name). All lie in the first 32 KiB of the register window and are read by the stock system itself.
pub const FIRST_READS: [(u32, &str); 9] = [
    (0x0004, "FPGA version"),
    (0x401c, "hardware version"),
    (0x1000, "SPU control"),
    (0x2028, "TPU trigger A"),
    (0x3000, "ADC SPI bridge, last word"),
    (0x3004, "ADC SPI bridge, read data"),
    (0x4008, "SCU mode"),
    (0x4030, "SCU status 2"),
    (0x4034, "SCU status"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_table_is_read_word_by_word() {
        assert_eq!(parse_clock_table("R0 0x000090\n\nR2 0x000200\r\nR352 0x016003\n"), Ok(vec![0x90, 0x200, 0x16003]));
        // the units' first line: a label, then a tab (hp197's MHO98, GitHub issue #4)
        assert_eq!(parse_clock_table("R0 (INIT)\t0x000090\nR0\t0x000010\n"), Ok(vec![0x90, 0x10]));
        assert!(parse_clock_table("R0 INIT 0x000090").is_err(), "a label is in brackets");
        assert!(parse_clock_table("R3 0x000200").is_err(), "the word's address is the register's");
        assert!(parse_clock_table("").is_err());
        assert!(parse_clock_table("R0 0x800000").is_err(), "a read word is no table entry");
        assert!(parse_clock_table("R0 0x1000000").is_err());
        assert!(parse_clock_table("R0 90").is_err());
        assert!(parse_clock_table("X0 0x000090").is_err());
        assert!(parse_clock_table("R0 0x000090 extra").is_err());
    }

    #[test]
    fn frames_go_out_most_significant_bit_first() {
        let f = frame_bits(0x80_0001);
        assert!(f[0] && f[23]);
        assert_eq!(f.iter().filter(|b| **b).count(), 2);
        assert_eq!(read_word(0x00c), 0x80_0c00);
        assert_eq!(read_word(0xffff), 0x9f_ff00, "13 address bits");
    }

    #[test]
    fn each_hardware_code_has_a_table_and_an_image() {
        for code in 1..=11 {
            assert!(clock_table(code).is_some() && crate::mho::acq::fpga_image(code).is_some(), "code {}", code);
        }
        assert_eq!(clock_table(3), Some("init_all_clock_V00_03.txt"));
        assert_eq!(clock_table(0), None);
    }

    fn bit_file(data: &[u8]) -> Vec<u8> {
        let mut b = vec![0x00, 0x09, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x00, 0x00, 0x01];
        for (k, v) in [
            (b'a', &b"design;UserID=0\0"[..]),
            (b'b', b"7k160tffg676\0"),
            (b'c', b"2026/05/22\0"),
            (b'd', b"10:32:00\0"),
        ] {
            b.push(k);
            b.extend((v.len() as u16).to_be_bytes());
            b.extend(v);
        }
        b.push(b'e');
        b.extend((data.len() as u32).to_be_bytes());
        b.extend(data);
        b
    }

    #[test]
    fn the_bitstream_payload_starts_after_the_header() {
        let data =
            [[0xffu8; 32].as_slice(), &[0x00, 0x00, 0x00, 0xbb, 0x11, 0x22, 0x00, 0x44], &[0xaa, 0x99, 0x55, 0x66]]
                .concat();
        let f = bit_file(&data);
        assert_eq!(bit_payload(&f), Ok(("7k160tffg676".to_string(), &data[..])));
        assert!(bit_payload(&bit_file(&[0xff; 64])).is_err(), "no sync word");
        assert!(bit_payload(&f[..f.len() - 1]).is_err(), "cut short");
        assert!(bit_payload(&[0u8; 20]).is_err());
    }

    #[test]
    fn the_first_reads_stay_in_the_mapped_window() {
        assert!(FIRST_READS.iter().all(|(o, _)| *o < 0x8000 && o % 4 == 0));
    }
}
