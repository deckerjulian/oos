// SPDX-License-Identifier: GPL-2.0-or-later
//! MIPI C-PHY six-wirestate, single-trio electrical symbols and 16b/7s words.
//!
//! Mapper: MIPI DevCon 2016, "Implementing MIPI C-PHY", p12. Symbol/state
//! rules and the independent 0224100 -> 7290 example: MIPI "Introduction from
//! Basic Theory to Practical Implementation", pp14-16. This is not base-five
//! decoding, eighteen-wirestate mode, lane merging, or a CSI/DSI packet parser.
use crate::decode::{Kind, Packet};

const SYNC: [u8; 7] = [3, 4, 4, 4, 4, 4, 3];
const LIMIT: usize = 250_000;

const fn regions() -> [(u8, u32); 28] {
    let mut out = [(0, 0); 28];
    let mut i = 0;
    while i < 7 { out[i + 1] = (1 << i, 0x4000 + i as u32 * 0x1000); i += 1; }
    let mut index = 0;
    let mut lo = 0;
    while lo < 5 {
        let mut hi = lo + 1;
        while hi < 7 {
            out[index + 8] = ((1 << lo) | (1 << hi), 0xb000 + index as u32 * 0x400);
            index += 1; hi += 1;
        }
        lo += 1;
    }
    out
}
const REGIONS: [(u8, u32); 28] = regions();

pub fn word(symbols: &[u8; 7]) -> Option<u16> {
    if symbols.iter().any(|&s| s > 4) { return None; }
    let flip = symbols.iter().enumerate().fold(0u8, |mask, (i, &s)| mask | if s == 4 { 1 << (6 - i) } else { 0 });
    let base = REGIONS.iter().find(|r| r.0 == flip)?.1;
    let payload = symbols.iter().filter(|&&s| s != 4).fold(0u32, |v, &s| (v << 2) | s as u32);
    u16::try_from(base + payload).ok()
}

pub fn symbols_for_word(value: u16) -> [u8; 7] {
    let &(flip, base) = REGIONS.iter().rev().find(|r| r.1 <= value as u32).unwrap();
    let mut payload = value as u32 - base;
    let mut symbols = [0; 7];
    for i in (0..7).rev() {
        if flip & (1 << (6 - i)) != 0 { symbols[i] = 4; }
        else { symbols[i] = (payload & 3) as u8; payload >>= 2; }
    }
    symbols
}

/// State IDs: +x,+y,+z,-x,-y,-z. Only used to build deterministic test signals.
pub fn wire_values(state: u8) -> [f32; 3] {
    match state { 0 => [0.4, 0.0, 0.2], 1 => [0.2, 0.4, 0.0], 2 => [0.0, 0.2, 0.4],
        3 => [0.0, 0.4, 0.2], 4 => [0.2, 0.0, 0.4], _ => [0.4, 0.2, 0.0] }
}

pub fn states_for_symbols(initial: u8, symbols: &[u8]) -> Vec<u8> {
    let mut state = initial % 6;
    let mut states = vec![state];
    for &s in symbols {
        let axis = state % 3;
        let sign = state / 3;
        state = if s == 4 { axis + (1 - sign) * 3 }
            else { (axis + if s & 2 != 0 { 1 } else { 2 }) % 3 + (sign ^ (s & 1)) * 3 };
        states.push(state);
    }
    states
}

fn symbol(previous: u8, next: u8) -> Option<u8> {
    if previous == next { return None; }
    if previous % 3 == next % 3 { return Some(4); }
    let rotation = u8::from((next % 3 + 3 - previous % 3) % 3 == 1);
    Some(rotation * 2 + u8::from(previous / 3 != next / 3))
}

fn state(a: f32, b: f32, c: f32, gap: f32) -> u8 {
    if !a.is_finite() || !b.is_finite() || !c.is_finite() { return 255; }
    if a.max(b).max(c) > 0.8 { return 8 + ((a > 0.8) as u8 * 4 + (b > 0.8) as u8 * 2 + (c > 0.8) as u8); }
    if a.abs().max(b.abs()).max(c.abs()) < gap { return 8; }
    let mut levels = [a, b, c];
    levels.sort_by(f32::total_cmp);
    if levels[1] - levels[0] < gap || levels[2] - levels[1] < gap { return 255; }
    match ((a > b) as u8) * 4 + ((b > c) as u8) * 2 + (c > a) as u8 {
        4 => 0, 2 => 1, 1 => 2, 3 => 3, 5 => 4, 6 => 5, _ => 255,
    }
}

fn packet(kind: Kind, value: u32, start: usize, end: usize, ok: bool) -> Packet {
    Packet { kind, value, t0: start as f64, t1: end as f64, value2: None, ok }
}
#[derive(Clone, Copy)]
struct Run { state: u8, start: usize, end: usize }

