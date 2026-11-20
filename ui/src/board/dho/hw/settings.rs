// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The hardware settings for the UI model (`settings`): roll and scan switching, Auto depth, live compression,
//! averaging, hardware segments and the UI trigger as a TPU trigger. A new field of `Settings` that follows the
//! model is filled here.

use super::decode::bw_code;
use super::plan::batch_plan;
use super::serial::{hw_serial, serial_lines_acquired};
use super::units::header_probe;
use crate::board::Slow;
use crate::model::*;
use oos_hal::dho::seq::{Chan, Couple, Segments, Settings, Trig};
use oos_hal::dho::{auxout, hz, roll, tpu, uarttrig};

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

/// SINGLE ends with the first frame after the press (`Settings::single`): one frame per batch and no batch after it
/// until the next press. Not when the UI may refuse that frame and wait for another: a protocol trigger (the decoder
/// decides) or a zone trigger.
fn single_takes_first(m: &Model) -> bool {
    m.trig_mode == TrigMode::Single && m.trig_type != TrigType::Bus && !m.zones.iter().any(|z| z.usable())
}

/// Whether the thread acquires another batch for SINGLE: not once a batch brought frames under this press's `stamp`
/// (`done`). The UI's STOP reached the thread only a few UI steps later, and each batch in between counted:
/// `acquired` +10 at 1 us/div, +3 at 1 ms/div per press (unit 2026-11-13).
pub(super) fn single_wants_more(b: &Settings, done: Option<u64>, stamp: u64) -> bool {
    !b.single || done != Some(stamp)
}

/// Live record length for Auto depth (keeps the read short enough for a fluid display).
pub(super) const LIVE_DEPTH: i128 = 100_000;

/// Auto depth at short time bases: the stock record is exactly one screen at the full rate, so after the software
/// trigger moves the record by a few samples the edge of the screen has no data (acquisition-start.md, C2H read rules
/// 3). An eighth more memory (at least 64 points, a multiple of 8) at the same rate: `hz::prep` keeps the full rate for
/// a depth above one screen and centres the record on the trigger.
pub(super) fn screen_margin(memsel: i128) -> i128 {
    (memsel + (memsel / 8).max(64) + 7) / 8 * 8
}

/// Auto depth with a trigger delay: the record is centred on the delayed screen, but the trigger has to stay inside it
/// (`seq::State::configure` clamps the delay to half a record, trigger.md 4). A delay beyond half of the `base` record
/// then showed only the part of the screen up to the record's end (10 us/div, 200 us delay: 20 us of 100, unit
/// 2026-11-13). The record then gets the depth that reaches from the trigger (a division inside) past the delay at the
/// full rate; the depth limit of the channels still applies afterwards.
fn delay_depth(mask: u32, tdiv: f64, delay: f64, base: i128) -> i128 {
    let p = hz::prep(mask, oos_hal::dho::seq::tdiv_fs(tdiv), base, 0);
    let span = p.memsel as f64 / p.rate.max(1) as f64;
    if !delay.is_finite() || 2.0 * delay.abs() <= span {
        return base;
    }
    let full = (oos_hal::dho::clock::NOMINAL_HZ / hz::ratio(mask) as i128) as f64;
    // at most the deepest record of the board; `settings` then limits it to the channels' memory
    let want = ((2.0 * delay.abs() + 2.0 * tdiv) * full).ceil().min(125e6) as i128;
    ((want + 7) / 8 * 8).max(base)
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
            let base = if auto.memsel as i128 > LIVE_DEPTH { LIVE_DEPTH } else { screen_margin(auto.memsel as i128) };
            delay_depth(mask, tdiv, m.delay as f64, base)
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
    let df = roll::depth_for(oos_hal::dho::seq::tdiv_fs(tdiv));
    let depth = if slow_mode(m) == Slow::Roll && df > 0 {
        if depth == 0 {
            df
        } else {
            depth.min(df)
        }
    } else {
        depth
    };
    let ch = std::array::from_fn(|c| Chan {
        scale: m.ch[c].volts_per_div() as f64,
        offset: m.ch[c].offset as f64,
        bw: bw_code(m.ch[c].bw),
        probe: m.ch[c].probe as f64,
        bias: m.ch[c].bias as f64,
        skew: m.ch[c].skew as f64,
        couple: match m.ch[c].coupling {
            // a board without it never gets it (`Model::fit_platform`)
            Coupling::Dc | Coupling::Dc50 => Couple::Dc,
            Coupling::Ac => Couple::Ac,
            Coupling::Gnd => Couple::Gnd,
        },
        invert: m.ch[c].invert,
    });
    // protocol trigger: hardware edge on the line where frames start (the protocol's start role: SPI chip select, I2S
    // word select, else the data line)
    let mut src = if m.trig_type == TrigType::Bus {
        [Some(m.dec_src), Some(m.dec_src2), m.dec_cs, m.dec_src4][m.dec_proto.start_role().min(3)].unwrap_or(m.dec_src)
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
            // frames start with a falling edge (start bit, SOF, break, I2C start, CS); I2S and the modules that say so
            // (1553, ARINC 429): either edge
            (TrigType::Bus, _) if m.dec_proto.module().is_some() => {
                match m.dec_proto.module().map(|p| p.start_edge()) {
                    Some(Slope::Rise) => tpu::Slope::Rising,
                    Some(Slope::Either) => tpu::Slope::Either,
                    _ => tpu::Slope::Falling,
                }
            }
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
        single: single_takes_first(m),
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
        segments: (m.seg_hw && m.segments > 0 && hw_segments_ok(m))
            .then_some(Segments { n: m.segments as u32, interval: m.seg_ivl as f64 }),
        aux: aux_out(m.aux_out),
        // the mask test and zone trigger come from the UI (`Shared::mask_want`, `zone_want`), merged in the acquisition
        // thread (`hw_merge`)
        hwmask: None,
        hwzone: None,
    };
    // the hardware UART fires inside the frame, the software trigger point is the end of its stop bit: the record is
    // centred on the software point (docs/fork/hwuart.md)
    if let (Trig::Uart(u), Slow::Off) = (&s.trigger, slow_mode(m)) {
        s.delay += uarttrig::sw_offset_s(u);
    }
    // the SPI and I2C receivers need every line they use among the acquired channels; else the edge trigger
    if !serial_lines_acquired(&s.trigger, s.mask) {
        s.trigger = Trig::Edge;
    }
    if let Some(count) = hw_avg_count(m, &s) {
        s.mode = oos_hal::dho::acq::Mode::Average { count };
    }
    if crate::board::policy::compress::enabled() {
        live_compress(m, &mut s);
    }
    s
}

