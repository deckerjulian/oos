// Copyright (C) 2026 OOS contributors
// SPDX-License-Identifier: GPL-2.0-or-later

//! Geometry of the hardware zone tables: the vertical extent of a zone polygon inside a time strip (one table column).

/// The lowest and highest value of the polygon `poly` (time, value) within the strip `ta..=tb`: its vertices inside
/// the strip and the crossings of its edges with the strip's borders. None when the polygon does not reach the strip.
/// For a simple polygon this is the exact vertical extent of the part inside the strip.
pub fn extent(poly: &[(f64, f64)], ta: f64, tb: f64) -> Option<(f64, f64)> {
    let n = poly.len();
    if n < 2 || tb < ta {
        return None;
    }
    let mut r: Option<(f64, f64)> = None;
    let mut take = |y: f64| {
        r = Some(r.map_or((y, y), |(a, b)| (a.min(y), b.max(y))));
    };
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        if (ta..=tb).contains(&p.0) {
            take(p.1);
        }
        let (lo, hi) = if p.0 <= q.0 { (p, q) } else { (q, p) };
        for x in [ta, tb] {
            if lo.0 < x && x < hi.0 {
                take(lo.1 + (hi.1 - lo.1) * (x - lo.0) / (hi.0 - lo.0));
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_of_box_and_triangle() {
        let bx = [(0.0, 1.0), (10.0, 1.0), (10.0, 2.0), (0.0, 2.0)];
        // (strip) -> extent
        let cases: [((f64, f64), Option<(f64, f64)>); 5] = [
            ((2.0, 3.0), Some((1.0, 2.0))),
            ((-5.0, -1.0), None),
            ((9.0, 12.0), Some((1.0, 2.0))),
            ((11.0, 12.0), None),
            ((-1.0, 0.0), Some((1.0, 2.0))),
        ];
        for ((a, b), want) in cases {
            assert_eq!(extent(&bx, a, b), want, "{}..{}", a, b);
        }
        // a triangle with its apex at t 5, value 4, base 0..10 at value 0
        let tri = [(0.0, 0.0), (10.0, 0.0), (5.0, 4.0)];
        assert_eq!(extent(&tri, 1.0, 2.0), Some((0.0, 1.6)));
        assert_eq!(extent(&tri, 4.0, 6.0), Some((0.0, 4.0)));
        let (lo, hi) = extent(&tri, 9.0, 9.5).unwrap();
        assert!(lo == 0.0 && (hi - 0.8).abs() < 1e-12, "{}", hi);
    }
}
