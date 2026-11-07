// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Built-in arbitrary waveform generator of the DHO900 "S" models (docs/protocols/afg.md): a DDS in the FPGA (BAR1
//! 0x3000..0x3ffc, banks selected by 0x3ff0 / 0x3ff4 / 0x3ff8, 156.25 MHz clock, 8192-sample s16 table), relays for
//! the attenuators over GPIO (afg_in1..5) and the generator supply (FG_M2C). Pure functions return the operations;
//! amplitude/offset are corrected with the factory table `cal_afg.hex` (read only). Units like the stock driver:
//! frequency uHz, amplitude (peak-to-peak) and offset nV.

/// One step of a generator sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Bar1 { off: u32, val: u32 },
    /// afg_in1..5 (bit n = line n + 1)
    Gpio(u8),
    /// FG_M2C: generator supply
    Power(bool),
    SleepMs(u32),
}

const B: u32 = 0x3ff0;
const M: u32 = 0x3ff4;
const A: u32 = 0x3ff8;
/// DDS clock at the nominal 1.25 GHz ADC clock.
pub const CLOCK_HZ: f64 = 156.25e6;

/// DDS clock in effect: ADC clock / 8 (`clock`; 156.25 MHz unless an experimental ADC clock is set).
pub fn clock_hz() -> f64 {
    crate::clock::fpga_hz()
}

fn bank(o: &mut Vec<Op>, sel: u32, w: &[(u32, u32)]) {
    o.push(Op::Bar1 { off: sel, val: 1 });
    o.extend(w.iter().map(|&(off, val)| Op::Bar1 { off, val }));
    o.push(Op::Bar1 { off: sel, val: 0 });
}

/// Phase increment (56 bits) for `f_uhz`: trunc(f / 156.25 MHz * 2^52) (DDS clock `clock_hz`).
pub fn increment(f_uhz: f64) -> u64 {
    ((f_uhz / (clock_hz() * 1e6)) * 4_503_599_627_370_496.0) as u64
}

/// Frequency; noise uses the fixed increment 2^38.
pub fn set_freq(o: &mut Vec<Op>, f_uhz: f64, noise: bool) {
    let inc = if noise { 1u64 << 38 } else { increment(f_uhz) };
    bank(o, B, &[(0x3048, (inc & 0xffff) as u32), (0x304c, (inc >> 16 & 0xffff) as u32), (0x3050, (inc >> 32 & 0xffff) as u32), (0x3054, (inc >> 48 & 0xff) as u32)]);
}

/// Phase (milli-degrees).
pub fn set_phase(o: &mut Vec<Op>, mdeg: f64) {
    bank(o, B, &[(0x3040, ((mdeg * 0.001 * 65536.0 / 360.0) as i64 & 0xffff) as u32)]);
}

/// The carrier table (8192 s16, sign-extended); the output is off afterwards.
pub fn upload(o: &mut Vec<Op>, table: &[i16]) {
    let mut w = vec![(0x3000, 0), (0x3030, 0), (0x3034, 0x1fff)];
    w.extend(table.iter().take(8192).map(|&s| (0x3038, s as i32 as u32)));
    bank(o, B, &w);
}

/// Sine table: trunc(32767 * sin(2 pi i / 8192)), angle in single precision.
pub fn sine_table() -> Vec<i16> {
    (0..8192).map(|i| (32767.0 * ((2.0 * std::f32::consts::PI * i as f32 / 8192.0).sin()) as f64) as i16).collect()
}

/// Output on or off.
pub fn output(o: &mut Vec<Op>, on: bool) {
    bank(o, B, &[(0x3000, on as u32)]);
}

/// The amplitude and offset DAC words (0xffff - code, codes clamped).
pub fn set_codes(o: &mut Vec<Op>, ampl: u32, offset: u32) {
    bank(o, A, &[(0x3070, 0xffff - ampl.min(0xffff))]);
    bank(o, A, &[(0x3078, 0xffff - offset.min(0xffff))]);
}

/// Relay change: bits 0..3 first, 3 ms, then bit 4 (break before make), only if it differs.
pub fn relays(o: &mut Vec<Op>, current: u8, code: u8) {
    if current != code {
        o.push(Op::Gpio(code & 0x0f));
        o.push(Op::SleepMs(3));
        o.push(Op::Gpio(code));
    }
}

/// Gear of an amplitude/offset (nV): (relay code, amplitude gear g1, flatness gear g2, offset gear g3, high-offset
/// flag, path gain) (afg.md 4.2 / 5.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gear {
    pub code: u8,
    pub g1: usize,
    pub g2: usize,
    pub g3: usize,
    pub high: bool,
    pub gain: f64,
}

pub fn gear(a_nv: f64, o_nv: f64, dc: bool) -> Gear {
    let g = |code, g1, g2, g3, high, gain| Gear { code, g1, g2, g3, high, gain };
    let (o_abs, v) = (o_nv.abs(), |x: f64| x * 1e9);
    if dc {
        return if o_abs <= v(2.0) { g(0x1b, 7, 2, 13, false, 1.0) } else { g(0x1f, 7, 2, 14, true, 1.0) };
    }
    if a_nv > v(6.0) {
        return g(0x14, 1, 1, 1, true, 10.0);
    }
    if a_nv > v(2.0) {
        return g(0x15, 0, 0, 0, true, 3.162_277_66);
    }
    if o_abs <= v(2.0) {
        let steps = [(0.005, 0x1b, 2, 0.003_162_277), (0.0165, 0x1a, 3, 0.01), (0.054, 0x19, 4, 0.031_622_776_6), (0.18, 0x18, 5, 0.1), (0.59, 0x11, 6, 0.316_227_766), (2.0, 0x10, 7, 1.0)];
        let &(_, code, gg, gain) = steps.iter().find(|s| a_nv <= v(s.0)).unwrap_or(&steps[5]);
        g(code, gg, 2, gg, false, gain)
    } else {
        let steps = [(0.054, 0x1f, 9, 0.031_622_776_6), (0.18, 0x1e, 10, 0.1), (0.59, 0x1d, 11, 0.316_227_766), (2.0, 0x1c, 12, 1.0)];
        let &(_, code, gg, gain) = steps.iter().find(|s| a_nv <= v(s.0)).unwrap_or(&steps[3]);
        g(code, gg, 0, gg, true, gain)
    }
}

