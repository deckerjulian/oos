// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! OOS cold boot: everything from a freshly booted FPGA image to the state where `seq::State::configure` takes over.
//! Implements the proposed sequence of docs/protocols/cold-boot.md section 4 (recipes in section 3), i.e. the start-up
//! plus the shell steps before it, with the parts
//! that already ran on the unit taken from `tools/hw/oos-hw.c` (fpga-boot, pll-init, afe-init, adc-init, spu-init,
//! scu-init, ddr-cal, adc-sync, adc-idelay).
//!
//! Tags: **[N]** not run under OOS yet; **[U]** verified, run on the unit under OOS; **[O]** observed on the unit
//! (agrees with its live data); **[A]** assumed, or an OOS choice. Nothing of the
//! calibration part (steps 7-21) has run on the unit yet.
//!
//! | # | step | tag |
//! |---|---|---|
//! | 1 | FPGA booted (`io::fpga_boot`: `\rBOOT\r` + slot, the only monitor command) and PCIe up; check 0x4000 | [U] |
//! | 2 | LMX2582 sample clock PLL over GPIO, 46 words 50 ms apart | [U] |
//! | 3 | AFE init x4 (`afe::Afe::init`) | [U] |
//! | 4 | ADC init 2 x 104 words + reg 0x0f (stock timing 1.1 ms / 100 us) | [U] words [N] |
//! | 5 | SPU init, 0x1014 = 0x00f00001 | [N] |
//! | 6 | SCU init 0x4004 | [N][U] |
//! | 7 | 0x1428 = 0x646e, MIG tap read-back (log), 0x1000 &= ~0xc | [N] |
//! | 8 | poll 0x1008 bit 29 (20 ms, 4 s), failure stops (never 0x1008 bit 31); 0x1210 only logged (0 until step 12 in a cold start) | [N][U] |
//! | 9 | ADC parameters of mode 0 from cal_adc.hex (`adc::Adc::parameter`) | [N][O] |
//! | 10 | DDR skip taps from cal_ddr.hex (skipped if absent), then poll 0x1008 bit 29 again | [N], poll [A] |
//! | 11 | sync-delay sweep 0..7 + SPI/SPU sync; no falling edge stays at 7, a register that never changes stops | [N][O], stop [A] |
//! | 12 | 32-step IDELAY eye scan, window search (`wnd`), load; fallback to the stored list | [N][O], fallback [A] |
//! | 13 | core alignment 0x1020 / 0x1014 | [N][O] |
//! | 14 | ADC internal trim, ADC 0 only, done = 0x1038 bit 6, 2 s wall-clock timeout (continues like the stock system) | [N], ADC 1 skipped [A] |
//! | 15 | ADC normal data 0x010d520f | [N] |
//! | 16 | core alignment again, 1 ms | [N] |
//! | 17 | 0x1418..0x1424 = 0x8000 | [N] |
//! | 18 | bit range 0x1014 (value unchanged) | [N] |
//! | 19 | sinc coefficient RAM: not done (only the FPGA display-trace path uses it) | [A] |
//! | 20 | AFE Hz enable x4 (3 writes each) | [N] |
//! | 21 | 0x4008 bits 26/24 = 0, bit 23 = 1 | [N] |
//!
//! Never done here (cold-boot.md "What OOS must never do"): any boot-monitor command other than BOOT, FPGA flash
//! erase/program, the factory DDR calibration or FPGA reload, writes under /rigol/data (the cal files are only read),
//! user calibrations, the ADC self test, setting 0x1008 bit 31 (BRAM fallback); 0x1008 is never written at all.
//!
//! Reads that steer decisions go through `Bus::rd` and time through `Clock`, so the whole sequence runs against
//! `sim::Unit` (scripted answers, virtual time) in the tests and in `dry_run`.

pub mod cal;
pub mod io;
pub mod regs;
pub mod sim;
pub mod wnd;

pub use cal::{DdrCal, CalFiles};
use regs::*;

use crate::dho::adc;
use crate::dho::afe::{self, AfeBus};
use crate::dho::bus::Bus;
use crate::dho::{scu, seq, tpu, Write};

/// Shadows of the write-only registers the boot modifies (all start at 0 except the ADC ones, which `regs::adc_init`
/// sets to their start values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shadows {
    pub s1000: u32,
    pub s1010: u32,
    pub s1014: u32,
    /// 16-bit shadow of 0x1020 (core alignment)
    pub s1020: u32,
    pub s1428: u32,
    pub s4004: u32,
    pub s4008: u32,
    /// ADC reg 0x00 (start 0xe4)
    pub adc00: u16,
    /// ADC reg 0x06 (start 0x01c3)
    pub adc06: u16,
}

