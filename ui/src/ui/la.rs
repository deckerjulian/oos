// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Logic analyser page and lanes (DHO900 with the PLA2216 probe, docs/protocols/la.md): LA on/off, the lines shown
//! (D0-D7, D8-D15, all, or any set in the Lines sub menu), the threshold of each pod (presets or a user value; the
//! hardware has one comparator DAC per pod of 8 lines), the lane height. The lanes sit in a band at the bottom of the
//! waveform area: one two-level trace per line, and a filled band where a pixel column holds two or more transitions
//! (bus style), so dense clocks stay readable at any time base. The records carry the 16 lines as packed words
//! (bit n = Dn): the demo source generates them, the hardware source reads them after the analog record
//! (`board/dho/hw.rs`). Hidden on models without the LA port (`platform().caps.la`).

use super::*;
use std::sync::Arc;

/// Threshold presets: (name, volts). The last entry is the user value (`LaSet::user`).
pub const PRESETS: [(&str, f32); 8] = [
    ("TTL 1.4 V", 1.4),
    ("CMOS 5 V: 2.5 V", 2.5),
    ("CMOS 3.3 V: 1.65 V", 1.65),
    ("LVCMOS 2.5 V: 1.25 V", 1.25),
    ("LVCMOS 1.8 V: 0.9 V", 0.9),
    ("LVCMOS 1.2 V: 0.6 V", 0.6),
    ("ECL -1.3 V", -1.3),
    ("User", f32::NAN),
];
/// Index of the user threshold in PRESETS.
pub const USER: usize = PRESETS.len() - 1;
/// Heading row of the lane strip in px.
const LA_HEAD: i32 = 14;
/// Row of the decoder under the lanes, when it reads LA lines.
const LA_DEC: i32 = 26;
/// Height of the time labels at the bottom of a window: the lanes in a window end above them.
const AXIS_ROW: i32 = 22;
/// Lane pitch in px (small, medium, large).
pub const HEIGHTS: [(&str, i32); 3] = [("Small", 12), ("Medium", 16), ("Large", 22)];
/// Line groups of the Lines row; "Custom" = any other set (Lines sub menu).
const GROUPS: [(&str, u16); 4] = [("D0-D7", 0x00ff), ("D8-D15", 0xff00), ("All", 0xffff), ("Custom", 0x000f)];
/// Line names (menu labels are static).
const NAMES: [&str; 16] = ["D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9", "D10", "D11", "D12", "D13", "D14", "D15"];
/// The hardware limits of a pod threshold (la.md 5.1).
pub const THR_MAX: f32 = 15.0;
/// Lane colours of pod 1 (D0-D7) and pod 2 (D8-D15).
const POD_COLORS: [Rgb; 2] = [rgb(0x4c, 0xd1, 0x6b), rgb(0xb4, 0xe6, 0x5a)];
/// Samples of a demo LA record (8 per pixel column of the full-width plot: the bus-style bands show).
pub const DEMO_SAMPLES: usize = 8192;

/// Pattern values of a line.
const PAT_BITS: [Bit; 3] = [Bit::H, Bit::L, Bit::X];

/// Whether the word `w` matches `pat`.
pub fn pattern_is(w: u16, pat: &[Bit; 16]) -> bool {
    pat.iter().enumerate().all(|(n, b)| match b {
        Bit::H => w >> n & 1 == 1,
        Bit::L => w >> n & 1 == 0,
        _ => true,
    })
}

/// First sample from `from` on where the lines match `pat` after they did not (the moment the pattern begins, as the
/// TPU fires it); a pattern already true at `from` counts from its next beginning. All X: None.
pub fn pattern_at(words: &[u16], pat: &[Bit; 16], from: usize) -> Option<usize> {
    let (mut mask, mut want) = (0u16, 0u16);
    for (n, b) in pat.iter().enumerate() {
        match b {
            Bit::H => (mask, want) = (mask | 1 << n, want | 1 << n),
            Bit::L => mask |= 1 << n,
            _ => {}
        }
    }
    if mask == 0 {
        return None;
    }
    let hit = |w: u16| w & mask == want;
    (from.max(1)..words.len()).find(|&k| hit(words[k]) && !hit(words[k - 1]))
}

/// Whether line `n` changes anywhere in the record.
pub fn line_moves(la: &LaRec, n: u8) -> bool {
    la.words.windows(2).any(|w| (w[0] ^ w[1]) >> n & 1 == 1)
}

/// Colour of LA line `n`.
pub fn line_color(n: u8) -> Rgb {
    POD_COLORS[(n as usize / 8).min(1)]
}

/// The model has the LA port.
pub fn available() -> bool {
    oos_hal::model::platform().caps.la
}

/// LA settings of the UI model (part of setups).
#[derive(Clone, Debug, PartialEq)]
pub struct LaSet {
    pub on: bool,
    /// lines shown: bit n = Dn (the hardware records all 16 anyway, la.md 0.1)
    pub show: u16,
    /// threshold preset per pod (index into PRESETS)
    pub preset: [usize; 2],
    /// user threshold per pod in volts
    pub user: [f32; 2],
    /// index into HEIGHTS
    pub height: usize,
}

impl Default for LaSet {
    fn default() -> LaSet {
        LaSet { on: false, show: 0xffff, preset: [0, 0], user: [1.65, 1.65], height: 1 }
    }
}

impl LaSet {
    /// Threshold of pod `p` (0 = D0-D7, 1 = D8-D15) in volts.
    pub fn thr(&self, p: usize) -> f32 {
        let k = self.preset[p].min(USER);
        if k == USER {
            self.user[p]
        } else {
            PRESETS[k].1
        }
    }

    /// The lines shown, D0 first.
    pub fn lines(&self) -> Vec<usize> {
        (0..16).filter(|n| self.show >> n & 1 == 1).collect()
    }

    /// Setup value: `on show preset1 preset2 user1 user2 height` (show in hex).
    pub fn to_text(&self) -> String {
        format!("{} {:04x} {} {} {} {} {}", self.on, self.show, self.preset[0], self.preset[1], self.user[0], self.user[1], self.height)
    }

    pub fn parse(v: &str) -> Result<LaSet, String> {
        let w: Vec<&str> = v.split_whitespace().collect();
        if w.len() != 7 {
            return Err("la: want 7 values".into());
        }
        let b = |s: &str| match s {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!("not a boolean: {}", s)),
        };
        let idx = |s: &str, n: usize| s.parse::<usize>().ok().filter(|&i| i < n).ok_or_else(|| format!("bad index {}", s));
        let volt = |s: &str| s.parse::<f32>().ok().filter(|x| x.is_finite()).map(|x| x.clamp(-THR_MAX, THR_MAX)).ok_or_else(|| format!("not a number: {}", s));
        Ok(LaSet {
            on: b(w[0])?,
            show: u16::from_str_radix(w[1], 16).map_err(|e| e.to_string())?,
            preset: [idx(w[2], PRESETS.len())?, idx(w[3], PRESETS.len())?],
            user: [volt(w[4])?, volt(w[5])?],
            height: idx(w[6], HEIGHTS.len())?,
        })
    }
}

