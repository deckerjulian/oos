// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:HISTogram`: `ENABle`, `TYPE` (a row of settings/histogram.rs), `SOURce` and `STATistics:RESult?`. A new
//! `:HISTogram` setting of a setup file is a row of that table, another command goes into `hist_cmd` here, with its
//! line in docs/scpi.md; either with its query in ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, parse_bool, parse_ch, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    /// Vertical or horizontal histogram of one live channel. The source is the measurement source, which is what the screen uses.
    pub(super) fn hist_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "ENABle") {
            if query {
                return t(if ui.m.hist == Hist::Off { "0" } else { "1" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    if on && ui.m.hist == Hist::Off {
                        ui.m.hist = Hist::Vertical;
                    } else if !on {
                        ui.m.hist = Hist::Off;
                    }
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if let Some(row) = crate::settings::command(&crate::settings::histogram::SLICES, n) {
            return self.setting_cmd(ui, row, query, arg);
        }
        if n.len() == 1 && kw(n0, "SOURce") {
            if query {
                let c = ui.hist_source().unwrap_or(0);
                return t(&format!("CHAN{}", c + 1));
            }
            return match parse_ch(arg) {
                Some(c) => {
                    ui.m.ch[c].on = true;
                    ui.set_meas_source(WaveId::Ch(c));
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 2 && kw(n0, "STATistics") && kw(n[1], "RESult") && query {
            let blank = "Sum:0,Peaks:0,Max:9.9E37,Min:9.9E37,Pk_Pk:9.9E37,Mean:9.9E37,Median:9.9E37,Mode:9.9E37,Bin width:9.9E37,Sigma:9.9E37";
            let Some(s) = ui.hist_stat() else { return t(blank) };
            let num = |v: f64| if v.is_finite() { e(v) } else { "9.9E37".to_string() };
            let pk = if s.max.is_finite() && s.min.is_finite() { e(s.max - s.min) } else { "9.9E37".to_string() };
            return t(&format!(
                "Sum:{},Peaks:{},Max:{},Min:{},Pk_Pk:{},Mean:{},Median:{},Mode:{},Bin width:{},Sigma:{}",
                s.sum,
                s.peaks,
                num(s.max),
                num(s.min),
                pk,
                num(s.mean),
                num(s.median),
                num(s.mode),
                num(s.bin),
                num(s.sigma)
            ));
        }
        self.push_err(format!("-113,\"Undefined header\" :HISTogram:{}", n.join(":")));
        None
    }
}
