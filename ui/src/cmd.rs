// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Text commands (host test scripts and the device control FIFO /tmp/oos-ui.ctl):
//!   key <name|0xNN>          front-panel key (names: ch1..ch4, runstop, single, auto, measure, ...)
//!   knob <name> <+n|-n|push> knob turns (hpos vpos hscale vscale tlevel multi1 multi2)
//!   tap <x> <y>              touch tap
//!   drag <x0> <y0> <x1> <y1> touch drag (8 steps)
//!   shot <path>              screenshot (PPM)
//!   frames <n>               host only: advance n frames
//!   storedir <path>          storage directory (setups, CSV, PNG)
//!   textsize <name>          Display > Text size (normal, large, larger)
//!   menu <name>              opens a menu by its title or the entry that opens it (ui/script.rs)
//!   set <label> = <value>    an item of the open menu by its label, to the option shown as <value>
//!   do <label>               presses an item of the open menu (an action or a sub-menu)
//!   pick <label>             opens the grid of all options of an item of the open menu (as a tap on its value)
//!   stat [path]              write fps / frame count / state to /tmp/oos-ui.stat
//!   quit

use crate::gfx::Canvas;
use crate::model::TEXT_SIZES;
use crate::ui::*;

pub enum Out {
    None,
    Frames(u32),
    Shot(String),
}

/// Commands the web remote may run: its own buttons and touch gestures. The others name files or end the program;
/// they stay with the local control FIFO and the host scripts.
pub fn remote_ok(line: &str) -> bool {
    matches!(line.split_whitespace().next(), Some("key" | "knob" | "tap" | "drag"))
}

pub fn run(ui: &mut Ui, line: &str) -> Out {
    let w: Vec<&str> = line.split_whitespace().collect();
    if w.is_empty() || w[0].starts_with('#') {
        return Out::None;
    }
    let num = |s: &str| -> i32 { s.trim_start_matches('+').parse().unwrap_or(0) };
    match w[0] {
        "key" if w.len() > 1 => {
            let code = if let Some(h) = w[1].strip_prefix("0x") {
                u8::from_str_radix(h, 16).unwrap_or(0)
            } else {
                KEY_NAMES.iter().find(|(n, _)| *n == w[1]).map(|x| x.1).unwrap_or(0)
            };
            ui.key(code);
        }
        "knob" if w.len() > 2 => {
            if let Some(&(_, cw)) = KNOB_NAMES.iter().find(|(n, _)| *n == w[1]) {
                if w[2] == "push" {
                    ui.key(cw + 2);
                } else {
                    // a turn, not a loop that stops the screen for minutes
                    let n = num(w[2]).clamp(-100, 100);
                    for _ in 0..n.abs() {
                        ui.key(if n > 0 { cw } else { cw + 1 });
                        ui.now += 0.2; // no acceleration for scripted turns
                    }
                }
            }
        }
        "tap" if w.len() > 2 => {
            let (x, y) = (num(w[1]), num(w[2]));
            ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x, y });
            ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x, y });
        }
        "drag" if w.len() > 4 => {
            let (x0, y0, x1, y1) = (num(w[1]), num(w[2]), num(w[3]), num(w[4]));
            ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x: x0, y: y0 });
            for s in 1..=8 {
                let x = x0 + (x1 - x0) * s / 8;
                let y = y0 + (y1 - y0) * s / 8;
                ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x, y });
            }
            if w.get(5) != Some(&"hold") {
                ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: x1, y: y1 });
            }
        }
        "pinch" if w.len() > 8 => {
            // pinch x0 y0 x1 y1 -> x0' y0' x1' y1' (two fingers, 8 steps)
            let v: Vec<i32> = w[1..9].iter().map(|s| num(s)).collect();
            ui.handle(Ev::Touch { id: 0, phase: Phase::Down, x: v[0], y: v[1] });
            ui.handle(Ev::Touch { id: 1, phase: Phase::Down, x: v[2], y: v[3] });
            for s in 1..=8 {
                let f = |a: i32, b: i32| a + (b - a) * s / 8;
                ui.handle(Ev::Touch { id: 0, phase: Phase::Move, x: f(v[0], v[4]), y: f(v[1], v[5]) });
                ui.handle(Ev::Touch { id: 1, phase: Phase::Move, x: f(v[2], v[6]), y: f(v[3], v[7]) });
            }
            ui.handle(Ev::Touch { id: 1, phase: Phase::Up, x: v[6], y: v[7] });
            ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: v[4], y: v[5] });
        }
        "release" => ui.handle(Ev::Touch { id: 0, phase: Phase::Up, x: 0, y: 0 }),
        // by name: the rest of the line (labels have spaces)
        "menu" | "set" | "do" | "pick" => {
            let rest = line.trim_start()[w[0].len()..].trim();
            let r = match w[0] {
                "menu" => ui.script_menu(rest),
                "do" => ui.script_do(rest),
                "pick" => ui.script_pick(rest),
                _ => match rest.split_once(" = ") {
                    Some((l, v)) => ui.script_set(l, v),
                    None => Err("set <label> = <value>".into()),
                },
            };
            if let Err(e) = r {
                eprintln!("oos-ui: {}: {}", line.trim(), e);
            }
        }
        "frames" if w.len() > 1 => return Out::Frames(num(w[1]).max(0) as u32),
        "shot" if w.len() > 1 => return Out::Shot(w[1].to_string()),
        "quit" => ui.quit = true,
        "selfcal" if w.len() > 1 => ui.selfcal_remote_full(w[1], w[2..].contains(&"zero"), w[2..].contains(&"adc")),
        "storedir" if w.len() > 1 => ui.store_dir = w[1].into(),
        "textsize" if w.len() > 1 => match TEXT_SIZES.iter().find(|t| t.name().eq_ignore_ascii_case(w[1])) {
            Some(&t) => ui.m.text_size = t,
            None => eprintln!("oos-ui: unknown text size: {}", w[1]),
        },
        "stat" => {
            let _ = std::fs::write(w.get(1).copied().unwrap_or("/tmp/oos-ui.stat"), stat_text(ui));
        }
        _ => eprintln!("oos-ui: unknown command: {}", line),
    }
    Out::None
}

