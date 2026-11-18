// Copyright (C) 2026 dekajoule
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
//! Every decoder is a module. The seven core protocols (`core.rs`: UART, I2C, SPI, CAN, LIN, Parallel, C-PHY) are
//! always built and keep the first places, since the hardware trigger types and SCPI name them (`Proto::UART` ...).
//! Every other file is behind a Cargo feature (`dec-<file>`, all on by default), so a build can leave decoders out.
//! `docs/protocol-modules.md` walks through writing one, and `example.rs` (compiled in the tests) uses every hook.

use crate::model::{BusWhat, Slope};
use oos_dsp::decode::Packet;

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

/// Option rows a module may have (bit rate, word size, variant ...).
pub const OPTS: usize = 4;

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

/// The values of the shared rows.
#[derive(Clone, Copy, Debug)]
pub struct Shared {
    pub baud: f64,
    pub bits: u32,
    pub parity: oos_dsp::decode::Parity,
    pub spi_mode: u8,
    pub can_rate: f64,
    pub canh: bool,
    #[cfg_attr(not(feature = "decoders"), allow(dead_code))] // for the optional modules
    pub usb_speed: oos_dsp::decode::UsbSpeed,
    /// 0 = auto
    pub cphy_rate: f64,
    /// the UART line is inverted (the protocol trigger's polarity)
    pub uart_inv: bool,
    /// SPI: a clock pause (s) that starts a frame (0: 20 bit times), and CS active high (the protocol trigger's)
    pub spi_timeout: f64,
    pub spi_cs_high: bool,
}

impl Default for Shared {
    /// What a new model has.
    fn default() -> Shared {
        Shared::of(&crate::model::Model::default())
    }
}

impl Shared {
    pub fn of(m: &crate::model::Model) -> Shared {
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
#[allow(dead_code)] // the rows of protocols that are still built in
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
    /// the shared rows ([`Protocol::shared`])
    pub sh: Shared,
    /// every analog channel (None: off) and the channel of each role, for a protocol whose lines are not its roles
    /// (Parallel: the data lines from the main source upwards)
    pub chans: [Option<&'a [f32]>; 4],
    pub src: [Option<usize>; 4],
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

    /// The shared rows of the Decode menu the protocol shows, before its own options ([`Row`]).
    fn shared(&self) -> &'static [Row] {
        &[]
    }

    /// `bit_time` when it depends on a shared row (`Row::Baud` ...).
    fn bit_time_sh(&self, opt: &[usize; OPTS], _sh: &Shared) -> f64 {
        self.bit_time(opt)
    }

    /// `frame_span` when it depends on a shared row.
    fn frame_span_sh(&self, opt: &[usize; OPTS], _sh: &Shared) -> Option<f64> {
        self.frame_span(opt)
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
            let dt = m.bit_time(&opt_fit(i as u8, opt_default(i as u8))) / 40.0;
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
                sh: Shared::default(),
                chans: [None; 4],
                src: [None; 4],
            };
            let p = m.decode(&l).unwrap_or_else(|e| panic!("{}: {}", m.name(), e));
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
}
