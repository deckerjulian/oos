// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The settings of one input channel (coupling, unit, scale, offset, bandwidth limit), the buses drawn on the plot, and
//! how the model fits the channels and sources of the board it runs on. A new channel setting goes here, with its
//! field in `Channel`.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Coupling {
    Dc,
    Ac,
    Gnd,
    /// DC into 50 ohms, on boards that offer it (`Board::couplings`)
    Dc50,
}

impl Coupling {
    pub fn name(self) -> &'static str {
        match self {
            Coupling::Dc => "DC",
            Coupling::Ac => "AC",
            Coupling::Gnd => "GND",
            Coupling::Dc50 => "DC50",
        }
    }
}

/// Deskew range (s).
pub const SKEW_MAX: f32 = 100e-9;

/// Unit of a channel.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AmpUnit {
    V,
    A,
    W,
    U,
}

impl AmpUnit {
    pub const ALL: [AmpUnit; 4] = [AmpUnit::V, AmpUnit::A, AmpUnit::W, AmpUnit::U];
    pub fn name(self) -> &'static str {
        ["V", "A", "W", "U"][self as usize]
    }
    /// A measurement's unit on such a channel: the V in it replaced.
    pub fn of(self, unit: &'static str) -> &'static str {
        match (self, unit) {
            (AmpUnit::A, "V") => "A",
            (AmpUnit::A, "Vs") => "As",
            (AmpUnit::A, "V/s") => "A/s",
            (AmpUnit::W, "V") => "W",
            (AmpUnit::W, "Vs") => "Ws",
            (AmpUnit::W, "V/s") => "W/s",
            (AmpUnit::U, "V") => "U",
            (AmpUnit::U, "Vs") => "Us",
            (AmpUnit::U, "V/s") => "U/s",
            _ => unit,
        }
    }
}

/// What stays put on the screen when the time base changes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HExpand {
    Center,
    Left,
    Right,
    Trigger,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Channel {
    pub on: bool,
    pub vscale: usize, // index into VSCALES
    /// fine scale: factor on the step of `vscale`, 1.0 = exactly on the step
    pub vfine: f32,
    /// what the channel measures: only the unit it shows
    pub unit: AmpUnit,
    /// zero correction in V: subtracted from what the channel reads
    pub bias: f32,
    /// deskew in s
    pub skew: f32,
    pub offset: f32, // volts added before display (vertical position)
    pub coupling: Coupling,
    /// bandwidth limit, index into BWS
    pub bw: usize,
    pub probe: f32,
    pub invert: bool,
}

impl Channel {
    pub fn volts_per_div(&self) -> f32 {
        VSCALES[self.vscale] * self.vfine * self.probe
    }

    /// Largest offset of the scale at the probe tip: the input gain path has its own range (afe-settings.md 3.2), the
    /// file's limits vary a little from block to block.
    pub fn offset_max(&self) -> f32 {
        let bnc = VSCALES[self.vscale] * self.vfine;
        self.probe
            * if bnc < 0.06 {
                2.0
            } else if bnc < 0.26 {
                12.0
            } else if bnc < 2.65 {
                56.0
            } else {
                550.0
            }
    }
}

/// How far a bus may sit from the middle of the grid, in divisions. Positive is up.
pub const BUS_POS_MIN: f32 = -3.6;
pub const BUS_POS_MAX: f32 = 3.6;

/// The bus drawn on the plot. Only I2C is defined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusKind {
    I2c,
}

impl BusKind {
    pub fn name(self) -> &'static str {
        match self {
            BusKind::I2c => "I2C",
        }
    }
}

/// One bus on the waveform: on or off, a label, and a vertical position in divisions.
#[derive(Clone, PartialEq, Debug)]
pub struct TraceBus {
    pub on: bool,
    pub label: String,
    /// divisions from the center of the grid, positive up
    pub pos: f32,
    pub kind: BusKind,
    /// clock (SCL) and data (SDA)
    pub clk: WaveId,
    pub data: WaveId,
    pub clk_thr: f32,
    pub data_thr: f32,
    /// the address word includes the read/write bit as its least significant bit
    pub addr_rw: bool,
}

