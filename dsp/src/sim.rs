// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Signal generators: functions of time (s) -> volts, for tests and the demo source.

/// Samples `f` at t0 + k dt, k = 0..n.
pub fn sample(f: impl Fn(f64) -> f64, t0: f64, dt: f64, n: usize) -> Vec<f32> {
    (0..n).map(|k| f(t0 + k as f64 * dt) as f32).collect()
}

/// Deterministic small noise (xorshift), amplitude `a` volts peak.
pub fn add_noise(v: &mut [f32], a: f32, seed: u64) {
    let mut s = seed | 1;
    for x in v.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *x += a * ((s >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0);
    }
}

/// Trapezoid transition from 0 to 1 over `tr` seconds starting at 0 (linear 10-90 % time = 0.8 tr).
fn ramp(t: f64, tr: f64) -> f64 {
    if tr <= 0.0 {
        return if t >= 0.0 { 1.0 } else { 0.0 };
    }
    (t / tr).clamp(0.0, 1.0)
}

/// One pulse of a train: high for `width` from its start, with amplitude `height` (fraction of hi - lo).
#[derive(Clone, Copy, Debug)]
pub struct Pulse {
    pub width: f64,
    pub height: f64,
}

/// Periodic pulse train with per-pulse overrides: pulse k starts at k * period (+ offset); `special(k)` may replace
/// width/height (glitches, runts, missing pulses = height 0).
pub struct PulseTrain {
    pub period: f64,
    pub width: f64,
    pub lo: f64,
    pub hi: f64,
    /// transition time (linear, 0..100 %)
    pub tr: f64,
    pub offset: f64,
    /// every `every`-th pulse (k % every == at) gets `special`
    pub every: i64,
    pub at: i64,
    pub special: Pulse,
}

impl PulseTrain {
    pub fn square(f: f64, lo: f64, hi: f64, tr: f64) -> PulseTrain {
        PulseTrain { period: 1.0 / f, width: 0.5 / f, lo, hi, tr, offset: 0.0, every: 0, at: 0, special: Pulse { width: 0.0, height: 0.0 } }
    }
    fn pulse(&self, k: i64) -> Pulse {
        if self.every > 0 && k.rem_euclid(self.every) == self.at {
            self.special
        } else {
            Pulse { width: self.width, height: 1.0 }
        }
    }
    pub fn v(&self, t: f64) -> f64 {
        let u = t - self.offset;
        let k = (u / self.period).floor() as i64;
        // contributions of this and the previous pulse (falling edge may extend into the next period)
        let mut x = 0.0;
        for kk in [k - 1, k] {
            let p = self.pulse(kk);
            let s = u - kk as f64 * self.period;
            let up = ramp(s, self.tr);
            let down = ramp(s - p.width, self.tr);
            x += p.height * (up - down);
        }
        self.lo + (self.hi - self.lo) * x
    }
}

/// Analog video standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoStd {
    Pal,
    Ntsc,
}

impl VideoStd {
    /// line period (s)
    pub fn line(self) -> f64 {
        match self {
            VideoStd::Pal => 64e-6,
            VideoStd::Ntsc => 63.5556e-6,
        }
    }
    pub fn lines(self) -> u32 {
        match self {
            VideoStd::Pal => 625,
            VideoStd::Ntsc => 525,
        }
    }
    /// pulses per equalizing / broad group (half-line spacing)
    pub fn eq(self) -> u32 {
        match self {
            VideoStd::Pal => 5,
            VideoStd::Ntsc => 6,
        }
    }
    pub fn hsync(self) -> f64 {
        match self {
            VideoStd::Pal => 4.7e-6,
            VideoStd::Ntsc => 4.7e-6,
        }
    }
}

