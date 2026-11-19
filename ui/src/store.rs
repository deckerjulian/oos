// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Storage: setup files (`key = value` text of the Model), waveform CSV and OOS's own waveform files, file names in the
//! storage directory. This file writes the setup (`setup_text`, the `ref<N>.*` lines) and names the parts, each
//! re-exported here so that callers say `store::<item>`: the setup reader in `setup_read.rs`, where files go and how
//! they are written in `paths.rs`, CSV, OOS's waveform and mask files in `waveform.rs`, waveform files of other
//! instruments in `import.rs`, PNG screenshots in `png.rs`.

use crate::math::{MATH_OPS, WINDOWS};
use crate::model::*;
use crate::wave::Record;
use oos_dsp::decode::Parity;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod header_import_tests;
mod import;
mod paths;
#[cfg(test)]
mod settings_snapshot;
mod setup_read;
#[cfg(test)]
mod tests;
pub mod views;
mod waveform;
pub use import::*;
pub use paths::*;
// the picture files (PNG, PPM) are base (gfx::png); a setup or a screenshot saved by store writes them
pub use crate::gfx::png::*;
pub use setup_read::*;
pub use waveform::*;

// ---------------------------------------------------------------------------------------------- setup files

fn name<T: Debug>(x: T) -> String {
    format!("{:?}", x)
}

fn list<T: ToString>(v: &[T]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")
}

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
    kv("trig_bus", bus_what_name(m.trig_bus).to_string());
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

/// The deskew of the channels and the ref slots that have one.
fn deskew_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
}

/// The `ch<N>.*` keys of each channel.
fn channel_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    for (c, ch) in m.ch.iter().enumerate() {
        let p = |k: &str| format!("ch{}.{}", c + 1, k);
        crate::settings::write(&crate::settings::channel::ON, m, c, kv);
        kv(&p("vdiv"), VSCALES[ch.vscale].to_string());
        if ch.vfine != 1.0 {
            kv(&p("vfine"), ch.vfine.to_string());
        }
        if ch.unit != AmpUnit::V {
            crate::settings::write(&crate::settings::channel::UNIT, m, c, kv);
        }
        if ch.bias != 0.0 {
            kv(&p("bias"), ch.bias.to_string());
        }
        if ch.skew != 0.0 {
            kv(&p("skew"), ch.skew.to_string());
        }
        kv(&p("offset"), ch.offset.to_string());
        kv(&p("coupling"), name(ch.coupling));
        crate::settings::write(&crate::settings::channel::BW, m, c, kv);
        kv(&p("probe"), ch.probe.to_string());
        crate::settings::write(&crate::settings::channel::INVERT, m, c, kv);
    }
}

/// The active channel, the cursor B source, the timebase and the trigger.
fn time_trigger_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
    crate::settings::write(&crate::settings::trigger::EDGE, m, 0, kv);
    kv("trig_type", name(m.trig_type));
    kv("trig_level2", m.trig_level2.to_string());
    crate::settings::write(&crate::settings::trigger::REJECT, m, 0, kv);
    kv("trig_src2", m.trig_src2.to_string());
    kv("trig_ref2", m.trig_ref2.map_or("none".into(), |i| i.to_string()));
    crate::settings::write(&crate::settings::trigger::SECOND, m, 0, kv);
    kv("trig_when", name(m.trig_when));
    crate::settings::write(&crate::settings::trigger::TIMES, m, 0, kv);
    kv("video_line", m.video_line.to_string());
    kv("pat", m.pat.iter().map(|&b| name(b)).collect::<Vec<_>>().join(" "));
    kv("pat_levels", list(&m.pat_levels));
    crate::settings::write(&crate::settings::trigger::WINDOW, m, 0, kv);
    kv("nth_n", m.nth_n.to_string());
    crate::settings::write(&crate::settings::trigger::SETUP_HOLD, m, 0, kv);
}

/// The acquisition, the segments, the memory depth and the windows of the plot.
fn acquire_window_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    crate::settings::write(&crate::settings::acquisition::MODE, m, 0, kv);
    kv("segments", m.segments.to_string());
    crate::settings::write(&crate::settings::acquisition::SEG_HW, m, 0, kv);
    kv("seg_interval", m.seg_ivl.to_string());
    kv("mdepth", m.mdepth.to_string());
    crate::settings::write(&crate::settings::display::VIEW, m, 0, kv);
    kv("windows", name(m.win_layout));
    kv("density", name(m.density));
    kv("split_pane", m.split_pane.to_string());
    kv("ch_window", m.ch_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("math_window", m.math_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
    kv("la_window", (m.la_win + 1).to_string());
    kv("ref_window", m.ref_win.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(" "));
}

/// The selected measurements, the voltmeter and the counter.
fn meas_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
}

/// The channel labels, the cursors, the AUX output, the fine steps and the horizontal expansion.
fn label_cursor_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
    crate::settings::write(&crate::settings::channel::FINE, m, 0, kv);
    kv("fine_t", m.fine_t.to_string());
    if m.tfine != 1.0 {
        kv("tfine", m.tfine.to_string());
    }
    crate::settings::write(&crate::settings::channel::EXPAND, m, 0, kv);
    kv("h_expand", format!("{:?}", m.h_expand));
}

