// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Mask (pass/fail) test: an envelope around a reference waveform, and records tested against it.
//!
//! The mask has one column per reference sample. Column i allows the band [min - tol_y, max + tol_y], with min/max
//! taken over the reference samples i - tol_x ..= i + tol_x, so the envelope is widened by tol_x samples in time and
//! tol_y volts in amplitude. A record passes when every sample lies inside the band of its column; a record of
//! another length is mapped onto the columns proportionally (both cover the same time window).

use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    /// lower limit per column (volts); -inf where the reference had no data
    pub lo: Vec<f32>,
    /// upper limit per column (volts); +inf where the reference had no data
    pub hi: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    pub pass: bool,
    /// samples outside the band
    pub violations: usize,
    /// first sample outside the band
    pub first: Option<usize>,
}

/// Sliding-window extreme over `v` (window i - w ..= i + w), NaN samples ignored; `better(a, b)` = a replaces b.
/// Monotonic deque: O(n) whatever the window. Columns whose window holds no finite sample get NaN.
fn sliding(v: &[f32], w: usize, better: impl Fn(f32, f32) -> bool) -> Vec<f32> {
    let n = v.len();
    let mut out = vec![f32::NAN; n];
    let mut dq: VecDeque<usize> = VecDeque::new();
    let mut next = 0; // next sample to enter the window
    for (i, o) in out.iter_mut().enumerate() {
        let end = (i + w).min(n.saturating_sub(1));
        while next <= end && next < n {
            let x = v[next];
            if x.is_finite() {
                while let Some(&b) = dq.back() {
                    if better(x, v[b]) || x == v[b] {
                        dq.pop_back();
                    } else {
                        break;
                    }
                }
                dq.push_back(next);
            }
            next += 1;
        }
        while let Some(&f) = dq.front() {
            if f + w < i {
                dq.pop_front();
            } else {
                break;
            }
        }
        if let Some(&f) = dq.front() {
            *o = v[f];
        }
    }
    out
}

/// Mask from the reference waveform `v`, widened by `tol_x` samples and `tol_y` volts.
pub fn build(v: &[f32], tol_x: usize, tol_y: f32) -> Mask {
    let tol_y = tol_y.abs();
    let mn = sliding(v, tol_x, |a, b| a < b);
    let mx = sliding(v, tol_x, |a, b| a > b);
    let lo = mn.iter().map(|&x| if x.is_finite() { x - tol_y } else { f32::NEG_INFINITY }).collect();
    let hi = mx.iter().map(|&x| if x.is_finite() { x + tol_y } else { f32::INFINITY }).collect();
    Mask { lo, hi }
}

impl Mask {
    pub fn len(&self) -> usize {
        self.lo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lo.is_empty()
    }

    /// Column of sample k of a record with `n` samples.
    pub fn column(&self, k: usize, n: usize) -> usize {
        if n == self.len() || n == 0 {
            k.min(self.len().saturating_sub(1))
        } else {
            ((k as u64 * self.len() as u64) / n as u64).min(self.len() as u64 - 1) as usize
        }
    }

    /// Union of the bands of the columns covering the fraction f0..f1 (0..1) of the window: (lowest lo, highest hi),
    /// for drawing the allowed region at a screen column.
    pub fn band(&self, f0: f64, f1: f64) -> (f32, f32) {
        let n = self.len();
        if n == 0 {
            return (f32::NEG_INFINITY, f32::INFINITY);
        }
        let a = ((f0 * n as f64).floor().max(0.0) as usize).min(n - 1);
        let b = ((f1 * n as f64).ceil() as usize).clamp(a + 1, n);
        let lo = self.lo[a..b].iter().copied().fold(f32::INFINITY, f32::min);
        let hi = self.hi[a..b].iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (lo, hi)
    }