/// The 16 lines of a record as packed words: word k (bit n = Dn) at time t0 + k dt relative to the trigger.
#[derive(Clone, Debug)]
pub struct LaRec {
    pub words: Arc<Vec<u16>>,
    pub t0: f64,
    pub dt: f64,
}

/// One pixel column of all 16 lines (bit n = Dn): level at the end of the column, lines that changed in the column
/// (or from the previous column), lines that changed at least twice in it (drawn as a filled band).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Col {
    pub hi: u16,
    pub edge: u16,
    pub busy: u16,
    /// the column has data (inside the record)
    pub valid: bool,
}

/// The columns 0..w of words whose sample k sits at x = x0 + k * xps (px). One pass over the samples in view, all
/// 16 lines at once with bit operations; between two sparse samples the level holds and the change shows in the column
/// of the later sample.
pub fn columns(words: &[u16], x0: f64, xps: f64, w: usize) -> Vec<Col> {
    let mut out = vec![Col::default(); w];
    if words.is_empty() || w == 0 || !(xps > 0.0) || !x0.is_finite() {
        return out;
    }
    let col = |k: usize| (x0 + k as f64 * xps).floor();
    // the last sample left of column 0 (its level reaches into the view), else the first one
    let k0 = ((-x0 / xps).floor().max(0.0) as usize).min(words.len() - 1);
    let mut prev = words[k0];
    let pc = col(k0);
    if pc >= w as f64 {
        return out;
    }
    if pc >= 0.0 {
        out[pc as usize] = Col { hi: prev, edge: 0, busy: 0, valid: true };
    }
    let mut pc = pc as i64;
    for (k, &wd) in words.iter().enumerate().skip(k0 + 1) {
        let c = col(k);
        let hold_to = if c >= w as f64 { w as i64 } else { c as i64 };
        for j in (pc + 1).max(0)..hold_to {
            out[j as usize] = Col { hi: prev, edge: 0, busy: 0, valid: true };
        }
        if c >= w as f64 {
            break;
        }
        let c = c as i64;
        let d = prev ^ wd;
        if c > pc {
            if c >= 0 {
                out[c as usize] = Col { hi: wd, edge: d, busy: 0, valid: true };
            }
        } else if c >= 0 {
            let o = &mut out[c as usize];
            o.busy |= o.edge & d;
            o.edge |= d;
            o.hi = wd;
        }
        prev = wd;
        pc = c;
    }
    out
}

/// Key of the cached columns: words (pointer, length), mapping, width.
type ColKey = (usize, usize, u64, u64, usize);

/// Columns of the last drawn LA record (the screen redraws ~30 times a second; deep records are walked once).
#[derive(Default)]
pub struct Cache(Option<(ColKey, Arc<Vec<Col>>)>);

impl Ui {
    pub(super) fn la_items(&self) -> Vec<Item> {
        let mut v = vec![Item::LaOn, Item::LaShow, Item::Open(MenuId::LaLines), Item::LaHeight];
        for p in 0..2 {
            v.push(Item::LaThr(p));
            if self.m.la.preset[p] == USER {
                v.push(Item::LaUser(p));
            }
        }
        v
    }

    pub(super) fn la_line_items(&self) -> Vec<Item> {
        (0..16).map(Item::LaLine).collect()
    }

    pub(super) fn la_kind(it: Item) -> Option<Kind> {
        Some(match it {
            Item::LaOn | Item::LaShow | Item::LaThr(_) | Item::LaHeight | Item::LaLine(_) | Item::LaPat(_) => Kind::Choice,
            Item::LaUser(_) => Kind::Value,
            _ => return None,
        })
    }

