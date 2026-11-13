// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Nominal high-speed serial records for the SIM menu. A preset is a public rate, a PRBS, rails, a tanh edge,
//! a Butterworth limit, one-tap de-emphasis, optional down-spread, and seeded Gaussian noise and periodic jitter.
//! The bits are the PRBS. There is no line coding.

const N: usize = 65_536;
const SPU: f64 = 8.0;
const FILTER_ORDER: u32 = 3;

/// One menu row. Numbers are SI: bits per second, volts, seconds, hertz.
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub name: &'static str,
    pub bits_per_s: f64,
    pub prbs: u8,
    pub v_high: f64,
    pub v_low: f64,
    /// 20–80 % isolated edge, seconds
    pub rise: f64,
    pub noise_rms: f64,
    /// Butterworth cutoff, hertz
    pub bw: f64,
    /// de-emphasis of a repeated symbol, dB. Zero leaves the rail.
    pub deemph_db: f64,
    pub ssc: bool,
    /// random jitter, seconds RMS
    pub rj_rms: f64,
    /// periodic jitter peak, seconds, and its frequency, hertz. Zero peak skips it.
    pub pj_peak: f64,
    pub pj_hz: f64,
    pub seed: u64,
}

pub const LANES: &[Lane] = &[
    Lane { name: "PCIe 1.0", bits_per_s: 2.5e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 120e-12, noise_rms: 4.15625e-3, bw: 2e9, deemph_db: 0.0, ssc: true, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 1 },
    Lane { name: "PCIe 2.0", bits_per_s: 5.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 60e-12, noise_rms: 4.3125e-3, bw: 4e9, deemph_db: 3.5, ssc: true, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 2 },
    Lane { name: "PCIe 3.0", bits_per_s: 8.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 35e-12, noise_rms: 4.5e-3, bw: 6e9, deemph_db: 3.5, ssc: true, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 3 },
    Lane { name: "PCIe 4.0", bits_per_s: 16.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 20e-12, noise_rms: 5e-3, bw: 12e9, deemph_db: 4.5, ssc: true, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 4 },
    Lane { name: "PCIe 5.0", bits_per_s: 32.0e9, prbs: 31, v_high: 0.5, v_low: -0.5, rise: 12e-12, noise_rms: 6e-3, bw: 22e9, deemph_db: 6.0, ssc: true, rj_rms: 1.2e-12, pj_peak: 4e-12, pj_hz: 8e6, seed: 5 },
    Lane { name: "USB 2.0 HS", bits_per_s: 480e6, prbs: 7, v_high: 0.4, v_low: -0.4, rise: 500e-12, noise_rms: 2e-3, bw: 250e6, deemph_db: 0.0, ssc: false, rj_rms: 8e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 6 },
    Lane { name: "USB 3.2 Gen 1", bits_per_s: 5.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 60e-12, noise_rms: 4e-3, bw: 3.5e9, deemph_db: 0.0, ssc: false, rj_rms: 2e-12, pj_peak: 4e-12, pj_hz: 2.5e6, seed: 7 },
    Lane { name: "USB 3.2 Gen 2", bits_per_s: 10.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 30e-12, noise_rms: 5e-3, bw: 7e9, deemph_db: 0.0, ssc: false, rj_rms: 1.5e-12, pj_peak: 4e-12, pj_hz: 5e6, seed: 8 },
    Lane { name: "SATA I", bits_per_s: 1.5e9, prbs: 7, v_high: 0.4, v_low: -0.4, rise: 150e-12, noise_rms: 3e-3, bw: 1e9, deemph_db: 0.0, ssc: false, rj_rms: 4e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 9 },
    Lane { name: "SATA II", bits_per_s: 3.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 90e-12, noise_rms: 3e-3, bw: 2e9, deemph_db: 0.0, ssc: false, rj_rms: 3e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 10 },
    Lane { name: "SATA III", bits_per_s: 6.0e9, prbs: 15, v_high: 0.4, v_low: -0.4, rise: 45e-12, noise_rms: 4e-3, bw: 4e9, deemph_db: 0.0, ssc: false, rj_rms: 2.5e-12, pj_peak: 6e-12, pj_hz: 3e6, seed: 11 },
    Lane { name: "10GBASE-KR", bits_per_s: 10.3125e9, prbs: 31, v_high: 0.425, v_low: -0.425, rise: 30e-12, noise_rms: 5e-3, bw: 7.5e9, deemph_db: 3.5, ssc: false, rj_rms: 1.2e-12, pj_peak: 0.0, pj_hz: 0.0, seed: 12 },
];

pub fn names() -> Vec<&'static str> {
    LANES.iter().map(|l| l.name).collect()
}

/// Sample interval that keeps eight samples per unit interval.
pub fn dt_of(lane: &Lane) -> f64 {
    1.0 / (SPU * lane.bits_per_s)
}

