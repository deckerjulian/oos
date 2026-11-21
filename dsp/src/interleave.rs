// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The error budget of an interleaved ADC from one sine record (issue #335, step 0).
//!
//! Consecutive samples of a channel come from `m` cores in turn (8, 4, 2 or 1 on the DHO800/900, by the channel set
//! and the rate; docs/protocols/adc-mode.md 6). A core that differs from the others shows as a fixed pattern: an
//! offset as tones at k fs/m, a gain or a timing error as tones at k fs/m +- f_in (a timing error grows with f_in, a
//! gain error with the amplitude). `budget` fits the sine on every core's own samples, so each core gets its offset,
//! gain and time error, and splits the rest of the record into harmonics, named extra tones (the image of a
//! generator's own sample rate) and noise. It says which correction would gain how much at that frequency, before
//! one is built. No hardware: it works on any record, from the unit or from a file.

use crate::bandwidth::tone;
use crate::bode::sine_fit;
use crate::spectrum::distortion;
use std::f64::consts::PI;

/// One core's difference from the mean of all cores.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Core {
    /// offset, in the units of the record
    pub offset: f64,
    /// gain, relative (0.001 = 0.1 % more than the mean)
    pub gain: f64,
    /// time error in seconds (positive: the core samples late)
    pub skew: f64,
}

/// The budget of one record. Levels are dBc, relative to the fundamental; `None` where a part is not there.
#[derive(Clone, Debug, PartialEq)]
pub struct Budget {
    /// frequency of the fundamental (Hz; the alias below fs/2 for a sine above it)
    pub f: f64,
    /// its amplitude, in the units of the record
    pub amp: f64,
    /// the cores in the order of the record's samples: sample k comes from core k mod m
    pub cores: Vec<Core>,
    /// the cores' offset pattern
    pub offset_dbc: f64,
    /// their gain pattern
    pub gain_dbc: f64,
    /// their timing pattern
    pub skew_dbc: f64,
    /// harmonics 2..=`HARMONICS` together, after the core patterns are taken off
    pub harmonics_dbc: f64,
    /// each extra tone asked for, (frequency, level); NaN for one that aliases onto the fundamental
    pub extra: Vec<(f64, f64)>,
    /// what is left: noise and everything not named above
    pub noise_dbc: f64,
    /// SINAD of the record without the extra tones (they belong to the source, not to the scope)
    pub sinad_db: f64,
    /// the same with the core patterns taken off as well (what a full interleave correction could reach)
    pub sinad_matched_db: f64,
}

/// Harmonics the budget names.
pub const HARMONICS: usize = 9;

