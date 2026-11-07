// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Extended decoders, part 2: SWD, JTAG, Quad-SPI flash/PSRAM, I3C (SDR), S/PDIF and USB Power Delivery.
//!
//! Same conventions as `decode`: analog channels through a comparator (threshold + hysteresis), packet times are
//! fractional sample indices, protocol-specific fields are `Kind::Field(label)`. Signal generators for the tests are
//! in `gen`.

use crate::decode::{Kind, Packet};
use crate::edges::{edges, states};

fn pk(t0: f64, t1: f64, kind: Kind, value: u32, value2: Option<u32>, ok: bool) -> Packet {
    Packet { t0, t1, kind, value, value2, ok }
}

fn field(t0: f64, t1: f64, label: &'static str, value: u32, value2: Option<u32>, ok: bool) -> Packet {
    pk(t0, t1, Kind::Field(label), value, value2, ok)
}

/// A value-less event tag (label starting with '!').
fn tag(t0: f64, t1: f64, label: &'static str, ok: bool) -> Packet {
    pk(t0, t1, Kind::Field(label), 0, None, ok)
}

fn rising(clk: &[f32], thr: f32, hyst: f32) -> Vec<f64> {
    edges(clk, thr, hyst).into_iter().filter(|e| e.rising).map(|e| e.t).collect()
}

/// Comparator state at time `t` (nearest sample).
fn at(st: &[Option<bool>], t: f64) -> Option<bool> {
    st.get((t + 0.5) as usize).copied().flatten()
}

/// Bits LSB first -> value.
fn lsb(bits: &[(f64, bool)]) -> u32 {
    bits.iter().take(32).enumerate().fold(0, |v, (k, b)| v | (b.1 as u32) << k)
}

/// Bits MSB first -> value.
fn msb(bits: &[(f64, bool)]) -> u64 {
    bits.iter().fold(0, |v, b| v << 1 | b.1 as u64)
}

// ---------------------------------------------------------------------------------------------------------------
// SWD

/// Serial Wire Debug (ARM Debug Interface Architecture Specification ADIv5.0 to ADIv5.2, ARM IHI 0031): SWDIO
/// sampled on the rising SWCLK edge. Emits
/// - `Field("!RESET")`: line reset, >= 50 cycles with SWDIO high
/// - `Field("!JTAG2SWD")`: the JTAG-to-SWD select sequence 0xE79E (16 bits, LSB first)
/// - `Field("REQ")`: packet request; value = register address A[3:2] as byte offset (0x0/0x4/0x8/0xC), value2 =
///   APnDP | RnW << 1, ok = even parity
/// - `Kind::Ack`: value = the 3-bit acknowledge (1 = OK, 2 = WAIT, 4 = FAULT, 7 = no response), ok = OK
/// - `Kind::Data`: 32-bit data (read or write), ok = even parity
///
/// Turnaround and idle cycles are skipped; after WAIT/FAULT there is no data phase. A line reset must be followed by
/// at least one idle cycle (as the protocol requires), else the next start bit counts into the reset.
pub fn swd(swclk: &[f32], swdio: &[f32], thr: f32, hyst: f32) -> Vec<Packet> {
    let io = states(swdio, thr, hyst);
    let bits: Vec<(f64, bool)> = rising(swclk, thr, hyst).into_iter().filter_map(|t| at(&io, t).map(|b| (t, b))).collect();
    let n = bits.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if bits[i].1 {
            let run = bits[i..].iter().take_while(|b| b.1).count();
            if run >= 50 {
                out.push(tag(bits[i].0, bits[i + run - 1].0, "!RESET", true));
                i += run;
                continue;
            }
        } else if i + 16 <= n && lsb(&bits[i..i + 16]) == 0xe79e {
            out.push(tag(bits[i].0, bits[i + 15].0, "!JTAG2SWD", true));
            i += 16;
            continue;
        }
        // request: start 1, APnDP, RnW, A2, A3, parity, stop 0, park 1
        if !(bits[i].1 && i + 8 <= n && !bits[i + 6].1 && bits[i + 7].1) {
            i += 1;
            continue;
        }
        let (ap, rnw) = (bits[i + 1].1, bits[i + 2].1);
        let a = lsb(&bits[i + 3..i + 5]);
        let par_ok = (ap as u32 + rnw as u32 + a.count_ones()) % 2 == bits[i + 5].1 as u32;
        out.push(field(bits[i].0, bits[i + 7].0, "REQ", a << 2, Some(ap as u32 | (rnw as u32) << 1), par_ok));
        // turnaround, ACK
        if i + 12 > n {
            break;
        }
        let ack = lsb(&bits[i + 9..i + 12]);
        out.push(pk(bits[i + 9].0, bits[i + 11].0, Kind::Ack, ack, None, ack == 1));
        let mut j = i + 12;
        if ack == 1 {
            // read: data, parity, turnaround; write: turnaround, data, parity
            let d0 = if rnw { j } else { j + 1 };
            if d0 + 33 > n {
                break;
            }
            let d = lsb(&bits[d0..d0 + 32]);
            let ok = d.count_ones() % 2 == bits[d0 + 32].1 as u32;
            out.push(pk(bits[d0].0, bits[d0 + 32].0, Kind::Data, d, None, ok));
            j += 34;
        } else {
            j += 1;
        }
        i = j;
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// JTAG

/// The 16 states of the IEEE 1149.1 TAP controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapState {
    TestLogicReset,
    RunTestIdle,
    SelectDrScan,
    CaptureDr,
    ShiftDr,
    Exit1Dr,
    PauseDr,
    Exit2Dr,
    UpdateDr,
    SelectIrScan,
    CaptureIr,
    ShiftIr,
    Exit1Ir,
    PauseIr,
    Exit2Ir,
    UpdateIr,
}

