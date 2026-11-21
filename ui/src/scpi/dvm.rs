// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:DVM`: `ENABle` and `MODE` of the digital voltmeter, declared in settings/meters.rs, and by hand `SOURce` and
//! `CURRent?`. A new `:DVM` setting is a row of that table, another command goes into `dvm_cmd` here; either with its
//! query in ui/tests/scpi.txt, docs/scpi.md gets the row's line from the table, the other's by hand (AGENTS.md).

use super::{e, kw, parse_ch, t, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn dvm_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        if let Some(row) = crate::settings::command(&crate::settings::meters::DVM_SLICES, n) {
            return self.setting_cmd(ui, row, query, arg);
        }
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.dvm_src.min(n_ch() - 1) + 1));
            }
            return self.set_arg(ui, arg, parse_ch(arg), |ui, c| ui.m.dvm_src = c);
        }
        if n.len() == 1 && kw(n0, "CURRent") && query {
            return t(&e(ui.dvm_value()));
        }
        self.push_err(format!("-113,\"Undefined header\" :DVM:{}", n.join(":")));
        None
    }
}
