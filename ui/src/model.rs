// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Scope state (what the user has set), value ladders and number formatting.

use crate::board::{board, Active, Board};
use crate::gfx::{rgb, Rgb};
use crate::math::{MathOp, Window};

/// Channel slots of the UI state: the largest model of the board (smaller models use the first `n_ch()`).
pub const NCH: usize = <Active as Board>::MAX_CHANNELS;
/// Reference waveform slots, R1..R10. A slot is waveform memory, not a channel index.
pub const NREF: usize = 10;
const _: () = assert!(NCH == oos_dsp::CHANNELS, "triggers and expressions in dsp take oos_dsp::CHANNELS channels");

/// Generator slots of the UI state: the largest model of the board (the unit's own: `gen_count`).
pub const NGEN: usize = <Active as Board>::MAX_GENERATORS;

/// Analog inputs of the unit (`oos_hal::model::platform()`), at most NCH.
pub fn n_ch() -> usize {
    oos_hal::model::platform().caps.channels.min(NCH)
}

/// Built-in generators of the unit (`oos_hal::model::platform()`), at most NGEN.
pub fn gen_count() -> usize {
    oos_hal::model::platform().caps.generators.min(NGEN)
}

/// The memory depths of the board (Acquire menu).
pub fn mdepths() -> &'static [(&'static str, u64)] {
    board().depths()
}

/// Channel names of the unit, "CH1".."CHn" (option lists of channel sources).
pub fn ch_names() -> Vec<String> {
    (0..n_ch()).map(|c| format!("CH{}", c + 1)).collect()
}

/// Optional channel source: `none` first, then the channels.
pub fn opt_ch_names(none: &str) -> Vec<String> {
    std::iter::once(none.to_string()).chain(ch_names()).collect()
}

/// Whether source index `s` exists on this unit: channels 0..n_ch(), and every index from NCH on (math channels).
pub fn src_ok(s: usize) -> bool {
    s < n_ch() || s >= NCH
}

/// A waveform a tool can name. A channel is the live front end. A ref is one stored record, measured at
/// that record's sample interval. Refs are not packed into the channel/math index.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum WaveId {
    Ch(usize),
    Ref(usize),
}

impl WaveId {
    pub fn ch(self) -> Option<usize> {
        match self {
            WaveId::Ch(c) => Some(c),
            WaveId::Ref(_) => None,
        }
    }

    /// Setup text: `1`..`4` are channels, `R1`..`R10` are refs. Old files only have channel numbers.
    pub fn token(self) -> String {
        match self {
            WaveId::Ch(c) => (c + 1).to_string(),
            WaveId::Ref(i) => format!("R{}", i + 1),
        }
    }

    /// Inverse of `token`. `0` is CH1, the same clamp the old channel parser used.
    pub fn parse(v: &str) -> Result<WaveId, String> {
        if let Some(rest) = v.strip_prefix('R').or_else(|| v.strip_prefix('r')) {
            let n = rest.parse::<usize>().map_err(|_| format!("not a source: {}", v))?;
            if (1..=NREF).contains(&n) {
                Ok(WaveId::Ref(n - 1))
            } else {
                Err(format!("R{} out of range", n))
            }
        } else {
            let i = v.parse::<usize>().map_err(|_| format!("not a source: {}", v))?;
            if i == 0 {
                Ok(WaveId::Ch(0))
            } else if i <= NCH {
                Ok(WaveId::Ch(i - 1))
            } else {
                Err(format!("{} out of range", i))
            }
        }
    }
}

/// `d` steps from source index `i` among `0..n`, cycling and skipping the channels the unit lacks (2-channel models:
/// CH3/CH4). The steps match the option lists (`ch_names`), so picking an option by its index distance works.
pub fn src_step(i: usize, n: usize, d: i32) -> usize {
    src_step_in(i, n, d, n_ch())
}

/// 1-2-5 steps from `lo` to `hi`. `zero` inserts 0 (an unset geometry or resistance). `d` moves along that list and stops at the ends.
pub fn eng_ladder(x: f64, d: i32, lo: f64, hi: f64, zero: bool) -> f64 {
    let mut l = Vec::new();
    if zero {
        l.push(0.0);
    }
    let mut e = lo;
    while e <= hi * 1.001 && l.len() < 48 {
        l.push(e);
        l.push(e * 2.0);
        l.push(e * 5.0);
        e *= 10.0;
    }
    if l.is_empty() {
        return x;
    }
    let i = l.iter().position(|&v| v + 1e-12 >= x).unwrap_or(l.len() - 1);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}

/// `src_step` for a unit with `nch` inputs.
fn src_step_in(i: usize, n: usize, d: i32, nch: usize) -> usize {
    let v: Vec<usize> = (0..n).filter(|&k| k < nch || k >= NCH).collect();
    if v.is_empty() {
        return i;
    }
    let p = v.iter().position(|&k| k == i).unwrap_or(0) as i32;
    v[(p + d).rem_euclid(v.len() as i32) as usize]
}

/// The same for an optional channel source encoded as 0 = none, k = channel k - 1.
pub fn opt_src_step(i: usize, d: i32) -> usize {
    opt_src_step_in(i, d, n_ch())
}

fn opt_src_step_in(i: usize, d: i32, nch: usize) -> usize {
    let v: Vec<usize> = (0..=nch).collect();
    let p = v.iter().position(|&k| k == i).unwrap_or(0) as i32;
    v[(p + d).rem_euclid(v.len() as i32) as usize]
}
pub const HDIV: f32 = 10.0;
pub const VDIV: f32 = 8.0;

/// Channel colours (yellow, sky blue, pink, teal: a colour-blind safe set).
const PALETTE: [Rgb; 8] = [
    rgb(0xf0, 0xe4, 0x42),
    rgb(0x56, 0xb4, 0xe9),
    rgb(0xe0, 0x8c, 0xc0),
    rgb(0x2e, 0xc4, 0xa0),
    rgb(0xf0, 0x90, 0x40),
    rgb(0xa0, 0x90, 0xf0),
    rgb(0xc0, 0xd0, 0x60),
    rgb(0xe0, 0x60, 0x60),
];
pub const CH_COLORS: [Rgb; NCH] = {
    let mut c = [PALETTE[0]; NCH];
    let mut i = 0;
    while i < NCH {
        c[i] = PALETTE[i];
        i += 1;
    }
    c
};

/// V/div at the probe tip for probe 1x: 500 µV .. 10 V (DHO800/900 range).
/// V/div at the probe tip for 1x. Below 1 mV/div the AFE stays at its highest analog gain and the FPGA read path
/// scales digitally (cal_vertical digital gain 2.15 / 5.38 / 10.76 at 500 / 200 / 100 uV, no forced 20 MHz).
pub const VSCALES: [f32; 16] = [100e-6, 200e-6, 500e-6, 1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0, 2.0, 5.0, 10.0];
/// Fastest time/div the ADC acquisition uses. Stored refs (SIM) may display faster than this.
pub const HW_MIN_TDIV: f32 = 1e-9;

/// s/div: 50 ps .. 1000 s. Below 1 ns/div is a display scale for stored records; the live ADC still runs at
/// `HW_MIN_TDIV` (`hw_time_per_div`).
pub fn tscales() -> Vec<f32> {
    let mut v = Vec::new();
    let mut d = 1e-11_f64;
    while d < 2000.0 {
        for m in [1.0, 2.0, 5.0] {
            let x = d * m;
            // from 50 ps/div (stock 5 ns, then 1 ns): below 5 ns/div the live screen holds 12..62 samples, drawn
            // with the windowed sinc of sparse views; the record keeps the margin of `board::dho::hw::screen_margin`
            if x >= 0.49e-10 && x <= 1000.1 {
                v.push(x as f32);
            }
        }
        d *= 10.0;
    }
    v
}

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slope {
    Rise,
    Fall,
    Either,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigMode {
    Auto,
    Normal,
    Single,
}

/// What the plot area shows: the scope traces, or one of the analysers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AView {
    Scope,
    Spectrum,
    Eye,
    Power,
    Bode,
    /// bandwidth test: step response of the front end (ui/bwtest.rs)
    Step,
    Magnetic,
    Inverter,
    Dpt,
}

/// Where the eye is drawn. The full page is the default. Side and Under are extra panes and leave that page available.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EyePlace {
    /// The eye fills the plot: the figures, the jitter column and the charts.
    Full,
    /// A pane to the right of the trace.
    Side,
    /// A pane under the trace.
    Under,
}

/// Power analysis page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerMode {
    Quality,
    Harmonics,
    Switching,
    Ripple,
    /// C = q / v over the turn-on record
    Capacitance,
    /// rail overshoot, undershoot, DC and turn-on maximum
    Rail,
}

/// Magnetic plot: current against flux linkage, or B against H when the core is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MagPlot {
    IPsi,
    BH,
}

/// Inverter page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvPage {
    Dq0,
    Mech,
}

/// Where the inverter takes its angle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvAngle {
    /// θ from the phase trio (DQ0 only)
    Off,
    Phase,
    Analog,
    Quad,
    Hall,
}

/// The optional fourth probe of a double pulse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DptFourth {
    Off,
    Gate,
    DiodeV,
    DiodeI,
}

/// Which edge the double-pulse plot is zoomed to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DptZoom {
    Both,
    TurnOn,
    TurnOff,
}