/// Composite video, negative sync: sync tip 0 V, blanking 0.3 V, picture 0.3..1.0 V (a horizontal ramp per line).
/// Each field starts with the vertical interval on a half-line grid: `eq` equalizing pulses, `eq` broad pulses, `eq`
/// equalizing pulses (1.5 * eq lines). Field 1's first normal line starts right after it, field 2's half a line later,
/// so the fields are distinguishable. Line n of a field = the n-th normal line.
pub fn video(std: VideoStd, t: f64) -> f64 {
    let l = std.line();
    let frame = std.lines() as f64 * l;
    let field_len = frame / 2.0;
    let tf = t.rem_euclid(frame);
    let (field, u) = if tf < field_len { (1, tf) } else { (2, tf - field_len) };
    let eq = std.eq();
    let vi = 1.5 * eq as f64 * l; // 3 groups of eq half lines
    let blank = 0.3;
    if u < vi {
        let slot = (u / (l / 2.0)).floor() as u32;
        let w = u - slot as f64 * l / 2.0;
        let group = slot / eq;
        let low = match group {
            1 => w < l / 2.0 - std.hsync(),  // broad pulse: low except the last 4.7 us
            _ => w < std.hsync() / 2.0,      // equalizing pulse: half an H sync
        };
        return if low { 0.0 } else { blank };
    }
    let off = if field == 2 { l / 2.0 } else { 0.0 };
    let w0 = u - vi - off;
    if w0 < 0.0 {
        return blank;
    }
    let w = w0.rem_euclid(l);
    if w < std.hsync() {
        0.0
    } else if w < 10e-6 {
        blank
    } else {
        blank + 0.7 * ((w - 10e-6) / (l - 12e-6)).clamp(0.0, 1.0)
    }
}

/// Time of line `n` (1-based, normal lines) of field `field` (1/2) in frame 0: start of its H sync.
pub fn video_line_time(std: VideoStd, field: u32, n: u32) -> f64 {
    let l = std.line();
    let field_len = std.lines() as f64 * l / 2.0;
    let s = if field == 2 { field_len } else { 0.0 };
    let off = if field == 2 { l / 2.0 } else { 0.0 };
    s + 1.5 * std.eq() as f64 * l + off + (n - 1) as f64 * l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_train_with_runt() {
        let p = PulseTrain { every: 4, at: 2, special: Pulse { width: 0.5e-3, height: 0.4 }, ..PulseTrain::square(1e3, 0.0, 2.0, 1e-6) };
        assert!((p.v(0.25e-3) - 2.0).abs() < 1e-9);
        assert!((p.v(0.75e-3) - 0.0).abs() < 1e-9);
        assert!((p.v(2.25e-3) - 0.8).abs() < 1e-9); // runt: 40 %
    }

    #[test]
    fn video_levels() {
        let s = VideoStd::Pal;
        let t = video_line_time(s, 1, 10);
        assert_eq!(video(s, t + 1e-6), 0.0);
        assert_eq!(video(s, t + 6e-6), 0.3);
        assert!(video(s, t + 40e-6) > 0.5);
    }
}

/// Piecewise-constant logic waveform: (start time, level) segments, sorted; with `tr` linear transitions.
#[derive(Clone, Debug, Default)]
pub struct Logic {
    pub seg: Vec<(f64, bool)>,
    pub lo: f64,
    pub hi: f64,
    pub tr: f64,
}

impl Logic {
    pub fn new(lo: f64, hi: f64, tr: f64) -> Logic {
        Logic { seg: Vec::new(), lo, hi, tr }
    }
    pub fn push(&mut self, t: f64, level: bool) {
        if self.seg.last().map_or(true, |&(_, l)| l != level) {
            self.seg.push((t, level));
        }
    }
    fn level_at(&self, t: f64) -> (bool, f64, bool) {
        // (level, time of the last change, previous level)
        let i = self.seg.partition_point(|&(s, _)| s <= t);
        if i == 0 {
            let l = self.seg.first().map_or(false, |&(_, l)| l);
            return (l, f64::NEG_INFINITY, l);
        }
        let (s, l) = self.seg[i - 1];
        let p = if i >= 2 { self.seg[i - 2].1 } else { l };
        (l, s, p)
    }
    pub fn v(&self, t: f64) -> f64 {
        let (l, s, p) = self.level_at(t);
        let x = |b: bool| if b { self.hi } else { self.lo };
        let f = if self.tr > 0.0 { ((t - s) / self.tr).clamp(0.0, 1.0) } else { 1.0 };
        x(p) + (x(l) - x(p)) * f
    }
}

