// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Fast display path for the hardware source: a frame's raw samples go straight into the phosphor map, one vertical
//! span per screen column (min..max of the column's samples, joined to the previous column's last sample), instead
//! of the screen record of `board::to_record` (software trigger over a tenth of a screen, then 3600 interpolated or
//! peak-detected points) for every frame. Used for dense views (at least one sample per column) with a plain edge
//! trigger; the hardware already put the edge at `pre` to within a sample or two, so the crossing is only refined
//! (sub-sample, by linear interpolation) near it.

/// The level crossing of the edge nearest to `pre` within +-`span` samples, as a fractional sample index (rising:
/// below -> at or above `level`; falling the other way). `either`: both directions. `hyst` > 0: like the hardware
/// comparators, only the first crossing after the signal was `hyst` on the other side of the level counts, so noise on
/// an edge of the other direction is no edge.
pub fn edge_near(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let lo = pre.saturating_sub(span).max(1);
    let hi = (pre + span).min(v.len() - 1);
    // back from the crossing at k: armed when the signal went past the band before it came back over the level
    let look = span.max(4096);
    let armed = |k: usize, up: bool| {
        if hyst <= 0.0 {
            return true;
        }
        for j in (k.saturating_sub(look)..k).rev() {
            let x = v[j];
            if up && x < level - hyst || !up && x > level + hyst {
                return true;
            }
            if up && x >= level || !up && x <= level {
                return false;
            }
        }
        false
    };
    let mut best: Option<f64> = None;
    for k in lo..=hi {
        let (a, b) = (v[k - 1], v[k]);
        let up = a < level && b >= level && (rising || either) && armed(k, true);
        let down = a > level && b <= level && (!rising || either) && armed(k, false);
        if up || down {
            let t = (k - 1) as f64 + ((level - a) / (b - a)) as f64;
            if best.is_none_or(|x| (t - pre as f64).abs() < (x - pre as f64).abs()) {
                best = Some(t);
            }
        }
    }
    best
}

/// Where the edge of the set direction is, seen at the resolution of the screen: `v` around `pre` (+-`span` samples)
/// smoothed by a centred moving average of `l` samples (one screen column), then the hardware's comparators with
/// hysteresis `hyst` (`oos_dsp::edges::trigger_edges`: a rising edge arms below level - `hyst` and fires at the level),
/// the edge of the set direction nearest to `pre`. Noise larger than the band
/// (a slow sine at a high rate) cannot make an edge of the other direction look right, which a test on single samples
/// cannot rule out. Only edges at times `fits` accepts count. Returns the edge and the nearest edge of either direction
/// (for the statistics).
#[allow(clippy::too_many_arguments)]
pub fn edge_smooth(v: &[f32], pre: usize, level: f32, hyst: f32, rising: bool, either: bool, span: usize, l: usize, fits: impl Fn(f64) -> bool) -> (Option<f64>, Option<oos_dsp::edges::Edge>) {
    let l = l.max(1);
    let a = pre.saturating_sub(span + l);
    let b = (pre + span + l).min(v.len());
    if b <= a + 2 * l {
        return (None, None);
    }
    // running sum over the window; NaN samples count as the level (none in hardware frames)
    let x = |i: usize| if v[i].is_finite() { v[i] as f64 } else { level as f64 };
    let mut s = Vec::with_capacity(b - a);
    let mut sum = 0.0;
    let half = l / 2;
    for i in a..(a + l).min(b) {
        sum += x(i);
    }
    // s[j] = mean of v[a + j - half .. a + j - half + l], defined from j = half
    for j in 0..(b - a) {
        let lo = a + j;
        if j >= half && lo + l - half <= b {
            s.push((sum / l as f64) as f32);
            if lo + l - half < b {
                sum += x(lo + l - half) - x(lo - half);
            }
        } else {
            s.push(f32::NAN);
        }
    }
    let edges = oos_dsp::edges::trigger_edges(&s, level, hyst);
    let d = |e: &oos_dsp::edges::Edge| (a as f64 + e.t - pre as f64).abs();
    let near = |ok: &dyn Fn(&oos_dsp::edges::Edge) -> bool| edges.iter().filter(|e| ok(e) && fits(a as f64 + e.t)).min_by(|p, q| d(p).total_cmp(&d(q))).copied();
    let want = near(&|e| either || e.rising == rising).map(|e| a as f64 + e.t);
    (want, near(&|_| true).map(|e| oos_dsp::edges::Edge { t: a as f64 + e.t, ..e }))
}

/// Screen mapping of one channel: x = (k - ti) * x_per_sample + x0 (columns), y = val * y_per_volt + y0 (rows).
#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub ti: f64,
    pub x_per_sample: f64,
    pub x0: f64,
    pub y_per_volt: f32,
    pub y0: f32,
}