impl Default for TraceBus {
    fn default() -> Self {
        Self {
            on: false,
            label: "I2C".into(),
            pos: -3.0,
            kind: BusKind::I2c,
            clk: WaveId::Ch(0),
            data: WaveId::Ch(1),
            clk_thr: 1.65,
            data_thr: 1.65,
            addr_rw: false,
        }
    }
}

impl TraceBus {
    /// The name drawn on the plot. An empty label falls back to the type.
    pub fn shown_label(&self) -> String {
        let t = self.label.trim();
        if t.is_empty() {
            self.kind.name().to_string()
        } else {
            t.to_string()
        }
    }
}

/// Keeps a bus label to the length of a channel name, without control characters.
pub fn bus_label(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(10).collect::<String>().trim().to_string()
}

/// Bandwidth limits of a channel (AFE low-pass): Auto = filter ~ fs / 4 as the
/// default, Full = widest filter (stock "OFF", 3150 MHz in the AFE), GND = no signal through the AFE; then the AFE
/// filter frequency in MHz (`board::dho::hw::bw_code`).
pub const BWS: [(&str, u32); 31] = [
    ("Auto", 0),
    ("Full", 0),
    ("20 MHz", 20),
    ("GND", 0),
    ("5 MHz", 5),
    ("10 MHz", 10),
    ("15 MHz", 15),
    ("25 MHz", 25),
    ("30 MHz", 30),
    ("35 MHz", 35),
    ("40 MHz", 40),
    ("50 MHz", 50),
    ("60 MHz", 60),
    ("70 MHz", 70),
    ("75 MHz", 75),
    ("80 MHz", 80),
    ("90 MHz", 90),
    ("100 MHz", 100),
    ("125 MHz", 125),
    ("150 MHz", 150),
    ("175 MHz", 175),
    ("200 MHz", 200),
    ("250 MHz", 250),
    ("300 MHz", 300),
    ("310 MHz", 310),
    ("400 MHz", 400),
    ("600 MHz", 600),
    ("800 MHz", 800),
    ("1 GHz", 1000),
    ("1.5 GHz", 1500),
    ("2 GHz", 2000),
];
/// Index of "20 MHz" in BWS (old setups, SCPI 20M).
pub const BW_20M: usize = 2;
/// Index of "Full" in BWS.
pub const BW_FULL: usize = 1;
/// Index of "Auto" in BWS: the filter follows the sample rate (the default, as on the stock system; with Full the
/// noise at slow time bases can exceed the trigger hysteresis, trigger.md 7).
pub const BW_AUTO: usize = 0;

impl Model {
    /// Keeps every offset within what the AFE of its scale can do.
    pub fn clamp_offsets(&mut self) {
        for ch in &mut self.ch {
            let m = ch.offset_max();
            ch.offset = ch.offset.clamp(-m, m);
        }
    }

