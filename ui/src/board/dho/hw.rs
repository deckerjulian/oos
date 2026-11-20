// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware source: acquisitions through `oos_hal::dho::seq` (configure on change, arm, wait, read the whole record),
//! decoded into volts per channel. The UI aligns each frame with the software trigger engine (all 12 trigger types on
//! real data; the hardware itself triggers with an edge on the source at the level, in AUTO or NORMAL).

#[cfg(any(target_os = "linux", test))]
mod afg;
#[cfg(any(target_os = "linux", test))]
mod batch;
mod decode;
#[cfg(test)]
mod liverun;
mod plan;
mod port;
mod seg;
mod serial;
mod settings;
#[cfg(any(target_os = "linux", test))]
mod slow;
#[cfg(any(target_os = "linux", test))]
mod stop;
#[cfg(test)]
mod tests;
mod trim;
mod units;

#[cfg(any(target_os = "linux", test))]
use self::afg::AfgRun;
#[cfg(any(target_os = "linux", test))]
use self::batch::BatchRun;
#[cfg(test)]
pub use self::decode::{codes_into, decode, decode_into};
#[cfg(any(target_os = "linux", test))]
use self::plan::{batch_plan, disarm};
#[cfg(any(target_os = "linux", test))]
use self::seg::{seg_abort, seg_step, SegRun};
pub use self::serial::serial_debug;
#[cfg(test)]
pub use self::settings::roll_la_t0;
pub use self::settings::{avg_hw_ok, seg_max, settings, slow_mode, ROLL_TDIV};
#[cfg(any(target_os = "linux", test))]
use self::settings::{avg_kind, raw_for_ui, single_wants_more};
#[cfg(any(target_os = "linux", test))]
use self::slow::SlowRun;
#[cfg(any(target_os = "linux", test))]
use self::stop::StopRun;
#[cfg(any(target_os = "linux", test))]
use self::trim::TrimRun;
#[cfg(any(target_os = "linux", test))]
use self::units::{counter_tick, hw_merge, unit_key, UnitGuard, UnitJobs};
pub use self::units::{dvm_hw_on, mask_hw_on, mask_map, mask_map_set, zone_hw_on};
use super::cores;
#[cfg(any(target_os = "linux", test))]
use crate::board::HwShared;
#[cfg(target_os = "linux")]
use crate::board::Shared;
#[cfg(any(target_os = "linux", test))]
use crate::board::Slow;
#[cfg(any(target_os = "linux", test))]
use oos_hal::dho::seq::Settings;
#[cfg(any(target_os = "linux", test))]
use oos_hal::dho::{afe, hz};
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

/// `seq::execute` on the loop's unit: the steps with their AFE steps, the read into a new buffer.
#[cfg(any(target_os = "linux", test))]
fn execute<P: port::Port>(
    steps: &[oos_hal::dho::seq::Step],
    bar: &mut P::Bus,
    afe: Option<&mut P::Afe>,
    c2h: &mut P::Stream,
    lines: &mut P::Lines,
    clock: &mut P::Clock,
) -> Result<Vec<u8>, String> {
    let mut dma = oos_hal::dho::bus::DmaBuf::new();
    oos_hal::dho::seq::execute_on(steps, bar, afe, c2h, lines, &mut dma, clock, |_| {}, || false)?;
    Ok(dma.into_vec())
}

/// A line for the status of the acquisition (the log, and System info).
#[cfg(any(target_os = "linux", test))]
fn report(out: &HwShared, s: String) {
    eprintln!("oos-ui hw: {}", s);
    out.lock().unwrap().status = s;
}

/// The time from `t` to `now`, both microseconds of the loop's clock.
#[cfg(any(target_os = "linux", test))]
fn since(now: u64, t: u64) -> std::time::Duration {
    std::time::Duration::from_micros(now.saturating_sub(t))
}

