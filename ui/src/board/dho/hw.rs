// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Hardware source: acquisitions through `oos_hal::dho::seq` (configure on change, arm, wait, read the whole record),
//! decoded into volts per channel. The UI aligns each frame with the software trigger engine (all 12 trigger types on
//! real data; the hardware itself triggers with an edge on the source at the level, in AUTO or NORMAL).

use crate::board::{Frame, Slow};
#[cfg(target_os = "linux")]
use crate::board::{HwShared, Shared, QUEUE_MAX};
use crate::model::*;
use oos_hal::dho::seq::{Chan, Couple, Settings, Trig};
use oos_hal::dho::{afe, hz, tpu};
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
    Settings {
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
        play4: true,
        blocks: 7,
        header: header_probe(),
        noise_reject: m.trig_nrej,
        frames: 1,
        compress: 1,
        trigger: hw_trigger(m),
        window: None,
        la,
    }
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
    let (hdr, bytes) = if s.header && bytes.len() >= 16 { bytes.split_at(16) } else { (&bytes[..0], bytes) };
    let w = oos_hal::dho::seq::deinterleave(bytes, s.mask);
    let ch = std::array::from_fn(|c| if w[c].is_empty() { Vec::new() } else { oos_hal::dho::seq::volts(&w[c], s.ch[c].scale, s.ch[c].offset) });
    // with FPGA compression every point stands for k samples (min/max pairs, acquisition-start.md)
    let k = s.compress.max(1) as usize;
    if !hdr.is_empty() {
        let tc = (s.trig_src.clamp(1, 4) - 1) as usize;
        let v: &Vec<f32> = &ch[tc];
        log_header(hdr, v, s, p.pre as usize / k, k);
    }
    Frame { seq, dt: k as f64 / p.rate as f64, pre: p.pre as usize / k, ch, roll: false, envelope: k > 1, scan: false, partial: false, acq: (p.rate as f64, p.memsel as usize), stamp: 0, la: None }
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

/// Batch settings for a configured acquisition: frames per batch (at most ~4 MB per read, the frame ring, 1000 /
/// ratio) and, for records above 1 M points, FPGA compression to ~625 k points per frame (the
/// stock live length at 2.5 M / k = 4).
pub fn batch_plan(s: &Settings, p: &hz::Params) -> Settings {
    let mut b = s.clone();
    b.compress = if p.memsel > 1_000_000 { p.memsel.div_ceil(625_000) } else { 1 };
    let fb = oos_hal::dho::seq::frame_bytes(&b, p).max(1);
    b.frames = ((4_000_000 / fb) as u32).min(p.max_frames).min(1000 / p.ratio.max(1)).max(1);
    b
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
        loop {
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
            let (want, want_mode) = {
                let g = out.lock().unwrap();
                (g.want.clone(), g.slow)
            };
            let Some(mut want) = want else {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            };
            want.afe = program_afe;
            if let Some(l) = want.la.as_mut() {
                l.cal = la_cal;
            }
            if current.as_ref().map_or(true, |c| c.0 != want) || mode != want_mode {
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
                    }
                    Err(e) => {
                        status(format!("settings: {}", e));
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        continue;
                    }
                }
            }
            let (s, p) = current.clone().unwrap();
            // :TFORce. A pulse of the force bit on the shadows now in force, then back. Not part of the set-up, so it
            // does not reconfigure.
            if std::mem::take(&mut out.lock().unwrap().force) {
                bar.apply(&oos_hal::dho::tpu::force(&state.ts));
            }
            if mode != Slow::Off {
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
                        Frame { seq: n, dt, pre: 0, ch, roll: false, envelope: true, scan: true, partial: !done, acq: (0.0, 0), stamp, la }
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
                        Frame { seq: n, dt, pre, ch: ring.clone().map(|r| r.into_iter().collect()), roll: true, envelope: true, scan: false, partial: len < keep, acq: (0.0, 0), stamp, la }
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
            let b = batch_plan(&s, &p);
            // one batch: arm, let the ring fill for up to 25 ms after the first frame, one play, one read
            match seq::acquire_batch(&mut state, &b, &p, &mut bar, &mut c2h, 25, |_| {}) {
                Ok((bytes, count)) => {
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
                    let fb = seq::frame_bytes(&b, &p);
                    // decode before taking the lock: the UI must not wait for 100 k-point conversions
                    let mut frames: Vec<Frame> = bytes
                        .chunks_exact(fb)
                        .take(count as usize)
                        .map(|chunk| {
                            n += 1;
                            Frame { stamp, ..decode(chunk, &b, &p, n) }
                        })
                        .collect();
                    if let (Some(w), Some(f)) = (la_words, frames.last_mut()) {
                        f.la = Some(la_record(w, &b, &p));
                    }
                    let mut g = out.lock().unwrap();
                    g.queue.extend(frames);
                    while g.queue.len() > QUEUE_MAX {
                        g.queue.pop_front();
                    }
                    if let Some(f) = g.queue.back() {
                        g.frame = f.clone();
                    }
                    drop(g);
                }
                Err(e) => {
                    status(format!("acquire: {}", e));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::to_record;

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
        let f = Frame { seq: 1, dt: 1e-3, pre: 0, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: true, scan: true, partial: true, acq: (0.0, 0), stamp: 0, la: None };
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
        m.tscale = tscales().iter().position(|&t| (t - 1e-9).abs() < 1e-12).unwrap();
        assert!((tscales()[m.tscale] - 1e-9).abs() < 1e-12);
        let s = settings(&m, false);
        let p = hz::prep(s.mask, oos_hal::dho::seq::tdiv_fs(1e-9), s.depth, 0);
        assert_eq!((p.rate, p.memsel, p.pre), (1_250_000_000, 80, 40), "depth {}", s.depth);
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
