// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! OOS hardware layer for the Rigol DHO800/DHO900 (RK3399 + Zynq FPGA over PCIe/XDMA).
//!
//! The register sequences are described in `docs/protocols/` (acquisition-start.md, horizontal-config.md,
//! trigger.md, afe-settings.md). No code of the vendor libraries is used. Functions that build register sequences are pure (they return `(offset, value)`
//! lists and update software shadows of the write-only registers), so they are unit-tested on the host; `bus` applies
//! them to the hardware.

pub mod acq;
pub mod adc;
pub mod afg;
pub mod afe;
pub mod board;
pub mod boot;
pub mod bootstatus;
pub mod bus;
pub mod calfile;
pub mod clock;
pub mod gpio;
pub mod hz;
pub mod la;
pub mod lmx2582;
pub mod model;
pub mod vendorkeys;
pub mod panel;
pub mod roll;
pub mod scu;
pub mod selfcal;
pub mod seq;
pub mod spu;
pub mod tpu;
pub mod update;
pub mod wlan;
pub mod wpu;

/// One register write: (BAR1 offset, value).
pub type Write = (u32, u32);