/// Adds `w` to every pixel of `map` (w x h, row-major) the trace covers: per column the span of its samples, joined
/// to the last sample of the column before (so steep edges stay connected). Needs at least one sample per column
/// (`x_per_sample <= 1`).
///
/// The spans are worked out first (`spans`) and then added in one of two ways with the same result: column by column
/// (one scattered read-modify-write per pixel, a row apart in memory) when the spans are short, or row by row over the
/// rows the frame touches (a vectorised pass that adds `wgt` where the row lies in a column's span) when they are long:
/// a noisy or fast signal at 100 samples per column covers tens of rows in every column, and the scattered writes
/// were three quarters of the time.
pub fn deposit(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16) {
    deposit_from(map, w, h, v, m, wgt);
}

/// `deposit` straight from a frame's read-out words (`Codes`): the same picture, without the f32 copy of the channel.
pub fn deposit_codes(map: &mut [u16], w: usize, h: usize, v: &Codes, m: &Map, wgt: u16) {
    deposit_from(map, w, h, v, m, wgt);
}

/// The samples of one channel as `deposit` needs them: their number, the extremes of a range and one sample.
pub trait Samples {
    fn len(&self) -> usize;
    /// (min, max) of samples `a..b` (`min_max`: NaN skipped; (inf, -inf) when none)
    fn extremes(&self, a: usize, b: usize) -> (f32, f32);
    fn at(&self, i: usize) -> f32;
}

impl Samples for [f32] {
    fn len(&self) -> usize {
        <[f32]>::len(self)
    }
    #[inline]
    fn extremes(&self, a: usize, b: usize) -> (f32, f32) {
        min_max(&self[a..b])
    }
    #[inline]
    fn at(&self, i: usize) -> f32 {
        self[i]
    }
}

/// One channel of a read-out (16-bit little-endian words, `stride` words per group, this channel's at `word`) with the
/// board's word-to-volt mapping. The mapping is monotone (a multiply by `scale` and a subtraction, each rounded
/// monotonically), so the extremes of the volts are the volts of the extreme words: exact, without converting every word.
pub struct Codes<'a> {
    pub bytes: &'a [u8],
    /// the same bytes as words when they are 2-byte aligned (the read-out buffers are): the vector loops
    pub words: Option<&'a [u16]>,
    pub stride: usize,
    pub word: usize,
    pub n: usize,
    pub affine: (f32, f32),
    pub volt: fn(u16, (f32, f32)) -> f32,
}

impl Codes<'_> {
    /// Word `i` of the channel.
    #[inline]
    pub fn code(&self, i: usize) -> u16 {
        let j = 2 * (i * self.stride + self.word);
        u16::from_le_bytes([self.bytes[j], self.bytes[j + 1]])
    }
}

impl Samples for Codes<'_> {
    fn len(&self) -> usize {
        self.n
    }
    #[inline]
    fn extremes(&self, a: usize, b: usize) -> (f32, f32) {
        if a >= b {
            return (f32::INFINITY, f32::NEG_INFINITY);
        }
        let (lo, hi) = match (self.words, self.stride) {
            (Some(w), s) => word_extremes(w, s, self.word, a, b),
            (None, 1) => code_extremes::<2>(&self.bytes[2 * a..2 * b], 0),
            (None, 2) => code_extremes::<4>(&self.bytes[4 * a..(4 * b).min(self.bytes.len())], self.word),
            (None, _) => code_extremes::<8>(&self.bytes[8 * a..(8 * b).min(self.bytes.len())], self.word),
        };
        let (x, y) = ((self.volt)(lo, self.affine), (self.volt)(hi, self.affine));
        // a negative scale turns the order around
        if x <= y { (x, y) } else { (y, x) }
    }
    #[inline]
    fn at(&self, i: usize) -> f32 {
        (self.volt)(self.code(i), self.affine)
    }
}

/// Smallest and largest of samples `a..b` (not empty) of the channel at `word` of every group of `stride` words.
/// On aarch64 NEON: whole groups of 8 samples through the de-interleaving loads (ld1/ld2/ld4), the rest one by one.
#[inline]
fn word_extremes(w: &[u16], stride: usize, word: usize, a: usize, b: usize) -> (u16, u16) {
    let (mut lo, mut hi) = (u16::MAX, u16::MIN);
    #[allow(unused_mut)]
    let mut i = a;
    #[cfg(target_arch = "aarch64")]
    {
        use core::arch::aarch64::*;
        // SAFETY: NEON is aarch64 baseline; every load reads 8 groups that lie inside `w` (checked by `end`)
        unsafe {
            let (mut vl, mut vh) = (vdupq_n_u16(u16::MAX), vdupq_n_u16(0));
            let end = |i: usize| (i + 8) * stride <= w.len() && i + 8 <= b;
            match stride {
                1 => {
                    while end(i) {
                        let x = vld1q_u16(w.as_ptr().add(i));
                        (vl, vh) = (vminq_u16(vl, x), vmaxq_u16(vh, x));
                        i += 8;
                    }
                }
                2 => {
                    while end(i) {
                        let g = vld2q_u16(w.as_ptr().add(2 * i));
                        let x = if word == 0 { g.0 } else { g.1 };
                        (vl, vh) = (vminq_u16(vl, x), vmaxq_u16(vh, x));
                        i += 8;
                    }
                }
                4 => {
                    while end(i) {
                        let g = vld4q_u16(w.as_ptr().add(4 * i));
                        let x = match word {
                            0 => g.0,
                            1 => g.1,
                            2 => g.2,
                            _ => g.3,
                        };
                        (vl, vh) = (vminq_u16(vl, x), vmaxq_u16(vh, x));
                        i += 8;
                    }
                }
                _ => {}
            }
            (lo, hi) = (lo.min(vminvq_u16(vl)), hi.max(vmaxvq_u16(vh)));
        }
    }
    while i < b {
        let x = w[i * stride + word];
        lo = lo.min(x);
        hi = hi.max(x);
        i += 1;
    }
    (lo, hi)
}