/// Returns packet times as native sample indices, like the other DSP decoders.
/// Unaligned mid-burst captures expose electrical symbols rather than guessed words.
pub fn decode(a: &[f32], b: &[f32], c: &[f32], dt: f64, rate: Option<f64>, min_gap: f32) -> Vec<Packet> {
    let n = a.len().min(b.len()).min(c.len());
    if n < 8 || !dt.is_finite() || dt <= 0.0 { return vec![packet(Kind::Field("!SOURCE"), 0, 0, n, false)]; }
    let gap = if min_gap.is_finite() { min_gap.abs().max(1e-6) } else { 0.02 };
    let mut runs: Vec<Run> = Vec::new();
    for i in 0..n {
        let s = state(a[i], b[i], c[i], gap);
        if let Some(last) = runs.last_mut().filter(|last| last.state == s) { last.end = i + 1; }
        else {
            if runs.len() >= 1_000_000 { return vec![packet(Kind::Field("!LIMIT"), 0, 0, i, false)]; }
            runs.push(Run { state: s, start: i, end: i + 1 });
        }
    }
    let mut lengths: Vec<_> = runs.iter().filter(|r| r.state < 6).map(|r| r.end - r.start).collect();
    if lengths.len() < 3 { return vec![packet(Kind::Field("!SOURCE"), 0, 0, n, false)]; }
    lengths.sort_unstable();
    let ui = rate.filter(|r| r.is_finite() && *r > 0.0).map_or(lengths[lengths.len() * 3 / 4] as f64, |r| 1.0 / r / dt);
    if ui < 2.5 { return vec![packet(Kind::Field("!SAMPLE"), 0, 0, n, false)]; }
    let stable = (ui * 0.35).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    let mut previous: Option<Run> = None;
    let mut recent: Vec<(u8, usize, usize, usize)> = Vec::new();
    let mut group: Vec<(u8, usize, usize)> = Vec::new();
    let (mut threes, mut aligned, mut posted) = (0usize, false, false);
    for run in runs {
        if run.state >= 8 && run.state < 16 && run.end - run.start >= stable {
            const LP: [&str; 8] = ["!LP000", "!LP001", "!LP010", "!LP011", "!LP100", "!LP101", "!LP110", "!LP111"];
            out.push(packet(Kind::Field(LP[(run.state - 8) as usize]), 0, run.start, run.end, true));
            previous = None; recent.clear(); group.clear(); aligned = false; threes = 0; posted = false;
            continue;
        }
        if run.state >= 6 || run.end - run.start < stable { continue; }
        let Some(last) = previous.replace(run) else { continue; };
        if run.start.saturating_sub(last.end) as f64 > ui * 0.75 {
            recent.clear(); group.clear(); aligned = false; threes = 0;
            continue;
        }
        let Some(s) = symbol(last.state, run.state) else { continue; };
        let start = last.end;
        recent.push((s, start, run.end, threes));
        if recent.len() > 7 { recent.remove(0); }
        threes = if s == 3 { threes + 1 } else { 0 };
        if !aligned && recent.len() == 7 && recent.iter().map(|x| x.0).eq(SYNC) && recent[0].3 >= 7 {
            out.push(packet(Kind::Field("SYNC"), 0, recent[0].1, run.end, true));
            aligned = true; posted = false; group.clear(); continue;
        }
        if recent.len() == 7 && recent.iter().all(|x| x.0 == 4) && !posted {
            out.push(packet(Kind::Stop, 0, recent[0].1, run.end, true));
            aligned = false; posted = true; group.clear(); continue;
        }
        if aligned {
            group.push((s, start, run.end));
            if group.len() == 7 {
                let symbols: [u8; 7] = std::array::from_fn(|i| group[i].0);
                let value = word(&symbols);
                out.push(packet(if value.is_some() { Kind::Word { cmd: false } } else { Kind::Field("!CODE") },
                    value.map_or(0, u32::from), group[0].1, run.end, value.is_some()));
                group.clear();
            }
        } else if !posted { out.push(packet(Kind::Field("SYM"), s as u32, start, run.end, true)); }
        if out.len() >= LIMIT - 1 { out.push(packet(Kind::Field("!LIMIT"), 0, run.end, n, false)); break; }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_word_and_all_65536_values_use_the_real_mapper() {
        assert_eq!(word(&[0, 2, 2, 4, 1, 0, 0]), Some(0x7290));
        assert_eq!(symbols_for_word(0x7290), [0, 2, 2, 4, 1, 0, 0]);
        let mut seen = std::collections::HashSet::new();
        for value in 0..=u16::MAX {
            let s = symbols_for_word(value);
            assert!(seen.insert(s));
            assert_eq!(word(&s), Some(value));
        }
        assert_eq!(word(&SYNC), None);
        assert_eq!(word(&[4; 7]), None);
    }
    #[test]
    fn every_wire_transition_and_reference_burst_decodes_known_words() {
        for previous in 0..6 { for s in 0..5 {
            let next = states_for_symbols(previous, &[s])[1];
            assert_eq!(symbol(previous, next), Some(s));
            let v = wire_values(next);
            assert_eq!(state(v[0], v[1], v[2], 0.02), next);
        } }
        let mut symbols = vec![3; 12]; symbols.extend(SYNC);
        for value in [0x7290, 0x1234, 0xffff] { symbols.extend(symbols_for_word(value)); }
        symbols.extend([4; 7]);
        let mut wires: [Vec<f32>; 3] = Default::default();
        for s in states_for_symbols(0, &symbols) {
            for _ in 0..20 { for (wire, value) in wires.iter_mut().zip(wire_values(s)) { wire.push(value); } }
        }
        let packets = decode(&wires[0], &wires[1], &wires[2], 20e-12, None, 0.02);
        let words: Vec<_> = packets.iter().filter(|p| p.kind == Kind::Word { cmd: false }).map(|p| p.value).collect();
        assert_eq!(words, vec![0x7290, 0x1234, 0xffff]);
        assert!(packets.iter().any(|p| p.kind == Kind::Field("SYNC")));
        assert!(packets.iter().any(|p| p.kind == Kind::Stop));
        let mid = decode(&wires[0][400..700], &wires[1][400..700], &wires[2][400..700], 20e-12, None, 0.02);
        assert!(!mid.iter().any(|p| p.kind == Kind::Word { cmd: false }), "no invented word alignment");
    }
}
