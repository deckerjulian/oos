// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Software triggers. Every trigger works on the comparator edges of `crate::edges` and returns trigger points as
//! fractional sample indices (ascending). The trigger point is where the condition is decided: the closing edge of a
//! pulse, the end of a transition, the moment a timeout expires, the edge that completes a pattern, ...

use crate::edges::{edges, Edge};
use crate::sim::VideoStd;

/// Time condition (seconds), as in the scope's "When" setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    Gt(f64),
    Lt(f64),
    In(f64, f64),
    Out(f64, f64),
}

impl When {
    pub fn ok(&self, x: f64) -> bool {
        match *self {
            When::Gt(a) => x > a,
            When::Lt(a) => x < a,
            When::In(a, b) => x > a && x < b,
            When::Out(a, b) => x < a || x > b,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slope {
    Rising,
    Falling,
    Either,
}

impl Slope {
    fn matches(self, rising: bool) -> bool {
        match self {
            Slope::Rising => rising,
            Slope::Falling => !rising,
            Slope::Either => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pol {
    Pos,
    Neg,
}

/// Pattern bit per channel: high, low, don't care, rising edge, falling edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bit {
    H,
    L,
    X,
    R,
    F,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoSync {
    AllLines,
    /// n-th normal line of any field (1-based)
    Line(u32),
    OddField,
    EvenField,
    AllFields,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WinWhen {
    Enter,
    Exit,
    /// stays inside the window for a time satisfying the condition (trigger on exit)
    Inside(When),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trigger {
    Edge { src: usize, slope: Slope, level: f32 },
    /// pulse width (between the two crossings of `level`) satisfies `when`; trigger on the closing edge
    Pulse { src: usize, pol: Pol, level: f32, when: When },
    /// transition time between `lo` and `hi` satisfies `when`; trigger at the end of the transition
    Slope { src: usize, slope: Slope, lo: f32, hi: f32, when: When },
    /// composite video with negative sync, `level` between sync tip and blanking
    Video { src: usize, std: VideoStd, sync: VideoSync, level: f32 },
    /// trigger when the pattern becomes true (or on its edge bit while the levels match)
    Pattern { bits: [Bit; 4], levels: [f32; 4] },
    /// the pattern (H/L/X) holds for a time satisfying `when`; trigger when it ends
    Duration { bits: [Bit; 4], levels: [f32; 4], when: When },
    /// no edge of `slope` for `time` after an edge; trigger when the time has elapsed
    Timeout { src: usize, slope: Slope, level: f32, time: f64 },
    /// pulse that crosses one threshold but not the other before returning; optional width condition
    Runt { src: usize, pol: Pol, lo: f32, hi: f32, when: Option<When> },
    Window { src: usize, lo: f32, hi: f32, when: WinWhen },
    /// time from an edge on A to the next edge on B satisfies `when`; trigger on the B edge
    Delay { a: usize, slope_a: Slope, level_a: f32, b: usize, slope_b: Slope, level_b: f32, when: When },
    /// data changes less than `setup` before or `hold` after a clock edge
    SetupHold { data: usize, clk: usize, clk_slope: Slope, data_level: f32, clk_level: f32, setup: f64, hold: f64 },
    /// the n-th edge after an idle time of at least `idle`
    NthEdge { src: usize, slope: Slope, level: f32, idle: f64, n: u32 },
}

/// Trigger input: up to 4 channels (empty slice = off), sample period, comparator hysteresis per channel (volts).
pub struct Input<'a> {
    pub ch: [&'a [f32]; 4],
    pub dt: f64,
    pub hyst: [f32; 4],
}

impl<'a> Input<'a> {
    fn edges(&self, c: usize, level: f32) -> Vec<Edge> {
        edges(self.ch[c], level, self.hyst[c])
    }
}

/// All trigger points in the record.
pub fn find_all(trig: &Trigger, inp: &Input) -> Vec<f64> {
    let dt = inp.dt;
    let mut out = Vec::new();
    match *trig {
        Trigger::Edge { src, slope, level } => {
            out = inp.edges(src, level).iter().filter(|e| slope.matches(e.rising)).map(|e| e.t).collect();
        }
        Trigger::Pulse { src, pol, level, when } => {
            let open = pol == Pol::Pos;
            let mut start = None;
            for e in inp.edges(src, level) {
                if e.rising == open {
                    start = Some(e.t);
                } else if let Some(s) = start.take() {
                    if when.ok((e.t - s) * dt) {
                        out.push(e.t);
                    }
                }
            }
        }
        Trigger::Slope { src, slope, lo, hi, when } => {
            let mut ev: Vec<(f64, bool, bool)> = Vec::new(); // (t, is_hi, rising)
            ev.extend(inp.edges(src, lo).iter().map(|e| (e.t, false, e.rising)));
            ev.extend(inp.edges(src, hi).iter().map(|e| (e.t, true, e.rising)));
            ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let (mut up, mut down) = (None, None);
            for (t, is_hi, rising) in ev {
                match (is_hi, rising) {
                    (false, true) => up = Some(t),
                    (false, false) => {
                        up = None;
                        if let Some(s) = down.take() {
                            if slope.matches(false) && when.ok((t - s) * dt) {
                                out.push(t);
                            }
                        }
                    }
                    (true, false) => down = Some(t),
                    (true, true) => {
                        down = None;
                        if let Some(s) = up.take() {
                            if slope.matches(true) && when.ok((t - s) * dt) {
                                out.push(t);
                            }
                        }
                    }
                }
            }
        }
        Trigger::Runt { src, pol, lo, hi, when } => {
            let mut ev: Vec<(f64, bool, bool)> = Vec::new();
            ev.extend(inp.edges(src, lo).iter().map(|e| (e.t, false, e.rising)));
            ev.extend(inp.edges(src, hi).iter().map(|e| (e.t, true, e.rising)));
            ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            // positive runt: rises through lo, never reaches hi, falls back through lo (negative: mirrored)
            let (open_hi, start_rising) = if pol == Pol::Pos { (false, true) } else { (true, false) };
            let mut start: Option<f64> = None;
            let mut spoiled = false;
            for (t, is_hi, rising) in ev {
                if is_hi == open_hi {
                    if rising == start_rising {
                        start = Some(t);
                        spoiled = false;
                    } else if let Some(s) = start.take() {
                        if !spoiled && when.map_or(true, |w| w.ok((t - s) * dt)) {
                            out.push(t);
                        }
                    }
                } else if start.is_some() && rising == start_rising {
                    spoiled = true; // reached the other threshold: a full pulse
                }
            }
        }
        Trigger::Window { src, lo, hi, when } => {
            let mut ev: Vec<(f64, bool, bool)> = Vec::new();
            ev.extend(inp.edges(src, lo).iter().map(|e| (e.t, false, e.rising)));
            ev.extend(inp.edges(src, hi).iter().map(|e| (e.t, true, e.rising)));
            ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let mut entered: Option<f64> = None;
            for (t, is_hi, rising) in ev {
                // entering: up through lo or down through hi; leaving: the opposite
                let enter = (!is_hi && rising) || (is_hi && !rising);
                if enter {
                    entered = Some(t);
                    if when == WinWhen::Enter {
                        out.push(t);
                    }
                } else {
                    match when {
                        WinWhen::Exit => out.push(t),
                        WinWhen::Inside(w) => {
                            if let Some(s) = entered {
                                if w.ok((t - s) * dt) {
                                    out.push(t);
                                }
                            }
                        }
                        WinWhen::Enter => {}
                    }
                    entered = None;
                }
            }
        }
        Trigger::Timeout { src, slope, level, time } => {
            let e: Vec<f64> = inp.edges(src, level).iter().filter(|e| slope.matches(e.rising)).map(|e| e.t).collect();
            let n = inp.ch[src].len() as f64;
            let lim = time / dt;
            for (i, &t) in e.iter().enumerate() {
                let next = e.get(i + 1).copied().unwrap_or(f64::INFINITY);
                if next - t > lim && t + lim < n {
                    out.push(t + lim);
                }
            }
        }
        Trigger::Delay { a, slope_a, level_a, b, slope_b, level_b, when } => {
            let ea: Vec<f64> = inp.edges(a, level_a).iter().filter(|e| slope_a.matches(e.rising)).map(|e| e.t).collect();
            let eb: Vec<f64> = inp.edges(b, level_b).iter().filter(|e| slope_b.matches(e.rising)).map(|e| e.t).collect();
            let mut j = 0;
            for &ta in &ea {
                while j < eb.len() && eb[j] <= ta {
                    j += 1;
                }
                if let Some(&tb) = eb.get(j) {
                    if when.ok((tb - ta) * dt) {
                        out.push(tb);
                    }
                }
            }
        }
        Trigger::SetupHold { data, clk, clk_slope, data_level, clk_level, setup, hold } => {
            let ec: Vec<f64> = inp.edges(clk, clk_level).iter().filter(|e| clk_slope.matches(e.rising)).map(|e| e.t).collect();
            let ed: Vec<f64> = inp.edges(data, data_level).iter().map(|e| e.t).collect();
            let mut j = 0;
            for &tc in &ec {
                while j < ed.len() && ed[j] <= tc {
                    j += 1;
                }
                let before = if j > 0 { Some(ed[j - 1]) } else { None };
                let after = ed.get(j).copied();
                if before.map_or(false, |td| (tc - td) * dt < setup) {
                    out.push(tc);
                } else if let Some(td) = after {
                    if (td - tc) * dt < hold {
                        out.push(td);
                    }
                }
            }
        }
        Trigger::NthEdge { src, slope, level, idle, n } => {
            let e: Vec<f64> = inp.edges(src, level).iter().filter(|e| slope.matches(e.rising)).map(|e| e.t).collect();
            let lim = idle / dt;
            let mut count = 0u32;
            let mut prev: Option<f64> = None;
            for &t in &e {
                let gap = prev.map_or(t, |p| t - p);
                count = if gap >= lim { 1 } else if count > 0 { count + 1 } else { 0 };
                if count == n {
                    out.push(t);
                }
                prev = Some(t);
            }
        }
        Trigger::Pattern { bits, levels } | Trigger::Duration { bits, levels, .. } => {
            let duration = if let Trigger::Duration { when, .. } = *trig { Some(when) } else { None };
            out = pattern(inp, bits, levels, duration);
        }
        Trigger::Video { src, std, sync, level } => out = video(inp, src, std, sync, level),
    }
    out
}

/// First trigger point after `from` (fractional index).
pub fn find_first(trig: &Trigger, inp: &Input, from: f64) -> Option<f64> {
    find_all(trig, inp).into_iter().find(|&t| t > from)
}

fn pattern(inp: &Input, bits: [Bit; 4], levels: [f32; 4], duration: Option<When>) -> Vec<f64> {
    let mut ev: Vec<(f64, usize, bool)> = Vec::new();
    let mut state = [None; 4];
    for c in 0..4 {
        if bits[c] == Bit::X || inp.ch[c].is_empty() {
            continue;
        }
        let e = inp.edges(c, levels[c]);
        // initial state: before the first edge the signal is on the other side
        state[c] = Some(e.first().map_or(inp.ch[c][0] > levels[c], |f| !f.rising));
        ev.extend(e.iter().map(|x| (x.t, c, x.rising)));
    }
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let level_ok = |s: &[Option<bool>; 4], skip: Option<usize>| {
        (0..4).all(|c| {
            Some(c) == skip
                || match bits[c] {
                    Bit::H => s[c] == Some(true),
                    Bit::L => s[c] == Some(false),
                    _ => true,
                }
        })
    };
    let edge_bit = (0..4).find(|&c| matches!(bits[c], Bit::R | Bit::F));
    let mut out = Vec::new();
    let mut was = level_ok(&state, None);
    let mut since = None;
    for (t, c, rising) in ev {
        state[c] = Some(rising);
        if let Some(eb) = edge_bit {
            if duration.is_none() && c == eb && rising == (bits[eb] == Bit::R) && level_ok(&state, Some(eb)) {
                out.push(t);
            }
            continue;
        }
        let now = level_ok(&state, None);
        match duration {
            None => {
                if now && !was {
                    out.push(t);
                }
            }
            Some(w) => {
                if now && !was {
                    since = Some(t);
                } else if !now && was {
                    if let Some(s) = since.take() {
                        if w.ok((t - s) * inp.dt) {
                            out.push(t);
                        }
                    }
                }
            }
        }
        was = now;
    }
    out
}

fn video(inp: &Input, src: usize, std: VideoStd, sync: VideoSync, level: f32) -> Vec<f64> {
    let dt = inp.dt;
    let l = std.line();
    let hs = std.hsync();
    // low pulses: (start, width in s)
    let mut pulses = Vec::new();
    let mut start = None;
    for e in inp.edges(src, level) {
        if !e.rising {
            start = Some(e.t);
        } else if let Some(s) = start.take() {
            pulses.push((s, (e.t - s) * dt));
        }
    }
    #[derive(PartialEq, Clone, Copy)]
    enum K {
        Eq,
        H,
        Broad,
    }
    let kind = |w: f64| {
        if w > 4.0 * hs {
            Some(K::Broad)
        } else if w > 0.75 * hs && w < 1.6 * hs {
            Some(K::H)
        } else if w > 0.25 * hs && w <= 0.75 * hs {
            Some(K::Eq)
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let mut prev = None;
    let mut field_start: Option<f64> = None; // first broad pulse of the current field
    let mut line = 0u32;
    for &(t, w) in &pulses {
        let k = kind(w);
        match k {
            Some(K::Broad) if prev != Some(K::Broad) => {
                field_start = Some(t);
                line = 0;
            }
            Some(K::H) => {
                if line == 0 {
                    if let Some(fs) = field_start {
                        // parity: field 1's first normal line is an integer number of lines after the first broad pulse
                        let r = ((t - fs) * dt / l).fract();
                        let odd = !(0.25..0.75).contains(&r);
                        let hit = match sync {
                            VideoSync::AllFields => true,
                            VideoSync::OddField => odd,
                            VideoSync::EvenField => !odd,
                            _ => false,
                        };
                        if hit {
                            out.push(fs);
                        }
                    }
                }
                line += 1;
                let hit = match sync {
                    VideoSync::AllLines => true,
                    VideoSync::Line(n) => field_start.is_some() && line == n,
                    _ => false,
                };
                if hit {
                    out.push(t);
                }
            }
            _ => {}
        }
        prev = k;
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}
