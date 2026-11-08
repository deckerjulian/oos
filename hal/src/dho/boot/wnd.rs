// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Decisions of the runtime ADC calibrations as pure functions: the sync-delay choice
//! and the lane window search (cold-boot.md 3.3 / 3.4). The expected results are the unit's own
//! log of 2026-11-02 [O].

/// Sync delay from the bit-1 values of ADC reg 7 for delays 0..7: the first d in 0..6 with r[d] = 1 and r[d+1] = 0
/// None = no falling edge (the stock system then leaves the delay at 7 without the final sync) [N].
pub fn sync_choice(r: &[bool; 8]) -> Option<u32> {
    (0..7).find(|&d| r[d] && !r[d + 1]).map(|d| d as u32)
}

/// Reference window: centre, polarity, start, end, length, end position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefWnd {
    /// centre C = E - (L + 1) / 2
    pub c: usize,
    /// lane bit at C (0 = error-free window)
    pub p: u32,
    /// S = E - L + 1 (one past the true start of the run)
    pub s: usize,
    /// E - 1
    pub end: usize,
    /// run length L
    pub len: usize,
    /// E: index of the first value after the run
    pub e: usize,
}

/// Shortest run `reference_window` accepts, exclusive: 14 taps at 1.25 GHz. The IDELAY tap is a fixed
/// time, the lane bit period shrinks with the ADC clock, so at an experimental clock the run is scaled by
/// 1.25 GHz / clock (2 GHz: 8) [A: only the 1.25 GHz value is known].
pub fn min_run() -> usize {
    (14 * crate::dho::clock::NOMINAL_HZ / crate::dho::clock::adc_hz()).max(4) as usize
}

/// Walks d = 1..31 on `lane` and takes the first run (0s or 1s) longer than 14 that is ended by a
/// transition. Run counters start at d = 1, so a run that starts at d = 0 counts one short [N].
pub fn reference_window(err: &[u32; 32], lane: u32) -> Option<RefWnd> {
    let min = min_run();
    let bit = |d: usize| (err[d] >> lane) & 1;
    let (mut ones, mut zeros, mut len, mut e) = (0usize, 0usize, 0usize, 0usize);
    for d in 1..32 {
        let (cur, prev) = (bit(d), bit(d - 1));
        if cur != prev && len == 0 {
            let run = if cur == 1 { zeros } else { ones };
            if run > min {
                len = run;
                e = d;
            }
        }
        if cur == 0 {
            ones = 0;
            zeros += 1;
        } else {
            zeros = 0;
            ones += 1;
        }
    }
    if len < min + 1 {
        return None;
    }
    let c = e - (len + 1) / 2;
    Some(RefWnd { c, p: bit(c), s: e - len + 1, end: e - 1, len, e })
}

/// Delays of lanes `first..first + 14`. Per lane: if its bit at C equals P, the run of
/// P around C (down to 0, up to 31) gives length n and last index e, delay = max(0, e - n/2); else if the bit at S
/// equals P, the run from S up gives e and delay = max(0, e - L/2); else if the bit at E-1 equals P, the run of P from
/// C+1 up gives n and e (e = C if there is none), delay = max(0, e - n/2); else the lane fails. On a failure
/// **none** of the 14 delays of this call is written [N]. `Err(lane)` = the first failing lane.
pub fn lane_windows(err: &[u32; 32], first: u32, r: &RefWnd) -> Result<[u32; 14], u32> {
    let mut out = [0u32; 14];
    for (k, o) in out.iter_mut().enumerate() {
        let lane = first + k as u32;
        let bit = |d: usize| (err[d] >> lane) & 1;
        // run of P upwards from `from` (inclusive): (count, last index or `dflt`)
        let up = |from: usize, dflt: usize| {
            let (mut n, mut last) = (0usize, dflt);
            for d in from..32 {
                if bit(d) != r.p {
                    break;
                }
                n += 1;
                last = d;
            }
            (n, last)
        };
        let (n, e) = if bit(r.c) == r.p {
            let below = (0..=r.c).rev().take_while(|&d| bit(d) == r.p).count();
            let (n_up, e) = up(r.c + 1, r.c);
            (below + n_up, e)
        } else if bit(r.s) == r.p {
            let (_, e) = up(r.s, r.s);
            (r.len, e)
        } else if bit(r.end) == r.p {
            up(r.c + 1, r.c)
        } else {
            return Err(lane);
        };
        *o = (e as i64 - (n / 2) as i64).max(0) as u32;
    }
    Ok(out)
}

/// Why the eye scan gave no complete delay list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EyeFail {
    /// no run longer than 14 on the reference lane 2
    NoReference,
    /// a lane without a window: the 14 delays of its half (0-13 or 14-27) are not set
    Lane(u32),
}

