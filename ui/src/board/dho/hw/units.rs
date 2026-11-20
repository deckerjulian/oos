// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The opt-in FPGA units around an acquisition: their system.conf switches, the measure unit's levels, the mask
//! and zone unit (comparator mapping, stall guard, merge into the settings), the DVM block and the frequency
//! counter, and the measure unit's side jobs in the acquisition loop (`UnitJobs`: DVM, levels, debug slots, :TFORce).
//! A new opt-in unit gets its switch and its steps here.

use crate::board::lock;
#[cfg(any(target_os = "linux", test))]
use crate::board::HwShared;
use oos_hal::dho::auxout;
use oos_hal::dho::seq::Settings;

/// Trigger investigation (trigger.md 8): with OOS_FRAME_HEADER set, frames are read with the 16-byte header of the stock
/// live loop and every frame's header is logged to /tmp/oos-hdr.log next to where the edge actually is.
pub(super) fn header_probe() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OOS_FRAME_HEADER").is_some())
}

/// `runs` runs of the measure unit, folded into the levels of the channels of `s` (`oos_hal::dho::levels`), in the
/// volts of the records.
#[cfg(any(target_os = "linux", test))]
pub(super) fn levels_measure<B: oos_hal::dho::bus::Bus, C: oos_hal::dho::boot::Clock>(
    bar: &mut B,
    clock: &mut C,
    s: &Settings,
    gain: &[f64; 4],
    runs: u32,
) -> [Option<crate::board::ChLevels>; 4] {
    use oos_hal::dho::levels;
    let mut got = Vec::with_capacity(runs as usize);
    for _ in 0..runs {
        match levels::run(bar, &mut |us| clock.sleep_us(us as u64)) {
            Some(r) => got.push(r),
            None => return [None; 4],
        }
    }
    let vert = std::array::from_fn(|c| levels::Vert { scale: s.ch[c].scale, offset: s.ch[c].offset, gain: gain[c] });
    levels::channel_levels(&got, s.mask, &vert).map(|l| {
        l.map(|l| crate::board::ChLevels { lo: l.lo as f32, hi: l.hi as f32, mean: l.mean as f32, clipped: l.clipped })
    })
}

/// The trigger status from the SCU status bits, cleared after every batch as the stock UI does
/// (docs/fork/trigger-lockin.md; verified on the unit 2026-11-13). `trig.hw_status = off` in system.conf, or
/// OOS_TRIG_HW_STATUS=0, takes it from the software refinement instead.
pub fn hw_status_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !HW_STATUS_OFF.get())
}

/// Off: the switch of `hw_status_on` reads `off`.
pub(super) const HW_STATUS_OFF: crate::sysconf::Flag =
    crate::sysconf::Flag { key: "trig.hw_status", env: Some("OOS_TRIG_HW_STATUS"), on: "off" };

/// `dvm.hw = on` in system.conf: the DVM reads the FPGA's DVM block (docs/fork/dvm.md) instead of the records. Off by
/// default until the result layout is verified on the unit.
pub fn dvm_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| DVM_HW.get())
}

pub(super) const DVM_HW: crate::sysconf::Flag = crate::sysconf::Flag { key: "dvm.hw", env: None, on: "on" };

/// `mask.hw = on` in system.conf: the mask test runs in the FPGA's mask unit (docs/fork/hwmask.md) and the software
/// test only where it cannot. Off by default until the sample scale and the slots are verified on the unit.
pub fn mask_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| MASK_HW.get())
}

pub(super) const MASK_HW: crate::sysconf::Flag = crate::sysconf::Flag { key: "mask.hw", env: None, on: "on" };

/// `zone.hw = on` in system.conf: the zone trigger qualifies frames in the FPGA (docs/fork/hwzone.md) and the records
/// are no longer filtered in software where it can. Off by default until the mapping is verified on the unit.
pub fn zone_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| ZONE_HW.get())
}

pub(super) const ZONE_HW: crate::sysconf::Flag = crate::sysconf::Flag { key: "zone.hw", env: None, on: "on" };

/// Keys of the comparator mapping in system.conf: `mask.map.<key> = <value>` (`mask::Map::set`).
const MASK_MAP_KEYS: [&str; 8] = ["domain", "cpd", "mid", "shift", "offs", "invert", "cols", "range"];

