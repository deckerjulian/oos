// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The clock of a record for the eye and the jitter measurements: thresholds from the record's levels, the mean
//! clock, then a PLL near it. The eye page and the measurement modules of oos-ui both start from here.

use super::{recover_clock, recover_clock_pll, Clock};
use crate::levels::stats;

#[derive(Clone, Debug)]
/// What the clock recovery of `infer` found: the rate, the decision threshold and its hysteresis in volts, the
/// samples per unit interval, how the clock was recovered, and a warning for a doubtful result (empty when none).
pub struct Summary {
    pub rate: f64,
    pub threshold: f32,
    pub hysteresis: f32,
    pub samples_per_ui: f64,
    pub recovery: &'static str,
    pub warning: String,
}

/// The clock of an NRZ record `v` (one sample every `dt` seconds): with `requested`, near that rate; with
/// `explicit_clock`, `v` is the clock itself and its mean period is taken. Err says why it cannot be found.
pub fn infer(v: &[f32], dt: f64, requested: Option<f64>, explicit_clock: bool) -> Result<(Summary, Clock), String> {
    if v.len() < 8 || !dt.is_finite() || dt <= 0.0 {
        return Err("Need at least 8 samples with a valid interval".into());
    }
    let st = stats(v, dt);
    let span = st.vmax - st.vmin;
    if !span.is_finite() || span <= 1e-9 {
        return Err("Flat source: no transitions to recover a clock".into());
    }
    let amplitude = if st.valid { (st.top - st.base).max(span * 0.25) } else { span };
    let threshold = if st.valid && !explicit_clock { 0.5 * (st.top + st.base) } else { 0.5 * (st.vmax + st.vmin) };
    // Voltage thresholds follow the data, never the waveform's display zoom.
    let hysteresis = amplitude * if explicit_clock { 0.2 } else { 0.1 };
    let mean_clock = recover_clock(v, dt, threshold, hysteresis, requested)
        .ok_or_else(|| "Need at least 3 usable transitions; set a rate or explicit clock".to_string())?;
    let samples_per_ui = mean_clock.period / dt;
    if !samples_per_ui.is_finite() || samples_per_ui < 2.0 {
        return Err("Recovered UI is below 2 samples: set a known rate/clock or use a faster record".into());
    }
    let clock = if explicit_clock {
        mean_clock
    } else {
        recover_clock_pll(v, dt, threshold, hysteresis, Some(mean_clock.rate()), 0.0, true).unwrap_or(mean_clock)
    };
    let warning = if samples_per_ui < 10.0 {
        format!("Limited sampling: {:.1} samples/UI", samples_per_ui)
    } else if span < 0.05 {
        "Low amplitude: check signal/noise before interpreting jitter".into()
    } else {
        String::new()
    };
    Ok((
        Summary {
            rate: clock.rate(),
            threshold,
            hysteresis,
            samples_per_ui,
            recovery: if explicit_clock {
                "Explicit clock"
            } else if requested.is_some() {
                "NRZ PLL near set rate"
            } else {
                "Auto NRZ PLL"
            },
            warning,
        },
        clock,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_nrz_pattern_gives_its_rate() {
        // a 1 MHz PRBS-like pattern, 20 samples a bit at 20 MSa/s
        let mut lfsr = 0x5au8;
        let mut v = Vec::new();
        for _ in 0..400 {
            lfsr = lfsr << 1 | ((lfsr >> 7 ^ lfsr >> 5 ^ lfsr >> 4 ^ lfsr >> 3) & 1);
            v.extend(std::iter::repeat_n(if lfsr & 1 == 1 { 1.0f32 } else { 0.0 }, 20));
        }
        let (summary, clock) = infer(&v, 50e-9, None, false).unwrap();
        assert!((summary.rate - 1e6).abs() < 1e3, "{}", summary.rate);
        assert!((summary.samples_per_ui - 20.0).abs() < 0.1, "{}", summary.samples_per_ui);
        assert!((clock.rate() - 1e6).abs() < 1e3);
    }

    #[test]
    fn a_flat_or_short_record_says_why() {
        assert!(infer(&[0.0; 4], 1e-9, None, false).unwrap_err().contains("8 samples"));
        assert!(infer(&[0.5; 64], 1e-9, None, false).unwrap_err().contains("Flat"));
    }
}
