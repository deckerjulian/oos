// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Quad-SPI flash and PSRAM.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{at, rising, Kind, Packet};
use crate::edges::{edges, states};

/// Quad-SPI frame layout: lines per phase (1, 2 or 4), address length (0 = no address, 3 or 4 bytes), dummy cycles
/// after the address, `mode` = a mode byte ("continuous read" bits M7..M0) on the address lines after the address.
/// 1-1-1 = { cmd 1, addr 1, data 1 }, 1-4-4 = { 1, 4, 4 }, QPI 4-4-4 = { 4, 4, 4 } and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QspiCfg {
    pub cmd_lines: u8,
    pub addr_lines: u8,
    pub data_lines: u8,
    pub addr_bytes: u8,
    pub dummy_cycles: u32,
    pub mode: bool,
}

impl QspiCfg {
    pub const fn new(
        cmd_lines: u8,
        addr_lines: u8,
        data_lines: u8,
        addr_bytes: u8,
        dummy_cycles: u32,
        mode: bool,
    ) -> QspiCfg {
        QspiCfg { cmd_lines, addr_lines, data_lines, addr_bytes, dummy_cycles, mode }
    }
}

/// Common serial flash / PSRAM opcodes (JEDEC JESD216 / vendor conventions shared by most parts).
pub fn qspi_opcode_name(op: u8) -> &'static str {
    match op {
        0x01 => "WRSR",
        0x02 => "PP",
        0x03 => "READ",
        0x04 => "WRDI",
        0x05 => "RDSR",
        0x06 => "WREN",
        0x0b => "FAST READ",
        0x0c => "FAST READ4B",
        0x12 => "PP4B",
        0x13 => "READ4B",
        0x15 => "RDSR3",
        0x20 => "SE 4K",
        0x21 => "SE4B 4K",
        0x31 => "WRSR2",
        0x32 => "QPP",
        0x34 => "QPP4B",
        0x35 => "RDSR2 / ENTER QPI",
        0x38 => "4PP / ENTER QPI",
        0x3b => "DREAD",
        0x3c => "DREAD4B",
        0x3e => "4PP4B",
        0x4b => "RDUID",
        0x52 => "BE 32K",
        0x5a => "RDSFDP",
        0x5c => "BE4B 32K",
        0x60 | 0xc7 => "CE",
        0x66 => "RSTEN",
        0x6b => "QREAD",
        0x6c => "QREAD4B",
        0x75 => "SUSPEND",
        0x7a => "RESUME",
        0x90 => "REMS",
        0x99 => "RST",
        0x9f => "RDID",
        0xab => "RES",
        0xb7 => "EN4B",
        0xb9 => "DP",
        0xbb => "2READ",
        0xbc => "2READ4B",
        0xd8 => "BE 64K",
        0xdc => "BE4B 64K",
        0xe9 => "EX4B",
        0xeb => "4READ",
        0xec => "4READ4B",
        0xf5 => "EXIT QPI",
        0xff => "EXIT QPI / MODE RST",
        _ => "?",
    }
}

/// Frame layout of a common opcode. `qpi` = commands on 4 lines (all phases on 4 lines, read dummy cycles =
/// `qpi_dummy`), `addr_bytes` = the current address mode (3 or 4) for opcodes that follow it. None for unknown
/// opcodes.
pub fn qspi_opcode_cfg(op: u8, qpi: bool, addr_bytes: u8, qpi_dummy: u32) -> Option<QspiCfg> {
    let ab = addr_bytes;
    let c = |a: u8, al: u8, dl: u8, mode: bool, dummy: u32| QspiCfg::new(1, al, dl, a, dummy, mode);
    let cfg = match op {
        0x03 | 0x02 | 0x20 | 0x52 | 0xd8 => c(ab, 1, 1, false, 0),
        0x0b => c(ab, 1, 1, false, 8),
        0x3b => c(ab, 1, 2, false, 8),
        0xbb => c(ab, 2, 2, true, 0),
        0x6b => c(ab, 1, 4, false, 8),
        0xeb => c(ab, 4, 4, true, 4),
        0x32 => c(ab, 1, 4, false, 0),
        0x38 => c(ab, 4, 4, false, 0),
        0x13 | 0x12 | 0x21 | 0x5c | 0xdc => c(4, 1, 1, false, 0),
        0x0c => c(4, 1, 1, false, 8),
        0x3c => c(4, 1, 2, false, 8),
        0xbc => c(4, 2, 2, true, 0),
        0x6c => c(4, 1, 4, false, 8),
        0xec => c(4, 4, 4, true, 4),
        0x34 => c(4, 1, 4, false, 0),
        0x3e => c(4, 4, 4, false, 0),
        0x5a => c(3, 1, 1, false, 8),
        0x90 => c(3, 1, 1, false, 0),
        0xab => c(0, 1, 1, false, 24),
        0x4b => c(0, 1, 1, false, 32),
        0x01 | 0x04 | 0x05 | 0x06 | 0x11 | 0x15 | 0x31 | 0x35 | 0x60 | 0x66 | 0x75 | 0x7a | 0x99 | 0x9f | 0xb7
        | 0xb9 | 0xc7 | 0xe9 | 0xf5 | 0xff => c(0, 1, 1, false, 0),
        _ => return None,
    };
    if !qpi {
        return Some(cfg);
    }
    // QPI: everything on 4 lines; dummy bytes become dummy clocks, read dummy cycles are configured in the part
    let dummy = match op {
        0xab | 0x4b => cfg.dummy_cycles / 4,
        _ if cfg.dummy_cycles > 0 || cfg.mode => qpi_dummy,
        _ => 0,
    };
    Some(QspiCfg {
        cmd_lines: 4,
        addr_lines: 4,
        data_lines: 4,
        addr_bytes: cfg.addr_bytes,
        dummy_cycles: dummy,
        mode: false,
    })
}

