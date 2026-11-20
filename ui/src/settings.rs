// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! One declaration per setting (docs/adr/code/0043): a row of a const table names a setting's key in setup files, its
//! type with its range or choices, its menu label, its SCPI command (or why it has none) and its line in
//! docs/scpi.md. The setup file (`store::setup_text` and `store::set`), the remote commands (scpi/display.rs, the
//! `:CHANnel`, `:TRIGger`, `:ACQuire` and `:BODE` branches of scpi/dispatch.rs, scpi/math.rs, scpi/dvm.rs, scpi/counter.rs,
//! scpi/mask.rs, scpi/histogram.rs, scpi/la.rs), the menu rows (ui/setting_items.rs) and the tables in docs/scpi.md follow from it. Rows only read
//! and write the model; what a change sets off on the screen (redraw, statistics) stays with the caller.
//!
//! The Display group (settings/display.rs), the trigger values (settings/trigger.rs), the acquisition settings
//! (settings/acquisition.rs), the math settings (settings/math.rs), the meters (settings/meters.rs), the channel
//! settings (settings/channel.rs), the switches of the mask test
//! (settings/mask.rs), the histogram (settings/histogram.rs), the Bode page (settings/bode.rs), the
//! switch of the logic analyser (settings/la.rs), the spectrum page (settings/spectrum.rs), the eye page
//! (settings/eye.rs) and the power analysis (settings/power.rs) have tables; the other
//! settings are still written out by hand in each of those places. The numbers of the analyser pages are `Ty::Real`
//! rows (f64 fields as they are stored) with a step of their own (docs/adr/code/0044). A numbered family is one row for all its instances: `<n>` in its key
//! stands for the instance's number, and the getter and setter take the instance's index (0 for a row that is not
//! numbered).
//! A new setting of a group with a table is a row there, its `Item` with its place in a menu (ui/item.rs,
//! ui/menu_items.rs, ui/setting_items.rs), its query in ui/tests/scpi.txt, and
//! `OOS_BLESS=1 cargo test --locked -q settings::tests` for the docs table.

use crate::model::{cycle, si_short, step_index, Model};

pub mod acquisition;
pub mod bode;
pub mod channel;
pub mod display;
pub mod eye;
pub mod histogram;
pub mod la;
pub mod mask;
pub mod math;
pub mod meters;
pub mod power;
pub mod spectrum;
pub mod trigger;

/// A setting's value between the model and the codecs below; each `Ty` uses one kind.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Val {
    Bool(bool),
    U32(u32),
    /// a choice by its place in the list
    Index(usize),
    /// seconds, hertz, or a real number of an f32 field
    F32(f32),
    /// a real number of an f64 field
    F64(f64),
}

impl Val {
    fn index(self) -> usize {
        match self {
            Val::Bool(b) => b as usize,
            Val::U32(n) => n as usize,
            Val::Index(i) => i,
            Val::F32(x) => x as usize,
            Val::F64(x) => x as usize,
        }
    }

    fn secs(self) -> f32 {
        match self {
            Val::F32(x) => x,
            Val::F64(x) => x as f32,
            v => v.index() as f32,
        }
    }

    fn real(self) -> f64 {
        match self {
            Val::F32(x) => x as f64,
            Val::F64(x) => x,
            v => v.index() as f64,
        }
    }
}

