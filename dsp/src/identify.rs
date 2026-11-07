// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal identification: names the signal on each channel (or a group of channels for I2C / SPI) and suggests a
//! trigger and, for serial buses, a decoder with its parameters.
//!
//! Method: per channel, statistics and the histogram levels decide between flat (DC), two-level (logic) and analog
//! signals. Logic signals: a regular period gives clock / PWM / pulse train (with glitch or missing-pulse
//! anomalies), irregular ones get a unit interval from the edge spacings (`eye::unit_interval`, snapped to standard
//! rates ±3 %) and are confirmed by running the decoders (CAN CRC, LIN parity / checksum, UART stop bits); 1-Wire by its
//! reset / slot timing, Manchester by runs of only 1 and 2 units. Analog signals: `spectrum::distortion` gives the
//! fundamental, harmonics and noise (sine / mains / triangle / sawtooth / noise), the envelope and the per-cycle
//! frequency give AM / FM, gaps in the activity give bursts; composite video by sync pulses at the line rate. Groups:
//! I2C (two lines idle high, SDA changes only while SCL is low except START / STOP, confirmed by the decoder) and SPI
//! (a clock regular within bursts, data lines changing next to clock edges, an optional chip select low around the
//! bursts, mode from the clock idle level and the edge the data changes on).

use crate::decode::{self, Kind, Parity};
use crate::edges::{edges, states, Edge};
use crate::eye::unit_interval;
use crate::measure::analyze;
use crate::spectrum::distortion;

