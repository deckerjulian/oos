// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Matching of the ADC core offsets of the active ADC mode at run time (fork, opt-in `cal.core_offset = tight`,
//! docs/fork/core-offset.md). The RT8847's eight cores each have an offset register (0x14 + 8n, about half an ADC step per register step, unit round 11); the stock
//! calibration trims every core to mid-scale within +-1.5 steps (self-cal.md 14.6), so two cores can differ by up to
//! 3 steps. At decimated rates neighbouring samples come from two cores (netlist), and the difference stays in every
//! frame: a fixed fs/2 pattern that the hardware average and the DVM block keep (spec 01 section 8, dvm.md).
//!
//! This trim matches the cores that sample one channel to each other, not to mid-scale: target = the channel's mean
//! over its cores, tolerance `TOL` steps. Matching needs no known input level, only one that every core of the
//! channel sees alike: the measure unit runs all cores over the same 1.7 ms. Channels whose mean is far from
//! mid-scale (core gain differences would then move the core means) or that clip are left as they are.
//!
//! Which measure slot shows which core register is found by nudging each core in turn (`order`), as self-cal does in
//! core-order mode; the mapping of the normal acquisition modes is not known otherwise (rt8847.md open item 8).

use crate::dho::levels::Slot;

/// What the trim needs from the unit.
pub trait TrimHw {
    /// One run of the measure unit: per slot the 12-bit min / max and the mean on the 16-bit scale.
    fn slots(&mut self) -> Option<[Slot; 8]>;
    /// Writes the offset register of core `core` (-1024..1023 ADC steps).
    fn set_offset(&mut self, core: usize, o: i32);
    /// Waits between the gate's runs.
    fn wait_ms(&mut self, _ms: u32) {}
}

/// Target in ADC steps: a core further than this from the channel mean is corrected. The register step is about
/// 0.55 ADC step (unit round 11), so a core can always get within 0.3; the rest is measurement noise.
pub const TOL: f64 = 0.6;
/// Accepted result in ADC steps when the noise keeps some core above `TOL` in every round: the best round is kept if
/// its worst core is within this. The stock calibration accepts 1.5.
pub const ACCEPT: f64 = 1.0;
/// Measure-unit runs averaged per reading in `trim`: one run's slot means scatter by about 0.25 step against each
/// other (unit round 11: slot 6 read +8.48, +8.20, +8.45 in three runs), more than the single-run 0.05 the mean of
/// 2^18 samples would give.
pub const AVG_RUNS: usize = 4;
/// Wait after offset writes before the next measure run, ms. A run started right after the SPI write partly sees the
/// old offset, so `order` measures too small a register step and `trim` then over-corrects and oscillates (unit
/// round 11: no convergence, 0.77 steps left). The stock firmware waits 2 ms in its core-order search and 50 ms
/// between calibration rounds.
pub const ORDER_SETTLE_MS: u32 = 5;
pub const TRIM_SETTLE_MS: u32 = 50;
/// Rounds of correction before giving up.
pub const ROUNDS: usize = 8;
/// Nudge for `order`, register steps: about 32 ADC steps at the stock's 8 codes per register step.
pub const NUDGE: i32 = 64;
/// A channel is matched only with its mean within this many steps of mid-scale: a core gain mismatch of 0.1 %
/// moves a core's mean by 0.25 step there.
pub const MID_WINDOW: f64 = 256.0;
/// With two or more channels, a channel is matched only when its cores start within this many steps of each other:
/// more means the slot grouping (`levels::slots`, not verified for the pair and four-channel modes) put cores of
/// different channels together. With one channel every slot carries it, and a core far off is what the trim is for
/// (unit round 6: one core about 12 steps high at 1.25 GSa/s).
pub const SPREAD_MAX: f64 = 8.0;
/// Quietness gate. Matching to the channel mean assumes every core sees the same signal on average; a signal that
/// is not quiet, or that is correlated with the interleave (near a multiple of fs/8), would be trimmed into the
/// offsets. A channel is matched only when every slot's samples stay within `QUIET_RANGE` steps (max - min of the
/// 12-bit codes; an open input spans 43 to 61 at 100 mV/div, unit round 9) and every slot's mean stays within `STABLE` steps (run-to-run scatter is about 0.3, round 11) over `GATE_RUNS`
/// runs `GATE_WAIT_MS` apart: an fs/8-correlated signal that is not locked to the ADC clock moves the core means from
/// run to run. A signal locked to the ADC clock (the built-in generator) cannot be told from an offset: the caller
/// does not trim while the generator runs.
pub const QUIET_RANGE: u16 = 128;
pub const STABLE: f64 = 1.0;
pub const GATE_RUNS: usize = 3;
pub const GATE_WAIT_MS: u32 = 20;
/// Mid-scale on the 16-bit measure scale.
const MID: f64 = 32768.0;

