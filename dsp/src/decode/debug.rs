// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The debug ports SWD and JTAG.
//!
//! A new decoder of the family goes into this file, a new family into a file of its own next to it (with its `mod` and
//! `pub use` line in `decode.rs`); the ui reaches it through a protocol module registered in `ui/src/protocols/mod.rs`
//! (ADR 0002).

use super::{at, lsb, rising, Kind, Packet};
use crate::edges::states;
use crate::protocols::ModuleInfo;

/// ARM SWD.
pub static SWD: ModuleInfo = ModuleInfo::new("Swd", "SWD", "debug").roles([Some("SWDIO"), Some("SWCLK"), None, None]);

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
    let bits: Vec<(f64, bool)> =
        rising(swclk, thr, hyst).into_iter().filter_map(|t| at(&io, t).map(|b| (t, b))).collect();
    let n = bits.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if bits[i].1 {
            let run = bits[i..].iter().take_while(|b| b.1).count();
            if run >= 50 {
                out.push(Packet::tag("!RESET", bits[i].0, bits[i + run - 1].0, true));
                i += run;
                continue;
            }
        } else if i + 16 <= n && lsb(&bits[i..i + 16]) == 0xe79e {
            out.push(Packet::tag("!JTAG2SWD", bits[i].0, bits[i + 15].0, true));
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
        out.push(
            Packet::field("REQ", bits[i].0, bits[i + 7].0, a << 2, par_ok).with_value2(ap as u32 | (rnw as u32) << 1),
        );
        // turnaround, ACK
        if i + 12 > n {
            break;
        }
        let ack = lsb(&bits[i + 9..i + 12]);
        out.push(Packet::new(Kind::Ack, bits[i + 9].0, bits[i + 11].0, ack, ack == 1));
        let mut j = i + 12;
        if ack == 1 {
            // read: data, parity, turnaround; write: turnaround, data, parity
            let d0 = if rnw { j } else { j + 1 };
            if d0 + 33 > n {
                break;
            }
            let d = lsb(&bits[d0..d0 + 32]);
            let ok = d.count_ones() % 2 == bits[d0 + 32].1 as u32;
            out.push(Packet::new(Kind::Data, bits[d0].0, bits[d0 + 32].0, d, ok));
            j += 34;
        } else {
            j += 1;
        }
        i = j;
    }
    out
}

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
fn jtag_walk(
    tck: &[f32],
    tms: &[f32],
    tdi: Option<&[f32]>,
    tdo: Option<&[f32]>,
    start: Option<TapState>,
    thr: f32,
    hyst: f32,
) -> (Vec<(f64, TapState)>, Vec<JtagShift>) {
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
                    cur = Some(JtagShift {
                        ir: ns == TapState::CaptureIr,
                        t: Vec::new(),
                        tdi: Vec::new(),
                        tdo: Vec::new(),
                    });
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
pub fn jtag_shifts(
    tck: &[f32],
    tms: &[f32],
    tdi: &[f32],
    tdo: Option<&[f32]>,
    start: Option<TapState>,
    thr: f32,
    hyst: f32,
) -> Vec<JtagShift> {
    jtag_walk(tck, tms, Some(tdi), tdo, start, thr, hyst).1
}

/// JTAG.
pub static JTAG: ModuleInfo =
    ModuleInfo::new("Jtag", "JTAG", "debug").roles([Some("TMS"), Some("TCK"), Some("TDI"), Some("TDO")]);

/// JTAG (IEEE 1149.1 TAP): TMS, TDI and TDO sampled on the rising TCK edge, the TAP controller tracked from `start`
/// (None = unknown until 5 TMS-high cycles). Emits `Field("!TLR")` on entering Test-Logic-Reset and per scan
/// `Field("IR")` / `Field("DR")`: value = TDI bits, value2 = TDO bits (None without TDO), first shifted bit = LSB.
/// Scans longer than 32 bits are split into 32-bit packets, the continuation packets labelled "IR+" / "DR+". The
/// bit count of a scan is in `jtag_shifts`.
pub fn jtag(
    tck: &[f32],
    tms: &[f32],
    tdi: &[f32],
    tdo: Option<&[f32]>,
    start: Option<TapState>,
    thr: f32,
    hyst: f32,
) -> Vec<Packet> {
    let (trans, shifts) = jtag_walk(tck, tms, Some(tdi), tdo, start, thr, hyst);
    let mut out: Vec<Packet> = trans
        .iter()
        .filter(|x| x.1 == TapState::TestLogicReset && !(x.0 == 0.0 && start.is_some()))
        .map(|x| Packet::tag("!TLR", x.0, x.0, true))
        .collect();
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
            let tdi = val(&s.tdi[a..a + ch.len()]);
            out.push(Packet { value2: tdo, ..Packet::field(label, ch[0], ch[ch.len() - 1], tdi, true) });
        }
    }
    out.sort_by(|a, b| a.t0.total_cmp(&b.t0));
    out
}
