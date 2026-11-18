// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The eye diagram: hits accumulated per pixel with linear interpolation between samples, and the voltage and timing
//! measurements read from it (`EyeMeas`). A new eye measurement is a field of `EyeMeas` filled in `Eye::measure`.

use super::{is_above, is_positive, is_probability, norm_inv, q_tail, Clock};

/// Eye diagram: a hit counter per pixel; `ui_span` unit intervals across `w` columns with the eye centred (crossings
/// at span/2 ± 0.5 UI), `v_max` at row 0 and `v_min` at row h-1.
#[derive(Clone, Debug, PartialEq)]
pub struct Eye {
    pub w: usize,
    pub h: usize,
    pub ui_span: u32,
    pub v_min: f32,
    pub v_max: f32,
    pub hits: Vec<u32>,
    /// records accumulated
    pub records: u32,
}

/// All eye measurements (NaN when not measurable).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeMeas {
    /// one / zero levels (mean in the central 20 % of the eye) and their sigmas, V
    pub one: f64,
    pub zero: f64,
    pub sigma_one: f64,
    pub sigma_zero: f64,
    /// one - zero
    pub amplitude: f64,
    /// (one - 3 σ1) - (zero + 3 σ0)
    pub height: f64,
    /// Q-scale eye height at a bit-error rate: (one − Q σ1) − (zero + Q σ0), clamped at 0.
    /// Q is `norm_inv(1 − BER)`. Empty until `height_at` fills it. The 3σ `height` stays as it is.
    pub height_ber: f64,
    /// 1 UI - 6 σ of the crossing time without the picture's own spread, s
    pub width: f64,
    /// crossing level relative to zero..one, %
    pub crossing_pct: f64,
    /// (one - zero) / (σ1 + σ0)
    pub q_factor: f64,
    /// 20-80 % transition times on the eye, s
    pub rise: f64,
    pub fall: f64,
}

impl Eye {
    pub fn new(w: usize, h: usize, ui_span: u32, v_min: f32, v_max: f32) -> Eye {
        let (w, h) = (w.max(1), h.max(1));
        Eye { w, h, ui_span: ui_span.max(1), v_min, v_max, hits: vec![0; w * h], records: 0 }
    }
    pub fn clear(&mut self) {
        self.hits.iter_mut().for_each(|x| *x = 0);
        self.records = 0;
    }
    pub fn at(&self, x: usize, y: usize) -> u32 {
        self.hits[y * self.w + x]
    }
    fn span(&self) -> f64 {
        self.ui_span as f64
    }
    /// voltage at the centre of row y
    fn row_v(&self, y: usize) -> f64 {
        self.v_max as f64 - (y as f64 + 0.5) / self.h as f64 * (self.v_max - self.v_min) as f64
    }
    fn row_of(&self, v: f64) -> Option<usize> {
        let y = ((self.v_max as f64 - v) / (self.v_max - self.v_min) as f64 * self.h as f64).floor();
        (y >= 0.0 && y < self.h as f64).then_some(y as usize)
    }
    /// UI position of the centre of column x
    fn col_u(&self, x: usize) -> f64 {
        (x as f64 + 0.5) / self.w as f64 * self.span()
    }
    /// Hits the point in every UI of the span. Folding by the span instead would keep a clock, which repeats every
    /// 2 UI, in one phase: one level per slot and no crossing.
    fn plot(&mut self, u: f64, v: f64) {
        if let Some(y) = self.row_of(v) {
            let (f, cols) = (u.rem_euclid(1.0), self.w as f64 / self.span());
            for k in 0..self.ui_span {
                let i = y * self.w + (((f + k as f64) * cols) as usize).min(self.w - 1);
                self.hits[i] = self.hits[i].saturating_add(1);
            }
        }
    }

