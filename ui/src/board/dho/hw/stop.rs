// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! STOP in the acquisition thread (docs/fork/stop-exact.md): the hold, with the newest frame read once more raw where
//! the live compression or the deep measurements want it, and the views read again from the stored record. The state
//! between passes is the last batch read and the view last read for. A change of what a STOP keeps or reads goes here.

use super::decode::decode_with;
use super::plan::{disarm, final_replay, stop_plan};
use super::slow::Io;
use super::{execute, port, report};
use crate::board::Frame;
use oos_hal::dho::bus::{Bus, DmaBuf};
use oos_hal::dho::hz;
use oos_hal::dho::seq::{self, Settings};

/// The STOP between passes of the loop: the last batch (its read settings, parameters, newest frame in the ring), the
/// view last read for, and whether arming is paused for a STOP (`Shared::hold`).
#[derive(Default)]
pub(super) struct StopRun {
    pub(super) last_batch: Option<(Settings, hz::Params, u32)>,
    replayed: Option<(f64, f64)>,
    pub(super) paused: bool,
}

impl StopRun {
    /// STOP: the batch in progress has been read. Once, before anything else of the STOP: disarm (a batch armed ahead
    /// writes the ring the views read again, so the last batch is gone with it), and the newest frame once more raw
    /// where the live compression or the deep measurements want it (the record that stays on screen). Both STOP ways go
    /// through here: the hold (PR #7) and the views read again (`view`). `last_read`: the batch read by the previous
    /// pass; `req`: the UI's final-frame request and what the deep measurements want, as read at the top of the pass.
    pub(super) fn hold<B: Bus, R: std::io::Read, C>(
        &mut self,
        io: Io<'_, B, R, C>,
        armed: &mut Option<(Settings, u64)>,
        last_read: Option<&(Settings, hz::Params, u32, bool)>,
        raw_dma: &mut DmaBuf,
        cores: &mut crate::board::dho::cores::Cores,
        req: (u64, crate::board::DeepWant),
    ) {
        let Io { state, bar, c2h, out, n, stamp, .. } = io;
        let (final_req, deep_want) = req;
        let ahead = armed.is_some();
        disarm(armed, state, bar, c2h);
        if ahead {
            self.last_batch = None;
        }
        let replay = last_read.and_then(|(lb, lp, count, raw)| {
            final_replay(lb, lp, *count, *raw, ahead, deep_want).map(|ds| (ds, *lp, *count))
        });
        let match_cores = out.lock().unwrap().cores;
        let last = replay.and_then(|(ds, lp, count)| {
            match seq::read_raw_again(state, &ds, &lp, bar, c2h, raw_dma, count - 1, |_| {}) {
                Ok(()) => {
                    *n += 1;
                    Some(Frame { stamp, ..decode_with(raw_dma.data(), &ds, &lp, *n, match_cores.then_some(cores)) })
                }
                Err(e) => {
                    report(out, format!("raw read of the deep window at STOP: {}", e));
                    None
                }
            }
        });
        let mut g = out.lock().unwrap();
        if let Some(f) = last {
            g.frame = f.clone();
            g.queue.push_back(f);
        }
        g.final_ack = final_req;
        drop(g);
        self.paused = true;
    }

    /// STOP (normal acquisitions only): nothing is armed, the record in the ring stays; for the view `v` (time per
    /// division, delay) the newest frame of the last batch is read again (`stop_plan`), once per view; settings changed
    /// meanwhile wait for RUN.
    pub(super) fn view<P: port::Port>(
        &mut self,
        v: (f64, f64),
        io: Io<'_, P::Bus, P::Stream, P::Clock>,
        lines: &mut P::Lines,
        cores: &mut crate::board::dho::cores::Cores,
    ) {
        use oos_hal::dho::boot::Clock;
        let Io { state, bar, c2h, clock, out, n, stamp } = io;
        // a final-frame request of the UI while the views are read again: answered as the hold does
        {
            let mut g = out.lock().unwrap();
            g.final_ack = g.final_req;
        }
        match &self.last_batch {
            Some((lb, lp, idx)) if self.replayed != Some(v) => {
                self.replayed = Some(v);
                let (rb, ti) = stop_plan(lb, lp, v.0, v.1);
                let steps = state.reread_steps(&rb, lp, *idx);
                match execute::<P>(&steps, bar, None, c2h, lines, clock) {
                    Ok(bytes) => {
                        let fb = seq::frame_bytes(&rb, lp);
                        *n += 1;
                        let m = out.lock().unwrap().cores.then_some(cores);
                        let f = Frame {
                            stamp,
                            fixed_ti: Some(ti),
                            ..decode_with(&bytes[..fb.min(bytes.len())], &rb, lp, *n, m)
                        };
                        out.lock().unwrap().replay = Some(f);
                    }
                    Err(e) => report(out, format!("STOP read: {}", e)),
                }
            }
            _ => clock.sleep_us(20000),
        }
    }

    /// No view to read for (running, or roll and scan): the next view asked for is read again.
    pub(super) fn no_view(&mut self) {
        self.replayed = None;
    }
}