/// Waveform histogram: off, vertical (voltage distribution of the whole screen), horizontal (time distribution in a
/// band of +-0.5 div around the trigger level).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hist {
    Off,
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    Run,
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigStatus {
    Triggered,
    Auto,
    Wait,
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcqMode {
    Normal,
    Peak,
    Average,
    HiRes,
}

pub use oos_dsp::measure::{Meas, ALL as ALL_MEAS};
pub use oos_dsp::sim::VideoStd;
pub use oos_dsp::trigger::{Bit, Pol};

/// Serial protocol of the decoder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Uart,
    I2c,
    Spi,
    Can,
    Lin,
    I2s,
    Mil1553,
    Parallel,
    CanFd,
    FlexRay,
    Dmx512,
    ModbusRtu,
    ModbusAscii,
    OneWire,
    Ds2482,
    Uart9,
    Swd,
    Jtag,
    Qspi,
    I3c,
    Spdif,
    UsbPd,
    Arinc429,
    Sent,
    /// USB 1.x / 2.0 low / full speed
    Usb,
    Manchester,
    Mlt3,
}

pub const PROTOS: [Proto; 27] = [
    Proto::Uart, Proto::I2c, Proto::Spi, Proto::Can, Proto::Lin, Proto::I2s, Proto::Mil1553, Proto::Parallel, Proto::CanFd,
    Proto::FlexRay, Proto::Dmx512, Proto::ModbusRtu, Proto::ModbusAscii, Proto::OneWire, Proto::Ds2482, Proto::Uart9, Proto::Swd,
    Proto::Jtag, Proto::Qspi, Proto::I3c, Proto::Spdif, Proto::UsbPd, Proto::Arinc429, Proto::Sent, Proto::Usb,
    Proto::Manchester, Proto::Mlt3,
];

impl Proto {
    pub fn name(self) -> &'static str {
        match self {
            Proto::Uart => "UART",
            Proto::I2c => "I2C",
            Proto::Spi => "SPI",
            Proto::Can => "CAN",
            Proto::Lin => "LIN",
            Proto::I2s => "I2S",
            Proto::Mil1553 => "1553",
            Proto::Parallel => "Parallel",
            Proto::CanFd => "CAN-FD",
            Proto::FlexRay => "FlexRay",
            Proto::Dmx512 => "DMX512",
            Proto::ModbusRtu => "Modbus RTU",
            Proto::ModbusAscii => "Modbus ASCII",
            Proto::OneWire => "1-Wire",
            Proto::Ds2482 => "DS2482",
            Proto::Uart9 => "UART 9-bit",
            Proto::Swd => "SWD",
            Proto::Jtag => "JTAG",
            Proto::Qspi => "Quad-SPI",
            Proto::I3c => "I3C",
            Proto::Spdif => "S/PDIF",
            Proto::UsbPd => "USB PD",
            Proto::Arinc429 => "ARINC 429",
            Proto::Sent => "SENT",
            Proto::Usb => "USB",
            Proto::Manchester => "Manchester",
            Proto::Mlt3 => "MLT-3",
        }
    }
    /// Role names of the sources (main, second, third optional, fourth optional); None = not used.
    pub fn roles(self) -> [Option<&'static str>; 4] {
        match self {
            Proto::I2c | Proto::I3c | Proto::Ds2482 => [Some("SDA"), Some("SCL"), None, None],
            Proto::Spi => [Some("MOSI"), Some("Clock"), Some("CS"), None],
            Proto::I2s => [Some("SD"), Some("SCK"), Some("WS"), None],
            Proto::Parallel => [Some("D0"), None, Some("Clock"), None],
            Proto::Swd => [Some("SWDIO"), Some("SWCLK"), None, None],
            Proto::Jtag => [Some("TMS"), Some("TCK"), Some("TDI"), Some("TDO")],
            Proto::Qspi => [Some("IO0"), Some("SCK"), Some("CS#"), Some("IO1")],
            Proto::UsbPd => [Some("CC"), None, None, None],
            // D- optional: D+ alone decodes without SE0
            Proto::Usb => [Some("D+"), None, Some("D-"), None],
            _ =>[Some("Source"), None, None, None],
        }
    }
    /// Serial protocols on one line that can also be decoded from a differential pair (RS-485: A - B).
    pub fn diff_ok(self) -> bool {
        matches!(self, Proto::Uart | Proto::Uart9 | Proto::ModbusRtu | Proto::ModbusAscii | Proto::Dmx512 | Proto::Can | Proto::CanFd | Proto::FlexRay | Proto::Lin | Proto::Arinc429)
    }
}

/// Trigger types (the software trigger engine of oos-dsp; on the unit later the TPU, docs/protocols/trigger.md).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrigType {
    Edge,
    Pulse,
    Slope,
    Video,
    Pattern,
    Duration,
    Timeout,
    Runt,
    Window,
    Delay,
    SetupHold,
    NthEdge,
    /// protocol trigger: the decoder of the Decode menu finds the event (BusWhat, value)
    Bus,
}

pub const TRIG_TYPES: [TrigType; 13] = [
    TrigType::Edge, TrigType::Pulse, TrigType::Slope, TrigType::Video, TrigType::Pattern, TrigType::Duration, TrigType::Timeout,
    TrigType::Runt, TrigType::Window, TrigType::Delay, TrigType::SetupHold, TrigType::NthEdge, TrigType::Bus,
];

/// What the protocol trigger looks for in the decoded packets (`trig::find_all`); the trigger point is the end of
/// the matching field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusWhat {
    /// every frame start (SOF, break, I2C start, 1553 command word, UART / SPI word)
    Frame,
    /// identifier / address / 1553 command word equal to the value
    Id,
    /// data byte / word / audio sample equal to the value
    Data,
    /// any packet with a failed check (CRC, parity, checksum, NACK, missing ACK)
    Error,
}
pub const BUS_WHATS: [BusWhat; 4] = [BusWhat::Frame, BusWhat::Id, BusWhat::Data, BusWhat::Error];
impl BusWhat {
    pub fn name(self) -> &'static str {
        match self {
            BusWhat::Frame => "Frame",
            BusWhat::Id => "ID/Addr",
            BusWhat::Data => "Data",
            BusWhat::Error => "Error",
        }
    }
}

impl TrigType {
    pub fn name(self) -> &'static str {
        match self {
            TrigType::Edge => "Edge",
            TrigType::Pulse => "Pulse",
            TrigType::Slope => "Slope",
            TrigType::Video => "Video",
            TrigType::Pattern => "Pattern",
            TrigType::Duration => "Duration",
            TrigType::Timeout => "Timeout",
            TrigType::Runt => "Runt",
            TrigType::Window => "Window",
            TrigType::Delay => "Delay",
            TrigType::SetupHold => "Setup/Hold",
            TrigType::NthEdge => "Nth edge",
            TrigType::Bus => "Bus",
        }
    }
}

/// Time condition kind of the "When" setting (Off only for runt).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhenKind {
    Off,
    Gt,
    Lt,
    In,
    Out,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinMode {
    Enter,
    Exit,
    Inside,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VSync {
    AllLines,
    Line,
    Odd,
    Even,
    AllFields,
}

/// 1-2-5 ladder for trigger times: 1 ns .. 10 s.
pub fn time_ladder() -> Vec<f32> {
    let mut v = Vec::new();
    let mut d = 1e-9_f64;
    while d < 20.0 {
        for m in [1.0, 2.0, 5.0] {
            if d * m <= 10.0 {
                v.push((d * m) as f32);
            }
        }
        d *= 10.0;
    }
    v
}

/// What the QUICK key (and the Quick button of the top bar) does; set in Utility > Quick key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quick {
    /// PPM of the screen in /tmp (the behaviour before the setting existed)
    Screenshot,
    /// PNG in the storage directory (Storage > Save screenshot)
    SavePng,
    SaveSetup,
    SaveCsv,
    Autoset,
    Single,
    RunStop,
    Clear,
}

pub const QUICKS: [Quick; 8] = [Quick::Screenshot, Quick::SavePng, Quick::SaveSetup, Quick::SaveCsv, Quick::Autoset, Quick::Single, Quick::RunStop, Quick::Clear];

impl Quick {
    /// Name in the Utility menu and its picker.
    pub fn name(self) -> &'static str {
        match self {
            Quick::Screenshot => "Screenshot",
            Quick::SavePng => "Save PNG",
            Quick::SaveSetup => "Save setup",
            Quick::SaveCsv => "Save CSV",
            Quick::Autoset => "Autoset",
            Quick::Single => "Single",
            Quick::RunStop => "Run/Stop",
            Quick::Clear => "Clear",
        }
    }

    /// Label of the Quick button in the top bar.
    pub fn short(self) -> &'static str {
        match self {
            Quick::Screenshot => "Snap",
            Quick::SavePng => "PNG",
            Quick::SaveSetup => "Setup",
            Quick::SaveCsv => "CSV",
            Quick::Autoset => "Auto",
            Quick::Single => "Single",
            Quick::RunStop => "Run/Stop",
            Quick::Clear => "Clear",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorMode {
    Off,
    Time,
    Volt,
    /// time cursors that read the active channel where they stand
    Track,
    /// time and voltage cursors at once
    Xy,
}

/// How the plot is split into waveform windows; every channel and math channel stands in one of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WinLayout {
    #[default]
    One,
    /// two side by side
    Cols,
    /// two one above the other
    Rows,
    /// two by two
    Quad,
}

impl WinLayout {
    pub const ALL: [WinLayout; 4] = [WinLayout::One, WinLayout::Cols, WinLayout::Rows, WinLayout::Quad];

