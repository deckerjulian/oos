// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the acquisition loop needs from a unit, as one bundle (ADR 0053, docs/board-on-a-host.md 4): the register
//! window, the sample stream, the front-end bus, the single lines, the clock, and the generator's lines, which are
//! opened at its first request. `run` in hw.rs takes a `Hw<P>` and names none of the unit's own types; `spawn` opens
//! the scope's (`Scope`), in the order it always did (the FPGA version before the stream and the front ends), a host
//! run gives the simulated unit's. One type parameter, compiled for the one
//! port of the scope: no call through a table in the read path.

use oos_hal::dho::afe::AfeBus;
use oos_hal::dho::afg::GenLines;
use oos_hal::dho::boot::Clock;
use oos_hal::dho::bus::Bus;
use oos_hal::dho::lines::Lines;

/// The types of one unit's sides.
pub trait Port {
    type Bus: Bus;
    type Stream: std::io::Read;
    type Afe: AfeBus;
    type Lines: Lines;
    type Clock: Clock;
    type Gen: GenLines;
    /// The generator's lines, opened at its first request; an error when the unit has none or they cannot be opened.
    fn open_gen(&mut self) -> std::io::Result<Self::Gen>;
}

/// One unit's sides, as the loop takes them. `afe` is None when OOS leaves the front ends as they are.
pub struct Hw<P: Port> {
    pub bar: P::Bus,
    pub c2h: P::Stream,
    pub afe: Option<P::Afe>,
    pub lines: P::Lines,
    pub clock: P::Clock,
    pub port: P,
}

/// The scope: BAR1, the XDMA C2H stream, the bit-banged front-end bus, its GPIO lines, the system clock.
#[cfg(target_os = "linux")]
pub struct Scope;

#[cfg(target_os = "linux")]
impl Port for Scope {
    type Bus = oos_hal::dho::bus::Bar1;
    type Stream = std::fs::File;
    type Afe = oos_hal::dho::afe::GpioAfe;
    type Lines = oos_hal::dho::lines::UnitLines;
    type Clock = oos_hal::dho::boot::SysClock;
    type Gen = oos_hal::dho::afg::Io;
    fn open_gen(&mut self) -> std::io::Result<Self::Gen> {
        oos_hal::dho::afg::Io::open()
    }
}
