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

/// Where CSV, PNG, mask and fast-frame files go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreDest {
    /// USB stick when mounted, otherwise the storage directory
    Auto,
    /// USB stick (`/mnt/usb/OOS`)
    Stick,
    /// Desktop folder of the logged-in user
    Desk,
}

impl StoreDest {
    pub const ALL: [StoreDest; 3] = [StoreDest::Auto, StoreDest::Stick, StoreDest::Desk];

    pub fn name(self) -> &'static str {
        match self {
            StoreDest::Auto => "Auto",
            StoreDest::Stick => "USB",
            StoreDest::Desk => "Desktop",
        }
    }
}

/// Desktop save folder (`$HOME/Desktop/OOS`, else `/root/Desktop/OOS`).
pub fn desktop_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join("Desktop").join("OOS")
}

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
    kv("dec_proto", name(m.dec_proto));
    kv("dec_src2", m.dec_src2.to_string());
    kv("dec_cs", m.dec_cs.map_or("none".into(), |c| c.to_string()));
    kv("dec_mode", m.dec_mode.to_string());
    kv("dec_bits", m.dec_bits.to_string());
    kv("dec_parity", name(m.dec_parity));
}

/// The one bus on the plot.
fn bus_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    let b = &m.bus;
    kv("bus_on", b.on.to_string());
    kv("bus_label", b.label.clone());
    kv("bus_pos", b.pos.to_string());
    kv("bus_kind", name(b.kind));
    kv("bus_clk", b.clk.token());
    kv("bus_data", b.data.token());
    kv("bus_clk_thr", b.clk_thr.to_string());
    kv("bus_data_thr", b.data_thr.to_string());
    kv("bus_rw", b.addr_rw.to_string());
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
    if let Some(c) = m.cur_b_ch {
        kv("cur_b_ch", c.to_string());
    }
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
    kv("density", name(m.density));
    kv("split_pane", m.split_pane.to_string());
    kv("ch_window", m.ch_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("math_window", m.math_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("la_window", (m.la_win + 1).to_string());
    kv("meas", m.meas.iter().map(|&(k, id)| format!("{:?}:{}", k, id.token())).collect::<Vec<_>>().join(" "));
    kv("meas2", m.meas2.iter().map(|&(k, a, b)| format!("{}:{}:{}", k, a.token(), b.token())).collect::<Vec<_>>().join(" "));
    if !m.cat.is_empty() {
        kv("cat", m.cat.iter().map(|&(k, a, b, c, d)| format!("{}:{}:{}:{}:{}", k, a.token(), b.token(), c.token(), d.token())).collect::<Vec<_>>().join(" "));
    }
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
    bus_lines(m, &mut kv);
    for k in (0..DECODERS).filter(|&k| k != m.dec_sel && m.decs[k].is_some()) {
        dec_lines(&m.dec_view(k), &mut |n: &str, v: String| kv(&format!("d{}.{}", k + 1, n), v));
    }
    kv("pane", m.pane.to_string());
    kv("meas_labels", m.meas_labels.to_string());
    kv("strip_dock", name(m.strip_dock));
    kv("trend_sel", format!("{} {}", m.trend_sel[0], m.trend_sel[1]));
    kv("zoom", m.zoom.to_string());
    kv("auto_roll", m.auto_roll.to_string());
    kv("zoom_tdiv", tscales()[m.zscale].to_string());
    kv("zdelay", m.zdelay.to_string());
    kv("color_grade", m.color_grade.to_string());
    kv("dots", m.dots.to_string());
    kv("hdmi_wave", m.hdmi_wave.to_string());
    kv("view", name(m.view));
    kv("bode", format!("{} {} {} {} {} {} {} {}", m.bode_start, m.bode_stop, m.bode_points, m.bode_amp, m.bode_in.token(), m.bode_out.token(), m.bode_gen, m.bode_addr));
    kv("spec", format!("{} {} {} {} {} {} {} {} {}", m.spec_src.token(), m.spec_center, m.spec_span, m.spec_rbw, name(m.spec_win), m.spec_trace, m.spec_ref, m.spec_wf, m.spec_dist));
    kv("spec_peaks", format!("{} {}", m.spec_sort, m.spec_harm));
    kv("eye", format!("{} {} {}", m.eye_src.token(), m.eye_rate, if m.eye_from_record { 1 } else { 0 }));
    kv("eye_place", name(m.eye_place));
    kv("power", format!("{} {} {}", m.pw_v.token(), m.pw_i.token(), name(m.pw_mode)));
    let opt = |id: Option<WaveId>| id.map(|w| w.token()).unwrap_or_else(|| "-".into());
    kv("meas_cfg", format!("{} {:?} {} {} {} {} {} {}", m.meas_idle, m.meas_out, m.meas_n, if m.meas_data { 1 } else { 0 }, m.meas_lpct, m.meas_hpct, m.meas_tsetup, m.meas_thold));
    kv("spec_acpr", format!("{} {}", m.spec_acpr_w, m.spec_acpr_s));
    kv("bode_mode", name(m.bode_mode));
    kv("eye_more", format!("{} {} {} {} {} {} {} {}", m.eye_ber, m.eye_pn_lo, m.eye_pn_hi, if m.eye_npj { 1 } else { 0 }, opt(m.eye_rail), m.eye_notch, opt(m.eye_b), if m.eye_hp { 1 } else { 0 }));
    kv("power_more", format!("{} {} {} {} {}", m.pw_fline, m.pw_fsw, opt(m.pw_vin), m.pw_ref.map(|v| v.to_string()).unwrap_or_else(|| "-".into()), opt(m.pw_clk)));
    kv("magnetic", format!("{} {} {} {} {} {} {:?} {}", m.mag_v.token(), m.mag_i.token(), m.mag_n, m.mag_ae, m.mag_le, m.mag_rcu, m.mag_plot, m.mag_cur));
    let opt = |c: Option<WaveId>| c.map_or("off".into(), |c| c.token());
    let line = |c: Option<u8>| c.map_or("off".into(), |c| c.to_string());
    kv("inverter", format!("{:?} {} {} {} {:?} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}", m.inv_page, m.inv_a.token(), m.inv_b.token(), m.inv_c.token(), m.inv_ang, m.inv_poles, m.inv_an.token(), m.inv_volt, opt(m.inv_tq), opt(m.inv_ev), opt(m.inv_ei), m.inv_scale, m.inv_degv, m.inv_off, m.inv_elec, m.inv_qa, m.inv_qb, line(m.inv_qi), m.inv_cpr, m.inv_swap, m.inv_ha, m.inv_hb, m.inv_hc));
    kv("dpt", format!("{} {} {} {:?} {} {} {:?}", m.dpt_v.token(), m.dpt_i.token(), m.dpt_g.token(), m.dpt_4th, m.dpt_4ch.token(), m.dpt_pct, m.dpt_zoom));
    kv("sim", format!("{} {} {} {} {} {} {} {} {} {} {} {} {} {} {}", m.sim_preset, m.sim_slot, m.sim_rate, m.sim_prbs, m.sim_vhigh, m.sim_vlow, m.sim_rise, m.sim_noise, m.sim_bw, m.sim_deemph, if m.sim_ssc { 1 } else { 0 }, m.sim_rj, m.sim_pj, m.sim_pjhz, 0));
    kv("sim_extra", format!("{} {} {} {} {} {}", m.sim_ssc_center, m.sim_ssc_deviation, m.sim_ssc_hz, m.sim_dcd_pp, m.sim_corr_rj, m.sim_corr_hz));
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
    kv("mask_inv", m.mask_inv.to_string());
    kv("quick", name(m.quick));
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

/// Old setup files stored an index into the 5 ns .. 1000 s ladder.
fn legacy_tscale(v: &str, ntime: usize) -> Result<usize, String> {
    let five = tscales().iter().position(|&t| t >= 4.9e-9).unwrap_or(0);
    Ok(five.saturating_add(p_idx(v, ntime.saturating_sub(five).max(1))?))
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
        "cur_b_ch" => m.cur_b_ch = Some(p_idx(v, NCH)?),
        "tdiv" => m.tscale = tdiv_index(v)?,
        // setups before the 1/2 ns steps: index into the old ladder (5 ns .. 1000 s)
        "tscale" => m.tscale = legacy_tscale(v, ntime)?,
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
        "density" => m.density = p_enum(v, &Density::ALL)?,
        "split_pane" => m.split_pane = p_idx(v, crate::ui::PANES_N)?,
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
                out.push((p_enum(k, &ALL_MEAS)?, WaveId::parse(c)?));
            }
            m.meas = out;
        }
        "meas2" => {
            let mut out = Vec::new();
            for e in v.split_whitespace() {
                let f: Vec<&str> = e.split(':').collect();
                let [k, a, b] = f[..] else { return Err(format!("not item:a:b: {}", e)) };
                out.push((p_idx(k, crate::ui::PAIRS_N)?, WaveId::parse(a)?, WaveId::parse(b)?));
            }
            m.meas2 = out;
        }
        "cat" => {
            let mut out = Vec::new();
            for e in v.split_whitespace() {
                let f: Vec<&str> = e.split(':').collect();
                let [k, a, b, c, d] = f[..] else { return Err(format!("not item:a:b:c:d: {}", e)) };
                let id = k.parse::<u16>().map_err(|_| format!("not a catalog id: {}", k))?;
                out.push((id, WaveId::parse(a)?, WaveId::parse(b)?, WaveId::parse(c)?, WaveId::parse(d)?));
            }
            m.cat = out;
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
        "strip_dock" => m.strip_dock = p_enum(v, &StripDock::ALL)?,
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
        "dec_proto" => m.dec_proto = p_enum(v, &PROTOS)?,
        "dec_src2" => m.dec_src2 = p_idx(v, NCH)?,
        "dec_cs" => m.dec_cs = if v == "none" { None } else { Some(p_idx(v, NCH)?) },
        "dec_mode" => m.dec_mode = p_idx(v, 4)? as u8,
        "dec_bits" => m.dec_bits = p_u32(v)?.clamp(4, 32),
        "dec_parity" => m.dec_parity = p_enum(v, &PARITIES)?,
        "zoom" => m.zoom = p_bool(v)?,
        "auto_roll" => m.auto_roll = p_bool(v)?,
        "zoom_tdiv" => m.zscale = tdiv_index(v)?,
        "zscale" => m.zscale = legacy_tscale(v, ntime)?,
        "zdelay" => m.zdelay = p_f32(v)?,
        "color_grade" => m.color_grade = p_bool(v)?,
        "dots" => m.dots = p_bool(v)?,
        "hdmi_wave" => m.hdmi_wave = p_bool(v)?,
        "view" => m.view = p_enum(v, &[AView::Scope, AView::Spectrum, AView::Eye, AView::Power, AView::Bode, AView::Magnetic, AView::Inverter, AView::Dpt])?,
        "bode" => {
            // the address is the rest of the line: it may be empty
            let w: Vec<&str> = v.trim().splitn(8, char::is_whitespace).collect();
            if w.len() < 7 {
                return Err("bode: want 8 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            (m.bode_start, m.bode_stop, m.bode_amp) = (f(w[0])?, f(w[1])?, f(w[3])?);
            m.bode_points = p_u32(w[2])?.clamp(2, 1000) as usize;
            m.bode_in = WaveId::parse(w[4])?;
            m.bode_out = WaveId::parse(w[5])?;
            m.bode_gen = p_idx(w[6], crate::ui::bode::GENERATORS.len())?;
            m.bode_addr = w.get(7).map_or("", |a| a.trim()).to_string();
        }
        "spec" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 9 {
                return Err("spec: want 9 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            m.spec_src = WaveId::parse(w[0])?;
            (m.spec_center, m.spec_span, m.spec_rbw) = (f(w[1])?, f(w[2])?, f(w[3])?);
            m.spec_win = p_enum(w[4], &WINDOWS)?;
            m.spec_trace = p_idx(w[5], 4)?;
            m.spec_ref = p_f32(w[6])?;
            (m.spec_wf, m.spec_dist) = (p_bool(w[7])?, p_bool(w[8])?);
        }
        "eye" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            m.eye_src = WaveId::parse(w.first().ok_or("eye: source")?)?;
            m.eye_rate = w.get(1).ok_or("eye: rate")?.parse::<f64>().map_err(|e| e.to_string())?;
            // older files have no third field: the rate was entered, not measured
            m.eye_from_record = w.get(2).is_some_and(|s| *s != "0");
        }
        "eye_place" => m.eye_place = p_enum(v, &[EyePlace::Full, EyePlace::Side, EyePlace::Under])?,
        "power" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 3 {
                return Err("power: want 3 values".into());
            }
            m.pw_v = WaveId::parse(w[0])?;
            m.pw_i = WaveId::parse(w[1])?;
            m.pw_mode = p_enum(w[2], &[PowerMode::Quality, PowerMode::Harmonics, PowerMode::Switching, PowerMode::Ripple, PowerMode::Capacitance, PowerMode::Rail])?;
        }
        "magnetic" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 8 {
                return Err("magnetic: want 8 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            m.mag_v = WaveId::parse(w[0])?;
            m.mag_i = WaveId::parse(w[1])?;
            m.mag_n = f(w[2])?.clamp(1.0, 10_000.0);
            m.mag_ae = f(w[3])?.max(0.0);
            m.mag_le = f(w[4])?.max(0.0);
            m.mag_rcu = f(w[5])?.max(0.0);
            m.mag_plot = p_enum(w[6], &[MagPlot::IPsi, MagPlot::BH])?;
            m.mag_cur = w[7].parse::<usize>().map_err(|e| e.to_string())?;
        }
        "inverter" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 23 {
                return Err("inverter: want 23 values".into());
            }
            let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
            let ch = |s: &str| WaveId::parse(s);
            let opt = |s: &str| -> Result<Option<WaveId>, String> { if s == "off" { Ok(None) } else { Ok(Some(ch(s)?)) } };
            let bit = |s: &str| -> Result<u8, String> {
                let n = s.parse::<u8>().map_err(|e| e.to_string())?;
                if n < 16 { Ok(n) } else { Err(format!("logic line {} out of range", n)) }
            };
            m.inv_page = p_enum(w[0], &[InvPage::Dq0, InvPage::Mech])?;
            (m.inv_a, m.inv_b, m.inv_c) = (ch(w[1])?, ch(w[2])?, ch(w[3])?);
            m.inv_ang = p_enum(w[4], &[InvAngle::Off, InvAngle::Phase, InvAngle::Analog, InvAngle::Quad, InvAngle::Hall])?;
            m.inv_poles = p_u32(w[5])?.clamp(1, 32);
            m.inv_an = ch(w[6])?;
            m.inv_volt = p_bool(w[7])?;
            (m.inv_tq, m.inv_ev, m.inv_ei) = (opt(w[8])?, opt(w[9])?, opt(w[10])?);
            (m.inv_scale, m.inv_degv, m.inv_off) = (f(w[11])?, f(w[12])?, f(w[13])?);
            m.inv_elec = p_bool(w[14])?;
            (m.inv_qa, m.inv_qb) = (bit(w[15])?, bit(w[16])?);
            m.inv_qi = if w[17] == "off" { None } else { Some(bit(w[17])?) };
            m.inv_cpr = f(w[18])?.clamp(1.0, 100_000.0);
            m.inv_swap = p_bool(w[19])?;
            (m.inv_ha, m.inv_hb, m.inv_hc) = (bit(w[20])?, bit(w[21])?, bit(w[22])?);
        }
        "dpt" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 7 {
                return Err("dpt: want 7 values".into());
            }
            m.dpt_v = WaveId::parse(w[0])?;
            m.dpt_i = WaveId::parse(w[1])?;
            m.dpt_g = WaveId::parse(w[2])?;
            m.dpt_4th = p_enum(w[3], &[DptFourth::Off, DptFourth::Gate, DptFourth::DiodeV, DptFourth::DiodeI])?;
            m.dpt_4ch = WaveId::parse(w[4])?;
            m.dpt_pct = w[5].parse::<f64>().map_err(|e| e.to_string())?.clamp(1.0, 49.0);
            m.dpt_zoom = p_enum(w[6], &[DptZoom::Both, DptZoom::TurnOn, DptZoom::TurnOff])?;
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
        "mask_inv" => m.mask_inv = p_bool(v)?,
        "quick" => m.quick = p_enum(v, &QUICKS)?,
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
        "meas_cfg" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 4 && w.len() != 8 {
                return Err("meas_cfg: idle outside n data [lpct hpct tsetup thold]".into());
            }
            m.meas_idle = w[0].parse::<f64>().map_err(|e| e.to_string())?;
            m.meas_out = p_enum(w[1], &[oos_dsp::measure::Outside::Above, oos_dsp::measure::Outside::Below, oos_dsp::measure::Outside::Both])?;
            m.meas_n = p_u32(w[2])?.clamp(1, 32);
            m.meas_data = p_bool(w[3])?;
            if w.len() == 8 {
                m.meas_lpct = w[4].parse::<f64>().map_err(|e| e.to_string())?;
                m.meas_hpct = w[5].parse::<f64>().map_err(|e| e.to_string())?;
                m.meas_tsetup = w[6].parse::<f64>().map_err(|e| e.to_string())?;
                m.meas_thold = w[7].parse::<f64>().map_err(|e| e.to_string())?;
            }
        }
        "spec_acpr" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            let [width, space] = w[..] else { return Err("spec_acpr: width spacing".into()) };
            m.spec_acpr_w = width.parse::<f64>().map_err(|e| e.to_string())?;
            m.spec_acpr_s = space.parse::<f64>().map_err(|e| e.to_string())?;
        }
        "bode_mode" => m.bode_mode = p_enum(v, &[oos_dsp::bode::Mode::Loop, oos_dsp::bode::Mode::Psrr, oos_dsp::bode::Mode::Z])?,
        "sim" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() < 2 {
                return Err("sim: preset slot".into());
            }
            m.sim_preset = p_idx(w[0], oos_dsp::serial::LANES.len())?;
            m.sim_slot = p_idx(w[1], NREF)?;
            m.apply_sim_preset();
            if w.len() >= 14 {
                let f = |s: &str| s.parse::<f64>().map_err(|e| e.to_string());
                m.sim_rate = f(w[2])?;
                m.sim_prbs = w[3].parse::<u8>().map_err(|e| e.to_string())?;
                m.sim_vhigh = f(w[4])?;
                m.sim_vlow = f(w[5])?;
                m.sim_rise = f(w[6])?;
                m.sim_noise = f(w[7])?;
                m.sim_bw = f(w[8])?;
                m.sim_deemph = f(w[9])?;
                m.sim_ssc = p_bool(w[10])?;
                m.sim_rj = f(w[11])?;
                m.sim_pj = f(w[12])?;
                m.sim_pjhz = f(w[13])?;
            }
        }
        "sim_extra" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            let [center, deviation, hz, dcd, corr, corner] = w[..] else {
                return Err("sim_extra: center deviation frequency DCD correlated-RJ corner".into());
            };
            let bounded = |s: &str, low: f64, high: f64| -> Result<f64, String> {
                s.parse::<f64>().ok().filter(|value| value.is_finite() && *value >= low && *value <= high)
                    .ok_or_else(|| format!("{s} must be between {low} and {high}"))
            };
            m.sim_ssc_center = p_bool(center)?;
            m.sim_ssc_deviation = bounded(deviation, 0.0, 0.05)?;
            m.sim_ssc_hz = bounded(hz, 1e3, 1e6)?;
            m.sim_dcd_pp = bounded(dcd, 0.0, 100e-12)?;
            m.sim_corr_rj = bounded(corr, 0.0, 50e-12)?;
            m.sim_corr_hz = bounded(corner, 1e3, 100e6)?;
        }
        "eye_more" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            let [ber, lo, hi, npj, rail, notch, b, hp] = w[..] else { return Err("eye_more".into()) };
            let opt = |s: &str| if s == "-" { Ok(None) } else { WaveId::parse(s).map(Some) };
            m.eye_ber = ber.parse::<f64>().map_err(|e| e.to_string())?;
            m.eye_pn_lo = lo.parse::<f64>().map_err(|e| e.to_string())?;
            m.eye_pn_hi = hi.parse::<f64>().map_err(|e| e.to_string())?;
            m.eye_npj = p_bool(npj)?;
            m.eye_rail = opt(rail)?;
            m.eye_notch = notch.parse::<f64>().map_err(|e| e.to_string())?;
            m.eye_b = opt(b)?;
            m.eye_hp = p_bool(hp)?;
        }
        "bus_on" => m.bus.on = p_bool(v)?,
        "bus_label" => m.bus.label = crate::model::bus_label(v),
        "bus_pos" => m.bus.pos = p_f32(v)?.clamp(BUS_POS_MIN, BUS_POS_MAX),
        "bus_kind" => m.bus.kind = p_enum(v, &[BusKind::I2c])?,
        "bus_clk" => m.bus.clk = WaveId::parse(v)?,
        "bus_data" => m.bus.data = WaveId::parse(v)?,
        "bus_clk_thr" => m.bus.clk_thr = p_f32(v)?.clamp(-40.0, 40.0),
        "bus_data_thr" => m.bus.data_thr = p_f32(v)?.clamp(-40.0, 40.0),
        "bus_rw" => m.bus.addr_rw = p_bool(v)?,
        "power_more" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            let [fline, fsw, vin, reference, clk] = w[..] else { return Err("power_more".into()) };
            let opt = |s: &str| if s == "-" { Ok(None) } else { WaveId::parse(s).map(Some) };
            m.pw_fline = fline.parse::<f64>().map_err(|e| e.to_string())?;
            m.pw_fsw = fsw.parse::<f64>().map_err(|e| e.to_string())?;
            m.pw_vin = opt(vin)?;
            m.pw_ref = if reference == "-" { None } else { Some(reference.parse::<f64>().map_err(|e| e.to_string())?) };
            m.pw_clk = opt(clk)?;
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
    // a setup from before the density row: the windows and the pane already say which of the four it was
    if !text.lines().any(|l| l.split('=').next().is_some_and(|k| k.trim() == "density")) {
        m.density = match m.win_layout {
            WinLayout::Quad => Density::Four,
            WinLayout::Cols | WinLayout::Rows => Density::Split,
            WinLayout::One if m.pane != 0 => Density::Split,
            WinLayout::One => Density::Scope,
        };
        // two across is not a density; the split is two stacked traces
        if m.win_layout == WinLayout::Cols {
            m.win_layout = WinLayout::Rows;
        }
        if m.density == Density::Split {
            m.split_pane = m.pane;
        }
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

/// One trace taken from a file: volts, the sample step, and the time of the first sample.
#[derive(Clone, Debug)]
pub struct WaveTrace {
    pub t0: f64,
    pub dt: f64,
    pub v: Vec<f32>,
    pub label: String,
}

/// CSV (`Time (s),Volts` or Storage `Time (s),CH1 (V),…`), an OOS `OOSW` `.bin`, or a `.wfm` whose header is `:WFM#002` or `:WFM#003`.
pub fn parse_wave(bytes: &[u8], name: &str) -> Result<Vec<WaveTrace>, String> {
    if bytes.starts_with(b"OOSW") {
        return parse_oosw(bytes);
    }
    if bytes.len() >= 10 && (bytes[2..].starts_with(b":WFM#") || bytes[2..].starts_with(b"WFM#") || bytes.starts_with(b":WFM#")) {
        return parse_wfm(bytes).map(|t| vec![t]);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "that file is not a waveform".to_string())?;
    parse_wave_csv(text, name)
}

fn parse_oosw(b: &[u8]) -> Result<Vec<WaveTrace>, String> {
    if b.len() < 36 || &b[..4] != b"OOSW" {
        return Err("not an OOS waveform".into());
    }
    let ver = u32::from_le_bytes(b[4..8].try_into().unwrap());
    if ver != 1 {
        return Err(format!("OOS waveform version {ver}"));
    }
    let nch = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let n = usize::try_from(u64::from_le_bytes(b[12..20].try_into().unwrap())).map_err(|_| "OOS waveform is too large".to_string())?;
    let dt = f64::from_le_bytes(b[20..28].try_into().unwrap());
    let t0 = f64::from_le_bytes(b[28..36].try_into().unwrap());
    if nch == 0 || nch > 8 || n < 2 || !dt.is_finite() || dt <= 0.0 || !t0.is_finite() {
        return Err("that file holds no waveform".into());
    }
    let need = n.checked_mul(4).and_then(|bytes| bytes.checked_add(4)).and_then(|bytes| bytes.checked_mul(nch)).and_then(|bytes| bytes.checked_add(36)).ok_or("OOS waveform is too large")?;
    if b.len() < need {
        return Err("truncated OOS waveform".into());
    }
    let mut out = Vec::with_capacity(nch);
    let mut p = 36;
    for _ in 0..nch {
        let ch = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        p += 4;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(f32::from_le_bytes(b[p..p + 4].try_into().unwrap()));
            p += 4;
        }
        out.push(WaveTrace { t0, dt, v, label: format!("CH{ch}") });
    }
    Ok(out)
}