/// Time source of the boot (sleeps and timeouts).
pub trait Clock {
    fn sleep_us(&mut self, us: u64);
    /// monotonic microseconds
    fn now_us(&mut self) -> u64;
}

/// Wall clock (`std::thread::sleep`, `Instant`).
pub struct SysClock(std::time::Instant);

impl Default for SysClock {
    fn default() -> SysClock {
        SysClock(std::time::Instant::now())
    }
}

impl Clock for SysClock {
    fn sleep_us(&mut self, us: u64) {
        std::thread::sleep(std::time::Duration::from_micros(us));
    }
    fn now_us(&mut self) -> u64 {
        self.0.elapsed().as_micros() as u64
    }
}

/// The three LMX2582 lines (`io::GpioPll` on the unit, `sim::PllRec` in tests).
pub trait PllPins {
    fn set(&mut self, pin: PllPin, v: bool);
}

/// One entry of the boot log.
#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    /// step number, title, tag
    Step(u32, &'static str, &'static str),
    Wr(u32, u32),
    Rd(u32, u32),
    Sleep(u64),
    /// one LMX2582 word [addr, hi, lo]
    Pll([u8; 3]),
    Note(String),
    Warn(String),
}

impl std::fmt::Display for Ev {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Ev::Step(n, t, tag) => write!(f, "== {:2} {} {}", n, t, tag),
            Ev::Wr(o, v) => write!(f, "   wr {:#06x} <- {:#010x}", o, v),
            Ev::Rd(o, v) => write!(f, "   rd {:#06x} -> {:#010x}", o, v),
            Ev::Sleep(us) => write!(f, "   sleep {} us", us),
            Ev::Pll(w) => write!(f, "   pll R{} <- {:#06x}", w[0], (w[1] as u16) << 8 | w[2] as u16),
            Ev::Note(s) => write!(f, "   # {}", s),
            Ev::Warn(s) => write!(f, "   !! {}", s),
        }
    }
}

/// Log as text; sleeps are folded into the line before them (`+N us`).
pub fn format_log(log: &[Ev]) -> String {
    let mut out = String::new();
    let mut line: Option<String> = None;
    let mut slept = 0u64;
    let flush = |out: &mut String, line: &mut Option<String>, slept: &mut u64| {
        if let Some(l) = line.take() {
            out.push_str(&l);
            if *slept > 0 {
                out.push_str(&format!("  +{} us", slept));
            }
            out.push('\n');
        } else if *slept > 0 {
            out.push_str(&format!("   sleep {} us\n", slept));
        }
        *slept = 0;
    };
    for e in log {
        if let Ev::Sleep(us) = e {
            slept += us;
            continue;
        }
        flush(&mut out, &mut line, &mut slept);
        line = Some(e.to_string());
    }
    flush(&mut out, &mut line, &mut slept);
    out
}

#[derive(Clone, Debug)]
pub struct Options {
    /// expected 0x4000
    pub fpga_version: u32,
    /// sleep before each ADC init word (`regs::ADC_INIT_PRE_US_STOCK` = run on the unit, `_SHORT` = 100 us)
    pub adc_init_pre_us: u32,
    /// lane delays loaded when the eye scan fails (the unit's list of 2026-11-02)
    pub idelay_fallback: [u32; 28],
    /// the 0x1210 check after the eye scan only warns instead of stopping, and 0x1210/0x1008 are logged before each
    /// of the steps 9..17 (diagnostic)
    pub ignore_clock: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { fpga_version: crate::dho::bus::FPGA_VERSION, adc_init_pre_us: ADC_INIT_PRE_US_STOCK, idelay_fallback: sim::UNIT_IDELAY, ignore_clock: false }
    }
}

/// Polls of 0x1008 bit 29 after the memory reset: 200 x 20 ms [N].
pub const DDR_POLLS: u32 = 200;
pub const DDR_POLL_US: u64 = 20_000;
/// Polls after the DDR tap reload: 51 x 500 ms [A].
pub const DDR_REPOLLS: u32 = 51;
pub const DDR_REPOLL_US: u64 = 500_000;
/// 0x1008 bit 29: "FPGA Memory DDR Init Pass" [N].
pub const DDR_INIT_PASS: u32 = 1 << 29;
/// 0x1210 window of the ADC clock check (`(v - 62494) < 13`) [N].
pub const ADC_CLK_MIN: u32 = 62494;
pub const ADC_CLK_MAX: u32 = 62506;

