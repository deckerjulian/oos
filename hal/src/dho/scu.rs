// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! System control unit (0x4000 block) and the SPU reset bits: arming an acquisition and reading it back.
//! docs/protocols/acquisition-start.md (record path) and adc-spu-init.md (play-last
//! read-out, verified word-exact on the unit 2026-11-02/03).

use crate::dho::Write;

/// Software copies of the write-only registers the sequences modify (the FPGA does not read them back).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shadows {
    pub s4004: u32,
    pub s4008: u32,
    pub s400c: u32,
    pub s1000: u32,
    pub s1010: u32,
    pub s4080: u32,
    /// SPU acquisition control 0x1004 (ADC order, mode bits, per-lane invert/filter)
    pub s1004: u32,
    /// SCU trigger control 0x4020 (bit0 = auto)
    pub s4020: u32,
    /// per-channel sample delays 0x1080 / 0x1084
    pub s1080: u32,
    pub s1084: u32,
}

impl Shadows {
    /// The stock register values at STOP after the hand-over of 2026-11-02 21:12 (CH1, 2.5 Mpts).
    pub const HANDOVER: Shadows =
        Shadows { s4004: 0x1, s4008: 0x80_0001, s400c: 0x6000_0000, s1000: 0, s1010: 0x2000_0000, s4080: 0x4000_0010, s1004: 0x0400_0000, s4020: 0x2001, s1080: 0x01e0_0000, s1084: 0 };
}

/// Read-out window: `n` points from memory point `start`, interleave factor `ratio`, FSM channel mask `fsm`.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub n: u32,
    pub start: u32,
    pub ratio: u32,
    pub fsm: u32,
    /// LA on: the record holds twice the words, so the play length doubles; the start and the TX length stay
    pub la: bool,
}

/// Status bits of 0x4034 (read only).
pub mod status {
    /// FSM stopped.
    pub const STOPPED: u32 = 1 << 13;
    /// waiting for a trigger
    pub const WAIT_TRIG: u32 = 1 << 4;
}

/// 0x4080 bit30: the armed acquisition is complete.
pub const PLAYINFO_DONE: u32 = 1 << 30;

/// Stop bit `f`: three writes of 0x4004.
pub fn set_stop(w: &mut Vec<Write>, f: bool, sh: &mut Shadows) {
    sh.s4004 &= 0x7fff_fffe;
    w.push((0x4004, sh.s4004));
    if f {
        sh.s4004 |= 0x8000_0000;
    }
    w.push((0x4004, sh.s4004));
    sh.s4004 &= 0x7fff_fffb;
    w.push((0x4004, sh.s4004));
}

/// Stop sequence without its status poll (poll 0x4034 bit13 first): stop 0, stop 1, play info clear, SPU reset,
/// TX reset, stop 0; 17 writes.
pub fn stop_sequence(w: &mut Vec<Write>, sh: &mut Shadows) {
    set_stop(w, false, sh);
    set_stop(w, true, sh);
    sh.s4080 &= 0x7fff_ffff;
    w.push((0x4080, sh.s4080));
    sh.s4080 |= 0x8000_0000;
    w.push((0x4080, sh.s4080));
    sh.s4080 &= 0x7fff_ffff;
    w.push((0x4080, sh.s4080));
    sh.s1000 |= 1;
    w.push((0x1000, sh.s1000));
    sh.s1000 &= !1;
    w.push((0x1000, sh.s1000));
    sh.s1000 &= !0x10;
    w.push((0x1000, sh.s1000));
    sh.s1000 |= 0x10;
    w.push((0x1000, sh.s1000));
    sh.s1000 &= !0x10;
    w.push((0x1000, sh.s1000));
    set_stop(w, false, sh);
}

/// Read-out window: channel processing on, LA processing off, window 0x1100/0x1108, TX length 0x11f0,
/// no frame header (0x1010 bit31), record sequence off (0x400c bit25); 7 writes.
fn readout_window(w: &mut Vec<Write>, win: &Window, sh: &mut Shadows) {
    let r4 = (win.n + 3) / 4 * 4;
    sh.s4008 |= 1 << 23;
    w.push((0x4008, sh.s4008));
    sh.s4008 &= !(1 << 24);
    w.push((0x4008, sh.s4008));
    w.push((0x1100, (win.n << win.la as u32) / win.ratio + (0x48 | (win.ratio << 9))));
    w.push((0x1108, win.start / win.ratio));
    let size = if r4 / win.ratio == 8 { 128 } else { r4 / win.ratio };
    w.push((0x11f0, size));
    sh.s1010 |= 1 << 31;
    w.push((0x1010, sh.s1010));
    sh.s400c &= !(1 << 25);
    w.push((0x400c, sh.s400c));
}

