// SPDX-License-Identifier: GPL-2.0-or-later
//! SCPI coverage extensions: the menu options that had no SCPI command of their own. Every header here calls the same
//! model fields and `Ui` functions as the menu `Item` it stands for (`activate`, `select_option`, the handlers of the
//! Studio chips), so the screen follows and nothing is done twice. The interpreter (`scpi.rs`) hands every command to
//! `Ui::scpi_ext_command` first; a header that is not in the tables below goes on to the older handlers.
//!
//! The headers are listed as tables (`T1`, `T2`, `T3` in the files of the tiers): a keyword with `<n>` takes the
//! numeric suffix (`CHANnel<n>`), long and short forms match like everywhere in `scpi.rs`.
//! This file: the dispatcher, the helpers, the trigger types, the history, the search.

use super::*;
use crate::scpi::{e as en, kw, num, parse_bool, parse_wave, suffix, wave_tok};

pub(super) type Reply = Result<Option<Vec<u8>>, String>;
pub(super) const DONE: Reply = Ok(None);

pub(super) fn bad(arg: &str) -> String { format!("-224,\"Illegal parameter value\" {}", arg) }
pub(super) fn conflict(why: &str) -> String { format!("-221,\"Settings conflict\" {}", why) }
pub(super) fn undefined(key: &str) -> String { format!("-113,\"Undefined header\" {}", key) }
pub(super) fn t(s: impl AsRef<str>) -> Reply { Ok(Some(s.as_ref().as_bytes().to_vec())) }
pub(super) fn b01(v: bool) -> &'static str { if v { "1" } else { "0" } }

/// The short form of a SCPI keyword: its upper-case letters and digits ("POSitive" -> "POS").
pub(super) fn short(long: &str) -> String {
    long.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect()
}

/// A choice by keyword: the query answers the short form, the set takes long or short form.
pub(super) fn pick<T: Copy + PartialEq>(q: bool, arg: &str, cur: &mut T, table: &[(T, &str)]) -> Reply {
    if q {
        return t(table.iter().find(|(v, _)| v == cur).map(|(_, n)| short(n)).unwrap_or_default());
    }
    match table.iter().find(|(_, n)| kw(arg.trim().trim_matches('"'), n)) {
        Some((v, _)) => { *cur = *v; DONE }
        None => Err(bad(arg)),
    }
}

impl Ui {
    /// A trigger level (`TrigLevel` or `TrigLevel2`) by SCPI: the query answers the level, a number sets it limited to
    /// the range of the table, like the stepper and the number pad (a level beyond the screen is not an error).
    pub(super) fn scpi_level(&mut self, q: bool, arg: &str, it: Item) -> Reply {
        if q { return t(en(self.range_get(it).unwrap_or(0.0))); }
        match num(arg).filter(|v| v.is_finite()) {
            Some(v) => { self.scpi_range_set(it, v as f64); DONE }
            None => Err(bad(arg)),
        }
    }
}

pub(super) fn numf(q: bool, arg: &str, cur: &mut f32, lo: f32, hi: f32) -> Reply {
    if q { return t(en(*cur as f64)); }
    match num(arg).filter(|v| *v >= lo && *v <= hi) {
        Some(v) => { *cur = v; DONE }
        None => Err(bad(arg)),
    }
}

pub(super) fn numd(q: bool, arg: &str, cur: &mut f64, lo: f64, hi: f64) -> Reply {
    if q { return t(en(*cur)); }
    match arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= lo && *v <= hi) {
        Some(v) => { *cur = v; DONE }
        None => Err(bad(arg)),
    }
}

pub(super) fn intf(q: bool, arg: &str, cur: &mut u32, lo: u32, hi: u32) -> Reply {
    if q { return t(cur.to_string()); }
    match arg.trim().parse::<f64>().ok().filter(|v| v.fract() == 0.0 && *v >= lo as f64 && *v <= hi as f64) {
        Some(v) => { *cur = v as u32; DONE }
        None => Err(bad(arg)),
    }
}

