// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:SIM`: `PRESet`, `SLOT`, `PLAY`, `SHOW`, `BUSY?`, `SSC:MODE|DEViation|FREQuency`, `JITTer:DCD|CORRelated|CORNer`
//! and `PATTern` of the signal simulator. A new `:SIM` command goes into `sim_cmd` here, with its line in
//! docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, parse_bool, parse_slot, t, Scpi};
use crate::ui::Ui;

impl Scpi {
    pub(super) fn sim_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n0 = n.first().copied().unwrap_or("");
        if n.len() == 1 && kw(n0, "BUSY") && query {
            return t(if ui.sim_generating() { "1" } else { "0" });
        }
        if n.len() == 2 && kw(n0, "SSC") && kw(n[1], "MODE") {
            if query {
                return t(if !ui.m.sim_ssc {
                    "OFF"
                } else if ui.m.sim_ssc_center {
                    "CENTER"
                } else {
                    "DOWN"
                });
            }
            match arg.trim().to_ascii_uppercase().as_str() {
                "OFF" => ui.m.sim_ssc = false,
                "DOWN" => {
                    ui.m.sim_ssc = true;
                    ui.m.sim_ssc_center = false;
                }
                "CENTER" | "CENT" => {
                    ui.m.sim_ssc = true;
                    ui.m.sim_ssc_center = true;
                }
                _ => {
                    self.push_err("-224,\"Illegal parameter value\" SSC mode".into());
                    return None;
                }
            }
            ui.mark_changed();
            return None;
        }
        if n.len() == 2 && (kw(n0, "SSC") || kw(n0, "JITTer")) {
            let setting = if kw(n0, "SSC") {
                if kw(n[1], "DEViation") {
                    Some((&mut ui.m.sim_ssc_deviation, 0.0, 0.05))
                } else if kw(n[1], "FREQuency") {
                    Some((&mut ui.m.sim_ssc_hz, 1e3, 1e6))
                } else {
                    None
                }
            } else if kw(n[1], "DCD") {
                Some((&mut ui.m.sim_dcd_pp, 0.0, 100e-12))
            } else if kw(n[1], "CORRelated") {
                Some((&mut ui.m.sim_corr_rj, 0.0, 50e-12))
            } else if kw(n[1], "CORNer") {
                Some((&mut ui.m.sim_corr_hz, 1e3, 100e6))
            } else {
                None
            };
            if let Some((value, min, max)) = setting {
                if query {
                    return t(&e(*value));
                }
                match arg.trim().parse::<f64>() {
                    Ok(v) if v.is_finite() && v >= min && v <= max => *value = v,
                    _ => {
                        self.push_err("-224,\"Illegal parameter value\" SIM setting".into());
                        return None;
                    }
                }
                ui.mark_changed();
                return None;
            }
        }
        if n.len() == 1 && (kw(n0, "PATTern") || n0.eq_ignore_ascii_case("PAT")) {
            if query {
                return t(&format!("PRBS{}", ui.m.sim_prbs));
            }
            let a = arg.trim().to_ascii_uppercase();
            let order = a.strip_prefix("PRBS").unwrap_or(&a).trim_start_matches('-').parse::<u8>().ok();
            if let Some(p @ (7 | 9 | 13 | 15 | 23 | 31)) = order {
                ui.m.sim_prbs = p;
                ui.mark_changed();
            } else {
                self.push_err("-224,\"Illegal parameter value\" PRBS order".into());
            }
            return None;
        }
        if kw(n0, "PRESet") {
            if query {
                return t(ui.sim_preset_name());
            }
            return if ui.sim_preset_set(arg) {
                ui.mark_changed();
                None
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            };
        }
        if kw(n0, "SLOT") {
            if query {
                return t(&format!("R{}", ui.sim_slot() + 1));
            }
            return self.set_arg(ui, arg, parse_slot(arg), |ui, i| {
                ui.sim_slot_set(i);
            });
        }
        if kw(n0, "PLAY") {
            if query {
                return t(if ui.sim_playing() { "1" } else { "0" });
            }
            let on = if arg.trim().is_empty() { Some(true) } else { parse_bool(arg) };
            return match on {
                Some(on) => {
                    match ui.sim_play_set(on) {
                        Ok(()) => ui.mark_changed(),
                        Err(error) => self.push_err(format!("-224,\"Illegal parameter value\" {}", error)),
                    }
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SHOW") {
            if query {
                return t(if ui.sim_shown() == Some(true) { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) if ui.sim_show_set(on) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        self.push_err(format!("-113,\"Undefined header\" :SIM:{}", n.join(":")));
        None
    }
}
