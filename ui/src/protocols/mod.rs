// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Protocol modules: a decoder with everything the scope needs around it, behind one registration.
//!
//! A protocol module is one file in this directory with a type that implements [`Protocol`], and one line in
//! [`MODULES`]. That is all: the Decode menu (protocol list, source rows named after the roles, option rows), saved
//! setups, the packet labels on the trace and in the event table, the demo traffic of the host build and the
//! protocol's own view in the pane under the plot come from the trait. What the decoder is lives in `oos-dsp`: its
//! [`ModuleInfo`] (key, name, the lines it takes, the call of the decoder, its bit time, what the protocol trigger
//! finds and starts with) stands next to the decoder and is listed in `oos_dsp::protocols::MODULES`; the module names
//! it with [`Protocol::info`].
//!
//! Every decoder is a module. The seven core protocols (`core.rs`: UART, I2C, SPI, CAN, LIN, Parallel, C-PHY) are
//! always built and keep the first places, since the hardware trigger types and SCPI name them (`Proto::UART` ...).
//! Every other file is behind a Cargo feature (`dec-<file>`, all on by default), so a build can leave decoders out.
//! `docs/protocol-modules.md` walks through writing one, and `example.rs` (compiled in the tests) uses every hook.

use crate::model::Slope;
use oos_dsp::decode::Packet;
use oos_dsp::protocols::ModuleInfo;

#[cfg(feature = "dec-avionics")]
mod avionics;
mod core;
#[cfg(feature = "dec-dcc")]
mod dcc;
#[cfg(feature = "dec-debug")]
mod debug;
#[cfg(feature = "dec-dmx512")]
mod dmx512;
#[cfg(test)]
pub mod example;
#[cfg(feature = "dec-i2s")]
mod i2s;
#[cfg(feature = "dec-i3c")]
mod i3c;
#[cfg(feature = "dec-linecode")]
mod linecode;
#[cfg(feature = "dec-onewire")]
mod onewire;
#[cfg(feature = "dec-qspi")]
mod qspi;
#[cfg(feature = "dec-serial")]
mod serial;
#[cfg(feature = "dec-spdif")]
mod spdif;
#[cfg(feature = "dec-usb")]
mod usb;
#[cfg(feature = "dec-usbpd")]
mod usbpd;
#[cfg(feature = "dec-vehicle")]
mod vehicle;

/// The registered modules. The order is the order in the Decode menu; the core protocols come first, at fixed places.
/// Never reuse or change a module's `key`: saved setups refer to it. A module behind a feature that is off is missing
/// from the list, and a setup that names it loads without its decoder.
pub static MODULES: &[&dyn Protocol] = &[
    // the core decoders at fixed places (`Proto::UART` ...): never move them
    &core::Uart,
    &core::I2c,
    &core::Spi,
    &core::Can,
    &core::Lin,
    &core::Parallel,
    &core::Cphy,
    #[cfg(feature = "dec-dmx512")]
    &dmx512::Dmx512,
    #[cfg(feature = "dec-spdif")]
    &spdif::Spdif,
    #[cfg(feature = "dec-usbpd")]
    &usbpd::UsbPd,
    #[cfg(feature = "dec-dcc")]
    &dcc::Dcc,
    #[cfg(feature = "dec-linecode")]
    &linecode::Manchester,
    #[cfg(feature = "dec-linecode")]
    &linecode::Mlt3,
    #[cfg(feature = "dec-serial")]
    &serial::Uart9,
    #[cfg(feature = "dec-serial")]
    &serial::ModbusRtu,
    #[cfg(feature = "dec-serial")]
    &serial::ModbusAscii,
    #[cfg(feature = "dec-onewire")]
    &onewire::OneWire,
    #[cfg(feature = "dec-onewire")]
    &onewire::Ds2482,
    #[cfg(feature = "dec-debug")]
    &debug::Swd,
    #[cfg(feature = "dec-debug")]
    &debug::Jtag,
    #[cfg(feature = "dec-qspi")]
    &qspi::Qspi,
    #[cfg(feature = "dec-i3c")]
    &i3c::I3c,
    #[cfg(feature = "dec-vehicle")]
    &vehicle::CanFd,
    #[cfg(feature = "dec-vehicle")]
    &vehicle::FlexRay,
    #[cfg(feature = "dec-vehicle")]
    &vehicle::Sent,
    #[cfg(feature = "dec-avionics")]
    &avionics::Arinc429,
    #[cfg(feature = "dec-avionics")]
    &avionics::Mil1553,
    #[cfg(feature = "dec-i2s")]
    &i2s::I2s,
    #[cfg(feature = "dec-usb")]
    &usb::Usb,
    #[cfg(test)]
    &example::Example,
];

