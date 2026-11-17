// SPDX-License-Identifier: GPL-2.0-or-later
//! The range table: for every "- value +" item its lowest and highest value, the step of one press and the unit, in
//! ONE place. `adjust` (the steppers and the knobs), the numeric pad (`numpad.rs`) and the SCPI setters of the same
//! settings all read it, so a limit is never written twice.
//!
//! Some limits depend on the state (V/div and the probe of the source, time/div, the decoder source): `item_range` is a
//! function of the `Ui`, not a list of constants. An item steps one of three ways (`Step`):
//! * `Add(step)`: the value moves by `step` per press, and stops at the limits (no wrap). It moves on the lattice of its
//!   step (`add_step`), so N presses up and N down return to the same value (the Horizontal delay too: its step is time/div / 25).
//! * `Ladder`: the item steps along its own 1-2-5 ladder or its list; `lo` and `hi` are the ends of it.
//! * `Index`: a whole number (a count or a slot), one per press, clamped.
//! A cyclic picker (a source, a type) is not in the table: it has no limits and wraps on purpose. Nor is an item whose
//! text is in other units than its setting (the cursors in time or volts, the SSC deviation in percent): the pad reads
//! the limits in the units it shows.
use super::*;

/// How one press of a stepper moves the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Step {
    Add(f64),
    Ladder,
    Index,
}

/// The limits of an item in base units (volts, seconds, hertz, ...), and how it steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Range {
    pub lo: f64,
    pub hi: f64,
    pub step: Step,
    pub unit: &'static str,
}

// ---------------------------------------------------------------------------------------------------- constants

/// The screen: the limit of a level or a threshold is this many divisions of its source either side of the middle.
pub(super) const LEVEL_DIVS: f64 = 5.0;
/// The delay stays within this many screens of the trigger.
pub(super) const DELAY_SCREENS: f64 = 50.0;
/// Steps of a level or an offset in one division.
pub(super) const STEPS_PER_DIV: f64 = 25.0;
/// Spectrum reference level, dBV.
pub(super) const SPEC_REF_DBV: (f32, f32) = (-200.0, 100.0);
/// Math trace offset, divisions.
pub(super) const MATH_OFFSET_DIV: f32 = 5.0;
pub(super) const INTENSITY: (i32, i32) = (10, 100);
pub(super) const GRID_BRIGHT: (i32, i32) = (10, 100);
pub(super) const STORE_SLOT: (i32, i32) = (1, 9);
/// Magnetic: turns of the winding.
/// Bode generator amplitude, Vpp.
pub(super) const BODE_AMP: (f64, f64) = (0.01, 20.0);
pub(super) const MAG_TURNS: (i32, i32) = (1, 10_000);
pub(super) const CNT_DIGITS: (i32, i32) = (3, 7);
pub(super) const NTH_N: (i32, i32) = (1, 65535);
pub(super) const MATH_LIN: (f32, f32) = (-1e6, 1e6);
pub(super) const SPEC_MARGIN: (f32, f32) = (0.0, 60.0);
pub(super) const SIM_LEVEL: (f64, f64) = (-2.0, 2.0);
pub(super) const SIM_DEEMPH: (f64, f64) = (0.0, 12.0);
pub(super) const SIM_SSC_DEV: (f64, f64) = (0.0, 0.05);
pub(super) const TRIG_BUS_VAL: (i64, i64) = (0, 0x1fff_ffff);
/// Simulator ladders: (lowest, highest, with a zero).
/// Eye bit rate ladder: (lowest, highest, with Auto in front).
pub(super) const EYE_RATE: (f64, f64, bool) = (1.0, 64e9, true);
pub(super) const SIM_RATE: (f64, f64, bool) = (1e8, 64e9, false);
pub(super) const SIM_RISE: (f64, f64, bool) = (5e-12, 2e-9, false);
pub(super) const SIM_BW: (f64, f64, bool) = (10e6, 50e9, false);
pub(super) const SIM_NOISE: (f64, f64, bool) = (100e-6, 50e-3, false);
pub(super) const SIM_SSC_HZ: (f64, f64, bool) = (1e3, 1e6, false);
pub(super) const SIM_DCD: (f64, f64, bool) = (0.5e-12, 100e-12, true);
pub(super) const SIM_CORR_RJ: (f64, f64, bool) = (0.1e-12, 50e-12, true);
pub(super) const SIM_CORR_HZ: (f64, f64, bool) = (1e3, 100e6, false);
pub(super) const SIM_RJ: (f64, f64, bool) = (0.1e-12, 50e-12, false);
pub(super) const SIM_PJ: (f64, f64, bool) = (0.5e-12, 50e-12, true);
pub(super) const SIM_PJ_HZ: (f64, f64, bool) = (1e5, 50e6, false);
/// Eye PLL loop bandwidth, Hz (0 = Auto, the rate / 1667). A loop faster than the edges it follows is not a loop.
pub(super) const EYE_PLL: (f64, f64) = (1e3, crate::model::EYE_PLL_MAX);

