// SPDX-License-Identifier: GPL-2.0-or-later

//! What a frame read with the FPGA display compression holds (0x1170, `seq::Settings::compress` = k), and a check of
//! a compressed read against a raw read of the same record (fork, docs/fork/compress.md).
//!
//! Unit evidence (unitlab round 2, 2026-11-13; private RE notes docs/logs/oos_unitlab_20261113.md) and the netlist
//! simulation (private RE notes docs/re/netlist/blocks/datapath.md, "Read-out compression 0x1170"): pair i holds the
//! minimum and the maximum of the stored samples [2k i, 2k (i + 1)) from the first sample of the read window, **in the
//! order they first occur** (the maximum first when it comes first). 100 % of the points for k = 3, 4, 15, 16, 64,
//! 128, one and two channels. `check` still tries the other layouts, so a read that does not fit says which does.

/// Samples a block stands for, in units of k. 2: points 2i and 2i+1 are the minimum and maximum of the same 2k
/// samples (every extreme survives). 1: point j covers its own k samples, even points one extreme, odd points the
/// other (an extreme in the wrong half is lost).
pub const BLOCK_K: usize = 2;
/// Order of the two points of a pair.
pub const ORDER: Order = Order::Time;
/// Points per min/max pair.
pub const PAIR: usize = 2;

/// Which extreme a pair (or, with blocks of k, an even point) gives first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// in the order the two extremes first occur in the block (the FPGA's)
    Time,
    MinFirst,
    MaxFirst,
}

/// A layout of the compressed points (see `BLOCK_K`); `shift` moves the points against the record by whole points
/// (point j stands for the block of point j + shift of an aligned read).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub block_k: usize,
    pub order: Order,
    pub shift: i64,
}

/// The layout the UI assumes.
pub const LAYOUT: Layout = Layout { block_k: BLOCK_K, order: ORDER, shift: 0 };

/// First index of the minimum and of the maximum of `s` (not empty).
fn extremes(s: &[u16]) -> ((usize, u16), (usize, u16)) {
    let (mut lo, mut hi) = ((0, s[0]), (0, s[0]));
    for (i, &x) in s.iter().enumerate() {
        if x < lo.1 {
            lo = (i, x);
        }
        if x > hi.1 {
            hi = (i, x);
        }
    }
    (lo, hi)
}

/// Point `j` of a compressed read of `raw` with factor `k` in layout `l`, None outside the record.
pub fn point(raw: &[u16], k: usize, l: Layout, j: usize) -> Option<u16> {
    let k = k.max(1);
    let j = j as i64 + l.shift;
    if j < 0 {
        return None;
    }
    let j = j as usize;
    let span = l.block_k.max(1) * k;
    let a = if l.block_k >= 2 { j / PAIR * span } else { j * k };
    let b = (a + span).min(raw.len());
    if a >= b {
        return None;
    }
    let ((ilo, lo), (ihi, hi)) = extremes(&raw[a..b]);
    let first = j.is_multiple_of(PAIR);
    let min_first = match l.order {
        Order::Time => ilo <= ihi,
        Order::MinFirst => true,
        Order::MaxFirst => false,
    };
    Some(if first == min_first { lo } else { hi })
}

/// The whole compressed read of `raw` (one point per k samples, the last block possibly short) in layout `l`.
pub fn model(raw: &[u16], k: usize, l: Layout) -> Vec<u16> {
    let n = raw.len().div_ceil(k.max(1));
    (0..n).map_while(|j| point(raw, k, l, j)).collect()
}

/// How well one layout explains a compressed read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub layout: Layout,
    /// points compared, points within one code, largest difference in codes
    pub points: usize,
    pub within: usize,
    pub max_err: u32,
}

/// Points compared per layout (enough for a 1 kHz square at every depth, cheap at k = 256).
pub const CHECK_POINTS: usize = 20_000;

