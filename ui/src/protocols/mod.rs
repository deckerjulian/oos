// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Protocol modules: a decoder with everything the scope needs around it, behind one registration.
//!
//! A protocol module is one file in this directory with a type that implements [`Protocol`], and one line in
//! [`MODULES`]. That is all: the Decode menu (protocol list, source rows named after the roles, option rows), saved
//! setups, the protocol trigger (bit time, what a frame start is, the starting value), the packet labels on the trace
//! and in the event table, the demo traffic of the host build and the protocol's own view in the pane under the plot
//! all come from the trait. The decoder itself usually lives in `oos-dsp` (decode + a generator for its tests) and the
//! module calls it.
//!
//! The protocols that existed before this interface are still wired by hand through `model::Proto`; they move here one
//! by one. A new protocol should come as a module. `docs/protocol-modules.md` walks through writing one, and
//! `example.rs` (compiled in the tests) uses every hook.

use crate::model::{BusWhat, Slope};
use oos_dsp::decode::Packet;

mod dcc;
mod dmx512;
#[cfg(test)]
pub mod example;
mod spdif;
mod usbpd;

/// The registered modules. The order is the order in the Decode menu, after the built-in protocols. Never reuse or
/// change a module's `key`: saved setups refer to it.
pub static MODULES: &[&dyn Protocol] = &[
    &dmx512::Dmx512,
    &spdif::Spdif,
    &usbpd::UsbPd,
    &dcc::Dcc,
    #[cfg(test)]
    &example::Example,
];

/// Option rows a module may have (bit rate, word size, variant ...).
pub const OPTS: usize = 4;

/// One option row of the Decode menu: a name and its choices; the setting is the index of the choice.
pub struct Opt {
    pub name: &'static str,
    pub choices: &'static [&'static str],
    pub default: usize,
}

/// The lines a decoder runs on, as voltages, in the order of [`Protocol::roles`]. `lines[0]` is always there (the main
/// source; with "B line (A-B)" the difference of the pair); a role the protocol does not name, or an optional role
/// set to "none", is `None`.
pub struct Lines<'a> {
    pub lines: [Option<&'a [f32]>; 4],
    /// sample spacing (s)
    pub dt: f64,
    /// decision threshold and hysteresis (V)
    pub thr: f32,
    pub hy: f32,
    /// the option settings, indices into [`Opt::choices`], already in range
    #[allow(dead_code)] // interface: no module of the scope build has options yet
    pub opt: [usize; OPTS],
}

impl<'a> Lines<'a> {
    /// The main line.
    pub fn main(&self) -> &'a [f32] {
        self.lines[0].unwrap_or(&[])
    }

    /// Line `k`, or the error the scope shows when it is missing.
    #[allow(dead_code)] // interface: no module of the scope build has a second line yet
    pub fn need(&self, k: usize, what: &'static str) -> Result<&'a [f32], &'static str> {
        self.lines[k].ok_or(what)
    }
}

/// Where the demo traffic sits and what the decoder starts with when the protocol is chosen.
pub struct Setup {
    /// channel per role (0 = CH1); `None` leaves an optional role unset
    pub src: [Option<usize>; 4],
    /// logic analyser line per role, for the LA input
    pub la: [Option<u8>; 4],
    /// decision threshold (V)
    pub thr: f32,
}

impl Default for Setup {
    /// Main line on CH3 and a second one on CH4, 3.3 V logic; the demo UART line D8 and clock D10 for the LA.
    fn default() -> Setup {
        Setup { src: [Some(2), Some(3), None, None], la: [Some(8), Some(10), None, None], thr: 1.65 }
    }
}

/// A line of demo traffic: voltage at time `t` (s) within one period.
pub type DemoLine = Box<dyn Fn(f64) -> f64 + Send + Sync>;

/// Demo traffic of the host build: one waveform per role, on the channels of [`Protocol::setup`], repeated every
/// `period` seconds.
pub struct Demo {
    pub period: f64,
    pub lines: [Option<DemoLine>; 4],
}

