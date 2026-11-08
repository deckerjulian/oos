// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! A model of the unit for the dry run and the tests: answers the reads the cold boot steers on with the values the
//! real unit gave (FPGA version, 0x1008 = 0x24000201, ADC clock counter, the log of the stock system's calibration of
//! 2026-11-02), records every write, and keeps virtual time. Nothing here touches hardware.

use super::regs::{PllPin, ADC_RDATA, ADC_SPI};
use super::Clock;
use crate::dho::afe::AfeBus;
use crate::dho::bus::Bus;
use crate::dho::Write;

/// "ADC0 Sync DO:1,0,1,1,1,0,0,1" (logcat 2026-11-02) [O].
pub const UNIT_SYNC_DO: [bool; 8] = [true, false, true, true, true, false, false, true];

/// Eye scan of the 28 lanes (logcat "ADC 0 Line k", d = 0 first, '1' = errors) [O].
pub const UNIT_EYE: [&str; 28] = [
    "11000000000000000000000011111111", "11000000000000000000000111111111", "11100000000000000000000011111111",
    "11000000000000000000000111111111", "11100000000000000000000011111111", "11100000000000000000000111111111",
    "11100000000000000000000111111111", "11100000000000000000000111111111", "11000000000000000000000111111111",
    "11100000000000000000000011111111", "11000000000000000000001111111111", "11000000000000000000001111111111",
    "11100000000000000000000011111111", "11100000000000000000000011111111", "11000000000000000000000111111111",
    "11000000000000000000001111111111", "11100000000000000000000011111111", "11100000000000000000000011111111",
    "11100000000000000000000011111111", "11100000000000000000000011111111", "11100000000000000000000011111111",
    "11100000000000000000000011111111", "11100000000000000000001111111111", "11000000000000000000001111111111",
    "11100000000000000000000111111111", "11000000000000000000000111111111", "11100000000000000000000111111111",
    "11100000000000000000000011111111",
];

/// The delays logged for that scan ("IDelay = ..."), hard-coded in `oos-hw adc-idelay` [O][U].
pub const UNIT_IDELAY: [u32; 28] = [12, 12, 13, 12, 13, 12, 12, 12, 12, 13, 11, 11, 13, 13, 12, 11, 13, 13, 13, 13, 13, 13, 12, 11, 12, 12, 12, 13];

/// Error words err[d] (bit k = lane k) from per-lane strings.
pub fn eye_from_lanes(lanes: &[&str; 28]) -> [u32; 32] {
    let mut e = [0u32; 32];
    for (k, s) in lanes.iter().enumerate() {
        for (d, c) in s.bytes().enumerate().take(32) {
            e[d] |= ((c == b'1') as u32) << k;
        }
    }
    e
}

/// Virtual clock: `sleep_us` advances the time.
#[derive(Default)]
pub struct SimClock {
    pub t_us: u64,
}

impl Clock for SimClock {
    fn sleep_us(&mut self, us: u64) {
        self.t_us += us;
    }
    fn now_us(&mut self) -> u64 {
        self.t_us
    }
}

/// The unit as the boot sees it through BAR1.
pub struct Unit {
    pub writes: Vec<Write>,
    pub reads: Vec<(u32, u32)>,
    /// 0x4000
    pub version: u32,
    /// number of 0x1008 reads that still return bit 29 = 0 (u32::MAX = never ready)
    pub ddr_busy_reads: u32,
    /// 0x1210
    pub adc_clk: u32,
    /// bit 1 of ADC reg 7 per sync delay
    pub sync_do: [bool; 8],
    /// bit 0 of ADC reg 7 per sync delay
    pub sync_low: [bool; 8],
    /// err[d] of the eye scan (bit k = lane k)
    pub eye: [u32; 32],
    /// 0x1030 (preamble code: Da in bits 7:0, Db in 23:16)
    pub preamble: u32,
    /// polls per core until the ADC trim reports done (u32::MAX = never)
    pub trim_polls: u32,
    /// MIG read-back 0x14a0, 0x14a4, 0x1498, 0x14ac, 0x14b4
    pub mig: [u32; 5],
    last_adc: u32,
    sync_delay: usize,
    idelay: usize,
    ddr_reads: u32,
    adc_trim: [u32; 8],
}

