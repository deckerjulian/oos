// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The MHO900 behind `Board`, a port of the platform alone (docs/mho.md): OOS has no code for its acquisition board,
//! so this board never starts an acquisition thread and drives no generator; the front panel is only listened to. The screen shows
//! the demo signal (the image starts oos-ui without `--hw`, and `Platform::drives_hardware` is false for this build).
//! What remains are the UI's terms of the board: channels, scales, couplings, depths, and the SoC's temperatures.

use super::{Board, PanelCmd, Slow, Zone};
use crate::model::{AcqMode, Coupling, GenCh, Model, TrigMode};
use oos_hal::panelframe as panel;
use std::sync::Mutex;

/// The Rigol MHO900.
pub struct Mho;

/// Time base from which the screen rolls (as on the DHO800/DHO900 and stock: 200 ms/div).
const ROLL_TDIV: f64 = 0.2;

/// The arbitrary wave of the demo generator: one period of samples.
static ARB: Mutex<Vec<f32>> = Mutex::new(Vec::new());

impl Board for Mho {
    const MAX_CHANNELS: usize = 4;
    const MAX_GENERATORS: usize = 1;

    /// nothing is applied to the hardware
    type Want = ();
    /// (on, Hz, Vpp, V): for the demo source only
    type Gen = (bool, f64, f64, f64);

    fn name(&self) -> &'static str {
        "MHO900"
    }

    fn want(&self, _: &Model) -> Self::Want {}

    fn slow_mode(&self, m: &Model) -> Slow {
        // the stock rule of the DHO800/DHO900: Auto Roll on -> roll, off -> scan from 200 ms/div unless averaging
        let tdiv = m.hw_time_per_div() as f64 * 1.0001;
        if m.auto_roll && tdiv >= ROLL_TDIV {
            Slow::Roll
        } else if !m.auto_roll && tdiv >= ROLL_TDIV && m.acq != AcqMode::Average {
            Slow::Scan { delay: m.delay as f64, single: m.trig_mode == TrigMode::Single }
        } else {
            Slow::Off
        }
    }

    fn roll_tdiv(&self) -> f64 {
        ROLL_TDIV
    }

    fn trig_hyst_div(&self, _: f64, noise_reject: bool) -> f64 {
        if noise_reject {
            0.6
        } else {
            0.2
        }
    }

    #[cfg(target_os = "linux")]
    fn spawn(&self, _: super::Spawn) -> super::HwShared {
        // never asked for: the image starts oos-ui without --hw, and main drops --hw where the platform drives no
        // hardware
        let mut s = super::Shared::default();
        s.status = "OOS drives no MHO900 acquisition hardware (docs/mho.md)".into();
        std::sync::Arc::new(Mutex::new(s))
    }

    #[cfg(target_os = "linux")]
    fn live(&self, _: &[String]) -> Option<super::LiveShared> {
        None
    }

    fn dry(&self) {
        println!("# the MHO900 has no register sequences in OOS (docs/mho.md)");
    }

    fn seg_max(&self, _: &Model) -> Option<usize> {
        None
    }

    fn avg_hw(&self, _: &Model) -> bool {
        false
    }

    fn dvm_hw(&self) -> bool {
        false
    }

    fn mask_hw(&self) -> bool {
        false
    }

    fn zone_hw(&self) -> bool {
        false
    }

    fn uart_debug(&self, _: &Model) -> String {
        "no hardware protocol trigger on this board".into()
    }

    fn mask_map(&self, _: Option<&str>) -> Result<String, String> {
        Err("no hardware mask unit on this board".into())
    }

    fn mask_code(&self, _: f64, _: f64, _: f64, _: f64) -> f64 {
        0.0
    }

    fn mask_layout(&self, points: u32, _: u32) -> (usize, f64) {
        (super::MASK_COLUMNS, points as f64 / super::MASK_COLUMNS as f64)
    }

    fn vscale_steps_nv(&self) -> Vec<i64> {
        oos_hal::mho::board::scale_ladder()
    }

    fn couplings(&self) -> &'static [Coupling] {
        // 1 Mohm and 50 ohm inputs (data sheet)
        &[Coupling::Dc, Coupling::Ac, Coupling::Gnd, Coupling::Dc50]
    }

    fn gen(&self, g: &GenCh) -> Self::Gen {
        (g.on, g.freq, g.ampl, g.offset)
    }

    fn gen_max_hz(&self, _: &GenCh) -> f64 {
        0.0
    }

    fn gen_clamp(&self, ampl_nv: f64, offset_nv: f64, _: f64) -> (f64, f64) {
        (ampl_nv, offset_nv)
    }

    fn arb_store(&self, v: &[f32]) -> Option<u32> {
        if v.len() < 2 {
            return None;
        }
        *super::lock(&ARB) = v.to_vec();
        Some(1)
    }

    fn arb_parse(&self, text: &str) -> Vec<f32> {
        text.lines().filter_map(|l| l.split([',', ';', '\t', ' ']).find(|w| !w.is_empty())?.parse().ok()).collect()
    }

    fn arb_at(&self, phase: f64) -> f64 {
        let a = super::lock(&ARB);
        if a.is_empty() {
            return 0.0;
        }
        let i = ((phase.rem_euclid(1.0) * a.len() as f64) as usize).min(a.len() - 1);
        a[i] as f64
    }

    fn fpga_version(&self) -> u32 {
        0
    }

    fn temp_limits(&self) -> (f32, f32) {
        // the SoC's (the same RK3399 as on the DHO800/DHO900); no FPGA is read
        (85.0, 85.0)
    }

    fn thermal_zones(&self) -> Vec<Zone> {
        let zones = oos_hal::mho::board::thermal_zones(std::path::Path::new("/sys/class/thermal"));
        zones.into_iter().map(|(name, temp)| Zone { name, temp }).collect()
    }

    fn fan(&self) -> &'static str {
        "full speed (pwm-fan)"
    }

    fn clock_steps(&self) -> &'static [f64] {
        oos_hal::target::ACTIVE.clock_steps
    }

    // The front panel is read, not driven: UART0 as on the DHO (the stock system opens ttyS0 and sends on it, platform
    // probe: assumed to be the same MCU protocol, frontpanel.md). Its frames go to the panel log, keys of the DHO's
    // codes act as keys. Nothing is written to the MCU: no LEDs, no power commands (tools: oos-panel, docs/mho.md).
    fn panel_port(&self) -> (&'static str, u32) {
        ("/dev/ttyS0", 1_500_000)
    }

    fn panel_cmd(&self, _: PanelCmd) -> Vec<u8> {
        Vec::new()
    }

    fn panel_leds(&self, _: u32) -> Vec<Vec<u8>> {
        Vec::new()
    }

    fn panel_keys(&self, b: &mut Vec<u8>) -> Vec<u8> {
        let pieces = panel::take(b);
        #[cfg(all(target_os = "linux", not(test)))]
        log_panel(&pieces);
        panel::keys(&pieces)
    }
}

