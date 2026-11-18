// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Layout, rendering, menus and input handling of the OOS scope screen (1024 x 600).

use crate::board::{board, Board};
use crate::font::{Font, Fonts};
use crate::gfx::*;
use crate::math::*;
use crate::model::*;
use crate::wave::*;

mod acquire;
mod afg;
mod analysers;
mod analysis;
mod autoset;
mod bars;
#[cfg(test)]
mod batch1_tests;
#[cfg(test)]
mod batch2_tests;
#[cfg(test)]
mod batch5_tests;
pub mod bode;
mod bt;
mod bus;
mod busview;
mod deskew;
mod dpt;
mod eyepage;
pub(crate) mod facts;
#[cfg(test)]
mod fix_tests;
mod hwmask;
mod hwzone;
mod init;
mod input;
mod item;
mod logger;
mod magnetic;
mod meascatalog;
mod measlabels;
mod measure_pairs;
mod measurement_nav;
mod menu_actions;
mod menu_dialog;
mod menu_items;
mod menu_text;
mod motor;
mod offline;
mod overclock;
mod pages;
mod panes;
mod plot_layout;
pub mod pointer;
mod records;
mod reflook;
mod render;
mod render_derived;
mod render_marks;
mod save_recall;
mod scpi_operations;
mod scroll;
mod selfcal;
mod sertrig;
mod service;
mod sim;
pub mod startup;
mod strip;
mod sysinfo;
mod tables;
mod tools;
mod trigger_controls;
mod update;
mod viewmore;
mod windows;
mod wlan;
mod worker;
mod workflow;
mod workspace;
pub(crate) use scpi_operations::{OperationState, OperationToken, Operations};
mod backlight;
mod bus_candidate;
mod capture_memory;
mod features;
mod graphics;
#[cfg(test)]
mod interplay_tests;
mod mathwave;
mod monitor;
mod reference_decode;
mod reference_search;
mod remote;
#[cfg(test)]
mod session_tests;
mod setting_items;
mod ssh;
mod time;
pub mod views;
#[cfg(test)]
use acquire::{fast_deposit, fast_plan, fast_take_next};
use item::{Item, PARTS};
pub use views::VIEWS_MAX;
#[cfg(test)]
mod backend_followup_tests;
mod bwtest;
pub mod la;
#[cfg(test)]
mod lint;
mod script;
#[cfg(test)]
pub(crate) mod testkit;

// The frame (top bar, waveform area, bottom bar with the cards) comes from the screen size: `Ui::frame`,
// `crate::layout`. Top bar: run state, menu buttons, network (buttons drawn 30 px high, tapped over the full bar
// height). Plot: the whole width and everything between the bars; the channel markers (left edge), the trigger level
// marker (right edge), the trigger position marker (top edge), the axis labels, the status line and the measurement
// results are drawn inside it.
pub use crate::layout::{Frame, Screen, METRICS, PANEL_H, PANEL_W};
/// Width of the marker strips at the left and right edge of the plot (the labels keep clear of them).
const MARK_W: i32 = 26;
/// Width of the measurement strip when it is docked on the left or the right.
const STRIP_SIDE: i32 = 220;
/// Height of the fork's compact bottom bar (Menu, source pills, Overlay / Stacked, time, trigger) that replaces the
/// cards: `Ui::frame` gives the body the rest of `METRICS.bottom_h`.
pub const BOTTOM_H: i32 = 36;
/// The frame on the 1024 x 600 panel, for the code that still places things for the panel (`PANEL_W` / `PANEL_H`);
/// new code takes `Ui::frame` and `Ui::scr`.
pub const W: i32 = PANEL_W;
pub const H: i32 = PANEL_H;
pub const TOP: Rect = Rect::new(0, 0, W, METRICS.top_h);
#[cfg(test)]
const PLOT_Y: i32 = TOP.h;
pub const BOTTOM: Rect = Rect::new(0, H - BOTTOM_H, W, BOTTOM_H);
/// The normal text size's `Lay::results_w` and `Lay::strip_row`, for the same code.
const RESULTS_W: i32 = LAYS[0].results_w;
const STRIP_ROW: i32 = LAYS[0].strip_row;

/// Where a picture is drawn: everything on `under`, or with `over` (System > Graphics = GPU) the layer over the
/// traces, and the traces as `jobs` for the GPU (`Ui::render_layers`, `crate::gpu`).
pub struct Layers<'a> {
    pub under: &'a mut Canvas,
    pub over: Option<&'a mut Canvas>,
    pub jobs: Vec<TraceJob>,
}

impl Layers<'_> {
    /// The canvas for what lies over the traces.
    pub fn over(&mut self) -> &mut Canvas {
        match &mut self.over {
            Some(o) => o,
            None => self.under,
        }
    }
}

/// A plot window's traces for the GPU: the phosphor maps of the shown channels (`zoom`: of the zoom window) in
/// `area`, with the tables the CPU would use (`draw_traces`, `draw_graded`).
pub struct TraceJob {
    pub area: Rect,
    pub zoom: bool,
    pub graded: bool,
    pub show: [bool; NCH],
    /// per channel 1024 alphas (not graded)
    pub alpha: Vec<u8>,
    /// 1024 colours (graded)
    pub colours: Vec<u32>,
    pub shift: [u32; NCH],
    pub col: [u32; NCH],
}
/// Layout sizes that grow with the text (Display > Text size). Everything else keeps its place: touch targets are
/// 40 px high at every size, and the larger fonts are set into them.
pub struct Lay {
    /// pitch of the measurement and cursor rows under the plot (the bar is 2 px lower)
    pub strip_row: i32,
    /// menu dialogs: label column and column width of the list rows, column width of the button grids
    pub dlg_label: i32,
    pub dlg_col: i32,
    pub grid_col: i32,
    /// width of the measurement results overlay (statistics on) at the top right of the plot
    pub results_w: i32,
    /// baselines in the bottom cards: name, big value, line under it
    pub card: (i32, i32, i32),
    /// row pitch of the results overlay: one line, two lines
    pub res_pitch: (i32, i32),
}

/// The layouts of Normal, Large and Larger.
pub const LAYS: [Lay; 3] = [
    Lay {
        strip_row: 18,
        dlg_label: 96,
        dlg_col: 400,
        grid_col: 150,
        results_w: 196,
        card: (18, 42, 58),
        res_pitch: (22, 42),
    },
    Lay {
        strip_row: 21,
        dlg_label: 104,
        dlg_col: 420,
        grid_col: 168,
        results_w: 224,
        card: (19, 43, 59),
        res_pitch: (24, 46),
    },
    Lay {
        strip_row: 24,
        dlg_label: 116,
        dlg_col: 440,
        grid_col: 190,
        results_w: 250,
        card: (20, 43, 59),
        res_pitch: (26, 50),
    },
];
/// Menu dialogs: header height, row pitch and control height of the list rows (touch targets of 40 px).
const DLG_HEAD: i32 = 50;
const DLG_ROW: i32 = 44;
const DLG_CTRL: i32 = 40;
/// records kept for history playback
const HISTORY: usize = 200;
/// Seconds the trigger level line stays after the level changed.
const LEVEL_LINE_S: f64 = 1.5;
/// Longest wait of a STOP for the hardware's final frame (`Ui::stop_final`): a batch in progress, its read and the raw
/// read of the newest frame.
const STOP_FINAL_WAIT: f64 = 3.0;
/// LA words a history entry keeps: above this only the newest record has its LA (200 x 1 M words = 400 MB at most).
const HISTORY_LA_WORDS: usize = 1_000_000;
/// Segment counts of the segmented recording (0 = off); the menu offers those within the limit of the set-up
/// (`Ui::seg_limit`).
pub const SEGMENTS: [usize; 12] = [0, 10, 50, 100, 200, 500, 1000, 2000, 5000, 10_000, 20_000, 50_000];
/// Most segments of the software recording (history RAM).
pub const SEG_MAX: usize = 1000;
/// Most hardware segments (the stock UI limit; the frame memory holds up to 491,520 of the smallest).
pub const SEG_HW_MAX: usize = 100_000;
/// Hardware segments: intervals after each segment (0x4068, at most 2^32 x 6.4 ns = 27 s).
pub const SEG_IVLS: [f32; 8] = [0.0, 1e-6, 10e-6, 100e-6, 1e-3, 10e-3, 100e-3, 1.0];
pub const SEG_IVL_MAX: f32 = 10.0;