    pub(super) fn la_label(it: Item) -> Option<&'static str> {
        Some(match it {
            Item::Open(MenuId::La) => "Logic analyser",
            Item::Open(MenuId::LaLines) => "Line select",
            Item::LaOn => "LA",
            Item::LaShow => "Lines",
            Item::LaThr(0) => "Thr D0-7",
            Item::LaThr(_) => "Thr D8-15",
            Item::LaUser(0) => "User D0-7",
            Item::LaUser(_) => "User D8-15",
            Item::LaHeight => "Lane height",
            Item::LaLine(n) | Item::LaPat(n) => NAMES[n.min(15)],
            Item::Open(MenuId::LaPat) => "D0-D15",
            Item::DecIn => "Input",
            _ => return None,
        })
    }

    pub(super) fn la_value(&self, it: Item) -> Option<String> {
        let l = &self.m.la;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        Some(match it {
            Item::LaOn => onoff(l.on),
            Item::LaShow => GROUPS.iter().take(3).find(|g| g.1 == l.show).map_or(GROUPS[3].0, |g| g.0).into(),
            Item::LaThr(p) => PRESETS[l.preset[p.min(1)].min(USER)].0.into(),
            Item::LaUser(p) => format!("{:.2} V", l.user[p.min(1)]),
            Item::LaHeight => HEIGHTS[l.height.min(HEIGHTS.len() - 1)].0.into(),
            Item::LaLine(n) => onoff(l.show >> n.min(15) & 1 == 1),
            Item::LaPat(n) => ["H", "L", "X"][PAT_BITS.iter().position(|&b| b == self.m.pat_la[n.min(15)]).unwrap_or(2)].into(),
            Item::Open(MenuId::LaPat) => self.m.pat_la.iter().rev().map(|&b| match b { Bit::H => '1', Bit::L => '0', _ => 'x' }).collect(),
            Item::DecIn => if self.m.dec_la_in { "LA".into() } else { "Analog".into() },
            Item::TrigSrc if self.m.trig_line().is_some() => NAMES[self.m.trig_line().unwrap_or(0) as usize].to_string(),
            // decoder roles on LA lines
            Item::DecSrc | Item::DecSrc2 | Item::DecCs | Item::DecSrc4 if self.m.dec_on_la() => self.m.dec_la[Self::dec_role(it)].map_or("None".into(), |n| NAMES[n as usize].to_string()),
            _ => return None,
        })
    }

    /// Index in `Model::dec_la` of a decoder source item.
    fn dec_role(it: Item) -> usize {
        match it {
            Item::DecSrc2 => 1,
            Item::DecCs => 2,
            Item::DecSrc4 => 3,
            _ => 0,
        }
    }

    pub(super) fn la_options(&self, it: Item) -> Option<Vec<String>> {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Some(match it {
            Item::LaOn | Item::LaLine(_) => names(&["Off", "On"]),
            Item::LaShow => GROUPS.iter().map(|g| g.0.to_string()).collect(),
            Item::LaThr(_) => PRESETS.iter().map(|p| p.0.to_string()).collect(),
            Item::LaHeight => HEIGHTS.iter().map(|h| h.0.to_string()).collect(),
            Item::LaPat(_) => names(&["H", "L", "X"]),
            Item::DecIn => names(&["Analog", "LA"]),
            Item::TrigSrc if self.m.trig_la_ok() => ch_names().into_iter().chain(NAMES.iter().map(|s| s.to_string())).collect(),
            Item::DecSrc | Item::DecSrc2 if self.m.dec_on_la() => names(&NAMES),
            Item::DecCs | Item::DecSrc4 if self.m.dec_on_la() => std::iter::once("None".to_string()).chain(NAMES.iter().map(|s| s.to_string())).collect(),
            _ => return None,
        })
    }

    pub(super) fn la_adjust(&mut self, it: Item, d: i32) -> bool {
        match it {
            Item::DecIn => {
                self.m.dec_la_in = !self.m.dec_la_in;
                self.settings_changed();
                return true;
            }
            Item::TrigSrc if self.m.trig_la_ok() => {
                // channels first, then D0..D15
                let n = n_ch();
                let i = self.m.trig_line().map_or(self.m.trig_src, |l| n + l as usize) as i32;
                let k = (i + d).rem_euclid((n + 16) as i32) as usize;
                if k < n {
                    (self.m.trig_src, self.m.trig_la) = (k, None);
                } else {
                    self.m.trig_la = Some((k - n) as u8);
                }
                self.settings_changed();
                return true;
            }
            Item::DecSrc | Item::DecSrc2 | Item::DecCs | Item::DecSrc4 if self.m.dec_on_la() => {
                // 0 = none on the optional roles, then D0..D15; the required roles cycle D0..D15
                let r = Self::dec_role(it);
                let slot = &mut self.m.dec_la[r];
                if r < 2 {
                    *slot = Some((slot.unwrap_or(0) as i32 + d).rem_euclid(16) as u8);
                } else {
                    let k = (slot.map_or(0, |n| n as i32 + 1) + d).rem_euclid(17);
                    *slot = if k == 0 { None } else { Some(k as u8 - 1) };
                }
                return true;
            }
            _ => {}
        }
        let l = &mut self.m.la;
        let cyc = |i: usize, n: usize| (i as i32 + d).rem_euclid(n as i32) as usize;
        match it {
            Item::LaOn => l.on = !l.on && available(),
            Item::LaShow => {
                let i = GROUPS.iter().take(3).position(|g| g.1 == l.show).unwrap_or(3);
                let k = cyc(i, GROUPS.len());
                // Custom keeps a custom set, else starts with D0-D3 (refined in the Lines sub menu)
                if k != 3 || i != 3 {
                    l.show = GROUPS[k].1;
                }
            }
            Item::LaLine(n) => {
                l.show ^= 1 << n.min(15);
            }
            Item::LaPat(n) => {
                let p = &mut self.m.pat_la[n.min(15)];
                *p = PAT_BITS[cyc(PAT_BITS.iter().position(|b| b == p).unwrap_or(2), 3)];
            }
            Item::LaThr(p) => l.preset[p.min(1)] = cyc(l.preset[p.min(1)], PRESETS.len()),
            Item::LaUser(p) => {
                let u = &mut l.user[p.min(1)];
                *u = ((*u + 0.01 * d as f32) * 100.0).round().clamp(-THR_MAX * 100.0, THR_MAX * 100.0) / 100.0;
            }
            Item::LaHeight => l.height = (l.height as i32 + d).clamp(0, HEIGHTS.len() as i32 - 1) as usize,
            _ => return false,
        }
        self.settings_changed();
        true
    }

    /// LA key: like a channel key. Off: switch on and open the page; page open: switch off and close; else open.
    pub(super) fn la_key(&mut self) {
        if !self.m.la.on && !self.la_probe_ok() {
            return self.toast("No LA probe connected");
        }
        if !self.m.la.on {
            self.m.la.on = true;
            self.open_menu(MenuId::La);
        } else if self.menu_open && matches!(self.menu_root(), MenuId::La | MenuId::LaLines) {
            self.m.la.on = false;
            self.close_menu();
        } else {
            self.open_menu(MenuId::La);
        }
        self.settings_changed();
    }

    /// From `step`: the probe state the hardware thread read.
    pub(super) fn la_probe_poll(&mut self) {
        if let Source::Hw { shared, .. } = &self.src {
            let p = shared.lock().unwrap().la_probe;
            self.la_probe_set(p);
        }
    }

    /// A probe that goes away takes the LA with it, as on the stock system; so does a setup saved with the LA on, read
    /// without a probe. SCPI can still switch it on afterwards (no change of the state here), which is the way out if the
    /// detection ever reads wrong.
    pub(super) fn la_probe_set(&mut self, p: Option<bool>) {
        if self.la_probe != Some(false) && p == Some(false) && self.m.la.on {
            self.m.la.on = false;
            self.settings_changed();
            self.toast("No LA probe: LA off");
        }
        self.la_probe = p;
    }

    /// The LA can be switched on: the probe is there, or nobody knows otherwise (demo signals, not read yet).
    pub(super) fn la_probe_ok(&self) -> bool {
        self.la_probe != Some(false)
    }

    /// Page title with the probe state (GPIO 120 on the unit).
    pub(super) fn la_title(&self) -> String {
        let probe = match (&self.src, self.la_probe) {
            (Source::Hw { .. }, Some(true)) => "probe connected",
            (Source::Hw { .. }, Some(false)) => "no probe",
            (Source::Hw { .. }, None) => "probe not read",
            _ => "demo lines",
        };
        format!("Logic analyser \u{b7} {}", probe)
    }

    /// Height (with the 2 px gap above it) and lane pitch of the strip for `n` lanes: at most half of the height shared
    /// with the graticule (the lanes get narrower, at least 9 px).
    fn la_fit(&self, n: usize) -> (i32, i32) {
        let mut pitch = HEIGHTS[self.m.la.height.min(HEIGHTS.len() - 1)].1;
        let room = self.plot_base() / 2 - LA_HEAD - self.la_dec_h() - 4;
        if n as i32 * pitch > room {
            pitch = (room / n.max(1) as i32).max(9);
        }
        (n as i32 * pitch + LA_HEAD + self.la_dec_h() + 4, pitch)
    }

    /// Decoders on LA lines with the lanes on the screen: their rows are in the strip, under what they decode.
    pub(super) fn la_dec_count(&self) -> i32 {
        if self.m.la.lines().is_empty() || self.m.view != AView::Scope || self.xy || !self.m.la_on() {
            return 0;
        }
        (0..DECODERS).filter(|&k| self.m.dec_is_on(k) && self.m.dec_slot(k).la_in).count() as i32
    }

    /// Height of the decoder row, if there is one.
    fn la_dec_h(&self) -> i32 {
        self.la_dec_count() * LA_DEC
    }

    /// Row i of the decoders in the strip; None when a decoder has its band in the analog plot.
    pub(super) fn la_dec_row_n(&self, i: i32) -> Option<Rect> {
        (i < self.la_dec_count()).then(|| {
            let n = self.m.la.lines().len();
            let (band, pitch) = self.la_band(n);
            Rect::new(band.x, band.y + LA_HEAD + n as i32 * pitch + i * LA_DEC, band.w, LA_DEC)
        })
    }

    /// The first of them.
    #[cfg(test)]
    pub(super) fn la_dec_row(&self) -> Option<Rect> {
        self.la_dec_row_n(0)
    }

    /// The lane strip under the graticule: (rectangle, lane pitch) for `n` lanes. In a split plot the lanes stand at
    /// the bottom of their window: over its lower half while a channel shares the window, all of it when not.
    pub(super) fn la_band(&self, n: usize) -> (Rect, i32) {
        if self.wins.len() > 1 {
            let w = self.lane_area();
            let shared = self.win_has_analog(self.la_win_of());
            // the time labels of the window stay readable under the lanes
            let room = if shared { w.h / 2 } else { w.h - AXIS_ROW } - LA_HEAD - self.la_dec_h() - 4;
            let want = HEIGHTS[self.m.la.height.min(HEIGHTS.len() - 1)].1;
            // alone in the window the lanes take its height (up to twice the chosen pitch); shared, at most the pitch
            let fill = (room / n.max(1) as i32).max(9);
            let pitch = if shared { want.min(fill) } else { fill.min(2 * want) };
            let h = n as i32 * pitch + LA_HEAD + self.la_dec_h() + 4;
            return (Rect::new(w.x, w.y + w.h - AXIS_ROW - h, w.w, h), pitch);
        }
        let (h, pitch) = self.la_fit(n);
        let g = self.grid;
        (Rect::new(g.x, g.y + g.h + 2 + self.eye_below(), g.w, h - 2), pitch)
    }

    /// Height the lane strip takes from the plot (0 when no lanes are drawn: analysers, XY, LA off; and in a split plot,
    /// where the lanes are in a window).
    pub(super) fn la_strip_h(&self) -> i32 {
        let n = self.m.la.lines().len();
        if self.m.la_on() && n > 0 && matches!(self.m.view, AView::Scope | AView::Eye) && !self.xy && self.n_wins() == 1 {
            self.la_fit(n).0
        } else {
            0
        }
    }

    /// Decoder packets of the LA lines of `rec`, times in s; cached per record and decoder settings, because the band
    /// is drawn every frame and a record can hold millions of words.
    pub(super) fn la_packets(&self, m: &Model, rec: &Record) -> Result<Vec<oos_dsp::decode::Packet>, &'static str> {
        use std::hash::{Hash, Hasher};
        let Some(la) = &rec.la else { return Err("no LA data") };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (Arc::as_ptr(&la.words) as usize, la.words.len(), la.t0.to_bits(), la.dt.to_bits()).hash(&mut h);
        format!(
            "{:?}",
            (m.dec_proto, m.dec_la, m.dec_baud, m.dec_bits, m.dec_parity, m.dec_mode, (m.dec_canrate, m.dec_fdrate, m.dec_canh), (m.dec_arate, m.dec_tick, m.dec_nibbles, m.dec_uspeed))
        )
        .hash(&mut h);
        let key = h.finish();
        if let Some(e) = self.dec_cache.borrow().iter().find(|e| e.0 == key) {
            return Ok(e.1.clone());
        }
        let mut p = crate::trig::decode_la(m, &la.words, la.dt)?;
        for x in p.iter_mut() {
            x.t0 = la.t0 + x.t0 * la.dt;
            x.t1 = la.t0 + x.t1 * la.dt;
        }
        self.dec_cache_put(key, &p);
        Ok(p)
    }

    /// The columns of the LA record for the trace width `w` with sample k at x0 + k xps, cached.
    fn la_columns(&self, la: &LaRec, x0: f64, xps: f64, w: usize) -> Arc<Vec<Col>> {
        let key = (Arc::as_ptr(&la.words) as usize, la.words.len(), x0.to_bits(), xps.to_bits(), w);
        let mut c = self.la_cols.borrow_mut();
        if let Some((k, v)) = &c.0 {
            if *k == key {
                return v.clone();
            }
        }
        let v = Arc::new(columns(&la.words, x0, xps, w));
        c.0 = Some((key, v.clone()));
        v
    }

    pub(super) fn draw_la(&self, cv: &mut Canvas) {
        let l = &self.m.la;
        if !available() || !l.on {
            return;
        }
        let lines = l.lines();
        if lines.is_empty() {
            return;
        }
        let f = &self.fonts;
        let a = self.lane_area();
        let (band, pitch) = self.la_band(lines.len());
        cv.set_clip(band);
        cv.fill(band, BG);
        cv.hline(band.x, band.x + band.w - 1, band.y, LINE);
        let head = format!("LA   D0-7 {}   D8-15 {}", si_short(l.thr(0) as f64, "V"), si_short(l.thr(1) as f64, "V"));
        let hx = cv.text(&f.s, a.x + MARK_W + 4, band.y + 12, &head, DIM);
        let rec = self.last[0].as_ref().and_then(|r| r.la.as_ref());
        if rec.is_none() {
            cv.text(&f.s, hx + 16, band.y + 12, "no LA data", DIM);
        }
        // traces between the marker strips
        let (xl, w) = (a.x + MARK_W, (a.w - 2 * MARK_W).max(1));
        let cols = rec.map(|la| {
            let (tdiv, delay) = self.view();
            let ppd = a.w as f64 / HDIV as f64;
            let x0 = a.w as f64 / 2.0 + (la.t0 - delay as f64) / tdiv as f64 * ppd - MARK_W as f64;
            self.la_columns(la, x0, la.dt / tdiv as f64 * ppd, w as usize)
        });
        for (i, &n) in lines.iter().enumerate() {
            let top = band.y + LA_HEAD + i as i32 * pitch;
            let (yh, yl) = (top + 2, top + pitch - 3);
            let col = POD_COLORS[n / 8];
            cv.text(&f.s, a.x + 3, yl + 1, NAMES[n], col);
            let Some(cols) = &cols else { continue };
            let bit = 1u16 << n;
            let dim = scale(col, 90);
            for (j, c) in cols.iter().enumerate() {
                if !c.valid {
                    continue;
                }
                let x = xl + j as i32;
                if c.busy & bit != 0 {
                    // many transitions in this column: a filled band between the rails
                    cv.vline(x, yh + 1, yl - 1, dim);
                    cv.pixel(x, yh, col, 255);
                    cv.pixel(x, yl, col, 255);
                } else if c.edge & bit != 0 {
                    cv.vline(x, yh, yl, col);
                } else {
                    cv.pixel(x, if c.hi & bit != 0 { yh } else { yl }, col, 255);
                }
            }
        }
        cv.no_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_hold_levels_mark_edges_and_compress_bursts() {
        // sparse: 4 samples, 10 px apart, from x = 5: D0 0 1 1 0, D1 toggles every sample
        let w = [0b00, 0b11, 0b01, 0b10];
        let c = columns(&w, 5.0, 10.0, 40);
        assert!(!c[4].valid && c[5].valid && c[5].hi == 0 && c[5].edge == 0);
        assert_eq!((c[14].hi, c[14].edge), (0, 0), "level holds until the next sample");
        assert_eq!((c[15].hi, c[15].edge, c[15].busy), (0b11, 0b11, 0));
        assert_eq!((c[25].hi, c[25].edge), (0b01, 0b10));
        assert_eq!((c[35].hi, c[35].edge), (0b10, 0b11));
        assert!(!c[36].valid, "no data after the last sample");
        // dense: 100 samples per column, D0 toggles every sample (busy), D1 once in column 1, D2 never
        let w: Vec<u16> = (0..400).map(|k| (k & 1) as u16 | if k >= 150 { 2 } else { 0 }).collect();
        let c = columns(&w, 0.0, 0.01, 4);
        assert!(c.iter().all(|c| c.valid && c.busy & 1 == 1 && c.busy & 6 == 0));
        assert_eq!((c[0].edge & 2, c[1].edge & 2, c[2].edge & 2), (0, 2, 0));
        assert_eq!(c[3].hi & 6, 2);
        // a record starting left of the view: the level of the sample before column 0 reaches into it
        let c = columns(&[0, 1, 0], -15.0, 10.0, 20);
        assert_eq!((c[0].hi, c[4].hi, c[5].hi, c[5].edge), (1, 1, 0, 1));
        assert!(columns(&[], 0.0, 1.0, 8).iter().all(|c| !c.valid));
        assert!(columns(&[1, 2], 50.0, 1.0, 8).iter().all(|c| !c.valid));
    }

    #[test]
    fn columns_of_a_deep_record_are_fast() {
        // 2 M words (the LA read limit) into 972 columns
        let w: Vec<u16> = (0..2_000_000u32).map(|k| (k >> 3) as u16).collect();
        let t = std::time::Instant::now();
        let c = columns(&w, 0.0, 972.0 / 2e6, 972);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("PERF columns of 2 M LA words: {:.1} ms", ms);
        assert!(c.iter().all(|c| c.valid && c.busy & 1 == 1));
    }

    #[test]
    fn presets_and_setup_text() {
        // preset volts -> pod DAC codes (la.md 5.1, the unit's zero codes)
        let cal = oos_hal::dho::la::Cal::default();
        let code = |v: f32| oos_hal::dho::la::thresholds(&cal, v as f64, v as f64).1;
        assert_eq!(code(PRESETS[0].1), 0x2336_2353);
        assert_eq!(code(PRESETS[2].1) & 0xffff, 9120);
        assert_eq!(code(PRESETS[1].1) >> 16, 9355);
        let mut l = LaSet { on: true, show: 0x0f0f, preset: [2, USER], user: [0.5, -0.25], height: 2 };
        assert_eq!((l.thr(0), l.thr(1)), (1.65, -0.25));
        assert_eq!(LaSet::parse(&l.to_text()).unwrap(), l);
        assert_eq!(l.lines(), [0, 1, 2, 3, 8, 9, 10, 11]);
        l.user[0] = 99.0;
        assert_eq!(LaSet::parse(&l.to_text()).unwrap().user[0], 15.0);
        assert!(LaSet::parse("true ffff 0 0").is_err());
        assert!(LaSet::parse("true ffff 9 0 1 1 1").is_err());
    }

    #[test]
    fn la_memory_and_rate_limits() {
        // the DHO924S limits: 125 M / 1.25 GSa/s with one channel, 31.25 M with the LA, rate halved with a
        // channel shown
        let mut m = Model::default();
        let (mem, sr) = (m.max_mem(), m.max_srate());
        m.la.on = true;
        assert_eq!(m.max_mem(), oos_hal::model::platform().limits.mem(1, true));
        assert!(m.max_mem() < mem);
        assert_eq!(m.max_srate(), sr / 2.0);
        // the hardware settings carry the LA, depth capped to the LA memory
        m.mdepth = mdepths().iter().position(|d| d.1 == 125_000_000).unwrap();
        let s = crate::board::dho::hw::settings(&m, false);
        let l = s.la.expect("LA in the settings");
        assert!(l.analog_shown && (l.thr[0] - 1.4).abs() < 1e-6);
        assert_eq!(s.depth, m.max_mem() as i128);
        m.la.on = false;
        assert!(crate::board::dho::hw::settings(&m, false).la.is_none());
        assert_eq!(crate::board::dho::hw::settings(&m, false).depth, 125_000_000);
    }

    #[test]
    fn la_page_key_and_lanes_on_the_demo() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert!(ui.items(MenuId::Main).contains(&Item::Open(MenuId::La)));
        ui.key(key::LA);
        assert!(ui.m.la.on && ui.menu_open && ui.menu == MenuId::La);
        // every option selects and reads back
        for it in [Item::LaShow, Item::LaThr(0), Item::LaThr(1), Item::LaHeight, Item::LaOn, Item::LaLine(5)] {
            let opts = ui.options(it);
            for o in opts.iter().rev().chain(opts.iter()) {
                ui.select_option(it, o);
                assert_eq!(&ui.value(it), o, "{:?}", it);
            }
        }
        ui.m.la.on = true;
        ui.select_option(Item::LaThr(1), "User");
        assert!(ui.items(MenuId::La).contains(&Item::LaUser(1)));
        ui.adjust(Item::LaUser(1), -7);
        assert_eq!(ui.m.la.thr(1), 1.58);
        ui.select_option(Item::LaShow, "All");
        ui.adjust(Item::LaLine(3), 1);
        assert_eq!((ui.m.la.show, ui.value(Item::LaShow).as_str()), (0xfff7, "Custom"));
        // the demo record carries the 16 lines: D14 is the 1 kHz square of CH1 (edge trigger at 1.5 V), high right
        // after the trigger and low before it
        ui.m.la.show = 0xffff;
        for _ in 0..5 {
            ui.step(0.033);
        }
        let rec = ui.last[0].as_ref().unwrap();
        let la = rec.la.as_ref().expect("LA words");
        assert_eq!(la.words.len(), DEMO_SAMPLES);
        let at = |t: f64| la.words[((t - la.t0) / la.dt) as usize];
        assert!(at(20e-6) >> 14 & 1 == 1 && at(-20e-6) >> 14 & 1 == 0);
        let mut cv = Canvas::new(W, H);
        ui.close_menu();
        ui.render(&mut cv);
        // the band is drawn: the lane of D0 has trace pixels of the pod-1 colour
        let (band, pitch) = ui.la_band(16);
        let y0 = band.y + 14;
        let lit = (y0..y0 + pitch).flat_map(|y| (band.x + MARK_W..band.x + band.w - MARK_W).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] & 0xff_ffff == POD_COLORS[0]).count();
        assert!(lit > 500, "{} lit pixels", lit);
        // the LA key again: off and closed
        ui.key(key::LA);
        ui.key(key::LA);
        assert!(!ui.m.la.on && !ui.menu_open);
        // setups keep the LA settings
        ui.m.la = LaSet { on: true, show: 0x00f0, preset: [3, USER], user: [0.3, 2.0], height: 0 };
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!(back.la, ui.m.la);
    }
}

