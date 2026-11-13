// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! The early end of the trigger wait for new settings (upstream f1b2d6c) on the fork's generic batch path
//! (`acquire_batch_until` over `plain_step`), against the host model of the FPGA (docs/fork/upstream-audit.md, P3
//! guard 3). Break it catches: a merge that edits upstream's Poll body and drops the `stop()` check from the fork's
//! copy, so a settings change in NORMAL without a trigger waits the full 2 s again.

use oos_hal::dho::bus::DmaBuf;
use oos_hal::dho::seg::sim::{engine, Engine};
use oos_hal::dho::seq::{acquire_batch_until, Settings, State, NEW_SETTINGS};
use std::time::{Duration, Instant};

#[test]
fn the_trigger_wait_ends_for_new_settings() {
    let s = Settings { tdiv: 1e-6, frames: 10, window: None, ..Settings::default() };
    let mut st = State::handover(1, None);
    let p = st.configure(&s, None, None).unwrap().1;
    // no trigger: the model records no frame, the done flag never comes
    let (_eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000_000, 0, p.ratio));
    let mut dma = DmaBuf::new();
    let t = Instant::now();
    let stop_at = Duration::from_millis(30);
    let r = acquire_batch_until(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma, false, 25, |_| {}, || t.elapsed() >= stop_at);
    let took = t.elapsed();
    assert_eq!(r, Err(NEW_SETTINGS.to_string()));
    assert!(took >= stop_at, "{:?}", took);
    // the check runs every ~10 ms; the full wait would be 2 s. Generous for a loaded host.
    assert!(took < Duration::from_millis(500), "{:?}", took);
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
