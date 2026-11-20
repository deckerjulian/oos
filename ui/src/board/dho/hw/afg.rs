// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The built-in generator in the acquisition thread (docs/protocols/afg.md): its calibration, its GPIO lines (open from
//! the first request on) and the request last applied, and the pass that applies a changed request. A change of how the
//! loop drives the generator goes here; its register and line sequences are `oos_hal::dho::afg`'s.

use super::plan::disarm;
use super::seg::{seg_abort, SegRun};
use super::slow::Io;
use super::{port, report};
use oos_hal::dho::afg::{GenLines, Mod, Wave};
use oos_hal::dho::seq::Settings;

/// A request of the generator: on, frequency (Hz), amplitude (Vpp), offset (V), wave, modulation (`Shared::afg`).
type Want = (bool, f64, f64, f64, Wave, Mod);

/// The generator between passes of the loop: its calibration, its lines once opened, the request last applied.
pub(super) struct AfgRun<G> {
    cal: Option<oos_hal::dho::afg::Cal>,
    io: Option<G>,
    done: Option<Want>,
}

impl<G: GenLines> AfgRun<G> {
    /// Nothing applied yet; the calibration from `cal_dir`.
    pub(super) fn new(cal_dir: &str) -> AfgRun<G> {
        AfgRun { cal: oos_hal::dho::afg::Cal::load(cal_dir), io: None, done: None }
    }

    /// The output runs (the request last applied is on).
    pub(super) fn on(&self) -> bool {
        self.done.is_some_and(|d| d.0)
    }

    /// Applies the request `want` when it changed; the GPIO lines stay open from the first request on, which is "off"
    /// at start: an output left running (oos-fpgad afg, an earlier oos-ui) goes off as at power-on.
    pub(super) fn apply<P: port::Port<Gen = G>>(
        &mut self,
        want: Option<Want>,
        unit: &mut P,
        io: Io<'_, P::Bus, P::Stream, P::Clock>,
        armed: &mut Option<(Settings, u64)>,
        segrun: &mut Option<SegRun>,
    ) {
        let Io { state, bar, c2h, clock, out, .. } = io;
        if want != self.done {
            disarm(armed, state, bar, c2h);
            seg_abort(segrun, state, bar, c2h, clock, out);
            // models without the generator (non-S): its GPIO lines and FPGA block are never touched
            if let Some((on, f, a, o, wave, md)) = want.filter(|_| oos_hal::model::platform().caps.has_gen()) {
                if self.io.is_none() {
                    self.io = unit.open_gen().map_err(|e| report(out, format!("generator GPIO: {}", e))).ok();
                }
                if let Some(io) = self.io.as_mut() {
                    // a running output keeps running when the change allows it
                    let quick = self
                        .done
                        .filter(|d| on && d.0 && d.4 == wave && d.5 == md && md == oos_hal::dho::afg::Mod::None)
                        .and_then(|d| {
                            oos_hal::dho::afg::retune(
                                self.cal.as_ref(),
                                wave,
                                d.1 * 1e6,
                                f * 1e6,
                                a * 1e9,
                                o * 1e9,
                                io.relay_code(),
                            )
                        });
                    let ops = if let Some(q) = quick {
                        q
                    } else if on {
                        oos_hal::dho::afg::start_mod(
                            self.cal.as_ref(),
                            wave,
                            f * 1e6,
                            a * 1e9,
                            o * 1e9,
                            io.relay_code(),
                            md,
                        )
                    } else {
                        oos_hal::dho::afg::idle(a * 1e9, o * 1e9, f * 1e6, io.relay_code())
                    };
                    oos_hal::dho::afg::run(bar, io, clock, &ops);
                    report(
                        out,
                        format!(
                            "generator: {}",
                            if on { format!("{:?} {} Hz {} Vpp {} V {:?}", wave, f, a, o, md) } else { "off".into() }
                        ),
                    );
                }
            }
            self.done = want;
        }
    }
}
