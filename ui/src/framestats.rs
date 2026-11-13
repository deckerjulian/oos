// SPDX-License-Identifier: GPL-2.0-or-later

//! Measurement statistics over every frame (fork, docs/fork/perf-plan.md item 1). The measurements are made on the
//! newest record of a screen refresh (`Ui::measure_last`, `MeasWorker`), so their statistics counted a few records a
//! second. The cheap ones (max, min, peak-peak, mean, RMS, standard deviation) are made here for every frame the deposit
//! workers draw, on the samples the record path would measure (`Ui::meas_slice`: the screen window of the frame, when it
//! holds more samples than the screen record), with the definitions of `oos_dsp::measure::analyze_min` (sums split over
//! lanes: equal to 1e-12 relative, extremes exact).
//!
//! Frames whose record is measured on its 3600 interpolated points (short time bases) are not counted: the sinc
//! points overshoot between the samples, so max, min and peak-peak on the raw samples would be another measurement.
//! `Counters::measured` against `used` shows how many frames the statistics cover.
//!
//! Envelope frames (min/max pairs: records above 1 M points through the FPGA compression, roll, scan) count for max,
//! min and peak-peak only (`needs_samples`); the record measurements of such frames show the mean-type ones as not
//! available (`Ui::ana_get`).

use crate::board::Frame;
use crate::model::*;

/// Measurements that weigh every sample (mean, RMS, deviation, area). The min/max pairs of an envelope frame (FPGA
/// compression of records above 1 M points, roll, scan) are no samples of the signal: these come out wrong on them,
/// while maximum, minimum and peak-peak stay right.
pub fn needs_samples(k: Meas) -> bool {
    matches!(k, Meas::Vavg | Meas::Vrms | Meas::VavgCyc | Meas::VrmsCyc | Meas::VrmsAc | Meas::StdDev | Meas::Area | Meas::AreaCyc)
}

/// The measurements made per frame.
pub const CHEAP: [Meas; 7] = [Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vavg, Meas::Vrms, Meas::StdDev, Meas::VrmsAc];

/// Statistics of one measurement over frames, as the Measure page keeps them (`Ui::mstat`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fold {
    pub n: u64,
    pub sum: f64,
    pub sumsq: f64,
    pub min: f64,
    pub max: f64,
}

impl Fold {
    pub fn push(&mut self, v: f64) {
        if self.n == 0 {
            (self.min, self.max) = (v, v);
        }
        self.n += 1;
        self.sum += v;
        self.sumsq += v * v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }
    pub fn add(&mut self, o: &Fold) {
        if o.n == 0 {
            return;
        }
        if self.n == 0 {
            (self.min, self.max) = (o.min, o.max);
        }
        self.n += o.n;
        self.sum += o.sum;
        self.sumsq += o.sumsq;
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
    }
}

/// Largest avg^2 / variance for which `Values::of` takes the standard deviation from its one pass (sum of squares minus
/// the squared mean): the relative error is about 1.1e-16 x (1 + avg^2 / var), under 3e-14 here.
const ONE_PASS: f64 = 256.0;

/// Per (measurement, channel) of the Measure page that is made per frame: its statistics.
pub type Folds = Vec<((Meas, usize), Fold)>;

/// Empty statistics for the measurements of `m` made per frame.
pub fn folds_for(m: &Model) -> Folds {
    m.meas.iter().filter(|(k, c)| CHEAP.contains(k) && m.ch[*c].on).map(|&e| (e, Fold::default())).collect()
}

/// Adds `b` to `a` (the same measurements: merged by key).
pub fn add_folds(a: &mut Folds, b: &Folds) {
    for (k, f) in b {
        match a.iter_mut().find(|(x, _)| x == k) {
            Some((_, g)) => g.add(f),
            None => a.push((*k, *f)),
        }
    }
}

/// What `analyze_min` gives for the cheap measurements of `v`; None where it gives nothing (too few samples) or the
/// record path would measure a filtered copy (a gap).
struct Values {
    vmax: f64,
    vmin: f64,
    avg: f64,
    rms: f64,
    std: f64,
}

