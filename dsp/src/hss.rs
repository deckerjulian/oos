// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! High-speed serial waveform from HSS Simulator v1.12.
//!
//! The PRBS taps, the spread-spectrum edge times, and the jitter steps are the
//! functions in `waveform_engine.py` (`PRBSGenerator`, `_ssc_edge_times`,
//! `_apply_jitter`). PCIe 3.0 Nominal is the JSON in `Presets/PCIe_3.0_Nominal.json`.
//! The engine default Butterworth order is 3 because that file omits `order`.
//! Samples are volts. The Python engine returns millivolts.
//! De-emphasis preserves the rail midpoint, and correlated RJ uses a physical
//! digital -3 dB corner and zero finite-record mean. These correct the source
//! port's offset-rail attenuation and normalized-frequency mistakes. SSC retains
//! the source's small-signal convention: "down-spread" removes its DC term.

/// Samples per line symbol when the record is rendered at the preset rate.
pub const SAMPLES_PER_UI: usize = 8;

/// Length used when spread-spectrum clocking is off.
pub const PLAIN_LEN: usize = 65_536;

/// Hard ceiling for one record (64 MiB of output samples). Temporary DSP buffers
/// are bounded by the same grid. A slower SSC modulation must fail explicitly.
pub const MAX_SAMPLES: usize = 16 * 1024 * 1024;

/// Bound edge-shaping work as well as samples: a huge rise/rate combination can
/// otherwise require billions of tanh evaluations inside an otherwise valid grid.
const MAX_SHAPING_WORK: f64 = 64.0 * 1024.0 * 1024.0;

/// Deterministic stand-in for NumPy's process-wide RNG. The widths are the JSON
/// amplitudes. The same seed draws the same record.
pub const PCIE3_SEED: u64 = 0x5043_4933;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Butterworth,
}

#[derive(Clone, Debug)]
pub struct Ssc {
    /// Percent, as stored in the JSON. 0.5 means 0.5%.
    pub deviation_percent: f64,
    pub freq_khz: f64,
    /// `"Down-spread only"` or `"Center-spread"`.
    pub profile: &'static str,
}

#[derive(Clone, Debug)]
pub enum Jitter {
    /// Gaussian, picoseconds RMS.
    Rj { ps_rms: f64 },
    /// Sine, peak picoseconds, frequency in kilohertz.
    Pj { freq_khz: f64, ps_peak: f64 },
    /// Duty-cycle distortion, picoseconds peak-to-peak. Rising edges late by half, falling early by half.
    Dcd { ps: f64 },
    /// Correlated RJ: white Gaussian, first-order low-pass at a physical -3 dB
    /// corner, then centered and rescaled to picoseconds RMS.
    CorrRj { ps_rms: f64, corner_khz: f64 },
}

/// Settings the generator reads. Voltages in the JSON are millivolts.
#[derive(Clone, Debug)]
pub struct Settings {
    pub name: &'static str,
    /// Line bit rate in Gb/s, the JSON `data_rate`.
    pub data_rate_gbps: f64,
    pub prbs_order: u32,
    pub v_high_mv: f64,
    pub v_low_mv: f64,
    /// 20–80% rise, picoseconds.
    pub rise_time_ps: f64,
    pub noise_rms_mv: f64,
    /// Gigahertz. Zero skips the filter.
    pub bandwidth_ghz: f64,
    pub filter: FilterKind,
    /// JSON omits this. The engine default is 3.
    pub filter_order: usize,
    /// De-emphasis in dB. Zero skips it.
    pub deemph_db: f64,
    pub ssc: Option<Ssc>,
    pub jitter: Vec<Jitter>,
    pub seed: u64,
}

/// PCIe 3.0 Nominal, copied from `Presets/PCIe_3.0_Nominal.json`.
pub fn pcie3_nominal() -> Settings {
    Settings {
        name: "PCIe 3.0",
        data_rate_gbps: 8.0,
        prbs_order: 31,
        v_high_mv: 500.0,
        v_low_mv: -500.0,
        rise_time_ps: 35.0,
        noise_rms_mv: 4.5,
        bandwidth_ghz: 6.0,
        filter: FilterKind::Butterworth,
        filter_order: 3,
        deemph_db: 3.5,
        ssc: Some(Ssc { deviation_percent: 0.5, freq_khz: 33.0, profile: "Down-spread only" }),
        jitter: vec![Jitter::Rj { ps_rms: 1.2 }, Jitter::Pj { freq_khz: 8000.0, ps_peak: 4.0 }],
        seed: PCIE3_SEED,
    }
}

