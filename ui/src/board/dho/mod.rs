// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The DHO800/DHO900 behind `Board`: the acquisition thread (`hw`), the read-out debug source (`fpga`), the generator
//! (`oos_hal::dho::afg`), the front ends (`oos_hal::dho::afe`), the panel MCU (`oos_hal::dho::panel`) and the board
//! readings (`oos_hal::dho::board`, `oos_hal::dho::bus`). The only part of the UI that uses `oos_hal::dho`.

pub mod fpga;
pub mod hw;
#[cfg(test)]
mod tests;

use super::{Board, PanelCmd, Slow, Zone};
use crate::model::{Coupling, GenCh, Model};
use oos_hal::dho::{afe, afg, board, bus, panel};

/// The DHO800/DHO900.
pub struct Dho;

/// The generator's wave shape.
pub fn wave(g: &GenCh) -> afg::Wave {
    use afg::Wave;
    let (d, s) = (g.duty / 100.0, g.symmetry / 100.0);
    match g.wave {
        1 => Wave::Square { duty: d },
        2 => Wave::Ramp { symmetry: s },
        3 => Wave::Pulse { duty: d, rise: 0.0, fall: 0.0 },
        4 => Wave::Noise,
        5 => Wave::Dc,
        6 => Wave::Arb { id: g.arb },
        _ => Wave::Sine,
    }
}

/// The generator's modulation (sine, square and ramp carriers only).
pub fn modulation(g: &GenCh) -> afg::Mod {
    use afg::{Mod, ModShape};
    let shape = [ModShape::Sine, ModShape::Square, ModShape::Triangle, ModShape::UpRamp, ModShape::DownRamp, ModShape::Noise][g.mod_shape.min(5)];
    let rate = g.mod_rate;
    match g.modulation {
        _ if g.wave > 2 => Mod::None,
        1 => Mod::Am { rate, depth: g.mod_depth / 100.0, shape },
        2 => Mod::Fm { rate, dev: g.fm_dev, shape },
        3 => Mod::Pm { rate, dev: g.pm_dev, shape },
        _ => Mod::None,
    }
}

/// The depths of the Acquire menu, read for the screen through the FPGA compression above 1 M (`hw::batch_plan`). New
/// entries go to the end: setups store the position.
const DEPTHS: [(&str, u64); 11] = [
    ("Auto", 0), ("1k", 1_000), ("10k", 10_000), ("100k", 100_000), ("1M", 1_000_000), ("10M", 10_000_000),
    ("25M", 25_000_000), ("50M", 50_000_000), ("62.5M", 62_500_000), ("100M", 100_000_000), ("125M", 125_000_000),
];

impl Board for Dho {
    const MAX_CHANNELS: usize = 4;
    const MAX_GENERATORS: usize = 1;

    type Want = oos_hal::dho::seq::Settings;
    /// (on, Hz, Vpp, V, wave, modulation)
    type Gen = (bool, f64, f64, f64, afg::Wave, afg::Mod);

