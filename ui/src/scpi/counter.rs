// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:COUNter`: `ENABle`, `MODE` and `NDIGits` of the frequency counter, declared in settings/meters.rs, and by hand
//! `SOURce`, `CURRent?` and `TOTalize:CLEar`. A new `:COUNter` setting is a row of that table, another command goes
//! into `counter_cmd` here; either with its query in ui/tests/scpi.txt, docs/scpi.md gets the row's line from the
//! table, the other's by hand (AGENTS.md).

use super::{e, kw, parse_ch, t, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn counter_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        if let Some(row) = crate::settings::command(&crate::settings::meters::COUNTER_SLICES, n) {
            return self.setting_cmd(ui, row, query, arg);
        }
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.cnt_src.min(n_ch() - 1) + 1));
            }
            return self.set_arg(ui, arg, parse_ch(arg), |ui, c| ui.m.cnt_src = c);
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