/// Where each core shows and how strongly its offset register acts: `slot[n]` = the slot of core n, `codes[n]` =
/// measure codes per register step of core n (signed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Order {
    pub slot: [usize; 8],
    pub codes: [f64; 8],
}

/// A nudge must move its slot by at least this many measure codes (4 ADC steps) to count.
pub const MIN_MOVE: f64 = 64.0;

/// Finds `Order` for the offsets `cur` now in the registers: each core in turn nudged by `NUDGE` register steps; the
/// slot that moves most is the core's, and no other slot may move by more than a quarter of it. The register step is
/// measured, not assumed: unit round 9 showed a nudge of 32 moving less than the 16 ADC steps the first version
/// expected (the stock core-order search uses a threshold of 7936 codes for a nudge of 1023, about 8 codes per
/// register step, half an ADC step). Err names the core and the deltas when a nudge is not clear.
pub fn order(hw: &mut dyn TrimHw, cur: &[i32; 8]) -> Result<Order, String> {
    let base = hw.slots().ok_or("measure unit not answering")?;
    let mut slot = [usize::MAX; 8];
    let mut codes = [0.0; 8];
    for n in 0..8 {
        hw.set_offset(n, cur[n] + NUDGE);
        hw.wait_ms(ORDER_SETTLE_MS);
        let m = hw.slots();
        hw.set_offset(n, cur[n]);
        hw.wait_ms(ORDER_SETTLE_MS);
        let m = m.ok_or("measure unit not answering")?;
        let d: [f64; 8] = std::array::from_fn(|k| m[k].mean - base[k].mean);
        let k = (0..8).max_by(|&a, &b| d[a].abs().total_cmp(&d[b].abs())).unwrap_or(0);
        let clear = d[k].abs() >= MIN_MOVE && (0..8).all(|j| j == k || d[j].abs() <= d[k].abs() / 4.0);
        if !clear {
            return Err(format!("core {}: nudge of {} moved the slots by {:?} codes", n, NUDGE, d.map(|x| x.round() as i64)));
        }
        slot[n] = k;
        codes[n] = d[k] / NUDGE as f64;
    }
    let mut seen = [false; 8];
    for &k in &slot {
        if seen[k] {
            return Err(format!("two cores show in one slot: {:?}", slot));
        }
        seen[k] = true;
    }
    Ok(Order { slot, codes })
}

/// Slot groups and codes per ADC step for channel set `mask` with the channels' digital gains (`State::gain`): the
/// slots of each acquired channel (`levels::slots`), and 32 codes per step where the gain of 2 and more puts the ADC
/// x2 of 0x105c before the measure unit, else 16.
pub fn plan(mask: u32, gain: &[f64; 4]) -> (Vec<(usize, Vec<usize>)>, [f64; 8]) {
    let mut lsb = [16.0; 8];
    let mut groups = Vec::new();
    for c in 0..4 {
        let sl = crate::dho::levels::slots(mask, c);
        if sl.is_empty() {
            continue;
        }
        for &k in &sl {
            lsb[k] = if gain[c] >= 2.0 { 32.0 } else { 16.0 };
        }
        groups.push((c, sl));
    }
    (groups, lsb)
}

/// The result of a trim.
#[derive(Clone, Debug, PartialEq)]
pub struct Trimmed {
    /// new offset of every core (cores of skipped channels unchanged)
    pub offsets: [i32; 8],
    /// largest core-to-channel-mean difference left, steps (0 when nothing was matched)
    pub residual: f64,
    /// channels matched / skipped (bit c = CH c+1)
    pub matched: u32,
    pub skipped: u32,
}

