// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Add-measurements catalog: tabs, collapsible groups, a one-line description and a drawn mark.
//! Every button calls a measurement that already exists. A button with no function is not listed.

use super::*;
use std::cell::RefCell;

const TABS: [&str; 6] = ["Standard", "Jitter", "Power", "IMDA", "DPM", "WBG-DPT"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub(super) enum Cat {
    Amplitude = 1,
    Maximum = 2,
    Minimum = 3,
    PeakToPeak = 4,
    PosOvershoot = 5,
    NegOvershoot = 6,
    Mean = 7,
    Rms = 8,
    AcRms = 9,
    Top = 10,
    Base = 11,
    Area = 12,
    Period = 13,
    Frequency = 14,
    UnitInterval = 15,
    DataRate = 16,
    PosWidth = 17,
    NegWidth = 18,
    Skew = 19,
    Delay = 20,
    Rise = 21,
    Fall = 22,
    Phase = 23,
    RiseSlew = 24,
    FallSlew = 25,
    Burst = 26,
    PosDuty = 27,
    NegDuty = 28,
    Outside = 29,
    Setup = 30,
    Hold = 31,
    NPeriods = 32,
    HighTime = 33,
    LowTime = 34,
    JitterSummary = 40,
    Tie = 41,
    PhaseNoise = 42,
    TjBer = 43,
    RjDd = 44,
    DjDd = 45,
    SjFreq = 46,
    Pj = 47,
    Ddj = 48,
    Dcd = 49,
    Srj = 50,
    J2 = 51,
    EyeHeight = 60,
    EyeWidth = 61,
    EyeHigh = 62,
    EyeHeightBer = 63,
    EyeWidthBer = 64,
    EyeLow = 65,
    QFactor = 66,
    BitHigh = 70,
    BitLow = 71,
    BitAmplitude = 72,
    DcCommon = 73,
    AcCommon = 74,
    DiffCross = 75,
    Tnt = 76,
    BitRate = 80,
    SscDev = 81,
    SscRate = 82,
    PowerQuality = 90,
    Harmonics = 91,
    Inrush = 92,
    InputC = 93,
    CycAmp = 94,
    CycTop = 95,
    CycBase = 96,
    CycPp = 97,
    CycMax = 98,
    CycMin = 99,
    TurnOn = 100,
    TurnOff = 101,
    EOn = 102,
    EOff = 103,
    ECond = 104,
    DvdtRise = 105,
    DvdtFall = 106,
    DidtRise = 107,
    DidtFall = 108,
    RdsOn = 109,
    Inductance = 110,
    MagProp = 111,
    MagLoss = 112,
    Ripple = 113,
    LineRipple = 114,
    SwRipple = 115,
    Psrr = 116,
    Impedance = 117,
    PowerFactor = 118,
    Dq0 = 130,
    Speed = 131,
    Accel = 132,
    Direction = 133,
    Angle = 134,
    Torque = 135,
    MechPower = 136,
    SysEff = 137,
    RailOver = 150,
    RailUnder = 151,
    RailTurn = 152,
    DcRail = 153,
    Vpeak = 170,
    Ipeak = 171,
    TdOn = 172,
    Tr = 173,
    Ton = 174,
    TdOff = 175,
    Tf = 176,
    Toff = 177,
    DpdtV = 178,
    DpdtI = 179,
    Tdt = 180,
    Trr = 181,
    Qrr = 182,
    Err = 183,
    Irrm = 184,
    DiodeDdt = 185,
    Qoss = 186,
}

#[derive(Clone, Copy)]
enum CatKind {
    Meas(Meas),
    Pair(usize),
    Wave,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    One,
    Two,
}

#[derive(Clone, Copy)]
enum Mark {
    Pulse,
    Square,
    Edge,
    Eye,
    Bars,
    Loop,
    Slope,
}

struct Meta {
    name: &'static str,
    blurb: &'static str,
    mark: Mark,
    unit: &'static str,
    kind: CatKind,
    need: Need,
}

const AMP: &[Cat] = &[
    Cat::Amplitude, Cat::Maximum, Cat::Minimum, Cat::PeakToPeak, Cat::PosOvershoot, Cat::NegOvershoot, Cat::Mean, Cat::Rms, Cat::AcRms, Cat::Top, Cat::Base, Cat::Area,
];
const TIME: &[Cat] = &[
    Cat::Period, Cat::Frequency, Cat::UnitInterval, Cat::DataRate, Cat::PosWidth, Cat::NegWidth, Cat::Skew, Cat::Delay, Cat::Rise, Cat::Fall, Cat::Phase, Cat::RiseSlew,
    Cat::FallSlew, Cat::Burst, Cat::PosDuty, Cat::NegDuty, Cat::Outside, Cat::Setup, Cat::Hold, Cat::NPeriods, Cat::HighTime, Cat::LowTime,
];
const JIT: &[Cat] = &[
    Cat::JitterSummary, Cat::Tie, Cat::PhaseNoise, Cat::TjBer, Cat::RjDd, Cat::DjDd, Cat::SjFreq, Cat::Pj, Cat::Ddj, Cat::Dcd, Cat::Srj, Cat::J2,
];
const EYE_G: &[Cat] = &[Cat::EyeHeight, Cat::EyeWidth, Cat::EyeHigh, Cat::EyeHeightBer, Cat::EyeWidthBer, Cat::EyeLow, Cat::QFactor];
const JAMP: &[Cat] = &[Cat::BitHigh, Cat::BitLow, Cat::BitAmplitude, Cat::DcCommon, Cat::AcCommon, Cat::DiffCross, Cat::Tnt];
const JTIME: &[Cat] = &[Cat::BitRate, Cat::SscDev, Cat::SscRate];
const PIN: &[Cat] = &[Cat::PowerQuality, Cat::Harmonics, Cat::Inrush, Cat::InputC];
const PAMP: &[Cat] = &[Cat::CycAmp, Cat::CycTop, Cat::CycBase, Cat::CycPp, Cat::CycMax, Cat::CycMin];
const PTIM: &[Cat] = &[Cat::TurnOn, Cat::TurnOff];
const PSW: &[Cat] = &[Cat::EOn, Cat::EOff, Cat::ECond, Cat::DvdtRise, Cat::DvdtFall, Cat::DidtRise, Cat::DidtFall, Cat::RdsOn];
const PMAG: &[Cat] = &[Cat::Inductance, Cat::MagProp, Cat::MagLoss];
const POUT: &[Cat] = &[Cat::Ripple, Cat::LineRipple, Cat::SwRipple];
const PFR: &[Cat] = &[Cat::Psrr, Cat::Impedance];
const PGR: &[Cat] = &[Cat::PowerFactor];
const INV: &[Cat] = &[Cat::Dq0];
const MECH: &[Cat] = &[Cat::Speed, Cat::Accel, Cat::Direction, Cat::Angle, Cat::Torque, Cat::MechPower, Cat::SysEff];
const RAIL: &[Cat] = &[Cat::RailOver, Cat::RailUnder, Cat::RailTurn, Cat::DcRail, Cat::TurnOn, Cat::TurnOff];
const DSW: &[Cat] = &[Cat::Vpeak, Cat::Ipeak, Cat::TdOn, Cat::TdOff, Cat::Tr, Cat::Tf, Cat::Ton, Cat::Toff, Cat::DpdtV, Cat::DpdtI, Cat::Tdt];
const DREC: &[Cat] = &[Cat::Trr, Cat::Qrr, Cat::Err, Cat::Irrm, Cat::DiodeDdt, Cat::Qoss];

fn tab_groups(tab: u8) -> &'static [(&'static str, &'static [Cat])] {
    match tab {
        0 => &[("AMPLITUDE MEASUREMENTS", AMP), ("TIME MEASUREMENTS", TIME)],
        1 => &[("JITTER MEASUREMENTS", JIT), ("EYE MEASUREMENTS", EYE_G), ("AMPLITUDE MEASUREMENTS", JAMP), ("TIME MEASUREMENTS", JTIME)],
        2 => &[
            ("INPUT ANALYSIS", PIN),
            ("AMPLITUDE ANALYSIS", PAMP),
            ("TIMING ANALYSIS", PTIM),
            ("SWITCHING ANALYSIS", PSW),
            ("MAGNETIC ANALYSIS", PMAG),
            ("OUTPUT ANALYSIS", POUT),
            ("FREQUENCY RESPONSE ANALYSIS", PFR),
            ("GREEN POWER ANALYSIS", PGR),
        ],
        3 => &[("INVERTER", INV), ("MECHANICAL", MECH)],
        4 => &[("RAIL", RAIL)],
        _ => &[("SWITCHING", DSW), ("RECOVERY", DREC)],
    }
}

pub(super) fn cat_from(n: u16) -> Option<Cat> {
    listed().into_iter().find(|c| *c as u16 == n)
}

