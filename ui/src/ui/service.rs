// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `Service<J, R>`: a named thread that lives as long as the part that started it and reports more than once, for a
//! connection or a state of its own that the frame loop must not wait for (WLAN, Bluetooth, the clock; ADR 0061). A
//! job is something else: one input, one result, `Worker<J, R>`.
//!
//! The part keeps a `Service` (empty at first), starts it with `start` when it first needs it, hands it jobs with
//! `send` and takes its reports with `take`, which never blocks, on each frame. What the thread does is its function:
//! it reads jobs from the receiver it is given and sends reports on the sender, and ends when the job channel is closed,
//! that is when the `Service` is dropped. In `cargo test` no thread is started: the tests feed the part's reports
//! themselves, and a test must not reach a daemon of the computer it runs on.

use std::sync::mpsc::{channel, Receiver, Sender};

pub(in crate::ui) struct Service<J, R> {
    tx: Option<Sender<J>>,
    rx: Option<Receiver<R>>,
}

impl<J, R> Default for Service<J, R> {
    fn default() -> Self {
        Self { tx: None, rx: None }
    }
}

impl<J: Send + 'static, R: Send + 'static> Service<J, R> {
    /// Starts the thread `name` with `run`, unless it runs already (or this is `cargo test`). When the thread cannot
    /// be started, the service stays empty and the next `start` tries again.
    pub(in crate::ui) fn start(&mut self, name: &str, run: fn(Receiver<J>, Sender<R>)) {
        if !cfg!(test) {
            self.spawn(name, run);
        }
    }

    /// `start` without the guard of the tests.
    fn spawn(&mut self, name: &str, run: fn(Receiver<J>, Sender<R>)) {
        if self.tx.is_some() {
            return;
        }
        let (tx, jobs) = channel();
        let (out, rx) = channel();
        if std::thread::Builder::new().name(name.into()).spawn(move || run(jobs, out)).is_ok() {
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
    }
}

impl<J, R> Service<J, R> {
    /// Hands `job` to the thread; false when it is not started or has ended.
    pub(in crate::ui) fn send(&self, job: J) -> bool {
        self.tx.as_ref().is_some_and(|tx| tx.send(job).is_ok())
    }

    /// The next report of the thread, if one is there. Never blocks.
    pub(in crate::ui) fn take(&self) -> Option<R> {
        self.rx.as_ref().and_then(|rx| rx.try_recv().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn echo(jobs: Receiver<u32>, out: Sender<u32>) {
        while let Ok(j) = jobs.recv() {
            let _ = out.send(j + 1);
        }
    }

    #[test]
    fn a_service_starts_no_thread_in_the_tests_and_takes_nothing() {
        let mut s: Service<u32, u32> = Service::default();
        s.start("echo", echo);
        assert!(!s.send(1), "no thread in cargo test");
        assert!(s.take().is_none());
    }

    /// The thread says whether it has the name `echo-svc` (1) or not (0), then answers each job.
    fn named(jobs: Receiver<u32>, out: Sender<u32>) {
        let _ = out.send(u32::from(std::thread::current().name() == Some("echo-svc")));
        echo(jobs, out);
    }

    #[test]
    fn a_started_service_runs_one_named_thread_answers_its_jobs_and_ends_with_it() {
        let mut s: Service<u32, u32> = Service::default();
        s.spawn("echo-svc", named);
        s.spawn("echo-svc", named);
        let next = |s: &Service<u32, u32>| s.rx.as_ref().and_then(|rx| rx.recv_timeout(Duration::from_secs(5)).ok());
        assert_eq!(next(&s), Some(1), "the thread has the name it was given");
        assert!(s.send(41));
        // a second thread would have said its name first
        assert_eq!(next(&s), Some(42), "a second start starts nothing");
        let (tx, rx) = (s.tx.take(), s.rx.take());
        drop(tx);
        let end = rx.map(|rx| rx.recv_timeout(Duration::from_secs(5)));
        assert!(
            matches!(end, Some(Err(std::sync::mpsc::RecvTimeoutError::Disconnected))),
            "the thread ended: {:?}",
            end
        );
    }
}
