// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Storage: setup files (`key = value` text of the Model), waveform CSV and OOS's own waveform files, file names in the
//! storage directory. Waveform files of other instruments are in `import.rs`, PNG screenshots in `png.rs`.

use crate::gfx::Canvas;
use crate::math::{MATH_OPS, WINDOWS};
use crate::model::*;
use crate::wave::Record;
use oos_dsp::decode::Parity;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod header_import_tests;
mod import;
mod png;
#[cfg(test)]
mod settings_snapshot;
#[cfg(test)]
mod tests;
pub use import::*;
pub use png::*;

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
    /// The local storage directory, also when a stick is mounted (on the unit `/tmp/oos`, in RAM)
    Local,
}

impl StoreDest {
    pub const ALL: [StoreDest; 3] = [StoreDest::Auto, StoreDest::Stick, StoreDest::Local];

    pub fn name(self) -> &'static str {
        match self {
            StoreDest::Auto => "Auto",
            StoreDest::Stick => "USB",
            StoreDest::Local => "Local",
        }
    }
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
    let refuse = |what: &str| {
        Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{}: {}", path.display(), what)))
    };
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
const WHENS: [WhenKind; 5] = [WhenKind::Off, WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out];
const HEXPANDS: [HExpand; 4] = [HExpand::Center, HExpand::Left, HExpand::Right, HExpand::Trigger];
const CURSORS: [CursorMode; 5] =
    [CursorMode::Off, CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy];