/// Matches the cores of every channel in `groups` (channel -> its slots, `levels::slots`) to the channel's mean.
/// `cur`: offsets now in the registers; `order`: from `order`; `lsb[k]`: codes per ADC step of slot k (16, or 32 with
/// the ADC x2 of 0x105c), for the tolerance and the gates. Corrections are in register steps through the measured
/// `order.codes`. Err when a matched channel does not converge in `ROUNDS`.
pub fn trim(hw: &mut dyn TrimHw, groups: &[(usize, Vec<usize>)], cur: &[i32; 8], order: &Order, lsb: &[f64; 8]) -> Result<Trimmed, String> {
    let mut o = *cur;
    let core_of = |slot: usize| order.slot.iter().position(|&k| k == slot);
    let mut matched = 0u32;
    let mut skipped = 0u32;
    let mut residual: f64 = 0.0;
    let mut runs = Vec::with_capacity(GATE_RUNS);
    for i in 0..GATE_RUNS {
        if i > 0 {
            hw.wait_ms(GATE_WAIT_MS);
        }
        runs.push(hw.slots().ok_or("measure unit not answering")?);
    }
    let first = runs[0];
    let quiet = |k: usize| runs.iter().all(|r| r[k].max.saturating_sub(r[k].min) <= QUIET_RANGE);
    let stable = |k: usize| {
        let (lo, hi) = runs.iter().fold((f64::MAX, f64::MIN), |(a, b), r| (a.min(r[k].mean), b.max(r[k].mean)));
        hi - lo <= STABLE * lsb[k]
    };
    let usable: Vec<&(usize, Vec<usize>)> = groups
        .iter()
        .filter(|(c, sl)| {
            let mean = sl.iter().map(|&k| first[k].mean).sum::<f64>() / sl.len().max(1) as f64;
            let clipped = sl.iter().any(|&k| first[k].min == 0 || first[k].max >= 4095);
            let near = sl.iter().all(|&k| (mean - MID).abs() <= MID_WINDOW * lsb[k]);
            let alike = groups.len() == 1 || sl.iter().all(|&k| (first[k].mean - mean).abs() <= SPREAD_MAX * lsb[k]);
            let ok = sl.len() > 1 && !clipped && near && alike && sl.iter().all(|&k| quiet(k) && stable(k));
            if ok {
                matched |= 1 << c;
            } else {
                skipped |= 1 << c;
            }
            ok
        })
        .collect();
    if usable.is_empty() {
        return Ok(Trimmed { offsets: o, residual: 0.0, matched, skipped });
    }
    let mut m = average(&runs);
    // the round with the smallest worst core: noise can keep one core above TOL in every round
    let mut best = (f64::MAX, o);
    for round in 0..=ROUNDS {
        let mut worst: f64 = 0.0;
        let mut fix: Vec<(usize, i32)> = Vec::new();
        for (_, sl) in &usable {
            let target = sl.iter().map(|&k| m[k].mean).sum::<f64>() / sl.len() as f64;
            for &k in sl {
                let e = (m[k].mean - target) / lsb[k];
                worst = worst.max(e.abs());
                if e.abs() > TOL {
                    let n = core_of(k).ok_or(format!("slot {} has no core", k))?;
                    // a correction below half a register step cannot be made: that core is as close as it gets
                    let step = ((m[k].mean - target) / order.codes[n]).round() as i32;
                    if step != 0 {
                        fix.push((n, o[n] - step));
                    }
                }
            }
        }
        residual = worst;
        if worst < best.0 {
            best = (worst, o);
        }
        if fix.is_empty() {
            return Ok(Trimmed { offsets: o, residual, matched, skipped });
        }
        if round == ROUNDS {
            break;
        }
        for (n, v) in fix {
            o[n] = v.clamp(-1024, 1023);
            hw.set_offset(n, o[n]);
        }
        hw.wait_ms(TRIM_SETTLE_MS);
        m = measure(hw)?;
    }
    let (worst, offs) = best;
    if worst <= ACCEPT {
        for n in 0..8 {
            if offs[n] != o[n] {
                hw.set_offset(n, offs[n]);
            }
        }
        return Ok(Trimmed { offsets: offs, residual: worst, matched, skipped });
    }
    Err(format!("core offsets did not converge in {} rounds, {:.2} steps left (best {:.2})", ROUNDS, residual, worst))
}

