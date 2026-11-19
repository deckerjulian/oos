// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Scope state (what the user has set), value ladders and number formatting.
//!
//! This file holds the `Model` struct, the channel and source counts, the value ladders and the SI formatting. The
//! types and methods of each area are in a file of `model/`: channels, trigger, decode, acquisition, display,
//! analysers, math, generator, and the start values in `defaults.rs`. A new setting goes into the struct here, its
//! type and its methods into the file of its area.

use crate::gfx::{rgb, Rgb};
use crate::math::{MathOp, Window};

mod acquisition;
mod analysers;
pub mod bode;
mod channels;
mod decode;
mod defaults;
mod display;
mod generator;
pub mod la;
mod limits;
mod math;
mod trigger;

pub use acquisition::*;
pub use analysers::*;
pub use channels::*;
pub use decode::*;
pub use display::*;
pub use generator::*;
pub use limits::*;
pub use math::*;
pub use trigger::*;

/// Channel slots of the UI state: the largest model of the board (smaller models use the first `n_ch()`). The board
/// of the build has as many (`platform.rs` holds it to that at compile time).
pub const NCH: usize = 4;
/// Reference waveform slots, R1..R10. A slot is waveform memory, not a channel index.
pub const NREF: usize = 10;
/// Decoder namespace: CH0..3 and reserved legacy LA4..19 stay unchanged.
/// LA decoding still uses dec_la_in/dec_la; stored roles append R1..R10.
pub const DEC_REF_BASE: usize = NCH + 16;
pub const DEC_SOURCE_COUNT: usize = DEC_REF_BASE + NREF;

pub fn decode_wave(source: usize) -> Option<WaveId> {
    if source < NCH {
        Some(WaveId::Ch(source))
    } else if (DEC_REF_BASE..DEC_SOURCE_COUNT).contains(&source) {
        Some(WaveId::Ref(source - DEC_REF_BASE))
    } else {
        None
    }
}

pub fn decode_source_name(source: usize) -> String {
    match decode_wave(source) {
        Some(WaveId::Ch(c)) => format!("CH{}", c + 1),
        Some(WaveId::Ref(i)) => format!("R{}", i + 1),
        None => "Unavailable".into(),
    }
}
const _: () = assert!(NCH == oos_dsp::CHANNELS, "triggers and expressions in dsp take oos_dsp::CHANNELS channels");

/// Generator slots of the UI state: the largest model of the board (the unit's own: `gen_count`). The board of the
/// build has as many (`platform.rs`).
pub const NGEN: usize = 1;

/// Analog inputs of the unit (`oos_hal::model::platform()`), at most NCH.
pub fn n_ch() -> usize {
    oos_hal::model::platform().caps.channels.min(NCH)
}

/// Built-in generators of the unit (`oos_hal::model::platform()`), at most NGEN.
pub fn gen_count() -> usize {
    oos_hal::model::platform().caps.generators.min(NGEN)
}