/// UART frames (idle high, start bit, `bits` data bits LSB first, optional parity, 1 stop bit), `gap` idle bit
/// times between bytes, starting at `t0` after one idle byte time.
pub fn uart_logic(bytes: &[u8], baud: f64, bits: u32, parity: Option<bool>, gap: f64, lo: f64, hi: f64) -> Logic {
    let tb = 1.0 / baud;
    let mut l = Logic::new(lo, hi, tb / 50.0);
    l.push(0.0, true);
    let mut t = 10.0 * tb;
    for &b in bytes {
        l.push(t, false);
        t += tb;
        let mut ones = 0;
        for i in 0..bits {
            let bit = (b >> i) & 1 == 1;
            ones += bit as u32;
            l.push(t, bit);
            t += tb;
        }
        if let Some(even) = parity {
            // even parity: total number of ones (data + parity) even
            l.push(t, if even { ones % 2 == 1 } else { ones % 2 == 0 });
            t += tb;
        }
        l.push(t, true);
        t += tb * (1.0 + gap);
    }
    l
}

/// One I2C transaction: address, read flag, data bytes; `ack` per byte (false = NACK, for the last read byte).
pub struct I2cXfer {
    pub addr: u8,
    pub read: bool,
    pub data: Vec<u8>,
    /// repeated start instead of start (no stop before it)
    pub repeated: bool,
    pub stop: bool,
}

/// I2C waveforms (SDA, SCL) at bit rate `rate`; the slave ACKs the address and written bytes, the master ACKs read
/// bytes except the last.
pub fn i2c_logic(xfers: &[I2cXfer], rate: f64, lo: f64, hi: f64) -> (Logic, Logic) {
    let q = 0.25 / rate;
    let (mut sda, mut scl) = (Logic::new(lo, hi, q / 20.0), Logic::new(lo, hi, q / 20.0));
    sda.push(0.0, true);
    scl.push(0.0, true);
    let mut t = 8.0 * q;
    let bit = |t: &mut f64, v: bool, sda: &mut Logic, scl: &mut Logic| {
        sda.push(*t, v); // data changes while SCL is low
        *t += q;
        scl.push(*t, true);
        *t += 2.0 * q;
        scl.push(*t, false);
        *t += q;
    };
    for x in xfers {
        if x.repeated {
            // SCL low -> SDA high -> SCL high -> SDA low (start while SCL high)
            sda.push(t, true);
            t += q;
            scl.push(t, true);
            t += q;
        }
        sda.push(t, false); // START: SDA falls while SCL high
        t += q;
        scl.push(t, false);
        t += q;
        let a = (x.addr << 1) | x.read as u8;
        for i in (0..8).rev() {
            bit(&mut t, (a >> i) & 1 == 1, &mut sda, &mut scl);
        }
        bit(&mut t, false, &mut sda, &mut scl); // address ACK
        for (k, &d) in x.data.iter().enumerate() {
            for i in (0..8).rev() {
                bit(&mut t, (d >> i) & 1 == 1, &mut sda, &mut scl);
            }
            let nack = x.read && k + 1 == x.data.len();
            bit(&mut t, nack, &mut sda, &mut scl);
        }
        if x.stop {
            sda.push(t, false);
            t += q;
            scl.push(t, true);
            t += q;
            sda.push(t, true); // STOP: SDA rises while SCL high
            t += 4.0 * q;
        }
    }
    (sda, scl)
}

/// SPI waveforms (CS active low, CLK, MOSI, MISO) for `words` (mosi, miso) of `bits` bits, MSB first, in SPI mode
/// `mode` (0..3), one frame per call, clock `rate`.
pub fn spi_logic(words: &[(u32, u32)], bits: u32, mode: u8, rate: f64, lo: f64, hi: f64) -> [Logic; 4] {
    let h = 0.5 / rate;
    let tr = h / 20.0;
    let (cpol, cpha) = (mode & 2 != 0, mode & 1 != 0);
    let mut cs = Logic::new(lo, hi, tr);
    let mut clk = Logic::new(lo, hi, tr);
    let mut mosi = Logic::new(lo, hi, tr);
    let mut miso = Logic::new(lo, hi, tr);
    cs.push(0.0, true);
    clk.push(0.0, cpol);
    mosi.push(0.0, false);
    miso.push(0.0, false);
    let mut t = 4.0 * h;
    cs.push(t, false);
    t += h;
    for &(mo, mi) in words {
        for i in (0..bits).rev() {
            let (a, b) = ((mo >> i) & 1 == 1, (mi >> i) & 1 == 1);
            if !cpha {
                // data valid before the first edge, sampled on it
                mosi.push(t, a);
                miso.push(t, b);
                t += h;
                clk.push(t, !cpol);
                t += h;
                clk.push(t, cpol);
            } else {
                // data changes on the first edge, sampled on the second
                clk.push(t, !cpol);
                mosi.push(t + tr, a);
                miso.push(t + tr, b);
                t += h;
                clk.push(t, cpol);
                t += h;
            }
        }
        t += h;
    }
    t += h;
    cs.push(t, true);
    [cs, clk, mosi, miso]
}