/// The budget of `v` (sample period `dt`) for `m` interleaved cores, with the levels of the tones at the frequencies
/// in `extra` (Hz; above fs/2 they are taken at their alias). None when the record holds no sine that carries at least
/// half of its AC power, is shorter than 64 samples per core, or when the fundamental lies within 1 % of a multiple of fs / (2 m): there every core sees
/// it at its own Nyquist or at DC, and its gain and phase cannot be told apart.
pub fn budget(v: &[f32], dt: f64, m: usize, extra: &[f64]) -> Option<Budget> {
    if m == 0 || v.len() < 64 * m || dt <= 0.0 {
        return None;
    }
    let f = tone(v, dt)?;
    let q = (2.0 * f * m as f64 * dt).fract();
    if m > 1 && !(0.01..=0.99).contains(&q) {
        return None;
    }
    let (amp, phase, mean) = sine_fit(v, dt, f)?;
    // the sine has to carry most of the record's AC power, or there is no fundamental to measure against
    let ac = v.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / v.len() as f64;
    if !(amp > 0.0) || amp * amp / 2.0 < 0.5 * ac {
        return None;
    }
    let w = 2.0 * PI * f;
    // each core on its own samples: they start at k dt and are m dt apart
    let mut fits = Vec::with_capacity(m);
    for k in 0..m {
        let sub: Vec<f32> = v[k..].iter().step_by(m).copied().collect();
        let (a, p, c) = sine_fit(&sub, m as f64 * dt, f)?;
        let late = (p - w * k as f64 * dt - phase + PI).rem_euclid(2.0 * PI) - PI;
        fits.push((a, late / w, c));
    }
    let n = m as f64;
    let (ga, gs, go) = fits.iter().fold((0.0, 0.0, 0.0), |s, x| (s.0 + x.0 / n, s.1 + x.1 / n, s.2 + x.2 / n));
    let cores: Vec<Core> =
        fits.iter().map(|&(a, s, c)| Core { offset: c - go, gain: a / ga - 1.0, skew: s - gs }).collect();
    // powers of the patterns against the fundamental's A^2 / 2
    let p1 = amp * amp / 2.0;
    let db = |x: f64| 10.0 * x.max(1e-30).log10();
    let mean_sq = |g: &dyn Fn(&Core) -> f64| cores.iter().map(|c| g(c).powi(2)).sum::<f64>() / n;
    let offset_dbc = db(mean_sq(&|c| c.offset) / p1);
    // a gain error g is an error A g sin(..) of power A^2 g^2 / 2, a time error t one of A w t cos(..): against the
    // fundamental's A^2 / 2 that is g^2 and (w t)^2
    let gain_dbc = db(mean_sq(&|c| c.gain));
    let skew_dbc = db(mean_sq(&|c| w * c.skew));
    // the extra tones off the record first, so that neither SINAD counts them
    let fs = 1.0 / dt;
    let mut own: Vec<f32> = v.to_vec();
    let mut levels = Vec::with_capacity(extra.len());
    for &fx in extra {
        let mut fa = fx.rem_euclid(fs);
        if fa > fs / 2.0 {
            fa = fs - fa;
        }
        // a tone that aliases onto the fundamental (the generator's image when the scope samples at its 156.25 MSa/s)
        // cannot be told from it: not taken off, its level not known
        if (fa - f).abs() < 8.0 / (v.len() as f64 * dt) {
            levels.push((fx, f64::NAN));
            continue;
        }
        let (a, p, _) = sine_fit(&own, dt, fa)?;
        let wa = 2.0 * PI * fa;
        for (i, y) in own.iter_mut().enumerate() {
            *y -= (a * (wa * i as f64 * dt + p).sin()) as f32;
        }
        levels.push((fx, db(a * a / 2.0 / p1)));
    }
    // then each core's sine replaced by the common one: what is left of the patterns is their part that is not a sine
    // at f (none for a clean sine)
    let matched: Vec<f32> = own
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            let c = &cores[i % m];
            let t = i as f64 * dt;
            let this = amp * (1.0 + c.gain) * (w * (t + c.skew) + phase).sin() + mean + c.offset;
            let common = amp * (w * t + phase).sin() + mean;
            (x as f64 - this + common) as f32
        })
        .collect();
    let raw = distortion(&own, dt, Some(f), HARMONICS)?;
    let clean = distortion(&matched, dt, Some(f), HARMONICS)?;
    Some(Budget {
        f,
        amp,
        cores,
        offset_dbc,
        gain_dbc,
        skew_dbc,
        harmonics_dbc: clean.thd_db,
        extra: levels,
        noise_dbc: -clean.snr_db,
        sinad_db: raw.sinad_db,
        sinad_matched_db: clean.sinad_db,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record of 8 cores at 1.25 GSa/s: a sine of amplitude 1 at `f` with a 2nd harmonic at -60 dBc, an extra tone
    /// at `fx` (-50 dBc), the cores' errors and some noise (a fixed generator, the same on every run).
    fn record(f: f64, cores: &[Core], fx: f64, noise: f64) -> Vec<f32> {
        let dt = 0.8e-9;
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        (0..65_536)
            .map(|i| {
                let c = &cores[i % cores.len()];
                let t = i as f64 * dt + c.skew;
                let s = (1.0 + c.gain) * (2.0 * PI * f * t + 0.4).sin()
                    + 1e-3 * (2.0 * 2.0 * PI * f * t).sin()
                    + 10f64.powf(-50.0 / 20.0) * (2.0 * PI * fx * i as f64 * dt).sin();
                (s + 0.2 + c.offset + noise * rnd()) as f32
            })
            .collect()
    }

    fn eight() -> Vec<Core> {
        let o = [3e-3, -1e-3, 0.0, 2e-3, -4e-3, 1e-3, 0.5e-3, -1.5e-3];
        let g = [1e-3, -0.5e-3, 0.0, 0.8e-3, -1.2e-3, 0.3e-3, 0.1e-3, -0.5e-3];
        let s = [2e-12, -1e-12, 0.0, 1.5e-12, -3e-12, 0.5e-12, 1e-12, -1e-12];
        (0..8).map(|k| Core { offset: o[k], gain: g[k], skew: s[k] }).collect()
    }

    // Break it catches: a core's samples fitted at the wrong start time (every core then seems late by k dt), the
    // offset, gain and time error of one core given to another, a pattern level that is not its power against the
    // fundamental.
    #[test]
    fn every_core_gets_its_own_offset_gain_and_time() {
        let want = eight();
        let f = 97.3e6;
        let b = budget(&record(f, &want, 31.1e6, 1e-3), 0.8e-9, 8, &[31.1e6]).unwrap();
        assert!((b.f - f).abs() < 1e3, "f {}", b.f);
        assert!((b.amp - 1.0).abs() < 1e-3, "amp {}", b.amp);
        for (k, (got, w)) in b.cores.iter().zip(&want).enumerate() {
            assert!((got.offset - w.offset).abs() < 1e-4, "core {} offset {:?}", k, got);
            assert!((got.gain - w.gain).abs() < 1e-4, "core {} gain {:?}", k, got);
            assert!((got.skew - w.skew).abs() < 0.2e-12, "core {} skew {:?}", k, got);
        }
        // the means of the patterns: offset 2.36e-3 rms, gain 0.68e-3 rms, time 1.62 ps rms
        let rms = |g: &dyn Fn(&Core) -> f64| (want.iter().map(|c| g(c).powi(2)).sum::<f64>() / 8.0).sqrt();
        let (o, g, s) = (rms(&|c| c.offset), rms(&|c| c.gain), rms(&|c| c.skew));
        let dbc = |x: f64| 20.0 * x.log10();
        assert!((b.offset_dbc - dbc(o / (1.0 / 2f64.sqrt()))).abs() < 0.3, "{}", b.offset_dbc);
        assert!((b.gain_dbc - dbc(g)).abs() < 0.3, "{}", b.gain_dbc);
        assert!((b.skew_dbc - dbc(2.0 * PI * f * s)).abs() < 0.5, "{}", b.skew_dbc);
        assert!((b.harmonics_dbc + 60.0).abs() < 1.0, "harmonics {}", b.harmonics_dbc);
        assert!((b.extra[0].1 + 50.0).abs() < 0.5, "extra {:?}", b.extra);
        // white noise of 1e-3 peak to peak: 2.9e-4 rms, -67.7 dBc
        assert!((b.noise_dbc + 67.7).abs() < 1.5, "noise {}", b.noise_dbc);
        // the patterns cost SINAD here: matching them brings it close to the harmonics, tone and noise
        assert!(b.sinad_matched_db > b.sinad_db + 3.0, "{} -> {}", b.sinad_db, b.sinad_matched_db);
    }

    // Break it catches: cores that are equal reported as different, a budget for a record that cannot carry one.
    #[test]
    fn equal_cores_show_nothing_and_bad_records_no_budget() {
        let same = vec![Core { offset: 0.0, gain: 0.0, skew: 0.0 }; 4];
        let b = budget(&record(10.17e6, &same, 1.0e6, 0.0), 0.8e-9, 4, &[]).unwrap();
        assert!(b.cores.iter().all(|c| c.offset.abs() < 1e-5 && c.gain.abs() < 1e-5 && c.skew.abs() < 1e-14));
        assert!(b.offset_dbc < -90.0 && b.gain_dbc < -90.0 && b.skew_dbc < -90.0, "{:?}", b);
        assert!((b.sinad_matched_db - b.sinad_db).abs() < 0.1);
        // the fundamental at fs / 16 = 78.125 MHz: every one of 8 cores sees it at its own Nyquist frequency
        assert!(budget(&record(78.125e6, &same, 1.0e6, 1e-3), 0.8e-9, 8, &[]).is_none());
        // a tone at fs - f aliases onto the fundamental: its level is not known, and the fundamental stays
        let b = budget(&record(10.17e6, &same, 1.0e6, 1e-3), 0.8e-9, 4, &[1.25e9 - 10.17e6]).unwrap();
        assert!(b.extra[0].1.is_nan() && b.sinad_db > 40.0, "{:?}", b);
        assert!(budget(&[0.0; 100], 0.8e-9, 8, &[]).is_none());
        // noise alone, and a sine under noise twice its power
        let none = vec![Core { offset: 0.0, gain: 0.0, skew: 0.0 }; 8];
        let mut noise = record(10.17e6, &none, 1.0e6, 20.0);
        assert!(budget(&noise, 0.8e-9, 8, &[]).is_none());
        noise = record(10.17e6, &none, 1.0e6, 3.5);
        assert!(budget(&noise, 0.8e-9, 8, &[]).is_none());
        assert!(budget(&[0.0; 4096], 0.8e-9, 0, &[]).is_none());
    }
}
