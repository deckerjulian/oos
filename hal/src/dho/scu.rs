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
    /// timestamp control 0x40b8 (bit0 latch, bit2 clear the first-trigger stamp)
    pub s40b8: u32,
}

impl Shadows {
    /// The stock register values at STOP after the hand-over of 2026-11-02 21:12 (CH1, 2.5 Mpts).
    pub const HANDOVER: Shadows =
        Shadows { s4004: 0x1, s4008: 0x80_0001, s400c: 0x6000_0000, s1000: 0, s1010: 0x2000_0000, s4080: 0x4000_0010, s1004: 0x0400_0000, s4020: 0x2001, s1080: 0x01e0_0000, s1084: 0, s40b8: 0 };
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
    /// average mode: the window also gets the average offset word 0x1110 (`avg_offset`)
    pub avg: bool,
}

/// Main window average offset (docs/fork/specs/01-hw-averaging.md 1.2); 0x1114 is the zoom window's, which OOS does
/// not use.
pub const AVG_OFFSET: u32 = 0x1110;

/// The average offset word for a window from memory point `s` (per channel) at interleave factor `m`: the average
/// buffer is addressed in half bursts of 5 words. Bits 31:8 = the half-burst address of the start, rounded down to a
/// multiple of 4; bits 7:0 = what is left of the start, in words (points x m).
pub fn avg_offset(s: u32, m: u32) -> u32 {
    let m = m.max(1) as u64;
    let s = s as u64;
    let qa = (m * s / 5) & !3;
    let rest = (s - qa * 5 / m) * m;
    (((qa & 0xff_fffc) << 8) | (rest & 0xff)) as u32
}

/// Status bits of 0x4034 (read only).
pub mod status {
    /// FSM stopped.
    pub const STOPPED: u32 = 1 << 13;
    /// waiting for a trigger
    pub const WAIT_TRIG: u32 = 1 << 4;
    /// sticky: the TPU's trigger fired (rising edge of SLICE_X62Y75_BFF, the trigger output into the SCU FSM), until
    /// the status clear (`clear_status`). Netlist, docs/fork/trigger-lockin.md "Trigger status from the hardware"
    pub const TPU_TRIGGER: u32 = 1 << 6;
    /// sticky: the FSM accepted a trigger while waiting (SLICE_X53Y73_AFF rising): the TPU's, the auto-trigger timeout
    /// (0x4020 bit0, 0x4060/0x4064) or a forced one, until the status clear
    pub const ACCEPTED: u32 = 1 << 7;
}

/// What triggered the acquisitions since the last status clear, from a read of 0x4034.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fired {
    /// the TPU fired at least once (the stock status priority puts bit 6 first)
    Trigger,
    /// acquisitions were accepted, none on a TPU trigger: the auto timeout (or force) made them
    Forced,
    /// nothing accepted
    None,
}

pub fn fired(v: u32) -> Fired {
    if v & status::TPU_TRIGGER != 0 {
        Fired::Trigger
    } else if v & status::ACCEPTED != 0 {
        Fired::Forced
    } else {
        Fired::None
    }
}

/// The stock status clear (libscope `DevSystemSCU_clrStatus(1, 1)`): the last read value with bits 31:30 set, then
/// with them clear. Bit 31 clears the sticky bits 6 and 7 (the netlist: SLICE_X34Y76_AFF in their next-state LUTs).
pub fn clear_status(last: u32) -> Vec<Write> {
    let v = last & 0x3fff_ffff;
    vec![(0x4034, v | 0xc000_0000), (0x4034, v)]
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
    if win.avg {
        w.push((AVG_OFFSET, avg_offset(win.start / win.ratio, win.ratio)));
    }
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
    record_run(fsm, sh, first, n, 0)
}

/// Hardware segmented recording (stock record state 0xb, docs/fork/specs/03-segmented-timestamps.md 1.2 steps 2..7):
/// `n` frames from frame 0, the FPGA re-arms itself between them and waits `interval_ticks` x 6.4 ns (0x4068) after
/// each. The same 29 writes as `arm`; with interval 0 they are identical.
pub fn record_arm(fsm: u32, sh: &mut Shadows, n: u32, interval_ticks: u32) -> Vec<Write> {
    record_run(fsm, sh, 0, n, interval_ticks)
}

