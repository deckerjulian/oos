// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Access to the FPGA register window (BAR1 of the XDMA endpoint, `/dev/xdma0_bypass`, 32 KiB).

use crate::dho::Write;

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

/// Reads `bytes` from the XDMA C2H stream (`/dev/xdma0_c2h_0`) into a new buffer (a `DmaBuf` used once). For reads in
/// a loop keep a `DmaBuf` instead: no allocation, no zero fill and no move per read.
pub fn read_dma(c2h: &mut impl std::io::Read, bytes: usize, short: impl FnMut(usize, usize)) -> std::io::Result<Vec<u8>> {
    let mut b = DmaBuf::new();
    b.read(c2h, bytes, short)?;
    Ok(b.into_vec())
}

const PAGE: usize = 4096;

/// Largest single C2H read() (4 MiB, whole pages). Live batches are planned to about 4 MB (`batch_plan` in oos-ui), but
/// one frame is read whatever its size: 1 M points of 3 or 4 channels is 8 MB, a compressed deep record of 4 channels
/// 5 MB. Larger transfers are split into reads of this size; the stream continues across them as across a short read.
pub const MAX_READ: usize = 4 << 20;

/// A C2H read buffer kept across reads. The transfer lands at the start of a 4 KiB aligned region of whole pages that
/// nothing else uses: the RK3399's PCIe is not cache coherent and the driver maps the user pages for the DMA, so a
/// buffer that shares a cache line or page with other heap data lets the transfer's cache maintenance corrupt that data
/// (oos-ui died in `free()` at 100 ns/div with 2500-byte reads, 2026-11-04). The region grows to the largest read and
/// is never zero-filled again: a read either fills `bytes` completely or fails.
#[derive(Default)]
pub struct DmaBuf {
    raw: Vec<u8>,
    /// start of the aligned region in `raw`
    off: usize,
    /// bytes of the aligned region, whole pages
    cap: usize,
    /// valid bytes from `off`
    len: usize,
}

impl DmaBuf {
    pub fn new() -> DmaBuf {
        DmaBuf::default()
    }

    /// Room for `bytes`: a new allocation only when the region is too small.
    fn reserve(&mut self, bytes: usize) {
        if bytes <= self.cap {
            return;
        }
        let cap = bytes.div_ceil(PAGE) * PAGE;
        // up to a page before the aligned start, and a page behind its end, so no other allocation shares its pages
        self.raw = vec![0u8; cap + 2 * PAGE];
        self.off = (PAGE - self.raw.as_ptr() as usize % PAGE) % PAGE;
        self.cap = cap;
    }

    /// Reads `bytes` from `c2h` to the start of the region. `short(got, k)` is called for every read shorter than
    /// what was left.
    pub fn read(&mut self, c2h: &mut impl std::io::Read, bytes: usize, mut short: impl FnMut(usize, usize)) -> std::io::Result<&mut [u8]> {
        self.reserve(bytes);
        self.len = 0;
        let off = self.off;
        let mut got = 0;
        while got < bytes {
            // at most `MAX_READ` per read(): the rest of the stream follows in the next one, as after a short read
            let want = (bytes - got).min(MAX_READ);
            match c2h.read(&mut self.raw[off + got..off + got + want])? {
                0 => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, format!("end of stream after {} of {} bytes", got, bytes))),
                k => {
                    if k < want {
                        short(got, k);
                    }
                    got += k;
                }
            }
        }
        self.len = bytes;
        Ok(&mut self.raw[off..off + bytes])
    }

    /// The bytes of the last read (after `truncate`).
    pub fn data(&self) -> &[u8] {
        &self.raw[self.off..self.off + self.len]
    }

    /// Keeps the first `n` bytes of the last read.
    pub fn truncate(&mut self, n: usize) {
        self.len = self.len.min(n);
    }

    /// The data as a plain vector, moved to the front of the allocation (no copy into a new one).
    pub fn into_vec(mut self) -> Vec<u8> {
        self.raw.copy_within(self.off..self.off + self.len, 0);
        self.raw.truncate(self.len);
        self.raw
    }
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
        Bar1::from_file(std::fs::OpenOptions::new().read(true).write(true).open("/dev/xdma0_bypass")?)
    }

    /// Maps the 32 KiB of BAR1 of an open `/dev/xdma0_bypass` (a plain file of that size in the tests).
    fn from_file(f: std::fs::File) -> std::io::Result<Bar1> {
        use std::os::unix::io::AsRawFd;
        let p = unsafe { libc::mmap(std::ptr::null_mut(), 0x8000, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0) };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Bar1 { regs: p as *mut u32, _file: f, gap_us: 10 })
    }
}