/// The live compression for the model (compress.rs): the factor of `policy::compress::k_for` for the record of `s` where the
/// model allows it (not in roll or scan), else none.
pub(super) fn live_compress(m: &Model, s: &mut Settings) {
    if slow_mode(m) != Slow::Off
        || !crate::board::policy::compress::allowed(m)
        || s.la.is_some()
        || s.segments.is_some()
        || !matches!(s.mode, oos_hal::dho::acq::Mode::Normal)
    {
        return;
    }
    let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(s.tdiv), s.depth, 0);
    s.compress = crate::board::policy::compress::k_for(p.rate, s.tdiv, p.memsel);
}

/// A reader of every frame in the UI that the model does not show (protocol watch, XY: `Shared::needs_raw`) switches
/// the live compression off.
pub(super) fn raw_for_ui(s: &mut Settings, needs_raw: bool) {
    if needs_raw {
        s.compress = 1;
    }
}

/// The batch was read with the live compression (`live_compress`): its newest frame is read again raw
/// (`seq::read_raw_again`) for the record the UI keeps.
pub(super) fn raw_newest(b: &Settings, p: &hz::Params) -> bool {
    b.compress > 1 && p.memsel <= crate::board::policy::compress::MAX_DEPTH
}

/// The AUX OUT set-up of the menu choice. Pass/fail takes the default pulse of `auxout::PassFail` (docs/fork/auxout.md).
pub(super) fn aux_out(a: AuxOut) -> auxout::AuxOut {
    match a {
        AuxOut::TrigOut => auxout::AuxOut::TriggerOut,
        AuxOut::Off => auxout::AuxOut::Off,
        AuxOut::PassFail => auxout::AuxOut::PassFail(auxout::PassFail::DEFAULT),
    }
}

/// The count of a hardware average for the model and its settings `s`, None for the software average
///: Average mode with `Model::avg_hw`, a power-of-two count, ordinary triggered
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

/// How the FPGA average is read: running (default), or with OOS_HWAVG=hold a cumulative average of the
/// count that is shown when complete and then cleared. A hardware test on the unit decides which one is right.
pub(super) fn avg_kind() -> oos_hal::dho::avg::Kind {
    static K: std::sync::OnceLock<oos_hal::dho::avg::Kind> = std::sync::OnceLock::new();
    *K.get_or_init(|| {
        if std::env::var("OOS_HWAVG").is_ok_and(|v| v == "hold") {
            oos_hal::dho::avg::Kind::Hold
        } else {
            oos_hal::dho::avg::Kind::Running
        }
    })
}

/// Hardware segments record ordinary triggered frames: not in roll or scan, not with the LA (its record layout and
/// read are not worked out for them).
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
    m.dec_la[m.dec_proto.start_role().min(3)].or(m.dec_la[0])
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
    let la = if m.la_on() {
        m.pat_la.map(|b| match b {
            Bit::H => Pat::H,
            Bit::L => Pat::L,
            _ => Pat::X,
        })
    } else {
        [Pat::X; 16]
    };
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
        TrigType::Delay => {
            Trig::Delay { srcb: m.trig_src2 as u32 + 1, eb: edge2, when, lower, upper, levelb: m.trig_level2 as f64 }
        }
        TrigType::Pattern => Trig::Pattern { pats, la, levels },
        TrigType::Duration => Trig::Duration { pats, la, when, lower, upper, levels },
        TrigType::SetupHold => Trig::SetupHold {
            clk: m.trig_src2 as u32 + 1,
            clk_edge: edge2,
            setup: m.setup as f64,
            hold: m.hold as f64,
            levelc: m.trig_level2 as f64,
        },
        TrigType::NthEdge => Trig::Nth { count: m.nth_n, idle: m.nth_idle as f64 },
        // protocol trigger: the TPU's UART, SPI or I2C receiver where it can (`trig.serial_hw = on`, `hw_serial`); else the TPU fires
        // on edges of the decoder's line (see `settings`) and the software picks the matching frame out of the record
        TrigType::Bus => hw_serial(m).unwrap_or(Trig::Edge),
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