pub(super) fn boolf(q: bool, arg: &str, cur: &mut bool) -> Reply {
    if q { return t(b01(*cur)); }
    match parse_bool(arg) {
        Some(v) => { *cur = v; DONE }
        None => Err(bad(arg)),
    }
}

/// A quoted string argument without its quotes.
pub(super) fn text_arg(arg: &str) -> String {
    let a = arg.trim();
    a.strip_prefix('"').and_then(|r| r.strip_suffix('"')).or_else(|| a.strip_prefix('\'').and_then(|r| r.strip_suffix('\''))).unwrap_or(a).to_string()
}

/// A number with an optional 0x / #H prefix.
pub(super) fn parse_int(arg: &str) -> Option<u64> {
    let a = arg.trim();
    if let Some(h) = a.strip_prefix("0x").or_else(|| a.strip_prefix("0X")).or_else(|| a.strip_prefix("#H")).or_else(|| a.strip_prefix("#h")) {
        return u64::from_str_radix(h, 16).ok();
    }
    a.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0 && v.fract() == 0.0).map(|v| v as u64)
}

const SLOPES: [(Slope, &str); 3] = [(Slope::Rise, "POSitive"), (Slope::Fall, "NEGative"), (Slope::Either, "RFALl")];
const POLS: [(Pol, &str); 2] = [(Pol::Pos, "POSitive"), (Pol::Neg, "NEGative")];
const WHENS: [(WhenKind, &str); 4] = [(WhenKind::Gt, "GREater"), (WhenKind::Lt, "LESS"), (WhenKind::In, "RANGe"), (WhenKind::Out, "OUTside")];
const WHENS_OFF: [(WhenKind, &str); 5] = [(WhenKind::Off, "NONE"), (WhenKind::Gt, "GREater"), (WhenKind::Lt, "LESS"), (WhenKind::In, "RANGe"), (WhenKind::Out, "OUTside")];

/// All headers of the extensions, by tier. A header that is not here is not answered by this file.
pub(super) fn all_headers() -> impl Iterator<Item = &'static str> {
    [T1, super::scpiext_a::T1A, super::scpiext_b::T2, super::scpiext_b::T2B, super::scpiext_c::T3, super::scpiext_c::T3B].into_iter().flatten().copied().chain(super::scpiext_b::SETUP.iter().map(|(h, _)| *h))
}

/// Matches the keywords of a command with a header of the tables; the suffix of the `<n>` keyword comes back.
pub(super) fn resolve(nodes: &[&str]) -> Option<(&'static str, Option<usize>)> {
    all_headers().find_map(|h| {
        let parts: Vec<&str> = h.split(':').collect();
        if parts.len() != nodes.len() { return None; }
        let mut idx = None;
        for (p, n) in parts.iter().zip(nodes) {
            if let Some(base) = p.strip_suffix("<n>") {
                let (k, s) = suffix(n);
                if !kw(k, base) { return None; }
                idx = Some(s?);
            } else if p.starts_with('*') {
                if !n.eq_ignore_ascii_case(p) { return None; }
            } else if !kw(n, p) || (h == "SYSTem:SETup" && *p == "SETup" && !n.eq_ignore_ascii_case("SETup")) {
                // :SYSTem:SET is the older key = value command: the short form of SETup stays with it
                return None;
            }
        }
        Some((h, idx))
    })
}

