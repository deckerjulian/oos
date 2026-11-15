// SPDX-License-Identifier: GPL-2.0-or-later
//! The settings card of one measurement row. A gear on the row opens it: a page of the Studio Settings Sheet (44 px
//! rows, at most seven per tab) or a dialog with 40 px controls in Minimal and Default. Every field starts as
//! "Shared": it follows the Measure menu until the row sets its own. The card is made of `Item::RowF` rows that read
//! and write `Model::row_cfg` for the row `meas_cfg` points at (single, pair or catalogue rows).
use super::*;
use super::sheet::SheetTab;
use crate::model::{RowCfg, RowKey};
use super::tools::PairKind;
use oos_dsp::measure::{BaseTop, EdgeSel, PairOpts, SearchDir, ToSel};

/// A field of the card.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RF {
    Source,
    BaseTop,
    OsRef,
    OsTop,
    OsBase,
    Gate,
    Pol,
    Off,
    Step,
    Low,
    High,
    Hyst,
    Used,
    Idle,
    BurstPol,
    N,
    NOff,
    NStep,
    NEdge,
    Side,
    OutVolts,
    From,
    To,
    Dir,
    WMin,
    WMax,
    Reset,
}

/// What the card offers for a row.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Group {
    Amp { overshoot: bool },
    Level,
    Timing { polarity: bool },
    Edges { used: bool },
    Burst,
    NPeriods,
    Outside,
    Pair(PairKind),
}

fn group_of(key: RowKey) -> Option<Group> {
    use oos_dsp::measure::Meas as M;
    Some(match key {
        RowKey::One(k, _) => match k {
            M::Vtop | M::Vbase | M::Vamp | M::Vupper | M::Vmid | M::Vlower => Group::Amp { overshoot: false },
            M::Overshoot | M::Preshoot => Group::Amp { overshoot: true },
            M::Period | M::Freq | M::PDuty | M::NDuty => Group::Timing { polarity: true },
            M::PWidth | M::NWidth => Group::Timing { polarity: false },
            M::Rise | M::Fall => Group::Edges { used: true },
            M::PSlew | M::NSlew | M::HighTime | M::LowTime => Group::Edges { used: false },
            M::Burst => Group::Burst,
            M::NPeriods => Group::NPeriods,
            M::Outside => Group::Outside,
            _ => Group::Level,
        },
        RowKey::Pair(k, _, _) => Group::Pair(tools::PAIRS[k.min(tools::PAIRS.len() - 1)].1),
        RowKey::Cat(..) => return None,
    })
}

const TO_NAMES: [&str; 5] = ["Rise", "Fall", "Both", "Same", "Opposite"];
const FROM_NAMES: [&str; 3] = ["Rise", "Fall", "Both"];

/// 1-2-5 times of a second from 100 ps to 500 us, with both signs and zero.
fn time_ladder(v: f64, d: i32) -> f64 {
    let pos: Vec<f64> = (-10..=-4).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e))).collect();
    let l: Vec<f64> = pos.iter().rev().map(|x| -x).chain(std::iter::once(0.0)).chain(pos.iter().copied()).collect();
    let i = (0..l.len()).min_by(|&a, &b| (l[a] - v).abs().total_cmp(&(l[b] - v).abs())).unwrap_or(0);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}

impl Ui {
    // ------------------------------------------------------------------------------------------------ the row

    /// The key of the row the card is open for.
    pub(super) fn rowf_key(&self) -> Option<RowKey> {
        let index = self.meas_cfg? as usize;
        self.m.row_keys().get(index).copied()
    }

    fn rowf_cfg(&self) -> RowCfg {
        self.rowf_key().and_then(|k| self.m.row_cfg(k)).unwrap_or_default()
    }

    fn rowf_edit(&mut self, f: impl FnOnce(&mut RowCfg)) {
        let Some(key) = self.rowf_key() else { return };
        let mut cfg = self.m.row_cfg(key).unwrap_or_default();
        f(&mut cfg);
        self.m.set_row_cfg(key, cfg);
        self.clear_measurement_statistics();
        self.dirty_map = true;
    }