/// `cal_afg.hex` payload (afg.md 6): 40 records of up to 100 linear segments.
pub struct Cal {
    p: Vec<u8>,
}

const REC: usize = 0x968;

impl Cal {
    pub fn from_file_bytes(d: &[u8]) -> Option<Cal> {
        let p = crate::calfile::parse(d).ok()?;
        (p.len() >= 0x18 + 40 * REC).then(|| Cal { p: p.to_vec() })
    }
    pub fn load(dir: &str) -> Option<Cal> {
        let def = format!("{}/default", dir.trim_end_matches('/'));
        let (b, _) = crate::calfile::load_first("cal_afg.hex", &[dir, def.as_str()])?;
        Cal::from_file_bytes(&b)
    }
    fn n(&self, r: usize) -> usize {
        i32::from_le_bytes(self.p[0x18 + r * REC..0x18 + r * REC + 4].try_into().unwrap()).clamp(0, 100) as usize
    }
    fn seg(&self, r: usize, i: usize) -> (i64, i64, f32, f32) {
        let b = 0x18 + r * REC;
        let i64_at = |o: usize| i64::from_le_bytes(self.p[o..o + 8].try_into().unwrap());
        let f32_at = |o: usize| f32::from_le_bytes(self.p[o..o + 4].try_into().unwrap());
        (i64_at(b + 8 + 8 * i), i64_at(b + 0x328 + 8 * i), f32_at(b + 0x648 + 4 * i), f32_at(b + 0x7d8 + 4 * i))
    }
    /// First segment of record `r` that contains `x` (lo <= x <= hi; flatness records below 100 kHz store hi first).
    fn find(&self, r: usize, x: f64, swapped: bool) -> Option<(f32, f32)> {
        (0..self.n(r)).map(|i| self.seg(r, i)).find(|&(lo, hi, _, _)| if swapped { lo as f64 >= x && x >= hi as f64 } else { lo as f64 <= x && x <= hi as f64 }).map(|(_, _, k, b)| (k, b))
    }
}

/// The amplitude and offset DAC codes for amplitude `a_nv`, offset `o_nv` at `f_uhz` (afg.md 5.2,
/// including the stock quirk of searching the offset record with the segment count of record 0).
pub fn codes(cal: Option<&Cal>, a_nv: f64, o_nv: f64, f_uhz: f64, noise: bool, dc: bool) -> (u32, u32, Gear) {
    let g = gear(a_nv, o_nv, dc);
    let (mut a, mut o) = (a_nv, o_nv);
    if let Some(c) = cal {
        if !noise {
            if let Some((k, b)) = c.find(16 + g.g1, a, false) {
                a = a * k as f64 + b as f64;
            }
        }
        let flat = if f_uhz < 1e11 { c.find(30 + g.g2, f_uhz, true) } else { c.find(34 + g.g2, f_uhz, false) };
        if let Some((k, b)) = flat {
            a /= f_uhz * k as f64 + b as f64;
        }
        let n0 = c.n(0);
        let i = (0..n0).find(|&i| {
            let (lo, hi, _, _) = c.seg(g.g3, i);
            lo as f64 <= o && o <= hi as f64
        });
        let i = i.unwrap_or(n0);
        if i < c.n(g.g3) {
            let (_, _, k, b) = c.seg(g.g3, i);
            o = o * k as f64 + b as f64;
        }
    }
    let ampl = (((a * 1e-9 / g.gain) / 1.158 + 2.75) / 5.29 * 65535.0) as i64;
    let off = ((o * 1e-9 * if g.high { 0.11 } else { 1.1 } + 2.75) / 5.29 * 65535.0) as i64;
    (ampl.clamp(0, 0xffff) as u32, off.clamp(0, 0xffff) as u32, g)
}

/// Limits of the stock system: amplitude 2 mV .. 10 V (f <= 10 MHz),
/// 5 V (10..30 MHz), 2.5 V above; offset +-(Vmax - A) / 2 with Vmax 10 V (f <= 10 MHz) else 5 V.
pub fn clamp(a_nv: f64, o_nv: f64, f_uhz: f64) -> (f64, f64) {
    let f = f_uhz * 1e-6;
    let amax = if f <= 10e6 { 10e9 } else if f <= 30e6 { 5e9 } else { 2.5e9 };
    let a = a_nv.clamp(2e6, amax);
    let vmax = if f <= 10e6 { 10e9 } else { 5e9 };
    let omax = ((vmax - a) / 2.0).max(0.0);
    (a, o_nv.clamp(-omax, omax))
}

/// Wave shapes of the basic mode (afg.md 3). The tables are OOS's own: the FPGA path does not depend on the table
/// type except noise (fixed phase increment), so any s16[8192] works [N].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    /// duty cycle 0.01..0.99
    Square { duty: f64 },
    /// symmetry 0..1: fraction of the period that rises
    Ramp { symmetry: f64 },
    /// duty 0.01..0.99, edge times in s (at least one DAC clock)
    Pulse { duty: f64, rise: f64, fall: f64 },
    Noise,
    /// level = offset (the offset DAC), the table is 0
    Dc,
    /// arbitrary wave: the table set by `arb_set`, `id` is what it returned (a new table is a new wave, so a running
    /// output gets it)
    Arb { id: u32 },
}

/// Samples of the waveform RAM.
pub const TABLE_LEN: usize = 8192;

/// The arbitrary table (id, samples): the UI sets it, the acquisition thread uploads it.
static ARB: std::sync::Mutex<(u32, Vec<i16>)> = std::sync::Mutex::new((0, Vec::new()));

