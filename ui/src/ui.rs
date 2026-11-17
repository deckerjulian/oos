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
mod input;
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
mod panes;
mod plot_layout;
mod reflook;
mod render;
mod render_derived;
mod render_marks;
mod scpi_operations;
mod scroll;
mod selfcal;
mod sertrig;
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
mod ssh;
mod time;
pub mod views;
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
    ("hpos", 0x8a),
    ("vpos", 0x9a),
    ("multi2", 0xaa),
    ("multi1", 0xba),
    ("hscale", 0xca),
    ("vscale", 0xda),
    ("tlevel", 0xea),
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Item {
    DecSel,
    DecTable,
    DecExport,
    SearchTable,
    SearchSave,
    StoreBin,
    PaneSel,
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
    MeasLabels,
    /// where the measurement strip sits
    StripDock,
    XyMode,
    RefSrc,
    RefSave,
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
    /// Acquire > Draw path: Batch (PR #7, `Model::draw_batch`, the default) or Standard (fast path)
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
    MeasGate,
    MeasTsetup,
    MeasThold,
    CurMode,
    CurSrcB,
    CurA,
    CurB,
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
}

/// The choices of an item, for its picker.
type OptionsFn = fn(&Ui, Item) -> Option<Vec<String>>;

/// One part of the menus (a module of ui/): what it answers about the items it owns, None where it has nothing of
/// that kind. A part answers None (false) for an item that is not its own, and no item of a menu belongs to two parts
/// (`no_menu_item_has_two_parts`), so the order of `PARTS` decides nothing.
struct Part {
    kind: Option<fn(Item) -> Option<Kind>>,
    label: Option<fn(Item) -> Option<&'static str>>,
    value: Option<fn(&Ui, Item) -> Option<String>>,
    adjust: Option<fn(&mut Ui, Item, i32) -> bool>,
    activate: Option<fn(&mut Ui, Item) -> bool>,
    options: Option<OptionsFn>,
    /// a line of text, not a control
    text: Option<fn(Item) -> bool>,
}

const NO_PART: Part =
    Part { kind: None, label: None, value: None, adjust: None, activate: None, options: None, text: None };

/// The parts of the menus, asked by `kind`, `label`, `value`, `adjust`, `activate`, `options` and `ctl` before the
/// items `ui.rs` keeps itself. `remote` answers for `ssh` too, except the actions.
const PARTS: [Part; 22] = [
    Part {
        kind: Some(Ui::meas_kind),
        label: Some(Ui::meas_cfg_label),
        value: Some(Ui::meas_value),
        adjust: Some(Ui::meas_adjust),
        options: Some(Ui::meas_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::ana_kind),
        label: Some(Ui::ana_label),
        value: Some(Ui::ana_value),
        adjust: Some(Ui::ana_adjust),
        activate: Some(Ui::ana_activate),
        options: Some(Ui::ana_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::magnetic_kind),
        label: Some(Ui::magnetic_label),
        value: Some(Ui::magnetic_value),
        adjust: Some(Ui::magnetic_adjust),
        options: Some(Ui::magnetic_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::inverter_kind),
        label: Some(Ui::inverter_label),
        value: Some(Ui::inverter_value),
        adjust: Some(Ui::inverter_adjust),
        options: Some(Ui::inverter_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::dpt_kind),
        label: Some(Ui::dpt_label),
        value: Some(Ui::dpt_value),
        adjust: Some(Ui::dpt_adjust),
        options: Some(Ui::dpt_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::cal_kind),
        label: Some(Ui::cal_label),
        value: Some(Ui::cal_value),
        adjust: Some(Ui::cal_adjust),
        activate: Some(Ui::cal_activate),
        options: Some(Ui::cal_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::bode_kind),
        label: Some(Ui::bode_label),
        value: Some(Ui::bode_value),
        adjust: Some(Ui::bode_adjust),
        activate: Some(Ui::bode_activate),
        options: Some(Ui::bode_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::sim_kind),
        label: Some(Ui::sim_label),
        value: Some(Ui::sim_value),
        adjust: Some(Ui::sim_adjust),
        activate: Some(Ui::sim_activate),
        options: Some(Ui::sim_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::offline_kind),
        label: Some(Ui::offline_label),
        value: Some(Ui::offline_value),
        adjust: Some(Ui::offline_adjust),
        options: Some(Ui::offline_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::afg_kind),
        label: Some(Ui::afg_label),
        value: Some(Ui::afg_value),
        adjust: Some(Ui::afg_adjust),
        activate: Some(Ui::afg_activate),
        options: Some(Ui::afg_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::bw_kind),
        label: Some(Ui::bw_label),
        value: Some(Ui::bw_value),
        adjust: Some(Ui::bw_adjust),
        options: Some(Ui::bw_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::la_kind),
        label: Some(Ui::la_label),
        value: Some(Ui::la_value),
        adjust: Some(Ui::la_adjust),
        options: Some(Ui::la_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::win_kind),
        label: Some(Ui::win_label),
        value: Some(Ui::win_value),
        adjust: Some(Ui::win_adjust),
        options: Some(Ui::win_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::bus_kind),
        label: Some(Ui::bus_label),
        value: Some(Ui::bus_value),
        adjust: Some(Ui::bus_adjust),
        options: Some(Ui::bus_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::time_kind),
        label: Some(Ui::time_label),
        value: Some(Ui::time_value),
        adjust: Some(Ui::time_adjust),
        activate: Some(Ui::time_activate),
        options: Some(Ui::time_options),
        text: Some(Ui::time_text),
    },
    Part {
        kind: Some(Ui::remote_kind),
        label: Some(Ui::remote_label),
        value: Some(Ui::remote_value),
        adjust: Some(Ui::remote_adjust),
        options: Some(Ui::remote_options),
        text: Some(Ui::remote_text),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::graphics_kind),
        label: Some(Ui::graphics_label),
        value: Some(Ui::graphics_value),
        adjust: Some(Ui::graphics_adjust),
        options: Some(Ui::graphics_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::views_kind),
        label: Some(Ui::views_label),
        value: Some(Ui::views_value),
        adjust: Some(Ui::views_adjust),
        options: Some(Ui::views_options),
        ..NO_PART
    },
    Part {
        label: Some(Ui::monitor_label),
        value: Some(Ui::monitor_value),
        adjust: Some(Ui::monitor_adjust),
        options: Some(Ui::monitor_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::features_kind),
        label: Some(Ui::features_label),
        value: Some(Ui::features_value),
        adjust: Some(Ui::features_adjust),
        options: Some(Ui::features_options),
        ..NO_PART
    },
    Part {
        kind: Some(Ui::backlight_kind),
        label: Some(Ui::backlight_label),
        value: Some(Ui::backlight_value),
        adjust: Some(Ui::backlight_adjust),
        ..NO_PART
    },
    Part { activate: Some(Ui::ssh_activate), ..NO_PART },
];

