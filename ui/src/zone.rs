// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Zone trigger (boxes / any shape): up to two zones in waveform coordinates (time relative to
//! the trigger, volts of the zone's source channel). A record is kept only if it intersects every "must intersect"
//! zone and none of the "must not intersect" zones; the hardware trigger stays the normal one.

/// One zone: a box (2 corner points) or a polygon (3 or more points).
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub on: bool,
    pub src: usize,
    /// true: the trace must pass through the zone; false: it must not
    pub inside: bool,
    /// (time s, volts)
    pub pts: Vec<(f64, f64)>,
}

impl Default for Zone {
    /// Off, CH1, "must intersect", no outline yet.
    fn default() -> Zone {
        Zone { on: false, src: 0, inside: true, pts: Vec::new() }
    }
}

impl Zone {
    /// The outline as a polygon (a box becomes its 4 corners).
    pub fn polygon(&self) -> Vec<(f64, f64)> {
        match self.pts.len() {
            2 => {
                let ((t0, v0), (t1, v1)) = (self.pts[0], self.pts[1]);
                vec![(t0, v0), (t1, v0), (t1, v1), (t0, v1)]
            }
            _ => self.pts.clone(),
        }
    }

    pub fn usable(&self) -> bool {
        self.on && self.pts.len() >= 2
    }
}

fn inside_poly(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    // even-odd ray casting
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
        j = i;
    }
    c
}

fn segments_cross(p: (f64, f64), q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    let d = |o: (f64, f64), x: (f64, f64), y: (f64, f64)| (x.0 - o.0) * (y.1 - o.1) - (x.1 - o.1) * (y.0 - o.0);
    let (d1, d2, d3, d4) = (d(a, b, p), d(a, b, q), d(p, q, a), d(p, q, b));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// Does the waveform `v` (sample k at time t0 + k dt) touch the zone? Samples inside, or a line between two
/// samples crossing an edge (sparse records).
pub fn hits(z: &Zone, v: &[f32], t0: f64, dt: f64) -> bool {
    let poly = z.polygon();
    if poly.len() < 3 || v.is_empty() || dt <= 0.0 {
        return false;
    }
    let (tmin, tmax) = poly.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    // only the samples around the zone's time range
    let k0 = (((tmin - t0) / dt).floor() - 1.0).max(0.0) as usize;
    let k1 = ((((tmax - t0) / dt).ceil() + 1.0).max(0.0) as usize).min(v.len().saturating_sub(1));
    let mut prev: Option<(f64, f64)> = None;
    for k in k0..=k1 {
        let y = v[k];
        if !y.is_finite() {
            prev = None;
            continue;
        }
        let p = (t0 + k as f64 * dt, y as f64);
        if inside_poly(p, &poly) {
            return true;
        }
        if let Some(q) = prev {
            let n = poly.len();
            if (0..n).any(|i| segments_cross(q, p, poly[i], poly[(i + 1) % n])) {
                return true;
            }
        }
        prev = Some(p);
    }
    false
}

/// The zone condition of a record: every usable zone met (channels without data fail an "inside" zone).
pub fn pass(zones: &[Zone], ch: &[Vec<f32>], t0: f64, dt: f64) -> bool {
    zones.iter().filter(|z| z.usable()).all(|z| {
        let v = ch.get(z.src).map(|v| v.as_slice()).unwrap_or(&[]);
        hits(z, v, t0, dt) == z.inside
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_and_polygon() {
        // a 1 kHz square wave 0 / 3 V, 1 us samples from -1 ms
        let v: Vec<f32> = (0..2000).map(|k| if (k / 500) % 2 == 0 { 0.0 } else { 3.0 }).collect();
        let (t0, dt) = (-1e-3, 1e-6);
        // a box around the high level at 0.2..0.3 ms, 2.5..3.5 V: the wave is low there (k 1200..1300 -> low)
        let b = Zone { on: true, src: 0, inside: true, pts: vec![(0.2e-3, 2.5), (0.3e-3, 3.5)] };
        assert!(!hits(&b, &v, t0, dt));
        // the same box at -0.3..-0.2 ms (k 700..800, high): hit
        let b2 = Zone { pts: vec![(-0.3e-3, 2.5), (-0.2e-3, 3.5)], ..b.clone() };
        assert!(hits(&b2, &v, t0, dt));
        // a thin box across the edge at t = 0 (k 1000) between 1 and 2 V: only the vertical edge passes it
        let edge = Zone { pts: vec![(-0.5e-6, 1.0), (0.5e-6, 2.0)], ..b.clone() };
        assert!(hits(&edge, &v, t0, dt));
        // a triangle around the edge at -0.5 ms (k 500, rising)
        let tri = Zone { pts: vec![(-0.52e-3, 1.0), (-0.48e-3, 1.0), (-0.5e-3, 2.0)], ..b.clone() };
        assert!(hits(&tri, &v, t0, dt));
        // pass: must hit b2, must not hit b
        let nb = Zone { inside: false, ..b };
        assert!(pass(&[b2.clone(), nb.clone()], &[v.clone()], t0, dt));
        assert!(!pass(&[Zone { inside: false, ..b2 }, nb], &[v], t0, dt));
        // nothing usable: pass
        assert!(pass(&[Zone::default()], &[], t0, dt));
    }
}