    /// Folds a record into the eye: each sample's position in UI comes from the clock (modulo 1 UI, drawn in every UI
    /// of the span); consecutive samples are joined by a line rasterised in steps of about one pixel, so a sparse
    /// record still draws lines.
    pub fn accumulate(&mut self, v: &[f32], dt: f64, c: &Clock) {
        if !is_positive(c.period) || !is_above(self.v_max, self.v_min) {
            return;
        }
        let off = self.span() / 2.0 - 0.5;
        let (cols_per_ui, rows_per_v) = (self.w as f64 / self.span(), self.h as f64 / (self.v_max - self.v_min) as f64);
        let max_steps = 4 * (self.w + self.h);
        // UI position of every sample: constant clock directly, PLL clock with a cursor over the ticks
        let mut j = 0usize;
        let mut ui = |t: f64| -> f64 {
            let tk = &c.ticks;
            if tk.len() < 2 {
                return (t - c.phase) / c.period;
            }
            while j + 2 < tk.len() && tk[j + 1] <= t {
                j += 1;
            }
            j as f64 + (t - tk[j]) / (tk[j + 1] - tk[j])
        };
        let mut prev: Option<(f64, f64)> = None;
        for (k, &x) in v.iter().enumerate() {
            if !x.is_finite() {
                prev = None;
                continue;
            }
            let u = ui(k as f64 * dt) + off;
            let x = x as f64;
            if let Some((pu, pv)) = prev {
                let steps = (((u - pu) * cols_per_ui).abs().max((x - pv).abs() * rows_per_v).ceil() as usize)
                    .clamp(1, max_steps);
                for s in 0..steps {
                    let a = s as f64 / steps as f64;
                    self.plot(pu + a * (u - pu), pv + a * (x - pv));
                }
            }
            prev = Some((u, x));
        }
        if let Some((u, x)) = prev {
            self.plot(u, x);
        }
        self.records = self.records.saturating_add(1);
    }

    /// Weighted (mean, sigma) of row voltages over the given rows and columns.
    fn stats(&self, rows: impl Iterator<Item = usize> + Clone, cols: &[usize]) -> (f64, f64, f64) {
        let (mut n, mut s, mut s2) = (0.0, 0.0, 0.0);
        for y in rows {
            let v = self.row_v(y);
            for &x in cols {
                let c = self.at(x, y) as f64;
                n += c;
                s += c * v;
                s2 += c * v * v;
            }
        }
        if n == 0.0 {
            return (f64::NAN, f64::NAN, 0.0);
        }
        let m = s / n;
        (m, (s2 / n - m * m).max(0.0).sqrt(), n)
    }

    fn cols_within(&self, centre: f64, half: f64) -> Vec<usize> {
        let span = self.span();
        let mut c: Vec<usize> = (0..self.w)
            .filter(|&x| {
                let d = (self.col_u(x) - centre + span / 2.0).rem_euclid(span) - span / 2.0;
                d.abs() <= half
            })
            .collect();
        if c.is_empty() {
            c.push((((centre.rem_euclid(span)) / span * self.w as f64) as usize).min(self.w - 1));
        }
        c
    }