// dark theme
const BG: Rgb = rgb(0x0a, 0x0a, 0x0b);
const BAR: Rgb = rgb(0x15, 0x15, 0x16);
const BTN: Rgb = rgb(0x1e, 0x1e, 0x20);
const BTN_BORDER: Rgb = rgb(0x34, 0x34, 0x37);
const CARD_BORDER: Rgb = rgb(0x2c, 0x2c, 0x2e);
const LINE: Rgb = rgb(0x3a, 0x3a, 0x3d);
const DLG_BG: Rgb = rgb(0x12, 0x12, 0x13);
const TEXT: Rgb = rgb(0xf2, 0xf2, 0xf2);
const LABEL: Rgb = rgb(0xc8, 0xc8, 0xc8);
const DIM: Rgb = rgb(0x90, 0x90, 0x92);
const AXIS_TEXT: Rgb = rgb(0xa4, 0xa4, 0xa6);
const DARK_TEXT: Rgb = rgb(0x0a, 0x0a, 0x0b);
const ACCENT: Rgb = rgb(0xf4, 0xf4, 0xf4);
const GREEN: Rgb = rgb(0x3f, 0xb9, 0x50);
const RED: Rgb = rgb(0xf8, 0x51, 0x49);
const AMBER: Rgb = rgb(0xe3, 0xb3, 0x41);
const BLUE: Rgb = rgb(0x79, 0xa6, 0xff);
const CURSOR: Rgb = rgb(0xc4, 0xc4, 0xc6);
/// Figures counted from the signal, and figures that rest on a model.
const MEASURED: Rgb = TEXT;
const MODELLED: Rgb = AMBER;
const GRID_LINE: Rgb = rgb(0x20, 0x20, 0x22);
/// translucent read-outs over the analysers
const PANEL_BG: Rgb = rgb(0x16, 0x16, 0x17);
/// every second row of a table
const ROW_ALT: Rgb = rgb(0x10, 0x10, 0x11);
const GRID_AXIS: Rgb = rgb(0x38, 0x38, 0x3b);
/// graticule: dots of the division lines, frame / centre axes / ticks (the classic dotted scope look)
const GRID_DOT: Rgb = rgb(0x68, 0x68, 0x6b);
const GRID_FRAME: Rgb = rgb(0x64, 0x64, 0x67);
/// the measurement page: the box of the picked measurement, a step lighter than a button so it reads as one panel
const PREVIEW_BG: Rgb = rgb(0x24, 0x24, 0x28);
/// the measurement page: the header row of a group, between a button and its border
const GROUP_HEAD: Rgb = rgb(0x2a, 0x2a, 0x2e);
/// the self-cal banner over the plot: a dark amber, so it is not taken for a dialog while the inputs must stay free
const CAL_BANNER: Rgb = rgb(0x30, 0x22, 0x08);

/// Averaging counts of Acquire > Averages.
/// Stops of the density scale, rare to frequent (`heat_palette`).
const HEAT_STOPS: [(f32, f32, f32); 5] =
    [(40.0, 70.0, 255.0), (0.0, 220.0, 255.0), (40.0, 240.0, 70.0), (255.0, 235.0, 30.0), (255.0, 40.0, 30.0)];
pub const AVERAGES: [u32; 10] = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];

/// Front-panel key codes (data byte of the MCU frame, see docs/protocols/frontpanel.md).
pub mod key {
    pub const REF: u8 = 0x01;
    pub const LA: u8 = 0x02;
    pub const GI: u8 = 0x03;
    pub const HMENU: u8 = 0x04;
    pub const NAVIGATE: u8 = 0x05;
    pub const CLEAR: u8 = 0x0b;
    pub const SLOPE: u8 = 0x09;
    pub const FORCE: u8 = 0x0a;
    pub const RUNSTOP: u8 = 0x0c;
    pub const AUTO: u8 = 0x0d;
    pub const SINGLE: u8 = 0x0e;
    pub const MEASURE: u8 = 0x0f;
    pub const CURSOR: u8 = 0x10;
    pub const DEFAULT: u8 = 0x11;
    pub const ANALYSE: u8 = 0x12;
    pub const TOUCHLOCK: u8 = 0x13;
    pub const QUICK: u8 = 0x14;
    pub const CH1: u8 = 0x15;
    pub const MATH: u8 = 0x19;
    pub const TMENU: u8 = 0x1a;
    pub const POWER: u8 = 0x1b;
    pub const HPOS_CW: u8 = 0x8a;
    pub const VPOS_CW: u8 = 0x9a;
    pub const MULTI2_CW: u8 = 0xaa;
    pub const MULTI1_CW: u8 = 0xba;
    pub const HSCALE_CW: u8 = 0xca;
    pub const VSCALE_CW: u8 = 0xda;
    pub const TLEVEL_CW: u8 = 0xea;
}

/// Options of the USB speed (decode::USB_SPEEDS).
const USPEED_NAMES: [&str; 3] = ["Auto", "Low 1.5M", "Full 12M"];

/// Longest record the web page takes as CSV (about 50 MB of text).
const WEB_CSV_MAX: usize = 2_000_000;

/// Height of a decoder band in the plot.
const DEC_BAND: i32 = 28;

/// Colours of the references.
const REF_COLORS: [(&str, Rgb); 5] = [
    ("White", rgb(0xd8, 0xd8, 0xd8)),
    ("Red", rgb(0xf0, 0x60, 0x60)),
    ("Green", rgb(0x60, 0xe0, 0x90)),
    ("Blue", rgb(0x70, 0xa8, 0xf8)),
    ("Violet", rgb(0xc8, 0x88, 0xf0)),
];
/// Kinds of delay and phase measurement and of second panes, for the setups.
pub const PAIRS_N: usize = tools::PAIRS.len();
/// The short names of the two-source variants (strip, SCPI).
pub fn pair_short_name(k: usize) -> &'static str {
    tools::PAIRS[k.min(PAIRS_N - 1)].0
}
/// The unit of a two-source variant: degrees for phase, none for the setup/hold violation count, else seconds.
pub fn pair_unit(k: usize) -> &'static str {
    match tools::PAIRS[k.min(PAIRS_N - 1)].1 {
        tools::PairKind::Phase => "\u{b0}",
        tools::PairKind::SetupAndHold => "",
        _ => "s",
    }
}

pub(crate) use analysers::{scpi_views, stored_views};

