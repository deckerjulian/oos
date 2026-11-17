// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Jitter, eye and common mode of serial data: one recovered clock and eye diagram per source give the jitter
//! decomposition, the eye opening, the bit levels and the spread-spectrum figures; two sources give the common mode.

use super::*;
use Mark::*;

pub const JITTER_SUMMARY: u16 = 40;
pub const TIE: u16 = 41;
pub const PHASE_NOISE: u16 = 42;
pub const TJ_BER: u16 = 43;
pub const RJ_DD: u16 = 44;
pub const DJ_DD: u16 = 45;
pub const SJ_FREQ: u16 = 46;
pub const PJ: u16 = 47;
pub const DDJ: u16 = 48;
pub const DCD: u16 = 49;
pub const SRJ: u16 = 50;
pub const J2: u16 = 51;
pub const EYE_HEIGHT: u16 = 60;
pub const EYE_WIDTH: u16 = 61;
pub const EYE_HIGH: u16 = 62;
pub const EYE_HEIGHT_BER: u16 = 63;
pub const EYE_WIDTH_BER: u16 = 64;
pub const EYE_LOW: u16 = 65;
pub const Q_FACTOR: u16 = 66;
pub const BIT_HIGH: u16 = 70;
pub const BIT_LOW: u16 = 71;
pub const BIT_AMPLITUDE: u16 = 72;
pub const DC_COMMON: u16 = 73;
pub const AC_COMMON: u16 = 74;
pub const DIFF_CROSS: u16 = 75;
pub const TNT: u16 = 76;
pub const BIT_RATE: u16 = 80;
pub const SSC_DEV: u16 = 81;
pub const SSC_RATE: u16 = 82;

