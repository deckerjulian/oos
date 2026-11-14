// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Stand-in for `gpio.rs` on hosts without the Linux GPIO character device (Windows): no line can be opened, so
//! the board code that needs one reports it as missing. Only the host build (demo, script, web remote) uses it.

use std::io;

pub struct Line(());

impl Line {
    pub fn output(name: &str, _value: bool) -> io::Result<Line> {
        Err(io::Error::new(io::ErrorKind::Unsupported, format!("GPIO line {}: no GPIO on this OS", name)))
    }
    pub fn input(name: &str) -> io::Result<Line> {
        Self::output(name, false)
    }
    pub fn set(&self, _v: bool) {}
    pub fn get(&self) -> bool {
        false
    }
}
