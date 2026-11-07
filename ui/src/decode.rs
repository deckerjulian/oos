// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Decoder settings tables (the decoders themselves are in oos-dsp `decode`).

/// CAN bit rates of the decoder.
pub const CAN_RATES: [f64; 5] = [125e3, 250e3, 500e3, 800e3, 1e6];
/// CAN-FD data phase bit rates.
pub const FD_RATES: [f64; 5] = [1e6, 2e6, 4e6, 5e6, 8e6];
/// ARINC 429 bit rates; 0 = from the pulse spacing.
pub const ARINC_RATES: [f64; 3] = [0.0, 12.5e3, 100e3];
/// SENT clock ticks (s); 0 = any tick (from the sync pulses).
pub const SENT_TICKS: [f64; 8] = [3e-6, 5e-6, 10e-6, 20e-6, 30e-6, 50e-6, 90e-6, 0.0];
/// USB speed settings (auto: from the idle state and the SYNC).
pub const USB_SPEEDS: [oos_dsp::decode_ext3::UsbSpeed; 3] = [oos_dsp::decode_ext3::UsbSpeed::Auto, oos_dsp::decode_ext3::UsbSpeed::Low, oos_dsp::decode_ext3::UsbSpeed::Full];
pub const BAUDS: [f64; 10] = [1200.0, 2400.0, 4800.0, 9600.0, 19200.0, 38400.0, 57600.0, 115200.0, 460800.0, 921600.0];