/// NRZ line symbol rate in hertz. PAM is not in this port.
pub fn symbol_rate_hz(s: &Settings) -> f64 {
    s.data_rate_gbps * 1e9
}

/// Sample interval and length. With SSC on, the length covers one full modulation
/// period at [`SAMPLES_PER_UI`] samples per unit interval. That is the ramp, not a
/// 1 µs slice of it.
pub fn sample_grid(s: &Settings) -> (f64, usize) {
    try_sample_grid(s).expect("invalid HSS settings; use try_sample_grid for user inputs")
}

/// Checked variant of [`sample_grid`], including a bound before any large allocation.
pub fn try_sample_grid(s: &Settings) -> Result<(f64, usize), String> {
    validate_settings(s)?;
    let dt = 1.0 / (SAMPLES_PER_UI as f64 * symbol_rate_hz(s));
    if !dt.is_finite() || dt <= 0.0 {
        return Err("data rate does not produce a finite sample interval".into());
    }
    let count = match &s.ssc {
        Some(ssc) => (1.0 / (ssc.freq_khz * 1e3 * dt)).ceil(),
        None => PLAIN_LEN as f64,
    };
    if !count.is_finite() || count > MAX_SAMPLES as f64 {
        return Err(format!("SSC record exceeds the {MAX_SAMPLES}-sample limit; increase SSC frequency or reduce data rate"));
    }
    let n = (count as usize).max(SAMPLES_PER_UI);
    validate_render_grid(s, dt, n)?;
    Ok((dt, n))
}

fn validate_settings(s: &Settings) -> Result<(), String> {
    for (name, value) in [
        ("data rate", s.data_rate_gbps), ("high level", s.v_high_mv),
        ("low level", s.v_low_mv), ("rise time", s.rise_time_ps),
        ("noise RMS", s.noise_rms_mv), ("bandwidth", s.bandwidth_ghz),
        ("de-emphasis", s.deemph_db),
    ] {
        if !value.is_finite() {
            return Err(format!("{name} must be finite"));
        }
    }
    let rate = symbol_rate_hz(s);
    if !rate.is_finite() || rate <= 0.0 {
        return Err("data rate must be positive and finite".into());
    }
    if s.v_high_mv <= s.v_low_mv || s.v_high_mv.abs().max(s.v_low_mv.abs()) * 1e-3 > f32::MAX as f64 {
        return Err("high level must exceed low level, within finite sample range".into());
    }
    if s.rise_time_ps < 0.0 || s.noise_rms_mv < 0.0 || s.bandwidth_ghz < 0.0 || s.deemph_db < 0.0 {
        return Err("rise time, noise, bandwidth and de-emphasis must be nonnegative".into());
    }
    if !matches!(s.prbs_order, 7 | 9 | 13 | 15 | 23 | 31) {
        return Err(format!("unsupported PRBS order {}", s.prbs_order));
    }
    if s.filter_order > 32 || (s.bandwidth_ghz > 0.0 && s.filter_order == 0) {
        return Err("Butterworth order must be between 1 and 32 when enabled".into());
    }
    if let Some(ssc) = &s.ssc {
        if !ssc.deviation_percent.is_finite() || !(0.0..100.0).contains(&ssc.deviation_percent) {
            return Err("SSC deviation must be finite and in [0, 100) percent".into());
        }
        if !ssc.freq_khz.is_finite() || ssc.freq_khz <= 0.0 || ssc.freq_khz * 1e3 >= rate / 2.0 {
            return Err("SSC frequency must be positive and below half the symbol rate".into());
        }
        if !matches!(ssc.profile, "Down-spread only" | "Center-spread") {
            return Err("SSC profile must be Down-spread only or Center-spread".into());
        }
    }
    for jitter in &s.jitter {
        let (amplitude, frequency, name) = match *jitter {
            Jitter::Rj { ps_rms } => (ps_rms, None, "random jitter"),
            Jitter::Pj { ps_peak, freq_khz } => (ps_peak, Some(freq_khz), "periodic jitter"),
            Jitter::Dcd { ps } => (ps, None, "duty-cycle distortion"),
            Jitter::CorrRj { ps_rms, corner_khz } => (ps_rms, Some(corner_khz), "correlated jitter"),
        };
        if !amplitude.is_finite() || amplitude < 0.0 {
            return Err(format!("{name} must be finite and nonnegative"));
        }
        if let Some(f) = frequency {
            if !f.is_finite() || f < 0.0 || (amplitude > 0.0 && (f == 0.0 || f * 1e3 >= rate / 2.0)) {
                return Err(format!("{name} frequency must be positive and below half the symbol rate when enabled"));
            }
        }
    }
    Ok(())
}

