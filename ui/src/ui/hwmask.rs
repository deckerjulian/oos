// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! The mask test in the FPGA (`mask.hw = on` in system.conf, docs/fork/hwmask.md): the software mask in the table of
//! the mask unit, its counters in the mask statistics. The software test stays the fallback and the reference: it runs
//! on every full record that still reaches the UI, and alone where the hardware cannot test.

use super::*;
use crate::board::{MaskRead, MaskWant};


impl Ui {
    /// The mask test runs in the hardware: enabled on this board, live hardware frames, the mask on and built, its
    /// channel acquired. The fast display path then stays on.
    pub(super) fn mask_hw_active(&self) -> bool {
        board().mask_hw() && self.src.is_hw() && self.m.mask_on && self.mask.is_some() && self.mask_cols.is_some() && self.m.ch[self.m.mask_src].on
    }

    /// The table columns of the mask (`Mask::columns`), after every new mask.
    pub(super) fn mask_cols_update(&mut self) {
        self.mask_cols = self.mask.as_ref().map(|m| m.columns(crate::board::MASK_COLUMNS));
    }

    /// Hands the mask test to the acquisition thread and takes its latest counters; a new failure stops the
    /// acquisition when "stop on fail" is set.
    pub(super) fn mask_hw_poll(&mut self) {
        let want = if self.mask_hw_active() {
            self.mask_cols.as_ref().map(|(lo, hi)| MaskWant { ch: self.m.mask_src, lo: lo.clone(), hi: hi.clone(), stop: self.m.mask_stop, clear: self.mask_hw_clear })
        } else {
            None
        };
        let Some(r) = self.src.hw_mask(want) else { return };
        if mask_stops(self.mask_hw, r, self.m.mask_src) && self.m.mask_stop && self.m.run == RunState::Run {
            self.m.run = RunState::Stop;
            self.toast("Mask failed: stopped");
        }
        self.mask_hw = r;
    }

    /// (passed, failed) of the mask statistics: the hardware counters while the hardware tests, else the software
    /// test's.
    pub(super) fn mask_counts(&self) -> (u64, u64) {
        match self.mask_hw.filter(|r| r.ch == self.m.mask_src && self.mask_hw_active()) {
            Some(r) => (r.frames.saturating_sub(r.fails), r.fails),
            None => (self.mask_pass, self.mask_fail),
        }
    }

    /// Clears the mask statistics, the hardware counters with them.
    pub(super) fn mask_counts_reset(&mut self) {
        self.mask_pass = 0;
        self.mask_fail = 0;
        self.mask_hw_clear += 1;
    }

    /// Fork, `:SYSTem:DEBug:MASK?`: the mask unit's raw counters and slot next to the software test and the frame
    /// counters of the pipeline (docs/fork/hwmask.md).
    pub fn mask_debug_text(&self) -> String {
        let (acq, used, _) = crate::drawpipe::counters().read();
        let sw = format!("sw_pass={},sw_fail={},acquired={},used={},records={}", self.mask_pass, self.mask_fail, acq, used, self.records);
        let Some(r) = self.mask_hw else {
            return format!("off,enabled={},active={},{}", board().mask_hw() as u8, self.mask_hw_active() as u8, sw);
        };
        format!(
            "ch={},slot={},ctrl={:#010x},frames_lo={:#010x},frames_hi={:#010x},fails_lo={:#010x},fails_hi={:#010x},frames={},fails={},n={},{}",
            r.ch + 1,
            r.slot,
            r.ctrl,
            r.raw[0],
            r.raw[1],
            r.raw[2],
            r.raw[3],
            r.frames,
            r.fails,
            r.n,
            sw
        )
    }
}

/// A new reading `now` after `before` shows a new failure on channel `src` (a cleared counter is none).
fn mask_stops(before: Option<MaskRead>, now: Option<MaskRead>, src: usize) -> bool {
    let Some(n) = now.filter(|r| r.ch == src) else { return false };
    match before.filter(|b| b.ch == src) {
        Some(b) if b.n == n.n => false,
        Some(b) => n.fails > b.fails,
        None => n.fails > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rd(ch: usize, fails: u64, n: u64) -> Option<MaskRead> {
        Some(MaskRead { ch, fails, frames: fails + 10, n, ..Default::default() })
    }

    #[test]
    fn a_new_failure_stops() {
        // (before, now, source) -> stop
        let cases = [
            (None, rd(0, 0, 1), 0, false),
            (None, rd(0, 2, 1), 0, true),
            (rd(0, 2, 1), rd(0, 2, 2), 0, false),
            (rd(0, 2, 1), rd(0, 3, 2), 0, true),
            // the same reading again
            (rd(0, 2, 1), rd(0, 2, 1), 0, false),
            // a cleared counter
            (rd(0, 5, 1), rd(0, 0, 2), 0, false),
            // another channel's reading
            (rd(1, 0, 1), rd(1, 3, 2), 0, false),
            (rd(1, 9, 1), rd(0, 1, 2), 0, true),
            (rd(0, 1, 1), None, 0, false),
        ];
        for (b, n, src, want) in cases {
            assert_eq!(mask_stops(b, n, src), want, "{:?} {:?}", b, n);
        }
    }

    #[test]
    fn hardware_counts_while_the_hardware_tests() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        (ui.mask_pass, ui.mask_fail) = (7, 2);
        ui.mask_hw = rd(0, 4, 3);
        // the demo source is no hardware: the software counts
        assert!(!ui.mask_hw_active());
        assert_eq!(ui.mask_counts(), (7, 2));
        ui.mask_counts_reset();
        assert_eq!((ui.mask_counts(), ui.mask_hw_clear), ((0, 0), 1));
        assert!(ui.mask_debug_text().starts_with("ch=1,slot=0,ctrl=0x00000000,"), "{}", ui.mask_debug_text());
        ui.mask_hw = None;
        assert!(ui.mask_debug_text().starts_with("off,enabled=0,active=0,sw_pass=0"));
    }
}
