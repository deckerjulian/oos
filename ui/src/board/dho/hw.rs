// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware source: acquisitions through `oos_hal::dho::seq` (configure on change, arm, wait, read the whole record),
//! decoded into volts per channel. The UI aligns each frame with the software trigger engine (all 12 trigger types on
//! real data; the hardware itself triggers with an edge on the source at the level, in AUTO or NORMAL).

use crate::board::{Frame, Slow};
#[cfg(target_os = "linux")]
use crate::board::{HwShared, Shared, QUEUE_MAX};
use crate::model::*;
use oos_hal::dho::seq::{Chan, Couple, Segments, Settings, Trig};
use oos_hal::dho::{afe, auxout, hz, tpu};
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

/// Time base from which the hardware source streams in roll mode: 200 ms/div (stock: 50 ms/div). Below it the stream
/// comes out too fast for its time axis with two or four channels, a normal acquisition is right there.
pub const ROLL_TDIV: f64 = 0.2;
/// Time base from which the hardware source sweeps in scan mode when Auto Roll is off (stock: 200 ms/div).
pub const SCAN_TDIV: f64 = 0.2;

/// The stock switching rule: Auto Roll on -> roll from 50 ms/div; off -> scan from 200 ms/div unless averaging.
pub fn slow_mode(m: &Model) -> Slow {
    // f32 time bases: 5 ms is 4.99999989 ms
    let tdiv = m.hw_time_per_div() as f64 * 1.0001;
    if m.auto_roll && tdiv >= ROLL_TDIV {
        Slow::Roll
    } else if !m.auto_roll && tdiv >= SCAN_TDIV && m.acq != AcqMode::Average {
        Slow::Scan { delay: m.delay as f64, single: m.trig_mode == TrigMode::Single }
    } else {
        Slow::Off
    }
}

/// Live record length for Auto depth (keeps the read short enough for a fluid display).
const LIVE_DEPTH: i128 = 100_000;

/// Auto depth at short time bases: the stock record is exactly one screen at the full rate, so after the software
/// trigger moves the record by a few samples the edge of the screen has no data (acquisition-start.md, C2H read rules
/// 3). An eighth more memory (at least 64 points, a multiple of 8) at the same rate: `hz::prep` keeps the full rate for
/// a depth above one screen and centres the record on the trigger.
fn screen_margin(memsel: i128) -> i128 {
    (memsel + (memsel / 8).max(64) + 7) / 8 * 8
}

/// The hardware settings for the UI model.
pub fn settings(m: &Model, program_afe: bool) -> Settings {
    let mut mask = (0..NCH).filter(|&c| m.ch[c].on).fold(0u32, |a, c| a | 1 << c);
    // a trigger source is acquired whether it is shown or not (trigger.md 2.1, 2.4)
    mask |= m.trig_channels().iter().enumerate().filter(|(_, &u)| u).fold(0u32, |a, (c, _)| a | 1 << c);
    if mask == 0 {
        // nothing shown: with the LA on the trigger source is acquired hidden (la.md 0.5), else CH1
        mask = if m.la_on() { 1 << m.trig_src.min(NCH - 1) } else { 1 };
    }
    let tdiv = m.hw_time_per_div() as f64;
    let depth = match mdepths()[m.mdepth].1 {
        // roll streams without reading the memory: the stock Auto depth gives the stock point spacing (10 us at
        // 100 ms/div)
        0 if slow_mode(m) != Slow::Off => 0,
        0 => {
            let auto = hz::prep(mask, oos_hal::dho::seq::tdiv_fs(tdiv), 0, 0);
            if auto.memsel as i128 > LIVE_DEPTH {
                LIVE_DEPTH
            } else {
                screen_margin(auto.memsel as i128)
            }
        }
        // roll and scan: a fixed depth below 1 M becomes 1 M (roll-mode.md 1.3)
        d if slow_mode(m) != Slow::Off => (d as i128).max(1_000_000),
        d => d as i128,
    };
    // logic analyser: the record holds analog and LA, so the depth stays within the LA limits
    let la = m.la_on().then(|| oos_hal::dho::la::Set {
        thr: [m.la.thr(0) as f64, m.la.thr(1) as f64],
        analog_shown: m.n_on() > 0,
        // the thread puts in the zero codes of the unit's cal_la.hex
        cal: oos_hal::dho::la::Cal::default(),
        roll: slow_mode(m) != Slow::Off,
    });
    // the depth of the menu may be from fewer channels: never beyond the limit of the channels on now
    let max_mem = oos_hal::model::platform().limits.mem(mask.count_ones() as usize, m.la_on());
    let depth = if depth > 0 { depth.min(max_mem as i128) } else { depth };
    // roll faster than 50 ms/div: no more than the roll stream's rate
    let df = oos_hal::dho::roll::depth_for(oos_hal::dho::seq::tdiv_fs(tdiv));
    let depth = if slow_mode(m) == Slow::Roll && df > 0 { if depth == 0 { df } else { depth.min(df) } } else { depth };
    let ch = std::array::from_fn(|c| Chan { scale: m.ch[c].volts_per_div() as f64, offset: m.ch[c].offset as f64, bw: bw_code(m.ch[c].bw), probe: m.ch[c].probe as f64, bias: m.ch[c].bias as f64, skew: m.ch[c].skew as f64,
        couple: match m.ch[c].coupling {
            // a board without it never gets it (`Model::fit_platform`)
            Coupling::Dc | Coupling::Dc50 => Couple::Dc,
            Coupling::Ac => Couple::Ac,
            Coupling::Gnd => Couple::Gnd,
        },
        invert: m.ch[c].invert,
    });
    // protocol trigger: hardware edge on the line where frames start (SPI: chip select, else the data line)
    let mut src = if m.trig_type == TrigType::Bus {
        match (m.dec_proto, m.dec_cs) {
            (Proto::Spi, Some(c)) | (Proto::I2s, Some(c)) => c,
            _ => m.dec_src,
        }
    } else {
        m.trig_src
    };
    if mask & (1 << src) == 0 {
        src = mask.trailing_zeros() as usize;
    }
    let mut s = Settings {
        // roll and scan have their own way with the position
        delay: if slow_mode(m) == Slow::Off { m.delay as f64 } else { 0.0 },
        mode: if m.acq == AcqMode::Peak { oos_hal::dho::acq::Mode::Peak } else { oos_hal::dho::acq::Mode::Normal },
        holdoff: (m.holdoff as f64).max(oos_hal::dho::seq::HOLDOFF_MIN),
        mask,
        tdiv,
        depth,
        ch,
        afe: program_afe,
        // LA line n: the stock source number 9 + n (trigger.md 2.2)
        trig_src: m.trig_line().or(bus_line(m)).map_or(src as u32 + 1, |n| 9 + n as u32),
        slope: match (m.trig_type, m.slope) {
            // frames start with a falling edge (start bit, SOF, break, I2C start, CS); 1553, I2S and ARINC 429:
            // either edge
            (TrigType::Bus, _) if matches!(m.dec_proto, Proto::Mil1553 | Proto::I2s | Proto::Arinc429) => tpu::Slope::Either,
            (TrigType::Bus, _) if m.dec_proto.module().is_some() => match m.dec_proto.module().map(|p| p.start_edge()) {
                Some(Slope::Rise) => tpu::Slope::Rising,
                Some(Slope::Either) => tpu::Slope::Either,
                _ => tpu::Slope::Falling,
            },
            (TrigType::Bus, _) => tpu::Slope::Falling,
            (_, Slope::Rise) => tpu::Slope::Rising,
            (_, Slope::Fall) => tpu::Slope::Falling,
            (_, Slope::Either) => tpu::Slope::Either,
        },
        // two-level types: the upper level; all others: the level itself
        level: match m.trig_type {
            TrigType::Slope | TrigType::Runt | TrigType::Window => m.trig_level.max(m.trig_level2),
            TrigType::Bus => m.dec_thr,
            _ => m.trig_level,
        } as f64,
        auto: m.trig_mode == TrigMode::Auto,
        auto_fast: m.auto_fast && m.trig_mode == TrigMode::Auto,
        play4: true,
        blocks: 7,
        header: header_probe(),
        noise_reject: m.trig_nrej,
        trig_couple: match m.trig_coupling() {
            TrigCoup::Dc => tpu::Coupling::Dc,
            TrigCoup::Ac => tpu::Coupling::Ac,
            TrigCoup::LfReject => tpu::Coupling::LfReject,
            TrigCoup::HfReject => tpu::Coupling::HfReject,
        },
        frames: 1,
        compress: 1,
        trigger: hw_trigger(m),
        window: None,
        la,
        segments: (m.seg_hw && m.segments > 0 && hw_segments_ok(m)).then(|| Segments { n: m.segments as u32, interval: m.seg_ivl as f64 }),
        aux: aux_out(m.aux_out),
        // the mask test and zone trigger come from the UI (`Shared::mask_want`, `zone_want`), merged in the acquisition
        // thread (`hw_merge`)
        hwmask: None,
        hwzone: None,
    };
    if let Some(count) = hw_avg_count(m, &s) {
        s.mode = oos_hal::dho::acq::Mode::Average { count };
    }
    if crate::compress::enabled() {
        live_compress(m, &mut s);
    }
    s
}

/// The live compression for the model (compress.rs): the factor of `compress::k_for` for the record of `s` where the
/// model allows it (not in roll or scan), else none.
fn live_compress(m: &Model, s: &mut Settings) {
    if slow_mode(m) != Slow::Off || !crate::compress::allowed(m) || s.la.is_some() || s.segments.is_some() || !matches!(s.mode, oos_hal::dho::acq::Mode::Normal) {
        return;
    }
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    s.compress = crate::compress::k_for(p.rate, s.tdiv, p.memsel);
}

/// The batch was read with the live compression (`live_compress`): its newest frame is read again raw
/// (`seq::read_raw_again`) for the record the UI keeps.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn raw_newest(b: &Settings, p: &hz::Params) -> bool {
    b.compress > 1 && p.memsel <= crate::compress::MAX_DEPTH
}

/// The AUX OUT set-up of the menu choice. Pass/fail takes the default pulse of `auxout::PassFail` (docs/fork/auxout.md).
fn aux_out(a: AuxOut) -> auxout::AuxOut {
    match a {
        AuxOut::TrigOut => auxout::AuxOut::TriggerOut,
        AuxOut::Off => auxout::AuxOut::Off,
        AuxOut::PassFail => auxout::AuxOut::PassFail(auxout::PassFail::DEFAULT),
    }
}

/// The count of a hardware average for the model and its settings `s`, None for the software average
/// (docs/fork/specs/01-hw-averaging.md): Average mode with `Model::avg_hw`, a power-of-two count, ordinary triggered
/// records (not roll or scan, which average in software as in stock), no LA, no hardware segments (they record raw
/// frames), and a record whose average segment fits the frame ring.
fn hw_avg_count(m: &Model, s: &Settings) -> Option<u32> {
    if m.acq != AcqMode::Average || !m.avg_hw || slow_mode(m) != Slow::Off || s.la.is_some() || s.segments.is_some() {
        return None;
    }
    let p = hz::avg_params(hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0));
    oos_hal::dho::avg::hw_count(m.avg_count).filter(|_| p.max_frames > 0)
}