pub(super) const T1: &[&str] = &[
    // trigger types
    "TRIGger:PULSe:POLarity", "TRIGger:PULSe:WHEN", "TRIGger:PULSe:WIDTh", "TRIGger:PULSe:UWIDth", "TRIGger:PULSe:LWIDth", "TRIGger:PULSe:LEVel",
    "TRIGger:SLOPe:SLOPe", "TRIGger:SLOPe:WHEN", "TRIGger:SLOPe:TIME", "TRIGger:SLOPe:TUPPer", "TRIGger:SLOPe:TLOWer", "TRIGger:SLOPe:ALEVel", "TRIGger:SLOPe:BLEVel",
    "TRIGger:RUNT:POLarity", "TRIGger:RUNT:WHEN", "TRIGger:RUNT:WLOWer", "TRIGger:RUNT:WUPPer",
    "TRIGger:VIDeo:STANdard", "TRIGger:VIDeo:MODE", "TRIGger:VIDeo:LINE", "TRIGger:VIDeo:LEVel",
    "TRIGger:PATTern:PATTern", "TRIGger:PATTern:LEVel", "TRIGger:DURation:PATTern", "TRIGger:DURation:LEVel",
    "TRIGger:DURation:WHEN", "TRIGger:DURation:TLOWer", "TRIGger:DURation:TUPPer",
    "TRIGger:WINDow:SLOPe", "TRIGger:WINDow:POSition", "TRIGger:WINDow:ALEVel", "TRIGger:WINDow:BLEVel", "TRIGger:WINDow:WHEN", "TRIGger:WINDow:TIME",
    "TRIGger:NEDGe:SLOPe", "TRIGger:NEDGe:IDLE", "TRIGger:NEDGe:EDGE", "TRIGger:NEDGe:LEVel",
    "TRIGger:SHOLd:STIMe", "TRIGger:SHOLd:HTIMe", "TRIGger:SHOLd:CSOurce", "TRIGger:SHOLd:DSOurce", "TRIGger:SHOLd:SLOPe",
    "TRIGger:TIMeout:SLOPe", "TRIGger:TIMeout:TIME", "TRIGger:TIMeout:LEVel",
    "TRIGger:DELay:SLOPA", "TRIGger:DELay:SLOPB", "TRIGger:DELay:WHEN", "TRIGger:DELay:TUPPer", "TRIGger:DELay:TLOWer", "TRIGger:DELay:ALEVel", "TRIGger:DELay:BLEVel",
    "TRIGger:BUS:WHEN", "TRIGger:BUS:VALue",
    "TRIGger:LEVel", "TRIGger:LEVel:AUTO", "TRIGger:LEVel2",
    // acquisition history
    "ACQuire:HISTory:ENABle", "ACQuire:HISTory:FRAMe", "ACQuire:HISTory:COUNt", "ACQuire:HISTory:NEXT", "ACQuire:HISTory:PREVious",
    // search (mark events)
    "SEARch:STATe", "SEARch:MODE", "SEARch:EDGE:SOURce", "SEARch:EDGE:SLOPe", "SEARch:EDGE:LEVel", "SEARch:EDGE:LEVel:AUTO",
    "SEARch:COPY:TO", "SEARch:COPY:FROM", "SEARch:COUNt", "SEARch:EVENt", "SEARch:DATA", "SEARch:SAVE", "SEARch:REMove",
];

impl Ui {
    /// The entry of `scpi.rs`: None = not a header of the extensions.
    pub(crate) fn scpi_ext_command(&mut self, nodes: &[&str], query: bool, arg: &str) -> Option<Reply> {
        // the layout of the screen and its controls (scpi_layout.rs)
        if let Some(r) = self.scpi_layout(nodes, query, arg) { return Some(r); }
        let (key, idx) = resolve(nodes)?;
        let r = self.ext_trigger(key, query, arg)
            .or_else(|| self.ext_history_search(key, query, arg))
            .or_else(|| self.ext_a(key, idx, query, arg))
            .or_else(|| self.ext_b(key, idx, query, arg))
            .or_else(|| self.ext_c(key, idx, query, arg))
            .unwrap_or_else(|| Err(undefined(key)));
        Some(r)
    }

    // ------------------------------------------------------------------------------------------ trigger types

    fn ext_trigger(&mut self, key: &str, q: bool, arg: &str) -> Option<Reply> {
        if !key.starts_with("TRIGger:") { return None; }
        let r = self.ext_trigger_run(key, q, arg)?;
        if !q && matches!(r, Ok(None)) { self.mark_changed(); }
        Some(r)
    }