    pub fn count(self) -> usize {
        match self {
            WinLayout::One => 1,
            WinLayout::Cols | WinLayout::Rows => 2,
            WinLayout::Quad => 4,
        }
    }
}

/// How much of the 1024×600 picture the plot takes. The same four apply to the panel and to a scaled window of it,
/// and to the waveform on the external display.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Density {
    /// top bar, plot, measurement strip, cards
    #[default]
    Scope,
    /// plot and measurement strip
    Full,
    /// plot, and under it a pane of numbers or two stacked traces
    Split,
    /// four equal panes
    Four,
}

impl Density {
    pub const ALL: [Density; 4] = [Density::Scope, Density::Full, Density::Split, Density::Four];

    pub fn label(self) -> &'static str {
        match self {
            Density::Scope => "Scope",
            Density::Full => "Full",
            Density::Split => "Split",
            Density::Four => "Four",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grid {
    Full,
    Light,
    None,
}

/// Where the measurement strip is drawn. Under is below the graticule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StripDock {
    Under,
    Left,
    Right,
    Above,
}

impl StripDock {
    pub const ALL: [StripDock; 4] = [StripDock::Under, StripDock::Left, StripDock::Right, StripDock::Above];

    pub fn label(self) -> &'static str {
        match self {
            StripDock::Under => "Under",
            StripDock::Left => "Left",
            StripDock::Right => "Right",
            StripDock::Above => "Above",
        }
    }
}

/// Settings of one decoder (the `dec_*` fields of `Model` are the one the menu edits).
#[derive(Clone, PartialEq, Debug)]
pub struct DecCfg {
    pub on: bool,
    pub src: usize,
    pub baud: usize,
    pub canrate: usize,
    pub canh: bool,
    pub fdrate: usize,
    pub arate: usize,
    pub tick: usize,
    pub nibbles: u32,
    pub uspeed: usize,
    pub src4: Option<usize>,
    pub la_in: bool,
    pub la: [Option<u8>; 4],
    pub diff: Option<usize>,
    pub thr: f32,
    pub ascii: bool,
    pub proto: Proto,
    pub src2: usize,
    pub cs: Option<usize>,
    pub mode: u8,
    pub bits: u32,
    pub parity: oos_dsp::decode::Parity,
}

/// Decoders at once.
pub const DECODERS: usize = 4;

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
    pub offset: f32,   // volts added before display (vertical position)
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
        self.probe * if bnc < 0.06 { 2.0 } else if bnc < 0.26 { 12.0 } else if bnc < 2.65 { 56.0 } else { 550.0 }
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
        Self { on: false, label: "I2C".into(), pos: -3.0, kind: BusKind::I2c, clk: WaveId::Ch(0), data: WaveId::Ch(1), clk_thr: 1.65, data_thr: 1.65, addr_rw: false }
    }
}

impl TraceBus {
    /// The name drawn on the plot. An empty label falls back to the type.
    pub fn shown_label(&self) -> String {
        let t = self.label.trim();
        if t.is_empty() { self.kind.name().to_string() } else { t.to_string() }
    }
}

/// Keeps a bus label to the length of a channel name, without control characters.
pub fn bus_label(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(10).collect::<String>().trim().to_string()
}

