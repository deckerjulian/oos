// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The figures of the analyser pages: `:EYE:ITEM?`, and the source lists and single answers of `:POWer:ITEM?`,
//! `:SPECtrum:ITEM?`, `:MAGnetic:ITEM?`, `:INVerter:ITEM?`, `:DPT:ITEM?` and `:BANDwidth:ITEM?`, whose headers
//! `exec_cmd` tries. A new figure query of an analyser page goes beside them, with its line in
//! docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{arg_item, arg_rest, e, kw, parse_wave, Scpi};
use crate::model::*;
use crate::ui::Ui;

impl Scpi {
    pub(super) fn waves(&mut self, arg: &str) -> Result<Vec<WaveId>, String> {
        let mut v = Vec::new();
        for s in arg.split(',') {
            let s = s.trim();
            if s.is_empty() {
                continue;
            }
            // the item name is not a source; callers pass only the source list
            match parse_wave(s) {
                Some(id) => v.push(id),
                None => return Err(format!("-224,\"Illegal parameter value\" {}", s)),
            }
        }
        Ok(v)
    }

    pub(super) fn sourced(
        &mut self,
        ui: &Ui,
        arg: &str,
        f: impl FnOnce(&Ui, &str, &[WaveId]) -> Result<f64, ()>,
    ) -> Option<Vec<u8>> {
        let src = match self.waves(arg_rest(arg)) {
            Ok(v) => v,
            Err(s) => {
                self.push_err(s);
                return Some(b"9.9E37".to_vec());
            }
        };
        self.one_fact(f(ui, &arg_item(arg), &src))
    }

    fn one_fact(&mut self, r: Result<f64, ()>) -> Option<Vec<u8>> {
        match r {
            Ok(v) => Some(e(v).into_bytes()),
            Err(()) => {
                self.push_err("-224,\"Illegal parameter value\"".into());
                Some(b"9.9E37".to_vec())
            }
        }
    }

    /// :EYE:ITEM? <figure> — height, width, unit interval, total jitter, and the other figures on the eye page.
    pub(super) fn eye_item(&mut self, ui: &Ui, arg: &str) -> Option<Vec<u8>> {
        let item = arg.split(',').next().unwrap_or("").trim();
        let Some(name) = crate::ui::EYE_ITEMS.iter().find(|n| kw(item, n)) else {
            self.push_err("-224,\"Illegal parameter value\"".into());
            return Some(b"9.9E37".to_vec());
        };
        let v = ui.eye_readings().iter().find(|(n, _)| n == name).map(|(_, v)| *v).unwrap_or(f64::NAN);
        Some(e(v).into_bytes())
    }
}