/// Smallest and largest word `k` of every group of `G` bytes in `b` (the groups of one range of samples; the last
/// group may be short when its word is still inside).
#[inline]
fn code_extremes<const G: usize>(b: &[u8], k: usize) -> (u16, u16) {
    let (mut lo, mut hi) = (u16::MAX, u16::MIN);
    let mut it = b.chunks_exact(G);
    for g in &mut it {
        let x = u16::from_le_bytes([g[2 * k], g[2 * k + 1]]);
        lo = lo.min(x);
        hi = hi.max(x);
    }
    let r = it.remainder();
    if r.len() >= 2 * k + 2 {
        let x = u16::from_le_bytes([r[2 * k], r[2 * k + 1]]);
        lo = lo.min(x);
        hi = hi.max(x);
    }
    (lo, hi)
}

fn deposit_from<S: Samples + ?Sized>(map: &mut [u16], w: usize, h: usize, v: &S, m: &Map, wgt: u16) {
    SPANS.with(|sp| {
        let sp = &mut *sp.borrow_mut();
        let Some((c0, c1, rmin, rmax, pixels)) = spans(v, m, w, h, sp) else { return };
        let (lo, hi) = (&sp.lo[..], &sp.hi[..]);
        if pixels > sweep_cost(rmax - rmin + 1, c1 - c0) {
            for r in rmin..=rmax {
                let row = &mut map[r * w + c0..r * w + c1];
                let r = r as i16;
                for ((px, &a), &b) in row.iter_mut().zip(&lo[c0..c1]).zip(&hi[c0..c1]) {
                    // wgt where a <= r <= b, else 0: no branch per pixel
                    let inside = ((a <= r) & (r <= b)) as u16;
                    *px = px.saturating_add(wgt & 0u16.wrapping_sub(inside));
                }
            }
        } else {
            for c in c0..c1 {
                let (a, b) = (lo[c], hi[c]);
                let mut i = a.max(0) as usize * w + c;
                for _ in a..=b {
                    map[i] = map[i].saturating_add(wgt);
                    i += w;
                }
            }
        }
    });
}

/// Per column the rows of `deposit` (`lo`..=`hi`, empty when lo > hi), kept per thread so a frame allocates nothing.
#[derive(Default)]
pub struct Spans {
    lo: Vec<i16>,
    hi: Vec<i16>,
}

thread_local! {
    static SPANS: std::cell::RefCell<Spans> = std::cell::RefCell::new(Spans::default());
}

/// Scattered pixel writes the row sweep of `rows` x `cols` pixels is worth: about one scattered read-modify-write per
/// eight pixels of the sweep (eight u16 lanes per vector step, a compare and an add each). Only the speed depends on it.
fn sweep_cost(rows: usize, cols: usize) -> usize {
    rows * cols / 8
}

/// The spans of `deposit` into `sp`: columns `c0..c1`, the rows used (`rmin..=rmax`) and the pixel count; None when
/// nothing is drawn.
fn spans<S: Samples + ?Sized>(v: &S, m: &Map, w: usize, h: usize, sp: &mut Spans) -> Option<(usize, usize, usize, usize, usize)> {
    if v.len() == 0 || m.x_per_sample <= 0.0 || w == 0 || h == 0 {
        return None;
    }
    // first sample of column c: ceil(k_of(c)); per column one min/max over a slice (no per-sample screen math)
    let k_of = |x: f64| m.ti + (x - m.x0) / m.x_per_sample;
    let first = |c: i64| (k_of(c as f64).ceil().max(0.0) as usize).min(v.len());
    let c0 = (((-m.ti) * m.x_per_sample + m.x0).floor() as i64).max(0);
    let c1 = ((((v.len() as f64 - m.ti) * m.x_per_sample + m.x0).ceil()) as i64).min(w as i64);
    if c1 <= c0 {
        return None;
    }
    if sp.lo.len() < w {
        sp.lo.resize(w, i16::MAX);
        sp.hi.resize(w, i16::MIN);
    }
    let hh = h as i32;
    let (mut rmin, mut rmax, mut pixels) = (usize::MAX, 0usize, 0usize);
    let mut ka = first(c0);
    for c in c0..c1 {
        let (lo, hi) = (&mut sp.lo[c as usize], &mut sp.hi[c as usize]);
        (*lo, *hi) = (i16::MAX, i16::MIN);
        let kb = first(c + 1);
        if kb <= ka {
            continue;
        }
        let (mut mn, mut mx) = v.extremes(ka, kb);
        // the join to the previous column's last sample
        if ka > 0 {
            let j = v.at(ka - 1);
            if j.is_finite() {
                mn = mn.min(j);
                mx = mx.max(j);
            }
        }
        ka = kb;
        if !(mn <= mx) {
            continue;
        }
        let (ya, yb) = (mn * m.y_per_volt + m.y0, mx * m.y_per_volt + m.y0);
        let r0 = (ya.min(yb).round() as i32).max(0);
        let r1 = (ya.max(yb).round() as i32).min(hh - 1);
        if r0 > r1 {
            continue;
        }
        (*lo, *hi) = (r0 as i16, r1 as i16);
        rmin = rmin.min(r0 as usize);
        rmax = rmax.max(r1 as usize);
        pixels += (r1 - r0 + 1) as usize;
    }
    (pixels > 0).then_some((c0 as usize, c1 as usize, rmin, rmax, pixels))
}

