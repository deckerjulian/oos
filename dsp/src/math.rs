// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Math channel: arithmetic on two sources, functions of one source, filters, FFT.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op2 {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op1 {
    /// running integral (trapezoid), V*s
    Intg,
    /// derivative (central difference), V/s
    Diff,
    Abs,
    Sqrt,
    Lg,
    Ln,
    Exp,
    /// first-order low-pass with corner frequency (Hz)
    Lowpass(f64),
    /// first-order high-pass with corner frequency (Hz)
    Highpass(f64),
    /// band-pass around `f0` (Hz) with bandwidth `bw` (Hz): RBJ biquad, run forward and backward (zero phase, 4th
    /// order magnitude)
    Bandpass(f64, f64),
    /// band-stop (notch) around `f0` with bandwidth `bw`, zero phase like `Bandpass`
    Bandstop(f64, f64),
}

/// RBJ audio-EQ-cookbook biquad coefficients (b0, b1, b2, a1, a2), normalised to a0 = 1.
fn biquad(notch: bool, f0: f64, bw: f64, dt: f64) -> [f64; 5] {
    let w0 = 2.0 * std::f64::consts::PI * (f0 * dt).clamp(1e-9, 0.499);
    let q = (f0 / bw.max(1e-9)).max(0.05);
    let alpha = w0.sin() / (2.0 * q);
    let c = w0.cos();
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = if notch { (1.0, -2.0 * c, 1.0) } else { (alpha, 0.0, -alpha) };
    [b0 / a0, b1 / a0, b2 / a0, -2.0 * c / a0, (1.0 - alpha) / a0]
}

/// Biquad forward then backward (zero phase); the start is primed with the first sample to avoid a step.
fn filtfilt(k: [f64; 5], a: &[f32]) -> Vec<f32> {
    let run = |x: &mut Vec<f64>| {
        let x0 = x.first().copied().unwrap_or(0.0);
        // steady state for a constant input x0: y = x0 * dc gain
        let g = (k[0] + k[1] + k[2]) / (1.0 + k[3] + k[4]);
        let (mut x1, mut x2, mut y1, mut y2) = (x0, x0, x0 * g, x0 * g);
        for v in x.iter_mut() {
            let y = k[0] * *v + k[1] * x1 + k[2] * x2 - k[3] * y1 - k[4] * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, y);
            *v = y;
        }
    };
    let mut x: Vec<f64> = a.iter().map(|&v| v as f64).collect();
    run(&mut x);
    x.reverse();
    run(&mut x);
    x.reverse();
    x.into_iter().map(|v| v as f32).collect()
}

pub fn binary(op: Op2, a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| match op {
            Op2::Add => x + y,
            Op2::Sub => x - y,
            Op2::Mul => x * y,
            Op2::Div => {
                if y.abs() < 1e-12 {
                    f32::NAN
                } else {
                    x / y
                }
            }
        })
        .collect()
}

pub fn unary(op: Op1, a: &[f32], dt: f64) -> Vec<f32> {
    let n = a.len();
    match op {
        Op1::Intg => {
            let mut s = 0.0f64;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                if i > 0 {
                    s += 0.5 * (a[i] as f64 + a[i - 1] as f64) * dt;
                }
                out.push(s as f32);
            }
            out
        }
        Op1::Diff => (0..n)
            .map(|i| {
                let (l, r) = (i.saturating_sub(1), (i + 1).min(n - 1));
                if r == l {
                    0.0
                } else {
                    ((a[r] as f64 - a[l] as f64) / ((r - l) as f64 * dt)) as f32
                }
            })
            .collect(),
        Op1::Abs => a.iter().map(|x| x.abs()).collect(),
        Op1::Sqrt => a.iter().map(|&x| if x < 0.0 { f32::NAN } else { x.sqrt() }).collect(),
        Op1::Lg => a.iter().map(|x| x.abs().max(1e-12).log10()).collect(),
        Op1::Ln => a.iter().map(|x| x.abs().max(1e-12).ln()).collect(),
        Op1::Exp => a.iter().map(|x| x.exp()).collect(),
        Op1::Lowpass(fc) => {
            let rc = 1.0 / (2.0 * std::f64::consts::PI * fc);
            let k = dt / (rc + dt);
            let mut y = a.first().copied().unwrap_or(0.0) as f64;
            a.iter()
                .map(|&x| {
                    y += k * (x as f64 - y);
                    y as f32
                })
                .collect()
        }
        Op1::Highpass(fc) => {
            let rc = 1.0 / (2.0 * std::f64::consts::PI * fc);
            let k = rc / (rc + dt);
            let (mut y, mut xp) = (0.0f64, a.first().copied().unwrap_or(0.0) as f64);
            a.iter()
                .map(|&x| {
                    y = k * (y + x as f64 - xp);
                    xp = x as f64;
                    y as f32
                })
                .collect()
        }
        Op1::Bandpass(f0, bw) => filtfilt(biquad(false, f0, bw, dt), a),
        Op1::Bandstop(f0, bw) => filtfilt(biquad(true, f0, bw, dt), a),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    Rect,
    Hann,
    Hamming,
    Blackman,
    Flattop,
}