#[cfg(test)]
mod decode_tests {
    use super::*;
    use oos_dsp::decode::Kind;

    /// A UI with the LA on and the decoder on LA lines, some records acquired.
    fn ui_on(p: Proto) -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.la.on = true;
        ui.m.dec_on = true;
        ui.set_dec_proto(p);
        ui.m.dec_la_in = true;
        for _ in 0..6 {
            ui.step(0.05);
        }
        ui
    }

    /// The switch of the decoder names no protocol: that is the row below it.
    #[test]
    fn decoder_switch_is_not_named_after_a_protocol() {
        assert_eq!(ui_on(Proto::I2s).item_label(Item::DecOn), "Decoder");
    }

    /// The decoder of LA lines sits under the lanes; the analog plot stays free. On analog channels it stays in the plot.
    #[test]
    fn decoder_row_is_beside_the_lanes() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
        let _ = std::fs::create_dir_all(&dir);
        let mut ui = ui_on(Proto::I2c);
        ui.m.tscale = tscales().iter().position(|x| (*x - 50e-6).abs() < 1e-7).unwrap();
        for _ in 0..6 {
            ui.step(0.05);
        }
        let row = ui.la_dec_row().unwrap();
        let (band, pitch) = ui.la_band(ui.m.la.lines().len());
        assert!(pitch >= 9);
        assert!(row.y >= band.y + LA_HEAD + 16 * pitch, "the row must be under the last lane: {:?} {:?}", row, band);
        assert!(row.y + row.h <= band.y + band.h + 2, "the row must stay in the strip: {:?} {:?}", row, band);
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        let _ = crate::store::write(&dir.join("la-decoder-row.png"), &crate::store::png_of(&cv));
        // something is drawn in the row, right of its label
        let px = |cv: &Canvas, r: Rect| (r.x + 60..r.x + r.w - 20).filter(|&x| cv.px[((r.y + r.h / 2) * cv.w + x) as usize] != BG).count();
        assert!(px(&cv, row) > 100, "the decoder row is empty");
        // off, or on the analog channels: no row, and the lanes move back up
        ui.m.dec_la_in = false;
        assert!(ui.la_dec_row().is_none());
        ui.m.dec_la_in = true;
        ui.m.dec_on = false;
        assert!(ui.la_dec_row().is_none());
        // no lanes shown: there is nothing to sit beside, the band goes back into the plot
        ui.m.dec_on = true;
        ui.m.la.on = false;
        assert!(ui.la_dec_row().is_none());
    }

    /// Without the probe the LA stays off, whichever way it is asked for; pulling the probe switches it off.
    #[test]
    fn the_la_needs_its_probe() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        // demo signals: no probe to ask for
        assert!(ui.la_probe_ok());
        ui.la_probe_set(Some(false));
        let r = ui.aux_cards().0.unwrap();
        ui.tap(r.x + 10, r.y + 10);
        assert!(!ui.m.la.on && !ui.menu_open);
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("No LA probe")));
        ui.key(key::LA);
        assert!(!ui.m.la.on && !ui.menu_open);
        ui.open_menu(MenuId::Main);
        ui.activate(Item::Open(MenuId::La));
        assert!(ui.menu != MenuId::La);
        ui.close_menu();
        // plugged in: as before
        ui.la_probe_set(Some(true));
        ui.key(key::LA);
        assert!(ui.m.la.on && ui.menu == MenuId::La);
        // pulled while on
        ui.la_probe_set(Some(false));
        assert!(!ui.m.la.on);
        assert!(ui.toast.as_ref().is_some_and(|t| t.0.contains("LA off")));
        // SCPI stays the way out, nothing switches it off again
        ui.m.la.on = true;
        ui.la_probe_set(Some(false));
        assert!(ui.m.la.on);
    }

    #[test]
    fn i2c_on_la_lines() {
        let ui = ui_on(Proto::I2c);
        assert!(ui.m.dec_on_la());
        let p = ui.decode_packets().unwrap();
        // the demo write to 0x50 repeats in the record
        assert!(p.iter().any(|x| matches!(x.kind, Kind::Addr { .. }) && x.value == 0x50), "{:?}", &p[..p.len().min(6)]);
        assert!(p.iter().any(|x| x.kind == Kind::Data && x.value == 0x42));
    }

    #[test]
    fn the_cache_follows_the_settings() {
        let mut ui = ui_on(Proto::I2c);
        let a = ui.decode_packets().unwrap();
        assert_eq!(ui.decode_packets().unwrap(), a);
        // SDA on an idle line: nothing of the I2C traffic left
        ui.m.dec_la[0] = Some(0);
        assert_ne!(ui.decode_packets().unwrap(), a);
    }

    #[test]
    fn la_off_falls_back_to_the_channels() {
        let mut ui = ui_on(Proto::Uart);
        ui.m.la.on = false;
        assert!(!ui.m.dec_on_la());
        assert!(!ui.decode_items().contains(&Item::DecIn));
        assert!(ui.decode_items().contains(&Item::DecThr));
    }

    #[test]
    fn menu_on_la_lines() {
        let mut ui = ui_on(Proto::Spi);
        let items = ui.decode_items();
        assert!(items.contains(&Item::DecIn) && !items.contains(&Item::DecThr));
        assert_eq!(ui.value(Item::DecSrc), "D11");
        assert_eq!(ui.options(Item::DecSrc).len(), 16);
        assert_eq!(ui.options(Item::DecCs).len(), 17);
        ui.select_option(Item::DecSrc, "D3");
        assert_eq!(ui.m.dec_la[0], Some(3));
        ui.select_option(Item::DecCs, "None");
        assert_eq!(ui.m.dec_la[2], None);
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!((back.dec_la_in, back.dec_la), (true, ui.m.dec_la));
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;

    #[test]
    fn scpi_and_csv() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let cv = Canvas::new(W, H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut q = |ui: &mut Ui, c: &str| scpi.exec_line(ui, &cv, c);
        let s = |b: Vec<u8>| String::from_utf8(b).unwrap();
        assert_eq!(s(q(&mut ui, ":LA:STAT?")), "0");
        q(&mut ui, ":LA:STAT ON");
        assert!(ui.m.la.on);
        q(&mut ui, ":LA:DIG3:DISP OFF");
        assert_eq!((ui.m.la.show >> 3 & 1, s(q(&mut ui, ":LA:DIGital3:DISPlay?"))), (0, "0".to_string()));
        q(&mut ui, ":LA:POD2:THR 0.8");
        assert_eq!((ui.m.la.thr(1), ui.m.la.preset[1]), (0.8, USER));
        assert_eq!(s(q(&mut ui, ":LA:POD2:THR?")), "8e-1");
        // out of range and bad nodes: errors, nothing changes
        q(&mut ui, ":LA:POD1:THR 99");
        q(&mut ui, ":LA:DIG16:DISP ON");
        q(&mut ui, ":LA:FOO?");
        assert_eq!(ui.m.la.preset[0], 0);
        assert!(s(q(&mut ui, ":SYST:ERR?")).contains("Illegal"));
        for _ in 0..4 {
            ui.step(0.05);
        }
        let la = ui.la_record().unwrap().clone();
        let pre = s(q(&mut ui, ":LA:PRE?"));
        assert!(pre.starts_with(&format!("{},", la.words.len())), "{}", pre);
        assert_eq!(s(q(&mut ui, ":LA:POIN?")), la.words.len().to_string());
        let d = q(&mut ui, ":LA:DATA?");
        let body = &d[11..];
        assert_eq!(body.len(), la.words.len() * 2);
        assert_eq!(u16::from_le_bytes([body[2], body[3]]), la.words[1]);

        // CSV: changes only, lossless for the lines chosen
        let csv = crate::store::la_csv(&la, &[14]);
        let rows: Vec<&str> = csv.lines().collect();
        assert_eq!(rows[0], "Time (s),D14");
        assert!(rows.len() < 20 && rows.len() > 3, "{} rows", rows.len());
        let edges = la.words.windows(2).filter(|w| (w[0] ^ w[1]) >> 14 & 1 == 1).count();
        assert_eq!(rows.len(), 1 + 1 + edges + 1);
    }
}