/// Where the frames of the panel MCU are written, for owners who try the panel (docs/mho.md); it stops at `LOG_MAX`.
#[cfg(all(target_os = "linux", not(test)))]
const PANEL_LOG: &str = "/tmp/oos-panel.log";
#[cfg(all(target_os = "linux", not(test)))]
const LOG_MAX: u64 = 1 << 20;

/// One line per frame (`frame cmd data`) and per run of skipped bytes (`skipped` and the bytes), with the time.
#[cfg(all(target_os = "linux", not(test)))]
fn log_panel(pieces: &[panel::Piece]) {
    use std::io::Write;
    if pieces.is_empty() || std::fs::metadata(PANEL_LOG).is_ok_and(|m| m.len() > LOG_MAX) {
        return;
    }
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mut text = String::new();
    let mut skipped = Vec::new();
    for p in pieces {
        match p {
            panel::Piece::Skipped(x) => skipped.push(format!("{:02x}", x)),
            panel::Piece::Frame(c, d) => {
                if !skipped.is_empty() {
                    text += &format!("{}.{:03} skipped {}\n", t.as_secs(), t.subsec_millis(), skipped.join(" "));
                    skipped.clear();
                }
                text += &format!("{}.{:03} frame {:02x} {:02x}\n", t.as_secs(), t.subsec_millis(), c, d);
            }
        }
    }
    if !skipped.is_empty() {
        text += &format!("{}.{:03} skipped {}\n", t.as_secs(), t.subsec_millis(), skipped.join(" "));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(PANEL_LOG) {
        let _ = f.write_all(text.as_bytes());
    }
}