/// Play-last read-out (verified word-exact): 28 writes, then ONE read of 2 * n bytes from /dev/xdma0_c2h_0.
pub fn readout(win: &Window, sh: &mut Shadows) -> Vec<Write> {
    let mut w = Vec::new();
    readout_window(&mut w, win, sh);
    stop_sequence(&mut w, sh);
    sh.s4004 &= !4;
    w.push((0x4004, sh.s4004));
    sh.s4004 |= 4;
    w.push((0x4004, sh.s4004));
    sh.s4008 = (sh.s4008 & !0xf) | win.fsm;
    w.push((0x4008, sh.s4008));
    sh.s4004 |= 1;
    w.push((0x4004, sh.s4004));
    w
}

/// Before a run: roll (0x4008 bit17) and scan (bit22) off.
fn set_run_prologue(w: &mut Vec<Write>, sh: &mut Shadows) {
    sh.s4008 &= !(1 << 17);
    w.push((0x4008, sh.s4008));
    sh.s4008 &= !(1 << 22);
    w.push((0x4008, sh.s4008));
}

/// The play-side processing chain to pass-through (compression 0x1170/0x1174 off
/// with k = 1, sinc 0x1130/0x1134 off, linear interpolation 0x1154.. off for main and zoom), as the stock system does
/// before every raw read (record-play.md section 6.1). Without it the live compression (k = 4 at 200 us/div) leaves
/// fewer TX points than 0x11f0 asks for, the frame never ends and the c2h read times out (unit, 2026-11-03).
/// Bits 28-29 of 0x1170/0x1174 and the upper bits of 0x1154 are assumed 0 (no OOS shadow yet). Zoom (0x4008 bit25)
/// is never set by OOS.
pub fn passthrough() -> Vec<Write> {
    vec![
        (0x1170, 0x4000_0001),
        (0x1174, 0x4000_0001),
        (0x1130, 0x0001_0101),
        (0x1134, 0x0001_0101),
        (0x1154, 0x1),
        (0x1158, 0),
        (0x115c, 0xffff_ffff),
        (0x1154, 0x3),
        (0x1160, 0),
        (0x1164, 0xffff_ffff),
    ]
}

/// Read-out with the stock type-4 play.
pub fn readout_play4(win: &Window, sh: &mut Shadows, first: u32, count: u32) -> Vec<Write> {
    let mut w = Vec::new();
    readout_window(&mut w, win, sh);
    stop_sequence(&mut w, sh);
    set_run_prologue(&mut w, sh);
    w.push((0x4068, 0));
    w.push((0x406c, 0));
    sh.s400c = (sh.s400c & 0x89ff_ffff) | 0x3000_0000;
    w.push((0x400c, sh.s400c));
    w.push((0x4010, first));
    w.push((0x4014, first));
    w.push((0x4018, first + count.max(1) - 1));
    sh.s400c &= !0x1000_0000;
    w.push((0x400c, sh.s400c));
    sh.s400c = (sh.s400c & !(1 << 26)) | 0x6000_0000;
    w.push((0x400c, sh.s400c));
    sh.s4008 = (sh.s4008 & !0xf) | win.fsm;
    w.push((0x4008, sh.s4008));
    sh.s4004 |= 1;
    w.push((0x4004, sh.s4004));
    w
}

/// Arm `n` frames from frame `first` (stop sequence, type-2 run, record sequence with position 0, FSM run); 29 writes. Then poll 0x4080 for `PLAYINFO_DONE`.
pub fn arm(fsm: u32, sh: &mut Shadows, first: u32, n: u32) -> Vec<Write> {
    let mut w = Vec::new();
    stop_sequence(&mut w, sh);
    set_run_prologue(&mut w, sh);
    w.push((0x406c, 0));
    w.push((0x4068, 0));
    sh.s400c = (sh.s400c & 0x89ff_ffff) | 0x3000_0000;
    w.push((0x400c, sh.s400c));
    w.push((0x4010, first));
    w.push((0x4014, first));
    w.push((0x4018, first + n.max(1) - 1));
    sh.s400c &= !0x1000_0000;
    w.push((0x400c, sh.s400c));
    sh.s400c |= 0x2200_0000;
    w.push((0x400c, sh.s400c));
    sh.s4008 = (sh.s4008 & !0xf) | fsm;
    w.push((0x4008, sh.s4008));
    sh.s4004 |= 1;
    w.push((0x4004, sh.s4004));
    w
}

