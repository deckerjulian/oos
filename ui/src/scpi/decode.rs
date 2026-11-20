// Copyright (C) 2026 dekajoule
// SPDX-License-Identifier: GPL-2.0-or-later

//! The decoders: `:BUS<n>` (`MODE`, `DISPlay`, `FORMat`, `EVENt`, `THReshold`, `DATA?`, and the sources and settings
//! under `RS232`, `IIC` and `SPI`) and `:DECode` (sources and roles, `PROTocol`, `STATe`, `SELect`, `THReshold`,
//! `INPut`, `EVENts?`, `BUSY?`, `CPHY:RATE`). A new decoder command goes into `bus_cmd` or `decode_cmd` here, with
//! its line in docs/scpi.md and its query in ui/tests/scpi.txt (AGENTS.md).

use super::{block, e, kw, nearest_index, num, parse_bool, parse_wave, suffix, t, Scpi};
use crate::model::*;
use crate::protocols::PROTOS;
use crate::ui::Ui;

pub(super) fn bus_index(tok: &str) -> Option<usize> {
    let (k, n) = suffix(tok);
    if kw(k, "BUS") {
        n.filter(|&n| (1..=DECODERS).contains(&n)).map(|n| n - 1)
    } else {
        None
    }
}

fn with_bus(ui: &mut Ui, k: usize, f: impl FnOnce(&mut Model)) {
    let was = ui.m.dec_sel;
    if k != was {
        ui.m.dec_select(k);
    }
    f(&mut ui.m);
    if k != was {
        ui.m.dec_select(was);
    }
    if k == was && ui.m.trig_type == TrigType::Bus {
        ui.mark_changed();
    }
}

fn parse_bus_wave(ui: &Ui, text: &str) -> Option<usize> {
    match parse_wave(text)? {
        WaveId::Ch(c) if c < n_ch() => Some(c),
        WaveId::Ref(i) if ui.ref_info(i).is_some() => Some(DEC_REF_BASE + i),
        _ => None,
    }
}

fn bus_source_name(source: usize) -> String {
    match decode_wave(source) {
        Some(WaveId::Ch(c)) => format!("CHAN{}", c + 1),
        Some(WaveId::Ref(i)) => format!("R{}", i + 1),
        None => "Unavailable".into(),
    }
}

/// Configure the requested decoder slot, preserving the visible/trigger decoder.
/// Only the active decoder's primary role can take over the acquisition source.
fn put_bus_wave(ui: &mut Ui, slot: usize, protocol: Proto, role: usize, source: Option<usize>) -> bool {
    let selected = ui.m.dec_sel;
    if slot != selected {
        ui.m.dec_select(slot);
    }
    let previous_protocol = ui.m.dec_proto;
    ui.m.dec_proto = protocol;
    let ok = if slot == selected {
        ui.configure_decoder_source(role, source)
    } else {
        ui.m.dec_la_in = false;
        match role {
            0 => ui.m.dec_src = source.unwrap_or(ui.m.dec_src),
            1 => ui.m.dec_src2 = source.unwrap_or(ui.m.dec_src2),
            2 => ui.m.dec_cs = source,
            _ => unreachable!(),
        }
        true
    };
    if !ok {
        ui.m.dec_proto = previous_protocol;
    }
    if slot != selected {
        ui.m.dec_select(selected);
    }
    if ok && slot == selected && ui.m.trig_type == TrigType::Bus {
        ui.mark_changed();
    }
    ok
}

fn proto_token(p: Proto) -> String {
    match p {
        Proto::PARALLEL => "PAR".into(),
        Proto::UART => "RS232".into(),
        Proto::SPI => "SPI".into(),
        Proto::I2C => "IIC".into(),
        Proto::LIN => "LIN".into(),
        Proto::CAN => "CAN".into(),
        other => other.name().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase(),
    }
}

fn parse_proto(arg: &str) -> Option<Proto> {
    let a = arg.trim();
    if kw(a, "PARallel") {
        return Some(Proto::PARALLEL);
    }
    if kw(a, "RS232") || kw(a, "UART") {
        return Some(Proto::UART);
    }
    if kw(a, "IIC") || kw(a, "I2C") {
        return Some(Proto::I2C);
    }
    if kw(a, "LIN") {
        return Some(Proto::LIN);
    }
    if kw(a, "SPI") {
        return Some(Proto::SPI);
    }
    if kw(a, "CAN") {
        return Some(Proto::CAN);
    }
    PROTOS.iter().copied().find(|p| proto_token(*p).eq_ignore_ascii_case(a))
}