/// The selected math channel as `math*` keys, the others as `m<N>.*`.
fn math_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    kv("math", name(m.math));
    kv("math_a", m.math_a.to_string());
    kv("math_b", m.math_b.to_string());
    kv("math_scale", m.math_scale.to_string());
    crate::settings::write(&crate::settings::math::FC, m, 0, kv);
    kv("math_lin", format!("{} {}", m.math_lin.0, m.math_lin.1));
    for (k, e) in m.math_expr.iter().enumerate().filter(|(_, e)| !e.is_empty()) {
        kv(&format!("math_expr{}", k + 1), e.clone());
    }
    kv("log_ivl", m.log_ivl.to_string());
    crate::settings::write(&crate::settings::spectrum::MARGIN, m, 0, kv);
    kv("math_offset", m.math_offset.to_string());
    crate::settings::write(&crate::settings::math::WINDOW_BW, m, 0, kv);
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
}

/// The panes, zoom, the drawing, HDMI and the views.
fn view_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    kv("pane", m.pane.to_string());
    kv("meas_labels", m.meas_labels.to_string());
    kv("strip_dock", name(m.strip_dock));
    kv("trend_sel", format!("{} {}", m.trend_sel[0], m.trend_sel[1]));
    kv("zoom", m.zoom.to_string());
    kv("auto_roll", m.auto_roll.to_string());
    kv("zoom_tdiv", tscales()[m.zscale].to_string());
    kv("zdelay", m.zdelay.to_string());
    crate::settings::write(&crate::settings::display::DRAW, m, 0, kv);
    kv("hdmi", name(m.hdmi));
    kv("hdmi_view", m.hdmi_view.to_string());
    kv("view", name(m.view));
    // Display > Views: the count and the one shown, then what each of the others keeps (ui/views.rs)
    kv("views", format!("{} {}", m.views, m.view_cur + 1));
    for (i, v) in m.view_slots.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
        kv(&format!("view{}", i + 1), v.clone());
    }
}

/// The Bode plot, the spectrum, the eye and the power analysis.
fn analysis_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
    crate::settings::write(&crate::settings::bode::LINK, m, 0, kv);
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
    crate::settings::write(&crate::settings::eye::PLACE, m, 0, kv);
    kv("power", format!("{} {} {}", m.pw_v.token(), m.pw_i.token(), name(m.pw_mode)));
}

/// The measurement setup and the later lines of the analysers, with the magnetics.
fn analysis_more_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
    crate::settings::write(&crate::settings::bode::MODE, m, 0, kv);
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
}

/// The inverter, the double pulse test and the signal simulator.
fn inverter_sim_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
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
}

/// The zones, the histogram, the mask test, the quick key, the text size and the logic lines.
fn zone_mask_lines(m: &Model, kv: &mut dyn FnMut(&str, String)) {
    // zones: "on src inside t:v t:v ..."
    for (k, z) in m.zones.iter().enumerate() {
        let pts: Vec<String> = z.pts.iter().map(|(t, v)| format!("{}:{}", t, v)).collect();
        kv(
            &format!("zone{}", k + 1),
            format!("{} {} {} {}", z.on, z.src + 1, z.inside, pts.join(" ")).trim().to_string(),
        );
    }
    kv("z_src", m.z_src.map_or("off".to_string(), |c| (c + 1).to_string()));
    crate::settings::write(&crate::settings::histogram::HIST, m, 0, kv);
    crate::settings::write(&crate::settings::mask::ON, m, 0, kv);
    kv("mask_src", m.mask_src.to_string());
    kv("mask_x", m.mask_x.to_string());
    kv("mask_y", m.mask_y.to_string());
    crate::settings::write(&crate::settings::mask::TEST, m, 0, kv);
    kv("quick", name(m.quick));
    kv("text_size", name(m.text_size));
    kv("la", m.la.to_text());
    kv(
        "dec_la",
        format!("{} {}", m.dec_la_in as u8, m.dec_la.map(|n| n.map_or("-".to_string(), |x| x.to_string())).join(" ")),
    );
}

/// The setup as `key = value` lines: every Model field except the acquisition state (run / trigger status).
/// Enums by variant name, indices into the value ladders as numbers, floats in Rust's shortest exact form.
pub fn setup_text(m: &Model) -> String {
    let mut s = String::from("# OOS setup v1: key = value (oos-ui)\n");
    let mut kv = |k: &str, v: String| {
        s.push_str(k);
        s.push_str(" = ");
        s.push_str(&v);
        s.push('\n');
    };
    deskew_lines(m, &mut kv);
    channel_lines(m, &mut kv);
    time_trigger_lines(m, &mut kv);
    acquire_window_lines(m, &mut kv);
    meas_lines(m, &mut kv);
    label_cursor_lines(m, &mut kv);
    math_lines(m, &mut kv);
    kv("dec_sel", m.dec_sel.to_string());
    dec_lines(m, &mut kv);
    bus_lines(m, &mut kv);
    for k in (0..DECODERS).filter(|&k| k != m.dec_sel && m.decs[k].is_some()) {
        dec_lines(&m.dec_view(k), &mut |n: &str, v: String| kv(&format!("d{}.{}", k + 1, n), v));
    }
    view_lines(m, &mut kv);
    analysis_lines(m, &mut kv);
    analysis_more_lines(m, &mut kv);
    inverter_sim_lines(m, &mut kv);
    zone_mask_lines(m, &mut kv);
    s
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

// at the end, so the lines of the program keep their numbers (tools/scope-binaries.sh)
#[cfg(test)]
pub(crate) mod hostile_tests;
