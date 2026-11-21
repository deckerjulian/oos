// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `oos-fpgad bringup`: the first bring-up step of the MHO900's acquisition board, run by hand (docs/mho.md,
//! "Bring-up, step 1"). The stock order of docs/protocols/mho-acquisition.md, section 1: supplies of the ADCs and front
//! ends, the sample clock (the unit's own table over the 3-wire bus, after its ID was read back), the FPGA supply, the
//! FPGA image (the unit's own file, slave serial over spi5), PCIe, then a read of the first registers. Each step says
//! what it saw; the first that fails ends the run. Nothing is written to a file, a flash or a partition. Not yet
//! verified on hardware.

use oos_hal::dho::gpio::Line;
use oos_hal::mho::{acq, bringup as b};
use std::time::{Duration, Instant};

fn opt<'a>(args: &'a [String], k: &str) -> Option<&'a str> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).map(|s| s.as_str())
}

/// The steps in order; `--until` ends after the named one.
const STEPS: [&str; 4] = ["power", "clock", "fpga", "pcie"];

pub(super) fn run(args: &[String]) -> i32 {
    let Some(code) = opt(args, "--hw-code").and_then(|c| c.parse::<u32>().ok()) else {
        eprintln!(
            "oos-fpgad bringup: --hw-code N is needed: the hardware code tools/mho-check.py reported under the stock \
             system (3 on every unit so far)"
        );
        return 2;
    };
    let until = opt(args, "--until").unwrap_or("pcie");
    let Some(last) = STEPS.iter().position(|s| *s == until) else {
        eprintln!("oos-fpgad bringup: --until takes one of {:?}", STEPS);
        return 2;
    };
    let (Some(table), Some(image)) = (b::clock_table(code), acq::fpga_image(code)) else {
        eprintln!("oos-fpgad bringup: no clock table or FPGA image is known for hardware code {}", code);
        return 2;
    };
    println!("bringup: hardware code {} (given), clock table {}, FPGA image {}", code, table, image);
    straps();
    if !args.iter().any(|a| a == "--yes") {
        println!(
            "bringup: this switches on the acquisition board's supplies and writes its clock chip and FPGA (volatile: \
             all of it is gone at power-off). Run again with --yes to go on."
        );
        return 0;
    }
    let steps: [(&str, &dyn Fn() -> Result<(), String>); 4] = [
        ("power", &|| power(&[b::line::ADC_POWER, b::line::AFE_POWER])),
        ("clock", &|| clock(table)),
        ("fpga", &|| power(&[b::line::FPGA_POWER]).and_then(|_| fpga(image))),
        ("pcie", &pcie),
    ];
    for (name, step) in steps.iter().take(last + 1) {
        println!("---- {}", name);
        if let Err(e) = step() {
            println!("bringup: {} FAILED: {}", name, e);
            return 1;
        }
    }
    println!("bringup: done up to {}", until);
    0
}

/// The hardware code straps as they read (inputs only), for the report: how they map to the code is not known.
fn straps() {
    let v: Vec<String> = b::line::HD_CODE
        .iter()
        .map(|n| match Line::input(n) {
            Ok(l) => (l.get() as u8).to_string(),
            Err(e) => format!("? ({})", e),
        })
        .collect();
    println!("bringup: straps hd_code0..3 = {}", v.join(" "));
}