#[cfg(test)]
mod trigger_tests {
    use super::*;

    fn ui_la() -> Ui {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.la.on = true;
        ui
    }

    #[test]
    fn source_list_and_stepping() {
        let mut ui = ui_la();
        assert_eq!(ui.options(Item::TrigSrc).len(), n_ch() + 16);
        ui.select_option(Item::TrigSrc, "D14");
        assert_eq!((ui.m.trig_line(), ui.value(Item::TrigSrc)), (Some(14), "D14".to_string()));
        assert!(!ui.trigger_items().contains(&Item::TrigLevel));
        // no analog channel is needed for the trigger
        assert_eq!(ui.m.trig_channels(), [false; NCH]);
        ui.select_option(Item::TrigSrc, "CH2");
        assert_eq!((ui.m.trig_line(), ui.m.trig_src), (None, 1));
        // saved in setups; other types and LA off fall back to the channel
        ui.m.trig_la = Some(3);
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!(back.trig_la, Some(3));
        ui.m.trig_type = TrigType::Runt;
        assert_eq!(ui.m.trig_line(), None);
        assert_eq!(ui.options(Item::TrigSrc).len(), n_ch());
        ui.m.trig_type = TrigType::Edge;
        ui.m.la.on = false;
        assert_eq!(ui.m.trig_line(), None);
    }