/// One period of `v` as the waveform RAM holds it: linear resampling to 8192 points, scaled so that the lowest sample
/// is -32767 and the highest +32767 (the Amplitude setting is then the peak-to-peak output). None without at least two
/// finite samples; a flat wave gives 0.
pub fn arb_table(v: &[f32]) -> Option<Vec<i16>> {
    let v: Vec<f64> = v.iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
    if v.len() < 2 {
        return None;
    }
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let (mid, half) = ((lo + hi) / 2.0, ((hi - lo) / 2.0).max(1e-30));
    Some(
        (0..TABLE_LEN)
            .map(|i| {
                let x = i as f64 * (v.len() - 1) as f64 / (TABLE_LEN - 1) as f64;
                let (k, f) = (x as usize, x.fract());
                let y = v[k] * (1.0 - f) + v[(k + 1).min(v.len() - 1)] * f;
                if hi > lo {
                    ((y - mid) / half * 32767.0).round() as i16
                } else {
                    0
                }
            })
            .collect(),
    )
}

/// Makes `v` the arbitrary wave; returns its id for `Wave::Arb`, None if `v` is too short.
pub fn arb_set(v: &[f32]) -> Option<u32> {
    let t = arb_table(v)?;
    let mut a = ARB.lock().unwrap();
    *a = (a.0.wrapping_add(1).max(1), t);
    Some(a.0)
}

/// The arbitrary wave at phase 0..1, -1..1 (demo source).
pub fn arb_at(ph: f64) -> f64 {
    let a = ARB.lock().unwrap();
    a.1.get((ph * a.1.len() as f64) as usize).map_or(0.0, |&v| v as f64 / 32767.0)
}

/// Samples of a text wave (the stock arbitrary file: lines split at `,` and `:`, the last number of each line counts, so
/// plain lists and OOS's own CSV both load; lines without a number are skipped).
pub fn arb_parse(text: &str) -> Vec<f32> {
    text.lines().filter_map(|l| l.split([',', ':']).next_back()?.trim().parse::<f32>().ok()).collect()
}

impl Wave {
    /// Frequency limit in Hz of the platform (afg.md 3, unless overridden in
    /// limits.conf, see `model`).
    pub fn max_hz(&self) -> f64 {
        self.max_hz_in(&crate::model::platform().limits)
    }

    /// Frequency limit in Hz under the limits `l`.
    pub fn max_hz_in(&self, l: &crate::model::Limits) -> f64 {
        let a = &l.afg_max_hz;
        match self {
            Wave::Sine => a.sine,
            Wave::Noise => a.noise,
            Wave::Dc => a.dc,
            Wave::Square { .. } => a.square,
            Wave::Ramp { .. } => a.ramp,
            Wave::Pulse { .. } => a.pulse,
            Wave::Arb { .. } => a.arb,
        }
    }

    /// The 8192-sample table for frequency `f_hz`. Square and pulse edges are linear over at least one DAC clock
    /// (8192 * f / 156.25 MHz table samples), so the edge does not jump by a whole clock from period to period at
    /// high frequencies (the stock system also rebuilds its square table on every frequency change).
    pub fn table(&self, f_hz: f64) -> Vec<i16> {
        const N: usize = 8192;
        let clock = (N as f64 * f_hz / clock_hz()).max(1.0);
        // a periodic trapezoid: rises from `r0` over `rw` samples, falls from `f0` over `fw` samples
        let trap = |r0: f64, rw: f64, f0: f64, fw: f64| -> Vec<i16> {
            (0..N)
                .map(|i| {
                    let x = i as f64;
                    let rel = |s: f64| (x - s).rem_euclid(N as f64);
                    let up = (rel(r0) / rw).min(1.0);
                    let down = (rel(f0) / fw).min(1.0);
                    // the most recent edge decides: after the rise up to high, after the fall down to low
                    let v = if rel(r0) <= rel(f0) { up } else { 1.0 - down };
                    (32767.0 * (2.0 * v - 1.0)).round() as i16
                })
                .collect()
        };
        match *self {
            Wave::Sine => sine_table(),
            Wave::Square { duty } => {
                let d = duty.clamp(0.01, 0.99) * N as f64;
                // the edges never longer than the high or the low part (5 % at 15 MHz: 410 samples high, 786 per clock)
                trap(0.0, clock.min(d), d, clock.min(N as f64 - d))
            }
            Wave::Pulse { duty, rise, fall } => {
                let per = N as f64 * f_hz;
                let (rw, fw) = ((rise * per).max(clock), (fall * per).max(clock));
                let d = duty.clamp(0.01, 0.99) * N as f64;
                trap(0.0, rw.min(d), d, fw.min(N as f64 - d))
            }
            Wave::Ramp { symmetry } => {
                let s = symmetry.clamp(0.0, 1.0) * N as f64;
                (0..N)
                    .map(|i| {
                        let x = i as f64;
                        let v = if x < s { x / s } else if s >= N as f64 { 1.0 } else { 1.0 - (x - s) / (N as f64 - s) };
                        (32767.0 * (2.0 * v - 1.0)).round() as i16
                    })
                    .collect()
            }
            Wave::Noise => {
                // uniform in -32767..32767, from a fixed-seed xorshift (same table every time)
                let mut x: u32 = 0x9e37_79b9;
                (0..N)
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 17;
                        x ^= x << 5;
                        ((x % 65535) as i32 - 32767) as i16
                    })
                    .collect()
            }
            Wave::Dc => vec![0; N],
            Wave::Arb { .. } => {
                let a = ARB.lock().unwrap();
                if a.1.len() == N {
                    a.1.clone()
                } else {
                    vec![0; N]
                }
            }
        }
    }
}

/// The whole start of a sine (afg.md 9): relays, DDS, table, codes, supply,
/// output on. `current` = the relay levels read from the lines.
pub fn start_sine(cal: Option<&Cal>, f_uhz: f64, a_nv: f64, o_nv: f64, current: u8) -> Vec<Op> {
    start(cal, Wave::Sine, f_uhz, a_nv, o_nv, current)
}

