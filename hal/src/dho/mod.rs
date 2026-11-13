// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The Rigol DHO800/DHO900 board: Zynq FPGA over PCIe/XDMA, RT8847IQ ADC, RT1642 front ends, LMX2582 clock, the
//! generator and logic analyser, the panel MCU and the board I/O (docs/hardware, docs/protocols). Everything here is
//! specific to this board; another board family gets a module of its own (docs/porting.md).

pub mod acq;
pub mod adc;
pub mod avg;
pub mod afe;
pub mod auxout;
pub mod afg;
pub mod board;
pub mod boot;
pub mod bus;
pub mod calfile;
pub mod clock;
pub mod counter;
pub mod coretrim;
pub mod dvm;
pub mod gpio;
pub mod hz;
pub mod la;
pub mod levels;
pub mod lmx2582;
pub mod mask;
pub mod pairs;
pub mod panel;
pub mod roll;
pub mod scu;
pub mod selfcal;
pub mod seg;
pub mod seq;
pub mod spu;
pub mod target;
pub mod tpu;
pub mod uarttrig;
pub mod wpu;
pub mod zone;
pub mod zonegeom;

/// One register write: (BAR1 offset, value).
pub type Write = (u32, u32);
