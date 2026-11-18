// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! One declaration per setting (docs/adr/code/0043): a row of a const table names a setting's key in setup files, its
//! type with its range or choices, its menu label, its SCPI command (or why it has none) and its line in
//! docs/scpi.md. The setup file (`store::setup_text` and `store::set`), the remote commands (scpi/display.rs), the
//! menu rows (ui/setting_items.rs) and the table in docs/scpi.md follow from it. Rows only read and write the model;
//! what a change sets off on the screen (redraw, statistics) stays with the caller.
//!
//! The Display group is the first table (settings/display.rs); the other settings are still written out by hand in
//! each of those places. A new Display setting is a row here, its `Item` with its place in a menu (ui/item.rs,
//! ui/menu_items.rs, ui/setting_items.rs), its query in ui/tests/scpi.txt, and
//! `OOS_BLESS=1 cargo test --locked -q settings::tests` for the docs table.

use crate::model::{cycle, Model};

pub mod display;

/// A setting's value between the model and the codecs below; each `Ty` uses one kind.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Val {
    Bool(bool),
    U32(u32),
    /// a choice by its place in the list
    Index(usize),
}

impl Val {
    fn index(self) -> usize {
        match self {
            Val::Bool(b) => b as usize,
            Val::U32(n) => n as usize,
            Val::Index(i) => i,
        }
    }
}

/// What a setting holds, and so how it is written in a setup, over SCPI, in the menu and in the docs.
pub enum Ty {
    /// `true`/`false` in a setup (`on`/`off`/`1`/`0` read too); `ON|OFF|1|0` over SCPI, the query answers `1` or `0`;
    /// Off/On in the menu, a step toggles
    Bool,
    /// a whole number, kept within `min..=max` wherever it comes from; the menu steps by `step` and shows `<n> <unit>`
    U32 { min: u32, max: u32, step: u32, unit: &'static str },
    /// one of a list; how a setup refuses a word that is none of them
    Choice(&'static [Choice], Unknown),
    /// a setting whose words follow none of the rules above
    Custom(&'static CustomIo),
}

/// One choice: its word in a setup file, its SCPI keyword (long form; the query answers the short form), its label
/// in the menu.
pub struct Choice {
    pub store: &'static str,
    pub scpi: &'static str,
    pub label: &'static str,
}

/// The error a setup reader gives for a word that is no choice, as each key gave it before the table.
pub enum Unknown {
    /// `unknown value: <word>`
    Value,
    /// `"<word>": a or b`
    Names,
}

/// The codecs of a `Ty::Custom` row, on the row's `Val`.
pub struct CustomIo {
    pub store: fn(Val) -> String,
    pub load: fn(&str) -> Result<Val, String>,
    pub query: fn(Val) -> String,
    /// the SCPI argument; None refuses it
    pub take: fn(&str) -> Option<Val>,
    /// the menu's options; the value is the label at the row's index (`Val::Bool` counts as 0 and 1)
    pub labels: &'static [&'static str],
    /// what the command takes, for docs/scpi.md (only the tests read the docs)
    #[cfg(test)]
    pub values: &'static str,
}

/// The remote command of a setting, its nodes below its subsystem (`GRADing:TIME`), or why it has none.
pub enum Scpi {
    Cmd(&'static str),
    None {
        #[cfg(test)]
        why: &'static str,
    },
}

pub struct Setting {
    /// the key in setup files; stored keys never change once released
    pub key: &'static str,
    pub scpi: Scpi,
    pub ty: Ty,
    /// the menu row's label
    pub label: &'static str,
    /// what it does, for docs/scpi.md
    #[cfg(test)]
    pub doc: &'static str,
    /// the SCPI input range of a `U32` when it differs from `min..=max`; a value in it is then kept within those
    pub accept: Option<(f64, f64)>,
    pub get: fn(&Model) -> Val,
    pub put: fn(&mut Model, Val),
}

/// Every row of every table.
pub fn all() -> impl Iterator<Item = &'static Setting> {
    display::GROUPS.into_iter().flat_map(|g| g.iter())
}

/// The row of a setup key.
pub fn find(key: &str) -> Option<&'static Setting> {
    all().find(|s| s.key == key)
}

/// The `key = value` lines of `rows`, in their order.
pub fn write(rows: &[Setting], m: &Model, kv: &mut dyn FnMut(&str, String)) {
    for s in rows {
        kv(s.key, s.store(m));
    }
}

impl Setting {
    /// The value as a setup file writes it.
    pub fn store(&self, m: &Model) -> String {
        let v = (self.get)(m);
        match &self.ty {
            Ty::Bool | Ty::U32 { .. } => match v {
                Val::Bool(b) => b.to_string(),
                v => v.index().to_string(),
            },
            Ty::Choice(list, _) => list[v.index()].store.to_string(),
            Ty::Custom(io) => (io.store)(v),
        }
    }

