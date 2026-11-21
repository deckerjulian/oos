// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The finite jobs a remote command can wait for with `*OPC?`, `*OPC` and `*WAI`: an autoset, a SIM generation, a
//! SINGLE capture and a STOP until its record is in place (#24). Each gets a token when it starts; a fence taken at the command stays pending until those jobs end,
//! and counts as cancelled when the workflow starts over. A new kind of job that a command waits for gets its token
//! here.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OperationState {
    Pending,
    Complete,
    Cancelled,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OperationToken {
    Auto(u64),
    Sim(u64, u64),
    Single(u64),
    Stop(u64),
}
/// The numbers of the jobs and the receipts of the finished ones (ADR 0062).
#[derive(Default)]
pub(super) struct Ledger {
    /// the last number given to a job (an autoset or a SINGLE)
    pub serial: u64,
    /// the number of the SINGLE capture armed last
    pub single: u64,
    /// the receipts of the finished jobs, the latest 128
    pub completed: std::collections::VecDeque<OperationToken>,
}

impl Ledger {
    /// The next job's number.
    pub fn next(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }

    /// A SINGLE capture is armed: it gets the next number.
    pub fn arm_single(&mut self) {
        self.single = self.next();
    }
}

/// Exact finite jobs at the barrier. Later captures never complete a cancelled job.
#[derive(Clone, Debug)]
pub(crate) struct Operations {
    epoch: u64,
    auto: Option<u64>,
    sim: Option<(u64, u64)>,
    single: Option<(u64, bool)>,
    stop: Option<u64>,
}
impl Operations {
    pub(crate) fn state(&self, ui: &Ui) -> OperationState {
        use OperationState::*;
        if self.epoch != ui.workflow_epoch {
            return Cancelled;
        }
        let now = ui.scpi_operations();
        let mut pending = false;
        for (token, active) in [
            (self.auto.map(OperationToken::Auto), self.auto.is_some() && self.auto == now.auto),
            (self.sim.map(|(epoch, id)| OperationToken::Sim(epoch, id)), self.sim.is_some() && self.sim == now.sim),
            (
                self.single.map(|(arm, _)| OperationToken::Single(arm)),
                self.single.is_some() && self.single == now.single,
            ),
            (self.stop.map(OperationToken::Stop), self.stop.is_some() && self.stop == now.stop),
        ] {
            if let Some(token) = token {
                if !ui.ops.completed.contains(&token) {
                    if active {
                        pending = true;
                    } else {
                        return Cancelled;
                    }
                }
            }
        }
        if pending {
            Pending
        } else {
            Complete
        }
    }
}
impl Ui {
    pub(super) fn scpi_finish_operation(&mut self, token: OperationToken) {
        // Receipts are metadata only, bounded independently of waveform history.
        // An expired fence is interrupted rather than falsely completed.
        let done = &mut self.ops.completed;
        if done.back() != Some(&token) {
            if done.len() == 128 {
                done.pop_front();
            }
            done.push_back(token);
        }
    }
    pub(crate) fn scpi_arm_operation(&mut self) {
        self.ops.arm_single();
    }
    pub(crate) fn scpi_operations(&self) -> Operations {
        Operations {
            epoch: self.workflow_epoch,
            auto: self.auto_operation(),
            sim: self.sim_operation(),
            single: (self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run)
                .then(|| (self.ops.single, self.record_mode())),
            // a STOP of the hardware not yet taken by the step, or waiting for its last record (`Stop::final_until`)
            stop: (self.src.is_hw()
                && self.m.run == RunState::Stop
                && (self.seen.running || self.stop.final_until.is_some()))
            .then_some(self.stop.final_req),
        }
    }
}

#[cfg(test)]
impl Ui {
    /// The receipts at their bound, as after many finished operations: from here on the list takes no more room (the
    /// soak test compares the memory in use from one cycle to the next, `soak.rs`).
    pub(crate) fn scpi_receipts_fill(&mut self) {
        for n in 0.. {
            let before = self.ops.completed.len();
            self.scpi_finish_operation(OperationToken::Single(u64::MAX - n));
            if self.ops.completed.len() == before {
                break;
            }
        }
    }
}
