// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The ADC core offsets matched on the unit (`cal.core_offset`, `coretrim`): the unit side of the trim, the kept
//! overlay and the trim run of the acquisition thread. A new way of matching the cores goes here.

/// `cal.core_offset = tight` in system.conf (docs/fork/core-offset.md): the core offsets of every ADC mode used are
/// matched to each other at its first configuration, and kept in OOS's own cal_adc.hex. Off by default.
pub(super) fn core_offset_tight() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| CORE_OFFSET.get())
}

pub(super) const CORE_OFFSET: crate::sysconf::Flag =
    crate::sysconf::Flag { key: "cal.core_offset", env: None, on: "tight" };

/// Tries per ADC mode and session: a mode whose channels sit far from mid-scale or clip is tried again at the next
/// configurations, at most this often.
pub(super) const TRIM_TRIES: u8 = 5;

#[cfg(any(target_os = "linux", test))]
/// A core trim waits until the configuration has stood this long: right after the start-up mode switches the front end
/// is still settling (unit round 15: the first try read the kept offsets 4.4 steps apart and a probe-comp signal
/// as quiet; two restarts later the same offsets read 0.29).
pub(super) const TRIM_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(any(target_os = "linux", test))]
/// A core trim waiting for `TRIM_HOLD` after the configuration of ADC mode `mode` with channel set `mask`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TrimPending {
    pub(super) mode: usize,
    pub(super) mask: u32,
    /// microseconds of the loop's clock
    pub(super) since: u64,
}

#[cfg(any(target_os = "linux", test))]
impl TrimPending {
    pub(super) fn due(&self, now: u64) -> bool {
        std::time::Duration::from_micros(now.saturating_sub(self.since)) >= TRIM_HOLD
    }
}

/// The unit side of `coretrim`: measure unit runs and ADC core offset writes.
#[cfg(any(target_os = "linux", test))]
struct UnitTrim<'a, B, C> {
    bar: &'a mut B,
    clock: &'a mut C,
    adc: &'a mut oos_hal::dho::adc::Adc,
}

#[cfg(any(target_os = "linux", test))]
impl<B: oos_hal::dho::bus::Bus, C: oos_hal::dho::boot::Clock> oos_hal::dho::coretrim::TrimHw for UnitTrim<'_, B, C> {
    fn slots(&mut self) -> Option<[oos_hal::dho::levels::Slot; 8]> {
        let clock = &mut *self.clock;
        oos_hal::dho::levels::run(self.bar, &mut |us| clock.sleep_us(us as u64))
    }
    fn wait_ms(&mut self, ms: u32) {
        self.clock.sleep_us(ms as u64 * 1000);
    }
    fn set_offset(&mut self, core: usize, o: i32) {
        for w in self.adc.core_offset(core, o) {
            self.clock.sleep_us(w.pre_us as u64);
            self.bar.wr(w.reg, w.val);
            self.clock.sleep_us(w.post_us as u64);
        }
    }
}

/// The core offsets laid over cal_adc: the kept file's with `cal.core_offset = tight`, none with the key off (the file
/// is then not even read, so the key off is a full undo).
pub(super) fn overlay_in_use(
    tight: bool,
    load: impl FnOnce() -> Option<oos_hal::dho::coretrim::Overlay>,
) -> oos_hal::dho::coretrim::Overlay {
    if tight {
        load().unwrap_or_default()
    } else {
        oos_hal::dho::coretrim::Overlay::default()
    }
}

/// The kept core offsets (`coretrim::Overlay`) from OOS's cal directory; None without a valid file.
pub(super) fn load_overlay() -> Option<oos_hal::dho::coretrim::Overlay> {
    use oos_hal::dho::{calfile, coretrim};
    let (b, _) = calfile::load_first(coretrim::OVERLAY_FILE, &[calfile::OOS_CAL_DIR])?;
    coretrim::Overlay::from_bytes(calfile::parse(&b).ok()?)
}

