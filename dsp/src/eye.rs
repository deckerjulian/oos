// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Eye diagram and jitter: clock recovery from data or from a strobe (DQS), constant-rate least-squares fit or a 1st/2nd-order PLL,
//! eye accumulation with linear interpolation between samples, eye measurements, time interval error (TIE), RJ/DJ
//! separation with a dual-Dirac tail fit, DCD and DDJ from conditional means, periodic jitter from the TIE spectrum
//! and the bathtub curve.
//!
//! Times here are seconds from the first sample (not sample indices), the clock is "ticks" at the ideal edge times.

mod clock;
mod diagram;
mod jitter;
mod periodic;
mod psij;
mod signal;
mod tails;

pub use clock::{
    center_strobe, clock_from_edges, gate_around_clock, recover_clock, recover_clock_pll, unit_interval, Clock,
};
pub use diagram::{Eye, EyeMeas};
pub use jitter::{
    bathtub, eye_width_at, jitter, q_tail, tie, tie_spectrum, tie_trend, tj_at, Bathtub, EdgeMeans, Jitter, JitterFit,
    BATHTUB_FLOOR, BER_J2, BER_J9, MIN_EDGES, Q_BER_1E12, RUN_MIN, TAIL_EDGES,
};
pub use periodic::{is_clock, npj, phase_noise, pj_lines, srj_pp, subrate_pp};
pub use psij::{psij, Psij, PsijSide};
pub use signal::{ac_common_pp, dc_common, diff_crossover, ssc_deviation, ssc_rate, tnt_ratio};
use tails::line_fit;
pub use tails::norm_inv;

fn is_positive(value: f64) -> bool {
    value > 0.0
}

fn is_above<T: PartialOrd>(value: T, lower: T) -> bool {
    value > lower
}

fn is_probability(value: f64) -> bool {
    value > 0.0 && value < 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::Greater;

    #[test]
    fn predicates_keep_unordered_values_and_infinities() {
        let values = [
            f64::NEG_INFINITY,
            -1.0,
            -f64::MIN_POSITIVE,
            -0.0,
            0.0,
            f64::from_bits(1),
            0.5,
            1.0,
            f64::INFINITY,
            f64::NAN,
            -f64::NAN,
        ];
        for a in values {
            assert_eq!(is_positive(a), a.partial_cmp(&0.0) == Some(Greater));
            assert_eq!(is_probability(a), a > 0.0 && a < 1.0);
            for b in values {
                assert_eq!(is_above(a, b), a.partial_cmp(&b) == Some(Greater));
                assert_eq!(is_above(a as f32, b as f32), (a as f32).partial_cmp(&(b as f32)) == Some(Greater));
            }
        }
    }
}
