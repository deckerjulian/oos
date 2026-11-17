// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! `exec_cmd`: one command (a header and its argument) to its answer. It tries the headers in a fixed order: the
//! IEEE 488.2 common commands (`*IDN?`, `*OPC`, `*ESR?`, `*CLS`, `*RST`), then `:RUN`, `:STOP`, `:SINGle`,
//! `:TFORce`, `:AUToscale`, `:CLEar`, then the subsystems. `:CHANnel<n>`, `:TRIGger`, `:ACQuire`, `:WAVeform`'s
//! settings, `:VIEW`, `:CALibrate`, `:BODE`, `:SOURce` (the generator) and the analyser pages' queries are answered
//! here; every other subsystem hands its nodes to the `*_cmd` method in its own file (measure.rs, system.rs, ...).
//! The order is part of the behaviour: a header an earlier branch takes never reaches a later one. A new command of
//! a subsystem with its own file goes there; one of a subsystem answered here goes into its branch here. Either way
//! it comes with its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::decode::bus_index;
use super::display::bmp;
use super::math::math_index;
use super::ser;
use super::{arg_item, arg_rest, block, e, kw, nearest, num, parse_bool, parse_ch, parse_wave, suffix, Scpi};
use crate::board::{board, Board};
use crate::gfx::Canvas;
use crate::model::*;
use crate::ui::Ui;

/// Fine scale (`:CHANnel<n>:VERNier ON`): the first row of the calibrated scale ladder at or above `want` (V/div at the
/// BNC), so the range never cuts the signal off, as `vscale` step and factor on it.
fn fine_scale(want: f32) -> (usize, f32) {
    let rows = board().vscale_steps_nv();
    let nv = want as f64 * 1e9 * (1.0 - 1e-6);
    let row = rows.iter().copied().find(|&k| k as f64 >= nv).unwrap_or(rows[rows.len() - 1]);
    let row = (row as f64 * 1e-9) as f32;
    let i = VSCALES.iter().rposition(|&s| s <= row * 1.000001).unwrap_or(0);
    let f = row / VSCALES[i];
    (i, if (f - 1.0).abs() < 1e-6 { 1.0 } else { f })
}