/// Every candidate layout (block of 2k in time order or a fixed order, block of k, shifts of -2..=2 points) against the compressed read `comp`
/// of the raw read `raw` of the same record (one channel, read-out words), best first. Only points whose block lies
/// inside both reads count, at most `CHECK_POINTS` from the middle of the record (the trigger point).
pub fn check(raw: &[u16], comp: &[u16], k: usize) -> Vec<Fit> {
    let n = comp.len().min(raw.len().div_ceil(k.max(1)));
    let lo = n.saturating_sub(CHECK_POINTS) / 2;
    let hi = (lo + CHECK_POINTS).min(n);
    let mut fits = Vec::new();
    for (block_k, order) in [(2, Order::Time), (2, Order::MinFirst), (2, Order::MaxFirst), (1, Order::MinFirst), (1, Order::MaxFirst)] {
        for shift in -2..=2i64 {
            let layout = Layout { block_k, order, shift };
            let (mut points, mut within, mut max_err) = (0, 0, 0u32);
            for (j, &c) in comp.iter().enumerate().take(hi).skip(lo) {
                let Some(m) = point(raw, k, layout, j) else { continue };
                let e = (c as i32 - m as i32).unsigned_abs();
                points += 1;
                within += (e <= 1) as usize;
                max_err = max_err.max(e);
            }
            fits.push(Fit { layout, points, within, max_err });
        }
    }
    fits.sort_by(|a, b| (b.within, a.max_err).cmp(&(a.within, b.max_err)));
    fits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noisy(n: usize) -> Vec<u16> {
        let mut x = 12345u32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let sq = if (i / 700) % 2 == 0 { 30_000 } else { 36_000 };
                (sq + (x >> 16) as i32 % 200 - 100) as u16
            })
            .collect()
    }

    #[test]
    fn the_pairs_are_min_and_max_of_2k_samples_in_time_order() {
        let raw = [5u16, 9, 1, 7, 3, 3, 8, 2];
        // block [5 9 1 7]: the max (9) comes before the min (1); block [3 3 8 2]: max 8 before min 2
        assert_eq!(model(&raw, 2, LAYOUT), vec![9, 1, 8, 2]);
        let up = [4u16, 1, 6, 9];
        assert_eq!(model(&up, 2, LAYOUT), vec![1, 9]);
        // the fixed orders, and a block of k
        assert_eq!(model(&raw, 2, Layout { order: Order::MinFirst, ..LAYOUT }), vec![1, 9, 2, 8]);
        assert_eq!(model(&raw, 2, Layout { order: Order::MaxFirst, ..LAYOUT }), vec![9, 1, 8, 2]);
        assert_eq!(model(&raw, 2, Layout { block_k: 1, order: Order::MinFirst, ..LAYOUT }), vec![5, 7, 3, 8]);
        // a short last block, a shift by one point, a flat block
        assert_eq!(model(&raw[..7], 2, LAYOUT), vec![9, 1, 3, 8]);
        assert_eq!(model(&raw, 2, Layout { shift: 1, ..LAYOUT }), vec![1, 8, 2]);
        assert_eq!(model(&[7u16; 4], 2, LAYOUT), vec![7, 7]);
    }

    #[test]
    fn a_single_sample_spike_survives_with_blocks_of_2k_only() {
        let mut raw = vec![1000u16; 64];
        // a spike in the half of a block whose point is the minimum with blocks of k
        raw[16] = 4000;
        assert!(model(&raw, 4, LAYOUT).contains(&4000));
        assert!(!model(&raw, 4, Layout { block_k: 1, order: Order::MinFirst, ..LAYOUT }).contains(&4000));
    }

    // Break it catches: a check that cannot tell time order from a fixed order (it fitted about half the points with
    // min first on the unit), or another layout winning on its own reads.
    #[test]
    fn check_finds_the_layout_a_read_was_made_with() {
        let raw = noisy(60_000);
        for k in [4usize, 16, 64, 256] {
            for truth in [LAYOUT, Layout { order: Order::MinFirst, ..LAYOUT }, Layout { order: Order::MaxFirst, ..LAYOUT }, Layout { block_k: 1, order: Order::MinFirst, ..LAYOUT }, Layout { shift: -1, ..LAYOUT }] {
                // a read shifted by -1 point: one point in front of the aligned ones
                let aligned = model(&raw, k, Layout { shift: 0, ..truth });
                let comp: Vec<u16> = std::iter::repeat_n(0, (-truth.shift).max(0) as usize).chain(aligned).collect();
                let fits = check(&raw, &comp, k);
                let best = fits[0];
                assert_eq!(best.layout, truth, "k {}", k);
                assert_eq!((best.within, best.max_err), (best.points, 0), "k {}", k);
                assert!(best.points > 100, "k {}: {} points", k, best.points);
            }
            // a time-ordered read fits a fixed order only in part
            let comp = model(&raw, k, LAYOUT);
            let min_first = check(&raw, &comp, k).into_iter().find(|f| f.layout == Layout { order: Order::MinFirst, ..LAYOUT }).unwrap();
            assert!(min_first.within < min_first.points * 9 / 10, "k {}: {} of {}", k, min_first.within, min_first.points);
        }
    }
}
