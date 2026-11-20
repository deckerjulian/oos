// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The decoder of the example protocol module of oos-ui (`ui/src/protocols/example.rs`), a template that uses every
//! hook: UART at a selectable bit rate. A real decoder declares its [`ModuleInfo`] in the file of its family
//! (`decode/<family>.rs`); the example's is here, behind the feature `example`, which oos-ui turns on for its tests
//! only, so it is in [`MODULES`](super::MODULES) at the place of the module in oos-ui's registry and never on a scope.

use super::{BusWhat, ModuleInfo};
use crate::decode::{uart, Kind, Packet, Parity};
use crate::trigger::Slope;

/// The bit rates of the module's first option.
pub const RATES: [f64; 3] = [9600.0, 19200.0, 115200.0];

pub static EXAMPLE: ModuleInfo = ModuleInfo::new("Example", "Example", "serial")
    .roles([Some("Data"), None, None, None])
    .trigger(trigger)
    .starts_on(BusWhat::Data, 0x4f)
    .starts_with(0, Slope::Falling)
    .bit(|opt, _| 1.0 / RATES[opt[0]])
    // a frame is ten bits; 200 bit times would be enough, this shows the hook
    .span(|opt, _| Some(40.0 / RATES[opt[0]]))
    .decoder(|l| Ok(uart(l.need(0, "data line missing")?, l.dt, RATES[l.opt[0]], 8, Parity::None, l.thr, l.hy, false)));

/// The protocol trigger: Frame = every byte; Data = the byte value; the generic rule for the rest.
fn trigger(what: BusWhat, value: u32, p: &Packet) -> Option<bool> {
    match what {
        BusWhat::Frame => Some(p.kind == Kind::Data),
        BusWhat::Data => Some(p.kind == Kind::Data && p.value == value),
        _ => None,
    }
}