/// The running state in one line: the `stat` command and `:SYSTem:STATe?`.
pub fn stat_text(ui: &Ui) -> String {
    let e = ui.fast_edges;
    let gpu = ui.t_gpu.map_or(String::new(), |t| format!(" gpu layers {:.1} copy {:.1} wait {:.1} (upload {:.1} draw {:.1} read {:.1}) ms", t[0], t[1], t[2], t[3], t[4], t[5]));
    let p = ui.t_parts;
    let gpu = format!(" parts meas {:.1} analyse {:.1} pane {:.1} max {:.1} ms{}", p[0], p[1], p[2], p[3], gpu);
    let (acq, used, dropped) = crate::drawpipe::counters().read();
    let measured = crate::drawpipe::counters().measured.load(std::sync::atomic::Ordering::Relaxed);
    let gpu = format!("{} pipeline {:?} interp {:?} acquired {} used {} dropped {} measured {}", gpu, crate::drawpipe::mode(), crate::sparse::interp(), acq, used, dropped, measured);
    format!(
        "fps {:.1} frames {} records {} run {:?} status {:?} t {:.1} step {:.1} ms (acq {:.1} draw {:.1}) render {:.1} ms present {:.1} ms edges {} {} {} {} off {:.0} {:.0}{}\n",
        ui.fps, ui.frames, ui.records, ui.m.run, ui.m.status, ui.now, ui.t_step, ui.t_acq, ui.t_draw, ui.t_render, ui.t_present, e[0], e[1], e[2], e[3], ui.fast_off.0, ui.fast_off.1, gpu
    )
}

pub fn write_ppm(cv: &Canvas, path: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut buf = Vec::with_capacity((cv.w * cv.h * 3) as usize + 32);
    write!(buf, "P6\n{} {}\n255\n", cv.w, cv.h)?;
    for p in &cv.px {
        buf.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(path, buf)
}