fn listed() -> Vec<Cat> {
    let mut v = Vec::new();
    for tab in 0..TABS.len() as u8 {
        for (_, items) in tab_groups(tab) {
            for &c in *items {
                if !v.contains(&c) {
                    v.push(c);
                }
            }
        }
    }
    v
}

fn meta(c: Cat) -> Meta {
    use CatKind::{Pair, Wave};
    use Mark::*;
    use Need::*;
    let m = |name, blurb, mark, unit, kind, need| Meta { name, blurb, mark, unit, kind, need };
    let meas = |k| CatKind::Meas(k);
    match c {
        Cat::Amplitude => m("Amplitude", "Difference between the top level and the base level.", Pulse, "V", meas(Meas::Vamp), One),
        Cat::Maximum => m("Maximum", "Highest sample in the record.", Pulse, "V", meas(Meas::Vmax), One),
        Cat::Minimum => m("Minimum", "Lowest sample in the record.", Pulse, "V", meas(Meas::Vmin), One),
        Cat::PeakToPeak => m("Peak-to-Peak", "Highest sample minus the lowest sample.", Pulse, "V", meas(Meas::Vpp), One),
        Cat::PosOvershoot => m("Positive Overshoot", "How far the peak rises above the top, as a percent of the amplitude.", Pulse, "%", meas(Meas::Overshoot), One),
        Cat::NegOvershoot => m("Negative Overshoot", "How far the trough falls below the base, as a percent of the amplitude.", Pulse, "%", meas(Meas::Preshoot), One),
        Cat::Mean => m("Mean", "Average of the samples in the record.", Pulse, "V", meas(Meas::Vavg), One),
        Cat::Rms => m("RMS", "Root-mean-square of the samples.", Pulse, "V", meas(Meas::Vrms), One),
        Cat::AcRms => m("AC RMS", "Root-mean-square with the mean removed.", Pulse, "V", meas(Meas::VrmsAc), One),
        Cat::Top => m("Top", "High level of the waveform, taken from the histogram.", Pulse, "V", meas(Meas::Vtop), One),
        Cat::Base => m("Base", "Low level of the waveform, taken from the histogram.", Pulse, "V", meas(Meas::Vbase), One),
        Cat::Area => m("Area", "Integral of the samples over the record.", Pulse, "Vs", meas(Meas::Area), One),
        Cat::Period => m("Period", "Time from one mid crossing to the next in the same direction.", Square, "s", meas(Meas::Period), One),
        Cat::Frequency => m("Frequency", "How often a full cycle repeats.", Square, "Hz", meas(Meas::Freq), One),
        Cat::UnitInterval => m("Unit Interval", "Shortest repeating gap between the mid crossings.", Square, "s", Wave, One),
        Cat::DataRate => m("Data Rate", "Repetition rate from the period, or from the unit interval when the period is empty.", Square, "Hz", Wave, One),
        Cat::PosWidth => m("Positive Pulse Width", "Time a cycle stays above the mid level.", Square, "s", meas(Meas::PWidth), One),
        Cat::NegWidth => m("Negative Pulse Width", "Time a cycle stays below the mid level.", Square, "s", meas(Meas::NWidth), One),
        Cat::Skew => m("Skew", "Mean gap from each edge to the nearest edge on the other source.", Edge, "s", Pair(8), Two),
        Cat::Delay => m("Delay", "Time from the first edge to the matching edge on the other source.", Edge, "s", Pair(0), Two),
        Cat::Rise => m("Rise Time", "Time to rise from the low reference to the high reference.", Edge, "s", meas(Meas::Rise), One),
        Cat::Fall => m("Fall Time", "Time to fall from the high reference to the low reference.", Edge, "s", meas(Meas::Fall), One),
        Cat::Phase => m("Phase", "Delay as a fraction of the period, in degrees.", Edge, "\u{b0}", Pair(4), Two),
        Cat::RiseSlew => m("Rising Slew Rate", "How fast the level changes across the rise.", Slope, "V/s", meas(Meas::PSlew), One),
        Cat::FallSlew => m("Falling Slew Rate", "How fast the level changes across the fall.", Slope, "V/s", meas(Meas::NSlew), One),
        Cat::Burst => m("Burst Width", "Mean length of a run of edges separated by the idle gap.", Square, "s", meas(Meas::Burst), One),
        Cat::PosDuty => m("Positive Duty Cycle", "High portion of a cycle, as a percent of the period.", Square, "%", meas(Meas::PDuty), One),
        Cat::NegDuty => m("Negative Duty Cycle", "Low portion of a cycle, as a percent of the period.", Square, "%", meas(Meas::NDuty), One),
        Cat::Outside => m("Time Outside Level", "Mean time spent beyond the high reference or the low reference.", Square, "s", meas(Meas::Outside), One),
        Cat::Setup => m("Setup Time", "How long the data edge leads the clock edge.", Edge, "s", Pair(12), Two),
        Cat::Hold => m("Hold Time", "How long the data edge stays put after the clock edge.", Edge, "s", Pair(13), Two),
        Cat::NPeriods => m("Duration N-Periods", "Mean time spanning N mid crossings.", Square, "s", meas(Meas::NPeriods), One),
        Cat::HighTime => m("High Time", "Time a cycle stays above the high reference.", Square, "s", meas(Meas::HighTime), One),
        Cat::LowTime => m("Low Time", "Time a cycle stays below the low reference.", Square, "s", meas(Meas::LowTime), One),
        Cat::JitterSummary => m("Jitter Summary", "Total jitter at the selected bit-error rate.", Eye, "s", Wave, One),
        Cat::Tie => m("TIE", "Peak-to-peak time-interval error against the recovered clock.", Eye, "s", Wave, One),
        Cat::PhaseNoise => m("Phase Noise", "RMS jitter integrated between the two offset frequencies.", Bars, "s", Wave, One),
        Cat::TjBer => m("TJ@BER", "Total jitter extrapolated to the selected bit-error rate.", Eye, "s", Wave, One),
        Cat::RjDd => m("RJ-dd", "Random jitter from the dual-Dirac fit of the tails.", Eye, "s", Wave, One),
        Cat::DjDd => m("DJ-dd", "Deterministic jitter from the dual-Dirac fit of the tails.", Eye, "s", Wave, One),
        Cat::SjFreq => m("SJ@Freq", "Largest periodic tone in the time-interval error.", Bars, "s", Wave, One),
        Cat::Pj => m("PJ", "Combined size of the periodic tones in the time-interval error.", Bars, "s", Wave, One),
        Cat::Ddj => m("DDJ", "Spread of the mean edge error across the run lengths before an edge.", Eye, "s", Wave, One),
        Cat::Dcd => m("DCD", "Difference between the mean rising-edge error and the falling-edge error.", Eye, "s", Wave, One),
        Cat::Srj => m("SRJ", "Peak-to-peak of the half-rate, quarter-rate and eighth-rate tones added together.", Bars, "s", Wave, One),
        Cat::J2 => m("J2", "Total jitter at a bit-error rate of 2.5e-3.", Eye, "s", Wave, One),
        Cat::EyeHeight => m("Eye Height", "Vertical opening inside three standard deviations of the two levels.", Eye, "V", Wave, One),
        Cat::EyeWidth => m("Eye Width", "Horizontal opening inside three standard deviations of the crossing.", Eye, "s", Wave, One),
        Cat::EyeHigh => m("Eye High", "Upper edge of that three-sigma opening.", Eye, "V", Wave, One),
        Cat::EyeHeightBer => m("Eye Height@BER", "Vertical opening extrapolated to the selected bit-error rate.", Eye, "V", Wave, One),
        Cat::EyeWidthBer => m("Eye Width@BER", "Time left in the unit interval after total jitter at that rate.", Eye, "s", Wave, One),
        Cat::EyeLow => m("Eye Low", "Lower edge of the three-sigma opening.", Eye, "V", Wave, One),
        Cat::QFactor => m("Q-Factor", "Eye amplitude divided by the noise on the two levels.", Eye, "", Wave, One),
        Cat::BitHigh => m("Bit High", "Mean level of a high bit in the middle of the eye.", Pulse, "V", Wave, One),
        Cat::BitLow => m("Bit Low", "Mean level of a low bit in the middle of the eye.", Pulse, "V", Wave, One),
        Cat::BitAmplitude => m("Bit Amplitude", "High bit level minus the low bit level.", Pulse, "V", Wave, One),
        Cat::DcCommon => m("DC Common Mode", "Mean of the two sources added together and halved.", Pulse, "V", Wave, Two),
        Cat::AcCommon => m("AC Common Mode (Pk-Pk)", "Peak-to-peak of the common mode after the high-pass.", Pulse, "V", Wave, Two),
        Cat::DiffCross => m("Differential Crossover", "Mean voltage where the two sources cross.", Pulse, "V", Wave, Two),
        Cat::Tnt => m("T/nT Ratio", "Mid-interval level of a steady bit against the last transition, in dB.", Pulse, "dB", Wave, One),
        Cat::BitRate => m("Data Rate", "Bit rate of the recovered clock.", Square, "Hz", Wave, One),
        Cat::SscDev => m("SSC Freq Dev", "Peak-to-peak spread of the cycle frequency, in parts per million.", Slope, "ppm", Wave, One),
        Cat::SscRate => m("SSC Modulation Rate", "How fast that frequency spread repeats.", Slope, "Hz", Wave, One),
        Cat::PowerQuality => m("Power Quality", "Real power over the whole cycles of the voltage and the current.", Bars, "W", Wave, One),
        Cat::Harmonics => m("Harmonics", "Current above the fundamental, as a percent of the fundamental.", Bars, "%", Wave, One),
        Cat::Inrush => m("Inrush Current", "Largest absolute current in the record.", Slope, "A", Wave, One),
        Cat::InputC => m("Input Capacitance", "Charge moved, divided by the voltage swing. The record is the turn-on.", Loop, "F", Wave, One),
        Cat::CycAmp => m("Cycle Amplitude", "Mean of each cycle's top minus its base.", Pulse, "V", meas(Meas::CycAmp), One),
        Cat::CycTop => m("Cycle Top", "Mean high level of each cycle.", Pulse, "V", meas(Meas::CycTop), One),
        Cat::CycBase => m("Cycle Base", "Mean low level of each cycle.", Pulse, "V", meas(Meas::CycBase), One),
        Cat::CycPp => m("Cycle Peak-to-Peak", "Mean of each cycle's highest sample minus its lowest.", Pulse, "V", meas(Meas::CycPp), One),
        Cat::CycMax => m("Cycle Maximum", "Mean of each cycle's highest sample.", Pulse, "V", meas(Meas::CycMax), One),
        Cat::CycMin => m("Cycle Minimum", "Mean of each cycle's lowest sample.", Pulse, "V", meas(Meas::CycMin), One),
        Cat::TurnOn => m("Turn On Time", "From the second source rising through mid-level to the first source reaching its top.", Edge, "s", Wave, Two),
        Cat::TurnOff => m("Turn Off Time", "From the second source falling through mid-level to the first source reaching its base.", Edge, "s", Wave, Two),
        Cat::EOn => m("E on", "Mean energy of a turn-on transition.", Slope, "J", Wave, One),
        Cat::EOff => m("E off", "Mean energy of a turn-off transition.", Slope, "J", Wave, One),
        Cat::ECond => m("E cond", "Mean energy while the switch stays on.", Slope, "J", Wave, One),
        Cat::DvdtRise => m("dv/dt rise", "Slope of the voltage on a rising edge.", Slope, "V/s", Wave, One),
        Cat::DvdtFall => m("dv/dt fall", "Slope of the voltage on a falling edge.", Slope, "V/s", Wave, One),
        Cat::DidtRise => m("di/dt rise", "Slope of the current on a rising edge.", Slope, "A/s", Wave, One),
        Cat::DidtFall => m("di/dt fall", "Slope of the current on a falling edge.", Slope, "A/s", Wave, One),
        Cat::RdsOn => m("RDS(on)", "On-state voltage divided by the on-state current.", Slope, "\u{3a9}", Wave, One),
        Cat::Inductance => m("Inductance", "Slope of flux linkage against current on the rising branch.", Loop, "H", Wave, One),
        Cat::MagProp => m("Magnetic Property", "Relative permeability when turns, area and path length are set.", Loop, "", Wave, One),
        Cat::MagLoss => m("Magnetic Loss", "Power left after the copper loss is taken out.", Loop, "W", Wave, One),
        Cat::Ripple => m("Ripple", "Peak-to-peak of the whole voltage record.", Pulse, "V", Wave, One),
        Cat::LineRipple => m("Line Ripple", "Peak-to-peak of the voltage at the line frequency.", Pulse, "V", Wave, One),
        Cat::SwRipple => m("Switching Ripple", "Peak-to-peak of the voltage at the switching frequency.", Pulse, "V", Wave, One),
        Cat::Psrr => m("PSRR", "Rejection of the first source by the second, at the measured frequency.", Bars, "dB", Wave, Two),
        Cat::Impedance => m("Impedance", "Linear ratio of the second source to the first at that frequency.", Bars, "", Wave, Two),
        Cat::PowerFactor => m("Power Factor", "Real power divided by the apparent power.", Bars, "", Wave, One),
        Cat::Dq0 => m("DQ0", "Direct-axis mean of the three phase sources.", Loop, "", Wave, One),
        Cat::Speed => m("Speed", "Mean shaft speed taken from the phase vector.", Slope, "rpm", Wave, One),
        Cat::Accel => m("Acceleration", "Slope of that speed across revolutions.", Slope, "rpm/s", Wave, One),
        Cat::Direction => m("Direction", "Sign of the speed: forward is 1, stopped is 0, reverse is -1.", Slope, "", Wave, One),
        Cat::Angle => m("Angle", "Shaft angle at the trigger.", Loop, "\u{b0}", Wave, One),
        Cat::Torque => m("Torque", "Mean of the torque source, in the chosen scale.", Bars, "N\u{b7}m", Wave, One),
        Cat::MechPower => m("Mechanical Power", "Torque times the shaft speed.", Bars, "W", Wave, One),
        Cat::SysEff => m("System Efficiency", "Mechanical power divided by the electrical power of the configured pair.", Bars, "", Wave, One),
        Cat::RailOver => m("Overshoot", "How far the rail rises above its reference.", Pulse, "V", Wave, One),
        Cat::RailUnder => m("Undershoot", "How far the rail falls below its reference.", Pulse, "V", Wave, One),
        Cat::RailTurn => m("Turn-on Overshoot", "Highest rail voltage after the input rises through mid-level.", Pulse, "V", Wave, Two),
        Cat::DcRail => m("DC Rail", "Mean voltage of the rail.", Pulse, "V", Wave, One),
        Cat::Vpeak => m("Vpeak", "Highest voltage on the turn-off edge.", Pulse, "V", Wave, One),
        Cat::Ipeak => m("Ipeak", "Highest current on the turn-on edge.", Pulse, "A", Wave, One),
        Cat::TdOn => m("Td(on)", "Delay from the gate rising through its threshold to the current starting to rise.", Edge, "s", Wave, One),
        Cat::TdOff => m("Td(off)", "Delay from the gate falling through its threshold to the voltage starting to rise.", Edge, "s", Wave, One),
        Cat::Tr => m("Tr", "Current rise time of the turn-on.", Edge, "s", Wave, One),
        Cat::Tf => m("Tf", "Voltage rise time of the turn-off.", Edge, "s", Wave, One),
        Cat::Ton => m("Ton", "Turn-on delay plus the current rise.", Edge, "s", Wave, One),
        Cat::Toff => m("Toff", "Turn-off delay plus the voltage rise.", Edge, "s", Wave, One),
        Cat::DpdtV => m("dv/dt", "Slope of the voltage on the pulse edge.", Slope, "V/s", Wave, One),
        Cat::DpdtI => m("di/dt", "Slope of the current on the pulse edge.", Slope, "A/s", Wave, One),
        Cat::Tdt => m("Tdt", "The smaller of the two dead times.", Edge, "s", Wave, One),
        Cat::Trr => m("Trr", "Reverse-recovery time of the diode current.", Edge, "s", Wave, One),
        Cat::Qrr => m("Qrr", "Reverse-recovery charge.", Pulse, "C", Wave, One),
        Cat::Err => m("Err", "Energy of the reverse-recovery interval.", Pulse, "J", Wave, One),
        Cat::Irrm => m("Irrm", "Peak reverse-recovery current.", Pulse, "A", Wave, One),
        Cat::DiodeDdt => m("Diode d/dt", "Slope of the recovery current up to its peak.", Slope, "A/s", Wave, One),
        Cat::Qoss => m("Qoss", "Charge moved while the voltage rises.", Pulse, "C", Wave, One),
    }
}