/// `start_sine` for any wave shape: noise with the fixed increment and without the amplitude correction, DC on the
/// offset-only gears, the frequency capped per shape.
pub fn start(cal: Option<&Cal>, wave: Wave, f_uhz: f64, a_nv: f64, o_nv: f64, current: u8) -> Vec<Op> {
    start_mod(cal, wave, f_uhz, a_nv, o_nv, current, Mod::None)
}

/// Shape of the modulating signal (`ModShape` 0..4; 5 = noise is not built).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModShape {
    Sine,
    Square,
    Triangle,
    UpRamp,
    DownRamp,
    /// noise (`ModShape` 5): a random table, and fixed rate words instead of the Rate setting (`modulate`)
    Noise,
}

impl ModShape {
    /// The shape at phase `i` / `n`, -1..1.
    fn at(self, i: usize, n: usize) -> f64 {
        let p = i as f64 / n as f64;
        match self {
            ModShape::Sine => (std::f64::consts::TAU * p).sin(),
            ModShape::Square => if p < 0.5 { 1.0 } else { -1.0 },
            ModShape::Triangle => if p < 0.25 { 4.0 * p } else if p < 0.75 { 2.0 - 4.0 * p } else { 4.0 * p - 4.0 },
            ModShape::UpRamp => 2.0 * p - 1.0,
            ModShape::DownRamp => 1.0 - 2.0 * p,
            ModShape::Noise => {
                // the same pseudo-random table every time (integer hash of the index), uniform in -1..1
                let mut x = (i as u32).wrapping_mul(0x9e37_79b9);
                x = (x ^ x >> 16).wrapping_mul(0x85eb_ca6b);
                x = (x ^ x >> 13).wrapping_mul(0xc2b2_ae35);
                x ^= x >> 16;
                x as f64 / u32::MAX as f64 * 2.0 - 1.0
            }
        }
    }
}

/// Modulation of the carrier (afg.md 2.2-2.4, 3): rate in Hz, AM depth as a fraction (1 = 100 %), FM deviation in
/// Hz, PM deviation in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mod {
    None,
    Am { rate: f64, depth: f64, shape: ModShape },
    Fm { rate: f64, dev: f64, shape: ModShape },
    Pm { rate: f64, dev: f64, shape: ModShape },
}

/// Modulation rate word: trunc(f / 78.125 MHz * 2^48) (afg.md 2.3).
pub fn mod_rate(f_hz: f64) -> u64 {
    (f_hz * 1e6 / (clock_hz() / 2.0 * 1e6) * 281_474_976_710_656.0) as u64
}

/// FM deviation words 0x3004 / 0x3008 / 0x300c for `dev_hz`.
pub fn fm_dev_words(dev_hz: f64) -> [u32; 3] {
    let d = (dev_hz * 1e6 / (clock_hz() * 1e6) * 281_474_976_710_656.0) as u64;
    [((d & 0x7fff) << 1) as u32, (d >> 15 & 0xffff) as u32, (d >> 31 & 0xffff) as u32]
}

/// PM deviation words for `dev_deg`: v = trunc(deg * 2^64 / 360), word = v / 65535 (or 1 below 0xffff), 16/16/16.
pub fn pm_dev_words(dev_deg: f64) -> [u32; 3] {
    let v = (dev_deg / 360.0 * 18_446_744_073_709_551_616.0) as u128;
    let w = if v >= 0xffff { (v / 65535) as u64 } else { 1 };
    [(w & 0xffff) as u32, (w >> 16 & 0xffff) as u32, (w >> 32 & 0xffff) as u32]
}

/// The 4096-entry modulation table. AM (u16): `(1 + m * s) * (m > 1 ? 1 / (1 + m) : 0.5) * 32767`
/// for the sine [N], the same formula with the other shapes [A]; FM / PM (s16): the shape at
/// full scale [A].
pub fn mod_table(md: &Mod) -> Vec<u32> {
    const N: usize = 4096;
    match *md {
        Mod::None => Vec::new(),
        Mod::Am { depth, shape, .. } => {
            let k = if depth > 1.0 { 1.0 / (1.0 + depth) } else { 0.5 };
            (0..N).map(|i| (((1.0 + depth * shape.at(i, N)) * k * 32767.0).max(0.0)) as u32).collect()
        }
        // FM noise: the stock system uploads a fixed table, offset binary around 32768, bell-shaped, sigma about 9900;
        // OOS makes its own with these statistics: the sum of 12 uniforms is
        // nearly normal with sigma 2 for values in -1..1
        Mod::Fm { shape: ModShape::Noise, .. } => (0..N)
            .map(|i| {
                let g: f64 = (0..12).map(|k| ModShape::Noise.at(i * 12 + k, N)).sum();
                (32768.0 + g * 9900.0 / 2.0).clamp(0.0, 65535.0) as u32
            })
            .collect(),
        // PM noise: unipolar, s16 upload
        Mod::Pm { shape: ModShape::Noise, .. } => (0..N).map(|i| (16383.5 * (1.0 + ModShape::Noise.at(i, N))) as u32).collect(),
        Mod::Fm { shape, .. } | Mod::Pm { shape, .. } => (0..N).map(|i| (32767.0 * shape.at(i, N)).round() as i16 as i32 as u32).collect(),
    }
}