/// The mapping of system.conf's `mask.map.*` lines over the default; bad lines are reported and skipped.
pub(super) fn mask_map_from(conf: &str) -> oos_hal::dho::mask::Map {
    let mut m = oos_hal::dho::mask::Map::default();
    for k in MASK_MAP_KEYS {
        if let Some(v) = crate::sysconf::value(conf, &format!("mask.map.{}", k)) {
            if let Err(e) = m.set(&format!("{}={}", k, v)) {
                eprintln!("oos-ui: system.conf mask.map.{}: {}", k, e);
            }
        }
    }
    m
}

fn mask_map_cell() -> &'static std::sync::Mutex<oos_hal::dho::mask::Map> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<oos_hal::dho::mask::Map>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(mask_map_from(&crate::sysconf::fork_text())))
}

/// The comparator mapping of the mask test and the zone trigger now (system.conf, then `mask_map_set`).
pub fn mask_map() -> oos_hal::dho::mask::Map {
    *lock(mask_map_cell())
}

/// Sets one `key=value` of the mapping (None: none), and returns the whole mapping. The acquisition thread takes the
/// new mapping with the next batch and reloads the tables.
pub fn mask_map_set(kv: Option<&str>) -> Result<String, String> {
    let mut m = lock(mask_map_cell());
    if let Some(kv) = kv {
        m.set(kv)?;
    }
    Ok(m.text())
}

/// What a stall of the mask/zone unit is kept for: time base, depth, channel set and comparator mapping. Another of
/// any of them tries the hardware again.
type UnitKey = (u64, i128, u32, oos_hal::dho::mask::Map);

pub(super) fn unit_key(s: &Settings, map: oos_hal::dho::mask::Map) -> UnitKey {
    (s.tdiv.to_bits(), s.depth, s.mask, map)
}

/// Consecutive failed arms ("no acquisition") with the mask/zone unit running after which it is dropped for these
/// settings (round 8: 200 us/div at 100k and Auto stopped the acquisition for good).
const UNIT_STALLS: u32 = 2;

/// The stall guard of the mask/zone unit: counts arms that found no acquisition while the unit ran, and blocks it
/// for the settings it stalled with.
#[derive(Default)]
pub(super) struct UnitGuard {
    stalls: u32,
    blocked: Option<UnitKey>,
}

impl UnitGuard {
    /// A batch ended: `stalled` = no acquisition, `unit` = the mask or zone unit was running. Returns true when the
    /// unit is now blocked for `key`.
    pub(super) fn batch(&mut self, stalled: bool, unit: bool, key: UnitKey) -> bool {
        if !(stalled && unit) {
            self.stalls = 0;
            return false;
        }
        self.stalls += 1;
        if self.stalls >= UNIT_STALLS {
            self.blocked = Some(key);
            self.stalls = 0;
            return true;
        }
        false
    }

    pub(super) fn blocks(&self, key: &UnitKey) -> bool {
        self.blocked.as_ref() == Some(key)
    }
}

/// The UI's mask test and zone trigger into the acquisition settings. The mask unit runs one of them: the mask test
/// wins. AUX OUT pass/fail follows the zone result while the zone trigger runs (0x119c source bit).
pub(super) fn hw_merge(
    want: &mut Settings,
    mask: Option<&crate::board::MaskWant>,
    zone: Option<&crate::board::ZoneWant>,
    map: oos_hal::dho::mask::Map,
) {
    want.hwmask = mask.map(|w| oos_hal::dho::seq::HwMask { map, ..hw_mask(w) });
    want.hwzone = zone.filter(|_| mask.is_none()).map(|z| oos_hal::dho::seq::HwZone {
        ch: z.ch,
        zones: z
            .zones
            .iter()
            .take(2)
            .map(|(poly, inside)| oos_hal::dho::zone::Zone { poly: poly.clone(), inside: *inside })
            .collect(),
        clear: z.clear,
        map,
    });
    if let (auxout::AuxOut::PassFail(p), Some(_)) = (&mut want.aux, &want.hwzone) {
        p.source = auxout::Source::Zone;
    }
}

/// The mask test of the acquisition settings from the UI's wish.
fn hw_mask(w: &crate::board::MaskWant) -> oos_hal::dho::seq::HwMask {
    oos_hal::dho::seq::HwMask {
        ch: w.ch,
        lo: w.lo.clone(),
        hi: w.hi.clone(),
        stop_on_fail: w.stop,
        clear: w.clear,
        map: oos_hal::dho::mask::Map::default(),
    }
}