impl Unit {
    /// The unit as observed: stock FPGA image, DDR ready at once (0x1008 = 0x24000201), ADC clock 62500, the logcat's
    /// sync and eye results, preamble 0 (s1020 = 0x1010 live), ADC trim done at the first poll [O]/[A for 62500 and the trim].
    pub fn nominal() -> Unit {
        Unit {
            writes: Vec::new(),
            reads: Vec::new(),
            version: crate::dho::bus::FPGA_VERSION,
            ddr_busy_reads: 0,
            adc_clk: 62500,
            sync_do: UNIT_SYNC_DO,
            sync_low: [false; 8],
            eye: eye_from_lanes(&UNIT_EYE),
            preamble: 0,
            trim_polls: 1,
            mig: [0x12, 0x8a2, 0x4d6, 0x924, 0x1ef],
            last_adc: 0,
            sync_delay: 7,
            idelay: 0,
            ddr_reads: 0,
            adc_trim: [0; 8],
        }
    }
}

impl Bus for Unit {
    fn wr(&mut self, off: u32, v: u32) {
        self.writes.push((off, v));
        match off {
            ADC_SPI => {
                self.last_adc = v;
                if v & 0x0080_0000 == 0 && (v >> 16) & 0x7f == 6 {
                    self.sync_delay = ((v >> 8) & 7) as usize;
                }
            }
            0x1018 if v & 0x3fff_f000 == 0x3fff_f000 => self.idelay = (v & 0x1f) as usize,
            _ => {}
        }
    }
    fn rd(&mut self, off: u32) -> u32 {
        let v = match off {
            0x4000 => self.version,
            0x1008 => {
                self.ddr_reads += 1;
                if self.ddr_busy_reads != u32::MAX && self.ddr_reads > self.ddr_busy_reads {
                    0x2400_0201
                } else {
                    0x0400_0201
                }
            }
            0x1210 => self.adc_clk,
            0x1024 => self.eye[self.idelay] & 0x3fff,
            0x1028 => (self.eye[self.idelay] >> 14) & 0x3fff,
            0x1030 => self.preamble,
            0x14a0 => self.mig[0],
            0x14a4 => self.mig[1],
            0x1498 => self.mig[2],
            0x14ac => self.mig[3],
            0x14b4 => self.mig[4],
            ADC_RDATA if self.last_adc & 0x0080_0000 != 0 => {
                let reg = (self.last_adc >> 16) & 0x7f;
                let data = if reg == 7 {
                    (self.sync_do[self.sync_delay] as u32) << 1 | self.sync_low[self.sync_delay] as u32
                } else if reg >= 0x16 && (reg - 0x16) % 8 == 0 && reg <= 0x4e {
                    let n = ((reg - 0x16) / 8) as usize;
                    self.adc_trim[n] += 1;
                    if self.trim_polls != u32::MAX && self.adc_trim[n] >= self.trim_polls {
                        0x40
                    } else {
                        0
                    }
                } else {
                    0
                };
                0x0400_0000 | (self.last_adc & 0x00ff_0000) | data
            }
            ADC_RDATA => 0x0400_0000 | (self.last_adc & 0x00ff_ffff),
            _ => 0,
        };
        self.reads.push((off, v));
        v
    }
}

/// Records LMX2582 words by sampling SDIO on the rising SCLK edges; LE high latches.
#[derive(Default)]
pub struct PllRec {
    pub words: Vec<[u8; 3]>,
    sdio: bool,
    bits: u32,
    n: u32,
}

impl super::PllPins for PllRec {
    fn set(&mut self, pin: PllPin, v: bool) {
        match pin {
            PllPin::Sdio => self.sdio = v,
            PllPin::Sclk if v => {
                self.bits = self.bits << 1 | self.sdio as u32;
                self.n += 1;
            }
            PllPin::Le if v => {
                if self.n == 24 {
                    let b = self.bits.to_be_bytes();
                    self.words.push([b[1], b[2], b[3]]);
                }
                self.bits = 0;
                self.n = 0;
            }
            _ => {}
        }
    }
}

/// AFE bus stub: counts writes, reads (efuse bits) return 0.
#[derive(Default)]
pub struct AfeStub {
    pub writes: usize,
    pub reads: usize,
}

impl AfeBus for AfeStub {
    fn write(&mut self, _ch: usize, _reg: u8, _val: u16) {
        self.writes += 1;
    }
    fn read(&mut self, _ch: usize, _reg: u8) -> u16 {
        self.reads += 1;
        0
    }
}
