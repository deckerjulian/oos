// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:TIMebase`: `[:MAIN]:SCALe|OFFSet|POSition`, `DELay` (zoom), `VERNier`, `MODE` and `XY` here; `HREFerence:MODE`
//! and `ROLL` are rows of settings/timebase.rs. A new `:TIMebase` setting of a setup file is a row of that table,
//! another command goes into `timebase_cmd` here, with its line in docs/scpi.md; either with its query in
//! ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, nearest, num, parse_bool, parse_ch, t, Scpi};
use crate::model::*;
use crate::ui::Ui;

fn chan_token(c: usize) -> String {
    format!("CHAN{}", c.min(NCH - 1) + 1)
}

impl Scpi {
    /// `:TIMebase` (`nodes` with the root).
    pub(super) fn timebase_cmd(&mut self, ui: &mut Ui, nodes: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n = &nodes[1..];
        let n0 = n.first().copied().unwrap_or("");
        // scale and position; delayed sweep, XY, the expand point and fine adjust below
        let last = nodes[nodes.len() - 1];
        if !(kw(n0, "DELay") || kw(n0, "XY") || kw(n0, "HREFerence") || kw(n0, "VERNier")) {
            if kw(last, "SCALe") {
                if query {
                    return t(&e(ui.m.time_per_div() as f64));
                }
                if let Some(v) = num(arg) {
                    ui.m.tscale = nearest(&tscales(), v);
                    ui.m.tfine = 1.0;
                    ui.mark_changed();
                    return None;
                }
            } else if kw(last, "OFFSet") || kw(last, "POSition") {
                if query {
                    return t(&e(ui.m.delay as f64));
                }
                if let Some(v) = num(arg) {
                    ui.m.delay = v;
                    ui.mark_changed();
                    return None;
                }
            }
        }
        if n.len() == 1 && kw(n0, "VERNier") {
            if query {
                return t(if ui.m.fine_t { "1" } else { "0" });
            }
            return self.set_arg(ui, arg, parse_bool(arg), |ui, on| {
                ui.m.fine_t = on;
                if !on {
                    ui.m.tfine = 1.0;
                }
            });
        }
        // the rows of the table (settings/timebase.rs): `HREFerence:MODE` and `ROLL`
        if let Some(row) = crate::settings::command(&crate::settings::timebase::SLICES, n) {
            return self.setting_cmd(ui, row, query, arg);
        }
        if n.len() == 2 && kw(n0, "DELay") {
            return self.timebase_delay(ui, n, query, arg);
        }
        if n.len() == 1 && kw(n0, "MODE") {
            return self.timebase_mode(ui, query, arg);
        }
        if n.len() == 2 && kw(n0, "XY") {
            return self.timebase_xy(ui, n, query, arg);
        }
        self.push_err(format!("-113,\"Undefined header\" :TIMebase:{}", n.join(":")));
        None
    }

    /// `:TIMebase:DELay:ENABle|SCALe|OFFSet|POSition`: the zoom window (`n` without the root).
    fn timebase_delay(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        if kw(n[1], "ENABle") {
            if query {
                return t(if ui.m.zoom { "1" } else { "0" });
            }
            return self.set_arg(ui, arg, parse_bool(arg), |ui, on| ui.zoom_set_remote(on));
        }
        if kw(n[1], "SCALe") {
            if query {
                return t(&e(tscales()[ui.m.zscale] as f64));
            }
            let Some(v) = num(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let scales = tscales();
            let mut i = nearest(&scales, v);
            if i > ui.m.tscale {
                i = ui.m.tscale;
            }
            ui.m.zscale = i;
            ui.zoom_clamp();
            ui.mark_changed();
            return None;
        }
        if kw(n[1], "OFFSet") || kw(n[1], "POSition") {
            if query {
                return t(&e(ui.m.zdelay as f64));
            }
            let Some(v) = num(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            ui.m.zdelay = v;
            ui.zoom_clamp();
            ui.mark_changed();
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :TIMebase:{}", n.join(":")));
        None
    }

    /// `:TIMebase:MODE`: MAIN or XY.
    fn timebase_mode(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        // MAIN is YT. XY is the screen's XY plot. This screen has no forced roll mode; auto roll is :TIMebase:ROLL.
        // The query names XY while that plot is on, so a controller can read the mode it just set.
        if query {
            return t(if ui.xy_on() { "XY" } else { "MAIN" });
        }
        if kw(arg, "MAIN") {
            ui.xy_set(false);
        } else if arg.trim().eq_ignore_ascii_case("XY") {
            ui.xy_set(true);
        } else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        }
        ui.mark_changed();
        None
    }

    /// `:TIMebase:XY:ENABle|X|Y|Z`: the XY plot (`n` without the root).
    fn timebase_xy(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        if kw(n[1], "ENABle") {
            if query {
                return t(if ui.xy_on() { "1" } else { "0" });
            }
            return self.set_arg(ui, arg, parse_bool(arg), |ui, on| ui.xy_set(on));
        }
        if kw(n[1], "X") || kw(n[1], "Y") {
            // The plot is CH1 against CH2. Those sources are not selectable.
            let x = kw(n[1], "X");
            if query {
                return t(if x { "CHAN1" } else { "CHAN2" });
            }
            let want = if x { 0 } else { 1 };
            if parse_ch(arg) == Some(want) {
                return None;
            }
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        }
        if kw(n[1], "Z") {
            if query {
                return t(&ui.m.z_src.map(chan_token).unwrap_or_else(|| "NONE".into()));
            }
            if kw(arg, "NONE") || kw(arg, "OFF") {
                ui.m.z_src = None;
            } else if let Some(c) = parse_ch(arg) {
                ui.m.z_src = Some(c);
            } else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            }
            ui.mark_changed();
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :TIMebase:{}", n.join(":")));
        None
    }
}