/// What a setting holds, and so how it is written in a setup, over SCPI, in the menu and in the docs.
pub enum Ty {
    /// `true`/`false` in a setup (`on`/`off`/`1`/`0` read too); `ON|OFF|1|0` over SCPI, the query answers `1` or `0`;
    /// Off/On in the menu, a step toggles
    Bool,
    /// a whole number, kept within `min..=max` wherever it comes from; the menu steps by `step` and shows `<n> <unit>`
    /// (`<n>` without a unit)
    U32 { min: u32, max: u32, step: u32, unit: &'static str },
    /// a whole number the menu offers as a list of steps: a setup reads any (0 as 1); over SCPI one from the first
    /// step, set to the step at or below it; the menu steps along the list, from the default's step when the value is
    /// on none, and shows `<n>`
    Steps(&'static [u32]),
    /// one of a list; how a setup refuses a word that is none of them, and how an SCPI argument names one
    Choice(&'static [Choice], Unknown, Arg),
    /// seconds, any finite number in a setup; over SCPI a number (within `accept` when the row has it), the query
    /// answers it as `1.000000E-06`; the menu steps along `ladder` and shows `<n> s`, or `zero` for 0 and below
    Seconds { ladder: fn(f32, i32) -> f32, zero: Option<&'static str> },
    /// hertz, as `Seconds` without `zero`; the menu shows `<n> Hz`
    Hertz { ladder: fn(f32, i32) -> f32 },
    /// a real number as its field keeps it: `Val::F64` in a setup in Rust's shortest exact form, read as `str::parse`
    /// reads it (`inf` and `NaN` too, with its error text, as the setup lines of the analyser pages read theirs), or
    /// `Val::F32` for an f32 field, its shortest form, read as a finite number; within `range` when the row has one,
    /// wherever it comes from. Over SCPI a number (within `accept` when the row has it), the query answers
    /// `1.000000E+03`. The menu shows `shown` of it and steps by the row's `step`, which such a row has
    Real { range: Option<(f64, f64)>, shown: fn(f64) -> String },
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
    /// `<text> <word>`
    Text(&'static str),
    /// `"<word>": a or b`
    Names,
}

/// How an SCPI argument names a choice.
pub enum Arg {
    /// the long or the short form of its keyword (`kw`)
    Keyword,
    /// any word that starts with the short form of its keyword, in any case (`POSitive` takes `pos` and `POSXYZ`)
    Prefix,
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
    /// the key in setup files; stored keys never change once released. `<line> <field>` (`dvm mode`) names a field of
    /// a setup line that holds several settings, which store.rs writes and reads by hand (`Setting::line`); `<n>`
    /// (`ch<n>.on`) the number of the instance of a numbered family (`Setting::key_at`)
    pub key: &'static str,
    pub scpi: Scpi,
    pub ty: Ty,
    /// the menu row's label
    pub label: &'static str,
    /// what it does, for docs/scpi.md
    #[cfg(test)]
    pub doc: &'static str,
    /// the SCPI input range of a `U32` when it differs from `min..=max` (a value in it is then kept within those),
    /// of `Seconds` when it has one
    pub accept: Option<(f64, f64)>,
    /// the menu's step when it is not the type's: the value `d` steps from the one in the model (a ladder of the row's
    /// own, a clamp against another field); None takes the type's step
    pub step: Option<fn(&Model, usize, i32) -> Val>,
    /// the value of instance n (0 for a row that is not numbered)
    pub get: fn(&Model, usize) -> Val,
    pub put: fn(&mut Model, usize, Val),
}

/// Every row of every table.
pub fn all() -> impl Iterator<Item = &'static Setting> {
    display::SLICES
        .into_iter()
        .chain(trigger::SLICES)
        .chain(acquisition::SLICES)
        .chain(math::SLICES)
        .chain(meters::DVM_SLICES)
        .chain(meters::COUNTER_SLICES)
        .chain(channel::SLICES)
        .chain(mask::SLICES)
        .chain(histogram::SLICES)
        .chain(bode::SLICES)
        .chain(la::SLICES)
        .chain(spectrum::SLICES)
        .chain(eye::SLICES)
        .chain(power::SLICES)
        .flat_map(|g| g.iter())
}

/// The row of `slices` whose command the nodes below the subsystem name, each in its long or short form.
pub fn command(slices: &[&'static [Setting]], nodes: &[&str]) -> Option<&'static Setting> {
    slices.iter().flat_map(|g| g.iter()).find(|s| match s.scpi {
        Scpi::Cmd(c) => {
            c.split(':').count() == nodes.len()
                && c.split(':').zip(nodes).all(|(word, node)| crate::words::kw(node, word))
        }
        _ => false,
    })
}

/// The row of a key (a setup key, a field of a setup line, or a numbered key as the row spells it: `ch<n>.on`).
pub fn find(key: &str) -> Option<&'static Setting> {
    all().find(|s| s.key == key)
}

/// The `key = value` lines of `rows` of instance n, in their order.
pub fn write(rows: &[Setting], m: &Model, n: usize, kv: &mut dyn FnMut(&str, String)) {
    for s in rows {
        kv(&s.key_at(n), s.store(m, n));
    }
}

impl Setting {
    /// The key of the setup line that holds this setting as one of its fields; None when the key is its own.
    pub fn line(&self) -> Option<&'static str> {
        self.key.split_once(' ').map(|(line, _)| line)
    }

    /// The key of instance n: `ch<n>.on` of index 1 is `ch2.on`; a key that is not numbered stays as it is.
    pub fn key_at(&self, n: usize) -> String {
        self.key.replace("<n>", &(n + 1).to_string())
    }

    /// The value as a setup file writes it.
    pub fn store(&self, m: &Model, n: usize) -> String {
        let v = (self.get)(m, n);
        match &self.ty {
            Ty::Bool | Ty::U32 { .. } | Ty::Steps(_) | Ty::Seconds { .. } | Ty::Hertz { .. } | Ty::Real { .. } => {
                match v {
                    Val::Bool(b) => b.to_string(),
                    Val::F32(x) => x.to_string(),
                    Val::F64(x) => x.to_string(),
                    v => v.index().to_string(),
                }
            }
            Ty::Choice(list, ..) => list[v.index()].store.to_string(),
            Ty::Custom(io) => (io.store)(v),
        }
    }

    /// Reads the value of a setup line; the error names what was wrong, as `store::set` reports it.
    pub fn load(&self, m: &mut Model, n: usize, text: &str) -> Result<(), String> {
        let v = match &self.ty {
            Ty::Bool => Val::Bool(crate::words::p_bool(text)?),
            Ty::U32 { min, max, .. } => Val::U32(crate::words::p_u32(text)?.clamp(*min, *max)),
            Ty::Steps(_) => Val::U32(crate::words::p_u32(text)?.max(1)),
            Ty::Choice(list, unknown, _) => {
                Val::Index(list.iter().position(|c| c.store == text).ok_or_else(|| match unknown {
                    Unknown::Value => format!("unknown value: {}", text),
                    Unknown::Text(what) => format!("{} {}", what, text),
                    Unknown::Names => {
                        format!("{:?}: {}", text, list.iter().map(|c| c.store).collect::<Vec<_>>().join(" or "))
                    }
                })?)
            }
            Ty::Seconds { .. } | Ty::Hertz { .. } => Val::F32(crate::words::p_f32(text)?),
            Ty::Real { range, .. } => {
                let (lo, hi) = range.unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
                match (self.get)(m, n) {
                    Val::F32(_) => Val::F32(crate::words::p_f32(text)?.clamp(lo as f32, hi as f32)),
                    _ => Val::F64(crate::words::p_f64(text)?.clamp(lo, hi)),
                }
            }
            Ty::Custom(io) => (io.load)(text)?,
        };
        (self.put)(m, n, v);
        Ok(())
    }

    /// The answer to the SCPI query.
    pub fn query(&self, m: &Model, n: usize) -> String {
        let v = (self.get)(m, n);
        match &self.ty {
            Ty::Bool => if v.index() != 0 { "1" } else { "0" }.to_string(),
            Ty::U32 { .. } | Ty::Steps(_) => v.index().to_string(),
            Ty::Choice(list, ..) => short(list[v.index()].scpi),
            Ty::Seconds { .. } | Ty::Hertz { .. } => crate::words::e(v.secs() as f64),
            Ty::Real { .. } => crate::words::e(v.real()),
            Ty::Custom(io) => (io.query)(v),
        }
    }

    /// Sets the value an SCPI argument names; false (the model untouched) when it names none.
    pub fn take(&self, m: &mut Model, n: usize, arg: &str) -> bool {
        let v = match &self.ty {
            Ty::Bool => crate::words::parse_bool(arg).map(Val::Bool),
            Ty::U32 { min, max, .. } => {
                let (lo, hi) = self.accept.unwrap_or((*min as f64, *max as f64));
                arg.trim()
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| (lo..=hi).contains(&(n as f64)))
                    .map(|n| Val::U32(n.clamp(*min, *max)))
            }
            Ty::Steps(list) => arg
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|&n| n >= list[0])
                .map(|n| Val::U32(list.iter().rev().copied().find(|&c| c <= n).unwrap_or(list[0]))),
            Ty::Choice(list, _, Arg::Keyword) => {
                list.iter().position(|c| crate::words::kw(arg, c.scpi)).map(Val::Index)
            }
            Ty::Choice(list, _, Arg::Prefix) => {
                let a = arg.to_ascii_uppercase();
                list.iter().position(|c| a.starts_with(&short(c.scpi))).map(Val::Index)
            }
            Ty::Seconds { .. } | Ty::Hertz { .. } => crate::words::num(arg)
                .filter(|&x| self.accept.is_none_or(|(lo, hi)| (lo..=hi).contains(&(x as f64))))
                .map(Val::F32),
            Ty::Real { range, .. } => {
                let (lo, hi) = range.unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
                let within = |x: f64| self.accept.is_none_or(|(a, b)| (a..=b).contains(&x));
                match (self.get)(m, n) {
                    Val::F32(_) => crate::words::num(arg)
                        .filter(|&x| within(x as f64))
                        .map(|x| Val::F32(x.clamp(lo as f32, hi as f32))),
                    _ => arg
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|&x| x.is_finite() && within(x))
                        .map(|x| Val::F64(x.clamp(lo, hi))),
                }
            }
            Ty::Custom(io) => (io.take)(arg),
        };
        v.map(|v| (self.put)(m, n, v)).is_some()
    }

    /// The menu row is a number with - and + (a choice otherwise).
    pub fn is_number(&self) -> bool {
        matches!(self.ty, Ty::U32 { .. } | Ty::Steps(_) | Ty::Seconds { .. } | Ty::Hertz { .. } | Ty::Real { .. })
    }

    /// The menu's options (none for a number but the steps of `Steps`).
    pub fn options(&self) -> Vec<String> {
        let labels: Vec<&str> = match &self.ty {
            Ty::Steps(list) => return list.iter().map(u32::to_string).collect(),
            Ty::Bool => vec!["Off", "On"],
            Ty::U32 { .. } | Ty::Seconds { .. } | Ty::Hertz { .. } | Ty::Real { .. } => Vec::new(),
            Ty::Choice(list, ..) => list.iter().map(|c| c.label).collect(),
            Ty::Custom(io) => io.labels.to_vec(),
        };
        labels.into_iter().map(str::to_string).collect()
    }

    /// The value the menu row shows.
    pub fn shown(&self, m: &Model, n: usize) -> String {
        let v = (self.get)(m, n);
        match &self.ty {
            Ty::U32 { unit: "", .. } => v.index().to_string(),
            Ty::U32 { unit, .. } => format!("{} {}", v.index(), unit),
            Ty::Steps(_) => v.index().to_string(),
            Ty::Seconds { zero: Some(z), .. } if v.secs() <= 0.0 => z.to_string(),
            Ty::Seconds { .. } => si_short(v.secs() as f64, "s"),
            Ty::Hertz { .. } => si_short(v.secs() as f64, "Hz"),
            Ty::Real { shown, .. } => shown(v.real()),
            _ => self.options()[v.index()].clone(),
        }
    }

    /// A step of the menu row by `d`: the row's own step when it has one; else a number by `d` steps within its
    /// range, a list round by `d`, and a switch or a list of two changes on every step.
    pub fn step(&self, m: &mut Model, n: usize, d: i32) {
        if let Some(step) = self.step {
            let v = step(m, n, d);
            return (self.put)(m, n, v);
        }
        let v = (self.get)(m, n);
        let v = match (&self.ty, v) {
            (_, Val::Bool(b)) => Val::Bool(!b),
            (Ty::U32 { min, max, step, .. }, Val::U32(x)) => {
                Val::U32((x as i32 + d * *step as i32).clamp(*min as i32, *max as i32) as u32)
            }
            (Ty::Steps(list), v) => {
                let i = list.iter().position(|&s| s == v.index() as u32);
                let i = i.or_else(|| list.iter().position(|&s| Val::U32(s) == (self.get)(&Model::default(), n)));
                Val::U32(list[step_index(i.unwrap_or(0), list.len(), d)])
            }
            (Ty::Seconds { ladder, .. } | Ty::Hertz { ladder }, v) => Val::F32(ladder(v.secs(), d)),
            // a real number steps by its row's step only
            (Ty::Real { .. }, v) => v,
            (_, v) => {
                let n = self.options().len();
                Val::Index(if n == 2 { 1 - v.index() } else { cycle(v.index(), n, d) })
            }
        };
        (self.put)(m, n, v);
    }

    /// What the SCPI command takes, for its row in docs/scpi.md.
    #[cfg(test)]
    fn takes(&self) -> String {
        match &self.ty {
            Ty::Bool => "`ON`, `OFF`, `1`, `0`; the query answers `1` or `0`".into(),
            Ty::U32 { min, max, unit, .. } => {
                let (lo, hi) = self.accept.unwrap_or((*min as f64, *max as f64));
                if (lo, hi) == (*min as f64, *max as f64) {
                    format!("a whole number from {} to {} {}", lo, hi, unit).trim_end().to_string()
                } else {
                    format!("a whole number from {} to {}, set within {} to {} {}", lo, hi, min, max, unit)
                }
            }
            Ty::Steps(list) => format!(
                "a whole number from {}, set to the step at or below it: {}",
                list[0],
                list.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            ),
            Ty::Choice(list, _, arg) => {
                let words = list.iter().map(|c| format!("`{}`", c.scpi)).collect::<Vec<_>>().join(", ");
                match arg {
                    Arg::Keyword => words,
                    Arg::Prefix => format!("{}, or any word that starts with a short form", words),
                }
            }
            Ty::Seconds { .. } => match self.accept {
                Some((lo, hi)) => format!("seconds from {} to {}", lo, hi),
                None => "seconds".into(),
            },
            Ty::Hertz { .. } => "hertz".into(),
            Ty::Real { .. } => match self.accept {
                Some((lo, hi)) => format!("a number from {} to {}", lo, hi),
                None => "a number".into(),
            },
            Ty::Custom(io) => io.values.into(),
        }
    }
}

/// The short form of a keyword, as a query answers it (`NEGative`: `NEG`).
fn short(word: &str) -> String {
    word.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect()
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
        let key = match r.key.split_once(' ') {
            Some((line, field)) => format!("`{}` ({})", line, field),
            None => format!("`{}`", r.key),
        };
        s.push_str(&format!("| {} | {} | {} | {} | {} |\n", cmd, takes, key, r.label, r.doc));
    }
    s
}

#[cfg(test)]
mod tests;
