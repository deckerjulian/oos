// SPDX-License-Identifier: GPL-2.0-or-later

//! The idle hook of the ring-fill wait (`acquire_batch_idle`, fork: the UI's acquisition thread draws frames there,
//! docs/fork/perf-plan.md item 1), against the host model of the FPGA. Break it catches: an idle call after the ring
//! is full (the read waits for the caller's work), an idle hook never called, a wait that sleeps on although the caller
//! has work.

use oos_hal::dho::bus::{Bus, DmaBuf};
use oos_hal::dho::seg::sim::{engine, Engine};
use oos_hal::dho::seq::{acquire_batch_idle, Settings, State};
use std::time::{Duration, Instant};

#[test]
fn idle_runs_while_the_ring_fills_and_never_after() {
    let s = Settings { tdiv: 1e-6, frames: 10, window: None, ..Settings::default() };
    let mut st = State::handover(1, None);
    let p = st.configure(&s, None, None).unwrap().1;
    // a trigger every microsecond, one more frame recorded per look at the done flag (0x4080)
    let (eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000, 1, p.ratio));
    let mut dma = DmaBuf::new();
    let (mut calls, mut full_at) = (0u32, None);
    let t = Instant::now();
    let r = acquire_batch_idle(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma, false, 25, |_| {}, || false, || {
        calls += 1;
        // the caller's work takes time; meanwhile the FPGA records the next frame
        let mut e = eng.borrow_mut();
        e.rd(0x4080);
        if e.rd(0x4010) + 1 >= 10 && full_at.is_none() {
            full_at = Some(calls);
        }
        true
    });
    assert_eq!(r, Ok(10));
    assert!(calls > 0, "the hook was never called");
    // the call that filled the ring was the last one: the read followed at once
    assert_eq!(full_at, Some(calls), "{} calls, full after {:?}", calls, full_at);
    // with work to do there was no sleep: far from the 25 ms wait
    assert!(t.elapsed() < Duration::from_millis(20), "{:?}", t.elapsed());
}

#[test]
fn without_work_the_wait_is_unchanged() {
    // the same batch, the hook with nothing to do: the ring never fills in the model, the wait ends after 25 ms
    let s = Settings { tdiv: 1e-6, frames: 10, window: None, ..Settings::default() };
    let mut st = State::handover(1, None);
    let p = st.configure(&s, None, None).unwrap().1;
    let (_eng, mut bar, mut c2h) = engine(Engine::new(0, 1_000_000_000, 1, p.ratio));
    let mut dma = DmaBuf::new();
    let mut calls = 0u32;
    let t = Instant::now();
    let r = acquire_batch_idle(&mut st, &s, &p, &mut bar, &mut c2h, &mut dma, false, 25, |_| {}, || false, || {
        calls += 1;
        false
    });
    assert!(r.as_ref().is_ok_and(|&n| n < 10), "{:?}", r);
    assert!(t.elapsed() >= Duration::from_millis(25) && calls > 10, "{:?} {}", t.elapsed(), calls);
}
