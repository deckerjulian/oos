// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Live FPGA source, EXPERIMENTAL: it writes FPGA registers.
//!
//! It repeats the read-out that was verified word-exact under OOS on 2026-11-02 (docs/protocols/adc-spu-init.md,
//! "Second frame under OOS"): 28 register writes
//! (channel processing, window 0x1100/0x1108, TX length 0x11f0, frame header, record sequence off, stop sequence,
//! play-info clear, SPU/TX reset, play-last pulse, FSM run) followed by ONE read of exactly 2*N bytes from /dev/xdma0_c2h_0.
//! Precondition: the FPGA was set up by the stock system (warm hand-over), OOS has not re-initialised it.
//! Open question this source answers: does the FSM run at the end re-arm the acquisition, so that repeating the
//! sequence gives live frames?

#[derive(Clone)]
pub struct Cfg {
    pub n: u32,
    pub start: u32,
    pub ratio: u32,
    pub fsm: u32,
    /// register shadows at STOP as read from the stock system: 0x4004, 0x4008, 0x400c, 0x1000, 0x1010, 0x4080
    pub sh: [u32; 6],
    /// arm a new acquisition before every read (record path, docs/protocols/acquisition-start.md)
    pub arm: bool,
    /// read with the stock type-4 play instead of play-last (variant 2 of acquisition-start.md section 4C)
    pub play4: bool,
}

impl Default for Cfg {
    fn default() -> Cfg {
        // values of the hand-over of 2026-11-02 21:12 (2.5 M points, trigger in the middle, ratio 1, fsm mode 1)
        Cfg { n: 4000, start: 1_248_000, ratio: 1, fsm: 1, sh: [0x1, 0x800001, 0x6000_0000, 0x0, 0x2000_0000, 0x4000_0010], arm: false, play4: false }
    }
}

use oos_hal::dho::scu::{Shadows, Window};

impl Cfg {
    pub fn window(&self) -> Window {
        Window { n: self.n, start: self.start, ratio: self.ratio, fsm: self.fsm, la: false }
    }
    pub fn shadows(&self) -> Shadows {
        Shadows { s4004: self.sh[0], s4008: self.sh[1], s400c: self.sh[2], s1000: self.sh[3], s1010: self.sh[4], s4080: self.sh[5], ..Shadows::HANDOVER }
    }
}

pub type Shared = std::sync::Arc<std::sync::Mutex<(u64, Vec<u16>, String)>>;

/// Starts the read-out thread; frames (sequence number, words, status text) appear in the returned slot.
#[cfg(target_os = "linux")]
pub fn spawn(cfg: Cfg, period_ms: u64) -> Shared {
    use std::os::unix::io::AsRawFd;
    let shared: Shared = std::sync::Arc::new(std::sync::Mutex::new((0, Vec::new(), "starting".into())));
    let out = shared.clone();
    std::thread::spawn(move || {
        let status = |s: String| {
            eprintln!("oos-ui fpga: {}", s);
            out.lock().unwrap().2 = s;
        };
        let bar = match std::fs::OpenOptions::new().read(true).write(true).open("/dev/xdma0_bypass") {
            Ok(f) => f,
            Err(e) => return status(format!("/dev/xdma0_bypass: {}", e)),
        };
        let p = unsafe { libc::mmap(std::ptr::null_mut(), 0x8000, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, bar.as_raw_fd(), 0) };
        if p == libc::MAP_FAILED {
            return status("mmap of BAR1 failed".into());
        }
        let regs = p as *mut u32;
        let rd = |off: u32| unsafe { std::ptr::read_volatile(regs.add((off >> 2) as usize)) };
        let version = rd(0x4000);
        if version != 0xe207_2115 {
            return status(format!("unexpected FPGA version {:#x}, not touching it", version));
        }
        let mut c2h = match std::fs::File::open("/dev/xdma0_c2h_0") {
            Ok(f) => f,
            Err(e) => return status(format!("/dev/xdma0_c2h_0: {}", e)),
        };
        let wr = |seq: &[(u32, u32)]| {
            for &(off, v) in seq {
                unsafe { std::ptr::write_volatile(regs.add((off >> 2) as usize), v) };
                std::thread::sleep(std::time::Duration::from_micros(10));
            }
        };
        // poll a status bit (read only): true when set within the timeout
        let wait_bit = |off: u32, bit: u32, timeout_ms: u64, step_us: u64| -> (bool, u32, f64) {
            let t = std::time::Instant::now();
            loop {
                let v = rd(off);
                if v & (1 << bit) != 0 {
                    return (true, v, t.elapsed().as_secs_f64() * 1000.0);
                }
                if t.elapsed().as_millis() as u64 >= timeout_ms {
                    return (false, v, t.elapsed().as_secs_f64() * 1000.0);
                }
                std::thread::sleep(std::time::Duration::from_micros(step_us));
            }
        };
        let mut sh = cfg.shadows();
        let win = cfg.window();
        status(format!("mode: {}, read {}", if cfg.arm { "arm + read" } else { "read only" }, if cfg.play4 { "type-4 play" } else { "play last" }));
        let mut buf = vec![0u8; 2 * cfg.n as usize];
        let mut seq = 0u64;
        let mut prev: Vec<u16> = Vec::new();
        let t_start = std::time::Instant::now();
        loop {
            let mut info = String::new();
            let mut count = 1;
            if cfg.arm {
                let (stopped, st, _) = wait_bit(0x4034, 13, 30, 500);
                wr(&oos_hal::dho::scu::arm(cfg.fsm, &mut sh, 0, 1));
                let (done, v, ms) = wait_bit(0x4080, 30, 1000, 2000);
                if done {
                    sh.s4080 = v;
                    count = rd(0x4010).wrapping_sub(rd(0x4014)).wrapping_add(1);
                }
                info = format!("pre-stop {} ({:#x}) done {} after {:.1} ms 0x4080 {:#x} count {} 0x4034 {:#x}", stopped, st, done, ms, v, count, rd(0x4034));
                if !done {
                    status(format!("arm: no acquisition within 1 s ({})", info));
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            }
            if cfg.play4 {
                wr(&oos_hal::dho::scu::readout_play4(&win, &mut sh, 0, 1));
            } else {
                wr(&oos_hal::dho::scu::readout(&win, &mut sh));
            }
            match oos_hal::dho::bus::read_dma(&mut c2h, buf.len(), |_, _| {}) {
                Ok(b) => {
                    buf = b;
                    seq += 1;
                    let words: Vec<u16> = buf.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
                    // log: does each cycle give new data? (changed words against the previous frame)
                    let changed = if prev.len() == words.len() { words.iter().zip(&prev).filter(|(a, b)| a != b).count() } else { words.len() };
                    if seq <= 20 || seq % 30 == 0 {
                        let sum: u64 = words.iter().map(|&w| w as u64).sum();
                        let (mn, mx) = (words.iter().min().copied().unwrap_or(0), words.iter().max().copied().unwrap_or(0));
                        eprintln!(
                            "oos-ui fpga: frame {} t {:.2} s sum {:#x} min {} max {} changed {} status {:#x} {}",
                            seq,
                            t_start.elapsed().as_secs_f64(),
                            sum,
                            mn,
                            mx,
                            changed,
                            rd(0x4034),
                            info
                        );
                    }
                    prev = words.clone();
                    let mut g = out.lock().unwrap();
                    *g = (seq, words, format!("frame {} changed {} status {:#x}", seq, changed, rd(0x4034)));
                }
                Err(e) => {
                    status(format!("c2h read: {} (status {:#x}) {}", e, rd(0x4034), info));
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(period_ms));
        }
    });
    shared
}