    /// Unit of channel `c`.
    pub fn unit(&self, c: usize) -> &'static str {
        self.ch[c.min(NCH - 1)].unit.name()
    }

    pub fn n_on(&self) -> usize {
        self.ch.iter().take(n_ch()).filter(|c| c.on).count()
    }
    /// Switches off the inputs the model does not have (2-channel models: CH3/CH4), whatever turned them on
    /// (setups, autoset, demos, SCPI). Returns true if something changed.
    pub fn fit_platform(&mut self) -> bool {
        let n = n_ch();
        let mut changed = false;
        // what this board does not have: a coupling from a setup of another board, a generator it lacks
        for ch in &mut self.ch {
            if !board().couplings().contains(&ch.coupling) {
                ch.coupling = Coupling::Dc;
                changed = true;
            }
        }
        if self.gen_sel >= gen_count().max(1) {
            self.gen_select(0);
            changed = true;
        }
        for c in n..NCH {
            changed |= std::mem::replace(&mut self.ch[c].on, false);
        }
        if self.active >= n {
            self.active = 0;
            changed = true;
        }
        // models without the LA port: no logic analyser
        if self.la.on && !crate::model::la::available() {
            self.la.on = false;
            changed = true;
        }
        if n >= NCH {
            return changed;
        }
        changed = self.fit_wave_sources(n, changed);
        self.fit_meas_sources(n, changed)
    }

    /// `fit_platform` for the Bode, trigger, mask, zone, analyser, decoder and Z input sources on a missing input.
    fn fit_wave_sources(&mut self, n: usize, mut changed: bool) -> bool {
        // the Bode sync channel goes to the last input (CH4 by default, CH2 on a 2-channel model), its input to none
        if self.bode_sync >= n {
            self.bode_sync = n - 1;
            changed = true;
        }
        if self.bode_sw_in > n {
            self.bode_sw_in = 0;
            changed = true;
        }
        // every other channel source that points at a missing input: CH1, or none for the optional ones
        for s in [&mut self.trig_src, &mut self.trig_src2, &mut self.mask_src, &mut self.bw_src, &mut self.afg_arb_src]
            .into_iter()
            .chain(self.zones.iter_mut().map(|z| &mut z.src))
        {
            if *s >= n {
                *s = 0;
                changed = true;
            }
        }
        for s in [
            &mut self.mag_v,
            &mut self.mag_i,
            &mut self.inv_a,
            &mut self.inv_b,
            &mut self.inv_c,
            &mut self.inv_an,
            &mut self.dpt_v,
            &mut self.dpt_i,
            &mut self.dpt_g,
            &mut self.dpt_4ch,
            &mut self.spec_src,
            &mut self.eye_src,
            &mut self.pw_v,
            &mut self.pw_i,
            &mut self.bode_in,
            &mut self.bode_out,
        ] {
            if let WaveId::Ch(c) = *s {
                if c >= n {
                    *s = WaveId::Ch(0);
                    changed = true;
                }
            }
        }
        for s in [&mut self.dec_src, &mut self.dec_src2] {
            if decode_wave(*s).is_none() || (*s < NCH && *s >= n) {
                *s = 0;
                changed = true;
            }
        }
        for s in [&mut self.dec_src4, &mut self.dec_diff, &mut self.dec_cs] {
            if s.is_some_and(|c| decode_wave(c).is_none() || (c < NCH && c >= n)) {
                *s = None;
                changed = true;
            }
        }
        if self.z_src.is_some_and(|c| c >= n) {
            self.z_src = None;
            changed = true;
        }
        changed
    }

    /// `fit_platform` for the measurement, catalogue, analyser and math sources on a missing input.
    fn fit_meas_sources(&mut self, n: usize, mut changed: bool) -> bool {
        for e in &mut self.meas {
            if let WaveId::Ch(c) = e.1 {
                if c >= n {
                    e.1 = WaveId::Ch(0);
                    changed = true;
                }
            }
        }
        for e in &mut self.meas2 {
            for id in [&mut e.1, &mut e.2] {
                if let WaveId::Ch(c) = *id {
                    if c >= n {
                        *id = WaveId::Ch(0);
                        changed = true;
                    }
                }
            }
        }
        let previous_cat = self.cat.len();
        self.cat.retain(|row| catalog_id_valid(row.0));
        self.cat.truncate(MEAS_SELECTION_MAX.saturating_sub(self.meas.len() + self.meas2.len()));
        changed |= previous_cat != self.cat.len();
        for row in &mut self.cat {
            for source in [&mut row.1, &mut row.2, &mut row.3, &mut row.4] {
                if let WaveId::Ch(c) = *source {
                    if c >= n {
                        *source = WaveId::Ch(0);
                        changed = true;
                    }
                } else if matches!(*source, WaveId::Ref(i) if i >= NREF) {
                    *source = WaveId::Ref(0);
                    changed = true;
                }
            }
        }
        for s in [
            &mut self.inv_tq,
            &mut self.inv_ev,
            &mut self.inv_ei,
            &mut self.eye_b,
            &mut self.eye_rail,
            &mut self.pw_vin,
            &mut self.pw_clk,
        ] {
            if let Some(WaveId::Ch(c)) = *s {
                if c >= n {
                    *s = None;
                    changed = true;
                }
            }
        }
        // math sources: channels 0..NCH, then math channels (always there)
        for s in [&mut self.math_a, &mut self.math_b]
            .into_iter()
            .chain(self.maths.iter_mut().flat_map(|m| [&mut m.a, &mut m.b]))
        {
            if !src_ok(*s) {
                *s = 0;
                changed = true;
            }
        }
        changed
    }
}
