// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The executor: runs the step lists that `State` builds, on the unit (`execute*`, with the AFE bus) or on any `Bus`
//! (`execute_bus*`, also the host model of the tests), and the batch loop of the live display on top of it (arm, wait
//! for the frame ring, play and read). How a step kind is carried out goes into `plain_step` (or `execute_into_until`
//! for the AFE steps); which steps a sequence has is decided in seq.rs, not here. The registers it polls and reads are
//! described in docs/protocols/. Every wait and every sleep goes through a `boot::Clock` (ADR 0053): the functions
//! ending in `_on` take one, so a host run waits in virtual time; the others are those with the system clock.

use super::{frames, read_len, unpad_in, wait_steps, Settings, State, Step};
use crate::dho::afe::{self, AfeBus};
use crate::dho::boot::{Clock, SysClock};
use crate::dho::bus::Bus;
use crate::dho::lines::Lines;
use crate::dho::{hz, la};

/// One batch on the unit: arm `frames(s, p)` frames, wait for the first, then let the ring fill until all frames are
/// recorded or `wait_ms` have passed since the first one (0x4010 reads back the last recorded frame, so the count is
/// `0x4010 - first + 1`), then play and read exactly those frames into `dma`.
/// `armed`: `prearm` already armed this batch with the same `s` and `p`; only the wait is left.
/// Returns the count; the bytes (`count` x `frame_bytes`) are `dma.data()`.
/// Generic over the bus, so the host model (`seg::sim::Engine`) runs it too.
pub fn acquire_batch<B: crate::dho::bus::Bus, R: std::io::Read>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    armed: bool,
    wait_ms: u64,
    log: impl FnMut(String),
) -> Result<u32, String> {
    acquire_batch_until(st, s, p, bar, c2h, dma, armed, wait_ms, log, || false)
}

/// `acquire_batch` whose wait for a trigger ends with `Err(NEW_SETTINGS)` as soon as `stop()` says so (checked every
/// 10 ms): in NORMAL without a trigger the wait lasts 2 s, and new settings would wait for it.
pub fn acquire_batch_until<B: crate::dho::bus::Bus, R: std::io::Read>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    armed: bool,
    wait_ms: u64,
    log: impl FnMut(String),
    stop: impl Fn() -> bool,
) -> Result<u32, String> {
    acquire_batch_idle(st, s, p, bar, c2h, dma, armed, wait_ms, log, stop, || false)
}

/// `acquire_batch_until` that lets the caller work while the ring fills: `idle()` is called instead of the sleep
/// between two looks at the frame count, and the count is read after every call, so the read waits at most one call.
/// `idle` returns false when it has nothing to do (the wait then sleeps as without it).
pub fn acquire_batch_idle<B: crate::dho::bus::Bus, R: std::io::Read>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    armed: bool,
    wait_ms: u64,
    log: impl FnMut(String),
    stop: impl Fn() -> bool,
    idle: impl FnMut() -> bool,
) -> Result<u32, String> {
    let mut clock = SysClock::default();
    acquire_batch_on(st, s, p, bar, c2h, dma, &mut clock, armed, wait_ms, log, stop, idle)
}

/// `acquire_batch_idle` on `clock`: the wait for the first frame, the wait while the ring fills and their sleeps.
pub fn acquire_batch_on<B: crate::dho::bus::Bus, R: std::io::Read, C: Clock>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    clock: &mut C,
    armed: bool,
    wait_ms: u64,
    mut log: impl FnMut(String),
    stop: impl Fn() -> bool,
    mut idle: impl FnMut() -> bool,
) -> Result<u32, String> {
    let nf = frames(s, p);
    let arm = if armed { wait_steps(p) } else { st.arm_steps(s, p) };
    execute_bus_on(&arm, bar, c2h, dma, clock, &mut log, &stop)?;
    let t = clock.now_us();
    let mut count = bar.rd(0x4010).wrapping_add(1).min(nf);
    while count < nf && clock.now_us() - t < wait_ms * 1000 {
        if !idle() {
            clock.sleep_us(200);
        }
        count = bar.rd(0x4010).wrapping_add(1).clamp(1, nf);
    }
    log(format!("{} of {} frames after {:.1} ms", count, nf, (clock.now_us() - t) as f64 * 1e-3));
    execute_bus_on(&st.read_steps(s, p, count), bar, c2h, dma, clock, &mut log, &|| false)?;
    Ok(count)
}

/// A second read of frame `index` of the batch just read with FPGA compression (`s.compress` > 1), this time raw: the
/// same record, every sample (fork, docs/fork/compress.md). The ring is played again as for the LA read and the stamp
/// plays of hardware segments; nothing may arm or write the FPGA in between. The frame lands in `dma`
/// (`frame_bytes` of `raw_settings(s)`).
pub fn read_raw_again<B: crate::dho::bus::Bus, R: std::io::Read>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    index: u32,
    log: impl FnMut(String),
) -> Result<(), String> {
    execute_bus(&st.read_steps_at(&raw_settings(s), p, index, 1), bar, c2h, dma, log)
}