    fn name(&self) -> &'static str {
        "DHO800/DHO900"
    }

    fn want(&self, m: &Model) -> Self::Want {
        hw::settings(m, false)
    }

    fn slow_mode(&self, m: &Model) -> Slow {
        hw::slow_mode(m)
    }

    fn roll_tdiv(&self) -> f64 {
        hw::ROLL_TDIV
    }

    #[cfg(target_os = "linux")]
    fn spawn(&self, o: super::Spawn) -> super::HwShared {
        hw::spawn(o.mask, o.program_afe, o.cal_dir, o.start_up)
    }

    #[cfg(target_os = "linux")]
    fn live(&self, args: &[String]) -> Option<super::LiveShared> {
        let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
        let mut cfg = fpga::Cfg::default();
        if let Some(n) = arg("--n").and_then(|v| v.parse().ok()) {
            cfg.n = n;
        }
        if let Some(s) = arg("--start").and_then(|v| v.parse().ok()) {
            cfg.start = s;
        }
        cfg.arm = args.iter().any(|a| a == "--fpga-arm");
        cfg.play4 = args.iter().any(|a| a == "--fpga-play4");
        Some(fpga::spawn(cfg, arg("--period-ms").and_then(|v| v.parse().ok()).unwrap_or(30)))
    }

    fn dry(&self) {
        use oos_hal::dho::scu;
        let c = fpga::Cfg::default();
        let win = c.window();
        let mut sh = c.shadows();
        for (title, seq) in [
            ("read-out (play last), from the stock values", scu::readout(&win, &mut c.shadows())),
            ("arm (first 0, n 1)", scu::arm(c.fsm, &mut sh, 0, 1)),
            ("then: read-out (play last)", scu::readout(&win, &mut sh.clone())),
            ("or: read-out with type-4 play", scu::readout_play4(&win, &mut sh, 0, 1)),
        ] {
            println!("# {} ({} writes)", title, seq.len());
            for (off, v) in seq {
                println!("{:#x} {:#x}", off, v);
            }
        }
    }

    fn depths(&self) -> &'static [(&'static str, u64)] {
        &DEPTHS
    }

    fn vscale_steps_nv(&self) -> Vec<i64> {
        afe::scale_ladder()
    }

    fn couplings(&self) -> &'static [Coupling] {
        &[Coupling::Dc, Coupling::Ac, Coupling::Gnd]
    }

    fn gen(&self, g: &GenCh) -> Self::Gen {
        (g.on, g.freq, g.ampl, g.offset, wave(g), modulation(g))
    }

    fn gen_max_hz(&self, g: &GenCh) -> f64 {
        wave(g).max_hz()
    }

    fn gen_clamp(&self, ampl_nv: f64, offset_nv: f64, f_uhz: f64) -> (f64, f64) {
        afg::clamp(ampl_nv, offset_nv, f_uhz)
    }

    fn arb_store(&self, v: &[f32]) -> Option<u32> {
        afg::arb_set(v)
    }

    fn arb_parse(&self, text: &str) -> Vec<f32> {
        afg::arb_parse(text)
    }

    fn arb_at(&self, phase: f64) -> f64 {
        afg::arb_at(phase)
    }

    fn fpga_version(&self) -> u32 {
        bus::FPGA_VERSION
    }

    fn temp_limits(&self) -> (f32, f32) {
        (board::SOC_LIMIT, board::FPGA_LIMIT)
    }

    fn thermal_zones(&self) -> Vec<Zone> {
        board::thermal_zones(std::path::Path::new("/sys/class/thermal")).into_iter().map(|z| Zone { name: z.name, temp: z.temp }).collect()
    }

    fn clock_steps(&self) -> &'static [f64] {
        oos_hal::target::ACTIVE.clock_steps
    }

    fn panel_port(&self) -> (&'static str, u32) {
        // UART0 to the panel MCU (frontpanel.md)
        ("/dev/ttyS0", 1_500_000)
    }

    fn panel_cmd(&self, c: PanelCmd) -> Vec<u8> {
        match c {
            PanelCmd::UiLoaded => panel::UI_LOADED,
            PanelCmd::PowerOff => panel::POWER_OFF,
            PanelCmd::Restart => panel::RESTART,
            PanelCmd::CancelPowerDown => panel::CANCEL_POWER_DOWN,
        }
        .to_vec()
    }

    fn panel_leds(&self, mask: u32) -> Vec<Vec<u8>> {
        panel::led_frames(mask).iter().map(|f| f.to_vec()).collect()
    }

    fn panel_keys(&self, b: &mut Vec<u8>) -> Vec<u8> {
        // frames FA cmd data crc7 AF; anything else is skipped byte by byte
        let mut keys = Vec::new();
        while b.len() >= 5 {
            if b[0] != 0xfa || b[4] != 0xaf || b[3] != panel::crc7(b[1], b[2]) {
                b.remove(0);
                continue;
            }
            keys.push(b[2]);
            b.drain(..5);
        }
        keys
    }
}