    #[test]
    fn demo_aligns_on_the_line() {
        // D14 = the CH1 1 kHz square: rising edge of the line at t = 0 of every record
        let mut ui = ui_la();
        ui.m.trig_la = Some(14);
        ui.m.slope = Slope::Rise;
        for _ in 0..4 {
            ui.step(0.05);
        }
        let la = ui.la_record().unwrap().clone();
        let at = |t: f64| la.words[((t - la.t0) / la.dt) as usize] >> 14 & 1;
        assert_eq!((at(-5e-6), at(5e-6)), (0, 1), "rising edge of D14 at the trigger point");
        assert!(ui.last[0].as_ref().unwrap().triggered);
        ui.m.slope = Slope::Fall;
        for _ in 0..4 {
            ui.step(0.05);
        }
        let la = ui.la_record().unwrap().clone();
        let at = |t: f64| la.words[((t - la.t0) / la.dt) as usize] >> 14 & 1;
        assert_eq!((at(-5e-6), at(5e-6)), (1, 0), "falling edge");
    }

    #[test]
    fn hardware_settings_carry_the_line() {
        let mut m = Model::default();
        m.la.on = true;
        m.trig_la = Some(5);
        assert_eq!(crate::board::dho::hw::settings(&m, false).trig_src, 14);
        m.trig_la = None;
        assert_eq!(crate::board::dho::hw::settings(&m, false).trig_src, 1);
    }