impl Values {
    fn of(v: &[f32], std: bool) -> Option<Values> {
        if v.len() < 4 {
            return None;
        }
        // the extremes (`fastdraw::min_max`, NEON), then the sums split over `L` lanes (vectorised): the same values as
        // `analyze_min` to about 1e-15 relative, the extremes exact. A NaN or infinity anywhere makes the sums not
        // finite (f32 samples cannot overflow an f64 sum): then None.
        let (vmin, vmax) = crate::fastdraw::min_max(v);
        const L: usize = 8;
        let (mut sum, mut sumsq) = ([0.0f64; L], [0.0f64; L]);
        let mut chunks = v.chunks_exact(L);
        for c in &mut chunks {
            let c: &[f32; L] = c.try_into().expect("L samples");
            for j in 0..L {
                let y = c[j] as f64;
                sum[j] += y;
                sumsq[j] += y * y;
            }
        }
        for (j, &x) in chunks.remainder().iter().enumerate() {
            sum[j] += x as f64;
            sumsq[j] += x as f64 * x as f64;
        }
        let n = v.len() as f64;
        let avg = sum.iter().sum::<f64>() / n;
        let ms = sumsq.iter().sum::<f64>() / n;
        if !(avg.is_finite() && ms.is_finite()) {
            return None;
        }
        let rms = ms.sqrt();
        let std = if !std {
            f64::NAN
        } else {
            // from the sums of the pass above where that is accurate: the subtraction loses about avg^2 / var of
            // the f64 precision, so with avg^2 <= ONE_PASS x var the error stays near 1e-14; else the second pass
            let var = ms - avg * avg;
            if var > 0.0 && avg * avg <= ONE_PASS * var {
                var.sqrt()
            } else {
                Self::std_two_pass(v, avg)
            }
        };
        Some(Values { vmax: vmax as f64, vmin: vmin as f64, avg, rms, std })
    }

    /// The standard deviation around `avg` in a second pass (`analyze_min`'s way).
    fn std_two_pass(v: &[f32], avg: f64) -> f64 {
        const L: usize = 8;
        let mut d = [0.0f64; L];
        let mut chunks = v.chunks_exact(L);
        for c in &mut chunks {
            let c: &[f32; L] = c.try_into().expect("L samples");
            for j in 0..L {
                let e = c[j] as f64 - avg;
                d[j] += e * e;
            }
        }
        for (j, &x) in chunks.remainder().iter().enumerate() {
            d[j] += (x as f64 - avg) * (x as f64 - avg);
        }
        (d.iter().sum::<f64>() / v.len() as f64).sqrt()
    }

    fn get(&self, k: Meas) -> f64 {
        match k {
            Meas::Vmax => self.vmax,
            Meas::Vmin => self.vmin,
            Meas::Vpp => self.vmax - self.vmin,
            Meas::Vavg => self.avg,
            Meas::Vrms => self.rms,
            Meas::StdDev | Meas::VrmsAc => self.std,
            _ => f64::NAN,
        }
    }
}

/// Folds the cheap measurements of frame `f` into `folds`, on the samples the record path would measure. Returns
/// whether the frame counted.
pub fn fold_frame(f: &Frame, m: &Model, folds: &mut Folds) -> bool {
    if folds.is_empty() {
        return false;
    }
    let n = crate::sparse::POINTS;
    let window = m.time_per_div() as f64 * HDIV as f64;
    let t_first = m.delay as f64 - window / 2.0;
    // the record's trigger point and screen window (`board::to_record`; roll and scan do not take the fast path)
    // min/max pairs of the live compression are no samples (compress.rs): the raw newest frame of the batch counts
    if f.roll || f.scan || crate::compress::live(f) {
        return false;
    }
    let ti = crate::board::trigger_point(f, m, n, window, t_first).unwrap_or(f.pre as f64);
    let len = f.ch.iter().map(|v| v.len()).max().unwrap_or(0);
    let i0 = (ti + t_first / f.dt).floor().max(0.0) as usize;
    let i1 = ((ti + (t_first + window) / f.dt).ceil().max(0.0) as usize).min(len);
    if i1 <= i0 + 1 || i1 - i0 <= n {
        return false;
    }
    let mut counted = false;
    let mut cache: [Option<Option<Values>>; NCH] = Default::default();
    for ((k, c), fold) in folds.iter_mut() {
        if f.envelope && needs_samples(*k) {
            continue;
        }
        let v = &f.ch[*c];
        if v.len() < 2 || i1 > v.len() {
            continue;
        }
        let std = m.meas.iter().any(|(x, y)| y == c && matches!(x, Meas::StdDev | Meas::VrmsAc));
        let vals = cache[*c].get_or_insert_with(|| Values::of(&v[i0..i1], std));
        if let Some(x) = vals.as_ref().map(|s| s.get(*k)).filter(|x| x.is_finite()) {
            fold.push(x);
            counted = true;
        }
    }
    counted
}