/// Starts the acquisition thread (`app_mask` = channel set the stock system left behind; `program_afe` = OOS sets the
/// AFEs from the UI channel settings, else they keep the stock set-up and the UI V/div must match it).
#[cfg(target_os = "linux")]
pub fn spawn(app_mask: u32, program_afe: bool, cal_dir: String, start_up: Option<String>) -> HwShared {
    use oos_hal::dho::boot::Clock;
    use oos_hal::dho::bus::{Bar1, Bus, FPGA_VERSION};
    let shared: HwShared = Arc::new(Mutex::new(Shared::default()));
    let out = shared.clone();
    std::thread::spawn(move || {
        crate::placement::place(crate::placement::Role::Hw);
        // every wait and every time of the loop on one clock (ADR 0053): the system's here, a host run's in a test
        let mut clock = oos_hal::dho::boot::SysClock::default();
        let status = |s: String| report(&out, s);
        // started with the start-up (rcS): nothing of the hardware is touched before its `end` line, written after
        // oos-fpgad cold-boot exited (its GPIO lines are free then)
        if let Some(path) = &start_up {
            status("waiting for the start-up".into());
            loop {
                let v = oos_hal::bootstatus::parse(&std::fs::read_to_string(path).unwrap_or_default());
                match v.finished {
                    Some(Ok(())) => break,
                    Some(Err(e)) => return status(format!("acquisition hardware not started: {}", e)),
                    None => clock.sleep_us(100000),
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
        let c2h = match std::fs::File::open("/dev/xdma0_c2h_0") {
            Ok(f) => f,
            Err(e) => return status(format!("/dev/xdma0_c2h_0: {}", e)),
        };
        let afe_bus = if program_afe {
            match afe::GpioAfe::open() {
                Ok(b) => Some(b),
                Err(e) => return status(format!("AFE GPIO: {}", e)),
            }
        } else {
            None
        };
        let lines = oos_hal::dho::lines::UnitLines;
        let hw = port::Hw { bar, c2h, afe: afe_bus, lines, clock, port: port::Scope };
        run(hw, app_mask, program_afe, cal_dir, out, || true);
    });
    shared
}

/// The acquisition loop on the unit `hw` (ADR 0053): the screen's settings to the FPGA, its records to the screen,
/// every wait and every time on `hw.clock`. `spawn` gives it the scope; it names none of the scope's own types.
#[cfg(any(target_os = "linux", test))]
fn run<P: port::Port>(
    hw: port::Hw<P>,
    app_mask: u32,
    program_afe: bool,
    cal_dir: String,
    out: HwShared,
    mut keep_going: impl FnMut() -> bool,
) {
    use oos_hal::dho::boot::Clock;
    use oos_hal::dho::lines::Lines;
    use oos_hal::dho::{adc, seq};
    {
        let status = |s: String| report(&out, s);
        let port::Hw { mut bar, mut c2h, afe: mut afe_bus, mut lines, mut clock, port: mut unit } = hw;
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
        // the core trim, with the matched core offsets: only with `cal.core_offset = tight` (key off = the cal_adc.hex
        // above, unchanged; `trim.rs`)
        let mut trim = TrimRun::new(cal_adc.as_mut());
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
        // roll and scan: the stream between passes
        let mut slow = SlowRun::new(clock.now_us());
        let mut mode = Slow::Off;
        // the built-in generator: its calibration, its lines, the request applied (`afg.rs`)
        let mut afg = AfgRun::<P::Gen>::new(&cal_dir);
        let mut stamp;
        let mut xadc_at: Option<u64> = None;
        // STOP: the last batch, the view last read for, and whether arming is paused (`stop.rs`)
        let mut stop = StopRun::default();
        // the batches: their buffers, raw reads, frames left out and the batch read last (`batch.rs`)
        let mut batch = BatchRun::default();
        // the learnt offsets of the ADC cores (`cores`)
        let mut cores = super::cores::Cores::default();
        // the batch armed ahead (`prearm_on`) and the SINGLE stamp it was armed under
        let mut armed: Option<(Settings, u64)> = None;
        // the hardware segmented recording in progress or read back
        let mut segrun: Option<SegRun> = None;
        // hardware average: triggers since the last clear, the `Shared::avg_clear` it was cleared for, batches logged
        let mut avg: Option<(oos_hal::dho::avg::Tracker, u64, u32)> = None;
        // the frequency counter: what runs since when, and the readings so far
        let mut counting: Option<(crate::board::CounterWant, u64)> = None;
        let mut count_n = 0u64;
        // the number of the configurations applied (the DVM block restarts after each)
        let mut configs = 0u64;
        // the mask/zone unit's stall guard
        let mut guard = UnitGuard::default();
        // the measure unit's side jobs: the DVM block, the levels, the debug slots (`units.rs`)
        let mut units = UnitJobs::default();
        // the pass's handles for a part of the loop (`slow::Io`), with the stamp `$stamp`
        macro_rules! io {
            ($stamp:expr) => {
                slow::Io {
                    state: &mut state,
                    bar: &mut bar,
                    c2h: &mut c2h,
                    clock: &mut clock,
                    out: &out,
                    n: &mut n,
                    stamp: $stamp,
                }
            };
        }
        // one pass per call of `keep_going`: always on the scope, a counted number in a host run
        while keep_going() {
            let last_read = batch.fresh.take();
            counter_tick(&mut bar, &mut clock, &out, &mut counting, &mut count_n);
            // FPGA die temperature and VCCINT for the System info page (one register read, about once a second)
            let now = clock.now_us();
            if xadc_at.is_none_or(|t| since(now, t) >= std::time::Duration::from_secs(1)) {
                xadc_at = Some(now);
                let x = oos_hal::dho::board::read_xadc(&mut bar);
                out.lock().unwrap().xadc = x;
                // LA probe detect, also with the LA off: without a probe it cannot be switched on
                if oos_hal::model::platform().caps.la {
                    let p = lines.la_probe().ok();
                    out.lock().unwrap().la_probe = p;
                }
            }
            // self-calibration (selfcal.rs): owns the FPGA and the AFEs until it is done; the result goes to /oos/cal
            // (the request is taken in its own statement: a lock in the `if let` scrutinee would be held for the whole
            // run, and the cancel check and the UI would wait on it forever)
            let req = out.lock().unwrap().cal_req.take();
            if let Some((chans, zero, adc_cores)) = req {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &mut clock, &out);
                slow.stop(&mut state, &mut bar);
                let res = match (afe_bus.as_mut(), cal.as_ref()) {
                    (Some(afe), Some(c)) => {
                        let cancel_out = out.clone();
                        let cancel = move || cancel_out.lock().unwrap().cal_cancel;
                        let mut unit = oos_hal::dho::selfcal::Unit {
                            bar: &mut bar,
                            afe,
                            c2h: &mut c2h,
                            lines: &mut lines,
                            clock: &mut clock,
                            state: &mut state,
                            cal: c,
                            cal_adc: cal_adc.as_ref(),
                            cancel: &cancel,
                            adc_new: None,
                        };
                        let adc_base = if adc_cores { cal_adc.as_ref() } else { None };
                        self_cal(&mut unit, c, adc_base, (&chans, zero), &cal_dir, &out)
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
                    trim.recalibrated(cal_adc.as_mut());
                }
                current = None;
                let mut g = out.lock().unwrap();
                (g.cal_progress, g.cal_result, g.cal_cancel) = (None, Some(res), false);
                continue;
            }
            // built-in generator (`afg.rs`): a changed request applied
            let (afg_want, stamp_now) = {
                let g = out.lock().unwrap();
                (g.afg[0], g.stamp)
            };
            afg.apply(afg_want, &mut unit, io!(stamp_now), &mut armed, &mut segrun);
            // read with the generator request; the settings read below are as new or newer
            stamp = stamp_now;
            let (want, want_mode, avg_clear, mask_want, zone_want, needs_raw) = {
                let g = out.lock().unwrap();
                (g.want.clone(), g.slow, g.avg_clear, g.mask_want.clone(), g.zone_want.clone(), g.needs_raw)
            };
            let Some(mut want) = want else {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                clock.sleep_us(50000);
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
            // STOP (`stop.rs`): the hold, once, with the newest frame read raw where it is wanted; then the views
            // read again from the stored record (normal acquisitions only)
            let (hold, final_req, deep_want) = {
                let g = out.lock().unwrap();
                (g.hold, g.final_req, g.deep_raw)
            };
            if hold && !stop.paused {
                stop.hold(
                    io!(stamp),
                    &mut armed,
                    last_read.as_ref(),
                    &mut batch.raw_dma,
                    &mut cores,
                    (final_req, deep_want),
                );
            }
            let stop_view = out.lock().unwrap().stop_view;
            match stop_view {
                Some(v) if mode == Slow::Off && want_mode == Slow::Off => {
                    stop.view::<P>(v, io!(stamp), &mut lines, &mut cores);
                    continue;
                }
                _ => stop.no_view(),
            }
            if current.as_ref().is_none_or(|c| c.0 != want) || mode != want_mode {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &mut clock, &out);
                mode = want_mode;
                stop.last_batch = None;
                slow.stop(&mut state, &mut bar);
                match state.configure(&want, cal.as_ref(), cal_adc.as_ref()) {
                    Ok((steps, p)) => {
                        if let Err(e) =
                            execute::<P>(&steps, &mut bar, afe_bus.as_mut(), &mut c2h, &mut lines, &mut clock)
                        {
                            status(format!("configure: {}", e));
                            clock.sleep_us(500000);
                            continue;
                        }
                        batch.la_fails = 0;
                        status(format!("configured: mask {:#x}, {} Sa/s, {} points", want.mask, p.rate, p.memsel));
                        current = Some((want.clone(), p));
                        configs += 1;
                        // core offsets of the ADC mode (opt-in): once this configuration has stood for TRIM_HOLD; a new
                        // configuration replaces the wait
                        trim.configured(state.mode, want.mask, &mut clock);
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
                        clock.sleep_us(500000);
                        continue;
                    }
                }
            }
            let (s, p) = current.clone().unwrap();
            // a hardware segment recording in the FPGA: the measure unit (DVM, levels, slots) and the ADC core offsets
            // wait until it has ended. `disarm` does not stop it, and a write in between would change the segments
            // still to come (H4 of the 0.1.13 review)
            let seg_recording = segrun.as_ref().is_some_and(|r| r.recording);
            // the measure unit's side jobs (`units.rs`) and the core trim (`trim.rs`), in this order: the DVM block,
            // the levels for the auto set-up, the core trim of the current configuration, the debug slots and :TFORce
            units.dvm(io!(stamp), &mut armed, &s, configs, seg_recording);
            units.levels(io!(stamp), &mut armed, &s, mode, seg_recording);
            trim.step(io!(stamp), &mut armed, cal_adc.as_mut(), mode, seg_recording, afg.on());
            units.slots_and_force(io!(stamp), &mut armed, mode, seg_recording);
            if mode != Slow::Off {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                slow.step(mode, &s, &p, io!(stamp), &mut current, &status);
                continue;
            }
            slow.stop(&mut state, &mut bar);
            if s.segments.is_some() {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                if let Err(e) = seg_step(
                    &mut segrun,
                    &s,
                    &p,
                    &mut state,
                    &mut bar,
                    &mut clock,
                    &mut c2h,
                    &mut batch.dma,
                    &out,
                    &mut n,
                    stamp,
                ) {
                    status(format!("segments: {}", e));
                    seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &mut clock, &out);
                    clock.sleep_us(200000);
                }
                continue;
            }
            // STOP, after the hold above: no arming until RUN or SINGLE (the UI's final-frame request is answered there)
            if hold {
                out.lock().unwrap().final_ack = final_req;
                clock.sleep_us(5000);
                continue;
            }
            if std::mem::take(&mut stop.paused) {
                // RUN or SINGLE: the frames of the batch before the STOP are not new
                out.lock().unwrap().queue.clear();
            }
            let b = batch_plan(&s, &p);
            if !single_wants_more(&b, batch.single_done, stamp) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                clock.sleep_us(2000);
                continue;
            }
            // hardware average: CLEAR, RUN or SINGLE start it over (nothing is armed ahead in this mode)
            if let Some((tr, seen, logged)) = avg.as_mut().filter(|a| a.1 != avg_clear) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                if let Err(e) = execute::<P>(&state.avg_clear_steps(), &mut bar, None, &mut c2h, &mut lines, &mut clock)
                {
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
            // new settings, a view read in STOP, or the STOP itself (`hold`): a wait for a trigger ends at once. The
            // UI waits for the thread to hold before it takes the record of the STOP as its last (#24); in Normal
            // without a trigger that wait was the whole `STOP_FINAL_WAIT`
            let changed = || {
                let g = out.lock().unwrap();
                (g.want.as_ref(), g.slow) != (asked.0.as_ref(), asked.1) || g.stop_view.is_some() || g.hold
            };
            // while the FPGA's ring fills this thread draws frames of the last batch (drawpipe.rs), one per look at
            // the frame count, so the read waits at most one frame longer
            let pipe = crate::drawpipe::pipe();
            let idle = || pipe.is_some_and(|x| x.help(&mut batch.helper));
            let t_batch = clock.now_us();
            let got = seq::acquire_batch_on(
                &mut state,
                &b,
                &p,
                &mut bar,
                &mut c2h,
                &mut batch.dma,
                &mut clock,
                ahead,
                25,
                |_| {},
                changed,
                idle,
            );
            let batch_ms = since(clock.now_us(), t_batch).as_secs_f32() * 1e3;
            if let Some(x) = pipe {
                x.help_done(&mut batch.helper);
            }
            match got {
                Ok(count) => {
                    let held = batch::Held {
                        armed: &mut armed,
                        avg: &mut avg,
                        cores: &mut cores,
                        last_batch: &mut stop.last_batch,
                        guard: &mut guard,
                    };
                    batch.take::<P>(io!(stamp), &mut lines, (&b, &p, count, batch_ms), held);
                }
                // new settings (or STOP) while it waited for a trigger: at once to them. The arm has filled frame 0
                // of the ring with pre-trigger samples (it stops only before the first trigger): a stored record
                // there is gone, later frames of the last batch are untouched
                Err(e) if e == seq::NEW_SETTINGS => {
                    if stop.last_batch.as_ref().is_some_and(|l| l.2 == 0) {
                        stop.last_batch = None;
                    }
                    // cut by the STOP (`hold`): the FSM still waits armed. New settings disarm it as they configure
                    // and a view read in STOP as it plays; a STOP that reads nothing again (#24: a whole record) must
                    // do it here, or the next SINGLE armed a running FSM and never got its record (unit 2026-11-20)
                    if out.lock().is_ok_and(|g| g.hold) {
                        if let Err(e) = seq::disarm(&mut state, &mut bar, &mut c2h, |_| {}) {
                            status(format!("disarm at STOP: {}", e));
                        }
                    }
                }
                Err(e) => {
                    if stop.last_batch.as_ref().is_some_and(|l| l.2 == 0) {
                        stop.last_batch = None;
                    }
                    status(format!("acquire: {}", e));
                    // the mask/zone unit stalled the arm: drop it for these settings (the next loop reconfigures)
                    let unit = state.mask_ctrl().is_some() || state.zone_ctrl().is_some();
                    if guard.batch(e.contains("no acquisition"), unit, unit_key(&b, mask_map())) {
                        status("mask/zone unit: no acquisition twice, software fallback for these settings".into());
                    }
                    clock.sleep_us(200000);
                }
            }
        }
    }
}

/// Self-calibration (selfcal.rs) on `unit` with the tables `c`, the ADC cores first from `adc_base` when given, for
/// the channels and the AFE zero of `req`; the result saved to /oos/cal. The progress goes to `out`; the line for the
/// status when it is done.
#[cfg(any(target_os = "linux", test))]
fn self_cal(
    unit: &mut dyn oos_hal::dho::selfcal::CalHw,
    c: &afe::Cal,
    adc_base: Option<&oos_hal::dho::adc::CalAdc>,
    req: (&[usize], bool),
    cal_dir: &str,
    out: &HwShared,
) -> Result<String, String> {
    let (chans, zero) = req;
    let prog_out = out.clone();
    let r = oos_hal::dho::selfcal::run_all_with(unit, c, adc_base, chans, zero, &mut |d, t, s| {
        prog_out.lock().unwrap().cal_progress = Some((d, t, s.to_string()));
    });
    match r {
        Ok(r) => oos_hal::dho::selfcal::save(&r, cal_dir).map(|files| {
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
