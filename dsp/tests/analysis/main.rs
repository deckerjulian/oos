// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Spectrum analyser, eye/jitter, power and identify against synthetic signals with known answers.
//!
//! One module per analyser, and here the helpers and imports they share. The tests of a new analyser get a module of
//! their own and a `mod` line here; a test of an existing one goes into its module.

use oos_dsp::decode::Parity;
use oos_dsp::eye::{self, Eye};
use oos_dsp::identify::{self, DecoderHint, Finding, SignalKind, TriggerHint};
use oos_dsp::math::Window;
use oos_dsp::power;
use oos_dsp::sim::{self, add_noise, sample, Logic, PulseTrain};
use oos_dsp::spectrum::{self, Detector, Settings, Trace};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

mod drives;
mod identify_signals;
mod jitter;
mod power_analysis;
mod spectra;

/// Deterministic Gaussian noise (xorshift + Box-Muller), standard deviation `sigma`.
struct Gauss(u64);

impl Gauss {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn next(&mut self, sigma: f64) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        sigma * (-2.0 * u.ln()).sqrt() * (2.0 * PI * v).cos()
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// PRBS7 (x^7 + x^6 + 1) bit sequence.
/// PRBS7 from its eighth bit (the start the eye tests were written with).
fn prbs7(n: usize) -> Vec<bool> {
    oos_dsp::hss::prbs_bits(7, n + 7).unwrap()[7..].iter().map(|&b| b == 1).collect()
}

/// Piecewise linear function through (t, value) points.
fn pwl(p: &[(f64, f64)], t: f64) -> f64 {
    if t <= p[0].0 {
        return p[0].1;
    }
    for w in p.windows(2) {
        if t <= w[1].0 {
            return w[0].1 + (w[1].1 - w[0].1) * (t - w[0].0) / (w[1].0 - w[0].0);
        }
    }
    p[p.len() - 1].1
}