#[cfg(test)]
mod tests {
    use super::*;

    // Break it catches: a standard deviation from the one pass where the subtraction is inaccurate (a DC level with a
    // little noise), or one that differs from the two-pass value beyond 1e-12 relative; a NaN that is not refused
    #[test]
    fn std_from_one_pass_stays_within_1e_12_of_two_passes() {
        let mut x = 1u32;
        let mut rnd = move || {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((x >> 8) & 0xffff) as f32 / 65536.0 - 0.5
        };
        let n = 100_000;
        let cases: Vec<Vec<f32>> = vec![
            (0..n).map(|i| if (i / 900) % 2 == 0 { 3.0 } else { 0.0 } + 0.01 * rnd()).collect(),
            (0..n).map(|_| 1.0 + 0.0002 * rnd()).collect(),
            (0..n).map(|_| -2.5 + 0.03 * rnd()).collect(),
            (0..n).map(|i| if i % 5000 == 0 { 3.0 } else { 0.0 } + 0.001 * rnd()).collect(),
            (0..n).map(|i| 0.9 * (i as f32 / 400.0).sin()).collect(),
            vec![0.7; n],
        ];
        for (k, v) in cases.iter().enumerate() {
            let a = Values::of(v, true).unwrap();
            let b = Values::std_two_pass(v, a.avg);
            assert!((a.std - b).abs() <= 1e-12 * b.max(1e-12), "case {}: {} vs {}", k, a.std, b);
        }
        let mut v = cases[0].clone();
        v[777] = f32::NAN;
        assert!(Values::of(&v, true).is_none());
        v[777] = f32::INFINITY;
        assert!(Values::of(&v, false).is_none());
    }

    /// `Values::of` as it was (finite check, compare-select extremes and sums in one pass, std always in a second
    /// pass): the reference of the timing guard. (avg, rms, std, max, min)
    fn values_reference(v: &[f32]) -> Option<(f64, f64, f64, f32, f32)> {
        if v.len() < 4 || !v.iter().all(|x| x.is_finite()) {
            return None;
        }
        const L: usize = 8;
        let (mut hi, mut lo) = ([v[0]; L], [v[0]; L]);
        let (mut sum, mut sumsq) = ([0.0f64; L], [0.0f64; L]);
        let mut chunks = v.chunks_exact(L);
        for c in &mut chunks {
            let c: &[f32; L] = c.try_into().expect("L samples");
            for j in 0..L {
                let x = c[j];
                hi[j] = if x > hi[j] { x } else { hi[j] };
                lo[j] = if x < lo[j] { x } else { lo[j] };
                let y = x as f64;
                sum[j] += y;
                sumsq[j] += y * y;
            }
        }
        for (j, &x) in chunks.remainder().iter().enumerate() {
            hi[j] = if x > hi[j] { x } else { hi[j] };
            lo[j] = if x < lo[j] { x } else { lo[j] };
            sum[j] += x as f64;
            sumsq[j] += x as f64 * x as f64;
        }
        let vmax = hi.iter().copied().fold(hi[0], |a, b| if b > a { b } else { a });
        let vmin = lo.iter().copied().fold(lo[0], |a, b| if b < a { b } else { a });
        let n = v.len() as f64;
        let avg = sum.iter().sum::<f64>() / n;
        let rms = (sumsq.iter().sum::<f64>() / n).sqrt();
        Some((avg, rms, Values::std_two_pass(v, avg), vmax, vmin))
    }