/// Clock recovery of the eye for the measurement modules: samples per UI and the recovered clock.
pub fn eye_infer(v: &[f32], dt: f64, rate: Option<f64>) -> Result<(f64, oos_dsp::eye::Clock), String> {
    eyepage::automatic::infer(v, dt, rate, false).map(|(s, clk)| (s.samples_per_ui, clk))
}
pub const PANES_N: usize = tables::PANES.len();
pub use eyepage::EYE_ITEMS;
pub use panes::{PANE_HARMS, TREND_SEL_N};
pub use tools::{CNT_MODES, DVM_MODES};
pub const LOG_STEPS_N: usize = logger::LOG_STEPS.len();

/// Keys of the frequency keypad: the unit ends the entry.
const FREQ_KEYS: [&str; 18] =
    ["7", "8", "9", "mHz", "4", "5", "6", "Hz", "1", "2", "3", "kHz", "0", ".", "Del", "MHz", "Clear", "Cancel"];
/// Keys of the hex keypad picker.
/// Largest protocol trigger value: 40 bits (five I2C data bytes).
pub const BUS_VAL_MAX: u64 = 0xff_ffff_ffff;

const KEYPAD: [&str; 19] =
    ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "Del", "Clear", "OK"];

/// A channel key on the front panel held this many seconds: AC/DC (`Ui::panel_key`).
pub const LONG_PRESS: f64 = 0.6;

pub const KEY_NAMES: [(&str, u8); 30] = [
    ("navfwd", 0x06),
    ("navplay", 0x07),
    ("navback", 0x08),
    ("ref", 0x01),
    ("la", 0x02),
    ("gi", 0x03),
    ("hmenu", 0x04),
    ("navigate", 0x05),
    ("slope", 0x09),
    ("force", 0x0a),
    ("clear", 0x0b),
    ("runstop", 0x0c),
    ("auto", 0x0d),
    ("single", 0x0e),
    ("measure", 0x0f),
    ("cursor", 0x10),
    ("default", 0x11),
    ("analyse", 0x12),
    ("touchlock", 0x13),
    ("quick", 0x14),
    ("ch1", 0x15),
    ("ch2", 0x16),
    ("ch3", 0x17),
    ("ch4", 0x18),
    ("math", 0x19),
    ("tmenu", 0x1a),
    ("power", 0x1b),
    ("hpos", 0x8c),
    ("vpos", 0x9c),
    ("tlevel", 0xec),
];
/// Knob names -> CW code (CCW = CW + 1, push = CW + 2).
pub const KNOB_NAMES: [(&str, u8); 7] = [
    ("hpos", key::HPOS_CW),
    ("vpos", key::VPOS_CW),
    ("multi2", key::MULTI2_CW),
    ("multi1", key::MULTI1_CW),
    ("hscale", key::HSCALE_CW),
    ("vscale", key::VSCALE_CW),
    ("tlevel", key::TLEVEL_CW),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuId {
    Main,
    Channel(usize),
    /// the rarer settings of a channel
    ChMore(usize),
    /// measurements into a file at an interval (ui/logger.rs)
    Logger,
    Horizontal,
    Trigger,
    Acquire,
    Measure,
    MeasAdd,
    /// short menu from a press-and-hold on Measure; Add meas is its shortcut
    MeasHold,
    /// delay and phase between two channels
    MeasPair,
    /// digital voltmeter and counter (ui/tools.rs)
    Dvm,
    Cursor,
    Display,
    Utility,
    Math,
    Ref,
    Decode,
    /// one bus drawn on the plot
    Bus,
    Storage,
    Mask,
    Zone,
    Spectrum,
    Eye,
    Power,
    Magnetic,
    Inverter,
    Dpt,
    Identify,
    SelfCal,
    /// network overview and WLAN through a USB stick (ui/wlan.rs)
    Network,
    /// System > Features (ui/features.rs) and its decoder page
    Features,
    /// System > Features > Decoders, page n (`features::DEC_PAGE` a page)
    Decoders(usize),
    /// which network services run (ui/remote.rs)
    Remote,
    /// the SSH server: password, host key, trusted keys (ui/ssh.rs)
    Ssh,
    /// mice and keyboards through a Bluetooth stick (ui/bt.rs)
    Bluetooth,
    /// clock, NTP and time zone (ui/time.rs)
    Time,
    Bode,
    Sim,
    SimAdvanced,
    Afg,
    BwTest,
    /// the overflow menu of an analyser view or XY (viewmore.rs)
    ViewMore,
    /// logic analyser (ui/la.rs) and its line selection
    La,
    LaLines,
    LaPat,
    /// waveform windows and full screen (ui/windows.rs)
    Windows,
    /// list of all options of a choice (`Ui::pick` names the item); picking returns to the menu of the item
    Pick,
    /// Full legacy measurement and pair catalog, including Selected.
    MeasSelected,
}

/// Buttons of the top bar.
#[derive(Clone, Copy, PartialEq, Debug)]
enum TopBtn {
    Menu(MenuId),
    /// Back to live acquisition from a simulation or a stored waveform.
    Live,
}

/// An open picker: the choice item, and the menu (with its Back target and focus) to return to.
#[derive(Clone, Copy, Debug)]
struct Pick {
    it: Item,
    menu: MenuId,
    parent: Option<MenuId>,
    focus: usize,
}

#[derive(Clone, Copy, Default)]
struct MStat {
    n: u64,
    sum: f64,
    sumsq: f64,
    min: f64,
    max: f64,
}

impl MStat {
    fn add(&mut self, v: f64) {
        if self.n == 0 {
            self.min = v;
            self.max = v;
        }
        self.n += 1;
        self.sum += v;
        self.sumsq += v * v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }
}

#[derive(Clone)]
struct RefWave {
    src: usize,
    /// index into REF_COLORS
    color: usize,
    label: String,
    t0: f64,
    dt: f64,
    v: std::sync::Arc<Vec<f32>>,
    vpd: f32,
    offset: f32,
    on: bool,
}

#[cfg(test)]
impl RefWave {
    /// A shown reference of the samples `v` (`dt` apart, the first at `t0`): slot colour 0, no label, 1 V/div, no
    /// offset; the tests change what they need with `..RefWave::test(..)`.
    fn test(v: Vec<f32>, dt: f64, t0: f64) -> RefWave {
        RefWave { src: 0, color: 0, label: String::new(), t0, dt, v: v.into(), vpd: 1.0, offset: 0.0, on: true }
    }
}

#[derive(PartialEq)]
enum Kind {
    Choice,
    Value,
    Action,
}

pub enum Ev {
    Key(u8),
    Touch { id: i32, phase: Phase, x: i32, y: i32 },
    Wheel { x: i32, y: i32, steps: i32, ctrl: bool, shift: bool, alt: bool },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Pending,
    /// went down in an edge strip of the plot with more than one view: a horizontal swipe switches views, a vertical
    /// drag becomes `Pending` (ui/views.rs)
    ViewEdge,
    /// drawing zone k (box: from the touch-down point to the finger; shape: the finger's path)
    Zone(usize),
    /// draw-a-box is armed: draw repeated zoom boxes until a tap/menu cancels
    ZoomBox,
    /// the corner button: its drawn origin when the finger went down, and where it sat before this gesture
    FullBtn {
        ox: i32,
        oy: i32,
        prev: Option<(i32, i32)>,
    },
    Offset(usize),
    RefOffset(usize),
    Delay,
    Overview,
    ZoomLeft,
    ZoomRight,
    ZoomVertical,
    Level,
    Level2,
    CursorA,
    CursorB,
    CursorC,
    CursorD,
    /// the bus, by its vertical position
    Bus,
    /// a list: by its content or its scroll bar (ui/scroll.rs)
    Scroll(scroll::List, scroll::Grip),
}

struct Pinch {
    active: bool,
    p0: (i32, i32),
    p1: (i32, i32),
    ref_dx: i32,
    ref_dy: i32,
    area: Rect,
    detail: bool,
}

struct TouchState {
    down: bool,
    x0: i32,
    y0: i32,
    x: i32,
    y: i32,
    /// `Ui::now` when the finger went down
    t0: f64,
    /// a press-and-hold already opened its menu; the lift is not a tap
    held: bool,
    drag: Drag,
    start_offset: f32,
    start_delay: f32,
    start_level: f32,
    start_cur: f32,
    /// the waveform window the finger went down in
    win: usize,
}

/// What the power dialog asked for; the main loop carries it out (panel MCU frames, docs/protocols/board.md 4/8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Off,
    Restart,
}