fn parse_wave_csv(text: &str, name: &str) -> Result<Vec<WaveTrace>, String> {
    let mut rows: Vec<(f64, Vec<f64>)> = Vec::new();
    for line in text.lines() {
        let mut cols = line.split(',').map(|s| s.trim().parse::<f64>().ok());
        let Some(Some(t)) = cols.next() else { continue };
        let rest: Vec<f64> = cols.map(|x| x.unwrap_or(f64::NAN)).collect();
        if rest.is_empty() {
            continue;
        }
        rows.push((t, rest));
    }
    let dt = match rows.as_slice() {
        [a, b, ..] if b.0 > a.0 => b.0 - a.0,
        _ => return Err("that file holds no waveform".into()),
    };
    let ntrace = rows.iter().map(|r| r.1.len()).max().unwrap_or(0).max(1);
    let mut traces = Vec::with_capacity(ntrace);
    for k in 0..ntrace {
        let v: Vec<f32> = rows.iter().map(|r| r.1.get(k).copied().unwrap_or(f64::NAN) as f32).collect();
        let label = if ntrace == 1 { name.into() } else { format!("CH{}", k + 1) };
        traces.push(WaveTrace { t0: rows[0].0, dt, v, label });
    }
    Ok(traces)
}

/// Reference waveform whose header is `:WFM#002` or `:WFM#003`, YT vector, first frame.
fn parse_wfm(b: &[u8]) -> Result<WaveTrace, String> {
    if b.len() < 78 {
        return Err("truncated WFM".into());
    }
    let le = match u16::from_le_bytes(b[0..2].try_into().unwrap()) {
        0x0F0F => true,
        0xF0F0 => false,
        _ => return Err("header is not :WFM#002 or :WFM#003".into()),
    };
    let get_u32 = |o| {
        let v = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        if le { v } else { v.swap_bytes() }
    };
    let get_i32 = |o| get_u32(o) as i32;
    let get_f64 = |o| {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&b[o..o + 8]);
        if !le {
            raw.reverse();
        }
        f64::from_le_bytes(raw)
    };
    let ver = &b[2..10];
    if ver != b":WFM#002" && ver != b":WFM#003" {
        return Err("header is not :WFM#002 or :WFM#003".into());
    }
    let bpp = b[15] as usize;
    if bpp == 0 || bpp > 8 {
        return Err("WFM sample size".into());
    }
    let curve = get_i32(16) as usize;
    let label = b[40..72].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect::<String>().trim().to_string();
    // :WFM#002 / :WFM#003: packed header after the 78-byte static block
    let mut i = 78usize;
    let need = |n: usize| -> Result<(), String> { if b.len() < n { Err("truncated WFM".into()) } else { Ok(()) } };
    need(i + 90)?;
    i += 90; // reference file data
    need(i + 160)?;
    let vscale = get_f64(i);
    let voff = get_f64(i + 8);
    let fmt = get_u32(i + 72); // after scale, offset, size, units, 4 doubles
    i += 160; // exp dim 1
    need(i + 160)?;
    i += 160; // exp dim 2
    need(i + 136)?;
    let dt = get_f64(i);
    let t0 = get_f64(i + 8);
    i += 136; // imp dim 1 → imp dim 2
    need(i + 136 + 12 + 12 + 24 + 30)?;
    i += 136 + 12 + 12 + 24; // imp dim 2, time bases, update spec → curve spec
    let data_start = get_u32(i + 14) as usize;
    let post_start = get_u32(i + 18) as usize;
    if !(dt > 0.0) {
        return Err("WFM has no time step".into());
    }
    if curve > b.len() || data_start > post_start {
        return Err("WFM curve is empty".into());
    }
    let start = curve.saturating_add(data_start);
    let stop = curve.saturating_add(post_start).min(b.len().saturating_sub(8));
    if stop <= start + bpp {
        return Err("WFM curve is empty".into());
    }
    let n = (stop - start) / bpp;
    if n < 2 {
        return Err("WFM curve is empty".into());
    }
    let mut v = Vec::with_capacity(n);
    for k in 0..n {
        let o = start + k * bpp;
        let code = match fmt {
            0 if bpp >= 2 => {
                let s = i16::from_le_bytes(b[o..o + 2].try_into().unwrap());
                (if le { s } else { s.swap_bytes() }) as f64
            }
            1 if bpp >= 4 => {
                let s = i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                (if le { s } else { s.swap_bytes() }) as f64
            }
            4 if bpp >= 4 => {
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&b[o..o + 4]);
                if !le {
                    raw.reverse();
                }
                f32::from_le_bytes(raw) as f64
            }
            5 if bpp >= 8 => get_f64(o),
            6 if bpp >= 1 => b[o] as f64,
            7 if bpp >= 1 => b[o] as i8 as f64,
            _ => return Err("WFM sample format".into()),
        };
        v.push((code * vscale + voff) as f32);
    }
    Ok(WaveTrace { t0, dt, v, label })
}

fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)?.try_into().ok().map(u32::from_le_bytes)
}

fn u64_le(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8)?.try_into().ok().map(u64::from_le_bytes)
}

fn f64_le(b: &[u8], at: usize) -> Option<f64> {
    b.get(at..at + 8)?.try_into().ok().map(f64::from_le_bytes)
}

fn f32_le(b: &[u8], at: usize) -> Option<f32> {
    b.get(at..at + 4)?.try_into().ok().map(f32::from_le_bytes)
}

/// One OOSW blob: the record and the number of bytes consumed.
pub fn parse_wave_bin(b: &[u8]) -> Result<(Record, usize), String> {
    if b.len() < 36 || &b[..4] != b"OOSW" {
        return Err("not an OOS waveform file".into());
    }
    let ver = u32_le(b, 4).unwrap_or(0);
    if ver != 1 {
        return Err(format!("waveform version {}", ver));
    }
    let nch = u32_le(b, 8).unwrap_or(0) as usize;
    let n = u64_le(b, 12).unwrap_or(0) as usize;
    let dt = f64_le(b, 20).unwrap_or(0.0);
    let t0 = f64_le(b, 28).unwrap_or(0.0);
    if nch == 0 || nch > NCH || dt <= 0.0 {
        return Err("waveform header".into());
    }
    let mut off = 36;
    let mut rec = Record { t0, dt, ch: Default::default(), triggered: true, partial: false, raw: None, la: None };
    for _ in 0..nch {
        let c = u32_le(b, off).ok_or("truncated waveform")? as usize;
        off += 4;
        if c < 1 || c > NCH {
            return Err(format!("channel {}", c));
        }
        if b.len() < off + 4 * n {
            return Err("truncated waveform".into());
        }
        let mut v = vec![0f32; n];
        for i in 0..n {
            v[i] = f32_le(b, off + 4 * i).unwrap_or(f32::NAN);
        }
        rec.ch[c - 1] = v;
        off += 4 * n;
    }
    Ok((rec, off))
}