/// The 0x1210 window at the ADC clock in effect: exactly `ADC_CLK_MIN..=ADC_CLK_MAX` at 1.25 GHz. The count falls as the
/// clock rises, it goes as the period: 1.375 GHz read 56816 on the unit, 62500 x 1.25 / 1.375 = 56818.
pub fn adc_clk_window() -> (u32, u32) {
    let hz = crate::dho::clock::adc_hz();
    if hz == 1_250_000_000 {
        return (ADC_CLK_MIN, ADC_CLK_MAX);
    }
    let c = 62_500.0 * 1.25e9 / hz as f64;
    ((c - 6.0).floor() as u32, (c + 6.0).ceil() as u32)
}

/// LMX2582 words for the ADC clock in effect: Rigol's table at 1.25 GHz, else `lmx2582::table_for` (experimental,
/// limits.conf `adc.clock_hz`; docs/protocols/pll.md "Other output frequencies").
pub fn pll_table() -> Result<[[u8; 3]; 46], String> {
    if !crate::dho::clock::overclocked() {
        return Ok(PLL_TABLE);
    }
    crate::dho::lmx2582::table_for(crate::dho::clock::adc_hz() as f64)
}
/// Settling time after each sync delay of the sweep [N].
pub const SYNC_SETTLE_US: u64 = 20_000;
/// ADC trim timeout over all 8 cores (2 s) [N], wall clock [A].
pub const ADC_TRIM_TIMEOUT_US: u64 = 2_000_000;

/// What the boot found and the state it leaves.
pub struct Outcome {
    /// hand-over to `seq::State::configure` (ADC mode forced on the first configure)
    pub state: seq::State,
    pub sh: Shadows,
    pub ddr_status: u32,
    pub adc_clk: u32,
    pub sync_do: [bool; 8],
    pub sync_delay: u32,
    /// err[d] of the eye scan
    pub eye: [u32; 32],
    pub idelay: [u32; 28],
    /// false = the scan failed and `Options::idelay_fallback` was loaded
    pub idelay_scanned: bool,
    /// (Da, Db) of the two core alignments
    pub preamble: [(u32, u32); 2],
    pub trim_done: bool,
    pub warnings: Vec<String>,
}

impl Outcome {
    /// Steps of cold-boot.md section 4 this module leaves to others.
    pub fn todo() -> &'static [&'static str] {
        &[
            "step 19: sinc coefficient RAM 0x1140/0x1144 (only needed for the FPGA display-trace sinc path)",
            "step 22: first-configuration registers configure() does not write yet: the horizontal display-trace block and the WAU off (cold-boot.md 3.9); 0x1080/0x1084 (acq) and the read-path gain/offset (wpu, with AFE set-up) are written",
        ]
    }

    pub fn summary(&self) -> String {
        let b = |v: &[bool]| v.iter().map(|&x| if x { "1" } else { "0" }).collect::<Vec<_>>().join(",");
        let mut s = format!(
            "0x1008 = {:#010x}, 0x1210 = {}, sync DO {} -> delay {}, lane delays {:?} ({}), preamble {:?}, ADC trim {}, 0x1014 = {:#010x}, 0x1020 = {:#06x}",
            self.ddr_status,
            self.adc_clk,
            b(&self.sync_do),
            self.sync_delay,
            self.idelay,
            if self.idelay_scanned { "scanned" } else { "FALLBACK" },
            self.preamble,
            if self.trim_done { "done" } else { "TIMEOUT" },
            self.sh.s1014,
            self.sh.s1020
        );
        for w in &self.warnings {
            s.push_str(&format!("\nwarning: {}", w));
        }
        s
    }
}

pub struct Report {
    pub log: Vec<Ev>,
    pub result: Result<Outcome, String>,
}

/// Hardware handles of the boot. `pll`/`afe` = None skips steps 2/3 (already done, e.g. by `oos-hw`).
pub struct Hw<'a, B: Bus> {
    pub bus: &'a mut B,
    pub clock: &'a mut dyn Clock,
    pub pll: Option<&'a mut dyn PllPins>,
    pub afe: Option<&'a mut dyn AfeBus>,
    /// steps 2 and 3 already done (`early_start`)
    pub early: Option<Early>,
}

/// Sample clock and AFEs set up while the FPGA still loads: both hang on GPIO lines of the SoC.
pub struct Early {
    afes: [afe::Afe; 4],
    notes: Vec<String>,
}

/// Steps 2 and 3 ahead of the others, for the wait after the BOOT command.
pub fn early_start(pll: &mut dyn PllPins, afe_bus: &mut dyn AfeBus, clock: &mut dyn Clock, progress: &mut dyn FnMut(u32)) -> Result<Early, String> {
    progress(2);
    // the 50 ms of the stock driver between words are not needed by the chip; the last word (R0) starts its calibration
    for w in pll_table()? {
        for (pin, v) in pll_edges(w) {
            pll.set(pin, v);
        }
        clock.sleep_us(PLL_EARLY_GAP_US);
    }
    clock.sleep_us(PLL_WORD_GAP_US);
    progress(3);
    let mut cb = AfeCount { inner: afe_bus, w: 0, r: 0 };
    let (afes, notes) = afe_init(&mut cb);
    Ok(Early { afes, notes })
}