/// `deposit` as it was before the span pass (one scattered write per pixel, one compare chain per sample): the
/// reference of the equality test and of the timing guard.
#[cfg(test)]
pub fn deposit_reference(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16) {
    if v.is_empty() || m.x_per_sample <= 0.0 {
        return;
    }
    let k_of = |x: f64| m.ti + (x - m.x0) / m.x_per_sample;
    let first = |c: i64| (k_of(c as f64).ceil().max(0.0) as usize).min(v.len());
    let c0 = (((-m.ti) * m.x_per_sample + m.x0).floor() as i64).max(0);
    let c1 = ((((v.len() as f64 - m.ti) * m.x_per_sample + m.x0).ceil()) as i64).min(w as i64);
    let hh = h as i32;
    let mut ka = first(c0);
    for c in c0..c1 {
        let kb = first(c + 1);
        if kb <= ka {
            continue;
        }
        let (mut mn, mut mx) = min_max_scalar(&v[ka..kb]);
        if ka > 0 && v[ka - 1].is_finite() {
            mn = mn.min(v[ka - 1]);
            mx = mx.max(v[ka - 1]);
        }
        ka = kb;
        if !(mn <= mx) {
            continue;
        }
        let (ya, yb) = (mn * m.y_per_volt + m.y0, mx * m.y_per_volt + m.y0);
        let r0 = (ya.min(yb).round() as i32).max(0);
        let r1 = (ya.max(yb).round() as i32).min(hh - 1);
        let mut i = r0.max(0) as usize * w + c as usize;
        for _ in r0..=r1 {
            map[i] = map[i].saturating_add(wgt);
            i += w;
        }
    }
}

/// Minimum and maximum of `s`, NaN skipped (`f32::min`/`max`: fminnm/fmaxnm on aarch64); (inf, -inf) when empty or
/// all NaN. On aarch64 four NEON accumulators of four lanes each (16 samples per step) instead of one compare chain
/// per sample, which bound the loop by the compare latency. The same values give the same extremes in any order, so
/// the result equals the one-chain loop (a zero may come back as -0.0 where the chain gave +0.0 or the other way;
/// both map to the same pixel row).
#[inline]
pub fn min_max(s: &[f32]) -> (f32, f32) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is part of the aarch64 baseline; loads stay inside `s` (chunks of 16)
        unsafe { min_max_neon(s) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        min_max_scalar(s)
    }
}

/// `min_max` as one chain: the reference, and the code on other targets.
#[cfg_attr(all(target_arch = "aarch64", not(test)), allow(dead_code))]
pub fn min_max_scalar(s: &[f32]) -> (f32, f32) {
    let (mut a, mut b) = (f32::INFINITY, f32::NEG_INFINITY);
    for &x in s {
        a = a.min(x);
        b = b.max(x);
    }
    (a, b)
}

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn min_max_neon(s: &[f32]) -> (f32, f32) {
    use core::arch::aarch64::*;
    let mut it = s.chunks_exact(16);
    let (mut n0, mut n1, mut n2, mut n3) = (vdupq_n_f32(f32::INFINITY), vdupq_n_f32(f32::INFINITY), vdupq_n_f32(f32::INFINITY), vdupq_n_f32(f32::INFINITY));
    let (mut x0, mut x1, mut x2, mut x3) = (vdupq_n_f32(f32::NEG_INFINITY), vdupq_n_f32(f32::NEG_INFINITY), vdupq_n_f32(f32::NEG_INFINITY), vdupq_n_f32(f32::NEG_INFINITY));
    for c in &mut it {
        let p = c.as_ptr();
        let (a, b, d, e) = (vld1q_f32(p), vld1q_f32(p.add(4)), vld1q_f32(p.add(8)), vld1q_f32(p.add(12)));
        n0 = vminnmq_f32(n0, a);
        n1 = vminnmq_f32(n1, b);
        n2 = vminnmq_f32(n2, d);
        n3 = vminnmq_f32(n3, e);
        x0 = vmaxnmq_f32(x0, a);
        x1 = vmaxnmq_f32(x1, b);
        x2 = vmaxnmq_f32(x2, d);
        x3 = vmaxnmq_f32(x3, e);
    }
    let mut a = vminnmvq_f32(vminnmq_f32(vminnmq_f32(n0, n1), vminnmq_f32(n2, n3)));
    let mut b = vmaxnmvq_f32(vmaxnmq_f32(vmaxnmq_f32(x0, x1), vmaxnmq_f32(x2, x3)));
    for &x in it.remainder() {
        a = a.min(x);
        b = b.max(x);
    }
    (a, b)
}