/// Validate predictable render errors without allocating the record or PRBS.
fn validate_render_grid(s: &Settings, dt: f64, n: usize) -> Result<usize, String> {
    validate_settings(s)?;
    if !dt.is_finite() || dt <= 0.0 {
        return Err("sample interval must be positive and finite".into());
    }
    if n > MAX_SAMPLES {
        return Err(format!("record exceeds the {MAX_SAMPLES}-sample limit"));
    }
    if n == 0 {
        return Ok(0);
    }
    let symbol_rate = symbol_rate_hz(s);
    // NRZ requires at least 2.5 samples per line symbol.
    let sample_rate = 1.0 / dt;
    if !sample_rate.is_finite() || symbol_rate > sample_rate / 2.5 {
        return Err("data rate is above the render grid".into());
    }
    let fc = s.bandwidth_ghz * 1e9;
    if !fc.is_finite() || fc >= 0.5 / dt {
        return Err("Butterworth cutoff is at or above the Nyquist frequency".into());
    }
    let max_spread = s.ssc.as_ref().map_or(0.0, |ssc| {
        ssc.deviation_percent / if ssc.profile == "Down-spread only" { 200.0 } else { 100.0 }
    });
    // This conservative margin also covers a requested partial SSC period on an
    // arbitrary external grid, rather than assuming the usual full-cycle grid.
    let jitter_margin: f64 = s.jitter.iter().map(|j| match *j {
        Jitter::Rj { ps_rms } | Jitter::CorrRj { ps_rms, .. } => 10.0 * ps_rms * 1e-12,
        Jitter::Pj { ps_peak, .. } => ps_peak * 1e-12,
        Jitter::Dcd { ps } => 0.5 * ps * 1e-12,
    }).sum();
    let symbols = ((n as f64 * dt + jitter_margin) * symbol_rate / (1.0 - max_spread)).ceil() + 8.0;
    if !symbols.is_finite() || symbols > MAX_SAMPLES as f64 {
        return Err("serial edge grid exceeds the bounded allocation limit".into());
    }
    let rise = s.rise_time_ps * 1e-12;
    let shaping_window = if rise == 0.0 { 0.0 } else {
        (8.0 * rise / (2.0 * 0.6f64.atanh()) / dt + 2.0).min(n as f64)
    };
    // Every symbol is conservatively counted as a potential transition. This
    // makes the CPU guard allocation-free and consistent for UI/SCPI preflight.
    if !shaping_window.is_finite() || symbols * shaping_window > MAX_SHAPING_WORK {
        return Err("edge shaping exceeds the work limit; reduce rise time, data rate or record duration".into());
    }
    Ok(symbols as usize)
}

/// Volts, length `n`, interval `dt`.
pub fn generate(s: &Settings, dt: f64, n: usize) -> Result<Vec<f32>, String> {
    let nsym = validate_render_grid(s, dt, n)?;
    if n == 0 {
        return Ok(Vec::new());
    }
    let symbol_rate = symbol_rate_hz(s);
    let fc = s.bandwidth_ghz * 1e9;
    let v_high = s.v_high_mv * 1e-3;
    let v_low = s.v_low_mv * 1e-3;
    let bits = prbs_bits(s.prbs_order, nsym)?;
    let mut levels = Vec::with_capacity(nsym);
    for &b in &bits {
        levels.push(if b == 0 { v_low } else { v_high });
    }
    if s.deemph_db > 0.0 {
        let attenuation = 10f64.powf(-s.deemph_db / 20.0);
        let midpoint = 0.5 * (v_high + v_low);
        for i in 1..levels.len() {
            if bits[i] == bits[i - 1] {
                levels[i] = midpoint + (levels[i] - midpoint) * attenuation;
            }
        }
    }
    let rise = s.rise_time_ps * 1e-12;
    let mut edges = match &s.ssc {
        Some(ssc) => ssc_edge_times(nsym, symbol_rate, ssc.deviation_percent / 100.0, ssc.freq_khz * 1e3, ssc.profile),
        None => (0..nsym).map(|i| i as f64 / symbol_rate).collect(),
    };
    let mut rng = Rng::new(s.seed);
    apply_jitter(&mut edges, &bits, &s.jitter, symbol_rate, &mut rng);
    if edges.iter().any(|t| !t.is_finite()) || edges.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err("jitter reorders serial edges; reduce jitter amplitude".into());
    }
    if edges.last().copied().unwrap_or(0.0) < (n - 1) as f64 * dt {
        return Err("jitter moves serial edges outside the render grid".into());
    }
    let mut wave = render(&edges, &levels, dt, n, rise, v_low, v_high);
    if fc > 0.0 {
        wave = butterworth_lowpass(s.filter_order, fc, dt, &wave);
    }
    let sigma = s.noise_rms_mv * 1e-3;
    if sigma > 0.0 {
        for x in &mut wave {
            *x += rng.normal(sigma);
        }
    }
    let result: Vec<f32> = wave.into_iter().map(|v| v as f32).collect();
    if result.iter().any(|v| !v.is_finite()) {
        return Err("serial settings exceed the finite sample range".into());
    }
    Ok(result)
}