static DEFS: [Def; 29] = [
    module(JITTER_SUMMARY, "Jitter Summary", "s", Eye, &[0], &[(JITTER, "JITTER")], "Total jitter at the selected bit-error rate."),
    module(TIE, "TIE", "s", Eye, &[0], &[(JITTER, "JITTER")], "Peak-to-peak time-interval error against the recovered clock."),
    module(PHASE_NOISE, "Phase Noise", "s", Bars, &[0], &[(JITTER, "JITTER")], "RMS jitter integrated between the two offset frequencies."),
    module(TJ_BER, "TJ@BER", "s", Eye, &[0], &[(JITTER, "JITTER")], "Total jitter extrapolated to the selected bit-error rate."),
    module(RJ_DD, "RJ-dd", "s", Eye, &[0], &[(JITTER, "JITTER")], "Random jitter from the dual-Dirac fit of the tails."),
    module(DJ_DD, "DJ-dd", "s", Eye, &[0], &[(JITTER, "JITTER")], "Deterministic jitter from the dual-Dirac fit of the tails."),
    module(SJ_FREQ, "SJ@Freq", "s", Bars, &[0], &[(JITTER, "JITTER")], "Largest periodic tone in the time-interval error."),
    module(PJ, "PJ", "s", Bars, &[0], &[(JITTER, "JITTER")], "Combined size of the periodic tones in the time-interval error."),
    module(DDJ, "DDJ", "s", Eye, &[0], &[(JITTER, "JITTER")], "Spread of the mean edge error across the run lengths before an edge."),
    module(DCD, "DCD", "s", Eye, &[0], &[(JITTER, "JITTER")], "Difference between the mean rising-edge error and the falling-edge error."),
    module(SRJ, "SRJ", "s", Bars, &[0], &[(JITTER, "JITTER")], "Peak-to-peak of the half-rate, quarter-rate and eighth-rate tones added together."),
    module(J2, "J2", "s", Eye, &[0], &[(JITTER, "JITTER")], "Total jitter at a bit-error rate of 2.5e-3."),
    module(EYE_HEIGHT, "Eye Height", "V", Eye, &[0], &[(JITTER, "EYE")], "Vertical opening inside three standard deviations of the two levels."),
    module(EYE_WIDTH, "Eye Width", "s", Eye, &[0], &[(JITTER, "EYE")], "Horizontal opening inside three standard deviations of the crossing."),
    module(EYE_HIGH, "Eye High", "V", Eye, &[0], &[(JITTER, "EYE")], "Upper edge of that three-sigma opening."),
    module(EYE_HEIGHT_BER, "Eye Height@BER", "V", Eye, &[0], &[(JITTER, "EYE")], "Vertical opening extrapolated to the selected bit-error rate."),
    module(EYE_WIDTH_BER, "Eye Width@BER", "s", Eye, &[0], &[(JITTER, "EYE")], "Time left in the unit interval after total jitter at that rate."),
    module(EYE_LOW, "Eye Low", "V", Eye, &[0], &[(JITTER, "EYE")], "Lower edge of the three-sigma opening."),
    module(Q_FACTOR, "Q-Factor", "", Eye, &[0], &[(JITTER, "EYE")], "Eye amplitude divided by the noise on the two levels."),
    module(BIT_HIGH, "Bit High", "V", Pulse, &[0], &[(JITTER, "AMPLITUDE")], "Mean level of a high bit in the middle of the eye."),
    module(BIT_LOW, "Bit Low", "V", Pulse, &[0], &[(JITTER, "AMPLITUDE")], "Mean level of a low bit in the middle of the eye."),
    module(BIT_AMPLITUDE, "Bit Amplitude", "V", Pulse, &[0], &[(JITTER, "AMPLITUDE")], "High bit level minus the low bit level."),
    module(DC_COMMON, "DC Common Mode", "V", Pulse, &[0, 1], &[(JITTER, "AMPLITUDE")], "Mean of the two sources added together and halved."),
    module(AC_COMMON, "AC Common Mode (Pk-Pk)", "V", Pulse, &[0, 1], &[(JITTER, "AMPLITUDE")], "Peak-to-peak of the common mode after the high-pass."),
    module(DIFF_CROSS, "Differential Crossover", "V", Pulse, &[0, 1], &[(JITTER, "AMPLITUDE")], "Mean voltage where the two sources cross."),
    module(TNT, "T/nT Ratio", "dB", Pulse, &[0], &[(JITTER, "AMPLITUDE")], "Mid-interval level of a steady bit against the last transition, in dB."),
    module(BIT_RATE, "NRZ Clock Rate", "b/s", Square, &[0], &[(JITTER, "TIMING")], "Rate of the recovered NRZ clock. Assumes one bit per symbol; not PAM or C-PHY throughput."),
    module(SSC_DEV, "SSC Freq Dev", "ppm", Slope, &[0], &[(JITTER, "TIMING")], "Peak-to-peak spread of the cycle frequency, in parts per million."),
    module(SSC_RATE, "SSC Modulation Rate", "Hz", Slope, &[0], &[(JITTER, "TIMING")], "How fast that frequency spread repeats."),
];

/// The measurements on two sources (common mode); the rest read one source through the eye.
const COMMON: [u16; 3] = [DC_COMMON, AC_COMMON, DIFF_CROSS];

pub struct Jitter;

