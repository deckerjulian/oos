// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal generators for the SWD, JTAG, Quad-SPI and I3C decoders.
//!
//! A generator for a new decoder goes next to the generators of its family, in `gen.rs` or a file in `gen/`; the
//! decoder itself goes into the file of its family in `decode/`, and its protocol module is registered in
//! `ui/src/protocols/mod.rs` (ADR 0002).

use crate::decode::{QspiCfg, TapState};
use crate::sim::Logic;

/// `n` bits of `v`, LSB first.
pub fn bits_lsb(v: u64, n: usize) -> Vec<bool> {
    (0..n).map(|k| k < 64 && v >> k & 1 == 1).collect()
}

/// A clock and N data lines: per cycle the lines change with the falling clock edge, the rising edge (half a
/// period later) samples. The clock idles low.
fn clocked<const N: usize>(cycles: &[[bool; N]], rate: f64, lo: f64, hi: f64) -> (Logic, [Logic; N]) {
    let h = 0.5 / rate;
    let mut clk = Logic::new(lo, hi, h / 20.0);
    let mut d: [Logic; N] = std::array::from_fn(|_| Logic::new(lo, hi, h / 20.0));
    clk.push(0.0, false);
    d.iter_mut().for_each(|l| l.push(0.0, false));
    let mut t = 4.0 * h;
    for c in cycles {
        clk.push(t, false);
        for (k, l) in d.iter_mut().enumerate() {
            l.push(t, c[k]);
        }
        clk.push(t + h, true);
        t += 2.0 * h;
    }
    clk.push(t, false);
    d.iter_mut().for_each(|l| l.push(t, false));
    (clk, d)
}

/// One SWD operation for `swd`.
#[derive(Clone, Copy, Debug)]
pub enum SwdOp {
    LineReset,
    JtagToSwd,
    Idle(u32),
    /// request (APnDP, RnW, register byte address 0/4/8/C), target ACK, data; `bad_parity` flips the data parity
    Xfer {
        ap: bool,
        read: bool,
        a: u8,
        ack: u8,
        data: u32,
        bad_parity: bool,
    },
}

/// SWD waveforms (SWCLK, SWDIO), host and target, all changes on the falling SWCLK edge; turnaround = high
/// (pull-up), idle = low.
pub fn swd(ops: &[SwdOp], rate: f64, lo: f64, hi: f64) -> (Logic, Logic) {
    let mut b: Vec<bool> = Vec::new();
    for op in ops {
        match *op {
            SwdOp::LineReset => b.extend([true; 56]),
            SwdOp::JtagToSwd => b.extend(bits_lsb(0xe79e, 16)),
            SwdOp::Idle(n) => b.extend((0..n).map(|_| false)),
            SwdOp::Xfer { ap, read, a, ack, data, bad_parity } => {
                let a = (a >> 2) as u32 & 3;
                let par = (ap as u32 + read as u32 + a.count_ones()) % 2 == 1;
                b.extend([true, ap, read, a & 1 == 1, a & 2 == 2, par, false, true]);
                b.push(true); // turnaround
                b.extend(bits_lsb(ack as u64, 3));
                let dp = (data.count_ones() % 2 == 1) != bad_parity;
                if ack == 1 && read {
                    b.extend(bits_lsb(data as u64, 32));
                    b.push(dp);
                    b.push(true);
                } else if ack == 1 {
                    b.push(true);
                    b.extend(bits_lsb(data as u64, 32));
                    b.push(dp);
                } else {
                    b.push(true);
                }
            }
        }
    }
    let (clk, [d]) = clocked(&b.iter().map(|&x| [x]).collect::<Vec<_>>(), rate, lo, hi);
    (clk, d)
}

/// JTAG cycle sequence builder (TMS, TDI, TDO per TCK cycle) that tracks the TAP state.
#[derive(Clone, Debug)]
pub struct JtagGen {
    pub cycles: Vec<[bool; 3]>,
    pub state: TapState,
}

impl Default for JtagGen {
    fn default() -> Self {
        Self::new()
    }
}

