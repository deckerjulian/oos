// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:DISPlay`: the settings of the Display group (`TYPE`, `GRADing:TIME`, `WBRightness`, `GBRightness`, `GRID`,
//! `RULers`, `COLor`), declared in settings/display.rs, and the screen picture `:DISPlay:DATA?` returns (`bmp`, also
//! the web page's `/screen.bmp`). A new `:DISPlay` setting is a row of that table, with its query in
//! ui/tests/scpi.txt (AGENTS.md); docs/scpi.md gets its line from the table.

use super::Scpi;
use crate::gfx::Canvas;
use crate::ui::Ui;

/// 24-bit BMP of the canvas (what :DISPlay:DATA? returns on the stock firmware).
pub fn bmp(cv: &Canvas) -> Vec<u8> {
    let (w, h) = (cv.w as usize, cv.h as usize);
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        for x in 0..w {
            let p = cv.px[y * w + x];
            b.extend_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
        b.resize(b.len() + row - w * 3, 0);
    }
    b
}

impl Scpi {
    /// The settings of the Display group (settings/display.rs): the row whose command the nodes name answers the
    /// query or takes the argument; a value it does not take is refused with -224, as before the table.
    pub(super) fn display_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let Some(row) = crate::settings::command(&crate::settings::display::SLICES, n) else {
            self.push_err(format!("-113,\"Undefined header\" :DISPlay:{}", n.join(":")));
            return None;
        };
        self.setting_cmd(ui, row, query, arg)
    }
}