/// AM, FM or PM set-up with the table upload and the rate written again (afg.md 2.4, 8); the output stays off until the caller switches it on.
pub fn modulate(o: &mut Vec<Op>, md: &Mod) {
    let (rate, ctl, dev) = match *md {
        Mod::None => return,
        Mod::Am { rate, .. } => (rate, 0x82, None),
        Mod::Fm { rate, dev, .. } => (rate, 0x84, Some(fm_dev_words(dev))),
        Mod::Pm { rate, dev, .. } => (rate, 0x88, Some(pm_dev_words(dev))),
    };
    // noise shape: the stock system writes constants instead of the rate (afg.md 2.3)
    let r = match *md {
        Mod::Am { shape: ModShape::Noise, .. } => 0x16_cdf9_3e9e,
        Mod::Fm { shape: ModShape::Noise, .. } => 0,
        Mod::Pm { shape: ModShape::Noise, .. } => 0x346_dc5d_6388,
        _ => mod_rate(rate),
    };
    let rw = [(0x302c, (r & 0xffff) as u32), (0x3030, (r >> 16 & 0xffff) as u32), (0x3034, (r >> 32 & 0xffff) as u32)];
    let w = |o: &mut Vec<Op>, off: u32, val: u32| o.push(Op::Bar1 { off, val });
    bank(o, B, &[(0x3000, 0)]);
    if let Some(d) = dev {
        // FM / PM: M=1; M=1 rate M=0; M=1, 0x3000; M=1 deviation M=0; M=1, M=1, 0x3010 <- 0, M=0; M=1, M=1, 0x3014 <- 0xffff, M=0, M=0
        w(o, M, 1);
        bank(o, M, &rw);
        w(o, M, 1);
        w(o, 0x3000, ctl);
        bank(o, M, &[(0x3004, d[0]), (0x3008, d[1]), (0x300c, d[2])]);
        w(o, M, 1);
        bank(o, M, &[(0x3010, 0)]);
        w(o, M, 1);
        bank(o, M, &[(0x3014, 0xffff)]);
        w(o, M, 0);
    } else {
        // AM: M=1 rate M=0; M=1, 0x3000 <- 0x82; M=1, M=1, 0x3010 <- 0xffff, M=0; M=1, M=1, 0x3014 <- 0, M=0, M=0
        bank(o, M, &rw);
        w(o, M, 1);
        w(o, 0x3000, ctl);
        w(o, M, 1);
        bank(o, M, &[(0x3010, 0xffff)]);
        w(o, M, 1);
        bank(o, M, &[(0x3014, 0)]);
        w(o, M, 0);
    }
    // modulation RAM: 4096 entries (FM: 4095)
    let t = mod_table(md);
    let n = if matches!(md, Mod::Fm { .. }) { t.len() - 1 } else { t.len() };
    let mut up = vec![(0x3000, 0), (0x3030, 1), (0x3034, 0x1fff)];
    up.extend(t[..n].iter().map(|&v| (0x3038, v)));
    bank(o, B, &up);
    bank(o, M, &rw);
}

/// `start` with a modulation (`Mod::None` = basic mode).
pub fn start_mod(cal: Option<&Cal>, wave: Wave, f_uhz: f64, a_nv: f64, o_nv: f64, current: u8, md: Mod) -> Vec<Op> {
    let f_uhz = f_uhz.clamp(2000.0, wave.max_hz() * 1e6);
    let (noise, dc) = (wave == Wave::Noise, wave == Wave::Dc);
    // noise runs at a fixed increment: its limits do not depend on the set frequency (those of 1 MHz)
    let (a_nv, o_nv) = if dc { (a_nv, o_nv.clamp(-5e9, 5e9)) } else { clamp(a_nv, o_nv, if noise { 1e12 } else { f_uhz }) };
    let (ac, oc, g) = codes(cal, a_nv, o_nv, f_uhz, noise, dc);
    let mut o = Vec::new();
    relays(&mut o, current, g.code);
    bank(&mut o, M, &[(0x3000, 0), (0x3014, 0x7fde), (0x3010, 0xffff)]);
    set_freq(&mut o, f_uhz, noise);
    set_phase(&mut o, 0.0);
    upload(&mut o, &wave.table(f_uhz * 1e-6));
    bank(&mut o, A, &[(0x306c, 0xffff), (0x3074, 0xffff)]);
    set_codes(&mut o, ac, oc);
    modulate(&mut o, &md);
    o.push(Op::Power(true));
    // a little supply settle time (the stock system has none)
    o.push(Op::SleepMs(5));
    if matches!(md, Mod::Fm { shape: ModShape::Noise, .. }) {
        // bit 7 with the enable
        bank(&mut o, B, &[(0x3000, 0x80), (0x3000, 0x81)]);
    } else {
        output(&mut o, true);
    }
    o
}

/// A running output without modulation to new frequency, amplitude or offset: only their registers, about a thousand
/// times faster than `start_mod` and with no gap in the signal. `None` = it takes `start_mod`: another gear (the relays
/// switch), another wave table, or noise.
pub fn retune(cal: Option<&Cal>, wave: Wave, f_old_uhz: f64, f_uhz: f64, a_nv: f64, o_nv: f64, current: u8) -> Option<Vec<Op>> {
    if wave == Wave::Noise {
        return None;
    }
    let dc = wave == Wave::Dc;
    let f_uhz = f_uhz.clamp(2000.0, wave.max_hz() * 1e6);
    let f_old_uhz = f_old_uhz.clamp(2000.0, wave.max_hz() * 1e6);
    let (a_nv, o_nv) = if dc { (a_nv, o_nv.clamp(-5e9, 5e9)) } else { clamp(a_nv, o_nv, f_uhz) };
    let (ac, oc, g) = codes(cal, a_nv, o_nv, f_uhz, false, dc);
    if g.code != current || wave.table(f_old_uhz * 1e-6) != wave.table(f_uhz * 1e-6) {
        return None;
    }
    let mut o = Vec::new();
    if f_uhz != f_old_uhz {
        set_freq(&mut o, f_uhz, false);
    }
    set_codes(&mut o, ac, oc);
    Some(o)
}

/// Output off and supply off.
pub fn stop() -> Vec<Op> {
    let mut o = vec![Op::Power(false)];
    output(&mut o, false);
    o
}

/// The stock off state for a set amplitude/offset: output and supply off, the relays on the gear of the levels
/// (the unit at power-on shows 0x15 for the default 6 V, afg.md 4.2), not all low as after opening the lines.
pub fn idle(a_nv: f64, o_nv: f64, f_uhz: f64, current: u8) -> Vec<Op> {
    let (a_nv, o_nv) = clamp(a_nv, o_nv, f_uhz);
    let mut o = stop();
    relays(&mut o, current, gear(a_nv, o_nv, false).code);
    o
}