/// Samples of one channel for the measurements in the background: a window of the hardware frame (shared, not
/// copied) or an own copy of the screen record.
enum MeasSrc {
    Frame(std::sync::Arc<crate::board::Frame>, usize, usize, usize),
    Own(Vec<f32>),
}

impl MeasSrc {
    fn samples(&self) -> &[f32] {
        match self {
            MeasSrc::Frame(f, c, i0, i1) => &f.ch[*c][*i0..*i1],
            MeasSrc::Own(v) => v,
        }
    }
}

/// One record's measurements: (channel, samples, dt, the smallest swing that counts as a signal, min/max pairs).
type MeasJob = (u64, Vec<(usize, MeasSrc, f64, f64, bool)>);
type MeasWork<'a> = (usize, &'a [f32], f64, f64, bool);
type MeasDone = (u64, Vec<(usize, Stats, oos_dsp::measure::Analysis)>);

/// The measurements of a record, each channel on its own thread (the four channels of 100 kpts took 27 ms in a row
/// on the unit); min/max pairs in time on the mean of neighbours (#118).
fn measure_channels(work: &[MeasWork]) -> Vec<(usize, Stats, oos_dsp::measure::Analysis)> {
    let one = |&(c, v, dt, min, p): &MeasWork| (c, stats(v, dt), analysis::analyze_samples(v, dt, min, 0.1, 0.9, p));
    if work.len() < 2 {
        return work.iter().map(one).collect();
    }
    std::thread::scope(|s| {
        let hs: Vec<_> = work.iter().map(|w| s.spawn(move || one(w))).collect();
        hs.into_iter().map(|h| h.join().expect("measurement thread")).collect()
    })
}

/// The measurements in the background, on the unit (`Ui::measure_in_background`): a thread on the slower cores
/// measures the newest record while the frame loop goes on, and its results are taken a frame or two later. A record
/// that comes while one is measured waits; a newer one replaces it. Without it (tests, scripts, the host) the
/// measurements are made in the frame they belong to.
struct MeasWorker {
    tx: std::sync::mpsc::Sender<MeasJob>,
    rx: std::sync::mpsc::Receiver<MeasDone>,
    /// a job is out
    busy: bool,
}