const BITS: [Bit; 5] = [Bit::H, Bit::L, Bit::X, Bit::R, Bit::F];
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
    kv("dec_cphy_rate", m.dec_cphy_rate.to_string());
    kv("dec_src4", m.dec_src4.map_or("none".into(), |c| (c + 1).to_string()));
    kv("dec_diff", m.dec_diff.map_or("none".into(), |c| (c + 1).to_string()));
    kv("trig_bus", m.trig_bus.name().to_string());
    kv("trig_bus_val", m.trig_bus_val.to_string());
    let s = &m.ser;
    kv("ser.rel", name(s.rel));
    kv("ser.max", s.max.to_string());
    kv("ser.mask", s.mask.to_string());
    kv("ser.uart_inv", s.uart_inv.to_string());
    kv("ser.uart_stop", name(s.uart_stop));
    kv("ser.uart_err", name(s.uart_err));
    kv("ser.spi_cs_high", s.spi_cs_high.to_string());
    kv("ser.spi_timeout", s.spi_timeout.to_string());
    kv("ser.addr_rel", name(s.addr_rel));
    kv("ser.addr_max", s.addr_max.to_string());
    kv("ser.dir", name(s.dir));
    kv("ser.ten_bit", s.ten_bit.to_string());
    kv("ser.bytes", s.bytes.to_string());
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
    for (i, &seconds) in m.deskew_ch.iter().enumerate() {
        if seconds != 0.0 {
            kv(&format!("deskew_ch{}", i + 1), seconds.to_string());
        }
    }
    for (i, &seconds) in m.deskew_ref.iter().enumerate() {
        if seconds != 0.0 {
            kv(&format!("deskew_ref{}", i + 1), seconds.to_string());
        }
    }
    for (c, ch) in m.ch.iter().enumerate() {
        let p = |k: &str| format!("ch{}.{}", c + 1, k);
        crate::settings::write(&crate::settings::channel::ON, m, c, &mut kv);
        kv(&p("vdiv"), VSCALES[ch.vscale].to_string());
        if ch.vfine != 1.0 {
            kv(&p("vfine"), ch.vfine.to_string());
        }
        if ch.unit != AmpUnit::V {
            crate::settings::write(&crate::settings::channel::UNIT, m, c, &mut kv);
        }
        if ch.bias != 0.0 {
            kv(&p("bias"), ch.bias.to_string());
        }
        if ch.skew != 0.0 {
            kv(&p("skew"), ch.skew.to_string());
        }
        kv(&p("offset"), ch.offset.to_string());
        kv(&p("coupling"), name(ch.coupling));
        crate::settings::write(&crate::settings::channel::BW, m, c, &mut kv);
        kv(&p("probe"), ch.probe.to_string());
        crate::settings::write(&crate::settings::channel::INVERT, m, c, &mut kv);
    }
    kv("active", m.active.to_string());
    if let Some(source) = m.cur_b_source {
        kv("cur_b_source", source.token());
    }
    kv("tdiv", tscales()[m.tscale].to_string());
    kv("delay", m.delay.to_string());
    kv("trig_src", m.trig_src.to_string());
    kv("trig_ref", m.trig_ref.map_or("none".into(), |i| i.to_string()));
    kv("trig_la", m.trig_la.map_or("none".to_string(), |n| n.to_string()));
    kv(
        "pat_la",
        m.pat_la
            .iter()
            .map(|&b| match b {
                Bit::H => 'H',
                Bit::L => 'L',
                _ => 'X',
            })
            .collect(),
    );
    kv("trig_level", m.trig_level.to_string());
    crate::settings::write(&crate::settings::trigger::EDGE, m, 0, &mut kv);
    kv("trig_type", name(m.trig_type));
    kv("trig_level2", m.trig_level2.to_string());
    crate::settings::write(&crate::settings::trigger::REJECT, m, 0, &mut kv);
    kv("trig_src2", m.trig_src2.to_string());
    kv("trig_ref2", m.trig_ref2.map_or("none".into(), |i| i.to_string()));
    crate::settings::write(&crate::settings::trigger::SECOND, m, 0, &mut kv);
    kv("trig_when", name(m.trig_when));
    crate::settings::write(&crate::settings::trigger::TIMES, m, 0, &mut kv);
    kv("video_line", m.video_line.to_string());
    kv("pat", m.pat.iter().map(|&b| name(b)).collect::<Vec<_>>().join(" "));
    kv("pat_levels", list(&m.pat_levels));
    crate::settings::write(&crate::settings::trigger::WINDOW, m, 0, &mut kv);
    kv("nth_n", m.nth_n.to_string());
    crate::settings::write(&crate::settings::trigger::SETUP_HOLD, m, 0, &mut kv);
    crate::settings::write(&crate::settings::acquisition::MODE, m, 0, &mut kv);
    kv("segments", m.segments.to_string());
    crate::settings::write(&crate::settings::acquisition::SEG_HW, m, 0, &mut kv);
    kv("seg_interval", m.seg_ivl.to_string());
    kv("mdepth", m.mdepth.to_string());
    crate::settings::write(&crate::settings::display::VIEW, m, 0, &mut kv);
    kv("windows", name(m.win_layout));
    kv("density", name(m.density));
    kv("split_pane", m.split_pane.to_string());
    kv("ch_window", m.ch_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("math_window", m.math_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("la_window", (m.la_win + 1).to_string());
    kv("ref_window", m.ref_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("meas", m.meas.iter().map(|&(k, id)| format!("{:?}:{}", k, id.token())).collect::<Vec<_>>().join(" "));
    kv(
        "meas2",
        m.meas2.iter().map(|&(k, a, b)| format!("{}:{}:{}", k, a.token(), b.token())).collect::<Vec<_>>().join(" "),
    );
    if !m.cat.is_empty() {
        kv(
            "cat",
            m.cat
                .iter()
                .map(|&(k, a, b, c, d)| format!("{}:{}:{}:{}:{}", k, a.token(), b.token(), c.token(), d.token()))
                .collect::<Vec<_>>()
                .join(" "),
        );
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
    kv("aux_out", name(m.aux_out));
    crate::settings::write(&crate::settings::channel::FINE, m, 0, &mut kv);
    kv("fine_t", m.fine_t.to_string());
    if m.tfine != 1.0 {
        kv("tfine", m.tfine.to_string());
    }
    crate::settings::write(&crate::settings::channel::EXPAND, m, 0, &mut kv);
    kv("h_expand", format!("{:?}", m.h_expand));
    kv("math", name(m.math));
    kv("math_a", m.math_a.to_string());
    kv("math_b", m.math_b.to_string());
    kv("math_scale", m.math_scale.to_string());
    crate::settings::write(&crate::settings::math::FC, m, 0, &mut kv);
    kv("math_lin", format!("{} {}", m.math_lin.0, m.math_lin.1));
    for (k, e) in m.math_expr.iter().enumerate().filter(|(_, e)| !e.is_empty()) {
        kv(&format!("math_expr{}", k + 1), e.clone());
    }
    kv("log_ivl", m.log_ivl.to_string());
    kv("spec_margin", m.spec_margin.to_string());
    kv("math_offset", m.math_offset.to_string());
    crate::settings::write(&crate::settings::math::WINDOW_BW, m, 0, &mut kv);
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
    crate::settings::write(&crate::settings::display::DRAW, m, 0, &mut kv);
    crate::settings::write(&crate::settings::system::DRAW_PATH, m, 0, &mut kv);
    kv("hdmi", name(m.hdmi));
    kv("hdmi_view", m.hdmi_view.to_string());
    kv("view", name(m.view));
    // Display > Views: the count and the one shown, then what each of the others keeps (ui/views.rs)
    kv("views", format!("{} {}", m.views, m.view_cur + 1));
    for (i, v) in m.view_slots.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
        kv(&format!("view{}", i + 1), v.clone());
    }
    kv(
        "bode",
        format!(
            "{} {} {} {} {} {} {} {}",
            m.bode_start,
            m.bode_stop,
            m.bode_points,
            m.bode_amp,
            m.bode_in.token(),
            m.bode_out.token(),
            m.bode_gen,
            m.bode_addr
        ),
    );
    // the generator's connection: lan (the address above) or usb; a line of its own, so older versions skip it
    kv("bode_link", if m.bode_usb { "usb" } else { "lan" }.to_string());
    // free sweep: time, log, sync channel, edge, level, input (0 = none), reference dBV
    kv(
        "bode_sweep",
        format!(
            "{} {} {} {} {} {} {}",
            m.bode_sw_time,
            m.bode_sw_log,
            m.bode_sync + 1,
            name(m.bode_sync_slope),
            m.bode_sync_level,
            m.bode_sw_in,
            m.bode_ref
        ),
    );
    kv(
        "spec",
        format!(
            "{} {} {} {} {} {} {} {} {}",
            m.spec_src.token(),
            m.spec_center,
            m.spec_span,
            m.spec_rbw,
            name(m.spec_win),
            m.spec_trace,
            m.spec_ref,
            m.spec_wf,
            m.spec_dist
        ),
    );
    kv("spec_peaks", format!("{} {}", m.spec_sort, m.spec_harm));
    kv("eye", format!("{} {} {}", m.eye_src.token(), m.eye_rate, if m.eye_from_record { 1 } else { 0 }));
    kv("eye_place", name(m.eye_place));
    kv("power", format!("{} {} {}", m.pw_v.token(), m.pw_i.token(), name(m.pw_mode)));
    let opt = |id: Option<WaveId>| id.map(|w| w.token()).unwrap_or_else(|| "-".into());
    kv(
        "meas_cfg",
        format!(
            "{} {:?} {} {} {} {} {} {}",
            m.meas_idle,
            m.meas_out,
            m.meas_n,
            if m.meas_data { 1 } else { 0 },
            m.meas_lpct,
            m.meas_hpct,
            m.meas_tsetup,
            m.meas_thold
        ),
    );
    kv("meas_full_record", m.meas_full_record.to_string());
    kv("spec_acpr", format!("{} {}", m.spec_acpr_w, m.spec_acpr_s));
    kv("bode_mode", name(m.bode_mode));
    kv(
        "eye_more",
        format!(
            "{} {} {} {} {} {} {} {}",
            m.eye_ber,
            m.eye_pn_lo,
            m.eye_pn_hi,
            if m.eye_npj { 1 } else { 0 },
            opt(m.eye_rail),
            m.eye_notch,
            opt(m.eye_b),
            if m.eye_hp { 1 } else { 0 }
        ),
    );
    kv(
        "power_more",
        format!(
            "{} {} {} {} {}",
            m.pw_fline,
            m.pw_fsw,
            opt(m.pw_vin),
            m.pw_ref.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            opt(m.pw_clk)
        ),
    );
    kv(
        "magnetic",
        format!(
            "{} {} {} {} {} {} {:?} {}",
            m.mag_v.token(),
            m.mag_i.token(),
            m.mag_n,
            m.mag_ae,
            m.mag_le,
            m.mag_rcu,
            m.mag_plot,
            m.mag_cur
        ),
    );
    let opt = |c: Option<WaveId>| c.map_or("off".into(), |c| c.token());
    let line = |c: Option<u8>| c.map_or("off".into(), |c| c.to_string());
    kv(
        "inverter",
        format!(
            "{:?} {} {} {} {:?} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            m.inv_page,
            m.inv_a.token(),
            m.inv_b.token(),
            m.inv_c.token(),
            m.inv_ang,
            m.inv_poles,
            m.inv_an.token(),
            m.inv_volt,
            opt(m.inv_tq),
            opt(m.inv_ev),
            opt(m.inv_ei),
            m.inv_scale,
            m.inv_degv,
            m.inv_off,
            m.inv_elec,
            m.inv_qa,
            m.inv_qb,
            line(m.inv_qi),
            m.inv_cpr,
            m.inv_swap,
            m.inv_ha,
            m.inv_hb,
            m.inv_hc
        ),
    );
    kv(
        "dpt",
        format!(
            "{} {} {} {:?} {} {} {:?}",
            m.dpt_v.token(),
            m.dpt_i.token(),
            m.dpt_g.token(),
            m.dpt_4th,
            m.dpt_4ch.token(),
            m.dpt_pct,
            m.dpt_zoom
        ),
    );
    kv(
        "sim",
        format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            m.sim_preset,
            m.sim_slot,
            m.sim_rate,
            m.sim_prbs,
            m.sim_vhigh,
            m.sim_vlow,
            m.sim_rise,
            m.sim_noise,
            m.sim_bw,
            m.sim_deemph,
            if m.sim_ssc { 1 } else { 0 },
            m.sim_rj,
            m.sim_pj,
            m.sim_pjhz,
            0
        ),
    );
    kv(
        "sim_extra",
        format!(
            "{} {} {} {} {} {}",
            m.sim_ssc_center, m.sim_ssc_deviation, m.sim_ssc_hz, m.sim_dcd_pp, m.sim_corr_rj, m.sim_corr_hz
        ),
    );
    // zones: "on src inside t:v t:v ..."
    for (k, z) in m.zones.iter().enumerate() {
        let pts: Vec<String> = z.pts.iter().map(|(t, v)| format!("{}:{}", t, v)).collect();
        kv(
            &format!("zone{}", k + 1),
            format!("{} {} {} {}", z.on, z.src + 1, z.inside, pts.join(" ")).trim().to_string(),
        );
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
    kv("text_size", name(m.text_size));
    kv("la", m.la.to_text());
    kv(
        "dec_la",
        format!("{} {}", m.dec_la_in as u8, m.dec_la.map(|n| n.map_or("-".to_string(), |x| x.to_string())).join(" ")),
    );
    s
}

pub(crate) fn p_bool(v: &str) -> Result<bool, String> {
    match v {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        _ => Err(format!("not a boolean: {}", v)),
    }
}

pub(crate) fn p_f32(v: &str) -> Result<f32, String> {
    v.parse::<f32>().ok().filter(|x| x.is_finite()).ok_or_else(|| format!("not a number: {}", v))
}

pub(crate) fn p_u32(v: &str) -> Result<u32, String> {
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

pub(crate) fn p_idx(v: &str, n: usize) -> Result<usize, String> {
    let i = v.parse::<usize>().map_err(|_| format!("not an index: {}", v))?;
    if i < n {
        Ok(i)
    } else {
        Err(format!("{} out of range (< {})", i, n))
    }
}

fn p_decode_source(v: &str) -> Result<usize, String> {
    let source = if v.trim().starts_with(['R', 'r']) {
        match WaveId::parse(v.trim())? {
            WaveId::Ref(i) => DEC_REF_BASE + i,
            _ => unreachable!(),
        }
    } else {
        p_idx(v, DEC_SOURCE_COUNT)?
    };
    decode_wave(source).map(|_| source).ok_or_else(|| "decoder source must be CH or R; LA uses dec_la".into())
}

fn p_decode_optional_one_based(v: &str) -> Result<Option<usize>, String> {
    if v.trim() == "none" {
        return Ok(None);
    }
    if v.trim().starts_with(['R', 'r']) {
        return p_decode_source(v).map(Some);
    }
    let source = p_idx(v, DEC_SOURCE_COUNT + 1)?.max(1) - 1;
    decode_wave(source).map(|_| Some(source)).ok_or_else(|| "invalid decoder source".into())
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
    if let Some((reference, suffix)) =
        k.strip_prefix("deskew_ch").map(|s| (false, s)).or_else(|| k.strip_prefix("deskew_ref").map(|s| (true, s)))
    {
        let values: &mut [f64] = if reference { &mut m.deskew_ref } else { &mut m.deskew_ch };
        let index = suffix
            .parse::<usize>()
            .ok()
            .and_then(|i| i.checked_sub(1))
            .filter(|&i| i < values.len())
            .ok_or("invalid deskew source")?;
        let value =
            v.parse::<f64>().ok().filter(|x| x.is_finite() && x.abs() <= 1000.0).ok_or("invalid deskew seconds")?;
        values[index] = value;
        return Ok(());
    }
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
                "a" => c.a = p_idx(v, crate::math::MATH_SOURCES)?,
                "b" => c.b = p_idx(v, crate::math::MATH_SOURCES)?,
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
        // the fields of the settings table (settings/channel.rs)
        if let Some(s) = crate::settings::find(&format!("ch<n>.{}", field)) {
            return s.load(m, c, v);
        }
        let ch = &mut m.ch[c];
        match field {
            "vfine" => ch.vfine = p_f32(v)?.clamp(1.0, 2.5),
            "bias" => ch.bias = p_f32(v)?,
            "skew" => ch.skew = p_f32(v)?.clamp(-SKEW_MAX, SKEW_MAX),
            // the keyboard takes ten characters; a hand-edited file gets no more
            "label" => m.ch_label[c] = v.trim().chars().take(10).collect(),
            "vdiv" => {
                let x = p_f32(v)?;
                // 20 V/div was once on the ladder, the hardware ends at 10
                let x = x.min(VSCALES[VSCALES.len() - 1]);
                ch.vscale = VSCALES
                    .iter()
                    .position(|&s| (s - x).abs() <= s * 1e-3)
                    .ok_or_else(|| format!("no V/div step {}", v))?;
            }
            // setups before the 100/200 uV steps: index into the old ladder (500 uV .. 20 V)
            "vscale" => ch.vscale = (p_idx(v, VSCALES.len() - 1)? + 2).min(VSCALES.len() - 1),
            "offset" => ch.offset = p_f32(v)?,
            "coupling" => ch.coupling = p_enum(v, &COUPLINGS)?,
            // setups before 2026-11-03
            "bw20" => ch.bw = if p_bool(v)? { BW_20M } else { BW_FULL },
            "probe" => ch.probe = p_f32(v)?,
            _ => {} // unknown keys of newer versions are ignored
        }
        return Ok(());
    }
    // the keys of the settings table (settings.rs); the fields of a line are read with the line, below
    if let Some(s) = crate::settings::find(k).filter(|s| s.line().is_none()) {
        return s.load(m, 0, v);
    }
    let ntime = tscales().len();
    match k {
        "active" => m.active = p_idx(v, NCH)?,
        "cur_b_ch" => m.cur_b_source = Some(WaveId::Ch(p_idx(v, NCH)?)),
        "cur_b_source" => m.cur_b_source = if v == "none" { None } else { Some(WaveId::parse(v)?) },
        "tdiv" => m.tscale = tdiv_index(v)?,
        // setups before the 1/2 ns steps: index into the old ladder (5 ns .. 1000 s)
        "tscale" => m.tscale = legacy_tscale(v, ntime)?,
        "delay" => m.delay = p_f32(v)?,
        "trig_src" => m.trig_src = p_idx(v, NCH)?,
        "trig_ref" => m.trig_ref = if v == "none" { None } else { Some(p_idx(v, NREF)?) },
        "pat_la" => {
            if v.trim().len() != 16 {
                return Err(format!("pat_la: {:?}", v));
            }
            for (b, c) in m.pat_la.iter_mut().zip(v.trim().chars()) {
                *b = match c {
                    'H' => Bit::H,
                    'L' => Bit::L,
                    _ => Bit::X,
                };
            }
        }
        "trig_la" => m.trig_la = if v.trim() == "none" { None } else { Some(p_idx(v, 16)? as u8) },
        "trig_level" => m.trig_level = p_f32(v)?,
        "trig_type" => m.trig_type = p_enum(v, &TRIG_TYPES)?,
        "trig_level2" => m.trig_level2 = p_f32(v)?,
        "trig_src2" => m.trig_src2 = p_idx(v, NCH)?,
        "trig_ref2" => m.trig_ref2 = if v == "none" { None } else { Some(p_idx(v, NREF)?) },
        "trig_when" => m.trig_when = p_enum(v, &WHENS)?,
        "video_line" => m.video_line = p_u32(v)?,
        "pat" => {
            let b = p_list(v, NCH, |x| p_enum(x, &BITS))?;
            m.pat.copy_from_slice(&b);
        }
        "pat_levels" => {
            let l = p_list(v, NCH, p_f32)?;
            m.pat_levels.copy_from_slice(&l);
        }
        "nth_n" => m.nth_n = p_u32(v)?,
        "segments" => m.segments = (p_u32(v)? as usize).min(crate::ui::SEG_HW_MAX),
        "seg_interval" => m.seg_ivl = p_f32(v)?.clamp(0.0, crate::ui::SEG_IVL_MAX),
        "mdepth" => m.mdepth = p_idx(v, mdepths().len())?,
        "windows" => m.win_layout = p_enum(v, &WinLayout::ALL)?,
        "density" => m.density = p_enum(v, &Density::ALL)?,
        "split_pane" => m.split_pane = p_idx(v, crate::ui::PANES_N)?,
        "ch_window" => {
            let w = p_list(v, NCH, |x| p_idx(x, 5)?.checked_sub(1).ok_or_else(|| "window 0".to_string()))?;
            m.ch_win.copy_from_slice(&w);
        }
        "la_window" => m.la_win = p_idx(v, 5)?.checked_sub(1).ok_or("window 0")?,
        "ref_window" => {
            let w =
                p_list(v, crate::model::NREF, |x| p_idx(x, 5)?.checked_sub(1).ok_or_else(|| "window 0".to_string()))?;
            m.ref_win.copy_from_slice(&w);
        }
        "math_window" => {
            let w =
                p_list(v, crate::math::NMATH, |x| p_idx(x, 5)?.checked_sub(1).ok_or_else(|| "window 0".to_string()))?;
            m.math_win.copy_from_slice(&w);
        }
        "meas" => {
            let mut out = Vec::new();
            for e in v.split_whitespace() {
                let (k, c) = e.split_once(':').ok_or_else(|| format!("not item:channel: {}", e))?;
                out.push((p_enum(k, &ALL_MEAS)?, WaveId::parse(c)?));
            }
            if out.len() + m.meas2.len() + m.cat.len() > MEAS_SELECTION_MAX {
                return Err("too many selected measurements".into());
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
            if m.meas.len() + out.len() + m.cat.len() > MEAS_SELECTION_MAX {
                return Err("too many selected measurements".into());
            }
            m.meas2 = out;
        }
        "cat" => {
            let mut out = Vec::new();
            for e in v.split_whitespace() {
                let f: Vec<&str> = e.split(':').collect();
                let [k, a, b, c, d] = f[..] else { return Err(format!("not item:a:b:c:d: {}", e)) };
                let id = k.parse::<u16>().map_err(|_| format!("not a catalog id: {}", k))?;
                if !catalog_id_valid(id) {
                    return Err(format!("unknown or non-Wave catalog id: {id}"));
                }
                if m.meas.len() + m.meas2.len() + out.len() >= MEAS_SELECTION_MAX {
                    return Err("too many selected measurements".into());
                }
                let row = (id, WaveId::parse(a)?, WaveId::parse(b)?, WaveId::parse(c)?, WaveId::parse(d)?);
                if out.contains(&row) {
                    return Err("duplicate catalog row".into());
                }
                out.push(row);
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
        "aux_out" => m.aux_out = p_enum(v, &AUX_OUTS)?,
        "fine_t" => m.fine_t = p_bool(v)?,
        "tfine" => m.tfine = p_f32(v)?.clamp(0.1, 1.0),
        "h_expand" => m.h_expand = p_enum(v, &HEXPANDS)?,
        "math" => m.math = p_enum(v, &MATH_OPS)?,
        "math_a" => m.math_a = p_idx(v, crate::math::MATH_SOURCES)?,
        "math_b" => m.math_b = p_idx(v, crate::math::MATH_SOURCES)?,
        "math_sel" => m.math_sel = p_idx(v, crate::math::NMATH)?,
        "math_scale" => m.math_scale = p_idx(v, crate::math::math_scales().len())?,
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
        "dec_on" => m.dec_on = p_bool(v)?,
        "dec_src" => m.dec_src = p_decode_source(v)?,
        "dec_baud" => m.dec_baud = p_idx(v, crate::decode::BAUDS.len())?,
        "dec_canrate" => m.dec_canrate = p_idx(v, crate::decode::CAN_RATES.len())?,
        "dec_canh" => m.dec_canh = p_bool(v)?,
        "dec_fdrate" => m.dec_fdrate = p_idx(v, crate::decode::FD_RATES.len())?,
        "dec_arate" => m.dec_arate = p_idx(v, crate::decode::ARINC_RATES.len())?,
        "dec_tick" => m.dec_tick = p_idx(v, crate::decode::SENT_TICKS.len())?,
        "dec_nibbles" => m.dec_nibbles = p_u32(v)?.clamp(1, 6),
        "dec_uspeed" => m.dec_uspeed = p_idx(v, crate::decode::USB_SPEEDS.len())?,
        "dec_cphy_rate" => m.dec_cphy_rate = crate::decode::parse_cphy_rate(v)?,
        "dec_src4" => m.dec_src4 = p_decode_optional_one_based(v)?,
        "dec_diff" => m.dec_diff = p_decode_optional_one_based(v)?,
        "trig_bus" => {
            m.trig_bus =
                *BUS_WHATS.iter().find(|b| b.name() == v.trim()).ok_or_else(|| format!("unknown bus match {}", v))?
        }
        "trig_bus_val" => m.trig_bus_val = v.trim().parse().map_err(|e| format!("{}", e))?,
        "ser.rel" => m.ser.rel = p_enum(v, &RELS)?,
        "ser.max" => m.ser.max = v.trim().parse().map_err(|e| format!("ser.max: {}", e))?,
        "ser.mask" => m.ser.mask = v.trim().parse().map_err(|e| format!("ser.mask: {}", e))?,
        "ser.uart_inv" => m.ser.uart_inv = p_bool(v)?,
        "ser.uart_stop" => m.ser.uart_stop = p_enum(v, &STOP_BITS)?,
        "ser.uart_err" => m.ser.uart_err = p_enum(v, &UART_ERRS)?,
        "ser.spi_cs_high" => m.ser.spi_cs_high = p_bool(v)?,
        "ser.spi_timeout" => m.ser.spi_timeout = p_f32(v)?.max(0.0),
        "ser.addr_rel" => m.ser.addr_rel = p_enum(v, &RELS)?,
        "ser.addr_max" => m.ser.addr_max = p_u32(v)?.min(0x3ff),
        "ser.dir" => m.ser.dir = p_enum(v, &I2C_DIRS)?,
        "ser.ten_bit" => m.ser.ten_bit = p_bool(v)?,
        "ser.bytes" => m.ser.bytes = p_u32(v)?.min(5) as u8,
        "dec_thr" => m.dec_thr = p_f32(v)?,
        "dec_ascii" => m.dec_ascii = p_bool(v)?,
        "dec_proto" => {
            m.dec_proto = Proto::from_key(v.trim()).ok_or_else(|| format!("unknown protocol: {}", v.trim()))?
        }
        // a module's option indices; brought into range where they are used (`protocols::opt_fit`)
        "dec_opt" => {
            m.dec_opt =
                p_list(v, crate::protocols::OPTS, |x| x.parse::<u8>().map_err(|e| e.to_string()))?.try_into().unwrap()
        }
        "dec_src2" => m.dec_src2 = p_decode_source(v)?,
        "dec_cs" => m.dec_cs = if v == "none" { None } else { Some(p_decode_source(v)?) },
        "dec_mode" => m.dec_mode = p_idx(v, 4)? as u8,
        "dec_bits" => m.dec_bits = p_u32(v)?.clamp(4, 32),
        "dec_parity" => m.dec_parity = p_enum(v, &PARITIES)?,
        "zoom" => m.zoom = p_bool(v)?,
        "auto_roll" => m.auto_roll = p_bool(v)?,
        "zoom_tdiv" => m.zscale = tdiv_index(v)?,
        "zscale" => m.zscale = legacy_tscale(v, ntime)?,
        "zdelay" => m.zdelay = p_f32(v)?,
        "hdmi" => m.hdmi = p_enum(v, &HDMI_MODES)?,
        "hdmi_view" => {
            let n = v.trim().parse::<usize>().map_err(|e| e.to_string())?;
            if n > crate::ui::VIEWS_MAX {
                return Err(format!("{:?}: 0 (the panel's) to {}", v, crate::ui::VIEWS_MAX));
            }
            m.hdmi_view = n;
        }
        "views" => {
            let n = p_list(v, 2, |x| x.parse::<usize>().map_err(|e| e.to_string()))?;
            if !(1..=crate::ui::VIEWS_MAX).contains(&n[0]) || !(1..=n[0]).contains(&n[1]) {
                return Err(format!("{:?}: count 1 to {}, then the one shown", v, crate::ui::VIEWS_MAX));
            }
            (m.views, m.view_cur) = (n[0], n[1] - 1);
        }
        k if k
            .strip_prefix("view")
            .and_then(|i| i.parse::<usize>().ok())
            .is_some_and(|i| (1..=crate::ui::VIEWS_MAX).contains(&i)) =>
        {
            let i: usize = k[4..].parse().unwrap_or(1);
            m.view_slots[i - 1] = crate::ui::views::slot_checked(v)?;
        }
        // setups before the Layout mode: true = Waveform, false = Mirror
        "hdmi_wave" => m.hdmi = if p_bool(v)? { HdmiMode::Waveform } else { HdmiMode::Mirror },
        "view" => m.view = p_enum(v, &crate::ui::stored_views())?,
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
        "bode_link" => {
            m.bode_usb = match v.trim() {
                "usb" => true,
                "lan" => false,
                _ => return Err(format!("not lan or usb: {}", v)),
            }
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
            m.bode_sync_slope = p_enum(w[3], &crate::settings::trigger::SLOPES[..2])?;
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
            m.pw_mode = p_enum(
                w[2],
                &[
                    PowerMode::Quality,
                    PowerMode::Harmonics,
                    PowerMode::Switching,
                    PowerMode::Ripple,
                    PowerMode::Capacitance,
                    PowerMode::Rail,
                ],
            )?;
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
            let opt = |s: &str| -> Result<Option<WaveId>, String> {
                if s == "off" {
                    Ok(None)
                } else {
                    Ok(Some(ch(s)?))
                }
            };
            let bit = |s: &str| -> Result<u8, String> {
                let n = s.parse::<u8>().map_err(|e| e.to_string())?;
                if n < 16 {
                    Ok(n)
                } else {
                    Err(format!("logic line {} out of range", n))
                }
            };
            m.inv_page = p_enum(w[0], &[InvPage::Dq0, InvPage::Mech])?;
            (m.inv_a, m.inv_b, m.inv_c) = (ch(w[1])?, ch(w[2])?, ch(w[3])?);
            m.inv_ang =
                p_enum(w[4], &[InvAngle::Off, InvAngle::Phase, InvAngle::Analog, InvAngle::Quad, InvAngle::Hall])?;
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
        "text_size" => m.text_size = p_enum(v, &TEXT_SIZES)?,
        "la" => m.la = crate::ui::la::LaSet::parse(v)?,
        "dec_la" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 5 {
                return Err(format!("dec_la: {:?}", v));
            }
            m.dec_la_in = w[0] == "1";
            for (r, s) in m.dec_la.iter_mut().zip(&w[1..]) {
                *r = if *s == "-" {
                    None
                } else {
                    Some(s.parse::<u8>().map_err(|_| format!("dec_la: {:?}", v))?.min(15))
                };
            }
        }
        "meas_full_record" => m.meas_full_record = p_bool(v)?,
        "meas_cfg" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 4 && w.len() != 8 {
                return Err("meas_cfg: idle outside n data [lpct hpct tsetup thold]".into());
            }
            m.meas_idle = w[0].parse::<f64>().map_err(|e| e.to_string())?;
            m.meas_out = p_enum(
                w[1],
                &[oos_dsp::measure::Outside::Above, oos_dsp::measure::Outside::Below, oos_dsp::measure::Outside::Both],
            )?;
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
        "bode_mode" => {
            m.bode_mode = p_enum(v, &[oos_dsp::bode::Mode::Loop, oos_dsp::bode::Mode::Psrr, oos_dsp::bode::Mode::Z])?
        }
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
                s.parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && *value >= low && *value <= high)
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
#[cfg_attr(not(test), allow(dead_code))] // the strict form, for the tests
pub fn parse_setup(text: &str) -> Result<Model, String> {
    let (m, bad) = parse_setup_lenient(text);
    match bad.into_iter().next() {
        Some(e) => Err(e),
        None => Ok(m),
    }
}

/// `parse_setup` that loads what it can: a line it cannot take (a key of a newer or another build, a protocol not in
/// this one, a value out of range) is left out and named in the list, the rest of the setup stands. A decoder whose
/// protocol is not in this build is switched off, its other settings kept. For recalling a setup and the restart.
pub fn parse_setup_lenient(text: &str) -> (Model, Vec<String>) {
    let mut bad = Vec::new();
    let m = parse_lines(text, &mut bad);
    (m, bad)
}

fn parse_lines(text: &str, bad: &mut Vec<String>) -> Model {
    let mut m = Model::default();
    let mut proto_missing = false;
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            bad.push(format!("line {}: no '='", ln + 1));
            continue;
        };
        let k = k.trim();
        if let Err(e) = set(&mut m, k, v.trim()) {
            proto_missing |= k == "dec_proto";
            bad.push(format!("line {}: {}: {}", ln + 1, k, e));
        }
    }
    if proto_missing {
        m.dec_on = false;
    }
    // a protocol that became a module since the setup was written: its options from the shared decoder rows
    if let (Proto::Module(i), false) =
        (m.dec_proto, text.lines().any(|l| l.split('=').next().is_some_and(|k| k.trim() == "dec_opt")))
    {
        let old = crate::protocols::Legacy {
            baud: m.dec_baud,
            bits: m.dec_bits,
            parity: m.dec_parity,
            canrate: m.dec_canrate,
            canh: m.dec_canh,
            fdrate: m.dec_fdrate,
            arate: m.dec_arate,
            tick: m.dec_tick,
            nibbles: m.dec_nibbles,
            uspeed: m.dec_uspeed,
        };
        if let Some(o) = crate::protocols::get(i).legacy(&old) {
            m.dec_opt = o.map(|x| x as u8);
            m.dec_opt = crate::protocols::opt_fit(i, m.dec_opt).map(|x| x as u8);
        }
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
    m
}

/// Display state of a ref slot, kept in a setup file as `ref<N>.<key>` lines. The samples are not part of a setup:
/// a recall restores how the slot looks, and applies it to the slot when that holds a waveform.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct RefLook {
    pub on: Option<bool>,
    pub vdiv: Option<f32>,
    pub offset: Option<f32>,
    pub color: Option<usize>,
    pub label: Option<String>,
}

/// `ref<N>.*` lines for the loaded slots (`None` = empty slot, nothing is written).
pub fn ref_setup_text(looks: &[Option<RefLook>]) -> String {
    let mut s = String::new();
    for (i, l) in looks.iter().enumerate() {
        let Some(l) = l else { continue };
        let mut kv = |k: &str, v: String| s.push_str(&format!("ref{}.{} = {}\n", i + 1, k, v));
        if let Some(v) = l.on {
            kv("on", v.to_string());
        }
        if let Some(v) = l.vdiv {
            kv("vdiv", v.to_string());
        }
        if let Some(v) = l.offset {
            kv("offset", v.to_string());
        }
        if let Some(v) = l.color {
            kv("color", v.to_string());
        }
        if let Some(v) = &l.label {
            kv("name", v.replace(['\r', '\n'], " "));
        }
    }
    s
}

/// The `ref<N>.*` lines of a setup file; a setup from before they existed gives all `None`.
pub fn parse_ref_looks(text: &str, n: usize) -> Vec<Option<RefLook>> {
    let mut out: Vec<Option<RefLook>> = vec![None; n];
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let Some(rest) = k.trim().strip_prefix("ref") else { continue };
        let Some((idx, key)) = rest.split_once('.') else { continue };
        let Some(i) = idx.parse::<usize>().ok().and_then(|i| i.checked_sub(1)).filter(|&i| i < n) else { continue };
        let v = v.trim();
        let l = out[i].get_or_insert_with(RefLook::default);
        match key {
            "on" => l.on = v.parse().ok(),
            "vdiv" => l.vdiv = v.parse().ok().filter(|x: &f32| x.is_finite() && *x > 0.0),
            "offset" => l.offset = v.parse().ok().filter(|x: &f32| x.is_finite()),
            "color" => l.color = v.parse().ok(),
            "name" => l.label = Some(v.to_string()),
            _ => {}
        }
    }
    out
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

/// Import bounds shared by native readers and remote uploads. They include the
/// supplied 80 MB / 20-million-point captures without unbounded allocations.
pub const MAX_WAVEFORM_BYTES: usize = 128 << 20;
pub const MAX_WAVEFORM_SAMPLES: usize = 32_000_000;

/// File systems of the kernel: their files can be regular and still endless (`/proc/kmsg`), or a device.
const NOT_STORAGE: [&str; 3] = ["/proc", "/sys", "/dev"];

/// The bytes of a waveform file named by a remote client (SCPI `:REFerence:LOAD`, also over VXI-11 and MCP) or found
/// beside the setups, read on the frame loop. Refused: a file of `NOT_STORAGE` (by the path it resolves to), and one
/// larger than the import limit, from its size before anything is read; the read stops one byte past the limit should
/// the file grow meanwhile. A path that is not there or not a regular file is `missing <name>`.
pub fn read_waveform_file(p: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let name = file_name(p);
    let missing = || format!("missing {name}");
    let real = p.canonicalize().map_err(|_| missing())?;
    if NOT_STORAGE.iter().any(|d| real.starts_with(d)) {
        return Err(format!("{name}: files under /proc, /sys and /dev are not read"));
    }
    let too_large = || format!("{name} exceeds the {} MiB import limit", MAX_WAVEFORM_BYTES >> 20);
    // before the open: opening a FIFO would wait for a writer
    let meta = std::fs::metadata(&real).map_err(|_| missing())?;
    if !meta.is_file() {
        return Err(missing());
    }
    if meta.len() > MAX_WAVEFORM_BYTES as u64 {
        return Err(too_large());
    }
    let file = std::fs::File::open(&real).map_err(|_| missing())?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_WAVEFORM_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
    if bytes.len() > MAX_WAVEFORM_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Extensions shown by native and browser waveform import. The header chooses the parser.
pub const WAVE_EXTS: &[&str] = &["wfm", "bin", "csv", "trc", "isf", "tr0", "wfmx"];

/// One OOSW blob: the record and the number of bytes consumed.
pub fn parse_wave_bin(b: &[u8]) -> Result<(Record, usize), String> {
    if b.len() < 36 || &b[..4] != b"OOSW" {
        return Err("not an OOS waveform file".into());
    }
    let f = Fields::new(b, true);
    let ver = f.u32(4).unwrap_or(0);
    if ver != 1 {
        return Err(format!("waveform version {}", ver));
    }
    let nch = f.u32(8).unwrap_or(0) as usize;
    // the sample count comes from the file: bounded before it sizes anything, so a damaged or hostile count is an
    // error and not an overflow or an allocation the size of the count
    let n = usize::try_from(f.u64(12).unwrap_or(0)).unwrap_or(usize::MAX);
    let dt = f.f64(20).unwrap_or(0.0);
    let t0 = f.f64(28).unwrap_or(0.0);
    if nch == 0 || nch > NCH || dt <= 0.0 {
        return Err("waveform header".into());
    }
    if n > MAX_WAVEFORM_SAMPLES {
        return Err("waveform exceeds the 32-million-sample import limit".into());
    }
    let mut off = 36;
    let mut rec =
        Record { t0, dt, ch: Default::default(), triggered: true, partial: false, raw: None, la: None, pairs: false };
    for _ in 0..nch {
        let c = f.u32(off)? as usize;
        off += 4;
        if !(1..=NCH).contains(&c) {
            return Err(format!("channel {}", c));
        }
        if b.len().saturating_sub(off) < 4 * n {
            return Err("truncated waveform".into());
        }
        let mut v = vec![0f32; n];
        for i in 0..n {
            v[i] = f.f32(off + 4 * i).unwrap_or(f32::NAN);
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
    let f = Fields::new(b, true);
    let ver = f.u32(4).unwrap_or(0);
    if ver != 1 {
        return Err(format!("fast-frame version {}", ver));
    }
    let n = f.u32(8).unwrap_or(0) as usize;
    // every frame is at least a header and one channel number: a count the bytes cannot hold is refused before it
    // sizes the list
    if n > (b.len() - 12) / 40 {
        return Err("truncated fast-frame file".into());
    }
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
                if best.as_ref().is_none_or(|(b, _)| n >= *b) {
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
    if lines.peek().is_some_and(|l| l.starts_with("# OOS mask")) {
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
                    if !(1..=NCH).contains(&n) {
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

/// Writes everything cached to the disks (a stick can be pulled, the scope switched off at the mains).
pub fn sync_disks() {
    unsafe { libc::sync() };
}