pub use oos_dsp::protocols::{Lines, Shared, OPTS};

/// A row of the Decode menu several protocols share, kept in its own field of the model (`dec_baud`, `dec_bits` ...):
/// SCPI (`:BUS<n>:RS232:BAUD` ...), the hardware protocol triggers and the setups use those fields by name, so a
/// protocol that has such a setting names the shared row ([`Protocol::shared`]) instead of an option of its own and
/// reads its value from [`Lines::sh`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Baud,
    Bits,
    Parity,
    /// SPI mode 0..3
    SpiMode,
    CanRate,
    /// CAN: the line is CAN-H (else CAN-L or a transceiver's RX)
    CanH,
    #[cfg_attr(not(feature = "decoders"), allow(dead_code))] // for the optional modules
    UsbSpeed,
    CphyRate,
    /// the event table in ASCII
    Fmt,
}

/// The values of the shared rows in `m`.
pub fn shared(m: &crate::model::Model) -> Shared {
    Shared {
        baud: crate::decode::BAUDS[m.dec_baud.min(crate::decode::BAUDS.len() - 1)],
        bits: m.dec_bits,
        parity: m.dec_parity,
        spi_mode: m.dec_mode,
        can_rate: crate::decode::CAN_RATES[m.dec_canrate.min(crate::decode::CAN_RATES.len() - 1)],
        canh: m.dec_canh,
        usb_speed: crate::decode::USB_SPEEDS[m.dec_uspeed.min(crate::decode::USB_SPEEDS.len() - 1)],
        cphy_rate: m.dec_cphy_rate,
        uart_inv: m.ser.uart_inv,
        spi_timeout: m.ser.spi_timeout as f64,
        spi_cs_high: m.ser.spi_cs_high,
    }
}

/// The baud rates of the Decode menu (`decode::BAUDS`), as the choices of a module's option row.
#[cfg_attr(not(feature = "decoders"), allow(dead_code))] // for the optional modules
pub const BAUD_NAMES: [&str; 10] =
    ["1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "460800", "921600"];
/// The default of a baud row: 115200, as the UART decoder.
#[cfg_attr(not(feature = "decoders"), allow(dead_code))] // for the optional modules
pub const BAUD_DEFAULT: usize = 7;

/// The decoder settings of a setup from before a protocol became a module: what its option rows were kept in.
/// A module that was a built-in protocol maps them onto its options (`Protocol::legacy`).
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // the rows of protocols that are still built in (USB speed: no module reads it yet)
pub struct Legacy {
    pub baud: usize,
    pub bits: u32,
    pub parity: oos_dsp::decode::Parity,
    pub canrate: usize,
    pub canh: bool,
    pub fdrate: usize,
    pub arate: usize,
    pub tick: usize,
    pub nibbles: u32,
    pub uspeed: usize,
}

/// One option row of the Decode menu: a name and its choices; the setting is the index of the choice.
pub struct Opt {
    pub name: &'static str,
    pub choices: &'static [&'static str],
    pub default: usize,
}

/// Where the demo traffic sits and what the decoder starts with when the protocol is chosen.
pub struct Setup {
    /// channel per role (0 = CH1); `None` leaves an optional role unset
    pub src: [Option<usize>; 4],
    /// logic analyser line per role, for the LA input
    pub la: [Option<u8>; 4],
    /// decision threshold (V)
    pub thr: f32,
    /// a trigger that holds the demo traffic still, set when the protocol is chosen; `None` leaves the trigger alone
    pub trig: Option<DemoTrig>,
    /// switch the channels of all its roles on when the protocol is chosen (the demo traffic is on all of them)
    pub show: bool,
    /// shared rows set when the protocol is chosen: the row and its value (an index, or the number of bits, 0/1)
    pub rows: &'static [(Row, u32)],
    /// switch every channel on (the protocol decodes across all of them)
    pub show_all: bool,
    /// the decoder takes the analog channels, not the LA lines
    pub la_off: bool,
}