    // Timing guard (tools/perf-guard.sh): the per-frame statistics of a 125 k-point screen window with every cheap
    // measurement on, against the old one-pass-plus-second-pass computation; the values must agree.
    #[test]
    #[ignore]
    fn perf_guard_framestats() {
        let len = 125_000usize;
        let dt = 1.0 / 1.25e9;
        let p = oos_dsp::sim::PulseTrain::square(3e5, -0.7, 0.9, 2e-8);
        let mut v = oos_dsp::sim::sample(|t| p.v(t), -(len as f64 / 2.0) * dt, dt, len);
        oos_dsp::sim::add_noise(&mut v, 0.04, 3);
        let (old, new) = crate::perfguard::best_pair(
            crate::perfguard::ROUNDS,
            33,
            || {
                std::hint::black_box(values_reference(std::hint::black_box(&v[..])));
            },
            || {
                std::hint::black_box(Values::of(std::hint::black_box(&v[..]), true).map(|x| (x.avg, x.rms, x.std, x.vmax, x.vmin)));
            },
        );
        let (a, b) = (values_reference(&v).unwrap(), Values::of(&v, true).unwrap());
        assert!(a.3 as f64 == b.vmax && a.4 as f64 == b.vmin && a.0 == b.avg && a.1 == b.rms);
        assert!((a.2 - b.std).abs() <= 1e-12 * a.2);
        let r = new.as_secs_f64() / old.as_secs_f64();
        println!("framestats 125 k pt, all cheap measurements: old {:?}, new {:?} (ratio {:.2})", old, new, r);
        assert!(r <= 0.7, "the per-frame statistics must stay faster: ratio {:.2}", r);
    }
    use std::sync::Arc;

    /// What the record path gives for frame `f`: `to_record`, the samples `Ui::meas_slice` takes, `analyze_min`.
    fn reference(f: &Frame, m: &Model, folds: &mut Folds) {
        let window = m.time_per_div() as f64 * HDIV as f64;
        let rec = crate::board::to_record(&Arc::new(f.clone()), m, crate::sparse::POINTS, window, m.delay as f64 - window / 2.0);
        for ((k, c), fold) in folds.iter_mut() {
            let Some(r) = rec.raw.as_ref() else { continue };
            if !(r.i1 <= r.frame.ch[*c].len() && r.i1 - r.i0 > rec.ch[*c].len()) {
                continue;
            }
            let a = oos_dsp::measure::analyze_min(&r.frame.ch[*c][r.i0..r.i1], r.frame.dt, 0.4 * m.ch[*c].volts_per_div() as f64);
            let x = a.get(*k);
            if x.is_finite() {
                fold.push(x);
            }
        }
    }