/// Vertical scale so the preset swing fills about six of the eight divisions, and the
/// offset that puts the midpoint on the centre line. `draw` uses `(v + offset) / vpd`.
pub fn vertical(s: &Settings) -> (f32, f32) {
    let hi = s.v_high_mv * 1e-3;
    let lo = s.v_low_mv * 1e-3;
    let vpd = ((hi - lo) / 6.0) as f32;
    let offset = (-0.5 * (hi + lo)) as f32;
    (vpd, offset)
}

/// Fibonacci LFSR. Output is the LSB, the register shifts right, feedback enters at the MSB.
/// Seed is all ones. Taps are bit offsets from the LSB.
///
/// Polynomials, as the Python class states them:
/// PRBS7 `1 + x^6 + x^7` taps (0, 1),
/// PRBS9 `1 + x^5 + x^9` taps (0, 4),
/// PRBS13 `1 + x + x^2 + x^12 + x^13` taps (0, 1, 11, 12),
/// PRBS15 `1 + x^14 + x^15` taps (0, 1),
/// PRBS23 `1 + x^18 + x^23` taps (0, 5),
/// PRBS31 `1 + x^28 + x^31` taps (0, 3).
pub fn prbs_bits(order: u32, n: usize) -> Result<Vec<u8>, String> {
    let taps: &[u32] = match order {
        7 => &[0, 1],
        9 => &[0, 4],
        13 => &[0, 1, 11, 12],
        15 => &[0, 1],
        23 => &[0, 5],
        31 => &[0, 3],
        _ => return Err(format!("unsupported PRBS order {order}")),
    };
    if order >= 64 {
        return Err("PRBS order does not fit a register".into());
    }
    let mask = (1u64 << order) - 1;
    let mut state = mask;
    let mut bits = vec![0u8; n];
    for b in &mut bits {
        *b = (state & 1) as u8;
        let mut feedback = 0u64;
        for &t in taps {
            feedback ^= (state >> t) & 1;
        }
        state = ((state >> 1) | (feedback << (order - 1))) & mask;
    }
    Ok(bits)
}

/// `_ssc_edge_times`. `deviation` is a fraction (0.005 = 0.5%). `f_mod` is hertz.
/// Down-spread removes the DC term before the small-signal period error is summed.
/// The first edge is forced to t = 0.
pub fn ssc_edge_times(num_symbols: usize, symbol_rate: f64, deviation: f64, f_mod: f64, profile: &str) -> Vec<f64> {
    if num_symbols == 0 {
        return Vec::new();
    }
    let f_mod = f_mod.max(1e-12);
    let symbol_rate = symbol_rate.max(1e-12);
    let deviation = deviation.max(0.0);
    let mut acc = 0.0;
    let mut edges = Vec::with_capacity(num_symbols);
    for i in 0..num_symbols {
        let t_nom = i as f64 / symbol_rate;
        let phase = (t_nom * f_mod) % 1.0;
        let tri01 = 1.0 - (2.0 * phase - 1.0).abs();
        let frac_dev = if profile == "Down-spread only" {
            -deviation * (tri01 - 0.5)
        } else {
            deviation * (2.0 * tri01 - 1.0)
        };
        let delta_t_step = -frac_dev / symbol_rate;
        acc += delta_t_step;
        edges.push(t_nom + acc);
    }
    let e0 = edges[0];
    for e in &mut edges {
        *e -= e0;
    }
    edges
}