impl Default for Setup {
    /// Main line on CH3 and a second one on CH4, 3.3 V logic; the demo UART line D8 and clock D10 for the LA; the
    /// trigger as it is.
    fn default() -> Setup {
        Setup {
            src: [Some(2), Some(3), None, None],
            la: [Some(8), Some(10), None, None],
            thr: 1.65,
            trig: None,
            show: false,
            rows: &[],
            show_all: false,
            la_off: false,
        }
    }
}

/// The trigger of [`Setup::trig`]: an edge of `slope` on the channel of role `role` at `level` (V). With an `idle` time
/// (s) only the first edge after the line was quiet that long (Nth edge, n = 1), else every edge (Edge).
#[derive(Clone, Copy, Debug)]
pub struct DemoTrig {
    pub role: usize,
    pub slope: Slope,
    pub idle: f32,
    pub level: f32,
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
#[cfg_attr(not(feature = "decoders"), allow(dead_code))] // for the optional modules
pub fn memory<T: Default + Send + 'static>(m: &mut Memory) -> &mut T {
    if !m.as_ref().is_some_and(|b| b.is::<T>()) {
        *m = Some(Box::new(T::default()));
    }
    m.as_mut().and_then(|b| b.downcast_mut::<T>()).unwrap()
}

/// Colour hint of a view's status line.
#[cfg_attr(not(feature = "decoders"), allow(dead_code))] // for the optional modules
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

/// A protocol module. Only `info` is required; everything else has a default that
/// fits a single-line serial protocol.
pub trait Protocol: Sync {
    /// What oos-dsp says of the decoder: its stable key, its name, the lines it takes, whether the main one may be a
    /// differential pair and the rule and start value of the protocol trigger, next to the decoder in its family's
    /// file (`oos_dsp::protocols::MODULES`).
    fn info(&self) -> &'static ModuleInfo;

    /// The option rows of the Decode menu.
    fn options(&self) -> &'static [Opt] {
        &[]
    }

    /// Text of a decoded field on the trace and in the event table; `None` = the generic text of its kind.
    fn text(&self, _p: &Packet, _opt: &[usize; OPTS]) -> Option<String> {
        None
    }

    /// The event table (and its CSV, `:...EVENts?`): one row per command, as (time of its first packet, text, check
    /// passed), instead of one row per field. `None` = a row per field with `text`.
    fn events(&self, _p: &[Packet], _opt: &[usize; OPTS]) -> Option<Vec<(f64, String, bool)>> {
        None
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

    /// The shared rows of the Decode menu the protocol shows, before its own options ([`Row`]).
    fn shared(&self) -> &'static [Row] {
        &[]
    }

    /// The role whose line frames start on, where the hardware trigger in front of the protocol trigger looks
    /// (`start_edge`): the main line, or e.g. a chip select or word select.
    fn start_role(&self) -> usize {
        0
    }

    /// For a protocol that was built in before it became a module: its option settings from a setup that kept them in
    /// the shared decoder fields (`Legacy`); `None` = the defaults.
    fn legacy(&self, _old: &Legacy) -> Option<[usize; OPTS]> {
        None
    }
}