/// One CAN frame for `can_logic`.
#[derive(Clone, Debug)]
pub struct CanFrame {
    pub id: u32,
    pub ext: bool,
    pub rtr: bool,
    pub data: Vec<u8>,
}

/// The bits of a CAN frame on the bus (true = recessive): SOF .. CRC with bit stuffing, CRC delimiter, ACK slot
/// (dominant when `ack`), ACK delimiter, EOF.
pub fn can_bits(f: &CanFrame, ack: bool) -> Vec<bool> {
    let mut b = vec![false]; // SOF
    let push = |b: &mut Vec<bool>, v: u32, n: u32| (0..n).rev().for_each(|i| b.push((v >> i) & 1 == 1));
    if f.ext {
        push(&mut b, f.id >> 18, 11);
        b.push(true); // SRR
        b.push(true); // IDE
        push(&mut b, f.id & 0x3ffff, 18);
        b.push(f.rtr);
        b.push(false); // r1
        b.push(false); // r0
    } else {
        push(&mut b, f.id, 11);
        b.push(f.rtr);
        b.push(false); // IDE
        b.push(false); // r0
    }
    push(&mut b, f.data.len().min(8) as u32, 4);
    if !f.rtr {
        f.data.iter().take(8).for_each(|&d| push(&mut b, d as u32, 8));
    }
    let crc = crate::decode::can_crc15(&b);
    push(&mut b, crc as u32, 15);
    // stuffing over SOF .. CRC
    let mut s = Vec::new();
    let (mut run, mut last) = (0, None);
    for &x in &b {
        s.push(x);
        if Some(x) == last {
            run += 1;
        } else {
            run = 1;
            last = Some(x);
        }
        if run == 5 {
            s.push(!x);
            last = Some(!x);
            run = 1;
        }
    }
    s.push(true); // CRC delimiter
    s.push(!ack); // ACK slot
    s.push(true); // ACK delimiter
    s.extend([true; 7]); // EOF
    s
}

/// A logic waveform of bus bits (true = recessive = `hi` level), `bit` seconds each, after `idle` recessive bits.
pub fn bits_logic(bits: &[bool], bit: f64, idle: usize, lo: f64, hi: f64) -> Logic {
    let mut l = Logic::new(lo, hi, bit / 40.0);
    l.push(0.0, true);
    let mut t = idle as f64 * bit;
    for &b in bits {
        l.push(t, b);
        t += bit;
    }
    l.push(t, true);
    l
}

/// CAN frames as a logic waveform (dominant low, like the RX pin), 11 idle bits between the frames.
pub fn can_logic(frames: &[CanFrame], bitrate: f64, lo: f64, hi: f64) -> Logic {
    let mut all = Vec::new();
    for f in frames {
        all.extend(can_bits(f, true));
        all.extend([true; 11]);
    }
    bits_logic(&all, 1.0 / bitrate, 12, lo, hi)
}

/// LIN frames (id, data, enhanced checksum?) as a logic waveform: break 13 bits + delimiter, sync 0x55, protected ID,
/// data, checksum; UART 8N1 bytes with one idle bit between them, 10 idle bits between frames.
pub fn lin_logic(frames: &[(u8, Vec<u8>, bool)], baud: f64, lo: f64, hi: f64) -> Logic {
    let mut bits = Vec::new();
    let byte = |bits: &mut Vec<bool>, v: u8| {
        bits.push(false);
        (0..8).for_each(|i| bits.push((v >> i) & 1 == 1));
        bits.push(true);
        bits.push(true);
    };
    for (id, data, enhanced) in frames {
        bits.extend([false; 13]);
        bits.push(true);
        byte(&mut bits, 0x55);
        let pid = crate::decode::lin_pid(*id);
        byte(&mut bits, pid);
        data.iter().for_each(|&d| byte(&mut bits, d));
        let mut cs = if *enhanced { vec![pid] } else { Vec::new() };
        cs.extend(data);
        byte(&mut bits, crate::decode::lin_checksum(&cs));
        bits.extend([true; 10]);
    }
    bits_logic(&bits, 1.0 / baud, 12, lo, hi)
}

