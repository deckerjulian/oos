// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! SCPI server (TCP 5555, the port of the stock firmware), a Rigol-compatible subset:
//! *IDN? *RST *OPC[?] *CLS *ESR?, :RUN :STOP :SINGle :AUToscale :CLEar,
//! :CHANnel<n>:DISPlay|SCALe|OFFSet|COUPling|BWLimit|PROBe|INVert, :TIMebase[:MAIN]:SCALe|OFFSet,
//! :TRIGger:MODE? :TRIGger:EDGE:SOURce|LEVel|SLOPe, :TRIGger:COUPling DC|AC|LFR|HFR, :TRIGger:SWEep, :TRIGger:HOLDoff, :TRIGger:STATus?, :ACQuire:SRATe?|MDEPth[?] (AUTO, 1k..125M),
//! :MEASure:ITEM? <item>[,CHANn], :WAVeform:SOURce|MODE NORM/RAW|FORMat|STARt|STOP|DATA?|PREamble?|POINts?, :DISPlay:DATA? (BMP),
//! :LA:STATe, :LA:DIGital<n>:DISPlay, :LA:POD<n>:THReshold, :LA:POINts?, :LA:PREamble?, :LA:DATA? (u16 LE words),
//! :COUNter:ENABle|SOURce|MODE FREQuency|PERiod|TOTalize|NDIGits 3..6, :COUNter:CURRent?, :COUNter:TOTalize:CLEar,
//! :SYSTem:ERRor?, :SYSTem:STATe?, :SYSTem:GRAPhics CPU|GPU, OOS: :VIEW SCOPe|SPECtrum|EYE|POWer|BODE, :DISPlay:VIEW <n>, :DISPlay:VIEW:COUNt 1..4, :IDENtify?, :CALibrate:STARt 0..4[,ZERO] / :STATus? /
//! :CANCel, :BODE:RUN ON|OFF / :DATA? / :GENerator "ip:port",
//! built-in generator: :SOURce[1]:OUTPut[:STATe] ON|OFF, :FUNCtion SINusoid|SQUare|RAMP|PULSe|NOISe|DC|ARB, :ARB:DATA, :ARB:CAPTure, :FUNCtion:SQUare:DUTY, :FUNCtion:RAMP:SYMMetry, :MOD[:STATe], :MOD:TYPe AM|FM|PM, :MOD:AM:DEPTh,
//! :MOD:FM|PM:DEViation, :MOD:<type>:INTernal:FREQuency|FUNCtion, :FREQuency <Hz>,
//! :VOLTage[:AMPLitude] <Vpp>, :VOLTage:OFFSet <V> (all with queries)
//! Settings go through the same model as the screen, so the screen follows remote changes.

