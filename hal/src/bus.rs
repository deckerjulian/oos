// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Access to the FPGA register window (BAR1 of the XDMA endpoint, `/dev/xdma0_bypass`, 32 KiB).

use crate::Write;

/// FPGA version register 0x4000 of the image this layer was written against.
pub const FPGA_VERSION: u32 = 0xe207_2115;

pub trait Bus {
    fn wr(&mut self, off: u32, v: u32);
    fn rd(&mut self, off: u32) -> u32;
    /// Applies a write sequence in order.
    fn apply(&mut self, seq: &[Write]) {
        for &(o, v) in seq {
            self.wr(o, v);
        }
    }
}

/// Reads `bytes` from the XDMA C2H stream (`/dev/xdma0_c2h_0`) into a new buffer. The transfer lands in a
/// 4 KiB aligned region of its own, padded to whole pages, and is then moved to the front: the RK3399's PCIe is not
/// cache coherent and the driver maps the user pages for the DMA, so a buffer that shares a cache line or page with
/// other heap data lets the transfer's cache maintenance corrupt that data (oos-ui died in `free()` at 100 ns/div
/// with 2500-byte reads, 2026-11-04). `short(got, k)` is called for every read shorter than what was left.
pub fn read_dma(c2h: &mut impl std::io::Read, bytes: usize, mut short: impl FnMut(usize, usize)) -> std::io::Result<Vec<u8>> {
    const PAGE: usize = 4096;
    let mut v = vec![0u8; bytes + 2 * PAGE];
    let off = (PAGE - v.as_ptr() as usize % PAGE) % PAGE;
    let mut got = 0;
    while got < bytes {
        match c2h.read(&mut v[off + got..off + bytes])? {
            0 => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, format!("end of stream after {} of {} bytes", got, bytes))),
            k => {
                if k < bytes - got {
                    short(got, k);
                }
                got += k;
            }
        }
    }
    v.copy_within(off..off + bytes, 0);
    v.truncate(bytes);
    Ok(v)
}

/// Test bus: records writes, answers reads from a table (0 if unset).
#[derive(Default)]
pub struct Recorder {
    pub writes: Vec<Write>,
    pub regs: std::collections::HashMap<u32, u32>,
}

impl Bus for Recorder {
    fn wr(&mut self, off: u32, v: u32) {
        self.writes.push((off, v));
    }
    fn rd(&mut self, off: u32) -> u32 {
        self.regs.get(&off).copied().unwrap_or(0)
    }
}

/// BAR1 mapped from `/dev/xdma0_bypass`. Writes are spaced by `gap_us` (the verified read-out used 10 us).
#[cfg(target_os = "linux")]
pub struct Bar1 {
    regs: *mut u32,
    _file: std::fs::File,
    pub gap_us: u64,
}

#[cfg(target_os = "linux")]
impl Bar1 {
    pub fn open() -> std::io::Result<Bar1> {
        use std::os::unix::io::AsRawFd;
        let f = std::fs::OpenOptions::new().read(true).write(true).open("/dev/xdma0_bypass")?;
        let p = unsafe { libc::mmap(std::ptr::null_mut(), 0x8000, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0) };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Bar1 { regs: p as *mut u32, _file: f, gap_us: 10 })
    }
}

#[cfg(target_os = "linux")]
impl Bus for Bar1 {
    fn wr(&mut self, off: u32, v: u32) {
        assert!(off < 0x8000 && off % 4 == 0);
        unsafe { std::ptr::write_volatile(self.regs.add((off >> 2) as usize), v) };
        if self.gap_us > 0 {
            std::thread::sleep(std::time::Duration::from_micros(self.gap_us));
        }
    }
    fn rd(&mut self, off: u32) -> u32 {
        assert!(off < 0x8000 && off % 4 == 0);
        unsafe { std::ptr::read_volatile(self.regs.add((off >> 2) as usize)) }
    }
}

#[cfg(target_os = "linux")]
unsafe impl Send for Bar1 {}

#[cfg(test)]
mod tests {
    #[test]
    fn read_dma_short_reads() {
        // a stream that hands out at most 1000 bytes a read
        struct Chunky(Vec<u8>, usize);
        impl std::io::Read for Chunky {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                let k = b.len().min(1000).min(self.0.len() - self.1);
                b[..k].copy_from_slice(&self.0[self.1..self.1 + k]);
                self.1 += k;
                Ok(k)
            }
        }
        let data: Vec<u8> = (0..2500u32).map(|i| (i * 7) as u8).collect();
        let mut shorts = 0;
        let v = super::read_dma(&mut Chunky(data.clone(), 0), 2500, |_, _| shorts += 1).unwrap();
        assert_eq!((v, shorts), (data.clone(), 2));
        assert!(super::read_dma(&mut Chunky(data, 0), 3000, |_, _| {}).is_err());
    }
}
