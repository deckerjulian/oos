// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware source: acquisitions through `oos_hal::dho::seq` (configure on change, arm, wait, read the whole record),
//! decoded into volts per channel. The UI aligns each frame with the software trigger engine (all 12 trigger types on
//! real data; the hardware itself triggers with an edge on the source at the level, in AUTO or NORMAL).

mod decode;
mod plan;
mod seg;
mod serial;
mod settings;
#[cfg(test)]
mod tests;
mod trim;
mod units;

#[cfg(any(target_os = "linux", test))]
pub use self::decode::{codes_into, decode, decode_into};
#[cfg(target_os = "linux")]
use self::decode::{decode_with, la_record, POOL};
#[cfg(target_os = "linux")]
use self::plan::{
    batch_plan, deep_raw_settings, disarm, edge_check, final_replay, frame0_check_wanted, frame0_drop, frame0_left_out,
    frame1_left_out, frame1_off_trigger, prearm_keeps_replay, prearm_on, stop_plan, FIRST_LEFT_MAX_S, FRAME1_CHECK_MAX,
};
#[cfg(target_os = "linux")]
use self::seg::{seg_abort, seg_step, SegRun};
pub use self::serial::serial_debug;
#[cfg(any(target_os = "linux", test))]
pub use self::settings::roll_la_t0;
pub use self::settings::{avg_hw_ok, seg_max, settings, slow_mode, ROLL_TDIV};
#[cfg(target_os = "linux")]
use self::settings::{avg_kind, raw_for_ui, raw_newest, single_wants_more};
#[cfg(target_os = "linux")]
use self::trim::{core_offset_tight, core_trim, load_overlay, overlay_in_use, TrimPending, TRIM_TRIES};
#[cfg(target_os = "linux")]
use self::units::{
    counter_tick, dvm_reading, dvm_step, hw_merge, hw_status_on, levels_measure, unit_key, DvmKey, DvmStep, UnitGuard,
};
pub use self::units::{dvm_hw_on, mask_hw_on, mask_map, mask_map_set, zone_hw_on};
use super::cores;
#[cfg(target_os = "linux")]
use crate::board::{Frame, Slow};
#[cfg(target_os = "linux")]
use crate::board::{HwShared, Shared, QUEUE_MAX, QUEUE_SAMPLES};
use crate::model::*;
#[cfg(target_os = "linux")]
use oos_hal::dho::seq::Settings;
#[cfg(target_os = "linux")]
use oos_hal::dho::{afe, hz, roll};
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