#[derive(Clone, PartialEq, Debug)]
pub struct Model {
    pub ch: [Channel; NCH],
    pub active: usize, // channel the vertical knobs act on
    /// Track cursor B's channel. None follows the active channel.
    pub cur_b_ch: Option<usize>,
    pub tscale: usize,
    pub delay: f32, // s, horizontal position of the trigger point relative to the centre
    pub trig_src: usize,
    /// edge and pulse trigger on a logic analyser line instead of `trig_src` (`trig_line`)
    pub trig_la: Option<u8>,
    /// pattern trigger: D0..D15 (H / L / X), with the LA on
    pub pat_la: [Bit; 16],
    pub trig_level: f32,
    pub slope: Slope,
    pub trig_mode: TrigMode,
    /// trigger holdoff, seconds (0 = the shortest there is)
    pub holdoff: f32,
    pub trig_type: TrigType,
    /// second level: low threshold (slope, runt, window), level of B (delay), clock level (setup/hold)
    pub trig_level2: f32,
    /// noise reject: wider comparator hysteresis (0.7 instead of 0.3 div)
    pub trig_nrej: bool,
    /// second source: B (delay), clock (setup/hold)
    pub trig_src2: usize,
    pub trig_slope2: Slope,
    pub trig_pol: Pol,
    pub trig_when: WhenKind,
    pub trig_t1: f32,
    pub trig_t2: f32,
    pub video_std: VideoStd,
    pub video_sync: VSync,
    pub video_line: u32,
    pub pat: [Bit; NCH],
    pub pat_levels: [f32; NCH],
    pub win_mode: WinMode,
    pub nth_idle: f32,
    pub nth_n: u32,
    pub setup: f32,
    pub hold: f32,
    pub run: RunState,
    pub status: TrigStatus,
    pub acq: AcqMode,
    pub avg_count: u32,
    /// segmented recording: records per RUN/SINGLE (0 = off), browsed with the history afterwards
    pub segments: usize,
    pub mdepth: usize, // index into `mdepths()` (0 = auto)
    pub persist: usize, // index into PERSIST
    pub intensity: u32, // 10..100 %
    pub grid: Grid,
    /// graticule brightness in % (10..100), and whether the frame with its ticks is drawn
    pub grid_bright: u32,
    pub grid_frame: bool,
    /// waveform windows, and the window (0-based) of each channel and math channel
    pub win_layout: WinLayout,
    /// Scope, Full, Split, Four. This is what the Windows row sets; `win_layout` is the split it asked for.
    pub density: Density,
    pub ch_win: [usize; NCH],
    pub math_win: [usize; crate::math::NMATH],
    /// window of the LA lanes while the plot is split (one window: their strip under the plot)
    pub la_win: usize,
    pub meas: Vec<(Meas, WaveId)>,
    /// delay and phase: entry of `ui::tools::PAIRS`, source A, source B (a channel or a loaded ref)
    pub meas2: Vec<(usize, WaveId, WaveId)>,
    /// Catalog results that are not a `Meas` row: id, source A, source B, third source, fourth source.
    pub cat: Vec<(u16, WaveId, WaveId, WaveId, WaveId)>,
    pub dvm_on: bool,
    pub dvm_src: usize,
    pub dvm_mode: usize,
    pub cnt_on: bool,
    pub cnt_src: usize,
    pub cnt_mode: usize,
    pub cnt_digits: u32,
    /// user names of the channels ("" = CHn)
    pub ch_label: [String; NCH],
    pub cursor: CursorMode,
    pub cur_a: f32, // fraction of the screen 0..1 (time: x, volt: y)
    pub cur_b: f32,
    /// voltage cursors of the XY mode
    pub cur_c: f32,
    pub cur_d: f32,
    pub touch_lock: bool,
    #[allow(dead_code)] // fine V/div steps: not implemented yet
    pub fine_v: bool,
    /// fine time base on the knob
    pub fine_t: bool,
    /// fine time base: factor on the step of `tscale`, 1.0 = on the step; the acquisition keeps the step
    pub tfine: f32,
    /// the scale changes around ground, not the middle of the screen
    pub v_expand_gnd: bool,
    pub h_expand: HExpand,
    pub math: MathOp,
    pub math_a: usize,
    pub math_b: usize,
    pub math_scale: usize, // index into math::math_scales() (waveform) or FFT_DB (FFT)
    /// corner frequency of the math low-/high-pass (Hz)
    pub math_fc: f32,
    /// Linear: A x + B
    pub math_lin: (f32, f32),
    /// the formula of each math channel
    pub math_expr: [String; crate::math::NMATH],
    /// data logger interval, index into `LOG_STEPS`
    pub log_ivl: usize,
    /// spectrum limit from a trace: dB above it
    pub spec_margin: f32,
    pub math_offset: f32,  // divisions
    pub fft_win: Window,
    /// bandwidth of the math band-pass/band-stop (Hz)
    pub math_bw: f32,
    /// the math channel the fields above edit (0..NMATH); the others are kept in `maths`
    pub math_sel: usize,
    pub maths: [crate::math::MathCh; crate::math::NMATH],
    /// the other decoders; None = not touched yet; the slot of `dec_sel` is not used
    pub decs: [Option<DecCfg>; DECODERS],
    pub dec_sel: usize,
    /// second pane under the plot, index into `ui::tables::PANES` (0 = traces, no pane drawn)
    pub pane: usize,
    /// pane Split returns to after Scope, Full or Four (0 = two stacked traces)
    pub split_pane: usize,
    /// what the measurements measure, drawn on the traces (measlabels.rs)
    pub meas_labels: bool,
    /// measurement strip: under the plot, left, right, or above the trace
    pub strip_dock: StripDock,
    /// what the two charts of the Trend pane show: 0 = by the list, 1 = off, k = row k - 2 of the measurement list
    pub trend_sel: [usize; 2],
    pub dec_on: bool,
    pub dec_src: usize,
    pub dec_baud: usize, // index into decode::BAUDS
    /// CAN bit rate, index into decode::CAN_RATES
    pub dec_canrate: usize,
    /// CAN probed on CAN_H (dominant high) instead of the RX/TX logic level
    pub dec_canh: bool,
    /// CAN-FD data phase bit rate (index into decode::FD_RATES)
    pub dec_fdrate: usize,
    /// ARINC 429 bit rate (index into decode::ARINC_RATES, 0 = auto)
    pub dec_arate: usize,
    /// SENT clock tick (index into decode::SENT_TICKS) and data nibbles per frame (1..6)
    pub dec_tick: usize,
    pub dec_nibbles: u32,
    /// USB speed (index into decode::USB_SPEEDS, 0 = auto)
    pub dec_uspeed: usize,
    /// fourth decoder source (JTAG TDO, Quad-SPI IO1)
    pub dec_src4: Option<usize>,
    /// decoder input: logic analyser lines instead of the channels (`dec_on_la`)
    pub dec_la_in: bool,
    /// LA line per decoder role (source, second, third, fourth; `Proto::roles`), None = not connected
    pub dec_la: [Option<u8>; 4],
    /// RS-485: decode the difference dec_src - dec_diff
    pub dec_diff: Option<usize>,
    /// protocol trigger condition and value
    pub trig_bus: BusWhat,
    pub trig_bus_val: u32,
    pub dec_thr: f32,
    pub dec_ascii: bool,
    pub dec_proto: Proto,
    /// second source: SCL (I2C), CLK (SPI)
    pub dec_src2: usize,
    /// SPI chip select (active low), None = frames from clock pauses
    pub dec_cs: Option<usize>,
    pub dec_mode: u8,
    /// data bits: UART 5..9, SPI 4..32
    pub dec_bits: u32,
    pub dec_parity: oos_dsp::decode::Parity,
    /// one bus on the plot
    pub bus: TraceBus,
    pub zoom: bool,
    /// slow time bases on the hardware: roll from 50 ms/div (on), scan from 200 ms/div (off), like stock "Auto Roll"
    pub auto_roll: bool,
    pub zscale: usize, // index into tscales(), <= tscale
    pub zdelay: f32,   // centre of the zoom window relative to the trigger
    /// colour-graded persistence: trace pixels coloured by hit density (temperature map)
    pub color_grade: bool,
    /// draw the samples as dots instead of connected lines (pseudo dot mode)
    pub dots: bool,
    /// External display. False copies the panel. True puts the plot on the display and leaves the panel for controls.
    /// The density changes that plot; the panel keeps its bars.
    pub hdmi_wave: bool,
    /// Z input: this channel sets the brightness of the other traces (low edge of the screen = dark)
    pub z_src: Option<usize>,
    /// histogram of the active channel from the phosphor map
    pub hist: Hist,
    pub view: AView,
    /// spectrum analyser: source, centre/span (Hz, span 0 = 0 .. fs/2), RBW (0 = auto), window, trace mode
    /// (0 normal, 1 average, 2 max hold, 3 min hold), reference level (dBV, top of the grid), waterfall, distortion
    pub spec_src: WaveId,
    pub spec_center: f64,
    pub spec_span: f64,
    pub spec_rbw: f64,
    pub spec_win: Window,
    /// peak table order: 0 = by frequency, 1 = by level
    pub spec_sort: usize,
    /// span of the pane, index into `ui::PANE_HARMS`
    pub spec_harm: usize,
    pub spec_trace: usize,
    pub spec_ref: f32,
    pub spec_wf: bool,
    pub spec_dist: bool,
    /// eye diagram: source, bit rate (0 = recovered from the edges)
    pub eye_src: WaveId,
    pub eye_rate: f64,
    /// the bit rate was measured from the record, not entered on the control
    pub eye_from_record: bool,
    /// beside the trace, or under it
    pub eye_place: EyePlace,
    /// BER for TJ, eye width and eye height. J2 and J9 ignore it.
    pub eye_ber: f64,
    /// phase-noise integration offsets, Hz
    pub eye_pn_lo: f64,
    pub eye_pn_hi: f64,
    /// NPJ stays off until this is set
    pub eye_npj: bool,
    /// optional clock / other rail: its edges fold the eye (DQS, recovered clock). Common-mode figures also use it.
    pub eye_b: Option<WaveId>,
    pub eye_rail: Option<WaveId>,
    /// Hz removed from the TIE as a sine; 0 = none
    pub eye_notch: f64,
    /// 30 kHz high-pass on the common mode
    pub eye_hp: bool,
    /// power analysis: voltage and current source, page
    pub pw_v: WaveId,
    pub pw_i: WaveId,
    pub pw_mode: PowerMode,
    /// line and switching frequency for the ripple tones; 0 = measured
    pub pw_fline: f64,
    pub pw_fsw: f64,
    /// input voltage for turn on/off, clock and reference for the rail (NaN = histogram top)
    pub pw_vin: Option<WaveId>,
    pub pw_clk: Option<WaveId>,
    /// None = the histogram top of the rail
    pub pw_ref: Option<f64>,
    /// Bode plot: sweep start/stop (Hz), points, generator amplitude (Vpp), input/output channel, generator preset
    /// and LAN address
    pub bode_start: f64,
    pub bode_stop: f64,
    pub bode_points: usize,
    pub bode_amp: f64,
    pub bode_in: WaveId,
    pub bode_out: WaveId,
    pub bode_gen: usize,
    pub bode_addr: String,
    /// loop gain, PSRR, or the linear Vout/Vin ratio
    pub bode_mode: oos_dsp::bode::Mode,
    /// burst idle (0 = one period), which side Time Outside counts, and Duration N
    pub meas_idle: f64,
    pub meas_out: oos_dsp::measure::Outside,
    pub meas_n: u32,
    /// false = clock edges, true = data edges
    pub meas_data: bool,
    /// Rise/fall low and high as fractions of amplitude (0.1/0.9 or 0.2/0.8).
    pub meas_lpct: f64,
    pub meas_hpct: f64,
    /// SetupAndHold limits (s). Zero means any negative setup or hold.
    pub meas_tsetup: f64,
    pub meas_thold: f64,
    /// ACPR channel width and spacing, Hz
    pub spec_acpr_w: f64,
    pub spec_acpr_s: f64,
    /// magnetic analysis: voltage, current (a channel or a loaded ref), turns, area m² (0 = unset), path m (0 = unset), copper Ω, plot, cursor
    pub mag_v: WaveId,
    pub mag_i: WaveId,
    pub mag_n: f64,
    pub mag_ae: f64,
    pub mag_le: f64,
    pub mag_rcu: f64,
    pub mag_plot: MagPlot,
    pub mag_cur: usize,
    /// inverter: page, phases, angle source, pole pairs, analog angle (channel or ref), voltage trio (else current)
    pub inv_page: InvPage,
    pub inv_a: WaveId,
    pub inv_b: WaveId,
    pub inv_c: WaveId,
    pub inv_ang: InvAngle,
    pub inv_poles: u32,
    pub inv_an: WaveId,
    pub inv_volt: bool,
    pub inv_tq: Option<WaveId>,
    pub inv_ev: Option<WaveId>,
    pub inv_ei: Option<WaveId>,
    pub inv_scale: f64,
    pub inv_degv: f64,
    pub inv_off: f64,
    /// analog angle is electrical (pole pairs convert it); false means the volts are already the shaft
    pub inv_elec: bool,
    pub inv_qa: u8,
    pub inv_qb: u8,
    pub inv_qi: Option<u8>,
    pub inv_cpr: f64,
    pub inv_swap: bool,
    pub inv_ha: u8,
    pub inv_hb: u8,
    pub inv_hc: u8,
    /// double pulse: Vds, Id, Vgs, optional fourth probe and its source, gate percent, zoom
    pub dpt_v: WaveId,
    pub dpt_i: WaveId,
    pub dpt_g: WaveId,
    pub dpt_4th: DptFourth,
    pub dpt_4ch: WaveId,
    pub dpt_pct: f64,
    pub dpt_zoom: DptZoom,
    /// hardware source: bumped with settings a measurement must wait for (`board::Shared::stamp`); not stored
    pub hw_stamp: u64,
    /// built-in generator: output on, frequency (Hz), amplitude (Vpp), offset (V); sine
    /// bandwidth test (Analysers > Bandwidth): source, signal frequency hint (0 = Auto), bins per period
    pub bw_src: usize,
    pub bw_hint: f64,
    pub bw_bins: usize,
    /// which generator the `afg_*` fields below are, the settings of the others
    pub gen_sel: usize,
    pub gen_bank: [GenCh; NGEN],
    pub afg_on: bool,
    /// arbitrary wave: id of `Board::arb_store` (0 = none yet), channel it is captured from
    pub afg_arb: u32,
    pub afg_arb_src: usize,
    pub afg_freq: f64,
    pub afg_ampl: f64,
    pub afg_offset: f64,
    /// index into AFG_WAVES; duty (square, pulse) and symmetry (ramp) in %
    pub afg_wave: usize,
    pub afg_duty: f64,
    pub afg_symmetry: f64,
    /// modulation: index into AFG_MODS, rate in Hz, AM depth in %, FM deviation in Hz, PM deviation in degrees,
    /// shape index into AFG_MOD_SHAPES
    pub afg_mod: usize,
    pub afg_mod_rate: f64,
    pub afg_mod_depth: f64,
    pub afg_fm_dev: f64,
    pub afg_pm_dev: f64,
    pub afg_mod_shape: usize,
    /// mask test: on/off, source channel, tolerances (divisions) used when the mask is created, stop on fail
    pub mask_on: bool,
    /// zone trigger (zone.rs): two zones in waveform coordinates
    pub zones: [crate::zone::Zone; 2],
    pub mask_src: usize,
    pub mask_x: f32,
    pub mask_y: f32,
    pub mask_stop: bool,
    pub mask_inv: bool,
    /// action of the QUICK key
    pub quick: Quick,
    /// logic analyser D0..D15 (DHO900, ui/la.rs)
    pub la: crate::ui::la::LaSet,
    /// SIM menu: which serial preset and which R slot Play writes, plus the knobs that override the preset.
    pub sim_preset: usize,
    pub sim_slot: usize,
    pub sim_rate: f64,
    pub sim_prbs: u8,
    pub sim_vhigh: f64,
    pub sim_vlow: f64,
    pub sim_rise: f64,
    pub sim_noise: f64,
    pub sim_bw: f64,
    pub sim_deemph: f64,
    pub sim_ssc: bool,
    pub sim_ssc_center: bool,
    /// Spread deviation as a fraction; 0.005 is 0.5%.
    pub sim_ssc_deviation: f64,
    pub sim_ssc_hz: f64,
    pub sim_rj: f64,
    pub sim_pj: f64,
    pub sim_pjhz: f64,
    /// Duty-cycle distortion, seconds peak-to-peak.
    pub sim_dcd_pp: f64,
    pub sim_corr_rj: f64,
    pub sim_corr_hz: f64,
}

