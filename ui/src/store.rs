// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Storage: setup files (`key = value` text of the Model), waveform CSV, PNG screenshots (own encoder: one fixed-Huffman
//! deflate block with matches against the pixel to the left and above, CRC32, Adler32), file names in the storage directory.

use crate::gfx::Canvas;
use crate::math::{MATH_OPS, WINDOWS};
use crate::model::*;
use crate::wave::Record;
use oos_dsp::decode::Parity;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

/// Default storage directory on the unit (created when missing).
pub const DEFAULT_DIR: &str = "/tmp/oos";
/// Folder on a USB stick (`/bin/oos-usb` mounts it at /mnt/usb).
pub const USB_DIR: &str = "/mnt/usb/OOS";

/// A stick is mounted, by the text of /proc/mounts.
pub fn usb_mounted(mounts: &str) -> bool {
    mounts.lines().any(|l| l.split(' ').nth(1) == Some("/mnt/usb"))
}

/// First free `<prefix>-NNN.<ext>` in `dir` (001 .. 999, then the last one is overwritten).
pub fn next_name(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{}-999.{}", prefix, ext));
    for i in 1..1000 {
        let c = dir.join(format!("{}-{:03}.{}", prefix, i, ext));
        // a link counts as taken, also one that points nowhere
        if c.symlink_metadata().is_err() {
            p = c;
            break;
        }
    }
    p
}

/// `<prefix>-<stamp>.<ext>` (`<prefix>-<stamp>-2.<ext>` and on when taken) with a time stamp, else `next_name`.
pub fn next_name_at(dir: &Path, prefix: &str, ext: &str, stamp: Option<&str>) -> PathBuf {
    let Some(st) = stamp else { return next_name(dir, prefix, ext) };
    for i in 1..100 {
        let n = if i == 1 { format!("{}-{}.{}", prefix, st, ext) } else { format!("{}-{}-{}.{}", prefix, st, i, ext) };
        if dir.join(&n).symlink_metadata().is_err() {
            return dir.join(n);
        }
    }
    next_name(dir, prefix, ext)
}

/// File name of a path for toasts ("shot-001.png").
pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A stick is foreign data and this program runs as root: a link or a device node on it, under a name a save would use,
/// could lead straight to the card. So only a regular file or a new one, and on the stick no link on the way there.
pub fn check_target(path: &Path) -> std::io::Result<()> {
    let refuse = |what: &str| Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{}: {}", path.display(), what)));
    if path.starts_with("/mnt/usb") {
        for a in path.ancestors().skip(1).take_while(|a| a.starts_with("/mnt/usb") && *a != Path::new("/mnt/usb")) {
            if a.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                return refuse("a link on the way there");
            }
        }
    }
    match path.symlink_metadata() {
        Ok(m) if !m.is_file() => refuse("not a regular file"),
        _ => Ok(()),
    }
}

/// Writes `data` to `path`, creating the directory first.
pub fn write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    check_target(path)?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, data)
}

// ---------------------------------------------------------------------------------------------- setup files

const COUPLINGS: [Coupling; 4] = [Coupling::Dc, Coupling::Ac, Coupling::Gnd, Coupling::Dc50];
const SLOPES: [Slope; 3] = [Slope::Rise, Slope::Fall, Slope::Either];
const TRIG_MODES: [TrigMode; 3] = [TrigMode::Auto, TrigMode::Normal, TrigMode::Single];
const ACQ_MODES: [AcqMode; 4] = [AcqMode::Normal, AcqMode::Peak, AcqMode::Average, AcqMode::HiRes];
const WHENS: [WhenKind; 5] = [WhenKind::Off, WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out];
const WIN_MODES: [WinMode; 3] = [WinMode::Enter, WinMode::Exit, WinMode::Inside];
const VSYNCS: [VSync; 5] = [VSync::AllLines, VSync::Line, VSync::Odd, VSync::Even, VSync::AllFields];
const HEXPANDS: [HExpand; 4] = [HExpand::Center, HExpand::Left, HExpand::Right, HExpand::Trigger];
const CURSORS: [CursorMode; 5] = [CursorMode::Off, CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy];
const GRIDS: [Grid; 3] = [Grid::Full, Grid::Light, Grid::None];
const VIDEO_STDS: [VideoStd; 2] = [VideoStd::Pal, VideoStd::Ntsc];
const BITS: [Bit; 5] = [Bit::H, Bit::L, Bit::X, Bit::R, Bit::F];
const POLS: [Pol; 2] = [Pol::Pos, Pol::Neg];
const PARITIES: [Parity; 3] = [Parity::None, Parity::Even, Parity::Odd];

fn name<T: Debug>(x: T) -> String {
    format!("{:?}", x)
}

fn list<T: ToString>(v: &[T]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")
}

/// The setup as `key = value` lines: every Model field except the acquisition state (run / trigger status).
/// Enums by variant name, indices into the value ladders as numbers, floats in Rust's shortest exact form.
/// The keys of one decoder.
fn dec_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    kv("dec_on", m.dec_on.to_string());
    kv("dec_src", m.dec_src.to_string());
    kv("dec_baud", m.dec_baud.to_string());
    kv("dec_canrate", m.dec_canrate.to_string());
    kv("dec_canh", m.dec_canh.to_string());
    kv("dec_fdrate", m.dec_fdrate.to_string());
    kv("dec_arate", m.dec_arate.to_string());
    kv("dec_tick", m.dec_tick.to_string());
    kv("dec_nibbles", m.dec_nibbles.to_string());
    kv("dec_uspeed", m.dec_uspeed.to_string());
    kv("dec_src4", m.dec_src4.map_or("none".into(), |c| (c + 1).to_string()));
    kv("dec_diff", m.dec_diff.map_or("none".into(), |c| (c + 1).to_string()));
    kv("trig_bus", m.trig_bus.name().to_string());
    kv("trig_bus_val", m.trig_bus_val.to_string());
    kv("dec_thr", m.dec_thr.to_string());
    kv("dec_ascii", m.dec_ascii.to_string());
    kv("dec_proto", m.dec_proto.key());
    kv("dec_opt", list(&m.dec_opt));
    kv("dec_src2", m.dec_src2.to_string());
    kv("dec_cs", m.dec_cs.map_or("none".into(), |c| c.to_string()));
    kv("dec_mode", m.dec_mode.to_string());
    kv("dec_bits", m.dec_bits.to_string());
    kv("dec_parity", name(m.dec_parity));
}

