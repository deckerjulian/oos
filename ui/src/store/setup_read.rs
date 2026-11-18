// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The setup reader: `set` takes one `key = value` line into the Model, `parse_setup` and `parse_setup_lenient` a
//! whole setup file, with the value parsers they share (`p_bool`, `p_f32`, `p_idx`, ...). A new key is read here,
//! in `set`, and written in `setup_text` of store.rs; a key of the settings table (settings.rs) needs neither.

use super::*;

const COUPLINGS: [Coupling; 4] = [Coupling::Dc, Coupling::Ac, Coupling::Gnd, Coupling::Dc50];
const WHENS: [WhenKind; 5] = [WhenKind::Off, WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out];
const HEXPANDS: [HExpand; 4] = [HExpand::Center, HExpand::Left, HExpand::Right, HExpand::Trigger];
const CURSORS: [CursorMode; 5] =
    [CursorMode::Off, CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy];
const BITS: [Bit; 5] = [Bit::H, Bit::L, Bit::X, Bit::R, Bit::F];
const PARITIES: [Parity; 3] = [Parity::None, Parity::Even, Parity::Odd];

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

/// A number of an f64 field, as `str::parse` reads it: also `inf` and `NaN`, with its error text.
pub(crate) fn p_f64(v: &str) -> Result<f64, String> {
    v.parse::<f64>().map_err(|e| e.to_string())
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

/// A group of keys of the setup reader: `Ok(true)` when it knows the key and has read the value.
type SetGroup = fn(&mut Model, &str, &str) -> Result<bool, String>;

/// Takes one `key = value` line of a setup into the model: each group of keys below is asked in turn, the first
/// that knows the key reads the value. A key no group knows (one of a newer version) is ignored.
pub fn set(m: &mut Model, k: &str, v: &str) -> Result<(), String> {
    let groups: [SetGroup; 9] = [
        set_numbered,
        set_table,
        set_time_trigger,
        set_meas_math,
        set_decoder,
        set_zoom_views,
        set_bode_spec_power,
        set_inverter_zones,
        set_more,
    ];
    for group in groups {
        if group(m, k, v)? {
            return Ok(());
        }
    }
    Ok(()) // unknown keys of newer versions are ignored
}

/// The numbered keys: the deskew of a channel or a ref slot, the other decoders, the math channels, the channels.
fn set_numbered(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
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
        return Ok(true);
    }
    // the other decoders: dec_sel comes first in the file
    if let Some((n, field)) = k.strip_prefix('d').and_then(|r| r.split_once('.')) {
        if let Ok(n) = n.parse::<usize>() {
            let n = n.checked_sub(1).filter(|&n| n < DECODERS).ok_or("unknown decoder")?;
            let mut t = m.dec_view(n);
            set(&mut t, field, v)?;
            m.dec_store(n, t.dec_cfg());
            return Ok(true);
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
            return Ok(true);
        }
    }
    if let Some((c, field)) = k.strip_prefix("ch").and_then(|r| r.split_once('.')) {
        let c = c.parse::<usize>().ok().filter(|c| (1..=NCH).contains(c)).ok_or("unknown channel")? - 1;
        // the fields of the settings table (settings/channel.rs)
        if let Some(s) = crate::settings::find(&format!("ch<n>.{}", field)) {
            return s.load(m, c, v).map(|()| true);
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
        return Ok(true);
    }
    Ok(false)
}

/// The keys of the settings table (settings.rs); the fields of a line are read with the line, below.
fn set_table(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    if let Some(s) = crate::settings::find(k).filter(|s| s.line().is_none()) {
        return s.load(m, 0, v).map(|()| true);
    }
    Ok(false)
}

/// The active channel, the cursor B source, the timebase, the trigger, the acquisition and the windows.
fn set_time_trigger(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
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
        _ => return Ok(false),
    }
    Ok(true)
}

/// The measurements, the meters, the panes, the cursors, the fine steps and the math channel.
fn set_meas_math(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    match k {
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
        _ => return Ok(false),
    }
    Ok(true)
}

