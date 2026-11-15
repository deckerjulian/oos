// SPDX-License-Identifier: GPL-2.0-or-later
//! Per-measurement settings of the readings that need no new analysis: which edge starts a cycle, which cycles count,
//! a manual overshoot reference, burst and N-period options, and the edge pairing of delay, phase, skew, setup and
//! hold (from edge, to edge, search direction, window). Every default gives the value the plain functions give.
use super::*;

/// Which edges start a cycle: those of the first mid crossing in the record, rising ones, or falling ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EdgePol {
    #[default]
    First,
    Rise,
    Fall,
}

/// The mid crossings that an N-period duration counts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NEdge {
    #[default]
    Rise,
    Fall,
    Both,
}

/// Settings of one single-source reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowOpts {
    pub pol: EdgePol,
    /// cycles skipped at the start, and then every `step`-th cycle counts
    pub offset: u32,
    pub step: u32,
    /// manual overshoot reference (top, base) in volts; None follows the measured top / base
    pub os_top: Option<f64>,
    pub os_base: Option<f64>,
    pub idle: f64,
    pub burst_falling: bool,
    pub side: Outside,
    pub n: u32,
    pub n_edge: NEdge,
    pub n_offset: u32,
    pub n_step: u32,
}

impl Default for RowOpts {
    fn default() -> Self {
        RowOpts { pol: EdgePol::First, offset: 0, step: 1, os_top: None, os_base: None, idle: 0.0, burst_falling: false,
            side: Outside::Above, n: 6, n_edge: NEdge::Rise, n_offset: 0, n_step: 1 }
    }
}

/// Every `step`-th item after the first `offset`.
fn pick(v: &[f64], offset: u32, step: u32) -> Vec<f64> {
    v.iter().skip(offset as usize).step_by(step.max(1) as usize).copied().collect()
}

/// Mean time from a mid crossing to the one `n` crossings later, over the crossings `edge` selects, starting after
/// `offset` windows and taking every `step`-th.
pub fn duration_n_row(a: &Analysis, n: usize, edge: NEdge, offset: u32, step: u32) -> f64 {
    if n == 0 {
        return f64::NAN;
    }
    let e = match edge {
        NEdge::Rise => a.rises.clone(),
        NEdge::Fall => a.falls.clone(),
        NEdge::Both => mids(a),
    };
    if e.len() <= n {
        return f64::NAN;
    }
    mean(&pick(&e.windows(n + 1).map(|w| w[n] - w[0]).collect::<Vec<_>>(), offset, step))
}

impl Analysis {
    fn polarity_edges(&self, pol: EdgePol) -> &[f64] {
        match pol {
            EdgePol::First => self.cycle_edges(),
            EdgePol::Rise => &self.rises,
            EdgePol::Fall => &self.falls,
        }
    }

    /// The cycle periods of the edges `o.pol` selects, after the offset and step.
    pub fn periods_row(&self, o: &RowOpts) -> Vec<f64> {
        let p: Vec<f64> = self.polarity_edges(o.pol).windows(2).map(|w| w[1] - w[0]).collect();
        pick(&p, o.offset, o.step)
    }

    /// A reading with the settings of one row. Readings with no setting here are `get`.
    pub fn get_row(&self, m: Meas, o: &RowOpts) -> f64 {
        if !self.valid {
            return f64::NAN;
        }
        match m {
            Meas::Period => mean(&self.periods_row(o)),
            Meas::Freq => mean(&self.periods_row(o).iter().map(|p| 1.0 / p).collect::<Vec<_>>()),
            Meas::PWidth => mean(&pick(&self.width_list(true), o.offset, o.step)),
            Meas::NWidth => mean(&pick(&self.width_list(false), o.offset, o.step)),
            Meas::PDuty | Meas::NDuty => {
                let rising = o.pol != EdgePol::Fall;
                // a cycle that starts falling reports its low part first
                let d: Vec<f64> = self.duty_list(rising).into_iter().map(|x| if rising { x } else { 100.0 - x }).collect();
                let pos = mean(&pick(&d, o.offset, o.step));
                if m == Meas::PDuty { pos } else { 100.0 - pos }
            }
            Meas::Overshoot | Meas::Preshoot => {
                let (top, base) = (o.os_top.unwrap_or(self.top), o.os_base.unwrap_or(self.base));
                let amp = top - base;
                if !(amp > 0.0) {
                    return f64::NAN;
                }
                if m == Meas::Overshoot { (self.vmax - top) / amp * 100.0 } else { (base - self.vmin) / amp * 100.0 }
            }
            Meas::Burst => burst_width_pol(self, o.idle, !o.burst_falling),
            Meas::Outside => time_outside(self, o.side),
            Meas::NPeriods => duration_n_row(self, o.n.max(1) as usize, o.n_edge, o.n_offset, o.n_step),
            _ => self.get(m),
        }
    }
}

