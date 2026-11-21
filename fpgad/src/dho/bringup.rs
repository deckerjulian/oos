// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The bring-up steps of `cold-boot` and `fpga-boot`: the xdma.ko parameters, PCIe host and xdma up and down,
//! and the start-up self-test of the calibration files and the temperatures. A new step of the bring-up goes
//! here.

#[cfg(target_os = "linux")]
use oos_hal::dho::afe;

// PCIe host and xdma up and down are the same on every board family with this endpoint (crate::pcie)
#[cfg(test)]
pub(super) use crate::pcie::xdma_params;
#[cfg(target_os = "linux")]
pub(super) use crate::pcie::{pcie_down, pcie_up};

/// Uptime of the BOOT command sent by `oos-fpgad fpga-boot`.
#[cfg(target_os = "linux")]
pub(super) const BOOTED_FILE: &str = "/tmp/oos-fpga-booted";

/// Start-up self-test: every calibration file OOS reads is there with a good CRC (a missing one only warns: the
/// scope runs on defaults, less accurate).
#[cfg(target_os = "linux")]
pub(super) fn check_cal(cal: &oos_hal::dho::boot::CalFiles, dir: &str, say: &dyn Fn(&str, &str, &str)) {
    const STAGE: &str = "Calibration data";
    say("run", STAGE, "");
    let mut missing = Vec::new();
    if cal.adc.is_none() {
        missing.push("cal_adc.hex");
    }
    if cal.ddr.is_none() {
        missing.push("cal_ddr.hex");
    }
    if afe::Cal::load_from(dir).is_err() {
        missing.push("vertical");
    }
    // only units with a generator have cal_afg.hex (the DHO800 and the DHO900 without "S" never had one)
    if oos_hal::model::platform().caps.has_gen() && oos_hal::dho::afg::Cal::load(dir).is_none() {
        missing.push("cal_afg.hex");
    }
    if missing.is_empty() {
        say("ok", STAGE, "all files read, CRC ok");
    } else {
        say("warn", STAGE, &format!("missing or damaged: {}", missing.join(", ")));
    }
}

#[cfg(target_os = "linux")]
pub(super) fn bar_for_temps() -> Option<oos_hal::dho::bus::Bar1> {
    oos_hal::dho::bus::Bar1::open().ok()
}

/// Start-up self-test: SoC and FPGA temperatures below the guard's limits (`board::is_hot`).
#[cfg(target_os = "linux")]
pub(super) fn check_temps(bar: &mut Option<oos_hal::dho::bus::Bar1>, say: &dyn Fn(&str, &str, &str)) {
    use oos_hal::dho::board;
    const STAGE: &str = "Temperatures";
    say("run", STAGE, "");
    let zones = board::thermal_zones(std::path::Path::new("/sys/class/thermal"));
    let fpga = bar.as_mut().and_then(|b| board::read_xadc(b)).map(|x| x.0);
    let soc = zones.iter().map(|z| z.temp).fold(f32::NAN, f32::max);
    // 0x1438 reads 0 on the DHO924S, under stock as well: no reading is no fault
    let txt = format!("SoC {:.0} C{}", soc, fpga.map_or(String::new(), |t| format!(", FPGA {:.0} C", t)));
    if board::is_hot(&zones, fpga) {
        say("warn", STAGE, &txt);
    } else {
        say("ok", STAGE, &txt);
    }
}
