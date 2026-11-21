// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:MASK`: `ENABle` (a row of settings/mask.rs), `SOURce`, `X`, `Y`, `CREate`, `RESet`, `OPERate` and the counts
//! `FAILed?`, `PASSed?`, `TOTal?` of the pass/fail test. A new `:MASK` setting of a setup file is a row of that table,
//! another command goes into `mask_cmd` here, with its line in docs/scpi.md; either with its query in
//! ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, num, parse_ch, t, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn mask_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        if let Some(row) = crate::settings::command(&crate::settings::mask::SLICES, n) {
            return self.setting_cmd(ui, row, query, arg);
        }
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                return t(&format!("CHAN{}", ui.m.mask_src.min(n_ch() - 1) + 1));
            }
            return self.set_arg(ui, arg, parse_ch(arg), |ui, c| ui.m.mask_src = c);
        }
        if n.len() == 1 && (kw(n0, "X") || kw(n0, "Y")) {
            let horizontal = kw(n0, "X");
            if query {
                return t(&e(if horizontal { ui.m.mask_x } else { ui.m.mask_y } as f64));
            }
            let Some(v) = num(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let ok = if horizontal { ui.mask_tolerance(Some(v), None) } else { ui.mask_tolerance(None, Some(v)) };
            if ok {
                ui.mark_changed();
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            }
            return None;
        }
        if n.len() == 1 && kw(n0, "CREate") && !query {
            if ui.mask_create() {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" source has no waveform".into());
            }
            return None;
        }
        if n.len() == 1 && kw(n0, "RESet") && !query {
            ui.mask_counts_reset();
            ui.mark_changed();
            return None;
        }
        if n.len() == 1 && kw(n0, "OPERate") {
            if query {
                return t(if ui.m.mask_on && ui.m.run == RunState::Run { "RUN" } else { "STOP" });
            }
            if !ui.m.mask_on {
                self.push_err("-224,\"Illegal parameter value\" mask is off".into());
                return None;
            }
            let a = arg.trim();
            if kw(a, "RUN") {
                ui.m.run = RunState::Run;
                if ui.m.trig_mode == TrigMode::Single {
                    ui.m.trig_mode = TrigMode::Auto;
                }
                ui.mark_changed();
                None
            } else if kw(a, "STOP") {
                ui.m.run = RunState::Stop;
                ui.mark_changed();
                None
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            }
        } else if n.len() == 1 && query && (kw(n0, "FAILed") || kw(n0, "PASSed") || kw(n0, "TOTal")) {
            let (pass, fail) = ui.mask_counts();
            let n = if kw(n0, "FAILed") {
                fail
            } else if kw(n0, "PASSed") {
                pass
            } else {
                pass + fail
            };
            t(&n.to_string())
        } else {
            self.push_err(format!("-113,\"Undefined header\" :MASK:{}", n.join(":")));
            None
        }
    }
}