    /// Reads the value of a setup line; the error names what was wrong, as `store::set` reports it.
    pub fn load(&self, m: &mut Model, text: &str) -> Result<(), String> {
        let v = match &self.ty {
            Ty::Bool => Val::Bool(crate::store::p_bool(text)?),
            Ty::U32 { min, max, .. } => Val::U32(crate::store::p_u32(text)?.clamp(*min, *max)),
            Ty::Choice(list, unknown) => {
                Val::Index(list.iter().position(|c| c.store == text).ok_or_else(|| match unknown {
                    Unknown::Value => format!("unknown value: {}", text),
                    Unknown::Names => {
                        format!("{:?}: {}", text, list.iter().map(|c| c.store).collect::<Vec<_>>().join(" or "))
                    }
                })?)
            }
            Ty::Custom(io) => (io.load)(text)?,
        };
        (self.put)(m, v);
        Ok(())
    }

    /// The answer to the SCPI query.
    pub fn query(&self, m: &Model) -> String {
        let v = (self.get)(m);
        match &self.ty {
            Ty::Bool => if v.index() != 0 { "1" } else { "0" }.to_string(),
            Ty::U32 { .. } => v.index().to_string(),
            Ty::Choice(list, _) => {
                list[v.index()].scpi.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect()
            }
            Ty::Custom(io) => (io.query)(v),
        }
    }

    /// Sets the value an SCPI argument names; false (the model untouched) when it names none.
    pub fn take(&self, m: &mut Model, arg: &str) -> bool {
        let v = match &self.ty {
            Ty::Bool => crate::scpi::parse_bool(arg).map(Val::Bool),
            Ty::U32 { min, max, .. } => {
                let (lo, hi) = self.accept.unwrap_or((*min as f64, *max as f64));
                arg.trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| (lo..=hi).contains(&(n as f64)))
                    .map(|n| Val::U32(n.clamp(*min, *max)))
            }
            Ty::Choice(list, _) => list.iter().position(|c| crate::scpi::kw(arg, c.scpi)).map(Val::Index),
            Ty::Custom(io) => (io.take)(arg),
        };
        v.map(|v| (self.put)(m, v)).is_some()
    }

    /// The menu row is a number with - and + (a choice otherwise).
    pub fn is_number(&self) -> bool {
        matches!(self.ty, Ty::U32 { .. })
    }

    /// The menu's options (none for a number).
    pub fn options(&self) -> Vec<String> {
        let labels: Vec<&str> = match &self.ty {
            Ty::Bool => vec!["Off", "On"],
            Ty::U32 { .. } => Vec::new(),
            Ty::Choice(list, _) => list.iter().map(|c| c.label).collect(),
            Ty::Custom(io) => io.labels.to_vec(),
        };
        labels.into_iter().map(str::to_string).collect()
    }

    /// The value the menu row shows.
    pub fn shown(&self, m: &Model) -> String {
        let v = (self.get)(m);
        match &self.ty {
            Ty::U32 { unit, .. } => format!("{} {}", v.index(), unit),
            _ => self.options()[v.index()].clone(),
        }
    }

    /// A step of the menu row by `d`: a number by `d` steps within its range, a list round by `d`; a switch or a list
    /// of two changes on every step.
    pub fn step(&self, m: &mut Model, d: i32) {
        let v = (self.get)(m);
        let v = match (&self.ty, v) {
            (_, Val::Bool(b)) => Val::Bool(!b),
            (Ty::U32 { min, max, step, .. }, Val::U32(n)) => {
                Val::U32((n as i32 + d * *step as i32).clamp(*min as i32, *max as i32) as u32)
            }
            (_, v) => {
                let n = self.options().len();
                Val::Index(if n == 2 { 1 - v.index() } else { cycle(v.index(), n, d) })
            }
        };
        (self.put)(m, v);
    }

    /// What the SCPI command takes, for its row in docs/scpi.md.
    #[cfg(test)]
    fn takes(&self) -> String {
        match &self.ty {
            Ty::Bool => "`ON`, `OFF`, `1`, `0`; the query answers `1` or `0`".into(),
            Ty::U32 { min, max, unit, .. } => {
                let (lo, hi) = self.accept.unwrap_or((*min as f64, *max as f64));
                format!("a whole number from {} to {}, set within {} to {} {}", lo, hi, min, max, unit)
            }
            Ty::Choice(list, _) => list.iter().map(|c| format!("`{}`", c.scpi)).collect::<Vec<_>>().join(", "),
            Ty::Custom(io) => io.values.into(),
        }
    }
}

/// The rows of `rows` as the Markdown table docs/scpi.md holds between `<!-- settings:<group>:begin -->` and
/// `<!-- settings:<group>:end -->`; `subsystem` is the SCPI root of the group (`:DISPlay`).
#[cfg(test)]
pub fn docs_table(rows: &[&[Setting]], subsystem: &str, menu: &str) -> String {
    let mut s =
        format!("| Command | Takes | Setup key | {} menu | What it does |\n| --- | --- | --- | --- | --- |\n", menu);
    for r in rows.iter().flat_map(|g| g.iter()) {
        let (cmd, takes) = match r.scpi {
            Scpi::Cmd(c) => (format!("`{}:{}[?]`", subsystem, c), r.takes()),
            Scpi::None { why } => ("none".to_string(), why.to_string()),
        };
        s.push_str(&format!("| {} | {} | `{}` | {} | {} |\n", cmd, takes, r.key, r.label, r.doc));
    }
    s
}

#[cfg(test)]
mod tests;
