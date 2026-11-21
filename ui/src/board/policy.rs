// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the acquisition thread decides by, of the live compression (docs/fork/compress.md) and of the raw reads of
//! deep records for the measurements (docs/fork/deep-meas.md): the switches, the limits, the factor and the rate
//! limits. They belong to the board layer because the thread reads them (ADR 0074; the board may name only base and
//! the model, ADR 0049); the signal modules `compress` and `deepmeas`, which draw and measure what the thread
//! delivers, name them here (`policy::compress::k_for`, `policy::deep::Want`).

pub mod compress;
pub mod deep;

/// A switch of system.conf the acquisition thread decides by: read once (the files do not change while the scope
/// runs), and for tests set for the calling thread (`force`). Always a `static` (the override is found by its place).
pub struct Switch {
    read: fn() -> bool,
    on: std::sync::OnceLock<bool>,
}

#[cfg(test)]
thread_local! {
    /// tests: the switches set for this thread (`Switch::force`), by their place
    static FORCED: std::cell::RefCell<Vec<(usize, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Switch {
    pub const fn new(read: fn() -> bool) -> Self {
        Switch { read, on: std::sync::OnceLock::new() }
    }

    /// The switch: the test's setting for this thread, else `read` (once).
    pub fn get(&self) -> bool {
        #[cfg(test)]
        if let Some(on) = FORCED.with(|f| f.borrow().iter().find(|x| x.0 == self.id()).map(|x| x.1)) {
            return on;
        }
        *self.on.get_or_init(self.read)
    }

    /// Sets the switch for the calling thread, `None` back to `read` (tests).
    #[cfg(test)]
    pub fn force(&self, on: Option<bool>) {
        FORCED.with(|f| {
            let mut f = f.borrow_mut();
            f.retain(|x| x.0 != self.id());
            f.extend(on.map(|on| (self.id(), on)));
        });
    }

    #[cfg(test)]
    fn id(&self) -> usize {
        self as *const Switch as usize
    }
}