pub fn setup_text(m: &Model) -> String {
    let mut s = String::from("# OOS setup v1: key = value (oos-ui)\n");
    let mut kv = |k: &str, v: String| {
        s.push_str(k);
        s.push_str(" = ");
        s.push_str(&v);
        s.push('\n');
    };
    for (c, ch) in m.ch.iter().enumerate() {
        let p = |k: &str| format!("ch{}.{}", c + 1, k);
        kv(&p("on"), ch.on.to_string());
        kv(&p("vdiv"), VSCALES[ch.vscale].to_string());
        if ch.vfine != 1.0 {
            kv(&p("vfine"), ch.vfine.to_string());
        }
        if ch.unit != AmpUnit::V {
            kv(&p("unit"), ch.unit.name().to_string());
        }
        if ch.bias != 0.0 {
            kv(&p("bias"), ch.bias.to_string());
        }
        if ch.skew != 0.0 {
            kv(&p("skew"), ch.skew.to_string());
        }
        kv(&p("offset"), ch.offset.to_string());
        kv(&p("coupling"), name(ch.coupling));
        kv(&p("bw"), BWS[ch.bw].0.to_string());
        kv(&p("probe"), ch.probe.to_string());
        kv(&p("invert"), ch.invert.to_string());
    }
    kv("active", m.active.to_string());
    kv("tdiv", tscales()[m.tscale].to_string());
    kv("delay", m.delay.to_string());
    kv("trig_src", m.trig_src.to_string());
    kv("trig_la", m.trig_la.map_or("none".to_string(), |n| n.to_string()));
    kv("pat_la", m.pat_la.iter().map(|&b| match b { Bit::H => 'H', Bit::L => 'L', _ => 'X' }).collect());
    kv("trig_level", m.trig_level.to_string());
    kv("slope", name(m.slope));
    kv("trig_mode", name(m.trig_mode));
    kv("holdoff", m.holdoff.to_string());
    kv("trig_type", name(m.trig_type));
    kv("trig_level2", m.trig_level2.to_string());
    kv("trig_nrej", m.trig_nrej.to_string());
    kv("trig_src2", m.trig_src2.to_string());
    kv("trig_slope2", name(m.trig_slope2));
    kv("trig_pol", name(m.trig_pol));
    kv("trig_when", name(m.trig_when));
    kv("trig_t1", m.trig_t1.to_string());
    kv("trig_t2", m.trig_t2.to_string());
    kv("video_std", name(m.video_std));
    kv("video_sync", name(m.video_sync));
    kv("video_line", m.video_line.to_string());
    kv("pat", m.pat.iter().map(|&b| name(b)).collect::<Vec<_>>().join(" "));
    kv("pat_levels", list(&m.pat_levels));
    kv("win_mode", name(m.win_mode));
    kv("nth_idle", m.nth_idle.to_string());
    kv("nth_n", m.nth_n.to_string());
    kv("setup", m.setup.to_string());
    kv("hold", m.hold.to_string());
    kv("acq", name(m.acq));
    kv("avg_count", m.avg_count.to_string());
    kv("segments", m.segments.to_string());
    kv("mdepth", m.mdepth.to_string());
    kv("persist", m.persist.to_string());
    kv("intensity", m.intensity.to_string());
    kv("grid", name(m.grid));
    kv("grid_bright", m.grid_bright.to_string());
    kv("grid_frame", m.grid_frame.to_string());
    kv("windows", name(m.win_layout));
    kv("ch_window", m.ch_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("math_window", m.math_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("la_window", (m.la_win + 1).to_string());
    kv("meas", m.meas.iter().map(|&(k, c)| format!("{:?}:{}", k, c + 1)).collect::<Vec<_>>().join(" "));
    kv("meas2", m.meas2.iter().map(|&(k, a, b)| format!("{}:{}:{}", k, a + 1, b + 1)).collect::<Vec<_>>().join(" "));
    kv("dvm", format!("{} {} {}", m.dvm_on, m.dvm_src, m.dvm_mode));
    kv("counter", format!("{} {} {} {}", m.cnt_on, m.cnt_src, m.cnt_mode, m.cnt_digits));
    for c in 0..NCH {
        if !m.ch_label[c].is_empty() {
            kv(&format!("ch{}.label", c + 1), m.ch_label[c].clone());
        }
    }
    kv("cursor", name(m.cursor));
    kv("cur_a", m.cur_a.to_string());
    kv("cur_b", m.cur_b.to_string());
    kv("cur_c", m.cur_c.to_string());
    kv("cur_d", m.cur_d.to_string());
    kv("touch_lock", m.touch_lock.to_string());
    kv("fine_v", m.fine_v.to_string());
    kv("fine_t", m.fine_t.to_string());
    if m.tfine != 1.0 {
        kv("tfine", m.tfine.to_string());
    }
    kv("v_expand_gnd", m.v_expand_gnd.to_string());
    kv("h_expand", format!("{:?}", m.h_expand));
    kv("math", name(m.math));
    kv("math_a", m.math_a.to_string());
    kv("math_b", m.math_b.to_string());
    kv("math_scale", m.math_scale.to_string());
    kv("math_fc", m.math_fc.to_string());
    kv("math_lin", format!("{} {}", m.math_lin.0, m.math_lin.1));
    for (k, e) in m.math_expr.iter().enumerate().filter(|(_, e)| !e.is_empty()) {
        kv(&format!("math_expr{}", k + 1), e.clone());
    }
    kv("log_ivl", m.log_ivl.to_string());
    kv("spec_margin", m.spec_margin.to_string());
    kv("math_offset", m.math_offset.to_string());
    kv("fft_win", name(m.fft_win));
    kv("math_bw", m.math_bw.to_string());
    // the "math*" keys are the selected math channel; the others as mN.* (N = 1..8)
    kv("math_sel", m.math_sel.to_string());
    for (k, c) in m.maths.iter().enumerate().filter(|(k, _)| *k != m.math_sel) {
        let p = |f: &str| format!("m{}.{}", k + 1, f);
        kv(&p("op"), name(c.op));
        kv(&p("a"), c.a.to_string());
        kv(&p("b"), c.b.to_string());
        kv(&p("scale"), c.scale.to_string());
        kv(&p("fc"), c.fc.to_string());
        kv(&p("bw"), c.bw.to_string());
        kv(&p("offset"), c.offset.to_string());
        kv(&p("win"), name(c.win));
        kv(&p("lin"), format!("{} {}", c.lin.0, c.lin.1));
    }
    kv("dec_sel", m.dec_sel.to_string());
    dec_lines(m, &mut kv);
    for k in (0..DECODERS).filter(|&k| k != m.dec_sel && m.decs[k].is_some()) {
        dec_lines(&m.dec_view(k), &mut |n: &str, v: String| kv(&format!("d{}.{}", k + 1, n), v));
    }
    kv("pane", m.pane.to_string());
    kv("meas_labels", m.meas_labels.to_string());
    kv("trend_sel", format!("{} {}", m.trend_sel[0], m.trend_sel[1]));
    kv("zoom", m.zoom.to_string());
    kv("auto_roll", m.auto_roll.to_string());
    kv("zoom_tdiv", tscales()[m.zscale].to_string());
    kv("zdelay", m.zdelay.to_string());
    kv("color_grade", m.color_grade.to_string());
    kv("dots", m.dots.to_string());
    kv("hdmi", name(m.hdmi));
    kv("view", name(m.view));
    kv("bode", format!("{} {} {} {} {} {} {} {}", m.bode_start, m.bode_stop, m.bode_points, m.bode_amp, m.bode_in + 1, m.bode_out + 1, m.bode_gen, m.bode_addr));
    // free sweep: time, log, sync channel, edge, level, input (0 = none), reference dBV
    kv("bode_sweep", format!("{} {} {} {} {} {} {}", m.bode_sw_time, m.bode_sw_log, m.bode_sync + 1, name(m.bode_sync_slope), m.bode_sync_level, m.bode_sw_in, m.bode_ref));
    kv("spec", format!("{} {} {} {} {} {} {} {} {}", m.spec_src + 1, m.spec_center, m.spec_span, m.spec_rbw, name(m.spec_win), m.spec_trace, m.spec_ref, m.spec_wf, m.spec_dist));
    kv("spec_peaks", format!("{} {}", m.spec_sort, m.spec_harm));
    kv("eye", format!("{} {}", m.eye_src + 1, m.eye_rate));
    kv("power", format!("{} {} {}", m.pw_v + 1, m.pw_i + 1, name(m.pw_mode)));
    // zones: "on src inside t:v t:v ..."
    for (k, z) in m.zones.iter().enumerate() {
        let pts: Vec<String> = z.pts.iter().map(|(t, v)| format!("{}:{}", t, v)).collect();
        kv(&format!("zone{}", k + 1), format!("{} {} {} {}", z.on, z.src + 1, z.inside, pts.join(" ")).trim().to_string());
    }
    kv("z_src", m.z_src.map_or("off".to_string(), |c| (c + 1).to_string()));
    kv("hist", name(m.hist));
    kv("mask_on", m.mask_on.to_string());
    kv("mask_src", m.mask_src.to_string());
    kv("mask_x", m.mask_x.to_string());
    kv("mask_y", m.mask_y.to_string());
    kv("mask_stop", m.mask_stop.to_string());
    kv("quick", name(m.quick));
    kv("text_size", name(m.text_size));
    kv("la", m.la.to_text());
    kv("dec_la", format!("{} {}", m.dec_la_in as u8, m.dec_la.map(|n| n.map_or("-".to_string(), |x| x.to_string())).join(" ")));
    s
}

fn p_bool(v: &str) -> Result<bool, String> {
    match v {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        _ => Err(format!("not a boolean: {}", v)),
    }
}

fn p_f32(v: &str) -> Result<f32, String> {
    v.parse::<f32>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("not a number: {}", v))
}

fn p_u32(v: &str) -> Result<u32, String> {
    v.parse::<u32>().map_err(|_| format!("not an integer: {}", v))
}

/// Index below n.
/// A time/div value as an index into `tscales()` (exact step, 0.1 % tolerance).
fn tdiv_index(v: &str) -> Result<usize, String> {
    let x = p_f32(v)?;
    tscales().iter().position(|&s| (s - x).abs() <= s * 1e-3).ok_or_else(|| format!("no time/div step {}", v))
}

fn p_idx(v: &str, n: usize) -> Result<usize, String> {
    let i = v.parse::<usize>().map_err(|_| format!("not an index: {}", v))?;
    if i < n {
        Ok(i)
    } else {
        Err(format!("{} out of range (< {})", i, n))
    }
}

fn p_enum<T: Debug + Copy>(v: &str, all: &[T]) -> Result<T, String> {
    all.iter().copied().find(|x| format!("{:?}", x) == v).ok_or_else(|| format!("unknown value: {}", v))
}

fn p_list<T>(v: &str, n: usize, f: impl Fn(&str) -> Result<T, String>) -> Result<Vec<T>, String> {
    let r: Vec<T> = v.split_whitespace().map(f).collect::<Result<_, _>>()?;
    if r.len() == n {
        Ok(r)
    } else {
        Err(format!("{} values, want {}", r.len(), n))
    }
}

pub fn set(m: &mut Model, k: &str, v: &str) -> Result<(), String> {
    // the other decoders: dec_sel comes first in the file
    if let Some((n, field)) = k.strip_prefix('d').and_then(|r| r.split_once('.')) {
        if let Ok(n) = n.parse::<usize>() {
            let n = n.checked_sub(1).filter(|&n| n < DECODERS).ok_or("unknown decoder")?;
            let mut t = m.dec_view(n);
            set(&mut t, field, v)?;
            m.dec_store(n, t.dec_cfg());
            return Ok(());
        }
    }
    if let Some((n, field)) = k.strip_prefix('m').and_then(|r| r.split_once('.')) {
        if let Ok(n) = n.parse::<usize>() {
            let c = m.maths.get_mut(n.wrapping_sub(1)).ok_or("unknown math channel")?;
            match field {
                "op" => c.op = p_enum(v, &MATH_OPS)?,
                "a" => c.a = p_idx(v, NCH + crate::math::NMATH)?,
                "b" => c.b = p_idx(v, NCH + crate::math::NMATH)?,
                "scale" => c.scale = p_idx(v, crate::math::math_scales().len())?,
                "fc" => c.fc = p_f32(v)?,
                "bw" => c.bw = p_f32(v)?,
                "offset" => c.offset = p_f32(v)?,
                "win" => c.win = p_enum(v, &WINDOWS)?,
                "lin" => {
                    let (a, b) = v.split_once(' ').ok_or("lin: two numbers")?;
                    c.lin = (p_f32(a)?, p_f32(b)?);
                }
                _ => return Err("unknown key".into()),
            }
            return Ok(());
        }
    }
    if let Some((c, field)) = k.strip_prefix("ch").and_then(|r| r.split_once('.')) {
        let c = c.parse::<usize>().ok().filter(|c| (1..=NCH).contains(c)).ok_or("unknown channel")? - 1;
        let ch = &mut m.ch[c];
        match field {
            "on" => ch.on = p_bool(v)?,
            "vfine" => ch.vfine = p_f32(v)?.clamp(1.0, 2.5),
            "unit" => ch.unit = *AmpUnit::ALL.iter().find(|u| u.name() == v.trim()).ok_or_else(|| format!("no unit {}", v))?,
            "bias" => ch.bias = p_f32(v)?,
            "skew" => ch.skew = p_f32(v)?.clamp(-SKEW_MAX, SKEW_MAX),
            // the keyboard takes ten characters; a hand-edited file gets no more
            "label" => m.ch_label[c] = v.trim().chars().take(10).collect(),
            "vdiv" => {
                let x = p_f32(v)?;
                // 20 V/div was once on the ladder, the hardware ends at 10
                let x = x.min(VSCALES[VSCALES.len() - 1]);
                ch.vscale = VSCALES.iter().position(|&s| (s - x).abs() <= s * 1e-3).ok_or_else(|| format!("no V/div step {}", v))?;
            }
            // setups before the 100/200 uV steps: index into the old ladder (500 uV .. 20 V)
            "vscale" => ch.vscale = (p_idx(v, VSCALES.len() - 1)? + 2).min(VSCALES.len() - 1),
            "offset" => ch.offset = p_f32(v)?,
            "coupling" => ch.coupling = p_enum(v, &COUPLINGS)?,
            "bw" => ch.bw = BWS.iter().position(|b| b.0 == v.trim()).ok_or_else(|| format!("unknown bandwidth {}", v))?,
            // setups before 2026-11-03
            "bw20" => ch.bw = if p_bool(v)? { BW_20M } else { BW_FULL },
            "probe" => ch.probe = p_f32(v)?,
            "invert" => ch.invert = p_bool(v)?,
            _ => {} // unknown keys of newer versions are ignored
        }
        return Ok(());
    }
    let ntime = tscales().len();
    match k {
        "active" => m.active = p_idx(v, NCH)?,
        "tdiv" => m.tscale = tdiv_index(v)?,
        // setups before the 1/2 ns steps: index into the old ladder (5 ns .. 1000 s)
        "tscale" => m.tscale = p_idx(v, ntime - 2)? + 2,
        "delay" => m.delay = p_f32(v)?,
        "trig_src" => m.trig_src = p_idx(v, NCH)?,
"pat_la" => {
            if v.trim().len() != 16 {
                return Err(format!("pat_la: {:?}", v));
            }
            for (b, c) in m.pat_la.iter_mut().zip(v.trim().chars()) {
                *b = match c { 'H' => Bit::H, 'L' => Bit::L, _ => Bit::X };
            }
        }
        "trig_la" => m.trig_la = if v.trim() == "none" { None } else { Some(p_idx(v, 16)? as u8) },
        "trig_level" => m.trig_level = p_f32(v)?,
        "slope" => m.slope = p_enum(v, &SLOPES)?,
        "trig_mode" => m.trig_mode = p_enum(v, &TRIG_MODES)?,
        "holdoff" => m.holdoff = p_f32(v)?,
        "trig_type" => m.trig_type = p_enum(v, &TRIG_TYPES)?,
        "trig_nrej" => m.trig_nrej = p_bool(v)?,
        "trig_level2" => m.trig_level2 = p_f32(v)?,
        "trig_src2" => m.trig_src2 = p_idx(v, NCH)?,
        "trig_slope2" => m.trig_slope2 = p_enum(v, &SLOPES)?,
        "trig_pol" => m.trig_pol = p_enum(v, &POLS)?,
        "trig_when" => m.trig_when = p_enum(v, &WHENS)?,
        "trig_t1" => m.trig_t1 = p_f32(v)?,
        "trig_t2" => m.trig_t2 = p_f32(v)?,
        "video_std" => m.video_std = p_enum(v, &VIDEO_STDS)?,
        "video_sync" => m.video_sync = p_enum(v, &VSYNCS)?,
        "video_line" => m.video_line = p_u32(v)?,
        "pat" => {
            let b = p_list(v, NCH, |x| p_enum(x, &BITS))?;
            m.pat.copy_from_slice(&b);
        }
        "pat_levels" => {
            let l = p_list(v, NCH, p_f32)?;
            m.pat_levels.copy_from_slice(&l);
        }
        "win_mode" => m.win_mode = p_enum(v, &WIN_MODES)?,
        "nth_idle" => m.nth_idle = p_f32(v)?,
        "nth_n" => m.nth_n = p_u32(v)?,
        "setup" => m.setup = p_f32(v)?,
        "hold" => m.hold = p_f32(v)?,
        "acq" => m.acq = p_enum(v, &ACQ_MODES)?,
        "avg_count" => m.avg_count = p_u32(v)?.max(1),
        "segments" => m.segments = (p_u32(v)? as usize).min(crate::ui::SEG_MAX),
        "mdepth" => m.mdepth = p_idx(v, mdepths().len())?,
        "persist" => m.persist = p_idx(v, PERSIST.len())?,
        "intensity" => m.intensity = p_u32(v)?.clamp(10, 100),
        "grid" => m.grid = p_enum(v, &GRIDS)?,
        "grid_bright" => m.grid_bright = p_u32(v)?.clamp(10, 100),
        "grid_frame" => m.grid_frame = p_bool(v)?,
        "windows" => m.win_layout = p_enum(v, &WinLayout::ALL)?,
        "ch_window" => {
            let w = p_list(v, NCH, |x| p_idx(x, 5)?.checked_sub(1).ok_or_else(|| "window 0".to_string()))?;
            m.ch_win.copy_from_slice(&w);
        }
        "la_window" => m.la_win = p_idx(v, 5)?.checked_sub(1).ok_or("window 0")?,
        "math_window" => {
            let w = p_list(v, crate::math::NMATH, |x| p_idx(x, 5)?.checked_sub(1).ok_or_else(|| "window 0".to_string()))?;
            m.math_win.copy_from_slice(&w);
        }
        "meas" => {
            let mut out = Vec::new();
            for e in v.split_whitespace() {
                let (k, c) = e.split_once(':').ok_or_else(|| format!("not item:channel: {}", e))?;
                let c = p_idx(c, NCH + 1)?.checked_sub(1).ok_or("channel 0")?;
                out.push((p_enum(k, &ALL_MEAS)?, c));
            }
            m.meas = out;
        }
        "meas2" => {
            let mut out = Vec::new();
            for e in v.split_whitespace() {
                let f: Vec<&str> = e.split(':').collect();
                let [k, a, b] = f[..] else { return Err(format!("not item:a:b: {}", e)) };
                let ch = |x: &str| p_idx(x, NCH + 1)?.checked_sub(1).ok_or("channel 0".to_string());
                out.push((p_idx(k, crate::ui::PAIRS_N)?, ch(a)?, ch(b)?));
            }
            m.meas2 = out;
        }
        "dvm" => {
            let f: Vec<&str> = v.split_whitespace().collect();
            let [on, c, mode] = f[..] else { return Err("dvm: on source mode".into()) };
            (m.dvm_on, m.dvm_src, m.dvm_mode) = (p_bool(on)?, p_idx(c, NCH)?, p_idx(mode, 3)?);
        }
        "counter" => {
            let f: Vec<&str> = v.split_whitespace().collect();
            let [on, c, mode, d] = f[..] else { return Err("counter: on source mode digits".into()) };
            (m.cnt_on, m.cnt_src, m.cnt_mode) = (p_bool(on)?, p_idx(c, NCH)?, p_idx(mode, 3)?);
            m.cnt_digits = p_idx(d, 8)?.max(3) as u32;
        }
        "dec_sel" => m.dec_sel = p_idx(v, DECODERS)?,
        "pane" => m.pane = p_idx(v, crate::ui::PANES_N)?,
        "meas_labels" => m.meas_labels = p_bool(v)?,
        "trend_sel" => {
            let f: Vec<&str> = v.split_whitespace().collect();
            let [a, b] = f[..] else { return Err("trend_sel: two values".into()) };
            m.trend_sel = [p_idx(a, crate::ui::TREND_SEL_N)?, p_idx(b, crate::ui::TREND_SEL_N)?];
        }
        "spec_peaks" => {
            let f: Vec<&str> = v.split_whitespace().collect();
            let [sort, harm] = f[..] else { return Err("spec_peaks: order harmonics".into()) };
            (m.spec_sort, m.spec_harm) = (p_idx(sort, 2)?, p_idx(harm, crate::ui::PANE_HARMS.len())?);
        }
        "cursor" => m.cursor = p_enum(v, &CURSORS)?,
        "cur_a" => m.cur_a = p_f32(v)?,
        "cur_b" => m.cur_b = p_f32(v)?,
        "cur_c" => m.cur_c = p_f32(v)?,
        "cur_d" => m.cur_d = p_f32(v)?,
        "touch_lock" => m.touch_lock = p_bool(v)?,
        "fine_v" => m.fine_v = p_bool(v)?,
        "fine_t" => m.fine_t = p_bool(v)?,
        "tfine" => m.tfine = p_f32(v)?.clamp(0.1, 1.0),
        "v_expand_gnd" => m.v_expand_gnd = p_bool(v)?,
        "h_expand" => m.h_expand = p_enum(v, &HEXPANDS)?,
        "math" => m.math = p_enum(v, &MATH_OPS)?,
        "math_a" => m.math_a = p_idx(v, NCH + crate::math::NMATH)?,
        "math_b" => m.math_b = p_idx(v, NCH + crate::math::NMATH)?,
        "math_bw" => m.math_bw = p_f32(v)?,
        "math_sel" => m.math_sel = p_idx(v, crate::math::NMATH)?,
        "math_scale" => m.math_scale = p_idx(v, crate::math::math_scales().len())?,
        "math_fc" => m.math_fc = p_f32(v)?,
        "log_ivl" => m.log_ivl = p_idx(v, crate::ui::LOG_STEPS_N)?,
        "spec_margin" => m.spec_margin = p_f32(v)?.clamp(0.0, 60.0),
        k if k.starts_with("math_expr") => {
            let n = k[9..].parse::<usize>().map_err(|_| "math_expr: no number")?;
            // a formula that does not parse stays as it is and draws nothing; only the length is checked here
            let e = m.math_expr.get_mut(n.wrapping_sub(1)).ok_or("unknown math channel")?;
            *e = v.trim().chars().take(oos_dsp::expr::EXPR_MAX).collect();
        }
        "math_lin" => {
            let (a, b) = v.split_once(' ').ok_or("math_lin: two numbers")?;
            m.math_lin = (p_f32(a)?, p_f32(b)?);
        }
        "math_offset" => m.math_offset = p_f32(v)?,
        "fft_win" => m.fft_win = p_enum(v, &WINDOWS)?,
        "dec_on" => m.dec_on = p_bool(v)?,
        "dec_src" => m.dec_src = p_idx(v, NCH)?,
        "dec_baud" => m.dec_baud = p_idx(v, crate::decode::BAUDS.len())?,
        "dec_canrate" => m.dec_canrate = p_idx(v, crate::decode::CAN_RATES.len())?,
        "dec_canh" => m.dec_canh = p_bool(v)?,
        "dec_fdrate" => m.dec_fdrate = p_idx(v, crate::decode::FD_RATES.len())?,
        "dec_arate" => m.dec_arate = p_idx(v, crate::decode::ARINC_RATES.len())?,
        "dec_tick" => m.dec_tick = p_idx(v, crate::decode::SENT_TICKS.len())?,
        "dec_nibbles" => m.dec_nibbles = p_u32(v)?.clamp(1, 6),
        "dec_uspeed" => m.dec_uspeed = p_idx(v, crate::decode::USB_SPEEDS.len())?,
        "dec_src4" => m.dec_src4 = if v.trim() == "none" { None } else { Some(p_idx(v, NCH + 1)?.max(1) - 1) },
        "dec_diff" => m.dec_diff = if v.trim() == "none" { None } else { Some(p_idx(v, NCH + 1)?.max(1) - 1) },
        "trig_bus" => m.trig_bus = *BUS_WHATS.iter().find(|b| b.name() == v.trim()).ok_or_else(|| format!("unknown bus match {}", v))?,
        "trig_bus_val" => m.trig_bus_val = v.trim().parse().map_err(|e| format!("{}", e))?,
        "dec_thr" => m.dec_thr = p_f32(v)?,
        "dec_ascii" => m.dec_ascii = p_bool(v)?,
        "dec_proto" => m.dec_proto = Proto::from_key(v.trim()).ok_or_else(|| format!("unknown protocol: {}", v.trim()))?,
        // a module's option indices; brought into range where they are used (`protocols::opt_fit`)
        "dec_opt" => m.dec_opt = p_list(v, crate::protocols::OPTS, |x| x.parse::<u8>().map_err(|e| e.to_string()))?.try_into().unwrap(),
        "dec_src2" => m.dec_src2 = p_idx(v, NCH)?,
        "dec_cs" => m.dec_cs = if v == "none" { None } else { Some(p_idx(v, NCH)?) },
        "dec_mode" => m.dec_mode = p_idx(v, 4)? as u8,
        "dec_bits" => m.dec_bits = p_u32(v)?.clamp(4, 32),
        "dec_parity" => m.dec_parity = p_enum(v, &PARITIES)?,
        "zoom" => m.zoom = p_bool(v)?,
        "auto_roll" => m.auto_roll = p_bool(v)?,
        "zoom_tdiv" => m.zscale = tdiv_index(v)?,
        "zscale" => m.zscale = p_idx(v, ntime - 2)? + 2,
        "zdelay" => m.zdelay = p_f32(v)?,
        "color_grade" => m.color_grade = p_bool(v)?,
        "dots" => m.dots = p_bool(v)?,
        "hdmi" => m.hdmi = p_enum(v, &HDMI_MODES)?,
        // setups before the Layout mode: true = Waveform, false = Mirror
        "hdmi_wave" => m.hdmi = if p_bool(v)? { HdmiMode::Waveform } else { HdmiMode::Mirror },
        "view" => m.view = p_enum(v, &[AView::Scope, AView::Spectrum, AView::Eye, AView::Power, AView::Bode])?,
        "bode" => {
            // the address is the rest of the line: it may be empty
            let w: Vec<&str> = v.trim().splitn(8, char::is_whitespace).collect();
            if w.len() < 7 {
                return Err("bode: want 8 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            (m.bode_start, m.bode_stop, m.bode_amp) = (f(w[0])?, f(w[1])?, f(w[3])?);
            m.bode_points = p_u32(w[2])?.clamp(2, 1000) as usize;
            m.bode_in = p_idx(w[4], NCH + 1)?.max(1) - 1;
            m.bode_out = p_idx(w[5], NCH + 1)?.max(1) - 1;
            m.bode_gen = p_idx(w[6], crate::ui::bode::GENERATORS.len())?;
            m.bode_addr = w.get(7).map_or("", |a| a.trim()).to_string();
        }
        "bode_sweep" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 7 {
                return Err("bode_sweep: want 7 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            m.bode_sw_time = f(w[0])?;
            if !m.bode_sw_time.is_finite() || m.bode_sw_time <= 0.0 {
                return Err("bode_sweep: the sweep time must be above 0".into());
            }
            m.bode_sw_log = p_bool(w[1])?;
            m.bode_sync = p_idx(w[2], NCH + 1)?.max(1) - 1;
            m.bode_sync_slope = p_enum(w[3], &SLOPES[..2])?;
            m.bode_sync_level = p_f32(w[4])?;
            m.bode_sw_in = p_idx(w[5], NCH + 1)?;
            m.bode_ref = f(w[6])?;
        }
        "spec" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 9 {
                return Err("spec: want 9 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            m.spec_src = p_idx(w[0], NCH + 1)?.max(1) - 1;
            (m.spec_center, m.spec_span, m.spec_rbw) = (f(w[1])?, f(w[2])?, f(w[3])?);
            m.spec_win = p_enum(w[4], &WINDOWS)?;
            m.spec_trace = p_idx(w[5], 4)?;
            m.spec_ref = p_f32(w[6])?;
            (m.spec_wf, m.spec_dist) = (p_bool(w[7])?, p_bool(w[8])?);
        }
        "eye" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            m.eye_src = p_idx(w.first().ok_or("eye: source")?, NCH + 1)?.max(1) - 1;
            m.eye_rate = w.get(1).ok_or("eye: rate")?.parse::<f64>().map_err(|e| e.to_string())?;
        }
        "power" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 3 {
                return Err("power: want 3 values".into());
            }
            m.pw_v = p_idx(w[0], NCH + 1)?.max(1) - 1;
            m.pw_i = p_idx(w[1], NCH + 1)?.max(1) - 1;
            m.pw_mode = p_enum(w[2], &[PowerMode::Quality, PowerMode::Harmonics, PowerMode::Switching, PowerMode::Ripple])?;
        }
        "zone1" | "zone2" => {
            let z = &mut m.zones[if k == "zone1" { 0 } else { 1 }];
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() < 3 {
                return Err("zone: want on src inside points".into());
            }
            z.on = p_bool(w[0])?;
            z.src = p_idx(w[1], NCH + 1)?.max(1) - 1;
            z.inside = p_bool(w[2])?;
            z.pts = w[3..]
                .iter()
                .map(|p| {
                    let (t, v) = p.split_once(':').ok_or("zone point: want t:v")?;
                    Ok((t.parse::<f64>().map_err(|e| e.to_string())?, v.parse::<f64>().map_err(|e| e.to_string())?))
                })
                .collect::<Result<_, String>>()?;
        }
        "z_src" => m.z_src = if v.trim() == "off" { None } else { Some(p_idx(v, NCH + 1)?.max(1) - 1) },
        "hist" => m.hist = p_enum(v, &[Hist::Off, Hist::Vertical, Hist::Horizontal])?,
        "mask_on" => m.mask_on = p_bool(v)?,
        "mask_src" => m.mask_src = p_idx(v, NCH)?,
        "mask_x" => m.mask_x = p_f32(v)?,
        "mask_y" => m.mask_y = p_f32(v)?,
        "mask_stop" => m.mask_stop = p_bool(v)?,
        "quick" => m.quick = p_enum(v, &QUICKS)?,
        "text_size" => m.text_size = p_enum(v, &TEXT_SIZES)?,
        "la" => m.la = crate::ui::la::LaSet::parse(v)?,
        "dec_la" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 5 {
                return Err(format!("dec_la: {:?}", v));
            }
            m.dec_la_in = w[0] == "1";
            for (r, s) in m.dec_la.iter_mut().zip(&w[1..]) {
                *r = if *s == "-" { None } else { Some(s.parse::<u8>().map_err(|_| format!("dec_la: {:?}", v))?.min(15)) };
            }
        }
        _ => {} // unknown keys of newer versions are ignored
    }
    Ok(())
}

/// Parses a setup file on top of the default set-up (keys missing in the file keep their defaults). Errors name the
/// line and the key; unknown keys are ignored.
pub fn parse_setup(text: &str) -> Result<Model, String> {
    let mut m = Model::default();
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: no '='", ln + 1))?;
        let k = k.trim();
        set(&mut m, k, v.trim()).map_err(|e| format!("line {}: {}: {}", ln + 1, k, e))?;
    }
    if m.zscale > m.tscale {
        m.zscale = m.tscale;
    }
    Ok(m)
}

// ---------------------------------------------------------------------------------------------- waveform CSV

/// CSV of the enabled channels of a record: `Time (s),CH1 (V),...`, one row per sample, time relative to the
/// trigger; samples without data (NaN) are empty fields.
pub fn wave_csv(rec: &Record, m: &Model) -> String {
    let chans: Vec<usize> = (0..NCH).filter(|&c| m.ch[c].on && !rec.ch[c].is_empty()).collect();
    let n = chans.iter().map(|&c| rec.ch[c].len()).max().unwrap_or(0);
    let mut s = String::with_capacity(n * (16 + 12 * chans.len()) + 64);
    s.push_str("Time (s)");
    for &c in &chans {
        s.push_str(&format!(",CH{} (V)", c + 1));
    }
    s.push('\n');
    for k in 0..n {
        s.push_str(&format!("{:.9e}", rec.t0 + k as f64 * rec.dt));
        for &c in &chans {
            s.push(',');
            if let Some(v) = rec.ch[c].get(k).filter(|v| v.is_finite()) {
                s.push_str(&v.to_string());
            }
        }
        s.push('\n');
    }
    s
}

/// Logic analyser record as CSV: the lines shown, one row at the record start and one for every change of them, so a
/// record of millions of samples stays small and nothing is lost (the levels hold until the next row). The last row is
/// the end of the record.
pub fn la_csv(la: &crate::ui::la::LaRec, lines: &[usize]) -> String {
    let mask = lines.iter().fold(0u16, |m, &n| m | 1 << n);
    let mut s = String::from("Time (s)");
    for n in lines {
        s.push_str(&format!(",D{}", n));
    }
    s.push('\n');
    let row = |s: &mut String, k: usize, w: u16| {
        s.push_str(&format!("{:.9e}", la.t0 + k as f64 * la.dt));
        for &n in lines {
            s.push_str(if w >> n & 1 == 1 { ",1" } else { ",0" });
        }
        s.push('\n');
    };
    let mut prev = None;
    for (k, &w) in la.words.iter().enumerate() {
        if prev != Some(w & mask) {
            row(&mut s, k, w);
            prev = Some(w & mask);
        }
    }
    if let (Some(&w), true) = (la.words.last(), la.words.len() > 1) {
        row(&mut s, la.words.len() - 1, w);
    }
    s
}

/// The record in a plain binary form of OOS, little endian:
/// "OOSW", u32 version 1, u32 number of channels, u64 samples per channel, f64 dt, f64 t0, then per channel its number
/// (u32, 1..4) and the samples as f32 volts.
pub fn wave_bin(rec: &Record, m: &Model) -> Vec<u8> {
    let chans: Vec<usize> = (0..NCH).filter(|&c| m.ch[c].on && !rec.ch[c].is_empty()).collect();
    let n = chans.iter().map(|&c| rec.ch[c].len()).max().unwrap_or(0);
    let mut b = Vec::with_capacity(36 + chans.len() * (4 + 4 * n));
    b.extend_from_slice(b"OOSW");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(chans.len() as u32).to_le_bytes());
    b.extend_from_slice(&(n as u64).to_le_bytes());
    b.extend_from_slice(&rec.dt.to_le_bytes());
    b.extend_from_slice(&rec.t0.to_le_bytes());
    for &c in &chans {
        b.extend_from_slice(&(c as u32 + 1).to_le_bytes());
        for i in 0..n {
            b.extend_from_slice(&rec.ch[c].get(i).copied().unwrap_or(f32::NAN).to_le_bytes());
        }
    }
    b
}