    fn ext_trigger_run(&mut self, key: &str, q: bool, arg: &str) -> Option<Reply> {
        let tm = |m: &mut Model, lower: bool, q: bool, arg: &str| numf(q, arg, if lower { &mut m.trig_t1 } else { &mut m.trig_t2 }, 1e-9, 10.0);
        Some(match key {
            // pulse width: WIDTh and LWIDth are the time (lower limit), UWIDth the upper limit
            "TRIGger:PULSe:POLarity" | "TRIGger:RUNT:POLarity" => pick(q, arg, &mut self.m.trig_pol, &POLS),
            "TRIGger:PULSe:WHEN" => self.pulse_when(q, arg),
            "TRIGger:PULSe:WIDTh" | "TRIGger:PULSe:LWIDth" | "TRIGger:SLOPe:TIME" | "TRIGger:SLOPe:TLOWer" | "TRIGger:RUNT:WLOWer"
            | "TRIGger:DURation:TLOWer" | "TRIGger:DELay:TLOWer" | "TRIGger:TIMeout:TIME" | "TRIGger:WINDow:TIME" => tm(&mut self.m, true, q, arg),
            "TRIGger:PULSe:UWIDth" | "TRIGger:SLOPe:TUPPer" | "TRIGger:RUNT:WUPPer" | "TRIGger:DURation:TUPPer" | "TRIGger:DELay:TUPPer" => tm(&mut self.m, false, q, arg),
            "TRIGger:SLOPe:WHEN" | "TRIGger:DURation:WHEN" | "TRIGger:DELay:WHEN" | "TRIGger:WINDow:WHEN" => pick(q, arg, &mut self.m.trig_when, &WHENS),
            "TRIGger:RUNT:WHEN" => pick(q, arg, &mut self.m.trig_when, &WHENS_OFF),
            "TRIGger:SLOPe:SLOPe" | "TRIGger:WINDow:SLOPe" | "TRIGger:NEDGe:SLOPe" | "TRIGger:TIMeout:SLOPe" | "TRIGger:DELay:SLOPA" => pick(q, arg, &mut self.m.slope, &SLOPES),
            "TRIGger:SHOLd:SLOPe" | "TRIGger:DELay:SLOPB" => pick(q, arg, &mut self.m.trig_slope2, &SLOPES),
            "TRIGger:SLOPe:ALEVel" | "TRIGger:WINDow:ALEVel" | "TRIGger:DELay:ALEVel" | "TRIGger:PULSe:LEVel" | "TRIGger:VIDeo:LEVel"
            | "TRIGger:NEDGe:LEVel" | "TRIGger:TIMeout:LEVel" | "TRIGger:LEVel" => self.scpi_level(q, arg, Item::TrigLevel),
            "TRIGger:SLOPe:BLEVel" | "TRIGger:WINDow:BLEVel" | "TRIGger:DELay:BLEVel" | "TRIGger:LEVel2" => self.scpi_level(q, arg, Item::TrigLevel2),
            "TRIGger:VIDeo:STANdard" => pick(q, arg, &mut self.m.video_std, &[(VideoStd::Pal, "PAL"), (VideoStd::Ntsc, "NTSC")]),
            "TRIGger:VIDeo:MODE" => pick(q, arg, &mut self.m.video_sync, &[(VSync::AllLines, "ALINes"), (VSync::Line, "LINE"), (VSync::Odd, "ODDField"), (VSync::Even, "EVENfield"), (VSync::AllFields, "AFIelds")]),
            "TRIGger:VIDeo:LINE" => {
                let top = (self.m.video_std.lines() as u32 / 2).saturating_sub(10).max(1);
                intf(q, arg, &mut self.m.video_line, 1, top)
            }
            "TRIGger:PATTern:PATTern" | "TRIGger:DURation:PATTern" => self.pattern_cmd(q, arg),
            "TRIGger:PATTern:LEVel" | "TRIGger:DURation:LEVel" => self.pattern_level(q, arg),
            "TRIGger:WINDow:POSition" => {
                let r = pick(q, arg, &mut self.m.win_mode, &[(WinMode::Enter, "ENTer"), (WinMode::Exit, "EXIT"), (WinMode::Inside, "TIMe")]);
                if r.is_err() && !q && arg.trim().to_ascii_uppercase().starts_with("INS") { self.m.win_mode = WinMode::Inside; DONE } else { r }
            }
            "TRIGger:NEDGe:IDLE" => numf(q, arg, &mut self.m.nth_idle, 1e-9, 10.0),
            "TRIGger:NEDGe:EDGE" => intf(q, arg, &mut self.m.nth_n, 1, 65535),
            "TRIGger:SHOLd:STIMe" => numf(q, arg, &mut self.m.setup, 1e-9, 10.0),
            "TRIGger:SHOLd:HTIMe" => numf(q, arg, &mut self.m.hold, 1e-9, 10.0),
            "TRIGger:SHOLd:DSOurce" | "TRIGger:SHOLd:CSOurce" => {
                let clock = key.ends_with("CSOurce");
                if q { return Some(t(wave_tok(if clock { self.m.trigger_wave2() } else { self.m.trigger_wave() }))); }
                match parse_wave(arg) {
                    Some(id) if self.set_trigger_source(id, clock) => DONE,
                    _ => Err(bad(arg)),
                }
            }
            "TRIGger:BUS:WHEN" => pick(q, arg, &mut self.m.trig_bus, &[(BusWhat::Frame, "FRAMe"), (BusWhat::Id, "ID"), (BusWhat::Data, "DATA"), (BusWhat::Error, "ERRor")]),
            "TRIGger:BUS:VALue" => {
                if q { return Some(t(format!("0x{:X}", self.m.trig_bus_val))); }
                match parse_int(arg).filter(|&v| v <= 0x1fff_ffff) {
                    Some(v) => { self.m.trig_bus_val = v as u32; DONE }
                    None => Err(bad(arg)),
                }
            }
            // Level 50 %: the Trig50 action of the trigger menu and of the Studio level tag
            "TRIGger:LEVel:AUTO" => { if q { return Some(Err(undefined(key))); } self.activate(Item::Trig50); DONE }
            _ => return None,
        })
    }

