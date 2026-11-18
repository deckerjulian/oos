// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:SYSTem`: the error queue (`ERRor?`), `STATe?`, `GRAPhics`, `PIPeline` (a row of settings/system.rs), `AOUTput`,
//! `LOG`, `SET` and `GET`, and the fork's `:SYSTem:DEBug:...` nodes with the raw state of the FPGA units. A new
//! `:SYSTem` setting of a setup file is a row of that table; another command goes into `system_cmd` here (a debug node
//! into `system_debug_cmd`), with its line in docs/scpi.md; either with its query in ui/tests/scpi.txt (AGENTS.md).

use super::{kw, parse_bool, Scpi};
use crate::board::{board, Board};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    /// `:SYSTem` (`nodes` with the root).
    pub(super) fn system_cmd(
        &mut self,
        ui: &mut Ui,
        nodes: &[&str],
        query: bool,
        arg: &str,
        cmd: &str,
    ) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        if nodes.len() >= 3 && kw(nodes[1], "DEBug") {
            return self.system_debug_cmd(ui, nodes, query, arg, cmd);
        }
        // test access: any key of a setup file on the live settings (`:SYST:SET ch1.bw = 20M`), its value, the state
        if nodes.len() == 2 {
            if kw(nodes[1], "SET") && !query {
                let r = arg
                    .split_once('=')
                    .ok_or("no '='".to_string())
                    .and_then(|(k, v)| crate::store::set(&mut ui.m, k.trim(), v.trim()));
                match r {
                    Ok(()) => ui.mark_changed(),
                    Err(e) => self.push_err(format!("-224,\"Illegal parameter value\" {}", e)),
                }
                // a setup file may hold keys of newer versions, here a typo must not pass for a setting; the keys that
                // are only written when they differ from their default do not show
                let k = arg.split('=').next().unwrap_or("").trim();
                let optional = ["vfine", "unit", "bias", "skew", "label", "tfine"].iter().any(|s| k.ends_with(s))
                    || k.starts_with("math_expr")
                    || k == "cat";
                if !optional
                    && !crate::store::setup_text(&ui.m).lines().any(|l| l.split('=').next().unwrap_or("").trim() == k)
                {
                    self.push_err(format!("-224,\"Illegal parameter value\" unknown key {}", k));
                }
                return None;
            }
            if kw(nodes[1], "GET") && query {
                let text = crate::store::setup_text(&ui.m);
                let v = text.lines().find_map(|l| {
                    l.split_once('=').filter(|(k, _)| k.trim() == arg.trim()).map(|x| x.1.trim().to_string())
                });
                return t(&v.unwrap_or_default());
            }
            if kw(nodes[1], "STATe") && query {
                return t(crate::cmd::stat_text(ui).trim());
            }
            // the rows of settings/system.rs (Acquire > Draw path); a change does not mark the model changed
            if let Some(row) = crate::settings::command(&crate::settings::system::SLICES, &nodes[1..]) {
                if query {
                    return t(&row.query(&ui.m, 0));
                }
                if !row.take(&mut ui.m, 0, arg) {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                }
                return None;
            }
            // System > Graphics: CPU or GPU (whether the GPU really draws: the `gpu` part of :SYSTem:STATe?)
            if kw(nodes[1], "GRAPhics") {
                if query {
                    return t(if ui.sys.gpu { "GPU" } else { "CPU" });
                }
                match arg.trim().to_ascii_uppercase().as_str() {
                    "GPU" => ui.graphics_set(true),
                    "CPU" => ui.graphics_set(false),
                    _ => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // rear AUX OUT, stock names TOUT and PFAil; OFF is OOS's (constant low)
            if kw(nodes[1], "AOUTput") {
                if query {
                    return t(match ui.m.aux_out {
                        AuxOut::TrigOut => "TOUT",
                        AuxOut::Off => "OFF",
                        AuxOut::PassFail => "PFA",
                    });
                }
                let want = match arg.trim().to_ascii_uppercase().as_str() {
                    "TOUT" => Some(AuxOut::TrigOut),
                    "OFF" => Some(AuxOut::Off),
                    "PFA" | "PFAIL" => Some(AuxOut::PassFail),
                    _ => None,
                };
                match want.filter(|a| a.offered(board().mask_hw() || board().zone_hw())) {
                    Some(a) => {
                        ui.m.aux_out = a;
                        ui.mark_changed();
                    }
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if kw(nodes[1], "LOG") && !query {
                match parse_bool(arg) {
                    Some(on) => ui.log_set(on),
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
        }
        if nodes.len() == 2 && kw(nodes[1], "ERRor") && query {
            return t(&if self.err.is_empty() { "0,\"No error\"".to_string() } else { self.err.remove(0) });
        }
        self.push_err(format!("-113,\"Undefined header\" {}", cmd));
        None
    }

    /// `:SYSTem:DEBug:...` of the fork, the raw state of the FPGA units next to the software (`nodes` with the root).
    fn system_debug_cmd(&mut self, ui: &mut Ui, nodes: &[&str], query: bool, arg: &str, cmd: &str) -> Option<Vec<u8>> {
        let t = |s: &str| Some(s.as_bytes().to_vec());
        // fork: the TPU UART receiver's words for the current settings (docs/fork/hwuart.md)
        if nodes.len() == 3 && (kw(nodes[2], "UART") || kw(nodes[2], "SERial")) && query {
            return t(&board().uart_debug(&ui.m));
        }
        // fork: the zone unit's raw status and counter next to the software filter (docs/fork/hwzone.md)
        if nodes.len() == 3 && kw(nodes[2], "ZONE") && query {
            return t(&ui.zone_debug_text());
        }
        // fork: the comparator mapping of the mask test and zone trigger, and the per-column view (docs/fork/hwmask.md)
        if nodes.len() == 4 && kw(nodes[2], "MASK") && kw(nodes[3], "MAP") {
            match board().mask_map(if query { None } else { Some(arg.trim()) }) {
                Ok(s) if query => return t(&s),
                Ok(_) => ui.mark_changed(),
                Err(e) => self.err.push(format!("-224,\"Illegal parameter value\" {}", e)),
            }
            return None;
        }
        if nodes.len() == 4 && kw(nodes[2], "MASK") && kw(nodes[3], "COLumns") && query {
            let mut it = arg.split(',').map(|x| x.trim().parse::<usize>());
            let (a, b) = match (it.next(), it.next()) {
                (Some(Ok(a)), Some(Ok(b))) => (a.min(b), a.max(b)),
                (Some(Ok(a)), None) => (a, a),
                _ => (0, 9),
            };
            return t(&ui.mask_columns_text(a, b));
        }
        // fork: the mask unit's raw counters and slot next to the software test (docs/fork/hwmask.md)
        if nodes.len() == 3 && kw(nodes[2], "MASK") && query {
            return t(&ui.mask_debug_text());
        }
        // fork: the DVM block's raw registers next to the software DVM (docs/fork/dvm.md)
        if nodes.len() == 3 && kw(nodes[2], "DVM") && query {
            return t(&ui.dvm_debug_text());
        }
        // fork, unit debug: force the DVM block's lane with one channel (each lane = one pair of ADC cores)
        if nodes.len() == 4 && kw(nodes[2], "DVM") && kw(nodes[3], "LANE") {
            if query {
                return t(&ui.dvm_lane_text());
            }
            match arg.trim().to_ascii_uppercase().as_str() {
                "AUTO" => ui.dvm_lane_set(None),
                a => match a.parse::<u32>() {
                    Ok(l) if l < 4 => ui.dvm_lane_set(Some(l)),
                    _ => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                },
            }
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" {}", cmd));
        None
    }
}

#[cfg(test)]
mod aux_out_tests {
    use super::*;

    #[test]
    fn aux_out_over_scpi_and_in_the_setup() {
        let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
        let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
        assert_eq!(q(&mut ui, ":SYST:AOUT?"), "TOUT", "trigger out by default, as the FPGA powers up");
        q(&mut ui, ":SYSTem:AOUTput off");
        assert_eq!((q(&mut ui, ":SYSTem:AOUTput?"), ui.m.aux_out), ("OFF".to_string(), AuxOut::Off));
        assert!(crate::store::setup_text(&ui.m).contains("aux_out = Off\n"));
        q(&mut ui, ":SYST:AOUT TOUT");
        assert_eq!(ui.m.aux_out, AuxOut::TrigOut);
        q(&mut ui, ":SYST:AOUT PFAil");
        assert_eq!(ui.m.aux_out == AuxOut::PassFail, board().mask_hw() || board().zone_hw());
        q(&mut ui, ":SYST:AOUT TOUT");
        q(&mut ui, ":SYST:AOUT blink");
        assert!(q(&mut ui, ":SYST:ERR?").starts_with("-224"));
        // fork: the mask unit's debug line, off without the hardware test
        assert!(q(&mut ui, ":SYSTem:DEBug:MASK?").starts_with("off,enabled=0,active=0,blocked=0,sw_pass=0,sw_fail=0,"));
        assert!(q(&mut ui, ":SYSTem:DEBug:ZONE?").starts_with("off,enabled=0,active=0,sw_drops=0,"));
        assert_eq!(q(&mut ui, ":SYSTem:DEBug:UART?"), "off,enabled=0");
        assert_eq!(q(&mut ui, ":SYSTem:DEBug:SERial?"), "off,enabled=0");
        // the protocol trigger's settings through the dispatcher, into the setup
        q(&mut ui, ":TRIGger:RS232:WHEN DATA");
        q(&mut ui, ":TRIG:RS232:RELation OUT");
        q(&mut ui, ":TRIG:RS232:DMAX 0x7e");
        assert_eq!(
            (q(&mut ui, ":TRIGger:RS232:RELation?"), ui.m.ser.max, ui.m.trig_bus),
            ("OUT".to_string(), 0x7e, BusWhat::Data)
        );
        assert!(crate::store::setup_text(&ui.m).contains("ser.rel = Out\n"));
        q(&mut ui, ":TRIG:IIC:AWIDth 8");
        assert!(q(&mut ui, ":SYST:ERR?").starts_with("-"));
        while !q(&mut ui, ":SYST:ERR?").starts_with("0,") {}
        // the comparator mapping: set one key, read it back, put the original back
        let orig = q(&mut ui, ":SYSTem:DEBug:MASK:MAP?");
        assert!(orig.starts_with("domain="), "{}", orig);
        q(&mut ui, ":SYSTem:DEBug:MASK:MAP cols=stock");
        assert!(q(&mut ui, ":SYST:DEB:MASK:MAP?").contains("cols=stock"));
        q(&mut ui, ":SYST:DEB:MASK:MAP cols=bogus");
        assert!(q(&mut ui, ":SYST:ERR?").starts_with("-224"));
        for kv in orig.split(' ') {
            q(&mut ui, &format!(":SYST:DEB:MASK:MAP {}", kv));
        }
        assert_eq!(q(&mut ui, ":SYST:DEB:MASK:MAP?"), orig);
        assert_eq!(q(&mut ui, ":SYST:DEB:MASK:COL? 0,3"), "off");
    }
}

#[cfg(test)]
mod draw_path_tests {
    use super::*;

    // Break it catches: the draw path switch not reaching the model or the setup, or the pipe running where the
    // switch is off
    #[test]
    fn draw_path_over_scpi_in_the_menu_and_the_setup() {
        let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
        let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
        assert_eq!(q(&mut ui, ":SYST:PIP?"), "BATC", "Batch is the default (ADR 0029)");
        q(&mut ui, ":SYST:PIP STANdard");
        assert!(!ui.m.draw_batch);
        assert!(crate::store::setup_text(&ui.m).contains("draw_path = standard\n"));
        assert!(!crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap().draw_batch);
        q(&mut ui, ":SYSTem:PIPeline BATCh");
        assert!(ui.m.draw_batch);
        assert!(crate::store::setup_text(&ui.m).contains("draw_path = batch\n"));
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert!(back.draw_batch);
        // the demo source is not the hardware: no workers
        ui.step(0.02);
        assert!(crate::drawpipe::pipe().is_none());
        q(&mut ui, ":SYST:PIP FRAM");
        assert!(!ui.m.draw_batch);
        q(&mut ui, ":SYST:PIP BOGUS");
        assert!(q(&mut ui, ":SYST:ERR?").starts_with("-224"));
    }
}
