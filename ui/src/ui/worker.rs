// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `Worker<J, R>`: a named background thread that computes one job at a time off the frame loop, for the work that is
//! too long for a frame (decoding a long reference, math over millions of points, a SIM generation). The frame loop
//! hands a job over with `submit` only while none is out, and takes the result with `poll`, which never blocks; `busy`
//! says that a job is out and its result not yet taken. Dropping the worker closes its job channel: the thread
//! finishes the job in hand, finds nobody to take the result and ends.
//!
//! To add a background job: put the job's inputs into a struct the thread can own (copies or `Arc`s, never borrows of
//! the `Ui`), write the computation as a function from that job to its result (with the key the result belongs to, so
//! a late result of an older setting can be told apart), keep an `Option<Worker<Job, Result>>` in the part's state and
//! start it with `get_or_insert_with` on the first long job. On each frame `poll` it, keep the result when its key is
//! still current, and `submit` the newest job when it is not `busy`; what waits meanwhile is kept as a key, not as a
//! second copy of the input.
use std::sync::mpsc::{self, TryRecvError, TrySendError};

pub(in crate::ui) struct Worker<J, R> {
    jobs: mpsc::SyncSender<J>,
    results: mpsc::Receiver<R>,
    busy: bool,
}

impl<J: Send + 'static, R: Send + 'static> Worker<J, R> {
    /// Starts the thread `name`, which runs `run` on each job it is handed.
    pub(in crate::ui) fn spawn(name: &str, mut run: impl FnMut(J) -> R + Send + 'static) -> std::io::Result<Self> {
        let (jobs, requests) = mpsc::sync_channel::<J>(1);
        let (finished, results) = mpsc::sync_channel(1);
        std::thread::Builder::new().name(name.into()).spawn(move || {
            while let Ok(job) = requests.recv() {
                if finished.send(run(job)).is_err() {
                    break;
                }
            }
        })?;
        Ok(Self { jobs, results, busy: false })
    }
}

impl<J, R> Worker<J, R> {
    /// A job is out and its result not yet polled.
    pub(in crate::ui) fn busy(&self) -> bool {
        self.busy
    }

    /// Hands `job` to the thread, unless one is already out (`Full`) or the thread is gone (`Disconnected`).
    pub(in crate::ui) fn submit(&mut self, job: J) -> Result<(), TrySendError<J>> {
        if self.busy {
            return Err(TrySendError::Full(job));
        }
        self.jobs.try_send(job)?;
        self.busy = true;
        Ok(())
    }

    /// The result of the job that is out, once it is done. Never blocks.
    pub(in crate::ui) fn poll(&mut self) -> Result<R, TryRecvError> {
        let result = self.results.try_recv()?;
        self.busy = false;
        Ok(result)
    }

    /// Waits up to `timeout` for the result of the job that is out; the bound only turns a hang into a failure.
    #[cfg(test)]
    pub(in crate::ui) fn wait(&mut self, timeout: std::time::Duration) -> Result<R, mpsc::RecvTimeoutError> {
        let result = self.results.recv_timeout(timeout)?;
        self.busy = false;
        Ok(result)
    }

    /// A worker whose thread is the test: it takes the jobs and sends the results.
    #[cfg(test)]
    pub(in crate::ui) fn manual() -> (Self, mpsc::Receiver<J>, mpsc::SyncSender<R>) {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (finished, results) = mpsc::sync_channel(1);
        (Self { jobs, results, busy: false }, requests, finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const BOUND: Duration = Duration::from_secs(20);

    #[test]
    fn one_job_is_out_until_its_result_is_polled() {
        let (release, gate) = mpsc::channel::<()>();
        let mut worker = Worker::spawn("worker-test", move |n: u32| {
            gate.recv().unwrap();
            n * 2
        })
        .unwrap();
        assert!(!worker.busy());
        worker.submit(1).unwrap();
        assert!(worker.busy());
        assert_eq!(worker.submit(2), Err(TrySendError::Full(2)));
        assert_eq!(worker.poll(), Err(TryRecvError::Empty));
        release.send(()).unwrap();
        assert_eq!(worker.wait(BOUND), Ok(2));
        assert!(!worker.busy());
        worker.submit(3).unwrap();
        release.send(()).unwrap();
        assert_eq!(worker.wait(BOUND), Ok(6));
        assert_eq!(worker.poll(), Err(TryRecvError::Empty));
    }

    #[test]
    fn the_thread_carries_its_name_and_state_between_jobs() {
        let mut count = 0;
        let mut worker = Worker::spawn("worker-name", move |()| {
            count += 1;
            (std::thread::current().name().map(String::from), count)
        })
        .unwrap();
        for expected in 1..=2 {
            worker.submit(()).unwrap();
            assert_eq!(worker.wait(BOUND), Ok((Some("worker-name".into()), expected)));
        }
    }

    #[test]
    fn dropping_the_worker_ends_the_thread_after_the_job_in_hand() {
        let (started, entered) = mpsc::channel::<()>();
        let (release, gate) = mpsc::channel::<()>();
        let (ended, exit) = mpsc::channel::<()>();
        struct Exit(mpsc::Sender<()>);
        impl Drop for Exit {
            fn drop(&mut self) {
                self.0.send(()).unwrap();
            }
        }
        let on_exit = Exit(ended);
        let mut worker = Worker::spawn("worker-drop", move |()| {
            let _ = &on_exit;
            started.send(()).unwrap();
            gate.recv().unwrap();
        })
        .unwrap();
        worker.submit(()).unwrap();
        entered.recv_timeout(BOUND).unwrap();
        drop(worker);
        release.send(()).unwrap();
        exit.recv_timeout(BOUND).unwrap();
    }

    #[test]
    fn a_manual_worker_hands_jobs_and_results_to_the_test() {
        let (mut worker, jobs, results) = Worker::<u8, u8>::manual();
        worker.submit(7).unwrap();
        assert_eq!(jobs.try_recv(), Ok(7));
        results.send(8).unwrap();
        assert_eq!(worker.poll(), Ok(8));
        drop(jobs);
        assert_eq!(worker.submit(9), Err(TrySendError::Disconnected(9)));
        assert!(!worker.busy());
    }
}
