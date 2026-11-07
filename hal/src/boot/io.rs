// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The unit's side of the boot outside BAR1: the FPGA boot monitor on `/dev/spidev1.0` (BOOT only), the LMX2582 lines,
//! and `power_on`, which chains FPGA boot, the caller's PCIe bring-up and `cold_boot`. Linux only, except the ioctl
//! numbers (checked on the host).

/// `_IOW('k', nr, size)` of linux/spi/spidev.h.
const fn spi_iow(nr: u32, size: u32) -> u32 {
    (1 << 30) | (size << 16) | (0x6b << 8) | nr
}
pub const SPI_IOC_WR_MODE: u32 = spi_iow(1, 1);
pub const SPI_IOC_WR_BITS_PER_WORD: u32 = spi_iow(3, 1);
pub const SPI_IOC_WR_MAX_SPEED_HZ: u32 = spi_iow(4, 4);

/// Sends `\rBOOT\r` + slot address to the FPGA boot monitor (`spi2boot`: spidev1.0, mode 0, 8 bit, 10 MHz, two
/// writes, no reply) [U]. Only the three image slots are accepted. This is the only monitor command in OOS; there is no
/// way to send PROG or flash data through this module.
#[cfg(target_os = "linux")]
pub fn fpga_boot(addr: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    let msg = super::regs::boot_message(addr)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{:#x} is not an FPGA image slot", addr)))?;
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open("/dev/spidev1.0")?;
    let fd = f.as_raw_fd();
    let mode: u8 = 0;
    let bits: u8 = 8;
    let speed: u32 = 10_000_000;
    for (req, p) in [
        (SPI_IOC_WR_MODE, &mode as *const u8 as *const libc::c_void),
        (SPI_IOC_WR_BITS_PER_WORD, &bits as *const u8 as *const libc::c_void),
        (SPI_IOC_WR_MAX_SPEED_HZ, &speed as *const u32 as *const libc::c_void),
    ] {
        if unsafe { libc::ioctl(fd, req as _, p) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    f.write_all(&msg[..6])?;
    f.write_all(&msg[6..])?;
    Ok(())
}

/// LMX2582 3-wire bus on the GPIO lines `spi_le`, `spi_sclk`, `spi_sdio` (gpio0 lines 6, 3, 2), 1 us after each change
/// as `oos-hw pll-init` [U].
#[cfg(target_os = "linux")]
pub struct GpioPll {
    le: crate::gpio::Line,
    sclk: crate::gpio::Line,
    sdio: crate::gpio::Line,
}

#[cfg(target_os = "linux")]
impl GpioPll {
    pub fn open() -> std::io::Result<GpioPll> {
        use crate::gpio::Line;
        Ok(GpioPll { le: Line::output("spi_le", false)?, sclk: Line::output("spi_sclk", false)?, sdio: Line::output("spi_sdio", false)? })
    }
}

#[cfg(target_os = "linux")]
impl super::PllPins for GpioPll {
    fn set(&mut self, pin: super::PllPin, v: bool) {
        use super::PllPin::*;
        match pin {
            Le => self.le.set(v),
            Sclk => self.sclk.set(v),
            Sdio => self.sdio.set(v),
        }
        std::thread::sleep(std::time::Duration::from_micros(1));
    }
}

/// Seconds the boot monitor gets before PCIe is probed (Rigol's scripts: fixed `sleep 10`). 3 s brought PCIe up at the
/// first try on the unit, and the bring-up tries again if not.
pub const FPGA_BOOT_WAIT_S: u64 = 3;

/// Seconds since boot (`/proc/uptime`): the clock shared by the processes of the start-up (the BOOT command is sent by
/// one process at the start of rcS, the cold boot runs later in another).
pub fn uptime() -> f64 {
    std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next().and_then(|t| t.parse().ok())).unwrap_or(0.0)
}

/// The wait for the bitstream load after the BOOT command: OOS_FPGA_WAIT or /oos/fpga-wait (seconds) for boot-time
/// trials, else `FPGA_BOOT_WAIT_S`; the PCIe bring-up retries anyway if the FPGA is not up yet (oos-fpga-up worked
/// with 5 s on 2026-11-02).
pub fn fpga_wait_s() -> f64 {
    std::env::var("OOS_FPGA_WAIT").ok().or_else(|| std::fs::read_to_string("/oos/fpga-wait").ok()).and_then(|s| s.trim().parse::<f64>().ok()).filter(|w| (1.0..=30.0).contains(w)).unwrap_or(FPGA_BOOT_WAIT_S as f64)
}

/// Cold start from power-on: BOOT command for `addr` (or, with `booted_at`, the uptime at which an earlier process
/// sent it), the rest of the wait, `pcie_up` (the caller loads the PCIe host and xdma modules, retrying as
/// `oos-fpga-up`, and maps BAR1), then the PLL and AFE lines and `cold_boot_with`.
#[cfg(target_os = "linux")]
pub fn power_on(
    addr: u32,
    booted_at: Option<f64>,
    pcie_up: impl FnOnce() -> std::io::Result<crate::bus::Bar1>,
    cal: &super::CalFiles,
    opt: &super::Options,
    progress: &mut dyn FnMut(u32),
) -> Result<super::Report, String> {
    let t0 = std::time::Instant::now();
    let sent = match booted_at {
        Some(t) => t,
        None => {
            fpga_boot(addr).map_err(|e| format!("FPGA boot: {}", e))?;
            uptime()
        }
    };
    // the FPGA loads now; sample clock and AFEs do not need it
    let mut pll = GpioPll::open().map_err(|e| format!("PLL lines: {}", e))?;
    let mut afe = crate::afe::GpioAfe::open().map_err(|e| format!("AFE lines: {}", e))?;
    let mut clk = super::SysClock::default();
    let early = super::early_start(&mut pll, &mut afe, &mut clk, progress)?;
    let wait = fpga_wait_s();
    let left = (sent + wait - uptime()).clamp(0.0, wait);
    std::thread::sleep(std::time::Duration::from_secs_f64(left));
    let mut bar = pcie_up().map_err(|e| format!("PCIe / BAR1: {}", e))?;
    // boot-time profile: FPGA boot command + fixed wait + PCIe bring-up before the register steps
    eprintln!("oos-fpgad: FPGA boot {:.1} s wait ({:.1} s of it here), PCIe up after {:.1} s, at uptime {:.1} s", wait, left, t0.elapsed().as_secs_f64(), uptime());
    Ok(super::cold_boot_with(super::Hw { bus: &mut bar, clock: &mut clk, pll: None, afe: Some(&mut afe), early: Some(early) }, cal, opt, progress))
}

#[cfg(test)]
mod tests {
    #[test]
    fn spidev_ioctl_numbers() {
        // linux/spi/spidev.h: SPI_IOC_WR_MODE 0x40016b01, SPI_IOC_WR_BITS_PER_WORD 0x40016b03, SPI_IOC_WR_MAX_SPEED_HZ
        // 0x40046b04 (fpga-boot.md)
        assert_eq!(super::SPI_IOC_WR_MODE, 0x4001_6b01);
        assert_eq!(super::SPI_IOC_WR_BITS_PER_WORD, 0x4001_6b03);
        assert_eq!(super::SPI_IOC_WR_MAX_SPEED_HZ, 0x4004_6b04);
    }
}
