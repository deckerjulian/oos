// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:CURSor`: `MODE`, the manual and track cursors (`MANual:TYPE`, `CAX|CBX|CAY|CBY`, `AXValue|AYValue|BXValue|
//! BYValue`), the deltas (`XDELta|IXDelta|YDELta`) and `MEASure:INDicator`. A new `:CURSor` command goes into
//! `cursor_cmd` here, with its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, parse_wave, t, wave_tok, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn cursor_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n0 = n.first().copied().unwrap_or("");
        if n.is_empty() || (n.len() == 1 && kw(n0, "MODE")) {
            if n.is_empty() {
                self.push_err("-113,\"Undefined header\" :CURSor".into());
                return None;
            }
            return self.cursor_mode(ui, query, arg);
        }
        if n.len() == 2 && kw(n0, "MEASure") && kw(n[1], "INDicator") {
            return self.meas_indicator(ui, query, arg);
        }
        if n.len() == 2 && kw(n0, "MANual") && kw(n[1], "TYPE") {
            return self.cursor_type(ui, query, arg);
        }
        if n.len() == 2
            && (kw(n0, "MANual") || kw(n0, "TRACk"))
            && (kw(n[1], "SOURce") || kw(n[1], "SOURce1") || kw(n[1], "SOURce2"))
        {
            return self.cursor_src(ui, n, query, arg);
        }
        if n.len() != 2 {
            self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
            return None;
        }
        let which = n[1];
        let xy = kw(n0, "XY");
        let known = kw(n0, "MANual") || kw(n0, "TRACk") || xy;
        if !known {
            self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
            return None;
        }
        let x = kw(which, "CAX") || (xy && kw(which, "AX"));
        let xb = kw(which, "CBX") || (xy && kw(which, "BX"));
        let y = kw(which, "CAY") || (xy && kw(which, "AY"));
        let yb = kw(which, "CBY") || (xy && kw(which, "BY"));
        if x || xb {
            return self.cursor_axis(ui, query, arg, true, x);
        }
        if y || yb {
            return self.cursor_axis(ui, query, arg, false, y);
        }
        if !query {
            self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
            return None;
        }
        self.cursor_readout(ui, n)
    }

    /// `:CURSor:MODE`: OFF, MANual, TRACk or XY.
    fn cursor_mode(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            let s = match ui.m.cursor {
                CursorMode::Off => "OFF",
                CursorMode::Time | CursorMode::Volt => "MAN",
                CursorMode::Track => "TRAC",
                CursorMode::Xy => "XY",
            };
            return t(s);
        }
        let a = arg.trim();
        ui.m.cursor = if kw(a, "OFF") {
            CursorMode::Off
        } else if kw(a, "MANual") {
            if ui.m.cursor == CursorMode::Volt {
                CursorMode::Volt
            } else {
                CursorMode::Time
            }
        } else if kw(a, "TRACk") {
            CursorMode::Track
        } else if a.eq_ignore_ascii_case("XY") {
            CursorMode::Xy
        } else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        None
    }

    /// `:CURSor:MANual:TYPE`: TIME or AMPLitude.
    fn cursor_type(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            return t(if ui.m.cursor == CursorMode::Volt { "AMPL" } else { "TIME" });
        }
        if kw(arg, "TIME") {
            if ui.m.cursor != CursorMode::Xy {
                ui.m.cursor = CursorMode::Time;
            }
        } else if kw(arg, "AMPLitude") {
            if ui.m.cursor != CursorMode::Xy {
                ui.m.cursor = CursorMode::Volt;
            }
        } else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        }
        None
    }

    /// `:CURSor:MANual|TRACk:SOURce[1|2]`: the waveform a cursor reads (`n` without the root).
    fn cursor_src(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n0 = n[0];
        let second = kw(n[1], "SOURce2");
        let track = kw(n0, "TRACk");
        if query {
            return t(&wave_tok(if second && track { ui.cursor_b_source() } else { ui.cursor_source() }));
        }
        let Some(source) = parse_wave(arg) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let ok = if second && track {
            ui.set_cursor_b_source(source)
        } else if second {
            source == ui.cursor_source()
        } else {
            ui.set_cursor_source(source)
        };
        if !ok {
            self.push_err("-224,\"Cursor source is unavailable or unsupported\"".into());
        }
        None
    }

    /// The read-outs under `:CURSor:MANual|TRACk|XY` (`AXValue?` to `YDELta?`, `n` without the root).
    fn cursor_readout(&mut self, ui: &Ui, n: &[&str]) -> Option<Vec<u8>> {
        let which = n[1];
        let miss = || Some(b"9.9E37".to_vec());
        let (ta, tb) = (ui.cursor_time_at(ui.m.cur_a), ui.cursor_time_at(ui.m.cur_b));
        let y_of = |a: bool| {
            if ui.m.cursor == CursorMode::Xy {
                ui.cursor_volt_at(if a { ui.m.cur_c } else { ui.m.cur_d })
            } else if ui.m.cursor == CursorMode::Volt {
                ui.cursor_volt_at(if a { ui.m.cur_a } else { ui.m.cur_b })
            } else if matches!(ui.m.cursor, CursorMode::Track | CursorMode::Time) {
                let source =
                    if !a && ui.m.cursor == CursorMode::Track { ui.cursor_b_source() } else { ui.cursor_source() };
                ui.wave_at_source(source, if a { ta } else { tb }).map(|v| v as f64).unwrap_or(f64::NAN)
            } else {
                f64::NAN
            }
        };
        if kw(which, "AXValue") {
            return if ui.m.cursor == CursorMode::Volt || ui.m.cursor == CursorMode::Off { miss() } else { t(&e(ta)) };
        }
        if kw(which, "BXValue") {
            return if ui.m.cursor == CursorMode::Volt || ui.m.cursor == CursorMode::Off { miss() } else { t(&e(tb)) };
        }
        if kw(which, "AYValue") {
            return t(&e(y_of(true)));
        }
        if kw(which, "BYValue") {
            return t(&e(y_of(false)));
        }
        if kw(which, "XDELta") {
            return if matches!(ui.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) {
                t(&e(tb - ta))
            } else {
                miss()
            };
        }
        if kw(which, "IXDelta") {
            let d = (tb - ta).abs();
            return if d > 0.0 && matches!(ui.m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) {
                t(&e(1.0 / d))
            } else {
                miss()
            };
        }
        if kw(which, "YDELta") {
            if ui.m.cursor == CursorMode::Track
                && ui.cursor_source_unit(ui.cursor_source()) != ui.cursor_source_unit(ui.cursor_b_source())
            {
                return miss();
            }
            let d = y_of(false) - y_of(true);
            return t(&e(d));
        }
        self.push_err(format!("-113,\"Undefined header\" :CURSor:{}", n.join(":")));
        None
    }

    /// Horizontal (`x`) or vertical cursor. `a` is cursor A.
    fn cursor_axis(&mut self, ui: &mut Ui, query: bool, arg: &str, x: bool, a: bool) -> Option<Vec<u8>> {
        let place = |ui: &Ui, x: bool, a: bool| -> Option<f32> {
            Some(match (x, ui.m.cursor) {
                (true, CursorMode::Volt | CursorMode::Off) => return None,
                (false, CursorMode::Time | CursorMode::Track | CursorMode::Off) => return None,
                (false, CursorMode::Xy) => {
                    if a {
                        ui.m.cur_c
                    } else {
                        ui.m.cur_d
                    }
                }
                (false, CursorMode::Volt) => {
                    if a {
                        ui.m.cur_a
                    } else {
                        ui.m.cur_b
                    }
                }
                (true, _) => {
                    if a {
                        ui.m.cur_a
                    } else {
                        ui.m.cur_b
                    }
                }
            })
        };
        if query {
            let Some(frac) = place(ui, x, a) else { return Some(b"9.9E37".to_vec()) };
            let v = if x { ui.cursor_time_at(frac) } else { ui.cursor_volt_at(frac) };
            return Some(e(v).into_bytes());
        }
        let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite()) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let frac = if x { ui.cursor_frac_time(v) } else { ui.cursor_frac_volt(v) };
        match (x, ui.m.cursor) {
            (true, CursorMode::Volt) => {
                ui.m.cur_c = ui.m.cur_a;
                ui.m.cur_d = ui.m.cur_b;
                if a {
                    ui.m.cur_a = frac;
                } else {
                    ui.m.cur_b = frac;
                }
                ui.m.cursor = CursorMode::Xy;
            }
            (false, CursorMode::Time | CursorMode::Track) => {
                if a {
                    ui.m.cur_c = frac;
                } else {
                    ui.m.cur_d = frac;
                }
                ui.m.cursor = CursorMode::Xy;
            }
            (false, CursorMode::Xy) => {
                if a {
                    ui.m.cur_c = frac;
                } else {
                    ui.m.cur_d = frac;
                }
            }
            (true, CursorMode::Off) => {
                if a {
                    ui.m.cur_a = frac;
                } else {
                    ui.m.cur_b = frac;
                }
                ui.m.cursor = CursorMode::Time;
            }
            (false, CursorMode::Off) => {
                if a {
                    ui.m.cur_a = frac;
                } else {
                    ui.m.cur_b = frac;
                }
                ui.m.cursor = CursorMode::Volt;
            }
            (true, _) => {
                if a {
                    ui.m.cur_a = frac;
                } else {
                    ui.m.cur_b = frac;
                }
            }
            (false, _) => {
                if a {
                    ui.m.cur_a = frac;
                } else {
                    ui.m.cur_b = frac;
                }
            }
        }
        None
    }
}
