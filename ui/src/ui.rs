// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Layout, rendering, menus and input handling of the OOS scope screen (1024 x 600).

use crate::font::{Font, Fonts};
use crate::math::*;
use crate::gfx::*;
use crate::model::*;
use crate::wave::*;
use crate::board::{board, Board};

mod analysis;
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
pub mod startup;
mod afg;
mod sysinfo;
mod tables;
mod logger;
mod measlabels;
mod overclock;
mod tools;
mod update;
mod viewmore;
mod wlan;
mod windows;
#[cfg(test)]
mod lint;
mod bwtest;
pub mod la;

pub const W: i32 = 1024;
pub const H: i32 = 600;
/// Top bar: run state, menu buttons, Quick / Autoset / Single / System (buttons drawn 30 px high, tapped over the
/// full 40 px).
pub const TOP: Rect = Rect::new(0, 0, W, 40);
/// Plot: the whole width and everything between the top bar and the cards; the channel markers (left edge), the
/// trigger level marker (right edge), the trigger position marker (top edge), the axis labels, the status line and the
/// measurement results are drawn inside it.
const PLOT_X: i32 = 0;
const PLOT_W: i32 = W;
const PLOT_Y: i32 = TOP.h;
/// Width of the marker strips at the left and right edge of the plot (the labels keep clear of them).
const MARK_W: i32 = 26;
pub const BOTTOM: Rect = Rect::new(0, 526, W, 74);
/// Width of the measurement results overlay (statistics on) at the top right of the plot.
const RESULTS_W: i32 = 196;
const STRIP_ROW: i32 = 18;
/// Menu dialogs: header height, row pitch and control height of the list rows (touch targets of 40 px).
const DLG_HEAD: i32 = 50;
const DLG_ROW: i32 = 44;
const DLG_CTRL: i32 = 40;
/// records kept for history playback
const HISTORY: usize = 200;
/// LA words a history entry keeps: above this only the newest record has its LA (200 x 1 M words = 400 MB at most).
const HISTORY_LA_WORDS: usize = 1_000_000;
/// Segment counts of the segmented recording (0 = off).
pub const SEGMENTS: [usize; 7] = [0, 10, 50, 100, 200, 500, 1000];
pub const SEG_MAX: usize = 1000;

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

/// Averaging counts of Acquire > Averages.
/// Stops of the density scale, rare to frequent (`heat_palette`).
const HEAT_STOPS: [(f32, f32, f32); 5] = [(40.0, 70.0, 255.0), (0.0, 220.0, 255.0), (40.0, 240.0, 70.0), (255.0, 235.0, 30.0), (255.0, 40.0, 30.0)];
const AVERAGES: [u32; 10] = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];

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

/// Options of the ARINC 429 bit rate (decode::ARINC_RATES).
const ARATE_NAMES: [&str; 3] = ["Auto", "12.5 k", "100 k"];

/// Options of the USB speed (decode::USB_SPEEDS).
const USPEED_NAMES: [&str; 3] = ["Auto", "Low 1.5M", "Full 12M"];

/// A SENT clock tick (0 = Auto).
fn tick_name(t: f64) -> String {
    if t > 0.0 {
        si_short(t, "s")
    } else {
        "Auto".into()
    }
}

/// Longest record the web page takes as CSV (about 50 MB of text).
const WEB_CSV_MAX: usize = 2_000_000;

/// Height of a decoder band in the plot.
const DEC_BAND: i32 = 28;

/// Colours of the references.
const REF_COLORS: [(&str, Rgb); 5] = [("White", rgb(0xd8, 0xd8, 0xd8)), ("Red", rgb(0xf0, 0x60, 0x60)), ("Green", rgb(0x60, 0xe0, 0x90)), ("Blue", rgb(0x70, 0xa8, 0xf8)), ("Violet", rgb(0xc8, 0x88, 0xf0))];
/// Kinds of delay and phase measurement and of second panes, for the setups.
pub const PAIRS_N: usize = tools::PAIRS.len();
pub const PANES_N: usize = tables::PANES.len();
pub use eyepage::EYE_ITEMS;
pub use panes::{PANE_HARMS, TREND_SEL_N};
pub const LOG_STEPS_N: usize = logger::LOG_STEPS.len();

/// Keys of the frequency keypad: the unit ends the entry.
const FREQ_KEYS: [&str; 18] = ["7", "8", "9", "mHz", "4", "5", "6", "Hz", "1", "2", "3", "kHz", "0", ".", "Del", "MHz", "Clear", "Cancel"];
/// Keys of the hex keypad picker.
const KEYPAD: [&str; 19] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "Del", "Clear", "OK"];

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
    Bode,
    Sim,
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
}

/// Buttons of the top bar.
#[derive(Clone, Copy, PartialEq, Debug)]
enum TopBtn {
    Menu(MenuId),
    System,
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

#[derive(Clone)]
struct RefWave {
    src: usize,
    /// index into REF_COLORS
    color: usize,
    label: String,
    t0: f64,
    dt: f64,
    v: Vec<f32>,
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
    DecOn,
    DecSrc,
    DecBaud,
    DecCanRate,
    DecCanH,
    DecFdRate,
    DecArate,
    DecTick,
    DecNibbles,
    DecUsbSpeed,
    DecSrc4,
    DecDiff,
    TrigBusWhat,
    TrigBusVal,
    DecThr,
    DecFmt,
    DecProto,
    DecSrc2,
    DecCs,
    DecMode,
    DecBits,
    DecParity,
    Zoom,
    AutoRoll,
    ZScale,
    ZDelay,
    MeasStats,
    MeasLabels,
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
    MeasSrc,
    MeasAdd(Meas),
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
    MeasTsetup,
    MeasThold,
    CurMode,
    CurA,
    CurB,
    Persist,
    Intensity,
    GridStyle,
    /// waveform windows (ui/windows.rs)
    WinLayout,
    FullScreen,
    WinCh(usize),
    WinMath(usize),
    WinLa,
    ClearDisp,
    Dots,
    Hdmi,
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
    BodeCur,
    BodeMode,
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
    TouchLock,
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
    WlanInfo,
    WlanScan,
    WlanForget,
    WlanNet,
    WlanForgetNet,
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
    SimRj,
    SimPj,
    SimPjHz,
    SimDefaults,
    Offline,
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
    /// drawing zone k (box: from the touch-down point to the finger; shape: the finger's path)
    Zone(usize),
    Offset(usize),
    Delay,
    Level,
    CursorA,
    CursorB,
}

struct Pinch {
    active: bool,
    p0: (i32, i32),
    p1: (i32, i32),
    ref_dx: i32,
    ref_dy: i32,
}

struct TouchState {
    down: bool,
    x0: i32,
    y0: i32,
    x: i32,
    y: i32,
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

pub struct Ui {
    pub m: Model,
    pub src: Source,
    pub fonts: Fonts,
    pub ph: Phosphor,
    pub phz: Phosphor,
    area: Rect,
    /// plot rectangle (10 x 8 div) of the current layout
    grid: Rect,
    layout_zoom: bool,
    /// the waveform windows in the plot (one: the plot itself); `area` is the first while nothing else is drawn
    wins: Vec<Rect>,
    /// the window being drawn
    cur_win: usize,
    /// the plot over the whole screen, without the top bar and the cards
    full: bool,
    vmaxz: [u32; NCH],
    rng: Rng,
    menu: MenuId,
    menu_open: bool,
    /// menu that opened the current one (Back button)
    parent: Option<MenuId>,
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
    ana: [oos_dsp::measure::Analysis; NCH],
    /// measurements of each ref slot, from the stored samples (no front-end bandwidth)
    ref_ana: [oos_dsp::measure::Analysis; NREF],
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
    /// records dropped by the zone condition
    zone_drops: u64,
    /// fast path frames by where the edge of the set direction was: at the hardware trigger point; further away with
    /// an edge of the other direction at the point (the hardware fired on the wrong edge); further away with no edge at
    /// the point (the point is off); none found. And the offsets of the last kind in samples: sum, largest
    /// (`stat`, trigger.md 8)
    pub fast_edges: [u64; 4],
    pub fast_off: (f64, f64),
    /// segmented recording: records since the acquisition started, and whether it ran in the last frame
    seg_n: usize,
    was_running: bool,
    /// analysers (analysis.rs)
    azr: analysis::State,
    /// self-calibration page (selfcal.rs)
    cal: selfcal::State,
    /// Bode sweep (bode.rs)
    bode: bode::State,
    /// SIM: walking the loaded serial record on the phosphor
    sim_play: bool,
    sim_i: usize,
    /// Offline mode. Off, the live screen stays as it is.
    offline: bool,
    /// Reference named on the card that replaces the trigger card, and read by the DVM and the counter.
    offline_ref: usize,
    offline_hold: Option<offline::LiveHold>,
    /// HDMI workspace: set by the main loop on frames it shows on the monitor; the waveform area (full width, top bar
    /// to cards) is copied here during `render`
    pub hdmi_snap_want: bool,
    pub hdmi_snap: Vec<u32>,
    /// start-up screen (startup.rs) and the last start-up's result (About)
    pub start_up: Option<startup::Screen>,
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
    pub frames: u64,
    /// records taken in, for the state line
    pub records: u64,
    /// auto set-up under way on the hardware (autoset.rs)
    auto: Option<autoset::AutoRun>,
    /// when the bar of views after an auto set-up came up (ui/autoset.rs)
    auto_bar: Option<f64>,
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
    pub t_step: f32,
    /// parts of `t_step` (ms, smoothed): taking records from the source, drawing them into the phosphor maps
    pub t_acq: f32,
    pub t_draw: f32,
    pub t_render: f32,
    pub t_present: f32,
    pub info: String,
    pub dirty_map: bool,
    vmax: [u32; NCH],
    /// brightest pixel of the merged maps (colour grade), main / zoom view
    vmax_cg: u32,
    vmaxz_cg: u32,
    /// storage directory (setups, CSV, PNG), created when missing
    pub store_dir: std::path::PathBuf,
    /// Auto / USB stick / desktop for waveform, mask and screenshot files
    store_dest: crate::store::StoreDest,
    /// Math menu category (kept when the operation is Off)
    math_fam: MathFamily,
    /// a USB stick is mounted
    usb: bool,
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

impl Ui {
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
            m,
            src,
            fonts: Fonts::load(),
            ph: Phosphor::new(1, 1),
            phz: Phosphor::new(1, 1),
            area: Rect::new(0, 0, 0, 0),
            grid: Rect::new(0, 0, 0, 0),
            layout_zoom: false,
            wins: Vec::new(),
            cur_win: 0,
            full: false,
            vmaxz: [1; NCH],
            rng: Rng::new(0x9e3779b97f4a7c15),
            menu: MenuId::Main,
            menu_open: false,
            parent: None,
            pick: None,
            focus: 0,
            meas_src: WaveId::Ch(0),
            last: [None],
            stats: [Stats::default(); NCH],
            ana: Default::default(),
            ref_ana: std::array::from_fn(|_| Default::default()),
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
            zone_drops: 0,
            fast_edges: [0; 4],
            fast_off: (0.0, 0.0),
            seg_n: 0,
            was_running: false,
            azr: Default::default(),
            cal: Default::default(),
            hdmi_snap_want: false,
            hdmi_snap: Vec::new(),
            start_up: None,
            start_up_last: None,
            bode: Default::default(),
            sim_play: false,
            sim_i: 0,
            offline: false,
            offline_ref: 0,
            offline_hold: None,
            touch: TouchState { down: false, x0: 0, y0: 0, x: 0, y: 0, drag: Drag::None, start_offset: 0.0, start_delay: 0.0, start_level: 0.0, start_cur: 0.0, win: 0 },
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
            meas_src_b: WaveId::Ch(1),
            ref_sel: 0,
            ref_src: 0,
            ref_files: Vec::new(),
            xy: false,
            pinch: Pinch { active: false, p0: (0, 0), p1: (0, 0), ref_dx: 0, ref_dy: 0 },
            knob_t: [0.0; 16],
            now: 0.0,
            frames: 0,
            records: 0,
            auto: None,
            auto_bar: None,
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
            t_step: 0.0,
            t_acq: 0.0,
            t_draw: 0.0,
            t_render: 0.0,
            t_present: 0.0,
            info: String::new(),
            dirty_map: true,
            vmax: [1; NCH],
            vmax_cg: 1,
            vmaxz_cg: 1,
            store_dir: std::path::PathBuf::from(crate::store::DEFAULT_DIR),
            store_dest: crate::store::StoreDest::Auto,
            math_fam: MathFamily::Arith,
            usb: false,
            la_probe: None,
            usb_check: 0.0,
            store_slot: 1,
            mask_ref: None,
            mask: None,
            mask_pass: 0,
            mask_fail: 0,
            shot_request: None,
            quit: false,
        };
        ui.relayout();
        ui
    }

    /// Measurement results in a panel on the right instead of the strip under the grid.
    fn results_panel(&self) -> bool {
        self.stats_on && !(self.m.meas.is_empty() && self.m.meas2.is_empty())
    }