/// Unmaps BAR1. The file closes by itself, but a mapping keeps the device in use: `rmmod xdma` fails while one is left.
#[cfg(target_os = "linux")]
impl Drop for Bar1 {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.regs as *mut libc::c_void, 0x8000) };
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
    // Break it catches: a `Bar1` that leaves its mapping behind when dropped. The mapping keeps /dev/xdma0_bypass in
    // use, so `rmmod xdma` fails until the process exits (seen on the unit, 2026-11-08: the repeat of the BOOT command
    // stopped there).
    #[cfg(target_os = "linux")]
    #[test]
    fn dropping_a_bar1_unmaps_it() {
        let path = std::env::temp_dir().join(format!("oos-bar1-unmap-test-{}", std::process::id()));
        let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        f.set_len(0x8000).unwrap();
        let name = path.to_str().unwrap().to_string();
        let mapped = || std::fs::read_to_string("/proc/self/maps").unwrap().lines().any(|l| l.contains(&name));
        let bar = super::Bar1::from_file(f).unwrap();
        assert!(mapped(), "the test must see the mapping while the Bar1 lives");
        drop(bar);
        let still = mapped();
        std::fs::remove_file(&path).unwrap();
        assert!(!still, "the mapping of the device file is still there after the Bar1 was dropped");
    }

    // Break it catches: a C2H transfer of more than `MAX_READ` handed to the driver in one read() (an 8 MB frame of
    // 4 channels at 1 M points), or a split that loses or reorders bytes
    #[test]
    fn large_reads_are_split_into_max_read_pieces() {
        struct Rec(Vec<u8>, usize, Vec<usize>);
        impl std::io::Read for Rec {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                self.2.push(b.len());
                let k = b.len().min(self.0.len() - self.1);
                b[..k].copy_from_slice(&self.0[self.1..self.1 + k]);
                self.1 += k;
                Ok(k)
            }
        }
        let n = 8_000_016;
        let data: Vec<u8> = (0..n).map(|i| (i * 7 % 251) as u8).collect();
        let mut r = Rec(data.clone(), 0, Vec::new());
        let mut shorts = 0;
        let mut b = super::DmaBuf::new();
        assert_eq!(b.read(&mut r, n, |_, _| shorts += 1).unwrap(), &data[..]);
        assert!(r.2.iter().all(|&x| x <= super::MAX_READ), "{:?}", r.2);
        assert_eq!(r.2.len(), n.div_ceil(super::MAX_READ));
        assert_eq!(shorts, 0, "planned pieces are no short reads");
        // a small read stays one read()
        let mut r = Rec(data[..4096].to_vec(), 0, Vec::new());
        b.read(&mut r, 4096, |_, _| {}).unwrap();
        assert_eq!(r.2, vec![4096]);
    }

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

    /// A stream that hands out reads of pseudo-random sizes (1..=max bytes).
    struct Ragged {
        data: Vec<u8>,
        at: usize,
        seed: u32,
        max: usize,
    }

    impl std::io::Read for Ragged {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let k = (1 + (self.seed >> 8) as usize % self.max).min(b.len()).min(self.data.len() - self.at);
            b[..k].copy_from_slice(&self.data[self.at..self.at + k]);
            self.at += k;
            Ok(k)
        }
    }

    // Break it catches: a reused buffer that mixes data of an earlier, longer read into a shorter one, loses its 4 KiB
    // alignment or its page of its own (heap corruption on the unit, 2026-11-04), or allocates again for every read.
    #[test]
    fn dma_buf_is_reused_aligned_and_exact_with_short_reads() {
        let mut b = super::DmaBuf::new();
        let mut ptr = None;
        for (i, &n) in [9000usize, 2500, 9000, 8192, 1, 4096].iter().enumerate() {
            let data: Vec<u8> = (0..n as u32).map(|x| (x.wrapping_mul(31) ^ i as u32 * 77) as u8).collect();
            let mut shorts = 0;
            let mut r = Ragged { data: data.clone(), at: 0, seed: i as u32, max: 1500 };
            let got = b.read(&mut r, n, |_, _| shorts += 1).unwrap().to_vec();
            assert_eq!(got, data, "read {} of {} bytes", i, n);
            assert_eq!(b.data(), &data[..]);
            assert!(shorts > 0 || n <= 1500);
            let p = b.data().as_ptr() as usize;
            assert_eq!(p % 4096, 0, "aligned start");
            // whole pages from the aligned start, all inside the one allocation
            assert!(b.cap % 4096 == 0 && b.cap >= n && b.off + b.cap <= b.raw.len());
            if i > 0 {
                assert_eq!(Some(p), ptr, "no new allocation for a read that fits");
            }
            ptr = Some(p);
        }
        // a larger read grows the region once
        let big: Vec<u8> = (0..20_000u32).map(|x| x as u8).collect();
        b.read(&mut Ragged { data: big.clone(), at: 0, seed: 9, max: 7000 }, 20_000, |_, _| {}).unwrap();
        assert_eq!((b.data(), b.data().as_ptr() as usize % 4096), (&big[..], 0));
        // an early end of the stream is an error, not a short buffer
        assert!(b.read(&mut Ragged { data: big[..100].to_vec(), at: 0, seed: 1, max: 40 }, 200, |_, _| {}).is_err());
        // truncate keeps the front
        b.read(&mut Ragged { data: big.clone(), at: 0, seed: 3, max: 9000 }, 20_000, |_, _| {}).unwrap();
        b.truncate(10);
        assert_eq!(b.data(), &big[..10]);
        assert_eq!(b.into_vec(), big[..10].to_vec());
    }
}