/// `s` without the FPGA compression: what `read_raw_again` reads with.
pub fn raw_settings(s: &Settings) -> Settings {
    Settings { compress: 1, frames: 1, ..s.clone() }
}

/// Arms the next batch right after a read (and the LA read of the same record), so the FPGA records while the host
/// decodes: the same writes as the arm `acquire_batch` would issue next, earlier. Read it with `acquire_batch(..,
/// armed = true, ..)` and the same `s` and `p`; anything else that writes the FPGA first needs `disarm`.
pub fn prearm<B: Bus, R: std::io::Read>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    log: impl FnMut(String),
) -> Result<(), String> {
    execute_bus(&st.arm_only_steps(s, p), bar, c2h, &mut crate::dho::bus::DmaBuf::new(), log)
}

/// Stops an acquisition armed by `prearm` that will not be read (`State::disarm_steps`).
pub fn disarm<B: Bus, R: std::io::Read>(
    st: &mut State,
    bar: &mut B,
    c2h: &mut R,
    log: impl FnMut(String),
) -> Result<(), String> {
    execute_bus(&st.disarm_steps(), bar, c2h, &mut crate::dho::bus::DmaBuf::new(), log)
}

/// The LA read of the acquisition just read out (`la_read_steps`) on the unit: LA words, bit n = Dn, word `start` of
/// `la::window` first.
pub fn acquire_la<B: Bus, R: std::io::Read>(
    st: &mut State,
    s: &Settings,
    p: &hz::Params,
    bar: &mut B,
    c2h: &mut R,
    log: impl FnMut(String),
) -> Result<Vec<u16>, String> {
    let (n, _) = la::window(&la::timing(p, s.mask));
    let steps = st.la_read_steps(s, p);
    let mut dma = crate::dho::bus::DmaBuf::new();
    match execute_bus(&steps, bar, c2h, &mut dma, log) {
        Ok(()) => Ok(la::words(dma.data(), n as usize)),
        Err(e) => {
            // the read failed before `la::restore` ran: without it the stream stays on the LA and the analog is gone
            for (off, v) in la::restore(&mut st.sh) {
                bar.wr(off, v);
            }
            Err(e)
        }
    }
}

/// Runs steps that need only the register window and the C2H stream (no AFE step) on any `Bus`: the unit, or a model
/// of it in the tests. The Read step goes into `dma` like `execute_into_until`.
pub fn execute_bus<B: crate::dho::bus::Bus, R: std::io::Read>(
    steps: &[Step],
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    mut log: impl FnMut(String),
) -> Result<(), String> {
    execute_bus_until(steps, bar, c2h, dma, &mut log, &|| false)
}

/// `execute_bus` whose polls end early with `Err(NEW_SETTINGS)` when `stop()` says so (checked every 10 ms).
pub fn execute_bus_until<B: crate::dho::bus::Bus, R: std::io::Read>(
    steps: &[Step],
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    log: &mut impl FnMut(String),
    stop: &impl Fn() -> bool,
) -> Result<(), String> {
    execute_bus_on(steps, bar, c2h, dma, &mut SysClock::default(), log, stop)
}

/// `execute_bus_until` on `clock`: the polls and the sleeps of the steps.
pub fn execute_bus_on<B: crate::dho::bus::Bus, R: std::io::Read, C: Clock>(
    steps: &[Step],
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    clock: &mut C,
    log: &mut impl FnMut(String),
    stop: &impl Fn() -> bool,
) -> Result<(), String> {
    for s in steps {
        plain_step(s, bar, c2h, dma, clock, log, stop)?;
    }
    Ok(())
}