impl MeasModule for Jitter {
    fn key(&self) -> &'static str {
        "jitter_eye"
    }

    fn defs(&self) -> &'static [Def] {
        &DEFS
    }

    /// A row without a value says why rather than showing dashes (the fork's catalogue did).
    fn missing_reason(&self, _id: u16) -> Option<&'static str> {
        Some("needs more edges")
    }

    fn settings(&self, m: &Model, v: &mut Vec<u64>) {
        for x in [m.eye_rate, m.eye_ber, m.eye_pn_lo, m.eye_pn_hi, m.eye_notch] {
            put(v, x);
        }
        v.extend([m.eye_hp as u64, m.eye_npj as u64]);
    }

    fn compute(&self, x: &Ctx, ids: &[u16]) -> Values {
        let nan = f64::NAN;
        let (a, b) = (x.row[0], x.row[1]);
        let m = x.model;
        let eye = ids.iter().any(|id| !COMMON.contains(id)).then(|| x.wave(a).and_then(|v| eye_of(v, x.dt, m.eye_ber, m.eye_pn_lo, m.eye_pn_hi, m.eye_rate))).flatten();
        let e = |f: fn(&EyeSnap) -> f64| eye.as_ref().map_or(nan, f);
        let values = ids
            .iter()
            .map(|&id| {
                let v = match id {
                    JITTER_SUMMARY | TJ_BER => e(|e| e.tj),
                    TIE => e(|e| e.tie_pp),
                    PHASE_NOISE => e(|e| e.pn),
                    RJ_DD => e(|e| e.rj),
                    DJ_DD => e(|e| e.dj),
                    SJ_FREQ => e(|e| e.sj),
                    PJ => e(|e| e.pj),
                    DDJ => e(|e| e.ddj),
                    DCD => e(|e| e.dcd),
                    SRJ => e(|e| e.srj),
                    J2 => e(|e| e.j2),
                    EYE_HEIGHT => e(|e| e.height),
                    EYE_WIDTH => e(|e| e.width),
                    EYE_HIGH => e(|e| e.one - 3.0 * e.s1),
                    EYE_HEIGHT_BER => e(|e| e.height_ber),
                    EYE_WIDTH_BER => e(|e| e.width_ber),
                    EYE_LOW => e(|e| e.zero + 3.0 * e.s0),
                    Q_FACTOR => e(|e| e.q),
                    BIT_HIGH => e(|e| e.one),
                    BIT_LOW => e(|e| e.zero),
                    BIT_AMPLITUDE => e(|e| e.amp),
                    TNT => e(|e| e.tnt),
                    BIT_RATE => e(|e| e.rate),
                    SSC_DEV => e(|e| e.ssc_ppm),
                    SSC_RATE => e(|e| e.ssc_hz),
                    DC_COMMON if a != b => x.align(&[a, b]).map_or(nan, |v| oos_dsp::eye::dc_common(v[0], v[1])),
                    AC_COMMON if a == b => x.wave(a).map_or(nan, |v| oos_dsp::eye::ac_common_pp(v, None, x.dt, m.eye_hp)),
                    AC_COMMON => x.align(&[a, b]).map_or(nan, |v| oos_dsp::eye::ac_common_pp(v[0], Some(v[1]), x.dt, m.eye_hp)),
                    DIFF_CROSS if a != b => x.align(&[a, b]).map_or(nan, |v| oos_dsp::eye::diff_crossover(v[0], v[1])),
                    _ => nan,
                };
                (id, v)
            })
            .collect();
        let quality = if ids.iter().all(|id| COMMON.contains(id)) {
            DERIVED
        } else {
            eye.as_ref().map_or("NRZ estimate: insufficient transitions or sampling", |e| e.quality)
        };
        Values { values, quality }
    }

    fn pending_quality(&self, id: u16) -> &'static str {
        if COMMON.contains(&id) {
            DERIVED
        } else {
            "NRZ estimate / jitter model"
        }
    }
}

/// What one source's recovered clock and eye give.
struct EyeSnap {
    quality: &'static str,
    rate: f64,
    tj: f64,
    tie_pp: f64,
    rj: f64,
    dj: f64,
    pj: f64,
    sj: f64,
    ddj: f64,
    dcd: f64,
    srj: f64,
    j2: f64,
    pn: f64,
    one: f64,
    zero: f64,
    amp: f64,
    height: f64,
    width: f64,
    height_ber: f64,
    width_ber: f64,
    q: f64,
    s1: f64,
    s0: f64,
    ssc_ppm: f64,
    ssc_hz: f64,
    tnt: f64,
}

