// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The built-in generators (one settings bank each, the selected one in the `afg_*` fields) and the SIM preset that
//! fills the serial-lane knobs. A new generator setting goes here, with its field in `GenCh`.

use super::*;

/// The settings of one built-in generator. The selected one is in the `afg_*` fields of the model, the others in
/// `Model::gen_bank` (`Model::gen_select`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenCh {
    pub on: bool,
    pub arb: u32,
    pub arb_src: usize,
    pub freq: f64,
    pub ampl: f64,
    pub offset: f64,
    pub wave: usize,
    pub duty: f64,
    pub symmetry: f64,
    pub modulation: usize,
    pub mod_rate: f64,
    pub mod_depth: f64,
    pub fm_dev: f64,
    pub pm_dev: f64,
    pub mod_shape: usize,
}

impl GenCh {
    pub const STANDARD: GenCh = GenCh {
        on: false,
        arb: 0,
        arb_src: 0,
        freq: 1000.0,
        ampl: 1.0,
        offset: 0.0,
        wave: 0,
        duty: 50.0,
        symmetry: 50.0,
        modulation: 0,
        mod_rate: 100.0,
        mod_depth: 100.0,
        fm_dev: 100.0,
        pm_dev: 90.0,
        mod_shape: 0,
    };
}

/// Wave shapes of the built-in generator (`Board::gen`).
pub const AFG_WAVES: [&str; 7] = ["Sine", "Square", "Ramp", "Pulse", "Noise", "DC", "Arb"];
/// Indices into AFG_WAVES the generator page treats apart.
pub const AFG_NOISE: usize = 4;
pub const AFG_DC: usize = 5;
/// Modulations and modulating shapes of the built-in generator (`Board::gen`).
pub const AFG_MODS: [&str; 4] = ["Off", "AM", "FM", "PM"];
pub const AFG_MOD_SHAPES: [&str; 6] = ["Sine", "Square", "Triangle", "Up ramp", "Down ramp", "Noise"];

impl Model {
    /// Copies the selected SIM preset into the knobs. Play then uses the knobs, not the table row itself.
    pub fn apply_sim_preset(&mut self) {
        let l = oos_dsp::serial::LANES[self.sim_preset.min(oos_dsp::serial::LANES.len() - 1)];
        self.sim_rate = l.bits_per_s;
        self.sim_prbs = l.prbs;
        self.sim_vhigh = l.v_high;
        self.sim_vlow = l.v_low;
        self.sim_rise = l.rise;
        self.sim_noise = l.noise_rms;
        self.sim_bw = l.bw;
        self.sim_deemph = l.deemph_db;
        self.sim_ssc = l.ssc;
        self.sim_ssc_center = l.ssc_center;
        self.sim_ssc_deviation = l.ssc_deviation;
        self.sim_ssc_hz = l.ssc_hz;
        self.sim_rj = l.rj_rms;
        self.sim_pj = l.pj_peak;
        self.sim_pjhz = l.pj_hz;
        self.sim_dcd_pp = l.dcd_pp;
        self.sim_corr_rj = l.corr_rj_rms;
        self.sim_corr_hz = l.corr_rj_hz;
    }

    /// The settings of the selected generator.
    pub fn gen_read(&self) -> GenCh {
        GenCh {
            on: self.afg_on,
            arb: self.afg_arb,
            arb_src: self.afg_arb_src,
            freq: self.afg_freq,
            ampl: self.afg_ampl,
            offset: self.afg_offset,
            wave: self.afg_wave,
            duty: self.afg_duty,
            symmetry: self.afg_symmetry,
            modulation: self.afg_mod,
            mod_rate: self.afg_mod_rate,
            mod_depth: self.afg_mod_depth,
            fm_dev: self.afg_fm_dev,
            pm_dev: self.afg_pm_dev,
            mod_shape: self.afg_mod_shape,
        }
    }

    fn gen_write(&mut self, g: GenCh) {
        (self.afg_on, self.afg_arb, self.afg_arb_src) = (g.on, g.arb, g.arb_src);
        (self.afg_freq, self.afg_ampl, self.afg_offset) = (g.freq, g.ampl, g.offset);
        (self.afg_wave, self.afg_duty, self.afg_symmetry) = (g.wave, g.duty, g.symmetry);
        (self.afg_mod, self.afg_mod_rate, self.afg_mod_depth) = (g.modulation, g.mod_rate, g.mod_depth);
        (self.afg_fm_dev, self.afg_pm_dev, self.afg_mod_shape) = (g.fm_dev, g.pm_dev, g.mod_shape);
    }

    /// The settings of generator `i`.
    pub fn gen_ch(&self, i: usize) -> GenCh {
        if i == self.gen_sel {
            self.gen_read()
        } else {
            self.gen_bank[i]
        }
    }

    /// Makes generator `i` the one the page and the `afg_*` fields work on.
    pub fn gen_select(&mut self, i: usize) {
        if i != self.gen_sel && i < NGEN {
            self.gen_bank[self.gen_sel] = self.gen_read();
            self.gen_write(self.gen_bank[i]);
            self.gen_sel = i;
        }
    }

    /// Highest frequency (Hz) of the selected generator's wave shape.
    pub fn gen_max_hz(&self) -> f64 {
        board().gen_max_hz(&self.gen_read())
    }
}