impl Scpi {
    /// One of the four decoders. RS232 is the screen's UART. The event table is one window for every bus.
    pub(super) fn bus_cmd(&mut self, ui: &mut Ui, k: usize, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let n0 = n.first().copied().unwrap_or("");
        let cfg = ui.m.dec_slot(k);
        if n.len() == 1 && kw(n0, "MODE") {
            if query {
                return t(&proto_token(cfg.proto));
            }
            let Some(p) = parse_proto(arg) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| m.set_dec_proto_only(p));
            return None;
        }
        if n.len() == 1 && kw(n0, "DISPlay") {
            if query {
                return t(if cfg.on { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    with_bus(ui, k, |m| m.dec_on = on);
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "FORMat") {
            if query {
                return t(if cfg.ascii { "ASC" } else { "HEX" });
            }
            let ascii = if kw(arg, "HEX") {
                Some(false)
            } else if kw(arg, "ASCii") {
                Some(true)
            } else {
                None
            };
            let Some(ascii) = ascii else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| m.dec_ascii = ascii);
            return None;
        }
        if n.len() == 1 && kw(n0, "EVENt") {
            if query {
                return t(if ui.decode_table_on() { "1" } else { "0" });
            }
            return match parse_bool(arg) {
                Some(on) => {
                    ui.set_decode_table(on);
                    ui.mark_changed();
                    None
                }
                None => {
                    self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                    None
                }
            };
        }
        if n.len() == 1 && kw(n0, "THReshold") {
            if query {
                return t(&e(cfg.thr as f64));
            }
            let num = arg.split(',').next_back().unwrap_or("").trim();
            let Some(v) = num.parse::<f64>().ok().filter(|v| v.is_finite()) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| m.dec_thr = v as f32);
            return None;
        }
        if n.len() == 1 && kw(n0, "DATA") && query {
            return self.bus_data(ui, k);
        }
        if n.len() == 2 && kw(n0, "RS232") {
            return self.bus_rs232(ui, k, n[1], query, arg);
        }
        if n.len() == 3 && kw(n0, "IIC") && kw(n[2], "SOURce") && (kw(n[1], "SCLK") || kw(n[1], "SDA")) {
            return self.bus_iic_source(ui, k, n, query, arg);
        }
        if n.len() == 3
            && kw(n0, "SPI")
            && kw(n[2], "SOURce")
            && (kw(n[1], "SCLK") || kw(n[1], "MOSI") || kw(n[1], "SS"))
        {
            return self.bus_spi_source(ui, k, n, query, arg);
        }
        if n.len() == 2 && kw(n0, "SPI") && kw(n[1], "DBITs") {
            if query {
                return t(&format!("{}", cfg.bits));
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (4..=32).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::SPI;
                m.dec_bits = v;
            });
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :BUS{}:{}", k + 1, n.join(":")));
        None
    }

    /// `:BUS<n>:DATA?`: the decoder's protocol and its rows of the event table.
    fn bus_data(&mut self, ui: &Ui, k: usize) -> Option<Vec<u8>> {
        let cfg = ui.m.dec_slot(k);
        let events = match ui.decode_events_result() {
            Ok(events) => events,
            Err("Decode computing") => return t("BUSY"),
            Err(error) => {
                self.push_err(format!("-230,\"{}\"", error));
                return None;
            }
        };
        let prefix = format!("D{} ", k + 1);
        let rows = events
            .lines()
            .filter(|line| line.split_once(',').is_some_and(|(_, rest)| rest.starts_with(&prefix)))
            .collect::<Vec<_>>()
            .join("\n");
        Some(block(format!("{}\n{}", proto_token(cfg.proto), rows).as_bytes()))
    }

    /// `:BUS<n>:IIC:SCLK|SDA:SOURce` (`n` without the root).
    fn bus_iic_source(&mut self, ui: &mut Ui, k: usize, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let cfg = ui.m.dec_slot(k);
        let clock = kw(n[1], "SCLK");
        if query {
            return t(&bus_source_name(if clock { cfg.src2 } else { cfg.src }));
        }
        let Some(source) = parse_bus_wave(ui, arg) else {
            self.push_err(format!("-224,\"Decoder source is unavailable\" {}", arg));
            return None;
        };
        if !put_bus_wave(ui, k, Proto::I2C, usize::from(clock), Some(source)) {
            self.push_err("-224,\"Decoder source is unavailable\"".into());
        }
        None
    }