#[cfg(test)]
mod part_tests {
    use super::*;

    // Break it catches: an item of a menu that two parts answer for, so the order of PARTS (and of the six chains it
    // replaced, which did not agree) decides what the menu shows or does. Actions are not run.
    #[test]
    fn no_menu_item_has_two_parts() {
        use MenuId::*;
        let ui = crate::ui::testkit::demo_ui();
        let mut items: Vec<Item> = Vec::new();
        let menus = [
            Main,
            Channel(0),
            ChMore(0),
            Logger,
            Horizontal,
            Trigger,
            Acquire,
            Measure,
            MeasAdd,
            MeasHold,
            MeasPair,
            Dvm,
            Cursor,
            Display,
            Utility,
            Math,
            Ref,
            Decode,
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
            Network,
            Features,
            Decoders(0),
            Remote,
            Ssh,
            Time,
            Bode,
            Sim,
            SimAdvanced,
            Afg,
            BwTest,
            ViewMore,
            La,
            LaLines,
            LaPat,
            Windows,
            MeasSelected,
        ];
        for m in menus {
            for it in ui.items(m) {
                if !items.contains(&it) {
                    items.push(it);
                }
            }
        }
        let mut owned = 0;
        for &it in &items {
            let mut u = crate::ui::testkit::demo_ui();
            let claims: Vec<usize> = (0..PARTS.len())
                .filter(|&i| {
                    let p = &PARTS[i];
                    p.kind.is_some_and(|f| f(it).is_some())
                        | p.label.is_some_and(|f| f(it).is_some())
                        | p.value.is_some_and(|f| f(&ui, it).is_some())
                        | p.options.is_some_and(|f| f(&ui, it).is_some())
                        | p.text.is_some_and(|f| f(it))
                        | p.adjust.is_some_and(|f| f(&mut u, it, 1))
                })
                .collect();
            assert!(claims.len() <= 1, "{:?} belongs to parts {:?}", it, claims);
            owned += claims.len();
        }
        assert!(items.len() > 400 && owned > 200, "{} items, {} of a part", items.len(), owned);
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
    Touch {
        id: i32,
        phase: Phase,
        x: i32,
        y: i32,
    },
    #[cfg_attr(not(test), allow(dead_code))] // only the tests make it
    Wheel {
        x: i32,
        y: i32,
        steps: i32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    },
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

/// One record's measurements: (channel, samples, dt, the smallest swing that counts as a signal).
type MeasJob = (u64, Vec<(usize, MeasSrc, f64, f64)>);
type MeasDone = (u64, Vec<(usize, Stats, oos_dsp::measure::Analysis)>);

/// The measurements of a record, each channel on its own thread (the four channels of 100 kpts took 27 ms in a row
/// on the unit).
fn measure_channels(work: &[(usize, &[f32], f64, f64)]) -> Vec<(usize, Stats, oos_dsp::measure::Analysis)> {
    let one =
        |&(c, v, dt, min): &(usize, &[f32], f64, f64)| (c, stats(v, dt), oos_dsp::measure::analyze_min(v, dt, min));
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
                    let w: Vec<(usize, &[f32], f64, f64)> =
                        work.iter().map(|(c, v, dt, min)| (*c, v.samples(), *dt, *min)).collect();
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
        let mut m = Model { meas: Self::default_meas(), ..Default::default() };
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
            wins: Vec::new(),
            cur_win: 0,
            full: false,
            full_at: None,
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
            desktop_panel: None,
            analyzed_view: AView::Scope,
            sim_play: false,
            sim_generation: None,
            sim_error: None,
            offline: false,
            look: reflook::RefLook::Live,
            meas_own: std::collections::HashMap::new(),
            meas_cfg: None,
            offline_ref: 0,
            offline_hold: None,
            touch: TouchState {
                down: false,
                x0: 0,
                y0: 0,
                x: 0,
                y: 0,
                t0: 0.0,
                held: false,
                drag: Drag::None,
                start_offset: 0.0,
                start_delay: 0.0,
                start_level: 0.0,
                start_cur: 0.0,
                win: 0,
            },
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
            pinch: Pinch {
                active: false,
                p0: (0, 0),
                p1: (0, 0),
                ref_dx: 0,
                ref_dy: 0,
                area: Rect::new(0, 0, 0, 0),
                detail: false,
            },
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
        let la = self.last[0]
            .as_ref()
            .and_then(|r| r.la.as_ref())
            .filter(|_| self.m.la_on())
            .ok_or("No logic analyser record")?;
        Ok(crate::store::la_csv(la, &self.m.la.lines()))
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
        self.mstat
            .iter()
            .map(|stat| (stat.n, [stat.sum.to_bits(), stat.sumsq.to_bits(), stat.min.to_bits(), stat.max.to_bits()]))
            .collect()
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
            && self.last[0].as_ref().and_then(|r| r.raw.as_ref()).is_some_and(|r| {
                crate::compress::live(&r.frame)
                    || (crate::deepmeas::want(m).is_some() && crate::deepmeas::deep_pairs(&r.frame))
            })
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

    /// (points, dt, time of the first point relative to the trigger) of the last record of channel c.
    #[cfg(test)]
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
    #[cfg(test)]
    pub fn raw_volts(&self, c: usize) -> Option<Vec<f32>> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        (!r.frame.envelope).then(|| v.iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect())
    }

    pub fn record_volts(&self, c: usize) -> Vec<f32> {
        self.last[0]
            .as_ref()
            .map(|r| r.ch[c].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect())
            .unwrap_or_default()
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
    #[cfg(test)]
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
        self.seg_times
            .iter()
            .map(|t| t.map_or("9.9E37".to_string(), |x| format!("{:e}", x)))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Hardware segments: shows segment `i` (played from the FPGA memory), stopping the acquisition.
    pub(super) fn seg_show(&mut self, i: usize) {
        self.m.run = RunState::Stop;
        self.seg_pos = Some(i);
        self.src.seg_request(i as u32);
        self.toast(&self.seg_label(i));
    }

    /// Shows record `hist_pos + d` of the history (d = 0: the newest), stopping the acquisition.
    fn history_step(&mut self, d: i32) {
        // hardware segments: any of them, played on demand
        if let Some(v) =
            self.seg_view.filter(|v| self.seg_hw_on() && v.phase == crate::board::SegPhase::Done && v.recorded > 0)
        {
            let n = v.recorded as i32;
            let cur = self.seg_pos.unwrap_or(n as usize - 1) as i32;
            let i = if d == 0 { n - 1 } else { (cur + d).clamp(0, n - 1) };
            return self.seg_show(i as usize);
        }
        let n = self.history.len();
        if n == 0 {
            return self.toast("History empty");
        }
        self.m.run = RunState::Stop;
        let cur = self.hist_pos.unwrap_or(n - 1) as i32;
        let i = if d == 0 { n as i32 - 1 } else { (cur + d).clamp(0, n as i32 - 1) } as usize;
        self.hist_pos = Some(i);
        self.last[0] = Some(self.history[i].clone());
        self.measure_last();
        self.toast(&format!("History {}/{}", i + 1, n));
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
        Record {
            t0: 0.0,
            dt: 1.0,
            ch: Default::default(),
            triggered: false,
            partial: false,
            raw: None,
            la: None,
            pairs: false,
        }
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
    fn search_events(&self) -> std::sync::Arc<Vec<f64>> {
        if self.m.trig_type == TrigType::Bus
            && (Self::decoder_uses_refs(&self.m) || self.m.deskew_ch.iter().any(|&shift| shift != 0.0))
        {
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
                if self.usb_used() {
                    std::path::PathBuf::from(crate::store::USB_DIR)
                } else {
                    self.store_dir.clone()
                }
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

    /// File of the current setup slot: a setup is small and has to survive a power cycle, so the default storage directory
    /// in RAM is not used for it when OOS's data partition is there.
    fn setup_path(&self) -> std::path::PathBuf {
        let keep = std::path::Path::new("/oos/setups");
        let dir = if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) && keep.is_dir() {
            keep
        } else {
            self.store_dir.as_path()
        };
        dir.join(format!("setup{}.txt", self.store_slot))
    }

    /// The `ref<N>.*` lines of a setup: how each loaded ref slot looks (on/off, scale, offset, colour, name).
    fn ref_setup_text(&self) -> String {
        let looks: Vec<_> = self
            .refs
            .iter()
            .map(|r| {
                r.as_ref().map(|r| crate::store::RefLook {
                    on: Some(r.on),
                    vdiv: Some(r.vpd),
                    offset: Some(r.offset),
                    color: Some(r.color),
                    label: Some(r.label.clone()),
                })
            })
            .collect();
        crate::store::ref_setup_text(&looks)
    }

    /// Applies the saved look to the ref slots that hold a waveform now (the samples are not in a setup).
    fn apply_ref_looks(&mut self, looks: &[Option<crate::store::RefLook>]) {
        for (r, l) in self.refs.iter_mut().zip(looks) {
            let (Some(r), Some(l)) = (r.as_mut(), l) else { continue };
            if let Some(v) = l.on {
                r.on = v;
            }
            if let Some(v) = l.vdiv {
                r.vpd = v;
            }
            if let Some(v) = l.offset {
                r.offset = v;
            }
            if let Some(v) = l.color {
                r.color = v.min(REF_COLORS.len() - 1);
            }
            if let Some(v) = &l.label {
                r.label = v.clone();
            }
        }
    }

    /// Loads setup<slot>.txt of the storage directory; the acquisition state (run / stop) stays as it is.
    fn recall_setup(&mut self) {
        let p = self.setup_path();
        // what this build cannot take (a decoder it does not have, a key of a newer build) is left out and named
        let text = std::fs::read_to_string(&p).map_err(|e| e.to_string());
        let parsed = text.as_ref().map_err(|e| e.clone()).map(|t| crate::store::parse_setup_lenient(t));
        match parsed {
            Ok((mut m, bad)) => {
                for e in &bad {
                    eprintln!("oos-ui: recall {}: left out: {}", p.display(), e);
                }
                // a set-up is a live one, as with the Default key: a reference or SIM session ends first, so a Live
                // afterwards does not bring back the set-up from before it over the recalled one
                if self.record_mode() {
                    self.leave_session();
                }
                if let Ok(t) = &text {
                    self.apply_ref_looks(&crate::store::parse_ref_looks(t, NREF));
                }
                (m.run, m.status) = (self.m.run, self.m.status);
                // the text size is a preference of this screen, not part of a set-up
                m.text_size = self.m.text_size;
                if m.mask_src != self.m.mask_src {
                    self.mask = None;
                    self.mask_ref = None;
                }
                self.m = m;
                // a set-up written by hand or by an older build may have the two levels the wrong way round
                self.m.order_trigger_levels();
                // what Bus switched on belongs to the set-up before: leaving Bus leaves the recalled one as it is
                self.bus_auto = None;
                self.math_state.borrow_mut().forget_fit();
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
                match bad.first() {
                    None => self.toast(&format!("Recalled {}", crate::store::file_name(&p))),
                    Some(e) => self.toast(&format!(
                        "Recalled {}, {} line{} left out ({})",
                        crate::store::file_name(&p),
                        bad.len(),
                        if bad.len() == 1 { "" } else { "s" },
                        e
                    )),
                }
            }
            Err(e) => self.toast(&format!("Recall failed: {}", e)),
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
    cv.set_clip(r);
    cv.text_in(f, Rect::new(r.x, r.y, r.w, r.h / 2 + 1), 1, &a, c);
    cv.text_in(f, Rect::new(r.x, r.y + r.h / 2 - 1, r.w, r.h / 2 + 1), 1, &b, c);
    cv.no_clip();
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
    let x_per_sample = f.dt / m.time_per_div() as f64 * w as f64 / HDIV as f64;
    crate::drawpipe::screen_map(m, w, h, c, x_per_sample, ti)
}

/// One frame of the fast path placed on the screen (`w` x `h` map): the trigger, the columns of a dense frame or the
/// screen record of a sparse one, and the statistics of the channels in `fold`. Touches nothing but the frame, so the
/// frames of a batch are placed on several cores; this is where most of the work is.
pub fn fast_plan(
    m: &Model,
    w: usize,
    h: usize,
    f: &std::sync::Arc<crate::board::Frame>,
    fold: [bool; NCH],
) -> FastFrame {
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
    let col = (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize;
    // the full search of `fastdraw::edge_smooth` (the Batch path takes `trignear::edge_smooth_near`)
    let t = crate::drawpipe::refine_edge(m, f, col, |level, hyst, rising, either, half, col| {
        crate::fastdraw::edge_smooth(src, f.pre, level, hyst, rising, either, half, col, |_| true)
    });
    let (class, off) = t.class.map_or((None, 0.0), |(k, d)| (Some(k), d));
    let ti = t.ti;
    let used = |c: usize| m.ch[c].on && !f.ch[c].is_empty();
    let spans = std::array::from_fn(|c| {
        (used(c) && !m.dots).then(|| crate::fastdraw::spans(w, h, &f.ch[c], &raw_map(m, w, h, c, f, ti)))
    });
    // the statistics on the samples of the screen window, as the measurements of a dense record
    let fold = std::array::from_fn(|c| {
        (fold[c] && used(c)).then(|| {
            let v = &f.ch[c];
            let i0 = ((ti + t_first / f.dt).floor().max(0.0) as usize).min(v.len());
            let i1 = ((ti + (t_first + window) / f.dt).ceil().max(0.0) as usize).min(v.len());
            crate::fastdraw::Fold::of(&v[i0..i1.max(i0)])
        })
    });
    FastFrame { plan: FastPlan::Raw { ti, class, off, spans }, triggered: t.triggered, fold }
}

/// The rows (first, last + 1) the points `v` of channel `c` of a screen record fall on, within a map `h` high.
fn rec_rows(m: &Model, h: usize, c: usize, v: &[f32]) -> (usize, usize) {
    let ch = &m.ch[c];
    let (vpd, ppd_y) = (ch.volts_per_div(), h as f32 / VDIV);
    let (lo, hi) =
        v.iter().filter(|x| x.is_finite()).fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
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
pub fn fast_deposit_t(
    m: &Model,
    w: usize,
    h: usize,
    c: usize,
    band: &mut [u16],
    c_lo: usize,
    f: &crate::board::Frame,
    p: &FastPlan,
) {
    if !m.ch[c].on {
        return;
    }
    match p {
        FastPlan::Rec(rec) => {
            if !rec.ch[c].is_empty() {
                let bw = band.len() / (h + 1);
                let c_hi = (c_lo + bw) as i32;
                crate::wave::trace_columns(
                    w,
                    h,
                    c,
                    m,
                    (m.time_per_div(), m.delay),
                    rec.t0,
                    rec.dt,
                    &rec.ch[c],
                    24,
                    None,
                    rec.pairs,
                    (c_lo as i32, c_hi),
                    |cx, r0, r1, wt| {
                        crate::fastdraw::diff_span(band, bw, cx as usize - c_lo, r0 as usize, r1 as usize, wt);
                    },
                );
            }
        }
        FastPlan::Raw { ti, spans, .. } => match &spans[c] {
            Some(s) => crate::fastdraw::add_spans_t(band, h, c_lo, s, 24),
            None if !f.ch[c].is_empty() => {
                crate::fastdraw::deposit_dots_t(band, h, c_lo, &f.ch[c], &raw_map(m, w, h, c, f, *ti), 24)
            }
            None => {}
        },
    }
}

/// Channel `c` of a placed frame into the rows `r_lo` .. `r_lo + band.len() / w` of that channel's map (`w` x `h`),
/// held in `band` (the whole map: `r_lo` 0): one frame after the other straight into the row-major map, the tests'
/// reference for the fast path's drawing.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn fast_deposit(
    m: &Model,
    w: usize,
    h: usize,
    c: usize,
    band: &mut [u16],
    r_lo: usize,
    f: &crate::board::Frame,
    p: &FastPlan,
) {
    if !m.ch[c].on {
        return;
    }
    match p {
        FastPlan::Rec(rec) => {
            if !rec.ch[c].is_empty() {
                crate::wave::draw_trace(
                    band,
                    w,
                    h,
                    r_lo,
                    c,
                    m,
                    (m.time_per_div(), m.delay),
                    rec.t0,
                    rec.dt,
                    &rec.ch[c],
                    24,
                    None,
                    rec.pairs,
                );
            }
        }
        FastPlan::Raw { ti, spans, .. } => match &spans[c] {
            Some(s) => crate::fastdraw::add_spans(band, w, r_lo, s, 24),
            None if !f.ch[c].is_empty() => {
                crate::fastdraw::deposit_dots_band(band, w, r_lo, h, &f.ch[c], &raw_map(m, w, h, c, f, *ti), 24)
            }
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
        let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            envelope,
            acq: (1e7 / (tdiv * 16.0), 10_000_000),
            ..Default::default()
        };
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
        let v: Vec<f32> = (0..n)
            .map(|i| {
                if (i / 2) % 50 < 10 {
                    1.0
                } else if i % 2 == 0 {
                    -1.0
                } else {
                    -0.98
                }
            })
            .collect();
        for (seq, envelope) in [(1u64, true), (2, false), (3, true)] {
            // a deep record (above 1 M points): pairs of the live compression are held instead (compress::hold)
            shared.lock().unwrap().queue.push_back(crate::board::Frame {
                seq,
                dt: window * 1.2 / n as f64,
                pre: n / 2,
                ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()],
                envelope,
                acq: (1e9, 10_000_000),
                ..Default::default()
            });
            for _ in 0..3 {
                ui.step(0.02);
            }
            let (avg, rms, max, pp) =
                (ui.meas(0, Meas::Vavg), ui.meas(0, Meas::Vrms), ui.meas(0, Meas::Vmax), ui.meas(0, Meas::Vpp));
            assert_eq!(avg.is_nan() && rms.is_nan(), envelope, "envelope {}: mean {} rms {}", envelope, avg, rms);
            assert!(
                (max - 1.0).abs() < 0.05 && (pp - 2.0).abs() < 0.05,
                "envelope {}: max {} pk-pk {}",
                envelope,
                max,
                pp
            );
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
        let frame = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            envelope,
            acq: (625e6, 100_000),
            ..Default::default()
        };
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
        assert_eq!(
            ui.last[0].as_ref().and_then(|r| r.raw.as_ref()).map(|r| (r.frame.seq, r.frame.envelope)),
            Some((5, false))
        );
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
        let frame = |seq: u64, envelope: bool, acq: usize, v: Vec<f32>| crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            envelope,
            acq: (1e9, acq),
            ..Default::default()
        };
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
        assert!(
            ui.stop_final.is_some()
                && ui.last[0].as_ref().is_some_and(|r| r.raw.as_ref().is_some_and(|x| x.frame.envelope))
        );
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
        let view = |seq: u64, envelope: bool, v: Vec<f32>| crate::board::Frame {
            fixed_ti: Some((n / 2) as f64),
            ..frame(seq, envelope, v)
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
            shared.lock().unwrap().queue.push_back(crate::board::Frame {
                seq,
                dt: window * 1.2 / n as f64,
                pre: n / 2,
                ch: [v, Vec::new(), Vec::new(), Vec::new()],
                envelope,
                acq: (1e9, 10_000_000),
                ..Default::default()
            });
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
        let frame = |seq: u64| crate::board::Frame {
            seq,
            dt: window * 1.2 / n as f64,
            pre: n / 2,
            ch: [v.clone(), Vec::new(), Vec::new(), Vec::new()],
            ..Default::default()
        };
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
            ch: [
                (0..n).map(|k| if (k / 200) % 2 == 1 { 1.0 } else { 0.0 }).collect(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut u = crate::ui::testkit::demo_ui();
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

        let mut ui = crate::ui::testkit::demo_ui();
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
            let mut ui = crate::ui::testkit::demo_ui();
            ui.m.dec_on = true;
            ui.set_dec_proto(proto);
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
            let _ =
                crate::store::write(&dir.join(format!("i-{:?}.png", proto).to_lowercase()), &crate::store::png_of(&cv));
        }
    }

    // Break it catches: the Bus trigger's own switches staying after going back to Edge (decoder on: the live picture
    // stayed raw; its demo source channels stayed on), or switching off what the user had on before Bus
    #[test]
    fn leaving_the_bus_trigger_undoes_only_what_it_switched_on() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
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
            let mut ui = crate::ui::testkit::demo_ui();
            ui.m.dec_proto = Proto::CAN;
            ui.set_trig_type(TrigType::Bus);
            ui.m.trig_bus_val = id as u64;
            ui.m.tscale = tscales().iter().position(|&t| (t - 50e-6).abs() < 1e-9).unwrap();
            for _ in 0..10 {
                ui.step(1.0 / 30.0);
            }
            assert!(ui.last[0].as_ref().unwrap().triggered, "id {:x} not triggered", id);
            let p = ui.decode_packets().unwrap();
            let hit = p
                .iter()
                .find(|x| matches!(x.kind, Kind::Id { .. }) && x.value == id)
                .expect("matching ID in the record");
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
        let mut ui = crate::ui::testkit::demo_ui();
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
                let mut ui = crate::ui::testkit::demo_ui();
                ui.set_dec_proto(proto);
                ui.set_trig_type(TrigType::Bus);
                let id_default = ui.m.trig_bus_val;
                ui.m.trig_bus = what;
                // a payload value of the demo traffic
                if what == BusWhat::Data {
                    ui.m.trig_bus_val = if proto == arinc {
                        0x2_5a5a
                    } else if proto == sent {
                        0x7f_480b
                    } else {
                        0x55
                    };
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
                assert!(
                    crate::trig::points(&ui.m, &p).iter().any(|t| t.abs() < tol),
                    "{:?} {:?}: no matching field ends at 0: {:?}",
                    proto,
                    what,
                    p
                );
                if what == BusWhat::Id && proto != sent {
                    let want = if proto == arinc { 0o310 } else { 0x2a };
                    assert_eq!(id_default, want);
                    assert!(p
                        .iter()
                        .any(|x| matches!(x.kind, Kind::Id { .. }) && x.value as u64 == want && x.t1.abs() < tol));
                }
                if what == BusWhat::Id {
                    let mut cv = Canvas::new(PANEL_W, PANEL_H);
                    ui.render(&mut cv);
                    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = crate::store::write(
                        &dir.join(format!("j-bus-{}.png", proto.key()).to_lowercase()),
                        &crate::store::png_of(&cv),
                    );
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
        for (what, val) in [
            (BusWhat::Id, None),
            (BusWhat::Id, Some(0x4B)),
            (BusWhat::Data, Some(0x40)),
            (BusWhat::Frame, None),
            (BusWhat::Error, None),
        ] {
            let mut ui = crate::ui::testkit::demo_ui();
            ui.m.dec_proto = Proto::from_key("Usb").unwrap();
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
            assert!(
                crate::trig::points(&ui.m, &p).iter().any(|t| t.abs() < tol),
                "USB {:?}: no matching field ends at 0: {:?}",
                what,
                p
            );
            match (what, val) {
                (BusWhat::Id, None) => {
                    assert!(p.iter().any(|x| x.kind == Kind::Field("ADDR") && x.value == 5 && x.t1.abs() < tol))
                }
                (BusWhat::Id, Some(v)) => {
                    assert!(p.iter().any(|x| x.kind == Kind::Field("PID") && x.value as u64 == v && x.t1.abs() < tol))
                }
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
            let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
                assert!(
                    bad.is_none(),
                    "{} {}: pixel {:?} differs: {:08x} drawn, {:08x} replayed",
                    what,
                    name,
                    bad.map(|i| (i as i32 % w, i as i32 / w)),
                    bad.map_or(0, |i| a.px[i]),
                    bad.map_or(0, |i| b.px[i])
                );
            }
        };
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let kinds: Vec<String> =
            ui.azr.findings.iter().map(|f| format!("{:?} {:?} {:.2}", f.channels, f.kind, f.confidence)).collect();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        assert!(
            cmds.contains(":SOUR1:APPL:SIN 1000,1,0")
                && cmds.contains(":OUTP1 ON")
                && cmds.contains(":SOUR1:APPL:SIN 100000"),
            "{}",
            cmds
        );
        let mut cv = Canvas::new(PANEL_W, PANEL_H);
        ui.render(&mut cv);
    }

    #[test]
    fn bode_sweep_with_the_builtin_generator() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
        (ui.m.bode_gen, ui.m.bode_start, ui.m.bode_stop, ui.m.bode_points, ui.m.bode_in, ui.m.bode_out) =
            (bode::BUILTIN, 1e3, 1e5, 3, WaveId::Ch(0), WaveId::Ch(0));
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
    #[cfg_attr(not(feature = "decoders"), ignore = "needs every decoder")]
    fn i2s_and_1553_decode_the_demo() {
        use super::*;
        use oos_dsp::decode::Kind;
        for (key, tdiv) in [("I2s", 10e-6f32), ("Mil1553", 20e-6)] {
            let proto = Proto::from_key(key).unwrap();
            let mut ui = crate::ui::testkit::demo_ui();
            ui.m.dec_on = true;
            ui.set_dec_proto(proto);
            // the trigger that holds the demo still: I2S the falling WS edge on CH1, 1553 the first rising edge after
            // 3 us of quiet bus on CH3
            let m = &ui.m;
            let want = if key == "I2s" {
                (TrigType::Edge, 0, Slope::Fall, 1.65)
            } else {
                (TrigType::NthEdge, 2, Slope::Rise, 2.0)
            };
            assert_eq!((m.trig_type, m.trig_src, m.slope, m.trig_level), want, "{}", key);
            assert!(key == "I2s" || (m.nth_idle, m.nth_n) == (3e-6, 1));
            ui.m.tscale = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
            for _ in 0..20 {
                ui.step(1.0 / 30.0);
            }
            let p = ui.decode_packets().unwrap();
            if key == "I2s" {
                let s: Vec<(bool, u32)> = p
                    .iter()
                    .filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None })
                    .collect();
                assert!(s.contains(&(false, 0x4000)) && s.contains(&(true, 0xc000)), "{:x?}", s);
            } else {
                let w: Vec<(bool, u32, bool)> = p
                    .iter()
                    .filter_map(|x| if let Kind::Word { cmd } = x.kind { Some((cmd, x.value, x.ok)) } else { None })
                    .collect();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
            let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
            let hits = (a.y..a.y + a.h)
                .flat_map(|y| (a.x..a.x + a.w).map(move |x| (x, y)))
                .filter(|&(x, y)| pal.contains(&cv.px[(y * W + x) as usize]))
                .count();
            // Axes and measurement labels cover a few trace pixels. The zoom
            // map can contain fewer than 1000 active pixels at this timebase.
            assert!(hits * 2 > active, "zoom {zoom}: {hits} of {active} graded pixels remain visible");
        }
    }

    /// Closing the display must not undo explicit overview/timebase changes.
    #[test]
    fn zoom_close_preserves_explicit_main_timebase_changes() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
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
        assert!(
            c.x >= row.x && c.x + c.w <= row.x + row.w && c.y >= row.y && c.y + c.h <= row.y + row.h,
            "close {:?} readout {:?}",
            c,
            row
        );
        let (o, z) = Ui::split_zoom(ui.grid);
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let dir = crate::ui::testkit::Scratch::new("mask");
        let mut ui = crate::ui::testkit::demo_ui();
        ui.store_dir = dir.clone();
        ui.store_dest = crate::store::StoreDest::Local;
        ui.step(0.033);
        let rec = ui.last[0].as_ref().unwrap();
        ui.refs[0] = Some(RefWave { label: "R1".into(), ..RefWave::test(rec.ch[0].clone(), rec.dt, rec.t0) });
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
        let dir = crate::ui::testkit::Scratch::new("ff");
        let mut ui = crate::ui::testkit::demo_ui();
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
        let dir = crate::ui::testkit::Scratch::new("ui-store");
        let mut ui = crate::ui::testkit::demo_ui();
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
        let f = crate::board::Frame {
            seq: 1,
            dt: 1e-9,
            pre: 500,
            ch: [v, Vec::new(), Vec::new(), Vec::new()],
            acq: (1e9, 1000),
            ..Default::default()
        };
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
        let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
        let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
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
            let v: Vec<f32> = (0..n)
                .map(|k| (2.0 * std::f64::consts::PI * 200e3 * (k as f64 - pre as f64 + 0.3) * dt).sin() as f32)
                .collect();
            crate::board::Frame {
                seq,
                dt,
                pre,
                ch: [v, Vec::new(), Vec::new(), Vec::new()],
                roll: false,
                envelope: false,
                scan: false,
                partial: false,
                acq: (1.25e9, n),
                stamp: 0,
                la: None,
                fixed_ti: None,
                hw_avg: false,
                basic: Default::default(),
            }
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
                    (0..n)
                        .map(|k| {
                            (2.0 * std::f64::consts::PI
                                * 2e6
                                * (c + 1) as f64
                                * (k as f64 - pre as f64 + 0.1 * seq as f64)
                                * dt)
                                .sin() as f32
                        })
                        .collect()
                });
                crate::board::Frame {
                    seq,
                    dt,
                    pre,
                    ch,
                    roll: false,
                    envelope: false,
                    scan: false,
                    partial: false,
                    acq: (1.0 / dt, n),
                    stamp: 0,
                    la: None,
                    fixed_ti: None,
                    hw_avg: false,
                    basic: Default::default(),
                }
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
                        let mut v: Vec<f32> = (0..n)
                            .map(|k| {
                                3.0 * (2.0
                                    * std::f64::consts::PI
                                    * 1.3e6
                                    * (k as f64 - pre as f64 + 0.37 * seq as f64)
                                    * dt)
                                    .sin() as f32
                            })
                            .collect();
                        v[pre + 7] = if seq % 3 == 0 { 9.0 } else { -1.5 };
                        crate::board::Frame {
                            seq,
                            dt,
                            pre,
                            ch: [v, Vec::new(), Vec::new(), Vec::new()],
                            roll: false,
                            envelope: false,
                            scan: false,
                            partial: false,
                            acq: (1.0 / dt, n),
                            stamp: 0,
                            la: None,
                            fixed_ti: None,
                            hw_avg: false,
                            basic: Default::default(),
                        }
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
            let v: Vec<f32> = (0..n)
                .map(|k| (0.25 + a * (2.0 * std::f64::consts::PI * 2e6 * (k as f64 - pre as f64) * dt).sin()) as f32)
                .collect();
            crate::board::Frame {
                seq,
                dt,
                pre,
                ch: [v, Vec::new(), Vec::new(), Vec::new()],
                roll: false,
                envelope: false,
                scan: false,
                partial: false,
                acq: (1.0 / dt, n),
                stamp: 0,
                la: None,
                fixed_ti: None,
                hw_avg: false,
                basic: Default::default(),
            }
        };
        let shared: crate::board::HwShared = Default::default();
        let mut ui = Ui::new(Source::Hw { shared: shared.clone(), seen: 0 });
        ui.m.tscale = tscales().iter().position(|&x| (x - 1e-6).abs() < 1e-9).unwrap();
        ui.m.trig_level = 0.0;
        ui.m.ch[0].offset = 0.0;
        ui.m.meas = vec![
            (Meas::Vpp, WaveId::Ch(0)),
            (Meas::Vavg, WaveId::Ch(0)),
            (Meas::Vrms, WaveId::Ch(0)),
            (Meas::StdDev, WaveId::Ch(0)),
            (Meas::Freq, WaveId::Ch(0)),
        ];
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
        assert!(
            (ui.mstat[0].max - 6.0).abs() < 0.01 && (ui.mstat[0].min - 2.0).abs() < 0.01,
            "{} {}",
            ui.mstat[0].min,
            ui.mstat[0].max
        );
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
        let frame = |seq: u64| crate::board::Frame {
            seq,
            dt: 0.8e-9,
            pre: 50,
            ch: [vec![0.0; 100], Vec::new(), Vec::new(), Vec::new()],
            ..Default::default()
        };
        shared.lock().unwrap().queue.extend((1..=100).map(frame));
        let mut src = Source::Hw { shared: shared.clone(), seen: 0 };
        let got = src.drain_newest(30);
        assert_eq!(got.iter().map(|f| f.seq).collect::<Vec<_>>(), (70..=99).collect::<Vec<_>>());
        let g = shared.lock().unwrap();
        assert_eq!((g.queue.len(), g.queue[0].seq, g.dropped), (1, 100, 69));
        drop(g);
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        let flat = |seq: u64, v: f32, hw_avg: bool| crate::board::Frame {
            seq,
            dt: 0.8e-9,
            pre: 6250,
            ch: [vec![v; 12_500], Vec::new(), Vec::new(), Vec::new()],
            acq: (1.25e9, 12_500),
            hw_avg,
            ..Default::default()
        };
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
        let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
        let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
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
        let (mut scpi, mut ui, cv) = crate::ui::testkit::scpi_session();
        let mut q = |ui: &mut Ui, c: &str| crate::ui::testkit::q(&mut scpi, ui, &cv, c);
        assert_eq!(q(&mut ui, ":ACQ:TYPE?"), "NORM");
        q(&mut ui, ":ACQuire:TYPE AVERages");
        q(&mut ui, ":ACQ:AVER 256");
        q(&mut ui, ":ACQ:AVER:HARD ON");
        assert_eq!((ui.m.acq, ui.m.avg_count, ui.m.avg_hw), (AcqMode::Average, 256, true));
        assert_eq!(
            (q(&mut ui, ":ACQ:TYPE?"), q(&mut ui, ":ACQ:AVER?"), q(&mut ui, ":ACQ:AVER:HARD?")),
            ("AVER".into(), "256".into(), "1".into())
        );
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

    /// Layout: the plot fills the width and the height between the top bar and the cards (results as an overlay),
    /// touch targets of at least 40 px everywhere.
    #[test]
    fn layout_plot_size_and_touch_targets() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
        // default: one measurement row under the plot
        assert_eq!((ui.grid.x, ui.grid.y, ui.grid.w, ui.grid.h), (0, 40, 1024, 456));
        ui.m.meas.clear();
        ui.relayout();
        assert_eq!((ui.grid.w, ui.grid.h), (1024, 480));
        // statistics: the results panel takes no room
        ui.m.meas = vec![(Meas::Freq, WaveId::Ch(0))];
        ui.stats_on = true;
        ui.relayout();
        assert_eq!((ui.grid.w, ui.grid.h), (1024, 480));
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
            let (sb, mb) = Ui::trig_parts(trg);
            let menu = ui.menu_btn();
            assert!(chs[0].x >= menu.right() && trg.right() <= ui.scr.w - 6 && trg.w >= 222 && tr.w >= 130, "{} on", n);
            for r in chs.iter().chain([tr, trg, menu].iter()) {
                assert!(r.h >= 40 && r.bottom() <= ui.scr.h && r.w >= 54, "{:?}", r);
            }
            // two buttons in a card: the finger gets half of its height each
            assert!(trg.h / 2 >= 32 && sb.w >= 60 && mb.y > sb.y + sb.h, "{:?} {:?}", sb, mb);
        }
        for m in [
            MenuId::Main,
            MenuId::Trigger,
            MenuId::Math,
            MenuId::MeasAdd,
            MenuId::Utility,
            MenuId::Channel(0),
            MenuId::Display,
            MenuId::Acquire,
        ] {
            ui.open_menu(m);
            let d = ui.dlg_layout().unwrap();
            let fr = ui.frame();
            assert!(
                d.close.h >= 40 && d.r.y >= fr.top.bottom() && d.r.bottom() <= fr.bottom.y,
                "{:?} {:?} {:?}",
                m,
                d.r,
                fr.bottom
            );
            assert!(d.rows.iter().all(|r| r.ctrl.h >= 40), "{:?}", m);
        }
    }

    /// Trigger > Type and Math > Operation: the value opens a picker, picking sets the value and returns to the menu
    /// (with its Back target); Back leaves it unchanged; the knobs still step the item.
    #[test]
    fn pickers_open_and_set_the_value() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
        let items = [
            Item::TrigType,
            Item::MathOpI,
            Item::MathFam,
            Item::FftWin,
            Item::VidSync,
            Item::AcqDepth,
            Item::Persist,
            Item::AcqAvg,
            Item::DecBaud,
            Item::QuickAct,
            Item::AcqMode,
            Item::TrigWhen,
            Item::Pat(1),
            Item::DecCs,
            Item::DecProto,
            Item::GridStyle,
            Item::ChProbe(0),
            Item::TrigMode,
            Item::StoreDest,
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
        for it in
            [Item::TrigType, Item::FftWin, Item::AcqDepth, Item::Persist, Item::AcqAvg, Item::DecBaud, Item::QuickAct]
        {
            assert!(matches!(ui.ctl(it, 300), Ctl::Pick), "{:?}", it);
        }
        assert!(matches!(ui.ctl(Item::AcqMode, 300), Ctl::Seg(..)));
    }

    /// Clear in STOP: maps, record, measurements, statistics and mask counts are gone and stay gone; the next
    /// acquisition (SINGLE) draws again.
    #[test]
    fn clear_empties_the_display_in_stop() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
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
        let yellow = (a.y + 60..a.y + a.h - 20)
            .flat_map(|y| (a.x + 60..a.x + a.w - 60).map(move |x| (x, y)))
            .filter(|&(x, y)| cv.px[(y * PANEL_W + x) as usize] == CH_COLORS[0])
            .count();
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
        let mut ui = crate::ui::testkit::demo_ui();
        let rest = vec![
            Item::MeasSrc,
            Item::MeasSrcB,
            Item::MeasCopyTo,
            Item::MeasGate,
            Item::MeasRefs,
            Item::MeasStats,
            Item::MeasLabels,
            Item::StripDock,
            Item::MeasIdle,
            Item::MeasTsetup,
            Item::MeasThold,
            Item::MeasOutside,
            Item::MeasN,
            Item::MeasEdges,
            Item::Open(MenuId::MeasPair),
            Item::Open(MenuId::Dvm),
            Item::MeasClear,
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
        ui.step(0.5);
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
        let dir = crate::ui::testkit::Scratch::new("ui-quick");
        let mut ui = crate::ui::testkit::demo_ui();
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
            let mut ui = crate::ui::testkit::demo_ui();
            ui.m.text_size = ts;
            ui.relayout();
            let f = ui.fonts();
            let r = ui.run_pill();
            let room = ui.top_buttons().first().map_or(ui.top_right(), |b| b.2.x) - 12;
            for (sr, depth) in [(1.25e9, 125e6), (625e6, 62.5e6), (3.125e6, 1.25e6)] {
                let (bold, main, mode) = Ui::top_acq(f, r.x + r.w + 14, room, sr, depth, "Average 1024".into());
                let w =
                    if bold { f.mb.width(&main) } else { f.m.width(&main) } + mode.as_ref().map_or(0, |m| f.m.width(m));
                assert!(r.x + r.w + 14 + w <= room, "{:?} {} {}", ts, main, w);
            }
        }
    }

    /// With several channels on, the key of a channel that is not selected only selects it; the next press opens its
    /// menu, the one after that switches it off. Another channel's menu closes, other menus stay.
    #[test]
    fn a_channel_key_first_selects_when_several_are_on() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
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
        let mut ui = crate::ui::testkit::demo_ui();
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
        // a finger held on the channel's card: AC/DC, and lifting it opens no menu
        ui.close_menu();
        let (x, y) = mid(ui.cards().0[0]);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(LONG_PRESS + 0.05);
        assert_eq!(ui.m.ch[0].coupling, Coupling::Ac);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        assert!(!ui.menu_open, "no tap after a long touch");
        // a short one is a tap
        ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
        ui.step(0.1);
        ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        assert!(ui.menu_open && ui.menu == MenuId::Channel(0) && ui.m.ch[0].coupling == Coupling::Ac);
    }

