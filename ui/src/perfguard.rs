// SPDX-License-Identifier: GPL-2.0-or-later

//! Timing for the fork's ratio guards (`perf_guard*` tests, tools/perf-guard.sh). Other work on the host (parallel
//! builds, other agents) slows whichever side happens to run during it, so one timing of each side in a row gave ratios
//! from 1.1 to 1.9 on an unchanged tree. Here the two sides alternate, round by round, and each keeps its fastest round:
//! load hits both alike, and the fastest round is the least disturbed one.

use std::time::{Duration, Instant};

/// Rounds per comparison.
pub const ROUNDS: usize = 9;

/// Time per call of `old` and of `new`: `rounds` rounds of `runs` calls each, the two sides alternating (which one
/// starts alternates too), the fastest round of each.
pub fn best_pair(rounds: usize, runs: usize, mut old: impl FnMut(), mut new: impl FnMut()) -> (Duration, Duration) {
    let runs = runs.max(1);
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        for _ in 0..runs {
            f();
        }
        t.elapsed() / runs as u32
    };
    let (mut a, mut b) = (Duration::MAX, Duration::MAX);
    for r in 0..rounds.max(1) {
        if r % 2 == 0 {
            a = a.min(time(&mut old));
            b = b.min(time(&mut new));
        } else {
            b = b.min(time(&mut new));
            a = a.min(time(&mut old));
        }
    }
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fastest_round_of_each_side() {
        let (mut na, mut nb) = (0, 0);
        let (a, b) = best_pair(4, 3, || na += 1, || nb += 1);
        assert_eq!((na, nb), (12, 12));
        assert!(a < Duration::from_millis(5) && b < Duration::from_millis(5));
        // a side slowed in one round only keeps its fast rounds
        let mut k = 0;
        let (slow, _) = best_pair(
            5,
            1,
            || {
                k += 1;
                if k == 2 {
                    std::thread::sleep(Duration::from_millis(30));
                }
            },
            || {},
        );
        assert!(slow < Duration::from_millis(10), "{:?}", slow);
    }
}