impl Window {
    pub fn w(self, i: usize, m: usize) -> f64 {
        let x = 2.0 * std::f64::consts::PI * i as f64 / (m - 1).max(1) as f64;
        match self {
            Window::Rect => 1.0,
            Window::Hann => 0.5 - 0.5 * x.cos(),
            Window::Hamming => 0.54 - 0.46 * x.cos(),
            Window::Blackman => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
            Window::Flattop => 0.215_578_95 - 0.416_631_58 * x.cos() + 0.277_263_158 * (2.0 * x).cos() - 0.083_578_947 * (3.0 * x).cos() + 0.006_947_368 * (4.0 * x).cos(),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Window::Rect => "Rect",
            Window::Hann => "Hann",
            Window::Hamming => "Hamming",
            Window::Blackman => "Blackman",
            Window::Flattop => "Flattop",
        }
    }
}

/// In-place radix-2 complex FFT (n = power of two).
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Amplitude spectrum: (bin spacing Hz, RMS volts per bin 0..=n/2). The window is normalised to its coherent gain, so
/// a sine on a bin centre reads its RMS value (with Flattop also between bins). NaN samples are dropped; the record is
/// zero-padded to a power of two.
pub fn spectrum(v: &[f32], dt: f64, win: Window) -> (f64, Vec<f64>) {
    let f: Vec<f64> = v.iter().filter(|x| x.is_finite()).map(|&x| x as f64).collect();
    let m = f.len();
    if m < 16 {
        return (0.0, Vec::new());
    }
    let n = m.next_power_of_two();
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    let mut wsum = 0.0;
    for (i, &x) in f.iter().enumerate() {
        let w = win.w(i, m);
        re[i] = x * w;
        wsum += w;
    }
    fft(&mut re, &mut im);
    let out = (0..=n / 2)
        .map(|k| {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() / wsum;
            if k == 0 {
                mag
            } else {
                2.0 * mag / std::f64::consts::SQRT_2
            }
        })
        .collect();
    (1.0 / (n as f64 * dt), out)
}

/// dBV of an RMS value.
pub fn dbv(vrms: f64) -> f64 {
    20.0 * vrms.max(1e-12).log10()
}