pub const MATH_COLOR: Rgb = rgb(0xa9, 0x8b, 0xff);
/// Trace colours of M1..M8 (M1 = the classic math colour).
pub const MATH_COLORS: [Rgb; 8] = [
    MATH_COLOR, rgb(0xff, 0x8b, 0xd0), rgb(0x8b, 0xe9, 0xff), rgb(0xff, 0xc8, 0x6b), rgb(0x9b, 0xff, 0x8b), rgb(0xff, 0x9b, 0x8b), rgb(0xd8, 0xd8, 0xd8), rgb(0x8b, 0xa8, 0xff),
];
/// FFT vertical scale in dB/div.
pub const FFT_DB: [f32; 5] = [1.0, 2.0, 5.0, 10.0, 20.0];

/// Bandwidth limits of a channel (AFE low-pass): Auto = filter ~ fs / 4 as the
/// default, Full = widest filter (stock "OFF", 3150 MHz in the AFE), GND = no signal through the AFE; then the AFE
/// filter frequency in MHz (`board::dho::hw::bw_code`).
pub const BWS: [(&str, u32); 31] = [
    ("Auto", 0), ("Full", 0), ("20 MHz", 20), ("GND", 0),
    ("5 MHz", 5), ("10 MHz", 10), ("15 MHz", 15), ("25 MHz", 25), ("30 MHz", 30), ("35 MHz", 35), ("40 MHz", 40),
    ("50 MHz", 50), ("60 MHz", 60), ("70 MHz", 70), ("75 MHz", 75), ("80 MHz", 80), ("90 MHz", 90), ("100 MHz", 100),
    ("125 MHz", 125), ("150 MHz", 150), ("175 MHz", 175), ("200 MHz", 200), ("250 MHz", 250), ("300 MHz", 300),
    ("310 MHz", 310), ("400 MHz", 400), ("600 MHz", 600), ("800 MHz", 800), ("1 GHz", 1000), ("1.5 GHz", 1500),
    ("2 GHz", 2000),
];
/// Index of "20 MHz" in BWS (old setups, SCPI 20M).
pub const BW_20M: usize = 2;
/// Index of "Full" in BWS.
pub const BW_FULL: usize = 1;
/// Index of "Auto" in BWS: the filter follows the sample rate (the default, as on the stock system; with Full the
/// noise at slow time bases can exceed the trigger hysteresis, trigger.md 7).
pub const BW_AUTO: usize = 0;

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

/// Persistence: name, seconds (0 = minimum, <0 = infinite).
pub const PERSIST: [(&str, f32); 7] = [("Min", 0.0), ("100 ms", 0.1), ("500 ms", 0.5), ("1 s", 1.0), ("5 s", 5.0), ("10 s", 10.0), ("Infinite", -1.0)];