// ---------------------------------------------------------------------------------------------- PNG

/// CRC-32 tables for eight bytes per step ("slicing by 8"): `T[0]` is the usual byte table, `T[k][n]` the CRC of byte
/// `n` followed by `k` zero bytes.
const CRC_T: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][n] = c;
        n += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut n = 0;
        while n < 256 {
            let c = t[k - 1][n];
            t[k][n] = t[0][(c & 0xff) as usize] ^ (c >> 8);
            n += 1;
        }
        k += 1;
    }
    t
};

/// CRC-32 (ISO 3309 / PNG / zip).
pub fn crc32(data: &[u8]) -> u32 {
    let t = &CRC_T;
    let mut c = !0u32;
    let mut it = data.chunks_exact(8);
    for b in &mut it {
        let lo = c ^ u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][(lo >> 8 & 0xff) as usize]
            ^ t[5][(lo >> 16 & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][b[4] as usize]
            ^ t[2][b[5] as usize]
            ^ t[1][b[6] as usize]
            ^ t[0][b[7] as usize];
    }
    !it.remainder().iter().fold(c, |c, &b| t[0][((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// Adler-32 (zlib). Sixteen lanes side by side (the compiler makes vector code of the lane loop): lane `k` sums the
/// bytes at `16 j + k` (`sa`) and those sums again (`sb`), and per chunk of 5552 bytes `b` grows by `len * a`,
/// `16 * sum(sb)` less `sum(k * sa[k])` (each byte counted `len - position` times), the same as byte by byte.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u64, 0u64);
    for chunk in data.chunks(5552) {
        let (mut sa, mut sb) = ([0u32; 16], [0u32; 16]);
        let mut blocks = chunk.chunks_exact(16);
        for blk in &mut blocks {
            for k in 0..16 {
                sa[k] += blk[k] as u32;
                sb[k] += sa[k];
            }
        }
        let n = (chunk.len() - blocks.remainder().len()) as u64;
        let sum_a: u64 = sa.iter().map(|&x| x as u64).sum();
        let sum_b: u64 = sb.iter().map(|&x| x as u64).sum();
        let weighted: u64 = sa.iter().enumerate().map(|(k, &x)| k as u64 * x as u64).sum();
        b += n * a + 16 * sum_b - weighted;
        a += sum_a;
        for &x in blocks.remainder() {
            a += x as u64;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    ((b << 16) | a) as u32
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Deflate length codes 257..285 (RFC 1951 3.2.5): base length and extra bits.
const LEN_BASE: [(u16, u8); 29] = [
    (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0), (11, 1), (13, 1), (15, 1), (17, 1), (19, 2), (23, 2), (27, 2),
    (31, 2), (35, 3), (43, 3), (51, 3), (59, 3), (67, 4), (83, 4), (99, 4), (115, 4), (131, 5), (163, 5), (195, 5), (227, 5), (258, 0),
];
/// Deflate distance codes 0..29: base distance and extra bits.
const DIST_BASE: [(u16, u8); 30] = [
    (1, 0), (2, 0), (3, 0), (4, 0), (5, 1), (7, 1), (9, 2), (13, 2), (17, 3), (25, 3), (33, 4), (49, 4), (65, 5), (97, 5), (129, 6),
    (193, 6), (257, 7), (385, 7), (513, 8), (769, 8), (1025, 9), (1537, 9), (2049, 10), (3073, 10), (4097, 11), (6145, 11),
    (8193, 12), (12289, 12), (16385, 13), (24577, 13),
];

/// Bit writer of a deflate stream: values LSB first, Huffman codes MSB first.
struct BitOut {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitOut {
    /// At most 32 bits at a time; whole bytes go out once 32 have gathered.
    fn bits(&mut self, v: u32, n: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        if self.n >= 32 {
            self.out.extend_from_slice(&(self.acc as u32).to_le_bytes());
            self.acc >>= 32;
            self.n -= 32;
        }
    }
    /// A Huffman code of `n` bits, first bit = most significant.
    fn code(&mut self, c: u32, n: u32) {
        self.bits(c.reverse_bits() >> (32 - n), n);
    }
    /// Literal/length symbol of the fixed code (RFC 1951 3.2.6).
    fn symbol(&mut self, s: u32) {
        match s {
            0..=143 => self.code(0x30 + s, 8),
            144..=255 => self.code(0x190 + s - 144, 9),
            256..=279 => self.code(s - 256, 7),
            _ => self.code(0xc0 + s - 280, 8),
        }
    }
    /// Length symbol and extra bits of a match of 3..=258 bytes, then the 5-bit distance code and its extra bits.
    fn matched(&mut self, len: usize, dist_code: usize, dist_extra: u32) {
        let i = LEN_CODE[len] as usize;
        self.symbol(257 + i as u32);
        self.bits((len - LEN_BASE[i].0 as usize) as u32, LEN_BASE[i].1 as u32);
        self.code(dist_code as u32, 5);
        self.bits(dist_extra, DIST_BASE[dist_code].1 as u32);
    }
    fn finish(mut self) -> Vec<u8> {
        while self.n > 0 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n = self.n.saturating_sub(8);
        }
        self.out
    }
}

/// Index into `LEN_BASE` for each match length 0..=258 (0..2 unused).
const LEN_CODE: [u8; 259] = {
    let mut t = [0u8; 259];
    let mut len = 3;
    while len <= 258 {
        let mut i = 28;
        while LEN_BASE[i].0 as usize > len {
            i -= 1;
        }
        t[len] = i as u8;
        len += 1;
    }
    t
};

/// Number of equal bytes at `i` and `i - d`, at most 258 (the longest deflate match); eight bytes per step.
#[inline]
fn match_len(raw: &[u8], i: usize, d: usize) -> usize {
    let max = (raw.len() - i).min(258);
    let (a, b) = (&raw[i..i + max], &raw[i - d..i - d + max]);
    let mut n = 0;
    while n + 8 <= max {
        let x = u64::from_le_bytes(a[n..n + 8].try_into().unwrap()) ^ u64::from_le_bytes(b[n..n + 8].try_into().unwrap());
        if x != 0 {
            return n + (x.trailing_zeros() / 8) as usize;
        }
        n += 8;
    }
    while n < max && a[n] == b[n] {
        n += 1;
    }
    n
}

/// Deflate (one block, fixed Huffman codes) of picture rows: besides literals only matches against the pixel to the left
/// (distance 3) and the pixel above (distance `stride`). A scope screen is flat areas, grid lines and a few traces, so
/// that is most of what a full LZ77 finds, in one pass, with no hash table and little time on the scope's CPU. The
/// left match costs 10 bits less than the one above (no extra distance bits), so it is looked for first and the one
/// above only when the left one is not the longest possible.
fn deflate_rows(raw: &[u8], stride: usize) -> Vec<u8> {
    let (stride_code, stride_extra) = {
        let c = DIST_BASE.iter().rposition(|&(b, _)| b as usize <= stride).unwrap_or(0);
        (c, stride.saturating_sub(DIST_BASE[c].0 as usize) as u32)
    };
    let stride_ok = stride > 3 && stride <= 32768;
    let mut o = BitOut { out: Vec::with_capacity(raw.len() / 8 + 64), acc: 0, n: 0 };
    o.bits(1, 1); // BFINAL
    o.bits(1, 2); // BTYPE 01 = fixed Huffman
    let mut i = 0;
    while i < raw.len() {
        let left = if i >= 3 { match_len(raw, i, 3) } else { 0 };
        let above = if left < 258 && stride_ok && i >= stride { match_len(raw, i, stride) } else { 0 };
        if above > left && above >= 3 {
            o.matched(above, stride_code, stride_extra);
            i += above;
        } else if left >= 3 {
            o.matched(left, 2, 0);
            i += left;
        } else {
            o.symbol(raw[i] as u32);
            i += 1;
        }
    }
    o.symbol(256);
    o.finish()
}

/// PNG (8-bit RGB) of XRGB8888 pixels: filter 0 per row, zlib stream of one fixed-Huffman deflate block
/// (`deflate_rows`), so a screen of flat colour and thin lines takes a few percent of its bitmap.
pub fn png(w: u32, h: u32, px: &[u32]) -> Vec<u8> {
    let stride = w as usize * 3 + 1;
    let mut raw = vec![0u8; stride * h as usize];
    for (out, row) in raw.chunks_exact_mut(stride).zip(px.chunks(w as usize)) {
        // out[0] = 0: filter type None
        for (o, &p) in out[1..].chunks_exact_mut(3).zip(row) {
            o.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01]; // deflate, 32 K window, no dictionary, (0x7801 % 31 == 0)
    z.extend_from_slice(&deflate_rows(&raw, stride));
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bit, RGB, deflate, filter set 0, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// PNG of the screen canvas.
/// Draws a binary PPM (P6, maxval 255) centred into the canvas (black around it); returns its size.
pub fn ppm_into(b: &[u8], cv: &mut Canvas) -> Option<(i32, i32)> {
    // header: "P6" w h maxval, separated by whitespace, comments with '#'
    let mut fields = Vec::new();
    let mut p = 2;
    if !b.starts_with(b"P6") {
        return None;
    }
    while fields.len() < 3 {
        while p < b.len() && (b[p].is_ascii_whitespace() || b[p] == b'#') {
            if b[p] == b'#' {
                while p < b.len() && b[p] != b'\n' {
                    p += 1;
                }
            }
            p += 1;
        }
        let s = p;
        while p < b.len() && b[p].is_ascii_digit() {
            p += 1;
        }
        fields.push(std::str::from_utf8(&b[s..p]).ok()?.parse::<i32>().ok()?);
    }
    let (w, h, max) = (fields[0], fields[1], fields[2]);
    p += 1;
    if max != 255 || w <= 0 || h <= 0 || b.len() < p + (w * h * 3) as usize {
        return None;
    }
    cv.fill(crate::gfx::Rect::new(0, 0, cv.w, cv.h), 0);
    let (ox, oy) = ((cv.w - w) / 2, (cv.h - h) / 2);
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (ox + x, oy + y);
            if (0..cv.w).contains(&cx) && (0..cv.h).contains(&cy) {
                let k = p + ((y * w + x) * 3) as usize;
                cv.px[(cy * cv.w + cx) as usize] = crate::gfx::rgb(b[k] as u32, b[k + 1] as u32, b[k + 2] as u32);
            }
        }
    }
    Some((w, h))
}

pub fn png_of(cv: &Canvas) -> Vec<u8> {
    png(cv.w as u32, cv.h as u32, &cv.px)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::rgb;
    use crate::math::{MathOp, Window};

    #[test]
    fn a_save_does_not_follow_links_or_write_to_devices() {
        let d = std::env::temp_dir().join(format!("oos-link-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (victim, link) = (d.join("victim"), d.join("shot-001.png"));
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(write(&link, b"x").is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        // taken, also when it dangles
        std::fs::remove_file(&victim).unwrap();
        assert_eq!(next_name(&d, "shot", "png"), d.join("shot-002.png"));
        assert!(write(&link, b"x").is_err());
        assert!(!victim.exists());
        assert!(write(Path::new("/dev/null"), b"x").is_err());
        write(&d.join("ok.txt"), b"1").unwrap();
        write(&d.join("ok.txt"), b"2").unwrap();
        assert_eq!(std::fs::read(d.join("ok.txt")).unwrap(), b"2");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tdiv_by_value_and_legacy_index() {
        let m = parse_setup("tdiv = 1e-9\nzoom_tdiv = 1e-9\n").unwrap();
        assert_eq!((tscales()[m.tscale], tscales()[m.zscale]), (1e-9, 1e-9));
        assert!(setup_text(&m).contains("tdiv = 0.000000001"));
        // old ladder started at 5 ns: index 0 was 5 ns/div, 3 was 50 ns
        let m = parse_setup("tscale = 3\nzscale = 0\n").unwrap();
        assert_eq!((tscales()[m.tscale], tscales()[m.zscale]), (50e-9, 5e-9));
        assert!(parse_setup("tdiv = 3e-9").is_err());
    }

    #[test]
    fn vdiv_by_value_and_legacy_index() {
        let m = parse_setup("ch1.vdiv = 0.0001\nch2.vscale = 10\n").unwrap();
        assert_eq!(VSCALES[m.ch[0].vscale], 100e-6);
        // old ladder started at 500 uV: index 10 was 1 V/div
        assert_eq!(VSCALES[m.ch[1].vscale], 1.0);
        assert_eq!(VSCALES[Model::default().ch[0].vscale], 1.0);
        assert!(setup_text(&m).contains("ch1.vdiv = 0.0001"));
    }

    /// A non-default set-up in every group of fields.
    fn custom() -> Model {
        let mut m = Model::default();
        (m.bode_sw_time, m.bode_sw_log, m.bode_sync, m.bode_sync_slope, m.bode_sync_level, m.bode_sw_in, m.bode_ref) = (12.5, false, 2, Slope::Fall, -0.75, 0, -6.02);
        m.ch[1].on = true;
        m.ch[1].vscale = 1; // 200 uV/div
        m.ch[1].offset = -0.123_456_79;
        m.ch[2].coupling = Coupling::Ac;
        m.ch[2].bw = BW_20M;
        m.ch[1].bw = 17;
        m.ch[3].probe = 10.0;
        m.ch[3].invert = true;
        m.active = 2;
        m.tscale = 5;
        m.delay = 1.5e-7;
        m.trig_src = 3;
        m.trig_level = 0.1;
        m.slope = Slope::Either;
        m.trig_mode = TrigMode::Normal;
        m.trig_type = TrigType::SetupHold;
        m.trig_level2 = -2.25;
        m.trig_src2 = 1;
        m.trig_slope2 = Slope::Fall;
        m.trig_pol = Pol::Neg;
        m.trig_when = WhenKind::Out;
        m.trig_t1 = 3e-9;
        m.trig_t2 = 0.7;
        m.video_std = VideoStd::Ntsc;
        m.video_sync = VSync::Line;
        m.video_line = 123;
        m.pat = [Bit::R, Bit::F, Bit::L, Bit::X];
        m.pat_levels = [0.1, -0.2, 3.3, 1e-3];
        m.win_mode = WinMode::Inside;
        m.nth_idle = 2e-3;
        m.nth_n = 7;
        m.setup = 5e-9;
        m.hold = 6e-9;
        m.acq = AcqMode::Average;
        m.avg_count = 256;
        m.mdepth = 4;
        m.persist = 6;
        m.intensity = 85;
        m.grid = Grid::Light;
        m.meas = vec![(Meas::Rise, 3), (Meas::Vrms, 0), (Meas::PDuty, 1)];
        m.cursor = CursorMode::Volt;
        m.cur_a = 0.125;
        m.cur_b = 0.875;
        m.touch_lock = true;
        m.maths[3] = crate::math::MathCh { op: MathOp::Bpf, a: 4, b: 0, scale: 7, fc: 1234.0, bw: 56.0, offset: -1.5, win: Window::Blackman, lin: (1.0, 0.0) };
        m.math_bw = 777.0;
        m.zones[1] = crate::zone::Zone { on: true, src: 2, inside: false, pts: vec![(-1e-4, 0.5), (2.5e-5, 1.25), (3e-5, -0.75)] };
        m.dots = true;
        m.z_src = Some(3);
        m.hist = Hist::Horizontal;
        (m.view, m.spec_src, m.spec_center, m.spec_span, m.spec_rbw, m.spec_trace, m.spec_ref, m.spec_wf, m.spec_dist) = (AView::Spectrum, 2, 1.5e6, 2e5, 1e3, 2, -12.5, true, true);
        (m.spec_sort, m.spec_harm, m.trend_sel) = (1, 4, [3, 1]);
        (m.eye_src, m.eye_rate, m.pw_v, m.pw_i, m.pw_mode) = (3, 1.25e9, 2, 0, PowerMode::Switching);
        m.math = MathOp::Fft;
        m.math_a = 2;
        m.math_b = 3;
        m.math_scale = 2;
        m.math_fc = 12345.0;
        m.math_offset = -1.3;
        m.fft_win = Window::Flattop;
        m.dec_on = true;
        m.dec_src = 1;
        m.dec_baud = 3;
        m.dec_thr = 0.8;
        m.dec_ascii = false;
        m.dec_proto = Proto::Spi;
        (m.dec_arate, m.dec_tick, m.dec_nibbles, m.dec_uspeed) = (2, 3, 4, 2);
        m.dec_src2 = 0;
        m.dec_cs = None;
        m.dec_mode = 3;
        m.dec_bits = 16;
        m.dec_parity = Parity::Odd;
        m.zoom = true;
        m.zscale = 2;
        m.zdelay = -3e-8;
        m.color_grade = true;
        m.mask_on = true;
        m.mask_src = 3;
        m.mask_x = 0.5;
        m.mask_y = 1.25;
        m.mask_stop = true;
        m.quick = Quick::SaveCsv;
        m.la = crate::ui::la::LaSet { on: true, show: 0x0f0f, preset: [2, crate::ui::la::USER], user: [0.5, -0.25], height: 2 };
        m
    }

    #[test]
    fn setup_round_trip() {
        let m = custom();
        assert_ne!(m, Model::default());
        let text = setup_text(&m);
        let back = parse_setup(&text).unwrap();
        assert_eq!(back, m);
        // and the default set-up
        assert_eq!(parse_setup(&setup_text(&Model::default())).unwrap(), Model::default());
    }

    #[test]
    fn setup_errors_and_unknown_keys() {
        let m = parse_setup("# comment\n\nnew_key_of_a_later_version = 3\nch2.on = true\n").unwrap();
        assert!(m.ch[1].on);
        assert!(parse_setup("tscale = 9999").unwrap_err().contains("line 1: tscale"));
        assert!(parse_setup("ch1.coupling = XY").is_err());
        assert!(parse_setup("ch9.on = true").is_err());
        assert!(parse_setup("pat = H L").is_err());
        assert!(parse_setup("delay").is_err());
    }

    /// The Quick key action: written by name, read back, unknown names rejected, missing key = Screenshot.
    #[test]
    fn setup_quick_key() {
        for &q in QUICKS.iter() {
            let mut m = Model::default();
            m.quick = q;
            let text = setup_text(&m);
            assert!(text.contains(&format!("quick = {:?}\n", q)));
            assert_eq!(parse_setup(&text).unwrap().quick, q);
        }
        assert_eq!(parse_setup("quick = Autoset\nquick_v2 = Whatever\n").unwrap().quick, Quick::Autoset);
        assert_eq!(parse_setup("ch1.on = true\n").unwrap().quick, Quick::Screenshot);
        assert!(parse_setup("quick = Coffee").unwrap_err().contains("line 1: quick"));
    }

    /// Every protocol is stored by name; the USB decoder with its D- source and speed.
    #[test]
    fn setup_protocols_by_name() {
        for &p in PROTOS.iter() {
            let mut m = Model::default();
            m.dec_proto = p;
            let text = setup_text(&m);
            assert!(text.contains(&format!("dec_proto = {}\n", p.key())));
            assert_eq!(parse_setup(&text).unwrap().dec_proto, p);
        }
        // the protocols that became modules keep the names of setups saved before
        for (old, name) in [("Dmx512", "DMX512"), ("Spdif", "S/PDIF"), ("UsbPd", "USB PD")] {
            assert_eq!(parse_setup(&format!("dec_proto = {}\n", old)).unwrap().dec_proto.name(), name);
        }
        // a module's options go with it
        let mut m = Model::default();
        m.dec_proto = Proto::from_key("Example").unwrap();
        m.dec_opt = [2, 1, 0, 0];
        let text = setup_text(&m);
        assert!(text.contains("dec_proto = Example\n") && text.contains("dec_opt = 2 1 0 0\n"));
        assert_eq!(parse_setup(&text).unwrap(), m);
        assert!(parse_setup("dec_proto = NoSuchBus").is_err());
        let mut m = Model::default();
        (m.dec_proto, m.dec_src, m.dec_cs, m.dec_uspeed) = (Proto::Usb, 2, Some(3), 1);
        let text = setup_text(&m);
        assert!(text.contains("dec_proto = Usb\n") && text.contains("dec_uspeed = 1\n"));
        assert_eq!(parse_setup(&text).unwrap(), m);
        assert!(parse_setup("dec_uspeed = 3").is_err());
    }

    #[test]
    fn csv_of_a_record() {
        let mut m = Model::default();
        m.ch[2].on = true;
        let mut rec = Record { t0: -1e-3, dt: 1e-6, ch: Default::default(), triggered: true, partial: false, raw: None, la: None };
        rec.ch[0] = vec![0.5, 1.25, f32::NAN];
        rec.ch[2] = vec![-1.0, 0.0, 3.3];
        let csv = wave_csv(&rec, &m);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Time (s),CH1 (V),CH3 (V)");
        assert_eq!(lines[1], "-1.000000000e-3,0.5,-1");
        assert_eq!(lines[3], "-9.980000000e-4,,3.3");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        // the eight-byte steps against the plain bitwise definitions, all lengths around the step sizes
        let data: Vec<u8> = (0..20000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        for n in (0..70).chain([5551, 5552, 5553, 11104, 20000]) {
            let d = &data[..n];
            let crc = !d.iter().fold(!0u32, |mut c, &x| {
                c ^= x as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                c
            });
            assert_eq!(crc32(d), crc, "CRC of {} bytes", n);
            let (a, b) = d.iter().fold((1u32, 0u32), |(a, b), &x| ((a + x as u32) % 65521, (b + (a + x as u32) % 65521) % 65521));
            assert_eq!(adler32(d), b << 16 | a, "Adler of {} bytes", n);
        }
        assert_eq!(adler32(&[0xff; 100_000]), {
            let (a, b) = (0..100_000).fold((1u64, 0u64), |(a, b), _| ((a + 255) % 65521, (b + (a + 255) % 65521) % 65521));
            (b << 16 | a) as u32
        });
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(adler32(&[]), 1);
    }

    /// Reads a deflate stream bit by bit (RFC 1951 3.1.1): values LSB first, Huffman codes MSB first.
    struct Bits<'a> {
        d: &'a [u8],
        p: usize,
    }
    impl Bits<'_> {
        fn bit(&mut self) -> u32 {
            let b = (self.d[self.p >> 3] >> (self.p & 7)) & 1;
            self.p += 1;
            b as u32
        }
        fn bits(&mut self, n: u32) -> u32 {
            (0..n).fold(0, |v, i| v | self.bit() << i)
        }
    }

    /// One symbol of the fixed Huffman code (RFC 1951 3.2.6): 7 bits 256..279, 8 bits 0..143 and 280..287, 9 bits 144..255.
    fn fixed_symbol(b: &mut Bits) -> u32 {
        let mut c = (0..7).fold(0, |c, _| c << 1 | b.bit());
        if c <= 0b001_0111 {
            return 256 + c;
        }
        c = c << 1 | b.bit();
        if (0b0011_0000..=0b1011_1111).contains(&c) {
            return c - 0b0011_0000;
        }
        if (0b1100_0000..=0b1100_0111).contains(&c) {
            return 280 + c - 0b1100_0000;
        }
        c = c << 1 | b.bit();
        144 + c - 0b1_1001_0000
    }

    /// Independent inflate for what the encoder may write (stored and fixed Huffman blocks, not dynamic ones); returns the
    /// data and the number of bytes the stream took.
    fn inflate(z: &[u8]) -> (Vec<u8>, usize) {
        const LBASE: [u32; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
        const LEXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
        const DBASE: [u32; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
        const DEXTRA: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
        let mut b = Bits { d: z, p: 0 };
        let mut out: Vec<u8> = Vec::new();
        loop {
            let last = b.bit();
            match b.bits(2) {
                0 => {
                    b.p = (b.p + 7) & !7;
                    let (n, nn) = (b.bits(16), b.bits(16));
                    assert_eq!(n, !nn & 0xffff, "stored block length check");
                    for _ in 0..n {
                        out.push(b.bits(8) as u8);
                    }
                }
                1 => loop {
                    let s = fixed_symbol(&mut b);
                    if s < 256 {
                        out.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        assert!(s <= 285, "length symbol {}", s);
                        let i = (s - 257) as usize;
                        let len = LBASE[i] + b.bits(LEXTRA[i]);
                        let dc = (0..5).fold(0, |c, _| c << 1 | b.bit()) as usize;
                        assert!(dc < 30, "distance code {}", dc);
                        let dist = (DBASE[dc] + b.bits(DEXTRA[dc])) as usize;
                        assert!(dist <= out.len(), "distance {} reaches before the start ({} bytes so far)", dist, out.len());
                        for _ in 0..len {
                            out.push(out[out.len() - dist]);
                        }
                    }
                },
                t => panic!("block type {} is not written by the encoder", t),
            }
            if last == 1 {
                break;
            }
        }
        (out, (b.p + 7) / 8)
    }

    #[test]
    fn inflate_reference_streams() {
        // Python zlib, Z_FIXED, raw deflate of b"hello hello hello hello": "hello " as literals, then one match (distance 6)
        let z = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01];
        let (out, used) = inflate(&z);
        assert_eq!(out, b"hello hello hello hello");
        assert_eq!(used, z.len());
        // a stored block
        let (out, used) = inflate(&[1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c']);
        assert_eq!((out.as_slice(), used), (&b"abc"[..], 8));
    }

    /// Parses the PNG structure: signature, chunk lengths and CRCs, IHDR, the zlib stream (inflated, Adler32) and the
    /// pixels.
    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let mut i = 8;
        let (mut w, mut h, mut z) = (0, 0, Vec::new());
        let mut kinds = Vec::new();
        while i < png.len() {
            let n = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
            let kind = &png[i + 4..i + 8];
            let data = &png[i + 8..i + 8 + n];
            let crc = u32::from_be_bytes(png[i + 8 + n..i + 12 + n].try_into().unwrap());
            assert_eq!(crc, crc32(&png[i + 4..i + 8 + n]), "CRC of {:?}", std::str::from_utf8(kind));
            match kind {
                b"IHDR" => {
                    w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                    h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                    assert_eq!(&data[8..], &[8, 2, 0, 0, 0]);
                }
                b"IDAT" => z.extend_from_slice(data),
                _ => {}
            }
            kinds.push(String::from_utf8_lossy(kind).into_owned());
            i += 12 + n;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
        assert_eq!(i, png.len());
        // zlib header, deflate data, Adler32
        assert_eq!(((z[0] as u32) << 8 | z[1] as u32) % 31, 0);
        assert_eq!(z[0] & 0x0f, 8);
        assert_eq!(z[1] & 0x20, 0, "no preset dictionary");
        let (raw, used) = inflate(&z[2..]);
        let p = 2 + used;
        assert_eq!(u32::from_be_bytes(z[p..p + 4].try_into().unwrap()), adler32(&raw));
        assert_eq!(p + 4, z.len());
        // rows: filter 0 + RGB
        let mut pix = Vec::new();
        for row in raw.chunks(w as usize * 3 + 1) {
            assert_eq!(row[0], 0);
            pix.extend_from_slice(&row[1..]);
        }
        assert_eq!(pix.len(), (w * h * 3) as usize);
        (w, h, pix)
    }

    #[test]
    fn ppm_power_on_picture() {
        let mut b = b"P6\n# OOS\n2 1\n255\n".to_vec();
        b.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        let mut cv = Canvas::new(10, 4);
        assert_eq!(ppm_into(&b, &mut cv), Some((2, 1)));
        assert_eq!((cv.px[(1 * 10 + 4) as usize], cv.px[(1 * 10 + 5) as usize]), (crate::gfx::rgb(255, 0, 0), crate::gfx::rgb(0, 0, 255)));
        assert!(ppm_into(b"P5\n1 1\n255\n\0", &mut cv).is_none());
    }

    #[test]
    fn png_structure_and_pixels() {
        // larger than one stored block (65535 bytes)
        let (w, h) = (301u32, 97u32);
        let px: Vec<u32> = (0..w * h).map(|i| i.wrapping_mul(2654435761u32) & 0xff_ffff).collect();
        let data = png(w, h, &px);
        let (dw, dh, pix) = decode(&data);
        assert_eq!((dw, dh), (w, h));
        for (k, &p) in px.iter().enumerate() {
            assert_eq!(&pix[k * 3..k * 3 + 3], &[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }

    /// A picture like the scope screen: black, a grid, filled boxes, a trace, some speckle.
    fn screen_like(w: u32, h: u32) -> Vec<u32> {
        let mut px = vec![0u32; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut c = 0;
                if x % 100 == 0 || y % 75 == 0 {
                    c = rgb(60, 60, 60);
                }
                if (40..200).contains(&x) && (30..70).contains(&y) {
                    c = rgb(20, 90, 160);
                }
                let trace = (h as f32 / 2.0 + (x as f32 / 20.0).sin() * h as f32 / 4.0) as u32;
                if y == trace || y == trace + 1 {
                    c = rgb(255, 220, 0);
                }
                if (x * 7 + y * 13) % 997 == 0 {
                    c = rgb(200, 200, 200);
                }
                px[(y * w + x) as usize] = c;
            }
        }
        px
    }

    fn assert_decodes_to(data: &[u8], w: u32, h: u32, px: &[u32]) {
        let (dw, dh, pix) = decode(data);
        assert_eq!((dw, dh), (w, h));
        for (k, &p) in px.iter().enumerate() {
            assert_eq!(&pix[k * 3..k * 3 + 3], &[(p >> 16) as u8, (p >> 8) as u8, p as u8], "pixel {}", k);
        }
    }

    #[test]
    fn png_of_a_screen_round_trips() {
        // runs longer than the 258 a match can take, rows equal to the row above, matches across the end of a row
        for (w, h) in [(1024u32, 600u32), (301, 97), (1, 1), (1, 300), (2, 3), (700, 1)] {
            let px = screen_like(w, h);
            assert_decodes_to(&png(w, h, &px), w, h, &px);
        }
        let flat = vec![rgb(1, 2, 3); 50 * 40];
        assert_decodes_to(&png(50, 40, &flat), 50, 40, &flat);
    }

    #[test]
    fn png_is_much_smaller_than_the_bitmap() {
        let (w, h) = (1024u32, 600u32);
        let raw = (w * 3 + 1) * h;
        let black = png(w, h, &vec![0u32; (w * h) as usize]);
        assert!(black.len() < raw as usize / 100, "black screen: {} bytes", black.len());
        let screen = png(w, h, &screen_like(w, h));
        assert!(screen.len() < raw as usize / 20, "screen-like picture: {} bytes of {}", screen.len(), raw);
    }

    /// Cross-check with an independent decoder when the host has Python with Pillow (skipped otherwise).
    #[test]
    fn png_opens_in_pillow() {
        let has_pillow = std::process::Command::new("python3").args(["-c", "import PIL"]).output().map_or(false, |o| o.status.success());
        if !has_pillow {
            return eprintln!("python3 with Pillow not found: check skipped");
        }
        let (w, h) = (64u32, 40u32);
        let px: Vec<u32> = (0..w * h).map(|i| rgb((i % w) * 4, (i / w) * 6, 0x80)).collect();
        let dir = std::env::temp_dir().join(format!("oos-ui-png-{}", std::process::id()));
        let path = dir.join("t.png");
        write(&path, &png(w, h, &px)).unwrap();
        // a full screen with long runs and repeated rows, compared byte for byte with what Pillow decodes
        let (sw, sh) = (1024u32, 600u32);
        let screen = screen_like(sw, sh);
        let big = dir.join("big.png");
        write(&big, &png(sw, sh, &screen)).unwrap();
        let rgb_bytes: Vec<u8> = screen.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
        write(&dir.join("big.rgb"), &rgb_bytes).unwrap();
        let script = "import sys\nfrom PIL import Image\nim = Image.open(sys.argv[1]); im.load()\nprint(im.size[0], im.size[1], im.mode, *im.getpixel((5, 7)))\nbig = Image.open(sys.argv[2]); big.load()\nprint(big.tobytes() == open(sys.argv[3], 'rb').read())";
        let out = std::process::Command::new("python3").arg("-c").arg(script).arg(&path).arg(&big).arg(dir.join("big.rgb")).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "Pillow: {}", String::from_utf8_lossy(&out.stderr));
        let s = String::from_utf8_lossy(&out.stdout);
        assert_eq!(s.trim(), format!("64 40 RGB {} {} 128\nTrue", 5 * 4, 7 * 6));
    }

    /// A real screen (the demo, four channels and the menus of the UI) decodes back to its pixels.
    #[test]
    fn png_of_the_demo_screen_round_trips() {
        let mut ui = crate::ui::Ui::new(crate::wave::Source::Demo { t: 0.0 });
        for c in ["key ch2", "key ch3", "key ch4", "key measure"] {
            crate::cmd::run(&mut ui, c);
        }
        let mut cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
        for _ in 0..3 {
            ui.step(0.04);
            ui.render(&mut cv);
        }
        assert_decodes_to(&png_of(&cv), cv.w as u32, cv.h as u32, &cv.px);
    }

    /// Time of one PNG of a real screen (the demo source, 4 channels on, as the web remote sends it) and of the
    /// screen-like test picture: `cargo test --release png_encode_time -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn png_encode_time() {
        let mut ui = crate::ui::Ui::new(crate::wave::Source::Demo { t: 0.0 });
        for c in ["key ch2", "key ch3", "key ch4"] {
            crate::cmd::run(&mut ui, c);
        }
        let mut cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
        for _ in 0..10 {
            ui.step(0.04);
            ui.render(&mut cv);
        }
        let synthetic = screen_like(cv.w as u32, cv.h as u32);
        for (name, px) in [("demo screen", &cv.px), ("screen-like", &synthetic)] {
            let n = 40;
            let mut size = 0;
            let t = std::time::Instant::now();
            for _ in 0..n {
                size = std::hint::black_box(png(cv.w as u32, cv.h as u32, px)).len();
            }
            let per = t.elapsed().as_secs_f64() / n as f64;
            println!("{}: {} x {}, {} bytes, {:.2} ms per PNG", name, cv.w, cv.h, size, per * 1e3);
            assert_decodes_to(&png(cv.w as u32, cv.h as u32, px), cv.w as u32, cv.h as u32, px);
        }
        // what the frame loop does per picture of the web remote now (http.rs `Shots`): one copy of the screen; and
        // what the encoder thread does before it encodes: compare it with the last copy
        let n = 200;
        let mut copy: Vec<u32> = Vec::with_capacity(cv.px.len());
        let t = std::time::Instant::now();
        for _ in 0..n {
            copy.clear();
            copy.extend_from_slice(std::hint::black_box(&cv.px));
        }
        let per_copy = t.elapsed().as_secs_f64() / n as f64;
        let t = std::time::Instant::now();
        let mut same = false;
        for _ in 0..n {
            same = crate::http::same_pixels(std::hint::black_box(&copy), std::hint::black_box(&cv.px));
        }
        let per_cmp = t.elapsed().as_secs_f64() / n as f64;
        assert!(same);
        println!("frame loop: copy of the screen {:.3} ms; encoder thread: comparison {:.3} ms", per_copy * 1e3, per_cmp * 1e3);
    }

    #[test]
    fn next_free_name() {
        let dir = std::env::temp_dir().join(format!("oos-ui-names-{}", std::process::id()));
        assert_eq!(file_name(&next_name(&dir, "shot", "png")), "shot-001.png");
        write(&dir.join("shot-001.png"), b"x").unwrap();
        write(&dir.join("shot-002.png"), b"x").unwrap();
        assert_eq!(file_name(&next_name(&dir, "shot", "png")), "shot-003.png");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