/// Fast-frame file: "OOSF", u32 version 1, u32 frame count, then that many OOSW blobs.
pub fn ff_bin(recs: &[Record], m: &Model) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"OOSF");
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&(recs.len() as u32).to_le_bytes());
    for rec in recs {
        b.extend_from_slice(&wave_bin(rec, m));
    }
    b
}

pub fn parse_ff(b: &[u8]) -> Result<Vec<Record>, String> {
    if b.len() >= 4 && &b[..4] == b"OOSW" {
        return parse_wave_bin(b).map(|(r, _)| vec![r]);
    }
    if b.len() < 12 || &b[..4] != b"OOSF" {
        return Err("not a fast-frame file".into());
    }
    let ver = u32_le(b, 4).unwrap_or(0);
    if ver != 1 {
        return Err(format!("fast-frame version {}", ver));
    }
    let n = u32_le(b, 8).unwrap_or(0) as usize;
    let mut off = 12;
    let mut recs = Vec::with_capacity(n);
    for _ in 0..n {
        let (r, used) = parse_wave_bin(b.get(off..).unwrap_or(&[]))?;
        recs.push(r);
        off += used;
    }
    Ok(recs)
}

/// Highest numbered `<prefix>-NNN.<ext>` in `dir`, if any.
pub fn latest_name(dir: &Path, prefix: &str, ext: &str) -> Option<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let s = e.file_name().to_string_lossy().into_owned();
        let want = format!("{}-", prefix);
        if let Some(rest) = s.strip_prefix(&want).and_then(|r| r.strip_suffix(&format!(".{}", ext))) {
            if let Ok(n) = rest.parse::<u32>() {
                if best.as_ref().map_or(true, |(b, _)| n >= *b) {
                    best = Some((n, e.path()));
                }
            }
        }
    }
    best.map(|x| x.1)
}