/// Volts, little-endian sample order, `N` points.
pub fn generate(lane: &Lane) -> Vec<f32> {
    let dt = dt_of(lane);
    let ui = 1.0 / lane.bits_per_s;
    let n_sym = (N as f64 * dt / ui).ceil() as usize + 8;
    let bits = prbs(lane.prbs, n_sym, 0xffff_ffff);
    let mut rng = Rng(lane.seed | 1);
    let mut edges = Vec::with_capacity(n_sym);
    let mut t = 0.0;
    let (lo, hi) = (lane.v_low, lane.v_high);
    let mid = 0.5 * (lo + hi);
    let atten = if lane.deemph_db > 0.0 { 10f64.powf(-lane.deemph_db / 20.0) } else { 1.0 };
    let mut prev = false;
    for &b in &bits {
        let bit = b != 0;
        let rail = if bit { hi } else { lo };
        let v = if bit == prev { mid + (rail - mid) * atten } else { rail };
        edges.push((t, v));
        let mut next = t + ui;
        if lane.ssc {
            // 0.5 % down-spread at 33 kHz, DC removed. One generate is a slice of the triangle.
            let tri = triangle((t * 33e3).fract());
            next = t + ui / (1.0 + 0.005 * (tri - 0.5));
        }
        t = next;
        prev = bit;
    }
    for e in &mut edges {
        e.0 += lane.rj_rms * rng.gauss();
        if lane.pj_peak > 0.0 && lane.pj_hz > 0.0 {
            e.0 += lane.pj_peak * (2.0 * std::f64::consts::PI * lane.pj_hz * e.0).sin();
        }
    }
    let tau = lane.rise / (2.0 * 0.6f64.atanh());
    let win = (6.0 * tau / dt).ceil() as i64;
    let mut v = vec![mid as f32; N];
    let mut k = 0usize;
    for i in 0..N {
        let ti = i as f64 * dt;
        while k + 1 < edges.len() && edges[k + 1].0 <= ti {
            k += 1;
        }
        let (te, level) = edges[k];
        let from = if k == 0 { mid } else { edges[k - 1].1 };
        let u = ((ti - te) / tau).clamp(-8.0, 8.0);
        let s = 0.5 * (1.0 + u.tanh());
        if (ti - te).abs() > win as f64 * dt {
            v[i] = level as f32;
        } else {
            v[i] = (from + (level - from) * s) as f32;
        }
    }
    let nyq = 0.45 / dt;
    let fc = lane.bw.min(nyq);
    v = butterworth(&v, dt, fc, FILTER_ORDER);
    for x in &mut v {
        *x += (lane.noise_rms * rng.gauss()) as f32;
    }
    v
}

/// Fibonacci LFSR, output = LSB, shift right, feedback at the MSB. Tap offsets from the LSB.
fn prbs(order: u8, n: usize, seed: u32) -> Vec<u8> {
    let taps: &[u32] = match order {
        7 => &[0, 1],
        9 => &[0, 4],
        13 => &[0, 1, 11, 12],
        15 => &[0, 1],
        23 => &[0, 5],
        _ => &[0, 3],
    };
    let mask = if order >= 31 { u32::MAX } else { (1u32 << order) - 1 };
    let mut state = (seed & mask).max(1);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push((state & 1) as u8);
        let mut fb = 0u32;
        for &t in taps {
            fb ^= (state >> t) & 1;
        }
        state = ((state >> 1) | (fb << (order as u32 - 1))) & mask;
    }
    out
}

fn triangle(x: f64) -> f64 {
    let x = x.fract().abs();
    if x < 0.5 { 2.0 * x } else { 2.0 - 2.0 * x }
}

struct Rng(u64);
impl Rng {
    fn u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn gauss(&mut self) -> f64 {
        let u = ((self.u64() >> 11) as f64 / (1u64 << 53) as f64).clamp(1e-12, 1.0 - 1e-12);
        let v = (self.u64() >> 11) as f64 / (1u64 << 53) as f64;
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// Cascaded second-order Butterworth, run forward and back (zero phase).
fn butterworth(x: &[f32], dt: f64, fc: f64, order: u32) -> Vec<f32> {
    let nsec = order.div_ceil(2) as usize;
    let mut sos = Vec::with_capacity(nsec);
    let w0 = 2.0 * std::f64::consts::PI * (fc * dt).clamp(1e-6, 0.49);
    for k in 0..nsec {
        let theta = std::f64::consts::PI * (2.0 * k as f64 + 1.0) / (2.0 * order as f64);
        let q = 1.0 / (2.0 * theta.sin());
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        let a0 = 1.0 + alpha;
        let b0 = (1.0 - c) / 2.0 / a0;
        sos.push([b0, 2.0 * b0, b0, -2.0 * c / a0, (1.0 - alpha) / a0]);
    }
    let mut y: Vec<f64> = x.iter().map(|&v| v as f64).collect();
    for k in &sos {
        filtfilt(*k, &mut y);
    }
    y.into_iter().map(|v| v as f32).collect()
}

fn filtfilt(k: [f64; 5], x: &mut [f64]) {
    let run = |x: &mut [f64]| {
        let x0 = x.first().copied().unwrap_or(0.0);
        let g = (k[0] + k[1] + k[2]) / (1.0 + k[3] + k[4]);
        let (mut x1, mut x2, mut y1, mut y2) = (x0, x0, x0 * g, x0 * g);
        for v in x.iter_mut() {
            let y = k[0] * *v + k[1] * x1 + k[2] * x2 - k[3] * y1 - k[4] * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, y);
            *v = y;
        }
    };
    run(x);
    x.reverse();
    run(x);
    x.reverse();
}
