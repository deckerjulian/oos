// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Math channels: the FFT scale, and the methods that read and select the math channel the menu edits (their colours
//! are the screen's, `ui.rs`). A new math setting goes here.

use super::*;

/// FFT vertical scale in dB/div.
pub const FFT_DB: [f32; 5] = [1.0, 2.0, 5.0, 10.0, 20.0];

impl Model {
    /// Settings of math channel `k` (the selected one from the edit fields).
    pub fn math_ch(&self, k: usize) -> crate::math::MathCh {
        if k == self.math_sel {
            crate::math::MathCh {
                lin: self.math_lin,
                op: self.math,
                a: self.math_a,
                b: self.math_b,
                scale: self.math_scale,
                fc: self.math_fc,
                bw: self.math_bw,
                offset: self.math_offset,
                win: self.fft_win,
            }
        } else {
            self.maths[k]
        }
    }

    /// Makes math channel `k` the one the menu edits.
    pub fn math_select(&mut self, k: usize) {
        self.maths[self.math_sel] = self.math_ch(self.math_sel);
        self.math_sel = k.min(crate::math::NMATH - 1);
        let c = self.maths[self.math_sel];
        self.math_lin = c.lin;
        (
            self.math,
            self.math_a,
            self.math_b,
            self.math_scale,
            self.math_fc,
            self.math_bw,
            self.math_offset,
            self.fft_win,
        ) = (c.op, c.a, c.b, c.scale, c.fc, c.bw, c.offset, c.win);
    }
}
