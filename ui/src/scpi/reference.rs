// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `:REFerence`: `SLOT`, `CURRent`, `SOURce`, `VSCale`, `VOFFset`, `COLor`, `RESet`, `LABel:CONTent`, `LOAD`,
//! `IMPort`, `SAVE`, `SHOW`, `DISPlay` and `CLEar` of the slots R1..R10. A new `:REFerence` command goes into `ref_cmd`
//! here, with its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{e, kw, num, parse_bool, parse_ch, parse_slot, t, Scpi};
use crate::model::*;
use crate::ui::Ui;

/// R1..R10, REF1..REF10, or 1..10.
fn ref_color_index(s: &str) -> Option<usize> {
    if kw(s, "GRAY") || kw(s, "WHITe") {
        Some(0)
    } else if kw(s, "RED") {
        Some(1)
    } else if kw(s, "GREen") {
        Some(2)
    } else if kw(s, "BLUE") {
        Some(3)
    } else if kw(s, "ORANge") || kw(s, "VIOLet") {
        Some(4)
    } else {
        None
    }
}

impl Scpi {
    /// `1,CHAN1` is slot 1 and the rest `CHAN1`. A bare `ON` stays on the current slot. A bad slot number is an error.
    /// Rigol's five reference colors. The screen's fifth color is violet; `ORANge` selects it because that is Rigol's fifth name.
    fn ref_color_name(i: usize) -> &'static str {
        ["GRAY", "RED", "GRE", "BLUE", "ORAN"][i.min(4)]
    }

    fn ref_pair(&mut self, ui: &Ui, arg: &str) -> (Option<usize>, String) {
        let arg = arg.trim();
        let (slot, rest) = if let Some((a, b)) = arg.split_once(',') {
            (parse_slot(a), b.trim().trim_matches('"').to_string())
        } else if arg.is_empty() {
            (Some(ui.ref_slot()), String::new())
        } else if let Some(i) = parse_slot(arg) {
            (Some(i), String::new())
        } else if arg.starts_with(|c: char| c.is_ascii_digit()) || arg.to_ascii_uppercase().starts_with('R') {
            (None, String::new())
        } else {
            (Some(ui.ref_slot()), arg.trim_matches('"').to_string())
        };
        match slot {
            Some(i) if i < NREF => (Some(i), rest),
            _ => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                (None, String::new())
            }
        }
    }

    pub(super) fn ref_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n0 = n.first().copied().unwrap_or("");
        if kw(n0, "SLOT") {
            if query {
                return t(&format!("R{}", ui.ref_slot() + 1));
            }
            return match parse_slot(arg) {
                Some(i) => {
                    ui.ref_slot_set(i);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SHOW") || kw(n0, "DISPlay") {
            let (slot, rest) = self.ref_pair(ui, arg);
            let slot = slot?;
            if query {
                return t(if ui.ref_shown_slot(slot) == Some(true) { "1" } else { "0" });
            }
            return match parse_bool(&rest) {
                Some(on) if ui.ref_show_slot(slot, on) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "CURRent") {
            if query {
                return t(&(ui.ref_slot() + 1).to_string());
            }
            return match parse_slot(arg) {
                Some(i) if ui.ref_slot_set(i) => {
                    ui.mark_changed();
                    None
                }
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if kw(n0, "SOURce") {
            return self.ref_source(ui, query, arg);
        }
        if kw(n0, "VSCale") || kw(n0, "VOFFset") {
            return self.ref_scale(ui, n0, query, arg);
        }
        if n.len() >= 2 && kw(n0, "LABel") && kw(n[1], "CONTent") {
            return self.ref_label(ui, query, arg);
        }
        if kw(n0, "RESet") && !query {
            let (slot, _) = self.ref_pair(ui, arg);
            let slot = slot?;
            if ui.ref_reset_scale(slot) {
                ui.mark_changed();
            } else {
                self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
            }
            return None;
        }
        if kw(n0, "COLor") {
            return self.ref_color_cmd(ui, query, arg);
        }
        if kw(n0, "CLEar") && !query {
            ui.ref_clear_all();
            ui.mark_changed();
            return None;
        }
        if kw(n0, "SAVE") && !query {
            return self.ref_save(ui, arg);
        }
        if kw(n0, "LOAD") && !query {
            if arg.trim().is_empty() {
                self.push_err("-224,\"Illegal parameter value\" no file".into());
                return None;
            }
            let r = ui.ref_load_named(arg);
            return self.ref_loaded(ui, r);
        }
        if kw(n0, "IMPort") && !query {
            let r = if arg.trim().is_empty() { ui.ref_import_slot() } else { ui.ref_load_named(arg) };
            return self.ref_loaded(ui, r);
        }
        self.push_err(format!("-113,\"Undefined header\" :REF:{}", n.join(":")));
        None
    }

    /// `:REFerence:SOURce`: the channel a save of a slot copies.
    fn ref_source(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        let (slot, rest) = self.ref_pair(ui, arg);
        let slot = slot?;
        if query {
            return t(&format!("CHAN{}", ui.ref_source_of(slot) + 1));
        }
        match parse_ch(&rest) {
            Some(c) if ui.ref_source_set(slot, c) => {
                ui.mark_changed();
                None
            }
            _ => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                None
            }
        }
    }

    /// `:REFerence:VSCale|VOFFset` (`n0`) of a slot.
    fn ref_scale(&mut self, ui: &mut Ui, n0: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        let scale = kw(n0, "VSCale");
        let (slot, rest) = self.ref_pair(ui, arg);
        let slot = slot?;
        if query {
            let v = if scale { ui.ref_vscale(slot) } else { ui.ref_voffset(slot) };
            return match v {
                Some(v) => t(&e(v as f64)),
                None => {
                    self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
                    None
                }
            };
        }
        let Some(v) = num(&rest) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        let ok = if scale { ui.ref_vscale_set(slot, v) } else { ui.ref_voffset_set(slot, v) };
        if ok {
            ui.mark_changed();
        } else {
            self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
        }
        None
    }

    /// `:REFerence:LABel:CONTent` of a slot.
    fn ref_label(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        let (slot, rest) = self.ref_pair(ui, arg);
        let slot = slot?;
        if query {
            return match ui.ref_label(slot) {
                Some(s) => t(&s),
                None => {
                    self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
                    None
                }
            };
        }
        if ui.ref_label_set(slot, rest) {
            ui.mark_changed();
        } else {
            self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
        }
        None
    }

    /// `:REFerence:COLor` of a slot.
    fn ref_color_cmd(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        let (slot, rest) = self.ref_pair(ui, arg);
        let slot = slot?;
        if query {
            return match ui.ref_color(slot) {
                Some(i) => t(Self::ref_color_name(i)),
                None => {
                    self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
                    None
                }
            };
        }
        let Some(i) = ref_color_index(&rest) else {
            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
            return None;
        };
        if ui.ref_color_set(slot, i) {
            ui.mark_changed();
        } else {
            self.push_err("-230,\"Data corrupt or stale\" reference is empty".into());
        }
        None
    }

    /// `:REFerence:SAVE`: the Source channel into the selected slot, or into the one `arg` names.
    fn ref_save(&mut self, ui: &mut Ui, arg: &str) -> Option<Vec<u8>> {
        if !arg.trim().is_empty() {
            match parse_slot(arg) {
                Some(i) if ui.ref_slot_set(i) => {}
                _ => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                }
            }
        }
        match ui.ref_save_slot() {
            Ok(()) => {
                ui.mark_changed();
                None
            }
            Err(e) => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", e));
                None
            }
        }
    }

    fn ref_loaded(&mut self, ui: &mut Ui, r: Result<(), String>) -> Option<Vec<u8>> {
        match r {
            Ok(()) => {
                ui.mark_changed();
                None
            }
            Err(e) if e.starts_with("missing ") => {
                self.push_err(format!("-256,\"File name not found\" {}", e.trim_start_matches("missing ")));
                None
            }
            Err(e) => {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", e));
                None
            }
        }
    }
}