    /// Channels with measurements, in the order of their first measurement (one strip row each).
    fn meas_groups(&self) -> Vec<WaveId> {
        let mut v = Vec::new();
        for id in self.m.meas.iter().map(|e| e.1).chain(self.m.meas2.iter().map(|e| e.1)) {
            if !v.contains(&id) {
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
        let n = self.m.meas.iter().filter(|e| e.1 == id).count() + self.m.meas2.iter().filter(|e| e.1 == id).count();
        n.div_ceil(tools::STRIP_PER_ROW) as i32
    }

    fn strip_rows(&self) -> i32 {
        let dvm = (self.m.dvm_on || self.m.cnt_on) as i32;
        if self.results_pane_on() {
            return dvm;
        }
        let meas = if self.results_panel() { 0 } else { self.meas_groups().iter().map(|&c| self.meas_strip_rows(c)).sum() };
        meas + (self.m.cursor != CursorMode::Off) as i32 + dvm
    }

    /// The plot: 10 x 8 divisions between the top bar and the cards; it gets shorter for strip rows (the results
    /// panel is an overlay and takes no room).
    fn plot_rect(&self) -> Rect {
        // the logic analyser lanes have their own strip under the graticule: the analog picture shrinks with it
        let avail = self.plot_base() - self.la_strip_h() - self.pane_h();
        Rect::new(PLOT_X, self.plot_top(), PLOT_W, (avail / 8 * 8).max(160))
    }

    /// Height for the graticule and the LA strip together (above the measurement rows).
    fn plot_base(&self) -> i32 {
        let rows = self.strip_rows();
        self.plot_bottom() - 2 - self.plot_top() - rows * STRIP_ROW - if rows > 0 { 4 } else { 0 }
    }

    /// Full screen: the plot takes the top bar and the cards too. Not with the touch lock, whose way out is the Menu
    /// button.
    fn full_on(&self) -> bool {
        self.full && !self.m.touch_lock
    }

    fn plot_top(&self) -> i32 {
        if self.full_on() { 0 } else { PLOT_Y }
    }

    /// Where the measurement rows end: the cards, or the bottom of the screen.
    fn plot_bottom(&self) -> i32 {
        if self.full_on() { H } else { BOTTOM.y }
    }

    /// Waveform windows in use: one while zoom, XY or an analysis view has the plot.
    fn n_wins(&self) -> usize {
        if self.m.zoom || self.view_on() { 1 } else { self.m.win_layout.count() }
    }

    /// The windows in the plot `g`, all of the same size (they share the phosphor size), in reading order.
    fn split_wins(&self, g: Rect) -> Vec<Rect> {
        const GAP: i32 = 6;
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
        self.m.ch_win[c].min(self.wins.len().max(1) - 1)
    }

    fn math_win_of(&self, k: usize) -> usize {
        self.m.math_win[k].min(self.wins.len().max(1) - 1)
    }

    /// Whether channel `c` belongs to the window being drawn.
    fn shows(&self, c: usize) -> bool {
        self.win_of(c) == self.cur_win
    }

    /// The rectangle channel `c` is drawn in: its window, or the main waveform area (zoom).
    fn ch_area(&self, c: usize) -> Rect {
        if self.wins.len() > 1 { self.wins[self.win_of(c)] } else { self.area }
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
        (0..NCH).any(|c| self.m.ch[c].on && self.win_of(c) == w) || (0..NMATH).any(|k| self.m.math_ch(k).op != MathOp::Off && self.math_win_of(k) == w)
    }

    /// Window of decoder `k`'s band in the plot, or None when it has a row under the LA lanes.
    fn dec_band_win(&self, k: usize) -> Option<usize> {
        let m = self.m.dec_view(k);
        if m.dec_on_la() {
            return if self.la_dec_count() > 0 { None } else { Some(0) };
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
            self.m.math_win[k] = if self.m.math_ch(k).op != MathOp::Off {
                i += 1;
                (i - 1) % n
            } else {
                0
            };
        }
        // the LA after them: in a window of its own while there is one
        self.m.la_win = if self.m.la_on() { i % n } else { 0 };
    }

    /// The full-screen button at the top right of the plot: (drawn, touch area).
    /// One tap enlarges the waveform over the 1024 x 600 panel; another tap brings the bars back. No extra gesture.
    fn full_btn(&self) -> (Rect, Rect) {
        let g = self.grid;
        // left of the view button while an analysis view has the plot
        let right = if self.view_on() { self.view_btn().x - 6 } else { g.x + g.w - 4 };
        let r = Rect::new(right - 36, g.y + 3, 36, 28);
        (r, Rect::new(r.x - 8, g.y, r.w + 16, 44))
    }

    fn draw_full_btn(&self, cv: &mut Canvas) {
        let (r, _) = self.full_btn();
        let on = self.full_on();
        cv.round(r, 5, if on { mix(PANEL_BG, AMBER, 40) } else { PANEL_BG }, 230);
        cv.round_outline(r, 5, if on { AMBER } else { mix(BTN_BORDER, AMBER, 90) });
        // four corner brackets: at the corners pointing in to enlarge, near the middle pointing out to go back
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let l = 7;
        let col = if on { AMBER } else { TEXT };
        for (sx, sy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            // corner of the bracket, and the way its arms go (x, y)
            let (px, py, ax, ay) = if on { (cx + sx * 4, cy + sy * 3, sx, sy) } else { (cx + sx * 12, cy + sy * 9, -sx, -sy) };
            let (hx0, hx1) = (px.min(px + ax * (l - 1)), px.max(px + ax * (l - 1)));
            let (vy0, vy1) = (py.min(py + ay * (l - 1)), py.max(py + ay * (l - 1)));
            // 2 px thick, the second pixel on the side of the arms
            cv.fill(Rect::new(hx0, py.min(py + ay), hx1 - hx0 + 1, 2), col);
            cv.fill(Rect::new(px.min(px + ax), vy0, 2, vy1 - vy0 + 1), col);
        }
    }

    /// Zoom: overview on top, label row, zoomed view below.
    fn split_zoom(g: Rect) -> (Rect, Rect) {
        (Rect::new(g.x, g.y, g.w, 104), Rect::new(g.x, g.y + 128, g.w, (g.h - 128) / 8 * 8))
    }

    /// The main waveform area (the zoomed view when zoom is on, the first window when the plot is split).
    fn wave_area(&self) -> Rect {
        if self.m.zoom {
            Self::split_zoom(self.grid).1
        } else {
            self.wins.first().copied().unwrap_or(self.grid)
        }
    }

    /// Recomputes the layout and reallocates the phosphor maps when the plot size changed.
    fn relayout(&mut self) {
        let g = self.plot_rect();
        let wins = self.split_wins(g);
        let w0 = wins[0];
        if g != self.grid || self.layout_zoom != self.m.zoom || self.wins.first() != Some(&w0) {
            self.grid = g;
            self.layout_zoom = self.m.zoom;
            if self.m.zoom {
                let (o, z) = Self::split_zoom(g);
                self.ph = Phosphor::new(o.w as usize, o.h as usize);
                self.phz = Phosphor::new(z.w as usize, z.h as usize);
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
        if self.m.zoom {
            (tscales()[self.m.zscale], self.m.zdelay)
        } else {
            (self.m.time_per_div(), self.m.delay)
        }
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
        self.m.zoom = on;
        if on {
            self.m.zscale = self.m.tscale.saturating_sub(3);
            self.m.zdelay = self.m.delay;
        }
        self.relayout();
    }

    /// Logic analyser record of the last acquisition (SCPI).
    pub fn la_record(&self) -> Option<&la::LaRec> {
        self.last[0].as_ref().and_then(|r| r.la.as_ref())
    }

    /// Called after remote (SCPI) changes: same effect as a change on the screen.
    pub fn mark_changed(&mut self) {
        self.settings_changed();
    }

    /// Measurement `k` of channel `c` (NaN when off or not measurable).
    pub fn meas(&self, c: usize, k: Meas) -> f64 {
        self.meas_at(WaveId::Ch(c), k)
    }

    /// Eye figures from the last refresh, in the order the page stores them. Empty until the eye has run.
    pub fn eye_readings(&self) -> &[(&'static str, f64)] {
        &self.azr.page.readings
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

    /// RAW read-out: (points, dt, t0) of every sample of the hardware record inside the screen window; `None` when the
    /// screen record is all there is.
    pub fn raw_info(&self, c: usize) -> Option<(usize, f64, f64)> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        // a compressed frame holds min/max pairs, not samples: then the screen record is the answer
        (r.i1 <= v.len() && r.i1 > r.i0 && !r.frame.envelope).then(|| (r.i1 - r.i0, r.frame.dt, r.t0))
    }

    /// The samples of `raw_info`.
    pub fn raw_volts(&self, c: usize) -> Option<Vec<f32>> {
        let r = self.last[0].as_ref()?.raw.as_ref()?;
        let v = r.frame.ch.get(c)?;
        (r.i1 <= v.len() && r.i1 > r.i0 && !r.frame.envelope).then(|| v[r.i0..r.i1].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect())
    }

    pub fn record_volts(&self, c: usize) -> Vec<f32> {
        self.last[0].as_ref().map(|r| r.ch[c].iter().map(|x| if x.is_finite() { *x } else { 0.0 }).collect()).unwrap_or_default()
    }

    pub fn toast(&mut self, s: &str) {
        self.toast = Some((s.to_string(), self.now + 2.5));
    }

    /// Clear (CLEAR key, Display > Clear, Quick key, SCPI :CLEar): empties the screen like a scope does. The
    /// waveforms, persistence / colour-grade maps, averaging, measurements and their statistics, mask counts and
    /// search marks start over; in STOP the screen stays empty until the next acquisition (RUN draws again at once).
    fn clear_display(&mut self) {
        self.wfm_total = 0;
        self.ph.clear();
        self.phz.clear();
        (self.vmax, self.vmaxz, self.vmax_cg, self.vmaxz_cg) = ([1; NCH], [1; NCH], 1, 1);
        // without a last record there is nothing to redraw in STOP, average with, compute math / decode / search on
        self.last[0] = None;
        self.hist_pos = None;
        self.cleared = true;
        self.stats = [Stats::default(); NCH];
        self.ana = Default::default();
        self.mstat.clear();
        self.mask_pass = 0;
        self.mask_fail = 0;
        self.toast("Display cleared");
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
        }
    }

    // ------------------------------------------------------------------ acquisition per frame

    pub fn step(&mut self, dt_wall: f64) {
        self.relayout();
        self.now += dt_wall;
        self.update_poll();
        self.wlan_poll();
        self.usb_poll();
        self.la_probe_poll();
        self.log_tick();
        self.m.clamp_offsets();
        self.src.publish_afg(&self.m);
        // SINGLE armed: from here on only what comes after the press counts
        let single = self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run;
        if single && !self.single_armed {
            self.m.hw_stamp += 1;
        }
        self.single_armed = single;
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
        if running && !self.was_running && self.m.segments > 0 {
            self.seg_n = 0;
            self.history.clear();
            self.hist_pos = None;
        }
        self.was_running = running;
        let persist = PERSIST[self.m.persist].1;
        // a source started in STOP (replay) still shows its first record, unless the screen was cleared
        if self.offline {
            // the record is the session: no demo acquisition on CH1-CH4
            self.idle_analog();
            let ready = self.refs.get(self.offline_ref).and_then(|r| r.as_ref()).is_some_and(|r| r.v.len() >= 2);
            if self.sim_play && !ready {
                self.sim_play = false;
            }
        } else if self.sim_play && self.m.run == RunState::Run {
            self.sim_play = false;
        }
        // a source started in STOP (replay) still shows its first record, unless the screen was cleared.
        // analog off and no LA: do not arm the ADC / demo path (SIM Play is a stored ref)
        let live = self.m.n_on() > 0 || self.m.la_on();
        if !self.offline && live && (self.m.run == RunState::Run || (self.last[0].is_none() && !self.cleared)) {
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
            let k = match self.m.trig_type {
                // the hardware source delivers batches: draw up to 48 queued records per frame
                _ if matches!(self.src, Source::Hw { .. }) => 48,
                TrigType::Edge | TrigType::Pattern => 6,
                TrigType::Video => 1,
                _ => 2,
            };
            let mut got = 0;
            let (mut ta, mut td) = (0f32, 0f32);
            // fast display path (fastdraw.rs): every queued frame but the newest straight into the phosphor map
            if self.fast_ok() {
                let t0 = std::time::Instant::now();
                let frames = self.src.drain_but_last();
                if !frames.is_empty() && clear_pending {
                    self.ph.clear();
                    clear_pending = false;
                }
                for f in &frames {
                    if self.fast_draw(f) {
                        self.m.status = TrigStatus::Triggered;
                    }
                }
                got += frames.len();
                td += t0.elapsed().as_secs_f32() * 1000.0;
            }
            for _ in 0..k {
                let t0 = std::time::Instant::now();
                let r = self.src.acquire(&self.m, dt_wall / k as f64, &mut self.rng);
                ta += t0.elapsed().as_secs_f32() * 1000.0;
                match r {
                    Some(mut rec) if !crate::zone::pass(&self.m.zones, &rec.ch, rec.t0, rec.dt) => {
                        // zone trigger: not a qualifying record
                        let _ = &mut rec;
                        self.zone_drops += 1;
                    }
                    // protocol trigger, Normal and Single: the hardware fires on every edge of the line, only the decoder
                    // decides; Single waits on for the record that holds the event
                    Some(rec) if self.m.trig_type == TrigType::Bus && self.m.trig_mode != TrigMode::Auto && !rec.triggered && !rec.partial && self.bus_can_match(&rec) => {
                        self.m.status = TrigStatus::Wait;
                    }
                    Some(mut rec) => {
                        self.m.status = if rec.triggered { TrigStatus::Triggered } else { TrigStatus::Auto };
                        for c in 0..NCH {
                            if self.m.ch[c].on && !rec.ch[c].is_empty() && !self.src.is_hw() {
                                let dt = rec.dt;
                                condition(&self.m, c, &mut rec.ch[c], dt);
                            }
                        }
                        if self.m.acq == AcqMode::Average {
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
                        if clear_pending {
                            self.ph.clear();
                            self.phz.clear();
                            clear_pending = false;
                        }
                        let mainv = (self.m.time_per_div(), self.m.delay);
                        let zv = (tscales()[self.m.zscale], self.m.zdelay);
                        let t1 = std::time::Instant::now();
                        let zw = self.z_weights(&rec);
                        for c in 0..NCH {
                            if self.m.ch[c].on && !rec.ch[c].is_empty() && self.m.z_src != Some(c) {
                                self.ph.draw_ex(c, &self.m, mainv, rec.t0, rec.dt, &rec.ch[c], 24, zw.as_deref());
                                if self.m.zoom {
                                    self.phz.draw_ex(c, &self.m, zv, rec.t0, rec.dt, &rec.ch[c], 24, zw.as_deref());
                                }
                            }
                        }
                        td += t1.elapsed().as_secs_f32() * 1000.0;
                        if self.history.len() >= HISTORY.max(self.m.segments) {
                            self.history.pop_front();
                        }
                        // a deep LA record is tens of MB: the history keeps the LA of short records only
                        let la = rec.la.clone().filter(|l| l.words.len() <= HISTORY_LA_WORDS);
                        self.history.push_back(Record { raw: None, la, ..rec.clone() });
                        self.hist_pos = None;
                        self.mask_test(&rec);
                        self.last[0] = Some(rec);
                        self.cleared = false;
                        got += 1;
                        self.records += 1;
                        if self.m.segments > 0 {
                            self.seg_n += 1;
                            if self.seg_n >= self.m.segments {
                                self.m.run = RunState::Stop;
                                self.toast(&format!("{} segments recorded", self.seg_n));
                                break;
                            }
                        } else if self.m.trig_mode == TrigMode::Single && !self.last[0].as_ref().is_some_and(|r| r.partial) {
                            self.m.run = RunState::Stop;
                            break;
                        }
                    }
                    None => {}
                }
                if self.m.run == RunState::Stop {
                    break;
                }
            }
            // WAIT only when the whole frame brought nothing (the hardware source delivers a new record only every
            // few attempts; an empty attempt must not override TRIG'D)
            // at large depths the hardware delivers fewer records than frames: WAIT only after 0.5 s without one, or
            // 1.5 record lengths at slow time bases (a 1 s record at 100 ms/div comes once per second), or 2.5 usual
            // record intervals (a 125 M record takes ~1 s to read)
            let wait_after = 0.5f64.max(1.5 * HDIV as f64 * self.m.time_per_div() as f64).max(2.5 * self.rec_iv);
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
            if got > 0 {
                self.measure_last();
                self.analyzer_update();
                self.protocol_watch();
                self.bode_update();
                let rows = self.meas_rows();
                self.trend_update(self.mstat.is_empty(), &rows);
                self.mstat.resize(rows.len(), MStat::default());
                self.counter_update();
                for (i, row) in rows.iter().enumerate() {
                    let v = row.v;
                    if v.is_finite() {
                        let st = &mut self.mstat[i];
                        if st.n == 0 {
                            st.min = v;
                            st.max = v;
                        }
                        st.n += 1;
                        st.sum += v;
                        st.sumsq += v * v;
                        st.min = st.min.min(v);
                        st.max = st.max.max(v);
                    }
                }
            }
        } else if !self.offline {
            // stopped: redraw the stored record with the current scale/offset
            self.ph.clear();
            self.phz.clear();
            let mainv = (self.m.time_per_div(), self.m.delay);
            let zv = (tscales()[self.m.zscale], self.m.zdelay);
            if let Some(rec) = self.last[0].clone() {
                let zw = self.z_weights(&rec);
                for c in 0..NCH {
                    if self.m.ch[c].on && !rec.ch[c].is_empty() && self.m.z_src != Some(c) {
                        self.ph.draw_ex(c, &self.m, mainv, rec.t0, rec.dt, &rec.ch[c], 24 * 6, zw.as_deref());
                        if self.m.zoom {
                            self.phz.draw_ex(c, &self.m, zv, rec.t0, rec.dt, &rec.ch[c], 24 * 6, zw.as_deref());
                        }
                    }
                }
            }
        }
        if self.m.run == RunState::Stop {
            self.m.status = TrigStatus::Stop;
            self.wfm_rate = 0.0;
        }
        self.pane_tick();
        // brightest pixel per channel: the trace brightness is graded relative to it
        // (follow upwards at once, downwards slowly: no flicker)
        let follow = |v: u32, mx: u32| if mx >= v { mx } else { (v * 7 + mx) / 8 }.max(1);
        let (mut full, mut fullz) = (false, false);
        for c in 0..NCH {
            if self.m.ch[c].on {
                let mx = self.ph.map[c].iter().copied().max().unwrap_or(0) as u32;
                self.vmax[c] = follow(self.vmax[c], mx);
                full |= mx > 0xc000;
                if self.m.zoom {
                    let mz = self.phz.map[c].iter().copied().max().unwrap_or(0) as u32;
                    self.vmaxz[c] = follow(self.vmaxz[c], mz);
                    fullz |= mz > 0xc000;
                }
            }
        }
        if self.m.color_grade {
            let on: [bool; NCH] = std::array::from_fn(|c| self.m.ch[c].on);
            self.vmax_cg = follow(self.vmax_cg, self.ph.merged_max(on));
            if self.m.zoom {
                self.vmaxz_cg = follow(self.vmaxz_cg, self.phz.merged_max(on));
            }
        }
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
                None => format!("Trig CH{} {}", m.trig_src + 1, si_short(m.trig_level as f64, "V")),
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

    /// The fast display path applies: hardware frames with a plain edge trigger and nothing that needs every record
    /// as a full record (averaging, mask test, zones, segments, zoom, XY, Z input, software coupling or filter).
    fn fast_ok(&self) -> bool {
        let m = &self.m;
        matches!(self.src, Source::Hw { .. })
            && m.trig_type == TrigType::Edge
            && m.acq == AcqMode::Normal
            && m.trig_mode != TrigMode::Single
            && m.segments == 0
            && !m.mask_on
            && !m.zoom
            && !self.xy
            && m.z_src.is_none()
            && !m.zones.iter().any(|z| z.usable())
            && board().slow_mode(m) == crate::board::Slow::Off
            && (0..NCH).all(|c| !m.ch[c].on || (matches!(m.ch[c].coupling, Coupling::Dc | Coupling::Dc50) && m.ch[c].bw != BW_20M))
    }

    /// One frame of the fast path into the main phosphor map; false if its trigger edge was not found near the
    /// hardware trigger point. Sparse frames (fewer than 2 samples per column) take the record path instead.
    fn fast_draw(&mut self, f: &crate::board::Frame) -> bool {
        let m = &self.m;
        let (w, h) = (self.ph.w, self.ph.h);
        let tdiv = m.hw_time_per_div() as f64;
        let x_per_sample = f.dt / tdiv * w as f64 / HDIV as f64;
        if x_per_sample > 0.5 || f.envelope || f.roll || f.scan {
            let fr = std::sync::Arc::new(f.clone());
            let window = tdiv * HDIV as f64;
            let rec = crate::board::to_record(&fr, m, 3600, window, m.delay as f64 - window / 2.0);
            for c in 0..NCH {
                if self.m.ch[c].on && !rec.ch[c].is_empty() {
                    self.ph.draw_ex(c, &self.m, (self.m.time_per_div(), self.m.delay), rec.t0, rec.dt, &rec.ch[c], 24, None);
                }
            }
            return rec.triggered;
        }
        let src = &f.ch[m.trig_src];
        // an LA line as the source: the hardware trigger point stands, the analog samples cannot refine it
        let on_la = m.trig_on_la();
        // the edge of the set direction nearest the hardware trigger point, up to half a screen away, on the signal smoothed
        // to one screen column: at decimated rates the hardware point lands near the level on either edge (trigger.md 8)
        let hyst = m.trig_hyst_div() * m.ch[m.trig_src].volts_per_div();
        let (rising, either) = (m.slope != Slope::Fall, m.slope == Slope::Either);
        let half = (tdiv * HDIV as f64 / 2.0 / f.dt) as usize;
        let col = (1.0 / x_per_sample).floor().clamp(1.0, 256.0) as usize;
        let edge = if on_la {
            None
        } else {
            let (e, any) = crate::fastdraw::edge_smooth(src, f.pre, m.trig_level, hyst, rising, either, half, col, |_| true);
            let k = match (e, any) {
                (None, _) => 3,
                (Some(t), _) if (t - f.pre as f64).abs() <= (col + 16) as f64 => 0,
                (Some(_), Some(n)) if !either && n.rising != rising => 1,
                (Some(t), _) => {
                    let d = (t - f.pre as f64).abs();
                    self.fast_off = (self.fast_off.0 + d, self.fast_off.1.max(d));
                    2
                }
            };
            self.fast_edges[k] += 1;
            e
        };
        let m = &self.m;
        let ti = edge.unwrap_or(f.pre as f64);
        let x0 = w as f64 / 2.0 - m.delay as f64 / tdiv * w as f64 / HDIV as f64;
        let ppd_y = h as f32 / VDIV;
        for c in 0..NCH {
            let ch = &m.ch[c];
            if !ch.on || f.ch[c].is_empty() {
                continue;
            }
            let vpd = ch.volts_per_div();
            let sign = -1.0;
            let xm = crate::fastdraw::Map { ti, x_per_sample, x0, y_per_volt: sign * ppd_y / vpd, y0: h as f32 / 2.0 - ch.offset / vpd * ppd_y };
            if m.dots {
                crate::fastdraw::deposit_dots(&mut self.ph.map[c], w, h, &f.ch[c], &xm, 24);
            } else {
                crate::fastdraw::deposit(&mut self.ph.map[c], w, h, &f.ch[c], &xm, 24);
            }
        }
        edge.is_some() || on_la
    }

    // ------------------------------------------------------------------ rendering

    pub fn render(&mut self, cv: &mut Canvas) {
        self.relayout();
        if self.start_up_active() {
            self.draw_start_up(cv);
            // the power dialog over it (the power key works during the start-up)
            self.draw_overlay(cv);
            return;
        }
        cv.no_clip();
        cv.clear(BG);
        let g = self.grid;
        if self.m.zoom && !self.xy {
            let (o, z) = Self::split_zoom(g);
            self.draw_grid(cv, o);
            self.draw_traces(cv, o, &self.ph, &self.vmax, self.vmax_cg);
            // zoom window in the overview: dim the rest, frame the window
            let (tz, dz) = self.view();
            let ppx = o.w as f32 / HDIV;
            let tdiv = self.m.time_per_div();
            let xa = o.x + (o.w as f32 / 2.0 + ((dz - tz * HDIV / 2.0) - self.m.delay) / tdiv * ppx) as i32;
            let xb = o.x + (o.w as f32 / 2.0 + ((dz + tz * HDIV / 2.0) - self.m.delay) / tdiv * ppx) as i32;
            cv.set_clip(o);
            cv.blend(Rect::new(o.x, o.y, (xa - o.x).max(0), o.h), BG, 170);
            cv.blend(Rect::new(xb, o.y, (o.x + o.w - xb).max(0), o.h), BG, 170);
            cv.rect(Rect::new(xa, o.y, (xb - xa).max(2), o.h), ACCENT);
            cv.no_clip();
            let f = &self.fonts;
            let lbl = format!("Zoom \u{b7} {}/div \u{b7} delay {}", si_short(tz as f64, "s"), si(dz as f64, "s"));
            cv.text(&f.sb, o.x, o.y + o.h + 17, &lbl, ACCENT);
            self.draw_refs_at(cv, o, (self.m.time_per_div(), self.m.delay));
            self.draw_grid(cv, z);
            self.draw_traces(cv, z, &self.phz, &self.vmaxz, self.vmaxz_cg);
        } else {
            for &w in &self.wins {
                self.draw_grid(cv, w);
            }
        }
        if self.view_on() {
            if self.xy {
                self.draw_xy(cv);
            } else {
                self.draw_analyzer(cv);
            }
            self.draw_view_btn(cv);
        } else {
            // window by window: each draws its own channels; what has no window of its own (references, decoders,
            // LA) goes with the first. Zoom has one window, its traces are drawn above.
            for i in 0..self.wins.len() {
                self.cur_win = i;
                if !self.m.zoom {
                    self.area = self.wins[i];
                    self.draw_mask(cv);
                    self.draw_traces(cv, self.area, &self.ph, &self.vmax, self.vmax_cg);
                }
                self.draw_hist(cv);
                self.draw_zones(cv);
                if i == 0 {
                    self.draw_refs(cv);
                }
                self.draw_math(cv);
                if i == self.la_win_of() {
                    self.draw_la(cv);
                }
                self.draw_decode(cv);
                self.draw_meas_labels(cv);
                self.draw_cursors(cv);
                self.draw_search(cv);
                self.draw_markers(cv);
                self.draw_axes(cv);
                self.draw_mask_pill(cv);
            }
            self.cur_win = 0;
            self.area = self.wave_area();
        }
        self.draw_cal_banner(cv);
        self.draw_info(cv);
        self.draw_full_btn(cv);
        self.draw_auto_bar(cv);
        self.draw_strip(cv);
        if self.pane_h() > 0 {
            self.draw_pane(cv);
            self.draw_pane_close(cv);
        }
        self.draw_results(cv);
        self.draw_table(cv);
        // HDMI workspace: the waveform area as it is now, before the bars and the menus go over it
        if self.hdmi_snap_want {
            let (y0, y1) = (TOP.h, BOTTOM.y);
            self.hdmi_snap.clear();
            self.hdmi_snap.extend_from_slice(&cv.px[(y0 * W) as usize..(y1 * W) as usize]);
        }
        if !self.full_on() {
            self.draw_top(cv);
            self.draw_bottom(cv);
        }
        self.draw_menu(cv);
        self.draw_toast(cv);
        self.draw_overlay(cv);
        self.frames += 1;
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

    /// Axis labels inside the plot: volts of the active channel along the left edge (not on the top and bottom
    /// lines, where the status line and the time labels are), times along the bottom edge. A volt label is left out
    /// rather than drawn over another label, the status line, the time labels, the mask pill or the decode band; it
    /// keeps clear of the channel markers at the left edge.
    fn draw_axes(&self, cv: &mut Canvas) {
        let f = &self.fonts;
        let a = self.area;
        let c = if self.m.ch[self.m.active].on && self.shows(self.m.active) { Some(self.m.active) } else { (0..NCH).find(|&i| self.m.ch[i].on && self.shows(i)) };
        if let Some(c) = c {
            let ch = &self.m.ch[c];
            let vpd = ch.volts_per_div();
            let span = (VDIV / 2.0 * vpd).abs() + ch.offset.abs();
            let mut taken: Vec<Rect> = vec![Rect::new(a.x, a.y, a.w, 20), Rect::new(a.x, a.y + a.h - 22, a.w, 22)];
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
                    let y = self.y_of_volts(k, 0.0).clamp(a.y + 9, a.y + a.h - 9);
                    taken.push(Rect::new(a.x, y - 9, MARK_W + 2, 18));
                }
            }
            // labels right at the left edge of the graticule, centred on their division line
            let x = a.x + 6;
            for j in 1..VDIV as i32 {
                let y = a.y + (j as f32 * a.h as f32 / VDIV).round() as i32;
                let v = (VDIV / 2.0 - j as f32) * vpd - ch.offset;
                let s = axis_label(v, vpd, ch.offset, span, ch.unit.name());
                let r = Rect::new(x - 3, y + 4 - f.s.size, f.s.width(&s) + 6, f.s.size + 4);
                if taken.iter().any(|t| overlaps(*t, r)) {
                    continue;
                }
                taken.push(r);
                backed_text(cv, &f.s, x, y + 4, &s, scale(CH_COLORS[c], 210));
            }
        }
        let (tdiv, delay) = self.view();
        let span = (HDIV / 2.0 * tdiv).abs() + delay.abs();
        // a narrow window (side by side) labels every second division, the centre always
        let every = if a.w < 700 { 2 } else { 1 };
        for i in 0..=HDIV as i32 {
            if (i - HDIV as i32 / 2).rem_euclid(every) != 0 {
                continue;
            }
            let x = a.x + (i as f32 * a.w as f32 / HDIV).round() as i32;
            let t = delay + (i as f32 - HDIV / 2.0) * tdiv;
            let zero = t.abs() < tdiv * 1e-3;
            let s = if zero { "0 s".to_string() } else { axis_label(t, tdiv, delay, span, "s") };
            let w = f.s.width(&s);
            let tx = (x - w / 2).clamp(a.x + 6, a.x + a.w - 6 - w);
            backed_text(cv, &f.s, tx, a.y + a.h - 6, &s, if zero { ACCENT } else { AXIS_TEXT });
        }
    }

    fn draw_traces(&self, cv: &mut Canvas, area: Rect, ph: &Phosphor, vmaxs: &[u32; NCH], vmax_cg: u32) {
        if self.m.color_grade {
            return self.draw_graded(cv, area, ph, vmax_cg);
        }
        // brightness: floor + sqrt(hits / brightest), the intensity setting raises floor and gain
        let inten = self.m.intensity as f32 / 100.0;
        let floor = 30.0 + 140.0 * inten;
        let gain = 0.35 + 1.3 * inten;
        let (w, h) = (ph.w, ph.h);
        for c in (0..NCH).rev() {
            if !self.m.ch[c].on || !self.shows(c) {
                continue;
            }
            let col = CH_COLORS[c];
            let (cr, cg, cb) = ((col >> 16) & 255, (col >> 8) & 255, col & 255);
            let vmax = vmaxs[c].max(1);
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
            let map = &ph.map[c];
            for y in 0..h {
                let row = &map[y * w..(y + 1) * w];
                let o = ((area.y + y as i32) * cv.w + area.x) as usize;
                for (x, &v) in row.iter().enumerate() {
                    if v == 0 {
                        continue;
                    }
                    let a = lut[((v as usize) >> shift).min(1023)].max(floor as u32 / 2);
                    let p = &mut cv.px[o + x];
                    // additive with saturation, but channels on top of each other mix towards the newer one
                    let d = *p;
                    let r = (((d >> 16) & 255) * (255 - a) / 255 + cr * a / 255).min(255);
                    let g = (((d >> 8) & 255) * (255 - a) / 255 + cg * a / 255).min(255);
                    let b = ((d & 255) * (255 - a) / 255 + cb * a / 255).min(255);
                    *p = rgb(r, g, b);
                }
            }
        }
    }

    /// Colour-graded persistence: the hit maps of all enabled channels are added up and every pixel is coloured by its
    /// density relative to the densest pixel (blue = rare .. red = frequent). Per frame only a 1024-entry LUT is
    /// built; per pixel it is an add per channel and one table look-up.
    fn draw_graded(&self, cv: &mut Canvas, area: Rect, ph: &Phosphor, vmax: u32) {
        let maps: Vec<&[u16]> = (0..NCH).filter(|&c| self.m.ch[c].on && self.shows(c)).map(|c| ph.map[c].as_slice()).collect();
        if maps.is_empty() {
            return;
        }
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
        let w = ph.w;
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
            if self.m.mask_stop && self.m.run == RunState::Run {
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
        self.mask_pass = 0;
        self.mask_fail = 0;
        self.toast(&format!("Mask created from CH{}", c + 1));
    }

    /// Creates the mask from the selected reference slot.
    fn create_mask_from_ref(&mut self) {
        let Some(r) = self.refs[self.ref_sel].as_ref().filter(|r| r.v.len() >= 2) else {
            return self.toast(&format!("R{} is empty", self.ref_sel + 1));
        };
        self.mask_ref = Some(r.v.clone());
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
        let f = &self.fonts;
        let c = self.m.mask_src;
        let parts: [(String, Rgb); 6] = [
            ("Mask ".into(), TEXT),
            (format!("CH{}", c + 1), CH_COLORS[c]),
            ("  \u{b7}  Passed ".into(), DIM),
            (fmt_count(self.mask_pass as f64), GREEN),
            ("  \u{b7}  Failed ".into(), DIM),
            (fmt_count(self.mask_fail as f64), if self.mask_fail > 0 { RED } else { TEXT }),
        ];
        let w: i32 = parts.iter().map(|(s, _)| f.sb.width(s)).sum::<i32>() + 24;
        let a = self.area;
        let r = Rect::new(a.x + 8, a.y + 24, w, 24);
        let col = if self.mask_fail > 0 { RED } else { GREEN };
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
        if let WaveId::Ch(c) = self.meas_src {
            if c < n_ch() && self.m.ch[c].on {
                return Some(c);
            }
        }
        (0..NCH).find(|&c| self.m.ch[c].on)
    }

    /// Histogram bins from the phosphor map: rows (vertical) or columns within +-0.5 div of the trigger level
    /// (horizontal); with the mean and standard deviation in pixels.
    fn hist_bins(&self) -> Option<(Vec<u64>, f64, f64)> {
        let c = self.hist_src()?;
        let (w, h) = (self.ph.w, self.ph.h);
        let map = &self.ph.map[c];
        let bins: Vec<u64> = match self.m.hist {
            Hist::Off => return None,
            Hist::Vertical => (0..h).map(|r| map[r * w..(r + 1) * w].iter().map(|&x| x as u64).sum()).collect(),
            Hist::Horizontal => {
                let ch = &self.m.ch[c];
                let ppd = h as f32 / VDIV;
                let yl = h as f32 / 2.0 - (self.m.trig_level + ch.offset) / ch.volts_per_div() * ppd;
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

    /// Histogram overlay: bars along the left edge (vertical) or the bottom (horizontal), mean and sigma in units.
    fn draw_hist(&self, cv: &mut Canvas) {
        let Some((bins, mean, sd)) = self.hist_bins() else { return };
        let Some(c) = self.hist_src().filter(|&c| self.shows(c)) else { return };
        let a = self.area;
        let col = CH_COLORS[c];
        let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f64;
        let depth = (a.w / 5).max(40) as f64;
        cv.set_clip(a);
        let ch = &self.m.ch[c];
        let txt = match self.m.hist {
            Hist::Vertical => {
                for (r, &b) in bins.iter().enumerate() {
                    let len = (b as f64 / peak * depth).round() as i32;
                    if len > 0 {
                        cv.blend(Rect::new(a.x, a.y + r as i32, len, 1), col, 150);
                    }
                }
                // pixel row -> volts
                let vpp = ch.volts_per_div() as f64 / (a.h as f64 / VDIV as f64);
                let mv = (a.h as f64 / 2.0 - mean) * vpp - ch.offset as f64;
                format!("Hist CH{}  mean {}  \u{3c3} {}", c + 1, si(mv, "V"), si(sd * vpp, "V"))
            }
            _ => {
                for (x, &b) in bins.iter().enumerate() {
                    let len = (b as f64 / peak * depth * 0.6).round() as i32;
                    if len > 0 {
                        cv.blend(Rect::new(a.x + x as i32, a.y + a.h - len, 1, len), col, 150);
                    }
                }
                let (tdiv, delay) = self.view();
                let spp = tdiv as f64 / (a.w as f64 / HDIV as f64);
                let mt = (mean - a.w as f64 / 2.0) * spp + delay as f64;
                format!("Hist CH{} @ level  mean {}  \u{3c3} {}", c + 1, si(mt, "s"), si(sd * spp, "s"))
            }
        };
        cv.no_clip();
        let f = &self.fonts;
        // above the decoder band and the time labels
        let r = Rect::new(a.x + 8, a.y + a.h - 76, f.s.width(&txt) + 16, 20);
        cv.round(r, 4, PANEL_BG, 230);
        cv.text_in(&f.s, r, 1, &txt, col);
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

    /// A math source of the last record: CH1..4, or math channel j < `below` (math on math without cycles).
    fn math_source(&self, s: usize, below: usize) -> Vec<f32> {
        let Some(rec) = &self.last[0] else { return Vec::new() };
        if s < NCH {
            rec.ch[s].clone()
        } else if s - NCH < below {
            self.math_wave_of(s - NCH)
        } else {
            Vec::new()
        }
    }

    /// The waveform of math channel `k` (empty for FFT/Off or missing sources).
    fn math_wave_of(&self, k: usize) -> Vec<f32> {
        let Some(rec) = &self.last[0] else { return Vec::new() };
        let c = self.m.math_ch(k);
        if matches!(c.op, MathOp::Off | MathOp::Fft) {
            return Vec::new();
        }
        if c.op == MathOp::Expr {
            let Ok(e) = oos_dsp::expr::Expr::parse(&self.m.math_expr[k]) else { return Vec::new() };
            let n = rec.ch.iter().map(|c| c.len()).max().unwrap_or(0);
            return e.eval(std::array::from_fn(|c| &rec.ch[c][..]), rec.t0, rec.dt, n);
        }
        let a = self.math_source(c.a, k);
        let b = if c.op.two_sources() { self.math_source(c.b, k) } else { Vec::new() };
        if a.is_empty() || (c.op.two_sources() && b.is_empty()) {
            return Vec::new();
        }
        let w = compute(c.op, &a, &b, rec.dt, c.fc as f64, c.bw as f64);
        if c.op == MathOp::Lin {
            return w.iter().map(|v| c.lin.0 * v + c.lin.1).collect();
        }
        w
    }

    /// The waveform of the selected math channel.
    fn math_wave(&self) -> Vec<f32> {
        self.math_wave_of(self.m.math_sel)
    }

    /// Picks the math scale so that the waveform spans about 6 divisions.
    fn math_autoscale(&mut self) {
        if self.m.math == MathOp::Fft {
            self.m.math_scale = 3;
            self.m.math_offset = 0.0;
            return;
        }
        let w = self.math_wave();
        let (mn, mx) = w.iter().filter(|x| x.is_finite()).fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        let sc = math_scales();
        if mx < mn {
            self.m.math_scale = sc.iter().position(|&x| x >= 1.0).unwrap_or(0);
            return;
        }
        let want = ((mx - mn) as f64 / 6.0).max(1e-9);
        self.m.math_scale = sc.iter().position(|&x| x >= want).unwrap_or(sc.len() - 1);
        self.m.math_offset = -((mx + mn) as f64 / 2.0 / sc[self.m.math_scale]) as f32;
    }

    fn draw_math(&self, cv: &mut Canvas) {
        // all traces first, then the labels on top of them
        let on: Vec<usize> = (0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off && self.math_win_of(k) == self.cur_win).collect();
        let labels: Vec<Option<String>> = on.iter().enumerate().map(|(row, &k)| self.draw_math_ch(cv, k, row as i32)).collect();
        let f = &self.fonts;
        for (row, (label, &k)) in labels.iter().zip(&on).enumerate() {
            if let Some(label) = label {
                // top right, under the status line, one row per math channel (the results panel goes below them)
                let r = Rect::new(self.area.x + self.area.w - 12 - f.s.width(label) - 16, self.area.y + 22 + 26 * row as i32, f.s.width(label) + 16, 20);
                cv.round(r, 4, rgb(0x1c, 0x16, 0x28), 230);
                cv.text_in(&f.s, r, 1, label, MATH_COLORS[k % MATH_COLORS.len()]);
            }
        }
    }

    /// Draws the trace of math channel `k`; returns its label.
    fn draw_math_ch(&self, cv: &mut Canvas, k: usize, row: i32) -> Option<String> {
        let c = self.m.math_ch(k);
        let Some(rec) = &self.last[0] else { return None };
        let col = MATH_COLORS[k % MATH_COLORS.len()];
        let f = &self.fonts;
        cv.set_clip(self.area);
        let ppy = self.area.h as f32 / VDIV;
        let cy = self.area.y as f32 + self.area.h as f32 / 2.0;
        let label;
        if c.op == MathOp::Fft {
            let a = self.math_source(c.a, k);
            let (df, sp) = if a.is_empty() { (0.0, Vec::new()) } else { oos_dsp::math::spectrum(&a, rec.dt, c.win) };
            let db: Vec<f32> = sp.iter().map(|&x| oos_dsp::math::dbv(x) as f32).collect();
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
                let y = (self.area.y as f32 + (top - v) / dbdiv * ppy).round() as i32;
                let x = self.area.x + px;
                if let Some((x0, y0)) = prev {
                    cv.line(x0, y0, x, y, col);
                }
                prev = Some((x, y));
            }
            label = format!("M{} FFT {} {}  {} dB/div  {}/div", k + 1, src_name(c.a), c.win.name(), dbdiv, si_short(span / HDIV as f64, "Hz"));
        } else {
            let w = self.math_wave_of(k);
            if w.is_empty() {
                cv.no_clip();
                cv.text(&f.s, self.area.x + 60, self.area.y + 40 + 18 * row, &format!("M{}: source off", k + 1), col);
                return None;
            }
            let sc = math_scales();
            let vpd = sc[c.scale.min(sc.len() - 1)] as f32;
            let tdiv = self.view().0 as f64;
            let ppx = self.area.w as f64 / HDIV as f64;
            let mut prev: Option<(i32, i32)> = None;
            let step = (w.len() / (self.area.w as usize * 2)).max(1);
            for i in (0..w.len()).step_by(step) {
                let v = w[i];
                if !v.is_finite() {
                    prev = None;
                    continue;
                }
                let t = rec.t0 + i as f64 * rec.dt - self.view().1 as f64;
                let x = (self.area.x as f64 + self.area.w as f64 / 2.0 + t / tdiv * ppx).round() as i32;
                let y = (cy - (v / vpd + c.offset) * ppy).clamp(-1e4, 1e4).round() as i32;
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
        let ppd = a.h as f32 / VDIV;
        (a.y as f32 + a.h as f32 / 2.0 - (v + ch.offset) / ch.volts_per_div() * ppd).round() as i32
    }

    fn y_of_wave(&self, id: WaveId, v: f32) -> i32 {
        match id {
            WaveId::Ch(c) => self.y_of_volts(c, v),
            WaveId::Ref(i) => {
                let a = self.area;
                let Some(r) = self.refs.get(i).and_then(|x| x.as_ref()) else {
                    return a.y;
                };
                let ppd = a.h as f32 / VDIV;
                (a.y as f32 + a.h as f32 / 2.0 - (v + r.offset) / r.vpd.max(1e-12) * ppd).round() as i32
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
        self.decode_packets_of(&self.m)
    }

    /// The same for the decoder that m is the view of; cached per record and settings, up to four decoders run every frame.
    fn decode_packets_of(&self, m: &Model) -> Result<Vec<oos_dsp::decode::Packet>, &'static str> {
        use std::hash::{Hash, Hasher};
        let Some(rec) = &self.last[0] else { return Ok(Vec::new()) };
        if m.dec_on_la() {
            return self.la_packets(m, rec);
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.wfm_total, self.hist_pos, rec.t0.to_bits(), rec.dt.to_bits()).hash(&mut h);
        for c in 0..NCH {
            (rec.ch[c].as_ptr() as usize, rec.ch[c].len(), m.ch[c].volts_per_div().to_bits()).hash(&mut h);
        }
        format!("{:?}", m.dec_cfg()).hash(&mut h);
        let key = h.finish();
        if let Some(e) = self.dec_cache.borrow().iter().find(|e| e.0 == key) {
            return Ok(e.1.clone());
        }
        let mut p = crate::trig::decode(m, std::array::from_fn(|c| &rec.ch[c][..]), rec.dt)?;
        for x in p.iter_mut() {
            x.t0 = rec.t0 + x.t0 * rec.dt;
            x.t1 = rec.t0 + x.t1 * rec.dt;
        }
        self.dec_cache_put(key, &p);
        Ok(p)
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
        if self.last[0].is_none() {
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
        let f = &self.fonts;
        let a = self.area;
        let col = if m.dec_on_la() { la::line_color(m.dec_la[0].unwrap_or(0)) } else { CH_COLORS[m.dec_src] };
        // rows under the lanes span all windows, like the lanes
        let a = if row.is_some() { self.lane_area() } else { a };
        let h = 20;
        cv.set_clip(row.unwrap_or(a));
        if row.is_none() {
            cv.blend(Rect::new(a.x, y - 3, a.w, h + 6), BG, 200);
        }
        let name = format!("{} {}", k + 1, m.dec_proto.name());
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
        for p in &packets {
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
        if !m.dec_on_la() {
            let c = m.dec_src;
            let ty = self.y_of_volts(c, m.dec_thr);
            cv.dashed_hline(a.x + 26, a.x + a.w - 26, ty, 2, 6, scale(col, 160));
            if m.dec_proto == Proto::Arinc429 {
                // the LO level
                let ty = self.y_of_volts(c, -m.dec_thr);
                cv.dashed_hline(a.x + 26, a.x + a.w - 26, ty, 2, 6, scale(col, 160));
            }
        }
        cv.no_clip();
    }

    fn draw_refs(&self, cv: &mut Canvas) {
        self.draw_refs_at(cv, self.area, self.view());
    }

    fn draw_refs_at(&self, cv: &mut Canvas, area: Rect, view: (f32, f32)) {
        let f = &self.fonts;
        cv.set_clip(area);
        let ppy = area.h as f32 / VDIV;
        let cy = area.y as f32 + area.h as f32 / 2.0;
        let tdiv = view.0 as f64;
        let delay = view.1 as f64;
        let ppx = area.w as f64 / HDIV as f64;
        let xmid = area.x as f64 + area.w as f64 / 2.0;
        // only the samples that land on this screen (plus a little margin)
        let t_lo = delay - tdiv * HDIV as f64 * 0.6;
        let t_hi = delay + tdiv * HDIV as f64 * 0.6;
        for (i, r) in self.refs.iter().enumerate() {
            let Some(r) = r else { continue };
            if !r.on || r.dt <= 0.0 || r.v.len() < 2 {
                continue;
            }
            let col = REF_COLORS[r.color.min(REF_COLORS.len() - 1)].1;
            let last = r.v.len() - 1;
            let k0 = ((t_lo - r.t0) / r.dt).floor().max(0.0) as usize;
            let k1 = ((t_hi - r.t0) / r.dt).ceil().max(0.0) as usize;
            let k0 = k0.min(last);
            let k1 = k1.min(last);
            if k1 <= k0 {
                continue;
            }
            let step = ((k1 - k0) / (area.w as usize * 2)).max(1);
            let mut prev: Option<(i32, i32)> = None;
            let mut first = None;
            for k in (k0..=k1).step_by(step) {
                let v = r.v[k];
                if !v.is_finite() {
                    prev = None;
                    continue;
                }
                let t = r.t0 + k as f64 * r.dt - delay;
                let x = (xmid + t / tdiv * ppx).round() as i32;
                let y = (cy - (v + r.offset) / r.vpd * ppy).round() as i32;
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
                let yy = y.clamp(area.y + 10, area.y + area.h - 10);
                // right of the volt labels
                let name = if r.label.is_empty() { format!("R{}", i + 1) } else { r.label.clone() };
                let b = Rect::new(area.x + 80, yy - 8, (f.s.width(&name) + 8).max(24), 17);
                cv.round(b, 3, col, 255);
                cv.text_in(&f.s, b, 1, &name, rgb(0, 0, 0));
            }
        }
        cv.no_clip();
    }

    fn draw_xy(&self, cv: &mut Canvas) {
        // CH1 = X, CH2 = Y in a square of 8 x 8 div in the middle of the grid
        let Some(rec) = &self.last[0] else { return };
        let (a, b) = (&rec.ch[0], &rec.ch[1]);
        let f = &self.fonts;
        let side = self.area.h;
        let sq = Rect::new(self.area.x + (self.area.w - side) / 2, self.area.y, side, side);
        cv.fill(sq, rgb(0x04, 0x04, 0x08));
        cv.rect(sq, GRID_AXIS);
        cv.vline(sq.x + side / 2, sq.y, sq.y + side - 1, GRID_LINE);
        cv.hline(sq.x, sq.x + side - 1, sq.y + side / 2, GRID_LINE);
        if a.is_empty() || b.is_empty() {
            cv.text_in(&f.m, sq, 1, "XY needs CH1 and CH2", DIM);
            return;
        }
        cv.set_clip(sq);
        let ppd = side as f32 / 8.0;
        let (ca, cb) = (&self.m.ch[0], &self.m.ch[1]);
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
        cv.text(&f.s, sq.x + 6, sq.y + 16, "XY  X = CH1  Y = CH2", col);
    }

    fn draw_markers(&self, cv: &mut Canvas) {
        let f = &self.fonts;
        let a = self.area;
        // channel ground markers at the left edge of the grid
        for c in 0..NCH {
            if !self.m.ch[c].on || !self.shows(c) {
                continue;
            }
            let y = self.y_of_volts(c, 0.0).clamp(a.y + 9, a.y + a.h - 9);
            let col = CH_COLORS[c];
            let r = Rect::new(a.x + 2, y - 8, 16, 16);
            cv.round(r, 3, col, 255);
            cv.triangle([(a.x + 17, y - 6), (a.x + 24, y), (a.x + 17, y + 6)], col);
            cv.text_in(&f.sb, r, 1, &format!("{}", c + 1), DARK_TEXT);
        }
        // trigger level marker at the right edge of the grid, its value next to it
        let ts = self.m.trig_src;
        let lvl = self.trig_level_shown();
        if self.m.ch[ts].on && self.shows(ts) && self.m.trig_line().is_none() {
            let y = self.y_of_volts(ts, lvl).clamp(a.y + 9, a.y + a.h - 9);
            let x1 = a.x + a.w - MARK_W;
            let r = Rect::new(x1 + 8, y - 8, 16, 16);
            cv.round(r, 3, ACCENT, 255);
            cv.triangle([(x1 + 9, y - 6), (x1 + 2, y), (x1 + 9, y + 6)], ACCENT);
            cv.text_in(&f.sb, r, 1, "T", DARK_TEXT);
            let s = si_short(lvl as f64, self.m.unit(ts));
            backed_text(cv, &f.sb, x1 - 4 - f.sb.width(&s), y + 5, &s, ACCENT);
            if self.touch.drag == Drag::Level {
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
            if self.m.ch[c].on && self.shows(c) {
                let y = self.y_of_volts(c, self.m.trig_level2).clamp(a.y + 9, a.y + a.h - 9);
                let x1 = a.x + a.w - MARK_W;
                let r = Rect::new(x1 + 8, y - 8, 16, 16);
                cv.round(r, 3, mix(BG, ACCENT, 170), 255);
                cv.triangle([(x1 + 9, y - 6), (x1 + 2, y), (x1 + 9, y + 6)], mix(BG, ACCENT, 170));
                cv.text_in(&f.sb, r, 1, tag, DARK_TEXT);
                let s = si_short(self.m.trig_level2 as f64, self.m.unit(ts));
                backed_text(cv, &f.sb, x1 - 4 - f.sb.width(&s), y + 5, &s, mix(BG, ACCENT, 200));
            }
        }
        // trigger position at the top edge of the grid
        let tx = self.x_of_time(0.0);
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
            cv.round(r, 5, LINE, 255);
            cv.round(r.inset(1), 4, BAR, 255);
            cv.text_in(&f.sb, r, 1, s, col);
        };
        match self.touch.drag {
            Drag::Offset(c) if self.shows(c) => {
                let y = self.y_of_volts(c, 0.0);
                cv.dashed_hline(a.x, a.x + a.w - 1, y, 6, 4, CH_COLORS[c]);
                tag(cv, a.x + 8, y - 30, &format!("Offset {}", si(self.m.ch[c].offset as f64, self.m.unit(c))), CH_COLORS[c]);
            }
            Drag::Delay if self.cur_win == self.touch.win.min(self.wins.len().max(1) - 1) => tag(cv, a.x + a.w / 2 - 60, a.y + 12, &format!("Delay {}", si(self.view().1 as f64, "s")), ACCENT),
            Drag::Level if self.shows(ts) => {
                let y = self.y_of_volts(ts, self.m.trig_level);
                tag(cv, a.x + a.w - 130, y - 30, &format!("Level {}", si(self.m.trig_level as f64, self.m.unit(ts))), ACCENT);
            }
            _ => {}
        }
    }

    fn draw_cursors(&self, cv: &mut Canvas) {
        // in the window of the channel they read
        if !self.shows(self.m.active) {
            return;
        }
        let f = &self.fonts;
        let a = self.area;
        let tagbox = |cv: &mut Canvas, x: i32, y: i32, s: &str| {
            let r = Rect::new(x, y, f.s.width(s) + 8, 14);
            cv.round(r, 2, CURSOR, 255);
            cv.text_in(&f.s, r, 1, s, DARK_TEXT);
        };
        match self.m.cursor {
            CursorMode::Off => {}
            CursorMode::Time | CursorMode::Track | CursorMode::Xy => {
                if self.m.cursor == CursorMode::Xy {
                    for (frac, name) in [(self.m.cur_c, "Y1"), (self.m.cur_d, "Y2")] {
                        let y = a.y + (frac * a.h as f32) as i32;
                        cv.dashed_hline(a.x, a.x + a.w - 1, y, 4, 3, CURSOR);
                        tagbox(cv, a.x + a.w - 26, y - 16, name);
                    }
                }
                let (ta, tb) = self.cursor_times();
                for (frac, name, t) in [(self.m.cur_a, "X1", ta), (self.m.cur_b, "X2", tb)] {
                    let x = a.x + (frac * a.w as f32) as i32;
                    cv.dashed_vline(x, a.y, a.y + a.h - 1, 4, 3, CURSOR);
                    tagbox(cv, x + 2, a.y + a.h - 36, name);
                    // tracking: a mark where the cursor meets the trace
                    if let (CursorMode::Track, Some(v)) = (self.m.cursor, self.wave_at(t)) {
                        let y = self.y_of_volts(self.m.active, v);
                        cv.dashed_hline(x - 10, x + 10, y, 2, 2, CURSOR);
                    }
                }
            }
            CursorMode::Volt => {
                for (frac, name) in [(self.m.cur_a, "Y1"), (self.m.cur_b, "Y2")] {
                    let y = a.y + (frac * a.h as f32) as i32;
                    cv.dashed_hline(a.x, a.x + a.w - 1, y, 4, 3, CURSOR);
                    tagbox(cv, a.x + a.w - 26, y - 16, name);
                }
            }
        }
    }

    /// Cursor read-outs: (label, value) pairs.
    fn cursor_values(&self) -> Vec<(String, String)> {
        match self.m.cursor {
            CursorMode::Off => Vec::new(),
            CursorMode::Xy => {
                let (ta, tb) = self.cursor_times();
                let ch = &self.m.ch[self.m.active];
                let v = |c: f32| (0.5 - c) * VDIV * ch.volts_per_div() - ch.offset;
                let (va, vb) = (v(self.m.cur_c), v(self.m.cur_d));
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("X2".into(), si(tb, "s")),
                    ("Y1".into(), si(va as f64, ch.unit.name())),
                    ("Y2".into(), si(vb as f64, ch.unit.name())),
                    ("\u{394}X".into(), si(tb - ta, "s")),
                    ("\u{394}Y".into(), si((vb - va) as f64, ch.unit.name())),
                ]
            }
            CursorMode::Track => {
                let (ta, tb) = self.cursor_times();
                let y = |t: f64| self.wave_at(t);
                let u = self.m.unit(self.m.active);
                let txt = |v: Option<f32>| v.map_or("----".to_string(), |v| si(v as f64, u));
                let dy = y(ta).zip(y(tb)).map(|(a, b)| b - a);
                vec![
                    ("X1".into(), si(ta, "s")),
                    ("Y1".into(), txt(y(ta))),
                    ("X2".into(), si(tb, "s")),
                    ("Y2".into(), txt(y(tb))),
                    ("\u{394}X".into(), si(tb - ta, "s")),
                    ("\u{394}Y".into(), txt(dy)),
                ]
            }
            CursorMode::Time => {
                let (tpd, delay) = self.view();
                let ta = (self.m.cur_a - 0.5) * HDIV * tpd + delay;
                let tb = (self.m.cur_b - 0.5) * HDIV * tpd + delay;
                let d = tb - ta;
                vec![
                    ("X1".into(), si(ta as f64, "s")),
                    ("X2".into(), si(tb as f64, "s")),
                    ("\u{394}X".into(), si(d as f64, "s")),
                    ("1/\u{394}X".into(), si(1.0 / d.abs() as f64, "Hz")),
                ]
            }
            CursorMode::Volt => {
                let ch = &self.m.ch[self.m.active];
                let va = (0.5 - self.m.cur_a) * VDIV * ch.volts_per_div() - ch.offset;
                let vb = (0.5 - self.m.cur_b) * VDIV * ch.volts_per_div() - ch.offset;
                let u = ch.unit.name();
                vec![("Y1".into(), si(va as f64, u)), ("Y2".into(), si(vb as f64, u)), ("\u{394}Y".into(), si((vb - va) as f64, u))]
            }
        }
    }

    /// Status line inside the top of the plot: touch lock, history position and search count on the left; a source that
    /// is not the live hardware on the right.
    fn draw_info(&self, cv: &mut Canvas) {
        let f = &self.fonts;
        let g = self.grid;
        let y = g.y + 14;
        // only what makes the picture something else than a live measurement; the rates are on System info
        let src = if self.offline {
            "Offline"
        } else {
            match self.src.name() {
                "DEMO" => "Demo signals",
                "REPLAY" => "Replay",
                _ => "",
            }
        };
        if !src.is_empty() {
            backed_text(cv, &f.s, self.full_btn().0.x - 8 - f.s.width(src), y, src, DIM);
        }
        let mut x = g.x + 8;
        if self.m.touch_lock {
            x = backed_text(cv, &f.sb, x, y, "Touch locked \u{b7} tap Menu to unlock", AMBER) + 16;
        }
        if let Some(i) = self.hist_pos {
            x = backed_text(cv, &f.sb, x, y, &format!("History {}/{}", i + 1, self.history.len()), BLUE) + 16;
        }
        if self.search_on {
            let n = self.search_events().len();
            backed_text(cv, &f.sb, x, y, &format!("Search: {} event{}", n, if n == 1 { "" } else { "s" }), AMBER);
        }
    }

    /// Search markers: an amber tick at the top of the plot for every event of the trigger condition.
    fn draw_search(&self, cv: &mut Canvas) {
        if !self.search_on {
            return;
        }
        let a = self.area;
        for t in self.search_events() {
            let x = self.x_of_time(t as f32);
            if x >= a.x && x < a.x + a.w {
                cv.triangle([(x - 4, a.y + 1), (x + 4, a.y + 1), (x, a.y + 8)], AMBER);
                cv.dashed_vline(x, a.y + 9, a.y + a.h - 1, 2, 6, mix(BG, AMBER, 90));
            }
        }
    }

    /// Measurement rows (one per channel) and the cursor row under the plot.
    fn draw_strip(&self, cv: &mut Canvas) {
        let f = &self.fonts;
        let g = self.grid;
        // the rows sit on the cards: what the plot's whole divisions leave over goes between plot and rows
        let mut y = (self.plot_bottom() + 2 - self.strip_rows() * STRIP_ROW).max(g.y + g.h + 4 + self.la_strip_h() + self.pane_h());
        let in_pane = self.results_pane_on();
        if !self.results_panel() && !in_pane {
            let rows = self.meas_rows();
            for id in self.meas_groups() {
                let mine: Vec<&tools::MeasRow> = rows.iter().filter(|r| r.src == id).collect();
                let col = self.wave_color(id);
                let label = self.wave_name(id);
                for part in mine.chunks(tools::STRIP_PER_ROW) {
                    cv.fill(Rect::new(6, y, W - 12, 16), BAR);
                    let r = Rect::new(8, y + 1, 42, 14);
                    cv.round(r, 3, col, 255);
                    cv.text_in(&f.sb, r, 1, &label, DARK_TEXT);
                    // the row's width shared out, whatever the number of values
                    let pitch = (W - 66) / part.len() as i32;
                    for (i, row) in part.iter().enumerate() {
                        let x = 58 + i as i32 * pitch;
                        let x2 = cv.text(&f.s, x, y + 12, &row.name, DIM) + 8;
                        cv.text(&f.sb, x2, y + 12, &row.text(), col);
                    }
                    y += STRIP_ROW;
                }
            }
        }
        if self.m.dvm_on || self.m.cnt_on {
            cv.fill(Rect::new(6, y, W - 12, 16), BAR);
            let mut x = 12;
            for (l, v) in self.dvm_row() {
                x = cv.text(&f.s, x, y + 12, &l, DIM) + 8;
                x = cv.text(&f.sb, x, y + 12, &v, TEXT) + 24;
            }
            y += STRIP_ROW;
        }
        if self.m.cursor != CursorMode::Off && !in_pane {
            cv.fill(Rect::new(6, y, W - 12, 16), BAR);
            let c = self.m.active;
            let pill = Rect::new(8, y + 1, 42, 14);
            cv.round(pill, 3, CH_COLORS[c], 255);
            cv.text_in(&f.sb, pill, 1, &format!("CH{}", c + 1), DARK_TEXT);
            let mode = match self.m.cursor {
                CursorMode::Volt => "Volt",
                CursorMode::Track => "Track",
                CursorMode::Xy => "XY",
                _ => "Time",
            };
            let r = Rect::new(56, y + 2, 38, 12);
            cv.round_outline(r, 3, DIM);
            cv.text_in(&f.s, r, 1, mode, LABEL);
            for (i, (l, v)) in self.cursor_values().iter().enumerate() {
                let x = 104 + i as i32 * if matches!(self.m.cursor, CursorMode::Track | CursorMode::Xy) { 150 } else { 200 };
                let x2 = cv.text(&f.s, x, y + 12, l, DIM) + 8;
                cv.text(&f.sb, x2, y + 12, v, TEXT);
            }
        }
    }

    /// Header and body of the measurement results overlay (statistics on): top right of the plot, under the status
    /// line and the math label, left of the strip where a drag moves the trigger level. Folded: header only.
    fn results_rects(&self) -> Option<(Rect, Rect)> {
        if !self.results_panel() {
            return None;
        }
        let a = self.over_area();
        let y = a.y + 22 + 26 * (0..NMATH).filter(|&k| self.m.math_ch(k).op != MathOp::Off).count() as i32;
        let x = a.x + a.w - 56 - RESULTS_W;
        let n = (self.m.meas.len() + self.m.meas2.len()) as i32;
        let head = Rect::new(x, y, RESULTS_W, 30);
        Some((head, Rect::new(x, y + 30, RESULTS_W, if self.results_min { 0 } else { n * Self::results_pitch(n) + 4 })))
    }

    /// Height of a row of the results card: two lines, or one when there are many.
    fn results_pitch(n: i32) -> i32 {
        if n > 6 { 22 } else { 42 }
    }

    /// Measurement results with statistics as a translucent card over the plot: value, mean, ±σ, acquisitions;
    /// tapping the header folds it.
    fn draw_results(&self, cv: &mut Canvas) {
        let Some((head, body)) = self.results_rects() else { return };
        let f = &self.fonts;
        let all = Rect::new(head.x, head.y, head.w, head.h + body.h);
        cv.round(all, 6, CARD_BORDER, 255);
        cv.round(all.inset(1), 5, BAR, 215);
        if self.results_min {
            chevron(cv, head.x + 13, head.y + 15, 1, DIM);
        } else {
            chevron_down(cv, head.x + 9, head.y + 12, DIM);
        }
        cv.text(&f.sb, head.x + 24, head.y + 19, "Measurements", TEXT);
        let n = self.mstat.first().map(|s| s.n).unwrap_or(0);
        let ns = format!("n {}", fmt_count(n as f64));
        cv.text(&f.s, head.x + head.w - 10 - f.s.width(&ns), head.y + 19, &ns, DIM);
        if self.results_min {
            return;
        }
        let rows = self.meas_rows();
        let pitch = Self::results_pitch(rows.len() as i32);
        for (i, r) in rows.iter().enumerate() {
            let by = body.y + i as i32 * pitch;
            cv.hline(body.x + 8, body.x + body.w - 9, by, LINE);
            let st = self.mstat.get(i).copied().unwrap_or_default();
            let mean = st.sum / st.n.max(1) as f64;
            if pitch < 42 {
                // one line: name, value, mean
                cv.text(&f.s, body.x + 10, by + 16, &r.name, LABEL);
                let s = r.text();
                cv.text(&f.sb, body.x + body.w - 10 - f.sb.width(&s), by + 16, &s, self.wave_color(r.src));
                continue;
            }
            cv.text(&f.s, body.x + 10, by + 16, &format!("{} {}", r.name, self.wave_name(r.src)), LABEL);
            if st.n > 1 {
                let sd = (st.sumsq / st.n as f64 - mean * mean).max(0.0).sqrt();
                let s = format!("\u{b1}{} \u{3c3}", si(sd, r.unit));
                cv.text(&f.s, body.x + body.w - 10 - f.s.width(&s), by + 16, &s, DIM);
                let s = format!("mean {}", r.fmt(mean));
                cv.text(&f.s, body.x + body.w - 10 - f.s.width(&s), by + 35, &s, DIM);
            }
            cv.text(&f.mb, body.x + 10, by + 35, &r.text(), self.wave_color(r.src));
        }
    }

    const RUN_PILL: Rect = Rect::new(8, 6, 92, 28);
    const NET_BOX: Rect = Rect::new(896, 5, 122, 30);
    /// Left of the network box while a WLAN stick is plugged in.
    const WLAN_BOX: Rect = Rect::new(786, 5, 104, 30);
    /// Start menu button left of the channel cards (all menus and key functions by touch).
    const MENU_BTN: Rect = Rect::new(6, 530, 62, 66);

    /// Top-bar buttons, right-aligned before the network box: what is used most, and the menus without a front-panel key
    /// (Display, Storage, System). Everything else is in the start menu.
    fn top_buttons(&self) -> Vec<(TopBtn, &'static str, Rect)> {
        let f = &self.fonts;
        let all: [(TopBtn, &'static str); 5] = [
            (TopBtn::Menu(MenuId::Measure), "Measure"),
            (TopBtn::Menu(MenuId::Cursor), "Cursor"),
            (TopBtn::Menu(MenuId::Display), "Display"),
            (TopBtn::Menu(MenuId::Storage), "Storage"),
            (TopBtn::System, "System"),
        ];
        let mut x = self.top_right() - 8;
        let mut out = Vec::new();
        for &(b, l) in all.iter().rev() {
            let w = f.mb.width(l) + 28;
            x -= w;
            out.push((b, l, Rect::new(x, 5, w, 30)));
            x -= 6;
        }
        out.reverse();
        out
    }

    /// While a USB stick is mounted: left of the network and WLAN boxes; the top-bar buttons move over for it.
    fn usb_box(&self) -> Rect {
        let next = if self.wlan_stick { Self::WLAN_BOX.x } else { Self::NET_BOX.x };
        Rect::new(next - 52, 5, 46, 30)
    }

    /// Where the buttons of the top bar end: the leftmost of the boxes on the right.
    fn top_right(&self) -> i32 {
        match (self.usb, self.wlan_stick) {
            (true, _) => self.usb_box().x,
            (false, true) => Self::WLAN_BOX.x,
            _ => Self::NET_BOX.x,
        }
    }

    /// A box of the top bar with a state dot and two lines.
    fn draw_link(&self, cv: &mut Canvas, r: Rect, l1: &str, addr: Option<&str>) {
        let f = &self.fonts;
        cv.round(r, 5, if self.menu_open && self.menu_root() == MenuId::Network { ACCENT } else { BTN_BORDER }, 255);
        cv.round(r.inset(1), 4, BG, 255);
        cv.circle(r.x + 11, r.y + 10, 3, if addr.is_some() { GREEN } else { DIM });
        cv.text(&f.s, r.x + 20, r.y + 13, l1, DIM);
        cv.text(&f.sb, r.x + 20, r.y + 26, addr.unwrap_or("not connected"), TEXT);
    }

    /// The address of an interface, if it has one.
    fn addr_of(&self, name: &str) -> Option<&str> {
        self.net_ifs.iter().find(|i| i.0 == name).map(|i| i.1.as_str())
    }

    /// Touch area of a top-bar button: the full bar height.
    fn top_hit(r: Rect) -> Rect {
        Rect::new(r.x - 2, 0, r.w + 4, TOP.h)
    }

    fn draw_top(&self, cv: &mut Canvas) {
        let f = &self.fonts;
        cv.fill(TOP, BAR);
        cv.hline(0, W - 1, TOP.h - 1, LINE);
        // run / trigger state; offline mode reads the record (playing or held) instead of a trigger
        let (txt, col) = self.run_caption();
        let r = Self::RUN_PILL;
        cv.round(r, 6, mix(BAR, col, 80), 255);
        cv.round(r.inset(1), 5, mix(BAR, col, 36), 255);
        cv.circle(r.x + 16, r.y + 14, 4, col);
        cv.text(&f.mb, r.x + 30, r.y + 19, txt, col);
        // acquisition: sample rate and depth of the hardware when it runs, else of the model; mode.
        // Offline mode shows the reference's own rate and length, with no acquire mode.
        let (caption, live_acq) = self.acq_caption();
        let buttons = self.top_buttons();
        let room = buttons.first().map_or(self.top_right(), |b| b.2.x) - 12;
        let acq_open = self.menu_open && self.menu_root() == if self.offline { MenuId::Ref } else { MenuId::Acquire };
        let tcol = if acq_open { ACCENT } else { TEXT };
        let x = cv.text(&f.mb, 114, 25, &caption, tcol);
        if live_acq {
            let acq = match self.m.acq {
                AcqMode::Normal => "Normal".to_string(),
                AcqMode::Peak => "Peak".to_string(),
                AcqMode::Average => format!("Average {}", self.m.avg_count),
                AcqMode::HiRes => "High res".to_string(),
            };
            let acq = format!(" \u{b7} {}", acq);
            if x + f.m.width(&acq) <= room {
                cv.text(&f.m, x, 25, &acq, tcol);
            }
        }
        // buttons (the open menu in orange)
        let root = self.menu_root();
        for (b, l, r) in buttons {
            let hl = self.menu_open
                && match b {
                    TopBtn::Menu(m) => root == m,
                    TopBtn::System => root == MenuId::Utility,
                };
            btn(cv, r, BTN, if hl { ACCENT } else { BTN_BORDER });
            cv.text_in(&f.mb, r, 1, l, if hl { ACCENT } else { TEXT });
        }
        // LAN: an address without a cable is only the fallback waiting for one
        let lan = self.addr_of("eth0").filter(|_| self.eth_link);
        self.draw_link(cv, Self::NET_BOX, if lan.is_none() { "LAN" } else if self.net_fixed { "LAN \u{b7} fixed" } else { "LAN \u{b7} DHCP" }, lan);
        // WLAN and USB sticks, while they are there
        if self.wlan_stick {
            self.draw_link(cv, Self::WLAN_BOX, "WLAN", self.addr_of(oos_hal::wlan::IFACE));
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
    fn draw_toast(&self, cv: &mut Canvas) {
        let Some((s, _)) = &self.toast else { return };
        let f = &self.fonts;
        let tw = f.mb.width(s) + 44;
        let mut r = Rect::new(W / 2 - tw / 2, PLOT_Y + 4, tw, 28);
        let dlg = self.dlg_layout().map(|d| d.r).filter(|d| overlaps(*d, r));
        if let Some(d) = dlg {
            let (left, right) = (d.x - 6, W - 6 - (d.x + d.w));
            if right >= tw + 16 {
                r.x = d.x + d.w + (right - tw) / 2;
            } else if left >= tw + 16 {
                r.x = 6 + (left - tw) / 2;
            } else {
                r.y = d.y + d.h + 6;
            }
        }
        cv.round(r, 6, mix(BAR, AMBER, 110), 255);
        cv.round(r.inset(1), 5, mix(BAR, AMBER, 34), 255);
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
        if m == MenuId::MeasAdd {
            MenuId::Measure
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
        let (y, h) = (530, 66);
        // 2-channel models: no cards for CH3/CH4 (zero width, never drawn or hit)
        let n = if self.offline { 0 } else { n_ch() };
        // Stored refs with analog off: no CH1..CH4 cards. TIME and trigger keep the bar.
        let hide_analog = (0..n).all(|c| !self.m.ch[c].on) && self.refs.iter().any(|r| r.as_ref().is_some_and(|r| r.on));
        let n_on = (0..n).filter(|&c| self.m.ch[c].on).count() as i32;
        let n_off = if hide_analog { 0 } else { n as i32 - n_on };
        let caps = &oos_hal::model::platform().caps;
        let aux = [(caps.la, self.m.la.on), (caps.has_gen(), self.m.afg_on)];
        let start = Self::MENU_BTN.x + Self::MENU_BTN.w + 7;
        let room = |wide: bool| {
            let a: i32 = aux.iter().filter(|a| a.0).map(|a| if a.1 && wide { AUX_WIDE } else { AUX_NARROW } + 7).sum();
            W - 6 - start - n_off * (56 + 7) - n_on * 7 - a - (130 + 7) - 222
        };
        let wide = n_on == 0 || room(true) / n_on.max(1) >= CH_MIN;
        let won = if n_on > 0 { (room(wide) / n_on).min(213) } else { 0 };
        let mut x = start;
        let mut ch = [Rect::new(0, 0, 0, 0); NCH];
        for (c, r) in ch.iter_mut().enumerate() {
            if c >= n || hide_analog {
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
        let rest = W - 6 - x - 7;
        let tw = ((rest as f32 * 0.42) as i32).min(rest - 222).max(130);
        let time = Rect::new(x, y, tw, h);
        x += tw + 7;
        Cards { ch, la, gen, time, trig: Rect::new(x, y, W - 6 - x, h) }
    }

    /// The big digits of a card while `s` fits `room`, else the next size: nothing is cut off with every card on.
    fn card_font<'a>(f: &'a Fonts, s: &str, room: i32) -> &'a Font {
        if f.l.width(s) <= room { &f.l } else { &f.m }
    }

    /// Trigger card buttons, one over the other at the right edge (touch): source, sweep; the rest of the card, and with
    /// it the level, opens the menu.
    fn trig_parts(r: Rect) -> (Rect, Rect) {
        let (w, h) = (66, (r.h - 12 - 4) / 2);
        let x = r.x + r.w - w - 6;
        (Rect::new(x, r.y + 6, w, h), Rect::new(x, r.y + 6 + h + 4, w, h))
    }

    /// A card like the channels': on, wide with its values; on, narrow with a full dot; off, with a ring.
    #[allow(clippy::too_many_arguments)]
    fn draw_aux_card(&self, cv: &mut Canvas, r: Rect, name: &str, col: Rgb, on: bool, sel: bool, big: &str, sub: &str) {
        let f = &self.fonts;
        cv.round(r, 7, if sel || on { col } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.set_clip(r.inset(3));
        if on && r.w > AUX_NARROW {
            cv.fill(Rect::new(r.x + 6, r.y + 10, 3, r.h - 20), col);
            cv.text(&f.sb, r.x + 16, r.y + 18, name, col);
            cv.circle(r.x + r.w - 16, r.y + 14, 5, col);
            cv.text(&f.mb, r.x + 16, r.y + 42, big, TEXT);
            cv.text(&f.s, r.x + 16, r.y + 58, sub, DIM);
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
        let f = &self.fonts;
        cv.fill(BOTTOM, BG);
        let root = if self.menu_open { Some(self.menu_root()) } else { None };
        // start menu button: three bars and "Menu"; amber "Locked" while touch is locked (a tap unlocks)
        let r = Self::MENU_BTN;
        let (hl, lock) = (root == Some(MenuId::Main), self.m.touch_lock);
        cv.round(r, 7, if hl { ACCENT } else if lock { AMBER } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        let col = if hl { ACCENT } else if lock { AMBER } else { TEXT };
        for k in 0..3 {
            cv.fill(Rect::new(r.x + r.w / 2 - 11, r.y + 16 + k * 7, 22, 3), col);
        }
        cv.text_in(&f.sb, Rect::new(r.x, r.y + 42, r.w, 16), 1, if lock { "Locked" } else { "Menu" }, col);
        let (chs, tr, trg) = self.cards();
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
                cv.text(&f.sb, r.x + 16, r.y + 18, &name, col);
                cv.circle(r.x + r.w - 16, r.y + 14, 5, col);
                let scale = format!("{}/div", si_short(ch.volts_per_div() as f64, ch.unit.name()));
                cv.text(Self::card_font(f, &scale, r.w - 24), r.x + 16, r.y + 42, &scale, TEXT);
                let cpl = ch.coupling.name();
                let mut sub = vec![cpl.to_string(), format!("{}\u{d7}", ch.probe)];
                if ch.bw != 0 && ch.bw != BW_FULL {
                    sub.push(format!("BW {}", BWS[ch.bw].0));
                }
                if ch.invert {
                    sub.push("Inv".into());
                }
                sub.push(si_short(ch.offset as f64, ch.unit.name()));
                cv.text(&f.s, r.x + 16, r.y + 58, &sub.join(" \u{b7} "), DIM);
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
        // time base (sample rate and depth are in the top bar)
        let r = tr;
        let sel = root == Some(MenuId::Horizontal);
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        cv.text(&f.sb, r.x + 12, r.y + 18, "TIME", LABEL);
        let tdiv = format!("{}/div", si_short(self.m.time_per_div() as f64, "s"));
        cv.text(Self::card_font(f, &tdiv, r.w - 20), r.x + 12, r.y + 42, &tdiv, TEXT);
        if self.m.zoom {
            cv.text(&f.sb, r.x + 12, r.y + 58, &format!("zoom {}/div", si_short(tscales()[self.m.zscale] as f64, "s")), ACCENT);
        } else {
            cv.text(&f.s, r.x + 12, r.y + 58, &format!("delay {}", si_short(self.m.delay as f64, "s")), DIM);
        }
        // trigger: type, slope / polarity / standard and level; source and sweep buttons.
        // Offline mode puts the reference name here: no edge, no level, no sweep.
        let r = trg;
        if self.offline {
            self.draw_offline_card(cv, r, root == Some(MenuId::Ref));
            return;
        }
        let sel = root == Some(MenuId::Trigger);
        cv.round(r, 7, if sel { ACCENT } else { CARD_BORDER }, 255);
        cv.round(r.inset(2), 5, BAR, 255);
        // type and slope on the first line, the level under them: it has the width up to the buttons
        let tx = cv.text(&f.sb, r.x + 12, r.y + 18, &self.m.trig_type.name().to_uppercase(), LABEL) + 8;
        if matches!(self.m.trig_type, TrigType::Edge | TrigType::Slope | TrigType::Timeout | TrigType::NthEdge | TrigType::Delay) {
            draw_slope(cv, tx, r.y + 6, self.m.slope, ACCENT);
        } else {
            let tag = match self.m.trig_type {
                TrigType::Pulse | TrigType::Runt => if self.m.trig_pol == Pol::Pos { "+" } else { "-" },
                TrigType::Video => if self.m.video_std == VideoStd::Pal { "P" } else { "N" },
                _ => "",
            };
            cv.text(&f.sb, tx, r.y + 18, tag, ACCENT);
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
            MenuId::Decode if !m.dec_on_la() => vec![ch(m.dec_src)],
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
            MenuId::ChMore(c) => vec![Item::ChUnit(c), Item::ChBias(c), Item::ChBiasZero(c), Item::ChSkew(c), Item::ChExpand],
            MenuId::Horizontal => {
                if self.m.zoom {
                    vec![Item::HScale, Item::HFine, Item::HExpandI, Item::HDelay, Item::HZero, Item::Zoom, Item::ZScale, Item::ZDelay, Item::AutoRoll]
                } else {
                    vec![Item::HScale, Item::HFine, Item::HExpandI, Item::HDelay, Item::HZero, Item::Zoom, Item::AutoRoll]
                }
            }
            MenuId::Trigger => self.trigger_items(),
            // the Horizontal Menu key opens Acquire; time base and zoom one level down
            MenuId::Acquire => {
                let mut v = vec![Item::AcqMode];
                if self.m.acq == AcqMode::Average {
                    v.push(Item::AcqAvg);
                }
                v.extend([Item::AcqDepth, Item::AcqSeg, Item::Open(MenuId::Horizontal)]);
                v
            }
            MenuId::MeasPair => (0..tools::PAIRS.len()).map(Item::MeasPair).collect(),
            MenuId::Dvm => vec![Item::DvmOn, Item::DvmSrc, Item::DvmMode, Item::CntOn, Item::CntSrc, Item::CntMode, Item::CntDigits, Item::CntClear],
            MenuId::Measure => vec![Item::MeasSrc, Item::MeasSrcB, Item::Open(MenuId::MeasAdd), Item::Open(MenuId::MeasPair), Item::MeasRefs, Item::MeasIdle, Item::MeasTsetup, Item::MeasThold, Item::MeasOutside, Item::MeasN, Item::MeasEdges, Item::MeasStats, Item::MeasLabels, Item::MeasClear],
            MenuId::Decode => self.decode_items(),
            MenuId::Logger => vec![Item::LogOn, Item::LogIvl, Item::LogInfo],
            MenuId::Ref => vec![Item::RefSrc, Item::RefSave, Item::RefClear, Item::RefSel, Item::RefVpd, Item::RefOffset, Item::RefColor, Item::RefLabel, Item::RefExport, Item::RefImport].into_iter().chain((0..NREF).map(Item::RefShow)).collect(),
            MenuId::MeasAdd => ALL_MEAS.iter().map(|&k| Item::MeasAdd(k)).collect(),
            MenuId::Cursor if self.m.cursor == CursorMode::Off => vec![Item::CurMode],
            MenuId::Cursor if self.m.cursor == CursorMode::Xy => vec![Item::CurMode, Item::CurA, Item::CurB, Item::CurC, Item::CurD],
            MenuId::Cursor => vec![Item::CurMode, Item::CurA, Item::CurB],
            MenuId::Display => {
                let mut v = vec![Item::Open(MenuId::Windows), Item::PaneSel, Item::Persist, Item::ColorGrade, Item::Intensity, Item::GridStyle, Item::GridBright, Item::GridFrame, Item::Dots, Item::ZInput, Item::Histogram, Item::XyMode, Item::Hdmi, Item::ClearDisp];
                if self.m.pane == tables::PANE_TREND {
                    v.splice(1..1, [Item::TrendSel(0), Item::TrendSel(1)]);
                }
                v
            }
            MenuId::Storage => vec![Item::StoreDest, Item::StoreSlot, Item::StoreSaveSetup, Item::StoreRecall, Item::StoreCsv, Item::StoreBin, Item::StoreFfSave, Item::StoreFfLoad, Item::StorePng],
            MenuId::Spectrum => vec![Item::SpecOn, Item::SpecSrc, Item::SpecCenter, Item::SpecSpan, Item::SpecHarm, Item::SpecRbw, Item::SpecWin, Item::SpecTrace, Item::SpecRef, Item::SpecWf, Item::SpecDist, Item::SpecAcprW, Item::SpecAcprS, Item::SpecPeak, Item::SpecMargin, Item::SpecLimitSet, Item::SpecLimitClear],
            MenuId::Eye => vec![Item::EyeOn, Item::EyeSrc, Item::EyeRate, Item::EyeBer, Item::EyeB, Item::EyeHp, Item::EyePnLo, Item::EyePnHi, Item::EyeNpj, Item::EyeRail, Item::EyeNotch, Item::EyeClear],
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
            MenuId::Network => vec![Item::NetEth, Item::NetMac, Item::NetWeb, Item::NetPorts, Item::NetWlan, Item::WlanScan, Item::WlanForget, Item::WlanInfo],
            MenuId::Bode => self.bode_items(),
            MenuId::Sim => self.sim_items(),
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
            MenuId::Utility => vec![Item::Offline, Item::QuickAct, Item::TouchLock, Item::BootMode, Item::Overclock, Item::Shot, Item::Open(MenuId::SelfCal), Item::Open(MenuId::Network), Item::SysInfo, Item::About, Item::Update, Item::UpdateRollback, Item::Stock],
            MenuId::Pick => {
                let n = self.pick.map_or(0, |p| self.options(p.it).len());
                (0..n).map(Item::PickOpt).collect()
            }
            MenuId::Math if self.m.math == MathOp::Off => vec![Item::MathSel, Item::MathFam, Item::MathOpI],
            MenuId::Math if self.m.math == MathOp::Expr => vec![Item::MathSel, Item::MathFam, Item::MathOpI, Item::MathExprI, Item::MathExprEdit, Item::MathScale, Item::MathOffset, Item::MathAuto],
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
                v
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
        let ana = vec![Open(MenuId::Spectrum), Open(MenuId::Eye), Open(MenuId::Power), Open(MenuId::Bode), Open(MenuId::BwTest), IdentifyAct, Open(MenuId::Magnetic), Open(MenuId::Inverter), Open(MenuId::Dpt)];
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
            ("Analysis", vec![Open(MenuId::Measure), Open(MenuId::Cursor), Open(MenuId::Math), Open(MenuId::Ref), Open(MenuId::Sim), Open(MenuId::Decode), Open(MenuId::Mask)]),
            ("Analysers", ana),
            ("Instruments", ins),
            ("Display", disp),
            ("System", vec![Open(MenuId::Storage), Shot, QuickRun, LockAct, Open(MenuId::Utility), PowerAct]),
        ];
        g.retain(|x| !x.1.is_empty());
        g
    }

    /// Shows record `hist_pos + d` of the history (d = 0: the newest), stopping the acquisition.
    fn history_step(&mut self, d: i32) {
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

    /// Recomputes the measurements of the record shown.
    fn measure_last(&mut self) {
        let Some(rec) = self.last[0].clone() else { return };
        for c in 0..NCH {
            if !self.m.ch[c].on {
                (self.stats[c], self.ana[c]) = (Stats::default(), Default::default());
                continue;
            }
            let (v, dt) = self.meas_wave(&rec, c);
            self.stats[c] = stats(&v, dt);
            self.ana[c] = oos_dsp::measure::analyze_refs(&v, dt, 0.4 * self.m.ch[c].volts_per_div() as f64, self.m.meas_lpct, self.m.meas_hpct);
        }
        self.measure_refs();
    }

    /// Each loaded ref, at the slot's own sample interval. Coupling and the bandwidth limit are not applied.
    fn measure_refs(&mut self) {
        for i in 0..NREF {
            self.ref_ana[i] = match &self.refs[i] {
                Some(r) if r.v.len() >= 2 && r.dt > 0.0 => oos_dsp::measure::analyze_refs(&r.v, r.dt, 0.4 * r.vpd as f64, self.m.meas_lpct, self.m.meas_hpct),
                _ => Default::default(),
            };
        }
    }

    /// A record with no samples, for an analyser whose sources are all stored refs.
    pub(super) fn blank_record() -> Record {
        Record { t0: 0.0, dt: 1.0, ch: Default::default(), triggered: false, partial: false, raw: None, la: None }
    }

    /// Samples to measure channel `c` on: every captured sample of the screen window when the hardware frame has more
    /// than the screen record (conditioned like the record), else the screen record.
    fn meas_wave(&self, rec: &Record, c: usize) -> (Vec<f32>, f64) {
        // high res: the screen record is the mean of those samples, the measurements follow it
        if let Some(r) = rec.raw.as_ref().filter(|_| self.m.acq != AcqMode::HiRes) {
            let v = &r.frame.ch[c];
            if r.i1 <= v.len() && r.i1 - r.i0 > rec.ch[c].len() {
                return (v[r.i0..r.i1].to_vec(), r.frame.dt);
            }
        }
        (rec.ch[c].clone(), rec.dt)
    }

    /// Search: times (s, relative to the trigger) of all events of the trigger condition in the record shown.
    fn search_events(&self) -> Vec<f64> {
        let Some(rec) = &self.last[0] else { return Vec::new() };
        if !self.m.trig_channels().iter().enumerate().all(|(c, &u)| !u || !rec.ch[c].is_empty()) {
            return Vec::new();
        }
        let refs: [&[f32]; NCH] = std::array::from_fn(|c| &rec.ch[c][..]);
        let hyst: [f32; NCH] = std::array::from_fn(|c| 0.1 * self.m.ch[c].volts_per_div());
        let inp = oos_dsp::trigger::Input { ch: refs, dt: rec.dt, hyst };
        crate::trig::find_all(&self.m, &inp).into_iter().map(|i| rec.t0 + i * rec.dt).collect()
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
        if self.m.la_on() {
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
        match p {
            Proto::Uart => v.extend([DecBaud, DecBits, DecParity, DecFmt]),
            Proto::Spi => v.extend([DecMode, DecBits]),
            Proto::Can => v.extend([DecCanRate, DecCanH]),
            Proto::CanFd => v.extend([DecCanRate, DecFdRate, DecCanH]),
            Proto::Lin | Proto::Uart9 => v.push(DecBaud),
            Proto::ModbusRtu => v.extend([DecBaud, DecParity]),
            Proto::ModbusAscii => v.extend([DecBaud, DecBits, DecParity]),
            Proto::I2s | Proto::Parallel => v.push(DecBits),
            Proto::Arinc429 => v.push(DecArate),
            Proto::Sent => v.extend([DecTick, DecNibbles]),
            Proto::Usb => v.push(DecUsbSpeed),
            Proto::Manchester | Proto::Mlt3 => v.push(DecBaud),
            _ => {}
        }
        // LA lines: the probe's comparators set the levels (LA page)
        if !on_la {
            v.push(DecThr);
        }
        // what it decoded, as a table, and as a file
        v.extend([DecTable, DecExport]);
        v
    }

    /// Switches the decoder protocol with sources that match the demo bus traffic.
    fn set_dec_proto(&mut self, p: Proto) {
        let m = &mut self.m;
        m.dec_proto = p;
        // LA input: the lines of the demo traffic (D8 UART, D9 CS, D10 clock, D11 MOSI, D12 SCL, D13 SDA)
        m.dec_la = match p {
            Proto::I2c | Proto::I3c | Proto::Ds2482 => [Some(13), Some(12), None, None],
            Proto::Spi => [Some(11), Some(10), Some(9), None],
            Proto::Parallel => [Some(0), None, None, None],
            _ => [Some(8), Some(10), None, None],
        };
        match p {
            Proto::Uart => {
                (m.dec_src, m.dec_bits) = (2, 8);
            }
            Proto::I2c => {
                (m.dec_src, m.dec_src2) = (2, 3);
            }
            Proto::Spi => {
                (m.dec_src, m.dec_src2, m.dec_cs, m.dec_mode, m.dec_bits) = (2, 3, Some(1), 0, 8);
            }
            Proto::Can => {
                (m.dec_src, m.dec_canrate, m.dec_canh) = (2, 2, false);
            }
            Proto::Lin => {
                (m.dec_src, m.dec_baud) = (2, 4);
            }
            // demo: WS on CH1, SD on CH2, SCK on CH3
            Proto::I2s => {
                (m.dec_src, m.dec_src2, m.dec_cs, m.dec_bits) = (1, 2, Some(0), 16);
                m.ch[0].on = true;
                m.ch[1].on = true;
            }
            // bipolar bus: threshold at 0 V
            Proto::Mil1553 => {
                (m.dec_src, m.dec_thr) = (2, 0.0);
            }
            Proto::ModbusAscii => {
                (m.dec_src, m.dec_bits) = (2, 7);
            }
            Proto::ModbusRtu => {
                (m.dec_src, m.dec_baud, m.dec_parity) = (2, 4, oos_dsp::decode::Parity::Even);
            }
            Proto::Uart9 => {
                (m.dec_src, m.dec_baud) = (2, 3);
            }
            // CC line ~1.1 V
            Proto::UsbPd => {
                (m.dec_src, m.dec_thr) = (2, 0.55);
            }
            Proto::Parallel => {
                (m.dec_src, m.dec_bits, m.dec_cs) = (0, 3, Some(3));
            }
            Proto::Jtag | Proto::Qspi => {
                (m.dec_src, m.dec_src2, m.dec_cs, m.dec_src4) = (0, 1, Some(2), Some(3));
            }
            Proto::Ds2482 | Proto::I3c | Proto::Swd => {
                (m.dec_src, m.dec_src2) = (2, 3);
            }
            // demo: one ARINC 429 line against ground (+-5 V), HI / LO at +-2.5 V
            Proto::Arinc429 => {
                (m.dec_src, m.dec_thr) = (2, 2.5);
            }
            // 5 V logic
            Proto::Sent => {
                (m.dec_src, m.dec_thr) = (2, 2.5);
            }
            // demo: D+ on CH3, D- on CH4, 3.3 V
            Proto::Usb => {
                (m.dec_src, m.dec_cs, m.dec_thr) = (2, Some(3), 1.65);
            }
            _ => m.dec_src = 2,
        }
        m.ch[2.min(NCH - 1)].on = true;
        if matches!(p, Proto::I2c | Proto::Spi) {
            m.ch[3.min(NCH - 1)].on = true;
        }
        // every source the protocol uses
        let r = p.roles();
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
        if p == Proto::Spi {
            m.ch[1].on = true;
        }
        // a trigger that holds the demo bus still: I2C start (first SDA fall after idle), SPI CS falling
        match p {
            Proto::I2c => {
                (m.trig_type, m.trig_src, m.slope, m.nth_idle, m.nth_n, m.trig_level) = (TrigType::NthEdge, 2, Slope::Fall, 20e-6, 1, 1.65);
            }
            Proto::Spi => {
                (m.trig_type, m.trig_src, m.slope, m.trig_level) = (TrigType::Edge, 1, Slope::Fall, 1.65);
            }
            // CAN start of frame / LIN break: first falling edge after the bus idle
            Proto::I2s => {
                (m.trig_type, m.trig_src, m.slope, m.trig_level) = (TrigType::Edge, 0, Slope::Fall, 1.65);
            }
            Proto::Mil1553 => {
                (m.trig_type, m.trig_src, m.slope, m.nth_idle, m.nth_n, m.trig_level) = (TrigType::NthEdge, 2, Slope::Rise, 3e-6, 1, 2.0);
            }
            Proto::Can | Proto::Lin => {
                (m.trig_type, m.trig_src, m.slope, m.nth_idle, m.nth_n, m.trig_level) = (TrigType::NthEdge, 2, Slope::Fall, 20e-6, 1, 1.65);
            }
            _ => {}
        }
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
            TT::Edge => v.extend([TrigSrc, TrigSlope, TrigLevel, Trig50]),
            // the protocol and its sources come from the Decode menu
            TT::Bus => {
                v.extend([DecProto, TrigBusWhat]);
                if matches!(m.trig_bus, BusWhat::Id | BusWhat::Data) {
                    v.push(TrigBusVal);
                }
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
            Item::DecSrc if self.m.dec_proto.roles()[0] != Some("Source") => self.m.dec_proto.roles()[0].unwrap_or("Source"),
            Item::DecSrc2 if self.m.dec_proto.roles()[1].is_some() => self.m.dec_proto.roles()[1].unwrap_or("Clock"),
            Item::DecCs if self.m.dec_proto.roles()[2].is_some() => self.m.dec_proto.roles()[2].unwrap_or("CS"),
            Item::DecSrc4 if self.m.dec_proto.roles()[3].is_some() => self.m.dec_proto.roles()[3].unwrap_or("Source 4"),
            Item::TrigT1 if matches!(self.m.trig_when, WhenKind::In | WhenKind::Out) => "Lower limit",
            Item::NavHist if self.hist_pos.is_some() => "Live",
            _ => Self::label(it),
        }
    }

    /// Switches the trigger type and sets starting values that trigger on the demo signals.
    fn set_trig_type(&mut self, t: TrigType) {
        if t == TrigType::Bus {
            // the decoder defines the protocol: switch it on with its demo sources and a value its traffic contains
            self.m.dec_on = true;
            let p = self.m.dec_proto;
            self.set_dec_proto(p);
            let m = &mut self.m;
            m.trig_type = t;
            (m.trig_bus, m.trig_bus_val) = match p {
                Proto::Can => (BusWhat::Id, 0x123),
                Proto::Lin => (BusWhat::Id, 0x21),
                Proto::I2c => (BusWhat::Id, 0x50),
                Proto::Mil1553 => (BusWhat::Id, 0x0c23),
                Proto::I2s => (BusWhat::Data, 0x7fff),
                Proto::FlexRay => (BusWhat::Id, 0x2a),
                // label 310 (octal)
                Proto::Arinc429 => (BusWhat::Id, 0o310),
                Proto::Sent => (BusWhat::Data, 0x7f480b),
                // the demo device's address
                Proto::Usb => (BusWhat::Id, 5),
                _ => (BusWhat::Frame, 0x40),
            };
            self.dirty_map = true;
            return;
        }
        let m = &mut self.m;
        m.trig_type = t;
        let on = |c: usize, m: &mut Model| m.ch[c].on = true;
        match t {
            TrigType::Bus => {} // handled above
            TrigType::Edge => {
                m.trig_src = 0;
                m.trig_level = 1.5;
            }
            TrigType::Pulse => {
                on(3, m);
                (m.trig_src, m.trig_pol, m.trig_when, m.trig_t1, m.trig_level) = (3, Pol::Pos, WhenKind::Lt, 30e-6, 1.65);
            }
            TrigType::Slope => {
                (m.trig_src, m.slope, m.trig_when, m.trig_t1, m.trig_level, m.trig_level2) = (0, Slope::Rise, WhenKind::Gt, 1e-6, 2.0, 0.5);
            }
            TrigType::Video => {
                on(1, m);
                (m.trig_src, m.trig_level) = (1, 0.15);
                m.ch[1].vscale = VSCALES.iter().position(|&v| (v - 0.2).abs() < 1e-6).unwrap_or(8);
                m.ch[1].offset = -0.5;
            }
            TrigType::Pattern => {
                on(3, m);
                m.pat = std::array::from_fn(|i| if i == 0 { Bit::H } else if i == NCH - 1 { Bit::R } else { Bit::X });
                m.trig_src = 0;
            }
            TrigType::Duration => {
                m.pat = std::array::from_fn(|i| if i == 0 { Bit::H } else { Bit::X });
                (m.trig_src, m.trig_when, m.trig_t1) = (0, WhenKind::Gt, 200e-6);
            }
            TrigType::Timeout => {
                (m.trig_src, m.slope, m.trig_t1, m.trig_level) = (0, Slope::Rise, 1.2e-3, 1.5);
            }
            TrigType::Runt => {
                (m.trig_src, m.trig_pol, m.trig_when, m.trig_level, m.trig_level2) = (0, Pol::Pos, WhenKind::Off, 2.0, 0.5);
            }
            TrigType::Window => {
                on(1, m);
                (m.trig_src, m.win_mode, m.trig_level, m.trig_level2) = (1, WinMode::Enter, 0.5, -0.5);
            }
            TrigType::Delay => {
                on(3, m);
                (m.trig_src, m.slope, m.trig_src2, m.trig_slope2, m.trig_when, m.trig_t1, m.trig_level, m.trig_level2) =
                    (0, Slope::Rise, 3, Slope::Rise, WhenKind::Gt, 40e-6, 1.5, 1.65);
            }
            TrigType::SetupHold => {
                on(2, m);
                on(3, m);
                (m.trig_src, m.trig_src2, m.trig_slope2, m.setup, m.hold, m.trig_level, m.trig_level2) = (2, 3, Slope::Rise, 2e-6, 2e-6, 1.65, 1.65);
            }
            TrigType::NthEdge => {
                on(2, m);
                (m.trig_src, m.slope, m.nth_idle, m.nth_n, m.trig_level) = (2, Slope::Fall, 50e-6, 1, 1.65);
            }
        }
        if m.trig_type != TrigType::Runt && m.trig_when == WhenKind::Off {
            m.trig_when = WhenKind::Lt;
        }
        self.dirty_map = true;
    }

    fn menu_title(&self, menu: MenuId) -> String {
        match menu {
            // /tmp is in RAM: say so where the files go
            MenuId::Storage if self.usb_used() => "Storage \u{b7} USB stick".into(),
            MenuId::Storage if self.store_dir.starts_with("/tmp") => format!("Storage \u{b7} {} (lost at power-off)", self.store_dir.display()),
            MenuId::Storage => format!("Storage \u{b7} {}", self.store_dir.display()),
            MenuId::Mask => "Mask test".into(),
            MenuId::Zone => "Zone trigger".into(),
            MenuId::Spectrum => "Spectrum analyser".into(),
            MenuId::Eye => "Eye diagram / jitter".into(),
            MenuId::Power => "Power analysis".into(),
            MenuId::Identify => "Identify: tap to apply".into(),
            MenuId::SelfCal => "Self-calibration".into(),
            MenuId::Network => "Network".into(),
            MenuId::Bode => "Bode plot".into(),
            MenuId::Sim => "SIM".into(),
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
            MenuId::MeasAdd => "Add measurement".into(),
            MenuId::MeasPair => "Delay and phase (A to B)".into(),
            MenuId::Dvm => "DVM and counter".into(),
            MenuId::Logger => "Data logger".into(),
            MenuId::Cursor => "Cursor".into(),
            MenuId::Display => "Display".into(),
            MenuId::Windows => "Windows".into(),
            MenuId::Utility => "System".into(),
            MenuId::Math => "Math".into(),
            MenuId::Ref => "Reference".into(),
            MenuId::Decode => "Decode".into(),
            MenuId::Pick => match self.pick {
                Some(p) if p.it == Item::TrigBusVal => format!("{} \u{b7} 0x{}_", self.item_label(p.it), self.keypad),
                Some(p) if p.it == Item::AfgFreqEntry => format!("Frequency \u{b7} {}_ (max {})", self.keypad, si_short(self.m.gen_max_hz(), "Hz")),
                Some(p) => format!("{} \u{b7} {}", self.menu_title(p.menu), self.item_label(p.it)),
                None => String::new(),
            },
        }
    }

    fn kind(it: Item) -> Kind {
        if let Some(k) = Self::meas_kind(it).or_else(|| Self::ana_kind(it)).or_else(|| Self::magnetic_kind(it)).or_else(|| Self::inverter_kind(it)).or_else(|| Self::dpt_kind(it)).or_else(|| Self::cal_kind(it)).or_else(|| Self::bode_kind(it)).or_else(|| Self::sim_kind(it)).or_else(|| Self::offline_kind(it)).or_else(|| Self::afg_kind(it)).or_else(|| Self::bw_kind(it)).or_else(|| Self::la_kind(it)).or_else(|| Self::win_kind(it)) {
            return k;
        }
        match it {
            Item::LogIvl | Item::ChBias(_) | Item::ChSkew(_) | Item::RefColor | Item::CurC | Item::CurD | Item::RefSel | Item::RefVpd | Item::RefOffset | Item::MathLinA | Item::MathLinB | Item::TrigHoldoff | Item::CntDigits | Item::MathScale | Item::MathOffset | Item::MathFc | Item::MathBw | Item::ZScale | Item::ZDelay | Item::DecThr => Kind::Value,
            Item::MathAuto | Item::ZoneBox | Item::ZoneShape | Item::ZoneClear => Kind::Action,
            Item::DecBits => Kind::Value,
            Item::TrigLevel2 | Item::TrigT1 | Item::TrigT2 | Item::VidLine | Item::PatLevel | Item::NthIdle | Item::NthN | Item::Setup | Item::Hold => Kind::Value,
            Item::ChScale(_) | Item::ChOffset(_) | Item::HScale | Item::HDelay | Item::TrigLevel | Item::AcqAvg | Item::CurA | Item::CurB | Item::Intensity | Item::GridBright => Kind::Value,
            Item::RefSave | Item::RefClear => Kind::Action,
            Item::StoreSlot | Item::MaskX | Item::MaskY => Kind::Value,
            Item::StoreSaveSetup | Item::StoreRecall | Item::StoreCsv | Item::StorePng | Item::StoreFfSave | Item::StoreFfLoad | Item::MaskCreate | Item::MaskFromRef | Item::MaskReset | Item::MaskClear => Kind::Action,
            Item::Open(_) | Item::MathExprEdit | Item::ChBiasZero(_) | Item::RefLabel | Item::RefExport | Item::RefImport | Item::DecTable | Item::DecExport | Item::SearchTable | Item::SearchSave | Item::StoreBin | Item::MeasPair(_) | Item::CntClear | Item::MaskSave | Item::MaskLoad | Item::ChLabel(_) | Item::SrcOn | Item::WlanInfo | Item::WlanScan | Item::WlanForget | Item::HZero | Item::Trig50 | Item::MeasAdd(_) | Item::MeasClear | Item::ClearDisp | Item::About | Item::SysInfo | Item::Shot | Item::Stock | Item::Update | Item::UpdateRollback => Kind::Action,
            Item::PickOpt(_) | Item::ViewExit | Item::ViewSettings => Kind::Action,
            Item::RunStop | Item::SingleAct | Item::AutosetAct | Item::DefaultAct | Item::SlopeAct | Item::ForceAct => Kind::Action,
            Item::NavHist | Item::NavBack | Item::NavFwd | Item::QuickRun | Item::LockAct | Item::PowerAct => Kind::Action,
            _ => Kind::Choice,
        }
    }

    fn label(it: Item) -> &'static str {
        if let Some(l) = Self::meas_cfg_label(it).or_else(|| Self::ana_label(it)).or_else(|| Self::magnetic_label(it)).or_else(|| Self::inverter_label(it)).or_else(|| Self::dpt_label(it)).or_else(|| Self::cal_label(it)).or_else(|| Self::bode_label(it)).or_else(|| Self::sim_label(it)).or_else(|| Self::offline_label(it)).or_else(|| Self::afg_label(it)).or_else(|| Self::bw_label(it)).or_else(|| Self::la_label(it)).or_else(|| Self::win_label(it)) {
            return l;
        }
        match it {
            Item::Open(MenuId::Acquire) => "Acquire",
            Item::Open(MenuId::Trigger) => "Trigger",
            Item::Open(MenuId::Horizontal) => "Horizontal",
            Item::Open(MenuId::ChMore(_)) => "More",
            Item::Open(MenuId::Measure) => "Measure",
            Item::Open(MenuId::MeasAdd) => "Add item",
            Item::Open(MenuId::Cursor) => "Cursor",
            Item::Open(MenuId::Display) => "Display",
            Item::Open(MenuId::Utility) => "System",
            Item::Open(MenuId::MeasPair) => "Channel pair",
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
            Item::PaneSel => "Second pane",
            Item::TrendSel(0) => "Trend chart 1",
            Item::TrendSel(_) => "Trend chart 2",
            Item::DecSrc => "Source",
            Item::DecBaud => "Baud",
            Item::DecCanRate => "Bit rate",
            Item::TrigBusWhat => "Match",
            Item::TrigBusVal => "Value",
            Item::DecCanH => "Signal",
            Item::DecFdRate => "Data rate",
            Item::DecArate => "Bit rate",
            Item::DecTick => "Clock tick",
            Item::DecNibbles => "Data nibbles",
            Item::DecUsbSpeed => "Speed",
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
            Item::RefSrc => "Source",
            Item::RefSave => "Save to next R",
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
            Item::MathAuto => "Auto scale",
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
            Item::AcqDepth => "Memory",
            Item::MeasSrc => "Source A",
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
            Item::MaskSave => "Save mask",
            Item::MaskLoad => "Load mask",
            Item::MathLinA => "A",
            Item::MathLinB => "B",
            Item::MeasAdd(k) => k.name(),
            Item::MeasClear => "Clear all",
            Item::CurMode => "Mode",
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
            Item::ClearDisp => "Clear",
            Item::Dots => "Dots",
            Item::Hdmi => "HDMI",
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
            Item::TouchLock => "Touch",
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
            Item::NetWeb => "Web remote",
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
        if let Some(v) = self.meas_value(it).or_else(|| self.ana_value(it)).or_else(|| self.magnetic_value(it)).or_else(|| self.inverter_value(it)).or_else(|| self.dpt_value(it)).or_else(|| self.cal_value(it)).or_else(|| self.bode_value(it)).or_else(|| self.sim_value(it)).or_else(|| self.offline_value(it)).or_else(|| self.afg_value(it)).or_else(|| self.bw_value(it)).or_else(|| self.la_value(it)).or_else(|| self.win_value(it)) {
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
            Item::TrigSrc => format!("CH{}", m.trig_src + 1),
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
            Item::TrigSrc2 => format!("CH{}", m.trig_src2 + 1),
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
            Item::AcqSeg => if m.segments == 0 { "Off".into() } else { m.segments.to_string() },
            Item::AcqDepth => mdepths()[m.mdepth].0.into(),
            Item::MeasSrc => self.wave_name(self.meas_src),
            Item::MeasSrcB => self.wave_name(self.meas_src_b),
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
            Item::CurB => self.cursor_values().get(1).map_or(format!("{:.0} %", m.cur_b * 100.0), |v| v.1.clone()),
            Item::Persist => PERSIST[m.persist].0.into(),
            Item::Intensity => format!("{} %", m.intensity),
            Item::GridStyle => format!("{:?}", m.grid),
            Item::GridBright => format!("{} %", m.grid_bright),
            Item::GridFrame => onoff(m.grid_frame),
            Item::TrigNoise => onoff(m.trig_nrej),
            Item::Dots => onoff(m.dots),
            Item::Hdmi => if m.hdmi_wave { "Waveform".into() } else { "Mirror".into() },
            Item::ZoneSel => format!("Z{}", self.zone_sel + 1),
            Item::ZoneOn => onoff(m.zones[self.zone_sel].on),
            Item::ZoneSrc => format!("CH{}", m.zones[self.zone_sel].src + 1),
            Item::ZoneMode => if m.zones[self.zone_sel].inside { "Intersect".into() } else { "Not intersect".into() },
            Item::ZInput => m.z_src.map_or("Off".to_string(), |c| format!("CH{}", c + 1)),
            Item::Histogram => format!("{:?}", m.hist),
            Item::TouchLock => if m.touch_lock { "Locked".into() } else { "On".into() },
            Item::MathOpI => m.math.name().into(),
            Item::MathFam => self.math_fam.name().into(),
            Item::StoreDest => self.store_dest.name().into(),
            Item::MaskInvert => onoff(m.mask_inv),
            Item::MeasStats => onoff(self.stats_on),
            Item::MeasLabels => onoff(self.m.meas_labels),
            Item::Zoom => onoff(m.zoom),
            Item::AutoRoll => onoff(m.auto_roll),
            Item::DecOn => onoff(m.dec_on),
            Item::DecSel => format!("D{}", m.dec_sel + 1),
            Item::PaneSel => tables::PANES[m.pane.min(tables::PANES.len() - 1)].into(),
            Item::TrendSel(k) => self.trend_options().get(m.trend_sel[k]).cloned().unwrap_or_else(|| "Auto".into()),
            Item::DecSrc => format!("CH{}", m.dec_src + 1),
            Item::DecBaud => format!("{}", crate::decode::BAUDS[m.dec_baud]),
            Item::DecCanRate => si(crate::decode::CAN_RATES[m.dec_canrate], "bit/s"),
            Item::TrigBusWhat => m.trig_bus.name().into(),
            Item::TrigBusVal => format!("0x{:X}", m.trig_bus_val),
            Item::DecCanH => if m.dec_canh { "CAN_H".into() } else { "RX/TX".into() },
            Item::DecFdRate => format!("{} M", crate::decode::FD_RATES[m.dec_fdrate] / 1e6),
            Item::DecArate => ARATE_NAMES[m.dec_arate].into(),
            Item::DecTick => tick_name(crate::decode::SENT_TICKS[m.dec_tick]),
            Item::DecNibbles => format!("{}", m.dec_nibbles),
            Item::DecUsbSpeed => USPEED_NAMES[m.dec_uspeed].into(),
            Item::DecSrc4 => m.dec_src4.map_or("None".into(), |c| format!("CH{}", c + 1)),
            Item::DecDiff => m.dec_diff.map_or("None".into(), |c| format!("CH{}", c + 1)),
            Item::DecThr => si(m.dec_thr as f64, "V"),
            Item::DecFmt => if m.dec_ascii { "ASCII".into() } else { "Hex".into() },
            Item::DecProto => m.dec_proto.name().into(),
            Item::DecSrc2 => format!("CH{}", m.dec_src2 + 1),
            Item::DecCs => m.dec_cs.map_or("None".into(), |c| format!("CH{}", c + 1)),
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
            Item::NetEth | Item::NetMac | Item::NetWeb | Item::NetPorts | Item::NetWlan => self.net_value(it),
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

    fn adjust(&mut self, it: Item, d: i32) {
        if self.meas_adjust(it, d) || self.ana_adjust(it, d) || self.magnetic_adjust(it, d) || self.inverter_adjust(it, d) || self.dpt_adjust(it, d) || self.cal_adjust(it, d) || self.bode_adjust(it, d) || self.sim_adjust(it, d) || self.offline_adjust(it, d) || self.afg_adjust(it, d) || self.bw_adjust(it, d) || self.la_adjust(it, d) || self.win_adjust(it, d) {
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
            Item::ChBias(c) => m.ch[c].bias += 0.01 * m.ch[c].volts_per_div() * d as f32,
            Item::ChSkew(c) => m.ch[c].skew = (m.ch[c].skew + 600e-12 * d as f32).clamp(-SKEW_MAX, SKEW_MAX),
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
            Item::ChOffset(c) => m.ch[c].offset += d as f32 * m.ch[c].volts_per_div() / 25.0,
            Item::HScale => {
                let old = m.time_per_div();
                if m.fine_t {
                    (m.tscale, m.tfine) = fine_time(old, d);
                } else {
                    m.tscale = step(m.tscale, tscales().len(), d);
                    m.tfine = 1.0;
                }
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
            }
            Item::HDelay => m.delay += d as f32 * m.time_per_div() / 25.0,
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
            Item::TrigLevel => m.trig_level += d as f32 * m.ch[m.trig_src].volts_per_div() / 25.0,
            Item::TrigType => {
                let i = TRIG_TYPES.iter().position(|&x| x == m.trig_type).unwrap_or(0);
                let t = TRIG_TYPES[cyc(i, TRIG_TYPES.len(), d)];
                self.set_trig_type(t);
            }
            Item::TrigLevel2 => {
                let c = if matches!(m.trig_type, TrigType::Delay | TrigType::SetupHold) { m.trig_src2 } else { m.trig_src };
                m.trig_level2 += d as f32 * m.ch[c].volts_per_div() / 25.0;
            }
            Item::TrigSrc2 => m.trig_src2 = src_step(m.trig_src2, NCH, d),
            Item::SearchOn => self.search_on = !self.search_on,
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
            Item::TrigT1 => m.trig_t1 = ladder_step(m.trig_t1, d),
            Item::TrigT2 => m.trig_t2 = ladder_step(m.trig_t2, d),
            Item::NthIdle => m.nth_idle = ladder_step(m.nth_idle, d),
            Item::Setup => m.setup = ladder_step(m.setup, d),
            Item::Hold => m.hold = ladder_step(m.hold, d),
            Item::VidStd => m.video_std = if m.video_std == VideoStd::Pal { VideoStd::Ntsc } else { VideoStd::Pal },
            Item::VidSync => {
                let all = [VSync::AllLines, VSync::Line, VSync::Odd, VSync::Even, VSync::AllFields];
                let i = all.iter().position(|&x| x == m.video_sync).unwrap();
                m.video_sync = all[cyc(i, all.len(), d)];
            }
            Item::VidLine => m.video_line = (m.video_line as i32 + d).clamp(1, m.video_std.lines() as i32 / 2 - 10) as u32,
            Item::Pat(c) => {
                let all = if m.trig_type == TrigType::Duration { &[Bit::H, Bit::L, Bit::X][..] } else { &[Bit::H, Bit::L, Bit::X, Bit::R, Bit::F][..] };
                let i = all.iter().position(|&x| x == m.pat[c]).unwrap_or(0);
                m.pat[c] = all[cyc(i, all.len(), d)];
            }
            Item::PatLevel => {
                let c = m.trig_src;
                m.pat_levels[c] += d as f32 * m.ch[c].volts_per_div() / 25.0;
            }
            Item::WinModeI => {
                let all = [WinMode::Enter, WinMode::Exit, WinMode::Inside];
                let i = all.iter().position(|&x| x == m.win_mode).unwrap();
                m.win_mode = all[cyc(i, 3, d)];
            }
            Item::NthN => m.nth_n = (m.nth_n as i32 + d).clamp(1, 65535) as u32,
            Item::AcqMode => {
                let all = [AcqMode::Normal, AcqMode::Peak, AcqMode::Average, AcqMode::HiRes];
                let i = all.iter().position(|&x| x == m.acq).unwrap();
                m.acq = all[cyc(i, 4, d)];
            }
            Item::AcqSeg => {
                let i = SEGMENTS.iter().position(|&n| n >= m.segments).unwrap_or(0);
                m.segments = SEGMENTS[cyc(i, SEGMENTS.len(), d)];
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
            Item::DvmOn => m.dvm_on = !m.dvm_on,
            Item::DvmSrc => m.dvm_src = src_step(m.dvm_src, NCH, d),
            Item::DvmMode => m.dvm_mode = (m.dvm_mode as i32 + d).rem_euclid(3) as usize,
            Item::CntOn => m.cnt_on = !m.cnt_on,
            Item::CntSrc => m.cnt_src = src_step(m.cnt_src, NCH, d),
            Item::CntMode => m.cnt_mode = (m.cnt_mode as i32 + d).rem_euclid(3) as usize,
            Item::CntDigits => m.cnt_digits = (m.cnt_digits as i32 + d).clamp(3, 7) as u32,
            Item::TrigHoldoff => m.holdoff = holdoff_step(m.holdoff, d),
            Item::MathLinA => m.math_lin.0 = (m.math_lin.0 + 0.1 * d as f32).clamp(-1e6, 1e6),
            Item::MathLinB => m.math_lin.1 = (m.math_lin.1 + 0.1 * d as f32).clamp(-1e6, 1e6),
            Item::CurMode => {
                let all = [CursorMode::Off, CursorMode::Time, CursorMode::Volt, CursorMode::Track, CursorMode::Xy];
                let i = all.iter().position(|&x| x == m.cursor).unwrap();
                m.cursor = all[cyc(i, all.len(), d)];
            }
            Item::CurA => m.cur_a = (m.cur_a + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::CurB => m.cur_b = (m.cur_b + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::CurC => m.cur_c = (m.cur_c + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::CurD => m.cur_d = (m.cur_d + d as f32 * 0.005).clamp(0.0, 1.0),
            Item::RefSel => self.ref_sel = (self.ref_sel as i32 + d).rem_euclid(NREF as i32) as usize,
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
            Item::RefOffset => {
                if let Some(r) = &mut self.refs[self.ref_sel] {
                    r.offset += 0.1 * r.vpd * d as f32;
                }
            }
            Item::Persist => {
                m.persist = cyc(m.persist, PERSIST.len(), d);
                self.ph.clear();
            }
            Item::Intensity => m.intensity = (m.intensity as i32 + d * 5).clamp(10, 100) as u32,
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
            Item::Hdmi => m.hdmi_wave = !m.hdmi_wave,
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
            Item::GridBright => m.grid_bright = (m.grid_bright as i32 + 10 * d).clamp(10, 100) as u32,
            Item::GridFrame => m.grid_frame = !m.grid_frame,
            Item::TrigNoise => m.trig_nrej = !m.trig_nrej,
            Item::GridStyle => {
                let all = [Grid::Full, Grid::Light, Grid::None];
                let i = all.iter().position(|&x| x == m.grid).unwrap();
                m.grid = all[cyc(i, 3, d)];
            }
            Item::TouchLock => m.touch_lock = !m.touch_lock,
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
            Item::MathOffset => m.math_offset += d as f32 * 0.1,
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
            Item::DecOn => self.m.dec_on = !self.m.dec_on,
            Item::DecSel => {
                let k = (self.m.dec_sel as i32 + d).rem_euclid(DECODERS as i32) as usize;
                self.m.dec_select(k);
            }
            Item::PaneSel => {
                self.m.pane = (self.m.pane as i32 + d).rem_euclid(tables::PANES.len() as i32) as usize;
                if self.m.pane != 0 && self.pane_h() == 0 {
                    self.toast("No room for a second pane here: fewer LA lanes or the scope view");
                }
            }
            Item::TrendSel(k) => {
                let n = self.trend_options().len() as i32;
                self.m.trend_sel[k] = (self.m.trend_sel[k].min(n as usize - 1) as i32 + d).rem_euclid(n) as usize;
            }
            Item::DecSrc => self.m.dec_src = src_step(self.m.dec_src, NCH, d),
            Item::DecBaud => self.m.dec_baud = step(self.m.dec_baud, crate::decode::BAUDS.len(), d),
            Item::DecCanRate => self.m.dec_canrate = step(self.m.dec_canrate, crate::decode::CAN_RATES.len(), d),
            Item::TrigBusWhat => {
                let i = BUS_WHATS.iter().position(|&x| x == self.m.trig_bus).unwrap_or(0);
                self.m.trig_bus = BUS_WHATS[cyc(i, BUS_WHATS.len(), d)];
            }
            Item::TrigBusVal => self.m.trig_bus_val = (self.m.trig_bus_val as i64 + d as i64).clamp(0, 0x1fff_ffff) as u32,
            Item::DecCanH => self.m.dec_canh = !self.m.dec_canh,
            Item::DecThr => self.m.dec_thr += d as f32 * self.m.ch[self.m.dec_src].volts_per_div() / 25.0,
            Item::DecFmt => self.m.dec_ascii = !self.m.dec_ascii,
            Item::DecProto => {
                let i = PROTOS.iter().position(|&x| x == self.m.dec_proto).unwrap();
                self.set_dec_proto(PROTOS[cyc(i, PROTOS.len(), d)]);
            }
            Item::DecFdRate => self.m.dec_fdrate = cyc(self.m.dec_fdrate, crate::decode::FD_RATES.len(), d),
            Item::DecArate => self.m.dec_arate = cyc(self.m.dec_arate, crate::decode::ARINC_RATES.len(), d),
            Item::DecTick => self.m.dec_tick = step(self.m.dec_tick, crate::decode::SENT_TICKS.len(), d),
            Item::DecNibbles => self.m.dec_nibbles = (self.m.dec_nibbles as i32 + d).clamp(1, 6) as u32,
            Item::DecUsbSpeed => self.m.dec_uspeed = cyc(self.m.dec_uspeed, crate::decode::USB_SPEEDS.len(), d),
            Item::DecSrc4 => {
                let i = self.m.dec_src4.map_or(0, |c| c + 1);
                let k = opt_src_step(i, d);
                self.m.dec_src4 = if k == 0 { None } else { Some(k - 1) };
            }
            Item::DecDiff => {
                let i = self.m.dec_diff.map_or(0, |c| c + 1);
                let k = opt_src_step(i, d);
                self.m.dec_diff = if k == 0 { None } else { Some(k - 1) };
            }
            Item::DecSrc2 => self.m.dec_src2 = src_step(self.m.dec_src2, NCH, d),
            Item::DecCs => {
                let i = self.m.dec_cs.map_or(0, |c| c + 1);
                let k = opt_src_step(i, d);
                self.m.dec_cs = if k == 0 { None } else { Some(k - 1) };
            }
            Item::DecMode => self.m.dec_mode = cyc(self.m.dec_mode as usize, 4, d) as u8,
            Item::DecBits => {
                let (lo, hi) = if self.m.dec_proto == Proto::Uart { (5, 9) } else { (4, 32) };
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
            Item::XyMode => self.xy = !self.xy,
            Item::BootMode => self.boot_mode_toggle(),
            Item::Overclock => self.overclock_step(d),
            Item::ColorGrade => self.m.color_grade = !self.m.color_grade,
            Item::StoreSlot => self.store_slot = (self.store_slot as i32 + d).clamp(1, 9) as u32,
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
        self.settings_changed();
    }

    fn settings_changed(&mut self) {
        if self.m.run == RunState::Run {
            self.dirty_map = true;
        }
        self.mstat.clear();
    }

    fn activate(&mut self, it: Item) {
        if Self::kind(it) == Kind::Action && (self.ana_activate(it) || self.cal_activate(it) || self.bode_activate(it) || self.sim_activate(it) || self.afg_activate(it)) {
            return;
        }
        match Self::kind(it) {
            Kind::Choice => self.adjust(it, 1),
            Kind::Value => {}
            Kind::Action => match it {
                Item::Open(MenuId::La) if !self.m.la.on && !self.la_probe_ok() => self.toast("No LA probe connected"),
                Item::Open(id) => {
                    let p = self.menu;
                    if id == MenuId::Cursor && self.m.cursor == CursorMode::Off {
                        // like the CURSOR key
                        self.m.cursor = CursorMode::Time;
                    }
                    self.open_menu(id);
                    self.parent = Some(p);
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
                Item::Trig50 => self.level_50(),
                Item::PickOpt(k) if self.pick.map_or(false, |p| p.it == Item::TrigBusVal) => {
                    // keypad: digits edit the entry, OK sets the value and goes back
                    match KEYPAD.get(k).copied() {
                        Some("OK") => {
                            self.m.trig_bus_val = u32::from_str_radix(if self.keypad.is_empty() { "0" } else { &self.keypad }, 16).unwrap_or(0).min(0x1fff_ffff);
                            self.close_picker();
                        }
                        Some("Clear") => self.keypad.clear(),
                        Some("Del") => {
                            self.keypad.pop();
                        }
                        Some(d) if self.keypad.len() < 8 => {
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
                Item::PickOpt(k) if self.pick.map_or(false, |p| p.it == Item::WlanForgetNet) => {
                    self.close_picker();
                    self.wlan_forget_pick(k);
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
                Item::MeasAdd(k) => {
                    let e = (k, self.meas_src);
                    if !self.m.meas.contains(&e) {
                        self.meas_make_room();
                        self.m.meas.push(e);
                    }
                    if k == Meas::Burst && !(self.m.meas_idle > 0.0) {
                        if let Some((a, _)) = self.meas_analysis(self.meas_src) {
                            let p = a.period();
                            if p.is_finite() && p > 0.0 {
                                self.m.meas_idle = p;
                            }
                        }
                    }
                    self.toast(&format!("{} {} added", k.name(), self.wave_name(self.meas_src)));
                }
                Item::MeasPair(k) => self.meas_pair_toggle(k),
                Item::CntClear => self.counter_clear(),
                Item::DecTable => self.table_toggle(tables::TableKind::Decode),
                Item::DecExport => self.events_export(tables::TableKind::Decode),
                Item::SearchTable => self.table_toggle(tables::TableKind::Search),
                Item::SearchSave => self.events_export(tables::TableKind::Search),
                Item::StoreBin => match &self.last[0] {
                    Some(rec) => {
                        let p = crate::store::next_name(&self.save_dir(), "wave", "bin");
                        let r = crate::store::write(&p, &crate::store::wave_bin(rec, &self.m));
                        self.toast_saved(&p, r);
                    }
                    None => self.toast("No waveform to save"),
                },
                Item::RefExport => self.ref_export(),
                Item::RefImport => self.ref_import(),
                Item::MaskSave => self.mask_save(),
                Item::MaskLoad => self.mask_load(),
                Item::ChLabel(c) => self.wlan.osk = Some(wlan::Osk::for_label(c, &self.m.ch_label[c])),
                Item::RefLabel => match &self.refs[self.ref_sel] {
                    Some(r) => self.wlan.osk = Some(wlan::Osk::for_ref(self.ref_sel, &r.label)),
                    None => self.toast("That slot is empty"),
                },
                Item::ChBiasZero(c) => self.zero_input(c),
                Item::MathExprEdit => self.wlan.osk = Some(wlan::Osk::for_expr(self.m.math_sel, &self.m.math_expr[self.m.math_sel])),
                Item::MeasClear => {
                    self.m.meas.clear();
                    self.m.meas2.clear();
                    self.mstat.clear();
                }
                Item::RefSave => {
                    let c = self.ref_src;
                    let rec = self.last[0].clone();
                    let saved = rec.as_ref().filter(|r| !r.ch[c].is_empty()).map(|r| {
                        let (v, dt) = self.meas_wave(r, c);
                        let t0 = if v.len() > r.ch[c].len() { r.raw.as_ref().map(|x| x.t0).unwrap_or(r.t0) } else { r.t0 };
                        RefWave { src: c, color: 0, label: String::new(), t0, dt, v, vpd: self.m.ch[c].volts_per_div(), offset: self.m.ch[c].offset, on: true }
                    });
                    match saved {
                        Some(w) => {
                            let slot = self.refs.iter().position(|r| r.is_none()).unwrap_or(NREF - 1);
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
                        self.info.clone(),
                        p.ident(),
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
                Item::ZoneBox | Item::ZoneShape => {
                    self.zone_arm = Some((self.zone_sel, it == Item::ZoneShape));
                    self.close_menu();
                    self.toast(if it == Item::ZoneShape { "Draw the zone with one finger" } else { "Drag a box for the zone" });
                }
                Item::ZoneClear => {
                    self.m.zones[self.zone_sel] = crate::zone::Zone { src: self.m.zones[self.zone_sel].src, ..Default::default() };
                }
                Item::StoreSaveSetup => {
                    let p = self.setup_path();
                    let r = crate::store::write(&p, crate::store::setup_text(&self.m).as_bytes());
                    self.toast_saved(&p, r);
                }
                Item::StoreRecall => self.recall_setup(),
                Item::StoreCsv => match &self.last[0] {
                    Some(rec) => {
                        let p = crate::store::next_name(&self.save_dir(), "wave", "csv");
                        let mut r = crate::store::write(&p, crate::store::wave_csv(rec, &self.m).as_bytes());
                        // the LA lines go to their own file: another sample rate, and only the changes
                        if let (Ok(()), Some(la), true) = (&r, &rec.la, self.m.la_on()) {
                            let q = crate::store::next_name(&self.save_dir(), "la", "csv");
                            r = crate::store::write(&q, crate::store::la_csv(la, &self.m.la.lines()).as_bytes());
                        }
                        self.toast_saved(&p, r);
                    }
                    None => self.toast("No waveform to save"),
                },
                Item::StorePng => {
                    // without the dialog: the screen is saved after the next frame (main loop / script runner)
                    let p = crate::store::next_name(&self.save_dir(), "shot", "png");
                    self.close_menu();
                    self.shot_request = Some(p.to_string_lossy().into_owned());
                }
                Item::MaskCreate => self.create_mask(),
                Item::MaskFromRef => self.create_mask_from_ref(),
                Item::MaskClear => {
                    self.mask = None;
                    self.mask_ref = None;
                    self.m.mask_on = false;
                    self.mask_pass = 0;
                    self.mask_fail = 0;
                    self.toast("Mask cleared");
                }
                Item::MaskReset => {
                    self.mask_pass = 0;
                    self.mask_fail = 0;
                }
                Item::StoreFfSave => self.ff_save(),
                Item::StoreFfLoad => self.ff_load(),
                Item::Stock => self.stock_tap(),
                Item::Update => self.dialog = Some(self.update_lines()),
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
            crate::store::StoreDest::Desk => {
                if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) {
                    crate::store::desktop_dir()
                } else {
                    self.store_dir.join("Desktop")
                }
            }
            crate::store::StoreDest::Auto => {
                if self.usb_used() { std::path::PathBuf::from(crate::store::USB_DIR) } else { self.store_dir.clone() }
            }
        }
    }

    /// For the message after saving.
    pub fn saved_where(&self) -> &'static str {
        match self.store_dest {
            crate::store::StoreDest::Desk => " on the desktop",
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
        let dir = if self.store_dir == std::path::Path::new(crate::store::DEFAULT_DIR) && keep.is_dir() { keep } else { self.store_dir.as_path() };
        dir.join(format!("setup{}.txt", self.store_slot))
    }

    /// Loads setup<slot>.txt of the storage directory; the acquisition state (run / stop) stays as it is.
    fn recall_setup(&mut self) {
        let p = self.setup_path();
        let parsed = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| crate::store::parse_setup(&t));
        match parsed {
            Ok(mut m) => {
                (m.run, m.status) = (self.m.run, self.m.status);
                if m.mask_src != self.m.mask_src {
                    self.mask = None;
                    self.mask_ref = None;
                }
                self.m = m;
                self.math_fam = if self.m.math == MathOp::Off { MathFamily::Arith } else { self.m.math.family() };
                if self.mask.is_none() {
                    self.m.mask_on = false;
                }
                self.build_mask();
                self.clamp_zoom();
                self.relayout();
                self.dirty_map = true;
                self.mstat.clear();
                self.toast(&format!("Recalled {}", crate::store::file_name(&p)));
            }
            Err(e) => self.toast(&format!("Recall failed: {}", e)),
        }
    }

    fn level_50(&mut self) {
        let s = self.stats[self.m.trig_src];
        if s.valid {
            self.m.trig_level = (s.vmax + s.vmin) / 2.0;
            self.settings_changed();
        }
    }

    fn open_menu(&mut self, id: MenuId) {
        self.menu = id;
        self.menu_open = true;
        self.parent = None;
        self.pick = None;
        self.focus = 0;
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
        if let Some(o) = self.meas_options(it).or_else(|| self.ana_options(it)).or_else(|| self.magnetic_options(it)).or_else(|| self.inverter_options(it)).or_else(|| self.dpt_options(it)).or_else(|| self.cal_options(it)).or_else(|| self.bode_options(it)).or_else(|| self.sim_options(it)).or_else(|| self.offline_options(it)).or_else(|| self.afg_options(it)).or_else(|| self.bw_options(it)).or_else(|| self.la_options(it)).or_else(|| self.win_options(it)) {
            return o;
        }
        let v: &[&str] = match it {
            Item::ChOn(_) | Item::ChInvert(_) | Item::MeasStats | Item::MeasLabels | Item::Zoom | Item::AutoRoll | Item::DecOn | Item::SearchOn | Item::ColorGrade | Item::MaskOn | Item::MaskInvert => &["Off", "On"],
            Item::MaskSrc => return ch_names(),
            Item::MaskAction => &["None", "Stop"],
            Item::ChCoupling(_) => return board().couplings().iter().map(|c| c.name().to_string()).collect(),

            Item::ChProbe(_) => &["1x", "10x", "100x"],
            Item::TrigSrc | Item::DecSrc | Item::RefSrc => return ch_names(),
            Item::MeasSrc | Item::MeasSrcB => return self.wave_names(),
            Item::MathA | Item::MathB => return (0..NCH + self.m.math_sel).filter(|&s| src_ok(s)).map(src_name).collect(),
            Item::MathSel => return (0..NMATH).map(|k| format!("M{}", k + 1)).collect(),
            Item::TrigSlope => &["Rising", "Falling", "Either"],
            Item::TrigMode => &["Auto", "Normal", "Single"],
            Item::TrigSrc2 => return ch_names(),
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
            Item::DecProto => return PROTOS.iter().map(|p| p.name().to_string()).collect(),
            Item::DecFdRate => &["1 M", "2 M", "4 M", "5 M", "8 M"],
            Item::DecArate => &ARATE_NAMES,
            Item::DecTick => return crate::decode::SENT_TICKS.iter().map(|&t| tick_name(t)).collect(),
            Item::DecNibbles => &["1", "2", "3", "4", "5", "6"],
            Item::DecUsbSpeed => &USPEED_NAMES,
            Item::DecSrc4 | Item::DecDiff => return opt_ch_names("None"),
            Item::DecCanH => &["RX/TX", "CAN_H"],
            Item::TrigBusWhat => &["Frame", "ID/Addr", "Data", "Error"],
            Item::DecSrc2 => return ch_names(),
            Item::DecCs => return opt_ch_names("None"),
            Item::DecMode => &["0", "1", "2", "3"],
            Item::DecParity => &["None", "Even", "Odd"],
            Item::AcqMode => &["Normal", "Peak", "Average", "High res"],
            Item::CurMode => &["Off", "Time", "Volt", "Track", "XY"],
            Item::GridStyle => &["Full", "Light", "None"],
            Item::GridFrame | Item::TrigNoise => &["Off", "On"],
            Item::Dots | Item::ZoneOn => &["Off", "On"],
            Item::Hdmi => &["Mirror", "Waveform"],
            Item::ZoneSel => &["Z1", "Z2"],
            Item::ZoneSrc => return ch_names(),
            Item::ZoneMode => &["Intersect", "Not intersect"],
            Item::ZInput => return opt_ch_names("Off"),
            Item::Histogram => &["Off", "Vertical", "Horizontal"],
            Item::TouchLock => &["On", "Locked"],
            Item::DecFmt => &["Hex", "ASCII"],
            Item::XyMode => &["YT", "XY"],
            Item::DecSel => &["D1", "D2", "D3", "D4"],
            Item::PaneSel => &tables::PANES,
            Item::TrendSel(_) => return self.trend_options(),
            Item::DvmSrc | Item::CntSrc => return (0..n_ch()).map(|c| format!("CH{}", c + 1)).collect(),
            Item::DvmOn | Item::CntOn | Item::ChFine | Item::HFine | Item::LogOn => &["Off", "On"],
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
            Item::AcqSeg => return SEGMENTS.iter().map(|&n| if n == 0 { "Off".into() } else { n.to_string() }).collect(),
            Item::DecBaud => return crate::decode::BAUDS.iter().map(|b| b.to_string()).collect(),
            Item::DecCanRate => return crate::decode::CAN_RATES.iter().map(|&b| si(b, "bit/s")).collect(),
            // the value picker is a hex keypad
            Item::TrigBusVal => return KEYPAD.iter().map(|k| k.to_string()).collect(),
            Item::AfgFreqEntry => return FREQ_KEYS.iter().map(|k| k.to_string()).collect(),
            Item::WlanNet => return self.wlan_net_options(),
            Item::WlanForgetNet => return self.wlan_forget_options(),
            Item::RefImport => return self.ref_files.iter().map(|p| crate::store::file_name(p)).collect(),
            Item::QuickAct => return QUICKS.iter().map(|q| q.name().to_string()).collect(),
            Item::Overclock => return board().clock_steps().iter().map(|&hz| overclock::label(hz)).collect(),
            _ => &[],
        };
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Control of a menu row whose control rectangle is `w` px wide: segmented buttons when all options fit, a
    /// picker (- value +, the value opens the list of all options) for longer option lists, else - value +.
    fn ctl(&self, it: Item, w: i32) -> Ctl {
        // - and + step, the value opens the keypad (`open_picker`)
        if it == Item::AfgFreq {
            return Ctl::Pick;
        }
        if let Item::RefShow(_) = it {
            return Ctl::Button;
        }
        if matches!(it, Item::MathExprI | Item::LogInfo | Item::CalStatus | Item::NetEth | Item::NetMac | Item::NetWeb | Item::NetPorts | Item::NetWlan) {
            return Ctl::Text;
        }
        match Self::kind(it) {
            Kind::Action => Ctl::Button,
            k => {
                let opts = self.options(it);
                let n = opts.len() as i32;
                let f = &self.fonts;
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
        let it = if it == Item::AfgFreq { Item::AfgFreqEntry } else { it };
        if it == Item::TrigBusVal {
            self.keypad = format!("{:X}", self.m.trig_bus_val);
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
            MenuId::Main => (Self::MENU_BTN.x, false),
            MenuId::Network => (Self::NET_BOX.x + Self::NET_BOX.w / 2, true),
            MenuId::ViewMore => (W, true),
            MenuId::La | MenuId::LaLines | MenuId::LaPat => self.aux_cards().0.map_or((W / 2, false), |r| (r.x + r.w / 2, false)),
            MenuId::Afg => self.aux_cards().1.map_or((W / 2, false), |r| (r.x + r.w / 2, false)),
            MenuId::Utility => self.top_buttons().iter().find(|t| t.0 == TopBtn::System).map_or((W / 2, false), |t| (t.2.x + t.2.w / 2, true)),
            // menus of front-panel keys (also opened from the start menu): centred above the cards
            m => self.top_buttons().iter().find(|t| t.0 == TopBtn::Menu(m)).map_or((W / 2, false), |t| (t.2.x + t.2.w / 2, true)),
        }
    }

    fn dlg_layout(&self) -> Option<Dlg> {
        if !self.menu_open {
            return None;
        }
        if self.menu == MenuId::Main {
            return Some(self.main_layout());
        }
        let items = self.items(self.menu);
        let n = items.len() as i32;
        let grid = items.iter().all(|&it| Self::kind(it) == Kind::Action);
        let wide = self.pick.is_some_and(|p| matches!(p.it, Item::WlanNet | Item::WlanForgetNet));
        let (cols, cw, rh, gap) = if wide { (2, 400, DLG_ROW + 2, 8) } else if self.menu == MenuId::ViewMore { (1, 190, DLG_ROW + 2, 8) } else if grid && self.menu == MenuId::MeasAdd {
            if n > 54 { (7, 130, DLG_ROW + 2, 8) } else { (6, 150, DLG_ROW + 2, 8) }
        } else if grid { (4, 150, DLG_ROW + 2, 8) } else if n > 5 { (2, 400, DLG_ROW, 16) } else { (1, 400, DLG_ROW, 16) };
        let nrows = (n + cols - 1) / cols;
        let w = 24 + cols * cw + (cols - 1) * gap;
        let h = DLG_HEAD + nrows * rh + 6;
        let (ax, top) = self.menu_anchor();
        let x = (ax - w / 2).clamp(6, W - 6 - w);
        let y = if top { TOP.h + 4 } else { (BOTTOM.y - 4 - h).max(TOP.h + 4) };
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
            let label = Rect::new(cx, ry, 96, DLG_CTRL);
            let ctrl = if Self::kind(it) == Kind::Action { Rect::new(cx, ry, cw, DLG_CTRL) } else { Rect::new(cx + 100, ry, cw - 100, DLG_CTRL) };
            rows.push(Row { it, label, ctl: self.ctl(it, ctrl.w), ctrl });
        }
        Some(Dlg { r, close, back, rows, heads: Vec::new() })
    }

    /// Start menu: one row of up to 6 buttons per group, the group label left of it; above the Menu button.
    fn main_layout(&self) -> Dlg {
        let (lw, cw, gap) = (104, 140, 6);
        let groups = self.main_groups();
        // six buttons per line; a longer group continues on the next line (label on its first line only)
        const PER_LINE: usize = 6;
        let lines: usize = groups.iter().map(|g| g.1.len().div_ceil(PER_LINE).max(1)).sum();
        let w = 24 + lw + PER_LINE as i32 * cw + (PER_LINE as i32 - 1) * gap;
        let h = DLG_HEAD + lines as i32 * (DLG_ROW + 2) + 6;
        let r = Rect::new(6, BOTTOM.y - 4 - h, w, h);
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

    /// Value control parts: minus, value, plus.
    fn value_parts(c: Rect) -> (Rect, Rect, Rect) {
        (Rect::new(c.x, c.y, 44, c.h), Rect::new(c.x + 48, c.y, c.w - 96, c.h), Rect::new(c.x + c.w - 44, c.y, 44, c.h))
    }

    fn draw_menu(&self, cv: &mut Canvas) {
        let Some(d) = self.dlg_layout() else { return };
        let f = &self.fonts;
        cv.round(d.r, 8, ACCENT, 255);
        cv.round(d.r.inset(2), 7, DLG_BG, 255);
        let tcol = match self.menu_root() {
            MenuId::Channel(c) => CH_COLORS[c],
            _ => TEXT,
        };
        // long titles (storage directory) keep their end
        let mut title = self.menu_title(self.menu);
        let room = d.back.unwrap_or(d.close).x - d.r.x - 30;
        while f.mb.width(&title) > room && title.chars().count() > 2 {
            let mut c = title.chars();
            c.next();
            c.next();
            title = format!("\u{2026}{}", c.as_str());
        }
        cv.text(&f.mb, d.r.x + 14, d.r.y + 31, &title, tcol);
        btn(cv, d.close, BTN, BTN_BORDER);
        cv.text_in(&f.mb, d.close, 1, "\u{d7}", TEXT);
        if let Some(b) = d.back {
            btn(cv, b, BTN, BTN_BORDER);
            cv.text_in(&f.sb, b, 1, "Back", TEXT);
        }
        for (r, name) in &d.heads {
            cv.text_in(&f.sb, *r, 0, name, DIM);
        }
        for (i, row) in d.rows.iter().enumerate() {
            let focus = i == self.focus;
            let kind = Self::kind(row.it);
            if row.label.w > 0 && kind != Kind::Action {
                cv.text_in(&f.s, row.label, 0, self.item_label(row.it), if focus { ACCENT } else { LABEL });
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
                            cv.text_in(&f.mb, r, 1, o, DARK_TEXT);
                        } else {
                            btn(cv, r, BTN, BTN_BORDER);
                            cv.text_in(&f.m, r, 1, o, TEXT);
                        }
                    }
                }
                Ctl::Text => cv.text_in(&f.m, c, 1, &self.value(row.it), TEXT),
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
                        None if kind == Kind::Action => self.item_label(row.it).to_string(),
                        None => self.value(row.it),
                    };
                    if added {
                        cv.round(c, 5, ACCENT, 255);
                        cv.text_in(&f.mb, c, 1, &txt, DARK_TEXT);
                    } else {
                        btn(cv, c, BTN, if focus { ACCENT } else { BTN_BORDER });
                        cv.text_in(&f.mb, c, 1, &txt, TEXT);
                    }
                    if let Item::Open(_) = row.it {
                        chevron(cv, c.x + c.w - 14, c.y + c.h / 2, 1, DIM);
                    }
                }
            }
            if focus {
                if let Ctl::Seg(..) = row.ctl {
                    cv.round_outline(Rect::new(c.x - 3, c.y - 3, c.w + 6, c.h + 6), 7, ACCENT);
                }
            }
        }
    }

    /// Tap inside the menu dialog; false when (x, y) is outside it.
    fn tap_menu(&mut self, x: i32, y: i32) -> bool {
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
                    self.menu = self.parent.take().unwrap_or(MenuId::Main);
                    self.focus = 0;
                }
                return true;
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
                    let (mi, _, pl) = Self::value_parts(row.ctrl);
                    if mi.contains(x, y) {
                        self.adjust(row.it, -1);
                    } else if pl.contains(x, y) {
                        self.adjust(row.it, 1);
                    } else if let Ctl::Pick = row.ctl {
                        self.open_picker(row.it);
                    }
                }
                Ctl::Button => self.activate(row.it),
            }
            break;
        }
        true
    }

    fn close_menu(&mut self) {
        self.menu_open = false;
        self.menu = MenuId::Main;
        self.parent = None;
        self.pick = None;
    }

    /// Power dialog box and its buttons.
    const PWR_BOX: Rect = Rect::new(W / 2 - 250, H / 2 - 100, 500, 200);
    const PWR_OFF: Rect = Rect::new(W / 2 - 230, H / 2 - 10, 140, 64);
    const PWR_RESTART: Rect = Rect::new(W / 2 - 70, H / 2 - 10, 140, 64);
    const PWR_CANCEL: Rect = Rect::new(W / 2 + 90, H / 2 - 10, 140, 64);

    fn draw_overlay(&self, cv: &mut Canvas) {
        let f = &self.fonts;
        self.draw_osk(cv);
        if self.power_dlg {
            cv.blend(Rect::new(0, 0, W, H), rgb(0, 0, 0), 160);
            let r = Self::PWR_BOX;
            cv.round(r, 8, ACCENT, 255);
            cv.round(r.inset(2), 7, DLG_BG, 255);
            cv.text_in(&f.lb, Rect::new(r.x, r.y + 18, r.w, 28), 1, "Power", ACCENT);
            cv.text_in(&f.m, Rect::new(r.x, r.y + 52, r.w, 24), 1, "Power key again or tap outside to cancel", DIM);
            for (b, t, c) in [(Self::PWR_OFF, "Power off", RED), (Self::PWR_RESTART, "Restart", ACCENT), (Self::PWR_CANCEL, "Cancel", TEXT)] {
                btn(cv, b, mix(DLG_BG, c, 40), c);
                cv.text_in(&f.mb, b, 1, t, c);
            }
        }
        if self.dialog.is_some() && self.sysinfo.is_some() {
            self.draw_sysinfo(cv);
        } else if let Some(lines) = &self.dialog {
            cv.blend(Rect::new(0, 0, W, H), rgb(0, 0, 0), 140);
            // taller for long texts (About with experimental limits)
            let h = (40 + lines.len() as i32 * 30).clamp(260, H - 20);
            let r = Rect::new(W / 2 - 240, H / 2 - h / 2, 480, h);
            cv.round(r, 8, ACCENT, 255);
            cv.round(r.inset(2), 7, DLG_BG, 255);
            for (i, l) in lines.iter().enumerate() {
                let fnt = if i == 0 { &f.lb } else { &f.m };
                cv.text_in(fnt, Rect::new(r.x, r.y + 20 + i as i32 * 30, r.w, 26), 1, l, if i == 0 { ACCENT } else { TEXT });
            }
        }
    }

    // ------------------------------------------------------------------ input

    /// A menu is on the screen (the desktop window turns Multi 1 with the wheel only then).
    pub fn menu_is_open(&self) -> bool {
        self.menu_open
    }

    pub fn handle(&mut self, ev: Ev) {
        if self.start_up_input(&ev) {
            return;
        }
        match ev {
            Ev::Key(k) => self.key(k),
            Ev::Touch { id, phase, x, y } => self.multi_touch(id, phase, x, y),
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

    pub fn key(&mut self, k: u8) {
        if k == 0 {
            return; // release
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
                m.trig_mode = TrigMode::Single;
                m.run = RunState::Run;
                self.dirty_map = true;
                if m.n_on() > 0 || m.la_on() {
                    m.floor_live_tdiv();
                }
            }
            key::AUTO => {
                if self.src.is_hw() {
                    self.auto_start();
                } else {
                    autoset(&mut self.m, &self.src);
                    self.dirty_map = true;
                    self.toast("Auto set-up");
                }
            }
            key::CLEAR => self.clear_display(),
            key::DEFAULT => {
                let meas = Self::default_meas();
                // the Quick key action is a preference, not part of the set-up that Default resets
                let quick = m.quick;
                self.m = Model::default();
                self.m.meas = meas;
                self.m.quick = quick;
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
                if self.m.run == RunState::Stop {
                    self.m.run = RunState::Run;
                }
                self.toast("Force trigger");
            }
            key::MEASURE => self.toggle_menu(MenuId::Measure),
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
            0x08 => self.history_step(-1),
            0x06 => self.history_step(1),
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
    fn multi_touch(&mut self, id: i32, ph: Phase, x: i32, y: i32) {
        if self.m.touch_lock {
            if id == 0 {
                self.touch_ev(ph, x, y);
            }
            return;
        }
        let p = &mut self.pinch;
        if id == 1 {
            match ph {
                Phase::Down => {
                    if self.touch.down && self.grid.contains(x, y) {
                        p.active = true;
                        p.p0 = (self.touch.x, self.touch.y);
                        p.p1 = (x, y);
                        p.ref_dx = (p.p1.0 - p.p0.0).abs();
                        p.ref_dy = (p.p1.1 - p.p0.1).abs();
                        // cancel the one-finger drag
                        self.touch.drag = Drag::None;
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
        if dt != 0 {
            self.adjust(Item::HScale, dt);
        }
        if dv != 0 {
            let c = self.m.active;
            self.adjust(Item::ChScale(c), dv);
        }
    }

    /// A finger in a split plot works in the window it went down in: its channels, its rectangle.
    fn touch_ev(&mut self, ph: Phase, x: i32, y: i32) {
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
        let in_plot = ph == Phase::Down && self.grid.contains(x, y) && !self.dlg_layout().map_or(false, |d| d.r.contains(x, y));
        let t = &mut self.touch;
        match ph {
            Phase::Down => {
                let sd = if self.m.zoom { self.m.zdelay } else { self.m.delay };
                *t = TouchState { down: true, x0: x, y0: y, x, y, drag: Drag::None, start_offset: 0.0, start_delay: sd, start_level: self.m.trig_level, start_cur: 0.0, win: self.cur_win };
                if self.m.touch_lock || self.dialog.is_some() || self.power_dlg || self.wlan.osk.is_some() {
                    return;
                }
                if in_plot {
                    t.drag = Drag::Pending;
                    if let Some((k, _)) = self.zone_arm {
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
                    let cur_hit = |frac: f32, vertical: bool| self.shows(m.active) && cur_hit(frac, vertical);
                    let drag = if m.cursor == CursorMode::Time && cur_hit(m.cur_a, true) {
                        Drag::CursorA
                    } else if m.cursor == CursorMode::Time && cur_hit(m.cur_b, true) {
                        Drag::CursorB
                    } else if m.cursor == CursorMode::Volt && cur_hit(m.cur_a, false) {
                        Drag::CursorA
                    } else if m.cursor == CursorMode::Volt && cur_hit(m.cur_b, false) {
                        Drag::CursorB
                    } else if near_right && dy.abs() >= dx.abs() && self.shows(m.trig_src) {
                        Drag::Level
                    } else if dy.abs() > dx.abs() {
                        // nearest enabled channel marker when starting at the left edge, else the active channel (in a
                        // split plot: of this window)
                        let mut c = if self.shows(m.active) { m.active } else { (0..NCH).find(|&i| m.ch[i].on && self.shows(i)).unwrap_or(m.active) };
                        if near_left {
                            let mut best = i32::MAX;
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
                        Drag::Offset(c)
                    } else {
                        Drag::Delay
                    };
                    let start_offset = match drag {
                        Drag::Offset(c) => self.m.ch[c].offset,
                        _ => 0.0,
                    };
                    let start_cur = match drag {
                        Drag::CursorA => self.m.cur_a,
                        Drag::CursorB => self.m.cur_b,
                        _ => 0.0,
                    };
                    if let Drag::Offset(c) = drag {
                        self.m.active = c;
                    }
                    let t = &mut self.touch;
                    t.drag = drag;
                    t.start_offset = start_offset;
                    t.start_cur = start_cur;
                }
                let t = &self.touch;
                let (dx, dy) = (x - t.x0, y - t.y0);
                let ppx = self.area.w as f32 / HDIV;
                let ppy = self.area.h as f32 / VDIV;
                match t.drag {
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
                    Drag::Level => {
                        let vpd = self.m.ch[self.m.trig_src].volts_per_div();
                        self.m.trig_level = t.start_level - dy as f32 / ppy * vpd;
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
                    Drag::CursorA | Drag::CursorB => {
                        let v = if self.m.cursor == CursorMode::Time {
                            t.start_cur + dx as f32 / self.area.w as f32
                        } else {
                            t.start_cur + dy as f32 / self.area.h as f32
                        }
                        .clamp(0.0, 1.0);
                        if t.drag == Drag::CursorA {
                            self.m.cur_a = v
                        } else {
                            self.m.cur_b = v
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                // Up
                let was = (t.down, t.drag, t.x0, t.y0);
                t.down = false;
                t.drag = Drag::None;
                if !was.0 {
                    return;
                }
                if let Drag::Zone(k) = was.1 {
                    self.finish_zone(k);
                    return;
                }
                if self.dialog.is_some() {
                    self.dialog = None;
                    return;
                }
                if self.m.touch_lock {
                    // the Menu button ("Locked") is the one touch target left: it unlocks
                    if Self::MENU_BTN.contains(was.2, was.3) {
                        self.key(key::TOUCHLOCK);
                    }
                    return;
                }
                if was.1 == Drag::None || was.1 == Drag::Pending {
                    if (x - was.2).abs() < 16 && (y - was.3).abs() < 16 {
                        self.tap(was.2, was.3);
                    } else {
                        self.pane_swipe(was.2, was.3, y);
                    }
                }
            }
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
        let (ppx, ppy) = (a.w as f64 / HDIV as f64, a.h as f64 / VDIV as f64);
        let to_wave = |(x, y): (i32, i32)| {
            let t = (x - (a.x + a.w / 2)) as f64 / ppx * tdiv as f64 + delay as f64;
            let v = ((a.y + a.h / 2) - y) as f64 / ppy * ch.volts_per_div() as f64 - ch.offset as f64;
            (t, v)
        };
        let ok = if shape { path.len() >= 3 } else { path.len() == 2 && (path[0].0 - path[1].0).abs() > 4 && (path[0].1 - path[1].1).abs() > 4 };
        if !ok {
            self.toast("Zone too small");
            return;
        }
        let z = &mut self.m.zones[k];
        z.pts = path.into_iter().map(to_wave).collect();
        z.on = true;
        self.toast(&format!("Zone {} set", k + 1));
    }

    /// Zones (green = must intersect, red = must not) and the zone being drawn.
    fn draw_zones(&self, cv: &mut Canvas) {
        let a = self.area;
        let (tdiv, delay) = self.view();
        let (ppx, ppy) = (a.w as f64 / HDIV as f64, a.h as f64 / VDIV as f64);
        cv.set_clip(a);
        for (k, z) in self.m.zones.iter().enumerate() {
            if z.pts.len() < 2 || !self.shows(z.src) {
                continue;
            }
            let ch = &self.m.ch[z.src];
            let px = |(t, v): (f64, f64)| {
                let x = a.x as f64 + a.w as f64 / 2.0 + (t - delay as f64) / tdiv as f64 * ppx;
                let y = a.y as f64 + a.h as f64 / 2.0 - (v + ch.offset as f64) / ch.volts_per_div() as f64 * ppy;
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
            cv.text(&self.fonts.s, lx + 4, ly + 14, &format!("Z{}", k + 1), col);
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
        if self.power_dlg {
            self.power_dlg = false;
            if (Self::PWR_OFF.contains(x, y) || Self::PWR_RESTART.contains(x, y)) && self.update_busy() {
                // a cut now leaves recovery half-written (docs/update.md)
                self.toast("Update running: wait until it is done");
            } else if Self::PWR_OFF.contains(x, y) {
                self.power_action = Some(PowerAction::Off);
                self.toast("Powering off ...");
            } else if Self::PWR_RESTART.contains(x, y) {
                self.power_action = Some(PowerAction::Restart);
                self.toast("Restarting ...");
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
        if self.auto_bar_tap(x, y) {
            return;
        }
        if self.full_btn().1.contains(x, y) {
            self.full = !self.full;
            return;
        }
        if self.view_on() && self.view_btn().contains(x, y) {
            return self.toggle_menu(MenuId::ViewMore);
        }
        if !self.menu_open && self.m.view == AView::Eye && !self.xy && self.eye_tap(x, y) {
            return;
        }
        if self.table_tap(x, y) || self.pane_tap(x, y) {
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
        if Self::top_hit(Self::RUN_PILL).contains(x, y) {
            if self.offline {
                self.toggle_offline_run();
                return;
            }
            return self.key(key::RUNSTOP);
        }
        if Self::top_hit(Self::NET_BOX).contains(x, y) {
            return self.toggle_menu(MenuId::Network);
        }
        if self.wlan_stick && Self::top_hit(Self::WLAN_BOX).contains(x, y) {
            return self.toggle_menu(MenuId::Network);
        }
        if self.usb && Self::top_hit(self.usb_box()).contains(x, y) {
            return self.toggle_menu(MenuId::Storage);
        }
        // sample rate, depth and mode in the top bar: the page that sets them
        let end = self.top_buttons().first().map_or(self.top_right(), |b| b.2.x) - 4;
        if Self::top_hit(Rect::new(Self::RUN_PILL.x + Self::RUN_PILL.w, Self::RUN_PILL.y, end - Self::RUN_PILL.x - Self::RUN_PILL.w, Self::RUN_PILL.h)).contains(x, y) {
            return self.toggle_menu(if self.offline { MenuId::Ref } else { MenuId::Acquire });
        }
        for (b, _, r) in self.top_buttons() {
            if Self::top_hit(r).contains(x, y) {
                return match b {
                    TopBtn::Menu(id) => self.toggle_menu(id),
                    TopBtn::System => self.toggle_menu(MenuId::Utility),
                };
            }
        }
        if Self::MENU_BTN.contains(x, y) {
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
        if tr.contains(x, y) {
            // the zoom/delay line: tap it to toggle zoom without opening Horizontal
            if y >= tr.y + tr.h / 2 {
                let on = !self.m.zoom;
                self.set_zoom(on);
                return;
            }
            return self.toggle_menu(MenuId::Horizontal);
        }
        if trg.contains(x, y) {
            if self.offline {
                return self.toggle_menu(MenuId::Ref);
            }
            // the buttons are low: each takes its half of the card for the finger
            if x >= Self::trig_parts(trg).0.x - 6 {
                return self.adjust(if y < trg.y + trg.h / 2 { Item::TrigSrc } else { Item::TrigMode }, 1);
            }
            return self.toggle_menu(MenuId::Trigger);
        }
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
    time: Rect,
    trig: Rect,
}

/// Width of the LA and generator cards, and the width a channel card keeps before they stay narrow.
const AUX_NARROW: i32 = 56;
const AUX_WIDE: i32 = 104;
const CH_MIN: i32 = 100;
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

fn backed_text(cv: &mut Canvas, f: &Font, x: i32, y: i32, s: &str, col: Rgb) -> i32 {
    cv.blend(Rect::new(x - 3, y - f.size + 2, f.width(s) + 6, f.size + 2), BG, 180);
    cv.text(f, x, y, s, col)
}

/// Button: 2 px border, filled.
fn btn(cv: &mut Canvas, r: Rect, fill: Rgb, border: Rgb) {
    cv.round(r, 5, border, 255);
    cv.round(r.inset(2), 4, fill, 255);
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
    let hex = |v: u32| if m.dec_bits > 8 { format!("{:04X}", v) } else { format!("{:02X}", v) };
            match p.kind {
                Kind::Addr { read } => format!("{} {}{}", hex(p.value), if read { "R" } else { "W" }, if p.ok { "" } else { " N" }),
                // ARINC 429 label in octal; SENT tick and data nibbles
                Kind::Id { .. } if m.dec_proto == Proto::Arinc429 => format!("L {:03o}", p.value),
                Kind::Field("SYNC") if m.dec_proto == Proto::Sent => format!("SYNC {}{}", si(p.value as f64 * 1e-9, "s"), if p.ok { "" } else { " !" }),
                Kind::Data if m.dec_proto == Proto::Sent => format!("{:0w$X}", p.value, w = p.value2.unwrap_or(1) as usize),
                // USB: the speed of the SYNC, PID names, bytes
                Kind::Field("SYNC") if m.dec_proto == Proto::Usb => format!("SYNC {}{}", if p.value >= 12000 { "FS" } else { "LS" }, if p.ok { "" } else { " !" }),
                Kind::Field("PID") if m.dec_proto == Proto::Usb && p.ok => oos_dsp::decode_ext3::usb_pid_name(p.value as u8).to_string(),
                Kind::Field("PID") if m.dec_proto == Proto::Usb => format!("PID {:02X} !", p.value),
                Kind::Data if m.dec_proto == Proto::Usb => format!("{:02X}", p.value),
                Kind::Id { ext, rtr } => format!("ID {}{}{}", if ext { format!("{:08X}", p.value) } else if m.dec_proto == Proto::Lin { format!("{:02X}", p.value) } else { format!("{:03X}", p.value) }, if rtr { " RTR" } else { "" }, if p.ok { "" } else { " !P" }),
                Kind::Sample { right } => format!("{} {:0w$X}", if right { "R" } else { "L" }, p.value, w = (m.dec_bits as usize).div_ceil(4)),
                Kind::Word { cmd } => format!("{} {:04X}{}", if cmd { "C/S" } else { "D" }, p.value, if p.ok { "" } else { " !P" }),
                Kind::Crc => format!("{} {:X}{}", if m.dec_proto == Proto::Lin { "CS" } else { "CRC" }, p.value, if p.ok { "" } else { " !" }),
                Kind::Field(l) if matches!(l, "MAC" | "SRC") && p.value2.is_some() => {
                    format!("{} {}{}", if l == "SRC" { "SRC" } else { "MAC" }, oos_dsp::linecode::mac_text(p.value, p.value2.unwrap_or(0)), if p.ok { "" } else { " !" })
                }
                Kind::Field(l) if matches!(l, "IP" | "DIP") => format!("{} {}{}", if l == "DIP" { "DIP" } else { "IP" }, oos_dsp::linecode::ip_text(p.value), if p.ok { "" } else { " !" }),
                Kind::Field(l) => match p.value2 {
                    Some(v2) if matches!(l, "DST" | "SRC" | "SER" | "PID") => format!("{} {:04X}{:08X}{}", l, v2, p.value, if p.ok { "" } else { " !" }),
                    _ => format!("{} {:X}{}", l, p.value, if p.ok { "" } else { " !" }),
                },
                Kind::Ack => format!("ACK {}", p.value),
                _ if m.dec_proto == Proto::Uart && m.dec_ascii && p.value == 0x20 => "SP".to_string(),
                _ if m.dec_proto == Proto::Uart && m.dec_ascii && (0x21..0x7f).contains(&p.value) => format!("{}", p.value as u8 as char),
                _ => match p.value2 {
                    Some(v2) => format!("{}/{}", hex(p.value), hex(v2)),
                    None if m.dec_proto == Proto::I2c && !p.ok => format!("{} N", hex(p.value)),
                    None => hex(p.value),
                },
            }
}

/// The short tag of an event (start, stop, acknowledge ...), None for a field with a value.
fn packet_tag(m: &Model, p: &oos_dsp::decode::Packet) -> Option<&'static str> {
    use oos_dsp::decode::Kind;
    let event = matches!(p.kind, Kind::Field(l) if l.starts_with('!'));
    if !(event || matches!(p.kind, Kind::Start | Kind::RepStart | Kind::Stop | Kind::Break) || (p.kind == Kind::Ack && m.dec_proto != Proto::Swd)) {
        return None;
    }
    Some(match p.kind {
        Kind::Field(l) => &l[1..],
        Kind::Start if matches!(m.dec_proto, Proto::Can) => "SOF",
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
    let prefixes = [(1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "\u{b5}"), (1e-9, "n"), (1e-12, "p")];
    let a = span.abs().max(step.abs()) as f64;
    let (mult, p) = prefixes.iter().copied().find(|(m, _)| a >= *m * 0.9995).unwrap_or((1e-12, "p"));
    let st = (step as f64 / mult).abs();
    let needed = |x: f64| {
        let mut dec = 0;
        while dec < 3 {
            let k = x * 10f64.powi(dec);
            if (k.round() - k).abs() < 1e-3 {
                break;
            }
            dec += 1;
        }
        dec
    };
    let dec = needed(st).max(needed((base as f64 / mult).abs()));
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

#[cfg(test)]
mod tests {
    use super::axis_label;

    /// CAN and LIN decode the demo bus traffic: IDs, data and CRC / checksum.
    #[test]
    fn can_and_lin_decode_the_demo_bus() {
        use super::*;
        use oos_dsp::decode::Kind;
        for (proto, tdiv, want_id) in [(Proto::Can, 100e-6f32, 0x123u32), (Proto::Lin, 1e-3, 0x21)] {
            // (I2S and 1553 have their own test below)
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
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
            let mut cv = Canvas::new(W, H);
            ui.render(&mut cv);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
            let _ = std::fs::create_dir_all(&dir);
            let _ = crate::store::write(&dir.join(format!("i-{:?}.png", proto).to_lowercase()), &crate::store::png_of(&cv));
        }
    }

    /// Protocol trigger: the end of the matching CAN identifier sits at the trigger point; "Error" never fires on clean
    /// traffic in Normal mode.
    #[test]
    fn bus_trigger_aligns_on_the_matching_frame() {
        use super::*;
        use oos_dsp::decode::Kind;
        for id in [0x123u32, 0x18da_f110] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_proto = Proto::Can;
            ui.set_trig_type(TrigType::Bus);
            ui.m.trig_bus_val = id;
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
                let mut cv = Canvas::new(W, H);
                ui.render(&mut cv);
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                let _ = std::fs::create_dir_all(&dir);
                let _ = crate::store::write(&dir.join("j-bus-trigger.png"), &crate::store::png_of(&cv));
            }
        }
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.m.dec_proto = Proto::Can;
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
    fn bus_trigger_on_arinc_sent_and_flexray() {
        use super::*;
        use oos_dsp::decode::Kind;
        for (proto, tdiv) in [(Proto::Arinc429, 100e-6f32), (Proto::Sent, 200e-6), (Proto::FlexRay, 5e-6)] {
            let tsc = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
            let tol = 3.0 * tdiv as f64 * HDIV as f64 / 3600.0; // three record samples
            for what in [BusWhat::Id, BusWhat::Data, BusWhat::Frame, BusWhat::Error] {
                let mut ui = Ui::new(Source::Demo { t: 0.0 });
                ui.m.dec_proto = proto;
                ui.set_trig_type(TrigType::Bus);
                let id_default = ui.m.trig_bus_val;
                ui.m.trig_bus = what;
                // a payload value of the demo traffic
                if what == BusWhat::Data {
                    ui.m.trig_bus_val = match proto {
                        Proto::Arinc429 => 0x2_5a5a,
                        Proto::Sent => 0x7f_480b,
                        _ => 0x55,
                    };
                } else if what == BusWhat::Id && proto == Proto::Sent {
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
                assert!(p.iter().any(|x| crate::trig::matches(&ui.m, x) && x.t1.abs() < tol), "{:?} {:?}: no matching field ends at 0: {:?}", proto, what, p);
                if what == BusWhat::Id && proto != Proto::Sent {
                    let want = if proto == Proto::Arinc429 { 0o310 } else { 0x2a };
                    assert_eq!(id_default, want);
                    assert!(p.iter().any(|x| matches!(x.kind, Kind::Id { .. }) && x.value == want && x.t1.abs() < tol));
                }
                if what == BusWhat::Id {
                    let mut cv = Canvas::new(W, H);
                    ui.render(&mut cv);
                    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = crate::store::write(&dir.join(format!("j-bus-{:?}.png", proto).to_lowercase()), &crate::store::png_of(&cv));
                }
            }
        }
    }

    /// Protocol trigger on USB with the demo traffic (full speed, D+ / D- on CH3 / CH4): the token address, a PID,
    /// a payload byte and Frame (end of SYNC) at the trigger point, Error never fires; D+ alone decodes the same.
    #[test]
    fn bus_trigger_on_usb() {
        use super::*;
        use oos_dsp::decode::Kind;
        let tdiv = 5e-6f32;
        let tsc = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
        let tol = 3.0 * tdiv as f64 * HDIV as f64 / 3600.0; // three record samples
        for (what, val) in [(BusWhat::Id, None), (BusWhat::Id, Some(0x4B)), (BusWhat::Data, Some(0x40)), (BusWhat::Frame, None), (BusWhat::Error, None)] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_proto = Proto::Usb;
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
            assert!(p.iter().any(|x| crate::trig::matches(&ui.m, x) && x.t1.abs() < tol), "USB {:?}: no matching field ends at 0: {:?}", what, p);
            match (what, val) {
                (BusWhat::Id, None) => assert!(p.iter().any(|x| x.kind == Kind::Field("ADDR") && x.value == 5 && x.t1.abs() < tol)),
                (BusWhat::Id, Some(v)) => assert!(p.iter().any(|x| x.kind == Kind::Field("PID") && x.value == v && x.t1.abs() < tol)),
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
                let mut cv = Canvas::new(W, H);
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
    fn extended_decoders_decode_their_demo() {
        use super::*;
        // (protocol, time/div that shows a whole frame, a label or kind that must appear)
        let cases: [(Proto, f32, &str); 12] = [
            (Proto::CanFd, 50e-6, "BRS"),
            (Proto::Dmx512, 100e-6, "SC"),
            (Proto::ModbusRtu, 1e-3, "FUNC"),
            (Proto::Uart9, 1e-3, "Addr"),
            (Proto::OneWire, 200e-6, "ROM"),
            (Proto::UsbPd, 100e-6, "HDR"),
            (Proto::Swd, 20e-6, "REQ"),
            (Proto::Spdif, 20e-6, "Sample"),
            (Proto::FlexRay, 5e-6, "HCRC"),
            (Proto::Arinc429, 100e-6, "SSM"),
            (Proto::Sent, 200e-6, "STAT"),
            (Proto::Usb, 5e-6, "ENDP"),
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
        let mut cv = Canvas::new(W, H);
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
            let mut cv = Canvas::new(W, H);
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
        ui.set_dec_proto(Proto::CanFd);
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

    fn spectrum_peak(ui: &super::Ui) -> Option<f64> {
        ui.strongest_line().map(|m| m.f)
    }

    #[test]
    fn protocol_watch_names_a_new_bus() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.set_dec_proto(Proto::Uart);
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
        let mut cv = Canvas::new(W, H);
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
        let mut cv = Canvas::new(W, H);
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
        ui.m.dec_proto = Proto::Can;
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
        key(&mut ui, "9"); // a ninth digit is ignored
        key(&mut ui, "Del");
        key(&mut ui, "0");
        let mut cv = Canvas::new(W, H);
        ui.render(&mut cv);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
        let _ = std::fs::create_dir_all(&dir);
        let _ = crate::store::write(&dir.join("j-keypad.png"), &crate::store::png_of(&cv));
        key(&mut ui, "OK");
        assert_eq!(ui.m.trig_bus_val, 0x18da_f110);
        assert_eq!(ui.menu, MenuId::Trigger);
    }

    /// I2S and MIL-STD-1553 decode their demo signals.
    #[test]
    fn i2s_and_1553_decode_the_demo() {
        use super::*;
        use oos_dsp::decode::Kind;
        for (proto, tdiv) in [(Proto::I2s, 10e-6f32), (Proto::Mil1553, 20e-6)] {
            let mut ui = Ui::new(Source::Demo { t: 0.0 });
            ui.m.dec_on = true;
            ui.set_dec_proto(proto);
            ui.m.tscale = tscales().iter().position(|&t| (t - tdiv).abs() < tdiv * 1e-3).unwrap();
            for _ in 0..20 {
                ui.step(1.0 / 30.0);
            }
            let p = ui.decode_packets().unwrap();
            match proto {
                Proto::I2s => {
                    let s: Vec<(bool, u32)> = p.iter().filter_map(|x| if let Kind::Sample { right } = x.kind { Some((right, x.value)) } else { None }).collect();
                    assert!(s.contains(&(false, 0x4000)) && s.contains(&(true, 0xc000)), "{:x?}", s);
                }
                _ => {
                    let w: Vec<(bool, u32, bool)> = p.iter().filter_map(|x| if let Kind::Word { cmd } = x.kind { Some((cmd, x.value, x.ok)) } else { None }).collect();
                    assert!(w.contains(&(true, 0x0c23, true)) && w.contains(&(false, 0xbeef, true)), "{:x?}", w);
                }
            }
            let mut cv = Canvas::new(W, H);
            ui.render(&mut cv);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-shots");
            let _ = std::fs::create_dir_all(&dir);
            let _ = crate::store::write(&dir.join(format!("i-{:?}.png", proto).to_lowercase()), &crate::store::png_of(&cv));
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
        let b = Ui::PWR_OFF;
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
        let b = Ui::PWR_RESTART;
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
        let mut cv = Canvas::new(W, H);
        for zoom in [false, true] {
            ui.set_zoom(zoom);
            for _ in 0..5 {
                ui.step(0.033);
            }
            ui.render(&mut cv);
            let pal = heat_palette();
            let a = ui.area;
            let hits = (a.y..a.y + a.h).flat_map(|y| (a.x..a.x + a.w).map(move |x| (x, y))).filter(|&(x, y)| pal.contains(&cv.px[(y * W + x) as usize])).count();
            assert!(hits > 1000, "zoom {}: {} graded pixels", zoom, hits);
        }
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
        ui.store_dest = crate::store::StoreDest::Desk;
        ui.step(0.033);
        let rec = ui.last[0].as_ref().unwrap();
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "R1".into(), t0: rec.t0, dt: rec.dt, v: rec.ch[0].clone(), vpd: 1.0, offset: 0.0, on: true });
        ui.activate(Item::MaskFromRef);
        assert!(ui.m.mask_on && ui.mask.is_some());
        ui.adjust(Item::MaskInvert, 1);
        assert!(ui.m.mask_inv && ui.mask.as_ref().unwrap().invert);
        ui.activate(Item::MaskSave);
        let desk = dir.join("Desktop");
        assert!(desk.join("mask-001.txt").is_file());
        assert!(dir.join("mask1.txt").is_file());
        let t = std::fs::read_to_string(desk.join("mask-001.txt")).unwrap();
        assert!(t.starts_with("# OOS mask\n") && t.contains("inv=1"));
        ui.activate(Item::MaskClear);
        assert!(ui.mask.is_none() && !ui.m.mask_on);
        ui.m.mask_inv = false;
        ui.activate(Item::MaskLoad);
        assert!(ui.m.mask_on && ui.m.mask_inv && ui.mask_ref.as_ref().unwrap().len() > 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fast_frame_save_and_load_and_desktop_dest() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("oos-ff-{}", std::process::id()));
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        ui.store_dir = dir.clone();
        ui.store_dest = crate::store::StoreDest::Desk;
        for _ in 0..4 {
            ui.step(0.033);
        }
        assert!(ui.history.len() >= 3);
        ui.activate(Item::StoreFfSave);
        let p = dir.join("Desktop/frame-001.bin");
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
            crate::board::Frame { seq, dt, pre, ch: [v, Vec::new(), Vec::new(), Vec::new()], roll: false, envelope: false, scan: false, partial: false, acq: (1.25e9, n), stamp: 0, la: None }
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

    /// The row of item `it` in the open menu dialog.
    fn row_of(ui: &super::Ui, it: super::Item) -> super::Row {
        ui.dlg_layout().expect("menu open").rows.into_iter().find(|r| r.it == it).expect("row")
    }

    /// Layout: the plot fills the width and the height between the top bar and the cards (results as an overlay),
    /// touch targets of at least 40 px everywhere.
    #[test]
    fn layout_plot_size_and_touch_targets() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
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
            assert!(Ui::top_hit(r).h >= 40 && r.x + r.w <= Ui::NET_BOX.x, "{:?}", r);
        }
        // sticks: their boxes beside the network box, the buttons move over and stay on the bar
        (ui.usb, ui.wlan_stick) = (true, true);
        for (_, _, r) in ui.top_buttons() {
            assert!(r.x > Ui::RUN_PILL.x + Ui::RUN_PILL.w && r.x + r.w <= ui.usb_box().x, "{:?}", r);
        }
        let u = ui.usb_box();
        assert!(u.x + u.w < Ui::WLAN_BOX.x && Ui::WLAN_BOX.x + Ui::WLAN_BOX.w < Ui::NET_BOX.x);
        ui.tap(u.x + 5, 10);
        assert!(ui.menu_open && ui.menu_root() == MenuId::Storage);
        ui.tap(u.x + 5, 10);
        ui.tap(Ui::WLAN_BOX.x + 5, 10);
        assert!(ui.menu_open && ui.menu_root() == MenuId::Network);
        ui.tap(Ui::WLAN_BOX.x + 5, 10);
        (ui.usb, ui.wlan_stick) = (false, false);
        for n in 0..=NCH {
            for c in 0..NCH {
                ui.m.ch[c].on = c < n;
            }
            let (chs, tr, trg) = ui.cards();
            let (sb, mb) = Ui::trig_parts(trg);
            assert!(chs[0].x >= Ui::MENU_BTN.x + Ui::MENU_BTN.w && trg.x + trg.w <= W - 6 && trg.w >= 222 && tr.w >= 130, "{} on", n);
            for r in chs.iter().chain([tr, trg, Ui::MENU_BTN].iter()) {
                assert!(r.h >= 40 && r.y + r.h <= H && r.w >= 54, "{:?}", r);
            }
            // two buttons in a card: the finger gets half of its height each
            assert!(trg.h / 2 >= 32 && sb.w >= 60 && mb.y > sb.y + sb.h, "{:?} {:?}", sb, mb);
        }
        // analog off, a ref on: the CH1..CH4 cards go, TIME stays; its lower half toggles zoom
        for c in 0..NCH {
            ui.m.ch[c].on = false;
        }
        ui.refs[0] = Some(RefWave { src: 0, color: 0, label: "DQ".into(), t0: 0.0, dt: 1e-9, v: vec![0.0, 1.0], vpd: 0.2, offset: 0.0, on: true });
        let (chs, tr, trg) = ui.cards();
        assert!(chs.iter().all(|r| r.w == 0), "{:?}", chs);
        assert!(tr.w >= 130 && trg.w >= 222);
        assert!(!ui.m.zoom);
        ui.tap(tr.x + 10, tr.y + tr.h - 8);
        assert!(ui.m.zoom && !ui.menu_open);
        ui.tap(tr.x + 10, tr.y + tr.h - 8);
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
            assert!(d.close.h >= 40 && d.r.y >= TOP.h && d.r.y + d.r.h <= BOTTOM.y, "{:?}", m);
            assert!(d.rows.iter().all(|r| r.ctrl.h >= 40), "{:?}", m);
        }
    }

    /// Trigger > Type and Math > Operation: the value opens a picker, picking sets the value and returns to the menu
    /// (with its Back target); Back leaves it unchanged; the knobs still step the item.
    #[test]
    fn pickers_open_and_set_the_value() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
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
        let mut cv = Canvas::new(W, H);
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
        let yellow = (a.y + 60..a.y + a.h - 20).flat_map(|y| (a.x + 60..a.x + a.w - 60).map(move |x| (x, y))).filter(|&(x, y)| cv.px[(y * W + x) as usize] == CH_COLORS[0]).count();
        assert_eq!(yellow, 0);
        // Display > Clear does the same; SINGLE acquires and draws again
        ui.key(key::SINGLE);
        ui.step(0.033);
        assert!(lit(&ui) > 500 && ui.last[0].is_some());
        ui.activate(Item::ClearDisp);
        ui.step(0.033);
        assert_eq!(lit(&ui), 0);
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
        let (x, y) = mid(Ui::MENU_BTN);
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

    /// Every front-panel key function is in the start menu, and every menu is reachable by its key and by touch.
    #[test]
    fn start_menu_and_keys_reach_everything() {
        use super::*;
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        for _ in 0..3 {
            ui.step(0.033);
        }
        // the menu keys
        for (k, m) in [(key::HMENU, MenuId::Acquire), (key::ANALYSE, MenuId::Decode), (key::TMENU, MenuId::Trigger), (key::MEASURE, MenuId::Measure), (key::CURSOR, MenuId::Cursor), (key::MATH, MenuId::Math), (key::REF, MenuId::Ref)] {
            ui.close_menu();
            ui.key(k);
            assert!(ui.menu_open && ui.menu == m, "key {:#x}", k);
        }
        assert!(ui.items(MenuId::Acquire).contains(&Item::Open(MenuId::Horizontal)));
        ui.close_menu();
        // the Menu button opens the start menu with all entries
        let (x, y) = mid(Ui::MENU_BTN);
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
            assert_eq!((ui.menu, ui.parent), (m, Some(MenuId::Main)));
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
        let (x, y) = mid(Ui::MENU_BTN);
        touch(&mut ui, x, y);
        assert!(!ui.m.touch_lock && !ui.menu_open);
        touch(&mut ui, x, y);
        assert_eq!(ui.menu, MenuId::Main);
    }

    #[test]
    fn axis_labels() {
        // prefix from the span, decimals from step and base (offset / delay)
        assert_eq!(axis_label(-1e-6, 2e-7, 0.0, 1e-6, "s"), "-1.0 \u{b5}s");
        assert_eq!(axis_label(1.5, 0.5, 0.0, 2.0, "V"), "1.5 V");
        assert_eq!(axis_label(-0.6, 0.2, 0.0, 0.8, "V"), "-600 mV");
        assert_eq!(axis_label(3.25, 2.0, 4.75, 12.75, "V"), "3.25 V");
        assert_eq!(axis_label(-1e-9, 2.0, 0.0, 8.0, "V"), "0 V");
    }
}

/// One step on the 1-2-5 time ladder from the nearest ladder value.
fn ladder_step(x: f32, d: i32) -> f32 {
    let l = time_ladder();
    let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(l.len() - 1);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}