/// Quad-SPI flash / PSRAM: CS# active low frames, all lines sampled on the rising SCK edge (SPI mode 0 or 3), MSB
/// first; on 2/4 lines IO1/IO3 carries the most significant bit of each cycle. `io` = IO0..IO3 (missing lines read
/// low). Per frame: `Field("CMD")` (value = opcode), `Field("ADDR")`, `Field("MODE")`, `Field("DUMMY")` (value =
/// cycles) and `Kind::Data` bytes (single line: value = IO0 byte, value2 = IO1 byte, like SPI MOSI/MISO). With
/// `auto` the opcode selects the layout (`qspi_opcode_cfg`; unknown opcodes use `cfg`), and the decoder follows
/// EN4B/EX4B and entering/leaving QPI (0x35/0x38 without data, 0xF5/0xFF); `cfg` gives the start state (cmd_lines 4 =
/// QPI, addr_bytes) and the QPI read dummy cycles.
pub fn qspi(cs: &[f32], sck: &[f32], io: &[&[f32]], cfg: &QspiCfg, auto: bool, thr: f32, hyst: f32) -> Vec<Packet> {
    let lines = |n: u8| -> u32 {
        match n {
            2 => 2,
            4 => 4,
            _ => 1,
        }
    };
    let ios: Vec<Vec<Option<bool>>> = io.iter().take(4).map(|v| states(v, thr, hyst)).collect();
    let clk = rising(sck, thr, hyst);
    // CS-low frames
    let mut frames = Vec::new();
    let mut open = if states(cs, thr, hyst).iter().flatten().next() == Some(&false) { Some(0.0) } else { None };
    for e in edges(cs, thr, hyst) {
        if !e.rising {
            open = Some(e.t);
        } else if let Some(s) = open.take() {
            frames.push((s, e.t));
        }
    }
    if let Some(s) = open {
        frames.push((s, f64::INFINITY));
    }
    let mut out = Vec::new();
    let (mut qpi, mut ab4) = (cfg.cmd_lines == 4, cfg.addr_bytes == 4);
    for (fs, fe) in frames {
        let a = clk.partition_point(|&t| t <= fs);
        let b = clk.partition_point(|&t| t < fe);
        let cyc: Vec<(f64, u32)> = clk[a..b]
            .iter()
            .map(|&t| (t, ios.iter().enumerate().fold(0u32, |v, (k, s)| v | (at(s, t).unwrap_or(false) as u32) << k)))
            .collect();
        let mut k = 0;
        // `nbits` bits on `l` lines -> (value, t0, t1)
        let take = |l: u32, nbits: u32, k: &mut usize| -> Option<(u32, f64, f64)> {
            let n = nbits.div_ceil(l) as usize;
            if n == 0 || *k + n > cyc.len() {
                return None;
            }
            let mask = (1u32 << l) - 1;
            let v = cyc[*k..*k + n].iter().fold(0u32, |v, c| v.wrapping_shl(l) | (c.1 & mask));
            let r = (v, cyc[*k].0, cyc[*k + n - 1].0);
            *k += n;
            Some(r)
        };
        let cmd_l = if auto {
            if qpi {
                4
            } else {
                1
            }
        } else {
            lines(cfg.cmd_lines)
        };
        let Some((op, t0, t1)) = take(cmd_l, 8, &mut k) else { continue };
        let op = op as u8;
        out.push(Packet::field("CMD", t0, t1, op as u32, true));
        let f = if auto {
            qspi_opcode_cfg(op, qpi, if ab4 { 4 } else { 3 }, cfg.dummy_cycles).unwrap_or(*cfg)
        } else {
            *cfg
        };
        if auto {
            let only_cmd = k == cyc.len();
            match op {
                0xb7 => ab4 = true,
                0xe9 => ab4 = false,
                0x35 | 0x38 if only_cmd && !qpi => qpi = true,
                0xf5 | 0xff if qpi => qpi = false,
                _ => {}
            }
            if only_cmd {
                continue;
            }
        }
        let (al, dl) = (lines(f.addr_lines), lines(f.data_lines));
        if f.addr_bytes > 0 {
            let Some((v, t0, t1)) = take(al, f.addr_bytes.min(4) as u32 * 8, &mut k) else { continue };
            out.push(Packet::field("ADDR", t0, t1, v, true));
        }
        if f.mode {
            let Some((v, t0, t1)) = take(al, 8, &mut k) else { continue };
            out.push(Packet::field("MODE", t0, t1, v, true));
        }
        if f.dummy_cycles > 0 {
            let n = f.dummy_cycles as usize;
            if k + n > cyc.len() {
                continue;
            }
            out.push(Packet::field("DUMMY", cyc[k].0, cyc[k + n - 1].0, f.dummy_cycles, true));
            k += n;
        }
        if dl == 1 {
            while k + 8 <= cyc.len() {
                let c = &cyc[k..k + 8];
                let mo = c.iter().fold(0u32, |v, x| v << 1 | (x.1 & 1));
                let mi = c.iter().fold(0u32, |v, x| v << 1 | (x.1 >> 1 & 1));
                out.push(Packet::new(Kind::Data, c[0].0, c[7].0, mo, true).with_value2(mi));
                k += 8;
            }
        } else {
            while let Some((v, t0, t1)) = take(dl, 8, &mut k) {
                out.push(Packet::new(Kind::Data, t0, t1, v, true));
            }
        }
    }
    out
}