/// A press (or `d` presses) of an `Add` stepper: `v` moves by `d` steps and is rounded to a hundredth of a step. The
/// value is stored as a 32-bit float, and a step that a float cannot hold would leave a residue (0 V, up, down: 745 pV);
/// on this lattice N presses up and N down return exactly, from wherever the value started.
pub(super) fn add_step(v: f64, step: f64, d: i32) -> f64 {
    if !(step.is_finite() && step > 0.0) { return v; }
    let q = step / 100.0;
    ((v + d as f64 * step) / q).round() * q
}

impl Ui {
    /// The range of an item, None when it has none (a picker, a toggle, an action).
    pub(super) fn item_range(&self, it: Item) -> Option<Range> {
        let m = &self.m;
        let sym = |r: f64, step: f64, unit| Some(Range { lo: -r, hi: r, step: Step::Add(step), unit });
        let ends = |t: (f64, f64, bool), unit| Some(Range { lo: if t.2 { 0.0 } else { t.0 }, hi: ladder_top(t), step: Step::Ladder, unit });
        let int = |t: (i32, i32), unit| Some(Range { lo: t.0 as f64, hi: t.1 as f64, step: Step::Index, unit });
        let flt = |t: (f32, f32), step: f64, unit| Some(Range { lo: t.0 as f64, hi: t.1 as f64, step: Step::Add(step), unit });
        match it {
            Item::TrigLevel | Item::TrigLevel2 => {
                let vpd = self.trigger_vpd(self.trigger_level_secondary(it)) as f64;
                sym(LEVEL_DIVS * vpd, vpd / STEPS_PER_DIV, "V")
            }
            Item::PatLevel => {
                let vpd = m.ch[m.trig_src.min(NCH - 1)].volts_per_div() as f64;
                sym(LEVEL_DIVS * vpd, vpd / STEPS_PER_DIV, "V")
            }
            Item::ChOffset(c) => sym(m.ch[c].offset_max() as f64, m.ch[c].volts_per_div() as f64 / STEPS_PER_DIV, "V"),
            Item::ChBias(c) => sym(m.ch[c].offset_max() as f64, 0.01 * m.ch[c].volts_per_div() as f64, "V"),
            Item::ChSkew(_) => sym(SKEW_MAX as f64, 600e-12, "s"),
            Item::HDelay => {
                let tdiv = m.time_per_div() as f64;
                sym(DELAY_SCREENS * HDIV as f64 * tdiv, tdiv / STEPS_PER_DIV, "s")
            }
            Item::MathOffset => flt((-MATH_OFFSET_DIV, MATH_OFFSET_DIV), 0.1, "div"),
            Item::DecThr => {
                let vpd = self.decoder_vpd() as f64;
                sym(LEVEL_DIVS * vpd, vpd / STEPS_PER_DIV, "V")
            }
            Item::BusClkThr | Item::BusDataThr => {
                let id = if it == Item::BusClkThr { m.bus.clk } else { m.bus.data };
                let step = self.bus_volt_step(id) as f64;
                sym(LEVEL_DIVS * STEPS_PER_DIV * step, step, "V")
            }
            Item::SpecRef => flt(SPEC_REF_DBV, 5.0, "dBV"),
            Item::RefOffset => {
                let vpd = self.refs.get(self.ref_sel).and_then(|r| r.as_ref()).map_or(1.0, |r| r.vpd as f64);
                sym(LEVEL_DIVS * vpd, 0.1 * vpd, "V")
            }
            Item::MathLinA | Item::MathLinB => flt(MATH_LIN, 0.1, ""),
            Item::SpecMargin => flt(SPEC_MARGIN, 1.0, "dB"),
            Item::SimHigh | Item::SimLow => Some(Range { lo: SIM_LEVEL.0, hi: SIM_LEVEL.1, step: Step::Add(0.05), unit: "V" }),
            Item::SimDeemph => Some(Range { lo: SIM_DEEMPH.0, hi: SIM_DEEMPH.1, step: Step::Add(0.5), unit: "dB" }),
            Item::BusPos => flt((BUS_POS_MIN, BUS_POS_MAX), 0.1, "div"),
            Item::Intensity => int(INTENSITY, "%"),
            Item::GridBright => int(GRID_BRIGHT, "%"),
            Item::StoreSlot => int(STORE_SLOT, ""),
            Item::MagN => int(MAG_TURNS, ""),
            Item::BodeAmp => Some(Range { lo: BODE_AMP.0, hi: BODE_AMP.1, step: Step::Ladder, unit: "Vpp" }),
            Item::CntDigits => int(CNT_DIGITS, ""),
            Item::NthN => int(NTH_N, ""),
            Item::DecBits => int(dec_bits(m.dec_proto), ""),
            Item::VidLine => int(vid_line(m.video_std), ""),
            Item::TrigBusVal => Some(Range { lo: TRIG_BUS_VAL.0 as f64, hi: TRIG_BUS_VAL.1 as f64, step: Step::Index, unit: "" }),
            // slots: a number among the references, not a loop
            Item::SimSlot | Item::RefSel => Some(Range { lo: 0.0, hi: (NREF - 1) as f64, step: Step::Index, unit: "" }),
            Item::SimRate => ends(SIM_RATE, "b/s"),
            Item::SimRise => ends(SIM_RISE, "s"),
            Item::SimBw => ends(SIM_BW, "Hz"),
            Item::SimNoise => ends(SIM_NOISE, "V"),
            Item::SimSscHz => ends(SIM_SSC_HZ, "Hz"),
            Item::SimDcd => ends(SIM_DCD, "s"),
            Item::SimCorrRj => ends(SIM_CORR_RJ, "s"),
            Item::SimCorrHz => ends(SIM_CORR_HZ, "Hz"),
            Item::SimRj => ends(SIM_RJ, "s"),
            Item::SimPj => ends(SIM_PJ, "s"),
            Item::SimPjHz => ends(SIM_PJ_HZ, "Hz"),
            Item::EyeRate => ends(EYE_RATE, "b/s"),
            Item::EyePll => Some(Range { lo: 0.0, hi: EYE_PLL.1, step: Step::Ladder, unit: "Hz" }),
            Item::SpecCenter | Item::SpecSpan | Item::SpecRbw => Some(Range { lo: 0.0, hi: analysis::FREQ_TOP, step: Step::Ladder, unit: "Hz" }),
            _ => None,
        }
    }