/// `Afe::init` on the four channels, a line of the log for each.
fn afe_init(cb: &mut AfeCount) -> ([afe::Afe; 4], Vec<String>) {
    let mut notes = Vec::new();
    let a = std::array::from_fn(|ch| {
        let (w0, r0) = (cb.w, cb.r);
        let (a, e) = afe::Afe::init(cb, ch);
        notes.push(format!("AFE CH{}: {} writes, {} reads, efuse 57/60..64 = {:04x?}", ch + 1, cb.w - w0, cb.r - r0, e));
        a
    });
    (a, notes)
}

/// Counts the AFE traffic of `Afe::init` (which needs a sized bus).
struct AfeCount<'b> {
    inner: &'b mut dyn AfeBus,
    w: usize,
    r: usize,
}

impl AfeBus for AfeCount<'_> {
    fn write(&mut self, ch: usize, reg: u8, val: u16) {
        self.w += 1;
        self.inner.write(ch, reg, val)
    }
    fn read(&mut self, ch: usize, reg: u8) -> u16 {
        self.r += 1;
        self.inner.read(ch, reg)
    }
}

struct Ctx<'a, B: Bus> {
    bus: &'a mut B,
    clk: &'a mut dyn Clock,
    sh: Shadows,
    log: Vec<Ev>,
    warnings: Vec<String>,
    /// called with the step number when a step starts (start-up screen)
    progress: &'a mut dyn FnMut(u32),
}

/// The start-up screen's group of a cold-boot step (the screen lists the groups in order; a step that stops the boot
/// marks its group failed).
pub fn stage(n: u32) -> &'static str {
    match n {
        1 => "FPGA image",
        2 => "Sample clock (PLL lock)",
        3 => "Analog front ends",
        // ADC init, SPU/SCU, DDR checks, ADC parameters and DDR taps alternate: one group, so no row starts twice
        4..=10 => "ADC and memory set-up",
        11..=16 => "ADC data lanes",
        _ => "Signal path",
    }
}