    /// `:BUS<n>:SPI:SCLK|MOSI|SS:SOURce` (`n` without the root); SS takes OFF.
    fn bus_spi_source(&mut self, ui: &mut Ui, k: usize, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        let cfg = ui.m.dec_slot(k);
        let role = if kw(n[1], "SCLK") {
            1
        } else if kw(n[1], "MOSI") {
            0
        } else {
            2
        };
        if query {
            return t(&match role {
                0 => bus_source_name(cfg.src),
                1 => bus_source_name(cfg.src2),
                _ => cfg.cs.map(bus_source_name).unwrap_or_else(|| "OFF".into()),
            });
        }
        let source = if role == 2 && (kw(arg, "OFF") || kw(arg, "NONE")) {
            None
        } else {
            match parse_bus_wave(ui, arg) {
                Some(source) => Some(source),
                None => {
                    self.push_err(format!("-224,\"Decoder source is unavailable\" {}", arg));
                    return None;
                }
            }
        };
        if !put_bus_wave(ui, k, Proto::SPI, role, source) {
            self.push_err("-224,\"Decoder source is unavailable\"".into());
        }
        None
    }

    fn bus_rs232(&mut self, ui: &mut Ui, k: usize, which: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        let cfg = ui.m.dec_slot(k);
        if kw(which, "TX") || kw(which, "RX") {
            let role = usize::from(kw(which, "RX"));
            if query {
                return t(&bus_source_name(if role == 1 { cfg.src2 } else { cfg.src }));
            }
            let Some(source) = parse_bus_wave(ui, arg) else {
                self.push_err(format!("-224,\"Decoder source is unavailable\" {}", arg));
                return None;
            };
            if !put_bus_wave(ui, k, Proto::UART, role, Some(source)) {
                self.push_err("-224,\"Decoder source is unavailable\"".into());
            }
            return None;
        }
        if kw(which, "BAUD") {
            if query {
                return t(&format!("{}", crate::decode::BAUDS[cfg.baud.min(crate::decode::BAUDS.len() - 1)] as u64));
            }
            let Some(v) = arg.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            let i = nearest_index(&crate::decode::BAUDS, v);
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::UART;
                m.dec_baud = i;
            });
            return None;
        }
        if kw(which, "PARity") {
            if query {
                return t(match cfg.parity {
                    oos_dsp::decode::Parity::None => "NONE",
                    oos_dsp::decode::Parity::Odd => "ODD",
                    oos_dsp::decode::Parity::Even => "EVEN",
                });
            }
            let p = if kw(arg, "NONE") {
                Some(oos_dsp::decode::Parity::None)
            } else if kw(arg, "ODD") {
                Some(oos_dsp::decode::Parity::Odd)
            } else if kw(arg, "EVEN") {
                Some(oos_dsp::decode::Parity::Even)
            } else {
                None
            };
            let Some(p) = p else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::UART;
                m.dec_parity = p;
            });
            return None;
        }
        if kw(which, "DBITs") {
            if query {
                return t(&format!("{}", cfg.bits));
            }
            let Some(v) = arg.trim().parse::<u32>().ok().filter(|v| (5..=9).contains(v)) else {
                self.push_err(format!("-224,\"Illegal parameter value\" {}", arg));
                return None;
            };
            with_bus(ui, k, |m| {
                m.dec_proto = Proto::UART;
                m.dec_bits = v;
            });
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" :BUS{}:RS232:{}", k + 1, which));
        None
    }

    pub(super) fn decode_cmd(&mut self, ui: &mut Ui, node: &str, query: bool, arg: &str) -> Option<Vec<u8>> {
        if kw(node, "SRATe") {
            return self.decode_cphy_rate_cmd(ui, query, arg);
        }
        let text = |value: String| Some(value.into_bytes());
        let illegal = |s: &mut Self| {
            s.push_err(format!("-224,\"Illegal decoder parameter\" {}", arg));
            None
        };
        if kw(node, "EVENts") && query {
            return match ui.decode_events_result() {
                Ok(events) => text(events),
                Err("Decode computing") => text("BUSY".into()),
                Err(error) => {
                    self.push_err(format!("-230,\"{}\"", error));
                    None
                }
            };
        }
        if kw(node, "BUSY") && query {
            return text(if ui.decode_busy() { "1" } else { "0" }.into());
        }
        let role = if kw(node, "SOURce") || kw(node, "SOURce1") || node.eq_ignore_ascii_case("SOUR1") {
            Some(0)
        } else if kw(node, "SOURce2") || node.eq_ignore_ascii_case("SOUR2") {
            Some(1)
        } else if kw(node, "CS") {
            Some(2)
        } else if kw(node, "SOURce4") || node.eq_ignore_ascii_case("SOUR4") {
            Some(3)
        } else if kw(node, "DIFFerential") {
            Some(4)
        } else {
            None
        };
        if let Some(role) = role {
            if query {
                if ui.m.dec_on_la() && role < 4 {
                    return text(ui.m.dec_la[role].map_or("NONE".into(), |line| format!("D{line}")));
                }
                let source = match role {
                    0 => Some(ui.m.dec_src),
                    1 => Some(ui.m.dec_src2),
                    2 => ui.m.dec_cs,
                    3 => ui.m.dec_src4,
                    _ => ui.m.dec_diff,
                };
                return text(source.map_or("NONE".into(), decode_source_name));
            }
            if ui.m.dec_on_la() && role < 4 {
                let source = arg
                    .strip_prefix('D')
                    .or_else(|| arg.strip_prefix('d'))
                    .and_then(|v| v.parse::<u8>().ok())
                    .filter(|&v| v < 16);
                if source.is_none() && !(role >= 2 && arg.eq_ignore_ascii_case("NONE")) {
                    return illegal(self);
                }
                ui.m.dec_la[role] = source;
                ui.mark_changed();
                return None;
            }
            let source = if arg.eq_ignore_ascii_case("NONE") {
                None
            } else {
                match parse_wave(arg) {
                    Some(WaveId::Ch(c)) => Some(c),
                    Some(WaveId::Ref(i)) => Some(DEC_REF_BASE + i),
                    None => return illegal(self),
                }
            };
            if !ui.configure_decoder_source(role, source) {
                return illegal(self);
            }
            ui.mark_changed();
            return None;
        }
        if kw(node, "PROTocol") || node.eq_ignore_ascii_case("PROTO") {
            if query {
                return text(ui.m.dec_proto.name().into());
            }
            let compact = |s: &str| {
                s.chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(char::to_uppercase).collect::<String>()
            };
            let name = compact(arg);
            let Some(protocol) = PROTOS.iter().copied().find(|p| compact(p.name()) == name) else {
                return illegal(self);
            };
            // SCPI configures an instrument, not the demo's suggested sources/trigger.
            ui.m.set_dec_proto_only(protocol);
            if protocol == Proto::CPHY {
                ui.m.dec_thr = 0.02;
                ui.m.dec_la_in = false;
            }
            ui.mark_changed();
            return None;
        }
        if kw(node, "STATe") {
            if query {
                return text(if ui.m.dec_on { "1" } else { "0" }.into());
            }
            let Some(on) = parse_bool(arg) else { return illegal(self) };
            ui.m.dec_on = on;
            ui.mark_changed();
            return None;
        }
        if kw(node, "SELect") {
            if query {
                return text((ui.m.dec_sel + 1).to_string());
            }
            let Some(slot) = arg.parse::<usize>().ok().filter(|n| (1..=DECODERS).contains(n)) else {
                return illegal(self);
            };
            ui.m.dec_select(slot - 1);
            return None;
        }
        if kw(node, "THReshold") {
            if query {
                return text(e(ui.m.dec_thr as f64));
            }
            let Some(threshold) = num(arg) else { return illegal(self) };
            ui.m.dec_thr = threshold;
            ui.mark_changed();
            return None;
        }
        if kw(node, "INPut") {
            if query {
                return text(if ui.m.dec_la_in { "LA" } else { "WAVEFORM" }.into());
            }
            if arg.eq_ignore_ascii_case("LA") && ui.m.la_on() && ui.m.dec_proto != Proto::CPHY {
                ui.m.dec_la_in = true;
            } else if arg.eq_ignore_ascii_case("WAVEFORM") || arg.eq_ignore_ascii_case("ANALOG") {
                ui.m.dec_la_in = false;
            } else {
                return illegal(self);
            }
            ui.mark_changed();
            return None;
        }
        self.push_err(format!("-113,\"Undefined header\" DECode:{}", node));
        None
    }

    pub(super) fn decode_cphy_rate_cmd(&mut self, ui: &mut Ui, query: bool, arg: &str) -> Option<Vec<u8>> {
        if query {
            return Some(if ui.m.dec_cphy_rate == 0.0 { b"AUTO".to_vec() } else { e(ui.m.dec_cphy_rate).into_bytes() });
        }
        match crate::decode::parse_cphy_rate(arg) {
            Ok(rate) => {
                ui.m.dec_cphy_rate = rate;
                ui.mark_changed();
            }
            Err(_) => self.push_err(format!("-224,\"Invalid C-PHY symbol rate\" {}", arg)),
        }
        None
    }
}