    /// The pages of the card, in order: name and rows.
    pub(super) fn measrow_plan(&self) -> Vec<(&'static str, Vec<Item>)> {
        let Some(key) = self.rowf_key() else { return Vec::new() };
        let cfg = self.rowf_cfg();
        let it = |f: RF| Item::RowF(f);
        let source = matches!(key, RowKey::One(..)).then_some(it(RF::Source));
        let with = |rows: Vec<Item>| -> Vec<Item> { source.into_iter().chain(rows).collect() };
        let Some(group) = group_of(key) else { return self.measrow_cat_plan(key) };
        match group {
            Group::Amp { overshoot } => {
                let mut rows = vec![it(RF::BaseTop)];
                if overshoot {
                    rows.push(it(RF::OsRef));
                    if cfg.os_top.is_some() || cfg.os_base.is_some() { rows.extend([it(RF::OsTop), it(RF::OsBase)]); }
                }
                rows.push(it(RF::Gate));
                vec![("Amplitude", with(rows))]
            }
            Group::Level => vec![("Gate", with(vec![it(RF::Gate)]))],
            Group::Timing { polarity } => {
                let mut rows = Vec::new();
                if polarity { rows.push(it(RF::Pol)); }
                rows.extend([it(RF::Off), it(RF::Step), it(RF::Gate)]);
                vec![("Cycles", with(rows)), ("Levels", vec![it(RF::Hyst), it(RF::BaseTop)])]
            }
            Group::Edges { used } => {
                let mut rows = vec![it(RF::Low), it(RF::High), it(RF::Hyst)];
                if used { rows.push(it(RF::Used)); }
                rows.extend([it(RF::BaseTop), it(RF::Gate)]);
                vec![("Edges", with(rows))]
            }
            Group::Burst => vec![("Counting", with(vec![it(RF::Idle), it(RF::BurstPol), it(RF::Hyst), it(RF::Gate)]))],
            Group::NPeriods => vec![("Counting", with(vec![it(RF::N), it(RF::NOff), it(RF::NStep), it(RF::NEdge), it(RF::Hyst), it(RF::Gate)]))],
            Group::Outside => vec![("Counting", with(vec![it(RF::Side), it(RF::OutVolts), it(RF::Low), it(RF::High), it(RF::Gate)]))],
            Group::Pair(kind) => {
                let delay = kind == PairKind::Delay;
                let mut rows = vec![it(RF::From), it(RF::To)];
                if delay { rows.push(it(RF::Dir)); }
                rows.extend([it(RF::WMin), it(RF::WMax)]);
                vec![("Edges", rows)]
            }
        }
    }

    /// Every item of the card, for the dialog of Minimal and Default.
    pub(super) fn measrow_items(&self) -> Vec<Item> {
        let mut v: Vec<Item> = self.measrow_plan().into_iter().flat_map(|(_, rows)| rows).collect();
        if !matches!(self.rowf_key(), Some(RowKey::Cat(..))) { v.push(Item::RowF(RF::Reset)); }
        v
    }

    /// The card as a page of the Studio sheet.
    pub(super) fn measrow_sheet_tabs(&self) -> Vec<SheetTab> {
        let cat = matches!(self.rowf_key(), Some(RowKey::Cat(..)));
        self.measrow_plan().into_iter().map(|(name, rows)| SheetTab { name, rows, foot: if cat { Vec::new() } else { vec![Item::RowF(RF::Reset)] } }).collect()
    }

    pub(super) fn measrow_title(&self) -> String {
        match self.meas_cfg {
            Some(i) => format!("{} settings", self.selected_measurement_label(i as usize)),
            None => "Measurement settings".into(),
        }
    }

    // ------------------------------------------------------------------------------------------------ items

    pub(super) fn measrow_kind(it: Item) -> Option<Kind> {
        let Item::RowF(f) = it else { return None };
        Some(match f {
            RF::Reset => Kind::Action,
            RF::Source | RF::BaseTop | RF::OsRef | RF::Gate | RF::Pol | RF::BurstPol | RF::NEdge | RF::Side | RF::From | RF::To | RF::Dir => Kind::Choice,
            RF::Used => Kind::Action,
            _ => Kind::Value,
        })
    }