impl dyn Protocol {
    /// The stable key of the module ([`ModuleInfo::key`]).
    pub fn key(&self) -> &'static str {
        self.info().key
    }

    /// The name of the module ([`ModuleInfo::name`]).
    pub fn name(&self) -> &'static str {
        self.info().name
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
            assert!(m.info().roles[0].is_some(), "{}: no main role", m.name());
        }
        // the core decoders at their fixed places (`Proto::UART` ...)
        for (p, key) in [
            (crate::model::Proto::UART, "Uart"),
            (crate::model::Proto::I2C, "I2c"),
            (crate::model::Proto::SPI, "Spi"),
            (crate::model::Proto::CAN, "Can"),
            (crate::model::Proto::LIN, "Lin"),
            (crate::model::Proto::PARALLEL, "Parallel"),
            (crate::model::Proto::CPHY, "Cphy"),
        ] {
            assert_eq!(p.key(), key);
        }
    }

    /// Every module with demo traffic decodes it without errors and finds something.
    #[test]
    fn modules_decode_their_demo() {
        for (i, m) in MODULES.iter().enumerate() {
            let Some(d) = demo(i as u8) else { continue };
            let sh = shared(&Default::default());
            let dt = m.info().bit_time(&opt_fit(i as u8, opt_default(i as u8)), &sh) / 40.0;
            let n = (3.0 * d.period / dt) as usize;
            let waves: Vec<Option<Vec<f32>>> = d
                .lines
                .iter()
                .map(|l| l.as_ref().map(|f| (0..n).map(|k| f((k as f64 * dt).rem_euclid(d.period)) as f32).collect()))
                .collect();
            let s = m.setup();
            let l = Lines {
                lines: std::array::from_fn(|k| waves[k].as_deref()),
                dt,
                thr: s.thr,
                hy: 0.1,
                opt: opt_fit(i as u8, opt_default(i as u8)),
                sh,
                chans: [None; 4],
                src: [None; 4],
            };
            let p = m.info().decode(&l).unwrap_or_else(|e| panic!("{}: {}", m.name(), e));
            assert!(!p.is_empty(), "{}: nothing decoded from its demo", m.name());
            // I2C: the master NACKs the last byte it reads before the stop, which the decoder flags like any NACK.
            use oos_dsp::decode::Kind;
            let read_end = |k: usize| {
                m.key() == "I2c" && p[k].kind == Kind::Data && p.get(k + 1).is_none_or(|n| n.kind == Kind::Stop)
            };
            let bad = p.iter().enumerate().find(|&(k, x)| !x.ok && !read_end(k));
            assert!(bad.is_none(), "{}: errors in its own demo: {:?}", m.name(), bad);
        }
    }

    /// The modules without demo traffic, each with the reason: `modules_decode_their_demo` runs none of them, and
    /// the host build shows no signal for them. A module in this list that has demo traffic fails the test, so the
    /// list only gets shorter.
    const NO_DEMO: [(&str, &str); 10] = [
        ("Uart", "the demo source has UART traffic on CH3 by itself, whatever the decoder"),
        ("Parallel", "none written yet; the decoder is tested in oos-dsp (dsp/tests/decode/parallel.rs)"),
        ("Cphy", "none written yet; the decoder is tested in oos-dsp (dsp/src/cphy.rs)"),
        ("Manchester", "none written yet; the decoder is tested in oos-dsp (dsp/src/linecode.rs)"),
        ("Mlt3", "none written yet; the decoder is tested in oos-dsp (dsp/src/linecode.rs)"),
        ("ModbusAscii", "none written yet; the decoder is tested in oos-dsp (dsp/tests/decode/modbus.rs)"),
        ("Ds2482", "none written yet; the decoder is tested in oos-dsp (dsp/tests/decode/onewire.rs)"),
        ("Jtag", "none written yet; the decoder is tested in oos-dsp (dsp/tests/decode/debug.rs)"),
        ("Qspi", "none written yet; the decoder is tested in oos-dsp (dsp/tests/decode/qspi.rs)"),
        ("I3c", "none written yet; the decoder is tested in oos-dsp (dsp/tests/decode/i3c.rs)"),
    ];

    /// What a decoder needs besides its module, each failure with its name and the part that is missing: its key in
    /// ui/tests/golden/decoder-keys.txt (a key never changes: `OOS_BLESS=1 cargo test --locked -q protocols::tests`
    /// adds the line of a new one and no other), its `ModuleInfo` in `oos_dsp::protocols::MODULES` at the same place
    /// (and, in the whole build, a module for every decoder there), its name under Protocol decoders in
    /// wiki/Features.md, demo traffic or the reason it has none, with demo traffic its case in
    /// `extended_decoders_decode_their_demo`, and for its file the Cargo feature among the default ones. The number of
    /// decoders the wiki and README.md give is the number registered. What a `ModuleInfo` needs is checked in oos-dsp
    /// (dsp/src/protocols.rs).
    #[test]
    fn every_decoder_is_complete() {
        use crate::ui::testkit::{count_of, document, exempt_once, names, section, stored_names};
        let wiki = section(&document("wiki/Features.md"), "**Protocol decoders");
        // the example module is compiled in the tests only
        let mods: Vec<_> = MODULES.iter().filter(|m| m.key() != "Example").collect();
        for m in &mods {
            assert!(names(&wiki, m.name()), "{}: not named under Protocol decoders in wiki/Features.md", m.name());
            match NO_DEMO.iter().find(|x| x.0 == m.key()) {
                Some(x) => {
                    assert!(m.demo().is_none(), "{}: it has demo traffic, take it out of NO_DEMO", m.key());
                    assert!(!x.1.is_empty(), "{}: in NO_DEMO without a reason", m.key());
                }
                None => assert!(m.demo().is_some(), "{}: no demo traffic, and no reason in NO_DEMO", m.key()),
            }
        }
        // the optional modules with demo traffic are shown on the screen of the demo by a test
        let shown = include_str!("../ui/tests.rs");
        let shown = shown.split_once("fn extended_decoders_decode_their_demo()").expect("the test of the demos").1;
        let shown = shown.split_once("\n}\n").expect("its end").0;
        for m in mods.iter().skip(7).filter(|m| m.demo().is_some()) {
            let case = format!("m(\"{}\")", m.key());
            assert!(
                shown.contains(&case),
                "{}: no case {} in extended_decoders_decode_their_demo (ui/src/ui/tests.rs)",
                m.name(),
                case
            );
        }
        // a build with fewer decoders has fewer keys than the list, and fewer than the texts count
        let whole = cfg!(feature = "decoders");
        let keys: Vec<&str> = mods.iter().map(|m| m.key()).collect();
        // a module is a decoder of oos-dsp, in the order of its registry; the whole build has every one of them
        let dsp = &oos_dsp::protocols::MODULES;
        for m in &mods {
            let registered = dsp.iter().any(|&i| std::ptr::eq(i, m.info()));
            assert!(registered, "{}: its ModuleInfo is not in oos_dsp::protocols::MODULES", m.key());
        }
        let order: Vec<&str> = dsp.iter().map(|i| i.key).filter(|k| keys.contains(k)).collect();
        assert_eq!(keys, order, "MODULES: not in the order of oos_dsp::protocols::MODULES");
        for i in dsp.iter().filter(|_| whole) {
            assert!(keys.contains(&i.key), "{}: in oos_dsp::protocols::MODULES, no module in ui/src/protocols", i.key);
        }
        exempt_once("NO_DEMO", &NO_DEMO.map(|x| x.0), &keys, whole);
        stored_names("decoder-keys.txt", &mods.iter().map(|m| m.key().to_string()).collect::<Vec<_>>(), whole);
        if whole {
            assert_eq!(count_of(&wiki, "Protocol decoders"), Some(mods.len()), "the number in wiki/Features.md");
            assert_eq!(
                count_of(&document("README.md"), "protocol decoders"),
                Some(mods.len()),
                "the number in README.md"
            );
        }
        // every file here but the core ones is a feature of its own, on by default
        let (toml, this) = (include_str!("../../Cargo.toml"), include_str!("mod.rs"));
        let default = toml.lines().find(|l| l.starts_with("decoders = ")).expect("the decoders of ui/Cargo.toml");
        for file in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/protocols")).unwrap() {
            let file = file.unwrap().file_name().into_string().unwrap();
            let Some(name) = file.strip_suffix(".rs").filter(|n| !["mod", "core", "example"].contains(n)) else {
                continue;
            };
            assert!(
                toml.contains(&format!("\ndec-{} = []\n", name)),
                "{}: no feature dec-{} in ui/Cargo.toml",
                file,
                name
            );
            assert!(default.contains(&format!("\"dec-{}\"", name)), "{}: dec-{} is not in `decoders`", file, name);
            let line = format!("#[cfg(feature = \"dec-{}\")]\nmod {};\n", name, name);
            assert!(this.contains(&line), "{}: no `mod {};` behind its feature in protocols/mod.rs", file, name);
        }
    }
}
