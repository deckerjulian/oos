// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! High-speed serial waveform from HSS Simulator v1.12.
//!
//! The PRBS taps, the spread-spectrum edge times, and the jitter steps are the
//! functions in `waveform_engine.py` (`PRBSGenerator`, `_ssc_edge_times`,
//! `_apply_jitter`). PCIe 3.0 Nominal is the JSON in `Presets/PCIe_3.0_Nominal.json`.
//! The engine default Butterworth order is 3 because that file omits `order`.
//! Samples are volts. The Python engine returns millivolts.

/// Samples per line symbol when the record is rendered at the preset rate.
pub const SAMPLES_PER_UI: usize = 8;

/// Length used when spread-spectrum clocking is off.
pub const PLAIN_LEN: usize = 65_536;

/// Deterministic stand-in for NumPy's process-wide RNG. The widths are the JSON
/// amplitudes. The same seed draws the same record.
pub const PCIE3_SEED: u64 = 0x5043_4933;

/// One nominal preset the menu can name. Disabled rows have no analog fields copied.
#[derive(Clone, Copy, Debug)]
pub struct PresetInfo {
    pub name: &'static str,
    pub enabled: bool,
}

/// First menu. Only PCIe 3.0 Nominal has its analog fields copied.
pub const MENU: &[PresetInfo] = &[
    PresetInfo { name: "PCIe 1.0", enabled: false },
    PresetInfo { name: "PCIe 2.0", enabled: false },
    PresetInfo { name: "PCIe 3.0", enabled: true },
    PresetInfo { name: "PCIe 4.0", enabled: false },
    PresetInfo { name: "PCIe 5.0", enabled: false },
    PresetInfo { name: "USB 2.0 HS", enabled: false },
    PresetInfo { name: "USB 3.2 Gen 1", enabled: false },
    PresetInfo { name: "USB 3.2 Gen 2", enabled: false },
    PresetInfo { name: "SATA I", enabled: false },
    PresetInfo { name: "SATA II", enabled: false },
    PresetInfo { name: "SATA III", enabled: false },
    PresetInfo { name: "10GBASE-KR", enabled: false },
    PresetInfo { name: "100BASE-TX", enabled: false },
];

/// Index of PCIe 3.0 in [`MENU`].
pub const PCIE3_INDEX: usize = 2;

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
    /// Correlated RJ: white Gaussian, first-order low-pass, then rescaled to picoseconds RMS.
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
    let fs = symbol_rate_hz(s);
    let dt = 1.0 / (SAMPLES_PER_UI as f64 * fs);
    let n = match &s.ssc {
        Some(ssc) if ssc.freq_khz > 0.0 => {
            let period = 1.0 / (ssc.freq_khz * 1e3);
            (period / dt).ceil() as usize
        }
        _ => PLAIN_LEN,
    };
    (dt, n.max(SAMPLES_PER_UI))
}