fn record_run(fsm: u32, sh: &mut Shadows, first: u32, n: u32, interval_ticks: u32) -> Vec<Write> {
    let mut w = Vec::new();
    stop_sequence(&mut w, sh);
    set_run_prologue(&mut w, sh);
    w.push((0x406c, 0));
    w.push((0x4068, interval_ticks));
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

/// One tick of the FSM delay 0x4068 and the acquisition timeout 0x406c: 6.4 ns.
pub const FSM_TICK_FS: i128 = 6_400_000;

/// 0x4068 value for a record interval of `fs` femtoseconds (truncated like the stock value_fs / 6,400,000), saturated
/// at the register width.
pub fn interval_ticks(fs: i128) -> u32 {
    (fs.max(0) / FSM_TICK_FS).min(u32::MAX as i128) as u32
}

/// Timestamp registers (spec 03, 1.1): latched stamp, control, first-trigger stamp. Low word first, high word at +4.
pub mod stamp {
    pub const LATCHED: u32 = 0x40b0;
    pub const CTRL: u32 = 0x40b8;
    pub const FIRST: u32 = 0x40c0;
    /// 0x40b8 bit0 (write): copy the current stamp into 0x40b0/0x40b4
    pub const LATCH: u32 = 1 << 0;
    /// 0x40b8 bit1 (read): the first-trigger stamp 0x40c0/0x40c4 is valid
    pub const FIRST_VALID: u32 = 1 << 1;
    /// 0x40b8 bit2 (write): clear the first-trigger stamp
    pub const FIRST_CLEAR: u32 = 1 << 2;
}

/// Clears the first-trigger stamp: 0x40b8 bit2 set, then cleared (stock `setTimeStampFirstClr`).
pub fn ts_first_clear(sh: &mut Shadows) -> Vec<Write> {
    sh.s40b8 |= stamp::FIRST_CLEAR;
    let a = (stamp::CTRL, sh.s40b8);
    sh.s40b8 &= !stamp::FIRST_CLEAR;
    vec![a, (stamp::CTRL, sh.s40b8)]
}

/// Latches the stamp into 0x40b0/0x40b4: 0x40b8 bit0 cleared, set, cleared (stock `getTimeStamp`). Read the pair after.
pub fn ts_latch(sh: &mut Shadows) -> Vec<Write> {
    sh.s40b8 &= !stamp::LATCH;
    let a = (stamp::CTRL, sh.s40b8);
    sh.s40b8 |= stamp::LATCH;
    let b = (stamp::CTRL, sh.s40b8);
    sh.s40b8 &= !stamp::LATCH;
    vec![a, b, (stamp::CTRL, sh.s40b8)]
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

    // Break it catches: an auto-forced batch shown as triggered (or the reverse), or a clear that leaves bits 31:30 set
    #[test]
    fn status_tells_tpu_trigger_from_forced() {
        // read on the unit with both sticky bits set (never cleared), and the cases after a clear
        assert_eq!(fired(0x11d2), Fired::Trigger);
        assert_eq!(fired(0x1192), Fired::Forced);
        assert_eq!(fired(0x1012), Fired::None);
        assert_eq!(clear_status(0x11d2), vec![(0x4034, 0xc000_11d2), (0x4034, 0x11d2)]);
        assert_eq!(clear_status(0xc000_0001)[1], (0x4034, 1));
    }

    #[test]
    fn auto_trigger_200us() {
        // acquisition-start.md: 200 us/div -> 0x4060 = 0x2625a0, 0x4064 = 0x17d7840; shadow 0x2001
        let mut sh = Shadows::HANDOVER;
        assert_eq!(auto_trigger(&mut sh, true, 200_000_000_000, 8_000_000), [(0x4020, 0x2001), (0x4060, 0x2625a0), (0x4064, 0x17d7840)]);
        assert_eq!(auto_trigger(&mut sh, false, 1_000_000_000, 0)[0], (0x4020, 0x2000));
    }

    const WIN: Window = Window { n: 4000, start: 1_248_000, ratio: 1, fsm: 1, la: false, avg: false };

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

    // Break it catches: a record arm that differs from the verified batch arm in anything but the 0x4068 interval, an
    // interval in 4 ns instead of 6.4 ns ticks, a stamp pulse that leaves bit 0 or bit 2 set.
    #[test]
    fn record_arm_is_the_batch_arm_with_an_interval() {
        let (mut a, mut b) = (Shadows::HANDOVER, Shadows::HANDOVER);
        assert_eq!(record_arm(1, &mut a, 100, 0), arm(1, &mut b, 0, 100));
        assert_eq!(a, b);
        // 1 ms = 156,250 ticks of 6.4 ns (spec 03, 4)
        assert_eq!(interval_ticks(1_000_000_000_000), 156_250);
        assert_eq!((interval_ticks(-5), interval_ticks(6_399_999), interval_ticks(i128::MAX)), (0, 0, u32::MAX));
        let mut c = Shadows::HANDOVER;
        let w = record_arm(1, &mut c, 100, 156_250);
        let d = arm(1, &mut Shadows::HANDOVER.clone(), 0, 100);
        let diff: Vec<usize> = (0..w.len()).filter(|&i| w[i] != d[i]).collect();
        assert_eq!(diff.len(), 1);
        assert_eq!(w[diff[0]], (0x4068, 156_250));
        // the full sequence of `seq::State::record_steps` (stamp clear, record arm) against the fixture from spec 03 1.2
        let mut sh = Shadows::HANDOVER;
        let mut v = ts_first_clear(&mut sh);
        v.extend(record_arm(1, &mut sh, 100, 156_250));
        assert_eq!(text(&v), include_str!("../../testdata/record-arm-writes.txt"));
    }

    #[test]
    fn stamp_pulses_leave_the_control_clear() {
        let mut sh = Shadows { s40b8: 0x10, ..Shadows::HANDOVER };
        assert_eq!(ts_first_clear(&mut sh), [(0x40b8, 0x14), (0x40b8, 0x10)]);
        assert_eq!(ts_latch(&mut sh), [(0x40b8, 0x10), (0x40b8, 0x11), (0x40b8, 0x10)]);
        assert_eq!(sh.s40b8, 0x10);
        let mut sh = Shadows { s40b8: 0x5, ..Shadows::HANDOVER };
        ts_latch(&mut sh);
        ts_first_clear(&mut sh);
        assert_eq!(sh.s40b8 & 0x5, 0);
    }

    // Break it catches: the start in words instead of points per channel, no rounding to 4 half bursts, the remainder
    // in points instead of words. Expected values by hand from spec 01, 1.2: q = m s / 5, qa = q & !3,
    // word = qa << 8 | (s - qa 5 / m) m.
    #[test]
    fn average_offset_word() {
        // s = 0: 0 for every m
        assert_eq!([1, 2, 4].map(|m| avg_offset(0, m)), [0, 0, 0]);
        // s = 3: q = 0, 1, 2 -> qa = 0; rest 3, 6, 12
        assert_eq!([1, 2, 4].map(|m| avg_offset(3, m)), [3, 6, 12]);
        // s = 7: q = 1, 2, 5 -> qa = 0, 0, 4; rest 7, 14, (7 - 5) x 4 = 8 -> 4 << 8 | 8
        assert_eq!([1, 2, 4].map(|m| avg_offset(7, m)), [7, 14, 0x408]);
        // s = 1,248,000: q = 249,600 / 499,200 / 998,400, all multiples of 4, rest 0
        assert_eq!([1, 2, 4].map(|m| avg_offset(1_248_000, m)), [249_600 << 8, 499_200 << 8, 998_400 << 8]);
        // s = 1,248,013, m = 2: q = 499,205 -> qa = 499,204; 499,204 x 5 / 2 = 1,248,010; rest 3 x 2 = 6
        assert_eq!(avg_offset(1_248_013, 2), 499_204 << 8 | 6);
        // the window writes it right after the start, only in average mode
        let mut sh = Shadows::HANDOVER;
        let w = readout_play4(&Window { avg: true, ..WIN }, &mut sh, 0, 1);
        let i = w.iter().position(|x| x.0 == 0x1108).unwrap();
        assert_eq!(w[i + 1], (AVG_OFFSET, 249_600 << 8));
        let n = readout_play4(&WIN, &mut Shadows::HANDOVER.clone(), 0, 1);
        assert_eq!(w.len(), n.len() + 1);
        assert!(!n.iter().any(|x| x.0 == AVG_OFFSET));
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
        let off = Window { n: 4000, start: 2000, ratio: 2, fsm: 3, la: false, avg: false };
        let on = Window { la: true, ..off };
        let w = |win: &Window| readout(win, &mut Shadows::HANDOVER.clone());
        let at = |v: &[Write], r: u32| v.iter().find(|x| x.0 == r).unwrap().1;
        let (a, b) = (w(&off), w(&on));
        assert_eq!(at(&a, 0x1100), 2000 + 0x448);
        assert_eq!(at(&b, 0x1100), 4000 + 0x448);
        assert_eq!((at(&a, 0x1108), at(&a, 0x11f0)), (at(&b, 0x1108), at(&b, 0x11f0)));
    }
}