    /// Pulse trigger "When": GREater, LESS, RANGe, OUTside, or the stock names PGReater, PLESs, PGLess, NGReater, NLESs, NGLess
    /// (polarity and condition together).
    fn pulse_when(&mut self, q: bool, arg: &str) -> Reply {
        if !q {
            let a = arg.trim().to_ascii_uppercase();
            let alias = match a.as_str() {
                "PGR" | "PGREATER" => Some((Pol::Pos, WhenKind::Gt)),
                "PLES" | "PLESS" => Some((Pol::Pos, WhenKind::Lt)),
                "PGL" | "PGLESS" => Some((Pol::Pos, WhenKind::In)),
                "NGR" | "NGREATER" => Some((Pol::Neg, WhenKind::Gt)),
                "NLES" | "NLESS" => Some((Pol::Neg, WhenKind::Lt)),
                "NGL" | "NGLESS" => Some((Pol::Neg, WhenKind::In)),
                _ => None,
            };
            if let Some((p, w)) = alias { (self.m.trig_pol, self.m.trig_when) = (p, w); return DONE; }
        }
        pick(q, arg, &mut self.m.trig_when, &WHENS)
    }

    /// `:TRIGger:PATTern:PATTern "HLXRF"`: one character per channel (H, L, X, R rising, F falling), then optionally the
    /// 16 logic lines D0..D15 (H, L, X).
    fn pattern_cmd(&mut self, q: bool, arg: &str) -> Reply {
        let nc = n_ch();
        let ch = |b: Bit| match b { Bit::H => 'H', Bit::L => 'L', Bit::X => 'X', Bit::R => 'R', Bit::F => 'F' };
        if q {
            let mut s: String = self.m.pat[..nc].iter().map(|&b| ch(b)).collect();
            if self.m.la_on() { s.extend(self.m.pat_la.iter().map(|&b| ch(b))); }
            return t(format!("\"{}\"", s));
        }
        let s = text_arg(arg).to_ascii_uppercase();
        let chars: Vec<char> = s.chars().collect();
        if chars.len() != nc && chars.len() != nc + 16 { return Err(bad(arg)); }
        let bit = |c: char, edges: bool| match c { 'H' | '1' => Some(Bit::H), 'L' | '0' => Some(Bit::L), 'X' => Some(Bit::X), 'R' if edges => Some(Bit::R), 'F' if edges => Some(Bit::F), _ => None };
        let mut pat = self.m.pat;
        let mut pat_la = self.m.pat_la;
        for (i, &c) in chars.iter().enumerate() {
            if i < nc { pat[i] = bit(c, true).ok_or_else(|| bad(arg))?; } else { pat_la[i - nc] = bit(c, false).ok_or_else(|| bad(arg))?; }
        }
        (self.m.pat, self.m.pat_la) = (pat, pat_la);
        DONE
    }

