// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The boundary between the scope application and the hardware of one board family (docs/porting.md).
//!
//! Everything the UI asks of the hardware goes through `Board`: the acquisition thread and what it is to apply, the
//! generator, the front-end steps, the front panel, the board readings. The UI speaks in the model's terms (volts per
//! division, seconds, Hz, the generator page's fields) and gets frames in volts back. Registers, dividers and clocks of
//! a board stay behind it, in its module: `dho` for the DHO800/DHO900. A lint test keeps `oos_hal::dho` and
//! `board::dho` out of the rest of the crate.
//!
//! The board is chosen when OOS is built (`Active`): an image is built for one board family anyway (kernel, device
//! tree). A port adds a module next to `dho`, implements `Board` for it and points `Active` at it.

pub mod dho;
mod record;

pub use record::to_record;

use crate::model::{Coupling, GenCh, Model, NCH, NGEN};
use std::sync::{Arc, Mutex};

/// The board this build drives.
pub type Active = dho::Dho;

/// The board of this build.
pub fn board() -> &'static Active {
    &dho::Dho
}

/// What the acquisition thread of the active board is to apply, made from the model (`Board::want`).
pub type Want = <Active as Board>::Want;
/// The generator settings of the active board, made from the model (`Board::gen`).
pub type Gen = <Active as Board>::Gen;

/// Start options of the acquisition thread.
pub struct Spawn {
    /// channels on at the start (bit per channel)
    pub mask: u32,
    /// set up the front ends (off for tests on a board without them)
    pub program_afe: bool,
    /// directory of the calibration files of the stock system
    pub cal_dir: String,
    /// start-up status file the thread reports its stages to (`oos_hal::bootstatus`)
    pub start_up: Option<String>,
}

/// One temperature reading of the SoC (thermal zone name, degC).
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub name: String,
    pub temp: f32,
}

/// Commands the UI sends to the front panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanelCmd {
    /// the application is up (end of the start-up)
    UiLoaded,
    PowerOff,
    Restart,
    /// a power key press is not a power-down
    CancelPowerDown,
}

/// Frames from the read-out-only debug source (`--fpga-live`): sequence, words, status.
pub type LiveShared = Arc<Mutex<(u64, Vec<u16>, String)>>;

/// The hardware of one board family as the UI sees it.
pub trait Board: Sync + 'static {
    /// what the acquisition thread is to apply for a model (compared with the last one by the thread)
    type Want: Send + 'static;
    /// the generator settings for a model
    type Gen: Send + Clone + PartialEq + 'static;

    /// analog channels of the largest model of the board (the unit's own: `model::n_ch`)
    const MAX_CHANNELS: usize;
    /// built-in generators of the largest model of the board (the unit's own: `model::gen_count`)
    const MAX_GENERATORS: usize;

    /// board family, for logs
    fn name(&self) -> &'static str;

    // ---- acquisition
    fn want(&self, m: &Model) -> Self::Want;
    /// roll, scan or ordinary records for the model's time base
    fn slow_mode(&self, m: &Model) -> Slow;
    /// time base (s/div) from which the board streams in roll mode
    fn roll_tdiv(&self) -> f64;
    /// starts the acquisition thread
    #[cfg(target_os = "linux")]
    fn spawn(&self, o: Spawn) -> HwShared;
    /// the read-out-only debug source (`--fpga-live`), if the board has one
    #[cfg(target_os = "linux")]
    fn live(&self, args: &[String]) -> Option<LiveShared>;
    /// prints what the debug source would write (`--fpga-dry`), no hardware access
    fn dry(&self);

    /// the memory depths of the Acquire menu (name, points per channel; 0 = Auto first). Setups store the position
    fn depths(&self) -> &'static [(&'static str, u64)];

    // ---- front end
    /// the fine volts-per-division steps of the front end, in nV
    fn vscale_steps_nv(&self) -> Vec<i64>;
    /// the input couplings of the channels, in menu order
    fn couplings(&self) -> &'static [Coupling];

    // ---- generators (models with one: `platform().caps.generators`)
    fn gen(&self, g: &GenCh) -> Self::Gen;
    /// highest frequency (Hz) of this wave shape
    fn gen_max_hz(&self, g: &GenCh) -> f64;
    /// amplitude and offset (nV) the output can make at `f_uhz` (µHz)
    fn gen_clamp(&self, ampl_nv: f64, offset_nv: f64, f_uhz: f64) -> (f64, f64);
    /// keeps one period of samples as the arbitrary wave; its id for the model, None without enough samples
    fn arb_store(&self, v: &[f32]) -> Option<u32>;
    /// samples of a text file, one per line
    fn arb_parse(&self, text: &str) -> Vec<f32>;
    /// the stored arbitrary wave at phase 0..1 (demo source)
    fn arb_at(&self, phase: f64) -> f64;

    // ---- board readings
    /// the FPGA version this build expects (System info)
    fn fpga_version(&self) -> u32;
    /// alarm limits (degC): SoC, FPGA
    fn temp_limits(&self) -> (f32, f32);
    fn thermal_zones(&self) -> Vec<Zone>;
    /// ADC clocks of System > Overclock, the standard one first
    fn clock_steps(&self) -> &'static [f64];

    // ---- front panel
    /// serial port and baud rate of the panel
    fn panel_port(&self) -> (&'static str, u32);
    fn panel_cmd(&self, c: PanelCmd) -> Vec<u8>;
    /// frames that set the LEDs (`mask` in panel LED order, `Ui::leds`)
    fn panel_leds(&self, mask: u32) -> Vec<Vec<u8>>;
    /// takes the complete frames from the front of `buf`, returns their key codes
    fn panel_keys(&self, buf: &mut Vec<u8>) -> Vec<u8>;
}