// ------------------------------------------------------------------------------------------------ edge pairs

/// The edge of the first source of a pair (the clock of setup and hold).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeSel {
    Rise,
    Fall,
    Both,
}

/// The edge of the second source: a direction, either, or the same or the opposite direction as the first one's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToSel {
    Rise,
    Fall,
    Both,
    Same,
    Opposite,
}

/// Search from the start of the record, or from its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchDir {
    #[default]
    First,
    Last,
}

/// How the edges of a pair are chosen. `min` / `max` limit the time from the first edge to the second one;
/// None keeps the default of the reading (skew +/-10 ns, setup and hold 0 to 10 ns, delay unlimited).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairOpts {
    pub from: EdgeSel,
    pub to: ToSel,
    pub dir: SearchDir,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl PairOpts {
    /// The edge flags of `Pair`: true = rising.
    pub fn rise_fall(a: bool, b: bool) -> PairOpts {
        PairOpts {
            from: if a { EdgeSel::Rise } else { EdgeSel::Fall },
            to: if b { ToSel::Rise } else { ToSel::Fall },
            dir: SearchDir::First,
            min: None,
            max: None,
        }
    }
}

impl From<Pair> for PairOpts {
    fn from(p: Pair) -> PairOpts {
        PairOpts::rise_fall(p.0, p.1)
    }
}

fn edges_of(a: &Analysis, s: EdgeSel) -> Vec<f64> {
    match s {
        EdgeSel::Rise => a.rises.clone(),
        EdgeSel::Fall => a.falls.clone(),
        EdgeSel::Both => mids(a),
    }
}

/// The edge lists to pair. From Both with Same / Opposite pairs rises with rises (falls) and falls with falls
/// (rises): two passes whose results are merged.
fn typed_pairs(a: &Analysis, b: &Analysis, o: &PairOpts) -> Vec<(Vec<f64>, Vec<f64>)> {
    let to_edges = |s: ToSel| match s {
        ToSel::Rise => b.rises.clone(),
        ToSel::Fall => b.falls.clone(),
        _ => mids(b),
    };
    match (o.from, o.to) {
        (EdgeSel::Both, ToSel::Same) => vec![(a.rises.clone(), b.rises.clone()), (a.falls.clone(), b.falls.clone())],
        (EdgeSel::Both, ToSel::Opposite) => vec![(a.rises.clone(), b.falls.clone()), (a.falls.clone(), b.rises.clone())],
        (EdgeSel::Both, to) => vec![(mids(a), to_edges(to))],
        (from, ToSel::Same) => vec![(edges_of(a, from), edges_of(b, from))],
        (from, ToSel::Opposite) => {
            let other = if from == EdgeSel::Rise { EdgeSel::Fall } else { EdgeSel::Rise };
            vec![(edges_of(a, from), edges_of(b, other))]
        }
        (from, to) => vec![(edges_of(a, from), to_edges(to))],
    }
}