/// `_apply_jitter`. PJ, RJ, DCD and correlated RJ follow the quoted function.
/// Data-dependent jitter needs `_compute_run_lengths`, which is not in the extract, so DDJ is not applied.
fn apply_jitter(et: &mut [f64], symbols: &[u8], comps: &[Jitter], symbol_rate: f64, rng: &mut Rng) {
    for j in comps {
        if let Jitter::Pj { freq_khz, ps_peak } = *j {
            let freq = freq_khz * 1e3;
            let amp = ps_peak * 1e-12;
            if freq > 0.0 && amp > 0.0 {
                for t in et.iter_mut() {
                    *t += amp * (2.0 * std::f64::consts::PI * freq * *t).sin();
                }
            }
        }
    }
    for j in comps {
        if let Jitter::Rj { ps_rms } = *j {
            let sigma = ps_rms * 1e-12;
            if sigma > 0.0 {
                for t in et.iter_mut() {
                    *t += rng.normal(sigma);
                }
            }
        }
    }
    for j in comps {
        if let Jitter::Dcd { ps } = *j {
            let amp = ps * 1e-12;
            if amp > 0.0 && symbols.len() >= et.len() {
                let half = amp / 2.0;
                for n in 1..et.len() {
                    if symbols[n] > symbols[n - 1] {
                        et[n] += half;
                    } else if symbols[n] < symbols[n - 1] {
                        et[n] -= half;
                    }
                }
            }
        }
    }
    for j in comps {
        if let Jitter::CorrRj { ps_rms, corner_khz } = *j {
            let sigma = ps_rms * 1e-12;
            if sigma > 0.0 && et.len() > 1 {
                // The pole is chosen for a digital -3 dB corner at the requested
                // physical frequency. The old normalized-Nyquist formula doubled
                // the corner and silently clamped low frequencies.
                let a = correlated_alpha(corner_khz * 1e3, symbol_rate);
                let mut prev = 0.0;
                let filtered: Vec<f64> = (0..et.len()).map(|_| {
                    prev += a * (rng.normal(1.0) - prev);
                    prev
                }).collect();
                let mean = filtered.iter().sum::<f64>() / filtered.len() as f64;
                let var = filtered.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / filtered.len() as f64;
                let rms = var.sqrt();
                if rms > 0.0 {
                    let scale = sigma / rms;
                    for (t, f) in et.iter_mut().zip(filtered) {
                        *t += (f - mean) * scale;
                    }
                }
            }
        }
    }
}

/// One-pole coefficient with |H(exp(j*2*pi*corner/rate))|^2 = 1/2.
fn correlated_alpha(corner: f64, rate: f64) -> f64 {
    let sine = (std::f64::consts::PI * corner / rate).sin();
    let q = 2.0 * sine * sine;
    (q * (q + 2.0)).sqrt() - q
}

/// Staircase at the exact edge time, then `dV * (tanh_step - ideal_step)` within ±4 tau.
/// `tau = rise / (2 * atanh(0.6))` so an isolated edge is the requested 20–80% rise.
fn render(edges: &[f64], levels: &[f64], dt: f64, n: usize, rise: f64, v_low: f64, v_high: f64) -> Vec<f64> {
    let mut wave = vec![levels.first().copied().unwrap_or(0.0); n];
    if edges.is_empty() || levels.is_empty() {
        return wave;
    }
    let mut e = 0usize;
    for i in 0..n {
        let t = i as f64 * dt;
        while e + 1 < edges.len() && edges[e + 1] <= t {
            e += 1;
        }
        while e > 0 && edges[e] > t {
            e -= 1;
        }
        wave[i] = levels[e.min(levels.len() - 1)];
    }
    if rise > 0.0 {
        let tau = rise / (2.0 * 0.6f64.atanh());
        let window = 4.0 * tau;
        for k in 1..edges.len().min(levels.len()) {
            let dv = levels[k] - levels[k - 1];
            if dv == 0.0 {
                continue;
            }
            let te = edges[k];
            let i0 = ((te - window) / dt).floor().max(0.0) as usize;
            if i0 >= n {
                continue;
            }
            let i1 = (((te + window) / dt).ceil() as usize).saturating_add(1).min(n);
            for i in i0..i1 {
                let t = i as f64 * dt;
                let x = (t - te) / tau;
                let tanh_step = 0.5 * (1.0 + x.tanh());
                let ideal = if t >= te { 1.0 } else { 0.0 };
                wave[i] += dv * (tanh_step - ideal);
            }
        }
    }
    let (lo, hi) = (v_low.min(v_high), v_low.max(v_high));
    for v in &mut wave {
        *v = v.clamp(lo, hi);
    }
    wave
}

