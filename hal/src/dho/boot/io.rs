// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The unit's side of the boot outside BAR1: the FPGA boot monitor on `/dev/spidev1.0` (BOOT only), the LMX2582 lines,
//! and `power_on`, which chains FPGA boot, the caller's PCIe bring-up and `cold_boot`. Linux only, except the ioctl
//! numbers (checked on the host) and the retry of the BOOT command, which runs against a fake FPGA.

use crate::dho::bus::Bus;

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
    le: crate::dho::gpio::Line,
    sclk: crate::dho::gpio::Line,
    sdio: crate::dho::gpio::Line,
}

#[cfg(target_os = "linux")]
impl GpioPll {
    pub fn open() -> std::io::Result<GpioPll> {
        use crate::dho::gpio::Line;
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

/// Seconds the boot monitor gets before PCIe is probed (the stock scripts: fixed `sleep 10`). 3 s brought PCIe up at the
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

/// How often the BOOT command is repeated when the version register still shows the golden image (see
/// `up_with_boot_retries`). Two: a golden image that loads slower than on the unit tested would swallow the first
/// repeat as well.
pub const FPGA_BOOT_RETRIES: u32 = 2;

/// What `up_with_boot_retries` needs from the board.
pub trait Link<B: Bus> {
    /// Loads the PCIe host driver and xdma and maps BAR1.
    fn up(&mut self) -> std::io::Result<B>;
    /// Unloads xdma and the PCIe host driver; an error when xdma is still loaded. The `B` of the link has been dropped
    /// before.
    fn down(&mut self) -> std::io::Result<()>;
    /// Sends the BOOT command and waits for the image to load.
    fn boot_and_wait(&mut self) -> std::io::Result<()>;
    /// The BOOT command is about to be repeated (attempt 1, 2, ...): for the log and the start-up screen.
    fn repeated(&mut self, attempt: u32);
}

/// The FPGA's boot monitor hears a BOOT command only once its golden image runs, about 11 s after power-on, and the
/// command is not queued: one sent earlier is lost and the golden image stays (seen on seven cold starts in a row on the unit, 2026-11-08,
/// docs/protocols/fpga-boot.md). PCIe then comes up on the golden image and register 0x4000 shows another version.
/// Called with the link of the first bring-up: while 0x4000 is not `expected`, up to `retries` times the drivers are
/// unloaded (the BAR is dropped first, or the unload fails), BOOT is sent again and the link is brought up again.
/// Returns the last link, wrong version or not: the cold boot's own check reports a version that is still wrong.
pub fn up_with_boot_retries<B: Bus>(mut bar: B, expected: u32, retries: u32, link: &mut dyn Link<B>) -> Result<B, String> {
    for attempt in 1..=retries {
        if bar.rd(0x4000) == expected {
            break;
        }
        drop(bar);
        link.down().map_err(|e| format!("unloading the PCIe drivers (repeat {}): {}", attempt, e))?;
        link.repeated(attempt);
        link.boot_and_wait().map_err(|e| format!("FPGA boot (repeat {}): {}", attempt, e))?;
        bar = link.up().map_err(|e| format!("PCIe / BAR1: {}", e))?;
    }
    Ok(bar)
}

/// The board's side of `up_with_boot_retries`: the caller's PCIe bring-up, unload and note, and the BOOT command with
/// the usual wait.
#[cfg(target_os = "linux")]
struct BootLink<U, D, N> {
    addr: u32,
    up: U,
    down: D,
    note: N,
}

#[cfg(target_os = "linux")]
impl<U, D, N> Link<crate::dho::bus::Bar1> for BootLink<U, D, N>
where
    U: FnMut() -> std::io::Result<crate::dho::bus::Bar1>,
    D: FnMut() -> std::io::Result<()>,
    N: FnMut(u32),
{
    fn up(&mut self) -> std::io::Result<crate::dho::bus::Bar1> {
        (self.up)()
    }
    fn down(&mut self) -> std::io::Result<()> {
        (self.down)()
    }
    fn boot_and_wait(&mut self) -> std::io::Result<()> {
        fpga_boot(self.addr)?;
        std::thread::sleep(std::time::Duration::from_secs_f64(fpga_wait_s()));
        Ok(())
    }
    fn repeated(&mut self, attempt: u32) {
        eprintln!("oos-fpgad: 0x4000 shows another image: the BOOT command got no answer, sending it again ({} of {})", attempt, FPGA_BOOT_RETRIES);
        (self.note)(attempt)
    }
}

/// Cold start from power-on: BOOT command for `addr` (or, with `booted_at`, the uptime at which an earlier process
/// sent it), the rest of the wait, `pcie_up` (the caller loads the PCIe host and xdma modules, retrying as
/// `oos-fpga-up`, and maps BAR1), then the PLL and AFE lines and `cold_boot_with`. If the version register then shows
/// another image than `opt.fpga_version`, the BOOT command was lost: `pcie_down` unloads the modules, `repeated` is told
/// the attempt, and BOOT and the bring-up run again, up to `FPGA_BOOT_RETRIES` times (`up_with_boot_retries`).
#[cfg(target_os = "linux")]
pub fn power_on(
    addr: u32,
    booted_at: Option<f64>,
    mut pcie_up: impl FnMut() -> std::io::Result<crate::dho::bus::Bar1>,
    pcie_down: impl FnMut() -> std::io::Result<()>,
    repeated: impl FnMut(u32),
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
    let mut afe = crate::dho::afe::GpioAfe::open().map_err(|e| format!("AFE lines: {}", e))?;
    let mut clk = super::SysClock::default();
    let early = super::early_start(&mut pll, &mut afe, &mut clk, progress)?;
    let wait = fpga_wait_s();
    let left = (sent + wait - uptime()).clamp(0.0, wait);
    std::thread::sleep(std::time::Duration::from_secs_f64(left));
    let first = pcie_up().map_err(|e| format!("PCIe / BAR1: {}", e))?;
    let mut link = BootLink { addr, up: pcie_up, down: pcie_down, note: repeated };
    let mut bar = up_with_boot_retries(first, opt.fpga_version, FPGA_BOOT_RETRIES, &mut link)?;
    // boot-time profile: FPGA boot command + fixed wait + PCIe bring-up (+ repeats) before the register steps
    eprintln!("oos-fpgad: FPGA boot {:.1} s wait ({:.1} s of it here), PCIe up after {:.1} s, at uptime {:.1} s", wait, left, t0.elapsed().as_secs_f64(), uptime());
    Ok(super::cold_boot_with(super::Hw { bus: &mut bar, clock: &mut clk, pll: None, afe: Some(&mut afe), early: Some(early) }, cal, opt, progress))
}

#[cfg(test)]
mod tests {
    use super::{up_with_boot_retries, Link, FPGA_BOOT_RETRIES};
    use crate::dho::bus::Bus;
    use std::cell::RefCell;
    use std::rc::Rc;

    const MAIN: u32 = 0xe207_2115; // the image the cold boot expects at 0x4000
    const GOLDEN: u32 = 0xe106_0816; // what the unit read after a BOOT that was lost (2026-11-08)

    type Log = Rc<RefCell<Vec<String>>>;

    /// BAR1 of the fake FPGA: reads 0x4000 as the image that runs; logs when it is dropped (the real `Bar1` unmaps).
    struct FakeBar {
        version: u32,
        log: Log,
    }
    impl Bus for FakeBar {
        fn wr(&mut self, _off: u32, _v: u32) {}
        fn rd(&mut self, off: u32) -> u32 {
            assert_eq!(off, 0x4000, "only the version register is read");
            self.version
        }
    }
    impl Drop for FakeBar {
        fn drop(&mut self) {
            self.log.borrow_mut().push("bar dropped".into());
        }
    }

    /// The boot monitor of the fake FPGA: the first `lost` BOOT commands of the retries get no answer (it is still
    /// loading its golden image), the next one loads the main image.
    struct FakeLink {
        log: Log,
        lost: u32,
        main_runs: bool,
        boot_error: bool,
        up_error: bool,
        down_error: bool,
    }
    impl FakeLink {
        fn new(log: &Log, lost: u32) -> FakeLink {
            FakeLink { log: log.clone(), lost, main_runs: false, boot_error: false, up_error: false, down_error: false }
        }
        fn bar(&self) -> FakeBar {
            FakeBar { version: if self.main_runs { MAIN } else { GOLDEN }, log: self.log.clone() }
        }
    }
    impl Link<FakeBar> for FakeLink {
        fn up(&mut self) -> std::io::Result<FakeBar> {
            self.log.borrow_mut().push("up".into());
            if self.up_error {
                return Err(std::io::Error::other("no link"));
            }
            Ok(self.bar())
        }
        fn down(&mut self) -> std::io::Result<()> {
            self.log.borrow_mut().push("down".into());
            if self.down_error {
                return Err(std::io::Error::other("xdma is busy"));
            }
            Ok(())
        }
        fn boot_and_wait(&mut self) -> std::io::Result<()> {
            self.log.borrow_mut().push("boot".into());
            if self.boot_error {
                return Err(std::io::Error::other("spidev1.0 busy"));
            }
            if self.lost > 0 {
                self.lost -= 1;
            } else {
                self.main_runs = true;
            }
            Ok(())
        }
        fn repeated(&mut self, attempt: u32) {
            self.log.borrow_mut().push(format!("repeat {}", attempt));
        }
    }

    fn events(log: &Log) -> Vec<String> {
        log.borrow().clone()
    }

    // Breaks it catches: a repeat on every start (a second BOOT, a reload of the drivers on a healthy unit).
    #[test]
    fn a_healthy_start_is_left_alone() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 0);
        link.main_runs = true;
        let first = link.bar();
        let mut bar = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).unwrap();
        assert_eq!(bar.rd(0x4000), MAIN);
        assert!(events(&log).is_empty(), "nothing is sent or reloaded: {:?}", events(&log));
    }

    // Breaks it catches: no repeat at all (the failure of 2026-11-08); the BAR still mapped or xdma still loaded when
    // BOOT goes out (rmmod fails, or the FPGA reconfigures behind a bound endpoint); BOOT after the reload.
    #[test]
    fn a_lost_boot_is_repeated_with_the_drivers_unloaded() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 0);
        let first = link.bar(); // golden image: the first BOOT was lost
        let mut bar = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).unwrap();
        assert_eq!(bar.rd(0x4000), MAIN);
        assert_eq!(events(&log), ["bar dropped", "down", "repeat 1", "boot", "up"]);
    }

    // Break it catches: stopping after one repeat (a golden image that is slower than the one on the unit).
    #[test]
    fn a_second_lost_boot_is_repeated_too() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 1);
        let first = link.bar();
        let mut bar = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).unwrap();
        assert_eq!(bar.rd(0x4000), MAIN);
        assert_eq!(events(&log), ["bar dropped", "down", "repeat 1", "boot", "up", "bar dropped", "down", "repeat 2", "boot", "up"]);
    }

    // Breaks it catches: an endless loop, a third repeat, and an error of its own that hides the cold boot's message:
    // the last link goes back so that the cold boot reports the version it reads.
    #[test]
    fn it_gives_up_after_two_repeats_and_hands_back_the_wrong_version() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 99);
        let first = link.bar();
        let mut bar = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).unwrap();
        assert_eq!(bar.rd(0x4000), GOLDEN);
        assert_eq!(events(&log).iter().filter(|e| *e == "boot").count(), 2);
    }

    // Break it catches: a BOOT command that fails (spidev busy) being ignored and the link brought up anyway.
    #[test]
    fn a_failing_boot_command_stops_the_start() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 0);
        link.boot_error = true;
        let first = link.bar();
        let e = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).err().expect("an error");
        assert!(e.contains("spidev1.0 busy"), "{}", e);
        assert!(!events(&log).contains(&"up".to_string()), "no bring-up after a failed BOOT: {:?}", events(&log));
    }

    // Break it catches: BOOT going out while xdma is still loaded (the FPGA reconfigures behind a bound endpoint:
    // the CPU can lock up) because the unload failed and nobody looked.
    #[test]
    fn a_failed_unload_stops_before_boot() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 0);
        link.down_error = true;
        let first = link.bar();
        let e = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).err().expect("an error");
        assert!(e.contains("xdma is busy"), "{}", e);
        assert!(!events(&log).contains(&"boot".to_string()), "BOOT must not go out: {:?}", events(&log));
    }

    // Break it catches: a PCIe bring-up that fails after a repeat being turned into success or retried without BOOT.
    #[test]
    fn a_failing_bring_up_after_a_repeat_is_an_error() {
        let log: Log = Default::default();
        let mut link = FakeLink::new(&log, 0);
        link.up_error = true;
        let first = link.bar();
        let e = up_with_boot_retries(first, MAIN, FPGA_BOOT_RETRIES, &mut link).err().expect("an error");
        assert!(e.contains("PCIe") && e.contains("no link"), "{}", e);
    }

    #[test]
    fn spidev_ioctl_numbers() {
        // linux/spi/spidev.h: SPI_IOC_WR_MODE 0x40016b01, SPI_IOC_WR_BITS_PER_WORD 0x40016b03, SPI_IOC_WR_MAX_SPEED_HZ
        // 0x40046b04 (fpga-boot.md)
        assert_eq!(super::SPI_IOC_WR_MODE, 0x4001_6b01);
        assert_eq!(super::SPI_IOC_WR_BITS_PER_WORD, 0x4001_6b03);
        assert_eq!(super::SPI_IOC_WR_MAX_SPEED_HZ, 0x4004_6b04);
    }
}