/// Time from the first source's first (or last) selected edge to the second source's first (or last) selected edge,
/// seconds (negative when the second comes first). With a window, only edges of the second source whose time from
/// the first edge is inside it count.
pub fn delay_opts(a: &Analysis, b: &Analysis, o: &PairOpts) -> f64 {
    let from: Vec<(f64, bool)> = edge_seq(a)
        .into_iter()
        .filter(|e| match o.from {
            EdgeSel::Rise => e.1,
            EdgeSel::Fall => !e.1,
            EdgeSel::Both => true,
        })
        .collect();
    let first = match o.dir {
        SearchDir::First => from.first(),
        SearchDir::Last => from.last(),
    };
    let Some(&(tf, rising)) = first else { return f64::NAN };
    let (lo, hi) = (o.min.unwrap_or(f64::NEG_INFINITY), o.max.unwrap_or(f64::INFINITY));
    let mut to = edge_seq(b)
        .into_iter()
        .filter(|e| match o.to {
            ToSel::Rise => e.1,
            ToSel::Fall => !e.1,
            ToSel::Both => true,
            ToSel::Same => e.1 == rising,
            ToSel::Opposite => e.1 != rising,
        })
        .filter(|e| e.0 - tf >= lo && e.0 - tf <= hi);
    let t = match o.dir {
        SearchDir::First => to.next(),
        SearchDir::Last => to.last(),
    };
    t.map_or(f64::NAN, |(tt, _)| tt - tf)
}

/// Mean phase of the second source against the first one's cycles, degrees.
pub fn phase_opts(a: &Analysis, b: &Analysis, o: &PairOpts) -> f64 {
    let d: Vec<f64> = typed_pairs(a, b, o).iter().flat_map(|(ea, eb)| phase_values(ea, eb)).collect();
    mean(&d)
}

/// Mean skew of the paired edges, limited to the window (default `SKEW_WINDOW`).
pub fn skew_opts(a: &Analysis, b: &Analysis, o: &PairOpts) -> f64 {
    let (min, max) = (o.min.unwrap_or(SKEW_WINDOW.0), o.max.unwrap_or(SKEW_WINDOW.1));
    let d: Vec<f64> = typed_pairs(a, b, o).iter().flat_map(|(ea, eb)| skew_values(ea, eb, min, max)).collect();
    mean(&d)
}

fn latch_of(s: EdgeSel) -> Latch {
    match s {
        EdgeSel::Rise => Latch::Rise,
        EdgeSel::Fall => Latch::Fall,
        EdgeSel::Both => Latch::Both,
    }
}

/// The data edges of setup and hold: the clock's direction for Same, the other one for Opposite.
fn data_edges(clock: EdgeSel, data: &Analysis, to: ToSel) -> Vec<f64> {
    match (to, clock) {
        (ToSel::Rise, _) | (ToSel::Same, EdgeSel::Rise) | (ToSel::Opposite, EdgeSel::Fall) => data.rises.clone(),
        (ToSel::Fall, _) | (ToSel::Same, EdgeSel::Fall) | (ToSel::Opposite, EdgeSel::Rise) => data.falls.clone(),
        _ => mids(data),
    }
}

/// Setup and hold times of the clock edges `o.from` selects against the data edges `o.to` selects, inside the window
/// (default `SETUP_HOLD_WINDOW`).
pub fn setup_hold_opts(clock: &Analysis, data: &Analysis, o: &PairOpts) -> (Vec<f64>, Vec<f64>) {
    let (min, max) = (o.min.unwrap_or(SETUP_HOLD_WINDOW.0), o.max.unwrap_or(SETUP_HOLD_WINDOW.1));
    setup_hold_core(&latch_of(o.from).edges(clock), &data_edges(o.from, data, o.to), min, max)
}

pub fn setup_time_opts(clock: &Analysis, data: &Analysis, o: &PairOpts) -> f64 {
    mean(&setup_hold_opts(clock, data, o).0)
}

pub fn hold_time_opts(clock: &Analysis, data: &Analysis, o: &PairOpts) -> f64 {
    mean(&setup_hold_opts(clock, data, o).1)
}