/// What a module keeps for its view from one record to the next (the devices seen so far ...). The scope gives each
/// decoder its own, empty at first, and empties it when the protocol changes or the display is cleared.
pub type Memory = Option<Box<dyn std::any::Any + Send>>;

/// The memory of a view as the module's own type `T`, made with `T::default()` when it is empty or of another type.
pub fn memory<T: Default + Send + 'static>(m: &mut Memory) -> &mut T {
    if !m.as_ref().is_some_and(|b| b.is::<T>()) {
        *m = Some(Box::new(T::default()));
    }
    m.as_mut().and_then(|b| b.downcast_mut::<T>()).unwrap()
}

/// Colour hint of a view's status line.
#[allow(dead_code)] // interface: no module of the scope build has a view yet
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Good,
    Bad,
}

/// A protocol's own picture of what it decoded, in the pane under the plot (Display > Pane > Bus view): a status line
/// and columns of text rows, for example the devices on a bus and their last state.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct View {
    pub status: Option<(String, Tone)>,
    pub sections: Vec<Section>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Section {
    pub title: String,
    pub rows: Vec<String>,
}

/// A protocol module. Only `key`, `name`, `bit_time` and `decode` are required; everything else has a default that
/// fits a single-line serial protocol.
pub trait Protocol: Sync {
    /// Stable name in saved setups and over SCPI: letters and digits only, never changed once released.
    fn key(&self) -> &'static str;

    /// Name in the Decode menu, in the event table and on the decoder's label.
    fn name(&self) -> &'static str;

    /// Names of the source rows: main, second, third (optional), fourth (optional). `None` = the row is not shown.
    fn roles(&self) -> [Option<&'static str>; 4] {
        [Some("Source"), None, None, None]
    }

    /// Whether the main line may be a differential pair (RS-485 style A - B).
    fn diff_ok(&self) -> bool {
        false
    }

    /// The option rows of the Decode menu.
    fn options(&self) -> &'static [Opt] {
        &[]
    }

    /// Bit time (s) with these options: the protocol trigger samples at a twentieth of it and searches 200 of them.
    fn bit_time(&self, opt: &[usize; OPTS]) -> f64;

    /// Record length (s) the protocol trigger needs before the trigger point, when 200 bit times are not right.
    fn frame_span(&self, _opt: &[usize; OPTS]) -> Option<f64> {
        None
    }

    /// Decodes the lines. Packet times are fractional sample indices.
    fn decode(&self, l: &Lines) -> Result<Vec<Packet>, &'static str>;

    /// Text of a decoded field on the trace and in the event table; `None` = the generic text of its kind.
    fn text(&self, _p: &Packet, _opt: &[usize; OPTS]) -> Option<String> {
        None
    }

    /// The event table (and its CSV, `:...EVENts?`): one row per command, as (time of its first packet, text, check
    /// passed), instead of one row per field. `None` = a row per field with `text`.
    fn events(&self, _p: &[Packet], _opt: &[usize; OPTS]) -> Option<Vec<(f64, String, bool)>> {
        None
    }

    /// Protocol trigger: whether `p` satisfies "`what` = `value`"; `None` = the generic rule (frame starts, IDs and
    /// addresses, data, failed checks).
    fn trigger_match(&self, _what: BusWhat, _value: u32, _p: &Packet) -> Option<bool> {
        None
    }

    /// What the protocol trigger looks for when it is switched on: a value the demo traffic contains.
    fn trigger_default(&self) -> (BusWhat, u32) {
        (BusWhat::Frame, 0x40)
    }

    /// The edge a frame starts with, for the hardware trigger in front of the protocol trigger.
    fn start_edge(&self) -> Slope {
        Slope::Fall
    }

    /// Sources and threshold when the protocol is chosen.
    fn setup(&self) -> Setup {
        Setup::default()
    }

    /// Demo traffic for the host build; built once.
    fn demo(&self) -> Option<Demo> {
        None
    }

    /// The protocol's own view of the decoded packets of a new record (`None` = no view). Called once per record;
    /// `mem` carries what the module wants to keep across records. A module with a view gets the Bus view pane opened
    /// when it is chosen and no pane is open.
    fn view(&self, _p: &[Packet], _opt: &[usize; OPTS], _mem: &mut Memory) -> Option<View> {
        None
    }
}