/// `AVG_RUNS` runs of the measure unit averaged per slot (means averaged, min and max over all runs).
fn measure(hw: &mut dyn TrimHw) -> Result<[Slot; 8], String> {
    let mut runs = Vec::with_capacity(AVG_RUNS);
    for _ in 0..AVG_RUNS {
        runs.push(hw.slots().ok_or("measure unit not answering")?);
    }
    Ok(average(&runs))
}

fn average(runs: &[[Slot; 8]]) -> [Slot; 8] {
    std::array::from_fn(|k| Slot {
        min: runs.iter().map(|r| r[k].min).min().unwrap_or(0),
        max: runs.iter().map(|r| r[k].max).max().unwrap_or(0),
        mean: runs.iter().map(|r| r[k].mean).sum::<f64>() / runs.len().max(1) as f64,
    })
}

/// The trimmed offsets OOS keeps (`/oos/cal/core_offsets.hex`, a `calfile` container), apart from the cal_adc
/// payload: loaded only with `cal.core_offset = tight`, so turning the key off goes back to the cal_adc.hex in use
/// before (Rigol's or the self-cal's) without touching it. Payload: a u16 mask of the modes held, then 11 modes x 8
/// i16 offsets, little endian.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overlay {
    pub modes: u16,
    pub off: [[i32; 8]; 11],
}

/// File name of `Overlay` in OOS's cal directory.
pub const OVERLAY_FILE: &str = "core_offsets.hex";
const OVERLAY_LEN: usize = 2 + 11 * 16;

impl Overlay {
    pub fn set(&mut self, mode: usize, o: &[i32; 8]) {
        if mode < 11 {
            self.modes |= 1 << mode;
            self.off[mode] = *o;
        }
    }