impl TapState {
    /// The state after a rising TCK edge with `tms` (IEEE 1149.1 figure 6-1).
    pub fn next(self, tms: bool) -> TapState {
        use TapState::*;
        match (self, tms) {
            (TestLogicReset, false) => RunTestIdle,
            (TestLogicReset, true) => TestLogicReset,
            (RunTestIdle, false) => RunTestIdle,
            (RunTestIdle, true) => SelectDrScan,
            (SelectDrScan, false) => CaptureDr,
            (SelectDrScan, true) => SelectIrScan,
            (CaptureDr, false) | (ShiftDr, false) | (Exit2Dr, false) => ShiftDr,
            (CaptureDr, true) | (ShiftDr, true) => Exit1Dr,
            (Exit1Dr, false) | (PauseDr, false) => PauseDr,
            (Exit1Dr, true) | (Exit2Dr, true) => UpdateDr,
            (PauseDr, true) => Exit2Dr,
            (UpdateDr, false) | (UpdateIr, false) => RunTestIdle,
            (UpdateDr, true) | (UpdateIr, true) => SelectDrScan,
            (SelectIrScan, false) => CaptureIr,
            (SelectIrScan, true) => TestLogicReset,
            (CaptureIr, false) | (ShiftIr, false) | (Exit2Ir, false) => ShiftIr,
            (CaptureIr, true) | (ShiftIr, true) => Exit1Ir,
            (Exit1Ir, false) | (PauseIr, false) => PauseIr,
            (Exit1Ir, true) | (Exit2Ir, true) => UpdateIr,
            (PauseIr, true) => Exit2Ir,
        }
    }

    pub fn name(self) -> &'static str {
        use TapState::*;
        match self {
            TestLogicReset => "Test-Logic-Reset",
            RunTestIdle => "Run-Test/Idle",
            SelectDrScan => "Select-DR-Scan",
            CaptureDr => "Capture-DR",
            ShiftDr => "Shift-DR",
            Exit1Dr => "Exit1-DR",
            PauseDr => "Pause-DR",
            Exit2Dr => "Exit2-DR",
            UpdateDr => "Update-DR",
            SelectIrScan => "Select-IR-Scan",
            CaptureIr => "Capture-IR",
            ShiftIr => "Shift-IR",
            Exit1Ir => "Exit1-IR",
            PauseIr => "Pause-IR",
            Exit2Ir => "Exit2-IR",
            UpdateIr => "Update-IR",
        }
    }
}

/// One complete IR or DR scan (Capture .. Update, pauses included): the bits in shift order (first = LSB).
#[derive(Clone, Debug, PartialEq)]
pub struct JtagShift {
    pub ir: bool,
    /// time of each bit (rising TCK edge)
    pub t: Vec<f64>,
    pub tdi: Vec<bool>,
    /// empty without a TDO channel
    pub tdo: Vec<bool>,
}

/// Walks the TAP controller: state changes (with the time of the edge) and completed scans. Without a known `start`
/// state the walk begins after 5 consecutive TMS-high edges (which reach Test-Logic-Reset from any state).
fn jtag_walk(tck: &[f32], tms: &[f32], tdi: Option<&[f32]>, tdo: Option<&[f32]>, start: Option<TapState>, thr: f32, hyst: f32) -> (Vec<(f64, TapState)>, Vec<JtagShift>) {
    let ms = states(tms, thr, hyst);
    let di = tdi.map(|v| states(v, thr, hyst));
    let dout = tdo.map(|v| states(v, thr, hyst));
    let mut st = start;
    let mut trans: Vec<(f64, TapState)> = start.map(|s| (0.0, s)).into_iter().collect();
    let mut shifts = Vec::new();
    let mut cur: Option<JtagShift> = None;
    let mut ones = 0;
    for t in rising(tck, thr, hyst) {
        let Some(m) = at(&ms, t) else { continue };
        let Some(s) = st else {
            ones = if m { ones + 1 } else { 0 };
            if ones >= 5 {
                st = Some(TapState::TestLogicReset);
                trans.push((t, TapState::TestLogicReset));
            }
            continue;
        };
        if matches!(s, TapState::ShiftDr | TapState::ShiftIr) {
            if let Some(c) = cur.as_mut() {
                c.t.push(t);
                c.tdi.push(di.as_ref().and_then(|d| at(d, t)).unwrap_or(false));
                if let Some(d) = &dout {
                    c.tdo.push(at(d, t).unwrap_or(false));
                }
            }
        }
        let ns = s.next(m);
        if ns != s {
            trans.push((t, ns));
            match ns {
                TapState::CaptureDr | TapState::CaptureIr => {
                    cur = Some(JtagShift { ir: ns == TapState::CaptureIr, t: Vec::new(), tdi: Vec::new(), tdo: Vec::new() });
                }
                TapState::UpdateDr | TapState::UpdateIr | TapState::TestLogicReset => {
                    if let Some(c) = cur.take() {
                        if !c.t.is_empty() {
                            shifts.push(c);
                        }
                    }
                }
                _ => {}
            }
        }
        st = Some(ns);
    }
    (trans, shifts)
}

/// TAP controller states of a JTAG capture: (time of the TCK edge, new state) for every change (`start` at t = 0
/// when given). See `jtag` for `start`.
pub fn jtag_states(tck: &[f32], tms: &[f32], start: Option<TapState>, thr: f32, hyst: f32) -> Vec<(f64, TapState)> {
    jtag_walk(tck, tms, None, None, start, thr, hyst).0
}

/// All IR/DR scans with their full length (`tdi.len()` bits).
pub fn jtag_shifts(tck: &[f32], tms: &[f32], tdi: &[f32], tdo: Option<&[f32]>, start: Option<TapState>, thr: f32, hyst: f32) -> Vec<JtagShift> {
    jtag_walk(tck, tms, Some(tdi), tdo, start, thr, hyst).1
}

