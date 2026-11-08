// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Progress of the start-up for the start-up screen: rcS, `oos-fpgad fpga-boot` and `oos-fpgad cold-boot` append lines
//! to one file in /tmp, oos-ui reads it. A line is `<uptime s>\t<state>\t<stage>\t<detail>`:
//!
//! - `run`: the stage started or its detail changed (`stage` = the label on the screen);
//! - `ok` / `warn` / `fail`: the stage's result (a warning does not stop the start-up);
//! - `end`: the start-up is complete, every stage still running is ok (written by rcS after oos-fpgad exited, so the
//!   GPIO lines oos-fpgad held are free when oos-ui opens them);
//! - `abort`: the start-up stopped, a stage still running failed.
//!
//! The uptimes are the boot-time profile of the start-up; the file stays in /tmp until the next power cycle.

/// Where the start-up writes its progress.
pub const PATH: &str = "/tmp/oos-boot.status";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Run,
    Ok,
    Warn,
    Fail,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub label: String,
    pub state: State,
    pub detail: String,
    /// uptime of the first line and of the result (None while running)
    pub t0: f64,
    pub t1: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub stages: Vec<Stage>,
    /// Some(Ok) after `end`, Some(Err(detail)) after `abort`
    pub finished: Option<Result<(), String>>,
    /// uptime of the last line
    pub t: f64,
}

impl View {
    pub fn warnings(&self) -> impl Iterator<Item = &Stage> {
        self.stages.iter().filter(|s| s.state == State::Warn)
    }
    pub fn failed(&self) -> Option<&Stage> {
        self.stages.iter().find(|s| s.state == State::Fail)
    }
}

/// One line of the file.
pub fn line(t: f64, state: &str, stage: &str, detail: &str) -> String {
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    format!("{:.2}\t{}\t{}\t{}\n", t, state, clean(stage), clean(detail))
}

/// Appends a line (errors are ignored: the start-up must not stop because /tmp is full).
pub fn append(path: &str, state: &str, stage: &str, detail: &str) {
    use std::io::Write;
    let l = line(uptime(), state, stage, detail);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(l.as_bytes());
    }
}

/// The file as stages in the order they first appear; unknown or broken lines are skipped.
pub fn parse(text: &str) -> View {
    let mut v = View::default();
    for l in text.lines() {
        let f: Vec<&str> = l.splitn(4, '\t').collect();
        if f.len() < 2 {
            continue;
        }
        let Ok(t) = f[0].trim().parse::<f64>() else { continue };
        let (label, detail) = (f.get(2).copied().unwrap_or("").trim(), f.get(3).copied().unwrap_or("").trim());
        v.t = t;
        let state = match f[1] {
            "run" => State::Run,
            "ok" => State::Ok,
            "warn" => State::Warn,
            "fail" => State::Fail,
            "end" | "abort" => {
                let fin = if f[1] == "end" { State::Ok } else { State::Fail };
                for s in v.stages.iter_mut().filter(|s| s.state == State::Run) {
                    (s.state, s.t1) = (fin, Some(t));
                }
                v.finished = Some(if f[1] == "end" { Ok(()) } else { Err(detail.to_string()) });
                continue;
            }
            _ => continue,
        };
        if label.is_empty() {
            continue;
        }
        let i = match v.stages.iter().position(|s| s.label == label) {
            Some(i) => i,
            None => {
                v.stages.push(Stage { label: label.into(), state, detail: String::new(), t0: t, t1: None });
                v.stages.len() - 1
            }
        };
        let s = &mut v.stages[i];
        // a warning stays a warning when the stage reports ok later; a failure stays a failure
        s.state = match (s.state, state) {
            (State::Fail, _) => State::Fail,
            (State::Warn, State::Ok | State::Run) => State::Warn,
            (_, n) => n,
        };
        s.t1 = if state == State::Run && s.state == State::Run { None } else { Some(t) };
        if !detail.is_empty() {
            s.detail = detail.into();
        } else if state == State::Ok && s.state == State::Ok {
            // "loading ..." is no longer true once the stage is done
            s.detail.clear();
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_up_file() {
        let text = [
            line(4.1, "ok", "Linux", "kernel 6.12.115"),
            line(4.2, "run", "FPGA image", "loading from flash"),
            line(4.6, "warn", "Calibration data", "cal_ddr.hex missing: defaults"),
            line(14.3, "run", "FPGA image", "PCIe link"),
            "garbage\n".into(),
            line(14.4, "ok", "FPGA image", "0xe2072115"),
            line(14.4, "run", "Sample clock (PLL lock)", ""),
        ]
        .concat();
        let v = parse(&text);
        assert_eq!(v.stages.iter().map(|s| (s.label.as_str(), s.state)).collect::<Vec<_>>(), [("Linux", State::Ok), ("FPGA image", State::Ok), ("Calibration data", State::Warn), ("Sample clock (PLL lock)", State::Run)]);
        assert_eq!((v.stages[1].t0, v.stages[1].t1, v.stages[1].detail.as_str()), (4.2, Some(14.4), "0xe2072115"));
        assert_eq!(v.warnings().count(), 1);
        assert!(v.finished.is_none() && v.t == 14.4);
        // abort: the running stage failed
        let v2 = parse(&(text.clone() + &line(16.0, "abort", "", "see /tmp/oos-cold.log")));
        assert_eq!(v2.failed().map(|s| s.label.as_str()), Some("Sample clock (PLL lock)"));
        assert_eq!(v2.finished, Some(Err("see /tmp/oos-cold.log".into())));
        // end: complete, the warning stays
        let v3 = parse(&(text + &line(18.0, "ok", "Calibration data", "") + &line(19.0, "end", "", "")));
        assert_eq!(v3.finished, Some(Ok(())));
        assert!(v3.failed().is_none() && v3.warnings().count() == 1 && v3.stages[3].state == State::Ok);
        // tabs and newlines in a detail do not break the format
        assert_eq!(parse(&line(1.0, "fail", "X", "a\tb\nc")).stages[0].detail, "a b c");
    }
}

/// Seconds since the kernel started.
fn uptime() -> f64 {
    std::fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next().and_then(|t| t.parse().ok())).unwrap_or(0.0)
}