/// The keys of one decoder and its serial trigger.
fn set_decoder(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    match k {
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
            m.trig_bus = *BUS_WHATS
                .iter()
                .find(|&&b| bus_what_name(b) == v.trim())
                .ok_or_else(|| format!("unknown bus match {}", v))?
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
        _ => return Ok(false),
    }
    Ok(true)
}

/// Zoom, HDMI and the views.
fn set_zoom_views(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    let ntime = tscales().len();
    match k {
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
        _ => return Ok(false),
    }
    Ok(true)
}

/// The Bode plot, the spectrum, the eye, the power analysis and the magnetics.
fn set_bode_spec_power(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    match k {
        "bode" => {
            // the address is the rest of the line: it may be empty
            let w: Vec<&str> = v.trim().splitn(8, char::is_whitespace).collect();
            if w.len() < 7 {
                return Err("bode: want 8 values".into());
            }
            (m.bode_start, m.bode_stop, m.bode_amp) = (p_f64(w[0])?, p_f64(w[1])?, p_f64(w[3])?);
            m.bode_points = p_u32(w[2])?.clamp(2, 1000) as usize;
            m.bode_in = WaveId::parse(w[4])?;
            m.bode_out = WaveId::parse(w[5])?;
            m.bode_gen = p_idx(w[6], crate::ui::bode::GENERATORS.len())?;
            m.bode_addr = w.get(7).map_or("", |a| a.trim()).to_string();
        }
        "bode_sweep" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 7 {
                return Err("bode_sweep: want 7 values".into());
            }
            m.bode_sw_time = p_f64(w[0])?;
            if !m.bode_sw_time.is_finite() || m.bode_sw_time <= 0.0 {
                return Err("bode_sweep: the sweep time must be above 0".into());
            }
            m.bode_sw_log = p_bool(w[1])?;
            m.bode_sync = p_idx(w[2], NCH + 1)?.max(1) - 1;
            m.bode_sync_slope = p_enum(w[3], &crate::settings::trigger::SLOPES[..2])?;
            m.bode_sync_level = p_f32(w[4])?;
            m.bode_sw_in = p_idx(w[5], NCH + 1)?;
            m.bode_ref = p_f64(w[6])?;
        }
        "spec" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 9 {
                return Err("spec: want 9 values".into());
            }
            m.spec_src = WaveId::parse(w[0])?;
            (m.spec_center, m.spec_span, m.spec_rbw) = (p_f64(w[1])?, p_f64(w[2])?, p_f64(w[3])?);
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
        "power" => {
            let w: Vec<&str> = v.split_whitespace().collect();
            if w.len() != 3 {
                return Err("power: want 3 values".into());
            }
            m.pw_v = WaveId::parse(w[0])?;
            m.pw_i = WaveId::parse(w[1])?;
            m.pw_mode = p_enum(w[2], &crate::settings::power::MODES)?;
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
        _ => return Ok(false),
    }
    Ok(true)
}

/// The inverter, the double pulse test, zones, histogram, mask test, quick key, text size and logic lines.
fn set_inverter_zones(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    match k {
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
        "mask_src" => m.mask_src = p_idx(v, NCH)?,
        "mask_x" => m.mask_x = p_f32(v)?,
        "mask_y" => m.mask_y = p_f32(v)?,
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
        _ => return Ok(false),
    }
    Ok(true)
}

/// The measurement setup, the later lines of the analysers, the simulator and the bus.
fn set_more(m: &mut Model, k: &str, v: &str) -> Result<bool, String> {
    match k {
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
            m.spec_acpr_w = p_f64(width)?;
            m.spec_acpr_s = p_f64(space)?;
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
            m.eye_ber = p_f64(ber)?;
            m.eye_pn_lo = p_f64(lo)?;
            m.eye_pn_hi = p_f64(hi)?;
            m.eye_npj = p_bool(npj)?;
            m.eye_rail = opt(rail)?;
            m.eye_notch = p_f64(notch)?;
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
            m.pw_fline = p_f64(fline)?;
            m.pw_fsw = p_f64(fsw)?;
            m.pw_vin = opt(vin)?;
            m.pw_ref = if reference == "-" { None } else { Some(reference.parse::<f64>().map_err(|e| e.to_string())?) };
            m.pw_clk = opt(clk)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
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