/// The model's set-up averages in the FPGA (`Board::avg_hw`).
pub fn avg_hw_ok(m: &Model) -> bool {
    matches!(settings(m, false).mode, oos_hal::dho::acq::Mode::Average { .. })
}

/// How the FPGA average is read (spec 01, 6.1): running (default), or with OOS_HWAVG=hold a cumulative average of the
/// count that is shown when complete and then cleared. The hardware test in docs/fork/unit-dho804.md decides.
fn avg_kind() -> oos_hal::dho::avg::Kind {
    static K: std::sync::OnceLock<oos_hal::dho::avg::Kind> = std::sync::OnceLock::new();
    *K.get_or_init(|| if std::env::var("OOS_HWAVG").is_ok_and(|v| v == "hold") { oos_hal::dho::avg::Kind::Hold } else { oos_hal::dho::avg::Kind::Running })
}

/// Hardware segments record ordinary triggered frames: not in roll or scan, not with the LA (its record layout and
/// read are not worked out for them, spec 03 5.9).
fn hw_segments_ok(m: &Model) -> bool {
    slow_mode(m) == Slow::Off && !m.la_on()
}

/// Most hardware segments for the model's set-up (`Board::seg_max`): the whole-memory limit of its segment size.
pub fn seg_max(m: &Model) -> Option<usize> {
    if !hw_segments_ok(m) {
        return None;
    }
    let s = settings(m, false);
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    Some(hz::max_record_frames(p.seg) as usize)
}

/// Settings for reading hardware segments back: the live batch plan (compression above 1 M points) with the header
/// on, at most ~4 MB and half the UI queue per read.
pub fn seg_read_plan(s: &Settings, p: &hz::Params) -> Settings {
    let b = batch_plan(s, p);
    let fb = oos_hal::dho::seq::frame_bytes(&oos_hal::dho::seg::read_settings(&b, 1), p).max(1);
    let k = ((4_000_000 / fb) as u32).clamp(1, (crate::board::QUEUE_MAX / 2) as u32);
    oos_hal::dho::seg::read_settings(&b, k)
}

/// Time of the first LA word of a roll frame relative to frame point `pre`, so that the newest word (index `n_la` - 1,
/// spacing `la_dt`) sits at the time of the newest analog point (index `len` - 1, spacing `dt`).
pub fn roll_la_t0(len: usize, pre: usize, dt: f64, n_la: usize, la_dt: f64) -> f64 {
    (len as f64 - 1.0 - pre as f64) * dt - (n_la as f64 - 1.0).max(0.0) * la_dt
}

/// Protocol trigger on LA lines: the line where frames start (as for the channels: SPI / I2S chip select, else data).
fn bus_line(m: &Model) -> Option<u8> {
    if !m.bus_on_la() {
        return None;
    }
    match (m.dec_proto, m.dec_la[2]) {
        (Proto::Spi, Some(n)) | (Proto::I2s, Some(n)) => Some(n),
        _ => m.dec_la[0],
    }
}

/// The UI trigger as a hardware (TPU) trigger. Two-level types: `Settings::level` is the upper level.
fn hw_trigger(m: &Model) -> Trig {
    use tpu::{Edge, Pat, Polarity, VideoStd as HStd, VideoSync as HSync, When, WindowEvent};
    let (t1, t2) = (m.trig_t1 as f64, m.trig_t2 as f64);
    let (when, lower, upper) = match m.trig_when {
        WhenKind::Off => (When::None, 0.0, 0.0),
        WhenKind::Gt => (When::Gt, t1, 0.0),
        WhenKind::Lt => (When::Lt, 0.0, t1),
        WhenKind::In => (When::In, t1.min(t2), t1.max(t2)),
        WhenKind::Out => (When::Out, t1.min(t2), t1.max(t2)),
    };
    let pol = if m.trig_pol == Pol::Pos { Polarity::Pos } else { Polarity::Neg };
    let lo = m.trig_level.min(m.trig_level2) as f64;
    // Duration has no edges: what is left from Pattern counts as don't care, as in the software trigger
    let pats = m.pat.map(|b| match b {
        Bit::H => Pat::H,
        Bit::L => Pat::L,
        Bit::X => Pat::X,
        Bit::R | Bit::F if m.trig_type == TrigType::Duration => Pat::X,
        Bit::R => Pat::Rise,
        Bit::F => Pat::Fall,
    });
    let levels = m.pat_levels.map(|v| v as f64);
    // D lines: H / L / X (an edge is only offered on the channels)
    let la = if m.la_on() { m.pat_la.map(|b| match b { Bit::H => Pat::H, Bit::L => Pat::L, _ => Pat::X }) } else { [Pat::X; 16] };
    let edge2 = if m.trig_slope2 == Slope::Fall { Edge::Falling } else { Edge::Rising };
    match m.trig_type {
        TrigType::Edge => Trig::Edge,
        TrigType::Pulse => Trig::Pulse { pol, when, lower, upper },
        TrigType::Slope => Trig::Slope { when, lower, upper, lo },
        TrigType::Runt => Trig::Runt { pol, when, lower, upper, lo },
        TrigType::Window => Trig::Window {
            event: match m.win_mode {
                WinMode::Enter => WindowEvent::Enter,
                WinMode::Exit => WindowEvent::Exit,
                WinMode::Inside => WindowEvent::Time,
            },
            time: t1,
            lo,
        },
        TrigType::Timeout => Trig::Timeout { time: t1 },
        TrigType::Delay => Trig::Delay { srcb: m.trig_src2 as u32 + 1, eb: edge2, when, lower, upper, levelb: m.trig_level2 as f64 },
        TrigType::Pattern => Trig::Pattern { pats, la, levels },
        TrigType::Duration => Trig::Duration { pats, la, when, lower, upper, levels },
        TrigType::SetupHold => Trig::SetupHold { clk: m.trig_src2 as u32 + 1, clk_edge: edge2, setup: m.setup as f64, hold: m.hold as f64, levelc: m.trig_level2 as f64 },
        TrigType::NthEdge => Trig::Nth { count: m.nth_n, idle: m.nth_idle as f64 },
        // protocol trigger: the TPU fires on edges of the decoder's line (see `settings`), the software picks the
        // matching frame out of the record
        TrigType::Bus => Trig::Edge,
        TrigType::Video => Trig::Video {
            std: if m.video_std == VideoStd::Pal { HStd::Pal } else { HStd::Ntsc },
            // the TPU has no "all fields": odd field triggers once per frame [A]
            sync: match m.video_sync {
                VSync::AllLines => HSync::AnyLine,
                VSync::Line => HSync::Line,
                VSync::Odd | VSync::AllFields => HSync::OddField,
                VSync::Even => HSync::EvenField,
            },
            line: m.video_line,
        },
    }
}

/// Trigger investigation (trigger.md 8): with OOS_FRAME_HEADER set, frames are read with the 16-byte header of the stock
/// live loop and every frame's header is logged to /tmp/oos-hdr.log next to where the edge actually is.
fn header_probe() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OOS_FRAME_HEADER").is_some())
}

/// `runs` runs of the measure unit, folded into the levels of the channels of `s` (`oos_hal::dho::levels`), in the
/// volts of the records.
#[cfg(target_os = "linux")]
fn levels_measure(bar: &mut oos_hal::dho::bus::Bar1, s: &Settings, gain: &[f64; 4], runs: u32) -> [Option<crate::board::ChLevels>; 4] {
    use oos_hal::dho::levels;
    let mut got = Vec::with_capacity(runs as usize);
    for _ in 0..runs {
        match levels::run(bar, &mut |us| std::thread::sleep(std::time::Duration::from_micros(us as u64))) {
            Some(r) => got.push(r),
            None => return [None; 4],
        }
    }
    let vert = std::array::from_fn(|c| levels::Vert { scale: s.ch[c].scale, offset: s.ch[c].offset, gain: gain[c] });
    levels::channel_levels(&got, s.mask, &vert).map(|l| l.map(|l| crate::board::ChLevels { lo: l.lo as f32, hi: l.hi as f32, mean: l.mean as f32, clipped: l.clipped }))
}

/// The trigger status from the SCU status bits, cleared after every batch as the stock UI does
/// (docs/fork/trigger-lockin.md; verified on the unit 2026-11-13). `trig.hw_status = off` in system.conf, or
/// OOS_TRIG_HW_STATUS=0, takes it from the software refinement instead.
pub fn hw_status_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let env = std::env::var("OOS_TRIG_HW_STATUS").ok().map(|v| if v.is_empty() || v == "0" { "off" } else { "on" });
        let conf = std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default();
        crate::drawpipe::conf_value(env, &conf, "trig.hw_status").as_deref() != Some("off")
    })
}

/// `dvm.hw = on` in system.conf: the DVM reads the FPGA's DVM block (docs/fork/dvm.md) instead of the records. Off by
/// default until the result layout is verified on the unit.
pub fn dvm_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| dvm_hw_from(&std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default()))
}

/// The `dvm.hw` line of system.conf; off unless `on`.
fn dvm_hw_from(conf: &str) -> bool {
    crate::drawpipe::conf_value(None, conf, "dvm.hw").as_deref() == Some("on")
}

/// `mask.hw = on` in system.conf: the mask test runs in the FPGA's mask unit (docs/fork/hwmask.md) and the software
/// test only where it cannot. Off by default until the sample scale and the slots are verified on the unit.
pub fn mask_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| mask_hw_from(&std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default()))
}

/// The `mask.hw` line of system.conf; off unless `on`.
fn mask_hw_from(conf: &str) -> bool {
    crate::drawpipe::conf_value(None, conf, "mask.hw").as_deref() == Some("on")
}

/// `zone.hw = on` in system.conf: the zone trigger qualifies frames in the FPGA (docs/fork/hwzone.md) and the records
/// are no longer filtered in software where it can. Off by default until the mapping is verified on the unit.
pub fn zone_hw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| zone_hw_from(&std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default()))
}

/// The `zone.hw` line of system.conf; off unless `on`.
fn zone_hw_from(conf: &str) -> bool {
    crate::drawpipe::conf_value(None, conf, "zone.hw").as_deref() == Some("on")
}

/// The UI's mask test and zone trigger into the acquisition settings. The mask unit runs one of them: the mask test
/// wins. AUX OUT pass/fail follows the zone result while the zone trigger runs (0x119c source bit).
fn hw_merge(want: &mut Settings, mask: Option<&crate::board::MaskWant>, zone: Option<&crate::board::ZoneWant>) {
    want.hwmask = mask.map(hw_mask);
    want.hwzone = zone.filter(|_| mask.is_none()).map(|z| oos_hal::dho::seq::HwZone {
        ch: z.ch,
        zones: z.zones.iter().take(2).map(|(poly, inside)| oos_hal::dho::zone::Zone { poly: poly.clone(), inside: *inside }).collect(),
        clear: z.clear,
    });
    if let (auxout::AuxOut::PassFail(p), Some(_)) = (&mut want.aux, &want.hwzone) {
        p.source = auxout::Source::Zone;
    }
}

/// The mask test of the acquisition settings from the UI's wish.
fn hw_mask(w: &crate::board::MaskWant) -> oos_hal::dho::seq::HwMask {
    oos_hal::dho::seq::HwMask { ch: w.ch, lo: w.lo.clone(), hi: w.hi.clone(), stop_on_fail: w.stop, clear: w.clear }
}