/// Starts the acquisition thread (`app_mask` = channel set the stock system left behind; `program_afe` = OOS sets the
/// AFEs from the UI channel settings, else they keep the stock set-up and the UI V/div must match it).
#[cfg(target_os = "linux")]
pub fn spawn(app_mask: u32, program_afe: bool, cal_dir: String, start_up: Option<String>) -> HwShared {
    use oos_hal::dho::bus::{Bar1, Bus, FPGA_VERSION};
    use oos_hal::dho::{adc, seq};
    let shared: HwShared = Arc::new(Mutex::new(Shared::default()));
    let out = shared.clone();
    std::thread::spawn(move || {
        crate::placement::place(crate::placement::Role::Hw);
        let status = |s: String| {
            eprintln!("oos-ui hw: {}", s);
            out.lock().unwrap().status = s;
        };
        // started with the start-up (rcS): nothing of the hardware is touched before its `end` line, written after
        // oos-fpgad cold-boot exited (its GPIO lines are free then)
        if let Some(path) = &start_up {
            status("waiting for the start-up".into());
            loop {
                let v = oos_hal::bootstatus::parse(&std::fs::read_to_string(path).unwrap_or_default());
                match v.finished {
                    Some(Ok(())) => break,
                    Some(Err(e)) => return status(format!("acquisition hardware not started: {}", e)),
                    None => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        }
        let mut bar = match Bar1::open() {
            Ok(b) => b,
            Err(e) => return status(format!("/dev/xdma0_bypass: {}", e)),
        };
        let v = bar.rd(0x4000);
        out.lock().unwrap().fpga_version = v;
        if v != FPGA_VERSION {
            return status(format!("unexpected FPGA version {:#x}, not touching it", v));
        }
        let mut c2h = match std::fs::File::open("/dev/xdma0_c2h_0") {
            Ok(f) => f,
            Err(e) => return status(format!("/dev/xdma0_c2h_0: {}", e)),
        };
        let mut afe_bus = if program_afe {
            match afe::GpioAfe::open() {
                Ok(b) => Some(b),
                Err(e) => return status(format!("AFE GPIO: {}", e)),
            }
        } else {
            None
        };
        let mut cal = match afe::Cal::load_from(&cal_dir) {
            Ok((c, from)) => {
                status(format!(
                    "calibration: {}",
                    from.iter().map(|(n, d)| format!("{} from {}", n, d)).collect::<Vec<_>>().join(", ")
                ));
                Some(c)
            }
            Err(_) => None,
        };
        let mut cal_adc = adc::CalAdc::load(&cal_dir).ok();
        // the matched core offsets: only with `cal.core_offset = tight` (key off = the cal_adc.hex above, unchanged)
        let mut trim_overlay = overlay_in_use(core_offset_tight(), load_overlay);
        if let Some(c) = cal_adc.as_mut() {
            trim_overlay.apply(c);
        }
        // LA pod zero codes (cal_la.hex, read only); without the file the middle of the DAC range (la.md 8.1)
        let la_cal = if oos_hal::model::platform().caps.la {
            oos_hal::dho::la::Cal::load(&cal_dir).unwrap_or_else(|| {
                status("cal_la.hex not found: LA thresholds with the zero code 0x2180".into());
                oos_hal::dho::la::Cal { zero: [0x2180; 2] }
            })
        } else {
            oos_hal::dho::la::Cal::default()
        };
        let mut state = seq::State::handover(app_mask, cal_adc.as_ref());
        let mut current: Option<(Settings, hz::Params)> = None;
        let mut n = 0u64;
        // roll/scan mode: streaming state (parameters, ring of volts per channel, last publish)
        let mut rolling: Option<roll::Params> = None;
        let mut ring: [std::collections::VecDeque<f32>; NCH] = Default::default();
        // the same for the LA words: ring, sweep picture (as f32 for `scan_write`; a word is exact in it)
        let mut ring_la: std::collections::VecDeque<u16> = Default::default();
        let mut sweep_la: Vec<f32> = Vec::new();
        let mut published = std::time::Instant::now();
        // scan: the sweep picture per channel (one screen), write position, last 0x1204, SINGLE sweep complete
        let mut sweep: [Vec<f32>; NCH] = Default::default();
        let mut cursor = 0usize;
        let mut last_info = roll::ScanInfo::default();
        let mut halted = false;
        // 0x40d0 reported done; the last chunks of the sweep are read once more before the stop
        let mut done_seen: Option<std::time::Instant> = None;
        let mut mode = Slow::Off;
        let afg_cal = oos_hal::dho::afg::Cal::load(&cal_dir);
        let mut afg_io: Option<oos_hal::dho::afg::Io> = None;
        let mut afg_done: Option<(bool, f64, f64, f64, oos_hal::dho::afg::Wave, oos_hal::dho::afg::Mod)> = None;
        let mut stamp;
        let mut xadc_at: Option<std::time::Instant> = None;
        let mut la_fails = 0u32;
        // STOP: the last batch (its read settings, parameters, newest frame in the ring) and the view last read for
        let mut last_batch: Option<(Settings, hz::Params, u32)> = None;
        let mut replayed: Option<(f64, f64)> = None;
        // the learnt offsets of the ADC cores (`cores`)
        let mut cores = super::cores::Cores::default();
        // the read buffer of the batches, kept (bus::DmaBuf)
        let mut dma = oos_hal::dho::bus::DmaBuf::new();
        // the raw second read of the newest frame of a compressed batch (compress.rs)
        let mut raw_dma = oos_hal::dho::bus::DmaBuf::new();
        // the last raw read of a deep record's screen window for the measurements (deepmeas.rs)
        let mut deep_last: Option<std::time::Instant> = None;
        // the settings of the last batch read, and whether its record was left out as the first with new ones
        let mut read_with: Option<Settings> = None;
        let mut first_left = false;
        // the last raw read of the newest frame of a live-compressed batch (compress.rs, at most one per RAW_PERIOD)
        let mut comp_last: Option<std::time::Instant> = None;
        // the batch read by the previous pass and nothing since (settings, parameters, frames, newest frame read
        // raw), and whether arming is paused for a STOP (`Shared::hold`, docs/fork/stop-exact.md)
        let mut fresh: Option<(Settings, hz::Params, u32, bool)> = None;
        let mut paused = false;
        // read-out buffers made so far: this one, and up to `POOL` in all lent to the deposit workers in batch mode
        let mut pool_made = 1usize;
        // this thread's share of the deposit work while it waits (drawpipe.rs)
        let mut helper = crate::drawpipe::Helper::default();
        // the batch armed ahead (`prearm_on`) and the SINGLE stamp it was armed under
        let mut armed: Option<(Settings, u64)> = None;
        // the SINGLE stamp whose batch brought its frame (`single_wants_more`)
        let mut single_done: Option<u64> = None;
        // the hardware segmented recording in progress or read back
        let mut segrun: Option<SegRun> = None;
        // hardware average: triggers since the last clear, the `Shared::avg_clear` it was cleared for, batches logged
        let mut avg: Option<(oos_hal::dho::avg::Tracker, u64, u32)> = None;
        // the frequency counter: what runs since when, and the readings so far
        let mut counting: Option<(crate::board::CounterWant, std::time::Instant)> = None;
        let mut count_n = 0u64;
        // the DVM block: what it runs on since when, the readings so far, and the number of the configurations applied
        let mut dvm_run: Option<(DvmKey, std::time::Instant)> = None;
        let mut dvm_n = 0u64;
        let mut configs = 0u64;
        // the mask/zone unit's stall guard
        let mut guard = UnitGuard::default();
        // the last request of the measure-unit levels answered
        let mut levels_done: Option<u64> = None;
        // `cal.core_offset = tight`: ADC modes whose core offsets were matched this session, and tries per mode
        let mut trim_done = [false; 11];
        let mut trim_tries = [0u8; 11];
        let mut trim_pending: Option<TrimPending> = None;
        let mut slots_done: Option<u64> = None;
        loop {
            let last_read = fresh.take();
            counter_tick(&mut bar, &out, &mut counting, &mut count_n);
            // FPGA die temperature and VCCINT for the System info page (one register read, about once a second)
            if xadc_at.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1)) {
                xadc_at = Some(std::time::Instant::now());
                let x = oos_hal::dho::board::read_xadc(&mut bar);
                out.lock().unwrap().xadc = x;
                // LA probe detect, also with the LA off: without a probe it cannot be switched on
                if oos_hal::model::platform().caps.la {
                    let p = oos_hal::dho::la::probe_present().ok();
                    out.lock().unwrap().la_probe = p;
                }
            }
            // self-calibration (selfcal.rs): owns the FPGA and the AFEs until it is done; the result goes to /oos/cal
            // (the request is taken in its own statement: a lock in the `if let` scrutinee would be held for the whole
            // run, and the cancel check and the UI would wait on it forever)
            let req = out.lock().unwrap().cal_req.take();
            if let Some((chans, zero, adc_cores)) = req {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &out);
                if rolling.take().is_some() {
                    bar.apply(&roll::stop(&mut state.sh));
                }
                let res = match (afe_bus.as_mut(), cal.as_ref()) {
                    (Some(afe), Some(c)) => {
                        let cancel_out = out.clone();
                        let cancel = move || cancel_out.lock().unwrap().cal_cancel;
                        let mut unit = oos_hal::dho::selfcal::Unit {
                            bar: &mut bar,
                            afe,
                            c2h: &mut c2h,
                            state: &mut state,
                            cal: c,
                            cal_adc: cal_adc.as_ref(),
                            cancel: &cancel,
                            adc_new: None,
                        };
                        let prog_out = out.clone();
                        let adc_base = if adc_cores { cal_adc.as_ref() } else { None };
                        let r = oos_hal::dho::selfcal::run_all_with(
                            &mut unit,
                            c,
                            adc_base,
                            &chans,
                            zero,
                            &mut |d, t, s| {
                                prog_out.lock().unwrap().cal_progress = Some((d, t, s.to_string()));
                            },
                        );
                        match r {
                            Ok(r) => oos_hal::dho::selfcal::save(&r, &cal_dir).map(|files| {
                                format!(
                                    "{} configurations, largest change {:.2} %, saved {}",
                                    r.measured,
                                    r.worst * 100.0,
                                    files.join(", ")
                                )
                            }),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err("self-cal needs the AFE bus (--afe) and the calibration files".into()),
                };
                status(format!(
                    "self-cal CH{:?}: {}",
                    chans.iter().map(|c| c + 1).collect::<Vec<_>>(),
                    match &res {
                        Ok(s) => format!("done, {}", s),
                        Err(e) => format!("failed: {}", e),
                    }
                ));
                // the new tables take effect at the next set-up
                if res.is_ok() {
                    cal = afe::Cal::load(&cal_dir).ok();
                    cal_adc = adc::CalAdc::load(&cal_dir).ok();
                    // the matched offsets over the new trims; every mode is matched again from there
                    if let Some(c) = cal_adc.as_mut() {
                        trim_overlay.apply(c);
                    }
                    (trim_done, trim_tries) = ([false; 11], [0; 11]);
                }
                current = None;
                let mut g = out.lock().unwrap();
                (g.cal_progress, g.cal_result, g.cal_cancel) = (None, Some(res), false);
                continue;
            }
            // built-in generator (afg.rs): apply a changed request; the GPIO lines stay open from the first request on,
            // which is "off" at start: an output left running (oos-fpgad afg, an earlier oos-ui) goes off as at power-on
            let (afg_want, stamp_now) = {
                let g = out.lock().unwrap();
                (g.afg[0], g.stamp)
            };
            if afg_want != afg_done {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &out);
                // models without the generator (non-S): its GPIO lines and FPGA block are never touched
                if let Some((on, f, a, o, wave, md)) = afg_want.filter(|_| oos_hal::model::platform().caps.has_gen()) {
                    if afg_io.is_none() {
                        afg_io =
                            oos_hal::dho::afg::Io::open().map_err(|e| status(format!("generator GPIO: {}", e))).ok();
                    }
                    if let Some(io) = afg_io.as_mut() {
                        // a running output keeps running when the change allows it
                        let quick = afg_done
                            .filter(|d| on && d.0 && d.4 == wave && d.5 == md && md == oos_hal::dho::afg::Mod::None)
                            .and_then(|d| {
                                oos_hal::dho::afg::retune(
                                    afg_cal.as_ref(),
                                    wave,
                                    d.1 * 1e6,
                                    f * 1e6,
                                    a * 1e9,
                                    o * 1e9,
                                    io.relays(),
                                )
                            });
                        let ops = if let Some(q) = quick {
                            q
                        } else if on {
                            oos_hal::dho::afg::start_mod(
                                afg_cal.as_ref(),
                                wave,
                                f * 1e6,
                                a * 1e9,
                                o * 1e9,
                                io.relays(),
                                md,
                            )
                        } else {
                            oos_hal::dho::afg::idle(a * 1e9, o * 1e9, f * 1e6, io.relays())
                        };
                        io.apply(&mut bar, &ops);
                        status(format!(
                            "generator: {}",
                            if on { format!("{:?} {} Hz {} Vpp {} V {:?}", wave, f, a, o, md) } else { "off".into() }
                        ));
                    }
                }
                afg_done = afg_want;
            }
            // read with the generator request; the settings read below are as new or newer
            stamp = stamp_now;
            let (want, want_mode, avg_clear, mask_want, zone_want, needs_raw) = {
                let g = out.lock().unwrap();
                (g.want.clone(), g.slow, g.avg_clear, g.mask_want.clone(), g.zone_want.clone(), g.needs_raw)
            };
            let Some(mut want) = want else {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            };
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
                let mut g = out.lock().unwrap();
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
            // STOP: the batch in progress has been read. Once, before anything else of the STOP: disarm (a batch armed
            // ahead writes the ring the views below read again, so the last batch is gone with it), and the newest
            // frame once more raw where the live compression or the deep measurements want it (the record that stays
            // on screen). Both STOP ways go through here: the hold (PR #7) and the views read again (`stop_view`)
            let (hold, final_req, deep_want) = {
                let g = out.lock().unwrap();
                (g.hold, g.final_req, g.deep_raw)
            };
            if hold && !paused {
                let ahead = armed.is_some();
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                if ahead {
                    last_batch = None;
                }
                let replay = last_read.as_ref().and_then(|(lb, lp, count, raw)| {
                    final_replay(lb, lp, *count, *raw, ahead, deep_want).map(|ds| (ds, *lp, *count))
                });
                let match_cores = out.lock().unwrap().cores;
                let last = replay.and_then(|(ds, lp, count)| {
                    match seq::read_raw_again(&mut state, &ds, &lp, &mut bar, &mut c2h, &mut raw_dma, count - 1, |_| {})
                    {
                        Ok(()) => {
                            n += 1;
                            Some(Frame {
                                stamp,
                                ..decode_with(raw_dma.data(), &ds, &lp, n, match_cores.then_some(&mut cores))
                            })
                        }
                        Err(e) => {
                            status(format!("raw read of the deep window at STOP: {}", e));
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
                paused = true;
            }
            // STOP (normal acquisitions only): nothing is armed, the record in the ring stays; for every new view
            // the newest frame of the last batch is read again (`stop_plan`), settings changed meanwhile wait for RUN
            let stop_view = out.lock().unwrap().stop_view;
            match stop_view {
                Some(v) if mode == Slow::Off && want_mode == Slow::Off => {
                    // a final-frame request of the UI while the views are read again: answered as the hold does
                    {
                        let mut g = out.lock().unwrap();
                        g.final_ack = g.final_req;
                    }
                    match &last_batch {
                        Some((lb, lp, idx)) if replayed != Some(v) => {
                            replayed = Some(v);
                            let (rb, ti) = stop_plan(lb, lp, v.0, v.1);
                            let steps = state.reread_steps(&rb, lp, *idx);
                            match seq::execute(&steps, &mut bar, None, &mut c2h, |_| {}) {
                                Ok(bytes) => {
                                    let fb = seq::frame_bytes(&rb, lp);
                                    n += 1;
                                    let m = out.lock().unwrap().cores.then_some(&mut cores);
                                    let f = Frame {
                                        stamp,
                                        fixed_ti: Some(ti),
                                        ..decode_with(&bytes[..fb.min(bytes.len())], &rb, lp, n, m)
                                    };
                                    out.lock().unwrap().replay = Some(f);
                                }
                                Err(e) => status(format!("STOP read: {}", e)),
                            }
                        }
                        _ => std::thread::sleep(std::time::Duration::from_millis(20)),
                    }
                    continue;
                }
                _ => replayed = None,
            }
            if current.as_ref().is_none_or(|c| c.0 != want) || mode != want_mode {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &out);
                mode = want_mode;
                last_batch = None;
                if rolling.take().is_some() {
                    bar.apply(&roll::stop(&mut state.sh));
                }
                match state.configure(&want, cal.as_ref(), cal_adc.as_ref()) {
                    Ok((steps, p)) => {
                        if let Err(e) = seq::execute(&steps, &mut bar, afe_bus.as_mut(), &mut c2h, |_| {}) {
                            status(format!("configure: {}", e));
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            continue;
                        }
                        la_fails = 0;
                        status(format!("configured: mask {:#x}, {} Sa/s, {} points", want.mask, p.rate, p.memsel));
                        current = Some((want.clone(), p));
                        configs += 1;
                        // core offsets of the ADC mode (opt-in): once this configuration has stood for TRIM_HOLD; a new
                        // configuration replaces the wait
                        let m = state.mode.min(10);
                        trim_pending = (core_offset_tight() && !trim_done[m] && trim_tries[m] < TRIM_TRIES)
                            .then(|| TrimPending { mode: m, mask: want.mask, since: std::time::Instant::now() });
                        // the configuration cleared the average (`seq::State::configure`)
                        avg = match want.mode {
                            oos_hal::dho::acq::Mode::Average { count } => {
                                Some((oos_hal::dho::avg::Tracker::new(avg_kind(), count), avg_clear, 0))
                            }
                            _ => None,
                        };
                    }
                    Err(e) => {
                        status(format!("settings: {}", e));
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    }
                }
            }
            let (s, p) = current.clone().unwrap();
            // a hardware segment recording in the FPGA: the measure unit (DVM, levels, slots) and the ADC core offsets
            // wait until it has ended. `disarm` does not stop it, and a write in between would change the segments
            // still to come (H4 of the 0.1.13 review)
            let seg_recording = segrun.as_ref().is_some_and(|r| r.recording);
            // the DVM block (`dvm.hw = on`): (re)started while nothing is armed, read like the counter
            if !seg_recording {
                use oos_hal::dho::dvm;
                let want = out.lock().unwrap().dvm_want;
                let elapsed = dvm_run.map_or(0.0, |r| r.1.elapsed().as_secs_f64());
                let (step, key) = dvm_step(want, s.mask, configs, dvm_run.map(|r| r.0), elapsed);
                match step {
                    DvmStep::Start(lane) => {
                        disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                        bar.apply(&dvm::start(lane, dvm::EXP));
                        out.lock().unwrap().dvm = None;
                    }
                    DvmStep::Stop => {
                        disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                        bar.apply(&dvm::stop());
                        out.lock().unwrap().dvm = None;
                    }
                    DvmStep::Read => {
                        let raw = dvm::read(&mut bar);
                        dvm_n += 1;
                        let r = key.map(|k| dvm_reading(&raw, k, &s, &state.gain, dvm_n));
                        out.lock().unwrap().dvm = r;
                    }
                    DvmStep::Idle => {}
                }
                if step != DvmStep::Idle {
                    dvm_run = key.map(|k| (k, std::time::Instant::now()));
                }
            }
            // levels for the auto set-up from the measure unit, once the settings they ask for are applied; the unit
            // is set up while nothing is armed (configuration crosses into the ADC clock without synchronisers)
            let lw = out.lock().unwrap().levels_want;
            if let Some((id, _, runs)) =
                lw.filter(|w| mode == Slow::Off && !seg_recording && stamp >= w.1 && levels_done != Some(w.0))
            {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                let v = levels_measure(&mut bar, &s, &state.gain, runs);
                out.lock().unwrap().levels = Some((id, v));
                levels_done = Some(id);
            }
            // the core trim of the current configuration, with nothing armed
            if let Some(t) =
                trim_pending.filter(|t| mode == Slow::Off && !seg_recording && t.due(std::time::Instant::now()))
            {
                trim_pending = None;
                // not while the generator runs: its signal is locked to the ADC clock (coretrim gate)
                let gen_on = afg_done.is_some_and(|d| d.0);
                if !gen_on && t.mode == state.mode.min(10) {
                    if let Some(c) = cal_adc.as_mut() {
                        disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                        trim_tries[t.mode] += 1;
                        trim_done[t.mode] = core_trim(&mut bar, &mut state, c, &mut trim_overlay, t.mask);
                    }
                }
            }
            // debug: one raw run of the measure unit for `:DEBug:SLOTs?`
            let sw = out.lock().unwrap().slots_want;
            if let Some(id) = sw.filter(|&id| mode == Slow::Off && !seg_recording && slots_done != Some(id)) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                let r = oos_hal::dho::levels::run(&mut bar, &mut |us| {
                    std::thread::sleep(std::time::Duration::from_micros(us as u64))
                });
                out.lock().unwrap().slots =
                    Some((id, r.map(|r| r.map(|x| [x.min as f32, x.max as f32, x.mean as f32]))));
                slots_done = Some(id);
            }
            // :TFORce (from PR #8): a pulse of the force bit on the shadows now in force, then back. Not part of the
            // set-up, so it does not reconfigure. Pulsed between batches: a batch already waiting for its trigger
            // takes it at its next arm
            if std::mem::take(&mut out.lock().unwrap().force) {
                bar.apply(&oos_hal::dho::tpu::force(&state.ts));
            }
            if mode != Slow::Off {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                // roll and scan (roll-mode.md): stream chunks; roll keeps 10 divisions and puts the newest point at the
                // right edge, scan writes the screen from left to right; publish ~30 pictures per second
                let tdiv_fs = seq::tdiv_fs(s.tdiv);
                let scan = match mode {
                    Slow::Scan { delay, single } => Some(((delay * 1e15) as i128, single)),
                    _ => None,
                };
                // LA on: its words come in chunks of their own in the same stream (la.md 4.4)
                let la_t = s.la.is_some().then(|| oos_hal::dho::la::timing(&p, s.mask));
                let sweep_start = |state: &mut seq::State, bar: &mut oos_hal::dho::bus::Bar1, rp: &roll::Params| {
                    match scan {
                        Some((delay_fs, _)) => {
                            let pos = roll::scan_trig_pos(delay_fs, tdiv_fs, p.dot_fs);
                            bar.apply(&roll::config_scan(&mut state.sh, rp, p.memsel, pos));
                        }
                        None => bar.apply(&roll::config(&mut state.sh, rp, p.memsel)),
                    }
                    if let Some(t) = &la_t {
                        bar.apply(&roll::la_config(rp, t));
                    }
                    bar.apply(&roll::start(&mut state.sh, s.mask));
                };
                let rp = match rolling {
                    Some(rp) => rp,
                    None => {
                        let rp = roll::params(s.mask, tdiv_fs, s.depth, &p);
                        sweep_start(&mut state, &mut bar, &rp);
                        ring.iter_mut().for_each(|r| r.clear());
                        sweep.iter_mut().for_each(|v| v.clear());
                        ring_la.clear();
                        sweep_la.clear();
                        (cursor, halted, done_seen, last_info) = (0, false, None, Default::default());
                        let name = if scan.is_some() { "scan" } else { "roll" };
                        status(format!(
                            "{}: {:.1} us per point, chunk {:.2} ms",
                            name,
                            rp.spacing_fs as f64 / 1e9,
                            rp.spacing_fs as f64 * 100.0 / rp.ratio as f64 / 1e12
                        ));
                        rolling = Some(rp);
                        // roll and scan stream raw points: no batch, so the state line's factor is set here
                        crate::compress::note_batch_k(1);
                        rp
                    }
                };
                if halted {
                    // SINGLE scan complete: wait for the next SINGLE press
                    let go = std::mem::take(&mut out.lock().unwrap().rearm);
                    if !go {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    sweep_start(&mut state, &mut bar, &rp);
                    (cursor, halted) = (0, false);
                }
                let dt = rp.spacing_fs as f64 / 1e15;
                let keep = ((10.0 * s.tdiv / dt).round() as usize).max(2);
                let la_dt = if la_t.is_some() { roll::la_spacing_fs(&rp) as f64 / 1e15 } else { dt };
                let keep_la = ((10.0 * s.tdiv / la_dt).round() as usize).max(2);
                bar.apply(&roll::fill_pulse(&state.sh));
                let c = roll::chunks(bar.rd(0x11e8));
                let c = if la_t.is_some() { roll::la_chunks(c) } else { c };
                if c == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(9));
                } else {
                    let buf = match oos_hal::dho::bus::read_dma(&mut c2h, c * roll::CHUNK_BYTES, |_, _| {}) {
                        Ok(b) => b,
                        Err(e) => {
                            status(format!(
                                "{} read of {} chunks: {}",
                                if scan.is_some() { "scan" } else { "roll" },
                                c,
                                e
                            ));
                            rolling = None;
                            current = None;
                            continue;
                        }
                    };
                    let (w, lw) = if la_t.is_some() {
                        roll::split_la(&buf, s.mask)
                    } else {
                        (roll::split(&buf, s.mask), Vec::new())
                    };
                    if scan.is_some() {
                        // the chunk counter restarts with every sweep: its column is the write position of the first
                        // analog chunk
                        let per = roll::points_per_chunk(s.mask);
                        let chunks = buf.as_chunks::<{ roll::CHUNK_BYTES }>().0;
                        let first = chunks.iter().map(|c| &c[..]).find(|c| !roll::is_la_chunk(c));
                        let first = first.unwrap_or(&buf[..roll::CHUNK_BYTES]);
                        let col = roll::scan_column(roll::counter(first), la_t.is_some());
                        cursor = (col * per) % keep;
                        if !lw.is_empty() {
                            let v: Vec<f32> = lw.iter().map(|&x| x as f32).collect();
                            scan_write(&mut sweep_la, keep_la, scan_la_pos(cursor, keep, keep_la), &v);
                        }
                        let fresh = w.iter().map(|v| v.len()).max().unwrap_or(0);
                        for ch in 0..NCH {
                            if w[ch].is_empty() {
                                sweep[ch].clear();
                                continue;
                            }
                            scan_write(
                                &mut sweep[ch],
                                keep,
                                cursor,
                                &seq::volts(&w[ch], s.ch[ch].scale, s.ch[ch].offset),
                            );
                        }
                        cursor = (cursor + fresh) % keep;
                    } else {
                        ring_la.extend(lw.iter().copied());
                        while ring_la.len() > keep_la {
                            ring_la.pop_front();
                        }
                        for ch in 0..NCH {
                            if w[ch].is_empty() {
                                ring[ch].clear();
                                continue;
                            }
                            ring[ch].extend(seq::volts(&w[ch], s.ch[ch].scale, s.ch[ch].offset));
                            while ring[ch].len() > keep {
                                ring[ch].pop_front();
                            }
                        }
                    }
                }
                // scan: after every read, also an empty one
                let mut done = false;
                if let Some((_, single)) = scan {
                    if done_seen.is_none() {
                        let r = roll::SCAN_INFO_REGS.map(|a| bar.rd(a));
                        let info = roll::scan_info(r[0], r[1]);
                        if info.triggered != last_info.triggered || info.done {
                            status(format!(
                                "scan: 0x1204 = {:#010x}, 0x40d0 = {:#x}, cursor {} of {}",
                                r[0], r[1], cursor, keep
                            ));
                        }
                        last_info = info;
                        // done arrives a few chunks before the end of the sweep (unit: 4 chunks): read on until the
                        // last column is in (cursor back at 0) or 100 ms passed
                        if info.done {
                            done_seen = Some(std::time::Instant::now());
                        }
                    }
                    if done_seen.is_some_and(|t| cursor == 0 || t.elapsed().as_millis() >= 100) {
                        status(format!(
                            "scan: sweep complete {} ms after done, cursor {} of {}",
                            done_seen.unwrap().elapsed().as_millis(),
                            cursor,
                            keep
                        ));
                        done_seen = None;
                        done = true;
                        // stock state 8 (RUN): stop, clear the average, configure again, next sweep after 500 ms; state 9
                        // (SINGLE): stop
                        bar.apply(&roll::scan_done(&mut state.sh));
                        halted = single;
                    }
                }
                if done || published.elapsed().as_millis() >= 33 {
                    published = std::time::Instant::now();
                    n += 1;
                    let f = if scan.is_some() {
                        let mut ch = sweep.clone();
                        if !done {
                            // a short gap ahead of the write position shows where the sweep is
                            scan_gap(&mut ch, cursor, keep / 50);
                        }
                        // word 0 at the left edge like analog point 0; only what the sweeps have written so far
                        let la = la_t.map(|_| crate::ui::la::LaRec {
                            words: Arc::new(sweep_la.iter().take_while(|x| !x.is_nan()).map(|&x| x as u16).collect()),
                            t0: 0.0,
                            dt: la_dt,
                        });
                        Frame {
                            seq: n,
                            dt,
                            pre: 0,
                            ch,
                            roll: false,
                            envelope: true,
                            scan: true,
                            partial: !done,
                            acq: (0.0, 0),
                            stamp,
                            la,
                            fixed_ti: None,
                            hw_avg: false,
                            basic: Default::default(),
                        }
                    } else {
                        let len = ring.iter().map(|r| r.len()).max().unwrap_or(0);
                        // the newest point at the right edge: index len - 1 sits half a screen right of `pre`
                        let pre = (len as f64 - 1.0 - 5.0 * s.tdiv / dt).max(0.0) as usize;
                        // the newest LA word next to the newest analog point, at the right edge
                        let la = la_t.map(|_| crate::ui::la::LaRec {
                            t0: roll_la_t0(len, pre, dt, ring_la.len(), la_dt),
                            words: Arc::new(ring_la.iter().copied().collect()),
                            dt: la_dt,
                        });
                        Frame {
                            seq: n,
                            dt,
                            pre,
                            ch: ring.clone().map(|r| r.into_iter().collect()),
                            roll: true,
                            envelope: true,
                            scan: false,
                            partial: len < keep,
                            acq: (0.0, 0),
                            stamp,
                            la,
                            fixed_ti: None,
                            hw_avg: false,
                            basic: Default::default(),
                        }
                    };
                    let mut g = out.lock().unwrap();
                    // a complete sweep must not be dropped by the next partial picture
                    if g.queue.back().is_none_or(|b| b.partial) {
                        g.queue.clear();
                    }
                    g.queue.push_back(f.clone());
                    g.frame = f;
                    g.halted = halted;
                }
                if done && !halted {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    sweep_start(&mut state, &mut bar, &rp);
                    cursor = 0;
                }
                continue;
            }
            if rolling.take().is_some() {
                bar.apply(&roll::stop(&mut state.sh));
            }
            if s.segments.is_some() {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                if let Err(e) =
                    seg_step(&mut segrun, &s, &p, &mut state, &mut bar, &mut c2h, &mut dma, &out, &mut n, stamp)
                {
                    status(format!("segments: {}", e));
                    seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &out);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                continue;
            }
            // STOP, after the hold above: no arming until RUN or SINGLE (the UI's final-frame request is answered there)
            if hold {
                out.lock().unwrap().final_ack = final_req;
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            if std::mem::take(&mut paused) {
                // RUN or SINGLE: the frames of the batch before the STOP are not new
                out.lock().unwrap().queue.clear();
            }
            let b = batch_plan(&s, &p);
            if !single_wants_more(&b, single_done, stamp) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            // hardware average: CLEAR, RUN or SINGLE start it over (nothing is armed ahead in this mode)
            if let Some((tr, seen, logged)) = avg.as_mut().filter(|a| a.1 != avg_clear) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                if let Err(e) = seq::execute(&state.avg_clear_steps(), &mut bar, None, &mut c2h, |_| {}) {
                    status(format!("average clear: {}", e));
                }
                tr.reset();
                (*seen, *logged) = (avg_clear, 0);
            }
            // one batch: arm, let the ring fill for up to 25 ms after the first frame, one play, one read
            // armed ahead with these settings and since the last SINGLE press: only the wait is left
            let ahead = armed.as_ref().is_some_and(|(a, st)| *a == b && *st == stamp);
            if !ahead {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
            }
            armed = None;
            let changed = || {
                let g = out.lock().unwrap();
                (g.want.as_ref(), g.slow) != (asked.0.as_ref(), asked.1) || g.stop_view.is_some()
            };
            // while the FPGA's ring fills this thread draws frames of the last batch (drawpipe.rs), one per look at
            // the frame count, so the read waits at most one frame longer
            let pipe = crate::drawpipe::pipe();
            let idle = || pipe.is_some_and(|x| x.help(&mut helper));
            let t_batch = std::time::Instant::now();
            let got = seq::acquire_batch_idle(
                &mut state,
                &b,
                &p,
                &mut bar,
                &mut c2h,
                &mut dma,
                ahead,
                25,
                |_| {},
                changed,
                idle,
            );
            let batch_ms = t_batch.elapsed().as_secs_f32() * 1e3;
            if let Some(x) = pipe {
                x.help_done(&mut helper);
            }
            match got {
                Ok(count) => {
                    guard.batch(false, false, unit_key(&b, mask_map()));
                    crate::compress::note_batch_k(if raw_newest(&b, &p) { b.compress } else { 1 });
                    // LA on: the second, raw read of the same record before anything else touches the FPGA
                    // after a few failures in a row the LA read stops until the next set-up: every failed read waits for
                    // its time-out, and the analog frames must not wait with it
                    let la_words = match b.la {
                        Some(_) if la_fails < 3 => {
                            match seq::acquire_la(&mut state, &b, &p, &mut bar, &mut c2h, |_| {}) {
                                Ok(w) => {
                                    la_fails = 0;
                                    Some(w)
                                }
                                Err(e) => {
                                    la_fails += 1;
                                    status(format!("LA read ({} of 3): {}", la_fails, e));
                                    None
                                }
                            }
                        }
                        _ => None,
                    };
                    // live compression: the newest frame once more, raw, before anything arms the FPGA again; at most
                    // once per compress::RAW_PERIOD (each read cost 7-9 % of the frames at 1 channel on the unit)
                    let raw_frame = if raw_newest(&b, &p)
                        && count > 0
                        && comp_last.is_none_or(|t| t.elapsed() >= crate::compress::RAW_PERIOD)
                    {
                        comp_last = Some(std::time::Instant::now());
                        // the flag first: a guard taken in the argument would be held for the whole decode
                        let mc = out.lock().unwrap().cores;
                        match seq::read_raw_again(
                            &mut state,
                            &b,
                            &p,
                            &mut bar,
                            &mut c2h,
                            &mut raw_dma,
                            count - 1,
                            |_| {},
                        ) {
                            Ok(()) => Some(decode_with(
                                raw_dma.data(),
                                &seq::raw_settings(&b),
                                &p,
                                0,
                                mc.then_some(&mut cores),
                            )),
                            Err(e) => {
                                status(format!("raw read of the newest frame: {}", e));
                                None
                            }
                        }
                    } else {
                        None
                    };
                    // deep record and mean-type measurements wanted: the screen window of the newest frame raw, at most
                    // once per deepmeas::PERIOD while running (deepmeas.rs)
                    let deep_want = out.lock().unwrap().deep_raw;
                    let deep = deep_want.filter(|_| count > 0).and_then(|w| {
                        deep_raw_settings(&b, &p).filter(|ds| {
                            crate::deepmeas::due(w, deep_last, std::time::Instant::now(), seq::frame_bytes(ds, &p))
                        })
                    });
                    let raw_frame = match deep {
                        Some(ds) if raw_frame.is_none() => {
                            deep_last = Some(std::time::Instant::now());
                            let mc = out.lock().unwrap().cores;
                            match seq::read_raw_again(
                                &mut state,
                                &ds,
                                &p,
                                &mut bar,
                                &mut c2h,
                                &mut raw_dma,
                                count - 1,
                                |_| {},
                            ) {
                                Ok(()) => Some(decode_with(raw_dma.data(), &ds, &p, 0, mc.then_some(&mut cores))),
                                Err(e) => {
                                    status(format!("raw read of the deep window: {}", e));
                                    None
                                }
                            }
                        }
                        _ => raw_frame,
                    };
                    fresh = Some((b.clone(), p, count, raw_frame.is_some()));
                    // trigger status of this batch from the SCU's sticky bits, then the stock status clear, before the next
                    // arm (prearm below) can record anything
                    if hw_status_on() {
                        let v = bar.rd(0x4034);
                        bar.apply(&oos_hal::dho::scu::clear_status(v));
                        let f = match oos_hal::dho::scu::fired(v) {
                            oos_hal::dho::scu::Fired::Trigger => Some(true),
                            oos_hal::dho::scu::Fired::Forced => Some(false),
                            oos_hal::dho::scu::Fired::None => None,
                        };
                        let mut g = out.lock().unwrap();
                        g.trig_fired = Some((g.trig_fired.map_or(1, |x| x.0 + 1), f));
                    }
                    // the mask unit's counters after every batch (docs/fork/hwmask.md)
                    let mr = state.mask_ctrl().zip(b.hwmask.as_ref()).zip(state.mask_run).map(|((ctrl, m), run)| {
                        let raw = oos_hal::dho::mask::read(&mut bar, ctrl);
                        (m.ch, run.0, ctrl, raw)
                    });
                    {
                        let mut g = out.lock().unwrap();
                        let n = g.mask.map_or(0, |r| r.n) + 1;
                        g.mask = mr.map(|(ch, slot, ctrl, raw)| crate::board::MaskRead {
                            ch,
                            slot,
                            ctrl,
                            raw: [raw.frames_lo, raw.frames_hi, raw.fails_lo, raw.fails_hi],
                            frames: raw.frames(),
                            fails: raw.fails(),
                            n,
                            scale: b.ch[ch].scale,
                            offset: b.ch[ch].offset,
                            gain: state.gain[ch],
                            points: p.memsel,
                            chans: b.mask,
                        });
                        // the table as loaded, for the column debug (`:SYSTem:DEBug:MASK:COLumns?`), when it changed
                        let table: Option<Vec<[u8; 4]>> = state
                            .mask_cols
                            .as_ref()
                            .filter(|_| mr.is_some())
                            .map(|t| t.iter().map(|k| [k.lo, k.hi, k.lo_en as u8, k.hi_en as u8]).collect());
                        if table.as_deref() != g.mask_table.as_deref().map(|t| &t[..]) {
                            g.mask_table = table.map(std::sync::Arc::new);
                        }
                    }
                    // the zone unit's status and qualified-frame counter (docs/fork/hwzone.md)
                    let zr = state
                        .zone_ctrl()
                        .zip(b.hwzone.as_ref())
                        .zip(state.zone_run)
                        .map(|((ctrl, z), run)| (z.ch, run.0, ctrl, oos_hal::dho::zone::read(&mut bar, ctrl)));
                    {
                        let mut g = out.lock().unwrap();
                        let n = g.zone.map_or(0, |r| r.n) + 1;
                        g.zone = zr.map(|(ch, slot, ctrl, r)| {
                            let c = r.counters;
                            crate::board::ZoneRead {
                                ch,
                                slot,
                                ctrl,
                                raw: [r.s4030, r.s4038, c.frames_lo, c.frames_hi, c.fails_lo, c.fails_hi],
                                frames: c.frames(),
                                armed: r.armed(),
                                pass: r.pass(),
                                in_cycle: r.in_cycle(),
                                state: r.state(),
                                n,
                                batch: (count, b.frames, batch_ms),
                            }
                        });
                    }
                    // hardware average: one frame, shown or not, the average cleared after a complete hold average
                    let after = avg.as_mut().map(|(tr, _, logged)| {
                        let a = tr.batch(count);
                        if *logged < 32 {
                            // hardware check: 0x10b0 progress and the first words next to the triggers counted here
                            *logged += 1;
                            // the newest frame's first words (every recorded frame was played)
                            let at = (count.max(1) as usize - 1) * seq::frame_bytes(&b, &p);
                            let w: Vec<String> = dma.data().get(at..).unwrap_or_default().as_chunks::<2>().0.iter().take(4).map(|x| format!("{:04x}", u16::from_le_bytes([x[0], x[1]]))).collect();
                            eprintln!("oos-ui hw: average {:?} batch {} of {} triggers, {} in, show {}, 0x10b0 = {}, words {}", tr.kind, *logged, count, a.triggers, a.show, oos_hal::dho::avg::progress(&mut bar), w.join(" "));
                        }
                        a
                    });
                    // the next batch records while this one is decoded (after the LA read, which needs this record);
                    // not in average mode, where a clear may have to come first, and not where the deep measurements
                    // read the newest frame raw (its ring must stay for the replay at STOP)
                    if count > 0 && b.single {
                        single_done = Some(stamp);
                    }
                    if prearm_on() && avg.is_none() && !b.single && prearm_keeps_replay(&b, &p, deep_want) {
                        match seq::prearm(&mut state, &b, &p, &mut bar, &mut c2h, |_| {}) {
                            Ok(()) => armed = Some((b.clone(), stamp)),
                            Err(e) => status(format!("prearm: {}", e)),
                        }
                    }
                    let fb = seq::frame_bytes(&b, &p);
                    // decode before taking the lock: the UI must not wait for 100 k-point conversions;
                    // every recorded frame was read; in average mode only the newest goes to the screen
                    let shown = match after {
                        Some(a) if !a.show => 0,
                        _ => count,
                    };
                    let avail = dma.data().len().checked_div(fb).map_or(0, |n| (shown as usize).min(n));
                    crate::drawpipe::Counters::add(&crate::drawpipe::counters().acquired, avail as u64);
                    // batch mode (drawpipe.rs): the newest frame to the queue, the others with the buffer itself to the
                    // deposit workers, the next batch read into a buffer they gave back
                    let pipe = crate::drawpipe::pipe().filter(|x| after.is_none() && avail > 1 && x.accepting());
                    // the raw newest frame stands for the newest compressed one (same record)
                    let raw_frame = raw_frame.filter(|_| avail as u32 == count);
                    let mut skip = 0usize;
                    // frame 0 of a small batch: kept when its edge is at the trigger point (issue #220), so a batch
                    // of two at 1 ms/div does not lose every other record
                    let f0_drop = after.is_none() && frame0_drop();
                    let f0_edge = frame0_check_wanted(avail, f0_drop, p.memsel)
                        .then(|| edge_check(&decode(&dma.data()[..fb], &b, &p, 0), &b));
                    let f0_left = frame0_left_out(avail, f0_drop, f0_edge);
                    let mut frames: Vec<Frame> = if let Some(pipe) = pipe {
                        n += avail as u64;
                        // frame 1, when the workers get it: left out when its edge is not at the trigger point
                        let f1_off = frame0_drop()
                            && avail > 2
                            && p.memsel <= FRAME1_CHECK_MAX
                            && frame1_off_trigger(&decode(&dma.data()[fb..2 * fb], &b, &p, 0), &b);
                        // the record of measurements: with the ADC-core correction, as on the path without batches
                        let mc = out.lock().unwrap().cores;
                        let newest = match raw_frame {
                            Some(f) => Frame { stamp, seq: n, ..f },
                            None => {
                                let last = &dma.data()[(avail - 1) * fb..avail * fb];
                                Frame { stamp, ..decode_with(last, &b, &p, n, mc.then_some(&mut cores)) }
                            }
                        };
                        let next =
                            pipe.take_buffer().and_then(|x| x.downcast::<oos_hal::dho::bus::DmaBuf>().ok()).map(|x| *x);
                        let next = next.or_else(|| {
                            (pool_made < POOL).then(|| {
                                pool_made += 1;
                                oos_hal::dho::bus::DmaBuf::new()
                            })
                        });
                        match next {
                            Some(next) => {
                                let full = std::mem::replace(&mut dma, next);
                                let (bs, ps) = (b.clone(), p);
                                let (cs, cp) = (b.clone(), p);
                                let codes: crate::drawpipe::CodesInto =
                                    Arc::new(move |c: &[u8], f: &mut Frame| codes_into(c, &cs, &cp, 0, f));
                                // frame 0 left out as on the path without batches (issue #5; unit 2026-11-16: the
                                // one stray frame of 7,075 in batch mode was frame 0 of its batch), unless its edge
                                // check kept it; a frame 1 off the trigger takes frame 0 with it (the workers take a
                                // run of frames)
                                skip = (f0_left || f1_off) as usize + f1_off as usize;
                                if f1_off {
                                    frame1_left_out();
                                }
                                pipe.feed_codes(
                                    Box::new(full),
                                    fb,
                                    skip,
                                    avail - 1,
                                    stamp,
                                    Arc::new(move |c: &[u8], f: &mut Frame| decode_into(c, &bs, &ps, 0, f)),
                                    Some(codes),
                                );
                            }
                            None => {
                                crate::drawpipe::Counters::add(&crate::drawpipe::counters().dropped, avail as u64 - 1)
                            }
                        }
                        // two frames: the newest is frame 1
                        if avail == 2 && frame0_drop() && frame1_off_trigger(&newest, &b) {
                            frame1_left_out();
                            skip += 1;
                            Vec::new()
                        } else {
                            vec![newest]
                        }
                    } else {
                        // only the newest frames the screen takes are decoded (`Shared::take`; on the fast cores,
                        // which the frame loop needs), in average mode the newest one
                        let (take, match_cores) = {
                            let g = out.lock().unwrap();
                            (if g.take == 0 { avail } else { g.take.min(avail) }, g.cores)
                        };
                        let take = if after.is_some() { take.min(1) } else { take };
                        // frame 0 of a batch is the frame the FPGA held across the stop, read-out and arm: recorded
                        // before this arm, sometimes joined with older memory (a stray trace; issue #5, mj1911). With
                        // more frames in the batch it is left out (1 of 26 at 1 kpts, 10 us/div); in a small batch only
                        // when its edge is not at the trigger point (`f0_left` above, issue #220)
                        let stale = f0_left as usize;
                        skip = (avail - take).max(stale);
                        n += skip as u64;
                        let mut all: Vec<Frame> = dma
                            .data()
                            .chunks_exact(fb.max(1))
                            .take(avail)
                            .skip(skip)
                            .map(|chunk| {
                                n += 1;
                                Frame {
                                    stamp,
                                    hw_avg: after.is_some(),
                                    ..decode_with(chunk, &b, &p, n, match_cores.then_some(&mut cores))
                                }
                            })
                            .collect();
                        // frame 1 without its edge at the trigger point: left out too, whether frame 0 before it was
                        // left out or kept by its edge check (`all` starts at frame `skip`)
                        let at1 = (f0_drop && avail > 1 && skip <= 1).then(|| 1 - skip);
                        if let Some(i) = at1.filter(|&i| all.get(i).is_some_and(|f| frame1_off_trigger(f, &b))) {
                            all.remove(i);
                            frame1_left_out();
                            skip += 1;
                        }
                        if let (Some(r), Some(last)) = (raw_frame.filter(|_| !all.is_empty()), all.last_mut()) {
                            *last = Frame { stamp, seq: last.seq, ..r };
                        }
                        all
                    };
                    // the first batch with new settings, of one frame (a deep record): that frame is frame 0, which the
                    // FPGA records before the arm (issue #5), and the first after a change can still be the older
                    // memory of the old settings (unit 2026-11-15: the whole screen window of a 10 M record was the
                    // 4 Vpp of the depth-Auto records before it). It is left out, never two in a row; not in SINGLE
                    // (its one record is the answer), not in hardware average, and only for a record shorter than
                    // `FIRST_LEFT_MAX_S`: at slow time bases the next record would keep the screen waiting as long
                    // again (1.25 s at 50 ms/div)
                    let first = read_with.as_ref() != Some(&b);
                    read_with = Some(b.clone());
                    // not in roll and scan: their pieces come in batches of one with settings of their own
                    let short = (p.memsel as f64) < FIRST_LEFT_MAX_S * p.rate as f64
                        && frames.first().is_some_and(|f| !f.roll && !f.scan);
                    if first
                        && !first_left
                        && short
                        && frames.len() == 1
                        && avail == 1
                        && !b.single
                        && after.is_none()
                        && frame0_drop()
                    {
                        skip += frames.len();
                        frames.clear();
                        first_left = true;
                        // the deep window read of this batch went with it: the next batch reads one at once
                        deep_last = None;
                    } else {
                        first_left = false;
                    }
                    if after.is_some_and(|a| a.clear) {
                        if let Err(e) = seq::execute(&state.avg_clear_steps(), &mut bar, None, &mut c2h, |_| {}) {
                            status(format!("average clear: {}", e));
                        }
                    }
                    if let (Some(w), Some(f)) = (la_words, frames.last_mut()) {
                        f.la = Some(la_record(w, &b, &p));
                    }
                    // the newest frame becomes the record: its six full-frame readings (PR #8, `BasicStats`) here, off
                    // the frame loop; the fast path folds the other frames itself
                    if let Some(f) = frames.last_mut() {
                        f.basic = std::array::from_fn(|c| crate::board::BasicStats::of(&f.ch[c]));
                    }
                    if count > 0 {
                        last_batch = Some((b.clone(), p, count - 1));
                    }
                    let mut g = out.lock().unwrap();
                    // the pipeline's counters take every frame read that is neither queued nor lent to the workers:
                    // the ones left out above, the ones the screen does not take, the oldest of a full queue
                    let dropped_before = g.dropped;
                    g.decoded += count as u64;
                    g.dropped += skip as u64;
                    g.queue.extend(frames);
                    let size = |f: &Frame| f.ch.iter().map(Vec::len).sum::<usize>();
                    let mut held: usize = g.queue.iter().map(size).sum();
                    // the newest frame stays whatever its size
                    while g.queue.len() > 1 && (g.queue.len() > QUEUE_MAX || held > QUEUE_SAMPLES) {
                        if let Some(f) = g.queue.pop_front() {
                            held -= size(&f);
                        }
                        g.dropped += 1;
                    }
                    crate::drawpipe::Counters::add(
                        &crate::drawpipe::counters().dropped,
                        (g.dropped - dropped_before) as u64,
                    );
                    if let Some(f) = g.queue.back() {
                        g.frame = f.clone();
                    }
                    drop(g);
                }
                // new settings (or STOP) while it waited for a trigger: at once to them. The arm has filled frame 0
                // of the ring with pre-trigger samples (it stops only before the first trigger): a stored record
                // there is gone, later frames of the last batch are untouched
                Err(e) if e == seq::NEW_SETTINGS => {
                    if last_batch.as_ref().is_some_and(|l| l.2 == 0) {
                        last_batch = None;
                    }
                }
                Err(e) => {
                    if last_batch.as_ref().is_some_and(|l| l.2 == 0) {
                        last_batch = None;
                    }
                    status(format!("acquire: {}", e));
                    // the mask/zone unit stalled the arm: drop it for these settings (the next loop reconfigures)
                    let unit = state.mask_ctrl().is_some() || state.zone_ctrl().is_some();
                    if guard.batch(e.contains("no acquisition"), unit, unit_key(&b, mask_map())) {
                        status("mask/zone unit: no acquisition twice, software fallback for these settings".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
    });
    shared
}

/// Writes the scan points `v` into the sweep picture (`width` points) from `at`, wrapping at the right edge.
fn scan_write(pic: &mut Vec<f32>, width: usize, at: usize, v: &[f32]) {
    if pic.len() != width {
        *pic = vec![f32::NAN; width];
    }
    for (i, &y) in v.iter().enumerate() {
        pic[(at + i) % width] = y;
    }
}

/// Where the LA words of a chunk go in the sweep picture: the same place on the screen as the analog point `cursor`.
fn scan_la_pos(cursor: usize, keep: usize, keep_la: usize) -> usize {
    ((cursor as f64 * keep_la as f64 / keep.max(1) as f64).round() as usize) % keep_la.max(1)
}

/// Blanks `gap` points from the write position (the old sweep right of it stays visible).
fn scan_gap(pic: &mut [Vec<f32>; NCH], at: usize, gap: usize) {
    for v in pic.iter_mut().filter(|v| !v.is_empty()) {
        let w = v.len();
        for i in 0..gap.min(w) {
            v[(at + i) % w] = f32::NAN;
        }
    }
}