/// Seconds between two readings of the DVM block, and from its (re)start to the first one. The window of 2^28 samples
/// is 0.21 s at 1.25 GSa/s; how long it really takes is open (docs/fork/dvm.md).
const DVM_READ_S: f64 = 0.5;

/// What the DVM block runs on: channel, lane, and the number of the acquisition configuration it was started after.
pub(super) type DvmKey = (usize, u32, u64);

/// The next step of the DVM block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DvmStep {
    Idle,
    /// (re)start on this lane
    Start(u32),
    Stop,
    Read,
}

/// The DVM block's next step: (re)started when the wanted channel, its lane in the acquired set `mask` or the
/// acquisition configuration (`config`, raised on every configuration) changed; stopped when nothing is wanted or the
/// channel is not acquired; read `DVM_READ_S` after the start or the last reading.
pub(super) fn dvm_step(
    want: Option<crate::board::DvmWant>,
    mask: u32,
    config: u64,
    running: Option<DvmKey>,
    elapsed_s: f64,
) -> (DvmStep, Option<DvmKey>) {
    let target = want.and_then(|w| oos_hal::dho::dvm::lane_forced(mask, w.ch, w.lane).map(|l| (w.ch, l, config)));
    if target != running {
        let step = match (target, running) {
            (Some((_, l, _)), _) => DvmStep::Start(l),
            (None, Some(_)) => DvmStep::Stop,
            (None, None) => DvmStep::Idle,
        };
        return (step, target);
    }
    (if running.is_some() && elapsed_s >= DVM_READ_S { DvmStep::Read } else { DvmStep::Idle }, running)
}

/// One reading of the DVM block for the UI: the raw registers and, when the decode takes them, the three values in the
/// volts of channel `ch` of `s`.
pub(super) fn dvm_reading(
    raw: &oos_hal::dho::dvm::Raw,
    key: DvmKey,
    s: &Settings,
    gain: &[f64; 4],
    n: u64,
) -> crate::board::DvmRead {
    use oos_hal::dho::dvm;
    let (ch, lane, _) = key;
    let vert = oos_hal::dho::levels::Vert { scale: s.ch[ch].scale, offset: s.ch[ch].offset, gain: gain[ch] };
    let volts = dvm::decode(raw, dvm::EXP)
        .map(|r| [dvm::Mode::Dc, dvm::Mode::AcRms, dvm::Mode::AcDcRms].map(|m| dvm::volts(&r, m, &vert)));
    crate::board::DvmRead {
        ch,
        lane,
        exp: dvm::EXP,
        raw: [raw.cfg, raw.sum_lo, raw.sum_hi, raw.sq_lo, raw.sq_hi],
        volts,
        n,
    }
}

/// The FPGA's frequency counter, once per pass of the acquisition loop: (re)started when the UI wants another set-up,
/// read once a gate (and 0.1 s) has passed since the start or the last reading (docs/protocols/counter.md).
#[cfg(any(target_os = "linux", test))]
pub(super) fn counter_tick<B: oos_hal::dho::bus::Bus, C: oos_hal::dho::boot::Clock>(
    bar: &mut B,
    clock: &mut C,
    out: &HwShared,
    counting: &mut Option<(crate::board::CounterWant, u64)>,
    n: &mut u64,
) {
    use oos_hal::dho::counter;
    let want = lock(out).counter_want;
    if want != counting.map(|c| c.0) {
        match want {
            Some(w) => {
                let mode = [counter::Mode::Frequency, counter::Mode::Period, counter::Mode::Totalize][w.mode.min(2)];
                bar.apply(&counter::start(&counter::Config { ch: w.ch, mode, digits: w.digits }));
                *counting = Some((w, clock.now_us()));
            }
            None => {
                bar.apply(&counter::stop());
                *counting = None;
            }
        }
        lock(out).counter = None;
        return;
    }
    if let Some((w, t)) = *counting {
        let gate = counter::gate_s(w.digits);
        if clock.now_us().saturating_sub(t) as f64 * 1e-6 >= gate + 0.1 {
            let r = counter::read(bar);
            *n += 1;
            lock(out).counter =
                Some(crate::board::CounterRead { period: counter::period(&r, gate), total: r.total, n: *n });
            *counting = Some((w, clock.now_us()));
        }
    }
}

/// The measure unit's side jobs between passes of the loop: what the DVM block runs on since when and its readings so
/// far, and the last requests of the levels for the auto set-up and of the debug slots answered.
#[cfg(any(target_os = "linux", test))]
#[derive(Default)]
pub(super) struct UnitJobs {
    dvm_run: Option<(DvmKey, u64)>,
    dvm_n: u64,
    levels_done: Option<u64>,
    slots_done: Option<u64>,
}