/// One decoded record: volts per enabled channel, sample period, index of the hardware trigger point.
#[derive(Clone, Default)]
pub struct Frame {
    pub seq: u64,
    pub dt: f64,
    pub pre: usize,
    pub ch: [Vec<f32>; NCH],
    /// roll mode: no trigger; the newest point belongs at the right edge of the screen
    pub roll: bool,
    /// the points are min/max pairs (FPGA compression, roll): never sinc-interpolate them
    pub envelope: bool,
    /// scan mode: the points are the screen from left to right (the sweep writes over the previous one)
    pub scan: bool,
    /// roll/scan picture before the screen is complete (SINGLE stops only on a complete one)
    pub partial: bool,
    /// sample rate and points per channel of the acquisition behind a compressed frame (top bar), else 0
    pub acq: (f64, usize),
    /// `Shared::stamp` in force when the generator and the settings behind this frame were applied
    pub stamp: u64,
    /// logic analyser words of the acquisition, times relative to the hardware trigger point (la.md 4.2; the last
    /// frame of a batch only, roll/scan: none yet)
    pub la: Option<crate::ui::la::LaRec>,
}

/// Acquisition mode for slow time bases (roll-mode.md 1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Slow {
    /// normal triggered records
    #[default]
    Off,
    Roll,
    /// `delay`: horizontal position (scan trigger position 0x4058); `single`: stop after one sweep
    Scan { delay: f64, single: bool },
}

/// Shared between UI and the acquisition thread: the settings the UI wants, the latest frame, a status line.
#[derive(Default)]
pub struct Shared {
    pub want: Option<Want>,
    pub slow: Slow,
    /// scan in SINGLE: the sweep is complete and the thread waits for `rearm` (the next SINGLE press)
    pub halted: bool,
    pub rearm: bool,
    /// the latest frame (auto set-up and the replay of the screen look at it)
    pub frame: Frame,
    /// frames not drawn yet, oldest first (one batch delivers many)
    pub queue: std::collections::VecDeque<Frame>,
    pub status: String,
    /// self-calibration: request (channels, AFE zero too), progress (done, total, text), result, cancel flag
    /// (channels, AFE zero, ADC cores)
    pub cal_req: Option<(Vec<usize>, bool, bool)>,
    pub cal_progress: Option<(usize, usize, String)>,
    pub cal_result: Option<Result<String, String>>,
    pub cal_cancel: bool,
    /// generators wanted (`Board::gen`); applied by the thread when they change
    pub afg: [Option<Gen>; NGEN],
    /// counter set by the UI with the settings; frames acquired after applying them carry it (`Frame::stamp`), so a
    /// sweep can tell frames of the new step from queued ones
    pub stamp: u64,
    /// one force-trigger pulse for the acquisition thread (the FPGA bit, not a new set-up)
    pub force: bool,
    /// FPGA version register 0x4000 (0 until the FPGA was reached)
    pub fpga_version: u32,
    /// FPGA XADC: die temperature (degC) and VCCINT (V), read about once a second (System info page)
    pub xadc: Option<(f32, f32)>,
    /// LA probe detect (GPIO 120, 1 = present), read about once a second; None = not read
    pub la_probe: Option<bool>,
}