fn eye_of(v: &[f32], dt: f64, ber: f64, pn_lo: f64, pn_hi: f64, rate: f64) -> Option<EyeSnap> {
    if v.len() < 16 || !(dt > 0.0) {
        return None;
    }
    let finite = v.iter().copied().filter(|x| x.is_finite());
    if finite.clone().take(16).count() < 16 {
        return None;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for x in finite {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if !(hi > lo) {
        return None;
    }
    let (summary, clk) = crate::ui::eye_infer(v, dt, (rate > 0.0).then_some(rate)).ok()?;
    let tie = oos_dsp::eye::tie(&clk.edges, &clk);
    let j = oos_dsp::eye::jitter(&tie);
    let mut means = oos_dsp::eye::EdgeMeans::default();
    means.add(&tie, &clk);
    let trend = oos_dsp::eye::tie_trend(&clk.edges, &tie, &clk);
    let (df, spec) = oos_dsp::eye::tie_spectrum(&trend, clk.period);
    let lines = oos_dsp::eye::pj_lines(df, &spec, 12);
    let pj = lines.iter().map(|(_, r)| r * r).sum::<f64>().sqrt();
    let sj = lines.first().map(|x| x.1).unwrap_or(f64::NAN);
    let pad = 0.15 * (hi - lo);
    let mut diagram = oos_dsp::eye::Eye::new(72, 40, 2, lo - pad, hi + pad);
    diagram.accumulate(v, dt, &clk);
    let meas = diagram.measure(clk.period);
    let (ssc_ppm, _) = oos_dsp::eye::ssc_deviation(&clk);
    Some(EyeSnap {
        quality: if summary < 10.0 {
            "NRZ estimate: limited sampling"
        } else {
            match j.fit {
                oos_dsp::eye::JitterFit::TooFew => "NRZ estimate: too few edges for jitter model",
                oos_dsp::eye::JitterFit::Limit => "NRZ estimate: dual-Dirac upper limits",
                oos_dsp::eye::JitterFit::Quiet => "NRZ estimate: jitter below time resolution",
                oos_dsp::eye::JitterFit::Tails => "NRZ estimate: dual-Dirac tail model",
            }
        },
        rate: clk.rate(),
        tj: oos_dsp::eye::tj_at(&j, ber),
        tie_pp: j.pp,
        rj: j.rj_rms,
        dj: j.dj_pp,
        pj,
        sj,
        ddj: means.ddj(),
        dcd: means.dcd(),
        srj: oos_dsp::eye::srj_pp(&trend, clk.period),
        j2: oos_dsp::eye::tj_at(&j, oos_dsp::eye::BER_J2),
        pn: oos_dsp::eye::phase_noise(&trend, clk.period, pn_lo, pn_hi),
        one: meas.one,
        zero: meas.zero,
        amp: meas.amplitude,
        height: meas.height,
        width: meas.width,
        height_ber: meas.height_at(ber),
        // eye_width_at is in unit intervals (the eye page shows UI); the tile is in seconds
        width_ber: oos_dsp::eye::eye_width_at(&j, clk.period, ber) * clk.period,
        q: meas.q_factor,
        s1: meas.sigma_one,
        s0: meas.sigma_zero,
        ssc_ppm,
        ssc_hz: oos_dsp::eye::ssc_rate(&clk),
        tnt: oos_dsp::eye::tnt_ratio(v, dt, &clk),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eye Width@BER has the unit of its tile, seconds: the unit interval left after TJ@BER, not a fraction of it.
    #[test]
    fn eye_width_at_ber_is_seconds() {
        // PRBS-11 at 1 Gb/s, 20 samples per bit, 4-sample edges, edge times jittered by up to ±10 ps
        let dt = 50e-12;
        let bits = oos_dsp::linecode::prbs11(3000);
        let mut s = 5u32;
        let mut v = Vec::new();
        let mut level = 0.0f32;
        for &b in &bits {
            let to = if b { 1.0 } else { 0.0 };
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let shift = ((s >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.4;
            for k in 0..20 {
                let u = ((k as f32 - shift) / 4.0).clamp(0.0, 1.0);
                v.push(level + (to - level) * u);
            }
            level = to;
        }
        let m = Model::default();
        let row = [WaveId::Ch(0); 4];
        let waves = [(WaveId::Ch(0), v)];
        let x = Ctx { model: &m, t0: 0.0, dt, row, waves: &waves };
        let r = Jitter.compute(&x, &[EYE_WIDTH_BER, TJ_BER, BIT_RATE]);
        let get = |id| r.values.iter().find(|v| v.0 == id).unwrap().1;
        let (ew, tj, ui) = (get(EYE_WIDTH_BER), get(TJ_BER), 1.0 / get(BIT_RATE));
        assert!((ui - 1e-9).abs() < 1e-12, "UI {ui}");
        assert!(tj.is_finite() && tj > 0.0 && tj < ui, "TJ {tj}");
        assert!((ew - (ui - tj)).abs() < 1e-15, "EW {ew}, UI {ui}, TJ {tj}");
    }
}