impl Default for Model {
    fn default() -> Model {
        let c = |on| Channel { on, vscale: 12, vfine: 1.0, unit: AmpUnit::V, bias: 0.0, skew: 0.0, offset: 0.0, coupling: Coupling::Dc, bw: BW_AUTO, probe: 1.0, invert: false };
        let ts = tscales();
        let t200u = ts.iter().position(|&t| (t - 200e-6).abs() < 1e-9).unwrap_or(ts.len() / 2);
        let mut m = Model {
            ch: std::array::from_fn(|i| c(i == 0)),
            active: 0,
            cur_b_ch: None,
            tscale: t200u,
            delay: 0.0,
            trig_src: 0,
            trig_la: None,
            pat_la: [Bit::X; 16],
            trig_level: 0.0,
            slope: Slope::Rise,
            trig_mode: TrigMode::Auto,
            holdoff: 0.0,
            trig_type: TrigType::Edge,
            trig_level2: 0.5,
            trig_nrej: false,
            trig_src2: 3,
            trig_slope2: Slope::Rise,
            trig_pol: Pol::Pos,
            trig_when: WhenKind::Lt,
            trig_t1: 20e-6,
            trig_t2: 40e-6,
            video_std: VideoStd::Pal,
            video_sync: VSync::AllFields,
            video_line: 10,
            pat: std::array::from_fn(|i| if i == 0 || i == NCH - 1 { Bit::H } else { Bit::X }),
            pat_levels: std::array::from_fn(|i| [1.25, 0.0, 1.65, 1.65][i.min(3)]),
            win_mode: WinMode::Enter,
            nth_idle: 50e-6,
            nth_n: 1,
            setup: 1e-6,
            hold: 1e-6,
            run: RunState::Run,
            status: TrigStatus::Wait,
            acq: AcqMode::Normal,
            avg_count: 16,
            segments: 0,
            mdepth: 0,
            persist: 0,
            intensity: 60,
            grid: Grid::Full,
            meas: Vec::new(),
            meas2: Vec::new(),
            cat: Vec::new(),
            dvm_on: false,
            dvm_src: 0,
            dvm_mode: 0,
            cnt_on: false,
            cnt_src: 0,
            cnt_mode: 0,
            cnt_digits: 5,
            ch_label: Default::default(),
            cursor: CursorMode::Off,
            cur_a: 0.3,
            cur_b: 0.7,
            cur_c: 0.3,
            cur_d: 0.7,
            touch_lock: false,
            fine_v: false,
            fine_t: false,
            tfine: 1.0,
            v_expand_gnd: false,
            h_expand: HExpand::Center,
            math: MathOp::Off,
            math_a: 0,
            math_b: 1,
            math_scale: 3,
            math_offset: 0.0,
            fft_win: Window::Hann,
            math_bw: 2e3,
            math_sel: 0,
            maths: [crate::math::MathCh::default(); crate::math::NMATH],
            math_fc: 10e3,
            math_lin: (1.0, 0.0),
            math_expr: Default::default(),
            log_ivl: 0,
            spec_margin: 10.0,
            decs: Default::default(),
            dec_sel: 0,
            pane: 0,
            split_pane: 0,
            meas_labels: false,
            strip_dock: StripDock::Under,
            trend_sel: [0, 0],
            dec_on: false,
            dec_src: 2,
            dec_baud: 7,
            dec_canrate: 2,
            dec_canh: false,
            dec_fdrate: 1,
            dec_arate: 0,
            dec_tick: 0,
            dec_nibbles: 6,
            dec_uspeed: 0,
            dec_src4: None,
            dec_la_in: false,
            dec_la: [Some(8), Some(10), None, None],
            dec_diff: None,
            trig_bus: BusWhat::Id,
            trig_bus_val: 0x123,
            dec_thr: 1.65,
            dec_ascii: true,
            dec_proto: Proto::Uart,
            dec_src2: 3,
            dec_cs: Some(1),
            dec_mode: 0,
            dec_bits: 8,
            dec_parity: oos_dsp::decode::Parity::None,
            bus: TraceBus::default(),
            zoom: false,
            auto_roll: true,
            zscale: 0,
            zdelay: 0.0,
            color_grade: false,
            // samples as dots by default
            dots: true,
            hdmi_wave: true,
            z_src: None,
            hist: Hist::Off,
            view: AView::Scope,
            spec_src: WaveId::Ch(0),
            spec_center: 0.0,
            spec_span: 0.0,
            spec_rbw: 0.0,
            // low side lobes at nearly half the resolution bandwidth of Flattop; the padding removes the scalloping
            spec_win: Window::Blackman,
            spec_sort: 0,
            spec_harm: 2,
            spec_trace: 0,
            spec_ref: 10.0,
            spec_wf: false,
            spec_dist: false,
            eye_src: WaveId::Ch(0),
            eye_rate: 0.0,
            eye_from_record: false,
            eye_place: EyePlace::Full,
            eye_ber: 1e-12,
            eye_pn_lo: 1e3,
            eye_pn_hi: 1e6,
            eye_npj: false,
            eye_b: None,
            eye_rail: None,
            eye_notch: 0.0,
            eye_hp: true,
            pw_v: WaveId::Ch(0),
            pw_i: WaveId::Ch(1),
            pw_mode: PowerMode::Quality,
            pw_fline: 0.0,
            pw_fsw: 0.0,
            pw_vin: None,
            pw_clk: None,
            pw_ref: None,
            bode_start: 100.0,
            bode_stop: 1e6,
            bode_points: 40,
            bode_amp: 1.0,
            bode_in: WaveId::Ch(0),
            bode_out: WaveId::Ch(1),
            bode_gen: 0,
            hw_stamp: 0,
            bode_addr: "192.168.1.100:5555".into(),
            bode_mode: oos_dsp::bode::Mode::Loop,
            meas_idle: 0.0,
            meas_out: oos_dsp::measure::Outside::Both,
            meas_n: 2,
            meas_data: false,
            meas_lpct: 0.1,
            meas_hpct: 0.9,
            meas_tsetup: 0.0,
            meas_thold: 0.0,
            spec_acpr_w: 1e6,
            spec_acpr_s: 2e6,
            mag_v: WaveId::Ch(0),
            mag_i: WaveId::Ch(1),
            mag_n: 1.0,
            mag_ae: 0.0,
            mag_le: 0.0,
            mag_rcu: 0.0,
            mag_plot: MagPlot::IPsi,
            mag_cur: 0,
            inv_page: InvPage::Dq0,
            inv_a: WaveId::Ch(0),
            inv_b: WaveId::Ch(1),
            inv_c: WaveId::Ch(2),
            inv_ang: InvAngle::Off,
            inv_poles: 1,
            inv_an: WaveId::Ch(3),
            inv_volt: true,
            inv_tq: None,
            inv_ev: None,
            inv_ei: None,
            inv_scale: 1.0,
            inv_degv: 1.0,
            inv_off: 0.0,
            inv_elec: false,
            inv_qa: 0,
            inv_qb: 1,
            inv_qi: None,
            inv_cpr: 1024.0,
            inv_swap: false,
            inv_ha: 0,
            inv_hb: 1,
            inv_hc: 2,
            dpt_v: WaveId::Ch(0),
            dpt_i: WaveId::Ch(1),
            dpt_g: WaveId::Ch(2),
            dpt_4th: DptFourth::Off,
            dpt_4ch: WaveId::Ch(3),
            dpt_pct: 10.0,
            dpt_zoom: DptZoom::Both,
            bw_src: 0,
            bw_hint: 0.0,
            bw_bins: 2000,
            gen_sel: 0,
            gen_bank: [GenCh::STANDARD; NGEN],
            afg_on: false,
            afg_arb: 0,
            grid_bright: 100,
            grid_frame: true,
            win_layout: WinLayout::One,
            density: Density::Scope,
            ch_win: [0; NCH],
            math_win: [0; crate::math::NMATH],
            la_win: 0,
            afg_arb_src: 0,
            afg_freq: 1000.0,
            afg_ampl: 1.0,
            afg_offset: 0.0,
            afg_wave: 0,
            afg_duty: 50.0,
            afg_symmetry: 50.0,
            afg_mod: 0,
            afg_mod_rate: 100.0,
            afg_mod_depth: 100.0,
            afg_fm_dev: 100.0,
            afg_pm_dev: 90.0,
            afg_mod_shape: 0,
            mask_on: false,
            zones: Default::default(),
            mask_src: 0,
            mask_x: 0.2,
            mask_y: 0.4,
            mask_stop: false,
            mask_inv: false,
            quick: Quick::Screenshot,
            la: Default::default(),
            sim_preset: 3,
            sim_slot: 0,
            sim_rate: 0.0,
            sim_prbs: 31,
            sim_vhigh: 0.0,
            sim_vlow: 0.0,
            sim_rise: 0.0,
            sim_noise: 0.0,
            sim_bw: 0.0,
            sim_deemph: 0.0,
            sim_ssc: false,
            sim_ssc_center: false,
            sim_ssc_deviation: 0.005,
            sim_ssc_hz: 33e3,
            sim_rj: 0.0,
            sim_pj: 0.0,
            sim_pjhz: 0.0,
            sim_dcd_pp: 0.0,
            sim_corr_rj: 0.0,
            sim_corr_hz: 1e6,
        };
        m.apply_sim_preset();
        m.ch[0].offset = -1.5;
        m.trig_level = 1.5;
        m
    }
}

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

    fn dsp_slope(s: Slope) -> oos_dsp::trigger::Slope {
        match s {
            Slope::Rise => oos_dsp::trigger::Slope::Rising,
            Slope::Fall => oos_dsp::trigger::Slope::Falling,
            Slope::Either => oos_dsp::trigger::Slope::Either,
        }
    }

    /// The time condition of the "When" setting (None = Off).
    pub fn when(&self) -> Option<oos_dsp::trigger::When> {
        use oos_dsp::trigger::When;
        let (a, b) = (self.trig_t1 as f64, self.trig_t2 as f64);
        match self.trig_when {
            WhenKind::Off => None,
            WhenKind::Gt => Some(When::Gt(a)),
            WhenKind::Lt => Some(When::Lt(a)),
            WhenKind::In => Some(When::In(a.min(b), a.max(b))),
            WhenKind::Out => Some(When::Out(a.min(b), a.max(b))),
        }
    }

    /// The configured trigger for the software trigger engine.
    pub fn trigger(&self) -> oos_dsp::trigger::Trigger {
        use oos_dsp::trigger::{Trigger, VideoSync, WinWhen};
        let (src, lv, lv2) = (self.trig_src, self.trig_level, self.trig_level2);
        let (hi, lo) = (lv.max(lv2), lv.min(lv2));
        let when = self.when().unwrap_or(oos_dsp::trigger::When::Gt(0.0));
        let sl = Self::dsp_slope(self.slope);
        match self.trig_type {
            TrigType::Edge => Trigger::Edge { src, slope: sl, level: lv },
            // the protocol trigger is evaluated by trig::find_all; this edge only stands for it elsewhere
            TrigType::Bus => Trigger::Edge { src: self.dec_src, slope: oos_dsp::trigger::Slope::Falling, level: self.dec_thr },
            TrigType::Pulse => Trigger::Pulse { src, pol: self.trig_pol, level: lv, when },
            TrigType::Slope => Trigger::Slope { src, slope: sl, lo, hi, when },
            TrigType::Video => Trigger::Video {
                src,
                std: self.video_std,
                level: lv,
                sync: match self.video_sync {
                    VSync::AllLines => VideoSync::AllLines,
                    VSync::Line => VideoSync::Line(self.video_line),
                    VSync::Odd => VideoSync::OddField,
                    VSync::Even => VideoSync::EvenField,
                    VSync::AllFields => VideoSync::AllFields,
                },
            },
            TrigType::Pattern => Trigger::Pattern { bits: self.pat, levels: self.pat_levels },
            TrigType::Duration => Trigger::Duration { bits: self.pat.map(|b| if matches!(b, Bit::R | Bit::F) { Bit::X } else { b }), levels: self.pat_levels, when },
            TrigType::Timeout => Trigger::Timeout { src, slope: sl, level: lv, time: self.trig_t1 as f64 },
            TrigType::Runt => Trigger::Runt { src, pol: self.trig_pol, lo, hi, when: self.when() },
            TrigType::Window => Trigger::Window {
                src,
                lo,
                hi,
                when: match self.win_mode {
                    WinMode::Enter => WinWhen::Enter,
                    WinMode::Exit => WinWhen::Exit,
                    WinMode::Inside => WinWhen::Inside(when),
                },
            },
            TrigType::Delay => Trigger::Delay { a: src, slope_a: sl, level_a: lv, b: self.trig_src2, slope_b: Self::dsp_slope(self.trig_slope2), level_b: lv2, when },
            TrigType::SetupHold => Trigger::SetupHold {
                data: src,
                clk: self.trig_src2,
                clk_slope: Self::dsp_slope(self.trig_slope2),
                data_level: lv,
                clk_level: lv2,
                setup: self.setup as f64,
                hold: self.hold as f64,
            },
            TrigType::NthEdge => Trigger::NthEdge { src, slope: sl, level: lv, idle: self.nth_idle as f64, n: self.nth_n },
        }
    }

    /// Channels the trigger reads.
    pub fn trig_channels(&self) -> [bool; NCH] {
        let mut u = [false; NCH];
        if self.trig_line().is_some() || self.bus_on_la() {
            return u;
        }
        match self.trig_type {
            TrigType::Pattern | TrigType::Duration => {
                for c in 0..NCH {
                    u[c] = self.pat[c] != Bit::X;
                }
            }
            TrigType::Delay | TrigType::SetupHold => {
                u[self.trig_src] = true;
                u[self.trig_src2] = true;
            }
            TrigType::Bus => {
                let r = self.dec_proto.roles();
                u[self.dec_src] = true;
                if r[1].is_some() {
                    u[self.dec_src2] = true;
                }
                if let (Some(_), Some(c)) = (r[2], self.dec_cs) {
                    u[c] = true;
                }
                if let (Some(_), Some(c)) = (r[3], self.dec_src4) {
                    u[c] = true;
                }
                if let (true, Some(c)) = (self.dec_proto.diff_ok(), self.dec_diff) {
                    u[c] = true;
                }
            }
            _ => u[self.trig_src] = true,
        }
        u
    }

    /// Time scale of the trigger condition (s): the search resolution must resolve it.
    pub fn trig_feature(&self) -> f64 {
        let t = |x: f32| x as f64;
        match self.trig_type {
            TrigType::Edge | TrigType::Pattern => f64::INFINITY,
            TrigType::Bus => self.bus_bit() / 20.0,
            TrigType::Video => 0.25e-6,
            TrigType::SetupHold => t(self.setup.min(self.hold)) / 4.0,
            TrigType::Timeout => t(self.trig_t1) / 20.0,
            TrigType::NthEdge => t(self.nth_idle) / 20.0,
            TrigType::Runt if self.trig_when == WhenKind::Off => f64::INFINITY,
            TrigType::Window if self.win_mode != WinMode::Inside => f64::INFINITY,
            _ => t(self.trig_t1.min(if matches!(self.trig_when, WhenKind::In | WhenKind::Out) { self.trig_t2 } else { f32::MAX })) / 20.0,
        }
    }

    /// Record length the trigger needs to decide (s).
    pub fn trig_span(&self) -> f64 {
        let t = |x: f32| x as f64;
        match self.trig_type {
            TrigType::Video => match self.video_sync {
                VSync::AllLines => 4.0 * self.video_std.line(),
                _ => 1.1 * self.video_std.lines() as f64 * self.video_std.line(),
            },
            TrigType::Timeout | TrigType::Duration => 3.0 * t(self.trig_t1.max(self.trig_t2)) + 6e-3,
            TrigType::NthEdge => 4.0 * t(self.nth_idle) + 2e-3,
            // room for a whole frame of the protocol before the trigger point (ARINC 429 auto rate: a low-speed word;
            // USB auto: low-speed packets)
            TrigType::Bus if self.dec_proto == Proto::Arinc429 && self.dec_arate == 0 => 200.0 / crate::decode::ARINC_RATES[1],
            TrigType::Bus if self.dec_proto == Proto::Usb && self.dec_uspeed == 0 => 200.0 / oos_dsp::decode_ext3::USB_LOW,
            TrigType::Bus => 200.0 * self.bus_bit(),
            TrigType::Edge | TrigType::Pattern => 0.0,
            _ => 3.0 * t(self.trig_t1.max(self.trig_t2)) + 3e-3,
        }
    }

    /// Bit time of the decoder's protocol (s), for the protocol trigger's sampling and search window.
    pub fn bus_bit(&self) -> f64 {
        match self.dec_proto {
            Proto::Uart | Proto::Lin | Proto::Uart9 | Proto::ModbusRtu | Proto::ModbusAscii => 1.0 / crate::decode::BAUDS[self.dec_baud],
            Proto::Can | Proto::CanFd => 1.0 / crate::decode::CAN_RATES[self.dec_canrate],
            Proto::I2c | Proto::Ds2482 => 2.5e-6,
            Proto::I3c => 0.1e-6,
            Proto::Spi | Proto::Qspi | Proto::Swd | Proto::Jtag | Proto::Parallel => 0.5e-6,
            Proto::I2s => 0.6e-6,
            Proto::Mil1553 => 1e-6,
            Proto::FlexRay => 0.1e-6,
            Proto::Dmx512 => 4e-6,
            Proto::OneWire => 15e-6,
            Proto::Spdif => 0.15e-6,
            Proto::UsbPd => 3.33e-6,
            // auto: high speed (the finer sampling)
            Proto::Arinc429 => {
                let r = crate::decode::ARINC_RATES[self.dec_arate];
                1.0 / if r > 0.0 { r } else { crate::decode::ARINC_RATES[2] }
            }
            // about 4 ticks per data bit; auto: the shortest tick
            Proto::Sent => {
                let t = crate::decode::SENT_TICKS[self.dec_tick];
                4.0 * if t > 0.0 { t } else { crate::decode::SENT_TICKS[0] }
            }
            // auto: full speed
            Proto::Usb => {
                use oos_dsp::decode_ext3::{UsbSpeed, USB_FULL, USB_LOW};
                1.0 / if crate::decode::USB_SPEEDS[self.dec_uspeed] == UsbSpeed::Low { USB_LOW } else { USB_FULL }
            }
            Proto::Manchester | Proto::Mlt3 => 1.0 / crate::decode::BAUDS[self.dec_baud],
        }
    }

    pub fn time_per_div(&self) -> f32 {
        tscales()[self.tscale] * self.tfine
    }

    /// The time base the acquisition runs at: the step, which covers at least the screen. Display scales below
    /// 1 ns/div stay on the plot; the ADC still takes a 1 ns/div window.
    pub fn hw_time_per_div(&self) -> f32 {
        tscales()[self.tscale].max(HW_MIN_TDIV)
    }

    /// Live analog / LA cannot display below 1 ns/div; the ADC window is that floor.
    pub fn floor_live_tdiv(&mut self) {
        if tscales()[self.tscale] < HW_MIN_TDIV {
            self.tscale = tscales().iter().position(|&t| t >= HW_MIN_TDIV).unwrap_or(0);
            self.tfine = 1.0;
        }
    }

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

    /// Settings of math channel `k` (the selected one from the edit fields).
    pub fn math_ch(&self, k: usize) -> crate::math::MathCh {
        if k == self.math_sel {
            crate::math::MathCh { lin: self.math_lin, op: self.math, a: self.math_a, b: self.math_b, scale: self.math_scale, fc: self.math_fc, bw: self.math_bw, offset: self.math_offset, win: self.fft_win }
        } else {
            self.maths[k]
        }
    }

    /// The settings of the decoder the menu edits.
    pub fn dec_cfg(&self) -> DecCfg {
        DecCfg { on: self.dec_on.clone(), src: self.dec_src.clone(), baud: self.dec_baud.clone(), canrate: self.dec_canrate.clone(), canh: self.dec_canh.clone(), fdrate: self.dec_fdrate.clone(), arate: self.dec_arate.clone(), tick: self.dec_tick.clone(), nibbles: self.dec_nibbles.clone(), uspeed: self.dec_uspeed.clone(), src4: self.dec_src4.clone(), la_in: self.dec_la_in.clone(), la: self.dec_la.clone(), diff: self.dec_diff.clone(), thr: self.dec_thr.clone(), ascii: self.dec_ascii.clone(), proto: self.dec_proto.clone(), src2: self.dec_src2.clone(), cs: self.dec_cs.clone(), mode: self.dec_mode.clone(), bits: self.dec_bits.clone(), parity: self.dec_parity.clone() }
    }

    fn dec_load(&mut self, c: &DecCfg) {
        self.dec_on = c.on.clone();
        self.dec_src = c.src.clone();
        self.dec_baud = c.baud.clone();
        self.dec_canrate = c.canrate.clone();
        self.dec_canh = c.canh.clone();
        self.dec_fdrate = c.fdrate.clone();
        self.dec_arate = c.arate.clone();
        self.dec_tick = c.tick.clone();
        self.dec_nibbles = c.nibbles.clone();
        self.dec_uspeed = c.uspeed.clone();
        self.dec_src4 = c.src4.clone();
        self.dec_la_in = c.la_in.clone();
        self.dec_la = c.la.clone();
        self.dec_diff = c.diff.clone();
        self.dec_thr = c.thr.clone();
        self.dec_ascii = c.ascii.clone();
        self.dec_proto = c.proto.clone();
        self.dec_src2 = c.src2.clone();
        self.dec_cs = c.cs.clone();
        self.dec_mode = c.mode.clone();
        self.dec_bits = c.bits.clone();
        self.dec_parity = c.parity.clone();
    }

    /// Settings of decoder `k`.
    pub fn dec_slot(&self, k: usize) -> DecCfg {
        if k == self.dec_sel {
            self.dec_cfg()
        } else {
            self.decs[k].clone().unwrap_or_else(|| Model::default().dec_cfg())
        }
    }

    pub fn dec_is_on(&self, k: usize) -> bool {
        if k == self.dec_sel {
            self.dec_on
        } else {
            self.decs[k].as_ref().is_some_and(|c| c.on)
        }
    }

    /// The model as decoder `k` sees it, for the code that reads the `dec_*` fields.
    pub fn dec_view(&self, k: usize) -> Model {
        let mut m = self.clone();
        if k != self.dec_sel {
            m.dec_load(&self.dec_slot(k));
        }
        m
    }

    /// Makes decoder `k` the one the menu edits.
    pub fn dec_select(&mut self, k: usize) {
        let k = k.min(DECODERS - 1);
        if k != self.dec_sel {
            let c = self.dec_slot(k);
            self.decs[self.dec_sel] = Some(self.dec_cfg());
            self.dec_load(&c);
            self.dec_sel = k;
        }
    }

    /// Writes `c` into slot `k` (setups).
    pub fn dec_store(&mut self, k: usize, c: DecCfg) {
        if k == self.dec_sel {
            self.dec_load(&c);
        } else {
            self.decs[k] = Some(c);
        }
    }

    /// Makes math channel `k` the one the menu edits.
    pub fn math_select(&mut self, k: usize) {
        self.maths[self.math_sel] = self.math_ch(self.math_sel);
        self.math_sel = k.min(crate::math::NMATH - 1);
        let c = self.maths[self.math_sel];
        self.math_lin = c.lin;
        (self.math, self.math_a, self.math_b, self.math_scale, self.math_fc, self.math_bw, self.math_offset, self.fft_win) = (c.op, c.a, c.b, c.scale, c.fc, c.bw, c.offset, c.win);
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
        if self.la.on && !crate::ui::la::available() {
            self.la.on = false;
            changed = true;
        }
        if n >= NCH {
            return changed;
        }
        // every other channel source that points at a missing input: CH1, or none for the optional ones
        for s in [&mut self.trig_src, &mut self.trig_src2, &mut self.dec_src, &mut self.dec_src2, &mut self.mask_src, &mut self.bw_src, &mut self.afg_arb_src]
            .into_iter()
            .chain(self.zones.iter_mut().map(|z| &mut z.src))
        {
            if *s >= n {
                *s = 0;
                changed = true;
            }
        }
        for s in [&mut self.mag_v, &mut self.mag_i, &mut self.inv_a, &mut self.inv_b, &mut self.inv_c, &mut self.inv_an, &mut self.dpt_v, &mut self.dpt_i, &mut self.dpt_g, &mut self.dpt_4ch, &mut self.spec_src, &mut self.eye_src, &mut self.pw_v, &mut self.pw_i, &mut self.bode_in, &mut self.bode_out] {
            if let WaveId::Ch(c) = *s {
                if c >= n {
                    *s = WaveId::Ch(0);
                    changed = true;
                }
            }
        }
        for s in [&mut self.dec_src4, &mut self.dec_diff, &mut self.dec_cs, &mut self.z_src] {
            if s.is_some_and(|c| c >= n) {
                *s = None;
                changed = true;
            }
        }
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
        for s in [&mut self.inv_tq, &mut self.inv_ev, &mut self.inv_ei, &mut self.eye_b, &mut self.eye_rail, &mut self.pw_vin, &mut self.pw_clk] {
            if let Some(WaveId::Ch(c)) = *s {
                if c >= n {
                    *s = None;
                    changed = true;
                }
            }
        }
        // math sources: channels 0..NCH, then math channels (always there)
        for s in [&mut self.math_a, &mut self.math_b].into_iter().chain(self.maths.iter_mut().flat_map(|m| [&mut m.a, &mut m.b])) {
            if !src_ok(*s) {
                *s = 0;
                changed = true;
            }
        }
        changed
    }
    /// Maximum sample rate for the number of enabled channels (1.25 GSa/s shared by the ADC cores; platform limits,
    /// `oos_hal::model`).
    pub fn max_srate(&self) -> f64 {
        let r = oos_hal::model::platform().limits.srate(self.n_on());
        // the analog rate is halved while the LA is on and a channel is shown (la.md 3.1)
        if self.la_on() && self.n_on() > 0 {
            r / 2.0
        } else {
            r
        }
    }
    /// The logic analyser is on (and the model has it).
    /// Comparator hysteresis of the trigger in divisions (trigger.md 2.3): 0.3, 0.7 with noise reject.
    pub fn trig_hyst_div(&self) -> f32 {
        if self.trig_nrej { 0.7 } else { 0.3 }
    }

    pub fn la_on(&self) -> bool {
        self.la.on && crate::ui::la::available()
    }
    /// The trigger types that can use an LA line (the stock system also has them for D0..D15; trigger.md 5).
    pub fn trig_la_ok(&self) -> bool {
        self.la_on() && matches!(self.trig_type, TrigType::Edge | TrigType::Pulse)
    }
    /// The pattern or duration trigger looks at LA lines.
    pub fn pat_on_la(&self) -> bool {
        self.la_on() && matches!(self.trig_type, TrigType::Pattern | TrigType::Duration) && self.pat_la.iter().any(|&b| b != Bit::X)
    }
    /// The protocol trigger on LA lines (the decoder's input is the LA).
    pub fn bus_on_la(&self) -> bool {
        self.trig_type == TrigType::Bus && self.dec_on_la()
    }
    /// The hardware trigger cannot be refined on the analog samples (it is on LA lines).
    pub fn trig_on_la(&self) -> bool {
        self.trig_line().is_some() || self.pat_on_la() || self.bus_on_la()
    }
    /// The LA line the trigger is on: only while the LA is on and with a type that can use one.
    pub fn trig_line(&self) -> Option<u8> {
        self.trig_la.filter(|_| self.trig_la_ok())
    }
    /// The decoder reads logic analyser lines (the choice only counts while the LA is on).
    pub fn dec_on_la(&self) -> bool {
        self.dec_la_in && self.la_on()
    }
    pub fn max_mem(&self) -> u64 {
        // limits 125 / 62.5 / 31.25 M (stock: 50 / 25 / 10 M); above 78.6 M only with the larger segment cap
        // (`hal::dho::hz::MEM_BURSTS`); overridable in limits.conf (`mem.1ch` ...)
        // with the LA on: 31.25 / 12.5 / 10 M (`max_mem_la`, la.md 3.2)
        oos_hal::model::platform().limits.mem(self.n_on(), self.la_on())
    }
    /// (sample rate, memory depth) as the acquisition would use them.
    pub fn srate_depth(&self) -> (f64, u64) {
        let window = self.hw_time_per_div() as f64 * HDIV as f64;
        let want = mdepths()[self.mdepth].1;
        let maxsr = self.max_srate();
        if want == 0 {
            let depth_cap = 1_000_000u64.min(self.max_mem());
            let sr = maxsr.min(depth_cap as f64 / window);
            let sr = quantize_srate(sr, maxsr);
            (sr, ((sr * window).round() as u64).max(1000).min(depth_cap))
        } else {
            let depth = want.min(self.max_mem());
            let sr = quantize_srate(maxsr.min(depth as f64 / window), maxsr);
            (sr, depth)
        }
    }
}

