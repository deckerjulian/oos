// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:WAVeform`: what `SOURce`, `MODE`, `FORMat`, `STARt`, `STOP` and `POINts` select, and the answers built from it,
//! `PREamble?` and `DATA?` (the settings themselves are parsed in `exec_cmd`). A new piece of a waveform answer goes
//! here, with its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{block, e, Scpi, RESP_MAX};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn wav_source_text(&self) -> String {
        match self.wav_src {
            WaveId::Ch(c) => format!("CHAN{}", c + 1),
            WaveId::Ref(i) => format!("R{}", i + 1),
        }
    }

    /// RAW, or MAXimum while stopped: the deep record. NORMal, and MAXimum while running: the screen.
    fn wave_memory(&self, ui: &Ui) -> bool {
        self.wav_mode == 2 || (self.wav_mode == 1 && ui.m.run == RunState::Stop)
    }

    pub(super) fn wave_len(&self, ui: &Ui) -> u64 {
        match self.wav_src {
            WaveId::Ch(c) => ui.channel_points(c, self.wave_memory(ui)) as u64,
            WaveId::Ref(i) => ui.ref_info(i).map(|(n, _, _, _, _)| n as u64).unwrap_or(0),
        }
    }

    /// How many points the current STARt/STOP page holds, without copying them.
    pub(super) fn page_count(&self, ui: &Ui) -> u64 {
        let n = self.wave_len(ui);
        let a = self.wav_start.max(1);
        if n == 0 || a > n {
            return 0;
        }
        let b = if self.wav_stop == 0 { n } else { self.wav_stop.clamp(a, n) };
        b - a + 1
    }

    fn wave_scale(&self, ui: &Ui) -> (f64, f64) {
        match self.wav_src {
            WaveId::Ch(c) => {
                let ch = &ui.m.ch[c];
                (ch.volts_per_div() as f64, ch.offset as f64)
            }
            WaveId::Ref(i) => ui.ref_info(i).map(|(_, _, _, vpd, off)| (vpd, off)).unwrap_or((1.0, 0.0)),
        }
    }

    fn y_codes(&self, vpd: f64, offset: f64) -> (f64, f64, f64) {
        let (yinc, yref) = match self.wav_fmt {
            1 => (vpd * VDIV as f64 / 60000.0, 32768.0),
            _ => (vpd * VDIV as f64 / 250.0, 128.0),
        };
        let yorg = if yinc == 0.0 { 0.0 } else { (offset / yinc).round() };
        (yinc, yorg, yref)
    }

    pub(super) fn wave_preamble(&self, ui: &Ui) -> String {
        let (vpd, offset) = self.wave_scale(ui);
        let (yinc, yorg, yref) = self.y_codes(vpd, offset);
        let (dt, t0) = self.wave_origin(ui);
        format!(
            "{},{},{},1,{},{},0,{},{},{}",
            self.wav_fmt,
            self.wav_mode,
            self.page_count(ui),
            e(dt),
            e(t0),
            e(yinc),
            yorg,
            yref
        )
    }

    /// Sample interval and the time of the first point of the current page. Does not copy the samples.
    fn wave_origin(&self, ui: &Ui) -> (f64, f64) {
        let (dt, t0) = match self.wav_src {
            WaveId::Ch(c) => ui.channel_origin(c, self.wave_memory(ui)).unwrap_or((0.0, 0.0)),
            WaveId::Ref(i) => ui.ref_info(i).map(|(_, dt, t0, _, _)| (dt, t0)).unwrap_or((0.0, 0.0)),
        };
        let n = self.wave_len(ui);
        let a = self.wav_start.max(1);
        if n == 0 || a > n {
            (dt, t0)
        } else {
            (dt, t0 + (a - 1) as f64 * dt)
        }
    }

    /// The page `:WAVeform:DATA?` returns, and the scale of the source.
    fn wave_page(&self, ui: &Ui) -> (Vec<f32>, f64, f64, f64, f64) {
        let (start, stop) = (self.wav_start.max(1), self.wav_stop);
        match self.wav_src {
            WaveId::Ch(c) => {
                let ch = &ui.m.ch[c];
                let (v, dt, t0) =
                    ui.channel_page(c, self.wave_memory(ui), start, stop).unwrap_or((Vec::new(), 0.0, 0.0));
                (v, dt, t0, ch.volts_per_div() as f64, ch.offset as f64)
            }
            WaveId::Ref(i) => ui.ref_page(i, start, stop).unwrap_or((Vec::new(), 0.0, 0.0, 1.0, 0.0)),
        }
    }

    pub(super) fn wave_data(&mut self, ui: &Ui) -> Option<Vec<u8>> {
        // Bound output before taking a copy of the requested page. Metadata never copies samples.
        let width = match self.wav_fmt {
            0 => 1,
            1 => 2,
            _ => 14,
        };
        if self.page_count(ui).saturating_mul(width).saturating_add(11) > RESP_MAX as u64 {
            self.push_err("-223,\"Too much data; select a shorter waveform page\"".into());
            return None;
        }
        let (vpd, offset) = self.wave_scale(ui);
        let (yinc, yorg, yref) = self.y_codes(vpd, offset);
        let (v, _, _, _, _) = self.wave_page(ui);
        Some(match self.wav_fmt {
            2 => {
                use std::fmt::Write as _;
                let mut text = String::with_capacity(v.len().saturating_mul(14));
                for (index, value) in v.iter().enumerate() {
                    if index != 0 {
                        text.push(',');
                    }
                    let _ = write!(text, "{}", e(*value as f64));
                }
                block(text.as_bytes())
            }
            1 => {
                // OOS wire policy: unsigned little-endian WORD, retained for existing clients.
                // The Rigol guide specifies two bytes per point but does not specify their order.
                let mut b = Vec::with_capacity(v.len() * 2);
                for x in v {
                    let code = ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 65535.0) as u16;
                    b.extend_from_slice(&code.to_le_bytes());
                }
                block(&b)
            }
            _ => {
                let b: Vec<u8> =
                    v.iter().map(|&x| ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 255.0) as u8).collect();
                block(&b)
            }
        })
    }
}