impl JtagGen {
    /// starts in Test-Logic-Reset
    pub fn new() -> JtagGen {
        JtagGen { cycles: Vec::new(), state: TapState::TestLogicReset }
    }
    pub fn clock(&mut self, tms: bool, tdi: bool, tdo: bool) {
        self.cycles.push([tms, tdi, tdo]);
        self.state = self.state.next(tms);
    }
    /// 5 cycles TMS high
    pub fn reset(&mut self) {
        (0..5).for_each(|_| self.clock(true, false, false));
    }
    /// `n` cycles TMS low (Run-Test/Idle)
    pub fn idle(&mut self, n: usize) {
        (0..n).for_each(|_| self.clock(false, false, false));
    }
    /// A complete IR or DR scan from Run-Test/Idle (or Test-Logic-Reset / Update) back to Run-Test/Idle; `tdo`
    /// shorter than `tdi` is padded with 0. `pause_at` = enter Pause before shifting bit k, for the given bit.
    pub fn shift(&mut self, ir: bool, tdi: &[bool], tdo: &[bool], pause_at: Option<usize>) {
        if self.state == TapState::TestLogicReset {
            self.clock(false, false, false);
        }
        self.clock(true, false, false);
        if ir {
            self.clock(true, false, false);
        }
        self.clock(false, false, false); // Capture
        self.clock(false, false, false); // -> Shift
        let n = tdi.len();
        for (k, &bit) in tdi.iter().enumerate() {
            if pause_at == Some(k) && k > 0 {
                // the previous bit already left Shift (TMS 1): Exit1 -> Pause (2 cycles) -> Exit2 -> Shift
                self.clock(false, false, false);
                self.clock(false, false, false);
                self.clock(true, false, false);
                self.clock(false, false, false);
            }
            let last = k + 1 == n || pause_at == Some(k + 1);
            self.clock(last, bit, tdo.get(k).copied().unwrap_or(false));
        }
        self.clock(true, false, false); // Update
        self.clock(false, false, false); // Run-Test/Idle
    }
    /// [TCK, TMS, TDI, TDO]
    pub fn logic(&self, rate: f64, lo: f64, hi: f64) -> [Logic; 4] {
        let (c, [tms, tdi, tdo]) = clocked(&self.cycles, rate, lo, hi);
        [c, tms, tdi, tdo]
    }
}

/// One Quad-SPI frame for `qspi`: opcode, address (when `cfg.addr_bytes` > 0), mode byte (when `cfg.mode`), dummy
/// cycles (lines low), data; `miso` = single-line data on IO1 (read) instead of IO0 (write).
#[derive(Clone, Debug)]
pub struct QspiFrame {
    pub cfg: QspiCfg,
    pub op: u8,
    pub addr: u32,
    pub mode: u8,
    pub data: Vec<u8>,
    pub miso: bool,
}

/// Quad-SPI waveforms [CS#, SCK, IO0, IO1, IO2, IO3], SPI mode 0, one CS frame per `QspiFrame`.
pub fn qspi(frames: &[QspiFrame], rate: f64, lo: f64, hi: f64) -> [Logic; 6] {
    let h = 0.5 / rate;
    let mk = || Logic::new(lo, hi, h / 20.0);
    let (mut cs, mut sck) = (mk(), mk());
    let mut io = [mk(), mk(), mk(), mk()];
    cs.push(0.0, true);
    sck.push(0.0, false);
    io.iter_mut().for_each(|l| l.push(0.0, false));
    let mut t = 4.0 * h;
    let lines = |n: u8| -> u32 {
        match n {
            2 => 2,
            4 => 4,
            _ => 1,
        }
    };
    for f in frames {
        cs.push(t, false);
        t += h;
        let mut cyc: Vec<[bool; 4]> = Vec::new();
        let mut emit = |l: u32, nbits: u32, v: u32, on_io1: bool| {
            for c in 0..nbits / l {
                let x = v >> (nbits - l * (c + 1)) & ((1 << l) - 1);
                let lv = |k: u32| if l == 1 { k == on_io1 as u32 && x == 1 } else { k < l && x >> k & 1 == 1 };
                cyc.push([lv(0), lv(1), lv(2), lv(3)]);
            }
        };
        emit(lines(f.cfg.cmd_lines), 8, f.op as u32, false);
        if f.cfg.addr_bytes > 0 {
            emit(lines(f.cfg.addr_lines), f.cfg.addr_bytes.min(4) as u32 * 8, f.addr, false);
        }
        if f.cfg.mode {
            emit(lines(f.cfg.addr_lines), 8, f.mode as u32, false);
        }
        for _ in 0..f.cfg.dummy_cycles {
            emit(1, 1, 0, false);
        }
        for &d in &f.data {
            emit(lines(f.cfg.data_lines), 8, d as u32, f.miso);
        }
        for c in cyc {
            for (k, l) in io.iter_mut().enumerate() {
                l.push(t, c[k]);
            }
            sck.push(t + h, true);
            sck.push(t + 2.0 * h, false);
            t += 2.0 * h;
        }
        t += h;
        cs.push(t, true);
        io.iter_mut().for_each(|l| l.push(t, false));
        t += 4.0 * h;
    }
    let [a, b, c, d] = io;
    [cs, sck, a, b, c, d]
}

