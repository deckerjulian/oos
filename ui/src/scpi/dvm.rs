// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:DVM`: `ENABle`, `SOURce`, `MODE` and `CURRent?` of the digital voltmeter. A new `:DVM` command goes into
//! `dvm_cmd` here, with its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, parse_bool, parse_ch, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn dvm_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.dvm_on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.m.dvm_on = on;
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
                return t(&format!("CHAN{}", ui.m.dvm_src.min(n_ch() - 1) + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.dvm_src = c;
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
            const NAMES: [&str; 3] = ["DC", "ACRMS", "ACDC"];
            if query {
                return t(NAMES[ui.m.dvm_mode.min(2)]);
            }
            let a = arg.trim();
            let mode = if kw(a, "DC") {
                Some(0)
            } else if kw(a, "ACRMs") || a.eq_ignore_ascii_case("ACRMS") {
                Some(1)
            } else if kw(a, "ACDC") || kw(a, "ACDCrms") {
                Some(2)
            } else {
                None
            };
            return match mode {
                Some(m) => {
                    ui.m.dvm_mode = m;
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
            return t(&e(ui.dvm_value()));
        }
        self.push_err(format!("-113,\"Undefined header\" :DVM:{}", n.join(":")));
        None
    }
}