/// Dots display (`Model::dots`): every sample as one pixel, no spans and no joins; the brightness of a column follows
/// where its samples dwell. Same mapping and column walk as `deposit`.
///
/// Where samples repeat the row before (flat or slow parts), one add per sample makes a chain of read-modify-writes on
/// one pixel, each waiting for the last; there each run of samples on one row adds `run x wgt` at once (saturating as
/// the single adds did: min(pixel + run x wgt, 65535)). On fast or noisy parts the run test would mispredict, so one
/// add per sample stays. Which of the two a column takes follows how many samples of the column before repeated their
/// row (signals change slowly from column to column); both give the same picture.
pub fn deposit_dots(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16) {
    let mut runs = false;
    dots_walk(w, v.len(), m, |c, a, b| dots_column(map, w, h, c, &v[a..b], m, wgt, &mut runs));
}

/// The column walk of `deposit_dots` over `n` samples: `col(c, a, b)` for each column c with samples a..b.
fn dots_walk(w: usize, n: usize, m: &Map, mut col: impl FnMut(usize, usize, usize)) {
    if n == 0 || m.x_per_sample <= 0.0 {
        return;
    }
    let k_of = |x: f64| m.ti + (x - m.x0) / m.x_per_sample;
    let first = |c: i64| (k_of(c as f64).ceil().max(0.0) as usize).min(n);
    let c0 = (((-m.ti) * m.x_per_sample + m.x0).floor() as i64).max(0);
    let c1 = ((((n as f64 - m.ti) * m.x_per_sample + m.x0).ceil()) as i64).min(w as i64);
    let mut ka = first(c0);
    for c in c0..c1 {
        let kb = first(c + 1).max(ka);
        if kb > ka {
            col(c as usize, ka, kb);
        }
        ka = kb;
    }
}

/// The dots of column `c` (samples `s`); `runs`: add in runs (in: this column, out: the next).
#[allow(clippy::too_many_arguments)]
#[inline]
fn dots_column(map: &mut [u16], w: usize, h: usize, c: usize, s: &[f32], m: &Map, wgt: u16, runs: &mut bool) {
    let hf = h as f32;
    // the row of a sample, None off the map (NaN fails both comparisons)
    let row = |x: f32| {
        let r = (x * m.y_per_volt + m.y0).round();
        (r >= 0.0 && r < hf).then_some(r as usize)
    };
    let mut repeats = 0usize;
    if *runs {
        let mut run: (Option<usize>, u32) = (None, 0);
        for &x in s {
            let r = row(x);
            if r == run.0 {
                run.1 += 1;
                repeats += 1;
                continue;
            }
            if let Some(r0) = run.0 {
                let i = r0 * w + c;
                map[i] = (map[i] as u32 + run.1 * wgt as u32).min(u16::MAX as u32) as u16;
            }
            run = (r, 1);
        }
        if let Some(r0) = run.0 {
            let i = r0 * w + c;
            map[i] = (map[i] as u32 + run.1 * wgt as u32).min(u16::MAX as u32) as u16;
        }
        *runs = 2 * repeats > s.len();
    } else {
        // the repeats of the first `PROBE` samples decide the next column (counting all cost more than it saved)
        let (head, tail) = s.split_at(s.len().min(PROBE));
        let mut prev = None;
        for &x in head {
            let r = row(x);
            repeats += (r == prev) as usize;
            prev = r;
            if let Some(r) = r {
                let i = r * w + c;
                map[i] = map[i].saturating_add(wgt);
            }
        }
        for &x in tail {
            if let Some(r) = row(x) {
                let i = r * w + c;
                map[i] = map[i].saturating_add(wgt);
            }
        }
        *runs = 2 * repeats > head.len();
    }
}

/// Samples of a column whose row repeats decide whether the next column adds in runs (`dots_column`).
const PROBE: usize = 32;