/// Butterworth low-pass, order `order`, cutoff `fc` hertz, forward and back on each
/// section so the slope is that of twice the order, and the amplitude at `fc`
/// is one half (-6 dB), rather than the single-pass -3 dB.
fn butterworth_lowpass(order: usize, fc: f64, dt: f64, x: &[f64]) -> Vec<f64> {
    if x.is_empty() || order == 0 || fc <= 0.0 {
        return x.to_vec();
    }
    let omega = (std::f64::consts::PI * fc * dt).tan();
    let alpha = 1.0 / omega;
    let mut y = x.to_vec();
    for k in 0..order {
        let theta = std::f64::consts::PI * (2 * k + 1) as f64 / (2 * order) as f64;
        let re = -theta.sin();
        let im = theta.cos();
        if im < -1e-9 {
            continue;
        }
        if im.abs() < 1e-9 {
            let b0 = -re / (alpha - re);
            let a1 = (-alpha - re) / (alpha - re);
            filtfilt([b0, b0, 0.0, a1, 0.0], &mut y);
        } else {
            let r2 = re * re + im * im;
            let a0 = alpha * alpha - 2.0 * re * alpha + r2;
            let az1 = -2.0 * alpha * alpha + 2.0 * r2;
            let az2 = alpha * alpha + 2.0 * re * alpha + r2;
            filtfilt([r2 / a0, 2.0 * r2 / a0, r2 / a0, az1 / a0, az2 / a0], &mut y);
        }
    }
    y
}