/// The generator's GPIO lines, held while OOS drives the generator, and the executor of the sequences.
#[cfg(target_os = "linux")]
pub struct Io {
    relays: [crate::gpio::Line; 5],
    power: crate::gpio::Line,
}

#[cfg(target_os = "linux")]
impl Io {
    /// Opens afg_in1..5 and FG_M2C_GPIO as outputs at the module-init state (all low).
    pub fn open() -> std::io::Result<Io> {
        let l = |n: &str| crate::gpio::Line::output(n, false);
        Ok(Io { relays: [l("afg_in1")?, l("afg_in2")?, l("afg_in3")?, l("afg_in4")?, l("afg_in5")?], power: l("FG_M2C_GPIO")? })
    }
    /// The current relay code (bit n = afg_in(n+1)).
    pub fn relays(&self) -> u8 {
        self.relays.iter().enumerate().fold(0, |c, (n, l)| c | (l.get() as u8) << n)
    }
    pub fn apply(&mut self, bar: &mut dyn crate::bus::Bus, ops: &[Op]) {
        for op in ops {
            match *op {
                Op::Bar1 { off, val } => bar.wr(off, val),
                Op::Gpio(code) => {
                    for (n, l) in self.relays.iter().enumerate() {
                        l.set(code >> n & 1 == 1);
                    }
                }
                Op::Power(on) => self.power.set(on),
                Op::SleepMs(ms) => std::thread::sleep(std::time::Duration::from_millis(ms as u64)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_cal() -> Option<Cal> {
        Cal::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../local/cal-full"))
    }

    #[test]
    fn retune_only_within_a_gear_and_a_table() {
        let cal = unit_cal();
        let c = cal.as_ref();
        let sine = Wave::Sine;
        let gear_of = |a, o| gear(a, o, false).code;
        let g = gear_of(1e9, 0.0);
        let ops = retune(c, sine, 1e9, 2e9, 1e9, 0.0, g).unwrap();
        assert!(ops.len() < 40 && !ops.iter().any(|o| matches!(o, Op::Gpio(_) | Op::Power(_))), "{}", ops.len());
        // the same frequency: only the level words
        assert!(retune(c, sine, 1e9, 1e9, 1.05e9, 0.0, g).unwrap().len() < ops.len());
        // relays on another gear, and noise, go the long way
        assert!(retune(c, sine, 1e9, 1e9, 1e9, 0.0, g ^ 1).is_none());
        assert!(retune(c, Wave::Noise, 1e9, 1e9, 1e9, 0.0, g).is_none());
        // a square has its edge in the table: 1 kHz and 10 MHz are different tables
        assert!(retune(c, Wave::Square { duty: 0.5 }, 1e9, 1e13, 1e9, 0.0, g).is_none());
    }

    #[test]
    fn wave_tables() {
        let high = |t: &[i16]| t.iter().filter(|&&v| v > 0).count() as f64 / t.len() as f64;
        // square: the duty cycle, full scale, and one DAC clock of edge at 10 MHz (524 samples) vs 1 at 1 kHz
        let t = Wave::Square { duty: 0.25 }.table(1e3);
        assert!((high(&t) - 0.25).abs() < 0.001 && t[100] == 32767 && t[4000] == -32767);
        let t = Wave::Square { duty: 0.5 }.table(10e6);
        let ramp = t.iter().filter(|&&v| v > -32767 && v < 32767).count();
        assert!((1000..1100).contains(&ramp), "{}", ramp);
        // 5 % at 15 MHz: the edges shrink to the high part, no jump to full scale at the fall
        let t = Wave::Square { duty: 0.05 }.table(15e6);
        assert!(t.windows(2).all(|w| (w[1] as i32 - w[0] as i32).abs() < 1000), "jump in the table");
        // ramp: symmetric = triangle with the top in the middle; symmetry 1 = rising saw
        let t = Wave::Ramp { symmetry: 0.5 }.table(1e3);
        assert_eq!((t[0], t[4096], t[8191] < -32000), (-32767, 32767, true));
        let t = Wave::Ramp { symmetry: 1.0 }.table(1e3);
        assert!(t.windows(2).all(|w| w[1] >= w[0]));
        // pulse: 10 % duty (rise start to fall start), 50 ns edges at 1 MHz = 410 samples each
        let t = Wave::Pulse { duty: 0.1, rise: 50e-9, fall: 50e-9 }.table(1e6);
        assert!(t[0] == -32767 && t[205].abs() < 200 && t[600] == 32767 && t[1500] == -32767);
        // noise: same table every time, mean near 0, spans the range
        let n = Wave::Noise.table(1e3);
        assert_eq!(n, Wave::Noise.table(5e3));
        let mean = n.iter().map(|&v| v as f64).sum::<f64>() / 8192.0;
        assert!(mean.abs() < 1000.0 && n.iter().any(|&v| v > 32000) && n.iter().any(|&v| v < -32000));
        assert!(Wave::Dc.table(1e3).iter().all(|&v| v == 0));
        assert_eq!(Wave::Ramp { symmetry: 0.5 }.max_hz(), 5e6);
    }

    #[test]
    fn start_per_wave() {
        let freq = |o: &[Op]| o.iter().filter_map(|x| if let Op::Bar1 { off: 0x3048, val } = x { Some(*val) } else { None }).next();
        // noise: fixed increment 2^38 (low word 0)
        let o = start(None, Wave::Noise, 1e9, 1e9, 0.0, 0x10);
        assert_eq!(freq(&o), Some(0));
        assert_eq!(start_sine(None, 1e9, 1e9, 0.0, 0x10), start(None, Wave::Sine, 1e9, 1e9, 0.0, 0x10));
        // DC: the offset-only gear 0x1b, a zero table
        let o = start(None, Wave::Dc, 1e9, 1e9, 1e9, 0);
        assert_eq!(o[0], Op::Gpio(0x0b));
        assert_eq!(o[2], Op::Gpio(0x1b));
        // ramp above 5 MHz is capped: increment of 5 MHz
        let o = start(None, Wave::Ramp { symmetry: 0.5 }, 1e13, 1e9, 0.0, 0x10);
        assert_eq!(freq(&o), Some((increment(5e12) & 0xffff) as u32));
    }

    #[test]
    fn modulation_words_and_sequences() {
        // rate word: 1 kHz -> 1e9 uHz / 78.125e12 * 2^48
        assert_eq!(mod_rate(1e3), (1e9 / 78.125e12 * 2f64.powi(48)) as u64);
        // FM deviation 1 MHz: d = 1e12 / 156.25e12 * 2^48 = 2^48 * 0.0064
        let d = (1e12 / 156.25e12 * 2f64.powi(48)) as u64;
        assert_eq!(fm_dev_words(1e6), [((d & 0x7fff) << 1) as u32, (d >> 15 & 0xffff) as u32, (d >> 31 & 0xffff) as u32]);
        // PM 180 degrees: v = 2^63, word = 2^63 / 65535
        let w = (1u128 << 63) / 65535;
        assert_eq!(pm_dev_words(180.0), [(w & 0xffff) as u32, (w >> 16 & 0xffff) as u32, (w >> 32 & 0xffff) as u32]);
        assert_eq!(pm_dev_words(0.0), [1, 0, 0]);
        // AM sine table at 100 %: 0.5 * 32767 at the start, full scale at the top, 0 at the bottom
        let t = mod_table(&Mod::Am { rate: 1e3, depth: 1.0, shape: ModShape::Sine });
        assert_eq!((t.len(), t[0], t[1024], t[3072]), (4096, 16383, 32767, 0));
        // FM table: s16 sign-extended
        let t = mod_table(&Mod::Fm { rate: 1e3, dev: 1e3, shape: ModShape::Square });
        assert_eq!((t[0], t[3000]), (32767, (-32767i32) as u32));
        // AM sequence (afg.md 2.4): B 0x3000 <- 0, rate, M 0x3000 <- 0x82, 0x3010 <- 0xffff, 0x3014 <- 0, table, rate
        let mut o = Vec::new();
        modulate(&mut o, &Mod::Am { rate: 1e3, depth: 0.5, shape: ModShape::Sine });
        let wr: Vec<(u32, u32)> = o.iter().filter_map(|x| if let Op::Bar1 { off, val } = x { Some((*off, *val)) } else { None }).collect();
        assert_eq!(&wr[..3], &[(B, 1), (0x3000, 0), (B, 0)]);
        assert!(wr.contains(&(0x3000, 0x82)) && wr.contains(&(0x3010, 0xffff)) && wr.contains(&(0x3014, 0)));
        assert_eq!(wr.iter().filter(|w| w.0 == 0x3038).count(), 4096);
        // FM uploads 4095 entries and sets 0x84 with the deviation words
        let mut o = Vec::new();
        modulate(&mut o, &Mod::Fm { rate: 1e3, dev: 1e6, shape: ModShape::Sine });
        let wr: Vec<(u32, u32)> = o.iter().filter_map(|x| if let Op::Bar1 { off, val } = x { Some((*off, *val)) } else { None }).collect();
        assert_eq!(wr.iter().filter(|w| w.0 == 0x3038).count(), 4095);
        assert!(wr.contains(&(0x3000, 0x84)) && wr.contains(&(0x3004, fm_dev_words(1e6)[0])));
        // the select registers always end at 0
        for sel in [B, M] {
            assert_eq!(wr.iter().rev().find(|w| w.0 == sel).map(|w| w.1), Some(0));
        }
        // start with a modulation: the modulation before the output goes on
        let o = start_mod(None, Wave::Sine, 1e9, 1e9, 0.0, 0x10, Mod::Am { rate: 10.0, depth: 1.0, shape: ModShape::Sine });
        let on = o.iter().rposition(|x| *x == Op::Bar1 { off: 0x3000, val: 1 }).unwrap();
        let am = o.iter().position(|x| *x == Op::Bar1 { off: 0x3000, val: 0x82 }).unwrap();
        assert!(am < on);
    }

    #[test]
    fn idle_keeps_the_gear_relays() {
        // from the lines as opened (all low): supply and output off, then the relays of the gear in two steps
        let o = idle(6e9, 0.0, 1e9, 0);
        let code = gear(6e9, 0.0, false).code;
        assert_eq!(code, 0x15);
        assert_eq!(o[..4], [Op::Power(false), Op::Bar1 { off: B, val: 1 }, Op::Bar1 { off: 0x3000, val: 0 }, Op::Bar1 { off: B, val: 0 }]);
        assert_eq!(o[4..], [Op::Gpio(code & 0x0f), Op::SleepMs(3), Op::Gpio(code)]);
        // already on the gear: no relay switching
        assert_eq!(idle(6e9, 0.0, 1e9, code), stop());
    }

    #[test]
    fn section_9_vectors() {
        assert_eq!(increment(1e9), 0x6_b5fc_a6af);
        let g = gear(1e9, 0.0, false);
        assert_eq!((g.code, g.g1, g.g2, g.g3, g.high), (0x10, 7, 2, 7, false));
        assert_eq!(gear(6e9, 0.0, false).code, 0x15);
        assert_eq!(gear(10e6, 0.0, false).code, 0x1a);
        assert_eq!(gear(1e9, 3e9, false).code, 0x1c);
        // uncalibrated codes
        let (a, o, _) = codes(None, 1e9, 0.0, 1e9, false, false);
        assert_eq!((0xffff - a, 0xffff - o), (0x5121, 0x7aeb));
        let Some(cal) = unit_cal() else { return };
        let (a, o, _) = codes(Some(&cal), 1e9, 0.0, 1e9, false, false);
        assert_eq!((0xffff - a, 0xffff - o), (0x523f, 0x7c31));
    }

    #[test]
    fn start_sequence_shape() {
        let s = start_sine(None, 1e9, 1e9, 0.0, 0x15);
        // relays break before make, the 8192-sample table, the codes, supply before the output
        assert_eq!(&s[..3], &[Op::Gpio(0x00), Op::SleepMs(3), Op::Gpio(0x10)]);
        assert_eq!(s.iter().filter(|x| matches!(x, Op::Bar1 { off: 0x3038, .. })).count(), 8192);
        let p = s.iter().position(|x| *x == Op::Power(true)).unwrap();
        let on = s.iter().rposition(|x| *x == Op::Bar1 { off: 0x3000, val: 1 }).unwrap();
        assert!(p < on);
        // every bank select ends at 0
        for sel in [B, M, A] {
            let last = s.iter().rev().find(|x| matches!(x, Op::Bar1 { off, .. } if *off == sel)).unwrap();
            assert_eq!(*last, Op::Bar1 { off: sel, val: 0 });
        }
        let t = sine_table();
        assert_eq!((t[0], t[2048], t[6144]), (0, 32767, -32767));
        assert_eq!(clamp(20e9, 9e9, 1e9), (10e9, 0.0));
        assert_eq!(clamp(1e9, 9e9, 50e12).0, 1e9);
    }
}

#[cfg(test)]
mod arb_tests {
    use super::*;

    #[test]
    fn table_scaling_and_resampling() {
        // a ramp 2..4 V: full scale, whatever the input level
        let t = arb_table(&[2.0, 3.0, 4.0]).unwrap();
        assert_eq!(t.len(), TABLE_LEN);
        assert_eq!((t[0], t[TABLE_LEN - 1]), (-32767, 32767));
        assert!(t[TABLE_LEN / 2].abs() < 20);
        assert!(t.windows(2).all(|w| w[0] <= w[1]));
        // a flat wave is silence, NaN samples are dropped, one sample is not a wave
        assert!(arb_table(&[1.5, 1.5]).unwrap().iter().all(|&v| v == 0));
        assert!(arb_table(&[f32::NAN, 1.0]).is_none());
        assert!(arb_table(&[]).is_none());
    }

    #[test]
    fn wave_uses_the_table() {
        let a = arb_set(&[0.0, 1.0]).unwrap();
        let b = arb_set(&[1.0, 0.0]).unwrap();
        assert_ne!(a, b, "a new table is a new wave: a running output gets it");
        let w = Wave::Arb { id: b };
        assert_eq!(w.table(1e3)[0], 32767);
        assert_eq!(w.max_hz(), 10e6);
    }

    #[test]
    fn text_formats() {
        assert_eq!(arb_parse("0\n0.5\n1\n"), vec![0.0, 0.5, 1.0]);
        // OOS's own CSV: the header is skipped, the value is the last column; the stock's `:` works too
        assert_eq!(arb_parse("Time (s),CH1 (V)\n1e-6,0.25\n2e-6,0.75\n"), vec![0.25, 0.75]);
        assert_eq!(arb_parse("1:2.5\n"), vec![2.5]);
    }
}

#[cfg(test)]
mod noise_mod_tests {
    use super::*;

    fn rate_words(md: &Mod) -> Vec<u32> {
        let mut o = Vec::new();
        modulate(&mut o, md);
        o.iter().filter_map(|x| if let Op::Bar1 { off: 0x302c..=0x3034, val } = x { Some(*val) } else { None }).collect()
    }

    #[test]
    fn noise_table_is_spread_and_repeatable() {
        let n = 4096;
        let v: Vec<f64> = (0..n).map(|i| ModShape::Noise.at(i, n)).collect();
        let mean = v.iter().sum::<f64>() / n as f64;
        let var = v.iter().map(|x| x * x).sum::<f64>() / n as f64;
        assert!(mean.abs() < 0.05, "{}", mean);
        // uniform -1..1: variance 1/3
        assert!((var - 1.0 / 3.0).abs() < 0.03, "{}", var);
        assert!(v.iter().all(|x| x.abs() <= 1.0));
        assert_eq!(mod_table(&Mod::Am { rate: 1.0, depth: 0.5, shape: ModShape::Noise }), mod_table(&Mod::Am { rate: 9.0, depth: 0.5, shape: ModShape::Noise }));
    }

    #[test]
    fn stock_rate_constants() {
        let w = rate_words(&Mod::Am { rate: 123.0, depth: 0.5, shape: ModShape::Noise });
        let r = 0x16_cdf9_3e9eu64;
        assert_eq!(w[..3], [(r & 0xffff) as u32, (r >> 16 & 0xffff) as u32, (r >> 32 & 0xffff) as u32]);
        assert!(rate_words(&Mod::Fm { rate: 123.0, dev: 1e3, shape: ModShape::Noise })[..3].iter().all(|&x| x == 0));
    }

    #[test]
    fn fm_noise_enables_with_bit_7() {
        let o = start_mod(None, Wave::Sine, 1e9, 1e9, 0.0, 0x10, Mod::Fm { rate: 1.0, dev: 1e3, shape: ModShape::Noise });
        let on = o.iter().rposition(|x| *x == Op::Bar1 { off: 0x3000, val: 0x81 }).unwrap();
        assert_eq!(o[on - 1], Op::Bar1 { off: 0x3000, val: 0x80 });
        // PM: unipolar; FM: offset binary around 32768, sigma about 9900
        let t = mod_table(&Mod::Pm { rate: 1.0, dev: 90.0, shape: ModShape::Noise });
        assert!(t.iter().all(|&v| v <= 0x7fff));
        let t = mod_table(&Mod::Fm { rate: 1.0, dev: 1e3, shape: ModShape::Noise });
        let n = t.len() as f64;
        let mean = t.iter().map(|&v| v as f64).sum::<f64>() / n;
        let sd = (t.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n).sqrt();
        assert!((mean - 32768.0).abs() < 800.0 && (sd - 9900.0).abs() < 900.0, "{} {}", mean, sd);
    }
}