/// Mask file: optional header (`# OOS mask` then `key=value` lines and `---`) then one volt per line.
pub struct MaskFile {
    pub src: Option<usize>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub inv: Option<bool>,
    pub v: Vec<f32>,
}

pub fn mask_text(src: usize, x: f32, y: f32, inv: bool, v: &[f32]) -> String {
    let mut s = String::from("# OOS mask\n");
    s.push_str(&format!("src={}\n", src + 1));
    s.push_str(&format!("x={}\n", x));
    s.push_str(&format!("y={}\n", y));
    s.push_str(&format!("inv={}\n", if inv { 1 } else { 0 }));
    s.push_str("---\n");
    for x in v {
        s.push_str(&format!("{:.9e}\n", x));
    }
    s
}

pub fn parse_mask(text: &str) -> Result<MaskFile, String> {
    let mut lines = text.lines().peekable();
    let mut src = None;
    let mut x = None;
    let mut y = None;
    let mut inv = None;
    if lines.peek().map_or(false, |l| l.starts_with("# OOS mask")) {
        lines.next();
        for line in lines.by_ref() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "---" {
                break;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("mask header: {}", line));
            };
            match k.trim() {
                "src" => {
                    let n: usize = v.trim().parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
                    if n < 1 || n > NCH {
                        return Err(format!("mask src {}", n));
                    }
                    src = Some(n - 1);
                }
                "x" => x = Some(v.trim().parse().map_err(|e: std::num::ParseFloatError| e.to_string())?),
                "y" => y = Some(v.trim().parse().map_err(|e: std::num::ParseFloatError| e.to_string())?),
                "inv" => inv = Some(matches!(v.trim(), "1" | "true" | "on")),
                _ => {}
            }
        }
    }
    let mut v = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        v.push(line.parse::<f32>().map_err(|e| e.to_string())?);
    }
    if v.is_empty() {
        return Err("empty mask".into());
    }
    Ok(MaskFile { src, x, y, inv, v })
}