    /// `:TRIGger:PATTern:LEVel <ch>,<volts>` / `:TRIGger:PATTern:LEVel? <ch>`.
    fn pattern_level(&mut self, q: bool, arg: &str) -> Reply {
        let mut it = arg.split(',');
        let c = it.next().and_then(|a| crate::scpi::parse_ch(a).or_else(|| a.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(1)))).filter(|&c| c < n_ch()).ok_or_else(|| bad(arg));
        if q { return t(en(self.m.pat_levels[c?] as f64)); }
        let v = it.next().and_then(num).ok_or_else(|| bad(arg));
        self.m.pat_levels[c?] = v?;
        DONE
    }

    // ------------------------------------------------------------------------------------------ history and search

    fn ext_history_search(&mut self, key: &str, q: bool, arg: &str) -> Option<Reply> {
        Some(match key {
            "ACQuire:HISTory:ENABle" => {
                if q { return Some(t(b01(self.hist_pos.is_some()))); }
                match parse_bool(arg) {
                    Some(true) if self.hist_pos.is_none() => { if self.history.is_empty() { return Some(Err(conflict("history empty"))); } self.history_step(0); DONE }
                    Some(false) if self.hist_pos.is_some() => { self.key(key::NAVIGATE); DONE }
                    Some(_) => DONE,
                    None => Err(bad(arg)),
                }
            }
            "ACQuire:HISTory:COUNt" => if q { t(self.history.len().to_string()) } else { Err(undefined(key)) },
            "ACQuire:HISTory:FRAMe" => {
                if q { return Some(t(self.hist_pos.map_or(0, |i| i + 1).to_string())); }
                let n = self.history.len();
                if n == 0 { return Some(Err(conflict("history empty"))); }
                let Some(v) = arg.trim().parse::<usize>().ok().filter(|v| (1..=n).contains(v)) else { return Some(Err(bad(arg))) };
                if self.hist_pos.is_none() { self.history_step(0); }
                let cur = self.hist_pos.unwrap_or(n - 1) as i32;
                if cur != v as i32 - 1 { self.history_step(v as i32 - 1 - cur); }
                DONE
            }
            // the NAVBack / NAVFwd keys of the Display menu
            "ACQuire:HISTory:NEXT" | "ACQuire:HISTory:PREVious" => {
                if q { return Some(Err(undefined(key))); }
                if self.history.is_empty() { return Some(Err(conflict("history empty"))); }
                self.key(if key.ends_with("NEXT") { 0x06 } else { 0x08 });
                DONE
            }
            _ if key.starts_with("SEARch:") => return Some(self.search_cmd(key, q, arg)),
            _ => return None,
        })
    }

    /// The Studio search of the Edge kind (source, level, slope) and the mark of the trigger events. Without a search of its
    /// own the values are the trigger's; the first set of a source, level or slope adds Search 1 like the Add rail does.
    fn search_cmd(&mut self, key: &str, q: bool, arg: &str) -> Reply {
        const SL: [(u8, &str); 3] = [(0, "POSitive"), (1, "NEGative"), (2, "RFALl")];
        match key {
            "SEARch:STATe" => {
                if q { return t(b01(self.search_on)); }
                match parse_bool(arg) { Some(v) => { self.search_on = v; DONE } None => Err(bad(arg)) }
            }
            "SEARch:MODE" => {
                if q { return t("EDGE"); }
                if kw(arg.trim(), "EDGE") { DONE } else { Err(bad(arg)) }
            }
            "SEARch:EDGE:SOURce" => {
                if q {
                    return t(wave_tok(self.studio.search.map_or_else(|| self.m.trigger_wave(), |s| s.src)));
                }
                let Some(id) = parse_wave(arg) else { return Err(bad(arg)) };
                match self.studio.search {
                    None => {
                        self.studio_search_add(Some(id));
                        if self.studio.search.is_none() { return Err(conflict("no source is on")); }
                    }
                    Some(sp) => {
                        self.studio.search = Some(super::addrail::SearchSpec { src: id, ..sp });
                        self.studio_search_set_50();
                    }
                }
                DONE
            }
            "SEARch:EDGE:SLOPe" => {
                let mut cur = self.studio.search.map_or(match self.m.slope { Slope::Rise => 0, Slope::Fall => 1, Slope::Either => 2 }, |s| s.slope);
                if q { return pick(true, arg, &mut cur, &SL); }
                pick(false, arg, &mut cur, &SL)?;
                self.search_spec_mut(|sp| sp.slope = cur)
            }
            "SEARch:EDGE:LEVel" => {
                if q { return t(en(self.studio.search.map_or(self.m.trig_level, |s| s.level) as f64)); }
                let Some(v) = num(arg) else { return Err(bad(arg)) };
                self.search_spec_mut(|sp| sp.level = v)
            }
            "SEARch:EDGE:LEVel:AUTO" => {
                if q { return Err(undefined(key)); }
                self.ensure_search()?;
                self.activate(Item::Srch50);
                DONE
            }
            "SEARch:COPY:TO" | "SEARch:COPY:FROM" => {
                if q { return Err(undefined(key)); }
                if !kw(arg.trim(), "TRIGger") { return Err(bad(arg)); }
                self.ensure_search()?;
                self.activate(if key.ends_with("TO") { Item::SrchToTrig } else { Item::SrchFromTrig });
                DONE
            }
            "SEARch:COUNt" => if q { t(self.search_events().len().to_string()) } else { Err(undefined(key)) },
            "SEARch:EVENt" => {
                let n = self.search_events().len();
                if q { return t(if n == 0 { 0 } else { self.studio.search_at.min(n - 1) + 1 }.to_string()); }
                let Some(v) = arg.trim().parse::<usize>().ok().filter(|v| (1..=n).contains(v)) else { return Err(bad(arg)) };
                let at = self.search_events()[v - 1];
                self.studio.search_at = v - 1;
                self.go_to_time(at);
                DONE
            }
            "SEARch:DATA" => {
                if !q { return Err(undefined(key)); }
                t(self.table_rows(tables::TableKind::Search).iter().map(|r| format!("{:e},{},{},{}", r.t, r.who, r.text, if r.ok { "ok" } else { "error" })).collect::<Vec<_>>().join("\n"))
            }
            "SEARch:SAVE" => { if q { return Err(undefined(key)); } self.activate(Item::SearchSave); DONE }
            "SEARch:REMove" => { if q { return Err(undefined(key)); } self.activate(Item::SrchRemove); DONE }
            _ => Err(undefined(key)),
        }
    }

    /// Search 1 exists (added on the trigger source when it did not).
    fn ensure_search(&mut self) -> Result<(), String> {
        if self.studio.search.is_none() {
            let src = match self.m.trigger_wave() { w @ WaveId::Ch(_) => w, _ => WaveId::Ch(self.m.trig_src.min(n_ch() - 1)) };
            self.studio_search_add(Some(src));
            if self.studio.search.is_none() { return Err(conflict("no source is on")); }
        }
        Ok(())
    }

    fn search_spec_mut(&mut self, f: impl FnOnce(&mut super::addrail::SearchSpec)) -> Reply {
        self.ensure_search()?;
        if let Some(mut sp) = self.studio.search { f(&mut sp); self.studio.search = Some(sp); }
        DONE
    }
}
