// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the board of the build allows of the model (ADR 0049): the model holds the settings and names no board; the
//! methods that ask the board what it has (couplings, the generator's range, the trigger comparator's hysteresis) are
//! here, in the signal layer, which may name both. The counts the model is sized by (`NCH`, `NGEN`) are held to the
//! board's at compile time. They are a trait, [`Platform`], since the model's types can have methods only where they
//! are declared (ADR 0064: the model becomes a crate of its own).

use crate::board::{board, Active, Board};
use crate::model::*;

const _: () = assert!(<Active as Board>::MAX_CHANNELS == NCH, "the model has as many channel slots as the board");
const _: () = assert!(<Active as Board>::MAX_GENERATORS == NGEN, "the model has as many generators as the board");

/// What the board of the build allows of the model.
pub trait Platform {
    /// Switches off the inputs the model does not have (2-channel models: CH3/CH4), whatever turned them on
    /// (setups, autoset, demos, SCPI). Returns true if something changed.
    fn fit_platform(&mut self) -> bool;

    /// Highest frequency (Hz) of the selected generator's wave shape.
    fn gen_max_hz(&self) -> f64;

    /// Comparator hysteresis of the trigger in divisions, as the hardware gets it (trigger.md 2.3): 0.3, 0.7 with
    /// noise reject, more at 2 mV/div and below (the table of the trigger source's scale at the BNC).
    fn trig_hyst_div(&self) -> f32;
}

impl Platform for Model {
    fn fit_platform(&mut self) -> bool {
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
        crate::measures::fit_sources(self, n, changed)
    }

    fn gen_max_hz(&self) -> f64 {
        board().gen_max_hz(&self.gen_read())
    }

    fn trig_hyst_div(&self) -> f32 {
        let c = &self.ch[self.trig_src.min(NCH - 1)];
        board().trig_hyst_div(c.volts_per_div() as f64 / c.probe as f64, self.trig_nrej) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setup_of_another_board_fits_this_one() {
        let mut m = Model::default();
        assert!(!m.fit_platform());
        if !board().couplings().contains(&Coupling::Dc50) {
            m.ch[1].coupling = Coupling::Dc50;
            assert!(m.fit_platform());
            assert_eq!(m.ch[1].coupling, Coupling::Dc);
        }
    }

    #[test]
    fn four_channel_unit_keeps_everything() {
        // the default platform (host tests) is the DHO924S
        let mut m = Model::default();
        m.ch[3].on = true;
        m.dec_src = 3;
        assert!(!m.fit_platform());
        assert!(m.ch[3].on && m.dec_src == 3);
        assert_eq!(ch_names().len(), 4);
    }

    #[test]
    fn the_boards_lists_are_usable() {
        let d = mdepths();
        assert_eq!(d[0].1, 0, "Auto first");
        assert!(d.windows(2).all(|w| w[0].1 < w[1].1), "ascending");
        assert_eq!(
            d[d.len() - 1].1,
            oos_hal::model::Limits::standard().max_mem[0],
            "the list ends at the largest depth"
        );
        assert!(board().couplings().contains(&Coupling::Dc), "the fallback of fit_platform");
        assert!(n_ch() <= NCH && gen_count() <= NGEN);
    }
}
