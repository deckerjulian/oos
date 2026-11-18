// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:COUNter`: `ENABle`, `SOURce`, `MODE`, `NDIGits`, `CURRent?` and `TOTalize:CLEar` of the frequency counter.
//! A new `:COUNter` command goes into `counter_cmd` here, with its line in docs/scpi.md and its query in
//! ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, parse_bool, parse_ch, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn counter_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.cnt_on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.cnt_on = on;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.cnt_src.min(n_ch() - 1) + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.cnt_src = c;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "MODE") {
            const NAMES: [&str; 3] = ["FREQ", "PER", "TOT"];
            if query {
                return t(NAMES[ui.m.cnt_mode.min(2)]);
            }
            let a = arg.trim();
            let mode = if kw(a, "FREQuency") {
                Some(0)
            } else if kw(a, "PERiod") {
                Some(1)
            } else if kw(a, "TOTalize") {
                Some(2)
            } else {
                None
            };
            return match mode {
                Some(m) => {
                    ui.m.cnt_mode = m;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "NDIGits") {
            if query {
                return t(&ui.m.cnt_digits.to_string());
            }
            return match arg.trim().parse::<u32>().ok().filter(|d| (3..=7).contains(d)) {
                Some(d) => {
                    ui.m.cnt_digits = d;
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "CURRent") && query {
            return t(&e(ui.counter_value()));
        }
        if n.len() == 2 && kw(n0, "TOTalize") && kw(n[1], "CLEar") && !query {
            ui.counter_clear();
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :COUNter:{}", n.join(":")));
        None
    }
}