    // Break it catches: per-frame statistics that differ from measuring each frame's record (other samples, another
    // trigger point, other expressions), frames counted that the record path measures on interpolated points.
    #[test]
    fn per_frame_statistics_equal_measuring_every_record() {
        for (tdiv, len, two) in [(1e-6, 12_512usize, false), (1e-6, 12_512, true), (2e-6, 30_000, false), (1e-7, 1408, false)] {
            let mut m = Model::default();
            m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
            m.trig_level = 0.0;
            for c in 0..NCH {
                (m.ch[c].on, m.ch[c].offset) = (c == 0 || c == 1 && two, 0.0);
            }
            m.meas = CHEAP.iter().flat_map(|&k| [(k, 0), (k, 1)]).collect();
            let r = if two { 2.0 } else { 1.0 };
            let dt = r / 1.25e9;
            let (mut got, mut want) = (folds_for(&m), folds_for(&m));
            let mut counted = 0;
            for k in 0..30 {
                let mut ch: [Vec<f32>; NCH] = Default::default();
                for (c, v) in ch.iter_mut().enumerate().take(if two { 2 } else { 1 }) {
                    let p = oos_dsp::sim::PulseTrain::square(3e5 * (1.0 + c as f64 * 0.4), -0.7, 0.9 + k as f64 * 0.01, 2e-8);
                    *v = oos_dsp::sim::sample(|t| p.v(t), -(len as f64 / 2.0) * dt + k as f64 * 1.1e-10, dt, len);
                    oos_dsp::sim::add_noise(v, 0.04, k as u64 + 3 * c as u64);
                }
                let f = Frame { seq: k as u64, dt, pre: len / 2, ch, ..Default::default() };
                counted += fold_frame(&f, &m, &mut got) as usize;
                reference(&f, &m, &mut want);
            }
            assert_eq!(got.len(), want.len());
            for ((k, a), (k2, b)) in got.iter().zip(&want) {
                assert_eq!((k, a.n), (k2, b.n), "{} s/div {} pt", tdiv, len);
                // the extremes exact; sums of f64 in another order: 1e-12 relative (12-bit data)
                if matches!(k.0, Meas::Vmax | Meas::Vmin | Meas::Vpp) {
                    assert_eq!(a, b, "{:?} {} s/div", k, tdiv);
                }
                let close = |x: f64, y: f64| (x - y).abs() <= 1e-12 * y.abs().max(1e-3);
                assert!(close(a.sum, b.sum) && close(a.sumsq, b.sumsq) && close(a.min, b.min) && close(a.max, b.max), "{:?} {} s/div: {:?} vs {:?}", k, tdiv, a, b);
            }
            let sparse = len == 1408;
            assert!(if sparse { counted == 0 } else { counted == 30 }, "{} s/div: {} counted", tdiv, counted);
            assert_eq!(got.len(), if two { 2 * CHEAP.len() } else { CHEAP.len() });
        }
    }

    // Break it catches: mean, RMS and deviation folded from the min/max pairs of an envelope frame (records above
    // 1 M points read through the FPGA compression): the mean of the pairs of a square with a 20 % duty is not its mean
    #[test]
    fn envelope_frames_count_extremes_only() {
        let mut m = Model { tscale: tscales().iter().position(|&t| (t as f64 - 1e-6).abs() < 1e-9).unwrap(), trig_level: 0.0, ..Model::default() };
        for c in 0..NCH {
            m.ch[c].on = c == 0;
        }
        m.meas = CHEAP.iter().map(|&k| (k, 0)).collect();
        // min/max pairs of a -1/+1 square with 20 % duty: every pair holds both levels around the edges, only one
        // level elsewhere
        let len = 12_512usize;
        let v: Vec<f32> = (0..len).map(|i| if (i / 2) % 50 < 10 { 1.0 } else if i % 2 == 0 { -1.0 } else { -0.98 }).collect();
        let dt = 1.0 / 1.25e9;
        let mut folds = folds_for(&m);
        // a deep record read through the compression (deeper than the live compression, whose pair frames do not
        // count at all: compress.rs)
        let deep = (1.25e9, crate::compress::MAX_DEPTH as usize * 10);
        let env = Frame { seq: 1, dt, pre: len / 2, ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()], envelope: true, acq: deep, ..Default::default() };
        assert!(fold_frame(&env, &m, &mut folds));
        for ((k, _), f) in &folds {
            assert_eq!(f.n, if needs_samples(*k) { 0 } else { 1 }, "{:?}", k);
        }
        // the same values as samples: everything counts
        let mut folds = folds_for(&m);
        let raw = Frame { envelope: false, ..env };
        assert!(fold_frame(&raw, &m, &mut folds));
        assert!(folds.iter().all(|(_, f)| f.n == 1));
        // live compression pairs: none
        let mut folds = folds_for(&m);
        let live = Frame { envelope: true, acq: (1.25e9, 100_000), ..raw };
        assert!(!fold_frame(&live, &m, &mut folds));
    }

}