impl<B: Bus> Ctx<'_, B> {
    /// `--ignore-clock` diagnostic: 0x1210 and 0x1008 before a step.
    fn clock_probe(&mut self, opt: &Options) {
        if opt.ignore_clock {
            let (c, d) = (self.rd(0x1210), self.rd(0x1008));
            self.note(format!("clock probe: 0x1210 = {}, 0x1008 = {:#010x}", c, d));
        }
    }

    fn step(&mut self, n: u32, t: &'static str, tag: &'static str) {
        (self.progress)(n);
        self.log.push(Ev::Step(n, t, tag));
        // time since the start of the cold boot, for the boot-time profile
        let now = self.clk.now_us();
        self.log.push(Ev::Note(format!("t = {:.1} ms", now as f64 / 1e3)));
    }
    fn note(&mut self, s: String) {
        self.log.push(Ev::Note(s));
    }
    fn warn(&mut self, s: String) {
        self.log.push(Ev::Warn(s.clone()));
        self.warnings.push(s);
    }
    fn wr(&mut self, off: u32, v: u32) {
        debug_assert!(off != 0x1008, "0x1008 is never written (bit 31 = BRAM fallback)");
        self.bus.wr(off, v);
        self.log.push(Ev::Wr(off, v));
    }
    fn rd(&mut self, off: u32) -> u32 {
        let v = self.bus.rd(off);
        self.log.push(Ev::Rd(off, v));
        v
    }
    fn sleep(&mut self, us: u64) {
        if us > 0 {
            self.clk.sleep_us(us);
            self.log.push(Ev::Sleep(us));
        }
    }
    fn apply(&mut self, ws: &[Write]) {
        for &(o, v) in ws {
            self.wr(o, v);
        }
    }
    fn adc(&mut self, ws: &[adc::Wr]) {
        for w in ws {
            self.sleep(w.pre_us as u64);
            self.wr(w.reg, w.val);
            self.sleep(w.post_us as u64);
        }
    }
    /// ADC register read: request, 100 us, 0x1038 low 16 bits (no XOR: reg 7 mask 0, raw value) [N].
    fn adc_read(&mut self, reg: u32) -> u32 {
        self.wr(ADC_SPI, read_request(reg));
        self.sleep(100);
        self.rd(ADC_RDATA) & 0xffff
    }
    /// ADC SPI sync on, SPU sync, ADC SPI sync off, SPU reset [N].
    fn sync_seq(&mut self) {
        let w = spi_sync(&mut self.sh, true);
        self.adc(&[w]);
        let w = spu_adc_sync(&mut self.sh);
        self.apply(&w);
        let w = spi_sync(&mut self.sh, false);
        self.adc(&[w]);
        let w = spu_reset(&mut self.sh);
        self.apply(&w);
    }
    fn cal_select1(&mut self) {
        let w = cal_select(&mut self.sh, 1);
        self.wr(w.0, w.1);
    }
    /// Reads 0x1008 until bit 29: first read, then up to `n` x (sleep, read). Returns the last value.
    fn poll_ddr(&mut self, n: u32, period_us: u64) -> Result<u32, u32> {
        let mut v = self.rd(0x1008);
        for _ in 0..n {
            if v & DDR_INIT_PASS != 0 {
                break;
            }
            self.sleep(period_us);
            v = self.rd(0x1008);
        }
        if v & DDR_INIT_PASS != 0 {
            Ok(v)
        } else {
            Err(v)
        }
    }

    /// 3.3 Sync delay sweep.
    fn sync_sweep(&mut self) -> ([bool; 8], [u32; 8], Option<u32>) {
        let (mut r, mut raw) = ([false; 8], [0u32; 8]);
        for (d, rd) in r.iter_mut().enumerate() {
            let w = sync_delay(&mut self.sh, d as u32);
            self.adc(&[w]);
            self.sleep(SYNC_SETTLE_US);
            self.sync_seq();
            raw[d] = self.adc_read(7);
            *rd = (raw[d] >> 1) & 1 != 0;
        }
        let d = wnd::sync_choice(&r);
        if let Some(d) = d {
            let w = sync_delay(&mut self.sh, d);
            self.adc(&[w]);
            self.sync_seq();
        }
        (r, raw, d)
    }

    /// 3.4 Lane delay scan (ADC test pattern, Gray off, sync, select 1, 32 delays), ADC back to normal data.
    fn eye_scan(&mut self) -> [u32; 32] {
        self.adc(&[data_mode(true)]);
        self.adc(&output_code_zero());
        let w = adc_gray(&mut self.sh, false);
        self.wr(w.0, w.1);
        self.sync_seq();
        self.cal_select1();
        let mut err = [0u32; 32];
        for (d, e) in err.iter_mut().enumerate() {
            self.apply(&idelay(None, d as u32));
            let w = idelay_enable(&mut self.sh);
            self.apply(&w);
            let (a, b) = (self.rd(0x1024), self.rd(0x1028));
            *e = ber_err(a, b);
        }
        self.adc(&[data_mode(false)]);
        err
    }

    /// 3.5 ADC core alignment; returns (Da, Db).
    fn core_align(&mut self) -> (u32, u32) {
        self.sh.s1014 = (self.sh.s1014 & 0x00ff_ffff) | 0x1400_0000;
        self.wr(0x1014, self.sh.s1014);
        self.adc(&[data_mode(false)]);
        self.sync_seq();
        self.cal_select1();
        self.sh.s1020 &= !0x1010 & 0xffff;
        self.wr(0x1020, self.sh.s1020);
        self.cal_select1();
        let mut p = [0u32; 4];
        for (i, pi) in p.iter_mut().enumerate() {
            self.sh.s1014 = (self.sh.s1014 & !0x300) | (i as u32) << 8;
            self.wr(0x1014, self.sh.s1014);
            *pi = self.rd(0x1030);
        }
        let (da, db) = (p[0] & 0xff, (p[0] >> 16) & 0xff);
        self.note(format!("preamble codes {:#x} {:#x} {:#x} {:#x}: Da {} Db {}", p[0], p[1], p[2], p[3], da, db));
        if da != db {
            self.warn(format!("core alignment: Da {} != Db {} (only logged)", da, db));
        }
        self.cal_select1();
        self.sh.s1020 = (self.sh.s1020 & 0xf0f0) | (da & 3) | (db & 3) << 8;
        self.wr(0x1020, self.sh.s1020);
        self.sh.s1020 = (self.sh.s1020 & !0x1010 & 0xffff) | 1 << 4 | 1 << 12;
        self.wr(0x1020, self.sh.s1020);
        (da, db)
    }

    /// 3.6 ADC internal trim, ADC 0 only. On a timeout the trim stops at once (the trim bits of the remaining cores stay
    /// set), as on the stock system; OOS warns.
    fn adc_trim(&mut self) -> bool {
        let w: Vec<adc::Wr> = (0..8).map(|n| qi_trim(n, true)).collect();
        self.adc(&w);
        let t0 = self.clk.now_us();
        for n in 0..8u32 {
            loop {
                let v = self.adc_read(0x16 + 8 * n);
                if v & 0x40 != 0 {
                    break;
                }
                if self.clk.now_us() - t0 > ADC_TRIM_TIMEOUT_US {
                    self.warn(format!("ADC trim: core {} not done after 2 s (0x1038 bit 6), trim left on for cores {}..7", n, n));
                    return false;
                }
            }
            self.adc(&[qi_trim(n, false)]);
        }
        true
    }

    fn run(&mut self, pll: Option<&mut dyn PllPins>, mut afe_bus: Option<&mut dyn AfeBus>, early: Option<Early>, cal: &CalFiles, opt: &Options) -> Result<Outcome, String> {
        self.step(1, "FPGA image running, PCIe up: version register", "[U]");
        let v = self.rd(0x4000);
        if v != opt.fpga_version {
            return Err(format!("0x4000 = {:#010x}, expected {:#010x}: wrong or no FPGA image, nothing written", v, opt.fpga_version));
        }

        let mut afes: [afe::Afe; 4] = if let Some(e) = early {
            self.note("sample clock and AFEs set up during the FPGA wait".into());
            e.notes.into_iter().for_each(|n| self.note(n));
            e.afes
        } else {
            let table = pll_table()?;
            if crate::dho::clock::overclocked() {
                self.step(2, "sample clock: LMX2582 EXPERIMENTAL table (limits.conf adc.clock_hz) over GPIO", "[A]");
                self.note(format!("EXPERIMENTAL ADC clock {} Hz (lmx2582::table_for)", crate::dho::clock::adc_hz()));
            } else {
                self.step(2, "sample clock: LMX2582 1.25 GHz table over GPIO", "[U]");
            }
            match pll {
                Some(p) => {
                    for w in table {
                        for (pin, v) in pll_edges(w) {
                            p.set(pin, v);
                        }
                        self.log.push(Ev::Pll(w));
                        self.sleep(PLL_WORD_GAP_US);
                    }
                }
                None => self.note("skipped (no PLL handle: already programmed)".into()),
            }

            self.step(3, "AFE init x4", "[U]");
            match afe_bus.as_deref_mut() {
                Some(b) => {
                    let (a, notes) = afe_init(&mut AfeCount { inner: b, w: 0, r: 0 });
                    notes.into_iter().for_each(|n| self.note(n));
                    a
                }
                None => {
                    self.note("skipped (no AFE handle: already initialised); shadows = state after the AFE init".into());
                    [0, 1, 2, 3].map(afe::Afe::new)
                }
            }
        };

        self.step(4, "ADC init: 2 x 104 words + reg 0x0f", "[U] words, timing per Options");
        let w = adc_init(&mut self.sh, opt.adc_init_pre_us);
        self.adc(&w);

        self.step(5, "SPU init (0x1014 = 0x00f00001)", "[N]");
        let w = spu_init(&mut self.sh);
        self.apply(&w);

        self.step(6, "SCU init", "[N][U]");
        let w = scu_init(&mut self.sh);
        self.apply(&w);

        self.step(7, "once-only: DAC polling, MIG read-back, memory reset off", "[N]");
        let w = dac_polling(&mut self.sh);
        self.wr(w.0, w.1);
        let mig: Vec<String> = MIG_TAPS.iter().map(|&(r, m, n)| format!("{} {:#x}", n, self.rd(r) & m)).collect();
        self.note(format!("MIG taps: {}", mig.join(", ")));
        let w = mem_reset_off(&mut self.sh);
        self.wr(w.0, w.1);

        self.step(8, "health checks: 0x1008 bit 29 (DDR init pass) stops on failure; 0x1210 logged", "[N][U]");
        let ddr = self.poll_ddr(DDR_POLLS, DDR_POLL_US).map_err(|v| {
            format!("DDR init not passed: 0x1008 = {:#010x} after {} x {} ms; stopped (no BRAM fallback)", v, DDR_POLLS, DDR_POLL_US / 1000)
        })?;
        // This is log only: in a cold start 0x1210 is 0 until the ADC sync (step 11) and
        // the eye scan (step 12) bring the ADC data clock up (unit, 2026-11-03). The real check is after step 12.
        let clk = self.rd(0x1210);
        let (cmin, cmax) = adc_clk_window();
        if !(cmin..=cmax).contains(&clk) {
            self.note(format!("0x1210 = {} before the ADC sync (normal in a cold start; checked again after the eye scan)", clk));
        }
        self.note(format!("0x1008 = {:#010x}, 0x1210 = {}", ddr, clk));

        self.clock_probe(opt);
        self.step(9, "ADC parameters of mode 0 from cal_adc.hex", "[N][O]");
        let mut a = adc::Adc::default();
        match &cal.adc {
            Some(c) => {
                let w = a.parameter(c, 0);
                self.adc(&w);
            }
            None => self.warn("cal_adc.hex missing: ADC cores uncalibrated (gain/offset/0x1058 not written)".into()),
        }

        self.clock_probe(opt);
        self.step(10, "DDR skip taps from cal_ddr.hex", "[N], re-poll [A]");
        let mut ddr_status = ddr;
        match &cal.ddr {
            Some(d) => {
                let w = ddr_taps(&mut self.sh, &d.taps);
                self.apply(&w);
                ddr_status = self.poll_ddr(DDR_REPOLLS, DDR_REPOLL_US).map_err(|v| {
                    format!("DDR init not passed after the tap reload: 0x1008 = {:#010x} after {} x 500 ms; stopped", v, DDR_REPOLLS)
                })?;
            }
            None => self.note("no cal_ddr.hex: skipped".into()),
        }

        self.clock_probe(opt);
        self.step(11, "ADC sync-delay sweep + SPI/SPU sync", "[N][O]");
        let (sync_do, reg7, d) = self.sync_sweep();
        let b: Vec<&str> = sync_do.iter().map(|&x| if x { "1" } else { "0" }).collect();
        // The pattern differs from start to start. Without a falling edge (0,0,0,1,1,1,1,1 and 0,0,0,0,0,0,0,0 with bit 0
        // moving, seen on the unit) stock stays at 7, where the sweep ended, so this start goes on too; the checks after the
        // eye scan decide. Only a register that never changes at all is no ADC answering.
        if d.is_none() && reg7.iter().all(|&x| x == reg7[0]) {
            return Err(format!("ADC0 Sync DO:{}: ADC register 7 never changes ({:#06x}); stopped", b.join(","), reg7[0]));
        }
        let sync_delay = d.unwrap_or(7);
        self.note(format!("ADC0 Sync DO:{} -> delay {}{} (unit: 1,0,1,1,1,0,0,1 -> 0)", b.join(","), sync_delay, if d.is_none() { ", no 1 -> 0 edge, stays as swept" } else { "" }));

        self.clock_probe(opt);
        self.step(12, "28-lane IDELAY eye scan, window search, load", "[N][O], fallback [A]");
        let eye = self.eye_scan();
        let (idelay, scanned) = match wnd::lane_delays(&eye) {
            Ok(d) => (d, true),
            Err(e) => {
                for lane in 0..28 {
                    let s = wnd::lane_string(&eye, lane);
                    self.note(format!("Line {:2}: {}", lane, s));
                }
                self.warn(format!("eye scan without a complete result ({:?}): stored delays loaded", e));
                (opt.idelay_fallback, false)
            }
        };
        if scanned {
            let off: Vec<usize> = (0..28).filter(|&l| idelay[l].abs_diff(opt.idelay_fallback[l]) > 1).collect();
            self.note(format!("delays {:?}; lanes more than 1 off the stored list: {:?}", idelay, off));
        }
        let w = idelay_load(&mut self.sh, &idelay);
        self.apply(&w);

        let clk = self.rd(0x1210);
        let (cmin, cmax) = adc_clk_window();
        if !(cmin..=cmax).contains(&clk) {
            let msg = format!("ADC clock counter 0x1210 = {} outside {}..{} after the ADC sync and eye scan: no sample clock", clk, cmin, cmax);
            if !opt.ignore_clock {
                return Err(msg + "; stopped");
            }
            self.warn(msg + "; going on (--ignore-clock)");
        }
        self.clock_probe(opt);
        self.step(13, "ADC core alignment", "[N][O]");
        let p0 = self.core_align();

        self.clock_probe(opt);
        self.step(14, "ADC internal trim (ADC 0)", "[N], ADC 1 skipped [A]");
        let trim_done = self.adc_trim();

        self.clock_probe(opt);
        self.step(15, "ADC normal data", "[N]");
        self.adc(&[data_mode(false)]);

        self.clock_probe(opt);
        self.step(16, "ADC core alignment again, 1 ms", "[N]");
        let p1 = self.core_align();
        self.sleep(1000);

        self.clock_probe(opt);
        self.step(17, "AFE offset words 0x1418..0x1424", "[N]");
        self.apply(&afe_offset());

        self.step(18, "ADC bit range (0x1014 unchanged)", "[N]");
        let w = bit_range(&mut self.sh);
        self.wr(w.0, w.1);

        self.step(19, "sinc coefficient RAM", "[A] not needed for the raw read-out");
        self.note("skipped (TODO only if the FPGA display-trace sinc path is used)".into());
        self.step(20, "AFE Hz enable x4", "[N]");
        match afe_bus.as_deref_mut() {
            Some(b) => {
                let mut n = 0;
                for a in afes.iter_mut() {
                    let mut ops = Vec::new();
                    a.hz_enable(&mut ops, true);
                    for op in ops {
                        if let afe::Op::Afe { ch, reg, val } = op {
                            b.write(ch, reg, val);
                            n += 1;
                        }
                    }
                }
                self.note(format!("{} AFE writes (3 per channel)", n));
            }
            None => self.note("skipped (no AFE handle)".into()),
        }

        self.step(21, "SCU: LA off, LA processing off, channel processing on (0x4008)", "[N]");
        let w = proc_enables(&mut self.sh);
        self.apply(&w);

        // Hand-over to seq: shadows start at 0 [A]; TPU shadows = the stock edge values (the TPU is write-only and
        // tpu::edge rewrites the registers from them) [A]; ADC mode unknown so that the first configure writes the input
        // mode + ADC parameters of the mode (cold-boot.md 2, "first ADC configuration") [A].
        let state = seq::State {
            sh: scu::Shadows { s4004: self.sh.s4004, s4008: self.sh.s4008, s400c: 0, s1000: self.sh.s1000, s1010: self.sh.s1010, s4080: 0, s1004: 0, s4020: 0, s1080: 0, s1084: 0, s40b8: 0 },
            ts: tpu::Shadows::APP,
            afe: afes,
            adc: a,
            mode: usize::MAX,
            gain: [1.0; 4],
            la_was_on: false, holdoff_set: false, aux: None, mask_cols: None, mask_run: None, s1190: 0,
        };
        Ok(Outcome {
            state,
            sh: self.sh,
            ddr_status,
            adc_clk: clk,
            sync_do,
            sync_delay,
            eye,
            idelay,
            idelay_scanned: scanned,
            preamble: [p0, p1],
            trim_done,
            warnings: std::mem::take(&mut self.warnings),
        })
    }
}