    /// (one, zero, σ1, σ0): the hits in the central 20 % of the eye, split at their mean voltage.
    /// One rail on a side uses every hit there. Two rails (a transition bit and a de-emphasised bit)
    /// use the rail nearer the decision level: that is the opening. Averaging the two puts the level
    /// in the empty gap and treats the split as noise, which closes the eye at a low BER.
    pub fn levels(&self) -> (f64, f64, f64, f64) {
        let cols = self.cols_within(self.span() / 2.0, 0.1);
        let (mid, _, n) = self.stats(0..self.h, &cols);
        if n == 0.0 {
            return (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        }
        let (one, s1) = self.side_level(&cols, mid, true);
        let (zero, s0) = self.side_level(&cols, mid, false);
        (one, zero, s1, s0)
    }

    /// Mean and sigma on one side of `mid`. A second rail beyond a valley is left out.
    fn side_level(&self, cols: &[usize], mid: f64, upper: bool) -> (f64, f64) {
        let mut rows: Vec<usize> = (0..self.h).filter(|&y| (self.row_v(y) >= mid) == upper).collect();
        rows.sort_by(|&a, &b| (self.row_v(a) - mid).abs().total_cmp(&(self.row_v(b) - mid).abs()));
        let counts: Vec<f64> = rows.iter().map(|&y| cols.iter().map(|&x| self.at(x, y) as f64).sum()).collect();
        let (mean, sigma, n) = self.stats(rows.iter().copied(), cols);
        if n == 0.0 || counts.is_empty() {
            return (mean, sigma);
        }
        let sm: Vec<f64> = (0..counts.len())
            .map(|i| {
                let a = if i > 0 { counts[i - 1] } else { counts[i] };
                let c = if i + 1 < counts.len() { counts[i + 1] } else { counts[i] };
                (a + counts[i] + c) / 3.0
            })
            .collect();
        let sm_max = sm.iter().copied().fold(0.0, f64::max);
        if !is_positive(sm_max) {
            return (mean, sigma);
        }
        let mut peak = None;
        for i in 1..sm.len().saturating_sub(1) {
            if sm[i] >= sm[i - 1] && sm[i] >= sm[i + 1] && sm[i] >= 0.2 * sm_max {
                peak = Some(i);
                break;
            }
        }
        let Some(p) = peak else { return (mean, sigma) };
        let mut valley: Option<usize> = None;
        let mut dropped = false;
        for i in (p + 1)..counts.len() {
            let local = sm[i] >= sm[i - 1] && (i + 1 >= sm.len() || sm[i] >= sm[i + 1]);
            if dropped && local && sm[i] >= 0.15 * sm_max && valley.is_some_and(|v| counts[i] > 3.0 * counts[v]) {
                let end = valley.unwrap_or(i);
                let floor = 0.15 * counts[p];
                let mut lo = p;
                while lo > 0 && counts[lo - 1] >= floor {
                    lo -= 1;
                }
                let (m, s, bn) = self.stats(rows[lo..end].iter().copied(), cols);
                if bn > 0.0 {
                    return (m, s);
                }
                break;
            }
            if counts[i] < 0.4 * counts[p] {
                dropped = true;
                if valley.is_none_or(|v| counts[i] < counts[v]) {
                    valley = Some(i);
                }
            }
        }
        (mean, sigma)
    }
    pub fn eye_amplitude(&self) -> f64 {
        let (one, zero, _, _) = self.levels();
        one - zero
    }
    pub fn eye_height(&self) -> f64 {
        let (one, zero, s1, s0) = self.levels();
        (one - 3.0 * s1) - (zero + 3.0 * s0)
    }
    pub fn q_factor(&self) -> f64 {
        let (one, zero, s1, s0) = self.levels();
        if s1 + s0 > 0.0 {
            (one - zero) / (s1 + s0)
        } else {
            f64::INFINITY
        }
    }
    /// Crossing level (V): mean of the hits between the levels at the left crossing (± 2 % UI).
    pub fn crossing_level(&self) -> f64 {
        let (one, zero, _, _) = self.levels();
        let a = one - zero;
        let cols = self.cols_within(self.span() / 2.0 - 0.5, 0.02);
        self.stats((0..self.h).filter(|&y| self.row_v(y) > zero + 0.1 * a && self.row_v(y) < one - 0.1 * a), &cols).0
    }
    pub fn crossing_pct(&self) -> f64 {
        let (one, zero, _, _) = self.levels();
        (self.crossing_level() - zero) / (one - zero) * 100.0
    }
    /// Offsets (UI) from the crossing of the hits in row y within ±0.5 UI: (offset, count).
    fn row_offsets(&self, y: usize) -> Vec<(f64, f64)> {
        let c = self.span() / 2.0 - 0.5;
        (0..self.w)
            .filter(|&x| self.at(x, y) > 0)
            .map(|x| (((self.col_u(x) - c + 0.5).rem_euclid(1.0)) - 0.5, self.at(x, y) as f64))
            .collect()
    }
    /// (mean, sigma) of the crossing time (UI, relative to the nominal crossing) in the rows at the crossing level.
    fn crossing_time(&self) -> (f64, f64) {
        let Some(y) = self.row_of(self.crossing_level()) else { return (f64::NAN, f64::NAN) };
        let mut o = Vec::new();
        for yy in y.saturating_sub(1)..=(y + 1).min(self.h - 1) {
            o.extend(self.row_offsets(yy));
        }
        let n: f64 = o.iter().map(|x| x.1).sum();
        if n == 0.0 {
            return (f64::NAN, f64::NAN);
        }
        let m = o.iter().map(|x| x.0 * x.1).sum::<f64>() / n;
        (m, (o.iter().map(|x| (x.0 - m).powi(2) * x.1).sum::<f64>() / n).sqrt())
    }
    /// Eye width (s): 1 UI - 6 σ of the crossing time, with the picture's own spread taken out (`unbinned_sigma`).
    pub fn eye_width(&self, period: f64) -> f64 {
        let cols = self.w as f64 / self.span();
        (1.0 - 6.0 * unbinned_sigma(self.crossing_time().1 * cols) / cols) * period
    }
    /// 20-80 % rise and fall times on the eye (s): at the 20 % and 80 % rows the hits left of the crossing belong to
    /// the rising (at 20 %) / falling (at 80 %) edges, those right of it to the other one.
    pub fn rise_fall(&self, period: f64) -> (f64, f64) {
        let (one, zero, _, _) = self.levels();
        let a = one - zero;
        let (mc, _) = self.crossing_time();
        let side = |level: f64, left: bool| -> f64 {
            let Some(y) = self.row_of(zero + level * a) else { return f64::NAN };
            let o: Vec<(f64, f64)> = self.row_offsets(y).into_iter().filter(|x| (x.0 < mc) == left).collect();
            let n: f64 = o.iter().map(|x| x.1).sum();
            if n == 0.0 {
                f64::NAN
            } else {
                o.iter().map(|x| x.0 * x.1).sum::<f64>() / n
            }
        };
        ((side(0.8, false) - side(0.2, true)) * period, (side(0.2, false) - side(0.8, true)) * period)
    }
    /// All measurements at once (`period` = unit interval, s).
    pub fn measure(&self, period: f64) -> EyeMeas {
        let (one, zero, s1, s0) = self.levels();
        let (rise, fall) = self.rise_fall(period);
        EyeMeas {
            one,
            zero,
            sigma_one: s1,
            sigma_zero: s0,
            amplitude: one - zero,
            height: self.eye_height(),
            height_ber: f64::NAN,
            width: self.eye_width(period),
            crossing_pct: self.crossing_pct(),
            q_factor: self.q_factor(),
            rise,
            fall,
        }
    }
}

/// Variance (columns²) of the column centres that a Gaussian crossing of σ = `s` columns lights when it is centred on
/// a column border.
fn binned_variance(s: f64) -> f64 {
    if !is_positive(s) {
        return 0.25;
    }
    let k = (8.0 * s).ceil() as i64 + 1;
    let (mut m1, mut m2) = (0.0, 0.0);
    for m in -k..k {
        let p = q_tail(m as f64 / s) - q_tail((m + 1) as f64 / s);
        let x = m as f64 + 0.5;
        m1 += p * x;
        m2 += p * x * x;
    }
    m2 - m1 * m1
}

/// The crossing's own σ (columns) from the σ the picture reads (columns). A line through the rows at the crossing
/// lights its column and a neighbour, and an edge takes time to pass those rows: a crossing without jitter reads half
/// a column (measured at 48 to 960 columns; 6 % of the UI in the 96 columns of a 2-UI eye), and a wide one reads a
/// variance a twelfth of a column² above its own. A Gaussian centred on a column border reads both, so the reading is
/// matched to that model.
fn unbinned_sigma(read: f64) -> f64 {
    let v = read * read;
    if !read.is_finite() {
        return f64::NAN;
    }
    if !is_above(v, binned_variance(0.0)) {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, read);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if binned_variance(mid) < v {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

impl EyeMeas {
    /// Vertical opening extrapolated to `ber`: replace the 3 of the 3σ height with `norm_inv(1 − ber)`, and clamp at 0.
    pub fn height_at(&self, ber: f64) -> f64 {
        if !is_probability(ber) {
            return f64::NAN;
        }
        let q = norm_inv(1.0 - ber);
        let h = (self.one - q * self.sigma_one) - (self.zero + q * self.sigma_zero);
        if h.is_finite() {
            h.max(0.0)
        } else {
            f64::NAN
        }
    }
}