/// I2S stream (SCK, WS, SD) of stereo samples: `bits` per slot, MSB first, one-clock delay after WS, SD changes on the
/// falling SCK edge. `fs` = sample rate (frames per second).
pub fn i2s_logic(frames: &[(i32, i32)], bits: u32, fs: f64, lo: f64, hi: f64) -> (Logic, Logic, Logic) {
    let tb = 1.0 / (fs * 2.0 * bits as f64);
    let (mut sck, mut ws, mut sd) = (Logic::new(lo, hi, tb / 20.0), Logic::new(lo, hi, tb / 20.0), Logic::new(lo, hi, tb / 20.0));
    // a stream of bits: WS changes one clock before the MSB of each word
    let mut slots: Vec<(bool, bool)> = Vec::new(); // (ws, sd) per clock
    for &(l, r) in frames {
        for (right, v) in [(false, l), (true, r)] {
            for i in (0..bits).rev() {
                slots.push((right, (v >> i) & 1 == 1));
            }
        }
    }
    ws.push(0.0, true);
    sd.push(0.0, false);
    sck.push(0.0, false);
    let t0 = 4.0 * tb;
    for (k, &(_, b)) in slots.iter().enumerate() {
        let t = t0 + k as f64 * tb;
        // WS of the word one clock ahead (the I2S delay), SD of this clock, both on the falling edge
        // wraps around, so the stream can be repeated seamlessly (t0 + k tb, period = slots x tb)
        let ws_next = slots[(k + 1) % slots.len()].0;
        ws.push(t, ws_next);
        sd.push(t, b);
        sck.push(t, false);
        sck.push(t + tb / 2.0, true);
    }
    let t_end = t0 + slots.len() as f64 * tb;
    sck.push(t_end, false);
    (sck, ws, sd)
}

/// A signal of several levels (bipolar buses): value changes at the given times with a linear transition `tr`.
#[derive(Clone, Debug, Default)]
pub struct Levels {
    pub seg: Vec<(f64, f64)>,
    pub tr: f64,
}

impl Levels {
    pub fn v(&self, t: f64) -> f64 {
        let i = self.seg.partition_point(|&(s, _)| s <= t);
        if i == 0 {
            return self.seg.first().map_or(0.0, |s| s.1);
        }
        let (s, l) = self.seg[i - 1];
        let p = if i >= 2 { self.seg[i - 2].1 } else { l };
        let f = if self.tr > 0.0 { ((t - s) / self.tr).clamp(0.0, 1.0) } else { 1.0 };
        p + (l - p) * f
    }
}

/// MIL-STD-1553 words (cmd sync?, value) as a bipolar bus signal of amplitude `a` (idle 0 V), `gap` us of idle
/// between the words.
pub fn mil1553_wave(words: &[(bool, u16)], a: f64, gap: f64) -> Levels {
    let us = 1e-6;
    let mut l = Levels { seg: vec![(0.0, 0.0)], tr: 0.05 * us };
    let mut t = 4.0 * us;
    for &(cmd, w) in words {
        let s = if cmd { a } else { -a };
        l.seg.push((t, s));
        l.seg.push((t + 1.5 * us, -s));
        t += 3.0 * us;
        let ones = w.count_ones();
        let parity = ones % 2 == 0; // odd parity: set the bit when the data has an even number of ones
        for i in 0..17 {
            let bit = if i < 16 { (w >> (15 - i)) & 1 == 1 } else { parity };
            let first = if bit { a } else { -a };
            l.seg.push((t, first));
            l.seg.push((t + 0.5 * us, -first));
            t += us;
        }
        l.seg.push((t, 0.0));
        t += gap * us;
    }
    l
}