/// `deposit_dots` as it was (one scattered write per sample): the reference of the equality test and timing guard.
#[cfg(test)]
pub fn deposit_dots_reference(map: &mut [u16], w: usize, h: usize, v: &[f32], m: &Map, wgt: u16) {
    if v.is_empty() || m.x_per_sample <= 0.0 {
        return;
    }
    let k_of = |x: f64| m.ti + (x - m.x0) / m.x_per_sample;
    let first = |c: i64| (k_of(c as f64).ceil().max(0.0) as usize).min(v.len());
    let c0 = (((-m.ti) * m.x_per_sample + m.x0).floor() as i64).max(0);
    let c1 = ((((v.len() as f64 - m.ti) * m.x_per_sample + m.x0).ceil()) as i64).min(w as i64);
    let mut ka = first(c0);
    for c in c0..c1 {
        let kb = first(c + 1);
        for &x in &v[ka..kb.max(ka)] {
            let r = (x * m.y_per_volt + m.y0).round();
            if r >= 0.0 && r < h as f32 {
                let i = r as usize * w + c as usize;
                map[i] = map[i].saturating_add(wgt);
            }
        }
        ka = kb.max(ka);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signals for the deposit equality: sine, noisy square, noise, a ramp, clipped beyond the map, NaN gaps, flat.
    fn signals(n: usize) -> Vec<Vec<f32>> {
        let mut x = 12345u32;
        let mut rnd = move || {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((x >> 8) & 0xffff) as f32 / 65536.0 - 0.5
        };
        let mut out = vec![
            (0..n).map(|i| 0.9 * (i as f32 / 400.0 * std::f32::consts::TAU).sin()).collect::<Vec<f32>>(),
            (0..n).map(|i| 0.7 * (i as f32 / 37.0).sin() + 0.3 * (i as f32 / 5.3).sin()).collect(),
            (0..n).map(|i| if (i / 900) % 2 == 0 { 1.5 } else { -1.5 } + 0.0).collect(),
            (0..n).map(|i| i as f32 / n as f32 * 10.0 - 5.0).collect(),
            (0..n).map(|i| 6.0 * (i as f32 / 300.0).sin()).collect(),
            vec![0.25; n],
        ];
        out.push((0..n).map(|_| rnd()).collect());
        out.push((0..n).map(|i| if (i / 700) % 2 == 0 { 1.0 } else { -1.0 } + 0.05 * rnd()).collect());
        let mut gaps: Vec<f32> = (0..n).map(|i| (i as f32 / 90.0).sin()).collect();
        for i in (0..n).step_by(97) {
            gaps[i..(i + 13).min(n)].fill(f32::NAN);
        }
        out.push(gaps);
        out
    }

    // Break it catches: a span pass that draws other pixels than one scattered write per pixel did (rows, columns,
    // joins, clipping, NaN gaps, saturation), whichever of the two ways it adds them.
    #[test]
    fn deposit_is_bit_identical_to_the_scattered_writes() {
        let (w, h) = (970usize, 448usize);
        let mut cases = 0;
        let mut sweeps = 0;
        for n in [1_408usize, 12_512, 100_000] {
            for v in signals(n) {
                for (xps, ti, x0) in [(1.0 / 100.0, n as f64 / 2.0, 485.0), (0.00776, n as f64 / 2.0 + 0.37, 485.0), (0.7, 3.3, 10.0), (970.0 / n as f64, 0.0, 0.0), (0.0123, n as f64 * 0.9, 120.5), (0.25, -40.0, 0.0)] {
                    for (ypv, y0) in [(-56.0f32, 224.0f32), (-560.0, 224.0), (37.0, 10.0), (-5.0, 447.6)] {
                        for (wgt, pre) in [(24u16, 0u16), (1, 0), (60_000, 30_000)] {
                            let m = Map { ti, x_per_sample: xps, x0, y_per_volt: ypv, y0 };
                            let (mut a, mut b) = (vec![pre; w * h], vec![pre; w * h]);
                            deposit_reference(&mut a, w, h, &v, &m, wgt);
                            deposit(&mut b, w, h, &v, &m, wgt);
                            assert!(a == b, "n {} xps {} ti {} y {} {} wgt {}", n, xps, ti, ypv, y0, wgt);
                            let mut sp = Spans::default();
                            if let Some((c0, c1, r0, r1, px)) = spans(&v[..], &m, w, h, &mut sp) {
                                sweeps += (px > sweep_cost(r1 - r0 + 1, c1 - c0)) as usize;
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
        // both ways of adding were compared
        assert!(sweeps > 20 && sweeps < cases - 20, "{} of {} cases swept", sweeps, cases);
    }

    // Timing guard (tools/perf-guard.sh): the span pass against one scattered write per pixel, per signal, at 100 k and
    // 12.5 k points over 970 columns. Never slower on short spans (square), faster on long ones (sine, noise).
    #[test]
    #[ignore]
    fn perf_guard_deposit_spans() {
        let (w, h) = (970usize, 448usize);
        let names = ["sine", "two sines", "square", "ramp", "clipped", "flat", "noise", "noisy square", "gaps"];
        let mut worst: f64 = 0.0;
        for n in [100_000usize, 12_512] {
            for (name, v) in names.iter().zip(signals(n)) {
                let m = Map { ti: n as f64 / 2.0, x_per_sample: 970.0 / n as f64 * 0.8, x0: 485.0, y_per_volt: -56.0, y0: 224.0 };
                let (mut ma, mut mb) = (vec![0u16; w * h], vec![0u16; w * h]);
                let (old, new) = crate::perfguard::best_pair(crate::perfguard::ROUNDS, 33, || deposit_reference(&mut ma, w, h, &v, &m, 1), || deposit(&mut mb, w, h, &v, &m, 1));
                let r = new.as_secs_f64() / old.as_secs_f64();
                worst = worst.max(r);
                println!("deposit {} pt {}: one write per pixel {:?}, span pass {:?} (ratio {:.2})", n, name, old, new, r);
            }
        }
        assert!(worst <= 1.25, "the span pass must not be slower: worst ratio {:.2}", worst);
    }

    // Break it catches: dots counted in runs drawing other pixels or other weights than one add per sample (runs
    // across columns, off-map samples, NaN gaps, saturation, a run cut by an off-map sample)
    #[test]
    fn dots_are_bit_identical_to_one_add_per_sample() {
        let (w, h) = (970usize, 448usize);
        for n in [1_408usize, 12_512, 100_000] {
            for v in signals(n) {
                for (xps, ti, x0) in [(1.0 / 100.0, n as f64 / 2.0, 485.0), (0.00776, n as f64 / 2.0 + 0.37, 485.0), (0.7, 3.3, 10.0), (0.0123, n as f64 * 0.9, 120.5)] {
                    for (ypv, y0) in [(-56.0f32, 224.0f32), (-560.0, 224.0), (37.0, 10.0), (-5.0, 447.6)] {
                        for (wgt, pre) in [(24u16, 0u16), (60_000, 30_000), (1, 65_534)] {
                            let m = Map { ti, x_per_sample: xps, x0, y_per_volt: ypv, y0 };
                            let (mut a, mut b) = (vec![pre; w * h], vec![pre; w * h]);
                            deposit_dots_reference(&mut a, w, h, &v, &m, wgt);
                            deposit_dots(&mut b, w, h, &v, &m, wgt);
                            assert!(a == b, "n {} xps {} ti {} y {} {} wgt {}", n, xps, ti, ypv, y0, wgt);
                        }
                    }
                }
            }
        }
    }

    // Timing guard (tools/perf-guard.sh): dots in runs against one add per sample, per signal at 100 k points.
    #[test]
    #[ignore]
    fn perf_guard_dots() {
        let (w, h) = (970usize, 448usize);
        let names = ["sine", "two sines", "square", "ramp", "clipped", "flat", "noise", "noisy square", "gaps"];
        let n = 100_000usize;
        let mut worst: f64 = 0.0;
        for (name, v) in names.iter().zip(signals(n)) {
            let m = Map { ti: n as f64 / 2.0, x_per_sample: 970.0 / n as f64 * 0.8, x0: 485.0, y_per_volt: -56.0, y0: 224.0 };
            let (mut ma, mut mb) = (vec![0u16; w * h], vec![0u16; w * h]);
            let (old, new) = crate::perfguard::best_pair(crate::perfguard::ROUNDS, 22, || deposit_dots_reference(&mut ma, w, h, &v, &m, 1), || deposit_dots(&mut mb, w, h, &v, &m, 1));
            let r = new.as_secs_f64() / old.as_secs_f64();
            worst = worst.max(r);
            println!("dots {} pt {}: one add per sample {:?}, runs {:?} (ratio {:.2})", n, name, old, new, r);
        }
        assert!(worst <= 1.25, "dots in runs must not be slower: worst ratio {:.2}", worst);
    }

    #[test]
    fn min_max_equals_the_chain() {
        for v in signals(1000) {
            for (a, b) in [(0usize, 0usize), (0, 1), (3, 19), (0, 1000), (17, 500), (999, 1000)] {
                let (x, y) = (min_max(&v[a..b]), min_max_scalar(&v[a..b]));
                assert!(x.0 == y.0 && x.1 == y.1 || (x.0.is_nan() && y.0.is_nan()), "{:?} {:?}", x, y);
            }
        }
        assert_eq!(min_max(&[]), (f32::INFINITY, f32::NEG_INFINITY));
        assert_eq!(min_max(&[f32::NAN; 40]), (f32::INFINITY, f32::NEG_INFINITY));
    }

    #[test]
    fn edge_is_refined_near_pre() {
        // a ramp crossing 0.5 between samples 11 and 12 (at 11.25), another rising crossing far away
        let mut v: Vec<f32> = (0..40).map(|k| (k as f32 - 9.0) * 0.4 - 0.4).collect();
        v[30] = -1.0;
        v[31] = 1.0;
        let t = edge_near(&v, 11, 0.5, 0.0, true, false, 4).unwrap();
        assert!((t - 11.25).abs() < 1e-4, "{}", t);
        assert!(edge_near(&v, 11, 0.5, 0.0, false, false, 4).is_none());
        // the nearest of two crossings wins
        assert!((edge_near(&v, 29, 0.0, 0.0, true, false, 40).unwrap() - 30.5).abs() < 1e-4);
    }

    /// A slow sine with noise larger than the band, the hardware point on the falling edge: the smoothed search finds
    /// the rising edge half a period away, never the noise on the falling one.
    #[test]
    fn smoothed_search_takes_the_trend_not_the_noise() {
        // 12500 samples a period, amplitude 0.55, noise +-0.075 (pseudo random), level 0, band 0.06
        let mut r = 12345u32;
        let v: Vec<f32> = (0..50_000)
            .map(|k| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                0.55 * (k as f32 / 12500.0 * std::f32::consts::TAU).sin() + ((r >> 16) as f32 / 65536.0 - 0.5) * 0.15
            })
            .collect();
        for pre in [6250 + 130, 6250 - 40, 6250, 18750 + 200] {
            let (e, any) = edge_smooth(&v, pre, 0.0, 0.06, true, false, 31_250, 62, |_| true);
            let t = e.unwrap();
            assert!((t - 12500.0).abs() < 60.0 || (t - 25000.0).abs() < 60.0, "pre {}: {}", pre, t);
            assert!(!any.unwrap().rising, "pre {}: the nearest edge is the falling one", pre);
        }
        // at the rising edge itself
        assert!((edge_smooth(&v, 12_480, 0.0, 0.06, true, false, 31_250, 62, |_| true).0.unwrap() - 12500.0).abs() < 60.0);
        // falling, nearer the one at 6250 than the one at 18750 (from 12480 both are about as far: the comparators fire
        // at the first crossing, so the noise decides which is nearer)
        assert!((edge_smooth(&v, 11_000, 0.0, 0.06, false, false, 31_250, 62, |_| true).0.unwrap() - 6250.0).abs() < 60.0);
    }

    // Break it catches: the software trigger arming on noise the hardware ignores. A 0..5 V square with +-0.1 V noise
    // and the level 0.15 V above the low rail at 1 V/div (h = 0.3 V): the hardware never arms a rising edge (it needs
    // a sample below -0.15 V), so it forces frames, and the software must not find "edges" in that noise
    #[test]
    fn a_level_on_the_noise_of_a_rail_finds_no_edge() {
        let mut r = 777u32;
        let v: Vec<f32> = (0..20_000)
            .map(|k| {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (if (k / 5000) % 2 == 1 { 5.0 } else { 0.0 }) + ((r >> 16) as f32 / 65536.0 - 0.5) * 0.2
            })
            .collect();
        for l in [1usize, 4, 16] {
            for pre in [1000, 2500, 12_000] {
                let (e, any) = edge_smooth(&v, pre, 0.15, 0.3, true, false, 9000, l, |_| true);
                assert_eq!(e, None, "l {} pre {}", l, pre);
                // the falling edges arm (from 5 V) and fire
                assert!(any.is_some_and(|x| !x.rising), "l {} pre {}", l, pre);
            }
        }
        // the level in the middle: the rising edges are found at their crossings
        let (e, _) = edge_smooth(&v, 5100, 2.5, 0.3, true, false, 9000, 1, |_| true);
        assert!((e.unwrap() - 5000.0).abs() < 1.0);
    }

    /// A falling edge with noise crosses the level upwards too: with hysteresis that is no rising edge, and the rising
    /// edge half a period away is found.
    #[test]
    fn hysteresis_keeps_noise_on_the_other_edge_out() {
        // a sine, 1000 samples a period, with a +-0.02 wiggle; it falls through 0 at 500
        let v: Vec<f32> = (0..3000).map(|k| (k as f32 / 1000.0 * std::f32::consts::TAU).sin() + if k % 2 == 0 { 0.02 } else { -0.02 }).collect();
        // without hysteresis the wiggle at the falling crossing looks like a rising edge
        assert!((edge_near(&v, 500, 0.0, 0.0, true, false, 16).unwrap() - 500.0).abs() < 3.0);
        // with it, nothing rising near 500 ...
        assert!(edge_near(&v, 500, 0.0, 0.1, true, false, 16).is_none());
        // ... and the rising crossing at 1000 within half a period
        assert!((edge_near(&v, 500, 0.0, 0.1, true, false, 600).unwrap() - 1000.0).abs() < 3.0);
        // the falling one is where the hardware said
        assert!((edge_near(&v, 500, 0.0, 0.1, false, false, 16).unwrap() - 500.0).abs() < 6.0);
    }

    #[test]
    fn spans_per_column_and_joined_edges() {
        let (w, h) = (10, 20);
        let mut map = vec![0u16; w * h];
        // a square: 4 samples per column, low (row 15) for 5 columns, then high (row 5)
        let v: Vec<f32> = (0..40).map(|k| if k < 20 { 0.0 } else { 1.0 }).collect();
        let m = Map { ti: 0.0, x_per_sample: 0.25, x0: 0.0, y_per_volt: -10.0, y0: 15.0 };
        deposit(&mut map, w, h, &v, &m, 1);
        let col = |c: usize| (0..h).filter(|&r| map[r * w + c] > 0).collect::<Vec<_>>();
        assert_eq!(col(2), [15]);
        // column 5 holds the step: joined from row 15 (last low sample) up to row 5
        assert_eq!(col(5), (5..=15).collect::<Vec<_>>());
        assert_eq!(col(7), [5]);
        // samples off screen are skipped; a shift by ti moves the step
        let mut map2 = vec![0u16; w * h];
        deposit(&mut map2, w, h, &v, &Map { ti: 8.0, ..m }, 1);
        assert_eq!((0..h).filter(|&r| map2[r * w + 3] > 0).count(), 11);
        // dots: only the sample rows, no join across the step, 4 samples per column add up
        let mut map3 = vec![0u16; w * h];
        deposit_dots(&mut map3, w, h, &v, &m, 1);
        let col3 = |c: usize| (0..h).filter(|&r| map3[r * w + c] > 0).collect::<Vec<_>>();
        assert_eq!((col3(5), map3[5 * w + 5], col3(4)), (vec![5], 4, vec![15]));
    }
}


