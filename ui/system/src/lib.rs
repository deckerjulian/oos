// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! oos-system: what oos-ui needs of the Linux system, without the scope's state (ADR 0064): the network helpers and
//! the services the screen only switches (mDNS, SNTP, SSH, USBTMC, `net.allow`), the clock, `system.conf`, thread
//! placement, JSON and base64. It names no module of oos-ui; the compiler keeps it that way. oos-ui brings each
//! module into its crate root (`main.rs`), so `crate::sysconf` and the other paths of oos-ui stay as they were.

pub mod base64;
pub mod clock;
pub mod ethlink;
pub mod hostname;
pub mod ifaddrs;
pub mod json;
pub mod mdns;
pub mod net;
pub mod net6;
pub mod netacl;
pub mod placement;
pub mod remote;
pub mod sntp;
pub mod ssh;
pub mod sysconf;
pub mod system;
pub mod usbtmc;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

// the generator of the hostile-input tests (ADR 0047), one file for every crate
#[cfg(test)]
#[path = "../../../hal/src/hostile/gen.rs"]
mod hostile;