use crate::gfx::Canvas;
use crate::board::{board, Board};
use crate::model::*;
use crate::ui::Ui;
use std::io::{ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

struct Client {
    s: TcpStream,
    buf: Vec<u8>,
}

/// Longest command line (`:SOURce:ARB:DATA` with its samples is the longest there is); a client past it is dropped.
const MAX_LINE: usize = 4 << 20;
/// Connections at once.
const MAX_CLIENTS: usize = 8;
/// A message stops here: one answer may be a long record, ten thousand of them in one line may not.
pub const RESP_MAX: usize = 64 << 20;
const ERR_MAX: usize = 20;
const ERR_LEN: usize = 256;
/// A client that takes no data for this long, or not all of a response in `SEND_LIMIT`, is dropped: the server runs in
/// the main loop, the screen and the keys wait while it writes.
const SEND_STALL: Duration = Duration::from_secs(2);
const SEND_LIMIT: Duration = Duration::from_secs(30);

/// Writes `d` to a client; false when it was given up (the connection is then to be closed).
pub fn send(s: &mut TcpStream, d: &[u8]) -> bool {
    let end = std::time::Instant::now() + SEND_LIMIT;
    let _ = s.set_write_timeout(Some(SEND_STALL));
    let mut p = 0;
    while p < d.len() {
        if std::time::Instant::now() >= end {
            return false;
        }
        match s.write(&d[p..]) {
            Ok(0) => return false,
            Ok(n) => p += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
    true
}

pub struct Scpi {
    l: Option<TcpListener>,
    clients: Vec<Client>,
    wav_src: usize,
    wav_fmt: u8, // 0 BYTE, 1 WORD, 2 ASCII
    /// :WAVeform:MODE: false = the screen record, true = every sample of the record in the screen window (RAW)
    wav_raw: bool,
    /// :WAVeform:STARt / :STOP: the first and last point `:WAVeform:DATA?` returns, 1-based (stop 0 = to the end)
    wav_start: usize,
    wav_stop: usize,
    err: Vec<String>,
    /// *ESR?: the standard event status register (OPC 1, query error 4, device error 8, execution error 16, command error 32)
    esr: u8,
}

/// SCPI keyword match: the long form or its short form (the upper-case part), case-insensitive.
fn kw(tok: &str, long: &str) -> bool {
    let t = tok.to_ascii_uppercase();
    let short: String = long.chars().filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).collect();
    t == long.to_ascii_uppercase() || t == short
}

/// Splits a keyword with a numeric suffix: "CHAN2" -> ("CHAN", 2).
fn suffix(tok: &str) -> (&str, Option<usize>) {
    let i = tok.find(|c: char| c.is_ascii_digit()).unwrap_or(tok.len());
    (&tok[..i], tok[i..].parse().ok())
}

fn parse_ch(s: &str) -> Option<usize> {
    let (k, n) = suffix(s.trim());
    if kw(k, "CHANnel") {
        n.filter(|&n| (1..=NCH).contains(&n)).map(|n| n - 1)
    } else {
        None
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_uppercase().as_str() {
        "1" | "ON" => Some(true),
        "0" | "OFF" => Some(false),
        _ => None,
    }
}

/// A finite number ("nan", "inf" and overflows are refused: they would end up in the model and in saved setups).
fn num(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

fn nearest(ladder: &[f32], v: f32) -> usize {
    let mut best = 0;
    for (i, &x) in ladder.iter().enumerate() {
        if ((x / v).ln()).abs() < ((ladder[best] / v).ln()).abs() {
            best = i;
        }
    }
    best
}

/// Rigol number format: 7 significant digits, two-digit exponent ("2.000000E-04").
fn e(v: f64) -> String {
    if !v.is_finite() {
        return "9.9E37".into();
    }
    let s = format!("{:.6E}", v);
    match s.split_once('E') {
        Some((m, x)) => {
            let n: i32 = x.parse().unwrap_or(0);
            format!("{}E{}{:02}", m, if n < 0 { '-' } else { '+' }, n.abs())
        }
        None => s,
    }
}

impl Scpi {
    pub fn open(port: u16) -> Scpi {
        Scpi { l: Self::bind(port), clients: Vec::new(), wav_src: 0, wav_fmt: 0, wav_raw: false, wav_start: 1, wav_stop: 0, err: Vec::new(), esr: 0 }
    }

    fn bind(port: u16) -> Option<TcpListener> {
        let l = TcpListener::bind(("0.0.0.0", port)).ok();
        if let Some(l) = &l {
            let _ = l.set_nonblocking(true);
            eprintln!("oos-ui: SCPI on port {}", port);
        } else {
            eprintln!("oos-ui: SCPI port {} not available", port);
        }
        l
    }

    /// System > Network > Remote services: the port open (`Some`), or closed with its connections.
    pub fn set_open(&mut self, port: Option<u16>) {
        match port {
            Some(p) if self.l.is_none() => self.l = Self::bind(p),
            Some(_) => {}
            None => {
                if self.l.take().is_some() {
                    eprintln!("oos-ui: SCPI off");
                }
                self.clients.clear();
            }
        }
    }

    pub fn poll(&mut self, ui: &mut Ui, cv: &Canvas) {
        if let Some(l) = &self.l {
            while let Ok((s, _)) = l.accept() {
                let _ = s.set_nonblocking(false);
                let _ = s.set_nodelay(true);
                // each connection costs a read per pass of the frame loop, one that does not wait (`net::read_now`)
                if self.clients.len() < MAX_CLIENTS {
                    self.clients.push(Client { s, buf: Vec::new() });
                }
            }
        }
        let mut i = 0;
        while i < self.clients.len() {
            let mut tmp = [0u8; 4096];
            let mut closed = false;
            loop {
                match crate::net::read_now(&self.clients[i].s, &mut tmp) {
                    Ok(0) => {
                        closed = true;
                        break;
                    }
                    Ok(n) => self.clients[i].buf.extend_from_slice(&tmp[..n]),
                    Err(e) if crate::net::later(&e) => break,
                    Err(_) => {
                        closed = true;
                        break;
                    }
                }
            }
            'lines: while let Some(p) = self.clients[i].buf.iter().position(|&c| c == b'\n') {
                let line: Vec<u8> = self.clients[i].buf.drain(..=p).collect();
                let line = String::from_utf8_lossy(&line).trim().to_string();
                for cmd in line.split(';') {
                    if let Some(resp) = self.exec(ui, cv, cmd.trim()) {
                        let mut out = resp;
                        out.push(b'\n');
                        if !send(&mut self.clients[i].s, &out) {
                            closed = true;
                            break 'lines;
                        }
                    }
                }
            }
            closed |= self.clients[i].buf.len() > MAX_LINE;
            if closed {
                self.clients.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// One program message (commands separated by ';'): the responses joined by ';' (empty without a query).
    pub fn exec_line(&mut self, ui: &mut Ui, cv: &Canvas, line: &str) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for cmd in line.split(';') {
            if out.len() >= RESP_MAX {
                self.err.push("-363,\"Input buffer overrun\" rest of the line dropped".into());
                break;
            }
            if let Some(r) = self.exec(ui, cv, cmd.trim()) {
                if !out.is_empty() {
                    out.push(b';');
                }
                out.extend_from_slice(&r);
            }
        }
        out
    }

    fn exec(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
        let errs = self.err.len();
        let r = self.exec_cmd(ui, cv, cmd);
        // a new error latches its class in the event status register (-1xx command, -2xx execution, -3xx device, -4xx query)
        for e in self.err.iter().skip(errs) {
            self.esr |= match e.trim_start_matches('-').chars().next() {
                Some('1') => 32,
                Some('2') => 16,
                Some('3') => 8,
                Some('4') => 4,
                _ => 0,
            };
        }
        // a client that never asks for its errors, or sends megabytes of nonsense, must not fill the memory
        for e in &mut self.err {
            if e.len() > ERR_LEN {
                *e = e.chars().take(ERR_LEN).collect();
            }
        }
        if self.err.len() > ERR_MAX {
            self.err.truncate(ERR_MAX - 1);
            self.err.push("-350,\"Queue overflow\"".into());
        }
        r
    }

    fn exec_cmd(&mut self, ui: &mut Ui, cv: &Canvas, cmd: &str) -> Option<Vec<u8>> {
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
                return t(&format!("OOS,{},{},oos-ui {}", p.caps.name, p.serial.as_deref().unwrap_or("0"), env!("CARGO_PKG_VERSION")));
            }
            "*OPC" if query => return t("1"),
            "*OPC" => {
                self.esr |= 1;
                return None;
            }
            "*ESR" if query => {
                let v = self.esr;
                self.esr = 0;
                return t(&v.to_string());
            }
            "*CLS" => {
                self.err.clear();
                self.esr = 0;
                return None;
            }
            "*RST" => {
                self.esr = 0;
                ui.key(crate::ui::key::DEFAULT);
                return None;
            }
            _ => {}
        }
        let nodes: Vec<&str> = head.trim_start_matches(':').split(':').collect();
        if nodes.len() >= 2 && kw(nodes[0], "LA") {
            return self.la_cmd(ui, &nodes[1..], query, arg);
        }
        if nodes.len() == 1 && kw(nodes[0], "AUToscale") && query {
            return t(if ui.auto_running() { "1" } else { "0" });
        }
        if nodes.len() == 1 && kw(nodes[0], "AUToscale") {
            ui.key(crate::ui::key::AUTO);
            return None;
        }
        if nodes.len() == 1 && kw(nodes[0], "CLEar") {
            ui.key(crate::ui::key::CLEAR);
            return None;
        }
        let m = &mut ui.m;
        let r: Option<Option<Vec<u8>>> = (|| {
            let n0 = nodes.first().copied().unwrap_or("");
            let (k0, sfx) = suffix(n0);
            if nodes.len() == 1 {
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
            if kw(k0, "CHANnel") && nodes.len() == 2 {
                let c = sfx?.checked_sub(1).filter(|&c| c < NCH)?;
                let n1 = nodes[1];
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
                    ch.vscale = nearest(&VSCALES, num(arg)? / ch.probe);
                    ch.vfine = 1.0;
                } else if kw(n1, "OFFSet") {
                    if query {
                        return Some(t(&e(ch.offset as f64)));
                    }
                    ch.offset = num(arg)?;
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
                        _ => BWS.iter().position(|b| b.0.replace(' ', "").to_ascii_uppercase() == a || b.0.replace(' ', "").to_ascii_uppercase() == format!("{}HZ", a))?,
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
                } else {
                    return None;
                }
                return Some(None);
            }
            if kw(n0, "TIMebase") {
                let last = *nodes.last()?;
                if kw(last, "SCALe") {
                    if query {
                        return Some(t(&e(m.time_per_div() as f64)));
                    }
                    m.tscale = nearest(&tscales(), num(arg)?);
                    m.tfine = 1.0;
                    return Some(None);
                }
                if kw(last, "OFFSet") || kw(last, "POSition") {
                    if query {
                        return Some(t(&e(m.delay as f64)));
                    }
                    m.delay = num(arg)?;
                    return Some(None);
                }
                return None;
            }
            if kw(n0, "TRIGger") {
                let last = *nodes.last()?;
                if nodes.len() == 2 && kw(last, "MODE") {
                    return Some(if query { t("EDGE") } else { None });
                }
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
                    m.holdoff = num(arg).filter(|&h| (0.0..=10.0).contains(&h))? as f32;
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
                    m.trig_mode = if a.starts_with("AUTO") { TrigMode::Auto } else if a.starts_with("NORM") { TrigMode::Normal } else if a.starts_with("SING") { TrigMode::Single } else { return None };
                    return Some(None);
                }
                if nodes.len() == 3 && kw(nodes[1], "EDGE") {
                    if kw(last, "SOURce") {
                        if query {
                            return Some(t(&m.trig_line().map_or(format!("CHAN{}", m.trig_src + 1), |n| format!("D{}", n))));
                        }
                        // D0..D15 like Rigol's source names, with the LA on
                        match arg.trim().to_ascii_uppercase().strip_prefix('D').and_then(|n| n.parse::<u8>().ok()).filter(|&n| n < 16 && m.trig_la_ok()) {
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
                        m.slope = if a.starts_with("POS") { Slope::Rise } else if a.starts_with("NEG") { Slope::Fall } else if a.starts_with("RFAL") { Slope::Either } else { return None };
                    } else {
                        return None;
                    }
                    return Some(None);
                }
                return None;
            }
            if kw(n0, "COUNter") && nodes.len() == 2 && !kw(nodes[1], "CURRent") {
                let k = nodes[1];
                if query {
                    return Some(t(&if kw(k, "ENABle") {
                        (m.cnt_on as u8).to_string()
                    } else if kw(k, "SOURce") {
                        format!("CHAN{}", m.cnt_src + 1)
                    } else if kw(k, "MODE") {
                        ["FREQ", "PER", "TOT"][m.cnt_mode.min(2)].to_string()
                    } else if kw(k, "NDIGits") {
                        m.cnt_digits.to_string()
                    } else {
                        return None;
                    }));
                }
                let a = arg.trim().to_ascii_uppercase();
                if kw(k, "ENABle") {
                    m.cnt_on = parse_bool(arg)?;
                } else if kw(k, "SOURce") {
                    m.cnt_src = parse_ch(arg).filter(|&c| c < n_ch())?;
                } else if kw(k, "MODE") {
                    m.cnt_mode = ["FREQ", "PER", "TOT"].iter().position(|p| a.starts_with(p))?;
                } else if kw(k, "NDIGits") {
                    m.cnt_digits = a.parse::<u32>().ok().filter(|d| (3..=6).contains(d))?;
                } else {
                    return None;
                }
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
            if kw(n0, "ACQuire") && nodes.len() == 2 && query {
                let (sr, depth) = m.srate_depth();
                if kw(nodes[1], "SRATe") {
                    return Some(t(&e(sr)));
                }
                if kw(nodes[1], "MDEPth") {
                    return Some(t(&e(depth as f64)));
                }
                return None;
            }
            None
        })();
        if let Some(resp) = r {
            ui.mark_changed();
            return resp;
        }
        // measurement, waveform, display, system (need the record / canvas)
        let n0 = nodes.first().copied().unwrap_or("");
        if kw(n0, "COUNter") && nodes.len() == 2 && kw(nodes[1], "CURRent") && query {
            return t(&ui.counter_value().map_or("9.9E37".into(), e));
        }
        if kw(n0, "COUNter") && nodes.len() == 3 && kw(nodes[1], "TOTalize") && kw(nodes[2], "CLEar") && !query {
            ui.counter_reset();
            return None;
        }
        if kw(n0, "MEASure") && nodes.len() == 2 && kw(nodes[1], "ITEM") && query {
            let mut parts = arg.split(',');
            let item = parts.next().unwrap_or("").trim().to_ascii_uppercase();
            let c = parts.next().and_then(parse_ch).unwrap_or(0);
            let k = match item.as_str() {
                "VPP" => Meas::Vpp,
                "VMAX" => Meas::Vmax,
                "VMIN" => Meas::Vmin,
                "VAVG" => Meas::Vavg,
                "VRMS" => Meas::Vrms,
                "VAMP" => Meas::Vamp,
                "FREQ" | "FREQUENCY" => Meas::Freq,
                "PER" | "PERIOD" => Meas::Period,
                "RTIM" | "RTIME" => Meas::Rise,
                "FTIM" | "FTIME" => Meas::Fall,
                "PWID" | "PWIDTH" => Meas::PWidth,
                "PDUT" | "PDUTY" => Meas::PDuty,
                "NDUT" | "NDUTY" => Meas::NDuty,
                "NWID" | "NWIDTH" => Meas::NWidth,
                "VTOP" => Meas::Vtop,
                "VBAS" | "VBASE" => Meas::Vbase,
                "OVER" | "OVERSHOOT" => Meas::Overshoot,
                "PRES" | "PRESHOOT" => Meas::Preshoot,
                _ => {
                    self.err.push("-224,\"Illegal parameter value\"".into());
                    return t("9.9E37");
                }
            };
            let v = ui.meas(c, k);
            return t(&if v.is_finite() { e(v) } else { "9.9E37".into() });
        }
        if kw(n0, "WAVeform") && nodes.len() == 2 {
            let n1 = nodes[1];
            if kw(n1, "SOURce") {
                if query {
                    return t(&format!("CHAN{}", self.wav_src + 1));
                }
                if let Some(c) = parse_ch(arg) {
                    self.wav_src = c;
                }
                return None;
            }
            if kw(n1, "MODE") {
                if query {
                    return t(if self.wav_raw { "RAW" } else { "NORM" });
                }
                self.wav_raw = arg.to_ascii_uppercase().starts_with("RAW");
                return None;
            }
            if kw(n1, "FORMat") {
                if query {
                    return t(["BYTE", "WORD", "ASC"][self.wav_fmt as usize]);
                }
                let a = arg.to_ascii_uppercase();
                self.wav_fmt = if a.starts_with("WORD") { 1 } else if a.starts_with("ASC") { 2 } else { 0 };
                return None;
            }
            if kw(n1, "STARt") || kw(n1, "STOP") {
                let stop = kw(n1, "STOP");
                if query {
                    let n = self.wave_len(ui);
                    return t(&(if stop { if self.wav_stop == 0 { n } else { self.wav_stop } } else { self.wav_start }).to_string());
                }
                let Some(p) = arg.trim().parse::<usize>().ok().filter(|&p| p >= 1) else {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                    return None;
                };
                if stop {
                    self.wav_stop = p;
                    self.wav_start = self.wav_start.min(p);
                } else {
                    self.wav_start = p;
                    if self.wav_stop != 0 {
                        self.wav_stop = self.wav_stop.max(p);
                    }
                }
                return None;
            }
            let (pts, pre) = self.wave_params(ui);
            if kw(n1, "POINts") && query {
                return t(&format!("{}", pts));
            }
            if kw(n1, "PREamble") && query {
                return t(&pre.0);
            }
            if kw(n1, "DATA") && query {
                return Some(self.wave_data(ui, pre.1));
            }
        }
        if kw(n0, "DISPlay") && nodes.len() == 2 && kw(nodes[1], "DATA") && query {
            return Some(block(&bmp(cv)));
        }
        // Display > Views (ui/views.rs): :DISPlay:VIEW <n> shows view n, :DISPlay:VIEW:COUNt <1..4> how many there are
        if kw(n0, "DISPlay") && nodes.len() >= 2 && kw(nodes[1], "VIEW") {
            let count = nodes.len() == 3 && kw(nodes[2], "COUNt");
            if nodes.len() > 3 || (nodes.len() == 3 && !count) {
                self.err.push("-113,\"Undefined header\"".into());
                return None;
            }
            if query {
                return t(&(if count { ui.m.views } else { ui.m.view_cur + 1 }).to_string());
            }
            let n = arg.trim().parse::<usize>().ok();
            match n {
                Some(n) if count && (1..=crate::ui::VIEWS_MAX).contains(&n) => ui.views_set(n),
                Some(n) if !count && (1..=ui.m.views).contains(&n) => ui.view_go(n - 1),
                _ => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
            }
            return None;
        }
        // OOS extensions: analyser view, Identify, self-calibration, Bode
        if kw(n0, "VIEW") && nodes.len() == 1 {
            const VIEWS: [(&str, AView); 5] = [("SCOPe", AView::Scope), ("SPECtrum", AView::Spectrum), ("EYE", AView::Eye), ("POWer", AView::Power), ("BODE", AView::Bode)];
            if query {
                return t(&format!("{:?}", ui.m.view).to_ascii_uppercase());
            }
            match VIEWS.iter().find(|(k, _)| kw(arg, k)) {
                Some((_, v)) => ui.m.view = *v,
                None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
            if kw(nodes[1], "STARt") {
                let mut it = arg.split(',');
                let ch = it.next().unwrap_or("0").trim();
                ui.selfcal_remote(ch, it.next().is_some_and(|z| kw(z.trim(), "ZERO")));
                return None;
            }
            if kw(nodes[1], "CANCel") {
                ui.selfcal_remote("cancel", false);
                return None;
            }
        }
        if kw(n0, "BODE") && nodes.len() == 2 {
            if kw(nodes[1], "RUN") {
                ui.bode_run(parse_bool(arg).unwrap_or(true));
                return None;
            }
            if kw(nodes[1], "DATA") && query {
                return t(&ui.bode_text());
            }
            if kw(nodes[1], "GENerator") {
                // "usb": the generator on the USB-A port; anything else is its LAN address (kept while on USB)
                if query {
                    return t(if ui.m.bode_usb { "usb" } else { ui.m.bode_addr.as_str() });
                }
                // one word, like the setup file keeps it
                let a = arg.trim().trim_matches('"');
                if a.contains(char::is_whitespace) {
                    self.err.push(format!("-224,\"Illegal parameter value\" {}", arg));
                } else if a.eq_ignore_ascii_case("usb") {
                    ui.m.bode_usb = true;
                } else {
                    (ui.m.bode_addr, ui.m.bode_usb) = (a.to_string(), false);
                }
                return None;
            }
        }
        if suffix(n0).1.is_none_or(|n| (1..=gen_count().max(1)).contains(&n)) && kw(suffix(n0).0, "SOURce") && nodes.len() >= 2 {
            if !oos_hal::model::platform().caps.has_gen() {
                self.err.push(format!("-241,\"Hardware missing\" no generator on the {}", oos_hal::model::platform().caps.name));
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
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        return t(&SHAPES[m.afg_mod_shape.min(5)].chars().filter(|c| c.is_ascii_uppercase()).collect::<String>());
                    }
                    match SHAPES.iter().position(|s| kw(a, s)) {
                        Some(i) => ui.m.afg_mod_shape = i,
                        None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                        None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            // :FUNCtion:SQUare:DUTY / :FUNCtion:PULSe:DUTY and :FUNCtion:RAMP:SYMMetry in %
            // arbitrary wave: :ARB:DATA <v>,<v>,... (one period, any scale), :ARB:CAPTure CHAN<n> (the screen of a channel)
            if has("ARB") {
                if has("DATA") && !query {
                    let v: Vec<f32> = arg.split(',').filter_map(num).collect();
                    if !ui.afg_arb_load(&v) {
                        self.err.push(format!("-224,\"Illegal parameter value\" {} samples", v.len()));
                    }
                } else if has("CAPTure") && !query {
                    match parse_ch(arg).ok_or("no channel".to_string()).and_then(|c| ui.afg_arb_capture(c)) {
                        Ok(_) => {}
                        Err(e) => self.err.push(format!("-224,\"Illegal parameter value\" {}", e)),
                    }
                } else {
                    self.err.push("-113,\"Undefined header\" :SOURce:ARB".into());
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
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
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
                    Some(v) => ui.afg_set(None, (want == 0).then_some(v), (want == 1).then_some(v), (want == 2).then_some(v)),
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
        }
        // test access: any key of a setup file on the live settings (`:SYST:SET ch1.bw = 20M`), its value, the state
        if kw(n0, "SYSTem") && nodes.len() == 2 {
            if kw(nodes[1], "SET") && !query {
                let r = arg.split_once('=').ok_or("no '='".to_string()).and_then(|(k, v)| crate::store::set(&mut ui.m, k.trim(), v.trim()));
                match r {
                    Ok(()) => ui.mark_changed(),
                    Err(e) => self.err.push(format!("-224,\"Illegal parameter value\" {}", e)),
                }
                // a setup file may hold keys of newer versions, here a typo must not pass for a setting; the keys that
                // are only written when they differ from their default do not show
                let k = arg.split('=').next().unwrap_or("").trim();
                let optional = ["vfine", "unit", "bias", "skew", "label", "tfine"].iter().any(|s| k.ends_with(s)) || k.starts_with("math_expr");
                if !optional && !crate::store::setup_text(&ui.m).lines().any(|l| l.split('=').next().unwrap_or("").trim() == k) {
                    self.err.push(format!("-224,\"Illegal parameter value\" unknown key {}", k));
                }
                return None;
            }
            if kw(nodes[1], "GET") && query {
                let text = crate::store::setup_text(&ui.m);
                let v = text.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == arg.trim()).map(|x| x.1.trim().to_string()));
                return t(&v.unwrap_or_default());
            }
            if kw(nodes[1], "STATe") && query {
                return t(crate::cmd::stat_text(ui).trim());
            }
            // System > Graphics: CPU or GPU (whether the GPU really draws: the `gpu` part of :SYSTem:STATe?)
            if kw(nodes[1], "GRAPhics") {
                if query {
                    return t(if ui.sys.gpu { "GPU" } else { "CPU" });
                }
                match arg.trim().to_ascii_uppercase().as_str() {
                    "GPU" => ui.graphics_set(true),
                    "CPU" => ui.graphics_set(false),
                    _ => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
            if kw(nodes[1], "LOG") && !query {
                match parse_bool(arg) {
                    Some(on) => ui.log_set(on),
                    None => self.err.push(format!("-224,\"Illegal parameter value\" {}", arg)),
                }
                return None;
            }
        }
        if kw(n0, "DECode") && nodes.len() == 2 && kw(nodes[1], "EVENts") && query {
            return t(&ui.events_text());
        }
        if kw(n0, "SYSTem") && nodes.len() == 2 && kw(nodes[1], "ERRor") && query {
            return t(&if self.err.is_empty() { "0,\"No error\"".to_string() } else { self.err.remove(0) });
        }
        self.err.push(format!("-113,\"Undefined header\" {}", cmd));
        None
    }

    /// The STARt..STOP page of a record of `n` points as an index range (empty when STARt is past the end).
    fn page(&self, n: usize) -> (usize, usize) {
        let a = (self.wav_start.max(1) - 1).min(n);
        let b = if self.wav_stop == 0 { n } else { self.wav_stop.clamp(a, n) };
        (a, b)
    }

    /// The number of points in the record of the current source and mode, before paging.
    fn wave_len(&self, ui: &Ui) -> usize {
        let info = if self.wav_raw { ui.raw_info(self.wav_src) } else { None };
        info.or_else(|| ui.record_info(self.wav_src)).map_or(0, |i| i.0)
    }

    /// (points, (preamble string, (yinc, yorigin, yref)))
    fn wave_params(&self, ui: &Ui) -> (usize, (String, (f64, f64, f64))) {
        let c = self.wav_src;
        let ch = &ui.m.ch[c];
        let vpd = ch.volts_per_div() as f64;
        let info = if self.wav_raw { ui.raw_info(c) } else { None };
        let (n, dt, t0) = info.or_else(|| ui.record_info(c)).unwrap_or((0, 0.0, 0.0));
        let (a, b) = self.page(n);
        let (pts, t0) = (b - a, t0 + a as f64 * dt);
        let (yinc, yref) = match self.wav_fmt {
            1 => (vpd * VDIV as f64 / 60000.0, 32768.0),
            _ => (vpd * VDIV as f64 / 250.0, 128.0),
        };
        let yorg = (ch.offset as f64 / yinc).round();
        let pre = format!("{},0,{},1,{},{},0,{},{},{}", self.wav_fmt, pts, e(dt), e(t0), e(yinc), yorg, yref);
        (pts, (pre, (yinc, yorg, yref)))
    }

    /// `:LA:...` (Rigol's names where they exist): state, lines shown, pod thresholds, and the record of the last
    /// acquisition: `:LA:DATA?` is an IEEE block of u16 words, little endian, bit n = Dn, sample k at
    /// t0 + k dt from `:LA:PREamble?` (points,dt,t0).
    fn la_cmd(&mut self, ui: &mut Ui, n: &[&str], query: bool, arg: &str) -> Option<Vec<u8>> {
        use crate::ui::la;
        let t = |s: String| Some(s.into_bytes());
        if !la::available() {
            self.err.push(format!("-241,\"Hardware missing\" no logic analyser port on the {}", oos_hal::model::platform().caps.name));
            return None;
        }
        let bad = |e: &mut Vec<String>| {
            e.push(format!("-224,\"Illegal parameter value\" {}", arg));
            None
        };
        let (k, idx) = suffix(n[0]);
        if n.len() == 1 && (kw(n[0], "STATe") || kw(n[0], "ENABle")) {
            if query {
                return t(if ui.m.la.on { "1" } else { "0" }.into());
            }
            match parse_bool(arg) {
                Some(on) => {
                    ui.m.la.on = on;
                    ui.mark_changed();
                    return None;
                }
                None => return bad(&mut self.err),
            }
        }
        if n.len() == 2 && kw(k, "DIGital") && kw(n[1], "DISPlay") {
            let Some(line) = idx.filter(|&i| i < 16) else { return bad(&mut self.err) };
            if query {
                return t((ui.m.la.show >> line & 1).to_string());
            }
            match parse_bool(arg) {
                Some(on) => {
                    ui.m.la.show = ui.m.la.show & !(1 << line) | (on as u16) << line;
                    return None;
                }
                None => return bad(&mut self.err),
            }
        }
        if n.len() == 2 && kw(k, "POD") && kw(n[1], "THReshold") {
            let Some(pod) = idx.filter(|&i| i == 1 || i == 2).map(|i| i - 1) else { return bad(&mut self.err) };
            if query {
                return t(format!("{:e}", ui.m.la.thr(pod)));
            }
            match arg.trim().parse::<f32>() {
                Ok(v) if v.abs() <= la::THR_MAX => {
                    (ui.m.la.preset[pod], ui.m.la.user[pod]) = (la::USER, v);
                    ui.mark_changed();
                    return None;
                }
                _ => return bad(&mut self.err),
            }
        }
        if n.len() == 1 && query && (kw(n[0], "POINts") || kw(n[0], "PREamble") || kw(n[0], "DATA")) {
            let Some(r) = ui.la_record() else {
                self.err.push("-230,\"Data corrupt or stale\" no LA record".into());
                return if kw(n[0], "DATA") { Some(block(&[])) } else { t("0".into()) };
            };
            if kw(n[0], "POINts") {
                return t(r.words.len().to_string());
            }
            if kw(n[0], "PREamble") {
                return t(format!("{},{:e},{:e}", r.words.len(), r.dt, r.t0));
            }
            return Some(block(&r.words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()));
        }
        self.err.push(format!("-113,\"Undefined header\" :LA:{}", n.join(":")));
        None
    }

    fn wave_data(&self, ui: &Ui, p: (f64, f64, f64)) -> Vec<u8> {
        let (yinc, yorg, yref) = p;
        let v = if self.wav_raw { ui.raw_volts(self.wav_src).unwrap_or_else(|| ui.record_volts(self.wav_src)) } else { ui.record_volts(self.wav_src) };
        let (a, b) = self.page(v.len());
        let v = &v[a..b];
        match self.wav_fmt {
            2 => {
                let s: Vec<String> = v.iter().map(|&x| e(x as f64)).collect();
                block(s.join(",").as_bytes())
            }
            1 => {
                let mut b = Vec::with_capacity(v.len() * 2);
                for &x in v {
                    let code = ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 65535.0) as u16;
                    b.extend_from_slice(&code.to_le_bytes());
                }
                block(&b)
            }
            _ => {
                let b: Vec<u8> = v.iter().map(|&x| ((x as f64 / yinc) + yorg + yref).round().clamp(0.0, 255.0) as u8).collect();
                block(&b)
            }
        }
    }
}

/// IEEE 488.2 definite-length block: #9 + 9 digits + data.
fn block(d: &[u8]) -> Vec<u8> {
    let mut o = format!("#9{:09}", d.len()).into_bytes();
    o.extend_from_slice(d);
    o
}

/// 24-bit BMP of the canvas (what :DISPlay:DATA? returns on the stock firmware).
pub fn bmp(cv: &Canvas) -> Vec<u8> {
    let (w, h) = (cv.w as usize, cv.h as usize);
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    for y in (0..h).rev() {
        for x in 0..w {
            let p = cv.px[y * w + x];
            b.extend_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
        b.resize(b.len() + row - w * 3, 0);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::Source;

    fn setup() -> (Scpi, Ui, Canvas) {
        let mut ui = Ui::new(Source::Demo { t: 0.0 });
        let cv = Canvas::new(crate::ui::PANEL_W, crate::ui::PANEL_H);
        for _ in 0..5 {
            ui.step(0.04);
        }
        (Scpi::open(0), ui, cv)
    }

    fn line(s: &mut Scpi, ui: &mut Ui, cv: &Canvas, l: &str) -> String {
        String::from_utf8_lossy(&s.exec_line(ui, cv, l)).into_owned()
    }

    #[test]
    fn esr_returns_an_integer_and_clears_on_read() {
        let (mut s, mut ui, cv) = setup();
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
        line(&mut s, &mut ui, &cv, "*OPC");
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?;*ESR?"), "1;0");
        // an undefined header is a command error (bit 5), an illegal value an execution error (bit 4)
        line(&mut s, &mut ui, &cv, ":NOPE:NOPE");
        line(&mut s, &mut ui, &cv, ":WAVeform:STARt 0");
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "48");
        line(&mut s, &mut ui, &cv, ":NOPE");
        line(&mut s, &mut ui, &cv, "*CLS");
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
        // reading the error queue does not latch the register again
        line(&mut s, &mut ui, &cv, ":NOPE");
        line(&mut s, &mut ui, &cv, "*ESR?");
        line(&mut s, &mut ui, &cv, ":SYSTem:ERRor?");
        assert_eq!(line(&mut s, &mut ui, &cv, "*ESR?"), "0");
    }

    #[test]
    fn waveform_start_stop_page_the_data() {
        let (mut s, mut ui, cv) = setup();
        line(&mut s, &mut ui, &cv, ":WAVeform:FORMat ASCii");
        let n: usize = line(&mut s, &mut ui, &cv, ":WAVeform:POINts?").parse().unwrap();
        assert!(n > 10);
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:STARt?"), "1");
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:STOP?"), n.to_string());
        let all = line(&mut s, &mut ui, &cv, ":WAVeform:DATA?");
        let all: Vec<&str> = all[11..].split(',').collect();
        assert_eq!(all.len(), n);
        let pre0 = line(&mut s, &mut ui, &cv, ":WAVeform:PREamble?");
        line(&mut s, &mut ui, &cv, ":WAVeform:STARt 3");
        line(&mut s, &mut ui, &cv, ":WAVeform:STOP 7");
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:POINts?"), "5");
        let page = line(&mut s, &mut ui, &cv, ":WAVeform:DATA?");
        assert_eq!(page[11..].split(',').collect::<Vec<_>>(), all[2..7]);
        // the preamble describes the page: its point count and the time of its first point
        let (p0, p1) = (pre0.split(',').collect::<Vec<_>>(), line(&mut s, &mut ui, &cv, ":WAVeform:PREamble?"));
        let p1: Vec<&str> = p1.split(',').collect();
        assert_eq!(p1[2], "5");
        let (dt, t0, t0p): (f64, f64, f64) = (p0[4].parse().unwrap(), p0[5].parse().unwrap(), p1[5].parse().unwrap());
        assert!((t0p - (t0 + 2.0 * dt)).abs() < dt * 1e-3);
        // STOP below STARt pulls STARt down; STARt above STOP pushes STOP up; a bad value changes nothing
        line(&mut s, &mut ui, &cv, ":WAVeform:STOP 2");
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:STARt?"), "2");
        line(&mut s, &mut ui, &cv, ":WAVeform:STARt 9");
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:STOP?"), "9");
        line(&mut s, &mut ui, &cv, ":WAVeform:STARt abc");
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:STARt?"), "9");
        assert!(line(&mut s, &mut ui, &cv, ":SYSTem:ERRor?").starts_with("-224"));
        // a page past the end is empty, STOP past the end is cut to the record
        line(&mut s, &mut ui, &cv, &format!(":WAVeform:STARt {}", n + 5));
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:POINts?"), "0");
        line(&mut s, &mut ui, &cv, &format!(":WAVeform:STARt {};:WAVeform:STOP {}", n - 1, n + 100));
        assert_eq!(line(&mut s, &mut ui, &cv, ":WAVeform:POINts?"), "2");
        // BYTE data is one byte per point of the page
        line(&mut s, &mut ui, &cv, ":WAVeform:FORMat BYTE");
        assert_eq!(&line(&mut s, &mut ui, &cv, ":WAVeform:DATA?")[..11], "#9000000002");
    }
}