/// Seconds between two readings of the DVM block, and from its (re)start to the first one. The window of 2^28 samples
/// is 0.21 s at 1.25 GSa/s; how long it really takes is open (docs/fork/dvm.md).
const DVM_READ_S: f64 = 0.5;

/// What the DVM block runs on: channel, lane, and the number of the acquisition configuration it was started after.
type DvmKey = (usize, u32, u64);

/// The next step of the DVM block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DvmStep {
    Idle,
    /// (re)start on this lane
    Start(u32),
    Stop,
    Read,
}

/// The DVM block's next step: (re)started when the wanted channel, its lane in the acquired set `mask` or the
/// acquisition configuration (`config`, raised on every configuration) changed; stopped when nothing is wanted or the
/// channel is not acquired; read `DVM_READ_S` after the start or the last reading.
fn dvm_step(want: Option<usize>, mask: u32, config: u64, running: Option<DvmKey>, elapsed_s: f64) -> (DvmStep, Option<DvmKey>) {
    let target = want.and_then(|ch| oos_hal::dho::dvm::lane(mask, ch).map(|l| (ch, l, config)));
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
fn dvm_reading(raw: &oos_hal::dho::dvm::Raw, key: DvmKey, s: &Settings, gain: &[f64; 4], n: u64) -> crate::board::DvmRead {
    use oos_hal::dho::dvm;
    let (ch, lane, _) = key;
    let vert = oos_hal::dho::levels::Vert { scale: s.ch[ch].scale, offset: s.ch[ch].offset, gain: gain[ch] };
    let volts = dvm::decode(raw, dvm::EXP).map(|r| [dvm::Mode::Dc, dvm::Mode::AcRms, dvm::Mode::AcDcRms].map(|m| dvm::volts(&r, m, &vert, dvm::SCALE)));
    crate::board::DvmRead { ch, lane, exp: dvm::EXP, raw: [raw.cfg, raw.sum_lo, raw.sum_hi, raw.sq_lo, raw.sq_hi], volts, n }
}

/// Arm the next batch before the decode of the current one (`seq::prearm`), so the decode is no longer dead time.
/// `acq.prearm = on` in system.conf, or OOS_PREARM set (any value). Off by default: the stop of an acquisition armed
/// ahead that is then not read (a settings change, the generator, self-cal, roll) is not verified on the unit yet
/// (docs/fork/specs/10-transfer-efficiency.md).
fn prearm_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let conf = std::fs::read_to_string(crate::sysconf::default_path()).unwrap_or_default();
        prearm_from(std::env::var_os("OOS_PREARM").is_some(), &conf)
    })
}

/// OOS_PREARM set, else the `acq.prearm` line of system.conf; off unless `on`.
fn prearm_from(env_set: bool, conf: &str) -> bool {
    env_set || crate::drawpipe::conf_value(None, conf, "acq.prearm").as_deref() == Some("on")
}

/// Stops the acquisition armed ahead, if one is, before anything else writes the FPGA.
#[cfg(target_os = "linux")]
fn disarm(armed: &mut Option<(Settings, u64)>, state: &mut oos_hal::dho::seq::State, bar: &mut oos_hal::dho::bus::Bar1, c2h: &mut std::fs::File) {
    if armed.take().is_some() {
        if let Err(e) = oos_hal::dho::seq::disarm(state, bar, c2h, |_| {}) {
            eprintln!("oos-ui hw: disarm: {}", e);
        }
    }
}

/// One line of the header log: the 8 header words, `pre`, and the offsets (samples) of the nearest rising and falling
/// crossing of the trigger level from `pre` on the trigger channel.
fn log_header(h: &[u8], v: &[f32], s: &Settings, pre: usize, k: usize) {
    use std::io::Write;
    static LINES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if LINES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 20_000 {
        return;
    }
    let words: Vec<String> = h.chunks_exact(2).map(|b| format!("{:04x}", u16::from_le_bytes([b[0], b[1]]))).collect();
    let tc = (s.trig_src.clamp(1, 4) - 1) as usize;
    let hyst = (0.3 * s.ch[tc].scale) as f32;
    let span = v.len();
    let off = |rising: bool| crate::fastdraw::edge_near(v, pre, s.level as f32, hyst, rising, false, span).map_or("-".to_string(), |t| format!("{:.1}", t - pre as f64));
    let line = format!("{} pre {} k {} n {} rise {} fall {} scale {} tdiv {:e}\n", words.join(" "), pre, k, v.len(), off(true), off(false), s.ch[tc].scale, s.tdiv);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/oos-hdr.log") {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Decodes a read-out of the whole record into a frame.
pub fn decode(bytes: &[u8], s: &Settings, p: &hz::Params, seq: u64) -> Frame {
    let mut f = Frame::default();
    decode_into(bytes, s, p, seq, &mut f);
    f
}

/// `decode` into a kept frame: its channel vectors are refilled (no allocation once they have room), every other
/// field is set as `decode` sets it.
pub fn decode_into(bytes: &[u8], s: &Settings, p: &hz::Params, seq: u64, f: &mut Frame) {
    let (hdr, bytes) = if s.header && bytes.len() >= 16 { bytes.split_at(16) } else { (&bytes[..0], bytes) };
    oos_hal::dho::seq::decode_volts_into(bytes, s.mask, s.ch.map(|k| (k.scale, k.offset)), &mut f.ch);
    // with FPGA compression every point stands for k samples (min/max pairs, acquisition-start.md)
    let k = s.compress.max(1) as usize;
    if !hdr.is_empty() && header_probe() {
        let tc = (s.trig_src.clamp(1, 4) - 1) as usize;
        log_header(hdr, &f.ch[tc], s, p.pre as usize / k, k);
    }
    // a window of the record (`batch_plan`): the trigger point counts from its first point
    let (_, start) = oos_hal::dho::seq::window(s, p);
    let pre = p.pre.saturating_sub(start) as usize / k;
    let ch = std::mem::take(&mut f.ch);
    *f = Frame { seq, dt: k as f64 / p.rate as f64, pre, ch, roll: false, envelope: k > 1, scan: false, partial: false, acq: (p.rate as f64, p.memsel as usize), stamp: 0, la: None, hw_avg: false };
}

/// Read-out buffers of the batch mode: one being read, two with the deposit workers (each holds one share and has room
/// for one more).
#[cfg(target_os = "linux")]
const POOL: usize = 3;

/// The read-out buffer lent to the deposit workers as it is (drawpipe.rs).
impl crate::drawpipe::RawBatch for oos_hal::dho::bus::DmaBuf {
    fn bytes(&self) -> &[u8] {
        self.data()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        self
    }
}

/// The LA words of the acquisition `p` (read with `seq::acquire_la`) as a record: word j of the window at
/// (start + j - trigger index) / LA rate from the hardware trigger point (la.md 3.3; the trigger index is [A] until
/// test T1).
pub fn la_record(words: Vec<u16>, s: &Settings, p: &hz::Params) -> crate::ui::la::LaRec {
    let t = oos_hal::dho::la::timing(p, s.mask);
    let (_, start) = oos_hal::dho::la::window(&t);
    let dt = 1.0 / t.rate.max(1) as f64;
    crate::ui::la::LaRec { words: Arc::new(words), t0: (start as f64 - t.pre as f64) * dt, dt }
}

/// hal bandwidth code of a BWS entry.
pub fn bw_code(i: usize) -> u32 {
    match i {
        0 => afe::bw::AUTO,
        BW_FULL => afe::bw::OFF,
        BW_20M => afe::bw::S20M,
        3 => afe::bw::GND,
        _ => afe::bw::mhz(BWS[i.min(BWS.len() - 1)].1),
    }
}

/// Points of a live frame read from a record above 1 M points (the stock live length at 2.5 M / k = 4).
const LIVE_READ: u32 = 625_000;

/// Batch settings for a configured acquisition: frames per batch (at most ~4 MB per read, the frame ring, 1000 /
/// ratio) and, for records above 1 M points, what of the record is read for the screen:
/// - the screen holds most of the record: the whole record through the FPGA compression to ~`LIVE_READ` points;
/// - the screen is a small part of it (a fixed depth at a short time base, at the full rate): the screen window with a
///   margin, every sample. Compressed, 10 M at 100 ns/div left about 80 min/max points for the screen.
pub fn batch_plan(s: &Settings, p: &hz::Params) -> Settings {
    let mut b = s.clone();
    if p.memsel > 1_000_000 {
        match screen_window(s, p) {
            Some(w) => b.window = Some(w),
            None => b.compress = p.memsel.div_ceil(LIVE_READ),
        }
    }
    let fb = oos_hal::dho::seq::frame_bytes(&b, p).max(1);
    // hardware average: one frame is read whatever the batch holds, so the read size does not limit it
    let by_size = if matches!(s.mode, oos_hal::dho::acq::Mode::Average { .. }) { u32::MAX } else { (4_000_000 / fb) as u32 };
    b.frames = by_size.min(p.max_frames).min(1000 / p.ratio.max(1)).max(1);
    b
}

/// The part of a deep record the screen shows, as a read-out window (`Settings::window`: points, first point from the
/// trigger point), when it fits `LIVE_READ` points: the screen and a quarter of a screen on each side (at least 4096
/// points), widened to the trigger point and 4096 points around it for the software trigger (a delay moves the screen
/// away from it).
/// The record is centred on the screen (`hz::prep`: it starts half a record before the delay). None with the LA,
/// which reads its own window of the same record.
fn screen_window(s: &Settings, p: &hz::Params) -> Option<(u32, i64)> {
    window_below(s, p, LIVE_READ as i64)
}

/// `screen_window` for windows of fewer than `cap` points.
fn window_below(s: &Settings, p: &hz::Params, cap: i64) -> Option<(u32, i64)> {
    if s.la.is_some() || p.rate == 0 {
        return None;
    }
    let screen = (s.tdiv * 10.0 * p.rate as f64).ceil() as i64;
    let margin = (screen / 4).max(4096);
    let (mem, pre, mid) = (p.memsel as i64, p.pre as i64, p.memsel as i64 / 2);
    // a multiple of 8 points like the stock TX length, inside the record
    let lo = ((mid - screen / 2 - margin).min(pre - 4096).max(0)) / 8 * 8;
    let hi = (mid + screen / 2 + margin).max(pre + 4096).min(mem);
    let n = (hi - lo + 7) / 8 * 8;
    if n >= cap || lo + n > mem {
        return None;
    }
    Some((n as u32, lo - pre))
}

/// The raw read of a deep record's screen window for the measurements (deepmeas.rs): the batch `b` read the whole
/// record through the compression (`batch_plan`, above 1 M points), Normal acquisition, no LA; the window of
/// `screen_window` with the record's edge allowed, at most `deepmeas::MAX_BYTES`. Settings for `seq::read_raw_again`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn deep_raw_settings(b: &Settings, p: &hz::Params) -> Option<Settings> {
    if b.compress < 2 || b.window.is_some() || p.memsel <= crate::compress::MAX_DEPTH || !matches!(b.mode, oos_hal::dho::acq::Mode::Normal) {
        return None;
    }
    let cap = (crate::deepmeas::MAX_BYTES / (2 * p.ratio.max(1) as usize)) as i64 + 1;
    // the whole record when the window would reach past it (`window_below` refuses a window beyond the end)
    let w = window_below(b, p, cap).or_else(|| ((p.memsel as i64) < cap && b.la.is_none()).then(|| (p.memsel, -(p.pre as i64))))?;
    Some(oos_hal::dho::seq::raw_settings(&Settings { window: Some(w), ..b.clone() }))
}