/// The `count` largest local maxima of a spectrum (bin, value), excluding DC, highest first.
pub fn peaks(spec: &[f64], count: usize) -> Vec<(usize, f64)> {
    let mut p: Vec<(usize, f64)> = (2..spec.len().saturating_sub(1)).filter(|&k| spec[k] > spec[k - 1] && spec[k] >= spec[k + 1]).map(|k| (k, spec[k])).collect();
    p.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    p.truncate(count);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_pass_and_stop() {
        // 1 kHz + 10 kHz at 1 MSa/s: the band-pass at 10 kHz keeps 10 kHz and removes 1 kHz, the notch the opposite
        let dt = 1e-6;
        let sine = |f: f64| (0..200_000).map(|i| (2.0 * std::f64::consts::PI * f * i as f64 * dt).sin() as f32).collect::<Vec<f32>>();
        let (a, b) = (sine(1e3), sine(10e3));
        let mix: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let rms = |v: &[f32], w: &[f32]| {
            // rms of the difference in the middle (edges excluded)
            let n = v.len();
            (v[n / 4..3 * n / 4].iter().zip(&w[n / 4..3 * n / 4]).map(|(x, y)| ((x - y) as f64).powi(2)).sum::<f64>() / (n / 2) as f64).sqrt()
        };
        let bp = unary(Op1::Bandpass(10e3, 2e3), &mix, dt);
        assert!(rms(&bp, &b) < 0.03, "band-pass error {}", rms(&bp, &b));
        let bs = unary(Op1::Bandstop(10e3, 2e3), &mix, dt);
        assert!(rms(&bs, &a) < 0.03, "band-stop error {}", rms(&bs, &a));
    }
    use crate::sim::sample;
    use std::f64::consts::PI;

    #[test]
    fn integral_and_derivative() {
        let dt = 1e-6;
        let one = vec![1.0f32; 1001];
        assert!((unary(Op1::Intg, &one, dt)[1000] as f64 - 1e-3).abs() < 1e-9);
        let ramp: Vec<f32> = (0..100).map(|i| i as f32 * 1e-3).collect(); // 1 mV per us = 1000 V/s
        let d = unary(Op1::Diff, &ramp, dt);
        assert!((d[50] - 1000.0).abs() < 0.1 && (d[0] - 1000.0).abs() < 0.1);
    }

    #[test]
    fn arithmetic() {
        let (a, b) = (vec![1.0, 2.0, 3.0], vec![2.0, 0.0, -1.0]);
        assert_eq!(binary(Op2::Add, &a, &b), [3.0, 2.0, 2.0]);
        assert_eq!(binary(Op2::Mul, &a, &b), [2.0, 0.0, -3.0]);
        assert!(binary(Op2::Div, &a, &b)[1].is_nan());
        assert_eq!(unary(Op1::Abs, &b, 1.0), [2.0, 0.0, 1.0]);
    }

    #[test]
    fn spectrum_amplitudes() {
        let (n, dt) = (4096, 1e-6);
        let df = 1.0 / (n as f64 * dt);
        // on a bin: 1 V peak -> 0.7071 V rms with every window
        let v = sample(|t| (2.0 * PI * 100.0 * df * t).sin(), 0.0, dt, n);
        for w in [Window::Rect, Window::Hann, Window::Hamming, Window::Blackman, Window::Flattop] {
            let (_, s) = spectrum(&v, dt, w);
            assert!((s[100] - 0.70711).abs() < 0.002, "{:?} {}", w, s[100]);
        }
        // between two bins: Flattop stays within 0.1 %, Hann reads low (scalloping)
        let v = sample(|t| (2.0 * PI * 100.5 * df * t).sin(), 0.0, dt, n);
        let fl = spectrum(&v, dt, Window::Flattop).1;
        assert!((fl[100].max(fl[101]) - 0.70711).abs() < 0.0015, "{}", fl[100].max(fl[101]));
        let hn = spectrum(&v, dt, Window::Hann).1;
        assert!(hn[100].max(hn[101]) < 0.62);
        assert!((dbv(0.70711) + 3.0103).abs() < 1e-3);
    }

    #[test]
    fn peak_search() {
        let (n, dt) = (4096, 1e-6);
        let df = 1.0 / (n as f64 * dt);
        let v = sample(|t| (2.0 * PI * 200.0 * df * t).sin() + 0.25 * (2.0 * PI * 600.0 * df * t).sin(), 0.0, dt, n);
        let p = peaks(&spectrum(&v, dt, Window::Hann).1, 2);
        assert_eq!((p[0].0, p[1].0), (200, 600));
        assert!((p[1].1 / p[0].1 - 0.25).abs() < 0.01);
    }

    #[test]
    fn filters_at_the_corner() {
        let dt = 1e-7;
        let fc = 10e3;
        let v = sample(|t| (2.0 * PI * fc * t).sin(), 0.0, dt, 200_000);
        let amp = |x: &[f32]| x[100_000..].iter().fold(0.0f32, |m, &y| m.max(y.abs()));
        assert!((amp(&unary(Op1::Lowpass(fc), &v, dt)) - 0.7071).abs() < 0.01);
        assert!((amp(&unary(Op1::Highpass(fc), &v, dt)) - 0.7071).abs() < 0.01);
        // a decade above / below: -20 dB
        let v10 = sample(|t| (2.0 * PI * 10.0 * fc * t).sin(), 0.0, dt, 200_000);
        assert!((amp(&unary(Op1::Lowpass(fc), &v10, dt)) - 0.0995).abs() < 0.005);
    }
}