    /// The secondary level of a trigger: the second source of Delay and Setup/Hold.
    pub(super) fn trigger_level_secondary(&self, it: Item) -> bool {
        it == Item::TrigLevel2 && matches!(self.m.trig_type, TrigType::Delay | TrigType::SetupHold)
    }

    /// Every item of the table in the present state (one per channel, and so on).
    #[cfg_attr(not(test), allow(dead_code))] // only the tests use it
    pub(super) fn range_items(&self) -> Vec<Item> {
        let mut v = vec![
            Item::TrigLevel, Item::TrigLevel2, Item::PatLevel, Item::HDelay, Item::MathOffset, Item::DecThr, Item::BusClkThr, Item::BusDataThr,
            Item::SpecRef, Item::RefOffset, Item::MathLinA, Item::MathLinB, Item::SpecMargin, Item::SimHigh, Item::SimLow, Item::SimDeemph,
            Item::BusPos, Item::Intensity, Item::GridBright,
            Item::StoreSlot, Item::MagN, Item::CntDigits, Item::NthN, Item::DecBits, Item::VidLine, Item::TrigBusVal, Item::SimSlot,
            Item::RefSel, Item::SimRate, Item::SimRise, Item::SimBw, Item::SimNoise, Item::SimSscHz, Item::SimDcd, Item::SimCorrRj,
            Item::SimCorrHz, Item::SimRj, Item::SimPj, Item::SimPjHz, Item::SpecCenter, Item::SpecSpan, Item::SpecRbw, Item::EyePll, Item::EyeRate, Item::BodeAmp,
        ];
        for c in 0..n_ch() { v.extend([Item::ChOffset(c), Item::ChBias(c), Item::ChSkew(c)]); }
        v
    }