    /// Tests the waveform `v` (same time window as the reference) against the mask. NaN samples are skipped.
    pub fn test(&self, v: &[f32]) -> Verdict {
        let mut violations = 0;
        let mut first = None;
        if !self.is_empty() {
            for (k, &x) in v.iter().enumerate() {
                if !x.is_finite() {
                    continue;
                }
                let c = self.column(k, v.len());
                if x < self.lo[c] || x > self.hi[c] {
                    violations += 1;
                    first.get_or_insert(k);
                }
            }
        }
        Verdict { pass: violations == 0, violations, first }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{add_noise, sample, Pulse, PulseTrain};

    const DT: f64 = 1e-6;
    const N: usize = 4000;

    /// 1 kHz square 0..3.3 V, 2 us edges, rising edges at samples 100, 1100, ...; `special`: pulse 2 replaced.
    fn square(special: Option<Pulse>, shift: f64, seed: u64) -> Vec<f32> {
        let mut p = PulseTrain::square(1e3, 0.0, 3.3, 2e-6);
        if let Some(s) = special {
            p.every = 4;
            p.at = 2;
            p.special = s;
        }
        let mut v = sample(|t| p.v(t), shift - 100e-6, DT, N);
        add_noise(&mut v, 0.02, seed);
        v
    }

    #[test]
    fn the_same_signal_passes() {
        let m = build(&square(None, 0.0, 1), 5, 0.2);
        assert_eq!(m.len(), N);
        for seed in 2..12 {
            // other noise and +-2 us trigger jitter stay inside 5 samples / 0.2 V
            let jitter = (seed as f64 - 7.0) * 0.4e-6;
            let v = square(None, jitter, seed);
            let r = m.test(&v);
            assert!(r.pass, "seed {}: {:?}", seed, r);
        }
    }

    #[test]
    fn a_runt_fails() {
        let m = build(&square(None, 0.0, 1), 5, 0.2);
        // pulse 2 reaches only 40 %: fails from its start (sample 2100) on
        let r = m.test(&square(Some(Pulse { width: 0.5e-3, height: 0.4 }), 0.0, 3));
        assert!(!r.pass);
        assert!(r.violations > 400, "{:?}", r);
        let first = r.first.unwrap();
        assert!((2100..2110).contains(&first), "{}", first);
    }

    #[test]
    fn a_glitch_fails() {
        let m = build(&square(None, 0.0, 1), 5, 0.2);
        // a 20 us glitch instead of the 500 us pulse at sample 2100
        let r = m.test(&square(Some(Pulse { width: 20e-6, height: 1.0 }), 0.0, 4));
        assert!(!r.pass);
        assert!((2115..2140).contains(&r.first.unwrap()), "{:?}", r);
        // a single spike of 0.5 V on a flat part fails too
        let mut v = square(None, 0.0, 5);
        v[1250] += 0.5;
        let r = m.test(&v);
        assert_eq!((r.pass, r.violations, r.first), (false, 1, Some(1250)));
    }

    #[test]
    fn envelope_and_mapping() {
        // a step at sample 50 of 100, tol_x 3: columns 47..=52 allow both levels
        let v: Vec<f32> = (0..100).map(|i| if i < 50 { 0.0 } else { 1.0 }).collect();
        let m = build(&v, 3, 0.1);
        assert_eq!((m.lo[40], m.hi[40]), (-0.1, 0.1));
        assert_eq!((m.lo[47], m.hi[47]), (-0.1, 1.1));
        assert_eq!((m.lo[52], m.hi[52]), (-0.1, 1.1));
        assert_eq!((m.lo[53], m.hi[53]), (0.9, 1.1));
        // a record at half the resolution maps onto every second column
        let half: Vec<f32> = (0..50).map(|i| if i < 25 { 0.05 } else { 1.0 }).collect();
        assert!(m.test(&half).pass);
        assert_eq!(m.band(0.0, 0.01), (-0.1, 0.1));
        assert_eq!(m.band(0.45, 0.55), (-0.1, 1.1));
        // NaN in the reference: no constraint there; NaN in the record: skipped
        let mut r = v.clone();
        r[10] = f32::NAN;
        let m2 = build(&r, 0, 0.1);
        assert_eq!((m2.lo[10], m2.hi[10]), (f32::NEG_INFINITY, f32::INFINITY));
        assert!(m2.test(&r).pass);
    }
}