    /// Every front-panel key function is in the start menu, and every menu is reachable by its key and by touch.
    #[test]
    fn start_menu_and_keys_reach_everything() {
        use super::*;
        let mut ui = crate::ui::testkit::demo_ui();
        for _ in 0..3 {
            ui.step(0.033);
        }
        // the menu keys
        for (k, m) in [
            (key::HMENU, MenuId::Acquire),
            (key::ANALYSE, MenuId::Decode),
            (key::TMENU, MenuId::Trigger),
            (key::MEASURE, MenuId::MeasAdd),
            (key::CURSOR, MenuId::Cursor),
            (key::MATH, MenuId::Math),
            (key::REF, MenuId::Ref),
        ] {
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
        for it in [
            Item::RunStop,
            Item::SingleAct,
            Item::AutosetAct,
            Item::ClearDisp,
            Item::DefaultAct,
            Item::SlopeAct,
            Item::ForceAct,
            Item::NavHist,
            Item::QuickRun,
            Item::Shot,
            Item::LockAct,
            Item::PowerAct,
        ] {
            assert!(all.contains(&it), "{:?}", it);
        }
        for m in [
            MenuId::Acquire,
            MenuId::Trigger,
            MenuId::Horizontal,
            MenuId::Measure,
            MenuId::Cursor,
            MenuId::Math,
            MenuId::Ref,
            MenuId::Decode,
            MenuId::Display,
            MenuId::Storage,
            MenuId::Mask,
            MenuId::Utility,
        ] {
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
        let mut ui = crate::ui::testkit::demo_ui();
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
fn time_ladder_step(x: f32, d: i32) -> f32 {
    let l = time_ladder();
    let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
    l[step_index(i, l.len(), d)]
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
