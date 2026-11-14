// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OperationState { Pending, Complete, Cancelled }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OperationToken { Auto(u64), Sim(u64, u64), Single(u64) }
/// Exact finite jobs at the barrier. Later captures never complete a cancelled job.
#[derive(Clone, Debug)]
pub(crate) struct Operations {
    epoch: u64,
    auto: Option<u64>,
    sim: Option<(u64, u64)>,
    single: Option<(u64, bool)>,
}
impl Operations {
    pub(crate) fn state(&self, ui: &Ui) -> OperationState {
        use OperationState::*;
        if self.epoch != ui.workflow_epoch { return Cancelled; }
        let now = ui.scpi_operations();
        let mut pending = false;
        for (token, active) in [
            (self.auto.map(OperationToken::Auto), self.auto.is_some() && self.auto == now.auto),
            (self.sim.map(|(epoch,id)| OperationToken::Sim(epoch,id)), self.sim.is_some() && self.sim == now.sim),
            (self.single.map(|(arm,_)| OperationToken::Single(arm)), self.single.is_some() && self.single == now.single),
        ] {
            if let Some(token) = token {
                if !ui.scpi_completed.contains(&token) {
                    if active { pending = true; } else { return Cancelled; }
                }
            }
        }
        if pending { Pending } else { Complete }
    }
}
impl Ui {
    pub(super) fn scpi_finish_operation(&mut self, token: OperationToken) {
        // Receipts are metadata only, bounded independently of waveform history.
        // An expired fence is interrupted rather than falsely completed.
        if self.scpi_completed.back() != Some(&token) {
            if self.scpi_completed.len() == 128 { self.scpi_completed.pop_front(); }
            self.scpi_completed.push_back(token);
        }
    }
    pub(crate) fn scpi_arm_operation(&mut self) {
        self.operation_serial = self.operation_serial.wrapping_add(1);
        self.single_operation = self.operation_serial;
    }
    pub(crate) fn scpi_operations(&self) -> Operations {
        Operations {
            epoch: self.workflow_epoch,
            auto: self.auto_operation(), sim: self.sim_operation(),
            single: (self.m.trig_mode == TrigMode::Single && self.m.run == RunState::Run)
                .then(|| (self.single_operation, self.record_mode())),
        }
    }
}