    /// The value of an item that moves by `Step::Add` (base units).
    pub(super) fn range_get(&self, it: Item) -> Option<f64> {
        let m = &self.m;
        Some(match it {
            Item::TrigLevel => m.trig_level as f64,
            Item::TrigLevel2 => m.trig_level2 as f64,
            Item::PatLevel => m.pat_levels[m.trig_src.min(NCH - 1)] as f64,
            Item::ChOffset(c) => m.ch[c].offset as f64,
            Item::ChBias(c) => m.ch[c].bias as f64,
            Item::ChSkew(c) => m.ch[c].skew as f64,
            Item::HDelay => m.delay as f64,
            Item::MathOffset => m.math_offset as f64,
            Item::DecThr => m.dec_thr as f64,
            Item::BusClkThr => m.bus.clk_thr as f64,
            Item::BusDataThr => m.bus.data_thr as f64,
            Item::SpecRef => m.spec_ref as f64,
            Item::RefOffset => self.refs.get(self.ref_sel)?.as_ref()?.offset as f64,
            _ => return None,
        })
    }

    /// Sets an item that moves by `Step::Add` to `v`, limited to its range. Returns the value it has now.
    pub(super) fn range_set(&mut self, it: Item, v: f64) -> Option<f64> {
        let r = self.item_range(it).filter(|r| matches!(r.step, Step::Add(_)))?;
        let v = if v.is_finite() { v.clamp(r.lo, r.hi) } else { return self.range_get(it) };
        let x = v as f32;

        let m = &mut self.m;
        match it {
            // the upper level of a two-level type stops at the lower one, and the lower at the upper
            Item::TrigLevel => m.trig_level = if m.trig_type.two_level() { x.max(m.trig_level2) } else { x },
            Item::TrigLevel2 => m.trig_level2 = if m.trig_type.two_level() { x.min(m.trig_level) } else { x },
            Item::PatLevel => m.pat_levels[m.trig_src.min(NCH - 1)] = x,
            Item::ChOffset(c) => m.ch[c].offset = x,
            Item::ChBias(c) => m.ch[c].bias = x,
            Item::ChSkew(c) => m.ch[c].skew = x,
            Item::HDelay => m.delay = x,
            Item::MathOffset => m.math_offset = x,
            Item::DecThr => m.dec_thr = x,
            Item::BusClkThr => m.bus.clk_thr = x,
            Item::BusDataThr => m.bus.data_thr = x,
            Item::SpecRef => m.spec_ref = x,
            Item::RefOffset => {
                if let Some(Some(r)) = self.refs.get_mut(self.ref_sel) { r.offset = x; } else { return None; }
            }
            _ => return None,
        }
        if matches!(it, Item::TrigLevel | Item::TrigLevel2) { self.replay.reset_search(); }
        self.range_get(it)
    }

    /// One press (or `d` presses) of an `Add` stepper: the value moves by the step of the table and stops at the limits.
    /// True when `it` is one.
    pub(super) fn range_step(&mut self, it: Item, d: i32) -> bool {
        let Some(Range { step: Step::Add(step), .. }) = self.item_range(it) else { return false };
        let Some(v) = self.range_get(it) else { return false };
        let near = |a: f64, b: f64| (a - b).abs() <= step * 1e-3;
        let mut to = add_step(v, step, d);
        // a press back from a limit that the last press ran into returns to where that press came from (the limit is
        // not on the lattice of the value), so a press up into the limit and a press down leave the value where it was
        if let Some((hit, at, from)) = self.clamp_home {
            if hit == it && near(v, at) && (from - at) * d as f64 > 0.0 { to = add_step(from, step, d - d.signum()); self.clamp_home = None; }
        }
        let now = self.range_set(it, to);
        if let (Some(now), Some(r)) = (now, self.item_range(it)) {
            let out = to < r.lo - step * 1e-3 || to > r.hi + step * 1e-3;
            if out && !near(v, now) { self.clamp_home = Some((it, now, v)); }
            else if !out { self.clamp_home = None; }
        }
        true
    }