/// Frames kept for the UI; older ones are dropped when the screen cannot keep up.
pub const QUEUE_MAX: usize = 96;

pub type HwShared = Arc<Mutex<Shared>>;

#[cfg(test)]
mod boundary {
    use std::path::{Path, PathBuf};

    fn rs_files(d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rs_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Where the children of a module file live: next to `main.rs` and `mod.rs`, else in the folder of its name.
    fn child_dir(f: &Path) -> PathBuf {
        let d = f.parent().unwrap();
        match f.file_stem().and_then(|s| s.to_str()) {
            Some("main") | Some("mod") => d.to_path_buf(),
            Some(s) => d.join(s),
            None => d.to_path_buf(),
        }
    }

    /// The code outside `#[cfg(test)]`: a file declared as a test module is test code as a whole; in other files an
    /// inline test module at the end of the file is cut off. Comment lines do not count.
    fn production(text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let mut end = lines.len();
        for i in 0..lines.len().saturating_sub(1) {
            let next = lines[i + 1].trim_start();
            if lines[i].trim() == "#[cfg(test)]" && next.starts_with("mod ") && next.ends_with('{') {
                end = i;
                break;
            }
        }
        lines[..end].iter().filter(|l| !l.trim_start().starts_with("//")).cloned().collect::<Vec<_>>().join("\n")
    }

    /// Only `board::dho` uses `oos_hal::dho`, and nothing outside `board` names `board::dho`: the rest of the UI talks
    /// to the hardware through `Board` (docs/porting.md). Tests may look behind it.
    #[test]
    fn only_the_board_module_touches_the_board() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rs_files(&src, &mut files);
        // files declared as `#[cfg(test)] mod name;`
        let mut test_only = Vec::new();
        for f in &files {
            let t = std::fs::read_to_string(f).unwrap();
            let l: Vec<&str> = t.lines().collect();
            for w in l.windows(2) {
                if w[0].trim() == "#[cfg(test)]" {
                    if let Some(n) = w[1].trim().strip_prefix("mod ").and_then(|r| r.strip_suffix(';')) {
                        test_only.push(child_dir(f).join(format!("{}.rs", n)));
                        test_only.push(child_dir(f).join(n).join("mod.rs"));
                    }
                }
            }
        }
        let dho = src.join("board").join("dho");
        let mut bad = Vec::new();
        for f in &files {
            if f.starts_with(&dho) || test_only.contains(f) {
                continue;
            }
            let code = production(&std::fs::read_to_string(f).unwrap());
            let rel = f.strip_prefix(&src).unwrap().display().to_string();
            for l in code.lines() {
                if l.contains("oos_hal::dho") || l.contains("board::dho") {
                    bad.push(format!("{}: {}", rel, l.trim()));
                }
            }
        }
        assert!(bad.is_empty(), "board code outside board/dho:\n{}", bad.join("\n"));
        // the check itself sees what it is meant to see
        assert!(production("use oos_hal::dho::afg;\n#[cfg(test)]\nmod tests {\n use oos_hal::dho::seq;\n}").contains("afg"));
        assert!(!production("fn a() {}\n#[cfg(test)]\nmod tests {\n use oos_hal::dho::seq;\n}").contains("seq"));
        assert!(test_only.iter().any(|p| p.ends_with("ui/lint.rs")), "{:?}", test_only);
    }
}