    #[test]
    fn scpi_source() {
        let mut ui = ui_la();
        let cv = Canvas::new(W, H);
        let mut scpi = crate::scpi::Scpi::open(0);
        scpi.exec_line(&mut ui, &cv, ":TRIG:EDGE:SOUR D7");
        assert_eq!(ui.m.trig_line(), Some(7));
        assert_eq!(scpi.exec_line(&mut ui, &cv, ":TRIG:EDGE:SOUR?"), b"D7");
        scpi.exec_line(&mut ui, &cv, ":TRIG:EDGE:SOUR CHAN2");
        assert_eq!((ui.m.trig_line(), ui.m.trig_src), (None, 1));
    }
}

#[cfg(test)]
mod pattern_tests {
    use super::*;

    #[test]
    fn begins_of_a_pattern() {
        let mut p = [Bit::X; 16];
        assert_eq!(pattern_at(&[0, 1, 3], &p, 0), None, "all X");
        (p[0], p[1]) = (Bit::H, Bit::L);
        assert_eq!(pattern_at(&[0, 1, 1, 3, 1], &p, 0), Some(1));
        assert_eq!(pattern_at(&[0, 1, 1, 3, 1], &p, 2), Some(4), "the next beginning");
        assert_eq!(pattern_at(&[1, 1, 1], &p, 0), None, "true from the start: no beginning");
    }

    #[test]
    fn demo_triggers_on_a_counter_value() {
        // D0-D7 count at 100 kHz: 0x5a begins every 2.56 ms; the trigger point must sit on that change
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.la.on = true;
        ui.m.trig_type = TrigType::Pattern;
        ui.m.pat = [Bit::X; NCH];
        for n in 0..8 {
            ui.m.pat_la[n] = if 0x5a >> n & 1 == 1 { Bit::H } else { Bit::L };
        }
        assert!(ui.m.pat_on_la() && ui.m.trig_on_la());
        assert!(ui.trigger_items().contains(&Item::Open(MenuId::LaPat)));
        for _ in 0..4 {
            ui.step(0.05);
        }
        let la = ui.la_record().unwrap().clone();
        let at = |t: f64| la.words[((t - la.t0) / la.dt) as usize] & 0xff;
        assert_eq!((at(-2e-6), at(2e-6)), (0x59, 0x5a));
        assert!(ui.last[0].as_ref().unwrap().triggered);
        let back = crate::store::parse_setup(&crate::store::setup_text(&ui.m)).unwrap();
        assert_eq!(back.pat_la, ui.m.pat_la);
    }