/// I2C / I3C SDR bus builder (SDA, SCL): conditions and bits, data changes while SCL is low.
#[derive(Clone, Debug)]
pub struct I3cBus {
    pub sda: Logic,
    pub scl: Logic,
    pub t: f64,
    q: f64,
    in_frame: bool,
}

impl I3cBus {
    /// `rate` = SCL frequency
    pub fn new(rate: f64, lo: f64, hi: f64) -> I3cBus {
        let q = 0.25 / rate;
        let (mut sda, mut scl) = (Logic::new(lo, hi, q / 20.0), Logic::new(lo, hi, q / 20.0));
        sda.push(0.0, true);
        scl.push(0.0, true);
        I3cBus { sda, scl, t: 8.0 * q, q, in_frame: false }
    }
    /// START, or repeated START inside a frame
    pub fn start(&mut self) {
        let q = self.q;
        if self.in_frame {
            self.sda.push(self.t, true);
            self.t += q;
            self.scl.push(self.t, true);
            self.t += q;
        }
        self.sda.push(self.t, false);
        self.t += q;
        self.scl.push(self.t, false);
        self.t += q;
        self.in_frame = true;
    }
    pub fn stop(&mut self) {
        let q = self.q;
        self.sda.push(self.t, false);
        self.t += q;
        self.scl.push(self.t, true);
        self.t += q;
        self.sda.push(self.t, true);
        self.t += 4.0 * q;
        self.in_frame = false;
    }
    pub fn bit(&mut self, b: bool) {
        let q = self.q;
        self.sda.push(self.t, b);
        self.t += q;
        self.scl.push(self.t, true);
        self.t += 2.0 * q;
        self.scl.push(self.t, false);
        self.t += q;
    }
    /// `n` bits of `v`, MSB first
    pub fn bits(&mut self, v: u64, n: u32) {
        (0..n).rev().for_each(|k| self.bit(v >> k & 1 == 1));
    }
    /// address byte + ACK bit (`ack` = driven low)
    pub fn addr(&mut self, addr: u8, read: bool, ack: bool) {
        self.bits((addr as u64) << 1 | read as u64, 8);
        self.bit(!ack);
    }
    /// I3C write byte + T-bit (odd parity; `bad` inverts it)
    pub fn byte_t(&mut self, v: u8, bad: bool) {
        self.bits(v as u64, 8);
        self.bit(v.count_ones().is_multiple_of(2) != bad);
    }
    /// I3C read byte + T-bit (`more` = 1: more data follows)
    pub fn read_t(&mut self, v: u8, more: bool) {
        self.bits(v as u64, 8);
        self.bit(more);
    }
    /// I2C byte + ACK bit
    pub fn byte_ack(&mut self, v: u8, ack: bool) {
        self.bits(v as u64, 8);
        self.bit(!ack);
    }
    /// HDR-like traffic: both lines toggle, including SDA changes while SCL is high (no SDR conditions)
    pub fn hdr_junk(&mut self, n: usize) {
        let q = self.q;
        for k in 0..n {
            self.scl.push(self.t, k % 2 == 0);
            self.t += q;
            self.sda.push(self.t, k % 3 == 0);
            self.t += q;
        }
        self.scl.push(self.t, false);
        self.t += q;
    }
    /// HDR exit pattern (SCL low, 4 SDA falling edges) followed by STOP
    pub fn hdr_exit(&mut self) {
        let q = self.q;
        self.scl.push(self.t, false);
        self.sda.push(self.t, false);
        self.t += q;
        for _ in 0..4 {
            self.sda.push(self.t, true);
            self.t += q;
            self.sda.push(self.t, false);
            self.t += q;
        }
        self.stop();
    }
}