/// Switches the named supplies on, in order, 100 ms apart (the stock script writes them one after the other).
fn power(lines: &[&str]) -> Result<(), String> {
    for n in lines {
        let l = Line::output(n, true).map_err(|e| e.to_string())?;
        drop(l); // the line keeps its level
        println!("bringup: {} = 1", n);
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// The clock chip's 3-wire bus, driven bit by bit.
struct Pll {
    sclk: Line,
    cs: Line,
    sdio: Option<Line>,
}

impl Pll {
    fn open() -> Result<Pll, String> {
        let e = |e: std::io::Error| e.to_string();
        // the two lines the stock system leaves at 1 and 0 while the clock runs
        drop(Line::output(b::line::PLL_RESET, true).map_err(e)?);
        drop(Line::output(b::line::SYSREF_RESET, false).map_err(e)?);
        Ok(Pll {
            sclk: Line::output(b::line::PLL_SCLK, false).map_err(e)?,
            cs: Line::output(b::line::PLL_CS, true).map_err(e)?,
            sdio: Some(Line::output(b::line::PLL_SDIO, false).map_err(e)?),
        })
    }

    fn clock_out(&self, bits: &[bool]) {
        for &bit in bits {
            if let Some(d) = &self.sdio {
                d.set(bit);
            }
            self.sclk.set(true);
            self.sclk.set(false);
        }
    }

    fn write(&self, word: u32) {
        self.cs.set(false);
        self.clock_out(&b::frame_bits(word));
        self.cs.set(true);
    }

    /// A read: 16 bits out, then the data line turns to an input for 8 bits, each read before its rising edge.
    fn read(&mut self, addr: u16) -> Result<u8, String> {
        self.cs.set(false);
        self.clock_out(&b::frame_bits(b::read_word(addr))[..16]);
        drop(self.sdio.take());
        let r = Line::input(b::line::PLL_SDIO).map(|d| {
            let mut v = 0u8;
            for _ in 0..8 {
                v = v << 1 | d.get() as u8;
                self.sclk.set(true);
                self.sclk.set(false);
            }
            v
        });
        self.cs.set(true);
        self.sdio = Some(Line::output(b::line::PLL_SDIO, false).map_err(|e| e.to_string())?);
        r.map_err(|e| e.to_string())
    }
}

/// The sample clock: the chip's ID first (a wrong bus or line stops here, before anything is written), then the
/// unit's table, then the ID again.
fn clock(table: &str) -> Result<(), String> {
    let path = format!("{}/{}", b::CLOCK_DIR, table);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", path, e))?;
    let words = b::parse_clock_table(&text).map_err(|e| format!("{}: {}", path, e))?;
    println!("bringup: {} holds {} register words", path, words.len());
    let mut pll = Pll::open()?;
    let id = |pll: &mut Pll| -> Result<Vec<u8>, String> {
        let mut v = Vec::new();
        for (a, what) in b::ID_REGS {
            let x = pll.read(a)?;
            println!("bringup: clock chip {:#05x} {} = {:#04x}", a, what, x);
            v.push(x);
        }
        Ok(v)
    };
    let before = id(&mut pll)?;
    if before[3..5] != b::VENDOR_ID {
        return Err(format!(
            "the clock chip's vendor ID reads {:02x} {:02x}, not {:02x} {:02x}: nothing written to it",
            before[3],
            before[4],
            b::VENDOR_ID[0],
            b::VENDOR_ID[1]
        ));
    }
    for (i, w) in words.iter().enumerate() {
        pll.write(*w);
        if i == 0 {
            // the first word is the chip's reset (R0): let it settle
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    println!("bringup: {} words written", words.len());
    std::thread::sleep(Duration::from_millis(100));
    id(&mut pll)?;
    Ok(())
}

/// Waits up to `ms` for line `l` to read `level`.
fn wait(l: &Line, level: bool, ms: u64) -> bool {
    let t = Instant::now();
    while l.get() != level {
        if t.elapsed() > Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

/// The FPGA in slave serial mode (Xilinx UG470): PROGRAM_B low, INIT_B high again, the bitstream over spi5, then
/// DONE. Then the 5 s the stock script waits.
fn fpga(image: &str) -> Result<(), String> {
    use std::io::Write;
    let path = format!("{}/{}", b::FPGA_DIR, image);
    let file = std::fs::read(&path).map_err(|e| format!("{}: {}", path, e))?;
    let (part, data) = b::bit_payload(&file).map_err(|e| format!("{}: {}", path, e))?;
    println!("bringup: {} for part {:?}, {} bytes", path, part, data.len());
    let e = |e: std::io::Error| e.to_string();
    // a second run: the FPGA must not reconfigure behind a bound endpoint
    crate::pcie::pcie_down().map_err(e)?;
    let init = Line::input(b::line::INIT_B).map_err(e)?;
    let done = Line::input(b::line::DONE).map_err(e)?;
    println!("bringup: before: INIT_B {} DONE {}", init.get() as u8, done.get() as u8);
    let prog = Line::output(b::line::PROGRAM_B, false).map_err(e)?;
    std::thread::sleep(Duration::from_millis(1));
    if !wait(&init, false, 100) {
        println!("bringup: INIT_B did not go low with PROGRAM_B low (going on)");
    }
    prog.set(true);
    if !wait(&init, true, 1000) {
        return Err("INIT_B stayed low after PROGRAM_B: the FPGA is not ready for its image (supply?)".into());
    }
    let mut spi =
        std::fs::OpenOptions::new().write(true).open(b::FPGA_SPI).map_err(|e| format!("{}: {}", b::FPGA_SPI, e))?;
    let t = Instant::now();
    for c in data.chunks(b::SPI_CHUNK) {
        spi.write_all(c).map_err(|e| format!("{}: {}", b::FPGA_SPI, e))?;
    }
    println!(
        "bringup: image sent in {:.1} s; INIT_B {} DONE {}",
        t.elapsed().as_secs_f32(),
        init.get() as u8,
        done.get() as u8
    );
    // extra clocks for the start-up sequence
    let _ = spi.write_all(&[0xff; 64]);
    if !wait(&done, true, 1000) {
        return Err(format!("DONE stayed low (INIT_B {}: low means a CRC error in the stream)", init.get() as u8));
    }
    println!("bringup: DONE high, waiting 5 s as the stock system does");
    std::thread::sleep(Duration::from_secs(5));
    Ok(())
}

/// PCIe host and xdma (the helper of the DHO800/DHO900, with its retries), then the first registers.
fn pcie() -> Result<(), String> {
    use oos_hal::dho::bus::Bus;
    let mut bar = crate::pcie::pcie_up().map_err(|e| e.to_string())?;
    let id = std::fs::read_to_string("/sys/bus/pci/devices/0000:01:00.0/device").unwrap_or_default();
    println!("bringup: PCIe endpoint 0000:01:00.0, device {}", id.trim());
    for (off, name) in b::FIRST_READS {
        println!("bringup: {:#06x} {} = {:#010x}", off, name, bar.rd(off));
    }
    Ok(())
}