/// Evaluation of the lane delay scan: reference lane 2, then lanes 0-13 and 14-27. `Ok` only if every lane has a window;
/// OOS then loads the list, else it falls back to the stored list [A].
pub fn lane_delays(err: &[u32; 32]) -> Result<[u32; 28], EyeFail> {
    let r = reference_window(err, 2).ok_or(EyeFail::NoReference)?;
    let lo = lane_windows(err, 0, &r).map_err(EyeFail::Lane)?;
    let hi = lane_windows(err, 14, &r).map_err(EyeFail::Lane)?;
    let mut d = [0u32; 28];
    d[..14].copy_from_slice(&lo);
    d[14..].copy_from_slice(&hi);
    Ok(d)
}

/// The eye of one lane as logged ("Line k: ..."), d = 0 first, '1' = errors.
pub fn lane_string(err: &[u32; 32], lane: u32) -> String {
    (0..32).map(|d| if (err[d] >> lane) & 1 != 0 { '1' } else { '0' }).collect()
}

#[cfg(test)]
mod tests {
    use super::super::sim;
    use super::*;

    #[test]
    fn sync_choice_cases() {
        // unit: "ADC0 Sync DO:1,0,1,1,1,0,0,1" -> 0 (live shadow s06 = 0x00c3) [O]
        assert_eq!(sync_choice(&sim::UNIT_SYNC_DO), Some(0));
        let b = |v: [u8; 8]| v.map(|x| x != 0);
        assert_eq!(sync_choice(&b([0, 1, 0, 0, 0, 0, 0, 0])), Some(1));
        assert_eq!(sync_choice(&b([1, 1, 1, 0, 0, 0, 0, 0])), Some(2));
        assert_eq!(sync_choice(&b([0, 0, 0, 0, 0, 0, 1, 0])), Some(6));
        assert_eq!(sync_choice(&b([0, 0, 0, 0, 0, 0, 0, 1])), None);
        assert_eq!(sync_choice(&b([1; 8])), None);
        assert_eq!(sync_choice(&b([0; 8])), None);
    }

    #[test]
    fn logcat_eye_gives_the_logged_delays() {
        let err = sim::eye_from_lanes(&sim::UNIT_EYE);
        // the documented summary of the scan: 0x0fffffff for d = 0, 1, 24..31, 0 for d = 3..21
        for d in [0, 1, 24, 31] {
            assert_eq!(err[d], 0x0fff_ffff);
        }
        assert!((3..=21).all(|d| err[d] == 0));
        for lane in 0..28 {
            assert_eq!(lane_string(&err, lane), sim::UNIT_EYE[lane as usize]);
        }
        let r = reference_window(&err, 2).unwrap();
        assert_eq!(r, RefWnd { c: 13, p: 0, s: 4, end: 23, len: 21, e: 24 });
        assert_eq!(lane_delays(&err).unwrap(), sim::UNIT_IDELAY);
    }

    fn eye(lanes: &[(u32, &str)]) -> [u32; 32] {
        let mut e = [0u32; 32];
        for &(l, s) in lanes {
            for (d, ch) in s.chars().enumerate() {
                e[d] |= ((ch == '1') as u32) << l;
            }
        }
        e
    }

    #[test]
    fn window_branches() {
        let reference = "11100000000000000000000011111111"; // C = 13, S = 4, E-1 = 23, L = 21
        // lane 0: error at C, error-free at S -> run from S up (4..=9), delay = 9 - 21/2 = max(0, -1) = 0
        // lane 1: errors at C and S, error-free at E-1 -> run from C+1 up: 14..=23, n = 10, delay 23 - 5 = 18
        // lane 3: errors at C, S and E-1 -> fails
        let e = eye(&[(2, reference), (0, "11110000001111111111111111111111"), (1, "11111111111111000000000011111111")]);
        let r = reference_window(&e, 2).unwrap();
        let d = lane_windows(&e, 0, &r).unwrap();
        assert_eq!((d[0], d[1], d[2]), (0, 18, 13));
        let e = eye(&[(2, reference), (3, "11111111111111111111111111111111")]);
        assert_eq!(lane_windows(&e, 0, &r), Err(3));
        assert_eq!(lane_delays(&e), Err(EyeFail::Lane(3)));
        // lane 2 has no run > 14: no reference
        let e = eye(&[(2, "11111111110000000000001111111111")]);
        assert_eq!(lane_delays(&e), Err(EyeFail::NoReference));
        // a run starting at d = 0 counts one short: 15 zeros from 0 -> 14 counted -> no reference
        let e = eye(&[(2, "00000000000000011111111111111111")]);
        assert_eq!(reference_window(&e, 2), None);
        let e = eye(&[(2, "00000000000000001111111111111111")]);
        assert_eq!(reference_window(&e, 2).map(|r| (r.len, r.e, r.c)), Some((15, 16, 8)));
    }
}