/// The FPGA's frequency counter, once per pass of the acquisition loop: (re)started when the UI wants another set-up,
/// read once a gate (and 0.1 s) has passed since the start or the last reading (docs/protocols/counter.md).
#[cfg(target_os = "linux")]
fn counter_tick(bar: &mut oos_hal::dho::bus::Bar1, out: &HwShared, counting: &mut Option<(crate::board::CounterWant, std::time::Instant)>, n: &mut u64) {
    use oos_hal::dho::bus::Bus;
    use oos_hal::dho::counter;
    let want = out.lock().unwrap().counter_want;
    if want != counting.map(|c| c.0) {
        match want {
            Some(w) => {
                let mode = [counter::Mode::Frequency, counter::Mode::Period, counter::Mode::Totalize][w.mode.min(2)];
                bar.apply(&counter::start(&counter::Config { ch: w.ch, mode, digits: w.digits }));
                *counting = Some((w, std::time::Instant::now()));
            }
            None => {
                bar.apply(&counter::stop());
                *counting = None;
            }
        }
        out.lock().unwrap().counter = None;
        return;
    }
    if let Some((w, t)) = *counting {
        let gate = counter::gate_s(w.digits);
        if t.elapsed().as_secs_f64() >= gate + 0.1 {
            let r = counter::read(bar);
            *n += 1;
            out.lock().unwrap().counter = Some(crate::board::CounterRead { period: counter::period(&r, gate), total: r.total, n: *n });
            *counting = Some((w, std::time::Instant::now()));
        }
    }
}

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
                status(format!("calibration: {}", from.iter().map(|(n, d)| format!("{} from {}", n, d)).collect::<Vec<_>>().join(", ")));
                Some(c)
            }
            Err(_) => None,
        };
        let mut cal_adc = adc::CalAdc::load(&cal_dir).ok();
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
        let mut rolling: Option<oos_hal::dho::roll::Params> = None;
        let mut ring: [std::collections::VecDeque<f32>; NCH] = Default::default();
        // the same for the LA words: ring, sweep picture (as f32 for `scan_write`; a word is exact in it)
        let mut ring_la: std::collections::VecDeque<u16> = Default::default();
        let mut sweep_la: Vec<f32> = Vec::new();
        let mut published = std::time::Instant::now();
        // scan: the sweep picture per channel (one screen), write position, last 0x1204, SINGLE sweep complete
        let mut sweep: [Vec<f32>; NCH] = Default::default();
        let mut cursor = 0usize;
        let mut last_info = oos_hal::dho::roll::ScanInfo::default();
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
        // the read buffer of the batches, kept (bus::DmaBuf)
        let mut dma = oos_hal::dho::bus::DmaBuf::new();
        // the raw second read of the newest frame of a compressed batch (compress.rs)
        let mut raw_dma = oos_hal::dho::bus::DmaBuf::new();
        // the last raw read of a deep record's screen window for the measurements (deepmeas.rs)
        let mut deep_last: Option<std::time::Instant> = None;
        // read-out buffers made so far: this one, and up to `POOL` in all lent to the deposit workers in batch mode
        let mut pool_made = 1usize;
        // this thread's share of the deposit work while it waits (drawpipe.rs)
        let mut helper = crate::drawpipe::Helper::default();
        // the batch armed ahead (`prearm_on`) and the SINGLE stamp it was armed under
        let mut armed: Option<(Settings, u64)> = None;
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
        // the last request of the measure-unit levels answered
        let mut levels_done: Option<u64> = None;
        let mut slots_done: Option<u64> = None;
        loop {
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
                    bar.apply(&oos_hal::dho::roll::stop(&mut state.sh));
                }
                let res = match (afe_bus.as_mut(), cal.as_ref()) {
                    (Some(afe), Some(c)) => {
                        let cancel_out = out.clone();
                        let cancel = move || cancel_out.lock().unwrap().cal_cancel;
                        let mut unit = oos_hal::dho::selfcal::Unit { bar: &mut bar, afe, c2h: &mut c2h, state: &mut state, cal: c, cal_adc: cal_adc.as_ref(), cancel: &cancel, adc_new: None };
                        let prog_out = out.clone();
                        let adc_base = if adc_cores { cal_adc.as_ref() } else { None };
                        let r = oos_hal::dho::selfcal::run_all_with(&mut unit, c, adc_base, &chans, zero, &mut |d, t, s| {
                            prog_out.lock().unwrap().cal_progress = Some((d, t, s.to_string()));
                        });
                        match r {
                            Ok(r) => oos_hal::dho::selfcal::save(&r, &cal_dir).map(|files| format!("{} configurations, largest change {:.2} %, saved {}", r.measured, r.worst * 100.0, files.join(", "))),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err("self-cal needs the AFE bus (--afe) and the calibration files".into()),
                };
                status(format!("self-cal CH{:?}: {}", chans.iter().map(|c| c + 1).collect::<Vec<_>>(), match &res { Ok(s) => format!("done, {}", s), Err(e) => format!("failed: {}", e) }));
                // the new tables take effect at the next set-up
                if res.is_ok() {
                    cal = afe::Cal::load(&cal_dir).ok();
                    cal_adc = adc::CalAdc::load(&cal_dir).ok();
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
                        afg_io = oos_hal::dho::afg::Io::open().map_err(|e| status(format!("generator GPIO: {}", e))).ok();
                    }
                    if let Some(io) = afg_io.as_mut() {
                        // a running output keeps running when the change allows it
                        let quick = afg_done.filter(|d| on && d.0 && d.4 == wave && d.5 == md && md == oos_hal::dho::afg::Mod::None).and_then(|d| oos_hal::dho::afg::retune(afg_cal.as_ref(), wave, d.1 * 1e6, f * 1e6, a * 1e9, o * 1e9, io.relays()));
                        let ops = if let Some(q) = quick { q } else if on { oos_hal::dho::afg::start_mod(afg_cal.as_ref(), wave, f * 1e6, a * 1e9, o * 1e9, io.relays(), md) } else { oos_hal::dho::afg::idle(a * 1e9, o * 1e9, f * 1e6, io.relays()) };
                        io.apply(&mut bar, &ops);
                        status(format!("generator: {}", if on { format!("{:?} {} Hz {} Vpp {} V {:?}", wave, f, a, o, md) } else { "off".into() }));
                    }
                }
                afg_done = afg_want;
            }
            // read with the generator request; the settings read below are as new or newer
            stamp = stamp_now;
            let (want, want_mode, avg_clear, mask_want, zone_want) = {
                let g = out.lock().unwrap();
                (g.want.clone(), g.slow, g.avg_clear, g.mask_want.clone(), g.zone_want.clone())
            };
            let Some(mut want) = want else {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            };
            // the settings as the UI gave them: when they change, a wait for a trigger ends early (`acquire_batch_until`)
            let asked = (Some(want.clone()), want_mode);
            want.afe = program_afe;
            hw_merge(&mut want, mask_want.as_ref(), zone_want.as_ref());
            if let Some(l) = want.la.as_mut() {
                l.cal = la_cal;
            }
            if current.as_ref().map_or(true, |c| c.0 != want) || mode != want_mode {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &out);
                mode = want_mode;
                if rolling.take().is_some() {
                    bar.apply(&oos_hal::dho::roll::stop(&mut state.sh));
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
                        // the configuration cleared the average (`seq::State::configure`)
                        avg = match want.mode {
                            oos_hal::dho::acq::Mode::Average { count } => Some((oos_hal::dho::avg::Tracker::new(avg_kind(), count), avg_clear, 0)),
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
            // the DVM block (`dvm.hw = on`): (re)started while nothing is armed, read like the counter
            {
                use oos_hal::dho::dvm;
                let want = out.lock().unwrap().dvm_want.map(|w| w.ch);
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
            if let Some((id, _, runs)) = lw.filter(|w| mode == Slow::Off && stamp >= w.1 && levels_done != Some(w.0)) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                let v = levels_measure(&mut bar, &s, &state.gain, runs);
                out.lock().unwrap().levels = Some((id, v));
                levels_done = Some(id);
            }
            // debug: one raw run of the measure unit for `:DEBug:SLOTs?`
            let sw = out.lock().unwrap().slots_want;
            if let Some(id) = sw.filter(|&id| mode == Slow::Off && slots_done != Some(id)) {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                let r = oos_hal::dho::levels::run(&mut bar, &mut |us| std::thread::sleep(std::time::Duration::from_micros(us as u64)));
                out.lock().unwrap().slots = Some((id, r.map(|r| r.map(|x| [x.min as f32, x.max as f32, x.mean as f32]))));
                slots_done = Some(id);
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
                let sweep_start = |state: &mut seq::State, bar: &mut oos_hal::dho::bus::Bar1, rp: &oos_hal::dho::roll::Params| {
                    match scan {
                        Some((delay_fs, _)) => {
                            let pos = oos_hal::dho::roll::scan_trig_pos(delay_fs, tdiv_fs, p.dot_fs);
                            bar.apply(&oos_hal::dho::roll::config_scan(&mut state.sh, rp, p.memsel, pos));
                        }
                        None => bar.apply(&oos_hal::dho::roll::config(&mut state.sh, rp, p.memsel)),
                    }
                    if let Some(t) = &la_t {
                        bar.apply(&oos_hal::dho::roll::la_config(rp, t));
                    }
                    bar.apply(&oos_hal::dho::roll::start(&mut state.sh, s.mask));
                };
                let rp = match rolling {
                    Some(rp) => rp,
                    None => {
                        let rp = oos_hal::dho::roll::params(s.mask, tdiv_fs, s.depth, &p);
                        sweep_start(&mut state, &mut bar, &rp);
                        ring.iter_mut().for_each(|r| r.clear());
                        sweep.iter_mut().for_each(|v| v.clear());
                        ring_la.clear();
                        sweep_la.clear();
                        (cursor, halted, done_seen, last_info) = (0, false, None, Default::default());
                        let name = if scan.is_some() { "scan" } else { "roll" };
                        status(format!("{}: {:.1} us per point, chunk {:.2} ms", name, rp.spacing_fs as f64 / 1e9, rp.spacing_fs as f64 * 100.0 / rp.ratio as f64 / 1e12));
                        rolling = Some(rp);
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
                let la_dt = if la_t.is_some() { oos_hal::dho::roll::la_spacing_fs(&rp) as f64 / 1e15 } else { dt };
                let keep_la = ((10.0 * s.tdiv / la_dt).round() as usize).max(2);
                bar.apply(&oos_hal::dho::roll::fill_pulse(&state.sh));
                let c = oos_hal::dho::roll::chunks(bar.rd(0x11e8));
                let c = if la_t.is_some() { oos_hal::dho::roll::la_chunks(c) } else { c };
                if c == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(9));
                } else {
                    let buf = match oos_hal::dho::bus::read_dma(&mut c2h, c * oos_hal::dho::roll::CHUNK_BYTES, |_, _| {}) {
                        Ok(b) => b,
                        Err(e) => {
                            status(format!("{} read of {} chunks: {}", if scan.is_some() { "scan" } else { "roll" }, c, e));
                            rolling = None;
                            current = None;
                            continue;
                        }
                    };
                    let (w, lw) = if la_t.is_some() { oos_hal::dho::roll::split_la(&buf, s.mask) } else { (oos_hal::dho::roll::split(&buf, s.mask), Vec::new()) };
                    if scan.is_some() {
                        // the chunk counter restarts with every sweep: its column is the write position of the first
                        // analog chunk
                        let per = oos_hal::dho::roll::points_per_chunk(s.mask);
                        let first = buf.chunks_exact(oos_hal::dho::roll::CHUNK_BYTES).find(|c| !oos_hal::dho::roll::is_la_chunk(c)).unwrap_or(&buf[..oos_hal::dho::roll::CHUNK_BYTES]);
                        let col = oos_hal::dho::roll::scan_column(oos_hal::dho::roll::counter(first), la_t.is_some());
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
                            scan_write(&mut sweep[ch], keep, cursor, &seq::volts(&w[ch], s.ch[ch].scale, s.ch[ch].offset));
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
                        let r = oos_hal::dho::roll::SCAN_INFO_REGS.map(|a| bar.rd(a));
                        let info = oos_hal::dho::roll::scan_info(r[0], r[1]);
                        if info.triggered != last_info.triggered || info.done {
                            status(format!("scan: 0x1204 = {:#010x}, 0x40d0 = {:#x}, cursor {} of {}", r[0], r[1], cursor, keep));
                        }
                        last_info = info;
                        // done arrives a few chunks before the end of the sweep (unit: 4 chunks): read on until the
                        // last column is in (cursor back at 0) or 100 ms passed
                        if info.done {
                            done_seen = Some(std::time::Instant::now());
                        }
                    }
                    if done_seen.is_some_and(|t| cursor == 0 || t.elapsed().as_millis() >= 100) {
                        status(format!("scan: sweep complete {} ms after done, cursor {} of {}", done_seen.unwrap().elapsed().as_millis(), cursor, keep));
                        done_seen = None;
                        done = true;
                        // stock state 8 (RUN): stop, clear the average, configure again, next sweep after 500 ms; state 9
                        // (SINGLE): stop
                        bar.apply(&oos_hal::dho::roll::scan_done(&mut state.sh));
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
                        let la = la_t.map(|_| crate::ui::la::LaRec { words: Arc::new(sweep_la.iter().take_while(|x| !x.is_nan()).map(|&x| x as u16).collect()), t0: 0.0, dt: la_dt });
                        Frame { seq: n, dt, pre: 0, ch, roll: false, envelope: true, scan: true, partial: !done, acq: (0.0, 0), stamp, la, hw_avg: false }
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
                        Frame { seq: n, dt, pre, ch: ring.clone().map(|r| r.into_iter().collect()), roll: true, envelope: true, scan: false, partial: len < keep, acq: (0.0, 0), stamp, la, hw_avg: false }
                    };
                    let mut g = out.lock().unwrap();
                    // a complete sweep must not be dropped by the next partial picture
                    if g.queue.back().map_or(true, |b| b.partial) {
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
                bar.apply(&oos_hal::dho::roll::stop(&mut state.sh));
            }
            if s.segments.is_some() {
                disarm(&mut armed, &mut state, &mut bar, &mut c2h);
                if let Err(e) = seg_step(&mut segrun, &s, &p, &mut state, &mut bar, &mut c2h, &mut dma, &out, &mut n, stamp) {
                    status(format!("segments: {}", e));
                    seg_abort(&mut segrun, &mut state, &mut bar, &mut c2h, &out);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                continue;
            }
            let b = batch_plan(&s, &p);
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
                (g.want.as_ref(), g.slow) != (asked.0.as_ref(), asked.1)
            };
            // while the FPGA's ring fills this thread draws frames of the last batch (drawpipe.rs), one per look at
            // the frame count, so the read waits at most one frame longer
            let pipe = crate::drawpipe::pipe();
            let idle = || pipe.is_some_and(|x| x.help(&mut helper));
            let got = seq::acquire_batch_idle(&mut state, &b, &p, &mut bar, &mut c2h, &mut dma, ahead, 25, |_| {}, changed, idle);
            if let Some(x) = pipe {
                x.help_done(&mut helper);
            }
            match got {
                Ok(count) => {
                    // LA on: the second, raw read of the same record before anything else touches the FPGA
                    // after a few failures in a row the LA read stops until the next set-up: every failed read waits for
                    // its time-out, and the analog frames must not wait with it
                    let la_words = match b.la {
                        Some(_) if la_fails < 3 => match seq::acquire_la(&mut state, &b, &p, &mut bar, &mut c2h, |_| {}) {
                            Ok(w) => {
                                la_fails = 0;
                                Some(w)
                            }
                            Err(e) => {
                                la_fails += 1;
                                status(format!("LA read ({} of 3): {}", la_fails, e));
                                None
                            }
                        },
                        _ => None,
                    };
                    // live compression: the newest frame once more, raw, before anything arms the FPGA again
                    let raw_frame = if raw_newest(&b, &p) && count > 0 {
                        match seq::read_raw_again(&mut state, &b, &p, &mut bar, &mut c2h, &mut raw_dma, count - 1, |_| {}) {
                            Ok(()) => Some(decode(raw_dma.data(), &seq::raw_settings(&b), &p, 0)),
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
                    let deep = deep_want.filter(|w| count > 0 && crate::deepmeas::due(*w, deep_last, std::time::Instant::now())).and_then(|_| deep_raw_settings(&b, &p));
                    let raw_frame = match deep {
                        Some(ds) if raw_frame.is_none() => {
                            deep_last = Some(std::time::Instant::now());
                            match seq::read_raw_again(&mut state, &ds, &p, &mut bar, &mut c2h, &mut raw_dma, count - 1, |_| {}) {
                                Ok(()) => Some(decode(raw_dma.data(), &ds, &p, 0)),
                                Err(e) => {
                                    status(format!("raw read of the deep window: {}", e));
                                    None
                                }
                            }
                        }
                        _ => raw_frame,
                    };
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
                        g.mask = mr.map(|(ch, slot, ctrl, raw)| crate::board::MaskRead { ch, slot, ctrl, raw: [raw.frames_lo, raw.frames_hi, raw.fails_lo, raw.fails_hi], frames: raw.frames(), fails: raw.fails(), n });
                    }
                    // the zone unit's status and qualified-frame counter (docs/fork/hwzone.md)
                    let zr = state.zone_ctrl().zip(b.hwzone.as_ref()).zip(state.zone_run).map(|((ctrl, z), run)| (z.ch, run.0, ctrl, oos_hal::dho::zone::read(&mut bar, ctrl)));
                    {
                        let mut g = out.lock().unwrap();
                        let n = g.zone.map_or(0, |r| r.n) + 1;
                        g.zone = zr.map(|(ch, slot, ctrl, r)| {
                            let c = r.counters;
                            crate::board::ZoneRead { ch, slot, ctrl, raw: [r.s4030, r.s4038, c.frames_lo, c.frames_hi, c.fails_lo, c.fails_hi], frames: c.frames(), armed: r.armed(), pass: r.pass(), n }
                        });
                    }
                    // hardware average: one frame, shown or not, the average cleared after a complete hold average
                    let after = avg.as_mut().map(|(tr, _, logged)| {
                        let a = tr.batch(count);
                        if *logged < 32 {
                            // hardware check: 0x10b0 progress and the first words next to the triggers counted here
                            *logged += 1;
                            let w: Vec<String> = dma.data().chunks_exact(2).take(4).map(|x| format!("{:04x}", u16::from_le_bytes([x[0], x[1]]))).collect();
                            eprintln!("oos-ui hw: average {:?} batch {} of {} triggers, {} in, show {}, 0x10b0 = {}, words {}", tr.kind, *logged, count, a.triggers, a.show, oos_hal::dho::avg::progress(&mut bar), w.join(" "));
                        }
                        a
                    });
                    // the next batch records while this one is decoded (after the LA read, which needs this record);
                    // not in average mode, where a clear may have to come first
                    if prearm_on() && avg.is_none() {
                        match seq::prearm(&mut state, &b, &p, &mut bar, &mut c2h, |_| {}) {
                            Ok(()) => armed = Some((b.clone(), stamp)),
                            Err(e) => status(format!("prearm: {}", e)),
                        }
                    }
                    let fb = seq::frame_bytes(&b, &p);
                    // decode before taking the lock: the UI must not wait for 100 k-point conversions
                    let shown = if after.is_some_and(|a| !a.show) { 0 } else { seq::batch_read(&b, count).1 };
                    let avail = if fb == 0 { 0 } else { (shown as usize).min(dma.data().len() / fb) };
                    crate::drawpipe::Counters::add(&crate::drawpipe::counters().acquired, avail as u64);
                    // batch mode (drawpipe.rs): the newest frame to the queue, the others with the buffer itself to the
                    // deposit workers, the next batch read into a buffer they gave back
                    let pipe = crate::drawpipe::pipe().filter(|x| after.is_none() && avail > 1 && x.accepting());
                    // the raw newest frame stands for the newest compressed one (same record)
                    let raw_frame = raw_frame.filter(|_| avail as u32 == count);
                    let mut frames: Vec<Frame> = if let Some(pipe) = pipe {
                        n += avail as u64;
                        let newest = match raw_frame {
                            Some(f) => Frame { stamp, seq: n, ..f },
                            None => Frame { stamp, ..decode(&dma.data()[(avail - 1) * fb..avail * fb], &b, &p, n) },
                        };
                        let next = pipe.take_buffer().and_then(|x| x.downcast::<oos_hal::dho::bus::DmaBuf>().ok()).map(|x| *x);
                        let next = next.or_else(|| (pool_made < POOL).then(|| {
                            pool_made += 1;
                            oos_hal::dho::bus::DmaBuf::new()
                        }));
                        match next {
                            Some(next) => {
                                let full = std::mem::replace(&mut dma, next);
                                let (bs, ps) = (b.clone(), p);
                                pipe.feed(Box::new(full), fb, avail - 1, stamp, Arc::new(move |c: &[u8], f: &mut Frame| decode_into(c, &bs, &ps, 0, f)));
                            }
                            None => crate::drawpipe::Counters::add(&crate::drawpipe::counters().dropped, avail as u64 - 1),
                        }
                        vec![newest]
                    } else {
                        let mut all: Vec<Frame> = dma
                            .data()
                            .chunks_exact(fb.max(1))
                            .take(avail)
                            .map(|chunk| {
                                n += 1;
                                Frame { stamp, hw_avg: after.is_some(), ..decode(chunk, &b, &p, n) }
                            })
                            .collect();
                        if let (Some(r), Some(last)) = (raw_frame, all.last_mut()) {
                            *last = Frame { stamp, seq: last.seq, ..r };
                        }
                        all
                    };
                    if after.is_some_and(|a| a.clear) {
                        if let Err(e) = seq::execute(&state.avg_clear_steps(), &mut bar, None, &mut c2h, |_| {}) {
                            status(format!("average clear: {}", e));
                        }
                    }
                    if let (Some(w), Some(f)) = (la_words, frames.last_mut()) {
                        f.la = Some(la_record(w, &b, &p));
                    }
                    let mut g = out.lock().unwrap();
                    g.queue.extend(frames);
                    let over = g.queue.len().saturating_sub(QUEUE_MAX);
                    g.queue.drain(..over);
                    crate::drawpipe::Counters::add(&crate::drawpipe::counters().dropped, over as u64);
                    if let Some(f) = g.queue.back() {
                        g.frame = f.clone();
                    }
                    drop(g);
                }
                // new settings while it waited for a trigger: at once to them
                Err(e) if e == seq::NEW_SETTINGS => {}
                Err(e) => {
                    status(format!("acquire: {}", e));
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
    });
    shared
}

/// A hardware segmented recording (docs/fork/specs/03-segmented-timestamps.md 3.2).
#[cfg(target_os = "linux")]
struct SegRun {
    /// `HwSeg::run` it belongs to
    run: u64,
    n: u32,
    recording: bool,
    /// segments recorded; the next one to read back
    count: u32,
    next: u32,
    /// read the frames into the queue too (the recording ended by itself, not by STOP)
    deliver: bool,
    tl: oos_hal::dho::seg::Timeline,
}

/// Ends a recording that is still in the FPGA before anything else writes it, and forgets the segments (their settings
/// are no longer the configured ones).
#[cfg(target_os = "linux")]
fn seg_abort(run: &mut Option<SegRun>, state: &mut oos_hal::dho::seq::State, bar: &mut oos_hal::dho::bus::Bar1, c2h: &mut std::fs::File, out: &HwShared) {
    let Some(r) = run.take() else { return };
    if r.recording {
        if let Err(e) = oos_hal::dho::seg::stop(state, bar, c2h) {
            eprintln!("oos-ui hw: segments stop: {}", e);
        }
    }
    let mut g = out.lock().unwrap();
    (g.seg.phase, g.seg.shown) = (crate::board::SegPhase::Idle, None);
    g.seg.times.clear();
}

/// One step of the hardware segmented recording: start on a new `HwSeg::run`, poll the progress every 10 ms (no
/// transfer), then stop and read back one batch per step (stamps by single-frame plays, frames into the queue), then
/// play single segments on request.
#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn seg_step(run: &mut Option<SegRun>, s: &Settings, p: &hz::Params, state: &mut oos_hal::dho::seq::State, bar: &mut oos_hal::dho::bus::Bar1, c2h: &mut std::fs::File, dma: &mut oos_hal::dho::bus::DmaBuf, out: &HwShared, n: &mut u64, stamp: u64) -> Result<(), String> {
    use crate::board::SegPhase;
    use oos_hal::dho::seg;
    let (cmd, running, want) = {
        let g = out.lock().unwrap();
        (g.seg.run, g.seg.running, g.seg.want)
    };
    if running && run.as_ref().map_or(0, |r| r.run) != cmd {
        if run.as_ref().is_some_and(|r| r.recording) {
            seg::stop(state, bar, c2h)?;
        }
        let total = seg::start(state, s, p, bar, c2h)?;
        *run = Some(SegRun { run: cmd, n: total, recording: true, count: 0, next: 0, deliver: true, tl: seg::Timeline::new(None) });
        let mut g = out.lock().unwrap();
        g.seg = crate::board::HwSeg { run: g.seg.run, running: g.seg.running, done_run: cmd, phase: SegPhase::Recording, total, ..Default::default() };
        return Ok(());
    }
    let Some(r) = run.as_mut() else {
        std::thread::sleep(std::time::Duration::from_millis(10));
        return Ok(());
    };
    if r.recording {
        let x = seg::status(bar, r.n);
        out.lock().unwrap().seg.recorded = x.recorded;
        if !x.complete(r.n) && running {
            std::thread::sleep(std::time::Duration::from_millis(10));
            return Ok(());
        }
        let tag = seg::first_trigger(bar);
        seg::stop(state, bar, c2h)?;
        (r.recording, r.count, r.next, r.deliver, r.tl) = (false, x.recorded, 0, running, seg::Timeline::new(tag));
        eprintln!("oos-ui hw: segments: {} of {} recorded, first-trigger stamp {:?}", x.recorded, r.n, tag);
        out.lock().unwrap().seg.phase = if r.count == 0 { SegPhase::Done } else { SegPhase::Reading };
        return Ok(());
    }
    let rs = seg_read_plan(s, p);
    let fb = oos_hal::dho::seq::frame_bytes(&rs, p);
    if r.next < r.count {
        r.deliver &= running;
        let cnt = rs.frames.min(r.count - r.next);
        if r.deliver && out.lock().unwrap().queue.len() + cnt as usize > crate::board::QUEUE_MAX {
            // the screen has not taken the last batch yet
            std::thread::sleep(std::time::Duration::from_millis(5));
            return Ok(());
        }
        let st = seg::stamps(state, &rs, p, bar, c2h, r.next, cnt)?;
        if r.next == 0 {
            // hardware check 5.3: header words 5..7 against the latch, frame by frame
            for (k, x) in st.iter().take(8).enumerate() {
                eprintln!("oos-ui hw: segment {} latch {:?} header {:?}", k, x.latch, x.header);
            }
        }
        let times: Vec<Option<f64>> = st.iter().map(|x| r.tl.push(*x)).collect();
        let mut frames = Vec::new();
        if r.deliver {
            seg::read(state, &rs, p, bar, c2h, dma, r.next, cnt)?;
            frames = dma
                .data()
                .chunks_exact(fb)
                .take(cnt as usize)
                .map(|chunk| {
                    *n += 1;
                    Frame { stamp, ..decode(chunk, &rs, p, *n) }
                })
                .collect();
        }
        r.next += cnt;
        let mut g = out.lock().unwrap();
        g.seg.times.extend(times);
        g.seg.read = r.next;
        if let Some(f) = frames.last() {
            g.frame = f.clone();
        }
        g.queue.extend(frames);
        if r.next >= r.count {
            g.seg.phase = SegPhase::Done;
        }
        return Ok(());
    }
    // done: single segments on request
    match want {
        Some(k) if k < r.count => {
            out.lock().unwrap().seg.want = None;
            let one = oos_hal::dho::seg::read_settings(&rs, 1);
            seg::read(state, &one, p, bar, c2h, dma, k, 1)?;
            *n += 1;
            let f = Frame { stamp, ..decode(&dma.data()[..fb.min(dma.data().len())], &one, p, *n) };
            out.lock().unwrap().seg.shown = Some((k, f));
        }
        Some(_) => out.lock().unwrap().seg.want = None,
        None => std::thread::sleep(std::time::Duration::from_millis(10)),
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::to_record;

    #[test]
    fn aux_out_reaches_the_settings() {
        let mut m = Model::default();
        assert_eq!(settings(&m, false).aux, auxout::AuxOut::TriggerOut);
        m.aux_out = AuxOut::Off;
        let s = settings(&m, false);
        assert_eq!(s.aux, auxout::AuxOut::Off);
        let w = oos_hal::dho::seq::State::handover(s.mask, None).configure(&s, None, None).unwrap().0;
        assert!(w.iter().any(|x| matches!(x, oos_hal::dho::seq::Step::Wr((0x1430, 2)))));
        m.aux_out = AuxOut::PassFail;
        assert_eq!(settings(&m, false).aux, auxout::AuxOut::PassFail(auxout::PassFail::DEFAULT));
    }

    #[test]
    fn mask_columns_of_the_board() {
        assert_eq!(crate::board::MASK_COLUMNS, oos_hal::dho::mask::COLUMNS);
    }

    /// Live compression and the mask test never run together: a mask (in software or in the FPGA, which needs
    /// `mask_on` too) keeps every frame raw, so the fast path the hardware mask keeps on draws uncompressed frames.
    #[test]
    fn a_mask_keeps_live_compression_off() {
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
        let mut s = settings(&m, false);
        live_compress(&m, &mut s);
        assert!(s.compress > 1, "compression applies at 10 us/div without a mask");
        m.mask_on = true;
        let mut s = settings(&m, false);
        live_compress(&m, &mut s);
        assert_eq!(s.compress, 1);
    }

    /// The zones in the hardware tables, and the "touched" logic modelled on the read-out words (the assumed sample
    /// scale): agrees with the software zone test on boxes on, off and across a square wave's edge.
    #[test]
    fn hardware_zone_tables_agree_with_the_software_zones() {
        use oos_hal::dho::{mask, zone};
        let (n, rate, pre, scale, offset) = (20_000u32, 100_000_000u64, 10_000u32, 1.0, -1.5);
        let dt = 1.0 / rate as f64;
        // 0 / 3 V square, period 4000 samples (40 us), rising edge at the trigger (sample 10,000)
        let v: Vec<f32> = (0..n as usize).map(|k| if (k / 2000) % 2 == 0 { 0.0 } else { 3.0 }).collect();
        let t0 = -(pre as f64) * dt;
        let touched = |poly: &[(f64, f64)]| -> bool {
            let t = zone::table(&zone::Zone { poly: poly.to_vec(), inside: true }, n, rate, pre, scale, offset);
            let per = zone::columns(n).1 as usize;
            let code = |x: f32| ((32768.0 + (x as f64 + offset) / scale * 7500.0).round() as u32 >> mask::SHIFT) as u8;
            let mut prev: Option<u8> = None;
            for (k, &x) in v.iter().enumerate() {
                let c = t[(k / per).min(t.len() - 1)];
                let s = code(x);
                if c.on && ((c.lo..=c.hi).contains(&s) || prev.is_some_and(|p| (p < c.lo && s > c.hi) || (p > c.hi && s < c.lo))) {
                    return true;
                }
                prev = Some(s);
            }
            false
        };
        // (box, touched): on the high level, on the low level, above the trace, across the rising edge at t = 0
        let cases = [
            (vec![(2e-6, 2.5), (8e-6, 3.5)], true),
            (vec![(-8e-6, -0.5), (-2e-6, 0.5)], true),
            (vec![(2e-6, 3.6), (8e-6, 4.0)], false),
            (vec![(-1e-6, 1.0), (1e-6, 2.0)], true),
            (vec![(5e-6, 1.0), (15e-6, 2.0)], false),
        ];
        for (pts, want) in cases {
            let sw = crate::zone::hits(&crate::zone::Zone { on: true, src: 0, inside: true, pts: pts.clone() }, &v, t0, dt);
            assert_eq!((sw, touched(&pts)), (want, want), "{:?}", pts);
        }
    }

    #[test]
    fn zone_hw_from_conf() {
        assert!(!zone_hw_from(""));
        assert!(zone_hw_from("zone.hw = on\n"));
    }

    #[test]
    fn mask_and_zone_merge() {
        use crate::board::{MaskWant, ZoneWant};
        let mw = MaskWant { ch: 0, lo: vec![-1.0; 1000], hi: vec![1.0; 1000], stop: false, clear: 0 };
        let zw = ZoneWant { ch: 1, zones: vec![(vec![(0.0, 0.0), (1e-6, 1.0)], true), (vec![(0.0, 2.0), (1e-6, 3.0)], false), (vec![], true)], clear: 4 };
        let pf = auxout::AuxOut::PassFail(auxout::PassFail::DEFAULT);
        let mut s = Settings { aux: pf, ..Settings::default() };
        hw_merge(&mut s, None, Some(&zw));
        let z = s.hwzone.as_ref().unwrap();
        assert_eq!((z.ch, z.zones.len(), z.zones[1].inside, z.clear), (1, 2, false, 4));
        assert_eq!(s.aux, auxout::AuxOut::PassFail(auxout::PassFail { source: auxout::Source::Zone, ..auxout::PassFail::DEFAULT }));
        // the mask test wins and keeps the mask source
        let mut s = Settings { aux: pf, ..Settings::default() };
        hw_merge(&mut s, Some(&mw), Some(&zw));
        assert!(s.hwmask.is_some() && s.hwzone.is_none() && s.aux == pf);
        let mut s = Settings::default();
        hw_merge(&mut s, None, None);
        assert!(s.hwmask.is_none() && s.hwzone.is_none() && s.aux == auxout::AuxOut::TriggerOut);
    }

    #[test]
    fn mask_hw_from_conf() {
        assert!(!mask_hw_from(""));
        assert!(mask_hw_from("mask.hw = on\n"));
        assert!(!mask_hw_from("mask.hw = 1\n"));
    }

    /// The software mask in the hardware table, and the comparator modelled on the read-out words (the assumed sample
    /// scale, `mask::code`): what the software passes the table passes, a runt fails both.
    #[test]
    fn hardware_table_agrees_with_the_software_mask() {
        use oos_hal::dho::mask;
        let (scale, offset) = (1.0, -1.5);
        let n = 20_000;
        // 5 periods of a 0..3 V square with a little noise, and a runt in period 3
        let wave = |runt: bool, seed: u32| -> Vec<f32> {
            (0..n)
                .map(|k| {
                    let ph = k % 4000;
                    let hi = if runt && k / 4000 == 3 { 1.2 } else { 3.0 };
                    let noise = (((k as u32).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 16) as f32 / 65536.0 - 0.5) * 0.04;
                    (if ph < 2000 { hi } else { 0.0 }) + noise
                })
                .collect()
        };
        let m = oos_dsp::mask::build(&wave(false, 1), 20, 0.2);
        let (lo, hi) = m.columns(mask::COLUMNS);
        let cols = mask::table(&lo, &hi, scale, offset);
        // the read-out word of a sample (inverse of `seq::decode_volts`), and the 8-bit comparator on it
        let fails = |v: &[f32]| -> bool {
            v.iter().enumerate().any(|(k, &x)| {
                let w = (32768.0 + (x as f64 + offset) / scale * 7500.0).round().clamp(0.0, 65535.0) as u32;
                let c = (w >> mask::SHIFT) as u8;
                let col = cols[k * mask::COLUMNS / n];
                (col.lo_en && c < col.lo) || (col.hi_en && c > col.hi)
            })
        };
        for seed in 2..8 {
            let v = wave(false, seed);
            assert!(m.test(&v).pass && !fails(&v), "seed {}", seed);
        }
        let v = wave(true, 9);
        assert!(!m.test(&v).pass && fails(&v));
    }

    #[test]
    fn dvm_hw_from_conf() {
        assert!(!dvm_hw_from(""));
        assert!(dvm_hw_from("dvm.hw = on # FPGA DVM\n"));
        assert!(!dvm_hw_from("dvm.hw = yes\n"));
    }

    #[test]
    fn dvm_steps() {
        let t0 = Some((1, 2, 7));
        // (want, mask, config, running, elapsed) -> (step, running after)
        let cases: [(Option<usize>, u32, u64, Option<DvmKey>, f64, DvmStep, Option<DvmKey>); 8] = [
            (None, 0b11, 7, None, 0.0, DvmStep::Idle, None),
            // CH2 of CH1+CH2: lane 2
            (Some(1), 0b11, 7, None, 0.0, DvmStep::Start(2), t0),
            (Some(1), 0b11, 7, t0, 0.1, DvmStep::Idle, t0),
            (Some(1), 0b11, 7, t0, 0.6, DvmStep::Read, t0),
            // a new configuration restarts it
            (Some(1), 0b11, 8, t0, 0.1, DvmStep::Start(2), Some((1, 2, 8))),
            // CH2 alone: lane 1
            (Some(1), 0b10, 7, t0, 0.6, DvmStep::Start(1), Some((1, 1, 7))),
            // the channel left the acquired set, or the DVM was switched off
            (Some(1), 0b01, 7, t0, 0.6, DvmStep::Stop, None),
            (None, 0b11, 7, t0, 0.6, DvmStep::Stop, None),
        ];
        for (want, mask, config, running, el, step, after) in cases {
            assert_eq!(dvm_step(want, mask, config, running, el), (step, after), "{:?} {:#b} {} {:?} {}", want, mask, config, running, el);
        }
    }

    #[test]
    fn prearm_from_env_or_conf() {
        assert!(!prearm_from(false, ""));
        assert!(prearm_from(true, ""));
        assert!(prearm_from(true, "acq.prearm = off\n"));
        assert!(prearm_from(false, "ntp = off\nacq.prearm = on   # arm ahead\n"));
        assert!(!prearm_from(false, "acq.prearm = yes\n"));
    }

    #[test]
    fn slow_modes_follow_auto_roll() {
        let mut m = Model::default();
        let at = |m: &mut Model, t: f64| {
            m.tscale = tscales().iter().position(|&x| (x as f64 - t).abs() < t * 1e-3).unwrap();
            slow_mode(m)
        };
        assert_eq!(at(&mut m, 0.1), Slow::Off);
        assert_eq!(at(&mut m, 0.2), Slow::Roll);
        m.auto_roll = false;
        assert_eq!(at(&mut m, 0.1), Slow::Off);
        assert_eq!(at(&mut m, 0.2), Slow::Scan { delay: 0.0, single: false });
        m.trig_mode = TrigMode::Single;
        assert_eq!(at(&mut m, 0.5), Slow::Scan { delay: 0.0, single: true });
        // averaging: no scan
        m.acq = AcqMode::Average;
        assert_eq!(at(&mut m, 0.5), Slow::Off);
        // roll and scan stream with the stock Auto depth
        m.acq = AcqMode::Normal;
        assert_eq!(settings(&m, false).depth, 0);
    }

    #[test]
    fn scan_picture_wraps_and_fills_the_screen() {
        let mut pic = Vec::new();
        scan_write(&mut pic, 10, 0, &[1.0; 4]);
        scan_write(&mut pic, 10, 8, &[2.0; 4]); // wraps: 8, 9, 0, 1
        assert_eq!(&pic[..4], &[2.0, 2.0, 1.0, 1.0]);
        assert!(pic[4].is_nan() && pic[9] == 2.0);
        let mut ch: [Vec<f32>; NCH] = [pic, Vec::new(), Vec::new(), Vec::new()];
        scan_gap(&mut ch, 9, 2);
        assert!(ch[0][9].is_nan() && ch[0][0].is_nan() && ch[0][1] == 2.0);
        // a scan frame of 1000 points spans the screen: point 0 left, point 999 right
        let m = Model::default();
        let v: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let f = Frame { seq: 1, dt: 1e-3, pre: 0, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: true, scan: true, partial: true, acq: (0.0, 0), stamp: 0, la: None, hw_avg: false };
        let r = to_record(&Arc::new(f), &m, 100, 1.0, -0.5);
        assert!(r.partial && !r.triggered);
        assert!(r.ch[0][0] < 10.0 && r.ch[0][99] > 980.0, "{} {}", r.ch[0][0], r.ch[0][99]);
    }

    #[test]
    fn scan_writes_the_la_where_the_analog_point_is() {
        // 25 000 analog points and 100 000 LA words over the screen
        assert_eq!(scan_la_pos(0, 25_000, 100_000), 0);
        assert_eq!(scan_la_pos(12_500, 25_000, 100_000), 50_000);
        assert_eq!(scan_la_pos(24_999, 25_000, 100_000), 99_996);
    }

    #[test]
    fn short_time_bases_read_beyond_the_screen() {
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap();
        // 100 ns/div, one channel: 1250 points per screen at 1.25 GSa/s -> 1408 at the same rate
        let s = settings(&m, false);
        assert_eq!(s.depth, 1408);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(1e-7), s.depth, 0);
        assert_eq!((p.rate, p.memsel, p.pre + p.post), (1_250_000_000, 1408, 1408));
        assert_eq!(p.pre, 704);
        // the smallest records get at least 64 more points
        assert_eq!(screen_margin(62), 128);
        // 1 ns/div: 12 points per screen, read 80 at the full rate (more than stock's smallest record, 62 at 5 ns)
        m.tscale = 0;
        assert_eq!(tscales()[0], 1e-9);
        let s = settings(&m, false);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(1e-9), s.depth, 0);
        assert_eq!((p.rate, p.memsel, p.pre), (1_250_000_000, 80, 40), "depth {}", s.depth);
    }

    // Break it catches: live compression where the model needs every sample, a factor that leaves fewer than two
    // pairs per column, compression above 1 M points (the deep-memory read), a batch that does not grow with it.
    #[test]
    fn live_compression_plan_by_time_base() {
        let plan = |tdiv: f64, two: bool| {
            let mut m = Model::default();
            m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-6).unwrap();
            m.ch[1].on = two;
            let mut s = settings(&m, false);
            let raw = s.clone();
            live_compress(&m, &mut s);
            let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
            let (b, r) = (batch_plan(&s, &p), batch_plan(&raw, &p));
            (b.compress, oos_hal::dho::seq::frame_bytes(&b, &p), b.frames, r.frames, p)
        };
        // every sample at 100 ns/div
        assert_eq!(plan(100e-9, false).0, 1);
        for (tdiv, two) in [(1e-6, false), (2e-6, false), (10e-6, false), (100e-6, false), (10e-6, true), (1e-3, false)] {
            let (k, fb, frames, raw_frames, p) = plan(tdiv, two);
            let per_column = p.rate as f64 * tdiv * 10.0 / crate::compress::SCREEN_COLUMNS;
            assert!(k >= crate::compress::K_MIN, "{} s/div: k {}", tdiv, k);
            // at least POINTS_PER_COLUMN points per column, less than twice that
            let points = per_column / k as f64;
            assert!((crate::compress::POINTS_PER_COLUMN..2.0 * crate::compress::POINTS_PER_COLUMN).contains(&points), "{} s/div: {} points per column", tdiv, points);
            assert_eq!(fb, 2 * p.ratio as usize * oos_hal::dho::seq::compressed_len(&Settings { compress: k, ..Settings::default() }, &p) as usize);
            assert!(frames >= raw_frames, "{} s/div: {} vs {} frames", tdiv, frames, raw_frames);
            assert!(raw_newest(&Settings { compress: k, ..Settings::default() }, &p));
        }
        // 10 us/div, one channel: 100 k points at 625 MSa/s (the Auto depth cap), 200 kB raw per frame become 13 kB,
        // 20 frames per batch become 299 (then the frame ring, 448, is the limit)
        let (k, fb, frames, raw_frames, p) = plan(10e-6, false);
        assert_eq!((p.rate, p.memsel, k, raw_frames), (625_000_000, 100_000, 15, 20), "{:?}", p);
        assert_eq!((fb, frames), (13_344, 299));
        // the model needs every sample
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
        m.trig_mode = TrigMode::Single;
        let mut s = settings(&m, false);
        live_compress(&m, &mut s);
        assert_eq!(s.compress, 1);
        // deep records keep the deep-memory read
        m.trig_mode = TrigMode::Auto;
        m.mdepth = mdepths().iter().position(|d| d.1 == 10_000_000).unwrap();
        let mut s = settings(&m, false);
        live_compress(&m, &mut s);
        assert_eq!(s.compress, 1);
    }

    // Break it catches: a raw deep read of the wrong part of the record (screen or trigger point outside it), one
    // beyond the budget, one where the batch was already raw or of the live compression, or in Peak.
    #[test]
    fn deep_raw_window_for_the_measurements() {
        let plan = |tdiv: f64, depth: u64, two: bool| {
            let mut m = Model::default();
            m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-6).unwrap();
            m.mdepth = mdepths().iter().position(|d| d.1 == depth).unwrap();
            m.ch[1].on = two;
            let s = settings(&m, false);
            let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, oos_hal::dho::seq::tdiv_fs(s.delay));
            let b = batch_plan(&s, &p);
            (b.clone(), p, deep_raw_settings(&b, &p))
        };
        // 10 M at 1 ms/div, CH1: the batch reads pairs, the measurement window raw: the whole record, 20 MB
        let (b, p, d) = plan(1e-3, 10_000_000, false);
        assert!(b.compress > 1 && b.window.is_none());
        let d = d.expect("raw window");
        assert_eq!(d.compress, 1);
        let (n, start) = oos_hal::dho::seq::window(&d, &p);
        let screen = (b.tdiv * 10.0 * p.rate as f64) as u32;
        assert!(start <= (p.memsel / 2).saturating_sub(screen / 2) && start + n >= (p.memsel / 2 + screen / 2).min(p.memsel), "{} + {} of {}", start, n, p.memsel);
        assert!(start < p.pre && start + n > p.pre);
        let bytes = oos_hal::dho::seq::frame_bytes(&d, &p);
        assert!(bytes <= crate::deepmeas::MAX_BYTES, "{} bytes", bytes);
        // the decoded frame is raw and knows its trigger point
        let f = decode(&vec![0u8; bytes], &d, &p, 1);
        assert!(!f.envelope && f.ch[0].len() == n as usize && f.pre == (p.pre - start) as usize);
        // 2 channels at 10 M: 40 MB, beyond the budget
        assert!(plan(1e-3, 10_000_000, true).2.is_none());
        // 25 M at 2 ms/div: 50 MB
        assert!(plan(2e-3, 25_000_000, false).2.is_none());
        // a deep record at a short time base is read raw already (screen window), 1 M is the live path
        assert!(plan(1e-7, 10_000_000, false).2.is_none());
        assert!(plan(1e-3, 1_000_000, false).2.is_none());
        // Peak: the memory holds peak pairs
        let (b, p, _) = plan(1e-3, 10_000_000, false);
        assert!(deep_raw_settings(&Settings { mode: oos_hal::dho::acq::Mode::Peak, ..b }, &p).is_none());
    }

    #[test]
    fn a_deep_record_at_a_short_time_base_reads_the_screen_window() {
        // 10 M at 100 ns/div, CH1: 1.25 GSa/s, the screen is 1250 of the 10 M points
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-7).abs() < 1e-12).unwrap();
        m.mdepth = mdepths().iter().position(|d| d.1 == 10_000_000).unwrap();
        let s = settings(&m, false);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, oos_hal::dho::seq::tdiv_fs(s.delay));
        let b = batch_plan(&s, &p);
        assert_eq!(b.compress, 1, "every sample, not ~80 min/max points for the screen");
        let (n, start) = oos_hal::dho::seq::window(&b, &p);
        assert!(n < 20_000 && n % 8 == 0, "{}", n);
        // the screen and the trigger point are inside, and the frame's trigger point counts from the window
        let screen = (s.tdiv * 10.0 * p.rate as f64) as u32;
        assert!(start <= p.memsel / 2 - screen / 2 && start + n >= p.memsel / 2 + screen / 2);
        assert!(start < p.pre && start + n > p.pre);
        let bytes = vec![0u8; seq_bytes(&b, &p)];
        let f = decode(&bytes, &b, &p, 1);
        assert_eq!(f.ch[0].len(), n as usize);
        assert_eq!(f.pre, (p.pre - start) as usize);
        // a long time base keeps the compressed whole record
        m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        let s = settings(&m, false);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
        let b = batch_plan(&s, &p);
        assert!(b.compress > 1 && b.window.is_none());
    }

    fn seq_bytes(b: &Settings, p: &hz::Params) -> usize {
        oos_hal::dho::seq::frame_bytes(b, p)
    }

    #[test]
    fn depth_stays_within_the_limit_of_the_channels_on() {
        let mut m = Model::default();
        m.mdepth = mdepths().iter().position(|d| d.1 == 125_000_000).unwrap();
        m.tscale = tscales().iter().position(|&t| t >= 1e-3).unwrap();
        for c in 0..NCH {
            m.ch[c].on = c == 0;
        }
        assert_eq!(settings(&m, false).depth, 125_000_000);
        for c in 0..NCH {
            m.ch[c].on = true;
        }
        assert_eq!(settings(&m, false).depth, m.max_mem() as i128);
        assert_eq!(m.max_mem(), 31_250_000);
    }

    // Break it catches: hardware segments requested in roll or with the LA, a read-back batch larger than the UI queue
    // or without the header that carries the stamp.
    #[test]
    fn hardware_segments_only_where_they_record() {
        let mut m = Model::default();
        m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap();
        (m.segments, m.seg_hw, m.seg_ivl) = (500, true, 0.0);
        let s = settings(&m, false);
        assert_eq!(s.segments, Some(Segments { n: 500, interval: 0.0 }));
        assert_eq!(seg_max(&m), Some(61_440));
        m.seg_hw = false;
        assert_eq!(settings(&m, false).segments, None);
        m.seg_hw = true;
        m.la.on = true;
        assert_eq!((settings(&m, false).segments, seg_max(&m)), (None, None));
        m.la.on = false;
        m.tscale = tscales().iter().position(|&x| x >= 0.2).unwrap();
        assert_eq!(settings(&m, false).segments, None);
        // read-back: header on, at most half the queue, ~4 MB
        for t in [1e-7, 1e-5, 1e-3] {
            m.tscale = tscales().iter().position(|&x| (x as f64 - t).abs() < t * 1e-3).unwrap();
            let s = settings(&m, false);
            let p = oos_hal::dho::seq::State::handover(s.mask, None).configure(&s, None, None).unwrap().1;
            let r = seg_read_plan(&s, &p);
            let fb = oos_hal::dho::seq::frame_bytes(&r, &p);
            assert!(r.header && r.frames >= 1 && r.frames as usize <= crate::board::QUEUE_MAX / 2);
            assert!(r.frames == 1 || r.frames as usize * fb <= 4_000_000, "{} x {}", r.frames, fb);
        }
    }

    #[test]
    fn settings_from_the_model() {
        let mut m = Model::default();
        m.ch[1].on = true;
        let s = settings(&m, false);
        assert_eq!(s.mask, 3);
        assert_eq!(s.trig_src, 1);
        // 200 us/div with 2 channels: Auto would be 1.25 M points -> live depth 100 k
        assert_eq!(s.depth, LIVE_DEPTH);
        assert!(s.window.is_none());
        // edge: the level itself, even below level 2; runt: upper level and lower level
        m.trig_level = 0.2;
        assert_eq!(settings(&m, false).level, 0.2f32 as f64);
        m.trig_type = TrigType::Runt;
        m.trig_level2 = 1.8;
        let s = settings(&m, false);
        assert_eq!(s.level, 1.8f32 as f64);
        assert!(matches!(s.trigger, Trig::Runt { lo, .. } if (lo - 0.2).abs() < 1e-6));
    }

    // Break it catches: a live batch whose frames the FPGA pads in the stream (frame bytes not a multiple of 8), so
    // every read moves the whole batch once more in `seq::unpad`. All time bases, depths and channel sets of the UI
    // that read through the batch path (not roll or scan); with and without the LA.
    #[test]
    fn live_batches_need_no_stream_padding() {
        let mut m = Model::default();
        let (mut bad, mut checked, mut la_seen) = (Vec::new(), 0, false);
        for la in [false, true] {
            m.la.on = la;
            for chans in 1u32..16 {
                for c in 0..NCH {
                    m.ch[c].on = chans & (1 << c) != 0;
                }
                for t in 0..tscales().len() {
                    m.tscale = t;
                    if slow_mode(&m) != Slow::Off {
                        continue;
                    }
                    for d in 0..mdepths().len() {
                        m.mdepth = d;
                        let s = settings(&m, false);
                        let Ok((_, p)) = oos_hal::dho::seq::State::handover(s.mask, None).configure(&s, None, None) else { continue };
                        let b = batch_plan(&s, &p);
                        let fb = oos_hal::dho::seq::frame_bytes(&b, &p);
                        checked += 1;
                        la_seen |= s.la.is_some();
                        if fb % 8 != 0 {
                            bad.push((la, chans, tscales()[t], mdepths()[d].0, fb));
                        }
                    }
                }
            }
        }
        assert!(checked > 1000 && la_seen, "{} set-ups checked, LA among them: {}", checked, la_seen);
        assert!(bad.is_empty(), "{} padded set-ups, e.g. {:?}", bad.len(), &bad[..bad.len().min(8)]);
    }

    #[test]
    fn decode_and_align_a_square() {
        // a 2-channel record: CH1 square 0/2.5 V (1 kHz), CH2 constant; 100 k points at 625 MSa/s -> fake 1 MSa/s here
        let m = Model::default();
        let p = hz::Params { ratio: 2, rate: 1_000_000, memsel: 4000, dot_fs: 1_000_000_000, seg: 0, max_frames: 0, pre: 2000, post: 2000 };
        let s = Settings { mask: 3, ch: [Chan { scale: 1.0, bw: 0, ..Settings::default().ch[0] }; 4], ..Settings::default() };
        let mut bytes = Vec::new();
        for i in 0..4000usize {
            // rising edge of CH1 at sample 2003 (3 samples after the hardware trigger point)
            let v1 = if (i + 1000 - 3) % 1000 < 500 { 2.5 } else { 0.0 };
            for v in [v1, 0.5] {
                let w = (32768.0 + v * 7500.0) as u16;
                bytes.extend(w.to_le_bytes());
            }
        }
        let f = decode(&bytes, &s, &p, 1);
        assert_eq!(f.ch[0].len(), 4000);
        assert!((f.ch[1][10] - 0.5).abs() < 1e-3);
        let window = 1e-3;
        let r = to_record(&Arc::new(f), &m, 1000, window, -window / 2.0);
        assert!(r.triggered);
        // the software edge (level 1.5 V) sits at the centre of the screen window
        assert!(r.ch[0][495] < 0.5 && r.ch[0][505] > 2.0, "{} {}", r.ch[0][495], r.ch[0][505]);
    }
}