/// Auto trigger (`auto`, `t1`, `t2`):
/// 0x4020 bit0 = auto (read-modify-write), 0x4060 = t1, 0x4064 = t2 in 4 ns ticks, with
/// t1 = clamp(20 x time/div, 10 ms, 500 ms), t2 = max(holdoff, clamp(10 x time/div, 100 ms, 250 ms)). NORMAL sweep =
/// the same writes with auto = false.
pub fn auto_trigger(sh: &mut Shadows, auto: bool, tdiv_fs: i128, holdoff_fs: i128) -> Vec<Write> {
    const MS: i128 = 1_000_000_000_000;
    let t1 = (20 * tdiv_fs).clamp(10 * MS, 500 * MS);
    let t2 = holdoff_fs.max((10 * tdiv_fs).clamp(100 * MS, 250 * MS));
    sh.s4020 = (sh.s4020 & !1) | auto as u32;
    vec![(0x4020, sh.s4020), (0x4060, (t1 / 4_000_000) as u32), (0x4064, (t2 / 4_000_000) as u32)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_trigger_200us() {
        // acquisition-start.md: 200 us/div -> 0x4060 = 0x2625a0, 0x4064 = 0x17d7840; shadow 0x2001
        let mut sh = Shadows::HANDOVER;
        assert_eq!(auto_trigger(&mut sh, true, 200_000_000_000, 8_000_000), [(0x4020, 0x2001), (0x4060, 0x2625a0), (0x4064, 0x17d7840)]);
        assert_eq!(auto_trigger(&mut sh, false, 1_000_000_000, 0)[0], (0x4020, 0x2000));
    }

    const WIN: Window = Window { n: 4000, start: 1_248_000, ratio: 1, fsm: 1, la: false };

    fn text(w: &[Write]) -> String {
        w.iter().map(|(o, v)| format!("{:#x} {:#x}\n", o, v)).collect()
    }

    #[test]
    fn readout_matches_the_verified_run() {
        let want = include_str!("../../testdata/second-frame-writes.txt");
        assert_eq!(text(&readout(&WIN, &mut Shadows::HANDOVER.clone())), want);
    }

    #[test]
    fn arm_record_values() {
        let mut sh = Shadows::HANDOVER;
        let w = arm(1, &mut sh, 0, 1);
        assert_eq!(w.len(), 29);
        let c: Vec<u32> = w.iter().filter(|x| x.0 == 0x400c).map(|x| x.1).collect();
        assert_eq!(c, [0x3000_0000, 0x2000_0000, 0x2200_0000]);
        assert_eq!(w.last(), Some(&(0x4004, 1)));
        // the read-out after an arm clears bit25 again
        let r = readout(&WIN, &mut sh);
        assert_eq!(r[6], (0x400c, 0x2000_0000));
    }

    #[test]
    fn play4_ends_in_play_mode() {
        let mut sh = Shadows::HANDOVER;
        arm(1, &mut sh, 0, 1);
        let w = readout_play4(&WIN, &mut sh, 0, 1);
        assert_eq!(w.len(), 36);
        assert!(w.contains(&(0x400c, 0x6000_0000)));
    }
}

#[cfg(test)]
mod la_window_tests {
    use super::*;

    /// Only the play length doubles with the LA on, start and TX length stay.
    #[test]
    fn play_length_doubles() {
        let off = Window { n: 4000, start: 2000, ratio: 2, fsm: 3, la: false };
        let on = Window { la: true, ..off };
        let w = |win: &Window| readout(win, &mut Shadows::HANDOVER.clone());
        let at = |v: &[Write], r: u32| v.iter().find(|x| x.0 == r).unwrap().1;
        let (a, b) = (w(&off), w(&on));
        assert_eq!(at(&a, 0x1100), 2000 + 0x448);
        assert_eq!(at(&b, 0x1100), 4000 + 0x448);
        assert_eq!((at(&a, 0x1108), at(&a, 0x11f0)), (at(&b, 0x1108), at(&b, 0x11f0)));
    }
}
