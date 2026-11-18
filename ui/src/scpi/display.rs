// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:DISPlay`: `TYPE`, `GRADing:TIME`, `WBRightness`, `GBRightness`, `GRID`, `RULers` and `COLor`, and the screen
//! picture `:DISPlay:DATA?` returns (`bmp`, also the web page's `/screen.bmp`). A new `:DISPlay` setting goes into
//! `display_cmd` here, with its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{kw, nearest_index, parse_bool, Scpi};
use crate::gfx::Canvas;
use crate::model::*;
use crate::ui::Ui;

fn persist_index(arg: &str) -> Option<usize> {
    let a = arg.trim();
    if kw(a, "MIN") {
        return Some(0);
    }
    if kw(a, "INFinite") {
        return Some(PERSIST.len() - 1);
    }
    let v = a.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)?;
    // the f32 seconds of PERSIST in f64 as written (whole milliseconds), so a time half-way picks as before
    let times: Vec<f64> = PERSIST.iter().filter(|p| p.1 >= 0.0).map(|p| (p.1 as f64 * 1e3).round() / 1e3).collect();
    Some(nearest_index(&times, v))
}

/// The answer to `:DISPlay:GRADing:TIME?` for entry `i` of `PERSIST`.
fn persist_token(i: usize) -> String {
    match PERSIST.get(i).map(|p| p.1) {
        Some(0.0) => "MIN".into(),
        Some(s) if s > 0.0 => s.to_string(),
        _ => "INF".into(),
    }
}

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
    /// Waveform drawing, persistence, brightness, and the graticule. Persistence snaps to the times the screen actually keeps.
    pub(super) fn display_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "TYPE") {
            if query {
                return t(if ui.m.dots { "DOTS" } else { "VECT" });
            }
            ui.m.dots = if kw(arg, "VECTors") {
                false
            } else if kw(arg, "DOTS") {
                true
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && kw(n0, "GRADing") && kw(n[1], "TIME") {
            if query {
                return t(&persist_token(ui.m.persist));
            }
            let Some(i) = persist_index(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.persist = i;
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "WBRightness") {
            if query {
                return t(&ui.m.intensity.to_string());
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (1..=100).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.intensity = v.clamp(10, 100);
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "GRID") {
            if query {
                return t(match ui.m.grid {
                    Grid::Full => "FULL",
                    Grid::Light => "HALF",
                    Grid::None => "NONE",
                });
            }
            ui.m.grid = if kw(arg, "FULL") {
                Grid::Full
            } else if kw(arg, "HALF") {
                Grid::Light
            } else if kw(arg, "NONE") {
                Grid::None
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "GBRightness") {
            if query {
                return t(&ui.m.grid_bright.to_string());
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (0..=100).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            // The screen's grid brightness starts at 10, the same floor as the waveform brightness.
            ui.m.grid_bright = v.clamp(10, 100);
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "RULers") {
            if query {
                return t(if ui.m.grid_frame { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.grid_frame = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "COLor") {
            if query {
                return t(if ui.m.color_grade { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.color_grade = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        self.push_err(format!("-113,\"Undefined header\" :DISPlay:{}", n.join(":")));
        None
    }
}
