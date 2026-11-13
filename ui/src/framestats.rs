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

use crate::board::Frame;
use crate::model::*;

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
        if v.len() < 4 || !v.iter().all(|x| x.is_finite()) {
            return None;
        }
        // one pass, the sums split over `L` lanes (vectorised): the same values as `analyze_min` to about 1e-15
        // relative, the extremes exact
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
        let std = if std {
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
            (d.iter().sum::<f64>() / n).sqrt()
        } else {
            f64::NAN
        };
        Some(Values { vmax: vmax as f64, vmin: vmin as f64, avg, rms, std })
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
}