    /// The offsets of the modes held, written over `cal`.
    pub fn apply(&self, cal: &mut crate::dho::adc::CalAdc) {
        for m in (0..11).filter(|m| self.modes & (1 << m) != 0) {
            cal.set_offsets(m, &self.off[m]);
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = self.modes.to_le_bytes().to_vec();
        for m in &self.off {
            for &v in m {
                b.extend((v as i16).to_le_bytes());
            }
        }
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Overlay> {
        if b.len() != OVERLAY_LEN {
            return None;
        }
        let mut o = Overlay { modes: u16::from_le_bytes([b[0], b[1]]) & 0x7ff, ..Overlay::default() };
        for (m, row) in o.off.iter_mut().enumerate() {
            for (n, v) in row.iter_mut().enumerate() {
                let i = 2 + 16 * m + 2 * n;
                *v = i16::from_le_bytes([b[i], b[i + 1]]) as i32;
            }
        }
        Some(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight cores with their own offset error (steps), shown in slots by `perm`, a channel level, the ADC x2 per
    /// slot; the measured mean includes the register offset and the error, rounded like the unit's 16-bit mean.
    struct Model {
        err: [f64; 8],
        reg: [i32; 8],
        perm: [usize; 8],
        level: f64,
        lsb: f64,
        /// measure codes per register step (the unit: about 8, half an ADC step)
        reg_codes: f64,
        writes: usize,
        /// per-run scatter of each slot mean, +- ADC steps (uniform, deterministic)
        noise: f64,
        seed: u64,
        /// share of a run that still sees the offsets before the last write when no wait came in between
        lag: f64,
        prev: [i32; 8],
    }

    impl TrimHw for Model {
        fn slots(&mut self) -> Option<[Slot; 8]> {
            let mut s = [Slot::default(); 8];
            for n in 0..8 {
                self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u = (self.seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
                let reg = self.reg[n] as f64 * (1.0 - self.lag) + self.prev[n] as f64 * self.lag;
                let mean = self.level + (self.err[n] + u * self.noise) * self.lsb + reg * self.reg_codes;
                let code = (mean / 16.0).clamp(0.0, 4095.0) as u16;
                s[self.perm[n]] = Slot { min: code.saturating_sub(20), max: (code + 20).min(4095), mean: mean.round() };
            }
            self.prev = self.reg;
            Some(s)
        }
        fn set_offset(&mut self, core: usize, o: i32) {
            self.reg[core] = o;
            self.writes += 1;
        }
        fn wait_ms(&mut self, ms: u32) {
            if ms >= ORDER_SETTLE_MS {
                self.prev = self.reg;
            }
        }
    }

    const PERM: [usize; 8] = [0, 2, 1, 3, 4, 6, 5, 7];
    const ORD: Order = Order { slot: PERM, codes: [8.0; 8] };

    /// Core n's level in ADC steps with register offset `o` (model units).
    fn total(m: &Model, n: usize, o: i32) -> f64 {
        m.err[n] + o as f64 * m.reg_codes / m.lsb
    }
    const ERR: [f64; 8] = [1.4, -1.3, 0.2, -0.9, 1.1, -1.5, 0.7, 0.0];

    fn model(level: f64) -> Model {
        Model { err: ERR, reg: [3, -2, 0, 1, 0, 4, -1, 0], perm: PERM, level, lsb: 16.0, reg_codes: 8.0, writes: 0, noise: 0.0, seed: 1, lag: 0.0, prev: [3, -2, 0, 1, 0, 4, -1, 0] }
    }

    // Break it catches: an overlay that writes modes it does not hold over the cal file, a byte layout that does not
    // read back.
    #[test]
    fn overlay_round_trip_and_apply() {
        let mut o = Overlay::default();
        o.set(2, &[1, -2, 3, -4, 5, -6, 7, -1024]);
        o.set(11, &[9; 8]);
        let b = o.to_bytes();
        assert_eq!(b.len(), OVERLAY_LEN);
        assert_eq!(Overlay::from_bytes(&b), Some(o.clone()));
        assert_eq!(Overlay::from_bytes(&b[1..]), None);
        let mut cal = crate::dho::adc::CalAdc::from_payload(vec![0; 0x738]).unwrap();
        cal.set_offsets(0, &[5; 8]);
        o.apply(&mut cal);
        assert_eq!((cal.offsets(2), cal.offsets(0)), ([1, -2, 3, -4, 5, -6, 7, -1024], [5; 8]));
    }

    #[test]
    fn plan_groups_slots_by_channel() {
        let (g, lsb) = plan(0b0001, &[2.14, 1.0, 1.0, 1.0]);
        assert_eq!((g, lsb), (vec![(0, (0..8).collect())], [32.0; 8]));
        let (g, lsb) = plan(0b0011, &[1.07, 2.2, 1.0, 1.0]);
        let (s0, s1) = (crate::dho::levels::slots(0b0011, 0), crate::dho::levels::slots(0b0011, 1));
        assert_eq!(g, vec![(0, s0.clone()), (1, s1)]);
        // CH2's gain of 2.2 doubles the codes per step on its slots only
        assert!((0..8).all(|k| lsb[k] == if s0.contains(&k) { 16.0 } else { 32.0 }), "{:?}", lsb);
        assert_eq!(plan(0b1111, &[1.0; 4]).0.len(), 4);
    }

    #[test]
    fn order_finds_the_slot_of_every_core() {
        let mut m = model(MID);
        let cur = m.reg;
        assert_eq!(order(&mut m, &cur), Ok(ORD));
        assert_eq!(m.reg, cur, "every nudge is taken back");
        // round 9: a weak register step is measured, not refused; a register that moves nothing is refused
        m.reg_codes = 2.0;
        assert_eq!(order(&mut m, &cur).map(|o| o.codes), Ok([2.0; 8]));
        m.reg_codes = 0.5;
        assert!(order(&mut m, &cur).unwrap_err().contains("core 0"));
    }

    // Break it catches: matching to mid-scale instead of to each other, a sign error in the correction, cores of one
    // channel corrected from another channel's mean, a tolerance that the 1-step register cannot reach.
    #[test]
    fn one_channel_matches_all_cores() {
        // one channel: every slot carries it; the input sits 40 steps above mid-scale
        let mut m = model(MID + 40.0 * 16.0);
        let cur = m.reg;
        let groups = vec![(0, (0..8).collect::<Vec<_>>())];
        let t = trim(&mut m, &groups, &cur, &ORD, &[16.0; 8]).unwrap();
        assert_eq!((t.matched, t.skipped), (1, 0));
        let total: Vec<f64> = (0..8).map(|n| total(&m, n, t.offsets[n])).collect();
        let mean = total.iter().sum::<f64>() / 8.0;
        assert!(total.iter().all(|x| (x - mean).abs() <= TOL + 1e-9), "{:?}", total);
        assert!(t.residual <= TOL);
        // the channel's level stays where it was (no pull to mid-scale)
        assert!((mean - (ERR.iter().sum::<f64>() / 8.0 + cur.iter().sum::<i32>() as f64 / 8.0)).abs() < 1.0);
    }

    #[test]
    fn two_channels_are_matched_separately() {
        // CH1 on slots 6, 4, 7, 5 and CH2 on 2, 0, 3, 1 (levels::slots of a pair); different levels per channel
        struct Two(Model);
        impl TrimHw for Two {
            fn slots(&mut self) -> Option<[Slot; 8]> {
                let mut s = self.0.slots()?;
                for k in [2, 0, 3, 1] {
                    s[k].mean += 100.0 * 16.0;
                }
                Some(s)
            }
            fn set_offset(&mut self, core: usize, o: i32) {
                self.0.set_offset(core, o)
            }
        }
        let mut m = Two(model(MID));
        let cur = m.0.reg;
        let groups = vec![(0, vec![6, 4, 7, 5]), (1, vec![2, 0, 3, 1])];
        let t = trim(&mut m, &groups, &cur, &ORD, &[16.0; 8]).unwrap();
        assert_eq!(t.matched, 0b11);
        for (_, sl) in &groups {
            let tot: Vec<f64> = sl.iter().map(|&k| PERM.iter().position(|&p| p == k).unwrap()).map(|n| total(&m.0, n, t.offsets[n])).collect();
            let mean = tot.iter().sum::<f64>() / tot.len() as f64;
            assert!(tot.iter().all(|x| (x - mean).abs() <= TOL + 1e-9), "{:?}", tot);
        }
    }

    #[test]
    fn far_from_mid_or_clipped_channels_are_left_alone() {
        for level in [MID + 300.0 * 16.0, 4095.0 * 16.0] {
            let mut m = model(level);
            let cur = m.reg;
            let t = trim(&mut m, &[(0, (0..8).collect())], &cur, &ORD, &[16.0; 8]).unwrap();
            assert_eq!((t.offsets, t.matched, t.skipped, m.writes), (cur, 0, 1, 0), "level {}", level);
        }
    }

    // Round 11: one run's slot means scatter by about 0.25 step, the register step is about 0.55 ADC step, and the
    // trim gave up with 0.77 left. Break it catches: measuring before a write has settled (a too small register step
    // from `order`, then over-correction that oscillates), giving up instead of keeping the best round within ACCEPT.
    #[test]
    fn noisy_slots_still_converge() {
        for seed in 1..20 {
            let mut m = model(MID);
            m.err[5] += 10.0;
            (m.noise, m.seed, m.reg_codes, m.lag) = (0.3, seed, 8.8, 0.6);
            let cur = m.reg;
            let ord = order(&mut m, &cur).unwrap();
            assert!(ord.codes.iter().all(|c| (c - 8.8).abs() < 0.2), "seed {}: register step measured after settling {:?}", seed, ord.codes);
            let t = trim(&mut m, &[(0, (0..8).collect())], &cur, &ord, &[16.0; 8]).unwrap_or_else(|e| panic!("seed {}: {}", seed, e));
            assert_eq!((t.matched, m.reg), (1, t.offsets), "seed {}: matched, the registers hold the kept offsets", seed);
            let tot: Vec<f64> = (0..8).map(|n| total(&m, n, t.offsets[n])).collect();
            let mean = tot.iter().sum::<f64>() / 8.0;
            assert!(tot.iter().all(|x| (x - mean).abs() <= ACCEPT + 0.2), "seed {}: {:?}", seed, tot);
        }
    }

    // Round 6: one core about 12 steps above the other seven (1.25 GSa/s, one channel). Break it catches: the
    // spread guard of the multi-channel case refusing the one-channel case, a mean target that stops short.
    #[test]
    fn one_core_far_off_is_brought_in() {
        let mut m = model(MID);
        m.err[5] += 12.0;
        let cur = m.reg;
        let t = trim(&mut m, &[(0, (0..8).collect())], &cur, &ORD, &[16.0; 8]).unwrap();
        assert_eq!(t.matched, 1);
        let total: Vec<f64> = (0..8).map(|n| total(&m, n, t.offsets[n])).collect();
        let mean = total.iter().sum::<f64>() / 8.0;
        assert!(total.iter().all(|x| (x - mean).abs() <= TOL + 1e-9), "{:?}", total);
        assert!(t.offsets[5] <= cur[5] - 11, "{:?}", t.offsets);
    }

    // Break it catches: a signal correlated with the interleave trimmed into the offsets.
    #[test]
    fn an_interleave_correlated_signal_is_refused() {
        // a component near fs/8 that is not locked to the ADC clock: each core sees its own phase of it, and the
        // phase walks between runs
        struct Fs8 {
            m: Model,
            run: usize,
        }
        impl TrimHw for Fs8 {
            fn slots(&mut self) -> Option<[Slot; 8]> {
                let mut s = self.m.slots()?;
                let ph = self.run as f64 * 0.7;
                for n in 0..8 {
                    s[self.m.perm[n]].mean += 6.0 * 16.0 * (std::f64::consts::TAU * n as f64 / 8.0 + ph).cos();
                }
                self.run += 1;
                Some(s)
            }
            fn set_offset(&mut self, core: usize, o: i32) {
                self.m.set_offset(core, o)
            }
        }
        let mut f = Fs8 { m: model(MID), run: 0 };
        let cur = f.m.reg;
        let t = trim(&mut f, &[(0, (0..8).collect())], &cur, &ORD, &[16.0; 8]).unwrap();
        assert_eq!((t.offsets, t.skipped, f.m.writes), (cur, 1, 0));
    }

    #[test]
    fn a_signal_is_refused() {
        // the probe-comp square: every slot spans the whole swing
        struct Sq(Model);
        impl TrimHw for Sq {
            fn slots(&mut self) -> Option<[Slot; 8]> {
                let mut s = self.0.slots()?;
                for x in s.iter_mut() {
                    (x.min, x.max) = (x.min - 400, x.max + 400);
                }
                Some(s)
            }
            fn set_offset(&mut self, core: usize, o: i32) {
                self.0.set_offset(core, o)
            }
        }
        let mut m = Sq(model(MID));
        let cur = m.0.reg;
        let t = trim(&mut m, &[(0, (0..8).collect())], &cur, &ORD, &[16.0; 8]).unwrap();
        assert_eq!((t.offsets, t.skipped), (cur, 1));
    }

    #[test]
    fn a_group_with_two_levels_is_left_alone() {
        // a wrong slot grouping: half the slots carry another channel 20 steps away
        struct Mixed(Model);
        impl TrimHw for Mixed {
            fn slots(&mut self) -> Option<[Slot; 8]> {
                let mut s = self.0.slots()?;
                for k in [0, 1, 2, 3] {
                    s[k].mean += 20.0 * 16.0;
                }
                Some(s)
            }
            fn set_offset(&mut self, core: usize, o: i32) {
                self.0.set_offset(core, o)
            }
        }
        let mut m = Mixed(model(MID));
        let cur = m.0.reg;
        // as one of two channels (with one channel every slot is the channel's, and this would be trimmed)
        let groups = vec![(0, (0..8).collect()), (1, vec![])];
        let t = trim(&mut m, &groups, &cur, &ORD, &[16.0; 8]).unwrap();
        assert_eq!((t.offsets, t.skipped, m.0.writes), (cur, 0b11, 0));
    }

    #[test]
    fn the_adc_x2_doubles_the_codes_per_step() {
        let mut m = model(MID);
        m.lsb = 32.0;
        let cur = m.reg;
        let t = trim(&mut m, &[(0, (0..8).collect())], &cur, &ORD, &[32.0; 8]).unwrap();
        let total: Vec<f64> = (0..8).map(|n| total(&m, n, t.offsets[n])).collect();
        let mean = total.iter().sum::<f64>() / 8.0;
        assert!(total.iter().all(|x| (x - mean).abs() <= TOL + 1e-9), "{:?}", total);
    }
}
