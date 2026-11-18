// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! Decoder settings tables (the decoders themselves are in oos-dsp `decode`, and the tables their options choose
//! from with them).

pub use oos_dsp::decode::{ARINC_RATES, BAUDS, CAN_RATES, SENT_TICKS};
/// CAN-FD data phase bit rates.
pub const FD_RATES: [f64; 5] = [1e6, 2e6, 4e6, 5e6, 8e6];
/// USB speed settings (auto: from the idle state and the SYNC).
pub const USB_SPEEDS: [oos_dsp::decode::UsbSpeed; 3] =
    [oos_dsp::decode::UsbSpeed::Auto, oos_dsp::decode::UsbSpeed::Low, oos_dsp::decode::UsbSpeed::Full];

pub const CPHY_RATES: [f64; 19] =
    [0.0, 0.5e9, 1e9, 1.5e9, 2e9, 2.25e9, 2.5e9, 3e9, 3.5e9, 4e9, 4.5e9, 6e9, 8e9, 10e9, 12e9, 16e9, 20e9, 25e9, 32e9];
pub fn cphy_rate_label(rate: f64) -> String {
    if rate == 0.0 {
        "Auto".into()
    } else {
        format!("{} Gsym/s", rate / 1e9)
    }
}
pub fn parse_cphy_rate(text: &str) -> Result<f64, String> {
    if text.trim().eq_ignore_ascii_case("AUTO") {
        return Ok(0.0);
    }
    let rate = text.trim().parse::<f64>().map_err(|_| "C-PHY symbol rate needs Hz or AUTO".to_string())?;
    if !rate.is_finite() || !(0.0..=1e12).contains(&rate) {
        return Err("C-PHY symbol rate must be AUTO or 0..1e12 sym/s".into());
    }
    Ok(rate)
}