/// One step of `execute_bus` / `execute_into_until` other than the AFE steps.
fn plain_step<B: crate::dho::bus::Bus, R: std::io::Read, C: Clock>(
    s: &Step,
    bar: &mut B,
    c2h: &mut R,
    dma: &mut crate::dho::bus::DmaBuf,
    clock: &mut C,
    log: &mut impl FnMut(String),
    stop: &impl Fn() -> bool,
) -> Result<(), String> {
    match s {
        Step::Wr((off, v)) => bar.wr(*off, *v),
        Step::Afe(_) => return Err("AFE step without AFE bus".into()),
        Step::Poll { reg, bit, ms, what } => {
            let t = clock.now_us();
            let ms_since = |clock: &mut C| (clock.now_us() - t) as f64 * 1e-3;
            let mut last = bar.rd(*reg);
            let mut k = 0u32;
            while last & bit == 0 && clock.now_us() - t < *ms * 1000 {
                clock.sleep_us(500);
                last = bar.rd(*reg);
                k += 1;
                if k.is_multiple_of(20) && last & bit == 0 && stop() {
                    log(format!("{} {} ({:.1} ms)", what, NEW_SETTINGS, ms_since(clock)));
                    return Err(NEW_SETTINGS.into());
                }
            }
            let ok = last & bit != 0;
            log(format!(
                "{} {} ({:#x} = {:#010x}, {:.1} ms)",
                what,
                if ok { "ok" } else { "TIMEOUT" },
                reg,
                last,
                ms_since(clock)
            ));
            if !ok && *reg == 0x4080 {
                // 0x4030 bit 20 = mask/zone read-back started, [3:0] = FSM state (5 = waiting for the column window)
                let st = bar.rd(0x4030);
                return Err(format!(
                    "no acquisition (0x4080 bit30 not set; 0x4030 = {:#010x}, readback {}, state {})",
                    st,
                    st >> 20 & 1,
                    st & 0xf
                ));
            }
        }
        Step::Read { frames, frame } => {
            let bytes = read_len(*frames, *frame);
            let buf = dma
                .read(c2h, bytes, |got, k| log(format!("c2h: short read of {} bytes (at {} of {})", k, got, bytes)))
                .map_err(|e| format!("c2h read of {} bytes: {} (0x4034 = {:#x})", bytes, e, bar.rd(0x4034)))?;
            let n = unpad_in(buf, *frame);
            dma.truncate(n);
        }
        Step::Peek(regs) => {
            log(regs.iter().map(|r| format!("{:#06x} = {:#010x}", r, bar.rd(*r))).collect::<Vec<_>>().join(", "))
        }
        Step::Sleep { us } => clock.sleep_us(*us as u64),
        Step::Note(n) => log(n.clone()),
    }
    Ok(())
}

/// Runs steps on the unit; returns the bytes of the Read step. `log` gets notes and poll results.
#[cfg(target_os = "linux")]
pub fn execute(
    steps: &[Step],
    bar: &mut crate::dho::bus::Bar1,
    afe_bus: Option<&mut afe::GpioAfe>,
    c2h: &mut std::fs::File,
    log: impl FnMut(String),
) -> Result<Vec<u8>, String> {
    let mut dma = crate::dho::bus::DmaBuf::new();
    execute_into_until(steps, bar, afe_bus, c2h, &mut dma, log, || false)?;
    Ok(dma.into_vec())
}

/// What `execute_until` returns when `stop` ended a poll: the same state of the FPGA as a poll that timed out.
pub const NEW_SETTINGS: &str = "stopped for new settings";

/// `execute` whose polls end early with `Err(NEW_SETTINGS)` when `stop()` says so (checked every 10 ms).
#[cfg(target_os = "linux")]
pub fn execute_until(
    steps: &[Step],
    bar: &mut crate::dho::bus::Bar1,
    afe_bus: Option<&mut afe::GpioAfe>,
    c2h: &mut std::fs::File,
    log: impl FnMut(String),
    stop: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    let mut dma = crate::dho::bus::DmaBuf::new();
    execute_into_until(steps, bar, afe_bus, c2h, &mut dma, log, stop)?;
    Ok(dma.into_vec())
}

/// `execute` with the Read step into `dma` (kept across batches): the bytes are `dma.data()`, without padding. Polls
/// end early like `execute_until`.
#[cfg(target_os = "linux")]
pub fn execute_into_until(
    steps: &[Step],
    bar: &mut crate::dho::bus::Bar1,
    afe_bus: Option<&mut afe::GpioAfe>,
    c2h: &mut std::fs::File,
    dma: &mut crate::dho::bus::DmaBuf,
    log: impl FnMut(String),
    stop: impl Fn() -> bool,
) -> Result<(), String> {
    let mut lines = crate::dho::lines::UnitLines;
    execute_on(steps, bar, afe_bus, c2h, &mut lines, dma, &mut SysClock::default(), log, stop)
}

/// `execute_into_until` on any unit: the register window, the front-end bus (None: an AFE step is an error), the
/// stream, the single lines and the clock are given. The unit's are `Bar1`, `GpioAfe`, the C2H file, `UnitLines` and
/// `SysClock`; a host run gives the simulated unit's (ADR 0053).
pub fn execute_on<B: Bus, A: AfeBus, R: std::io::Read, L: Lines, C: Clock>(
    steps: &[Step],
    bar: &mut B,
    mut afe_bus: Option<&mut A>,
    c2h: &mut R,
    lines: &mut L,
    dma: &mut crate::dho::bus::DmaBuf,
    clock: &mut C,
    mut log: impl FnMut(String),
    stop: impl Fn() -> bool,
) -> Result<(), String> {
    for s in steps {
        match s {
            Step::Afe(afe::Op::Afe { ch, reg, val }) => {
                afe_bus.as_deref_mut().ok_or("AFE step without AFE bus")?.write(*ch, *reg, *val)
            }
            Step::Afe(afe::Op::Gear { ch, gear }) => lines.gear(*ch, *gear).map_err(|e| format!("gear: {}", e))?,
            Step::Afe(afe::Op::Adc(w)) => {
                clock.sleep_us(100);
                bar.wr(0x1034, *w);
                clock.sleep_us(100);
            }
            _ => plain_step(s, bar, c2h, dma, clock, &mut log, &stop)?,
        }
    }
    Ok(())
}