#[derive(Clone)]
struct EyeSnap {
    period: f64,
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

fn stamp(v: &[f32], dt: f64, extra: u64) -> u64 {
    let mut h = dt.to_bits() ^ extra ^ (v.len() as u64).wrapping_mul(0x9e3779b97f4a7c15);
    let step = (v.len() / 24).max(1);
    for x in v.iter().step_by(step) {
        h = h.wrapping_mul(0x100000001b3).wrapping_add(x.to_bits() as u64);
    }
    h
}

fn eye_of(v: &[f32], dt: f64, ber: f64, pn_lo: f64, pn_hi: f64) -> Option<EyeSnap> {
    if v.len() < 16 || !(dt > 0.0) {
        return None;
    }
    let key = stamp(v, dt, ber.to_bits() ^ pn_lo.to_bits().rotate_left(11) ^ pn_hi.to_bits().rotate_left(23));
    thread_local! {
        static CACHE: RefCell<Option<(u64, EyeSnap)>> = RefCell::new(None);
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().as_ref().filter(|e| e.0 == key).map(|e| e.1.clone())) {
        return Some(hit);
    }
    let finite: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if finite.len() < 16 {
        return None;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &x in &finite {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if !(hi > lo) {
        return None;
    }
    let thr = 0.5 * (lo + hi);
    let hyst = (0.05 * (hi - lo)).max(1e-9);
    let clk = oos_dsp::eye::recover_clock(v, dt, thr, hyst, None)?;
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
    let snap = EyeSnap {
        period: clk.period,
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
        width_ber: oos_dsp::eye::eye_width_at(&j, clk.period, ber),
        q: meas.q_factor,
        s1: meas.sigma_one,
        s0: meas.sigma_zero,
        ssc_ppm,
        ssc_hz: oos_dsp::eye::ssc_rate(&clk),
        tnt: oos_dsp::eye::tnt_ratio(v, dt, &clk),
    };
    CACHE.with(|c| *c.borrow_mut() = Some((key, snap.clone())));
    Some(snap)
}

struct Hit {
    n: u16,
    r: Rect,
}

struct Lay {
    r: Rect,
    close: Rect,
    settings: Rect,
    tabs: Vec<Rect>,
    preview: Rect,
    mark: Rect,
    sources: Vec<Hit>,
    add: Rect,
    view: Rect,
    groups: Vec<Hit>,
    items: Vec<Hit>,
    scroll_up: Option<Rect>,
    scroll_down: Option<Rect>,
    content: i32,
}

impl Ui {
    fn cat_sel(&self) -> Cat {
        cat_from(self.cat_sel).unwrap_or(Cat::Amplitude)
    }

    fn src_label(&self, id: WaveId) -> String {
        match id {
            WaveId::Ch(c) => format!("Ch {}", c + 1),
            WaveId::Ref(i) => format!("Ref {}", i + 1),
        }
    }

    fn source_slots(&self) -> Vec<(u8, &'static str, WaveId)> {
        let tab = self.cat_tab;
        let sel = self.cat_sel();
        let a = self.meas_src;
        let b = self.meas_src_b;
        match tab {
            2 => vec![(0, "Voltage source", a), (1, "Current source", b)],
            3 => {
                let mut v = vec![(0, "Phase A", a), (1, "Phase B", b), (2, "Phase C", self.cat_c)];
                if matches!(sel, Cat::Torque | Cat::MechPower | Cat::SysEff) {
                    v.push((3, "Torque", self.cat_d));
                }
                v
            }
            4 => vec![(0, "Rail", a), (1, "Input", b)],
            5 => vec![(0, "Voltage", a), (1, "Current", b), (2, "Gate", self.cat_c)],
            _ => {
                if meta(sel).need == Need::Two {
                    vec![(0, "Source", a), (1, "Source B", b)]
                } else {
                    vec![(0, "Source", a)]
                }
            }
        }
    }

    fn catalog_lay(&self) -> Lay {
        let r = Rect::new(8, TOP.h + 4, W - 16, BOTTOM.y - TOP.h - 8);
        let close = Rect::new(r.x + r.w - 56, r.y + 6, 44, DLG_CTRL);
        let settings = Rect::new(close.x - 108, r.y + 6, 100, 30);
        let mut tabs = Vec::new();
        let tw = (r.w - 24) / TABS.len() as i32;
        for i in 0..TABS.len() {
            tabs.push(Rect::new(r.x + 12 + i as i32 * tw, r.y + 42, tw - 6, 28));
        }
        let preview = Rect::new(r.x + 12, r.y + 78, r.w - 24, 108);
        let mark = Rect::new(preview.x + 8, preview.y + 8, 168, preview.h - 16);
        let slots = self.source_slots();
        let rows = if slots.len() > 2 { 2 } else { 1 };
        let src_y = preview.y + preview.h + 8;
        let src_h = 36;
        let add = Rect::new(r.x + r.w - 132, src_y, 120, src_h);
        let room = add.x - r.x - 20;
        let per = room / (slots.len().min(2) as i32).max(1);
        let mut sources = Vec::new();
        for (i, (id, _, _)) in slots.iter().enumerate() {
            let col = (i % 2) as i32;
            let row = (i / 2) as i32;
            let x = r.x + 12 + col * per;
            let y = src_y + row * (src_h + 6);
            sources.push(Hit { n: *id as u16, r: Rect::new(x + per / 3, y, per - per / 3 - 8, src_h) });
        }
        let view_y = src_y + rows * (src_h + 6) + 4;
        let view = Rect::new(r.x + 10, view_y, r.w - 28, r.y + r.h - 8 - view_y);
        let mut y = 0;
        let mut groups = Vec::new();
        let mut items = Vec::new();
        let open = self.cat_open[self.cat_tab as usize];
        let cols = 3;
        let gap = 6;
        let bw = (view.w - 8 - (cols - 1) * gap) / cols;
        let bh = 42;
        for (g, (_, list)) in tab_groups(self.cat_tab).iter().enumerate() {
            groups.push(Hit { n: g as u16, r: Rect::new(0, y, view.w - 4, 26) });
            y += 28;
            if open & (1 << g) == 0 {
                continue;
            }
            for (i, &c) in list.iter().enumerate() {
                let col = (i % cols as usize) as i32;
                let row = (i / cols as usize) as i32;
                items.push(Hit { n: c as u16, r: Rect::new(col * (bw + gap), y + row * (bh + gap), bw, bh) });
            }
            let nrows = list.len().div_ceil(cols as usize) as i32;
            y += nrows * (bh + gap) + 4;
        }
        let (scroll_up, scroll_down) = if y > view.h {
            let x = view.x + view.w + 2;
            (Some(Rect::new(x, view.y, 14, view.h / 2 - 2)), Some(Rect::new(x, view.y + view.h / 2, 14, view.h / 2)))
        } else {
            (None, None)
        };
        Lay { r, close, settings, tabs, preview, mark, sources, add, view, groups, items, scroll_up, scroll_down, content: y }
    }

    pub(super) fn catalog_settings_rect(&self) -> Rect {
        self.catalog_lay().settings
    }

    /// Set control on an added standard measurement, in screen coordinates.
    pub(super) fn catalog_set_rect(&self, k: Meas) -> Option<Rect> {
        let lay = self.catalog_lay();
        lay.items.iter().find_map(|it| {
            let c = cat_from(it.n)?;
            let CatKind::Meas(mk) = meta(c).kind else { return None };
            (mk == k).then(|| self.catalog_set_on(screen_rect(lay.view, it.r, self.cat_scroll), c)).flatten()
        })
    }

    /// Header of one group on the current tab, in screen coordinates.
    pub(super) fn catalog_group_rect(&self, group: usize) -> Option<Rect> {
        let lay = self.catalog_lay();
        lay.groups.iter().find(|g| g.n as usize == group).map(|g| screen_rect(lay.view, g.r, self.cat_scroll))
    }

    pub(super) fn catalog_view(&self) -> Rect {
        self.catalog_lay().view
    }

    pub(super) fn catalog_scroll_down(&self) -> Option<Rect> {
        self.catalog_lay().scroll_down
    }

    fn catalog_set_on(&self, rect: Rect, c: Cat) -> Option<Rect> {
        let CatKind::Meas(_) = meta(c).kind else { return None };
        self.cat_added(c).then(|| Rect::new(rect.x + rect.w - 52, rect.y + (rect.h - 40) / 2, 48, 40))
    }

    pub(super) fn catalog_dialog(&self) -> Dlg {
        let lay = self.catalog_lay();
        Dlg { r: lay.r, close: lay.close, back: None, rows: Vec::new(), heads: Vec::new() }
    }

    fn content_h(&self) -> i32 {
        self.catalog_lay().content
    }

    fn clamp_scroll(&mut self) {
        let h = self.content_h();
        let view = self.catalog_lay().view.h;
        let max = (h - view).max(0);
        self.cat_scroll = self.cat_scroll.clamp(0, max);
    }

    pub(super) fn draw_catalog(&self, cv: &mut Canvas) {
        let lay = self.catalog_lay();
        let f = &self.fonts;
        cv.round(lay.r, 8, ACCENT, 255);
        cv.round(lay.r.inset(2), 7, DLG_BG, 255);
        cv.text(&f.mb, lay.r.x + 16, lay.r.y + 28, "ADD MEASUREMENTS", TEXT);
        btn(cv, lay.close, BTN, BTN_BORDER);
        cv.text_in(&f.mb, lay.close, 1, "\u{d7}", TEXT);
        btn(cv, lay.settings, BTN, BTN_BORDER);
        cv.text_in(&f.s, lay.settings, 1, "Settings", TEXT);
        for (i, rect) in lay.tabs.iter().enumerate() {
            let on = i as u8 == self.cat_tab;
            if on {
                cv.round(*rect, 4, rgb(0x2e, 0x2e, 0x32), 255);
                cv.round_outline(*rect, 4, ACCENT);
            }
            cv.text_in(&f.s, *rect, 1, TABS[i], if on { TEXT } else { DIM });
        }
        cv.round(lay.preview, 6, rgb(0x24, 0x24, 0x28), 255);
        let sel = self.cat_sel();
        let info = meta(sel);
        self.draw_mark(cv, lay.mark, info.mark, AMBER);
        let tx = lay.mark.x + lay.mark.w + 16;
        let tw = lay.preview.x + lay.preview.w - tx - 12;
        cv.text(&f.mb, tx, lay.preview.y + 28, info.name, TEXT);
        let mut y = lay.preview.y + 48;
        for line in wrap_lines(&f.s, info.blurb, tw) {
            cv.text(&f.s, tx, y, &line, LABEL);
            y += f.s.size + 2;
            if y > lay.preview.y + lay.preview.h - 4 {
                break;
            }
        }
        let slots = self.source_slots();
        for (hit, (_, label, id)) in lay.sources.iter().zip(&slots) {
            let lab = Rect::new(hit.r.x - hit.r.w / 2, hit.r.y, hit.r.w / 2, hit.r.h);
            cv.text_in(&f.s, lab, 0, label, DIM);
            btn(cv, hit.r, BTN, BTN_BORDER);
            cv.text_in(&f.s, Rect::new(hit.r.x + 8, hit.r.y, hit.r.w - 22, hit.r.h), 0, &self.src_label(*id), TEXT);
            chevron_down(cv, hit.r.x + hit.r.w - 16, hit.r.y + hit.r.h / 2 - 3, DIM);
        }
        btn(cv, lay.add, BTN, BTN_BORDER);
        cv.text_in(&f.mb, lay.add, 1, "Add", TEXT);
        cv.set_clip(lay.view);
        let scroll = self.cat_scroll;
        let open = self.cat_open[self.cat_tab as usize];
        for g in &lay.groups {
            let rect = screen_rect(lay.view, g.r, scroll);
            cv.rect(rect, rgb(0x2a, 0x2a, 0x2e));
            let name = tab_groups(self.cat_tab).get(g.n as usize).map(|g| g.0).unwrap_or("");
            cv.text_in(&f.s, Rect::new(rect.x + 10, rect.y, rect.w - 36, rect.h), 0, name, LABEL);
            if open & (1 << g.n) != 0 {
                chevron_down(cv, rect.x + rect.w - 18, rect.y + rect.h / 2 - 3, DIM);
            } else {
                chevron(cv, rect.x + rect.w - 16, rect.y + rect.h / 2, 1, DIM);
            }
        }
        for it in &lay.items {
            let Some(c) = cat_from(it.n) else { continue };
            let rect = screen_rect(lay.view, it.r, scroll);
            let on = c == sel;
            let added = self.cat_added(c);
            let set = self.catalog_set_on(rect, c);
            btn(cv, rect, if added { rgb(0x32, 0x32, 0x36) } else { BTN }, if on { ACCENT } else { BTN_BORDER });
            if on {
                cv.round_outline(rect, 5, ACCENT);
            }
            let icon = Rect::new(rect.x + 4, rect.y + 6, 32, rect.h - 12);
            self.draw_mark(cv, icon, meta(c).mark, BLUE);
            let (a, b) = split_name(meta(c).name);
            let text_w = rect.w - 46 - if set.is_some() { 52 } else { 0 };
            let text = Rect::new(rect.x + 40, rect.y, text_w, rect.h);
            if let Some(set) = set {
                btn(cv, set, BTN, ACCENT);
                cv.text_in(&f.s, set, 1, "Set", TEXT);
            }
            if let Some(b) = b {
                cv.text_in(&f.s, Rect::new(text.x, text.y, text.w, text.h / 2), 0, a, TEXT);
                cv.text_in(&f.s, Rect::new(text.x, text.y + text.h / 2, text.w, text.h / 2), 0, b, TEXT);
            } else {
                cv.text_in(&f.s, text, 0, a, TEXT);
            }
        }
        cv.no_clip();
        for (rect, up) in [(lay.scroll_up, true), (lay.scroll_down, false)].into_iter().filter_map(|(r, up)| r.map(|r| (r, up))) {
            btn(cv, rect, BTN, BTN_BORDER);
            if up {
                cv.text_in(&f.s, rect, 1, "\u{25b2}", DIM);
            } else {
                cv.text_in(&f.s, rect, 1, "\u{25bc}", DIM);
            }
        }
    }

    fn draw_mark(&self, cv: &mut Canvas, r: Rect, mark: Mark, col: Rgb) {
        let p = |x: f32, y: f32| (r.x + (x * r.w as f32) as i32, r.y + ((1.0 - y) * r.h as f32) as i32);
        let stroke = |cv: &mut Canvas, pts: &[(f32, f32)]| {
            for w in pts.windows(2) {
                let (a, b) = (p(w[0].0, w[0].1), p(w[1].0, w[1].1));
                cv.line(a.0, a.1, b.0, b.1, col);
            }
        };
        let base = [(0.08, 0.22), (0.92, 0.22)];
        match mark {
            Mark::Pulse => stroke(cv, &[(0.08, 0.35), (0.22, 0.35), (0.28, 0.78), (0.42, 0.78), (0.48, 0.48), (0.62, 0.48), (0.68, 0.78), (0.82, 0.78), (0.88, 0.35), (0.94, 0.35)]),
            Mark::Square => stroke(cv, &[(0.08, 0.3), (0.22, 0.3), (0.22, 0.75), (0.48, 0.75), (0.48, 0.3), (0.74, 0.3), (0.74, 0.75), (0.92, 0.75)]),
            Mark::Edge => {
                stroke(cv, &[(0.1, 0.28), (0.38, 0.28), (0.62, 0.74), (0.9, 0.74)]);
                stroke(cv, &[(0.18, 0.55), (0.82, 0.55)]);
            }
            Mark::Eye => {
                stroke(cv, &[(0.12, 0.25), (0.5, 0.72), (0.88, 0.25)]);
                stroke(cv, &[(0.12, 0.75), (0.5, 0.28), (0.88, 0.75)]);
            }
            Mark::Bars => {
                for (i, h) in [0.45, 0.78, 0.55].into_iter().enumerate() {
                    let x = 0.22 + i as f32 * 0.24;
                    stroke(cv, &[(x, 0.22), (x, h)]);
                }
            }
            Mark::Loop => {
                let pts: Vec<(f32, f32)> = (0..=16).map(|k| {
                    let a = k as f32 / 16.0 * std::f32::consts::TAU;
                    (0.5 + 0.32 * a.cos(), 0.5 + 0.28 * a.sin())
                }).collect();
                stroke(cv, &pts);
            }
            Mark::Slope => stroke(cv, &[(0.12, 0.28), (0.42, 0.28), (0.78, 0.78), (0.9, 0.78)]),
        }
        if !matches!(mark, Mark::Eye | Mark::Bars | Mark::Loop) {
            stroke(cv, &base);
        }
    }

    pub(super) fn catalog_tap(&mut self, x: i32, y: i32) -> bool {
        let lay = self.catalog_lay();
        if !lay.r.contains(x, y) {
            return false;
        }
        if lay.close.contains(x, y) {
            self.close_menu();
            return true;
        }
        if lay.settings.contains(x, y) {
            let p = self.menu;
            self.open_menu(MenuId::Measure);
            self.parent = Some(p);
            return true;
        }
        if lay.add.contains(x, y) {
            self.catalog_add(self.cat_sel());
            return true;
        }
        for (i, rect) in lay.tabs.iter().enumerate() {
            if rect.contains(x, y) {
                self.cat_tab = i as u8;
                self.cat_scroll = 0;
                let first = tab_groups(self.cat_tab).first().and_then(|g| g.1.first()).copied().unwrap_or(Cat::Amplitude);
                if tab_groups(self.cat_tab).iter().all(|g| !g.1.contains(&self.cat_sel())) {
                    self.cat_sel = first as u16;
                }
                return true;
            }
        }
        for hit in &lay.sources {
            if hit.r.contains(x, y) {
                match hit.n {
                    0 => self.open_picker(Item::MeasSrc),
                    1 => self.open_picker(Item::MeasSrcB),
                    2 => self.cat_c = self.step_wave(self.cat_c, if x < hit.r.x + hit.r.w / 2 { -1 } else { 1 }),
                    _ => self.cat_d = self.step_wave(self.cat_d, if x < hit.r.x + hit.r.w / 2 { -1 } else { 1 }),
                }
                return true;
            }
        }
        if lay.scroll_up.is_some_and(|r| r.contains(x, y)) {
            self.cat_scroll = self.cat_scroll.saturating_sub(80);
            return true;
        }
        if lay.scroll_down.is_some_and(|r| r.contains(x, y)) {
            self.cat_scroll += 80;
            self.clamp_scroll();
            return true;
        }
        if lay.view.contains(x, y) {
            let cy = y - lay.view.y + self.cat_scroll;
            let cx = x - lay.view.x;
            for g in &lay.groups {
                if g.r.contains(cx, cy) {
                    let bit = 1u16 << g.n;
                    self.cat_open[self.cat_tab as usize] ^= bit;
                    if self.cat_open[self.cat_tab as usize] & bit != 0 {
                        self.cat_scroll = g.r.y;
                        self.clamp_scroll();
                    }
                    return true;
                }
            }
            for it in &lay.items {
                if it.r.contains(cx, cy) {
                    let rect = screen_rect(lay.view, it.r, self.cat_scroll);
                    if let Some(c) = cat_from(it.n) {
                        if let Some(set) = self.catalog_set_on(rect, c) {
                            if set.contains(x, y) {
                                if let CatKind::Meas(k) = meta(c).kind {
                                    if let Some(i) = self.m.meas.iter().position(|&(mk, id)| mk == k && id == self.meas_src) {
                                        self.open_meas_cfg(i);
                                    }
                                }
                                return true;
                            }
                        }
                    }
                    self.cat_sel = it.n;
                    return true;
                }
            }
        }
        true
    }

    fn cat_added(&self, c: Cat) -> bool {
        match meta(c).kind {
            CatKind::Meas(k) => self.m.meas.iter().any(|e| e.0 == k && e.1 == self.meas_src),
            CatKind::Pair(k) => self.m.meas2.iter().any(|e| e.0 == k && e.1 == self.meas_src && e.2 == self.meas_src_b),
            CatKind::Wave => self.m.cat.iter().any(|e| e.0 == c as u16 && e.1 == self.meas_src && e.2 == self.meas_src_b && e.3 == self.cat_c && e.4 == self.cat_d),
        }
    }

    pub(super) fn catalog_add(&mut self, c: Cat) {
        match meta(c).kind {
            CatKind::Meas(k) => self.activate(Item::MeasAdd(k)),
            CatKind::Pair(k) => self.meas_pair_toggle(k),
            CatKind::Wave => {
                let row = (c as u16, self.meas_src, self.meas_src_b, self.cat_c, self.cat_d);
                if let Some(i) = self.m.cat.iter().position(|e| *e == row) {
                    self.m.cat.remove(i);
                    self.toast(&format!("{} removed", meta(c).name));
                } else if self.meas_make_room() {
                    self.m.cat.push(row);
                    self.toast(&format!("{} added", meta(c).name));
                }
                self.mstat.clear();
            }
        }
    }

    fn wave_vec(&self, id: WaveId) -> Option<(Vec<f32>, f64, f64)> {
        match id {
            WaveId::Ref(i) => {
                let (v, t0, dt) = self.reference_slice(i)?;
                (v.len() >= 2 && dt > 0.0).then_some((v.to_vec(), t0, dt))
            }
            WaveId::Ch(_) => {
                let rec = self.last[0].as_ref()?;
                let s = self.wave_samples(rec, id)?;
                Some((s.v, s.t0, s.dt))
            }
        }
    }

    fn align(&self, ids: &[WaveId]) -> Option<(f64, Vec<Vec<f32>>)> {
        let waves: Vec<(Vec<f32>, f64, f64)> = ids.iter().map(|&id| self.wave_vec(id)).collect::<Option<_>>()?;
        let best = waves.iter().enumerate().min_by(|a, b| a.1.2.total_cmp(&b.1.2)).map(|(i, _)| i)?;
        let (t0, dt, n) = (waves[best].1, waves[best].2, waves[best].0.len());
        if !(dt > 0.0) || n < 2 {
            return None;
        }
        let mut out = Vec::with_capacity(waves.len());
        for (v, wt0, wdt) in &waves {
            if (*wdt - dt).abs() <= dt * 1e-6 && (*wt0 - t0).abs() <= dt && v.len() == n {
                out.push(v.clone());
                continue;
            }
            let mut col = vec![f32::NAN; n];
            for i in 0..n {
                let t = t0 + i as f64 * dt;
                let x = (t - *wt0) / *wdt;
                if x < 0.0 || x > (v.len() - 1) as f64 {
                    continue;
                }
                let k = x.floor() as usize;
                let f = (x - k as f64) as f32;
                let a = v[k];
                let b = v[(k + 1).min(v.len() - 1)];
                if a.is_finite() && b.is_finite() {
                    col[i] = a + (b - a) * f;
                }
            }
            out.push(col);
        }
        Some((dt, out))
    }

    pub(super) fn cat_row(&self, id: u16, a: WaveId, b: WaveId, c: WaveId, d: WaveId) -> Option<tools::MeasRow> {
        let cat = cat_from(id)?;
        let info = meta(cat);
        let v = self.cat_number(cat, a, b, c, d);
        Some(tools::MeasRow { name: info.name.to_string(), src: a, v, unit: info.unit })
    }

    fn cat_number(&self, c: Cat, a: WaveId, b: WaveId, csrc: WaveId, d: WaveId) -> f64 {
        let nan = f64::NAN;
        let one = |id| self.wave_vec(id).map(|w| w);
        match c {
            Cat::UnitInterval => {
                let Some((v, _, dt)) = one(a) else { return nan };
                let an = oos_dsp::measure::analyze(&v, dt);
                let u = an.get(Meas::EstUi);
                if u.is_finite() && u > 0.0 { u } else { an.period() }
            }
            Cat::DataRate => {
                let Some((v, _, dt)) = one(a) else { return nan };
                let an = oos_dsp::measure::analyze(&v, dt);
                let p = an.period();
                if p.is_finite() && p > 0.0 {
                    1.0 / p
                } else {
                    let u = an.get(Meas::EstUi);
                    if u.is_finite() && u > 0.0 { 1.0 / u } else { nan }
                }
            }
            Cat::JitterSummary | Cat::TjBer => self.eye_num(a, |e| e.tj),
            Cat::Tie => self.eye_num(a, |e| e.tie_pp),
            Cat::PhaseNoise => self.eye_num(a, |e| e.pn),
            Cat::RjDd => self.eye_num(a, |e| e.rj),
            Cat::DjDd => self.eye_num(a, |e| e.dj),
            Cat::SjFreq => self.eye_num(a, |e| e.sj),
            Cat::Pj => self.eye_num(a, |e| e.pj),
            Cat::Ddj => self.eye_num(a, |e| e.ddj),
            Cat::Dcd => self.eye_num(a, |e| e.dcd),
            Cat::Srj => self.eye_num(a, |e| e.srj),
            Cat::J2 => self.eye_num(a, |e| e.j2),
            Cat::EyeHeight => self.eye_num(a, |e| e.height),
            Cat::EyeWidth => self.eye_num(a, |e| e.width),
            Cat::EyeHigh => self.eye_num(a, |e| e.one - 3.0 * e.s1),
            Cat::EyeHeightBer => self.eye_num(a, |e| e.height_ber),
            Cat::EyeWidthBer => self.eye_num(a, |e| e.width_ber),
            Cat::EyeLow => self.eye_num(a, |e| e.zero + 3.0 * e.s0),
            Cat::QFactor => self.eye_num(a, |e| e.q),
            Cat::BitHigh => self.eye_num(a, |e| e.one),
            Cat::BitLow => self.eye_num(a, |e| e.zero),
            Cat::BitAmplitude => self.eye_num(a, |e| e.amp),
            Cat::Tnt => self.eye_num(a, |e| e.tnt),
            Cat::BitRate => self.eye_num(a, |e| e.rate),
            Cat::SscDev => self.eye_num(a, |e| e.ssc_ppm),
            Cat::SscRate => self.eye_num(a, |e| e.ssc_hz),
            Cat::DcCommon => {
                if a == b { return nan; }
                let Some((dt, v)) = self.align(&[a, b]) else { return nan };
                let _ = dt;
                oos_dsp::eye::dc_common(&v[0], &v[1])
            }
            Cat::AcCommon => {
                let Some((v, _, dt)) = one(a) else { return nan };
                if a == b {
                    oos_dsp::eye::ac_common_pp(&v, None, dt, self.m.eye_hp)
                } else if let Some((_, w)) = self.align(&[a, b]) {
                    oos_dsp::eye::ac_common_pp(&w[0], Some(&w[1]), dt, self.m.eye_hp)
                } else {
                    nan
                }
            }
            Cat::DiffCross => {
                if a == b { return nan; }
                let Some((_, v)) = self.align(&[a, b]) else { return nan };
                oos_dsp::eye::diff_crossover(&v[0], &v[1])
            }
            Cat::PowerQuality => self.vi(a, b, |v, i, dt| oos_dsp::power::quality(v, i, dt, None).p_real),
            Cat::Harmonics => self.vi(a, b, |_, i, dt| {
                let h = oos_dsp::power::harmonics(i, dt, None, 40);
                let fund = h.iter().find(|x| x.0 == 1).map(|x| x.1).unwrap_or(0.0);
                let rest = h.iter().filter(|x| x.0 > 1).map(|x| x.1 * x.1).sum::<f64>().sqrt();
                if fund > 0.0 { 100.0 * rest / fund } else { nan }
            }),
            Cat::Inrush => self.vi(a, b, |_, i, dt| oos_dsp::power::inrush(i, dt).0),
            Cat::InputC => self.vi(a, b, |v, i, dt| oos_dsp::power::input_capacitance(v, i, dt)),
            Cat::TurnOn => self.vi(a, b, |vout, vin, dt| oos_dsp::power::turn_on_time(vin, vout, dt)),
            Cat::TurnOff => self.vi(a, b, |vout, vin, dt| oos_dsp::power::turn_off_time(vin, vout, dt)),
            Cat::EOn => self.sw(a, b, |s| s.e_on),
            Cat::EOff => self.sw(a, b, |s| s.e_off),
            Cat::ECond => self.sw(a, b, |s| s.e_cond),
            Cat::DvdtRise => self.sw(a, b, |s| s.dvdt_rise),
            Cat::DvdtFall => self.sw(a, b, |s| s.dvdt_fall),
            Cat::DidtRise => self.sw(a, b, |s| s.didt_rise),
            Cat::DidtFall => self.sw(a, b, |s| s.didt_fall),
            Cat::RdsOn => self.sw(a, b, |s| s.rds_on),
            Cat::Inductance => self.mag(a, b).and_then(|m| m.l).unwrap_or(nan),
            Cat::MagProp => self.mag(a, b).and_then(|m| m.mu_r).unwrap_or(nan),
            Cat::MagLoss => self.mag(a, b).and_then(|m| m.loss).unwrap_or(nan),
            Cat::Ripple => one(a).map(|(v, _, _)| oos_dsp::power::ripple(&v).0).unwrap_or(nan),
            Cat::LineRipple => one(a).map(|(v, _, dt)| {
                let f = if self.m.pw_fline > 0.0 { self.m.pw_fline } else { oos_dsp::power::line_hz(&v, dt) };
                oos_dsp::power::tone_ripple(&v, dt, f).pp
            }).unwrap_or(nan),
            Cat::SwRipple => one(a).map(|(v, _, dt)| {
                let f = if self.m.pw_fsw > 0.0 { self.m.pw_fsw } else { oos_dsp::power::switch_hz(&v, dt) };
                oos_dsp::power::switching_ripple(&v, dt, f).pp
            }).unwrap_or(nan),
            Cat::Psrr => self.ratio(a, b, oos_dsp::bode::Mode::Psrr),
            Cat::Impedance => self.ratio(a, b, oos_dsp::bode::Mode::Z),
            Cat::PowerFactor => self.vi(a, b, |v, i, dt| oos_dsp::power::quality(v, i, dt, None).pf),
            Cat::Dq0 => self.phases(a, b, csrc).and_then(|(dt, v)| oos_dsp::motor::dq0(&v[0], &v[1], &v[2], dt, oos_dsp::motor::Theta::Vector).map(|q| q.d)).unwrap_or(nan),
            Cat::Speed => self.shaft(a, b, csrc).and_then(|s| s.rpm).unwrap_or(nan),
            Cat::Accel => self.shaft(a, b, csrc).and_then(|s| s.accel).unwrap_or(nan),
            Cat::Direction => self.shaft(a, b, csrc).map(|s| match s.direction {
                oos_dsp::motor::Direction::Forward => 1.0,
                oos_dsp::motor::Direction::Stopped => 0.0,
                oos_dsp::motor::Direction::Reverse => -1.0,
            }).unwrap_or(nan),
            Cat::Angle => self.shaft(a, b, csrc).and_then(|s| s.angle_deg).unwrap_or(nan),
            Cat::Torque | Cat::MechPower | Cat::SysEff => self.mech(a, b, csrc, d).map(|m| match c {
                Cat::Torque => m.torque.unwrap_or(nan),
                Cat::MechPower => m.p_mech.unwrap_or(nan),
                _ => m.eff.unwrap_or(nan),
            }).unwrap_or(nan),
            Cat::RailOver | Cat::RailUnder | Cat::RailTurn | Cat::DcRail => {
                let Some((v, _, dt)) = one(a) else { return nan };
                let vin = if a != b { self.wave_vec(b).map(|w| w.0) } else { None };
                let rail = oos_dsp::power::rail(&v, dt, self.m.pw_ref.filter(|v| v.is_finite() && *v != 0.0), vin.as_deref(), None);
                match c {
                    Cat::RailOver => rail.overshoot,
                    Cat::RailUnder => rail.undershoot,
                    Cat::RailTurn => rail.turn_on,
                    _ => rail.dc,
                }
            }
            Cat::Vpeak | Cat::Ipeak | Cat::TdOn | Cat::TdOff | Cat::Tr | Cat::Tf | Cat::Ton | Cat::Toff | Cat::DpdtV | Cat::DpdtI | Cat::Tdt | Cat::Trr | Cat::Qrr | Cat::Err | Cat::Irrm | Cat::DiodeDdt | Cat::Qoss => {
                self.pulse(a, b, csrc).map(|p| match c {
                    Cat::Vpeak => p.vpeak.unwrap_or(nan),
                    Cat::Ipeak => p.ipeak.unwrap_or(nan),
                    Cat::TdOn => p.td_on.unwrap_or(nan),
                    Cat::TdOff => p.td_off.unwrap_or(nan),
                    Cat::Tr => p.tr.unwrap_or(nan),
                    Cat::Tf => p.tf.unwrap_or(nan),
                    Cat::Ton => p.ton.unwrap_or(nan),
                    Cat::Toff => p.toff.unwrap_or(nan),
                    Cat::DpdtV => p.dvdt.unwrap_or(nan),
                    Cat::DpdtI => p.didt.unwrap_or(nan),
                    Cat::Tdt => p.tdt.unwrap_or(nan),
                    Cat::Trr => p.trr.unwrap_or(nan),
                    Cat::Qrr => p.qrr.unwrap_or(nan),
                    Cat::Err => p.err.unwrap_or(nan),
                    Cat::Irrm => p.irrm.unwrap_or(nan),
                    Cat::DiodeDdt => p.diode_didt.unwrap_or(nan),
                    _ => p.qoss.unwrap_or(nan),
                }).unwrap_or(nan)
            }
            _ => nan,
        }
    }

    fn eye_num(&self, id: WaveId, f: impl FnOnce(&EyeSnap) -> f64) -> f64 {
        let Some((v, _, dt)) = self.wave_vec(id) else { return f64::NAN };
        eye_of(&v, dt, self.m.eye_ber, self.m.eye_pn_lo, self.m.eye_pn_hi).map(|e| f(&e)).unwrap_or(f64::NAN)
    }

    fn vi(&self, a: WaveId, b: WaveId, f: impl FnOnce(&[f32], &[f32], f64) -> f64) -> f64 {
        self.align(&[a, b]).map(|(dt, v)| f(&v[0], &v[1], dt)).unwrap_or(f64::NAN)
    }

    fn sw(&self, a: WaveId, b: WaveId, f: impl FnOnce(&oos_dsp::power::Switching) -> f64) -> f64 {
        self.align(&[a, b]).map(|(dt, v)| f(&oos_dsp::power::switching(&v[0], &v[1], dt, 10.0))).unwrap_or(f64::NAN)
    }

    fn mag(&self, a: WaveId, b: WaveId) -> Option<oos_dsp::magnetic::Magnetic> {
        let (dt, v) = self.align(&[a, b])?;
        let geom = oos_dsp::magnetic::Geom {
            turns: self.m.mag_n.max(1.0),
            area: (self.m.mag_ae > 0.0).then_some(self.m.mag_ae),
            length: (self.m.mag_le > 0.0).then_some(self.m.mag_le),
            rcu: self.m.mag_rcu.max(0.0),
        };
        Some(oos_dsp::magnetic::measure(&v[0], &v[1], dt, geom))
    }

    fn ratio(&self, a: WaveId, b: WaveId, mode: oos_dsp::bode::Mode) -> f64 {
        let Some((dt, v)) = self.align(&[a, b]) else { return f64::NAN };
        let f = oos_dsp::power::line_hz(&v[0], dt);
        oos_dsp::bode::response(mode, &v[0], &v[1], dt, f).map(|p| p.0).unwrap_or(f64::NAN)
    }

    fn phases(&self, a: WaveId, b: WaveId, c: WaveId) -> Option<(f64, Vec<Vec<f32>>)> {
        if a == b || a == c || b == c {
            return None;
        }
        self.align(&[a, b, c])
    }

    fn shaft(&self, a: WaveId, b: WaveId, c: WaveId) -> Option<oos_dsp::motor::Shaft> {
        let (dt, v) = self.phases(a, b, c)?;
        Some(oos_dsp::motor::shaft_phase(&v[0], &v[1], &v[2], dt, 0.0, self.m.inv_poles.max(1)))
    }

    fn mech(&self, a: WaveId, b: WaveId, c: WaveId, d: WaveId) -> Option<oos_dsp::motor::Mech> {
        let shaft = self.shaft(a, b, c)?;
        let torque = self.wave_vec(d).map(|w| w.0);
        let ev = self.m.inv_ev.and_then(|id| self.wave_vec(id)).map(|w| w.0);
        let ei = self.m.inv_ei.and_then(|id| self.wave_vec(id)).map(|w| w.0);
        Some(oos_dsp::motor::mechanical(shaft, torque.as_deref(), self.m.inv_scale, ev.as_deref(), ei.as_deref()))
    }

    fn pulse(&self, a: WaveId, b: WaveId, g: WaveId) -> Option<oos_dsp::dpt::Pulse> {
        let (dt, v) = self.align(&[a, b])?;
        let gate = if g != a && g != b { self.wave_vec(g).map(|w| w.0) } else { None };
        Some(oos_dsp::dpt::measure(&v[0], &v[1], gate.as_deref(), oos_dsp::dpt::Fourth::Off, dt, self.m.dpt_pct as f64))
    }
}

fn screen_rect(view: Rect, r: Rect, scroll: i32) -> Rect {
    Rect::new(view.x + r.x, view.y + r.y - scroll, r.w, r.h)
}

fn split_name(name: &str) -> (&str, Option<&str>) {
    let Some(mid) = name.find(' ') else { return (name, None) };
    let i = name.match_indices(' ').min_by_key(|(i, _)| (*i as i32 - name.len() as i32 / 2).abs()).map(|(i, _)| i).unwrap_or(mid);
    (&name[..i], Some(&name[i + 1..]))
}

fn wrap_lines(f: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if cur.is_empty() || f.width(&trial) <= width {
            cur = trial;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(ui: &mut Ui, slot: usize, v: Vec<f32>, dt: f64) {
        ui.refs[slot] = Some(RefWave { src: 0, color: 0, label: String::new(), t0: 0.0, dt, v, vpd: 1.0, offset: 0.0, on: true });
    }

    fn square(amp: f32, n: usize, half: usize) -> Vec<f32> {
        (0..n).map(|i| if (i / half) % 2 == 0 { amp } else { -amp }).collect()
    }

    #[test]
    fn every_button_has_a_name_and_the_unwired_ones_are_absent() {
        let names: Vec<&str> = listed().into_iter().map(|c| meta(c).name).collect();
        for want in ["Amplitude", "Low Time", "Jitter Summary", "Eye Height@BER", "T/nT Ratio", "Power Quality", "Inductance", "DQ0", "Vpeak", "Qoss", "Td(on)", "RDS(on)"] {
            assert!(names.contains(&want), "missing {want}");
        }
        for absent in ["Pattern Length", "SSC Slew Rate", "ISI", "RJ", "DJ", "HSS", "WaveformEngine"] {
            assert!(!names.iter().any(|n| n.eq_ignore_ascii_case(absent)), "{absent} shipped");
        }
        assert!(names.contains(&"RJ-dd") && names.contains(&"DJ-dd"), "dual-Dirac names");
        assert_eq!(TABS.len(), 6);
    }

    #[test]
    fn add_puts_a_reference_measurement_on_the_strip() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        put(&mut ui, 1, square(1.0, 2000, 40), 1e-6);
        ui.meas_src = WaveId::Ref(1);
        ui.m.meas.clear();
        ui.catalog_add(Cat::Amplitude);
        ui.catalog_add(Cat::LowTime);
        let rows = ui.meas_rows();
        let amp = rows.iter().find(|r| r.name == "Amp").expect("amplitude");
        assert!((amp.v - 2.0).abs() < 0.15, "amplitude {}", amp.v);
        assert!(rows.iter().any(|r| r.name == "Low time" && r.v.is_finite()), "low time");
        ui.catalog_add(Cat::DataRate);
        let rate = ui.meas_rows().into_iter().find(|r| r.name == "Data Rate").unwrap();
        assert!(rate.v.is_finite() && rate.v > 0.0, "{}", rate.v);
    }

    #[test]
    fn power_jitter_and_double_pulse_return_numbers() {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let n = 2000;
        let v: Vec<f32> = (0..n).map(|i| (i as f32 * 0.02).sin()).collect();
        let i: Vec<f32> = (0..n).map(|i| 0.4 * (i as f32 * 0.02).sin()).collect();
        put(&mut ui, 0, v, 1e-4);
        put(&mut ui, 1, i, 1e-4);
        ui.meas_src = WaveId::Ref(0);
        ui.meas_src_b = WaveId::Ref(1);
        ui.m.cat.clear();
        ui.catalog_add(Cat::PowerQuality);
        let p = ui.meas_rows().into_iter().find(|r| r.name == "Power Quality").unwrap();
        assert!(p.v.is_finite() && p.v > 0.0, "power {}", p.v);
        put(&mut ui, 2, square(0.5, 4000, 20), 1e-9);
        ui.meas_src = WaveId::Ref(2);
        ui.catalog_add(Cat::EyeHeight);
        ui.catalog_add(Cat::Tie);
        let rows = ui.meas_rows();
        assert!(rows.iter().any(|r| r.name == "Eye Height" && r.v.is_finite()), "eye");
        assert!(rows.iter().any(|r| r.name == "TIE" && r.v.is_finite()), "tie");

        let dt = 1e-9;
        let tend = 13e-6;
        let n = (tend / dt) as usize;
        let pwl = |pts: &[(f64, f64)], t: f64| -> f32 {
            if t <= pts[0].0 { return pts[0].1 as f32; }
            for w in pts.windows(2) {
                if t <= w[1].0 {
                    let u = (t - w[0].0) / (w[1].0 - w[0].0);
                    return (w[0].1 + u * (w[1].1 - w[0].1)) as f32;
                }
            }
            pts[pts.len() - 1].1 as f32
        };
        let gate = [(0.0, 0.0), (1e-6, 0.0), (1.02e-6, 10.0), (6e-6, 10.0), (6.02e-6, 0.0), (10e-6, 0.0), (10.02e-6, 10.0), (12e-6, 10.0), (12.02e-6, 0.0)];
        let id = [(0.0, 0.0), (1.1e-6, 0.0), (1.2e-6, 10.0), (6.05e-6, 10.0), (6.15e-6, 0.0), (10.05e-6, 0.0), (10.15e-6, 10.0), (10.25e-6, 14.0), (10.35e-6, 10.0), (12e-6, 10.0)];
        let vds = [(0.0, 400.0), (1.05e-6, 400.0), (1.15e-6, 1.0), (6.1e-6, 1.0), (6.2e-6, 400.0), (6.25e-6, 480.0), (6.4e-6, 400.0), (10.05e-6, 400.0), (10.15e-6, 1.0), (12e-6, 1.0)];
        put(&mut ui, 3, (0..n).map(|k| pwl(&vds, k as f64 * dt)).collect(), dt);
        put(&mut ui, 4, (0..n).map(|k| pwl(&id, k as f64 * dt)).collect(), dt);
        put(&mut ui, 5, (0..n).map(|k| pwl(&gate, k as f64 * dt)).collect(), dt);
        ui.meas_src = WaveId::Ref(3);
        ui.meas_src_b = WaveId::Ref(4);
        ui.cat_c = WaveId::Ref(5);
        ui.catalog_add(Cat::Vpeak);
        let peak = ui.meas_rows().into_iter().find(|r| r.name == "Vpeak").unwrap();
        assert!((peak.v - 480.0).abs() < 1.0, "vpeak {}", peak.v);
    }

    #[test]
    fn catalog_screenshots() {
        let Ok(dir) = std::env::var("OOS_CATALOG_DIR") else { return };
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for _ in 0..4 {
            ui.step(0.05);
        }
        ui.open_menu(MenuId::MeasAdd);
        ui.meas_src = WaveId::Ref(1);
        ui.cat_tab = 0;
        ui.cat_sel = Cat::Amplitude as u16;
        ui.cat_open[0] = 1;
        ui.cat_scroll = 0;
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/oos-standard.png"), crate::store::png_of(&cv)).unwrap();
        ui.cat_tab = 1;
        ui.cat_sel = Cat::JitterSummary as u16;
        ui.cat_open[1] = 1;
        ui.cat_scroll = 0;
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/oos-jitter.png"), crate::store::png_of(&cv)).unwrap();
        ui.cat_tab = 2;
        ui.cat_sel = Cat::PowerQuality as u16;
        ui.cat_open[2] = 1;
        ui.cat_scroll = 0;
        ui.meas_src = WaveId::Ch(0);
        ui.meas_src_b = WaveId::Ch(1);
        ui.render(&mut cv);
        std::fs::write(format!("{dir}/oos-power.png"), crate::store::png_of(&cv)).unwrap();
    }
}
