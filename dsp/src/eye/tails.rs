// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Gaussian tail fitting for the dual-Dirac jitter model: the inverse normal, the straight-line fit and the fit of
//! both TIE tails that gives RJ and DJ. A different tail model replaces `fit_tails` here.

use super::{is_positive, Jitter, JitterFit, Q_BER_1E12, TAIL_EDGES};

/// Distinct TIE values needed in each tail: fewer is a quantised TIE, not a tail.
const TAIL_DISTINCT: usize = 5;
/// Points kept per tail for the fit, and outermost points left out when thinning.
const TAIL_POINTS: usize = 48;
const TAIL_SKIP: usize = 3;
/// Least-squares line t = phase + n period through (n, t).
pub(super) fn line_fit(n: &[f64], t: &[f64]) -> Option<(f64, f64)> {
    let k = n.len() as f64;
    if n.len() < 2 {
        return None;
    }
    let (mn, mt) = (n.iter().sum::<f64>() / k, t.iter().sum::<f64>() / k);
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (a, b) in n.iter().zip(t) {
        sxy += (a - mn) * (b - mt);
        sxx += (a - mn) * (a - mn);
    }
    if !is_positive(sxx) {
        return None;
    }
    let p = sxy / sxx;
    Some((mt - p * mn, p))
}

/// Inverse of the standard normal CDF (Acklam's rational approximation, relative error < 1.2e-9).
pub fn norm_inv(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969683028665376e1,
        2.209460984245205e2,
        -2.759285104469687e2,
        1.383_577_518_672_69e2,
        -3.066479806614716e1,
        2.506628277459239,
    ];
    const B: [f64; 5] =
        [-5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2, 6.680131188771972e1, -1.328068155288572e1];
    const C: [f64; 6] = [
        -7.784894002430293e-3,
        -3.223964580411365e-1,
        -2.400758277161838,
        -2.549732539343734,
        4.374664141464968,
        2.938163982698783,
    ];
    const D: [f64; 4] = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996, 3.754408661907416];
    let p = p.clamp(1e-300, 1.0 - 1e-16);
    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    if p < 0.02425 {
        tail((-2.0 * p.ln()).sqrt())
    } else if p > 1.0 - 0.02425 {
        -tail((-2.0 * (1.0 - p).ln()).sqrt())
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    }
}

/// Gaussian tail fit on the Q scale: the tail points (x, p) with p = ρ Φ((x - μ) / σ); for each weight ρ on a grid
/// the line x = μ + σ Φ⁻¹(p / ρ) is fitted, the ρ with the smallest normalised residual wins. Returns (μ, σ).
fn tail_fit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let p_max = pts.iter().fold(0.0f64, |a, x| a.max(x.1));
    let mut best: Option<(f64, f64, f64)> = None;
    let mut rho = 1.0;
    while rho >= 2.5 * p_max && rho > 0.04 {
        let z: Vec<f64> = pts.iter().map(|&(_, p)| norm_inv(p / rho)).collect();
        let x: Vec<f64> = pts.iter().map(|&(x, _)| x).collect();
        if let Some((mu, sigma)) = line_fit(&z, &x) {
            if sigma > 0.0 {
                let rss: f64 = z.iter().zip(&x).map(|(z, x)| (x - mu - sigma * z).powi(2)).sum::<f64>()
                    / (sigma * sigma * z.len() as f64);
                if best.is_none_or(|b| rss < b.2) {
                    best = Some((mu, sigma, rss));
                }
            }
        }
        rho -= 0.05;
    }
    best.map(|b| (b.0, b.1))
}

/// Tail points of the sorted `s` (outer end first) up to rank `k_max`: (value, CDF at the mid rank). Values within
/// `tol` are one point, so a quantised TIE gives its few steps and not a flat tail. Long tails are thinned evenly in
/// log p, without the `TAIL_SKIP` outermost points, which scatter most.
fn tail_points(s: &[f64], k_max: usize, tol: f64) -> Vec<(f64, f64)> {
    let n = s.len() as f64;
    let mut pts = Vec::new();
    let mut i = 0;
    while i < k_max.min(s.len()) {
        let mut j = i + 1;
        while j < s.len() && s[j] - s[i] <= tol {
            j += 1;
        }
        pts.push((s[i..j].iter().sum::<f64>() / (j - i) as f64, (i + j) as f64 / 2.0 / n));
        i = j;
    }
    if pts.len() <= TAIL_POINTS {
        return pts;
    }
    let (first, last) = (TAIL_SKIP as f64 + 1.0, pts.len() as f64);
    let mut idx: Vec<usize> = (0..TAIL_POINTS)
        .map(|k| (first * (last / first).powf(k as f64 / (TAIL_POINTS - 1) as f64) - 1.0).round() as usize)
        .collect();
    idx.dedup();
    idx.into_iter().map(|k| pts[k]).collect()
}

/// The dual-Dirac fit on the sorted TIE; false when the tails do not carry it. A sigma well above the RMS cannot be
/// (rms^2 = (DJ/2)^2 + RJ^2), it comes from a few stray points.
pub(super) fn fit_tails(s: &[f64], j: &mut Jitter) -> bool {
    let n = s.len();
    if n < TAIL_EDGES {
        return false;
    }
    let p_max = (0.05f64).max(10.0 / n as f64).min(0.2);
    let k_max = ((p_max * n as f64) as usize).max(3);
    let tol = 1e-3 * j.rms;
    let rev: Vec<f64> = s.iter().rev().map(|x| -x).collect();
    let (left, right) = (tail_points(s, k_max, tol), tail_points(&rev, k_max, tol));
    if left.len() < TAIL_DISTINCT || right.len() < TAIL_DISTINCT {
        return false;
    }
    let (Some((ml, sl)), Some((mr, sr))) = (tail_fit(&left), tail_fit(&right)) else { return false };
    let rj = 0.5 * (sl + sr);
    if rj > 1.25 * j.rms {
        return false;
    }
    // right mean below left mean is no DJ: one Gaussian in the middle
    let (a, b) = if ml <= -mr { (ml, -mr) } else { (0.5 * (ml - mr), 0.5 * (ml - mr)) };
    j.rj_rms = rj;
    j.dj_pp = b - a;
    j.tj_pp = j.dj_pp + 2.0 * Q_BER_1E12 * rj;
    (j.left, j.right) = ((a, sl), (b, sr));
    j.fit = JitterFit::Tails;
    true
}
