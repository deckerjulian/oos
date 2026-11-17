// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Layout, rendering, menus and input handling of the OOS scope screen (1024 x 600).

use crate::font::{Font, Fonts};
use crate::math::*;
use crate::gfx::*;
use crate::model::*;
use crate::wave::*;
use crate::board::{board, Board};

mod analysers;
mod analysis;
mod scroll;
mod measure_pairs;
mod bus;
mod busview;
mod magnetic;
mod motor;
mod dpt;
mod eyepage;
mod panes;
mod autoset;
mod selfcal;
pub mod bode;
mod sim;
mod offline;
mod reflook;
mod measrow;
mod workflow;
mod deskew;
pub mod startup;
mod afg;
mod sysinfo;
mod tables;
mod logger;
mod measlabels;
mod overclock;
mod tools;
mod feedback;
mod hwmask;
mod hwzone;
mod sertrig;
mod measurement_nav;
mod meascatalog;
pub(crate) mod facts;
mod update;
mod viewmore;
mod wlan;
mod windows;
mod workspace;
mod studio;
mod dock;
mod studio_views;
mod sheet;
mod addrail;
mod addmenu;
mod zoomtab;
mod studioctx;
mod scpiext;
mod scpiext_a;
mod scpiext_b;
mod scpiext_c;
#[cfg(test)]
#[path = "scpi_coverage_tests.rs"]
mod scpi_coverage_tests;
#[cfg(test)]
mod sheet_tests;
mod walk_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod fix_tests;
#[cfg(test)]
mod batch1_tests;
#[cfg(test)]
mod batch2_tests;
#[cfg(test)]
mod batch3a_tests;
#[cfg(test)]
mod batch4_tests;
#[cfg(test)]
mod batch5_tests;
#[cfg(test)]
mod batch6_tests;
#[cfg(test)]
mod batch7_tests;
#[cfg(test)]
mod batch7b_tests;
#[cfg(test)]
mod batch8_tests;
#[cfg(test)]
mod batch9_tests;
#[cfg(test)]
mod batch10_tests;
#[cfg(test)]
mod batch11b_tests;
#[cfg(test)]
mod batch11c_tests;
#[cfg(test)]
mod batch11d_tests;
#[cfg(test)]
mod batch11_tests;
mod hotspots;
mod measdrag;
mod axis;
mod measview;
mod caticons;
mod fullchrome;
mod addtiles;
mod menugrid;
mod numpad;
mod ranges;
mod trigslider;
mod sharedzoom;
mod classicbadges;
mod measpanel;
mod measscroll;
#[cfg(test)]
mod batch21_tests;
#[cfg(test)]
mod batch22_tests;
#[cfg(test)]
mod batch23_tests;
#[cfg(test)]
mod batch24_tests;
#[cfg(test)]
mod batch25_tests;
#[cfg(test)]
mod batch26_tests;
#[cfg(test)]
mod batch27_tests;
#[cfg(test)]
mod batch29_tests;
#[cfg(test)]
mod measpanel_redesign_tests;
#[cfg(test)]
mod batch30_tests;
#[cfg(test)]
mod batch31_tests;
#[cfg(test)]
mod batch33_tests;
#[cfg(test)]
mod dockwalk_tests;
#[cfg(test)]
mod batch34_tests;
#[cfg(test)]
mod scpimap;
#[cfg(test)]
mod uiaudit_tests;
#[cfg(test)]
mod batch12_tests;
#[cfg(test)]
mod batch13_tests;
#[cfg(test)]
mod batch14_tests;
#[cfg(test)]
mod batch16_tests;
#[cfg(test)]
mod batch17_tests;
#[cfg(test)]
mod batch18_tests;
#[cfg(test)]
mod batch19_tests;
mod trigger_controls;
mod scpi_operations;
pub(crate) use scpi_operations::{Operations, OperationState, OperationToken};
mod reference_search;
mod reference_decode;
mod bus_candidate;
mod mathwave;
mod capture_memory;
mod time;
mod remote;
mod ssh;
mod backlight;
mod graphics;
pub mod views;
mod monitor;
mod features;
#[cfg(test)]
mod interplay_tests;
#[cfg(test)]
mod session_tests;
pub use views::VIEWS_MAX;
mod script;
#[cfg(test)]
mod lint;
#[cfg(test)]
mod backend_followup_tests;
mod bwtest;
pub mod la;

// The frame (top bar, waveform area, bottom bar with the cards) comes from the screen size: `Ui::frame`,
// `crate::layout`. Top bar: run state, menu buttons, network (buttons drawn 30 px high, tapped over the full bar
// height). Plot: the whole width and everything between the bars; the channel markers (left edge), the trigger level
// marker (right edge), the trigger position marker (top edge), the axis labels, the status line and the measurement
// results are drawn inside it.
pub use crate::layout::{Frame, Screen, METRICS, PANEL_H, PANEL_W};
/// Width of the marker strips at the left and right edge of the plot (the labels keep clear of them).
const MARK_W: i32 = 26;
/// Width of the volts axis of the histogram view.
const HIST_AXIS_W: i32 = 46;
/// Height of the badge of a reference trace (name row, scale and rate row).
const REF_BADGE_H: i32 = 40;
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
#[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
const PLOT_X: i32 = 0;
#[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
const PLOT_W: i32 = W;
#[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
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
    Lay { strip_row: 18, dlg_label: 96, dlg_col: 400, grid_col: 150, results_w: 196, card: (18, 42, 58), res_pitch: (22, 42) },
    Lay { strip_row: 21, dlg_label: 104, dlg_col: 420, grid_col: 168, results_w: 224, card: (19, 43, 59), res_pitch: (24, 46) },
    Lay { strip_row: 24, dlg_label: 116, dlg_col: 440, grid_col: 190, results_w: 250, card: (20, 43, 59), res_pitch: (26, 50) },
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
const HEAT_STOPS: [(f32, f32, f32); 5] = [(40.0, 70.0, 255.0), (0.0, 220.0, 255.0), (40.0, 240.0, 70.0), (255.0, 235.0, 30.0), (255.0, 40.0, 30.0)];
pub const AVERAGES: [u32; 10] = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];

/// Front-panel key codes (data byte of the MCU frame, see docs/protocols/frontpanel.md).
#[allow(dead_code)] // the full code table, not every code is matched by name
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
const REF_COLORS: [(&str, Rgb); 5] = [("White", rgb(0xd8, 0xd8, 0xd8)), ("Red", rgb(0xf0, 0x60, 0x60)), ("Green", rgb(0x60, 0xe0, 0x90)), ("Blue", rgb(0x70, 0xa8, 0xf8)), ("Violet", rgb(0xc8, 0x88, 0xf0))];
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
        tools::PairKind::SetupAndHold | tools::PairKind::SetupAndHoldBoth => "",
        _ => "s",
    }
}

pub(crate) use analysers::{scpi_views, stored_views};

/// Clock recovery of the eye for the measurement modules: samples per UI and the recovered clock.
pub fn eye_infer(v: &[f32], dt: f64, rate: Option<f64>) -> Result<(f64, oos_dsp::eye::Clock), String> {
    eyepage::automatic::infer(v, dt, rate, false, 0.0).map(|(s, clk)| (s.samples_per_ui, clk))
}
pub const PANES_N: usize = tables::PANES.len();
pub use eyepage::EYE_ITEMS;
pub use panes::{PANE_HARMS, TREND_SEL_N};
pub const LOG_STEPS_N: usize = logger::LOG_STEPS.len();

/// Keys of the frequency keypad: the unit ends the entry.
const FREQ_KEYS: [&str; 18] = ["7", "8", "9", "mHz", "4", "5", "6", "Hz", "1", "2", "3", "kHz", "0", ".", "Del", "MHz", "Clear", "Cancel"];
/// Keys of the hex keypad picker.
/// Largest protocol trigger value: 40 bits (five I2C data bytes).
pub const BUS_VAL_MAX: u64 = 0xff_ffff_ffff;

const KEYPAD: [&str; 19] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "Del", "Clear", "OK"];

/// The one long-press time: a finger held this long opens the hold action of what it is on (a channel's quick actions,
/// the Measure short menu, the plot's actions); a channel key on the front panel held as long switches AC/DC
/// (`Ui::panel_key`). A ring fills under the finger meanwhile (`Ui::press_ring`).
pub const LONG_PRESS: f64 = 0.5;

pub const KEY_NAMES: [(&str, u8); 30] = [
    ("navfwd", 0x06), ("navplay", 0x07), ("navback", 0x08),
    ("ref", 0x01), ("la", 0x02), ("gi", 0x03), ("hmenu", 0x04), ("navigate", 0x05), ("slope", 0x09), ("force", 0x0a), ("clear", 0x0b),
    ("runstop", 0x0c), ("auto", 0x0d), ("single", 0x0e), ("measure", 0x0f), ("cursor", 0x10), ("default", 0x11), ("analyse", 0x12),
    ("touchlock", 0x13), ("quick", 0x14), ("ch1", 0x15), ("ch2", 0x16), ("ch3", 0x17), ("ch4", 0x18), ("math", 0x19), ("tmenu", 0x1a),
    ("power", 0x1b), ("hpos", 0x8c), ("vpos", 0x9c), ("tlevel", 0xec),
];
/// Knob names -> CW code (CCW = CW + 1, push = CW + 2).
pub const KNOB_NAMES: [(&str, u8); 7] = [("hpos", 0x8a), ("vpos", 0x9a), ("multi2", 0xaa), ("multi1", 0xba), ("hscale", 0xca), ("vscale", 0xda), ("tlevel", 0xea)];

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
    /// statistics: on or off, population, columns, reset
    MeasStat,
    /// the settings card of one measurement row (ui/measrow.rs)
    MeasRow,
    /// shared reference levels (the Studio sheet has them on a tab of Measure)
    MeasLevels,
    /// separate fall levels
    MeasFall,
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
    /// Studio: the panels that can be added next to the plot (ui/sheet.rs)
    AddPanel,
    /// Full legacy measurement and pair catalog, including Selected.
    MeasLegacy,
    /// Studio: the settings of the Edge search added from the Add panel (ui/addrail.rs)
    Search,
    MeasSelected,
}

/// Buttons of the top bar.
#[derive(Clone, Copy, PartialEq, Debug)]
enum TopBtn {
    Menu(MenuId),
    System,
    Load,
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
    /// Adds one acquisition's value. With the current population only the latest acquisition is kept.
    fn push(&mut self, v: f64, current: bool) {
        if current { *self = MStat::default(); }
        if self.n == 0 { self.min = v; self.max = v; }
        self.n += 1;
        self.sum += v;
        self.sumsq += v * v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }

    fn add(&mut self, v: f64) {
        self.push(v, false);
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Item {
    DecSel,
    DecTable,
    DecExport,
    SearchTable,
    SearchSave,
    StoreBin,
    PaneSel,
    FullScreen,
    StripPos,
    DecOn,
    DecSrc,
    DecBaud,
    DecCanRate,
    DecCanH,
    DecUsbSpeed,
    /// option row `k` of a protocol module
    DecOpt(usize),
    DecSrc4,
    DecDiff,
    TrigBusWhat,
    TrigBusVal,
    /// protocol trigger settings beyond condition and value (`SerialTrig`)
    SerRel,
    SerMax,
    SerMask,
    UartPol,
    UartStop,
    UartErr,
    SpiCs,
    SpiTimeout,
    I2cDir,
    I2cAddrBits,
    I2cBytes,
    DecThr,
    DecFmt,
    DecProto,
    DecSrc2,
    DecCs,
    DecMode,
    DecBits,
    DecParity,
    BusOn,
    BusLabel,
    BusPos,
    BusClk,
    BusClkThr,
    BusData,
    BusDataThr,
    BusRw,
    Zoom,
    AutoRoll,
    ZScale,
    ZDelay,
    MeasStats,
    /// statistics population and the columns shown
    MeasPop,
    MeasShowSd,
    MeasShowMin,
    MeasShowMax,
    MeasStatReset,
    MeasLabels,
    /// where the measurement strip sits
    StripDock,
    XyMode,
    RefSrc,
    RefSave,
    /// Studio: the same, into the next free slot (offered when the selected slot is full)
    RefSaveNext,
    RefShow(usize),
    RefClear,
    MathOpI,
    MathFam,
    MathA,
    MathB,
    MathScale,
    MathOffset,
    FftWin,
    MathFc,
    MathAuto,
    MathSel,
    /// Studio: turns the selected math channel off (the first row of its menu)
    MathOff,
    MathBw,
    Open(MenuId),
    ChOn(usize),
    ChCoupling(usize),
    ChBw(usize),
    ChProbe(usize),
    ChInvert(usize),
    ChScale(usize),
    ChOffset(usize),
    HScale,
    HDelay,
    HZero,
    AutoDeskew,
    ClearDeskew,
    TrigSrc,
    TrigSlope,
    TrigMode,
    TrigLevel,
    Trig50,
    TrigType,
    SearchOn,
    TrigLevel2,
    TrigSrc2,
    TrigSlope2,
    TrigPol,
    TrigWhen,
    TrigT1,
    TrigT2,
    VidStd,
    VidSync,
    VidLine,
    Pat(usize),
    PatLevel,
    WinModeI,
    NthIdle,
    NthN,
    Setup,
    Hold,
    AcqMode,
    AcqAvg,
    AcqDepth,
    AcqSeg,
    /// software matching of the ADC cores' offsets (`board::dho::cores`)
    AcqCores,
    /// Acquire > Draw path: Standard (fast path) or Batch (PR #7, `Model::draw_batch`)
    AcqDrawPath,
    /// a note while the sample rate is below the ADC's: what lies above half of it folds down, nothing filters it
    AcqAlias,
    /// segments recorded by the FPGA (`Model::seg_hw`)
    AcqSegHw,
    /// Average mode: the FPGA averages (`Model::avg_hw`)
    AcqAvgHw,
    AcqSegIvl,
    /// the trigger times of the hardware segments
    AcqSegTable,
    MeasSrc,
    /// copies every measurement on Source A to the picked source
    MeasCopyTo,
    MeasAdd(Meas),
    /// shortcut on the press-and-hold menu: the next usual measurement
    AddMeas,
    /// settings of one added measurement, index into `m.meas`
    MeasCfg(u16),
    /// a field of the measurement settings card
    RowF(measrow::RF),
    MeasPair(usize),
    MeasSrcB,
    DvmOn,
    DvmSrc,
    DvmMode,
    CntOn,
    CntSrc,
    CntMode,
    CntDigits,
    CntClear,
    TrigHoldoff,
    ChLabel(usize),
    MathExprI,
    MathExprEdit,
    LogOn,
    LogIvl,
    LogInfo,
    SpecLimitSet,
    SpecLimitClear,
    SpecMargin,
    ChFine,
    ChUnit(usize),
    ChBias(usize),
    ChBiasZero(usize),
    ChSkew(usize),
    ChExpand,
    HFine,
    HExpandI,
    RefColor,
    RefLabel,
    CurC,
    CurD,
    RefSel,
    RefVpd,
    RefOffset,
    RefExport,
    RefImport,
    RefLoad,
    MaskSave,
    MaskLoad,
    MathLinA,
    MathLinB,
    MeasClear,
    MeasIdle,
    MeasOutside,
    MeasN,
    MeasEdges,
    MeasRefs,
    /// Levels: percent or volts, low, middle, high, hysteresis, top/base method, fall levels
    MeasLvType,
    MeasLow,
    MeasMid,
    MeasHigh,
    MeasHyst,
    MeasBaseTop,
    MeasSepFall,
    MeasFallLow,
    MeasFallMid,
    MeasFallHigh,
    MeasGate,
    /// Studio: the badge row under the lanes as Studio chips or as the classic strip
    BadgeStyle,
    /// Studio: the zoom controls as a pull-down tab over the zoom view, or in the title bar of the waveform
    ZoomBar,
    MeasTsetup,
    MeasThold,
    CurMode,
    CurSrcB,
    CurA,
    CurB,
    /// Studio cursor settings (ui/addrail.rs): type, source, second source, readout place, linked, remove
    CurType,
    CurSrc,
    CurSecond,
    CurReadout,
    CurLink,
    CurRemove,
    /// Studio Edge search settings (ui/addrail.rs)
    SrchSrc,
    SrchSlope,
    SrchLevel,
    Srch50,
    SrchFromTrig,
    SrchToTrig,
    SrchEvent,
    SrchRemove,
    Persist,
    Intensity,
    GridStyle,
    /// view density (ui/windows.rs): Scope, Full, Split, Four
    WinLayout,
    WinCh(usize),
    /// window of reference R<n+1> in a split
    WinRef(usize),
    WinMath(usize),
    WinLa,
    ClearDisp,
    Dots,
    /// Display > Layout: the stock OOS bars or the compact ones
    Layout,
    /// Display > Bottom bar: Cards or Compact (`Model::bar_compact`)
    BottomBar,
    /// System > Features: one row per feature of `features::features()`, one per decoder of `PROTOS`
    FeatOn(usize),
    FeatDec(usize),
    /// Display > Monitor view (ui/monitor.rs)
    HdmiView,
    Hdmi,
    TextSize,
    ZInput,
    Histogram,
    SpecOn,
    SpecSrc,
    SpecCenter,
    SpecSpan,
    SpecRbw,
    SpecWin,
    SpecTrace,
    SpecRef,
    SpecWf,
    SpecDist,
    SpecAcprW,
    SpecAcprS,
    SpecPeak,
    /// harmonics the pane's span follows
    SpecHarm,
    /// what a chart of the Trend pane shows
    TrendSel(usize),
    EyeOn,
    /// one press: measure the bit rate from the record and open the eye
    EyeAct,
    EyePlace,
    EyeSrc,
    EyeRate,
    EyeClear,
    EyeBer,
    EyeB,
    EyeHp,
    EyePnLo,
    EyePnHi,
    EyeNpj,
    EyeRail,
    EyeNotch,
    /// closed-loop bandwidth of the recovered clock (0 = auto)
    EyePll,
    PowOn,
    PowV,
    PowI,
    PowMode,
    PowFline,
    PowFsw,
    PowVin,
    PowRef,
    PowClk,
    IdentifyAct,
    ProtoWatch,
    IdFind(usize),
    AfgSel,
    AfgOn,
    AfgArbSrc,
    AfgArbGrab,
    AfgArbFile,
    GridBright,
    GridFrame,
    /// noise reject of the trigger comparators
    TrigNoise,
    /// the auto set-up turns noise reject on for a noisy trigger signal (opt-in)
    TrigAutoNoise,
    /// coupling of the edge trigger
    TrigCoup,
    LaOn,
    DecIn,
    LaShow,
    /// threshold preset / user threshold of pod 0 (D0-D7) or 1 (D8-D15)
    LaThr(usize),
    LaUser(usize),
    LaHeight,
    LaLine(usize),
    LaPat(usize),
    BwOn,
    BwSrc,
    BwHint,
    BwBins,
    AfgFreq,
    AfgFreqEntry,
    AfgAmpl,
    AfgOffset,
    AfgWave,
    AfgDuty,
    AfgSym,
    AfgMod,
    AfgModRate,
    AfgModDepth,
    AfgModShape,
    BodeOn,
    BodeRun,
    BodeStart,
    BodeStop,
    BodePoints,
    BodeAmp,
    BodeIn,
    BodeOut,
    BodeGen,
    BodeAddr,
    BodeLink,
    BodeCur,
    BodeMode,
    BodeSwTime,
    BodeSwLog,
    BodeSync,
    BodeSyncEdge,
    BodeSyncLvl,
    BodeRef,
    CalCh,
    CalZero,
    CalAdc,
    CalStart,
    CalCancel,
    CalStatus,
    ZoneSel,
    ZoneOn,
    ZoneSrc,
    ZoneMode,
    ZoneBox,
    ZoneShape,
    ZoneClear,
    About,
    SysInfo,
    Shot,
    AuxOut,
    Stock,
    Update,
    UpdateRollback,
    BootMode,
    Overclock,
    ViewExit,
    ViewSettings,
    SrcOn,
    NetEth,
    NetMac,
    NetWeb,
    NetPorts,
    NetWlan,
    /// System > Network > Remote services (ui/remote.rs): a switch per service, and where it is reached
    RemWeb,
    RemScpi,
    RemVxi,
    RemTelnet,
    RemSsh,
    RemScpiInfo,
    RemVxiInfo,
    RemTelnetInfo,
    /// System > Network > Remote services > SSH settings (ui/ssh.rs); `SshRemoveKey` is its picker
    SshState,
    SshPassword,
    SshShowPass,
    SshHostKey,
    SshKeys,
    SshKeyList,
    SshImport,
    SshRemove,
    SshRemoveKey,
    /// the mDNS switch, and the scope's name on the network (`<name>.local`, a picker with Other on the keyboard)
    RemMdns,
    RemName,
    /// who may reach the scope (`net.allow`, a picker with Other on the keyboard), and whether that is in force
    RemAllow,
    RemFirewall,
    /// System > Graphics: CPU or GPU (ui/graphics.rs)
    Graphics,
    /// Display > Views (ui/views.rs)
    Views,
    /// Display > Backlight, percent (ui/backlight.rs)
    Backlight,
    WlanInfo,
    WlanScan,
    WlanForget,
    WlanNet,
    WlanForgetNet,
    /// one more autonegotiation for a link at 10 Mbit/s (ethlink.rs)
    NetRetry10,
    /// System > Date and time (ui/time.rs)
    TimeNow,
    NtpOn,
    NtpServer,
    NtpStatus,
    NtpSync,
    TimeZone,
    TimeSet,
    ColorGrade,
    StoreSlot,
    StoreDest,
    StoreSaveSetup,
    StoreRecall,
    StoreCsv,
    StorePng,
    StoreFfSave,
    StoreFfLoad,
    MaskOn,
    MaskSrc,
    MaskX,
    MaskY,
    MaskAction,
    MaskInvert,
    MaskCreate,
    MaskFromRef,
    MaskReset,
    MaskClear,
    QuickAct,
    /// option n of the open picker
    PickOpt(usize),
    // start menu entries for the front-panel keys
    RunStop,
    SingleAct,
    AutosetAct,
    DefaultAct,
    SlopeAct,
    ForceAct,
    NavHist,
    NavBack,
    NavFwd,
    QuickRun,
    LockAct,
    PowerAct,
    MagOn,
    MagV,
    MagI,
    MagN,
    MagAe,
    MagLe,
    MagRcu,
    MagPlot,
    MagCur,
    InvOn,
    InvPage,
    InvA,
    InvB,
    InvC,
    InvAng,
    InvPoles,
    InvAn,
    InvQty,
    InvTq,
    InvEv,
    InvEi,
    InvScale,
    InvDeg,
    InvOff,
    InvElec,
    InvQa,
    InvQb,
    InvQi,
    InvCpr,
    InvSwap,
    InvHa,
    InvHb,
    InvHc,
    DptOn,
    DptV,
    DptI,
    DptG,
    DptFourth,
    Dpt4,
    DptPct,
    DptZoom,
    SimPreset,
    SimSlot,
    SimPlay,
    SimLive,
    SimShow,
    SimRate,
    SimPat,
    SimHigh,
    SimLow,
    SimRise,
    SimBw,
    SimDeemph,
    SimNoise,
    SimSsc,
    SimSscCenter,
    SimSscDeviation,
    SimSscHz,
    SimDcd,
    SimCorrRj,
    SimCorrHz,
    SimRj,
    SimPj,
    SimPjHz,
    SimDefaults,
    Offline,
    /// Appended so all previous menu item discriminants remain stable.
    DecCphyRate,
    /// Remove a selected row, indexed across single, pair and catalog rows.
    MeasRemove(u16),
    CatSourceC,
    CatSourceD,
    /// SIM sheet: write the signal to the chosen ref (no SIM mode)
    SimAddRef,
    /// Studio SIM sheet: a waveform file into the SIM slot (and into playback)
    SimLoad,
}

#[derive(PartialEq)]
enum Kind {
    Choice,
    Value,
    Action,
}

#[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
pub enum Ev {
    Key(u8),
    Touch { id: i32, phase: Phase, x: i32, y: i32 },
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
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
    FullBtn { ox: i32, oy: i32, prev: Option<(i32, i32)> },
    Offset(usize),
    RefOffset(usize),
    /// the vertical position of math channel k (its lane was dragged up or down)
    MathOffset(usize),
    Delay,
    Overview,
    /// a new zoom box drawn on the empty overview (Studio)
    ZoomNew,
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

impl Ui {
    /// Power off or restart, as the buttons of the power dialog ask (SCPI `:SYSTem:POWer:OFF|RESTart CONFIRM`): the main loop
    /// carries it out. False while an update job runs: a cut now leaves recovery half-written (docs/update.md).
    pub(crate) fn power_request(&mut self, action: PowerAction) -> bool {
        if self.update_busy() {
            self.toast("Update running: wait until it is done");
            return false;
        }
        self.power_action = Some(action);
        self.toast(match action { PowerAction::Off => "Powering off ...", PowerAction::Restart => "Restarting ..." });
        true
    }

    /// How many stored frames can be shown. Frame 1 is the oldest.
    pub(crate) fn history_len(&self) -> usize {
        self.history.len()
    }

    /// The frame on screen, 1-based. Live is the newest stored frame.
    pub(crate) fn history_frame(&self) -> usize {
        self.hist_pos.map(|i| i + 1).unwrap_or(self.history.len())
    }

    /// Shows frame `frame` (1-based) and stops. Frame 1 is the oldest.
    pub(crate) fn history_show(&mut self, frame: usize) -> bool {
        let n = self.history.len();
        if frame == 0 || frame > n {
            return false;
        }
        self.m.run = RunState::Stop;
        let i = frame - 1;
        self.hist_pos = Some(i);
        self.last[0] = Some(self.history[i].clone());
        self.measure_last();
        self.toast(&format!("History {}/{}", frame, n));
        true
    }

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

/// One record's measurements: (channel, samples, dt, the smallest swing that counts as a signal).
type MeasJob = (u64, Vec<(usize, MeasSrc, f64, f64)>);
type MeasDone = (u64, Vec<(usize, Stats, oos_dsp::measure::Analysis)>);

/// The measurements of a record, each channel on its own thread (the four channels of 100 kpts took 27 ms in a row
/// on the unit).
fn measure_channels(work: &[(usize, &[f32], f64, f64)]) -> Vec<(usize, Stats, oos_dsp::measure::Analysis)> {
    let one = |&(c, v, dt, min): &(usize, &[f32], f64, f64)| (c, stats(v, dt), oos_dsp::measure::analyze_min(v, dt, min));
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
                    let w: Vec<(usize, &[f32], f64, f64)> = work.iter().map(|(c, v, dt, min)| (*c, v.samples(), *dt, *min)).collect();
                    if done.send((n, measure_channels(&w))).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(MeasWorker { tx, rx, busy: false })
    }
}

/// What Clear eye / Clear counter / Reset counts / Clear limit / Delete (zone) say when there was nothing to clear.
pub(super) const NOTHING_TO_CLEAR: &str = "Nothing to clear";

#[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
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
    pub lan_status: String,
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    pub lan_web_port: Option<u16>,
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
    /// The last Measure > Copy meas. to: from, to, how many (the row shows the result instead of "Pick").
    meas_copied: Option<(WaveId, WaveId, usize)>,
    /// the waveform windows in the plot (one: the plot itself); `area` is the first while nothing else is drawn
    wins: Vec<Rect>,
    /// the window being drawn
    cur_win: usize,
    /// Full density on the panel: the plot covers the top bar and the cards. Stays off while the external display
    /// shows the waveform, so the panel keeps the controls, unless the expand button asked for it (`full_asked`).
    full: bool,
    /// Top left of the corner button after a drag. None keeps it at the top right of the plot.
    full_at: Option<(i32, i32)>,
    /// The time view (zoomed or not, s/div, delay) the time cursors were last placed in (`cursor_follow_view`).
    cursor_view: Option<(bool, f32, f32)>,
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
    pub(crate) search_on: bool,
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
    studio: studio::Studio,
    sheet: sheet::SheetState,
    pub desktop_panel: Option<bool>,
    /// the analyser view the last frame had: a change of view analyses what is already there (a stopped record gets no new one)
    analyzed_view: AView,
    sim_play: bool,
    /// SIM mode playing offline refs (file or Add to R<n>) as live acquisitions: the slots in play.
    sim_refs: Vec<usize>,
    sim_generation: Option<sim::Generation>,
    sim_error: Option<String>,
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    sim_i: usize,
    /// Offline mode. Off, the live screen stays as it is.
    offline: bool,
    /// Take over, offline waveforms, or the live scope.
    look: reflook::RefLook,
    /// Settings of each added measurement. Missing keys follow the shared measure menu.
    meas_own: std::collections::HashMap<(Meas, WaveId), reflook::MeasOwn>,
    /// The same of the views that are not shown (`ui/views.rs`).
    view_own: [std::collections::HashMap<(Meas, WaveId), reflook::MeasOwn>; VIEWS_MAX],
    /// The gate a row measures with while its key or samples are made (None: the shared one).
    row_gate: std::cell::Cell<Option<bool>>,
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
    /// the XY plot's X and Y channels (the Add panel sets them; the default is CH1 against CH2)
    xy_src: [usize; 2],
    /// the histogram's own channel (the Add panel sets it; None follows the measurement source)
    hist_ch: Option<usize>,
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
    /// the numeric entry pad of the Studio sheets (ui/numpad.rs); its text is `keypad`
    numpad: Option<numpad::NumPad>,
    /// the value a ladder stepper started from when it was between two rungs: (item, that value, the rung it went to)
    ladder_home: Option<(Item, f64, f64)>,
    /// the last press of an `Add` stepper that ran into a limit: (item, the limit, the value it came from)
    clamp_home: Option<(Item, f64, f64)>,
    /// The source the cursors measure when it was picked in the cursor menu: only the cursors move to it (the active
    /// channel and the measurement source stay). Selecting a trace anywhere else lets the cursors follow it again.
    cursor_pin: Option<WaveId>,
    /// the Eye source was chosen on purpose (its sheet, SIM, a loaded record): Auto keeps it instead of the selected lane
    eye_src_chosen: bool,
    /// Channels a source row turned on by itself (`feedback.rs`): turned off again when the row moves on, unless used.
    auto_on: Vec<usize>,
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

/// A 1-based inclusive page. `stop == 0` means the last sample. A start past the end is an empty page.
fn copy_page(v: &[f32], dt: f64, t0: f64, start: u64, stop: u64) -> (Vec<f32>, f64, f64) {
    let n = v.len() as u64;
    if n == 0 || start > n {
        return (Vec::new(), dt, t0);
    }
    let a = start.max(1);
    let b = if stop == 0 { n } else { stop.clamp(a, n) };
    let lo = (a - 1) as usize;
    let hi = b as usize;
    let page = v[lo..hi].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect();
    (page, dt, t0 + lo as f64 * dt)
}

#[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
impl Ui {
    /// The fonts of the text size.
    pub fn fonts(&self) -> &'static Fonts {
        Fonts::get(self.m.text_size.index())
    }

    /// The layout sizes of the text size.
    pub fn lay(&self) -> &'static Lay {
        &LAYS[self.m.text_size.index()]
    }

    pub fn new(src: Source) -> Ui {
        let mut m = Model::default();
        m.meas = Self::default_meas();
        if let Source::Replay { .. } = src {
            // the replayed frame of 2026-11-02: 3.2 µs at 0.8 ns
            m.tscale = tscales().iter().position(|&t| (t - 200e-9).abs() < 1e-12).unwrap_or(0);
            m.run = RunState::Stop;
            m.trig_level = 1.2;
            m.ch[0].offset = -1.25;
        }
        let mut ui = Ui {
            cat_tab: 0,
            cat_sel: 1,
            cat_open: [1; crate::measures::CATEGORIES.len()],
            cat_scroll: 0,
            cat_c: WaveId::Ch(2),
            cat_d: WaveId::Ch(3),
            meas_page: 0,
            meas_catalog_page: 0,
            workflow_hold: None,
            session_is_sim: false,
            replay: workflow::Replay::default(),
            workflow_epoch: 0,
            bus_candidate: Default::default(),
            open_wave_request: false,
            load_slot_ask: false,
            load_destination: None,
            lan_status: String::new(),
            lan_web_port: None,
            m,
            src,
            ph: Phosphor::new(1, 1),
            phz: Phosphor::new(1, 1),
            area: Rect::new(0, 0, 0, 0),
            grid: Rect::new(0, 0, 0, 0),
            layout_zoom: false,
            zoom_saved: None,
            zoom_vfactor: 1.0,
            zoom_vcenter: 0.0,
            zoom_user_closed: false,
            meas_copied: None,
            wins: Vec::new(),
            cur_win: 0,
            full: false,
            full_at: None,
            cursor_view: None,
            full_asked: false,
            vmaxz: [1; NCH],
            trace_bus: Default::default(),
            rng: Rng::new(0x9e3779b97f4a7c15),
            menu: MenuId::Main,
            menu_open: false,
            parent: None,
            menu_history: Vec::new(),
            pick: None,
            focus: 0,
            meas_src: WaveId::Ch(0),
            last: [None],
            stats: [Stats::default(); NCH],
            ana: Default::default(),
            ref_ana: std::array::from_fn(|_| Default::default()),
            ref_ana_valid: [false; NREF],
            meas_bg: None,
            meas_want: false,
            meas_n: 0,
            meas_env_sent: [false; NCH],
            meas_env: [false; NCH],
            meas_pending: None,
            bus_auto: None,
            final_req: 0,
            stop_final: None,
            meas_new: false,
            history: std::collections::VecDeque::new(),
            hist_pos: None,
            search_on: false,
            toast: None,
            dialog: None,
            sysinfo: None,
            sysinfo_t: 0.0,
            panel_tx: Vec::new(),
            power_dlg: false,
            power_action: None,
            upd: Default::default(),
            wlan: Default::default(),
            last_rec_t: 0.0,
            rec_iv: 0.0,
            keypad: String::new(),
            numpad: None,
            ladder_home: None,
            clamp_home: None,
            cursor_pin: None,
            eye_src_chosen: false,
            auto_on: Vec::new(),
            la_cols: Default::default(),
            dec_cache: Default::default(),
            table: None,
            table_top: 0,
            zone_sel: 0,
            zone_arm: None,
            zone_path: Vec::new(),
            zoom_arm: false,
            zoom_path: Vec::new(),
            zoom_map: None,
            zone_drops: 0,
            zone_hw: None,
            zone_hw_clear: 0,
            fast_edges: [0; 4],
            fast_off: (0.0, 0.0),
            fast_take: crate::board::QUEUE_MAX as f32,
            stop_ti_off: None,
            stop_raw: None,
            stop_pic: None,
            t_fast: 0.0,
            t_fast_ab: (0.0, 0.0),
            t_fast_fold: 0.0,
            fast_n: 0.0,
            acc_t: Default::default(),
            seg_n: 0,
            seg_run: 0,
            seg_view: None,
            seg_pos: None,
            seg_times: Vec::new(),
            was_running: false,
            azr: Default::default(),
            cal: Default::default(),
            hdmi_snap_want: false,
            hdmi_present: false,
            hdmi_snap: Vec::new(),
            hdmi_over: None,
            hdmi_out: None,
            start_up: None,
            start_up_last: None,
            bode: Default::default(),
            workspace: workspace::Workspace::default(),
            studio: studio::Studio::default(),
            sheet: sheet::SheetState::default(),
            desktop_panel: None,
            analyzed_view: AView::Scope,
            sim_play: false,
            sim_refs: Vec::new(),
            sim_generation: None,
            sim_error: None,
            sim_i: 0,
            offline: false,
            look: reflook::RefLook::Live,
            meas_own: std::collections::HashMap::new(),
            view_own: Default::default(),
            row_gate: std::cell::Cell::new(None),
            meas_cfg: None,
            offline_ref: 0,
            offline_hold: None,
            touch: TouchState { down: false, x0: 0, y0: 0, x: 0, y: 0, t0: 0.0, held: false, drag: Drag::None, start_offset: 0.0, start_delay: 0.0, start_level: 0.0, start_cur: 0.0, win: 0 },
            mstat: Vec::new(),
            trend: Vec::new(),
            pn: Default::default(),
            log: logger::Logger::default(),
            single_armed: false,
            stats_on: false,
            results_min: false,
            eye_from: 1.0,
            eye_to: 1.0,
            eye_t: 0.0,
            eye_big: None,
            cleared: false,
            refs: std::array::from_fn(|_| None),
            cnt_total: 0,
            cnt_stat: MStat::default(),
            cnt_hw: None,
            cnt_clear: 0,
            dvm_hw: None,
            dvm_lane: None,
            meas_src_b: WaveId::Ch(1),
            ref_sel: 0,
            ref_src: 0,
            ref_files: Vec::new(),
            xy: false,
            xy_src: [0, 1],
            hist_ch: None,
            pinch: Pinch { active: false, p0: (0, 0), p1: (0, 0), ref_dx: 0, ref_dy: 0, area: Rect::new(0, 0, 0, 0), detail: false },
            knob_t: [0.0; 16],
            now: 0.0,
            level_seen: (f32::NAN, f32::NAN),
            level_changed: 0.0,
            frames: 0,
            records: 0,
            auto: None,
            operation_serial: 0,
            scpi_completed: std::collections::VecDeque::new(),
            single_operation: 0,
            auto_bar: None,
            slot_req: 0,
            fps: 0.0,
            wfm_rate: 0.0,
            wfm_total: 0,
            net: String::new(),
            net_if: String::new(),
            net_ifs: Vec::new(),
            net_fixed: false,
            wlan_stick: false,
            limits_path: oos_hal::model::LIMITS_CONF.into(),
            clock_next: oos_hal::model::platform().limits.adc_clock_hz,
            eth_link: false,
            eth: Default::default(),
            sys: Default::default(),
            sys_path: None,
            ntp: Default::default(),
            ssh: Default::default(),
            remote: Default::default(),
            clock_known: false,
            t_step: 0.0,
            t_acq: 0.0,
            t_draw: 0.0,
            t_render: 0.0,
            t_present: 0.0,
            t_gpu: None,
            t_gpu_screens: None,
            t_parts: [0.0; 4],
            info: String::new(),
            dirty_map: true,
            vmax: [1; NCH],
            vmax_cg: 1,
            vmaxz_cg: 1,
            pic_frames: 0,
            per_frame: [1; NCH],
            per_framez: [1; NCH],
            per_frame_cg: [1; 2],
            store_dir: std::path::PathBuf::from(crate::store::DEFAULT_DIR),
            store_dest: crate::store::StoreDest::Auto,
            math_fam: MathFamily::Arith,
            math_state: Default::default(),
            capture_views: Default::default(),
            scr: Screen::PANEL,
            view_pass: false,
            usb: false,
            ch_hold: None,
            card_hold: None,
            la_probe: None,
            usb_check: 0.0,
            store_slot: 1,
            mask_ref: None,
            mask: None,
            mask_pass: 0,
            mask_fail: 0,
            mask_cols: None,
            mask_hw: None,
            mask_hw_clear: 0,
            unit_blocked: None,
            shot_request: None,
            quit: false,
        };
        ui.ensure_meas_own();
        ui.relayout();
        ui
    }

    /// Measurement results in a panel on the right instead of the strip under the grid.
    fn results_panel(&self) -> bool {
        self.stats_on && !self.studio_owns_measurements() && !(self.m.meas.is_empty() && self.m.meas2.is_empty() && self.m.cat.is_empty())
    }

    /// Channels with measurements, in the order of their first measurement (one strip row each).
    fn meas_groups(&self) -> Vec<WaveId> {
        let mut v = Vec::new();
        for id in self.m.meas.iter().map(|e| e.1).chain(self.m.meas2.iter().map(|e| e.1)).chain(self.m.cat.iter().map(|e| meascatalog::primary_source(*e))) {
            if !v.contains(&id) && !self.meas_source_hidden(id) {
                v.push(id);
            }
        }
        v
    }

    /// The measurements of a default set-up: one full strip row.
    fn default_meas() -> Vec<(Meas, WaveId)> {
        [Meas::Vpp, Meas::Freq, Meas::Vavg, Meas::Vrms, Meas::Rise, Meas::PDuty].map(|k| (k, WaveId::Ch(0))).to_vec()
    }

    /// Strip rows of the measurements of channel `c`: six to a row.
    fn meas_strip_rows(&self, id: WaveId) -> i32 {
        let n = self.m.meas.iter().filter(|e| e.1 == id).count() + self.m.meas2.iter().filter(|e| e.1 == id).count() + self.m.cat.iter().filter(|e| meascatalog::primary_source(**e) == id).count();
        n.div_ceil(tools::STRIP_PER_ROW) as i32
    }

    fn strip_rows(&self) -> i32 {
        if self.m.strip_dock == StripDock::Hidden { return 0; }
        let dvm = (self.m.dvm_on || self.m.cnt_on) as i32;
        // Studio shows measurements and cursor read-outs as badges and in its panel
        if self.studio_owns_measurements() { return dvm; }
        if self.results_pane_on() {
            return dvm;
        }
        let meas = if self.results_panel() { 0 } else { self.meas_groups().iter().map(|&c| self.meas_strip_rows(c)).sum() };
        self.visible_measurement_rows(meas) + (self.m.cursor != CursorMode::Off) as i32 + dvm
    }

    /// The plot: 10 x 8 divisions between the top bar and the cards; it gets shorter for strip rows (the results
    /// panel is an overlay and takes no room). With the eye open the trace keeps the left part, or the top.
    fn plot_rect(&self) -> Rect {
        // Studio: the waveform's place in the dock; the other views have places of their own
        if self.dock_on() {
            let l = self.dock_wave_rect();
            let top = self.plot_top();
            let h = ((self.plot_base() - self.la_strip_h()) / 8 * 8).max(96);
            return Rect::new(l.x, top, l.w, h);
        }
        // the logic analyser lanes have their own strip under the graticule: the analog picture shrinks with it
        let avail = self.plot_base() - self.la_strip_h() - self.pane_h();
        let top = self.plot_top();
        let (plot_x, plot_w) = self.plot_span();
        if self.eye_open() && self.eye_side() && self.workspace.eye_float.is_none() {
            let hi = (plot_w - 300).max(0);
            let wave_w = (plot_w * 58 / 100).clamp(360.min(hi), hi);
            let h = (avail / 8 * 8).max(160);
            return Rect::new(if self.workspace.eye_left { plot_x + plot_w - wave_w } else { plot_x }, top, wave_w, h);
        }
        let h = ((avail - self.eye_below()) / 8 * 8).max(160);
        Rect::new(plot_x, top, plot_w, h)
    }

    /// Top bar, waveform area and bottom bar of the screen.
    pub fn frame(&self) -> Frame {
        let mut f = Frame::of(self.scr);
        if !self.m.compact() {
            return f;
        }
        // the compact bottom bar: the body takes what the cards would have had
        let grow = f.bottom.h - BOTTOM_H;
        f.body.h += grow;
        f.bottom = Rect::new(f.bottom.x, f.bottom.y + grow, f.bottom.w, BOTTOM_H);
        f.menu_btn = Rect::new(f.bottom.x + 4, f.bottom.y + 3, 46, 30);
        let x = f.menu_btn.right() + 6;
        f.cards = Rect::new(x, f.bottom.y + 3, (f.bottom.right() - METRICS.edge - x).max(0), 30);
        f
    }

    /// The eye is showing next to the trace (it does not replace the waveform).
    pub(super) fn eye_open(&self) -> bool {
        self.m.view == AView::Eye && !self.xy_over()
    }

    pub(super) fn eye_side(&self) -> bool {
        self.m.eye_place == EyePlace::Side || (self.m.eye_place == EyePlace::Under && self.eye_below() == 0)
    }

    /// Height taken under the trace when the eye is stacked there.
    pub(super) fn eye_below(&self) -> i32 {
        if self.dock_on() { return 0; }
        if self.workspace.eye_float.is_some() || !(self.eye_open() && self.m.eye_place == EyePlace::Under) {
            return 0;
        }
        let avail = self.plot_base() - self.la_strip_h() - self.pane_h();
        let room = avail - 160;
        if room < 140 {
            return 0;
        }
        (avail * 42 / 100).clamp(180, 260).min(room)
    }

    /// The eye's rectangle: to the right of the trace, or under it.
    pub(super) fn eye_pane_rect(&self) -> Rect {
        if self.dock_on() {
            // in the dock: the eye's own place, or the waveform's when it fills the plot
            return self.dock_content(dock::VKind::Eye).unwrap_or_else(|| {
                // the eye on the whole plot has the waveform's place; one that is a tab behind another has none on the screen
                let w = if self.m.eye_place == EyePlace::Full { self.dock_wave_rect() } else { let r = self.dock_region(); Rect::new(r.x - r.w - 100, r.y, r.w, r.h) };
                Rect::new(w.x, w.y + dock::STRIP_H, w.w, (w.h - dock::STRIP_H).max(1))
            });
        }
        if let Some(rect) = self.workspace.eye_float { return rect; }
        let wave = self.plot_rect();
        if self.m.eye_place == EyePlace::Full { return wave; }
        if self.eye_side() {
            if self.workspace.eye_left { return Rect::new(self.plot_span().0, wave.y, (wave.x - self.plot_span().0 - 4).max(1), wave.h); }
            let x = wave.x + wave.w + 4;
            Rect::new(x, wave.y, (self.plot_span().0 + self.plot_span().1 - x).max(1), wave.h)
        } else {
            Rect::new(wave.x, wave.y + wave.h + 2, wave.w, (self.eye_below() - 4).max(1))
        }
    }

    /// Where the eye page itself is drawn: under the placement bar, on the whole plot or in the pane.
    fn eye_page_rect(&self) -> Rect {
        if !self.eye_open() {
            return self.area;
        }
        let a = self.eye_pane_rect();
        const BAR: i32 = workspace::HEADER;
        Rect::new(a.x, a.y + BAR, a.w, (a.h - BAR).max(1))
    }

    /// Height for the graticule and the LA strip together (above a strip that sits under the plot).
    fn plot_base(&self) -> i32 {
        let under = if self.m.strip_dock == StripDock::Below { self.strip_rows_h() } else { 0 };
        self.plot_bottom() - 2 - self.plot_top() - under
    }

    /// Full density drawn on the panel. Not while the external display shows the waveform (the panel stays the
    /// controls) unless the expand button asked for it, and not with the touch lock, whose way out is the Menu button.
    fn full_on(&self) -> bool {
        self.full && !self.m.touch_lock
    }

    fn plot_top(&self) -> i32 {
        if self.dock_on() { return self.dock_wave_rect().y + dock::STRIP_H; }
        let y = if self.full_on() { 0 } else { self.frame().body.y };
        let y = if self.m.strip_dock == StripDock::Above { y + self.strip_rows_h() } else { y };
        y
    }

    /// Where the measurement rows end: the cards, or the bottom of the screen.
    fn plot_bottom(&self) -> i32 {
        if self.dock_on() { return self.dock_wave_rect().bottom() - self.studio_row_h(); }
        if self.full_on() { self.scr.h } else { self.frame().bottom.y - self.studio_row_h() }
    }

    /// Waveform windows in use: one while zoom, XY or an analysis view has the plot.
    fn n_wins(&self) -> usize {
        if self.stacked() { return self.trace_sources().len().max(1); }
        if self.m.zoom || self.view_on() || self.eye_open() { 1 } else { self.m.win_layout.count() }
    }

    /// The windows in the plot `g`, all of the same size (they share the phosphor size), in reading order.
    fn split_wins(&self, g: Rect) -> Vec<Rect> {
        const GAP: i32 = 6;
        if self.stacked() {
            let n = self.n_wins() as i32;
            let gap = if n > 8 { 2 } else if self.m.layout == Layout::Studio && self.studio.compact { 1 } else { 4 };
            let h = ((g.h - (n - 1) * gap) / n).max(1);
            return (0..n).map(|row| Rect::new(g.x, g.y + row * (h + gap), g.w, h)).collect();
        }
        let (cols, rows) = match (self.n_wins(), self.m.win_layout) {
            (1, _) => return vec![g],
            (_, WinLayout::Cols) => (2, 1),
            (_, WinLayout::Rows) => (1, 2),
            _ => (2, 2),
        };
        let w = (g.w - (cols - 1) * GAP) / cols;
        // a whole number of pixels per division keeps the graticule even
        let h = ((g.h - (rows - 1) * GAP) / rows / 8 * 8).max(64);
        let mut v = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                v.push(Rect::new(g.x + c * (w + GAP), g.y + r * (h + GAP), w, h));
            }
        }
        v
    }

    /// Window of channel `c` (a window number beyond the layout goes to the last).
    fn win_of(&self, c: usize) -> usize {
        if self.stacked() { return self.trace_sources().iter().position(|source| *source == workspace::Trace::Channel(c)).unwrap_or(usize::MAX); }
        self.m.ch_win[c].min(self.wins.len().max(1) - 1)
    }

    /// Window of reference `i` in a split: its Windows row; in Stacked its own row.
    fn ref_win_of(&self, i: usize) -> usize {
        if self.stacked() { return self.trace_sources().iter().position(|source| *source == workspace::Trace::Reference(i)).unwrap_or(usize::MAX); }
        self.m.ref_win[i].min(self.wins.len().max(1) - 1)
    }

    fn math_win_of(&self, k: usize) -> usize {
        if self.stacked() { return self.trace_sources().iter().position(|source| *source == workspace::Trace::Math(k)).unwrap_or(usize::MAX); }
        self.m.math_win[k].min(self.wins.len().max(1) - 1)
    }

    /// Whether channel `c` belongs to the window being drawn.
    fn shows(&self, c: usize) -> bool {
        self.win_of(c) == self.cur_win
    }

    /// The rectangle channel `c` is drawn in: its window, or the main waveform area (zoom).
    fn ch_area(&self, c: usize) -> Rect {
        if self.wins.len() > 1 { self.wins.get(self.win_of(c)).copied().unwrap_or(self.area) } else { self.area }
    }

    /// Window of the LA lanes.
    fn la_win_of(&self) -> usize {
        self.m.la_win.min(self.wins.len().max(1) - 1)
    }

    /// Area whose time axis the LA lanes and the decoders on them use: the window of the LA, or the main area.
    fn lane_area(&self) -> Rect {
        if self.wins.len() > 1 { self.wins[self.la_win_of()] } else { self.area }
    }

    /// Whether window `w` has an analog channel or a math channel in it.
    fn win_has_analog(&self, w: usize) -> bool {
        (0..NCH).any(|c| self.m.ch[c].on && self.win_of(c) == w) || (0..NMATH).any(|k| self.m.math_shown(k) && self.math_win_of(k) == w)
    }

    /// Window of decoder `k`'s band in the plot, or None when it has a row under the LA lanes.
    fn dec_band_win(&self, k: usize) -> Option<usize> {
        let m = self.m.dec_view(k);
        if m.dec_on_la() {
            return if self.la_dec_count() > 0 { None } else { Some(0) };
        }
        if let Some(WaveId::Ref(i)) = decode_wave(m.dec_src) {
            return self.source_area(WaveId::Ref(i)).and_then(|area| self.wins.iter().position(|r| *r == area)).or(Some(0));
        }
        Some(if self.wins.len() > 1 { self.win_of(m.dec_src) } else { 0 })
    }

    /// Area for cards over the plot (tables, results): the whole plot when it is split.
    fn over_area(&self) -> Rect {
        if self.wins.len() > 1 { self.grid } else { self.area }
    }

    /// Spreads the channels that are on over the windows of a new layout, in channel order (math channels after
    /// them), so that a split shows something in every window at once.
    fn spread_wins(&mut self) {
        let n = self.m.win_layout.count();
        let mut i = 0;
        for c in 0..NCH {
            self.m.ch_win[c] = if self.m.ch[c].on {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        for k in 0..NMATH {
            self.m.math_win[k] = if self.m.math_shown(k) {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        // shown references after them, so a loaded R1 gets a window of its own as well
        for r in 0..NREF {
            self.m.ref_win[r] = if self.refs[r].as_ref().is_some_and(|x| x.on) {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        // the LA after them: in a window of its own while there is one
        self.m.la_win = if self.m.la_on() { i % n } else { 0 };
    }

    /// The corner the expand button keeps until it is dragged: (drawn, touch area).
    fn full_home(&self) -> (Rect, Rect) {
        let g = self.grid;
        // left of the view button while an analysis view has the plot
        let right = if self.view_on() { self.view_btn().x - 6 } else { g.x + g.w - 4 };
        let r = Rect::new(right - 36, g.y + 3, 36, 28);
        Self::full_pair(r)
    }

    /// Touch area around a drawn corner button. At the home corner this is the same rectangle as before.
    fn full_pair(r: Rect) -> (Rect, Rect) {
        (r, Rect::new(r.x - 8, r.y - 3, r.w + 16, 44))
    }

    /// The expand button, at the top right of the plot or where it was dropped: (drawn, touch area).
    fn full_btn(&self) -> (Rect, Rect) {
        let (home, _) = self.full_home();
        let r = match self.full_at {
            Some((x, y)) => Rect::new(x, y, home.w, home.h),
            None => home,
        };
        Self::full_pair(r)
    }

    /// Keeps the drawn corner button on the screen.
    fn clamp_full(&self, x: i32, y: i32) -> (i32, i32) {
        let (home, _) = self.full_home();
        (x.clamp(0, (W - home.w).max(0)), y.clamp(0, (H - home.h).max(0)))
    }

    fn draw_full_btn(&self, cv: &mut Canvas) {
        let (r, _) = self.full_btn();
        let on = self.full_on();
        if self.m.layout == Layout::Studio {
            let f = self.fonts();
            let label = if on { "Exit" } else { "Full" };
            backed_text(cv, &f.s, r.x - f.s.width(label) - 6, r.y + r.h / 2 + 4, label, if on { AMBER } else { DIM });
        }
        cv.round(r, 5, if on { mix(PANEL_BG, AMBER, 40) } else { PANEL_BG }, 230);
        cv.round_outline(r, 5, if on { AMBER } else { mix(BTN_BORDER, AMBER, 90) });
        // four corner brackets: at the corners pointing in to enlarge, near the middle pointing out to go back
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let l = 7;
        let col = if on { AMBER } else { TEXT };
        for (sx, sy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            // corner of the bracket, and the way its arms go (x, y)
            // pointing out while Full is selected, including when the panel keeps its bars for the external display
            let back = self.m.density == Density::Full && !self.m.touch_lock;
            let (px, py, ax, ay) = if back { (cx + sx * 4, cy + sy * 3, sx, sy) } else { (cx + sx * 12, cy + sy * 9, -sx, -sy) };
            let (hx0, hx1) = (px.min(px + ax * (l - 1)), px.max(px + ax * (l - 1)));
            let (vy0, vy1) = (py.min(py + ay * (l - 1)), py.max(py + ay * (l - 1)));
            // 2 px thick, the second pixel on the side of the arms
            cv.fill(Rect::new(hx0, py.min(py + ay), hx1 - hx0 + 1, 2), col);
            cv.fill(Rect::new(px.min(px + ax), vy0, 2, vy1 - vy0 + 1), col);
        }
    }

    /// Corner magnifier toggles the zoom display. Draw-a-box is a separate tool.
    fn zoom_btn(&self) -> (Rect, Rect) {
        let (full, hit) = self.full_home();
        // below the full-screen touch target, so the two buttons do not share a tap
        let r = Rect::new(full.x, hit.y + hit.h + 2, full.w, full.h);
        (r, Rect::new(r.x - 5, r.y, r.w + 10, 40))
    }

    /// Studio: the trigger tag and its value move left of the corner buttons when its level is as high as they are.
    pub(super) fn trigger_tag_x(&self, x1: i32, y: i32) -> i32 {
        if self.m.layout != Layout::Studio { return x1; }
        let mut x1 = x1;
        // the corner buttons carry a word at their left ("Zoom CH1"): the tag keeps clear of that too
        let word = self.fonts().s.width("Zoom CH4") + 10;
        for b in [self.full_btn().1, self.zoom_btn().1] {
            if y + 10 > b.y && y - 10 < b.bottom() && x1 + 26 > b.x - word + 34 { x1 = x1.min(b.x - word - MARK_W); }
        }
        // a divider handle of the dock lies over the right edge of the waveform: the tag stays left of it
        if self.dock_on() {
            for d in &self.dock_layout().dividers {
                let (cx, cy) = (d.line.x + d.line.w / 2, d.line.y + d.line.h / 2);
                let h = if d.dir == dock::Dir::H { Rect::new(cx - 12, cy - 22, 24, 44) } else { Rect::new(cx - 22, cy - 12, 44, 24) };
                if y + 10 > h.y && y - 10 < h.bottom() && x1 + MARK_W > h.x && x1 < h.right() { x1 = h.x - MARK_W - 2; }
            }
        }
        x1
    }

    fn draw_zoom_btn(&self, cv: &mut Canvas) {
        let (r, _) = self.zoom_btn();
        let armed = self.m.zoom;
        // Studio: a word beside each corner icon, so full screen and zoom are not taken for each other
        if self.m.layout == Layout::Studio {
            let f = self.fonts();
            let word = match self.studio_zoom_lane().filter(|_| armed) { Some(t) => format!("Zoom {}", sharedzoom::trace_name(t)), None => "Zoom".to_string() };
            backed_text(cv, &f.s, r.x - f.s.width(&word) - 6, r.y + r.h / 2 + 4, &word, if armed { AMBER } else { DIM });
        }
        cv.round(r, 5, PANEL_BG, 220);
        cv.round_outline(r, 5, if armed { AMBER } else { BTN_BORDER });
        let col = if armed { AMBER } else { TEXT };
        cv.line(r.x + 5, r.y + 4, r.x + r.w - 5, r.y + 4, col);
        cv.line(r.x + r.w - 5, r.y + 4, r.x + r.w - 5, r.y + r.h - 4, col);
        cv.line(r.x + 5, r.y + 4, r.x + r.w - 5, r.y + r.h - 4, col);
        cv.round_outline(Rect::new(r.x + r.w - 16, r.y + 6, 8, 8), 4, col);
        cv.line(r.x + r.w - 15, r.y + 13, r.x + r.w - 21, r.y + 19, col);
    }

    /// The box while a zoom-area drag is in progress.
    fn draw_zoom_box(&self, cv: &mut Canvas) {
        if self.touch.drag != Drag::ZoomBox || self.zoom_path.len() != 2 {
            return;
        }
        let ((x0, y0), (x1, y1)) = (self.zoom_path[0], self.zoom_path[1]);
        let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
        if w < 2 && h < 2 {
            return;
        }
        cv.set_clip(self.zoom_map.map_or(self.grid, |map| map.0));
        cv.rect(Rect::new(x0.min(x1), y0.min(y1), w.max(1), h.max(1)), ACCENT);
        cv.no_clip();
    }

    /// Time base under a point of the waveform. While zoom is on, the overview keeps the main time base.
    fn zoom_drag_map(&self, x: i32, y: i32) -> Option<(Rect, f32, f32)> {
        if self.view_on() {
            return None;
        }
        if self.m.zoom {
            let (o, z) = self.split_zoom(self.grid);
            if o.contains(x, y) {
                return Some((o, self.m.time_per_div(), self.m.delay));
            }
            if z.contains(x, y) {
                let lane = self.wins.iter().copied().find(|lane| lane.contains(x, y)).unwrap_or(z);
                return Some((lane, tscales()[self.m.zscale], self.m.zdelay));
            }
            return None;
        }
        if self.area.w > 0 && self.area.contains(x, y) {
            Some((self.area, self.m.time_per_div(), self.m.delay))
        } else {
            None
        }
    }

    /// Draw boxes repeatedly until a plain tap or menu cancels this tool.
    fn arm_zoom_area(&mut self) {
        if self.view_on() {
            self.toast("Not on this view");
            return;
        }
        self.zoom_arm = !self.zoom_arm;
        if self.zoom_arm {
            self.zone_arm = None;
            self.toast("Drag a box to zoom");
        }
    }

    pub(super) fn zoom_vertical_transform(&self, area: Rect) -> (f32, f32) {
        let detail = self.split_zoom(self.grid).1;
        if self.m.zoom && !self.view_on() && area.y >= detail.y && area.y < detail.y + detail.h && self.studio_vzoom_covers(area) {
            (self.zoom_vfactor, self.zoom_vcenter)
        } else { (1.0, 0.0) }
    }

    pub(super) fn zoom_y_of_divs(&self, area: Rect, divs: f32) -> f32 {
        let (factor, center) = self.zoom_vertical_transform(area);
        area.y as f32 + area.h as f32 * 0.5 - (divs - center) * factor * area.h as f32 / VDIV
    }

    fn zoom_divs_at(&self, area: Rect, fraction: f32) -> f32 {
        let (factor, center) = self.zoom_vertical_transform(area);
        center + (0.5 - fraction) * VDIV / factor
    }

    fn zoom_display_model(&self) -> Model {
        let mut display = self.m.clone();
        for channel in &mut display.ch {
            let vpd = channel.volts_per_div();
            channel.vfine /= self.zoom_vfactor;
            channel.offset -= self.zoom_vcenter * vpd;
        }
        display
    }

    fn zoom_vertical_step(&mut self, direction: i32, area: Rect, fraction: f32) {
        let anchor = self.zoom_divs_at(area, fraction);
        self.zoom_vfactor = (self.zoom_vfactor * 2.0f32.powi(direction)).clamp(1.0, 1024.0);
        self.zoom_vcenter = anchor - (0.5 - fraction) * VDIV / self.zoom_vfactor;
        self.dirty_map = true;
    }

    fn main_horizontal_step(&mut self, direction: i32, fraction: f32) {
        let divisions = (fraction as f64 - 0.5) * HDIV as f64;
        let anchor = self.m.delay as f64 + divisions * self.m.time_per_div() as f64;
        self.adjust(Item::HScale, direction);
        self.m.delay = (anchor - divisions * self.m.time_per_div() as f64) as f32;
        self.clamp_zoom();
    }

    /// Studio: the zoom box of the overview gets wider (steps < 0) or narrower (steps > 0) about the time under x.
    pub(super) fn overview_box_step(&mut self, steps: i32, x: i32) {
        let o = self.split_zoom(self.grid).0;
        let ppx = o.w as f32 / HDIV;
        let t = self.m.delay + (x - o.x - o.w / 2) as f32 / ppx * self.m.time_per_div();
        let old = tscales()[self.m.zscale];
        let z = (self.m.zscale as i64 - steps.signum() as i64 * steps.unsigned_abs() as i64).clamp(0, (tscales().len() - 1) as i64) as usize;
        self.m.zscale = z.min(self.m.tscale);
        let new = tscales()[self.m.zscale];
        self.m.zdelay = t + (self.m.zdelay - t) * new / old;
        self.clamp_zoom();
        self.settings_changed();
    }

    fn zoom_horizontal_step(&mut self, direction: i32, _area: Rect, fraction: f32) {
        let divisions = (fraction as f64 - 0.5) * HDIV as f64;
        let anchor = self.m.zdelay as f64 + divisions * tscales()[self.m.zscale] as f64;
        self.m.zscale = (self.m.zscale as i64 + direction as i64).clamp(0, (tscales().len() - 1) as i64) as usize;
        self.m.zscale = self.m.zscale.min(self.m.tscale);
        self.m.zdelay = (anchor - divisions * tscales()[self.m.zscale] as f64) as f32;
        self.clamp_zoom();
        self.settings_changed();
    }

    /// Plain wheel pans time; Ctrl-wheel zooms time around the pointer.
    pub fn pointer_wheel(&mut self, x: i32, y: i32, steps: i32, zoom_time: bool) -> bool {
        self.pointer_wheel_motion(x, y, steps, zoom_time, false)
    }

    fn pointer_wheel_motion(&mut self, x: i32, y: i32, steps: i32, zoom_time: bool, vertical: bool) -> bool {
        if self.eye_open() && self.eye_pane_rect().contains(x,y) { return false; }
        if self.load_slot_ask || self.menu_open || self.dialog.is_some() || self.power_dlg || self.m.touch_lock || self.view_on() || !self.grid.contains(x, y) { return false; }
        // Studio: the wheel on an axis strip changes that scale
        if self.axis_wheel(x, y, steps) { return true; }
        let lane = self.wins.iter().position(|area| area.contains(x, y));
        let detail = self.m.zoom && lane.is_some();
        // Studio: the wheel over the zoomed view zooms about the pointer (the zoom box is panned by dragging or in the overview)
        // Studio: the wheel over the overview changes the width of the zoom box about the pointer; over the zoomed
        // waveform it leaves the zoom alone (the zoom is the box's business: drag the box or the content)
        if self.m.layout == Layout::Studio && self.m.zoom && !vertical {
            if detail { return false; }
            // (Ctrl-wheel on the overview keeps changing the main time base)
            if !zoom_time && self.split_zoom(self.grid).0.contains(x, y) {
                self.overview_box_step(steps.clamp(-32, 32), x);
                self.dirty_map = true;
                return true;
            }
        }
        if self.m.zoom && lane.is_none() && !self.split_zoom(self.grid).0.contains(x, y) { return false; }
        let source = if self.stacked() { lane.and_then(|lane| self.trace_sources().get(lane).copied()) } else { None };
        match source {
            Some(workspace::Trace::Channel(c)) => self.select_workspace_source(WaveId::Ch(c)),
            Some(workspace::Trace::Reference(i)) => self.select_workspace_source(WaveId::Ref(i)),
            Some(workspace::Trace::Math(i)) => self.m.math_select(i),
            None => {},
        }
        let steps = steps.clamp(-32, 32);
        if zoom_time {
            let area = if detail { self.wins[lane.unwrap()] }
                else if self.m.zoom { self.split_zoom(self.grid).0 }
                else { lane.map(|index| self.wins[index]).unwrap_or(self.area) };
            let fraction = ((x - area.x) as f32 / area.w.max(1) as f32).clamp(0.0, 1.0);
            for _ in 0..steps.unsigned_abs() {
                if detail { self.zoom_horizontal_step(-steps.signum(), area, fraction); }
                else { self.main_horizontal_step(-steps.signum(), fraction); }
            }
        } else if vertical {
            if detail {
                let area = self.wins[lane.unwrap()];
                let fraction = (y - area.y) as f32 / area.h.max(1) as f32;
                for _ in 0..steps.unsigned_abs() { self.zoom_vertical_step(steps.signum(), area, fraction); }
            } else {
                match source {
                    Some(workspace::Trace::Math(_)) => self.adjust(Item::MathScale, -steps),
                    _ => match self.selected_workspace_source() {
                        Some(WaveId::Ref(i)) => { self.ref_sel = i; self.adjust(Item::RefVpd, -steps); }
                        Some(WaveId::Ch(c)) => self.adjust(Item::ChScale(c), -steps),
                        None => return false,
                    },
                }
            }
        } else if detail {
            self.m.zdelay -= steps as f32 * tscales()[self.m.zscale] * 0.5;
            self.clamp_zoom();
        } else {
            let shift = steps as f32 * self.m.time_per_div() * 0.5;
            if self.m.layout == Layout::Studio && self.m.zoom {
                // Studio: panning the overview moves the zoom box only; the main time base and delay stay
                self.m.zdelay -= shift;
                self.clamp_zoom();
            } else {
                self.m.delay -= shift;
                if self.m.zoom { self.m.zdelay -= shift; self.clamp_zoom(); }
            }
        }
        self.dirty_map = true;
        true
    }

    /// Compact controls in the existing zoom row; no additional panel space.
    fn zoom_controls(&self) -> [(Rect, &'static str); 5] {
        let row = self.zoom_readout();
        std::array::from_fn(|i| (Rect::new(row.x + row.w - 480 + i as i32 * 62, row.y + 2, 58, row.h - 4), ["H-", "H+", "V-", "V+", "Box"][i]))
    }

    /// Zoom: overview on top, a readout row (with Close), zoomed view below.
    pub(super) fn split_zoom(&self, g: Rect) -> (Rect, Rect) {
        // Studio: no row of its own, the zoom controls are in the title strip of the waveform; the others keep their 32 px row
        let gap = if self.m.layout == Layout::Studio { 6 } else { 32 };
        // Studio: the overview has the height the user gave it
        let top = if self.m.layout == Layout::Studio { self.studio_ov_h(g) } else { 96 };
        // short: the overview goes first, the lanes and the zoom bar keep their room
        let top = if self.m.layout == Layout::Studio { let room = (g.h - gap - 96).max(0); if room < 40 { 0 } else { top.min(room) } } else { top };
        (Rect::new(g.x, g.y, g.w, top), Rect::new(g.x, g.y + top + gap, g.w, (g.h - top - gap) / 8 * 8))
    }

    /// The strip between the overview and the zoomed view.
    fn zoom_readout(&self) -> Rect {
        // Studio: the zoom controls sit in the pull-down tab over the zoom view, or in the title strip of the waveform
        if self.zoom_tab_mode() { return self.zoom_tab_region(); }
        if self.m.layout == Layout::Studio {
            let r = self.dock_wave_rect();
            let s = Self::dock_strip(r);
            let left = s.x + 150;
            let right = self.dock_extra_rect(dock::VKind::Wave, r).map_or(Self::dock_close_rect(r).x, |e| e.x);
            return Rect::new(left, s.y, (right - left).max(1), s.h);
        }
        let (o, z) = self.split_zoom(self.grid);
        Rect::new(o.x, o.y + o.h, o.w, (z.y - o.y - o.h).max(1))
    }

    /// Close removes the detail display while retaining explicit overview/timebase changes.
    pub(super) fn zoom_close(&self) -> Rect {
        let r = self.zoom_readout();
        Rect::new(r.x + r.w - 78, r.y + 4, 70, (r.h - 8).max(22))
    }

    /// The main waveform area (the zoomed view when zoom is on, the first window when the plot is split).
    fn wave_area(&self) -> Rect {
        self.wins.first().copied().unwrap_or(self.grid)
    }

    /// Recomputes the layout and reallocates the phosphor maps when the plot size changed.
    fn relayout(&mut self) {
        let g = self.plot_rect();
        // Stacked is a display format for the detail view. The shared overview
        // remains intact when horizontal zoom is enabled.
        let detail = if self.m.zoom { self.split_zoom(g).1 } else { g };
        let wins = self.split_wins(detail);
        let w0 = wins[0];
        if self.view_pass {
            // the second picture (HDMI layout): rectangles of its own, the maps of the panel
            self.grid = g;
        } else if g != self.grid || self.layout_zoom != self.m.zoom || self.wins.first() != Some(&w0) {
            self.grid = g;
            self.layout_zoom = self.m.zoom;
            if self.m.zoom {
                let (o, _) = self.split_zoom(g);
                self.ph = Phosphor::new(o.w as usize, o.h as usize);
                self.phz = Phosphor::new(w0.w as usize, w0.h as usize);
            } else {
                // one map per channel at the size of a window: each channel is drawn into its own window
                self.ph = Phosphor::new(w0.w as usize, w0.h as usize);
                self.phz = Phosphor::new(1, 1);
            }
            self.dirty_map = true;
        }
        self.wins = wins;
        self.cur_win = 0;
        self.area = self.wave_area();
    }

    /// Time base of the main waveform area: (s/div, delay).
    fn view(&self) -> (f32, f32) {
        let base = if self.m.zoom {
            (tscales()[self.m.zscale], self.m.zdelay)
        } else {
            (self.m.time_per_div(), self.m.delay)
        };
        self.studio_lane_view(base)
    }

    /// The last record for the web page, as Storage > CSV writes it. A long record is refused rather than cut: making
    /// the text runs in the frame loop, and a file that looks complete but is not would be worse.
    pub fn web_wave_csv(&self) -> Result<String, String> {
        let rec = self.last[0].as_ref().ok_or("No waveform yet")?;
        let n = rec.ch.iter().map(|c| c.len()).max().unwrap_or(0);
        if n > WEB_CSV_MAX {
            return Err(format!("{} points are too many for the browser: use Storage on the scope, or SCPI", n));
        }
        Ok(crate::store::wave_csv(rec, &self.m))
    }

    /// The last record in the binary form of Storage.
    pub fn web_wave_bin(&self) -> Result<Vec<u8>, String> {
        let rec = self.last[0].as_ref().ok_or("No waveform yet")?;
        Ok(crate::store::wave_bin(rec, &self.m))
    }

    /// The LA lines of the last record, one row per change.
    pub fn web_la_csv(&self) -> Result<String, String> {
        let la = self.last[0].as_ref().and_then(|r| r.la.as_ref()).filter(|_| self.m.la_on()).ok_or("No logic analyser record")?;
        Ok(crate::store::la_csv(la, &self.m.la.lines()))
    }

    /// The zoom window in the overview as the screen x of its two edges: its width is the overview's over the zoom
    /// factor, its place the zoom delay against the main delay.
    pub(super) fn zoom_window_x(&self, o: Rect) -> (i32, i32) {
        let (tz, dz) = (tscales()[self.m.zscale], self.m.zdelay);
        let ppx = o.w as f32 / HDIV;
        let tdiv = self.m.time_per_div();
        // a window far off the overview (an extreme delay) is held a screen width beyond it: drawn the same (it is
        // clipped to the overview), and the pixel arithmetic of the callers stays within i32
        let far = 2.0 * o.w as f32;
        let at = |t: f32| o.x + (o.w as f32 / 2.0 + (t - self.m.delay) / tdiv * ppx).clamp(-far, far) as i32;
        (at(dz - tz * HDIV / 2.0), at(dz + tz * HDIV / 2.0))
    }

    /// Where the zoom window is as a share of the record in the overview, 0 to 100 %, and its magnification.
    pub(super) fn zoom_box(&self) -> (f32, f32) {
        let tdiv = self.m.time_per_div();
        let pct = 50.0 + (self.m.zdelay - self.m.delay) / (tdiv * HDIV) * 100.0;
        (pct, tdiv / tscales()[self.m.zscale])
    }

    /// The zoom asks for more resolution than the record has: a sample is more than 3 px from the next. Studio then marks each sample with a dot
    /// and keeps the draw mode (lines join the samples; Display > Dots stays dots only).
    pub(super) fn zoom_sparse(&self, dt: f64) -> bool {
        self.m.layout == Layout::Studio && self.m.zoom && dt > 0.0
            && (self.phz.w.max(1) as f64 / HDIV as f64) * dt / tscales()[self.m.zscale] as f64 > 3.0
    }

    /// Keeps the zoom window inside the main window and not slower than the main time base.
    fn clamp_zoom(&mut self) {
        if self.m.zscale > self.m.tscale {
            self.m.zscale = self.m.tscale;
        }
        let half_main = self.m.time_per_div() * HDIV / 2.0;
        let half_zoom = tscales()[self.m.zscale] * HDIV / 2.0;
        let lim = (half_main - half_zoom).max(0.0);
        self.m.zdelay = self.m.zdelay.clamp(self.m.delay - lim, self.m.delay + lim);
    }

    fn set_zoom(&mut self, on: bool) {
        self.zoom_user_closed = !on;
        if on == self.m.zoom {
            self.relayout();
            return;
        }
        if on {
            self.zoom_saved = Some((self.m.tscale, self.m.tfine, self.m.delay));
            self.m.zscale = self.m.tscale.saturating_sub(3);
            self.m.zdelay = self.m.delay;
            self.zoom_vfactor = 1.0;
            self.zoom_vcenter = 0.0;
        } else {
            self.zoom_saved = None;
        }
        self.m.zoom = on;
        self.zoom_arm = false;
        self.zoom_path.clear();
        self.zoom_map = None;
        self.relayout();
    }

    pub(super) fn set_record_zoom(&mut self, on: bool) {
        if !self.zoom_user_closed { self.set_zoom(on); }
    }

    /// Detail scale, delay and display magnification controls in the existing zoom row.
    fn draw_zoom_readout(&self, cv: &mut Canvas, tz: f32, dz: f32) {
        let f = self.fonts();
        let row = self.zoom_readout();
        let close = self.zoom_close();
        let lbl = format!("Zoom \u{b7} {}/div \u{b7} {} \u{b7} V {:.2}x", si_short(tz as f64, "s"), si(dz as f64, "s"), self.zoom_vfactor);
        let reset = Rect::new(close.x - 76, close.y, 70, close.h);
        let controls = self.zoom_controls();
        let lab = Rect::new(row.x + 4, row.y, (controls[0].0.x - row.x - 10).max(1), row.h);
        cv.set_clip(lab);
        cv.text_in(&f.sb, lab, 0, &lbl, ACCENT);
        cv.no_clip();
        btn(cv, close, BTN, ACCENT);
        cv.text_in(&f.sb, close, 1, "Close", ACCENT);
        btn(cv, reset, BTN, BTN_BORDER);
        cv.text_in(&f.sb, reset, 1, "Reset", TEXT);
        for (rect, text) in controls {
            btn(cv, rect, if text == "Box" && self.zoom_arm { ACCENT } else { BTN }, BTN_BORDER);
            cv.text_in(&f.sb, rect, 1, text, if text == "Box" && self.zoom_arm { DARK_TEXT } else { TEXT });
        }
    }

    fn zoom_close_tap(&mut self, x: i32, y: i32) -> bool {
        let y = self.zoom_row_y(x, y);
        if self.m.zoom && !self.view_on() {
            for (rect, text) in self.zoom_controls() {
                if !rect.contains(x, y) { continue; }
                let area = self.wave_area();
                match text {
                    "H-" => self.zoom_horizontal_step(1, area, 0.5),
                    "H+" => self.zoom_horizontal_step(-1, area, 0.5),
                    "V-" => self.zoom_vertical_step(-1, area, 0.5),
                    "V+" => self.zoom_vertical_step(1, area, 0.5),
                    _ => self.arm_zoom_area(),
                }
                return true;
            }
        }
        let close = self.zoom_close();
        if self.m.zoom && Rect::new(close.x - 76, close.y, 70, close.h).contains(x, y) {
            self.reset_zoom();
            return true;
        }
        if self.m.zoom && !self.xy_over() && !self.view_on() && self.zoom_close().contains(x, y) {
            self.set_zoom(false);
            return true;
        }
        false
    }

    fn reset_zoom(&mut self) {
        if !self.m.zoom { return; }
        self.m.zscale = self.m.tscale;
        self.m.zdelay = self.m.delay;
        self.zoom_vfactor = 1.0;
        self.zoom_vcenter = 0.0;
        self.zoom_arm = false;
        self.clamp_zoom();
        self.settings_changed();
    }

    fn eye_tap(&mut self, x: i32, y: i32) -> bool {
        if self.eye_open() {
            if self.eye_pane_close().contains(x, y) {
                self.view_exit();
                self.relayout();
                return true;
            }
        }
        false
    }

    /// Logic analyser record of the last acquisition (SCPI).
    pub fn la_record(&self) -> Option<&la::LaRec> {
        self.last[0].as_ref().and_then(|r| r.la.as_ref())
    }

    /// Called after remote (SCPI) changes: same effect as a change on the screen.
    pub fn mark_changed(&mut self) {
        self.settings_changed();
    }

    #[cfg(test)]
    pub(crate) fn measurement_statistics_snapshot_for_test(&self) -> Vec<(u64, [u64; 4])> {
        self.mstat.iter().map(|stat| (stat.n,
            [stat.sum.to_bits(), stat.sumsq.to_bits(), stat.min.to_bits(), stat.max.to_bits()])).collect()
    }

    /// Measurement `k` of channel `c` (NaN when off or not measurable).
    pub fn meas(&self, c: usize, k: Meas) -> f64 {
        self.meas_at(WaveId::Ch(c), k)
    }

    /// Eye figures from the last refresh, in the order the page stores them. Empty until the eye has run.
    pub fn eye_readings(&self) -> &[(&'static str, f64)] {
        &self.azr.page.readings
    }

    /// Records read by the UI beyond what the model shows the board (`Shared::needs_raw`): the protocol watch and XY.
    fn needs_raw(&self) -> bool {
        self.azr.watch || self.xy
    }

    /// A STOP that waits for the hardware's final frame: a plain STOP (not the end of Single or segments, not a mask
    /// stop or a history view) while the record shown is of live-compression pairs, or of a deep record's pairs with raw
    /// deep measurements on.
    fn stop_wants_final(&self) -> bool {
        let m = &self.m;
        m.trig_mode != TrigMode::Single
            && m.segments == 0
            && !(m.mask_on && m.mask_stop)
            && self.hist_pos.is_none()
            && self.seg_pos.is_none()
            && self.last[0].as_ref().and_then(|r| r.raw.as_ref()).is_some_and(|r| crate::compress::live(&r.frame) || (crate::deepmeas::want(m).is_some() && crate::deepmeas::deep_pairs(&r.frame)))
    }

    /// Measurement `k` of channel `c` as last measured; NaN for a mean-type measurement made on min/max pairs.
    pub(crate) fn ana_get(&self, c: usize, k: Meas) -> f64 {
        if self.meas_env[c] && crate::framestats::needs_samples(k) {
            f64::NAN
        } else {
            self.ana[c].get(k)
        }
    }

    /// Whether channel `c` of record `rec` is measured on min/max pairs (`meas_slice`).
    pub(super) fn meas_on_pairs(&self, rec: &Record, c: usize) -> bool {
        match self.meas_frame(rec, c) {
            Some((f, _, _)) => f.envelope,
            None => rec.pairs || rec.raw.as_ref().is_some_and(|r| r.frame.envelope),
        }
    }

    #[allow(dead_code)]
    pub fn channel_stats(&self, c: usize) -> Option<Stats> {
        if self.m.ch[c].on && self.stats[c].valid {
            Some(self.stats[c])
        } else {
            None
        }
    }

    /// (points, dt, time of the first point relative to the trigger) of the last record of channel c.
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    pub fn record_info(&self, c: usize) -> Option<(usize, f64, f64)> {
        let r = self.last[0].as_ref()?;
        if r.ch[c].is_empty() {
            return None;
        }
        Some((r.ch[c].len(), r.dt, r.t0))
    }

    /// A record the protocol trigger can judge: not roll or scan (they have no trigger point), and with the decoder's
    /// input in it; otherwise it is shown as it comes rather than held back for ever.
    fn bus_can_match(&self, rec: &Record) -> bool {
        if board().slow_mode(&self.m) != crate::board::Slow::Off {
            return false;
        }
        if self.m.dec_on_la() {
            rec.la.is_some()
        } else {
            rec.ch.get(self.m.dec_src).is_some_and(|v| !v.is_empty())
        }
    }

    /// RAW read-out: (points, dt, t0) of the entire retained native acquisition; `None` when the
    /// screen record is all there is.
    pub fn raw_info(&self, c: usize) -> Option<(usize, f64, f64)> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        // a compressed frame holds min/max pairs, not samples: then the screen record is the answer
        (!v.is_empty() && !r.frame.envelope).then(|| (v.len(), r.frame.dt, r.t0 - r.i0 as f64 * r.frame.dt))
    }

    /// The samples of `raw_info`.
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    pub fn raw_volts(&self, c: usize) -> Option<Vec<f32>> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        (!r.frame.envelope).then(|| v.iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect())
    }

    pub fn record_volts(&self, c: usize) -> Vec<f32> {
        self.last[0].as_ref().map(|r| r.ch[c].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect()).unwrap_or_default()
    }

    /// The samples a `:WAVeform` read uses. `memory` is RAW, or MAXimum while stopped: the deep record when the
    /// acquisition kept one, otherwise the screen record.
    fn channel_view(&self, c: usize, memory: bool) -> Option<(&[f32], f64, f64)> {
        let rec = self.last[0].as_ref()?;
        if memory {
            if let Some(raw) = rec.raw.as_ref().filter(|r| !r.frame.envelope) {
                if let Some(v) = raw.frame.ch.get(c) {
                    if !v.is_empty() && raw.frame.dt.is_finite() && raw.frame.dt > 0.0 {
                        return Some((v.as_slice(), raw.frame.dt, raw.t0 - raw.i0 as f64 * raw.frame.dt));
                    }
                }
            }
        }
        let v = rec.ch.get(c)?;
        (!v.is_empty()).then_some((v.as_slice(), rec.dt, rec.t0))
    }

    /// How many points `channel_page` can walk. Zero when that source has no record.
    pub fn channel_points(&self, c: usize, memory: bool) -> usize {
        self.channel_view(c, memory).map(|(v, _, _)| v.len()).unwrap_or(0)
    }

    /// `(dt, t0)` of that same record. None when it is empty.
    pub fn channel_origin(&self, c: usize, memory: bool) -> Option<(f64, f64)> {
        self.channel_view(c, memory).map(|(_, dt, t0)| (dt, t0))
    }

    /// One page of a channel, 1-based inclusive, the same numbering as `:WAVeform:STARt` and `:WAVeform:STOP`.
    /// `stop == 0` reads through the last point. The time is the first sample of the page.
    pub fn channel_page(&self, c: usize, memory: bool, start: u64, stop: u64) -> Option<(Vec<f32>, f64, f64)> {
        let (v, dt, t0) = self.channel_view(c, memory)?;
        Some(copy_page(v, dt, t0, start, stop))
    }

    /// Samples of a reference slot, finite volts. Empty when that slot holds nothing.
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    pub(crate) fn ref_volts(&self, slot: usize) -> Vec<f32> {
        self.ref_page(slot, 1, 0).map(|(v, _, _, _, _)| v).unwrap_or_default()
    }

    /// One page of a reference slot. `None` when the slot is empty. Volts per division and offset travel with it.
    pub fn ref_page(&self, slot: usize, start: u64, stop: u64) -> Option<(Vec<f32>, f64, f64, f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !(r.dt > 0.0) {
            return None;
        }
        let (page, dt, t0) = copy_page(&r.v, r.dt, r.t0, start, stop);
        let vpd = if r.vpd > 0.0 { r.vpd as f64 } else { 1.0 };
        Some((page, dt, t0, vpd, r.offset as f64))
    }

    /// (points, dt, t0, volts per division, offset) of a slot that has a record.
    pub(crate) fn ref_info(&self, slot: usize) -> Option<(usize, f64, f64, f64, f64)> {
        let r = self.refs.get(slot)?.as_ref()?;
        if r.v.len() < 2 || !(r.dt > 0.0) {
            return None;
        }
        let vpd = if r.vpd > 0.0 { r.vpd as f64 } else { 1.0 };
        Some((r.v.len(), r.dt, r.t0, vpd, r.offset as f64))
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

    // ------------------------------------------------------------------ acquisition per frame

    pub fn step(&mut self, dt_wall: f64) {
        self.cursor_follow_view();
        // Acquire > Draw path: the batch pipeline of PR #7 on the hardware only
        crate::drawpipe::set_enabled(self.m.draw_batch && self.src.is_hw());
        // System > Features: what is left out runs nothing
        self.features_enforce();
        self.poll_reference_measurements();
        self.poll_catalog_measurements();
        self.math_fit_tick();
        self.sim_poll();
        self.eye_auto_poll();
        self.relayout();
        self.now += dt_wall;
        self.meas_hold_tick();
        self.studio_hold_tick();
        self.meas_mark_tick();
        self.workspace_hold_tick();
        self.update_poll();
        self.wlan_poll();
        self.ntp_poll();
        self.ssh_poll();
        self.allow_poll();
        self.usb_poll();
        self.ch_hold_poll();
        self.dock_poll();
        self.md_tick();
        self.panel_fling_step(dt_wall);
        self.zoom_tab_tick();
        if self.m.layout == Layout::Studio && self.azr.spec.is_none() && self.last[0].is_some() && (self.m.pane == tables::PANE_SPECTRUM || self.m.view == AView::Spectrum) { self.analyzer_update(); }
        self.studio_delay_clamp();
        self.counter_poll();
        self.dvm_poll();
        self.mask_hw_poll();
        self.zone_hw_poll();
        self.la_probe_poll();
        self.log_tick();
        self.m.clamp_offsets();
        if (self.m.trig_level, self.m.trig_level2) != self.level_seen {
            // the first look (start-up) is no change
            if !self.level_seen.0.is_nan() {
                self.level_changed = self.now;
            }
            self.level_seen = (self.m.trig_level, self.m.trig_level2);
        }
        self.src.publish_afg(&self.m);
        // SINGLE armed: from here on only what comes after the press counts
        let single = self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run;
        if single && !self.single_armed {
            self.m.hw_stamp += 1;
            self.hw_avg_restart();
        }
        self.single_armed = single;
        self.bus_candidate_guard_tick();
        // 2-channel models: CH3/CH4 stay off whatever switched them on (setups, autoset, demos, SCPI)
        if self.m.fit_platform() {
            self.mark_changed();
        }
        if let WaveId::Ch(c) = self.meas_src {
            if c >= n_ch() {
                self.meas_src = WaveId::Ch(0);
            }
        }
        if let WaveId::Ch(c) = self.meas_src_b {
            if c >= n_ch() {
                self.meas_src_b = WaveId::Ch(0);
            }
        }
        if self.ref_src >= n_ch() {
            self.ref_src = n_ch() - 1;
        }
        self.auto_step();
        self.sysinfo_poll();
        self.cal_poll();
        // segmented recording: a new RUN/SINGLE starts a new set of segments
        let running = self.m.run == RunState::Run;
        let hw_seg = self.seg_hw_on();
        // RUN after STOP: the hardware average starts over (stock clears it on every state change)
        if running && !self.was_running {
            self.hw_avg_restart();
        }
        if running && !self.was_running && self.m.segments > 0 {
            self.seg_n = 0;
            self.history.clear();
            self.hist_pos = None;
            if hw_seg {
                self.seg_run += 1;
                (self.seg_pos, self.seg_view) = (None, None);
                self.seg_times.clear();
            }
        }
        // STOP of a deep record on pairs with raw deep measurements: the hardware reads the newest frame raw and
        // stops arming; until it is here the records go on (docs/fork/stop-exact.md)
        if !running && self.was_running && self.stop_wants_final() {
            self.final_req += 1;
            self.stop_final = Some(self.now + STOP_FINAL_WAIT);
        }
        // held while stopped, unless the screen still waits for a first record (a source started in STOP, as the
        // record path below takes it); a Clear in STOP leaves the screen empty, not the thread free to arm
        let final_done = self.src.hw_hold(!running && (self.last[0].is_some() || self.cleared), self.final_req);
        // the protocol watch and XY read every record: no live compression for them (compress.rs)
        self.src.hw_needs_raw(self.needs_raw());
        self.was_running = running;
        if hw_seg {
            self.seg_poll(running);
        }
        if !running { self.cancel_demo_acquisition(); }
        let persist = PERSIST[self.m.persist].1;
        if self.record_mode() {
            if self.m.run == RunState::Run { self.sim_play = true; }
            self.sim_tick(dt_wall);
            if self.m.run == RunState::Stop { self.refresh_stopped_reference(); }
        }
        if self.m.run == RunState::Stop && !self.record_mode() {
            self.refresh_capture_view();
            self.refresh_stopped_measurements();
        }
        let live = self.m.n_on() > 0 || self.m.la_on();
        // a source started in STOP (replay) still shows its first record, unless the screen was cleared
        if !self.record_mode() && live && (self.m.run == RunState::Run || self.stop_final.is_some() || (self.last[0].is_none() && !self.cleared)) {
            if self.m.run == RunState::Run {
                self.stop_ti_off = None;
                (self.stop_raw, self.stop_pic) = (None, None);
            }
            if self.dirty_map {
                self.ph.clear();
                self.phz.clear();
                self.dirty_map = false;
            }
            // minimum persistence: replace the picture when a new record arrives, not every frame (at large depths
            // the hardware delivers fewer records than frames, and clearing each frame made the trace flicker)
            let mut clear_pending = persist == 0.0;
            if persist > 0.0 {
                let f = (-dt_wall / persist as f64).exp() as f32;
                self.ph.decay(f);
                self.phz.decay(f);
            }
            let fast = self.fast_ok();
            if !fast {
                self.src.hw_take_all();
            }
            let k = match self.m.trig_type {
                // fast path: it draws the queue on several cores, the record is the newest frame only (frames that
                // come in meanwhile wait for the next screen's fast path instead of a full record each, one by one)
                _ if fast => 1,
                // the hardware source delivers batches: draw up to 48 queued records per frame
                _ if matches!(self.src, Source::Hw { .. }) => 48,
                TrigType::Edge | TrigType::Pattern => 6,
                TrigType::Video => 1,
                _ => 2,
            };
            let mut got = 0;
            let (mut ta, mut td) = (0f32, 0f32);
            // the newest raw deep record of this step (deepmeas.rs): with one frame per batch (2 ms/div and up at
            // 10 M) the next pair frame is often queued behind it, which drew it on the fast path or replaced it as
            // the record, and the measurement stayed at the last one for good (unit 2026-11-13)
            let deep_raw = crate::deepmeas::want(&self.m).is_some();
            let mut raw_rec: Option<Record> = None;
            if !fast {
                if let Some(p) = crate::drawpipe::pipe() {
                    p.set_view(None, 0, 0);
                }
            }
            // fast display path (fastdraw.rs): every queued frame but the newest straight into the phosphor map
            if fast {
                let t0 = std::time::Instant::now();
                let frames = self.src.drain_newest(self.fast_max());
                if !frames.is_empty() && clear_pending {
                    self.ph.clear();
                    clear_pending = false;
                }
                let n = frames.len();
                if let Some(f) = frames.iter().rev().find(|f| deep_raw && crate::deepmeas::deep_raw_frame(f)) {
                    raw_rec = Some(crate::wave::hw_record(&self.m, f.clone()));
                }
                if n > 0 && self.fast_draw_all(frames) > 0 {
                    self.m.status = TrigStatus::Triggered;
                }
                got += n;
                let ms = t0.elapsed().as_secs_f32() * 1000.0;
                td += ms;
                self.t_fast += 0.05 * (ms - self.t_fast);
                if n > 0 && ms > 0.0 {
                    // as many as would have filled the budget at this pace: with a fixed part in each screen's time
                    // (starting the helpers) this settles where the time meets the budget. Down only when the budget
                    // was exceeded, up only when the take was the limit: the hardware delivers in bursts, and a screen
                    // with a few frames says nothing about how many fit
                    self.fast_take = fast_take_next(self.fast_take, n, ms, self.fast_budget());
                }
                // batch mode: the other frames of the batches, drawn by the deposit workers (drawpipe.rs)
                if let Some(p) = crate::drawpipe::pipe() {
                    let t1 = std::time::Instant::now();
                    p.set_view(Some(&self.m), self.ph.w, self.ph.h);
                    let d = p.merge(&mut self.ph, &mut clear_pending, &mut self.fast_edges, &mut self.fast_off);
                    if d.triggered > 0 {
                        self.m.status = TrigStatus::Triggered;
                    }
                    got += d.frames as usize;
                    self.mstat_fold(&d.folds);
                    td += t1.elapsed().as_secs_f32() * 1000.0;
                }
            }
            for _ in 0..k {
                let t0 = std::time::Instant::now();
                let r = self.acquire_bus_candidate(dt_wall / k as f64);
                ta += t0.elapsed().as_secs_f32() * 1000.0;
                if r.is_none() && self.bus_candidate_waiting() { break; }
                match r {
                    Some(mut rec) if !crate::zone::pass(&self.m.zones, &rec.ch, rec.t0, rec.dt) => {
                        // zone trigger: not a qualifying record
                        let _ = &mut rec;
                        self.zone_drops += 1;
                    }
                    // protocol trigger, Normal and Single: the hardware fires on every edge of the line, only the decoder
                    // decides; Single waits on for the record that holds the event
                    Some(rec) if self.m.trig_type == TrigType::Bus && !self.mixed_bus_mode() && self.m.trig_mode != TrigMode::Auto && !rec.triggered && !rec.partial && self.bus_can_match(&rec) => {
                        self.m.status = TrigStatus::Wait;
                    }
                    Some(mut rec) => {
                        self.bus_candidate_committed();
                        self.m.status = if rec.triggered { TrigStatus::Triggered } else { TrigStatus::Auto };
                        for c in 0..NCH {
                            if self.m.ch[c].on && !rec.ch[c].is_empty() && !self.src.is_hw()
                                && !rec.raw.as_ref().is_some_and(|raw| raw.conditioned) {
                                let dt = rec.dt;
                                condition(&self.m, c, &mut rec.ch[c], dt);
                            }
                        }
                        // the FPGA averaged the frame already: blending would average the average
                        let hw_avg = rec.raw.as_ref().is_some_and(|r| r.frame.hw_avg);
                        if self.m.acq == AcqMode::Average && !hw_avg {
                            if let Some(prev) = &self.last[0] {
                                let a = 1.0 / self.m.avg_count as f32;
                                for c in 0..NCH {
                                    if rec.ch[c].len() == prev.ch[c].len() {
                                        for (x, p) in rec.ch[c].iter_mut().zip(prev.ch[c].iter()) {
                                            if p.is_finite() {
                                                *x = p + (*x - p) * a;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // minimum persistence: the picture of this step is every frame of it (the fast path's and this
                        // record), cleared once before the first of them
                        if clear_pending {
                            self.ph.clear();
                            self.phz.clear();
                            clear_pending = false;
                        }
                        let mainv = (self.m.time_per_div(), self.m.delay);
                        let zv = (tscales()[self.m.zscale], self.m.zdelay);
                        let zoom_model = self.zoom_display_model();
                        let detail = self.captured_record_view(&rec, zv);
                        let sparse = self.zoom_sparse(detail.as_deref().map_or(rec.dt, |d| d.dt));
                        self.phz.fat = sparse;
                        let t1 = std::time::Instant::now();
                        let zw = self.z_weights(&rec);
                        // the fast path draws the other frames of the picture from their samples (`draw_frame`): the
                        // newest one the same way, or it adds about 3.6 times their counts per pixel (3600 record points
                        // against one span per column) and the grading of the picture follows how many of each it holds:
                        // the trace flickered with the batch sizes (docs/fork/flicker.md)
                        let frame = rec.raw.as_ref().filter(|_| self.fast_ok()).map(|r| r.frame.clone());
                        if let Some(f) = frame {
                            let stats = crate::drawpipe::Stats { edges: &mut self.fast_edges, off: &mut self.fast_off };
                            crate::drawpipe::draw_frame(&mut self.ph, &self.m, &f, Some(stats));
                        } else {
                            for c in 0..NCH {
                                if self.m.ch[c].on && !rec.ch[c].is_empty() && self.m.z_src != Some(c) {
                                    let shifted_main = self.deskew_channel_view(&rec, c, mainv);
                                    let main = shifted_main.as_deref().unwrap_or(&rec);
                                    self.ph.draw_ex(c, &self.m, mainv, main.t0 - self.m.deskew_ch[c], main.dt, &main.ch[c], 24, if shifted_main.is_some() { None } else { zw.as_deref() }, main.pairs);
                                    if self.m.zoom && !self.studio_zoom_covers(c) {
                                        // one lane zooms alone (Studio): this one stays at the main time base
                                        self.phz.draw_ex(c, &self.m, mainv, main.t0 - self.m.deskew_ch[c], main.dt, &main.ch[c], 24, if shifted_main.is_some() { None } else { zw.as_deref() }, main.pairs);
                                    } else if self.m.zoom {
                                        let shifted = self.deskew_channel_view(&rec, c, zv);
                                        let z = shifted.as_deref().or(detail.as_deref()).unwrap_or(&rec);
                                        self.phz.draw_ex(c, &zoom_model, zv, z.t0 - self.m.deskew_ch[c], z.dt, &z.ch[c], 24, if shifted.is_some() || detail.is_some() { None } else { zw.as_deref() }, z.pairs);
                                    }
                                }
                            }
                        }
                        td += t1.elapsed().as_secs_f32() * 1000.0;
                        if self.history.len() >= if hw_seg { HISTORY } else { HISTORY.max(self.seg_count()) } {
                            self.history.pop_front();
                        }
                        // a deep LA record is tens of MB: the history keeps the LA of short records only
                        let la = rec.la.clone().filter(|l| l.words.len() <= HISTORY_LA_WORDS);
                        self.history.push_back(Record { raw: None, la, ..rec.clone() });
                        self.hist_pos = None;
                        self.mask_test(&rec);
                        if deep_raw && rec.raw.as_ref().is_some_and(|r| crate::deepmeas::deep_raw_frame(&r.frame)) {
                            raw_rec = Some(rec.clone());
                        }
                        self.last[0] = Some(rec);
                        self.cleared = false;
                        got += 1;
                        self.records += 1;
                        if self.m.segments > 0 {
                            self.seg_n += 1;
                            // hardware segments end when the thread has read them all back (`seg_poll`)
                            if !hw_seg && self.seg_n >= self.seg_count() {
                                if self.m.trig_mode == TrigMode::Single { self.scpi_finish_operation(OperationToken::Single(self.single_operation)); }
                                self.m.run = RunState::Stop;
                                self.toast(&format!("{} segments recorded", self.seg_n));
                                break;
                            }
                        } else if self.m.trig_mode == TrigMode::Single && !self.last[0].as_ref().is_some_and(|r| r.partial) {
                            self.scpi_finish_operation(OperationToken::Single(self.single_operation));
                            self.m.run = RunState::Stop;
                            break;
                        }
                    }
                    None => {}
                }
                if self.m.run == RunState::Stop && self.stop_final.is_none() {
                    break;
                }
            }
            // WAIT only when the whole frame brought nothing (the hardware source delivers a new record only every
            // few attempts; an empty attempt must not override TRIG'D)
            // at large depths the hardware delivers fewer records than frames: WAIT only after 0.5 s without one, or
            // 1.5 record lengths at slow time bases (a 1 s record at 100 ms/div comes once per second), or 2.5 usual
            // record intervals (a 125 M record takes ~1 s to read)
            let wait_after = 0.5f64.max(1.5 * HDIV as f64 * self.m.time_per_div() as f64).max(2.5 * self.rec_iv);
            // `trig.hw_status = on`: the SCU says whether the TPU fired or the auto timeout forced the batch; it
            // overrides what the software refinement concluded (docs/fork/trigger-lockin.md)
            if got > 0 {
                if let Some((_, Some(tpu))) = self.src.hw_trig_fired() {
                    self.m.status = if tpu { TrigStatus::Triggered } else { TrigStatus::Auto };
                }
            }
            // minimum persistence shows the frames of this step alone: their number grades the picture (`picture_ref`)
            if persist != 0.0 {
                self.pic_frames = 0;
            } else if got > 0 {
                self.pic_frames = got as u32;
            }
            if got > 0 {
                let iv = (self.now - self.last_rec_t).min(2.0);
                self.rec_iv += 0.3 * (iv - self.rec_iv);
                self.last_rec_t = self.now;
            } else if self.m.run == RunState::Run && self.now - self.last_rec_t > wait_after {
                self.m.status = TrigStatus::Wait;
            }
            self.t_acq += 0.05 * (ta - self.t_acq);
            self.t_draw += 0.05 * (td - self.t_draw);
            let rate = got as f32 / dt_wall.max(1e-3) as f32;
            self.wfm_rate += 0.1 * (rate - self.wfm_rate);
            self.wfm_total += got as u64;
            crate::drawpipe::Counters::add(&crate::drawpipe::counters().used, got as u64);
            // deep record on min/max pairs while raw reads of its screen window come (deepmeas.rs): the
            // measurement stays that of the last raw record
            let shown = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| &*r.frame);
            let hold = crate::deepmeas::hold(&self.m, shown) || crate::compress::hold(&self.m, shown);
            if got > 0 {
                let p0 = std::time::Instant::now();
                if hold {
                    // the measurement shown stays that of the last raw deep record, or of this step's
                    if let Some(r) = raw_rec.take() {
                        if self.meas_bg.is_some() {
                            self.meas_want = true;
                            self.meas_pending = Some(r);
                        } else {
                            self.measure_rec(&r);
                            self.meas_new = true;
                        }
                    }
                } else if self.meas_bg.is_some() {
                    // the result for the last record is still good when it comes after this one: `meas_n` only
                    // changes where a result out becomes wrong (a clear, measuring here at once)
                    self.meas_want = true;
                    // raw deep reads: this raw record is the one to measure, even when pair records (held) follow
                    // before the worker is free; measuring the newest then measured a pair record (Mean NaN live)
                    // (only a raw deep record: a clone of every record cost the screen rate, 26 instead of 27 frames a
                    // second with GPU and HDMI Layout, unit 2026-11-15)
                    let raw_deep = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).is_some_and(|r| crate::deepmeas::deep_raw_frame(&r.frame));
                    if raw_deep && crate::deepmeas::want(&self.m).is_some() {
                        self.meas_pending = self.last[0].clone();
                    }
                } else {
                    self.measure_last();
                    self.meas_new = true;
                }
                let p1 = std::time::Instant::now();
                self.analyzer_update();
                self.monitor_analyzer_update();
                self.protocol_watch();
                self.bode_update();
                let p2 = std::time::Instant::now();
                self.prof_add(0, p1 - p0);
                self.prof_add(1, p2 - p1);
            }
        } else if !self.record_mode() {
            // stopped: the whole stored record for the view shown (read again by the thread, `stop_replay`), then
            // redraw the stored record with the current scale/offset
            self.stop_replay();
            self.ph.clear();
            self.phz.clear();
            let mainv = (self.m.time_per_div(), self.m.delay);
            let zv = (tscales()[self.m.zscale], self.m.zdelay);
            let zoom_model = self.zoom_display_model();
            let detail = self.captured_view(zv);
            let sparse = self.zoom_sparse(detail.as_deref().map_or_else(|| self.last[0].as_ref().map_or(1.0, |r| r.dt), |d| d.dt));
            self.phz.fat = sparse;
            let shown = self.stop_picture().cloned().or_else(|| self.last[0].clone());
            if let Some(rec) = shown {
                let zw = self.z_weights(&rec);
                for c in 0..NCH {
                    if self.m.ch[c].on && !rec.ch[c].is_empty() && self.m.z_src != Some(c) {
                        let shifted_main = self.deskew_channel_view(&rec, c, mainv);
                        let main = shifted_main.as_deref().unwrap_or(&rec);
                        self.ph.draw_ex(c, &self.m, mainv, main.t0 - self.m.deskew_ch[c], main.dt, &main.ch[c], 24 * 6, if shifted_main.is_some() { None } else { zw.as_deref() }, main.pairs);
                        if self.m.zoom && !self.studio_zoom_covers(c) {
                            self.phz.draw_ex(c, &self.m, mainv, main.t0 - self.m.deskew_ch[c], main.dt, &main.ch[c], 24 * 6, if shifted_main.is_some() { None } else { zw.as_deref() }, main.pairs);
                        } else if self.m.zoom {
                            let shifted = self.deskew_channel_view(&rec, c, zv);
                            let z = shifted.as_deref().or(detail.as_deref()).unwrap_or(&rec);
                            self.phz.draw_ex(c, &zoom_model, zv, z.t0 - self.m.deskew_ch[c], z.dt, &z.ch[c], 24 * 6, if shifted.is_some() || detail.is_some() { None } else { zw.as_deref() }, z.pairs);
                        }
                    }
                }
            }
        }
        if self.analyzed_view != self.m.view {
            self.analyzed_view = self.m.view;
            // Studio opens the eye beside the plot: the waveform stays; Full is an explicit choice
            if self.m.view == AView::Eye && self.m.layout == Layout::Studio && self.m.eye_place == EyePlace::Full && !self.studio.eye_full {
                self.eye_set_place(EyePlace::Side);
            }
            if self.m.view != AView::Scope { self.analyzer_update(); }
        }
        // the measurements of the newest record (in the background: when they are done, also after a stop)
        self.meas_poll();
        if std::mem::take(&mut self.meas_new) {
            let rows = self.meas_rows();
            self.trend_update(self.mstat.is_empty(), &rows);
            self.mstat.resize(rows.len(), MStat::default());
            self.counter_update();
            for (i, row) in rows.iter().enumerate() {
                if row.computing {
                    if let Some(entry) = i.checked_sub(self.m.meas.len() + self.m.meas2.len()).and_then(|i| self.m.cat.get(i)).copied() {
                        self.queue_catalog_statistics(entry, self.wfm_total);
                    }
                }
                let v = row.v;
                if v.is_finite() {
                    let current = self.m.meas_pop_current;
                    self.mstat[i].push(v, current);
                }
            }
        }
        // the final frame was taken (or did not come in time): stopped
        if self.stop_final.is_some_and(|t| final_done || self.now > t) {
            self.stop_final = None;
        }
        if self.m.run == RunState::Stop {
            if !self.record_mode() { self.m.status = TrigStatus::Stop; }
            self.wfm_rate = 0.0;
        }
        let p3 = std::time::Instant::now();
        self.pane_tick();
        let p4 = std::time::Instant::now();
        self.prof_add(2, p4 - p3);
        let p5 = std::time::Instant::now();
        // brightest pixel per channel: the trace brightness is graded relative to it
        // (follow upwards at once, downwards slowly: no flicker)
        let (full, fullz) = self.grade_refs();
        self.prof_add(3, p5.elapsed());
        // long or infinite persistence: halve the maps before the counts saturate (keeps the density ratios)
        if full {
            self.ph.halve();
            self.vmax.iter_mut().for_each(|v| *v = (*v / 2).max(1));
            self.vmax_cg = (self.vmax_cg / 2).max(1);
        }
        if fullz {
            self.phz.halve();
            self.vmaxz.iter_mut().for_each(|v| *v = (*v / 2).max(1));
            self.vmaxz_cg = (self.vmaxz_cg / 2).max(1);
        }
        if let Some((_, t)) = &self.toast {
            if self.now > *t {
                self.toast = None;
            }
        }
    }

    /// One line of settings for the HDMI workspace: channels, time base, trigger, state, rate.
    pub fn hdmi_header(&self) -> String {
        let m = &self.m;
        let mut parts: Vec<String> = (0..NCH).filter(|&c| m.ch[c].on).map(|c| format!("CH{} {}/div", c + 1, si_short(m.ch[c].volts_per_div() as f64, "V"))).collect();
        parts.push(format!("{}/div", si_short(m.time_per_div() as f64, "s")));
        if self.offline {
            let (name, sub) = self.ref_card_lines();
            parts.push(if sub == "empty" || sub.ends_with("pts") { name } else { format!("{} {}", name, sub) });
            parts.push(self.run_caption().0.into());
        } else {
            parts.push(match m.trig_line() {
                Some(n) => format!("Trig D{}", n),
                None => format!("Trig {} {}", self.trigger_source_label(), si_short(self.trig_level_shown() as f64, "V")),
            });
            let st = match m.status {
                TrigStatus::Triggered => "TRIG'D",
                TrigStatus::Auto => "AUTO",
                TrigStatus::Wait => "WAIT",
                TrigStatus::Stop => "STOP",
            };
            parts.push(st.into());
        }
        parts.push(format!("{:.0} wfm/s", self.wfm_rate));
        parts.join("   \u{b7}   ")
    }

    /// Frames the fast path takes in one screen: as many as fit into `fast_budget` at the pace of the last screens.
    /// Beyond that the frame rate would drop (and with it the time the newest frame waits to be seen).
    fn fast_max(&self) -> usize {
        (self.fast_take as usize).clamp(4, crate::board::QUEUE_MAX)
    }

    /// Time of a screen the fast path may spend (ms): what a frame of the 30 a second (33 ms) leaves after the rest of
    /// the step, drawing and showing the screen, less 2 ms of slack; 5 to 28 ms.
    fn fast_budget(&self) -> f32 {
        let rest = (self.t_step - self.t_fast).max(0.0) + self.t_render + self.t_present;
        (31.0 - rest).clamp(5.0, 28.0)
    }

    /// STOP on the hardware: the thread acquires nothing and reads the stored record again for every view (time base,
    /// delay), as much of it as the view shows: zoomed out the whole record, zoomed in every sample of the part on
    /// screen. The trigger stays where the last record had it (software trigger included).
    fn stop_replay(&mut self) {
        let ok = self.src.is_hw()
            && board().slow_mode(&self.m) == crate::board::Slow::Off
            && !self.m.la_on()
            && self.m.segments == 0
            && self.hist_pos.is_none()
            && self.last[0].as_ref().is_some_and(|r| r.raw.is_some());
        if !ok {
            self.src.hw_stop(None);
            self.stop_ti_off = None;
            (self.stop_raw, self.stop_pic) = (None, None);
            return;
        }
        // the software trigger of the record shown at the stop, as a time from the hardware trigger point
        if self.stop_ti_off.is_none() {
            // and that record itself when it is of samples: measured while the views come as pairs
            self.stop_raw = self.last[0].clone().filter(|r| r.raw.as_ref().is_some_and(|x| !x.frame.envelope));
            self.stop_pic = None;
            self.stop_ti_off = self.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|raw| {
                let f = &raw.frame;
                let ti = raw.i0 as f64 - raw.t0 / f.dt;
                (ti - f.fixed_ti.unwrap_or(f.pre as f64)) * f.dt
            });
        }
        let view = (self.m.time_per_div() as f64, self.m.delay as f64);
        let Some(mut f) = self.src.hw_stop(Some(view)) else { return };
        if let (Some(t), Some(off)) = (f.fixed_ti, self.stop_ti_off) {
            f.fixed_ti = Some(t + off / f.dt);
        }
        let window = view.0 * HDIV as f64;
        let rec = crate::board::to_record(&std::sync::Arc::new(f), &self.m, 3600, window, view.1 - window / 2.0);
        // a view read as min/max pairs (zoomed out on a record above `LIVE_READ`: the whole record compressed) after a
        // STOP on a raw record (the final frame of a deep record): the pairs are drawn, the raw record stays the one
        // measured and read out; pairs hold no Mean, RMS or Std dev (A3 of the 2026-11-16 review). A view read raw (far
        // zoomed in) is the record shown and measured, as before
        match self.stop_raw.as_ref().filter(|_| rec.raw.as_ref().is_some_and(|r| r.frame.envelope)) {
            Some(raw) => (self.last[0], self.stop_pic) = (Some(raw.clone()), Some(rec)),
            None => (self.last[0], self.stop_pic) = (Some(rec), None),
        }
        // the record counted when it came in RUN: its values for the view, not a new one for the statistics, the trend
        // and the catalog (as `refresh_stopped_measurements`)
        self.measure_last();
    }

    /// STOP: the pair record to draw for the view (`stop_pic`) while the raw record of the stop is the record shown.
    fn stop_picture(&self) -> Option<&Record> {
        let frame = |r: Option<&Record>| r.and_then(|r| r.raw.as_ref()).map(|r| std::sync::Arc::as_ptr(&r.frame));
        let same = frame(self.last[0].as_ref()).is_some_and(|f| Some(f) == frame(self.stop_raw.as_ref()));
        self.stop_pic.as_ref().filter(|_| same)
    }

    /// The fast display path applies: hardware frames with a plain edge trigger and nothing that needs every record
    /// as a full record (averaging, a software mask test, zones, segments, zoom, XY, Z input, software coupling or filter).
    fn fast_ok(&self) -> bool {
        let m = &self.m;
        matches!(self.src, Source::Hw { .. })
            && m.trig_type == TrigType::Edge
            && m.acq == AcqMode::Normal
            && m.trig_mode != TrigMode::Single
            && m.segments == 0
            && (!m.mask_on || self.mask_hw_active())
            && !m.zoom
            && !self.xy_over()
            && m.z_src.is_none()
            && (!m.zones.iter().any(|z| z.usable()) || self.zone_hw_active())
            && board().slow_mode(m) == crate::board::Slow::Off
            && (0..NCH).all(|c| !m.ch[c].on || (matches!(m.ch[c].coupling, Coupling::Dc | Coupling::Dc50) && m.ch[c].bw != BW_20M))
    }

    /// The fast path for the queued frames (every one but the newest): placed on several cores (trigger, columns or
    /// screen record, statistics of each frame), then the maps filled in bands, channel by band on several cores.
    /// Returns the frames that counted as triggered.
    fn fast_draw_all(&mut self, frames: Vec<crate::board::Frame>) -> usize {
        let frames: Vec<std::sync::Arc<crate::board::Frame>> = frames.into_iter().map(std::sync::Arc::new).collect();
        let (w, h) = (self.ph.w, self.ph.h);
        let m = &self.m;
        // the statistics of every frame for the voltage measurements, once the measurements of the record have set
        // up their rows
        let rows_ready = self.mstat.len() == m.meas.len() + m.meas2.len();
        let fold: [bool; NCH] = std::array::from_fn(|c| rows_ready && m.ch[c].on && m.meas.iter().any(|&(k, mc)| mc == WaveId::Ch(c) && crate::fastdraw::Fold::covers(k)));
        let t0 = std::time::Instant::now();
        let placed = crate::fastdraw::par_map(&frames, |f| fast_plan(m, w, h, f, fold));
        let t1 = std::time::Instant::now();
        let mut trig = 0;
        for (p, f) in placed.iter().zip(&frames) {
            trig += p.triggered as usize;
            if let FastPlan::Raw { class: Some(k), off, .. } = p.plan {
                self.fast_edges[k] += 1;
                if k == 2 {
                    self.fast_off = (self.fast_off.0 + off, self.fast_off.1.max(off));
                }
            }
            // as the deposit workers (`framestats::fold_frame`): nothing of live-compression pairs (the raw newest
            // frame of the batch counts), of other min/max pairs only what pairs give (Max, Min, Pk-pk)
            if rows_ready && !crate::compress::live(f) {
                for (i, &(k, src)) in self.m.meas.iter().enumerate() {
                    let Some(c) = src.ch() else { continue };
                    if f.envelope && crate::framestats::needs_samples(k) {
                        continue;
                    }
                    if let Some(v) = p.fold[c].and_then(|f| f.get(k)).filter(|v| v.is_finite()) {
                        self.mstat[i].add(v);
                    }
                }
            }
        }
        // the maps: each channel's frames collected as differences (`fastdraw::diff_span`), in bands of columns so one
        // channel alone also spreads over the cores, then summed into the map once
        let m = &self.m;
        let on: Vec<usize> = (0..NCH).filter(|&c| m.ch[c].on).collect();
        for &c in &on {
            if self.acc_t[c].len() != w * (h + 1) {
                self.acc_t[c] = vec![0; w * (h + 1)];
            }
        }
        let bands = if frames.len() < 2 || on.is_empty() { 1 } else { (crate::fastdraw::helpers() + 1).div_ceil(on.len()) };
        let band_cols = w.div_ceil(bands).max(1);
        let mut jobs: Vec<(usize, usize, &mut [u16])> = Vec::new();
        for (c, acc) in self.acc_t.iter_mut().enumerate() {
            if on.contains(&c) {
                jobs.extend(acc.chunks_mut(band_cols * (h + 1)).enumerate().map(|(b, band)| (c, b * band_cols, band)));
            }
        }
        crate::fastdraw::par_each(jobs, |(c, c_lo, band)| {
            for (f, p) in frames.iter().zip(&placed) {
                fast_deposit_t(m, w, h, c, band, c_lo, f, &p.plan);
            }
        });
        // the rows the frames touched, per channel: only those are added (a trace covers a part of the height)
        let rows: [(usize, usize); NCH] = std::array::from_fn(|c| {
            placed.iter().fold((usize::MAX, 0), |(a, b), p| {
                let (x, y) = match &p.plan {
                    FastPlan::Raw { spans, .. } => spans[c].as_ref().map_or((0, h), crate::fastdraw::span_rows),
                    // a screen record: its points' rows (a segment between two points stays between them)
                    FastPlan::Rec(rec) => rec_rows(m, h, c, &rec.ch[c]),
                };
                (a.min(x), b.max(y))
            })
        });
        let t2 = std::time::Instant::now();
        let pairs: Vec<(usize, &mut Vec<u16>, &mut Vec<u16>)> = self.ph.map.iter_mut().zip(self.acc_t.iter_mut()).enumerate().filter(|(c, _)| on.contains(c)).map(|(c, (a, b))| (c, a, b)).collect();
        crate::fastdraw::par_each(pairs, |(c, map, acc)| crate::fastdraw::fold_t(map, acc, w, h, band_cols, rows[c].0..rows[c].1));
        let (a, b) = ((t1 - t0).as_secs_f32() * 1e3, t1.elapsed().as_secs_f32() * 1e3);
        self.t_fast_ab.0 += 0.05 * (a - self.t_fast_ab.0);
        self.t_fast_ab.1 += 0.05 * (b - self.t_fast_ab.1);
        self.t_fast_fold += 0.05 * (t2.elapsed().as_secs_f32() * 1e3 - self.t_fast_fold);
        self.fast_n += 0.05 * (frames.len() as f32 - self.fast_n);
        trig
    }

    /// One frame of the fast path into the main phosphor map; false if its trigger edge was not found near the
    /// hardware trigger point. Sparse frames (fewer than 2 samples per column) take the record path instead.
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    fn fast_draw(&mut self, f: &crate::board::Frame) -> bool {
        let stats = crate::drawpipe::Stats { edges: &mut self.fast_edges, off: &mut self.fast_off };
        crate::drawpipe::draw_frame(&mut self.ph, &self.m, f, Some(stats))
    }

    // ------------------------------------------------------------------ rendering

    pub fn render(&mut self, cv: &mut Canvas) {
        self.render_layers(&mut Layers { under: cv, over: None, jobs: Vec::new() });
    }

    /// The screen into `l`: with one canvas everything is drawn on it; with two (System > Graphics = GPU) the
    /// background, graticule and masks go under the traces, everything else over them, and the traces become jobs
    /// for the GPU (`gpu.rs`). The order of the drawing is the same either way.
    pub fn render_layers(&mut self, l: &mut Layers) {
        self.studio_frame_start();
        self.relayout();
        if let Some(o) = l.over.as_deref_mut() {
            o.no_clip();
            o.clear(0);
        }
        if self.start_up_active() {
            self.draw_start_up(l.under);
            // the power dialog over it (the power key works during the start-up)
            self.draw_overlay(l.over());
            return;
        }
        l.under.no_clip();
        l.under.clear(BG);
        let g = self.grid;
        if self.m.zoom && !self.xy_over() {
            let (o, _) = self.split_zoom(g);
            self.draw_grid(l.under, o);
            self.traces(l, o, false, true);
            self.draw_refs_at(l.over(), o, (self.m.time_per_div(), self.m.delay), true);
            self.draw_studio_overview_math(l.over(), o);
            // zoom window in the overview: dim the rest, frame the window
            let (xa, xb) = self.zoom_window_x(o);
            let cv = l.over();
            cv.set_clip(o);
            cv.blend(Rect::new(o.x, o.y, (xa - o.x).max(0), o.h), BG, 170);
            cv.blend(Rect::new(xb, o.y, (o.x + o.w - xb).max(0), o.h), BG, 170);
            if self.m.layout == Layout::Studio {
                self.draw_studio_window(cv, o, xa, xb);
                self.draw_studio_overview_cursors(cv, o);
            } else {
                cv.rect(Rect::new(xa, o.y, (xb - xa).max(2), o.h), ACCENT);
                for edge in [xa, xb] {
                    cv.round(Rect::new(edge - 3, o.y + o.h / 2 - 12, 7, 24), 3, ACCENT, 255);
                }
            }
            cv.no_clip();
            for &lane in &self.wins { self.draw_grid(l.under, lane); }
            self.draw_lane_chrome(l.under);
        } else {
            for &w in &self.wins {
                self.draw_grid(l.under, w);
            }
            self.draw_lane_chrome(l.under);
        }
        if self.view_on() {
            // the analysers and XY draw their own picture: under what goes over it
            if self.xy {
                self.draw_xy(l.under);
            } else {
                self.draw_analyzer(l.under);
            }
            self.draw_view_btn(l.over());
        } else {
            // Every detail lane draws its own sources. The shared zoom overview
            // was drawn above; detail maps use the same lane dimensions.
            for i in 0..self.wins.len() {
                self.cur_win = i;
                self.area = self.wins[i];
                if !self.m.zoom {
                    self.draw_mask(l.under);
                    self.traces(l, self.area, false, false);
                } else {
                    self.traces(l, self.area, true, false);
                }
                let cv = l.over();
                if !self.dock_on() { self.draw_hist(cv); }
                self.draw_zones(cv);
                if self.wins.len() > 1 || i == 0 || self.stacked() {
                    if self.waves_on() {
                        self.draw_refs_at(cv, self.waves_plot(self.area), self.view(), false);
                    } else { self.draw_refs(cv); }
                }
                self.draw_math(cv);
                if i == self.la_win_of() {
                    self.draw_la(cv);
                }
                self.draw_decode(cv);
                if i == 0 {
                    self.draw_bus(cv);
                }
                self.draw_meas_labels(cv);
                self.draw_cursors(cv);
                self.draw_search(cv);
                self.draw_bus_event(cv);
                self.draw_markers(cv);
                self.draw_axes(cv);
                self.draw_mask_pill(cv);
                if i == 0 {
                    self.draw_waves_board(cv);
                }
            }
            self.cur_win = 0;
            self.area = self.wave_area();
        }
        let cv = l.over();
        if self.m.zoom && !self.xy_over() && !self.view_on() {
            let (tz, dz) = self.view();
            if self.m.layout != Layout::Studio { self.draw_zoom_readout(cv, tz, dz); } else { self.draw_zoom_tab(cv); }
        }
        if self.eye_open() {
            // eye_pane_rect maps Full to the plot while retaining floating panes.
            self.draw_eye_pane(cv);
        }
        self.draw_cal_banner(cv);
        self.draw_info(cv);
        self.draw_studio_overlay(cv);
        self.draw_studio_zoom_overlay(cv);
        self.draw_zoom_box(cv);
        self.draw_full_btn(cv);
        // the zoom magnifier belongs to the scope picture: an analyser page has its own controls under it
        if !(self.eye_open() && self.m.eye_place==EyePlace::Full && self.workspace.eye_float.is_none()) && !self.view_on() {
            self.draw_zoom_btn(cv);
        }
        self.draw_auto_bar(cv);
        self.draw_strip(cv);
        // Full on the external display is the plot and the strip. The statistics card and a lower pane stay on the panel.
        if self.hdmi_snap_want && self.m.density == Density::Full {
            self.capture_hdmi(l);
        }
        let cv = l.over();
        if self.pane_h() > 0 {
            cv.set_view_clip(self.dock_on().then(|| self.dock_rect(dock::VKind::Pane)).flatten());
            self.draw_pane(cv);
            // in a dock the view's title strip has the close button
            if !self.dock_on() { self.draw_pane_close(cv); }
            cv.set_view_clip(None);
        }
        self.draw_results(cv);
        self.draw_studio_panel(cv);
        cv.set_view_clip(self.dock_on().then(|| self.dock_rect(dock::VKind::Table)).flatten());
        self.draw_table(cv);
        cv.set_view_clip(None);
        self.draw_view_tiles(cv);
        self.draw_dock(cv);
        self.draw_md_overlays(cv);
        self.draw_axis_highlight(cv);
        self.draw_full_chrome(cv);
        // waveform on the external display: the band between the bars, before the bars and the menus go over it
        // (the GPU puts it together from the layers itself)
        if self.hdmi_snap_want && self.m.density != Density::Full {
            self.capture_hdmi(l);
        }
        let cv = l.over();
        if !self.full_on() {
            self.draw_top(cv);
            self.draw_bottom(cv);
        }
        self.draw_menu(cv);
        self.draw_info_card(cv);
        self.draw_toast(cv);
        self.draw_overlay(cv);
        self.workspace_draw(cv);
        self.draw_studio_floating(cv);
        self.draw_studio_view_menu(cv);
        self.draw_studio_add_menu(cv);
        self.draw_studio_ctx(cv);
        self.frames += 1;
    }

    /// The traces of a plot window: drawn now on one canvas, a job for the GPU with layers. `zoom`: the zoom
    /// window's maps.
    /// `all_sources`: every channel that is on (the zoom overview), else only those of the window being drawn.
    fn traces(&self, l: &mut Layers, area: Rect, zoom: bool, all_sources: bool) {
        let (ph, vmax, vmax_cg) = if zoom { (&self.phz, &self.vmaxz, self.vmaxz_cg) } else { (&self.ph, &self.vmax, self.vmax_cg) };
        if l.over.is_none() {
            return self.draw_traces(l.under, area, ph, vmax, vmax_cg, all_sources);
        }
        let show: [bool; NCH] = std::array::from_fn(|c| self.m.ch[c].on && (all_sources || self.shows(c)));
        if !show.iter().any(|&s| s) {
            return;
        }
        let graded = self.m.color_grade;
        let (mut alpha, mut colours, mut shift) = (Vec::new(), Vec::new(), [0u32; NCH]);
        if graded {
            let (lut, sh) = self.grade_lut(vmax_cg);
            colours = lut.to_vec();
            shift[0] = sh;
        } else {
            alpha = vec![0u8; NCH * 1024];
            for c in 0..NCH {
                let (lut, sh, least) = self.trace_lut(vmax[c]);
                for (i, a) in alpha[c * 1024..(c + 1) * 1024].iter_mut().enumerate() {
                    *a = lut[i].max(least) as u8;
                }
                shift[c] = sh;
            }
        }
        l.jobs.push(TraceJob { area, zoom, graded, show, alpha, colours, shift, col: CH_COLORS });
    }

    /// The screen once more, laid out for the size of `cv` (Display > HDMI = Layout): the same settings, menus and
    /// records, sharp at the monitor's resolution. The traces come from the panel's phosphor maps, stretched to the
    /// larger plot (`draw_traces`). The panel's layout is as it was afterwards.
    pub fn render_view(&mut self, cv: &mut Canvas) {
        self.render_view_layers(&mut Layers { under: cv, over: None, jobs: Vec::new() });
    }

    /// `render_view` into layers (System > Graphics = GPU): the traces become jobs at the monitor's size.
    pub fn render_view_layers(&mut self, l: &mut Layers) {
        // Display > Monitor view: another view than the panel's
        if self.monitor_view().is_some() {
            self.with_monitor_view(|u| u.render_view_layers_as_is(l));
            return;
        }
        self.render_view_layers_as_is(l);
    }

    /// `render_view_layers` of the view in place.
    fn render_view_layers_as_is(&mut self, l: &mut Layers) {
        let keep = (self.scr, self.grid, std::mem::take(&mut self.wins), self.area, self.cur_win, self.frames, self.hdmi_snap_want);
        self.scr = Screen { w: l.under.w, h: l.under.h };
        (self.view_pass, self.hdmi_snap_want) = (true, false);
        self.render_layers(l);
        self.view_pass = false;
        (self.scr, self.grid, self.wins, self.area, self.cur_win, self.frames, self.hdmi_snap_want) = keep;
    }

    /// Graticule in the classic scope style: dotted division lines (Full), a solid frame (Full, Light) and solid
    /// centre axes, with ticks every fifth of a division on the axes (both sides) and on the frame (inwards).
    fn draw_grid(&self, cv: &mut Canvas, g: Rect) {
        let dx = g.w as f32 / HDIV;
        let dy = g.h as f32 / VDIV;
        let (x1, y1) = (g.x + g.w - 1, g.y + g.h - 1);
        let xs = |k: i32, per: f32| (g.x + (k as f32 * dx / per).round() as i32).min(x1);
        let ys = |k: i32, per: f32| (g.y + (k as f32 * dy / per).round() as i32).min(y1);
        let cx = g.x + (g.w as f32 / 2.0).round() as i32;
        let cy = g.y + (g.h as f32 / 2.0).round() as i32;
        // Display > Grid brightness: towards the background
        let k = self.m.grid_bright.clamp(10, 100) * 255 / 100;
        let (dot, line) = (mix(BG, GRID_DOT, k), mix(BG, GRID_FRAME, k));
        if self.m.grid == Grid::Full {
            // dots about every 5 px along each division line (a whole number of dots per division, so they line
            // up with the ticks)
            let (nx, ny) = ((dx / 5.0).round().max(2.0), (dy / 5.0).round().max(2.0));
            for i in 1..HDIV as i32 {
                let x = xs(i, 1.0);
                if x != cx {
                    for k in 0..=(VDIV * ny) as i32 {
                        cv.pixel(x, ys(k, ny), dot, 255);
                    }
                }
            }
            for j in 1..VDIV as i32 {
                let y = ys(j, 1.0);
                if y != cy {
                    for k in 0..=(HDIV * nx) as i32 {
                        cv.pixel(xs(k, nx), y, dot, 255);
                    }
                }
            }
        }
        if self.m.grid != Grid::None && self.m.grid_frame {
            // frame with inward ticks every fifth of a division
            cv.rect(Rect::new(g.x, g.y, g.w, g.h), line);
            for k in 1..(HDIV as i32 * 5) {
                let (x, len) = (xs(k, 5.0), if k % 5 == 0 { 8 } else { 4 });
                cv.vline(x, g.y, g.y + len, line);
                cv.vline(x, y1 - len, y1, line);
            }
            for k in 1..(VDIV as i32 * 5) {
                let (y, len) = (ys(k, 5.0), if k % 5 == 0 { 8 } else { 4 });
                cv.hline(g.x, g.x + len, y, line);
                cv.hline(x1 - len, x1, y, line);
            }
        }
        // centre axes with ticks on both sides every fifth of a division, longer at the divisions
        cv.vline(cx, g.y, y1, line);
        cv.hline(g.x, x1, cy, line);
        for k in 0..=(HDIV as i32 * 5) {
            let len = if k % 5 == 0 { 6 } else { 3 };
            cv.vline(xs(k, 5.0), cy - len, cy + len, line);
        }
        for k in 0..=(VDIV as i32 * 5) {
            let len = if k % 5 == 0 { 6 } else { 3 };
            cv.hline(cx - len, cx + len, ys(k, 5.0), line);
        }
    }

    /// (V/div, offset, unit, colour) of the ref that labels the volt axis when no channel is shown here: the selected
    /// ref, else the first one this window holds.
    fn ref_axis_source(&self) -> Option<(f32, f32, &'static str, Rgb)> {
        let here: Vec<usize> = self.shown_refs().into_iter().filter(|&i| self.workspace_source_visible_here(WaveId::Ref(i))).collect();
        let i = here.iter().copied().find(|&i| self.selected_workspace_source() == Some(WaveId::Ref(i))).or(here.first().copied())?;
        let r = self.refs[i].as_ref()?;
        Some((r.vpd, r.offset, "V", self.wave_color(WaveId::Ref(i))))
    }

    /// Stacked lanes: where one ends and the next begins, and whose it is. A bar in the source's colour runs down
    /// the left edge of each lane, and a solid line fills the gap between two lanes.
    fn draw_lane_chrome(&self, cv: &mut Canvas) {
        if !self.stacked() || self.wins.len() < 2 { return; }
        let sources = self.trace_sources();
        for (i, lane) in self.wins.iter().enumerate() {
            let col = match sources.get(i) {
                Some(workspace::Trace::Channel(c)) => CH_COLORS[*c],
                Some(workspace::Trace::Reference(r)) => self.wave_color(WaveId::Ref(*r)),
                Some(workspace::Trace::Math(k)) => MATH_COLORS[*k % MATH_COLORS.len()],
                None => continue,
            };
            cv.fill(Rect::new(lane.x, lane.y, 3, lane.h), mix(BG, col, 190));
            if let Some(next) = self.wins.get(i + 1) {
                let gap = (next.y - lane.bottom()).max(1);
                cv.fill(Rect::new(lane.x, lane.bottom(), lane.w, gap), mix(BG, TEXT, 90));
            }
        }
    }

    /// The ADC converts 7500 codes per division around the middle of the screen, so its 16-bit range ends 4.37
    /// divisions above and below it. A channel whose record reaches either end is clipping: what it shows there is
    /// the limit of the converter, not the signal, and every amplitude measured from it is too small.
    pub const CLIP_DIVS: f32 = 4.35;

    /// Channel `c` is on and its record reaches the end of the ADC range.
    pub fn channel_clipping(&self, c: usize) -> bool {
        let Some(ch) = self.m.ch.get(c).filter(|ch| ch.on) else { return false };
        if self.record_mode() { return false; }
        let a = &self.ana[c];
        if !a.valid { return false; }
        let vpd = ch.volts_per_div().max(1e-12) as f64;
        let (top, bottom) = ((a.vmax + ch.offset as f64) / vpd, (a.vmin + ch.offset as f64) / vpd);
        top >= Self::CLIP_DIVS as f64 || bottom <= -(Self::CLIP_DIVS as f64)
    }

    /// The channels that clip now, for the warning in the plot.
    pub fn clipping_channels(&self) -> Vec<usize> {
        (0..n_ch()).filter(|&c| self.channel_clipping(c)).collect()
    }

    /// The math channel whose lane holds the point (stacked lanes only).
    fn math_lane_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.stacked() { return None; }
        let lane = self.wins.iter().position(|w| w.contains(x, y))?;
        match self.trace_sources().get(lane) { Some(workspace::Trace::Math(k)) if self.m.math_ch(*k).op != MathOp::Fft => Some(*k), _ => None }
    }

    /// The scale of the math channel drawn in this lane: volts per division, offset in volts, unit, colour.
    fn math_axis_source(&self) -> Option<(f32, f32, &'static str, Rgb)> {
        let k = (0..NMATH).find(|&k| self.m.math_shown(k) && self.math_win_of(k) == self.cur_win)?;
        let c = self.m.math_ch(k);
        if c.op == MathOp::Fft { return None; }
        let vpd = *math_scales().get(c.scale)? as f32;
        Some((vpd, c.offset * vpd, c.op.unit(), MATH_COLORS[k % MATH_COLORS.len()]))
    }

    /// Axis labels inside the plot: volts of the active channel along the left edge (not on the top and bottom
    /// lines, where the status line and the time labels are), times along the bottom edge. A volt label is left out
    /// rather than drawn over another label, the status line, the time labels, the mask pill or the decode band; it
    /// keeps clear of the channel markers at the left edge.
    fn draw_axes(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let a = self.area;
        let c = if self.m.ch[self.m.active].on && self.shows(self.m.active) { Some(self.m.active) } else { (0..NCH).find(|&i| self.m.ch[i].on && self.shows(i)) };
        // with no channel to label, the shown ref gives the axis (R1 in the lane or window that holds it)
        let axis = match c {
            Some(c) => { let ch = &self.m.ch[c]; Some((ch.volts_per_div(), ch.offset, ch.unit.name(), CH_COLORS[c])) }
            None => self.ref_axis_source().or_else(|| if self.stacked() { self.math_axis_source() } else { None }),
        };
        // a lane is one source with its own scale: its labels are not given up for a status line or time labels
        // that are not in it, and its top and bottom values are always there
        let lanes = self.stacked() && self.wins.len() > 1;
        let last_lane = self.cur_win + 1 >= self.wins.len() || self.studio_lane_has_own_time();
        if let Some((base_vpd, base_offset, unit, color)) = axis {
            let (factor, center) = self.zoom_vertical_transform(a);
            let vpd = base_vpd / factor;
            let offset = base_offset - center * base_vpd;
            let span = (VDIV / 2.0 * vpd).abs() + offset.abs();
            let mut taken: Vec<Rect> = Vec::new();
            if !lanes || (self.cur_win == 0 && self.m.layout != Layout::Studio) { taken.push(Rect::new(a.x, a.y, a.w, 20)); }
            if !lanes || last_lane { taken.push(Rect::new(a.x, a.y + a.h - 22, a.w, 22)); }
            if self.m.mask_on && self.mask.is_some() && self.shows(self.m.mask_src) {
                taken.push(Rect::new(a.x, a.y + 22, a.w, 28));
            }
            let bands = (0..DECODERS).filter(|&k| self.m.dec_is_on(k) && self.dec_band_win(k) == Some(self.cur_win)).count() as i32;
            if bands > 0 {
                taken.push(Rect::new(a.x, a.y + a.h - 50 - (bands - 1) * DEC_BAND, a.w, bands * DEC_BAND));
            }
            // the channel ground markers at the left edge: a label on their line is left out (as on the stock
            // screen, where the marker takes the place of the "0" label)
            for k in 0..NCH {
                if self.m.ch[k].on && self.shows(k) {
                    let y = self.y_of_volts(k, 0.0).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
                    taken.push(Rect::new(a.x, y - 9, MARK_W + 2, 18));
                }
            }
            // what a lane has at its right edge: the corner buttons with their words, and the trigger tag with its value
            let mut right_obstacles: Vec<Rect> = Vec::new();
            if lanes {
                if self.m.layout == Layout::Studio {
                    for b in [self.full_btn().0, self.zoom_btn().0] { right_obstacles.push(Rect::new(b.x - 62, b.y - 2, b.w + 66, b.h + 4)); }
                }
                let ts = self.m.trig_src;
                if self.m.trig_ref.is_none() && self.m.ch[ts].on && self.shows(ts) {
                    let y = self.y_of_volts(ts, self.trig_level_shown()).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
                    let x1 = self.trigger_tag_x(a.x + a.w - MARK_W, y);
                    let left = x1 - 8 - f.sb.width(&si_short(self.trig_level_shown() as f64, self.m.unit(ts)));
                    right_obstacles.push(Rect::new(left, y - 11, a.x + a.w - left, 22));
                }
            }
            // labels right at the left edge of the graticule, centred on their division line
            let x = a.x + 6;
            // a lane: the values of its top and bottom edge first (its full scale), then the centre, the quarters and
            // the rest for as long as they fit between them
            let order: Vec<i32> = if lanes { vec![0, VDIV as i32, 4, 2, 6, 1, 3, 5, 7] } else { (1..VDIV as i32).collect() };
            for j in order {
                let line = a.y + (j as f32 * a.h as f32 / VDIV).round() as i32;
                // an edge label sits just inside its edge
                let y = if j == 0 { a.y + f.s.size - 2 } else if j == VDIV as i32 { a.y + a.h - 6 - if last_lane { 26 } else { 0 } } else { line };
                let v = (VDIV / 2.0 - j as f32) * vpd - offset;
                let s = axis_label(v, vpd, offset, span, unit);
                // an offset that is not on the steps of the scale (a fitted math channel) would spell every digit:
                // three significant ones say as much
                let s = if s.chars().count() > 9 { si_short(v as f64, unit) } else { s };
                let w = f.s.width(&s);
                // a lane has its scale at the right edge; what already sits there (the corner buttons, the trigger tag)
                // moves a label to its left instead of losing it
                let mut x = if lanes { a.x + a.w - 6 - w } else { x };
                let mut r = Rect::new(x - 3, y + 4 - f.s.size, w + 6, f.s.size + 4);
                if lanes {
                    for _ in 0..2 {
                        if let Some(o) = right_obstacles.iter().find(|o| overlaps(**o, r)) {
                            x = o.x - 6 - w;
                            r = Rect::new(x - 3, y + 4 - f.s.size, w + 6, f.s.size + 4);
                        }
                    }
                }
                if taken.iter().any(|t| overlaps(*t, r)) {
                    continue;
                }
                taken.push(r);
                backed_text(cv, &f.s, x, y + 4, &s, scale(color, 210));
            }
        }
        if self.stacked() && self.cur_win + 1 < self.wins.len() && !self.studio_lane_has_own_time() { return; }
        let (tdiv, delay) = self.view();
        let span = (HDIV / 2.0 * tdiv).abs() + delay.abs();
        // a narrow window (side by side) labels every second division, the centre always
        let every = if a.w < 700 { 2 } else { 1 };
        if self.m.layout == Layout::Studio {
            // round times at any delay (a zoom window sits where it is put): the labels are whole steps from the
            // trigger, not the window's centre plus whole divisions
            let mut step = tdiv as f64 * every as f64;
            // a lane with a time/div of its own (a fitted record) is not on the 1-2-5 ladder: its labels are
            if self.studio_lane_has_own_time() { step = nice_step_below(step); }
            let mut free = i32::MIN;
            for (k, t) in studio_time_ticks_at(tdiv, delay, step) {
                let x = a.x + (((t - delay as f64) / tdiv as f64 + HDIV as f64 / 2.0) * a.w as f64 / HDIV as f64).round() as i32;
                let zero = k == 0;
                let s = if zero { "0 s".to_string() } else { axis_label64(t, step as f32, 0.0, span, "s") };
                let w = f.s.width(&s);
                let tx = (x - w / 2).clamp(a.x + 6, a.x + a.w - 6 - w);
                // a label pushed against an edge must not run into its neighbour
                if tx < free + 8 { continue; }
                free = tx + w;
                backed_text(cv, &f.s, tx, a.y + a.h - 6, &s, if zero { ACCENT } else { AXIS_TEXT });
            }
            return;
        }
        let mut free = i32::MIN;
        for i in 0..=HDIV as i32 {
            if (i - HDIV as i32 / 2).rem_euclid(every) != 0 {
                continue;
            }
            let x = a.x + (i as f32 * a.w as f32 / HDIV).round() as i32;
            let t = delay as f64 + (i as f64 - HDIV as f64 / 2.0) * tdiv as f64;
            let zero = t.abs() < tdiv as f64 * 1e-3;
            let s = if zero { "0 s".to_string() } else { axis_label64(t, tdiv, delay, span, "s") };
            let w = f.s.width(&s);
            let tx = (x - w / 2).clamp(a.x + 6, a.x + a.w - 6 - w);
            if tx < free + 8 { continue; }
            free = tx + w;
            backed_text(cv, &f.s, tx, a.y + a.h - 6, &s, if zero { ACCENT } else { AXIS_TEXT });
        }
    }

    fn draw_traces(&self, cv: &mut Canvas, area: Rect, ph: &Phosphor, vmaxs: &[u32; NCH], vmax_cg: u32, all_sources: bool) {
        if self.m.color_grade {
            return self.draw_graded(cv, area, ph, vmax_cg, all_sources);
        }
        let (w, h) = (ph.w, ph.h);
        for c in (0..NCH).rev() {
            if !self.m.ch[c].on || (!all_sources && !self.shows(c)) {
                continue;
            }
            let col = CH_COLORS[c];
            let (cr, cg, cb) = ((col >> 16) & 255, (col >> 8) & 255, col & 255);
            let (lut, shift, least) = self.trace_lut(vmaxs[c]);
            let map = &ph.map[c];
            let put = |p: &mut u32, v: u16| {
                if v == 0 {
                    return;
                }
                let a = lut[((v as usize) >> shift).min(1023)].max(least);
                // additive with saturation, but channels on top of each other mix towards the newer one
                let d = *p;
                let r = (((d >> 16) & 255) * (255 - a) / 255 + cr * a / 255).min(255);
                let g = (((d >> 8) & 255) * (255 - a) / 255 + cg * a / 255).min(255);
                let b = ((d & 255) * (255 - a) / 255 + cb * a / 255).min(255);
                *p = rgb(r, g, b);
            };
            match Self::stretch(area, w, h) {
                None => {
                    for y in 0..h {
                        let row = &map[y * w..(y + 1) * w];
                        let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                        for (x, &v) in row.iter().enumerate() {
                            put(&mut cv.px[o + x], v);
                        }
                    }
                }
                Some((xs, ys)) => {
                    for (y, &sy) in ys.iter().enumerate() {
                        let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                        for (x, &sx) in xs.iter().enumerate() {
                            put(&mut cv.px[o + x], map[sy * w + sx]);
                        }
                    }
                }
            }
        }
    }

    /// One part of `step` into `t_parts` (`:SYSTem:STATe?`).
    fn prof_add(&mut self, i: usize, d: std::time::Duration) {
        self.t_parts[i] += 0.05 * (d.as_secs_f32() * 1e3 - self.t_parts[i]);
    }

    /// Brightness of a trace pixel by its count (`draw_traces`): floor + sqrt(hits / brightest), the intensity
    /// setting raises floor and gain. The count shifted right by the second value indexes the table; a pixel that
    /// was hit at all gets at least the third.
    /// The counts the picture is graded against (`picture_ref`), per channel and for colour grade; whether the main and
    /// the zoom map are near saturation.
    fn grade_refs(&mut self) -> (bool, bool) {
        let n = self.pic_frames;
        let (mut full, mut fullz) = (false, false);
        for c in 0..NCH {
            if self.m.ch[c].on {
                let mx = self.ph.map[c].iter().copied().max().unwrap_or(0) as u32;
                self.vmax[c] = picture_ref(self.vmax[c], &mut self.per_frame[c], mx, n);
                full |= mx > 0xc000;
                if self.m.zoom {
                    let mz = self.phz.map[c].iter().copied().max().unwrap_or(0) as u32;
                    self.vmaxz[c] = picture_ref(self.vmaxz[c], &mut self.per_framez[c], mz, n);
                    fullz |= mz > 0xc000;
                }
            }
        }
        if self.m.color_grade {
            let on: [bool; NCH] = std::array::from_fn(|c| self.m.ch[c].on);
            self.vmax_cg = picture_ref(self.vmax_cg, &mut self.per_frame_cg[0], self.ph.merged_max(on), n);
            if self.m.zoom {
                self.vmaxz_cg = picture_ref(self.vmaxz_cg, &mut self.per_frame_cg[1], self.phz.merged_max(on), n);
            }
        }
        (full, fullz)
    }

    fn trace_lut(&self, vmax: u32) -> ([u32; 1024], u32, u32) {
        let inten = self.m.intensity as f32 / 100.0;
        let floor = 30.0 + 140.0 * inten;
        let gain = 0.35 + 1.3 * inten;
        let vmax = vmax.max(1);
        let mut shift = 0;
        while (vmax >> shift) > 1023 {
            shift += 1;
        }
        let top = ((vmax >> shift).max(1)) as f32;
        let mut lut = [0u32; 1024];
        for (i, l) in lut.iter_mut().enumerate() {
            let v = (i as f32 + 0.5) / top;
            *l = if i == 0 && shift == 0 { 0 } else { (floor + (255.0 - floor) * (v * gain).min(1.0).sqrt()).min(255.0) as u32 };
        }
        (lut, shift, floor as u32 / 2)
    }

    /// Colour of a colour-graded pixel by the summed count (`draw_graded`), and the shift of the index.
    fn grade_lut(&self, vmax: u32) -> ([u32; 1024], u32) {
        let gain = 0.35 + 1.3 * self.m.intensity as f32 / 100.0;
        let vmax = vmax.max(1);
        let mut shift = 0;
        while (vmax >> shift) > 1023 {
            shift += 1;
        }
        let top = (vmax >> shift).max(1) as f32;
        let pal = heat_palette();
        let mut lut = [0u32; 1024];
        for (i, l) in lut.iter_mut().enumerate() {
            let v = ((i as f32 + 0.5) / top * gain).min(1.0).sqrt();
            *l = pal[((v * 255.0) as usize).min(255)];
        }
        (lut, shift)
    }

    /// A plot of another size than its phosphor map (the HDMI layout): for each of its columns and rows the cell of
    /// the map it shows (the nearest). None: the same size, one pixel per cell.
    fn stretch(area: Rect, w: usize, h: usize) -> Option<(Vec<usize>, Vec<usize>)> {
        let (aw, ah) = (area.w.max(0) as usize, area.h.max(0) as usize);
        if (aw, ah) == (w, h) || w == 0 || h == 0 {
            return None;
        }
        Some(((0..aw).map(|x| x * w / aw).collect(), (0..ah).map(|y| y * h / ah).collect()))
    }

    /// Colour-graded persistence: the hit maps of all enabled channels are added up and every pixel is coloured by its
    /// density relative to the densest pixel (blue = rare .. red = frequent). Per frame only a 1024-entry LUT is
    /// built; per pixel it is an add per channel and one table look-up.
    fn draw_graded(&self, cv: &mut Canvas, area: Rect, ph: &Phosphor, vmax: u32, all_sources: bool) {
        let maps: Vec<&[u16]> = (0..NCH).filter(|&c| self.m.ch[c].on && (all_sources || self.shows(c))).map(|c| ph.map[c].as_slice()).collect();
        if maps.is_empty() {
            return;
        }
        let (lut, shift) = self.grade_lut(vmax);
        let w = ph.w;
        if let Some((xs, ys)) = Self::stretch(area, w, ph.h) {
            for (y, &sy) in ys.iter().enumerate() {
                let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                for (x, &sx) in xs.iter().enumerate() {
                    let s: u32 = maps.iter().map(|m| m[sy * w + sx] as u32).sum();
                    if s != 0 {
                        cv.px[o + x] = lut[((s >> shift) as usize).min(1023)];
                    }
                }
            }
            return;
        }
        for y in 0..ph.h {
            let o = ((area.y + y as i32) * cv.w + area.x) as usize;
            let row = y * w;
            for (x, d) in cv.px[o..o + w].iter_mut().enumerate() {
                let s: u32 = maps.iter().map(|m| m[row + x] as u32).sum();
                if s != 0 {
                    *d = lut[((s >> shift) as usize).min(1023)];
                }
            }
        }
    }

    /// Mask test of one acquisition (live records only): counts passed / failed, stops on a failure if set.
    fn mask_test(&mut self, rec: &Record) {
        if !self.m.mask_on {
            return;
        }
        let Some(mask) = &self.mask else { return };
        let v = &rec.ch[self.m.mask_src];
        if v.is_empty() {
            return;
        }
        if mask.test(v).pass {
            self.mask_pass += 1;
        } else {
            self.mask_fail += 1;
            // with the hardware test the counters decide (hwmask.rs)
            if self.m.mask_stop && self.m.run == RunState::Run && !self.mask_hw_active() {
                self.m.run = RunState::Stop;
                self.toast("Mask failed: stopped");
            }
        }
    }

    /// (Re)builds the mask from the reference waveform with the tolerances of the menu (X in divisions of the 10-div
    /// window, Y in divisions of the source channel's current V/div).
    fn build_mask(&mut self) {
        let Some(r) = &self.mask_ref else { return };
        let tol_x = (self.m.mask_x / HDIV * r.len() as f32).round() as usize;
        let tol_y = self.m.mask_y * self.m.ch[self.m.mask_src].volts_per_div();
        let mut mask = oos_dsp::mask::build(r, tol_x, tol_y);
        mask.invert = self.m.mask_inv;
        self.mask = Some(mask);
        self.mask_cols_update();
    }

    /// Creates the mask from the source channel of the record shown and starts the test.
    fn create_mask(&mut self) {
        let c = self.m.mask_src;
        let Some(v) = self.last[0].as_ref().map(|r| r.ch[c].clone()).filter(|v| !v.is_empty()) else {
            return self.toast(&format!("CH{} has no data", c + 1));
        };
        self.mask_ref = Some(v);
        self.build_mask();
        self.m.mask_on = true;
        self.mask_counts_reset();
        self.toast(&format!("Mask created from CH{}", c + 1));
    }

    /// Creates the mask from the selected reference slot.
    fn create_mask_from_ref(&mut self) {
        let Some(r) = self.refs[self.ref_sel].as_ref().filter(|r| r.v.len() >= 2) else {
            return self.toast(&format!("R{} is empty", self.ref_sel + 1));
        };
        self.mask_ref = Some(r.v.as_ref().clone());
        self.m.mask_src = r.src.min(NCH - 1);
        self.build_mask();
        self.m.mask_on = true;
        self.mask_pass = 0;
        self.mask_fail = 0;
        self.toast(&format!("Mask created from R{}", self.ref_sel + 1));
    }

    /// Off plus the operations of the selected Math category.
    fn math_ops_in_family(&self) -> Vec<MathOp> {
        let mut v = vec![MathOp::Off];
        v.extend_from_slice(self.math_fam.ops());
        v
    }

    fn ff_save(&mut self) {
        let recs: Vec<Record> = if !self.history.is_empty() {
            self.history.iter().cloned().collect()
        } else if let Some(rec) = self.last[0].as_ref() {
            vec![rec.clone()]
        } else {
            return self.toast("No frames to save");
        };
        let p = crate::store::next_name(&self.save_dir(), "frame", "bin");
        let r = crate::store::write(&p, &crate::store::ff_bin(&recs, &self.m));
        self.toast_saved(&p, r);
    }

    fn ff_load(&mut self) {
        let p = match crate::store::latest_name(&self.save_dir(), "frame", "bin") {
            Some(p) => p,
            None => return self.toast("No frame-NNN.bin in the save folder"),
        };
        let data = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => return self.toast(&format!("Load failed: {}", e)),
        };
        match crate::store::parse_ff(&data) {
            Ok(recs) if !recs.is_empty() => {
                self.history = recs.into();
                let n = self.history.len();
                self.hist_pos = Some(n - 1);
                self.last[0] = Some(self.history[n - 1].clone());
                self.m.run = RunState::Stop;
                self.measure_last();
                self.toast(&format!("Loaded {} frames from {}", n, crate::store::file_name(&p)));
            }
            Ok(_) => self.toast("That file holds no frames"),
            Err(e) => self.toast(&format!("Load failed: {}", e)),
        }
    }

    /// Forbidden region of the mask (outside the envelope) tinted red, the envelope edges a little stronger.
    fn draw_mask(&self, cv: &mut Canvas) {
        let (Some(mask), true, false) = (&self.mask, self.m.mask_on, self.xy) else { return };
        if !self.shows(self.m.mask_src) {
            return;
        }
        let a = self.area;
        let ch = &self.m.ch[self.m.mask_src];
        let ppd = a.h as f32 / VDIV;
        let cy = a.y as f32 + a.h as f32 / 2.0;
        let y_of = |v: f32| -> i32 { (cy - (v + ch.offset) / ch.volts_per_div() * ppd).round().clamp(-1e6, 1e6) as i32 };
        let (top, bot) = (a.y, a.y + a.h);
        for x in 0..a.w {
            let (lo, hi) = mask.band(x as f64 / a.w as f64, (x + 1) as f64 / a.w as f64);
            let yh = y_of(hi).clamp(top, bot);
            let yl = (y_of(lo) + 1).clamp(top, bot);
            let px = a.x + x;
            if self.m.mask_inv {
                if yl > yh {
                    cv.blend(Rect::new(px, yh, 1, yl - yh), RED, 58);
                    cv.blend(Rect::new(px, yh, 1, 1), RED, 120);
                    cv.blend(Rect::new(px, yl - 1, 1, 1), RED, 120);
                }
            } else {
                if yh > top {
                    cv.blend(Rect::new(px, top, 1, yh - top), RED, 58);
                    cv.blend(Rect::new(px, yh - 1, 1, 1), RED, 120);
                }
                if yl < bot {
                    cv.blend(Rect::new(px, yl, 1, bot - yl), RED, 58);
                    cv.blend(Rect::new(px, yl, 1, 1), RED, 120);
                }
            }
        }
    }

    /// "Mask CH1 · Passed n · Failed m" pill at the top left of the plot, under the status line.
    fn draw_mask_pill(&self, cv: &mut Canvas) {
        if !self.m.mask_on || self.mask.is_none() || !self.shows(self.m.mask_src) {
            return;
        }
        let f = self.fonts();
        let c = self.m.mask_src;
        let (pass, fail) = self.mask_counts();
        let parts: [(String, Rgb); 6] = [
            (if self.mask_hw_active() { "Mask (HW) ".into() } else { "Mask ".into() }, TEXT),
            (format!("CH{}", c + 1), CH_COLORS[c]),
            ("  \u{b7}  Passed ".into(), DIM),
            (fmt_count(pass as f64), GREEN),
            ("  \u{b7}  Failed ".into(), DIM),
            (fmt_count(fail as f64), if fail > 0 { RED } else { TEXT }),
        ];
        let w: i32 = parts.iter().map(|(s, _)| f.sb.width(s)).sum::<i32>() + 24;
        let a = self.area;
        let r = Rect::new(a.x + 8, a.y + 24, w, 24);
        let col = if fail > 0 { RED } else { GREEN };
        cv.round(r, 12, mix(BAR, col, 110), 255);
        cv.round(r.inset(1), 11, mix(BAR, col, 26), 255);
        let mut x = r.x + 12;
        for (s, c) in &parts {
            x = cv.text(&f.sb, x, r.y + 17, s, *c);
        }
    }

    /// Z input: brightness 0..1 per sample from the Z channel's screen position (bottom = dark, top = full).
    fn z_weights(&self, rec: &Record) -> Option<Vec<f32>> {
        let z = self.m.z_src?;
        let v = &rec.ch[z];
        if v.is_empty() {
            return None;
        }
        let ch = &self.m.ch[z];
        let vpd = ch.volts_per_div();
        Some(v.iter().map(|&x| (((x + ch.offset) / vpd + VDIV / 2.0) / VDIV).clamp(0.0, 1.0)).collect())
    }

    /// Histogram source: the measurement source if it is on, else the first channel on.
    fn hist_src(&self) -> Option<usize> {
        if let Some(c) = self.hist_ch.filter(|&c| c < n_ch() && self.m.ch[c].on) {
            return Some(c);
        }
        if let WaveId::Ch(c) = self.meas_src {
            if c < n_ch() && self.m.ch[c].on {
                return Some(c);
            }
        }
        (0..n_ch()).find(|&c| self.m.ch[c].on)
    }

    /// Histogram bins from the phosphor map: rows (vertical) or columns within +-0.5 div of the trigger level
    /// (horizontal); with the mean and standard deviation in pixels.
    fn hist_bins(&self) -> Option<(Vec<u64>, f64, f64)> {
        let c = self.hist_src()?;
        let area = self.source_area(WaveId::Ch(c)).unwrap_or(self.wave_area());
        let phosphor = if self.m.zoom { &self.phz } else { &self.ph };
        let (w, h) = (phosphor.w, phosphor.h);
        let map = &phosphor.map[c];
        let bins: Vec<u64> = match self.m.hist {
            Hist::Off => return None,
            Hist::Vertical => (0..h).map(|r| map[r * w..(r + 1) * w].iter().map(|&x| x as u64).sum()).collect(),
            Hist::Horizontal => {
                let ch = &self.m.ch[c];
                let ppd = h as f32 / VDIV;
                let (factor, center) = self.zoom_vertical_transform(area);
                let yl = h as f32 / 2.0 - ((self.m.trig_level + ch.offset) / ch.volts_per_div() - center) * factor * ppd;
                let (r0, r1) = (((yl - ppd / 2.0).max(0.0)) as usize, ((yl + ppd / 2.0).max(0.0) as usize).min(h));
                (0..w).map(|x| (r0..r1).map(|r| map[r * w + x] as u64).sum()).collect()
            }
        };
        let n: u64 = bins.iter().sum();
        if n == 0 {
            return Some((bins, f64::NAN, f64::NAN));
        }
        let mean = bins.iter().enumerate().map(|(i, &b)| i as f64 * b as f64).sum::<f64>() / n as f64;
        let var = bins.iter().enumerate().map(|(i, &b)| (i as f64 - mean).powi(2) * b as f64).sum::<f64>() / n as f64;
        Some((bins, mean, var.sqrt()))
    }

    /// Studio: the views that are not the waveform (histogram, XY, analysers) each draw in the rectangle of their own
    /// view. What they draw is the same as in the other layouts, only in another rectangle.
    fn draw_view_tiles(&mut self, cv: &mut Canvas) {
        if !self.dock_on() { return; }
        use dock::VKind;
        let saved = self.area;
        for k in [VKind::Hist, VKind::Xy, VKind::View] {
            if !self.dock_shown(k) { continue; }
            let Some(r) = self.dock_content(k) else { continue };
            // whatever the view draws stays in its own rectangle, also where its code lifts the clip
            cv.set_view_clip(Some(r));
            cv.fill(r, BG);
            match k {
                VKind::Hist => self.draw_hist_in(cv, r, true),
                VKind::Xy => { self.area = r; self.draw_xy(cv); self.draw_view_btn(cv); }
                _ => { self.area = r; self.draw_analyzer(cv); self.draw_view_btn(cv); }
            }
            cv.set_view_clip(None);
        }
        self.area = saved;
    }

    fn draw_hist(&self, cv: &mut Canvas) { self.draw_hist_in(cv, self.area, false) }

    /// The axes of the histogram view: volts at the left on the division lines of the lane, a count scale along the top.
    fn draw_hist_axes(&self, cv: &mut Canvas, a: Rect, (ax, ly, lh): (i32, i32, i32), c: usize, peak: u64) {
        let f = self.fonts();
        let ch = &self.m.ch[c];
        let lane = self.source_area(WaveId::Ch(c)).unwrap_or(a);
        let (factor, center) = self.zoom_vertical_transform(lane);
        let vpd = ch.volts_per_div() / factor;
        let offset = ch.offset - center * ch.volts_per_div();
        let span = (VDIV / 2.0 * vpd).abs() + offset.abs();
        cv.vline(a.x + ax, a.y + ly, a.y + ly + lh, GRID_AXIS);
        let mut free = i32::MIN;
        for j in 0..=VDIV as i32 {
            let y = a.y + ly + j * lh / VDIV as i32;
            cv.hline(a.x + ax - 4, a.x + ax, y, GRID_AXIS);
            let v = (VDIV / 2.0 - j as f32) * vpd - offset;
            let s = axis_label(v, vpd, offset, span, self.m.unit(c));
            let ty = (y + 4).clamp(a.y + 12, a.bottom() - 2);
            if ty < free + f.s.size { continue; }
            free = ty;
            cv.text(&f.s, a.x + 2, ty, &s, AXIS_TEXT);
        }
        // the count scale: nought at the axis, the largest bin at the end of the longest bar
        let top = a.y + (ly - 14).max(2);
        let end = a.x + ax + ((a.w - ax) * 9 / 10).max(40);
        cv.hline(a.x + ax, end, top + 12, GRID_AXIS);
        cv.text(&f.s, a.x + ax + 2, top + 10, "0", AXIS_TEXT);
        let label = format!("count {peak}");
        cv.text(&f.s, (end - f.s.width(&label)).max(a.x + ax + 14), top + 10, &label, AXIS_TEXT);
    }

    /// Histogram overlay: bars along the left edge (vertical) or the bottom (horizontal), mean and sigma in units.
    /// As a view of its own (`tile`) it fills `a`.
    fn draw_hist_in(&self, cv: &mut Canvas, a: Rect, tile: bool) {
        let Some((bins, mean, sd)) = self.hist_bins() else { return };
        let Some(c) = self.hist_src().filter(|&c| self.shows(c)) else { return };
        let col = CH_COLORS[c];
        let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f64;
        // as a view of its own: a volts axis at the left, on the lines of the lane it reads, and a count scale
        let ax = if tile && self.m.hist == Hist::Vertical { HIST_AXIS_W } else { 0 };
        let lane = if tile { self.source_area(WaveId::Ch(c)).unwrap_or(a) } else { a };
        let (ly, lh) = if tile && self.m.hist == Hist::Vertical { ((lane.y - a.y).max(0), lane.h.min(a.h)) } else { (0, a.h) };
        let depth = if tile { ((a.w - ax) * 9 / 10).max(40) as f64 } else { (a.w / 5).max(40) as f64 };
        let hdepth = if tile { a.h as f64 * 0.85 / 0.6 } else { depth };
        cv.set_clip(a);
        let ch = &self.m.ch[c];
        let txt = match self.m.hist {
            Hist::Vertical => {
                // a bin per row of the map; a larger plot (HDMI layout) gives each bin its share of rows
                let n = bins.len().max(1) as i32;
                for (r, &b) in bins.iter().enumerate() {
                    let len = (b as f64 / peak * depth).round() as i32;
                    let (y0, y1) = (ly + r as i32 * lh / n, ly + (r as i32 + 1) * lh / n);
                    if len > 0 {
                        cv.blend(Rect::new(a.x + ax, a.y + y0, len, (y1 - y0).max(1)), col, 150);
                    }
                }
                if tile { self.draw_hist_axes(cv, a, (ax, ly, lh), c, peak as u64); }
                // pixel row -> volts
                let (factor, center) = self.zoom_vertical_transform(self.area);
                let h = bins.len().max(1) as f64;
                let vpp = ch.volts_per_div() as f64 * VDIV as f64 / h / factor as f64;
                let mv = (h / 2.0 - mean) * vpp + center as f64 * ch.volts_per_div() as f64 - ch.offset as f64;
                format!("Hist CH{}  mean {}  \u{3c3} {}", c + 1, si(mv, self.m.unit(c)), si(sd * vpp, self.m.unit(c)))
            }
            _ => {
                let n = bins.len().max(1) as i32;
                for (x, &b) in bins.iter().enumerate() {
                    let len = (b as f64 / peak * hdepth * 0.6).round() as i32;
                    let (x0, x1) = (x as i32 * a.w / n, (x as i32 + 1) * a.w / n);
                    if len > 0 {
                        cv.blend(Rect::new(a.x + x0, a.y + a.h - len, (x1 - x0).max(1), len), col, 150);
                    }
                }
                let (tdiv, delay) = self.view();
                let spp = tdiv as f64 / (n as f64 / HDIV as f64);
                let mt = (mean - n as f64 / 2.0) * spp + delay as f64;
                format!("Hist CH{} @ level  mean {}  \u{3c3} {}", c + 1, si(mt, "s"), si(sd * spp, "s"))
            }
        };
        cv.no_clip();
        let f = self.fonts();
        // above the decoder band and the time labels, right of the volt labels
        // as a view of its own the header starts right of the volts axis, so the two never meet
        let r = if tile { Self::hist_head_rect(a, ax, f.s.width(&txt) + 16) } else { Rect::new(a.x + 14 + f.s.width("-0.00 mV"), a.y + a.h - 76, f.s.width(&txt) + 16, 20) };
        cv.round(r, 4, PANEL_BG, 230);
        cv.text_in(&f.s, r, 1, &txt, col);
    }

    /// The header of a histogram view: right of its volts axis and inside the view.
    pub(super) fn hist_head_rect(a: Rect, ax: i32, w: i32) -> Rect {
        let x = a.x + ax + 8;
        Rect::new(x, a.y + 24, w.min((a.right() - x - 4).max(20)), 20)
    }

    fn math_scale_text(&self) -> String {
        Self::math_scale_text_of(&self.m.math_ch(self.m.math_sel))
    }

    fn math_scale_text_of(c: &MathCh) -> String {
        if c.op == MathOp::Fft {
            format!("{} dB", FFT_DB[c.scale.min(FFT_DB.len() - 1)])
        } else {
            let sc = math_scales();
            si_short(sc[c.scale.min(sc.len() - 1)], c.op.unit())
        }
    }

    /// Test/legacy adapter; production rendering reuses immutable timed results.
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    fn math_source(&self, s: usize, below: usize) -> Vec<f32> {
        self.math_source_record(s, below).map_or_else(Vec::new, |w| w.v.as_ref().clone())
    }

    /// The waveform of math channel `k` (empty for FFT/Off or missing sources).
    fn math_wave_of(&self, k: usize) -> Vec<f32> {
        if self.m.math_ch(k).op == MathOp::Fft { return Vec::new(); }
        self.math_record(k).ok().flatten().map_or_else(Vec::new, |w| w.v.as_ref().clone())
    }

    /// The waveform of the selected math channel.
    fn math_wave(&self) -> Vec<f32> {
        self.math_wave_of(self.m.math_sel)
    }

    /// Picks the math scale so that the waveform spans about 6 divisions.
    /// Studio: the menu of a math channel that is on starts with its Turn off.
    fn with_math_off(&self, mut v: Vec<Item>) -> Vec<Item> {
        if self.sheets_on() && self.m.math != MathOp::Off { v.insert(0, Item::MathOff); }
        v
    }

    /// Math channel `k` goes off, and so does its chip in the bottom bar.
    pub(super) fn math_turn_off(&mut self, k: usize) {
        if k >= NMATH { return; }
        self.math_set_shown(k, false);
        self.settings_changed();
        self.close_menu();
        self.relayout();
    }

    pub(crate) fn math_autoscale(&mut self) {
        self.math_fit(self.m.math_sel);
    }

    /// Fits the scale and offset of math channel `k` to its result. A result that is flat (a source minus itself) is
    /// not stretched to nanovolts: the scale stays within 1/100 of the V/div of the sources it was made from.
    fn math_fit(&mut self, k: usize) {
        let mut c = self.m.math_ch(k);
        if c.op == MathOp::Fft {
            (c.scale, c.offset) = (3, 0.0);
        } else {
            let w = self.math_wave_of(k);
            let (mn, mx) = w.iter().filter(|x| x.is_finite()).fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
            let sc = math_scales();
            if mx < mn {
                c.scale = sc.iter().position(|&x| x >= 1.0).unwrap_or(0);
            } else {
                let floor = if c.op.unit() == "V" { self.math_source_vpd(k) as f64 / 100.0 } else { 0.0 };
                let want = ((mx - mn) as f64 / 6.0).max(1e-9).max(floor);
                c.scale = sc.iter().position(|&x| x >= want).unwrap_or(sc.len() - 1);
                c.offset = -((mx + mn) as f64 / 2.0 / sc[c.scale]) as f32;
            }
        }
        if k == self.m.math_sel { (self.m.math_scale, self.m.math_offset) = (c.scale, c.offset); }
        self.m.maths[k] = c;
    }

    fn draw_math(&self, cv: &mut Canvas) {
        // all traces first, then the labels on top of them
        let on: Vec<usize> = (0..NMATH).filter(|&k| self.m.math_shown(k) && self.math_win_of(k) == self.cur_win).collect();
        let labels: Vec<Option<String>> = on.iter().enumerate().map(|(row, &k)| self.draw_math_ch(cv, k, row as i32)).collect();
        let f = self.fonts();
        for (row, (label, &k)) in labels.iter().zip(&on).enumerate() {
            if let Some(label) = label {
                // top right, under the status line, one row per math channel (the results panel goes below them)
                // (in lanes the right edge is the scale's: the label sits at the left, after the lane's badge)
                let lanes = self.stacked() && self.wins.len() > 1;
                let r = if lanes { Rect::new(self.area.x + 56, self.area.y + 4 + 22 * row as i32, f.s.width(label) + 16, 20) }
                    else { Rect::new(self.area.x + self.area.w - 12 - f.s.width(label) - 16, self.area.y + 22 + 26 * row as i32, f.s.width(label) + 16, 20) };
                cv.round(r, 4, rgb(0x1c, 0x16, 0x28), 230);
                cv.text_in(&f.s, r, 1, label, MATH_COLORS[k % MATH_COLORS.len()]);
            }
        }
    }

    /// Draws the trace of math channel `k`; returns its label.
    fn draw_math_ch(&self, cv: &mut Canvas, k: usize, row: i32) -> Option<String> {
        let c = self.m.math_ch(k);
        let col = MATH_COLORS[k % MATH_COLORS.len()];
        let f = self.fonts();
        // the last stacked lane keeps its time labels clear: the trace is clipped above them
        let mut lane = self.area;
        if self.stacked() && self.cur_win + 1 == self.wins.len() { lane.h = (lane.h - (f.s.size + 8)).max(f.s.size); }
        cv.set_clip(lane);
        let ppy = self.area.h as f32 / VDIV;
        let cy = self.area.y as f32 + self.area.h as f32 / 2.0;
        let label;
        if c.op == MathOp::Fft {
            let Some(w) = self.math_record(k).ok().flatten() else { cv.no_clip(); return None; };
            let df = w.dt;
            let db = w.v.as_ref();
            if db.is_empty() {
                cv.no_clip();
                return None;
            }
            let dbdiv = FFT_DB[c.scale.min(FFT_DB.len() - 1)];
            // top of the grid = +20 dBV (moved by the offset), x: 0 .. fs/2 over the full width
            let top = 20.0 + c.offset * dbdiv;
            let span = df * (db.len() - 1) as f64;
            let n = db.len();
            let mut prev: Option<(i32, i32)> = None;
            for px in 0..self.area.w {
                // peak of the bins that fall into this pixel column
                let k0 = (px as usize * (n - 1)) / self.area.w as usize;
                let k1 = (((px + 1) as usize * (n - 1)) / self.area.w as usize).max(k0 + 1).min(n);
                let v = db[k0..k1].iter().copied().fold(f32::MIN, f32::max);
                let (zoom, center) = self.zoom_vertical_transform(self.area);
                let d = VDIV * 0.5 - (top - v) / dbdiv;
                let y = (cy - (d - center) * zoom * ppy).round() as i32;
                let x = self.area.x + px;
                if let Some((x0, y0)) = prev {
                    cv.line(x0, y0, x, y, col);
                }
                prev = Some((x, y));
            }
            label = format!("M{} FFT {} {}  {} dB/div  {}/div", k + 1, src_name(c.a), c.win.name(), dbdiv, si_short(span / HDIV as f64, "Hz"));
        } else {
            let result = self.math_record(k);
            let rec = result.as_ref().ok().and_then(|w| w.as_ref());
            if rec.is_none() {
                cv.no_clip();
                let status = match result { Err(e) => e, _ => "Computing" };
                cv.text(&f.s, self.area.x + 60, self.area.y + 40 + 18 * row, &format!("M{}: {}", k + 1, status), col);
                return None;
            }
            let rec = rec.unwrap();
            let w = rec.v.as_ref();
            let sc = math_scales();
            let vpd = sc[c.scale.min(sc.len() - 1)] as f32;
            let (tdiv, vdelay) = self.view();
            let (tdiv, vdelay) = (tdiv as f64, vdelay as f64);
            let ppx = self.area.w as f64 / HDIV as f64;
            let mut prev: Option<(i32, i32)> = None;
            let left = vdelay - tdiv * HDIV as f64 * 0.5;
            let right = left + tdiv * HDIV as f64;
            let at = |t: f64| ((t - rec.t0) / rec.dt).floor().clamp(-1.0, w.len() as f64) as isize;
            let begin = (at(left) - 1).clamp(0, w.len() as isize) as usize;
            let end = (at(right) + 2).clamp(0, w.len() as isize) as usize;
            let step = ((end - begin) / (self.area.w as usize * 2)).max(1);
            for i in (begin..end).step_by(step) {
                let v = w[i];
                if !v.is_finite() {
                    prev = None;
                    continue;
                }
                let t = rec.t0 + i as f64 * rec.dt - vdelay;
                let x = (self.area.x as f64 + self.area.w as f64 / 2.0 + t / tdiv * ppx).round() as i32;
                let (zoom, center) = self.zoom_vertical_transform(self.area);
                let y = (cy - (v / vpd + c.offset - center) * zoom * ppy).clamp(-1e4, 1e4).round() as i32;
                if let Some((x0, y0)) = prev {
                    cv.line(x0, y0, x, y, col);
                }
                prev = Some((x, y));
            }
            let name = if c.op == MathOp::Expr { self.m.math_expr[k].clone() } else { c.op.formula(c.a, c.b) };
            label = format!("M{} = {}  {}/div", k + 1, name, Self::math_scale_text_of(&c));
        }
        cv.no_clip();
        Some(label)
    }

    fn y_of_volts(&self, c: usize, v: f32) -> i32 {
        let a = self.ch_area(c);
        let ch = &self.m.ch[c];
        self.zoom_y_of_divs(a, (v + ch.offset) / ch.volts_per_div()).round() as i32
    }

    fn y_of_wave(&self, id: WaveId, v: f32) -> i32 {
        match id {
            WaveId::Ch(c) => self.y_of_volts(c, v),
            WaveId::Ref(i) => {
                let a = self.area;
                let Some(r) = self.refs.get(i).and_then(|x| x.as_ref()) else {
                    return a.y;
                };
                self.zoom_y_of_divs(a, (v + r.offset) / r.vpd.max(1e-12)).round() as i32
            }
        }
    }

    fn x_of_time(&self, t: f32) -> i32 {
        let a = self.area;
        let (tdiv, delay) = self.view();
        let ppd = a.w as f32 / HDIV;
        (a.x as f32 + a.w as f32 / 2.0 + (t - delay) / tdiv * ppd).round() as i32
    }

    /// Decoded packets of the last record (times in s relative to the trigger), of the decoder the menu edits.
    #[cfg(test)]
    fn decode_packets(&self) -> Result<Vec<oos_dsp::decode::Packet>, &'static str> {
        self.decode_packets_of(&self.m).map(|packets| packets.as_ref().clone())
    }

    /// The same for the decoder that m is the view of; cached per record and settings, up to four decoders run every frame.
    fn decode_packets_of(&self, m: &Model) -> Result<std::sync::Arc<Vec<oos_dsp::decode::Packet>>, &'static str> {
        use std::hash::{Hash, Hasher};
        if Self::decoder_uses_refs(m) || (!m.dec_on_la() && m.deskew_ch.iter().any(|&shift| shift != 0.0)) { return self.reference_decode_packets(m); }
        let Some(rec) = &self.last[0] else { return Ok(std::sync::Arc::new(Vec::new())) };
        if m.dec_on_la() {
            return self.la_packets(m, rec).map(std::sync::Arc::new);
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.wfm_total, self.hist_pos, rec.t0.to_bits(), rec.dt.to_bits()).hash(&mut h);
        for c in 0..NCH {
            (rec.ch[c].as_ptr() as usize, rec.ch[c].len(), m.ch[c].volts_per_div().to_bits()).hash(&mut h);
        }
        format!("{:?}", m.dec_cfg()).hash(&mut h);
        let key = h.finish();
        if let Some(e) = self.dec_cache.borrow().iter().find(|e| e.0 == key) {
            return Ok(std::sync::Arc::new(e.1.clone()));
        }
        let mut p = crate::trig::decode(m, std::array::from_fn(|c| &rec.ch[c][..]), rec.dt)?;
        for x in p.iter_mut() {
            x.t0 = rec.t0 + x.t0 * rec.dt;
            x.t1 = rec.t0 + x.t1 * rec.dt;
        }
        self.dec_cache_put(key, &p);
        Ok(std::sync::Arc::new(p))
    }

    /// Keeps what the decoders of one record need.
    fn dec_cache_put(&self, key: u64, p: &[oos_dsp::decode::Packet]) {
        let mut c = self.dec_cache.borrow_mut();
        if c.len() >= DECODERS * 2 {
            c.remove(0);
        }
        c.push((key, p.to_vec()));
    }

    /// All decoders that are on: bands at the bottom of the plot, or rows under the lanes for those on LA lines.
    fn draw_decode(&self, cv: &mut Canvas) {
        if self.last[0].is_none() && !self.refs.iter().any(Option::is_some) {
            return;
        }
        let (mut band, mut lane) = (0, 0);
        let bottom = self.area.y + self.area.h - 46;
        for k in 0..DECODERS {
            if !self.m.dec_is_on(k) {
                continue;
            }
            let m = self.m.dec_view(k);
            // rows under the lanes go with the LA's window, bands with their source's
            let win = self.dec_band_win(k);
            let row = if win.is_none() { self.la_dec_row_n(lane) } else { None };
            if row.is_some() {
                lane += 1;
            }
            if win.unwrap_or(self.la_win_of()) != self.cur_win {
                continue;
            }
            let y = match row {
                Some(r) => r.y + 3,
                None => {
                    band += 1;
                    bottom - (band - 1) * DEC_BAND
                }
            };
            self.draw_decoder(cv, &m, k, y, row);
        }
    }

    /// One decoder's band at height y; row = its place under the lanes.
    fn draw_decoder(&self, cv: &mut Canvas, m: &Model, k: usize, y: i32, row: Option<Rect>) {
        use oos_dsp::decode::Kind;
        let f = self.fonts();
        let a = self.area;
        let col = if m.dec_on_la() { la::line_color(m.dec_la[0].unwrap_or(0)) }
            else { decode_wave(m.dec_src).map_or(DIM, |source| self.wave_color(source)) };
        // rows under the lanes span all windows, like the lanes
        let a = if row.is_some() { self.lane_area() } else { a };
        let h = 20;
        cv.set_clip(row.unwrap_or(a));
        if row.is_none() {
            cv.blend(Rect::new(a.x, y - 3, a.w, h + 6), BG, 200);
        }
        let name = if m.dec_on_la() { format!("{} {}", k + 1, m.dec_proto.name()) }
            else { format!("{} {} {}", k + 1, m.dec_proto.name(), decode_source_name(m.dec_src)) };
        cv.text(&f.sb, a.x + 4, y + 15, &name, DIM);
        let packets = match self.decode_packets_of(m) {
            Ok(p) => p,
            Err(e) => {
                cv.text(&f.s, a.x + 16 + f.sb.width(&name), y + 15, e, DIM);
                cv.no_clip();
                return;
            }
        };
        let (tdiv, delay) = self.view();
        let ppx = a.w as f64 / HDIV as f64;
        let xt = |t: f64| (a.x as f64 + a.w as f64 / 2.0 + (t - delay as f64) / tdiv as f64 * ppx).round() as i32;
        let left = a.x + 10 + f.sb.width(&name);
        let mut x_prev_end = left;
        for p in packets.iter() {
            let (x0, x1) = (xt(p.t0), xt(p.t1));
            if x1 < left || x0 > a.x + a.w {
                continue;
            }
            let event = matches!(p.kind, Kind::Field(l) if l.starts_with('!'));
            if let Some(tag) = packet_tag(m, p) {
                let r = Rect::new(x0 - f.s.width(tag) / 2 - 3, y + 3, f.s.width(tag) + 6, h - 6);
                let bad = p.kind == Kind::Stop || ((p.kind == Kind::Ack || event) && !p.ok);
                cv.round(r, 3, if bad { mix(BG, RED, 140) } else { mix(BG, GREEN, 140) }, 255);
                cv.text_in(&f.s, r, 1, tag, TEXT);
                continue;
            }
            let x0 = x0.max(x_prev_end + 1);
            if x1 - x0 < 3 {
                continue;
            }
            let is_addr = matches!(p.kind, Kind::Addr { .. } | Kind::Id { .. } | Kind::Word { cmd: true } | Kind::Field(_));
            let base = if is_addr { ACCENT } else { col };
            let fill = if p.ok { scale(base, 70) } else { rgb(0x70, 0x18, 0x18) };
            cv.fill(Rect::new(x0 + 3, y, x1 - x0 - 6, h), fill);
            cv.line(x0, y + h / 2, x0 + 3, y, base);
            cv.line(x0, y + h / 2, x0 + 3, y + h - 1, base);
            cv.line(x1, y + h / 2, x1 - 3, y, base);
            cv.line(x1, y + h / 2, x1 - 3, y + h - 1, base);
            cv.hline(x0 + 3, x1 - 3, y, base);
            cv.hline(x0 + 3, x1 - 3, y + h - 1, base);
            let txt = packet_text(m, p);
            if f.s.width(&txt) + 6 < x1 - x0 {
                cv.text_in(&f.s, Rect::new(x0, y, x1 - x0, h), 1, &txt, TEXT);
            }
            x_prev_end = x1;
        }
        // threshold line on the source trace (LA lines have no analog level)
        if !m.dec_on_la() && m.dec_proto != Proto::CPHY {
            let Some(source) = decode_wave(m.dec_src) else { cv.no_clip(); return; };
            let ty = self.y_of_wave(source, m.dec_thr);
            cv.dashed_hline(a.x + 26, a.x + a.w - 26, ty, 2, 6, scale(col, 160));
            if m.dec_proto.module().is_some_and(|p| p.key() == "Arinc429") {
                // the LO level of ARINC 429
                let ty = self.y_of_wave(source, -m.dec_thr);
                cv.dashed_hline(a.x + 26, a.x + a.w - 26, ty, 2, 6, scale(col, 160));
            }
        }
        cv.no_clip();
    }

    /// Vertical scale and the sample rate of this record. Time per division stays the one horizontal scale.
    fn ref_badge_readout(r: &RefWave) -> (String, String) {
        let scale = format!("{}/div", si_short(r.vpd as f64, "V"));
        let rate = if r.dt.is_finite() && r.dt > 0.0 {
            si_short(1.0 / r.dt, "Sa/s")
        } else {
            "-".into()
        };
        (scale, rate)
    }

    fn ref_badge_lines(&self, slot: usize, r: &RefWave, maximum: i32) -> (String, String) {
        let tag = format!("R{}", slot + 1);
        let mut name = if r.label.trim().is_empty() || r.label.trim() == tag { tag.clone() } else { format!("{} {}", tag, r.label.trim()) };
        let width = (maximum - 12).max(16);
        if self.fonts().sb.width(&name) > width {
            while name.len() > tag.len() && self.fonts().sb.width(&format!("{}...", name)) > width { name.pop(); }
            if self.fonts().sb.width(&format!("{}...", name)) <= width { name.push_str("..."); }
            else { name = tag; }
        }
        let (scale, rate) = Self::ref_badge_readout(r);
        (name, format!("{} · {}", scale, rate))
    }

    /// Name, volts per division, and the record's sample rate, beside the trace.
    fn draw_ref_badge(&self, cv: &mut Canvas, area: Rect, y: i32, col: Rgb, slot: usize, r: &RefWave) {
        let f = self.fonts();
        // Studio lanes: the same small outlined badge a channel has at the left of its lane. The name, the scale and
        // the signal are on the reference's pill in the bottom bar, so nothing covers the trace
        if self.m.layout == Layout::Studio && self.stacked() {
            // at the middle of the lane, where a channel's badge sits when its ground line is centred: clear of the
            // volt labels at the top and the time labels at the bottom
            let _ = y;
            // right of the volt labels, which a reference has down its whole lane
            let b = Rect::new(area.x + 52, (area.y + area.h / 2 - 12).max(area.y + 2), 44, 24);
            cv.round(b, 4, col, 255);
            cv.round(b.inset(1), 3, BG, 255);
            cv.text_in(&f.sb, b, 1, &format!("R{}", slot + 1), col);
            return;
        }
        let maximum = (area.w - 16).clamp(36, 300);
        let (name, readout) = self.ref_badge_lines(slot, r, maximum);
        let text_w = f.sb.width(&name).max(f.s.width(&readout));
        let bw = (text_w + 12).clamp(36, maximum);
        let bh = REF_BADGE_H;
        let y0 = y.clamp(area.y + 2, (area.y + area.h - bh - self.ref_badge_pad(area)).max(area.y + 2));
        let x0 = (area.x + 80).min((area.x + area.w - bw - 4).max(area.x + 2));
        let b = Rect::new(x0, y0, bw, bh);
        cv.round(b, 3, col, 255);
        cv.set_clip(b);
        let ink = rgb(0, 0, 0);
        // two rows of their own: the name above, the scale and rate below it
        let row = (b.h - 4) / 2;
        cv.text_in(&f.sb, Rect::new(b.x, b.y + 2, b.w, row), 1, &name, ink);
        cv.text_in(&f.s, Rect::new(b.x, b.y + 2 + row, b.w, row), 1, &readout, ink);
        cv.set_clip(area);
    }

    /// Room a ref badge keeps clear under it: the time labels along the bottom of the lowest lane.
    pub(super) fn ref_badge_pad(&self, lane: Rect) -> i32 {
        let base = if self.stacked() && self.cur_win + 1 < self.wins.len() { 2 } else { self.fonts().s.size + 10 };
        // the Fit row and the With live note sit above the time labels of the last lane: the badge stays clear of them
        // (only a lane that reaches the bottom of the plot is under them)
        if !self.live_ref_note().is_empty() && !self.view_on() && !self.eye_open() && lane.bottom() >= self.area.bottom() - 4 { base.max(52) } else { base }
    }

    fn draw_refs(&self, cv: &mut Canvas) {
        self.draw_refs_at(cv, self.area, self.view(), false);
    }

    fn draw_refs_at(&self, cv: &mut Canvas, area: Rect, view: (f32, f32), all_sources: bool) {
        cv.set_clip(area);
        let ppx = area.w as f64 / HDIV as f64;
        if !(view.0 > 0.0) {
            cv.no_clip();
            return;
        }
        let mut badges: Vec<i32> = Vec::new();
        for (i, r) in self.refs.iter().enumerate() {
            let Some(r) = r else { continue };
            // Studio: a ref with its own horizontal scale keeps it in the overview too
            let (tdiv, delay) = if all_sources { self.studio_ref_own(i).unwrap_or(view) } else { view };
            let (tdiv, delay) = (tdiv as f64, delay as f64);
            let half = tdiv * HDIV as f64 / 2.0;
            let (t_left, t_right) = (delay - half, delay + half);
            let t0 = self.source_time_origin(WaveId::Ref(i), r.t0);
            if !all_sources && self.stacked() && self.trace_sources().get(self.cur_win) != Some(&workspace::Trace::Reference(i)) { continue; }
            // a split (not Stacked): each reference in the window its Windows row names
            if !all_sources && !self.stacked() && self.wins.len() > 1 && self.ref_win_of(i) != self.cur_win { continue; }
            if !r.on || r.v.is_empty() || !(r.dt > 0.0) {
                continue;
            }
            let col = REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1;
            // samples inside this window, not a stride of the whole record (that aliases a long record)
            let n = r.v.len() as isize;
            let at = |t: f64| -> isize {
                let x = (t - t0) / r.dt;
                if !x.is_finite() {
                    return 0;
                }
                x.floor().clamp(-1.0, n as f64) as isize
            };
            let k0 = (at(t_left) - 1).clamp(0, n) as usize;
            let k1 = (at(t_right) + 2).clamp(0, n) as usize;
            if k1 <= k0 {
                continue;
            }
            let cols = (area.w as usize).saturating_mul(2).max(1);
            let step = ((k1 - k0) / cols).max(1);
            let mut prev: Option<(i32, i32)> = None;
            let mut first = None;
            for k in (k0..k1).step_by(step).chain(std::iter::once(k1 - 1)) {
                let v = r.v[k];
                if !v.is_finite() {
                    prev = None;
                    continue;
                }
                let t = t0 + k as f64 * r.dt - delay;
                let x = (area.x as f64 + area.w as f64 / 2.0 + t / tdiv * ppx).round() as i32;
                let y = self.zoom_y_of_divs(area, (v + r.offset) / r.vpd).round() as i32;
                if let Some((x0, y0)) = prev {
                    if (x - x0).abs() < area.w * 2 && (y - y0).abs() < area.h * 4 {
                        cv.line(x0, y0, x, y, col);
                    }
                }
                if first.is_none() && x >= area.x && x < area.x + area.w {
                    first = Some(y);
                }
                prev = Some((x, y));
            }
            if let Some(y) = first {
                // badges of references that start at the same height are fanned out instead of printed over each other
                // none in the overview of a zoom (it shows the whole record, the detail has the badges)
                if !all_sources {
                    let (lo, hi) = (area.y + 2, (area.y + area.h - REF_BADGE_H - self.ref_badge_pad(area)).max(area.y + 2));
                    let spot = badge_spot(&badges, y, lo, hi);
                    badges.push(spot);
                    self.draw_ref_badge(cv, area, spot, col, i, r);
                }
            }
        }
        cv.no_clip();
    }

    fn draw_xy(&self, cv: &mut Canvas) {
        // the X and Y channels (CH1 and CH2 unless the Add panel chose others) in a square of 8 x 8 div in the middle of the grid
        let Some(rec) = &self.last[0] else { return };
        let (xc, yc) = (self.xy_src[0].min(n_ch() - 1), self.xy_src[1].min(n_ch() - 1));
        let (a, b) = (&rec.ch[xc], &rec.ch[yc]);
        let f = self.fonts();
        // a square that fits the area both ways: a narrow docked pane keeps its title and its left edge
        let side = self.area.h.min(self.area.w);
        let sq = Rect::new(self.area.x + (self.area.w - side) / 2, self.area.y + (self.area.h - side) / 2, side, side);
        cv.fill(sq, rgb(0x04, 0x04, 0x08));
        cv.rect(sq, GRID_AXIS);
        cv.vline(sq.x + side / 2, sq.y, sq.y + side - 1, GRID_LINE);
        cv.hline(sq.x, sq.x + side - 1, sq.y + side / 2, GRID_LINE);
        if a.is_empty() || b.is_empty() {
            cv.text_in(&f.m, sq, 1, &format!("XY needs CH{} and CH{}", xc + 1, yc + 1), DIM);
            return;
        }
        cv.set_clip(sq);
        let ppd = side as f32 / 8.0;
        let (ca, cb) = (&self.m.ch[xc], &self.m.ch[yc]);
        let col = rgb(0x9c, 0xf0, 0x9c);
        for k in 0..a.len().min(b.len()) {
            if !a[k].is_finite() || !b[k].is_finite() {
                continue;
            }
            let x = (sq.x as f32 + side as f32 / 2.0 + (a[k] + ca.offset) / ca.volts_per_div() * ppd) as i32;
            let y = (sq.y as f32 + side as f32 / 2.0 - (b[k] + cb.offset) / cb.volts_per_div() * ppd) as i32;
            cv.pixel(x, y, col, 160);
            cv.pixel(x + 1, y, col, 70);
            cv.pixel(x, y + 1, col, 70);
        }
        cv.no_clip();
        cv.text(&f.s, sq.x + 6, sq.y + 16, &format!("XY  X = CH{}  Y = CH{}", xc + 1, yc + 1), col);
    }

    fn draw_markers(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let a = self.area;
        // channel ground markers at the left edge of the grid
        for c in 0..NCH {
            if !self.m.ch[c].on || !self.shows(c) {
                continue;
            }
            let y = self.y_of_volts(c, 0.0).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
            let col = CH_COLORS[c];
            if self.m.layout == Layout::Studio { self.studio_lane_badge(cv, c, y); continue; }
            let r = Rect::new(a.x + 2, y - 8, 16, 16);
            cv.round(r, 3, col, 255);
            cv.triangle([(a.x + 17, y - 6), (a.x + 24, y), (a.x + 17, y + 6)], col);
            cv.text_in(&f.sb, r, 1, &format!("{}", c + 1), DARK_TEXT);
        }
        // trigger level marker at the right edge of the grid, its value next to it. Not with AC or LF reject trigger
        // coupling: the level then counts from the signal's mean, a mark at that level in volts would point nowhere
        // (the trigger card shows the level)
        let ts = self.m.trig_src;
        let lvl = self.trig_level_shown();
        let filtered = matches!(self.m.trig_coupling(), TrigCoup::Ac | TrigCoup::LfReject);
        if self.m.trig_ref.is_none() && self.m.ch[ts].on && self.shows(ts) && self.m.trig_line().is_none() && !filtered && self.trigger_absolute_level_visible(false) {
            let y = self.y_of_volts(ts, lvl).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
            let x1 = self.trigger_tag_x(a.x + a.w - MARK_W, y);
            let r = Rect::new(x1 + 8, y - 8, 16, 16);
            cv.round(r, 3, ACCENT, 255);
            cv.triangle([(x1 + 9, y - 6), (x1 + 2, y), (x1 + 9, y + 6)], ACCENT);
            cv.text_in(&f.sb, r, 1, "T", DARK_TEXT);
            let s = si_short(lvl as f64, self.m.unit(ts));
            backed_text(cv, &f.sb, x1 - 4 - f.sb.width(&s), y + 5, &s, ACCENT);
            if self.touch.drag == Drag::Level || self.level_line_on() {
                cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, ACCENT);
            }
        }
        // second level: lower threshold, or level of source B / the clock
        let t = self.m.trig_type;
        let c2 = match t {
            TrigType::Slope | TrigType::Runt | TrigType::Window => Some((ts, "t")),
            TrigType::Delay => Some((self.m.trig_src2, "B")),
            TrigType::SetupHold => Some((self.m.trig_src2, "C")),
            _ => None,
        };
        if let Some((c, tag)) = c2 {
            let secondary = matches!(t, TrigType::Delay | TrigType::SetupHold);
            let source = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
            if source == WaveId::Ch(c) && self.m.ch[c].on && self.shows(c) && self.trigger_absolute_level_visible(secondary) {
                let y = self.y_of_volts(c, self.m.trig_level2).clamp(a.y + (a.h / 2).min(9), a.y + a.h - (a.h / 2).min(9));
                let x1 = self.trigger_tag_x(a.x + a.w - MARK_W, y);
                let r = Rect::new(x1 + 8, y - 8, 16, 16);
                cv.round(r, 3, mix(BG, ACCENT, 170), 255);
                cv.triangle([(x1 + 9, y - 6), (x1 + 2, y), (x1 + 9, y + 6)], mix(BG, ACCENT, 170));
                cv.text_in(&f.sb, r, 1, tag, DARK_TEXT);
                let s = si_short(self.m.trig_level2 as f64, self.m.unit(ts));
                backed_text(cv, &f.sb, x1 - 4 - f.sb.width(&s), y + 5, &s, mix(BG, ACCENT, 200));
                if self.touch.drag == Drag::Level2 { cv.hline(a.x, a.x + a.w - 1, y, ACCENT); }
            }
        }
        self.draw_reference_trigger_markers(cv);
        // Trigger time is absolute in an imported record.
        let tx = self.x_of_time(self.replay.trigger_time.unwrap_or(0.0) as f32);
        let ty = a.y + 9;
        if tx < a.x {
            cv.triangle([(a.x + 1, ty - 4), (a.x + 9, ty - 9), (a.x + 9, ty + 1)], ACCENT);
        } else if tx > a.x + a.w {
            let x = a.x + a.w - 1;
            cv.triangle([(x, ty - 4), (x - 8, ty - 9), (x - 8, ty + 1)], ACCENT);
        } else {
            cv.triangle([(tx - 6, ty - 8), (tx + 6, ty - 8), (tx, ty)], ACCENT);
        }
        // drag feedback
        let tag = |cv: &mut Canvas, x: i32, y: i32, s: &str, col: Rgb| {
            let r = Rect::new(x, y, f.sb.width(s) + 20, 22);
            cv.framed(r, 5, 1, LINE, 4, BAR);
            cv.text_in(&f.sb, r, 1, s, col);
        };
        match self.touch.drag {
            Drag::Offset(c) if self.shows(c) => {
                let y = self.y_of_volts(c, 0.0);
                cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, CH_COLORS[c]);
                tag(cv, a.x + 8, y - 30, &format!("Offset {}", si(self.m.ch[c].offset as f64, self.m.unit(c))), CH_COLORS[c]);
            }
            Drag::Delay if self.cur_win == self.touch.win.min(self.wins.len().max(1) - 1) => tag(cv, a.x + a.w / 2 - 60, a.y + 12, &format!("Delay {}", si(self.view().1 as f64, "s")), ACCENT),
            Drag::Level if self.shows(ts) && !filtered && self.m.trig_ref.is_none() && self.trigger_absolute_level_visible(false) => {
                let y = self.y_of_volts(ts, self.m.trig_level);
                tag(cv, a.x + a.w - 130, y - 30, &format!("Level {}", si(self.m.trig_level as f64, self.m.unit(ts))), ACCENT);
            }
            _ => {}
        }
    }

    fn draw_cursors(&self, cv: &mut Canvas) {
        let source = self.cursor_source();
        if !self.workspace_source_visible_here(source)
            && !(self.m.cursor == CursorMode::Track && self.workspace_source_visible_here(self.cursor_b_source())) { return; }
        let f = self.fonts();
        let a = self.area;
        cv.set_clip(a);
        let picked = self.workspace.cursor_sel.and_then(|k| self.cursor_tags().into_iter().find(|t| t.0 == k).map(|t| t.1));
        let tagbox = |cv: &mut Canvas, x: i32, y: i32, s: &str| {
            let r = Rect::new(x, y, f.s.width(s) + 8, 14);
            cv.round(r, 2, CURSOR, 255);
            cv.text_in(&f.s, r, 1, s, DARK_TEXT);
            if picked == Some(r) { cv.round_outline(Rect::new(r.x - 2, r.y - 2, r.w + 4, r.h + 4), 3, ACCENT); }
        };
        match self.m.cursor {
            CursorMode::Off => {}
            CursorMode::Time | CursorMode::Track | CursorMode::Xy => {
                if self.m.cursor == CursorMode::Xy {
                    for (frac, name) in [(self.m.cur_c, "Y1"), (self.m.cur_d, "Y2")] {
                        let y = a.y + (frac * a.h as f32) as i32;
                        cv.dashed_hline(a.x, a.x + a.w - 1, y, 4, 3, CURSOR);
                        tagbox(cv, self.right_tag_x(a, y - 16, f.s.width(name) + 8), y - 16, name);
                    }
                }
                let (ta, tb) = self.cursor_times();
                for (frac, name, t) in [(self.m.cur_a, "X1", ta), (self.m.cur_b, "X2", tb)] {
                    // a cursor that the zoomed view does not show keeps its time and is not drawn
                    if !(0.0..=1.0).contains(&frac) { continue; }
                    let x = a.x + (frac * a.w as f32) as i32;
                    cv.dashed_vline(x, a.y, a.y + a.h - 1, 4, 3, CURSOR);
                    tagbox(cv, x + 2, a.y + a.h - 36, name);
                    // Each tracking mark follows its own native source and lane.
                    if self.m.cursor == CursorMode::Track {
                        let id = if name == "X1" { source } else { self.cursor_b_source() };
                        if self.workspace_source_visible_here(id) {
                            if let Some(v) = self.wave_at_source(id, t) {
                                let y = self.y_of_wave(id, v);
                                cv.dashed_hline(x - 10, x + 10, y, 2, 2, CURSOR);
                            }
                        }
                    }
                }
            }
            CursorMode::Volt => {
                for (frac, name) in [(self.m.cur_a, "Y1"), (self.m.cur_b, "Y2")] {
                    let y = a.y + (frac * a.h as f32) as i32;
                    cv.dashed_hline(a.x, a.x + a.w - 1, y, 4, 3, CURSOR);
                    tagbox(cv, self.right_tag_x(a, y - 16, f.s.width(name) + 8), y - 16, name);
                }
            }
        }
        self.draw_studio_cursor_readouts(cv, a);
        cv.no_clip();
    }

    /// Left edge of a cursor tag at the right end of the plot: where it clears the full-screen and zoom buttons.
    fn right_tag_x(&self, a: Rect, y: i32, w: i32) -> i32 {
        let mut x = a.x + a.w - 26;
        for r in [self.full_btn().0, self.zoom_btn().0] {
            if x + w > r.x - 2 && x < r.right() + 2 && y < r.bottom() + 2 && y + 14 > r.y - 2 { x = r.x - 6 - w; }
        }
        x
    }

    /// Cursor read-outs: (label, value) pairs.
    fn cursor_values(&self) -> Vec<(String, String)> {
        let (source, scale, offset, unit) = match self.cursor_source_context() {
            Some(context) => context,
            None if self.m.cursor == CursorMode::Track => {
                let source = self.cursor_source();
                (source, 1.0, 0.0, self.cursor_source_unit(source))
            }
            None => return Vec::new(),
        };
        let area = self.source_area(source).unwrap_or(self.area);
        match self.m.cursor {
            CursorMode::Off => Vec::new(),
            CursorMode::Xy => {
                let (ta, tb) = self.cursor_times();
                let v = |c: f32| self.zoom_divs_at(area, c) * scale - offset;
                let (va, vb) = (v(self.m.cur_c), v(self.m.cur_d));
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("X2".into(), si(tb, "s")),
                    ("Y1".into(), si(va as f64, unit)),
                    ("Y2".into(), si(vb as f64, unit)),
                    ("\u{394}X".into(), si(tb - ta, "s")),
                    ("\u{394}Y".into(), si((vb - va) as f64, unit)),
                ]
            }
            CursorMode::Track => {
                let (ta, tb) = self.cursor_times();
                let second = self.cursor_b_source();
                let (ya, yb) = (self.wave_at_source(source, ta), self.wave_at_source(second, tb));
                let (ua, ub) = (unit, self.cursor_source_unit(second));
                let txt = |v: Option<f32>, u: &str| v.map_or("----".to_string(), |v| si(v as f64, u));
                let dy = ya.zip(yb).filter(|_| ua == ub).map(|(a, b)| b - a);
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("Y1".into(), txt(ya, ua)),
                    ("X2".into(), si(tb, "s")),
                    ("Y2".into(), txt(yb, ub)),
                    ("\u{394}X".into(), si(tb - ta, "s")),
                    ("\u{394}Y".into(), txt(dy, if ua == ub { ua } else { "" })),
                ]
            }
            CursorMode::Time => {
                let (ta, tb) = self.cursor_times();
                let d = tb - ta;
                vec![
                    ("X1".into(), si(ta as f64, "s")),
                    ("X2".into(), si(tb as f64, "s")),
                    ("\u{394}X".into(), si(d as f64, "s")),
                    ("1/\u{394}X".into(), Self::inv_dt(d as f64)),
                ]
            }
            CursorMode::Volt => {
                let va = self.zoom_divs_at(area, self.m.cur_a) * scale - offset;
                let vb = self.zoom_divs_at(area, self.m.cur_b) * scale - offset;
                vec![("Y1".into(), si(va as f64, unit)), ("Y2".into(), si(vb as f64, unit)), ("\u{394}Y".into(), si((vb - va) as f64, unit))]
            }
        }
    }

    /// Status line inside the top of the plot: touch lock, history position and search count on the left; a source that
    /// is not the live hardware on the right.
    fn draw_info(&self, cv: &mut Canvas) {
        if self.eye_open() && self.m.eye_place == EyePlace::Full && self.workspace.eye_float.is_none() { return; }
        let f = self.fonts();
        let g = self.grid;
        let y = g.y + 14;
        // only what makes the picture something else than a live measurement; the rates are on System info
        let record_caption = self.record_caption();
        let src = if self.record_mode() {
            record_caption.as_str()
        } else {
            match self.src.name() {
                "DEMO" | "REPLAY" if self.m.layout == Layout::Studio => "",
                "DEMO" => "Demo signals",
                "REPLAY" => "Replay",
                _ => "",
            }
        };
        if !src.is_empty() {
            backed_text(cv, &f.s, self.full_home().0.x - 16 - f.s.width(src), y, src, DIM);
        }
        let mut x = g.x + 8;
        // a channel at the end of the ADC range: said in red, first on the line
        let clip = self.clipping_channels();
        if !clip.is_empty() {
            let names: Vec<String> = clip.iter().map(|c| format!("CH{}", c + 1)).collect();
            x = backed_text(cv, &f.sb, x, y, &format!("Clipping: {} \u{b7} raise V/div or move the trace", names.join(" ")), RED) + 16;
        }
        if self.m.touch_lock {
            let hint = if self.m.layout == Layout::Studio { "Touch locked \u{b7} tap the gear to unlock" } else { "Touch locked \u{b7} tap Unlock" };
            x = backed_text(cv, &f.sb, x, y, hint, AMBER) + 16;
        }
        let seg = self.seg_view.filter(|_| self.seg_hw_on()).and_then(|v| match (v.phase, self.seg_pos) {
            (crate::board::SegPhase::Recording, _) => Some(format!("Recording {}/{}", v.recorded, v.total)),
            (crate::board::SegPhase::Reading, _) => Some(format!("Reading {}/{}", v.read, v.recorded)),
            (crate::board::SegPhase::Done, Some(i)) => Some(self.seg_label(i)),
            _ => None,
        });
        if let Some(t) = seg {
            x = backed_text(cv, &f.sb, x, y, &t, BLUE) + 16;
        } else if let Some(i) = self.hist_pos {
            x = backed_text(cv, &f.sb, x, y, &format!("History {}/{}", i + 1, self.history.len()), BLUE) + 16;
        }
        if self.search_on {
            let events = self.search_events();
            let label = if self.m.trig_ref.is_some() && !self.reference_search_supported() {
                "Search: condition needs hardware channels".into()
            } else if self.m.trig_ref.is_some() && self.reference_search_busy() {
                format!("Search {} · full record · computing…", self.trigger_source_label())
            } else {
                format!("Search {}{}: {} events", self.trigger_source_label(), if self.m.trig_ref.is_some() { " · full record" } else { "" }, events.len())
            };
            backed_text(cv, &f.sb, x, y, &label, AMBER);
        }
        let note = self.live_ref_note();
        if !note.is_empty() && !self.view_on() && !self.eye_open() {
            let y = g.y + g.h - 36;
            let fit = Rect::new(g.x + 8, y - 13, 52, 24);
            btn(cv, fit, BTN, ACCENT);
            cv.text_in(&f.s, fit, 1, "Fit", TEXT);
            let lab = Rect::new(fit.x + fit.w + 8, y - 12, (g.x + g.w - fit.x - fit.w - 16).max(1), 22);
            cv.set_clip(lab);
            let text = self.fit_ref_note(&note, lab.w);
            // over a trace of the last lane: backed, like the time labels
            cv.blend(Rect::new(lab.x - 3, lab.y + 2, f.s.width(&text) + 6, lab.h - 4), BG, 190);
            cv.text_in(&f.s, lab, 0, &text, LABEL);
            cv.no_clip();
        }
    }

    /// The "With live" note cut to `width`, naming how many references no longer fit ("+2").
    fn fit_ref_note(&self, note: &str, width: i32) -> String {
        let f = self.fonts();
        if f.s.width(note) <= width { return note.to_string(); }
        let Some((head, rest)) = note.split_once(" · ") else { return note.to_string() };
        let parts: Vec<&str> = rest.split("   ").collect();
        for keep in (0..parts.len()).rev() {
            let text = if keep == 0 { format!("{head} · +{}", parts.len()) } else { format!("{head} · {}   +{}", parts[..keep].join("   "), parts.len() - keep) };
            if f.s.width(&text) <= width || keep == 0 { return text; }
        }
        note.to_string()
    }

    /// Search markers: an amber tick at the top of the plot for every event of the trigger condition.
    fn draw_search(&self, cv: &mut Canvas) {
        if !self.search_on && self.studio_search_marks().is_none() {
            return;
        }
        let a = self.area;
        // the markers take the colour of the channel that is searched
        let mark_col = match &self.studio.search { Some(spec) if self.m.layout == Layout::Studio => self.wave_color(spec.src), _ => AMBER };
        let events = self.search_events();
        let (scale, delay) = self.view();
        let left = delay as f64 - scale as f64 * HDIV as f64 * 0.5;
        let per_pixel = scale as f64 * HDIV as f64 / a.w.max(1) as f64;
        let mut first = events.partition_point(|&time| time < left);
        // At most one marker per occupied pixel, without walking millions of
        // off-screen events or hiding their actual count in Search results.
        for column in 0..a.w {
            let next = events.partition_point(|&time| time < left + (column + 1) as f64 * per_pixel);
            if next > first {
                let x = a.x + column;
                cv.triangle([(x - 4, a.y + 1), (x + 4, a.y + 1), (x, a.y + 8)], mark_col);
                cv.dashed_vline(x, a.y + 9, a.y + a.h - 1, 2, 6, mix(BG, mark_col, 90));
            }
            first = next;
        }
    }

    /// Height the strip takes when it sits under the plot or above the trace, with the measurement card if that is
    /// docked there too.
    fn strip_rows_h(&self) -> i32 {
        let rows = self.strip_rows();
        (if rows == 0 { 0 } else { rows * self.lay().strip_row + 4 }) + self.results_band_h()
    }

    /// The measurement card in a dock (Minimal layout), when it was docked rather than floated.
    fn results_docked(&self) -> Option<StripDock> {
        (self.m.compact() && self.workspace.measure_docked && self.workspace.measure_float.is_none() && self.results_panel()).then_some(self.m.strip_dock).filter(|d| *d != StripDock::Hidden)
    }

    /// Width a side-docked measurement card takes from the plot.
    fn results_side_w(&self) -> i32 {
        if matches!(self.results_docked(), Some(StripDock::Left | StripDock::Right)) { self.lay().results_w + 6 } else { 0 }
    }

    /// Height a card docked under or above the plot takes: the header and two rows, paged.
    fn results_band_h(&self) -> i32 {
        if !matches!(self.results_docked(), Some(StripDock::Below | StripDock::Above)) { return 0; }
        let total = self.measurement_count();
        let page = self.results_page_size();
        let lines = total.min(page).div_ceil(self.results_cols()) as i32;
        let nav = if total > page { 38 } else { 0 };
        30 + if self.results_min { 0 } else { lines * self.results_row(total as i32) + nav + 4 } + 4
    }

    /// Left edge and width of the graticule. A side dock keeps a column for the strip and for a docked card.
    fn plot_span(&self) -> (i32, i32) {
        if self.dock_on() { let l = self.dock_wave_rect(); return (l.x, l.w); }
        let body = self.frame().body;
        let side = if self.strip_rows() > 0 { STRIP_SIDE } else { 0 } + self.results_side_w();
        let dock = self.studio_dock_w();
        match self.m.strip_dock {
            StripDock::Left => (body.x + side, body.w - side - dock),
            StripDock::Right => (body.x, body.w - side - dock),
            _ => (body.x, body.w - dock),
        }
    }

    /// Top of the rows when the strip runs across the screen.
    fn strip_band_y(&self) -> i32 {
        if self.m.strip_dock == StripDock::Above {
            let top = if self.full_on() { 0 } else { self.frame().body.y };
            return top + 2;
        }
        let g = self.grid;
        // the rows sit on the cards: what the plot's whole divisions leave over goes between plot and rows
        (self.plot_bottom() + 2 - self.strip_rows() * self.lay().strip_row).max(g.y + g.h + 4 + self.eye_below() + self.la_strip_h() + self.pane_h())
    }

    /// The column a side dock reserves, from the graticule down to the cards.
    fn strip_column(&self) -> Rect {
        let y = self.grid.y;
        let h = (self.plot_bottom() - y).max(0);
        let body = self.frame().body;
        let x = if self.m.strip_dock == StripDock::Left { body.x } else { body.right() - STRIP_SIDE };
        Rect::new(x, y, STRIP_SIDE, h)
    }

    /// Where the strip is drawn. None when it has nothing to show.
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    fn strip_rect(&self) -> Option<Rect> {
        let rows = self.strip_rows();
        if rows == 0 {
            return None;
        }
        match self.m.strip_dock {
            StripDock::Left | StripDock::Right => Some(self.strip_column()),
            _ => Some(Rect::new(0, self.strip_band_y(), self.scr.w, rows * self.lay().strip_row)),
        }
    }

    /// One line of the strip when it is a column: channel pill on the first line of a group.
    fn strip_side_lines(&self) -> Vec<StripLine> {
        let mut lines = Vec::new();
        let in_pane = self.results_pane_on() || self.studio_owns_measurements();
        if !self.results_panel() && !in_pane {
            let rows = self.meas_rows();
            for id in self.meas_groups() {
                let col = self.wave_color(id);
                let label = self.wave_name(id);
                let mine: Vec<&tools::MeasRow> = rows.iter().filter(|r| r.src == id).collect();
                for (i, row) in mine.iter().enumerate() {
                    lines.push(StripLine { pill: (i == 0).then(|| (label.clone(), col)), name: row.name.clone(), value: row.text(), col, align: i > 0 });
                }
            }
        }
        if self.m.dvm_on || self.m.cnt_on {
            for (name, value) in self.dvm_row() {
                lines.push(StripLine { pill: None, name, value, col: TEXT, align: false });
            }
        }
        if self.m.cursor != CursorMode::Off && !in_pane {
            let mode = match self.m.cursor {
                CursorMode::Volt => "Volt",
                CursorMode::Track => "Track",
                CursorMode::Xy => "XY",
                _ => "Time",
            };
            let vals = self.cursor_values();
            if let Some((label, value)) = vals.first() {
                lines.push(StripLine {
                    pill: self.cursor_badge(),
                    name: format!("{mode} {label}"),
                    value: value.clone(),
                    col: TEXT,
                    align: false,
                });
            }
            for (name, value) in vals.iter().skip(1) {
                lines.push(StripLine { pill: None, name: name.clone(), value: value.clone(), col: TEXT, align: true });
            }
        }
        lines
    }

    /// Measurement rows and the cursor row. Measure > Dock puts them under the plot, above the trace, or on a side.
    fn draw_strip(&self, cv: &mut Canvas) {
        if self.strip_rows() == 0 {
            return;
        }
        if self.draw_paged_measurement_strip(cv) { return; }
        match self.m.strip_dock {
            StripDock::Left | StripDock::Right => self.draw_strip_side(cv),
            _ => self.draw_strip_band(cv, self.strip_band_y()),
        }
    }

    /// The strip as rows across the screen, starting at `y`.
    fn draw_strip_band(&self, cv: &mut Canvas, mut y: i32) {
        let f = self.fonts();
        // bar height and text baseline of a row; the source pill and the values right of it
        let bh = self.lay().strip_row - 2;
        let base = (bh + f.s.size * 72 / 100) / 2;
        let pw = 42.max(f.sb.width("CH4").max(f.sb.width("R10")) + 18);
        let vx = 8 + pw + 8;
        // a row: the width of the screen less the edges
        let row = |y: i32| Rect::new(METRICS.edge, y, self.scr.w - 2 * METRICS.edge, bh);
        let pill = |cv: &mut Canvas, y: i32, label: &str, col: Rgb| {
            let r = Rect::new(8, y + 1, pw, bh - 2);
            cv.round(r, 3, col, 255);
            cv.text_in(&f.sb, r, 1, label, DARK_TEXT);
        };
        let in_pane = self.results_pane_on() || self.studio_owns_measurements();
        if !self.results_panel() && !in_pane {
            let rows = self.meas_rows();
            for id in self.meas_groups() {
                let mine: Vec<&tools::MeasRow> = rows.iter().filter(|r| r.src == id).collect();
                let col = self.wave_color(id);
                let label = self.wave_name(id);
                for part in mine.chunks(tools::STRIP_PER_ROW) {
                    cv.fill(row(y), BAR);
                    pill(cv, y, &label, col);
                    // the row's width shared out, whatever the number of values
                    let pitch = (row(y).right() - 2 - vx) / part.len() as i32;
                    for (i, r) in part.iter().enumerate() {
                        let x = vx + i as i32 * pitch;
                        // name and value share the cell; at the larger text sizes the name is shortened, then dropped, and
                        // the value is cut last, so nothing runs over the edge of the screen
                        let (txt, cell) = (r.text(), (pitch - 6).min(row(y).right() - 2 - x));
                        let nroom = cell - f.sb.width(&txt) - 8;
                        let mut x2 = x;
                        if nroom >= 24 {
                            let shown = f.s.width(&r.name).min(nroom);
                            cv.text_fit(&f.s, Rect::new(x, y, nroom, bh), 0, &r.name, DIM);
                            x2 = x + shown + 8;
                        }
                        cv.text_fit(&f.sb, Rect::new(x2, y, (x + cell - x2).max(8), bh), 0, &txt, col);
                    }
                    y += self.lay().strip_row;
                }
            }
        }
        if self.m.dvm_on || self.m.cnt_on {
            cv.fill(row(y), BAR);
            let mut x = 12;
            for (l, v) in self.dvm_row() {
                x = cv.text(&f.s, x, y + base, &l, DIM) + 8;
                x = cv.text(&f.sb, x, y + base, &v, TEXT) + 24;
            }
            y += self.lay().strip_row;
        }
        if self.m.cursor != CursorMode::Off && !in_pane {
            cv.fill(row(y), BAR);
            if let Some((label, color)) = self.cursor_badge() {
                pill(cv, y, &label, color);
            }
            let mode = match self.m.cursor {
                CursorMode::Volt => "Volt",
                CursorMode::Track => "Track",
                CursorMode::Xy => "XY",
                _ => "Time",
            };
            let r = Rect::new(vx - 2, y + 2, 38.max(f.s.width("Track") + 8), bh - 4);
            cv.round_outline(r, 3, DIM);
            cv.text_in(&f.s, r, 1, mode, LABEL);
            let vals = self.cursor_values();
            let x0 = r.x + r.w + 10;
            // the pitch of the values, closer where a large text size would push the last one off the row
            let pitch = if matches!(self.m.cursor, CursorMode::Track | CursorMode::Xy) { 150 } else { 200 }.min((row(y).right() - 6 - x0) / vals.len().max(1) as i32);
            for (i, (l, v)) in vals.iter().enumerate() {
                let x = x0 + i as i32 * pitch;
                let x2 = cv.text(&f.s, x, y + base, l, DIM) + 8;
                cv.text(&f.sb, x2, y + base, v, TEXT);
            }
        }
    }

    /// The same read-outs in a column beside the graticule: one measurement a line, the value at the right.
    fn draw_strip_side(&self, cv: &mut Canvas) {
        let lines = self.strip_side_lines();
        if lines.is_empty() {
            return;
        }
        let col = self.strip_column();
        let n = lines.len() as i32;
        let strip_row = self.lay().strip_row;
        let room = (col.h - 4).max(strip_row);
        let pitch = if n * strip_row <= room { strip_row } else { (room / n).max(12) };
        let f = self.fonts();
        cv.set_clip(col);
        let mut y = col.y + 2;
        for line in &lines {
            let bar = Rect::new(col.x + 4, y, col.w - 8, (pitch - 2).max(12));
            cv.fill(bar, BAR);
            let mut text_x = bar.x + 6;
            if let Some((label, pill_col)) = &line.pill {
                let ph = (bar.h - 2).clamp(10, 14);
                let r = Rect::new(bar.x + 2, bar.y + (bar.h - ph) / 2, 42, ph);
                cv.round(r, 3, *pill_col, 255);
                cv.text_in(&f.sb, r, 1, label, DARK_TEXT);
                text_x = r.x + r.w + 6;
            } else if line.align {
                text_x = bar.x + 50;
            }
            let room_r = bar.x + bar.w - 4;
            let vw = if line.value.is_empty() { 0 } else { f.sb.width(&line.value) };
            // the value keeps the right edge; a long one is clipped there and the name gives way
            let vx = if vw == 0 { room_r } else { (room_r - vw).max(text_x) };
            let base = bar.y + bar.h - 4;
            if vw > 0 {
                cv.set_clip(Rect::new(text_x, bar.y, (room_r - text_x).max(0), bar.h));
                cv.text(&f.sb, vx, base, &line.value, line.col);
                cv.set_clip(col);
            }
            let name_w = vx - 6 - text_x;
            if name_w > 8 && !line.name.is_empty() {
                cv.set_clip(Rect::new(text_x, bar.y, name_w, bar.h));
                cv.text(&f.s, text_x, base, &line.name, DIM);
                cv.set_clip(col);
            }
            y += pitch;
            if y + 12 > col.y + col.h {
                break;
            }
        }
        cv.no_clip();
    }

    /// Header and body of the measurement results overlay (statistics on): top right of the plot, under the status
    /// line and the math label, left of the strip where a drag moves the trigger level. Folded: header only.
    fn results_rects(&self) -> Option<(Rect, Rect)> {
        if !self.results_panel() {
            return None;
        }
        let a = self.over_area();
        let total = self.measurement_count();
        let page = self.results_page_size();
        let n = total.min(page).div_ceil(self.results_cols()) as i32;
        let nav = if total > page { 38 } else { 0 };
        let body_h = if self.results_min { 0 } else { n * self.results_row(total as i32) + nav + 4 };
        let panel_h = 30 + body_h;
        let rw = self.lay().results_w * self.results_cols() as i32;
        let fr = self.frame();
        let (top, bottom, sw) = (fr.top.bottom(), fr.bottom.y, self.scr.w);
        let docked = self.results_docked();
        let strip_side = if self.strip_rows() > 0 { STRIP_SIDE } else { 0 };
        let strip_h = { let rows = self.strip_rows(); if rows == 0 { 0 } else { rows * self.lay().strip_row + 4 } };
        let (body_top, plot_x, plot_w) = (if self.full_on() { 0 } else { fr.body.y }, self.plot_span().0, self.plot_span().1);
        let default = match docked {
            Some(StripDock::Left) => (fr.body.x + strip_side + 2, body_top),
            Some(StripDock::Right) => (fr.body.right() - strip_side - rw - 2, body_top),
            Some(StripDock::Above) => (plot_x + (plot_w - rw) / 2, body_top + strip_h + 2),
            Some(StripDock::Below) => (plot_x + (plot_w - rw) / 2, bottom - strip_h - panel_h - 2),
            Some(StripDock::Hidden) | None => (a.x + a.w - 56 - rw, a.y + 22 + 26 * (0..NMATH).filter(|&k| self.m.math_shown(k)).count() as i32),
        };
        let (mut x, mut y) = self.workspace.measure_float.unwrap_or(default);
        // A floating card never covers the full-screen and zoom corner buttons or the zoom bar.
        let floating = docked.is_none();
        for r in if floating { vec![self.full_btn().0, self.zoom_btn().0] } else { Vec::new() } {
            let r = Rect::new(r.x - 4, r.y - 4, r.w + 8, r.h + 8);
            if x + rw > r.x && x < r.right() && y < r.bottom() && y + panel_h > r.y { x = (r.x - rw).max(0); }
        }
        if floating && self.m.zoom && !self.view_on() { y = y.max(self.zoom_readout().bottom() + 2); }
        // Expanding a collapsed panel or changing its page must never put the
        // body and navigation beyond the touchable workspace.
        let x = x.clamp(0, sw - rw);
        let y = y.clamp(top, (bottom - panel_h).max(top));
        let head = Rect::new(x, y, rw, 30);
        Some((head, Rect::new(x, y + 30, rw, body_h)))
    }

    /// Keep source/value and mean/deviation readable even for large selections (two lines at the normal text size).
    fn results_pitch(_n: i32) -> i32 {
        42
    }

    /// Height of a row of the results card at the text size: always two lines.
    fn results_row(&self, n: i32) -> i32 {
        self.lay().res_pitch.1.max(Self::results_pitch(n))
    }

    fn results_name(&self, index: usize, row: &tools::MeasRow) -> String {
        if let Some(&(kind, a, b)) = index.checked_sub(self.m.meas.len()).and_then(|i| self.m.meas2.get(i)) {
            format!("{}-{} {}", self.wave_name(a), self.wave_name(b), tools::PAIRS[kind.min(tools::PAIRS.len() - 1)].0)
        } else if index >= self.m.meas.len() + self.m.meas2.len() {
            self.selected_measurement_label(index)
        } else { format!("{} {}", self.wave_name(row.src), row.name) }
    }

    /// Measurement results with statistics as a translucent card over the plot: value, mean, ±σ, acquisitions;
    /// tapping the header folds it.
    fn draw_results(&self, cv: &mut Canvas) {
        let Some((head, body)) = self.results_rects() else { return };
        let f = self.fonts();
        let all = Rect::new(head.x, head.y, head.w, head.h + body.h);
        cv.round(all, 6, CARD_BORDER, 255);
        cv.round(all.inset(1), 5, BAR, 215);
        if self.results_min {
            chevron(cv, head.x + 13, head.y + 15, 1, DIM);
        } else {
            chevron_down(cv, head.x + 9, head.y + 12, DIM);
        }
        cv.text(&f.s, head.x + 24, head.y + 19, &format!("Meas · {}", self.measurement_gate_label()), TEXT);
        let n = self.mstat.first().map(|s| s.n).unwrap_or(0);
        let ns = format!("n {}", fmt_count(n as f64));
        cv.text(&f.s, head.x + head.w - 10 - f.s.width(&ns), head.y + 19, &ns, DIM);
        if self.results_min {
            return;
        }
        let rows: Vec<(usize, tools::MeasRow)> = self.meas_rows().into_iter().enumerate().filter(|(_, r)| !self.meas_row_hidden(r)).collect();
        let pitch = self.results_row(rows.len() as i32);
        let (one, two) = self.lay().res_pitch;
        // baselines: 16 and 35 px into a row at the normal text size
        let (y1, y2) = (one - 6, two - 7);
        let (first, end) = self.results_page_range(rows.len());
        let cols = self.results_cols();
        let cw = body.w / cols as i32;
        for (page_row, (i, r)) in rows.iter().skip(first).take(end - first).enumerate() {
            // the rows of a card docked under or above the plot run in columns
            let body = Rect::new(body.x + (page_row % cols) as i32 * cw, body.y, cw, body.h);
            let by = body.y + (page_row / cols) as i32 * pitch;
            cv.hline(body.x + 8, body.x + body.w - 9, by, LINE);
            let st = self.mstat.get(*i).copied().unwrap_or_default();
            let mean = st.sum / st.n.max(1) as f64;
            let name = self.results_name(*i, r);
            let value = r.text();
            let value_x = body.x + body.w - 10 - f.sb.width(&value);
            let sd = if st.n > 1 {
                let sd = (st.sumsq / st.n as f64 - mean * mean).max(0.0).sqrt();
                format!("\u{3c3} {}", si(sd, r.unit))
            } else { String::new() };
            let sd_x = body.x + body.w - 10 - f.s.width(&sd);
            cv.set_clip(Rect::new(body.x + 10, by, (value_x - body.x - 15).max(1), y1 + 4));
            cv.text(&f.s, body.x + 10, by + y1, &name, LABEL);
            cv.no_clip();
            cv.text(&f.sb, value_x, by + y1, &value, self.wave_color(r.src));
            if st.n > 1 {
                cv.set_clip(Rect::new(body.x + 10, by + y1 + 4, (sd_x - body.x - 15).max(1), pitch - y1 - 4));
                cv.text(&f.s, body.x + 10, by + y2, &format!("avg {}", r.fmt(mean)), DIM);
                cv.no_clip();
                cv.text(&f.s, sd_x, by + y2, &sd, DIM);
            }
        }
        self.draw_results_page_buttons(cv, body, rows.len());
    }

    /// Run state pill at the left of the top bar (wider for the larger text sizes).
    fn run_pill(&self) -> Rect {
        if self.m.layout == Layout::Studio { return self.studio_top().run; }
        Rect::new(8, 6, 92.max(30 + self.fonts().mb.width("TRIG'D") + 12), 28)
    }

    /// Network box at the right of the top bar: room for an address, two lines of text (taller at the larger text
    /// sizes).
    fn net_box(&self) -> Rect {
        let k = self.m.text_size.index() as i32;
        let w = 122.max(28 + self.fonts().sb.width("255.255.255.255"));
        let top = self.frame().top;
        Rect::new(top.right() - METRICS.edge - w, top.y + 5 - 2 * k, w, 30 + 4 * k)
    }

    /// Left of the network box while a WLAN stick is plugged in.
    fn wlan_box(&self) -> Rect {
        let n = self.net_box();
        let w = if self.m.text_size == TextSize::Normal { 104 } else { n.w };
        Rect::new(n.x - 6 - w, n.y, w, n.h)
    }
    /// Start menu button left of the channel cards (all menus and key functions by touch).
    /// The fork's compact bottom bar on the panel; `menu_btn` follows the frame on other screens.
    const MENU_BTN: Rect = Rect::new(4, H - BOTTOM_H + 3, 46, 30);
    const SINGLE_BTN: Rect = Rect::new(104, 5, 54, 30);
    /// Run state pill at the panel's normal text size (`run_pill` grows with the text).
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    const RUN_PILL: Rect = Rect::new(8, 6, 92, 28);
    fn menu_btn(&self) -> Rect {
        self.frame().menu_btn
    }

    /// Single button right of the run state pill (moves over when the pill grows with the text size).
    fn single_btn(&self) -> Rect {
        if self.m.layout == Layout::Studio { return self.studio_top().single; }
        let p = self.run_pill();
        // the Default layout has no Single button (the front-panel key), the caption follows the pill as in stock OOS
        if !self.m.compact() {
            return Rect::new(p.right() + 2, Self::SINGLE_BTN.y, 0, Self::SINGLE_BTN.h);
        }
        Rect::new(p.right() + 4, Self::SINGLE_BTN.y, Self::SINGLE_BTN.w, Self::SINGLE_BTN.h)
    }

    /// Top-bar buttons, right-aligned before the network box: what is used most, and the menus without a front-panel key
    /// (Display, Storage). Everything else is in the start menu: System first in its group, SIM under Analysis; Load file
    /// is a row of the Reference menu.
    fn top_buttons(&self) -> Vec<(TopBtn, &'static str, Rect)> {
        if self.m.layout == Layout::Studio { return self.studio_top_buttons(); }
        let f = self.fonts();
        let mut all = vec![
            (TopBtn::Load, "Load file"),
            (TopBtn::Menu(MenuId::Sim), "SIM"),
            (TopBtn::Menu(MenuId::Measure), "Measure"),
            (TopBtn::Menu(MenuId::Cursor), "Cursor"),
            (TopBtn::Menu(MenuId::Display), "Display"),
            (TopBtn::Menu(MenuId::Storage), "Storage"),
            (TopBtn::System, "System"),
        ];
        if !self.m.compact() {
            // stock OOS buttons; Load and SIM stay in the Sources menu and Menu > Analysis
            all.retain(|(b, _)| !matches!(b, TopBtn::Load | TopBtn::Menu(MenuId::Sim)));
        }
        if self.m.compact() {
            // Measure and Cursor are keys of the front panel (Measure opens Add measurements), and entries of the Menu
            all.retain(|(b, _)| !matches!(b, TopBtn::Menu(MenuId::Measure | MenuId::Cursor)));
        }
        all.retain(|(b, _)| match b {
            TopBtn::Menu(m) => !self.menu_left_out(*m),
            _ => true,
        });
        if self.record_mode() {
            all.insert(0, (TopBtn::Live, "Live"));
        }
        let required = self.single_btn().right() + 8 + f.s.width(&self.acq_caption().0) + 12;
        let fits = |items: &[(TopBtn, &'static str)], padding: i32| {
            let width = items.iter().map(|(_, label)| (f.mb.width(label) + padding).max(40)).sum::<i32>()
                + items.len().saturating_sub(1) as i32 * 6;
            self.top_right() - 8 - width >= required
        };
        let mut padding = 28;
        if !fits(&all, padding) {
            if let Some((_, label)) = all.iter_mut().find(|(b, _)| *b == TopBtn::Load) { *label = "Load"; }
        }
        if !fits(&all, padding) { padding = 20; }
        for (button, short) in [(TopBtn::Menu(MenuId::Storage), "Store"), (TopBtn::Menu(MenuId::Display), "Disp"),
            (TopBtn::Menu(MenuId::Measure), "Meas"), (TopBtn::System, "Sys")] {
            if fits(&all, padding) { break; }
            if let Some((_, label)) = all.iter_mut().find(|(b, _)| *b == button) { *label = short; }
        }
        let mut x = self.top_right() - 8;
        let mut out = Vec::new();
        for &(b, l) in all.iter().rev() {
            let w = (f.mb.width(l) + padding).max(40);
            x -= w;
            out.push((b, l, Rect::new(x, 5, w, 30)));
            x -= 6;
        }
        out.reverse();
        out
    }

    fn top_acq_caption(&self, width: i32) -> (String, bool) {
        let (caption, live) = self.acq_caption();
        if !live && self.fonts().s.width(&caption) > width {
            if let Some(r) = self.refs.get(self.active_reference()).and_then(Option::as_ref) {
                return (format!("{} · {}", if self.session_is_sim { "SIM" } else { "REF" }, si_digits(r.v.len() as f64, "pts", 3)), false);
            }
        }
        (caption, live)
    }

    /// While a USB stick is mounted: left of the network and WLAN boxes; the top-bar buttons move over for it.
    fn usb_box(&self) -> Rect {
        let next = if self.wlan_stick { self.wlan_box().x } else { self.net_box().x };
        Rect::new(next - 52, 5, 46, 30)
    }

    /// Where the buttons of the top bar end: the leftmost of the boxes on the right.
    fn top_right(&self) -> i32 {
        match (self.usb, self.wlan_stick) {
            (true, _) => self.usb_box().x,
            (false, true) => self.wlan_box().x,
            _ => self.net_box().x,
        }
    }

    /// A box of the top bar with a state dot and two lines.
    fn draw_link(&self, cv: &mut Canvas, r: Rect, l1: &str, addr: Option<&str>) {
        let f = self.fonts();
        cv.round(r, 5, if self.menu_open && self.menu_root() == MenuId::Network { ACCENT } else { BTN_BORDER }, 255);
        cv.round(r.inset(1), 4, BG, 255);
        // first line under the top edge, the second on the bottom edge
        let (cap, y1) = (f.s.size * 72 / 100, r.y + 5 + f.s.size * 72 / 100);
        cv.circle(r.x + 11, y1 - cap / 2 + 1, 3, if addr.is_some() { GREEN } else { DIM });
        cv.text(&f.s, r.x + 20, y1, l1, DIM);
        cv.text(&f.sb, r.x + 20, r.y + r.h - 4, addr.unwrap_or("not connected"), TEXT);
    }

    /// The address of an interface, if it has one.
    fn addr_of(&self, name: &str) -> Option<&str> {
        self.net_ifs.iter().find(|i| i.0 == name).map(|i| i.1.as_str())
    }

    /// Touch area of a top-bar button: the full bar height.
    fn top_hit(&self, r: Rect) -> Rect {
        let top = self.frame().top;
        Rect::new(r.x - 2, top.y, r.w + 4, top.h)
    }

    /// The acquisition mode as the top bar names it.
    fn acq_name(&self) -> String {
        match self.m.acq {
            AcqMode::Normal => "Normal".to_string(),
            AcqMode::Peak => "Peak".to_string(),
            AcqMode::Average if self.src.is_hw() && board().avg_hw(&self.m) => format!("Average {} HW", self.m.avg_count),
            AcqMode::Average => format!("Average {}", self.m.avg_count),
            AcqMode::HiRes => "High res".to_string(),
        }
    }

    /// The acquisition text of the top bar from `x` on, before the buttons at `room`: sample rate and depth in the bold
    /// font, else in the regular one, else without the spaces; the mode only when it fits too. Larger text with a long
    /// depth ran under the buttons. (bold, rate and depth, mode)
    fn top_acq(f: &Fonts, x: i32, room: i32, sr: f64, depth: f64, mode: String) -> (bool, String, Option<String>) {
        let (rate, pts) = (si_digits(sr, "Sa/s", 3), si_digits(depth, "pts", 3));
        let full = format!("{} \u{b7} {}", rate, pts);
        let (bold, main) = if x + f.mb.width(&full) <= room {
            (true, full)
        } else if x + f.m.width(&full) <= room {
            (false, full)
        } else {
            // tighter, then the rate alone (large text with the view buttons, a USB stick, a WLAN stick)
            let tight = format!("{}\u{b7}{}", rate.replace(' ', ""), pts.replace(' ', ""));
            let alone = rate.replace(' ', "");
            let main = [tight, alone].into_iter().find(|t| x + f.m.width(t) <= room).unwrap_or_default();
            (false, main)
        };
        let end = x + if bold { f.mb.width(&main) } else { f.m.width(&main) };
        let mode = format!(" \u{b7} {}", mode);
        let mode = (end + f.m.width(&mode) <= room).then_some(mode);
        (bold, main, mode)
    }

    fn draw_top(&self, cv: &mut Canvas) {
        if self.m.layout == Layout::Studio { return self.draw_studio_top(cv); }
        let f = self.fonts();
        let top = self.frame().top;
        cv.fill(top, BAR);
        cv.hline(top.x, top.right() - 1, top.bottom() - 1, LINE);
        // run / trigger state; offline mode reads the record (playing or held) instead of a trigger
        let (txt, col) = self.run_caption();
        let r = self.run_pill();
        cv.round(r, 6, mix(BAR, col, 80), 255);
        cv.round(r.inset(1), 5, mix(BAR, col, 36), 255);
        cv.circle(r.x + 16, r.y + 14, 4, col);
        cv.text_in(&f.mb, Rect::new(r.x + 30, r.y, r.w - 30, r.h), 0, txt, col);
        let single = self.single_btn();
        if single.w > 0 {
            btn(cv, single, BTN, BTN_BORDER);
            cv.text_in(&f.s, single, 1, "Single", TEXT);
        }
        // acquisition: sample rate and depth of the hardware when it runs, else of the model; mode.
        // Offline mode shows the reference's own rate and length, with no acquire mode.
        let buttons = self.top_buttons();
        let room = self.view_box().map_or(buttons.first().map_or(self.top_right(), |b| b.2.x), |v| v.x) - 12;
        let x0 = single.right() + 8;
        let (caption, live_acq) = self.top_acq_caption((room - x0).max(1));
        let acq_open = self.menu_open && self.menu_root() == if self.record_mode() { MenuId::Ref } else { MenuId::Acquire };
        let tcol = if acq_open { ACCENT } else { TEXT };
        cv.set_clip(Rect::new(x0, top.y, (room - x0).max(1), top.h));
        if live_acq {
            let (sr, depth) = self.live_rate_depth();
            let (bold, main, mode) = Self::top_acq(f, x0, room, sr, depth as f64, self.acq_name());
            let x = cv.text(if bold { &f.mb } else { &f.m }, x0, 25, &main, tcol);
            if let Some(m) = mode {
                cv.text(&f.m, x, 25, &m, tcol);
            }
        } else {
            cv.text(&f.s, x0, 25, &caption, tcol);
        }
        cv.no_clip();
        self.draw_view_box(cv);
        // buttons (the open menu in orange)
        let root = self.menu_root();
        for (b, l, r) in buttons {
            let hl = match b {
                TopBtn::Live => true,
                TopBtn::Menu(m) => self.menu_open && root == m,
                TopBtn::System => self.menu_open && root == MenuId::Utility,
                TopBtn::Load => false,
            };
            btn(cv, r, BTN, if hl { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.mb, r, 1, l, if hl { ACCENT } else { TEXT });
        }
        // LAN: an address without a cable is only the fallback waiting for one
        let lan = self.addr_of("eth0").filter(|_| self.eth_link);
        self.draw_link(cv, self.net_box(), if lan.is_none() { "LAN" } else if self.net_fixed { "LAN \u{b7} local" } else { "LAN \u{b7} DHCP" }, lan);
        // WLAN and USB sticks, while they are there
        if self.wlan_stick {
            self.draw_link(cv, self.wlan_box(), "WLAN", self.addr_of(oos_hal::wlan::IFACE));
        }
        if self.usb {
            let r = self.usb_box();
            cv.round(r, 5, if self.menu_open && self.menu_root() == MenuId::Storage { ACCENT } else { BTN_BORDER }, 255);
            cv.round(r.inset(1), 4, BG, 255);
            cv.circle(r.x + 9, r.y + 15, 3, GREEN);
            cv.text(&f.sb, r.x + 16, r.y + 20, "USB", TEXT);
        }
    }

    /// Messages as an amber pill at the top centre of the plot; next to or under an open menu dialog that would
    /// cover it (it never hides dialog controls).
    /// A message that reports something done: it gets a tick, not the warning triangle.
    /// A message that only informs (nothing to clear, which view, a channel turned on): a neutral pill with an "i".
    pub(super) fn toast_is_info(s: &str) -> bool {
        s == NOTHING_TO_CLEAR || s == "Views off" || s.ends_with(" turned on") || (s.starts_with("View ") && s.contains(" of "))
    }

    pub(super) fn toast_is_success(s: &str) -> bool {
        matches!(s, "Eye cleared" | "Counter cleared" | "Limit cleared" | "Counts reset" | "Mask cleared") || s.starts_with("Copied ") || (s.starts_with("Zone ") && s.ends_with(" deleted")) || s.ends_with(" added") || s.ends_with(" removed") || s.contains(" \u{b7} ") || s.starts_with("Loaded ") || s.starts_with("Saved ") || s.contains(" loaded from ") || s.starts_with("Restored ") || s.contains(" saved to ") || s.starts_with("Returned to live") || s.starts_with("Fit ")
    }

    fn draw_toast(&self, cv: &mut Canvas) {
        let Some((s, _)) = &self.toast else { return };
        let f = self.fonts();
        let tw = f.mb.width(s) + 44;
        let body = self.frame().body;
        let mut r = Rect::new(body.x + body.w / 2 - tw / 2, body.y + 4, tw, 28);
        if self.eye_open() {
            let pane = self.eye_pane_rect();
            if overlaps(pane, r) {
                let wave = self.grid;
                let room = pane.x - wave.x - 12;
                if room >= tw {
                    r.x = wave.x + (room - tw) / 2;
                }
            }
        }
        let dlg = self.menu_rect().filter(|d| overlaps(*d, r));
        if let Some(d) = dlg {
            let (left, right) = (d.x - 6, body.right() - 6 - d.right());
            if right >= tw + 16 {
                r.x = d.x + d.w + (right - tw) / 2;
            } else if left >= tw + 16 {
                r.x = 6 + (left - tw) / 2;
            } else {
                r.y = d.y + d.h + 6;
                // never over the bottom bar: above the dialog when there is room, else where it was
                if r.y + r.h > self.frame().bottom.y { r.y = if d.y - r.h - 6 >= body.y { d.y - r.h - 6 } else { body.y + 4 }; }
            }
        }
        if Self::toast_is_info(s) {
            // information, not a warning: a neutral pill with an "i"
            cv.round(r, 6, mix(BAR, TEXT, 80), 255);
            cv.round(r.inset(1), 5, mix(BAR, TEXT, 20), 255);
            let (cx, cy) = (r.x + 18, r.y + 14);
            cv.circle(cx, cy, 8, TEXT);
            cv.circle(cx, cy, 7, mix(BAR, TEXT, 20));
            cv.pixel(cx, cy - 4, TEXT, 255);
            cv.vline(cx, cy - 1, cy + 4, TEXT);
            cv.text(&f.mb, r.x + 32, r.y + 19, s, TEXT);
            return;
        }
        cv.round(r, 6, mix(BAR, AMBER, 110), 255);
        cv.round(r.inset(1), 5, mix(BAR, AMBER, 34), 255);
        if Self::toast_is_success(s) {
            // done, not a warning: a tick in green
            cv.round(r, 6, mix(BAR, GREEN, 110), 255);
            cv.round(r.inset(1), 5, mix(BAR, GREEN, 34), 255);
            for d in [0, 1] { cv.line(r.x + 11, r.y + 15 + d, r.x + 16, r.y + 20 + d, GREEN); cv.line(r.x + 16, r.y + 20 + d, r.x + 25, r.y + 8 + d, GREEN); }
            cv.text(&f.mb, r.x + 32, r.y + 19, s, GREEN);
            return;
        }
        cv.triangle([(r.x + 11, r.y + 20), (r.x + 25, r.y + 20), (r.x + 18, r.y + 7)], AMBER);
        cv.vline(r.x + 18, r.y + 11, r.y + 15, DARK_TEXT);
        cv.pixel(r.x + 18, r.y + 18, DARK_TEXT, 255);
        cv.text(&f.mb, r.x + 32, r.y + 19, s, AMBER);
    }

    /// The menu a sub menu or picker belongs to (button and card highlights, dialog position).
    fn menu_root(&self) -> MenuId {
        let m = match (self.menu, self.pick) {
            (MenuId::Pick, Some(p)) => p.menu,
            (m, _) => m,
        };
        if matches!(m, MenuId::MeasAdd | MenuId::MeasHold | MenuId::MeasSelected) {
            MenuId::Measure
        } else if m == MenuId::SimAdvanced {
            MenuId::Sim
        } else {
            m
        }
    }

    /// Bottom cards after the Menu button: channels (wide when on, up to 213 px), TIME (at least 130 px), trigger
    /// (at least 222 px).
    fn cards(&self) -> ([Rect; NCH], Rect, Rect) {
        let c = self.card_layout();
        (c.ch, c.time, c.trig)
    }

    /// The cards of the logic analyser and the generator, where the model has them.
    fn aux_cards(&self) -> (Option<Rect>, Option<Rect>) {
        let c = self.card_layout();
        (c.la, c.gen)
    }

    /// All bottom cards. LA and generator sit behind the channels, wide like them when they are on and the channels keep
    /// room for their text, else narrow.
    fn card_layout(&self) -> Cards {
        if self.m.layout == Layout::Studio { self.studio_cards() } else if self.m.compact() { self.compact_cards() } else { self.classic_card_layout() }
    }

    /// The stock OOS bottom row (Display > Layout > Default).
    fn classic_card_layout(&self) -> Cards {
        let row = self.frame().cards;
        let (y, h) = (row.y, row.h);
        // 2-channel models: no cards for CH3/CH4 (zero width, never drawn or hit)
        let n = n_ch();
        let n_on = (0..n).filter(|&c| self.m.ch[c].on).count() as i32;
        let n_off = n as i32 - n_on;
        let caps = &oos_hal::model::platform().caps;
        let aux = [(caps.la, self.m.la.on), (caps.has_gen() && !self.m.gen_hidden, self.m.afg_on)];
        let ref_card = !self.record_mode() && !self.shown_refs().is_empty();
        let start = row.x;
        let room = |wide: bool| {
            let a: i32 = aux.iter().filter(|a| a.0).map(|a| if a.1 && wide { AUX_WIDE } else { AUX_NARROW } + 7).sum::<i32>()
                + if ref_card { AUX_NARROW + 7 } else { 0 };
            row.right() - start - n_off * (56 + 7) - n_on * 7 - a - (130 + 7) - 222
        };
        let wide = n_on == 0 || room(true) / n_on.max(1) >= CH_MIN;
        let won = if n_on > 0 { (room(wide) / n_on).min(213) } else { 0 };
        let mut x = start;
        let mut ch = [Rect::new(0, 0, 0, 0); NCH];
        for (c, r) in ch.iter_mut().enumerate() {
            if c >= n {
                *r = Rect::new(x, y, 0, h);
                continue;
            }
            let w = if self.m.ch[c].on { won } else { 56 };
            *r = Rect::new(x, y, w, h);
            x += w + 7;
        }
        let mut next = |(has, on): (bool, bool)| {
            has.then(|| {
                let w = if on && wide { AUX_WIDE } else { AUX_NARROW };
                let r = Rect::new(x, y, w, h);
                x += w + 7;
                r
            })
        };
        let (la, gen) = (next(aux[0]), next(aux[1]));
        let refs = ref_card.then(|| { let r = Rect::new(x, y, AUX_NARROW, h); x += AUX_NARROW + 7; r });
        let rest = row.right() - x - 7;
        let tw = ((rest as f32 * 0.42) as i32).min(rest - 222).max(130);
        let tw = tw.min(TIME_CARD_MAX);
        let trw = (rest - tw).min(TRIG_CARD_MAX);
        // a wide screen: TIME and trigger keep their largest size and move to the right edge, the room stays between
        // the channels and them (as in the top bar)
        let x = row.right() - trw - 7 - tw;
        Cards { ch, la, gen, refs, time: Rect::new(x, y, tw, h), trig: Rect::new(x + tw + 7, y, trw, h) }
    }

    /// The big digits of a card while `s` fits `room`, else the next size: nothing is cut off with every card on.
    fn card_font(f: &'static Fonts, s: &str, room: i32) -> &'static Font {
        fit_font(&[&f.l, &f.m, &Fonts::get(0).m], room, s)
    }

    /// The line under V/div of a channel card: coupling, probe, bandwidth, invert, offset.
    fn card_sub(&self, c: usize) -> String {
        let ch = &self.m.ch[c];
        let mut sub = vec![ch.coupling.name().to_string(), format!("{}\u{d7}", ch.probe)];
        if ch.bw != 0 && ch.bw != BW_FULL {
            sub.push(format!("BW {}", BWS[ch.bw].0));
        }
        if ch.invert {
            sub.push("Inv".into());
        }
        sub.push(si_short(ch.offset as f64, ch.unit.name()));
        sub.join(" \u{b7} ")
    }

    /// Font of that line, one for all channel cards: the normal size when a line would run out of its card at the
    /// text size.
    fn card_sub_font(&self) -> &'static Font {
        let (f, chs) = (self.fonts(), self.cards().0);
        let all_fit = (0..NCH).filter(|&c| chs[c].w > 0 && self.m.ch[c].on).all(|c| f.s.width(&self.card_sub(c)) <= chs[c].w - 22);
        if all_fit { &f.s } else { &Fonts::get(0).s }
    }

    /// Trigger card buttons, one over the other at the right edge (touch): source, sweep; the rest of the card, and with
    /// it the level, opens the menu.
    pub(super) fn trig_parts(r: Rect) -> (Rect, Rect) {
        let (w, h) = (66, (r.h - 12 - 4) / 2);
        let x = r.x + r.w - w - 6;
        (Rect::new(x, r.y + 6, w, h), Rect::new(x, r.y + 6 + h + 4, w, h))
    }

    /// A card like the channels': on, wide with its values; on, narrow with a full dot; off, with a ring.
    #[allow(clippy::too_many_arguments)]
    /// The refs that are shown over the live channels, in slot order.
    pub(super) fn shown_refs(&self) -> Vec<usize> {
        (0..NREF).filter(|&i| self.refs[i].as_ref().is_some_and(|r| r.on)).collect()
    }

    /// REF card of the Default bottom row: the active ref; a tap makes the next shown ref the active source.
    fn draw_ref_card(&self, cv: &mut Canvas, r: Rect) {
        let f = self.fonts();
        let shown = self.shown_refs();
        let active = shown.iter().copied().find(|&i| self.selected_workspace_source() == Some(WaveId::Ref(i)));
        let col = active.map_or(LABEL, |i| self.wave_color(WaveId::Ref(i)));
        cv.round(r, 7, if active.is_some() { col } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.set_clip(r.inset(3));
        cv.text_in(&f.sb, Rect::new(r.x, r.y + 8, r.w, 18), 1, "REF", if active.is_some() { col } else { LABEL });
        let name = active.map_or_else(|| format!("{} on", shown.len()), |i| format!("R{}", i + 1));
        cv.text_in(&f.mb, Rect::new(r.x, r.y + 30, r.w, 24), 1, &name, TEXT);
        cv.no_clip();
    }

    /// Makes the next shown ref the active source (cursors, measurements and the cards follow it).
    pub(super) fn cycle_active_ref(&mut self) {
        let shown = self.shown_refs();
        let Some(&first) = shown.first() else { return };
        let next = match self.selected_workspace_source() {
            Some(WaveId::Ref(i)) => shown.iter().copied().find(|&k| k > i).unwrap_or(first),
            _ => first,
        };
        self.select_workspace_source(WaveId::Ref(next));
        self.toast(&format!("Active source R{}", next + 1));
    }

    fn draw_aux_card(&self, cv: &mut Canvas, r: Rect, name: &str, col: Rgb, on: bool, sel: bool, big: &str, sub: &str) {
        let f = self.fonts();
        cv.round(r, 7, if sel || on { col } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.set_clip(r.inset(3));
        if on && r.w > AUX_NARROW {
            cv.fill(Rect::new(r.x + 6, r.y + 10, 3, r.h - 20), col);
            let (yn, yb, ys) = self.lay().card;
            cv.text(&f.sb, r.x + 16, r.y + yn, name, col);
            cv.circle(r.x + r.w - 16, r.y + 14, 5, col);
            cv.text(fit_font(&[&f.mb, &Fonts::get(0).mb], r.w - 22, big), r.x + 16, r.y + yb, big, TEXT);
            cv.text(fit_font(&[&f.s, &Fonts::get(0).s], r.w - 22, sub), r.x + 16, r.y + ys, sub, DIM);
        } else {
            cv.text_in(&f.sb, Rect::new(r.x, r.y + 8, r.w, 18), 1, name, if on { col } else { LABEL });
            let (cx, cy) = (r.x + r.w / 2, r.y + 45);
            cv.circle(cx, cy, 7, col);
            if !on {
                cv.circle(cx, cy, 6, BAR);
            }
        }
        cv.no_clip();
    }

    fn draw_bottom(&self, cv: &mut Canvas) {
        if self.m.layout == Layout::Studio { self.draw_studio_bottom(cv) } else if self.m.compact() { self.draw_workspace_footer(cv) } else { self.draw_classic_bottom(cv) }
    }

    /// The stock OOS cards: Menu, channels, LA, generator, TIME and trigger (Display > Layout > Default).
    fn draw_classic_bottom(&self, cv: &mut Canvas) {
        let f = self.fonts();
        let (yn, yb, ys) = self.lay().card;
        // a line that would run out of a narrow card takes the normal size
        let small = |s: &str, room: i32| fit_font(&[&f.s, &Fonts::get(0).s], room, s);
        cv.fill(self.frame().bottom, BG);
        let root = if self.menu_open { Some(self.menu_root()) } else { None };
        // start menu button: three bars and "Menu"; amber "Locked" while touch is locked (a tap unlocks)
        let r = self.menu_btn();
        let (hl, lock) = (root == Some(MenuId::Main), self.m.touch_lock);
        cv.round(r, 7, if hl { ACCENT } else if lock { AMBER } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        let col = if hl { ACCENT } else if lock { AMBER } else { TEXT };
        for k in 0..3 {
            cv.fill(Rect::new(r.x + r.w / 2 - 11, r.y + 16 + k * 7, 22, 3), col);
        }
        cv.text_in(&f.sb, Rect::new(r.x, r.y + 42, r.w, 16), 1, if lock { "Locked" } else { "Menu" }, col);
        let (chs, tr, trg) = self.cards();
        let sub_font = self.card_sub_font();
        for (c, &r) in chs.iter().enumerate() {
            if r.w == 0 {
                continue;
            }
            let ch = &self.m.ch[c];
            let col = CH_COLORS[c];
            let sel = root == Some(MenuId::Channel(c));
            let border = if ch.on && (self.m.active == c || sel) { col } else { CARD_BORDER };
            cv.round(r, 7, border, 255);
            cv.round(r.inset(2), 5, BAR, 255);
            // narrow cards: the text stays inside
            cv.set_clip(r.inset(3));
            let name = self.ch_name(c);
            if ch.on {
                cv.fill(Rect::new(r.x + 6, r.y + 10, 3, r.h - 20), col);
                cv.text(&f.sb, r.x + 16, r.y + yn, &name, col);
                cv.circle(r.x + r.w - 16, r.y + 14, 5, col);
                let scale = format!("{}/div", si_short(ch.volts_per_div() as f64, ch.unit.name()));
                cv.text(Self::card_font(f, &scale, r.w - 24), r.x + 16, r.y + yb, &scale, TEXT);
                cv.text(sub_font, r.x + 16, r.y + ys, &self.card_sub(c), DIM);
            } else {
                cv.text_in(&f.sb, Rect::new(r.x, r.y + 8, r.w, 18), 1, &name, LABEL);
                let (cx, cy) = (r.x + r.w / 2, r.y + 45);
                cv.circle(cx, cy, 7, col);
                cv.circle(cx, cy, 6, BAR);
            }
            cv.no_clip();
        }
        // logic analyser and generator, in the manner of the channels
        let (la, gen) = self.aux_cards();
        if let Some(r) = la {
            let sel = matches!(root, Some(MenuId::La | MenuId::LaLines | MenuId::LaPat));
            let l = &self.m.la;
            let sub = format!("{} \u{b7} {}", si_short(l.thr(0) as f64, "V"), si_short(l.thr(1) as f64, "V"));
            // grey without a probe: nothing to switch on
            let col = if l.on || self.la_probe_ok() { la::line_color(0) } else { CARD_BORDER };
            self.draw_aux_card(cv, r, "LA", col, l.on, sel, &format!("{} lines", l.lines().len()), &sub);
        }
        if let Some(r) = gen {
            let (big, sub) = self.afg_card_text();
            self.draw_aux_card(cv, r, "GEN", GEN_COLOR, self.m.afg_on, root == Some(MenuId::Afg), &big, &sub);
        }
        if let Some(r) = self.card_layout().refs { self.draw_ref_card(cv, r); }
        // time base (sample rate and depth are in the top bar)
        let r = tr;
        let sel = root == Some(MenuId::Horizontal);
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.text(&f.sb, r.x + 12, r.y + yn, "TIME", LABEL);
        let tdiv = format!("{}/div", si_short(self.m.time_per_div() as f64, "s"));
        cv.text(Self::card_font(f, &tdiv, r.w - 20), r.x + 12, r.y + yb, &tdiv, TEXT);
        if self.m.zoom {
            cv.text(&f.sb, r.x + 12, r.y + ys, &format!("zoom {}/div", si_short(tscales()[self.m.zscale] as f64, "s")), ACCENT);
        } else {
            let t = format!("delay {}", si_short(self.m.delay as f64, "s"));
            cv.text(small(&t, r.w - 18), r.x + 12, r.y + ys, &t, DIM);
        }
        // trigger: type, slope / polarity / standard and level; source and sweep buttons
        let r = trg;
        let sel = root == Some(MenuId::Trigger);
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        // type and slope on the first line, the level under them: it has the width up to the buttons
        let tx = cv.text(&f.sb, r.x + 12, r.y + yn, &self.m.trig_type.name().to_uppercase(), LABEL) + 8;
        if matches!(self.m.trig_type, TrigType::Edge | TrigType::Slope | TrigType::Timeout | TrigType::NthEdge | TrigType::Delay) {
            draw_slope(cv, tx, r.y + 6, self.m.slope, ACCENT);
        } else {
            let tag = match self.m.trig_type {
                TrigType::Pulse | TrigType::Runt => if self.m.trig_pol == Pol::Pos { "+" } else { "-" },
                TrigType::Video => if self.m.video_std == VideoStd::Pal { "P" } else { "N" },
                _ => "",
            };
            cv.text(&f.sb, tx, r.y + yn, tag, ACCENT);
        }
        // an LA line has the pod's threshold, not a level of its own
        let lv = match self.m.trig_line() {
            Some(n) => si(self.m.la.thr(n as usize / 8) as f64, "V"),
            None => si(self.trig_level_shown() as f64, self.m.unit(self.m.trig_src)),
        };
        let (sb, mb) = Self::trig_parts(r);
        // the big digits while they fit
        cv.text(Self::card_font(f, &lv, sb.x - r.x - 18), r.x + 12, r.y + 50, &lv, TEXT);
        let mode = match self.m.trig_mode {
            TrigMode::Auto => "Auto",
            TrigMode::Normal => "Normal",
            TrigMode::Single => "Single",
        };
        let ts = self.m.trig_src;
        let (src, scol) = match self.m.trig_line() {
            Some(n) => (format!("D{}", n), la::line_color(n)),
            None => (format!("CH{}", ts + 1), CH_COLORS[ts]),
        };
        for (b, val, col) in [(sb, src, scol), (mb, mode.to_string(), TEXT)] {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.sb, b, 1, &val, col);
        }
    }

    /// Front-panel LED mask (19 LEDs, stock order; `Inputs::set_leds` maps it to
    /// the MCU's hardware bits):
    /// 0-3 CH1-CH4, 4 RUN green, 5 STOP red, 6 SINGLE red, 7 SINGLE orange, 8 LA, 9 G1, 10 G2, 11 TOUCH,
    /// 12 A up/down, 13 B left/right, 14 SLOPE1, 15 SLOPE2, 16 A left/right, 17 B up/down, 18 INTENSITY.
    pub fn leds(&self) -> u32 {
        let mut m = 0u32;
        for c in 0..NCH {
            if self.m.ch[c].on {
                m |= 1 << c;
            }
        }
        if self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run {
            m |= (1 << 6) | (1 << 7); // the stock system always drives both SINGLE LEDs together
        } else if self.m.run == RunState::Run {
            m |= 1 << 4;
        } else {
            m |= 1 << 5;
        }
        if self.m.touch_lock {
            m |= 1 << 11;
        }
        match self.m.slope {
            Slope::Rise => m |= 1 << 14,
            Slope::Fall => m |= 1 << 15,
            Slope::Either => m |= (1 << 14) | (1 << 15),
        }
        m
    }

    // ------------------------------------------------------------------ menus

    fn items(&self, menu: MenuId) -> Vec<Item> {
        if let Some(v) = self.features_items(menu) {
            return v;
        }
        let mut v = self.menu_items(menu);
        if !self.off_sources(menu).is_empty() {
            v.push(Item::SrcOn);
        }
        v
    }

    /// Channels a page reads that are switched off: the page has nothing to work with, and says so with a button.
    /// A ref is stored memory, so a switched-off channel does not blank it.
    fn off_sources(&self, menu: MenuId) -> Vec<usize> {
        let m = &self.m;
        let ch = WaveId::Ch;
        let want: Vec<WaveId> = match menu {
            MenuId::Power if matches!(m.pw_mode, PowerMode::Ripple | PowerMode::Rail) => vec![m.pw_v],
            MenuId::Power => vec![m.pw_v, m.pw_i],
            MenuId::Spectrum => vec![m.spec_src],
            MenuId::Eye => vec![m.eye_src],
            MenuId::Bode if m.bode_gen == bode::FREE => [Some(m.bode_out), m.bode_sw_in.checked_sub(1).map(ch), Some(ch(m.bode_sync))].into_iter().flatten().collect(),
            MenuId::Bode => vec![m.bode_in, m.bode_out],
            MenuId::Magnetic => vec![m.mag_v, m.mag_i],
            MenuId::Inverter if m.inv_page == InvPage::Dq0 => {
                let mut v = vec![m.inv_a, m.inv_b, m.inv_c];
                if m.inv_ang == InvAngle::Analog {
                    v.push(m.inv_an);
                }
                v
            }
            MenuId::Inverter => {
                let mut v = match m.inv_ang {
                    InvAngle::Analog => vec![m.inv_an],
                    InvAngle::Quad | InvAngle::Hall | InvAngle::Off => Vec::new(),
                    InvAngle::Phase => vec![m.inv_a, m.inv_b, m.inv_c],
                };
                v.extend(m.inv_tq);
                v.extend(m.inv_ev);
                v.extend(m.inv_ei);
                v
            }
            MenuId::Dpt => {
                let mut v = vec![m.dpt_v, m.dpt_i, m.dpt_g];
                if m.dpt_4th != DptFourth::Off {
                    v.push(m.dpt_4ch);
                }
                v
            }
            MenuId::BwTest => vec![ch(m.bw_src)],
            MenuId::Mask => vec![ch(m.mask_src)],
            MenuId::Dvm if self.offline => Vec::new(),
            MenuId::Dvm => [m.dvm_on.then_some(ch(m.dvm_src)), m.cnt_on.then_some(ch(m.cnt_src))].into_iter().flatten().collect(),
            MenuId::Decode if !m.dec_on_la() => decode_wave(m.dec_src).into_iter().collect(),
            _ => Vec::new(),
        };
        let mut v: Vec<usize> = want.into_iter().filter_map(|id| match id {
            WaveId::Ch(c) if c < n_ch() && !m.ch[c].on => Some(c),
            _ => None,
        }).collect();
        v.dedup();
        v
    }

    fn menu_items(&self, menu: MenuId) -> Vec<Item> {
        match menu {
            MenuId::Main => self.main_groups().into_iter().flat_map(|g| g.1).collect(),
            MenuId::Channel(c) => vec![Item::ChOn(c), Item::ChScale(c), Item::ChOffset(c), Item::ChCoupling(c), Item::ChBw(c), Item::ChProbe(c), Item::ChInvert(c), Item::ChFine, Item::ChLabel(c), Item::Open(MenuId::ChMore(c))],
            MenuId::ChMore(c) => vec![Item::ChUnit(c), Item::ChBias(c), Item::ChBiasZero(c), Item::ChSkew(c), Item::AutoDeskew, Item::ChExpand],
            MenuId::Horizontal => {
                let mut v = if self.m.zoom {
                    vec![Item::HScale, Item::HFine, Item::HExpandI, Item::HDelay, Item::HZero, Item::AutoDeskew, Item::ClearDeskew, Item::Zoom, Item::ZScale, Item::ZDelay, Item::AutoRoll]
                } else {
                    vec![Item::HScale, Item::HFine, Item::HExpandI, Item::HDelay, Item::HZero, Item::AutoDeskew, Item::ClearDeskew, Item::Zoom, Item::AutoRoll]
                };
                // Studio: where the zoom controls are is a setting of the zoom, on its tab
                if self.m.layout == Layout::Studio { let at = v.iter().position(|i| *i == Item::AutoRoll).unwrap_or(v.len()); v.insert(at, Item::ZoomBar); }
                v
            }
            MenuId::Trigger => self.trigger_items(),
            // the Horizontal Menu key opens Acquire; time base and zoom one level down
            MenuId::Acquire => {
                let mut v = vec![Item::AcqMode];
                if self.m.acq == AcqMode::Average {
                    v.push(Item::AcqAvg);
                    if self.src.is_hw() {
                        v.push(Item::AcqAvgHw);
                    }
                }
                v.push(Item::AcqDepth);
                v.push(Item::AcqSeg);
                if self.m.segments > 0 {
                    v.push(Item::AcqSegHw);
                }
                if self.seg_hw_on() {
                    v.extend([Item::AcqSegIvl, Item::AcqSegTable]);
                }
                v.push(Item::AcqCores);
                v.push(Item::AcqDrawPath);
                if self.alias_above().is_some() {
                    v.push(Item::AcqAlias);
                }
                v.push(Item::Open(MenuId::Horizontal));
                v
            }
            MenuId::MeasPair => (0..tools::PAIRS.len()).map(Item::MeasPair).collect(),
            MenuId::MeasLevels => vec![Item::MeasLvType, Item::MeasRefs, Item::MeasLow, Item::MeasMid, Item::MeasHigh, Item::MeasHyst, Item::MeasBaseTop, Item::Open(MenuId::MeasFall)],
            MenuId::MeasFall => vec![Item::MeasSepFall, Item::MeasFallLow, Item::MeasFallMid, Item::MeasFallHigh],
            MenuId::Dvm => vec![Item::DvmOn, Item::DvmSrc, Item::DvmMode, Item::CntOn, Item::CntSrc, Item::CntMode, Item::CntDigits, Item::CntClear],
            MenuId::Measure => { let mut v = vec![Item::MeasSrc, Item::MeasSrcB, Item::MeasCopyTo, Item::MeasGate, Item::Open(MenuId::MeasPair), Item::Open(MenuId::Dvm), Item::Open(MenuId::MeasLevels), Item::MeasIdle, Item::MeasTsetup, Item::MeasThold, Item::MeasOutside, Item::MeasN, Item::MeasEdges, Item::Open(MenuId::MeasStat), Item::MeasLabels, Item::StripDock, Item::MeasClear]; if self.m.layout == Layout::Studio {
                v.retain(|i| *i != Item::StripDock);
                let g = v.iter().position(|i| *i == Item::MeasGate).unwrap_or(0);
                v.insert(g + 1, Item::BadgeStyle);
            }
            if self.sheets_on() {
                // the sheet has the levels on a tab of their own; the stock layout opens them from one row
                let at = v.iter().position(|i| *i == Item::Open(MenuId::MeasLevels)).unwrap_or(0);
                v.splice(at..=at, self.items(MenuId::MeasLevels));
            }
            v }
            MenuId::MeasHold => vec![Item::AddMeas],
            MenuId::MeasRow => self.measrow_items(),
            MenuId::MeasStat => vec![Item::MeasStats, Item::MeasPop, Item::MeasShowSd, Item::MeasShowMin, Item::MeasShowMax, Item::MeasStatReset],
            MenuId::Decode => self.decode_items(),
            MenuId::Bus => self.bus_items(),
            MenuId::Logger => vec![Item::LogOn, Item::LogIvl, Item::LogInfo],
            MenuId::Ref => vec![Item::RefSrc, Item::RefSave, Item::RefSaveNext, Item::RefClear, Item::RefSel, Item::RefVpd, Item::RefOffset, Item::RefColor, Item::RefLabel, Item::RefLoad, Item::RefExport, Item::RefImport].into_iter().filter(|&it| it != Item::RefLoad || self.feature_on("load")).chain((0..NREF).map(Item::RefShow)).collect(),
            MenuId::MeasAdd => Vec::new(),
            MenuId::MeasLegacy => self.measurement_catalog(),
            MenuId::AddPanel => Vec::new(),
            MenuId::Search => self.addrail_search_items(),
            MenuId::Cursor if self.m.layout == Layout::Studio => self.addrail_cursor_items(),
            MenuId::MeasSelected => self.measurement_catalog(),
            MenuId::Cursor if self.m.cursor == CursorMode::Off => vec![Item::CurMode],
            MenuId::Cursor if self.m.cursor == CursorMode::Xy => vec![Item::CurMode, Item::CurA, Item::CurB, Item::CurC, Item::CurD],
            MenuId::Cursor if self.m.cursor == CursorMode::Track => vec![Item::CurMode, Item::CurA, Item::CurB, Item::CurSrcB],
            MenuId::Cursor => vec![Item::CurMode, Item::CurA, Item::CurB],
            MenuId::Display => {
                let mut v = vec![Item::Open(MenuId::Windows), Item::Layout, Item::Views, Item::Persist, Item::ColorGrade, Item::Intensity, Item::GridStyle, Item::GridBright, Item::GridFrame, Item::Dots, Item::BottomBar, Item::ZInput, Item::Histogram, Item::XyMode, Item::Hdmi, Item::HdmiView, Item::Backlight, Item::TextSize, Item::ClearDisp];
                if self.m.pane == tables::PANE_TREND {
                    v.splice(1..1, [Item::TrendSel(0), Item::TrendSel(1)]);
                }
                v
            }
            MenuId::Storage => vec![Item::StoreDest, Item::StoreSlot, Item::StoreSaveSetup, Item::StoreRecall, Item::StoreCsv, Item::StoreBin, Item::StoreFfSave, Item::StoreFfLoad, Item::StorePng],
            MenuId::Spectrum => vec![Item::SpecOn, Item::SpecSrc, Item::SpecCenter, Item::SpecSpan, Item::SpecHarm, Item::SpecRbw, Item::SpecWin, Item::SpecTrace, Item::SpecRef, Item::SpecWf, Item::SpecDist, Item::SpecAcprW, Item::SpecAcprS, Item::SpecPeak, Item::SpecMargin, Item::SpecLimitSet, Item::SpecLimitClear],
            MenuId::Eye => vec![Item::EyeAct, Item::EyeOn, Item::EyePlace, Item::EyeSrc, Item::EyeRate, Item::EyeBer, Item::EyeB, Item::EyeHp, Item::EyePnLo, Item::EyePnHi, Item::EyeNpj, Item::EyeRail, Item::EyeNotch, Item::EyePll, Item::EyeClear],
            MenuId::Power => {
                let mut v = vec![Item::PowOn, Item::PowV, Item::PowI, Item::PowMode];
                match self.m.pw_mode {
                    PowerMode::Ripple => v.extend([Item::PowFline, Item::PowFsw, Item::PowVin]),
                    PowerMode::Rail => v.extend([Item::PowRef, Item::PowVin, Item::PowClk]),
                    _ => {}
                }
                v
            }
            MenuId::Identify => self.identify_items(),
            MenuId::SelfCal => self.cal_items(),
            MenuId::Network => vec![Item::NetEth, Item::NetRetry10, Item::NetMac, Item::Open(MenuId::Remote), Item::NetPorts, Item::NetWlan, Item::WlanScan, Item::WlanForget, Item::WlanInfo],
            MenuId::Features | MenuId::Decoders(_) => self.features_items(menu).unwrap_or_default(),
            MenuId::Remote => self.remote_items(),
            MenuId::Ssh => self.ssh_items(),
            MenuId::Time => self.time_items(),
            MenuId::Bode => self.bode_items(),
            MenuId::Sim => self.sim_items(),
            MenuId::SimAdvanced => self.sim_advanced_items(),
            MenuId::Magnetic => self.magnetic_items(),
            MenuId::Inverter => self.inverter_items(),
            MenuId::Dpt => self.dpt_items(),
            MenuId::Afg => self.afg_items(),
            MenuId::BwTest => self.bw_items(),
            MenuId::ViewMore => self.view_more_items(),
            MenuId::La => self.la_items(),
            MenuId::LaLines => self.la_line_items(),
            MenuId::LaPat => (0..16).map(Item::LaPat).collect(),
            MenuId::Windows => self.win_items(),
            MenuId::Zone => vec![Item::ZoneSel, Item::ZoneOn, Item::ZoneSrc, Item::ZoneMode, Item::ZoneBox, Item::ZoneShape, Item::ZoneClear],
            MenuId::Mask => vec![Item::MaskOn, Item::MaskSrc, Item::MaskInvert, Item::MaskX, Item::MaskY, Item::MaskAction, Item::MaskCreate, Item::MaskFromRef, Item::MaskReset, Item::MaskClear, Item::MaskSave, Item::MaskLoad],
            MenuId::Utility => vec![Item::Offline, Item::QuickAct, Item::AuxOut, Item::BootMode, Item::Overclock, Item::Graphics, Item::Shot, Item::Open(MenuId::SelfCal), Item::Open(MenuId::Network), Item::Open(MenuId::Features), Item::Open(MenuId::Time), Item::SysInfo, Item::About, Item::Update, Item::UpdateRollback, Item::Stock],
            MenuId::Pick => {
                let n = self.pick.map_or(0, |p| self.options(p.it).len());
                (0..n).map(Item::PickOpt).collect()
            }
            MenuId::Math if self.m.math == MathOp::Off => vec![Item::MathSel, Item::MathFam, Item::MathOpI],
            MenuId::Math if self.m.math == MathOp::Expr => self.with_math_off(vec![Item::MathSel, Item::MathFam, Item::MathOpI, Item::MathExprI, Item::MathExprEdit, Item::MathScale, Item::MathOffset, Item::MathAuto]),
            MenuId::Math => {
                let mut v = vec![Item::MathSel, Item::MathFam, Item::MathOpI, Item::MathA];
                if self.m.math.two_sources() {
                    v.push(Item::MathB);
                }
                if self.m.math == MathOp::Lin {
                    v.extend([Item::MathLinA, Item::MathLinB]);
                }
                if self.m.math.has_fc() {
                    v.push(Item::MathFc);
                }
                if self.m.math.has_bw() {
                    v.push(Item::MathBw);
                }
                v.extend([Item::MathScale, Item::MathOffset]);
                if self.m.math == MathOp::Fft {
                    v.push(Item::FftWin);
                }
                if self.m.math != MathOp::Off {
                    v.push(Item::MathAuto);
                }
                self.with_math_off(v)
            }
        }
    }

    /// The start menu (Menu button, Multi 1 with no menu open): every front-panel key function and every menu, in
    /// groups (label, entries). The logic analyser and the generator only on models that have them; the history
    /// steps appear while a stored record is shown.
    fn main_groups(&self) -> Vec<(&'static str, Vec<Item>)> {
        use Item::*;
        let mut disp = vec![Open(MenuId::Display), Open(MenuId::Horizontal), NavHist];
        if self.hist_pos.is_some() {
            disp.extend([NavBack, NavFwd]);
        }
        // System > Features: what is left out is not offered
        let ana: Vec<Item> = analysers::ANALYSERS
            .iter()
            .flat_map(|a| a.start())
            .filter(|it| !matches!(it, Open(m) if self.menu_left_out(*m)))
            .collect();
        // the generator page only on models with one (S models), the LA only with the port
        let mut ins = Vec::new();
        if oos_hal::model::platform().caps.has_gen() {
            ins.push(Open(MenuId::Afg));
        }
        if la::available() {
            ins.push(Open(MenuId::La));
        }
        ins.push(Open(MenuId::Dvm));
        ins.push(Open(MenuId::Logger));
        let mut g = vec![
            ("Acquisition", vec![RunStop, SingleAct, AutosetAct, ClearDisp, DefaultAct, Open(MenuId::Acquire)]),
            ("Trigger", vec![Open(MenuId::Trigger), SlopeAct, ForceAct, Open(MenuId::Zone)]),
            ("Analysis", [Open(MenuId::Measure), Open(MenuId::Cursor), Open(MenuId::Math), Open(MenuId::Ref), Open(MenuId::Sim), Open(MenuId::Decode), Open(MenuId::Bus), Open(MenuId::Mask)].into_iter().filter(|it| !matches!(it, Open(m) if self.menu_left_out(*m))).collect()),
            ("Analysers", ana),
            ("Instruments", ins),
            ("Display", disp),
            ("System", vec![Open(MenuId::Storage), Shot, QuickRun, LockAct, Open(MenuId::Utility), PowerAct]),
        ];
        g.retain(|x| !x.1.is_empty());
        g
    }

    /// Hardware segments are recorded for the current set-up (else the software history records them).
    fn seg_hw_on(&self) -> bool {
        self.src.is_hw() && self.m.seg_hw && self.m.segments > 0 && board().seg_max(&self.m).is_some()
    }

    /// Most segments for the current set-up: the hardware limit with hardware segments, else the history limit.
    fn seg_limit(&self) -> usize {
        match board().seg_max(&self.m) {
            Some(x) if self.m.seg_hw && self.src.is_hw() => x.min(SEG_HW_MAX),
            _ => SEG_MAX,
        }
    }

    /// Segments one recording takes.
    fn seg_count(&self) -> usize {
        self.m.segments.min(self.seg_limit())
    }

    /// The segment counts of the menu: the steps within the limit, and the limit itself.
    fn seg_options(&self) -> Vec<usize> {
        let lim = self.seg_limit();
        let mut v: Vec<usize> = SEGMENTS.iter().copied().filter(|&n| n <= lim).collect();
        if v.last() != Some(&lim) {
            v.push(lim);
        }
        v
    }

    /// Hardware segments: tells the thread about RUN/STOP, takes the times when the read-back is done, stops when every
    /// segment is on the screen, shows a segment asked for.
    fn seg_poll(&mut self, running: bool) {
        let Some(v) = self.src.seg_sync(self.seg_run, running) else { return };
        let was = self.seg_view.map(|x| x.phase);
        self.seg_view = Some(v);
        if v.phase == crate::board::SegPhase::Done && was != Some(crate::board::SegPhase::Done) {
            self.seg_times = self.src.seg_times();
            if running && !v.queued {
                self.m.run = RunState::Stop;
                self.toast(&format!("{} segments recorded", v.recorded));
            }
        } else if running && v.phase == crate::board::SegPhase::Done && !v.queued {
            self.m.run = RunState::Stop;
            self.toast(&format!("{} segments recorded", v.recorded));
        }
        if let Some((k, rec)) = self.src.seg_shown(&self.m) {
            self.seg_pos = Some(k as usize);
            self.last[0] = Some(rec);
            self.cleared = false;
            self.measure_last();
        }
    }

    /// "Segment k/N", the trigger time since the first trigger and the step from the previous segment.
    fn seg_label(&self, i: usize) -> String {
        let n = self.seg_view.map_or(0, |v| v.recorded);
        let t = |k: usize| self.seg_times.get(k).copied().flatten();
        let mut s = format!("Segment {}/{}", i + 1, n);
        if let Some(x) = t(i) {
            s += &format!(" \u{b7} {}", si(x, "s"));
            if let Some(p) = i.checked_sub(1).and_then(t) {
                s += &format!(" \u{b7} \u{394}t {}", si(x - p, "s"));
            }
        }
        s
    }

    /// Hardware segments: the trigger times since the first trigger, comma separated, in seconds (9.9E37: no stamp).
    pub fn seg_times_text(&self) -> String {
        self.seg_times.iter().map(|t| t.map_or("9.9E37".to_string(), |x| format!("{:e}", x))).collect::<Vec<_>>().join(",")
    }

    /// Hardware segments: shows segment `i` (played from the FPGA memory), stopping the acquisition.
    pub(super) fn seg_show(&mut self, i: usize) {
        self.m.run = RunState::Stop;
        self.seg_pos = Some(i);
        self.src.seg_request(i as u32);
        self.toast(&self.seg_label(i));
    }

    /// Shows record `hist_pos + d` of the history (d = 0: the newest), stopping the acquisition.
    pub(crate) fn history_step(&mut self, d: i32) -> bool {
        // hardware segments: any of them, played on demand
        if let Some(v) = self.seg_view.filter(|v| self.seg_hw_on() && v.phase == crate::board::SegPhase::Done && v.recorded > 0) {
            let n = v.recorded as i32;
            let cur = self.seg_pos.unwrap_or(n as usize - 1) as i32;
            let i = if d == 0 { n - 1 } else { (cur + d).clamp(0, n - 1) };
            self.seg_show(i as usize);
            return true;
        }
        let n = self.history.len();
        if n == 0 {
            self.toast("History empty");
            return false;
        }
        self.m.run = RunState::Stop;
        let cur = self.hist_pos.unwrap_or(n - 1) as i32;
        let i = if d == 0 { n as i32 - 1 } else { (cur + d).clamp(0, n as i32 - 1) } as usize;
        self.hist_pos = Some(i);
        self.last[0] = Some(self.history[i].clone());
        self.measure_last();
        self.toast(&format!("History {}/{}", i + 1, n));
        true
    }

    /// Recomputes the measurements of the record shown, and of every loaded ref, at once (a result from the
    /// background for an older record is dropped).
    pub(crate) fn measure_last(&mut self) {
        self.measure_channel_sources();
        self.measure_refs();
    }

    /// The measurements of `rec` now, on this thread.
    fn measure_rec(&mut self, rec: &Record) {
        let work: Vec<(usize, &[f32], f64, f64)> = (0..NCH)
            .filter(|&c| self.m.ch[c].on)
            .map(|c| {
                let (v, dt) = self.meas_slice(rec, c);
                (c, v, dt, 0.4 * self.m.ch[c].volts_per_div() as f64)
            })
            .collect();
        let done = measure_channels(&work);
        self.meas_env_sent = std::array::from_fn(|c| self.meas_on_pairs(rec, c));
        self.meas_set(done);
        self.meas_n += 1;
        self.meas_want = false;
    }

    /// Statistics of measurements made on every frame by the deposit workers (framestats.rs) into the Measure page's
    /// statistics, once these exist (the first measured record starts them).
    fn mstat_fold(&mut self, folds: &crate::framestats::Folds) {
        if self.mstat.len() < self.m.meas.len() {
            return;
        }
        for (key, f) in folds.iter().filter(|x| x.1.n > 0) {
            if let Some(i) = self.m.meas.iter().position(|e| *e == (key.0, WaveId::Ch(key.1))) {
                let st = &mut self.mstat[i];
                if st.n == 0 {
                    (st.min, st.max) = (f.min, f.max);
                }
                st.n += f.n;
                st.sum += f.sum;
                st.sumsq += f.sumsq;
                st.min = st.min.min(f.min);
                st.max = st.max.max(f.max);
            }
        }
    }

    /// Each loaded ref, at the slot's own sample interval. Coupling and the bandwidth limit are not applied.
    fn measure_refs(&mut self) {
        self.measure_reference_sources();
    }

    /// A record with no samples, for an analyser whose sources are all stored refs.
    pub(super) fn blank_record() -> Record {
        Record { t0: 0.0, dt: 1.0, ch: Default::default(), triggered: false, partial: false, raw: None, la: None, pairs: false }
    }

    fn meas_set(&mut self, done: Vec<(usize, Stats, oos_dsp::measure::Analysis)>) {
        self.meas_env = self.meas_env_sent;
        for c in 0..NCH {
            (self.stats[c], self.ana[c]) = (Stats::default(), Default::default());
        }
        for (c, st, an) in done {
            (self.stats[c], self.ana[c]) = (st, std::sync::Arc::new(an));
        }
    }

    /// From now on the measurements of new records are made in the background (the frame loop on the unit).
    pub fn measure_in_background(&mut self) {
        self.meas_bg = MeasWorker::start();
    }

    /// The background's side, every frame: takes a finished result, and gives it the record shown when it wants one.
    fn meas_poll(&mut self) {
        let Some(w) = self.meas_bg.as_mut() else { return };
        if let Ok((n, done)) = w.rx.try_recv() {
            w.busy = false;
            if n == self.meas_n {
                // of the record shown when it was sent: in Run a few frames old, the newest result there is
                self.meas_set(done);
                self.meas_new = true;
            }
        }
        let w = self.meas_bg.as_mut().expect("worker");
        if w.busy || !self.meas_want {
            return;
        }
        self.meas_want = false;
        let pending = self.meas_pending.take();
        let Some(rec) = pending.as_ref().or(self.last[0].as_ref()) else { return };
        // a held pair record (deepmeas.rs) is never measured: the measurement stays that of the last raw one
        if crate::deepmeas::hold(&self.m, rec.raw.as_ref().map(|r| &*r.frame)) {
            return;
        }
        let env: [bool; NCH] = std::array::from_fn(|c| self.meas_on_pairs(rec, c));
        let job: Vec<(usize, MeasSrc, f64, f64)> = (0..NCH)
            .filter(|&c| self.m.ch[c].on)
            .map(|c| {
                let min = 0.4 * self.m.ch[c].volts_per_div() as f64;
                match self.meas_frame(rec, c) {
                    Some((f, i0, i1)) => {
                        let dt = f.dt;
                        (c, MeasSrc::Frame(f, c, i0, i1), dt, min)
                    }
                    None => (c, MeasSrc::Own(rec.ch[c].clone()), rec.dt, min),
                }
            })
            .collect();
        let w = self.meas_bg.as_mut().expect("worker");
        if w.tx.send((self.meas_n, job)).is_ok() {
            w.busy = true;
            self.meas_env_sent = env;
        } else {
            // the thread is gone: measure here again
            self.meas_bg = None;
            self.measure_last();
            self.meas_new = true;
        }
    }

    /// The window of the hardware frame `meas_slice` measures channel `c` on, if it does.
    fn meas_frame(&self, rec: &Record, c: usize) -> Option<(std::sync::Arc<crate::board::Frame>, usize, usize)> {
        let r = rec.raw.as_ref().filter(|_| self.m.acq != AcqMode::HiRes)?;
        (r.i1 <= r.frame.ch[c].len() && r.i1 - r.i0 > rec.ch[c].len()).then(|| (r.frame.clone(), r.i0, r.i1))
    }

    /// Samples to measure channel `c` on: every captured sample of the screen window when the hardware frame has more
    /// than the screen record (conditioned like the record), else the screen record.
    fn meas_slice<'a>(&self, rec: &'a Record, c: usize) -> (&'a [f32], f64) {
        // high res: the screen record is the mean of those samples, the measurements follow it
        if let Some(r) = rec.raw.as_ref().filter(|_| self.m.acq != AcqMode::HiRes) {
            if self.meas_frame(rec, c).is_some() {
                return (&r.frame.ch[c][r.i0..r.i1], r.frame.dt);
            }
        }
        (&rec.ch[c], rec.dt)
    }

    /// `meas_slice` as an own copy.
    fn meas_wave(&self, rec: &Record, c: usize) -> (Vec<f32>, f64) {
        let (v, dt) = self.meas_slice(rec, c);
        (v.to_vec(), dt)
    }

    /// Search: times (s, relative to the trigger) of all events of the trigger condition in the record shown.
    pub(crate) fn search_events(&self) -> std::sync::Arc<Vec<f64>> {
        if let Some(marks) = self.studio_search_marks() { return marks; }
        if self.m.trig_type == TrigType::Bus && (Self::decoder_uses_refs(&self.m)
            || self.m.deskew_ch.iter().any(|&shift| shift != 0.0)) {
            return self.decoder_trigger_hits(&self.m).unwrap_or_default();
        }
        if let Some(slot) = self.m.trig_ref {
            return self.reference_trigger_hits(slot).unwrap_or_else(|| std::sync::Arc::new(Vec::new()));
        }
        let Some(rec) = &self.last[0] else { return std::sync::Arc::new(Vec::new()) };
        if !self.m.trig_channels().iter().enumerate().all(|(c, &u)| !u || !rec.ch[c].is_empty()) {
            return std::sync::Arc::new(Vec::new());
        }
        let refs: [&[f32]; NCH] = std::array::from_fn(|c| &rec.ch[c][..]);
        let hyst: [f32; NCH] = std::array::from_fn(|c| 0.1 * self.m.ch[c].volts_per_div());
        let inp = oos_dsp::trigger::Input { ch: refs, dt: rec.dt, hyst };
        let t0 = self.source_time_origin(self.m.trigger_wave(), rec.t0);
        let mut times: Vec<_> = crate::trig::find_all(&self.m, &inp).into_iter().map(|i| t0 + i * rec.dt).collect();
        times.sort_by(f64::total_cmp);
        std::sync::Arc::new(times)
    }

    /// Half the sample rate while it is below the ADC's and the samples are taken one in so many (Normal, Average, High
    /// res): the hardware keeps every n-th sample and filters nothing before, so a signal above it shows at a wrong,
    /// lower frequency. None at the full rate and in Peak, whose min/max see every sample.
    pub fn alias_above(&self) -> Option<f64> {
        let (sr, _) = self.m.srate_depth();
        (self.m.acq != AcqMode::Peak && sr < self.m.max_srate() * 0.999).then_some(sr / 2.0)
    }

    /// The level line is on: shortly after the trigger level changed (`LEVEL_LINE_S`).
    fn level_line_on(&self) -> bool {
        self.level_changed > 0.0 && self.now - self.level_changed < LEVEL_LINE_S
    }

    /// Level shown at the trigger marker: the pattern level of the source for pattern/duration, else the level.
    fn trig_level_shown(&self) -> f32 {
        if matches!(self.m.trig_type, TrigType::Pattern | TrigType::Duration) {
            self.m.pat_levels[self.m.trig_src]
        } else {
            self.m.trig_level
        }
    }

    /// Decode menu rows for the selected protocol.
    fn decode_items(&self) -> Vec<Item> {
        use Item::*;
        let p = self.m.dec_proto;
        let r = p.roles();
        let on_la = self.m.dec_on_la();
        let mut v = vec![DecSel, DecOn, DecProto];
        if self.m.la_on() && p != Proto::CPHY {
            v.push(DecIn);
        }
        v.push(DecSrc);
        if r[1].is_some() {
            v.push(DecSrc2);
        }
        if r[2].is_some() {
            v.push(DecCs);
        }
        if r[3].is_some() {
            v.push(DecSrc4);
        }
        if p.diff_ok() && !on_la {
            v.push(DecDiff);
        }
        // the shared rows the protocol names, then its own options
        use crate::protocols::Row;
        for r in p.module().map_or(&[][..], |x| x.shared()) {
            v.push(match r {
                Row::Baud => DecBaud,
                Row::Bits => DecBits,
                Row::Parity => DecParity,
                Row::SpiMode => DecMode,
                Row::CanRate => DecCanRate,
                Row::CanH => DecCanH,
                Row::UsbSpeed => DecUsbSpeed,
                Row::CphyRate => DecCphyRate,
                Row::Fmt => DecFmt,
            });
        }
        v.extend((0..p.module().map_or(0, |x| x.options().len())).map(DecOpt));
        // LA lines: the probe's comparators set the levels (LA page)
        if !on_la {
            v.push(DecThr);
        }
        // what it decoded, as a table, and as a file
        v.extend([DecTable, DecExport]);
        v
    }

    /// Switches the decoder protocol. The demo takes the sources, channels and trigger that match its bus traffic
    /// (`Protocol::setup`). On every other source (the hardware, Live, Replay of a recorded file) one setting must not
    /// change another (the forum, eTobey): the channels and the trigger stay as they are, the sources the protocols
    /// share stay, and a role the old protocol did not have takes a channel that is on and no other role uses, or none.
    /// The protocol's own parameters (threshold, baud, bits ...) are set on both.
    fn set_dec_proto(&mut self, p: Proto) {
        let Proto::Module(i) = p;
        let s = crate::protocols::get(i).setup();
        if Self::decoder_uses_refs(&self.m) {
            self.m.dec_proto = p;
            if s.la_off {
                (self.m.dec_thr, self.m.dec_la_in) = (s.thr, false);
            }
            return;
        }
        let demo = self.src.is_demo();
        let m = &mut self.m;
        let changed = m.dec_proto != p;
        let old = m.dec_proto.roles();
        m.dec_proto = p;
        // its threshold and shared rows; its own options from their defaults when it is newly chosen
        m.dec_thr = s.thr;
        let r = p.roles();
        if demo {
            // its demo channels and LA lines; roles the protocol doesn't have keep their sources
            m.dec_la = s.la;
            m.dec_src = s.src[0].unwrap_or(2);
            if r[2].is_some() {
                m.dec_cs = s.src[2];
            }
            if r[3].is_some() {
                m.dec_src4 = s.src[3];
            }
            if let Some(c) = s.src[1] {
                m.dec_src2 = c;
            }
        } else {
            // a new role: a channel that is on and that no other role of the protocol uses
            let mut used = vec![m.dec_src];
            if r[1].is_some() && old[1].is_some() && m.dec_src2 != m.dec_src {
                used.push(m.dec_src2);
            }
            used.extend([(r[2], old[2], m.dec_cs), (r[3], old[3], m.dec_src4)].into_iter().filter_map(|(n, o, c)| n.and(o).and(c)));
            let mut free = |m: &Model| {
                let c = (0..NCH).find(|&c| m.ch[c].on && !used.contains(&c));
                used.extend(c);
                c
            };
            if r[1].is_some() && (old[1].is_none() || m.dec_src2 == m.dec_src) {
                if let Some(c) = free(m) {
                    m.dec_src2 = c;
                }
            }
            if r[2].is_some() && (old[2].is_none() || m.dec_cs.is_none()) {
                m.dec_cs = free(m);
            }
            if r[3].is_some() && (old[3].is_none() || m.dec_src4.is_none()) {
                m.dec_src4 = free(m);
            }
        }
        if s.la_off {
            m.dec_la_in = false;
        }
        use crate::protocols::Row;
        for &(r, v) in s.rows {
            match r {
                Row::Baud => m.dec_baud = v as usize,
                Row::Bits => m.dec_bits = v,
                Row::Parity => m.dec_parity = [oos_dsp::decode::Parity::None, oos_dsp::decode::Parity::Even, oos_dsp::decode::Parity::Odd][(v as usize).min(2)],
                Row::SpiMode => m.dec_mode = v as u8,
                Row::CanRate => m.dec_canrate = v as usize,
                Row::CanH => m.dec_canh = v != 0,
                Row::UsbSpeed => m.dec_uspeed = v as usize,
                Row::CphyRate | Row::Fmt => {}
            }
        }
        if changed {
            m.dec_opt = crate::protocols::opt_default(i);
            // a module with a view of its own: open it, when no other pane is open
            if m.pane == 0 && crate::protocols::get(i).view(&[], &crate::protocols::opt_fit(i, m.dec_opt), &mut None).is_some() {
                m.pane = tables::PANE_BUS;
            }
        }
        if !demo {
            self.dirty_map = true;
            return;
        }
        // the demo: every source the protocol uses; the traffic of some is on all its channels or on every channel
        m.ch[m.dec_src].on = true;
        if r[1].is_some() {
            m.ch[m.dec_src2].on = true;
        }
        if let (Some(_), Some(c)) = (r[2], m.dec_cs) {
            m.ch[c].on = true;
        }
        if let (Some(_), Some(c)) = (r[3], m.dec_src4) {
            m.ch[c].on = true;
        }
        if s.show {
            for c in s.src.into_iter().flatten().filter(|&c| c < NCH) {
                m.ch[c].on = true;
            }
        }
        if s.show_all {
            for channel in &mut m.ch {
                channel.on = true;
            }
        }
        // the trigger is not touched: choosing a protocol only configures the decoder (and shows its sources)
        self.dirty_map = true;
    }

    /// Trigger menu rows for the selected type.
    fn trigger_items(&self) -> Vec<Item> {
        use crate::model::TrigType as TT;
        use Item::*;
        let m = &self.m;
        let when = |v: &mut Vec<Item>| {
            v.push(TrigWhen);
            if m.trig_when != WhenKind::Off {
                v.push(TrigT1);
            }
            if matches!(m.trig_when, WhenKind::In | WhenKind::Out) {
                v.push(TrigT2);
            }
        };
        let mut v = vec![Item::TrigType];
        match m.trig_type {
            // an LA line has no level of its own (LA page: pod threshold)
            TT::Edge if m.trig_line().is_some() => v.extend([TrigSrc, TrigSlope]),
            TT::Edge => v.extend([TrigSrc, TrigSlope, TrigLevel, Trig50, TrigCoup]),
            // the protocol and its sources come from the Decode menu
            TT::Bus => {
                v.extend([DecProto, TrigBusWhat]);
                if matches!(m.trig_bus, BusWhat::Id | BusWhat::Data) {
                    v.push(TrigBusVal);
                }
                v.extend(self.serial_items());
            }
            TT::Pulse => {
                v.extend([TrigSrc, TrigPol]);
                when(&mut v);
                v.push(TrigLevel);
            }
            TT::Slope => {
                v.extend([TrigSrc, TrigSlope]);
                when(&mut v);
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::Video => {
                v.extend([TrigSrc, VidStd, VidSync]);
                if m.video_sync == VSync::Line {
                    v.push(VidLine);
                }
                v.push(TrigLevel);
            }
            TT::Pattern => {
                v.extend([Pat(0), Pat(1), Pat(2), Pat(3), TrigSrc, PatLevel]);
                if m.la_on() {
                    v.push(Open(MenuId::LaPat));
                }
            }
            TT::Duration => {
                v.extend([Pat(0), Pat(1), Pat(2), Pat(3)]);
                when(&mut v);
                v.extend([TrigSrc, PatLevel]);
                if m.la_on() {
                    v.push(Open(MenuId::LaPat));
                }
            }
            TT::Timeout => v.extend([TrigSrc, TrigSlope, TrigT1, TrigLevel]),
            TT::Runt => {
                v.extend([TrigSrc, TrigPol]);
                when(&mut v);
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::Window => {
                v.extend([TrigSrc, WinModeI]);
                if m.win_mode == WinMode::Inside {
                    when(&mut v);
                }
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::Delay => {
                v.extend([TrigSrc, TrigSlope, TrigSrc2, TrigSlope2]);
                when(&mut v);
                v.extend([TrigLevel, TrigLevel2]);
            }
            TT::SetupHold => v.extend([TrigSrc, TrigSrc2, TrigSlope2, Setup, Hold, TrigLevel, TrigLevel2]),
            TT::NthEdge => v.extend([TrigSrc, TrigSlope, NthIdle, NthN, TrigLevel]),
        }
        // the analog comparators: not for a protocol or an LA line
        if m.trig_type != TT::Bus && m.trig_line().is_none() {
            v.push(TrigNoise);
        }
        v.push(TrigAutoNoise);
        v.push(TrigMode);
        v.push(SearchOn);
        if self.search_on {
            v.extend([SearchTable, SearchSave]);
        }
        v.push(Open(MenuId::Zone));
        v.push(TrigHoldoff);
        v
    }

    /// Row label, with the trigger rows named after their role in the selected trigger type.
    fn item_label(&self, it: Item) -> &'static str {
        if it == Item::SimPlay {
            if self.sim_generating() { return "Cancel generation"; }
            return if !self.sim_mode() { "Run simulation mode" } else if self.m.run == RunState::Run { "Stop simulation" } else { "Run simulation" };
        }
        let t = self.m.trig_type;
        let two_level = matches!(t, TrigType::Slope | TrigType::Runt | TrigType::Window);
        match it {
            Item::CurA if self.m.cursor == CursorMode::Xy => "X1",
            Item::CurB if self.m.cursor == CursorMode::Xy => "X2",
            Item::TrigLevel if two_level => "Upper",
            Item::TrigLevel2 if two_level => "Lower",
            Item::TrigLevel if t == TrigType::Delay => "Level A",
            Item::TrigLevel2 if t == TrigType::Delay => "Level B",
            Item::TrigLevel if t == TrigType::SetupHold => "Data level",
            Item::TrigLevel2 if t == TrigType::SetupHold => "Clock level",
            Item::TrigSrc if t == TrigType::Delay => "Source A",
            Item::TrigSrc if t == TrigType::SetupHold => "Data",
            Item::TrigSrc if matches!(t, TrigType::Pattern | TrigType::Duration) => "Level of",
            Item::TrigSrc2 if t == TrigType::SetupHold => "Clock",
            Item::TrigSlope if t == TrigType::Delay => "Slope A",
            Item::TrigSlope2 if t == TrigType::SetupHold => "Clock edge",
            Item::TrigT1 if t == TrigType::Timeout => "Timeout",
            Item::DecOpt(k) => self.m.dec_proto.module().and_then(|p| p.options().get(k)).map_or("Option", |o| o.name),
            Item::DecSrc if self.m.dec_proto.roles()[0] != Some("Source") => self.m.dec_proto.roles()[0].unwrap_or("Source"),
            Item::DecSrc2 if self.m.dec_proto.roles()[1].is_some() => self.m.dec_proto.roles()[1].unwrap_or("Clock"),
            Item::DecCs if self.m.dec_proto.roles()[2].is_some() => self.m.dec_proto.roles()[2].unwrap_or("CS"),
            Item::DecSrc4 if self.m.dec_proto.roles()[3].is_some() => self.m.dec_proto.roles()[3].unwrap_or("Source 4"),
            Item::DecThr if self.m.dec_proto == Proto::CPHY => "Min level gap",
            Item::TrigT1 if matches!(self.m.trig_when, WhenKind::In | WhenKind::Out) => "Lower limit",
            Item::NavHist if self.hist_pos.is_some() => "Live",
            _ => Self::label(it),
        }
    }

    /// Switches the trigger type and sets starting values that trigger on the demo signals.
    fn set_trig_type(&mut self, t: TrigType) {
        let was_bus = self.m.trig_type == TrigType::Bus;
        // leaving Bus: off again what entering it switched on (the decoder kept the live picture raw, its demo sources
        // stayed on); a type set elsewhere since leaves nothing to undo
        let undo = if t != TrigType::Bus { self.bus_auto.take().filter(|_| was_bus) } else { None };
        if let Some((chans, dec)) = undo {
            for c in (0..NCH).filter(|&c| chans[c]) {
                self.m.ch[c].on = false;
            }
            if dec {
                self.m.dec_on = false;
            }
        }
        if t == TrigType::Bus && Self::decoder_uses_refs(&self.m) {
            if let Some(source) = decode_wave(self.m.dec_src) {
                self.set_trigger_source(source, false);
                self.m.trig_type = t;
                self.m.dec_on = true;
                self.replay.reset_search();
                return;
            }
        }
        if self.set_reference_trigger_type(t) { return; }
        if t == TrigType::Bus {
            // the decoder's protocol defines the bus: the starting match is a value its traffic contains.
            // Choosing the trigger type neither switches a decoder on nor touches its sources or any channel.
            let m = &mut self.m;
            m.trig_type = t;
            // a value the protocol's demo traffic contains
            let Proto::Module(i) = m.dec_proto;
            let (what, val) = crate::protocols::get(i).trigger_default();
            (m.trig_bus, m.trig_bus_val, m.ser) = (what, val as u64, SerialTrig::default());
            self.dirty_map = true;
            return;
        }
        // the presets below suit the demo signals (each type on a channel that shows it). Every other source (the
        // hardware, a live or replayed record) keeps what the user set: the channels, source, level and slope, and
        // also the width, the pattern, the lower level and the rest that several types share (the forum, eTobey: one
        // setting must not change another). A SIM session triggers on its reference: set_reference_trigger_type
        let demo = self.src.is_demo();
        let m = &mut self.m;
        m.trig_type = t;
        // Only fields of the trigger itself change: a source that is still valid stays, and no channel is switched on.
        let keep = |m: &Model, c: usize| if c < NCH && m.ch[c].on { c } else { (0..NCH).find(|&k| m.ch[k].on).unwrap_or(c.min(NCH - 1)) };
        m.trig_src = keep(m, m.trig_src);
        let src = m.trig_src;
        let second = |m: &Model| {
            let c = m.trig_src2;
            if c != src && c < NCH && m.ch[c].on { c }
            else { (0..NCH).find(|&k| k != src && m.ch[k].on).unwrap_or(if c != src && c < NCH { c } else { (src + 1) % NCH }) }
        };
        match t {
            _ if !demo => {}
            TrigType::Bus => {} // handled above
            TrigType::Edge => {
                m.trig_level = 1.5;
            }
            TrigType::Pulse => {
                (m.trig_pol, m.trig_when, m.trig_t1, m.trig_level) = (Pol::Pos, WhenKind::Lt, 30e-6, 1.65);
            }
            TrigType::Slope => {
                (m.slope, m.trig_when, m.trig_t1, m.trig_level, m.trig_level2) = (Slope::Rise, WhenKind::Gt, 1e-6, 2.0, 0.5);
            }
            TrigType::Video => {
                m.trig_level = 0.15;
            }
            TrigType::Pattern => {
                m.pat = std::array::from_fn(|i| if i == 0 { Bit::H } else if i == NCH - 1 { Bit::R } else { Bit::X });
            }
            TrigType::Duration => {
                m.pat = std::array::from_fn(|i| if i == 0 { Bit::H } else { Bit::X });
                (m.trig_when, m.trig_t1) = (WhenKind::Gt, 200e-6);
            }
            TrigType::Timeout => {
                (m.slope, m.trig_t1, m.trig_level) = (Slope::Rise, 1.2e-3, 1.5);
            }
            TrigType::Runt => {
                (m.trig_pol, m.trig_when, m.trig_level, m.trig_level2) = (Pol::Pos, WhenKind::Off, 2.0, 0.5);
            }
            TrigType::Window => {
                (m.win_mode, m.trig_level, m.trig_level2) = (WinMode::Enter, 0.5, -0.5);
            }
            TrigType::Delay => {
                let s2 = second(m);
                (m.slope, m.trig_src2, m.trig_slope2, m.trig_when, m.trig_t1, m.trig_level, m.trig_level2) =
                    (Slope::Rise, s2, Slope::Rise, WhenKind::Gt, 40e-6, 1.5, 1.65);
            }
            TrigType::SetupHold => {
                let s2 = second(m);
                (m.trig_src2, m.trig_slope2, m.setup, m.hold, m.trig_level, m.trig_level2) = (s2, Slope::Rise, 2e-6, 2e-6, 1.65, 1.65);
            }
            TrigType::NthEdge => {
                (m.slope, m.nth_idle, m.nth_n, m.trig_level) = (Slope::Fall, 50e-6, 1, 1.65);
            }
        }
        if m.trig_type != TrigType::Runt && m.trig_when == WhenKind::Off {
            m.trig_when = WhenKind::Lt;
        }
        if !demo {
            // the second source of Delay and Setup/Hold on a channel that is on (PR #8, W4R)
            let src = m.trig_src;
            if matches!(m.trig_type, TrigType::Delay | TrigType::SetupHold) && (m.trig_src2 == src || !m.ch[m.trig_src2.min(NCH - 1)].on) {
                if let Some(c) = (0..NCH).find(|&k| k != src && m.ch[k].on) {
                    m.trig_src2 = c;
                }
            }
        }
        // the lower level the type finds (Delay's level B, say) not above the shared upper one
        m.order_trigger_levels();
        self.dirty_map = true;
    }

    fn menu_title(&self, menu: MenuId) -> String {
        match menu {
            // where the files go now; /tmp is in RAM: say so
            MenuId::Storage if self.save_dir() == std::path::Path::new(crate::store::USB_DIR) => "Storage \u{b7} USB stick".into(),
            MenuId::Storage if self.save_dir().starts_with("/tmp") => format!("Storage \u{b7} {} (lost at power-off)", self.save_dir().display()),
            MenuId::Storage => format!("Storage \u{b7} {}", self.save_dir().display()),
            MenuId::Mask => "Mask test".into(),
            MenuId::Zone => "Zone trigger".into(),
            MenuId::Spectrum => "Spectrum analyser".into(),
            MenuId::Eye => "Eye diagram / jitter".into(),
            MenuId::Power => "Power analysis".into(),
            MenuId::Identify => "Identify: tap to apply".into(),
            MenuId::SelfCal => "Self-calibration".into(),
            MenuId::Network => "Network".into(),
            MenuId::Features => "Features".into(),
            MenuId::Decoders(k) => format!("Decoders {} of {}", k + 1, PROTOS.len().div_ceil(features::DEC_PAGE)),
            MenuId::Remote => "Remote services".into(),
            MenuId::Ssh => "SSH".into(),
            MenuId::Time => "Date and time".into(),
            MenuId::Bode => "Bode plot".into(),
            MenuId::Sim => "SIM".into(),
            MenuId::SimAdvanced => "SIM advanced".into(),
            MenuId::Magnetic => "Magnetic".into(),
            MenuId::Inverter => "Inverter".into(),
            MenuId::Dpt => "Double pulse".into(),
            MenuId::Afg => "Generator (GI/GO)".into(),
            MenuId::BwTest => "Bandwidth test (step response)".into(),
            MenuId::ViewMore => self.view_name().into(),
            MenuId::La => self.la_title(),
            MenuId::LaLines => "Logic analyser \u{b7} lines shown".into(),
            MenuId::LaPat => "Pattern \u{b7} D0-D15".into(),
            MenuId::Main => "Menu".into(),
            MenuId::Channel(c) => format!("CH{}", c + 1),
            MenuId::Horizontal => "Horizontal".into(),
            MenuId::ChMore(c) => format!("CH{} more", c + 1),
            MenuId::Trigger => "Trigger".into(),
            MenuId::Acquire => "Acquire".into(),
            MenuId::Measure => "Measure".into(),
            MenuId::MeasAdd => "Add measurements".into(),
            MenuId::MeasLegacy => "All measurements".into(),
            MenuId::AddPanel => "Add".into(),
            MenuId::Search => "Search 1".into(),
            MenuId::MeasSelected => format!("Selected measurements ({})", self.measurement_count()),
            MenuId::MeasHold => "Add".into(),
            MenuId::MeasPair => format!("Pairs {} to {}", self.pair_source_caption(self.meas_src), self.pair_source_caption(self.meas_src_b)),
            MenuId::MeasLevels => "Measurement levels".into(),
            MenuId::MeasRow => self.measrow_title(),
            MenuId::MeasStat => "Statistics".into(),
            MenuId::MeasFall => "Fall levels".into(),
            MenuId::Dvm => "DVM and counter".into(),
            MenuId::Logger => "Data logger".into(),
            MenuId::Cursor => format!("Cursor · {}", self.cursor_wave().map_or("No source".into(), |id| self.wave_name(id))),
            MenuId::Display => "Display".into(),
            MenuId::Windows => "Windows".into(),
            MenuId::Utility => "System".into(),
            MenuId::Math => "Math".into(),
            MenuId::Ref => self.refs[self.ref_sel].as_ref().map_or(format!("Reference R{} · empty", self.ref_sel + 1), |r| {
                // a name only when it is one: not empty and not the slot's own name again
                let own = format!("R{}", self.ref_sel + 1);
                if r.label.is_empty() || r.label.eq_ignore_ascii_case(&own) { format!("Reference {own}") }
                else { format!("Reference {own} \u{b7} {}", r.label) }
            }),
            MenuId::Decode => "Decode".into(),
            MenuId::Bus => "Bus · I2C".into(),
            MenuId::Pick => match self.pick {
                Some(p) if Self::hex_item(p.it) => format!("{} \u{b7} 0x{}_", self.item_label(p.it), self.keypad),
                Some(p) if p.it == Item::AfgFreqEntry => format!("Frequency \u{b7} {}_ (max {})", self.keypad, si_short(self.m.gen_max_hz(), "Hz")),
                Some(p) if p.it == Item::RefImport && self.sheets_on() => format!("Load file into R{}", self.ref_sel + 1),
                Some(p) => format!("{} \u{b7} {}", self.menu_title(p.menu), self.item_label(p.it)),
                None => String::new(),
            },
        }
    }

    fn kind(it: Item) -> Kind {
        if matches!(it, Item::AutoDeskew | Item::ClearDeskew) { return Kind::Action; }
        if let Some(k) = Self::addrail_kind(it).or_else(|| Self::meas_kind(it)).or_else(|| Self::measrow_kind(it)).or_else(|| Self::ana_kind(it)).or_else(|| Self::magnetic_kind(it)).or_else(|| Self::inverter_kind(it)).or_else(|| Self::dpt_kind(it)).or_else(|| Self::cal_kind(it)).or_else(|| Self::bode_kind(it)).or_else(|| Self::sim_kind(it)).or_else(|| Self::offline_kind(it)).or_else(|| Self::afg_kind(it)).or_else(|| Self::bw_kind(it)).or_else(|| Self::la_kind(it)).or_else(|| Self::win_kind(it)).or_else(|| Self::bus_kind(it)).or_else(|| Self::time_kind(it)).or_else(|| Self::remote_kind(it)).or_else(|| Self::graphics_kind(it)).or_else(|| Self::views_kind(it)).or_else(|| Self::features_kind(it)).or_else(|| Self::backlight_kind(it)) {
            return k;
        }
        match it {
            Item::LogIvl | Item::ChBias(_) | Item::ChSkew(_) | Item::RefColor | Item::CurC | Item::CurD | Item::RefSel | Item::RefVpd | Item::RefOffset | Item::MathLinA | Item::MathLinB | Item::TrigHoldoff | Item::CntDigits | Item::MathScale | Item::MathOffset | Item::MathFc | Item::MathBw | Item::ZScale | Item::ZDelay | Item::DecThr => Kind::Value,
            Item::MathAuto | Item::ZoneBox | Item::ZoneShape | Item::ZoneClear => Kind::Action,
            Item::DecBits => Kind::Value,
            Item::TrigLevel2 | Item::TrigT1 | Item::TrigT2 | Item::VidLine | Item::PatLevel | Item::NthIdle | Item::NthN | Item::Setup | Item::Hold => Kind::Value,
            Item::ChScale(_) | Item::ChOffset(_) | Item::HScale | Item::HDelay | Item::TrigLevel | Item::AcqAvg | Item::CurA | Item::CurB | Item::Intensity | Item::GridBright => Kind::Value,
            Item::RefSave | Item::RefSaveNext | Item::RefClear => Kind::Action,
            Item::StoreSlot | Item::MaskX | Item::MaskY => Kind::Value,
            Item::StoreSaveSetup | Item::StoreRecall | Item::StoreCsv | Item::StorePng | Item::StoreFfSave | Item::StoreFfLoad | Item::MaskCreate | Item::MaskFromRef | Item::MaskReset | Item::MaskClear => Kind::Action,
            Item::Open(_) | Item::MathExprEdit | Item::ChBiasZero(_) | Item::RefLabel | Item::RefExport | Item::RefImport | Item::RefLoad | Item::DecTable | Item::DecExport | Item::SearchTable | Item::SearchSave | Item::StoreBin | Item::MeasPair(_) | Item::CntClear | Item::MaskSave | Item::MaskLoad | Item::ChLabel(_) | Item::SrcOn | Item::WlanInfo | Item::WlanScan | Item::WlanForget | Item::HZero | Item::Trig50 | Item::MeasAdd(_) | Item::AddMeas | Item::MeasCfg(_) | Item::MeasRemove(_) | Item::MeasClear | Item::AcqSegTable | Item::ClearDisp | Item::MathOff | Item::About | Item::SysInfo | Item::Shot | Item::Stock | Item::Update | Item::UpdateRollback => Kind::Action,
            Item::PickOpt(_) | Item::ViewExit | Item::ViewSettings => Kind::Action,
            Item::SpiTimeout => Kind::Value,
            Item::RunStop | Item::SingleAct | Item::AutosetAct | Item::DefaultAct | Item::SlopeAct | Item::ForceAct => Kind::Action,
            Item::NavHist | Item::NavBack | Item::NavFwd | Item::QuickRun | Item::LockAct | Item::PowerAct => Kind::Action,
            _ => Kind::Choice,
        }
    }

    fn label(it: Item) -> &'static str {
        if let Some(l) = Self::addrail_label(it).or_else(|| Self::meas_cfg_label(it)).or_else(|| Self::measrow_label(it)).or_else(|| Self::ana_label(it)).or_else(|| Self::magnetic_label(it)).or_else(|| Self::inverter_label(it)).or_else(|| Self::dpt_label(it)).or_else(|| Self::cal_label(it)).or_else(|| Self::bode_label(it)).or_else(|| Self::sim_label(it)).or_else(|| Self::offline_label(it)).or_else(|| Self::afg_label(it)).or_else(|| Self::bw_label(it)).or_else(|| Self::la_label(it)).or_else(|| Self::win_label(it)).or_else(|| Self::bus_label(it)).or_else(|| Self::time_label(it)).or_else(|| Self::remote_label(it)).or_else(|| Self::graphics_label(it)).or_else(|| Self::views_label(it)).or_else(|| Self::monitor_label(it)).or_else(|| Self::features_label(it)).or_else(|| Self::backlight_label(it)) {
            return l;
        }
        match it {
            Item::Open(MenuId::Acquire) => "Acquire",
            Item::Open(MenuId::Trigger) => "Trigger",
            Item::Open(MenuId::Horizontal) => "Horizontal",
            Item::Open(MenuId::ChMore(_)) => "More",
            Item::Open(MenuId::Measure) => "Measure",
            Item::AddMeas => "Add meas",
            Item::Open(MenuId::Cursor) => "Cursor",
            Item::Open(MenuId::Display) => "Display",
            Item::Open(MenuId::Utility) => "System",
            Item::Open(MenuId::MeasPair) => "Channel pair",
            Item::Open(MenuId::MeasFall) => "Fall levels",
            Item::Open(MenuId::MeasLevels) => "Levels",
            Item::Open(MenuId::MeasStat) => "Statistics",
            Item::Open(MenuId::Dvm) => "DVM / counter",
            Item::Open(MenuId::Logger) => "Data logger",
            Item::Open(MenuId::Network) => "Network",
            Item::Open(MenuId::Math) => "Math",
            Item::Open(MenuId::Ref) => "Reference",
            Item::Open(MenuId::Storage) => "Storage",
            Item::Open(MenuId::Mask) => "Mask",
            Item::Open(MenuId::Zone) => "Zones",
            Item::Open(MenuId::Magnetic) => "Magnetic",
            Item::Open(MenuId::Inverter) => "Inverter",
            Item::Open(MenuId::Dpt) => "Double pulse",
            Item::ColorGrade => "Colour grade",
            Item::StoreSlot => "Setup slot",
            Item::StoreDest => "Save to",
            Item::StoreSaveSetup => "Save setup",
            Item::StoreRecall => "Recall setup",
            Item::StoreCsv => "Save waveform (CSV)",
            Item::StorePng => "Save screenshot (PNG)",
            Item::StoreFfSave => "Save fast-frame",
            Item::StoreFfLoad => "Load fast-frame",
            Item::MaskOn => "Mask test",
            Item::MaskSrc => "Source",
            Item::MaskX => "Tolerance X",
            Item::MaskY => "Tolerance Y",
            Item::MaskAction => "On fail",
            Item::MaskInvert => "Invert",
            Item::MaskCreate => "Create from CH",
            Item::MaskFromRef => "Create from ref",
            Item::MaskReset => "Reset counts",
            Item::MaskClear => "Clear mask",
            Item::MeasStats => "Statistics",
            Item::MeasLabels => "Labels on trace",
            Item::StripDock => "Dock",
            Item::Zoom => "Zoom",
            Item::AutoRoll => "Auto Roll",
            Item::Open(MenuId::Decode) => "Decode",
            Item::DecOn => "Decoder",
            Item::DecSel => "Decoder no.",
            Item::DecTable => "Event table",
            Item::DecExport => "Export events",
            Item::SearchTable => "Event list",
            Item::SearchSave => "Save events",
            Item::StoreBin => "Save waveform (binary)",
            Item::PaneSel => "Lower",
            Item::TrendSel(0) => "Trend chart 1",
            Item::TrendSel(_) => "Trend chart 2",
            Item::DecSrc => "Source",
            Item::DecBaud => "Baud",
            Item::DecCphyRate => "Symbol rate",
            Item::DecCanRate => "Bit rate",
            Item::TrigBusWhat => "Match",
            Item::TrigBusVal => "Value",
            Item::SerRel => "Relation",
            Item::SerMax => "Max",
            Item::SerMask => "Mask",
            Item::UartPol => "Polarity",
            Item::UartStop => "Stop bits",
            Item::UartErr => "Error type",
            Item::SpiCs => "CS",
            Item::SpiTimeout => "Timeout",
            Item::I2cDir => "Direction",
            Item::I2cAddrBits => "Address",
            Item::I2cBytes => "Bytes",
            Item::DecCanH => "Signal",
            Item::DecUsbSpeed => "Speed",
            Item::DecOpt(_) => "Option",
            Item::DecSrc4 => "Source 4",
            Item::DecDiff => "B line (A-B)",
            Item::DecThr => "Threshold",
            Item::DecFmt => "Format",
            Item::DecProto => "Protocol",
            Item::DecSrc2 => "Clock",
            Item::DecCs => "CS",
            Item::DecMode => "Mode",
            Item::DecBits => "Data bits",
            Item::DecParity => "Parity",
            Item::ZScale => "Zoom time/div",
            Item::ZDelay => "Zoom position",
            Item::XyMode => "Format",
            Item::RefSrc => "Save from",
            Item::RefSave => "Save to next R",
            Item::RefSaveNext => "Store in the next free R",
            Item::RefShow(i) => ["R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10"][i.min(NREF - 1)],
            Item::RefClear => "Clear all",
            Item::MathOpI => "Operation",
            Item::MathFam => "Category",
            Item::MathA => "Source A",
            Item::MathB => "Source B",
            Item::MathScale => "Scale",
            Item::MathOffset => "Offset",
            Item::FftWin => "Window",
            Item::MathFc => "Frequency",
            Item::MathBw => "Bandwidth",
            Item::MathSel => "Channel",
            Item::MathOff => "Turn off",
            Item::MathAuto => "Auto scale",
            Item::Open(MenuId::MeasSelected) => "Selected",
            Item::Open(_) => "",
            Item::ChOn(_) => "Display",
            Item::ChCoupling(_) => "Coupling",
            Item::ChBw(_) => "Bandwidth",
            Item::ChProbe(_) => "Probe",
            Item::ChInvert(_) => "Invert",
            Item::ChScale(_) => "Scale",
            Item::ChOffset(_) => "Offset",
            Item::HScale => "Time/div",
            Item::HDelay => "Delay",
            Item::HZero => "Delay to 0",
            Item::AutoDeskew => "Auto deskew",
            Item::ClearDeskew => "Clear deskew",
            Item::TrigSrc => "Source",
            Item::TrigSlope => "Slope",
            Item::TrigMode => "Sweep",
            Item::TrigLevel => "Level",
            Item::Trig50 => "Level 50 %",
            Item::TrigType => "Type",
            Item::SearchOn => "Mark events",
            Item::TrigLevel2 => "Level 2",
            Item::TrigSrc2 => "Source B",
            Item::TrigSlope2 => "Slope B",
            Item::TrigPol => "Polarity",
            Item::TrigWhen => "When",
            Item::TrigT1 => "Time",
            Item::TrigT2 => "Upper limit",
            Item::VidStd => "Standard",
            Item::VidSync => "Sync",
            Item::VidLine => "Line",
            Item::Pat(0) => "CH1",
            Item::Pat(1) => "CH2",
            Item::Pat(2) => "CH3",
            Item::Pat(_) => "CH4",
            Item::PatLevel => "Level",
            Item::WinModeI => "Trigger on",
            Item::NthIdle => "Idle time",
            Item::NthN => "Edge no.",
            Item::Setup => "Setup",
            Item::Hold => "Hold",
            Item::AcqMode => "Mode",
            Item::AcqAvg => "Averages",
            Item::AcqSeg => "Segments",
            Item::AcqSegHw => "HW segments",
            Item::AcqAvgHw => "HW average",
            Item::AcqSegIvl => "Seg. interval",
            Item::AcqSegTable => "Segment times",
            Item::AcqDepth => "Memory",
            Item::AcqCores => "ADC cores",
            Item::AcqDrawPath => "Draw path",
            Item::AcqAlias => "Aliasing",
            Item::MeasSrc => "Source A",
            Item::MeasCopyTo => "Copy meas. to",
            Item::MeasSrcB => "Source B",
            Item::MeasPair(k) => tools::PAIRS[k.min(tools::PAIRS.len() - 1)].0,
            Item::DvmOn => "DVM",
            Item::DvmSrc => "DVM source",
            Item::DvmMode => "DVM mode",
            Item::CntOn => "Counter",
            Item::CntSrc => "Count source",
            Item::CntMode => "Count mode",
            Item::CntDigits => "Digits",
            Item::CntClear => "Clear counter",
            Item::TrigHoldoff => "Holdoff",
            Item::ChLabel(_) => "Name",
            Item::MathExprI => "Formula",
            Item::MathExprEdit => "Edit formula",
            Item::LogOn => "Logging",
            Item::LogIvl => "Interval",
            Item::LogInfo => "File",
            Item::ChFine => "Fine scale",
            Item::ChUnit(_) => "Unit",
            Item::ChBias(_) => "Zero bias",
            Item::ChBiasZero(_) => "Zero this input",
            Item::ChSkew(_) => "Deskew",
            Item::ChExpand => "Expand around",
            Item::HFine => "Fine time base",
            Item::HExpandI => "Expand around",
            Item::RefColor => "Slot color",
            Item::RefLabel => "Slot name",
            Item::RefSel => "Slot",
            Item::RefVpd => "Slot scale",
            Item::RefOffset => "Slot offset",
            Item::RefExport => "Export slot",
            Item::RefImport => "Import slot",
            Item::RefLoad => "Load file",
            Item::MaskSave => "Save mask",
            Item::MaskLoad => "Load mask",
            Item::MathLinA => "A",
            Item::MathLinB => "B",
            Item::MeasAdd(k) => meascatalog::meas_display_name(k),
            Item::MeasCfg(_) => "Set",
            Item::MeasClear => "Clear all",
            Item::MeasRemove(_) => "Remove",
            Item::CatSourceC => "Source C",
            Item::CatSourceD => "Source D",
            Item::CurMode => "Mode",
            Item::CurSrcB => "Cursor B source",
            Item::CurA => "Cursor A",
            Item::CurB => "Cursor B",
            Item::CurC => "Y1",
            Item::CurD => "Y2",
            Item::Persist => "Persistence",
            Item::Intensity => "Intensity",
            Item::GridStyle => "Grid",
            Item::GridBright => "Grid bright.",
            Item::GridFrame => "Grid frame",
            Item::TrigNoise => "Noise reject",
            Item::TrigAutoNoise => "Auto noise rej.",
            Item::TrigCoup => "Coupling",
            Item::ClearDisp => "Clear",
            Item::Dots => "Dots",
            Item::Layout => "Layout",
            Item::BottomBar => "Bottom bar",
            Item::Hdmi => "HDMI",
            Item::TextSize => "Text size",
            Item::ZoneSel => "Zone",
            Item::ZoneOn => "Enable",
            Item::ZoneSrc => "Source",
            Item::ZoneMode => "Condition",
            Item::ZoneBox => "Draw box",
            Item::ZoneShape => "Draw shape",
            Item::ZoneClear => "Delete",
            Item::ZInput => "Z input",
            Item::Histogram => "Histogram",
            Item::About => "About OOS",
            Item::SysInfo => "System info",
            Item::Shot => "Screenshot",
            Item::AuxOut => "AUX out",
            Item::Stock => "Back to stock",
            Item::Update => "Update",
            Item::BootMode => "Boot mode",
            Item::Overclock => "Overclock",
            Item::ViewExit => "Exit",
            Item::ViewSettings => "Settings",
            Item::SrcOn => "Switch on source",
            Item::WlanInfo => "WLAN details",
            Item::WlanScan => "Find WLAN networks",
            Item::WlanForget => "Forget WLAN network",
            Item::NetEth => "Ethernet",
            Item::NetMac => "MAC",
            Item::NetPorts => "Other access",
            Item::NetWlan => "WLAN",
            Item::WlanNet => "Networks",
            Item::WlanForgetNet => "Forget",
            Item::UpdateRollback => "Roll back update",
            Item::QuickAct => "Quick key",
            // drawn with the option text of the picker
            Item::PickOpt(_) => "",
            Item::RunStop => "Run/Stop",
            Item::SingleAct => "Single",
            Item::AutosetAct => "Autoset",
            Item::DefaultAct => "Default",
            Item::SlopeAct => "Slope",
            Item::ForceAct => "Force",
            Item::NavHist => "History",
            Item::NavBack => "Older",
            Item::NavFwd => "Newer",
            // drawn with the action (see draw_menu)
            Item::QuickRun => "Quick",
            Item::LockAct => "Touch lock",
            Item::PowerAct => "Power off",
            // analysers: ana_label
            _ => "",
        }
    }

    fn value(&self, it: Item) -> String {
        if it == Item::CatSourceC { return self.wave_name(self.cat_c); }
        // the desktop build reports its own web port and listeners (wlan.rs `net_value`)
        if it == Item::NetWeb && cfg!(windows) && !self.lan_status.is_empty() { return self.net_value(it); }
        if it == Item::CatSourceD { return self.wave_name(self.cat_d); }
        if let Some(v) = self.addrail_value(it).or_else(|| self.meas_value(it)).or_else(|| self.measrow_value(it)).or_else(|| self.ana_value(it)).or_else(|| self.magnetic_value(it)).or_else(|| self.inverter_value(it)).or_else(|| self.dpt_value(it)).or_else(|| self.cal_value(it)).or_else(|| self.bode_value(it)).or_else(|| self.sim_value(it)).or_else(|| self.offline_value(it)).or_else(|| self.afg_value(it)).or_else(|| self.bw_value(it)).or_else(|| self.la_value(it)).or_else(|| self.win_value(it)).or_else(|| self.bus_value(it)).or_else(|| self.time_value(it)).or_else(|| self.remote_value(it)).or_else(|| self.graphics_value(it)).or_else(|| self.views_value(it)).or_else(|| self.monitor_value(it)).or_else(|| self.features_value(it)).or_else(|| self.backlight_value(it)) {
            return v;
        }
        let m = &self.m;
        let onoff = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
        match it {
            Item::ChOn(c) => onoff(m.ch[c].on),
            Item::ChCoupling(c) => m.ch[c].coupling.name().into(),
            Item::ChBw(c) => BWS[m.ch[c].bw].0.into(),
            Item::ChProbe(c) => format!("{}x", m.ch[c].probe),
            Item::ChInvert(c) => onoff(m.ch[c].invert),
            Item::ChScale(c) => si_short(m.ch[c].volts_per_div() as f64, m.unit(c)),
            Item::ChOffset(c) => si(m.ch[c].offset as f64, m.unit(c)),
            Item::ChUnit(c) => m.unit(c).into(),
            Item::ChBias(c) => si(m.ch[c].bias as f64, m.unit(c)),
            Item::ChSkew(c) => si(m.ch[c].skew as f64, "s"),
            Item::ChExpand => if m.v_expand_gnd { "Ground" } else { "Center" }.into(),
            Item::HFine => onoff(m.fine_t),
            Item::HExpandI => format!("{:?}", m.h_expand),
            Item::RefColor => self.refs[self.ref_sel].as_ref().map_or("-", |r| REF_COLORS[r.color.min(REF_COLORS.len() - 1)].0).into(),
            Item::HScale => si_short(m.time_per_div() as f64, "s"),
            Item::HDelay => si(m.delay as f64, "s"),
            Item::TrigSrc => self.trigger_source_label(),
            Item::TrigSlope => match m.slope {
                Slope::Rise => "Rising".into(),
                Slope::Fall => "Falling".into(),
                Slope::Either => "Either".into(),
            },
            Item::TrigMode => format!("{:?}", m.trig_mode),
            Item::TrigLevel => si(m.trig_level as f64, m.unit(m.trig_src)),
            Item::TrigType => m.trig_type.name().into(),
            Item::SearchOn => if self.search_on { "On".into() } else { "Off".into() },
            Item::TrigLevel2 => si(m.trig_level2 as f64, m.unit(m.trig_src)),
            Item::TrigSrc2 => Self::trigger_wave_label(m.trigger_wave2()),
            Item::TrigSlope2 => match m.trig_slope2 {
                Slope::Rise => "Rising".into(),
                Slope::Fall => "Falling".into(),
                Slope::Either => "Either".into(),
            },
            Item::TrigPol => if m.trig_pol == Pol::Pos { "Positive".into() } else { "Negative".into() },
            Item::TrigWhen => match m.trig_when {
                WhenKind::Off => "Off".into(),
                WhenKind::Gt => ">".into(),
                WhenKind::Lt => "<".into(),
                WhenKind::In => "Inside".into(),
                WhenKind::Out => "Outside".into(),
            },
            Item::TrigT1 => si_short(m.trig_t1 as f64, "s"),
            Item::TrigT2 => si_short(m.trig_t2 as f64, "s"),
            Item::VidStd => if m.video_std == VideoStd::Pal { "PAL".into() } else { "NTSC".into() },
            Item::VidSync => match m.video_sync {
                VSync::AllLines => "Lines".into(),
                VSync::Line => "Line n".into(),
                VSync::Odd => "Odd".into(),
                VSync::Even => "Even".into(),
                VSync::AllFields => "Fields".into(),
            },
            Item::VidLine => format!("{}", m.video_line),
            Item::Pat(c) => match m.pat[c] {
                Bit::H => "H".into(),
                Bit::L => "L".into(),
                Bit::X => "X".into(),
                Bit::R => "Rise".into(),
                Bit::F => "Fall".into(),
            },
            Item::PatLevel => si(m.pat_levels[m.trig_src] as f64, "V"),
            Item::WinModeI => match m.win_mode {
                WinMode::Enter => "Enter".into(),
                WinMode::Exit => "Exit".into(),
                WinMode::Inside => "Inside".into(),
            },
            Item::NthIdle => si_short(m.nth_idle as f64, "s"),
            Item::NthN => format!("{}", m.nth_n),
            Item::Setup => si_short(m.setup as f64, "s"),
            Item::Hold => si_short(m.hold as f64, "s"),
            Item::AcqMode => match m.acq {
                AcqMode::Normal => "Normal".into(),
                AcqMode::Peak => "Peak".into(),
                AcqMode::Average => "Average".into(),
                AcqMode::HiRes => "High res".into(),
            },
            Item::AcqAvg => format!("{}", m.avg_count),
            Item::AcqSeg => if m.segments == 0 { "Off".into() } else { self.seg_count().to_string() },
            Item::AcqSegHw => onoff(m.seg_hw),
            Item::AcqAvgHw => onoff(m.avg_hw),
            Item::AcqSegIvl => seg_ivl_name(m.seg_ivl),
            Item::AcqDepth => mdepths()[m.mdepth].0.into(),
            Item::AcqCores => if m.cores { "Matched" } else { "Raw" }.into(),
            Item::AcqDrawPath => if m.draw_batch { "Batch" } else { "Standard" }.into(),
            Item::AcqAlias => self.alias_above().map_or(String::new(), |f| format!("above {}, Peak shows it", si_short(f, "Hz"))),
            Item::MeasSrc => self.wave_name(self.meas_src),
            Item::MeasCopyTo => match self.meas_copied {
                // what the last copy did, while the source is still the one it was made from
                Some((from, to, _)) if from == self.meas_src => format!("Copied to {}", self.wave_name(to)),
                _ => "Pick".into(),
            },
            Item::MeasSrcB => self.wave_name(self.meas_src_b),
            Item::CurSrcB => self.wave_name(self.cursor_b_source()),
            Item::ChFine => onoff(m.fine_v),
            Item::MathExprI => if m.math_expr[m.math_sel].is_empty() { "none".to_string() } else { m.math_expr[m.math_sel].clone() },
            Item::LogOn => onoff(self.log.file.is_some()),
            Item::LogIvl => si_short(logger::LOG_STEPS[m.log_ivl.min(logger::LOG_STEPS.len() - 1)], "s"),
            Item::LogInfo => self.log_info(),
            Item::CurC => self.cursor_values().get(2).map_or(format!("{:.0} %", m.cur_c * 100.0), |v| v.1.clone()),
            Item::CurD => self.cursor_values().get(3).map_or(format!("{:.0} %", m.cur_d * 100.0), |v| v.1.clone()),
            Item::RefSel => format!("R{}", self.ref_sel + 1),
            Item::RefVpd => self.refs[self.ref_sel].as_ref().map_or("-".into(), |r| format!("{}/div", si_short(r.vpd as f64, "V"))),
            Item::RefOffset => self.refs[self.ref_sel].as_ref().map_or("-".into(), |r| si_short(r.offset as f64, "V")),
            Item::DvmOn => onoff(m.dvm_on),
            Item::DvmSrc => format!("CH{}", m.dvm_src + 1),
            Item::DvmMode => tools::DVM_MODES[m.dvm_mode.min(2)].into(),
            Item::CntOn => onoff(m.cnt_on),
            Item::CntSrc => format!("CH{}", m.cnt_src + 1),
            Item::CntMode => tools::CNT_MODES[m.cnt_mode.min(2)].into(),
            Item::CntDigits => m.cnt_digits.to_string(),
            Item::TrigHoldoff => if m.holdoff <= 0.0 { "Off".into() } else { si_short(m.holdoff as f64, "s") },
            Item::MathLinA => format!("{:.3}", m.math_lin.0),
            Item::MathLinB => format!("{:.3}", m.math_lin.1),
            Item::CurMode => format!("{:?}", m.cursor),
            // where the cursor is, as the read-out under the plot says it; the percentage only without a record
            Item::CurA => self.cursor_values().first().map_or(format!("{:.0} %", m.cur_a * 100.0), |v| v.1.clone()),
            Item::CurB => self.cursor_values().get(if m.cursor == CursorMode::Track { 2 } else { 1 }).map_or(format!("{:.0} %", m.cur_b * 100.0), |v| v.1.clone()),
            Item::Persist => PERSIST[m.persist].0.into(),
            Item::Intensity => format!("{} %", m.intensity),
            Item::GridStyle => format!("{:?}", m.grid),
            Item::GridBright => format!("{} %", m.grid_bright),
            Item::GridFrame => onoff(m.grid_frame),
            Item::TrigNoise => onoff(m.trig_nrej),
            Item::TrigAutoNoise => onoff(m.auto_nrej),
            Item::TrigCoup => m.trig_coup.name().into(),
            Item::Dots => onoff(m.dots),
            Item::Layout => m.layout.name().into(),
            Item::BottomBar => if m.bar_compact { "Compact" } else { "Cards" }.into(),
            Item::Hdmi => format!("{:?}", m.hdmi),
            Item::TextSize => m.text_size.name().into(),
            Item::ZoneSel => format!("Z{}", self.zone_sel + 1),
            Item::ZoneOn => onoff(m.zones[self.zone_sel].on),
            Item::ZoneSrc => format!("CH{}", m.zones[self.zone_sel].src + 1),
            Item::ZoneMode => if m.zones[self.zone_sel].inside { "Intersect".into() } else { "Not intersect".into() },
            Item::ZInput => m.z_src.map_or("Off".to_string(), |c| format!("CH{}", c + 1)),
            Item::Histogram => format!("{:?}", m.hist),
            Item::AuxOut => m.aux_out.name().into(),
            Item::MathOpI => m.math.name().into(),
            Item::MathFam => self.math_fam.name().into(),
            Item::StoreDest => self.store_dest.name().into(),
            Item::MaskInvert => onoff(m.mask_inv),
            Item::MeasStats => onoff(self.stats_on),
            Item::MeasLabels => onoff(self.m.meas_labels),
            Item::StripDock => self.m.strip_dock.label().into(),
            Item::Zoom => onoff(m.zoom),
            Item::AutoRoll => onoff(m.auto_roll),
            Item::DecOn => onoff(m.dec_on),
            Item::DecSel => format!("D{}", m.dec_sel + 1),
            Item::PaneSel => tables::PANES[m.pane.min(tables::PANES.len() - 1)].into(),
            Item::TrendSel(k) => self.trend_options().get(m.trend_sel[k]).cloned().unwrap_or_else(|| "Auto".into()),
            Item::DecSrc => decode_source_name(m.dec_src),
            Item::DecBaud => format!("{}", crate::decode::BAUDS[m.dec_baud]),
            Item::DecCphyRate => crate::decode::cphy_rate_label(m.dec_cphy_rate),
            Item::DecCanRate => si(crate::decode::CAN_RATES[m.dec_canrate], "bit/s"),
            Item::TrigBusWhat => m.trig_bus.name().into(),
            Item::TrigBusVal => format!("0x{:X}", m.trig_bus_val),
            Item::SerRel | Item::SerMax | Item::SerMask | Item::UartPol | Item::UartStop | Item::UartErr | Item::SpiCs | Item::SpiTimeout | Item::I2cDir | Item::I2cAddrBits | Item::I2cBytes => self.serial_value(it),
            Item::DecCanH => if m.dec_canh { "CAN_H".into() } else { "RX/TX".into() },
            Item::DecUsbSpeed => USPEED_NAMES[m.dec_uspeed].into(),
            Item::DecSrc4 => m.dec_src4.map_or("None".into(), decode_source_name),
            Item::DecDiff => m.dec_diff.map_or("None".into(), decode_source_name),
            Item::DecThr => si(m.dec_thr as f64, "V"),
            Item::DecFmt => if m.dec_ascii { "ASCII".into() } else { "Hex".into() },
            Item::DecProto => m.dec_proto.name().into(),
            Item::DecOpt(k) => m.dec_proto.module().and_then(|p| p.options().get(k)).map_or(String::new(), |o| o.choices[m.dec_opts()[k]].into()),
            Item::DecSrc2 => decode_source_name(m.dec_src2),
            Item::DecCs => m.dec_cs.map_or("None".into(), decode_source_name),
            Item::DecMode => format!("{}", m.dec_mode),
            Item::DecBits => format!("{}", m.dec_bits),
            Item::DecParity => match m.dec_parity {
                oos_dsp::decode::Parity::None => "None".into(),
                oos_dsp::decode::Parity::Even => "Even".into(),
                oos_dsp::decode::Parity::Odd => "Odd".into(),
            },
            Item::ZScale => si_short(tscales()[m.zscale] as f64, "s"),
            Item::ZDelay => si(m.zdelay as f64, "s"),
            Item::XyMode => if self.xy { "XY".into() } else { "YT".into() },
            Item::BootMode => self.boot_mode().into(),
            Item::Overclock => overclock::label(self.clock_next),
            Item::NetEth | Item::NetMac | Item::NetPorts | Item::NetWlan => self.net_value(it),
            Item::RefSrc => format!("CH{}", self.ref_src + 1),
            Item::RefShow(i) => match &self.refs[i] {
                None => "empty".into(),
                Some(r) => {
                    let src = if r.label.is_empty() { format!("CH{}", r.src + 1) } else { r.label.clone() };
                    format!("{} {}", src, if r.on { "on" } else { "off" })
                }
            },
            Item::MathA => src_name(m.math_a),
            Item::MathB => src_name(m.math_b),
            Item::MathSel => format!("M{}", m.math_sel + 1),
            Item::MathBw => si_short(m.math_bw as f64, "Hz"),
            Item::MathScale => self.math_scale_text(),
            Item::MathOffset => format!("{:.2} div", m.math_offset),
            Item::FftWin => m.fft_win.name().into(),
            Item::MathFc => si_short(m.math_fc as f64, "Hz"),
            Item::ColorGrade => onoff(m.color_grade),
            Item::StoreSlot => format!("{}  (setup{}.txt)", self.store_slot, self.store_slot),
            Item::MaskOn => onoff(m.mask_on),
            Item::MaskSrc => format!("CH{}", m.mask_src + 1),
            Item::MaskX => format!("{:.2} div", m.mask_x),
            Item::MaskY => format!("{:.2} div", m.mask_y),
            Item::MaskAction => if m.mask_stop { "Stop".into() } else { "None".into() },
            Item::QuickAct => m.quick.name().into(),
            _ => String::new(),
        }
    }

    /// The segment items of the Acquire menu; false for any other item.
    fn seg_adjust(&mut self, it: Item, d: i32) -> bool {
        let cyc = |i: usize, n: usize, d: i32| -> usize { (i as i32 + d).rem_euclid(n as i32) as usize };
        match it {
            Item::AcqSeg => {
                let o = self.seg_options();
                let cur = if self.m.segments == 0 { 0 } else { self.seg_count() };
                let i = o.iter().position(|&n| n >= cur).unwrap_or(0);
                self.m.segments = o[cyc(i, o.len(), d)];
            }
            Item::AcqAvgHw => {
                self.m.avg_hw = !self.m.avg_hw;
                if self.m.avg_hw && !board().avg_hw(&self.m) {
                    self.toast("HW average needs a count of 2..1024 and ordinary records: software average");
                }
            }
            Item::AcqSegHw => {
                self.m.seg_hw = !self.m.seg_hw;
                // the other limit applies from now on
                self.m.segments = self.seg_count();
            }
            Item::AcqSegIvl => {
                let i = SEG_IVLS.iter().position(|&x| x >= self.m.seg_ivl).unwrap_or(0);
                self.m.seg_ivl = SEG_IVLS[cyc(i, SEG_IVLS.len(), d)];
            }
            _ => return false,
        }
        true
    }

    /// One press (or `d` presses) of a stepper. The limits are in the range table (`ranges.rs`).
    fn adjust(&mut self, it: Item, d: i32) {
        if self.item_blocked_toast(it) { return; }
        // a channel the user switches is theirs from then on, not one a source row turned on
        if let Item::ChOn(c) = it { self.auto_on.retain(|&a| a != c); }
        self.adjust_item(it, d);
        self.range_enforce(it);
    }

    fn adjust_item(&mut self, it: Item, d: i32) {
        if matches!(it, Item::CatSourceC | Item::CatSourceD) {
            let current = if it == Item::CatSourceC { self.cat_c } else { self.cat_d };
            let next = self.step_wave(current, d);
            if it == Item::CatSourceC { self.cat_c = next; } else { self.cat_d = next; }
            return;
        }
        if matches!(it, Item::MathA | Item::MathB) {
            let sources = self.math_source_ids(self.m.math_sel);
            let current = if it == Item::MathA { self.m.math_a } else { self.m.math_b };
            if !sources.is_empty() {
                let i = sources.iter().position(|&s| s == current).unwrap_or(0);
                let source = sources[(i as i32 + d).rem_euclid(sources.len() as i32) as usize];
                if it == Item::MathA { self.m.math_a = source; } else { self.m.math_b = source; }
                self.settings_changed();
            }
            return;
        }
        if self.addrail_adjust(it, d) || self.bus_adjust(it, d) || self.meas_adjust(it, d) || self.measrow_adjust(it, d) || self.ana_adjust(it, d) || self.magnetic_adjust(it, d) || self.inverter_adjust(it, d) || self.dpt_adjust(it, d) || self.cal_adjust(it, d) || self.bode_adjust(it, d) || self.sim_adjust(it, d) || self.offline_adjust(it, d) || self.afg_adjust(it, d) || self.bw_adjust(it, d) || self.la_adjust(it, d) || self.win_adjust(it, d) || self.time_adjust(it, d) || self.remote_adjust(it, d) || self.graphics_adjust(it, d) || self.views_adjust(it, d) || self.monitor_adjust(it, d) || self.features_adjust(it, d) || self.backlight_adjust(it, d) {
            return;
        }
        if matches!(it, Item::TrigSrc | Item::TrigSrc2) {
            let secondary = it == Item::TrigSrc2;
            let sources = self.trigger_sources();
            let selected = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
            if !sources.is_empty() {
                let pos = sources.iter().position(|&id| id == selected).unwrap_or(0);
                let id = sources[(pos as i32 + d).rem_euclid(sources.len() as i32) as usize];
                self.set_trigger_source(id, secondary);
            }
            return;
        }
        // the trigger times step on the 1-2-5 ladder; a value between two rungs (30 us) goes to the nearest one in the
        // direction of the press and the press back returns to it
        if matches!(it, Item::TrigT1 | Item::TrigT2 | Item::NthIdle | Item::Setup | Item::Hold) {
            let cur = match it { Item::TrigT1 => self.m.trig_t1, Item::TrigT2 => self.m.trig_t2, Item::NthIdle => self.m.nth_idle, Item::Setup => self.m.setup, _ => self.m.hold } as f64;
            let l: Vec<f64> = time_ladder().iter().map(|&x| x as f64).collect();
            let v = self.ladder_move(it, cur, d, &l) as f32;
            match it { Item::TrigT1 => self.m.trig_t1 = v, Item::TrigT2 => self.m.trig_t2 = v, Item::NthIdle => self.m.nth_idle = v, Item::Setup => self.m.setup = v, _ => self.m.hold = v }
            self.settings_changed();
            return;
        }
        // the steppers that add a step to a value and stop at the limits of the range table
        if self.range_step(it, d) {
            // the reference level restarts the averaging and the picture of the spectrum
            if it == Item::SpecRef {
                self.azr.spec_avg = None;
                self.analyzer_update();
            }
            // a level of a reference trigger does not restart what was measured
            if !(matches!(it, Item::TrigLevel | Item::TrigLevel2) && self.m.trig_ref.is_some()) {
                self.settings_changed();
            }
            return;
        }
        if self.seg_adjust(it, d) {
            return;
        }
        let m = &mut self.m;
        let step = |i: usize, n: usize, d: i32| -> usize { (i as i32 + d).clamp(0, n as i32 - 1) as usize };
        let cyc = |i: usize, n: usize, d: i32| -> usize { (i as i32 + d).rem_euclid(n as i32) as usize };
        match it {
            Item::ChOn(c) => {
                m.ch[c].on = !m.ch[c].on && c < n_ch();
                if m.ch[c].on && m.run == RunState::Run {
                    m.floor_live_tdiv();
                }
            }
            Item::ChCoupling(c) => {
                let all = board().couplings();
                let i = all.iter().position(|&x| x == m.ch[c].coupling).unwrap_or(0);
                m.ch[c].coupling = all[cyc(i, all.len(), d)];
            }
            Item::ChBw(c) => m.ch[c].bw = cyc(m.ch[c].bw, BWS.len(), d),
            Item::ChProbe(c) => {
                let all = [1.0, 10.0, 100.0];
                let i = all.iter().position(|&x| x == m.ch[c].probe).unwrap_or(0);
                m.ch[c].probe = all[cyc(i, 3, d)];
            }
            Item::ChInvert(c) => m.ch[c].invert = !m.ch[c].invert,
            Item::ChScale(c) => {
                let old = m.ch[c].volts_per_div();
                if m.fine_v {
                // row by row through the calibration table: each scale has its own record
                let rows = board().vscale_steps_nv();
                let now = VSCALES[m.ch[c].vscale] as f64 * m.ch[c].vfine as f64 * 1e9 * 0.999999;
                let at = rows.iter().position(|&k| k as f64 >= now).unwrap_or(rows.len() - 1) as i32;
                let want = (rows[(at + d).clamp(0, rows.len() as i32 - 1) as usize] as f64 * 1e-9) as f32;
                let i = VSCALES.iter().rposition(|&s| s <= want * 1.000001).unwrap_or(0);
                let f = want / VSCALES[i];
                (m.ch[c].vscale, m.ch[c].vfine) = (i, if (f - 1.0).abs() < 1e-6 { 1.0 } else { f });
                } else {
                    m.ch[c].vscale = step(m.ch[c].vscale, VSCALES.len(), d);
                    m.ch[c].vfine = 1.0;
                }
                // around ground: the zero line keeps its place on the screen
                if m.v_expand_gnd {
                    m.ch[c].offset *= m.ch[c].volts_per_div() / old;
                }
            }
            Item::ChUnit(c) => {
                let i = AmpUnit::ALL.iter().position(|&u| u == m.ch[c].unit).unwrap_or(0);
                m.ch[c].unit = AmpUnit::ALL[cyc(i, AmpUnit::ALL.len(), d)];
            }
            Item::ChExpand => m.v_expand_gnd = !m.v_expand_gnd,
            Item::HFine => m.fine_t = !m.fine_t,
            Item::HExpandI => {
                let all = [HExpand::Center, HExpand::Left, HExpand::Right, HExpand::Trigger];
                let i = all.iter().position(|&x| x == m.h_expand).unwrap_or(0);
                m.h_expand = all[cyc(i, all.len(), d)];
            }
            Item::ChFine => m.fine_v = !m.fine_v,
            Item::LogOn => {
                if self.log.file.is_some() {
                    self.log_stop("Log ended");
                } else {
                    self.log_start();
                }
            }
            Item::LogIvl => m.log_ivl = step(m.log_ivl, logger::LOG_STEPS.len(), d),
            Item::HScale => {
                let old = m.time_per_div();
                if m.fine_t {
                    (m.tscale, m.tfine) = fine_time(old, d);
                } else {
                    m.tscale = step(m.tscale, tscales().len(), d);
                    m.tfine = 1.0;
                }
                // Studio: the zoom keeps its factor and its place in the overview when the main time base changes
                let keep = (m.layout == Layout::Studio && m.zoom).then(|| (old / tscales()[m.zscale], (m.zdelay - m.delay) / (old * HDIV)));
                if m.zscale > m.tscale {
                    m.zscale = m.tscale;
                }
                // the delay is the time in the middle of the screen: keep the expand point where it is
                let new = m.time_per_div();
                let half = HDIV / 2.0;
                match m.h_expand {
                    HExpand::Center => {}
                    HExpand::Left => m.delay -= half * (old - new),
                    HExpand::Right => m.delay += half * (old - new),
                    HExpand::Trigger => m.delay *= new / old,
                }
                if let Some((factor, place)) = keep {
                    let want = new / factor;
                    m.zscale = tscales().iter().enumerate().min_by(|a, b| (a.1 / want).ln().abs().total_cmp(&(b.1 / want).ln().abs())).map_or(0, |(i, _)| i).min(m.tscale);
                    m.zdelay = m.delay + place * new * HDIV;
                }
            }
            Item::TrigSrc => {
                m.trig_src = src_step(m.trig_src, NCH, d);
            }
            Item::TrigSlope => {
                let all = [Slope::Rise, Slope::Fall, Slope::Either];
                let i = all.iter().position(|&x| x == m.slope).unwrap();
                m.slope = all[cyc(i, 3, d)];
            }
            Item::TrigMode => {
                let all = [TrigMode::Auto, TrigMode::Normal, TrigMode::Single];
                let i = all.iter().position(|&x| x == m.trig_mode).unwrap();
                m.trig_mode = all[cyc(i, 3, d)];
                if m.trig_mode == TrigMode::Single {
                    m.run = RunState::Run;
                }
            }
            Item::TrigType => {
                let i = TRIG_TYPES.iter().position(|&x| x == m.trig_type).unwrap_or(0);
                let t = TRIG_TYPES[cyc(i, TRIG_TYPES.len(), d)];
                self.set_trig_type(t);
            }
            Item::TrigSrc2 => m.trig_src2 = src_step(m.trig_src2, NCH, d),
            Item::SearchOn => { self.search_on = !self.search_on; return; }
            Item::TrigSlope2 => {
                let all = [Slope::Rise, Slope::Fall, Slope::Either];
                let i = all.iter().position(|&x| x == m.trig_slope2).unwrap();
                m.trig_slope2 = all[cyc(i, 3, d)];
            }
            Item::TrigPol => m.trig_pol = if m.trig_pol == Pol::Pos { Pol::Neg } else { Pol::Pos },
            Item::TrigWhen => {
                let all: &[WhenKind] = if m.trig_type == TrigType::Runt { &[WhenKind::Off, WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out] } else { &[WhenKind::Gt, WhenKind::Lt, WhenKind::In, WhenKind::Out] };
                let i = all.iter().position(|&x| x == m.trig_when).unwrap_or(0);
                m.trig_when = all[cyc(i, all.len(), d)];
            }
            Item::VidStd => m.video_std = if m.video_std == VideoStd::Pal { VideoStd::Ntsc } else { VideoStd::Pal },
            Item::VidSync => {
                let all = [VSync::AllLines, VSync::Line, VSync::Odd, VSync::Even, VSync::AllFields];
                let i = all.iter().position(|&x| x == m.video_sync).unwrap();
                m.video_sync = all[cyc(i, all.len(), d)];
            }
            Item::VidLine => m.video_line = (m.video_line as i32 + d).clamp(ranges::vid_line(m.video_std).0, ranges::vid_line(m.video_std).1) as u32,
            Item::Pat(c) => {
                let all = if m.trig_type == TrigType::Duration { &[Bit::H, Bit::L, Bit::X][..] } else { &[Bit::H, Bit::L, Bit::X, Bit::R, Bit::F][..] };
                let i = all.iter().position(|&x| x == m.pat[c]).unwrap_or(0);
                m.pat[c] = all[cyc(i, all.len(), d)];
            }
            Item::WinModeI => {
                let all = [WinMode::Enter, WinMode::Exit, WinMode::Inside];
                let i = all.iter().position(|&x| x == m.win_mode).unwrap();
                m.win_mode = all[cyc(i, 3, d)];
            }
            Item::NthN => m.nth_n = (m.nth_n as i32 + d).clamp(ranges::NTH_N.0, ranges::NTH_N.1) as u32,
            Item::AcqMode => {
                let all = [AcqMode::Normal, AcqMode::Peak, AcqMode::Average, AcqMode::HiRes];
                let i = all.iter().position(|&x| x == m.acq).unwrap();
                m.acq = all[cyc(i, 4, d)];
            }
            Item::AcqAvg => {
                let i = AVERAGES.iter().position(|&x| x == m.avg_count).unwrap_or(3);
                m.avg_count = AVERAGES[step(i, AVERAGES.len(), d)];
            }
            Item::AcqDepth => m.mdepth = cyc(m.mdepth, mdepths().len(), d),
            Item::MeasSrc => {
                let cur = self.meas_src;
                self.meas_src = self.step_wave(cur, d);
            }
            Item::MeasSrcB => {
                let cur = self.meas_src_b;
                self.meas_src_b = self.step_wave(cur, d);
            }
            // Track cursor B on its own source (follows cursor A's until set)
            Item::CurSrcB => {
                let cur = self.cursor_b_source();
                let next = self.step_wave(cur, d);
                self.set_cursor_b_source(next);
            }
            Item::DvmOn => m.dvm_on = !m.dvm_on,
            Item::DvmSrc => m.dvm_src = src_step(m.dvm_src, NCH, d),
            Item::DvmMode => m.dvm_mode = (m.dvm_mode as i32 + d).rem_euclid(3) as usize,
            Item::CntOn => m.cnt_on = !m.cnt_on,
            Item::CntSrc => m.cnt_src = src_step(m.cnt_src, NCH, d),
            Item::CntMode => m.cnt_mode = (m.cnt_mode as i32 + d).rem_euclid(3) as usize,
            Item::CntDigits => m.cnt_digits = (m.cnt_digits as i32 + d).clamp(ranges::CNT_DIGITS.0, ranges::CNT_DIGITS.1) as u32,
            Item::TrigHoldoff => m.holdoff = holdoff_step(m.holdoff, d),
            Item::MathLinA => m.math_lin.0 = (m.math_lin.0 + 0.1 * d as f32).clamp(ranges::MATH_LIN.0, ranges::MATH_LIN.1),
            Item::MathLinB => m.math_lin.1 = (m.math_lin.1 + 0.1 * d as f32).clamp(ranges::MATH_LIN.0, ranges::MATH_LIN.1),
            Item::CurMode => {
                let all = [CursorMode::Off, CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy];
                let i = all.iter().position(|&x| x == m.cursor).unwrap();
                m.cursor = all[cyc(i, all.len(), d)];
            }
            // a cursor on a voltage reads higher the further up the screen it is: + raises the reading (moves it up)
            Item::CurA | Item::CurB | Item::CurC | Item::CurD => {
                let vertical = matches!(it, Item::CurC | Item::CurD) || m.cursor == CursorMode::Volt;
                let delta = d as f32 * 0.005 * if vertical { -1.0 } else { 1.0 };
                let field = match it { Item::CurA => &mut m.cur_a, Item::CurB => &mut m.cur_b, Item::CurC => &mut m.cur_c, _ => &mut m.cur_d };
                *field = (*field + delta).clamp(0.0, 1.0);
            }
            Item::RefSel => {
                let slot = (self.ref_sel as i32 + d).clamp(0, NREF as i32 - 1) as usize;
                self.select_workspace_source(WaveId::Ref(slot));
            }
            Item::RefColor => {
                if let Some(r) = &mut self.refs[self.ref_sel] {
                    r.color = cyc(r.color.min(REF_COLORS.len() - 1), REF_COLORS.len(), d);
                }
            }
            Item::RefVpd => {
                if let Some(r) = &mut self.refs[self.ref_sel] {
                    let i = VSCALES.iter().position(|&s| s >= r.vpd * 0.999).unwrap_or(VSCALES.len() - 1);
                    r.vpd = VSCALES[(i as i32 + d).clamp(0, VSCALES.len() as i32 - 1) as usize];
                }
                self.measure_refs();
            }
            Item::Persist => {
                m.persist = cyc(m.persist, PERSIST.len(), d);
                self.ph.clear();
            }
            Item::Intensity => m.intensity = (m.intensity as i32 + d * 5).clamp(ranges::INTENSITY.0, ranges::INTENSITY.1) as u32,
            Item::ZoneSel => self.zone_sel = cyc(self.zone_sel, 2, d),
            Item::ZoneOn => {
                let z = &mut m.zones[self.zone_sel];
                z.on = !z.on;
            }
            Item::ZoneSrc => {
                let z = &mut m.zones[self.zone_sel];
                z.src = src_step(z.src, NCH, d);
            }
            Item::ZoneMode => {
                let z = &mut m.zones[self.zone_sel];
                z.inside = !z.inside;
            }
            Item::Dots => {
                m.dots = !m.dots;
                self.dirty_map = true;
            }
            Item::Layout => {
                let i = LAYOUTS.iter().position(|&l| l == m.layout).unwrap_or(0);
                m.layout = LAYOUTS[cyc(i, LAYOUTS.len(), d)];
                if m.layout == Layout::Studio { self.studio.enter = true; }
                self.dirty_map = true;
            }
            Item::BottomBar => {
                m.bar_compact = !m.bar_compact;
                self.dirty_map = true;
            }
            Item::AcqCores => m.cores = !m.cores,
            Item::AcqDrawPath => {
                m.draw_batch = !m.draw_batch;
                self.dirty_map = true;
            }
            Item::Hdmi => {
                let i = HDMI_MODES.iter().position(|&x| x == m.hdmi).unwrap_or(0);
                m.hdmi = HDMI_MODES[step(i, HDMI_MODES.len(), d)];
                if self.m.hdmi == HdmiMode::Layout {
                    self.toast("HDMI Layout: a second picture each monitor frame, the panel may get fewer frames");
                }
            }
            Item::TextSize => {
                let i = m.text_size.index();
                m.text_size = TEXT_SIZES[(i as i32 + d).clamp(0, TEXT_SIZES.len() as i32 - 1) as usize];
            }
            Item::ZInput => {
                let i = m.z_src.map_or(0, |c| c + 1);
                let i = opt_src_step(i, d);
                m.z_src = if i == 0 { None } else { Some(i - 1) };
                self.dirty_map = true;
            }
            Item::Histogram => {
                let all = [Hist::Off, Hist::Vertical, Hist::Horizontal];
                let i = all.iter().position(|&h| h == m.hist).unwrap_or(0);
                m.hist = all[cyc(i, 3, d)];
            }
            Item::GridBright => m.grid_bright = (m.grid_bright as i32 + 10 * d).clamp(ranges::GRID_BRIGHT.0, ranges::GRID_BRIGHT.1) as u32,
            Item::GridFrame => m.grid_frame = !m.grid_frame,
            Item::TrigNoise => m.trig_nrej = !m.trig_nrej,
            Item::TrigAutoNoise => m.auto_nrej = !m.auto_nrej,
            Item::TrigCoup => {
                let i = TRIG_COUPS.iter().position(|&x| x == m.trig_coup).unwrap_or(0);
                m.trig_coup = TRIG_COUPS[cyc(i, TRIG_COUPS.len(), d)];
            }
            Item::GridStyle => {
                let all = [Grid::Full, Grid::Light, Grid::None];
                let i = all.iter().position(|&x| x == m.grid).unwrap();
                m.grid = all[cyc(i, 3, d)];
            }
            Item::AuxOut => {
                let all = AuxOut::choices(board().mask_hw() || board().zone_hw());
                let i = all.iter().position(|&a| a == m.aux_out).unwrap_or(0);
                m.aux_out = all[cyc(i, all.len(), d)];
            }
            Item::QuickAct => {
                let i = QUICKS.iter().position(|&q| q == m.quick).unwrap_or(0);
                m.quick = QUICKS[cyc(i, QUICKS.len(), d)];
            }
            Item::MathOpI => {
                let fam = self.math_fam;
                let mut ops = vec![MathOp::Off];
                ops.extend_from_slice(fam.ops());
                let i = ops.iter().position(|&x| x == m.math).unwrap_or(0);
                m.math = ops[cyc(i, ops.len(), d)];
                m.math_offset = 0.0;
                self.math_autoscale();
            }
            Item::MathFam => {
                let i = MathFamily::ALL.iter().position(|&f| f == self.math_fam).unwrap_or(0);
                self.math_fam = MathFamily::ALL[cyc(i, MathFamily::ALL.len(), d)];
                if m.math != MathOp::Off && m.math.family() != self.math_fam {
                    m.math = self.math_fam.ops()[0];
                    m.math_offset = 0.0;
                    self.math_autoscale();
                }
            }
            // sources: CH1..4 and the math channels below the selected one
            Item::MathA => m.math_a = src_step(m.math_a.min(NCH + m.math_sel - 1), NCH + m.math_sel, d),
            Item::MathB => m.math_b = src_step(m.math_b.min(NCH + m.math_sel - 1), NCH + m.math_sel, d),
            Item::MathSel => {
                let k = cyc(m.math_sel, NMATH, d);
                m.math_select(k);
                if m.math != MathOp::Off {
                    self.math_fam = m.math.family();
                }
            }
            Item::MathBw => {
                let l = time_ladder();
                let per = 1.0 / m.math_bw.max(1e-6);
                let i = l.iter().position(|&x| x >= per * 0.999).unwrap_or(0);
                m.math_bw = 1.0 / l[step(i, l.len(), -d)];
            }
            Item::MathScale => {
                let n = if m.math == MathOp::Fft { FFT_DB.len() } else { math_scales().len() };
                m.math_scale = step(m.math_scale, n, d);
            }
            Item::FftWin => {
                let i = WINDOWS.iter().position(|&w| w == m.fft_win).unwrap_or(0);
                m.fft_win = WINDOWS[cyc(i, WINDOWS.len(), d)];
            }
            Item::MathFc => {
                let l = time_ladder();
                let per = 1.0 / m.math_fc.max(1e-6);
                let i = l.iter().position(|&x| x >= per * 0.999).unwrap_or(0);
                m.math_fc = 1.0 / l[step(i, l.len(), -d)];
            }
            Item::MeasStats => self.stats_on = !self.stats_on,
            Item::MeasLabels => self.m.meas_labels = !self.m.meas_labels,
            Item::StripDock => {
                let all = StripDock::ALL;
                let i = all.iter().position(|&s| s == m.strip_dock).unwrap_or(0);
                m.strip_dock = all[(i as i32 + d).clamp(0, all.len() as i32 - 1) as usize];
                // the statistics card follows the dock the strip uses
                self.workspace.measure_docked = true; self.workspace.measure_float = None;
            }
            Item::DecOn => self.m.dec_on = !self.m.dec_on,
            Item::DecSel => {
                let k = (self.m.dec_sel as i32 + d).rem_euclid(DECODERS as i32) as usize;
                self.m.dec_select(k);
            }
            Item::PaneSel => {
                self.m.pane = (self.m.pane as i32 + d).rem_euclid(tables::PANES.len() as i32) as usize;
                // a pane chosen from the Windows page in Scope makes it Split
                if self.m.density == Density::Scope && self.m.pane != 0 { self.m.density = Density::Split; }
                if self.m.density == Density::Split {
                    self.m.split_pane = self.m.pane;
                    let prev = self.m.win_layout;
                    self.m.win_layout = if self.m.pane == 0 { WinLayout::Rows } else { WinLayout::One };
                    if self.m.win_layout != prev && self.m.win_layout != WinLayout::One {
                        self.spread_wins();
                        if self.m.zoom {
                            self.toast("Windows show while zoom is off");
                        }
                    }
                } else if self.m.pane != 0 && self.pane_h() == 0 {
                    self.toast("No room for a second pane here: fewer LA lanes or the scope view");
                }
            }
            Item::TrendSel(k) => {
                let n = self.trend_options().len() as i32;
                self.m.trend_sel[k] = (self.m.trend_sel[k].min(n as usize - 1) as i32 + d).rem_euclid(n) as usize;
            }
            Item::DecSrc => { let next = self.decoder_source_step(Some(self.m.dec_src), false, d); self.configure_decoder_source(0, next); }
            Item::DecBaud => self.m.dec_baud = step(self.m.dec_baud, crate::decode::BAUDS.len(), d),
            Item::DecCphyRate => {
                let rates = crate::decode::CPHY_RATES;
                let index = rates.iter().position(|&rate| rate >= self.m.dec_cphy_rate).unwrap_or(rates.len() - 1);
                self.m.dec_cphy_rate = rates[cyc(index, rates.len(), d)];
            }
            Item::DecCanRate => self.m.dec_canrate = step(self.m.dec_canrate, crate::decode::CAN_RATES.len(), d),
            Item::TrigBusWhat => {
                let i = BUS_WHATS.iter().position(|&x| x == self.m.trig_bus).unwrap_or(0);
                self.m.trig_bus = BUS_WHATS[cyc(i, BUS_WHATS.len(), d)];
            }
            Item::TrigBusVal => self.m.trig_bus_val = (self.m.trig_bus_val as i64 + d as i64).clamp(0, BUS_VAL_MAX as i64) as u64,
            Item::SerRel | Item::SerMax | Item::SerMask | Item::UartPol | Item::UartStop | Item::UartErr | Item::SpiCs | Item::SpiTimeout | Item::I2cDir | Item::I2cAddrBits | Item::I2cBytes => self.serial_adjust(it, d),
            Item::DecCanH => self.m.dec_canh = !self.m.dec_canh,
            Item::DecFmt => self.m.dec_ascii = !self.m.dec_ascii,
            Item::DecProto => {
                // only the decoders System > Features offers (none: a hand-edited system.conf, see `load_system`)
                let on = self.protos_on();
                if !on.is_empty() {
                    let i = on.iter().position(|&x| x == self.m.dec_proto).unwrap_or(0);
                    self.set_dec_proto(on[cyc(i, on.len(), d)]);
                }
            }
            Item::DecUsbSpeed => self.m.dec_uspeed = cyc(self.m.dec_uspeed, crate::decode::USB_SPEEDS.len(), d),
            Item::DecOpt(k) => {
                if let Some(o) = self.m.dec_proto.module().and_then(|p| p.options().get(k)) {
                    self.m.dec_opt[k] = cyc(self.m.dec_opts()[k], o.choices.len(), d) as u8;
                }
            }
            Item::DecSrc4 => {
                let next = self.decoder_source_step(self.m.dec_src4, true, d);
                self.configure_decoder_source(3, next);
            }
            Item::DecDiff => {
                let next = self.decoder_source_step(self.m.dec_diff, true, d);
                self.configure_decoder_source(4, next);
            }
            Item::DecSrc2 => { let next = self.decoder_source_step(Some(self.m.dec_src2), false, d); self.configure_decoder_source(1, next); }
            Item::DecCs => {
                let next = self.decoder_source_step(self.m.dec_cs, self.m.dec_proto != Proto::CPHY, d);
                self.configure_decoder_source(2, next);
            }
            Item::DecMode => self.m.dec_mode = cyc(self.m.dec_mode as usize, 4, d) as u8,
            Item::DecBits => {
                let (lo, hi) = if self.m.dec_proto == Proto::UART { (5, 9) } else { (4, 32) };
                self.m.dec_bits = (self.m.dec_bits as i32 + d).clamp(lo, hi) as u32;
            }
            Item::DecParity => {
                use oos_dsp::decode::Parity;
                let all = [Parity::None, Parity::Even, Parity::Odd];
                let i = all.iter().position(|&x| x == self.m.dec_parity).unwrap();
                self.m.dec_parity = all[cyc(i, 3, d)];
            }
            Item::Zoom => {
                let on = !self.m.zoom;
                self.set_zoom(on);
            }
            Item::AutoRoll => self.m.auto_roll = !self.m.auto_roll,
            Item::ZScale => {
                self.m.zscale = step(self.m.zscale, tscales().len(), d);
                self.clamp_zoom();
            }
            Item::ZDelay => {
                self.m.zdelay += d as f32 * tscales()[self.m.zscale] / 25.0;
                self.clamp_zoom();
            }
            Item::XyMode => self.xy = !self.xy_over(),
            Item::BootMode => self.boot_mode_toggle(),
            Item::Overclock => self.overclock_step(d),
            Item::ColorGrade => self.m.color_grade = !self.m.color_grade,
            Item::StoreSlot => self.store_slot = (self.store_slot as i32 + d).clamp(ranges::STORE_SLOT.0, ranges::STORE_SLOT.1) as u32,
            Item::StoreDest => {
                let i = crate::store::StoreDest::ALL.iter().position(|&x| x == self.store_dest).unwrap_or(0);
                self.store_dest = crate::store::StoreDest::ALL[cyc(i, crate::store::StoreDest::ALL.len(), d)];
            }
            Item::MaskOn => {
                self.m.mask_on = !self.m.mask_on;
                if self.m.mask_on && self.mask.is_none() {
                    self.create_mask();
                }
            }
            Item::MaskSrc => {
                // a mask belongs to its channel: the new source needs a new one
                self.m.mask_src = src_step(self.m.mask_src, NCH, d);
                self.mask = None;
                self.mask_ref = None;
                self.m.mask_on = false;
            }
            Item::MaskX => {
                self.m.mask_x = ((self.m.mask_x + d as f32 * 0.04) * 100.0).round().clamp(0.0, 200.0) / 100.0;
                self.build_mask();
            }
            Item::MaskY => {
                self.m.mask_y = ((self.m.mask_y + d as f32 * 0.04) * 100.0).round().clamp(4.0, 400.0) / 100.0;
                self.build_mask();
            }
            Item::MaskAction => self.m.mask_stop = !self.m.mask_stop,
            Item::MaskInvert => {
                self.m.mask_inv = !self.m.mask_inv;
                self.build_mask();
            }
            Item::RefSrc => self.ref_src = src_step(self.ref_src, NCH, d),
            Item::RefShow(i) => {
                if let Some(r) = &mut self.refs[i] {
                    r.on = !r.on;
                }
            }
            _ => {}
        }
        // moving the strip does not change what was measured
        if it == Item::StripDock {
            self.relayout();
            return;
        }
        // nor does the pane that shows the values, or which values a trend chart shows
        if matches!(it, Item::PaneSel | Item::TrendSel(_)) {
            self.relayout();
            if self.m.run == RunState::Run { self.dirty_map = true; }
            return;
        }
        self.settings_changed();
        if it == Item::Hdmi {
            self.sync_full();
        }
    }

    fn clear_measurement_statistics(&mut self) {
        self.mstat.clear();
        // the settings of a row that is no longer selected go with it
        if !self.m.row_cfg.is_empty() {
            let keys = self.m.row_keys();
            self.m.row_cfg.retain(|(k, _)| keys.contains(k));
        }
        self.replay.clear_deferred_statistics();
        self.reset_catalog_statistics();
    }

    fn settings_changed(&mut self) {
        // whatever moved the upper level (50 %, SCPI's LEVel, a fit) leaves the two levels in order
        self.m.order_trigger_levels();
        self.cancel_bus_candidate(true);
        if self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run { self.single_armed = false; self.scpi_arm_operation(); }
        if self.m.run == RunState::Run {
            self.dirty_map = true;
        }
        self.clear_measurement_statistics();
    }

    fn activate(&mut self, it: Item) {
        if self.item_blocked_toast(it) { return; }
        if self.addrail_activate(it) { return; }
        if self.measrow_activate(it) { return; }
        if Self::kind(it) == Kind::Action && (self.ana_activate(it) || self.cal_activate(it) || self.bode_activate(it) || self.sim_activate(it) || self.afg_activate(it) || self.time_activate(it) || self.ssh_activate(it)) {
            return;
        }
        match Self::kind(it) {
            Kind::Choice => self.adjust(it, 1),
            Kind::Value => {}
            Kind::Action => match it {
                Item::Open(MenuId::La) if !self.m.la.on && !self.la_probe_ok() => self.toast("No LA probe connected"),
                // start menu > Measure: the measurement page, as the Measure button and key open it
                Item::Open(MenuId::Measure) if self.menu == MenuId::Main => {
                    self.ensure_meas_own();
                    self.open_submenu(MenuId::MeasAdd);
                }
                Item::Open(id) => {
                    if id == MenuId::Cursor && self.m.cursor == CursorMode::Off {
                        // like the CURSOR key
                        self.m.cursor = CursorMode::Time;
                    }
                    self.open_submenu(id);
                }
                // start menu: the front-panel key functions; one-shot actions close the menu, stepping ones keep it
                Item::RunStop | Item::SingleAct | Item::AutosetAct | Item::DefaultAct | Item::ForceAct | Item::LockAct => {
                    self.close_menu();
                    self.key(match it {
                        Item::RunStop => key::RUNSTOP,
                        Item::SingleAct => key::SINGLE,
                        Item::AutosetAct => key::AUTO,
                        Item::DefaultAct => key::DEFAULT,
                        Item::ForceAct => key::FORCE,
                        _ => key::TOUCHLOCK,
                    });
                }
                Item::SlopeAct => self.key(key::SLOPE),
                Item::NavHist => self.key(key::NAVIGATE),
                Item::NavBack => self.key(0x08),
                Item::NavFwd => self.key(0x06),
                Item::QuickRun => {
                    self.close_menu();
                    self.quick();
                }
                Item::PowerAct => {
                    // the dialog of the POWER key, without the MCU answer that belongs to a key press
                    self.close_menu();
                    self.dialog = None;
                    self.power_dlg = true;
                }
                Item::HZero => {
                    self.m.delay = 0.0;
                    self.settings_changed();
                }
                Item::AutoDeskew => self.auto_deskew(),
                Item::ClearDeskew => self.clear_deskew(),
                Item::Trig50 => self.level_50(),
                Item::PickOpt(k) if self.pick.is_some_and(|p| Self::hex_item(p.it)) => {
                    // keypad: digits edit the entry, OK sets the value and goes back
                    match KEYPAD.get(k).copied() {
                        Some("OK") => {
                            let v = u64::from_str_radix(if self.keypad.is_empty() { "0" } else { &self.keypad }, 16).unwrap_or(0).min(BUS_VAL_MAX);
                            if let Some(p) = self.pick {
                                self.set_hex(p.it, v);
                            }
                            self.close_picker();
                        }
                        Some("Clear") => self.keypad.clear(),
                        Some("Del") => {
                            self.keypad.pop();
                        }
                        Some(d) if self.keypad.len() < 10 => {
                            if self.keypad == "0" {
                                self.keypad.clear();
                            }
                            self.keypad.push_str(d);
                        }
                        _ => {}
                    }
                }
                Item::PickOpt(k) if self.pick.map_or(false, |p| p.it == Item::AfgFreqEntry) => self.freq_key(k),
                Item::PickOpt(k) if self.pick.map_or(false, |p| p.it == Item::WlanNet) => {
                    self.close_picker();
                    self.wlan_pick(k);
                }
                Item::PickOpt(k) if self.pick.is_some_and(|p| matches!(p.it, Item::NtpServer | Item::TimeZone | Item::RemName | Item::RemAllow)) => {
                    let it = self.pick.map(|p| p.it).unwrap_or(Item::TimeZone);
                    self.close_picker();
                    self.time_pick(it, k);
                }
                Item::PickOpt(k) if self.pick.map_or(false, |p| p.it == Item::WlanForgetNet) => {
                    self.close_picker();
                    self.wlan_forget_pick(k);
                }
                Item::PickOpt(k) if self.pick.is_some_and(|p| p.it == Item::SshRemoveKey) => {
                    self.close_picker();
                    self.ssh_remove_pick(k);
                }
                Item::PickOpt(k) if self.pick.map_or(false, |p| p.it == Item::RefImport) => {
                    let p = self.ref_files.get(k).cloned();
                    self.close_picker();
                    if let Some(p) = p {
                        self.ref_load_path(&p);
                    }
                }
                Item::PickOpt(k) => {
                    if let Some(p) = self.pick {
                        let opts = self.options(p.it);
                        self.close_picker();
                        if let Some(o) = opts.get(k) {
                            self.select_option(p.it, o);
                        }
                    }
                }
                Item::AddMeas => {
                    if let Some(k) = self.next_usual_meas() {
                        self.activate(Item::MeasAdd(k));
                    }
                }
                Item::MeasCfg(i) => self.open_meas_cfg(i as usize),
                Item::MeasAdd(k) => {
                    let e = (k, self.meas_src);
                    if let Some(index) = self.m.meas.iter().position(|item| *item == e) {
                        self.m.meas.remove(index);
                        self.meas_own.remove(&e);
                        if self.meas_cfg == Some(index as u16) {
                            self.meas_cfg = None;
                        } else if self.meas_cfg.is_some_and(|n| n as usize > index) {
                            self.meas_cfg = self.meas_cfg.map(|n| n - 1);
                        }
                        self.clear_measurement_statistics();
                        self.toast(&format!("{} {} removed", meascatalog::meas_display_name(k), self.wave_name(self.meas_src)));
                        return;
                    } else {
                        if !self.meas_make_room() { return; }
                        self.m.meas.push(e);
                        self.clear_measurement_statistics();
                    }
                    if k == Meas::Burst && !(self.m.meas_idle > 0.0) {
                        if let Some((a, _)) = self.meas_analysis(self.meas_src) {
                            let p = a.period();
                            if p.is_finite() && p > 0.0 {
                                self.m.meas_idle = p;
                            }
                        }
                    }
                    self.ensure_meas_own();
                    self.toast(&format!("{} {} added", meascatalog::meas_display_name(k), self.wave_name(self.meas_src)));
                }
                Item::MeasPair(k) => self.meas_pair_toggle(k),
                Item::CntClear => {
                    if self.cnt_total == 0 && self.cnt_hw.is_none() {
                        self.toast(NOTHING_TO_CLEAR);
                    } else {
                        self.counter_clear();
                        self.toast("Counter cleared");
                    }
                }
                Item::DecTable => self.table_toggle(tables::TableKind::Decode),
                Item::DecExport => self.events_export(tables::TableKind::Decode),
                Item::SearchTable => self.table_toggle(tables::TableKind::Search),
                Item::SearchSave => self.events_export(tables::TableKind::Search),
                Item::StoreBin => self.store_wave(false, None),
                Item::RefExport => self.ref_export(),
                Item::RefImport if self.sheets_on() => self.studio_load_file(self.ref_sel),
                Item::RefImport => self.ref_import(),
                Item::RefLoad => self.ref_load(),
                Item::MaskSave => self.mask_save(),
                Item::MaskLoad => self.mask_load(),
                Item::ChLabel(c) => self.wlan.osk = Some(wlan::Osk::for_label(c, &self.m.ch_label[c])),
                Item::BusLabel => self.wlan.osk = Some(wlan::Osk::for_bus(&self.m.bus.label)),
                Item::RefLabel => match &self.refs[self.ref_sel] {
                    Some(r) => self.wlan.osk = Some(wlan::Osk::for_ref(self.ref_sel, &r.label)),
                    None => self.toast("That slot is empty"),
                },
                Item::ChBiasZero(c) => self.zero_input(c),
                Item::MathExprEdit => self.wlan.osk = Some(wlan::Osk::for_expr(self.m.math_sel, &self.m.math_expr[self.m.math_sel])),
                Item::MeasRemove(index) => self.remove_measurement(index as usize),
                Item::MeasClear => {
                    self.m.meas.clear();
                    self.m.meas2.clear();
                    self.m.cat.clear();
                    self.meas_own.clear();
                    self.meas_cfg = None;
                    self.clear_measurement_statistics();
                }
                Item::RefSave | Item::RefSaveNext => {
                    let c = self.ref_src;
                    let rec = self.last[0].clone();
                    let saved = rec.as_ref().filter(|r| !r.ch[c].is_empty()).map(|r| {
                        let (v, dt) = self.meas_wave(r, c);
                        let t0 = if v.len() > r.ch[c].len() { r.raw.as_ref().map(|x| x.t0).unwrap_or(r.t0) } else { r.t0 };
                        RefWave { src: c, color: 0, label: String::new(), t0, dt, v: v.into(), vpd: self.m.ch[c].volts_per_div(), offset: self.m.ch[c].offset, on: true }
                    });
                    match saved {
                        Some(w) => {
                            let next = self.refs.iter().position(|r| r.is_none()).unwrap_or(NREF - 1);
                            // Studio: into the slot the picker shows; the secondary action goes to the next free one
                            let slot = if self.sheets_on() && it != Item::RefSaveNext { self.ref_sel } else { next };
                            self.m.deskew_ref[slot] = self.m.deskew_ch[c];
                            self.refs[slot] = Some(w);
                            self.toast(&format!("CH{} saved to R{}", c + 1, slot + 1));
                            self.measure_refs();
                            self.analyzer_update();
                        }
                        None => self.toast("Source channel has no data"),
                    }
                }
                Item::RefClear => {
                    self.refs = std::array::from_fn(|_| None);
                    self.measure_refs();
                    self.analyzer_update();
                },
                Item::ClearDisp => {
                    if self.menu == MenuId::Main {
                        self.close_menu();
                    }
                    self.clear_display();
                }
                Item::SysInfo => self.sysinfo_open(),
                Item::ViewExit => self.view_exit(),
                Item::ViewSettings => self.view_settings(),
                Item::About => {
                    let p = oos_hal::model::platform();
                    let build = std::fs::read_to_string("/etc/oos-build").unwrap_or_default();
                    let mut d = vec![
                        "OOS".into(),
                        "An open operating system for the DHO800/DHO900".into(),
                        if build.is_empty() { format!("oos-ui {}", env!("CARGO_PKG_VERSION")) } else { format!("Build {}", oos_hal::update::short(build.trim())) },
                        // the commit this binary was built from: the same line on the scope and on a desktop
                        option_env!("OOS_GIT").map_or(String::new(), |g| format!("Commit {g}")),
                        self.info.clone(),
                        format!("Rigol {}", p.ident()),
                        p.caps_line(),
                    ];
                    // experimental limits (limits.conf) and detection problems: the details are on System info
                    if p.experimental() {
                        d.push(format!("EXPERIMENTAL limits ({}), see System info", p.overrides.len()));
                    }
                    if !p.warnings.is_empty() {
                        d.push(format!("{} warning(s), see System info", p.warnings.len()));
                    }
                    d.extend([
                        format!("Source: {}   {:.0} fps", self.src.name(), self.fps),
                        self.start_up_summary().unwrap_or_default(),
                        "Tap to close".into(),
                    ]);
                    d.retain(|l| !l.is_empty());
                    self.dialog = Some(d)
                }
                Item::Shot => {
                    // without the dialog on it
                    self.close_menu();
                    self.shot_request = Some(String::new());
                }
                Item::MathAuto => self.math_autoscale(),
                Item::MathOff => self.math_turn_off(self.m.math_sel),
                Item::ZoneBox | Item::ZoneShape => {
                    self.zone_arm = Some((self.zone_sel, it == Item::ZoneShape));
                    self.close_menu();
                    self.toast(if it == Item::ZoneShape { "Draw the zone with one finger" } else { "Drag a box for the zone" });
                }
                Item::ZoneClear => {
                    let blank = crate::zone::Zone { src: self.m.zones[self.zone_sel].src, ..Default::default() };
                    if self.m.zones[self.zone_sel] == blank {
                        self.toast(NOTHING_TO_CLEAR);
                    } else {
                        self.m.zones[self.zone_sel] = blank;
                        self.toast(&format!("Zone {} deleted", self.zone_sel + 1));
                    }
                }
                Item::StoreSaveSetup => {
                    let p = self.setup_path();
                    let r = crate::store::write(&p, self.setup_text_full().as_bytes());
                    self.toast_saved(&p, r);
                }
                Item::StoreRecall => self.recall_setup(),
                Item::StoreCsv => self.store_wave(true, None),
                Item::StorePng => {
                    // without the dialog: the screen is saved after the next frame (main loop / script runner)
                    let p = self.new_file(&self.save_dir(), "shot", "png");
                    self.close_menu();
                    self.shot_request = Some(p.to_string_lossy().into_owned());
                }
                Item::MaskCreate => self.create_mask(),
                Item::MaskFromRef => self.create_mask_from_ref(),
                Item::MaskClear => {
                    if self.mask.is_none() && self.mask_ref.is_none() && !self.m.mask_on && self.mask_pass == 0 && self.mask_fail == 0 {
                        self.toast(NOTHING_TO_CLEAR);
                    } else {
                        self.mask = None;
                        self.mask_ref = None;
                        self.m.mask_on = false;
                        self.mask_counts_reset();
                        self.toast("Mask cleared");
                    }
                }
                Item::MaskReset => {
                    if self.mask_pass == 0 && self.mask_fail == 0 {
                        self.toast(NOTHING_TO_CLEAR);
                    } else {
                        self.mask_counts_reset();
                        self.toast("Counts reset");
                    }
                }
                Item::StoreFfSave => self.ff_save(),
                Item::StoreFfLoad => self.ff_load(),
                Item::Stock => self.stock_tap(),
                Item::Update => self.dialog = Some(self.update_lines()),
                Item::AcqSegTable => self.table_toggle(tables::TableKind::Segments),
                Item::SrcOn => {
                    let v = self.off_sources(self.menu);
                    for &c in &v {
                        self.m.ch[c].on = true;
                    }
                    self.focus = 0;
                    self.settings_changed();
                    let names: Vec<String> = v.iter().map(|c| format!("CH{}", c + 1)).collect();
                    self.toast(&format!("{} on", names.join(", ")));
                }
                Item::WlanInfo => self.dialog = Some(self.wlan_lines()),
                Item::WlanScan => self.wlan_scan(),
                Item::WlanForget => self.wlan_forget_tap(),
                Item::UpdateRollback => self.update_rollback_tap(),
                _ => {}
            },
        }
    }

    /// The band the external display scales when it shows the waveform: between the top bar and the cards.
    /// HDMI workspace: the waveform area as it is now (one canvas: the band between the bars; with layers: the layer
    /// over the traces, the GPU puts it together itself).
    fn capture_hdmi(&mut self, l: &Layers) {
        match l.over.as_deref() {
            None => {
                let cv = &*l.under;
                let body = self.frame().body;
                self.hdmi_snap.clear();
                self.hdmi_snap.extend_from_slice(&cv.px[(body.y * cv.w) as usize..(body.bottom() * cv.w) as usize]);
            }
            Some(o) => {
                let s = self.hdmi_over.get_or_insert_with(|| Canvas::new_layer(o.w, o.h));
                if (s.w, s.h) != (o.w, o.h) {
                    *s = Canvas::new_layer(o.w, o.h);
                }
                s.copy_from(o);
            }
        }
    }

    /// Storage > Save waveform (CSV or binary): the newest record to the save folder, or to a file of the caller
    /// (SCPI `:SAVE:WAVeform "path"`).
    pub(crate) fn store_wave(&mut self, csv: bool, path: Option<std::path::PathBuf>) {
        let Some(rec) = &self.last[0] else { return self.toast("No waveform to save") };
        let p = path.clone().unwrap_or_else(|| self.new_file(&self.save_dir(), "wave", if csv { "csv" } else { "bin" }));
        let mut r = if csv { crate::store::write(&p, crate::store::wave_csv(rec, &self.m).as_bytes()) } else { crate::store::write(&p, &crate::store::wave_bin(rec, &self.m)) };
        // the LA lines go to their own file: another sample rate, and only the changes
        if let (true, Ok(()), Some(la), true, None) = (csv, &r, &rec.la, self.m.la_on(), &path) {
            let q = self.new_file(&self.save_dir(), "la", "csv");
            r = crate::store::write(&q, crate::store::la_csv(la, &self.m.la.lines()).as_bytes());
        }
        self.toast_saved(&p, r);
    }

    fn toast_saved(&mut self, p: &std::path::Path, r: std::io::Result<()>) {
        match r {
            Ok(()) => {
                self.flush_stick();
                self.toast(&format!("Saved {}{}", crate::store::file_name(p), self.saved_where()));
            }
            Err(e) => self.toast(&format!("Save failed: {}", e)),
        }
    }

    /// A USB stick is mounted and the storage directory was not set by hand: files go to it.
    pub fn usb_used(&self) -> bool {
        self.usb && self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR)
    }

    /// Where screenshots, CSV, mask copies and fast-frame files go now.
    pub fn save_dir(&self) -> std::path::PathBuf {
        match self.store_dest {
            crate::store::StoreDest::Stick => std::path::PathBuf::from(crate::store::USB_DIR),
            crate::store::StoreDest::Local => self.store_dir.clone(),
            crate::store::StoreDest::Auto => {
                if self.usb_used() { std::path::PathBuf::from(crate::store::USB_DIR) } else { self.store_dir.clone() }
            }
        }
    }

    /// For the message after saving.
    pub fn saved_where(&self) -> &'static str {
        match self.store_dest {
            crate::store::StoreDest::Local if self.usb => " locally, not on the stick",
            crate::store::StoreDest::Stick => " on the USB stick",
            crate::store::StoreDest::Auto if self.usb_used() => " on the USB stick",
            _ => "",
        }
    }

    /// After a file: on the stick for real, so that it can be pulled. Not in the frame loop, a stick can take its time.
    pub fn flush_stick(&self) {
        if self.usb_used() {
            let _ = std::thread::Builder::new().name("sync".into()).spawn(crate::store::sync_disks);
        }
    }

    /// From `step`: is a stick mounted (`/bin/oos-usb`); says so when that changes.
    fn usb_poll(&mut self) {
        if cfg!(test) || self.now < self.usb_check {
            return;
        }
        self.usb_check = self.now + 2.0;
        self.wlan_stick = wlan::stick_there();
        let on = std::fs::read_to_string("/proc/mounts").is_ok_and(|t| crate::store::usb_mounted(&t));
        if on != self.usb {
            self.usb = on;
            if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) {
                self.toast(if on { "USB stick ready: files are saved to it" } else { "USB stick removed" });
            }
        }
    }

    /// The setup file the Storage menu uses. An empty name is the current slot (`setupN.txt`). A number 1 to 9 selects
    /// that slot. A `.stp` or `.txt` name is that file, beside the setups when the path is relative. `slot` and `suffix`
    /// are the refusals.
    fn setup_file(&mut self, path: &str) -> Result<std::path::PathBuf, String> {
        let raw = path.trim().trim_matches('"');
        if raw.is_empty() {
            return Ok(self.setup_path());
        }
        if let Ok(slot) = raw.parse::<u32>() {
            if !(1..=9).contains(&slot) {
                return Err("slot".into());
            }
            self.store_slot = slot;
            return Ok(self.setup_path());
        }
        let named = std::path::Path::new(raw);
        let ext = named.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "stp" && ext != "txt" {
            return Err("suffix".into());
        }
        Ok(if named.is_absolute() {
            named.to_path_buf()
        } else {
            self.setup_path().parent().map(|d| d.join(named)).unwrap_or_else(|| named.to_path_buf())
        })
    }

    /// The `ref<N>.*` lines of a setup: how each loaded ref slot looks (on/off, scale, offset, colour, name).
    fn ref_setup_text(&self) -> String {
        let looks: Vec<_> = self.refs.iter().map(|r| r.as_ref().map(|r| crate::store::RefLook {
            on: Some(r.on), vdiv: Some(r.vpd), offset: Some(r.offset), color: Some(r.color), label: Some(r.label.clone()),
        })).collect();
        crate::store::ref_setup_text(&looks)
    }

    /// Applies the saved look to the ref slots that hold a waveform now (the samples are not in a setup).
    fn apply_ref_looks(&mut self, looks: &[Option<crate::store::RefLook>]) {
        for (r, l) in self.refs.iter_mut().zip(looks) {
            let (Some(r), Some(l)) = (r.as_mut(), l) else { continue };
            if let Some(v) = l.on { r.on = v; }
            if let Some(v) = l.vdiv { r.vpd = v; }
            if let Some(v) = l.offset { r.offset = v; }
            if let Some(v) = l.color { r.color = v.min(REF_COLORS.len() - 1); }
            if let Some(v) = &l.label { r.label = v.clone(); }
        }
    }

    /// Loads setup<slot>.txt of the storage directory; the acquisition state (run / stop) stays as it is.
    fn recall_setup(&mut self) {
        let p = self.setup_path();
        let text = std::fs::read_to_string(&p).map_err(|e| e.to_string());
        // a set-up is a live one, as with the Default key: a reference or SIM session ends first, so a Live afterwards
        // does not bring back the set-up from before it over the recalled one
        if text.as_deref().is_ok_and(|t| crate::store::parse_setup(t).is_ok()) && self.record_mode() {
            self.leave_session();
        }
        match text.and_then(|t| self.apply_setup_text(&t)) {
            Ok(()) => self.toast(&format!("Recalled {}", crate::store::file_name(&p))),
            Err(e) => self.toast(&format!("Recall failed: {}", e)),
        }
    }

    /// The text of the whole setup (the keys of the setup file and the look of the ref slots): what Save setup writes and
    /// `:SYSTem:SETup?` returns.
    pub(crate) fn setup_text_full(&mut self) -> String {
        self.sync_view_state();
        format!("{}{}", crate::store::setup_text(&self.m), self.ref_setup_text())
    }

    /// Applies a setup text to the live settings, the way Recall setup does; the acquisition state stays as it is.
    pub(crate) fn apply_setup_text(&mut self, text: &str) -> Result<(), String> {
        let parsed = crate::store::parse_setup(text);
        match parsed {
            Ok(mut m) => {
                self.apply_ref_looks(&crate::store::parse_ref_looks(text, NREF));
                (m.run, m.status) = (self.m.run, self.m.status);
                // the text size is a preference of this screen, not part of a set-up
                m.text_size = self.m.text_size;
                if m.mask_src != self.m.mask_src {
                    self.mask = None;
                    self.mask_ref = None;
                }
                self.m = m;
                self.restore_view_state();
                self.view_state_restore(text);
                if self.m.layout == Layout::Studio { self.studio.enter = true; }
                self.math_state.borrow_mut().forget_fit();
                self.m.order_trigger_levels();
                self.bus_auto = None;
                self.math_fam = if self.m.math == MathOp::Off { MathFamily::Arith } else { self.m.math.family() };
                self.sync_full();
                if self.mask.is_none() {
                    self.m.mask_on = false;
                }
                self.build_mask();
                self.clamp_zoom();
                self.relayout();
                self.dirty_map = true;
                self.clear_measurement_statistics();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// File of the current setup slot: a setup is small and has to survive a power cycle, so the default storage directory
    /// in RAM is not used for it when OOS's data partition is there.
    fn setup_path(&self) -> std::path::PathBuf {
        let keep = std::path::Path::new("/oos/setups");
        let dir = if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) && keep.is_dir() { keep } else { self.store_dir.as_path() };
        dir.join(format!("setup{}.txt", self.store_slot))
    }

    fn level_50(&mut self) {
        if self.m.trig_ref.is_some() { self.fit_reference_trigger_levels(); self.settings_changed(); return; }
        let s = self.stats[self.m.trig_src];
        if s.valid {
            self.m.trig_level = crate::wave::mid_level(&s);
            self.settings_changed();
        }
    }

    fn open_menu(&mut self, id: MenuId) {
        self.zoom_arm = false;
        self.zoom_path.clear();
        self.zoom_map = None;
        self.menu = id;
        self.menu_open = true;
        self.parent = None;
        self.menu_history.clear();
        self.pick = None;
        self.focus = 0;
    }

    fn open_submenu(&mut self, id: MenuId) {
        let mut history = std::mem::take(&mut self.menu_history);
        history.push((self.menu, self.focus));
        self.open_menu(id);
        self.parent = history.last().map(|entry| entry.0);
        self.menu_history = history;
    }

    /// How long a finger stays down on Measure before the short menu opens. A shorter press is a tap.
    pub(super) const MEAS_HOLD_S: f64 = LONG_PRESS;

    /// Usual measurements, in the order the Add meas shortcut offers them. Anything left follows `ALL_MEAS`.
    const USUAL_MEAS: [Meas; 6] = [Meas::Vpp, Meas::Freq, Meas::Period, Meas::Vrms, Meas::Vavg, Meas::Rise];

    /// The next usual measurement that is not already on the measure source.
    fn next_usual_meas(&self) -> Option<Meas> {
        let src = self.meas_src;
        let on = |k: Meas| self.m.meas.iter().any(|e| e.0 == k && e.1 == src);
        Self::USUAL_MEAS.into_iter().chain(ALL_MEAS).find(|k| !on(*k))
    }

    /// Rectangle of the Measure button on the top bar.
    fn measure_btn(&self) -> Option<Rect> {
        self.top_buttons().into_iter().find(|b| b.0 == TopBtn::Menu(MenuId::Measure)).map(|b| b.2)
    }

    fn on_measure_btn(&self, x: i32, y: i32) -> bool {
        self.measure_btn().is_some_and(|r| self.top_hit(r).contains(x, y))
    }

    /// A tap on Measure opens the measurement list. Press again to close it.
    fn open_meas_list(&mut self) {
        self.ensure_meas_own();
        if self.menu_open && self.menu == MenuId::MeasAdd {
            self.close_menu();
        } else {
            self.open_menu(MenuId::MeasAdd);
            if self.m.layout == Layout::Studio { self.sheet_add_open(); }
        }
    }

    /// A finger held on Measure opens the short menu. The lift is handled by `meas_hold_up`, not as a tap.
    fn meas_hold_tick(&mut self) {
        let t = &self.touch;
        if !t.down || t.held || self.m.touch_lock || self.full_on() || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() {
            return;
        }
        if t.drag != Drag::None && t.drag != Drag::Pending {
            return;
        }
        if (t.x - t.x0).abs() > 12 || (t.y - t.y0).abs() > 12 || self.now - t.t0 < Self::MEAS_HOLD_S {
            return;
        }
        if !self.on_measure_btn(t.x0, t.y0) {
            return;
        }
        self.touch.held = true;
        self.open_menu(MenuId::MeasHold);
    }

    /// Lift after the short menu opened. On the Measure button, or on Add meas, that shortcut runs.
    /// A lift anywhere else leaves the menu so the shortcut can still be tapped.
    fn meas_hold_up(&mut self, x: i32, y: i32) {
        if !(self.menu_open && self.menu == MenuId::MeasHold) {
            return;
        }
        if self.tap_menu(x, y) {
            return;
        }
        if self.on_measure_btn(x, y) {
            self.activate(Item::AddMeas);
            self.close_menu();
        }
    }

    fn toggle_menu(&mut self, id: MenuId) {
        if self.menu_open && self.menu_root() == id {
            self.close_menu();
        } else {
            self.open_menu(id);
        }
    }

    /// The options of a choice item for segmented buttons (empty: shown as - value +).
    fn options(&self, it: Item) -> Vec<String> {
        if let Some(o) = self.addrail_options(it).or_else(|| self.meas_options(it)).or_else(|| self.measrow_options(it)).or_else(|| self.ana_options(it)).or_else(|| self.magnetic_options(it)).or_else(|| self.inverter_options(it)).or_else(|| self.dpt_options(it)).or_else(|| self.cal_options(it)).or_else(|| self.bode_options(it)).or_else(|| self.sim_options(it)).or_else(|| self.offline_options(it)).or_else(|| self.afg_options(it)).or_else(|| self.bw_options(it)).or_else(|| self.la_options(it)).or_else(|| self.win_options(it)).or_else(|| self.bus_options(it)).or_else(|| self.time_options(it)).or_else(|| self.remote_options(it)).or_else(|| self.graphics_options(it)).or_else(|| self.views_options(it)).or_else(|| self.monitor_options(it)).or_else(|| self.features_options(it)) {
            return o;
        }
        let v: &[&str] = match it {
            Item::StripDock => return StripDock::ALL.iter().map(|s| s.label().to_string()).collect(),
            Item::ChOn(_) | Item::ChInvert(_) | Item::MeasStats | Item::MeasLabels | Item::Zoom | Item::AutoRoll | Item::DecOn | Item::SearchOn | Item::ColorGrade | Item::MaskOn | Item::MaskInvert => &["Off", "On"],
            Item::MaskSrc => return ch_names(),
            Item::MaskAction => &["None", "Stop"],
            Item::ChCoupling(_) => return board().couplings().iter().map(|c| c.name().to_string()).collect(),

            Item::ChProbe(_) => &["1x", "10x", "100x"],
            Item::TrigSrc | Item::TrigSrc2 => return self.trigger_sources().into_iter().map(Self::trigger_wave_label).collect(),
            Item::DecSrc => return self.decoder_sources().into_iter().map(decode_source_name).collect(),
            Item::RefSrc => return ch_names(),
            Item::MeasSrc | Item::MeasSrcB | Item::CurSrcB | Item::CatSourceC | Item::CatSourceD => return self.wave_names(),
            Item::MeasCopyTo => return self.wave_names().into_iter().filter(|n| *n != self.wave_name(self.meas_src)).collect(),
            Item::MathA | Item::MathB => return self.math_source_ids(self.m.math_sel).into_iter().map(src_name).collect(),
            Item::MathSel => return (0..NMATH).map(|k| format!("M{}", k + 1)).collect(),
            Item::TrigSlope => &["Rising", "Falling", "Either"],
            Item::TrigMode => &["Auto", "Normal", "Single"],
            Item::TrigCoup => return TRIG_COUPS.iter().map(|c| c.name().to_string()).collect(),
            Item::TrigSlope2 => &["Rising", "Falling", "Either"],
            Item::TrigPol => &["Positive", "Negative"],
            Item::TrigWhen => {
                if self.m.trig_type == TrigType::Runt {
                    &["Off", ">", "<", "Inside", "Outside"]
                } else {
                    &[">", "<", "Inside", "Outside"]
                }
            }
            Item::VidStd => &["PAL", "NTSC"],
            Item::VidSync => &["Lines", "Line n", "Odd", "Even", "Fields"],
            Item::Pat(_) => {
                if self.m.trig_type == TrigType::Duration {
                    &["H", "L", "X"]
                } else {
                    &["H", "L", "X", "Rise", "Fall"]
                }
            }
            Item::WinModeI => &["Enter", "Exit", "Inside"],
            Item::DecProto => return self.protos_on().iter().map(|p| p.name().to_string()).collect(),
            Item::DecUsbSpeed => &USPEED_NAMES,
            Item::DecOpt(k) => return self.m.dec_proto.module().and_then(|p| p.options().get(k)).map_or(Vec::new(), |o| o.choices.iter().map(|c| c.to_string()).collect()),
            Item::DecSrc4 | Item::DecDiff => return std::iter::once("None".into()).chain(self.decoder_sources().into_iter().map(decode_source_name)).collect(),
            Item::DecCanH => &["RX/TX", "CAN_H"],
            Item::TrigBusWhat => &["Frame", "ID/Addr", "Data", "Error"],
            Item::DecSrc2 => return self.decoder_sources().into_iter().map(decode_source_name).collect(),
            Item::DecCs => return (if self.m.dec_proto == Proto::CPHY { Vec::new() } else { vec!["None".into()] }).into_iter()
                .chain(self.decoder_sources().into_iter().map(decode_source_name)).collect(),
            Item::DecMode => &["0", "1", "2", "3"],
            Item::DecParity => &["None", "Even", "Odd"],
            Item::AcqMode => &["Normal", "Peak", "Average", "High res"],
            Item::CurMode => &["Off", "Time", "Volt", "Track", "XY"],
            Item::GridStyle => &["Full", "Light", "None"],
            Item::GridFrame | Item::TrigNoise | Item::TrigAutoNoise => &["Off", "On"],
            Item::Dots | Item::ZoneOn => &["Off", "On"],
            Item::Layout => &["Default", "Minimal", "Studio"],
            Item::BottomBar => &["Cards", "Compact"],
            Item::AcqCores => &["Raw", "Matched"],
            Item::AcqDrawPath => &["Standard", "Batch"],
            Item::Hdmi => &["Mirror", "Waveform", "Layout"],
            Item::TextSize => return TEXT_SIZES.iter().map(|t| t.name().to_string()).collect(),
            Item::ZoneSel => &["Z1", "Z2"],
            Item::ZoneSrc => return ch_names(),
            Item::ZoneMode => &["Intersect", "Not intersect"],
            Item::ZInput => return opt_ch_names("Off"),
            Item::Histogram => &["Off", "Vertical", "Horizontal"],
            Item::AuxOut => return AuxOut::choices(board().mask_hw() || board().zone_hw()).iter().map(|a| a.name().to_string()).collect(),
            Item::DecFmt => &["Hex", "ASCII"],
            Item::XyMode => &["YT", "XY"],
            Item::DecSel => &["D1", "D2", "D3", "D4"],
            Item::PaneSel => &tables::PANES,
            Item::TrendSel(_) => return self.trend_options(),
            Item::DvmSrc | Item::CntSrc => return (0..n_ch()).map(|c| format!("CH{}", c + 1)).collect(),
            Item::DvmOn | Item::CntOn | Item::ChFine | Item::HFine | Item::LogOn | Item::AcqSegHw | Item::AcqAvgHw => &["Off", "On"],
            Item::ChUnit(_) => &["V", "A", "W", "U"],
            Item::ChExpand => &["Center", "Ground"],
            Item::HExpandI => &["Center", "Left", "Right", "Trigger"],
            Item::DvmMode => &tools::DVM_MODES,
            Item::CntMode => &tools::CNT_MODES,
            Item::BootMode => &["One-shot", "Persist"],
            Item::FftWin => &["Rect", "Hann", "Hamming", "Blackman", "Flattop"],
            // long lists: shown as a picker (see `ctl`)
            Item::MathOpI => return self.math_ops_in_family().iter().map(|o| o.name().to_string()).collect(),
            Item::MathFam => return MathFamily::ALL.iter().map(|f| f.name().to_string()).collect(),
            Item::StoreDest => return crate::store::StoreDest::ALL.iter().map(|d| d.name().to_string()).collect(),
            Item::TrigType => return TRIG_TYPES.iter().map(|t| t.name().to_string()).collect(),
            Item::AcqDepth => return mdepths().iter().map(|d| d.0.to_string()).collect(),
            Item::ChBw(_) => return BWS.iter().map(|b| b.0.to_string()).collect(),
            Item::Persist => return PERSIST.iter().map(|p| p.0.to_string()).collect(),
            Item::AcqAvg => return AVERAGES.iter().map(|n| n.to_string()).collect(),
            Item::AcqSeg => return self.seg_options().iter().map(|&n| if n == 0 { "Off".into() } else { n.to_string() }).collect(),
            Item::AcqSegIvl => return SEG_IVLS.iter().map(|&x| seg_ivl_name(x)).collect(),
            Item::DecBaud => return crate::decode::BAUDS.iter().map(|b| b.to_string()).collect(),
            Item::DecCphyRate => return crate::decode::CPHY_RATES.into_iter().map(crate::decode::cphy_rate_label).collect(),
            Item::DecCanRate => return crate::decode::CAN_RATES.iter().map(|&b| si(b, "bit/s")).collect(),
            // the value picker is a hex keypad
            Item::TrigBusVal | Item::SerMax | Item::SerMask => return KEYPAD.iter().map(|k| k.to_string()).collect(),
            Item::SerRel | Item::UartPol | Item::UartStop | Item::UartErr | Item::SpiCs | Item::I2cDir | Item::I2cAddrBits | Item::I2cBytes => return Self::serial_options(it),
            Item::AfgFreqEntry => return FREQ_KEYS.iter().map(|k| k.to_string()).collect(),
            Item::WlanNet => return self.wlan_net_options(),
            Item::WlanForgetNet => return self.wlan_forget_options(),
            Item::RefImport => return self.ref_files.iter().map(|p| crate::store::file_name(p)).collect(),
            Item::QuickAct => return QUICKS.iter().map(|q| q.name().to_string()).collect(),
            // Studio: the ten slots are one list (ui/sheet.rs)
            Item::RefSel if self.sheets_on() => return (1..=NREF).map(|i| format!("R{i}")).collect(),
            Item::Overclock => return board().clock_steps().iter().map(|&hz| overclock::label(hz)).collect(),
            _ => &[],
        };
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Control of a menu row whose control rectangle is `w` px wide: segmented buttons when all options fit, a
    /// picker (- value +, the value opens the list of all options) for longer option lists, else - value +.
    fn ctl(&self, it: Item, w: i32) -> Ctl {
        // - and + step, the value opens the keypad (`open_picker`); the name's and the address list's value open their
        // list even when short
        if matches!(it, Item::AfgFreq | Item::RemName | Item::RemAllow) {
            return Ctl::Pick;
        }
        if let Item::RefShow(_) = it {
            return Ctl::Button;
        }
        // a finding is one wide button; its text takes a smaller font instead of being cut
        if let Item::IdFind(_) = it {
            let cur = self.value(it);
            return Ctl::Seg(vec![cur.clone()], Some(0));
        }
        if matches!(it, Item::MathExprI | Item::LogInfo | Item::CalStatus | Item::NetEth | Item::NetMac | Item::NetPorts | Item::NetWlan | Item::AcqAlias) || Self::time_text(it) || Self::remote_text(it) {
            return Ctl::Text;
        }
        match Self::kind(it) {
            Kind::Action => Ctl::Button,
            k => {
                let opts = self.options(it);
                let n = opts.len() as i32;
                let f = self.fonts();
                if n == 0 {
                    Ctl::Value
                } else if k == Kind::Choice && opts.iter().all(|o| f.mb.width(o) + 12 <= (w - (n - 1) * 4) / n) {
                    let cur = self.value(it);
                    let sel = opts.iter().position(|o| *o == cur);
                    Ctl::Seg(opts, sel)
                } else {
                    Ctl::Pick
                }
            }
        }
    }

    /// Opens the picker of a choice item of the current menu; picking an option returns to this menu.
    fn open_picker(&mut self, it: Item) {
        if self.item_blocked_toast(it) { return; }
        let it = if it == Item::AfgFreq { Item::AfgFreqEntry } else { it };
        if Self::hex_item(it) {
            self.keypad = format!("{:X}", self.hex_value(it));
        }
        if it == Item::AfgFreqEntry {
            self.keypad.clear();
        }
        let opts = self.options(it);
        let cur = self.value(it);
        self.pick = Some(Pick { it, menu: self.menu, parent: self.parent, focus: self.focus });
        self.parent = Some(self.menu);
        self.menu = MenuId::Pick;
        self.focus = opts.iter().position(|o| *o == cur).unwrap_or(0);
    }

    /// Leaves the picker for the menu it was opened from (after a pick, or with Back).
    fn close_picker(&mut self) {
        if let Some(p) = self.pick.take() {
            (self.menu, self.parent, self.focus) = (p.menu, p.parent, p.focus);
        }
    }

    /// Sets a choice item to the option `want`: one adjust step by the distance in the option list (every item with
    /// options moves by index, cycling or clamped), else stepping until it shows `want`.
    fn select_option(&mut self, it: Item, want: &str) {
        if self.item_blocked_toast(it) { return; }
        if it == Item::DecCphyRate {
            if let Some(rate) = crate::decode::CPHY_RATES.into_iter().find(|&rate| crate::decode::cphy_rate_label(rate) == want) {
                self.m.dec_cphy_rate = rate;
                self.settings_changed();
            }
            return;
        }
        if it == Item::Views {
            // a count is chosen: the new views exist, the dialog gets out of the way of the plot, the toast says which
            let n = if want == "Off" { 1 } else { want.parse().unwrap_or(self.m.views) };
            self.views_set(n);
            self.close_menu();
            return;
        }
        if it == Item::MeasCopyTo {
            if let Some(to) = self.wave_ids().into_iter().find(|&w| self.wave_name(w) == want) {
                let from = self.meas_src;
                let n = self.copy_measurements(from, to);
                self.meas_copied = Some((from, to, n));
                self.toast(&format!("Copied {} measurement{} to {}", n, if n == 1 { "" } else { "s" }, want));
            }
            return;
        }
        if it == Item::EyePlace {
            // a placement choice always places the eye, also the one it already has while it floats
            if let Some(place) = [(EyePlace::Full, "Full"), (EyePlace::Side, "Side"), (EyePlace::Under, "Under")].into_iter().find(|(_, l)| *l == want).map(|(p, _)| p) {
                self.workspace.eye_float = None;
                self.eye_set_place(place);
                self.relayout();
            }
            return;
        }
        if it == Item::StripDock {
            // every dock choice docks the card, also the one already selected after it was floated
            if let Some(dock) = StripDock::ALL.into_iter().find(|d| d.label() == want) {
                self.m.strip_dock = dock;
                self.workspace.measure_docked = true; self.workspace.measure_float = None;
                self.relayout();
            }
            return;
        }
        if self.pick_source_direct(it, want) { return; }
        let opts = self.options(it);
        let cur = self.value(it);
        if let (Some(k), Some(i)) = (opts.iter().position(|o| o == want), opts.iter().position(|o| *o == cur)) {
            if k != i {
                self.adjust(it, k as i32 - i as i32);
            }
            return;
        }
        for _ in 0..8 {
            if self.value(it) == want {
                return;
            }
            self.adjust(it, 1);
        }
    }

    /// x centre of what opened the menu, and whether that is in the top bar (the dialog hangs below it) or a card
    /// (the dialog sits above it).
    fn menu_anchor(&self) -> (i32, bool) {
        let (chs, tr, trg) = self.cards();
        match self.menu_root() {
            MenuId::Channel(c) | MenuId::ChMore(c) => (chs[c].x + chs[c].w / 2, false),
            MenuId::Horizontal => (tr.x + tr.w / 2, false),
            MenuId::Trigger => (trg.x + trg.w / 2, false),
            MenuId::Main => (self.menu_btn().x, false),
            MenuId::Network | MenuId::Remote | MenuId::Ssh => (self.net_box().x + self.net_box().w / 2, true),
            MenuId::ViewMore => (self.scr.w, true),
            MenuId::La | MenuId::LaLines | MenuId::LaPat => self.aux_cards().0.map_or((self.scr.w / 2, false), |r| (r.x + r.w / 2, false)),
            MenuId::Afg => self.aux_cards().1.map_or((self.scr.w / 2, false), |r| (r.x + r.w / 2, false)),
            // menus of front-panel keys (also opened from the start menu): centred above the cards
            MenuId::Utility => self.top_buttons().iter().find(|t| t.0 == TopBtn::System).map_or((self.scr.w / 2, false), |t| (t.2.x + t.2.w / 2, true)),
            m => self.top_buttons().iter().find(|t| t.0 == TopBtn::Menu(m)).map_or((self.scr.w / 2, false), |t| (t.2.x + t.2.w / 2, true)),
        }
    }

    fn dlg_layout(&self) -> Option<Dlg> {
        if !self.menu_open {
            return None;
        }
        if self.menu == MenuId::MeasAdd { return Some(self.catalog_dialog()); }
        if matches!(self.menu, MenuId::MeasSelected | MenuId::MeasLegacy) { return Some(self.measurement_chooser_layout()); }
        if self.menu == MenuId::Main {
            return Some(self.main_layout());
        }
        let items = self.items(self.menu);
        let n = items.len() as i32;
        let grid = items.iter().all(|&it| Self::kind(it) == Kind::Action);
        let wide = self.pick.is_some_and(|p| matches!(p.it, Item::WlanNet | Item::WlanForgetNet | Item::SshRemoveKey));
        let l = self.lay();
        let (cols, cw, rh, gap) = if self.menu == MenuId::MeasHold { (1, 220 + l.grid_col - 150, DLG_ROW, 16) } else if wide { (2, l.dlg_col, DLG_ROW + 2, 8) } else if self.menu == MenuId::ViewMore { (1, l.grid_col + 40, DLG_ROW + 2, 8) } else if grid && self.menu == MenuId::MeasAdd {
            // the whole catalogue in one grid: narrower columns where the larger text sizes would run off the screen
            if n > 54 { (7, ((self.scr.w - 72) / 7).min(l.grid_col - 20), DLG_ROW + 2, 8) } else { (6, ((self.scr.w - 64) / 6).min(l.grid_col), DLG_ROW + 2, 8) }
        } else if grid { (4, l.grid_col, DLG_ROW + 2, 8) } else if n > 5 { (2, l.dlg_col, DLG_ROW, 16) } else { (1, l.dlg_col, DLG_ROW, 16) };
        let fr = self.frame();
        // the dialog stays between the top and the bottom bar: rows are pressed together (never below the control
        // height), then, when that is not enough, the list gets another column while the screen has room for it
        let avail = fr.bottom.y - 4 - (fr.top.bottom() + 4);
        let (mut cols, mut rh) = (cols, rh);
        let mut nrows = (n + cols - 1) / cols;
        while nrows > 0 {
            let fit = (avail - DLG_HEAD - 6) / nrows;
            if DLG_HEAD + nrows * rh + 6 <= avail { break; }
            if fit > DLG_CTRL { rh = fit; break; }
            if grid || 24 + (cols + 1) * cw + cols * gap > self.scr.w - 2 * METRICS.edge || cols >= n { rh = fit.max(DLG_CTRL); break; }
            cols += 1;
            nrows = (n + cols - 1) / cols;
        }
        let w = 24 + cols * cw + (cols - 1) * gap;
        let h = DLG_HEAD + nrows * rh + 6;
        let (ax, top) = self.menu_anchor();
        let x = (ax - w / 2).clamp(METRICS.edge, self.scr.w - METRICS.edge - w);
        let y = if top { fr.top.bottom() + 4 } else { (fr.bottom.y - 4 - h).max(fr.top.bottom() + 4) };
        let r = Rect::new(x, y, w, h);
        let close = Rect::new(r.x + r.w - 52, r.y + 6, 44, DLG_CTRL);
        let back = self.parent.map(|_| Rect::new(close.x - 88, close.y, 82, DLG_CTRL));
        let mut rows = Vec::new();
        for (i, &it) in items.iter().enumerate() {
            let i = i as i32;
            if grid {
                let (col, row) = (i % cols, i / cols);
                let ctrl = Rect::new(r.x + 12 + col * (cw + gap), r.y + DLG_HEAD + row * rh, cw, DLG_CTRL);
                rows.push(Row { it, label: Rect::new(ctrl.x, ctrl.y, 0, 0), ctrl, ctl: Ctl::Button });
                continue;
            }
            let (col, row) = (i / nrows, i % nrows);
            let cx = r.x + 12 + col * (cw + gap);
            let ry = r.y + DLG_HEAD + row * rh;
            let label = Rect::new(cx, ry, l.dlg_label, DLG_CTRL);
            let lw = l.dlg_label + 4;
            let ctrl = if Self::kind(it) == Kind::Action { Rect::new(cx, ry, cw, DLG_CTRL) } else { Rect::new(cx + lw, ry, cw - lw, DLG_CTRL) };
            rows.push(Row { it, label, ctl: self.ctl(it, ctrl.w), ctrl });
        }
        Some(Dlg { r, close, back, rows, heads: Vec::new() })
    }

    /// Start menu: one row of up to 6 buttons per group, the group label left of it; above the Menu button.
    fn main_layout(&self) -> Dlg {
        let (gap, groups) = (6, self.main_groups());
        // the label column follows the widest group name (larger text); the buttons give up width for it
        let lw = groups.iter().map(|g| self.fonts().sb.width(g.0) + 20).max().unwrap_or(0).max(104);
        let cw = 140.min((self.scr.w - 2 * METRICS.edge - 24 - lw - 5 * gap) / 6);
        // six buttons per line; a longer group continues on the next line (label on its first line only)
        const PER_LINE: usize = 6;
        let lines: usize = groups.iter().map(|g| g.1.len().div_ceil(PER_LINE).max(1)).sum();
        let w = 24 + lw + PER_LINE as i32 * cw + (PER_LINE as i32 - 1) * gap;
        let h = DLG_HEAD + lines as i32 * (DLG_ROW + 2) + 6;
        let r = Rect::new(METRICS.edge, self.frame().bottom.y - 4 - h, w, h);
        let close = Rect::new(r.x + r.w - 52, r.y + 6, 44, DLG_CTRL);
        let (mut rows, mut heads) = (Vec::new(), Vec::new());
        let mut line = 0;
        for (name, items) in groups {
            let y0 = r.y + DLG_HEAD + line as i32 * (DLG_ROW + 2);
            heads.push((Rect::new(r.x + 14, y0, lw, DLG_CTRL), name));
            let n = items.len().div_ceil(PER_LINE).max(1);
            for (i, it) in items.into_iter().enumerate() {
                let y = y0 + (i / PER_LINE) as i32 * (DLG_ROW + 2);
                let ctrl = Rect::new(r.x + 12 + lw + (i % PER_LINE) as i32 * (cw + gap), y, cw, DLG_CTRL);
                rows.push(Row { it, label: Rect::new(ctrl.x, ctrl.y, 0, 0), ctrl, ctl: Ctl::Button });
            }
            line += n;
        }
        Dlg { r, close, back: None, rows, heads }
    }

    /// Font and box of the text of a menu button, and whether it fits: centred in the button while it fits, else
    /// centred left of the chevron of a button that opens a menu, else one size smaller (long labels at the large text
    /// sizes).
    fn btn_text(&self, c: Rect, open: bool, t: &str) -> (&'static Font, Rect, bool) {
        let f = self.fonts();
        if f.mb.width(t) + if open { 40 } else { 8 } <= c.w {
            return (&f.mb, c, true);
        }
        let r = if open { Rect::new(c.x, c.y, c.w - 18, c.h) } else { c };
        let font = if f.mb.width(t) + 8 <= r.w { &f.mb } else { &f.sb };
        (font, r, font.width(t) + 8 <= r.w)
    }

    /// Value control parts: minus, value, plus.
    fn value_parts(c: Rect) -> (Rect, Rect, Rect) {
        (Rect::new(c.x, c.y, 44, c.h), Rect::new(c.x + 48, c.y, c.w - 96, c.h), Rect::new(c.x + c.w - 44, c.y, 44, c.h))
    }

    fn draw_menu(&self, cv: &mut Canvas) {
        if !self.menu_open { return; }
        if self.sheet_active() {
            self.draw_sheet(cv);
            return;
        }
        if self.menu == MenuId::MeasAdd {
            self.draw_catalog(cv);
            return;
        }
        let Some(d) = self.dlg_layout() else { return };
        let f = self.fonts();
        cv.framed(d.r, 8, 2, ACCENT, 7, DLG_BG);
        let tcol = match self.menu_root() {
            MenuId::Channel(c) => CH_COLORS[c],
            _ => TEXT,
        };
        // long titles (storage directory) keep their end
        let mut title = self.menu_title(self.menu);
        let room = d.back.unwrap_or(d.close).x - d.r.x - 30;
        if self.menu == MenuId::Ref {
            while f.mb.width(&title) > room && title.chars().count() > 2 {
                let keep = title.chars().count() - 2;
                title = format!("{}…", title.chars().take(keep).collect::<String>());
            }
        } else {
            while f.mb.width(&title) > room && title.chars().count() > 2 {
                let mut c = title.chars();
                c.next();
                c.next();
                title = format!("\u{2026}{}", c.as_str());
            }
        }
        if self.menu == MenuId::MeasPair { self.draw_pair_header(cv, &d); }
        else { cv.text(&f.mb, d.r.x + 14, d.r.y + 31, &title, tcol); }
        btn(cv, d.close, BTN, BTN_BORDER);
        cv.text_in(&f.mb, d.close, 1, "\u{d7}", TEXT);
        if let Some(b) = d.back {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.sb, b, 1, "Back", TEXT);
        }
        self.draw_measurement_groups(cv);
        for (r, name) in &d.heads {
            cv.text_fit(&f.sb, Rect::new(r.x, r.y, r.w - 6, r.h), 0, name, DIM);
        }
        for (i, row) in d.rows.iter().enumerate() {
            let focus = i == self.focus;
            let kind = Self::kind(row.it);
            if row.label.w > 0 && kind != Kind::Action {
                cv.text_fit(&f.s, row.label, 0, self.item_label(row.it), if focus { ACCENT } else { LABEL });
            }
            let c = row.ctrl;
            match &row.ctl {
                Ctl::Seg(opts, sel) => {
                    let n = opts.len() as i32;
                    let sw = (c.w - (n - 1) * 4) / n;
                    for (k, o) in opts.iter().enumerate() {
                        let r = Rect::new(c.x + k as i32 * (sw + 4), c.y, sw, c.h);
                        if *sel == Some(k) {
                            cv.round(r, 5, ACCENT, 255);
                            fitted_text(cv, &[&f.mb, &f.m, &f.s], r, o, DARK_TEXT);
                        } else {
                            btn(cv, r, BTN, BTN_BORDER);
                            fitted_text(cv, &[&f.m, &f.s], r, o, TEXT);
                        }
                    }
                }
                Ctl::Text => {
                    let v = self.value(row.it);
                    cv.text_in(self.text_row_font(c.w, &v), c, 1, &v, TEXT)
                }
                Ctl::Value | Ctl::Pick => {
                    let (mi, mid, pl) = Self::value_parts(c);
                    btn(cv, mi, BTN, BTN_BORDER);
                    cv.text_in(&f.mb, mi, 1, "-", TEXT);
                    btn(cv, mid, BTN, if focus { ACCENT } else { BTN_BORDER });
                    cv.text_in(&f.mb, mid, 1, &self.value(row.it), TEXT);
                    if let Ctl::Pick = row.ctl {
                        // the value opens the list
                        chevron_down(cv, mid.x + mid.w - 20, mid.y + mid.h / 2 - 3, DIM);
                    }
                    btn(cv, pl, BTN, BTN_BORDER);
                    cv.text_in(&f.mb, pl, 1, "+", TEXT);
                }
                Ctl::Button => {
                    let picked = match (row.it, self.pick) {
                        (Item::PickOpt(k), Some(p)) => self.options(p.it).get(k).map(|o| (o.clone(), *o == self.value(p.it))),
                        _ => None,
                    };
                    // orange: added measurement, current option of a picker, active state of a start menu entry
                    let on = match row.it {
                        Item::MeasAdd(k) => self.m.meas.contains(&(k, self.meas_src)),
                        Item::MeasPair(k) => self.m.meas2.contains(&(k, self.meas_src, self.meas_src_b)),
                        Item::NavHist => self.hist_pos.is_some(),
                        Item::SingleAct => self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run,
                        Item::RunStop => self.m.run == RunState::Run,
                        _ => false,
                    };
                    let added = on || picked.as_ref().map_or(false, |p| p.1);
                    let txt = match picked {
                        Some((o, _)) => o,
                        None if row.it == Item::QuickRun => format!("Quick \u{b7} {}", self.m.quick.short()),
                        None if matches!(row.it, Item::MeasRemove(_)) => if let Item::MeasRemove(index) = row.it { format!("Remove {}", self.selected_measurement_label(index as usize)) } else { unreachable!() },
                        None if kind == Kind::Action => self.item_label(row.it).to_string(),
                        None => self.value(row.it),
                    };
                    let (tf, tr, fits) = self.btn_text(c, matches!(row.it, Item::Open(_)), &txt);
                    let ink = if added { DARK_TEXT } else { TEXT };
                    if added {
                        cv.round(c, 5, ACCENT, 255);
                    } else {
                        btn(cv, c, BTN, if focus { ACCENT } else { BTN_BORDER });
                    }
                    // a label too long for its button goes on two lines (the last resort is "..."), never over the edge
                    if fits && std::ptr::eq(tf, &f.mb) { cv.text_in(tf, tr, 1, &txt, ink); } else { fitted_text(cv, &[&f.mb], tr, &txt, ink); }
                    if let Item::Open(_) = row.it {
                        chevron(cv, c.x + c.w - 14, c.y + c.h / 2, 1, DIM);
                    }
                }
            }
            if self.item_blocked(row.it).is_some() {
                Self::draw_blocked_veil(cv, if kind == Kind::Action { Rect::new(0, 0, 0, 0) } else { row.label }, c);
            }
            if focus {
                if let Ctl::Seg(..) = row.ctl {
                    cv.round_outline(Rect::new(c.x - 3, c.y - 3, c.w + 6, c.h + 6), 7, ACCENT);
                }
            }
        }
    }

    /// Back in a sub menu: the menu it was opened from, with the focus it had.
    fn menu_back(&mut self) {
        if let Some((menu, focus)) = self.menu_history.pop() {
            self.menu = menu;
            self.focus = focus;
            self.parent = self.menu_history.last().map(|entry| entry.0);
        } else {
            self.menu = self.parent.take().unwrap_or(MenuId::Main);
            self.focus = 0;
        }
    }

    /// Tap inside the menu dialog; false when (x, y) is outside it.
    fn tap_menu(&mut self, x: i32, y: i32) -> bool {
        if self.sheet_active() {
            return self.sheet_tap(x, y);
        }
        if self.menu_open && self.menu == MenuId::MeasAdd {
            return self.catalog_tap(x, y);
        }
        if self.measurement_group_tap(x, y) { return true; }
        let Some(d) = self.dlg_layout() else { return false };
        if !d.r.contains(x, y) {
            return false;
        }
        if d.close.contains(x, y) {
            self.close_menu();
            return true;
        }
        if let Some(b) = d.back {
            if b.contains(x, y) {
                if self.menu == MenuId::Pick {
                    self.close_picker();
                } else {
                    self.menu_back();
                }
                return true;
            }
        }
        if self.menu == MenuId::MeasPair {
            for (item, rect) in self.pair_header_sources(&d) {
                if rect.contains(x, y) { self.open_picker(item); return true; }
            }
        }
        for (i, row) in d.rows.iter().enumerate() {
            // the label of a picker row opens the list as well
            let on_label = matches!(row.ctl, Ctl::Pick) && row.label.contains(x, y);
            if !row.ctrl.contains(x, y) && !on_label {
                continue;
            }
            self.focus = i;
            if on_label {
                self.open_picker(row.it);
                break;
            }
            match &row.ctl {
                Ctl::Text => {}
                Ctl::Seg(opts, _) => {
                    let n = opts.len() as i32;
                    let k = ((x - row.ctrl.x) * n / row.ctrl.w).clamp(0, n - 1) as usize;
                    self.select_option(row.it, &opts[k]);
                }
                Ctl::Value | Ctl::Pick => {
                    let (mi, mid, pl) = Self::value_parts(row.ctrl);
                    if mi.contains(x, y) {
                        self.adjust(row.it, -1);
                    } else if pl.contains(x, y) {
                        self.adjust(row.it, 1);
                    } else if let Ctl::Pick = row.ctl {
                        self.open_picker(row.it);
                    } else if mid.contains(x, y) {
                        // the value of a number row opens the numeric pad, as in the Studio sheets
                        self.open_numpad(row.it);
                    }
                }
                Ctl::Button => self.activate(row.it),
            }
            break;
        }
        true
    }

    fn close_menu(&mut self) {
        if self.menu == MenuId::MeasRow { self.meas_cfg = None; }
        self.menu_open = false;
        self.menu = MenuId::Main;
        self.sheet.scroll = 0;
        self.parent = None;
        self.menu_history.clear();
        self.pick = None;
    }

    /// Power dialog in the middle of the screen: its box, and the buttons Power off, Restart, Cancel.
    fn pwr_layout(&self) -> (Rect, [Rect; 3]) {
        let b = self.scr.rect().center(500, 200);
        let btn = |dx: i32| Rect::new(b.x + 250 + dx, b.y + 90, 140, 64);
        (b, [btn(-230), btn(-70), btn(90)])
    }

    fn draw_overlay(&self, cv: &mut Canvas) {
        self.draw_load_slot(cv);
        let f = self.fonts();
        self.draw_osk(cv);
        self.draw_trig_slider(cv);
        self.draw_numpad(cv);
        if self.power_dlg {
            cv.blend(self.scr.rect(), rgb(0, 0, 0), 160);
            let (r, [off, restart, cancel]) = self.pwr_layout();
            cv.framed(r, 8, 2, ACCENT, 7, DLG_BG);
            cv.text_in(&f.lb, Rect::new(r.x, r.y + 18, r.w, 28), 1, "Power", ACCENT);
            cv.text_in(&f.m, Rect::new(r.x, r.y + 52, r.w, 24), 1, "Power key again or tap outside to cancel", DIM);
            for (b, t, c) in [(off, "Power off", RED), (restart, "Restart", ACCENT), (cancel, "Cancel", TEXT)] {
                btn(cv, b, mix(DLG_BG, c, 40), c);
                cv.text_in(&f.mb, b, 1, t, c);
            }
        }
        if self.dialog.is_some() && self.sysinfo.is_some() {
            self.draw_sysinfo(cv);
        } else if let Some(lines) = &self.dialog {
            let scr = self.scr.rect();
            cv.blend(scr, rgb(0, 0, 0), 140);
            // taller for long texts (About with experimental limits)
            let h = (40 + lines.len() as i32 * 30).clamp(260, scr.h - 20);
            // wider for long lines at the larger text sizes
            let w = lines.iter().map(|l| f.m.width(l) + 40).max().unwrap_or(0).clamp(480, scr.w - 20);
            let r = scr.center(w, h);
            cv.framed(r, 8, 2, ACCENT, 7, DLG_BG);
            for (i, l) in lines.iter().enumerate() {
                let fnt = if i == 0 { &f.lb } else { &f.m };
                cv.text_in(fnt, Rect::new(r.x, r.y + 20 + i as i32 * 30, r.w, 26), 1, l, if i == 0 { ACCENT } else { TEXT });
            }
        }
    }

    // ------------------------------------------------------------------ input

    /// A menu is on the screen (the desktop window turns Multi 1 with the wheel only then).
    #[cfg_attr(target_os = "linux", allow(dead_code))] // used by the desktop build and the tests
    pub fn menu_is_open(&self) -> bool {
        self.menu_open
    }

    pub fn handle(&mut self, ev: Ev) {
        if self.start_up_input(&ev) {
            return;
        }
        match ev {
            Ev::Key(k) => self.panel_key(k),
            Ev::Touch { id, phase, x, y } => self.multi_touch(id, phase, x, y),
            Ev::Wheel { x, y, steps, ctrl, shift, alt } => {
                if self.menu_open { crate::cmd::run(self, &format!("knob multi1 {steps:+}")); }
                else if !ctrl && !shift && !alt && self.panel_wheel(x, y, steps) {}
                else if alt { crate::cmd::run(self, &format!("knob tlevel {:+}", steps.clamp(-32, 32) * 5)); }
                else if ctrl { self.pointer_wheel(x, y, steps, true); }
                else if shift { self.pointer_wheel_motion(x, y, steps, false, true); }
                else { self.pointer_wheel(x, y, steps, false); }
            }
        }
    }

    fn knob_accel(&mut self, k: u8) -> i32 {
        let i = ((k >> 4) & 15) as usize;
        let dt = self.now - self.knob_t[i];
        self.knob_t[i] = self.now;
        if dt < 0.04 {
            4
        } else if dt < 0.1 {
            2
        } else {
            1
        }
    }

    /// Force belongs to the current acquisition owner, independent of modal UI.
    pub(crate) fn force_trigger(&mut self) {
        if self.m.run == RunState::Stop && self.m.trig_mode == TrigMode::Single {
            self.scpi_arm_operation();
            self.single_armed = false;
        }
        if self.record_mode() { self.record_key(key::FORCE); return; }
        self.bus_candidate_key(key::FORCE);
        self.m.run = RunState::Run;
        self.src.force_hw();
        self.toast("Force trigger");
    }

    /// A key of the front panel (the panel sends a key's code when it is pressed and 0 when it is let go): as `key`,
    /// but a channel key acts when it is let go, and held for `LONG_PRESS` it switches the channel between AC and DC
    /// coupling instead (at once, while still held). Scripts, the control FIFO and the web remote press keys through
    /// `key`, as before.
    pub fn panel_key(&mut self, k: u8) {
        let c = k.wrapping_sub(key::CH1) as usize;
        if c < 4 && c < n_ch() {
            // a second channel key while one is held: the held one counts as a short press
            self.ch_release();
            self.ch_hold = Some((c, self.now, false));
            return;
        }
        if k == 0 {
            self.ch_release();
        } else if self.ch_hold.is_some() {
            self.ch_release();
        }
        self.key(k);
    }

    /// A held channel key let go: its own action, unless it was held long enough for AC/DC.
    fn ch_release(&mut self) {
        if let Some((c, _, long)) = self.ch_hold.take() {
            if !long {
                self.key(key::CH1 + c as u8);
            }
        }
    }

    /// A channel key held for `LONG_PRESS`: AC/DC once; letting it go then does nothing more. A finger held on a
    /// channel's card opens that channel's quick actions (never a silent AC/DC); the lift is no tap.
    fn ch_hold_poll(&mut self) {
        if let Some((c, t)) = self.card_hold {
            let still = self.touch.down && (self.touch.x - self.touch.x0).abs() < 16 && (self.touch.y - self.touch.y0).abs() < 16;
            if !still {
                self.card_hold = None;
            } else if self.now - t >= LONG_PRESS {
                self.card_hold = None;
                let (x, y) = (self.touch.x0, self.touch.y0);
                self.touch.down = false;
                self.channel_popup(c, x, y);
                self.workspace.lift_pending = true;
            }
        }
        if let Some((c, t, false)) = self.ch_hold {
            if self.now - t >= LONG_PRESS {
                self.ch_hold = Some((c, t, true));
                self.toggle_ac_dc(c);
            }
        }
    }

    /// A long press of a channel key: AC or DC coupling.
    fn toggle_ac_dc(&mut self, c: usize) {
        let to = if self.m.ch[c].coupling == Coupling::Ac { Coupling::Dc } else { Coupling::Ac };
        if !board().couplings().contains(&to) {
            return self.toast(&format!("CH{}: no {} coupling on this scope", c + 1, to.name()));
        }
        self.m.ch[c].coupling = to;
        self.toast(&format!("CH{}: {} coupling", c + 1, to.name()));
        self.settings_changed();
    }

    pub fn key(&mut self, k: u8) {
        if k == 0 {
            return; // release
        }
        if k == key::FORCE { self.force_trigger(); return; }
        if self.load_slot_ask { self.cancel_load_slot(); return; }
        if (k == key::RUNSTOP && self.m.run == RunState::Run) || k == key::SINGLE || k == key::DEFAULT || (0x15..=0x18).contains(&k) {
            self.sim_cancel_generation();
        }
        if k == key::POWER {
            // the stock Launcher answers every power-key press with "cancel power-down" (FA 05 02); the MCU very
            // likely cuts power by itself otherwise (board.md section 4)
            self.panel_tx.push(crate::board::PanelCmd::CancelPowerDown);
            self.power_dlg = !self.power_dlg;
            if self.power_dlg {
                self.dialog = None;
                self.close_menu();
            }
            return;
        }
        if self.power_dlg {
            self.power_dlg = false;
            return;
        }
        if self.wlan.osk.is_some() {
            return self.osk_key(k);
        }
        if self.dialog.is_some() {
            self.dialog = None;
            return;
        }
        if self.record_key(k) { return; }
        self.bus_candidate_key(k);
        let m = &mut self.m;
        match k {
            0x15..=0x18 if (k - 0x15) as usize >= n_ch() => {
                let name = &oos_hal::model::platform().caps.name;
                self.toast(&format!("CH{}: the {} has {} channels", k - 0x14, name, n_ch()));
            }
            0x15..=0x18 => {
                let c = (k - 0x15) as usize;
                if !m.ch[c].on {
                    m.ch[c].on = true;
                    m.active = c;
                    if m.run == RunState::Run {
                        m.floor_live_tdiv();
                    }
                    self.open_menu(MenuId::Channel(c));
                } else if m.n_on() > 1 && m.active != c {
                    // several channels on: the first press only selects the channel, so the knobs act on it; the
                    // next press opens its menu. Another channel's menu would show the wrong channel: it closes.
                    m.active = c;
                    if self.menu_open && matches!(self.menu, MenuId::Channel(_)) {
                        self.close_menu();
                    }
                } else if self.menu_open && self.menu == MenuId::Channel(c) {
                    m.ch[c].on = false;
                    self.menu_open = false;
                    if let Some(o) = (0..NCH).find(|&i| self.m.ch[i].on) {
                        self.m.active = o;
                    }
                } else {
                    m.active = c;
                    self.open_menu(MenuId::Channel(c));
                }
                self.select_workspace_source(WaveId::Ch(if self.m.ch[c].on { c } else { self.m.active }));
                self.settings_changed();
            }
            key::RUNSTOP => {
                m.run = if m.run == RunState::Run { RunState::Stop } else { RunState::Run };
                if m.run == RunState::Run && m.trig_mode == TrigMode::Single {
                    m.trig_mode = TrigMode::Auto;
                }
                if m.run == RunState::Run && (m.n_on() > 0 || m.la_on()) {
                    m.floor_live_tdiv();
                }
            }
            key::SINGLE => {
                self.operation_serial = self.operation_serial.wrapping_add(1);
                self.single_operation = self.operation_serial;
                m.trig_mode = TrigMode::Single;
                m.run = RunState::Run;
                self.single_armed = false;
                self.dirty_map = true;
                if m.n_on() > 0 || m.la_on() {
                    m.floor_live_tdiv();
                }
            }
            key::AUTO => {
                self.autoset_live_only();
            }
            key::CLEAR => self.clear_display(),
            key::DEFAULT => {
                let meas = Self::default_meas();
                // the Quick key action is a preference, not part of the set-up that Default resets
                let (quick, text_size) = (m.quick, m.text_size);
                self.m = Model::default();
                self.m.meas = meas;
                (self.m.quick, self.m.text_size) = (quick, text_size);
                self.bus_auto = None;
                self.clear_measurement_statistics();
                self.select_workspace_source(WaveId::Ch(self.m.active));
                self.meas_own.clear();
                self.meas_cfg = None;
                self.ensure_meas_own();
                self.stats_on = false;
                self.xy = false;
                self.mask = None;
                self.mask_ref = None;
                self.close_menu();
                self.dirty_map = true;
                self.toast("Default set-up");
            }
            key::SLOPE => self.adjust(Item::TrigSlope, 1),
            key::FORCE => {
                self.force_trigger();
            }
            key::MEASURE => self.open_meas_list(),
            key::CURSOR => {
                if self.m.cursor == CursorMode::Off {
                    self.m.cursor = CursorMode::Time;
                }
                self.toggle_menu(MenuId::Cursor)
            }
            key::TMENU => self.toggle_menu(MenuId::Trigger),
            // the Horizontal Menu key: Acquire (with Horizontal as a sub menu); the TIME card opens Horizontal
            key::HMENU => self.toggle_menu(MenuId::Acquire),
            key::QUICK => self.quick(),
            key::TOUCHLOCK => {
                self.m.touch_lock = !self.m.touch_lock;
                let s = if self.m.touch_lock { "Touch locked" } else { "Touch on" };
                self.toast(s);
            }
            key::MATH => self.toggle_menu(MenuId::Math),
            key::REF => self.toggle_menu(MenuId::Ref),
            key::ANALYSE => self.toggle_menu(MenuId::Decode),
            // history: back / forward step through the stored records (stops the acquisition), play-stop = live
            0x08 => {
                self.history_step(-1);
            }
            0x06 => {
                self.history_step(1);
            }
            0x07 | key::NAVIGATE => {
                if self.hist_pos.is_some() {
                    self.hist_pos = None;
                    self.m.run = RunState::Run;
                    self.toast("Live");
                } else {
                    self.history_step(0);
                }
            }
            key::LA | key::GI => {
                let p = oos_hal::model::platform();
                let (name, has) = if k == key::LA { ("logic analyser port", p.caps.la) } else { ("generator", p.caps.has_gen()) };
                if !has {
                    self.toast(&format!("The {} has no {}", p.caps.name, name));
                } else if k == key::GI {
                    self.open_menu(MenuId::Afg);
                } else {
                    self.la_key();
                }
            }
            0xf1..=0xf4 => self.toast(&format!("CH{} overload", k - 0xf0)),
            _ => self.knob(k),
        }
    }

    fn knob(&mut self, k: u8) {
        let base = k & 0xf0;
        let sub = k & 0x0f;
        if !(0x8..=0xe).contains(&(base >> 4)) || !(0xa..=0xc).contains(&sub) {
            return;
        }
        let push = sub == 0xc;
        let dir = if sub == 0xa { 1 } else { -1 };
        let acc = if push { 1 } else { self.knob_accel(k) };
        if let Some(WaveId::Ref(slot)) = self.selected_workspace_source() {
            self.ref_sel = slot;
            match base {
                0xd0 => {
                    if push { self.toggle_menu(MenuId::Ref); }
                    else { self.adjust(Item::RefVpd, -dir); }
                    return;
                }
                0x90 => {
                    if push {
                        self.refs[self.ref_sel].as_mut().unwrap().offset = 0.0;
                        self.settings_changed();
                    } else { self.adjust(Item::RefOffset, dir * acc); }
                    return;
                }
                _ => {}
            }
        } else if self.record_mode() && matches!(base, 0xd0 | 0x90) {
            self.toast("Select a loaded reference for the vertical controls");
            return;
        }
        let c = self.m.active;
        match base {
            0xd0 => {
                if push {
                    self.toggle_menu(MenuId::Channel(c));
                } else {
                    // CW = more sensitive (smaller V/div)
                    self.adjust(Item::ChScale(c), -dir);
                }
            }
            0x90 => {
                if push {
                    self.m.ch[c].offset = 0.0;
                    self.settings_changed();
                } else {
                    self.adjust(Item::ChOffset(c), dir * acc);
                }
            }
            0xc0 => {
                if push {
                    let on = !self.m.zoom;
                    self.set_zoom(on);
                } else if self.m.zoom {
                    self.adjust(Item::ZScale, -dir);
                } else {
                    self.adjust(Item::HScale, -dir);
                }
            }
            0x80 => {
                if push {
                    if self.m.zoom {
                        self.m.zdelay = self.m.delay;
                    } else {
                        self.m.delay = 0.0;
                    }
                    self.settings_changed();
                } else if self.m.zoom {
                    self.adjust(Item::ZDelay, -dir * acc);
                } else {
                    self.adjust(Item::HDelay, -dir * acc);
                }
            }
            0xe0 => {
                if push {
                    self.level_50();
                } else {
                    self.adjust(Item::TrigLevel, dir * acc);
                }
            }
            0xb0 | 0xa0 => {
                let items = self.items(self.menu);
                if !self.menu_open {
                    self.open_menu(MenuId::Main);
                    return;
                }
                if items.is_empty() {
                    return;
                }
                self.focus = self.focus.min(items.len() - 1);
                let it = items[self.focus];
                let picker = self.dlg_layout().and_then(|d| d.rows.get(self.focus).map(|r| matches!(r.ctl, Ctl::Pick))).unwrap_or(false);
                if push {
                    if picker {
                        // push on a choice with a long list: the list
                        self.open_picker(it);
                    } else if Self::kind(it) == Kind::Value {
                        // push on a value: next item
                        self.focus = (self.focus + 1) % items.len();
                    } else {
                        self.activate(it);
                    }
                } else if base == 0xb0 {
                    // multi 1 selects the item
                    self.focus = (self.focus as i32 + dir).rem_euclid(items.len() as i32) as usize;
                } else {
                    // multi 2 changes the value
                    if Self::kind(it) != Kind::Action {
                        self.adjust(it, dir * acc);
                    }
                }
            }
            _ => {}
        }
    }

    /// Two fingers in the grid: horizontal spread = time/div, vertical spread = V/div of the active channel.
    /// A second finger makes the touch a pinch or a two-finger swipe: it is never a long press. Every hold that is
    /// counting stops, and a context menu the first finger's hold had already opened goes away again.
    fn second_finger_down(&mut self) {
        if self.workspace_cancel_hold() {
            self.studio.ctx = None;
            if self.menu_open && self.menu == MenuId::MeasHold { self.close_menu(); }
        }
        self.studio.md.hold = None;
        self.studio.sz.press = None;
        self.studio.dock.press = None;
        // the lift of the first finger is not a tap either
        if self.touch.down { self.touch.held = true; }
    }

    fn multi_touch(&mut self, id: i32, ph: Phase, x: i32, y: i32) {
        if id == 1 && ph == Phase::Down { self.second_finger_down(); }
        if self.load_slot_ask {
            if id == 0 && ph == Phase::Up { self.load_slot_tap(x, y); }
            return;
        }
        if self.eye_open() && self.eye_pane_rect().contains(x,y) {
            if id == 0 { self.touch_ev(ph,x,y); }
            return;
        }
        if self.m.touch_lock {
            if id == 0 {
                self.touch_ev(ph, x, y);
            }
            return;
        }
        // the second finger on an axis strip: a pinch along that axis
        if id == 1 && self.m.layout == Layout::Studio && self.studio.axis.touch.is_some() {
            if ph == Phase::Down { self.axis_pinch_down(x, y); } else { self.axis_pinch_event(ph, x, y); }
            return;
        }
        let map = self.zoom_drag_map(self.touch.x, self.touch.y);
        let modal = self.load_slot_ask || self.menu_open || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some();
        let detail_y = self.split_zoom(self.grid).1.y;
        let p = &mut self.pinch;
        if id == 1 {
            match ph {
                Phase::Down => {
                    if !modal && self.touch.down && map.is_some_and(|(area, _, _)| area.contains(x, y)) {
                        p.active = true;
                        p.area = map.unwrap().0;
                        p.detail = self.m.zoom && p.area.y >= detail_y;
                        p.p0 = (self.touch.x, self.touch.y);
                        p.p1 = (x, y);
                        p.ref_dx = (p.p1.0 - p.p0.0).abs();
                        p.ref_dy = (p.p1.1 - p.p0.1).abs();
                        // cancel the one-finger drag
                        self.touch.drag = Drag::None;
                        self.zoom_path.clear();
                        self.zoom_map = None;
                    }
                }
                Phase::Move => p.p1 = (x, y),
                Phase::Up => {
                    p.active = false;
                    self.touch.drag = Drag::None;
                    self.touch.down = false; // no tap when the first finger lifts
                }
            }
        } else if p.active {
            match ph {
                Phase::Move => p.p0 = (x, y),
                Phase::Up => {
                    p.active = false;
                    self.touch.down = false;
                }
                Phase::Down => {}
            }
        } else {
            self.touch_ev(ph, x, y);
            return;
        }
        let p = &mut self.pinch;
        if !p.active {
            return;
        }
        let dx = (p.p1.0 - p.p0.0).abs();
        let dy = (p.p1.1 - p.p0.1).abs();
        let mut dt = 0;
        let mut dv = 0;
        if p.ref_dx > 40 && dx * 100 > p.ref_dx * 135 {
            dt = -1;
            p.ref_dx = dx;
        } else if p.ref_dx > 40 && dx * 100 < p.ref_dx * 74 {
            dt = 1;
            p.ref_dx = dx;
        }
        if p.ref_dy > 40 && dy * 100 > p.ref_dy * 135 {
            dv = -1;
            p.ref_dy = dy;
        } else if p.ref_dy > 40 && dy * 100 < p.ref_dy * 74 {
            dv = 1;
            p.ref_dy = dy;
        }
        let area = p.area;
        let detail = p.detail;
        let midpoint = ((p.p0.0 + p.p1.0) as f32 * 0.5, (p.p0.1 + p.p1.1) as f32 * 0.5);
        let fx = ((midpoint.0 - area.x as f32) / area.w.max(1) as f32).clamp(0.0, 1.0);
        let fy = ((midpoint.1 - area.y as f32) / area.h.max(1) as f32).clamp(0.0, 1.0);
        let studio_zoom = self.m.layout == Layout::Studio && self.m.zoom;
        if dt != 0 {
            if studio_zoom {
                // the overview: the width of the zoom box; the zoomed waveform: nothing
                if !detail { self.overview_box_step(-dt, midpoint.0 as i32); }
            }
            else if detail { self.zoom_horizontal_step(dt, area, fx); }
            else { self.main_horizontal_step(dt, fx); }
        }
        if dv != 0 {
            if studio_zoom && detail {}
            else if detail { self.zoom_vertical_step(-dv, area, fy); }
            else if !self.m.zoom {
                match self.selected_workspace_source() {
                    Some(WaveId::Ref(i)) => { self.ref_sel = i; self.adjust(Item::RefVpd, dv); }
                    _ => self.adjust(Item::ChScale(self.m.active), dv),
                }
            }
        }
    }

    /// A finger in a split plot works in the window it went down in: its channels, its rectangle.
    fn touch_ev(&mut self, ph: Phase, x: i32, y: i32) {
        if self.studio_touch(ph, x, y) { return; }
        if self.workspace_touch(ph, x, y) { return; }
        if self.wins.len() < 2 {
            return self.touch_in(ph, x, y);
        }
        let keep = (self.area, self.cur_win);
        let w = if ph == Phase::Down { self.wins.iter().position(|r| r.contains(x, y)).unwrap_or(0) } else { self.touch.win.min(self.wins.len() - 1) };
        (self.area, self.cur_win) = (self.wins[w], w);
        self.touch_in(ph, x, y);
        (self.area, self.cur_win) = keep;
    }

    fn touch_in(&mut self, ph: Phase, x: i32, y: i32) {
        let covered = self.menu_rect().map_or(false, |d| d.contains(x, y));
        let in_plot = ph == Phase::Down && self.grid.contains(x, y) && !covered
            && !(self.eye_open() && self.eye_pane_rect().contains(x,y));
        // a press on the corner button moves that button; it is not a drag of the trace
        let on_full = ph == Phase::Down && !covered && self.full_btn().1.contains(x, y);
        // the plot buttons are taps; a zoom box starts on the waveform, not on them
        let on_chrome = ph == Phase::Down
            && (on_full || (self.zoom_btn().1.contains(x, y) && !self.view_on()) || (self.m.zoom && self.zoom_readout().contains(x, y)) || (self.view_on() && self.view_btn().contains(x, y)));
        let zmap = if self.zoom_arm && in_plot && !on_chrome { self.zoom_drag_map(x, y) } else { None };
        let full_from = if on_full {
            let (r, _) = self.full_btn();
            Some((r.x, r.y, self.full_at))
        } else {
            None
        };
        // a finger on a list that scrolls (the measurement page, the Results pane, a table) moves it
        let grip = if ph == Phase::Down && !self.m.touch_lock && self.dialog.is_none() && !self.power_dlg && self.wlan.osk.is_none() { self.scroll_grip(x, y) } else { None };
        let t = &mut self.touch;
        match ph {
            Phase::Down => {
                let sd = if self.m.zoom { self.m.zdelay } else { self.m.delay };
                *t = TouchState { down: true, x0: x, y0: y, x, y, t0: self.now, held: false, drag: Drag::None, start_offset: 0.0, start_delay: sd, start_level: self.m.trig_level, start_cur: 0.0, win: self.cur_win };
                if self.m.touch_lock || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() {
                    return;
                }
                if let Some((l, g)) = grip {
                    t.drag = Drag::Scroll(l, g);
                    return;
                }
                if let Some((ox, oy, prev)) = full_from {
                    t.drag = Drag::FullBtn { ox, oy, prev };
                } else if !in_plot {
                    // a finger held on the card of a channel that is on: AC/DC, as the channel key
                    let (chs, _, _) = self.cards();
                    let y = self.bar_y(y);
                    self.card_hold = chs.iter().position(|r| r.contains(x, y)).filter(|&c| self.m.ch[c].on).map(|c| (c, self.now));
                } else {
                    // an edge strip of the plot swipes between the views (not while a zoom box is armed)
                    let edge = zmap.is_none() && self.view_edge(x);
                    let t = &mut self.touch;
                    t.drag = if edge { Drag::ViewEdge } else { Drag::Pending };
                    if let Some(map) = zmap {
                        t.drag = Drag::ZoomBox;
                        self.zoom_path = vec![(x, y), (x, y)];
                        self.zoom_map = Some(map);
                    } else if let Some((k, _)) = self.zone_arm {
                        t.drag = Drag::Zone(k);
                        self.zone_path = vec![(x, y)];
                    }
                }
            }
            Phase::Move => {
                if !t.down {
                    return;
                }
                t.x = x;
                t.y = y;
                if let Drag::Scroll(l, g) = t.drag {
                    let y0 = t.y0;
                    self.scroll_move(l, g, y0, y);
                    return;
                }
                if t.drag == Drag::ViewEdge {
                    let (x0, dx, dy) = (t.x0, x - t.x0, y - t.y0);
                    match self.view_swipe(x0, dx, dy) {
                        Some(true) => self.touch.drag = Drag::None,
                        Some(false) => return,
                        None => self.touch.drag = Drag::Pending,
                    }
                }
                let t = &mut self.touch;
                let (tx0, ty0, tdrag) = (t.x0, t.y0, t.drag);
                let (dx, dy) = (x - tx0, y - ty0);
                if tdrag == Drag::Pending && (dx.abs() > 10 || dy.abs() > 10) {
                    // choose what to drag
                    let near_right = tx0 > self.area.x + self.area.w - 50;
                    let near_left = tx0 < self.area.x + 50;
                    let cur_hit = |frac: f32, vertical: bool| -> bool {
                        if vertical {
                            (self.area.x + (frac * self.area.w as f32) as i32 - tx0).abs() < 24
                        } else {
                            (self.area.y + (frac * self.area.h as f32) as i32 - ty0).abs() < 24
                        }
                    };
                    let m = &self.m;
                    // the cursors stand in the window of the active channel
                    // the cursors can be grabbed where they are shown: in the lane or window of their source; a time cursor also in
                    // any other lane, where its band edge stands at the same time
                    let here = self.cursor_visible_here();
                    let cur_hit = |frac: f32, vertical: bool| (here || (vertical && self.wins.len() > 1)) && cur_hit(frac, vertical);
                    let under = if dy.abs() > dx.abs() && !near_left && !m.zoom { self.trace_under(tx0, ty0) } else { None };
                    let overview_drag = if m.zoom {
                        let overview = self.split_zoom(self.grid).0;
                        if overview.contains(tx0, ty0) {
                            let ppd = overview.w as f32 / HDIV;
                            let half = tscales()[m.zscale] * HDIV * 0.5;
                            let edge = |time| overview.x + overview.w / 2 + ((time - m.delay) / m.time_per_div() * ppd) as i32;
                            let left = (tx0 - edge(m.zdelay - half)).abs();
                            let right = (tx0 - edge(m.zdelay + half)).abs();
                            Some(if left < 20 && left <= right { Drag::ZoomLeft }
                                 else if right < 20 { Drag::ZoomRight }
                                 else if self.m.layout == Layout::Studio && { let (xa, xb) = self.zoom_window_x(overview); tx0 < xa || tx0 > xb } { Drag::ZoomNew }
                                 else { Drag::Overview })
                        } else { None }
                    } else { None };
                    let drag = if let Some(drag) = overview_drag { drag }
                    else if m.cursor == CursorMode::Xy && dy.abs() >= dx.abs() && cur_hit(m.cur_c, false) {
                        Drag::CursorC
                    } else if m.cursor == CursorMode::Xy && dy.abs() >= dx.abs() && cur_hit(m.cur_d, false) {
                        Drag::CursorD
                    } else if matches!(m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) && cur_hit(m.cur_a, true) {
                        Drag::CursorA
                    } else if matches!(m.cursor, CursorMode::Time | CursorMode::Track | CursorMode::Xy) && cur_hit(m.cur_b, true) {
                        Drag::CursorB
                    } else if m.cursor == CursorMode::Volt && cur_hit(m.cur_a, false) {
                        Drag::CursorA
                    } else if m.cursor == CursorMode::Volt && cur_hit(m.cur_b, false) {
                        Drag::CursorB
                    } else if near_right && dy.abs() >= dx.abs() && self.trigger_absolute_level_visible(false) && (m.trig_ref.is_some() || self.shows(m.trig_src)) {
                        let secondary = matches!(m.trig_type, TrigType::Delay | TrigType::SetupHold);
                        let upper = self.trigger_level_y(m.trig_level, false).unwrap_or(ty0);
                        let lower = self.trigger_level_y(m.trig_level2, secondary).unwrap_or(ty0);
                        if matches!(m.trig_type, TrigType::Runt | TrigType::Slope | TrigType::Window | TrigType::Delay | TrigType::SetupHold)
                            && (ty0 - lower).abs() < (ty0 - upper).abs() { Drag::Level2 } else { Drag::Level }
                    } else if m.zoom && dy.abs() > dx.abs() {
                        Drag::ZoomVertical
                    } else if under.is_some() && !(m.bus.on && (self.bus_y() - ty0).abs() <= 18) {
                        // the trace under the finger moves, not the active channel
                        match under { Some(WaveId::Ref(i)) => Drag::RefOffset(i), Some(WaveId::Ch(c)) => Drag::Offset(c), None => Drag::Delay }
                    } else if dy.abs() > dx.abs() && self.math_lane_at(tx0, ty0).is_some() {
                        // a lane of a math channel: it moves like a channel's does
                        Drag::MathOffset(self.math_lane_at(tx0, ty0).unwrap())
                    } else if dy.abs() > dx.abs() && self.drag_reference().is_some() {
                        Drag::RefOffset(self.drag_reference().unwrap())
                    } else if dy.abs() > dx.abs() {
                        // nearest enabled channel marker when starting at the left edge, else the active channel (in a
                        // split plot: of this window)
                        let mut c = if self.shows(m.active) { m.active } else { (0..NCH).find(|&i| m.ch[i].on && self.shows(i)).unwrap_or(m.active) };
                        let mut best = i32::MAX;
                        if near_left {
                            for i in 0..NCH {
                                if m.ch[i].on && self.shows(i) {
                                    let d = (self.y_of_volts(i, 0.0) - ty0).abs();
                                    if d < best {
                                        best = d;
                                        c = i;
                                    }
                                }
                            }
                        }
                        let bus_d = (self.bus_y() - ty0).abs();
                        if m.bus.on && bus_d <= 18 && bus_d < best { Drag::Bus } else { Drag::Offset(c) }
                    } else {
                        Drag::Delay
                    };
                    let start_offset = match drag {
                        Drag::Offset(c) => self.m.ch[c].offset,
                        Drag::RefOffset(i) => self.refs[i].as_ref().map_or(0.0, |reference| reference.offset),
                        Drag::MathOffset(k) => self.m.math_ch(k).offset,
                        Drag::ZoomLeft | Drag::ZoomRight => tscales()[self.m.zscale],
                        Drag::ZoomVertical => self.zoom_vcenter,
                        Drag::Bus => self.m.bus.pos,
                        _ => 0.0,
                    };
                    let start_cur = match drag {
                        Drag::ZoomNew => { let o = self.split_zoom(self.grid).0; self.m.delay + (tx0 - o.x - o.w / 2) as f32 / (o.w as f32 / HDIV) * self.m.time_per_div() }
                        Drag::ZoomLeft => self.m.zdelay + tscales()[self.m.zscale] * HDIV * 0.5,
                        Drag::ZoomRight => self.m.zdelay - tscales()[self.m.zscale] * HDIV * 0.5,
                        Drag::CursorA => self.m.cur_a,
                        Drag::CursorB => self.m.cur_b,
                        Drag::CursorC => self.m.cur_c,
                        Drag::CursorD => self.m.cur_d,
                        _ => 0.0,
                    };
                    match drag {
                        Drag::ZoomVertical => {
                            if let Some(i) = self.drag_reference() { self.select_workspace_source(WaveId::Ref(i)); }
                            else if let Some(c) = (0..NCH).find(|&c| self.m.ch[c].on && self.shows(c)) { self.select_workspace_source(WaveId::Ch(c)); }
                        }
                        Drag::Offset(c) => self.select_workspace_source(WaveId::Ch(c)),
                        Drag::RefOffset(i) => self.select_workspace_source(WaveId::Ref(i)),
                        _ => {},
                    }
                    let start_level = if drag == Drag::Level2 { self.m.trig_level2 } else { self.m.trig_level };
                    let t = &mut self.touch;
                    t.start_level = start_level;
                    t.drag = drag;
                    t.start_offset = start_offset;
                    t.start_cur = start_cur;
                }
                let t = &self.touch;
                let (dx, dy) = (x - t.x0, y - t.y0);
                let ppx = self.area.w as f32 / HDIV;
                let ppy = self.area.h as f32 / VDIV;
                match t.drag {
                    Drag::ZoomVertical => {
                        self.zoom_vcenter = t.start_offset + dy as f32 / ppy / self.zoom_vfactor;
                        self.dirty_map = true;
                    }
                    Drag::RefOffset(i) => {
                        if let Some(reference) = &mut self.refs[i] {
                            reference.offset = t.start_offset - dy as f32 / ppy * reference.vpd;
                        }
                    }
                    Drag::MathOffset(k) => {
                        // a math channel's position is in divisions
                        let mut c = self.m.math_ch(k);
                        c.offset = t.start_offset - dy as f32 / ppy;
                        if k == self.m.math_sel { self.m.math_offset = c.offset; }
                        self.m.maths[k] = c;
                        self.settings_changed();
                    }
                    Drag::Overview => {
                        let overview = self.split_zoom(self.grid).0;
                        self.m.zdelay = t.start_delay + dx as f32 / (overview.w as f32 / HDIV) * self.m.time_per_div();
                        self.clamp_zoom(); self.settings_changed();
                    }
                    Drag::ZoomNew => {
                        let overview = self.split_zoom(self.grid).0;
                        let time = self.m.delay + (x.clamp(overview.x, overview.right()) - overview.x - overview.w / 2) as f32 / (overview.w as f32 / HDIV) * self.m.time_per_div();
                        let (t0, t1) = (t.start_cur.min(time), t.start_cur.max(time));
                        if t1 - t0 < 4.0 * self.m.time_per_div() * HDIV / overview.w.max(1) as f32 { return; }
                        // the finest step whose window holds the box, and not slower than the main time base
                        let scales = tscales();
                        let mut z = 0;
                        while z < self.m.tscale && scales[z] * HDIV < (t1 - t0) * 0.999 { z += 1; }
                        self.m.zscale = z;
                        self.m.zdelay = (t0 + t1) * 0.5;
                        self.clamp_zoom(); self.settings_changed();
                    }
                    Drag::ZoomLeft | Drag::ZoomRight => {
                        let overview = self.split_zoom(self.grid).0;
                        let time = self.m.delay + (x - overview.x - overview.w / 2) as f32 / (overview.w as f32 / HDIV) * self.m.time_per_div();
                        let span = if t.drag == Drag::ZoomLeft { t.start_cur - time } else { time - t.start_cur };
                        let wanted = (span / HDIV).max(tscales()[0]);
                        self.m.zscale = tscales().iter().enumerate().min_by(|a, b| (a.1 - wanted).abs().total_cmp(&(b.1 - wanted).abs())).map_or(0, |(i, _)| i).min(self.m.tscale);
                        let half = tscales()[self.m.zscale] * HDIV * 0.5;
                        self.m.zdelay = if t.drag == Drag::ZoomLeft { t.start_cur - half } else { t.start_cur + half };
                        self.clamp_zoom(); self.settings_changed();
                    }
                    Drag::Offset(c) => {
                        let vpd = self.m.ch[c].volts_per_div();
                        self.m.ch[c].offset = t.start_offset - dy as f32 / ppy * vpd;
                        self.settings_changed();
                    }
                    Drag::Delay => {
                        if self.m.zoom {
                            let zt = tscales()[self.m.zscale];
                            self.m.zdelay = t.start_delay - dx as f32 / ppx * zt;
                            self.clamp_zoom();
                        } else {
                            self.m.delay = t.start_delay - dx as f32 / ppx * self.m.time_per_div();
                        }
                        self.settings_changed();
                    }
                    Drag::Level | Drag::Level2 => {
                        let lower = t.drag == Drag::Level2;
                        let secondary = lower && matches!(self.m.trig_type, TrigType::Delay | TrigType::SetupHold);
                        let id = if secondary { self.m.trigger_wave2() } else { self.m.trigger_wave() };
                        let ppy = self.source_area(id).map_or(ppy, |area| area.h as f32 / VDIV * self.zoom_vertical_transform(area).0);
                        let level = t.start_level - dy as f32 / ppy * self.trigger_vpd(secondary);
                        // a two-level type: the dragged line stops at the other one, as the knob does
                        let two = self.m.trig_type.two_level();
                        if lower {
                            self.m.trig_level2 = if two { level.min(self.m.trig_level) } else { level };
                        } else {
                            self.m.trig_level = if two { level.max(self.m.trig_level2) } else { level };
                        }
                        self.settings_changed();
                    }
                    Drag::Bus => {
                        self.m.bus.pos = (t.start_offset - dy as f32 / ppy).clamp(BUS_POS_MIN, BUS_POS_MAX);
                        self.settings_changed();
                    }
                    Drag::Zone(_) => {
                        let shape = self.zone_arm.map_or(false, |a| a.1);
                        let last = self.zone_path.last().copied().unwrap_or((x, y));
                        if !shape {
                            self.zone_path.truncate(1);
                            self.zone_path.push((x, y));
                        } else if (x - last.0).abs() + (y - last.1).abs() >= 6 {
                            self.zone_path.push((x, y));
                        }
                    }
                    Drag::ZoomBox => {
                        if let Some(p) = self.zoom_path.last_mut() {
                            *p = (x, y);
                        }
                    }
                    Drag::FullBtn { ox, oy, prev } => {
                        // past the tap slop the button follows the finger; a shorter move is still a tap
                        if dx.abs() >= 16 || dy.abs() >= 16 {
                            let (px, py) = self.clamp_full(ox + dx, oy + dy);
                            self.full_at = Some((px, py));
                        } else {
                            self.full_at = prev;
                        }
                    }
                    Drag::CursorA | Drag::CursorB | Drag::CursorC | Drag::CursorD => {
                        let v = if self.m.cursor == CursorMode::Volt || matches!(t.drag, Drag::CursorC | Drag::CursorD) {
                            t.start_cur + dy as f32 / self.area.h as f32
                        } else {
                            t.start_cur + dx as f32 / self.area.w as f32
                        }
                        .clamp(0.0, 1.0);
                        match t.drag {
                            Drag::CursorA => {
                                let moved = v - self.m.cur_a;
                                self.m.cur_a = v;
                                if self.studio.cur_linked && self.m.layout == Layout::Studio { self.m.cur_b = (self.m.cur_b + moved).clamp(0.0, 1.0); }
                            }
                            Drag::CursorB => {
                                let moved = v - self.m.cur_b;
                                self.m.cur_b = v;
                                if self.studio.cur_linked && self.m.layout == Layout::Studio { self.m.cur_a = (self.m.cur_a + moved).clamp(0.0, 1.0); }
                            }
                            Drag::CursorC => self.m.cur_c = v,
                            Drag::CursorD => self.m.cur_d = v,
                            _ => {},
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                // Up
                let was = (t.down, t.drag, t.x0, t.y0, t.held);
                t.down = false;
                t.drag = Drag::None;
                t.held = false;
                if !was.0 {
                    return;
                }
                // the short menu is already open; this lift runs its shortcut instead of a tap
                if was.4 {
                    self.meas_hold_up(x, y);
                    return;
                }
                if let Drag::Zone(k) = was.1 {
                    self.finish_zone(k);
                    return;
                }
                if was.1 == Drag::ZoomBox {
                    self.finish_zoom_box();
                    return;
                }
                if let Drag::FullBtn { ox, oy, prev } = was.1 {
                    let (dx, dy) = (x - was.2, y - was.3);
                    if dx.abs() < 16 && dy.abs() < 16 {
                        self.full_at = prev;
                        self.tap(was.2, was.3);
                    } else {
                        let (px, py) = self.clamp_full(ox + dx, oy + dy);
                        self.full_at = Some((px, py));
                    }
                    return;
                }
                if self.dialog.is_some() {
                    self.dialog = None;
                    return;
                }
                if self.m.touch_lock {
                    // the Menu button ("Locked") is the one touch target left: it unlocks
                    if self.menu_btn().contains(was.2, was.3) {
                        self.key(key::TOUCHLOCK);
                    }
                    return;
                }
                // a press on a list that did not move it is a tap on what is under the finger
                if let Drag::Scroll(_, g) = was.1 {
                    if matches!(g, scroll::Grip::Content { .. }) && (x - was.2).abs() < 16 && (y - was.3).abs() < scroll::SLOP {
                        self.tap(was.2, was.3);
                    }
                    return;
                }
                if (was.1 == Drag::None || was.1 == Drag::Pending || was.1 == Drag::ViewEdge) && (x - was.2).abs() < 16 && (y - was.3).abs() < 16 {
                    self.tap(was.2, was.3);
                }
            }
        }
    }

    /// A box magnifies both axes without changing source scales or acquisition.
    fn finish_zoom_box(&mut self) {
        let path = std::mem::take(&mut self.zoom_path);
        let map = self.zoom_map.take();
        if path.len() != 2 {
            return;
        }
        let ((x0, y0), (x1, y1)) = (path[0], path[1]);
        // A plain tap ends box drawing; valid boxes can be drawn repeatedly.
        if (x1 - x0).abs() < 12 || (y1 - y0).abs() < 12 {
            if (x1 - x0).abs() < 12 && (y1 - y0).abs() < 12 { self.zoom_arm = false; }
            return;
        }
        let Some((a, tdiv, delay)) = map else { return };
        if a.w < 8 || tdiv <= 0.0 {
            return;
        }
        let ppd = a.w as f32 / HDIV;
        let at = |x: i32| {
            let x = x.clamp(a.x, a.x + a.w - 1);
            delay + (x - (a.x + a.w / 2)) as f32 / ppd * tdiv
        };
        let (t0, t1) = (at(x0.min(x1)), at(x0.max(x1)));
        let span = t1 - t0;
        if span <= 0.0 {
            return;
        }
        // finest step whose window still holds the box, and not slower than the main time base
        let scales = tscales();
        let mut z = 0;
        while z < self.m.tscale && scales[z] * HDIV < span * 0.999 {
            z += 1;
        }
        let on = self.m.zoom;
        let (factor, center) = self.zoom_vertical_transform(a);
        let low = y0.min(y1).clamp(a.y, a.y + a.h);
        let high = y0.max(y1).clamp(a.y, a.y + a.h);
        let midpoint = (low + high) as f32 * 0.5;
        let next_center = center + (0.5 - (midpoint - a.y as f32) / a.h as f32) * VDIV / factor;
        let next_factor = (factor * a.h as f32 / (high - low).max(1) as f32).clamp(1.0, 1024.0);
        if !on { self.zoom_saved = Some((self.m.tscale, self.m.tfine, self.m.delay)); }
        self.m.zscale = z;
        self.m.zdelay = (t0 + t1) * 0.5;
        self.m.zoom = true;
        self.zoom_vfactor = next_factor;
        self.zoom_vcenter = next_center;
        self.clamp_zoom();
        self.zoom_arm = true;
        self.dirty_map = true;
        if !on {
            self.relayout();
        }
    }

    /// Turns the drawn screen path into zone `k` (waveform coordinates of its source channel).
    fn finish_zone(&mut self, k: usize) {
        let shape = self.zone_arm.map_or(false, |a| a.1);
        self.zone_arm = None;
        let path = std::mem::take(&mut self.zone_path);
        let a = self.area;
        let (tdiv, delay) = self.view();
        let ch = &self.m.ch[self.m.zones[k].src];
        let ppx = a.w as f64 / HDIV as f64;
        let to_wave = |(x, y): (i32, i32)| {
            let t = (x - (a.x + a.w / 2)) as f64 / ppx * tdiv as f64 + delay as f64;
            let v = self.zoom_divs_at(a, (y - a.y) as f32 / a.h as f32) as f64 * ch.volts_per_div() as f64 - ch.offset as f64;
            (t, v)
        };
        let ok = if shape { path.len() >= 3 } else { path.len() == 2 && (path[0].0 - path[1].0).abs() > 4 && (path[0].1 - path[1].1).abs() > 4 };
        if !ok {
            self.toast("Zone too small");
            return;
        }
        let points = path.into_iter().map(to_wave).collect();
        let z = &mut self.m.zones[k];
        z.pts = points;
        z.on = true;
        self.toast(&format!("Zone {} set", k + 1));
    }

    /// Zones (green = must intersect, red = must not) and the zone being drawn.
    fn draw_zones(&self, cv: &mut Canvas) {
        let a = self.area;
        let (tdiv, delay) = self.view();
        let ppx = a.w as f64 / HDIV as f64;
        cv.set_clip(a);
        for (k, z) in self.m.zones.iter().enumerate() {
            if z.pts.len() < 2 || !self.shows(z.src) {
                continue;
            }
            let ch = &self.m.ch[z.src];
            let px = |(t, v): (f64, f64)| {
                let x = a.x as f64 + a.w as f64 / 2.0 + (t - delay as f64) / tdiv as f64 * ppx;
                let y = self.zoom_y_of_divs(a, ((v + ch.offset as f64) / ch.volts_per_div() as f64) as f32) as f64;
                (x.clamp(-1e4, 1e4).round() as i32, y.clamp(-1e4, 1e4).round() as i32)
            };
            let poly: Vec<(i32, i32)> = z.polygon().into_iter().map(px).collect();
            let col = if !z.on { DIM } else if z.inside { GREEN } else { RED };
            for i in 0..poly.len() {
                let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
                cv.line(p.0, p.1, q.0, q.1, col);
                cv.line(p.0 + 1, p.1, q.0 + 1, q.1, col);
            }
            let (lx, ly) = poly.iter().fold((i32::MAX, i32::MAX), |(x, y), p| (x.min(p.0), y.min(p.1)));
            cv.text(&self.fonts().s, lx + 4, ly + 14, &format!("Z{}", k + 1), col);
        }
        // the path being drawn is in screen coordinates: once, over the whole plot
        if self.cur_win != 0 {
            cv.no_clip();
            return;
        }
        cv.set_clip(self.grid);
        if self.zone_arm.map_or(false, |a| a.1) {
            for w in self.zone_path.windows(2) {
                cv.line(w[0].0, w[0].1, w[1].0, w[1].1, ACCENT);
            }
        }
        if self.zone_arm.map_or(false, |a| !a.1) && self.zone_path.len() == 2 {
            let ((x0, y0), (x1, y1)) = (self.zone_path[0], self.zone_path[1]);
            for (p, q) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
                cv.line(p.0, p.1, q.0, q.1, ACCENT);
            }
        }
        cv.no_clip();
    }

    pub fn tap(&mut self, x: i32, y: i32) {
        if self.numpad_tap(x, y) { return; }
        if self.load_slot_tap(x, y) { return; }
        let box_button = (self.m.layout != Layout::Studio && self.m.zoom && self.zoom_controls().iter().any(|(rect, text)| *text == "Box" && rect.contains(x, y)))
            || (self.m.layout == Layout::Studio && self.studio_zoom_bar().is_some_and(|b| b.boxr.contains(x, self.zoom_row_y(x, y))));
        if !box_button { self.zoom_arm = false; }
        if self.workspace_popup_tap(x, y) { return; }
        if self.single_btn().contains(x, y) { self.key(key::SINGLE); return; }
        if self.m.layout == Layout::Studio && self.full_on() && self.full_chrome_tap(x, y) { return; }
        if self.m.layout == Layout::Studio && !self.full_on() && self.studio_tap(x, y) { return; }
        if self.m.layout == Layout::Minimal && !self.full_on() && self.workspace_footer_tap(x, self.bar_y(y)) { return; }
        if self.power_dlg {
            self.power_dlg = false;
            let (_, [off, restart, _]) = self.pwr_layout();
            if off.contains(x, y) {
                self.power_request(PowerAction::Off);
            } else if restart.contains(x, y) {
                self.power_request(PowerAction::Restart);
            }
            return;
        }
        if self.wlan.osk.is_some() {
            return self.osk_tap(x, y);
        }
        if self.dialog.is_some() {
            self.dialog = None;
            return;
        }
        if self.tap_menu(x, y) {
            return;
        }
        if self.waves_board_tap(x, y) {
            return;
        }
        if let Some(r) = self.live_ref_fit_rect() {
            if r.contains(x, y) {
                self.fit_shown_reference();
                return;
            }
        }
        if !self.menu_open && self.measurement_navigation_tap(x, y) { return; }
        if self.auto_bar_tap(x, y) {
            return;
        }
        if self.full_btn().1.contains(x, y) {
            self.toggle_expand();
            return;
        }
        if !self.menu_open && self.eye_open() && self.eye_pane_rect().contains(x,y) {
            if self.eye_tap(x,y) || self.eye_column_tap(x,y) { return; }
            let a=self.eye_pane_rect();
            if y<a.y+workspace::HEADER { self.eye_header_tap(x,y); }
            return;
        }
        if self.zoom_btn().1.contains(x, y) && !self.view_on() {
            self.set_zoom(!self.m.zoom);
            return;
        }
        if self.zoom_close_tap(x, y) { return; }
        if (self.view_on() || self.view_tile_rect().is_some()) && self.view_btn().contains(x, y) {
            return self.toggle_menu(MenuId::ViewMore);
        }
        if self.view_fix_tap(x, y) { return; }
        if !self.menu_open && self.m.view == AView::Eye && !self.xy_over() && (self.eye_tap(x, y) || self.eye_column_tap(x, y)) {
            return;
        }
        if self.table_tap(x, y) || self.pane_tap(x, y) {
            return;
        }
        if self.eye_tap(x, y) {
            return;
        }
        if self.full_on() {
            // no top bar and no cards: only what is over the plot
            if let Some((h, _)) = self.results_rects() {
                if Rect::new(h.x, h.y - 5, h.w, h.h + 10).contains(x, y) {
                    self.results_min = !self.results_min;
                    return;
                }
            }
            if self.menu_open && self.grid.contains(x, y) {
                self.close_menu();
            }
            return;
        }
        let pill = self.run_pill();
        if self.top_hit(pill).contains(x, y) {
            return self.key(key::RUNSTOP);
        }
        if self.top_hit(self.net_box()).contains(x, y) {
            return self.toggle_menu(MenuId::Network);
        }
        if self.wlan_stick && self.top_hit(self.wlan_box()).contains(x, y) {
            return self.toggle_menu(MenuId::Network);
        }
        if self.usb && self.top_hit(self.usb_box()).contains(x, y) {
            return self.toggle_menu(MenuId::Storage);
        }
        if self.view_box_tap(x, y) {
            return;
        }
        // sample rate, depth and mode in the top bar: the page that sets them
        let end = self.view_box().map_or(self.top_buttons().first().map_or(self.top_right(), |b| b.2.x), |v| v.x) - 4;
        if self.top_hit(Rect::new(pill.x + pill.w, pill.y, end - pill.x - pill.w, pill.h)).contains(x, y) {
            return self.toggle_menu(if self.record_mode() { MenuId::Ref } else { MenuId::Acquire });
        }
        for (b, _, r) in self.top_buttons() {
            if self.top_hit(r).contains(x, y) {
                return match b {
                    // a tap opens the measurement list; a press-and-hold opens the short menu (meas_hold_tick)
                    TopBtn::Menu(MenuId::Measure) => self.open_meas_list(),
                    TopBtn::Menu(id) => self.toggle_menu(id),
                    TopBtn::Live => self.return_to_live(),
                    TopBtn::System => self.toggle_menu(MenuId::Utility),
                    TopBtn::Load => self.request_waveform(),
                };
            }
        }
        if self.menu_btn().contains(x, y) {
            return self.toggle_menu(MenuId::Main);
        }
        // header of the results overlay (40 px touch area): fold / unfold
        if let Some((h, _)) = self.results_rects() {
            if Rect::new(h.x, h.y - 5, h.w, h.h + 10).contains(x, y) {
                self.results_min = !self.results_min;
                return;
            }
        }
        let (chs, tr, trg) = self.cards();
        for (c, r) in chs.iter().enumerate() {
            if r.contains(x, y) {
                if !self.m.ch[c].on {
                    self.m.ch[c].on = true;
                    self.settings_changed();
                } else if self.m.n_on() > 1 && self.m.active != c {
                    // as the channel key: with several channels on, the first tap only selects; the next opens the
                    // menu. Another channel's menu would show the wrong channel: it closes.
                    self.m.active = c;
                    if self.menu_open && matches!(self.menu, MenuId::Channel(_)) {
                        self.close_menu();
                    }
                    return self.select_workspace_source(WaveId::Ch(c));
                }
                self.m.active = c;
                return self.toggle_menu(MenuId::Channel(c));
            }
        }
        let (la, gen) = self.aux_cards();
        if la.is_some_and(|r| r.contains(x, y)) {
            // like a channel: an LA that is off comes on with its page
            if self.m.la.on {
                return self.toggle_menu(MenuId::La);
            }
            return self.la_key(); // says so when there is no probe
        }
        if gen.is_some_and(|r| r.contains(x, y)) {
            // only the page: an output does not come on by touching a card
            return self.toggle_menu(MenuId::Afg);
        }
        if self.card_layout().refs.is_some_and(|r| r.contains(x, y)) { return self.cycle_active_ref(); }
        if tr.contains(x, y) {
            return self.toggle_menu(MenuId::Horizontal);
        }
        if self.record_mode() {
            for (slot, rect) in self.record_cards() {
                if rect.contains(x, y) {
                    self.ref_sel = slot;
                    self.offline_ref = slot;
                    self.meas_src = WaveId::Ref(slot);
                    if x >= rect.x + rect.w - 42 && y < rect.y + 28 {
                        if let Some(reference) = self.refs[slot].as_mut() { reference.on = !reference.on; }
                        return;
                    }
                    return self.toggle_menu(MenuId::Ref);
                }
            }
        }
        if trg.contains(x, y) {
            if self.record_mode() {
                let (run, live) = Self::session_buttons(trg);
                if run.contains(x, y) { self.key(key::RUNSTOP); }
                else if live.contains(x, y) { self.return_to_live(); }
                else { self.toggle_menu(MenuId::Trigger); }
                return;
            }
            // the Source and Mode buttons are what is drawn as buttons; the rest of the card opens the trigger page
            let (sb, mb) = Self::trig_parts(trg);
            let near = |b: Rect| Rect::new(b.x - 2, b.y - 2, b.w + 4, b.h + 4).contains(x, y);
            if near(sb) { return self.adjust(Item::TrigSrc, 1); }
            if near(mb) { return self.adjust(Item::TrigMode, 1); }
            return self.toggle_menu(MenuId::Trigger);
        }
        if self.plot_tap(x, y) { return; }
        if self.menu_open && self.grid.contains(x, y) {
            self.close_menu();
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
    /// the REF card of the Default bottom row: only while a ref is shown over the live channels
    refs: Option<Rect>,
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
    (0..8).flat_map(|k| [y + 36 * k, y - 36 * k]).map(|c| c.clamp(lo, hi))
        .find(|c| taken.iter().all(|t| (t - c).abs() >= 36)).unwrap_or(y.clamp(lo, hi))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Text on a dark backing, for labels drawn over the waveforms; returns the x after the text.
/// `fine_time`: next fine time base from `now`, by `d` hundredths on three digits, as (step, factor). The step is the
/// one at or above the value; from the roll time base on the screen is the record, so there only the steps remain.
fn fine_time(now: f32, d: i32) -> (usize, f32) {
    let ts = tscales();
    let raw = now as f64 * (1.0 + 0.01 * d as f64);
    let q = 10f64.powf(raw.max(1e-12).log10().floor() - 2.0);
    let want = (((raw / q).round() * q) as f32).clamp(ts[0], ts[ts.len() - 1]);
    let i = ts.iter().position(|&t| t >= want * 0.99999).unwrap_or(ts.len() - 1);
    if ts[i] as f64 >= board().roll_tdiv() * 0.999 {
        let at = ts.iter().position(|&t| t >= now * 0.99999).unwrap_or(ts.len() - 1) as i32;
        return ((at + d.signum()).clamp(0, ts.len() as i32 - 1) as usize, 1.0);
    }
    let f = want / ts[i];
    (i, if (f - 1.0).abs() < 1e-5 { 1.0 } else { f })
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

/// `s` centred in `r` in the first of `fonts` that fits; a text too long for the last one goes on two lines of it.
fn fitted_text(cv: &mut Canvas, fonts: &[&'static Font], r: Rect, s: &str, c: Rgb) {
    let room = r.w - 12;
    let f = fit_font(fonts, room, s);
    if f.width(s) <= room {
        return cv.text_in(f, r, 1, s, c);
    }
    let words: Vec<&str> = s.split(' ').collect();
    let first = (1..words.len()).rev().find(|&k| f.width(&words[..k].join(" ")) <= room).unwrap_or(1);
    let (a, b) = (words[..first].join(" "), words[first..].join(" "));
    let outer = cv.save_clip();
    cv.clip_within(r);
    cv.text_fit(f, Rect::new(r.x + 4, r.y, r.w - 8, r.h / 2 + 1), 1, &a, c);
    cv.text_fit(f, Rect::new(r.x + 4, r.y + r.h / 2 - 1, r.w - 8, r.h / 2 + 1), 1, &b, c);
    cv.restore_clip(outer);
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
                Kind::Addr { read } => format!("{} {}{}", hex(p.value), if read { "R" } else { "W" }, if p.ok { "" } else { " N" }),
                Kind::Id { ext, rtr } => format!("ID {}{}{}", if ext { format!("{:08X}", p.value) } else if m.dec_proto == Proto::LIN { format!("{:02X}", p.value) } else { format!("{:03X}", p.value) }, if rtr { " RTR" } else { "" }, if p.ok { "" } else { " !P" }),
                Kind::Sample { right } => format!("{} {:0w$X}", if right { "R" } else { "L" }, p.value, w = (m.dec_bits as usize).div_ceil(4)),
                Kind::Word { cmd } => format!("{} {:04X}{}", if cmd { "C/S" } else { "D" }, p.value, if p.ok { "" } else { " !P" }),
                Kind::Crc => format!("{} {:X}{}", if m.dec_proto == Proto::LIN { "CS" } else { "CRC" }, p.value, if p.ok { "" } else { " !" }),
                Kind::Field(l) if matches!(l, "MAC" | "SRC") && p.value2.is_some() => {
                    format!("{} {}{}", if l == "SRC" { "SRC" } else { "MAC" }, oos_dsp::linecode::mac_text(p.value, p.value2.unwrap_or(0)), if p.ok { "" } else { " !" })
                }
                Kind::Field(l) if matches!(l, "IP" | "DIP") => format!("{} {}{}", if l == "DIP" { "DIP" } else { "IP" }, oos_dsp::linecode::ip_text(p.value), if p.ok { "" } else { " !" }),
                Kind::Field(l) => match p.value2 {
                    Some(v2) if matches!(l, "DST" | "SRC" | "SER" | "PID") => format!("{} {:04X}{:08X}{}", l, v2, p.value, if p.ok { "" } else { " !" }),
                    _ => format!("{} {:X}{}", l, p.value, if p.ok { "" } else { " !" }),
                },
                Kind::Ack => format!("ACK {}", p.value),
                _ if m.dec_proto == Proto::UART && m.dec_ascii && p.value == 0x20 => "SP".to_string(),
                _ if m.dec_proto == Proto::UART && m.dec_ascii && (0x21..0x7f).contains(&p.value) => format!("{}", p.value as u8 as char),
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

/// Next holdoff on the 1-2-5 ladder from 100 ns to 10 s; 0 is off.
fn holdoff_step(h: f32, d: i32) -> f32 {
    let mut steps = vec![0.0f32];
    let mut x = 1e-7f32;
    while x < 11.0 {
        steps.extend([x, 2.0 * x, 5.0 * x]);
        x *= 10.0;
    }
    steps.retain(|&s| s <= 10.0);
    let i = steps.iter().position(|&s| s >= h * 0.999).unwrap_or(steps.len() - 1) as i32;
    steps[(i + d).clamp(0, steps.len() as i32 - 1) as usize]
}

/// The largest 1-2-5 step that is not above `x`.
fn nice_step_below(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) { return 1.0; }
    let base = 10f64.powf(x.log10().floor());
    [5.0, 2.0, 1.0].into_iter().map(|m| m * base).find(|&s| s <= x * 1.000001).unwrap_or(base)
}

/// The labelled times of the time axis, as (whole steps from the trigger, seconds): the window is `delay` +- 5 divisions
/// of `tdiv`; a step is `every` divisions. Whole steps, so a window put anywhere has round labels.
fn studio_time_ticks(tdiv: f32, delay: f32, every: i32) -> Vec<(i64, f64)> {
    studio_time_ticks_at(tdiv, delay, tdiv as f64 * every as f64)
}

/// `studio_time_ticks` with the step given (a lane whose time/div is not on the ladder labels round steps).
fn studio_time_ticks_at(tdiv: f32, delay: f32, step: f64) -> Vec<(i64, f64)> {
    let half = tdiv as f64 * HDIV as f64 / 2.0;
    let (left, right) = (delay as f64 - half, delay as f64 + half);
    let first = (left / step - 1e-6).ceil() as i64;
    (first..).map(|k| (k, k as f64 * step)).take_while(|&(_, t)| t <= right + step * 1e-6).collect()
}

/// Axis label with the SI prefix of the axis `span` and as many decimals as `step` and `base` (offset / delay)
/// need: "-1.0 µs", "500 mV", "3.25 V".
fn axis_label(v: f32, step: f32, base: f32, span: f32, unit: &str) -> String {
    axis_label64(v as f64, step, base, span, unit)
}

/// `axis_label` for a time that does not fit an f32 (a deep zoom far from the trigger).
fn axis_label64(v: f64, step: f32, base: f32, span: f32, unit: &str) -> String {
    let prefixes = [(1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "\u{b5}"), (1e-9, "n"), (1e-12, "p")];
    let a = span.abs().max(step.abs()) as f64;
    let (mult, p) = prefixes.iter().copied().find(|(m, _)| a >= *m * 0.9995).unwrap_or((1e-12, "p"));
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
    let dec = needed(st, 9).max(needed((base as f64 / mult).abs(), 3));
    // a step far below one unit of the prefix (a deep zoom) needs digits down to its own size
    let dec = if st > 0.0 && st < 1.0 { dec.max((-st.log10() - 1e-6).ceil().min(9.0) as i32) } else { dec };
    let x = v / mult;
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

fn draw_slope(cv: &mut Canvas, x: i32, y: i32, s: Slope, col: Rgb) {
    draw_slope1(cv, x, y, s, col);
    draw_slope1(cv, x + 1, y, s, col);
}

fn draw_slope1(cv: &mut Canvas, x: i32, y: i32, s: Slope, col: Rgb) {
    // 14 x 16 icon: rising / falling edge
    let (x0, x1, x2, x3) = (x, x + 5, x + 9, x + 14);
    let (ylo, yhi) = (y + 14, y + 2);
    match s {
        Slope::Rise => {
            cv.hline(x0, x1, ylo, col);
            cv.line(x1, ylo, x2, yhi, col);
            cv.hline(x2, x3, yhi, col);
        }
        Slope::Fall => {
            cv.hline(x0, x1, yhi, col);
            cv.line(x1, yhi, x2, ylo, col);
            cv.hline(x2, x3, ylo, col);
        }
        Slope::Either => {
            cv.line(x0, ylo, x0 + 6, yhi, col);
            cv.line(x0 + 7, yhi, x3, ylo, col);
        }
    }
}

/// The fast path's take for the next screen after drawing `n` frames in `ms` with `budget` ms (`Ui::fast_max`).
fn fast_take_next(take: f32, n: usize, ms: f32, budget: f32) -> f32 {
    let est = n as f32 * budget / ms;
    let limited = n >= (take as usize).clamp(4, crate::board::QUEUE_MAX);
    if ms > budget || (limited && est > take) {
        (take + 0.5 * (est - take)).clamp(4.0, crate::board::QUEUE_MAX as f32)
    } else {
        take
    }
}

/// What the fast display path does with one frame (`fast_plan`).
pub enum FastPlan {
    /// sparse views (fewer than 2 samples a column), envelopes, roll and scan: a screen record, drawn as a trace
    /// (made here, so the interpolation is spread over the frames, not the channels)
    Rec(crate::wave::Record),
    /// dense views: the raw samples with the trigger at sample `ti`; `class`: the `Ui::fast_edges` index (None: the
    /// source is an LA line), `off`: how far the edge was from the hardware point (class 2); per channel the columns'
    /// spans (None: dots, drawn sample by sample)
    Raw { ti: f64, class: Option<usize>, off: f64, spans: [Option<crate::fastdraw::Spans>; NCH] },
}

/// One frame placed by `fast_plan`: how to draw it, whether it counts as triggered (false if its trigger edge was not
/// found near the hardware trigger point), and per channel the voltage values of the statistics (where wanted).
pub struct FastFrame {
    pub plan: FastPlan,
    pub triggered: bool,
    pub fold: [Option<crate::fastdraw::Fold>; NCH],
}

/// Where channel `c` of a dense frame lands on the `w` x `h` map with the trigger at sample `ti`.
fn raw_map(m: &Model, w: usize, h: usize, c: usize, f: &crate::board::Frame, ti: f64) -> crate::fastdraw::Map {
    let ch = &m.ch[c];
    let tdiv = m.time_per_div() as f64;
    let x_per_sample = f.dt / tdiv * w as f64 / HDIV as f64;
    let x0 = w as f64 / 2.0 - m.delay as f64 / tdiv * w as f64 / HDIV as f64;
    let ppd_y = h as f32 / VDIV;
    let vpd = ch.volts_per_div();
    crate::fastdraw::Map { ti, x_per_sample, x0, y_per_volt: -ppd_y / vpd, y0: h as f32 / 2.0 - ch.offset / vpd * ppd_y }
}

/// One frame of the fast path placed on the screen (`w` x `h` map): the trigger, the columns of a dense frame or the
/// screen record of a sparse one, and the statistics of the channels in `fold`. Touches nothing but the frame, so the
/// frames of a batch are placed on several cores; this is where most of the work is.
pub fn fast_plan(m: &Model, w: usize, h: usize, f: &std::sync::Arc<crate::board::Frame>, fold: [bool; NCH]) -> FastFrame {
    let tdiv = m.time_per_div() as f64;
    let window = tdiv * HDIV as f64;
    let t_first = m.delay as f64 - window / 2.0;
    let x_per_sample = f.dt / tdiv * w as f64 / HDIV as f64;
    if x_per_sample > 0.5 || f.envelope || f.roll || f.scan {
        // the record's 3600 points: fewer thin out the picture of a fast signal (at 8 samples a period, two points a
        // column lit a fifth fewer pixels than 3600, and those a sixth fewer than a nearly continuous curve)
        let rec = crate::board::to_record(f, m, 3600, window, t_first);
        // the statistics on the screen record, as the measurements of a sparse record
        let fold = std::array::from_fn(|c| (fold[c] && m.ch[c].on).then(|| crate::fastdraw::Fold::of(&rec.ch[c])));
        let triggered = rec.triggered;
        return FastFrame { plan: FastPlan::Rec(rec), triggered, fold };
    }
    let src = &f.ch[m.trig_src];
    // an LA line as the source: the hardware trigger point stands, the analog samples cannot refine it
    let on_la = m.trig_on_la();
    // the edge of the set direction nearest the hardware trigger point, up to half a screen away, on the signal smoothed
    // to one screen column: at decimated rates the hardware point lands near the level on either edge (trigger.md 8)
    let hyst = m.trig_hyst_div() * m.ch[m.trig_src].volts_per_div();
    let (rising, either) = (m.slope != Slope::Fall, m.slope == Slope::Either);
    let half = (window / 2.0 / f.dt) as usize;
    let col = (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize;
    let (edge, class, off) = if on_la {
        (None, None, 0.0)
    } else {
        let (e, any) = crate::fastdraw::edge_smooth(src, f.pre, crate::board::sw_trig_level(m, f), hyst, rising, either, half, col, |_| true);
        let (k, d) = match (e, any) {
            (None, _) => (3, 0.0),
            (Some(t), _) if (t - f.pre as f64).abs() <= (col + 16) as f64 => (0, 0.0),
            (Some(_), Some(n)) if !either && n.rising != rising => (1, 0.0),
            (Some(t), _) => (2, (t - f.pre as f64).abs()),
        };
        (e, Some(k), d)
    };
    let ti = edge.unwrap_or(f.pre as f64);
    let used = |c: usize| m.ch[c].on && !f.ch[c].is_empty();
    let spans = std::array::from_fn(|c| (used(c) && !m.dots).then(|| crate::fastdraw::spans(w, h, &f.ch[c], &raw_map(m, w, h, c, f, ti))));
    // the statistics on the samples of the screen window, as the measurements of a dense record
    let fold = std::array::from_fn(|c| {
        (fold[c] && used(c)).then(|| {
            let v = &f.ch[c];
            let i0 = ((ti + t_first / f.dt).floor().max(0.0) as usize).min(v.len());
            let i1 = ((ti + (t_first + window) / f.dt).ceil().max(0.0) as usize).min(v.len());
            crate::fastdraw::Fold::of(&v[i0..i1.max(i0)])
        })
    });
    // a filtered trigger path (coupling): the hardware point stands where the samples show no crossing
    let triggered = edge.is_some() || on_la || m.trig_coupling() != TrigCoup::Dc;
    FastFrame { plan: FastPlan::Raw { ti, class, off, spans }, triggered, fold }
}

/// The rows (first, last + 1) the points `v` of channel `c` of a screen record fall on, within a map `h` high.
fn rec_rows(m: &Model, h: usize, c: usize, v: &[f32]) -> (usize, usize) {
    let ch = &m.ch[c];
    let (vpd, ppd_y) = (ch.volts_per_div(), h as f32 / VDIV);
    let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
    if lo > hi {
        return (usize::MAX, 0);
    }
    let y = |val: f32| h as f32 / 2.0 - (val + ch.offset) / vpd * ppd_y;
    // rounded as `wave::trace_columns`, one row of slack each side
    let (a, b) = (y(hi).round() - 1.0, y(lo).round() + 1.0);
    (a.max(0.0) as usize, (b.max(-1.0) + 1.0).min(h as f32) as usize)
}

/// Channel `c` of a placed frame into a band of its collected differences (`fastdraw::diff_span`): the columns
/// `c_lo ..` of a map `w` x `h`, `h + 1` rows of them.
#[allow(clippy::too_many_arguments)]
pub fn fast_deposit_t(m: &Model, w: usize, h: usize, c: usize, band: &mut [u16], c_lo: usize, f: &crate::board::Frame, p: &FastPlan) {
    if !m.ch[c].on {
        return;
    }
    match p {
        FastPlan::Rec(rec) => {
            if !rec.ch[c].is_empty() {
                let bw = band.len() / (h + 1);
                let c_hi = (c_lo + bw) as i32;
                crate::wave::trace_columns(w, h, c, m, (m.time_per_div(), m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None, rec.pairs, (c_lo as i32, c_hi), |cx, r0, r1, wt| {
                    crate::fastdraw::diff_span(band, bw, cx as usize - c_lo, r0 as usize, r1 as usize, wt);
                });
            }
        }
        FastPlan::Raw { ti, spans, .. } => match &spans[c] {
            Some(s) => crate::fastdraw::add_spans_t(band, h, c_lo, s, 24),
            None if !f.ch[c].is_empty() => crate::fastdraw::deposit_dots_t(band, h, c_lo, &f.ch[c], &raw_map(m, w, h, c, f, *ti), 24),
            None => {}
        },
    }
}

/// Channel `c` of a placed frame into the rows `r_lo` .. `r_lo + band.len() / w` of that channel's map (`w` x `h`),
/// held in `band` (the whole map: `r_lo` 0): one frame after the other straight into the row-major map, the tests'
/// reference for the fast path's drawing.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn fast_deposit(m: &Model, w: usize, h: usize, c: usize, band: &mut [u16], r_lo: usize, f: &crate::board::Frame, p: &FastPlan) {
    if !m.ch[c].on {
        return;
    }
    match p {
        FastPlan::Rec(rec) => {
            if !rec.ch[c].is_empty() {
                crate::wave::draw_trace(band, w, h, r_lo, c, m, (m.time_per_div(), m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None, rec.pairs);
            }
        }
        FastPlan::Raw { ti, spans, .. } => match &spans[c] {
            Some(s) => crate::fastdraw::add_spans(band, w, r_lo, s, 24),
            None if !f.ch[c].is_empty() => crate::fastdraw::deposit_dots_band(band, w, r_lo, h, &f.ch[c], &raw_map(m, w, h, c, f, *ti), 24),
            None => {}
        },
    }
}

/// The count a trace pixel is graded against (full brightness), from the brightest pixel `mx` of the picture. It
/// follows a higher `mx` at once and a lower one slowly (`prev`, no flicker). Minimum persistence (`frames` > 0, the
/// frames in the picture): the picture holds as many frames as arrived since the last screen refresh, in batches, so
/// its counts rise and fall with that number. Graded against the per-frame maximum (`per_frame`, followed the same
/// way) times `frames`, equal frames give equal brightness whatever their number. Without this a refresh with one
/// frame after one with several came out dim: the trace pumped at the batch rate (docs/fork/trigger-lockin.md).
fn picture_ref(prev: u32, per_frame: &mut u32, mx: u32, frames: u32) -> u32 {
    let follow = |v: u32, mx: u32| if mx >= v { mx } else { (v * 7 + mx) / 8 }.max(1);
    if frames == 0 {
        return follow(prev, mx);
    }
    *per_frame = follow(*per_frame, mx.div_ceil(frames));
    per_frame.saturating_mul(frames).max(1)
}

#[cfg(test)]
mod tests {
    use super::axis_label;

    // Break it catches (unit 2026-11-13, 1 ms/div 10 M, meas.deep_raw = on): the background measurement measuring the
    // newest record when the worker gets free. Raw deep records come once a second between pair records (held), so it
    // measured a pair record: Mean stayed NaN live. The raw record waiting for the worker is the one measured.
    #[test]
    fn live_raw_deep_records_are_measured_while_the_worker_is_busy() {
        for tdiv in [1e-3, 2e-3, 5e-3] {
            live_raw_at(tdiv);
        }
    }

    fn live_raw_at(tdiv: f64) {
        use super::*;
        crate::deepmeas::force(Some(true));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.measure_in_background();
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - tdiv).abs() < tdiv * 1e-6).unwrap();
        ui.m.mdepth = mdepths().iter().position(|d| d.1 == 10_000_000).unwrap();
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame { seq, dt: window * 1.2 / n as f64, pre: n / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], envelope, acq: (1e7 / (tdiv * 16.0), 10_000_000), ..Default::default() };
        let raw = |high: f32| -> Vec<f32> { (0..n).map(|i| if (i / 50) % 4 == 0 { high } else { 0.0 }).collect() };
        let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 1.0 }).collect();
        let mut seq = 0;
        for round in 0..3 {
            // a new value each round: a measurement that stays at the last one is caught too
            let high = 2.0 + round as f32;
            // the worker is still on the previous record when the raw one comes, and pair records follow
            ui.meas_bg.as_mut().unwrap().busy = true;
            seq += 1;
            shared.lock().unwrap().queue.push_back(frame(seq, false, raw(high)));
            // one batch of one frame each at 2 and 5 ms/div: the next pair record is queued before the UI looks
            if tdiv > 1.5e-3 {
                seq += 1;
                shared.lock().unwrap().queue.push_back(frame(seq, true, pairs.clone()));
            }
            ui.step(0.02);
            for _ in 0..3 {
                seq += 1;
                shared.lock().unwrap().queue.push_back(frame(seq, true, pairs.clone()));
                ui.step(0.02);
            }
            ui.meas_bg.as_mut().unwrap().busy = false;
            for _ in 0..50 {
                ui.step(0.02);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let avg = ui.meas(0, Meas::Vavg);
            assert!((avg - high as f64 / 4.0).abs() < 0.03, "Mean live at {} s/div, round {}: {}", tdiv, round, avg);
        }
        crate::deepmeas::force(None);
    }

    // Break it catches: Clear keeping the raw deep record that waits for the busy worker. The next record asked for a
    // measurement and the worker took the record cleared away: its values were kept and shown for the pair records
    // after the Clear (held, deepmeas.rs).
    #[test]
    fn clear_drops_the_record_waiting_for_the_worker() {
        use super::*;
        crate::deepmeas::force(Some(true));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.measure_in_background();
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        let frame = |seq: u64, envelope: bool, depth: usize, high: f32| {
            let v: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { high } else { 0.0 }).collect();
            let dt = window * 1.2 / n as f64;
            let ch = [v, Vec::new(), Vec::new(), Vec::new()];
            crate::board::Frame { seq, dt, pre: n / 2, ch, envelope, acq: (1e9, depth), ..Default::default() }
        };
        // steps until the worker has answered what the record asked for (however slow the computer)
        let wait = |ui: &mut Ui| {
            let t = std::time::Instant::now();
            loop {
                ui.step(0.02);
                if (!ui.meas_bg.as_ref().unwrap().busy && !ui.meas_want) || t.elapsed().as_secs() > 20 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        // a raw deep record waits for the busy worker
        ui.meas_bg.as_mut().unwrap().busy = true;
        shared.lock().unwrap().queue.push_back(frame(1, false, 10_000_000, 2.0));
        ui.step(0.02);
        assert!(ui.meas_pending.is_some());
        ui.clear_display();
        // the worker is free and a shallow record comes: it is the one measured
        ui.meas_bg.as_mut().unwrap().busy = false;
        shared.lock().unwrap().queue.push_back(frame(2, false, 100_000, 4.0));
        wait(&mut ui);
        // a deep pair record: its measurement stays that of the last record measured
        shared.lock().unwrap().queue.push_back(frame(3, true, 10_000_000, 1.0));
        wait(&mut ui);
        let avg = ui.meas(0, Meas::Vavg);
        assert!((avg - 1.0).abs() < 0.03, "Mean held after the Clear: {}", avg);
        crate::deepmeas::force(None);
    }

    // Break it catches: Mean, RMS and Std dev shown from min/max pairs (a deep record read through the FPGA
    // compression), or Max/Min/Pk-pk lost with them
    #[test]
    fn measurements_on_min_max_pairs_leave_out_the_mean_type() {
        use super::*;
        // without the raw reads of deepmeas.rs (meas.deep_raw = off)
        crate::deepmeas::force(Some(false));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        let v: Vec<f32> = (0..n).map(|i| if (i / 2) % 50 < 10 { 1.0 } else if i % 2 == 0 { -1.0 } else { -0.98 }).collect();
        for (seq, envelope) in [(1u64, true), (2, false), (3, true)] {
            // a deep record (above 1 M points): pairs of the live compression are held instead (compress::hold)
            shared.lock().unwrap().queue.push_back(crate::board::Frame { seq, dt: window * 1.2 / n as f64, pre: n / 2, ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()], envelope, acq: (1e9, 10_000_000), ..Default::default() });
            for _ in 0..3 {
                ui.step(0.02);
            }
            let (avg, rms, max, pp) = (ui.meas(0, Meas::Vavg), ui.meas(0, Meas::Vrms), ui.meas(0, Meas::Vmax), ui.meas(0, Meas::Vpp));
            assert_eq!(avg.is_nan() && rms.is_nan(), envelope, "envelope {}: mean {} rms {}", envelope, avg, rms);
            assert!((max - 1.0).abs() < 0.05 && (pp - 2.0).abs() < 0.05, "envelope {}: max {} pk-pk {}", envelope, max, pp);
            let row = ui.meas_rows().into_iter().find(|r| r.name == Meas::Vavg.name()).map(|r| r.v);
            assert!(row.is_none_or(|x| x.is_nan() == envelope));
        }
        crate::deepmeas::force(None);
    }

    // Break it catches (compress.rs, raw newest frame every RAW_PERIOD): live-compression pair records measured between
    // two raw ones (mean-type NaN flicker, statistics counting pairs), a raw record not measured, a STOP that keeps a
    // pair record instead of asking for the final raw frame
    #[test]
    fn live_pair_records_keep_the_raw_measurement_and_stop_takes_a_raw_frame() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0)), (Meas::Vmax, WaveId::Ch(0))];
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame { seq, dt: window * 1.2 / n as f64, pre: n / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], envelope, acq: (625e6, 100_000), ..Default::default() };
        let raw: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { 1.0 } else { 0.0 }).collect();
        let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 1.5 }).collect();
        let steps = |ui: &mut Ui, k: usize| {
            for _ in 0..k {
                ui.step(0.02);
            }
        };
        shared.lock().unwrap().queue.push_back(frame(1, false, raw.clone()));
        steps(&mut ui, 2);
        let avg = ui.meas(0, Meas::Vavg);
        assert!((avg - 0.25).abs() < 0.02, "raw mean {}", avg);
        let counted = ui.mstat.first().map_or(0, |s| s.n);
        for seq in 2..5 {
            shared.lock().unwrap().queue.push_back(frame(seq, true, pairs.clone()));
            steps(&mut ui, 2);
            assert_eq!(ui.meas(0, Meas::Vavg), avg);
            assert!((ui.meas(0, Meas::Vmax) - 1.0).abs() < 0.02);
        }
        assert_eq!(ui.mstat.first().map_or(0, |s| s.n), counted);
        // STOP on a pair record: the final raw frame is asked for and shown
        ui.m.run = RunState::Stop;
        steps(&mut ui, 1);
        assert!(ui.stop_final.is_some() && shared.lock().unwrap().final_req == 1);
        {
            let mut g = shared.lock().unwrap();
            g.queue.push_back(frame(5, false, raw.iter().map(|x| 2.0 * x).collect()));
            g.final_ack = 1;
        }
        steps(&mut ui, 2);
        assert!(ui.stop_final.is_none());
        assert_eq!(ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope)), Some((5, false)));
        assert!((ui.meas(0, Meas::Vavg) - 0.5).abs() < 0.03);
    }

    // Break it catches: the protocol watch or XY reading pair records (the hardware thread is not told)
    #[test]
    fn protocol_watch_and_xy_ask_for_raw_frames() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.step(0.02);
        assert!(!shared.lock().unwrap().needs_raw);
        ui.azr.watch = true;
        ui.step(0.02);
        assert!(shared.lock().unwrap().needs_raw);
        (ui.azr.watch, ui.xy) = (false, true);
        ui.step(0.02);
        assert!(shared.lock().unwrap().needs_raw);
        ui.xy = false;
        ui.step(0.02);
        assert!(!shared.lock().unwrap().needs_raw);
    }

    // Break it catches (docs/fork/stop-exact.md): a STOP that keeps the pair record on screen while the measurements
    // show an older raw record, a final frame not taken or taken after the stop, frames after it taken, the hardware
    // left arming in STOP or held in RUN, a STOP waiting forever, a plain STOP of a shallow record waiting at all
    #[test]
    fn stop_shows_the_final_raw_frame_of_a_deep_record() {
        use super::*;
        crate::deepmeas::force(Some(true));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0))];
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        let frame = |seq: u64, envelope: bool, acq: usize, v: Vec<f32>| crate::board::Frame { seq, dt: window * 1.2 / n as f64, pre: n / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], envelope, acq: (1e9, acq), ..Default::default() };
        let raw = |top: f32| -> Vec<f32> { (0..n).map(|i| if (i / 50) % 4 == 0 { top } else { 0.0 }).collect() };
        let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 1.0 }).collect();
        let steps = |ui: &mut Ui, k: usize| {
            for _ in 0..k {
                ui.step(0.02);
            }
        };
        shared.lock().unwrap().queue.push_back(frame(1, true, 10_000_000, pairs.clone()));
        steps(&mut ui, 3);
        assert!(!shared.lock().unwrap().hold);
        // STOP on a pair record: the thread is held and asked for the final frame, the UI waits for it
        ui.m.run = RunState::Stop;
        steps(&mut ui, 2);
        {
            let g = shared.lock().unwrap();
            assert!(g.hold && g.final_req == 1);
        }
        assert!(ui.stop_final.is_some() && ui.last[0].as_ref().is_some_and(|r| r.raw.as_ref().is_some_and(|x| x.frame.envelope)));
        // the thread: the last frame of the batch in progress, then the newest frame raw, then the answer
        {
            let mut g = shared.lock().unwrap();
            g.queue.push_back(frame(2, true, 10_000_000, pairs.clone()));
            g.queue.push_back(frame(3, false, 10_000_000, raw(2.0)));
            g.final_ack = 1;
        }
        steps(&mut ui, 2);
        assert!(ui.stop_final.is_none());
        let shown = ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope));
        assert_eq!(shown, Some((3, false)), "the final raw frame is the record shown");
        let avg = ui.meas(0, Meas::Vavg);
        assert!((avg - 0.5).abs() < 0.03, "mean of the record shown: {}", avg);
        assert_eq!(ui.m.status, TrigStatus::Stop);
        // frames after it stay unseen in STOP
        shared.lock().unwrap().queue.push_back(frame(4, false, 10_000_000, raw(4.0)));
        steps(&mut ui, 2);
        assert_eq!(ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq), Some(3));
        assert!(shared.lock().unwrap().hold);
        // RUN: armed again
        ui.m.run = RunState::Run;
        steps(&mut ui, 1);
        assert!(!shared.lock().unwrap().hold);
        // no answer: the STOP ends after STOP_FINAL_WAIT anyway
        shared.lock().unwrap().queue.clear();
        shared.lock().unwrap().queue.push_back(frame(5, true, 10_000_000, pairs.clone()));
        steps(&mut ui, 2);
        ui.m.run = RunState::Stop;
        steps(&mut ui, 2);
        assert!(ui.stop_final.is_some());
        steps(&mut ui, (STOP_FINAL_WAIT / 0.02) as usize + 2);
        assert!(ui.stop_final.is_none() && ui.m.status == TrigStatus::Stop);
        // a shallow record: STOP holds the thread at once, without a final frame
        ui.m.run = RunState::Run;
        steps(&mut ui, 1);
        shared.lock().unwrap().queue.push_back(frame(6, false, 100_000, raw(1.0)));
        steps(&mut ui, 2);
        ui.m.run = RunState::Stop;
        steps(&mut ui, 1);
        assert!(ui.stop_final.is_none() && shared.lock().unwrap().hold && shared.lock().unwrap().final_req == 2);
        crate::deepmeas::force(None);
    }

    // Break it catches (docs/fork/stop-exact.md): after the final raw frame of a deep record the view read again in
    // STOP (`stop_replay`, min/max pairs of the whole record) replaced the record measured, so Mean, RMS, Std dev and
    // AC RMS went to NaN one step after the STOP; a view counted in the statistics; a raw window read for a view far
    // zoomed in not measured; the raw record of the STOP not measured again once the view is pairs again
    #[test]
    fn stop_measures_the_final_raw_frame_while_the_views_are_drawn_from_pairs() {
        use super::*;
        crate::deepmeas::force(Some(true));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        let ch1 = WaveId::Ch(0);
        let kinds = [Meas::Vavg, Meas::Vrms, Meas::StdDev, Meas::VrmsAc, Meas::Vmax];
        ui.m.meas = kinds.iter().map(|&k| (k, ch1)).collect();
        ui.m.meas2.clear();
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            envelope,
            acq: (1e9, 10_000_000),
            ..Default::default()
        };
        // what the thread answers for a view in STOP: the stored record read again, trigger at the point given
        let view = |seq: u64, envelope: bool, v: Vec<f32>| {
            crate::board::Frame { fixed_ti: Some((n / 2) as f64), ..frame(seq, envelope, v) }
        };
        // a square of a quarter duty, 2 V high: mean 0.5, RMS 1, std dev and AC RMS sqrt(0.75)
        let raw: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { 2.0 } else { 0.0 }).collect();
        let pairs: Vec<f32> = (0..n).map(|i| if i % 2 == 0 { 0.0 } else { 2.0 }).collect();
        let steps = |ui: &mut Ui, k: usize| {
            for _ in 0..k {
                ui.step(0.02);
            }
        };
        let check = |ui: &Ui, what: &str| {
            let sd = 0.75f64.sqrt();
            for (k, want) in kinds.into_iter().zip([0.5, 1.0, sd, sd, 2.0]) {
                let v = ui.meas(0, k);
                assert!((v - want).abs() < 0.03, "{}: {:?} {} instead of {}", what, k, v, want);
            }
        };
        shared.lock().unwrap().queue.push_back(frame(1, true, pairs.clone()));
        steps(&mut ui, 3);
        ui.m.run = RunState::Stop;
        steps(&mut ui, 2);
        {
            let mut g = shared.lock().unwrap();
            g.queue.push_back(frame(2, false, raw.clone()));
            g.final_ack = 1;
        }
        steps(&mut ui, 2);
        assert!(ui.stop_final.is_none());
        check(&ui, "the final raw frame");
        let counted = ui.mstat[0].n;
        assert!(counted > 0);
        // the view of the STOP read again as pairs: drawn, while the raw frame stays the record measured
        steps(&mut ui, 1);
        assert!(shared.lock().unwrap().stop_view.is_some(), "the UI asks for the view read again");
        shared.lock().unwrap().replay = Some(view(3, true, pairs.clone()));
        steps(&mut ui, 2);
        check(&ui, "the view read as pairs");
        assert_eq!(ui.mstat[0].n, counted, "the view counted in the statistics");
        let measured = |ui: &Ui| ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq);
        let drawn = |ui: &Ui| ui.stop_picture().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope));
        assert_eq!(measured(&ui), Some(2));
        assert_eq!(drawn(&ui), Some((3, true)), "the pairs read for the view are drawn");
        // zoomed in and out (a new time base clears the statistics, as any setting): the screen part of the raw frame
        for d in [-1, 1] {
            ui.adjust(Item::HScale, d);
            steps(&mut ui, 1);
            shared.lock().unwrap().replay = Some(view(4, true, pairs.clone()));
            steps(&mut ui, 2);
            check(&ui, "a new view read as pairs");
            assert!(ui.mstat.iter().all(|s| s.n == 0), "the record of the STOP counted for the view");
            assert_eq!(measured(&ui), Some(2));
            assert_eq!(drawn(&ui), Some((4, true)));
        }
        // far zoomed in the thread reads every sample of the view: that window is the record measured, as before
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 10e-6).abs() < 1e-11).unwrap();
        steps(&mut ui, 1);
        shared.lock().unwrap().replay = Some(view(5, false, vec![3.0; n]));
        steps(&mut ui, 2);
        assert_eq!(measured(&ui), Some(5));
        assert_eq!(drawn(&ui), None, "the raw window is drawn as the record shown");
        assert!((ui.meas(0, Meas::Vavg) - 3.0).abs() < 1e-3, "mean of the raw window {}", ui.meas(0, Meas::Vavg));
        // zoomed out again: pairs, measured from the raw frame of the STOP again
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        steps(&mut ui, 1);
        shared.lock().unwrap().replay = Some(view(6, true, pairs.clone()));
        steps(&mut ui, 2);
        assert_eq!((measured(&ui), drawn(&ui)), (Some(2), Some((6, true))));
        check(&ui, "pairs after the raw window");
        assert!(ui.mstat.iter().all(|s| s.n == 0));
        // RUN: the record of the STOP is let go
        ui.m.run = RunState::Run;
        shared.lock().unwrap().queue.push_back(frame(7, true, pairs.clone()));
        steps(&mut ui, 2);
        assert!(ui.stop_raw.is_none() && ui.stop_picture().is_none());
        crate::deepmeas::force(None);
    }

    // Break it catches: the stored record read again in STOP (`stop_replay`) counted as a new record in the statistics,
    // the trend and the catalog, once at the STOP and once for every view after it
    #[test]
    fn stop_reads_the_record_again_without_counting_it() {
        use super::*;
        crate::deepmeas::force(Some(false));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        ui.m.trig_level = 0.0;
        ui.m.ch[0].offset = 0.0;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vmax, WaveId::Ch(0))];
        ui.m.meas2.clear();
        let (n, dt) = (12_500usize, 0.8e-9);
        let frame = |seq: u64, top: f32| {
            let v: Vec<f32> = (0..n).map(|k| if (k + n / 2) % 1000 < 200 { top } else { -1.0 }).collect();
            let ch = [v, Vec::new(), Vec::new(), Vec::new()];
            crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
        };
        // what the thread answers for the view shown in STOP: the stored record read again
        let replay = |seq: u64, top: f32| crate::board::Frame { fixed_ti: Some((n / 2) as f64), ..frame(seq, top) };
        shared.lock().unwrap().queue.push_back(frame(1, 1.0));
        ui.step(0.05);
        assert_eq!(ui.mstat[0].n, 1);
        ui.m.run = RunState::Stop;
        ui.step(0.05);
        assert!(shared.lock().unwrap().stop_view.is_some(), "the UI asks for the view read again");
        shared.lock().unwrap().replay = Some(replay(2, 1.0));
        ui.step(0.05);
        assert_eq!(ui.mstat[0].n, 1, "the record of the STOP counted again");
        // a new view: the statistics start over, the record read again for it is shown and measured, not counted
        for (k, top) in [(1, 1.5), (-1, 2.0)] {
            ui.adjust(Item::HScale, k);
            ui.step(0.05);
            shared.lock().unwrap().replay = Some(replay(3, top));
            ui.step(0.05);
            assert_eq!(ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq), Some(3));
            assert!((ui.meas(0, Meas::Vmax) - top as f64).abs() < 1e-3, "Vmax {}", ui.meas(0, Meas::Vmax));
            assert!(ui.mstat.iter().all(|s| s.n == 0), "the record of the STOP counted for the view");
        }
        crate::deepmeas::force(None);
    }

    // Break it catches: Clear in STOP releasing the hold (the hold asked for a record shown): the thread armed while
    // stopped, overwrote the stored record and filled the queue, which the next RUN took; a source started in STOP
    // without its first record
    #[test]
    fn clear_in_stop_keeps_the_hardware_held() {
        use super::*;
        crate::deepmeas::force(Some(false));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        let (n, dt) = (12_500usize, 0.8e-9);
        let frame = |seq: u64| {
            let ch = [vec![0.5; n], Vec::new(), Vec::new(), Vec::new()];
            crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
        };
        // started in STOP: not held until the first record is here
        ui.m.run = RunState::Stop;
        ui.step(0.05);
        assert!(!shared.lock().unwrap().hold, "a source started in STOP gets its first record");
        shared.lock().unwrap().queue.push_back(frame(1));
        ui.step(0.05);
        ui.step(0.05);
        assert!(ui.last[0].is_some() && shared.lock().unwrap().hold);
        ui.clear_display();
        for _ in 0..3 {
            ui.step(0.05);
        }
        assert!(ui.last[0].is_none() && shared.lock().unwrap().hold, "Clear in STOP released the hardware");
        ui.m.run = RunState::Run;
        ui.step(0.05);
        assert!(!shared.lock().unwrap().hold);
        crate::deepmeas::force(None);
    }

    // Break it catches: RUN after STOP taking a frame queued before the RUN (the batch in progress at the STOP) as the
    // first record of the run, before the thread dropped the queue on its next pass
    #[test]
    fn run_after_stop_takes_no_frame_from_before() {
        use super::*;
        crate::deepmeas::force(Some(false));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        let (n, dt) = (12_500usize, 0.8e-9);
        let frame = |seq: u64| {
            let ch = [vec![0.5; n], Vec::new(), Vec::new(), Vec::new()];
            crate::board::Frame { seq, dt, pre: n / 2, ch, acq: (1.0 / dt, n), ..Default::default() }
        };
        let seq = |ui: &Ui| ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| r.frame.seq);
        shared.lock().unwrap().queue.push_back(frame(1));
        ui.step(0.05);
        ui.m.run = RunState::Stop;
        ui.step(0.05);
        assert!(shared.lock().unwrap().hold);
        // the batch in progress at the STOP is read and queued; the UI takes nothing in STOP
        shared.lock().unwrap().queue.push_back(frame(2));
        ui.step(0.05);
        assert_eq!(seq(&ui), Some(1));
        ui.m.run = RunState::Run;
        ui.step(0.05);
        assert_eq!(seq(&ui), Some(1), "a frame acquired before the RUN became the first record of the run");
        // what the thread acquires after the RUN is taken
        shared.lock().unwrap().queue.push_back(frame(3));
        ui.step(0.05);
        assert_eq!(seq(&ui), Some(3));
        crate::deepmeas::force(None);
    }

    // Break it catches (deepmeas.rs): with raw deep reads on, a pair record measured between two raw ones (Mean back
    // to NaN, statistics counting pair records), a raw deep record not measured, the hold active in Stop or Single
    #[test]
    fn deep_raw_records_carry_the_measurements_between_pair_records() {
        use super::*;
        crate::deepmeas::force(Some(true));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0)), (Meas::Vmax, WaveId::Ch(0))];
        ui.m.tscale = tscales().iter().position(|&t| (t as f64 - 1e-3).abs() < 1e-9).unwrap();
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 40_000usize;
        // raw: mean 0.25; pairs: the min/max of the same signal, other top so the record shown is visible
        let raw: Vec<f32> = (0..n).map(|i| if (i / 50) % 4 == 0 { 1.0 } else { 0.0 }).collect();
        let pairs = |top: f32| -> Vec<f32> { (0..n).map(|i| if i % 2 == 0 { 0.0 } else { top }).collect() };
        let push = |seq: u64, envelope: bool, v: Vec<f32>| {
            shared.lock().unwrap().queue.push_back(crate::board::Frame { seq, dt: window * 1.2 / n as f64, pre: n / 2, ch: [v, Vec::new(), Vec::new(), Vec::new()], envelope, acq: (1e9, 10_000_000), ..Default::default() });
        };
        let step = |ui: &mut Ui| {
            for _ in 0..3 {
                ui.step(0.02);
            }
        };
        // before the first raw record: no mean-type value on pairs, the maximum from them
        push(1, true, pairs(1.0));
        step(&mut ui);
        assert!(ui.meas(0, Meas::Vavg).is_nan() && (ui.meas(0, Meas::Vmax) - 1.0).abs() < 0.02);
        push(2, false, raw.clone());
        step(&mut ui);
        let avg = ui.meas(0, Meas::Vavg);
        assert!((avg - 0.25).abs() < 0.02, "raw mean {}", avg);
        let counted = ui.mstat.first().map_or(0, |s| s.n);
        // pair records keep the raw record's values, the statistics do not count them
        for seq in 3..6 {
            push(seq, true, pairs(1.5));
            step(&mut ui);
            assert_eq!(ui.meas(0, Meas::Vavg), avg);
            assert!((ui.meas(0, Meas::Vmax) - 1.0).abs() < 0.02);
        }
        assert_eq!(ui.mstat.first().map_or(0, |s| s.n), counted);
        // off: pair records are measured (mean-type NaN, max of the pairs)
        crate::deepmeas::force(Some(false));
        push(6, true, pairs(1.5));
        step(&mut ui);
        assert!(ui.meas(0, Meas::Vavg).is_nan() && (ui.meas(0, Meas::Vmax) - 1.5).abs() < 0.02);
        crate::deepmeas::force(None);
    }

    // Break it catches: the newest frame of a picture drawn another way than the others (record points instead of the
    // frame's samples), so the grade follows the mix of the two in each picture: the flicker at 500 us/div on the
    // probe-comp output (docs/fork/flicker.md). Equal frames, 1 to 5 of them per picture, must look the same.
    #[test]
    fn hardware_pictures_look_the_same_whatever_the_frames_per_picture() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        ui.m.persist = 0;
        ui.m.trig_level = 0.15;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        assert!(ui.fast_ok());
        // 100 k samples over 1.2 screens: a 0.3 V square with a little noise
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 100_000usize;
        let mut x = 9u32;
        let v: Vec<f32> = (0..n)
            .map(|k| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (if (k / 4000) % 2 == 1 { 0.3 } else { 0.0 }) + ((x >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.004
            })
            .collect();
        let frame = |seq: u64| crate::board::Frame { seq, dt: window * 1.2 / n as f64, pre: n / 2, ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()], ..Default::default() };
        let mut seq = 0;
        let mut picture = |ui: &mut Ui, k: usize| -> Vec<u32> {
            {
                let mut g = shared.lock().unwrap();
                for _ in 0..k {
                    seq += 1;
                    g.queue.push_back(frame(seq));
                }
            }
            ui.step(0.033);
            let (lut, sh, _) = ui.trace_lut(ui.vmax[0]);
            ui.ph.map[0].iter().map(|&c| if c == 0 { 0 } else { lut[((c as u32) >> sh).min(1023) as usize] }).collect()
        };
        let first = picture(&mut ui, 3);
        for k in [1, 2, 3, 1, 5, 1, 2] {
            let p = picture(&mut ui, k);
            let worst = first.iter().zip(&p).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
            assert!(worst <= 3, "{} frames: brightness off by {}", k, worst);
        }
    }

    // Break it catches: Trig'd shown for a batch the auto timeout forced (the software found an edge in it), or the
    // hardware status ignored when the thread reports it
    #[test]
    fn hardware_trigger_status_overrides_the_software_one() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.run = RunState::Run;
        ui.m.trig_level = 0.5;
        let window = ui.m.time_per_div() as f64 * HDIV as f64;
        let n = 4000usize;
        let frame = |seq: u64| crate::board::Frame {
            seq,
            dt: window / n as f64 * 1.2,
            pre: n / 2,
            ch: [(0..n).map(|k| if (k / 200) % 2 == 1 { 1.0 } else { 0.0 }).collect(), Vec::new(), Vec::new(), Vec::new()],
            ..Default::default()
        };
        let mut seq = 0;
        let mut run = |ui: &mut Ui, fired: Option<(u64, Option<bool>)>| {
            {
                let mut g = shared.lock().unwrap();
                for _ in 0..3 {
                    seq += 1;
                    g.queue.push_back(frame(seq));
                }
                g.trig_fired = fired;
            }
            ui.step(0.02);
            ui.m.status
        };
        // off (no reading): the software finds the edge at the hardware point
        assert_eq!(run(&mut ui, None), TrigStatus::Triggered);
        assert_eq!(run(&mut ui, Some((1, Some(false)))), TrigStatus::Auto);
        assert_eq!(run(&mut ui, Some((2, Some(true)))), TrigStatus::Triggered);
    }

    // Break it catches: the trace brightness at minimum persistence following the number of frames per picture (one
    // frame after a batch of several came out dim: the flicker after autoset, docs/fork/trigger-lockin.md)
    #[test]
    fn equal_frames_give_equal_brightness_whatever_their_number() {
        let mut ui = super::Ui::new(crate::wave::Source::Demo { t: 0.0 });
        ui.m.persist = 0;
        for c in 1..super::NCH {
            ui.m.ch[c].on = false;
        }
        // one frame: a 1 kHz square with a little noise, 3600 points over the screen
        let mut x = 3u32;
        let v: Vec<f32> = (0..3600)
            .map(|k| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (if (k / 360) % 2 == 0 { 0.0 } else { 0.3 }) + ((x >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.004
            })
            .collect();
        let (tdiv, window) = (ui.m.time_per_div(), ui.m.time_per_div() as f64 * super::HDIV as f64);
        let picture = |ui: &mut super::Ui, n: u32| -> Vec<u32> {
            ui.ph.clear();
            for _ in 0..n {
                ui.ph.draw_ex(0, &ui.m.clone(), (tdiv, 0.0), -window / 2.0, window / 3600.0, &v, 24, None, false);
            }
            ui.pic_frames = n;
            ui.grade_refs();
            let (lut, sh, _) = ui.trace_lut(ui.vmax[0]);
            // as `draw_traces`: an empty pixel stays dark
            ui.ph.map[0].iter().map(|&k| if k == 0 { 0 } else { lut[((k as u32) >> sh).min(1023) as usize] }).collect()
        };
        let first = picture(&mut ui, 4);
        for n in [1, 7, 1, 2, 16, 1, 4] {
            let p = picture(&mut ui, n);
            let worst = first.iter().zip(&p).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
            assert!(worst <= 3, "{} frames: brightness off by {}", n, worst);
        }
        // the plain grade (longer persistence) is what pumped: one frame after sixteen is graded against 16 times its
        // counts
        ui.pic_frames = 0;
        ui.m.persist = 1;
        let _ = picture(&mut ui, 16);
        ui.pic_frames = 0;
        ui.grade_refs();
        let bright = |ui: &super::Ui| ui.vmax[0];
        let before = bright(&ui);
        ui.ph.clear();
        ui.ph.draw_ex(0, &ui.m.clone(), (tdiv, 0.0), -window / 2.0, window / 3600.0, &v, 24, None, false);
        ui.grade_refs();
        assert!(bright(&ui) > ui.ph.map[0].iter().copied().max().unwrap() as u32 * 8, "{}", before);
    }

    /// Measurements in the background (the frame loop on the unit) while a new record comes every frame: their results
    /// still arrive (0.1.7 to 0.1.10 dropped each one as older than the record shown, and the values froze in Run).
    #[test]
    fn background_measurements_follow_a_running_record() {
        let mut u = super::Ui::new(crate::wave::Source::Demo { t: 0.0 });
        u.measure_in_background();
        let n = |u: &super::Ui| u.mstat.iter().map(|s| s.n).max().unwrap_or(0);
        for _ in 0..120 {
            u.step(1.0 / 30.0);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        assert!(n(&u) >= 20, "{} results taken in 120 frames", n(&u));
    }

    /// Measure > Add item: a second tap on a measurement removes it again, the others stay.
    #[test]
    fn add_item_toggles_a_measurement() {
        use super::*;

        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.meas.clear();
        ui.meas_src = WaveId::Ch(0);
        let (a, b) = (ALL_MEAS[0], ALL_MEAS[1]);
        ui.activate(Item::MeasAdd(a));
        ui.activate(Item::MeasAdd(b));
        assert_eq!(ui.m.meas, vec![(a, WaveId::Ch(0)), (b, WaveId::Ch(0))]);
        ui.activate(Item::MeasAdd(a));
        assert_eq!(ui.m.meas, vec![(b, WaveId::Ch(0))]);
        // the same measurement of another channel is its own entry
        ui.meas_src = WaveId::Ch(1);
        ui.activate(Item::MeasAdd(b));
        assert_eq!(ui.m.meas, vec![(b, WaveId::Ch(0)), (b, WaveId::Ch(1))]);
        ui.meas_src = WaveId::Ch(0);
        ui.activate(Item::MeasAdd(b));
        assert_eq!(ui.m.meas, vec![(b, WaveId::Ch(1))]);
    }

    /// CAN and LIN decode the demo bus traffic: IDs, data and CRC / checksum.
    #[test]
    fn can_and_lin_decode_the_demo_bus() {
        use super::*;
        use oos_dsp::decode::Kind;
        for (proto, tdiv, want_id) in [(Proto::CAN, 100e-6f32, 0x123u32), (Proto::LIN, 1e-3, 0x21)] {
            // (I2S and 1553 have their own test below)
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_on = true;
            ui.set_dec_proto(proto);
            // a trigger that holds the demo bus still: the first falling edge after the bus idle
            (ui.m.trig_type, ui.m.trig_src, ui.m.slope, ui.m.nth_idle, ui.m.nth_n, ui.m.trig_level) = (TrigType::NthEdge, 2, Slope::Fall, 20e-6, 1, 1.65);
            ui.m.tscale = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
            for _ in 0..20 {
                ui.step(1.0 / 30.0);
            }
            let p = ui.decode_packets().unwrap();
            let ids: Vec<u32> = p.iter().filter(|x| matches!(x.kind, Kind::Id { .. })).map(|x| x.value).collect();
            assert!(ids.contains(&want_id), "{:?}: ids {:x?}", proto, ids);
            assert!(p.iter().filter(|x| x.kind == Kind::Crc).all(|x| x.ok), "{:?}: {:?}", proto, p);
            assert!(p.iter().any(|x| x.kind == Kind::Crc), "{:?}: no CRC/checksum", proto);
            // picture for the eye (target/test-shots)
            let mut cv = Canvas::new(PANEL_W, PANEL_H);
            ui.render(&mut cv);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
            let _ = std::fs::create_dir_all(&dir);
            let _ = crate::store::write(&dir.join(format!("i-{:?}.png", proto).to_lowercase()), &crate::store::png_of(&cv));
        }
    }

    // Break it catches: the Bus trigger's own switches staying after going back to Edge (decoder on: the live picture
    // stayed raw; its demo source channels stayed on), or switching off what the user had on before Bus
    #[test]
    #[ignore = "the fork keeps choosing Bus from switching a decoder on (batch2_tests)"]
    fn leaving_the_bus_trigger_undoes_only_what_it_switched_on() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for c in 0..NCH {
            ui.m.ch[c].on = c == 0;
        }
        ui.m.dec_on = false;
        ui.m.dec_proto = Proto::I2C;
        ui.set_trig_type(TrigType::Bus);
        assert!(ui.m.dec_on && ui.m.ch[2].on && ui.m.ch[3].on, "Bus: the decoder and its sources (I2C on CH3/CH4)");
        // Bus again: nothing new recorded, still undone once
        ui.set_trig_type(TrigType::Bus);
        ui.set_trig_type(TrigType::Edge);
        assert!(!ui.m.dec_on, "the decoder Bus switched on is off again");
        let on: Vec<bool> = (0..NCH).map(|c| ui.m.ch[c].on).collect();
        assert_eq!(on, vec![true, false, false, false], "only the user's CH1 stays");
        // what the user had on before stays on: the decoder and CH3
        ui.m.dec_on = true;
        ui.m.ch[2].on = true;
        ui.set_trig_type(TrigType::Bus);
        ui.set_trig_type(TrigType::Edge);
        assert!(ui.m.dec_on && ui.m.ch[2].on && !ui.m.ch[3].on, "user's decoder and CH3 kept, CH4 off again");
        // another type in between that is not Bus: nothing to undo twice
        ui.set_trig_type(TrigType::Slope);
        assert!(ui.m.dec_on && ui.m.ch[2].on);
    }

    /// Protocol trigger: the end of the matching CAN identifier sits at the trigger point; "Error" never fires on clean
    /// traffic in Normal mode.
    #[test]
    fn bus_trigger_aligns_on_the_matching_frame() {
        use super::*;
        use oos_dsp::decode::Kind;
        for id in [0x123u32, 0x18da_f110] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_on = true;
            ui.set_dec_proto(Proto::CAN);
            ui.set_trig_type(TrigType::Bus);
            ui.m.trig_bus_val = id as u64;
            ui.m.tscale = tscales().iter().position(|&t| (t - 50e-6).abs() < 1e-9).unwrap();
            for _ in 0..10 {
                ui.step(1.0 / 30.0);
            }
            assert!(ui.last[0].as_ref().unwrap().triggered, "id {:x} not triggered", id);
            let p = ui.decode_packets().unwrap();
            let hit = p.iter().find(|x| matches!(x.kind, Kind::Id { .. }) && x.value == id).expect("matching ID in the record");
            // trigger point = end of the matching ID field = t 0 (within one screen point of 500 us / 3600)
            assert!(hit.t1.abs() < 0.5e-6, "id {:x}: field ends at {} s", id, hit.t1);
            if id == 0x123 {
                ui.open_menu(MenuId::Trigger);
                let mut cv = Canvas::new(PANEL_W, PANEL_H);
                ui.render(&mut cv);
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                let _ = std::fs::create_dir_all(&dir);
                let _ = crate::store::write(&dir.join("j-bus-trigger.png"), &crate::store::png_of(&cv));
            }
        }
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.dec_proto = Proto::CAN;
        ui.set_trig_type(TrigType::Bus);
        ui.m.trig_bus = BusWhat::Error;
        ui.m.trig_mode = TrigMode::Normal;
        ui.m.tscale = tscales().iter().position(|&t| (t - 50e-6).abs() < 1e-9).unwrap();
        for _ in 0..10 {
            ui.step(1.0 / 30.0);
        }
        assert!(ui.last[0].is_none(), "Error trigger fired on clean traffic");
    }

    /// Protocol trigger on ARINC 429 (label), SENT (data nibbles) and FlexRay (frame ID) with the demo traffic: the end
    /// of the matching field sits at the trigger point, Frame triggers, Error never fires on the clean traffic.
    #[test]
    #[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
    fn bus_trigger_on_arinc_sent_and_flexray() {
        use super::*;
        use oos_dsp::decode::Kind;
        let k = |s: &str| Proto::from_key(s).unwrap();
        let (arinc, sent) = (k("Arinc429"), k("Sent"));
        for (proto, tdiv) in [(arinc, 100e-6f32), (sent, 200e-6), (k("FlexRay"), 5e-6)] {
            let tsc = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
            let tol = 3.0 * tdiv as f64 * HDIV as f64 / 3600.0; // three record samples
            for what in [BusWhat::Id, BusWhat::Data, BusWhat::Frame, BusWhat::Error] {
                let mut ui = Ui::new(Source::Demo { t: 0.0 });
                ui.m.dec_on = true;
                ui.set_dec_proto(proto);
                ui.set_trig_type(TrigType::Bus);
                let id_default = ui.m.trig_bus_val;
                ui.m.trig_bus = what;
                // a payload value of the demo traffic
                if what == BusWhat::Data {
                    ui.m.trig_bus_val = if proto == arinc { 0x2_5a5a } else if proto == sent { 0x7f_480b } else { 0x55 };
                } else if what == BusWhat::Id && proto == sent {
                    ui.m.trig_bus_val = 0x4; // status nibble of the second frame
                }
                ui.m.trig_mode = TrigMode::Normal;
                ui.m.tscale = tsc;
                for _ in 0..6 {
                    ui.step(1.0 / 30.0);
                }
                if what == BusWhat::Error {
                    assert!(ui.last[0].is_none(), "{:?}: Error trigger fired on clean traffic", proto);
                    continue;
                }
                assert!(ui.last[0].as_ref().is_some_and(|r| r.triggered), "{:?} {:?}: not triggered", proto, what);
                let p = ui.decode_packets().unwrap();
                assert!(crate::trig::points(&ui.m, &p).iter().any(|t| t.abs() < tol), "{:?} {:?}: no matching field ends at 0: {:?}", proto, what, p);
                if what == BusWhat::Id && proto != sent {
                    let want = if proto == arinc { 0o310 } else { 0x2a };
                    assert_eq!(id_default, want);
                    assert!(p.iter().any(|x| matches!(x.kind, Kind::Id { .. }) && x.value as u64 == want && x.t1.abs() < tol));
                }
                if what == BusWhat::Id {
                    let mut cv = Canvas::new(PANEL_W, PANEL_H);
                    ui.render(&mut cv);
                    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = crate::store::write(&dir.join(format!("j-bus-{}.png", proto.key()).to_lowercase()), &crate::store::png_of(&cv));
                }
            }
        }
    }

    /// Protocol trigger on USB with the demo traffic (full speed, D+ / D- on CH3 / CH4): the token address, a PID,
    /// a payload byte and Frame (end of SYNC) at the trigger point, Error never fires; D+ alone decodes the same.
    #[test]
    #[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
    fn bus_trigger_on_usb() {
        use super::*;
        use oos_dsp::decode::Kind;
        let tdiv = 5e-6f32;
        let tsc = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
        let tol = 3.0 * tdiv as f64 * HDIV as f64 / 3600.0; // three record samples
        for (what, val) in [(BusWhat::Id, None), (BusWhat::Id, Some(0x4B)), (BusWhat::Data, Some(0x40)), (BusWhat::Frame, None), (BusWhat::Error, None)] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_on = true;
            ui.set_dec_proto(Proto::from_key("Usb").unwrap());
            ui.set_trig_type(TrigType::Bus);
            assert_eq!((ui.m.dec_src, ui.m.dec_cs, ui.m.trig_bus, ui.m.trig_bus_val), (2, Some(3), BusWhat::Id, 5));
            assert!(ui.decode_items().contains(&Item::DecUsbSpeed) && ui.decode_items().contains(&Item::DecCs));
            assert_eq!(ui.item_label(Item::DecCs), "D-");
            ui.m.trig_bus = what;
            if let Some(v) = val {
                ui.m.trig_bus_val = v;
            }
            ui.m.trig_mode = TrigMode::Normal;
            ui.m.tscale = tsc;
            for _ in 0..6 {
                ui.step(1.0 / 30.0);
            }
            if what == BusWhat::Error {
                assert!(ui.last[0].is_none(), "USB: Error trigger fired on clean traffic");
                continue;
            }
            assert!(ui.last[0].as_ref().is_some_and(|r| r.triggered), "USB {:?} {:?}: not triggered", what, val);
            let p = ui.decode_packets().unwrap();
            assert!(crate::trig::points(&ui.m, &p).iter().any(|t| t.abs() < tol), "USB {:?}: no matching field ends at 0: {:?}", what, p);
            match (what, val) {
                (BusWhat::Id, None) => assert!(p.iter().any(|x| x.kind == Kind::Field("ADDR") && x.value == 5 && x.t1.abs() < tol)),
                (BusWhat::Id, Some(v)) => assert!(p.iter().any(|x| x.kind == Kind::Field("PID") && x.value as u64 == v && x.t1.abs() < tol)),
                (BusWhat::Frame, _) => assert!(p.iter().any(|x| x.kind == Kind::Field("SYNC") && x.t1.abs() < tol)),
                _ => {}
            }
            assert!(p.iter().all(|x| x.ok), "{:?}", p);
            // D+ alone: the same packets (without the EOP of a packet cut by the record start), the speed settings
            let both: Vec<_> = p.iter().map(|x| (x.kind, x.value)).collect();
            ui.m.dec_cs = None;
            let one: Vec<_> = ui.decode_packets().unwrap().iter().map(|x| (x.kind, x.value)).collect();
            assert!(both.ends_with(&one) && one.len() + 1 >= both.len(), "{:?}\n{:?}", one, both);
            ui.m.dec_uspeed = 2;
            assert_eq!(ui.decode_packets().unwrap().len(), one.len());
            ui.m.dec_uspeed = 1;
            assert!(ui.decode_packets().unwrap().iter().all(|x| x.kind != Kind::Field("PID")));
            if what == BusWhat::Id && val.is_none() {
                ui.m.dec_cs = Some(3);
                ui.m.dec_uspeed = 0;
                let mut cv = Canvas::new(PANEL_W, PANEL_H);
                ui.render(&mut cv);
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                let _ = std::fs::create_dir_all(&dir);
                let _ = crate::store::write(&dir.join("j-bus-usb.png"), &crate::store::png_of(&cv));
            }
        }
    }

    /// The protocol trigger value is entered on the hex keypad.
    #[test]
    fn math_on_math() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.ch[1].on = true;
        ui.step(0.02);
        // M1 = CH1 + CH2, M2 = Abs(M1); M1 offers only the channels, M2 also M1
        ui.m.math = MathOp::Add;
        assert_eq!(ui.options(Item::MathA).len(), NCH);
        ui.m.math_select(1);
        assert_eq!(ui.options(Item::MathA).last().map(|s| s.as_str()), Some("M1"));
        ui.m.math = MathOp::Abs;
        ui.m.math_a = NCH; // M1
        let rec = ui.last[0].clone().unwrap();
        let m1 = ui.math_wave_of(0);
        let m2 = ui.math_wave_of(1);
        assert_eq!(m1.len(), rec.ch[0].len());
        for i in (0..m1.len()).step_by(97) {
            assert_eq!(m1[i], rec.ch[0][i] + rec.ch[1][i]);
            assert_eq!(m2[i], m1[i].abs());
        }
        // switching back keeps both channels
        ui.m.math_select(0);
        assert_eq!((ui.m.math, ui.m.math_ch(1).op, ui.m.math_ch(1).a), (MathOp::Add, MathOp::Abs, NCH));
    }

    #[test]
    fn zone_drawn_by_touch_filters_records() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.relayout();
        ui.open_menu(MenuId::Zone);
        ui.activate(Item::ZoneBox);
        assert_eq!(ui.zone_arm, Some((0, false)));
        // a box in the top left corner of the plot (far above the demo signal)
        let a = ui.area;
        let (x0, y0, x1, y1) = (a.x + 60, a.y + 20, a.x + 160, a.y + 60);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y: y0 });
        ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x: x1, y: y1 });
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: x1, y: y1 });
        let z = ui.m.zones[0].clone();
        assert!(z.on && z.inside && z.pts.len() == 2, "{:?}", z);
        assert!(z.pts[0].1 > z.pts[1].1, "top edge has the higher voltage");
        // "must intersect" an empty region: every record is dropped
        ui.last[0] = None;
        for _ in 0..5 {
            ui.step(0.02);
        }
        assert!(ui.zone_drops > 0 && ui.last[0].is_none());
        // "must not intersect": records pass again
        ui.m.zones[0].inside = false;
        ui.step(0.02);
        assert!(ui.last[0].is_some());
    }

    #[test]
    #[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
    fn extended_decoders_decode_their_demo() {
        use super::*;
        // (protocol, time/div that shows a whole frame, a label or kind that must appear)
        // (the modules DMX512, S/PDIF and USB PD run through the same path as every protocol)
        let m = |k: &str| Proto::from_key(k).unwrap();
        let cases: [(Proto, f32, &str); 14] = [
            (m("Dcc"), 10e-3, "PRE"),
            (m("CanFd"), 50e-6, "BRS"),
            (m("Dmx512"), 100e-6, "SC"),
            (m("ModbusRtu"), 1e-3, "FUNC"),
            (m("Uart9"), 1e-3, "Addr"),
            (m("OneWire"), 200e-6, "ROM"),
            (m("UsbPd"), 100e-6, "HDR"),
            (m("Swd"), 20e-6, "REQ"),
            (m("Spdif"), 20e-6, "Sample"),
            (m("Example"), 1e-3, "Data"),
            (m("FlexRay"), 5e-6, "HCRC"),
            (m("Arinc429"), 100e-6, "SSM"),
            (m("Sent"), 200e-6, "STAT"),
            (m("Usb"), 5e-6, "ENDP"),
        ];
        for (p, tdiv, want) in cases {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.set_dec_proto(p);
            ui.m.dec_on = true;
            ui.m.tscale = tscales().iter().position(|&x| (x - tdiv).abs() < tdiv * 1e-3).unwrap();
            ui.m.trig_type = TrigType::Edge;
            ui.m.trig_mode = TrigMode::Auto;
            let mut found = String::new();
            for _ in 0..6 {
                ui.step(0.05);
                let pk = ui.decode_packets().unwrap_or_default();
                found = format!("{:?}", pk);
                if found.contains(want) {
                    break;
                }
            }
            assert!(found.contains(want), "{:?}: no {} in {}", p, want, &found[..found.len().min(400)]);
        }
    }

    #[test]
    fn segmented_recording_stops_after_n_records() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.segments = 10;
        for _ in 0..20 {
            ui.step(0.02);
        }
        assert_eq!((ui.m.run, ui.seg_n, ui.history.len()), (RunState::Stop, 10, 10));
        // RUN again: a new set
        ui.key(key::RUNSTOP);
        ui.step(0.02);
        assert!(ui.seg_n <= 6 && ui.history.len() == ui.seg_n);
    }

    #[test]
    fn analysers_on_the_demo_signal() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap();
        ui.step(0.05);
        // spectrum: the strongest line of the 1 kHz square is its fundamental
        ui.open_menu(MenuId::Spectrum);
        ui.adjust(Item::SpecOn, 1);
        assert_eq!(ui.m.view, AView::Spectrum);
        ui.step(0.05);
        let p = spectrum_peak(&ui).expect("a peak");
        // within a display column (the spectrum has one point per plot column)
        assert!((p - 1e3).abs() < 40.0, "peak at {} Hz", p);
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        // eye: the recovered unit interval is half a period (0.5 ms)
        ui.adjust(Item::EyeOn, 1);
        for _ in 0..3 {
            ui.step(0.05);
        }
        let (_, _, period) = ui.azr.eye_res.clone().expect("eye result");
        assert!((period - 0.5e-3).abs() < 0.02e-3, "UI {}", period);
        ui.render(&mut cv);
        // power and identify do not panic; identify names CH1 and applies an edge trigger on it
        ui.adjust(Item::PowOn, 1);
        ui.step(0.05);
        assert!(!ui.azr.power.is_empty());
        ui.render(&mut cv);
        ui.activate(Item::IdentifyAct);
        assert_eq!(ui.menu, MenuId::Identify);
        let k = ui.azr.findings.iter().position(|f| f.channels.contains(&0)).expect("a finding on CH1");
        ui.m.trig_src = 3;
        ui.activate(Item::IdFind(k));
        assert_eq!(ui.m.trig_src, 0);
    }

    /// Screenshots of the new views for a visual check: `OOS_SHOTS=<dir> cargo test shots -- --ignored`.
    #[test]
    #[ignore]
    fn shots_of_new_views() {
        use super::*;
        let dir = std::env::var("OOS_SHOTS").unwrap_or_else(|_| ".".into());
        let shot = |ui: &mut Ui, name: &str| {
            let mut cv = Canvas::new(PANEL_W, PANEL_H);
            ui.render(&mut cv);
            std::fs::write(format!("{}/{}.png", dir, name), crate::store::png_of(&cv)).unwrap();
        };
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-3).abs() < 1e-6).unwrap();
        ui.m.ch[1].on = true;
        ui.step(0.05);
        ui.m.spec_wf = true;
        ui.m.spec_dist = true;
        ui.adjust(Item::SpecOn, 1);
        for _ in 0..30 {
            ui.step(0.05);
        }
        shot(&mut ui, "spectrum");
        ui.adjust(Item::EyeOn, 1);
        for _ in 0..10 {
            ui.step(0.05);
        }
        shot(&mut ui, "eye");
        ui.adjust(Item::PowOn, 1);
        ui.step(0.05);
        shot(&mut ui, "power");
        ui.m.view = AView::Scope;
        ui.m.math = MathOp::Add;
        ui.m.math_select(1);
        ui.m.math = MathOp::Bpf;
        ui.m.math_a = NCH;
        ui.m.hist = Hist::Vertical;
        ui.m.zones[0] = crate::zone::Zone { on: true, src: 0, inside: true, pts: vec![(-2e-3, 2.0), (-1.5e-3, 3.0)] };
        for _ in 0..10 {
            ui.step(0.05);
        }
        shot(&mut ui, "scope_math_zone_hist");
        ui.activate(Item::IdentifyAct);
        shot(&mut ui, "identify");
        ui.close_menu();
        ui.set_dec_proto(Proto::from_key("CanFd").unwrap());
        ui.m.dec_on = true;
        ui.m.math = MathOp::Off;
        ui.m.math_select(0);
        ui.m.math = MathOp::Off;
        ui.m.zones[0].on = false;
        ui.m.hist = Hist::Off;
        ui.m.tscale = tscales().iter().position(|&x| (x - 20e-6).abs() < 1e-9).unwrap();
        for _ in 0..5 {
            ui.step(0.05);
        }
        shot(&mut ui, "canfd");
        ui.open_menu(MenuId::Main);
        shot(&mut ui, "main_menu");
    }

    /// System > Graphics = GPU draws the layers from commands: replayed, they give exactly the pixels of drawing
    /// directly, on every kind of screen.
    #[test]
    #[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
    fn recorded_layers_replay_to_the_same_pixels() {
        use super::*;
        let same = |ui: &mut Ui, what: &str| {
            let (w, h) = (ui.scr.w, ui.scr.h);
            let (mut u, mut o) = (Canvas::new(w, h), Canvas::new_layer(w, h));
            ui.render_layers(&mut Layers { under: &mut u, over: Some(&mut o), jobs: Vec::new() });
            let (mut ru, mut ro) = (Canvas::new(w, h), Canvas::new_layer(w, h));
            ru.record(true);
            ro.record(true);
            ui.render_layers(&mut Layers { under: &mut ru, over: Some(&mut ro), jobs: Vec::new() });
            let (cu, co) = (ru.rec.take().unwrap(), ro.rec.take().unwrap());
            assert!(cu.cmds.len() > 10, "{}: nothing recorded", what);
            let (mut pu, mut po) = (Canvas::new(w, h), Canvas::new_layer(w, h));
            cu.replay(&mut pu);
            co.replay(&mut po);
            for (name, a, b) in [("under", &u, &pu), ("over", &o, &po)] {
                let bad = a.px.iter().zip(&b.px).position(|(p, q)| p != q);
                assert!(bad.is_none(), "{} {}: pixel {:?} differs: {:08x} drawn, {:08x} replayed", what, name, bad.map(|i| (i as i32 % w, i as i32 / w)), bad.map_or(0, |i| a.px[i]), bad.map_or(0, |i| b.px[i]));
            }
        };
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.ch[1].on = true;
        for _ in 0..5 {
            ui.step(0.05);
        }
        same(&mut ui, "scope");
        ui.open_menu(MenuId::Main);
        same(&mut ui, "main menu");
        ui.close_menu();
        ui.open_menu(MenuId::Measure);
        same(&mut ui, "measure menu");
        ui.close_menu();
        ui.key(key::MEASURE);
        same(&mut ui, "measure page");
        ui.close_menu();
        ui.m.zoom = true;
        ui.m.color_grade = true;
        ui.step(0.05);
        same(&mut ui, "zoom, colour grade");
        ui.m.zoom = false;
        ui.m.color_grade = false;
        ui.adjust(Item::SpecOn, 1);
        for _ in 0..10 {
            ui.step(0.05);
        }
        same(&mut ui, "spectrum");
        ui.adjust(Item::EyeOn, 1);
        for _ in 0..10 {
            ui.step(0.05);
        }
        same(&mut ui, "eye");
        ui.m.view = AView::Scope;
        ui.set_dec_proto(Proto::from_key("CanFd").unwrap());
        ui.m.dec_on = true;
        for _ in 0..5 {
            ui.step(0.05);
        }
        same(&mut ui, "decode");
    }

    fn spectrum_peak(ui: &super::Ui) -> Option<f64> {
        ui.strongest_line().map(|m| m.f)
    }

    #[test]
    fn protocol_watch_names_a_new_bus() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_dec_proto(Proto::UART);
        // demo UART on CH3 (115200 Bd): watch on at 100 us/div (a screen record resolves the bits), a few seconds
        ui.m.ch[2].on = true;
        ui.m.tscale = tscales().iter().position(|&x| (x - 100e-6).abs() < 1e-9).unwrap();
        ui.adjust(Item::ProtoWatch, 1);
        let mut named = String::new();
        for _ in 0..80 {
            ui.step(0.05);
            if let Some((t, _)) = &ui.toast {
                if t.starts_with("Protocol watch") {
                    named = t.clone();
                    break;
                }
            }
        }
        let kinds: Vec<String> = ui.azr.findings.iter().map(|f| format!("{:?} {:?} {:.2}", f.channels, f.kind, f.confidence)).collect();
        assert!(named.contains("CH3"), "toast: {:?}, seen {:?}, findings {:?}", named, ui.azr.watch_seen, kinds);
        assert!(ui.identify_items().len() > 1);
    }

    #[test]
    fn bode_sweep_drives_the_generator() {
        use super::*;
        use std::io::Read;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let got = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let g2 = got.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 4096];
                while let Ok(n) = s.read(&mut b) {
                    if n == 0 {
                        break;
                    }
                    g2.lock().unwrap().push_str(&String::from_utf8_lossy(&b[..n]));
                }
            }
        });
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        (ui.m.bode_addr, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_points, ui.m.bode_gen) = (addr, 1e3, 1e5, 5, 1);
        let ts0 = ui.m.tscale;
        ui.activate(Item::BodeRun);
        assert!(ui.bode.running);
        for _ in 0..200 {
            ui.step(0.05);
            if !ui.bode.running {
                break;
            }
        }
        assert!(!ui.bode.running, "sweep did not finish ({} of {})", ui.bode.idx, ui.bode.freqs.len());
        assert_eq!(ui.bode.points.len(), 5);
        assert_eq!(ui.m.tscale, ts0, "time base restored");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let cmds = got.lock().unwrap().clone();
        assert!(cmds.contains(":SOUR1:APPL:SIN 1000,1,0") && cmds.contains(":OUTP1 ON") && cmds.contains(":SOUR1:APPL:SIN 100000"), "{}", cmds);
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
    }

    #[test]
    fn bode_sweep_with_the_builtin_generator() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        (ui.m.bode_gen, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_points, ui.m.bode_in, ui.m.bode_out) = (bode::BUILTIN, 1e3, 1e5, 3, WaveId::Ch(0), WaveId::Ch(0));
        (ui.m.afg_on, ui.m.afg_freq) = (false, 1234.0);
        ui.activate(Item::BodeRun);
        assert!(ui.m.afg_on && ui.m.afg_freq == 1e3 && ui.m.afg_offset == 0.0);
        let mut seen = vec![];
        for _ in 0..200 {
            if !ui.bode.running {
                break;
            }
            seen.push(ui.m.afg_freq);
            ui.step(0.05);
        }
        assert!(!ui.bode.running);
        assert!(seen.iter().any(|f| (f / 1e4 - 1.0).abs() < 1e-9) && seen.contains(&1e5), "{:?}", seen);
        assert_eq!((ui.m.afg_on, ui.m.afg_freq), (false, 1234.0), "generator restored");
        // one channel on the generator: gain only
        assert!(ui.bode.points.iter().all(|p| p.2.is_nan() && p.1.is_finite()), "{:?}", ui.bode.points);
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
    }

    #[test]
    fn web_csv_refuses_a_record_it_would_have_to_cut() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert!(ui.web_wave_csv().is_err());
        for _ in 0..30 {
            ui.step(0.02);
        }
        assert!(ui.web_wave_csv().unwrap().starts_with("Time (s)"));
        ui.last[0].as_mut().unwrap().ch[0] = vec![0.0; WEB_CSV_MAX + 1];
        assert!(ui.web_wave_csv().unwrap_err().contains("too many"));
    }

    #[test]
    fn keypad_enters_a_long_can_id() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.dec_proto = Proto::CAN;
        ui.set_trig_type(TrigType::Bus);
        ui.open_menu(MenuId::Trigger);
        ui.open_picker(Item::TrigBusVal);
        assert_eq!(ui.keypad, "123");
        let key = |ui: &mut Ui, k: &str| {
            let i = KEYPAD.iter().position(|x| *x == k).unwrap();
            ui.activate(Item::PickOpt(i));
        };
        key(&mut ui, "Clear");
        for d in ["1", "8", "D", "A", "F", "1", "1", "0"] {
            key(&mut ui, d);
        }
        // up to ten digits (40 bits): the eleventh is ignored
        for d in ["9", "9", "9"] {
            key(&mut ui, d);
        }
        key(&mut ui, "Del");
        key(&mut ui, "0");
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
        let _ = std::fs::create_dir_all(&dir);
        let _ = crate::store::write(&dir.join("j-keypad.png"), &crate::store::png_of(&cv));
        key(&mut ui, "OK");
        assert_eq!(ui.m.trig_bus_val, 0x18_daf1_1090);
        assert_eq!(ui.menu, MenuId::Trigger);
    }

    /// I2S and MIL-STD-1553 decode their demo signals.
    #[test]
    #[ignore = "the fork: choosing a protocol leaves the trigger alone (batch2_tests), no demo trigger to hold the decode"]
    fn i2s_and_1553_decode_the_demo() {
        use super::*;
        use oos_dsp::decode::Kind;
        for (key, tdiv) in [("I2s", 10e-6f32), ("Mil1553", 20e-6)] {
            let proto = Proto::from_key(key).unwrap();
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_on = true;
            ui.set_dec_proto(proto);
            // the trigger that holds the demo still: I2S the falling WS edge on CH1, 1553 the first rising edge after
            // 3 us of quiet bus on CH3
            let m = &ui.m;
            let want = if key == "I2s" { (TrigType::Edge, 0, Slope::Fall, 1.65) } else { (TrigType::NthEdge, 2, Slope::Rise, 2.0) };
            assert_eq!((m.trig_type, m.trig_src, m.slope, m.trig_level), want, "{}", key);
            assert!(key == "I2s" || (m.nth_idle, m.nth_n) == (3e-6, 1));
            ui.m.tscale = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
            for _ in 0..20 {
                ui.step(1.0 / 30.0);
            }
            let p = ui.decode_packets().unwrap();
            if key == "I2s" {
                let s: Vec<(bool, u32)> = p.iter().filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None }).collect();
                assert!(s.contains(&(false, 0x4000)) && s.contains(&(true, 0xc000)), "{:x?}", s);
            } else {
                let w: Vec<(bool, u32, bool)> = p.iter().filter_map(|x| if let Kind::Word { cmd } = x.kind { Some((cmd, x.value, x.ok)) } else { None }).collect();
                assert!(w.contains(&(true, 0x0c23, true)) && w.contains(&(false, 0xbeef, true)), "{:x?}", w);
            }
            let mut cv = Canvas::new(PANEL_W, PANEL_H);
            ui.render(&mut cv);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
            let _ = std::fs::create_dir_all(&dir);
            let _ = crate::store::write(&dir.join(format!("i-{}.png", key).to_lowercase()), &crate::store::png_of(&cv));
        }
    }

    /// Power key: "cancel power-down" to the MCU on every press, dialog toggles, buttons set the action.
    #[test]
    fn power_key_dialog() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.key(key::POWER);
        assert!(ui.power_dlg);
        assert_eq!(ui.panel_tx, vec![crate::board::PanelCmd::CancelPowerDown]);
        // second press cancels, again with the cancel frame
        ui.key(key::POWER);
        assert!(!ui.power_dlg && ui.power_action.is_none());
        assert_eq!(ui.panel_tx.len(), 2);
        // tap on "Power off"
        ui.key(key::POWER);
        let b = ui.pwr_layout().1[0];
        ui.tap(b.x + b.w / 2, b.y + b.h / 2);
        assert_eq!(ui.power_action, Some(PowerAction::Off));
        assert!(!ui.power_dlg);
        // tap outside: nothing happens
        ui.power_action = None;
        ui.key(key::POWER);
        ui.tap(5, 5);
        assert!(!ui.power_dlg && ui.power_action.is_none());
        // any other key closes the dialog without acting
        ui.key(key::POWER);
        ui.key(key::RUNSTOP);
        assert!(!ui.power_dlg && ui.power_action.is_none());
        let b = ui.pwr_layout().1[1];
        ui.key(key::POWER);
        ui.tap(b.x + 5, b.y + 5);
        assert_eq!(ui.power_action, Some(PowerAction::Restart));
    }

    /// Every trigger type with its starting values triggers on the demo signals.
    #[test]
    fn every_trigger_type_triggers_on_the_demo() {
        use super::*;
        for &t in TRIG_TYPES.iter() {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            for c in 0..NCH { ui.m.ch[c].on = true; }
            // the starting values are for these sources of the demo signals
            (ui.m.trig_src, ui.m.trig_src2) = match t { TrigType::Pulse => (3, 1), TrigType::Video | TrigType::Window => (1, 3), TrigType::Delay => (0, 3), TrigType::SetupHold => (2, 3), TrigType::NthEdge => (2, 3), _ => (0, 3) };
            ui.set_trig_type(t);
            ui.m.trig_mode = TrigMode::Normal;
            let mut hits = 0;
            for _ in 0..20 {
                ui.step(0.033);
                if ui.m.status == TrigStatus::Triggered {
                    hits += 1;
                }
            }
            assert!(hits >= 5, "{:?}: triggered in {} of 20 frames", t, hits);
        }
    }

    #[test]
    fn heat_palette_runs_blue_to_red() {
        let p = super::heat_palette();
        let c = |x: u32| ((x >> 16) & 255, (x >> 8) & 255, x & 255);
        let (r0, g0, b0) = c(p[0]);
        let (r1, g1, b1) = c(p[128]);
        let (r2, g2, b2) = c(p[255]);
        assert!(b0 > r0 && b0 > g0, "rare = blue");
        assert!(g1 > r1 && g1 > b1, "middle = green");
        assert!(r2 > g2 && r2 > b2, "frequent = red");
    }

    /// Colour grade in the normal and the zoomed view: trace pixels take palette colours.
    #[test]
    fn colour_grade_renders() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.ch[1].on = true;
        ui.m.color_grade = true;
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        for zoom in [false, true] {
            ui.set_zoom(zoom);
            for _ in 0..5 {
                ui.step(0.033);
            }
            ui.render(&mut cv);
            let pal = heat_palette();
            let a = ui.area;
            let ph = if zoom { &ui.phz } else { &ui.ph };
            let mut direct = Canvas::new(W, H);
            ui.draw_graded(&mut direct, a, ph, if zoom { ui.vmaxz_cg } else { ui.vmax_cg }, false);
            let mut active = 0;
            for y in 0..ph.h {
                for x in 0..ph.w {
                    let expected = (0..NCH).any(|c| ui.m.ch[c].on && ui.shows(c) && ph.map[c][y * ph.w + x] != 0);
                    let graded = pal.contains(&direct.px[((a.y + y as i32) * W + a.x + x as i32) as usize]);
                    assert_eq!(graded, expected, "zoom {zoom}: map pixel {x},{y}");
                    active += usize::from(expected);
                }
            }
            assert!(active > 0, "the fixture must deposit trace samples");
            let hits = (a.y..a.y + a.h).flat_map(|y| (a.x..a.x + a.w).map(move |x| (x, y))).filter(|&(x, y)| pal.contains(&cv.px[(y * W + x) as usize])).count();
            // Axes and measurement labels cover a few trace pixels. The zoom
            // map can contain fewer than 1000 active pixels at this timebase.
            assert!(hits * 2 > active, "zoom {zoom}: {hits} of {active} graded pixels remain visible");
        }
    }

    /// Closing the display must not undo explicit overview/timebase changes.
    #[test]
    fn zoom_close_preserves_explicit_main_timebase_changes() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.step(0.02);
        let (ts, tf, d) = (ui.m.tscale, ui.m.tfine, ui.m.delay);
        ui.set_zoom(true);
        ui.m.tscale = if ts > 3 { ts - 3 } else { ts + 3 };
        ui.m.tfine = 0.97;
        ui.m.delay = d + 2.5e-4;
        let changed = (ui.m.tscale, ui.m.tfine, ui.m.delay);
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        let row = ui.zoom_readout();
        let c = ui.zoom_close();
        assert!(c.x >= row.x && c.x + c.w <= row.x + row.w && c.y >= row.y && c.y + c.h <= row.y + row.h, "close {:?} readout {:?}", c, row);
        let (o, z) = ui.split_zoom(ui.grid);
        assert!(c.y >= o.y + o.h && c.y + c.h <= z.y, "close {:?} between {:?} and {:?}", c, o, z);
        assert_ne!(cv.px[((c.y + c.h / 2) * W + c.x + c.w / 2) as usize], BG);
        ui.tap(c.x + c.w / 2, c.y + c.h / 2);
        assert!(!ui.m.zoom);
        assert_eq!((ui.m.tscale, ui.m.tfine, ui.m.delay), changed);
        let _ = tf;
    }

    /// Mask from CH1 (square): passes; from CH4 (PWM with sweeping duty) with a tight X tolerance: fails, and stops.
    #[test]
    fn mask_test_counts_and_stops() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.step(0.033);
        ui.create_mask();
        assert!(ui.m.mask_on && ui.mask.is_some());
        for _ in 0..10 {
            ui.step(0.033);
        }
        assert!(ui.mask_pass >= 30 && ui.mask_fail == 0, "pass {} fail {}", ui.mask_pass, ui.mask_fail);
        ui.m.ch[3].on = true;
        ui.adjust(Item::MaskSrc, 3);
        assert!(ui.mask.is_none() && !ui.m.mask_on);
        ui.step(0.033);
        ui.m.mask_x = 0.04;
        ui.create_mask();
        ui.m.mask_stop = true;
        for _ in 0..100 {
            ui.step(0.033);
            if ui.m.run == RunState::Stop {
                break;
            }
        }
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.mask_fail, 1);
    }

    #[test]
    fn math_category_filters_the_operation_list() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.select_option(Item::MathFam, "Logic");
        let ops = ui.options(Item::MathOpI);
        assert!(ops.contains(&"AND".into()) && ops.contains(&"XOR".into()));
        assert!(!ops.iter().any(|s| s == "FFT" || s == "A+B"));
        ui.select_option(Item::MathOpI, "AND");
        assert_eq!(ui.m.math, MathOp::And);
        ui.select_option(Item::MathFam, "Convert");
        assert_eq!(ui.m.math, MathOp::A2d);
        ui.select_option(Item::MathFam, "Arithmetic");
        assert!(ui.options(Item::MathOpI).contains(&"Conv".into()));
    }

    #[test]
    fn mask_from_ref_invert_save_and_load() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("oos-mask-{}", std::process::id()));
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.store_dir = dir.clone();
        ui.store_dest = crate::store::StoreDest::Local;
        ui.step(0.033);
        let rec = ui.last[0].as_ref().unwrap();
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: rec.t0, dt: rec.dt, v: rec.ch[0].clone().into(), vpd: 1.0, offset: 0.0, on: true });
        ui.activate(Item::MaskFromRef);
        assert!(ui.m.mask_on && ui.mask.is_some());
        ui.adjust(Item::MaskInvert, 1);
        assert!(ui.m.mask_inv && ui.mask.as_ref().unwrap().invert);
        ui.activate(Item::MaskSave);
        assert!(dir.join("mask-001.txt").is_file());
        assert!(dir.join("mask1.txt").is_file());
        let t = std::fs::read_to_string(dir.join("mask-001.txt")).unwrap();
        assert!(t.starts_with("# OOS mask\n") && t.contains("inv=1"));
        ui.activate(Item::MaskClear);
        assert!(ui.mask.is_none() && !ui.m.mask_on);
        ui.m.mask_inv = false;
        ui.activate(Item::MaskLoad);
        assert!(ui.m.mask_on && ui.m.mask_inv && ui.mask_ref.as_ref().unwrap().len() > 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fast_frame_save_and_load_and_local_dest() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("oos-ff-{}", std::process::id()));
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.store_dir = dir.clone();
        ui.store_dest = crate::store::StoreDest::Local;
        for _ in 0..4 {
            ui.step(0.033);
        }
        assert!(ui.history.len() >= 3);
        ui.activate(Item::StoreFfSave);
        let p = dir.join("frame-001.bin");
        assert!(p.is_file());
        let n = ui.history.len();
        ui.history.clear();
        ui.last[0] = None;
        ui.activate(Item::StoreFfLoad);
        assert_eq!(ui.history.len(), n);
        assert!(ui.hist_pos.is_some());
        assert_eq!(ui.m.run, RunState::Stop);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Save setup, waveform CSV; recall restores the setup but not the run state.
    #[test]
    fn storage_save_and_recall() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("oos-ui-store-{}", std::process::id()));
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.store_dir = dir.join("sub");
        ui.m.ch[2].on = true;
        ui.m.tscale -= 2;
        ui.m.color_grade = true;
        ui.step(0.033);
        let saved = ui.m.clone();
        ui.activate(Item::StoreSaveSetup);
        ui.activate(Item::StoreCsv);
        let csv = std::fs::read_to_string(dir.join("sub/wave-001.csv")).unwrap();
        assert!(csv.starts_with("Time (s),CH1 (V),CH3 (V)\n"));
        assert_eq!(csv.lines().count(), 1 + ui.last[0].as_ref().unwrap().ch[0].len());
        ui.key(key::DEFAULT);
        ui.m.run = RunState::Stop;
        ui.activate(Item::StoreRecall);
        assert_eq!(ui.m.run, RunState::Stop);
        let mut want = saved;
        (want.run, want.status) = (ui.m.run, ui.m.status);
        assert_eq!(ui.m, want);
        ui.store_slot = 2;
        ui.activate(Item::StoreRecall);
        assert!(ui.toast.as_ref().unwrap().0.starts_with("Recall failed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Break it catches: a hardware recording that starts without a new number for the thread, stops on the count of
    // records that reached the screen instead of the thread's read-back, browses the RAM history instead of playing
    // the segment from the FPGA, or loses the stamps in the status line and the table. The thread is played by hand.
    #[test]
    fn hardware_segments_record_in_the_thread_and_browse_on_demand() {
        use super::*;
        use crate::board::SegPhase;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap();
        (ui.m.segments, ui.m.seg_hw, ui.m.seg_ivl) = (100, true, 1e-3);
        ui.m.run = RunState::Run;
        ui.step(0.02);
        {
            let g = shared.lock().unwrap();
            assert_eq!((g.seg.run, g.seg.running), (1, true));
            let w = g.want.as_ref().unwrap();
            assert_eq!(w.segments, Some(oos_hal::dho::seq::Segments { n: 100, interval: 1e-3f32 as f64 }));
        }
        // the thread records: no frame, the scope keeps running and shows the progress
        {
            let mut g = shared.lock().unwrap();
            (g.seg.done_run, g.seg.phase, g.seg.total, g.seg.recorded) = (1, SegPhase::Recording, 100, 40);
        }
        for _ in 0..30 {
            ui.step(0.02);
        }
        assert_eq!(ui.m.run, RunState::Run);
        assert_eq!(ui.seg_view.unwrap().recorded, 40);
        // read back: 100 stamps 10 us apart, the frames went through the queue; done once the queue is empty
        {
            let mut g = shared.lock().unwrap();
            (g.seg.phase, g.seg.recorded, g.seg.read) = (SegPhase::Done, 100, 100);
            g.seg.times = (0..100).map(|k| Some(k as f64 * 10e-6)).collect();
        }
        ui.step(0.02);
        assert_eq!(ui.m.run, RunState::Stop);
        assert_eq!(ui.toast.as_ref().unwrap().0, "100 segments recorded");
        // browse: back from the newest asks the thread for segment 99, then 98
        ui.history_step(-1);
        assert_eq!(shared.lock().unwrap().seg.want, Some(98));
        assert_eq!(ui.seg_label(98), "Segment 99/100 \u{b7} 980 \u{b5}s \u{b7} \u{394}t 10.0 \u{b5}s");
        let v: Vec<f32> = (0..1000).map(|k| ((k as f32) * 0.1).sin()).collect();
        let f = crate::board::Frame { seq: 1, dt: 1e-9, pre: 500, ch: [v, Vec::new(), Vec::new(), Vec::new()], acq: (1e9, 1000), ..Default::default() };
        shared.lock().unwrap().seg.shown = Some((98, f));
        ui.step(0.02);
        assert_eq!(ui.seg_pos, Some(98));
        assert!(ui.last[0].is_some());
        assert!(shared.lock().unwrap().seg.shown.is_none());
        // the table and SCPI have every stamp
        let rows = ui.table_rows(tables::TableKind::Segments);
        assert_eq!(rows.len(), 100);
        assert_eq!((rows[0].text.as_str(), rows[3].ok), ("", true));
        assert!(rows[3].text.starts_with("\u{394}t 10"));
        assert!(ui.seg_times_text().starts_with("0e0,1e-5,2e-5"));
        // RUN again: a new recording number
        ui.m.run = RunState::Run;
        ui.step(0.02);
        assert_eq!(shared.lock().unwrap().seg.run, 2);
        assert!(ui.seg_times.is_empty());
    }

    // Break it catches: the software limit applied to hardware segments, or the hardware limit to the software history.
    #[test]
    fn segment_counts_follow_the_limit_of_the_set_up() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared, seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x as f64 - 1e-7).abs() < 1e-10).unwrap();
        ui.m.segments = 10;
        assert_eq!(*ui.seg_options().last().unwrap(), SEG_MAX);
        ui.adjust(Item::AcqSegHw, 1);
        // 100 ns/div, 1 channel: 1408 points -> 256 bursts -> 61,440 segments
        let lim = board().seg_max(&ui.m).unwrap();
        assert_eq!(lim, 61_440);
        let o = ui.seg_options();
        assert_eq!(&o[o.len() - 2..], &[50_000, 61_440]);
        for want in ["61440", "Off", "20000", "10"] {
            ui.select_option(Item::AcqSeg, want);
            assert_eq!(ui.value(Item::AcqSeg), want);
        }
        ui.select_option(Item::AcqSeg, "61440");
        // back to software: the history limit
        ui.adjust(Item::AcqSegHw, 1);
        assert_eq!((ui.m.segments, ui.value(Item::AcqSeg)), (SEG_MAX, "1000".to_string()));
        let o = ui.options(Item::AcqSegIvl);
        assert_eq!(o.len(), SEG_IVLS.len());
        for want in o.iter().rev().chain(o.iter()) {
            ui.select_option(Item::AcqSegIvl, want);
            assert_eq!(&ui.value(Item::AcqSegIvl), want);
        }
        assert_eq!(ui.m.seg_ivl, 1.0);
        // roll: no hardware segments
        ui.m.seg_hw = true;
        ui.m.tscale = tscales().iter().position(|&x| x >= 0.2).unwrap();
        assert!(board().seg_max(&ui.m).is_none() && !ui.seg_hw_on());
    }

    #[test]
    fn segment_scpi() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let cv = Canvas::new(PANEL_W, PANEL_H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut q = |ui: &mut Ui, c: &str| String::from_utf8(scpi.exec_line(ui, &cv, c)).unwrap();
        q(&mut ui, ":ACQ:SEGM 5000");
        q(&mut ui, ":ACQ:SEGM:HARD ON");
        q(&mut ui, ":ACQuire:SEGMents:INTerval 1e-4");
        assert_eq!((ui.m.segments, ui.m.seg_hw, ui.m.seg_ivl), (5000, true, 1e-4));
        assert_eq!((q(&mut ui, ":ACQ:SEGM?"), q(&mut ui, ":ACQ:SEGM:HARD?")), ("5000".into(), "1".into()));
        assert!(q(&mut ui, ":ACQ:SEGM:INT?").starts_with("1.000000E-04"));
        q(&mut ui, ":ACQ:SEGM:INT -3");
        q(&mut ui, ":ACQ:SEGM OFF");
        assert_eq!((ui.m.segments, ui.m.seg_ivl), (0, 0.0));
        ui.seg_times = vec![Some(0.0), None, Some(2e-3)];
        assert_eq!(q(&mut ui, ":ACQ:SEGM:TIM?"), "0e0,9.9E37,2e-3");
    }

    /// Centre of a rectangle (for taps).
    fn mid(r: super::Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// Fast display path: every queued hardware frame is drawn, the newest also becomes the record.
    #[test]
    fn fast_path_draws_every_queued_frame() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        // 1 us/div, 12.5 k points at 0.8 ns, a 200 kHz sine through 0 V rising at the trigger point
        let (n, dt, pre) = (12_500usize, 0.8e-9, 6250usize);
        let frame = |seq: u64| {
            let v: Vec<f32> = (0..n).map(|k| (2.0 * std::f64::consts::PI * 200e3 * (k as f64 - pre as f64 + 0.3) * dt).sin() as f32).collect();
            crate::board::Frame { seq, dt, pre, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (1.25e9, n), stamp: 0, la: None, fixed_ti: None, hw_avg: false, basic: Default::default() }
        };
        shared.lock().unwrap().queue.extend((1..=10).map(frame));
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        ui.m.trig_level = 0.0;
        ui.m.ch[0].offset = 0.0;
        assert!(ui.fast_ok());
        ui.step(0.05);
        assert_eq!(ui.wfm_total, 10);
        assert!(shared.lock().unwrap().queue.is_empty());
        assert_eq!(ui.m.status, TrigStatus::Triggered);
        // the zero crossing sits at the centre of the plot in every frame: the centre column is lit at the middle row
        let (w, h) = (ui.ph.w, ui.ph.h);
        let lit: Vec<usize> = (0..h).filter(|&r| ui.ph.map[0][r * w + w / 2] > 0).collect();
        assert!(lit.iter().any(|&r| (r as i32 - h as i32 / 2).abs() <= 2), "{:?}", lit);
        // averaging needs every record: back to the record path
        ui.m.acq = AcqMode::Average;
        assert!(!ui.fast_ok());
    }

    /// The fast path on several cores draws the same maps as one frame after the other, dense and sparse, four
    /// channels; with it on, the record loop takes one frame a screen and leaves the rest of the queue to it.
    #[test]
    fn fast_path_on_several_cores_draws_as_one_by_one() {
        use super::*;
        for &(tdiv, n, dt) in &[(1e-6, 12_500usize, 0.8e-9), (1e-7, 1_000, 3.2e-9)] {
            let pre = n / 2;
            let frame = |seq: u64| {
                let ch: [Vec<f32>; NCH] = std::array::from_fn(|c| {
                    (0..n).map(|k| (2.0 * std::f64::consts::PI * 2e6 * (c + 1) as f64 * (k as f64 - pre as f64 + 0.1 * seq as f64) * dt).sin() as f32).collect()
                });
                crate::board::Frame { seq, dt, pre, ch, roll: false, envelope: false, scan: false, partial: false, acq: (1.0 / dt, n), stamp: 0, la: None, fixed_ti: None, hw_avg: false, basic: Default::default() }
            };
            let shared: crate::board::HwShared = Default::default();
            let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
            ui.m.tscale = tscales().iter().position(|&x| (x as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
            ui.m.trig_level = 0.0;
            for c in 0..NCH {
                ui.m.ch[c].on = true;
                ui.m.ch[c].offset = 0.0;
            }
            assert!(ui.fast_ok());
            let frames: Vec<_> = (1..=40).map(frame).collect();
            let (w, h) = (ui.ph.w, ui.ph.h);
            let mut want = crate::wave::Phosphor::new(w, h);
            for f in &frames {
                let f = std::sync::Arc::new(f.clone());
                let p = fast_plan(&ui.m, w, h, &f, [false; NCH]);
                for c in 0..NCH {
                    fast_deposit(&ui.m, w, h, c, &mut want.map[c], 0, &f, &p.plan);
                }
            }
            ui.ph.clear();
            assert_eq!(ui.fast_draw_all(frames.clone()), 40);
            for c in 0..NCH {
                assert!(want.map[c].iter().any(|&x| x > 0), "{} s/div CH{}: nothing drawn", tdiv, c + 1);
                assert!(ui.ph.map[c] == want.map[c], "{} s/div CH{}: maps differ", tdiv, c + 1);
            }
            // through `step`: 41 queued frames, 40 by the fast path, the newest as the record; nothing left
            shared.lock().unwrap().queue.extend((1..=41).map(frame));
            // the take handed to the thread is the one before the step: the step then adapts it to the time it
            // measured, which under load (tests in parallel) is less
            let take = ui.fast_max() + 1;
            ui.step(0.05);
            assert_eq!(ui.wfm_total, 41);
            assert_eq!(ui.records, 1);
            assert!(shared.lock().unwrap().queue.is_empty());
            // the thread is told how many of a batch to decode; the record path wants them all
            assert_eq!(shared.lock().unwrap().take, take);
            ui.m.acq = AcqMode::Average;
            ui.step(0.05);
            assert_eq!(shared.lock().unwrap().take, 0);
            ui.m.acq = AcqMode::Normal;
        }
    }

    /// One channel in bands of rows on several cores, lines and dots: the same map as the whole map at once.
    #[test]
    fn fast_path_bands_draw_as_the_whole_map() {
        use super::*;
        for &(tdiv, n, dt) in &[(1e-6, 12_500usize, 0.8e-9), (1e-7, 1_000, 3.2e-9)] {
            for dots in [false, true] {
                let pre = n / 2;
                let frames: Vec<crate::board::Frame> = (1..=24u64)
                    .map(|seq| {
                        // a sine with a frame-dependent phase and a spike, off the screen at the top in some frames
                        let mut v: Vec<f32> = (0..n).map(|k| 3.0 * (2.0 * std::f64::consts::PI * 1.3e6 * (k as f64 - pre as f64 + 0.37 * seq as f64) * dt).sin() as f32).collect();
                        v[pre + 7] = if seq % 3 == 0 { 9.0 } else { -1.5 };
                        crate::board::Frame { seq, dt, pre, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (1.0 / dt, n), stamp: 0, la: None, fixed_ti: None, hw_avg: false, basic: Default::default() }
                    })
                    .collect();
                let mut ui = Ui::new(Source::Hw { shared: Default::default(), seen: 0 });
                ui.m.tscale = tscales().iter().position(|&x| (x as f64 - tdiv).abs() < tdiv * 1e-3).unwrap();
                ui.m.trig_level = 0.0;
                ui.m.ch[0].offset = 0.0;
                ui.m.dots = dots;
                let (w, h) = (ui.ph.w, ui.ph.h);
                let mut want = vec![0u16; w * h];
                for f in &frames {
                    let f = std::sync::Arc::new(f.clone());
                    let p = fast_plan(&ui.m, w, h, &f, [false; NCH]);
                    fast_deposit(&ui.m, w, h, 0, &mut want, 0, &f, &p.plan);
                }
                ui.ph.clear();
                ui.fast_draw_all(frames);
                assert!(want.iter().filter(|&&x| x > 0).count() > w / 2, "{} s/div dots {}: little drawn", tdiv, dots);
                assert!(ui.ph.map[0] == want, "{} s/div dots {}: maps differ", tdiv, dots);
            }
        }
    }

    /// Statistics of the voltage measurements over every frame of the fast path, not only over the records; the
    /// values of one frame equal what the measurements give on it.
    #[test]
    fn statistics_take_every_frame_of_the_fast_path() {
        use super::*;
        let (n, dt, pre) = (12_500usize, 0.8e-9, 6250usize);
        // frame k: a sine of amplitude 1 + k/10 around 0.25 V
        let frame = |seq: u64| {
            let a = 1.0 + seq as f64 / 10.0;
            let v: Vec<f32> = (0..n).map(|k| (0.25 + a * (2.0 * std::f64::consts::PI * 2e6 * (k as f64 - pre as f64) * dt).sin()) as f32).collect();
            crate::board::Frame { seq, dt, pre, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (1.0 / dt, n), stamp: 0, la: None, fixed_ti: None, hw_avg: false, basic: Default::default() }
        };
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        ui.m.trig_level = 0.0;
        ui.m.ch[0].offset = 0.0;
        ui.m.meas = vec![(Meas::Vpp, WaveId::Ch(0)), (Meas::Vavg, WaveId::Ch(0)), (Meas::Vrms, WaveId::Ch(0)), (Meas::StdDev, WaveId::Ch(0)), (Meas::Freq, WaveId::Ch(0))];
        ui.m.meas2.clear();
        // the first record sets up the rows
        shared.lock().unwrap().queue.push_back(frame(0));
        ui.step(0.05);
        assert_eq!(ui.mstat.len(), 5);
        assert_eq!(ui.mstat[0].n, 1);
        // 20 more: 19 by the fast path, the newest as the record
        shared.lock().unwrap().queue.extend((1..=20).map(frame));
        ui.step(0.05);
        assert_eq!(ui.mstat[0].n, 21, "Pk-pk over every frame");
        assert_eq!(ui.mstat[4].n, 2, "Freq only over the records");
        // the largest Pk-pk is that of frame 20 (amplitude 3), the smallest that of frame 0
        assert!((ui.mstat[0].max - 6.0).abs() < 0.01 && (ui.mstat[0].min - 2.0).abs() < 0.01, "{} {}", ui.mstat[0].min, ui.mstat[0].max);
        // a frame's fold gives the values of the measurements on the same samples
        let f = frame(7);
        let fo = crate::fastdraw::Fold::of(&f.ch[0][1000..9000]);
        let an = oos_dsp::measure::analyze(&f.ch[0][1000..9000], dt);
        for k in [Meas::Vmax, Meas::Vmin, Meas::Vpp, Meas::Vavg, Meas::Vrms, Meas::StdDev, Meas::VrmsAc] {
            let (a, b) = (fo.get(k).unwrap(), an.get(k));
            assert!((a - b).abs() < 1e-5, "{:?}: fold {} measure {}", k, a, b);
        }
        assert!(fo.get(Meas::Freq).is_none());
    }

    // Break it catches: the fast path folding Mean, RMS, Std dev and AC RMS of min/max pairs into the statistics
    // (deep pairs: the value shown is NaN while the statistics count pair values), or anything of live-compression
    // pairs (only their raw newest frame counts), where the deposit workers (`framestats::fold_frame`) leave them out
    #[test]
    fn statistics_of_the_fast_path_leave_out_what_pairs_cannot_give() {
        use super::*;
        crate::deepmeas::force(Some(false));
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        ui.m.trig_level = 0.0;
        ui.m.ch[0].offset = 0.0;
        for c in 1..NCH {
            ui.m.ch[c].on = false;
        }
        ui.m.meas = vec![(Meas::Vavg, WaveId::Ch(0)), (Meas::Vmax, WaveId::Ch(0)), (Meas::StdDev, WaveId::Ch(0))];
        ui.m.meas2.clear();
        let n = 20_000usize;
        let dt = 1e-5 * 1.2 / n as f64;
        // a fifth of the time high: the mean (-0.6) is far from that of the pairs (0)
        let frame = |seq: u64, envelope: bool, depth: usize| {
            let v: Vec<f32> = (0..n).map(|k| if (k + n / 2) % 1000 < 200 { 1.0 } else { -1.0 }).collect();
            let ch = [v, Vec::new(), Vec::new(), Vec::new()];
            crate::board::Frame { seq, dt, pre: n / 2, ch, envelope, acq: (1.0 / dt, depth), ..Default::default() }
        };
        let counts = |ui: &Ui| ui.mstat.iter().map(|s| s.n).collect::<Vec<_>>();
        // deep pairs: the newest is the record (Max measured), the fast path draws the other nine
        shared.lock().unwrap().queue.push_back(frame(0, true, 10_000_000));
        ui.step(0.05);
        assert_eq!(counts(&ui), [0, 1, 0]);
        shared.lock().unwrap().queue.extend((1..=10).map(|s| frame(s, true, 10_000_000)));
        ui.step(0.05);
        assert_eq!(counts(&ui), [0, 11, 0], "Max of every frame, no Mean or Std dev of pairs");
        // live-compression pairs: a raw record, then pair frames; only the raw one counts
        ui.clear_display();
        ui.m.run = RunState::Run;
        shared.lock().unwrap().queue.push_back(frame(11, false, n));
        ui.step(0.05);
        assert_eq!(counts(&ui), [1, 1, 1]);
        shared.lock().unwrap().queue.extend((12..=21).map(|s| frame(s, true, n)));
        ui.step(0.05);
        assert_eq!(counts(&ui), [1, 1, 1], "values of live-compression pairs counted");
        crate::deepmeas::force(None);
    }

    /// The fast path takes what fits into its time budget: the newest frames, the older ones are counted as dropped.
    #[test]
    fn fast_path_keeps_to_its_budget() {
        use super::*;
        let shared: crate::board::HwShared = Default::default();
        let frame = |seq: u64| crate::board::Frame { seq, dt: 0.8e-9, pre: 50, ch: [vec![0.0; 100], Vec::new(), Vec::new(), Vec::new()], ..Default::default() };
        shared.lock().unwrap().queue.extend((1..=100).map(frame));
        let mut src = Source::Hw { shared: shared.clone(), seen: 0 };
        let got = src.drain_newest(30);
        assert_eq!(got.iter().map(|f| f.seq).collect::<Vec<_>>(), (70..=99).collect::<Vec<_>>());
        let g = shared.lock().unwrap();
        assert_eq!((g.queue.len(), g.queue[0].seq, g.dropped), (1, 100, 69));
        drop(g);
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert_eq!(ui.fast_max(), crate::board::QUEUE_MAX);
        // nothing else in the screen: 28 ms; a slow screen leaves at least 5
        assert_eq!(ui.fast_budget(), 28.0);
        ui.t_render = 40.0;
        assert_eq!(ui.fast_budget(), 5.0);
        // the take settles where the time meets the budget, with a fixed 3 ms a screen and 0.3 ms a frame
        ui.t_render = 0.0;
        // a queue always fuller than the take, 3 ms a screen and 0.3 ms a frame: settles where the time meets 28 ms
        let mut take = crate::board::QUEUE_MAX as f32;
        for _ in 0..40 {
            let n = (take as usize).clamp(4, crate::board::QUEUE_MAX);
            take = fast_take_next(take, n, 3.0 + 0.3 * n as f32, 28.0);
        }
        assert!((take - 83.3).abs() < 1.0, "{}", take);
        // from below, too
        let mut low = 4.0;
        for _ in 0..40 {
            let n = (low as usize).clamp(4, crate::board::QUEUE_MAX);
            low = fast_take_next(low, n, 3.0 + 0.3 * n as f32, 28.0);
        }
        assert!((low - 83.3).abs() < 1.0, "{}", low);
        // a screen with a few frames (a burst still to come) leaves the take alone; one over the budget lowers it
        assert_eq!(fast_take_next(take, 6, 4.8, 28.0), take);
        assert!(fast_take_next(take, 6, 40.0, 28.0) < take);
    }

    /// The trigger level line shows for a moment after the level changes by any way (not only while dragging it);
    /// with AC or LF reject coupling there is no level mark (the level counts from the signal's mean).
    #[test]
    fn level_line_after_a_change_and_no_mark_with_ac_coupling() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.step(0.05);
        assert!(!ui.level_line_on(), "no line at start-up");
        ui.adjust(Item::TrigLevel, 3);
        ui.step(0.05);
        assert!(ui.level_line_on());
        for _ in 0..40 {
            ui.step(0.05);
        }
        assert!(!ui.level_line_on(), "gone after {} s", LEVEL_LINE_S);
        // the mark: drawn in the accent colour at the right edge of the plot, at the level's height
        let mark = |ui: &mut Ui| {
            let mut cv = Canvas::new(ui.area.w, ui.area.h + 200);
            ui.render(&mut cv);
            let g = ui.grid;
            let x = g.x + g.w - MARK_W + 16;
            (g.y..g.y + g.h).filter(|&y| cv.px[(y * cv.w + x) as usize] == ACCENT).count()
        };
        ui.m.trig_coup = TrigCoup::Dc;
        assert!(mark(&mut ui) > 3, "DC: the T mark is there");
        ui.m.trig_coup = TrigCoup::Ac;
        assert_eq!(mark(&mut ui), 0, "AC: no T mark");
        ui.m.trig_coup = TrigCoup::LfReject;
        assert_eq!(mark(&mut ui), 0, "LF reject: no T mark");
    }

    // Break it catches: an FPGA-averaged frame blended once more in software, the software average lost for frames
    // the FPGA did not average, no clear request on CLEAR / RUN / SINGLE, hardware averaging where the board falls back
    // (roll, a count the FPGA does not take, the switch off).
    #[test]
    fn hardware_average_bypasses_the_blend_and_restarts() {
        use super::*;
        use oos_hal::dho::acq::Mode;
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        // 100 us/div, Auto depth: 62.5 MSa/s
        ui.m.tscale = tscales().iter().position(|&x| (x - 100e-6).abs() < 1e-9).unwrap();
        (ui.m.acq, ui.m.avg_count, ui.m.avg_hw) = (AcqMode::Average, 16, true);
        ui.m.run = RunState::Run;
        let flat = |seq: u64, v: f32, hw_avg: bool| crate::board::Frame { seq, dt: 0.8e-9, pre: 6250, ch: [vec![v; 12_500], Vec::new(), Vec::new(), Vec::new()], acq: (1.25e9, 12_500), hw_avg, ..Default::default() };
        let mid = |ui: &Ui| {
            let c = &ui.last[0].as_ref().unwrap().ch[0];
            c[c.len() / 2]
        };
        ui.step(0.02);
        let c0 = {
            let g = shared.lock().unwrap();
            assert_eq!(g.want.as_ref().unwrap().mode, Mode::Average { count: 16 });
            g.avg_clear
        };
        assert!(board().avg_hw(&ui.m));
        assert!(ui.items(MenuId::Acquire).contains(&Item::AcqAvgHw));
        for (k, v) in [1.0, 0.0].into_iter().enumerate() {
            shared.lock().unwrap().queue.push_back(flat(k as u64 + 1, v, true));
            ui.step(0.02);
        }
        assert_eq!(mid(&ui), 0.0);
        // a frame the FPGA did not average is blended: 0 + (1 - 0) / 16
        shared.lock().unwrap().queue.push_back(flat(3, 1.0, false));
        ui.step(0.02);
        assert!((mid(&ui) - 1.0 / 16.0).abs() < 1e-6, "{}", mid(&ui));
        // CLEAR, then STOP and RUN, then SINGLE: each a new clear request
        ui.clear_display();
        ui.step(0.02);
        let c1 = shared.lock().unwrap().avg_clear;
        assert!(c1 > c0);
        ui.m.run = RunState::Stop;
        ui.step(0.02);
        ui.m.run = RunState::Run;
        ui.step(0.02);
        let c2 = shared.lock().unwrap().avg_clear;
        assert!(c2 > c1);
        ui.m.trig_mode = TrigMode::Single;
        ui.step(0.02);
        assert!(shared.lock().unwrap().avg_clear > c2);
        ui.m.trig_mode = TrigMode::Auto;
        // fall-backs to the software average
        ui.m.avg_count = 24;
        assert!(!board().avg_hw(&ui.m));
        ui.m.avg_count = 16;
        ui.m.avg_hw = false;
        assert!(!board().avg_hw(&ui.m));
        ui.m.avg_hw = true;
        ui.m.tscale = tscales().iter().position(|&x| x >= 0.2).unwrap();
        assert!(!board().avg_hw(&ui.m));
        // 1 us/div at 1.25 GSa/s: the FPGA averages at every rate (0x1004 bit 2, B1)
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        assert!(board().avg_hw(&ui.m));
    }

    #[test]
    fn dvm_lane_scpi() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let cv = Canvas::new(PANEL_W, PANEL_H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut q = |ui: &mut Ui, c: &str| String::from_utf8(scpi.exec_line(ui, &cv, c)).unwrap();
        assert_eq!(q(&mut ui, ":SYST:DEB:DVM:LANE?"), "AUTO");
        q(&mut ui, ":SYSTem:DEBug:DVM:LANE 3");
        assert_eq!((q(&mut ui, ":SYST:DEB:DVM:LANE?"), ui.dvm_lane), ("3".into(), Some(3)));
        q(&mut ui, ":SYST:DEB:DVM:LANE 4");
        assert_eq!(ui.dvm_lane, Some(3));
        assert!(q(&mut ui, ":SYST:ERR?").starts_with("-224"));
        q(&mut ui, ":SYST:DEB:DVM:LANE auto");
        assert_eq!(ui.dvm_lane, None);
    }

    #[test]
    fn average_scpi() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let cv = Canvas::new(PANEL_W, PANEL_H);
        let mut scpi = crate::scpi::Scpi::open(0);
        let mut q = |ui: &mut Ui, c: &str| String::from_utf8(scpi.exec_line(ui, &cv, c)).unwrap();
        assert_eq!(q(&mut ui, ":ACQ:TYPE?"), "NORM");
        q(&mut ui, ":ACQuire:TYPE AVERages");
        q(&mut ui, ":ACQ:AVER 256");
        q(&mut ui, ":ACQ:AVER:HARD ON");
        assert_eq!((ui.m.acq, ui.m.avg_count, ui.m.avg_hw), (AcqMode::Average, 256, true));
        assert_eq!((q(&mut ui, ":ACQ:TYPE?"), q(&mut ui, ":ACQ:AVER?"), q(&mut ui, ":ACQ:AVER:HARD?")), ("AVER".into(), "256".into(), "1".into()));
        // a count the menu does not offer snaps down to the next one it does (PR #8, as the stock firmware); a bogus type
        // leaves the type alone
        q(&mut ui, ":ACQ:AVER 100");
        q(&mut ui, ":ACQ:TYPE BOGUS");
        assert_eq!((ui.m.acq, ui.m.avg_count), (AcqMode::Average, 64));
        q(&mut ui, ":ACQ:AVER:HARD OFF");
        q(&mut ui, ":ACQ:TYPE PEAK");
        assert_eq!((ui.m.acq, ui.m.avg_hw), (AcqMode::Peak, false));
    }

    /// The row of item `it` in the open menu dialog.
    fn row_of(ui: &super::Ui, it: super::Item) -> super::Row {
        ui.dlg_layout().expect("menu open").rows.into_iter().find(|r| r.it == it).expect("row")
    }

    /// The compact 30px footer returns space to the plot. Menus keep 40px rows.
    #[test]
    fn layout_plot_size_and_touch_targets() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.layout = crate::model::Layout::Minimal;
        ui.relayout();
        // Minimal: one measurement row under the plot
        assert_eq!((ui.grid.x, ui.grid.y, ui.grid.w, ui.grid.h), (0, 40, 1024, 496));
        ui.m.meas.clear();
        ui.relayout();
        assert_eq!((ui.grid.w, ui.grid.h), (1024, 520));
        // statistics: the results panel takes no room
        ui.m.meas = vec![(Meas::Freq, WaveId::Ch(0))];
        ui.stats_on = true;
        ui.relayout();
        assert_eq!((ui.grid.w, ui.grid.h), (1024, 520));
        for (_, _, r) in ui.top_buttons() {
            assert!(ui.top_hit(r).h >= 40 && r.x + r.w <= ui.net_box().x, "{:?}", r);
        }
        // sticks: their boxes beside the network box, the buttons move over and stay on the bar
        (ui.usb, ui.wlan_stick) = (true, true);
        for (_, _, r) in ui.top_buttons() {
            assert!(r.x > ui.run_pill().x + ui.run_pill().w && r.x + r.w <= ui.usb_box().x, "{:?}", r);
        }
        let u = ui.usb_box();
        assert!(u.x + u.w < ui.wlan_box().x && ui.wlan_box().x + ui.wlan_box().w < ui.net_box().x);
        ui.tap(u.x + 5, 10);
        assert!(ui.menu_open && ui.menu_root() == MenuId::Storage);
        ui.tap(u.x + 5, 10);
        ui.tap(ui.wlan_box().x + 5, 10);
        assert!(ui.menu_open && ui.menu_root() == MenuId::Network);
        ui.tap(ui.wlan_box().x + 5, 10);
        (ui.usb, ui.wlan_stick) = (false, false);
        for n in 0..=NCH {
            for c in 0..NCH {
                ui.m.ch[c].on = c < n;
            }
            let (chs, tr, trg) = ui.cards();
            assert!(trg.x + trg.w <= W && trg.w >= 222 && tr.w >= 124, "{} on", n);
            for r in chs.iter().filter(|r| r.w > 0).chain([tr, trg, Ui::MENU_BTN].iter()) {
                assert!(r.h >= 30 && r.y + r.h <= H && r.w >= 38, "{:?}", r);
            }
        }
        // Analog off, a ref on: CH pills disappear while the reference and Time remain accessible.
        for c in 0..NCH {
            ui.m.ch[c].on = false;
        }
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "DQ".into(), t0: 0.0, dt: 1e-9, v: vec![0.0, 1.0].into(), vpd: 0.2, offset: 0.0, on: true });
        let (chs, tr, trg) = ui.cards();
        assert!(chs.iter().all(|r| r.w == 0), "{:?}", chs);
        assert!(tr.w >= 124 && trg.w >= 222);
        ui.tap(145, BOTTOM.y + 15);
        assert!(ui.menu_open && ui.menu == MenuId::Ref, "a stored reference has a direct source pill");
        ui.close_menu();
        assert!(!ui.m.zoom);
        ui.tap(tr.x + 10, tr.y + 10);
        assert!(ui.menu_open && ui.menu_root() == MenuId::Horizontal);
        ui.refs[0] = None;
        for c in 0..n_ch() {
            ui.m.ch[c].on = true;
        }

        for m in [MenuId::Main, MenuId::Trigger, MenuId::Math, MenuId::MeasAdd, MenuId::Utility, MenuId::Channel(0), MenuId::Display, MenuId::Acquire] {
            ui.open_menu(m);
            let d = ui.dlg_layout().unwrap();
            let fr = ui.frame();
            assert!(d.close.h >= 40 && d.r.y >= fr.top.bottom() && d.r.bottom() <= fr.bottom.y, "{:?}", m);
            assert!(d.rows.iter().all(|r| r.ctrl.h >= 40), "{:?}", m);
        }
    }

    /// Trigger > Type and Math > Operation: the value opens a picker, picking sets the value and returns to the menu
    /// (with its Back target); Back leaves it unchanged; the knobs still step the item.
    #[test]
    fn pickers_open_and_set_the_value() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.layout = Layout::Default; // the stock dialogs: Minimal and Studio open sheets
        ui.open_menu(MenuId::Main);
        ui.activate(Item::Open(MenuId::Trigger));
        let row = row_of(&ui, Item::TrigType);
        assert!(matches!(row.ctl, Ctl::Pick));
        let (x, y) = mid(Ui::value_parts(row.ctrl).1);
        ui.tap(x, y);
        assert_eq!(ui.menu, MenuId::Pick);
        assert_eq!(ui.items(MenuId::Pick).len(), TRIG_TYPES.len());
        let k = TRIG_TYPES.iter().position(|&t| t == TrigType::Runt).unwrap();
        let (x, y) = mid(row_of(&ui, Item::PickOpt(k)).ctrl);
        ui.tap(x, y);
        assert_eq!(ui.m.trig_type, TrigType::Runt);
        assert_eq!((ui.menu, ui.parent), (MenuId::Trigger, Some(MenuId::Main)));
        assert!(ui.pick.is_none() && ui.menu_open);
        // the label opens it too; Back keeps the value
        let (x, y) = mid(row_of(&ui, Item::TrigType).label);
        ui.tap(x, y);
        assert_eq!(ui.menu, MenuId::Pick);
        let (x, y) = mid(ui.dlg_layout().unwrap().back.unwrap());
        ui.tap(x, y);
        assert_eq!((ui.menu, ui.m.trig_type), (MenuId::Trigger, TrigType::Runt));
        // Multi 2 steps the focused item, Multi 1 push opens the list
        ui.focus = 0;
        ui.now = 10.0; // a slow turn: no acceleration
        ui.key(key::MULTI2_CW);
        assert_eq!(ui.m.trig_type, TrigType::Window);
        ui.key(key::MULTI1_CW + 2);
        assert_eq!(ui.menu, MenuId::Pick);
        assert_eq!(ui.focus, TRIG_TYPES.iter().position(|&t| t == TrigType::Window).unwrap());
        ui.key(key::MULTI1_CW);
        ui.key(key::MULTI1_CW + 2);
        assert_eq!((ui.menu, ui.m.trig_type), (MenuId::Trigger, TrigType::Delay));
        // math operation
        ui.toggle_menu(MenuId::Math);
        ui.select_option(Item::MathFam, "Filter");
        let (x, y) = mid(Ui::value_parts(row_of(&ui, Item::MathOpI).ctrl).1);
        ui.tap(x, y);
        assert_eq!(ui.items(MenuId::Pick).len(), ui.options(Item::MathOpI).len());
        let fft = ui.options(Item::MathOpI).iter().position(|s| s == "FFT").unwrap();
        let (x, y) = mid(row_of(&ui, Item::PickOpt(fft)).ctrl);
        ui.tap(x, y);
        assert_eq!((ui.m.math, ui.menu), (MathOp::Fft, MenuId::Math));
        // tapping the menu's top-bar button while its picker is open closes the menu
        ui.open_picker(Item::FftWin);
        ui.toggle_menu(MenuId::Math);
        assert!(!ui.menu_open && ui.pick.is_none());
    }

    /// Every option of every item with a picker or segmented buttons can be selected and reads back as chosen.
    #[test]
    fn every_option_selects() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let items = [
            Item::TrigType, Item::MathOpI, Item::MathFam, Item::FftWin, Item::VidSync, Item::AcqDepth, Item::Persist, Item::AcqAvg, Item::DecBaud, Item::QuickAct,
            Item::AcqMode, Item::TrigWhen, Item::Pat(1), Item::DecCs, Item::DecProto, Item::GridStyle, Item::ChProbe(0), Item::TrigMode, Item::StoreDest,
            Item::TrigCoup,
        ];
        for it in items {
            let opts = ui.options(it);
            assert!(!opts.is_empty(), "{:?}", it);
            for o in opts.iter().rev().chain(opts.iter()) {
                ui.select_option(it, o);
                assert_eq!(&ui.value(it), o, "{:?}", it);
            }
        }
        // long lists get a picker, short ones segmented buttons
        for it in [Item::TrigType, Item::FftWin, Item::AcqDepth, Item::Persist, Item::AcqAvg, Item::DecBaud, Item::QuickAct] {
            assert!(matches!(ui.ctl(it, 300), Ctl::Pick), "{:?}", it);
        }
        assert!(matches!(ui.ctl(Item::AcqMode, 300), Ctl::Seg(..)));
    }

    /// Clear in STOP: maps, record, measurements, statistics and mask counts are gone and stay gone; the next
    /// acquisition (SINGLE) draws again.
    #[test]
    fn clear_empties_the_display_in_stop() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let lit = |ui: &Ui| ui.ph.map.iter().map(|m| m.iter().filter(|&&v| v > 0).count()).sum::<usize>();
        for _ in 0..5 {
            ui.step(0.033);
        }
        ui.create_mask();
        ui.step(0.033);
        ui.key(key::RUNSTOP);
        ui.step(0.033);
        assert!(lit(&ui) > 500 && ui.mask_pass > 0 && !ui.mstat.is_empty());
        ui.key(key::CLEAR);
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        for _ in 0..5 {
            ui.step(0.033);
            ui.render(&mut cv);
        }
        assert_eq!(lit(&ui), 0);
        assert!(ui.last[0].is_none() && ui.mstat.is_empty() && (ui.mask_pass, ui.mask_fail) == (0, 0));
        assert!(ui.meas(0, Meas::Freq).is_nan());
        assert_eq!(ui.toast.as_ref().unwrap().0, "Display cleared");
        assert_eq!(ui.m.run, RunState::Stop);
        // no trace pixel in the plot (below the mask pill)
        let a = ui.area;
        let yellow = (a.y + 60..a.y + a.h - 20).flat_map(|y| (a.x + 60..a.x + a.w - 60).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * PANEL_W + x) as usize] == CH_COLORS[0]).count();
        assert_eq!(yellow, 0);
        // Display > Clear does the same; SINGLE acquires and draws again
        ui.key(key::SINGLE);
        ui.step(0.033);
        assert!(lit(&ui) > 500 && ui.last[0].is_some());
        ui.activate(Item::ClearDisp);
        ui.step(0.033);
        assert_eq!(lit(&ui), 0);
    }

    /// A tap on Measure opens the measurement list. A press-and-hold opens a short menu and Add meas,
    /// released on the button, adds the next usual measurement. The rest of the Measure page is unchanged.
    #[test]
    fn measure_tap_lists_and_hold_adds() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.layout = Layout::Default; // the stock top bar with its Measure button
        let rest = vec![
            Item::MeasSrc, Item::MeasSrcB, Item::MeasCopyTo, Item::MeasGate, Item::Open(MenuId::MeasPair), Item::Open(MenuId::Dvm), Item::Open(MenuId::MeasLevels), Item::MeasIdle, Item::MeasTsetup, Item::MeasThold, Item::MeasOutside, Item::MeasN, Item::MeasEdges, Item::Open(MenuId::MeasStat), Item::MeasLabels, Item::StripDock, Item::MeasClear,
        ];
        ui.key(key::MEASURE);
        assert_eq!(ui.menu, MenuId::MeasAdd);
        let settings = ui.catalog_settings_rect();
        ui.tap(settings.x + settings.w / 2, settings.y + settings.h / 2);
        assert_eq!(ui.menu, MenuId::Measure, "settings is visible in the chooser");
        assert_eq!(ui.items(MenuId::Measure), rest);
        for it in ui.items(MenuId::Measure) {
            assert_ne!(ui.item_label(it), "Add item");
        }
        ui.close_menu();
        let r = ui.measure_btn().unwrap();
        let (x, y) = mid(r);
        ui.tap(x, y);
        assert!(ui.menu_open && ui.menu == MenuId::MeasAdd);
        assert!(!ui.m.meas.iter().any(|e| e.0 == Meas::Period));
        ui.tap(x, y);
        assert!(!ui.menu_open);
        // a short press is still a tap
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(0.2);
        assert!(!ui.menu_open);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        assert!(ui.menu_open && ui.menu == MenuId::MeasAdd);
        ui.close_menu();
        // press and hold: the short menu, then the shortcut on release
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(LONG_PRESS + 0.05);
        assert!(ui.menu_open && ui.menu == MenuId::MeasHold);
        assert_eq!(ui.items(MenuId::MeasHold), vec![Item::AddMeas]);
        assert_eq!(ui.item_label(Item::AddMeas), "Add meas");
        let n = ui.m.meas.len();
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        assert!(ui.m.meas.contains(&(Meas::Period, WaveId::Ch(0))));
        assert_eq!(ui.m.meas.len(), n + 1);
        assert!(!ui.menu_open);
        // a lift away from the button leaves the menu and adds nothing further
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(0.5);
        let n = ui.m.meas.len();
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: 8, y: 300 });
        assert!(ui.menu_open && ui.menu == MenuId::MeasHold);
        assert_eq!(ui.m.meas.len(), n);
        let (bx, by) = mid(row_of(&ui, Item::AddMeas).ctrl);
        ui.tap(bx, by);
        assert!(ui.m.meas.len() > n);
        assert_eq!(ui.menu, MenuId::MeasHold);
    }

    /// QUICK key and the Quick button run the action set in System > Quick key; Default keeps the setting.
    #[test]
    fn quick_key_dispatch() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("oos-ui-quick-{}", std::process::id()));
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.store_dir = dir.clone();
        ui.step(0.033);
        // default: screenshot as before
        ui.key(key::QUICK);
        assert_eq!(ui.shot_request.take(), Some(String::new()));
        ui.select_option(Item::QuickAct, "Save setup");
        assert_eq!(ui.m.quick, Quick::SaveSetup);
        ui.key(key::QUICK);
        assert!(dir.join("setup1.txt").exists());
        ui.m.quick = Quick::SaveCsv;
        ui.key(key::QUICK);
        assert!(dir.join("wave-001.csv").exists());
        ui.m.quick = Quick::SavePng;
        ui.key(key::QUICK);
        assert!(ui.shot_request.take().unwrap().ends_with("shot-001.png"));
        ui.m.quick = Quick::RunStop;
        ui.key(key::QUICK);
        assert_eq!(ui.m.run, RunState::Stop);
        ui.m.quick = Quick::Clear;
        ui.key(key::QUICK);
        assert!(ui.cleared && ui.last[0].is_none());
        ui.m.quick = Quick::Single;
        ui.key(key::QUICK);
        assert_eq!((ui.m.trig_mode, ui.m.run), (TrigMode::Single, RunState::Run));
        ui.m.quick = Quick::Autoset;
        ui.key(key::QUICK);
        assert_eq!(ui.toast.as_ref().unwrap().0, "Auto set-up");
        // the start menu entry does the same and closes the menu
        ui.m.quick = Quick::RunStop;
        let (x, y) = mid(ui.menu_btn());
        ui.tap(x, y);
        let run = ui.m.run;
        let (x, y) = mid(row_of(&ui, Item::QuickRun).ctrl);
        ui.tap(x, y);
        assert_ne!(ui.m.run, run);
        assert!(!ui.menu_open);
        ui.key(key::DEFAULT);
        assert_eq!(ui.m.quick, Quick::RunStop);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The top bar's acquisition text stays clear of the buttons in every text size, with the longest values.
    #[test]
    fn the_acquisition_text_fits_before_the_buttons() {
        use super::*;
        for ts in TEXT_SIZES {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.text_size = ts;
            ui.relayout();
            let f = ui.fonts();
            let r = ui.run_pill();
            let room = ui.top_buttons().first().map_or(ui.top_right(), |b| b.2.x) - 12;
            for (sr, depth) in [(1.25e9, 125e6), (625e6, 62.5e6), (3.125e6, 1.25e6)] {
                let (bold, main, mode) = Ui::top_acq(f, r.x + r.w + 14, room, sr, depth, "Average 1024".into());
                let w = if bold { f.mb.width(&main) } else { f.m.width(&main) } + mode.as_ref().map_or(0, |m| f.m.width(m));
                assert!(r.x + r.w + 14 + w <= room, "{:?} {} {}", ts, main, w);
            }
        }
    }

    /// With several channels on, the key of a channel that is not selected only selects it; the next press opens its
    /// menu, the one after that switches it off. Another channel's menu closes, other menus stay.
    #[test]
    fn a_channel_key_first_selects_when_several_are_on() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        // the stock cards; Minimal and Studio cards have their own taps
        ui.m.layout = Layout::Default;
        ui.relayout();
        ui.step(0.033);
        assert!(ui.m.ch[0].on && ui.m.n_on() == 1 && ui.m.active == 0);
        ui.key(key::CH1 + 1);
        assert!(ui.m.ch[1].on && ui.m.active == 1 && ui.menu_open && ui.menu == MenuId::Channel(1));
        ui.key(key::CH1);
        assert!(ui.m.ch[0].on && ui.m.active == 0 && !ui.menu_open, "select only, CH2's menu closes");
        ui.key(key::CH1);
        assert!(ui.menu_open && ui.menu == MenuId::Channel(0), "selected: the menu opens");
        ui.key(key::CH1);
        assert!(!ui.m.ch[0].on && ui.m.active == 1, "menu open: off");
        // one channel on: its key opens the menu at once, as before
        ui.key(key::CH1 + 1);
        assert!(ui.menu_open && ui.menu == MenuId::Channel(1));
        // a menu that is not a channel's stays while the key selects
        ui.key(key::CH1);
        assert!(ui.m.ch[0].on && ui.m.active == 0);
        ui.open_menu(MenuId::Display);
        ui.key(key::CH1 + 1);
        assert!(ui.m.active == 1 && ui.menu_open && ui.menu == MenuId::Display);
        // a tap on a channel's card does the same: select first, then the menu
        ui.close_menu();
        let card = |ui: &Ui, c: usize| mid(ui.cards().0[c]);
        let (x, y) = card(&ui, 0);
        ui.tap(x, y);
        assert!(ui.m.active == 0 && !ui.menu_open, "tap: select only");
        let (x, y) = card(&ui, 0);
        ui.tap(x, y);
        assert!(ui.menu_open && ui.menu == MenuId::Channel(0), "tap again: the menu");
        let (x, y) = card(&ui, 1);
        ui.tap(x, y);
        assert!(ui.m.active == 1 && !ui.menu_open, "CH1's menu closes when CH2 is selected");
    }

    /// A channel key held on the front panel: AC/DC, nothing else; a short press does what the key does, when it is
    /// let go.
    #[test]
    fn a_long_press_of_a_channel_key_switches_ac_dc() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.step(0.033);
        assert!(ui.m.ch[0].on && ui.m.ch[0].coupling == Coupling::Dc && !ui.menu_open);
        // short: the menu opens when the key is let go
        ui.panel_key(key::CH1);
        assert!(!ui.menu_open, "not yet");
        ui.step(0.1);
        ui.panel_key(0);
        assert!(ui.menu_open && ui.menu == MenuId::Channel(0));
        // long: AC while still held, the menu stays, the channel on
        ui.panel_key(key::CH1);
        ui.step(LONG_PRESS);
        assert!(ui.m.ch[0].on && ui.m.ch[0].coupling == Coupling::Ac);
        ui.step(0.5);
        ui.panel_key(0);
        assert!(ui.m.ch[0].on && ui.m.ch[0].coupling == Coupling::Ac && ui.menu_open, "letting go does nothing more");
        ui.panel_key(key::CH1);
        ui.step(1.0);
        ui.panel_key(0);
        assert_eq!(ui.m.ch[0].coupling, Coupling::Dc, "and back");
        // a short press with the menu open: off
        ui.panel_key(key::CH1);
        ui.panel_key(0);
        assert!(!ui.m.ch[0].on);
        // scripts and the remote: every press at once
        ui.key(key::CH1);
        assert!(ui.m.ch[0].on);
        // a finger held on the channel's card: its quick actions (no silent AC/DC), and lifting it opens no menu
        ui.close_menu();
        let (x, y) = mid(ui.cards().0[0]);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(LONG_PRESS + 0.05);
        assert_eq!(ui.m.ch[0].coupling, Coupling::Dc, "a hold never flips the coupling");
        assert!(ui.workspace.popup.is_some(), "the quick actions are open");
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        assert!(!ui.menu_open, "no tap after a long touch");
        ui.dismiss_context();
        // a short one is a tap
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(0.1);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        assert!(ui.menu_open && ui.menu == MenuId::Channel(0) && ui.m.ch[0].coupling == Coupling::Dc);
    }

    /// Every front-panel key function is in the start menu, and every menu is reachable by its key and by touch.
    #[test]
    fn start_menu_and_keys_reach_everything() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for _ in 0..3 {
            ui.step(0.033);
        }
        // the menu keys
        for (k, m) in [(key::HMENU, MenuId::Acquire), (key::ANALYSE, MenuId::Decode), (key::TMENU, MenuId::Trigger), (key::MEASURE, MenuId::MeasAdd), (key::CURSOR, MenuId::Cursor), (key::MATH, MenuId::Math), (key::REF, MenuId::Ref)] {
            ui.close_menu();
            ui.key(k);
            assert!(ui.menu_open && ui.menu == m, "key {:#x}", k);
        }
        assert!(ui.items(MenuId::Acquire).contains(&Item::Open(MenuId::Horizontal)));
        ui.close_menu();
        // the Menu button opens the start menu with all entries
        let (x, y) = mid(ui.menu_btn());
        ui.tap(x, y);
        assert_eq!(ui.menu, MenuId::Main);
        let all = ui.items(MenuId::Main);
        for it in [Item::RunStop, Item::SingleAct, Item::AutosetAct, Item::ClearDisp, Item::DefaultAct, Item::SlopeAct, Item::ForceAct, Item::NavHist, Item::QuickRun, Item::Shot, Item::LockAct, Item::PowerAct] {
            assert!(all.contains(&it), "{:?}", it);
        }
        for m in [MenuId::Acquire, MenuId::Trigger, MenuId::Horizontal, MenuId::Measure, MenuId::Cursor, MenuId::Math, MenuId::Ref, MenuId::Decode, MenuId::Display, MenuId::Storage, MenuId::Mask, MenuId::Utility] {
            assert!(all.contains(&Item::Open(m)), "{:?}", m);
            ui.open_menu(MenuId::Main);
            let (x, y) = mid(row_of(&ui, Item::Open(m)).ctrl);
            ui.tap(x, y);
            // Measure is the measurement page, as its key and the top bar open it (with Back to the start menu)
            let page = if m == MenuId::Measure { MenuId::MeasAdd } else { m };
            assert_eq!((ui.menu, ui.parent), (page, Some(MenuId::Main)));
            if m == MenuId::Measure {
                let (x, y) = mid(ui.dlg_layout().unwrap().back.expect("Back to the start menu"));
                ui.tap(x, y);
                assert_eq!(ui.menu, MenuId::Main);
            }
        }
        assert_eq!(ui.m.cursor, CursorMode::Time, "Cursor entry turns the cursors on like the key");
        // slope steps and keeps the menu, history entries appear with the history
        ui.open_menu(MenuId::Main);
        let s = ui.m.slope;
        let (x, y) = mid(row_of(&ui, Item::SlopeAct).ctrl);
        ui.tap(x, y);
        assert!(ui.m.slope != s && ui.menu == MenuId::Main);
        assert!(!ui.items(MenuId::Main).contains(&Item::NavBack));
        let (x, y) = mid(row_of(&ui, Item::NavHist).ctrl);
        ui.tap(x, y);
        assert!(ui.hist_pos.is_some() && ui.items(MenuId::Main).contains(&Item::NavBack));
        let i = ui.hist_pos.unwrap();
        let (x, y) = mid(row_of(&ui, Item::NavBack).ctrl);
        ui.tap(x, y);
        assert_eq!(ui.hist_pos, Some(i - 1));
        // power: the dialog, without the MCU frame of a key press
        let (x, y) = mid(row_of(&ui, Item::PowerAct).ctrl);
        ui.tap(x, y);
        assert!(ui.power_dlg && ui.panel_tx.is_empty() && !ui.menu_open);
        ui.tap(5, 300);
        // touch lock from the start menu: touches do nothing, except the Menu button, which unlocks
        ui.open_menu(MenuId::Main);
        let (x, y) = mid(row_of(&ui, Item::LockAct).ctrl);
        ui.tap(x, y);
        assert!(ui.m.touch_lock && !ui.menu_open);
        let touch = |ui: &mut Ui, x: i32, y: i32| {
            ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
            ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        };
        let (tr, _) = (ui.cards().1, ());
        touch(&mut ui, tr.x + 10, tr.y + 10);
        assert!(!ui.menu_open && ui.m.touch_lock);
        let (x, y) = mid(ui.menu_btn());
        touch(&mut ui, x, y);
        assert!(!ui.m.touch_lock && !ui.menu_open);
        touch(&mut ui, x, y);
        assert_eq!(ui.menu, MenuId::Main);
    }

    #[test]
    fn nested_menu_back_restores_each_parent_and_its_focus_after_a_picker() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.layout = Layout::Default; // the stock dialogs: Minimal and Studio open sheets
        let back = |ui: &mut Ui| {
            let (x, y) = mid(ui.dlg_layout().unwrap().back.expect("nested menu has Back"));
            ui.tap(x, y);
        };
        ui.open_menu(MenuId::MeasAdd);
        ui.focus = 2;
        ui.activate(Item::Open(MenuId::Measure));
        ui.focus = 5;
        ui.open_picker(Item::MeasSrc);
        back(&mut ui);
        assert_eq!((ui.menu, ui.focus), (MenuId::Measure, 5));
        ui.focus = 7;
        ui.activate(Item::Open(MenuId::MeasPair));
        back(&mut ui);
        assert_eq!((ui.menu, ui.focus), (MenuId::Measure, 7));
        back(&mut ui);
        assert_eq!((ui.menu, ui.focus), (MenuId::MeasAdd, 2));
        assert!(ui.parent.is_none() && ui.menu_history.is_empty());
        ui.activate(Item::Open(MenuId::Measure));
        ui.close_menu();
        ui.open_menu(MenuId::Trigger);
        assert!(ui.parent.is_none() && ui.menu_history.is_empty());
        assert!(ui.dlg_layout().unwrap().back.is_none(), "a direct toolbar action starts a fresh route");
    }

    #[test]
    fn axis_labels() {
        // prefix from the span, decimals from step and base (offset / delay)
        assert_eq!(axis_label(-1e-6, 2e-7, 0.0, 1e-6, "s"), "-1.0 \u{b5}s");
        assert_eq!(axis_label(1.5, 0.5, 0.0, 2.0, "V"), "1.5 V");
        assert_eq!(axis_label(-0.6, 0.2, 0.0, 0.8, "V"), "-600 mV");
        assert_eq!(axis_label(3.25, 2.0, 4.75, 12.75, "V"), "3.25 V");
        assert_eq!(axis_label(-1e-9, 2.0, 0.0, 8.0, "V"), "0 V");
        // 1 us/div at a delay of 3 ms: every label its own (was "3 ms" each)
        assert_eq!(axis_label(3.001e-3, 1e-6, 3e-3, 3.005e-3, "s"), "3.001 ms");
        assert_eq!(axis_label(2.996e-3, 1e-6, 3e-3, 3.005e-3, "s"), "2.996 ms");
        assert_eq!(axis_label(0.2e-6, 0.1e-6, 0.0, 0.5e-6, "s"), "200 ns");
        // an offset of 1.481 V: the labels keep its three decimals
        assert_eq!(axis_label(1.519, 1.0, 1.481, 4.481, "V"), "1.519 V");
        assert_eq!(axis_label(1.545455, 1.0, 1.454545, 4.454545, "V"), "1.545 V");
        // 1 us/div at a delay of 1 s
        assert_eq!(axis_label(1.000002, 1e-6, 1.0, 1.000005, "s"), "1.000002 s");
    }
}

/// One step on the 1-2-5 time ladder from the nearest ladder value.
fn ladder_step(x: f32, d: i32) -> f32 {
    let l = time_ladder();
    let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}

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
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        assert!(ui.items(MenuId::Utility).contains(&Item::AuxOut));
        let want: &[&str] = if board().mask_hw() || board().zone_hw() { &["Trigger out", "Off", "Pass/fail"] } else { &["Trigger out", "Off"] };
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