    /// After any stepper: a value that is outside its range (a setting loaded, a path that did not clamp) comes back in.
    pub(super) fn range_enforce(&mut self, it: Item) {
        let Some(r) = self.item_range(it) else { return };
        if !matches!(r.step, Step::Add(_)) { return; }
        if let Some(v) = self.range_get(it) {
            if v < r.lo || v > r.hi { self.range_set(it, v); }
        }
    }

    /// An SCPI number for an item of the table: limited like the stepper, not refused.
    pub(super) fn scpi_range_set(&mut self, it: Item, v: f64) -> bool {
        self.range_set(it, v).is_some()
    }
}


impl Ui {
    /// The trigger level (the upper one, or the lower one when `secondary`) for the SCPI module.
    pub(crate) fn scpi_trigger_level(&self, secondary: bool) -> f32 {
        if secondary { self.m.trig_level2 } else { self.m.trig_level }
    }

    /// A trigger level by SCPI, limited like the stepper and the number pad (same table).
    pub(crate) fn scpi_set_trigger_level(&mut self, secondary: bool, v: f32) {
        self.scpi_range_set(if secondary { Item::TrigLevel2 } else { Item::TrigLevel }, v as f64);
    }
}

/// The highest value of an `eng_ladder`.
fn ladder_top(t: (f64, f64, bool)) -> f64 {
    eng_ladder_list(t.0, t.1, t.2).last().copied().unwrap_or(t.1)
}

/// Data bits of a serial decoder: UART 5 to 9, the others 4 to 32.
pub(super) fn dec_bits(p: Proto) -> (i32, i32) {
    if p == Proto::UART { (5, 9) } else { (4, 32) }
}

/// The lines a video trigger can pick (the first half of the lines of a standard, less its blanking).
pub(super) fn vid_line(s: VideoStd) -> (i32, i32) {
    (1, s.lines() as i32 / 2 - 10)
}
// ---------------------------------------------------------------------------------------------------- ladders

impl Ui {
    /// `d` presses on a ladder of values (sorted, ascending).
    /// * On a rung the value moves `d` rungs and stops at the ends.
    /// * Between two rungs (a preset, a default, a value set by SCPI) the value joins the ladder as a rung of its own for
    ///   as long as the presses go on (`ladder_home`): up goes to the rung above, down to the one below, and N presses
    ///   up and N down come back to it exactly. A value set by anything else (the pad, SCPI) starts over.
    pub(super) fn ladder_move(&mut self, it: Item, cur: f64, d: i32, l: &[f64]) -> f64 {
        if l.is_empty() || d == 0 { return cur; }
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()) + f64::MIN_POSITIVE;
        let last = l.len() - 1;
        let kept = match self.ladder_home { Some((hit, hv, to)) if hit == it && near(cur, to) => Some(hv), _ => None };
        let between = !l.iter().any(|&x| near(x, cur)) && cur > l[0] && cur < l[last];
        let home = kept.or(between.then_some(cur));
        let mut list = l.to_vec();
        if let Some(h) = home {
            if !list.iter().any(|&x| near(x, h)) {
                let at = list.partition_point(|&x| x < h);
                list.insert(at, h);
            }
        }
        // a value outside the ladder snaps to its end
        let i = list.iter().position(|&x| near(x, cur) || x > cur).map_or(list.len() as i32, |i| i as i32);
        let on = list.get(i as usize).is_some_and(|&x| near(x, cur));
        let k = if on || d < 0 { i + d } else { i + d - 1 };
        let to = list[k.clamp(0, list.len() as i32 - 1) as usize];
        self.ladder_home = home.map(|h| (it, h, to));
        to
    }

    /// `ladder_move` on the 1-2-5 ladder of a table entry `(lowest, highest, with a zero)`.
    pub(super) fn ladder_eng(&mut self, it: Item, cur: f64, d: i32, t: (f64, f64, bool)) -> f64 {
        let l = eng_ladder_list(t.0, t.1, t.2);
        self.ladder_move(it, cur, d, &l)
    }
}
