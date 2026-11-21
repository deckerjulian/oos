// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware source: acquisitions through `oos_hal::dho::seq` (configure on change, arm, wait, read the whole record),
//! decoded into volts per channel. The UI aligns each frame with the software trigger engine (all 12 trigger types on
//! real data; the hardware itself triggers with an edge on the source at the level, in AUTO or NORMAL).

#[cfg(any(target_os = "linux", test))]
mod afg;
#[cfg(any(target_os = "linux", test))]
mod batch;
#[cfg(any(target_os = "linux", test))]
mod config;
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
#[cfg(any(target_os = "linux", test))]
use self::config::{ConfigRun, Request};
#[cfg(test)]
pub use self::decode::{codes_into, decode, decode_into};
#[cfg(any(target_os = "linux", test))]
use self::plan::{batch_plan, disarm};
#[cfg(any(target_os = "linux", test))]
use self::seg::{seg_abort, seg_step, SegRun};
pub use self::seg::{set_upper_key, upper_mem};
pub use self::serial::serial_debug;
#[cfg(test)]
pub use self::settings::roll_la_t0;
#[cfg(any(target_os = "linux", test))]
use self::settings::single_wants_more;
pub use self::settings::{avg_hw_ok, seg_max, settings, slow_mode, upper_plan, ROLL_TDIV};
#[cfg(any(target_os = "linux", test))]
use self::slow::SlowRun;
#[cfg(any(target_os = "linux", test))]
use self::stop::StopRun;
#[cfg(any(target_os = "linux", test))]
use self::trim::TrimRun;
#[cfg(any(target_os = "linux", test))]
use self::units::{counter_tick, unit_key, UnitGuard, UnitJobs};
pub use self::units::{dvm_hw_on, mask_hw_on, mask_map, mask_map_set, zone_hw_on};
use super::cores;
#[cfg(target_os = "linux")]
use crate::board::Shared;
#[cfg(any(target_os = "linux", test))]
use crate::board::Slow;
#[cfg(any(target_os = "linux", test))]
use crate::board::{lock, HwShared};
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
    lock(out).status = s;
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
    use oos_hal::dho::bus::{Bar1, Bus, FPGA_VERSION};
    let shared: HwShared = Arc::new(Mutex::new(Shared::default()));
    let out = shared.clone();
    std::thread::spawn(move || {
        crate::placement::place(crate::placement::Role::Hw);
        // every wait and every time of the loop on one clock (ADR 0053): the system's here, a host run's in a test
        let mut clock = oos_hal::dho::boot::SysClock::default();
        let status = |s: String| report(&out, s);
        // started with the start-up (rcS): nothing of the hardware is touched before its `end` line
        if let Some(path) = &start_up {
            if !wait_start_up(&mut clock, || std::fs::read_to_string(path).unwrap_or_default(), &out) {
                return;
            }
        }
        let mut bar = match Bar1::open() {
            Ok(b) => b,
            Err(e) => return status(format!("/dev/xdma0_bypass: {}", e)),
        };
        let v = bar.rd(0x4000);
        lock(&out).fpga_version = v;
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

/// The wait for the start-up (rcS, `oos_hal::bootstatus`): its status file, given by `read`, read again every 100 ms of
/// `clock` until its `end` line, written after oos-fpgad cold-boot exited (its GPIO lines are free then). False when
/// the start-up stopped (`abort`): the status line says why, and the hardware is not touched.
#[cfg(any(target_os = "linux", test))]
fn wait_start_up(clock: &mut impl oos_hal::dho::boot::Clock, mut read: impl FnMut() -> String, out: &HwShared) -> bool {
    report(out, "waiting for the start-up".into());
    loop {
        match oos_hal::bootstatus::parse(&read()).finished {
            Some(Ok(())) => return true,
            Some(Err(e)) => {
                report(out, format!("acquisition hardware not started: {}", e));
                return false;
            }
            None => clock.sleep_us(100000),
        }
    }
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
    let mut l = Loop::new(hw, app_mask, program_afe, cal_dir, out);
    // one pass per call of `keep_going`: always on the scope, a counted number in a host run
    while keep_going() {
        l.pass();
    }
}

/// What the acquisition loop owns between passes: the unit's sides, the options of the run, the calibration, the
/// shadow state and the state of each part of the loop. `new` is the set-up before the first pass, `pass` one pass.
#[cfg(any(target_os = "linux", test))]
struct Loop<P: port::Port> {
    bar: P::Bus,
    c2h: P::Stream,
    afe_bus: Option<P::Afe>,
    lines: P::Lines,
    clock: P::Clock,
    unit: P,
    /// OOS sets the AFEs from the UI channel settings (`spawn`)
    program_afe: bool,
    /// the folder of the calibration files
    cal_dir: String,
    out: HwShared,
    cal: Option<afe::Cal>,
    cal_adc: Option<oos_hal::dho::adc::CalAdc>,
    /// the core trim, with the matched core offsets (`trim.rs`)
    trim: TrimRun,
    /// LA pod zero codes (cal_la.hex, read only)
    la_cal: oos_hal::dho::la::Cal,
    state: oos_hal::dho::seq::State,
    /// the configuration applied, roll or scan, the configurations counted, the hardware average (`config.rs`)
    config: ConfigRun,
    n: u64,
    /// roll and scan: the stream between passes (`slow.rs`)
    slow: SlowRun,
    /// the built-in generator: its calibration, its lines, the request applied (`afg.rs`)
    afg: AfgRun<P::Gen>,
    xadc_at: Option<u64>,
    /// STOP: the last batch, the view last read for, and whether arming is paused (`stop.rs`)
    stop: StopRun,
    /// the batches: their buffers, raw reads, frames left out and the batch read last (`batch.rs`)
    batch: BatchRun,
    /// the learnt offsets of the ADC cores (`cores`)
    cores: super::cores::Cores,
    /// the batch armed ahead (`prearm_on`) and the SINGLE stamp it was armed under
    armed: Option<(Settings, u64)>,
    /// the hardware segmented recording in progress or read back
    segrun: Option<SegRun>,
    /// the frequency counter: what runs since when, and the readings so far
    counting: Option<(crate::board::CounterWant, u64)>,
    count_n: u64,
    /// the mask/zone unit's stall guard
    guard: UnitGuard,
    /// the measure unit's side jobs: the DVM block, the levels, the debug slots (`units.rs`)
    units: UnitJobs,
}

/// The pass's handles of the loop `$l` for a part of it (`slow::Io`), with the stamp `$stamp`.
#[cfg(any(target_os = "linux", test))]
macro_rules! io {
    ($l:ident, $stamp:expr) => {
        slow::Io {
            state: &mut $l.state,
            bar: &mut $l.bar,
            c2h: &mut $l.c2h,
            clock: &mut $l.clock,
            out: &$l.out,
            n: &mut $l.n,
            stamp: $stamp,
        }
    };
}

#[cfg(any(target_os = "linux", test))]
impl<P: port::Port> Loop<P> {
    /// The set-up before the first pass: the calibration files of `cal_dir` (the front ends', the ADC's with the core
    /// trim's overlay, the LA pod's zero codes), the shadow state handed over from the channel set `app_mask`, and the
    /// parts of the loop with nothing done yet.
    fn new(hw: port::Hw<P>, app_mask: u32, program_afe: bool, cal_dir: String, out: HwShared) -> Loop<P> {
        use oos_hal::dho::boot::Clock;
        use oos_hal::dho::{adc, seq};
        let status = |s: String| report(&out, s);
        let port::Hw { bar, c2h, afe: afe_bus, lines, mut clock, port: unit } = hw;
        let cal = match afe::Cal::load_from(&cal_dir) {
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
        let trim = TrimRun::new(cal_adc.as_mut());
        // LA pod zero codes (cal_la.hex, read only); without the file the middle of the DAC range (la.md 8.1)
        let la_cal = if oos_hal::model::platform().caps.la {
            oos_hal::dho::la::Cal::load(&cal_dir).unwrap_or_else(|| {
                status("cal_la.hex not found: LA thresholds with the zero code 0x2180".into());
                oos_hal::dho::la::Cal { zero: [0x2180; 2] }
            })
        } else {
            oos_hal::dho::la::Cal::default()
        };
        let state = seq::State::handover(app_mask, cal_adc.as_ref());
        let slow = SlowRun::new(clock.now_us());
        let afg = AfgRun::<P::Gen>::new(&cal_dir);
        Loop {
            bar,
            c2h,
            afe_bus,
            lines,
            clock,
            unit,
            program_afe,
            cal_dir,
            out,
            cal,
            cal_adc,
            trim,
            la_cal,
            state,
            config: ConfigRun::default(),
            n: 0,
            slow,
            afg,
            xadc_at: None,
            stop: StopRun::default(),
            batch: BatchRun::default(),
            cores: super::cores::Cores::default(),
            armed: None,
            segrun: None,
            counting: None,
            count_n: 0,
            guard: UnitGuard::default(),
            units: UnitJobs::default(),
        }
    }

    /// One pass of the loop: the board readings, a self-calibration asked for, the generator, the UI's request, STOP,
    /// the configuration on a change, the side jobs, then roll and scan, the segments or a batch. A `return` ends the
    /// pass where the loop's `continue` did.
    fn pass(&mut self) {
        use oos_hal::dho::boot::Clock;
        use oos_hal::dho::lines::Lines;
        let last_read = self.batch.fresh.take();
        counter_tick(&mut self.bar, &mut self.clock, &self.out, &mut self.counting, &mut self.count_n);
        // FPGA die temperature and VCCINT for the System info page (one register read, about once a second)
        let now = self.clock.now_us();
        if self.xadc_at.is_none_or(|t| since(now, t) >= std::time::Duration::from_secs(1)) {
            self.xadc_at = Some(now);
            let x = oos_hal::dho::board::read_xadc(&mut self.bar);
            lock(&self.out).xadc = x;
            // LA probe detect, also with the LA off: without a probe it cannot be switched on
            if oos_hal::model::platform().caps.la {
                let p = self.lines.la_probe().ok();
                lock(&self.out).la_probe = p;
            }
        }
        // self-calibration (selfcal.rs): owns the FPGA and the AFEs until it is done; the result goes to /oos/cal
        // (the request is taken in its own statement: a lock in the `if let` scrutinee would be held for the whole
        // run, and the cancel check and the UI would wait on it forever)
        let req = lock(&self.out).cal_req.take();
        if let Some((chans, zero, adc_cores)) = req {
            self.calibrate(&chans, zero, adc_cores);
            return;
        }
        // System > Probe optional memory (`seg.rs`): every segment size of its plan, then the UI's settings again
        let probe = lock(&self.out).upper_want.take();
        if let Some((n, plan)) = probe {
            self.probe_upper(n, &plan);
            return;
        }
        // built-in generator (`afg.rs`): a changed request applied
        let (afg_want, stamp_now) = {
            let g = lock(&self.out);
            (g.afg[0], g.stamp)
        };
        self.afg.apply(afg_want, &mut self.unit, io!(self, stamp_now), &mut self.armed, &mut self.segrun);
        // read with the generator request; the settings read below are as new or newer
        let stamp = stamp_now;
        let Some(req) = Request::read(&self.out, &self.guard, self.la_cal, self.program_afe) else {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
            self.clock.sleep_us(50000);
            return;
        };
        // STOP (`stop.rs`): the hold, once, with the newest frame read raw where it is wanted; then the views
        // read again from the stored record (normal acquisitions only)
        let (hold, final_req, deep_want) = {
            let g = lock(&self.out);
            (g.hold, g.final_req, g.deep_raw)
        };
        if hold && !self.stop.paused {
            self.stop.hold(
                io!(self, stamp),
                &mut self.armed,
                last_read.as_ref(),
                &mut self.batch.raw_dma,
                &mut self.cores,
                (final_req, deep_want),
            );
        }
        let stop_view = lock(&self.out).stop_view;
        match stop_view {
            Some(v) if self.config.mode == Slow::Off && req.mode == Slow::Off => {
                self.stop.view::<P>(v, io!(self, stamp), &mut self.lines, &mut self.cores);
                return;
            }
            _ => self.stop.no_view(),
        }
        if !self.configure(&req) {
            return;
        }
        // `configure` returns true only with a configuration in `current`: the one it kept or the one it applied
        let Some((s, p)) = self.config.current.clone() else {
            unreachable!("configure() returned true without a configuration")
        };
        let mode = self.config.mode;
        let status = |s: String| report(&self.out, s);
        // a hardware segment recording in the FPGA: the measure unit (DVM, levels, slots) and the ADC core offsets
        // wait until it has ended. `disarm` does not stop it, and a write in between would change the segments
        // still to come (H4 of the 0.1.13 review)
        let seg_recording = self.segrun.as_ref().is_some_and(|r| r.recording);
        // the measure unit's side jobs (`units.rs`) and the core trim (`trim.rs`), in this order: the DVM block,
        // the levels for the auto set-up, the core trim of the current configuration, the debug slots and :TFORce
        self.units.dvm(io!(self, stamp), &mut self.armed, &s, self.config.configs, seg_recording);
        self.units.levels(io!(self, stamp), &mut self.armed, &s, mode, seg_recording);
        self.trim.step(io!(self, stamp), &mut self.armed, self.cal_adc.as_mut(), mode, seg_recording, self.afg.on());
        self.units.slots_and_force(io!(self, stamp), &mut self.armed, mode, seg_recording);
        if mode != Slow::Off {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
            self.slow.step(mode, &s, &p, io!(self, stamp), &mut self.config.current, &status);
            return;
        }
        self.slow.stop(&mut self.state, &mut self.bar);
        if s.segments.is_some() {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
            if let Err(e) = seg_step(&mut self.segrun, (&s, &p), &mut self.batch.dma, io!(self, stamp)) {
                status(format!("segments: {}", e));
                seg_abort(&mut self.segrun, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock, &self.out);
                self.clock.sleep_us(200000);
            }
            return;
        }
        self.batch_pass((&s, &p), &req, (hold, final_req), stamp);
    }

    /// Self-calibration (selfcal.rs) of the channels `chans`, the AFE zero too with `zero`, the ADC cores first with
    /// `adc_cores`: nothing armed, no segments recorded, no stream; the new tables are loaded when it succeeded, and the
    /// next pass configures again. The result goes to `Shared::cal_result`.
    fn calibrate(&mut self, chans: &[usize], zero: bool, adc_cores: bool) {
        use oos_hal::dho::adc;
        let status = |s: String| report(&self.out, s);
        disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
        seg_abort(&mut self.segrun, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock, &self.out);
        self.slow.stop(&mut self.state, &mut self.bar);
        let res = match (self.afe_bus.as_mut(), self.cal.as_ref()) {
            (Some(afe), Some(c)) => {
                let cancel_out = self.out.clone();
                let cancel = move || lock(&cancel_out).cal_cancel;
                let mut unit = oos_hal::dho::selfcal::Unit {
                    bar: &mut self.bar,
                    afe,
                    c2h: &mut self.c2h,
                    lines: &mut self.lines,
                    clock: &mut self.clock,
                    state: &mut self.state,
                    cal: c,
                    cal_adc: self.cal_adc.as_ref(),
                    cancel: &cancel,
                    adc_new: None,
                };
                let adc_base = if adc_cores { self.cal_adc.as_ref() } else { None };
                self_cal(&mut unit, c, adc_base, (chans, zero), &self.cal_dir, &self.out)
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
            self.cal = afe::Cal::load(&self.cal_dir).ok();
            self.cal_adc = adc::CalAdc::load(&self.cal_dir).ok();
            // the matched offsets over the new trims; every mode is matched again from there
            self.trim.recalibrated(self.cal_adc.as_mut());
        }
        self.config.current = None;
        let mut g = lock(&self.out);
        (g.cal_progress, g.cal_result, g.cal_cancel) = (None, Some(res), false);
    }

    /// A pass of normal acquisitions with the settings `s` configured as `p` (`sp`), as `req` asked for them, with
    /// `hold` and the final-frame request of STOP read at the top of the pass: in STOP nothing is armed; else, when
    /// SINGLE wants more, the average cleared when asked, one batch armed (or armed ahead), waited for and read.
    fn batch_pass(&mut self, sp: (&Settings, &hz::Params), req: &Request, stop: (bool, u64), stamp: u64) {
        use oos_hal::dho::boot::Clock;
        use oos_hal::dho::seq;
        let status = |s: String| report(&self.out, s);
        let ((s, p), (hold, final_req), avg_clear) = (sp, stop, req.avg_clear);
        // STOP, after the hold above: no arming until RUN or SINGLE (the UI's final-frame request is answered there)
        if hold {
            lock(&self.out).final_ack = final_req;
            self.clock.sleep_us(5000);
            return;
        }
        if std::mem::take(&mut self.stop.paused) {
            // RUN or SINGLE: the frames of the batch before the STOP are not new
            lock(&self.out).queue.clear();
        }
        let b = batch_plan(s, p);
        if !single_wants_more(&b, self.batch.single_done, stamp) {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
            self.clock.sleep_us(2000);
            return;
        }
        // hardware average: CLEAR, RUN or SINGLE start it over (nothing is armed ahead in this mode)
        if let Some((tr, seen, logged)) = self.config.avg.as_mut().filter(|a| a.1 != avg_clear) {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
            if let Err(e) = execute::<P>(
                &self.state.avg_clear_steps(),
                &mut self.bar,
                None,
                &mut self.c2h,
                &mut self.lines,
                &mut self.clock,
            ) {
                status(format!("average clear: {}", e));
            }
            tr.reset();
            (*seen, *logged) = (avg_clear, 0);
        }
        // one batch: arm, let the ring fill for up to 25 ms after the first frame, one play, one read
        // armed ahead with these settings and since the last SINGLE press: only the wait is left
        let ahead = self.armed.as_ref().is_some_and(|(a, st)| *a == b && *st == stamp);
        if !ahead {
            disarm(&mut self.armed, &mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock);
        }
        self.armed = None;
        // new settings, a view read in STOP, or the STOP itself (`hold`): a wait for a trigger ends at once. The
        // UI waits for the thread to hold before it takes the record of the STOP as its last (#24); in Normal
        // without a trigger that wait was the whole `STOP_FINAL_WAIT`
        let asked = &req.asked;
        let changed = || {
            let g = lock(&self.out);
            (g.want.as_ref(), g.slow) != (asked.0.as_ref(), asked.1) || g.stop_view.is_some() || g.hold
        };
        // while the FPGA's ring fills this thread draws frames of the last batch (drawpipe.rs), one per look at
        // the frame count, so the read waits at most one frame longer
        let pipe = crate::board::pipe::workers();
        let idle = || pipe.is_some_and(|x| x.help(&mut self.batch.helper));
        let t_batch = self.clock.now_us();
        let got = seq::acquire_batch_on(
            &mut self.state,
            &b,
            p,
            &mut self.bar,
            &mut self.c2h,
            &mut self.batch.dma,
            &mut self.clock,
            ahead,
            25,
            |_| {},
            changed,
            idle,
        );
        let batch_ms = since(self.clock.now_us(), t_batch).as_secs_f32() * 1e3;
        if let Some(x) = pipe {
            x.help_done(&mut self.batch.helper);
        }
        match got {
            Ok(count) => {
                let held = batch::Held {
                    armed: &mut self.armed,
                    avg: &mut self.config.avg,
                    cores: &mut self.cores,
                    last_batch: &mut self.stop.last_batch,
                    guard: &mut self.guard,
                };
                self.batch.take::<P>(io!(self, stamp), &mut self.lines, (&b, p, count, batch_ms), held);
            }
            // new settings (or STOP) while it waited for a trigger: at once to them. The arm has filled frame 0
            // of the ring with pre-trigger samples (it stops only before the first trigger): a stored record
            // there is gone, later frames of the last batch are untouched
            Err(e) if e == seq::NEW_SETTINGS => {
                if self.stop.last_batch.as_ref().is_some_and(|l| l.2 == 0) {
                    self.stop.last_batch = None;
                }
                // cut by the STOP (`hold`): the FSM still waits armed. New settings disarm it as they configure
                // and a view read in STOP as it plays; a STOP that reads nothing again (#24: a whole record) must
                // do it here, or the next SINGLE armed a running FSM and never got its record (unit 2026-11-20)
                if self.out.lock().is_ok_and(|g| g.hold) {
                    if let Err(e) = seq::disarm(&mut self.state, &mut self.bar, &mut self.c2h, &mut self.clock, |_| {})
                    {
                        status(format!("disarm at STOP: {}", e));
                    }
                }
            }
            Err(e) => {
                if self.stop.last_batch.as_ref().is_some_and(|l| l.2 == 0) {
                    self.stop.last_batch = None;
                }
                status(format!("acquire: {}", e));
                // the mask/zone unit stalled the arm: drop it for these settings (the next loop reconfigures)
                let unit = self.state.mask_ctrl().is_some() || self.state.zone_ctrl().is_some();
                if self.guard.batch(e.contains("no acquisition"), unit, unit_key(&b, mask_map())) {
                    status("mask/zone unit: no acquisition twice, software fallback for these settings".into());
                }
                self.clock.sleep_us(200000);
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
        lock(&prog_out).cal_progress = Some((d, t, s.to_string()));
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
