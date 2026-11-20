// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings in the acquisition thread (docs/protocols/acquisition-start.md): what the UI asks for, read once a
//! pass and completed for the unit, and the configuration of the unit when it changed. The state between passes is
//! the configuration applied: its settings and parameters, roll or scan, the configurations counted and the hardware
//! average's tracker. A change of how the loop takes the UI's settings or configures the unit goes here.

use super::plan::disarm;
use super::seg::seg_abort;
use super::settings::{avg_kind, raw_for_ui};
use super::units::{hw_merge, mask_map, unit_key, UnitGuard};
use super::{execute, port, report, Loop};
use crate::board::{lock, HwShared, Slow};
use oos_hal::dho::hz;
use oos_hal::dho::seq::Settings;

/// The configuration between passes of the loop: the settings applied with their parameters (None: configure at the
/// next pass), roll, scan or neither as configured, the number of the configurations applied (the DVM block restarts
/// after each), and the hardware average: triggers since the last clear, the `Shared::avg_clear` it was cleared for,
/// batches logged.
#[derive(Default)]
pub(super) struct ConfigRun {
    pub(super) current: Option<(Settings, hz::Params)>,
    pub(super) mode: Slow,
    pub(super) configs: u64,
    pub(super) avg: Option<(oos_hal::dho::avg::Tracker, u64, u32)>,
}

/// What the UI asks for in a pass: the settings for the unit, the settings as the UI gave them with roll or scan
/// (when they change, a wait for a trigger ends early: `acquire_batch_until`), roll or scan, and the
/// `Shared::avg_clear` asked for.
pub(super) struct Request {
    pub(super) want: Settings,
    pub(super) asked: (Option<Settings>, Slow),
    pub(super) mode: Slow,
    pub(super) avg_clear: u64,
}

impl Request {
    /// The request read from `out` and completed for the unit: the front ends with `program_afe`, raw reads where the
    /// UI needs them, the mask and zone units unless `guard` blocks them for these settings (said in
    /// `Shared::unit_blocked`), the LA's zero codes `la_cal`. None when the UI asks for no acquisition.
    pub(super) fn read(
        out: &HwShared,
        guard: &UnitGuard,
        la_cal: oos_hal::dho::la::Cal,
        program_afe: bool,
    ) -> Option<Request> {
        let (want, want_mode, avg_clear, mask_want, zone_want, needs_raw) = {
            let g = lock(out);
            (g.want.clone(), g.slow, g.avg_clear, g.mask_want.clone(), g.zone_want.clone(), g.needs_raw)
        };
        let mut want = want?;
        // the settings as the UI gave them: when they change, a wait for a trigger ends early (`acquire_batch_until`)
        let asked = (Some(want.clone()), want_mode);
        want.afe = program_afe;
        raw_for_ui(&mut want, needs_raw);
        let map = mask_map();
        let key = unit_key(&want, map);
        if guard.blocks(&key) {
            hw_merge(&mut want, None, None, map);
        } else {
            hw_merge(&mut want, mask_want.as_ref(), zone_want.as_ref(), map);
        }
        {
            let mut g = lock(out);
            let why = guard.blocks(&key).then(|| {
                "the mask/zone unit stopped the acquisition with these settings: tested in software".to_string()
            });
            if g.unit_blocked != why {
                g.unit_blocked = why;
            }
        }
        if let Some(l) = want.la.as_mut() {
            l.cal = la_cal;
        }
        Some(Request { want, asked, mode: want_mode, avg_clear })
    }
}

impl<P: port::Port> Loop<P> {
    /// The configuration on a change of the settings or of roll and scan asked for in `req`: nothing armed, no
    /// segments recorded, no stream, then the configuration's steps with their front-end steps; the batch part's LA
    /// failures, the core trim's wait and the hardware average start over. False when it failed: the pass ends there
    /// and the next one tries again.
    pub(super) fn configure(&mut self, req: &Request) -> bool {
        use oos_hal::dho::boot::Clock;
        let status = |s: String| report(&self.out, s);
        let Request { want, mode: want_mode, avg_clear, .. } = req;
        if self.config.current.as_ref().is_none_or(|c| c.0 != *want) || self.config.mode != *want_mode {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h);
            seg_abort(&mut self.segrun, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock, &self.out);
            self.config.mode = *want_mode;
            self.stop.last_batch = None;
            self.slow.stop(&mut self.state, &mut self.bar);
            match self.state.configure(want, self.cal.as_ref(), self.cal_adc.as_ref()) {
                Ok((steps, p)) => {
                    if let Err(e) = execute::<P>(
                        &steps,
                        &mut self.bar,
                        self.afe_bus.as_mut(),
                        &mut self.c2h,
                        &mut self.lines,
                        &mut self.clock,
                    ) {
                        status(format!("configure: {}", e));
                        self.clock.sleep_us(500000);
                        return false;
                    }
                    self.batch.la_fails = 0;
                    status(format!("configured: mask {:#x}, {} Sa/s, {} points", want.mask, p.rate, p.memsel));
                    self.config.current = Some((want.clone(), p));
                    self.config.configs += 1;
                    // core offsets of the ADC mode (opt-in): once this configuration has stood for TRIM_HOLD; a new
                    // configuration replaces the wait
                    self.trim.configured(self.state.mode, want.mask, &mut self.clock);
                    // the configuration cleared the average (`seq::State::configure`)
                    self.config.avg = match want.mode {
                        oos_hal::dho::acq::Mode::Average { count } => {
                            Some((oos_hal::dho::avg::Tracker::new(avg_kind(), count), *avg_clear, 0))
                        }
                        _ => None,
                    };
                }
                Err(e) => {
                    status(format!("settings: {}", e));
                    self.clock.sleep_us(500000);
                    return false;
                }
            }
        }
        true
    }
}