// ---------------------------------------------------------------------------------------------- PNG

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    })
}

/// CRC-32 (ISO 3309 / PNG / zip).
pub fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    !data.iter().fold(!0u32, |c, &b| t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// Adler-32 (zlib).
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
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
    acc: u32,
    n: u32,
}

impl BitOut {
    fn bits(&mut self, v: u32, n: u32) {
        self.acc |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
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
    fn matched(&mut self, len: usize, dist_code: usize, dist_extra: u32) {
        let i = LEN_BASE.iter().rposition(|&(b, _)| b as usize <= len).unwrap();
        self.symbol(257 + i as u32);
        self.bits((len - LEN_BASE[i].0 as usize) as u32, LEN_BASE[i].1 as u32);
        self.code(dist_code as u32, 5);
        self.bits(dist_extra, DIST_BASE[dist_code].1 as u32);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Deflate (one block, fixed Huffman codes) of picture rows: besides literals only matches against the pixel to the left
/// (distance 3) and the pixel above (distance `stride`). A scope screen is flat areas, grid lines and a few traces, so
/// that is most of what a full LZ77 finds, in one pass, with no hash table and little time on the scope's CPU.
fn deflate_rows(raw: &[u8], stride: usize) -> Vec<u8> {
    let (stride_code, stride_extra) = {
        let c = DIST_BASE.iter().rposition(|&(b, _)| b as usize <= stride).unwrap_or(0);
        (c, stride.saturating_sub(DIST_BASE[c].0 as usize) as u32)
    };
    let stride_ok = stride > 3 && stride <= 32768;
    let run = |i: usize, d: usize| raw[i..].iter().zip(&raw[i - d..]).take(258).take_while(|(a, b)| a == b).count();
    let mut o = BitOut { out: Vec::with_capacity(raw.len() / 8 + 64), acc: 0, n: 0 };
    o.bits(1, 1); // BFINAL
    o.bits(1, 2); // BTYPE 01 = fixed Huffman
    let mut i = 0;
    while i < raw.len() {
        let left = if i >= 3 { run(i, 3) } else { 0 };
        let above = if stride_ok && i >= stride { run(i, stride) } else { 0 };
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
    let mut raw = Vec::with_capacity(stride * h as usize);
    for row in px.chunks(w as usize).take(h as usize) {
        raw.push(0);
        for &p in row {
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
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

/// Writes everything cached to the disks (a stick can be pulled, the scope switched off at the mains). Nothing to do
/// on hosts without `sync(2)`; their files reach the disk on their own.
pub fn sync_disks() {
    #[cfg(unix)]
    unsafe {
        libc::sync()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::rgb;
    use crate::math::{MathOp, Window};

    #[test]
    #[cfg(unix)]
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
        m.meas = vec![(Meas::Rise, WaveId::Ch(3)), (Meas::Vrms, WaveId::Ch(0)), (Meas::PDuty, WaveId::Ch(1))];
        m.strip_dock = StripDock::Right;
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
        (m.view, m.spec_src, m.spec_center, m.spec_span, m.spec_rbw, m.spec_trace, m.spec_ref, m.spec_wf, m.spec_dist) = (AView::Spectrum, WaveId::Ch(2), 1.5e6, 2e5, 1e3, 2, -12.5, true, true);
        (m.spec_sort, m.spec_harm, m.trend_sel) = (1, 4, [3, 1]);
        (m.eye_src, m.eye_rate, m.pw_v, m.pw_i, m.pw_mode) = (WaveId::Ch(3), 1.25e9, WaveId::Ch(2), WaveId::Ch(0), PowerMode::Switching);
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
    fn a_ref_in_a_setup_stays_a_ref_and_a_channel_number_stays_a_channel() {
        let mut m = Model::default();
        m.mag_v = WaveId::Ref(1);
        m.mag_i = WaveId::Ch(0);
        m.dpt_v = WaveId::Ref(0);
        m.inv_a = WaveId::Ref(4);
        m.inv_tq = Some(WaveId::Ref(9));
        m.spec_src = WaveId::Ref(0);
        m.eye_src = WaveId::Ref(2);
        m.pw_v = WaveId::Ref(1);
        m.pw_i = WaveId::Ch(0);
        m.bode_in = WaveId::Ref(3);
        m.bode_out = WaveId::Ch(1);
        m.meas = vec![(Meas::Vpp, WaveId::Ref(0))];
        m.meas2 = vec![(0, WaveId::Ref(0), WaveId::Ch(1))];
        let text = setup_text(&m);
        assert!(text.contains("magnetic = R2 1 "));
        assert!(text.contains("dpt = R1 "));
        assert!(text.contains("spec = R1 "));
        assert!(text.contains("eye = R3 "));
        assert!(text.contains("power = R2 1 "));
        assert!(text.contains("bode = "));
        assert!(text.contains(" R4 2 "));
        assert!(text.contains("meas = Vpp:R1"));
        assert!(text.contains("meas2 = 0:R1:2"));
        let back = parse_setup(&text).unwrap();
        assert_eq!((back.mag_v, back.mag_i, back.dpt_v, back.inv_a, back.inv_tq), (m.mag_v, m.mag_i, m.dpt_v, m.inv_a, m.inv_tq));
        assert_eq!((back.spec_src, back.eye_src, back.pw_v, back.pw_i, back.bode_in, back.bode_out), (m.spec_src, m.eye_src, m.pw_v, m.pw_i, m.bode_in, m.bode_out));
        assert_eq!((back.meas, back.meas2), (m.meas, m.meas2));
        // a setup written before refs were sources
        let old = parse_setup("magnetic = 1 2 1 0 0 0 IPsi 0\ninverter = Dq0 1 2 3 Off 1 4 true off off off 1 1 0 false 0 1 off 1024 false 0 1 2\ndpt = 1 2 3 Off 4 10 Both\n").unwrap();
        assert_eq!(old.mag_v, WaveId::Ch(0));
        assert_eq!(old.mag_i, WaveId::Ch(1));
        assert_eq!(old.inv_a, WaveId::Ch(0));
        assert_eq!(old.dpt_g, WaveId::Ch(2));
        assert_eq!(old.inv_tq, None);
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
    fn setup_sim_advanced_round_trip_and_legacy_defaults() {
        let mut model = Model::default();
        model.sim_prbs = 13;
        model.sim_ssc_center = true;
        model.sim_ssc_deviation = 0.01;
        model.sim_ssc_hz = 40e3;
        model.sim_dcd_pp = 8e-12;
        model.sim_corr_rj = 2e-12;
        model.sim_corr_hz = 2e6;
        let setup = setup_text(&model);
        assert_eq!(parse_setup(&setup).unwrap(), model);
        let legacy = setup.lines().filter(|line| !line.starts_with("sim_extra =")).collect::<Vec<_>>().join("\n");
        let old = parse_setup(&legacy).unwrap();
        assert_eq!(old.sim_prbs, 13);
        assert!(!old.sim_ssc_center);
        assert_eq!(old.sim_ssc_deviation, 0.005);
        assert_eq!(old.sim_ssc_hz, 33e3);
        assert_eq!(old.sim_dcd_pp, 0.0);
        assert_eq!(old.sim_corr_rj, 0.0);
        assert_eq!(old.sim_corr_hz, 1e6);
        for invalid in ["true NaN 33000 0 0 1e6", "false .005 0 0 0 1e6",
                        "true .005 33000 -1 0 1e6", "false .005 33000 0 1 1e6"] {
            assert!(parse_setup(&format!("sim_extra = {invalid}\n")).is_err());
        }
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
            assert!(text.contains(&format!("dec_proto = {:?}\n", p)));
            assert_eq!(parse_setup(&text).unwrap().dec_proto, p);
        }
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
        let back = parse_wave(csv.as_bytes(), "wave.csv").unwrap();
        assert_eq!(back.len(), 2);
        assert!((back[0].dt - 1e-6).abs() < 1e-18);
        assert_eq!(back[0].v[0], 0.5);
        assert_eq!(back[1].v[2], 3.3);
    }

    #[test]
    fn oos_bin_and_wfm_round_trip() {
        let mut m = Model::default();
        m.ch[0].on = true;
        let rec = Record { t0: -2e-6, dt: 5e-9, ch: { let mut c: [Vec<f32>; NCH] = Default::default(); c[0] = vec![0.1, 0.2, -0.3, 0.4]; c }, triggered: true, partial: false, raw: None, la: None };
        let bin = wave_bin(&rec, &m);
        let t = &parse_wave(&bin, "wave.bin").unwrap()[0];
        assert_eq!(t.v, rec.ch[0]);
        assert!((t.dt - rec.dt).abs() < 1e-18);
        assert!((t.t0 - rec.t0).abs() < 1e-18);

        let v = [0.0_f32, 0.5, 1.0, -0.25];
        let wfm = wfm_f32(b":WFM#003", -1e-6, 1e-9, &v, "lane");
        let g = parse_wave(&wfm, "x.wfm").unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].label, "lane");
        assert!((g[0].dt - 1e-9).abs() < 1e-18);
        assert!((g[0].t0 + 1e-6).abs() < 1e-18);
        for (a, b) in g[0].v.iter().zip(v) {
            assert!((a - b).abs() < 1e-6, "{a} {b}");
        }
        let old = wfm_f32(b":WFM#001", -1e-6, 1e-9, &v, "lane");
        assert_eq!(parse_wave(&old, "old.wfm").unwrap_err(), "header is not :WFM#002 or :WFM#003");
        let two = wfm_f32(b":WFM#002", 0.0, 2e-9, &v, "two");
        assert_eq!(parse_wave(&two, "two.wfm").unwrap()[0].label, "two");
    }

    /// A short `:WFM#002` or `:WFM#003` curve of f32 samples, little endian.
    fn wfm_f32(magic: &[u8; 8], t0: f64, dt: f64, v: &[f32], label: &str) -> Vec<u8> {
        const CURVE: usize = 838;
        let mut b = vec![0u8; CURVE + v.len() * 4 + 8];
        b[0..2].copy_from_slice(&0x0F0Fu16.to_le_bytes());
        b[2..10].copy_from_slice(magic);
        b[15] = 4;
        b[16..20].copy_from_slice(&(CURVE as i32).to_le_bytes());
        let lab = label.as_bytes();
        b[40..40 + lab.len()].copy_from_slice(lab);
        let put_f64 = |b: &mut [u8], o: usize, x: f64| b[o..o + 8].copy_from_slice(&x.to_le_bytes());
        let put_u32 = |b: &mut [u8], o: usize, x: u32| b[o..o + 4].copy_from_slice(&x.to_le_bytes());
        // exp dim 1 at 168
        put_f64(&mut b, 168, 1.0);
        put_f64(&mut b, 176, 0.0);
        put_u32(&mut b, 168 + 72, 4);
        // imp dim 1 at 488
        put_f64(&mut b, 488, dt);
        put_f64(&mut b, 496, t0);
        // curve spec at 808
        put_u32(&mut b, 808 + 14, 0);
        put_u32(&mut b, 808 + 18, (v.len() * 4) as u32);
        for (k, &x) in v.iter().enumerate() {
            b[CURVE + k * 4..CURVE + k * 4 + 4].copy_from_slice(&x.to_le_bytes());
        }
        b
    }

    #[test]
    fn wave_and_fast_frame_round_trip() {
        let mut m = Model::default();
        m.ch[0].on = true;
        m.ch[2].on = true;
        let mut rec = Record { t0: -1e-3, dt: 1e-6, ch: Default::default(), triggered: true, partial: false, raw: None, la: None };
        rec.ch[0] = vec![0.5, 1.25];
        rec.ch[2] = vec![-1.0, 0.0];
        let b = wave_bin(&rec, &m);
        let (back, n) = parse_wave_bin(&b).unwrap();
        assert_eq!(n, b.len());
        assert_eq!(back.dt, rec.dt);
        assert_eq!(back.ch[0], rec.ch[0]);
        assert_eq!(back.ch[2], rec.ch[2]);
        let ff = ff_bin(&[rec.clone(), rec.clone()], &m);
        let recs = parse_ff(&ff).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[1].ch[0], rec.ch[0]);
        assert_eq!(parse_ff(&b).unwrap().len(), 1);
        let t = mask_text(2, 0.2, 0.4, true, &[1.0, 2.5]);
        let mf = parse_mask(&t).unwrap();
        assert_eq!((mf.src, mf.inv, mf.v), (Some(2), Some(true), vec![1.0, 2.5]));
        let old = parse_mask("1.0\n2.0\n").unwrap();
        assert_eq!(old.v, vec![1.0, 2.0]);
        assert!(old.src.is_none());
    }

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
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