    pub(super) fn measrow_label(it: Item) -> Option<&'static str> {
        let Item::RowF(f) = it else { return None };
        Some(match f {
            RF::Source => "Source",
            RF::BaseTop => "Base / top",
            RF::OsRef => "Overshoot ref",
            RF::OsTop => "Top ref",
            RF::OsBase => "Base ref",
            RF::Gate => "Gate",
            RF::Pol => "Edge",
            RF::Off => "Cycle offset",
            RF::Step => "Cycle step",
            RF::Low => "Low level",
            RF::High => "High level",
            RF::Hyst => "Hysteresis",
            RF::Used => "Edges used",
            RF::Idle => "Idle",
            RF::BurstPol => "Burst edge",
            RF::N => "N periods",
            RF::NOff => "Offset",
            RF::NStep => "Step",
            RF::NEdge => "Edge type",
            RF::Side => "Outside",
            RF::OutVolts => "Level (V)",
            RF::From => "From edge",
            RF::To => "To edge",
            RF::Dir => "Direction",
            RF::WMin => "Window min",
            RF::WMax => "Window max",
            RF::Reset => "Follow shared",
        })
    }

    /// The dynamic text of the read-only rows.
    pub(super) fn measrow_text(&self, it: Item) -> Option<String> {
        match it {
            Item::RowF(RF::Used) => {
                let RowKey::One(kind, id) = self.rowf_key()? else { return None };
                Some(format!("Edges used: {}", self.meas_edge_coverage_caption(id, kind)))
            }
            _ => None,
        }
    }

    fn rowf_pair_kind(&self) -> Option<PairKind> {
        match self.rowf_key()? { RowKey::Pair(k, ..) => Some(tools::PAIRS[k.min(tools::PAIRS.len() - 1)].1), _ => None }
    }

    /// Setup and hold choose a clock edge and a data edge: no Same or Opposite, no direction.
    fn rowf_is_clocked(&self) -> bool {
        self.rowf_pair_kind().is_some_and(|k| !matches!(k, PairKind::Delay | PairKind::Phase | PairKind::Skew))
    }

    /// The pairing the row has now, and the one its reading has by itself (what Shared means).
    fn rowf_pair(&self) -> Option<(PairOpts, PairOpts)> {
        let RowKey::Pair(k, a, b) = self.rowf_key()? else { return None };
        let (_, pk, ea, eb) = tools::PAIRS[k.min(tools::PAIRS.len() - 1)];
        let shared = match pk {
            PairKind::SetupBoth | PairKind::HoldBoth | PairKind::SetupAndHoldBoth => PairOpts { from: EdgeSel::Both, to: ToSel::Both, dir: SearchDir::First, min: None, max: None },
            PairKind::Setup | PairKind::Hold | PairKind::SetupAndHold => PairOpts { to: ToSel::Both, ..PairOpts::rise_fall(ea, eb) },
            _ => PairOpts::rise_fall(ea, eb),
        };
        Some((self.pair_opts(k, a, b), shared))
    }

    pub(super) fn measrow_value(&self, it: Item) -> Option<String> {
        let Item::RowF(f) = it else { return None };
        let c = self.rowf_cfg();
        let m = &self.m;
        let pct = |x: f64| format!("{:.0}%", x * 100.0);
        let lv = |x: f64| if m.meas_lv.absolute { si_short(x, "V") } else { pct(x) };
        Some(match f {
            RF::Source => match self.rowf_key() { Some(RowKey::One(_, id)) => self.wave_name(id), _ => String::new() },
            RF::BaseTop => match c.base_top { Some(b) => b.label().into(), None => self.rowf_shared_base_top() },
            RF::OsRef => if c.os_top.is_some() || c.os_base.is_some() { "Manual".into() } else { "Auto".into() },
            RF::OsTop => si_short(c.os_top.unwrap_or(1.0), "V"),
            RF::OsBase => si_short(c.os_base.unwrap_or(0.0), "V"),
            RF::Gate => match c.gate_full { None => "Shared".into(), Some(true) => "Full record".into(), Some(false) => "Screen".into() },
            RF::Pol => ["First", "Rising", "Falling"][c.pol.unwrap_or(0).min(2) as usize].into(),
            RF::Off => c.off.unwrap_or(0).to_string(),
            RF::Step => c.step.unwrap_or(1).to_string(),
            RF::Low => c.lo.map_or("Shared".into(), lv),
            RF::High => c.hi.map_or("Shared".into(), lv),
            RF::Hyst => c.hyst.map_or("Shared".into(), |h| if m.meas_lv.absolute { si_short(h, "V") } else { pct(h) }),
            RF::Used => String::new(),
            RF::Idle => match c.idle { None => "Shared".into(), Some(x) if x > 0.0 => si_short(x, "s"), Some(_) => "period".into() },
            RF::BurstPol => if c.burst_fall == Some(true) { "Falling".into() } else { "Rising".into() },
            RF::N => c.n.map_or("Shared".into(), |n| n.to_string()),
            RF::NOff => c.n_off.unwrap_or(0).to_string(),
            RF::NStep => c.n_step.unwrap_or(1).to_string(),
            RF::NEdge => match c.n_edge { None => "Shared".into(), Some(x) => ["Rising", "Falling", "Both"][x.min(2) as usize].into() },
            RF::Side => match c.side { None => "Shared".into(), Some(x) => ["High", "Low", "Both"][x.min(2) as usize].into() },
            RF::OutVolts => c.out_volts.map_or("Reference".into(), |v| si_short(v, "V")),
            RF::From => { let (now, _) = self.rowf_pair()?; FROM_NAMES[match now.from { EdgeSel::Rise => 0, EdgeSel::Fall => 1, _ => 2 }].into() }
            RF::To => { let (now, _) = self.rowf_pair()?; TO_NAMES[match now.to { ToSel::Rise => 0, ToSel::Fall => 1, ToSel::Both => 2, ToSel::Same => 3, _ => 4 }].into() }
            RF::Dir => if self.rowf_pair()?.0.dir == SearchDir::Last { "Last".into() } else { "First".into() },
            RF::WMin => { let (now, _) = self.rowf_pair()?; now.min.or(self.rowf_window_default(true)).map_or("Off".into(), |x| si_short(x, "s")) }
            RF::WMax => { let (now, _) = self.rowf_pair()?; now.max.or(self.rowf_window_default(false)).map_or("Off".into(), |x| si_short(x, "s")) }
            RF::Reset => String::new(),
        })
    }

    fn rowf_shared_base_top(&self) -> String {
        format!("Shared ({})", self.m.meas_lv.base_top.label())
    }

    pub(super) fn measrow_options(&self, it: Item) -> Option<Vec<String>> {
        let Item::RowF(f) = it else { return None };
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Some(match f {
            RF::Source => self.wave_names(),
            RF::BaseTop => std::iter::once(self.rowf_shared_base_top()).chain(BaseTop::ALL.iter().map(|b| b.label().to_string())).collect(),
            RF::OsRef => s(&["Auto", "Manual"]),
            RF::Gate => s(&["Shared", "Screen", "Full record"]),
            RF::Pol => s(&["First", "Rising", "Falling"]),
            RF::BurstPol => s(&["Rising", "Falling"]),
            RF::NEdge => s(&["Shared", "Rising", "Falling", "Both"]),
            RF::Side => s(&["Shared", "High", "Low", "Both"]),
            RF::From => s(&FROM_NAMES),
            RF::To => if self.rowf_is_clocked() { s(&FROM_NAMES) } else { s(&TO_NAMES) },
            RF::Dir => s(&["First", "Last"]),
            _ => return None,
        })
    }

    pub(super) fn measrow_adjust(&mut self, it: Item, d: i32) -> bool {
        let Item::RowF(f) = it else { return false };
        let c = self.rowf_cfg();
        let abs = self.m.meas_lv.absolute;
        let step = |cur: u32, d: i32, min: u32, max: u32| (cur as i64 + d as i64).clamp(min as i64, max as i64) as u32;
        match f {
            RF::Source => self.rowf_source(d),
            RF::BaseTop => {
                // Shared, then the methods
                let all = BaseTop::ALL;
                let cur = c.base_top.map_or(0, |b| 1 + all.iter().position(|&x| x == b).unwrap_or(0)) as i32;
                let n = (all.len() + 1) as i32;
                let next = (cur + d).rem_euclid(n) as usize;
                self.rowf_edit(|c| c.base_top = if next == 0 { None } else { Some(all[next - 1]) });
            }
            RF::OsRef => {
                if d.rem_euclid(2) != 0 {
                    let manual = c.os_top.is_some() || c.os_base.is_some();
                    // Manual starts from the levels the row measures now
                    let (top, base) = match self.rowf_key() {
                        Some(RowKey::One(_, id)) => self.meas_analysis(id).map_or((1.0, 0.0), |(a, _)| (a.top, a.base)),
                        _ => (1.0, 0.0),
                    };
                    self.rowf_edit(|c| if manual { (c.os_top, c.os_base) = (None, None) } else { (c.os_top, c.os_base) = (Some(top), Some(base)) });
                }
            }
            RF::OsTop => self.rowf_edit(|c| c.os_top = Some(tools::ladder(c.os_top.unwrap_or(1.0), d, false))),
            RF::OsBase => self.rowf_edit(|c| c.os_base = Some(tools::ladder(c.os_base.unwrap_or(0.0), d, false))),
            RF::Gate => {
                let cur = match c.gate_full { None => 0, Some(false) => 1, Some(true) => 2 };
                let next = (cur + d).rem_euclid(3);
                self.rowf_edit(|c| c.gate_full = match next { 0 => None, 1 => Some(false), _ => Some(true) });
            }
            RF::Pol => {
                let next = (c.pol.unwrap_or(0) as i32 + d).rem_euclid(3) as u8;
                self.rowf_edit(|c| c.pol = (next != 0).then_some(next));
            }
            RF::Off => self.rowf_edit(|c| { let v = step(c.off.unwrap_or(0), d, 0, 1000); c.off = (v != 0).then_some(v); }),
            RF::Step => self.rowf_edit(|c| { let v = step(c.step.unwrap_or(1), d, 1, 1000); c.step = (v != 1).then_some(v); }),
            RF::Low | RF::High => {
                let low = f == RF::Low;
                let sh = &self.m;
                let shared = if abs { sh.meas_lv.volts[if low { 0 } else { 2 }] } else if low { sh.meas_lpct } else { sh.meas_hpct };
                let cur = if low { c.lo } else { c.hi }.unwrap_or(shared);
                let new = if abs { tools::ladder(cur, d, false) } else { ((cur * 100.0).round() + d as f64) / 100.0 };
                let (lo, mid, hi) = if abs { (sh.meas_lv.volts[0], sh.meas_lv.volts[1], sh.meas_lv.volts[2]) } else { (sh.meas_lpct, sh.meas_lv.mid, sh.meas_hpct) };
                let (lo, hi) = (if low { new } else { c.lo.unwrap_or(lo) }, if low { c.hi.unwrap_or(hi) } else { new });
                let ordered = lo < mid && mid < hi && (abs || (lo >= 0.01 && hi <= 0.99));
                if ordered {
                    self.rowf_edit(|c| { let v = (new - shared).abs() > 1e-9; if low { c.lo = v.then_some(new) } else { c.hi = v.then_some(new) } });
                }
            }
            RF::Hyst => {
                let shared = if abs { self.m.meas_lv.vhyst } else { self.m.meas_lv.hyst };
                let cur = c.hyst.unwrap_or(shared);
                let new = if abs { tools::ladder(cur, d, true).max(1e-4) } else { (((cur * 100.0).round() + d as f64).clamp(1.0, 50.0)) / 100.0 };
                self.rowf_edit(|c| c.hyst = ((new - shared).abs() > 1e-9).then_some(new));
            }
            RF::Idle => {
                let cur = c.idle.unwrap_or(self.m.meas_idle);
                let l: Vec<f64> = std::iter::once(0.0).chain((0..10).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e - 9)))).collect();
                let i = l.iter().position(|&v| v >= cur * 0.999).unwrap_or(0);
                let v = l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize];
                let shared = self.m.meas_idle;
                self.rowf_edit(|c| c.idle = ((v - shared).abs() > 1e-15).then_some(v));
            }
            RF::BurstPol => if d.rem_euclid(2) != 0 { self.rowf_edit(|c| c.burst_fall = if c.burst_fall == Some(true) { None } else { Some(true) }) },
            RF::N => {
                let v = step(c.n.unwrap_or(self.m.meas_n), d, 1, 32);
                let shared = self.m.meas_n;
                self.rowf_edit(|c| c.n = (v != shared).then_some(v));
            }
            RF::NOff => self.rowf_edit(|c| { let v = step(c.n_off.unwrap_or(0), d, 0, 1000); c.n_off = (v != 0).then_some(v); }),
            RF::NStep => self.rowf_edit(|c| { let v = step(c.n_step.unwrap_or(1), d, 1, 1000); c.n_step = (v != 1).then_some(v); }),
            RF::NEdge => {
                let cur = c.n_edge.map_or(0, |x| x as i32 + 1);
                let next = (cur + d).rem_euclid(4);
                self.rowf_edit(|c| c.n_edge = (next != 0).then(|| (next - 1) as u8));
            }
            RF::Side => {
                let cur = c.side.map_or(0, |x| x as i32 + 1);
                let next = (cur + d).rem_euclid(4);
                self.rowf_edit(|c| c.side = (next != 0).then(|| (next - 1) as u8));
            }
            RF::OutVolts => {
                // from the reference: the row's upper level
                let start = match self.rowf_key() {
                    Some(RowKey::One(_, id)) => self.meas_analysis(id).map_or(0.0, |(a, _)| a.get(oos_dsp::measure::Meas::Vupper)),
                    _ => 0.0,
                };
                let cur = c.out_volts.unwrap_or(start);
                let v = if c.out_volts.is_some() { tools::ladder(cur, d, false) } else { tools::ladder(cur, 0, false) };
                self.rowf_edit(|c| c.out_volts = Some(v));
            }
            RF::From | RF::To | RF::Dir | RF::WMin | RF::WMax => return self.rowf_adjust_pair(f, d),
            RF::Used | RF::Reset => {}
        }
        true
    }

    fn rowf_adjust_pair(&mut self, f: RF, d: i32) -> bool {
        let Some((now, shared)) = self.rowf_pair() else { return true };
        let code_from = |s: EdgeSel| match s { EdgeSel::Rise => 0u8, EdgeSel::Fall => 1, EdgeSel::Both => 2 };
        let code_to = |s: ToSel| match s { ToSel::Rise => 0u8, ToSel::Fall => 1, ToSel::Both => 2, ToSel::Same => 3, ToSel::Opposite => 4 };
        let clocked = self.rowf_is_clocked();
        match f {
            RF::From => {
                let next = (code_from(now.from) as i32 + d).rem_euclid(3) as u8;
                self.rowf_edit(|c| c.from = (next != code_from(shared.from)).then_some(next));
            }
            RF::To => {
                let n = if clocked { 3 } else { 5 };
                let next = (code_to(now.to) as i32 + d).rem_euclid(n) as u8;
                self.rowf_edit(|c| c.to = (next != code_to(shared.to)).then_some(next));
            }
            RF::Dir => if d.rem_euclid(2) != 0 {
                let last = now.dir != SearchDir::Last;
                self.rowf_edit(|c| c.dir = last.then_some(1));
            },
            RF::WMin | RF::WMax => {
                let low = f == RF::WMin;
                let cur = if low { now.min.or(self.rowf_window_default(true)) } else { now.max.or(self.rowf_window_default(false)) };
                // a window that is off starts at the usual 0 to 10 ns
                let start = if low { 0.0 } else { 10e-9 };
                let new = match cur { Some(x) => time_ladder(x, d), None => start };
                let default = if low { shared.min } else { shared.max };
                let skew_like = default.is_none() && self.rowf_pair_kind() != Some(PairKind::Delay);
                let _ = skew_like;
                // equal to what the reading does by itself: follow
                let base = self.rowf_window_default(low);
                self.rowf_edit(|c| {
                    let v = base.is_none_or(|b| (b - new).abs() > 1e-18).then_some(new);
                    if low { c.wmin = v } else { c.wmax = v }
                });
            }
            _ => {}
        }
        true
    }

    /// The window a pair reading uses by itself: skew +-10 ns, setup and hold 0 to 10 ns, delay and phase none.
    fn rowf_window_default(&self, low: bool) -> Option<f64> {
        match self.rowf_pair_kind()? {
            PairKind::Skew => Some(if low { oos_dsp::measure::SKEW_WINDOW.0 } else { oos_dsp::measure::SKEW_WINDOW.1 }),
            PairKind::Delay | PairKind::Phase => None,
            _ => Some(if low { oos_dsp::measure::SETUP_HOLD_WINDOW.0 } else { oos_dsp::measure::SETUP_HOLD_WINDOW.1 }),
        }
    }

    pub(super) fn measrow_activate(&mut self, it: Item) -> bool {
        match it {
            Item::MeasStatReset => {
                self.clear_measurement_statistics();
                self.toast("Statistics reset");
                true
            }
            Item::RowF(RF::Reset) => {
                self.rowf_edit(|c| *c = RowCfg::default());
                true
            }
            Item::RowF(_) => true,
            _ => false,
        }
    }

    /// Steps the source of a single row through the channels and references, keeping its settings.
    fn rowf_source(&mut self, d: i32) {
        let Some(RowKey::One(kind, id)) = self.rowf_key() else { return };
        let Some(index) = self.meas_cfg.map(|i| i as usize) else { return };
        let mut sources: Vec<WaveId> = (0..n_ch()).map(WaveId::Ch).collect();
        sources.extend((0..NREF).filter(|&i| self.refs[i].is_some()).map(WaveId::Ref));
        if sources.is_empty() { return; }
        let at = sources.iter().position(|s| *s == id).unwrap_or(0);
        let next = sources[(at as i32 + d).rem_euclid(sources.len() as i32) as usize];
        if next == id { return; }
        if self.m.meas.iter().enumerate().any(|(j, e)| j != index && *e == (kind, next)) {
            self.toast("That measurement is already on this source");
            return;
        }
        let cfg = self.m.row_cfg(RowKey::One(kind, id));
        let own = self.meas_own.remove(&(kind, id));
        self.m.meas[index].1 = next;
        self.m.set_row_cfg(RowKey::One(kind, id), RowCfg::default());
        if let Some(cfg) = cfg { self.m.set_row_cfg(RowKey::One(kind, next), cfg); }
        if let Some(own) = own { self.meas_own.insert((kind, next), own); }
        self.clear_measurement_statistics();
    }
}


impl Ui {
    /// The card of a catalogue row (eye, jitter, power): the settings its readings use. They are the shared settings of
    /// the Eye and Power pages, so a change here is the same change there.
    fn measrow_cat_plan(&self, key: RowKey) -> Vec<(&'static str, Vec<Item>)> {
        let RowKey::Cat(id, ..) = key else { return Vec::new() };
        match super::meascatalog::backend::card_group(id) {
            1 => vec![("Jitter", vec![Item::EyeRate, Item::EyeBer, Item::EyePll, Item::EyePnLo, Item::EyePnHi, Item::EyeNotch, Item::EyeHp])],
            2 => vec![("Power", vec![Item::PowFline, Item::PowFsw, Item::PowRef])],
            3 => vec![("Common mode", vec![Item::EyeHp])],
            _ => Vec::new(),
        }
    }
}