/// Biquad forward then backward. Coefficients are `(b0, b1, b2, a1, a2)` with `a0 = 1`.
fn filtfilt(k: [f64; 5], x: &mut [f64]) {
    let run = |x: &mut [f64]| {
        let x0 = x.first().copied().unwrap_or(0.0);
        let g = (k[0] + k[1] + k[2]) / (1.0 + k[3] + k[4]);
        let (mut x1, mut x2, mut y1, mut y2) = (x0, x0, x0 * g, x0 * g);
        for v in x.iter_mut() {
            let y = k[0] * *v + k[1] * x1 + k[2] * x2 - k[3] * y1 - k[4] * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, y);
            *v = y;
        }
    };
    run(x);
    x.reverse();
    run(x);
    x.reverse();
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn uniform(&mut self) -> f64 {
        let x = (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64);
        x.clamp(1e-12, 1.0 - 1e-12)
    }
    fn normal(&mut self, sigma: f64) -> f64 {
        let u1 = self.uniform();
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos() * sigma
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prbs7_is_maximal_and_prbs31_taps_match_the_polynomial() {
        let b = prbs_bits(7, 127).unwrap();
        assert_eq!(b[0], 1);
        assert_eq!(prbs_bits(7, 128).unwrap()[127], b[0]);
        let mut seen = std::collections::HashSet::new();
        let mut state = (1u64 << 7) - 1;
        for _ in 0..127 {
            assert!(seen.insert(state));
            let feedback = ((state >> 0) & 1) ^ ((state >> 1) & 1);
            state = ((state >> 1) | (feedback << 6)) & 127;
        }
        let bits = prbs_bits(31, 4).unwrap();
        assert_eq!(bits, vec![1, 1, 1, 1]);
        let state = (1u64 << 31) - 1;
        let feedback = ((state >> 0) & 1) ^ ((state >> 3) & 1);
        assert_eq!(feedback, 0);
    }

    #[test]
    fn ssc_matches_the_small_signal_ramp() {
        let symbol_rate = 1.0e9;
        let edges = ssc_edge_times(4, symbol_rate, 0.005, 33_000.0, "Down-spread only");
        assert_eq!(edges[0], 0.0);
        let mut acc = 0.0;
        let mut expect = Vec::new();
        for i in 0..4 {
            let t_nom = i as f64 / symbol_rate;
            let phase = (t_nom * 33_000.0) % 1.0;
            let tri01 = 1.0 - (2.0 * phase - 1.0).abs();
            let frac = -0.005 * (tri01 - 0.5);
            acc += -frac / symbol_rate;
            expect.push(t_nom + acc);
        }
        let e0 = expect[0];
        for (g, e) in edges.iter().zip(expect) {
            assert!((g - (e - e0)).abs() < 1e-18, "{g} vs {}", e - e0);
        }
        assert!(edges[1] > 0.0 && edges[1] < 1.0e-9);
    }

    #[test]
    fn periodic_jitter_is_the_sine_on_the_edge() {
        let mut et = vec![0.0, 1.0e-9, 2.0e-9];
        let symbols = vec![0u8, 1, 0];
        let mut rng = Rng::new(1);
        apply_jitter(&mut et, &symbols, &[Jitter::Pj { freq_khz: 8000.0, ps_peak: 4.0 }], 1.0e9, &mut rng);
        let amp = 4.0e-12;
        let f = 8.0e6;
        assert_eq!(et[0], 0.0);
        let t1 = 1.0e-9;
        let shifted = t1 + amp * (2.0 * std::f64::consts::PI * f * t1).sin();
        assert!((et[1] - shifted).abs() < 1e-20);
    }

    #[test]
    fn full_cycle_ssc_has_the_requested_span_and_integrated_triangle() {
        let rate = 1e6;
        let hz = 1e3;
        let depth = 0.005;
        let ui = 1.0 / rate;
        let period = 1.0 / hz;
        for (profile, span, quarter_tie) in [
            ("Down-spread only", depth, -depth * period / 16.0),
            ("Center-spread", 2.0 * depth, depth * period / 8.0),
        ] {
            let edges = ssc_edge_times(1001, rate, depth, hz, profile);
            let periods: Vec<f64> = edges.windows(2).map(|p| p[1] - p[0]).collect();
            let low = periods.iter().copied().fold(f64::INFINITY, f64::min);
            let high = periods.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            assert!(((high - low) / ui - span).abs() < 1e-9, "{profile}: span {}", (high - low) / ui);
            assert!((edges[1000] - period).abs() < 1e-15, "{profile}: DC term");
            // The integral of the zero-mean triangle at one quarter period is
            // -depth*T/16 for source-compatible downspread, +depth*T/8 for center.
            let tie = edges[250] - 250.0 * ui;
            assert!((tie - quarter_tie).abs() <= depth * ui, "{profile}: {tie} expected {quarter_tie}");
            assert!(periods.iter().all(|p| *p > 0.0));
        }
    }

    #[test]
    fn dcd_changes_high_and_low_widths_by_the_requested_peak_to_peak_time() {
        let mut edges = vec![0.0, 1e-9, 2e-9, 3e-9, 4e-9];
        let bits = [0, 1, 0, 1, 0];
        let mut rng = Rng::new(1);
        apply_jitter(&mut edges, &bits, &[Jitter::Dcd { ps: 40.0 }], 1e9, &mut rng);
        let levels = [-0.5, 0.5, -0.5, 0.5, -0.5];
        let dt = 1e-12;
        let wave = render(&edges, &levels, dt, 4500, 10e-12, -0.5, 0.5);
        let crossings: Vec<f64> = wave.windows(2).enumerate().filter_map(|(i, p)| {
            ((p[0] < 0.0 && p[1] >= 0.0) || (p[0] > 0.0 && p[1] <= 0.0))
                .then(|| (i as f64 - p[0] / (p[1] - p[0])) * dt)
        }).collect();
        assert_eq!(crossings.len(), 4);
        assert!((crossings[1] - crossings[0] - 960e-12).abs() < dt);
        assert!((crossings[2] - crossings[1] - 1040e-12).abs() < dt);
        assert!((crossings[3] - crossings[1] - 2e-9).abs() < dt);
    }

    #[test]
    fn correlated_rj_has_zero_mean_requested_rms_and_a_physical_corner() {
        let n = 262_144;
        let rate = 1e9;
        let corner = rate / 32.0;
        let sigma = 2e-12;
        let mut edges: Vec<f64> = (0..n).map(|i| i as f64 / rate).collect();
        let mut rng = Rng::new(9);
        apply_jitter(&mut edges, &vec![0; n], &[Jitter::CorrRj { ps_rms: sigma * 1e12, corner_khz: corner / 1e3 }], rate, &mut rng);
        let jitter: Vec<f64> = edges.iter().enumerate().map(|(i, t)| t - i as f64 / rate).collect();
        let mean = jitter.iter().sum::<f64>() / n as f64;
        let variance = jitter.iter().map(|x| x * x).sum::<f64>() / n as f64;
        assert!(mean.abs() < sigma * 1e-6, "mean {mean}");
        assert!((variance.sqrt() / sigma - 1.0).abs() < 1e-6);
        let correlation = jitter.windows(2).map(|p| p[0] * p[1]).sum::<f64>() / (n - 1) as f64 / variance;
        assert!((correlation - 0.82224).abs() < 0.01, "lag-1 correlation {correlation}");
        // Welch power at 31.25 MHz should be half its low-frequency plateau.
        // This observes the generated noise, rather than checking the coefficient
        // formula, and distinguishes the old approximately doubled corner.
        let power = |bins: &[usize]| -> f64 {
            let mut sum = 0.0;
            for block in jitter.chunks_exact(1024) {
                for &bin in bins {
                    let mut real = 0.0;
                    let mut imag = 0.0;
                    for (i, x) in block.iter().enumerate() {
                        let angle = 2.0 * std::f64::consts::PI * bin as f64 * i as f64 / 1024.0;
                        real += x * angle.cos();
                        imag += x * angle.sin();
                    }
                    sum += real * real + imag * imag;
                }
            }
            sum / bins.len() as f64
        };
        let ratio = power(&[31, 32, 33]) / power(&[3, 4, 5]);
        assert!((0.40..0.63).contains(&ratio), "corner/plateau power {ratio}");
    }

    #[test]
    fn de_emphasis_keeps_the_midpoint_of_offset_rails() {
        let mut settings = pcie3_nominal();
        settings.ssc = None;
        settings.jitter.clear();
        settings.noise_rms_mv = 0.0;
        settings.bandwidth_ghz = 0.0;
        settings.rise_time_ps = 0.0;
        settings.v_low_mv = 100.0;
        settings.v_high_mv = 1100.0;
        settings.deemph_db = 20.0 * 2.0f64.log10();
        let dt = 1.0 / (8.0 * symbol_rate_hz(&settings));
        let wave = generate(&settings, dt, 4096).unwrap();
        // A 6.02 dB repeated symbol halves its excursion about 600 mV.
        for expected in [0.1f32, 0.35, 0.85, 1.1] {
            assert!(wave.iter().any(|x| (*x - expected).abs() < 1e-6), "missing {expected}");
        }
        assert!(wave.iter().all(|x| [0.1f32, 0.35, 0.85, 1.1].iter().any(|expected| (*x - *expected).abs() < 1e-6)));
    }

    #[test]
    fn butterworth_passes_dc_and_cuts_a_high_tone() {
        let dt = 1.0e-8;
        let n = 4000;
        let dc = vec![1.0; n];
        let out = butterworth_lowpass(3, 1.0e6, dt, &dc);
        assert!((out[n / 2] - 1.0).abs() < 1e-3);
        let fc = 1.0e6;
        let tone = |f: f64| -> f64 {
            let x: Vec<f64> = (0..n).map(|i| (2.0 * std::f64::consts::PI * f * i as f64 * dt).sin()).collect();
            let y = butterworth_lowpass(3, fc, dt, &x);
            let a = n / 4;
            let b = 3 * n / 4;
            let rms = |z: &[f64]| (z.iter().map(|v| v * v).sum::<f64>() / z.len() as f64).sqrt();
            rms(&y[a..b]) / rms(&x[a..b])
        };
        let low = tone(fc / 20.0);
        assert!(low > 0.95, "{low}");
        let high = tone(fc * 5.0);
        assert!(high < 1.0e-3, "{high}");
    }

    #[test]
    fn pcie3_grid_is_one_full_ssc_period() {
        let s = pcie3_nominal();
        let (dt, n) = sample_grid(&s);
        let symbol = 8.0e9;
        assert!((dt - 1.0 / (8.0 * symbol)).abs() < 1e-24);
        let period = 1.0 / 33.0e3;
        assert!(n as f64 * dt + 1e-15 >= period);
        assert!(n > PLAIN_LEN);
        let (vpd, offset) = vertical(&s);
        assert!((vpd as f64 - 1.0 / 6.0).abs() < 1e-6);
        assert_eq!(offset, 0.0);
    }

    #[test]
    fn pcie3_record_is_deterministic_and_in_volts() {
        let s = pcie3_nominal();
        let (dt, n) = sample_grid(&s);
        let a = generate(&s, dt, n).unwrap();
        let b = generate(&s, dt, n).unwrap();
        assert_eq!(a.len(), n);
        assert_eq!(a, b);
        assert!(a.iter().all(|v| v.is_finite()));
        let (min, max) = a.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        assert!(max > 0.35 && max < 0.7, "{max}");
        assert!(min < -0.35 && min > -0.7, "{min}");
        let period = 1.0 / 33.0e3;
        assert!(n as f64 * dt + 1e-15 >= period);
    }
}