impl MeasWorker {
    fn start() -> Option<MeasWorker> {
        let (tx, jobs) = std::sync::mpsc::channel::<MeasJob>();
        let (done, rx) = std::sync::mpsc::channel::<MeasDone>();
        std::thread::Builder::new()
            .name("measure".into())
            .spawn(move || {
                crate::http::to_little_cores();
                while let Ok((n, work)) = jobs.recv() {
                    let w: Vec<MeasWork> =
                        work.iter().map(|(c, v, dt, min, p)| (*c, v.samples(), *dt, *min, *p)).collect();
                    if done.send((n, measure_channels(&w))).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(MeasWorker { tx, rx, busy: false })
    }
}

pub struct Ui {
    /// Add-measurements catalog: tab, selected button, which groups are open, scroll, and the extra sources.
    cat_tab: u8,
    cat_sel: u16,
    cat_open: [u16; crate::measures::CATEGORIES.len()],
    cat_scroll: i32,
    cat_c: WaveId,
    cat_d: WaveId,
    meas_page: usize,
    meas_catalog_page: usize,
    workflow_hold: Option<workflow::LiveSession>,
    session_is_sim: bool,
    replay: workflow::Replay,
    workflow_epoch: u64,
    bus_candidate: bus_candidate::State,
    pub open_wave_request: bool,
    load_slot_ask: bool,
    load_destination: Option<usize>,
    pub m: Model,
    pub src: Source,
    pub ph: Phosphor,
    pub phz: Phosphor,
    area: Rect,
    /// plot rectangle (10 x 8 div) of the current layout
    grid: Rect,
    layout_zoom: bool,
    /// Entry context used by event navigation; closing zoom never rolls back main controls.
    zoom_saved: Option<(usize, f32, f32)>,
    // Display magnification only: original graticule divisions at detail centre.
    zoom_vfactor: f32,
    zoom_vcenter: f32,
    zoom_user_closed: bool,
    /// the waveform windows in the plot (one: the plot itself); `area` is the first while nothing else is drawn
    wins: Vec<Rect>,
    /// the window being drawn
    cur_win: usize,
    /// Full density on the panel: the plot covers the top bar and the cards. Stays off while the external display
    /// shows the waveform, so the panel keeps the controls, unless the expand button asked for it (`full_asked`).
    full: bool,
    /// Top left of the corner button after a drag. None keeps it at the top right of the plot.
    full_at: Option<(i32, i32)>,
    /// Full came from a tap on the expand button: the panel enlarges its plot even while the external display shows
    /// the waveform. Full chosen under Density, a display plugged in, or leaving Full clears it.
    full_asked: bool,
    vmaxz: [u32; NCH],
    rng: Rng,
    trace_bus: std::cell::RefCell<bus::Cache>,
    menu: MenuId,
    menu_open: bool,
    /// menu that opened the current one (Back button)
    parent: Option<MenuId>,
    /// Previous menu locations, so nested Back preserves the complete route.
    menu_history: Vec<(MenuId, usize)>,
    /// the open picker (menu == MenuId::Pick)
    pick: Option<Pick>,
    focus: usize,
    meas_src: WaveId,
    last: [Option<Record>; 1],
    stats: [Stats; NCH],
    /// the last acquired records (newest last) for history playback
    history: std::collections::VecDeque<Record>,
    /// position in `history` while viewing it (None = live)
    hist_pos: Option<usize>,
    /// mark all trigger events in the record (search)
    search_on: bool,
    /// measurements per channel (oos-dsp) of the last record
    ana: [std::sync::Arc<oos_dsp::measure::Analysis>; NCH],
    /// measurements of each ref slot, from the stored samples (no front-end bandwidth)
    ref_ana: [std::sync::Arc<oos_dsp::measure::Analysis>; NREF],
    /// False when a hidden, unrequested reference skipped the capture-time scan.
    ref_ana_valid: [bool; NREF],
    /// the measurements in the background (`measure_in_background`); `meas_want`: the record shown is still to be
    /// measured; `meas_n`: counts what makes a result still out wrong (a clear, a measurement made here at once), a
    /// result sent before is dropped; a new record does not count (in Run one comes nearly every frame, and every
    /// result would be dropped); `meas_new`: new results for the statistics, the trend and the counter
    meas_bg: Option<MeasWorker>,
    meas_want: bool,
    meas_n: u64,
    meas_new: bool,
    /// per channel: the measurement sent (`meas_env_sent`) and the one shown (`meas_env`) ran on min/max pairs
    /// (`framestats::needs_samples` are then not available)
    meas_env_sent: [bool; NCH],
    meas_env: [bool; NCH],
    /// the record a background measurement is wanted of (`meas_want`), when it must not be the newest at the time the
    /// worker is free: with raw deep reads (deepmeas.rs) the newest is then often a pair record, held
    meas_pending: Option<Record>,
    /// what choosing the Bus trigger switched on by itself (`set_trig_type`): the channels and whether the decoder; leaving
    /// Bus switches exactly those off again, what the user had on stays
    bus_auto: Option<([bool; NCH], bool)>,
    /// STOP waiting for the hardware's final frame (docs/fork/stop-exact.md): the request number and until when
    final_req: u64,
    stop_final: Option<f64>,
    toast: Option<(String, f64)>,
    dialog: Option<Vec<String>>,
    /// System info page (sysinfo.rs): its rows while open (shown instead of the dialog lines), last refresh
    sysinfo: Option<[Vec<sysinfo::Row>; 3]>,
    sysinfo_t: f64,
    touch: TouchState,
    /// zone being edited in the Zone menu; a "draw" action arms the next drag in the plot (zone, shape)
    zone_sel: usize,
    zone_arm: Option<(usize, bool)>,
    /// screen path of the zone being drawn
    zone_path: Vec<(i32, i32)>,
    /// Persistent draw-a-box tool; independent of the corner zoom-display toggle.
    zoom_arm: bool,
    /// screen corners of the zoom box being drawn
    zoom_path: Vec<(i32, i32)>,
    /// waveform rectangle and time base the zoom box started in: (rect, s/div, delay)
    zoom_map: Option<(Rect, f32, f32)>,
    /// records dropped by the zone condition
    zone_drops: u64,
    /// the zone unit (hwzone.rs): latest reading, and a number raised to clear its counter
    zone_hw: Option<crate::board::ZoneRead>,
    zone_hw_clear: u64,
    /// fast path frames by where the edge of the set direction was: at the hardware trigger point; further away with
    /// an edge of the other direction at the point (the hardware fired on the wrong edge); further away with no edge at
    /// the point (the point is off); none found. And the offsets of the last kind in samples: sum, largest
    /// (`stat`, trigger.md 8)
    pub fast_edges: [u64; 4],
    pub fast_off: (f64, f64),
    /// frames the fast path takes in one screen (`fast_max`), its time in the last screens (ms, smoothed) and that of
    /// its two phases (placing, drawing; `:SYSTem:STATe?`)
    pub fast_take: f32,
    /// STOP: the software trigger of the record shown at the stop (s from the hardware trigger point), kept for the
    /// records read again (`stop_replay`)
    stop_ti_off: Option<f64>,
    /// STOP: the raw record shown at the stop (the final frame of a deep or live-compressed record), and the min/max
    /// pair record the thread read again for the view when that came as pairs. The pairs are drawn; the raw record
    /// stays `last[0]`, the record measured and read out (`stop_replay`, docs/fork/stop-exact.md)
    stop_raw: Option<Record>,
    stop_pic: Option<Record>,
    pub t_fast: f32,
    pub t_fast_ab: (f32, f32),
    /// of the drawing: adding the collected frames to the maps (ms), frames a call (smoothed)
    pub t_fast_fold: f32,
    pub fast_n: f32,
    /// the fast path's collected differences per channel (`fastdraw::diff_span`), zero between screens
    acc_t: [Vec<u16>; NCH],
    /// segmented recording: records since the acquisition started, and whether it ran in the last frame
    seg_n: usize,
    /// hardware segments: recording number told to the thread, its state, the segment shown (browsing), the trigger
    /// times read back
    seg_run: u64,
    seg_view: Option<crate::wave::SegView>,
    seg_pos: Option<usize>,
    seg_times: Vec<Option<f64>>,
    was_running: bool,
    /// analysers (analysis.rs)
    azr: analysis::State,
    /// self-calibration page (selfcal.rs)
    cal: selfcal::State,
    /// Bode sweep (bode.rs)
    bode: bode::State,
    /// SIM: Play is moving the timebase window through the selected record
    workspace: workspace::Workspace,
    pub desktop_panel: Option<bool>,
    /// the analyser view the last frame had: a change of view analyses what is already there (a stopped record gets no new one)
    analyzed_view: AView,
    sim_play: bool,
    sim_generation: Option<sim::Generation>,
    sim_error: Option<String>,
    /// Offline mode. Off, the live screen stays as it is.
    offline: bool,
    /// Take over, offline waveforms, or the live scope.
    look: reflook::RefLook,
    /// Settings of each added measurement. Missing keys follow the shared measure menu.
    meas_own: std::collections::HashMap<(Meas, WaveId), reflook::MeasOwn>,
    /// Settings dialog open on this index of `m.meas`.
    meas_cfg: Option<u16>,
    /// Reference named on the card that replaces the trigger card, and read by the DVM and the counter.
    offline_ref: usize,
    offline_hold: Option<offline::LiveHold>,
    /// HDMI workspace: set by the main loop on frames it shows on the monitor; the waveform area (full width, top bar
    /// to cards) is copied here during `render`
    pub hdmi_snap_want: bool,
    /// An external display is attached. Only then does "waveform on the external display" keep the panel bars in Full.
    hdmi_present: bool,
    pub hdmi_snap: Vec<u32>,
    /// the same for the GPU: the layer over the traces at that point (the monitor's workspace has no menus)
    pub hdmi_over: Option<Canvas>,
    /// the monitor's size while one is driven (main.rs), for `:SYSTem:STATe?`
    pub hdmi_out: Option<(u32, u32)>,
    /// start-up screen (startup.rs) and the last start-up's result (About)
    pub start_up: Option<startup::StartScreen>,
    start_up_last: Option<oos_hal::bootstatus::View>,
    /// signed updates (update.rs): running oos-update job, watcher notice
    upd: update::UpdateUi,
    wlan: wlan::WlanUi,
    pinch: Pinch,
    mstat: Vec<MStat>,
    /// past values of each measurement row
    trend: Vec<panes::Trend>,
    /// what the second pane keeps between frames
    pn: panes::PaneState,
    log: logger::Logger,
    single_armed: bool,
    stats_on: bool,
    /// measurement results overlay folded to its header
    results_min: bool,
    /// Eye page: the right column slides from `eye_from` to `eye_to` (0 hidden, 1 shown), starting at `eye_t`.
    eye_from: f32,
    eye_to: f32,
    eye_t: f64,
    /// Which of the four eye-page plots fills the chart area.
    eye_big: Option<u8>,
    /// Clear pressed: the screen stays empty (also in STOP) until the next record
    cleared: bool,
    refs: [Option<RefWave>; NREF],
    /// counter: edges so far, frequency statistics
    cnt_total: u64,
    cnt_stat: MStat,
    /// the hardware counter (tools.rs): latest reading, and a number raised by Clear counter
    cnt_hw: Option<crate::board::CounterRead>,
    cnt_clear: u64,
    /// the FPGA's DVM block (tools.rs, `dvm.hw = on`): latest reading
    dvm_hw: Option<crate::board::DvmRead>,
    /// `:SYSTem:DEBug:DVM:LANE`: a lane forced for the DVM block (one channel only)
    dvm_lane: Option<u32>,
    /// source B of delay and phase
    meas_src_b: WaveId,
    /// the reference the Slot rows work on
    ref_sel: usize,
    ref_src: usize,
    /// files the Import slot picker is showing
    ref_files: Vec<std::path::PathBuf>,
    xy: bool,
    knob_t: [f64; 16],
    pub now: f64,
    /// the trigger level last seen and when it changed: the level line shows for `LEVEL_LINE_S` after a change (knob,
    /// touch, remote), so the level can be followed while it moves
    level_seen: (f32, f32),
    level_changed: f64,
    pub frames: u64,
    /// records taken in, for the state line
    pub records: u64,
    /// auto set-up under way on the hardware (autoset.rs)
    auto: Option<autoset::AutoRun>,
    operation_serial: u64,
    scpi_completed: std::collections::VecDeque<OperationToken>,
    single_operation: u64,
    /// when the bar of views after an auto set-up came up (ui/autoset.rs)
    auto_bar: Option<f64>,
    /// request number of the last `:DEBug:SLOTs?`
    slot_req: u64,
    pub fps: f32,
    /// acquisitions per second (smoothed)
    pub wfm_rate: f32,
    /// every waveform acquired since the start (Clear resets it)
    pub wfm_total: u64,
    /// IPv4 address shown in the top bar ("" = no network)
    pub net: String,
    /// its interface (eth0, wlan0)
    pub net_if: String,
    /// all interfaces with an address: name, address
    pub net_ifs: Vec<(String, String)>,
    /// no DHCP answer at the start: the fixed address of rcS
    pub net_fixed: bool,
    /// a WLAN stick is plugged in (its interface exists)
    wlan_stick: bool,
    /// a mouse and a keyboard (USB or Bluetooth): the pointer and the modifier keys (ui/pointer.rs)
    pub mouse: pointer::Mouse,
    /// System > Network > Bluetooth (ui/bt.rs), and the stick and its inputs for the top bar
    bt: bt::BtUi,
    /// System > Overclock: limits.conf and the ADC clock of the next start (overclock.rs)
    pub limits_path: String,
    clock_next: f64,
    /// a cable in the LAN port with a link
    pub eth_link: bool,
    /// one more autonegotiation for a link at 10 Mbit/s; the main loop drives it (ethlink.rs)
    pub eth: crate::ethlink::Retry,
    /// settings of the scope itself (sysconf.rs); written to `sys_path` (none in tests: nothing written)
    pub sys: crate::sysconf::SysConf,
    pub sys_path: Option<std::path::PathBuf>,
    /// NTP worker and schedule (ui/time.rs)
    ntp: time::NtpUi,
    ssh: ssh::SshUi,
    /// System > Network > Remote services > Allowed (ui/remote.rs)
    remote: remote::RemoteUi,
    /// the system clock was set in this boot (NTP or by hand): file names carry the date
    pub clock_known: bool,
    pub t_step: f32,
    /// parts of `t_step` (ms, smoothed): taking records from the source, drawing them into the phosphor maps
    pub t_acq: f32,
    pub t_draw: f32,
    pub t_render: f32,
    pub t_present: f32,
    /// System > Graphics = GPU: the helper's last panel frame (ms): upload, draw, read back; None on the CPU
    pub t_gpu: Option<[f32; 6]>,
    /// the helper's time of the last panel frame and, while the GPU draws for a monitor, of the last monitor frame (ms)
    pub t_gpu_screens: Option<(f32, Option<f32>)>,
    /// parts of `step` (ms, smoothed): measurements, analysers and decoders, the second pane, the brightest pixels
    pub t_parts: [f32; 4],
    pub info: String,
    pub dirty_map: bool,
    vmax: [u32; NCH],
    /// brightest pixel of the merged maps (colour grade), main / zoom view
    vmax_cg: u32,
    vmaxz_cg: u32,
    /// minimum persistence: frames in the picture on screen, and the brightest pixel per frame behind `vmax`, `vmaxz`,
    /// `vmax_cg`, `vmaxz_cg` (`picture_ref`)
    pic_frames: u32,
    per_frame: [u32; NCH],
    per_framez: [u32; NCH],
    per_frame_cg: [u32; 2],
    /// storage directory (setups, CSV, PNG), created when missing
    pub store_dir: std::path::PathBuf,
    /// Auto / USB stick / desktop for waveform, mask and screenshot files
    store_dest: crate::store::StoreDest,
    /// Math menu category (kept when the operation is Off)
    math_fam: MathFamily,
    math_state: std::cell::RefCell<mathwave::State>,
    capture_views: std::cell::RefCell<capture_memory::State>,
    /// the screen the layout is made for: the panel, or another size in host test runs (`--size`)
    pub scr: Screen,
    /// `render_view` is drawing the second picture: the layout must not touch the phosphor maps
    view_pass: bool,
    /// a USB stick is mounted
    usb: bool,
    /// a channel key held on the front panel: channel, since when, and whether it was long enough for AC/DC already
    /// (`panel_key`)
    ch_hold: Option<(usize, f64, bool)>,
    /// a finger on the card of a channel that is on, since when (a long touch: AC/DC, as the key)
    card_hold: Option<(usize, f64)>,
    /// LA probe detect: None = not known (no hardware, not read yet)
    la_probe: Option<bool>,
    usb_check: f64,
    /// setup slot n = file setup<n>.txt
    store_slot: u32,
    /// mask test: reference waveform (volts, screen time), the mask built from it, counts
    mask_ref: Option<Vec<f32>>,
    mask: Option<oos_dsp::mask::Mask>,
    mask_pass: u64,
    mask_fail: u64,
    /// the mask in the columns of the hardware table (hwmask.rs), the latest counters of the mask unit, and a number
    /// raised to clear them
    mask_cols: Option<(Vec<f32>, Vec<f32>)>,
    mask_hw: Option<crate::board::MaskRead>,
    mask_hw_clear: u64,
    /// the acquisition thread dropped the mask/zone unit for the current settings after it stalled the acquisition:
    /// why (hwmask.rs); the software test and filter take over
    unit_blocked: Option<String>,
    pub shot_request: Option<String>,
    pub quit: bool,
    /// commands for the front panel, sent by the main loop (power handshake)
    pub panel_tx: Vec<crate::board::PanelCmd>,
    /// power dialog open (power key)
    pub power_dlg: bool,
    pub power_action: Option<PowerAction>,
    /// time of the last record (UI clock), for the WAIT status
    last_rec_t: f64,
    /// smoothed time between two records (deep hardware records arrive about once per second)
    rec_iv: f64,
    /// hex entry of the keypad picker (protocol trigger value)
    keypad: String,
    /// logic analyser lanes: pixel columns of the last LA record drawn (la.rs)
    la_cols: std::cell::RefCell<la::Cache>,
    /// decoded LA lines of the record shown: (key of record and settings, packets with times in s)
    dec_cache: std::cell::RefCell<Vec<(u64, Vec<oos_dsp::decode::Packet>)>>,
    /// table window over the plot, first row shown
    table: Option<tables::TableKind>,
    table_top: usize,
}

/// One line of the measurement strip when it is docked on the left or the right.
struct StripLine {
    pill: Option<(String, Rgb)>,
    name: String,
    value: String,
    col: Rgb,
    /// the name lines up with the names under a channel pill
    align: bool,
}

impl Ui {
    /// The fonts of the text size.
    pub fn fonts(&self) -> &'static Fonts {
        Fonts::get(self.m.text_size.index())
    }

    /// The layout sizes of the text size.
    pub fn lay(&self) -> &'static Lay {
        &LAYS[self.m.text_size.index()]
    }

    /// Called after remote (SCPI) changes: same effect as a change on the screen.
    pub fn mark_changed(&mut self) {
        self.settings_changed();
    }

    pub fn toast(&mut self, s: &str) {
        self.toast = Some((s.to_string(), self.now + 2.5));
    }

    /// Clear (CLEAR key, Display > Clear, Quick key, SCPI :CLEar): empties the screen like a scope does. The
    /// waveforms, persistence / colour-grade maps, averaging, measurements and their statistics, mask counts and
    /// search marks start over; in STOP the screen stays empty until the next acquisition (RUN draws again at once).
    fn clear_display(&mut self) {
        self.wfm_total = 0;
        self.bus_views_clear();
        self.ph.clear();
        self.phz.clear();
        (self.vmax, self.vmaxz, self.vmax_cg, self.vmaxz_cg) = ([1; NCH], [1; NCH], 1, 1);
        // without a last record there is nothing to redraw in STOP, average with, compute math / decode / search on
        self.last[0] = None;
        self.hist_pos = None;
        self.cleared = true;
        self.stats = [Stats::default(); NCH];
        // a measurement still out, or a record waiting for the worker, belongs to what was cleared away
        (self.meas_n, self.meas_want, self.meas_pending) = (self.meas_n + 1, false, None);
        self.ana = Default::default();
        self.clear_measurement_statistics();
        self.mask_counts_reset();
        (self.zone_drops, self.zone_hw_clear) = (0, self.zone_hw_clear + 1);
        self.hw_avg_restart();
        self.toast("Display cleared");
    }

    /// The hardware average starts over (`Model::hw_avg_clear`); nothing changes for the other sources.
    fn hw_avg_restart(&mut self) {
        if self.src.is_hw() {
            self.m.hw_avg_clear += 1;
        }
    }

    /// QUICK key and the Quick button of the top bar: the action chosen in System > Quick key.
    fn quick(&mut self) {
        match self.m.quick {
            Quick::Screenshot => self.shot_request = Some(String::new()),
            Quick::SavePng => self.activate(Item::StorePng),
            Quick::SaveSetup => self.activate(Item::StoreSaveSetup),
            Quick::SaveCsv => self.activate(Item::StoreCsv),
            Quick::Autoset => self.key(key::AUTO),
            Quick::Single => self.key(key::SINGLE),
            Quick::RunStop => self.key(key::RUNSTOP),
            Quick::Clear => self.clear_display(),
            Quick::NextView => {
                if self.m.views < 2 {
                    return self.toast("Set Display > Views first");
                }
                self.view_step(1)
            }
        }
    }

    fn level_50(&mut self) {
        if self.m.trig_ref.is_some() {
            self.fit_reference_trigger_levels();
            self.settings_changed();
            return;
        }
        let s = self.stats[self.m.trig_src];
        if s.valid {
            self.m.trig_level = crate::wave::mid_level(&s);
            self.settings_changed();
        }
    }
}

#[derive(Clone)]
enum Ctl {
    /// segmented buttons: options, selected
    Seg(Vec<String>, Option<usize>),
    /// - value +
    Value,
    /// - value +, the value opens the list of all options (MenuId::Pick)
    Pick,
    Button,
    /// plain text: a state, nothing to press
    Text,
}

struct Row {
    it: Item,
    label: Rect,
    ctrl: Rect,
    ctl: Ctl,
}

/// The bottom row.
struct Cards {
    ch: [Rect; NCH],
    la: Option<Rect>,
    gen: Option<Rect>,
    time: Rect,
    trig: Rect,
}

/// Width of the LA and generator cards, and the width a channel card keeps before they stay narrow.
const AUX_NARROW: i32 = 56;
const AUX_WIDE: i32 = 104;
const CH_MIN: i32 = 100;
/// Widest TIME and trigger cards: more room only spreads their text (the panel never gets that far, at most 234 / 324).
const TIME_CARD_MAX: i32 = 240;
const TRIG_CARD_MAX: i32 = 330;
/// Colour of the generator card.
const GEN_COLOR: Rgb = rgb(0xb3, 0x94, 0xf5);

/// Menu dialog layout.
struct Dlg {
    r: Rect,
    close: Rect,
    back: Option<Rect>,
    rows: Vec<Row>,
    /// group labels (start menu)
    heads: Vec<(Rect, &'static str)>,
}

/// Temperature map of the colour-graded display, 256 steps: blue (rare) -> cyan -> green -> yellow -> red (frequent).
fn heat_palette() -> &'static [Rgb; 256] {
    static PAL: std::sync::OnceLock<[Rgb; 256]> = std::sync::OnceLock::new();
    PAL.get_or_init(|| {
        let mut p = [0; 256];
        for (i, e) in p.iter_mut().enumerate() {
            let t = i as f32 / 255.0 * 4.0;
            let k = (t as usize).min(3);
            let f = t - k as f32;
            let (a, b) = (HEAT_STOPS[k], HEAT_STOPS[k + 1]);
            let ch = |x: f32, y: f32| (x + (y - x) * f).round() as u32;
            *e = rgb(ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2));
        }
        p
    })
}

