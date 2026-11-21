// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The generator presets of the Bode plot (ui/bode.rs): a setup keeps the index of one (`bode`, its seventh value).

use oos_dsp::bode;

/// A generator preset. The commands may hold several lines (`\n`), each sent as a command of its own; `{f}` is the
/// frequency in Hz, `{a}` the amplitude in Vpp.
pub struct Gen {
    pub name: &'static str,
    /// sent once at the start, before the first frequency
    pub setup: &'static str,
    /// sent for every frequency
    pub step: &'static str,
    /// sent once after the first frequency
    pub on: &'static str,
}

const fn gen(name: &'static str, setup: &'static str, step: &'static str, on: &'static str) -> Gen {
    Gen { name, setup, step, on }
}

/// Generator presets. Entry `BUILTIN` is the scope's own, entry `FREE` a generator that sweeps on its own (no
/// commands). The setup file keeps the index: new presets go at the end.
///
/// CHANnel (UTG900 series generators, UTG932, UTG962): commands from the "UTG900 Series Programmable Signal Source
/// Programming Manual" REV 00, 2022-07-15 (Chinese, instrument software V3.06.004), the same in the English "UTG900E
/// Series Programming Manual" of 2022-07-15: `:CHANnel<n>:MODe CONTINUE`, `:CHANnel<n>:BASE:WAVe SINe`, `:CHANnel<n>:AMPLitude:UNIT VPP`,
/// `:CHANnel<n>:BASE:AMPLitude`, `:CHANnel<n>:BASE:OFFSet`, `:CHANnel<n>:BASE:FREQuency` (Hz), `:CHANnel<n>:OUTPut ON`;
/// every command ends with a newline. Not yet tried with a real UTG900.
pub const GENERATORS: [Gen; 6] = [
    gen("Built-in", "", "", ""),
    gen("APPL", "", ":SOUR1:APPL:SIN {f},{a},0", ":OUTP1 ON"),
    gen("BSWV", "", "C1:BSWV WVTP,SINE,FRQ,{f},AMP,{a},OFST,0", "C1:OUTP ON"),
    gen("SCPI", "", "FREQ {f};VOLT {a}", "OUTP ON"),
    gen("Free-running sweep", "", "", ""),
    gen(
        "CHANnel",
        ":CHANnel1:MODe CONTINUE\n:CHANnel1:BASE:WAVe SINe\n:CHANnel1:AMPLitude:UNIT VPP\n:CHANnel1:BASE:AMPLitude {a}\n:CHANnel1:BASE:OFFSet 0",
        ":CHANnel1:BASE:FREQuency {f}",
        ":CHANnel1:OUTPut ON",
    ),
];

/// The scope's own generator, and one that sweeps on its own (indices of `GENERATORS`).
pub const BUILTIN: usize = 0;
pub const FREE: usize = 4;

/// The command lines of a preset template for `f` Hz and `a` Vpp.
pub fn gen_lines(tpl: &str, f: f64, a: f64) -> Vec<String> {
    tpl.split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.replace("{f}", &bode::fmt_hz(f)).replace("{a}", &bode::fmt_hz(a)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utg900_preset_commands() {
        let g = GENERATORS.iter().position(|g| g.name == "CHANnel").unwrap();
        // the setup file keeps the index: the older presets keep theirs
        assert_eq!((g, GENERATORS[BUILTIN].name, GENERATORS[FREE].name), (5, "Built-in", "Free-running sweep"));
        let g = &GENERATORS[g];
        assert_eq!(
            gen_lines(g.setup, 100.0, 2.0),
            vec![
                ":CHANnel1:MODe CONTINUE",
                ":CHANnel1:BASE:WAVe SINe",
                ":CHANnel1:AMPLitude:UNIT VPP",
                ":CHANnel1:BASE:AMPLitude 2",
                ":CHANnel1:BASE:OFFSet 0"
            ]
        );
        assert_eq!(gen_lines(g.step, 12500.0, 2.0), vec![":CHANnel1:BASE:FREQuency 12500"]);
        assert_eq!(gen_lines(g.on, 1.0, 2.0), vec![":CHANnel1:OUTPut ON"]);
        // the older presets: one line per step, nothing before
        assert_eq!(gen_lines(GENERATORS[1].step, 1e3, 1.0), vec![":SOUR1:APPL:SIN 1000,1,0"]);
        assert!(gen_lines(GENERATORS[1].setup, 1e3, 1.0).is_empty());
    }
}