impl Scpi {
    pub(super) fn exec_cmd(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
        if cmd.is_empty() {
            return None;
        }
        let (head, arg) = match cmd.find(char::is_whitespace) {
            Some(i) => (&cmd[..i], cmd[i..].trim()),
            None => (cmd, ""),
        };
        let query = head.ends_with('?');
        let head = head.trim_end_matches('?');
        let up = head.to_ascii_uppercase();
        let t = |s: &str| Some(s.as_bytes().to_vec());
        // common commands
        match up.as_str() {
            "*IDN" => {
                let p = oos_hal::model::platform();
                return t(&format!(
                    "OOS,{},{},oos-ui {}",
                    p.caps.name,
                    p.serial.as_deref().unwrap_or("0"),
                    env!("CARGO_PKG_VERSION")
                ));
            }
            "*OPC" if !query => {
                self.poll_events(ui);
                if self.opc_events.len() < 64 {
                    self.opc_events.push(ui.scpi_operations());
                } else {
                    self.push_err("-350,\"Too many pending OPC events\"".into());
                }
                self.poll_events(ui);
                return None;
            }
            "*ESR" if query => {
                self.poll_events(ui);
                let status = std::mem::take(&mut self.event_status);
                return t(&status.to_string());
            }
            "*CLS" if !query => {
                self.err.clear();
                self.event_status = 0;
                self.opc_events.clear();
                return None;
            }
            "*RST" if !query => {
                self.event_status = 0;
                self.opc_events.clear();
                ui.key(crate::ui::key::DEFAULT);
                return None;
            }
            _ => {}
        }
        let nodes: Vec<&str> = head.trim_start_matches(':').split(':').collect();
        if !query && nodes.len() == 1 {
            let command = if kw(nodes[0], "STOP") {
                Some(0)
            } else if kw(nodes[0], "RUN") {
                Some(1)
            } else if kw(nodes[0], "SINGle") {
                Some(2)
            } else {
                None
            };
            if command.is_some_and(|command| ui.mixed_bus_command(command)) {
                return None;
            }
        }
        // Rigol: a forced trigger releases Normal or Single when the level never crosses. On a reference or SIM
        // session that is the replay flag (`force_trigger`); on the live hardware it pulses the FPGA force bit.
        if !query
            && ((nodes.len() == 1 && kw(nodes[0], "TFORce"))
                || (nodes.len() == 2 && kw(nodes[0], "TRIGger") && kw(nodes[1], "FORCe")))
        {
            ui.force_trigger();
            return None;
        }
        if nodes.len() == 3
            && kw(nodes[0], "DECode")
            && kw(nodes[1], "CPHY")
            && (kw(nodes[2], "RATE") || kw(nodes[2], "SRATe"))
        {
            return self.decode_cphy_rate_cmd(ui, query, arg);
        }
        if nodes.len() == 2 && kw(nodes[0], "DECode") {
            return self.decode_cmd(ui, nodes[1], query, arg);
        }
        if nodes.len() >= 2 && kw(nodes[0], "LA") {
            return self.la_cmd(ui, &nodes[1..], query, arg);
        }
        // fork debug: raw measure-unit slots (the slot map of the auto set-up's levels)
        if nodes.len() == 2 && kw(nodes[0], "DEBug") && kw(nodes[1], "SLOTs") && query {
            return t(&ui.debug_slots());
        }
        if nodes.len() == 1 && kw(nodes[0], "AUToscale") && query {
            return t(if ui.auto_running() { "1" } else { "0" });
        }
        if nodes.len() == 1 && kw(nodes[0], "AUToscale") {
            ui.key(crate::ui::key::AUTO);
            return None;
        }
        if !query && nodes.len() == 1 && kw(nodes[0], "CLEar") {
            ui.key(crate::ui::key::CLEAR);
            return None;
        }
        if !query && nodes.len() == 1 && kw(nodes[0], "STOP") && ui.sim_generating() {
            let _ = ui.sim_play_set(false);
            ui.mark_changed();
            return None;
        }
        // A reference capture uses the same acquisition state as the screen.
        // RUN and STOP are assignments, so repeating either must not toggle it.
        // Call the shared record handler directly: an open UI dialog must not
        // consume a remote acquisition command as a dialog-dismissal key.
        if !query
            && nodes.len() == 1
            && ui.record_mode()
            && ["RUN", "STOP", "SINGle"].iter().any(|command| kw(nodes[0], command))
        {
            let key = crate::ui::key::RUNSTOP;
            if kw(nodes[0], "RUN") {
                if ui.m.trig_mode == TrigMode::Single {
                    ui.m.trig_mode = TrigMode::Normal;
                }
                if ui.m.run != RunState::Run {
                    ui.record_key(key);
                }
            } else if kw(nodes[0], "STOP") {
                if ui.m.run == RunState::Run {
                    ui.record_key(key);
                } else {
                    let _ = ui.sim_play_set(false);
                }
            } else {
                ui.record_key(crate::ui::key::SINGLE);
            }
            ui.mark_changed();
            return None;
        }
        // Reference trigger identities belong to the UI acquisition router, not
        // a silent physical-channel alias inside Model::trigger().
        if nodes.first().is_some_and(|node| kw(node, "TRIGger")) {
            let last = nodes.last().copied().unwrap_or("");
            let (leaf, source_number) = suffix(last);
            if (nodes.len() == 2 || nodes.len() == 3) && kw(leaf, "SOURce") {
                let secondary = source_number == Some(2);
                if query {
                    if !secondary && ui.m.trig_ref.is_none() {
                        if let Some(line) = ui.m.trig_line() {
                            return t(&format!("D{line}"));
                        }
                    }
                    let id = if secondary { ui.m.trigger_wave2() } else { ui.m.trigger_wave() };
                    return t(&match id {
                        WaveId::Ref(i) => format!("R{}", i + 1),
                        WaveId::Ch(c) => format!("CHAN{}", c + 1),
                    });
                }
                if let Some(id) = parse_wave(arg) {
                    if !ui.set_trigger_source(id, secondary) {
                        self.push_err("-224,\"Trigger source is unavailable\"".into());
                    }
                    ui.mark_changed();
                    return None;
                }
                // Leave D0..D15 to the existing LA source handler.
                if !arg.trim().to_ascii_uppercase().starts_with('D') {
                    self.push_err("-224,\"Invalid trigger source\"".into());
                    return None;
                }
            }
            if nodes.len() == 2 && (kw(last, "TYPe") || kw(last, "MODE")) {
                if query {
                    return t(if kw(last, "MODE") {
                        match ui.m.trig_type {
                            TrigType::Edge => "EDGE",
                            TrigType::Pulse => "PULS",
                            TrigType::Slope => "SLOP",
                            TrigType::Video => "VID",
                            TrigType::Pattern => "PATT",
                            TrigType::Duration => "DUR",
                            TrigType::Timeout => "TIM",
                            TrigType::Runt => "RUNT",
                            TrigType::Window => "WIND",
                            TrigType::Delay => "DEL",
                            TrigType::SetupHold => "SET",
                            TrigType::NthEdge => "NEDG",
                            TrigType::Bus => "BUS",
                        }
                    } else {
                        ui.m.trig_type.name()
                    });
                }
                let name = arg.trim().to_ascii_uppercase();
                let kind = match name.as_str() {
                    "EDGE" => Some(TrigType::Edge),
                    "PULSE" | "PULS" => Some(TrigType::Pulse),
                    "SLOPE" | "SLOP" => Some(TrigType::Slope),
                    "VIDEO" | "VID" => Some(TrigType::Video),
                    "PATTERN" | "PATT" => Some(TrigType::Pattern),
                    "DURATION" | "DUR" => Some(TrigType::Duration),
                    "TIMEOUT" | "TIM" => Some(TrigType::Timeout),
                    "RUNT" => Some(TrigType::Runt),
                    "WINDOW" | "WIND" => Some(TrigType::Window),
                    "DELAY" | "DEL" => Some(TrigType::Delay),
                    "SETUPHOLD" | "SETUP" | "SET" => Some(TrigType::SetupHold),
                    "NTHEDGE" | "NTH" | "NEDGE" | "NEDG" => Some(TrigType::NthEdge),
                    "BUS" => Some(TrigType::Bus),
                    _ => None,
                };
                if let Some(kind) = kind {
                    ui.trigger_type_set(kind);
                    ui.mark_changed();
                } else {
                    self.push_err("-224,\"Invalid trigger type\"".into());
                }
                return None;
            }
            if nodes.len() == 3 && kw(nodes[1], "RUNT") {
                let upper = kw(last, "UPPer") || kw(last, "ALEVel");
                let (lower, higher) = (ui.m.trig_level2, ui.m.trig_level);
                let value = if upper {
                    Some(&mut ui.m.trig_level)
                } else if kw(last, "LOWer") || kw(last, "BLEVel") {
                    Some(&mut ui.m.trig_level2)
                } else {
                    None
                };
                if let Some(value) = value {
                    if query {
                        return t(&e(*value as f64));
                    }
                    // the pair stays in order, as :MEASure:SETup:MAX|MIN: a threshold past the other is refused
                    match num(arg) {
                        Some(number) if (upper && number < lower) || (!upper && number > higher) => {
                            let other = if upper { "below the lower" } else { "above the upper" };
                            self.push_err(format!("-224,\"Illegal parameter value\" {other} threshold"));
                        }
                        Some(number) => {
                            *value = number;
                            ui.mark_changed();
                        }
                        None => self.push_err("-224,\"Invalid runt threshold\"".into()),
                    }
                    return None;
                }
            }
        }
        if !query
            && (nodes.len() == 1 && kw(nodes[0], "SINGle")
                || nodes.len() == 2
                    && kw(nodes[0], "TRIGger")
                    && kw(nodes[1], "SWEep")
                    && arg.to_ascii_uppercase().starts_with("SING")
                    && ui.m.run == RunState::Run)
        {
            ui.scpi_arm_operation();
        }
        if nodes.first().is_some_and(|n| kw(n, "TIMebase")) {
            return self.timebase_cmd(ui, &nodes, query, arg);
        }
        let m = &mut ui.m;
        let src = &ui.src;
        let r: Option<Option<Vec<u8>>> = (|| {
            let n0 = nodes.first().copied().unwrap_or("");
            let (k0, sfx) = suffix(n0);
            if !query && nodes.len() == 1 {
                if kw(n0, "RUN") {
                    m.run = RunState::Run;
                    if m.trig_mode == TrigMode::Single {
                        m.trig_mode = TrigMode::Auto;
                    }
                    return Some(None);
                }
                if kw(n0, "STOP") {
                    m.run = RunState::Stop;
                    return Some(None);
                }
                if kw(n0, "SINGle") {
                    m.trig_mode = TrigMode::Single;
                    m.run = RunState::Run;
                    return Some(None);
                }
            }
            if kw(k0, "CHANnel") && (nodes.len() == 2 || nodes.len() == 3) {
                let c = sfx?.checked_sub(1).filter(|&c| c < NCH)?;
                let n1 = nodes[1];
                if nodes.len() == 3 && kw(nodes[1], "LABel") && kw(nodes[2], "CONTent") {
                    if query {
                        return Some(t(&m.ch_label[c]));
                    }
                    let text = arg.trim().trim_matches('"').chars().take(10).collect::<String>();
                    m.ch_label[c] = text;
                    return Some(None);
                }
                if nodes.len() == 3 && kw(nodes[1], "LABel") && kw(nodes[2], "SHOW") {
                    if query {
                        return Some(t(if m.ch_label[c].is_empty() { "0" } else { "1" }));
                    }
                    return match parse_bool(arg) {
                        Some(false) => {
                            m.ch_label[c].clear();
                            Some(None)
                        }
                        Some(true) => {
                            if m.ch_label[c].is_empty() {
                                m.ch_label[c] = format!("CH{}", c + 1);
                            }
                            Some(None)
                        }
                        None => {
                            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                            Some(None)
                        }
                    };
                }
                if nodes.len() != 2 {
                    return None;
                }
                if kw(n1, "VERNier") {
                    // One fine-adjust switch for every channel: the screen has a single control.
                    if query {
                        return Some(t(if m.fine_v { "1" } else { "0" }));
                    }
                    return match parse_bool(arg) {
                        Some(on) => {
                            // off keeps a fine scale until the next coarse step, as the Channel menu's switch does
                            m.fine_v = on;
                            Some(None)
                        }
                        None => {
                            self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                            Some(None)
                        }
                    };
                }
                if kw(n1, "EXPand") {
                    // One switch for every channel: scale around ground, or around the screen center.
                    // The manual spells this GND|CENTer on math. This screen keeps it on the channel.
                    if query {
                        return Some(t(if m.v_expand_gnd { "GND" } else { "CENT" }));
                    }
                    if kw(arg, "GND") {
                        m.v_expand_gnd = true;
                    } else if kw(arg, "CENTer") {
                        m.v_expand_gnd = false;
                    } else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    }
                    return Some(None);
                }
                let expand_gnd = m.v_expand_gnd;
                let slow_deskew = m.time_per_div() > 10e-6;
                let fine = m.fine_v;
                let ch = &mut m.ch[c];
                if kw(n1, "DISPlay") {
                    if query {
                        return Some(t(if ch.on { "1" } else { "0" }));
                    }
                    ch.on = parse_bool(arg)?;
                } else if kw(n1, "SCALe") {
                    if query {
                        return Some(t(&e(ch.volts_per_div() as f64)));
                    }
                    let old = ch.volts_per_div();
                    let want = num(arg).filter(|&v| v > 0.0)? / ch.probe;
                    (ch.vscale, ch.vfine) = if fine { fine_scale(want) } else { (nearest(&VSCALES, want), 1.0) };
                    if expand_gnd && old > 0.0 {
                        ch.offset *= ch.volts_per_div() / old;
                    }
                } else if kw(n1, "OFFSet") || kw(n1, "POSition") {
                    if query {
                        return Some(t(&e(ch.offset as f64)));
                    }
                    ch.offset = num(arg)?;
                } else if kw(n1, "UNITs") {
                    if query {
                        return Some(t(match ch.unit {
                            AmpUnit::V => "VOLT",
                            AmpUnit::A => "AMP",
                            AmpUnit::W => "WATT",
                            AmpUnit::U => "UNKN",
                        }));
                    }
                    let a = arg.trim();
                    ch.unit = if kw(a, "VOLTage") {
                        AmpUnit::V
                    } else if kw(a, "AMPere") {
                        AmpUnit::A
                    } else if kw(a, "WATT") {
                        AmpUnit::W
                    } else if kw(a, "UNKNown") {
                        AmpUnit::U
                    } else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                } else if kw(n1, "COUPling") {
                    if query {
                        return Some(t(ch.coupling.name()));
                    }
                    ch.coupling = match arg.to_ascii_uppercase().as_str() {
                        "DC" => Coupling::Dc,
                        "AC" => Coupling::Ac,
                        "GND" => Coupling::Gnd,
                        "DC50" if board().couplings().contains(&Coupling::Dc50) => Coupling::Dc50,
                        _ => return None,
                    };
                } else if kw(n1, "BWLimit") {
                    if query {
                        return Some(t(match ch.bw {
                            BW_20M => "20M".to_string(),
                            BW_FULL => "OFF".to_string(),
                            0 => "AUTO".to_string(),
                            i => BWS[i].0.replace(' ', "").to_ascii_uppercase(),
                        }
                        .as_str()));
                    }
                    // OFF (full), 20M, AUTO, GND or a filter like 100M / 100MHZ / 1GHZ
                    let a = arg.trim().to_ascii_uppercase().replace(' ', "");
                    ch.bw = match a.as_str() {
                        "OFF" | "FULL" => BW_FULL,
                        "AUTO" => 0,
                        "GND" => 3,
                        "20M" | "20MHZ" => BW_20M,
                        _ => BWS.iter().position(|b| {
                            b.0.replace(' ', "").to_ascii_uppercase() == a
                                || b.0.replace(' ', "").to_ascii_uppercase() == format!("{}HZ", a)
                        })?,
                    };
                } else if kw(n1, "PROBe") {
                    if query {
                        return Some(t(&e(ch.probe as f64)));
                    }
                    ch.probe = num(arg).filter(|&p| p > 0.0)?;
                } else if kw(n1, "INVert") {
                    if query {
                        return Some(t(if ch.invert { "1" } else { "0" }));
                    }
                    ch.invert = parse_bool(arg)?;
                } else if kw(n1, "BIAS") {
                    // The screen's zero correction, subtracted from the channel reading.
                    // The DHO programmer manual has no header for it.
                    if query {
                        return Some(t(&e(ch.bias as f64)));
                    }
                    let Some(v) = num(arg) else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    ch.bias = v;
                } else if kw(n1, "TCALibrate") {
                    if query {
                        return Some(t(&e(ch.skew as f64)));
                    }
                    // The manual refuses a deskew change on a slow time base.
                    if slow_deskew {
                        self.push_err("-224,\"Illegal parameter value\" timebase".into());
                        return Some(None);
                    }
                    let Some(v) = num(arg).filter(|v| v.abs() <= SKEW_MAX) else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    ch.skew = v;
                } else {
                    return None;
                }
                return Some(None);
            }
            if kw(n0, "TRIGger") && nodes.len() == 3 && ["RS232", "SPI", "IIC"].iter().any(|b| kw(nodes[1], b)) {
                return ser::trig(m, nodes[1], nodes[2], arg, query);
            }
            if kw(n0, "TRIGger") {
                let last = *nodes.last()?;
                if nodes.len() == 2 && kw(last, "STATus") {
                    let s = match (m.run, m.status) {
                        (RunState::Stop, _) => "STOP",
                        (_, TrigStatus::Triggered) => "TD",
                        (_, TrigStatus::Auto) => "AUTO",
                        (_, TrigStatus::Wait) => "WAIT",
                        _ => "STOP",
                    };
                    return Some(t(s));
                }
                if nodes.len() == 2 && kw(last, "COUPling") {
                    if query {
                        return Some(t(match m.trig_coup {
                            TrigCoup::Dc => "DC",
                            TrigCoup::Ac => "AC",
                            TrigCoup::LfReject => "LFR",
                            TrigCoup::HfReject => "HFR",
                        }));
                    }
                    let a = arg.trim().to_ascii_uppercase();
                    m.trig_coup = match a.as_str() {
                        "DC" => TrigCoup::Dc,
                        "AC" => TrigCoup::Ac,
                        "LFR" | "LFREJECT" => TrigCoup::LfReject,
                        "HFR" | "HFREJECT" => TrigCoup::HfReject,
                        _ => return None,
                    };
                    return Some(None);
                }
                if nodes.len() == 2 && kw(last, "NREJect") {
                    if query {
                        return Some(t(if m.trig_nrej { "1" } else { "0" }));
                    }
                    let a = arg.trim().to_ascii_uppercase();
                    m.trig_nrej = match a.as_str() {
                        "1" | "ON" => true,
                        "0" | "OFF" => false,
                        _ => return None,
                    };
                    return Some(None);
                }
                if nodes.len() == 2 && kw(last, "HOLDoff") {
                    if query {
                        return Some(t(&e(m.holdoff as f64)));
                    }
                    m.holdoff = num(arg).filter(|&h| (0.0..=10.0).contains(&h))?;
                    return Some(None);
                }
                if nodes.len() == 2 && kw(last, "SWEep") {
                    if query {
                        return Some(t(match m.trig_mode {
                            TrigMode::Auto => "AUTO",
                            TrigMode::Normal => "NORM",
                            TrigMode::Single => "SING",
                        }));
                    }
                    let a = arg.to_ascii_uppercase();
                    m.trig_mode = if a.starts_with("AUTO") {
                        TrigMode::Auto
                    } else if a.starts_with("NORM") {
                        TrigMode::Normal
                    } else if a.starts_with("SING") {
                        TrigMode::Single
                    } else {
                        return None;
                    };
                    return Some(None);
                }
                if nodes.len() == 3 && kw(nodes[1], "EDGE") {
                    if kw(last, "SOURce") {
                        if query {
                            return Some(t(&m
                                .trig_line()
                                .map_or(format!("CHAN{}", m.trig_src + 1), |n| format!("D{}", n))));
                        }
                        // D0..D15 like Rigol's source names, with the LA on
                        match arg
                            .trim()
                            .to_ascii_uppercase()
                            .strip_prefix('D')
                            .and_then(|n| n.parse::<u8>().ok())
                            .filter(|&n| n < 16 && m.trig_la_ok())
                        {
                            Some(n) => m.trig_la = Some(n),
                            None => (m.trig_src, m.trig_la) = (parse_ch(arg)?, None),
                        }
                    } else if kw(last, "LEVel") {
                        if query {
                            return Some(t(&e(m.trig_level as f64)));
                        }
                        m.trig_level = num(arg)?;
                    } else if kw(last, "SLOPe") {
                        if query {
                            return Some(t(match m.slope {
                                Slope::Rise => "POS",
                                Slope::Fall => "NEG",
                                Slope::Either => "RFAL",
                            }));
                        }
                        let a = arg.to_ascii_uppercase();
                        m.slope = if a.starts_with("POS") {
                            Slope::Rise
                        } else if a.starts_with("NEG") {
                            Slope::Fall
                        } else if a.starts_with("RFAL") {
                            Slope::Either
                        } else {
                            return None;
                        };
                    } else {
                        return None;
                    }
                    return Some(None);
                }
                return None;
            }
            if kw(n0, "ACQuire") && nodes.len() == 3 && kw(nodes[1], "ULTRa") && kw(nodes[2], "MAXFrame") {
                if query {
                    return Some(t(&m.segments.to_string()));
                }
                let Some(n) = arg.trim().parse::<usize>().ok() else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return Some(None);
                };
                let steps = crate::ui::SEGMENTS;
                m.segments = *steps.iter().min_by_key(|s| s.abs_diff(n)).unwrap_or(&0);
                return Some(None);
            }
            if kw(n0, "ACQuire") && nodes.len() == 2 && !query && kw(nodes[1], "MDEPth") {
                // AUTO or one of the depths of the Acquire menu (`mdepths`), e.g. 1000000 or 1M
                let a = arg.trim().to_ascii_uppercase();
                let want = if a.starts_with("AUTO") {
                    0
                } else {
                    let (v, mul) = match a.chars().last() {
                        Some('K') => (&a[..a.len() - 1], 1e3),
                        Some('M') => (&a[..a.len() - 1], 1e6),
                        _ => (&a[..], 1.0),
                    };
                    (v.parse::<f64>().ok()? * mul).round() as u64
                };
                m.mdepth = mdepths().iter().position(|d| d.1 == want)?;
                return Some(None);
            }
            if kw(n0, "ACQuire") && nodes.len() == 2 && kw(nodes[1], "CORes") {
                // the ADC cores matched in software (on) or the words as they come (Acquire > ADC cores)
                if query {
                    return Some(t(if m.cores { "MATC" } else { "RAW" }));
                }
                let a = arg.trim().to_ascii_uppercase();
                m.cores = if a.starts_with("MATC") || a == "ON" || a == "1" {
                    true
                } else if a.starts_with("RAW") || a == "OFF" || a == "0" {
                    false
                } else {
                    return None;
                };
                return Some(None);
            }
            // segmented recording (OOS): :ACQuire:SEGMents <n>|OFF, :SEGMents:HARDware ON|OFF, :SEGMents:INTerval <s>
            if kw(n0, "ACQuire") && (2..=3).contains(&nodes.len()) && kw(nodes[1], "SEGMents") {
                match (nodes.get(2).copied(), query) {
                    (None, true) => return Some(t(&m.segments.to_string())),
                    (None, false) => {
                        m.segments = if parse_bool(arg) == Some(false) {
                            0
                        } else {
                            (arg.trim().parse::<usize>().ok()?).min(crate::ui::SEG_HW_MAX)
                        }
                    }
                    (Some(x), true) if kw(x, "HARDware") => return Some(t(if m.seg_hw { "1" } else { "0" })),
                    (Some(x), false) if kw(x, "HARDware") => m.seg_hw = parse_bool(arg)?,
                    (Some(x), true) if kw(x, "INTerval") => return Some(t(&e(m.seg_ivl as f64))),
                    (Some(x), false) if kw(x, "INTerval") => m.seg_ivl = num(arg)?.clamp(0.0, crate::ui::SEG_IVL_MAX),
                    _ => return None,
                }
                return Some(None);
            }
            // OOS: :ACQuire:AVERages:HARDware ON|OFF
            if kw(n0, "ACQuire") && nodes.len() == 3 && kw(nodes[1], "AVERages") && kw(nodes[2], "HARDware") {
                if query {
                    return Some(t(if m.avg_hw { "1" } else { "0" }));
                }
                m.avg_hw = parse_bool(arg)?;
                return Some(None);
            }
            if kw(n0, "ACQuire") && nodes.len() == 2 {
                let n1 = nodes[1];
                if kw(n1, "TYPE") {
                    if query {
                        return Some(t(match m.acq {
                            AcqMode::Normal => "NORM",
                            AcqMode::Peak => "PEAK",
                            AcqMode::Average => "AVER",
                            AcqMode::HiRes => "HRES",
                        }));
                    }
                    let a = arg.trim();
                    m.acq = if kw(a, "NORMal") {
                        AcqMode::Normal
                    } else if kw(a, "PEAK") {
                        AcqMode::Peak
                    } else if kw(a, "AVERages") {
                        AcqMode::Average
                    } else if kw(a, "HRESolution") || a.eq_ignore_ascii_case("HRES") {
                        AcqMode::HiRes
                    } else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    return Some(None);
                }
                if kw(n1, "AVERages") {
                    if query {
                        return Some(t(&m.avg_count.to_string()));
                    }
                    // The Acquire menu's counts. A value that is not one of them snaps down to the next lower power of two.
                    const COUNTS: [u32; 10] = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];
                    let Some(n) = arg.trim().parse::<u32>().ok().filter(|n| *n >= 2) else {
                        self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                        return Some(None);
                    };
                    m.avg_count = COUNTS.iter().rev().find(|&&c| c <= n).copied().unwrap_or(2);
                    return Some(None);
                }
                if query {
                    // what the top bar shows: the live hardware record, not the model's stock-system choice for Auto
                    let (sr, depth) = src.rate_depth(m);
                    if kw(n1, "SRATe") {
                        return Some(t(&e(sr)));
                    }
                    if kw(n1, "MDEPth") {
                        return Some(t(&e(depth as f64)));
                    }
                }
                return None;
            }
            None
        })();
        if let Some(resp) = r {
            if !query {
                ui.mark_changed();
            }
            return resp;
        }
        // measurement, waveform, display, system (need the record / canvas)
        let n0 = nodes.first().copied().unwrap_or("");
        if kw(n0, "MEASure") {
            return self.measure_cmd(ui, &nodes, query, arg, cmd);
        }
        if kw(n0, "EYE") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.eye_item(ui, arg);
        }
        if kw(n0, "POWer") && nodes.len() == 2 && query {
            if kw(nodes[1], "HARMonics") {
                let src = match self.waves(arg) {
                    Ok(v) => v,
                    Err(s) => {
                        self.push_err(s);
                        return t("9.9E37");
                    }
                };
                return match ui.fact_harmonics(&src) {
                    Ok(h) if h.is_empty() => t("9.9E37"),
                    Ok(h) => t(&h.iter().map(|(n, a)| format!("{},{}", n, e(*a))).collect::<Vec<_>>().join(";")),
                    Err(()) => {
                        self.push_err("-224,\"Illegal parameter value\"".into());
                        t("9.9E37")
                    }
                };
            }
            if kw(nodes[1], "ITEM") {
                return self.sourced(ui, arg, |ui, item, src| ui.fact_power(item, src));
            }
        }
        if kw(n0, "SPECtrum") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_spectrum(item, src));
        }
        if kw(n0, "MAGnetic") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_magnetic(item, src));
        }
        if kw(n0, "INVerter") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            let src = match self.waves(arg_rest(arg)) {
                Ok(v) => v,
                Err(s) => {
                    self.push_err(s);
                    return t("9.9E37");
                }
            };
            return match ui.fact_inverter(&arg_item(arg), &src) {
                Ok(crate::ui::facts::Fact::Word(w)) => t(w),
                Ok(crate::ui::facts::Fact::Num(v)) => t(&e(v)),
                Err(()) => {
                    self.push_err("-224,\"Illegal parameter value\"".into());
                    t("9.9E37")
                }
            };
        }
        if kw(n0, "DPT") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_dpt(item, src));
        }
        if kw(n0, "BANDwidth") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            return self.sourced(ui, arg, |ui, item, src| ui.fact_bandwidth(item, src));
        }
        if kw(n0, "WAVeform") && nodes.len() == 2 {
            let n1 = nodes[1];
            if kw(n1, "SOURce") {
                if query {
                    return t(&self.wav_source_text());
                }
                if let Some(id) = parse_wave(arg) {
                    self.wav_src = id;
                } else if !arg.is_empty() {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                }
                return None;
            }
            if kw(n1, "MODE") {
                if query {
                    return t(["NORM", "MAX", "RAW"][self.wav_mode.min(2) as usize]);
                }
                let a = arg.trim();
                self.wav_mode = if kw(a, "RAW") {
                    2
                } else if kw(a, "MAXimum") {
                    1
                } else if kw(a, "NORMal") {
                    0
                } else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                return None;
            }
            if kw(n1, "FORMat") {
                if query {
                    return t(["BYTE", "WORD", "ASC"][self.wav_fmt as usize]);
                }
                let format = if kw(arg.trim(), "BYTE") {
                    Some(0)
                } else if kw(arg.trim(), "WORD") {
                    Some(1)
                } else if kw(arg.trim(), "ASCii") {
                    Some(2)
                } else {
                    None
                };
                if let Some(format) = format {
                    self.wav_fmt = format;
                } else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                }
                return None;
            }
            if kw(n1, "STARt") || kw(n1, "STOP") {
                let stop = kw(n1, "STOP");
                if query {
                    let n = self.wave_len(ui);
                    let v = if stop {
                        if self.wav_stop == 0 {
                            n
                        } else {
                            self.wav_stop
                        }
                    } else {
                        self.wav_start
                    };
                    return t(&v.to_string());
                }
                let Some(p) = arg.trim().parse::<u64>().ok().filter(|&p| p >= 1) else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                if stop {
                    self.wav_stop = p;
                    if p < self.wav_start {
                        self.wav_start = p;
                    }
                } else {
                    self.wav_start = p;
                    if self.wav_stop != 0 && self.wav_stop < p {
                        self.wav_stop = p;
                    }
                }
                return None;
            }
            if kw(n1, "POINts") {
                if query {
                    return t(&self.page_count(ui).to_string());
                }
                // Rigol: the count to read, starting at STARt. stop 0 still means "the rest" until this sets it.
                let Some(n) = arg.trim().parse::<u64>().ok().filter(|&n| n >= 1) else {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                self.wav_stop = self.wav_start.saturating_add(n - 1);
                return None;
            }
            if kw(n1, "PREamble") && query {
                return t(&self.wave_preamble(ui));
            }
            if kw(n1, "DATA") && query {
                return self.wave_data(ui);
            }
        }
        if kw(n0, "REFerence") {
            return self.ref_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "SIM") {
            return self.sim_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "DISPlay")
            && !(nodes.len() == 2 && kw(nodes[1], "DATA"))
            && !(nodes.len() >= 2 && kw(nodes[1], "VIEW"))
        {
            return self.display_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "DISPlay") && nodes.len() == 2 && kw(nodes[1], "DATA") && query {
            let kind = arg.split_whitespace().next().unwrap_or("").trim();
            let up = kind.to_ascii_uppercase();
            if up.starts_with("PNG") {
                return Some(block(&crate::store::png_of(cv)));
            }
            if up.is_empty() || up.starts_with("BMP") {
                return Some(block(&bmp(cv)));
            }
            self.push_err(format!("-224,\"Illegal parameter value\" {}", kind));
            return None;
        }
        // Display > Views (ui/views.rs): :DISPlay:VIEW <n> shows view n, :DISPlay:VIEW:COUNt <1..4> how many there are
        if kw(n0, "DISPlay") && nodes.len() >= 2 && kw(nodes[1], "VIEW") {
            let count = nodes.len() == 3 && kw(nodes[2], "COUNt");
            // Display > Monitor view (ui/monitor.rs): the view an HDMI monitor in Layout shows, 0 the panel's
            if nodes.len() == 3 && kw(nodes[2], "MONitor") {
                if query {
                    return t(&ui.m.hdmi_view.to_string());
                }
                match arg.trim().parse::<usize>().ok() {
                    Some(n) if n <= ui.m.views && (n == 0 || ui.m.views >= 2) => ui.m.hdmi_view = n,
                    _ => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if nodes.len() > 3 || (nodes.len() == 3 && !count) {
                self.push_err("-113,\"Undefined header\"".into());
                return None;
            }
            if query {
                return t(&(if count { ui.m.views } else { ui.m.view_cur + 1 }).to_string());
            }
            let n = arg.trim().parse::<usize>().ok();
            match n {
                Some(n) if count && (1..=crate::ui::VIEWS_MAX).contains(&n) => ui.views_set(n),
                Some(n) if !count && (1..=ui.m.views).contains(&n) => ui.view_go(n - 1),
                _ => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
            }
            return None;
        }
        // OOS extensions: analyser view, Identify, self-calibration, Bode
        if kw(n0, "VIEW") && nodes.len() == 1 {
            let views = crate::ui::scpi_views();
            if query {
                return t(&format!("{:?}", ui.m.view).to_ascii_uppercase());
            }
            match views.iter().find(|(k, _)| kw(arg, k)) {
                Some((_, v)) => ui.set_analysis_view(*v),
                None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
            }
            return None;
        }
        if kw(n0, "IDENtify") && query {
            return t(&ui.identify_text());
        }
        if kw(n0, "CALibrate") && nodes.len() == 2 {
            if kw(nodes[1], "STATus") && query {
                return t(&ui.cal_status_text());
            }
            // :CALibrate:STARt 0..4[,ZERO]   (0 = all channels; the inputs must be disconnected)
            if kw(nodes[1], "STARt") && !query {
                let mut it = arg.split(',');
                let ch = it.next().unwrap_or("0").trim();
                ui.selfcal_remote(ch, it.next().is_some_and(|z| kw(z.trim(), "ZERO")));
                return None;
            }
            if kw(nodes[1], "CANCel") && !query {
                ui.selfcal_remote("cancel", false);
                return None;
            }
        }
        if kw(n0, "BODE") && nodes.len() == 2 {
            if kw(nodes[1], "RUN") && !query {
                ui.bode_run(parse_bool(arg).unwrap_or(true));
                return None;
            }
            if kw(nodes[1], "DATA") && query {
                return t(&ui.bode_text());
            }
            if kw(nodes[1], "MODE") {
                const MODES: [(&str, oos_dsp::bode::Mode); 3] = [
                    ("LOOP", oos_dsp::bode::Mode::Loop),
                    ("PSRR", oos_dsp::bode::Mode::Psrr),
                    ("IMPedance", oos_dsp::bode::Mode::Z),
                ];
                if query {
                    let name = match ui.m.bode_mode {
                        oos_dsp::bode::Mode::Loop => "LOOP",
                        oos_dsp::bode::Mode::Psrr => "PSRR",
                        oos_dsp::bode::Mode::Z => "IMP",
                    };
                    return t(name);
                }
                match MODES.iter().find(|(k, _)| kw(arg, k)) {
                    Some((_, m)) => {
                        ui.m.bode_mode = *m;
                        ui.mark_changed();
                    }
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if kw(nodes[1], "GENerator") {
                // "usb": the generator on the USB-A port; anything else is its LAN address (kept while on USB)
                if query {
                    return t(if ui.m.bode_usb { "usb" } else { ui.m.bode_addr.as_str() });
                }
                // one word, like the setup file keeps it
                let a = arg.trim().trim_matches('"');
                if a.contains(char::is_whitespace) {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                } else if a.eq_ignore_ascii_case("usb") {
                    ui.m.bode_usb = true;
                } else {
                    (ui.m.bode_addr, ui.m.bode_usb) = (a.to_string(), false);
                }
                return None;
            }
        }
        if suffix(n0).1.is_none_or(|n| (1..=gen_count().max(1)).contains(&n))
            && kw(suffix(n0).0, "SOURce")
            && nodes.len() >= 2
        {
            if !oos_hal::model::platform().caps.has_gen() {
                self.push_err(format!(
                    "-241,\"Hardware missing\" no generator on the {}",
                    oos_hal::model::platform().caps.name
                ));
                return None;
            }
            // :SOURce<n> talks to generator n, whichever one the page shows
            let g = suffix(n0).1.unwrap_or(1) - 1;
            if g != ui.m.gen_sel {
                let shown = ui.m.gen_sel;
                ui.m.gen_select(g);
                let r = self.exec_cmd(ui, cv, cmd);
                ui.m.gen_select(shown);
                return r;
            }
            let has = |k: &str| nodes[1..].iter().any(|n| kw(suffix(n).0, k));
            let m = &ui.m;
            let x = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            if has("OUTPut") {
                if query {
                    return t(if m.afg_on { "1" } else { "0" });
                }
                match parse_bool(arg) {
                    Some(on) => ui.afg_set(Some(on), None, None, None),
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // modulation (Rigol DG style): :MOD[:STATe] ON|OFF, :MOD:TYPe AM|FM|PM, :MOD:AM:DEPTh <%>, :MOD:FM:DEViation <Hz>,
            // :MOD:PM:DEViation <deg>, :MOD:<type>:INTernal:FREQuency <Hz>, :MOD:<type>:INTernal:FUNCtion <shape>
            if has("MOD") {
                const SHAPES: [&str; 6] = ["SINusoid", "SQUare", "TRIangle", "RAMP", "NRAMp", "NOISe"];
                let a = arg.trim();
                if has("INTernal") && has("FUNCtion") {
                    if query {
                        return t(&SHAPES[m.afg_mod_shape.min(5)]
                            .chars()
                            .filter(|c| c.is_ascii_uppercase())
                            .collect::<String>());
                    }
                    match SHAPES.iter().position(|s| kw(a, s)) {
                        Some(i) => ui.m.afg_mod_shape = i,
                        None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                    }
                    return None;
                }
                let (cur, key): (f64, u8) = if has("INTernal") && has("FREQuency") {
                    (m.afg_mod_rate, 0)
                } else if has("DEPTh") {
                    (m.afg_mod_depth, 1)
                } else if has("DEViation") && has("PM") {
                    (m.afg_pm_dev, 3)
                } else if has("DEViation") {
                    (m.afg_fm_dev, 2)
                } else if has("TYPe") {
                    if query {
                        return t(["NONE", "AM", "FM", "PM"][m.afg_mod.min(3)]);
                    }
                    match ["AM", "FM", "PM"].iter().position(|k| kw(a, k)) {
                        Some(i) => ui.m.afg_mod = i + 1,
                        None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                    }
                    ui.afg_set(None, None, None, None);
                    return None;
                } else {
                    // :MOD[:STATe]
                    if query {
                        return t(if m.afg_mod > 0 { "1" } else { "0" });
                    }
                    match parse_bool(arg) {
                        Some(on) => ui.m.afg_mod = if on { ui.m.afg_mod.max(1) } else { 0 },
                        None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                    }
                    return None;
                };
                if query {
                    return t(&e(cur));
                }
                match x {
                    Some(v) => {
                        let mm = &mut ui.m;
                        match key {
                            0 => mm.afg_mod_rate = v.clamp(0.002, 1e6),
                            1 => mm.afg_mod_depth = v.clamp(0.0, 120.0),
                            2 => mm.afg_fm_dev = v.max(0.01),
                            _ => mm.afg_pm_dev = v.clamp(0.0, 360.0),
                        }
                        ui.afg_set(None, None, None, None);
                    }
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // :FUNCtion:SQUare:DUTY / :FUNCtion:PULSe:DUTY and :FUNCtion:RAMP:SYMMetry in %
            // arbitrary wave: :ARB:DATA <v>,<v>,... (one period, any scale), :ARB:CAPTure CHAN<n> (the screen of a channel)
            if has("ARB") {
                if has("DATA") && !query {
                    let v: Vec<f32> = arg.split(',').filter_map(num).collect();
                    if !ui.afg_arb_load(&v) {
                        self.push_err(format!("-224,\"Illegal parameter value\" {} samples", v.len()));
                    }
                } else if has("CAPTure") && !query {
                    match parse_ch(arg).ok_or("no channel".to_string()).and_then(|c| ui.afg_arb_capture(c)) {
                        Ok(_) => {}
                        Err(e) => self.push_err(format!("-224,\"Illegal parameter value\" {}", e)),
                    }
                } else {
                    self.push_err("-113,\"Undefined header\" :SOURce:ARB".into());
                }
                return None;
            }
            if has("FUNCtion") && (has("DUTY") || has("SYMMetry")) {
                let duty = has("DUTY");
                if query {
                    return t(&e(if duty { m.afg_duty } else { m.afg_symmetry }));
                }
                match x {
                    Some(v) if duty => ui.m.afg_duty = v.clamp(5.0, 95.0),
                    Some(v) => ui.m.afg_symmetry = v.clamp(0.0, 100.0),
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if has("FUNCtion") {
                const SHORT: [&str; 7] = ["SIN", "SQU", "RAMP", "PULS", "NOIS", "DC", "ARB"];
                const LONG: [&str; 7] = ["SINusoid", "SQUare", "RAMP", "PULSe", "NOISe", "DC", "ARB"];
                if query {
                    return t(SHORT[m.afg_wave.min(6)]);
                }
                match LONG.iter().position(|l| kw(arg.trim(), l)) {
                    Some(i) => {
                        ui.m.afg_wave = i;
                        ui.afg_set(None, None, None, None);
                    }
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            let (want, cur): (u8, f64) = if has("FREQuency") {
                (0, m.afg_freq)
            } else if has("VOLTage") && has("OFFSet") {
                (2, m.afg_offset)
            } else if has("VOLTage") {
                (1, m.afg_ampl)
            } else {
                (9, 0.0)
            };
            if want < 9 {
                if query {
                    return t(&e(cur));
                }
                match x {
                    Some(v) => {
                        ui.afg_set(None, (want == 0).then_some(v), (want == 1).then_some(v), (want == 2).then_some(v))
                    }
                    None => self.push_err(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
        }
        if kw(n0, "SYSTem") {
            return self.system_cmd(ui, &nodes, query, arg, cmd);
        }
        if kw(n0, "ACQuire") && nodes.len() == 3 && kw(nodes[1], "SEGMents") && kw(nodes[2], "TIMes") && query {
            return t(&ui.seg_times_text());
        }
        if kw(n0, "DECode") && nodes.len() == 2 && kw(nodes[1], "EVENts") && query {
            return t(&ui.events_text());
        }
        if kw(n0, "CURSor") {
            return self.cursor_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "MASK") {
            return self.mask_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "HISTogram") {
            return self.hist_cmd(ui, &nodes[1..], query, arg);
        }
        if let Some(k) = math_index(n0) {
            return self.math_cmd(ui, k, &nodes[1..], query, arg);
        }
        if let Some(k) = bus_index(n0) {
            return self.bus_cmd(ui, k, &nodes[1..], query, arg);
        }
        if kw(n0, "DVM") {
            return self.dvm_cmd(ui, &nodes[1..], query, arg);
        }
        if kw(n0, "COUNter") {
            return self.counter_cmd(ui, &nodes[1..], query, arg);
        }
        self.push_err(format!("-113,\"Undefined header\" {}", cmd));
        None
    }
}