/// Sample rates are the ADC rate divided by an integer decimation (1-2-5 steps here).
fn quantize_srate(sr: f64, max: f64) -> f64 {
    let mut best = max;
    let mut d = 1.0;
    while best > sr * 1.0001 && d < 1e12 {
        for m in [1.0, 2.0, 5.0] {
            let c = max / (d * m);
            if c <= sr * 1.0001 {
                return c;
            }
        }
        d *= 10.0;
        best = max / d;
    }
    best
}

/// Engineering format: 3 significant digits by default, SI prefix, unit.
pub fn si(v: f64, unit: &str) -> String {
    si_digits(v, unit, 3)
}

pub fn si_digits(v: f64, unit: &str, digits: usize) -> String {
    if !v.is_finite() {
        return format!("---- {}", unit);
    }
    if v == 0.0 {
        return format!("0 {}", unit);
    }
    let prefixes = [(1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "\u{b5}"), (1e-9, "n"), (1e-12, "p")];
    let a = v.abs();
    let (mult, p) = prefixes.iter().copied().find(|(m, _)| a >= *m * 0.9995).unwrap_or((1e-12, "p"));
    let x = v / mult;
    let ax = x.abs();
    let dec = if ax >= 100.0 { digits.saturating_sub(3) } else if ax >= 10.0 { digits.saturating_sub(2) } else { digits.saturating_sub(1) };
    format!("{:.*} {}{}", dec, x, p, unit)
}