/// Module `i` of [`MODULES`].
pub fn get(i: u8) -> &'static dyn Protocol {
    MODULES[i as usize]
}

/// The option settings of module `i` brought into range (a setup from another version may hold anything).
pub fn opt_fit(i: u8, opt: [u8; OPTS]) -> [usize; OPTS] {
    let o = get(i).options();
    std::array::from_fn(|k| o.get(k).map_or(0, |x| (opt[k] as usize).min(x.choices.len().saturating_sub(1))))
}

/// The default option settings of module `i`.
pub fn opt_default(i: u8) -> [u8; OPTS] {
    let o = get(i).options();
    std::array::from_fn(|k| o.get(k).map_or(0, |x| x.default as u8))
}

/// The demo traffic of module `i`, built on first use.
pub fn demo(i: u8) -> Option<&'static Demo> {
    static D: std::sync::OnceLock<Vec<Option<Demo>>> = std::sync::OnceLock::new();
    D.get_or_init(|| MODULES.iter().map(|m| m.demo()).collect())[i as usize].as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys are unique and safe for setup files; names are unique; option defaults are in range; every role a module
    /// names has a demo channel when the module has demo traffic.
    #[test]
    fn modules_are_well_formed() {
        let mut keys = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for m in MODULES {
            assert!(!m.key().is_empty() && m.key().chars().all(|c| c.is_ascii_alphanumeric()), "key {:?}", m.key());
            assert!(keys.insert(m.key()), "key {} twice", m.key());
            assert!(names.insert(m.name()), "name {} twice", m.name());
            assert!(m.options().len() <= OPTS, "{}: more than {} options", m.name(), OPTS);
            for o in m.options() {
                assert!(o.default < o.choices.len(), "{}: {} default", m.name(), o.name);
            }
            assert!(m.roles()[0].is_some(), "{}: no main role", m.name());
            assert!(m.bit_time(&[0; OPTS]) > 0.0, "{}: bit time", m.name());
        }
        // the built-in protocols must not take a module's key or name
        for p in crate::model::BUILTIN_PROTOS {
            assert!(!keys.contains(format!("{:?}", p).as_str()) && !names.contains(p.name()), "{:?} clashes with a module", p);
        }
    }

    /// Every module with demo traffic decodes it without errors and finds something.
    #[test]
    fn modules_decode_their_demo() {
        for (i, m) in MODULES.iter().enumerate() {
            let Some(d) = demo(i as u8) else { continue };
            let dt = m.bit_time(&opt_fit(i as u8, opt_default(i as u8))) / 40.0;
            let n = (3.0 * d.period / dt) as usize;
            let waves: Vec<Option<Vec<f32>>> = d.lines.iter().map(|l| l.as_ref().map(|f| (0..n).map(|k| f((k as f64 * dt).rem_euclid(d.period)) as f32).collect())).collect();
            let s = m.setup();
            let l = Lines { lines: std::array::from_fn(|k| waves[k].as_deref()), dt, thr: s.thr, hy: 0.1, opt: opt_fit(i as u8, opt_default(i as u8)) };
            let p = m.decode(&l).unwrap_or_else(|e| panic!("{}: {}", m.name(), e));
            assert!(!p.is_empty(), "{}: nothing decoded from its demo", m.name());
            assert!(p.iter().all(|x| x.ok), "{}: errors in its own demo: {:?}", m.name(), p.iter().find(|x| !x.ok));
        }
    }
}