/// Runs the cold boot (steps 1-21 above) on `hw`; the FPGA must already run its image with PCIe up (`io::power_on`
/// does the BOOT command and the PCIe hand-off first). Stops at the first failed check; the log has everything done
/// up to there.
pub fn cold_boot<B: Bus>(hw: Hw<'_, B>, cal: &CalFiles, opt: &Options) -> Report {
    cold_boot_with(hw, cal, opt, &mut |_| {})
}

/// `cold_boot` that reports the start of every step (`stage` groups them for the start-up screen).
pub fn cold_boot_with<B: Bus>(hw: Hw<'_, B>, cal: &CalFiles, opt: &Options, progress: &mut dyn FnMut(u32)) -> Report {
    let Hw { bus, clock, pll, afe, early } = hw;
    let mut c = Ctx { bus, clk: clock, sh: Shadows::default(), log: Vec::new(), warnings: Vec::new(), progress };
    for n in &cal.notes {
        c.note(n.clone());
    }
    let result = c.run(pll, afe, early, cal, opt);
    Report { log: c.log, result }
}

/// Dry run: the whole boot against `sim::Unit::nominal()` (reads answered with the unit's expected values, virtual
/// time), printed. On the unit the reads decide the branches, so this is the plan for a unit that behaves as on
/// 2026-11-02.
pub fn dry_run(cal: &CalFiles, opt: &Options) -> String {
    let mut unit = sim::Unit::nominal();
    if let Some(d) = &cal.ddr {
        unit.mig = [d.taps[0], d.taps[2], d.taps[1], d.taps[3], d.taps[4]];
    }
    let mut clk = sim::SimClock::default();
    let mut pll = sim::PllRec::default();
    let mut afe = sim::AfeStub::default();
    let r = cold_boot(Hw { bus: &mut unit, clock: &mut clk, pll: Some(&mut pll), afe: Some(&mut afe), early: None }, cal, opt);
    let mut s = String::from(
        "OOS cold boot, DRY RUN: no hardware. Reads are answered by sim::Unit with the unit's values of 2026-11-02 \
         (0x4000 = 0xe2072115, 0x1008 = 0x24000201, 0x1210 = 62500, ADC0 Sync DO 1,0,1,1,1,0,0,1, the logcat eye scan, \
         preamble 0, ADC trim done at once); on the unit those reads decide the branches.\n",
    );
    s.push_str(&format_log(&r.log));
    let wr = unit.writes.len();
    s.push_str(&format!(
        "-- {} register writes, {} reads, {} LMX2582 words, {} AFE writes, simulated time {:.2} s\n",
        wr,
        unit.reads.len(),
        pll.words.len(),
        afe.writes,
        clk.t_us as f64 / 1e6
    ));
    match &r.result {
        Ok(o) => s.push_str(&format!("-- result: ok\n{}\n", o.summary())),
        Err(e) => s.push_str(&format!("-- result: STOPPED: {}\n", e)),
    }
    for t in Outcome::todo() {
        s.push_str(&format!("-- todo: {}\n", t));
    }
    s
}

#[cfg(test)]
mod tests;