#[cfg(any(target_os = "linux", test))]
impl UnitJobs {
    /// The DVM block (`dvm.hw = on`) on the settings `s`, configuration number `configs`: (re)started while nothing is
    /// armed, read like the counter; not during a hardware segment recording (`seg_recording`).
    pub(super) fn dvm<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
        &mut self,
        io: super::slow::Io<'_, B, R, C>,
        armed: &mut Option<(Settings, u64)>,
        s: &Settings,
        configs: u64,
        seg_recording: bool,
    ) {
        let super::slow::Io { state, bar, c2h, clock, out, .. } = io;
        if !seg_recording {
            use oos_hal::dho::dvm;
            let want = lock(out).dvm_want;
            let now = clock.now_us();
            let elapsed = self.dvm_run.map_or(0.0, |r| super::since(now, r.1).as_secs_f64());
            let (step, key) = dvm_step(want, s.mask, configs, self.dvm_run.map(|r| r.0), elapsed);
            match step {
                DvmStep::Start(lane) => {
                    super::plan::disarm(armed, state, bar, c2h);
                    bar.apply(&dvm::start(lane, dvm::EXP));
                    lock(out).dvm = None;
                }
                DvmStep::Stop => {
                    super::plan::disarm(armed, state, bar, c2h);
                    bar.apply(&dvm::stop());
                    lock(out).dvm = None;
                }
                DvmStep::Read => {
                    let raw = dvm::read(bar);
                    self.dvm_n += 1;
                    let r = key.map(|k| dvm_reading(&raw, k, s, &state.gain, self.dvm_n));
                    lock(out).dvm = r;
                }
                DvmStep::Idle => {}
            }
            if step != DvmStep::Idle {
                let now = clock.now_us();
                self.dvm_run = key.map(|k| (k, now));
            }
        }
    }

    /// Levels for the auto set-up from the measure unit, once the settings they ask for are applied (the stamp of
    /// `io`); the unit is set up while nothing is armed (configuration crosses into the ADC clock without
    /// synchronisers).
    pub(super) fn levels<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
        &mut self,
        io: super::slow::Io<'_, B, R, C>,
        armed: &mut Option<(Settings, u64)>,
        s: &Settings,
        mode: crate::board::Slow,
        seg_recording: bool,
    ) {
        let super::slow::Io { state, bar, c2h, clock, out, stamp, .. } = io;
        let lw = lock(out).levels_want;
        if let Some((id, _, runs)) = lw.filter(|w| {
            mode == crate::board::Slow::Off && !seg_recording && stamp >= w.1 && self.levels_done != Some(w.0)
        }) {
            super::plan::disarm(armed, state, bar, c2h);
            let v = levels_measure(bar, clock, s, &state.gain, runs);
            lock(out).levels = Some((id, v));
            self.levels_done = Some(id);
        }
    }

    /// Debug: one raw run of the measure unit for `:DEBug:SLOTs?`. Then :TFORce (from PR #8): a pulse of the force bit
    /// on the shadows now in force, then back. Not part of the set-up, so it does not reconfigure. Pulsed between
    /// batches: a batch already waiting for its trigger takes it at its next arm.
    pub(super) fn slots_and_force<B: oos_hal::dho::bus::Bus, R: std::io::Read, C: oos_hal::dho::boot::Clock>(
        &mut self,
        io: super::slow::Io<'_, B, R, C>,
        armed: &mut Option<(Settings, u64)>,
        mode: crate::board::Slow,
        seg_recording: bool,
    ) {
        let super::slow::Io { state, bar, c2h, clock, out, .. } = io;
        let sw = lock(out).slots_want;
        if let Some(id) =
            sw.filter(|&id| mode == crate::board::Slow::Off && !seg_recording && self.slots_done != Some(id))
        {
            super::plan::disarm(armed, state, bar, c2h);
            let r = oos_hal::dho::levels::run(bar, &mut |us| clock.sleep_us(us as u64));
            lock(out).slots = Some((id, r.map(|r| r.map(|x| [x.min as f32, x.max as f32, x.mean as f32]))));
            self.slots_done = Some(id);
        }
        if std::mem::take(&mut lock(out).force) {
            bar.apply(&oos_hal::dho::tpu::force(&state.ts));
        }
    }
}
