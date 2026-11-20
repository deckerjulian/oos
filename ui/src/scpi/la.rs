// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:LA`: `STATe` (a row of settings/la.rs), `DIGital<n>:DISPlay`, `POD<n>:THReshold` and the record of the last
//! acquisition (`POINts?`, `PREamble?`, `DATA?`). A new `:LA` setting of a setup file is a row of that table, another
//! command goes into `la_cmd` here, with its line in docs/scpi.md; either with its query in ui/tests/scpi.txt
//! (AGENTS.md).

use super::{block, kw, parse_bool, suffix, Scpi};
use crate::ui::Ui;

impl Scpi {
    /// `:LA:...` (Rigol's names where they exist): state, lines shown, pod thresholds, and the record of the last
    /// acquisition: `:LA:DATA?` is an IEEE block of u16 words, little endian, bit n = Dn, sample k at
    /// t0 + k dt from `:LA:PREamble?` (points,dt,t0).
    pub(super) fn la_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        use crate::ui::la;
        let t = |s: String| Some(s.into_bytes());
        if !la::available() {
            self.push_err(format!(
                "-241,\"Hardware missing\" no logic analyser port on the {}",
                oos_hal::model::platform().caps.name
            ));
            return None;
        }
        let bad = |s: &mut Self| {
            s.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            None
        };
        // the switch of the settings table (settings/la.rs); ENABle is another name of STATe
        let row_nodes: &[&str] = if n.len() == 1 && kw(n[0], "ENABle") { &["STATe"] } else { n };
        if let Some(row) = crate::settings::command(&crate::settings::la::SLICES, row_nodes) {
            return self.setting_cmd(ui, row, query, arg);
        }
        let (k, idx) = suffix(n[0]);
        if n.len() == 2 && kw(k, "DIGital") && kw(n[1], "DISPlay") {
            let Some(line) = idx.filter(|&i| i < 16) else { return bad(self) };
            if query {
                return t((ui.m.la.show >> line & 1).to_string());
            }
            match parse_bool(arg) {
                Some(on) => {
                    ui.m.la.show = ui.m.la.show & !(1 << line) | (on as u16) << line;
                    return None;
                }
                None => return bad(self),
            }
        }
        if n.len() == 2 && kw(k, "POD") && kw(n[1], "THReshold") {
            let Some(pod) = idx.filter(|&i| i == 1 || i == 2).map(|i| i - 1) else { return bad(self) };
            if query {
                return t(format!("{:e}", ui.m.la.thr(pod)));
            }
            match arg.trim().parse::<f32>() {
                Ok(v) if v.abs() <= la::THR_MAX => {
                    (ui.m.la.preset[pod], ui.m.la.user[pod]) = (la::USER, v);
                    ui.mark_changed();
                    return None;
                }
                _ => return bad(self),
            }
        }
        if n.len() == 1 && query && (kw(n[0], "POINts") || kw(n[0], "PREamble") || kw(n[0], "DATA")) {
            let Some(r) = ui.la_record() else {
                self.push_err("-230,\"Data corrupt or stale\" no LA record".into());
                return if kw(n[0], "DATA") { Some(block(&[])) } else { t("0".into()) };
            };
            if kw(n[0], "POINts") {
                return t(r.words.len().to_string());
            }
            if kw(n[0], "PREamble") {
                return t(format!("{},{:e},{:e}", r.words.len(), r.dt, r.t0));
            }
            return Some(block(&r.words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()));
        }
        self.push_err(format!("-113,\"Undefined header\" :LA:{}", n.join(":")));
        None
    }
}
