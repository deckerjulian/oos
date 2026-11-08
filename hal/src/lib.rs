// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! OOS hardware layer.
//!
//! The crate root holds what does not depend on one board: the model and its limits (`model`), the vendor file keys
//! (`vendorkeys`), signed updates (`update`), WLAN (`wlan`) and the start-up status file (`bootstatus`). Everything of
//! the DHO800/DHO900 board (RK3399 + Zynq FPGA over PCIe/XDMA) is in `dho`; a port to another board family adds a
//! module next to it (docs/porting.md).
//!
//! The register sequences are described in `docs/protocols/`. No code of the vendor libraries is used. Functions that
//! build register sequences are pure (they return `(offset, value)` lists and update software shadows of the
//! write-only registers), so they are unit-tested on the host; `dho::bus` applies them to the hardware.

pub mod bootstatus;
pub mod model;
pub mod target;
pub mod vendorkeys;
pub mod update;
pub mod wlan;

pub mod dho;

/// zlib / IEEE 802.3 CRC32.
pub fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}