/// The memory depths of the board (Acquire menu): the table of this build's board (`oos_hal::target::DEPTHS`).
pub fn mdepths() -> &'static [(&'static str, u64)] {
    oos_hal::target::DEPTHS
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

/// Total selected single, pair and catalog rows, shared by UI and setup validation.
pub const MEAS_SELECTION_MAX: usize = 1024;

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

/// 1-2-5 steps over the decades `10^lo` .. `5 * 10^(hi - 1)`, each `k * 10^e` as written (`powi`), with 0 in front
/// when `zero` (off, or auto).
pub fn decades(lo: i32, hi: i32, zero: bool) -> Vec<f64> {
    zero.then_some(0.0).into_iter().chain((lo..hi).flat_map(|e| [1.0, 2.0, 5.0].map(|k| k * 10f64.powi(e)))).collect()
}

/// The entry of the ladder `l` that is `d` steps from `x`, stopping at its ends. `x` stands at the first entry not
/// below it (a thousandth lower counts), or at index `miss` when the ladder has none.
pub fn ladder_step(l: &[f64], x: f64, d: i32, miss: usize) -> f64 {
    let i = l.iter().position(|&v| v >= x * 0.999).unwrap_or(miss);
    l[(i as i32 + d).clamp(0, l.len() as i32 - 1) as usize]
}

/// Index `i` of `n` moved by `d`, wrapping around.
pub fn cycle(i: usize, n: usize, d: i32) -> usize {
    (i as i32 + d).rem_euclid(n as i32) as usize
}

/// Index `i` of `n` moved by `d`, stopping at the ends.
pub fn step_index(i: usize, n: usize, d: i32) -> usize {
    (i as i32 + d).clamp(0, n as i32 - 1) as usize
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

/// Channel colours (yellow, teal, pink, sky blue: a colour-blind safe set). CH2 teal and CH4 blue as the colour
/// rings of the scope's own probes and inputs have them (CH2 cyan, CH4 blue); until 0.1.11 the two were the other way.
const PALETTE: [Rgb; 8] = [
    rgb(0xf0, 0xe4, 0x42),
    rgb(0x2e, 0xc4, 0xa0),
    rgb(0xe0, 0x8c, 0xc0),
    rgb(0x56, 0xb4, 0xe9),
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
pub const VSCALES: [f32; 16] =
    [100e-6, 200e-6, 500e-6, 1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0, 2.0, 5.0, 10.0];
/// Fastest time/div the ADC acquisition uses. Stored refs (SIM) may display faster than this.
pub const HW_MIN_TDIV: f32 = 1e-9;

/// s/div: 50 ps .. 1000 s. Below 1 ns/div is a display scale for stored records; the live ADC still runs at
/// `HW_MIN_TDIV` (`hw_time_per_div`).
/// The step of `ladder` nearest to `v`, by ratio (a 1-2-5 ladder: 0.37 is nearer to 0.5 than to 0.2).
pub fn nearest_step(ladder: &[f32], v: f32) -> usize {
    let mut best = 0;
    for (i, &x) in ladder.iter().enumerate() {
        if ((x / v).ln()).abs() < ((ladder[best] / v).ln()).abs() {
            best = i;
        }
    }
    best
}

pub fn tscales() -> Vec<f32> {
    // from 50 ps/div (stock 5 ns, then 1 ns): below 5 ns/div the live screen holds 12..62 samples, drawn with the
    // windowed sinc of sparse views; the record keeps the margin of `board::dho::hw::screen_margin`
    decades(-11, 4, false).into_iter().filter(|&x| (0.49e-10..=1000.1).contains(&x)).map(|x| x as f32).collect()
}

pub use oos_dsp::measure::{Meas, ALL as ALL_MEAS};
pub use oos_dsp::sim::VideoStd;
pub use oos_dsp::trigger::{Bit, Pol};

#[derive(Clone, PartialEq, Debug)]
pub struct Model {
    pub ch: [Channel; NCH],
    /// Post-capture time correction in seconds, subtracted from native source time.
    /// Independent of the hardware channel's sampling deskew; samples are never rewritten.
    pub deskew_ch: [f64; NCH],
    pub deskew_ref: [f64; NREF],
    pub active: usize, // channel the vertical knobs act on
    /// Track cursor B's source. None follows the selected workspace source.
    pub cur_b_source: Option<WaveId>,
    pub tscale: usize,
    pub delay: f32, // s, horizontal position of the trigger point relative to the centre
    pub trig_src: usize,
    /// Stored reference trigger source; None selects the physical trig_src channel.
    pub trig_ref: Option<usize>,
    /// edge and pulse trigger on a logic analyser line instead of `trig_src` (`trig_line`)
    pub trig_la: Option<u8>,
    /// pattern trigger: D0..D15 (H / L / X), with the LA on
    pub pat_la: [Bit; 16],
    pub trig_level: f32,
    pub slope: Slope,
    pub trig_mode: TrigMode,
    /// the sweep (Auto or Normal) before a Single, which Run goes back to (`Model::run_sweep`); not part of a setup
    pub sweep_before_single: TrigMode,
    /// trigger holdoff, seconds (0 = the shortest there is)
    pub holdoff: f32,
    pub trig_type: TrigType,
    /// second level: low threshold (slope, runt, window), level of B (delay), clock level (setup/hold)
    pub trig_level2: f32,
    /// noise reject: wider comparator hysteresis (0.7 instead of 0.3 div)
    pub trig_nrej: bool,
    /// the auto set-up sets noise reject from the noise on the trigger signal's flat levels (opt-in, off by default)
    pub auto_nrej: bool,
    /// coupling of the edge trigger on a channel (`trig_coupling` gives the one in effect)
    pub trig_coup: TrigCoup,
    /// second source: B (delay), clock (setup/hold)
    pub trig_src2: usize,
    /// Stored reference for delay B / setup-hold clock.
    pub trig_ref2: Option<usize>,
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
    /// the offsets of the interleaved ADC cores taken off in software (`board::dho::cores`; Acquire > ADC cores)
    pub cores: bool,
    pub avg_count: u32,
    /// Average mode: the FPGA averages where the board can (`Board::avg_hw`), else
    /// the software average; experimental, not verified on the unit
    pub avg_hw: bool,
    /// segmented recording: records per RUN/SINGLE (0 = off), browsed with the history afterwards
    pub segments: usize,
    /// segments recorded by the FPGA (one arm, hardware re-arm and trigger stamps) instead of the software history;
    /// experimental, not verified on the unit
    pub seg_hw: bool,
    /// hardware segments: wait after each segment before the next arm, seconds (0 = back to back)
    pub seg_ivl: f32,
    pub mdepth: usize,  // index into `mdepths()` (0 = auto)
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
    /// Window of each reference trace in a split (not Stacked).
    pub ref_win: [usize; NREF],
    pub meas: Vec<(Meas, WaveId)>,
    /// delay and phase: entry of `measures::pairs::PAIRS`, source A, source B (a channel or a loaded ref)
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
    pub aux_out: AuxOut,
    /// fine V/div: the scale knob walks the rows of the calibration table (`Board::vscale_steps_nv`), SCPI
    /// `:CHANnel<n>:SCALe` takes any value; one switch for all channels
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
    pub math_offset: f32, // divisions
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
    /// Known C-PHY symbol rate in sym/s; zero recovers timing from the record.
    pub dec_cphy_rate: f64,
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
    /// the value (or lower limit) of the condition: up to 40 bits (I2C, 5 bytes)
    pub trig_bus_val: u64,
    pub ser: SerialTrig,
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
    /// option settings of a protocol module (`protocols::Opt`), indices into its choices
    pub dec_opt: [u8; oos_dsp::protocols::OPTS],
    pub zoom: bool,
    /// slow time bases on the hardware: roll from 50 ms/div (on), scan from 200 ms/div (off), like stock "Auto Roll"
    pub auto_roll: bool,
    pub zscale: usize, // index into tscales(), <= tscale
    pub zdelay: f32,   // centre of the zoom window relative to the trigger
    /// colour-graded persistence: trace pixels coloured by hit density (temperature map)
    pub color_grade: bool,
    /// draw the samples as dots instead of connected lines (pseudo dot mode)
    pub dots: bool,
    /// what an HDMI monitor shows (Display > HDMI). Mirror copies the panel. Waveform and Layout put the plot on the
    /// display and leave the panel for controls; the density changes that plot, the panel keeps its bars.
    pub hdmi: HdmiMode,
    /// Display > Bottom bar: the compact bar of PR #8 (Menu, Sources, the active source, Overlay/Stacked, time,
    /// trigger) instead of the cards (the default)
    pub bar_compact: bool,
    /// Display > Monitor view: the view (1-based) an HDMI monitor in Layout shows instead of the panel's (0: the panel's;
    /// ui/monitor.rs)
    pub hdmi_view: usize,
    /// Display > Views: how many screen arrangements there are to switch between (1: off; ui/views.rs)
    pub views: usize,
    /// the one shown (0-based)
    pub view_cur: usize,
    /// what each view keeps while another one is shown (ui/views.rs, `Ui::view_snapshot`); empty: not set up yet
    pub view_slots: [String; VIEWS_MAX],
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
    /// False measures the visible time window; true measures each source's complete stored record.
    pub meas_full_record: bool,
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
    /// the external generator is on the USB-A port (USBTMC, the first one found) instead of `bode_addr`
    pub bode_usb: bool,
    /// Bode with a generator that sweeps on its own (`model::bode::FREE`, same start/stop): sweep time (s), logarithmic
    /// (else linear), sync channel, its edge and level (V), input channel (0 = none, k = CHk: then the gain is against
    /// `bode_ref` dBV)
    pub bode_sw_time: f64,
    pub bode_sw_log: bool,
    pub bode_sync: usize,
    pub bode_sync_slope: Slope,
    pub bode_sync_level: f32,
    pub bode_sw_in: usize,
    pub bode_ref: f64,
    /// hardware source: bumped with settings a measurement must wait for (`board::Shared::stamp`); not stored
    pub hw_stamp: u64,
    /// hardware source: bumped when the hardware average has to start over (CLEAR, RUN, SINGLE); not stored
    pub hw_avg_clear: u64,
    /// AUTO sweep with the shortest auto-trigger time, set by the auto set-up while it reads levels; not stored
    pub auto_fast: bool,
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
    /// size of the text on the screen
    pub text_size: TextSize,
    /// logic analyser D0..D15 (DHO900, ui/la.rs)
    pub la: crate::model::la::LaSet,
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

/// Engineering format: 3 significant digits by default, SI prefix, unit.
pub fn si(v: f64, unit: &str) -> String {
    si_digits(v, unit, 3)
}

/// SI prefixes from tera to pico, the largest first.
pub const SI_PREFIXES: [(f64, &str); 9] = [
    (1e12, "T"),
    (1e9, "G"),
    (1e6, "M"),
    (1e3, "k"),
    (1.0, ""),
    (1e-3, "m"),
    (1e-6, "\u{b5}"),
    (1e-9, "n"),
    (1e-12, "p"),
];

/// The prefix of `SI_PREFIXES[from..]` for the magnitude `a`: the largest it reaches (half a per mille short counts),
/// pico below them all.
pub fn si_prefix(a: f64, from: usize) -> (f64, &'static str) {
    SI_PREFIXES[from..].iter().copied().find(|(m, _)| a >= *m * 0.9995).unwrap_or((1e-12, "p"))
}

pub fn si_digits(v: f64, unit: &str, digits: usize) -> String {
    if !v.is_finite() {
        return format!("---- {}", unit);
    }
    if v == 0.0 {
        return format!("0 {}", unit);
    }
    let (mult, p) = si_prefix(v.abs(), 0);
    let x = v / mult;
    let ax = x.abs();
    let dec = if ax >= 100.0 {
        digits.saturating_sub(3)
    } else if ax >= 10.0 {
        digits.saturating_sub(2)
    } else {
        digits.saturating_sub(1)
    };
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
}