/// Volts, length `n`, interval `dt`.
pub fn generate(s: &Settings, dt: f64, n: usize) -> Result<Vec<f32>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    if !(dt > 0.0) {
        return Err("sample interval must be positive".into());
    }
    let symbol_rate = symbol_rate_hz(s);
    if !(symbol_rate > 0.0) {
        return Err("data rate must be positive".into());
    }
    // The engine refuses a data rate above sample_rate * bits_per_symbol / (2.5 * line_multiplier).
    // NRZ: both multipliers are 1.
    let sample_rate = 1.0 / dt;
    if symbol_rate > sample_rate / 2.5 {
        return Err("data rate is above the render grid".into());
    }
    let v_high = s.v_high_mv * 1e-3;
    let v_low = s.v_low_mv * 1e-3;
    // One extra symbol past the record, plus a margin for the bounded SSC time slip.
    let mut nsym = ((n as f64) * dt * symbol_rate).ceil() as usize + 8;
    if s.ssc.is_some() {
        nsym += ((0.02 * nsym as f64) as usize).max(64);
    }
    let bits = prbs_bits(s.prbs_order, nsym)?;
    let mut levels = Vec::with_capacity(nsym);
    for &b in &bits {
        levels.push(if b == 0 { v_low } else { v_high });
    }
    if s.deemph_db > 0.0 {
        let factor = 10f64.powf(s.deemph_db / 20.0);
        let ideal = levels.clone();
        for i in 1..levels.len() {
            if ideal[i] == ideal[i - 1] {
                levels[i] /= factor;
            }
        }
    }
    let mut edges = match &s.ssc {
        Some(ssc) => ssc_edge_times(nsym, symbol_rate, ssc.deviation_percent / 100.0, ssc.freq_khz * 1e3, ssc.profile),
        None => (0..nsym).map(|i| i as f64 / symbol_rate).collect(),
    };
    let mut rng = Rng::new(s.seed);
    apply_jitter(&mut edges, &bits, &s.jitter, symbol_rate, &mut rng);
    let rise = s.rise_time_ps * 1e-12;
    let mut wave = render(&edges, &levels, dt, n, rise, v_low, v_high);
    if s.bandwidth_ghz > 0.0 {
        let fc = s.bandwidth_ghz * 1e9;
        let nyquist = 0.5 / dt;
        if fc >= nyquist {
            return Err("Butterworth cutoff is at or above the Nyquist frequency".into());
        }
        if s.filter != FilterKind::Butterworth {
            return Err("only a Butterworth limit is implemented".into());
        }
        wave = butterworth_lowpass(s.filter_order, fc, dt, &wave);
    }
    let sigma = s.noise_rms_mv * 1e-3;
    if sigma > 0.0 {
        for x in &mut wave {
            *x += rng.normal(sigma);
        }
    }
    Ok(wave.into_iter().map(|v| v as f32).collect())
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
                let white: Vec<f64> = (0..et.len()).map(|_| rng.normal(1.0)).collect();
                let filtered = if symbol_rate > 0.0 && corner_khz > 0.0 {
                    let fc_hz = corner_khz * 1e3;
                    let fc_norm = (fc_hz / (symbol_rate / 2.0)).clamp(1e-6, 0.9999);
                    let a = 2.0 * std::f64::consts::PI * fc_norm / (1.0 + 2.0 * std::f64::consts::PI * fc_norm);
                    let mut y = vec![0.0; white.len()];
                    let mut prev = 0.0;
                    for (i, &x) in white.iter().enumerate() {
                        let yi = a * x + (1.0 - a) * prev;
                        y[i] = yi;
                        prev = yi;
                    }
                    y
                } else {
                    white
                };
                let mean = filtered.iter().sum::<f64>() / filtered.len() as f64;
                let var = filtered.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / filtered.len() as f64;
                let rms = var.sqrt();
                if rms > 0.0 {
                    let scale = sigma / rms;
                    for (t, f) in et.iter_mut().zip(filtered) {
                        *t += f * scale;
                    }
                }
            }
        }
    }
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
            let i1 = (((te + window) / dt).ceil() as usize + 1).min(n);
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
/// second-order section so the slope is that of twice the order.
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
            y = filtfilt([b0, b0, 0.0, a1, 0.0], &y);
        } else {
            let r2 = re * re + im * im;
            let a0 = alpha * alpha - 2.0 * re * alpha + r2;
            let az1 = -2.0 * alpha * alpha + 2.0 * r2;
            let az2 = alpha * alpha + 2.0 * re * alpha + r2;
            y = filtfilt([r2 / a0, 2.0 * r2 / a0, r2 / a0, az1 / a0, az2 / a0], &y);
        }
    }
    y
}

/// Biquad forward then backward. Coefficients are `(b0, b1, b2, a1, a2)` with `a0 = 1`.
fn filtfilt(k: [f64; 5], a: &[f64]) -> Vec<f64> {
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
    let mut x = a.to_vec();
    run(&mut x);
    x.reverse();
    run(&mut x);
    x.reverse();
    x
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