/// Where a reference badge goes: the wanted height, or the nearest free slot (36 px apart) between `lo` and `hi`.
fn badge_spot(taken: &[i32], y: i32, lo: i32, hi: i32) -> i32 {
    (0..8)
        .flat_map(|k| [y + 36 * k, y - 36 * k])
        .map(|c| c.clamp(lo, hi))
        .find(|c| taken.iter().all(|t| (t - c).abs() >= 36))
        .unwrap_or(y.clamp(lo, hi))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// The first of `fonts` in which `s` is at most `room` wide, else the last.
impl Ui {
    /// Font of a menu row that is only text (a long status line): the menu font, else smaller ones down to the small
    /// font of the normal text size.
    fn text_row_font(&self, room: i32, s: &str) -> &'static Font {
        let (f, n) = (self.fonts(), Fonts::get(0));
        fit_font(&[&f.m, &f.s, &n.m, &n.s], room, s)
    }
}

fn fit_font(fonts: &[&'static Font], room: i32, s: &str) -> &'static Font {
    fonts.iter().copied().find(|f| f.width(s) <= room).unwrap_or(fonts[fonts.len() - 1])
}

fn backed_text(cv: &mut Canvas, f: &Font, x: i32, y: i32, s: &str, col: Rgb) -> i32 {
    cv.blend(Rect::new(x - 3, y - f.size + 2, f.width(s) + 6, f.size + 2), BG, 180);
    cv.text(f, x, y, s, col)
}

/// Button: 2 px border, filled.
fn btn(cv: &mut Canvas, r: Rect, fill: Rgb, border: Rgb) {
    cv.framed(r, 5, 2, border, 4, fill);
}

fn chevron_down(cv: &mut Canvas, x: i32, y: i32, col: Rgb) {
    for d in 0..2 {
        cv.line(x, y + d, x + 4, y + 4 + d, col);
        cv.line(x + 4, y + 4 + d, x + 8, y + d, col);
    }
}

/// "180", "1.10k", "12.5k".
fn fmt_count(v: f64) -> String {
    if v >= 1e3 {
        let s = si_digits(v, "", 3);
        s.replace(' ', "")
    } else {
        format!("{:.0}", v)
    }
}

/// Text of one decoded field.
fn packet_text(m: &Model, p: &oos_dsp::decode::Packet) -> String {
    use oos_dsp::decode::Kind;
    if let Some(t) = m.dec_proto.module().and_then(|x| x.text(p, &m.dec_opts())) {
        return t;
    }
    let hex = |v: u32| if m.dec_bits > 8 { format!("{:04X}", v) } else { format!("{:02X}", v) };
    match p.kind {
        Kind::Addr { read } => {
            format!("{} {}{}", hex(p.value), if read { "R" } else { "W" }, if p.ok { "" } else { " N" })
        }
        Kind::Id { ext, rtr } => format!(
            "ID {}{}{}",
            if ext {
                format!("{:08X}", p.value)
            } else if m.dec_proto == Proto::LIN {
                format!("{:02X}", p.value)
            } else {
                format!("{:03X}", p.value)
            },
            if rtr { " RTR" } else { "" },
            if p.ok { "" } else { " !P" }
        ),
        Kind::Sample { right } => {
            format!("{} {:0w$X}", if right { "R" } else { "L" }, p.value, w = (m.dec_bits as usize).div_ceil(4))
        }
        Kind::Word { cmd } => {
            format!("{} {:04X}{}", if cmd { "C/S" } else { "D" }, p.value, if p.ok { "" } else { " !P" })
        }
        Kind::Crc => format!(
            "{} {:X}{}",
            if m.dec_proto == Proto::LIN { "CS" } else { "CRC" },
            p.value,
            if p.ok { "" } else { " !" }
        ),
        Kind::Field(l) if matches!(l, "MAC" | "SRC") && p.value2.is_some() => {
            format!(
                "{} {}{}",
                if l == "SRC" { "SRC" } else { "MAC" },
                oos_dsp::linecode::mac_text(p.value, p.value2.unwrap_or(0)),
                if p.ok { "" } else { " !" }
            )
        }
        Kind::Field(l) if matches!(l, "IP" | "DIP") => format!(
            "{} {}{}",
            if l == "DIP" { "DIP" } else { "IP" },
            oos_dsp::linecode::ip_text(p.value),
            if p.ok { "" } else { " !" }
        ),
        Kind::Field(l) => match p.value2 {
            Some(v2) if matches!(l, "DST" | "SRC" | "SER" | "PID") => {
                format!("{} {:04X}{:08X}{}", l, v2, p.value, if p.ok { "" } else { " !" })
            }
            _ => format!("{} {:X}{}", l, p.value, if p.ok { "" } else { " !" }),
        },
        Kind::Ack => format!("ACK {}", p.value),
        _ if m.dec_proto == Proto::UART && m.dec_ascii && p.value == 0x20 => "SP".to_string(),
        _ if m.dec_proto == Proto::UART && m.dec_ascii && (0x21..0x7f).contains(&p.value) => {
            format!("{}", p.value as u8 as char)
        }
        _ => match p.value2 {
            Some(v2) => format!("{}/{}", hex(p.value), hex(v2)),
            None if m.dec_proto == Proto::I2C && !p.ok => format!("{} N", hex(p.value)),
            None => hex(p.value),
        },
    }
}

/// The short tag of an event (start, stop, acknowledge ...), None for a field with a value.
fn packet_tag(m: &Model, p: &oos_dsp::decode::Packet) -> Option<&'static str> {
    use oos_dsp::decode::Kind;
    // a field a protocol module names itself is no tag (the ACK of SWD has a value)
    if m.dec_proto.module().is_some_and(|x| x.text(p, &m.dec_opts()).is_some()) {
        return None;
    }
    let event = matches!(p.kind, Kind::Field(l) if l.starts_with('!'));
    if !(event || matches!(p.kind, Kind::Start | Kind::RepStart | Kind::Stop | Kind::Break | Kind::Ack)) {
        return None;
    }
    Some(match p.kind {
        Kind::Field(l) => &l[1..],
        Kind::Start if matches!(m.dec_proto, Proto::CAN) => "SOF",
        Kind::Start => "S",
        Kind::RepStart => "Sr",
        Kind::Break => "BRK",
        Kind::Ack if p.ok => "ACK",
        Kind::Ack => "NAK",
        _ => "P",
    })
}

/// Axis label with the SI prefix of the axis `span` and as many decimals as `step` and `base` (offset / delay)
/// need: "-1.0 µs", "500 mV", "3.25 V".
fn axis_label(v: f32, step: f32, base: f32, span: f32, unit: &str) -> String {
    // up to giga: an axis does not go to tera
    let (mult, p) = si_prefix(span.abs().max(step.abs()) as f64, 1);
    let st = (step as f64 / mult).abs();
    // decimals that show `x` (a step or the base): a whole number of the last digit, not 0 (a step of 0.001 ms is not
    // "0"), at most `max`: 6 for the step (a delay of seconds at a time base of microseconds), 3 for the base (an
    // offset dragged to 1.454545 V shows as 1.455)
    let needed = |x: f64, max: i32| {
        if x < 1e-12 {
            return 0;
        }
        let mut dec = 0;
        while dec < max {
            let k = x * 10f64.powi(dec);
            if k.round() >= 1.0 && (k.round() - k).abs() < 1e-3 {
                break;
            }
            dec += 1;
        }
        dec
    };
    let dec = needed(st, 6).max(needed((base as f64 / mult).abs(), 3));
    let x = v as f64 / mult;
    let x = if x.abs() < st * 1e-3 { 0.0 } else { x };
    format!("{:.*} {}{}", dec as usize, x, p, unit)
}

/// Small chevron pointing left (dir -1) or right (dir 1), centred at (x, y).
fn chevron(cv: &mut Canvas, x: i32, y: i32, dir: i32, col: Rgb) {
    for d in 0..2 {
        cv.line(x - 2 * dir + d, y - 5, x + 3 * dir + d, y, col);
        cv.line(x + 3 * dir + d, y, x - 2 * dir + d, y + 5, col);
    }
}

#[cfg(test)]
mod acquisition_tests;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod tests;

/// Menu text of a hardware segment interval.
fn seg_ivl_name(x: f32) -> String {
    if x <= 0.0 {
        "None".into()
    } else {
        si(x as f64, "s")
    }
}

#[cfg(test)]
mod aux_out_tests {
    use super::*;

    #[test]
    fn aux_out_in_the_system_menu_with_the_offered_choices() {
        let mut ui = crate::ui::testkit::demo_ui();
        assert!(ui.items(MenuId::Utility).contains(&Item::AuxOut));
        let want: &[&str] = if board().mask_hw() || board().zone_hw() {
            &["Trigger out", "Off", "Pass/fail"]
        } else {
            &["Trigger out", "Off"]
        };
        assert_eq!(ui.options(Item::AuxOut), want);
        assert_eq!(ui.value(Item::AuxOut), "Trigger out");
        ui.select_option(Item::AuxOut, "Off");
        assert_eq!((ui.m.aux_out, ui.value(Item::AuxOut).as_str()), (AuxOut::Off, "Off"));
        ui.select_option(Item::AuxOut, "Trigger out");
        assert_eq!(ui.m.aux_out, AuxOut::TrigOut);
        // pass/fail where the mask test runs in hardware
        assert_eq!(AuxOut::choices(true), AUX_OUTS);
        assert_eq!(AuxOut::choices(false), [AuxOut::TrigOut, AuxOut::Off]);
    }
}

#[cfg(test)]
mod zoom_tests;
