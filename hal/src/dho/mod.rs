// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Rigol DHO800/DHO900 board: Zynq FPGA over PCIe/XDMA, RT8847IQ ADC, RT1642 front ends, LMX2582 clock, the
//! generator and logic analyser, the panel MCU and the board I/O (docs/hardware, docs/protocols). Everything here is
//! specific to this board; another board family gets a module of its own (docs/porting.md).

pub mod acq;
pub mod adc;
pub mod afe;
pub mod afg;
pub mod auxout;
pub mod avg;
pub mod board;
pub mod boot;
pub mod bus;
pub mod calfile;
pub mod clock;
pub mod coretrim;
pub mod counter;
pub mod dvm;
pub mod gpio;
pub mod hz;
pub mod la;
pub mod levels;
pub mod lmx2582;
pub mod madeup;
pub mod mask;
pub mod pairs;
pub mod panel;
pub mod readout;
pub mod roll;
pub mod scu;
pub mod seg;
pub mod selfcal;
pub mod seq;
pub mod serialtrig;
pub mod spu;
pub mod target;
pub mod tpu;
pub mod uarttrig;
pub mod wpu;
pub mod zone;
pub mod zonegeom;

/// One register write: (BAR1 offset, value).
pub type Write = (u32, u32);

/// The way back of the read-out, for the simulated unit; the tests only (ADR 0053).
#[cfg(any(test, feature = "sim"))]
pub mod encode;
/// The simulated unit; the tests only (ADR 0053).
#[cfg(any(test, feature = "sim"))]
pub mod sim;