/// JTAG (IEEE 1149.1 TAP): TMS, TDI and TDO sampled on the rising TCK edge, the TAP controller tracked from `start`
/// (None = unknown until 5 TMS-high cycles). Emits `Field("!TLR")` on entering Test-Logic-Reset and per scan
/// `Field("IR")` / `Field("DR")`: value = TDI bits, value2 = TDO bits (None without TDO), first shifted bit = LSB.
/// Scans longer than 32 bits are split into 32-bit packets, the continuation packets labelled "IR+" / "DR+". The
/// bit count of a scan is in `jtag_shifts`.
pub fn jtag(tck: &[f32], tms: &[f32], tdi: &[f32], tdo: Option<&[f32]>, start: Option<TapState>, thr: f32, hyst: f32) -> Vec<Packet> {
    let (trans, shifts) = jtag_walk(tck, tms, Some(tdi), tdo, start, thr, hyst);
    let mut out: Vec<Packet> = trans.iter().filter(|x| x.1 == TapState::TestLogicReset && !(x.0 == 0.0 && start.is_some())).map(|x| tag(x.0, x.0, "!TLR", true)).collect();
    let val = |b: &[bool]| b.iter().enumerate().fold(0u32, |v, (k, &x)| v | (x as u32) << k);
    for s in &shifts {
        for (c, ch) in s.t.chunks(32).enumerate() {
            let a = c * 32;
            let label = match (s.ir, c > 0) {
                (true, false) => "IR",
                (true, true) => "IR+",
                (false, false) => "DR",
                (false, true) => "DR+",
            };
            let tdo = if s.tdo.is_empty() { None } else { Some(val(&s.tdo[a..a + ch.len()])) };
            out.push(field(ch[0], ch[ch.len() - 1], label, val(&s.tdi[a..a + ch.len()]), tdo, true));
        }
    }
    out.sort_by(|a, b| a.t0.total_cmp(&b.t0));
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Quad-SPI

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
    pub const fn new(cmd_lines: u8, addr_lines: u8, data_lines: u8, addr_bytes: u8, dummy_cycles: u32, mode: bool) -> QspiCfg {
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
        0x01 | 0x04 | 0x05 | 0x06 | 0x11 | 0x15 | 0x31 | 0x35 | 0x60 | 0x66 | 0x75 | 0x7a | 0x99 | 0x9f | 0xb7 | 0xb9 | 0xc7 | 0xe9 | 0xf5 | 0xff => c(0, 1, 1, false, 0),
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
    Some(QspiCfg { cmd_lines: 4, addr_lines: 4, data_lines: 4, addr_bytes: cfg.addr_bytes, dummy_cycles: dummy, mode: false })
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
        let cmd_l = if auto { if qpi { 4 } else { 1 } } else { lines(cfg.cmd_lines) };
        let Some((op, t0, t1)) = take(cmd_l, 8, &mut k) else { continue };
        let op = op as u8;
        out.push(field(t0, t1, "CMD", op as u32, None, true));
        let f = if auto { qspi_opcode_cfg(op, qpi, if ab4 { 4 } else { 3 }, cfg.dummy_cycles).unwrap_or(*cfg) } else { *cfg };
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
            out.push(field(t0, t1, "ADDR", v, None, true));
        }
        if f.mode {
            let Some((v, t0, t1)) = take(al, 8, &mut k) else { continue };
            out.push(field(t0, t1, "MODE", v, None, true));
        }
        if f.dummy_cycles > 0 {
            let n = f.dummy_cycles as usize;
            if k + n > cyc.len() {
                continue;
            }
            out.push(field(cyc[k].0, cyc[k + n - 1].0, "DUMMY", f.dummy_cycles, None, true));
            k += n;
        }
        if dl == 1 {
            while k + 8 <= cyc.len() {
                let c = &cyc[k..k + 8];
                let mo = c.iter().fold(0u32, |v, x| v << 1 | (x.1 & 1));
                let mi = c.iter().fold(0u32, |v, x| v << 1 | (x.1 >> 1 & 1));
                out.push(pk(c[0].0, c[7].0, Kind::Data, mo, Some(mi), true));
                k += 8;
            }
        } else {
            while let Some((v, t0, t1)) = take(dl, 8, &mut k) {
                out.push(pk(t0, t1, Kind::Data, v, None, true));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// I3C

/// Common Command Code names (MIPI I3C Basic v1.1.1, table 16).
pub fn i3c_ccc_name(code: u8) -> &'static str {
    match code {
        0x00 => "ENEC",
        0x01 => "DISEC",
        0x02 => "ENTAS0",
        0x03 => "ENTAS1",
        0x04 => "ENTAS2",
        0x05 => "ENTAS3",
        0x06 => "RSTDAA",
        0x07 => "ENTDAA",
        0x08 => "DEFTGTS",
        0x09 => "SETMWL",
        0x0a => "SETMRL",
        0x0b => "ENTTM",
        0x12 => "ENDXFER",
        0x20..=0x27 => "ENTHDR",
        0x28 => "SETXTIME",
        0x29 => "SETAASA",
        0x2a => "RSTACT",
        0x2b => "DEFGRPA",
        0x2c => "RSTGRPA",
        0x80 => "ENEC (direct)",
        0x81 => "DISEC (direct)",
        0x82..=0x85 => "ENTAS (direct)",
        0x86 => "RSTDAA (direct)",
        0x87 => "SETDASA",
        0x88 => "SETNEWDA",
        0x89 => "SETMWL (direct)",
        0x8a => "SETMRL (direct)",
        0x8b => "GETMWL",
        0x8c => "GETMRL",
        0x8d => "GETPID",
        0x8e => "GETBCR",
        0x8f => "GETDCR",
        0x90 => "GETSTATUS",
        0x91 => "GETACCCR",
        0x92 => "ENDXFER (direct)",
        0x93 => "SETBRGTGT",
        0x94 => "GETMXDS",
        0x95 => "GETCAPS",
        0x9a => "RSTACT (direct)",
        _ => "?",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum I3cPh {
    Idle,
    Addr,
    Ccc,
    Bytes { read: bool, i3c: bool },
    Pid,
    Da,
}

struct I3c {
    out: Vec<Packet>,
    ph: I3cPh,
    buf: Vec<(f64, bool)>,
    in_frame: bool,
    after_start: bool,
    daa: bool,
    ccc: Option<u8>,
    known: Vec<u8>,
    user: Vec<u8>,
    hdr: bool,
}

impl I3c {
    fn bit(&mut self, t: f64, b: bool) {
        let need = match self.ph {
            I3cPh::Idle => return,
            I3cPh::Pid => 64,
            _ => 9,
        };
        self.buf.push((t, b));
        if self.buf.len() < need {
            return;
        }
        let u = std::mem::take(&mut self.buf);
        let (t0, t1) = (u[0].0, u[u.len() - 1].0);
        let v8 = msb(&u[..8]) as u32;
        let ninth = u.get(8).map_or(false, |x| x.1);
        let t_odd = (v8.count_ones() + ninth as u32) % 2 == 1;
        match self.ph {
            I3cPh::Idle => {}
            I3cPh::Addr => {
                let (addr, rnw, ack) = (v8 >> 1, v8 & 1 == 1, !ninth);
                let known = self.known.contains(&(addr as u8));
                if addr == 0x7e && rnw && self.daa {
                    self.out.push(pk(t0, t1, Kind::Addr { read: true }, addr, None, ack));
                    self.ph = if ack { I3cPh::Pid } else { I3cPh::Idle };
                } else if addr == 0x7e && !rnw {
                    self.out.push(pk(t0, t1, Kind::Addr { read: false }, addr, None, ack));
                    self.ccc = None;
                    self.daa = false;
                    self.ph = if ack { I3cPh::Ccc } else { I3cPh::Idle };
                } else if self.after_start && rnw && known {
                    // a target's in-band interrupt request won the address arbitration after START
                    self.out.push(field(t0, t1, "IBI", addr, None, ack));
                    self.ph = if ack { I3cPh::Bytes { read: true, i3c: true } } else { I3cPh::Idle };
                } else {
                    let i3c = known || self.ccc.map_or(false, |c| c >= 0x80);
                    self.out.push(pk(t0, t1, Kind::Addr { read: rnw }, addr, None, ack));
                    self.ph = if ack || i3c { I3cPh::Bytes { read: rnw, i3c } } else { I3cPh::Idle };
                    if !ack && i3c {
                        self.ph = I3cPh::Idle;
                    }
                }
            }
            I3cPh::Ccc => {
                self.out.push(field(t0, t1, "CCC", v8, None, t_odd));
                self.ccc = Some(v8 as u8);
                self.ph = I3cPh::Bytes { read: false, i3c: true };
                match v8 {
                    0x07 => self.daa = true,
                    0x06 => self.known = self.user.clone(),
                    0x20..=0x27 => {
                        self.out.push(field(t1, t1, "!HDR", v8 & 7, None, true));
                        self.hdr = true;
                        self.ph = I3cPh::Idle;
                    }
                    _ => {}
                }
            }
            I3cPh::Bytes { read, i3c } => {
                if !i3c {
                    self.out.push(pk(t0, t1, Kind::Data, v8, None, !ninth));
                } else if read {
                    self.out.push(pk(t0, t1, Kind::Data, v8, None, true));
                    if !ninth {
                        // T-bit low: the target ends the read
                        self.out.push(tag(t1, t1, "!EOD", true));
                        self.ph = I3cPh::Idle;
                    }
                } else if matches!(self.ccc, Some(0x87 | 0x88)) {
                    // SETDASA / SETNEWDA: the new dynamic address
                    let da = v8 >> 1;
                    self.out.push(field(t0, t1, "DA", da, None, t_odd));
                    if t_odd && !self.known.contains(&(da as u8)) {
                        self.known.push(da as u8);
                    }
                } else {
                    self.out.push(pk(t0, t1, Kind::Data, v8, None, t_odd));
                }
            }
            I3cPh::Pid => {
                let pid = msb(&u[..48]);
                self.out.push(field(t0, u[47].0, "PID", pid as u32, Some((pid >> 32) as u32), true));
                self.out.push(field(u[48].0, u[55].0, "BCR", msb(&u[48..56]) as u32, None, true));
                self.out.push(field(u[56].0, t1, "DCR", msb(&u[56..64]) as u32, None, true));
                self.ph = I3cPh::Da;
            }
            I3cPh::Da => {
                // 7-bit dynamic address, odd parity bit, target ACK
                let da = v8 >> 1;
                let ok = (v8.count_ones() % 2 == 1) && !ninth;
                self.out.push(field(t0, t1, "DA", da, None, ok));
                if ok && !self.known.contains(&(da as u8)) {
                    self.known.push(da as u8);
                }
                self.ph = I3cPh::Idle;
            }
        }
    }
}

/// I3C SDR (MIPI I3C Basic v1.1.1): START/Sr/STOP and bits like I2C (SDA sampled on the rising SCL edge, MSB first).
/// Emits Start/RepStart/Stop, `Kind::Addr { read }` (ok = ACK) incl. the broadcast address 0x7E, `Field("CCC")`
/// (value = code, see `i3c_ccc_name`; ok = T-bit odd parity), `Kind::Data` (I3C write: ok = T-bit odd parity; I3C
/// read: ok, followed by `Field("!EOD")` when the target ends the read with T = 0; I2C: ok = ACK). ENTDAA: per
/// target `Field("PID")` (48-bit provisioned ID: low 32 bits in value, high 16 in value2), `Field("BCR")`,
/// `Field("DCR")` and `Field("DA")` (assigned address, ok = parity and ACK); SETDASA/SETNEWDA give `Field("DA")` too.
/// ENTHDRx gives `Field("!HDR")` (value = x) and the decoder skips the HDR traffic until the HDR exit pattern (4 SDA
/// falling edges while SCL stays low), marked `Field("!HDREXIT")`.
///
/// Addresses use T-bits (I3C) when they were assigned in the capture or are listed in `i3c_addrs`, and inside direct
/// CCCs; all others are decoded as I2C with ACK bits. A read of a known I3C address directly after START (not Sr) is
/// shown as `Field("IBI")` (the target won the arbitration with an in-band interrupt request).
pub fn i3c(sda: &[f32], scl: &[f32], i3c_addrs: &[u8], thr: f32, hyst: f32) -> Vec<Packet> {
    let scl_s = states(scl, thr, hyst);
    let sda_s = states(sda, thr, hyst);
    // 0 = SCL rising, 1 = SDA falling, 2 = SDA rising, 3 = SCL falling
    let mut ev: Vec<(f64, u8)> = edges(scl, thr, hyst).iter().map(|e| (e.t, if e.rising { 0 } else { 3 })).collect();
    ev.extend(edges(sda, thr, hyst).iter().map(|e| (e.t, if e.rising { 2 } else { 1 })));
    ev.sort_by(|a, b| a.0.total_cmp(&b.0));
    let high = |s: &[Option<bool>], t: f64| at(s, t).unwrap_or(true);
    let mut d = I3c { out: Vec::new(), ph: I3cPh::Idle, buf: Vec::new(), in_frame: false, after_start: false, daa: false, ccc: None, known: i3c_addrs.to_vec(), user: i3c_addrs.to_vec(), hdr: false };
    let mut exit_cnt = 0;
    for (t, k) in ev {
        if d.hdr {
            match k {
                0 | 3 => exit_cnt = 0,
                1 if !high(&scl_s, t) => {
                    exit_cnt += 1;
                    if exit_cnt == 4 {
                        d.hdr = false;
                        d.ph = I3cPh::Idle;
                        d.out.push(tag(t, t, "!HDREXIT", true));
                    }
                }
                _ => {}
            }
            continue;
        }
        match k {
            1 if high(&scl_s, t) => {
                d.out.push(pk(t, t, if d.in_frame { Kind::RepStart } else { Kind::Start }, 0, None, true));
                d.after_start = !d.in_frame;
                d.in_frame = true;
                d.ph = I3cPh::Addr;
                d.buf.clear();
            }
            2 if high(&scl_s, t) => {
                d.out.push(pk(t, t, Kind::Stop, 0, None, true));
                d.in_frame = false;
                d.daa = false;
                d.ccc = None;
                d.ph = I3cPh::Idle;
                d.buf.clear();
            }
            0 => {
                let b = high(&sda_s, t);
                d.bit(t, b);
                if d.hdr {
                    exit_cnt = 0;
                }
            }
            _ => {}
        }
    }
    d.out
}

// ---------------------------------------------------------------------------------------------------------------
// Biphase-mark helpers (S/PDIF, USB PD)

fn edge_times(v: &[f32], thr: f32, hyst: f32) -> Vec<f64> {
    edges(v, thr, hyst).into_iter().map(|e| e.t).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// S/PDIF

/// S/PDIF / AES3 (IEC 60958-1/-3/-4): biphase-mark coded subframes of 32 time slots. The unit interval (half a time
/// slot) is recovered from the shortest pulses, the preambles B (Z, block start, channel A), M (X, channel A) and W
/// (Y, channel B) are found by their 3-UI pulses. Emits `Field("BLK")` at every B preamble (value = block count) and
/// one `Kind::Sample { right }` per subframe (`right` = W preamble): value = the 24 bits of slots 4..27 (aux + audio,
/// LSB first; 20-bit audio = value >> 4), value2 = V | U << 1 | C << 2 | P << 3, ok = even parity and V = 0.
pub fn spdif(v: &[f32], thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let e = edge_times(v, thr, hyst);
    if e.len() < 64 {
        return out;
    }
    let d: Vec<f64> = e.windows(2).map(|w| w[1] - w[0]).collect();
    let mut s = d.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    let cand = s[s.len() / 200];
    let near: Vec<f64> = s.iter().copied().filter(|&x| x >= 0.7 * cand && x <= 1.35 * cand).collect();
    let ui = near.iter().sum::<f64>() / near.len().max(1) as f64;
    if !(ui > 0.0) {
        return out;
    }
    let u: Vec<u8> = d
        .iter()
        .map(|&x| match x / ui {
            r if r < 0.5 => 0,
            r if r < 1.5 => 1,
            r if r < 2.5 => 2,
            r if r < 3.5 => 3,
            _ => 0,
        })
        .collect();
    let mut blk = 0;
    let mut i = 0;
    while i + 4 <= u.len() {
        let right = match u[i..i + 4] {
            [3, 1, 1, 3] => None,
            [3, 3, 1, 1] => Some(false),
            [3, 2, 1, 2] => Some(true),
            _ => {
                i += 1;
                continue;
            }
        };
        // 28 biphase-mark bits: 2 UI = 0, 1 + 1 UI = 1
        let (mut j, mut bits, mut n) = (i + 4, 0u32, 0);
        while n < 28 && j < u.len() {
            match u[j] {
                2 => j += 1,
                1 if u.get(j + 1) == Some(&1) => {
                    bits |= 1 << n;
                    j += 2;
                }
                _ => break,
            }
            n += 1;
        }
        if n < 28 {
            i += 1;
            continue;
        }
        if right.is_none() {
            out.push(field(e[i], e[i + 4], "BLK", blk, None, true));
            blk += 1;
        }
        let vflag = bits >> 24 & 1 == 1;
        out.push(pk(e[i], e[j], Kind::Sample { right: right == Some(true) }, bits & 0xff_ffff, Some(bits >> 24), bits.count_ones() % 2 == 0 && !vflag));
        i = j;
    }
    out
}

/// The 192 channel-status bits (channel A) of the first complete block in `spdif` packets: bit n in byte n / 8, bit
/// n % 8 (byte 0 bit 0 = professional flag, the first transmitted bit).
pub fn spdif_channel_status(p: &[Packet]) -> Option<[u8; 24]> {
    let b = p.iter().position(|x| matches!(x.kind, Kind::Field("BLK")))?;
    let mut cs = [0u8; 24];
    let mut n = 0;
    for x in &p[b + 1..] {
        if x.kind == (Kind::Sample { right: false }) {
            let c = x.value2.unwrap_or(0) >> 2 & 1;
            cs[n / 8] |= (c as u8) << (n % 8);
            n += 1;
            if n == 192 {
                return Some(cs);
            }
        }
    }
    None
}

/// Sample rate from the channel status (IEC 60958-3 consumer: byte 3 bits 0..3; IEC 60958-4 professional: byte 0
/// bits 6..7). None when not indicated.
pub fn spdif_sample_rate(cs: &[u8; 24]) -> Option<u32> {
    if cs[0] & 1 == 1 {
        return match cs[0] >> 6 {
            1 => Some(48000),
            2 => Some(44100),
            3 => Some(32000),
            _ => None,
        };
    }
    match cs[3] & 0x0f {
        0 => Some(44100),
        2 => Some(48000),
        3 => Some(32000),
        4 => Some(22050),
        6 => Some(24000),
        8 => Some(88200),
        9 => Some(768000),
        10 => Some(96000),
        12 => Some(176400),
        14 => Some(192000),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// USB Power Delivery

/// 4b5b symbols of USB PD (r3.1 table 5-1) as 5-bit numbers, first transmitted bit = LSB.
pub const PD_4B5B: [u8; 16] = [0x1e, 0x09, 0x14, 0x15, 0x0a, 0x0b, 0x0e, 0x0f, 0x12, 0x13, 0x16, 0x17, 0x1a, 0x1b, 0x1c, 0x1d];
pub const PD_SYNC1: u8 = 0x18;
pub const PD_SYNC2: u8 = 0x11;
pub const PD_SYNC3: u8 = 0x06;
pub const PD_RST1: u8 = 0x07;
pub const PD_RST2: u8 = 0x19;
pub const PD_EOP: u8 = 0x0d;

/// Ordered sets (start of packet / resets): label, K-codes.
const PD_SETS: [(&str, [u8; 4]); 7] = [
    ("SOP", [PD_SYNC1, PD_SYNC1, PD_SYNC1, PD_SYNC2]),
    ("SOP'", [PD_SYNC1, PD_SYNC1, PD_SYNC3, PD_SYNC3]),
    ("SOP''", [PD_SYNC1, PD_SYNC3, PD_SYNC1, PD_SYNC3]),
    ("SOP'D", [PD_SYNC1, PD_RST2, PD_RST2, PD_SYNC3]),
    ("SOP''D", [PD_SYNC1, PD_RST2, PD_SYNC3, PD_SYNC2]),
    ("!HRST", [PD_RST1, PD_RST1, PD_RST1, PD_RST2]),
    ("!CRST", [PD_RST1, PD_SYNC1, PD_RST1, PD_SYNC3]),
];

/// CRC-32 of USB PD (IEEE 802.3: reflected polynomial 0xEDB88320, init and final XOR 0xFFFFFFFF).
pub fn usbpd_crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { c >> 1 ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Message type name from a USB PD message header (r3.1 tables 6-5 control, 6-6 data, 6-53 extended).
pub fn usbpd_msg_name(hdr: u16) -> &'static str {
    let t = hdr & 0x1f;
    let ndo = hdr >> 12 & 7;
    if hdr & 0x8000 != 0 {
        return match t {
            1 => "Source_Capabilities_Extended",
            2 => "Status",
            3 => "Get_Battery_Cap",
            4 => "Get_Battery_Status",
            5 => "Battery_Capabilities",
            6 => "Get_Manufacturer_Info",
            7 => "Manufacturer_Info",
            8 => "Security_Request",
            9 => "Security_Response",
            10 => "Firmware_Update_Request",
            11 => "Firmware_Update_Response",
            12 => "PPS_Status",
            13 => "Country_Info",
            14 => "Country_Codes",
            15 => "Sink_Capabilities_Extended",
            16 => "Extended_Control",
            17 => "EPR_Source_Capabilities",
            18 => "EPR_Sink_Capabilities",
            30 => "Vendor_Defined_Extended",
            _ => "Reserved",
        };
    }
    if ndo == 0 {
        match t {
            1 => "GoodCRC",
            2 => "GotoMin",
            3 => "Accept",
            4 => "Reject",
            5 => "Ping",
            6 => "PS_RDY",
            7 => "Get_Source_Cap",
            8 => "Get_Sink_Cap",
            9 => "DR_Swap",
            10 => "PR_Swap",
            11 => "VCONN_Swap",
            12 => "Wait",
            13 => "Soft_Reset",
            14 => "Data_Reset",
            15 => "Data_Reset_Complete",
            16 => "Not_Supported",
            17 => "Get_Source_Cap_Extended",
            18 => "Get_Status",
            19 => "FR_Swap",
            20 => "Get_PPS_Status",
            21 => "Get_Country_Codes",
            22 => "Get_Sink_Cap_Extended",
            23 => "Get_Source_Info",
            24 => "Get_Revision",
            _ => "Reserved",
        }
    } else {
        match t {
            1 => "Source_Capabilities",
            2 => "Request",
            3 => "BIST",
            4 => "Sink_Capabilities",
            5 => "Battery_Status",
            6 => "Alert",
            7 => "Get_Country_Info",
            8 => "Enter_USB",
            9 => "EPR_Request",
            10 => "EPR_Mode",
            11 => "Source_Info",
            12 => "Revision",
            15 => "Vendor_Defined",
            _ => "Reserved",
        }
    }
}

/// A decoded power data object (USB PD r3.1 section 6.4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pdo {
    Fixed { volts: f64, amps: f64 },
    Battery { min_v: f64, max_v: f64, watts: f64 },
    Variable { min_v: f64, max_v: f64, amps: f64 },
    /// SPR programmable power supply (augmented PDO)
    Pps { min_v: f64, max_v: f64, amps: f64 },
    Other(u32),
}

/// Decodes a source/sink capability PDO.
pub fn usbpd_pdo(pdo: u32) -> Pdo {
    let f = |sh: u32, bits: u32| (pdo >> sh & ((1 << bits) - 1)) as f64;
    match pdo >> 30 {
        0 => Pdo::Fixed { volts: f(10, 10) * 0.05, amps: f(0, 10) * 0.01 },
        1 => Pdo::Battery { min_v: f(10, 10) * 0.05, max_v: f(20, 10) * 0.05, watts: f(0, 10) * 0.25 },
        2 => Pdo::Variable { min_v: f(10, 10) * 0.05, max_v: f(20, 10) * 0.05, amps: f(0, 10) * 0.01 },
        _ if pdo >> 28 & 3 == 0 => Pdo::Pps { min_v: f(8, 8) * 0.1, max_v: f(17, 8) * 0.1, amps: f(0, 7) * 0.05 },
        _ => Pdo::Other(pdo),
    }
}

/// One BMC burst decoded to bits (t0, t1, bit).
fn bmc_bits(e: &[f64], start: usize, bit: f64) -> (Vec<(f64, f64, bool)>, usize) {
    let mut bits = Vec::new();
    let mut tb = bit;
    let mut j = start;
    loop {
        let Some(&t1) = e.get(j + 1) else {
            // last edge of the capture: start of a final 0 bit
            if j < e.len() && !bits.is_empty() {
                bits.push((e[j], e[j] + tb, false));
            }
            j += 1;
            break;
        };
        let d = t1 - e[j];
        if d > 1.6 * tb {
            // idle after the burst: the last edge starts a final 0 bit (EOP ends with 0, then the line holds)
            if !bits.is_empty() {
                bits.push((e[j], e[j] + tb, false));
            }
            j += 1;
            break;
        } else if d > 0.75 * tb {
            bits.push((e[j], t1, false));
            tb = 0.9 * tb + 0.1 * d;
            j += 1;
        } else if d > 0.25 * tb {
            match e.get(j + 2) {
                Some(&t2) if t2 - t1 > 0.25 * tb && t2 - t1 <= 0.75 * tb => {
                    bits.push((e[j], t2, true));
                    tb = 0.9 * tb + 0.1 * (t2 - e[j]);
                    j += 2;
                }
                Some(&t2) if t2 - t1 > 1.6 * tb => {
                    // a final 1 whose cell ends low without another edge (e.g. RST-2 at the end of Hard Reset)
                    bits.push((e[j], 2.0 * t1 - e[j], true));
                    j += 2;
                    break;
                }
                None => {
                    bits.push((e[j], 2.0 * t1 - e[j], true));
                    j += 2;
                    break;
                }
                _ => {
                    j += 1;
                    break;
                }
            }
        } else {
            j += 1;
            break;
        }
    }
    (bits, j)
}

/// One burst's bits -> packets.
fn pd_frame(b: &[(f64, f64, bool)], out: &mut Vec<Packet>) {
    let n = b.len();
    let Some(brk) = (0..n.saturating_sub(1)).find(|&k| b[k].2 == b[k + 1].2) else { return };
    let sym = |s: usize| -> Option<u8> { (s + 5 <= n).then(|| (0..5).fold(0u8, |v, k| v | (b[s + k].2 as u8) << k)) };
    let nib = |s: usize| -> Option<u32> { sym(s).and_then(|x| PD_4B5B.iter().position(|&c| c == x)).map(|p| p as u32) };
    // the preamble (alternating, ends with 1) runs into the first K-code: try both possible boundaries
    let mut best: Option<(usize, usize, u32)> = None; // (start, set, matching K-codes)
    for s in [brk, brk + 1] {
        if s < 8 {
            continue;
        }
        let Some(k) = (0..4).map(|q| sym(s + 5 * q)).collect::<Option<Vec<u8>>>() else { continue };
        for (si, set) in PD_SETS.iter().enumerate() {
            let m = (0..4).filter(|&q| k[q] == set.1[q]).count() as u32;
            if m >= 3 && best.map_or(true, |x| m > x.2) {
                best = Some((s, si, m));
            }
        }
    }
    let Some((s, si, m)) = best else { return };
    out.push(field(b[0].0, b[s - 1].1, "PRE", s as u32, None, true));
    let (label, _) = PD_SETS[si];
    out.push(tag(b[s].0, b[s + 19].1, label, m == 4));
    if label.starts_with('!') {
        return;
    }
    let mut p = s + 20;
    let word = |p: &mut usize, nibbles: usize| -> Option<(u32, f64, f64)> {
        let t0 = b.get(*p)?.0;
        let mut v = 0u32;
        for q in 0..nibbles {
            v |= nib(*p)? << (4 * q);
            *p += 5;
        }
        Some((v, t0, b[*p - 1].1))
    };
    let mut bytes = Vec::new();
    let Some((hdr, t0, t1)) = word(&mut p, 4) else {
        out.push(tag(b[p.min(n - 1)].0, b[n - 1].1, "!ERR", false));
        return;
    };
    out.push(field(t0, t1, "HDR", hdr, None, true));
    bytes.extend_from_slice(&(hdr as u16).to_le_bytes());
    for _ in 0..(hdr >> 12 & 7) {
        let Some((d, t0, t1)) = word(&mut p, 8) else {
            out.push(tag(b[p.min(n - 1)].0, b[n - 1].1, "!ERR", false));
            return;
        };
        out.push(field(t0, t1, "PDO", d, None, true));
        bytes.extend_from_slice(&d.to_le_bytes());
    }
    let Some((crc, t0, t1)) = word(&mut p, 8) else {
        out.push(tag(b[p.min(n - 1)].0, b[n - 1].1, "!ERR", false));
        return;
    };
    out.push(pk(t0, t1, Kind::Crc, crc, None, crc == usbpd_crc32(&bytes)));
    match sym(p) {
        Some(PD_EOP) => out.push(tag(b[p].0, b[p + 4].1, "!EOP", true)),
        _ => out.push(tag(b[p.min(n - 1)].0, b[n - 1].1, "!ERR", false)),
    }
}

/// USB Power Delivery (USB PD r3.1 chapter 5) on the CC line: BMC at 300 kbit/s +-10 % (the bit time is tracked
/// from the nominal value), preamble, 4b5b symbols LSB first. Per message: `Field("PRE")` (value = preamble bits),
/// the ordered set `Field("SOP")` / `"SOP'"` / `"SOP''"` / `"SOP'D"` / `"SOP''D"` (ok = all four K-codes right, three
/// suffice), `Field("HDR")` (value = 16-bit header, see `usbpd_msg_name`), `Field("PDO")` per 32-bit data object (see
/// `usbpd_pdo`), `Kind::Crc` (value = received CRC-32, ok = matches) and `Field("!EOP")`; `Field("!HRST")` /
/// `Field("!CRST")` for hard / cable reset, `Field("!ERR")` for an invalid symbol.
pub fn usbpd(cc: &[f32], dt: f64, thr: f32, hyst: f32) -> Vec<Packet> {
    let mut out = Vec::new();
    let bit = 1.0 / 300e3 / dt;
    if !(bit >= 4.0) || !bit.is_finite() {
        return out;
    }
    let e = edge_times(cc, thr, hyst);
    let mut i = 0;
    while i < e.len() {
        let (b, j) = bmc_bits(&e, i, bit);
        if b.len() >= 40 {
            pd_frame(&b, &mut out);
        }
        i = j.max(i + 1);
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------

/// Signal generators for the decoders above (tests, demo source).
pub mod gen {
    use super::{usbpd_crc32, QspiCfg, TapState, PD_4B5B, PD_EOP, PD_SETS};
    use crate::sim::Logic;

    /// `n` bits of `v`, LSB first.
    pub fn bits_lsb(v: u64, n: usize) -> Vec<bool> {
        (0..n).map(|k| k < 64 && v >> k & 1 == 1).collect()
    }

    /// A clock and N data lines: per cycle the lines change with the falling clock edge, the rising edge (half a
    /// period later) samples. The clock idles low.
    fn clocked<const N: usize>(cycles: &[[bool; N]], rate: f64, lo: f64, hi: f64) -> (Logic, Vec<Logic>) {
        let h = 0.5 / rate;
        let mut clk = Logic::new(lo, hi, h / 20.0);
        let mut d: Vec<Logic> = (0..N).map(|_| Logic::new(lo, hi, h / 20.0)).collect();
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
        Xfer { ap: bool, read: bool, a: u8, ack: u8, data: u32, bad_parity: bool },
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
        let (clk, mut d) = clocked(&b.iter().map(|&x| [x]).collect::<Vec<_>>(), rate, lo, hi);
        (clk, d.remove(0))
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
            for k in 0..n {
                if pause_at == Some(k) && k > 0 {
                    // the previous bit already left Shift (TMS 1): Exit1 -> Pause (2 cycles) -> Exit2 -> Shift
                    self.clock(false, false, false);
                    self.clock(false, false, false);
                    self.clock(true, false, false);
                    self.clock(false, false, false);
                }
                let last = k + 1 == n || pause_at == Some(k + 1);
                self.clock(last, tdi[k], tdo.get(k).copied().unwrap_or(false));
            }
            self.clock(true, false, false); // Update
            self.clock(false, false, false); // Run-Test/Idle
        }
        /// [TCK, TMS, TDI, TDO]
        pub fn logic(&self, rate: f64, lo: f64, hi: f64) -> [Logic; 4] {
            let (c, d) = clocked(&self.cycles, rate, lo, hi);
            let mut d = d.into_iter();
            [c, d.next().unwrap(), d.next().unwrap(), d.next().unwrap()]
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
            self.bit((v.count_ones() % 2 == 0) != bad);
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

    /// Biphase-mark half-cells -> logic waveform from `t`: `ui` seconds per half cell; a final transition
    /// ends the last cell, then the line holds.
    fn halves_logic(halves: &[bool], ui: f64, l: &mut Logic, t: &mut f64) {
        for &x in halves {
            l.push(*t, x);
            *t += ui;
        }
        if let Some(&last) = halves.last() {
            l.push(*t, !last);
        }
    }

    /// Biphase-mark coding of `bits` starting from `level`: a transition at every cell start, one more in the middle
    /// of a 1.
    fn bmc(bits: &[bool], level: &mut bool, out: &mut Vec<bool>) {
        for &b in bits {
            *level = !*level;
            out.push(*level);
            if b {
                *level = !*level;
            }
            out.push(*level);
        }
    }

    /// S/PDIF stream of stereo frames (24-bit samples), channel status `cs` (both subframes), V = 0, U = 0, frame 0
    /// starts a block; `bad_parity` = subframe indices (2 frame + channel) with a wrong parity bit; `fs` = sample rate.
    pub fn spdif(frames: &[(u32, u32)], cs: &[u8; 24], fs: f64, bad_parity: &[usize], lo: f64, hi: f64) -> Logic {
        let ui = 1.0 / (128.0 * fs);
        let mut h: Vec<bool> = Vec::new();
        let mut lv = false;
        for (k, &(l, r)) in frames.iter().enumerate() {
            for (ch, s) in [(0usize, l), (1, r)] {
                let pre: u8 = if ch == 1 {
                    0b1110_0100
                } else if k % 192 == 0 {
                    0b1110_1000
                } else {
                    0b1110_0010
                };
                let before = lv;
                for q in (0..8).rev() {
                    lv = (pre >> q & 1 == 1) != before;
                    h.push(lv);
                }
                let c = cs[(k % 192) / 8] >> (k % 8) & 1 == 1;
                let mut bits = bits_lsb(s as u64 & 0xff_ffff, 24);
                bits.extend([false, false, c]);
                let ones = bits.iter().filter(|&&b| b).count();
                bits.push((ones % 2 == 1) != bad_parity.contains(&(2 * k + ch)));
                bmc(&bits, &mut lv, &mut h);
            }
        }
        let mut l = Logic::new(lo, hi, ui / 10.0);
        l.push(0.0, false);
        let mut t = 8.0 * ui;
        halves_logic(&h, ui, &mut l, &mut t);
        l
    }

    /// One USB PD transmission for `usbpd`.
    #[derive(Clone, Debug)]
    pub enum PdFrame {
        /// `sop` = index of the ordered set (0 SOP, 1 SOP', 2 SOP'', 3 SOP'_Debug, 4 SOP''_Debug)
        Msg { sop: usize, header: u16, data: Vec<u32>, bad_crc: bool },
        HardReset,
        CableReset,
    }

    /// The bits of a transmission: preamble (64 alternating bits from 0), ordered set, 4b5b payload with CRC-32, EOP.
    pub fn usbpd_bits(f: &PdFrame) -> Vec<bool> {
        let mut b: Vec<bool> = (0..64).map(|k| k % 2 == 1).collect();
        let sym = |b: &mut Vec<bool>, s: u8| b.extend(bits_lsb(s as u64, 5));
        let set = match f {
            PdFrame::Msg { sop, .. } => (*sop).min(4),
            PdFrame::HardReset => 5,
            PdFrame::CableReset => 6,
        };
        PD_SETS[set].1.iter().for_each(|&k| sym(&mut b, k));
        if let PdFrame::Msg { header, data, bad_crc, .. } = f {
            let mut bytes = header.to_le_bytes().to_vec();
            data.iter().for_each(|d| bytes.extend(d.to_le_bytes()));
            let crc = usbpd_crc32(&bytes) ^ *bad_crc as u32;
            bytes.extend(crc.to_le_bytes());
            for x in bytes {
                sym(&mut b, PD_4B5B[(x & 15) as usize]);
                sym(&mut b, PD_4B5B[(x >> 4) as usize]);
            }
            sym(&mut b, PD_EOP);
        }
        b
    }

    /// USB PD transmissions on CC as BMC at `bitrate`, idle low, 40 bit times between them.
    pub fn usbpd(frames: &[PdFrame], bitrate: f64, lo: f64, hi: f64) -> Logic {
        let ui = 0.5 / bitrate;
        let mut l = Logic::new(lo, hi, ui / 15.0);
        l.push(0.0, false);
        let mut t = 20.0 * ui;
        for f in frames {
            let mut h = Vec::new();
            let mut lv = false;
            bmc(&usbpd_bits(f), &mut lv, &mut h);
            for &x in &h {
                l.push(t, x);
                t += ui;
            }
            // hold low after the last bit (final transition if the line is high)
            l.push(t, false);
            t += 80.0 * ui;
        }
        l
    }
}
