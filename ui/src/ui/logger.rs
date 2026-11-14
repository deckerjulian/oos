// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Data logger: the measurements as CSV rows at a fixed interval, into a new file where the storage page saves.

use super::*;
use std::io::Write;

/// Interval steps (s).
pub const LOG_STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0];
/// A log ends here: a stick left in for a year must not fill up.
const LOG_ROWS_MAX: u64 = 1_000_000;

#[derive(Default)]
pub struct Logger {
    /// the file while logging
    pub file: Option<std::path::PathBuf>,
    next: f64,
    start: f64,
    pub rows: u64,
    /// the first line: a measurement added or removed would no longer fit under it
    head: String,
}

#[cfg(test)]
mod catalog_tests {
    use super::*;

    #[test]
    fn catalog_log_headers_name_required_sources_and_pending_values_stay_empty() {
        let mut ui=Ui::new(Source::Demo{t:0.0});ui.m.meas.clear();ui.m.meas2.clear();
        ui.m.cat=vec![(90,WaveId::Ref(0),WaveId::Ref(1),WaveId::Ch(2),WaveId::Ch(3)),
            (92,WaveId::Ref(0),WaveId::Ref(1),WaveId::Ch(2),WaveId::Ch(3))];
        let header=ui.log_head();
        assert!(header.contains("R1-R2 Power Quality (W)"),"{header}");
        assert!(header.contains("R2 Inrush Current (A)"),"{header}");
        assert!(!header.contains("CH3") && !header.contains("CH4"),"unused sources must not be claimed");
        let rows=ui.meas_rows();assert_eq!(rows.len(),2);assert!(rows.iter().all(|row|row.v.is_nan()));
        assert_eq!(ui.wfm_total,0);
    }
}

impl Ui {
    fn log_head(&self) -> String {
        let cols: Vec<String> = self.meas_rows().iter().enumerate().map(|(i,r)| {
            if i < self.m.meas.len()+self.m.meas2.len() {
                format!("{} {} ({})",r.name.replace(','," "),self.wave_name(r.src),r.unit)
            } else { format!("{} ({})",self.results_name(i,r).replace(','," "),r.unit) }
        }).collect();
        format!("Elapsed (s),Unix time (s),{}", cols.join(","))
    }

    /// Logger on or off from outside the menu (SCPI).
    pub fn log_set(&mut self, on: bool) {
        match (on, self.log.file.is_some()) {
            (true, false) => self.log_start(),
            (false, true) => self.log_stop("Log ended"),
            _ => {}
        }
    }

    pub(super) fn log_start(&mut self) {
        if self.meas_rows().is_empty() {
            return self.toast("No measurements: tap Measure");
        }
        let dir = self.save_dir();
        let p = self.new_file(&dir, "log", "csv");
        let head = self.log_head();
        if let Err(e) = crate::store::write(&p, format!("{}\n", head).as_bytes()) {
            return self.toast(&format!("Log not started: {}", e));
        }
        self.toast(&format!("Logging to {}{}", crate::store::file_name(&p), self.saved_where()));
        self.log = Logger { file: Some(p), next: self.now, start: self.now, rows: 0, head };
    }

    pub(super) fn log_stop(&mut self, why: &str) {
        let Some(p) = self.log.file.take() else { return };
        self.flush_stick();
        self.toast(&format!("{}: {} rows in {}", why, self.log.rows, crate::store::file_name(&p)));
    }

    /// One row when it is due. The measurements are the latest there are: a stopped scope logs the same values on.
    pub(super) fn log_tick(&mut self) {
        let Some(p) = self.log.file.clone() else { return };
        if self.now < self.log.next {
            return;
        }
        if self.log_head() != self.log.head {
            return self.log_stop("Measurements changed, log ended");
        }
        let step = LOG_STEPS[self.m.log_ivl.min(LOG_STEPS.len() - 1)];
        self.log.next += step;
        // after a stall: go on from now, not with a burst of rows
        if self.log.next < self.now {
            self.log.next = self.now + step;
        }
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let mut row = format!("{:.3},{:.3}", self.now - self.log.start, unix);
        for r in self.meas_rows() {
            row.push(',');
            if r.v.is_finite() {
                row.push_str(&format!("{:e}", r.v));
            }
        }
        row.push('\n');
        let res = crate::store::check_target(&p).and_then(|_| std::fs::OpenOptions::new().append(true).open(&p)).and_then(|mut f| f.write_all(row.as_bytes()));
        if let Err(e) = res {
            self.log.file = None;
            return self.toast(&format!("Log ended: {}", e));
        }
        self.log.rows += 1;
        if self.log.rows >= LOG_ROWS_MAX {
            self.log_stop("Log full");
        }
    }

    pub(super) fn log_info(&self) -> String {
        match &self.log.file {
            Some(p) => format!("{}: {} rows", crate::store::file_name(p), self.log.rows),
            None => format!("{} measurements, off", self.meas_rows().len()),
        }
    }
}