/// Clock edges (as `o.from` selects them) whose setup is below `tsetup` or hold below `thold`.
pub fn setup_hold_violations_opts(clock: &Analysis, data: &Analysis, tsetup: f64, thold: f64, o: &PairOpts) -> f64 {
    setup_hold_violations_on(clock, data, tsetup, thold, latch_of(o.from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn an(rises: &[f64], falls: &[f64]) -> Analysis {
        Analysis { valid: true, top: 1.0, base: 0.0, vmax: 1.1, vmin: -0.1, rises: rises.to_vec(), falls: falls.to_vec(), t_end: 1e-3, ..Default::default() }
    }

    #[test]
    fn defaults_equal_the_plain_readings() {
        let a = an(&[1.0, 3.0, 5.0, 7.0], &[2.0, 4.0, 6.0]);
        let b = an(&[1.4, 3.4, 5.4], &[2.4, 4.4, 6.4]);
        let o = RowOpts::default();
        for m in [Meas::Period, Meas::Freq, Meas::PWidth, Meas::NWidth, Meas::PDuty, Meas::NDuty, Meas::Overshoot, Meas::Preshoot, Meas::Burst, Meas::Outside, Meas::NPeriods] {
            let (x, y) = (a.get(m), a.get_row(m, &o));
            assert!(x == y || (x.is_nan() && y.is_nan()), "{m:?} {x} {y}");
        }
        for (ra, rb) in [(true, true), (false, false), (true, false), (false, true)] {
            let p = Pair(ra, rb);
            let o = PairOpts::from(p);
            assert_eq!(delay(&a, &b, p).to_bits(), delay_opts(&a, &b, &o).to_bits());
            assert_eq!(phase(&a, &b, p).to_bits(), phase_opts(&a, &b, &o).to_bits());
            assert_eq!(skew(&a, &b, p).to_bits(), skew_opts(&a, &b, &o).to_bits());
        }
    }

    #[test]
    fn polarity_offset_and_step_choose_the_cycles() {
        // rising edges every 2 s but the falling ones every 3 s
        let a = an(&[0.0, 2.0, 4.0, 6.0, 8.0], &[1.0, 4.0, 7.0, 10.0]);
        let mut o = RowOpts::default();
        o.pol = EdgePol::Rise;
        assert_eq!(a.get_row(Meas::Period, &o), 2.0);
        o.pol = EdgePol::Fall;
        assert_eq!(a.get_row(Meas::Period, &o), 3.0);
        assert!((a.get_row(Meas::Freq, &o) - 1.0 / 3.0).abs() < 1e-12);
        // the first mid crossing is a rise: First follows the rises
        o.pol = EdgePol::First;
        assert_eq!(a.get_row(Meas::Period, &o), 2.0);
        // a record that starts falling follows the falls
        let b = an(&[2.0, 5.0, 8.0], &[0.5, 3.5, 6.5]);
        assert_eq!(b.get_row(Meas::Period, &RowOpts::default()), 3.0);
        // offset and step on varying periods 1, 2, 4, 8
        let c = an(&[0.0, 1.0, 3.0, 7.0, 15.0], &[]);
        let mut o = RowOpts::default();
        o.offset = 1;
        assert_eq!(c.get_row(Meas::Period, &o), (2.0 + 4.0 + 8.0) / 3.0);
        o.step = 2;
        assert_eq!(c.get_row(Meas::Period, &o), (2.0 + 8.0) / 2.0);
        o.offset = 0;
        assert_eq!(c.get_row(Meas::Period, &o), (1.0 + 4.0) / 2.0);
    }

    #[test]
    fn duty_follows_the_polarity_that_starts_the_cycle() {
        // high 1 of 4 s
        let a = an(&[0.0, 4.0, 8.0, 12.0], &[1.0, 5.0, 9.0]);
        let mut o = RowOpts::default();
        assert_eq!(a.get_row(Meas::PDuty, &o), 25.0);
        assert_eq!(a.get_row(Meas::NDuty, &o), 75.0);
        o.pol = EdgePol::Fall;
        assert_eq!(a.get_row(Meas::PDuty, &o), 25.0, "the same signal from the other edge");
    }

    #[test]
    fn manual_overshoot_reference_replaces_the_measured_levels() {
        let a = an(&[], &[]);
        assert!((a.get_row(Meas::Overshoot, &RowOpts::default()) - 10.0).abs() < 1e-9);
        let o = RowOpts { os_top: Some(0.9), os_base: Some(0.0), ..Default::default() };
        assert!((a.get_row(Meas::Overshoot, &o) - (1.1 - 0.9) / 0.9 * 100.0).abs() < 1e-9);
        let o = RowOpts { os_top: Some(1.0), os_base: Some(0.1), ..Default::default() };
        assert!((a.get_row(Meas::Preshoot, &o) - (0.1 + 0.1) / 0.9 * 100.0).abs() < 1e-9);
        let bad = RowOpts { os_top: Some(0.0), os_base: Some(1.0), ..Default::default() };
        assert!(a.get_row(Meas::Overshoot, &bad).is_nan());
    }

    #[test]
    fn burst_polarity_and_n_period_options() {
        // two bursts on a clock: edges 1 us apart inside, 100 us idle between
        let rises = [100e-6, 101e-6, 102e-6, 300e-6, 301e-6, 302e-6];
        let falls = [100.5e-6, 101.5e-6, 102.5e-6, 300.5e-6, 301.5e-6, 302.5e-6];
        let a = an(&rises, &falls);
        let rise = a.get_row(Meas::Burst, &RowOpts { idle: 50e-6, ..Default::default() });
        assert!((rise - 2.5e-6).abs() < 1e-12, "{rise}");
        // a burst that starts on a falling edge needs a falling edge after an idle gap: the first one is at 100.5 us,
        // only 0.5 us after the rise, so the burst starts at the second pair, never after a long gap
        let fall = a.get_row(Meas::Burst, &RowOpts { idle: 50e-6, burst_falling: true, ..Default::default() });
        assert!(fall.is_nan() || fall != rise, "{fall}");
        let both = RowOpts { n: 1, n_edge: NEdge::Both, ..Default::default() };
        assert!((a.get_row(Meas::NPeriods, &both) - duration_n(&a, 1, true)).abs() < 1e-18);
        let fl = RowOpts { n: 1, n_edge: NEdge::Fall, ..Default::default() };
        assert!((a.get_row(Meas::NPeriods, &fl) - mean(&a.falls.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>())).abs() < 1e-18);
        // rising windows of N = 1: 1, 1, 198, 1, 1 us
        let skip = RowOpts { n: 1, n_offset: 3, ..Default::default() };
        assert!((a.get_row(Meas::NPeriods, &skip) - 1e-6).abs() < 1e-12, "{}", a.get_row(Meas::NPeriods, &skip));
        let step = RowOpts { n: 1, n_step: 2, ..Default::default() };
        // windows 0, 2, 4 of 1, 1, 198, 1, 1: 1, 198, 1
        assert!((a.get_row(Meas::NPeriods, &step) - 200e-6 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn delay_from_to_direction_and_window() {
        let a = an(&[1.0, 5.0, 9.0], &[3.0, 7.0, 11.0]);
        let b = an(&[2.0, 6.0, 10.0], &[4.0, 8.0, 12.0]);
        let mk = |from, to, dir| PairOpts { from, to, dir, min: None, max: None };
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Rise, ToSel::Rise, SearchDir::First)), 1.0);
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Rise, ToSel::Fall, SearchDir::First)), 3.0);
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Fall, ToSel::Rise, SearchDir::First)), -1.0);
        // last: from the last rise (9) to the last rise of B (10)
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Rise, ToSel::Rise, SearchDir::Last)), 1.0);
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Fall, ToSel::Fall, SearchDir::Last)), 1.0);
        // Both with Same / Opposite: the direction of the chosen first edge (a rise at 1)
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Both, ToSel::Same, SearchDir::First)), 1.0);
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Both, ToSel::Opposite, SearchDir::First)), 3.0);
        // Both / Both: the first edge of each source
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Both, ToSel::Both, SearchDir::First)), 1.0);
        // from the last edge (a fall at 11) the opposite edge of B is the last rise at 10
        assert_eq!(delay_opts(&a, &b, &mk(EdgeSel::Both, ToSel::Opposite, SearchDir::Last)), -1.0);
        // window: only B edges 2.5..3.5 s after the first rise of A (1): the fall at 4 (3 s)
        let w = PairOpts { min: Some(2.5), max: Some(3.5), ..mk(EdgeSel::Rise, ToSel::Both, SearchDir::First) };
        assert_eq!(delay_opts(&a, &b, &w), 3.0);
        let none = PairOpts { min: Some(20.0), ..w };
        assert!(delay_opts(&a, &b, &none).is_nan());
        assert!(delay_opts(&an(&[], &[]), &b, &w).is_nan());
    }

    #[test]
    fn skew_window_and_pass_merge() {
        // B follows A by 2 ns on rises and 4 ns on falls
        let a = an(&[10e-9, 110e-9, 210e-9], &[60e-9, 160e-9, 260e-9]);
        let b = an(&[12e-9, 112e-9, 212e-9], &[64e-9, 164e-9, 264e-9]);
        let o = |from, to| PairOpts { from, to, dir: SearchDir::First, min: None, max: None };
        assert!((skew_opts(&a, &b, &o(EdgeSel::Rise, ToSel::Rise)) - 2e-9).abs() < 1e-15);
        assert!((skew_opts(&a, &b, &o(EdgeSel::Fall, ToSel::Fall)) - 4e-9).abs() < 1e-15);
        // both passes merged: (2 + 4) / 2
        assert!((skew_opts(&a, &b, &o(EdgeSel::Both, ToSel::Same)) - 3e-9).abs() < 1e-15);
        // opposite: a rise of A to a fall of B 54 ns later is outside the +-10 ns window; nothing pairs
        assert!(skew_opts(&a, &b, &o(EdgeSel::Both, ToSel::Opposite)).is_nan());
        // a wider window finds a pair
        let wide = PairOpts { min: Some(-1e-6), max: Some(1e-6), ..o(EdgeSel::Rise, ToSel::Fall) };
        assert!(skew_opts(&a, &b, &wide).is_finite());
        // a window that excludes the 2 ns rise skew
        let narrow = PairOpts { min: Some(3e-9), max: Some(10e-9), ..o(EdgeSel::Rise, ToSel::Rise) };
        assert!(skew_opts(&a, &b, &narrow).is_nan());
    }

    #[test]
    fn setup_and_hold_select_clock_and_data_edges_and_the_window() {
        // clock rises at 10, 20, 30 ns; data changes 3 ns before (setup) and 2 ns after rising clock edges
        let clock = an(&[10e-9, 20e-9, 30e-9], &[15e-9, 25e-9]);
        let data = an(&[7e-9, 27e-9], &[12e-9, 22e-9, 33e-9]);
        let d = PairOpts { from: EdgeSel::Rise, to: ToSel::Both, dir: SearchDir::First, min: None, max: None };
        let (s, h) = setup_hold_opts(&clock, &data, &d);
        let (ps, ph) = setup_hold_times(&clock, &data, Latch::Rise);
        assert_eq!((s.clone(), h.clone()), (ps, ph), "the default is the plain pairing");
        // data rises only
        let r = PairOpts { to: ToSel::Rise, ..d };
        let (s, h) = setup_hold_opts(&clock, &data, &r);
        assert!(!s.is_empty() && (s[0] - 3e-9).abs() < 1e-15, "{s:?}");
        assert!(h.iter().all(|x| *x >= 0.0));
        // a window of 0..2 ns excludes the 3 ns setup
        let w = PairOpts { max: Some(2e-9), ..r };
        let (s, _) = setup_hold_opts(&clock, &data, &w);
        assert!(s.iter().all(|x| *x <= 2e-9 + 1e-18), "{s:?}");
        // a negative minimum accepts a violation
        let neg = PairOpts { min: Some(-5e-9), ..r };
        let (_, hn) = setup_hold_opts(&clock, &data, &neg);
        let (_, h0) = setup_hold_opts(&clock, &data, &r);
        assert!(hn.len() >= h0.len());
        // Opposite of a rising clock is falling data
        assert_eq!(data_edges(EdgeSel::Rise, &data, ToSel::Opposite), data.falls);
        assert_eq!(data_edges(EdgeSel::Fall, &data, ToSel::Same), data.falls);
        assert!(setup_time_opts(&clock, &data, &d).is_finite() && hold_time_opts(&clock, &data, &d).is_finite());
    }
}
