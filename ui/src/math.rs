// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Math channel operations (computed by oos-dsp).

pub use oos_dsp::math::Window;
use oos_dsp::math::{binary, unary, Op1, Op2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MathOp {
    Off,
    Add,
    Sub,
    Mul,
    Div,
    Intg,
    Diff,
    Abs,
    Sqrt,
    Lg,
    Ln,
    Exp,
    /// A x + B
    Lin,
    /// a formula over the channels (`oos_dsp::expr`)
    Expr,
    Lpf,
    Hpf,
    Bpf,
    Bsf,
    Fft,
}

pub const MATH_OPS: [MathOp; 19] = [
    MathOp::Off, MathOp::Add, MathOp::Sub, MathOp::Mul, MathOp::Div, MathOp::Intg, MathOp::Diff, MathOp::Abs, MathOp::Sqrt, MathOp::Lg,
    MathOp::Ln, MathOp::Exp, MathOp::Lin, MathOp::Expr, MathOp::Lpf, MathOp::Hpf, MathOp::Bpf, MathOp::Bsf, MathOp::Fft,
];

/// Math channels (8, math on math).
pub const NMATH: usize = 8;

/// Name of a math source: 0..3 = CH1..CH4, 4.. = M1..M8.
pub fn src_name(i: usize) -> String {
    if i < 4 {
        format!("CH{}", i + 1)
    } else {
        format!("M{}", i - 3)
    }
}

pub const WINDOWS: [Window; 5] = [Window::Rect, Window::Hann, Window::Hamming, Window::Blackman, Window::Flattop];

impl MathOp {
    pub fn name(self) -> &'static str {
        match self {
            MathOp::Off => "Off",
            MathOp::Add => "A+B",
            MathOp::Sub => "A-B",
            MathOp::Mul => "AxB",
            MathOp::Div => "A/B",
            MathOp::Intg => "Intg",
            MathOp::Diff => "Diff",
            MathOp::Abs => "Abs",
            MathOp::Sqrt => "Sqrt",
            MathOp::Lg => "Lg",
            MathOp::Ln => "Ln",
            MathOp::Lin => "Ax+B",
            MathOp::Expr => "Formula",
            MathOp::Exp => "Exp",
            MathOp::Lpf => "Low-pass",
            MathOp::Hpf => "High-pass",
            MathOp::Bpf => "Band-pass",
            MathOp::Bsf => "Band-stop",
            MathOp::Fft => "FFT",
        }
    }
    pub fn two_sources(self) -> bool {
        matches!(self, MathOp::Add | MathOp::Sub | MathOp::Mul | MathOp::Div)
    }
    pub fn unit(self) -> &'static str {
        match self {
            MathOp::Mul => "V\u{b2}",
            MathOp::Div | MathOp::Lg | MathOp::Ln | MathOp::Exp | MathOp::Expr => "",
            MathOp::Intg => "Vs",
            MathOp::Diff => "V/s",
            MathOp::Fft => "dB",
            _ => "V",
        }
    }
    /// Label of the formula, e.g. "CH1 + CH2", "Intg(M1)" (sources as `src_name` indices).
    pub fn formula(self, a: usize, b: usize) -> String {
        let (a, b) = (src_name(a), src_name(b));
        match self {
            MathOp::Add => format!("{} + {}", a, b),
            MathOp::Sub => format!("{} - {}", a, b),
            MathOp::Mul => format!("{} x {}", a, b),
            MathOp::Div => format!("{} / {}", a, b),
            op => format!("{}({})", op.name(), a),
        }
    }
    /// Filters with a corner/centre frequency; band filters also have a bandwidth.
    pub fn has_fc(self) -> bool {
        matches!(self, MathOp::Lpf | MathOp::Hpf | MathOp::Bpf | MathOp::Bsf)
    }
    pub fn has_bw(self) -> bool {
        matches!(self, MathOp::Bpf | MathOp::Bsf)
    }
}

/// One math channel's settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MathCh {
    pub op: MathOp,
    pub a: usize,
    pub b: usize,
    pub scale: usize,
    pub fc: f32,
    pub bw: f32,
    pub offset: f32,
    pub win: Window,
    /// A and B of Ax+B
    pub lin: (f32, f32),
}

impl Default for MathCh {
    fn default() -> MathCh {
        MathCh { op: MathOp::Off, a: 0, b: 1, scale: 3, fc: 10e3, bw: 2e3, offset: 0.0, win: Window::Hann, lin: (1.0, 0.0) }
    }
}

/// The math waveform (not for FFT/Off); `fc` = filter corner/centre frequency, `bw` = band filter bandwidth.
pub fn compute(op: MathOp, a: &[f32], b: &[f32], dt: f64, fc: f64, bw: f64) -> Vec<f32> {
    match op {
        MathOp::Add => binary(Op2::Add, a, b),
        MathOp::Sub => binary(Op2::Sub, a, b),
        MathOp::Mul => binary(Op2::Mul, a, b),
        MathOp::Div => binary(Op2::Div, a, b),
        MathOp::Intg => unary(Op1::Intg, a, dt),
        MathOp::Diff => unary(Op1::Diff, a, dt),
        MathOp::Abs => unary(Op1::Abs, a, dt),
        MathOp::Sqrt => unary(Op1::Sqrt, a, dt),
        MathOp::Lg => unary(Op1::Lg, a, dt),
        MathOp::Ln => unary(Op1::Ln, a, dt),
        // applied by the caller, which knows A and B
        MathOp::Lin | MathOp::Expr => a.to_vec(),
        MathOp::Exp => unary(Op1::Exp, a, dt),
        MathOp::Lpf => unary(Op1::Lowpass(fc), a, dt),
        MathOp::Hpf => unary(Op1::Highpass(fc), a, dt),
        MathOp::Bpf => unary(Op1::Bandpass(fc, bw), a, dt),
        MathOp::Bsf => unary(Op1::Bandstop(fc, bw), a, dt),
        MathOp::Off | MathOp::Fft => Vec::new(),
    }
}

/// 1-2-5 scale ladder for the math channel: 1 n .. 1 G (units per division).
pub fn math_scales() -> Vec<f64> {
    let mut v = Vec::new();
    let mut d = 1e-9;
    while d < 2e9 {
        for m in [1.0, 2.0, 5.0] {
            v.push(d * m);
        }
        d *= 10.0;
    }
    v
}