/// Matches the core offsets of the active ADC mode (`coretrim`), keeps them in `cal` and in `overlay`, and saves the
/// overlay to OOS's own cal directory when they changed (never a Rigol file, never cal_adc.hex). True when the mode is
/// done (matched, or nothing to change); false to try again at a later configuration.
#[cfg(any(target_os = "linux", test))]
pub(super) fn core_trim<B: oos_hal::dho::bus::Bus, C: oos_hal::dho::boot::Clock>(
    bar: &mut B,
    clock: &mut C,
    state: &mut oos_hal::dho::seq::State,
    cal: &mut oos_hal::dho::adc::CalAdc,
    overlay: &mut oos_hal::dho::coretrim::Overlay,
    mask: u32,
) -> bool {
    use oos_hal::dho::{calfile, coretrim};
    let mode = state.mode;
    let cur = cal.offsets(mode);
    let (groups, lsb) = coretrim::plan(mask, &state.gain);
    let mut hw = UnitTrim { bar, clock, adc: &mut state.adc };
    // the registers may still hold what the cold boot or the stock system wrote: start from `cal`'s values
    for (n, &o) in cur.iter().enumerate() {
        coretrim::TrimHw::set_offset(&mut hw, n, o);
    }
    coretrim::TrimHw::wait_ms(&mut hw, coretrim::TRIM_SETTLE_MS);
    // gates first: a mode whose input carries a signal is left alone without a nudge or a log line, and tried again
    // at a later configuration
    let g = match coretrim::gate(&mut hw, &groups, &lsb) {
        Ok(g) if g.matched != 0 => g,
        Ok(_) => return false,
        Err(e) => {
            eprintln!("oos-ui hw: core offsets of ADC mode {}: {}", mode, e);
            return false;
        }
    };
    let order = match coretrim::order(&mut hw, &cur) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oos-ui hw: core offsets of ADC mode {}: core order not found: {}", mode, e);
            return false;
        }
    };
    match coretrim::trim_gated(&mut hw, &groups, &cur, &order, &lsb, &g) {
        Ok(t) => {
            eprintln!("oos-ui hw: core offsets of ADC mode {}: {:?} -> {:?}, order {:?}, {:.2} steps left (kept {:.2}), matched {:#x} skipped {:#x}", mode, cur, t.offsets, order, t.residual, t.kept, t.matched, t.skipped);
            if t.offsets != cur {
                // this session uses the result; the file only takes a move beyond the per-boot scatter
                cal.set_offsets(mode, &t.offsets);
                if !coretrim::worth_saving(&cur, &t.offsets) {
                    eprintln!("oos-ui hw: core offsets of ADC mode {}: within {} register steps of the kept values, not saved", mode, coretrim::SAVE_MIN_STEPS);
                    return t.matched != 0 && t.skipped == 0;
                }
                overlay.set(mode, &t.offsets);
                if let Err(e) =
                    calfile::save(calfile::OOS_CAL_DIR, coretrim::OVERLAY_FILE, &overlay.to_bytes(), calfile::now())
                {
                    eprintln!(
                        "oos-ui hw: core offsets: {}/{} not saved: {}",
                        calfile::OOS_CAL_DIR,
                        coretrim::OVERLAY_FILE,
                        e
                    );
                }
            }
            t.matched != 0 && t.skipped == 0
        }
        Err(e) => {
            // the registers hold the last round's values; the stored ones go back
            for (n, &o) in cur.iter().enumerate() {
                coretrim::TrimHw::set_offset(&mut hw, n, o);
            }
            eprintln!("oos-ui hw: core offsets of ADC mode {}: {}, order {:?}", mode, e, order);
            false
        }
    }
}

/// The core trim between passes of the loop (`cal.core_offset = tight`): the matched offsets laid over cal_adc (the
/// kept file's, none with the key off), the ADC modes matched this session and the tries per mode, and the trim waiting
/// for its configuration to stand.
#[cfg(any(target_os = "linux", test))]
pub(super) struct TrimRun {
    overlay: oos_hal::dho::coretrim::Overlay,
    done: [bool; 11],
    tries: [u8; 11],
    pending: Option<TrimPending>,
}

#[cfg(any(target_os = "linux", test))]
impl TrimRun {
    /// The kept offsets, laid over `cal_adc` (the cal_adc.hex loaded; unchanged with the key off).
    pub(super) fn new(cal_adc: Option<&mut oos_hal::dho::adc::CalAdc>) -> TrimRun {
        let overlay = overlay_in_use(core_offset_tight(), load_overlay);
        if let Some(c) = cal_adc {
            overlay.apply(c);
        }
        TrimRun { overlay, done: [false; 11], tries: [0; 11], pending: None }
    }

    /// A self-calibration saved new trims (`cal_adc` loaded again): the matched offsets over them; every mode is
    /// matched again from there.
    pub(super) fn recalibrated(&mut self, cal_adc: Option<&mut oos_hal::dho::adc::CalAdc>) {
        if let Some(c) = cal_adc {
            self.overlay.apply(c);
        }
        (self.done, self.tries) = ([false; 11], [0; 11]);
    }

    /// A configuration of ADC mode `mode` with channel set `mask` stands from now (opt-in): its core offsets once it
    /// has stood for `TRIM_HOLD`; a new configuration replaces the wait.
    pub(super) fn configured<C: oos_hal::dho::boot::Clock>(&mut self, mode: usize, mask: u32, clock: &mut C) {
        let m = mode.min(10);
        self.pending = (core_offset_tight() && !self.done[m] && self.tries[m] < TRIM_TRIES).then(|| TrimPending {
            mode: m,
            mask,
            since: clock.now_us(),
        });
    }

    /// The core trim of the current configuration, with nothing armed, once it is due; not while roll or scan runs
    /// (`mode`), a hardware segment recording (`seg_recording`) or the generator (`gen_on`).
    pub(super) fn step<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
        &mut self,
        io: super::slow::Io<'_, B, R, C>,
        armed: &mut Option<(oos_hal::dho::seq::Settings, u64)>,
        cal_adc: Option<&mut oos_hal::dho::adc::CalAdc>,
        mode: crate::board::Slow,
        seg_recording: bool,
        gen_on: bool,
    ) {
        let super::slow::Io { state, bar, c2h, clock, .. } = io;
        let now = clock.now_us();
        if let Some(t) = self.pending.filter(|t| mode == crate::board::Slow::Off && !seg_recording && t.due(now)) {
            self.pending = None;
            // not while the generator runs: its signal is locked to the ADC clock (coretrim gate)
            if !gen_on && t.mode == state.mode.min(10) {
                if let Some(c) = cal_adc {
                    super::plan::disarm(armed, state, bar, c2h);
                    self.tries[t.mode] += 1;
                    self.done[t.mode] = core_trim(bar, clock, state, c, &mut self.overlay, t.mask);
                }
            }
        }
    }
}
