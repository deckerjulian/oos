// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The early end of the trigger wait for new settings (upstream f1b2d6c) on the fork's generic batch path
//! (`acquire_batch_until` over `plain_step`), against the host model of the FPGA (docs/fork/upstream-audit.md, P3
//! guard 3). Break it catches: a merge that edits upstream's Poll body and drops the `stop()` check from the fork's
//! copy, so a settings change in NORMAL without a trigger waits the full 2 s again. The stop request is counted, not
//! timed (ADR 0020).

use oos_hal::dho::bus::DmaBuf;
use oos_hal::dho::seg::sim::{engine, Engine};
use oos_hal::dho::seq::{acquire_batch_until, Settings, State, NEW_SETTINGS};
use std::cell::Cell;
use std::time::{Duration, Instant};

#[test]
fn the_trigger_wait_ends_for_new_settings() {
    let s = Settings { tdiv: 1e-6, frames: 10, window: None, ..Settings::default() };
    let mut st = State::handover(1, None);
    let p = st.configure(&s, None, None).unwrap().1;
    // no trigger: the model records no frame, the done flag never comes
    let (_eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000_000, 0, p.ratio));
    let mut dma = DmaBuf::new();
    // new settings arrive at the third look at `stop`
    let asked = Cell::new(0u32);
    let stop = || {
        asked.set(asked.get() + 1);
        asked.get() >= 3
    };
    let r = acquire_batch_until(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma, false, 25, |_| {}, stop);
    assert_eq!(r, Err(NEW_SETTINGS.to_string()));
    // the wait ended at the first yes, not at its 2 s time-out
    assert_eq!(asked.get(), 3);
}

#[test]
fn without_new_settings_the_wait_runs_to_its_end() {
    // the same set-up never stopped: the full 2 s wait ends in the time-out error, not NEW_SETTINGS
    let s = Settings { tdiv: 1e-6, frames: 10, window: None, ..Settings::default() };
    let mut st = State::handover(1, None);
    let p = st.configure(&s, None, None).unwrap().1;
    let (_eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000_000, 0, p.ratio));
    let mut dma = DmaBuf::new();
    let t = Instant::now();
    let r = acquire_batch_until(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma, false, 25, |_| {}, || false);
    assert!(matches!(&r, Err(e) if e != NEW_SETTINGS), "{:?}", r);
    assert!(t.elapsed() >= Duration::from_millis(2000), "{:?}", t.elapsed());
}