    #[test]
    fn hardware_slots() {
        let mut m = Model::default();
        m.la.on = true;
        m.trig_type = TrigType::Pattern;
        m.pat_la[0] = Bit::H;
        m.pat_la[15] = Bit::L;
        let s = crate::board::dho::hw::settings(&m, false);
        let oos_hal::dho::seq::Trig::Pattern { la, .. } = s.trigger else { panic!() };
        assert_eq!((la[0], la[15], la[1]), (oos_hal::dho::tpu::Pat::H, oos_hal::dho::tpu::Pat::L, oos_hal::dho::tpu::Pat::X));
        m.la.on = false;
        let s = crate::board::dho::hw::settings(&m, false);
        let oos_hal::dho::seq::Trig::Pattern { la, .. } = s.trigger else { panic!() };
        assert!(la.iter().all(|&p| p == oos_hal::dho::tpu::Pat::X));
    }
}

#[cfg(test)]
mod bus_tests {
    use super::*;
    use oos_dsp::decode::Kind;

    #[test]
    fn protocol_trigger_on_la_lines() {
        // I2C on D12/D13: trigger on the data byte 0x42, the field ends at the trigger point
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.la.on = true;
        ui.m.dec_on = true;
        ui.set_dec_proto(Proto::I2c);
        ui.m.dec_la_in = true;
        (ui.m.trig_type, ui.m.trig_bus, ui.m.trig_bus_val) = (TrigType::Bus, BusWhat::Data, 0x42);
        ui.m.tscale = tscales().iter().position(|&t| t >= 20e-6).unwrap();
        assert!(ui.m.bus_on_la() && ui.m.trig_on_la());
        assert_eq!(ui.m.trig_channels(), [false; NCH]);
        for _ in 0..4 {
            ui.step(0.05);
        }
        assert!(ui.last[0].as_ref().unwrap().triggered);
        let p = ui.decode_packets().unwrap();
        let t = p.iter().filter(|x| x.kind == Kind::Data && x.value == 0x42).map(|x| x.t1.abs()).fold(f64::MAX, f64::min);
        // within two LA samples
        let dl = ui.la_record().unwrap().dt;
        assert!(t < 3.0 * dl, "field end {} s from the trigger, LA sample {}", t, dl);
    }

    #[test]
    fn hardware_edge_on_the_frame_line() {
        let mut m = Model::default();
        m.la.on = true;
        m.dec_la_in = true;
        m.trig_type = TrigType::Bus;
        (m.dec_proto, m.dec_la) = (Proto::Spi, [Some(11), Some(10), Some(9), None]);
        assert_eq!(crate::board::dho::hw::settings(&m, false).trig_src, 9 + 9);
        (m.dec_proto, m.dec_la) = (Proto::Uart, [Some(8), None, None, None]);
        assert_eq!(crate::board::dho::hw::settings(&m, false).trig_src, 9 + 8);
        m.dec_la_in = false;
        assert!(crate::board::dho::hw::settings(&m, false).trig_src <= 4);
    }
}

#[cfg(test)]
mod duration_tests {
    use super::*;

    #[test]
    fn duration_on_a_line() {
        // D14 = the 1 kHz square, high for 500 us: "longer than 300 us" fires 300 us after the rising edge
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.la.on = true;
        ui.m.trig_type = TrigType::Duration;
        ui.m.pat = [Bit::X; NCH];
        ui.m.pat_la[14] = Bit::H;
        (ui.m.trig_when, ui.m.trig_t1) = (WhenKind::Gt, 300e-6);
        assert!(ui.m.pat_on_la());
        assert!(ui.trigger_items().contains(&Item::Open(MenuId::LaPat)));
        for _ in 0..4 {
            ui.step(0.05);
        }
        assert!(ui.last[0].as_ref().unwrap().triggered);
        let la = ui.la_record().unwrap().clone();
        let at = |t: f64| la.words[((t - la.t0) / la.dt) as usize] >> 14 & 1;
        assert_eq!(at(0.0), 1, "the line is high at the trigger point");
        let s = crate::board::dho::hw::settings(&ui.m, false);
        assert!(matches!(s.trigger, oos_hal::dho::seq::Trig::Duration { la, .. } if la[14] == oos_hal::dho::tpu::Pat::H));
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    /// Roll and scan keep the LA: a D line as the source reaches `seq` together with the LA settings it needs.
    #[test]
    fn slow_time_bases_keep_the_la() {
        let mut m = Model::default();
        m.la.on = true;
        m.trig_la = Some(5);
        m.tscale = tscales().iter().position(|&t| t >= 0.2).unwrap();
        assert_ne!(crate::board::dho::hw::slow_mode(&m), crate::board::Slow::Off);
        let s = crate::board::dho::hw::settings(&m, false);
        assert!(s.la.is_some_and(|l| l.roll) && s.trig_src == 14, "{:?}", s.trig_src);
        let mut st = oos_hal::dho::seq::State::handover(1, None);
        let (steps, _) = st.configure(&s, None, None).unwrap();
        // the LA stream is on and the record is not doubled
        let wr: Vec<_> = steps.iter().filter_map(|x| if let oos_hal::dho::seq::Step::Wr(w) = x { Some(*w) } else { None }).collect();
        assert!(wr.iter().rev().find(|w| w.0 == 0x4008).unwrap().1 & (1 << 24 | 1 << 26) == 1 << 24 | 1 << 26);
    }

    #[test]
    fn newest_la_word_at_the_newest_analog_point() {
        // 1000 analog points of 1 ms, pre at 400: the newest is 599 ms after pre; 2000 LA words of 0.5 ms
        let t0 = crate::board::dho::hw::roll_la_t0(1000, 400, 1e-3, 2000, 0.5e-3);
        assert!((t0 + 1999.0 * 0.5e-3 - 0.599).abs() < 1e-9);
    }

    /// The channel the trigger reads is acquired hidden, with the LA on or off.
    #[test]
    fn hidden_trigger_channel() {
        let mut m = Model::default();
        m.la.on = true;
        m.ch[0].on = false;
        m.trig_src = 1;
        let s = crate::board::dho::hw::settings(&m, false);
        assert_eq!((s.mask, s.trig_src), (2, 2));
        m.la.on = false;
        assert_eq!(crate::board::dho::hw::settings(&m, false).mask, 2);
    }
}