#[derive(Clone, Debug, PartialEq)]
pub enum SignalKind {
    Dc { level: f64 },
    Noise { rms: f64 },
    /// amplitude = peak, V
    Sine { freq: f64, amp: f64 },
    Mains { freq: f64, vrms: f64 },
    /// square wave / clock
    Clock { freq: f64, duty: f64, lo: f64, hi: f64 },
    Pwm { freq: f64, duty_min: f64, duty_max: f64 },
    Triangle { freq: f64, amp: f64 },
    Sawtooth { freq: f64, amp: f64, rising: bool },
    /// periodic, but none of the above (amplitude = peak of the fundamental)
    Periodic { freq: f64, amp: f64 },
    /// narrow periodic pulses (width = typical pulse width; `positive` = high pulses); `anomalies` = glitches /
    /// missing pulses
    PulseTrain { freq: f64, width: f64, positive: bool, anomalies: usize },
    /// a single step (fewer than 3 edges)
    Step { rising: bool, lo: f64, hi: f64 },
    /// bursts of a signal of `freq`, each `length` s, repeating every `period` s (NaN with a single burst)
    Burst { freq: f64, length: f64, period: f64 },
    Am { carrier: f64, modulation: f64, depth: f64 },
    Fm { carrier: f64, modulation: f64, deviation: f64 },
    Uart { baud: f64, idle_high: bool },
    I2c { rate: f64 },
    Spi { rate: f64, mode: u8, bits: u32 },
    Can { bitrate: f64, dominant_low: bool },
    Lin { baud: f64 },
    OneWire,
    Manchester { bitrate: f64 },
    /// serial data of an unknown protocol (`rate` = 1 / unit interval)
    Data { rate: f64 },
    /// composite video; line rate 15.625 kHz (PAL) or 15.734 kHz (NTSC)
    Video { line_rate: f64, ntsc: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub enum TriggerHint {
    /// free running / auto (DC, noise)
    None,
    /// edge trigger; `holdoff` s (0 = none, set for bursts)
    Edge { source: usize, level: f32, rising: bool, holdoff: f64 },
    /// pulse width trigger: pulses (`positive` = high) shorter than `less_than` or longer than `greater_than` s
    PulseWidth { source: usize, level: f32, positive: bool, less_than: Option<f64>, greater_than: Option<f64> },
    Video { source: usize, ntsc: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DecoderHint {
    Uart { ch: usize, baud: f64, bits: u32, parity: Parity, thr: f32, invert: bool },
    I2c { sda: usize, scl: usize, thr: f32 },
    Spi { clk: usize, data: Vec<usize>, cs: Option<usize>, mode: u8, bits: u32, thr: f32 },
    Can { ch: usize, bitrate: f64, thr: f32, invert: bool },
    Lin { ch: usize, baud: f64, thr: f32 },
    OneWire { ch: usize, thr: f32 },
    Manchester { ch: usize, bitrate: f64, thr: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub channels: Vec<usize>,
    pub kind: SignalKind,
    /// 0..1
    pub confidence: f32,
    pub summary: String,
    pub trigger: TriggerHint,
    pub decoder: Option<DecoderHint>,
}

/// Standard UART / CAN / LIN / clock rates that estimates are snapped to (±3 %).
pub const STANDARD_RATES: [f64; 42] = [
    50.0, 75.0, 110.0, 150.0, 300.0, 600.0, 1200.0, 2400.0, 4800.0, 9600.0, 10_000.0, 10_400.0, 14_400.0, 19_200.0, 20_000.0, 28_800.0, 31_250.0, 33_333.0, 38_400.0, 50_000.0, 57_600.0,
    76_800.0, 83_333.0, 100_000.0, 115_200.0, 125_000.0, 230_400.0, 250_000.0, 400_000.0, 460_800.0, 500_000.0, 800_000.0, 921_600.0, 1e6, 1.5e6, 2e6, 2.5e6, 3e6, 4e6, 5e6, 8e6, 10e6,
];

/// The nearest standard rate within ±3 %, else `r` itself.
pub fn snap_rate(r: f64) -> f64 {
    STANDARD_RATES.iter().copied().filter(|&s| (r / s - 1.0).abs() <= 0.03).min_by(|a, b| (r / a - 1.0).abs().total_cmp(&(r / b - 1.0).abs())).unwrap_or(r)
}

/// Bit rate of a serial signal from its pulse widths (`eye::unit_interval`, the GCD of the edge spacings), snapped
/// to a standard rate.
pub fn estimate_baud(v: &[f32], dt: f64, thr: f32, hyst: f32) -> Option<f64> {
    let e = edges(v, thr, hyst);
    let d: Vec<f64> = e.windows(2).map(|w| (w[1].t - w[0].t) * dt).collect();
    unit_interval(&d).map(|u| snap_rate(1.0 / u))
}

/// Value with an SI prefix and 4 significant digits ("1.000 kHz").
fn si(x: f64, unit: &str) -> String {
    if !x.is_finite() {
        return format!("? {}", unit);
    }
    if x.abs() < 1e-12 {
        return format!("0 {}", unit);
    }
    let p = [(1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "µ"), (1e-9, "n"), (1e-12, "p")];
    let (s, pre) = p.iter().copied().find(|(s, _)| x.abs() >= *s * 0.9995).unwrap_or((1e-12, "p"));
    let y = x / s;
    let dec = if y.abs() >= 100.0 { 1 } else if y.abs() >= 10.0 { 2 } else { 3 };
    format!("{:.*} {}{}", dec, y, pre, unit)
}

fn median(x: &mut [f64]) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    x.sort_by(f64::total_cmp);
    x[x.len() / 2]
}

/// Per-channel features.
struct Chan<'a> {
    idx: usize,
    v: &'a [f32],
    dt: f64,
    n: usize,
    min: f64,
    max: f64,
    mean: f64,
    std: f64,
    /// histogram levels
    lo: f64,
    hi: f64,
    logic: bool,
    thr: f32,
    hyst: f32,
    /// edges at thr (times in samples)
    e: Vec<Edge>,
}

impl<'a> Chan<'a> {
    fn new(idx: usize, v: &'a [f32], dt: f64) -> Chan<'a> {
        let (mut n, mut s, mut s2, mut min, mut max) = (0usize, 0.0, 0.0, f64::INFINITY, f64::NEG_INFINITY);
        for &x in v {
            if x.is_finite() {
                let x = x as f64;
                n += 1;
                s += x;
                s2 += x * x;
                min = min.min(x);
                max = max.max(x);
            }
        }
        let mean = if n > 0 { s / n as f64 } else { f64::NAN };
        let std = if n > 0 { (s2 / n as f64 - mean * mean).max(0.0).sqrt() } else { f64::NAN };
        let a = analyze(v, dt);
        let (lo, hi) = if a.valid { (a.base, a.top) } else { (min, max) };
        let amp = hi - lo;
        let near = if amp > 0.0 { v.iter().filter(|x| x.is_finite() && ((**x as f64 - hi).abs() < 0.15 * amp || (**x as f64 - lo).abs() < 0.15 * amp)).count() } else { 0 };
        let logic = n > 0 && amp > 0.0 && near as f64 >= 0.8 * n as f64 && amp >= 0.6 * (max - min);
        let thr = ((lo + hi) / 2.0) as f32;
        let hyst = (0.2 * amp) as f32;
        let e = if amp > 0.0 && logic { edges(v, thr, hyst) } else { Vec::new() };
        Chan { idx, v, dt, n, min, max, mean, std, lo, hi, logic, thr, hyst, e }
    }
    fn mid(&self) -> f32 {
        ((self.min + self.max) / 2.0) as f32
    }
    /// Durations (s) of the high (`high` = true) or low pulses between edges.
    fn widths(&self, high: bool) -> Vec<f64> {
        self.e.windows(2).filter(|w| w[0].rising == high && w[1].rising != high).map(|w| (w[1].t - w[0].t) * self.dt).collect()
    }
    /// Level of the longest run (the idle level of a bus), including the runs before the first / after the last edge.
    fn idle_high(&self) -> bool {
        let Some(first) = self.e.first() else { return self.mean > (self.lo + self.hi) / 2.0 };
        let last = self.e[self.e.len() - 1];
        let mut best = (first.t, !first.rising);
        if (self.n as f64 - last.t) > best.0 {
            best = (self.n as f64 - last.t, last.rising);
        }
        for w in self.e.windows(2) {
            if w[1].t - w[0].t > best.0 {
                best = (w[1].t - w[0].t, w[0].rising);
            }
        }
        best.1
    }
    fn edge_trigger(&self, rising: bool) -> TriggerHint {
        TriggerHint::Edge { source: self.idx, level: self.thr, rising, holdoff: 0.0 }
    }
}

fn finding(ch: &[usize], kind: SignalKind, confidence: f32, summary: String, trigger: TriggerHint, decoder: Option<DecoderHint>) -> Finding {
    Finding { channels: ch.to_vec(), kind, confidence: confidence.clamp(0.0, 1.0), summary, trigger, decoder }
}

/// Identifies the signals on the channels `ch` (all sampled every `dt`); findings sorted by confidence, highest first.
/// Every channel gets one finding, except channels that belong to an I2C / SPI group (one finding for the group).
pub fn identify(ch: &[&[f32]], dt: f64) -> Vec<Finding> {
    if !(dt > 0.0) || !dt.is_finite() {
        return Vec::new();
    }
    let chans: Vec<Chan> = ch.iter().enumerate().map(|(i, v)| Chan::new(i, v, dt)).collect();
    let mut out = Vec::new();
    let mut claimed = vec![false; chans.len()];
    if let Some((f, used)) = find_i2c(&chans) {
        used.iter().for_each(|&u| claimed[u] = true);
        out.push(f);
    }
    if let Some((f, used)) = find_spi(&chans, &claimed) {
        used.iter().for_each(|&u| claimed[u] = true);
        out.push(f);
    }
    for c in &chans {
        if !claimed[c.idx] {
            if let Some(f) = single(c) {
                out.push(f);
            }
        }
    }
    out.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    out
}

/// One channel on its own.
fn single(c: &Chan) -> Option<Finding> {
    if c.n < 16 {
        return None;
    }
    let pp = c.max - c.min;
    if pp < 0.01 || c.std < 0.002 * c.mean.abs() {
        return Some(finding(&[c.idx], SignalKind::Dc { level: c.mean }, 0.95, format!("DC {}", si(c.mean, "V")), TriggerHint::None, None));
    }
    if let Some(f) = video(c) {
        return Some(f);
    }
    if c.logic {
        return Some(logic(c));
    }
    Some(analog(c))
}

// ---------------------------------------------------------------------------------------------------------------
// video

fn video(c: &Chan) -> Option<Finding> {
    if c.dt > 1e-6 || (c.n as f64 * c.dt) < 5.0 * 64e-6 {
        return None;
    }
    let pp = c.max - c.min;
    let thr = (c.min + 0.15 * pp) as f32;
    let e = edges(c.v, thr, (0.05 * pp) as f32);
    let falls: Vec<f64> = e.iter().filter(|x| !x.rising).map(|x| x.t * c.dt).collect();
    if falls.len() < 6 {
        return None;
    }
    let d: Vec<f64> = falls.windows(2).map(|w| w[1] - w[0]).collect();
    let lines: Vec<f64> = d.iter().copied().filter(|&x| x > 60e-6 && x < 68e-6).collect();
    let halves = d.iter().filter(|&&x| x > 30e-6 && x < 34e-6).count();
    if lines.len() < 4 || ((lines.len() + halves) as f64) < 0.8 * d.len() as f64 {
        return None;
    }
    // sync pulses are short (H sync 4.7 us, equalising 2.35 us; broad pulses are long but rare)
    let lows: Vec<f64> = e.windows(2).filter(|w| !w[0].rising && w[1].rising).map(|w| (w[1].t - w[0].t) * c.dt).collect();
    if (lows.iter().filter(|&&w| w < 8e-6).count() as f64) < 0.7 * lows.len() as f64 {
        return None;
    }
    let line = lines.iter().sum::<f64>() / lines.len() as f64;
    let rate = 1.0 / line;
    let ntsc = (rate - 15_734.0).abs() < (rate - 15_625.0).abs();
    let summary = format!("Composite video {}, line rate {}", if ntsc { "NTSC" } else { "PAL" }, si(rate, "Hz"));
    Some(finding(&[c.idx], SignalKind::Video { line_rate: rate, ntsc }, 0.9, summary, TriggerHint::Video { source: c.idx, ntsc }, None))
}

// ---------------------------------------------------------------------------------------------------------------
// logic signals

fn logic(c: &Chan) -> Finding {
    let id = [c.idx];
    let levels = format!("{} .. {}", si(c.lo, "V"), si(c.hi, "V"));
    if c.e.len() < 3 {
        let rising = c.e.first().map_or(c.mean > c.thr as f64, |e| e.rising);
        return finding(&id, SignalKind::Step { rising, lo: c.lo, hi: c.hi }, 0.5, format!("Single {} edge, {}", if rising { "rising" } else { "falling" }, levels), c.edge_trigger(rising), None);
    }
    // a clean clock / PWM / pulse train wins; a pulse train with anomalies may be serial data with a repetitive
    // pattern (0x55 sync bytes), so a decoder-confirmed protocol beats it
    let periodic = periodic_logic(c, &levels);
    if let Some(f) = periodic.as_ref().filter(|f| !matches!(f.kind, SignalKind::PulseTrain { anomalies: 1.., .. })) {
        return f.clone();
    }
    let (decoded, other) = serial(c);
    if let Some(f) = decoded.or(periodic) {
        return f;
    }
    if let Some(f) = one_wire(c) {
        return f;
    }
    if let Some(f) = other {
        return f;
    }
    if let Some(f) = burst_clock(c) {
        return f;
    }
    let rate = c.e.len() as f64 / (c.n as f64 * c.dt);
    finding(&id, SignalKind::Data { rate: f64::NAN }, 0.3, format!("Irregular digital signal, {} edges/s, {}", si(rate, ""), levels), c.edge_trigger(true), None)
}

/// Clock, PWM or pulse train: the rising edges are regular (≥ 80 % of the periods within ±3 % of the median).
fn periodic_logic(c: &Chan, levels: &str) -> Option<Finding> {
    let r: Vec<f64> = c.e.iter().filter(|e| e.rising).map(|e| e.t * c.dt).collect();
    if r.len() < 4 {
        return None;
    }
    let per: Vec<f64> = r.windows(2).map(|w| w[1] - w[0]).collect();
    let p = median(&mut per.clone());
    let regular = per.iter().filter(|&&x| (x / p - 1.0).abs() < 0.03).count();
    if (regular as f64) < 0.8 * per.len() as f64 {
        return None;
    }
    let mut missing = per.len() - regular;
    // high time of every regular period
    let mut duty = Vec::new();
    for (k, w) in r.windows(2).enumerate() {
        if (per[k] / p - 1.0).abs() >= 0.03 {
            continue;
        }
        let j = c.e.partition_point(|e| e.t * c.dt <= w[0]);
        if let Some(f) = c.e[j..].iter().find(|e| !e.rising) {
            if f.t * c.dt < w[1] {
                duty.push((f.t * c.dt - w[0]) / per[k]);
            }
        }
    }
    if duty.is_empty() {
        return None;
    }
    let dmed = median(&mut duty.clone());
    let typical = duty.iter().filter(|&&d| (d - dmed).abs() < 0.02).count();
    let (dmin, dmax) = duty.iter().fold((1.0f64, 0.0f64), |(a, b), &d| (a.min(d), b.max(d)));
    let freq = 1.0 / p;
    let id = [c.idx];
    let positive = dmed <= 0.5;
    let width = if positive { dmed * p } else { (1.0 - dmed) * p };
    // glitches: a few pulses much narrower than the rest
    let glitches = duty.iter().filter(|&&d| if positive { d < 0.5 * dmed } else { 1.0 - d < 0.5 * (1.0 - dmed) }).count();
    if typical as f64 >= 0.7 * duty.len() as f64 && (glitches > 0 || missing > 0) {
        missing += glitches;
        let trigger = if glitches > 0 {
            TriggerHint::PulseWidth { source: c.idx, level: c.thr, positive, less_than: Some(0.5 * width), greater_than: None }
        } else {
            TriggerHint::PulseWidth { source: c.idx, level: c.thr, positive: !positive, less_than: None, greater_than: Some(1.5 * (p - width)) }
        };
        let summary = format!("Pulse train {}, width {}, {} anomal{}", si(freq, "Hz"), si(width, "s"), missing, if missing == 1 { "y" } else { "ies" });
        return Some(finding(&id, SignalKind::PulseTrain { freq, width, positive, anomalies: missing }, 0.8, summary, trigger, None));
    }
    if dmax - dmin > 0.05 {
        let summary = format!("PWM {}, duty {:.1} .. {:.1} %, {}", si(freq, "Hz"), dmin * 100.0, dmax * 100.0, levels);
        return Some(finding(&id, SignalKind::Pwm { freq, duty_min: dmin, duty_max: dmax }, 0.85, summary, c.edge_trigger(true), None));
    }
    if dmed < 0.2 || dmed > 0.8 {
        let summary = format!("Pulse train {}, width {}, {}", si(freq, "Hz"), si(width, "s"), levels);
        return Some(finding(&id, SignalKind::PulseTrain { freq, width, positive, anomalies: 0 }, 0.85, summary, c.edge_trigger(positive), None));
    }
    let summary = format!("Clock {}, duty {:.1} %, {}", si(freq, "Hz"), dmed * 100.0, levels);
    Some(finding(&id, SignalKind::Clock { freq, duty: dmed, lo: c.lo, hi: c.hi }, 0.9, summary, c.edge_trigger(true), None))
}

/// 1-Wire: starts high (idle), a reset pulse (low 400 µs .. 1 ms) and short low slots (< 120 µs).
fn one_wire(c: &Chan) -> Option<Finding> {
    if c.e.first().map_or(true, |e| e.rising) {
        return None;
    }
    let lows = c.widths(false);
    let resets = lows.iter().filter(|&&w| (400e-6..1e-3).contains(&w)).count();
    let slots = lows.iter().filter(|&&w| w < 120e-6 && w > 0.5e-6).count();
    let presence = lows.iter().filter(|&&w| (120e-6..400e-6).contains(&w)).count();
    if resets == 0 || slots < 4 || resets + slots + presence < lows.len() {
        return None;
    }
    let summary = format!("1-Wire, {} reset{}, {} slots", resets, if resets == 1 { "" } else { "s" }, slots);
    let trig = TriggerHint::PulseWidth { source: c.idx, level: c.thr, positive: false, less_than: None, greater_than: Some(400e-6) };
    Some(finding(&[c.idx], SignalKind::OneWire, 0.8, summary, trig, Some(DecoderHint::OneWire { ch: c.idx, thr: c.thr })))
}

/// Serial data with a unit interval: (CAN / LIN / UART confirmed by the decoders, else Manchester or unknown data).
fn serial(c: &Chan) -> (Option<Finding>, Option<Finding>) {
    let d: Vec<f64> = c.e.windows(2).map(|w| (w[1].t - w[0].t) * c.dt).collect();
    if d.len() < 8 {
        return (None, None);
    }
    let Some(u) = unit_interval(&d) else { return (None, None) };
    let short: Vec<f64> = d.iter().map(|x| x / u).filter(|&x| x < 12.5).collect();
    let fit = short.iter().filter(|x| (*x - x.round()).abs() < 0.2).count();
    if short.len() < 8 || (fit as f64) < 0.9 * short.len() as f64 || u < 2.5 * c.dt {
        return (None, None);
    }
    let baud = snap_rate(1.0 / u);
    let idle_high = c.idle_high();
    let id = [c.idx];
    let (thr, hyst) = (c.thr, c.hyst);
    // CAN: CRCs must match
    let can = decode::can(c.v, c.dt, baud, thr, hyst, !idle_high);
    let crc_ok = can.iter().filter(|p| p.kind == Kind::Crc && p.ok).count();
    let crcs = can.iter().filter(|p| p.kind == Kind::Crc).count();
    if crc_ok >= 1 && 2 * crc_ok >= crcs {
        let summary = format!("CAN {}, {} frame{}", si(baud, "bit/s"), crc_ok, if crc_ok == 1 { "" } else { "s" });
        return (Some(finding(&id, SignalKind::Can { bitrate: baud, dominant_low: idle_high }, 0.95, summary, c.edge_trigger(!idle_high), Some(DecoderHint::Can { ch: c.idx, bitrate: baud, thr, invert: !idle_high }))), None);
    }
    // LIN: break + sync + protected ID with good parity
    if idle_high {
        let lin = decode::lin(c.v, c.dt, baud, thr, hyst);
        let pid_ok = lin.iter().filter(|p| matches!(p.kind, Kind::Id { .. }) && p.ok).count();
        if pid_ok >= 1 && lin.iter().any(|p| p.kind == Kind::Break) {
            let trig = TriggerHint::PulseWidth { source: c.idx, level: thr, positive: false, less_than: None, greater_than: Some(10.0 / baud) };
            let summary = format!("LIN {} Bd, {} frame{}", si(baud, ""), pid_ok, if pid_ok == 1 { "" } else { "s" });
            return (Some(finding(&id, SignalKind::Lin { baud }, 0.95, summary, trig, Some(DecoderHint::Lin { ch: c.idx, baud, thr }))), None);
        }
    }
    // UART 8N1: stop bits must be high
    let uart = decode::uart(c.v, c.dt, baud, 8, Parity::None, thr, hyst, !idle_high);
    let ok = uart.iter().filter(|p| p.ok).count();
    if uart.len() >= 2 && ok as f64 >= 0.9 * uart.len() as f64 {
        let conf = 0.6 + 0.3 * (uart.len().min(10) as f32 / 10.0);
        let summary = format!("UART {} Bd 8N1, idle {}, {} characters", si(baud, "").trim_end(), if idle_high { "high" } else { "low (inverted)" }, uart.len());
        return (Some(finding(&id, SignalKind::Uart { baud, idle_high }, conf, summary, c.edge_trigger(!idle_high), Some(DecoderHint::Uart { ch: c.idx, baud, bits: 8, parity: Parity::None, thr, invert: !idle_high }))), None);
    }
    // Manchester / biphase: inside the active part only runs of 1 and 2 half-bits (3 rarely: MIL-STD-1553 sync)
    let runs: Vec<i64> = short.iter().map(|x| x.round() as i64).collect();
    let (r1, r2, r3) = (runs.iter().filter(|&&r| r == 1).count(), runs.iter().filter(|&&r| r == 2).count(), runs.iter().filter(|&&r| r == 3).count());
    let long_in_burst = runs.iter().filter(|&&r| r > 3 && r < 8).count();
    if r1 + r2 + r3 + long_in_burst >= runs.len() && long_in_burst == 0 && r1 * 10 >= runs.len() && r2 * 10 >= runs.len() && r3 * 20 <= runs.len() {
        let bitrate = snap_rate(0.5 / u);
        let summary = format!("Manchester {}", si(bitrate, "bit/s"));
        return (None, Some(finding(&id, SignalKind::Manchester { bitrate }, 0.75, summary, c.edge_trigger(true), Some(DecoderHint::Manchester { ch: c.idx, bitrate, thr }))));
    }
    let summary = format!("Serial data, unit interval {} ({} bit/s)", si(u, "s"), si(1.0 / u, "").trim_end());
    (None, Some(finding(&id, SignalKind::Data { rate: 1.0 / u }, 0.4, summary, c.edge_trigger(!idle_high), None)))
}

/// A clock that runs in bursts: edge spacings inside the bursts equal (±15 %), separated by gaps of > 3 spacings.
fn burst_clock(c: &Chan) -> Option<Finding> {
    let d: Vec<f64> = c.e.windows(2).map(|w| (w[1].t - w[0].t) * c.dt).collect();
    let m = median(&mut d.clone());
    let inner: Vec<f64> = d.iter().copied().filter(|&x| x <= 3.0 * m).collect();
    let gaps: Vec<usize> = (0..d.len()).filter(|&k| d[k] > 3.0 * m).collect();
    if gaps.is_empty() || (inner.iter().filter(|&&x| (x / m - 1.0).abs() < 0.15).count() as f64) < 0.9 * inner.len() as f64 {
        return None;
    }
    let (freq, length, period) = burst_stats(&c.e, &gaps, c.dt, 0.5 / m);
    let summary = format!("Clock bursts {}, {} long", si(freq, "Hz"), si(length, "s"));
    let trig = TriggerHint::Edge { source: c.idx, level: c.thr, rising: true, holdoff: 1.1 * length };
    Some(finding(&[c.idx], SignalKind::Burst { freq, length, period }, 0.6, summary, trig, None))
}

/// (freq, mean burst length, burst period) from edges split at the spacing indices `gaps`.
fn burst_stats(e: &[Edge], gaps: &[usize], dt: f64, freq: f64) -> (f64, f64, f64) {
    let mut starts = vec![e[0].t];
    let mut lens = Vec::new();
    let mut s = e[0].t;
    for &g in gaps {
        lens.push((e[g].t - s) * dt);
        s = e[g + 1].t;
        starts.push(s);
    }
    lens.push((e[e.len() - 1].t - s) * dt);
    let length = lens.iter().sum::<f64>() / lens.len() as f64;
    let period = if starts.len() >= 2 { (starts[starts.len() - 1] - starts[0]) * dt / (starts.len() - 1) as f64 } else { f64::NAN };
    (freq, length, period)
}

// ---------------------------------------------------------------------------------------------------------------
// analog signals

/// Modulation frequency of a sequence x sampled at times t (rising crossings of its mean, hysteresis 20 % of pp).
fn mod_freq(x: &[f32], t: &[f64]) -> f64 {
    let (lo, hi) = x.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let mean = x.iter().sum::<f32>() / x.len().max(1) as f32;
    let r: Vec<f64> = edges(x, mean, 0.2 * (hi - lo)).iter().filter(|e| e.rising).map(|e| {
        let k = (e.t.floor() as usize).min(t.len().saturating_sub(2));
        t[k] + (t[k + 1] - t[k]) * (e.t - k as f64)
    }).collect();
    if r.len() < 2 {
        return f64::NAN;
    }
    (r.len() - 1) as f64 / (r[r.len() - 1] - r[0])
}

/// A modulation sequence must be smooth: RMS of its first difference < 0.6 x its standard deviation (≥ 13 samples
/// per modulation period; noise gives about 1.4).
fn smooth(x: &[f32]) -> bool {
    if x.len() < 8 {
        return false;
    }
    let m = x.iter().map(|&v| v as f64).sum::<f64>() / x.len() as f64;
    let sd = (x.iter().map(|&v| (v as f64 - m).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
    let dd = (x.windows(2).map(|w| (w[1] as f64 - w[0] as f64).powi(2)).sum::<f64>() / (x.len() - 1) as f64).sqrt();
    sd > 0.0 && dd < 0.6 * sd
}

/// AM: envelope = maximum of |v - mean| per carrier cycle; (depth, modulation frequency).
fn am(c: &Chan, f: f64) -> (f64, f64) {
    let spc = 1.0 / (f * c.dt);
    if spc < 4.0 || c.n as f64 / spc < 8.0 {
        return (0.0, f64::NAN);
    }
    let mut env = Vec::new();
    let mut tt = Vec::new();
    let mut k = 0.0;
    while (k + spc) as usize <= c.v.len() {
        let blk = &c.v[k as usize..(k + spc) as usize];
        let m = blk.iter().filter(|x| x.is_finite()).fold(0.0f64, |a, &x| a.max((x as f64 - c.mean).abs()));
        env.push(m as f32);
        tt.push((k + spc / 2.0) * c.dt);
        k += spc;
    }
    let mut s = env.iter().map(|&x| x as f64).collect::<Vec<_>>();
    s.sort_by(f64::total_cmp);
    let (lo, hi) = (s[s.len() / 50], s[s.len() - 1 - s.len() / 50]);
    let depth = if hi + lo > 0.0 && smooth(&env) { (hi - lo) / (hi + lo) } else { 0.0 };
    (depth, mod_freq(&env, &tt))
}

/// FM: frequency per carrier cycle from the rising crossings of the mean; (mean frequency = carrier, peak deviation
/// Hz, modulation frequency).
fn fm(c: &Chan) -> (f64, f64, f64) {
    let r: Vec<f64> = edges(c.v, c.mean as f32, (0.2 * c.std) as f32).iter().filter(|e| e.rising).map(|e| e.t * c.dt).collect();
    if r.len() < 16 {
        return (f64::NAN, 0.0, f64::NAN);
    }
    let carrier = (r.len() - 1) as f64 / (r[r.len() - 1] - r[0]);
    let fi: Vec<f32> = r.windows(2).map(|w| (1.0 / (w[1] - w[0])) as f32).collect();
    let tm: Vec<f64> = r.windows(2).map(|w| 0.5 * (w[0] + w[1])).collect();
    let mut s: Vec<f64> = fi.iter().map(|&x| x as f64).collect();
    s.sort_by(f64::total_cmp);
    let dev = if smooth(&fi) { 0.5 * (s[s.len() - 1 - s.len() / 50] - s[s.len() / 50]) } else { 0.0 };
    (carrier, dev, mod_freq(&fi, &tm))
}

/// Bursts in an analog signal: samples beyond 10 % of pp from the median are active; gaps between active samples
/// longer than 20 x the 95th percentile gap (and > 1 % of the record) separate bursts.
fn analog_burst(c: &Chan) -> Option<Finding> {
    let mut sorted: Vec<f64> = c.v.iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
    let med = median(&mut sorted);
    let thr = 0.1 * (c.max - c.min);
    let act: Vec<usize> = (0..c.v.len()).filter(|&k| c.v[k].is_finite() && (c.v[k] as f64 - med).abs() > thr).collect();
    if act.len() < 16 {
        return None;
    }
    let g: Vec<usize> = act.windows(2).map(|w| w[1] - w[0]).collect();
    let mut gs = g.clone();
    gs.sort_unstable();
    let p95 = gs[gs.len() * 95 / 100].max(1);
    let lim = (20 * p95).max(c.n / 100);
    let cuts: Vec<usize> = (0..g.len()).filter(|&k| g[k] > lim).collect();
    let idle = act[0] + (c.n - 1 - act[act.len() - 1]) + cuts.iter().map(|&k| g[k]).sum::<usize>();
    if cuts.is_empty() && (idle as f64) < 0.3 * c.n as f64 {
        return None;
    }
    // inner frequency from the crossings of the median inside the bursts
    let x = edges(c.v, med as f32, thr as f32);
    let rising = x.iter().filter(|e| e.rising).count() as f64;
    let active_time = (c.n - idle) as f64 * c.dt;
    let freq = rising / active_time.max(c.dt);
    let e: Vec<Edge> = act.iter().map(|&k| Edge { t: k as f64, rising: true }).collect();
    let (freq, length, period) = burst_stats(&e, &cuts, c.dt, freq);
    let summary = format!("Bursts of {}, {} long{}", si(freq, "Hz"), si(length, "s"), if period.is_finite() { format!(", every {}", si(period, "s")) } else { String::new() });
    let trig = TriggerHint::Edge { source: c.idx, level: (med + thr) as f32, rising: true, holdoff: 1.1 * length };
    Some(finding(&[c.idx], SignalKind::Burst { freq, length, period }, 0.7, summary, trig, None))
}

fn analog(c: &Chan) -> Finding {
    let id = [c.idx];
    let trig = TriggerHint::Edge { source: c.idx, level: c.mid(), rising: true, holdoff: 0.0 };
    let noise = || finding(&id, SignalKind::Noise { rms: c.std }, 0.6, format!("Noise, {} rms", si(c.std, "V")), TriggerHint::None, None);
    if let Some(b) = analog_burst(c) {
        return b;
    }
    let Some(d) = distortion(c.v, c.dt, None, 9) else { return noise() };
    let f = d.f1;
    let p1 = 10f64.powf(d.a1_dbv / 10.0);
    let amp = (2.0 * p1).sqrt();
    let frac = p1 / (c.std * c.std).max(1e-30);
    let (depth, fam) = am(c, f);
    if depth > 0.05 && fam.is_finite() && fam < f / 4.0 && frac > 0.3 {
        let summary = format!("AM carrier {}, modulation {}, depth {:.0} %", si(f, "Hz"), si(fam, "Hz"), depth * 100.0);
        return finding(&id, SignalKind::Am { carrier: f, modulation: fam, depth }, 0.75, summary, trig, None);
    }
    let (fc, dev, ffm) = fm(c);
    if dev > 0.005 * fc && ffm.is_finite() && ffm < fc / 4.0 {
        let summary = format!("FM carrier {}, modulation {}, deviation {}", si(fc, "Hz"), si(ffm, "Hz"), si(dev, "Hz"));
        return finding(&id, SignalKind::Fm { carrier: fc, modulation: ffm, deviation: dev }, 0.7, summary, trig, None);
    }
    // periodic: the fundamental and its harmonics carry most of the power
    if frac < 0.2 || frac * (1.0 + (d.thd_pct / 100.0).powi(2)) < 0.5 {
        return noise();
    }
    let h = |k: usize| d.harmonics.get(k - 2).map_or(-200.0, |x| x.1);
    let conf = (0.5 + (d.snr_db as f32 - 10.0) / 60.0).clamp(0.5, 0.95);
    if (45.0..65.0).contains(&f) && d.thd_pct < 20.0 {
        let vrms = (p1 + p1 * (d.thd_pct / 100.0).powi(2)).sqrt();
        let summary = format!("Mains {}, {} rms, THD {:.1} %", si(f, "Hz"), si(vrms, "V"), d.thd_pct);
        return finding(&id, SignalKind::Mains { freq: f, vrms }, conf, summary, trig, None);
    }
    if d.thd_pct < 5.0 {
        let summary = format!("Sine {}, {} peak, THD {:.2} %", si(f, "Hz"), si(amp, "V"), d.thd_pct);
        return finding(&id, SignalKind::Sine { freq: f, amp }, conf, summary, trig, None);
    }
    // triangle: odd harmonics 1/n² (-19.1, -28.0 dBc); sawtooth: all harmonics 1/n (-6.0, -9.5 dBc)
    if h(2) < -30.0 && (h(3) + 19.08).abs() < 2.0 && (h(5) + 27.96).abs() < 3.0 {
        let a = (c.max - c.min) / 2.0;
        return finding(&id, SignalKind::Triangle { freq: f, amp: a }, conf, format!("Triangle {}, {} peak", si(f, "Hz"), si(a, "V")), trig, None);
    }
    if (h(2) + 6.02).abs() < 1.5 && (h(3) + 9.54).abs() < 1.5 {
        // the slow ramp has the sign of most sample-to-sample differences
        let up = c.v.windows(2).filter(|w| w[1] > w[0]).count() * 2 > c.v.len();
        let a = (c.max - c.min) / 2.0;
        let summary = format!("Sawtooth {} ({}), {} peak", si(f, "Hz"), if up { "rising" } else { "falling" }, si(a, "V"));
        return finding(&id, SignalKind::Sawtooth { freq: f, amp: a, rising: up }, conf, summary, trig, None);
    }
    let summary = format!("Periodic {}, THD {:.0} %", si(f, "Hz"), d.thd_pct);
    finding(&id, SignalKind::Periodic { freq: f, amp }, conf * 0.6, summary, trig, None)
}

// ---------------------------------------------------------------------------------------------------------------
// channel groups

/// State of a channel at sample time t (from its comparator states).
fn state_at(s: &[Option<bool>], t: f64) -> Option<bool> {
    s.get(t.max(0.0) as usize).copied().flatten()
}

/// I2C: SDA and SCL idle high, SCL has more edges, SDA changes while SCL is high only for START / STOP (≤ 30 % of
/// its edges, at least one), and the decoder finds an address byte.
fn find_i2c(ch: &[Chan]) -> Option<(Finding, Vec<usize>)> {
    let cand: Vec<&Chan> = ch.iter().filter(|c| c.logic && c.e.len() >= 8 && c.idle_high()).collect();
    let mut best: Option<(Finding, Vec<usize>, usize)> = None;
    for sda in &cand {
        for scl in &cand {
            if sda.idx == scl.idx || scl.e.len() <= sda.e.len() {
                continue;
            }
            let ss = states(scl.v, scl.thr, scl.hyst);
            let during_high = sda.e.iter().filter(|e| state_at(&ss, e.t) == Some(true)).count();
            if during_high == 0 || during_high as f64 > 0.3 * sda.e.len() as f64 {
                continue;
            }
            let thr = (sda.thr + scl.thr) / 2.0;
            let hyst = sda.hyst.min(scl.hyst);
            let p = decode::i2c(sda.v, scl.v, thr, hyst);
            let addrs = p.iter().filter(|x| matches!(x.kind, Kind::Addr { .. })).count();
            if addrs == 0 {
                continue;
            }
            let acked = p.iter().filter(|x| matches!(x.kind, Kind::Addr { .. }) && x.ok).count();
            if best.as_ref().map_or(false, |b| b.2 >= p.len()) {
                continue;
            }
            // bit rate from the SCL period (median rising-to-rising spacing)
            let mut r: Vec<f64> = scl.e.iter().filter(|e| e.rising).map(|e| e.t).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]) * scl.dt).collect();
            let rate = 1.0 / median(&mut r);
            let summary = format!("I2C {} (SDA CH{}, SCL CH{}), {} address{}", si(snap_rate(rate), "bit/s"), sda.idx + 1, scl.idx + 1, addrs, if addrs == 1 { "" } else { "es" });
            let conf = 0.85 + 0.1 * acked as f32 / addrs as f32;
            let trig = TriggerHint::Edge { source: sda.idx, level: thr, rising: false, holdoff: 0.0 };
            let f = finding(&[sda.idx, scl.idx], SignalKind::I2c { rate: snap_rate(rate) }, conf, summary, trig, Some(DecoderHint::I2c { sda: sda.idx, scl: scl.idx, thr }));
            best = Some((f, vec![sda.idx, scl.idx], p.len()));
        }
    }
    best.map(|b| (b.0, b.1))
}

/// SPI: a clock regular within bursts (≥ 70 % of its edge spacings within ±20 % of the median), data lines whose
/// edges lie next to clock edges (≥ 80 % within 30 % of the half period), an optional chip select (few edges, low
/// around ≥ 95 % of the clock edges); CPOL = clock idle level, CPHA = data changes on the leading edge.
fn find_spi(ch: &[Chan], claimed: &[bool]) -> Option<(Finding, Vec<usize>)> {
    let free: Vec<&Chan> = ch.iter().filter(|c| c.logic && !claimed[c.idx] && c.e.len() >= 8).collect();
    let clk = free
        .iter()
        .filter(|c| {
            let mut d: Vec<f64> = c.e.windows(2).map(|w| w[1].t - w[0].t).collect();
            let m = median(&mut d);
            d.iter().filter(|&&x| (x / m - 1.0).abs() < 0.2).count() as f64 >= 0.7 * d.len() as f64
        })
        .max_by_key(|c| c.e.len())?;
    let mut d: Vec<f64> = clk.e.windows(2).map(|w| w[1].t - w[0].t).collect();
    let half = median(&mut d);
    let ce: Vec<f64> = clk.e.iter().map(|e| e.t).collect();
    let nearest = |t: f64| -> Option<usize> {
        let i = ce.partition_point(|&x| x < t);
        [i.checked_sub(1), (i < ce.len()).then_some(i)].into_iter().flatten().min_by(|&a, &b| (ce[a] - t).abs().total_cmp(&(ce[b] - t).abs()))
    };
    // chip select: few edges, low while the clock runs
    let cs = ch
        .iter()
        .filter(|c| c.logic && !claimed[c.idx] && c.idx != clk.idx && !c.e.is_empty() && c.e.len() * 8 <= clk.e.len().max(8))
        .filter_map(|c| {
            let s = states(c.v, c.thr, c.hyst);
            let low = clk.e.iter().filter(|e| state_at(&s, e.t) == Some(false)).count();
            (low as f64 >= 0.95 * clk.e.len() as f64).then_some((c, s))
        })
        .max_by_key(|(c, _)| c.e.len());
    // without a chip select the clock must run in bursts (a free-running clock is no SPI)
    if cs.is_none() && !d.iter().any(|&x| x > 3.0 * half) {
        return None;
    }
    // clock idle level: when CS is high, else at the record start
    let cpol = match &cs {
        Some((_, s)) => {
            let ks = states(clk.v, clk.thr, clk.hyst);
            let hi: Vec<bool> = (0..clk.v.len()).filter(|&k| s[k] == Some(true)).filter_map(|k| ks[k]).collect();
            if hi.is_empty() {
                !clk.e[0].rising
            } else {
                hi.iter().filter(|&&b| b).count() * 2 > hi.len()
            }
        }
        None => !clk.e[0].rising,
    };
    let leading = |k: usize| clk.e[k].rising != cpol;
    let mut data = Vec::new();
    let mut lead_votes = (0usize, 0usize);
    for c in ch.iter().filter(|c| c.logic && !claimed[c.idx] && c.e.len() >= 2) {
        if c.idx == clk.idx || cs.as_ref().is_some_and(|(x, _)| x.idx == c.idx) {
            continue;
        }
        let near: Vec<usize> = c.e.iter().filter_map(|e| nearest(e.t).filter(|&k| (ce[k] - e.t).abs() <= 0.3 * half)).collect();
        if near.len() as f64 >= 0.8 * c.e.len() as f64 {
            data.push(c.idx);
            let l = near.iter().filter(|&&k| leading(k)).count();
            lead_votes = (lead_votes.0 + l, lead_votes.1 + near.len() - l);
        }
    }
    if data.is_empty() {
        return None;
    }
    let cpha = lead_votes.0 > lead_votes.1;
    let mode = (cpol as u8) << 1 | cpha as u8;
    // bits per frame: leading clock edges per CS-low interval (or per clock burst)
    let mut counts = Vec::new();
    let mut n = 0u32;
    for k in 0..clk.e.len() {
        if k > 0 && clk.e[k].t - clk.e[k - 1].t > 3.0 * half && n > 0 {
            counts.push(n);
            n = 0;
        }
        if leading(k) {
            n += 1;
        }
    }
    if n > 0 {
        counts.push(n);
    }
    let g = counts.iter().fold(0u32, |a, &b| gcd(a, b));
    let bits = if g % 8 == 0 && g > 0 { 8 } else { g.clamp(1, 32) };
    let thr = clk.thr;
    let cs_v = cs.as_ref().map(|(c, _)| c.v);
    let p = decode::spi(clk.v, ch[data[0]].v, None, cs_v, mode, bits, true, thr, clk.hyst, 3.0 * half);
    if p.is_empty() {
        return None;
    }
    let rate = 0.5 / (half * clk.dt);
    let cs_idx = cs.as_ref().map(|(c, _)| c.idx);
    let mut chans = vec![clk.idx];
    chans.extend(&data);
    chans.extend(cs_idx);
    let summary = format!(
        "SPI mode {} {} (CLK CH{}, data {}{}), {} bits, {} words",
        mode,
        si(snap_rate(rate), "Hz"),
        clk.idx + 1,
        data.iter().map(|d| format!("CH{}", d + 1)).collect::<Vec<_>>().join("+"),
        cs_idx.map_or(String::new(), |c| format!(", CS CH{}", c + 1)),
        bits,
        p.len()
    );
    let trig = match cs_idx {
        Some(c) => TriggerHint::Edge { source: c, level: ch[c].thr, rising: false, holdoff: 0.0 },
        None => TriggerHint::Edge { source: clk.idx, level: thr, rising: !cpol, holdoff: 0.0 },
    };
    let conf = if cs_idx.is_some() { 0.9 } else { 0.75 };
    let f = finding(&chans, SignalKind::Spi { rate: snap_rate(rate), mode, bits }, conf, summary, trig, Some(DecoderHint::Spi { clk: clk.idx, data: data.clone(), cs: cs_idx, mode, bits, thr }));
    Some((f, chans))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}