/// Compact scale label: "200 µs", "1 V", "500 mV" (no trailing zeros).
pub fn si_short(v: f64, unit: &str) -> String {
    let s = si_digits(v, unit, 3);
    // "1.00 V" -> "1 V", "2.50 V" -> "2.5 V"
    if let Some((num, rest)) = s.split_once(' ') {
        if num.contains('.') {
            let t = num.trim_end_matches('0').trim_end_matches('.');
            return format!("{} {}", t, rest);
        }
    }
    s
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    #[test]
    fn source_steps_skip_missing_channels() {
        // 2-channel unit: CH1, CH2, then (math sources) M1.. at NCH..
        assert_eq!(src_step_in(0, NCH, 1, 2), 1);
        assert_eq!(src_step_in(1, NCH, 1, 2), 0);
        assert_eq!(src_step_in(0, NCH, -1, 2), 1);
        assert_eq!(src_step_in(1, NCH + 2, 1, 2), NCH);
        assert_eq!(src_step_in(NCH + 1, NCH + 2, 1, 2), 0);
        // a stale CH4 source moves from CH1
        assert_eq!(src_step_in(3, NCH, 1, 2), 1);
        // 4 channels: plain cycling
        assert_eq!(src_step_in(3, NCH, 1, 4), 0);
        assert_eq!(src_step_in(2, NCH + 1, 1, 4), 3);
        assert_eq!((opt_src_step_in(2, 1, 2), opt_src_step_in(0, -1, 2), opt_src_step_in(4, 1, 4)), (0, 2, 0));
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
    fn every_generator_keeps_its_settings() {
        let mut m = Model::default();
        assert_eq!(m.gen_read(), GenCh::STANDARD);
        m.afg_freq = 5e3;
        m.afg_on = true;
        if NGEN < 2 {
            m.gen_select(1);
            assert_eq!((m.gen_sel, m.afg_freq), (0, 5e3), "no second generator on this board");
            return;
        }
        m.gen_select(1);
        assert_eq!(m.gen_read(), GenCh::STANDARD);
        m.afg_freq = 7e3;
        assert_eq!(m.gen_ch(0).freq, 5e3);
        m.gen_select(0);
        assert!(m.afg_freq == 5e3 && m.afg_on);
        assert_eq!(m.gen_ch(1).freq, 7e3);
    }

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
    fn the_boards_lists_are_usable() {
        let d = mdepths();
        assert_eq!(d[0].1, 0, "Auto first");
        assert!(d.windows(2).all(|w| w[0].1 < w[1].1), "ascending");
        assert_eq!(d[d.len() - 1].1, oos_hal::model::Limits::standard().max_mem[0], "the list ends at the largest depth");
        assert!(board().couplings().contains(&Coupling::Dc), "the fallback of fit_platform");
        assert!(n_ch() <= NCH && gen_count() <= NGEN);
    }
}
