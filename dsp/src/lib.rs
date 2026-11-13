// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! OOS signal processing, platform independent, no dependencies.
//!
//! - `bandwidth`: equivalent-time folding of periodic signals (also above Nyquist), step response -> rise time and
//!   magnitude response (front-end bandwidth test)
//! - `edges`: comparator with hysteresis -> edge list with interpolated times (the base of all triggers)
//! - `trigger`: software trigger types of a modern scope (edge, pulse, slope, video, pattern, duration, timeout, runt,
//!   window, delay, setup/hold, Nth edge) with time conditions
//! - `measure`: voltage, time and two-channel measurements
//! - `mask`: mask (pass/fail) test: envelope around a reference waveform, records tested against it
//! - `expr`: math expressions over the channels
//! - `math`: arithmetic, integral, derivative, filters, FFT with windows
//! - `magnetic`: inductance, B/H/μr, core loss and the I vs ∫V loop
//! - `motor`: Clarke/Park DQ0, shaft speed and the mechanical power ratio
//! - `dpt`: wide-bandgap double-pulse timings, recovery and Qoss
//! - `sim`: signal generators for tests and the demo source (pulse trains with glitches/runts, composite video, ...)
//! - `serial`: nominal high-speed serial records for the SIM menu (PRBS, tanh edges, Butterworth, de-emphasis)
//!
//! Waveforms are `&[f32]` volts sampled every `dt` seconds; times inside a waveform are fractional sample indices.

/// Analog channels the triggers and the expressions take. A board with more needs them extended here.
pub const CHANNELS: usize = 4;

pub mod bandwidth;
pub mod bode;
pub mod decode;
pub mod decode_ext;
pub mod decode_ext2;
pub mod decode_ext3;
pub mod dpt;
pub mod edges;
pub mod expr;
pub mod eye;
pub mod identify;
pub mod interp;
pub mod magnetic;
pub mod mask;
pub mod math;
pub mod measure;
pub mod motor;
pub mod power;
pub mod serial;
pub mod sim;
pub mod spectrum;
pub mod trigger;
